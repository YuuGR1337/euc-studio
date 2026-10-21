use euc_image::Report;
use euc_profile::Sha256;
use serde::Serialize;

/// Current boot permission. Recovery never implies retention of an old application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BootState {
    /// The protected simulated bootloader can accept a new complete transaction.
    RecoverableBootloader,
    /// A complete stored image passed verification and the final boot gate.
    Application {
        /// Number of committed application bytes.
        image_len: usize,
        /// Digest of those bytes, checked again on a power cycle.
        sha256: Sha256,
    },
}

/// Transaction phase, independent of any vendor wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// One operation per application erase page.
    Erase,
    /// One operation per ordered image chunk.
    Write,
    /// One complete-image verification operation.
    Verify,
    /// One final boot gate operation.
    Reboot,
}

/// Deterministic effect of a fault rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FaultEffect {
    /// Fail before mutation and retry within the configured bound.
    Transient,
    /// Apply half an erase/write operation, or interrupt verify/reboot before completion.
    Interrupt,
    /// Write the chunk, then flip bit zero of its first byte to model a storage error.
    CorruptWrite,
}

/// One deterministic fault rule; overlapping rules are rejected before erasure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Fault {
    /// Phase to affect.
    pub phase: Phase,
    /// Zero-based page/chunk index; zero for verify/reboot.
    pub operation: usize,
    /// One-based attempt, or every attempt when absent.
    pub attempt: Option<u8>,
    /// Effect to apply; corruption is valid only for writes.
    pub effect: FaultEffect,
}

/// Transaction parameters with finite per-operation limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Write chunk size in bytes, from one byte through the application slot size.
    pub chunk_size: usize,
    /// Total attempts per operation, including the first; must be in `1..=32`.
    pub max_attempts: u8,
}

impl Default for Options {
    fn default() -> Self {
        Self { chunk_size: 256, max_attempts: 3 }
    }
}

/// An exact attempted operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Step {
    /// Transaction phase.
    pub phase: Phase,
    /// Zero-based phase-local operation index.
    pub operation: usize,
    /// One-based attempt number.
    pub attempt: u8,
}

/// A fault rule that actually triggered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TriggeredFault {
    /// Exact point of injection.
    pub step: Step,
    /// Injected effect.
    pub effect: FaultEffect,
}

/// Reason for refusing an update before any state or storage change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Rejection {
    /// See the input image gate report for details.
    ImageGate,
    /// Chunk size or retry limit is outside its documented bounds.
    Options,
    /// This rule has invalid coordinates, an invalid effect, or overlaps an earlier rule.
    FaultPlan {
        /// Zero-based index in the caller's fault list.
        index: usize,
    },
}

/// Structured end of a transaction. Failure after admission leaves the bootloader active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Outcome {
    /// The complete new application passed the final boot gate.
    Booted,
    /// Preflight refused the request and preserved the prior state.
    Rejected {
        /// Why preflight refused it.
        reason: Rejection,
    },
    /// An interruption ended the transaction without automatic retry.
    Interrupted {
        /// Exact interruption point.
        step: Step,
    },
    /// Transient faults consumed the operation's attempt limit.
    RetryLimit {
        /// Last attempted operation.
        step: Step,
    },
    /// Stored bytes, erased tail, or the stored-image gate failed.
    VerificationFailed {
        /// Gate that found the failure.
        phase: Phase,
    },
}

/// Total operation attempts, including transient failures and interruptions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Attempts {
    /// Erase page attempts.
    pub erase: usize,
    /// Write chunk attempts.
    pub write: usize,
    /// Full-image verification attempts.
    pub verify: usize,
    /// Final boot gate attempts.
    pub reboot: usize,
}

impl Attempts {
    pub(crate) fn record(&mut self, phase: Phase) {
        let count = match phase {
            Phase::Erase => &mut self.erase,
            Phase::Write => &mut self.write,
            Phase::Verify => &mut self.verify,
            Phase::Reboot => &mut self.reboot,
        };
        *count += 1;
    }
}

/// A transaction report without firmware bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FlashReport {
    /// How the transaction ended.
    pub outcome: Outcome,
    /// Final boot permission.
    pub state: BootState,
    /// Existing image gate applied before any mutation.
    pub input_gate: Report,
    /// Most recent stored-image gate, if verification reached it.
    pub stored_gate: Option<Report>,
    /// Operation attempt counts by phase.
    pub attempts: Attempts,
    /// Injected faults in execution order.
    pub faults: Vec<TriggeredFault>,
    /// Programmed bytes, including a partial interrupted chunk; not proof of valid content.
    pub written_bytes: usize,
}
