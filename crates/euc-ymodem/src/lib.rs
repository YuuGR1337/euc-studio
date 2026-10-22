//! Sans-IO YMODEM-1K with CRC-16.
//!
//! Two state machines and the packet format, with no I/O, no clock and no allocation beyond the
//! packets themselves. The caller owns the link and the timers:
//!
//! - feed every byte that arrives into [`Sender::receive`] (or [`Receiver::receive`]) and send
//!   what it returns;
//! - when the sender neither transmitted nor changed [`Sender::awaiting`] for the caller's
//!   response time, call [`Sender::timeout`] and send what it returns (bytes that move nothing,
//!   such as a receiver's `C` polls, must not restart that timer); when the line was quiet for the
//!   receiver's timeout, call [`Receiver::timeout`];
//! - stop when [`Sender::status`] is no longer [`SendStatus::Running`].
//!
//! The protocol is classic YMODEM batch mode as the `rb`/`sb` tools speak it: the receiver asks
//! for CRC mode with `C`, the sender answers with block 0 (file name and decimal size), then
//! 1024-byte data blocks (`STX`; a tail of at most 128 bytes goes in a 128-byte `SOH` block),
//! padded with `0x1A`, each acknowledged by `ACK` or refused by `NAK`. `EOT` ends the file (the
//! receiver refuses the first one, as `rb` does) and an empty block 0 ends the batch. Two `CAN`
//! bytes in a row cancel the transfer from either side.
//!
//! Only the CRC-16 variant exists here: the checksum variant cannot protect a firmware image.
//!
//! ```
//! use euc_ymodem::{Receiver, ReceiverConfig, SendStatus, Sender, SenderConfig};
//!
//! let image: Vec<u8> = (0..3000u32).map(|i| i as u8).collect();
//! let mut sender = Sender::new("app.bin", &image, SenderConfig::default())?;
//! let mut receiver = Receiver::new(ReceiverConfig::default());
//! let mut to_sender = receiver.start();
//! while sender.status() == SendStatus::Running {
//!     let mut to_receiver = Vec::new();
//!     for byte in std::mem::take(&mut to_sender) {
//!         to_receiver.extend(sender.receive(byte).unwrap_or_default());
//!     }
//!     to_sender = receiver.receive(&to_receiver);
//! }
//! assert_eq!(sender.status(), SendStatus::Done { closed: true });
//! assert_eq!(receiver.file().unwrap().data, image);
//! # Ok::<(), euc_ymodem::SetupError>(())
//! ```

mod packet;
mod receiver;
mod sender;

pub use packet::{
    ACK, Block, BlockError, CAN, CPMEOF, CRC_MODE, EOT, FileInfo, HeaderError, LONG, NAK, SHORT, SOH, STX, cancel,
    crc16, decode_block, encode_block, header_payload, packet_len, parse_header,
};
pub use receiver::{ReceiveError, ReceiveStatus, ReceivedFile, Receiver, ReceiverConfig};
pub use sender::{Awaiting, PacketId, Progress, SendError, SendStatus, Sender, SenderConfig, SetupError};
