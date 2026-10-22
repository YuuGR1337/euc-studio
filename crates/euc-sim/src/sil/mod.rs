//! Host software in the loop: our firmware built for the host (`libeuc_sil`, from
//! `firmware/ports/sim`) running as the machine's CPU, with its quirks switched at run time.
//!
//! The library runs the Begode Extreme app's own code natively and drives the board through the
//! same register sequences as the image, which this backend sends to the machine's bus (see
//! [`SilCpu`]). The machine, its peripheral models, the plant and the board logic are the ones an
//! emulated run has, so sessions, scenarios, comparisons and recordings work unchanged. What
//! differs is time: the firmware's code takes none, and each register access costs
//! [`ACCESS_CYCLES`] instead (`docs/sil.md`).
//!
//! A run needs three things from a [`SilLibrary`]: an [`image`](SilLibrary::image) (a vector
//! table that routes each interrupt the library handles to a context of its own), a
//! [`map`](SilLibrary::map) (the probes, which the backend mirrors into emulated SRAM after every
//! quantum, so `Session` reads them like an image's variables) and a
//! [`factory`](SilLibrary::factory) for the CPU with the quirk mask. Build the machine with
//! [`EXCEPTION_TIMING`]: without instruction timing there are no context-switch costs either.

// FFI to the SIL library and its coroutine stack; each unsafe block says why it holds.
#![allow(unsafe_code)]

mod cpu;
mod library;

use std::path::{Path, PathBuf};
use std::rc::Rc;

pub use cpu::{ACCESS_CYCLES, PORT_FAULT, SilCpu};

use crate::config::{RamVar, StockMap};
use crate::machine::{CpuFactory, ExceptionTiming};

/// Exception switch costs for SIL runs: none. The firmware's code has no instruction timing, so
/// the machine charges no entry, return or tail-chain cycles either (and reports no ISR load).
pub const EXCEPTION_TIMING: ExceptionTiming = ExceptionTiming::REFERENCE;

/// Where the probes are mirrored in emulated SRAM: `struct app` of the library, from here up.
const MIRROR_BASE: u32 = 0x2000_0000;
/// The initial stack pointer: the exception unit stacks its frames below it, above the mirror and
/// below the library's DMA buffers (`EUC_SIL_DMA_RAM_ADDRESS`).
const STACK_TOP: u32 = 0x2000_8000;
/// Room kept for exception frames between the mirror and [`STACK_TOP`].
const FRAME_ROOM: u32 = 0x400;
/// Image offsets of the synthetic vector table's targets: each handled exception's context, the
/// default handler, thread mode.
const HANDLERS_AT: u32 = 0x200;
const DEFAULT_HANDLER_AT: u32 = 0x3F0;
const THREAD_AT: u32 = 0x3F8;
/// Size of the synthetic image.
const IMAGE_BYTES: usize = 0x400;
/// Vector table entries of the STM32F1 high-density line (16 + 60).
const VECTORS: u32 = 76;
/// TIM8 `BDTR`, which the default handler clears the main output enable in.
pub(crate) const TIM8_BDTR: u32 = 0x4001_3444;

/// One switchable stock behaviour (`firmware/config/quirks.def`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quirk {
    /// Bit in a mask.
    pub index: u32,
    /// ID as in `quirks.def` (`ASIN_ACCEL`).
    pub name: String,
    /// One line on the stock behaviour.
    pub summary: String,
    /// The compat profile has it.
    pub compat: bool,
    /// The clean profile keeps it.
    pub clean: bool,
}

/// An interrupt context of the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// Device interrupt number (`IRQn`).
    pub irq: u16,
    /// For people.
    pub name: String,
}

/// A probe of the library: a named place in its `struct app`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// Name, as in `probes.toml`.
    pub name: String,
    /// `u8`, `s16`, `f32`, ...
    pub kind: String,
    /// `cutoff` or none.
    pub role: Option<String>,
    /// What it is.
    pub note: Option<String>,
    /// Byte offset in this build's `struct app`.
    pub offset: u32,
    /// Size in bytes.
    pub size: u32,
}

/// A probe's bytes and where they go in emulated SRAM.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mirror {
    pub(crate) offset: u32,
    pub(crate) size: u32,
    pub(crate) address: u32,
}

/// A `libeuc_sil` build: what it handles, its quirks and probes. See the module docs.
#[derive(Debug, Clone)]
pub struct SilLibrary {
    path: PathBuf,
    quirks: Vec<Quirk>,
    compat: u64,
    clean: u64,
    contexts: Vec<Context>,
    probes: Vec<Probe>,
    state_bytes: u32,
}

/// Why this platform cannot run the SIL library, or `None` where it can. The library is built and
/// tested on macOS and Linux only: on Windows the firmware build has no link for it (the storage pages
/// aliased to host arrays), and a loaded DLL cannot be deleted, which each power-on's fresh copy needs.
pub const UNSUPPORTED: Option<&str> = if cfg!(windows) {
    Some("ours-sil is not supported on Windows yet")
} else if cfg!(unix) {
    None
} else {
    Some("ours-sil needs macOS or Linux")
};

/// The library's file name on this platform (`libeuc_sil.dylib`, `libeuc_sil.so`, `euc_sil.dll`).
pub const fn library_file_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "libeuc_sil.dylib"
    } else if cfg!(windows) {
        "euc_sil.dll"
    } else {
        "libeuc_sil.so"
    }
}

/// `firmware/build/host-sil/<library>` in `dir` or the nearest folder above it that has one.
pub fn find_library(dir: &Path) -> Option<PathBuf> {
    let relative = Path::new("firmware/build/host-sil").join(library_file_name());
    dir.ancestors().map(|d| d.join(&relative)).find(|p| p.is_file())
}

/// Bytes a probe of `kind` has.
fn kind_bytes(kind: &str) -> Option<u32> {
    match kind {
        "u8" | "s8" => Some(1),
        "u16" | "s16" => Some(2),
        "u32" | "s32" | "f32" => Some(4),
        _ => None,
    }
}

impl SilLibrary {
    /// Opens the library at `path`: loads a copy once to read its quirks, contexts and probes.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        if let Some(why) = UNSUPPORTED {
            return Err(why.to_owned());
        }
        let path = path.into();
        let instance = library::Instance::load(&path)?;
        let api = instance.api;
        let name = |what: &str, pointer| instance.text(pointer).ok_or_else(|| format!("{}: no {what}", path.display()));
        // SAFETY (each call below): plain getters of the loaded library, called with indexes below
        // the counts it reports.
        let count = unsafe { (api.quirk_count)() };
        if count > 64 {
            return Err(format!("{}: {count} quirks do not fit a 64-bit mask", path.display()));
        }
        let (compat, clean) = unsafe {
            ((api.quirk_profile_mask)(library::PROFILE_COMPAT), (api.quirk_profile_mask)(library::PROFILE_CLEAN))
        };
        let mut quirks = Vec::new();
        for index in 0..count {
            quirks.push(Quirk {
                index,
                name: name("quirk name", unsafe { (api.quirk_name)(index) })?,
                summary: name("quirk summary", unsafe { (api.quirk_summary)(index) })?,
                compat: compat >> index & 1 == 1,
                clean: clean >> index & 1 == 1,
            });
        }
        let mut contexts = Vec::new();
        for index in 0..unsafe { (api.context_count)() } {
            let irq = unsafe { (api.context_irq)(index) };
            let irq = u16::try_from(irq)
                .ok()
                .filter(|&irq| u32::from(irq) + 16 < VECTORS)
                .ok_or_else(|| format!("{}: context {index} serves IRQ {irq}", path.display()))?;
            contexts.push(Context { irq, name: name("context name", unsafe { (api.context_name)(index) })? });
        }
        let state_bytes = unsafe { (api.state_bytes)() };
        let mut probes = Vec::new();
        for index in 0..unsafe { (api.probe_count)() } {
            let probe = Probe {
                name: name("probe name", unsafe { (api.probe_name)(index) })?,
                kind: name("probe kind", unsafe { (api.probe_kind)(index) })?,
                role: instance.text(unsafe { (api.probe_role)(index) }),
                note: instance.text(unsafe { (api.probe_note)(index) }),
                offset: unsafe { (api.probe_offset)(index) },
                size: unsafe { (api.probe_size)(index) },
            };
            if kind_bytes(&probe.kind) != Some(probe.size) || probe.offset + probe.size > state_bytes {
                return Err(format!(
                    "{}: probe {} is {} bytes at {} ({}), in a state of {state_bytes} bytes",
                    path.display(),
                    probe.name,
                    probe.size,
                    probe.offset,
                    probe.kind
                ));
            }
            probes.push(probe);
        }
        if MIRROR_BASE + state_bytes > STACK_TOP - FRAME_ROOM {
            return Err(format!("{}: a state of {state_bytes} bytes does not fit the probe mirror", path.display()));
        }
        Ok(Self { path, quirks, compat, clean, contexts, probes, state_bytes })
    }

    /// The library file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The quirks, in bit order.
    pub fn quirks(&self) -> &[Quirk] {
        &self.quirks
    }

    /// The quirks of the compat profile.
    pub const fn compat_mask(&self) -> u64 {
        self.compat
    }

    /// The quirks the clean profile keeps.
    pub const fn clean_mask(&self) -> u64 {
        self.clean
    }

    /// Every quirk.
    pub fn all_mask(&self) -> u64 {
        if self.quirks.len() >= 64 { u64::MAX } else { (1 << self.quirks.len()) - 1 }
    }

    /// The interrupt contexts.
    pub fn contexts(&self) -> &[Context] {
        &self.contexts
    }

    /// The probes, with this build's offsets.
    pub fn probes(&self) -> &[Probe] {
        &self.probes
    }

    /// Size of the library's `struct app`.
    pub const fn state_bytes(&self) -> u32 {
        self.state_bytes
    }

    fn quirk(&self, name: &str) -> Result<u64, String> {
        self.quirks.iter().find(|q| q.name.eq_ignore_ascii_case(name)).map(|q| 1 << q.index).ok_or_else(|| {
            let names: Vec<&str> = self.quirks.iter().map(|q| q.name.as_str()).collect();
            format!("unknown quirk {name} (known: {})", names.join(", "))
        })
    }

    /// A quirk mask from text: a base (`compat`, `clean`, `none`, `all`, or a number such as
    /// `0x1f`), then quirks to add (`+NAME`, or `NAME`) or remove (`-NAME`), separated by commas or
    /// written one after the other: `clean+ASIN_ACCEL`, `compat,-NO_WATCHDOG`, `ASIN_ACCEL,KEIL_FTZ`
    /// (no base: none). [`describe`](Self::describe) writes this form.
    pub fn parse_quirks(&self, spec: &str) -> Result<u64, String> {
        let mut tokens = Vec::new();
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let mut start = 0;
            for (i, c) in part.char_indices().skip(1) {
                if c == '+' || c == '-' {
                    tokens.push(&part[start..i]);
                    start = i;
                }
            }
            tokens.push(&part[start..]);
        }
        if tokens.is_empty() {
            return Err("no quirks given: use compat, clean, none, a mask or quirk names".to_owned());
        }
        let mut mask = None;
        for (i, token) in tokens.iter().enumerate() {
            let (sign, body) = match token.as_bytes()[0] {
                b'+' => (1, token[1..].trim()),
                b'-' => (-1, token[1..].trim()),
                _ => (0, token.trim()),
            };
            if i == 0 && sign == 0 {
                let base = match body.to_ascii_lowercase().as_str() {
                    "compat" => Some(self.compat),
                    "clean" => Some(self.clean),
                    "none" => Some(0),
                    "all" => Some(self.all_mask()),
                    number => parse_number(number),
                };
                if let Some(base) = base {
                    if base & !self.all_mask() != 0 {
                        return Err(format!("mask {body} has bits above the {} quirks", self.quirks.len()));
                    }
                    mask = Some(base);
                    continue;
                }
            }
            let bit = self.quirk(body)?;
            if sign < 0 {
                let base = mask.ok_or_else(|| format!("-{body}: start with compat, clean or none to remove from"))?;
                mask = Some(base & !bit);
            } else {
                mask = Some(mask.unwrap_or(0) | bit);
            }
        }
        Ok(mask.unwrap_or(0))
    }

    /// `mask` as [`parse_quirks`](Self::parse_quirks) reads it, from the profile it is closest to:
    /// `compat`, `clean`, `clean+ASIN_ACCEL-NO_WATCHDOG`, `none+KEIL_FTZ`.
    pub fn describe(&self, mask: u64) -> String {
        let bases = [("compat", self.compat), ("clean", self.clean), ("none", 0), ("all", self.all_mask())];
        let (name, base) = bases.into_iter().min_by_key(|(_, base)| (base ^ mask).count_ones()).unwrap_or(("none", 0));
        let mut text = name.to_owned();
        for quirk in &self.quirks {
            let bit = 1 << quirk.index;
            if (base ^ mask) & bit != 0 {
                text.push(if mask & bit != 0 { '+' } else { '-' });
                text.push_str(&quirk.name);
            }
        }
        text
    }

    /// The flash image a SIL machine boots: a vector table, the reset vector pointing at thread mode
    /// and each interrupt the library handles at a context of its own; every other one shares the
    /// default handler, as in the image. Nothing in it runs.
    pub fn image(&self, load_addr: u32) -> Vec<u8> {
        let mut image = vec![0_u8; IMAGE_BYTES];
        let mut put = |slot: u32, value: u32| {
            let at = 4 * slot as usize;
            image[at..at + 4].copy_from_slice(&value.to_le_bytes());
        };
        put(0, STACK_TOP);
        put(1, (load_addr + THREAD_AT) | 1);
        for exception in 2..VECTORS {
            put(exception, (load_addr + DEFAULT_HANDLER_AT) | 1);
        }
        for context in &self.contexts {
            let exception = 16 + u32::from(context.irq);
            put(exception, (load_addr + HANDLERS_AT + 4 * exception) | 1);
        }
        image
    }

    /// The probes as a variable map named `label`: each at its place in the mirror.
    pub fn map(&self, label: &str) -> StockMap {
        let ram = self
            .probes
            .iter()
            .map(|p| RamVar {
                name: p.name.clone(),
                addr: MIRROR_BASE + p.offset,
                size: p.size,
                kind: p.kind.clone(),
                note: p.note.clone(),
                role: p.role.clone(),
            })
            .collect();
        StockMap { schema: 1, build: label.to_owned(), entries: Vec::new(), ram }
    }

    /// The CPU backend: a fresh library copy with quirk `mask` at every power-on.
    pub fn factory(&self, mask: u64) -> Box<CpuFactory<'static>> {
        let path = self.path.clone();
        let contexts: Rc<[u16]> = self.contexts.iter().map(|c| 16 + c.irq).collect();
        let mirror: Rc<[Mirror]> = self
            .probes
            .iter()
            .map(|p| Mirror { offset: p.offset, size: p.size, address: MIRROR_BASE + p.offset })
            .collect();
        Box::new(move |_regions| Ok(Box::new(SilCpu::new(path, mask, contexts, mirror)) as Box<dyn euc_core::Cpu>))
    }
}

fn parse_number(text: &str) -> Option<u64> {
    match text.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) => text.parse().ok(),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Machine, MachineConfig, PlantState, WheelFiles};

    /// Three quirks, as `open` reads them, for the text tests.
    fn fake() -> SilLibrary {
        let quirk = |index, name: &str, compat, clean| Quirk {
            index,
            name: name.into(),
            summary: String::new(),
            compat,
            clean,
        };
        SilLibrary {
            path: PathBuf::from("libeuc_sil.test"),
            quirks: vec![
                quirk(0, "ASIN_ACCEL", true, false),
                quirk(1, "NO_WATCHDOG", true, false),
                quirk(2, "WC_DEAD", true, true),
            ],
            compat: 0b111,
            clean: 0b100,
            contexts: vec![Context { irq: 44, name: "TIM8 update".into() }, Context { irq: 3, name: "RTC".into() }],
            probes: vec![Probe {
                name: "speed_cms".into(),
                kind: "s16".into(),
                role: Some("cutoff".into()),
                note: None,
                offset: 8,
                size: 2,
            }],
            state_bytes: 64,
        }
    }

    #[test]
    fn open_refuses_on_windows_and_names_a_missing_file_elsewhere() {
        assert_eq!(UNSUPPORTED.is_some(), cfg!(windows));
        let Err(why) = SilLibrary::open("no-such-dir/libeuc_sil.test") else { panic!("there is no library there") };
        if cfg!(windows) {
            assert_eq!(why, "ours-sil is not supported on Windows yet");
        } else {
            assert!(why.contains("no-such-dir"), "{why}");
        }
    }

    #[test]
    fn quirk_texts_parse_and_describe_both_ways() {
        let lib = fake();
        let parse = |spec: &str| lib.parse_quirks(spec);
        assert_eq!(parse("compat"), Ok(0b111));
        assert_eq!(parse("CLEAN"), Ok(0b100));
        assert_eq!(parse("none"), Ok(0));
        assert_eq!(parse("all"), Ok(0b111));
        assert_eq!(parse("0x3"), Ok(0b011));
        assert_eq!(parse("5"), Ok(0b101));
        assert_eq!(parse("clean+ASIN_ACCEL"), Ok(0b101));
        assert_eq!(parse("compat-NO_WATCHDOG"), Ok(0b101));
        assert_eq!(parse("compat, -no_watchdog, +ASIN_ACCEL"), Ok(0b101));
        assert_eq!(parse("ASIN_ACCEL,WC_DEAD"), Ok(0b101), "names alone start from none");
        for mask in 0..8 {
            let text = lib.describe(mask);
            assert_eq!(parse(&text), Ok(mask), "{text}");
        }
        assert_eq!(lib.describe(0b111), "compat");
        assert_eq!(lib.describe(0b100), "clean");
        assert_eq!(lib.describe(0b101), "compat-NO_WATCHDOG", "a tie goes to the first profile");
    }

    #[test]
    fn bad_quirk_texts_are_refused_with_the_reason() {
        let lib = fake();
        assert!(lib.parse_quirks("").unwrap_err().starts_with("no quirks"));
        assert!(lib.parse_quirks("clean+NOPE").unwrap_err().contains("unknown quirk NOPE (known: ASIN_ACCEL"));
        assert!(lib.parse_quirks("-ASIN_ACCEL").unwrap_err().contains("start with compat, clean or none"));
        assert!(lib.parse_quirks("0x8").unwrap_err().contains("bits above the 3 quirks"));
    }

    #[test]
    fn the_image_routes_each_handled_interrupt_to_a_context_of_its_own() {
        let lib = fake();
        let base = 0x0800_3000;
        let image = lib.image(base);
        let word = |slot: usize| u32::from_le_bytes(image[4 * slot..4 * slot + 4].try_into().unwrap());
        assert_eq!(word(0), STACK_TOP);
        assert_eq!(word(1), base + THREAD_AT + 1);
        let default = base + DEFAULT_HANDLER_AT + 1;
        let tim8 = word(16 + 44);
        let rtc = word(16 + 3);
        assert!(tim8 != default && rtc != default && tim8 != rtc);
        assert_eq!(tim8 & 1, 1, "Thumb bit");
        assert!((tim8 & !1) - base < IMAGE_BYTES as u32);
        let shared = (2..76).filter(|&slot| word(slot) == default).count();
        assert_eq!(shared, 76 - 2 - 2, "every other exception shares the default handler");
    }

    #[test]
    fn the_map_places_each_probe_in_the_mirror() {
        let map = fake().map("ours-sil (compat)");
        assert_eq!((map.build.as_str(), map.ram.len()), ("ours-sil (compat)", 1));
        let var = &map.ram[0];
        assert_eq!(
            (var.addr, var.size, var.kind.as_str(), var.role.as_deref()),
            (MIRROR_BASE + 8, 2, "s16", Some("cutoff"))
        );
    }

    /// The library this checkout built.
    fn built() -> SilLibrary {
        let path = find_library(Path::new(env!("CARGO_MANIFEST_DIR")))
            .expect("build it: (cd firmware && cmake --workflow --preset host-sil)");
        SilLibrary::open(path).unwrap()
    }

    /// The Extreme with a static plant on the built library, quirks `mask`.
    fn machine(lib: &SilLibrary, mask: u64, power_cycles: Vec<f64>) -> Machine {
        let files = WheelFiles::load(&euc_profile::ProfileSource::Builtin, "begode-extreme").unwrap();
        let board = files.board_with_settings("neutral").unwrap();
        let base = files.profile.board.app.base;
        let image = lib.image(base);
        let map = lib.map("ours-sil");
        let config = MachineConfig {
            board: &board,
            stockmap: Some(&map),
            image: &image,
            load_addr: base,
            hclk: files.hclk(),
            plant: PlantState::default(),
            injections: vec![(2.5, b"V".to_vec())],
            power_cycles,
            cpi: crate::DEFAULT_CPI,
            exception_timing: EXCEPTION_TIMING,
            max_quantum: 4000,
            marker_window_s: 0.0,
        };
        Machine::new(&config, &crate::registry(), lib.factory(mask)).unwrap()
    }

    fn gyro_config(machine: &Machine) -> u8 {
        machine.component::<euc_devices::Mpu6500>("imu").unwrap().register(0x1B)
    }

    #[test]
    #[ignore = "needs firmware/build/host-sil (cmake --workflow --preset host-sil)"]
    fn the_built_library_boots_answers_and_mirrors_its_probes() {
        let lib = built();
        assert_eq!(lib.contexts().len(), 4, "TIM8 update, ADC, USART1, RTC");
        assert!(lib.quirks().iter().any(|q| q.name == "GYRO_500DPS_CLIP"));
        let mut machine = machine(&lib, lib.compat_mask(), Vec::new());
        assert_eq!(machine.run(4 * machine.hclk()), None);
        let text: Vec<u8> = machine.board().ble_tx.iter().map(|&(_, b)| b).collect();
        assert!(text.windows(9).any(|w| w == b"GW1825020"), "V answers with the banner");
        assert!(text.windows(2).filter(|w| w == &[0x55, 0xAA]).count() > 20, "telemetry frames stream");
        // The probes read through the mirror.
        let map = lib.map("ours-sil");
        let speed = map.ram.iter().find(|v| v.name == "speed_cms").unwrap();
        assert_eq!(machine.memory(speed.addr, 2), Some(&[0, 0][..]), "standing still");
    }

    #[test]
    #[ignore = "needs firmware/build/host-sil (cmake --workflow --preset host-sil)"]
    fn each_run_takes_its_own_quirks() {
        let lib = built();
        let gyro = lib.quirks().iter().find(|q| q.name == "GYRO_500DPS_CLIP").unwrap().index;
        // Two machines at once, each on its own copy of the library: the masks do not leak.
        let mut compat = machine(&lib, lib.compat_mask(), Vec::new());
        let mut wide = machine(&lib, lib.compat_mask() & !(1 << gyro), Vec::new());
        for _ in 0..20 {
            assert_eq!(compat.run(compat.hclk() / 10), None);
            assert_eq!(wide.run(wide.hclk() / 10), None);
        }
        assert_eq!(gyro_config(&compat), 0x08, "stock +-500 deg/s");
        assert_eq!(gyro_config(&wide), 0x18, "+-2000 deg/s without the quirk");
    }

    #[test]
    #[ignore = "needs firmware/build/host-sil (cmake --workflow --preset host-sil)"]
    fn a_power_cycle_starts_a_fresh_firmware() {
        let lib = built();
        let mut machine = machine(&lib, lib.clean_mask(), vec![3.0]);
        assert_eq!(machine.run(6 * machine.hclk()), None);
        let starts = machine.sys.diag.events.iter().filter(|(_, text)| text.contains("NVIC enable RTC")).count();
        assert_eq!(starts, 2, "the start-up ran again after the power cycle");
        assert_eq!(gyro_config(&machine), 0x18, "the clean quirks hold after the cycle");
        let late = 5 * machine.hclk();
        let frames = machine.board().ble_tx.iter().filter(|&&(at, b)| at > late && b == 0x55).count();
        assert!(frames > 5, "telemetry streams again");
    }
}
