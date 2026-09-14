use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

use super::types::{RuntimeOperation, ServiceState};
use super::util::{now, unique_token};

pub(crate) fn active_operation(app: &AppHandle) -> Option<RuntimeOperation> {
    let state = app.try_state::<ServiceState>()?;
    let operation = state.operation.lock().ok()?.clone();
    operation
}

pub(crate) fn begin_operation(state: &ServiceState) -> Result<OperationGuard, String> {
    let mut active = state
        .operation
        .lock()
        .map_err(|_| "运行时任务状态锁已损坏".to_string())?;
    if active.is_some() {
        return Err("已有运行时任务正在执行，请等待当前任务完成".to_string());
    }
    let operation = RuntimeOperation {
        id: format!("runtime-{}", unique_token()),
        cancel: Arc::new(AtomicBool::new(false)),
        sequence: Arc::new(AtomicU64::new(0)),
        progress: Arc::new(AtomicU64::new(0)),
        started_at: now(),
    };
    *active = Some(operation.clone());
    Ok(OperationGuard {
        operation: state.operation.clone(),
        id: operation.id,
    })
}

pub(crate) struct OperationGuard {
    operation: Arc<Mutex<Option<RuntimeOperation>>>,
    id: String,
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.operation.lock() {
            if active
                .as_ref()
                .map(|operation| operation.id == self.id)
                .unwrap_or(false)
            {
                *active = None;
            }
        }
    }
}

pub(crate) fn check_cancelled(app: &AppHandle) -> Result<(), String> {
    if is_cancelled(app) {
        Err("运行时任务已取消".to_string())
    } else {
        Ok(())
    }
}

pub(crate) fn is_cancelled(app: &AppHandle) -> bool {
    active_operation(app)
        .map(|operation| operation.cancel.load(Ordering::Relaxed))
        .unwrap_or(false)
}
