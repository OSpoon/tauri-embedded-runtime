use flate2::read::GzDecoder;
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use tar::Archive;
use zip::ZipArchive;

use super::types::{ArchiveKind, Artifact};

pub(crate) fn safe_archive_path(base: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut output = base.to_path_buf();
    for component in relative.components() {
        match component {
            Component::Normal(value) => output.push(value),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("压缩包包含越界路径: {}", relative.display()));
            }
        }
    }
    if !output.starts_with(base) {
        return Err(format!("压缩包路径超出安装目录: {}", relative.display()));
    }
    Ok(output)
}

pub(crate) fn ensure_no_symlink_ancestors(base: &Path, target: &Path) -> Result<(), String> {
    let relative = target
        .strip_prefix(base)
        .map_err(|_| format!("解压目标不在安装目录内: {}", target.display()))?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(value) = component else {
            continue;
        };
        current.push(value);
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "压缩包路径经过符号链接，已拒绝: {}",
                    current.display()
                ));
            }
        }
    }
    Ok(())
}

fn safe_relative_link(base: &Path, parent: &Path, target: &Path) -> Result<(), String> {
    let mut output = base.to_path_buf();
    for component in parent.components().chain(target.components()) {
        match component {
            Component::Normal(value) => output.push(value),
            Component::CurDir => {}
            Component::ParentDir => {
                if output == base {
                    return Err(format!("压缩包链接越界: {}", target.display()));
                }
                output.pop();
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("压缩包链接包含绝对路径: {}", target.display()));
            }
        }
    }
    if !output.starts_with(base) {
        return Err(format!("压缩包链接超出安装目录: {}", target.display()));
    }
    Ok(())
}

fn extract_tar_gz(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|error| format!("无法打开 tar.gz: {error}"))?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    for entry in archive
        .entries()
        .map_err(|error| format!("无法读取 tar.gz 目录: {error}"))?
    {
        let mut entry = entry.map_err(|error| format!("无法读取 tar.gz 文件: {error}"))?;
        let relative = entry
            .path()
            .map_err(|error| format!("无法读取压缩包路径: {error}"))?
            .into_owned();
        let target = safe_archive_path(destination, &relative)?;
        ensure_no_symlink_ancestors(destination, &target)?;
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            fs::create_dir_all(&target).map_err(|error| format!("无法创建目录: {error}"))?;
        } else if entry_type.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| format!("无法创建目录: {error}"))?;
            }
            entry
                .unpack(&target)
                .map_err(|error| format!("无法解压文件 {}: {error}", relative.display()))?;
        } else if entry_type.is_symlink() {
            #[cfg(unix)]
            {
                let link = entry
                    .link_name()
                    .map_err(|error| format!("无法读取压缩包链接: {error}"))?
                    .ok_or_else(|| "压缩包链接缺少目标".to_string())?;
                let parent = relative.parent().unwrap_or_else(|| Path::new(""));
                safe_relative_link(destination, parent, &link)?;
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| format!("无法创建链接目录: {error}"))?;
                }
                std::os::unix::fs::symlink(&link, &target)
                    .map_err(|error| format!("无法创建压缩包链接: {error}"))?;
            }
            #[cfg(not(unix))]
            {
                return Err("Windows 不支持当前压缩包中的符号链接".to_string());
            }
        } else if entry_type.is_hard_link() {
            let link = entry
                .link_name()
                .map_err(|error| format!("无法读取压缩包硬链接: {error}"))?
                .ok_or_else(|| "压缩包硬链接缺少目标".to_string())?;
            let source = safe_archive_path(destination, &link)?;
            ensure_no_symlink_ancestors(destination, &source)?;
            if !source.is_file() {
                return Err(format!("压缩包硬链接源文件不存在: {}", link.display()));
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("无法创建硬链接目录: {error}"))?;
            }
            fs::hard_link(&source, &target)
                .map_err(|error| format!("无法创建压缩包硬链接: {error}"))?;
        } else {
            return Err(format!(
                "压缩包包含不支持的文件类型: {}",
                relative.display()
            ));
        }
    }
    Ok(())
}

fn extract_zip(archive_path: &Path, destination: &Path) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|error| format!("无法打开 zip: {error}"))?;
    let mut archive = ZipArchive::new(file).map_err(|error| format!("无法读取 zip: {error}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("无法读取 zip 文件: {error}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| format!("zip 包含越界路径: {}", entry.name()))?;
        let target = safe_archive_path(destination, &relative)?;
        ensure_no_symlink_ancestors(destination, &target)?;
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|error| format!("无法创建目录: {error}"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("无法创建目录: {error}"))?;
        }
        let mut output =
            File::create(&target).map_err(|error| format!("无法写入 zip 文件: {error}"))?;
        std::io::copy(&mut entry, &mut output)
            .map_err(|error| format!("无法解压文件 {}: {error}", relative.display()))?;
    }
    Ok(())
}

pub(crate) fn extract(
    artifact: &Artifact,
    archive_path: &Path,
    destination: &Path,
) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| format!("无法创建解压目录: {error}"))?;
    match artifact.archive {
        ArchiveKind::TarGz => extract_tar_gz(archive_path, destination),
        ArchiveKind::Zip => extract_zip(archive_path, destination),
    }
}
