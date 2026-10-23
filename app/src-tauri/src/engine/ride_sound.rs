//! The ride bed: what a rider hears from the wheel besides its buzzer, synthesized sample by
//! sample from the simulator's own signals (Free Ride). Nothing here is a recording or a canned
//! effect, and nothing sounds unless a signal says so:
//!
//! - the motor: a whine at the electrical frequency (pole pairs × wheel revolutions per second),
//!   as loud as the phase current, low (a real wheel is nearly silent);
//! - the tyre: rolling noise from the wheel speed, in the band and grain of the ground material
//!   under the wheel (euc-world), joint ticks on pavers and setts, a hiss when wet, and the hum of
//!   the tread's knob repeats;
//! - the wind: pink noise in a band that rises with the airspeed (the sim has no wind, so the
//!   airspeed is the wheel's speed).
//!
//! Methods after EUC Thrills' audio (src/audio/AudioEngine.ts, MIT, by VibezZzCoder): a band-pass
//! on pink noise sweeping about 240 Hz to 1.2 kHz with speed for the wind, per-surface noise bands
//! for the tyre with a 0.2 s crossfade between surfaces, and the ride ducked by 0.22 under each
//! beep. None of its recordings is used.

use euc_world::Material;
use std::f64::consts::{PI, TAU};

const SAMPLE_RATE: f64 = 48_000.0;
/// The bed's targets glide over this, s: the host sends one sample every 1/60 s.
const GLIDE_S: f64 = 0.02;
/// Material changes crossfade over this, s.
const CROSSFADE_S: f64 = 0.2;
/// Filter coefficients follow their targets every this many samples.
const RETUNE_EVERY: u32 = 16;

/// The wind's band-pass centre at rest and at `WIND_TOP_MPS`, Hz, and its loudness.
const WIND_HZ: [f64; 2] = [240.0, 1200.0];
const WIND_TOP_MPS: f64 = 25.0;
const WIND_FROM_MPS: f64 = 4.0;
const WIND_GAIN: f64 = 0.3;
/// The tyre's loudness at `TYRE_FULL_MPS`.
const TYRE_GAIN: f64 = 0.1;
const TYRE_FULL_MPS: f64 = 12.0;
/// The knob hum's loudness at `KNOB_FULL_MPS`.
const KNOB_GAIN: f64 = 0.012;
const KNOB_FULL_MPS: f64 = 8.0;
/// The motor's loudness at `MOTOR_FULL_A` of phase current: low, a real wheel is nearly silent.
const MOTOR_GAIN: f64 = 0.03;
const MOTOR_FULL_A: f64 = 80.0;
/// Below this phase current the motor is not heard, A.
const MOTOR_FROM_A: f64 = 2.0;
/// The ride ducks by this under a beep, with these attack and release times, s.
const DUCK: f64 = 0.22;
const DUCK_ATTACK_S: f64 = 0.03;
const DUCK_RELEASE_S: f64 = 0.3;

/// One sample of what the ride sounds like, from the simulator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ride {
    /// Simulated time of the sample, s.
    pub t_s: f64,
    /// The motor's electrical frequency, Hz: pole pairs × wheel revolutions per second.
    pub electrical_hz: f64,
    /// The phase current's magnitude (|iq|), A.
    pub phase_current_a: f64,
    /// Ground speed, m/s; with no wind in the sim it is also the airspeed.
    pub speed_mps: f64,
    /// The tread's knob repeats meeting the ground per second, Hz.
    pub knob_hz: f64,
    /// The ground under the wheel.
    pub surface: Material,
    pub wet: bool,
}

impl Ride {
    /// Finite and non-negative, every frequency under the output's Nyquist limit.
    pub fn valid(&self) -> bool {
        let finite = [self.t_s, self.electrical_hz, self.phase_current_a, self.speed_mps, self.knob_hz];
        finite.iter().all(|v| v.is_finite() && *v >= 0.0)
            && self.electrical_hz < SAMPLE_RATE / 2.0
            && self.knob_hz < SAMPLE_RATE / 2.0
    }
}

/// How a material sounds under the tyre: the noise band's centre (Hz) and Q, its loudness, the
/// joint spacing that ticks (m, 0 for none), how much gravel-like crackle, and how much of the
/// knob hum the ground lets through.
#[derive(Debug, Clone, Copy)]
struct Voice {
    hz: f64,
    q: f64,
    level: f64,
    joints_m: f64,
    crackle: f64,
    knobs: f64,
}

fn voice(material: Material) -> Voice {
    let v = |hz, q, level, joints_m, crackle, knobs| Voice { hz, q, level, joints_m, crackle, knobs };
    match material {
        Material::Asphalt => v(1500.0, 0.8, 1.0, 0.0, 0.0, 1.0),
        Material::WornAsphalt => v(1100.0, 0.8, 1.15, 0.0, 0.1, 0.9),
        Material::Concrete => v(1800.0, 0.9, 0.9, 0.0, 0.0, 1.0),
        // Small pavers, about 20 cm, and setts, 14 cm (euc-world's chunk.rs), tick at their joints.
        Material::PavingStones => v(1200.0, 0.8, 1.0, 0.2, 0.0, 0.8),
        Material::Cobblestones => v(650.0, 0.7, 1.4, 0.14, 0.0, 0.6),
        Material::Gravel => v(2600.0, 0.6, 1.4, 0.0, 1.0, 0.5),
        Material::Grass => v(450.0, 0.7, 0.55, 0.0, 0.0, 0.3),
        Material::Dirt => v(750.0, 0.7, 0.9, 0.0, 0.2, 0.4),
        Material::Paint => v(1900.0, 1.0, 0.75, 0.0, 0.0, 1.0),
        Material::Metal => v(900.0, 2.5, 1.1, 0.0, 0.0, 1.0),
    }
}

/// A one-pole glide toward a target: `rate` is the per-sample blend.
fn glide(value: &mut f64, target: f64, rate: f64) {
    *value += (target - *value) * rate;
}
fn blend(seconds: f64) -> f64 {
    1.0 - (-1.0 / (SAMPLE_RATE * seconds)).exp()
}
fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A constant-peak band-pass biquad (RBJ), or a high-pass with `high`.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}
impl Biquad {
    fn band(hz: f64, q: f64) -> Self {
        let mut f = Self::default();
        f.tune_band(hz, q);
        f
    }
    fn tune_band(&mut self, hz: f64, q: f64) {
        let w = TAU * hz.clamp(20.0, SAMPLE_RATE * 0.45) / SAMPLE_RATE;
        let alpha = w.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        self.b0 = alpha / a0;
        self.b1 = 0.0;
        self.b2 = -alpha / a0;
        self.a1 = -2.0 * w.cos() / a0;
        self.a2 = (1.0 - alpha) / a0;
    }
    fn high(hz: f64) -> Self {
        let w = TAU * hz / SAMPLE_RATE;
        let alpha = w.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let c = w.cos();
        Self {
            b0: (1.0 + c) / 2.0 / a0,
            b1: -(1.0 + c) / a0,
            b2: (1.0 + c) / 2.0 / a0,
            a1: -2.0 * c / a0,
            a2: (1.0 - alpha) / a0,
            ..Self::default()
        }
    }
    fn next(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// White noise (xorshift32) and Paul Kellet's economy pink filter on it.
#[derive(Debug, Clone, Copy)]
struct Noise {
    state: u32,
    b: [f64; 3],
}
impl Noise {
    fn white(&mut self) -> f64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        f64::from(x) / f64::from(u32::MAX) * 2.0 - 1.0
    }
    fn pink(&mut self, white: f64) -> f64 {
        self.b[0] = 0.99765 * self.b[0] + white * 0.099_046;
        self.b[1] = 0.96300 * self.b[1] + white * 0.296_516_4;
        self.b[2] = 0.57000 * self.b[2] + white * 1.052_691_3;
        (self.b[0] + self.b[1] + self.b[2] + white * 0.1848) * 0.2
    }
}

/// The bed's parameters, gliding toward each sample's targets.
#[derive(Debug, Clone, Copy, Default)]
struct Params {
    electrical_hz: f64,
    motor: f64,
    speed_mps: f64,
    knob_hz: f64,
    wet: f64,
}

pub struct Bed {
    target: Params,
    now: Params,
    noise: Noise,
    wind: Biquad,
    /// The tyre's band now and the one fading out after a material change.
    tyre: [Biquad; 2],
    voices: [Voice; 2],
    /// 1 when only `tyre[0]` sounds; falls from 1 to 0 across a crossfade.
    fade: f64,
    material: Option<Material>,
    hiss: Biquad,
    ticks: Biquad,
    tick_phase: f64,
    tick_env: f64,
    motor_phase: f64,
    knob_phase: f64,
    duck: f64,
    count: u32,
    glide: f64,
}

impl Bed {
    pub fn new() -> Self {
        let asphalt = voice(Material::Asphalt);
        Self {
            target: Params::default(),
            now: Params::default(),
            noise: Noise { state: 0x9E37_79B9, b: [0.0; 3] },
            wind: Biquad::band(WIND_HZ[0], 0.9),
            tyre: [Biquad::band(asphalt.hz, asphalt.q); 2],
            voices: [asphalt; 2],
            fade: 1.0,
            material: None,
            hiss: Biquad::high(4000.0),
            ticks: Biquad::band(2200.0, 1.2),
            tick_phase: 0.0,
            tick_env: 0.0,
            motor_phase: 0.0,
            knob_phase: 0.0,
            duck: 1.0,
            count: 0,
            glide: blend(GLIDE_S),
        }
    }

    /// One sample of the ride: its targets, reached over `GLIDE_S`.
    pub fn set(&mut self, ride: &Ride) {
        let current = ride.phase_current_a.abs();
        let motor = if current < MOTOR_FROM_A { 0.0 } else { (current / MOTOR_FULL_A).min(1.0).powf(0.8) };
        self.target = Params {
            electrical_hz: ride.electrical_hz,
            motor,
            speed_mps: ride.speed_mps,
            knob_hz: ride.knob_hz,
            wet: if ride.wet { 1.0 } else { 0.0 },
        };
        if self.material != Some(ride.surface) {
            let next = voice(ride.surface);
            if self.material.is_some() {
                // The old band keeps playing in slot 1 while the new one fades in in slot 0.
                self.tyre[1] = self.tyre[0];
                self.voices[1] = self.voices[0];
                self.fade = 0.0;
            }
            self.tyre[0] = Biquad::band(next.hz, next.q);
            self.voices[0] = next;
            self.material = Some(ride.surface);
        }
    }

    /// No more samples (the run is not a free ride, or it stopped): everything glides to rest.
    pub fn quiet(&mut self) {
        self.target = Params::default();
    }

    /// At rest: no targets and nothing left ringing.
    pub fn silent(&self) -> bool {
        self.target.speed_mps == 0.0 && self.target.motor == 0.0 && self.now.speed_mps < 1e-4 && self.now.motor < 1e-5
    }

    /// The next output sample; `beeping` is whether the buzzer sounds now (the ride ducks under it).
    pub fn next(&mut self, beeping: bool) -> f64 {
        let g = self.glide;
        glide(&mut self.now.electrical_hz, self.target.electrical_hz, g);
        glide(&mut self.now.motor, self.target.motor, g);
        glide(&mut self.now.speed_mps, self.target.speed_mps, g);
        glide(&mut self.now.knob_hz, self.target.knob_hz, g);
        glide(&mut self.now.wet, self.target.wet, g);
        let duck_target = if beeping { 1.0 - DUCK } else { 1.0 };
        let duck_rate = blend(if duck_target < self.duck { DUCK_ATTACK_S } else { DUCK_RELEASE_S });
        glide(&mut self.duck, duck_target, duck_rate);
        let v = self.now.speed_mps;
        if self.count.is_multiple_of(RETUNE_EVERY) {
            let reach = (v / WIND_TOP_MPS).clamp(0.0, 1.0);
            self.wind.tune_band(WIND_HZ[0] * (WIND_HZ[1] / WIND_HZ[0]).powf(reach), 0.9);
        }
        self.count = self.count.wrapping_add(1);
        if self.fade < 1.0 {
            self.fade = (self.fade + 1.0 / (SAMPLE_RATE * CROSSFADE_S)).min(1.0);
        }

        let white = self.noise.white();
        let pink = self.noise.pink(white);

        let wind = WIND_GAIN * smoothstep(WIND_FROM_MPS, WIND_TOP_MPS, v).powf(1.7) * self.wind.next(pink);

        let rolling = (v / TYRE_FULL_MPS).min(1.6).powf(1.2);
        // Equal power across a material change.
        let (new, old) = ((self.fade * PI / 2.0).sin(), (self.fade * PI / 2.0).cos());
        let mut tyre = 0.0;
        for (slot, weight) in [(0, new), (1, old)] {
            if weight < 1e-4 {
                continue;
            }
            let voice = self.voices[slot];
            let band = self.tyre[slot].next(white);
            // Gravel crackles: sparse sharp grains, more often the faster the wheel turns.
            let crackle = if voice.crackle > 0.0 && self.noise.white().abs() > 1.0 - 0.004 * v.min(10.0) {
                voice.crackle * 3.0 * self.noise.white()
            } else {
                0.0
            };
            tyre += weight * voice.level * (band + crackle);
        }
        tyre *= TYRE_GAIN * rolling;

        // Joint ticks: one short burst each time the wheel crosses a joint.
        let joints = self.voices[0].joints_m;
        if joints > 0.0 && v > 0.05 {
            self.tick_phase += v / joints / SAMPLE_RATE;
            if self.tick_phase >= 1.0 {
                self.tick_phase -= 1.0;
                self.tick_env = 1.0;
            }
        }
        self.tick_env *= (-1.0 / (SAMPLE_RATE * 0.003)).exp();
        let ticks = 0.5 * TYRE_GAIN * (v / 6.0).min(1.0) * self.tick_env * self.ticks.next(white) * new;

        let hiss = 0.4 * TYRE_GAIN * self.now.wet * (v / TYRE_FULL_MPS).min(1.5) * self.hiss.next(white);

        // The tread's knob repeats, and the stagger's second partial, under the rolling noise.
        self.knob_phase = (self.knob_phase + self.now.knob_hz / SAMPLE_RATE).fract();
        let knob_wave = (TAU * self.knob_phase).sin() + 0.5 * (2.0 * TAU * self.knob_phase).sin();
        let knobs = KNOB_GAIN * (v / KNOB_FULL_MPS).min(1.0) * self.voices[0].knobs * knob_wave;

        // The motor: the electrical frequency, and a little of its sixth harmonic (commutation).
        self.motor_phase = (self.motor_phase + self.now.electrical_hz / SAMPLE_RATE).fract();
        let sixth = if 6.0 * self.now.electrical_hz < SAMPLE_RATE * 0.45 {
            0.3 * (6.0 * TAU * self.motor_phase).sin()
        } else {
            0.0
        };
        let motor = MOTOR_GAIN * self.now.motor * ((TAU * self.motor_phase).sin() + sixth);

        let bed = (wind + tyre + ticks + hiss + knobs + motor) * self.duck;
        // A soft limit: the bed never clips the buzzer above it.
        0.5 * (bed / 0.5).tanh()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ride(speed_mps: f64, current_a: f64, surface: Material) -> Ride {
        let wheel_rev_s = speed_mps / (TAU * 0.2251);
        Ride {
            t_s: 0.0,
            electrical_hz: 26.0 * wheel_rev_s,
            phase_current_a: current_a,
            speed_mps,
            knob_hz: 15.0 * wheel_rev_s,
            surface,
            wet: false,
        }
    }
    fn run(bed: &mut Bed, seconds: f64, beeping: bool) -> Vec<f64> {
        (0..(seconds * SAMPLE_RATE) as usize).map(|_| bed.next(beeping)).collect()
    }
    fn rms(samples: &[f64]) -> f64 {
        (samples.iter().map(|x| x * x).sum::<f64>() / samples.len() as f64).sqrt()
    }

    #[test]
    fn silent_at_rest_and_after_the_ride_stops() {
        let mut bed = Bed::new();
        bed.set(&ride(0.0, 0.0, Material::Asphalt));
        assert!(rms(&run(&mut bed, 0.2, false)) < 1e-9);
        bed.set(&ride(8.0, 20.0, Material::Asphalt));
        assert!(rms(&run(&mut bed, 0.3, false)) > 1e-3);
        bed.quiet();
        let tail = run(&mut bed, 0.5, false);
        assert!(rms(&tail[tail.len() - 4800..]) < 1e-4);
        assert!(bed.silent());
    }

    #[test]
    fn the_motor_sings_at_the_electrical_frequency() {
        // Motor alone: a current with no speed isolates it from the wind and the tyre.
        let mut bed = Bed::new();
        let hz = 203.0;
        bed.set(&Ride { electrical_hz: hz, phase_current_a: 60.0, ..ride(0.0, 0.0, Material::Asphalt) });
        let pcm = run(&mut bed, 1.0, false);
        let settled = &pcm[pcm.len() / 2..];
        // Strip the sixth harmonic: count the fundamental's upward zero crossings after a crude
        // low-pass (a 1 ms moving average).
        let width = (SAMPLE_RATE * 0.001) as usize;
        let smooth: Vec<f64> = settled.windows(width).map(|w| w.iter().sum::<f64>() / width as f64).collect();
        let crossings = smooth.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        let measured = crossings as f64 / (smooth.len() as f64 / SAMPLE_RATE);
        assert!((measured - hz).abs() < 4.0, "measured {measured} Hz");
        // Low: a real wheel is nearly silent.
        assert!(rms(settled) < 0.03);
        // No current, no whine.
        bed.set(&Ride { electrical_hz: hz, phase_current_a: 1.0, ..ride(0.0, 0.0, Material::Asphalt) });
        let quiet = run(&mut bed, 0.5, false);
        assert!(rms(&quiet[quiet.len() - 4800..]) < 1e-4);
    }

    #[test]
    fn louder_with_speed_and_ducked_under_a_beep() {
        let level = |speed: f64| {
            let mut bed = Bed::new();
            bed.set(&ride(speed, 0.0, Material::Asphalt));
            let pcm = run(&mut bed, 1.0, false);
            rms(&pcm[pcm.len() / 2..])
        };
        let (slow, fast) = (level(3.0), level(15.0));
        assert!(fast > 3.0 * slow, "slow {slow} fast {fast}");
        let mut bed = Bed::new();
        bed.set(&ride(10.0, 0.0, Material::Asphalt));
        let open = run(&mut bed, 1.0, false);
        let ducked = run(&mut bed, 0.5, true);
        let ratio = rms(&ducked[ducked.len() / 2..]) / rms(&open[open.len() / 2..]);
        assert!((ratio - (1.0 - DUCK)).abs() < 0.08, "ratio {ratio}");
    }

    #[test]
    fn each_material_sounds_and_a_change_crossfades_without_a_jump() {
        for material in Material::ALL {
            let mut bed = Bed::new();
            bed.set(&ride(6.0, 0.0, material));
            let pcm = run(&mut bed, 0.5, false);
            assert!(pcm.iter().all(|x| x.is_finite() && x.abs() <= 0.5), "{material:?}");
            assert!(rms(&pcm[pcm.len() / 2..]) > 1e-3, "{material:?}");
        }
        let mut bed = Bed::new();
        bed.set(&ride(6.0, 0.0, Material::Asphalt));
        let before = run(&mut bed, 0.5, false);
        bed.set(&ride(6.0, 0.0, Material::Cobblestones));
        let across = run(&mut bed, 0.25, false);
        let largest_step = |pcm: &[f64]| pcm.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f64::max);
        assert!(largest_step(&across) < 3.0 * largest_step(&before[before.len() / 2..]).max(0.01));
    }

    #[test]
    fn a_sample_out_of_range_is_refused() {
        assert!(ride(5.0, 10.0, Material::Asphalt).valid());
        assert!(!Ride { speed_mps: f64::NAN, ..ride(5.0, 10.0, Material::Asphalt) }.valid());
        assert!(!Ride { electrical_hz: 30_000.0, ..ride(5.0, 10.0, Material::Asphalt) }.valid());
        assert!(!Ride { phase_current_a: -1.0, ..ride(5.0, 10.0, Material::Asphalt) }.valid());
    }
}
