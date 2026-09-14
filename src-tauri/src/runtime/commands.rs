use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State};

use super::events::{emit, emit_detailed_with_metadata};
use super::install::{install, install_component, install_runtime_stage};
use super::operation::begin_operation;
use super::plan::{bootstrap_plan, BootstrapStageKind};
use super::probe::inspect;
use super::process::terminate_orphaned_processes;
use super::project::{
    emit_project_tools_event, ensure_project_environments, ensure_project_environments_for_service,
    ensure_project_environments_for_service_ordered, ensure_single_project_environment,
    project_snapshots,
};
use super::projects::service_for_id;
use super::services::{service_status, start_services, stop_children};
use super::storage::{paths, read_manifest, safe_generation_path, write_manifest};
use super::types::{
    ProjectSnapshot, RuntimeProjectInfo, RuntimeSetupStep, RuntimeSnapshot, ServiceSnapshot,
    ServiceState,
};

#[tauri::command]
pub fn runtime_status(app: AppHandle) -> Result<RuntimeSnapshot, String> {
    inspect(&app)
}

#[tauri::command]
pub fn runtime_setup_plan() -> Vec<RuntimeSetupStep> {
    super::plan::setup_steps()
}

#[tauri::command]
pub fn runtime_projects_catalog() -> Result<Vec<RuntimeProjectInfo>, String> {
    super::project::project_catalog()
}

#[tauri::command]
pub async fn runtime_bootstrap(
    app: AppHandle,
    state: State<'_, ServiceState>,
) -> Result<RuntimeSnapshot, String> {
    let service_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = begin_operation(&service_state)?;
        bootstrap_blocking(app, service_state)
    })
    .await
    .map_err(|error| format!("运行时后台任务异常: {error}"))?
}

fn can_reuse_runtime_component(
    app: &AppHandle,
    snapshot: &RuntimeSnapshot,
    component: &str,
) -> bool {
    let ready = match component {
        "python" => snapshot.python.status == "ready" && snapshot.python.path.is_some(),
        "node" => snapshot.node.status == "ready" && snapshot.node.path.is_some(),
        _ => false,
    };
    if !ready {
        return false;
    }
    let Ok(runtime) = paths(app) else {
        return false;
    };
    snapshot
        .active_generation
        .as_deref()
        .and_then(|generation| safe_generation_path(&runtime, generation))
        .map(|generation| generation.join("generation.complete").is_file())
        .unwrap_or(false)
}

pub(crate) fn bootstrap_blocking(
    app: AppHandle,
    state: ServiceState,
) -> Result<RuntimeSnapshot, String> {
    let plan = bootstrap_plan();
    let check_stage = plan
        .iter()
        .find(|stage| matches!(stage.kind, BootstrapStageKind::Check))
        .ok_or_else(|| "运行时执行计划缺少环境检查阶段".to_string())?;
    emit(
        &app,
        check_stage.phase,
        "running",
        "正在检查应用运行时",
        check_stage.progress.at(20),
    );
    let current = inspect(&app)?;
    emit_detailed_with_metadata(
        &app,
        check_stage.phase,
        "completed",
        "安装环境检查完成",
        check_stage.progress.at(70),
        Some(format!(
            "platform={} arch={} runtime_status={}",
            current.platform, current.arch, current.status
        )),
        None,
        None,
        None,
        0,
        None,
    );
    let previous_manifest = if current.status == "ready" {
        paths(&app).ok().and_then(|runtime| read_manifest(&runtime))
    } else {
        None
    };
    let mut base_snapshot = current.clone();
    let mut project_snapshots = Vec::new();
    for stage in plan.iter().filter(|stage| {
        !matches!(
            stage.kind,
            BootstrapStageKind::Check | BootstrapStageKind::Verify
        )
    }) {
        match stage.kind {
            BootstrapStageKind::Runtime(service) => {
                let spec = service_for_id(service)
                    .ok_or_else(|| format!("运行时执行计划引用了未注册服务: {service}"))?;
                if can_reuse_runtime_component(&app, &base_snapshot, service) {
                    let path = if service == "python" {
                        base_snapshot.python.path.clone()
                    } else {
                        base_snapshot.node.path.clone()
                    };
                    emit_detailed_with_metadata(
                        &app,
                        stage.phase,
                        "cached",
                        &format!("{} 已就绪，跳过下载与解压", spec.runtime_title),
                        stage.progress.end,
                        path,
                        None,
                        None,
                        Some(service.to_string()),
                        0,
                        None,
                    );
                } else {
                    base_snapshot = install_runtime_stage(
                        &app,
                        base_snapshot.status != "ready",
                        service,
                        stage.progress,
                    )?;
                }
            }
            BootstrapStageKind::Projects(service) => {
                project_snapshots.extend(ensure_project_environments_for_service_ordered(
                    &app,
                    &base_snapshot,
                    service,
                    false,
                    stage.progress,
                )?);
            }
            BootstrapStageKind::Tools => {
                emit_project_tools_event(&app, &project_snapshots, "completed", stage.progress.end);
            }
            BootstrapStageKind::Check | BootstrapStageKind::Verify => {}
        }
    }
    if let Err(error) = start_services(&app, &state) {
        if let Some(previous_manifest) = previous_manifest {
            if let Ok(runtime) = paths(&app) {
                let _ = write_manifest(&runtime, &previous_manifest);
                emit(
                    &app,
                    "rollback",
                    "completed",
                    "服务启动失败，已恢复上一份可用运行时",
                    100,
                );
            }
        } else if error == "运行时任务已取消" {
            emit_detailed_with_metadata(
                &app,
                "cancelled",
                "cancelled",
                &error,
                100,
                None,
                None,
                None,
                None,
                0,
                Some("cancelled".to_string()),
            );
        } else {
            emit_detailed_with_metadata(
                &app,
                "services",
                "failed",
                &error,
                100,
                None,
                None,
                None,
                None,
                0,
                Some("service_start_failed".to_string()),
            );
        }
        return Err(error);
    }
    inspect(&app)
}

#[tauri::command]
pub async fn runtime_install(
    app: AppHandle,
    state: State<'_, ServiceState>,
) -> Result<RuntimeSnapshot, String> {
    let runtime_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = begin_operation(&runtime_state)?;
        let base_snapshot = install(&app, false)?;
        ensure_project_environments(&app, &base_snapshot, false)?;
        inspect(&app)
    })
    .await
    .map_err(|error| format!("运行时安装后台任务异常: {error}"))?
}

#[tauri::command]
pub async fn runtime_repair(
    app: AppHandle,
    state: State<'_, ServiceState>,
) -> Result<RuntimeSnapshot, String> {
    let runtime_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = begin_operation(&runtime_state)?;
        let base_snapshot = install(&app, true)?;
        ensure_project_environments(&app, &base_snapshot, true)?;
        inspect(&app)
    })
    .await
    .map_err(|error| format!("运行时修复后台任务异常: {error}"))?
}

#[tauri::command]
pub async fn runtime_repair_component(
    app: AppHandle,
    state: State<'_, ServiceState>,
    component: String,
) -> Result<RuntimeSnapshot, String> {
    if !matches!(component.as_str(), "python" | "node") {
        return Err(format!("不支持修复运行时组件: {component}"));
    }
    let runtime_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = begin_operation(&runtime_state)?;
        let base_snapshot = install_component(&app, &component)?;
        if base_snapshot.status != "ready" {
            return Err(format!(
                "{component} 修复完成后检查未通过: {}",
                base_snapshot.message
            ));
        }
        ensure_project_environments_for_service(&app, &base_snapshot, &component, true)?;
        inspect(&app)
    })
    .await
    .map_err(|error| format!("运行时组件修复后台任务异常: {error}"))?
}

#[tauri::command]
pub async fn runtime_start_services(
    app: AppHandle,
    state: State<'_, ServiceState>,
) -> Result<Vec<ServiceSnapshot>, String> {
    let service_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = begin_operation(&service_state)?;
        let snapshot = inspect(&app)?;
        if snapshot.status != "ready" {
            return Err(format!("运行时尚未就绪: {}", snapshot.message));
        }
        ensure_project_environments(&app, &snapshot, false)?;
        start_services(&app, &service_state)
    })
    .await
    .map_err(|error| format!("示例服务启动后台任务异常: {error}"))?
}

#[tauri::command]
pub fn runtime_projects_status(app: AppHandle) -> Result<Vec<ProjectSnapshot>, String> {
    let runtime = super::storage::paths(&app)?;
    project_snapshots(&runtime)
}

#[tauri::command]
pub async fn runtime_projects_install(
    app: AppHandle,
    state: State<'_, ServiceState>,
) -> Result<Vec<ProjectSnapshot>, String> {
    let runtime_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = begin_operation(&runtime_state)?;
        let base_snapshot = inspect(&app)?;
        if base_snapshot.status != "ready" {
            return Err(format!("基础运行时尚未就绪: {}", base_snapshot.message));
        }
        ensure_project_environments(&app, &base_snapshot, false)
    })
    .await
    .map_err(|error| format!("项目依赖安装后台任务异常: {error}"))?
}

#[tauri::command]
pub async fn runtime_projects_repair(
    app: AppHandle,
    state: State<'_, ServiceState>,
) -> Result<Vec<ProjectSnapshot>, String> {
    let runtime_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = begin_operation(&runtime_state)?;
        let base_snapshot = inspect(&app)?;
        if base_snapshot.status != "ready" {
            return Err(format!("基础运行时尚未就绪: {}", base_snapshot.message));
        }
        ensure_project_environments(&app, &base_snapshot, true)
    })
    .await
    .map_err(|error| format!("项目依赖修复后台任务异常: {error}"))?
}

#[tauri::command]
pub async fn runtime_project_repair(
    app: AppHandle,
    state: State<'_, ServiceState>,
    project_id: String,
) -> Result<ProjectSnapshot, String> {
    let runtime_state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = begin_operation(&runtime_state)?;
        let base_snapshot = inspect(&app)?;
        if base_snapshot.status != "ready" {
            return Err(format!("基础运行时尚未就绪: {}", base_snapshot.message));
        }
        ensure_single_project_environment(&app, &base_snapshot, &project_id, true)
    })
    .await
    .map_err(|error| format!("项目环境修复后台任务异常: {error}"))?
}

#[tauri::command]
pub fn runtime_cancel(app: AppHandle, state: State<'_, ServiceState>) -> Result<(), String> {
    let operation = state
        .operation
        .lock()
        .map_err(|_| "运行时任务状态锁已损坏".to_string())?
        .clone()
        .ok_or_else(|| "当前没有正在执行的运行时任务".to_string())?;
    let progress = operation.progress.load(Ordering::Relaxed).min(100) as u8;
    operation.cancel.store(true, Ordering::Relaxed);
    emit(
        &app,
        "cancel",
        "cancelling",
        "已发送取消请求，正在等待当前 I/O 安全结束",
        progress,
    );
    Ok(())
}

#[tauri::command]
pub fn runtime_stop_services(state: State<'_, ServiceState>) -> Result<(), String> {
    let mut children = state
        .children
        .lock()
        .map_err(|_| "服务状态锁已损坏".to_string())?;
    stop_children(&mut children);
    Ok(())
}

pub(crate) fn shutdown_runtime(app: &AppHandle) {
    if let Some(state) = app.try_state::<ServiceState>() {
        if let Ok(operation) = state.operation.lock() {
            if let Some(operation) = operation.as_ref() {
                operation.cancel.store(true, Ordering::Relaxed);
            }
        }
        if let Ok(mut children) = state.children.lock() {
            stop_children(&mut children);
        }
        if let Ok(runtime) = paths(app) {
            terminate_orphaned_processes(&runtime.root);
        }
    }
}

#[tauri::command]
pub fn runtime_service_status(
    state: State<'_, ServiceState>,
) -> Result<Vec<ServiceSnapshot>, String> {
    service_status(&state)
}
