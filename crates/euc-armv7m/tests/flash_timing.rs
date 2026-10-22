//! The flash timing model (`euc_armv7m::Flash`, `Cpu::model_cycles`, `Cpu::run_for`): zero wait
//! states reproduce the TRM costs exactly, two wait states delay misses, flash data reads and
//! fetch-bound 32-bit code, and the result never depends on how a program is split into runs.
//!
//! Encodings come from `llvm-mc -triple=thumbv7m-none-eabi -show-encoding`.

#![allow(unsafe_code)]

use std::ptr::NonNull;

use euc_armv7m::{Armv7m, Flash};
use euc_core::{Access, Bus, BusFault, BusFaultKind, CoreRegs, Cpu, Exit, Region, RegionKind, Size};

const FLASH: u32 = 0x0800_0000;
const RAM: u32 = 0x2000_0000;
const SIZE: u32 = 0x1_0000;

const NOP: u16 = 0xBF00;
const SUBS_R0_1: u16 = 0x3801;
/// `bne` to the instruction 2 bytes before it.
const BNE_BACK_2: u16 = 0xD1FD;
const B_SELF: u16 = 0xE7FE;
/// `mov.w r0, #1`: one cycle, four bytes.
const MOV_W: [u16; 2] = [0xF04F, 0x0001];

/// No MMIO: every access is plain memory.
struct NoMmio;

impl Bus for NoMmio {
    fn read(&mut self, addr: u32, _size: Size) -> Result<u32, BusFault> {
        Err(BusFault { addr, access: Access::Read, kind: BusFaultKind::Unmapped })
    }

    fn write(&mut self, addr: u32, _size: Size, _value: u32) -> Result<(), BusFault> {
        Err(BusFault { addr, access: Access::Write, kind: BusFaultKind::Unmapped })
    }

    fn fetch(&mut self, addr: u32) -> Result<u16, BusFault> {
        Err(BusFault { addr, access: Access::Fetch, kind: BusFaultKind::Unmapped })
    }

    fn exception_return(&mut self, _regs: &mut CoreRegs, _exc_return: u32) {}
}

/// A core with `code` at the start of 64 KiB of flash and 64 KiB of RAM.
struct Rig {
    _flash: Box<[u8]>,
    _ram: Box<[u8]>,
    cpu: Armv7m,
}

impl Rig {
    fn new(code: &[u16], flash: Option<Flash>) -> Self {
        let mut rom = vec![0; SIZE as usize].into_boxed_slice();
        let mut ram = vec![0; SIZE as usize].into_boxed_slice();
        for (i, h) in code.iter().enumerate() {
            rom[2 * i..2 * i + 2].copy_from_slice(&h.to_le_bytes());
        }
        let regions = [
            Region {
                base: FLASH,
                size: SIZE,
                kind: RegionKind::Memory { ptr: NonNull::from(&mut rom[0]), writable: false },
            },
            Region {
                base: RAM,
                size: SIZE,
                kind: RegionKind::Memory { ptr: NonNull::from(&mut ram[0]), writable: true },
            },
        ];
        // SAFETY: both buffers are boxed, live in the rig next to the core and are only touched
        // between runs.
        let mut cpu = unsafe { Armv7m::new(&regions) }.unwrap();
        cpu.reset(RAM + 0x8000, FLASH | 1);
        cpu.set_flash(flash);
        Self { _flash: rom, _ram: ram, cpu }
    }

    fn run(&mut self, n: u32) {
        assert_eq!(self.cpu.run(&mut NoMmio, n), Exit::Budget, "at PC 0x{:08X}", self.cpu.regs().pc);
    }

    fn model(&self) -> u64 {
        self.cpu.model_cycles().expect("the model is on")
    }
}

/// A loop with a literal load, a forward branch into the next line and a backward branch that
/// misses the prefetch buffer:
///
/// ```text
/// 0x00 movs r0, #50
/// 0x02 ldr r1, [pc, #12]   ; the literal at 0x10
/// 0x04 ldr r3, [r2]        ; SRAM
/// 0x06 mov.w r4, #1
/// 0x0A b.n 0x14            ; into line 2, the prefetched one
/// 0x0C nop                 ; skipped
/// 0x0E nop
/// 0x10 .word 0x56781234
/// 0x14 subs r0, #1
/// 0x16 bne 0x02            ; back into line 0
/// 0x18 b .
/// ```
const MIXED: [u16; 14] =
    [0x2032, 0x4903, 0x6813, 0xF04F, 0x0401, 0xE003, NOP, NOP, 0x1234, 0x5678, SUBS_R0_1, 0xD1F4, B_SELF, NOP];

#[test]
fn the_model_is_off_by_default() {
    let mut rig = Rig::new(&MIXED, None);
    rig.cpu.regs_mut().set_reg(2, RAM);
    rig.run(10);
    assert_eq!(rig.cpu.model_cycles(), None);
}

#[test]
fn zero_wait_states_reproduce_the_trm_costs() {
    let mut rig = Rig::new(&MIXED, Some(Flash::stm32f1(0)));
    rig.cpu.regs_mut().set_reg(2, RAM);
    rig.run(400);
    assert_eq!(rig.cpu.regs().reg(0), 0, "the loop ran to its end");
    assert_eq!(rig.model(), rig.cpu.cycles());
}

#[test]
fn a_loop_inside_one_line_runs_from_the_prefetch_buffer() {
    // 100 iterations: 100 subs (1), 99 taken bne (1 + P) and one not taken (1).
    let mut rig = Rig::new(&[SUBS_R0_1, BNE_BACK_2, B_SELF], Some(Flash::STM32F1_72MHZ));
    rig.cpu.regs_mut().set_reg(0, 100);
    rig.run(200);
    assert_eq!(rig.cpu.cycles(), 398);
    assert_eq!(rig.model(), 398, "no wait states inside the buffered line");
}

#[test]
fn a_loop_across_two_lines_reads_the_array_each_time() {
    // subs at 0x06 (line 0), bne at 0x08 (line 1) back to 0x06.
    let code = [NOP, NOP, NOP, SUBS_R0_1, BNE_BACK_2, B_SELF];
    let mut zero = Rig::new(&code, Some(Flash::stm32f1(0)));
    let mut two = Rig::new(&code, Some(Flash::STM32F1_72MHZ));
    for rig in [&mut zero, &mut two] {
        rig.cpu.regs_mut().set_reg(0, 50);
        rig.run(103);
    }
    assert_eq!(zero.model(), zero.cpu.cycles());
    let extra = two.model() - zero.model();
    assert!(extra >= 2 * 49, "each taken bne misses and waits for the array: {extra} extra cycles");
}

#[test]
fn a_literal_load_waits_for_the_array() {
    // Two loop iterations inside line 0 leave the array idle, then ldr r1, [pc, #4] reads 0x0C.
    let code = [SUBS_R0_1, BNE_BACK_2, 0x4901, NOP, NOP, NOP, 0x1234, 0x5678];
    let mut rig = Rig::new(&code, Some(Flash::STM32F1_72MHZ));
    rig.cpu.regs_mut().set_reg(0, 3);
    rig.run(7);
    assert_eq!(rig.cpu.regs().reg(1), 0x5678_1234);
    assert_eq!(rig.cpu.cycles(), 12);
    assert_eq!(rig.model(), 14, "two wait states on the D-Code read");
}

#[test]
fn sixteen_bit_code_keeps_up_and_thirty_two_bit_code_is_fetch_bound() {
    let mut nops = Rig::new(&[NOP; 40], Some(Flash::STM32F1_72MHZ));
    nops.run(32);
    assert_eq!(nops.model(), 32, "a line of four 1-cycle instructions outlasts its 3-cycle read");

    let wide: Vec<u16> = std::iter::repeat_n(MOV_W, 20).flatten().collect();
    let mut rig = Rig::new(&wide, Some(Flash::STM32F1_72MHZ));
    rig.run(16);
    assert_eq!(rig.cpu.cycles(), 16);
    // Line 0 is ready at once; each later line arrives 3 cycles after the one before it, and its
    // two instructions take a cycle each.
    assert_eq!(rig.model(), 4 + 3 * 6 + 2);
}

#[test]
fn the_clock_does_not_depend_on_how_runs_are_split() {
    let mut whole = Rig::new(&MIXED, Some(Flash::STM32F1_72MHZ));
    let mut single = Rig::new(&MIXED, Some(Flash::STM32F1_72MHZ));
    let mut limited = Rig::new(&MIXED, Some(Flash::STM32F1_72MHZ));
    for rig in [&mut whole, &mut single, &mut limited] {
        rig.cpu.regs_mut().set_reg(2, RAM);
    }
    whole.run(300);
    for _ in 0..300 {
        single.run(1);
    }
    while limited.cpu.retired() < 300 {
        let left = (300 - limited.cpu.retired()) as u32;
        assert_eq!(limited.cpu.run_for(&mut NoMmio, left, 7), Exit::Budget);
    }
    assert_eq!(single.model(), whole.model());
    assert_eq!(limited.model(), whole.model());
    assert!(whole.model() > whole.cpu.cycles(), "wait states cost time");
}

#[test]
fn a_cycle_limit_ends_the_run_at_the_instruction_that_reaches_it() {
    let mut rig = Rig::new(&[NOP; 64], Some(Flash::STM32F1_72MHZ));
    assert_eq!(rig.cpu.run_for(&mut NoMmio, 1000, 10), Exit::Budget);
    assert_eq!((rig.cpu.retired(), rig.model()), (10, 10));
    // A core without the model ignores the limit.
    let mut plain = Rig::new(&[NOP; 64], None);
    assert_eq!(plain.cpu.run_for(&mut NoMmio, 20, 10), Exit::Budget);
    assert_eq!(plain.cpu.retired(), 20);
}
