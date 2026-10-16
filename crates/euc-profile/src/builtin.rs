//! The repo's wheel profiles, compiled into the binary.

use std::path::PathBuf;

use crate::dir::from_text;
use crate::{Origin, ProfileError, WHEEL_FILE, WheelProfile};

/// `(id, wheel.toml)` for every folder in the repo's `profiles/`, sorted by id.
///
/// Embedding them means a plain `euc fetch` trusts only pins that were reviewed and built with
/// the code, never a `profiles/` folder it happens to find on disk. A new wheel is added here.
const PROFILES: &[(&str, &str)] = &[("begode-extreme", include_str!("../../../profiles/begode-extreme/wheel.toml"))];

pub(crate) fn ids() -> Vec<String> {
    PROFILES.iter().map(|(id, _)| (*id).to_owned()).collect()
}

pub(crate) fn load(id: &str) -> Result<WheelProfile, ProfileError> {
    let Some((_, text)) = PROFILES.iter().find(|(known, _)| *known == id) else {
        return Err(ProfileError::UnknownWheel { id: id.to_owned(), searched: Origin::Builtin, available: ids() });
    };
    from_text(text, id, PathBuf::from(format!("built-in:{id}/{WHEEL_FILE}")), Origin::Builtin)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::ProfileDir;

    fn repo_profiles() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../profiles")
    }

    #[test]
    fn every_builtin_profile_loads() {
        for id in ids() {
            let wheel = load(&id).unwrap();
            assert_eq!((wheel.id.as_str(), &wheel.origin), (id.as_str(), &Origin::Builtin));
        }
        let mut sorted = ids();
        sorted.sort();
        assert_eq!(ids(), sorted, "keep PROFILES sorted by id");
    }

    /// The binary must carry exactly what is on disk: a profile edited, added or removed without
    /// rebuilding (or without registering it in `PROFILES`) fails here.
    #[test]
    fn embedded_profiles_match_the_repo() {
        for (id, text) in PROFILES {
            let path = repo_profiles().join(id).join(WHEEL_FILE);
            let on_disk = fs::read_to_string(&path).unwrap();
            assert_eq!(*text, on_disk, "{} differs from the built-in copy; rebuild", path.display());
        }
        assert_eq!(ProfileDir::new(repo_profiles()).ids().unwrap(), ids(), "register every profile folder in PROFILES");
    }

    #[test]
    fn unknown_wheel_names_the_built_in_set() {
        let message = load("nope").unwrap_err().to_string();
        assert_eq!(message, "no wheel profile \"nope\" in the built-in profiles (available: begode-extreme)");
    }
}
