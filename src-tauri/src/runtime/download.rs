use fs2::available_space;
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

use super::artifacts::{hex_digest, sha256_file, verify_artifact_signature};
use super::events::{emit_detailed, emit_detailed_with_metadata};
use super::operation::{check_cancelled, is_cancelled};
use super::plan::ProgressRange;
use super::types::{Artifact, RuntimePaths};

pub(crate) fn download_artifact(
    app: &AppHandle,
    runtime: &RuntimePaths,
    artifact: &Artifact,
    label: &str,
    step_phase: &str,
    progress: ProgressRange,
) -> Result<PathBuf, String> {
    let artifact_id = &artifact.id;
    let target = runtime.downloads.join(&artifact.file_name);
    let partial = runtime
        .downloads
        .join(format!(".{}.part", artifact.file_name));
    if target.is_file() && sha256_file(&target)? == artifact.sha256 {
        verify_artifact_signature(artifact, &target)?;
        let size = fs::metadata(&target).ok().map(|metadata| metadata.len());
        emit_detailed(
            app,
            step_phase,
            "cached",
            &format!("已复用缓存的 {label} 运行时"),
            progress.end,
            Some(format!("CACHE {}", target.display())),
            size,
            size,
        );
        return Ok(target);
    }
    if target.exists() {
        fs::remove_file(&target)
            .map_err(|error| format!("无法清理损坏的 {label} 下载缓存: {error}"))?;
    }

    if partial.is_file() && sha256_file(&partial).ok().as_deref() == Some(artifact.sha256.as_str())
    {
        verify_artifact_signature(artifact, &partial)?;
        let size = fs::metadata(&partial).ok().map(|metadata| metadata.len());
        fs::rename(&partial, &target)
            .map_err(|error| format!("无法提交已完成的 {label} 断点缓存: {error}"))?;
        emit_detailed_with_metadata(
            app,
            step_phase,
            "completed",
            &format!("{label} 断点缓存校验通过"),
            progress.end,
            Some(format!("RESUME-CACHE {}", target.display())),
            size,
            size,
            Some(artifact_id.clone()),
            0,
            None,
        );
        return Ok(target);
    }

    let client = Client::builder()
        .user_agent("tauri-embedded-runtime/0.1")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60 * 60))
        .build()
        .map_err(|error| format!("无法创建下载客户端: {error}"))?;

    let max_retries = 3_u32;
    let mut last_error = format!("下载 {label} 运行时失败");
    for retry_count in 0..=max_retries {
        check_cancelled(app)?;
        let partial_bytes = fs::metadata(&partial)
            .ok()
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len())
            .unwrap_or_default();
        let mut request = client.get(artifact.url.as_str());
        if partial_bytes > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={partial_bytes}-"));
        }

        let mut response = match request.send() {
            Ok(response) => response,
            Err(error) => {
                last_error = format!("下载 {label} 运行时失败: {error}");
                if retry_count == max_retries {
                    break;
                }
                emit_detailed_with_metadata(
                    app,
                    step_phase,
                    "retrying",
                    &format!(
                        "下载连接失败，正在重试 ({}/{})",
                        retry_count + 1,
                        max_retries
                    ),
                    progress.start,
                    Some(format!("GET {}", artifact.url)),
                    Some(partial_bytes),
                    None,
                    Some(label.to_string()),
                    retry_count + 1,
                    Some("download_retry".to_string()),
                );
                thread::sleep(Duration::from_millis(500 * u64::from(retry_count + 1)));
                continue;
            }
        };

        if !response.status().is_success() {
            last_error = format!("下载 {label} 运行时失败，HTTP 状态码 {}", response.status());
            if retry_count == max_retries {
                break;
            }
            if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                let _ = fs::remove_file(&partial);
            }
            emit_detailed_with_metadata(
                app,
                step_phase,
                "retrying",
                &format!(
                    "HTTP 请求失败，正在重试 ({}/{})",
                    retry_count + 1,
                    max_retries
                ),
                progress.start,
                Some(format!("GET {}", artifact.url)),
                Some(partial_bytes),
                None,
                Some(label.to_string()),
                retry_count + 1,
                Some("http_error".to_string()),
            );
            thread::sleep(Duration::from_millis(500 * u64::from(retry_count + 1)));
            continue;
        }

        let resumed =
            partial_bytes > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        let downloaded_start = if resumed { partial_bytes } else { 0 };
        if !resumed && partial_bytes > 0 {
            fs::remove_file(&partial)
                .map_err(|error| format!("无法重置 {label} 断点缓存: {error}"))?;
        }
        let total_bytes = response
            .content_length()
            .map(|length| length.saturating_add(downloaded_start));
        if let Some(total_bytes) = total_bytes {
            if let Ok(free_bytes) = available_space(&runtime.downloads) {
                if total_bytes > free_bytes {
                    return Err(format!(
                        "磁盘空间不足，{label} 至少需要 {}，当前可用 {}",
                        format_bytes(total_bytes),
                        format_bytes(free_bytes)
                    ));
                }
            }
        }
        emit_detailed_with_metadata(
            app,
            step_phase,
            "running",
            &format!(
                "正在下载 {label} 运行时{}",
                if resumed { "（断点续传）" } else { "" }
            ),
            progress.start,
            Some(format!("GET {}", artifact.url)),
            Some(downloaded_start),
            total_bytes,
            Some(label.to_string()),
            retry_count,
            None,
        );

        let attempt = (|| -> Result<u64, String> {
            let mut output = if resumed {
                OpenOptions::new()
                    .append(true)
                    .open(&partial)
                    .map_err(|error| format!("无法打开 {label} 断点缓存: {error}"))?
            } else {
                File::create(&partial)
                    .map_err(|error| format!("无法创建 {label} 下载缓存: {error}"))?
            };
            let mut hasher = Sha256::new();
            if resumed {
                let mut existing = File::open(&partial)
                    .map_err(|error| format!("无法读取 {label} 断点缓存: {error}"))?;
                let mut existing_buffer = [0_u8; 1024 * 128];
                loop {
                    let read = existing
                        .read(&mut existing_buffer)
                        .map_err(|error| format!("读取 {label} 断点缓存失败: {error}"))?;
                    if read == 0 {
                        break;
                    }
                    hasher.update(&existing_buffer[..read]);
                }
            }
            let mut buffer = [0_u8; 1024 * 128];
            let mut downloaded_bytes = downloaded_start;
            let mut last_event_bytes = downloaded_start;
            loop {
                check_cancelled(app)?;
                let read = response
                    .read(&mut buffer)
                    .map_err(|error| format!("读取 {label} 下载内容失败: {error}"))?;
                if read == 0 {
                    break;
                }
                output
                    .write_all(&buffer[..read])
                    .map_err(|error| format!("写入 {label} 下载缓存失败: {error}"))?;
                hasher.update(&buffer[..read]);
                downloaded_bytes += read as u64;
                if downloaded_bytes.saturating_sub(last_event_bytes) >= 1024 * 1024
                    || total_bytes.is_some_and(|total| downloaded_bytes >= total)
                {
                    let download_progress = total_bytes
                        .filter(|total| *total > 0)
                        .map(|total| {
                            progress
                                .at(((downloaded_bytes.saturating_mul(100) / total).min(100)) as u8)
                        })
                        .unwrap_or(progress.start);
                    emit_detailed_with_metadata(
                        app,
                        step_phase,
                        "running",
                        &format!("下载 {label}：{}", format_bytes(downloaded_bytes)),
                        download_progress,
                        Some(format!("GET {}", artifact.url)),
                        Some(downloaded_bytes),
                        total_bytes,
                        Some(label.to_string()),
                        retry_count,
                        None,
                    );
                    last_event_bytes = downloaded_bytes;
                }
            }
            output
                .sync_all()
                .map_err(|error| format!("无法落盘 {label} 下载缓存: {error}"))?;
            let digest = hex_digest(&hasher.finalize());
            if digest != artifact.sha256 {
                return Err(format!(
                    "{label} 运行时校验失败，期望 SHA-256 {}, 实际 {}",
                    artifact.sha256, digest
                ));
            }
            verify_artifact_signature(artifact, &partial)?;
            Ok(downloaded_bytes)
        })();

        match attempt {
            Ok(downloaded_bytes) => {
                fs::rename(&partial, &target)
                    .map_err(|error| format!("无法提交 {label} 下载缓存: {error}"))?;
                emit_detailed_with_metadata(
                    app,
                    step_phase,
                    "completed",
                    &format!("{label} 下载完成，SHA-256 校验通过"),
                    progress.end,
                    Some(format!("SHA-256 {}", artifact.sha256)),
                    Some(downloaded_bytes),
                    total_bytes,
                    Some(label.to_string()),
                    retry_count,
                    None,
                );
                return Ok(target);
            }
            Err(error) => {
                last_error = error;
                if is_cancelled(app) {
                    return Err("运行时任务已取消".to_string());
                }
                if last_error.contains("校验失败") {
                    let _ = fs::remove_file(&partial);
                }
                if retry_count < max_retries {
                    emit_detailed_with_metadata(
                        app,
                        step_phase,
                        "retrying",
                        &format!(
                            "{label} 下载暂时失败，正在重试 ({}/{})",
                            retry_count + 1,
                            max_retries
                        ),
                        progress.start,
                        Some(format!("GET {}", artifact.url)),
                        fs::metadata(&partial).ok().map(|metadata| metadata.len()),
                        total_bytes,
                        Some(label.to_string()),
                        retry_count + 1,
                        Some("download_retry".to_string()),
                    );
                    thread::sleep(Duration::from_millis(500 * u64::from(retry_count + 1)));
                }
            }
        }
    }
    Err(last_error)
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
