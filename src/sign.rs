//! Defines utilities for comparing the state of directories in terms
//! of the files contained within.

use anyhow::Result;
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
        for entry in dir.read_dir()? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb)?;
            } else {
                cb(path.to_path_buf())?;
            }
        }
    }
    Ok(())
}

/// Produce a hash for the given path.
fn hash_file(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha1::new();
    copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(Base64::encode_string(&hash))
}

/// Produces a snapshot of the given folder.
pub fn compute_signatures(path: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let mut signatures = BTreeMap::new();
    visit_dirs(path, &mut |filepath| {
        let hash = hash_file(&filepath)?;
        signatures.insert(filepath, hash);
        Ok(())
    })?;
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
            let hash = hash_file(&filepath)?;
            if *signature != hash {
                differences.push(filepath);
            }
        } else {
            differences.push(filepath);
        }
        Ok(())
    })?;
    Ok(differences)
}
