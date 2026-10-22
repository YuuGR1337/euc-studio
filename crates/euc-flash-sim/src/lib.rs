//! Software-only, vendor-neutral application-slot flash transactions.
//!
//! The vendor OTA wire protocol is UNKNOWN. This crate defines no real transport, service
//! commands, or vendor bootloader implementation. It models protected regions, bounded retries,
//! deterministic faults, and a complete-image boot gate. See `docs/spec/flash-simulation.md`.
//!
//! A single slot can lose its old application during erasure. A recoverable bootloader means
//! another complete simulated transaction is possible; it does not mean rollback is possible.

mod regions;
mod report;

use euc_image::{Image, Report, validate};
use euc_profile::WheelProfile;

pub use regions::{Geometry, Regions, SetupError};
pub use report::{
    Attempts, BootState, Fault, FaultEffect, FlashReport, Options, Outcome, Phase, Rejection, Step, TriggeredFault,
};

/// In-memory storage and abstract boot permission for one application slot.
///
/// All mutation is private and restricted to the checked application range. Boot permission
/// is model metadata, not a claim about the persistence scheme of a physical bootloader.
#[derive(Debug)]
pub struct FlashSim {
    wheel: WheelProfile,
    geometry: Geometry,
    regions: Regions,
    storage: Vec<u8>,
    state: BootState,
}

impl FlashSim {
    /// Creates erased synthetic storage in recoverable bootloader state.
    pub fn new(wheel: WheelProfile, geometry: Geometry) -> Result<Self, SetupError> {
        let regions = geometry.regions(&wheel)?;
        Ok(Self {
            wheel,
            geometry,
            regions,
            storage: vec![0xFF; geometry.flash_size as usize],
            state: BootState::RecoverableBootloader,
        })
    }

    /// Checked storage regions; their ends are exclusive.
    pub fn regions(&self) -> &Regions {
        &self.regions
    }

    /// Current boot permission.
    pub fn state(&self) -> &BootState {
        &self.state
    }

    /// Reads synthetic storage. Out-of-bounds addresses and length overflow return `None`.
    pub fn read(&self, address: u32, len: usize) -> Option<&[u8]> {
        let offset = address.checked_sub(self.regions.flash.start)? as usize;
        self.storage.get(offset..offset.checked_add(len)?)
    }

    /// Rechecks a committed application or keeps the recoverable bootloader active.
    ///
    /// An interrupted transaction never acquires boot permission, even if its bytes look valid.
    /// This models reliable commit metadata surviving a power cycle, not a physical journal.
    pub fn power_cycle(&mut self) -> BootState {
        if let BootState::Application { image_len, sha256 } = self.state {
            let app = self.application();
            let gate = validate(&Image::raw(app[..image_len].to_vec()), &self.wheel);
            if !gate.passed || gate.sha256 != sha256 || !app[image_len..].iter().all(|&byte| byte == 0xFF) {
                self.state = BootState::RecoverableBootloader;
            }
        }
        self.state.clone()
    }

    /// Attempts a complete application update with deterministic fault injection.
    ///
    /// Preflight rejection preserves the previous state. Every admitted failure leaves an
    /// explicit recoverable bootloader. Recovery starts a fresh transaction, including erasure.
    pub fn update(&mut self, image: &Image, options: Options, faults: &[Fault]) -> FlashReport {
        let mut report = FlashReport {
            outcome: Outcome::Booted,
            state: self.state.clone(),
            input_gate: validate(image, &self.wheel),
            stored_gate: None,
            attempts: Attempts::default(),
            faults: Vec::new(),
            written_bytes: 0,
        };
        if let Some(reason) = self.rejection(image, options, faults, &report.input_gate) {
            report.outcome = Outcome::Rejected { reason };
            return report;
        }
        // Invalidate boot permission before the first destructive operation. Never boot a partial slot.
        self.state = BootState::RecoverableBootloader;
        report.outcome = match self.execute(image, options, faults, &mut report) {
            Ok(()) => Outcome::Booted,
            Err(outcome) => outcome,
        };
        report.state = self.state.clone();
        report
    }

    fn rejection(&self, image: &Image, options: Options, faults: &[Fault], gate: &Report) -> Option<Rejection> {
        if !gate.passed {
            return Some(Rejection::ImageGate);
        }
        if options.chunk_size == 0
            || options.chunk_size > self.application().len()
            || !(1..=32).contains(&options.max_attempts)
        {
            return Some(Rejection::Options);
        }
        for (index, fault) in faults.iter().enumerate() {
            let operations = match fault.phase {
                Phase::Erase => self.application().len() / self.geometry.page_size as usize,
                Phase::Write => image.bytes().len().div_ceil(options.chunk_size),
                Phase::Verify | Phase::Reboot => 1,
            };
            let overlaps = faults[..index].iter().any(|other| {
                other.phase == fault.phase
                    && other.operation == fault.operation
                    && (other.attempt.is_none() || fault.attempt.is_none() || other.attempt == fault.attempt)
            });
            if fault.operation >= operations
                || fault.attempt.is_some_and(|attempt| attempt == 0 || attempt > options.max_attempts)
                || (fault.effect == FaultEffect::CorruptWrite && fault.phase != Phase::Write)
                || overlaps
            {
                return Some(Rejection::FaultPlan { index });
            }
        }
        None
    }

    fn execute(
        &mut self,
        image: &Image,
        options: Options,
        faults: &[Fault],
        report: &mut FlashReport,
    ) -> Result<(), Outcome> {
        let page_size = self.geometry.page_size as usize;
        for operation in 0..self.application().len() / page_size {
            let step = attempt(Phase::Erase, operation, options, faults, report)?;
            let len = if step.interrupted { page_size.div_ceil(2) } else { page_size };
            let start = operation * page_size;
            self.application_mut()[start..start + len].fill(0xFF);
            step.finish()?;
        }
        for (operation, chunk) in image.bytes().chunks(options.chunk_size).enumerate() {
            let step = attempt(Phase::Write, operation, options, faults, report)?;
            let len = if step.interrupted { chunk.len().div_ceil(2) } else { chunk.len() };
            let start = operation * options.chunk_size;
            // Erasure precedes every transaction; programming follows NOR's one-to-zero rule.
            let target = &mut self.application_mut()[start..start + len];
            for (stored, &byte) in target.iter_mut().zip(&chunk[..len]) {
                *stored &= byte;
            }
            if step.corrupted {
                target[0] ^= 1; // Deliberate storage error, not a modeled flash programming transition.
            }
            report.written_bytes += len;
            step.finish()?;
        }
        for phase in [Phase::Verify, Phase::Reboot] {
            attempt(phase, 0, options, faults, report)?.finish()?;
            let (gate, complete) = self.stored_gate(image);
            let passed = gate.passed && complete;
            report.stored_gate = Some(gate);
            if !passed {
                return Err(Outcome::VerificationFailed { phase });
            }
        }
        self.state = BootState::Application { image_len: image.bytes().len(), sha256: report.input_gate.sha256 };
        Ok(())
    }

    fn stored_gate(&self, image: &Image) -> (Report, bool) {
        let (stored, tail) = self.application().split_at(image.bytes().len());
        let complete = stored == image.bytes() && tail.iter().all(|&byte| byte == 0xFF);
        (validate(&Image::raw(stored.to_vec()), &self.wheel), complete)
    }

    fn application(&self) -> &[u8] {
        let start = (self.regions.application.start - self.regions.flash.start) as usize;
        &self.storage[start..start + self.wheel.board.app.max_size as usize]
    }

    fn application_mut(&mut self) -> &mut [u8] {
        let start = (self.regions.application.start - self.regions.flash.start) as usize;
        &mut self.storage[start..start + self.wheel.board.app.max_size as usize]
    }
}

struct Operation {
    step: Step,
    interrupted: bool,
    corrupted: bool,
}

impl Operation {
    fn finish(self) -> Result<(), Outcome> {
        if self.interrupted { Err(Outcome::Interrupted { step: self.step }) } else { Ok(()) }
    }
}

fn attempt(
    phase: Phase,
    operation: usize,
    options: Options,
    faults: &[Fault],
    report: &mut FlashReport,
) -> Result<Operation, Outcome> {
    for attempt in 1..=options.max_attempts {
        let step = Step { phase, operation, attempt };
        report.attempts.record(phase);
        let effect = faults
            .iter()
            .find(|fault| {
                fault.phase == phase
                    && fault.operation == operation
                    && fault.attempt.is_none_or(|wanted| wanted == attempt)
            })
            .map(|fault| fault.effect);
        if let Some(effect) = effect {
            report.faults.push(TriggeredFault { step, effect });
        }
        if effect == Some(FaultEffect::Transient) {
            if attempt == options.max_attempts {
                return Err(Outcome::RetryLimit { step });
            }
            continue;
        }
        return Ok(Operation {
            step,
            interrupted: effect == Some(FaultEffect::Interrupt),
            corrupted: effect == Some(FaultEffect::CorruptWrite),
        });
    }
    // Options are validated before admission, so the loop always executes at least once.
    unreachable!("validated attempt limit is nonzero")
}
