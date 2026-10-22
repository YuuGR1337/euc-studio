//! The JSONL byte log: one JSON object per line, every byte in both directions, every gate,
//! step, link event, operator prompt and the outcome.

use std::io::Write;
use std::time::Duration;

use serde::Serialize;

use crate::gate::GateCheck;
use crate::session::Outcome;

/// One log line. Times are microseconds on the transport's clock.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Record<'a> {
    /// First line of a session.
    Start {
        /// Tool version.
        version: &'a str,
        /// `simulator` or `ble`.
        link: crate::LinkKind,
        /// The wheel's identity on the link.
        peer: &'a str,
        /// Wheel profile id.
        wheel: &'a str,
        /// Image to flash.
        image_sha256: String,
        /// Stock image for recovery.
        recovery_sha256: String,
    },
    /// A gate result.
    Gate {
        /// Time.
        t_us: u64,
        /// The check.
        #[serde(flatten)]
        check: &'a GateCheck,
    },
    /// A step of the state machine.
    Step {
        /// Time.
        t_us: u64,
        /// Step name.
        step: &'a str,
        /// Details.
        detail: String,
    },
    /// Bytes written.
    Tx {
        /// Time.
        t_us: u64,
        /// `command` or `ymodem`.
        kind: &'a str,
        /// The bytes, hex.
        hex: String,
    },
    /// Bytes received.
    Rx {
        /// Arrival time.
        t_us: u64,
        /// The bytes, hex.
        hex: String,
    },
    /// A link event: `lost`, `reconnected`, `reconnect-failed`, `refused`.
    Link {
        /// Time.
        t_us: u64,
        /// Event name.
        event: &'a str,
        /// Details.
        detail: String,
    },
    /// A question to the person at the wheel and the answer.
    Operator {
        /// Time.
        t_us: u64,
        /// What was asked or shown.
        prompt: &'a str,
        /// The answer, if any.
        answer: Option<String>,
    },
    /// Last line of a session.
    Outcome {
        /// Time.
        t_us: u64,
        /// How it ended.
        outcome: &'a Outcome,
    },
}

/// Where log lines go. A broken sink never stops a flash; the error is counted.
pub struct FlashLog {
    sink: Option<Box<dyn Write>>,
    kept: Option<Vec<String>>,
    lines: u64,
    write_errors: u64,
}

impl std::fmt::Debug for FlashLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlashLog")
            .field("lines", &self.lines)
            .field("write_errors", &self.write_errors)
            .finish_non_exhaustive()
    }
}

impl FlashLog {
    /// Lines go to `sink` (a file, usually).
    pub fn to_writer(sink: Box<dyn Write>) -> Self {
        Self { sink: Some(sink), kept: None, lines: 0, write_errors: 0 }
    }

    /// Lines are kept in memory ([`lines`](Self::lines)).
    pub fn in_memory() -> Self {
        Self { sink: None, kept: Some(Vec::new()), lines: 0, write_errors: 0 }
    }

    /// Lines are counted and dropped (the fault campaign).
    pub fn discard() -> Self {
        Self { sink: None, kept: None, lines: 0, write_errors: 0 }
    }

    /// False when lines are only counted: callers skip building expensive records then.
    pub const fn enabled(&self) -> bool {
        self.sink.is_some() || self.kept.is_some()
    }

    /// Writes one record.
    pub fn record(&mut self, record: &Record<'_>) {
        self.lines += 1;
        if self.sink.is_none() && self.kept.is_none() {
            return;
        }
        let Ok(line) = serde_json::to_string(record) else {
            self.write_errors += 1;
            return;
        };
        if let Some(sink) = &mut self.sink
            && writeln!(sink, "{line}").and_then(|()| sink.flush()).is_err()
        {
            self.write_errors += 1;
        }
        if let Some(kept) = &mut self.kept {
            kept.push(line);
        }
    }

    /// Lines kept in memory.
    pub fn lines(&self) -> &[String] {
        self.kept.as_deref().unwrap_or_default()
    }

    /// Records written (or counted).
    pub const fn count(&self) -> u64 {
        self.lines
    }

    /// Lines the sink failed to take.
    pub const fn write_errors(&self) -> u64 {
        self.write_errors
    }
}

/// Microseconds for the log.
pub(crate) fn micros(time: Duration) -> u64 {
    u64::try_from(time.as_micros()).unwrap_or(u64::MAX)
}

/// Lowercase hex without separators.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_are_one_json_object_per_line() {
        let mut log = FlashLog::in_memory();
        log.record(&Record::Tx { t_us: 5, kind: "command", hex: hex(b"V") });
        log.record(&Record::Rx { t_us: 9, hex: hex(&[0x55, 0xAA]) });
        assert_eq!(
            log.lines(),
            [r#"{"type":"tx","t_us":5,"kind":"command","hex":"56"}"#, r#"{"type":"rx","t_us":9,"hex":"55aa"}"#]
        );
        assert_eq!(log.count(), 2);

        let mut discarded = FlashLog::discard();
        discarded.record(&Record::Rx { t_us: 1, hex: String::new() });
        assert_eq!((discarded.count(), discarded.lines().len()), (1, 0));
    }

    #[test]
    fn writer_sink_gets_the_lines() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut log = FlashLog::to_writer(Box::new(file.reopen().unwrap()));
        log.record(&Record::Step { t_us: 1, step: "listen", detail: "5 s".into() });
        let text = std::fs::read_to_string(file.path()).unwrap();
        assert_eq!(text, "{\"type\":\"step\",\"t_us\":1,\"step\":\"listen\",\"detail\":\"5 s\"}\n");
    }
}
