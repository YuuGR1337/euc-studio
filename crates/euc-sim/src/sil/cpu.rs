//! [`SilCpu`]: the host SIL library as the machine's CPU backend.
//!
//! The library's thread mode (`euc_sil_run`: start-up, then the main loop) runs on a coroutine
//! stack of its own. Every host call it makes (a register access, a spin) is one step of the
//! firmware in simulated time: the access goes to the machine's bus at once and costs
//! [`ACCESS_CYCLES`]; once the quantum's budget is spent, or the bus asks for the run loop (a poll
//! fast-forward, a reset request), the coroutine pauses inside that call and [`Cpu::run_for`]
//! returns to the machine. The machine takes interrupts between quanta with its exception unit, as
//! for an emulated core; when the next quantum resumes the coroutine and the active exception is
//! one the library has a context for, the paused call answers with that context's index and the
//! library runs it there, nested, before the interrupted code goes on. Its return is the exception
//! return. A reset stops the coroutine (the library unwinds with `longjmp` to `euc_sil_run`), and
//! the next power-on loads a fresh copy of the library.
//!
//! Registers: only `xPSR.IPSR` (which context runs), `LR` (the `EXC_RETURN` the exception unit set)
//! and the stack pointer (where the exception unit stacks its frames in emulated SRAM) mean
//! anything; `PC` names the context, for diagnostics.

use std::ffi::{c_char, c_void};
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;

use corosensei::{Coroutine, CoroutineResult, Yielder};
use euc_core::{Bus, CoreRegs, Cpu, Exit, Fault, Size};

use super::library::{self, HostTable, Instance};
use super::{Mirror, TIM8_BDTR};

/// HCLK cycles one register access costs: about the instructions around an access in the port's
/// loops (six at two cycles each), so that loops that count their polls (the SPI wait) time out as
/// late as on the image. An ESTIMATE; see `docs/sil.md`.
pub const ACCESS_CYCLES: u64 = 12;

/// `Fault::Unknown` code for a request the library's port could not serve (its own report is
/// printed to standard error).
pub const PORT_FAULT: u32 = 0x5115;

/// One quantum's worth of work, handed to the coroutine when it resumes.
struct Turn {
    /// The machine's bus for this quantum; `None` when the turn stops the firmware.
    bus: Option<NonNull<dyn Bus>>,
    regs: NonNull<CoreRegs>,
    max_calls: u64,
    max_cycles: u64,
}

/// What a quantum did, handed back when the coroutine pauses.
struct Pause {
    exit: Exit,
    cycles: u64,
    calls: u64,
}

/// How the library's run ended.
struct Outcome {
    status: u32,
    fault: Option<String>,
}

type Fiber = Coroutine<Turn, Pause, Outcome>;

/// A context running (or paused) on the coroutine stack: thread mode is exception 0.
struct Activation {
    exception: u16,
    /// Cycles of a spin still to run before its call returns.
    spin_left: u64,
}

/// The coroutine side: what the library's host calls work with. Lives on the coroutine's heap
/// allocation; the engine side never touches it.
struct FiberHost {
    yielder: *const Yielder<Turn, Pause>,
    turn: Turn,
    used_cycles: u64,
    calls: u64,
    stack: Vec<Activation>,
    /// Exception number of each context index.
    contexts: Rc<[u16]>,
    /// An exit the last access ended with (a bus fault), for the next pause.
    pending: Option<Exit>,
    fault: Option<String>,
    stopped: bool,
}

impl FiberHost {
    fn bus(&mut self) -> Option<&mut dyn Bus> {
        // SAFETY: the pointer is the machine's bus for the current quantum: valid and not used
        // elsewhere while the coroutine runs, which is only inside `SilCpu::run_for`.
        self.turn.bus.map(|mut bus| unsafe { bus.as_mut() })
    }

    fn regs(&mut self) -> &mut CoreRegs {
        // SAFETY: `SilCpu`'s registers, lent for the quantum as the bus is.
        unsafe { self.turn.regs.as_mut() }
    }

    /// Pauses the coroutine with `exit` until the next quantum. True when that quantum stops the
    /// firmware instead.
    fn pause(&mut self, exit: Exit) -> bool {
        let pause = Pause { exit, cycles: self.used_cycles, calls: self.calls };
        self.used_cycles = 0;
        self.calls = 0;
        // SAFETY: the yielder belongs to the coroutine that runs this code; it outlives the run.
        self.turn = unsafe { &*self.yielder }.suspend(pause);
        if self.turn.bus.is_none() {
            self.stopped = true;
        }
        self.stopped
    }

    /// One register access on the bus.
    fn access(&mut self, address: u32, bytes: u32, write: Option<u32>) -> u32 {
        let size = match bytes {
            1 => Size::Byte,
            2 => Size::Half,
            4 => Size::Word,
            _ => {
                self.fail(format!("SIL port: a {bytes}-byte access at 0x{address:08X}"));
                return 0;
            }
        };
        if !address.is_multiple_of(bytes) {
            self.fail(format!("SIL port: unaligned {bytes}-byte access at 0x{address:08X}"));
            return 0;
        }
        let pc = self.regs().pc;
        let Some(bus) = self.bus() else { return 0 };
        bus.note_pc(pc);
        let result = match write {
            None => bus.read(address, size),
            Some(value) => bus.write(address, size, value).map(|()| 0),
        };
        self.used_cycles += ACCESS_CYCLES;
        self.calls += 1;
        result.unwrap_or_else(|fault| {
            self.pending = Some(Exit::Fault(Fault::Bus(fault)));
            0
        })
    }

    /// The port asked for something it cannot do: the run ends with a fault.
    fn fail(&mut self, why: String) {
        self.pending = Some(Exit::Fault(Fault::Unknown(PORT_FAULT)));
        self.fault.get_or_insert(why);
    }

    /// After a host call: pauses when the quantum is over, then answers what the library does next.
    fn settle(&mut self) -> i32 {
        loop {
            if self.stopped {
                return library::STOP;
            }
            if let Some(exit) = self.pending.take() {
                // A fault halts the machine; it never resumes this quantum's work.
                if self.pause(exit) {
                    return library::STOP;
                }
                continue;
            }
            let yield_now = self.bus().is_some_and(Bus::take_yield);
            if yield_now || self.used_cycles >= self.turn.max_cycles || self.calls >= self.turn.max_calls {
                if self.pause(if yield_now { Exit::Yield } else { Exit::Budget }) {
                    return library::STOP;
                }
                continue;
            }
            let ipsr = self.regs().ipsr();
            let top = self.stack.last().map_or(0, |a| a.exception);
            if ipsr != top {
                match self.contexts.iter().position(|&e| e == ipsr) {
                    Some(index) if !self.stack.iter().any(|a| a.exception == ipsr) => {
                        self.stack.push(Activation { exception: ipsr, spin_left: 0 });
                        return index as i32;
                    }
                    _ => return self.park_unhandled(),
                }
            }
            let room = self.turn.max_cycles.saturating_sub(self.used_cycles);
            if let Some(activation) = self.stack.last_mut()
                && activation.spin_left > 0
            {
                let step = activation.spin_left.min(room);
                activation.spin_left -= step;
                self.used_cycles += step;
                continue;
            }
            return library::GO_ON;
        }
    }

    /// An exception with no context in the library: as the image's `Default_Handler`, the bridge
    /// goes off and the core parks (the machine reports the vector as a blocker).
    fn park_unhandled(&mut self) -> i32 {
        if let Some(bus) = self.bus()
            && let Ok(bdtr) = bus.read(TIM8_BDTR, Size::Word)
        {
            let _ = bus.write(TIM8_BDTR, Size::Word, bdtr & !0xC000);
        }
        loop {
            self.used_cycles = self.used_cycles.max(self.turn.max_cycles);
            self.calls += 1;
            if self.pause(Exit::Budget) {
                return library::STOP;
            }
        }
    }
}

/// The host functions the library calls (`struct euc_sil_host`).
mod calls {
    use super::{FiberHost, c_char, c_void, library};

    fn host<'a>(context: *mut c_void) -> &'a mut FiberHost {
        // SAFETY: `context` is the `FiberHost` the coroutine attached; the library calls back only
        // from inside `euc_sil_run` on the coroutine stack, one call at a time.
        unsafe { &mut *context.cast::<FiberHost>() }
    }

    pub(super) unsafe extern "C" fn read(context: *mut c_void, address: u32, bytes: u32, value: *mut u32) -> i32 {
        let host = host(context);
        let read = if host.stopped { 0 } else { host.access(address, bytes, None) };
        // SAFETY: the library passes a pointer to its own local.
        unsafe { value.write(read) };
        host.settle()
    }

    pub(super) unsafe extern "C" fn write(context: *mut c_void, address: u32, bytes: u32, value: u32) -> i32 {
        let host = host(context);
        if !host.stopped {
            host.access(address, bytes, Some(value));
        }
        host.settle()
    }

    pub(super) unsafe extern "C" fn spin(context: *mut c_void, cycles: u32) -> i32 {
        let host = host(context);
        if let Some(activation) = host.stack.last_mut() {
            activation.spin_left += u64::from(cycles);
        }
        host.calls += 1;
        host.settle()
    }

    pub(super) unsafe extern "C" fn returned(context: *mut c_void, index: u32) -> i32 {
        let host = host(context);
        if host.stopped {
            return library::STOP;
        }
        let expected = host.contexts.get(index as usize).copied();
        let ended = host.stack.pop().map(|a| a.exception);
        if ended.is_none() || ended != expected {
            host.fail(format!("SIL port: context {index} returned, but {ended:?} was running"));
            return host.settle();
        }
        let regs = host.turn.regs;
        if let Some(bus) = host.bus() {
            // SAFETY: as `FiberHost::regs`; the bus and the registers are distinct objects.
            let regs = unsafe { &mut *regs.as_ptr() };
            let exc_return = regs.lr;
            bus.exception_return(regs, exc_return);
        }
        host.settle()
    }

    pub(super) unsafe extern "C" fn load(context: *mut c_void, address: u32, bytes: *mut u8, count: u32) {
        let host = host(context);
        let Some(bus) = host.bus() else { return };
        for i in 0..count {
            let value = bus.read(address.wrapping_add(i), euc_core::Size::Byte).unwrap_or(0xFF);
            // SAFETY: the library passes a buffer of `count` bytes.
            unsafe { bytes.add(i as usize).write(value as u8) };
        }
    }

    pub(super) unsafe extern "C" fn store(context: *mut c_void, address: u32, bytes: *const u8, count: u32) {
        let host = host(context);
        let Some(bus) = host.bus() else { return };
        for i in 0..count {
            // SAFETY: the library passes a buffer of `count` bytes.
            let value = unsafe { bytes.add(i as usize).read() };
            let _ = bus.write(address.wrapping_add(i), euc_core::Size::Byte, u32::from(value));
        }
    }

    pub(super) unsafe extern "C" fn fault(context: *mut c_void, what: *const c_char) {
        let host = host(context);
        let text = if what.is_null() {
            "SIL port fault".to_owned()
        } else {
            // SAFETY: the library passes a NUL-terminated string.
            unsafe { std::ffi::CStr::from_ptr(what) }.to_string_lossy().into_owned()
        };
        host.fault.get_or_insert(text);
        host.stopped = true;
    }
}

/// The host SIL library as a [`Cpu`]. See the module docs.
pub struct SilCpu {
    library: PathBuf,
    mask: u64,
    contexts: Rc<[u16]>,
    mirror: Rc<[Mirror]>,
    regs: CoreRegs,
    retired: u64,
    cycles: u64,
    fiber: Option<Fiber>,
    /// The library copy the fiber runs; dropped after it.
    instance: Option<Instance>,
    state: *const u8,
}

impl SilCpu {
    pub(crate) fn new(library: PathBuf, mask: u64, contexts: Rc<[u16]>, mirror: Rc<[Mirror]>) -> Self {
        Self {
            library,
            mask,
            contexts,
            mirror,
            regs: CoreRegs::default(),
            retired: 0,
            cycles: 0,
            fiber: None,
            instance: None,
            state: std::ptr::null(),
        }
    }

    /// Loads a fresh library copy and starts its thread mode on a new coroutine (it runs from the
    /// first resume).
    fn start(&mut self) -> Result<(), String> {
        let instance = Instance::load(&self.library)?;
        let api = instance.api;
        // SAFETY: plain getter of the loaded library.
        self.state = unsafe { (api.state)() }.cast();
        let (mask, contexts) = (self.mask, Rc::clone(&self.contexts));
        self.fiber = Some(Coroutine::new(move |yielder: &Yielder<Turn, Pause>, first: Turn| {
            let stopped = first.bus.is_none();
            let host = Box::into_raw(Box::new(FiberHost {
                yielder,
                turn: first,
                used_cycles: 0,
                calls: 0,
                stack: vec![Activation { exception: 0, spin_left: 0 }],
                contexts,
                pending: None,
                fault: None,
                stopped,
            }));
            let table = HostTable {
                context: host.cast(),
                read: calls::read,
                write: calls::write,
                spin: calls::spin,
                returned: calls::returned,
                load: calls::load,
                store: calls::store,
                fault: calls::fault,
            };
            // SAFETY: the library copy outlives the coroutine (`SilCpu` drops it after the fiber);
            // the table and the host it points to live until `euc_sil_run` returns.
            let status = if stopped {
                library::RUN_STOPPED
            } else {
                unsafe {
                    (api.quirk_set_mask)(mask);
                    if (api.attach)(&raw const table) { (api.run)() } else { u32::MAX }
                }
            };
            // SAFETY: made by `Box::into_raw` above; the library no longer calls back.
            let host = unsafe { Box::from_raw(host) };
            Outcome { status, fault: host.fault }
        }));
        self.instance = Some(instance);
        Ok(())
    }

    /// Stops the firmware: the paused host call answers stop, the library unwinds to the end of
    /// `euc_sil_run`, and the copy is dropped.
    fn stop(&mut self) {
        if let Some(mut fiber) = self.fiber.take()
            && fiber.started()
            && !fiber.done()
        {
            let turn = Turn { bus: None, regs: NonNull::from(&mut self.regs), max_calls: 0, max_cycles: 0 };
            if let CoroutineResult::Yield(_) = fiber.resume(turn) {
                // The library went on after a stop: drop its stack without running anything on it
                // (the host it holds leaks).
                // SAFETY: the coroutine stack holds only the library's C frames and this module's
                // host calls, none of which needs to run code to be dropped.
                unsafe { fiber.force_reset() };
            }
        }
        self.instance = None;
        self.state = std::ptr::null();
    }

    /// Copies the probes from the library's state into emulated SRAM.
    fn mirror(&self, bus: &mut dyn Bus) {
        if self.state.is_null() {
            return;
        }
        for probe in self.mirror.iter() {
            for i in 0..probe.size {
                // SAFETY: `state` points at the library's struct app of at least `offset + size`
                // bytes (checked when the library was opened); the firmware is paused.
                let byte = unsafe { self.state.add((probe.offset + i) as usize).read() };
                let _ = bus.write(probe.address + i, Size::Byte, u32::from(byte));
            }
        }
    }
}

impl Drop for SilCpu {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Cpu for SilCpu {
    fn reset(&mut self, sp: u32, reset_vector: u32) {
        self.stop();
        self.regs = CoreRegs::at_reset(sp, reset_vector);
    }

    fn run(&mut self, bus: &mut dyn Bus, max_instr: u32) -> Exit {
        self.run_for(bus, max_instr, u64::MAX)
    }

    fn run_for(&mut self, bus: &mut dyn Bus, max_instr: u32, max_cycles: u64) -> Exit {
        if self.fiber.is_none()
            && let Err(why) = self.start()
        {
            eprintln!("euc-sim: {why}");
            return Exit::Fault(Fault::Unknown(PORT_FAULT));
        }
        // SAFETY: only the lifetime is erased; the coroutine uses the pointer during this resume
        // alone, and a pause always comes back before `run_for` returns.
        let bus_ptr: NonNull<dyn Bus> = unsafe { std::mem::transmute(NonNull::from(&mut *bus)) };
        let turn = Turn {
            bus: Some(bus_ptr),
            regs: NonNull::from(&mut self.regs),
            max_calls: u64::from(max_instr.max(1)),
            max_cycles: max_cycles.max(1),
        };
        let Some(fiber) = self.fiber.as_mut() else { return Exit::Fault(Fault::Unknown(PORT_FAULT)) };
        let exit = match fiber.resume(turn) {
            CoroutineResult::Yield(pause) => {
                self.cycles = self.cycles.saturating_add(pause.cycles);
                self.retired += pause.calls;
                pause.exit
            }
            CoroutineResult::Return(outcome) => {
                self.fiber = None;
                let why = outcome.fault.unwrap_or_else(|| {
                    format!("the SIL library stopped by itself (euc_sil_run returned {})", outcome.status)
                });
                eprintln!("euc-sim: {why}");
                Exit::Fault(Fault::Unknown(PORT_FAULT))
            }
        };
        self.mirror(bus);
        exit
    }

    fn regs(&self) -> &CoreRegs {
        &self.regs
    }

    fn regs_mut(&mut self) -> &mut CoreRegs {
        &mut self.regs
    }

    fn retired(&self) -> u64 {
        self.retired
    }

    fn watch(&mut self, _addrs: &[u32]) {}

    fn invalidate(&mut self, _addr: u32, _len: u32) {}

    fn model_cycles(&self) -> Option<u64> {
        Some(self.cycles)
    }
}
