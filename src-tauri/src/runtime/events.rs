use tauri::{AppHandle, Emitter};

use super::operation::active_operation;
use super::types::RuntimeEvent;
use super::util::now;

pub(crate) fn emit(app: &AppHandle, phase: &str, status: &str, message: &str, progress: u8) {
    emit_detailed(app, phase, status, message, progress, None, None, None);
}

// This helper mirrors the event payload so call sites can keep progress output
// close to the operation that produces it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_detailed(
    app: &AppHandle,
    phase: &str,
    status: &str,
    message: &str,
    progress: u8,
    output: Option<String>,
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
) {
    emit_detailed_with_metadata(
        app,
        phase,
        status,
        message,
        progress,
        output,
        downloaded_bytes,
        total_bytes,
        None,
        0,
        None,
    );
}

// Metadata is intentionally explicit because it is part of the public event
// contract consumed by the reusable setup module.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_detailed_with_metadata(
    app: &AppHandle,
    phase: &str,
    status: &str,
    message: &str,
    progress: u8,
    output: Option<String>,
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
    artifact: Option<String>,
    retry_count: u32,
    error_code: Option<String>,
) {
    let operation = active_operation(app);
    let progress = operation
        .as_ref()
        .map(|operation| {
            let requested = u64::from(progress.min(100));
            let current = operation
                .progress
                .load(std::sync::atomic::Ordering::Relaxed)
                .min(100);
            let progress = requested.max(current);
            operation
                .progress
                .store(progress, std::sync::atomic::Ordering::Relaxed);
            progress as u8
        })
        .unwrap_or_else(|| progress.min(100));
    let event = RuntimeEvent {
        phase: phase.to_string(),
        status: status.to_string(),
        message: message.to_string(),
        progress,
        timestamp: now(),
        output,
        downloaded_bytes,
        total_bytes,
        operation_id: operation.as_ref().map(|value| value.id.clone()),
        sequence: operation
            .as_ref()
            .map(|value| {
                value
                    .sequence
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    + 1
            })
            .unwrap_or_default(),
        artifact,
        retry_count,
        error_code,
        started_at: operation.as_ref().map(|value| value.started_at),
        updated_at: now(),
    };
    let _ = app.emit("runtime://event", event);
}
