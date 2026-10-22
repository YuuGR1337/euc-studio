//! The sender against the receiver over a simulated link that drops, corrupts, cuts and delays.
//!
//! Invariant under every fault mix: when the sender reports the file delivered, the receiver
//! holds exactly the original bytes. Failures are allowed; silent corruption is not.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use euc_ymodem::{ReceiveStatus, Receiver, ReceiverConfig, SendStatus, Sender, SenderConfig};

/// `SplitMix64`: a small, seedable generator for reproducible fault mixes.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, per_mille: u64) -> bool {
        self.below(1000) < per_mille
    }
}

#[derive(Clone, Copy)]
struct Faults {
    drop: u64,
    corrupt: u64,
    cut: u64,
    spike: u64,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Event {
    at: u64,
    seq: u64,
    to_receiver: bool,
    bytes: Vec<u8>,
}

struct Link {
    rng: Rng,
    faults: Faults,
    queue: BinaryHeap<Reverse<Event>>,
    seq: u64,
    /// Last arrival per direction: BLE and UART deliver in order, so a stall holds up what follows.
    last: [u64; 2],
}

impl Link {
    /// Sends `bytes` at `now`; they arrive after the transmission time and a latency, if at all.
    fn send(&mut self, now: u64, to_receiver: bool, mut bytes: Vec<u8>) {
        let faults = self.faults;
        if bytes.is_empty() || self.rng.chance(faults.drop) {
            return;
        }
        if self.rng.chance(faults.corrupt) {
            let at = self.rng.below(bytes.len() as u64) as usize;
            bytes[at] ^= 1 + self.rng.below(255) as u8;
        }
        if bytes.len() > 1 && self.rng.chance(faults.cut) {
            let at = self.rng.below(bytes.len() as u64) as usize;
            bytes.remove(at);
        }
        // About 2 bytes per millisecond, 10..50 ms latency, now and then a multi-second stall.
        let mut latency = bytes.len() as u64 / 2 + 10 + self.rng.below(40);
        if self.rng.chance(faults.spike) {
            latency += 2_000 + self.rng.below(4_000);
        }
        let lane = usize::from(to_receiver);
        let at = (now + latency).max(self.last[lane]);
        self.last[lane] = at;
        self.seq += 1;
        self.queue.push(Reverse(Event { at, seq: self.seq, to_receiver, bytes }));
    }
}

struct Outcome {
    sender: SendStatus,
    delivered: bool,
    receiver: ReceiveStatus,
    matches: bool,
}

fn transfer(seed: u64, len: usize, faults: Faults, purge: bool) -> Outcome {
    const SENDER_TIMEOUT: u64 = 3_000;
    const RECEIVER_TIMEOUT: u64 = 1_000;
    let mut rng = Rng(seed);
    let image: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
    let mut sender = Sender::new("app.bin", &image, SenderConfig::default()).unwrap();
    let mut receiver = Receiver::new(ReceiverConfig { max_size: 49_152, ..ReceiverConfig::default() });
    let mut link = Link { rng: Rng(seed ^ 0xA5A5), faults, queue: BinaryHeap::new(), seq: 0, last: [0; 2] };

    let mut now = 0;
    let (mut sender_quiet, mut receiver_quiet) = (0, 0);
    // When the sender's latest packet has left: earlier answers are stale (see Sender::delivered).
    let mut sent_until = 0;
    link.send(now, false, receiver.start());
    // Bounded: virtual 30 minutes is far beyond any legitimate transfer here.
    while now < 30 * 60 * 1000 && sender.status() == SendStatus::Running {
        let next_timer = (sender_quiet + SENDER_TIMEOUT).min(receiver_quiet + RECEIVER_TIMEOUT);
        let next_event = link.queue.peek().map_or(u64::MAX, |Reverse(event)| event.at);
        if next_event <= next_timer {
            let Reverse(event) = link.queue.pop().unwrap();
            now = event.at;
            if event.to_receiver {
                receiver_quiet = now;
                let reply = receiver.receive(&event.bytes);
                link.send(now, false, reply);
            } else {
                // The sender's timer runs from its last transmission or step, not from any byte:
                // a receiver polling with `C` must not hold a lost packet off forever.
                for byte in event.bytes {
                    if purge && now < sent_until && byte != euc_ymodem::CAN {
                        continue;
                    }
                    let before = sender.awaiting();
                    if let Some(packet) = sender.receive(byte) {
                        sent_until = now + packet.len() as u64 / 2;
                        link.send(now, true, packet);
                        sender_quiet = now;
                    } else if sender.awaiting() != before {
                        sender_quiet = now;
                    }
                }
            }
        } else {
            now = next_timer;
            if now == sender_quiet + SENDER_TIMEOUT {
                sender_quiet = now;
                if let Some(packet) = sender.timeout() {
                    sent_until = now + packet.len() as u64 / 2;
                    link.send(now, true, packet);
                }
            } else {
                receiver_quiet = now;
                if !matches!(receiver.status(), ReceiveStatus::Done { .. } | ReceiveStatus::Failed(_)) {
                    let reply = receiver.timeout();
                    link.send(now, false, reply);
                }
            }
        }
    }
    Outcome {
        sender: sender.status(),
        delivered: sender.delivered(),
        receiver: receiver.status(),
        matches: receiver.file().is_some_and(|file| file.data == image),
    }
}

#[test]
fn clean_link_delivers_every_size() {
    let clean = Faults { drop: 0, corrupt: 0, cut: 0, spike: 0 };
    for len in [0, 1, 127, 128, 129, 1023, 1024, 1025, 3000, 49_152] {
        let outcome = transfer(len as u64, len, clean, true);
        assert_eq!(outcome.sender, SendStatus::Done { closed: true }, "len {len}");
        assert_eq!(outcome.receiver, ReceiveStatus::Done { closed: true }, "len {len}");
        assert!(outcome.matches, "len {len}");
    }
}

fn campaign(purge: bool) -> (usize, usize) {
    let mixes = [
        Faults { drop: 20, corrupt: 0, cut: 0, spike: 0 },
        Faults { drop: 0, corrupt: 30, cut: 0, spike: 0 },
        Faults { drop: 0, corrupt: 0, cut: 30, spike: 0 },
        Faults { drop: 0, corrupt: 0, cut: 0, spike: 10 },
        Faults { drop: 15, corrupt: 15, cut: 15, spike: 5 },
        Faults { drop: 80, corrupt: 80, cut: 80, spike: 30 },
    ];
    let mut delivered = 0;
    let mut runs = 0;
    for (mix_index, faults) in mixes.into_iter().enumerate() {
        for seed in 0..40 {
            let outcome = transfer(seed * 7919 + mix_index as u64, 20_000, faults, purge);
            runs += 1;
            assert_ne!(outcome.sender, SendStatus::Running, "mix {mix_index} seed {seed} did not terminate");
            if outcome.delivered {
                delivered += 1;
                assert!(outcome.matches, "mix {mix_index} seed {seed}: delivered but the bytes differ");
            }
        }
    }
    (delivered, runs)
}

#[test]
fn faulty_links_never_corrupt_silently() {
    let (delivered, runs) = campaign(true);
    // The light mixes recover from nearly everything; the heavy one may fail often.
    assert!(delivered * 10 >= runs * 7, "only {delivered} of {runs} transfers delivered");
}

#[test]
fn even_without_the_stale_answer_purge_nothing_is_corrupted() {
    // Late answers then cost transfers (the sender runs one block ahead and the receiver
    // cancels), but a delivered file is still exact.
    let (delivered, runs) = campaign(false);
    assert!(delivered > 0 && delivered <= runs);
}
