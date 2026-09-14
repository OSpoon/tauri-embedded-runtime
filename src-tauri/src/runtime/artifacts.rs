use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use super::types::{
    ArchiveKind, Artifact, ArtifactManifest, NODE_VERSION, PYTHON_VERSION, RUNTIME_REVISION,
};
use super::util::{architecture_name, platform_name};

const ARTIFACT_MANIFEST_SOURCE: &str = include_str!("../../resources/runtime-artifacts.json");

pub(crate) fn artifact_manifest() -> Result<ArtifactManifest, String> {
    let manifest = serde_json::from_str::<ArtifactManifest>(ARTIFACT_MANIFEST_SOURCE)
        .map_err(|error| format!("运行时制品 manifest 无效: {error}"))?;
    if manifest.schema_version != 1 || manifest.runtime_revision != RUNTIME_REVISION {
        return Err("运行时制品 manifest 版本不匹配".to_string());
    }
    Ok(manifest)
}

pub(crate) fn artifact_for(runtime_name: &str) -> Result<Artifact, String> {
    let manifest = artifact_manifest()?;
    let platform = platform_name();
    let arch = architecture_name();
    let entry = manifest
        .artifacts
        .into_iter()
        .find(|entry| {
            entry.runtime == runtime_name && entry.platform == platform && entry.arch == arch
        })
        .ok_or_else(|| format!("暂不支持的 {runtime_name} 平台: {platform}-{arch}"))?;
    let expected_version = match runtime_name {
        "python" => PYTHON_VERSION,
        "node" => NODE_VERSION,
        _ => return Err(format!("未知运行时类型: {runtime_name}")),
    };
    if entry.version != expected_version {
        return Err(format!(
            "制品 {} 的版本 {} 与运行时策略 {} 不一致",
            entry.id, entry.version, expected_version
        ));
    }
    if entry.signature.is_some() != entry.signing_key.is_some() {
        return Err(format!("制品 {} 的签名和签名公钥必须同时提供", entry.id));
    }
    let archive = match entry.archive.as_str() {
        "tar.gz" => ArchiveKind::TarGz,
        "zip" => ArchiveKind::Zip,
        value => return Err(format!("制品 {} 使用未知压缩格式: {value}", entry.id)),
    };
    if entry.sha256.len() != 64 || !entry.sha256.chars().all(|value| value.is_ascii_hexdigit()) {
        return Err(format!("制品 {} 的 SHA-256 清单无效", entry.id));
    }
    if !entry.url.starts_with("https://") {
        return Err(format!("制品 {} 的下载地址必须使用 HTTPS", entry.id));
    }
    Ok(Artifact {
        id: entry.id,
        file_name: entry.file_name,
        url: entry.url,
        sha256: entry.sha256,
        archive,
        signature: entry.signature,
        signing_key: entry.signing_key,
    })
}

pub(crate) fn python_artifact() -> Result<Artifact, String> {
    artifact_for("python")
}

pub(crate) fn node_artifact() -> Result<Artifact, String> {
    artifact_for("node")
}

pub(crate) fn verify_artifact_signature(
    artifact: &Artifact,
    archive_path: &Path,
) -> Result<(), String> {
    match (&artifact.signature, &artifact.signing_key) {
        (None, None) => Ok(()),
        (Some(signature), Some(signing_key)) => {
            let signature_bytes = STANDARD.decode(signature).map_err(|error| {
                format!(
                    "制品 {} 的 Ed25519 签名不是合法 Base64: {error}",
                    artifact.id
                )
            })?;
            let key_bytes = STANDARD.decode(signing_key).map_err(|error| {
                format!(
                    "制品 {} 的 Ed25519 签名公钥不是合法 Base64: {error}",
                    artifact.id
                )
            })?;
            let key_bytes: [u8; 32] = key_bytes
                .try_into()
                .map_err(|_| format!("制品 {} 的 Ed25519 公钥长度必须为 32 字节", artifact.id))?;
            let verifying_key = VerifyingKey::from_bytes(&key_bytes)
                .map_err(|error| format!("制品 {} 的 Ed25519 公钥无效: {error}", artifact.id))?;
            let signature = Signature::from_slice(&signature_bytes).map_err(|error| {
                format!("制品 {} 的 Ed25519 签名长度无效: {error}", artifact.id)
            })?;
            let payload = fs::read(archive_path).map_err(|error| {
                format!(
                    "无法读取 {} 的签名校验文件: {error}",
                    archive_path.display()
                )
            })?;
            verifying_key
                .verify(&payload, &signature)
                .map_err(|_| format!("制品 {} 的 Ed25519 签名校验失败", artifact.id))
        }
        _ => Err(format!(
            "制品 {} 的签名元数据不完整，已拒绝安装",
            artifact.id
        )),
    }
}

pub(crate) fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("无法读取校验文件: {error}"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 128];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("读取下载文件失败: {error}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_digest(&hasher.finalize()))
}
