//! The virtual phone: the firmware's BLE stream decoded into frames and replies, and the
//! commands a phone app may send.
//!
//! Never-send commands (`c` `y`, `!` `@`, `+` `-`: they cut the motor and erase flash, enter the
//! bootloader or power the wheel off) have no id here, so nothing in the studio can build them.

use std::time::Duration;

use euc_proto_begode::{
    AlarmMode, Assembler, Command, LightMode, PedalMode, RollCutoff, SAFE_BYTE_GAP, Safety, Segment, Units, describe,
};
use serde::Serialize;

/// One decoded line in the phone's log.
#[derive(Debug, Clone, Serialize)]
pub struct PhoneFrame {
    pub id: u64,
    /// Simulated seconds of the segment's last byte.
    pub t: f64,
    pub kind: &'static str,
    pub summary: String,
    pub fields: Vec<(String, String)>,
    pub hex: String,
    /// A reply to a phone command (`V`, `N`, speed alarms), worth a timeline entry.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub reply: bool,
}

/// Turns the BLE byte stream into [`PhoneFrame`]s.
#[derive(Default)]
pub struct PhoneDecoder {
    assembler: Assembler,
    next_id: u64,
}

impl PhoneDecoder {
    /// Feeds bytes (with their simulated time) and appends every completed segment to `out`.
    pub fn push(&mut self, bytes: &[(f64, u8)], out: &mut Vec<PhoneFrame>) {
        let Some(&(t, _)) = bytes.last() else { return };
        let raw: Vec<u8> = bytes.iter().map(|&(_, b)| b).collect();
        self.assembler.push(&raw);
        while let Some(segment) = self.assembler.next_segment() {
            self.next_id += 1;
            out.push(phone_frame(self.next_id, t, &segment));
        }
    }
}

/// The phone's line for a segment: the shared describer (`euc_proto_begode::describe`, which
/// recordings use too), with the segment's id, time and bytes.
fn phone_frame(id: u64, t: f64, segment: &Segment) -> PhoneFrame {
    let d = describe::describe(segment);
    let hex = describe::hex(describe::segment_bytes(segment));
    PhoneFrame { id, t, kind: d.kind, summary: d.summary, fields: d.fields, hex, reply: d.reply }
}

/// A command the phone can send, for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct CommandSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub pattern: &'static str,
    pub safety: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<Param>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<Choice>,
    /// The wheel ignores it above this speed, m/s.
    #[serde(rename = "maxSpeed", skip_serializing_if = "Option::is_none")]
    pub max_speed: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Param {
    pub min: u8,
    pub max: u8,
    pub default: u8,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Choice {
    pub value: u8,
    pub label: &'static str,
}

const fn choice(value: u8, label: &'static str) -> Choice {
    Choice { value, label }
}

/// Every id the phone accepts, with its parameter shape. Never-send commands are absent.
const IDS: [(&str, Option<Param>, &[Choice]); 20] = [
    ("horn", None, &[]),
    ("light", None, &[choice(0, "Off"), choice(1, "On"), choice(2, "Strobe")]),
    ("version", None, &[]),
    ("name", None, &[]),
    ("pedal_mode", None, &[choice(0, "Soft"), choice(1, "Medium"), choice(2, "Hard")]),
    (
        "speed_alarms",
        None,
        &[choice(0, "All on"), choice(1, "Level 1 off"), choice(2, "Level 2 off"), choice(3, "Level 3 off")],
    ),
    ("roll_cutoff", None, &[choice(0, "Low"), choice(1, "Medium"), choice(2, "High")]),
    ("units", None, &[choice(0, "Metric"), choice(1, "Imperial")]),
    ("pwm_tiltback", None, &[choice(0, "Off"), choice(1, "On")]),
    ("tiltback_off", None, &[]),
    ("tiltback", Some(Param { min: 0, max: 99, default: 35 }), &[]),
    ("pwm_alarm", Some(Param { min: 50, max: 90, default: 80 }), &[]),
    ("beeper_volume", Some(Param { min: 0, max: 9, default: 5 }), &[]),
    ("wm", Some(Param { min: 0, max: 1, default: 0 }), &[]),
    ("turn_compensation", Some(Param { min: 0, max: 9, default: 3 }), &[]),
    ("pedal_trim", Some(Param { min: 0, max: 9, default: 4 }), &[]),
    ("wc", Some(Param { min: 0, max: 9, default: 0 }), &[]),
    ("angle_limit", Some(Param { min: 0, max: 9, default: 0 }), &[]),
    ("debug_dump", None, &[]),
    ("cancel_sequence", None, &[]),
];

/// The command `id` with `value`, refusing anything outside the catalogue.
pub fn command(id: &str, value: Option<u8>) -> Result<Command, String> {
    let need = |v: Option<u8>| v.ok_or_else(|| format!("{id} needs a value"));
    let pick = |v: Option<u8>, name: &str| -> Result<u8, String> { v.ok_or_else(|| format!("{name} needs a choice")) };
    let bad = || format!("{id}: value {} is not one of its choices", value.unwrap_or_default());
    Ok(match id {
        "horn" => Command::Horn,
        "light" => Command::Light(LightMode::from_value(pick(value, id)?).ok_or_else(bad)?),
        "version" => Command::Version,
        "name" => Command::Name,
        "pedal_mode" => Command::PedalMode(PedalMode::from_value(pick(value, id)?).ok_or_else(bad)?),
        "speed_alarms" => Command::SpeedAlarms(AlarmMode::from_value(pick(value, id)?).ok_or_else(bad)?),
        "roll_cutoff" => Command::RollCutoff(RollCutoff::from_value(pick(value, id)?).ok_or_else(bad)?),
        "units" => Command::Units(Units::from_value(pick(value, id)?).ok_or_else(bad)?),
        "pwm_tiltback" => Command::PwmTiltback(pick(value, id)? != 0),
        "tiltback_off" => Command::TiltbackOff,
        "tiltback" => Command::Tiltback(need(value)?),
        "pwm_alarm" => Command::PwmAlarm(need(value)?),
        "beeper_volume" => Command::BeeperVolume(need(value)?),
        "wm" => Command::Wm(need(value)?),
        "turn_compensation" => Command::TurnCompensation(need(value)?),
        "pedal_trim" => Command::PedalTrim(need(value)?),
        "wc" => Command::Wc(need(value)?),
        "angle_limit" => Command::AngleLimit(need(value)?),
        "debug_dump" => Command::DebugDump,
        "cancel_sequence" => Command::CancelSequence,
        other => return Err(format!("\"{other}\" is not a command the phone can send")),
    })
}

/// The bytes to send for `command`, each with the simulated seconds to wait before it. Refuses
/// never-send commands even if one is constructed elsewhere.
pub fn encode(command: &Command) -> Result<Vec<(f64, u8)>, String> {
    if command.safety() == Safety::Never {
        return Err(format!("{} is never sent from an app", command.info().name));
    }
    let steps = command.encode().map_err(|e| e.to_string())?;
    Ok(steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let gap = if i == 0 { Duration::ZERO } else { step.min_gap.max(SAFE_BYTE_GAP) + SAFE_BYTE_GAP };
            (gap.as_secs_f64(), step.byte)
        })
        .collect())
}

/// The catalogue for the UI.
pub fn catalogue() -> Vec<CommandSpec> {
    IDS.iter()
        .filter_map(|&(id, param, choices)| {
            let value = param.map(|p| p.default).or_else(|| choices.first().map(|c| c.value));
            let info = command(id, value).ok()?.info();
            let safety = match info.safety {
                Safety::Read => "read",
                Safety::Comfort => "comfort",
                Safety::Ride => "ride",
                Safety::Debug => "debug",
                Safety::Never => return None,
            };
            Some(CommandSpec {
                id,
                name: info.name,
                pattern: info.pattern,
                safety,
                param,
                choices: choices.to_vec(),
                max_speed: info.max_speed.map(|v| f64::from(v) / 100.0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_has_no_never_send_command() {
        let ids: Vec<&str> = catalogue().iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), IDS.len());
        for c in Command::ALL {
            if c.safety() == Safety::Never {
                assert!(encode(&c).is_err(), "{c:?} must be refused");
                assert!(!ids.contains(&c.info().name));
            }
        }
        for c in catalogue() {
            let bytes =
                encode(&command(c.id, c.param.map(|p| p.default).or(c.choices.first().map(|x| x.value))).unwrap())
                    .unwrap();
            assert!(bytes.iter().all(|&(_, b)| !b"cy!@+-".contains(&b)), "{} sends a never-send byte", c.id);
        }
    }

    #[test]
    fn unknown_or_out_of_range_commands_are_refused() {
        assert!(command("calibrate_imu", None).is_err());
        assert!(command("enter_bootloader", None).is_err());
        assert!(command("light", Some(7)).is_err());
        assert!(encode(&command("pwm_alarm", Some(95)).unwrap()).is_err());
    }

    #[test]
    fn multi_byte_commands_wait_between_bytes() {
        let steps = encode(&command("tiltback", Some(35)).unwrap()).unwrap();
        assert_eq!(steps.iter().map(|&(_, b)| b).collect::<Vec<_>>(), b"WY35");
        assert_eq!(steps[0].0, 0.0);
        assert!(steps[1..].iter().all(|&(gap, _)| gap >= 0.04));
    }

    #[test]
    fn decodes_frames_and_text() {
        let mut d = PhoneDecoder::default();
        let frame =
            [0x55, 0xAA, 0x19, 0x64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x00, 0x18, 0x5A, 0x5A, 0x5A, 0x5A];
        let mut bytes: Vec<(f64, u8)> = b"GW1825020".iter().map(|&b| (1.0, b)).collect();
        bytes.extend(frame.iter().map(|&b| (1.1, b)));
        let mut out = Vec::new();
        d.push(&bytes, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].summary, "Version GW1825020");
        assert_eq!(out[1].kind, "live");
        assert!(out[1].summary.contains("130.00 V"), "{}", out[1].summary);
    }

    /// The phone shows what it showed before it used the shared describer: the real idle capture
    /// (tests/data/begode-extreme), after a version reply, line for line. Golden values from the
    /// app's own describer, which matched the shared one on all 381 segments when it was removed.
    #[test]
    fn the_real_capture_reads_as_before() {
        let capture = include_str!("../../../../tests/data/begode-extreme/idle-2026-10-15.hex");
        let mut d = PhoneDecoder::default();
        let mut out = Vec::new();
        d.push(&b"GW1825020".map(|b| (1.0, b)), &mut out);
        for line in capture.lines() {
            let mut parts = line.split_whitespace();
            let t: f64 = parts.next().unwrap().parse().unwrap();
            let bytes: Vec<(f64, u8)> = parts.map(|h| (t, u8::from_str_radix(h, 16).unwrap())).collect();
            d.push(&bytes, &mut out);
        }
        let count = |kind: &str| out.iter().filter(|f| f.kind == kind).count();
        assert_eq!(out.len(), 381);
        assert_eq!(["live", "bms", "cells", "status", "power", "text"].map(count), [76, 76, 76, 76, 76, 1]);
        fn line(f: &PhoneFrame) -> (&str, &str, bool) {
            (f.kind, &f.summary, f.reply)
        }
        let fields = |f: &PhoneFrame| f.fields.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", ");
        assert_eq!(line(&out[0]), ("text", "Version GW1825020", true));
        assert_eq!(out[0].hex, "47 57 31 38 32 35 30 32 30");
        assert_eq!(line(&out[1]), ("live", "0.0 km/h, 129.70 V, 1.90 A", false));
        assert_eq!(
            fields(&out[1]),
            "Voltage=129.70 V, Speed=0.00 km/h, Range=117 km, Trip=6 m, Phase current=1.90 A, \
             Board temp=25.3 °C, Beeper volume=9"
        );
        assert_eq!(out[1].hex, "55 AA 19 55 00 00 00 75 00 06 00 BE F0 FA 14 A9 00 09 00 18 5A 5A 5A 5A");
        assert_eq!(line(&out[2]), ("bms", "BMS block 0: 129.70 V, 0.00 A", false));
        assert_eq!(
            fields(&out[2]),
            "Block=0, Voltage=129.70 V, Current=0.00 A, Temp max=23, Temp min=22, PWM alarm=80 %"
        );
        assert_eq!(line(&out[3]), ("cells", "Pack B cells 1-8", false));
        assert_eq!(fields(&out[3]), "Cells (V)=4.074 4.074 4.075 4.075 4.075 4.075 4.074 4.075");
        assert_eq!(line(&out[4]), ("status", "Odometer 1468.7 km, tiltback 69 km/h", false));
        assert_eq!(fields(&out[4]), "Odometer=1468670 m, Auto off=7079 s, Tiltback=69 km/h, Light=0");
        assert_eq!(line(&out[5]), ("power", "PWM -1 %, battery -1.53 A", false));
        assert_eq!(fields(&out[5]), "PWM=-1 %, Battery current=-1.53 A");
        assert_eq!(out[5].hex, "55 AA FF 67 00 00 00 00 FF FF 00 00 00 00 00 00 00 00 07 18 5A 5A 5A 5A");
        assert_eq!((out[1].t, out[2].t, out[5].id), (1.673, 1.674, 6), "the time of each segment's last byte");
    }
}
