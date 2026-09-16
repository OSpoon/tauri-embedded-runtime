use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};

pub(crate) const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub(crate) const RUNTIME_REVISION: &str = "runtime-policy-1";
pub(crate) const PYTHON_VERSION: &str = "3.12.14";
pub(crate) const NODE_VERSION: &str = "v24.21.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRequirements {
    pub python: bool,
    pub node: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeManifest {
    pub schema_version: u32,
    pub app_version: String,
    pub platform: String,
    pub arch: String,
    pub runtime_revision: String,
    pub python_version: String,
    pub node_version: String,
    pub state: String,
    pub active_generation: Option<String>,
    #[serde(default)]
    pub python_generation: Option<String>,
    #[serde(default)]
    pub node_generation: Option<String>,
    pub requirements: RuntimeRequirements,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeComponent {
    pub required: bool,
    pub present: bool,
    pub status: String,
    pub path: Option<String>,
    pub version: Option<String>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSnapshot {
    pub project_id: String,
    pub status: String,
    pub active_generation: Option<String>,
    pub dependency_revision: String,
    pub python_dependencies: Vec<String>,
    pub node_dependencies: Vec<String>,
    pub tools: Vec<String>,
    pub issues: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProjectInfo {
    pub project_id: String,
    pub service: String,
    pub service_label: String,
    pub framework: String,
    pub display_name: String,
    pub health_path: String,
    pub demo_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeSnapshot {
    pub status: String,
    pub platform: String,
    pub arch: String,
    pub runtime_root: String,
    pub active_generation: Option<String>,
    pub runtime_revision: String,
    pub requirements: RuntimeRequirements,
    pub selected_runtime: String,
    pub python: RuntimeComponent,
    pub node: RuntimeComponent,
    pub projects: Vec<ProjectSnapshot>,
    pub issues: Vec<String>,
    pub message: String,
    pub checked_at: u64,
    pub operation_id: Option<String>,
    pub operation_status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeEvent {
    pub phase: String,
    pub status: String,
    pub message: String,
    pub progress: u8,
    pub timestamp: u64,
    pub output: Option<String>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub operation_id: Option<String>,
    pub sequence: u64,
    pub artifact: Option<String>,
    pub retry_count: u32,
    pub error_code: Option<String>,
    pub started_at: Option<u64>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeSetupStep {
    pub id: String,
    pub phase: String,
    pub title: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceSnapshot {
    pub name: String,
    pub running: bool,
    pub port: u16,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ArchiveKind {
    TarGz,
    Zip,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ArtifactManifest {
    pub(crate) schema_version: u32,
    pub(crate) runtime_revision: String,
    pub(crate) artifacts: Vec<ArtifactManifestEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ArtifactManifestEntry {
    pub(crate) id: String,
    pub(crate) runtime: String,
    pub(crate) version: String,
    pub(crate) platform: String,
    pub(crate) arch: String,
    pub(crate) file_name: String,
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) archive: String,
    pub(crate) signature: Option<String>,
    pub(crate) signing_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Artifact {
    pub(crate) id: String,
    pub(crate) file_name: String,
    pub(crate) url: String,
    pub(crate) sha256: String,
    pub(crate) archive: ArchiveKind,
    pub(crate) signature: Option<String>,
    pub(crate) signing_key: Option<String>,
}

pub(crate) struct RuntimePaths {
    pub(crate) root: PathBuf,
    pub(crate) manifest: PathBuf,
    pub(crate) transaction: PathBuf,
    pub(crate) lock: PathBuf,
    pub(crate) downloads: PathBuf,
    pub(crate) generations: PathBuf,
    pub(crate) logs: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RuntimeTransaction {
    pub(crate) schema_version: u32,
    pub(crate) operation_id: Option<String>,
    pub(crate) state: String,
    pub(crate) generation: String,
    pub(crate) previous_generation: Option<String>,
    pub(crate) started_at: u64,
    pub(crate) updated_at: u64,
    pub(crate) error: Option<String>,
}

pub(crate) struct RuntimeLock(pub(crate) File);

#[derive(Clone)]
pub struct ServiceState {
    pub(crate) children: Arc<Mutex<Vec<RunningService>>>,
    pub(crate) operation: Arc<Mutex<Option<RuntimeOperation>>>,
}

#[derive(Clone)]
pub(crate) struct RuntimeOperation {
    pub(crate) id: String,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) sequence: Arc<AtomicU64>,
    pub(crate) progress: Arc<AtomicU64>,
    pub(crate) started_at: u64,
}

pub(crate) struct RunningService {
    pub(crate) name: String,
    pub(crate) port: u16,
    pub(crate) health_path: String,
    pub(crate) child: std::process::Child,
}

impl Default for ServiceState {
    fn default() -> Self {
        Self {
            children: Arc::new(Mutex::new(Vec::new())),
            operation: Arc::new(Mutex::new(None)),
        }
    }
}

impl Drop for RuntimeLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}
