# `.euc_info` image block (layout 1)

Every firmware image built from this repo carries a 128-byte info block. The OTA flasher, `euc image validate` and the
studio read it to identify an image before it goes anywhere near a wheel. Stock vendor images do not have this block;
they are identified by their pinned sha256 instead.

## Placement

- Image offset `0x140` from the start of the image. For the Begode Extreme the image links at `0x08003000`, so the block
  sits at `0x08003140`, right after the 76-entry STM32F1 high-density vector table (`0x130` bytes) and 16 bytes of padding.
- Linker section `.euc_info`, `KEEP`, 4-byte aligned.

## Layout (little-endian)

| Offset | Size | Field | Value |
|---|---|---|---|
| 0x00 | 4 | `magic` | `0x46435545` (bytes `45 55 43 46`, ASCII "EUCF") |
| 0x04 | 2 | `layout` | `1` |
| 0x06 | 2 | `size` | `128` |
| 0x08 | 16 | `board` | board id, ASCII, NUL padded, e.g. `begode-extreme` |
| 0x18 | 8 | `profile` | `compat` or `clean`, ASCII, NUL padded |
| 0x20 | 2 | `ver_major` | semver major |
| 0x22 | 2 | `ver_minor` | semver minor |
| 0x24 | 2 | `ver_patch` | semver patch |
| 0x26 | 2 | `flags` | bit 0 = dev build (unsigned or dev key); other bits 0 |
| 0x28 | 12 | `git_sha` | short git sha, ASCII, NUL padded |
| 0x34 | 4 | `image_len` | length of the whole image in bytes |
| 0x38 | 4 | `crc32` | CRC-32/ISO-HDLC of the whole image with `crc32` and `signature` set to zero |
| 0x3C | 64 | `signature` | Ed25519 signature of the same bytes; all zero = unsigned |
| 0x7C | 4 | `reserved` | `0` |

## Rules

- The firmware build writes every field except `image_len`, `crc32` and `signature`, which stay zero.
- `euc image seal` (a later milestone) fills `image_len`, `crc32` and optionally `signature`.
- `euc image validate` accepts an unsealed image (`image_len == 0`) as a dev image and says so; a sealed image must have
  a matching length and CRC.

## Image gate (what `euc image validate` checks for the Begode Extreme)

1. Size ≤ 49,152 bytes (`0x08003000`-`0x0800EFFF`), so the image never reaches the storage pages at `0x0800F000`,
   `0x0800F800`, `0x08010000` and `0x08010800`.
2. Word 0 (initial SP) is 8-byte aligned and inside `0x20000000`-`0x2000C000`.
3. Word 1 (reset) and every non-zero vector up to entry 75 are odd (Thumb) and point inside the image.
4. If the image has an `.euc_info` block: magic, layout, size and board match; if sealed, length and CRC match.
5. If it has no block, its sha256 must be a pinned stock build in the wheel profile; anything else is refused.
