use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures::stream::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use log::{error, info};
use uuid::Uuid;
use quick_xml::events::Event;
use quick_xml::Reader;
use serde_json::Value;
use reqwest::Client;
use tokio::io::AsyncBufReadExt;

use crate::stream_manager::StreamManager;

pub struct StreamingServer {
    host: String,
    port: u16,
    stream_manager: Arc<RwLock<StreamManager>>,
}

// --- HLS session tracking for inactivity timeout ---
struct HlsSession {
    tmp_dir: String,
    rtsp_url: String,
    last_access: Instant,
    shutdown: mpsc::Sender<()>,
}

static HLS_SESSIONS: Lazy<Arc<RwLock<HashMap<String, HlsSession>>>> = Lazy::new(|| {
    Arc::new(RwLock::new(HashMap::new()))
});

const HLS_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
struct StartStreamRequest {
    rtsp_url: String,
}

#[derive(Serialize)]
struct ApiResponse {
    success: bool,
    message: String,
}

#[derive(Serialize)]
struct StreamListResponse {
    streams: Vec<String>,
}

#[derive(Deserialize)]
struct ProxyCamerasQuery {
    ip: String,
    port: Option<String>,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Deserialize)]
struct ProxyRtspQuery {
    ip: String,
    port: Option<String>,
    username: Option<String>,
    password: Option<String>,
    channel: Option<String>,
    stream_number: Option<String>,
}

#[derive(Deserialize)]
struct ProxyHlsRtspQuery {
    ip: String,
    port: Option<String>,
    username: Option<String>,
    password: Option<String>,
    channel: Option<String>,
    stream_number: Option<String>,
}

#[derive(Serialize)]
struct ChannelInfo {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct ChannelListResponse {
    channels: Vec<ChannelInfo>,
}

// --- Client-side (browser) logging ---
#[derive(Deserialize)]
struct ClientLogRequest {
    level: String,
    message: String,
}

async fn client_log(Json(payload): Json<ClientLogRequest>) -> impl IntoResponse {
    match payload.level.to_lowercase().as_str() {
        "error" => error!("[browser] {}", payload.message),
        "warn" => log::warn!("[browser] {}", payload.message),
        "debug" => log::debug!("[browser] {}", payload.message),
        _ => info!("[browser] {}", payload.message),
    }
    StatusCode::OK
}

impl StreamingServer {
    pub fn new(host: String, port: u16, stream_manager: Arc<RwLock<StreamManager>>) -> Self {
        Self {
            host,
            port,
            stream_manager,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let app = Router::new()
            .route("/", get(root_handler))
            .route("/api/streams", get(list_streams))
            .route("/api/stream/:id/start", post(start_stream))
            .route("/api/stream/:id/stop", post(stop_stream))
            .route("/stream/:id/mpegts", get(stream_mpegts))
            .route("/stream", get(direct_stream))
            .route("/stream/hls", get(stream_hls_direct))
            .route("/stream/hls/:id/playlist.m3u8", get(stream_hls_session_playlist))
            .route("/stream/hls/:id/:file", get(stream_hls_session_segment))
            .route("/player", get(player_page))
            .route("/stream/:id/hls/playlist.m3u8", get(stream_hls_playlist))
            .route("/stream/:id/hls/:segment", get(stream_hls_segment))
            .route("/proxy/cameras", get(proxy_cameras))
            .route("/proxy/rtsp", get(proxy_rtsp))
            .route("/proxyhl/rtsp", get(proxy_hls_rtsp))
            .route("/proxyhl/sessions", get(list_proxyhl_sessions))
            .route("/proxyhl/segment/:id/:file", get(proxy_hls_segment))
            .route("/api/client-log", post(client_log))
            .layer(CorsLayer::permissive())
            .with_state(self.stream_manager);

        let addr = format!("{}:{}", self.host, self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        info!("Server listening on http://{}", addr);
        info!("API endpoints:");
        info!("  GET /player?rtsp_url=<url> - Play stream in browser");
        info!("  GET /stream?rtsp_url=<url> - Stream directly from RTSP URL (for VLC/ffplay)");
        info!("  POST /api/stream/:id/start - Start a stream (form: rtsp_url)");
        info!("  POST /api/stream/:id/stop - Stop a stream");
        info!("  GET /api/streams - List all streams");
        info!("  GET /stream/:id/mpegts - Get MPEG-TS stream");
        info!("  GET /stream/:id/hls/playlist.m3u8 - Get HLS playlist");
        info!("  GET /proxyhl/rtsp - HLS playlist from Hikvision RTSP");
        info!("  GET /proxyhl/sessions - List active HLS sessions");
        info!("  POST /api/client-log - Browser console logs forwarded here");

        axum::serve(listener, app).await?;

        Ok(())
    }
}

/// Spawn a task that reads FFmpeg's stderr line by line and logs it
fn log_ffmpeg_stderr(stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stderr);
        let mut line = String::new();
        loop {
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    error!("[ffmpeg] {}", line.trim());
                    line.clear();
                }
                Err(e) => {
                    error!("[ffmpeg] stderr read error: {}", e);
                    break;
                }
            }
        }
    });
}

async fn root_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": "RTSP Proxy Server",
        "version": "0.1.0",
        "endpoints": {
            "player": "GET /player?rtsp_url=<url>&speed=<1.0-4.0> - Play HLS stream in browser with speed control",
            "direct_stream": "GET /stream?rtsp_url=<url> - Stream directly from RTSP URL (for VLC/ffplay)",
            "hls_stream": "GET /stream/hls?rtsp_url=<url>&speed=<0.5-4.0> - Create HLS session from RTSP URL with fast-forward support",
            "hls_playlist": "GET /stream/hls/{id}/playlist.m3u8 - Get HLS playlist for session",
            "hls_segment": "GET /stream/hls/{id}/{file} - Get HLS segment",
            "api_streams": "GET /api/streams - List all managed streams",
            "api_start_stream": "POST /api/stream/:id/start?rtsp_url=<url> - Start managed stream",
            "api_stop_stream": "POST /api/stream/:id/stop - Stop managed stream",
            "stream_mpegts": "GET /stream/:id/mpegts - Get MPEG-TS stream (managed)",
            "stream_hls_managed": "GET /stream/:id/hls/playlist.m3u8 - Get HLS playlist (managed)",
            "proxy_cameras": "GET /proxy/cameras?ip=<ip> - List cameras from Hikvision NVR",
            "proxy_rtsp": "GET /proxy/rtsp?ip=<ip>&channel=<ch> - Get MJPEG stream from Hikvision",
            "proxy_hls_rtsp": "GET /proxyhl/rtsp?ip=<ip>&channel=<ch> - Create HLS session from Hikvision",
            "proxyhl_playlist": "GET /proxyhl/segment/{id}/playlist.m3u8 - Get HLS playlist (Hikvision)",
            "proxyhl_segment": "GET /proxyhl/segment/{id}/{file} - Get HLS segment (Hikvision)",
            "proxyhl_sessions": "GET /proxyhl/sessions - List all active HLS sessions (both endpoints)",
            "client_log": "POST /api/client-log - Forward browser console logs to server log file"
        },
        "examples": {
            "browser_hls": "http://localhost:8080/player?rtsp_url=rtsp://user:pass@camera-ip:554/stream",
            "browser_hls_2x": "http://localhost:8080/player?rtsp_url=rtsp://user:pass@camera-ip:554/stream&speed=2.0",
            "vlc_direct": "vlc http://localhost:8080/stream?rtsp_url=rtsp://user:pass@camera-ip:554/stream",
            "hls_generic": "http://localhost:8080/stream/hls?rtsp_url=rtsp://user:pass@camera-ip:554/stream",
            "hls_fastforward": "http://localhost:8080/stream/hls?rtsp_url=rtsp://user:pass@camera-ip:554/stream&speed=2.0",
            "hikvision": "http://localhost:8080/proxyhl/rtsp?ip=192.168.1.100&channel=1"
        }
    }))
}

async fn list_streams(
    State(manager): State<Arc<RwLock<StreamManager>>>,
) -> impl IntoResponse {
    let manager = manager.read().await;
    let streams = manager.list_streams();

    Json(StreamListResponse { streams })
}

async fn start_stream(
    Path(id): Path<String>,
    maybe_query: Option<Query<StartStreamRequest>>,
    State(manager): State<Arc<RwLock<StreamManager>>>,
    body: String,
) -> impl IntoResponse {
    info!("Received request to start stream {}", id);

    let rtsp_url = if let Some(Query(params)) = maybe_query {
        params.rtsp_url
    } else {
        let s = body;
        let mut rtsp_url: Option<String> = None;
        for pair in s.split('&') {
            let mut parts = pair.splitn(2, '=');
            if let Some(key) = parts.next() {
                if key == "rtsp_url" {
                    let val = parts.next().unwrap_or("");
                    match urlencoding::decode(val) {
                        Ok(decoded) => {
                            rtsp_url = Some(decoded.into_owned());
                            break;
                        }
                        Err(_) => {
                            rtsp_url = Some(val.to_string());
                            break;
                        }
                    }
                }
            }
        }
        match rtsp_url {
            Some(v) => v,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ApiResponse {
                        success: false,
                        message: "Missing rtsp_url in query or form body".to_string(),
                    }),
                ).into_response();
            }
        }
    };

    let mut manager = manager.write().await;
    match manager.start_stream(id.clone(), rtsp_url).await {
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message: format!("Stream {} started", id),
            }),
        ).into_response(),
        Err(e) => {
            error!("Failed to start stream {}: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    success: false,
                    message: format!("Failed to start stream: {}", e),
                }),
            ).into_response()
        }
    }
}

async fn stop_stream(
    Path(id): Path<String>,
    State(manager): State<Arc<RwLock<StreamManager>>>,
) -> impl IntoResponse {
    info!("Received request to stop stream {}", id);

    let mut manager = manager.write().await;
    match manager.stop_stream(&id).await {
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message: format!("Stream {} stopped", id),
            }),
        ),
        Err(e) => {
            error!("Failed to stop stream {}: {}", id, e);
            (
                StatusCode::NOT_FOUND,
                Json(ApiResponse {
                    success: false,
                    message: format!("Failed to stop stream: {}", e),
                }),
            )
        }
    }
}

async fn stream_mpegts(
    Path(id): Path<String>,
    State(manager): State<Arc<RwLock<StreamManager>>>,
) -> Response {
    info!("MPEG-TS stream requested for {}", id);

    let manager = manager.read().await;
    let stream_info = match manager.get_stream(&id) {
        Some(info) => info,
        None => {
            return (
                StatusCode::NOT_FOUND,
                "Stream not found",
            ).into_response();
        }
    };

    let client = stream_info.client.read().await;
    let receiver = match client.get_data_receiver().await {
        Some(rx) => rx,
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get stream receiver",
            ).into_response();
        }
    };
    drop(client);
    drop(manager);

    let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(receiver)
        .map(|chunk| Ok::<_, std::io::Error>(chunk));
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "video/mp2t")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Content-Type-Options", "nosniff")
        .body(body)
        .unwrap()
}

async fn stream_hls_playlist(
    Path(id): Path<String>,
    State(manager): State<Arc<RwLock<StreamManager>>>,
) -> Response {
    info!("HLS playlist requested for {}", id);

    let manager = manager.read().await;
    if manager.get_stream(&id).is_none() {
        return (StatusCode::NOT_FOUND, "Stream not found").into_response();
    }

    let playlist = format!(
        "#EXTM3U\n\
         #EXT-X-VERSION:3\n\
         #EXT-X-TARGETDURATION:10\n\
         #EXT-X-MEDIA-SEQUENCE:0\n\
         #EXTINF:10.0,\n\
         /stream/{}/mpegts\n",
        id
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(playlist))
        .unwrap()
}

async fn stream_hls_segment(
    Path((id, segment)): Path<(String, String)>,
    State(manager): State<Arc<RwLock<StreamManager>>>,
) -> Response {
    info!("HLS segment {} requested for stream {}", segment, id);
    stream_mpegts(Path(id), State(manager)).await
}

#[derive(Deserialize)]
struct DirectStreamQuery {
    rtsp_url: String,
    #[serde(default)]
    speed: Option<f32>,
}

async fn direct_stream(
    Query(params): Query<DirectStreamQuery>,
) -> Response {
    if !params.rtsp_url.starts_with("rtsp://") && !params.rtsp_url.starts_with("rtsps://") {
        error!("Invalid RTSP URL format: {}", params.rtsp_url);
        return (
            StatusCode::BAD_REQUEST,
            format!(
                "Invalid RTSP URL: '{}'. URL must start with 'rtsp://' or 'rtsps://'. \
                 Make sure to properly URL-encode the rtsp_url parameter.",
                params.rtsp_url
            ),
        )
            .into_response();
    }

    info!("Direct stream requested for {}", params.rtsp_url);

    let mut child = match crate::ffmpeg_command()
        .args(&[
            "-rtsp_transport", "tcp",
            "-i", &params.rtsp_url,
            "-f", "mpegts",
            "-codec:v", "libx264",
            "-preset", "ultrafast",
            "-codec:a", "aac",
            "-ar", "44100",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            error!("Failed to start FFmpeg: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to start FFmpeg: {}. Make sure FFmpeg is installed and in PATH.", e),
            ).into_response();
        }
    };

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    log_ffmpeg_stderr(stderr);

    let stream = async_stream::stream! {
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut buffer = vec![0u8; 188 * 7];

        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => {
                    info!("FFmpeg stream ended");
                    break;
                }
                Ok(n) => {
                    yield Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(&buffer[..n]));
                }
                Err(e) => {
                    error!("Error reading from FFmpeg: {}", e);
                    break;
                }
            }
        }
    };

    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "video/mp2t")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("X-Content-Type-Options", "nosniff")
        .body(body)
        .unwrap()
}

async fn stream_hls_direct(Query(params): Query<DirectStreamQuery>) -> Response {

    info!("HLS handler entered - URL: {}", params.rtsp_url);
    let speed = params.speed.unwrap_or(1.0).max(0.5).min(4.0);

    if !params.rtsp_url.starts_with("rtsp://") && !params.rtsp_url.starts_with("rtsps://") {
        error!("Invalid RTSP URL format: {}", params.rtsp_url);
        return (
            StatusCode::BAD_REQUEST,
            format!("Invalid RTSP URL: '{}' ...", params.rtsp_url),
        )
            .into_response();
    }

    info!("Direct HLS stream requested for {} at {}x speed", params.rtsp_url, speed);
    if speed >= 2.0 {
        info!("Fast-forward mode: audio will be dropped for faster processing");
    }

    let id = Uuid::new_v4().to_string();
    let tmp_dir = std::env::temp_dir()
        .join(format!("hls-stream-{}", id))
        .to_string_lossy()
        .to_string();
    let playlist_path = format!("{}/playlist.m3u8", tmp_dir);
    let segment_pattern = format!("{}/segment%03d.ts", tmp_dir);
    let base_url = format!("/stream/hls/{}/", id);

    if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
        error!("Failed to create temp directory: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create temp directory: {}", e)).into_response();
    }

    let playlist_path_clone = playlist_path.clone();
    let rtsp_url_clone = params.rtsp_url.clone();

    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
    {
        let mut map = HLS_SESSIONS.write().await;
        map.insert(
            id.clone(),
            HlsSession {
                tmp_dir: tmp_dir.clone(),
                rtsp_url: params.rtsp_url.clone(),
                last_access: Instant::now(),
                shutdown: shutdown_tx.clone(),
            },
        );
    }

    let id_clone_for_ffmpeg = id.clone();
    let tmp_dir_for_ffmpeg = tmp_dir.clone();
    let sessions_for_ffmpeg = HLS_SESSIONS.clone();
    tokio::spawn(async move {
        let mut ffmpeg_args: Vec<String> = vec![
            "-rtsp_transport".to_string(), "tcp".to_string(),
        ];
        if speed > 1.0 {
            ffmpeg_args.extend_from_slice(&[
                "-readrate".to_string(), speed.to_string(),
            ]);
        }
        ffmpeg_args.extend_from_slice(&[
            "-i".to_string(), rtsp_url_clone.clone(),
        ]);
        if (speed - 1.0).abs() > 0.01 {
            let video_filter = format!("setpts=PTS/{}", speed);
            ffmpeg_args.extend_from_slice(&["-vf".to_string(), video_filter]);
            if speed >= 2.0 {
                ffmpeg_args.extend_from_slice(&["-an".to_string()]);
            } else {
                let audio_filter = format!("atempo={}", speed);
                ffmpeg_args.extend_from_slice(&["-af".to_string(), audio_filter]);
            }
        }
        ffmpeg_args.extend_from_slice(&[
            "-f".to_string(), "hls".to_string(),
            "-hls_time".to_string(), "2".to_string(),
            "-hls_list_size".to_string(), "10".to_string(),
            "-hls_flags".to_string(), "delete_segments+independent_segments".to_string(),
            "-hls_segment_filename".to_string(), segment_pattern.clone(),
            "-hls_base_url".to_string(), base_url.clone(),
            "-codec:v".to_string(), "libx264".to_string(),
            "-preset".to_string(), "veryfast".to_string(),
            "-tune".to_string(), "zerolatency".to_string(),
            "-profile:v".to_string(), "baseline".to_string(),
            "-g".to_string(), "50".to_string(),
            "-keyint_min".to_string(), "25".to_string(),
            "-sc_threshold".to_string(), "0".to_string(),
            "-b:v".to_string(), "1000k".to_string(),
            "-threads".to_string(), "0".to_string(),
        ]);
        if speed < 2.0 {
            ffmpeg_args.extend_from_slice(&[
                "-codec:a".to_string(), "aac".to_string(),
                "-ar".to_string(), "44100".to_string(),
                "-b:a".to_string(), "96k".to_string(),
            ]);
        }
        ffmpeg_args.push(playlist_path_clone.clone());

        info!("HLS session {}: starting FFmpeg, segments -> {}", id_clone_for_ffmpeg, segment_pattern);
        let mut child = match crate::ffmpeg_command()
            .args(&ffmpeg_args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                error!("Failed to start FFmpeg for HLS: {}", e);
                let mut map = sessions_for_ffmpeg.write().await;
                map.remove(&id_clone_for_ffmpeg);
                return;
            }
        };
        let stderr = child.stderr.take().unwrap();
        log_ffmpeg_stderr(stderr);

        tokio::select! {
            _ = shutdown_rx.recv() => {
                info!("Shutting down HLS session {}", id_clone_for_ffmpeg);
                let _ = child.kill().await;
            }
            _ = child.wait() => {
                info!("HLS ffmpeg process exited for session {}", id_clone_for_ffmpeg);
            }
        }
        let _ = std::fs::remove_dir_all(&tmp_dir_for_ffmpeg);
        let mut map = sessions_for_ffmpeg.write().await;
        map.remove(&id_clone_for_ffmpeg);
    });

    let id_for_monitor = id.clone();
    let sessions_for_monitor = HLS_SESSIONS.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let should_shutdown = {
                let map = sessions_for_monitor.read().await;
                if let Some(sess) = map.get(&id_for_monitor) {
                    sess.last_access.elapsed() > HLS_IDLE_TIMEOUT
                } else {
                    false
                }
            };
            if should_shutdown {
                info!("HLS session {} idle timeout reached; requesting shutdown", id_for_monitor);
                let mut map = sessions_for_monitor.write().await;
                if let Some(sess) = map.get(&id_for_monitor) {
                    let _ = sess.shutdown.try_send(());
                }
                break;
            }
        }
    });

    let playlist_rel_url = format!("/stream/hls/{}/playlist.m3u8", id);
    let mut ready = false;
    for _ in 0..40 {
        if let Ok(meta) = std::fs::metadata(&playlist_path) {
            if meta.len() > 0 {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
    }

    if !ready {
        error!("Failed to find playlist after waiting: {}", playlist_path);
        return (
            StatusCode::BAD_GATEWAY,
            "HLS playlist not available; source may be unreachable",
        )
            .into_response();
    }

    info!("HLS session {}: playlist ready at {}", id, playlist_path);
    {
        let mut map = HLS_SESSIONS.write().await;
        if let Some(sess) = map.get_mut(&id) {
            sess.last_access = Instant::now();
        }
    }

    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, playlist_rel_url)
        .body(Body::empty())
        .unwrap()
}

async fn stream_hls_session_playlist(Path(id): Path<String>) -> Response {
    let tmp_dir = std::env::temp_dir()
        .join(format!("hls-stream-{}", id))
        .to_string_lossy()
        .to_string();
    let path = format!("{}/playlist.m3u8", tmp_dir);

    {
        let mut map = HLS_SESSIONS.write().await;
        if let Some(sess) = map.get_mut(&id) {
            sess.last_access = Instant::now();
        }
    }

    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(e) => {
            error!("Playlist read error for {}: {}", path, e);
            (
                StatusCode::NOT_FOUND,
                "Playlist not found",
            )
                .into_response()
        }
    }
}

async fn stream_hls_session_segment(Path((id, file)): Path<(String, String)>) -> Response {
    if file.contains("..") || file.contains('/') || file.contains('\\') {
        return (
            StatusCode::BAD_REQUEST,
            "Invalid segment path",
        )
            .into_response();
    }

    {
        let mut map = HLS_SESSIONS.write().await;
        if let Some(sess) = map.get_mut(&id) {
            sess.last_access = Instant::now();
        }
    }

    let path = std::env::temp_dir()
        .join(format!("hls-stream-{}", id))
        .join(&file)
        .to_string_lossy()
        .to_string();

    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let ctype = if file.ends_with(".ts") {
                "video/mp2t"
            } else if file.ends_with(".m3u8") {
                "application/vnd.apple.mpegurl"
            } else {
                "application/octet-stream"
            };

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, ctype)
                .header(header::CACHE_CONTROL, "no-cache")
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(e) => {
            error!("Segment read error for {}: {}", path, e);
            (
                StatusCode::NOT_FOUND,
                "Segment not found",
            )
                .into_response()
        }
    }
}

async fn proxy_cameras(Query(params): Query<ProxyCamerasQuery>) -> Response {
    let port = params.port.unwrap_or_else(|| "554".to_string());
    let username = params.username.unwrap_or_else(|| "admin".to_string());
    let password = params.password.unwrap_or_default();

    let encoded_user = urlencoding::encode(&username);
    let encoded_pass = urlencoding::encode(&password);
    let isapi_url = format!(
        "http://{}:{}@{}:{}/ISAPI/Streaming/channels",
        encoded_user, encoded_pass, params.ip, port
    );

    info!("Fetching cameras from {}:{}", params.ip, port);

    let client = Client::new();
    let response = match client
        .get(&isapi_url)
        .header("Accept", "application/json, application/xml")
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            error!("Camera fetch error: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                Json(ApiResponse {
                    success: false,
                    message: format!("Failed to contact NVR: {}", e),
                }),
            )
                .into_response();
        }
    };

    if !response.status().is_success() {
        let status = response.status();
        error!("NVR returned HTTP {}", status);
        return (
            StatusCode::BAD_GATEWAY,
            Json(ApiResponse {
                success: false,
                message: format!("NVR responded with {}", status),
            }),
        )
            .into_response();
    }

    let body = match response.text().await {
        Ok(text) => text,
        Err(e) => {
            error!("Failed reading NVR response: {}", e);
            return (
                StatusCode::BAD_GATEWAY,
                Json(ApiResponse {
                    success: false,
                    message: format!("Failed to read NVR response: {}", e),
                }),
            )
                .into_response();
        }
    };

    if let Ok(value) = serde_json::from_str::<Value>(&body) {
        return (
            StatusCode::OK,
            Json(value),
        )
            .into_response();
    }

    let channels = parse_channels_xml(&body);
    let response = ChannelListResponse { channels };

    (
        StatusCode::OK,
        Json(response),
    )
        .into_response()
}

fn parse_channels_xml(xml: &str) -> Vec<ChannelInfo> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);

    let mut buf = Vec::new();
    let mut channels = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_name: Option<String> = None;
    let mut in_channel = false;
    let mut current_tag: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "StreamingChannel" {
                    in_channel = true;
                    current_id = None;
                    current_name = None;
                } else if in_channel {
                    current_tag = Some(name);
                }
            }
            Ok(Event::Text(e)) => {
                if in_channel {
                    if let Some(tag) = &current_tag {
                        let text = e.unescape().unwrap_or_default().to_string();
                        if tag == "id" {
                            current_id = Some(text);
                        } else if tag == "name" {
                            current_name = Some(text);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "StreamingChannel" {
                    let id = current_id.clone().unwrap_or_else(|| format!("{}", channels.len() + 1));
                    let name_val = current_name.clone().unwrap_or_else(|| format!("Camera {}", channels.len() + 1));
                    channels.push(ChannelInfo { id, name: name_val });
                    in_channel = false;
                    current_tag = None;
                } else if in_channel {
                    current_tag = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    channels
}

async fn proxy_rtsp(Query(params): Query<ProxyRtspQuery>) -> Response {
    let port = params.port.unwrap_or_else(|| "554".to_string());
    let username = params.username.unwrap_or_else(|| "admin".to_string());
    let password = params.password.unwrap_or_default();
    let channel = params.channel.unwrap_or_else(|| "1".to_string());
    let stream_number = params.stream_number.unwrap_or_else(|| "1".to_string());

    let suffix = format!("{}{:02}", channel, stream_number.parse::<u32>().unwrap_or(1));

    let encoded_user = urlencoding::encode(&username);
    let encoded_pass = urlencoding::encode(&password);
    let rtsp_url = format!(
        "rtsp://{}:{}@{}:{}/ISAPI/Streaming/channels/{}",
        encoded_user, encoded_pass, params.ip, port, suffix
    );

    info!("Proxying RTSP channel {} from {}", channel, params.ip);

    let mut child = match crate::ffmpeg_command()
        .args(&[
            "-rtsp_transport", "tcp",
            "-i", &rtsp_url,
            "-vf", "scale=640:480",
            "-q:v", "5",
            "-f", "mjpeg",
            "-fflags", "flush_packets",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            error!("Failed to start FFmpeg: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to start FFmpeg: {}. Ensure ffmpeg is installed and in PATH.", e),
            )
                .into_response();
        }
    };

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    log_ffmpeg_stderr(stderr);

    let stream = async_stream::stream! {
        let mut child = child;
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut buffer = vec![0u8; 8192];

        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => {
                    info!("FFmpeg stream ended");
                    break;
                }
                Ok(n) => {
                    yield Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(&buffer[..n]));
                }
                Err(e) => {
                    error!("Error reading from FFmpeg: {}", e);
                    break;
                }
            }
        }

        let _ = child.wait().await;
    };

    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "multipart/x-mixed-replace; boundary=ffserver")
        .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
        .header("Pragma", "no-cache")
        .header("Expires", "0")
        .body(body)
        .unwrap()
}

async fn proxy_hls_rtsp(Query(params): Query<ProxyHlsRtspQuery>) -> Response {
    info!("Direct HLS stream requested for Hikvision channel");

    let port = params.port.unwrap_or_else(|| "554".to_string());
    let username = params.username.unwrap_or_else(|| "admin".to_string());
    let password = params.password.unwrap_or_default();
    let channel = params.channel.unwrap_or_else(|| "1".to_string());
    let stream_number = params.stream_number.unwrap_or_else(|| "1".to_string());

    let suffix = format!("{}{:02}", channel, stream_number.parse::<u32>().unwrap_or(1));

    let encoded_user = urlencoding::encode(&username);
    let encoded_pass = urlencoding::encode(&password);
    let rtsp_url = format!(
        "rtsp://{}:{}@{}:{}/ISAPI/Streaming/channels/{}",
        encoded_user, encoded_pass, params.ip, port, suffix
    );

    let id = Uuid::new_v4().to_string();
    let tmp_dir = std::env::temp_dir()
        .join(format!("hls-proxyhl-{}", id))
        .to_string_lossy()
        .to_string();
    let playlist_path = format!("{}/playlist.m3u8", tmp_dir);
    let segment_pattern = format!("{}/segment%03d.ts", tmp_dir);
    let base_url = format!("/proxyhl/segment/{}/", id);

    if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
        error!("Failed to create temp directory: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create temp directory: {}", e),
        )
            .into_response();
    }

    let playlist_path_clone = playlist_path.clone();
    let rtsp_url_clone = rtsp_url.clone();

    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
    {
        let mut map = HLS_SESSIONS.write().await;
        map.insert(
            id.clone(),
            HlsSession {
                tmp_dir: tmp_dir.clone(),
                rtsp_url: rtsp_url.clone(),
                last_access: Instant::now(),
                shutdown: shutdown_tx.clone(),
            },
        );
    }

    let id_clone_for_ffmpeg = id.clone();
    let tmp_dir_for_ffmpeg = tmp_dir.clone();
    let sessions_for_ffmpeg = HLS_SESSIONS.clone();
    tokio::spawn(async move {
        let mut child = match crate::ffmpeg_command()
            .args(&[
                "-rtsp_transport", "tcp",
                "-i", &rtsp_url_clone,
                "-f", "hls",
                "-hls_time", "2",
                "-hls_list_size", "10",
                "-hls_flags", "delete_segments+independent_segments",
                "-hls_segment_filename", &segment_pattern,
                "-hls_base_url", &base_url,
                "-codec:v", "libx264",
                "-preset", "ultrafast",
                "-tune", "zerolatency",
                "-g", "50",
                "-keyint_min", "25",
                "-sc_threshold", "0",
                "-b:v", "2000k",
                "-codec:a", "aac",
                "-ar", "44100",
                "-b:a", "128k",
                &playlist_path_clone,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                error!("Failed to start FFmpeg for HLS: {}", e);
                let mut map = sessions_for_ffmpeg.write().await;
                map.remove(&id_clone_for_ffmpeg);
                return;
            }
        };
        let stderr = child.stderr.take().unwrap();
        log_ffmpeg_stderr(stderr);

        tokio::select! {
            _ = shutdown_rx.recv() => {
                info!("Shutting down HLS session {} due to inactivity or explicit stop", id_clone_for_ffmpeg);
                let _ = child.kill().await;
            }
            _ = child.wait() => {
                info!("HLS ffmpeg process exited for session {}", id_clone_for_ffmpeg);
            }
        }
        let _ = std::fs::remove_dir_all(&tmp_dir_for_ffmpeg);
        let mut map = sessions_for_ffmpeg.write().await;
        map.remove(&id_clone_for_ffmpeg);
    });

    let id_for_monitor = id.clone();
    let sessions_for_monitor = HLS_SESSIONS.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let should_shutdown = {
                let map = sessions_for_monitor.read().await;
                if let Some(sess) = map.get(&id_for_monitor) {
                    sess.last_access.elapsed() > HLS_IDLE_TIMEOUT
                } else {
                    false
                }
            };
            if should_shutdown {
                info!("HLS session {} idle timeout reached; requesting shutdown", id_for_monitor);
                let mut map = sessions_for_monitor.write().await;
                if let Some(sess) = map.get(&id_for_monitor) {
                    let _ = sess.shutdown.try_send(());
                }
                break;
            }
        }
    });

    let playlist_rel_url = format!("/proxyhl/segment/{}/playlist.m3u8", id);
    let mut ready = false;
    for _ in 0..40 {
        if let Ok(meta) = std::fs::metadata(&playlist_path) {
            if meta.len() > 0 {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
    }

    if !ready {
        error!("Failed to find playlist after waiting: {}", playlist_path);
        return (
            StatusCode::BAD_GATEWAY,
            "HLS playlist not available; source may be unreachable or credentials invalid",
        )
            .into_response();
    }

    {
        let mut map = HLS_SESSIONS.write().await;
        if let Some(sess) = map.get_mut(&id) {
            sess.last_access = Instant::now();
        }
    }

    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, playlist_rel_url)
        .body(Body::empty())
        .unwrap()
}

async fn proxy_hls_segment(Path((id, file)): Path<(String, String)>) -> Response {
    if file.contains("..") || file.contains('/') || file.contains('\\') {
        return (
            StatusCode::BAD_REQUEST,
            "Invalid segment path",
        )
            .into_response();
    }

    {
        let mut map = HLS_SESSIONS.write().await;
        if let Some(sess) = map.get_mut(&id) {
            sess.last_access = Instant::now();
        }
    }

    let path = std::env::temp_dir()
        .join(format!("hls-proxyhl-{}", id))
        .join(&file)
        .to_string_lossy()
        .to_string();

    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let ctype = if file.ends_with(".ts") {
                "video/mp2t"
            } else if file.ends_with(".m3u8") {
                "application/vnd.apple.mpegurl"
            } else {
                "application/octet-stream"
            };

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, ctype)
                .header(header::CACHE_CONTROL, "no-cache")
                .body(Body::from(bytes))
                .unwrap()
        }
        Err(e) => {
            error!("Segment read error for {}: {}", path, e);
            (
                StatusCode::NOT_FOUND,
                "Segment not found",
            )
                .into_response()
        }
    }
}

#[derive(Serialize)]
struct HlsSessionView {
    id: String,
    rtsp_url: String,
    last_access_secs: u64,
}

#[derive(Serialize)]
struct HlsSessionsListResponse {
    sessions: Vec<HlsSessionView>,
}

async fn list_proxyhl_sessions() -> impl IntoResponse {
    let map = HLS_SESSIONS.read().await;
    let mut sessions: Vec<HlsSessionView> = Vec::new();
    for (id, sess) in map.iter() {
        sessions.push(HlsSessionView {
            id: id.clone(),
            rtsp_url: sess.rtsp_url.clone(),
            last_access_secs: sess.last_access.elapsed().as_secs(),
        });
    }
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last_access_secs));
    Json(HlsSessionsListResponse { sessions })
}

async fn player_page(Query(params): Query<DirectStreamQuery>) -> Response {
    if !params.rtsp_url.starts_with("rtsp://") && !params.rtsp_url.starts_with("rtsps://") {
        let error_html = format!(r#"<!DOCTYPE html>
<html>
<head><title>Error</title></head>
<body style="font-family: Arial; padding: 20px; background: #1a1a1a; color: #fff;">
<h1>❌ Invalid RTSP URL</h1>
<p>The provided URL '{rtsp_url}' is invalid.</p>
<p>RTSP URLs must start with 'rtsp://' or 'rtsps://'.</p>
<p><strong>Make sure to properly URL-encode the rtsp_url parameter.</strong></p>
<h3>Example:</h3>
<code style="background: #333; padding: 10px; display: block;">
/player?rtsp_url=rtsp%3A%2F%2Fadmin%3Apassword%40192.168.1.100%3A554%2Fstream&speed=1.0
</code>
</body>
</html>"#, rtsp_url = params.rtsp_url);
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(error_html))
            .unwrap();
    }

    let speed = params.speed.unwrap_or(1.0).max(0.5).min(4.0);
    let mut hls_url = format!("/stream/hls?rtsp_url={}", urlencoding::encode(&params.rtsp_url));
    if (speed - 1.0).abs() > 0.01 {
        hls_url.push_str(&format!("&speed={}", speed));
    }

    let audio_warning = if speed >= 2.0 {
        "<div style=\"margin-top: 10px; padding: 10px; background: #ff9800; color: #000; border-radius: 4px;\">\
        ⚡ Fast-forward mode: Audio disabled for faster processing</div>"
    } else {
        ""
    };

    let rtsp_url_js = params.rtsp_url.replace('\\', "\\\\").replace('\'', "\\'");
    let hls_url_js = hls_url.replace('\\', "\\\\").replace('\'', "\\'");

    let html = format!(r#"<!DOCTYPE html>
<html>
<head>
    <title>RTSP Stream Player</title>
    <script src="https://cdn.jsdelivr.net/npm/hls.js@latest"></script>
    <style>
        body {{
            margin: 0;
            padding: 20px;
            font-family: Arial, sans-serif;
            background: #1a1a1a;
            color: #fff;
        }}
        .container {{
            max-width: 1200px;
            margin: 0 auto;
        }}
        h1 {{
            text-align: center;
            margin-bottom: 20px;
        }}
        .video-wrapper {{
            background: #000;
            padding: 20px;
            border-radius: 8px;
            text-align: center;
        }}
        video {{
            width: 100%;
            max-width: 1000px;
            height: auto;
            border-radius: 4px;
        }}
        .info {{
            margin-top: 20px;
            padding: 15px;
            background: #2a2a2a;
            border-radius: 4px;
        }}
        .status {{
            padding: 10px;
            margin-top: 10px;
            background: #334455;
            border-radius: 4px;
            font-size: 12px;
        }}
        .controls {{
            margin-top: 15px;
            padding: 15px;
            background: #2a2a2a;
            border-radius: 4px;
            display: flex;
            align-items: center;
            gap: 15px;
            justify-content: center;
        }}
        .controls button {{
            padding: 8px 16px;
            background: #4CAF50;
            color: white;
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-size: 14px;
        }}
        .controls button:hover {{
            background: #45a049;
        }}
        .controls button.active {{
            background: #2196F3;
        }}
        .speed-label {{
            font-weight: bold;
            color: #4CAF50;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>🎥 RTSP Stream Player</h1>
        <div class="video-wrapper">
            <video id="player" controls autoplay width="800" height="600"></video>
        </div>
        <div class="controls">
            <span>Playback Speed:</span>
            <button onclick="changeSpeed(0.5)">0.5x</button>
            <button onclick="changeSpeed(1.0)" class="active" id="btn-1">1.0x</button>
            <button onclick="changeSpeed(1.5)" id="btn-1.5">1.5x</button>
            <button onclick="changeSpeed(2.0)" id="btn-2">2.0x</button>
            <button onclick="changeSpeed(3.0)" id="btn-3">3.0x</button>
            <button onclick="changeSpeed(4.0)" id="btn-4">4.0x</button>
            <span class="speed-label" id="current-speed">(Current: {speed}x)</span>
        </div>
        <div class="info">
            <strong>Stream URL:</strong><br>
            <code>{rtsp_url}</code>
            <div class="status" id="status">Loading...</div>
            {audio_warning}
        </div>
    </div>
    <script>
        function logToServer(level, message) {{
            fetch('/api/client-log', {{
                method: 'POST',
                headers: {{'Content-Type': 'application/json'}},
                body: JSON.stringify({{level: level, message: message}})
            }}).catch(function() {{}});
        }}

        ['log', 'warn', 'error'].forEach(function(fn) {{
            const original = console[fn];
            console[fn] = function() {{
                original.apply(console, arguments);
                const args = Array.prototype.slice.call(arguments);
                const msg = args.map(function(a) {{
                    return (typeof a === 'object') ? JSON.stringify(a) : String(a);
                }}).join(' ');
                logToServer(fn === 'log' ? 'info' : fn, msg);
            }};
        }});

        window.addEventListener('error', function(e) {{
            logToServer('error', 'window.onerror: ' + e.message + ' at ' + e.filename + ':' + e.lineno);
        }});

        logToServer('info', 'Player page loaded. Hls defined: ' + (typeof Hls !== 'undefined'));

        const videoElement = document.getElementById('player');
        const statusDiv = document.getElementById('status');
        const hlsSourceUrl = '{hls_url_js}';

        logToServer('info', 'HLS source URL resolved to: ' + hlsSourceUrl);

        if (typeof Hls === 'undefined') {{
            logToServer('error', 'Hls.js did not load from CDN');
            statusDiv.innerHTML = '❌ hls.js failed to load';
        }} else if (!Hls.isSupported()) {{
            logToServer('error', 'Hls.isSupported() returned false in this browser');
            statusDiv.innerHTML = '❌ HLS not supported in this browser';
        }} else {{
            logToServer('info', 'Creating Hls instance and calling loadSource');
            const hls = new Hls();

            hls.loadSource(hlsSourceUrl);
            hls.attachMedia(videoElement);

            hls.on(Hls.Events.MANIFEST_PARSED, function() {{
                logToServer('info', 'MANIFEST_PARSED event fired');
                statusDiv.innerHTML = '✅ Stream loaded successfully. Playing...';
                videoElement.play().catch(function(e) {{
                    logToServer('warn', 'Autoplay blocked: ' + e.message);
                    statusDiv.innerHTML = '⚠️ Autoplay blocked: ' + e.message;
                }});
            }});

            hls.on(Hls.Events.ERROR, function(event, data) {{
                logToServer('error', 'HLS.js error: ' + JSON.stringify(data));
                if (data.fatal) {{
                    statusDiv.innerHTML = '❌ Stream error: ' + (data.details || 'unknown');
                }}
            }});
        }}

        function changeSpeed(speed) {{
            const rtspUrl = encodeURIComponent('{rtsp_url_js}');
            const newUrl = '/player?rtsp_url=' + rtspUrl + '&speed=' + speed;
            logToServer('info', 'Changing speed to ' + speed + ', navigating to ' + newUrl);
            window.location.href = newUrl;
        }}

        const currentSpeed = {speed};
        document.querySelectorAll('.controls button').forEach(function(btn) {{
            btn.classList.remove('active');
        }});
        const activeBtn = document.getElementById('btn-' + currentSpeed);
        if (activeBtn) {{
            activeBtn.classList.add('active');
        }}
    </script>
</body>
</html>"#,
        speed = speed,
        rtsp_url = params.rtsp_url,
        audio_warning = audio_warning,
        hls_url_js = hls_url_js,
        rtsp_url_js = rtsp_url_js
    );

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(html))
        .unwrap()
}