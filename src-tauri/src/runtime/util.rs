#[cfg(unix)]
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

pub(crate) fn unique_token() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("{}-{}", millis, std::process::id())
}

pub(crate) fn platform_name() -> String {
    std::env::consts::OS.to_string()
}

pub(crate) fn architecture_name() -> String {
    std::env::consts::ARCH.to_string()
}

pub(crate) fn runtime_bin_dir(generation: &Path, runtime: &str) -> PathBuf {
    let root = generation.join(runtime);
    if cfg!(windows) {
        root
    } else {
        root.join("bin")
    }
}

#[cfg(unix)]
pub(crate) fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("无法同步运行时目录 {}: {error}", path.display()))
}

#[cfg(not(unix))]
pub(crate) fn sync_directory(_path: &Path) -> Result<(), String> {
    Ok(())
}
