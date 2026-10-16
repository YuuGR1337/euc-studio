//! Engine backbone shared by every CPU backend, peripheral model and wheel.
//!
//! The simulator is deterministic: one [`Clock`] counts HCLK cycles, a [`Scheduler`] orders
//! every future event by `(cycle, sequence)`, and nothing inside the engine reads wall time.
//! A CPU backend implements [`Cpu`] and reaches memory and peripherals only through a [`Bus`],
//! so the interpreter we ship and the Unicorn test oracle are interchangeable.
//!
//! The run loop that M1 builds on these pieces looks like this:
//!
//! ```text
//! loop {
//!     fire every event due at clock.now()
//!     take the highest-priority pending exception (engine-owned exception unit)
//!     budget = instructions until the next event, capped at the quantum
//!     match cpu.run(bus, budget) { Exit::Budget => .., Exit::ExceptionReturn(..) => .., .. }
//!     clock.advance_by(cycles spent)
//! }
//! ```

mod bus;
mod clock;
mod cpu;
mod regs;
mod sched;

pub use bus::{Access, Bus, BusFault, BusFaultKind, Lane, Size};
pub use clock::Clock;
pub use cpu::{Cpu, Exit, Fault};
pub use regs::CoreRegs;
pub use sched::{EventId, Scheduler};
