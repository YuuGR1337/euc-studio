//! Control bytes, CRC-16 and the block format.

use serde::Serialize;
use thiserror::Error;

/// Start of a 128-byte block.
pub const SOH: u8 = 0x01;
/// Start of a 1024-byte block.
pub const STX: u8 = 0x02;
/// End of file.
pub const EOT: u8 = 0x04;
/// Block received.
pub const ACK: u8 = 0x06;
/// Block refused; send it again.
pub const NAK: u8 = 0x15;
/// Cancel; two in a row end the transfer.
pub const CAN: u8 = 0x18;
/// The receiver's request for a CRC-16 transfer (`C`); it also asks for the next file header.
pub const CRC_MODE: u8 = b'C';
/// Padding after the last data byte (CP/M end of file).
pub const CPMEOF: u8 = 0x1A;
/// Payload of an `SOH` block.
pub const SHORT: usize = 128;
/// Payload of an `STX` block.
pub const LONG: usize = 1024;

/// Bytes around the payload: start, sequence, its complement, two CRC bytes.
const OVERHEAD: usize = 5;

/// CRC-16/XMODEM (polynomial 0x1021, initial value 0, no reflection), as YMODEM sends it.
///
/// ```
/// assert_eq!(euc_ymodem::crc16(b"123456789"), 0x31C3);
/// ```
pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0_u16;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 { crc << 1 } else { (crc << 1) ^ 0x1021 };
        }
    }
    crc
}

/// Total length of the packet that starts with `start`, or `None` if `start` begins no block.
pub const fn packet_len(start: u8) -> Option<usize> {
    match start {
        SOH => Some(SHORT + OVERHEAD),
        STX => Some(LONG + OVERHEAD),
        _ => None,
    }
}

/// Encodes one block. `payload` must be exactly [`SHORT`] or [`LONG`] bytes.
///
/// # Panics
///
/// On any other payload length: the state machines only build full blocks.
pub fn encode_block(seq: u8, payload: &[u8]) -> Vec<u8> {
    let start = match payload.len() {
        SHORT => SOH,
        LONG => STX,
        other => panic!("a YMODEM payload is 128 or 1024 bytes, not {other}"),
    };
    let mut packet = Vec::with_capacity(payload.len() + OVERHEAD);
    packet.extend_from_slice(&[start, seq, !seq]);
    packet.extend_from_slice(payload);
    packet.extend_from_slice(&crc16(payload).to_be_bytes());
    packet
}

/// One block as the receiver read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// Sequence number, modulo 256; 0 for file headers.
    pub seq: u8,
    /// Payload, 128 or 1024 bytes.
    pub data: Vec<u8>,
}

/// A packet that does not decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BlockError {
    /// The first byte is neither `SOH` nor `STX`.
    #[error("0x{0:02X} starts no block")]
    Start(u8),
    /// The packet is shorter or longer than its start byte says.
    #[error("{found} bytes; this block needs {expected}")]
    Length {
        /// Bytes given.
        found: usize,
        /// Bytes the start byte asks for.
        expected: usize,
    },
    /// The sequence complement does not match.
    #[error("sequence {seq} has complement 0x{complement:02X}")]
    Sequence {
        /// Sequence byte.
        seq: u8,
        /// Complement byte.
        complement: u8,
    },
    /// The payload does not match its CRC.
    #[error("CRC 0x{stored:04X} does not match the payload's 0x{computed:04X}")]
    Crc {
        /// CRC in the packet.
        stored: u16,
        /// CRC of the payload.
        computed: u16,
    },
}

/// Decodes one complete block packet.
pub fn decode_block(packet: &[u8]) -> Result<Block, BlockError> {
    let start = *packet.first().ok_or(BlockError::Length { found: 0, expected: SHORT + OVERHEAD })?;
    let expected = packet_len(start).ok_or(BlockError::Start(start))?;
    if packet.len() != expected {
        return Err(BlockError::Length { found: packet.len(), expected });
    }
    let (seq, complement) = (packet[1], packet[2]);
    if complement != !seq {
        return Err(BlockError::Sequence { seq, complement });
    }
    let data = &packet[3..expected - 2];
    let stored = u16::from_be_bytes([packet[expected - 2], packet[expected - 1]]);
    let computed = crc16(data);
    if stored != computed {
        return Err(BlockError::Crc { stored, computed });
    }
    Ok(Block { seq, data: data.to_vec() })
}

/// The bytes a side sends to cancel: two `CAN`.
pub const fn cancel() -> [u8; 2] {
    [CAN, CAN]
}

/// What a file header (block 0) carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileInfo {
    /// File name, printable ASCII.
    pub name: String,
    /// File length in bytes, when the header gives one.
    pub size: Option<u64>,
}

/// A file name the header cannot carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HeaderError {
    /// Empty names end a batch; a file needs a name.
    #[error("the file name is empty")]
    Empty,
    /// Only printable ASCII without `/` and spaces.
    #[error("the file name must be printable ASCII without spaces or '/'")]
    Characters,
    /// Name and size together must fit a 1024-byte block.
    #[error("the file name is too long for a header block")]
    TooLong,
}

/// Block 0 payload for a file: name, NUL, size in decimal, NUL padding; 128 bytes when that fits,
/// else 1024. `None` gives the empty header that ends a batch.
pub fn header_payload(file: Option<(&str, u64)>) -> Result<Vec<u8>, HeaderError> {
    let Some((name, size)) = file else { return Ok(vec![0; SHORT]) };
    if name.is_empty() {
        return Err(HeaderError::Empty);
    }
    if !name.bytes().all(|byte| byte.is_ascii_graphic() && byte != b'/') {
        return Err(HeaderError::Characters);
    }
    let mut payload = name.as_bytes().to_vec();
    payload.push(0);
    payload.extend_from_slice(size.to_string().as_bytes());
    // At least one NUL must end the size field.
    let len = match payload.len() {
        n if n < SHORT => SHORT,
        n if n < LONG => LONG,
        _ => return Err(HeaderError::TooLong),
    };
    payload.resize(len, 0);
    Ok(payload)
}

/// Reads a block 0 payload. `None` for the empty header that ends a batch, or a payload that
/// holds no readable name.
pub fn parse_header(payload: &[u8]) -> Option<FileInfo> {
    let end = payload.iter().position(|&byte| byte == 0)?;
    let name = &payload[..end];
    if name.is_empty() || !name.iter().all(u8::is_ascii_graphic) {
        return None;
    }
    let rest = &payload[end + 1..];
    let digits: Vec<u8> = rest.iter().copied().take_while(u8::is_ascii_digit).collect();
    let size = std::str::from_utf8(&digits).ok().and_then(|text| text.parse().ok());
    Some(FileInfo { name: String::from_utf8_lossy(name).into_owned(), size })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_the_xmodem_check_value() {
        assert_eq!(crc16(b"123456789"), 0x31C3);
        assert_eq!(crc16(&[]), 0);
        assert_eq!(crc16(&[0; 128]), 0);
    }

    #[test]
    fn blocks_round_trip() {
        let payload: Vec<u8> = (0..LONG).map(|i| (i * 7) as u8).collect();
        let packet = encode_block(5, &payload);
        assert_eq!(packet.len(), 1029);
        assert_eq!(&packet[..3], &[STX, 5, 0xFA]);
        assert_eq!(decode_block(&packet), Ok(Block { seq: 5, data: payload }));

        let short = encode_block(0, &[0x1A; SHORT]);
        assert_eq!((short[0], short.len()), (SOH, 133));
        assert_eq!(packet_len(SOH), Some(133));
        assert_eq!(packet_len(EOT), None);
    }

    #[test]
    fn every_single_byte_error_is_caught() {
        let payload: Vec<u8> = (0..SHORT).map(|i| i as u8).collect();
        let packet = encode_block(1, &payload);
        for at in 0..packet.len() {
            for flip in [0x01, 0x80, 0xFF] {
                let mut bad = packet.clone();
                bad[at] ^= flip;
                assert!(decode_block(&bad).is_err(), "byte {at} ^ {flip:#04X} slipped through");
            }
        }
        assert!(matches!(decode_block(&packet[..100]), Err(BlockError::Length { found: 100, expected: 133 })));
        assert_eq!(decode_block(&[]), Err(BlockError::Length { found: 0, expected: 133 }));
    }

    #[test]
    #[should_panic(expected = "128 or 1024")]
    fn odd_payloads_are_a_bug() {
        let _ = encode_block(1, &[0; 100]);
    }

    #[test]
    fn headers_carry_name_and_size() {
        let header = header_payload(Some(("app.bin", 49_152))).unwrap();
        assert_eq!(header.len(), SHORT);
        assert_eq!(&header[..14], b"app.bin\x0049152\x00");
        assert_eq!(parse_header(&header), Some(FileInfo { name: "app.bin".into(), size: Some(49_152) }));

        let end = header_payload(None).unwrap();
        assert_eq!(end, vec![0; SHORT]);
        assert_eq!(parse_header(&end), None);

        let long = "n".repeat(200);
        assert_eq!(header_payload(Some((&long, 1))).unwrap().len(), LONG);
        assert_eq!(header_payload(Some((&"n".repeat(1100), 1))), Err(HeaderError::TooLong));
        assert_eq!(header_payload(Some(("", 1))), Err(HeaderError::Empty));
        assert_eq!(header_payload(Some(("a b", 1))), Err(HeaderError::Characters));
        assert_eq!(header_payload(Some(("dir/a", 1))), Err(HeaderError::Characters));
    }

    #[test]
    fn headers_without_a_size_still_parse() {
        let mut payload = vec![0; SHORT];
        payload[..5].copy_from_slice(b"a.bin");
        assert_eq!(parse_header(&payload), Some(FileInfo { name: "a.bin".into(), size: None }));
        payload[6..10].copy_from_slice(b"12 3");
        assert_eq!(parse_header(&payload).unwrap().size, Some(12));
    }
}
