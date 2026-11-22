//! hopilo Padupilo: satukan video + klip suara, lalu beri subtitle beranimasi.

pub mod dict;
pub mod export;
pub mod media;
pub mod placement;
pub mod project;
pub mod render;
pub mod setup;
pub mod subs;
pub mod transcribe;

use std::path::PathBuf;

/// Folder aplikasi (tempat .exe). Semua data portable disimpan di sini.
pub fn app_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PADUPILO_HOME") {
        return PathBuf::from(dir);
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}
