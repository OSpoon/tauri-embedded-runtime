use std::collections::HashSet;
use std::fs;

use super::types::RuntimePaths;

const RETAINED_GENERATIONS: usize = 2;

/// Retain the active generation, the previous generation, and one additional
/// recent complete generation for rollback/debugging. Only application-owned,
/// complete directories are eligible for removal.
pub(crate) fn prune_generations(
    runtime: &RuntimePaths,
    active_generation: &str,
    previous_generation: Option<&str>,
) -> Result<(), String> {
    let mut protected = HashSet::from([active_generation.to_string()]);
    if let Some(previous_generation) = previous_generation {
        protected.insert(previous_generation.to_string());
    }

    let mut generations = fs::read_dir(&runtime.generations)
        .map_err(|error| format!("无法扫描待清理的运行时 generation: {error}"))?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_complete_directory = fs::symlink_metadata(&path)
                .map(|metadata| metadata.file_type().is_dir())
                .unwrap_or(false)
                && fs::symlink_metadata(path.join("generation.complete"))
                    .map(|metadata| metadata.file_type().is_file())
                    .unwrap_or(false);
            (name.starts_with("generation-") && is_complete_directory).then_some((name, path))
        })
        .collect::<Vec<_>>();
    generations.sort_by(|left, right| left.0.cmp(&right.0));

    let recent = generations
        .iter()
        .rev()
        .take(RETAINED_GENERATIONS)
        .map(|(name, _)| name.clone())
        .collect::<HashSet<_>>();
    protected.extend(recent);

    for (name, path) in generations {
        if protected.contains(&name) {
            continue;
        }
        fs::remove_dir_all(&path)
            .map_err(|error| format!("无法清理旧运行时 generation {}: {error}", path.display()))?;
    }
    Ok(())
}
