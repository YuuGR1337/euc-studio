//! The gated OTA flasher (plan section "OTA flasher (v1, gated)"; docs/spec/flasher.md).
//!
//! [`flash`] runs one session over a [`Transport`]: the offline gates (1 image, 2 provenance,
//! 3 dry-run, the recovery image, 7 first flash = stock), the wheel gates (4 identity, 5 wheel
//! state, 6 typed confirmation, 5 again), the bootloader entry (`!` `@`), a YMODEM-1K transfer,
//! the boot check, and on failure the stock restore, then a stop with the SWD guide. Every byte,
//! gate and step goes to a [`FlashLog`] (JSONL).
//!
//! Two transports exist:
//!
//! - [`sim::SimTransport`]: the simulated wheel with an emulated bootloader on `euc-flash-sim`,
//!   fault injection and virtual time. `euc flash --sim` and the fault campaign use it.
//! - `BleTransport` (feature `ble`, off by default): a real wheel over `euc-ble`'s central. No
//!   shipped binary enables it, and its writes stay refused unless the documented override names
//!   the wheel. Gates 2 and 3 refuse every real flash today: our images need signatures that do
//!   not exist yet, and gate 3 needs an emulator ride that is not implemented yet.
//!
//! Both transports judge every write with [`euc_ble::Guard`]: never `+` `-` `c` `y`, `!` `@`
//! only when armed, YMODEM only to a proven bootloader.

#[cfg(feature = "ble")]
pub mod ble;
pub mod campaign;
mod gate;
mod ledger;
mod log;
mod observe;
mod session;
pub mod sim;
mod transport;

pub use gate::{
    GateCheck, GateId, Limits, Status, Target, confirmation, confirmation_text, dry_run, first_flash_stock, image,
    model_name, provenance, recovery_image,
};
pub use ledger::{DryRun, FileLedger, FlashRecord, Ledger, MemoryLedger};
pub use log::{FlashLog, Record};
pub use session::{
    Outcome, Plan, Purpose, Report, Timing, TransferReport, TransferResult, WheelSeen, expected_name, flash, restore,
};
pub use transport::{Inbound, LinkError, LinkKind, Operator, Transport};
