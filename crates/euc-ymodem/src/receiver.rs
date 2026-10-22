//! The receiving side: one file in YMODEM batch mode, as a bootloader would take it.
//!
//! The simulator's bootloader model runs this receiver; the tests use it as the sender's peer.

use serde::Serialize;
use thiserror::Error;

use crate::packet::{ACK, CAN, CRC_MODE, EOT, NAK, cancel, decode_block, packet_len, parse_header};

/// Receiver limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverConfig {
    /// Largest file accepted; a bigger header cancels the transfer.
    pub max_size: u64,
    /// Refuse headers without a size field (a bootloader needs the length to verify the image).
    pub require_size: bool,
    /// Bad packets and timeouts in a row, once data flows, before the receiver cancels.
    pub max_errors: u8,
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self { max_size: 16 * 1024 * 1024, require_size: true, max_errors: 10 }
    }
}

/// Why the receiver cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ReceiveError {
    /// The header announced a file larger than `max_size`, or gave no size when one is required.
    #[error("the header announced an unacceptable size")]
    Size,
    /// A block arrived out of order: the sender and the receiver lost step.
    #[error("block {found} arrived while block {expected} was due")]
    Sequence {
        /// Sequence byte expected (mod 256).
        expected: u8,
        /// Sequence byte received.
        found: u8,
    },
    /// Too many bad packets or timeouts in a row.
    #[error("too many errors in a row")]
    TooManyErrors,
    /// More data than the header announced.
    #[error("more data than the header announced")]
    Overflow,
    /// The batch held a second file; this receiver takes one.
    #[error("a second file was offered")]
    SecondFile,
    /// Two `CAN` bytes arrived.
    #[error("the sender cancelled the transfer")]
    CancelledBySender,
}

/// Where the receiver stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ReceiveStatus {
    /// Waiting for a header (polling with `C`).
    Idle,
    /// A file is coming in.
    Receiving,
    /// The file is complete; `closed` tells whether the batch-closing header arrived.
    Done {
        /// The empty header arrived.
        closed: bool,
    },
    /// The receiver cancelled or was cancelled.
    Failed(ReceiveError),
}

/// A complete file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedFile {
    /// Name from the header.
    pub name: String,
    /// Bytes, cut to the header's size (padding removed).
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    AwaitHeader,
    AwaitData,
    AwaitClose,
    Done { closed: bool },
    Failed(ReceiveError),
}

/// Receives one file. Feed it everything the link delivers; call [`timeout`](Self::timeout) when
/// the line was quiet for the receiver's timeout (about a second while polling for a header,
/// longer than one packet's transmission once data flows).
#[derive(Debug, Clone)]
pub struct Receiver {
    config: ReceiverConfig,
    state: State,
    /// Bytes of a packet that has not arrived completely.
    partial: Vec<u8>,
    /// After a bad start byte: drop everything until the line goes quiet.
    purging: bool,
    name: String,
    size: Option<u64>,
    data: Vec<u8>,
    /// Next block number (not wrapped).
    next: u32,
    errors: u8,
    /// An `EOT` was refused and no block came after it.
    eot_pending: bool,
    cans: u8,
}

impl Receiver {
    /// A receiver waiting for a header.
    pub fn new(config: ReceiverConfig) -> Self {
        Self {
            config,
            state: State::AwaitHeader,
            partial: Vec::new(),
            purging: false,
            name: String::new(),
            size: None,
            data: Vec::new(),
            next: 1,
            errors: 0,
            eot_pending: false,
            cans: 0,
        }
    }

    /// The first `C`, to send when the receiver starts listening.
    pub fn start(&self) -> Vec<u8> {
        vec![CRC_MODE]
    }

    /// Where the transfer stands.
    pub const fn status(&self) -> ReceiveStatus {
        match self.state {
            State::AwaitHeader => ReceiveStatus::Idle,
            State::AwaitData | State::AwaitClose => ReceiveStatus::Receiving,
            State::Done { closed } => ReceiveStatus::Done { closed },
            State::Failed(error) => ReceiveStatus::Failed(error),
        }
    }

    /// True while bytes of an incomplete packet are buffered.
    pub fn mid_packet(&self) -> bool {
        !self.partial.is_empty() || self.purging
    }

    /// Data bytes accepted so far (block padding included).
    pub fn received(&self) -> usize {
        self.data.len()
    }

    /// The file, once complete (once the end of file was acknowledged).
    pub fn file(&self) -> Option<ReceivedFile> {
        let complete = matches!(self.state, State::AwaitClose | State::Done { .. });
        complete.then(|| {
            let mut data = self.data.clone();
            if let Some(size) = self.size {
                data.truncate(size as usize);
            }
            ReceivedFile { name: self.name.clone(), data }
        })
    }

    /// Bytes from the link. Returns the bytes to send back.
    pub fn receive(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for &byte in bytes {
            if matches!(self.state, State::Done { .. } | State::Failed(_)) {
                break;
            }
            self.byte(byte, &mut out);
        }
        out
    }

    /// The line was quiet for the receiver's timeout. Returns what to send.
    pub fn timeout(&mut self) -> Vec<u8> {
        let had_partial = self.mid_packet();
        self.partial.clear();
        self.purging = false;
        match self.state {
            // Polling for a header never gives up: a bootloader waits as long as it has power.
            State::AwaitHeader => vec![CRC_MODE],
            State::AwaitData => {
                if had_partial || self.next > 1 || self.eot_pending {
                    self.error(NAK)
                } else {
                    // Header acknowledged, first block not here yet: ask again.
                    self.error(CRC_MODE)
                }
            }
            State::AwaitClose => {
                self.errors += 1;
                if self.errors >= self.config.max_errors {
                    self.state = State::Done { closed: false };
                    Vec::new()
                } else {
                    vec![CRC_MODE]
                }
            }
            State::Done { .. } | State::Failed(_) => Vec::new(),
        }
    }

    fn byte(&mut self, byte: u8, out: &mut Vec<u8>) {
        if self.purging {
            return;
        }
        if self.partial.is_empty() {
            if byte == CAN {
                self.cans += 1;
                if self.cans >= 2 {
                    self.state = State::Failed(ReceiveError::CancelledBySender);
                }
                return;
            }
            self.cans = 0;
            match byte {
                EOT => self.eot(out),
                start if packet_len(start).is_some() => self.partial.push(start),
                // Garbage between packets: wait for the line to go quiet, then refuse.
                _ if self.state == State::AwaitData => self.purging = true,
                _ => {}
            }
            return;
        }
        self.partial.push(byte);
        let len = packet_len(self.partial[0]).unwrap_or(0);
        if self.partial.len() == len {
            let packet = std::mem::take(&mut self.partial);
            self.packet(&packet, out);
        }
    }

    fn packet(&mut self, packet: &[u8], out: &mut Vec<u8>) {
        let Ok(block) = decode_block(packet) else {
            out.extend(self.error(NAK));
            return;
        };
        match self.state {
            State::AwaitHeader => {
                if block.seq != 0 {
                    // A data block before any header: this sender is in another session.
                    self.fail(ReceiveError::Sequence { expected: 0, found: block.seq }, out);
                    return;
                }
                let Some(info) = parse_header(&block.data) else {
                    // An empty batch: nothing to receive. Keep polling.
                    out.extend_from_slice(&[ACK, CRC_MODE]);
                    return;
                };
                let too_big = info.size.is_some_and(|size| size > self.config.max_size);
                if too_big || (self.config.require_size && info.size.is_none()) {
                    self.fail(ReceiveError::Size, out);
                    return;
                }
                self.name = info.name;
                self.size = info.size;
                self.state = State::AwaitData;
                self.errors = 0;
                out.extend_from_slice(&[ACK, CRC_MODE]);
            }
            State::AwaitData => {
                let expected = self.next as u8;
                if block.seq == expected {
                    if self.size.is_some_and(|size| self.data.len() as u64 >= size) {
                        self.fail(ReceiveError::Overflow, out);
                        return;
                    }
                    self.data.extend_from_slice(&block.data);
                    self.next += 1;
                    self.errors = 0;
                    self.eot_pending = false;
                    out.push(ACK);
                } else if block.seq == expected.wrapping_sub(1) {
                    // The sender missed our ACK: acknowledge again, keep nothing. Block 0
                    // repeats the header while block 1 is due.
                    out.push(ACK);
                    if self.next == 1 {
                        out.push(CRC_MODE);
                    }
                } else {
                    self.fail(ReceiveError::Sequence { expected, found: block.seq }, out);
                }
            }
            State::AwaitClose => {
                if block.seq != 0 {
                    self.fail(ReceiveError::Sequence { expected: 0, found: block.seq }, out);
                } else if parse_header(&block.data).is_some() {
                    self.fail(ReceiveError::SecondFile, out);
                } else {
                    out.push(ACK);
                    self.state = State::Done { closed: true };
                }
            }
            State::Done { .. } | State::Failed(_) => {}
        }
    }

    fn eot(&mut self, out: &mut Vec<u8>) {
        match self.state {
            State::AwaitData => {
                let complete = self.size.is_none_or(|size| self.data.len() as u64 >= size);
                if !complete {
                    // An EOT before the announced length is noise or a lost block: refuse it.
                    out.extend(self.error(NAK));
                } else if self.eot_pending {
                    out.extend_from_slice(&[ACK, CRC_MODE]);
                    self.state = State::AwaitClose;
                    self.errors = 0;
                } else {
                    // Refuse the first EOT, as `rb` does, so a stray 0x04 cannot end the file.
                    self.eot_pending = true;
                    out.push(NAK);
                }
            }
            // The sender missed our ACK of the EOT.
            State::AwaitClose => out.extend_from_slice(&[ACK, CRC_MODE]),
            _ => {}
        }
    }

    /// Counts one error; past the limit the receiver cancels. Returns what to send.
    fn error(&mut self, reply: u8) -> Vec<u8> {
        self.errors += 1;
        if self.errors >= self.config.max_errors {
            self.state = State::Failed(ReceiveError::TooManyErrors);
            return cancel().to_vec();
        }
        vec![reply]
    }

    fn fail(&mut self, error: ReceiveError, out: &mut Vec<u8>) {
        self.state = State::Failed(error);
        out.extend_from_slice(&cancel());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{SHORT, encode_block, header_payload};

    fn header(name: &str, size: u64) -> Vec<u8> {
        encode_block(0, &header_payload(Some((name, size))).unwrap())
    }

    fn block(seq: u8, fill: u8) -> Vec<u8> {
        encode_block(seq, &[fill; SHORT])
    }

    #[test]
    fn receives_a_file_and_cuts_the_padding() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        assert_eq!(rx.start(), vec![CRC_MODE]);
        assert_eq!(rx.receive(&header("a.bin", 200)), vec![ACK, CRC_MODE]);
        assert_eq!(rx.status(), ReceiveStatus::Receiving);
        assert_eq!(rx.receive(&block(1, 0xAA)), vec![ACK]);
        assert_eq!(rx.receive(&block(2, 0xBB)), vec![ACK]);
        assert_eq!(rx.receive(&[EOT]), vec![NAK]);
        assert_eq!(rx.receive(&[EOT]), vec![ACK, CRC_MODE]);
        let file = rx.file().unwrap();
        assert_eq!((file.name.as_str(), file.data.len()), ("a.bin", 200));
        assert!(file.data[..128].iter().all(|&b| b == 0xAA) && file.data[128..].iter().all(|&b| b == 0xBB));
        assert_eq!(rx.receive(&encode_block(0, &[0; SHORT])), vec![ACK]);
        assert_eq!(rx.status(), ReceiveStatus::Done { closed: true });
    }

    #[test]
    fn packets_may_arrive_in_pieces() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        let packet = header("a.bin", 10);
        assert_eq!(rx.receive(&packet[..20]), []);
        assert!(rx.mid_packet());
        assert_eq!(rx.receive(&packet[20..]), vec![ACK, CRC_MODE]);
    }

    #[test]
    fn duplicates_are_acknowledged_not_stored() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        rx.receive(&header("a.bin", 256));
        assert_eq!(rx.receive(&header("a.bin", 256)), vec![ACK, CRC_MODE], "repeated header");
        rx.receive(&block(1, 1));
        assert_eq!(rx.receive(&block(1, 1)), vec![ACK]);
        assert_eq!(rx.received(), 128);
    }

    #[test]
    fn bad_crc_is_refused() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        rx.receive(&header("a.bin", 128));
        let mut bad = block(1, 7);
        bad[50] ^= 0x10;
        assert_eq!(rx.receive(&bad), vec![NAK]);
        assert_eq!(rx.receive(&block(1, 7)), vec![ACK]);
    }

    #[test]
    fn a_lost_byte_is_refused_after_the_timeout() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        rx.receive(&header("a.bin", 128));
        let packet = block(1, 7);
        assert_eq!(rx.receive(&packet[..packet.len() - 1]), []);
        assert_eq!(rx.timeout(), vec![NAK]);
        assert_eq!(rx.receive(&packet), vec![ACK]);
    }

    #[test]
    fn garbage_is_purged_until_the_line_is_quiet() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        rx.receive(&header("a.bin", 128));
        // A corrupted start byte: the rest of the packet must not be read as packets or EOTs.
        let mut packet = block(1, EOT);
        packet[0] = 0x55;
        assert_eq!(rx.receive(&packet), []);
        assert_eq!(rx.timeout(), vec![NAK]);
        assert_eq!(rx.receive(&block(1, EOT)), vec![ACK]);
    }

    #[test]
    fn early_eot_is_refused() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        rx.receive(&header("a.bin", 300));
        rx.receive(&block(1, 1));
        assert_eq!(rx.receive(&[EOT]), vec![NAK]);
        assert_eq!(rx.receive(&[EOT]), vec![NAK], "still short: no ACK");
        assert!(rx.file().is_none());
    }

    #[test]
    fn out_of_order_blocks_cancel() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        rx.receive(&header("a.bin", 1000));
        rx.receive(&block(1, 1));
        assert_eq!(rx.receive(&block(3, 1)), vec![CAN, CAN]);
        assert_eq!(rx.status(), ReceiveStatus::Failed(ReceiveError::Sequence { expected: 2, found: 3 }));
    }

    #[test]
    fn data_before_a_header_cancels() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        assert_eq!(rx.receive(&block(4, 1)), vec![CAN, CAN]);
    }

    #[test]
    fn oversized_or_unsized_files_cancel() {
        let config = ReceiverConfig { max_size: 1000, ..ReceiverConfig::default() };
        let mut rx = Receiver::new(config);
        assert_eq!(rx.receive(&header("a.bin", 1001)), vec![CAN, CAN]);
        assert_eq!(rx.status(), ReceiveStatus::Failed(ReceiveError::Size));

        let mut payload = vec![0; SHORT];
        payload[..5].copy_from_slice(b"a.bin");
        let mut rx = Receiver::new(ReceiverConfig::default());
        assert_eq!(rx.receive(&encode_block(0, &payload)), vec![CAN, CAN]);
    }

    #[test]
    fn more_data_than_announced_cancels() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        rx.receive(&header("a.bin", 100));
        rx.receive(&block(1, 1));
        assert_eq!(rx.receive(&block(2, 1)), vec![CAN, CAN]);
        assert_eq!(rx.status(), ReceiveStatus::Failed(ReceiveError::Overflow));
    }

    #[test]
    fn too_many_errors_cancel() {
        let config = ReceiverConfig { max_errors: 3, ..ReceiverConfig::default() };
        let mut rx = Receiver::new(config);
        rx.receive(&header("a.bin", 100));
        assert_eq!(rx.timeout(), vec![CRC_MODE]);
        assert_eq!(rx.timeout(), vec![CRC_MODE]);
        assert_eq!(rx.timeout(), vec![CAN, CAN]);
        assert_eq!(rx.status(), ReceiveStatus::Failed(ReceiveError::TooManyErrors));
    }

    #[test]
    fn header_polling_never_gives_up() {
        let mut rx = Receiver::new(ReceiverConfig { max_errors: 2, ..ReceiverConfig::default() });
        for _ in 0..100 {
            assert_eq!(rx.timeout(), vec![CRC_MODE]);
        }
        assert_eq!(rx.status(), ReceiveStatus::Idle);
    }

    #[test]
    fn two_cans_cancel() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        rx.receive(&header("a.bin", 100));
        assert_eq!(rx.receive(&[CAN, CAN]), []);
        assert_eq!(rx.status(), ReceiveStatus::Failed(ReceiveError::CancelledBySender));
    }

    #[test]
    fn a_second_file_cancels_and_a_missed_eot_ack_repeats() {
        let mut rx = Receiver::new(ReceiverConfig::default());
        rx.receive(&header("a.bin", 128));
        rx.receive(&block(1, 1));
        rx.receive(&[EOT, EOT]);
        assert_eq!(rx.receive(&[EOT]), vec![ACK, CRC_MODE], "the sender missed the ACK");
        assert_eq!(rx.receive(&header("b.bin", 1)), vec![CAN, CAN]);
        assert_eq!(rx.status(), ReceiveStatus::Failed(ReceiveError::SecondFile));
        assert!(rx.file().is_none(), "a failed batch hands out nothing");
    }

    #[test]
    fn an_unanswered_close_still_completes() {
        let mut rx = Receiver::new(ReceiverConfig { max_errors: 2, ..ReceiverConfig::default() });
        rx.receive(&header("a.bin", 1));
        rx.receive(&block(1, 1));
        rx.receive(&[EOT, EOT]);
        assert_eq!(rx.timeout(), vec![CRC_MODE]);
        assert_eq!(rx.timeout(), []);
        assert_eq!(rx.status(), ReceiveStatus::Done { closed: false });
        assert_eq!(rx.file().unwrap().data, vec![1]);
    }
}
