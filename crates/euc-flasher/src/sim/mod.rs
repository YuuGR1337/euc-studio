//! The simulated wheel the flasher can flash: a behaviour model of the application firmware and
//! of the vendor bootloader on top of `euc-flash-sim`, a BLE link with latency and faults, and a
//! scripted operator. Time is virtual: a 48 KiB flash takes milliseconds of wall time.
//!
//! ```
//! use euc_flasher::sim::{Sim, images};
//! use euc_flasher::{FlashLog, MemoryLedger, Outcome, Plan, Target, flash};
//!
//! let stock = images::synthetic_app(8_000, 1);
//! let wheel = images::sim_profile(&[&stock]);
//! let stock = euc_image::Image::raw(stock);
//! let mut sim = Sim::new(wheel.clone(), &stock, Default::default(), Default::default(), Default::default(), 7)?;
//! let plan = Plan::new(wheel, stock.clone(), stock, Target::Simulator);
//! let report = flash(&plan, &mut sim.transport, &mut sim.operator, &mut MemoryLedger::default(), &mut FlashLog::discard());
//! assert!(matches!(report.outcome, Outcome::Flashed { .. }), "{:?}", report.outcome);
//! # Ok::<(), String>(())
//! ```

mod faults;
pub mod images;
mod wheel;
mod world;

use std::cell::RefCell;
use std::rc::Rc;

use euc_image::Image;
use euc_profile::WheelProfile;

pub use faults::{AppFirmware, FaultPlan, OperatorScript, Rng, WheelScript};
pub use wheel::{Micros, SimWheel, WheelState, WheelStats};
pub use world::{SIM_PEER, SimConfig, SimOperator, SimTransport, SimWorld};

/// A simulated wheel with its transport and operator.
#[derive(Debug, Clone)]
pub struct Sim {
    /// The shared world (clock, wheel, link).
    pub world: Rc<RefCell<SimWorld>>,
    /// The flasher's link.
    pub transport: SimTransport,
    /// The person at the wheel.
    pub operator: SimOperator,
}

impl Sim {
    /// A powered wheel running `factory`, connected, with `faults` armed.
    pub fn new(
        profile: WheelProfile,
        factory: &Image,
        wheel: WheelScript,
        operator: OperatorScript,
        faults: FaultPlan,
        seed: u64,
    ) -> Result<Self, String> {
        let wheel = SimWheel::new(profile, factory, wheel, &faults)?;
        let world = Rc::new(RefCell::new(SimWorld::new(wheel, faults, SimConfig::default(), seed)));
        Ok(Self {
            transport: SimTransport::new(Rc::clone(&world)),
            operator: SimOperator::new(Rc::clone(&world), operator),
            world,
        })
    }
}
