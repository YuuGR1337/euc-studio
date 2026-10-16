use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashSet};

/// Handle to a scheduled event; pass it to [`Scheduler::cancel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventId(u64);

/// A deterministic queue of future events, ordered by `(cycle, sequence)`.
///
/// Events fire in cycle order. Events due at the same cycle fire in the order they were
/// scheduled: a heap keyed by cycle alone would break ties arbitrarily, and two runs of the same
/// scenario could then interleave a timer update and a UART byte differently.
///
/// Cancellation is lazy. A cancelled entry stays in the heap and is dropped when it reaches the
/// front, so reprogramming a timer (cancel + schedule) costs two cheap operations instead of a
/// heap search. The set of live ids is only ever probed, never iterated, so its hash order cannot
/// leak into the event order.
///
/// `E` is whatever the caller needs to route an event, typically a device id plus a kind.
#[derive(Debug)]
pub struct Scheduler<E> {
    heap: BinaryHeap<Reverse<Entry<E>>>,
    live: HashSet<u64>,
    next_seq: u64,
}

impl<E> Scheduler<E> {
    /// An empty scheduler.
    pub fn new() -> Self {
        Self { heap: BinaryHeap::new(), live: HashSet::new(), next_seq: 0 }
    }

    /// Schedules `event` at cycle `at`.
    ///
    /// A cycle in the past is allowed: the event fires on the next [`pop_due`](Self::pop_due).
    pub fn schedule(&mut self, at: u64, event: E) -> EventId {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.live.insert(seq);
        self.heap.push(Reverse(Entry { at, seq, event }));
        EventId(seq)
    }

    /// Cancels a pending event. Returns `false` if it already fired or was cancelled before.
    pub fn cancel(&mut self, id: EventId) -> bool {
        self.live.remove(&id.0)
    }

    /// The cycle of the earliest pending event, or `None` when nothing is scheduled.
    ///
    /// Takes `&mut self` because it drops cancelled entries from the front of the heap.
    pub fn next_at(&mut self) -> Option<u64> {
        self.drop_cancelled_front();
        self.heap.peek().map(|Reverse(entry)| entry.at)
    }

    /// Removes and returns the earliest pending event if it is due at or before `now`.
    ///
    /// Call it in a loop until it returns `None` to fire everything due.
    pub fn pop_due(&mut self, now: u64) -> Option<(u64, E)> {
        if self.next_at()? > now {
            return None;
        }
        let Reverse(entry) = self.heap.pop()?;
        self.live.remove(&entry.seq);
        Some((entry.at, entry.event))
    }

    /// Number of pending (not fired, not cancelled) events.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// True when no event is pending.
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    fn drop_cancelled_front(&mut self) {
        while let Some(Reverse(entry)) = self.heap.peek() {
            if self.live.contains(&entry.seq) {
                break;
            }
            self.heap.pop();
        }
    }
}

impl<E> Default for Scheduler<E> {
    fn default() -> Self {
        Self::new()
    }
}

/// Heap entry. Ordering looks only at `(at, seq)`; the payload never takes part.
#[derive(Debug)]
struct Entry<E> {
    at: u64,
    seq: u64,
    event: E,
}

impl<E> Entry<E> {
    fn key(&self) -> (u64, u64) {
        (self.at, self.seq)
    }
}

impl<E> PartialEq for Entry<E> {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl<E> Eq for Entry<E> {}

impl<E> PartialOrd for Entry<E> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<E> Ord for Entry<E> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain<E>(sched: &mut Scheduler<E>, now: u64) -> Vec<(u64, E)> {
        std::iter::from_fn(|| sched.pop_due(now)).collect()
    }

    #[test]
    fn fires_in_cycle_order() {
        let mut sched = Scheduler::new();
        sched.schedule(30, "c");
        sched.schedule(10, "a");
        sched.schedule(20, "b");
        assert_eq!(sched.next_at(), Some(10));
        assert_eq!(drain(&mut sched, 100), [(10, "a"), (20, "b"), (30, "c")]);
        assert!(sched.is_empty());
        assert_eq!(sched.next_at(), None);
    }

    #[test]
    fn ties_fire_in_schedule_order() {
        let mut sched = Scheduler::new();
        for name in ["tim8", "adc", "usart1", "rtc"] {
            sched.schedule(5, name);
        }
        sched.schedule(4, "early");
        let order: Vec<_> = drain(&mut sched, 5).into_iter().map(|(_, e)| e).collect();
        assert_eq!(order, ["early", "tim8", "adc", "usart1", "rtc"]);
    }

    #[test]
    fn pop_due_waits_for_now() {
        let mut sched = Scheduler::new();
        sched.schedule(10, 1);
        sched.schedule(11, 2);
        assert_eq!(sched.pop_due(9), None);
        assert_eq!(sched.pop_due(10), Some((10, 1)));
        assert_eq!(sched.pop_due(10), None);
        assert_eq!(sched.len(), 1);
    }

    #[test]
    fn past_events_fire_at_once() {
        let mut sched = Scheduler::new();
        sched.schedule(3, ());
        assert_eq!(sched.pop_due(1_000), Some((3, ())));
    }

    #[test]
    fn cancel_skips_the_event() {
        let mut sched = Scheduler::new();
        let first = sched.schedule(10, "reprogrammed timer");
        sched.schedule(20, "uart");
        assert!(sched.cancel(first));
        assert!(!sched.cancel(first), "second cancel is a no-op");
        assert_eq!(sched.len(), 1);
        assert_eq!(sched.next_at(), Some(20));
        assert_eq!(drain(&mut sched, 100), [(20, "uart")]);
    }

    #[test]
    fn cancel_after_fire_reports_false() {
        let mut sched = Scheduler::new();
        let id = sched.schedule(1, ());
        assert!(sched.pop_due(1).is_some());
        assert!(!sched.cancel(id));
    }

    /// Against a reference model (stable sort by cycle), with cancels mixed in.
    #[test]
    fn matches_a_stable_sort_reference() {
        let mut sched = Scheduler::new();
        let mut reference = Vec::new();
        let mut ids = Vec::new();
        let mut lcg: u64 = 0x2545_F491_4F6C_DD1D;
        for n in 0..2_000_u32 {
            lcg = lcg.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let at = (lcg >> 33) % 64;
            ids.push(sched.schedule(at, n));
            reference.push((at, n));
        }
        for n in (0..2_000).step_by(7) {
            assert!(sched.cancel(ids[n]));
            reference.retain(|&(_, e)| e as usize != n);
        }
        reference.sort_by_key(|&(at, _)| at);
        assert_eq!(drain(&mut sched, u64::MAX), reference);
    }
}
