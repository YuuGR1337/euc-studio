//! What the flasher learns from the wheel's bytes: frames, text replies, and what runs.

use std::collections::VecDeque;
use std::time::Duration;

use euc_ble::{Guard, LinkMode};
use euc_proto_begode::{Assembler, Reply, Segment, Telemetry};

/// How long decoded frames and replies are kept.
const KEEP: Duration = Duration::from_secs(60);

/// The wheel's stream, decoded.
#[derive(Debug, Default)]
pub(crate) struct Observer {
    assembler: Assembler,
    pub(crate) frames: VecDeque<(Duration, Telemetry)>,
    pub(crate) replies: VecDeque<(Duration, Reply)>,
    /// The same rules the transport's guard applies, fed the same bytes: the session asks it
    /// before it writes, so the guard refusing a write means the two disagree (a bug).
    mode: Guard,
}

impl Observer {
    /// One notification that arrived at `at`.
    pub(crate) fn push(&mut self, at: Duration, bytes: &[u8]) {
        self.mode.observe(at, bytes);
        self.assembler.push(bytes);
        while let Some(segment) = self.assembler.next_segment() {
            match segment {
                Segment::Frame(frame) => self.frames.push_back((at, Telemetry::decode(&frame))),
                Segment::Text(text) => {
                    // Bootloader traffic (polls, ACK, NAK) is no reply; skip it.
                    if text.iter().all(|&byte| byte.is_ascii_graphic() || byte == b'\r' || byte == b'\n') {
                        self.replies.push_back((at, Reply::parse(&text)));
                    }
                }
            }
        }
        while self.frames.front().is_some_and(|(time, _)| at.saturating_sub(*time) > KEEP) {
            self.frames.pop_front();
        }
        while self.replies.front().is_some_and(|(time, _)| at.saturating_sub(*time) > KEEP) {
            self.replies.pop_front();
        }
    }

    /// The link came back: what runs must be shown again.
    pub(crate) fn forget(&mut self) {
        self.mode.forget();
        self.assembler = Assembler::new();
    }

    /// Frames received at or after `since`.
    pub(crate) fn frames_since(&self, since: Duration) -> impl Iterator<Item = &(Duration, Telemetry)> {
        self.frames.iter().filter(move |(at, _)| *at >= since)
    }

    /// The first reply at or after `since` that `pick` accepts.
    pub(crate) fn reply_since<T>(&self, since: Duration, pick: impl Fn(&Reply) -> Option<T>) -> Option<T> {
        self.replies.iter().filter(|(at, _)| *at >= since).find_map(|(_, reply)| pick(reply))
    }

    /// Frames arrived within [`euc_ble::APP_FRESH`] of `now`.
    pub(crate) fn app_running(&self, now: Duration) -> bool {
        self.mode.mode(now) == LinkMode::Application
    }

    /// The bootloader's receiver spoke since the frames stopped (and since the link came up).
    pub(crate) fn bootloader_listening(&self, now: Duration) -> bool {
        self.mode.mode(now) == LinkMode::Bootloader
    }
}

#[cfg(test)]
mod tests {
    use euc_proto_begode::{Frame, Telemetry};

    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn frames_replies_and_polls() {
        let mut observer = Observer::default();
        let frame = Frame::new(0x04, 0x18);
        let bytes = frame.as_bytes().to_vec();
        observer.push(ms(0), b"GW1825020");
        observer.push(ms(10), &bytes[..20]);
        observer.push(ms(20), &bytes[20..]);
        assert_eq!(observer.frames.len(), 1);
        assert!(matches!(observer.frames[0].1, Telemetry::Status(_)));
        assert_eq!(observer.reply_since(ms(0), |r| matches!(r, Reply::Version(_)).then_some(())), Some(()));
        assert!(observer.app_running(ms(1000)));
        observer.push(ms(1500), b"C");
        assert!(!observer.bootloader_listening(ms(1500)), "frames too recent");
        assert!(observer.bootloader_listening(ms(2100)));
        observer.push(ms(2200), &[0x06]);
        assert!(observer.bootloader_listening(ms(3000)));
        observer.forget();
        assert!(!observer.bootloader_listening(ms(3100)), "a reconnect needs new evidence");
        observer.push(ms(3200), &[0x15]);
        assert!(observer.bootloader_listening(ms(3200)));
        observer.push(ms(4000), &bytes);
        assert!(!observer.bootloader_listening(ms(4000)));
    }
}
