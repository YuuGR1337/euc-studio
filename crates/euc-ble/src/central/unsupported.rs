//! Systems without a central implementation: every call reports Unsupported.

// The methods keep the macOS handle's signatures, so the shared code stays the same.
#![allow(clippy::unused_self)]

use std::sync::mpsc::Receiver;

use super::{BleError, Event};

/// How one write went (never constructed here).
pub(super) enum Write {
    Sent,
    Busy,
    Gone,
}

pub(super) struct Central;

impl Central {
    pub(super) fn start() -> Result<(Self, Receiver<Event>), BleError> {
        Err(BleError::Unsupported)
    }

    pub(super) fn scan(&self, _on: bool) {}

    pub(super) fn connect(&self, _identifier: &str) -> bool {
        false
    }

    pub(super) fn write(&self, _bytes: &[u8]) -> Write {
        Write::Gone
    }

    pub(super) fn disconnect(&self) {}
}
