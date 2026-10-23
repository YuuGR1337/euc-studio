//! What touches the ground, gathered for sound (Free Ride's impacts and scrapes): each step's
//! tyre, shell and rider contacts with the ground, summed or kept at their largest until the host
//! takes them ([`Physics::take_contact_digest`](crate::Physics::take_contact_digest)). It only
//! reads the contacts, so the simulation is the same with or without it.

use mujoco_rs::prelude::*;

use euc_telemetry::BodyRole;

use crate::{GROUND_GROUP, mjcf::names};

/// The tyre's resting load follows its load with this time constant, seconds: slopes, a crouch or
/// a rider stepping off change it without a hit.
const TYRE_REST_S: f64 = 0.5;

/// The shell's and the rider's resting loads follow theirs with this time constant, seconds: a
/// rider lying on the road presses steadily and makes no new thud.
const BODY_REST_S: f64 = 0.2;

/// The rider's parts, as [`ContactDigest::rider_part`] names them.
const PARTS: [&str; 5] = ["head", "trunk", "arm", "leg", "foot"];

/// What touched the ground since the last digest. Impulses count only the load above the resting
/// load (`∫(F − F_rest)⁺ dt`), so a body that lies still makes none.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ContactDigest {
    /// The tyre on the ground, impulse above its resting load, N·s: a kerb, a pothole's edge.
    pub tyre_impact_ns: f64,
    /// The tyre load's fastest rise (smoothed over 5 ms), N/s.
    pub tyre_load_rate_n_s: f64,
    /// The shell and pedals on the ground, impulse above their resting load, N·s: a wheel that
    /// drops or tips onto the road.
    pub shell_impact_ns: f64,
    /// The largest normal force of one shell or pedal contact on the ground, N: a scrape's
    /// loudness, with [`shell_slip_mps`](Self::shell_slip_mps).
    pub shell_contact_n: f64,
    /// That contact's slip speed along the ground, m/s: a scrape's grain.
    pub shell_slip_mps: f64,
    /// The rider on the ground, impulse above the resting load, N·s: a fall.
    pub rider_impact_ns: f64,
    /// The rider's part with the largest share of that impulse: head, trunk, arm, leg or foot.
    pub rider_part: Option<&'static str>,
}

/// What a geom is, for the ground's contacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Ground,
    Tyre,
    Shell,
    /// A rider geom and its part ([`PARTS`] index).
    Rider(usize),
    Other,
}

/// Loads are smoothed over this time constant first, seconds: the contact solver's step-to-step
/// chatter (hundreds of newtons on the rolling tyre) is not a hit; a kerb's or a pothole's lasts
/// 10-50 ms.
const QUICK_S: f64 = 0.005;

/// A load, smoothed ([`QUICK_S`]), and its resting value, following it slowly.
#[derive(Debug, Clone, Copy, Default)]
struct Load {
    quick_n: f64,
    rest_n: f64,
    started: bool,
}

impl Load {
    /// The smoothed load above the resting load this step, newtons; then the rest follows.
    fn excess(&mut self, load: f64, dt: f64, rest_s: f64) -> f64 {
        if !self.started {
            (self.quick_n, self.rest_n, self.started) = (load, load, true);
        }
        self.quick_n += (load - self.quick_n) * (dt / QUICK_S).min(1.0);
        let excess = (self.quick_n - self.rest_n).max(0.0);
        self.rest_n += (self.quick_n - self.rest_n) * (dt / rest_s).min(1.0);
        excess
    }
}

/// The bodies that tell the classes apart.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Roots<'a> {
    pub wheel: usize,
    pub shell: usize,
    /// The model's named bodies and their roles (the rider's are [`BodyRole::Rider`]).
    pub bodies: &'a [(usize, BodyRole, &'static str)],
}

/// Gathers the digest step by step.
#[derive(Debug, Clone, Default)]
pub(crate) struct ContactWatch {
    classes: Vec<Class>,
    tyre: Load,
    shell: Load,
    rider: Load,
    last_tyre_n: Option<f64>,
    digest: ContactDigest,
    part_ns: [f64; PARTS.len()],
}

impl ContactWatch {
    /// One step's contacts, after the step (its forces).
    pub(crate) fn observe(&mut self, data: &MjData<Box<MjModel>>, roots: Roots<'_>, dt: f64) {
        let model = data.model();
        if self.classes.len() != model.ffi().ngeom as usize {
            self.classes = classify(model, roots);
        }
        let (mut tyre, mut shell, mut rider) = (0.0, 0.0, 0.0);
        let mut parts = [0.0; PARTS.len()];
        let mut biggest: Option<(f64, usize, usize)> = None;
        for (index, contact) in data.contact().iter().enumerate() {
            if contact.exclude != 0 {
                continue;
            }
            let geoms = [contact.geom[0] as usize, contact.geom[1] as usize];
            let (other, geom) = match (self.classes[geoms[0]], self.classes[geoms[1]]) {
                (Class::Ground, class) => (class, geoms[1]),
                (class, Class::Ground) => (class, geoms[0]),
                _ => continue,
            };
            // A degenerate contact can come back without a finite force; it carries none.
            let normal = data.contact_force(index)[0].abs();
            if !normal.is_finite() {
                continue;
            }
            match other {
                Class::Tyre => tyre += normal,
                Class::Shell => {
                    shell += normal;
                    if biggest.is_none_or(|(most, _, _)| normal > most) {
                        biggest = Some((normal, index, geom));
                    }
                }
                Class::Rider(part) => {
                    rider += normal;
                    parts[part] += normal;
                }
                Class::Ground | Class::Other => {}
            }
        }
        let digest = &mut self.digest;
        digest.tyre_impact_ns += self.tyre.excess(tyre, dt, TYRE_REST_S) * dt;
        let quick = self.tyre.quick_n;
        if let Some(last) = self.last_tyre_n {
            digest.tyre_load_rate_n_s = digest.tyre_load_rate_n_s.max((quick - last) / dt);
        }
        self.last_tyre_n = Some(quick);
        digest.shell_impact_ns += self.shell.excess(shell, dt, BODY_REST_S) * dt;
        if let Some((normal, index, geom)) = biggest
            && normal > digest.shell_contact_n
        {
            digest.shell_contact_n = normal;
            digest.shell_slip_mps = slip_mps(data, index, geom);
        }
        let excess = self.rider.excess(rider, dt, BODY_REST_S) * dt;
        digest.rider_impact_ns += excess;
        if rider > 0.0 {
            for (sum, part) in self.part_ns.iter_mut().zip(parts) {
                *sum += excess * part / rider;
            }
        }
    }

    /// The digest since the last call; the next one starts empty (the resting loads go on).
    pub(crate) fn take(&mut self) -> ContactDigest {
        let mut digest = std::mem::take(&mut self.digest);
        let (part, most) =
            self.part_ns.iter().enumerate().fold((0, 0.0), |best, (k, &v)| if v > best.1 { (k, v) } else { best });
        digest.rider_part = (most > 0.0).then_some(PARTS[part]);
        self.part_ns = [0.0; PARTS.len()];
        digest
    }
}

/// Every geom's class: the ground's group, then by the body it hangs from (the wheel's subtree is
/// the tyre, a rider body's or the rigid rider's feet the rider, the rest of the shell's subtree the
/// shell and its pedals).
fn classify(model: &MjModel, roots: Roots<'_>) -> Vec<Class> {
    let feet = model.name_to_id(MjtObj::mjOBJ_BODY, names::FEET);
    let (groups, bodies, parents) = (model.geom_group(), model.geom_bodyid(), model.body_parentid());
    (0..model.ffi().ngeom as usize)
        .map(|geom| {
            if groups[geom] as usize == GROUND_GROUP {
                return Class::Ground;
            }
            let mut body = bodies[geom] as usize;
            loop {
                if body == roots.wheel {
                    return Class::Tyre;
                }
                let rider = roots.bodies.iter().any(|&(id, role, _)| id == body && role == BodyRole::Rider);
                if rider || Some(body) == feet {
                    return Class::Rider(part(model.id_to_name(MjtObj::mjOBJ_GEOM, geom).unwrap_or("")));
                }
                if body == roots.shell {
                    return Class::Shell;
                }
                if body == 0 {
                    return Class::Other;
                }
                body = parents[body] as usize;
            }
        })
        .collect()
}

/// The rider's part a geom belongs to, from its name ([`PARTS`] index).
fn part(name: &str) -> usize {
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    if has(&["head", "neck"]) {
        0
    } else if has(&["arm", "hand"]) {
        2
    } else if has(&["thigh", "shank", "knee", "shin", "calf", "leg"]) {
        3
    } else if has(&["foot", "sole", "feet", "toe", "heel"]) {
        4
    } else {
        1
    }
}

/// A contact's slip along the ground, m/s: the geom's velocity at the contact point, less its part
/// along the contact's normal (the ground does not move).
fn slip_mps(data: &MjData<Box<MjModel>>, index: usize, geom: usize) -> f64 {
    let contact = &data.contact()[index];
    let velocity = data.object_velocity(MjtObj::mjOBJ_GEOM, geom, false);
    let at = data.geom_xpos()[geom];
    let arm = [contact.pos[0] - at[0], contact.pos[1] - at[1], contact.pos[2] - at[2]];
    let (w, v) = ([velocity[0], velocity[1], velocity[2]], [velocity[3], velocity[4], velocity[5]]);
    let point = [
        v[0] + w[1] * arm[2] - w[2] * arm[1],
        v[1] + w[2] * arm[0] - w[0] * arm[2],
        v[2] + w[0] * arm[1] - w[1] * arm[0],
    ];
    let normal = [contact.frame[0], contact.frame[1], contact.frame[2]];
    let along = point[0] * normal[0] + point[1] * normal[1] + point[2] * normal[2];
    let slip = [point[0] - along * normal[0], point[1] - along * normal[1], point[2] - along * normal[2]];
    (slip[0] * slip[0] + slip[1] * slip[1] + slip[2] * slip[2]).sqrt()
}
