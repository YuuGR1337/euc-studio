//! Surface materials and their contact parameters.
//!
//! The friction and rolling resistance values are generic estimates for a pneumatic 3-inch EUC
//! tire, taken from the usual published ranges for road tires. None of them is measured on a
//! wheel; they set the relative order of the surfaces, not a validated grip model.

use serde::{Deserialize, Serialize};

/// What the ground is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    /// Dense road asphalt.
    Asphalt,
    /// Worn, ravelled or patched asphalt.
    WornAsphalt,
    /// Cast concrete: slabs, gutters, curbs, driveways, stairs.
    Concrete,
    /// Small concrete pavers on sidewalks and plazas.
    PavingStones,
    /// Rounded stone setts with open joints.
    Cobblestones,
    /// Loose gravel.
    Gravel,
    /// Lawn.
    Grass,
    /// Packed dirt.
    Dirt,
    /// Road marking paint.
    Paint,
    /// Cast iron (manhole covers, grates).
    Metal,
}

impl Material {
    /// Every material, in code order.
    pub const ALL: [Self; 10] = [
        Self::Asphalt,
        Self::WornAsphalt,
        Self::Concrete,
        Self::PavingStones,
        Self::Cobblestones,
        Self::Gravel,
        Self::Grass,
        Self::Dirt,
        Self::Paint,
        Self::Metal,
    ];

    /// Sliding friction coefficient, dry and wet (estimates).
    pub const fn friction(self) -> [f64; 2] {
        match self {
            Self::Asphalt => [0.90, 0.60],
            Self::WornAsphalt => [0.80, 0.50],
            Self::Concrete => [0.85, 0.60],
            Self::PavingStones => [0.75, 0.50],
            Self::Cobblestones => [0.70, 0.40],
            Self::Gravel => [0.55, 0.50],
            Self::Grass => [0.45, 0.30],
            Self::Dirt => [0.60, 0.40],
            Self::Paint => [0.65, 0.35],
            Self::Metal => [0.55, 0.25],
        }
    }

    /// Rolling resistance coefficient (rolling force over normal load, estimates).
    pub const fn rolling_resistance(self) -> f64 {
        match self {
            Self::Asphalt | Self::Paint => 0.010,
            Self::WornAsphalt | Self::PavingStones => 0.012,
            Self::Concrete | Self::Metal => 0.009,
            Self::Cobblestones => 0.018,
            Self::Gravel => 0.035,
            Self::Grass => 0.060,
            Self::Dirt => 0.030,
        }
    }

    /// Stable one-byte code for digests and render payloads.
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// The ground's material and contact parameters at a point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    /// Material.
    pub material: Material,
    /// A wet patch or a puddle.
    pub wet: bool,
    /// Sliding friction coefficient.
    pub friction: f64,
    /// Rolling resistance coefficient.
    pub rolling_resistance: f64,
}

impl Surface {
    /// The parameters of a material, dry or wet. Wet ground also rolls a little harder.
    pub const fn of(material: Material, wet: bool) -> Self {
        let friction = material.friction();
        let rolling = material.rolling_resistance();
        Self {
            material,
            wet,
            friction: if wet { friction[1] } else { friction[0] },
            rolling_resistance: if wet { rolling * 1.1 } else { rolling },
        }
    }
}
