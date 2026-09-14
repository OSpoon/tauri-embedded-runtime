use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

use super::artifacts::hex_digest;
use super::events::emit_detailed_with_metadata;
use super::operation::is_cancelled;
use super::plan::ProgressRange;
use super::probe::find_binary;
use super::process::{prepare_command, terminate};
use super::projects::{service_for_id, PROJECT_MODULES};
use super::storage::acquire_lock;
use super::storage::{ensure_layout, paths, read_manifest, safe_generation_path};
use super::types::{ProjectSnapshot, RuntimePaths, RuntimeSnapshot, MANIFEST_SCHEMA_VERSION};
use super::util::{now, runtime_bin_dir, sync_directory, unique_token};

#[derive(Debug, Clone, Deserialize)]
struct ProjectProfile {
    schema_version: u32,
    project_id: String,
    service: String,
    framework: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    entrypoint: String,
    #[serde(default)]
    launch_args: Vec<String>,
    #[serde(default)]
    health_path: String,
    #[serde(default)]
    demo_path: Option<String>,
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    package_name: Option<String>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    tools: Vec<ProjectToolSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProjectToolSpec {
    name: String,
    required: bool,
    artifact_id: Option<String>,
    executable: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectManifest {
    schema_version: u32,
    project_id: String,
    dependency_revision: String,
    #[serde(default)]
    base_python_generation: Option<String>,
    #[serde(default)]
    base_node_generation: Option<String>,
    state: String,
    active_generation: Option<String>,
    updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectTransaction {
    schema_version: u32,
    project_id: String,
    state: String,
    generation: String,
    previous_generation: Option<String>,
    dependency_revision: String,
    #[serde(default)]
    base_python_generation: Option<String>,
    #[serde(default)]
    base_node_generation: Option<String>,
    started_at: u64,
    updated_at: u64,
    error: Option<String>,
}

pub(crate) struct ProjectPaths {
    pub(crate) root: PathBuf,
    pub(crate) manifest: PathBuf,
    pub(crate) transaction: PathBuf,
    pub(crate) generations: PathBuf,
}

pub(crate) struct ProjectEnvironment {
    pub(crate) project_id: String,
    pub(crate) root: PathBuf,
    pub(crate) generation: PathBuf,
    pub(crate) service: String,
    pub(crate) display_name: String,
    pub(crate) entrypoint: String,
    pub(crate) launch_args: Vec<String>,
    pub(crate) health_path: String,
    pub(crate) python: Option<PathBuf>,
    pub(crate) node_modules: Option<PathBuf>,
    pub(crate) ffmpeg: Option<PathBuf>,
}

struct ProjectPreparation<'a> {
    base_generation: &'a Path,
    base_python: &'a Path,
    base_node: Option<&'a Path>,
    progress: ProgressRange,
}

fn profile_source(project_id: &str) -> Option<&'static str> {
    PROJECT_MODULES
        .iter()
        .find(|module| module.project_id == project_id)
        .map(|module| module.profile_source)
}

fn service_source(project_id: &str) -> Option<&'static str> {
    PROJECT_MODULES
        .iter()
        .find(|module| module.project_id == project_id)
        .map(|module| module.service_source)
}

fn project_profile(project_id: &str) -> Result<ProjectProfile, String> {
    let module = PROJECT_MODULES
        .iter()
        .find(|module| module.project_id == project_id)
        .ok_or_else(|| format!("未知项目服务模块: {project_id}"))?;
    let source = module.profile_source;
    let mut profile = serde_json::from_str::<ProjectProfile>(source)
        .map_err(|error| format!("项目服务模块 {project_id} 清单无效: {error}"))?;
    if profile.schema_version != 1 || profile.project_id != project_id {
        return Err(format!("项目服务模块 {project_id} 清单版本不匹配"));
    }
    if !matches!(profile.service.as_str(), "python" | "node") {
        return Err(format!("项目服务模块 {project_id} 服务类型无效"));
    }
    if profile.service != module.service.id {
        return Err(format!(
            "项目服务模块 {project_id} 的 service 与注册表不一致"
        ));
    }
    if profile.service == "python" && profile.requirements.is_empty() {
        return Err(format!("项目服务模块 {project_id} 没有 Python 依赖"));
    }
    if profile.service == "node" && profile.dependencies.is_empty() {
        return Err(format!("项目服务模块 {project_id} 没有 Node.js 依赖"));
    }
    if profile.entrypoint.is_empty() {
        profile.entrypoint = if profile.service == "python" {
            "services/python_service.py".to_string()
        } else {
            "services/node_service.mjs".to_string()
        };
    }
    if profile.launch_args.is_empty() {
        profile.launch_args = vec![
            "{entrypoint}".to_string(),
            "--port".to_string(),
            "{port}".to_string(),
        ];
    }
    if profile.health_path.is_empty() {
        profile.health_path = "/health".to_string();
    }
    let entrypoint_path = Path::new(&profile.entrypoint);
    if entrypoint_path.is_absolute()
        || entrypoint_path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(format!("项目服务模块 {project_id} 入口路径无效"));
    }
    if !profile.health_path.starts_with('/')
        || profile.health_path.contains('\r')
        || profile.health_path.contains('\n')
    {
        return Err(format!("项目服务模块 {project_id} 健康检查路径无效"));
    }
    if profile
        .demo_path
        .as_deref()
        .is_some_and(|path| !path.starts_with('/') || path.contains('\r') || path.contains('\n'))
    {
        return Err(format!("项目服务模块 {project_id} 示例调用路径无效"));
    }
    Ok(profile)
}

fn dependency_revision(project_id: &str) -> Result<String, String> {
    let source =
        profile_source(project_id).ok_or_else(|| format!("未知项目服务模块: {project_id}"))?;
    let service =
        service_source(project_id).ok_or_else(|| format!("未知项目服务模块: {project_id}"))?;
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    hasher.update(b"\n--service-source--\n");
    hasher.update(service.as_bytes());
    Ok(hex_digest(&hasher.finalize()))
}

fn project_paths(runtime: &RuntimePaths, project_id: &str) -> ProjectPaths {
    let root = runtime.root.join("projects").join(project_id);
    ProjectPaths {
        manifest: root.join("manifest.json"),
        transaction: root.join("transaction.json"),
        generations: root.join("generations"),
        root,
    }
}

fn ensure_project_layout(project: &ProjectPaths) -> Result<(), String> {
    fs::create_dir_all(&project.generations)
        .map_err(|error| format!("无法创建项目环境目录: {error}"))?;
    Ok(())
}

pub(super) fn ensure_project_cache_layout(
    project: &ProjectPaths,
    service: &str,
) -> Result<(), String> {
    let (required_cache, obsolete_cache, service_label) = match service {
        "python" => ("pip", "npm", "Python"),
        "node" => ("npm", "pip", "Node.js"),
        _ => return Err(format!("项目服务类型不支持: {service}")),
    };
    let cache_root = project.root.join("cache");
    fs::create_dir_all(cache_root.join(required_cache))
        .map_err(|error| format!("无法创建 {service_label} 依赖缓存目录: {error}"))?;
    let obsolete_path = cache_root.join(obsolete_cache);
    if obsolete_path.exists() {
        fs::remove_dir_all(&obsolete_path)
            .map_err(|error| format!("无法清理错误的 {service_label} 项目缓存目录: {error}"))?;
    }
    Ok(())
}

fn recover_project_layout(project: &ProjectPaths) -> Result<(), String> {
    ensure_project_layout(project)?;
    for entry in fs::read_dir(&project.generations)
        .map_err(|error| format!("无法扫描项目环境 generation: {error}"))?
        .flatten()
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_directory = fs::symlink_metadata(&path)
            .map(|metadata| metadata.file_type().is_dir())
            .unwrap_or(false);
        if name.starts_with('.') && name.ends_with(".staging") && is_directory {
            fs::remove_dir_all(&path)
                .map_err(|error| format!("无法清理项目环境暂存目录: {error}"))?;
        }
    }
    for entry in fs::read_dir(&project.root)
        .map_err(|error| format!("无法扫描项目环境临时文件: {error}"))?
        .flatten()
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_file = fs::symlink_metadata(&path)
            .map(|metadata| metadata.file_type().is_file())
            .unwrap_or(false);
        if (name.starts_with("manifest.json.tmp-") || name.starts_with("transaction.json.tmp-"))
            && is_file
        {
            fs::remove_file(&path).map_err(|error| format!("无法清理项目环境临时文件: {error}"))?;
        }
    }
    if let Some(mut transaction) = read_project_transaction(project) {
        if transaction.state == "running" {
            transaction.state = "interrupted".to_string();
            transaction.updated_at = now();
            transaction.error = Some(
                "上一次项目环境操作未正常完成，暂存目录已清理，旧 generation 保留".to_string(),
            );
            write_project_transaction(project, &transaction)?;
        }
    }
    Ok(())
}

fn read_project_manifest(project: &ProjectPaths) -> Option<ProjectManifest> {
    let content = fs::read_to_string(&project.manifest).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_project_manifest(
    project: &ProjectPaths,
    manifest: &ProjectManifest,
) -> Result<(), String> {
    let temp = project
        .root
        .join(format!("manifest.json.tmp-{}", unique_token()));
    let content = serde_json::to_vec_pretty(manifest)
        .map_err(|error| format!("无法序列化项目环境 manifest: {error}"))?;
    {
        let mut file =
            File::create(&temp).map_err(|error| format!("无法创建项目环境 manifest: {error}"))?;
        file.write_all(&content)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("无法落盘项目环境 manifest: {error}"))?;
    }
    fs::rename(&temp, &project.manifest)
        .map_err(|error| format!("无法提交项目环境 manifest: {error}"))?;
    sync_directory(&project.root)
}

fn write_project_transaction(
    project: &ProjectPaths,
    transaction: &ProjectTransaction,
) -> Result<(), String> {
    let temp = project
        .root
        .join(format!("transaction.json.tmp-{}", unique_token()));
    let content = serde_json::to_vec_pretty(transaction)
        .map_err(|error| format!("无法序列化项目环境事务: {error}"))?;
    {
        let mut file =
            File::create(&temp).map_err(|error| format!("无法创建项目环境事务: {error}"))?;
        file.write_all(&content)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("无法落盘项目环境事务: {error}"))?;
    }
    fs::rename(&temp, &project.transaction)
        .map_err(|error| format!("无法提交项目环境事务: {error}"))?;
    sync_directory(&project.root)
}

fn clear_project_transaction(project: &ProjectPaths) -> Result<(), String> {
    if project.transaction.exists() {
        fs::remove_file(&project.transaction)
            .map_err(|error| format!("无法清理项目环境事务: {error}"))?;
        sync_directory(&project.root)?;
    }
    Ok(())
}

fn read_project_transaction(project: &ProjectPaths) -> Option<ProjectTransaction> {
    let content = fs::read_to_string(&project.transaction).ok()?;
    serde_json::from_str(&content).ok()
}

fn safe_project_generation_path(project: &ProjectPaths, generation: &str) -> Option<PathBuf> {
    let path = Path::new(generation);
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return None;
    }
    Some(project.generations.join(path))
}

fn project_python_path(generation: &Path) -> PathBuf {
    if cfg!(windows) {
        generation.join("python").join("Scripts").join("python.exe")
    } else {
        generation.join("python").join("bin").join("python")
    }
}

fn project_generation_is_complete(project: &ProjectPaths, generation: &str) -> bool {
    safe_project_generation_path(project, generation)
        .map(|path| {
            fs::symlink_metadata(path.join("generation.complete"))
                .map(|metadata| metadata.file_type().is_file())
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn tool_path(generation: &Path, tool: &ProjectToolSpec) -> Option<PathBuf> {
    find_binary(
        &generation.join("tools"),
        &[tool.executable.as_str(), "ffmpeg", "ffmpeg.exe"],
    )
}

fn required_tools_ready(profile: &ProjectProfile, generation: &Path) -> bool {
    profile
        .tools
        .iter()
        .filter(|tool| tool.required)
        .all(|tool| tool.artifact_id.is_some() && tool_path(generation, tool).is_some())
}

struct ProbeOutput {
    success: bool,
    output: String,
}

fn run_probe(mut command: Command, label: &str) -> Result<ProbeOutput, String> {
    prepare_command(&mut command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法执行 {label} 健康检查: {error}"))?;
    for _ in 0..30 {
        if child
            .try_wait()
            .map_err(|error| format!("无法读取 {label} 健康检查状态: {error}"))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("无法读取 {label} 健康检查输出: {error}"))?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Ok(ProbeOutput {
                success: output.status.success(),
                output: format!("{}{}", stdout.trim(), stderr.trim()),
            });
        }
        thread::sleep(Duration::from_millis(100));
    }
    terminate(&mut child);
    Err(format!("{label} 健康检查超时"))
}

fn package_name_from_requirement(requirement: &str) -> Option<String> {
    requirement
        .split(['=', '<', '>', '!', '~', '[', ';', ' '])
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.replace('-', "_"))
}

fn probe_python_dependencies(
    generation: &Path,
    python: &Path,
    requirements: &[String],
) -> Vec<String> {
    let modules = requirements
        .iter()
        .filter_map(|requirement| package_name_from_requirement(requirement))
        .collect::<Vec<_>>();
    let mut issues = Vec::new();
    if modules.is_empty() {
        return issues;
    }

    let import_script =
        "import importlib.util,sys; missing=[name for name in sys.argv[1:] if importlib.util.find_spec(name) is None]; print('\\n'.join(missing)); sys.exit(1 if missing else 0)";
    let mut import_command = Command::new(python);
    import_command
        .current_dir(generation)
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME")
        .env_remove("VIRTUAL_ENV")
        .env_remove("CONDA_PREFIX")
        .env_remove("CONDA_DEFAULT_ENV")
        .env("PYTHONNOUSERSITE", "1")
        .arg("-c")
        .arg(import_script)
        .args(&modules);
    match run_probe(import_command, "Python import") {
        Ok(result) if result.success => {}
        Ok(result) => issues.push(format!(
            "Python 依赖无法 import{}",
            if result.output.is_empty() {
                String::new()
            } else {
                format!("：{}", result.output)
            }
        )),
        Err(error) => issues.push(error),
    }

    let mut pip_command = Command::new(python);
    pip_command
        .current_dir(generation)
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME")
        .env_remove("VIRTUAL_ENV")
        .env("PYTHONNOUSERSITE", "1")
        .args(["-m", "pip", "check"]);
    match run_probe(pip_command, "pip check") {
        Ok(result) if result.success => {}
        Ok(result) => issues.push(format!(
            "pip check 未通过{}",
            if result.output.is_empty() {
                String::new()
            } else {
                format!("：{}", result.output)
            }
        )),
        Err(error) => issues.push(error),
    }
    issues
}

fn declared_node_version_matches(actual: &str, expected: &str) -> bool {
    let expected = expected.trim_start_matches(['^', '~', '=', ' ']);
    actual == expected
}

fn active_base_node(runtime: &RuntimePaths) -> Option<PathBuf> {
    let generation = read_manifest(runtime)
        .and_then(|manifest| manifest.active_generation)
        .and_then(|generation| safe_generation_path(runtime, &generation))?;
    find_binary(&generation.join("node"), &["node", "node.exe"])
}

fn node_dependency_path(generation: &Path, dependency: &str) -> PathBuf {
    generation.join("node_modules").join(dependency)
}

fn probe_node_dependencies(
    runtime: &RuntimePaths,
    generation: &Path,
    profile: &ProjectProfile,
) -> Vec<String> {
    let mut issues = Vec::new();
    let mut import_names = Vec::new();
    for (dependency, expected) in &profile.dependencies {
        let package_json = node_dependency_path(generation, dependency).join("package.json");
        let Ok(content) = fs::read_to_string(&package_json) else {
            issues.push(format!("Node.js 依赖 {dependency} 缺少 package.json"));
            continue;
        };
        let Ok(package) = serde_json::from_str::<serde_json::Value>(&content) else {
            issues.push(format!("Node.js 依赖 {dependency} 的 package.json 无效"));
            continue;
        };
        match package.get("version").and_then(|value| value.as_str()) {
            Some(actual) if declared_node_version_matches(actual, expected) => {}
            Some(actual) => issues.push(format!(
                "Node.js 依赖 {dependency} 版本不匹配：当前 {actual}，需要 {expected}"
            )),
            None => issues.push(format!("Node.js 依赖 {dependency} 缺少版本信息")),
        }
        import_names.push(dependency.clone());
    }

    let Some(node) = active_base_node(runtime) else {
        issues.push("无法执行 Node.js 依赖 import 检查：基础 Node.js 不可用".to_string());
        return issues;
    };
    if import_names.is_empty() {
        return issues;
    }
    let names = serde_json::to_string(&import_names).unwrap_or_else(|_| "[]".to_string());
    let script = format!("const names={names}; for (const name of names) await import(name);");
    let mut command = Command::new(node);
    command
        .current_dir(generation)
        .env_remove("NODE_PATH")
        .arg("--input-type=module")
        .arg("-e")
        .arg(script);
    match run_probe(command, "Node.js import") {
        Ok(result) if result.success => {}
        Ok(result) => issues.push(format!(
            "Node.js 依赖无法 import{}",
            if result.output.is_empty() {
                String::new()
            } else {
                format!("：{}", result.output)
            }
        )),
        Err(error) => issues.push(error),
    }
    issues
}

fn project_dependency_issues(
    runtime: &RuntimePaths,
    project: &ProjectPaths,
    profile: &ProjectProfile,
    generation_name: Option<&str>,
) -> Vec<String> {
    let Some(generation_name) = generation_name else {
        return vec!["没有 active generation".to_string()];
    };
    let Some(generation) = safe_project_generation_path(project, generation_name) else {
        return vec!["active generation 路径无效".to_string()];
    };
    let mut issues = Vec::new();
    if !project_generation_is_complete(project, generation_name) {
        issues.push("generation 缺少完成标记".to_string());
    }
    match profile.service.as_str() {
        "python" => {
            let python = project_python_path(&generation);
            if !python.is_file() {
                issues.push("Python venv 解释器不存在".to_string());
            } else {
                issues.extend(probe_python_dependencies(
                    &generation,
                    &python,
                    &profile.requirements,
                ));
            }
            if !generation.join("requirements.lock").is_file() {
                issues.push("requirements.lock 不存在".to_string());
            }
            if !generation.join(&profile.entrypoint).is_file() {
                issues.push(format!(
                    "{} 服务入口不存在",
                    profile
                        .display_name
                        .as_deref()
                        .unwrap_or(&profile.framework)
                ));
            }
        }
        "node" => {
            if !generation.join("node_modules").is_dir() {
                issues.push("node_modules 不存在".to_string());
            } else {
                issues.extend(probe_node_dependencies(runtime, &generation, profile));
            }
            if !generation.join("package.json").is_file() {
                issues.push("package.json 不存在".to_string());
            }
            if !generation.join("package-lock.json").is_file() {
                issues.push("package-lock.json 不存在".to_string());
            }
            if !generation.join(&profile.entrypoint).is_file() {
                issues.push(format!(
                    "{} 服务入口不存在",
                    profile
                        .display_name
                        .as_deref()
                        .unwrap_or(&profile.framework)
                ));
            }
        }
        _ => issues.push("项目服务类型不支持".to_string()),
    }
    issues
}

fn project_dependencies_ready(
    project: &ProjectPaths,
    profile: &ProjectProfile,
    generation: &str,
) -> bool {
    let Some(path) = safe_project_generation_path(project, generation) else {
        return false;
    };
    if !project_generation_is_complete(project, generation) {
        return false;
    }
    match profile.service.as_str() {
        "python" => {
            project_python_path(&path).is_file()
                && path.join("requirements.lock").is_file()
                && path.join(&profile.entrypoint).is_file()
        }
        "node" => {
            path.join("node_modules").is_dir()
                && path.join("package.json").is_file()
                && path.join("package-lock.json").is_file()
                && path.join(&profile.entrypoint).is_file()
        }
        _ => false,
    }
}

fn project_snapshot_for(
    runtime: &RuntimePaths,
    project_id: &str,
) -> Result<ProjectSnapshot, String> {
    let profile = project_profile(project_id)?;
    let project = project_paths(runtime, project_id);
    ensure_project_layout(&project)?;
    ensure_project_cache_layout(&project, &profile.service)?;
    let revision = dependency_revision(project_id)?;
    let manifest = read_project_manifest(&project);
    let runtime_manifest = read_manifest(runtime);
    let base_python_generation = runtime_manifest
        .as_ref()
        .and_then(|manifest| manifest.python_generation.clone());
    let base_node_generation = runtime_manifest
        .as_ref()
        .and_then(|manifest| manifest.node_generation.clone());
    let active_generation = manifest
        .as_ref()
        .and_then(|manifest| manifest.active_generation.clone());
    let generation_ready = active_generation
        .as_deref()
        .map(|generation| project_dependencies_ready(&project, &profile, generation))
        .unwrap_or(false);
    let dependency_revision_matches = manifest
        .as_ref()
        .is_some_and(|manifest| manifest.dependency_revision == revision);
    let base_component_generation_matches = manifest.as_ref().is_some_and(|manifest| match profile
        .service
        .as_str()
    {
        "python" => {
            manifest.base_python_generation.is_some()
                && manifest.base_python_generation == base_python_generation
        }
        "node" => {
            manifest.base_node_generation.is_some()
                && manifest.base_node_generation == base_node_generation
        }
        _ => false,
    });
    let manifest_ready = manifest.as_ref().is_some_and(|manifest| {
        manifest.schema_version == 1
            && manifest.project_id == project_id
            && manifest.state == "ready"
            && dependency_revision_matches
            && base_component_generation_matches
    });
    let tools_ready = active_generation
        .as_deref()
        .and_then(|generation| safe_project_generation_path(&project, generation))
        .map(|generation| required_tools_ready(&profile, &generation))
        .unwrap_or(false);
    let dependency_issues = if manifest.is_some() && manifest_ready {
        project_dependency_issues(runtime, &project, &profile, active_generation.as_deref())
    } else {
        Vec::new()
    };
    let manifest_issues = if manifest.is_some() && !manifest_ready {
        let mut issues = Vec::new();
        if !dependency_revision_matches {
            issues.push("项目依赖清单已变化".to_string());
        }
        if !base_component_generation_matches {
            issues.push(format!(
                "基础 {} 运行时 generation 已变化，需要重建项目环境",
                if profile.service == "python" {
                    "Python"
                } else {
                    "Node.js"
                }
            ));
        }
        issues
    } else {
        Vec::new()
    };
    let interrupted = read_project_transaction(&project)
        .filter(|transaction| transaction.state == "interrupted")
        .and_then(|transaction| transaction.error);
    let status = if manifest.is_none() {
        "missing"
    } else if !manifest_ready {
        "outdated"
    } else if !generation_ready || !tools_ready || !dependency_issues.is_empty() {
        "corrupted"
    } else {
        "ready"
    };
    let tools = profile
        .tools
        .iter()
        .map(|tool| {
            if tool.required {
                tool.name.clone()
            } else {
                format!("{} (optional)", tool.name)
            }
        })
        .collect();
    Ok(ProjectSnapshot {
        project_id: project_id.to_string(),
        status: status.to_string(),
        active_generation,
        dependency_revision: revision,
        python_dependencies: profile.requirements,
        node_dependencies: profile.dependencies.into_keys().collect(),
        tools,
        message: match status {
            "ready" if interrupted.is_some() => format!(
                "{} 项目依赖可用；上一次操作中断，已保留当前 generation",
                profile.framework
            ),
            "ready" => format!("{} 项目依赖已准备完成", profile.framework),
            "missing" => format!("{} 项目依赖尚未安装", profile.framework),
            "outdated" if !base_component_generation_matches => {
                format!(
                    "基础 {} 运行时已更新，需要重新准备项目环境",
                    if profile.service == "python" {
                        "Python"
                    } else {
                        "Node.js"
                    }
                )
            }
            "outdated" => "项目依赖清单已变化，需要重新安装".to_string(),
            _ => "项目依赖环境不完整，需要修复".to_string(),
        },
        issues: manifest_issues
            .into_iter()
            .chain(dependency_issues)
            .chain(interrupted)
            .collect(),
    })
}

pub(crate) fn project_snapshots(runtime: &RuntimePaths) -> Result<Vec<ProjectSnapshot>, String> {
    ensure_layout(runtime)?;
    PROJECT_MODULES
        .iter()
        .map(|module| project_snapshot_for(runtime, module.project_id))
        .collect()
}

pub(crate) fn project_catalog() -> Result<Vec<super::types::RuntimeProjectInfo>, String> {
    PROJECT_MODULES
        .iter()
        .map(|module| {
            let profile = project_profile(module.project_id)?;
            let service = service_for_id(module.service.id)
                .ok_or_else(|| format!("项目服务模块 {} 未注册服务", module.project_id))?;
            Ok(super::types::RuntimeProjectInfo {
                project_id: module.project_id.to_string(),
                service: service.id.to_string(),
                service_label: service.display_name.to_string(),
                framework: profile.framework.clone(),
                display_name: profile
                    .display_name
                    .unwrap_or_else(|| profile.framework.clone()),
                health_path: profile.health_path,
                demo_path: profile.demo_path,
            })
        })
        .collect()
}

pub(crate) fn project_environment(
    runtime: &RuntimePaths,
    project_id: &str,
) -> Result<ProjectEnvironment, String> {
    let snapshot = project_snapshot_for(runtime, project_id)?;
    if snapshot.status != "ready" {
        return Err(format!("项目依赖尚未就绪: {}", snapshot.message));
    }
    let profile = project_profile(project_id)?;
    let project = project_paths(runtime, project_id);
    let generation_name = snapshot
        .active_generation
        .as_deref()
        .ok_or_else(|| format!("项目环境 {project_id} active generation 无效"))?;
    let generation = safe_project_generation_path(&project, generation_name)
        .ok_or_else(|| format!("项目环境 {project_id} generation 路径无效"))?;
    let python = (profile.service == "python").then(|| project_python_path(&generation));
    let node_modules = (profile.service == "node").then(|| generation.join("node_modules"));
    let ffmpeg = profile
        .tools
        .iter()
        .find(|tool| tool.name == "ffmpeg")
        .and_then(|tool| tool_path(&generation, tool));
    Ok(ProjectEnvironment {
        project_id: project_id.to_string(),
        root: project.root,
        generation,
        service: profile.service,
        display_name: profile
            .display_name
            .unwrap_or_else(|| profile.framework.clone()),
        entrypoint: profile.entrypoint,
        launch_args: profile.launch_args,
        health_path: profile.health_path,
        python,
        node_modules,
        ffmpeg,
    })
}

fn configure_private_env(
    command: &mut Command,
    base_generation: &Path,
    project_root: &Path,
    service: &str,
    extra_bin: Option<&Path>,
) {
    let delimiter = if cfg!(windows) { ";" } else { ":" };
    let node_bin = runtime_bin_dir(base_generation, "node");
    let python_bin = runtime_bin_dir(base_generation, "python");
    let mut path_entries = Vec::new();
    if let Some(extra_bin) = extra_bin {
        path_entries.push(extra_bin.to_string_lossy().into_owned());
    }
    path_entries.push(node_bin.to_string_lossy().into_owned());
    path_entries.push(python_bin.to_string_lossy().into_owned());
    command
        .env_clear()
        .env("PATH", path_entries.join(delimiter))
        .env("HOME", project_root)
        .env("RUNTIME_PROJECT_ROOT", project_root);
    match service {
        "python" => {
            command
                .env("PYTHONNOUSERSITE", "1")
                .env("PYTHONUTF8", "1")
                .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
                .env("PIP_NO_INPUT", "1")
                .env("PIP_CACHE_DIR", project_root.join("cache").join("pip"));
        }
        "node" => {
            command
                .env("npm_config_cache", project_root.join("cache").join("npm"))
                .env("npm_config_update_notifier", "false")
                .env("npm_config_fund", "false")
                .env("npm_config_audit", "false");
        }
        _ => {}
    }
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
}

fn emit_command_line(app: &AppHandle, phase: &str, label: &str, line: String, progress: u8) {
    let message = if line.is_empty() {
        format!("{label} 正在运行")
    } else {
        format!("{label}: {line}")
    };
    emit_detailed_with_metadata(
        app,
        phase,
        "running",
        &message,
        progress,
        Some(line),
        None,
        None,
        Some(label.to_string()),
        0,
        None,
    );
}

fn spawn_reader<R>(reader: R, sender: Sender<String>)
where
    R: std::io::Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            let _ = sender.send(line);
        }
    });
}

fn run_command_with_output(
    app: &AppHandle,
    phase: &str,
    label: &str,
    command: &mut Command,
    progress: u8,
) -> Result<(), String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    prepare_command(command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动 {label}: {error}"))?;
    let Some(stdout) = child.stdout.take() else {
        terminate(&mut child);
        return Err(format!("{label} 未提供标准输出"));
    };
    let Some(stderr) = child.stderr.take() else {
        terminate(&mut child);
        return Err(format!("{label} 未提供错误输出"));
    };
    let (sender, receiver) = mpsc::channel();
    spawn_reader(stdout, sender.clone());
    spawn_reader(stderr, sender);

    let status = loop {
        if is_cancelled(app) {
            terminate(&mut child);
            return Err("运行时任务已取消".to_string());
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("无法读取 {label} 状态: {error}"))?
        {
            break status;
        }
        if let Ok(line) = receiver.recv_timeout(Duration::from_millis(100)) {
            emit_command_line(app, phase, label, line, progress);
        }
    };
    for line in receiver {
        emit_command_line(app, phase, label, line, progress);
    }
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} 退出状态: {status}"))
    }
}

fn write_text_file(path: &Path, content: &str) -> Result<(), String> {
    let mut file = File::create(path).map_err(|error| format!("无法创建项目文件: {error}"))?;
    file.write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("无法写入项目文件: {error}"))
}

pub(crate) fn ensure_project_environments(
    app: &AppHandle,
    runtime_snapshot: &RuntimeSnapshot,
    force: bool,
) -> Result<Vec<ProjectSnapshot>, String> {
    ensure_project_environments_filtered(
        app,
        runtime_snapshot,
        force,
        None,
        true,
        ProgressRange::new(0, 100),
    )
}

pub(crate) fn ensure_project_environments_for_service(
    app: &AppHandle,
    runtime_snapshot: &RuntimeSnapshot,
    service: &str,
    force: bool,
) -> Result<Vec<ProjectSnapshot>, String> {
    if !matches!(service, "python" | "node") {
        return Err(format!("不支持修复的项目运行时类型: {service}"));
    }
    ensure_project_environments_filtered(
        app,
        runtime_snapshot,
        force,
        Some(service),
        true,
        ProgressRange::new(0, 100),
    )
}

pub(crate) fn ensure_project_environments_for_service_ordered(
    app: &AppHandle,
    runtime_snapshot: &RuntimeSnapshot,
    service: &str,
    force: bool,
    progress: ProgressRange,
) -> Result<Vec<ProjectSnapshot>, String> {
    if !matches!(service, "python" | "node") {
        return Err(format!("不支持准备的项目运行时类型: {service}"));
    }
    ensure_project_environments_filtered(
        app,
        runtime_snapshot,
        force,
        Some(service),
        false,
        progress,
    )
}

pub(crate) fn emit_project_tools_event(
    app: &AppHandle,
    snapshots: &[ProjectSnapshot],
    status: &str,
    progress: u8,
) {
    let output = if snapshots.is_empty() {
        "没有注册项目工具".to_string()
    } else {
        snapshots
            .iter()
            .map(|snapshot| {
                let tools = if snapshot.tools.is_empty() {
                    "无".to_string()
                } else {
                    snapshot.tools.join(", ")
                };
                format!("{}: {}", snapshot.project_id, tools)
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    emit_detailed_with_metadata(
        app,
        "project-tools",
        status,
        if status == "cached" {
            "项目工具已检查，跳过重复处理"
        } else {
            "项目工具检查完成"
        },
        progress,
        Some(output),
        None,
        None,
        None,
        0,
        None,
    );
}

fn ensure_project_environments_filtered(
    app: &AppHandle,
    runtime_snapshot: &RuntimeSnapshot,
    force: bool,
    service_filter: Option<&str>,
    emit_tools: bool,
    progress: ProgressRange,
) -> Result<Vec<ProjectSnapshot>, String> {
    let runtime = paths(app)?;
    let _lock = acquire_lock(&runtime)?;
    let base_generation = runtime_snapshot
        .active_generation
        .as_deref()
        .and_then(|generation| safe_generation_path(&runtime, generation))
        .ok_or_else(|| "基础运行时 active generation 无效".to_string())?;
    let base_python = runtime_snapshot
        .python
        .path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "基础 Python 解释器不存在".to_string())?;
    let base_node = runtime_snapshot.node.path.as_deref().map(PathBuf::from);

    let mut snapshots = Vec::new();
    for module in PROJECT_MODULES {
        let project_id = module.project_id;
        if service_filter.is_some_and(|service| {
            project_profile(project_id)
                .map(|profile| profile.service != service)
                .unwrap_or(true)
        }) {
            continue;
        }
        snapshots.push(ensure_project_environment_for_id(
            app,
            &runtime,
            &ProjectPreparation {
                base_generation: &base_generation,
                base_python: &base_python,
                base_node: base_node.as_deref(),
                progress,
            },
            project_id,
            force,
        )?);
    }
    if emit_tools {
        emit_project_tools_event(app, &snapshots, "completed", progress.end);
    }
    Ok(snapshots)
}

pub(crate) fn ensure_single_project_environment(
    app: &AppHandle,
    runtime_snapshot: &RuntimeSnapshot,
    project_id: &str,
    force: bool,
) -> Result<ProjectSnapshot, String> {
    if !PROJECT_MODULES
        .iter()
        .any(|module| module.project_id == project_id)
    {
        return Err(format!("未知项目服务模块: {project_id}"));
    }
    let runtime = paths(app)?;
    let _lock = acquire_lock(&runtime)?;
    let base_generation = runtime_snapshot
        .active_generation
        .as_deref()
        .and_then(|generation| safe_generation_path(&runtime, generation))
        .ok_or_else(|| "基础运行时 active generation 无效".to_string())?;
    let base_python = runtime_snapshot
        .python
        .path
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| "基础 Python 解释器不存在".to_string())?;
    let base_node = runtime_snapshot.node.path.as_deref().map(PathBuf::from);
    let snapshot = ensure_project_environment_for_id(
        app,
        &runtime,
        &ProjectPreparation {
            base_generation: &base_generation,
            base_python: &base_python,
            base_node: base_node.as_deref(),
            progress: ProgressRange::new(0, 100),
        },
        project_id,
        force,
    )?;
    emit_project_tools_event(app, std::slice::from_ref(&snapshot), "completed", 100);
    Ok(snapshot)
}

fn ensure_project_environment_for_id(
    app: &AppHandle,
    runtime: &RuntimePaths,
    preparation: &ProjectPreparation<'_>,
    project_id: &str,
    force: bool,
) -> Result<ProjectSnapshot, String> {
    let profile = project_profile(project_id)?;
    let current = project_snapshot_for(runtime, project_id)?;
    if !force && current.status == "ready" {
        let phase = if profile.service == "python" {
            "project-python"
        } else {
            "project-node"
        };
        let dependency_output = if profile.service == "python" {
            profile.requirements.join(", ")
        } else {
            profile
                .dependencies
                .iter()
                .map(|(name, version)| format!("{name}@{version}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        emit_detailed_with_metadata(
            app,
            phase,
            "cached",
            &format!("{} 环境已就绪，跳过依赖安装", profile.framework),
            preparation.progress.end,
            Some(dependency_output),
            None,
            None,
            Some(project_id.to_string()),
            0,
            None,
        );
        return Ok(current);
    }
    install_project_environment(app, runtime, preparation, project_id)?;
    let prepared = project_snapshot_for(runtime, project_id)?;
    if prepared.status != "ready" {
        let details = if prepared.issues.is_empty() {
            prepared.message.clone()
        } else {
            prepared.issues.join("；")
        };
        return Err(format!(
            "{} 项目环境安装后校验未通过: {details}",
            prepared.project_id
        ));
    }
    Ok(prepared)
}

fn install_project_environment(
    app: &AppHandle,
    runtime: &RuntimePaths,
    preparation: &ProjectPreparation<'_>,
    project_id: &str,
) -> Result<(), String> {
    let profile = project_profile(project_id)?;
    let project = project_paths(runtime, project_id);
    ensure_project_layout(&project)?;
    ensure_project_cache_layout(&project, &profile.service)?;
    recover_project_layout(&project)?;

    let generation = format!("project-generation-{}", unique_token());
    let staging = project.generations.join(format!(".{}.staging", generation));
    let final_path = project.generations.join(&generation);
    fs::create_dir_all(&staging).map_err(|error| format!("无法创建项目环境暂存目录: {error}"))?;
    let dependency_revision = dependency_revision(project_id)?;
    let service_source = service_source(project_id)
        .ok_or_else(|| format!("项目服务模块 {project_id} 脚本不存在"))?;
    let base_generation_name = preparation
        .base_generation
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .ok_or_else(|| "基础运行时 generation 名称无效".to_string())?;
    let base_manifest = read_manifest(runtime);
    let base_python_generation = base_manifest
        .as_ref()
        .and_then(|manifest| manifest.python_generation.clone())
        .or_else(|| Some(base_generation_name.clone()));
    let base_node_generation = base_manifest
        .as_ref()
        .and_then(|manifest| manifest.node_generation.clone())
        .or_else(|| Some(base_generation_name.clone()));
    let transaction = ProjectTransaction {
        schema_version: MANIFEST_SCHEMA_VERSION,
        project_id: project_id.to_string(),
        state: "running".to_string(),
        generation: generation.clone(),
        previous_generation: read_project_manifest(&project)
            .and_then(|manifest| manifest.active_generation),
        dependency_revision: dependency_revision.clone(),
        base_python_generation: base_python_generation.clone(),
        base_node_generation: base_node_generation.clone(),
        started_at: now(),
        updated_at: now(),
        error: None,
    };
    if let Err(error) = write_project_transaction(&project, &transaction) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }

    let result = (|| {
        match profile.service.as_str() {
            "python" => {
                emit_detailed_with_metadata(
                    app,
                    "project-python",
                    "running",
                    &format!("正在创建 {} 项目 Python venv", profile.framework),
                    preparation.progress.at(10),
                    Some(format!("{} -m venv", preparation.base_python.display())),
                    None,
                    None,
                    Some(project_id.to_string()),
                    0,
                    None,
                );
                let mut venv_command = Command::new(preparation.base_python);
                configure_private_env(
                    &mut venv_command,
                    preparation.base_generation,
                    &project.root,
                    "python",
                    None,
                );
                venv_command
                    .current_dir(&staging)
                    .arg("-m")
                    .arg("venv")
                    .arg(staging.join("python"));
                run_command_with_output(
                    app,
                    "project-python",
                    "python venv",
                    &mut venv_command,
                    preparation.progress.at(48),
                )?;

                let project_python = project_python_path(&staging);
                if !project_python.is_file() {
                    return Err("Python venv 创建成功但解释器不存在".to_string());
                }
                let requirements_path = staging.join("requirements.lock");
                write_text_file(
                    &requirements_path,
                    &format!("{}\n", profile.requirements.join("\n")),
                )?;
                let venv_bin = project_python
                    .parent()
                    .ok_or_else(|| "Python venv bin 目录无效".to_string())?;
                let mut pip_command = Command::new(&project_python);
                configure_private_env(
                    &mut pip_command,
                    preparation.base_generation,
                    &project.root,
                    "python",
                    Some(venv_bin),
                );
                pip_command
                    .current_dir(&staging)
                    .args([
                        "-m",
                        "pip",
                        "install",
                        "--disable-pip-version-check",
                        "--no-input",
                        "--no-compile",
                        "-r",
                    ])
                    .arg(&requirements_path);
                run_command_with_output(
                    app,
                    "project-python",
                    "pip install",
                    &mut pip_command,
                    preparation.progress.at(78),
                )?;
                let entrypoint = staging.join(&profile.entrypoint);
                if let Some(parent) = entrypoint.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| format!("无法创建项目服务目录: {error}"))?;
                }
                write_text_file(&entrypoint, service_source)?;
            }
            "node" => {
                let base_node = preparation
                    .base_node
                    .ok_or_else(|| "基础 Node.js 解释器不存在".to_string())?;
                let dependency_output = profile
                    .dependencies
                    .iter()
                    .map(|(name, version)| format!("{name}@{version}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let package_json = json!({
                    "name": profile.package_name.clone().unwrap_or_else(|| project_id.to_string()),
                    "private": true,
                    "type": "module",
                    "dependencies": profile.dependencies,
                });
                write_text_file(
                    &staging.join("package.json"),
                    &format!("{}\n", serde_json::to_string_pretty(&package_json).unwrap()),
                )?;
                let npm_cli = find_binary(preparation.base_generation, &["npm-cli.js"])
                    .ok_or_else(|| "私有 Node.js 运行时未包含 npm-cli.js".to_string())?;
                let mut npm_command = Command::new(base_node);
                configure_private_env(
                    &mut npm_command,
                    preparation.base_generation,
                    &project.root,
                    "node",
                    None,
                );
                npm_command
                    .current_dir(&staging)
                    .arg(&npm_cli)
                    .args([
                        "install",
                        "--ignore-scripts",
                        "--no-audit",
                        "--no-fund",
                        "--package-lock=true",
                        "--prefix",
                    ])
                    .arg(&staging);
                emit_detailed_with_metadata(
                    app,
                    "project-node",
                    "running",
                    &format!("正在安装 {} 项目 Node.js 依赖", profile.framework),
                    preparation.progress.at(10),
                    Some(format!("npm install {dependency_output}")),
                    None,
                    None,
                    Some(project_id.to_string()),
                    0,
                    None,
                );
                run_command_with_output(
                    app,
                    "project-node",
                    "npm install",
                    &mut npm_command,
                    preparation.progress.at(72),
                )?;
                if !staging.join("package-lock.json").is_file() {
                    return Err("npm install 未生成 package-lock.json".to_string());
                }
                let entrypoint = staging.join(&profile.entrypoint);
                if let Some(parent) = entrypoint.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| format!("无法创建项目服务目录: {error}"))?;
                }
                write_text_file(&entrypoint, service_source)?;
            }
            _ => return Err(format!("项目服务模块 {project_id} 类型不支持")),
        }

        write_text_file(&staging.join("generation.complete"), "ok\n")?;
        fs::rename(&staging, &final_path)
            .map_err(|error| format!("无法提交项目环境 generation: {error}"))?;
        sync_directory(&project.generations)?;
        let manifest = ProjectManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            project_id: project_id.to_string(),
            dependency_revision: dependency_revision.clone(),
            base_python_generation: base_python_generation.clone(),
            base_node_generation: base_node_generation.clone(),
            state: "ready".to_string(),
            active_generation: Some(generation.clone()),
            updated_at: now(),
        };
        write_project_manifest(&project, &manifest)?;
        clear_project_transaction(&project)?;
        let previous_generation = transaction.previous_generation.as_deref();
        prune_project_generations(&project, &generation, previous_generation)?;
        emit_detailed_with_metadata(
            app,
            if profile.service == "python" {
                "project-python"
            } else {
                "project-node"
            },
            "completed",
            &format!("{} 项目环境已准备完成", profile.framework),
            preparation.progress.end,
            Some(final_path.display().to_string()),
            None,
            None,
            Some(project_id.to_string()),
            0,
            None,
        );
        Ok(())
    })();

    if let Err(error) = &result {
        let failed = ProjectTransaction {
            state: "failed".to_string(),
            updated_at: now(),
            error: Some(error.clone()),
            ..transaction
        };
        let _ = write_project_transaction(&project, &failed);
    }
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

fn prune_project_generations(
    project: &ProjectPaths,
    active_generation: &str,
    previous_generation: Option<&str>,
) -> Result<(), String> {
    let mut protected = std::collections::HashSet::from([active_generation.to_string()]);
    if let Some(previous_generation) = previous_generation {
        protected.insert(previous_generation.to_string());
    }
    let mut generations = fs::read_dir(&project.generations)
        .map_err(|error| format!("无法扫描待清理的项目环境 generation: {error}"))?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let complete = fs::symlink_metadata(&path)
                .map(|metadata| metadata.file_type().is_dir())
                .unwrap_or(false)
                && fs::symlink_metadata(path.join("generation.complete"))
                    .map(|metadata| metadata.file_type().is_file())
                    .unwrap_or(false);
            (name.starts_with("project-generation-") && complete).then_some((name, path))
        })
        .collect::<Vec<_>>();
    generations.sort_by(|left, right| left.0.cmp(&right.0));
    protected.extend(
        generations
            .iter()
            .rev()
            .take(2)
            .map(|(name, _)| name.clone()),
    );
    for (name, path) in generations {
        if !protected.contains(&name) {
            fs::remove_dir_all(&path)
                .map_err(|error| format!("无法清理旧项目环境 generation: {error}"))?;
        }
    }
    Ok(())
}
