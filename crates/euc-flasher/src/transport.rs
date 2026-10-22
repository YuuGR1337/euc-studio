//! What the flasher needs from a link to a wheel.

use std::time::Duration;

use euc_ble::{Outbound, Refusal};
use serde::Serialize;
use thiserror::Error;

/// Bytes the wheel sent, with the time they arrived (the transport's clock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inbound {
    /// Arrival time, from [`Transport::now`]'s clock.
    pub at: Duration,
    /// One notification.
    pub bytes: Vec<u8>,
}

/// Which kind of link a session ran on; logged with every run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkKind {
    /// The simulated wheel: virtual time, emulated bootloader.
    Simulator,
    /// A real wheel over Bluetooth.
    Ble,
}

/// A link failure. Nothing more was sent.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "kebab-case")]
pub enum LinkError {
    /// The connection dropped (or the wheel lost power).
    #[error("the link dropped")]
    Lost,
    /// The outbound guard refused the write.
    #[error("the guard refused the write: {0}")]
    Refused(Refusal),
    /// Real writes are disabled for this wheel.
    #[error("real writes are disabled (listen-only)")]
    ListenOnly,
    /// Anything else the link reports.
    #[error("{0}")]
    Other(String),
}

/// A link to one wheel. Every write goes through [`euc_ble::Guard`] inside the transport.
pub trait Transport {
    /// Which kind of link this is.
    fn kind(&self) -> LinkKind;
    /// The wheel's stable identity on this link (a BLE identifier).
    fn peer(&self) -> String;
    /// Monotonic time since the transport started; virtual in the simulator.
    fn now(&self) -> Duration;
    /// Sends one command or one whole YMODEM transmission. Returns once the last write left.
    fn send(&mut self, out: Outbound<'_>) -> Result<(), LinkError>;
    /// Allows one `!` `@` sequence through the guard.
    fn arm_entry(&mut self);
    /// The next notification, waiting at most `timeout`; `Ok(None)` when none arrived.
    fn recv(&mut self, timeout: Duration) -> Result<Option<Inbound>, LinkError>;
    /// Connects again after [`LinkError::Lost`], waiting at most `timeout`.
    fn reconnect(&mut self, timeout: Duration) -> Result<(), LinkError>;
}

/// The person at the wheel. The flasher asks; a terminal, the studio or a script answers.
pub trait Operator {
    /// Shows progress or an instruction.
    fn say(&mut self, text: &str);
    /// Gate 6: returns what the person typed when asked to type `expected`.
    fn confirm(&mut self, expected: &str) -> String;
    /// Asks for a short press of the wheel's power button now (our clean firmware asks for it
    /// after `!` `@`). Returns when the person says they pressed it.
    fn press_power_button(&mut self);
    /// The wheel stopped answering: asks the person to switch it off and on again. False when
    /// they decline.
    fn power_cycle_wheel(&mut self) -> bool;
}
