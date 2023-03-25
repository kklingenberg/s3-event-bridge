//! Defines utilities for comparing the state of directories in terms
//! of the files contained within.

use anyhow::{Context, Result};
use base64ct::{Base64, Encoding};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::copy;
use std::path::{Path, PathBuf};

/// Visit files within `dir`. Source:
/// https://doc.rust-lang.org/stable/std/fs/fn.read_dir.html
fn visit_dirs<F>(dir: &Path, cb: &mut F) -> Result<()>
where
    F: FnMut(PathBuf) -> Result<()>,
{
    if dir.is_dir() {
        for entry in dir
            .read_dir()
            .with_context(|| format!("Failed to read directory {:?}", dir))?
        {
            let entry = entry.context("Failed to read directory entry")?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb)
                    .with_context(|| format!("Failed to visit directory {:?}", &path))?;
            } else {
                cb(path.to_path_buf())
                    .with_context(|| format!("Failed to visit file {:?}", &path))?;
            }
        }
    }
    Ok(())
}

/// Produce a hash for the given path.
fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("Failed to open file {:?}", path))?;
    let mut hasher = Sha1::new();
    copy(&mut file, &mut hasher)
        .with_context(|| format!("Failed to hash file contents of {:?}", path))?;
    let hash = hasher.finalize();
    Ok(Base64::encode_string(&hash))
}

/// Produces an empty snapshot, guaranteed to be different to anything
/// except another empty snapshot.
pub fn empty_signatures() -> Result<BTreeMap<PathBuf, String>> {
    Ok(BTreeMap::new())
}

/// Produces a snapshot of the given folder.
pub fn compute_signatures(path: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let mut signatures = BTreeMap::new();
    visit_dirs(path, &mut |filepath| {
        let hash = hash_file(&filepath)
            .with_context(|| format!("Failed to compute signature for file {:?}", &filepath))?;
        signatures.insert(filepath, hash);
        Ok(())
    })
    .with_context(|| format!("Failed to compute signature for directory {:?}", path))?;
    Ok(signatures)
}

/// Produces a list of paths with differences with respect to the
/// given signatures snapshot.
pub fn find_signature_differences(
    path: &Path,
    snapshot: &BTreeMap<PathBuf, String>,
) -> Result<Vec<PathBuf>> {
    let mut differences = Vec::new();
    visit_dirs(path, &mut |filepath| {
        let presigned = snapshot.get(&filepath);
        if let Some(signature) = presigned {
            let hash = hash_file(&filepath)
                .with_context(|| format!("Failed to compute signature for file {:?}", &filepath))?;
            if *signature != hash {
                differences.push(filepath);
            }
        } else {
            differences.push(filepath);
        }
        Ok(())
    })
    .with_context(|| {
        format!(
            "Failed to compute signature differences for directory {:?}",
            path
        )
    })?;
    Ok(differences)
}
