//! Bounded physical inputs. The simulation thread owns GPIO delivery.

use std::time::{Duration, Instant};

pub(super) const HOLD_LEASE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy)]
pub(super) enum Button {
    Power,
    Handle,
}

impl Button {
    pub(super) fn parse(name: &str) -> Result<Self, String> {
        match name {
            "power" => Ok(Self::Power),
            "handle" => Ok(Self::Handle),
            _ => Err("Physical button must be power or handle".into()),
        }
    }

    pub(super) const fn index(self) -> usize {
        match self {
            Self::Power => 0,
            Self::Handle => 1,
        }
    }

    pub(super) const fn id(self) -> &'static str {
        match self {
            Self::Power => "power",
            Self::Handle => "handle",
        }
    }

    pub(super) fn label(self, duration_ms: u32) -> String {
        let name = match self {
            Self::Power => "power/light button PB15",
            Self::Handle => "handle button PA12",
        };
        format!("Queued {name} for {duration_ms} ms")
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Request {
    pub epoch: u64,
    pub button: Button,
    pub duration_ms: u32,
}

/// At most one queued request and one active pulse per button, protected by the runner mutex.
#[derive(Default)]
pub(super) struct Gate {
    pub epoch: u64,
    pub accepting: bool,
    /// Power can wake the controller while Handle remains unavailable.
    pub power_accepting: bool,
    pub pending: [bool; 2],
    pub active: [bool; 2],
    /// One latest state per GPIO, never a stream of queued press durations.
    pub holds: [Hold; 2],
}

#[derive(Default)]
pub(super) struct Hold {
    pub pressed: bool,
    pub applied: bool,
    received: Option<Instant>,
    /// A missed lease or pause needs an explicit release before a fresh press.
    release_required: bool,
}

impl Gate {
    fn accepts(&self, button: Button) -> bool {
        self.accepting || matches!(button, Button::Power) && self.power_accepting
    }

    pub(super) fn set_readiness(&mut self, buttons: bool, power: bool) {
        self.accepting = buttons;
        self.power_accepting = power;
        for button in [Button::Power, Button::Handle] {
            if !self.accepts(button) {
                let hold = &mut self.holds[button.index()];
                hold.release_required |= hold.pressed;
                hold.pressed = false;
                hold.received = None;
            }
        }
    }

    pub(super) fn reserve(&mut self, button: Button, duration_ms: u32) -> Result<Request, String> {
        if !(1..=10_000).contains(&duration_ms) {
            return Err("Button duration must be 1 through 10000 ms".into());
        }
        if !self.accepting {
            return Err("Buttons need a running simulator after its initial power hold".into());
        }
        let index = button.index();
        if self.pending[index] || self.active[index] || self.holds[index].pressed || self.holds[index].applied {
            return Err("This button already has a queued or active pulse".into());
        }
        self.pending[index] = true;
        Ok(Request { epoch: self.epoch, button, duration_ms })
    }

    /// Epoch validation and last-state replacement happen under the same mutex as run changes.
    pub(super) fn set_state(&mut self, button: Button, pressed: bool, epoch: u64) -> Result<(), String> {
        self.set_state_at(button, pressed, epoch, Instant::now())
    }

    fn set_state_at(&mut self, button: Button, pressed: bool, epoch: u64, now: Instant) -> Result<(), String> {
        if epoch != self.epoch {
            return Err("Physical button state belongs to an old run".into());
        }
        self.expire(now);
        let index = button.index();
        if pressed {
            if !self.accepts(button) {
                return Err("Buttons need a running simulator after its initial power hold".into());
            }
            if self.pending[index] || self.active[index] {
                return Err("This button already has a queued or active pulse".into());
            }
            if self.holds[index].release_required {
                return Err("Release this button before starting a new hold".into());
            }
            if !self.holds[index].pressed && self.holds[index].applied {
                return Err("Wait for the previous button hold to release".into());
            }
        }
        let hold = &mut self.holds[index];
        hold.pressed = pressed;
        hold.received = pressed.then_some(now);
        if !pressed {
            hold.release_required = false;
        }
        Ok(())
    }

    pub(super) fn expire(&mut self, now: Instant) {
        for hold in &mut self.holds {
            if hold.pressed && hold.received.is_none_or(|at| now.saturating_duration_since(at) >= HOLD_LEASE) {
                hold.pressed = false;
                hold.received = None;
                hold.release_required = true;
            }
        }
    }

    pub(super) fn cancel_holds(&mut self) {
        for hold in &mut self.holds {
            hold.release_required |= hold.pressed;
            hold.pressed = false;
            hold.received = None;
        }
    }
    pub(super) fn reject_hold(&mut self, button: Button) {
        let hold = &mut self.holds[button.index()];
        hold.pressed = false;
        hold.received = None;
        hold.release_required = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running() -> Gate {
        Gate { epoch: 7, accepting: true, ..Gate::default() }
    }

    #[test]
    fn power_can_wake_without_admitting_handle_and_pause_cancels_both() {
        let mut gate = running();
        gate.set_readiness(false, true);
        gate.set_state(Button::Power, true, 7).unwrap();
        assert!(gate.set_state(Button::Handle, true, 7).is_err());
        gate.set_readiness(false, true);
        assert!(gate.holds[0].pressed, "a wake hold survives low latch");
        gate.set_readiness(false, false);
        assert!(!gate.holds[0].pressed);
        assert!(gate.set_state(Button::Power, true, 7).is_err());
        gate.set_state(Button::Power, false, 7).unwrap();
        gate.set_readiness(false, true);
        gate.set_state(Button::Power, true, 7).unwrap();
    }

    #[test]
    fn live_renewal_keeps_one_slot_and_expires_at_exact_wall_deadline() {
        let mut gate = running();
        let at = Instant::now();
        gate.set_state_at(Button::Power, true, 7, at).unwrap();
        gate.set_state_at(Button::Power, true, 7, at + Duration::from_millis(100)).unwrap();
        gate.expire(at + Duration::from_millis(349));
        assert!(gate.holds[0].pressed);
        gate.expire(at + Duration::from_millis(350));
        assert!(!gate.holds[0].pressed);
        assert!(gate.holds[0].release_required);
        assert!(!gate.pending[0]);
        assert!(!gate.active[0]);
    }

    #[test]
    fn live_late_renewal_cannot_resurrect_an_expired_hold() {
        let mut gate = running();
        let at = Instant::now();
        gate.set_state_at(Button::Handle, true, 7, at).unwrap();
        assert!(gate.set_state_at(Button::Handle, true, 7, at + HOLD_LEASE).is_err());
        assert!(!gate.holds[1].pressed);
        gate.set_state_at(Button::Handle, false, 7, at + HOLD_LEASE).unwrap();
        gate.set_state_at(Button::Handle, true, 7, at + HOLD_LEASE).unwrap();
        assert!(gate.holds[1].pressed);
    }

    #[test]
    fn live_stale_epoch_cannot_press_or_release_replacement_run() {
        let mut gate = running();
        gate.set_state(Button::Power, true, 7).unwrap();
        assert!(gate.set_state(Button::Power, false, 6).is_err());
        assert!(gate.set_state(Button::Handle, true, 6).is_err());
        assert!(gate.holds[0].pressed);
        assert!(!gate.holds[1].pressed);
    }

    #[test]
    fn live_pause_requires_release_and_allows_release_while_not_accepting() {
        let mut gate = running();
        gate.set_state(Button::Power, true, 7).unwrap();
        gate.cancel_holds();
        gate.accepting = false;
        assert!(!gate.holds[0].pressed);
        assert!(gate.set_state(Button::Power, true, 7).is_err());
        gate.set_state(Button::Power, false, 7).unwrap();
        gate.accepting = true;
        gate.set_state(Button::Power, true, 7).unwrap();
    }

    #[test]
    fn live_reset_and_pulse_admission_stay_separate() {
        let mut gate = running();
        gate.set_state(Button::Power, true, 7).unwrap();
        assert!(gate.reserve(Button::Power, 120).is_err());
        gate.reserve(Button::Handle, 120).unwrap();
        assert!(gate.set_state(Button::Handle, true, 7).is_err());
        gate = Gate { epoch: 8, accepting: true, ..Gate::default() };
        assert!(gate.set_state(Button::Power, false, 7).is_err());
        assert!(!gate.holds[0].pressed);
        assert!(!gate.holds[0].applied);
        gate.set_state(Button::Power, true, 8).unwrap();
    }

    #[test]
    fn live_new_press_cannot_erase_a_release_before_gpio_delivery() {
        let mut gate = running();
        gate.set_state(Button::Power, true, 7).unwrap();
        gate.holds[0].applied = true;
        gate.set_state(Button::Power, false, 7).unwrap();
        assert!(gate.set_state(Button::Power, true, 7).is_err());
        assert!(!gate.holds[0].pressed);
        gate.holds[0].applied = false;
        gate.set_state(Button::Power, true, 7).unwrap();
    }
}
