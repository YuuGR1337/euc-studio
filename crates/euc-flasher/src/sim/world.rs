//! The simulated link and clock: BLE writes and notifications with latency, order, faults, and
//! the same outbound guard a real link uses.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use euc_ble::{Guard, MTU, Outbound};
use euc_ymodem::{SOH, STX, encode_block, packet_len};

use super::faults::{FaultPlan, OperatorScript, Rng};
use super::wheel::{Micros, SimWheel};
use crate::transport::{Inbound, LinkError, LinkKind, Operator, Transport};

/// Link timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimConfig {
    /// Time one write without response takes (about one connection event).
    pub write_interval: Micros,
    /// Shortest one-way latency.
    pub latency: Micros,
    /// Random extra latency, up to this much.
    pub jitter: Micros,
    /// Time to connect.
    pub connect: Micros,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self { write_interval: 10_000, latency: 15_000, jitter: 10_000, connect: 1_000_000 }
    }
}

/// The wheel, the link and the clock.
#[derive(Debug)]
pub struct SimWorld {
    now: Micros,
    /// The wheel.
    pub wheel: SimWheel,
    config: SimConfig,
    plan: FaultPlan,
    rng: Rng,
    connected: bool,
    /// The link can come back from this time (the wheel must be powered too).
    down_until: Micros,
    was_powered: bool,
    to_wheel: VecDeque<(Micros, Vec<u8>)>,
    to_phone: VecDeque<(Micros, Vec<u8>)>,
    last_to_wheel: Micros,
    last_to_phone: Micros,
    writes: u64,
    notifications: u64,
    data_blocks: u64,
    guard: Guard,
    /// Faults that fired, in order (for reports).
    pub events: Vec<String>,
}

impl SimWorld {
    /// A connected link to `wheel` at time 0.
    pub fn new(wheel: SimWheel, plan: FaultPlan, config: SimConfig, seed: u64) -> Self {
        let was_powered = wheel.powered();
        Self {
            now: 0,
            wheel,
            config,
            plan,
            rng: Rng::new(seed),
            connected: true,
            down_until: 0,
            was_powered,
            to_wheel: VecDeque::new(),
            to_phone: VecDeque::new(),
            last_to_wheel: 0,
            last_to_phone: 0,
            writes: 0,
            notifications: 0,
            data_blocks: 0,
            guard: Guard::new(),
            events: Vec::new(),
        }
    }

    /// The clock.
    pub const fn now(&self) -> Micros {
        self.now
    }

    /// True while the phone is connected.
    pub const fn connected(&self) -> bool {
        self.connected
    }

    /// Replaces the fault plan of the link and the wheel.
    pub fn set_faults(&mut self, plan: FaultPlan) {
        self.wheel.set_faults(&plan);
        self.plan = plan;
    }

    /// Runs the wheel and the link up to `until`.
    pub fn advance_to(&mut self, until: Micros) {
        while let Some(at) = self.next_internal().filter(|&at| at <= until) {
            self.process(at);
        }
        self.now = self.now.max(until);
    }

    /// Runs the wheel and the link for `duration`.
    pub fn advance(&mut self, duration: Duration) {
        let until = self.now + micros(duration);
        self.advance_to(until);
    }

    /// Switches the wheel on; the BLE module comes up half a second later.
    pub fn power_on(&mut self) {
        self.wheel.power_on(self.now);
        self.was_powered = true;
        self.down_until = self.now + 500_000;
    }

    /// Cuts the wheel's power; the link drops.
    pub fn power_off(&mut self) {
        self.wheel.power_off();
        self.lose_link(Micros::MAX);
    }

    fn next_internal(&self) -> Option<Micros> {
        let delivery = self.to_wheel.front().map(|(at, _)| *at);
        [delivery, self.wheel.next_wake()].into_iter().flatten().min()
    }

    fn process(&mut self, at: Micros) {
        self.now = self.now.max(at);
        while self.to_wheel.front().is_some_and(|(due, _)| *due <= self.now) {
            let (_, bytes) = self.to_wheel.pop_front().unwrap_or_default();
            self.wheel.receive(self.now, &bytes);
            self.emit();
        }
        if self.wheel.next_wake().is_some_and(|wake| wake <= self.now) {
            self.wheel.wake(self.now);
            self.emit();
        }
    }

    /// Takes the wheel's UART output into notifications of at most [`MTU`] bytes.
    fn emit(&mut self) {
        let powered = self.wheel.powered();
        if self.was_powered && !powered {
            self.events.push(format!("{} ms: the wheel lost power", self.now / 1000));
            self.lose_link(Micros::MAX);
        }
        self.was_powered = powered;
        let out = self.wheel.take_output();
        if !self.connected || out.is_empty() {
            return;
        }
        for chunk in out.chunks(MTU) {
            let index = self.notifications;
            self.notifications += 1;
            if self.plan.drop_notifications.contains(&index) {
                self.events.push(format!("notification {index} dropped"));
                continue;
            }
            let mut bytes = chunk.to_vec();
            if self.plan.corrupt_notifications.contains(&index) {
                let at = self.rng.below(bytes.len() as u64) as usize;
                bytes[at] ^= 1 + self.rng.below(255) as u8;
                self.events.push(format!("notification {index} corrupted"));
            }
            let mut latency = self.latency();
            if let Some((_, extra)) = self.plan.stalls.iter().find(|(i, _)| *i == index) {
                latency += micros(*extra);
                self.events.push(format!("notification {index} stalled {} ms", extra.as_millis()));
            }
            let at = (self.now + latency).max(self.last_to_phone);
            self.last_to_phone = at;
            self.to_phone.push_back((at, bytes));
        }
    }

    fn latency(&mut self) -> Micros {
        self.config.latency + self.rng.below(self.config.jitter + 1)
    }

    fn lose_link(&mut self, until: Micros) {
        self.connected = false;
        self.down_until = until;
        self.to_phone.clear();
        self.to_wheel.clear();
    }

    fn send(&mut self, out: Outbound<'_>) -> Result<(), LinkError> {
        if !self.connected {
            return Err(LinkError::Lost);
        }
        let now = Duration::from_micros(self.now);
        self.guard.check(now, out).map_err(LinkError::Refused)?;
        let mut bytes = match out {
            Outbound::Command(byte) => vec![byte],
            Outbound::Ymodem(packet) => packet.to_vec(),
        };
        if matches!(bytes.first(), Some(&(SOH | STX))) && bytes.get(1).is_some_and(|&seq| seq != 0) {
            self.data_blocks += 1;
            if self.plan.wrong_block == Some(self.data_blocks) && packet_len(bytes[0]) == Some(bytes.len()) {
                // Other payload bytes with a matching CRC: YMODEM cannot tell.
                let mut payload = bytes[3..bytes.len() - 2].to_vec();
                for byte in &mut payload {
                    *byte = byte.wrapping_add(0x5B);
                }
                bytes = encode_block(bytes[1], &payload);
                self.events.push(format!("data block {} replaced (wrong image)", self.data_blocks));
            }
        }
        for chunk in bytes.chunks(MTU) {
            let until = self.now + self.config.write_interval;
            self.advance_to(until);
            if !self.connected {
                return Err(LinkError::Lost);
            }
            let index = self.writes;
            self.writes += 1;
            if let Some((_, down)) = self.plan.link_losses.iter().find(|(i, _)| *i == index) {
                self.events.push(format!("write {index}: link lost for {} ms", down.as_millis()));
                let until = self.now + micros(*down);
                self.lose_link(until);
                return Err(LinkError::Lost);
            }
            if self.plan.power_losses.contains(&index) {
                self.events.push(format!("write {index}: the wheel lost power"));
                self.wheel.power_off();
                self.was_powered = false;
                self.lose_link(Micros::MAX);
                return Err(LinkError::Lost);
            }
            if self.plan.drop_writes.contains(&index) {
                self.events.push(format!("write {index} dropped"));
                continue;
            }
            let mut chunk = chunk.to_vec();
            if self.plan.corrupt_writes.contains(&index) {
                let at = self.rng.below(chunk.len() as u64) as usize;
                chunk[at] ^= 1 + self.rng.below(255) as u8;
                self.events.push(format!("write {index} corrupted"));
            }
            let at = (self.now + self.latency()).max(self.last_to_wheel);
            self.last_to_wheel = at;
            self.to_wheel.push_back((at, chunk));
        }
        Ok(())
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<Inbound>, LinkError> {
        let deadline = self.now + micros(timeout);
        loop {
            if !self.connected {
                return Err(LinkError::Lost);
            }
            let front = self.to_phone.front().map(|(at, _)| *at);
            let internal = self.next_internal();
            if let Some(at) = front.filter(|&at| at <= deadline && internal.is_none_or(|i| at <= i)) {
                self.now = self.now.max(at);
                let (at, bytes) = self.to_phone.pop_front().unwrap_or_default();
                let at = Duration::from_micros(at);
                self.guard.observe(at, &bytes);
                return Ok(Some(Inbound { at, bytes }));
            }
            match internal {
                Some(at) if at <= deadline => self.process(at),
                _ => {
                    self.now = self.now.max(deadline);
                    return Ok(None);
                }
            }
        }
    }

    fn reconnect(&mut self, timeout: Duration) -> Result<(), LinkError> {
        if self.connected {
            return Ok(());
        }
        let deadline = self.now + micros(timeout);
        let ready = (self.now + self.config.connect).max(self.down_until);
        if ready > deadline {
            self.advance_to(deadline);
            return Err(LinkError::Lost);
        }
        self.advance_to(ready);
        if !self.wheel.powered() {
            self.advance_to(deadline);
            return Err(LinkError::Lost);
        }
        self.connected = true;
        self.guard.forget();
        Ok(())
    }
}

/// A [`Transport`] onto a [`SimWorld`].
#[derive(Debug, Clone)]
pub struct SimTransport {
    world: Rc<RefCell<SimWorld>>,
}

impl SimTransport {
    /// A transport onto `world`.
    pub const fn new(world: Rc<RefCell<SimWorld>>) -> Self {
        Self { world }
    }
}

/// The simulated wheel's identity on the link.
pub const SIM_PEER: &str = "SIM-0000-EXTREME";

impl Transport for SimTransport {
    fn kind(&self) -> LinkKind {
        LinkKind::Simulator
    }

    fn peer(&self) -> String {
        SIM_PEER.to_owned()
    }

    fn now(&self) -> Duration {
        Duration::from_micros(self.world.borrow().now)
    }

    fn send(&mut self, out: Outbound<'_>) -> Result<(), LinkError> {
        self.world.borrow_mut().send(out)
    }

    fn arm_entry(&mut self) {
        self.world.borrow_mut().guard.arm_entry();
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<Inbound>, LinkError> {
        self.world.borrow_mut().recv(timeout)
    }

    fn reconnect(&mut self, timeout: Duration) -> Result<(), LinkError> {
        self.world.borrow_mut().reconnect(timeout)
    }
}

/// A scripted person at the simulated wheel.
#[derive(Debug, Clone)]
pub struct SimOperator {
    world: Rc<RefCell<SimWorld>>,
    script: OperatorScript,
    /// Everything the flasher said, in order.
    pub said: Vec<String>,
}

impl SimOperator {
    /// An operator at `world`'s wheel.
    pub const fn new(world: Rc<RefCell<SimWorld>>, script: OperatorScript) -> Self {
        Self { world, script, said: Vec::new() }
    }
}

impl Operator for SimOperator {
    fn say(&mut self, text: &str) {
        self.said.push(text.to_owned());
    }

    fn confirm(&mut self, expected: &str) -> String {
        self.script.types.clone().unwrap_or_else(|| expected.to_owned())
    }

    fn press_power_button(&mut self) {
        let mut world = self.world.borrow_mut();
        world.advance(self.script.press_delay);
        if self.script.presses_button {
            let now = world.now;
            world.wheel.press_button(now);
        }
    }

    fn power_cycle_wheel(&mut self) -> bool {
        if !self.script.power_cycles {
            return false;
        }
        let mut world = self.world.borrow_mut();
        world.advance(Duration::from_secs(1));
        if world.wheel.powered() {
            world.power_off();
        }
        world.advance(self.script.power_cycle_delay);
        world.power_on();
        true
    }
}

fn micros(duration: Duration) -> Micros {
    u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
}
