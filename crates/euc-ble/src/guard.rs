//! The outbound byte guard.

use std::collections::VecDeque;
use std::time::Duration;

use euc_ymodem::{ACK, CAN, CRC_MODE, EOT, NAK, decode_block};
use serde::Serialize;
use thiserror::Error;

/// Bytes no write may ever be: they stop the motor (`c` `y`), toggle the drive mode and power the
/// wheel off (`+` `-`). The flasher has no use for them.
pub const FORBIDDEN: [u8; 4] = *b"+-cy";
/// Read commands: firmware banner and model name.
pub const READ_COMMANDS: [u8; 2] = *b"VN";
/// Shortest time between two command writes (the protocol page's app guidance).
pub const COMMAND_GAP: Duration = Duration::from_millis(20);
/// Shortest time around a service byte. The stock firmware needs at least 100 ticks (202 ms)
/// between `!` and `@`; the margin covers BLE scheduling.
pub const SERVICE_GAP: Duration = Duration::from_millis(250);
/// A frame this recent proves the application firmware runs.
pub const APP_FRESH: Duration = Duration::from_secs(2);
/// No frame for this long, and the bootloader's own bytes: it listens. Longer than the 0.9 s the
/// stock firmware pauses its telemetry after a received byte.
pub const APP_SILENCE: Duration = Duration::from_secs(2);

const HEADER: [u8; 2] = [0x55, 0xAA];
const FOOTER: [u8; 4] = [0x5A; 4];
const FRAME_LEN: usize = 24;

/// One write the guard judges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outbound<'a> {
    /// One command byte for the application firmware.
    Command(u8),
    /// One whole YMODEM transmission for the bootloader: a block packet, `EOT`, or two `CAN`.
    Ymodem(&'a [u8]),
}

/// What the wheel's own bytes say runs on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LinkMode {
    /// No evidence yet, or old evidence.
    Unknown,
    /// Telemetry frames arrived within [`APP_FRESH`].
    Application,
    /// No frame for [`APP_SILENCE`], and a YMODEM receiver's bytes (a `C` poll, ACK, NAK or
    /// CAN, alone in a notification) arrived after the last frame and since the link came up.
    Bootloader,
}

/// Why the guard refused a write. Nothing was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Refusal {
    /// `+`, `-`, `c` or `y`: never sent.
    #[error("0x{byte:02X} is a service byte that is never sent")]
    Forbidden {
        /// The byte.
        byte: u8,
    },
    /// Not a byte the flasher needs.
    #[error("0x{byte:02X} is not on the allowlist")]
    NotAllowlisted {
        /// The byte.
        byte: u8,
    },
    /// `!` or `@` without an armed entry.
    #[error("bootloader entry is not armed")]
    EntryNotArmed,
    /// After `!` only `@` may follow.
    #[error("only '@' may follow '!'")]
    EntryIncomplete,
    /// `!` needs proof that the application runs.
    #[error("no recent application frames")]
    NoApplication,
    /// Commands go to the application, not to the bootloader.
    #[error("the bootloader is listening; commands are not sent")]
    CommandInBootloader,
    /// Too soon after the previous write.
    #[error("{waited_ms} ms since the previous write; {needed_ms} ms needed")]
    TooSoon {
        /// Time since the previous write.
        waited_ms: u64,
        /// Required gap.
        needed_ms: u64,
    },
    /// YMODEM without the bootloader's proof.
    #[error("the bootloader has not proved it is listening")]
    NotInBootloader,
    /// Not a whole, valid YMODEM transmission.
    #[error("not a whole YMODEM packet, EOT or cancel")]
    NotYmodem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Disarmed,
    Armed,
    Bang { at: Duration },
}

/// Judges every outbound write against what the wheel sent. Feed it every inbound notification
/// with [`observe`](Self::observe), ask [`check`](Self::check) before each write.
#[derive(Debug, Clone)]
pub struct Guard {
    /// The last bytes seen, to find frames split across notifications.
    window: VecDeque<u8>,
    last_frame: Option<Duration>,
    /// The latest notification of YMODEM receiver bytes after the latest frame.
    receiver: Option<Duration>,
    last_write: Option<Duration>,
    entry: Entry,
}

impl Default for Guard {
    fn default() -> Self {
        Self::new()
    }
}

impl Guard {
    /// A guard with no evidence and the entry disarmed.
    pub fn new() -> Self {
        Self {
            window: VecDeque::with_capacity(FRAME_LEN),
            last_frame: None,
            receiver: None,
            last_write: None,
            entry: Entry::Disarmed,
        }
    }

    /// One inbound notification at `now` (the caller's monotonic clock).
    pub fn observe(&mut self, now: Duration, bytes: &[u8]) {
        for &byte in bytes {
            if self.window.len() == FRAME_LEN {
                self.window.pop_front();
            }
            self.window.push_back(byte);
            if self.window.len() == FRAME_LEN
                && self.window.iter().take(2).eq(HEADER.iter())
                && self.window.iter().skip(FRAME_LEN - FOOTER.len()).eq(FOOTER.iter())
            {
                self.last_frame = Some(now);
                self.receiver = None;
            }
        }
        // The receiver's bytes come alone: a notification of nothing but `C`, ACK, NAK or CAN.
        if !bytes.is_empty() && bytes.iter().all(|byte| [CRC_MODE, ACK, NAK, CAN].contains(byte)) {
            self.receiver = Some(now);
        }
    }

    /// Forgets what the wheel showed (frames and receiver bytes), keeping the write history and
    /// the entry state. Transports call it when the link comes back: an application that boots
    /// sends no frame for two seconds, so evidence from before the drop proves nothing.
    pub fn forget(&mut self) {
        self.window.clear();
        self.last_frame = None;
        self.receiver = None;
    }

    /// What runs on the wheel, by its own bytes.
    pub fn mode(&self, now: Duration) -> LinkMode {
        if self.last_frame.is_some_and(|at| now.saturating_sub(at) < APP_FRESH) {
            return LinkMode::Application;
        }
        let quiet = self.last_frame.is_none_or(|at| now.saturating_sub(at) >= APP_SILENCE);
        if quiet && self.receiver.is_some() { LinkMode::Bootloader } else { LinkMode::Unknown }
    }

    /// Allows one `!` `@` sequence. The flasher calls it once every gate passed.
    pub fn arm_entry(&mut self) {
        self.entry = Entry::Armed;
    }

    /// Withdraws an armed or half-sent entry.
    pub fn disarm(&mut self) {
        self.entry = Entry::Disarmed;
    }

    /// True while `!` went out and `@` has not.
    pub const fn entry_pending(&self) -> bool {
        matches!(self.entry, Entry::Bang { .. })
    }

    /// Judges a write at `now`. On `Ok` the caller must send it: the guard records it as sent.
    pub fn check(&mut self, now: Duration, out: Outbound<'_>) -> Result<(), Refusal> {
        match out {
            Outbound::Command(byte) => self.command(now, byte)?,
            Outbound::Ymodem(bytes) => {
                if self.mode(now) != LinkMode::Bootloader {
                    return Err(Refusal::NotInBootloader);
                }
                let whole = bytes == [EOT] || bytes == [CAN, CAN] || decode_block(bytes).is_ok();
                if !whole {
                    return Err(Refusal::NotYmodem);
                }
                self.entry = Entry::Disarmed;
            }
        }
        self.last_write = Some(now);
        Ok(())
    }

    fn command(&mut self, now: Duration, byte: u8) -> Result<(), Refusal> {
        if FORBIDDEN.contains(&byte) {
            return Err(Refusal::Forbidden { byte });
        }
        if let Entry::Bang { at } = self.entry {
            if byte != b'@' {
                return Err(Refusal::EntryIncomplete);
            }
            gap(now.saturating_sub(at), SERVICE_GAP)?;
            self.entry = Entry::Disarmed;
            return Ok(());
        }
        match byte {
            b'!' => {
                if self.entry != Entry::Armed {
                    return Err(Refusal::EntryNotArmed);
                }
                if self.mode(now) != LinkMode::Application {
                    return Err(Refusal::NoApplication);
                }
                self.since_last_write(now, SERVICE_GAP)?;
                self.entry = Entry::Bang { at: now };
                Ok(())
            }
            b'@' => Err(Refusal::EntryNotArmed),
            byte if READ_COMMANDS.contains(&byte) => {
                if self.mode(now) == LinkMode::Bootloader {
                    return Err(Refusal::CommandInBootloader);
                }
                self.since_last_write(now, COMMAND_GAP)
            }
            byte => Err(Refusal::NotAllowlisted { byte }),
        }
    }

    fn since_last_write(&self, now: Duration, needed: Duration) -> Result<(), Refusal> {
        match self.last_write {
            Some(at) => gap(now.saturating_sub(at), needed),
            None => Ok(()),
        }
    }
}

fn gap(waited: Duration, needed: Duration) -> Result<(), Refusal> {
    if waited < needed {
        Err(Refusal::TooSoon { waited_ms: waited.as_millis() as u64, needed_ms: needed.as_millis() as u64 })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use euc_ymodem::{ACK, encode_block};

    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    fn frame() -> Vec<u8> {
        let mut frame = vec![0x55, 0xAA];
        frame.extend_from_slice(&[0x43; 16]);
        frame.extend_from_slice(&[0x04, 0x18, 0x5A, 0x5A, 0x5A, 0x5A]);
        frame
    }

    /// A guard that saw application frames up to `at`, split 20 + 4 as the module sends them.
    fn app_guard(at: Duration) -> Guard {
        let mut guard = Guard::new();
        let frame = frame();
        guard.observe(at, &frame[..20]);
        guard.observe(at, &frame[20..]);
        guard
    }

    fn bootloader_guard(at: Duration) -> Guard {
        let mut guard = app_guard(Duration::ZERO);
        guard.observe(at, b"C");
        guard
    }

    #[test]
    fn service_bytes_are_never_sent() {
        for mut guard in [Guard::new(), app_guard(ms(0)), bootloader_guard(ms(5000))] {
            guard.arm_entry();
            for byte in FORBIDDEN {
                assert_eq!(guard.check(ms(9000), Outbound::Command(byte)), Err(Refusal::Forbidden { byte }));
            }
        }
        let mut guard = app_guard(ms(0));
        guard.arm_entry();
        guard.check(ms(300), Outbound::Command(b'!')).unwrap();
        assert_eq!(guard.check(ms(600), Outbound::Command(b'c')), Err(Refusal::Forbidden { byte: b'c' }));
    }

    #[test]
    fn only_read_commands_pass_without_an_entry() {
        let mut guard = app_guard(ms(0));
        assert_eq!(guard.check(ms(10), Outbound::Command(b'V')), Ok(()));
        assert_eq!(
            guard.check(ms(20), Outbound::Command(b'N')),
            Err(Refusal::TooSoon { waited_ms: 10, needed_ms: 20 })
        );
        assert_eq!(guard.check(ms(40), Outbound::Command(b'N')), Ok(()));
        for byte in [b'b', b'W', b'Q', b'k', b'h', 0x00, 0xFF] {
            assert_eq!(guard.check(ms(500), Outbound::Command(byte)), Err(Refusal::NotAllowlisted { byte }));
        }
        assert_eq!(guard.check(ms(500), Outbound::Command(b'!')), Err(Refusal::EntryNotArmed));
        assert_eq!(guard.check(ms(500), Outbound::Command(b'@')), Err(Refusal::EntryNotArmed));
    }

    #[test]
    fn entry_needs_arming_app_frames_and_the_gap() {
        let mut guard = Guard::new();
        guard.arm_entry();
        assert_eq!(guard.check(ms(100), Outbound::Command(b'!')), Err(Refusal::NoApplication));

        let mut guard = app_guard(ms(0));
        guard.arm_entry();
        guard.check(ms(100), Outbound::Command(b'V')).unwrap();
        assert_eq!(
            guard.check(ms(200), Outbound::Command(b'!')),
            Err(Refusal::TooSoon { waited_ms: 100, needed_ms: 250 })
        );
        guard.check(ms(400), Outbound::Command(b'!')).unwrap();
        assert!(guard.entry_pending());
        assert_eq!(guard.check(ms(450), Outbound::Command(b'V')), Err(Refusal::EntryIncomplete));
        assert_eq!(
            guard.check(ms(500), Outbound::Command(b'@')),
            Err(Refusal::TooSoon { waited_ms: 100, needed_ms: 250 })
        );
        guard.check(ms(700), Outbound::Command(b'@')).unwrap();
        assert!(!guard.entry_pending());
        // Spent: a second sequence needs a new arming.
        assert_eq!(guard.check(ms(2000), Outbound::Command(b'!')), Err(Refusal::EntryNotArmed));
    }

    #[test]
    fn stale_frames_do_not_prove_the_application() {
        let mut guard = app_guard(ms(0));
        guard.arm_entry();
        assert_eq!(guard.check(ms(2500), Outbound::Command(b'!')), Err(Refusal::NoApplication));
        assert_eq!(guard.mode(ms(2500)), LinkMode::Unknown);
    }

    #[test]
    fn ymodem_needs_the_bootloader_poll_after_the_frames_stop() {
        let block = encode_block(1, &[0x2B; 128]); // full of '+' bytes: data, not commands
        let mut guard = app_guard(ms(0));
        assert_eq!(guard.check(ms(500), Outbound::Ymodem(&block)), Err(Refusal::NotInBootloader));
        guard.observe(ms(1000), b"C");
        assert_eq!(guard.mode(ms(1000)), LinkMode::Application, "frames stopped too recently");
        assert_eq!(guard.check(ms(1000), Outbound::Ymodem(&block)), Err(Refusal::NotInBootloader));
        assert_eq!(guard.mode(ms(2100)), LinkMode::Bootloader);
        assert_eq!(guard.check(ms(2100), Outbound::Ymodem(&block)), Ok(()));
        assert_eq!(guard.check(ms(2100), Outbound::Ymodem(&[EOT])), Ok(()));
        assert_eq!(guard.check(ms(2100), Outbound::Ymodem(&[CAN, CAN])), Ok(()));
        // ACKs and NAKs from the receiver keep the mode.
        guard.observe(ms(2200), &[ACK]);
        assert_eq!(guard.mode(ms(9000)), LinkMode::Bootloader);
        // After a reconnect the evidence is gone until the receiver speaks again.
        guard.forget();
        assert_eq!(guard.check(ms(9050), Outbound::Ymodem(&block)), Err(Refusal::NotInBootloader));
        guard.observe(ms(9060), &[NAK]);
        assert_eq!(guard.check(ms(9060), Outbound::Ymodem(&block)), Ok(()));
        // The application is back: no more YMODEM.
        let frame = frame();
        guard.observe(ms(9100), &frame);
        assert_eq!(guard.check(ms(9100), Outbound::Ymodem(&block)), Err(Refusal::NotInBootloader));
        assert_eq!(
            guard.check(ms(20_000), Outbound::Ymodem(&block)),
            Err(Refusal::NotInBootloader),
            "needs a new poll"
        );
    }

    #[test]
    fn a_c_inside_a_frame_is_not_a_poll() {
        let mut guard = Guard::new();
        let frame = frame(); // its payload is all 0x43, 'C'
        guard.observe(ms(0), &frame[..20]);
        guard.observe(ms(0), &frame[20..]);
        assert_eq!(guard.mode(ms(5000)), LinkMode::Unknown);
    }

    #[test]
    fn only_whole_ymodem_transmissions_pass() {
        let mut guard = bootloader_guard(ms(3000));
        let block = encode_block(1, &[7; 128]);
        assert_eq!(guard.check(ms(3000), Outbound::Ymodem(&block[..20])), Err(Refusal::NotYmodem));
        let mut bad = block.clone();
        bad[40] ^= 1;
        assert_eq!(guard.check(ms(3000), Outbound::Ymodem(&bad)), Err(Refusal::NotYmodem));
        assert_eq!(guard.check(ms(3000), Outbound::Ymodem(b"V")), Err(Refusal::NotYmodem));
        assert_eq!(guard.check(ms(3000), Outbound::Command(b'V')), Err(Refusal::CommandInBootloader));
    }
}
