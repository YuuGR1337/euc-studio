//! `BleTransport`: a real wheel over `euc-ble`'s CoreBluetooth central (feature `ble`).
//!
//! No shipped binary enables this feature. Even with it, every write is refused unless
//! `EUC_BLE_REAL_WRITES` names the wheel (SAFETY.md), and the guard judges each write. Gates 2 and
//! 3 refuse every real flash today. This transport has never been connected to a wheel.

use std::time::Duration;

use euc_ble::Outbound;
use euc_ble::central::{BleError, Central, Link};

use crate::transport::{Inbound, LinkError, LinkKind, Transport};

/// A connection to one real wheel.
pub struct BleTransport {
    link: Link,
}

impl BleTransport {
    /// Starts the central and connects to the peripheral with `identifier` (from a scan).
    pub fn connect(identifier: &str, timeout: Duration) -> Result<Self, BleError> {
        let central = Central::new(timeout)?;
        Ok(Self { link: central.connect(identifier, timeout)? })
    }

    /// The connection's write policy (listen-only unless the override names this wheel).
    pub const fn policy(&self) -> &euc_ble::WritePolicy {
        self.link.policy()
    }
}

fn link_error(error: BleError) -> LinkError {
    match error {
        BleError::Disconnected => LinkError::Lost,
        BleError::ListenOnly => LinkError::ListenOnly,
        BleError::Refused(refusal) => LinkError::Refused(refusal),
        other => LinkError::Other(other.to_string()),
    }
}

impl Transport for BleTransport {
    fn kind(&self) -> LinkKind {
        LinkKind::Ble
    }

    fn peer(&self) -> String {
        self.link.peer().to_owned()
    }

    fn now(&self) -> Duration {
        self.link.now()
    }

    fn send(&mut self, out: Outbound<'_>) -> Result<(), LinkError> {
        self.link.send(out).map_err(link_error)
    }

    fn arm_entry(&mut self) {
        self.link.arm_entry();
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<Inbound>, LinkError> {
        match self.link.recv(timeout) {
            Ok(Some((at, bytes))) => Ok(Some(Inbound { at, bytes })),
            Ok(None) => Ok(None),
            Err(error) => Err(link_error(error)),
        }
    }

    fn reconnect(&mut self, timeout: Duration) -> Result<(), LinkError> {
        self.link.reconnect(timeout).map_err(link_error)
    }
}
