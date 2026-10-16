//! The image gate for one wheel (`docs/spec/euc-info.md`, "Image gate").

use std::fmt;

use euc_profile::{AppLayout, Date, Ram, Sha256, WheelProfile};
use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use thiserror::Error;

use crate::{EucInfo, Format, Image, InfoError, info_crc32};

/// One step of the gate, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Gate {
    /// An ELF must be linked at the app base.
    LoadAddress,
    /// The image fits the app slot and holds a whole vector table.
    Size,
    /// The initial stack pointer is 8-byte aligned and inside RAM.
    StackPointer,
    /// The reset vector and every other handler are Thumb addresses inside the image.
    Vectors,
    /// A `.euc_info` block, if present, is well formed, for this board, and intact when sealed.
    InfoBlock,
    /// The image is a pinned stock build or one of ours.
    Provenance,
}

impl fmt::Display for Gate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::LoadAddress => "load address",
            Self::Size => "size",
            Self::StackPointer => "stack pointer",
            Self::Vectors => "vectors",
            Self::InfoBlock => "info block",
            Self::Provenance => "provenance",
        })
    }
}

/// Result of one gate step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Passed; the text says what was found.
    Pass(String),
    /// Did not apply to this image; the text says why.
    Skip(String),
    /// Failed.
    Fail(Problem),
}

impl Outcome {
    /// True for [`Outcome::Fail`].
    pub const fn is_fail(&self) -> bool {
        matches!(self, Self::Fail(_))
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass(text) | Self::Skip(text) => f.write_str(text),
            Self::Fail(problem) => problem.fmt(f),
        }
    }
}

/// A gate step and its outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Which step.
    pub gate: Gate,
    /// What happened.
    pub outcome: Outcome,
}

/// Serialized flat for tools: `{gate, status, message, problem?}`.
impl Serialize for Check {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (status, problem) = match &self.outcome {
            Outcome::Pass(_) => ("pass", None),
            Outcome::Skip(_) => ("skip", None),
            Outcome::Fail(problem) => ("fail", Some(problem)),
        };
        let mut out = serializer.serialize_struct("Check", 4)?;
        out.serialize_field("gate", &self.gate)?;
        out.serialize_field("status", status)?;
        out.serialize_field("message", &self.outcome.to_string())?;
        match problem {
            Some(problem) => out.serialize_field("problem", problem)?,
            None => out.skip_field("problem")?,
        }
        out.end()
    }
}

/// Why an image is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Problem {
    /// The ELF is linked for another address.
    #[error("linked at 0x{found:08X}; this wheel loads apps at 0x{expected:08X}")]
    LoadAddress {
        /// Lowest load address in the ELF.
        found: u32,
        /// The profile's app base.
        expected: u32,
    },
    /// Larger than the app slot: flashing it would overwrite the settings pages.
    #[error("{size} bytes is more than the {max}-byte app slot; it would overwrite the settings pages")]
    TooLarge {
        /// Image size.
        size: usize,
        /// Slot size.
        max: u32,
    },
    /// Shorter than its own vector table.
    #[error("{size} bytes is shorter than the {min}-byte vector table")]
    TooSmall {
        /// Image size.
        size: usize,
        /// Vector table size.
        min: usize,
    },
    /// The core would fault on its first push (ARMv7-M wants an 8-byte aligned stack at reset).
    #[error("initial SP 0x{sp:08X} is not 8-byte aligned")]
    StackUnaligned {
        /// Word 0 of the image.
        sp: u32,
    },
    /// The stack would start outside RAM.
    #[error("initial SP 0x{sp:08X} is outside RAM 0x{base:08X}..=0x{end:08X}")]
    StackOutsideRam {
        /// Word 0 of the image.
        sp: u32,
        /// RAM start.
        base: u32,
        /// RAM end (an empty full-descending stack starts here).
        end: u32,
    },
    /// The reset vector is zero.
    #[error("the reset vector is 0")]
    NoReset,
    /// A handler address with bit 0 clear would fault with INVSTATE on entry.
    #[error("vector {index} ({vector}) = 0x{value:08X} is even, so it is not a Thumb address")]
    NotThumb {
        /// Index in the vector table.
        index: u32,
        /// Exception name.
        vector: String,
        /// The entry.
        value: u32,
    },
    /// A handler outside the image would run whatever is in flash there.
    #[error("vector {index} ({vector}) = 0x{value:08X} points outside the image 0x{start:08X}..0x{end:08X}")]
    OutsideImage {
        /// Index in the vector table.
        index: u32,
        /// Exception name.
        vector: String,
        /// The entry.
        value: u32,
        /// Image start.
        start: u32,
        /// Image end.
        end: u32,
    },
    /// The `.euc_info` block has the magic but breaks layout 1.
    #[error(".euc_info block: {error}")]
    Info {
        /// What is wrong.
        error: InfoError,
    },
    /// Built for another board.
    #[error("built for board \"{found}\"; this wheel's board is \"{expected}\"")]
    BoardMismatch {
        /// Board in the block.
        found: String,
        /// Board in the profile.
        expected: String,
    },
    /// Sealed for a different length: bytes were added or cut after sealing.
    #[error("sealed length is {stored} bytes but the image has {actual}")]
    LengthMismatch {
        /// `image_len` in the block.
        stored: u32,
        /// Real size.
        actual: usize,
    },
    /// The bytes changed after sealing.
    #[error("CRC-32 in the block is 0x{stored:08X} but the image hashes to 0x{computed:08X}")]
    CrcMismatch {
        /// `crc32` in the block.
        stored: u32,
        /// CRC of the image.
        computed: u32,
    },
    /// Not ours and not a pinned stock build.
    #[error("no .euc_info block, and sha256 {sha256} is not a pinned build for {wheel}; unknown images are refused")]
    Unpinned {
        /// Digest of the image.
        sha256: Sha256,
        /// Wheel display name.
        wheel: String,
    },
}

/// What the image turned out to be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Identity {
    /// An official build pinned in the wheel profile.
    Stock {
        /// Build name from the profile.
        label: String,
        /// Release date, if pinned.
        date: Option<Date>,
    },
    /// A build from this repo, identified by its `.euc_info` block.
    Ours {
        /// The block.
        info: EucInfo,
    },
    /// Neither.
    Unknown,
}

impl fmt::Display for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stock { label, date: Some(date) } => write!(f, "pinned stock build {label} ({date})"),
            Self::Stock { label, date: None } => write!(f, "pinned stock build {label}"),
            Self::Ours { info } => {
                let state = if info.is_sealed() { "sealed" } else { "unsealed" };
                let dev = if info.dev { " dev" } else { "" };
                write!(f, "{} {} {} ({}),{dev} {state}", info.board, info.profile, info.version, info.git_sha)
            }
            Self::Unknown => f.write_str("unknown image"),
        }
    }
}

/// Everything the gate found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    /// Wheel id the image was checked for.
    pub wheel: String,
    /// How the image was supplied.
    pub format: Format,
    /// Where the image loads.
    pub load_addr: u32,
    /// Image size in bytes.
    pub size: usize,
    /// Digest of the flash bytes.
    pub sha256: Sha256,
    /// What the image is.
    pub identity: Identity,
    /// Every step, in order.
    pub checks: Vec<Check>,
    /// True when no step failed.
    pub passed: bool,
}

impl Report {
    /// The failed steps' problems.
    pub fn problems(&self) -> impl Iterator<Item = &Problem> {
        self.checks.iter().filter_map(|check| match &check.outcome {
            Outcome::Fail(problem) => Some(problem),
            _ => None,
        })
    }
}

/// Runs the image gate for `wheel`.
pub fn validate(image: &Image, wheel: &WheelProfile) -> Report {
    let app = &wheel.board.app;
    let bytes = image.bytes();
    let sha256 = Sha256::digest(bytes);
    let table = app.vectors as usize * 4;
    let has_table = bytes.len() >= table;
    let no_table = || Outcome::Skip("no complete vector table".to_owned());

    let (info_outcome, provenance, identity) = identify(bytes, wheel, sha256);
    let checks = vec![
        Check { gate: Gate::LoadAddress, outcome: load_address(image, app) },
        Check { gate: Gate::Size, outcome: size(bytes.len(), table, app.max_size) },
        Check {
            gate: Gate::StackPointer,
            outcome: if has_table { stack_pointer(word(bytes, 0), wheel.board.ram) } else { no_table() },
        },
        Check { gate: Gate::Vectors, outcome: if has_table { vectors(bytes, app) } else { no_table() } },
        Check { gate: Gate::InfoBlock, outcome: info_outcome },
        Check { gate: Gate::Provenance, outcome: provenance },
    ];
    let passed = !checks.iter().any(|check| check.outcome.is_fail());
    Report {
        wheel: wheel.id.clone(),
        format: image.format(),
        load_addr: image.load_addr().unwrap_or(app.base),
        size: bytes.len(),
        sha256,
        identity,
        checks,
        passed,
    }
}

fn load_address(image: &Image, app: &AppLayout) -> Outcome {
    match image.load_addr() {
        None => Outcome::Skip(format!("{}, assumed to load at 0x{:08X}", Format::Raw, app.base)),
        Some(addr) if addr == app.base => Outcome::Pass(format!("linked at 0x{addr:08X}")),
        Some(found) => Outcome::Fail(Problem::LoadAddress { found, expected: app.base }),
    }
}

fn size(size: usize, table: usize, max: u32) -> Outcome {
    if size > max as usize {
        Outcome::Fail(Problem::TooLarge { size, max })
    } else if size < table {
        Outcome::Fail(Problem::TooSmall { size, min: table })
    } else {
        Outcome::Pass(format!("{size} of {max} bytes"))
    }
}

fn stack_pointer(sp: u32, ram: Ram) -> Outcome {
    if !sp.is_multiple_of(8) {
        Outcome::Fail(Problem::StackUnaligned { sp })
    } else if sp <= ram.base || sp > ram.end() {
        Outcome::Fail(Problem::StackOutsideRam { sp, base: ram.base, end: ram.end() })
    } else {
        Outcome::Pass(format!("0x{sp:08X}, inside RAM"))
    }
}

fn vectors(bytes: &[u8], app: &AppLayout) -> Outcome {
    let start = app.base;
    let end = app.base.saturating_add(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
    let mut handlers = 0;
    for index in 1..app.vectors {
        let value = word(bytes, index as usize);
        if value == 0 {
            if index == 1 {
                return Outcome::Fail(Problem::NoReset);
            }
            continue;
        }
        if value & 1 == 0 {
            return Outcome::Fail(Problem::NotThumb { index, vector: vector_name(index), value });
        }
        if !(start..end).contains(&(value & !1)) {
            return Outcome::Fail(Problem::OutsideImage { index, vector: vector_name(index), value, start, end });
        }
        handlers += 1;
    }
    Outcome::Pass(format!("reset 0x{:08X}, {handlers} handlers inside the image", word(bytes, 1)))
}

/// The `.euc_info` and provenance outcomes, and the identity they establish.
fn identify(bytes: &[u8], wheel: &WheelProfile, sha256: Sha256) -> (Outcome, Outcome, Identity) {
    let offset = wheel.board.app.info_offset as usize;
    match EucInfo::find(bytes, offset) {
        Err(error) => (
            Outcome::Fail(Problem::Info { error }),
            Outcome::Skip("the .euc_info block is not valid".to_owned()),
            Identity::Unknown,
        ),
        Ok(None) => {
            let none = Outcome::Skip("not present (stock builds have none)".to_owned());
            match wheel.find_pin(&sha256) {
                Some(pin) => {
                    let identity = Identity::Stock { label: pin.label.to_owned(), date: pin.date };
                    (none, Outcome::Pass(identity.to_string()), identity)
                }
                None => {
                    (none, Outcome::Fail(Problem::Unpinned { sha256, wheel: wheel.name.clone() }), Identity::Unknown)
                }
            }
        }
        Ok(Some(info)) => {
            let check = info_block(&info, bytes, &wheel.board.id, offset);
            let provenance = if check.is_fail() {
                Outcome::Skip("the .euc_info block failed its check".to_owned())
            } else if info.is_sealed() {
                Outcome::Pass("our build, sealed (signatures are checked by the flasher)".to_owned())
            } else {
                Outcome::Pass("our development build, unsealed".to_owned())
            };
            let identity = Identity::Ours { info };
            let check = match check {
                Outcome::Pass(_) => Outcome::Pass(identity.to_string()),
                other => other,
            };
            (check, provenance, identity)
        }
    }
}

fn info_block(info: &EucInfo, bytes: &[u8], board: &str, offset: usize) -> Outcome {
    if info.board != board {
        return Outcome::Fail(Problem::BoardMismatch { found: info.board.clone(), expected: board.to_owned() });
    }
    if info.is_sealed() {
        if info.image_len as usize != bytes.len() {
            return Outcome::Fail(Problem::LengthMismatch { stored: info.image_len, actual: bytes.len() });
        }
        let computed = info_crc32(bytes, offset);
        if info.crc32 != computed {
            return Outcome::Fail(Problem::CrcMismatch { stored: info.crc32, computed });
        }
    }
    Outcome::Pass(String::new())
}

fn word(bytes: &[u8], index: usize) -> u32 {
    let at = index * 4;
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Cortex-M exception name for a vector table index.
fn vector_name(index: u32) -> String {
    match index {
        1 => "Reset".into(),
        2 => "NMI".into(),
        3 => "HardFault".into(),
        4 => "MemManage".into(),
        5 => "BusFault".into(),
        6 => "UsageFault".into(),
        11 => "SVCall".into(),
        12 => "DebugMon".into(),
        14 => "PendSV".into(),
        15 => "SysTick".into(),
        7..=10 | 13 => "reserved".into(),
        irq => format!("IRQ{}", irq - 16),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::tests::build as build_elf;
    use crate::info::tests::{seal, write_block};

    const BASE: u32 = 0x0800_3000;
    const INFO: usize = 0x140;

    fn wheel(pinned: &[u8]) -> WheelProfile {
        WheelProfile::parse(&format!(
            r#"
schema = 1
id = "test-wheel"
name = "Test Wheel"
brand = "Test"

[board]
id = "test-board"
mcu = "STM32F103RC-class"
core = "Cortex-M3"
hclk_hz = 72_000_000
app = {{ base = 0x0800_3000, max_size = 49_152, vectors = 76, info_offset = 0x140 }}
ram = {{ base = 0x2000_0000, size = 0xC000 }}

[[firmware]]
kind = "manual"
sha256 = "{}"
label = "Synthetic 1"
date = 2025-01-02
"#,
            Sha256::digest(pinned)
        ))
        .unwrap()
    }

    /// A plausible app: SP at the top of a 8 KiB stack, reset and three handlers inside, the rest 0.
    fn app(len: usize) -> Vec<u8> {
        let mut image: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let mut vectors = [0_u32; 76];
        vectors[0] = 0x2000_2000;
        vectors[1] = BASE + 0x201;
        vectors[2] = BASE + 0x205;
        vectors[3] = BASE + 0x205;
        vectors[60] = BASE + 0x301; // IRQ44
        for (i, v) in vectors.iter().enumerate() {
            image[4 * i..4 * i + 4].copy_from_slice(&v.to_le_bytes());
        }
        image
    }

    fn set_word(image: &mut [u8], index: usize, value: u32) {
        image[4 * index..4 * index + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn outcome(report: &Report, gate: Gate) -> &Outcome {
        &report.checks.iter().find(|c| c.gate == gate).unwrap().outcome
    }

    fn only_problem(report: &Report) -> &Problem {
        let problems: Vec<_> = report.problems().collect();
        assert_eq!(problems.len(), 1, "{problems:?}");
        problems[0]
    }

    #[test]
    fn pinned_stock_build_passes() {
        let stock = app(0x1000);
        let report = validate(&Image::raw(stock.clone()), &wheel(&stock));
        assert!(report.passed, "{report:#?}");
        assert_eq!(report.identity.to_string(), "pinned stock build Synthetic 1 (2025-01-02)");
        assert_eq!(outcome(&report, Gate::Provenance), &Outcome::Pass(report.identity.to_string()));
        assert!(matches!(outcome(&report, Gate::LoadAddress), Outcome::Skip(_)));
        assert_eq!(outcome(&report, Gate::Vectors).to_string(), "reset 0x08003201, 4 handlers inside the image");
    }

    #[test]
    fn unknown_image_is_refused() {
        let report = validate(&Image::raw(app(0x1000)), &wheel(b"something else"));
        assert!(!report.passed);
        assert_eq!(report.identity, Identity::Unknown);
        let Problem::Unpinned { sha256, wheel } = only_problem(&report) else { panic!("expected Unpinned") };
        assert_eq!((*sha256, wheel.as_str()), (report.sha256, "Test Wheel"));
    }

    #[test]
    fn our_unsealed_dev_image_passes() {
        let mut image = app(0x1000);
        write_block(&mut image, INFO, "test-board", "clean");
        let report = validate(&Image::raw(image), &wheel(b""));
        assert!(report.passed, "{report:#?}");
        assert!(matches!(report.identity, Identity::Ours { .. }));
        assert_eq!(outcome(&report, Gate::InfoBlock).to_string(), "test-board clean 0.3.1 (abc1234), dev unsealed");
        assert_eq!(outcome(&report, Gate::Provenance).to_string(), "our development build, unsealed");
    }

    #[test]
    fn sealed_image_must_stay_intact() {
        let mut image = app(0x1000);
        write_block(&mut image, INFO, "test-board", "compat");
        seal(&mut image, INFO);
        assert!(validate(&Image::raw(image.clone()), &wheel(b"")).passed);

        let mut tampered = image.clone();
        tampered[0x800] ^= 0x01;
        let report = validate(&Image::raw(tampered), &wheel(b""));
        assert!(matches!(only_problem(&report), Problem::CrcMismatch { .. }));
        assert!(matches!(outcome(&report, Gate::Provenance), Outcome::Skip(_)));

        image.extend_from_slice(&[0; 4]);
        let report = validate(&Image::raw(image), &wheel(b""));
        assert_eq!(only_problem(&report), &Problem::LengthMismatch { stored: 0x1000, actual: 0x1004 });
    }

    #[test]
    fn image_for_another_board_is_refused() {
        let mut image = app(0x1000);
        write_block(&mut image, INFO, "other-board", "clean");
        let report = validate(&Image::raw(image), &wheel(b""));
        assert_eq!(
            only_problem(&report).to_string(),
            "built for board \"other-board\"; this wheel's board is \"test-board\""
        );
    }

    #[test]
    fn malformed_block_is_refused() {
        let mut image = app(0x1000);
        write_block(&mut image, INFO, "test-board", "clean");
        image[INFO + 4] = 9;
        let report = validate(&Image::raw(image), &wheel(b""));
        assert_eq!(only_problem(&report), &Problem::Info { error: InfoError::Layout(9) });
    }

    #[test]
    fn size_limits() {
        let big = app(49_153);
        let report = validate(&Image::raw(big.clone()), &wheel(&big));
        assert_eq!(only_problem(&report), &Problem::TooLarge { size: 49_153, max: 49_152 });
        assert_eq!(
            only_problem(&report).to_string(),
            "49153 bytes is more than the 49152-byte app slot; it would overwrite the settings pages"
        );

        let tiny = vec![0; 0x100];
        let report = validate(&Image::raw(tiny.clone()), &wheel(&tiny));
        assert_eq!(only_problem(&report), &Problem::TooSmall { size: 0x100, min: 0x130 });
        assert!(matches!(outcome(&report, Gate::Vectors), Outcome::Skip(_)));
    }

    #[test]
    fn stack_pointer_rules() {
        let check = |sp| {
            let mut image = app(0x1000);
            set_word(&mut image, 0, sp);
            let report = validate(&Image::raw(image.clone()), &wheel(&image));
            report.problems().cloned().collect::<Vec<_>>()
        };
        assert_eq!(check(0x2000_C000), []);
        assert_eq!(check(0x2000_1EEC), [Problem::StackUnaligned { sp: 0x2000_1EEC }]);
        let outside = |sp| Problem::StackOutsideRam { sp, base: 0x2000_0000, end: 0x2000_C000 };
        assert_eq!(check(0x2000_C008), [outside(0x2000_C008)]);
        assert_eq!(check(0x2000_0000), [outside(0x2000_0000)]);
    }

    #[test]
    fn vector_rules() {
        let check = |index: usize, value| {
            let mut image = app(0x1000);
            set_word(&mut image, index, value);
            let report = validate(&Image::raw(image.clone()), &wheel(&image));
            only_problem(&report).to_string()
        };
        assert_eq!(check(1, 0), "the reset vector is 0");
        assert_eq!(check(3, BASE + 0x204), "vector 3 (HardFault) = 0x08003204 is even, so it is not a Thumb address");
        assert_eq!(
            check(60, 0x0800_0101),
            "vector 60 (IRQ44) = 0x08000101 points outside the image 0x08003000..0x08004000"
        );
        assert_eq!(
            check(75, BASE + 0x1001),
            "vector 75 (IRQ59) = 0x08004001 points outside the image 0x08003000..0x08004000"
        );
    }

    #[test]
    fn elf_must_link_at_the_app_base() {
        let mut image = app(0x1000);
        write_block(&mut image, INFO, "test-board", "clean");

        let report = validate(&Image::from_bytes(&build_elf(40, &[(1, BASE, image.as_slice())])).unwrap(), &wheel(b""));
        assert!(report.passed, "{report:#?}");
        assert_eq!(report.format, Format::Elf);
        assert_eq!(outcome(&report, Gate::LoadAddress).to_string(), "linked at 0x08003000");

        let elsewhere = Image::from_bytes(&build_elf(40, &[(1, 0x0800_0000, image.as_slice())])).unwrap();
        let report = validate(&elsewhere, &wheel(b""));
        let problems: Vec<_> = report.problems().collect();
        assert_eq!(problems[0], &Problem::LoadAddress { found: 0x0800_0000, expected: BASE });
    }

    #[test]
    fn report_serializes_flat_checks() {
        let report = validate(&Image::raw(vec![0; 0x100]), &wheel(b"x"));
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["passed"], false);
        assert_eq!(json["identity"]["kind"], "unknown");
        let size = &json["checks"][1];
        assert_eq!(size["gate"], "size");
        assert_eq!(size["status"], "fail");
        assert_eq!(size["problem"]["kind"], "too-small");
        assert_eq!(size["message"], "256 bytes is shorter than the 304-byte vector table");
        assert!(json["checks"][0].get("problem").is_none());
    }
}
