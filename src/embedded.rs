//src/embedded.rs

use std::fs;
use std::path::{Path, PathBuf};

/// The raw bytes of ffmpeg.exe, baked into the executable.
pub static FFMPEG_BYTES: &[u8] = include_bytes!("../third_party/ffmpeg/ffmpeg.exe");

/// Returns the path to the extracted ffmpeg.exe (cached in %LOCALAPPDATA%).
pub fn ensure_ffmpeg_extracted() -> PathBuf {
    let cache_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rtsp-proxy-ffmpeg");

    let ffmpeg_path = cache_dir.join("ffmpeg.exe");

    if !ffmpeg_path.exists() {
        // Create directory if it doesn't exist
        if let Some(parent) = ffmpeg_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create FFmpeg cache directory");
        }
        // Write the embedded binary to disk
        fs::write(&ffmpeg_path, FFMPEG_BYTES).expect("Failed to write ffmpeg.exe to disk");
    }

    ffmpeg_path
}