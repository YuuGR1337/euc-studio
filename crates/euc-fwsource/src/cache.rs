use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use euc_profile::Sha256;
use serde::{Deserialize, Serialize};

use crate::FwError;

const UNVERIFIED_DIR: &str = "unverified";

/// Whether cached bytes matched a pin when they were stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trust {
    /// The sha256 matched a pin in a wheel profile.
    Verified,
    /// Kept on request (`--accept-unpinned`); never used where a verified image is required.
    Unverified,
}

/// Where a cached image came from, stored next to it as `<sha256>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Wheel profile id.
    pub wheel: String,
    /// Build name: the pin's label, or the server's title for an unpinned build.
    pub label: String,
    /// Download URL or imported file path.
    pub origin: String,
}

/// One image found in the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CachedImage {
    /// Digest from the file name.
    pub sha256: Sha256,
    /// The image file.
    pub path: PathBuf,
    /// Size in bytes.
    pub size: u64,
    /// Verified or not.
    pub trust: Trust,
    /// The sidecar, when present and readable.
    pub provenance: Option<Provenance>,
    /// False when the bytes no longer hash to the name (disk damage or a manual edit).
    pub intact: bool,
}

/// Content-addressed firmware cache: `<root>/<sha256>.bin` for verified images and
/// `<root>/unverified/<sha256>.bin` for the rest.
///
/// Naming files by digest means a file is its own checksum, two wheels that share a build share
/// the file, and nothing else (labels, URLs) can be confused for identity.
#[derive(Debug, Clone)]
pub struct FirmwareCache {
    root: PathBuf,
}

impl FirmwareCache {
    /// A cache rooted at `root`. Folders are created on first store.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The per-user default: `<platform cache dir>/euc-studio/firmware`
    /// (`~/Library/Caches/euc-studio/firmware` on macOS). `None` if the OS reports no home.
    pub fn default_root() -> Option<PathBuf> {
        ProjectDirs::from("", "", "euc-studio").map(|dirs| dirs.cache_dir().join("firmware"))
    }

    /// The cache folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where an image with this digest and trust lives (whether or not it exists).
    pub fn path(&self, sha256: &Sha256, trust: Trust) -> PathBuf {
        let dir = match trust {
            Trust::Verified => self.root.clone(),
            Trust::Unverified => self.root.join(UNVERIFIED_DIR),
        };
        dir.join(format!("{sha256}.bin"))
    }

    /// The verified image with this digest, if it is cached and still intact.
    pub fn verified(&self, sha256: &Sha256) -> Option<PathBuf> {
        let path = self.path(sha256, Trust::Verified);
        let bytes = fs::read(&path).ok()?;
        (Sha256::digest(&bytes) == *sha256).then_some(path)
    }

    /// Stores `bytes` under their digest with a provenance sidecar and returns the image path.
    ///
    /// Writes go to a temporary file first and are renamed into place, so a crash never leaves a
    /// truncated image under a valid digest.
    pub fn store(&self, bytes: &[u8], trust: Trust, provenance: &Provenance) -> Result<PathBuf, FwError> {
        let sha256 = Sha256::digest(bytes);
        let path = self.path(&sha256, trust);
        let dir = path.parent().unwrap_or(&self.root);
        fs::create_dir_all(dir).map_err(|e| cache_error(dir, e))?;
        let sidecar_path = path.with_extension("json");
        let sidecar = serde_json::to_vec_pretty(provenance).map_err(|e| cache_error(&sidecar_path, e.into()))?;
        write_atomic(&sidecar_path, &sidecar)?;
        write_atomic(&path, bytes)?;
        Ok(path)
    }

    /// Every cached image, verified first, each group sorted by digest.
    pub fn list(&self) -> Result<Vec<CachedImage>, FwError> {
        let mut images = list_dir(&self.root, Trust::Verified)?;
        images.extend(list_dir(&self.root.join(UNVERIFIED_DIR), Trust::Unverified)?);
        Ok(images)
    }
}

fn list_dir(dir: &Path, trust: Trust) -> Result<Vec<CachedImage>, FwError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(cache_error(dir, e)),
    };
    let mut images = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| cache_error(dir, e))?.path();
        let digest = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".bin"));
        let Some(sha256) = digest.and_then(|d| d.parse::<Sha256>().ok()) else { continue };
        let bytes = fs::read(&path).map_err(|e| cache_error(&path, e))?;
        let provenance = fs::read(path.with_extension("json")).ok().and_then(|j| serde_json::from_slice(&j).ok());
        images.push(CachedImage {
            sha256,
            size: bytes.len() as u64,
            trust,
            provenance,
            intact: Sha256::digest(&bytes) == sha256,
            path,
        });
    }
    images.sort_by_key(|image| image.sha256);
    Ok(images)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), FwError> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes).map_err(|e| cache_error(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| cache_error(path, e))
}

fn cache_error(path: &Path, source: io::Error) -> FwError {
    FwError::Cache { path: path.to_owned(), source }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provenance(label: &str) -> Provenance {
        Provenance { wheel: "demo".into(), label: label.into(), origin: "https://example.test/a.bin".into() }
    }

    #[test]
    fn stores_by_digest_and_lists() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = FirmwareCache::new(tmp.path());
        assert_eq!(cache.list().unwrap(), [], "a missing cache folder is an empty cache");

        let path = cache.store(b"stock bytes", Trust::Verified, &provenance("Stock 1")).unwrap();
        let unverified = cache.store(b"new bytes", Trust::Unverified, &provenance("New")).unwrap();
        let sha = Sha256::digest(b"stock bytes");
        assert_eq!(path, tmp.path().join(format!("{sha}.bin")));
        assert_eq!(unverified.parent().unwrap(), tmp.path().join("unverified"));
        assert_eq!(cache.verified(&sha), Some(path.clone()));
        assert_eq!(cache.verified(&Sha256::digest(b"new bytes")), None, "unverified is never verified");

        let listed = cache.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!((listed[0].trust, listed[0].size, listed[0].intact), (Trust::Verified, 11, true));
        assert_eq!(listed[0].provenance, Some(provenance("Stock 1")));
        assert_eq!(listed[1].trust, Trust::Unverified);
    }

    #[test]
    fn damaged_files_are_not_verified() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = FirmwareCache::new(tmp.path());
        let path = cache.store(b"stock bytes", Trust::Verified, &provenance("Stock 1")).unwrap();
        fs::write(&path, b"stock bytez").unwrap();
        assert_eq!(cache.verified(&Sha256::digest(b"stock bytes")), None);
        assert!(!cache.list().unwrap()[0].intact);
    }

    #[test]
    fn ignores_foreign_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("notes.txt"), b"x").unwrap();
        fs::write(tmp.path().join("abc.bin"), b"x").unwrap();
        assert_eq!(FirmwareCache::new(tmp.path()).list().unwrap(), []);
    }

    #[test]
    fn default_root_ends_in_firmware() {
        if let Some(root) = FirmwareCache::default_root() {
            assert!(root.ends_with("firmware"));
            assert!(root.to_string_lossy().contains("euc-studio"));
        }
    }
}
