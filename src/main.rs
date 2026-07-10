#![windows_subsystem = "windows"]

use anyhow::Result;
use clap::Parser;
use log::{info, LevelFilter};
use std::sync::Arc;
use tokio::sync::RwLock;

mod embedded;
mod gui;
mod rtsp_client;
mod service;
mod shared;                   // <-- this exposes shared/mod.rs → pub mod logger;
mod stream_manager;
mod streaming_server;

use stream_manager::StreamManager;
use streaming_server::StreamingServer;

/// Returns a `tokio::process::Command` pointing to the bundled ffmpeg.exe.
/// The binary is extracted automatically if not already present.
pub fn ffmpeg_command() -> tokio::process::Command {
    let ffmpeg_path = embedded::ensure_ffmpeg_extracted();
    tokio::process::Command::new(&ffmpeg_path)
}

#[derive(Parser, Debug)]
#[command(name = "rtsp-proxy")]
#[command(about = "RTSP to HLS/MPEG-TS proxy server", long_about = None)]
struct Args {
    #[arg(long)]
    server: bool,

    #[arg(short, long, default_value = "5000")]
    port: u16,

    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    #[arg(long)]
    install: bool,

    #[arg(long)]
    uninstall: bool,
}

fn main() -> Result<()> {
    // Initialize YOUR NTSM‑style file logger (writes to rtsp-proxy.log)
    shared::logger::init_with_level(LevelFilter::Info)
        .map_err(|e| anyhow::anyhow!("Logger init failed: {}", e))?;

    let args = Args::parse();

    if args.server {
        let _ = embedded::ensure_ffmpeg_extracted();
        run_proxy_server(args.host, args.port)
    } else if args.install {
        service::install_service()
    } else if args.uninstall {
        service::uninstall_service()
    } else {
        gui::run_gui();
        Ok(())
    }
}

fn run_proxy_server(host: String, port: u16) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        run_proxy_server_async(host, port).await
    })
}

async fn run_proxy_server_async(host: String, port: u16) -> Result<()> {
    info!("Starting RTSP Proxy Server");
    info!("Server will listen on {}:{}", host, port);

    let stream_manager = Arc::new(RwLock::new(StreamManager::new()));
    let server = StreamingServer::new(host, port, stream_manager);
    server.run().await?;

    Ok(())
}