use std::collections::HashSet;

use thiserror::Error;

use crate::{Channel, FirmwareSource, SCHEMA, Sha256, WheelProfile};

/// Size of the board field in the `.euc_info` block.
const BOARD_ID_MAX: usize = 16;
/// Size of the `.euc_info` block.
const INFO_BLOCK_LEN: u32 = 128;

/// Why a profile could not be read.
#[derive(Debug, Error)]
pub enum ParseError {
    /// Not valid TOML, or a field has the wrong name or type. The message points at the line.
    #[error("{0}")]
    Syntax(#[from] toml::de::Error),
    /// Valid TOML that breaks a rule of the schema.
    #[error("{0}")]
    Invalid(#[from] Problem),
}

/// A rule of the schema that a profile breaks.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Problem {
    /// The file was written for another schema version.
    #[error("schema {0} is not supported; this build reads schema {SCHEMA}")]
    Schema(u32),
    /// A required text field is empty.
    #[error("`{0}` must not be empty")]
    Empty(&'static str),
    /// An id with characters outside `a-z`, `0-9` and `-`.
    #[error("`{field}` = \"{value}\" must use only lowercase letters, digits and dashes")]
    NotKebab {
        /// Field name.
        field: &'static str,
        /// The bad value.
        value: String,
    },
    /// The board id does not fit the `.euc_info` board field.
    #[error("`board.id` = \"{0}\" is longer than {BOARD_ID_MAX} bytes, the size of the .euc_info board field")]
    BoardIdTooLong(String),
    /// The app slot is inconsistent.
    #[error("`board.app`: {0}")]
    App(String),
    /// The RAM region is empty or wraps past 4 GiB.
    #[error("`board.ram`: base 0x{base:08X} + size 0x{size:X} is not a valid region")]
    Ram {
        /// Region start.
        base: u32,
        /// Region size.
        size: u32,
    },
    /// Two firmware sources pin the same bytes.
    #[error("sha256 {0} is pinned twice")]
    DuplicatePin(Sha256),
    /// A Begode file name that is not a plain file name; it ends up in a URL and a cache path.
    #[error("firmware file \"{0}\" must be a plain file name (letters, digits, `_`, `-`, `.`)")]
    FileName(String),
    /// A Begode product code with characters that would need escaping in the API query.
    #[error("firmware type_code \"{0}\" must use only letters and digits")]
    TypeCode(String),
}

impl WheelProfile {
    /// Parses and validates the text of a `wheel.toml`.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let profile: Self = toml::from_str(text)?;
        profile.validate()?;
        Ok(profile)
    }

    /// Checks the rules TOML types cannot express.
    pub fn validate(&self) -> Result<(), Problem> {
        if self.schema != SCHEMA {
            return Err(Problem::Schema(self.schema));
        }
        kebab("id", &self.id)?;
        non_empty("name", &self.name)?;
        non_empty("brand", &self.brand)?;
        non_empty("board.mcu", &self.board.mcu)?;
        non_empty("board.core", &self.board.core)?;
        kebab("board.id", &self.board.id)?;
        if self.board.id.len() > BOARD_ID_MAX {
            return Err(Problem::BoardIdTooLong(self.board.id.clone()));
        }
        self.validate_app()?;
        let ram = self.board.ram;
        if ram.size == 0 || ram.base.checked_add(ram.size).is_none() {
            return Err(Problem::Ram { base: ram.base, size: ram.size });
        }
        self.validate_firmware()
    }

    fn validate_app(&self) -> Result<(), Problem> {
        let app = &self.board.app;
        let table = app.vectors.checked_mul(4).filter(|&bytes| app.vectors >= 16 && bytes <= app.max_size);
        let Some(table) = table else {
            return Err(Problem::App(format!(
                "{} vectors do not fit a {} byte slot (a Cortex-M table has at least 16)",
                app.vectors, app.max_size
            )));
        };
        // VTOR needs the table aligned to its size rounded up to a power of two, at least 128 bytes.
        let align = table.next_power_of_two().max(128);
        if !app.base.is_multiple_of(align) {
            return Err(Problem::App(format!(
                "base 0x{:08X} is not {align}-byte aligned, which VTOR needs for {} vectors",
                app.base, app.vectors
            )));
        }
        let Some(end) = app.base.checked_add(app.max_size) else {
            return Err(Problem::App(format!("base 0x{:08X} + max_size {} wraps past 4 GiB", app.base, app.max_size)));
        };
        if let Some(page) = app.storage_pages.iter().find(|&&page| (app.base..end).contains(&page)) {
            return Err(Problem::App(format!(
                "slot 0x{:08X}..0x{end:08X} overlaps the storage page at 0x{page:08X}",
                app.base
            )));
        }
        if app.info_offset < table
            || !app.info_offset.is_multiple_of(4)
            || app.info_offset.saturating_add(INFO_BLOCK_LEN) > app.max_size
        {
            return Err(Problem::App(format!(
                "info_offset 0x{:X} must be word aligned, after the 0x{table:X} byte vector table and inside the slot",
                app.info_offset
            )));
        }
        Ok(())
    }

    fn validate_firmware(&self) -> Result<(), Problem> {
        let mut seen = HashSet::new();
        for source in &self.firmware {
            non_empty("firmware.label", source.label())?;
            if let FirmwareSource::BegodeApi(build) = source {
                let type_code = build.type_code.as_deref().unwrap_or_default();
                if build.channel == Channel::Current {
                    non_empty("firmware.type_code", type_code)?;
                }
                if !type_code.bytes().all(|b| b.is_ascii_alphanumeric()) {
                    return Err(Problem::TypeCode(type_code.to_owned()));
                }
                if !is_plain_file_name(&build.file) {
                    return Err(Problem::FileName(build.file.clone()));
                }
            }
            if let Some(pin) = source.pin()
                && !seen.insert(pin.sha256)
            {
                return Err(Problem::DuplicatePin(pin.sha256));
            }
        }
        Ok(())
    }
}

fn non_empty(field: &'static str, value: &str) -> Result<(), Problem> {
    if value.trim().is_empty() { Err(Problem::Empty(field)) } else { Ok(()) }
}

fn kebab(field: &'static str, value: &str) -> Result<(), Problem> {
    non_empty(field, value)?;
    let ok = value.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-');
    if ok { Ok(()) } else { Err(Problem::NotKebab { field, value: value.to_owned() }) }
}

fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Date;

    const SHA_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const SHA_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn profile(board_and_firmware: &str) -> String {
        format!(
            r#"
schema = 1
id = "test-wheel"
name = "Test Wheel"
brand = "Test"
{board_and_firmware}
"#
        )
    }

    fn board(app: &str) -> String {
        format!(
            r#"
[board]
id = "test-board"
mcu = "STM32F103RC-class"
core = "Cortex-M3"
hclk_hz = 72_000_000
ram = {{ base = 0x2000_0000, size = 0xC000 }}
app = {{ {app} }}
"#
        )
    }

    const APP: &str =
        "base = 0x0800_3000, max_size = 49_152, vectors = 76, info_offset = 0x140, storage_pages = [0x0800_F000]";

    fn firmware(entries: &str) -> String {
        profile(&format!("{}\n{entries}", board(APP)))
    }

    fn problem(text: &str) -> Problem {
        match WheelProfile::parse(text) {
            Err(ParseError::Invalid(problem)) => problem,
            other => panic!("expected a schema problem, got {other:?}"),
        }
    }

    #[test]
    fn parses_every_source_kind() {
        let text = firmware(&format!(
            r#"
[[firmware]]
kind = "begode-api"
type_code = "GW0000001"
file = "pwm_1000.bin"
sha256 = "{SHA_A}"
label = "Build A"
date = 2025-11-01

[[firmware]]
kind = "manual"
sha256 = "{SHA_B}"
label = "Build B"
hint = "ask the dealer"

[[firmware]]
kind = "local"
path = "firmware/build/test.bin"
label = "ours"
"#
        ));
        let wheel = WheelProfile::parse(&text).unwrap();
        assert_eq!(wheel.board.app.end(), 0x0800_F000);
        assert_eq!(wheel.board.ram.end(), 0x2000_C000);

        let FirmwareSource::BegodeApi(api) = &wheel.firmware[0] else { panic!("first source is begode-api") };
        assert_eq!(api.channel, Channel::Current, "channel defaults to current");
        assert_eq!(api.date, Some(Date { year: 2025, month: 11, day: 1 }));

        let labels: Vec<_> = wheel.pins().map(|pin| pin.label).collect();
        assert_eq!(labels, ["Build A", "Build B"], "local builds are not pinned");
        let b = wheel.find_pin(&SHA_B.parse().unwrap()).unwrap();
        assert_eq!((b.label, b.date), ("Build B", None));
        assert!(wheel.find_pin(&Sha256::digest(b"other")).is_none());
    }

    #[test]
    fn syntax_errors_point_at_the_line() {
        let text = profile(&format!("{}\nunexpected = true\n", board(APP)));
        let message = WheelProfile::parse(&text).unwrap_err().to_string();
        assert!(message.contains("unexpected"), "{message}");
        assert!(message.contains("line"), "{message}");
    }

    #[test]
    fn rejects_a_bad_sha_with_its_text() {
        let text = firmware("[[firmware]]\nkind = \"manual\"\nsha256 = \"abc\"\nlabel = \"x\"\n");
        let message = WheelProfile::parse(&text).unwrap_err().to_string();
        assert!(message.contains("`abc` is not a sha256"), "{message}");
    }

    #[test]
    fn rejects_a_time_in_a_date() {
        let text = firmware(&format!(
            "[[firmware]]\nkind = \"manual\"\nsha256 = \"{SHA_A}\"\nlabel = \"x\"\ndate = 2025-11-01T10:00:00\n"
        ));
        let message = WheelProfile::parse(&text).unwrap_err().to_string();
        assert!(message.contains("expected a plain date"), "{message}");
    }

    #[test]
    fn rejects_unknown_schema() {
        let text = firmware("").replace("schema = 1", "schema = 2");
        assert_eq!(problem(&text), Problem::Schema(2));
    }

    #[test]
    fn rejects_bad_ids() {
        let text = firmware("").replace("\"test-wheel\"", "\"Test Wheel\"");
        assert!(matches!(problem(&text), Problem::NotKebab { field: "id", .. }));
        let text = firmware("").replace("\"test-board\"", "\"a-board-id-far-too-long\"");
        assert!(matches!(problem(&text), Problem::BoardIdTooLong(_)));
    }

    #[test]
    fn rejects_an_app_slot_over_the_storage_pages() {
        let text = profile(&board(&APP.replace("49_152", "49_153")));
        let message = problem(&text).to_string();
        assert!(message.contains("overlaps the storage page at 0x0800F000"), "{message}");
    }

    #[test]
    fn rejects_a_misaligned_vector_table() {
        let text = profile(&board(&APP.replace("0x0800_3000", "0x0800_3100")));
        assert!(problem(&text).to_string().contains("512-byte aligned"));
    }

    #[test]
    fn rejects_an_info_block_inside_the_vector_table() {
        let text = profile(&board(&APP.replace("0x140", "0x100")));
        assert!(problem(&text).to_string().contains("info_offset 0x100"));
    }

    #[test]
    fn rejects_duplicate_pins_and_unsafe_file_names() {
        let twice = format!("[[firmware]]\nkind = \"manual\"\nsha256 = \"{SHA_A}\"\nlabel = \"x\"\n");
        assert_eq!(problem(&firmware(&twice.repeat(2))), Problem::DuplicatePin(SHA_A.parse().unwrap()));

        let sneaky = format!(
            "[[firmware]]\nkind = \"begode-api\"\ntype_code = \"T\"\nfile = \"../x.bin\"\nsha256 = \"{SHA_A}\"\nlabel = \"x\"\n"
        );
        assert_eq!(problem(&firmware(&sneaky)), Problem::FileName("../x.bin".into()));
    }

    #[test]
    fn current_builds_need_a_type_code() {
        let entry = |channel: &str| {
            format!(
                "[[firmware]]\nkind = \"begode-api\"\nchannel = \"{channel}\"\nfile = \"a.bin\"\nsha256 = \"{SHA_A}\"\nlabel = \"x\"\n"
            )
        };
        assert_eq!(problem(&firmware(&entry("current"))), Problem::Empty("firmware.type_code"));
        assert!(WheelProfile::parse(&firmware(&entry("archive"))).is_ok());
        let odd = entry("current").replace("file =", "type_code = \"GW1&x=1\"\nfile =");
        assert_eq!(problem(&firmware(&odd)), Problem::TypeCode("GW1&x=1".into()));
    }
}
