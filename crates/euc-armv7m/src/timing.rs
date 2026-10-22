//! Cycle counts from the Cortex-M3 Technical Reference Manual r2p1 (ARM DDI 0337I), section 18.2
//! "Instruction set summary", Table 18-1.
//!
//! The model is the TRM's, with three choices where the TRM gives a range:
//!
//! - `P`, the pipeline refill after a taken branch or a PC write, is "1 to 3 depending on the
//!   alignment and width of the target instruction"; we use 2.
//! - Long multiplies (3 to 5 or 4 to 7 cycles) and divides (2 to 12) terminate early depending
//!   on the operands; [`long_multiply`] and [`divide`] spread that range over the operand sizes.
//! - "Neighboring load and store single instructions can pipeline their address and data phases":
//!   a load or store single that follows another one takes 1 cycle instead of 2.
//!
//! [`Armv7m::cycles`](crate::Armv7m::cycles) counts the core alone, at zero wait states.
//! [`Flash`] puts the STM32F1 flash interface in front of it (wait states, the two-line prefetch
//! buffer, flash data reads on the D-Code bus), and [`FlashPipe`] turns the core's costs into HCLK
//! time when the engine selects that model. Exception entry and return (12 cycles each at zero
//! wait states) stay the engine's business.

use crate::op::Op;

/// Pipeline refill after a taken branch.
pub const P: u64 = 2;

/// Base cycles of `op` when it does not branch and its timing does not depend on data.
/// Branch refills, register counts of `LDM`/`STM` (1 + N, the N added when they run), and early
/// termination of long multiplies and divides are added when the instruction runs.
pub const fn base(op: &Op) -> u8 {
    match op {
        Op::Load { .. }
        | Op::LoadIdx { .. }
        | Op::LoadReg { .. }
        | Op::LoadLit { .. }
        | Op::Store { .. }
        | Op::StoreIdx { .. }
        | Op::StoreReg { .. }
        | Op::Ldrex { .. }
        | Op::Strex { .. }
        | Op::Table { .. }
        | Op::Mla { .. } => 2,
        // 1 + N with N = 2.
        Op::Dual { .. } | Op::LoadDualLit { .. } => 3,
        // Exceptions: their cost is the engine's exception entry.
        Op::Svc { .. } | Op::Bkpt { .. } | Op::Undefined { .. } | Op::Unpredictable { .. } => 0,
        _ => 1,
    }
}

/// True for the loads and stores of a single register, which pipeline with a neighbour.
pub const fn is_single_transfer(op: &Op) -> bool {
    matches!(
        op,
        Op::Load { .. }
            | Op::LoadIdx { .. }
            | Op::LoadReg { .. }
            | Op::LoadLit { .. }
            | Op::Store { .. }
            | Op::StoreIdx { .. }
            | Op::StoreReg { .. }
    )
}

/// `UMULL`/`SMULL` take 3 to 5 cycles, `UMLAL`/`SMLAL` 4 to 7: the multiplier terminates early
/// when the operands are small. Returns the cycles for operands `a` and `b` (as unsigned or
/// two's complement magnitudes).
pub const fn long_multiply(a: u32, b: u32, signed: bool, accumulate: bool) -> u64 {
    let (wide_a, wide_b) = (wide(a, signed), wide(b, signed));
    let base = 3 + wide_a as u64 + wide_b as u64;
    if accumulate { base + 1 + (wide_a && wide_b) as u64 } else { base }
}

/// True when `x` needs more than 16 bits (as a magnitude when `signed`).
const fn wide(x: u32, signed: bool) -> bool {
    let x = if signed && (x as i32) < 0 { !x } else { x };
    x.leading_zeros() < 16
}

/// `SDIV`/`UDIV` take 2 to 12 cycles, terminating early on the quotient's size: 2 when the
/// divisor exceeds the dividend, up to 12 for a 32-bit quotient.
pub const fn divide(dividend: u32, divisor: u32) -> u64 {
    if divisor == 0 || dividend < divisor {
        return 2;
    }
    let quotient_bits = (divisor.leading_zeros() - dividend.leading_zeros() + 1) as u64;
    2 + (quotient_bits * 10).div_ceil(32)
}

/// The STM32F1 embedded-flash read interface in front of the core (RM0008 rev 21, section 3.3.3
/// "Read interface"), delaying the zero-wait Cortex-M3 TRM costs above.
///
/// - The array reads one 64-bit line at a time in `wait_states + 1` HCLK cycles, and finishes a
///   read once it has started.
/// - The prefetch buffer holds two lines: the line the core executes from and the next one. The
///   controller reads the next line as soon as a block is free: back to back after a miss,
///   otherwise when the core moves into the next line and frees the older block.
/// - A taken branch into the current line or the prefetched next one is served from the buffer.
///   Any other target is a demand read, which replaces a prefetch that has not started.
/// - Data reads from flash (literal pools, `TBB`/`TBH` tables, constants, data flash) use the
///   D-Code bus. They go before a prefetch that has not started and wait for a read in progress.
/// - A fetched line serves an instruction one decode cycle after the read completes. With
///   `wait_states` 0 this reproduces the zero-wait TRM costs, including the refill `P` = 2.
/// - Fetches and loads outside the array (SRAM, peripherals) see no wait states.
///
/// The engine charges exception entry, return and tail-chaining, including their flash fetches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flash {
    /// `FLASH_ACR.LATENCY`: 2 for HCLK above 48 MHz.
    pub wait_states: u8,
    /// First address of the flash array.
    pub base: u32,
    /// Bytes in the flash array.
    pub size: u32,
}

impl Flash {
    /// An STM32F1 at 72 MHz: two wait states over the main flash window (up to 1 MiB at
    /// `0x0800_0000`). Firmware sets `LATENCY` 2 and `PRFTBE` before it raises the clock. The model
    /// applies them from reset, because the engine also runs HCLK at full speed from reset.
    pub const STM32F1_72MHZ: Self = Self::stm32f1(2);

    /// The STM32F1 main flash window with `wait_states` wait states.
    pub const fn stm32f1(wait_states: u8) -> Self {
        Self { wait_states, base: 0x0800_0000, size: 0x10_0000 }
    }

    /// True when `addr` is in the flash array.
    #[inline(always)]
    #[allow(clippy::inline_always)] // once per instruction in the timed loop
    pub const fn contains(&self, addr: u32) -> bool {
        addr.wrapping_sub(self.base) < self.size
    }

    /// HCLK cycles of one line read.
    #[inline(always)]
    #[allow(clippy::inline_always)] // once per instruction in the timed loop
    pub const fn line_cycles(&self) -> u64 {
        self.wait_states as u64 + 1
    }
}

/// No current line in the prefetch buffer.
const NO_LINE: u32 = u32::MAX;

/// [`Flash`] timing state between instructions. Times are HCLK cycles on the model's own clock
/// ([`FlashPipe::now`]), which only instructions advance; the engine adds its own costs outside it.
#[derive(Debug, Clone, Copy)]
pub struct FlashPipe {
    /// When the previous instruction ended.
    t: u64,
    /// The 8-byte line the core executes from (`address >> 3`), or [`NO_LINE`].
    cur: u32,
    /// Earliest start of an instruction in `cur`: its read completed, plus one decode cycle.
    cur_usable: u64,
    /// Start of the read of line `cur + 1`, the prefetch. It can lie ahead of the clock while the
    /// array is busy; a D-Code read requested before it delays it, and a demand miss replaces it.
    pf_start: u64,
    /// When the array finishes its last read other than the prefetch.
    busy: u64,
}

impl Default for FlashPipe {
    fn default() -> Self {
        Self { t: 0, cur: NO_LINE, cur_usable: 0, pf_start: 0, busy: 0 }
    }
}

impl FlashPipe {
    /// The model clock: HCLK cycles the instructions have taken so far.
    pub const fn now(&self) -> u64 {
        self.t
    }

    /// The engine moved PC to `pc` (exception entry, return or tail-chain, a reset) and charged the
    /// switch, including the first fetch: an instruction at `pc` can start at once, and the
    /// controller prefetches the following line back to back.
    pub fn redirect(&mut self, flash: &Flash, pc: u32) {
        if flash.contains(pc) {
            self.cur = pc >> 3;
            self.cur_usable = self.t;
            self.busy = self.t.saturating_sub(1);
            self.pf_start = self.busy;
        } else {
            self.cur = NO_LINE;
        }
    }

    /// When the array can start a read requested at `at`: after its last committed read and after a
    /// prefetch already in progress. The flag is true when a prefetch exists that has not started
    /// by `at`, so the new read goes first.
    fn array_start(&self, flash: &Flash, at: u64) -> (u64, bool) {
        let has_prefetch = self.cur != NO_LINE;
        let started = has_prefetch && self.pf_start <= at;
        let after_prefetch = if started { self.pf_start + flash.line_cycles() } else { 0 };
        (at.max(self.busy).max(after_prefetch), has_prefetch && !started)
    }

    /// The core needs `line`, requested at `at`, and it is neither `cur` nor `cur + 1`.
    fn miss(&mut self, flash: &Flash, line: u32, at: u64) {
        let (start, _) = self.array_start(flash, at);
        let lc = flash.line_cycles();
        self.cur = line;
        self.cur_usable = start + lc + 1;
        self.busy = start + lc;
        self.pf_start = self.busy;
    }

    /// The core moves from `cur` into the prefetched `cur + 1`, fetching from it at `at`. The block
    /// it leaves takes the prefetch of the line after.
    fn advance(&mut self, flash: &Flash, at: u64) {
        let lc = flash.line_cycles();
        let usable = self.pf_start + lc + 1;
        self.cur = self.cur.wrapping_add(1);
        self.cur_usable = usable;
        let freed = at.max(usable - 1);
        self.pf_start = freed.max(self.busy).max(self.pf_start + lc);
    }

    /// The earliest start of an instruction whose last byte is at `last`, fetched sequentially.
    #[inline(always)]
    #[allow(clippy::inline_always)] // once per instruction in the timed loop
    fn fetch(&mut self, flash: &Flash, last: u32) -> u64 {
        let line = last >> 3;
        if line != self.cur {
            if self.cur != NO_LINE && line == self.cur.wrapping_add(1) {
                // The core fetches one cycle before it executes.
                self.advance(flash, self.t.saturating_sub(1));
            } else {
                // Entering flash from elsewhere without a branch: requested as early as a
                // zero-wait fetch would be, so only the wait states show.
                self.miss(flash, line, self.t.saturating_sub(P));
            }
        }
        self.t.max(self.cur_usable)
    }

    /// Accounts for one retired instruction at `pc`, `len` bytes long. It took `cost` zero-wait
    /// cycles, including the refill [`P`] when it moved PC, and made `flash_reads` data reads from
    /// flash. `next` is PC after it. Returns the model clock after it.
    #[inline(always)]
    #[allow(clippy::inline_always)] // once per instruction in the timed loop
    pub fn retire(&mut self, flash: &Flash, pc: u32, len: u32, cost: u64, flash_reads: u32, next: u32) -> u64 {
        let start = if flash.contains(pc) { self.fetch(flash, pc.wrapping_add(len - 1)) } else { self.t };
        let taken = next != pc.wrapping_add(len);
        let core = if taken { cost.saturating_sub(P) } else { cost };
        let mut end = start + core;
        if flash_reads > 0 {
            // The data phases end the instruction's zero-wait cycles; each read takes a line time.
            let reads = u64::from(flash_reads);
            let (from, ahead_of_prefetch) = self.array_start(flash, end.saturating_sub(reads));
            let done = from + reads * flash.line_cycles();
            self.busy = done;
            if ahead_of_prefetch {
                self.pf_start = self.pf_start.max(done);
            }
            end = end.max(done);
        }
        if taken {
            if flash.contains(next) {
                let line = next >> 3;
                if line == self.cur {
                    // Served from the buffer.
                } else if self.cur != NO_LINE && line == self.cur.wrapping_add(1) {
                    self.advance(flash, end);
                } else {
                    self.miss(flash, line, end);
                }
            }
            end += P;
        }
        self.t = end;
        end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divide_spans_the_trm_range() {
        assert_eq!(divide(5, 10), 2);
        assert_eq!(divide(5, 0), 2);
        assert!(divide(u32::MAX, 1) <= 12);
        assert_eq!(divide(u32::MAX, 1), 12);
        assert!(divide(1000, 7) > divide(10, 7));
    }

    #[test]
    fn long_multiply_spans_the_trm_range() {
        assert_eq!(long_multiply(3, 4, false, false), 3);
        assert_eq!(long_multiply(u32::MAX, u32::MAX, false, false), 5);
        assert_eq!(long_multiply(-3_i32 as u32, 7, true, false), 3, "small negative numbers terminate early");
        assert_eq!(long_multiply(u32::MAX, 0x1_0000, false, true), 7);
        assert_eq!(long_multiply(1, 1, false, true), 4);
    }
}
