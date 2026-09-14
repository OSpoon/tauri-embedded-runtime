use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use tauri::{AppHandle, Manager};

use super::types::{RuntimeLock, RuntimeManifest, RuntimePaths, RuntimeTransaction};
use super::util::{sync_directory, unique_token};

pub(crate) fn paths(app: &AppHandle) -> Result<RuntimePaths, String> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("无法解析应用数据目录: {error}"))?
        .join("runtime");

    Ok(RuntimePaths {
        manifest: root.join("manifest.json"),
        transaction: root.join("transaction.json"),
        lock: root.join("install.lock"),
        downloads: root.join("downloads"),
        generations: root.join("generations"),
        logs: root.join("logs"),
        root,
    })
}

pub(crate) fn ensure_layout(runtime: &RuntimePaths) -> Result<(), String> {
    for directory in [
        &runtime.root,
        &runtime.downloads,
        &runtime.generations,
        &runtime.logs,
    ] {
        fs::create_dir_all(directory)
            .map_err(|error| format!("无法创建运行时目录 {}: {error}", directory.display()))?;
    }
    Ok(())
}

pub(crate) fn recover_interrupted_layout(
    runtime: &RuntimePaths,
) -> Result<Option<RuntimeTransaction>, String> {
    let entries = fs::read_dir(&runtime.generations)
        .map_err(|error| format!("无法扫描运行时 generation: {error}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_directory = fs::symlink_metadata(&path)
            .map(|metadata| metadata.file_type().is_dir())
            .unwrap_or(false);
        if name.starts_with('.') && name.ends_with(".staging") && is_directory {
            fs::remove_dir_all(&path)
                .map_err(|error| format!("无法清理中断的运行时暂存目录: {error}"))?;
        }
    }
    for entry in fs::read_dir(&runtime.root)
        .map_err(|error| format!("无法扫描运行时临时文件: {error}"))?
        .flatten()
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_file = fs::symlink_metadata(&path)
            .map(|metadata| metadata.file_type().is_file())
            .unwrap_or(false);
        if name.starts_with("manifest.json.tmp-") && is_file {
            fs::remove_file(&path)
                .map_err(|error| format!("无法清理中断的 manifest 临时文件: {error}"))?;
        }
        if name.starts_with("transaction.json.tmp-") && is_file {
            fs::remove_file(&path)
                .map_err(|error| format!("无法清理中断的事务临时文件: {error}"))?;
        }
    }
    let Some(mut transaction) = read_transaction(runtime) else {
        return Ok(None);
    };
    if transaction.state == "running" {
        transaction.state = "interrupted".to_string();
        transaction.updated_at = crate::runtime::util::now();
        transaction.error =
            Some("上一次运行时操作未正常完成，暂存目录已清理，旧 generation 保留".to_string());
        write_transaction(runtime, &transaction)?;
    }
    Ok(Some(transaction))
}

pub(crate) fn acquire_lock(runtime: &RuntimePaths) -> Result<RuntimeLock, String> {
    ensure_layout(runtime)?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&runtime.lock)
        .map_err(|error| format!("无法打开运行时锁: {error}"))?;
    file.try_lock_exclusive()
        .map_err(|_| "另一个运行时安装或修复流程正在进行，请等待它完成后重试".to_string())?;
    Ok(RuntimeLock(file))
}

pub(crate) fn read_manifest(runtime: &RuntimePaths) -> Option<RuntimeManifest> {
    let content = fs::read_to_string(&runtime.manifest).ok()?;
    serde_json::from_str(&content).ok()
}

pub(crate) fn write_manifest(
    runtime: &RuntimePaths,
    manifest: &RuntimeManifest,
) -> Result<(), String> {
    let temp = runtime
        .root
        .join(format!("manifest.json.tmp-{}", unique_token()));
    let content = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("无法序列化运行时 manifest: {error}"))?;
    {
        let mut file =
            File::create(&temp).map_err(|error| format!("无法写入运行时 manifest: {error}"))?;
        file.write_all(&content)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("无法落盘运行时 manifest: {error}"))?;
    }

    #[cfg(windows)]
    {
        let backup = runtime.root.join("manifest.previous.json");
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("无法清理旧 manifest 备份: {error}"))?;
        }
        if runtime.manifest.exists() {
            fs::rename(&runtime.manifest, &backup)
                .map_err(|error| format!("无法暂存旧运行时 manifest: {error}"))?;
        }
        match fs::rename(&temp, &runtime.manifest) {
            Ok(()) => {
                let _ = fs::remove_file(backup);
                sync_directory(&runtime.root)?;
                Ok(())
            }
            Err(error) => {
                let _ = fs::rename(&backup, &runtime.manifest);
                Err(format!("无法原子替换运行时 manifest: {error}"))
            }
        }
    }

    #[cfg(not(windows))]
    {
        fs::rename(&temp, &runtime.manifest)
            .map_err(|error| format!("无法原子替换运行时 manifest: {error}"))?;
        sync_directory(&runtime.root)?;
        Ok(())
    }
}

pub(crate) fn read_transaction(runtime: &RuntimePaths) -> Option<RuntimeTransaction> {
    let content = fs::read_to_string(&runtime.transaction).ok()?;
    serde_json::from_str(&content).ok()
}

pub(crate) fn write_transaction(
    runtime: &RuntimePaths,
    transaction: &RuntimeTransaction,
) -> Result<(), String> {
    let temp = runtime
        .root
        .join(format!("transaction.json.tmp-{}", unique_token()));
    let content = serde_json::to_vec_pretty(transaction)
        .map_err(|error| format!("无法序列化运行时事务: {error}"))?;
    {
        let mut file =
            File::create(&temp).map_err(|error| format!("无法创建运行时事务: {error}"))?;
        file.write_all(&content)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("无法落盘运行时事务: {error}"))?;
    }
    fs::rename(&temp, &runtime.transaction)
        .map_err(|error| format!("无法提交运行时事务: {error}"))?;
    sync_directory(&runtime.root)
}

pub(crate) fn clear_transaction(runtime: &RuntimePaths) -> Result<(), String> {
    if runtime.transaction.exists() {
        fs::remove_file(&runtime.transaction)
            .map_err(|error| format!("无法清理运行时事务: {error}"))?;
        sync_directory(&runtime.root)?;
    }
    Ok(())
}

pub(crate) fn safe_generation_path(runtime: &RuntimePaths, generation: &str) -> Option<PathBuf> {
    let path = Path::new(generation);
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return None;
    }
    Some(runtime.generations.join(path))
}
