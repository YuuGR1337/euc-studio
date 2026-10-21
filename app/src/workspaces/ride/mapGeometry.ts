import * as THREE from "three";
import { semantic } from "../../design/tokens.ts";
import { type HeightMapView, MAP_PAUSE_M, MAP_WARNING_M } from "./mapSurface.ts";

// Illustrative height colours aid shape reading in the existing dark scene.
// They do not represent grip, road material, or validated riding conditions.
const DATUM = new THREE.Color("#354349");
const RAISED = new THREE.Color("#64726B");
const LOWERED = new THREE.Color("#253745");
const WARNING = new THREE.Color(semantic.caution).multiplyScalar(0.3);
const PAUSE = new THREE.Color(semantic.danger).multiplyScalar(0.4);
const GRID = new THREE.Color("#7A8B8E");
const WARNING_LINE = new THREE.Color(semantic.caution);
const PAUSE_LINE = new THREE.Color(semantic["danger-text"]);
const EDGE_LINE = new THREE.Color("#EDF0EC");

interface SurfaceGeometry {
  surface: THREE.BufferGeometry;
  grid: THREE.BufferGeometry;
}

export function buildMapSurface(map: HeightMapView): SurfaceGeometry {
  // Six separate vertices per cell preserve exact triangle normals and sharp
  // metre-aligned edge bands; shared normals would smooth over surface creases.
  const triangleVertices = (map.nx - 1) * (map.ny - 1) * 6;
  const positions = new Float32Array(triangleVertices * 3);
  const colors = new Float32Array(positions.length);
  const spanX = (map.nx - 1) * map.spacing_m;
  const spanY = (map.ny - 1) * map.spacing_m;
  const color = new THREE.Color();
  let offset = 0;

  function vertex(ix: number, iy: number, band: THREE.Color | null) {
    const height = (map.heights_mm[iy * map.nx + ix] as number) / 1000;
    const x = map.origin_xy_m[0] + ix * map.spacing_m;
    const y = map.origin_xy_m[1] + iy * map.spacing_m;
    // Physics (x, y, z) → Three.js (x, z, -y), a rotation with positive determinant.
    positions[offset] = x;
    positions[offset + 1] = height;
    positions[offset + 2] = -y;
    if (band) color.copy(band);
    else color.copy(DATUM).lerp(height >= 0 ? RAISED : LOWERED, Math.min(Math.abs(height) / 2, 1));
    colors[offset] = color.r;
    colors[offset + 1] = color.g;
    colors[offset + 2] = color.b;
    offset += 3;
  }

  for (let iy = 0; iy < map.ny - 1; iy++) {
    for (let ix = 0; ix < map.nx - 1; ix++) {
      const centreX = (ix + 0.5) * map.spacing_m;
      const centreY = (iy + 0.5) * map.spacing_m;
      const clearance = Math.min(centreX, centreY, spanX - centreX, spanY - centreY);
      // The revision-1 grid puts both band boundaries exactly on cell edges.
      const band = clearance < MAP_PAUSE_M ? PAUSE : clearance < MAP_WARNING_M ? WARNING : null;
      // MuJoCo 3.9.0: 00→11 diagonal, upward winding after the frame rotation.
      vertex(ix, iy, band);
      vertex(ix + 1, iy, band);
      vertex(ix + 1, iy + 1, band);
      vertex(ix, iy, band);
      vertex(ix + 1, iy + 1, band);
      vertex(ix, iy + 1, band);
    }
  }

  const surface = new THREE.BufferGeometry();
  surface.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  surface.setAttribute("color", new THREE.BufferAttribute(colors, 3));
  surface.computeVertexNormals();
  surface.computeBoundingBox();
  surface.computeBoundingSphere();

  const lines: number[] = [];
  const lineColors: number[] = [];
  const cellsPerMetre = Math.round(1 / map.spacing_m);

  function lineVertex(ix: number, iy: number, lineColor: THREE.Color) {
    lines.push(
      map.origin_xy_m[0] + ix * map.spacing_m,
      (map.heights_mm[iy * map.nx + ix] as number) / 1000,
      -(map.origin_xy_m[1] + iy * map.spacing_m),
    );
    lineColors.push(lineColor.r, lineColor.g, lineColor.b);
  }

  function lineColor(index: number, count: number, along: number, span: number): THREE.Color {
    const clearance = Math.min(index, count - 1 - index) * map.spacing_m;
    // An inset boundary is a closed square, not a line extending through the outer bands.
    if (Math.min(along, span - along) < clearance) return GRID;
    if (clearance === 0) return EDGE_LINE;
    if (clearance === MAP_PAUSE_M) return PAUSE_LINE;
    if (clearance === MAP_WARNING_M) return WARNING_LINE;
    return GRID;
  }

  // Each metre line is segmented at every authoritative sample. Every segment
  // lies on an actual cell edge, including across bumps and potholes. The mesh's
  // polygon offset resolves depth conflict without lifting a second plane.
  for (let ix = 0; ix < map.nx; ix += cellsPerMetre) {
    for (let iy = 0; iy < map.ny - 1; iy++) {
      const tint = lineColor(ix, map.nx, (iy + 0.5) * map.spacing_m, spanY);
      lineVertex(ix, iy, tint);
      lineVertex(ix, iy + 1, tint);
    }
  }
  for (let iy = 0; iy < map.ny; iy += cellsPerMetre) {
    for (let ix = 0; ix < map.nx - 1; ix++) {
      const tint = lineColor(iy, map.ny, (ix + 0.5) * map.spacing_m, spanX);
      lineVertex(ix, iy, tint);
      lineVertex(ix + 1, iy, tint);
    }
  }
  const grid = new THREE.BufferGeometry();
  grid.setAttribute("position", new THREE.Float32BufferAttribute(lines, 3));
  grid.setAttribute("color", new THREE.Float32BufferAttribute(lineColors, 3));
  grid.computeBoundingBox();
  grid.computeBoundingSphere();
  return { surface, grid };
}
