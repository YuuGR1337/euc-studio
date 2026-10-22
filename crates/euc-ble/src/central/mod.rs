//! The CoreBluetooth central: scan for wheels, connect to one, read its notifications and, only
//! when the write policy is armed, write through the guard.
//!
//! macOS only; elsewhere every call reports [`BleError::Unsupported`]. This module is compiled
//! with the `central` feature, which no default build enables. It has never been run against a
//! wheel: the first run on hardware waits for an SWD backup and the owner's go-ahead (SAFETY.md).

// Elsewhere the platform layer is a stub that never connects, so the events and the link code
// it would feed are never constructed there.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(target_os = "macos"))]
mod unsupported;
#[cfg(not(target_os = "macos"))]
use unsupported as platform;

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::Serialize;
use thiserror::Error;

use crate::{Guard, MTU, Outbound, Refusal, WritePolicy};

/// A wheel seen while scanning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found {
    /// CoreBluetooth's identifier for the peripheral (stable on this Mac, not the MAC address).
    pub identifier: String,
    /// Advertised name, when there is one.
    pub name: Option<String>,
    /// Signal strength in dBm.
    pub rssi: i64,
}

/// What can go wrong on the link.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "kebab-case")]
pub enum BleError {
    /// No CoreBluetooth central on this system.
    #[error("this system has no supported Bluetooth central")]
    Unsupported,
    /// Bluetooth access was denied, or Bluetooth is off or unavailable.
    #[error("Bluetooth is not available: {0}")]
    Unavailable(String),
    /// The wheel was not seen while scanning.
    #[error("no wheel with that identifier was found")]
    NotFound,
    /// Connecting failed.
    #[error("cannot connect: {0}")]
    Connect(String),
    /// The wheel's service or characteristic did not come up.
    #[error("the wheel's FFE0/FFE1 characteristic is not available: {0}")]
    Characteristic(String),
    /// The link dropped.
    #[error("the link dropped")]
    Disconnected,
    /// A wait ran out.
    #[error("timed out")]
    Timeout,
    /// The write policy is listen-only: nothing is written to a real wheel without the override.
    #[error("real writes are disabled; see EUC_BLE_REAL_WRITES in SAFETY.md")]
    ListenOnly,
    /// The guard refused the write.
    #[error("refused: {0}")]
    Refused(Refusal),
}

/// What the platform layer reports.
#[derive(Debug)]
pub(crate) enum Event {
    State(&'static str),
    Discovered(Found),
    Connected,
    ConnectFailed(String),
    Disconnected,
    Ready,
    NotReady(String),
    /// Bytes, stamped when CoreBluetooth delivered them.
    Notification(Instant, Vec<u8>),
}

/// Waits up to `timeout` for an event `pick` accepts; other events are handed to `other`.
fn wait_for<T>(
    events: &Receiver<Event>,
    timeout: Duration,
    mut pick: impl FnMut(&Event) -> Option<T>,
    mut other: impl FnMut(Event),
) -> Result<T, BleError> {
    let deadline = Instant::now() + timeout;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(event) => match pick(&event) {
                Some(value) => return Ok(value),
                None => other(event),
            },
            Err(RecvTimeoutError::Timeout) => return Err(BleError::Timeout),
            Err(RecvTimeoutError::Disconnected) => return Err(BleError::Unsupported),
        }
    }
}

/// A Bluetooth central, powered on.
pub struct Central {
    inner: platform::Central,
    events: Receiver<Event>,
}

impl Central {
    /// Starts the central and waits up to `timeout` for Bluetooth to be on.
    pub fn new(timeout: Duration) -> Result<Self, BleError> {
        let (inner, events) = platform::Central::start()?;
        // "unknown" and "resetting" are passing states; wait for one that settles it.
        let settled = |event: &Event| match event {
            Event::State(state) if !matches!(*state, "unknown" | "resetting") => Some(*state),
            _ => None,
        };
        let state = wait_for(&events, timeout, settled, drop)?;
        if state != "poweredOn" {
            return Err(BleError::Unavailable(state.to_owned()));
        }
        Ok(Self { inner, events })
    }

    /// Scans for `duration`; returns the peripherals that advertise the wheel service and whose
    /// name starts with `prefix` (empty: all), strongest first.
    pub fn scan(&self, prefix: &str, duration: Duration) -> Result<Vec<Found>, BleError> {
        self.inner.scan(true);
        let mut found: Vec<Found> = Vec::new();
        let result = wait_for(
            &self.events,
            duration,
            |_| None::<()>,
            |event| {
                if let Event::Discovered(peer) = event {
                    let named = peer.name.as_deref().unwrap_or_default().starts_with(prefix);
                    if named && !found.iter().any(|f| f.identifier == peer.identifier) {
                        found.push(peer);
                    }
                }
            },
        );
        self.inner.scan(false);
        if let Err(error) = result
            && error != BleError::Timeout
        {
            return Err(error);
        }
        found.sort_by_key(|f| std::cmp::Reverse(f.rssi));
        Ok(found)
    }

    /// Connects to the peripheral with `identifier` (seen by a scan of this central, or scanned
    /// for now) and subscribes to its notifications. The write policy comes from the environment.
    pub fn connect(self, identifier: &str, timeout: Duration) -> Result<Link, BleError> {
        let policy = WritePolicy::from_env(identifier);
        let mut link = Link {
            inner: self.inner,
            events: self.events,
            peer: identifier.to_owned(),
            guard: Guard::new(),
            policy,
            started: Instant::now(),
            pending: Vec::new(),
        };
        link.open(timeout)?;
        Ok(link)
    }
}

/// A connection to one wheel.
pub struct Link {
    inner: platform::Central,
    events: Receiver<Event>,
    peer: String,
    guard: Guard,
    policy: WritePolicy,
    started: Instant,
    /// Notifications that arrived while waiting for something else.
    pending: Vec<(Instant, Vec<u8>)>,
}

impl Link {
    fn open(&mut self, timeout: Duration) -> Result<(), BleError> {
        if !self.inner.connect(&self.peer) {
            // Not seen yet: scan for it.
            self.inner.scan(true);
            let peer = self.peer.clone();
            let seen = wait_for(
                &self.events,
                timeout,
                |event| match event {
                    Event::Discovered(found) if found.identifier == peer => Some(()),
                    _ => None,
                },
                drop,
            );
            self.inner.scan(false);
            seen.map_err(|_| BleError::NotFound)?;
            if !self.inner.connect(&self.peer) {
                return Err(BleError::NotFound);
            }
        }
        let pending = &mut self.pending;
        let result = wait_for(
            &self.events,
            timeout,
            |event| match event {
                Event::Ready => Some(Ok(())),
                Event::ConnectFailed(why) => Some(Err(BleError::Connect(why.clone()))),
                Event::NotReady(why) => Some(Err(BleError::Characteristic(why.clone()))),
                Event::Disconnected => Some(Err(BleError::Disconnected)),
                _ => None,
            },
            |event| {
                if let Event::Notification(at, bytes) = event {
                    pending.push((at, bytes));
                }
            },
        );
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) | Err(error) => {
                self.inner.disconnect();
                Err(error)
            }
        }
    }

    /// The wheel's BLE identifier.
    pub fn peer(&self) -> &str {
        &self.peer
    }

    /// The write policy of this connection.
    pub const fn policy(&self) -> &WritePolicy {
        &self.policy
    }

    /// The guard, with what it saw.
    pub const fn guard(&self) -> &Guard {
        &self.guard
    }

    /// Allows one `!` `@` sequence (the flasher calls this once every gate passed).
    pub fn arm_entry(&mut self) {
        self.guard.arm_entry();
    }

    /// Time since the connection started; the guard's clock.
    pub fn now(&self) -> Duration {
        self.started.elapsed()
    }

    /// The next notification and its arrival time on [`now`](Self::now)'s clock, waiting up
    /// to `timeout`. `Ok(None)` when none arrived.
    pub fn recv(&mut self, timeout: Duration) -> Result<Option<(Duration, Vec<u8>)>, BleError> {
        let (at, bytes) = if self.pending.is_empty() {
            match wait_for(
                &self.events,
                timeout,
                |event| match event {
                    Event::Notification(at, bytes) => Some(Ok((*at, bytes.clone()))),
                    Event::Disconnected => Some(Err(BleError::Disconnected)),
                    _ => None,
                },
                drop,
            ) {
                Ok(result) => result?,
                Err(BleError::Timeout) => return Ok(None),
                Err(error) => return Err(error),
            }
        } else {
            self.pending.remove(0)
        };
        let at = at.saturating_duration_since(self.started);
        self.guard.observe(at, &bytes);
        Ok(Some((at, bytes)))
    }

    /// Writes one command or YMODEM transmission: the policy first, then the guard, then
    /// writes without response of at most [`MTU`] bytes, paced by CoreBluetooth's flow control.
    pub fn send(&mut self, out: Outbound<'_>) -> Result<(), BleError> {
        if !self.policy.allows_writes() {
            return Err(BleError::ListenOnly);
        }
        let now = self.now();
        self.guard.check(now, out).map_err(BleError::Refused)?;
        let bytes = match out {
            Outbound::Command(byte) => vec![byte],
            Outbound::Ymodem(bytes) => bytes.to_vec(),
        };
        for chunk in bytes.chunks(MTU) {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match self.inner.write(chunk) {
                    platform::Write::Sent => break,
                    platform::Write::Gone => return Err(BleError::Disconnected),
                    platform::Write::Busy if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    platform::Write::Busy => return Err(BleError::Timeout),
                }
            }
        }
        Ok(())
    }

    /// Connects again after the link dropped.
    pub fn reconnect(&mut self, timeout: Duration) -> Result<(), BleError> {
        self.inner.disconnect();
        self.pending.clear();
        self.guard.forget();
        self.open(timeout)
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.inner.disconnect();
    }
}
