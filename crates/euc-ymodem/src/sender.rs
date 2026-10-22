//! The sending side: one file in YMODEM batch mode.

use serde::Serialize;
use thiserror::Error;

use crate::packet::{
    ACK, CAN, CPMEOF, CRC_MODE, EOT, HeaderError, LONG, NAK, SHORT, cancel, encode_block, header_payload,
};

/// Sender limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SenderConfig {
    /// Transmissions of one packet, the first included, before the transfer fails (classic: 10).
    pub max_attempts: u8,
    /// Response timeouts while waiting for the receiver's first `C` before giving up.
    pub start_timeouts: u8,
    /// Send a tail of at most 128 bytes as a 128-byte `SOH` block instead of a padded 1 KiB one.
    pub short_tail: bool,
}

impl Default for SenderConfig {
    fn default() -> Self {
        Self { max_attempts: 10, start_timeouts: 6, short_tail: true }
    }
}

/// A file or configuration the sender cannot start with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SetupError {
    /// The name does not fit a header block.
    #[error("bad file name: {0}")]
    Name(HeaderError),
    /// `max_attempts` and `start_timeouts` must be at least 1.
    #[error("attempt limits must be at least 1")]
    Limits,
}

/// Which packet a failure concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "block", rename_all = "kebab-case")]
pub enum PacketId {
    /// Block 0 with the file name and size.
    Header,
    /// Data block, numbered from 1 (not wrapped at 256).
    Data(u32),
    /// The end-of-file byte.
    Eot,
    /// The empty block 0 that closes the batch.
    Close,
}

/// Why a transfer ended without delivering the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SendError {
    /// No `C` arrived within the start timeouts.
    #[error("the receiver never asked for the file")]
    NoReceiver,
    /// One packet was refused or unanswered too often.
    #[error("{packet:?} failed {attempts} times")]
    RetryLimit {
        /// The packet.
        packet: PacketId,
        /// Transmissions made.
        attempts: u8,
    },
    /// Two `CAN` bytes arrived.
    #[error("the receiver cancelled the transfer")]
    CancelledByReceiver,
    /// [`Sender::cancel`] was called.
    #[error("the sender cancelled the transfer")]
    CancelledBySender,
}

/// Where the sender stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SendStatus {
    /// Still exchanging packets.
    Running,
    /// The receiver acknowledged the end of file. `closed` is false when the batch-closing empty
    /// header got no answer; the file itself was delivered either way.
    Done {
        /// The receiver acknowledged the empty header too.
        closed: bool,
    },
    /// The transfer failed. [`Sender::delivered`] still tells whether the end of file was
    /// acknowledged before the failure.
    Failed(SendError),
}

/// What a running sender waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "packet", rename_all = "kebab-case")]
pub enum Awaiting {
    /// The receiver's first `C`.
    Start,
    /// The answer to this packet.
    Answer(PacketId),
    /// The `C` that asks for the first data block.
    DataStart,
    /// The `C` that asks for the next file header (the batch close follows).
    CloseStart,
    /// Nothing: the transfer ended.
    Nothing,
}

/// Counters for logs and progress bars.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Progress {
    /// Data blocks the receiver acknowledged.
    pub acked_blocks: u32,
    /// Data blocks in the file.
    pub total_blocks: u32,
    /// File bytes acknowledged (the padding of the last block not counted).
    pub acked_bytes: u64,
    /// File length.
    pub total_bytes: u64,
    /// Packets sent, retransmissions included.
    pub packets_sent: u32,
    /// Retransmissions after a `NAK` or a timeout.
    pub retransmissions: u32,
    /// `NAK` bytes that caused a retransmission.
    pub naks: u32,
    /// Response timeouts reported by the caller.
    pub timeouts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    AwaitStart,
    AwaitHeaderAck,
    AwaitDataStart,
    AwaitBlockAck(u32),
    AwaitEotAck,
    AwaitCloseStart,
    AwaitCloseAck,
    Done { closed: bool },
    Failed(SendError),
}

/// Sends one file. See the crate docs for the calling pattern.
#[derive(Debug, Clone)]
pub struct Sender {
    config: SenderConfig,
    header: Vec<u8>,
    data: Vec<u8>,
    /// Start offset of every data block; the last may be short.
    blocks: Vec<(usize, usize)>,
    state: State,
    /// Transmissions of the packet waited on, or timeouts while waiting for a `C`.
    attempts: u8,
    cans: u8,
    /// Timeouts while waiting for the `C` that confirms an acknowledged EOT.
    close_waits: u8,
    delivered: bool,
    progress: Progress,
}

impl Sender {
    /// A sender for `data` under `name`, waiting for the receiver's `C`.
    pub fn new(name: &str, data: &[u8], config: SenderConfig) -> Result<Self, SetupError> {
        if config.max_attempts == 0 || config.start_timeouts == 0 {
            return Err(SetupError::Limits);
        }
        let header = header_payload(Some((name, data.len() as u64))).map_err(SetupError::Name)?;
        let mut blocks = Vec::new();
        let mut offset = 0;
        while offset < data.len() {
            let left = data.len() - offset;
            let size = if config.short_tail && left <= SHORT { SHORT } else { LONG };
            blocks.push((offset, size));
            offset += size.min(left);
        }
        let progress =
            Progress { total_blocks: blocks.len() as u32, total_bytes: data.len() as u64, ..Progress::default() };
        Ok(Self {
            config,
            header: encode_block(0, &header),
            data: data.to_vec(),
            blocks,
            state: State::AwaitStart,
            attempts: 0,
            cans: 0,
            close_waits: 0,
            delivered: false,
            progress,
        })
    }

    /// Where the transfer stands.
    pub const fn status(&self) -> SendStatus {
        match self.state {
            State::Done { closed } => SendStatus::Done { closed },
            State::Failed(error) => SendStatus::Failed(error),
            _ => SendStatus::Running,
        }
    }

    /// True once the receiver acknowledged the end of file and then asked for the next file (or
    /// stayed silent for the close timeouts without refusing the EOT): every byte arrived and
    /// passed the receiver's checks, whatever happens to the batch close afterwards.
    ///
    /// Like every XMODEM-family sender this one cannot tell a late acknowledgement of an earlier
    /// packet from a fresh one. Callers drop receiver bytes, except `CAN`, that arrive before
    /// they finished transmitting the current packet: such bytes answer an earlier one.
    pub const fn delivered(&self) -> bool {
        self.delivered
    }

    /// Counters so far.
    pub const fn progress(&self) -> Progress {
        self.progress
    }

    /// True while the sender waits for the receiver's first `C` (callers usually wait longer
    /// there than for a packet's answer).
    pub const fn waiting_for_start(&self) -> bool {
        matches!(self.state, State::AwaitStart)
    }

    /// What the sender waits for. The caller's response timer measures time since this last
    /// changed or the sender last transmitted; bytes that change neither (a receiver polling
    /// with `C` while a packet is out, noise) must not restart it.
    pub const fn awaiting(&self) -> Awaiting {
        match self.state {
            State::AwaitStart => Awaiting::Start,
            State::AwaitHeaderAck => Awaiting::Answer(PacketId::Header),
            State::AwaitDataStart => Awaiting::DataStart,
            State::AwaitBlockAck(index) => Awaiting::Answer(PacketId::Data(index + 1)),
            State::AwaitEotAck => Awaiting::Answer(PacketId::Eot),
            State::AwaitCloseStart => Awaiting::CloseStart,
            State::AwaitCloseAck => Awaiting::Answer(PacketId::Close),
            State::Done { .. } | State::Failed(_) => Awaiting::Nothing,
        }
    }

    /// One byte from the receiver. Returns the bytes to transmit now, if any.
    pub fn receive(&mut self, byte: u8) -> Option<Vec<u8>> {
        if !matches!(self.status(), SendStatus::Running) {
            return None;
        }
        if byte == CAN {
            self.cans += 1;
            if self.cans >= 2 {
                self.state = State::Failed(SendError::CancelledByReceiver);
            }
            return None;
        }
        self.cans = 0;
        match (self.state, byte) {
            (State::AwaitStart, CRC_MODE) => Some(self.send_header()),
            (State::AwaitHeaderAck, ACK) => {
                self.state = State::AwaitDataStart;
                self.attempts = 0;
                None
            }
            // A NAK here means the receiver took the header but its `C` got lost.
            (State::AwaitDataStart, CRC_MODE | NAK) => Some(self.send_block_or_eot(0)),
            (State::AwaitBlockAck(index), ACK) => {
                let (offset, size) = self.blocks[index as usize];
                self.progress.acked_blocks += 1;
                self.progress.acked_bytes = (offset + size.min(self.data.len() - offset)) as u64;
                Some(self.send_block_or_eot(index + 1))
            }
            (State::AwaitHeaderAck | State::AwaitBlockAck(_) | State::AwaitEotAck | State::AwaitCloseAck, NAK) => {
                self.retransmit(true)
            }
            // Tentative: a late ACK of an earlier packet looks the same. The receiver's next
            // move settles it: `C` asks for the next file, NAK still refuses the EOT.
            (State::AwaitEotAck, ACK) => {
                self.state = State::AwaitCloseStart;
                self.close_waits = 0;
                None
            }
            (State::AwaitCloseStart, CRC_MODE) => {
                self.delivered = true;
                self.state = State::AwaitCloseAck;
                self.attempts = 1;
                Some(self.close_packet())
            }
            (State::AwaitCloseStart, NAK) => {
                self.state = State::AwaitEotAck;
                self.retransmit(true)
            }
            // The close packet got lost and the receiver polls again.
            (State::AwaitCloseAck, CRC_MODE) => self.retransmit(false),
            (State::AwaitCloseAck, ACK) => {
                self.state = State::Done { closed: true };
                None
            }
            // Anything else is noise, a late duplicate or a byte meant for another state.
            _ => None,
        }
    }

    /// The caller's response timer expired with no useful byte. Returns what to transmit.
    pub fn timeout(&mut self) -> Option<Vec<u8>> {
        if !matches!(self.status(), SendStatus::Running) {
            return None;
        }
        self.progress.timeouts += 1;
        match self.state {
            State::AwaitStart => {
                self.attempts += 1;
                if self.attempts >= self.config.start_timeouts {
                    self.state = State::Failed(SendError::NoReceiver);
                }
                None
            }
            // The receiver polls with `C` or NAK on its own; nothing to resend.
            State::AwaitDataStart => {
                self.attempts += 1;
                if self.attempts >= self.config.max_attempts {
                    let attempts = self.attempts;
                    self.state = State::Failed(SendError::RetryLimit { packet: PacketId::Data(1), attempts });
                }
                None
            }
            // The EOT was acknowledged and nothing contradicted it: the file is delivered.
            State::AwaitCloseStart => {
                self.close_waits += 1;
                if self.close_waits >= self.config.max_attempts {
                    self.delivered = true;
                    self.state = State::Done { closed: false };
                }
                None
            }
            State::AwaitCloseAck => {
                if self.attempts >= self.config.max_attempts {
                    self.state = State::Done { closed: false };
                    return None;
                }
                self.retransmit(false)
            }
            _ => self.retransmit(false),
        }
    }

    /// Cancels the transfer. Returns the two `CAN` bytes to send.
    pub fn cancel(&mut self) -> Vec<u8> {
        if matches!(self.status(), SendStatus::Running) {
            self.state = State::Failed(SendError::CancelledBySender);
        }
        cancel().to_vec()
    }

    fn send_header(&mut self) -> Vec<u8> {
        self.state = State::AwaitHeaderAck;
        self.attempts = 1;
        self.progress.packets_sent += 1;
        self.header.clone()
    }

    fn send_block_or_eot(&mut self, index: u32) -> Vec<u8> {
        self.attempts = 1;
        self.progress.packets_sent += 1;
        if index as usize >= self.blocks.len() {
            self.state = State::AwaitEotAck;
            return vec![EOT];
        }
        self.state = State::AwaitBlockAck(index);
        self.block(index)
    }

    fn block(&self, index: u32) -> Vec<u8> {
        let (offset, size) = self.blocks[index as usize];
        let end = (offset + size).min(self.data.len());
        let mut payload = self.data[offset..end].to_vec();
        payload.resize(size, CPMEOF);
        // Block numbers start at 1 and wrap at 256, as the protocol wants.
        encode_block((index + 1) as u8, &payload)
    }

    fn close_packet(&mut self) -> Vec<u8> {
        self.progress.packets_sent += 1;
        let payload = header_payload(None).unwrap_or_else(|_| vec![0; SHORT]);
        encode_block(0, &payload)
    }

    fn packet_id(&self) -> PacketId {
        match self.state {
            State::AwaitBlockAck(index) => PacketId::Data(index + 1),
            State::AwaitEotAck => PacketId::Eot,
            State::AwaitCloseAck | State::AwaitCloseStart => PacketId::Close,
            _ => PacketId::Header,
        }
    }

    /// Sends the packet waited on again, or fails once it used up its attempts.
    fn retransmit(&mut self, nak: bool) -> Option<Vec<u8>> {
        if nak {
            self.progress.naks += 1;
        }
        if self.attempts >= self.config.max_attempts {
            let packet = self.packet_id();
            self.state = State::Failed(SendError::RetryLimit { packet, attempts: self.attempts });
            return None;
        }
        self.attempts += 1;
        self.progress.packets_sent += 1;
        self.progress.retransmissions += 1;
        match self.state {
            State::AwaitHeaderAck => Some(self.header.clone()),
            State::AwaitBlockAck(index) => Some(self.block(index)),
            State::AwaitEotAck => Some(vec![EOT]),
            State::AwaitCloseAck => Some(self.close_packet()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{SOH, STX, decode_block, parse_header};

    fn data(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i * 31 + 7) as u8).collect()
    }

    /// Drives a sender with scripted receiver bytes; returns every transmission.
    fn run(sender: &mut Sender, script: &[u8]) -> Vec<Vec<u8>> {
        script.iter().filter_map(|&byte| sender.receive(byte)).collect()
    }

    #[test]
    fn happy_path_packets() {
        let file = data(2100);
        let mut sender = Sender::new("fw.bin", &file, SenderConfig::default()).unwrap();
        assert!(sender.waiting_for_start());
        let sent = run(&mut sender, &[CRC_MODE, ACK, CRC_MODE, ACK, ACK, ACK, NAK, ACK, CRC_MODE, ACK]);
        // Header, two 1 KiB blocks, a 128-byte tail, EOT twice (first refused), close.
        assert_eq!(sent.len(), 7);
        let header = decode_block(&sent[0]).unwrap();
        assert_eq!((header.seq, parse_header(&header.data).unwrap().size), (0, Some(2100)));
        assert_eq!((sent[1][0], sent[1][1]), (STX, 1));
        assert_eq!((sent[2][0], sent[2][1]), (STX, 2));
        let tail = decode_block(&sent[3]).unwrap();
        assert_eq!((sent[3][0], tail.seq), (SOH, 3));
        assert_eq!(&tail.data[..52], &file[2048..]);
        assert!(tail.data[52..].iter().all(|&b| b == CPMEOF));
        assert_eq!((sent[4].as_slice(), sent[5].as_slice()), ([EOT].as_slice(), [EOT].as_slice()));
        assert_eq!(decode_block(&sent[6]).unwrap().data, vec![0; SHORT]);
        assert_eq!(sender.status(), SendStatus::Done { closed: true });
        assert!(sender.delivered());
        let progress = sender.progress();
        assert_eq!((progress.acked_blocks, progress.acked_bytes, progress.naks), (3, 2100, 1));
    }

    #[test]
    fn long_tail_without_short_blocks() {
        let config = SenderConfig { short_tail: false, ..SenderConfig::default() };
        let mut sender = Sender::new("fw.bin", &data(1030), config).unwrap();
        let sent = run(&mut sender, &[CRC_MODE, ACK, CRC_MODE, ACK]);
        assert_eq!(sent[2][0], STX, "the 6-byte tail still goes in a 1 KiB block");
        assert_eq!(sender.progress().total_blocks, 2);
    }

    #[test]
    fn empty_file_sends_eot_right_after_the_header() {
        let mut sender = Sender::new("empty", &[], SenderConfig::default()).unwrap();
        let sent = run(&mut sender, &[CRC_MODE, ACK, CRC_MODE]);
        assert_eq!(sent.last().unwrap(), &vec![EOT]);
    }

    #[test]
    fn nak_and_timeout_retransmit_the_same_block() {
        let mut sender = Sender::new("fw.bin", &data(3000), SenderConfig::default()).unwrap();
        let sent = run(&mut sender, &[CRC_MODE, ACK, CRC_MODE, ACK]);
        let block2 = sent.last().unwrap().clone();
        assert_eq!(sender.receive(NAK), Some(block2.clone()));
        assert_eq!(sender.timeout(), Some(block2.clone()));
        let progress = sender.progress();
        assert_eq!((progress.retransmissions, progress.naks, progress.timeouts), (2, 1, 1));
        // Noise and late duplicates change nothing.
        assert_eq!(sender.receive(b'x'), None);
        assert_eq!(sender.receive(CRC_MODE), None);
        assert_eq!(sender.status(), SendStatus::Running);
        assert_eq!(sender.receive(ACK).unwrap()[1], 3);
    }

    #[test]
    fn retry_limit_fails_the_transfer() {
        let config = SenderConfig { max_attempts: 3, ..SenderConfig::default() };
        let mut sender = Sender::new("fw.bin", &data(100), config).unwrap();
        run(&mut sender, &[CRC_MODE, ACK, CRC_MODE]);
        assert!(sender.receive(NAK).is_some());
        assert!(sender.timeout().is_some());
        assert_eq!(sender.receive(NAK), None);
        assert_eq!(
            sender.status(),
            SendStatus::Failed(SendError::RetryLimit { packet: PacketId::Data(1), attempts: 3 })
        );
        assert!(!sender.delivered());
        assert_eq!(sender.receive(ACK), None, "a failed sender stays failed");
    }

    #[test]
    fn header_retries_count_too() {
        let config = SenderConfig { max_attempts: 2, ..SenderConfig::default() };
        let mut sender = Sender::new("fw.bin", &data(100), config).unwrap();
        assert!(sender.receive(CRC_MODE).is_some());
        assert!(sender.timeout().is_some());
        assert_eq!(sender.timeout(), None);
        assert_eq!(
            sender.status(),
            SendStatus::Failed(SendError::RetryLimit { packet: PacketId::Header, attempts: 2 })
        );
    }

    #[test]
    fn two_cans_cancel_one_does_not() {
        let mut sender = Sender::new("fw.bin", &data(100), SenderConfig::default()).unwrap();
        run(&mut sender, &[CRC_MODE, ACK, CRC_MODE]);
        assert_eq!(sender.receive(CAN), None);
        assert_eq!(sender.receive(NAK).map(|p| p[1]), Some(1), "a lone CAN is noise");
        sender.receive(CAN);
        sender.receive(CAN);
        assert_eq!(sender.status(), SendStatus::Failed(SendError::CancelledByReceiver));
    }

    #[test]
    fn sender_cancel_sends_two_cans() {
        let mut sender = Sender::new("fw.bin", &data(100), SenderConfig::default()).unwrap();
        assert_eq!(sender.cancel(), vec![CAN, CAN]);
        assert_eq!(sender.status(), SendStatus::Failed(SendError::CancelledBySender));
    }

    #[test]
    fn no_receiver_after_the_start_timeouts() {
        let config = SenderConfig { start_timeouts: 2, ..SenderConfig::default() };
        let mut sender = Sender::new("fw.bin", &data(10), config).unwrap();
        assert_eq!(sender.timeout(), None);
        assert_eq!(sender.status(), SendStatus::Running);
        assert_eq!(sender.timeout(), None);
        assert_eq!(sender.status(), SendStatus::Failed(SendError::NoReceiver));
    }

    #[test]
    fn nak_after_the_header_starts_the_data() {
        let mut sender = Sender::new("fw.bin", &data(10), SenderConfig::default()).unwrap();
        run(&mut sender, &[CRC_MODE, ACK]);
        assert_eq!(sender.receive(NAK).map(|p| p[1]), Some(1));
    }

    #[test]
    fn unanswered_close_still_delivers() {
        let config = SenderConfig { max_attempts: 2, ..SenderConfig::default() };
        let mut sender = Sender::new("fw.bin", &data(10), config).unwrap();
        run(&mut sender, &[CRC_MODE, ACK, CRC_MODE, ACK, ACK]);
        assert!(!sender.delivered(), "an ACK of the EOT alone is tentative");
        assert_eq!(sender.status(), SendStatus::Running);
        sender.timeout();
        sender.timeout();
        assert_eq!(sender.status(), SendStatus::Done { closed: false });
        assert!(sender.delivered(), "nothing contradicted the ACK");

        let mut sender = Sender::new("fw.bin", &data(10), config).unwrap();
        run(&mut sender, &[CRC_MODE, ACK, CRC_MODE, ACK, ACK, CRC_MODE]);
        assert!(sender.timeout().is_some(), "the close packet is sent again once");
        assert_eq!(sender.timeout(), None);
        assert_eq!(sender.status(), SendStatus::Done { closed: false });
    }

    #[test]
    fn a_late_ack_cannot_end_the_file() {
        let mut sender = Sender::new("fw.bin", &data(10), SenderConfig::default()).unwrap();
        // Block 1 is sent twice; its second ACK arrives after the EOT went out.
        let sent = run(&mut sender, &[CRC_MODE, ACK, CRC_MODE, ACK, ACK]);
        assert_eq!(sent.last().unwrap(), &vec![EOT]);
        // The receiver refuses the EOT: the ACK was stale, so the EOT goes out again.
        assert_eq!(sender.receive(NAK), Some(vec![EOT]));
        assert!(!sender.delivered());
        assert_eq!(sender.receive(ACK), None);
        assert_eq!(decode_block(&sender.receive(CRC_MODE).unwrap()).unwrap().data, vec![0; SHORT]);
        assert!(sender.delivered());
        assert_eq!(sender.receive(CRC_MODE).map(|p| p[1]), Some(0), "a lost close packet goes out again");
    }

    #[test]
    fn block_numbers_wrap_at_256() {
        let mut sender = Sender::new("big", &data(LONG * 300), SenderConfig::default()).unwrap();
        let mut last = run(&mut sender, &[CRC_MODE, ACK, CRC_MODE]);
        for _ in 0..255 {
            last.push(sender.receive(ACK).unwrap());
        }
        assert_eq!(last.last().unwrap()[1], 0, "block 256 carries sequence 0");
        assert_eq!(sender.receive(ACK).unwrap()[1], 1);
    }

    #[test]
    fn bad_setup_is_refused() {
        assert_eq!(Sender::new("", &[], SenderConfig::default()).unwrap_err(), SetupError::Name(HeaderError::Empty));
        let config = SenderConfig { max_attempts: 0, ..SenderConfig::default() };
        assert_eq!(Sender::new("a", &[], config).unwrap_err(), SetupError::Limits);
    }
}
