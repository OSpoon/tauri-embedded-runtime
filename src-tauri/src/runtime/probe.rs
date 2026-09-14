use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::AppHandle;

use super::operation::active_operation;
use super::storage::{paths, safe_generation_path, write_manifest};
use super::types::{
    RuntimeComponent, RuntimeManifest, RuntimePaths, RuntimeRequirements, RuntimeSnapshot,
    MANIFEST_SCHEMA_VERSION, NODE_VERSION, PYTHON_VERSION, RUNTIME_REVISION,
};
use super::util::{architecture_name, now, platform_name};

/// Both runtimes are enabled in the default example so the complete lifecycle
/// is exercised instead of leaving the runtime manager untested.
pub(crate) fn runtime_policy() -> RuntimeRequirements {
    RuntimeRequirements {
        python: true,
        node: true,
    }
}

pub(crate) fn find_binary(root: &Path, names: &[&str]) -> Option<PathBuf> {
    if !root.is_dir() {
        return None;
    }
    for entry in fs::read_dir(root).ok()?.flatten() {
        let path = entry.path();
        let file_type = entry.file_type().ok()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_file()
            && path
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| names.contains(&value))
                .unwrap_or(false)
        {
            return Some(path);
        }
        if file_type.is_dir() {
            if let Some(found) = find_binary(&path, names) {
                return Some(found);
            }
        }
    }
    None
}

pub(crate) fn binary_paths(
    runtime: &RuntimePaths,
    generation: Option<&str>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    let Some(generation) = generation.and_then(|value| safe_generation_path(runtime, value)) else {
        return (None, None);
    };
    let python = find_binary(
        &generation.join("python"),
        &["python3.12", "python.exe", "python"],
    );
    let node = find_binary(&generation.join("node"), &["node", "node.exe"]);
    (python, node)
}

pub(crate) fn component(
    required: bool,
    path: Option<PathBuf>,
    expected_version: &str,
) -> RuntimeComponent {
    let version = path.as_ref().and_then(|value| probe_version(value));
    let mut issues = Vec::new();
    let status = if !required {
        "disabled"
    } else if path.is_none() {
        issues.push("运行时可执行文件不存在".to_string());
        "missing"
    } else if version.is_none() {
        issues.push("运行时可执行文件无法执行 --version".to_string());
        "corrupted"
    } else if !version_matches(version.as_deref(), expected_version) {
        issues.push(format!(
            "版本不匹配：当前 {}，需要 {}",
            version.as_deref().unwrap_or("未知"),
            expected_version
        ));
        "outdated"
    } else {
        "ready"
    };
    RuntimeComponent {
        required,
        present: path.is_some() && version.is_some(),
        status: status.to_string(),
        path: path.map(|value| value.to_string_lossy().into_owned()),
        version,
        issues,
    }
}

pub(crate) fn component_matches_version(component: &RuntimeComponent, expected: &str) -> bool {
    component.status == "ready" && version_matches(component.version.as_deref(), expected)
}

pub(crate) fn version_matches(actual: Option<&str>, expected: &str) -> bool {
    let expected = expected.trim_start_matches('v');
    actual
        .unwrap_or_default()
        .split_whitespace()
        .map(|part| {
            part.trim_start_matches('v')
                .trim_matches(|value: char| !value.is_ascii_digit() && value != '.')
        })
        .any(|part| part == expected)
}

pub(crate) fn probe_version(binary: &Path) -> Option<String> {
    let output = Command::new(binary)
        .arg("--version")
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME")
        .env_remove("VIRTUAL_ENV")
        .env_remove("CONDA_PREFIX")
        .env_remove("CONDA_DEFAULT_ENV")
        .env_remove("NODE_PATH")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let value = if stdout.is_empty() { stderr } else { stdout };
    (!value.is_empty()).then_some(value)
}

pub(crate) fn manifest_matches_policy(
    manifest: &RuntimeManifest,
    requirements: &RuntimeRequirements,
) -> bool {
    manifest.schema_version == MANIFEST_SCHEMA_VERSION
        && manifest.platform == platform_name()
        && manifest.arch == architecture_name()
        && manifest.runtime_revision == RUNTIME_REVISION
        && manifest.python_version == PYTHON_VERSION
        && manifest.node_version == NODE_VERSION
        && manifest.requirements.python == requirements.python
        && manifest.requirements.node == requirements.node
}

pub(crate) fn backfill_component_generations(
    manifest: &mut RuntimeManifest,
    requirements: &RuntimeRequirements,
) -> bool {
    let Some(active_generation) = manifest.active_generation.clone() else {
        return false;
    };
    let mut changed = false;
    if requirements.python && manifest.python_generation.is_none() {
        manifest.python_generation = Some(active_generation.clone());
        changed = true;
    }
    if requirements.node && manifest.node_generation.is_none() {
        manifest.node_generation = Some(active_generation);
        changed = true;
    }
    changed
}

pub(crate) fn recoverable_generation(
    runtime: &RuntimePaths,
    requirements: &RuntimeRequirements,
) -> Option<String> {
    let mut generations = fs::read_dir(&runtime.generations)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !path.is_dir() || !name.starts_with("generation-") {
                return None;
            }
            if !path.join("generation.complete").is_file() {
                return None;
            }
            let (python, node) = binary_paths(runtime, Some(&name));
            let python_ok = !requirements.python
                || python
                    .as_ref()
                    .and_then(|value| probe_version(value))
                    .as_deref()
                    .map(|value| version_matches(Some(value), PYTHON_VERSION))
                    .unwrap_or(false);
            let node_ok = !requirements.node
                || node
                    .as_ref()
                    .and_then(|value| probe_version(value))
                    .as_deref()
                    .map(|value| version_matches(Some(value), NODE_VERSION))
                    .unwrap_or(false);
            (python_ok && node_ok).then_some(name)
        })
        .collect::<Vec<_>>();
    generations.sort();
    generations.pop()
}

pub(crate) fn inspect(app: &AppHandle) -> Result<RuntimeSnapshot, String> {
    inspect_with_requirements(app, runtime_policy())
}

pub(crate) fn inspect_with_requirements(
    app: &AppHandle,
    requirements: RuntimeRequirements,
) -> Result<RuntimeSnapshot, String> {
    let runtime = paths(app)?;
    super::storage::ensure_layout(&runtime)?;
    let operation = active_operation(app);
    let recovered_transaction = if operation.is_none() {
        super::storage::recover_interrupted_layout(&runtime)?
    } else {
        None
    };
    let mut manifest = super::storage::read_manifest(&runtime);
    let manifest_generation_is_valid = manifest.as_ref().is_some_and(|value| {
        manifest_matches_policy(value, &requirements)
            && value.state == "ready"
            && value
                .active_generation
                .as_deref()
                .and_then(|generation| safe_generation_path(&runtime, generation))
                .map(|generation| generation.join("generation.complete").is_file())
                .unwrap_or(false)
    });
    if !manifest_generation_is_valid {
        if let Some(generation) = recoverable_generation(&runtime, &requirements) {
            let recovered = RuntimeManifest {
                schema_version: MANIFEST_SCHEMA_VERSION,
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                platform: platform_name(),
                arch: architecture_name(),
                runtime_revision: RUNTIME_REVISION.to_string(),
                python_version: PYTHON_VERSION.to_string(),
                node_version: NODE_VERSION.to_string(),
                state: "ready".to_string(),
                active_generation: Some(generation.clone()),
                python_generation: Some(generation.clone()),
                node_generation: Some(generation),
                requirements: requirements.clone(),
            };
            write_manifest(&runtime, &recovered)?;
            manifest = Some(recovered);
        }
    }
    // Manifests written before component generations were introduced deserialize
    // with empty fields. Backfill them from the validated active generation so
    // project environments do not look outdated on every subsequent launch.
    if manifest_generation_is_valid {
        if let Some(existing) = manifest.as_mut() {
            if backfill_component_generations(existing, &requirements) {
                write_manifest(&runtime, existing)?;
            }
        }
    }
    let generation = manifest
        .as_ref()
        .and_then(|value| value.active_generation.clone());
    let (python_path, node_path) = binary_paths(&runtime, generation.as_deref());
    let python = component(requirements.python, python_path, PYTHON_VERSION);
    let node = component(requirements.node, node_path, NODE_VERSION);
    let projects = super::project::project_snapshots(&runtime)?;
    let generation_is_complete = generation
        .as_deref()
        .and_then(|value| safe_generation_path(&runtime, value))
        .map(|value| value.join("generation.complete").is_file())
        .unwrap_or(false);
    let manifest_is_compatible = manifest
        .as_ref()
        .map(|value| manifest_matches_policy(value, &requirements))
        .unwrap_or(false);

    let status = if manifest.is_none() {
        "missing"
    } else if !manifest_is_compatible
        || !generation_is_complete
        || (requirements.python && !component_matches_version(&python, PYTHON_VERSION))
        || (requirements.node && !component_matches_version(&node, NODE_VERSION))
    {
        if python.status == "outdated" || node.status == "outdated" {
            "outdated"
        } else {
            "corrupted"
        }
    } else {
        "ready"
    };
    let message = match status {
        "ready" => "Python 和 Node.js 私有运行时检查通过".to_string(),
        "missing" => "应用运行时尚未安装".to_string(),
        "corrupted" => "应用运行时不完整，需要修复".to_string(),
        "outdated" => "应用运行时版本不符合当前策略，需要更新".to_string(),
        _ => "应用运行时状态未知".to_string(),
    };
    let mut issues = python
        .issues
        .iter()
        .chain(node.issues.iter())
        .cloned()
        .collect::<Vec<_>>();
    if let Some(transaction) = recovered_transaction {
        issues.push(
            transaction
                .error
                .unwrap_or_else(|| "上一次运行时事务未正常完成，已保留旧环境".to_string()),
        );
    }
    if !generation_is_complete {
        issues.push("当前 active generation 缺少完成标记".to_string());
    }
    Ok(RuntimeSnapshot {
        status: status.to_string(),
        platform: platform_name(),
        arch: architecture_name(),
        runtime_root: runtime.root.to_string_lossy().into_owned(),
        active_generation: generation,
        runtime_revision: RUNTIME_REVISION.to_string(),
        requirements,
        python,
        node,
        projects,
        issues,
        message,
        checked_at: now(),
        operation_id: operation.as_ref().map(|value| value.id.clone()),
        operation_status: if operation.is_some() {
            "running".to_string()
        } else {
            "idle".to_string()
        },
    })
}
