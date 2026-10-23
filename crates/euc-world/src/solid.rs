//! Oriented boxes for vertical faces: curbs, stair steps, plaza walls, ramp sides, buildings and
//! props. The physics keeps a pool of box geoms and moves these into it near the wheel.

// Geometry reads like its formulas: a, b (ends), x, y, z (axes).
#![allow(clippy::many_single_char_names)]

use serde::{Deserialize, Serialize};

use crate::det::{Rect, V2};
use crate::features::PropKind;

/// What a solid stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "prop")]
pub enum SolidKind {
    /// A curb stone along a sidewalk edge.
    Curb,
    /// One stair step.
    Step,
    /// A raised plaza's platform (its sides are retaining walls).
    Platform,
    /// A ramp slab; its top is the ramp surface.
    Ramp,
    /// A building.
    Building,
    /// A prop's collision shape.
    Prop(PropKind),
}

/// An oriented box. `axes` are the box's unit x, y and z axes in world coordinates (rows),
/// right-handed; `half` are the half extents along them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    /// Stable identity, shared by every chunk that lists the solid.
    pub id: u64,
    /// What it is.
    pub kind: SolidKind,
    /// Centre, metres.
    pub centre: [f64; 3],
    /// Half extents along `axes`, metres.
    pub half: [f64; 3],
    /// Unit axes (rows): x, y, z.
    pub axes: [[f64; 3]; 3],
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit3(a: [f64; 3]) -> [f64; 3] {
    let l = dot3(a, a).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

impl Solid {
    /// An upright box over the rectangle `[u0, u1] × [v0, v1]` of the frame (`origin`, `u`, `v`)
    /// from `z0` up to `z1`. `u` and `v` must be unit and `u × v` must point up.
    #[allow(clippy::too_many_arguments)]
    pub fn upright(id: u64, kind: SolidKind, origin: V2, u: V2, v: V2, uv0: V2, uv1: V2, z: [f64; 2]) -> Self {
        let (cu, cv) = (f64::midpoint(uv0[0], uv1[0]), f64::midpoint(uv0[1], uv1[1]));
        Self {
            id,
            kind,
            centre: [origin[0] + u[0] * cu + v[0] * cv, origin[1] + u[1] * cu + v[1] * cv, f64::midpoint(z[0], z[1])],
            half: [(uv1[0] - uv0[0]).abs() / 2.0, (uv1[1] - uv0[1]).abs() / 2.0, (z[1] - z[0]) / 2.0],
            axes: [[u[0], u[1], 0.0], [v[0], v[1], 0.0], [0.0, 0.0, 1.0]],
        }
    }

    /// A slab whose top face runs from `a` to `b` (3-D points on its centre line), `width`
    /// wide to the left of the travel direction `a → b` (`side` = +1) or to its right (−1),
    /// and `thickness` deep below the top face.
    pub fn slab(id: u64, kind: SolidKind, a: [f64; 3], b: [f64; 3], width: f64, side: f64, thickness: f64) -> Self {
        let x = unit3([b[0] - a[0], b[1] - a[1], b[2] - a[2]]);
        let horizontal = unit3([-x[1], x[0], 0.0]);
        // y across (left of travel), z = x × y points up for a slab that is not vertical.
        let y = horizontal;
        let z = cross3(x, y);
        let length = dot3([b[0] - a[0], b[1] - a[1], b[2] - a[2]], x);
        let mid = [f64::midpoint(a[0], b[0]), f64::midpoint(a[1], b[1]), f64::midpoint(a[2], b[2])];
        let offset = side * width / 2.0;
        Self {
            id,
            kind,
            centre: [
                mid[0] + y[0] * offset - z[0] * thickness / 2.0,
                mid[1] + y[1] * offset - z[1] * thickness / 2.0,
                mid[2] + y[2] * offset - z[2] * thickness / 2.0,
            ],
            half: [length / 2.0, width / 2.0, thickness / 2.0],
            axes: [x, y, z],
        }
    }

    /// A point in the box frame.
    pub fn to_local(&self, p: [f64; 3]) -> [f64; 3] {
        let d = [p[0] - self.centre[0], p[1] - self.centre[1], p[2] - self.centre[2]];
        [dot3(d, self.axes[0]), dot3(d, self.axes[1]), dot3(d, self.axes[2])]
    }

    /// Whether `p` lies inside the box grown by `margin`.
    pub fn contains(&self, p: [f64; 3], margin: f64) -> bool {
        let l = self.to_local(p);
        (0..3).all(|k| l[k].abs() <= self.half[k] + margin)
    }

    /// Height of the top face above `p`, if `p` lies within the footprint grown by `margin`.
    pub fn top_at(&self, p: V2, margin: f64) -> Option<f64> {
        let z = self.axes[2];
        if z[2] <= 1e-6 {
            return None;
        }
        let top = [
            self.centre[0] + z[0] * self.half[2],
            self.centre[1] + z[1] * self.half[2],
            self.centre[2] + z[2] * self.half[2],
        ];
        let height = top[2] - (z[0] * (p[0] - top[0]) + z[1] * (p[1] - top[1])) / z[2];
        let l = self.to_local([p[0], p[1], height]);
        (l[0].abs() <= self.half[0] + margin && l[1].abs() <= self.half[1] + margin).then_some(height)
    }

    /// Horizontal bounds.
    pub fn bounds(&self) -> Rect {
        let mut extent = [0.0; 2];
        for (k, e) in extent.iter_mut().enumerate() {
            *e = (0..3).map(|a| (self.axes[a][k] * self.half[a]).abs()).sum::<f64>();
        }
        Rect::new(
            [self.centre[0] - extent[0], self.centre[1] - extent[1]],
            [self.centre[0] + extent[0], self.centre[1] + extent[1]],
        )
    }

    /// The orientation as a unit quaternion `[w, x, y, z]` (for MuJoCo and three.js).
    pub fn quaternion(&self) -> [f64; 4] {
        // m[r][c] = axes[c][r]: the axes are the rotation matrix's columns.
        let m = |r: usize, c: usize| self.axes[c][r];
        let trace = m(0, 0) + m(1, 1) + m(2, 2);
        let q = if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            [0.25 * s, (m(2, 1) - m(1, 2)) / s, (m(0, 2) - m(2, 0)) / s, (m(1, 0) - m(0, 1)) / s]
        } else if m(0, 0) > m(1, 1) && m(0, 0) > m(2, 2) {
            let s = (1.0 + m(0, 0) - m(1, 1) - m(2, 2)).sqrt() * 2.0;
            [(m(2, 1) - m(1, 2)) / s, 0.25 * s, (m(0, 1) + m(1, 0)) / s, (m(0, 2) + m(2, 0)) / s]
        } else if m(1, 1) > m(2, 2) {
            let s = (1.0 + m(1, 1) - m(0, 0) - m(2, 2)).sqrt() * 2.0;
            [(m(0, 2) - m(2, 0)) / s, (m(0, 1) + m(1, 0)) / s, 0.25 * s, (m(1, 2) + m(2, 1)) / s]
        } else {
            let s = (1.0 + m(2, 2) - m(0, 0) - m(1, 1)).sqrt() * 2.0;
            [(m(1, 0) - m(0, 1)) / s, (m(0, 2) + m(2, 0)) / s, (m(1, 2) + m(2, 1)) / s, 0.25 * s]
        };
        let norm = q.iter().map(|v| v * v).sum::<f64>().sqrt();
        q.map(|v| v / norm)
    }

    /// Highest point, metres.
    pub fn top(&self) -> f64 {
        self.centre[2] + (0..3).map(|a| (self.axes[a][2] * self.half[a]).abs()).sum::<f64>()
    }
}
