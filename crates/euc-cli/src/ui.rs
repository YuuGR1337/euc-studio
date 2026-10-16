//! Terminal output: aligned columns, color only on a TTY, JSON on request.
//!
//! Colors go through `anstream`, which strips them when stdout is not a terminal and honors
//! `NO_COLOR` and `CLICOLOR_FORCE`.

use std::fmt::Display;

use anstream::{eprintln, println};
use anstyle::{AnsiColor, Style};
use serde::Serialize;

pub(crate) const GOOD: Style = AnsiColor::Green.on_default().bold();
pub(crate) const BAD: Style = AnsiColor::Red.on_default().bold();
pub(crate) const WARN: Style = AnsiColor::Yellow.on_default().bold();
pub(crate) const DIM: Style = Style::new().dimmed();
pub(crate) const STRONG: Style = Style::new().bold();

/// Width of the status column.
const STATUS: usize = 10;

/// `text` in `style`.
pub(crate) fn paint(style: Style, text: impl Display) -> String {
    format!("{style}{text}{style:#}")
}

/// One row: a styled status word padded to the status column, then the rest.
pub(crate) fn row(style: Style, status: &str, rest: impl Display) {
    println!("{style}{status:<STATUS$}{style:#}{rest}");
}

/// A continuation line under a row, aligned with its text.
pub(crate) fn detail(text: impl Display) {
    println!("{:STATUS$}{text}", "");
}

/// Pretty JSON on stdout.
pub(crate) fn json(value: &impl Serialize) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// The error and its causes on stderr.
pub(crate) fn error(error: &anyhow::Error) {
    eprintln!("{} {error:#}", paint(BAD, "error:"));
}

/// `41596` as `41,596`.
pub(crate) fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_thousands() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(41_596), "41,596");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
