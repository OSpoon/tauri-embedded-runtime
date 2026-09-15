use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tauri::AppHandle;

use super::archive::extract;
use super::artifacts::{node_artifact, python_artifact};
use super::cleanup::prune_generations;
use super::download::download_artifact;
use super::events::{emit, emit_detailed_with_metadata};
use super::operation::{active_operation, check_cancelled};
use super::plan::ProgressRange;
use super::probe::{
    find_binary, inspect_with_requirements, probe_version, runtime_policy, version_matches,
};
use super::storage::{
    acquire_lock, clear_transaction, paths, read_manifest, safe_generation_path, write_manifest,
    write_transaction,
};
use super::types::{
    RuntimeManifest, RuntimePaths, RuntimeRequirements, RuntimeSnapshot, RuntimeTransaction,
    MANIFEST_SCHEMA_VERSION, NODE_VERSION, PYTHON_VERSION, RUNTIME_REVISION,
};
use super::util::{architecture_name, now, platform_name, sync_directory, unique_token};

fn write_text_file(path: &Path, content: &str) -> Result<(), String> {
    let mut file = File::create(path).map_err(|error| format!("无法创建运行时文件: {error}"))?;
    file.write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("无法写入运行时文件: {error}"))
}

#[derive(Clone, Copy)]
struct InstallProgress {
    python: ProgressRange,
    node: ProgressRange,
    validation: u8,
    commit: u8,
}

impl InstallProgress {
    fn single_component(component: &str, range: ProgressRange) -> Self {
        let component_range = range.subrange(8, 88);
        let empty = ProgressRange::new(range.start, range.start);
        Self {
            python: if component == "python" {
                component_range
            } else {
                empty
            },
            node: if component == "node" {
                component_range
            } else {
                empty
            },
            validation: range.at(90),
            commit: range.end,
        }
    }

    fn full() -> Self {
        let range = ProgressRange::new(8, 92);
        Self {
            python: range.subrange(0, 42),
            node: range.subrange(48, 90),
            validation: 96,
            commit: 100,
        }
    }
}

pub(super) fn copy_runtime_tree(
    source: &Path,
    destination: &Path,
    label: &str,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("无法读取现有 {label} 运行时: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!("现有 {label} 运行时目录无效"));
    }
    let source_root = fs::canonicalize(source)
        .map_err(|error| format!("无法解析现有 {label} 运行时目录: {error}"))?;
    fs::create_dir_all(destination)
        .map_err(|error| format!("无法创建复用的 {label} 运行时目录: {error}"))?;

    copy_runtime_directory(&source_root, source, destination, label)
}

fn copy_runtime_directory(
    source_root: &Path,
    source: &Path,
    destination: &Path,
    label: &str,
) -> Result<(), String> {
    for entry in fs::read_dir(source)
        .map_err(|error| format!("无法扫描现有 {label} 运行时: {error}"))?
        .flatten()
    {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path)
            .map_err(|error| format!("无法读取现有 {label} 运行时文件: {error}"))?;
        if metadata.file_type().is_symlink() {
            #[cfg(unix)]
            {
                let target = fs::read_link(&source_path)
                    .map_err(|error| format!("无法读取现有 {label} 运行时符号链接: {error}"))?;
                validate_runtime_link(source_root, &source_path, &target, label)?;
                std::os::unix::fs::symlink(&target, &destination_path)
                    .map_err(|error| format!("无法复制现有 {label} 运行时符号链接: {error}"))?;
            }
            #[cfg(not(unix))]
            {
                return Err(format!("现有 {label} 运行时包含不受支持的符号链接"));
            }
        } else if metadata.is_dir() {
            fs::create_dir_all(&destination_path)
                .map_err(|error| format!("无法创建复用的 {label} 运行时目录: {error}"))?;
            copy_runtime_directory(source_root, &source_path, &destination_path, label)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)
                .map_err(|error| format!("无法复用 {label} 运行时文件: {error}"))?;
        } else {
            return Err(format!(
                "现有 {label} 运行时包含不受支持的文件类型: {}",
                source_path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn validate_runtime_link(
    source_root: &Path,
    source_link: &Path,
    target: &Path,
    label: &str,
) -> Result<(), String> {
    if target.is_absolute() {
        return Err(format!(
            "现有 {label} 运行时包含绝对符号链接: {}",
            source_link.display()
        ));
    }
    let resolved = fs::canonicalize(source_link.parent().unwrap_or(source_root).join(target))
        .map_err(|error| format!("无法校验现有 {label} 运行时符号链接: {error}"))?;
    if !resolved.starts_with(source_root) {
        return Err(format!(
            "现有 {label} 运行时符号链接超出运行时目录: {}",
            source_link.display()
        ));
    }
    Ok(())
}

fn install_generation_with_target(
    app: &AppHandle,
    runtime: &RuntimePaths,
    requirements: &RuntimeRequirements,
    target: Option<&str>,
    progress: InstallProgress,
) -> Result<String, String> {
    let generation = format!("generation-{}", unique_token());
    let staging_name = format!(".{}.staging", generation);
    let staging = runtime.generations.join(&staging_name);
    let final_path = runtime.generations.join(&generation);
    fs::create_dir_all(&staging).map_err(|error| format!("无法创建运行时暂存目录: {error}"))?;
    let previous_manifest = read_manifest(runtime);
    let transaction = RuntimeTransaction {
        schema_version: MANIFEST_SCHEMA_VERSION,
        operation_id: active_operation(app).map(|operation| operation.id),
        state: "running".to_string(),
        generation: generation.clone(),
        previous_generation: previous_manifest
            .as_ref()
            .and_then(|manifest| manifest.active_generation.clone()),
        started_at: now(),
        updated_at: now(),
        error: None,
    };
    if let Err(error) = write_transaction(runtime, &transaction) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    let result = (|| {
        let previous_generation = transaction
            .previous_generation
            .as_deref()
            .and_then(|generation| safe_generation_path(runtime, generation));
        if requirements.python && target != Some("node") {
            check_cancelled(app)?;
            emit(
                app,
                "python",
                "running",
                "准备 Python 运行时",
                progress.python.start,
            );
            let artifact = python_artifact()?;
            let archive = download_artifact(
                app,
                runtime,
                &artifact,
                "Python",
                "python",
                progress.python.subrange(12, 76),
            )?;
            emit(
                app,
                "python",
                "running",
                "正在准备私有 Python 运行时",
                progress.python.at(88),
            );
            extract(&artifact, &archive, &staging.join("python"))?;
        } else if requirements.python {
            let source = previous_generation
                .as_deref()
                .map(|generation| generation.join("python"))
                .ok_or_else(|| "无法单项修复 Node.js：现有 Python generation 不存在".to_string())?;
            copy_runtime_tree(&source, &staging.join("python"), "Python")?;
        }
        if requirements.node && target != Some("python") {
            check_cancelled(app)?;
            emit(
                app,
                "node",
                "running",
                "准备 Node.js 运行时",
                progress.node.start,
            );
            let artifact = node_artifact()?;
            let archive = download_artifact(
                app,
                runtime,
                &artifact,
                "Node.js",
                "node",
                progress.node.subrange(12, 76),
            )?;
            emit(
                app,
                "node",
                "running",
                "正在准备私有 Node.js 运行时",
                progress.node.at(88),
            );
            extract(&artifact, &archive, &staging.join("node"))?;
        } else if requirements.node {
            let source = previous_generation
                .as_deref()
                .map(|generation| generation.join("node"))
                .ok_or_else(|| "无法单项修复 Python：现有 Node.js generation 不存在".to_string())?;
            copy_runtime_tree(&source, &staging.join("node"), "Node.js")?;
        }

        check_cancelled(app)?;
        let validation_phase = if requirements.node { "node" } else { "python" };
        emit(
            app,
            validation_phase,
            "running",
            if requirements.node {
                "正在校验 Python 和 Node.js 版本"
            } else {
                "正在校验 Python 版本"
            },
            progress.validation,
        );
        let python = find_binary(
            &staging.join("python"),
            &["python3.12", "python.exe", "python"],
        );
        let node = find_binary(&staging.join("node"), &["node", "node.exe"]);
        if requirements.python
            && python
                .as_ref()
                .and_then(|path| probe_version(path))
                .as_deref()
                .map(|version| !version_matches(Some(version), PYTHON_VERSION))
                .unwrap_or(true)
        {
            return Err(format!("私有 Python 运行时校验失败，需要 {PYTHON_VERSION}"));
        }
        if requirements.node
            && node
                .as_ref()
                .and_then(|path| probe_version(path))
                .as_deref()
                .map(|version| !version_matches(Some(version), NODE_VERSION))
                .unwrap_or(true)
        {
            return Err(format!("私有 Node.js 运行时校验失败，需要 {NODE_VERSION}"));
        }

        check_cancelled(app)?;
        emit(
            app,
            validation_phase,
            "running",
            "正在提交基础运行时 generation",
            progress.commit,
        );
        write_text_file(&staging.join("generation.complete"), "ok\n")?;
        fs::rename(&staging, &final_path)
            .map_err(|error| format!("无法提交运行时 generation: {error}"))?;
        sync_directory(&runtime.generations)?;

        let manifest = RuntimeManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            platform: platform_name(),
            arch: architecture_name(),
            runtime_revision: RUNTIME_REVISION.to_string(),
            python_version: PYTHON_VERSION.to_string(),
            node_version: NODE_VERSION.to_string(),
            state: "ready".to_string(),
            active_generation: Some(generation.clone()),
            python_generation: if target == Some("node") {
                previous_manifest
                    .as_ref()
                    .and_then(|manifest| manifest.python_generation.clone())
                    .or_else(|| transaction.previous_generation.clone())
            } else {
                Some(generation.clone())
            },
            node_generation: if target == Some("python") {
                previous_manifest
                    .as_ref()
                    .and_then(|manifest| manifest.node_generation.clone())
                    .or_else(|| transaction.previous_generation.clone())
            } else {
                Some(generation.clone())
            },
            requirements: requirements.clone(),
        };
        write_manifest(runtime, &manifest)?;
        clear_transaction(runtime)?;
        // Cleanup is post-commit housekeeping. A failure here must not make a
        // successfully committed generation look like a failed installation.
        let _ = prune_generations(
            runtime,
            &generation,
            transaction.previous_generation.as_deref(),
        );
        Ok(generation)
    })();

    if let Err(error) = &result {
        let failed = RuntimeTransaction {
            state: "failed".to_string(),
            updated_at: now(),
            error: Some(error.clone()),
            ..transaction
        };
        let _ = write_transaction(runtime, &failed);
    }
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

pub(crate) fn install(app: &AppHandle, repair: bool) -> Result<RuntimeSnapshot, String> {
    install_with_requirements(app, repair, None, runtime_policy(), InstallProgress::full())
}

pub(crate) fn install_component(
    app: &AppHandle,
    component: &str,
) -> Result<RuntimeSnapshot, String> {
    install_with_requirements(
        app,
        true,
        Some(component),
        runtime_policy(),
        InstallProgress::full(),
    )
}

pub(crate) fn install_runtime_stage(
    app: &AppHandle,
    repair: bool,
    component: &str,
    progress_range: ProgressRange,
) -> Result<RuntimeSnapshot, String> {
    let requirements = match component {
        "python" => RuntimeRequirements {
            python: true,
            node: false,
        },
        "node" => runtime_policy(),
        _ => return Err(format!("不支持准备运行时组件: {component}")),
    };
    install_with_requirements(
        app,
        repair,
        Some(component),
        requirements,
        InstallProgress::single_component(component, progress_range),
    )
}

fn install_with_requirements(
    app: &AppHandle,
    repair: bool,
    target: Option<&str>,
    requirements: RuntimeRequirements,
    progress: InstallProgress,
) -> Result<RuntimeSnapshot, String> {
    let runtime = paths(app)?;
    let _lock = acquire_lock(&runtime)?;
    emit(app, "preflight", "running", "正在检查运行时安装策略", 5);
    let artifact_check = (|| {
        if requirements.python && target != Some("node") {
            python_artifact()?;
        }
        if requirements.node && target != Some("python") {
            node_artifact()?;
        }
        Ok::<(), String>(())
    })();
    if let Err(error) = artifact_check {
        emit_detailed_with_metadata(
            app,
            "preflight",
            "failed",
            &error,
            100,
            None,
            None,
            None,
            None,
            0,
            Some("unsupported_platform".to_string()),
        );
        return Err(error);
    }
    let install_phase = target.unwrap_or("install");
    emit(
        app,
        install_phase,
        "running",
        if repair {
            "正在创建新的运行时 generation 以修复当前环境"
        } else {
            "正在创建新的运行时 generation"
        },
        progress
            .python
            .start
            .min(progress.node.start)
            .min(progress.validation),
    );
    let generation_result =
        install_generation_with_target(app, &runtime, &requirements, target, progress);
    match generation_result {
        Ok(_) => {
            let (phase, message) = if requirements.node {
                ("node", "基础 Python 和 Node.js 运行时已准备完成")
            } else {
                ("python", "基础 Python 运行时已准备完成")
            };
            emit(app, phase, "completed", message, progress.commit);
            inspect_with_requirements(app, requirements)
        }
        Err(error) => {
            let cancelled = error == "运行时任务已取消";
            emit_detailed_with_metadata(
                app,
                if cancelled { "cancelled" } else { "install" },
                if cancelled { "cancelled" } else { "failed" },
                &error,
                100,
                None,
                None,
                None,
                None,
                0,
                Some(if cancelled {
                    "cancelled".to_string()
                } else {
                    "install_failed".to_string()
                }),
            );
            Err(error)
        }
    }
}
