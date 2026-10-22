//! The CPU backend a machine runs the firmware on.
//!
//! [`Backend::Own`] is our Cortex-M3 interpreter (`euc-armv7m`): every build has it and it is
//! the default. [`Backend::Unicorn`] is QEMU's core through `euc-unicorn`, GPL-2.0, a development
//! and CI oracle only: it exists in builds with this crate's `unicorn` feature, which no shipped
//! build enables.

use std::fmt;
use std::str::FromStr;

use crate::{CpuFactory, ExceptionTiming};

/// How the engine times the firmware's instructions (see `docs/models/cpu-timing.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// A fixed cost per instruction, `MachineConfig::cpi` HCLK cycles (stage A). Every backend
    /// supports it.
    Fixed,
    /// Our core's per-instruction model (stage B): Cortex-M3 TRM costs behind the STM32F1 flash
    /// interface with this many wait states and its prefetch buffer (`euc_armv7m::Flash`). Our core
    /// only; Unicorn has no timing model.
    Flash {
        /// `FLASH_ACR.LATENCY` (2 at 72 MHz).
        wait_states: u8,
    },
}

/// The timing the shipped engine uses.
pub const DEFAULT_TIMING: Timing = Timing::Fixed;

impl Timing {
    /// `fixed`, `flash` (two wait states) or `flash-wN`, for reports and recordings.
    pub fn name(self) -> String {
        match self {
            Self::Fixed => "fixed".to_owned(),
            Self::Flash { wait_states: 2 } => "flash".to_owned(),
            Self::Flash { wait_states } => format!("flash-w{wait_states}"),
        }
    }

    /// The exception switch costs that go with this timing: 12 + W cycles for entry and return and
    /// 6 + W for a tail-chain, W = 2 for [`Timing::Fixed`].
    pub const fn exception_timing(self) -> ExceptionTiming {
        match self {
            Self::Fixed => ExceptionTiming::with_flash_wait_states(2),
            Self::Flash { wait_states } => ExceptionTiming::with_flash_wait_states(wait_states),
        }
    }
}

impl Default for Timing {
    fn default() -> Self {
        DEFAULT_TIMING
    }
}

impl fmt::Display for Timing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

impl FromStr for Timing {
    type Err = String;

    /// `fixed`, `flash` or `flash-wN` (N wait states, 0 to 7).
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let bad = || format!("unknown timing `{text}` (fixed, flash, flash-w0 ... flash-w7)");
        match text {
            "fixed" => Ok(Self::Fixed),
            "flash" => Ok(Self::Flash { wait_states: 2 }),
            other => {
                let wait_states: u8 = other.strip_prefix("flash-w").and_then(|w| w.parse().ok()).ok_or_else(bad)?;
                if wait_states > 7 {
                    return Err(bad());
                }
                Ok(Self::Flash { wait_states })
            }
        }
    }
}

/// Which CPU core runs the firmware.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Backend {
    /// Our own interpreter: exact instruction budgets, verified against Unicorn.
    #[default]
    Own,
    /// Unicorn (builds with the `unicorn` feature). With `exact` every quantum ends after
    /// exactly its budget, as in the Python reference harness and on [`Backend::Own`]; without
    /// it, at the next translated-block boundary (faster, slightly different quanta).
    Unicorn {
        /// End quanta on the exact budget.
        exact: bool,
    },
}

impl Backend {
    /// `own` or `unicorn`, for reports and status bars.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Unicorn { .. } => "unicorn",
        }
    }

    /// True when this build has the backend.
    pub const fn available(self) -> bool {
        matches!(self, Self::Own) || cfg!(feature = "unicorn")
    }

    /// The timing this backend uses unless told otherwise: [`DEFAULT_TIMING`] on our core,
    /// [`Timing::Fixed`] on Unicorn, which has no timing model.
    pub const fn default_timing(self) -> Timing {
        match self {
            Self::Own => DEFAULT_TIMING,
            Self::Unicorn { .. } => Timing::Fixed,
        }
    }

    /// A factory that builds the backend with its [default timing](Self::default_timing).
    pub fn factory(self) -> Result<Box<CpuFactory<'static>>, String> {
        self.factory_with(self.default_timing())
    }

    /// A factory that builds the backend for a machine's memory layout with `timing`, or why this
    /// build cannot.
    pub fn factory_with(self, timing: Timing) -> Result<Box<CpuFactory<'static>>, String> {
        match self {
            Self::Own => Ok(Box::new(move |regions| {
                // SAFETY: `Machine` calls the factory with regions whose memory it owns in boxed
                // buffers that never move and outlive the CPU it stores next to them, and touches
                // that memory during a run only from inside the run's bus callbacks.
                #[allow(unsafe_code)]
                let mut cpu = unsafe { euc_armv7m::Armv7m::new(regions) }.map_err(|e| e.to_string())?;
                if let Timing::Flash { wait_states } = timing {
                    cpu.set_flash(Some(euc_armv7m::Flash::stm32f1(wait_states)));
                }
                Ok(Box::new(cpu) as Box<dyn euc_core::Cpu>)
            })),
            Self::Unicorn { .. } if timing != Timing::Fixed => {
                Err(format!("the Unicorn backend has no timing model: use the fixed timing, not `{timing}`"))
            }
            #[cfg(feature = "unicorn")]
            Self::Unicorn { exact } => Ok(unicorn(exact)),
            #[cfg(not(feature = "unicorn"))]
            Self::Unicorn { .. } => Err("this build has no Unicorn backend: it is GPL-2.0 and for development only \
                 (build with the `unicorn` feature), and `--cpu own` needs nothing extra"
                .to_owned()),
        }
    }
}

#[cfg(feature = "unicorn")]
fn unicorn(exact: bool) -> Box<CpuFactory<'static>> {
    use euc_unicorn::{Quantum, UnicornCpu};
    let quantum = if exact { Quantum::Exact } else { Quantum::Block };
    Box::new(move |regions| {
        // SAFETY: as for `Backend::Own` above.
        #[allow(unsafe_code)]
        let cpu = unsafe { UnicornCpu::with_quantum(regions, quantum) }.map_err(|e| e.to_string())?;
        Ok(Box::new(cpu) as Box<dyn euc_core::Cpu>)
    })
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Backend {
    type Err = String;

    /// `own` or `unicorn` (block quanta).
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "own" => Ok(Self::Own),
            "unicorn" => Ok(Self::Unicorn { exact: false }),
            other => Err(format!("unknown CPU backend `{other}` (own, unicorn)")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_parsing() {
        assert_eq!("own".parse(), Ok(Backend::Own));
        assert_eq!("unicorn".parse(), Ok(Backend::Unicorn { exact: false }));
        assert!("qemu".parse::<Backend>().is_err());
        assert_eq!(Backend::default().to_string(), "own");
        assert!(Backend::Own.available());
        assert_eq!(Backend::Unicorn { exact: true }.available(), cfg!(feature = "unicorn"));
        assert!(Backend::Own.factory().is_ok());
    }

    #[test]
    fn timing_names_parse_back() {
        for timing in [Timing::Fixed, Timing::Flash { wait_states: 2 }, Timing::Flash { wait_states: 0 }] {
            assert_eq!(timing.name().parse(), Ok(timing));
        }
        assert_eq!("flash-w2".parse(), Ok(Timing::Flash { wait_states: 2 }));
        assert!("flash-w8".parse::<Timing>().is_err());
        assert!("cpi".parse::<Timing>().is_err());
        assert_eq!(Timing::Flash { wait_states: 3 }.exception_timing().entry, 15);
        assert!(Backend::Unicorn { exact: true }.factory_with(Timing::Flash { wait_states: 2 }).is_err());
        assert_eq!(Backend::Unicorn { exact: false }.default_timing(), Timing::Fixed);
    }
}
