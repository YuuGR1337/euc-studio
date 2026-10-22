//! What a phone shows for each segment the wheel sends: a kind, one line for people and the
//! decoded fields, in wire order. Recordings store BLE frames this way.

use std::fmt::Write as _;

use crate::{Reply, Segment, Telemetry};

/// A segment, decoded for people.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Described {
    /// `live`, `bms`, `cells`, `status`, `power`, `other` or `text`.
    pub kind: &'static str,
    /// One line (`31.4 km/h, 128.20 V, 2.10 A`).
    pub summary: String,
    /// Decoded fields, in wire order.
    pub fields: Vec<(String, String)>,
    /// A reply to a phone command (`V`, `N`, speed alarms).
    pub reply: bool,
}

/// The segment's bytes as they went over the air.
pub fn segment_bytes(segment: &Segment) -> &[u8] {
    match segment {
        Segment::Text(bytes) => bytes,
        Segment::Frame(frame) => frame.as_bytes(),
    }
}

/// `55 AA 19`: bytes as spaced upper-case hex.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{b:02X}");
    }
    s
}

fn field(name: &str, value: impl Into<String>) -> (String, String) {
    (name.to_owned(), value.into())
}

/// Decodes `segment` for people.
pub fn describe(segment: &Segment) -> Described {
    match segment {
        Segment::Text(bytes) => {
            let (reply, summary) = match Reply::parse(bytes) {
                Reply::Version(v) => (true, format!("Version {v}")),
                Reply::Name(n) => (true, format!("Name {n}")),
                Reply::SpeedAlarms(r) => (true, format!("Speed alarms: {r:?}")),
                Reply::ImuInit { ok } => (false, format!("IMU init {}", if ok { "ok" } else { "FAILED" })),
                Reply::InitDone { boot_count } => (false, format!("Init done, boot {boot_count}")),
                Reply::Text(text) => (false, text.trim().chars().take(80).collect()),
            };
            Described { kind: "text", summary, fields: Vec::new(), reply }
        }
        Segment::Frame(frame) => {
            let (kind, summary, fields) = match Telemetry::decode(frame) {
                Telemetry::Live(l) => (
                    "live",
                    format!("{:.1} km/h, {:.2} V, {:.2} A", l.speed_kmh(), l.voltage_v(), l.phase_current_a()),
                    vec![
                        field("Voltage", format!("{:.2} V", l.voltage_v())),
                        field("Speed", format!("{:.2} km/h", l.speed_kmh())),
                        field("Range", format!("{} km", l.range)),
                        field("Trip", format!("{} m", l.trip)),
                        field("Phase current", format!("{:.2} A", l.phase_current_a())),
                        field("Board temp", format!("{:.1} °C", l.temperature_c())),
                        field("Beeper volume", l.beeper_volume.to_string()),
                    ],
                ),
                Telemetry::Bms(b) => (
                    "bms",
                    format!("BMS block {}: {:.2} V, {:.2} A", b.block, b.voltage_v(), b.current_a()),
                    vec![
                        field("Block", b.block.to_string()),
                        field("Voltage", format!("{:.2} V", b.voltage_v())),
                        field("Current", format!("{:.2} A", b.current_a())),
                        field("Temp max", b.temp_max.to_string()),
                        field("Temp min", b.temp_min.to_string()),
                        field("PWM alarm", format!("{} %", b.pwm_alarm)),
                    ],
                ),
                Telemetry::Cells(c) => {
                    let first = c.first_cell();
                    let cells: Vec<String> =
                        c.millivolts.iter().map(|mv| format!("{:.3}", f64::from(*mv) / 1000.0)).collect();
                    (
                        "cells",
                        format!("Pack {:?} cells {}-{}", c.pack, first + 1, first + c.millivolts.len()),
                        vec![field("Cells (V)", cells.join(" "))],
                    )
                }
                Telemetry::Status(s) => (
                    "status",
                    format!(
                        "Odometer {:.1} km, tiltback {}",
                        f64::from(s.odometer) / 1000.0,
                        if s.tiltback >= 200 { "off".to_owned() } else { format!("{} km/h", s.tiltback) }
                    ),
                    vec![
                        field("Odometer", format!("{} m", s.odometer)),
                        field("Auto off", format!("{} s", s.auto_off)),
                        field("Tiltback", format!("{} km/h", s.tiltback)),
                        field("Light", s.light.to_string()),
                    ],
                ),
                Telemetry::Power(p) => (
                    "power",
                    format!("PWM {} %, battery {:.2} A", p.pwm, p.battery_current_a()),
                    vec![
                        field("PWM", format!("{} %", p.pwm)),
                        field("Battery current", format!("{:.2} A", p.battery_current_a())),
                    ],
                ),
                Telemetry::Other(f) => ("other", format!("Frame {:02X}", f.kind()), Vec::new()),
            };
            Described { kind, summary, fields, reply: false }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Assembler;

    #[test]
    fn describes_a_live_frame() {
        let mut assembler = Assembler::new();
        assembler
            .push(&[0x55, 0xAA, 0x19, 0x55, 0, 0, 0, 0x75, 0, 0x06, 0, 0xBE, 0xF0, 0xFA, 0x14, 0xA9, 0, 0x09, 0, 0x18]);
        assembler.push(&[0x5A, 0x5A, 0x5A, 0x5A]);
        let frame = assembler.next_segment().expect("a frame");
        let live = describe(&frame);
        assert_eq!((live.kind, live.reply), ("live", false));
        assert!(live.summary.contains("129.70 V"), "{}", live.summary);
        assert_eq!(live.fields[0], ("Voltage".into(), "129.70 V".into()));
        assert_eq!(&hex(segment_bytes(&frame))[..8], "55 AA 19");
    }
}
