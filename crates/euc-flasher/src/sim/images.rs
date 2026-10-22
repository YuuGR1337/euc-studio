//! Synthetic images and the simulator's wheel profile, built at run time. No vendor bytes: a
//! "stock" image here is a made-up application whose sha256 the simulated profile pins.

use std::fmt::Write as _;

use euc_flash_sim::Geometry;
use euc_image::{Image, seal};
use euc_profile::{Sha256, WheelProfile};

/// Where every Begode Extreme-style app loads.
pub const APP_BASE: u32 = 0x0800_3000;
/// Offset of the `.euc_info` block (docs/spec/euc-info.md).
pub const INFO_OFFSET: usize = 0x140;
/// The board id our images carry.
pub const BOARD: &str = "begode-extreme";

/// The simulated flash: 256 KiB from `0x0800_0000` in 2 KiB pages (a synthetic geometry that fits
/// the Extreme's app slot and settings pages; not a measured vendor layout).
pub const fn geometry() -> Geometry {
    Geometry { flash_base: 0x0800_0000, flash_size: 0x0004_0000, page_size: 0x800 }
}

/// A plausible application of `len` bytes: a 76-entry vector table (initial SP in RAM, Thumb
/// handlers inside the image) and seeded filler. `len` must hold the vector table.
pub fn synthetic_app(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed ^ 0x005E_ED0F_F1A5;
    let mut image: Vec<u8> = (0..len)
        .map(|_| {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as u8
        })
        .collect();
    let mut vectors = [0_u32; 76];
    vectors[0] = 0x2000_2000;
    vectors[1] = APP_BASE + 0x201;
    vectors[2] = APP_BASE + 0x205;
    vectors[3] = APP_BASE + 0x205;
    vectors[15] = APP_BASE + 0x209;
    for (index, vector) in vectors.iter().enumerate() {
        image[4 * index..4 * index + 4].copy_from_slice(&vector.to_le_bytes());
    }
    // Clear the block slot so a stock image never carries an info block by accident.
    image[INFO_OFFSET..INFO_OFFSET + 4].fill(0);
    image
}

/// One of our builds: a synthetic app with a layout-1 `.euc_info` block for `wheel`'s board,
/// development flag set, sealed with `euc_image::seal`.
///
/// # Panics
///
/// If `len` cannot hold the vector table and the block, or sealing fails (a bug).
pub fn our_image(wheel: &WheelProfile, len: usize, seed: u64, profile: &str) -> Image {
    let mut bytes = synthetic_app(len, seed);
    let block = &mut bytes[INFO_OFFSET..INFO_OFFSET + 128];
    block.fill(0);
    block[..4].copy_from_slice(b"EUCF");
    block[4..6].copy_from_slice(&1_u16.to_le_bytes());
    block[6..8].copy_from_slice(&128_u16.to_le_bytes());
    let board = wheel.board.id.as_bytes();
    block[8..8 + board.len()].copy_from_slice(board);
    block[0x18..0x18 + profile.len()].copy_from_slice(profile.as_bytes());
    for (i, part) in [0_u16, 6, u16::try_from(seed % 100).unwrap_or(0)].into_iter().enumerate() {
        block[0x20 + 2 * i..0x22 + 2 * i].copy_from_slice(&part.to_le_bytes());
    }
    block[0x26..0x28].copy_from_slice(&1_u16.to_le_bytes());
    block[0x28..0x2F].copy_from_slice(b"0000sim");
    seal(&Image::raw(bytes), wheel).expect("a synthetic image seals")
}

/// The simulator's wheel profile: the Begode Extreme's board layout, brand "Sim", model
/// `EXTREME`, with `stock` images pinned as its official builds.
///
/// # Panics
///
/// Never for valid pins (the profile text is fixed).
pub fn sim_profile(stock: &[&[u8]]) -> WheelProfile {
    let mut text = String::from(
        r#"
schema = 1
id = "sim-extreme"
name = "Sim Extreme"
brand = "Sim"

[board]
id = "begode-extreme"
mcu = "STM32F103RC-class"
core = "Cortex-M3"
hclk_hz = 72_000_000
app = { base = 0x0800_3000, max_size = 49_152, vectors = 76, info_offset = 0x140, storage_pages = [0x0800_F000, 0x0800_F800, 0x0801_0000, 0x0801_0800] }
ram = { base = 0x2000_0000, size = 0xC000 }
"#,
    );
    for (index, image) in stock.iter().enumerate() {
        let _ = write!(
            text,
            "\n[[firmware]]\nkind = \"manual\"\nsha256 = \"{}\"\nlabel = \"SIM-STOCK-{}\"\n",
            Sha256::digest(image),
            index + 1
        );
    }
    WheelProfile::parse(&text).expect("the simulator profile parses")
}

#[cfg(test)]
mod tests {
    use euc_image::{Identity, validate};

    use super::*;

    #[test]
    fn synthetic_images_pass_the_gate() {
        let stock = synthetic_app(12_000, 1);
        let wheel = sim_profile(&[&stock]);
        let report = validate(&Image::raw(stock), &wheel);
        assert!(report.passed, "{report:#?}");
        assert!(matches!(report.identity, Identity::Stock { .. }));

        let ours = our_image(&wheel, 9_000, 2, "clean");
        let report = validate(&ours, &wheel);
        assert!(report.passed, "{report:#?}");
        let Identity::Ours { info } = report.identity else { panic!("ours") };
        assert!(info.is_sealed() && info.dev && !info.signed);
        assert_eq!(info.version.to_string(), "0.6.2");
    }

    #[test]
    fn geometry_fits_the_profile() {
        let wheel = sim_profile(&[]);
        assert!(euc_flash_sim::FlashSim::new(wheel, geometry()).is_ok());
    }
}
