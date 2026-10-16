//! Flattens the loadable segments of a firmware ELF into flash bytes.

use object::Endianness;
use object::elf::{ELFCLASS32, ELFDATA2LSB, ELFMAG, EM_ARM, FileHeader32, PT_LOAD};
use object::read::elf::{FileHeader, ProgramHeader};

use crate::ImageError;

pub(crate) const MAGIC: &[u8] = &ELFMAG;

/// Widest span the segments may cover. Larger than the flash of any part we target, small enough
/// that a segment linked into RAM by mistake is reported instead of allocating gigabytes.
const MAX_SPAN: u64 = 16 << 20;

/// Returns the lowest load address and the bytes from there up.
///
/// Segments are placed at their physical (load) address, not their virtual one: `.data` runs from
/// RAM but is stored in flash, and flash is what an image describes. Gaps read as erased flash
/// (`0xFF`).
pub(crate) fn flatten(data: &[u8]) -> Result<(u32, Vec<u8>), ImageError> {
    if data.get(4) != Some(&ELFCLASS32.0) || data.get(5) != Some(&ELFDATA2LSB.0) {
        return Err(ImageError::NotArm);
    }
    let elf_error = |e: object::Error| ImageError::Elf(e.to_string());
    let header = FileHeader32::<Endianness>::parse(data).map_err(elf_error)?;
    let endian = header.endian().map_err(elf_error)?;
    if header.e_machine(endian) != EM_ARM {
        return Err(ImageError::NotArm);
    }

    let mut segments = Vec::new();
    for ph in header.program_headers(endian, data).map_err(elf_error)? {
        if ph.p_type(endian) != PT_LOAD || ph.p_filesz(endian) == 0 {
            continue;
        }
        let bytes = ph
            .data(endian, data)
            .map_err(|()| ImageError::Elf("a segment points past the end of the file".to_owned()))?;
        segments.push((ph.p_paddr(endian), bytes));
    }

    let start = segments.iter().map(|&(addr, _)| addr).min().ok_or(ImageError::Empty)?;
    let end = segments.iter().map(|&(addr, bytes)| u64::from(addr) + bytes.len() as u64).max().unwrap_or_default();
    if end - u64::from(start) > MAX_SPAN {
        return Err(ImageError::Sparse { start, end });
    }
    let mut image = vec![0xFF; (end - u64::from(start)) as usize];
    for (addr, bytes) in segments {
        let at = (addr - start) as usize;
        image[at..at + bytes.len()].copy_from_slice(bytes);
    }
    Ok((start, image))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal ELF32 little-endian ARM executable with one program header per segment.
    /// Each segment is `(p_type, p_paddr, bytes)`; `p_vaddr` is set to RAM to prove it is ignored.
    pub(crate) fn build(machine: u16, segments: &[(u32, u32, &[u8])]) -> Vec<u8> {
        const EHDR: u32 = 52;
        const PHDR: u32 = 32;
        let count = segments.len() as u32;
        let mut out = Vec::new();
        out.extend_from_slice(&[0x7F, b'E', b'L', b'F', 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&2_u16.to_le_bytes()); // ET_EXEC
        out.extend_from_slice(&machine.to_le_bytes());
        for word in [1_u32, 0x0800_3000, EHDR, 0, 0x0500_0200] {
            out.extend_from_slice(&word.to_le_bytes()); // version, entry, phoff, shoff, flags
        }
        for half in [EHDR as u16, PHDR as u16, count as u16, 40, 0, 0] {
            out.extend_from_slice(&half.to_le_bytes()); // ehsize, phentsize, phnum, shentsize, shnum, shstrndx
        }
        let mut offset = EHDR + PHDR * count;
        for &(p_type, paddr, bytes) in segments {
            let len = bytes.len() as u32;
            for word in [p_type, offset, 0x2000_0000, paddr, len, len, 5, 4] {
                out.extend_from_slice(&word.to_le_bytes());
            }
            offset += len;
        }
        for &(_, _, bytes) in segments {
            out.extend_from_slice(bytes);
        }
        out
    }

    const ARM: u16 = 40;
    const LOAD: u32 = 1;
    const NOTE: u32 = 4;

    #[test]
    fn places_segments_at_their_load_address() {
        let elf = build(ARM, &[(LOAD, 0x0800_3010, &[3, 4]), (NOTE, 0x0, &[9; 8]), (LOAD, 0x0800_3000, &[1, 2])]);
        let (start, bytes) = flatten(&elf).unwrap();
        assert_eq!(start, 0x0800_3000);
        let mut expected = vec![0xFF; 0x12];
        expected[..2].copy_from_slice(&[1, 2]);
        expected[0x10..].copy_from_slice(&[3, 4]);
        assert_eq!(bytes, expected);
    }

    #[test]
    fn rejects_other_machines_and_classes() {
        assert_eq!(flatten(&build(62, &[(LOAD, 0, &[1])])), Err(ImageError::NotArm));
        let mut elf64 = build(ARM, &[(LOAD, 0, &[1])]);
        elf64[4] = 2;
        assert_eq!(flatten(&elf64), Err(ImageError::NotArm));
    }

    #[test]
    fn rejects_empty_and_sparse_images() {
        assert_eq!(flatten(&build(ARM, &[(NOTE, 0, &[1])])), Err(ImageError::Empty));
        let sparse = build(ARM, &[(LOAD, 0x0800_3000, &[1]), (LOAD, 0x2000_0000, &[2])]);
        assert!(matches!(flatten(&sparse), Err(ImageError::Sparse { start: 0x0800_3000, .. })));
    }

    #[test]
    fn rejects_truncated_files() {
        let mut elf = build(ARM, &[(LOAD, 0x0800_3000, &[1; 16])]);
        elf.truncate(elf.len() - 4);
        assert!(matches!(flatten(&elf), Err(ImageError::Elf(_))));
        assert!(matches!(flatten(&elf[..30]), Err(ImageError::Elf(_))));
    }
}
