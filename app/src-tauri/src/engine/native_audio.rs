//! Native macOS sound: the wheel's buzzer and, in a free ride, the ride bed (motor, tyre, wind:
//! ride_sound.rs). The live runner owns this service; Compare must not use it.
//!
//! Start before firmware stepping. Reset once per run, then submit every sampled simulation
//! frontier, including batches with no tone changes. Audio never reconstructs a past boot.
//! Both voices play on simulated time; the master volume scales both, and the bed can be faded
//! out on its own (a hidden window) while the buzzer goes on.

use std::collections::VecDeque;
use std::fmt;

pub use super::host::Tone;
use super::ride_sound::Bed;
pub use super::ride_sound::Ride;
const SILENT: Tone = Tone { t_s: 0.0, hz: 0.0, duty: 0.0 };

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioError(pub String);

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for AudioError {}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AudioStatus {
    pub backend: &'static str,
    pub state: &'static str,
    pub enabled: bool,
    pub error: Option<String>,
}

const SAMPLE_RATE: f64 = 48_000.0;
const LEAD_FRAMES: u64 = 2_880; // 60 ms scheduling margin, not a recording/replay buffer.
const MAX_EVENTS: usize = 2_048;
const MAX_BATCH: usize = 128;
/// The master volume follows the panel over this, s; the bed fades in or out over `BED_FADE_S`.
const VOLUME_GLIDE_S: f64 = 0.05;
const BED_FADE_S: f64 = 0.3;

pub use platform::NativeBuzzer;

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    };
    use std::thread::{self, JoinHandle, Thread};
    use std::time::{Duration, Instant};
    use std::{ffi::c_void, ptr, slice};

    const BUFFER_COUNT: usize = 3;
    const BUFFER_FRAMES: usize = 256;
    const MESSAGE_COUNT: usize = 32;
    const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);

    fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        value.lock().unwrap_or_else(PoisonError::into_inner)
    }

    struct Shared {
        status: Mutex<AudioStatus>,
        epoch: AtomicU64,
        has_epoch: AtomicBool,
        stop: AtomicBool,
        fault: AtomicBool,
        #[cfg(test)]
        rendered_samples: AtomicU64,
        #[cfg(test)]
        nonzero_samples: AtomicU64,
    }
    impl Shared {
        fn fail(&self, error: &AudioError) {
            self.fault.store(true, Ordering::Release);
            let mut status = lock(&self.status);
            status.state = "error";
            status.error = Some(error.to_string());
        }
        fn refresh(&self) {
            let mut status = lock(&self.status);
            if !self.fault.load(Ordering::Acquire) {
                status.state = if status.enabled { "ready" } else { "muted" };
                status.error = None;
            }
        }
    }

    type Reply = SyncSender<Result<(), AudioError>>;
    struct Control {
        reply: Reply,
        deadline: Instant,
        cancelled: Arc<AtomicBool>,
    }
    impl Control {
        fn expired(&self) -> bool {
            self.cancelled.load(Ordering::Acquire) || Instant::now() >= self.deadline
        }
        fn answer(self, result: Result<(), AudioError>, shared: &Shared) {
            let result =
                if self.expired() { Err(failure("Native audio control expired; output silenced")) } else { result };
            if let Err(error) = &result {
                shared.fail(error);
            }
            let _ = self.reply.send(result);
        }
    }
    struct Batch {
        epoch: u64,
        time: f64,
        rate: f64,
        len: usize,
        tones: [Tone; MAX_BATCH],
        ride: Option<Ride>,
    }
    // A fixed-size channel preallocates the tone payloads. submit() allocates no payload or Vec.
    #[allow(clippy::large_enum_variant)]
    enum Message {
        Reset(u64, f64, Control),
        Playing(u64, bool, Control),
        Enabled(bool, Control),
        Feed(Batch),
        Mix(f64, bool),
    }

    struct Inner {
        tx: SyncSender<Message>,
        shared: Arc<Shared>,
        wake: Thread,
        join: Mutex<Option<JoinHandle<()>>>,
    }
    impl Drop for Inner {
        fn drop(&mut self) {
            self.shared.stop.store(true, Ordering::Release);
            self.wake.unpark();
            // The worker and callback hold Shared, never Inner: there is no ownership cycle.
            if let Some(join) = lock(&self.join).take() {
                let _ = join.join();
            }
        }
    }

    /// Cheap clones share one native owner. The final clone stops, joins, and disposes output.
    #[derive(Clone)]
    pub struct NativeBuzzer {
        inner: Arc<Inner>,
    }

    impl NativeBuzzer {
        /// Completes AudioQueue creation and priming before returning. No audio gesture is needed.
        pub fn start(enabled: bool) -> Result<Self, AudioError> {
            let shared = Arc::new(Shared {
                status: Mutex::new(AudioStatus {
                    backend: "native",
                    state: if enabled { "ready" } else { "muted" },
                    enabled,
                    error: None,
                }),
                epoch: AtomicU64::new(0),
                has_epoch: AtomicBool::new(false),
                stop: AtomicBool::new(false),
                fault: AtomicBool::new(false),
                #[cfg(test)]
                rendered_samples: AtomicU64::new(0),
                #[cfg(test)]
                nonzero_samples: AtomicU64::new(0),
            });
            let (tx, rx) = mpsc::sync_channel(MESSAGE_COUNT);
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let worker_shared = Arc::clone(&shared);
            let join = thread::Builder::new()
                .name("euc-buzzer".into())
                .spawn(move || run(rx, worker_shared, ready_tx))
                .map_err(|error| failure(format!("Cannot start native audio thread: {error}")))?;
            let wake = join.thread().clone();
            match ready_rx.recv() {
                Ok(Ok(())) => Ok(Self { inner: Arc::new(Inner { tx, shared, wake, join: Mutex::new(Some(join)) }) }),
                result => {
                    shared.stop.store(true, Ordering::Release);
                    wake.unpark();
                    let _ = join.join();
                    Err(match result {
                        Ok(Err(error)) => error,
                        _ => failure("Native audio thread stopped during initialization"),
                    })
                }
            }
        }

        /// Flush the previous run before stepping the next run. Also clears a recoverable fault.
        pub fn reset(&self, epoch: u64, sim_time: f64) -> Result<(), AudioError> {
            self.control(|reply| Message::Reset(epoch, sim_time, reply))
        }

        /// Submit every source frontier, even with no changes. Never waits for the audio thread.
        /// rate is simulated seconds per audio second; it changes timing, never carrier frequency.
        /// `ride` is the free ride's sample for the bed; None quiets it (any other run).
        pub fn submit(
            &self,
            epoch: u64,
            sim_time: f64,
            rate: f64,
            tones: &[Tone],
            ride: Option<Ride>,
        ) -> Result<(), AudioError> {
            self.check_epoch(epoch)?;
            if self.inner.shared.fault.load(Ordering::Acquire) {
                return Err(failure(self.status().error.unwrap_or_else(|| "Native audio is unavailable".into())));
            }
            if tones.len() > MAX_BATCH {
                return self.fail(failure(format!("Native audio batch exceeds {MAX_BATCH} tone changes")));
            }
            let mut batch = Batch { epoch, time: sim_time, rate, len: tones.len(), tones: [SILENT; MAX_BATCH], ride };
            batch.tones[..tones.len()].copy_from_slice(tones);
            self.enqueue(Message::Feed(batch))
        }

        /// Pause flushes queued PCM. Resume uses only the source's current state on the next submit.
        pub fn set_playing(&self, epoch: u64, playing: bool) -> Result<(), AudioError> {
            self.check_epoch(epoch)?;
            self.control(|reply| Message::Playing(epoch, playing, reply))
        }

        pub fn set_enabled(&self, enabled: bool) -> Result<(), AudioError> {
            // enabled reports the requested preference even when queue/control failure prevents output.
            lock(&self.inner.shared.status).enabled = enabled;
            self.control(|reply| Message::Enabled(enabled, reply))
        }

        pub fn status(&self) -> AudioStatus {
            lock(&self.inner.shared.status).clone()
        }

        /// The master volume (0 to 1, both voices) and whether the ride bed sounds (false while
        /// the window is hidden). A preference: it never waits for the audio thread.
        pub fn set_mix(&self, volume: f64, bed: bool) -> Result<(), AudioError> {
            if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
                return Err(failure("Sound volume must be between 0 and 1"));
            }
            self.enqueue(Message::Mix(volume, bed))
        }

        /// PCM samples successfully enqueued, and samples with nonzero amplitude. Not speaker proof.
        /// Counts remain cumulative across epochs so a no-webview boot check can compare snapshots.
        #[cfg(test)]
        pub(crate) fn rendered_samples(&self) -> (u64, u64) {
            (
                self.inner.shared.rendered_samples.load(Ordering::Acquire),
                self.inner.shared.nonzero_samples.load(Ordering::Acquire),
            )
        }

        fn check_epoch(&self, epoch: u64) -> Result<(), AudioError> {
            if !self.inner.shared.has_epoch.load(Ordering::Acquire)
                || self.inner.shared.epoch.load(Ordering::Acquire) != epoch
            {
                return Err(failure("Native audio command belongs to another run epoch"));
            }
            Ok(())
        }
        fn fail<T>(&self, error: AudioError) -> Result<T, AudioError> {
            self.inner.shared.fail(&error);
            self.inner.wake.unpark();
            Err(error)
        }
        fn enqueue(&self, message: Message) -> Result<(), AudioError> {
            match self.inner.tx.try_send(message) {
                Ok(()) => {
                    self.inner.wake.unpark();
                    Ok(())
                }
                Err(TrySendError::Full(_)) => self.fail(failure("Native audio message queue is full; output silenced")),
                Err(TrySendError::Disconnected(_)) => self.fail(failure("Native audio owner thread stopped")),
            }
        }
        fn control(&self, make: impl FnOnce(Control) -> Message) -> Result<(), AudioError> {
            let (tx, rx) = mpsc::sync_channel(1);
            let cancelled = Arc::new(AtomicBool::new(false));
            self.enqueue(make(Control {
                reply: tx,
                deadline: Instant::now() + CONTROL_TIMEOUT,
                cancelled: Arc::clone(&cancelled),
            }))?;
            match rx.recv_timeout(CONTROL_TIMEOUT) {
                Ok(result) => result,
                Err(_) => {
                    cancelled.store(true, Ordering::Release);
                    self.fail(failure("Native audio control reply timed out; output silenced"))
                }
            }
        }
    }

    fn run(rx: Receiver<Message>, shared: Arc<Shared>, ready: Reply) {
        let mut device = match Device::new() {
            Ok(device) => device,
            Err(error) => {
                shared.fail(&error);
                let _ = ready.send(Err(error));
                return;
            }
        };
        let mut scheduler = Scheduler::new();
        let mut playing = false;
        let mut enabled = lock(&shared.status).enabled;
        let mut stopped_for_fault = false;
        let mut last_callback = Instant::now();
        if ready.send(Ok(())).is_err() {
            return;
        }
        while !shared.stop.load(Ordering::Acquire) {
            if shared.fault.load(Ordering::Acquire) && !stopped_for_fault {
                if let Err(error) = device.stop() {
                    shared.fail(&error);
                }
                scheduler.suspend();
                stopped_for_fault = true;
            }
            // Bound command work so a producer cannot starve buffer refills or shutdown.
            for _ in 0..MESSAGE_COUNT {
                let message = match rx.try_recv() {
                    Ok(message) => message,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                };
                match message {
                    Message::Reset(_, _, control) | Message::Playing(_, _, control) | Message::Enabled(_, control)
                        if control.expired() =>
                    {
                        control.answer(Err(failure("Native audio command expired before execution")), &shared);
                    }
                    Message::Reset(epoch, time, control) => {
                        let result = scheduler.reset(epoch, time).and_then(|()| device.restart());
                        if result.is_ok() && !control.expired() {
                            playing = false;
                            shared.epoch.store(epoch, Ordering::Release);
                            shared.has_epoch.store(true, Ordering::Release);
                            shared.fault.store(false, Ordering::Release);
                            stopped_for_fault = false;
                            last_callback = Instant::now();
                            shared.refresh();
                        } else if let Err(error) = &result {
                            shared.fail(error);
                        }
                        control.answer(result, &shared);
                    }
                    Message::Playing(epoch, value, control) => {
                        let result = if scheduler.epoch != Some(epoch) {
                            Err(failure("Native audio pause belongs to another run epoch"))
                        } else {
                            if playing == value {
                                Ok(())
                            } else {
                                playing = value;
                                scheduler.suspend();
                                if stopped_for_fault { Ok(()) } else { device.restart() }
                            }
                        };
                        if let Err(error) = &result {
                            shared.fail(error);
                        }
                        last_callback = Instant::now();
                        control.answer(result, &shared);
                    }
                    Message::Enabled(value, control) => {
                        lock(&shared.status).enabled = value;
                        let result = if enabled == value {
                            Ok(())
                        } else {
                            enabled = value;
                            scheduler.suspend();
                            if stopped_for_fault { Ok(()) } else { device.restart() }
                        };
                        if let Err(error) = &result {
                            shared.fail(error);
                        }
                        last_callback = Instant::now();
                        shared.refresh();
                        control.answer(result, &shared);
                    }
                    Message::Feed(batch) => {
                        if shared.fault.load(Ordering::Acquire) || scheduler.epoch != Some(batch.epoch) {
                            continue;
                        }
                        let result = if enabled && playing {
                            scheduler.push_ride(
                                batch.epoch,
                                batch.time,
                                batch.rate,
                                &batch.tones[..batch.len],
                                batch.ride,
                            )
                        } else {
                            scheduler.observe(batch.epoch, batch.time, batch.rate, &batch.tones[..batch.len])
                        };
                        if let Err(error) = result {
                            shared.fail(&error);
                        }
                    }
                    Message::Mix(volume, bed) => scheduler.mix(volume, bed),
                }
            }
            if !shared.fault.load(Ordering::Acquire) {
                let returned = device.context.returned.swap(0, Ordering::AcqRel);
                if returned != 0 {
                    last_callback = Instant::now();
                }
                let mut indices = [0, 1, 2];
                // Multiple buffers can return while the owner is descheduled. Preserve their
                // callback order, including a wrap from slot 2 to slot 0.
                indices.sort_unstable_by_key(|index| device.context.order[*index].load(Ordering::Acquire));
                for index in indices {
                    if returned & (1 << index) == 0 {
                        continue;
                    }
                    match device.fill(index, &mut scheduler, enabled && playing) {
                        Ok(stats) => {
                            #[cfg(test)]
                            {
                                shared.rendered_samples.fetch_add(stats.samples, Ordering::Release);
                                shared.nonzero_samples.fetch_add(stats.nonzero, Ordering::Release);
                            }
                            #[cfg(not(test))]
                            let _ = stats;
                        }
                        Err(error) => {
                            shared.fail(&error);
                            break;
                        }
                    }
                }
                if last_callback.elapsed() > Duration::from_secs(1) {
                    shared.fail(&failure("Native audio output stopped returning buffers"));
                }
            }
            thread::park_timeout(Duration::from_millis(1));
        }
        // Device drops here, on its owner thread, before the proxy's last Drop joins it.
    }

    #[repr(C)]
    struct Format {
        sample_rate: f64,
        format_id: u32,
        flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels: u32,
        bits: u32,
        reserved: u32,
    }
    #[repr(C)]
    struct Buffer {
        capacity: u32,
        data: *mut c_void,
        byte_size: u32,
        user_data: *mut c_void,
        packet_capacity: u32,
        packets: *mut c_void,
        packet_count: u32,
    }
    type Queue = *mut c_void;
    #[link(name = "AudioToolbox", kind = "framework")]
    unsafe extern "C" {
        fn AudioQueueNewOutput(
            format: *const Format,
            callback: unsafe extern "C" fn(*mut c_void, Queue, *mut Buffer),
            user: *mut c_void,
            run_loop: *const c_void,
            mode: *const c_void,
            flags: u32,
            queue: *mut Queue,
        ) -> i32;
        fn AudioQueueAllocateBuffer(queue: Queue, bytes: u32, buffer: *mut *mut Buffer) -> i32;
        fn AudioQueueEnqueueBuffer(queue: Queue, buffer: *mut Buffer, packets: u32, descriptions: *const c_void)
        -> i32;
        fn AudioQueueStart(queue: Queue, time: *const c_void) -> i32;
        fn AudioQueueStop(queue: Queue, immediate: u8) -> i32;
        fn AudioQueueDispose(queue: Queue, immediate: u8) -> i32;
    }
    fn os_result(operation: &str, status: i32) -> Result<(), AudioError> {
        if status == 0 { Ok(()) } else { Err(failure(format!("{operation} failed (OSStatus {status})"))) }
    }

    struct Callback {
        buffers: [AtomicPtr<Buffer>; BUFFER_COUNT],
        returned: AtomicU8,
        order: [AtomicU64; BUFFER_COUNT],
        sequence: AtomicU64,
        wake: Thread,
    }
    // AudioQueue owns buffer memory. Only the owner writes PCM, and only after its callback returns it.
    unsafe extern "C" fn returned(user: *mut c_void, _queue: Queue, buffer: *mut Buffer) {
        // SAFETY: Device retains this boxed context through synchronous AudioQueueDispose.
        let context = unsafe { &*user.cast::<Callback>() };
        for index in 0..BUFFER_COUNT {
            if context.buffers[index].load(Ordering::Acquire) == buffer {
                let sequence = context.sequence.fetch_add(1, Ordering::Relaxed);
                context.order[index].store(sequence, Ordering::Release);
                context.returned.fetch_or(1 << index, Ordering::Release);
                context.wake.unpark();
                break;
            }
        }
    }

    #[derive(Default)]
    struct RenderStats {
        #[cfg(test)]
        samples: u64,
        #[cfg(test)]
        nonzero: u64,
    }

    struct Device {
        queue: Queue,
        context: std::mem::ManuallyDrop<Box<Callback>>,
    }
    impl Device {
        fn new() -> Result<Self, AudioError> {
            let context = Box::new(Callback {
                buffers: std::array::from_fn(|_| AtomicPtr::new(ptr::null_mut())),
                returned: AtomicU8::new(0),
                order: std::array::from_fn(|_| AtomicU64::new(0)),
                sequence: AtomicU64::new(0),
                wake: thread::current(),
            });
            let mut device = Self { queue: ptr::null_mut(), context: std::mem::ManuallyDrop::new(context) };
            let format = Format {
                sample_rate: SAMPLE_RATE,
                format_id: u32::from_be_bytes(*b"lpcm"),
                flags: 1 | 8,
                bytes_per_packet: 4,
                frames_per_packet: 1,
                bytes_per_frame: 4,
                channels: 1,
                bits: 32,
                reserved: 0,
            };
            // SAFETY: repr(C) types follow the SDK ABI. NULL run loop selects AudioQueue's internal thread.
            os_result("AudioQueueNewOutput", unsafe {
                AudioQueueNewOutput(
                    &format,
                    returned,
                    (&mut **device.context as *mut Callback).cast(),
                    ptr::null(),
                    ptr::null(),
                    0,
                    &mut device.queue,
                )
            })?;
            if device.queue.is_null() {
                return Err(failure("AudioQueueNewOutput returned no queue"));
            }
            for slot in &device.context.buffers {
                let mut buffer = ptr::null_mut();
                // SAFETY: AudioQueue writes a valid buffer pointer on success; Device owns its queue.
                os_result("AudioQueueAllocateBuffer", unsafe {
                    AudioQueueAllocateBuffer(device.queue, (BUFFER_FRAMES * 4) as u32, &mut buffer)
                })?;
                if buffer.is_null() {
                    return Err(failure("AudioQueueAllocateBuffer returned no buffer"));
                }
                slot.store(buffer, Ordering::Release);
            }
            device.prime()?;
            Ok(device)
        }
        fn prime(&mut self) -> Result<(), AudioError> {
            self.context.returned.store(0, Ordering::Release);
            for index in 0..BUFFER_COUNT {
                self.write(index, |_| Ok(()))?;
            }
            // SAFETY: All buffers are initialized before starting; NULL means start immediately.
            os_result("AudioQueueStart", unsafe { AudioQueueStart(self.queue, ptr::null()) })
        }
        fn stop(&mut self) -> Result<(), AudioError> {
            // SAFETY: Only this owner thread calls queue lifecycle functions. Callbacks never enqueue.
            os_result("AudioQueueStop", unsafe { AudioQueueStop(self.queue, 1) })
        }
        fn restart(&mut self) -> Result<(), AudioError> {
            self.stop()?;
            self.prime()
        }
        fn fill(&mut self, index: usize, scheduler: &mut Scheduler, active: bool) -> Result<RenderStats, AudioError> {
            #[allow(unused_mut)]
            let mut stats = RenderStats::default();
            self.write(index, |pcm| {
                if active {
                    scheduler.render(pcm)?;
                }
                #[cfg(test)]
                {
                    stats.samples = pcm.len() as u64;
                    stats.nonzero = pcm.iter().filter(|value| value.abs() > 1e-8).count() as u64;
                }
                Ok(())
            })?;
            Ok(stats)
        }
        fn write(
            &mut self,
            index: usize,
            fill: impl FnOnce(&mut [f32]) -> Result<(), AudioError>,
        ) -> Result<(), AudioError> {
            let buffer = self.context.buffers[index].load(Ordering::Acquire);
            // SAFETY: This buffer is newly allocated, synchronously stopped, or returned by AudioQueue.
            let buffer = unsafe { &mut *buffer };
            if buffer.data.is_null() || buffer.capacity < (BUFFER_FRAMES * 4) as u32 {
                return Err(failure("AudioQueue returned an invalid PCM buffer"));
            }
            // SAFETY: Float PCM buffers have at least BUFFER_FRAMES * 4 bytes of suitably aligned storage.
            let pcm = unsafe { slice::from_raw_parts_mut(buffer.data.cast::<f32>(), BUFFER_FRAMES) };
            pcm.fill(0.0);
            fill(pcm)?;
            buffer.byte_size = (BUFFER_FRAMES * 4) as u32;
            // SAFETY: PCM has a constant packet size, so packet descriptions are absent.
            os_result("AudioQueueEnqueueBuffer", unsafe { AudioQueueEnqueueBuffer(self.queue, buffer, 0, ptr::null()) })
        }
    }
    impl Drop for Device {
        fn drop(&mut self) {
            if !self.queue.is_null() {
                // SAFETY: Context and buffer slots remain alive until synchronous disposal returns.
                // Apple forbids subsequent callbacks after Dispose returns when called outside callbacks.
                let result = unsafe { AudioQueueDispose(self.queue, 1) };
                if result != 0 {
                    // Preserve callback memory if the OS refuses disposal. A bounded leak is safer
                    // than freeing memory a failed queue might still access.
                    eprintln!("[native audio] AudioQueueDispose failed (OSStatus {result}); callback storage retained");
                    return;
                }
            }
            // SAFETY: No queue exists, or synchronous disposal guaranteed the callbacks have ended.
            unsafe {
                std::mem::ManuallyDrop::drop(&mut self.context);
            }
        }
    }

    #[cfg(test)]
    mod ownership_tests {
        use super::*;

        fn shared() -> Arc<Shared> {
            Arc::new(Shared {
                status: Mutex::new(AudioStatus { backend: "native", state: "ready", enabled: true, error: None }),
                epoch: AtomicU64::new(1),
                has_epoch: AtomicBool::new(true),
                stop: AtomicBool::new(false),
                fault: AtomicBool::new(false),
                rendered_samples: AtomicU64::new(0),
                nonzero_samples: AtomicU64::new(0),
            })
        }

        #[test]
        fn last_proxy_drop_stops_and_joins_one_owner_without_opening_audio() {
            let shared = shared();
            let finished = Arc::new(AtomicBool::new(false));
            let worker_shared = Arc::clone(&shared);
            let worker_finished = Arc::clone(&finished);
            let join = thread::spawn(move || {
                while !worker_shared.stop.load(Ordering::Acquire) {
                    thread::park();
                }
                worker_finished.store(true, Ordering::Release);
            });
            let (tx, _rx) = mpsc::sync_channel(MESSAGE_COUNT);
            let proxy = NativeBuzzer {
                inner: Arc::new(Inner {
                    tx,
                    shared: Arc::clone(&shared),
                    wake: join.thread().clone(),
                    join: Mutex::new(Some(join)),
                }),
            };
            let last = proxy.clone();
            assert_eq!(last.rendered_samples(), (0, 0));
            drop(proxy);
            assert!(!shared.stop.load(Ordering::Acquire));
            assert!(!finished.load(Ordering::Acquire));
            drop(last);
            assert!(finished.load(Ordering::Acquire));
        }

        #[test]
        fn full_message_queue_sets_an_explicit_fault_without_waiting() {
            let shared = shared();
            let (tx, _rx) = mpsc::sync_channel(1);
            let proxy =
                NativeBuzzer { inner: Arc::new(Inner { tx, shared, wake: thread::current(), join: Mutex::new(None) }) };
            proxy.submit(1, 0.0, 1.0, &[], None).unwrap();
            assert!(proxy.submit(1, 0.01, 1.0, &[], None).is_err());
            assert_eq!(proxy.status().state, "error");
            assert!(proxy.status().error.unwrap().contains("queue is full"));
        }

        #[test]
        fn expired_control_cannot_report_success() {
            let shared = shared();
            let (tx, rx) = mpsc::sync_channel(1);
            let control = Control { reply: tx, deadline: Instant::now(), cancelled: Arc::new(AtomicBool::new(false)) };
            control.answer(Ok(()), &shared);
            assert!(rx.recv().unwrap().is_err());
            assert!(shared.fault.load(Ordering::Acquire));
        }

        #[test]
        fn callback_preserves_return_order_across_buffer_slot_wrap() {
            let mut context = Box::new(Callback {
                buffers: std::array::from_fn(|i| AtomicPtr::new(ptr::without_provenance_mut::<Buffer>(i + 1))),
                returned: AtomicU8::new(0),
                order: std::array::from_fn(|_| AtomicU64::new(0)),
                sequence: AtomicU64::new(0),
                wake: thread::current(),
            });
            let user = (&mut *context as *mut Callback).cast();
            // The callback compares these pointers only. It never dereferences buffer storage.
            unsafe {
                returned(user, ptr::null_mut(), context.buffers[2].load(Ordering::Relaxed));
            }
            unsafe {
                returned(user, ptr::null_mut(), context.buffers[0].load(Ordering::Relaxed));
            }
            assert_eq!(context.returned.load(Ordering::Acquire), 0b101);
            assert!(context.order[2].load(Ordering::Acquire) < context.order[0].load(Ordering::Acquire));
        }

        #[test]
        fn ffi_layout_matches_macos_64_bit_sdk() {
            assert_eq!(std::mem::size_of::<Format>(), 40);
            assert_eq!(std::mem::align_of::<Format>(), 8);
            assert_eq!(std::mem::size_of::<Buffer>(), 56);
            assert_eq!(std::mem::offset_of!(Buffer, data), 8);
            assert_eq!(std::mem::offset_of!(Buffer, byte_size), 16);
            assert_eq!(std::mem::offset_of!(Buffer, packets), 40);
        }

        #[test]
        #[ignore = "opens the local audio device and plays two short buzzer pulses"]
        fn native_device_renders_live_pulses_after_silent_clock_resync() {
            let audio = NativeBuzzer::start(true).unwrap();
            audio.reset(1, 0.0).unwrap();
            audio.set_playing(1, true).unwrap();
            let edges = [
                Tone { t_s: 0.3, hz: 2700.0, duty: 0.5 },
                Tone { t_s: 0.373, hz: 0.0, duty: 0.0 },
                Tone { t_s: 0.5, hz: 2700.0, duty: 0.5 },
                Tone { t_s: 0.573, hz: 0.0, duty: 0.0 },
            ];
            let started = Instant::now();
            let mut next = 0;
            loop {
                let time = started.elapsed().as_secs_f64();
                let first = next;
                while next < edges.len() && edges[next].t_s <= time {
                    next += 1;
                }
                // This deliberately lags the actual 1x source clock throughout the silent start.
                let rate = if time < 0.29 { 0.9034 } else { 1.0 };
                audio.submit(1, time, rate, &edges[first..next], None).unwrap();
                assert_eq!(audio.status().state, "ready", "{:?}", audio.status());
                if time >= 0.8 {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            audio.set_playing(1, false).unwrap();
            let (samples, nonzero) = audio.rendered_samples();
            assert_eq!(next, edges.len());
            assert!(samples > 0 && nonzero > 0, "successfully enqueued PCM: {samples}/{nonzero}");
            println!("Native AudioQueue enqueued {samples} samples, including {nonzero} nonzero samples");
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    /// Unsupported platforms expose an explicit status; the parent may select WebAudio there.
    #[derive(Clone)]
    pub struct NativeBuzzer {
        enabled: Arc<AtomicBool>,
    }
    impl NativeBuzzer {
        pub fn start(enabled: bool) -> Result<Self, AudioError> {
            Ok(Self { enabled: Arc::new(AtomicBool::new(enabled)) })
        }
        pub fn reset(&self, _epoch: u64, _time: f64) -> Result<(), AudioError> {
            Ok(())
        }
        pub fn submit(
            &self,
            _epoch: u64,
            _time: f64,
            _rate: f64,
            _tones: &[Tone],
            _ride: Option<Ride>,
        ) -> Result<(), AudioError> {
            Ok(())
        }
        pub fn set_mix(&self, _volume: f64, _bed: bool) -> Result<(), AudioError> {
            Ok(())
        }
        pub fn set_playing(&self, _epoch: u64, _playing: bool) -> Result<(), AudioError> {
            Ok(())
        }
        pub fn set_enabled(&self, enabled: bool) -> Result<(), AudioError> {
            self.enabled.store(enabled, Ordering::Relaxed);
            Ok(())
        }
        pub fn status(&self) -> AudioStatus {
            AudioStatus {
                backend: "unsupported",
                state: "unsupported",
                enabled: self.enabled.load(Ordering::Relaxed),
                error: None,
            }
        }
    }
}

fn failure(message: impl Into<String>) -> AudioError {
    AudioError(message.into())
}

struct Scheduled {
    frame: u64,
    tone: Tone,
}

struct Scheduler {
    epoch: Option<u64>,
    last_time: f64,
    rate: f64,
    offset: Option<f64>,
    cursor: u64,
    frontier: u64,
    source_tone: Tone,
    events: VecDeque<Scheduled>,
    synth: Synth,
    /// The free ride's samples for the bed, on the same clock as the tones (None quiets it).
    rides: VecDeque<(u64, Option<Ride>)>,
    /// The last batch carried a ride sample.
    riding: bool,
    bed: Bed,
    /// The master volume and the bed's gate: targets, and where they are now.
    volume: f64,
    volume_now: f64,
    bed_on: bool,
    bed_now: f64,
}

impl Scheduler {
    fn new() -> Self {
        Self {
            epoch: None,
            last_time: 0.0,
            rate: 1.0,
            offset: None,
            cursor: 0,
            frontier: 0,
            source_tone: SILENT,
            events: VecDeque::with_capacity(MAX_EVENTS),
            synth: Synth::new(),
            rides: VecDeque::with_capacity(MAX_EVENTS),
            riding: false,
            bed: Bed::new(),
            volume: 1.0,
            volume_now: 1.0,
            // The ride bed is opt-in (the studio's "Ride sound (experimental)"): silent until a mix
            // turns it on.
            bed_on: false,
            bed_now: 0.0,
        }
    }
    fn mix(&mut self, volume: f64, bed: bool) {
        self.volume = volume.clamp(0.0, 1.0);
        self.bed_on = bed;
    }
    fn reset(&mut self, epoch: u64, time: f64) -> Result<(), AudioError> {
        if !time.is_finite() || time < 0.0 || self.epoch.is_some_and(|old| epoch < old) {
            return Err(failure("Invalid audio reset time or old run epoch"));
        }
        self.suspend();
        self.epoch = Some(epoch);
        self.last_time = time;
        self.rate = 1.0;
        self.source_tone = SILENT;
        Ok(())
    }
    fn validate(&self, epoch: u64, time: f64, rate: f64, tones: &[Tone]) -> Result<(), AudioError> {
        if self.epoch != Some(epoch) {
            return Err(failure("Audio input belongs to another run epoch"));
        }
        if !time.is_finite() || time < self.last_time || !rate.is_finite() || rate <= 0.0 {
            return Err(failure("Invalid or reversed audio source clock"));
        }
        let mut previous = self.last_time;
        for tone in tones {
            if !tone.t_s.is_finite()
                || tone.t_s < previous
                || tone.t_s > time
                || !tone.hz.is_finite()
                || !(0.0..SAMPLE_RATE / 2.0).contains(&tone.hz)
                || !tone.duty.is_finite()
                || !(0.0..=1.0).contains(&tone.duty)
            {
                return Err(failure("Invalid, reversed, or unsupported native buzzer tone"));
            }
            previous = tone.t_s;
        }
        Ok(())
    }
    fn observe(&mut self, epoch: u64, time: f64, rate: f64, tones: &[Tone]) -> Result<(), AudioError> {
        self.validate(epoch, time, rate, tones)?;
        self.last_time = time;
        self.rate = rate;
        if let Some(tone) = tones.last() {
            self.source_tone = *tone;
        }
        Ok(())
    }
    #[cfg(test)]
    fn push(&mut self, epoch: u64, time: f64, rate: f64, tones: &[Tone]) -> Result<(), AudioError> {
        self.push_ride(epoch, time, rate, tones, None)
    }
    fn push_ride(
        &mut self,
        epoch: u64,
        time: f64,
        rate: f64,
        tones: &[Tone],
        ride: Option<Ride>,
    ) -> Result<(), AudioError> {
        self.validate(epoch, time, rate, tones)?;
        if let Some(ride) = &ride
            && (!ride.valid() || ride.t_s < self.last_time || ride.t_s > time)
        {
            return Err(failure("Invalid or out-of-batch ride sound sample"));
        }
        if (time - self.last_time) / rate > 1.0 {
            return Err(failure("Native audio source interval exceeded the bounded scheduling window"));
        }
        let initial = self.offset.is_none();
        // A rate estimate is a derivative, not a clock. Integrating 1 / measuredRTF accumulates
        // phase error even when producer and consumer really run at 1x. Re-anchor only at a safe
        // silent frontier: no queued edges, no incoming edges, no live source tone or filter tail.
        // Active pulses and future edges keep their existing clock mapping. Late onsets never
        // enter this branch and cannot be replayed. The original window bound stays unchanged.
        let silent_frontier = tones.is_empty()
            && self.events.is_empty()
            && (self.source_tone.hz == 0.0 || self.source_tone.duty == 0.0 || self.source_tone.duty == 1.0)
            && self.synth.silent();
        let offset = if silent_frontier && (initial || time > self.last_time) {
            self.cursor as f64 + LEAD_FRAMES as f64 - time / rate * SAMPLE_RATE
        } else {
            match self.offset {
                None => self.cursor as f64 + LEAD_FRAMES as f64 - self.last_time / rate * SAMPLE_RATE,
                Some(offset) => offset + self.last_time * SAMPLE_RATE * (1.0 / self.rate - 1.0 / rate),
            }
        };
        let mapped = |t: f64| offset + t / rate * SAMPLE_RATE;
        let frontier = mapped(time);
        if !frontier.is_finite() || frontier < self.cursor as f64 || frontier > self.cursor as f64 + SAMPLE_RATE {
            return Err(failure(format!(
                "Native audio source clock exceeded the bounded scheduling window: source={time:.6}s previous={:.6}s rate={rate:.6} previous_rate={:.6} cursor={} frontier={frontier:.3} lead={:.3} frames pending={}",
                self.last_time,
                self.rate,
                self.cursor,
                frontier - self.cursor as f64,
                self.events.len()
            )));
        }
        let baseline = initial && self.source_tone.hz > 0.0;
        if self.events.len() + tones.len() + usize::from(baseline) > MAX_EVENTS {
            return Err(failure("Native audio tone queue is full"));
        }
        for tone in tones {
            let at = mapped(tone.t_s);
            if !at.is_finite() || at.round() < self.cursor as f64 {
                return Err(failure("Late native audio tone discarded; historical sound cannot replay"));
            }
        }
        if baseline {
            // Resume only the source's current steady state, never its discarded transitions.
            self.events.push_back(Scheduled { frame: self.cursor + LEAD_FRAMES, tone: self.source_tone });
        }
        for tone in tones {
            self.events.push_back(Scheduled { frame: mapped(tone.t_s).round() as u64, tone: *tone });
        }
        // The bed is a set of gliding parameters, not edges: a late sample takes effect now.
        if ride.is_some() || self.riding {
            let at = ride.map_or(time, |r| r.t_s);
            let frame = (mapped(at).round() as u64).max(self.cursor);
            if self.rides.len() < MAX_EVENTS {
                self.rides.push_back((frame, ride));
            }
            self.riding = ride.is_some();
        }
        self.offset = Some(offset);
        self.frontier = frontier.round() as u64;
        self.observe(epoch, time, rate, tones)
    }
    fn render(&mut self, output: &mut [f32]) -> Result<(), AudioError> {
        let mut underrun = false;
        let volume_rate = 1.0 - (-1.0 / (SAMPLE_RATE * VOLUME_GLIDE_S)).exp();
        let bed_step = 1.0 / (SAMPLE_RATE * BED_FADE_S);
        for sample in output {
            while self.events.front().is_some_and(|event| event.frame <= self.cursor) {
                if let Some(event) = self.events.pop_front() {
                    self.synth.tone(event.tone);
                }
            }
            while self.rides.front().is_some_and(|(frame, _)| *frame <= self.cursor) {
                match self.rides.pop_front() {
                    Some((_, Some(ride))) => self.bed.set(&ride),
                    Some((_, None)) => self.bed.quiet(),
                    None => {}
                }
            }
            self.volume_now += (self.volume - self.volume_now) * volume_rate;
            self.bed_now =
                if self.bed_on { (self.bed_now + bed_step).min(1.0) } else { (self.bed_now - bed_step).max(0.0) };
            if self.cursor < self.frontier {
                let buzzer = f64::from(self.synth.next());
                let bed = if self.bed.silent() || self.bed_now == 0.0 {
                    0.0
                } else {
                    self.bed_now * self.bed.next(self.synth.target_gain > 0.0)
                };
                *sample = ((buzzer + bed) * self.volume_now).clamp(-0.9, 0.9) as f32;
            } else {
                *sample = 0.0;
                underrun |= self.synth.target_gain > 0.0;
            }
            self.cursor = self.cursor.saturating_add(1);
        }
        if underrun { Err(failure("Native audio input stopped during a tone; output silenced")) } else { Ok(()) }
    }
    fn suspend(&mut self) {
        self.events.clear();
        self.offset = None;
        self.cursor = 0;
        self.frontier = 0;
        self.synth = Synth::new();
        self.rides.clear();
        self.riding = false;
        self.bed = Bed::new();
    }
}

/// Constant-peak band-pass at 2.7 kHz, Q=2.2, matching the existing WebAudio approximation.
struct Synth {
    hz: f64,
    phase: f64,
    gain: f64,
    target_gain: f64,
    b0: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}
impl Synth {
    fn silent(&self) -> bool {
        self.target_gain == 0.0 && self.gain.abs() < 1e-8 && self.y1.abs() < 1e-8 && self.y2.abs() < 1e-8
    }
    fn new() -> Self {
        let w = std::f64::consts::TAU * 2700.0 / SAMPLE_RATE;
        let alpha = w.sin() / (2.0 * 2.2);
        Self {
            hz: 2700.0,
            phase: 0.0,
            gain: 0.0,
            target_gain: 0.0,
            b0: alpha / (1.0 + alpha),
            a1: -2.0 * w.cos() / (1.0 + alpha),
            a2: (1.0 - alpha) / (1.0 + alpha),
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
    fn tone(&mut self, tone: Tone) {
        if tone.hz > 0.0 {
            self.hz = tone.hz;
        }
        self.target_gain = if tone.hz > 0.0 && tone.duty > 0.0 && tone.duty < 1.0 {
            0.22 * (std::f64::consts::PI * tone.duty).sin()
        } else {
            0.0
        };
    }
    fn next(&mut self) -> f32 {
        self.phase = (self.phase + self.hz / SAMPLE_RATE).fract();
        self.gain += (self.target_gain - self.gain) * (1.0 - (-1.0 / (SAMPLE_RATE * 0.0008)).exp());
        let x = if self.phase < 0.5 { self.gain } else { -self.gain };
        let y = self.b0 * (x - self.x2) - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y.clamp(-0.8, 0.8) as f32
    }
}

/// A run's sound offline, as the live runner would play it at 1x: each sample's tones and ride
/// sound pushed in order, with the output rendered up to the sample's time before each push (the
/// device's clock), then the scheduling lead flushed. 48 kHz mono from the first sample's time;
/// `ride_sound` as the studio's opt-in switch. Test-only: the README showcase (`showcase.rs`).
#[cfg(test)]
pub(super) fn render_offline(
    samples: &[(f64, Vec<Tone>, Option<Ride>)],
    ride_sound: bool,
) -> Result<Vec<f32>, AudioError> {
    let Some(&(start, ..)) = samples.first() else { return Ok(Vec::new()) };
    let mut scheduler = Scheduler::new();
    scheduler.reset(1, start)?;
    scheduler.mix(1.0, ride_sound);
    scheduler.bed_now = if ride_sound { 1.0 } else { 0.0 };
    let mut out = Vec::new();
    let mut render_to = |scheduler: &mut Scheduler, frame: u64| -> Result<(), AudioError> {
        let mut chunk = vec![0.0; usize::try_from(frame.saturating_sub(scheduler.cursor)).unwrap_or(0)];
        scheduler.render(&mut chunk)?;
        out.extend_from_slice(&chunk);
        Ok(())
    };
    for (t_s, tones, ride) in samples {
        render_to(&mut scheduler, ((t_s - start) * SAMPLE_RATE).round() as u64)?;
        scheduler.push_ride(1, *t_s, 1.0, tones, *ride)?;
    }
    let end = scheduler.cursor + LEAD_FRAMES;
    render_to(&mut scheduler, end)?;
    out.drain(..usize::try_from(LEAD_FRAMES).unwrap_or(0).min(out.len()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(t_s: f64, hz: f64) -> Tone {
        Tone { t_s, hz, duty: if hz > 0.0 { 0.5 } else { 0.0 } }
    }
    fn scheduler() -> Scheduler {
        let mut s = Scheduler::new();
        s.reset(1, 0.0).unwrap();
        s.push(1, 0.0, 1.0, &[]).unwrap();
        s
    }
    /// A scheduler with the opt-in ride sound on (already faded in).
    fn riding() -> Scheduler {
        let mut s = scheduler();
        s.mix(1.0, true);
        s.bed_now = 1.0;
        s
    }
    fn energy(samples: &[f32]) -> f64 {
        samples.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / samples.len() as f64
    }
    fn frame(seconds: f64) -> usize {
        (seconds * SAMPLE_RATE).round() as usize
    }

    #[test]
    fn firmware_horn_keeps_both_73ms_pulses_and_the_silent_gap() {
        let mut s = scheduler();
        s.push(1, 0.4, 1.0, &[tone(0.0, 2700.0), tone(0.073, 0.0), tone(0.2, 2700.0), tone(0.273, 0.0)]).unwrap();
        let mut pcm = vec![0.0; frame(0.45)];
        s.render(&mut pcm).unwrap();
        assert_eq!(energy(&pcm[..LEAD_FRAMES as usize]), 0.0);
        assert!(energy(&pcm[frame(0.07)..frame(0.12)]) > 0.001);
        assert!(energy(&pcm[frame(0.15)..frame(0.24)]) < 1e-8);
        assert!(energy(&pcm[frame(0.27)..frame(0.32)]) > 0.001);
        assert!(energy(&pcm[frame(0.36)..frame(0.44)]) < 1e-8);
    }

    #[test]
    fn old_epoch_and_invalid_batch_do_not_replace_valid_tones() {
        let mut s = scheduler();
        assert!(s.push(0, 0.1, 1.0, &[tone(0.0, 2700.0)]).is_err());
        assert!(s.push(1, 0.1, 1.0, &[tone(0.0, 2700.0), tone(f64::NAN, 0.0)]).is_err());
        s.push(1, 0.2, 1.0, &[tone(0.0, 2700.0), tone(0.1, 0.0)]).unwrap();
        let mut pcm = vec![0.0; frame(0.25)];
        s.render(&mut pcm).unwrap();
        assert!(energy(&pcm[frame(0.07)..frame(0.14)]) > 0.001);
    }

    #[test]
    fn reset_discards_the_previous_runs_queued_boot() {
        let mut s = scheduler();
        s.push(1, 0.2, 1.0, &[tone(0.0, 2700.0)]).unwrap();
        s.reset(2, 0.0).unwrap();
        s.push(2, 0.2, 1.0, &[]).unwrap();
        let mut pcm = vec![1.0; frame(0.2)];
        s.render(&mut pcm).unwrap();
        assert_eq!(energy(&pcm), 0.0);
        assert!(s.push(1, 0.3, 1.0, &[]).is_err());
    }

    #[test]
    fn pause_discards_complete_past_pulses_instead_of_replaying_them() {
        let mut s = scheduler();
        s.push(1, 0.2, 1.0, &[tone(0.0, 2700.0), tone(0.1, 0.0)]).unwrap();
        let mut before = vec![0.0; frame(0.1)];
        s.render(&mut before).unwrap();
        assert!(energy(&before[frame(0.07)..]) > 0.001);
        s.suspend();
        s.push(1, 0.3, 1.0, &[]).unwrap();
        let mut pcm = vec![1.0; frame(0.05)];
        s.render(&mut pcm).unwrap();
        assert_eq!(energy(&pcm), 0.0);
    }

    #[test]
    fn late_onset_fails_instead_of_replaying_a_missed_boot() {
        let mut s = scheduler();
        let mut pcm = vec![0.0; frame(0.2)];
        s.render(&mut pcm).unwrap();
        assert!(s.push(1, 0.21, 1.0, &[tone(0.0, 2700.0)]).is_err());
    }

    #[test]
    fn source_stall_does_not_hold_an_alarm_forever() {
        let mut s = scheduler();
        s.push(1, 0.02, 1.0, &[tone(0.0, 2700.0)]).unwrap();
        let mut pcm = vec![1.0; frame(0.2)];
        assert!(s.render(&mut pcm).is_err());
        assert!(pcm[frame(0.081)..].iter().all(|value| *value == 0.0));
    }

    #[test]
    fn rate_changes_duration_but_preserves_carrier_frequency() {
        let mut slow = scheduler();
        slow.push(1, 0.2, 0.5, &[tone(0.0, 2700.0), tone(0.1, 0.0)]).unwrap();
        let mut pcm = vec![0.0; frame(0.4)];
        slow.render(&mut pcm).unwrap();
        let carrier = &pcm[frame(0.08)..frame(0.18)];
        let crossings = carrier.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((crossings as i32 - 270).abs() <= 2);
        assert!(energy(&pcm[frame(0.18)..frame(0.24)]) > 0.001);
        assert!(energy(&pcm[frame(0.3)..frame(0.39)]) < 1e-8);
    }

    #[test]
    fn event_count_and_future_horizon_are_bounded() {
        let mut s = scheduler();
        let many: Vec<_> = (0..=MAX_EVENTS).map(|i| tone(i as f64 / 10000.0, 2700.0)).collect();
        assert!(s.push(1, 0.3, 1.0, &many).is_err());
        assert!(s.push(1, 100.0, 1.0, &[]).is_err());
    }

    #[test]
    fn resume_schedules_current_tone_before_its_new_off_transition() {
        let mut s = scheduler();
        s.push(1, 0.1, 1.0, &[tone(0.0, 2700.0)]).unwrap();
        s.suspend();
        s.push(1, 0.2, 1.0, &[tone(0.15, 0.0)]).unwrap();
        let mut pcm = vec![0.0; frame(0.15)];
        s.render(&mut pcm).unwrap();
        assert!(energy(&pcm[frame(0.07)..frame(0.1)]) > 0.001);
        assert!(energy(&pcm[frame(0.13)..]) < 1e-8);
    }

    #[test]
    fn muted_observations_keep_current_state_without_accumulating_history() {
        let mut s = scheduler();
        s.suspend();
        for i in 1..=10000 {
            s.observe(1, i as f64, 1.0, &[tone(i as f64, 0.0)]).unwrap();
        }
        s.push(1, 10000.1, 1.0, &[]).unwrap();
        let mut pcm = vec![1.0; frame(0.1)];
        s.render(&mut pcm).unwrap();
        assert_eq!(energy(&pcm), 0.0);
    }

    #[test]
    fn full_pwm_duty_is_silent_and_does_not_create_a_stuck_tone() {
        let mut s = scheduler();
        s.push(1, 0.1, 1.0, &[Tone { t_s: 0.0, hz: 2700.0, duty: 1.0 }]).unwrap();
        let mut pcm = vec![1.0; frame(0.2)];
        s.render(&mut pcm).unwrap();
        assert_eq!(energy(&pcm), 0.0);
    }

    #[test]
    fn rate_switch_keeps_an_already_scheduled_pulse_boundary() {
        let mut s = scheduler();
        s.push(1, 0.05, 1.0, &[tone(0.0, 2700.0)]).unwrap();
        s.push(1, 0.15, 0.5, &[tone(0.1, 0.0)]).unwrap();
        let mut pcm = vec![0.0; frame(0.3)];
        s.render(&mut pcm).unwrap();
        // First 50 ms remain at 1x. The following 50 simulated ms take 100 audio ms.
        assert!(energy(&pcm[frame(0.18)..frame(0.2)]) > 0.001);
        assert!(energy(&pcm[frame(0.24)..frame(0.29)]) < 1e-8);
    }

    #[test]
    fn noisy_rate_estimate_cannot_drift_away_from_the_audio_clock() {
        let mut s = scheduler();
        let mut pcm = [0.0; 480];
        for i in 1..=1000 {
            s.render(&mut pcm).unwrap();
            // The real producer and consumer both advance 10 ms. The sliding estimate is 2% high.
            s.push(1, i as f64 / 100.0, 1.02, &[]).unwrap();
        }
    }

    #[test]
    fn silent_clock_resync_preserves_the_next_pulse_edges_and_carrier() {
        let mut s = scheduler();
        let mut silent = [0.0; 480];
        for i in 1..=1000 {
            s.render(&mut silent).unwrap();
            let lagging_rate = if i % 20 < 10 { 0.9034 } else { 1.2 };
            s.push(1, i as f64 / 100.0, lagging_rate, &[]).unwrap();
        }
        s.push(1, 10.01, 1.0, &[]).unwrap();
        s.push(1, 10.02, 1.0, &[tone(10.015, 2700.0)]).unwrap();
        s.push(1, 10.09, 1.0, &[tone(10.088, 0.0)]).unwrap();
        let onset = s.events[0].frame - s.cursor;
        let end = s.events[1].frame - s.cursor;
        assert_eq!(end - onset, frame(0.073) as u64);
        let mut pcm = vec![0.0; frame(0.18)];
        s.render(&mut pcm).unwrap();
        assert_eq!(energy(&pcm[..onset as usize]), 0.0);
        let carrier = &pcm[onset as usize + frame(0.01)..onset as usize + frame(0.06)];
        let crossings = carrier.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((crossings as i32 - 135).abs() <= 2);
        assert!(energy(&pcm[end as usize + frame(0.03)..]) < 1e-8);
    }

    #[test]
    fn silent_source_does_not_retime_queued_pulse_edges() {
        let mut s = scheduler();
        s.push(1, 0.1, 1.0, &[tone(0.01, 2700.0), tone(0.083, 0.0)]).unwrap();
        let edges: Vec<_> = s.events.iter().map(|event| event.frame).collect();
        assert_eq!(s.source_tone.hz, 0.0);
        assert!(s.synth.silent());
        s.push(1, 0.11, 0.9034, &[]).unwrap();
        assert_eq!(s.events.iter().map(|event| event.frame).collect::<Vec<_>>(), edges);
    }

    #[test]
    fn duplicate_silent_source_time_does_not_refresh_the_frontier() {
        let mut s = scheduler();
        s.render(&mut [0.0; 2400]).unwrap();
        s.push(1, 0.0, 1.0, &[]).unwrap();
        assert_eq!(s.frontier, LEAD_FRAMES);
        s.render(&mut [0.0; 960]).unwrap();
        assert!(s.push(1, 0.0, 1.0, &[]).is_err());
    }

    #[test]
    fn silent_clock_resync_does_not_replay_a_late_onset() {
        let mut s = scheduler();
        for i in 1..=100 {
            s.render(&mut [0.0; 480]).unwrap();
            s.push(1, i as f64 / 100.0, 0.9034, &[]).unwrap();
        }
        s.render(&mut [0.0; 4800]).unwrap();
        assert!(s.push(1, 1.11, 0.9034, &[tone(1.001, 2700.0)]).is_err());
        assert!(s.events.is_empty());
    }

    fn ride(t_s: f64, speed_mps: f64) -> Ride {
        Ride {
            t_s,
            electrical_hz: 26.0 * speed_mps / (std::f64::consts::TAU * 0.2251),
            phase_current_a: 20.0,
            speed_mps,
            knob_hz: 15.0 * speed_mps / (std::f64::consts::TAU * 0.2251),
            surface: euc_world::Material::Asphalt,
            wet: false,
        }
    }
    /// Feeds a free ride at 60 samples a second for `seconds`, rendering as it goes.
    fn ride_for(s: &mut Scheduler, from: f64, seconds: f64, speed: f64, tones: &[Tone]) -> Vec<f32> {
        let mut pcm = Vec::new();
        let steps = (seconds * 60.0).round() as usize;
        for i in 1..=steps {
            let t = from + i as f64 / 60.0;
            let mine: Vec<Tone> = tones.iter().copied().filter(|x| x.t_s > t - 1.0 / 60.0 && x.t_s <= t).collect();
            s.push_ride(1, t, 1.0, &mine, Some(ride(t, speed))).unwrap();
            let mut block = vec![0.0; 800];
            s.render(&mut block).unwrap();
            pcm.extend(block);
        }
        pcm
    }

    #[test]
    fn the_ride_sound_is_off_until_a_mix_turns_it_on_and_the_buzzer_plays_regardless() {
        let mut s = scheduler();
        let pcm = ride_for(&mut s, 0.0, 1.0, 10.0, &[tone(0.7, 2700.0), tone(0.8, 0.0)]);
        assert_eq!(energy(&pcm[frame(0.3)..frame(0.7)]), 0.0);
        assert!(energy(&pcm[frame(0.72)..frame(0.8)]) > 1e-3);
    }

    #[test]
    fn a_free_ride_sounds_on_simulated_time_and_quiets_when_its_samples_stop() {
        let mut s = riding();
        let pcm = ride_for(&mut s, 0.0, 1.0, 10.0, &[]);
        // Nothing before the lead, then the bed.
        assert_eq!(energy(&pcm[..LEAD_FRAMES as usize - 1]), 0.0);
        assert!(energy(&pcm[frame(0.5)..]) > 1e-6);
        // A batch with no ride sample (a scenario run, say) quiets it; it never errors.
        let mut tail = Vec::new();
        for i in 1..=30 {
            s.push(1, 1.0 + i as f64 / 60.0, 1.0, &[]).unwrap();
            let mut block = vec![0.0; 800];
            s.render(&mut block).unwrap();
            tail.extend(block);
        }
        assert!(energy(&tail[tail.len() - 4800..]) < 1e-9);
    }

    #[test]
    fn the_master_volume_scales_both_voices_and_a_hidden_window_quiets_only_the_bed() {
        let loud = {
            let mut s = riding();
            let pcm = ride_for(&mut s, 0.0, 1.0, 10.0, &[]);
            energy(&pcm[frame(0.5)..])
        };
        let mut s = riding();
        s.mix(0.5, true);
        let pcm = ride_for(&mut s, 0.0, 1.0, 10.0, &[]);
        let half = energy(&pcm[frame(0.5)..]);
        assert!((half / loud - 0.25).abs() < 0.05, "half {half} loud {loud}");
        // Hidden: the bed fades out over 0.3 s; a beep still sounds.
        let mut s = riding();
        s.mix(1.0, false);
        let pcm = ride_for(&mut s, 0.0, 1.0, 10.0, &[tone(0.7, 2700.0), tone(0.8, 0.0)]);
        assert!(energy(&pcm[frame(0.4)..frame(0.7)]) < 1e-12);
        assert!(energy(&pcm[frame(0.72)..frame(0.8)]) > 1e-3);
    }

    #[test]
    fn a_ride_sample_outside_its_batch_or_out_of_range_is_refused() {
        let mut s = scheduler();
        assert!(s.push_ride(1, 0.1, 1.0, &[], Some(ride(0.2, 5.0))).is_err());
        assert!(s.push_ride(1, 0.1, 1.0, &[], Some(Ride { speed_mps: f64::NAN, ..ride(0.1, 5.0) })).is_err());
        s.push_ride(1, 0.1, 1.0, &[], Some(ride(0.1, 5.0))).unwrap();
    }
}
