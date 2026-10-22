//! The road network: a jittered lattice of intersections joined by street rows and columns,
//! Catmull-Rom centrelines flattened into polylines, and the smooth road-edge distance field.
//!
//! Node `(i, j)` sits near `(64 i − 32, 64 j)`. A row edge joins `(i, j)` to `(i + 1, j)`, a
//! column edge `(i, j)` to `(i, j + 1)`. Every fourth row and column is an arterial and always
//! exists. Each 4 × 4 superblock keeps all of its local edges along one axis and some along the
//! other, so every node reaches an arterial along its row or column: the network is connected.
//! Everything here is a pure function of the seed and integer indices, never of generation order.

// Geometry reads like its formulas: p, a, b, d, t, m.
#![allow(clippy::many_single_char_names)]

use serde::{Deserialize, Serialize};

use crate::det::{self, Rect, Stream, V2, hash};
use crate::surface::Material;

/// Distance between neighbouring intersections, metres.
pub const PITCH_M: f64 = 64.0;
/// Every this many rows and columns is an arterial.
pub const ARTERIAL_EVERY: i64 = 4;
/// Full width of an arterial between curb faces, metres (four 3.5 m lanes).
pub const ARTERIAL_WIDTH_M: f64 = 14.0;
/// Range of full local street widths, metres.
pub const LOCAL_WIDTH_M: [f64; 2] = [7.0, 10.0];
/// Blend radius of the smooth union between crossing streets: the scale of curb corner radii, metres.
pub const CORNER_BLEND_M: f64 = 4.0;
/// The spawn street's centreline: y on row 0, straight for |x| ≤ 64 m. The spawn at the origin
/// is then 1.5 m right of it, inside the crown's level middle.
pub const SPAWN_ROAD_Y_M: f64 = 1.5;
/// Distance beyond which a street no longer changes heights: sidewalk width + blend + slack, metres.
pub const FIELD_REACH_M: f64 = 12.0;
/// Field value standing for "no street in reach", metres.
pub const FAR_M: f64 = 1.0e3;

const NODE_JITTER_M: f64 = 9.0;
const ARTERIAL_JITTER_M: f64 = 2.0;
const LOCAL_BULGE_M: f64 = 6.0;
const ARTERIAL_BULGE_M: f64 = 2.0;
/// Probability that a local edge on a superblock's sparse axis exists.
const LOCAL_KEEP: f64 = 0.6;
const SPAN_SEGMENTS: usize = 16;

const SALT_NODE: u64 = 0x101;
const SALT_MID: u64 = 0x102;
const SALT_WIDTH: u64 = 0x103;
const SALT_SUPER: u64 = 0x104;
const SALT_EDGE: u64 = 0x105;
const SALT_MATERIAL: u64 = 0x106;

/// Which way an edge runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// From node `(i, j)` to `(i + 1, j)`, roughly along +x.
    Row,
    /// From node `(i, j)` to `(i, j + 1)`, roughly along +y.
    Column,
}

/// One edge of the lattice. Orders rows before columns, then by `i`, then by `j`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EdgeId {
    /// Row or column.
    pub axis: Axis,
    /// Lattice column of the start node.
    pub i: i64,
    /// Lattice row of the start node.
    pub j: i64,
}

impl EdgeId {
    /// The start node.
    pub const fn start(self) -> (i64, i64) {
        (self.i, self.j)
    }

    /// The end node.
    pub const fn end(self) -> (i64, i64) {
        match self.axis {
            Axis::Row => (self.i + 1, self.j),
            Axis::Column => (self.i, self.j + 1),
        }
    }

    /// The street (whole row or column) this edge belongs to.
    pub const fn street(self) -> i64 {
        match self.axis {
            Axis::Row => self.j,
            Axis::Column => self.i,
        }
    }

    /// Whether the edge is part of an arterial.
    pub const fn arterial(self) -> bool {
        self.street().rem_euclid(ARTERIAL_EVERY) == 0
    }

    /// The neighbour one step back along the same street.
    const fn previous(self) -> Self {
        match self.axis {
            Axis::Row => Self { i: self.i - 1, ..self },
            Axis::Column => Self { j: self.j - 1, ..self },
        }
    }

    /// The neighbour one step forward along the same street.
    const fn next(self) -> Self {
        match self.axis {
            Axis::Row => Self { i: self.i + 1, ..self },
            Axis::Column => Self { j: self.j + 1, ..self },
        }
    }
}

/// Street class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreetClass {
    /// Four lanes, double centre line, asphalt.
    Arterial,
    /// Two lanes; asphalt, concrete slabs or cobblestones.
    Local,
}

/// One existing edge with its flattened centreline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    /// Lattice identity.
    pub id: EdgeId,
    /// Arterial or local.
    pub class: StreetClass,
    /// Half the width between curb faces, metres.
    pub half_width_m: f64,
    /// Road surface.
    pub material: Material,
    /// Sidewalk surface along this edge.
    pub sidewalk: Material,
    /// Centreline polyline from the start node to the end node.
    pub points: Vec<V2>,
    /// Arc length at each point from the start node, metres.
    pub s: Vec<f64>,
    /// Bounds of the centreline.
    pub bounds: Rect,
}

/// The nearest centreline point of the nearer street group.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Near {
    /// Index of the edge in the list the field was evaluated over.
    pub edge: usize,
    /// Arc length along the edge, metres.
    pub s: f64,
    /// Signed distance from the centreline, left of the edge direction positive, metres.
    pub across: f64,
    /// Unit tangent of the centreline there.
    pub tangent: V2,
    /// The closest centreline point.
    pub point: V2,
}

/// The road-edge distance field at a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadHit {
    /// Smooth signed distance to the curb face line: negative on the carriageway, metres.
    /// Exact up to [`FIELD_REACH_M`]; beyond it only its clamp to the sidewalk width is used.
    pub f: f64,
    /// Hard minimum over row edges of `distance − half width`.
    pub row: f64,
    /// Hard minimum over column edges of `distance − half width`.
    pub column: f64,
    /// Nearest centreline of the nearer group, if any street is in reach.
    pub near: Option<Near>,
    /// Nearest centreline of the other group, if any street of it is in reach.
    pub near_other: Option<Near>,
}

impl RoadHit {
    /// Value of the group the point does not belong to: how far the point is from crossing streets.
    pub fn other_group(&self) -> f64 {
        self.row.max(self.column)
    }
}

/// Lattice position of node `(i, j)`.
pub fn node(seed: u64, i: i64, j: i64) -> V2 {
    let base = [i as f64 * PITCH_M - PITCH_M / 2.0, j as f64 * PITCH_M];
    // The spawn street: row 0 stays straight from node -1 to node 2 (x from -96 to 96 m).
    if j == 0 && (-1..=2).contains(&i) {
        return [base[0], SPAWN_ROAD_Y_M];
    }
    let mut s = Stream::new(hash(seed, SALT_NODE, i, j));
    let jx = if i.rem_euclid(ARTERIAL_EVERY) == 0 { ARTERIAL_JITTER_M } else { NODE_JITTER_M };
    let jy = if j.rem_euclid(ARTERIAL_EVERY) == 0 { ARTERIAL_JITTER_M } else { NODE_JITTER_M };
    [base[0] + s.signed() * jx, base[1] + s.signed() * jy]
}

/// The midpoint control of an edge, pushed sideways by its bulge.
fn mid(seed: u64, id: EdgeId) -> V2 {
    let (a, b) = (node(seed, id.i, id.j), node(seed, id.end().0, id.end().1));
    let m = det::scale(det::add(a, b), 0.5);
    if id.axis == Axis::Row && id.j == 0 && (-1..=1).contains(&id.i) {
        return m;
    }
    let normal = det::perp(det::norm_or(det::sub(b, a), [1.0, 0.0]));
    let bulge = if id.arterial() { ARTERIAL_BULGE_M } else { LOCAL_BULGE_M };
    det::add(m, det::scale(normal, Stream::new(hash(seed, SALT_MID, id.i * 2 + id.axis as i64, id.j)).signed() * bulge))
}

/// Whether the superblock containing local edge `id` keeps all of its row edges.
fn rows_first(seed: u64, id: EdgeId) -> bool {
    hash(seed, SALT_SUPER, id.i.div_euclid(ARTERIAL_EVERY), id.j.div_euclid(ARTERIAL_EVERY)) & 1 == 0
}

/// Whether an edge exists.
pub fn exists(seed: u64, id: EdgeId) -> bool {
    if id.arterial() {
        return true;
    }
    let dense = match id.axis {
        Axis::Row => rows_first(seed, id),
        Axis::Column => !rows_first(seed, id),
    };
    dense || det::unit(hash(seed, SALT_EDGE, id.i * 2 + id.axis as i64, id.j)) < LOCAL_KEEP
}

/// Number of existing edges at node `(i, j)`.
pub fn degree(seed: u64, i: i64, j: i64) -> usize {
    [
        EdgeId { axis: Axis::Row, i: i - 1, j },
        EdgeId { axis: Axis::Row, i, j },
        EdgeId { axis: Axis::Column, i, j: j - 1 },
        EdgeId { axis: Axis::Column, i, j },
    ]
    .into_iter()
    .filter(|&id| exists(seed, id))
    .count()
}

/// Half the curb-to-curb width of the street an edge belongs to.
pub fn half_width(seed: u64, id: EdgeId) -> f64 {
    if id.arterial() {
        return ARTERIAL_WIDTH_M / 2.0;
    }
    let u = det::unit(hash(seed, SALT_WIDTH, id.axis as i64, id.street()));
    det::lerp(LOCAL_WIDTH_M[0], LOCAL_WIDTH_M[1], u) * 0.5
}

fn catmull_rom(p0: V2, p1: V2, p2: V2, p3: V2, t: f64) -> V2 {
    let (t2, t3) = (t * t, t * t * t);
    let axis = |k: usize| {
        0.5 * (2.0 * p1[k]
            + (p2[k] - p0[k]) * t
            + (2.0 * p0[k] - 5.0 * p1[k] + 4.0 * p2[k] - p3[k]) * t2
            + (3.0 * p1[k] - p0[k] - 3.0 * p2[k] + p3[k]) * t3)
    };
    [axis(0), axis(1)]
}

/// Builds an edge's geometry whether or not it exists (the caller checks [`exists`]).
pub fn build(seed: u64, id: EdgeId) -> Edge {
    let a = node(seed, id.i, id.j);
    let b = node(seed, id.end().0, id.end().1);
    let m = mid(seed, id);
    let before = mid(seed, id.previous());
    let after = mid(seed, id.next());
    let mut points = Vec::with_capacity(2 * SPAN_SEGMENTS + 1);
    for k in 0..=SPAN_SEGMENTS {
        points.push(catmull_rom(before, a, m, b, k as f64 / SPAN_SEGMENTS as f64));
    }
    for k in 1..=SPAN_SEGMENTS {
        points.push(catmull_rom(a, m, b, after, k as f64 / SPAN_SEGMENTS as f64));
    }
    // Exact node positions at both ends, so neighbouring edges share their endpoints bit for bit.
    points[0] = a;
    points[2 * SPAN_SEGMENTS] = b;
    let mut s = Vec::with_capacity(points.len());
    let mut bounds = Rect::empty();
    let mut total = 0.0;
    for (k, &p) in points.iter().enumerate() {
        if k > 0 {
            total += det::len(det::sub(p, points[k - 1]));
        }
        s.push(total);
        bounds.include(p);
    }
    let mut pick = Stream::new(hash(seed, SALT_MATERIAL, id.i * 2 + id.axis as i64, id.j));
    let (class, material) = if id.arterial() {
        (StreetClass::Arterial, Material::Asphalt)
    } else {
        let u = pick.unit();
        let material = if u < 0.15 {
            Material::Cobblestones
        } else if u < 0.27 {
            Material::Concrete
        } else {
            Material::Asphalt
        };
        (StreetClass::Local, material)
    };
    let sidewalk = if pick.unit() < 0.55 { Material::Concrete } else { Material::PavingStones };
    Edge { id, class, half_width_m: half_width(seed, id), material, sidewalk, points, s, bounds }
}

/// Every existing edge whose curb line comes within `reach` of `rect`, in [`EdgeId`] order.
pub fn edges_near(seed: u64, rect: Rect, reach: f64) -> Vec<Edge> {
    // Nodes stay within 9 m of the lattice, bulges add 6 m and the spline overshoots a few metres.
    let margin = reach + ARTERIAL_WIDTH_M / 2.0 + 25.0;
    let lo = |v: f64, offset: f64| ((v - margin + offset) / PITCH_M).floor() as i64 - 1;
    let hi = |v: f64, offset: f64| ((v + margin + offset) / PITCH_M).ceil() as i64 + 1;
    let (i0, i1) = (lo(rect.min[0], PITCH_M / 2.0), hi(rect.max[0], PITCH_M / 2.0));
    let (j0, j1) = (lo(rect.min[1], 0.0), hi(rect.max[1], 0.0));
    let mut edges = Vec::new();
    for axis in [Axis::Row, Axis::Column] {
        for i in i0..=i1 {
            for j in j0..=j1 {
                let id = EdgeId { axis, i, j };
                if !exists(seed, id) {
                    continue;
                }
                let edge = build(seed, id);
                if rect.gap(&edge.bounds) - edge.half_width_m <= reach {
                    edges.push(edge);
                }
            }
        }
    }
    edges
}

/// The road field at `p` over `edges[k]` for every `k` in `candidates` (ascending).
pub fn field(edges: &[Edge], candidates: &[u16], p: V2) -> RoadHit {
    let mut best = [(FAR_M, None::<Near>); 2];
    for &k in candidates {
        let edge = &edges[usize::from(k)];
        let group = edge.id.axis as usize;
        // A whole edge farther than the group's best cannot change it (and ties keep the first).
        if edge.bounds.distance(p) - edge.half_width_m >= best[group].0 {
            continue;
        }
        for seg in 0..edge.points.len() - 1 {
            let (a, b) = (edge.points[seg], edge.points[seg + 1]);
            let (d, t) = det::segment_distance(p, a, b);
            let value = d - edge.half_width_m;
            if value < best[group].0 {
                let ab = det::sub(b, a);
                let length = edge.s[seg + 1] - edge.s[seg];
                let tangent = det::norm_or(ab, [1.0, 0.0]);
                let side = if det::cross(ab, det::sub(p, a)) >= 0.0 { 1.0 } else { -1.0 };
                best[group] = (
                    value,
                    Some(Near {
                        edge: usize::from(k),
                        s: edge.s[seg] + t * length,
                        across: side * d,
                        tangent,
                        point: det::add(a, det::scale(ab, t)),
                    }),
                );
            }
        }
    }
    let [(row, row_near), (column, column_near)] = best;
    let f = det::smin(row, column, CORNER_BLEND_M);
    let (near, near_other) = if row <= column { (row_near, column_near) } else { (column_near, row_near) };
    RoadHit { f, row, column, near, near_other }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbouring_edges_share_endpoints_and_tangents() {
        for seed in [1, 2, 99] {
            let a = build(seed, EdgeId { axis: Axis::Row, i: 3, j: 1 });
            let b = build(seed, EdgeId { axis: Axis::Row, i: 4, j: 1 });
            assert_eq!(a.points.last(), b.points.first());
            let ta = det::norm_or(det::sub(a.points[32], a.points[31]), [1.0, 0.0]);
            let tb = det::norm_or(det::sub(b.points[1], b.points[0]), [1.0, 0.0]);
            assert!(det::dot(ta, tb) > 0.99, "kink at a node: {ta:?} {tb:?}");
        }
    }

    #[test]
    fn the_spawn_street_is_straight_near_the_origin() {
        for seed in [0, 5, 1234] {
            for i in -1..=1 {
                let edge = build(seed, EdgeId { axis: Axis::Row, i, j: 0 });
                for p in &edge.points {
                    if p[0].abs() <= 64.0 {
                        assert!((p[1] - SPAWN_ROAD_Y_M).abs() < 1e-9, "seed {seed}: {p:?}");
                    }
                }
            }
        }
    }
}
