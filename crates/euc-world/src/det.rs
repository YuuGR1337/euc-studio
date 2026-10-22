//! Deterministic arithmetic: integer hashing, value noise, smooth blends and 2-D vectors.
//!
//! Every generation decision and every sampled height uses only IEEE-754 binary64 `+ - * /`,
//! `sqrt`, `floor`, `abs`, `min`, `max` and integer operations. These are correctly rounded (or
//! exact) on every target the engine ships for, and Rust neither contracts them into fused
//! multiply-adds nor reorders them. Transcendental functions (`sin`, `cos`, `exp`, `powf`,
//! `hypot`, `atan2`) call the platform's libm, whose last bit differs between macOS, Linux and
//! Windows, so this crate never calls them.

/// A 2-D point or vector, metres.
pub type V2 = [f64; 2];

const GOLDEN: u64 = 0x9e37_79b9_7f4a_7c15;

/// The `SplitMix64` finalizer: a bijective avalanche of 64 bits.
pub const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Hash of a seed, a domain salt and two integer coordinates.
pub const fn hash(seed: u64, salt: u64, a: i64, b: i64) -> u64 {
    let h = mix(seed ^ salt.wrapping_mul(GOLDEN));
    let h = mix(h ^ (a as u64).wrapping_mul(0xd6e8_feb8_6659_fd93));
    mix(h ^ (b as u64).wrapping_mul(0xa076_1d64_78bd_642f))
}

/// The top 53 bits of `h` as a uniform value in `[0, 1)`.
pub fn unit(h: u64) -> f64 {
    (h >> 11) as f64 * (1.0 / 9_007_199_254_740_992.0)
}

/// A short deterministic stream for the parameters of one item, seeded by its hash.
pub struct Stream(u64);

impl Stream {
    /// A stream seeded by `h`.
    pub const fn new(h: u64) -> Self {
        Self(h)
    }

    /// The next 64 random bits.
    pub const fn bits(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(GOLDEN);
        mix(self.0)
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        unit(self.bits())
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// Uniform in `[-1, 1)`.
    pub fn signed(&mut self) -> f64 {
        self.unit() * 2.0 - 1.0
    }

    /// An integer in `lo..=hi`.
    pub fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.bits() % (hi - lo + 1) as u64) as i64
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
}

/// Quintic fade `6t⁵ − 15t⁴ + 10t³`: zero first and second derivatives at 0 and 1.
pub fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Cubic smoothstep from 0 at `e0` to 1 at `e1`.
pub fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Linear interpolation.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Polynomial smooth minimum: exactly `min(a, b)` once `|a − b| ≥ k`, at most `k/4` below it.
pub fn smin(a: f64, b: f64, k: f64) -> f64 {
    let h = (k - (a - b).abs()).max(0.0) / k;
    if h == 0.0 { a.min(b) } else { a.min(b) - h * h * k * 0.25 }
}

/// Value noise in `[-1, 1]` with quintic interpolation on a lattice of `wavelength` metres.
///
/// Its slope is at most `3.75 / wavelength` per unit amplitude along each axis.
pub fn value_noise(seed: u64, salt: u64, p: V2, wavelength: f64) -> f64 {
    let (gx, gy) = (p[0] / wavelength, p[1] / wavelength);
    let (fx, fy) = (gx.floor(), gy.floor());
    let (ix, iy) = (fx as i64, fy as i64);
    let (tx, ty) = (fade(gx - fx), fade(gy - fy));
    let corner = |dx: i64, dy: i64| unit(hash(seed, salt, ix + dx, iy + dy)) * 2.0 - 1.0;
    let bottom = lerp(corner(0, 0), corner(1, 0), tx);
    let top = lerp(corner(0, 1), corner(1, 1), tx);
    lerp(bottom, top, ty)
}

/// Vector sum.
pub fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}

/// Vector difference.
pub fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}

/// Scaled vector.
pub fn scale(a: V2, s: f64) -> V2 {
    [a[0] * s, a[1] * s]
}

/// Dot product.
pub fn dot(a: V2, b: V2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// Z of the cross product: positive when `b` lies to the left of `a`.
pub fn cross(a: V2, b: V2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// Euclidean length (`sqrt`, never `hypot`).
pub fn len(a: V2) -> f64 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

/// Unit vector, or `fallback` for a vector shorter than 1 µm.
pub fn norm_or(a: V2, fallback: V2) -> V2 {
    let l = len(a);
    if l < 1e-6 { fallback } else { [a[0] / l, a[1] / l] }
}

/// The vector turned 90° to the left.
pub const fn perp(a: V2) -> V2 {
    [-a[1], a[0]]
}

/// Distance from `p` to segment `ab`, and the clamped parameter of the closest point.
pub fn segment_distance(p: V2, a: V2, b: V2) -> (f64, f64) {
    let ab = sub(b, a);
    let l2 = dot(ab, ab);
    let t = if l2 > 0.0 { (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    (len(sub(p, add(a, scale(ab, t)))), t)
}

/// An axis-aligned rectangle, metres.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    /// Minimum x and y.
    pub min: V2,
    /// Maximum x and y.
    pub max: V2,
}

impl Rect {
    /// The rectangle from two corners.
    pub const fn new(min: V2, max: V2) -> Self {
        Self { min, max }
    }

    /// An empty accumulator for [`Self::include`].
    pub const fn empty() -> Self {
        Self { min: [f64::INFINITY; 2], max: [f64::NEG_INFINITY; 2] }
    }

    /// Grows the rectangle to contain `p`.
    pub fn include(&mut self, p: V2) {
        self.min = [self.min[0].min(p[0]), self.min[1].min(p[1])];
        self.max = [self.max[0].max(p[0]), self.max[1].max(p[1])];
    }

    /// The rectangle grown by `m` on every side.
    #[must_use]
    pub fn grow(self, m: f64) -> Self {
        Self { min: [self.min[0] - m, self.min[1] - m], max: [self.max[0] + m, self.max[1] + m] }
    }

    /// Closed-interval overlap.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.min[0] <= other.max[0]
            && other.min[0] <= self.max[0]
            && self.min[1] <= other.max[1]
            && other.min[1] <= self.max[1]
    }

    /// Whether `p` lies inside or on the border.
    pub fn contains(&self, p: V2) -> bool {
        p[0] >= self.min[0] && p[0] <= self.max[0] && p[1] >= self.min[1] && p[1] <= self.max[1]
    }

    /// Distance from `p` to the rectangle (0 inside).
    pub fn distance(&self, p: V2) -> f64 {
        let dx = (self.min[0] - p[0]).max(p[0] - self.max[0]).max(0.0);
        let dy = (self.min[1] - p[1]).max(p[1] - self.max[1]).max(0.0);
        (dx * dx + dy * dy).sqrt()
    }

    /// Shortest distance between two rectangles (0 when they overlap).
    pub fn gap(&self, other: &Self) -> f64 {
        let dx = (self.min[0] - other.max[0]).max(other.min[0] - self.max[0]).max(0.0);
        let dy = (self.min[1] - other.max[1]).max(other.min[1] - self.max[1]).max(0.0);
        (dx * dx + dy * dy).sqrt()
    }
}

/// FNV-1a over bytes, for digests and golden tests (not cryptographic).
pub struct Digest(pub u64);

impl Digest {
    /// A digest seeded with a domain string.
    pub fn new(domain: &[u8]) -> Self {
        let mut d = Self(0xcbf2_9ce4_8422_2325);
        d.bytes(domain);
        d
    }

    /// Adds raw bytes.
    pub fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    /// Adds a float by its bits, with negative zero folded into zero.
    pub fn float(&mut self, value: f64) {
        let bits = value.to_bits();
        self.bytes(&(if bits << 1 == 0 { 0 } else { bits }).to_le_bytes());
    }

    /// Adds an integer.
    pub fn int(&mut self, value: i64) {
        self.bytes(&value.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smin_is_exact_min_outside_its_blend() {
        assert_eq!(smin(1.0, 5.0, 4.0).to_bits(), 1.0f64.to_bits());
        assert!(smin(1.0, 1.0, 4.0) < 1.0 && smin(1.0, 1.0, 4.0) >= 0.0);
    }

    #[test]
    fn value_noise_stays_in_range_and_is_continuous() {
        let mut previous = value_noise(7, 1, [0.0, 0.3], 10.0);
        for i in 1..2000 {
            let v = value_noise(7, 1, [f64::from(i) * 0.01, 0.3], 10.0);
            assert!((-1.0..=1.0).contains(&v));
            assert!((v - previous).abs() <= 3.75 / 10.0 * 2.0 * 0.01 + 1e-12);
            previous = v;
        }
    }
}
