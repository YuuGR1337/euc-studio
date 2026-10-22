# Begode Extreme model

3D scan by Tarik Caramanico, cleaned for EUC Studio; part of this repository under its license.

The scan was cleaned and completed: flat panels and round parts were trued, the tire was rebuilt with
a regular tread, and the motor covers, rim ring, display window, rear roller parts, battery packs,
rear shock, rear handle and pedal decks were modelled. `docs/hardware/geometry.md` explains each
step; `profiles/begode-extreme/geometry.toml` holds the measurements.

Parts that follow the owner's photos (2026-10-16) or an estimate, to be refined from his measurements:

- **Rear shock.** DNM coil-over without reservoir, 200 mm eye to eye and 750 lb/in (MEASURED by the
  owner), 56 mm x 115 mm spring. It runs forward and down from its top eye on the rocker, under the
  roller, to its bottom eye on the carriage. Stroke ESTIMATE. No brand marks.
- **Rear linkage: rocker and link per side.** Current linkage per owner photos; the scan shows an
  older system, which is not drawn. The rocker plate (outline, window, empty hole), the top eye and
  the rocker pin are placed from the owner's photos; the link runs from the rocker pin to the
  carriage. The rocker's frame pivot and the carriage clevis are hidden in every photo and are
  ESTIMATES (`docs/hardware/geometry.md`, "Rear linkage").
- **Rear handle.** Orange CNC U-handle with a black grip bar. The arms' outer faces are in the
  scan; their three windows, blind pocket, foot and the grip position follow the owner's photos.
- **Pedals.** The decks were not on the wheel for the scan; their shape and size are an ESTIMATE.

| File                      | Triangles | Size    | Use                  |
| ------------------------- | --------- | ------- | -------------------- |
| `begode-extreme-lod0.glb` | 294k      | 2.5 MB  | close-ups            |
| `begode-extreme-lod1.glb` | 114k      | 1.1 MB  | default view         |
| `begode-extreme-lod2.glb` | 41k       | 0.37 MB | far away, thumbnails |

The files use meshopt compression (`EXT_meshopt_compression`, `KHR_mesh_quantization`). three.js
loads them with `GLTFLoader` and the `MeshoptDecoder`; drei's `useGLTF` enables it by default.

## Axes and nodes

Metres, glTF axes: x forward (headlight end), y up, z right. The origin is the axle centre, so put the
model 0.2296 m above the ground (the unloaded tire radius).

```text
begode-extreme
├─ wheel                 spin about z to roll (a positive angle about -z rolls forward)
│  ├─ tire
│  ├─ hub                motor covers and screws
│  └─ rim                orange flange ring
└─ body                  pitch and roll the shell here
   ├─ shell              grained plastic: normal map on TEXCOORD_0 (LOD0, LOD1)
   ├─ seat
   ├─ roller_sleeve_left, roller_sleeve_right   rear roller halves (smoked grey): the RGB tail light
   ├─ roller_center_band
   ├─ ble_led            blue status LED of the Bluetooth data board
   ├─ rear_handle        orange CNC U-handle with the black grip bar
   ├─ speaker_left, speaker_right   roller end caps (speakers)
   ├─ handle             side plates and tube
   ├─ headlight
   ├─ headlight_lens     the four lenses to light
   ├─ display_window     smoked glass; TEXCOORD_0 spans the T16-007 LED panel
   ├─ shock              coil-over only (body, collar, spring, shaft, red knob, eyes); see below
   ├─ shock_links        the rocker's pivot shaft, the front frame blocks and their pin (static)
   ├─ shock_mount        the carriage's clevis and bottom shaft: moves with the axle
   ├─ rocker             both rocker plates, the top-eye and rocker-pin cross tubes and spacers,
   │                     chrome bolt heads: turns about z
   ├─ link               both link plates: turns about z on the rocker pin
   ├─ pedal_mount_left, pedal_mount_right
   ├─ axle
   └─ pedal_left, pedal_right   origin on the hinge pin: fold about x
```

- **Shock and linkage.** Current linkage per owner photos; the scan shows an older system. `shock`
  has its origin on the top eye; its local +Y axis points from the bottom eye to the top eye, and
  the bottom eye is at local (0, -0.200, 0), so scaling Y about the origin moves the bottom eye.
  `shock_mount` has its origin on the bottom eye, `rocker` on its frame pivot and `link` on the
  rocker pin; at rest the four origins are the linkage at full extension
  (`profiles/begode-extreme/geometry.toml` `[suspension] linkage_pivots_xz_m`). With the axle
  `t` metres up the rods, `shock_mount` moves by `t` up the rods (0.13 degrees rearward), the
  rocker turns about z until the link's length fits between the rocker pin and the bottom eye,
  the link turns to point at the bottom eye, and the shock spans the two eyes. The studio does
  this from the Suspension channel (`app/src/workspaces/ride/shockLinkage.ts`). The pivot is an
  ESTIMATE, so the drawn shock length can differ from the engine's shock channel (up to 17 mm).
- **Pedals.** `pedal_left` folds up with rotation.x = +90 degrees, `pedal_right` with -90 degrees.
  Their shape and size are an estimate (see above).
- **Display.** On `display_window`, u and v run from 0 to 1 across the 82.6 x 55.2 mm LED panel of
  `profiles/begode-extreme/display.toml`, u to the right and v down as in that file, with the top of
  the digits toward the front. Use the texture with `flipY = false`.
- **Lights.** `headlight_lens`, the two `roller_sleeve_*` nodes (smoked grey when unlit, as in the
  owner's photos; the orange-red diffusers over the tail-light LED rings show when they glow) and
  `ble_led` have their own materials, so the app can make them
  emissive. `profiles/begode-extreme/lights.toml` places the LED rings inside the sleeves.
- **Colours.** Each part has its own PBR material; ambient occlusion is baked into the vertex colours.
