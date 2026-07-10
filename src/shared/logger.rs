// src/shared/logger.rs
//! App-wide file logger.
//!
//! Features:
//!   - Single log file next to the exe: ntsm-agent.log
//!   - Rotation: new file when size exceeds MAX_BYTES (5 MB)
//!   - Retention: keeps last 3 rotated files (.1, .2, .3), deletes older ones
//!   - Format: [YYYY-MM-DD HH:MM:SS UTC] [LEVEL] target — message
//!   - Implements log::Log so log::info!() / log::error!() etc. work everywhere
//!   - Thread-safe via Mutex<Option<File>> – no file is created while logs are Off
//!   - Runtime log-level switching via set_level() / current_level()
//!   - Call logger::init() once at startup in every entry point

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{Level, LevelFilter, Metadata, Record, SetLoggerError};

// ─── Configuration ────────────────────────────────────────────────────────────

/// Max log file size before rotation (5 MB)
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// Number of rotated files to keep (.1 … .N)
const KEEP_ROTATED: u32 = 3;

/// Log file name (written next to the exe)
const LOG_FILE: &str = "rtsp-proxy.log";

// ─── Runtime level ────────────────────────────────────────────────────────────

static CURRENT_LEVEL: AtomicU8 = AtomicU8::new(LevelFilter::Off as u8);

pub fn set_level(level: LevelFilter) {
    let old = CURRENT_LEVEL.swap(level as u8, Ordering::Relaxed);
    log::set_max_level(level);

    if old == LevelFilter::Off as u8 && level != LevelFilter::Off {
        ensure_file_open();
    }
}

pub fn current_level() -> LevelFilter {
    match CURRENT_LEVEL.load(Ordering::Relaxed) {
        1 => LevelFilter::Error,
        2 => LevelFilter::Warn,
        3 => LevelFilter::Info,
        4 => LevelFilter::Debug,
        5 => LevelFilter::Trace,
        _ => LevelFilter::Off,
    }
}

pub fn level_from_str(s: &str) -> LevelFilter {
    match s.to_lowercase().as_str() {
        "error" => LevelFilter::Error,
        "warn"  => LevelFilter::Warn,
        "info"  => LevelFilter::Info,
        "debug" => LevelFilter::Debug,
        "trace" => LevelFilter::Trace,
        _       => LevelFilter::Off,
    }
}

pub fn level_to_str(level: LevelFilter) -> &'static str {
    match level {
        LevelFilter::Off   => "off",
        LevelFilter::Error => "error",
        LevelFilter::Warn  => "warn",
        LevelFilter::Info  => "info",
        LevelFilter::Debug => "debug",
        LevelFilter::Trace => "trace",
    }
}

// ─── Logger struct ────────────────────────────────────────────────────────────

struct FileLogger {
    file: Mutex<Option<File>>,
    path: PathBuf,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= current_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let line = format_record(record);

        let mut guard = self.file.lock().unwrap();
        if let Some(ref mut file) = *guard {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
            drop(guard);
            self.maybe_rotate();
        }

        if record.level() <= Level::Warn {
            eprintln!("{line}");
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(ref mut file) = *guard {
                let _ = file.flush();
            }
        }
    }
}

impl FileLogger {
    fn maybe_rotate(&self) {
        let guard = self.file.lock().unwrap();
        if guard.is_none() {
            return;
        }
        let size = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if size < MAX_BYTES {
            return;
        }
        drop(guard);

        let oldest = rotated_path(&self.path, KEEP_ROTATED);
        let _ = fs::remove_file(&oldest);
        for i in (1..KEEP_ROTATED).rev() {
            let from = rotated_path(&self.path, i);
            let to = rotated_path(&self.path, i + 1);
            let _ = fs::rename(&from, &to);
        }
        let _ = fs::rename(&self.path, rotated_path(&self.path, 1));

        if let Ok(new_file) = open_log_file(&self.path) {
            let mut guard = self.file.lock().unwrap();
            *guard = Some(new_file);
        }
    }
}

// ─── Stored pointer for later file‑opening ────────────────────────────────────

static LOGGER_PTR: AtomicPtr<FileLogger> = AtomicPtr::new(std::ptr::null_mut());

fn ensure_file_open() {
    let ptr = LOGGER_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        return;
    }
    let logger = unsafe { &*ptr };
    let mut file_guard = logger.file.lock().unwrap();
    if file_guard.is_none() {
        match open_log_file(&logger.path) {
            Ok(file) => {
                let banner = format!(
                    "\n{}\n[{}] [INFO] ntsm_agent — process started (PID {})\n{}",
                    "─".repeat(72),
                    timestamp(),
                    std::process::id(),
                    "─".repeat(72),
                );
                let mut f = file;
                let _ = writeln!(f, "{banner}");
                *file_guard = Some(f);
            }
            Err(_) => {
                let fallback = std::env::temp_dir().join(LOG_FILE);
                if let Ok(f) = open_log_file(&fallback) {
                    *file_guard = Some(f);
                }
            }
        }
    }
}

// ─── Public init ──────────────────────────────────────────────────────────────

pub fn init() -> Result<(), SetLoggerError> {
    init_with_level(default_level())
}

pub fn init_with_level(level: LevelFilter) -> Result<(), SetLoggerError> {
    let path = log_path();

    let file = if level != LevelFilter::Off {
        let mut f = open_log_file(&path).unwrap_or_else(|_| {
            let fallback = std::env::temp_dir().join(LOG_FILE);
            open_log_file(&fallback).expect("cannot open any log file")
        });
        let banner = format!(
            "\n{}\n[{}] [INFO] ntsm_agent — process started (PID {})\n{}",
            "─".repeat(72),
            timestamp(),
            std::process::id(),
            "─".repeat(72),
        );
        let _ = writeln!(f, "{banner}");
        Some(f)
    } else {
        None
    };

    let logger = Box::new(FileLogger {
        file: Mutex::new(file),
        path,
    });

    // Keep a raw pointer so ensure_file_open() can access the logger later.
    let ptr = &*logger as *const FileLogger as *mut FileLogger;
    LOGGER_PTR.store(ptr, Ordering::Release);

    // Hand ownership to the log crate (the pointer remains valid forever).
    log::set_boxed_logger(logger)?;

    CURRENT_LEVEL.store(level as u8, Ordering::Relaxed);
    log::set_max_level(level);
    Ok(())
}

pub fn flush() {
    log::logger().flush();
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn format_record(record: &Record) -> String {
    let short = record
        .target()
        .rsplit("::")
        .next()
        .unwrap_or(record.target());
    format!(
        "[{ts}] [{lvl}] [{short}] {msg}",
        ts = timestamp(),
        lvl = record.level(),
        short = short,
        msg = record.args(),
    )
}

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86_400;

    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}-{month:02}-{day:02} {h:02}:{m:02}:{s:02} UTC")
}

fn days_to_ymd(mut z: u64) -> (u64, u64, u64) {
    z += 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn rotated_path(base: &PathBuf, n: u32) -> PathBuf {
    let mut p = base.as_os_str().to_owned();
    p.push(format!(".{n}"));
    PathBuf::from(p)
}

fn open_log_file(path: &PathBuf) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn log_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(LOG_FILE)
}

fn default_level() -> LevelFilter {
    LevelFilter::Off
}