//! The BLE side of the OTA path for Begode wheels (service `FFE0`, characteristic `FFE1`).
//!
//! Three layers, each able to refuse a write on its own:
//!
//! - [`WritePolicy`]: whether this process may write to a real wheel at all. The default is
//!   [`WritePolicy::ListenOnly`]; only the documented override
//!   ([`OVERRIDE_ENV`] set to [`override_value`] for that exact wheel) arms it.
//! - [`Guard`]: what may be written, judged on the bytes the wheel sends back. Never `+` `-` `c`
//!   `y`; `!` `@` only once the flasher armed the entry, while application frames prove the app
//!   runs, with the stock firmware's minimum gap; YMODEM only after the bootloader proved itself
//!   (the app's frames stopped and the receiver's own `C`, ACK or NAK arrived since the link came
//!   up), and only whole, valid YMODEM packets.
//! - `central` (feature `central`, macOS): the CoreBluetooth central. It applies the policy, then
//!   the guard, to every write. No default build enables the feature, and no shipped binary uses
//!   it.
//!
//! The simulator's transport runs every write through the same [`Guard`], so the fault campaign
//! exercises the rules a real link would apply.

mod guard;
mod policy;

#[cfg(feature = "central")]
pub mod central;

pub use guard::{
    APP_FRESH, APP_SILENCE, COMMAND_GAP, FORBIDDEN, Guard, LinkMode, Outbound, READ_COMMANDS, Refusal, SERVICE_GAP,
};
pub use policy::{OVERRIDE_ENV, OVERRIDE_PREFIX, WritePolicy, override_value};

/// The wheel's GATT service.
pub const SERVICE: &str = "FFE0";
/// The wheel's one characteristic: notifications out, writes in.
pub const CHARACTERISTIC: &str = "FFE1";
/// Bytes per write: the HM-10 style bridge forwards 20-byte writes.
pub const MTU: usize = 20;
