/*! EUC Thrills — (c) 2026 VibezZzCoder — MIT — https://github.com/VibezZzCoder/EUC-thrills */
import * as THREE from 'three';

/**
 * A tiny procedural geometry kit for blockout characters.
 *
 * **Why this exists.** The M2 rider was primitives — a capsule per limb, a
 * capsule torso, a sphere head, boxes for everything that needed to be a
 * different colour. That was the right call at M2, when the job was to prove a
 * joint hierarchy. It is the wrong shape for a figure the player looks at for
 * three minutes at a time: the owner's note on the M7.5 build was that the
 * world reads low-poly, and the rider is the part of it the chase camera never
 * looks away from.
 *
 * The constraint that shapes everything here is the **draw-call budget**
 * (`DESIGN.md` §8): the frame ceiling is 150, the slice sits at 102, and the
 * ghost is a second rig. Detail added as *more meshes* costs two draw calls a
 * part. So detail is added as **more geometry inside the same meshes** instead,
 * and this file is what makes that affordable to author:
 *
 *   - `loftGeometry` builds a body from a stack of authored cross-sections, so
 *     a jacket can have shoulders, a waist, and a hem instead of being one
 *     capsule — at the cost of triangles, which the budget has in abundance
 *     (209,782 of 400,000 at M11).
 *   - `patchGeometry` builds a panel that lies *on* that body, sampling the
 *     same profile. This is the whole reason the identity blue stops floating:
 *     a box laid across a curved back stands off it at the corners, and a patch
 *     of the back itself cannot.
 *   - `mergeGeometries` folds a part's pieces into one buffer, so a helmet
 *     shell, its rear spoiler, and its chin bar are one mesh and one draw call.
 *
 * **Shade is a vertex multiplier, not a colour.** Every generator writes a
 * `color` attribute, and the materials that consume these geometries set
 * `vertexColors: true`, so the attribute *multiplies* the material colour
 * rather than replacing it. That keeps one material per look — the jacket, the
 * trousers, and a seam are all one draw call — while still separating their
 * values, which is what stops a black-clad rider reading as a single mass in
 * shade. Default 1 is "exactly the material colour", so a part that forgets to
 * pass one is not the black mesh `DESIGN.md` §7c warns about.
 *
 * **Every generator also writes `uv`, and nothing was obliged to use it until
 * M23.** A loft already walks a surface parameterization to place its vertices
 * — the angle around a section and the height up the profile — and a patch
 * already walks its own square; handing those two numbers on as texture
 * coordinates costs one attribute and closes the resolution ceiling that
 * vertex tint has. A material with no `map` ignores the attribute entirely, so
 * every look built before this change renders exactly as it did. `mapUvInto`
 * is how a look folds a part's unit square onto one page of a shared atlas,
 * which is what keeps printed graphics free of draw calls.
 *
 * The one thing to know about a loft's `u`: the seam column is **shared**, so
 * the last quad runs the coordinate backwards from `(radial-1)/radial` to 0.
 * The seam sits at `u = 0`, which by `loftPoint`'s convention is the rider's
 * LEFT, and art is placed away from it. A duplicated seam column would fix the
 * wrap and split the normals, which is a shading crack down a rider's side in
 * exchange for a texel nobody is looking at.
 *
 * Nothing here is animated and nothing here is read by `simulation/`. It is
 * geometry construction, run once per rig.
 */

/** One authored cross-section of a lofted body, in the part's local frame. */
export interface LoftRing {
  /** Height. Rings are sorted by this; `loftProfile` refuses a tie. */
  y: number;
  /** Half-extent across the rider, +X. */
  halfWidth: number;
  /** Half-extent fore and aft, +Z. */
  halfDepth: number;
  /** Forward offset of the section's centre. A chest sits ahead of a waist. */
  z?: number;
  /** Lateral offset of the section's centre. */
  x?: number;
  /**
   * Corner squareness of the section: 2 is a true ellipse, and higher values
   * push it toward a rounded rectangle.
   *
   * This is the single most useful number in the kit. A figure built entirely
   * from elliptical sections reads as a snowman no matter how well the heights
   * are chosen — the M2 torso comment already says so — and the fix is not
   * more sections but *flatter* ones. A jacket back is close to flat, a
   * shoulder is nearly square in plan, and a forearm is a soft oval.
   */
  square?: number;
}

/** A y-ascending, resolved profile. `v = 0` is always the lowest ring. */
export type LoftProfile = readonly Required<LoftRing>[];

/**
 * Normalise authored rings into a profile.
 *
 * Limbs are natural to author from the joint downward and bodies from the hem
 * up, so both orders are accepted and one is returned: **`v = 0` is the lowest
 * ring, always**, which is what lets `patchGeometry` name a span of a sleeve
 * and a span of a jacket the same way.
 *
 * Refuses rather than sorts silently. Two rings at the same height are a
 * degenerate strip that `computeVertexNormals` turns into a NaN normal, and a
 * NaN normal is a part that renders black — which is the failure mode this
 * project has shipped, or nearly shipped, five times (`DESIGN.md` §7c). A
 * deliberate crease is authored as two rings a millimetre apart, not zero.
 */
export function loftProfile(rings: readonly LoftRing[]): LoftProfile {
  if (rings.length < 2) throw new Error('a loft profile needs at least two rings');
  const ordered = rings[0]!.y <= rings[rings.length - 1]!.y ? rings : [...rings].reverse();
  const resolved = ordered.map((ring) => ({
    y: ring.y,
    halfWidth: ring.halfWidth,
    halfDepth: ring.halfDepth,
    z: ring.z ?? 0,
    x: ring.x ?? 0,
    square: ring.square ?? 2,
  }));
  for (let i = 1; i < resolved.length; i += 1) {
    if (!(resolved[i]!.y > resolved[i - 1]!.y)) {
      throw new Error(`loft ring ${i} does not rise above ring ${i - 1}`);
    }
  }
  return resolved;
}

/** Blend two rings. Used between authored sections, never outside the profile. */
function ringAt(profile: LoftProfile, v: number): Required<LoftRing> {
  const last = profile.length - 1;
  const clamped = Math.min(Math.max(v, 0), last);
  const index = Math.min(Math.floor(clamped), last - 1);
  const f = clamped - index;
  const a = profile[index]!;
  const b = profile[index + 1]!;
  return {
    y: a.y + (b.y - a.y) * f,
    halfWidth: a.halfWidth + (b.halfWidth - a.halfWidth) * f,
    halfDepth: a.halfDepth + (b.halfDepth - a.halfDepth) * f,
    z: a.z + (b.z - a.z) * f,
    x: a.x + (b.x - a.x) * f,
    square: a.square + (b.square - a.square) * f,
  };
}

/**
 * The `v` that lands at a given height — so a panel is authored in metres.
 *
 * Ring indices are an implementation detail of the profile, and an authored
 * "the back panel runs from the waist to the shoulders" that has to be
 * restated as "rings 4 to 8" silently breaks the moment a ring is inserted
 * between them. Every panel in `render/rider.ts` is placed through this.
 */
export function vAtHeight(profile: LoftProfile, y: number): number {
  const last = profile.length - 1;
  if (y <= profile[0]!.y) return 0;
  if (y >= profile[last]!.y) return last;
  for (let i = 1; i <= last; i += 1) {
    const above = profile[i]!;
    if (y <= above.y) {
      const below = profile[i - 1]!;
      return i - 1 + (y - below.y) / (above.y - below.y);
    }
  }
  return last;
}

/**
 * A point on the body.
 *
 * `u` is the angle around the section, measured from the rider's LEFT (+X)
 * toward FORWARD (+Z) — the world convention in `data/tuning.ts`, so a panel
 * on the back is authored at `-Math.PI / 2` and reads that way.
 */
export function loftPoint(profile: LoftProfile, u: number, v: number, out: THREE.Vector3): THREE.Vector3 {
  const ring = ringAt(profile, v);
  const e = 2 / ring.square;
  const c = Math.cos(u);
  const s = Math.sin(u);
  return out.set(
    ring.x + ring.halfWidth * Math.sign(c) * Math.abs(c) ** e,
    ring.y,
    ring.z + ring.halfDepth * Math.sign(s) * Math.abs(s) ** e,
  );
}

const NORMAL_TANGENT_U = new THREE.Vector3();
const NORMAL_TANGENT_V = new THREE.Vector3();
const NORMAL_SAMPLE_A = new THREE.Vector3();
const NORMAL_SAMPLE_B = new THREE.Vector3();

/**
 * The outward normal, by central difference rather than by the ellipse's
 * analytic gradient.
 *
 * The analytic form ignores the taper, so on a shoulder that narrows sharply
 * over four centimetres it points a good ten degrees wrong — and a panel
 * offset along a wrong normal is a panel that sinks into the body at one end
 * and floats at the other, which is precisely the artefact this kit exists to
 * remove. Differencing costs four extra samples per vertex, once, at build.
 */
export function loftNormal(profile: LoftProfile, u: number, v: number, out: THREE.Vector3): THREE.Vector3 {
  const du = 1e-3;
  const dv = 1e-3;
  const last = profile.length - 1;
  loftPoint(profile, u + du, v, NORMAL_SAMPLE_A);
  loftPoint(profile, u - du, v, NORMAL_SAMPLE_B);
  NORMAL_TANGENT_U.subVectors(NORMAL_SAMPLE_A, NORMAL_SAMPLE_B);
  const vHigh = Math.min(v + dv, last);
  const vLow = Math.max(v - dv, 0);
  loftPoint(profile, u, vHigh, NORMAL_SAMPLE_A);
  loftPoint(profile, u, vLow, NORMAL_SAMPLE_B);
  NORMAL_TANGENT_V.subVectors(NORMAL_SAMPLE_A, NORMAL_SAMPLE_B);
  out.crossVectors(NORMAL_TANGENT_V, NORMAL_TANGENT_U);
  const length = out.length();
  // A pole ring (radius zero) has no tangent in u. Point straight along the
  // axis rather than returning a zero vector that normalises to NaN.
  if (length < 1e-9) return out.set(0, v > (last - v) ? 1 : -1, 0);
  return out.multiplyScalar(1 / length);
}

export interface LoftOptions {
  /** Sections around the body. 16 is ample for a limb, 20 for a torso. */
  radialSegments?: number;
  /** Extra interpolated rows between each authored pair, for smooth tapers. */
  subdivisions?: number;
  /** Close the ends. A ring of radius ~0 closes to a single pole vertex. */
  capBottom?: boolean;
  capTop?: boolean;
  /** Vertex multiplier on the material colour. 1 is the material as authored. */
  shade?: number;
  /**
   * Give the seam column its own vertices, so the texture coordinate runs
   * 0 → 1 the whole way round instead of wrapping inside the last facet — M28.
   *
   * **A loft that is going to be paged needs this; nothing else does.** With
   * the seam shared, the facet between the last column and column 0 runs its
   * `u` from `(n−1)/n` back to `0`, which on a mapped body draws the *entire*
   * page reversed and squeezed into one strip down the rider's left. That is
   * exactly the "seam column runs backwards" note `DESIGN.md` §7i records as
   * the reason a loft was never printed on before the first printed jersey.
   *
   * The cost is one duplicate column of vertices, and the shading crack that
   * would come with it is closed by hand: after the normals are computed, the
   * two seam columns are averaged and share one normal, so the surface is as
   * smooth as the shared-seam build. Off by default — every loft on every
   * other rider and machine builds byte-identically with it off.
   */
  splitSeam?: boolean;
}

/** Is this end ring small enough to close to a point rather than a flat disc? */
function isPole(ring: Required<LoftRing>): boolean {
  return ring.halfWidth < 1e-4 && ring.halfDepth < 1e-4;
}

/**
 * The body itself: a closed tube through every section.
 *
 * Vertices are shared around the seam at `u = 0`, so `computeVertexNormals`
 * produces one smooth surface with no shading crack down the rider's left. A
 * crease that *should* read hard — a jacket hem, a collar — is authored as two
 * rings a few millimetres apart, which is how it would be modelled anyway.
 */
export function loftGeometry(profile: LoftProfile, options: LoftOptions = {}): THREE.BufferGeometry {
  const radial = Math.max(3, Math.round(options.radialSegments ?? 16));
  const subdivisions = Math.max(0, Math.round(options.subdivisions ?? 0));
  const shade = options.shade ?? 1;
  const splitSeam = options.splitSeam ?? false;
  /** Vertex columns per row: one more than the facets when the seam is split. */
  const columns = splitSeam ? radial + 1 : radial;

  const rows: number[] = [];
  for (let i = 0; i < profile.length - 1; i += 1) {
    for (let s = 0; s <= subdivisions; s += 1) rows.push(i + s / (subdivisions + 1));
  }
  rows.push(profile.length - 1);

  const positions: number[] = [];
  const colours: number[] = [];
  const uvs: number[] = [];
  const indices: number[] = [];
  const point = new THREE.Vector3();

  const bottomPole = isPole(profile[0]!);
  const topPole = isPole(profile[profile.length - 1]!);
  const vSpan = Math.max(1e-6, profile.length - 1);

  // One vertex per (u, v), with the u = 2π column folded back onto u = 0 —
  // or, with `splitSeam`, given its own column at u = 1 so a page can wrap.
  for (const v of rows) {
    for (let i = 0; i < columns; i += 1) {
      loftPoint(profile, (i / radial) * Math.PI * 2, v, point);
      positions.push(point.x, point.y, point.z);
      colours.push(shade, shade, shade);
      // The surface's own parameters, handed on as texture coordinates — see
      // the `uv` note in the file comment. `v` is the profile's ring-index
      // axis normalised, which is the space `vAtHeight` speaks, so a look can
      // convert a height in metres into a texture row without knowing how many
      // rings the profile happens to have.
      uvs.push(i / radial, v / vSpan);
    }
  }
  const at = (row: number, i: number): number => row * columns + (splitSeam ? i : i % radial);
  for (let row = 0; row < rows.length - 1; row += 1) {
    for (let i = 0; i < radial; i += 1) {
      const a = at(row, i);
      const b = at(row, i + 1);
      const c = at(row + 1, i + 1);
      const d = at(row + 1, i);
      // Wound so the surface faces outward; see the note in the test.
      indices.push(a, d, b, b, d, c);
    }
  }

  const fan = (row: number, ring: Required<LoftRing>, upward: boolean): void => {
    if (isPole(ring)) return;
    const centre = positions.length / 3;
    positions.push(ring.x, ring.y, ring.z);
    colours.push(shade, shade, shade);
    // A cap has no surface parameter of its own; it takes the middle of the
    // texture row its ring sits on, so a mapped part's end disc samples the
    // same neighbourhood as the surface around it rather than a corner.
    uvs.push(0.5, upward ? 1 : 0);
    for (let i = 0; i < radial; i += 1) {
      const a = at(row, i);
      const b = at(row, i + 1);
      if (upward) indices.push(centre, b, a);
      else indices.push(centre, a, b);
    }
  };
  if ((options.capBottom ?? true) && !bottomPole) fan(0, profile[0]!, false);
  if ((options.capTop ?? true) && !topPole) fan(rows.length - 1, profile[profile.length - 1]!, true);

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute('color', new THREE.Float32BufferAttribute(colours, 3));
  geometry.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2));
  geometry.setIndex(indices);
  geometry.computeVertexNormals();
  if (splitSeam) {
    // The two seam columns sit on one point and were averaged from one side
    // each; give them the mean so the seam shades as if it were shared.
    const normal = geometry.getAttribute('normal');
    for (let row = 0; row < rows.length; row += 1) {
      const a = at(row, 0);
      const b = at(row, radial);
      const x = normal.getX(a) + normal.getX(b);
      const y = normal.getY(a) + normal.getY(b);
      const z = normal.getZ(a) + normal.getZ(b);
      const length = Math.hypot(x, y, z) || 1;
      normal.setXYZ(a, x / length, y / length, z / length);
      normal.setXYZ(b, x / length, y / length, z / length);
    }
    normal.needsUpdate = true;
  }
  return geometry;
}

export interface PatchOptions {
  /** Angular span, radians, in the same frame as `loftPoint`. */
  u0: number;
  u1: number;
  /** Vertical span in ring-index space. `v = 0` is the profile's lowest ring. */
  v0: number;
  v1: number;
  uSegments?: number;
  vSegments?: number;
  /** How far the outer face stands off the body, metres. A jacket panel is thin. */
  lift?: number;
  /** How far the inner face sinks into it. Negative, and enough to never gap. */
  sink?: number;
  /**
   * Diagonal shear: the v span slides by this many rings from one end of the
   * u span to the other. A chest chevron is a sheared band, not a bent box.
   */
  skew?: number;
  /** Taper the band's height toward its ends, as a fraction of the span. */
  taper?: number;
  /**
   * Swell the band's height toward its middle, as a fraction of the span.
   *
   * **The axis a visor needed and a stripe never did.** Every patch before
   * M23 Phase A1d was a band of constant height, which is right for a rail, a
   * cuff or a shoulder stripe — and is why a helmet's eyeport drawn as one
   * came out as a television: a constant-`v` span is a pair of horizontal
   * rings around the shell by construction, so it has a dead-straight top and
   * bottom and square corners at any width. `taper` cannot fix it; it pulls
   * both edges *in* at the ends, which is a lens, not an eyeport.
   *
   * Swelling the middle instead gives an arched brow and a chin notch from one
   * number, and composes with `taper` — the two together are a shield.
   */
  bulge?: number;
  /** Slide the band's centre line at its middle, in ring indices. */
  bow?: number;
  shade?: number;
  /**
   * Run the patch's `s` with **arc** along the body rather than with angle —
   * M28 Phase 2, and the reason is a printed plate on a wheel's shell.
   *
   * A patch's columns are spaced evenly in `u`, which on a circle is evenly
   * in metres and on a square-5 superellipse is nothing of the kind: across
   * a 0.21 rad span on that shell's flank the first of eight columns covered
   * 73 mm of a 151 mm plate and the last 7 mm, so a mark centred on the page
   * came out crushed into the plate's front half with its trailing letter
   * 1.6× the width of its leading one (a blind critic measured it; the
   * verifier found the cause here). With this set the mid-row's cumulative
   * arc is sampled once at build and each column lands at its arc fraction,
   * so the page runs at one scale across the whole patch. Opt-in, so every
   * existing patch on every rider and machine is byte-identical.
   */
  uByArc?: boolean;
}

/**
 * A panel lying on a lofted body.
 *
 * Built as a slab — an outer face, an inner face sunk below the surface, and a
 * rim joining them — rather than a single shell, because a shell has no
 * silhouette and a panel seen edge-on at the shoulder is exactly where the
 * chase camera spends its time. The inner face is what makes the panel
 * unconditionally opaque over its own body: sunk 3 mm into a surface that
 * curves away, nothing shows through at a grazing angle.
 *
 * The rim is what a box laid on a curve cannot have: it follows the body, so
 * the panel's edge is the same distance proud of the jacket everywhere along
 * its length.
 */
export function patchGeometry(profile: LoftProfile, options: PatchOptions): THREE.BufferGeometry {
  const uSegments = Math.max(1, Math.round(options.uSegments ?? 8));
  const vSegments = Math.max(1, Math.round(options.vSegments ?? 4));
  const lift = options.lift ?? 0.01;
  const sink = options.sink ?? -0.006;
  const skew = options.skew ?? 0;
  const taper = options.taper ?? 0;
  const bulge = options.bulge ?? 0;
  const bow = options.bow ?? 0;
  const shade = options.shade ?? 1;

  const positions: number[] = [];
  const colours: number[] = [];
  const uvs: number[] = [];
  const indices: number[] = [];
  const point = new THREE.Vector3();
  const normal = new THREE.Vector3();

  /**
   * `s`/`t` are the patch's **own** square, not the body's — which is exactly
   * what a decal wants. A panel authored as "±0.95 rad of the front, waist to
   * collarbone" gets a texture space whose corners are its own corners however
   * the body curves underneath it, so art painted for it cannot slide when the
   * profile it lies on gains a ring.
   */
  const push = (u: number, v: number, offset: number, s: number, t: number): number => {
    const index = positions.length / 3;
    loftPoint(profile, u, v, point);
    loftNormal(profile, u, v, normal);
    positions.push(
      point.x + normal.x * offset,
      point.y + normal.y * offset,
      point.z + normal.z * offset,
    );
    colours.push(shade, shade, shade);
    uvs.push(s, t);
    return index;
  };

  /**
   * `s` to `u`: linear in angle, or — `uByArc` — linear in metres along the
   * mid row, by inverting the cumulative arc sampled once here.
   */
  const uAt = (() => {
    if (!options.uByArc) return (s: number): number => options.u0 + (options.u1 - options.u0) * s;
    const steps = 192;
    const vMid = (options.v0 + options.v1) / 2;
    const arcs = new Float64Array(steps + 1);
    const a = new THREE.Vector3();
    const b = new THREE.Vector3();
    loftPoint(profile, options.u0, vMid, a);
    for (let k = 1; k <= steps; k += 1) {
      loftPoint(profile, options.u0 + (options.u1 - options.u0) * (k / steps), vMid, b);
      arcs[k] = arcs[k - 1]! + a.distanceTo(b);
      a.copy(b);
    }
    const total = arcs[steps]!;
    /** Arc from `ua` to `ub` along the mid row, finely enough to follow a flat face's centre. */
    const arcBetween = (ua: number, ub: number): number => {
      loftPoint(profile, ua, vMid, a);
      let length = 0;
      for (let k = 1; k <= 24; k += 1) {
        loftPoint(profile, ua + (ub - ua) * (k / 24), vMid, b);
        length += a.distanceTo(b);
        a.copy(b);
      }
      return length;
    };
    return (s: number): number => {
      const target = Math.min(1, Math.max(0, s)) * total;
      let k = 1;
      while (k < steps && arcs[k]! < target) k += 1;
      // Inside the coarse step, bisect on the true arc rather than
      // interpolating in `u`: across the flat centre of a square
      // superellipse one step of 0.001 rad is 26 mm of surface with almost
      // all of it in the middle, and a linear guess there lands a column
      // 12 mm off the even spacing this option exists to give.
      const ua = options.u0 + (options.u1 - options.u0) * ((k - 1) / steps);
      const ub = options.u0 + (options.u1 - options.u0) * (k / steps);
      let low = ua;
      let high = ub;
      for (let it = 0; it < 16; it += 1) {
        const mid = (low + high) / 2;
        if (arcs[k - 1]! + arcBetween(ua, mid) < target) low = mid;
        else high = mid;
      }
      return (low + high) / 2;
    };
  })();

  /** The patch's own (s, t) square mapped onto the body's (u, v). */
  const coords = (s: number, t: number): [number, number] => {
    const u = uAt(s);
    // How far along the span this column is from either end: 1 in the middle,
    // 0 at the edges. Both the swell and the bow are measured against it.
    const middle = 1 - Math.abs(s - 0.5) * 2;
    const centre = (options.v0 + options.v1) / 2 + skew * (s - 0.5) + bow * middle;
    const halfSpan = ((options.v1 - options.v0) / 2)
      * (1 - taper * Math.abs(s - 0.5) * 2)
      * (1 + bulge * middle);
    return [u, centre + halfSpan * (t * 2 - 1)];
  };

  // Two grids of the same shape, one lifted proud and one sunk in.
  const outer: number[][] = [];
  const inner: number[][] = [];
  for (let i = 0; i <= uSegments; i += 1) {
    const outerRow: number[] = [];
    const innerRow: number[] = [];
    for (let j = 0; j <= vSegments; j += 1) {
      const s = i / uSegments;
      const t = j / vSegments;
      const [u, v] = coords(s, t);
      outerRow.push(push(u, v, lift, s, t));
      innerRow.push(push(u, v, sink, s, t));
    }
    outer.push(outerRow);
    inner.push(innerRow);
  }
  for (let i = 0; i < uSegments; i += 1) {
    for (let j = 0; j < vSegments; j += 1) {
      const a = outer[i]![j]!;
      const b = outer[i + 1]![j]!;
      const c = outer[i + 1]![j + 1]!;
      const d = outer[i]![j + 1]!;
      indices.push(a, d, b, b, d, c);
      const ia = inner[i]![j]!;
      const ib = inner[i + 1]![j]!;
      const ic = inner[i + 1]![j + 1]!;
      const id = inner[i]![j + 1]!;
      indices.push(ia, ib, id, ib, ic, id);
    }
  }

  // The rim, as four strips of independent vertices so it shades hard against
  // the faces rather than smearing the panel's edge back into the body.
  const rim = (a0: number[], a1: number[], flip: boolean): void => {
    for (let k = 0; k < a0.length - 1; k += 1) {
      const p = positions.length / 3;
      for (const index of [a0[k]!, a0[k + 1]!, a1[k + 1]!, a1[k]!]) {
        positions.push(positions[index * 3]!, positions[index * 3 + 1]!, positions[index * 3 + 2]!);
        colours.push(shade, shade, shade);
        uvs.push(uvs[index * 2]!, uvs[index * 2 + 1]!);
      }
      if (flip) indices.push(p, p + 2, p + 1, p, p + 3, p + 2);
      else indices.push(p, p + 1, p + 2, p, p + 2, p + 3);
    }
  };
  const outerLow = outer.map((row) => row[0]!);
  const innerLow = inner.map((row) => row[0]!);
  const outerHigh = outer.map((row) => row[vSegments]!);
  const innerHigh = inner.map((row) => row[vSegments]!);
  rim(outerLow, innerLow, false);
  rim(outerHigh, innerHigh, true);
  rim(outer[0]!, inner[0]!, true);
  rim(outer[uSegments]!, inner[uSegments]!, false);

  // **A span authored from its outboard end back toward the centre is a mirror
  // in parameter space, and a mirror flips the orientation of the whole
  // surface.** Every face above is wound assuming `s` runs the same way as `u`
  // and `t` the same way as `v`. When one of them runs backwards the outer face
  // is wound *inward*: back-face culling removes it, `computeVertexNormals`
  // points what is left into the body, and the panel is not a mistake the eye
  // can name — it is simply absent, with only its rim left catching light at
  // the wrong sign. The rider shipped exactly that for a whole milestone, one
  // authored line apart from its own mirror image: `Math.PI / 2 + side * 0.92`
  // is an increasing span on the rider's left and a decreasing one on the
  // right, so the right chest chevron was drawn inside-out from every angle
  // while its twin was fine. Mirroring the span *is* the natural way to author
  // a symmetric pair, so the kit re-winds rather than silently dropping a part.
  if ((options.u1 - options.u0) * (options.v1 - options.v0) < 0) {
    for (let i = 0; i < indices.length; i += 3) {
      const middle = indices[i + 1]!;
      indices[i + 1] = indices[i + 2]!;
      indices[i + 2] = middle;
    }
  }

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute('color', new THREE.Float32BufferAttribute(colours, 3));
  geometry.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2));
  geometry.setIndex(indices);
  geometry.computeVertexNormals();
  return geometry;
}

/**
 * Give a stock three geometry the `color` attribute the merged buffers need.
 *
 * Every material in the rig sets `vertexColors: true`, and a geometry without
 * the attribute renders **black** under one — the exact trap `DESIGN.md` §7c
 * records from the instanced props, one system over. Mixing a `BoxGeometry`
 * into a merge without this is that bug, so `mergeGeometries` refuses instead.
 */
export function shaded<T extends THREE.BufferGeometry>(geometry: T, shade = 1): T {
  const count = geometry.getAttribute('position').count;
  const colours = new Float32Array(count * 3).fill(shade);
  geometry.setAttribute('color', new THREE.BufferAttribute(colours, 3));
  return geometry;
}

/** A per-channel vertex multiplier over a material's colour. 1,1,1 is the colour. */
export type Tint = readonly [number, number, number];

/**
 * The vertex multiplier that repaints a `base`-coloured material as `target`.
 *
 * The whole repaint system rests on one shader fact: a vertex colour
 * *multiplies* the material colour, per channel, in linear space — so the
 * ratio of two linear colours is the paint that turns one into the other.
 * `THREE.Color` decodes both hexes through the same sRGB transfer, which
 * keeps the ratio honest and keeps every painted target authored in
 * `data/tuning.ts` like any other colour (invariant 4 in spirit: the hex
 * lives there; only the arithmetic lives here).
 *
 * It sits in the kit beside `shaded()` because both look files need it and for
 * the same reason — `shaded` is this function's scalar half, and a scalar
 * cannot change hue. It lived in `render/riderLook.ts` until M22 gave a
 * *machine* a pale trim material that has to reach green, blue and near-black
 * from one base; importing 4,000 lines of rider looks to borrow eight lines of
 * colour arithmetic was the wrong direction of dependency.
 */
export function tintOver(base: number, target: number, targetScale = 1): Tint {
  const b = new THREE.Color(base);
  const t = new THREE.Color(target).multiplyScalar(targetScale);
  return [
    t.r / Math.max(1e-3, b.r),
    t.g / Math.max(1e-3, b.g),
    t.b / Math.max(1e-3, b.b),
  ];
}

/**
 * Fold several geometries into one buffer, and dispose the inputs.
 *
 * Disposing here is deliberate: the pieces are intermediates that never reach a
 * mesh, and a caller that had to track them separately from the result would
 * be one forgotten line away from the leak the M0 resource test exists to
 * catch. What the caller tracks is what it draws.
 */
export function mergeGeometries(parts: readonly THREE.BufferGeometry[]): THREE.BufferGeometry {
  if (parts.length === 0) throw new Error('nothing to merge');
  const positions: number[] = [];
  const normals: number[] = [];
  const colours: number[] = [];
  const uvs: number[] = [];
  const indices: number[] = [];
  // **All or nothing on `uv`** — M23. Every generator in this kit writes one,
  // so a merge that loses it is a merge with a stock geometry in it, and a
  // half-filled attribute is worse than none: the parts that had coordinates
  // would keep them and the parts that did not would silently sample texel
  // (0, 0). `mapUvInto` refuses a geometry with no `uv`, so the atlas path
  // cannot ride on a merge that dropped it.
  const mapped = parts.every((part) => part.getAttribute('uv') !== undefined);
  for (const part of parts) {
    const position = part.getAttribute('position');
    const normal = part.getAttribute('normal');
    const colour = part.getAttribute('color');
    const uv = part.getAttribute('uv');
    if (!normal) throw new Error('a merged geometry needs normals');
    if (!colour) throw new Error('a merged geometry needs a color attribute; see shaded()');
    const offset = positions.length / 3;
    for (let i = 0; i < position.count; i += 1) {
      positions.push(position.getX(i), position.getY(i), position.getZ(i));
      normals.push(normal.getX(i), normal.getY(i), normal.getZ(i));
      colours.push(colour.getX(i), colour.getY(i), colour.getZ(i));
      if (mapped && uv) uvs.push(uv.getX(i), uv.getY(i));
    }
    const index = part.getIndex();
    if (index) {
      for (let i = 0; i < index.count; i += 1) indices.push(index.getX(i) + offset);
    } else {
      for (let i = 0; i < position.count; i += 1) indices.push(i + offset);
    }
    part.dispose();
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
  geometry.setAttribute('normal', new THREE.Float32BufferAttribute(normals, 3));
  geometry.setAttribute('color', new THREE.Float32BufferAttribute(colours, 3));
  if (mapped) geometry.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2));
  geometry.setIndex(indices);
  return geometry;
}

/** A rectangle of a texture atlas, in texture coordinates. */
export interface UvRect {
  readonly u0: number;
  readonly v0: number;
  readonly u1: number;
  readonly v1: number;
}

/**
 * Fold a part's own `uv` square into one rectangle of a shared atlas.
 *
 * **This is the whole of the atlas mechanism** — M23 Phase A1b. Every
 * generator above writes texture coordinates spanning the unit square, because
 * a part's natural parameters are the only ones it can know; which *page* of
 * which sheet those coordinates land on is a decision the look makes, after the
 * fact, on the geometry it just built. One texture then serves a chest print, a
 * leg script and a visor gradient at no extra draw call, which is the only
 * reason a rider may carry printed graphics at all (`docs/PLANS.md` §23.9d).
 *
 * Refuses a geometry with no `uv` rather than adding one. A part that arrived
 * here unmapped is a part whose surface parameters were thrown away somewhere
 * upstream, and inventing a square for it would put art on a surface at an
 * angle nobody chose.
 */
export function mapUvInto<T extends THREE.BufferGeometry>(geometry: T, rect: UvRect): T {
  const uv = geometry.getAttribute('uv');
  if (!uv) throw new Error('mapUvInto needs a geometry with texture coordinates');
  const width = rect.u1 - rect.u0;
  const height = rect.v1 - rect.v0;
  for (let i = 0; i < uv.count; i += 1) {
    uv.setXY(i, rect.u0 + uv.getX(i) * width, rect.v0 + uv.getY(i) * height);
  }
  uv.needsUpdate = true;
  return geometry;
}

/**
 * A limb profile: a tapered tube with a rounded end, and optional padding
 * seams.
 *
 * The seams are the cheapest thing in this file and do more for "padded moto
 * gear" than any amount of surface detail — a pair of rings a centimetre apart
 * with three millimetres of difference between them reads as a quilted panel
 * break at chase distance, and costs two rows of triangles.
 *
 * @param length   Distance from the joint to the far end, hanging along -Y.
 * @param radii    Radius at the joint, at mid-limb, and at the far end.
 * @param seams    Fractions along the limb, from the joint, to break.
 */
export function limbProfile(
  length: number,
  radii: readonly [number, number, number],
  seams: readonly number[] = [],
  options: { flatten?: number; roundEnd?: boolean; square?: number } = {},
): LoftProfile {
  const flatten = options.flatten ?? 0.92;
  const square = options.square ?? 2.3;
  const [top, mid, end] = radii;
  const radiusAt = (t: number): number => (t < 0.5 ? top + (mid - top) * (t / 0.5) : mid + (end - mid) * ((t - 0.5) / 0.5));
  const rings: LoftRing[] = [];
  const add = (t: number, scale = 1): void => {
    const radius = radiusAt(Math.min(Math.max(t, 0), 1)) * scale;
    rings.push({ y: -t * length, halfWidth: radius, halfDepth: radius * flatten, square });
  };
  add(0);
  const stops = new Set<number>([0.25, 0.5, 0.75]);
  for (const seam of seams) {
    stops.add(seam - 0.018);
    stops.add(seam + 0.018);
  }
  const sorted = [...stops].filter((t) => t > 0.02 && t < 0.97).sort((a, b) => a - b);
  for (const t of sorted) {
    const onSeam = seams.some((seam) => Math.abs(t - seam) < 0.02);
    add(t, onSeam ? (t < seams.find((seam) => Math.abs(t - seam) < 0.02)! ? 1.05 : 0.95) : 1);
  }
  if (options.roundEnd ?? true) {
    // A hemispherical close, so a wrist or an ankle is not a cut pipe.
    const endRadius = radiusAt(1);
    for (const [t, scale] of [[0.5, 0.86], [0.85, 0.54], [1, 0]] as const) {
      rings.push({
        y: -length - endRadius * 0.55 * t,
        halfWidth: endRadius * scale,
        halfDepth: endRadius * scale * flatten,
        square,
      });
    }
  } else {
    add(1);
  }
  return loftProfile(rings);
}
