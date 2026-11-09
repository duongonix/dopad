use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context as _, anyhow};

use super::{NODE_VERSION, NodeTarget};

const MAX_EXTRACTED_BYTES: u64 = 700 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 100_000;

pub(super) fn final_runtime_path(runtimes: &Path, target: &NodeTarget) -> PathBuf {
    runtimes.join(format!("node-v{NODE_VERSION}-{}", target.id))
}

pub(super) fn extract_zip(archive_path: &Path, destination: &Path) -> anyhow::Result<()> {
    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file).context("Node.js ZIP archive is invalid")?;
    anyhow::ensure!(
        archive.len() <= MAX_ARCHIVE_ENTRIES,
        "Node.js ZIP has too many entries"
    );
    let mut extracted = 0u64;
    let mut seen = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("Node.js ZIP contains an unsafe path"))?;
        validate_relative_path(&relative)?;
        anyhow::ensure!(
            seen.insert(relative.clone()),
            "Node.js ZIP contains duplicate paths"
        );
        extracted = extracted
            .checked_add(entry.size())
            .ok_or_else(|| anyhow!("Node.js archive size overflow"))?;
        anyhow::ensure!(
            extracted <= MAX_EXTRACTED_BYTES,
            "Node.js archive expands beyond the size limit"
        );
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            continue;
        }
        let output = destination.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(output)?;
            continue;
        }
        create_parent(&output)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?;
        std::io::copy(&mut entry, &mut file)?;
    }
    Ok(())
}

pub(super) fn extract_tar_gz(archive_path: &Path, destination: &Path) -> anyhow::Result<()> {
    let file = File::open(archive_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .context("Node.js TAR archive is invalid")?;
    let mut extracted = 0u64;
    let mut count = 0usize;
    let mut seen = HashSet::new();
    for entry in entries {
        let mut entry = entry?;
        count += 1;
        anyhow::ensure!(
            count <= MAX_ARCHIVE_ENTRIES,
            "Node.js TAR has too many entries"
        );
        let relative = entry.path()?.into_owned();
        validate_relative_path(&relative)?;
        anyhow::ensure!(
            seen.insert(relative.clone()),
            "Node.js TAR contains duplicate paths"
        );
        let entry_type = entry.header().entry_type();
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            continue;
        }
        let output = destination.join(&relative);
        if entry_type.is_dir() {
            fs::create_dir_all(output)?;
            continue;
        }
        anyhow::ensure!(
            entry_type.is_file() || entry_type.is_contiguous(),
            "Node.js TAR contains an unsupported entry type"
        );
        let size = entry.header().size()?;
        extracted = extracted
            .checked_add(size)
            .ok_or_else(|| anyhow!("Node.js archive size overflow"))?;
        anyhow::ensure!(
            extracted <= MAX_EXTRACTED_BYTES,
            "Node.js archive expands beyond the size limit"
        );
        create_parent(&output)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        std::io::copy(&mut entry, &mut file)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = entry.header().mode().unwrap_or(0o644) & 0o777;
            fs::set_permissions(output, fs::Permissions::from_mode(mode))?;
        }
    }
    Ok(())
}

fn create_parent(path: &Path) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("archive entry has no parent directory"))?;
    fs::create_dir_all(parent)?;
    Ok(())
}

pub(super) fn validate_relative_path(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        path.components()
            .all(|component| matches!(component, Component::Normal(_))),
        "Node.js archive contains a path traversal entry"
    );
    Ok(())
}
