//! Segment parameters of the lateral rider (`articulated_lateral_v1`): published averages for
//! adult men, scaled to the rider's height and mass. Not anyone's body.
//!
//! - Segments: de Leva, P. (1996), "Adjustments to Zatsiorsky-Seluyanov's segment inertia
//!   parameters", Journal of Biomechanics 29(9), 1223-1230, male values (stature 1741 mm).
//! - Hip joint centres: Harrington, M. E. et al. (2007), "Prediction of the hip joint centre in
//!   adults, children, and patients with cerebral palsy based on magnetic resonance imaging",
//!   Journal of Biomechanics 40(3), 595-602: 0.33 × pelvis width + 7.3 mm from the midline.
//! - Joint ranges: the American Academy of Orthopaedic Surgeons' normal active ranges.

/// One leg segment, as fractions of the whole body.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Segment {
    /// Mass, of the body mass.
    pub mass: f64,
    /// Length, of the stature: hip to knee, knee to ankle, heel to toe.
    pub length: f64,
    /// Centre of mass from the proximal joint, of the length.
    pub com: f64,
    /// Radii of gyration about the centre of mass, of the length: about the sagittal
    /// (front-back), transverse (side-side) and longitudinal (along the segment) axes.
    pub gyration: [f64; 3],
}

/// de Leva's male stature, metres: the lengths below are fractions of it.
const STATURE_M: f64 = 1.741;

/// The thigh, hip joint centre to knee joint centre.
pub(crate) const THIGH: Segment =
    Segment { mass: 0.1416, length: 0.4222 / STATURE_M, com: 0.4095, gyration: [0.329, 0.329, 0.149] };

/// The shank, knee joint centre to lateral malleolus.
pub(crate) const SHANK: Segment =
    Segment { mass: 0.0433, length: 0.4340 / STATURE_M, com: 0.4459, gyration: [0.255, 0.249, 0.103] };

/// The foot, heel to the tip of the longest toe.
pub(crate) const FOOT: Segment =
    Segment { mass: 0.0137, length: 0.2581 / STATURE_M, com: 0.4415, gyration: [0.257, 0.245, 0.124] };

/// A hip joint centre's distance from the midline, of the stature: Harrington's regression with
/// a 250 mm pelvis width (between the anterior superior iliac spines) for a 1.78 m man,
/// 90 mm (ESTIMATE: the pelvis width is a typical adult value, not a measurement).
pub(crate) const HIP_HALF_WIDTH: f64 = (0.33 * 0.250 + 0.0073) / 1.78;

/// A shoulder's distance from the midline, of the stature (ESTIMATE: half a typical adult
/// shoulder width); places the fixed arms, which carry no mass of their own.
pub(crate) const SHOULDER_HALF_WIDTH: f64 = 0.115;

/// Normal ranges of motion (AAOS), radians.
pub(crate) mod range {
    /// Hip flexion, 120°.
    pub const HIP_FLEXION: f64 = 120.0 * std::f64::consts::PI / 180.0;
    /// Hip extension, 30°.
    pub const HIP_EXTENSION: f64 = 30.0 * std::f64::consts::PI / 180.0;
    /// Hip abduction, 45°.
    pub const HIP_ABDUCTION: f64 = 45.0 * std::f64::consts::PI / 180.0;
    /// Hip adduction, 30°.
    pub const HIP_ADDUCTION: f64 = 30.0 * std::f64::consts::PI / 180.0;
    /// Hip internal and external rotation, 45° each way.
    pub const HIP_ROTATION: f64 = 45.0 * std::f64::consts::PI / 180.0;
    /// Knee flexion, 135°.
    pub const KNEE_FLEXION: f64 = 135.0 * std::f64::consts::PI / 180.0;
    /// Ankle inversion, 35°.
    pub const ANKLE_INVERSION: f64 = 35.0 * std::f64::consts::PI / 180.0;
    /// Ankle eversion, 15°.
    pub const ANKLE_EVERSION: f64 = 15.0 * std::f64::consts::PI / 180.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn de_leva_legs_leave_three_fifths_of_the_mass_to_the_upper_body() {
        let legs = 2.0 * (THIGH.mass + SHANK.mass + FOOT.mass);
        assert!((1.0 - legs - 0.6028).abs() < 1e-9, "{legs}");
        // A 1.78 m adult: 43 cm thigh, 44 cm shank, 9 cm from the midline to each hip.
        assert!((THIGH.length * 1.78 - 0.4317).abs() < 1e-3);
        assert!((SHANK.length * 1.78 - 0.4437).abs() < 1e-3);
        assert!((HIP_HALF_WIDTH * 1.78 - 0.0898).abs() < 1e-4);
    }
}
