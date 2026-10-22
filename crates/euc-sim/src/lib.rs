//! The simulated controller: memory map, board components, run loop and reports.
//!
//! A [`Machine`] is built from a wheel's `board.toml` (instances, wiring, analog channels) and
//! a firmware image, with any [`euc_core::Cpu`] backend. Components come from a
//! [`euc_core::Registry`]; [`registry`] holds every kind this repo models. The run loop and the
//! access path are ports of the Python reference harness. Explicit reference timing (CPI 1.25
//! and [`ExceptionTiming::REFERENCE`]) preserves its cycle accounting; shipped callers use
//! [`DEFAULT_CPI`] and the default exception costs.
//!
//! With a live [`Plant`] (the electrical plant and the body physics, see [`plant`]) a
//! [`Session`] runs the wheel with its rider, samples [`euc_telemetry::Snapshot`]s for viewers
//! and recordings, and [`scenario`]s script and judge a ride.

mod board;
pub mod bridge;
pub mod compare;
pub mod config;
pub mod cpu;
mod machine;
mod memory;
pub mod plant;
pub mod probes;
pub mod recording;
mod report;
pub mod scenario;
mod session;
pub mod settings;
pub mod sil;
mod system;
mod wheel;

use euc_core::Registry;

pub use board::{Action, BoardLogic, Dwt};
pub use machine::{
    BOARD, BuildError, CpuFactory, DEFAULT_CPI, ExceptionTiming, Machine, MachineConfig, PLANT, exception_name,
};
pub use plant::{Live, Plant, PlantState};
pub use report::Report;
pub use session::{Manual, Session};
pub use system::{Blocker, Diagnostics};
pub use wheel::{LiveError, LoadError, WheelFiles, split_plant};

/// Every component kind this repo models: STM32F1 peripherals, board devices and the
/// simulator's own.
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    euc_stm32f1::register(&mut registry);
    euc_devices::register(&mut registry);
    board::register(&mut registry);
    registry
}
