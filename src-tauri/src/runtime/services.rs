use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

use super::events::{emit, emit_detailed_with_metadata};
use super::operation::{check_cancelled, is_cancelled};
use super::probe::{binary_paths, inspect};
use super::process::{prepare_command, terminate, terminate_orphaned_processes};
use super::project::{project_environment, ProjectEnvironment};
use super::projects::PROJECT_MODULES;
use super::storage::{paths, safe_generation_path};
use super::types::{RunningService, RuntimePaths, ServiceSnapshot, ServiceState};
use super::util::runtime_bin_dir;

impl Drop for ServiceState {
    fn drop(&mut self) {
        if Arc::strong_count(&self.children) == 1 {
            if let Ok(mut children) = self.children.lock() {
                stop_children(&mut children);
            }
        }
    }
}

fn reserve_port() -> Result<u16, String> {
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|error| format!("无法分配本地服务端口: {error}"))
}

fn health_check(port: u16, health_path: &str) -> bool {
    let address = match format!("127.0.0.1:{port}").parse() {
        Ok(address) => address,
        Err(_) => return false,
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(250)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let request =
        format!("GET {health_path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = Vec::new();
    if stream.read_to_end(&mut response).is_err() {
        return false;
    }
    String::from_utf8_lossy(&response).contains(" 200 ")
}

fn wait_for_health(
    app: &AppHandle,
    child: &mut Child,
    port: u16,
    name: &str,
    health_path: &str,
) -> Result<(), String> {
    for _ in 0..60 {
        if is_cancelled(app) {
            terminate(child);
            return Err("运行时任务已取消".to_string());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("{name} 服务启动失败，退出状态: {status}"));
        }
        if health_check(port, health_path) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    terminate(child);
    Err(format!("{name} 服务健康检查超时"))
}

fn service_env(
    base_generation: &Path,
    project: &ProjectEnvironment,
    service_dir: &Path,
) -> Vec<(&'static str, String)> {
    let delimiter = if cfg!(windows) { ";" } else { ":" };
    let python_bin = runtime_bin_dir(base_generation, "python");
    let node_bin = runtime_bin_dir(base_generation, "node");
    let project_python_bin = project
        .python
        .as_ref()
        .and_then(|path| path.parent())
        .map(|path| path.to_string_lossy().into_owned());
    let mut environment = vec![
        (
            "PATH",
            format!(
                "{}{}{}{}{}",
                project_python_bin.as_deref().unwrap_or(""),
                if project_python_bin.is_some() {
                    delimiter
                } else {
                    ""
                },
                node_bin.display(),
                delimiter,
                python_bin.display(),
            ),
        ),
        ("PYTHONUTF8", "1".to_string()),
        ("PYTHONNOUSERSITE", "1".to_string()),
        ("PYTHONPATH", service_dir.to_string_lossy().into_owned()),
        (
            "NODE_PATH",
            project
                .node_modules
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
        ("HOME", project.root.to_string_lossy().into_owned()),
        (
            "RUNTIME_SERVICE_ROOT",
            service_dir.to_string_lossy().into_owned(),
        ),
    ];
    if let Some(ffmpeg) = project.ffmpeg.as_ref() {
        environment.push(("FFMPEG_PATH", ffmpeg.to_string_lossy().into_owned()));
    }
    environment
}

fn open_service_log(runtime: &RuntimePaths, name: &str) -> Result<(File, File), String> {
    let path = runtime.logs.join(format!("{name}-service.log"));
    const MAX_LOG_BYTES: u64 = 10 * 1024 * 1024;
    if fs::metadata(&path)
        .map(|metadata| metadata.len() >= MAX_LOG_BYTES)
        .unwrap_or(false)
    {
        let backup = runtime.logs.join(format!("{name}-service.log.1"));
        let _ = fs::remove_file(&backup);
        fs::rename(&path, &backup).map_err(|error| format!("无法轮转 {name} 服务日志: {error}"))?;
    }
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("无法打开 {name} 服务日志: {error}"))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("无法复制 {name} 服务日志句柄: {error}"))?;
    Ok((stdout, stderr))
}

#[allow(clippy::too_many_arguments)]
fn start_service_with_health(
    app: &AppHandle,
    runtime: &RuntimePaths,
    base_generation: &Path,
    project: &ProjectEnvironment,
    name: &str,
    display_name: &str,
    binary: &Path,
    script: &Path,
    health_path: &str,
    launch_args: &[String],
) -> Result<RunningService, String> {
    let max_attempts = 3_u32;
    let mut last_error = format!("{display_name} 服务启动失败");
    for attempt in 1..=max_attempts {
        check_cancelled(app)?;
        let port = reserve_port()?;
        let mut child = spawn_service(
            runtime,
            base_generation,
            project,
            name,
            binary,
            script,
            port,
            launch_args,
        )?;
        match wait_for_health(app, &mut child, port, display_name, health_path) {
            Ok(()) => {
                return Ok(RunningService {
                    name: name.to_string(),
                    port,
                    health_path: health_path.to_string(),
                    child,
                });
            }
            Err(error) => {
                last_error = error;
                if is_cancelled(app) {
                    return Err("运行时任务已取消".to_string());
                }
                if attempt < max_attempts {
                    emit_detailed_with_metadata(
                        app,
                        "services",
                        "retrying",
                        &format!(
                            "{display_name} 服务启动瞬态失败，正在更换端口重试 ({}/{})",
                            attempt, max_attempts
                        ),
                        96,
                        Some(format!("127.0.0.1:{port}")),
                        None,
                        None,
                        None,
                        attempt,
                        Some("service_start_retry".to_string()),
                    );
                    thread::sleep(Duration::from_millis(200 * u64::from(attempt)));
                }
            }
        }
    }
    Err(last_error)
}

#[allow(clippy::too_many_arguments)]
fn spawn_service(
    runtime: &RuntimePaths,
    base_generation: &Path,
    project: &ProjectEnvironment,
    name: &str,
    binary: &Path,
    script: &Path,
    port: u16,
    launch_args: &[String],
) -> Result<Child, String> {
    let (stdout, stderr) = open_service_log(runtime, name)?;
    let service_dir = script
        .parent()
        .ok_or_else(|| format!("{name} 服务目录无效"))?;
    let mut command = Command::new(binary);
    command
        .env_clear()
        .current_dir(service_dir)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    for argument in launch_args {
        let value = match argument.as_str() {
            "{entrypoint}" => script.to_string_lossy().into_owned(),
            "{port}" => port.to_string(),
            value => value.to_string(),
        };
        command.arg(value);
    }
    for (key, value) in service_env(base_generation, project, service_dir) {
        command.env(key, value);
    }
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }
    prepare_command(&mut command);
    command
        .spawn()
        .map_err(|error| format!("无法启动 {name} 服务: {error}"))
}

pub(crate) fn stop_children(children: &mut Vec<RunningService>) {
    for service in children.iter_mut() {
        terminate(&mut service.child);
    }
    children.clear();
}

pub(crate) fn start_services(
    app: &AppHandle,
    state: &ServiceState,
) -> Result<Vec<ServiceSnapshot>, String> {
    check_cancelled(app)?;
    let runtime = paths(app)?;
    let snapshot = inspect(app)?;
    if snapshot.status != "ready" {
        return Err(format!("运行时尚未就绪: {}", snapshot.message));
    }
    let generation = snapshot
        .active_generation
        .as_deref()
        .and_then(|value| safe_generation_path(&runtime, value))
        .ok_or_else(|| "运行时 active generation 无效".to_string())?;
    let (_, node) = binary_paths(&runtime, snapshot.active_generation.as_deref());
    let node = node.ok_or_else(|| "私有 Node.js 解释器不存在".to_string())?;
    emit(app, "verify", "running", "正在验证运行时并启动示例服务", 92);
    let projects = PROJECT_MODULES
        .iter()
        .map(|module| project_environment(&runtime, module.project_id))
        .collect::<Result<Vec<_>, _>>()?;

    let mut children = state
        .children
        .lock()
        .map_err(|_| "服务状态锁已损坏".to_string())?;
    if children.is_empty() {
        terminate_orphaned_processes(&runtime.root);
    }
    let already_running = children.iter_mut().all(|service| {
        service
            .child
            .try_wait()
            .map(|status| status.is_none())
            .unwrap_or(false)
    });
    if !children.is_empty() && already_running {
        if children
            .iter()
            .all(|service| health_check(service.port, &service.health_path))
        {
            let output = children
                .iter()
                .map(|service| {
                    format!(
                        "{}: 127.0.0.1:{}{} OK",
                        service.name, service.port, service.health_path
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            emit_detailed_with_metadata(
                app,
                "services",
                "ready",
                "Python 和 Node.js 示例服务已运行，健康检查通过",
                100,
                Some(output),
                None,
                None,
                None,
                0,
                None,
            );
            return Ok(children
                .iter()
                .map(|service| ServiceSnapshot {
                    name: service.name.clone(),
                    running: true,
                    port: service.port,
                })
                .collect());
        }
        emit(
            app,
            "services",
            "running",
            "已有示例服务健康检查未通过，正在重新启动",
            96,
        );
    }
    stop_children(&mut children);

    for project in projects {
        let (binary, missing_binary) = if project.service == "python" {
            (
                project.python.clone(),
                format!("{} 项目 Python venv 解释器不存在", project.display_name),
            )
        } else {
            (
                Some(node.clone()),
                format!("{} 项目 Node.js 解释器不存在", project.display_name),
            )
        };
        let binary = binary.ok_or(missing_binary)?;
        let script = project.generation.join(&project.entrypoint);
        if !script.is_file() {
            stop_children(&mut children);
            return Err(format!("{} 服务入口不存在", project.display_name));
        }
        match start_service_with_health(
            app,
            &runtime,
            &generation,
            &project,
            &project.project_id,
            &project.display_name,
            &binary,
            &script,
            &project.health_path,
            &project.launch_args,
        ) {
            Ok(service) => children.push(service),
            Err(error) => {
                stop_children(&mut children);
                return Err(error);
            }
        }
    }

    let output = children
        .iter()
        .map(|service| {
            format!(
                "{}: 127.0.0.1:{}{} OK",
                service.name, service.port, service.health_path
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    emit_detailed_with_metadata(
        app,
        "services",
        "ready",
        "Python 和 Node.js 示例服务已启动，健康检查通过",
        100,
        Some(output),
        None,
        None,
        None,
        0,
        None,
    );
    Ok(children
        .iter()
        .map(|service| ServiceSnapshot {
            name: service.name.clone(),
            running: true,
            port: service.port,
        })
        .collect())
}

pub(crate) fn service_status(state: &ServiceState) -> Result<Vec<ServiceSnapshot>, String> {
    let mut children = state
        .children
        .lock()
        .map_err(|_| "服务状态锁已损坏".to_string())?;
    children
        .iter_mut()
        .map(|service| {
            let running = service
                .child
                .try_wait()
                .map(|status| status.is_none())
                .unwrap_or(false);
            Ok(ServiceSnapshot {
                name: service.name.clone(),
                running,
                port: service.port,
            })
        })
        .collect()
}
