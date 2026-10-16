/// Architectural register state of an ARMv7-M core, the same for every backend.
///
/// The engine works on it between [`Cpu::run`](crate::Cpu::run) calls: the exception unit stacks
/// and unstacks frames, the inspector shows it, save states copy it. Field names follow the ARM
/// Architecture Reference Manual (ARMv7-M, B1.4) so engine code reads like its pseudocode.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreRegs {
    /// R0 to R12.
    pub r: [u32; 13],
    /// Main stack pointer, `SP_main`.
    pub msp: u32,
    /// Process stack pointer, `SP_process`.
    pub psp: u32,
    /// Link register, R14.
    pub lr: u32,
    /// Address of the next instruction, R15, with bit 0 clear.
    pub pc: u32,
    /// APSR, IPSR and EPSR combined.
    pub xpsr: u32,
    /// `PRIMASK.PM`: when set, only NMI and HardFault can preempt.
    pub primask: bool,
    /// `FAULTMASK.FM`: when set, only NMI can preempt.
    pub faultmask: bool,
    /// `BASEPRI`: exceptions at this priority or lower are masked; 0 masks nothing.
    pub basepri: u8,
    /// `CONTROL`: bit 0 `nPRIV`, bit 1 `SPSEL`.
    pub control: u8,
}

impl CoreRegs {
    /// `EPSR.T`, the Thumb bit. ARMv7-M has no ARM state; executing with it clear is a UsageFault.
    pub const XPSR_T: u32 = 1 << 24;
    /// `IPSR`: the active exception number, 0 in thread mode.
    pub const XPSR_IPSR: u32 = 0x1FF;
    /// `CONTROL.SPSEL`: thread mode uses the process stack.
    pub const CONTROL_SPSEL: u8 = 1 << 1;

    /// Register state after a reset that loaded `sp` and `reset_vector` from a vector table.
    ///
    /// Follows `TakeReset()`: `SP_main` gets `sp` with the low two bits cleared, LR is
    /// `0xFFFF_FFFF`, the core is in privileged thread mode on the main stack, and `EPSR.T` comes
    /// from bit 0 of the vector (an even vector faults on the first instruction, as on hardware).
    pub fn at_reset(sp: u32, reset_vector: u32) -> Self {
        Self {
            msp: sp & !3,
            lr: u32::MAX,
            pc: reset_vector & !1,
            xpsr: if reset_vector & 1 == 1 { Self::XPSR_T } else { 0 },
            ..Self::default()
        }
    }

    /// The active exception number from `IPSR`; 0 means thread mode.
    pub const fn ipsr(&self) -> u16 {
        (self.xpsr & Self::XPSR_IPSR) as u16
    }

    /// True while an exception handler runs.
    pub const fn is_handler_mode(&self) -> bool {
        self.ipsr() != 0
    }

    /// True when R13 currently means `SP_process`: thread mode with `CONTROL.SPSEL` set.
    pub const fn uses_psp(&self) -> bool {
        !self.is_handler_mode() && self.control & Self::CONTROL_SPSEL != 0
    }

    /// The active stack pointer (R13).
    pub const fn sp(&self) -> u32 {
        if self.uses_psp() { self.psp } else { self.msp }
    }

    /// Writes the active stack pointer. Bits 1:0 always read as zero on ARMv7-M.
    pub fn set_sp(&mut self, value: u32) {
        let value = value & !3;
        if self.uses_psp() {
            self.psp = value;
        } else {
            self.msp = value;
        }
    }

    /// R0 to R15 by number, with R13 resolved to the active stack pointer.
    ///
    /// # Panics
    ///
    /// If `n > 15`.
    pub const fn reg(&self, n: usize) -> u32 {
        match n {
            0..=12 => self.r[n],
            13 => self.sp(),
            14 => self.lr,
            15 => self.pc,
            _ => panic!("ARMv7-M has 16 core registers"),
        }
    }

    /// Writes R0 to R15 by number. R13 goes to the active stack pointer; R15 is stored as given.
    ///
    /// # Panics
    ///
    /// If `n > 15`.
    pub fn set_reg(&mut self, n: usize, value: u32) {
        match n {
            0..=12 => self.r[n] = value,
            13 => self.set_sp(value),
            14 => self.lr = value,
            15 => self.pc = value,
            _ => panic!("ARMv7-M has 16 core registers"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_state_follows_take_reset() {
        let regs = CoreRegs::at_reset(0x2000_1EEB, 0x0800_322D);
        assert_eq!(regs.msp, 0x2000_1EE8);
        assert_eq!(regs.sp(), 0x2000_1EE8);
        assert_eq!(regs.pc, 0x0800_322C);
        assert_eq!(regs.lr, 0xFFFF_FFFF);
        assert_eq!(regs.xpsr, CoreRegs::XPSR_T);
        assert!(!regs.is_handler_mode());
        assert!(!regs.primask && !regs.faultmask);
    }

    #[test]
    fn even_reset_vector_clears_the_thumb_bit() {
        assert_eq!(CoreRegs::at_reset(0x2000_0000, 0x0800_3000).xpsr & CoreRegs::XPSR_T, 0);
    }

    #[test]
    fn r13_follows_mode_and_spsel() {
        let mut regs = CoreRegs { msp: 0x2000_1000, psp: 0x2000_0800, ..CoreRegs::default() };
        assert_eq!(regs.reg(13), 0x2000_1000);

        regs.control = CoreRegs::CONTROL_SPSEL;
        assert_eq!(regs.reg(13), 0x2000_0800);
        regs.set_reg(13, 0x2000_07F3);
        assert_eq!(regs.psp, 0x2000_07F0, "SP bits 1:0 read as zero");

        // Handler mode always runs on the main stack, whatever SPSEL says.
        regs.xpsr = 37;
        assert_eq!(regs.ipsr(), 37);
        assert_eq!(regs.sp(), 0x2000_1000);
    }

    #[test]
    fn numbered_access_reaches_every_register() {
        let mut regs = CoreRegs::default();
        for n in 0..16 {
            regs.set_reg(n, 0x100 * n as u32 + 4);
        }
        assert_eq!(regs.r[7], 0x704);
        assert_eq!(regs.lr, 0xE04);
        assert_eq!(regs.pc, 0xF04);
        assert_eq!(regs.reg(13), 0xD04);
    }
}
