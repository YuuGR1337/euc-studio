//! Keeping untrusted text from driving the terminal.

/// True for characters that can change how a terminal or editor shows the text around them:
/// C0 and C1 controls and DEL (ANSI escape sequences start with ESC or CSI), and the Unicode
/// `Bidi_Control` characters (overrides and isolates can reorder what a reader sees).
pub fn is_unsafe_display_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Makes text from outside the program (vendor servers, sidecar files, file names) safe to print
/// and to store.
///
/// Tab, CR and LF become spaces, so notes stay on one line. Every other unsafe character becomes
/// a visible escape such as `\u{1b}`, so tampering shows instead of acting. Safe text is returned
/// unchanged, and sanitizing twice changes nothing.
pub fn sanitize_display(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\t' | '\n' | '\r' => out.push(' '),
            c if is_unsafe_display_char(c) => out.extend(c.escape_unicode()),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_terminal_sequences() {
        assert_eq!(sanitize_display("\u{1b}[2J\u{1b}]0;owned\u{7}FW"), "\\u{1b}[2J\\u{1b}]0;owned\\u{7}FW");
        assert_eq!(sanitize_display("a\u{9b}31mb\u{7f}"), "a\\u{9b}31mb\\u{7f}", "C1 CSI and DEL");
        assert_eq!(sanitize_display("x\0y"), "x\\u{0}y");
    }

    #[test]
    fn escapes_bidi_controls() {
        assert_eq!(sanitize_display("GW\u{202E}7001"), "GW\\u{202e}7001");
        assert_eq!(sanitize_display("\u{2066}a\u{2069}\u{200F}"), "\\u{2066}a\\u{2069}\\u{200f}");
    }

    #[test]
    fn flattens_whitespace_controls() {
        assert_eq!(sanitize_display("1.20241108\n2.tilt\tangles\r"), "1.20241108 2.tilt angles ");
    }

    #[test]
    fn leaves_safe_text_alone_and_is_idempotent() {
        let text = "EXTREME_GW1825007 · 1.Update time:20251101 · 初始化完成";
        assert_eq!(sanitize_display(text), text);
        let once = sanitize_display("\u{1b}[31m\u{202E}");
        assert_eq!(sanitize_display(&once), once);
    }
}
