//! What this machine remembers between runs: simulator dry-runs (gate 3) and each wheel's flash
//! history (gate 7).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use euc_profile::Sha256;
use serde::{Deserialize, Serialize};

/// A simulator dry-run of one image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DryRun {
    /// The image.
    pub sha256: Sha256,
    /// Wheel profile id.
    pub wheel: String,
    /// The emulated bootloader took the image and the simulated wheel booted it with frames and
    /// `V` / `N` answers.
    pub flashed: bool,
    /// After that, `!` `@` reached the bootloader again and a second flash booted.
    pub reentry: bool,
    /// A 30 s scenario ride of the image in the emulator. `None`: not run (not implemented yet).
    pub ride: Option<bool>,
}

impl DryRun {
    /// True when every part of gate 3 passed, the ride included.
    pub fn complete(&self) -> bool {
        self.flashed && self.reentry && self.ride == Some(true)
    }
}

/// One flash of a wheel, kept for gate 7.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlashRecord {
    /// The wheel's identity on its link.
    pub peer: String,
    /// The image that ended up booting.
    pub sha256: Sha256,
    /// It was a pinned stock image.
    pub stock: bool,
}

/// Storage for dry-runs and flash history.
pub trait Ledger {
    /// The dry-run of `sha256` for `wheel`, if one was recorded.
    fn dry_run(&self, sha256: &Sha256, wheel: &str) -> Option<DryRun>;
    /// Records a dry-run (replaces an older one of the same image and wheel).
    fn record_dry_run(&mut self, run: DryRun);
    /// Successful flashes of the wheel `peer`.
    fn history(&self, peer: &str) -> Vec<FlashRecord>;
    /// Records a successful flash.
    fn record_flash(&mut self, record: FlashRecord);
}

/// A ledger in memory (the simulator, tests).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryLedger {
    /// Dry-runs.
    pub dry_runs: Vec<DryRun>,
    /// Flashes.
    pub flashes: Vec<FlashRecord>,
}

impl Ledger for MemoryLedger {
    fn dry_run(&self, sha256: &Sha256, wheel: &str) -> Option<DryRun> {
        self.dry_runs.iter().find(|run| run.sha256 == *sha256 && run.wheel == wheel).cloned()
    }

    fn record_dry_run(&mut self, run: DryRun) {
        self.dry_runs.retain(|old| !(old.sha256 == run.sha256 && old.wheel == run.wheel));
        self.dry_runs.push(run);
    }

    fn history(&self, peer: &str) -> Vec<FlashRecord> {
        self.flashes.iter().filter(|record| record.peer == peer).cloned().collect()
    }

    fn record_flash(&mut self, record: FlashRecord) {
        self.flashes.push(record);
    }
}

/// A ledger in a JSON file, written through on every change.
#[derive(Debug)]
pub struct FileLedger {
    path: PathBuf,
    data: MemoryLedger,
    /// The last write failure, if any; the in-memory state stays authoritative for the run.
    pub write_error: Option<String>,
}

impl FileLedger {
    /// Opens `path`; a missing file is an empty ledger.
    pub fn open(path: &Path) -> io::Result<Self> {
        let data = match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(io::Error::other)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => MemoryLedger::default(),
            Err(error) => return Err(error),
        };
        Ok(Self { path: path.to_owned(), data, write_error: None })
    }

    fn save(&mut self) {
        let result = (|| {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)?;
            }
            let text = serde_json::to_string_pretty(&self.data).map_err(io::Error::other)?;
            let temporary = self.path.with_extension("json.tmp");
            fs::write(&temporary, text)?;
            fs::rename(&temporary, &self.path)
        })();
        self.write_error = result.err().map(|error| error.to_string());
    }
}

impl Ledger for FileLedger {
    fn dry_run(&self, sha256: &Sha256, wheel: &str) -> Option<DryRun> {
        self.data.dry_run(sha256, wheel)
    }

    fn record_dry_run(&mut self, run: DryRun) {
        self.data.record_dry_run(run);
        self.save();
    }

    fn history(&self, peer: &str) -> Vec<FlashRecord> {
        self.data.history(peer)
    }

    fn record_flash(&mut self, record: FlashRecord) {
        self.data.record_flash(record);
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(byte: u8, ride: Option<bool>) -> DryRun {
        DryRun { sha256: Sha256::digest(&[byte]), wheel: "w".into(), flashed: true, reentry: true, ride }
    }

    #[test]
    fn memory_ledger_replaces_dry_runs_and_filters_history() {
        let mut ledger = MemoryLedger::default();
        ledger.record_dry_run(run(1, None));
        ledger.record_dry_run(run(1, Some(true)));
        assert_eq!(ledger.dry_runs.len(), 1);
        assert!(ledger.dry_run(&Sha256::digest(&[1]), "w").unwrap().complete());
        assert!(ledger.dry_run(&Sha256::digest(&[1]), "other").is_none());
        assert!(!run(2, None).complete(), "no ride, no gate 3");
        ledger.record_flash(FlashRecord { peer: "a".into(), sha256: Sha256::digest(&[1]), stock: true });
        ledger.record_flash(FlashRecord { peer: "b".into(), sha256: Sha256::digest(&[2]), stock: false });
        assert_eq!(ledger.history("a").len(), 1);
    }

    #[test]
    fn file_ledger_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/ledger.json");
        let mut ledger = FileLedger::open(&path).unwrap();
        ledger.record_dry_run(run(3, None));
        ledger.record_flash(FlashRecord { peer: "a".into(), sha256: Sha256::digest(&[3]), stock: true });
        assert!(ledger.write_error.is_none());
        let again = FileLedger::open(&path).unwrap();
        assert_eq!(again.history("a").len(), 1);
        assert!(again.dry_run(&Sha256::digest(&[3]), "w").is_some());
    }
}
