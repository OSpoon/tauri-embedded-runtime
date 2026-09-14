use super::archive::{ensure_no_symlink_ancestors, safe_archive_path};
use super::artifacts::{artifact_for, artifact_manifest, verify_artifact_signature};
use super::cleanup::prune_generations;
use super::operation::begin_operation;
use super::plan::{bootstrap_plan, BootstrapStageKind};
use super::probe::{backfill_component_generations, probe_version, version_matches};
use super::storage::{
    ensure_layout, recover_interrupted_layout, safe_generation_path, write_transaction,
};
use super::types::{
    ArchiveKind, Artifact, RuntimeManifest, RuntimePaths, RuntimeRequirements, RuntimeTransaction,
    ServiceState, MANIFEST_SCHEMA_VERSION,
};
use super::util::unique_token;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn missing_binary_fails_probe() {
    assert!(probe_version(Path::new("/definitely/missing/runtime-binary")).is_none());
}

#[test]
fn runtime_version_probe_requires_an_exact_version_token() {
    assert!(version_matches(Some("Python 3.12.14"), "3.12.14"));
    assert!(version_matches(Some("v24.21.0"), "v24.21.0"));
    assert!(!version_matches(Some("v24.21.01"), "v24.21.0"));
    assert!(!version_matches(Some("Python 3.12.13"), "3.12.14"));
}

#[test]
fn legacy_manifest_generations_are_backfilled_once() {
    let mut manifest = RuntimeManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        app_version: "0.1.0".to_string(),
        platform: "macos".to_string(),
        arch: "aarch64".to_string(),
        runtime_revision: "runtime-policy-1".to_string(),
        python_version: "3.12.14".to_string(),
        node_version: "v24.21.0".to_string(),
        state: "ready".to_string(),
        active_generation: Some("generation-legacy".to_string()),
        python_generation: None,
        node_generation: None,
        requirements: RuntimeRequirements {
            python: true,
            node: true,
        },
    };
    let requirements = RuntimeRequirements {
        python: true,
        node: true,
    };
    assert!(backfill_component_generations(&mut manifest, &requirements));
    assert_eq!(
        manifest.python_generation.as_deref(),
        Some("generation-legacy")
    );
    assert_eq!(
        manifest.node_generation.as_deref(),
        Some("generation-legacy")
    );
    assert!(!backfill_component_generations(
        &mut manifest,
        &requirements
    ));
}

#[test]
fn archive_paths_cannot_escape_destination() {
    let base = Path::new("/tmp/runtime");
    assert!(safe_archive_path(base, Path::new("../escape")).is_err());
    assert!(safe_archive_path(base, Path::new("python/bin/python")).is_ok());
}

#[test]
fn generation_names_are_single_path_components() {
    let runtime = RuntimePaths {
        root: PathBuf::from("/tmp/runtime"),
        manifest: PathBuf::new(),
        transaction: PathBuf::new(),
        lock: PathBuf::new(),
        downloads: PathBuf::new(),
        generations: PathBuf::from("/tmp/runtime/generations"),
        logs: PathBuf::new(),
    };
    assert!(safe_generation_path(&runtime, "generation-1").is_some());
    assert!(safe_generation_path(&runtime, "../outside").is_none());
    assert!(safe_generation_path(&runtime, "nested/generation").is_none());
}

#[test]
fn runtime_operations_are_mutually_exclusive() {
    let state = ServiceState::default();
    let operation = begin_operation(&state).expect("first operation should start");
    assert!(begin_operation(&state).is_err());
    drop(operation);
    assert!(begin_operation(&state).is_ok());
}

#[test]
fn artifact_manifest_is_pinned_and_well_formed() {
    let manifest = artifact_manifest().expect("artifact manifest should parse");
    assert_eq!(manifest.schema_version, 1);
    assert_eq!(manifest.runtime_revision, super::types::RUNTIME_REVISION);
    assert_eq!(manifest.artifacts.len(), 10);
    for artifact in &manifest.artifacts {
        assert!(artifact.url.starts_with("https://"));
        assert_eq!(artifact.sha256.len(), 64);
        assert!(artifact
            .sha256
            .chars()
            .all(|value| value.is_ascii_hexdigit()));
        assert_eq!(artifact.signature.is_some(), artifact.signing_key.is_some());
    }

    let current_python = artifact_for("python").expect("current Python artifact should exist");
    assert!(verify_artifact_signature(&current_python, Path::new("python.tar.gz")).is_ok());
}

#[test]
fn artifact_signature_verification_accepts_only_original_payload() {
    let root = std::env::temp_dir().join(format!("runtime-signature-test-{}", unique_token()));
    let archive = root.join("artifact.tar.gz");
    let payload = b"runtime artifact payload";
    fs::create_dir_all(&root).expect("create signature test directory");
    fs::write(&archive, payload).expect("write signature test payload");

    let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
    let signature = signing_key.sign(payload);
    let artifact = Artifact {
        id: "test-artifact".to_string(),
        file_name: "artifact.tar.gz".to_string(),
        url: "https://example.com/artifact.tar.gz".to_string(),
        sha256: String::new(),
        archive: ArchiveKind::TarGz,
        signature: Some(STANDARD.encode(signature.to_bytes())),
        signing_key: Some(STANDARD.encode(signing_key.verifying_key().to_bytes())),
    };

    assert!(verify_artifact_signature(&artifact, &archive).is_ok());
    fs::write(&archive, b"tampered runtime artifact").expect("tamper signature test payload");
    assert!(verify_artifact_signature(&artifact, &archive).is_err());
    fs::remove_dir_all(root).expect("remove signature test directory");
}

#[test]
fn project_service_modules_are_kept_independent() {
    let python: serde_json::Value = serde_json::from_str(include_str!(
        "../../resources/projects/python-fastapi/project.json"
    ))
    .expect("Python project module manifest should parse");
    let node: serde_json::Value = serde_json::from_str(include_str!(
        "../../resources/projects/node-express/project.json"
    ))
    .expect("Node project module manifest should parse");

    assert_eq!(python["project_id"], "python-fastapi");
    assert_eq!(python["service"], "python");
    assert_eq!(python["framework"], "fastapi");
    assert!(python["requirements"].as_array().is_some());
    assert!(python.get("dependencies").is_none());

    assert_eq!(node["project_id"], "node-express");
    assert_eq!(node["service"], "node");
    assert_eq!(node["framework"], "express");
    assert!(node["dependencies"].as_object().is_some());
    assert!(node.get("requirements").is_none());
}

#[test]
fn bootstrap_plan_follows_registered_service_order() {
    let stages = bootstrap_plan();
    let ids = stages.iter().map(|stage| stage.id).collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            "check",
            "python",
            "project-python",
            "node",
            "project-node",
            "project-tools",
            "verify",
        ]
    );
    assert!(matches!(
        stages[1].kind,
        BootstrapStageKind::Runtime("python")
    ));
    assert!(matches!(
        stages[2].kind,
        BootstrapStageKind::Projects("python")
    ));
    assert!(stages
        .windows(2)
        .all(|pair| pair[0].progress.end <= pair[1].progress.start));
    assert_eq!(stages.first().map(|stage| stage.progress.start), Some(0));
    assert_eq!(stages.last().map(|stage| stage.progress.end), Some(100));
}

#[test]
fn interrupted_staging_is_recovered_without_touching_download_parts() {
    let root = std::env::temp_dir().join(format!("runtime-recovery-test-{}", unique_token()));
    let runtime = RuntimePaths {
        manifest: root.join("manifest.json"),
        transaction: root.join("transaction.json"),
        lock: root.join("install.lock"),
        downloads: root.join("downloads"),
        generations: root.join("generations"),
        logs: root.join("logs"),
        root: root.clone(),
    };
    ensure_layout(&runtime).expect("create runtime layout");
    let staging = runtime.generations.join(".generation-crashed.staging");
    fs::create_dir_all(&staging).expect("create staging directory");
    let partial = runtime.downloads.join("artifact.part");
    fs::write(&partial, b"partial").expect("create partial download");
    write_transaction(
        &runtime,
        &RuntimeTransaction {
            schema_version: MANIFEST_SCHEMA_VERSION,
            operation_id: None,
            state: "running".to_string(),
            generation: "generation-crashed".to_string(),
            previous_generation: None,
            started_at: 1,
            updated_at: 1,
            error: None,
        },
    )
    .expect("write interrupted transaction");

    let recovered = recover_interrupted_layout(&runtime)
        .expect("recover runtime layout")
        .expect("return interrupted transaction");
    assert!(!staging.exists());
    assert!(partial.exists());
    assert_eq!(recovered.state, "interrupted");
    assert!(runtime.transaction.exists());
    fs::remove_dir_all(root).expect("remove recovery test directory");
}

#[test]
fn generation_cleanup_preserves_active_previous_and_recent_generations() {
    let root = std::env::temp_dir().join(format!("runtime-cleanup-test-{}", unique_token()));
    let runtime = RuntimePaths {
        manifest: root.join("manifest.json"),
        transaction: root.join("transaction.json"),
        lock: root.join("install.lock"),
        downloads: root.join("downloads"),
        generations: root.join("generations"),
        logs: root.join("logs"),
        root: root.clone(),
    };
    ensure_layout(&runtime).expect("create runtime layout");
    for index in 1..=5 {
        let generation = runtime.generations.join(format!("generation-{index}"));
        fs::create_dir_all(&generation).expect("create generation");
        fs::write(generation.join("generation.complete"), b"ok\n")
            .expect("mark generation complete");
    }

    prune_generations(&runtime, "generation-5", Some("generation-1"))
        .expect("prune old generations");
    assert!(runtime.generations.join("generation-5").exists());
    assert!(runtime.generations.join("generation-4").exists());
    assert!(runtime.generations.join("generation-1").exists());
    assert!(!runtime.generations.join("generation-2").exists());
    assert!(!runtime.generations.join("generation-3").exists());
    fs::remove_dir_all(root).expect("remove cleanup test directory");
}

#[cfg(unix)]
#[test]
fn archive_targets_cannot_follow_existing_symlinks() {
    let root = std::env::temp_dir().join(format!("runtime-archive-test-{}", unique_token()));
    let outside = root.join("outside");
    let destination = root.join("destination");
    fs::create_dir_all(&outside).expect("create outside directory");
    fs::create_dir_all(&destination).expect("create destination directory");
    std::os::unix::fs::symlink(&outside, destination.join("link")).expect("create test symlink");

    assert!(ensure_no_symlink_ancestors(&destination, &destination.join("link/file")).is_err());
    fs::remove_dir_all(root).expect("remove archive test directory");
}
