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
- `euc image seal` fills only `image_len` and `crc32` for unsigned development BIN images.
- Sealing preserves the development flag, metadata, zero signature, and all bytes outside those two fields.
- `euc image validate` accepts an unsealed image (`image_len == 0`) as a dev image and says so; a sealed image must have
  a matching length and CRC.

## Unsigned development sealing

```sh
euc image seal <input> --out <output> [--wheel <wheel-id>]
euc --json image seal <input> --out <output> [--wheel <wheel-id>]
```

The command selects the wheel as `euc image validate` does. With one profile, `--wheel` is optional.
With multiple profiles, the command requires `--wheel`. The global `--profiles` option selects another profile folder.

The input must pass the selected wheel's image gate before sealing. It must contain our valid layout-1 `.euc_info` block.
The input path must point to a regular file.
The CLI rejects files larger than the selected application slot before loading their contents.
It reads at most `max_size + 1` bytes and rejects growth beyond the slot limit.
The development flag must be set. Every signature byte must be zero.
The command refuses ELF inputs, pinned stock images, signed images, and images without the development flag.
It does not convert ELF files or add an info block.

Sealing writes the complete BIN length into `image_len` first. It then computes CRC-32/ISO-HDLC over the complete image.
For this calculation, the `crc32` and `signature` fields count as zero. The calculation includes the new `image_len`.
Sealing writes the CRC and validates the complete result against the same wheel.
It changes no other bytes and adds no padding.

An existing seal must pass its length and CRC checks before sealing. Sealing that image again produces the same bytes.
The command refuses a damaged seal instead of replacing it.

The output must name a new file. Existing files, directories, and symlinks are refused, including aliases of the input.
The command writes and syncs an exclusively created temporary file in the output directory.
It then creates the output through an atomic, exclusive hard link and removes the temporary link.
If the filesystem does not support hard links, the command fails without replacing an existing path.
An interrupted process can leave a temporary file. The command never reuses an existing temporary file.

Successful JSON output includes `input`, output `file`, the output gate report, and `profiles_source`.
A sealing refusal includes `input`, `out`, `error.kind`, and `profiles_source`.
Gate refusals also include the full report in `error.report`. File errors use the CLI's existing error output.

The pure Rust API is `euc_image::seal(image: &Image, wheel: &WheelProfile) -> Result<Image, SealError>`.
It returns a new image and leaves the input unchanged. It performs no file or BLE operations.
`SealError` reports format, input gate, stock pin, missing info, signature, development flag, or output gate failures.
The gate error variants carry a boxed `Report`.

Sealing provides length and CRC checks only. It does not prove cryptographic authenticity or verify signatures.
This command creates no keys and implements no signing operation. It does not authorize hardware use.

## Image gate (what `euc image validate` checks for the Begode Extreme)

1. Size ≤ 49,152 bytes (`0x08003000`-`0x0800EFFF`), so the image never reaches the storage pages at `0x0800F000`,
   `0x0800F800`, `0x08010000` and `0x08010800`.
2. Word 0 (initial SP) is 8-byte aligned and inside `0x20000000`-`0x2000C000`.
3. Word 1 (reset) and every non-zero vector up to entry 75 are odd (Thumb) and point inside the image.
4. If the image has an `.euc_info` block: magic, layout, size and board match; if sealed, length and CRC match.
5. If it has no block, its sha256 must be a pinned stock build in the wheel profile; anything else is refused.
