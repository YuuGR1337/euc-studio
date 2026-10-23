# Free Ride redesign (FR)

Status, 2026-10-23: FR1 study and plan approved. Built: FR2 (controls), FR2b (native controls,
in the physics), FR3 (the Chase camera), FR4 (the city's daylight), the
play mode (no clicks, crash freely, respawn, the wheel's pad; see the end of the decisions), drive
mode (a throttle; an intent-level rider in the physics), the HUD's motor readout, FR6's first
sound phase (motor, tyre, wind; opt-in and experimental), and the rider body: first EUC Thrills' Cool Rider, copied, now
the street kit, EUC Studio's own look of the owner's riding outfit. Not built: FR5, the rest of
FR6 (gauges, warning pills, impact sounds), FR7, FR8, FR9's effects and FR10.

Tarik (Human) asked for a better Free Ride: the world looks primitive, there is no sky, and the
controls feel bad. He pointed at [EUC Thrills](https://github.com/VibezZzCoder/EUC-thrills), an
open-source browser game about electric unicycles by
[VibezZzCoder](https://github.com/VibezZzCoder) ([play it](https://vibezzzcoder.github.io/EUC-thrills/)),
as the reference for world design, controls, body and polish.

Claude studied EUC Thrills at commit
[`a66dabd`](https://github.com/VibezZzCoder/EUC-thrills/tree/a66dabd56686936516465f47123c7f54990692e7)
(v1.0.0, 2026-10-18): it read the source and rode the built game headlessly. This document lists
what makes the game feel polished, decides item by item what Free Ride does with each idea, and
proposes the order of the work. EUC Thrills is credited here and wherever one of its ideas lands
in our code.

## What is weak today

Claude's reading of the code at the start (`app/src/workspaces/freeRide/`,
`app/src/workspaces/ride/`), before FR2. FR2, FR3, the rider body and the play mode have since
answered each point:

- **No sky or daylight.** The background and the fog are black (`surface.bg`). The fog closes
  from 18 m to 110 m. Light is the studio rig: a white-over-black hemisphere at 0.35, one key
  light whose shadow box is ±2 m around the wheel, and in-scene Lightformer softboxes.
- **Plain ground and buildings.** Ground colour is one flat colour per material texel, tuned
  dark for the studio. Buildings are coloured boxes. Props are simple instanced parts.
- **Keys are steps.** In Free Ride a key sends ±10° of lean and ±30°/s of turn at once, with no
  ramp (`controller.ts`, `read()`). Ride's manual mode ramps its keys, but uses other limits
  (keys reach 9° of lean, a stick 15°, and turns reach 0.8 rad/s). The two mappings differ.
- **Slow turning.** 30°/s is the most Free Ride can ask for at any speed. Below 1.5 m/s the
  physics fades manual turn requests to zero, so the rider cannot turn on the spot.
- **Arming takes several steps.** Choose a source, choose a gamepad, wait for 250 ms of
  neutral input, press "Arm and ride". The safety rules are right; the steps around them are many.
- **Side-on camera.** The follow camera sits 1.8 m to the side at a fixed 42° field of view.
  Its arm stretches 1.1 m with speed, but the heading spring is the same at every speed, there is
  no look-ahead in time, and it only keeps above the ground, so it can pass into buildings.
- **A mannequin rider.** The rider is drawn from its physics capsules in greys.
- **No ride sound** beyond the firmware buzzer, which is already exact.

## Constraints that shape every choice

1. **Physics first.** The wheel is MuJoCo physics plus real firmware on an emulated MCU. Input
   only sets rider intent: lean (the firmware answers with torque), steer (heading target), and
   body motion through the articulated rider. Nothing sets speed, torque, position or velocity.
2. **The firmware owns the wheel's behaviour.** Beeps, tiltback, cut-out, speed and battery come
   from the firmware and the electrical plant. The studio never invents them.
3. **A developer tool, honest by default.** Visuals never change physics. A visual must not
   suggest physics that is not there (a solid-looking object the wheel rides through, a body
   pose the physics does not hold). The rider figure follows the physics bodies part by part.
   (Until 2026-10-23 every cosmetic layer could also be switched off to show the bodies' raw
   shapes; the owner had that view removed, so the figure is the only way the rider is drawn.)
4. **The Tauri webview.** macOS WebKit with WebGL 2 on Metal. The simulation thread already uses
   about 56 % of one core in Free Ride (`docs/models/world.md`). Rendering must leave the UI
   responsive. A software renderer gets the low tier automatically.
5. **The studio design system** (`app/DESIGN.md`). HUD and settings use our tokens and
   components (dark cards, orange accent, semantic yellow and red, rolling digits). The game's
   own UI style is not copied.
6. **The world's determinism rules.** Anything generated in `euc-world` hashes integer
   coordinates with the seed, never a running random stream. Visual-only detail drawn in the
   studio follows the same rule so that a recording replays the same picture.

## Licence handling

EUC Thrills code is MIT (© 2026 VibezZzCoder). Its original assets (3D models, textures,
materials, procedurally generated geometry and audio, UI art) are CC BY 4.0. Its
[NOTICE](https://github.com/VibezZzCoder/EUC-thrills/blob/main/NOTICE.md) excludes the eight
real riders' names, likenesses and personas, their logos, their contributed recordings and one
third-party mark, and says that presenting the work, or a light modification of it, as original
is a licence violation.

| What we take | How we handle it |
| --- | --- |
| Ideas, methods and starting values (the default) | Re-implemented in our own code, in our structure. A comment names the source, for example "Starting values from EUC Thrills (MIT), src/render/chaseCamera.ts". This document is the main credit. |
| Code ported or translated closely | The file keeps EUC Thrills' MIT copyright and permission notice in its header. NOTICE gets a "Code adapted from EUC Thrills" block with the full MIT notice, and its sentence "This repository does not vendor third-party source code" is updated. The ported parts stay MIT. No phase below plans a close port; the fallback exists if one turns out cleaner. |
| An original asset, or the output of their asset painter run as is (sky, facade atlas, grass strokes, audio) | Not planned. If a phase uses one: the attribution line their NOTICE asks for ("Assets from EUC Thrills by VibezZzCoder, CC BY 4.0, https://github.com/VibezZzCoder/EUC-thrills"), a note of what changed, the file kept under CC BY 4.0 (CC BY forbids extra restrictions, so PolyForm Noncommercial cannot cover it), and a NOTICE entry. |
| Their CC0 sound loops | Not used. Ride sounds are synthesized (below). If a recording is ever needed, it is fetched from its original CC0 source with provenance recorded. |
| Real riders, logos, contributed and crash recordings, machine-generated vocals, the third-party mark | Never used, not even as inspiration for a preset. Our rider kit is generic and fictional. |

## What makes EUC Thrills feel polished, and what Free Ride does with it

Decisions: **adopt as code** (port with attribution), **adopt as asset** (CC BY with
attribution), **re-implement** (the idea in our own code), **skip**. "World" is the city world
and its drawing (`euc-world`, terrain rendering); "Free Ride" is the Free Ride view: controls,
camera, rider visuals, HUD and audio. "The physics" is `euc-physics` and the native hosts. File paths in the "What they do" column are EUC Thrills paths.

### World look

EUC Thrills has no post-processing at all. Its polish is colour discipline: a painted sky whose
bottom colour is exactly the fog colour, one sun shared by sky and shadows, a muted mid-value
palette mixed in linear light, and textures painted in code. Claude rates this the most useful
lesson in the game.

| Item | What they do | Decision | Why, and how for us |
| --- | --- | --- | --- |
| Sky | An equirectangular image painted on the CPU once per level (`src/render/skyImage.ts`): a horizon-to-zenith ramp shaped by `y^0.62`, warm forward scatter low toward the sun, value-noise clouds projected on a flat plane overhead, a Gaussian sun core and glow. Zenith #5892d8, horizon #bcd6ee, 1024 × 512, set as `scene.background`. | Re-implement (World) | Cheap, offline, no image files. Our painter, our palette, seeded from the world seed so clouds repeat with a recording. Only on a world; the studio sweep stays black for scenarios. |
| Sun and fill | One `HemisphereLight` (sky #9dc4ea, ground #b2ab97, 1.10) and one sun (#fff4e6, 2.6). The painted sun and the light share azimuth and elevation, so shadows agree with the sky. | Re-implement (World) | Same structure. Intensities are re-tuned for our tone mapping (next row). A fixed afternoon sun first. |
| Tone mapping | ACES filmic, exposure 1.0. Colours authored lighter than real black gear, because near-black crushes under ACES. | Skip ACES; keep our Neutral tone mapping | On 2026-10-16 Neutral at exposure 1.5 was chosen because ACES turned the wheel's orange handle red against the owner's photos. Sky and palette are tuned for Neutral instead. The wheel colours are checked again under daylight. |
| Reflections | Ultra only: the sky repainted at half float, the lower half replaced by ground bounce, PMREM-filtered into a 256 cube. | Re-implement (World) | On a world, the studio Lightformers make the shell reflect a photo studio. A PMREM of our own sky (one 256 cube, about 6 MB, built once) fits. |
| Shadows | One shadow box of ±30 m that follows the rider, 1024 or 2048 map, `PCFShadowMap`. Ultra snaps the box to whole texels and pushes its centre 19 m ahead along the camera's forward. | Re-implement (World) | Ours covers ±2 m. A 2048 box of about ±25 m, snapped to texels and pushed ahead, so buildings, trees and the rider all cast. Their 16-tap Vogel filter and baked far shadow map are skipped (cost). |
| Fog | Linear fog from 120 m to 470 m in the horizon colour. Fog never touches the sky. | Re-implement (World) | The key rule is fog colour = sky horizon colour, so the ground's far edge dissolves into the sky. Our distances are bounded by the stream: today level 1 ends at 104 m. With ring 2 (designed in `docs/models/world.md`, not built) the fog can run about 60 m to 240 m. |
| Post-processing | None. MSAA, tone mapping and the palette do the work; specular anti-aliasing in shaders. | Skip post (agree) | No composer, no bloom. Keep MSAA. |
| Palette | Muted mid-values everywhere, no pure white even in road paint, roughness 0.88 to 1.0. Grass #435f3c, pavement #797b7e, brick #6f5046, walls #bdb5a0 to #c2a088. | Re-implement (World) | Our `PALETTE` in `worldMesh.ts` is "muted for the dark studio" and too dark for daylight. Re-tune it against the sky, keep material identity (asphalt, worn asphalt, concrete, pavers, cobbles, gravel, grass, dirt, paint, metal). |
| Ground variation | Per-cell vertex tint from five noise scales (1 m, 5.7 m, 27 m, hue 19 m, saturation 41 m), amplitude per material (pavement 0.075, grass 0.20). | Re-implement (World) | Breaks the flat look at no texture cost. Hash-based, so deterministic. |
| Ground detail | One 256² × 7 `DataArrayTexture` painted in code (grass strokes, gravel stones, soil grit, wood grain, pavement grain), sampled in world XZ at a metre scale per material, twice with a rotation plus once at 0.17× to hide tiling; joints fade with pixel footprint. | Re-implement (World) | Our ground already has real joints, setts and cracks in its height (physics feels them). The detail texture must draw what the physics has: concrete joints every 1.5 m, 14 cm setts, paver joints. Draw what the wheel feels. |
| Road markings | One merged ribbon mesh lifted 15 mm, vertex colour paint with wear; dashes 3 m with 4.5 m gaps. | Re-implement later (World) | Our paint is texels at 6.25 cm in the material grid. Ribbons from the road graph give sharper lines; do it after the palette and detail work. |
| Contact grounding | Ultra: analytic AO pools under the wheel (r 0.6 m) and the body (r 1.05 m, stretched along the sun) in the ground shader, plus per-vertex AO from props. | Re-implement (World, data from Free Ride) | The strongest cheap "grounded" effect. Two points per frame (wheel contact, pelvis) come from the physics poses. Replaces drei `ContactShadows` on a world. |
| Buildings | A metric box with a look composer: commercial with a 0.75 m parapet and a set-back tower, residential with a pitched roof (rise min(span × 0.42, 7 m)), industrial with sawtooth roofs. Storey bands by height (2, 4 or 11). A procedural 512² facade atlas whose texels only darken the instance colour. All instanced unit parts. | Re-implement (World) | Our buildings are already exact physics boxes from `euc-world` parcels. The look is drawn on top: kind, roof and storeys chosen by hash from the parcel, sent in the render payload only (physics unchanged). Their storefront interiors and per-building metric facades are skipped. |
| Skyline | 72 distant blocks in a ring beyond the course, 14 to 60 m tall. | Re-implement (World) | Gives the horizon a city. For an infinite world: ring 2 chunks at low detail plus cheap far silhouettes from parcel tiles, under the fog. |
| Trees and shrubs | Instanced, vertex-coloured, no textures: lobed blob crowns, tiered conifers, squashed shrubs; three LODs in one index buffer chosen by `drawRange`; batched in 64 m cells. Trunks are solid boxes; shrubs are soft (drag, no crash). | Re-implement (World) | Trees are already `euc-world` props with box colliders. Better crowns, a conifer kind for parks, LOD by draw range. Soft shrubs need a physics decision (skip until then). |
| Grass | Instanced 9-blade clumps on a 1 m jittered lattice over grass cells, denser near roads, scaled to zero between 60 m and 100 m, wind sway in the vertex shader, no shadows. | Re-implement (World) | Big win for little cost. Same lattice-plus-hash rule. Shorter fade on low tier. |
| Street furniture | Lamps, benches (top rideable), bins, signposts, fence bays, tyre stacks, bollards. Solid, soft or visual-only by kind. | Re-implement (World) | We have lamp posts, benches, bins, hydrants and bollards as solids. Better models first. New kinds that a wheel can hit (fences, signposts) are added to `euc-world` as solids, never as visual-only parts. |
| People, vans, traffic | 20 people, 4 EUC riders, parked cars, 2 traffic vans on authored paths; physical upright boxes with look-ahead yielding; walk animation from distance travelled. | Re-implement later, honesty rule first (World) | A visual-only person or van that the wheel rides through misleads a physics tool. Option A: draw ambient life only outside the physics reach (more than 12 m from the wheel) and fade it out as the wheel nears. Option B: wait for kinematic mocap actors (a hook in `docs/models/world.md`). Open question for Tarik. |
| Birds | 9 birds in one draw call, crossing the rider's view, never landing. | Re-implement (World, optional) | Cheap life in the sky, far from any physics. |
| Hand-built venues (kickers, berms, decks, Switchback Park) | Authored features with entry and exit heights; "feature left, bypass right". | Skip for now | `euc-world` has plazas, stairs and ramps. A park or trail biome (designed, not built) is the place for features later. |
| Water at the ford | None; the ford is a dry dip with a boardwalk. | Re-implement our own (World) | We already have wet patches and potholes that hold water. Wet gloss and puddle surfaces are in world.md's "not built" list. |

**Built (2026-10-22): FR4** (`world: daylight for the city world (FR4)`): the
painted sky with one sun shared by the sky and its shadows, fog in the horizon colour, the sky's
environment for reflections, the daylight palette and contact pools under the wheel and the
rider, in world mode only; scenarios keep the studio sweep. `docs/models/world.md` has the
details.

### Camera

EUC Thrills' chase camera (`src/render/chaseCamera.ts`, constants in `src/data/tuning.ts`) is the
single biggest reason it feels fast and readable. Every smoothed value is a first-order lag with
no overshoot, and the camera reads only position, heading, roll, speed, airborne and crashed. All of
it carries over, because none of it touches physics.

| Item | What they do | Decision | Why, and how for us |
| --- | --- | --- | --- |
| Behind the rider | Directly behind, arm 4.2 m at rest stretching to 6.0 m at top speed (τ 0.55 s), 1.95 m high, aimed at hip height + 0.35 m. | Re-implement (Free Ride) | Our follow view sits 1.8 m to the side. Free Ride's default becomes a true chase; the side view stays in the view cycle. |
| Speed field of view | Vertical FOV 1.13 rad (65°) at rest to 1.36 rad (78°) at top speed, τ 0.70 s (the slowest lag); a player trim of −8° to +12°. | Re-implement (Free Ride) | Ours is a fixed 42°. Speed reference is the run's tiltback speed (else 50 km/h), not a game top speed. |
| Yaw lag | Follows the rider's heading with a lag of 0.42 s at rest falling to 0.14 s at speed: slow pivots do not whip the camera. | Re-implement (Free Ride) | Plus one addition for a physics wheel: a low-pass filter on heading and height first, so wobble and pothole hits do not shake the view. |
| Look-ahead | Aim leads by 0.22 s × speed, at most 3.6 m (τ 0.35 s), zero in reverse; the aim uses the true heading, the body the lagged one, so the view leads into a turn. | Re-implement (Free Ride) | |
| Bank | 0.15 × the wheel's roll, capped at 0.10 rad, τ 0.18 s. | Re-implement (Free Ride) | Roll from the physics shell pose. |
| Obstruction | One ray from the aim to the wanted camera position, pull-in τ 0.05 s, restore τ 0.55 s, arm at least 1.6 m. | Re-implement (Free Ride) | Ray against the solids the stream already holds (oriented boxes), plus the ground height. Today the camera only stays above the ground. |
| Air and landing | Follows 35 % of height gained in the air; a landing dip of 0.028 m per m/s of impact (cap 0.14 m). No camera shake (their owner removed it). | Re-implement air follow; skip shake (Free Ride) | The landing dip waits until hops exist. |
| View toggle | C toggles chase and a slow diagnostic orbit. | Re-implement (Free Ride) | C already cycles our camera. Cycle: chase, orbit (user-dragged, existing), side (today's follow). |
| Reset snap | On reset the camera snaps behind the heading at rest length and FOV. | Re-implement (Free Ride) | Same on restart and on a new world. Reduced motion keeps cutting instead of easing (DESIGN.md). |
| Fixed step and interpolation | 120 Hz fixed step, camera stepped inside it, render interpolates. | Already ours | `prepareVisualFrame` interpolates physics poses; the camera runs per frame on interpolated poses with exact springs. |

### Controls

EUC Thrills inverts the real loop on purpose ("The game inverts the real EUC control loop",
`src/data/tuning.ts`): throttle sets a lean, the lean makes acceleration directly, steering sets a
yaw rate. We cannot copy that part. What carries over is how input becomes a smooth intent:

| Item | What they do | Decision | Why, and how for us |
| --- | --- | --- | --- |
| One input layer | Semantic actions; each device keeps its own state; axes merge as "strongest magnitude wins"; blur or a hidden tab clears everything; raw values, smoothing in one place downstream. | Re-implement (Free Ride) | Ride's manual mode (`lib/riderInput.ts`) and Free Ride (`controller.ts`) map input differently. One module serves both, with one set of limits and one test file. |
| Throttle to lean | `target = throttle × max lean`, chased with τ 0.14 s and a rate limit of 3 rad/s. Their comment calls τ "the single most important number" for responsiveness. | Re-implement (Free Ride) | Our intent is the same kind of thing: a desired lean the rider's ankles hold and the firmware answers with torque. Keys get a first-order lag (τ about 0.15 s) toward their level instead of a step; sticks and triggers pass through. The physics keeps its own lean-rate limit (12°/s before FR2b; manual lean 30°/s since), which is the body's limit. |
| Dead zones and curves | Stick: radial, rescaled, 0.18, capped at magnitude 1, linear after. Triggers: RT − LT, 0.08. Touch: 1.35 exponent. | Re-implement (Free Ride) | Keep our radial 0.15 with hysteresis; add triggers as forward and back lean (max of stick and trigger, never the sum); an optional response exponent in settings. |
| Brake to a stop, then reverse | Holding brake below 0.6 m/s for 0.35 s is needed before reverse; until then "a stop is a stop"; a shoulder-check glance shows during the wait. | Re-implement as intent logic (Free Ride) | Below 0.6 m/s the back-lean intent is pulled toward zero until back is held for 0.35 s more. This shapes intent only; the firmware still decides what the wheel does. |
| Steering authority by speed | Yaw-rate authority 3.6 rad/s at rest falling to 0.85 rad/s by 9 m/s (exponent 1.5), clamped so lateral acceleration stays under 0.75 g (1.05 g at speed) times grip; roll follows from `atan(a/g)`. | Re-implement within physics caps (Free Ride; the physics sets the caps) | Our physics already leans the wheel by `atan(v·rate/g)` and caps the lean at 20° (0.36 g). Steer input becomes a turn-rate request scheduled by speed and capped by that same lateral limit, so a fast turn goes wide for a reason the rider can feel. Before FR2b a 30°/s cap (Free Ride host) and 1.0 rad/s (physics) blocked the low-speed part; since FR2b a manual turn may ask up to 2.5 rad/s, and the physics caps it with speed. |
| Turning at walking pace | Steering is off below 0.05 m/s. | Physics decision | Before FR2b our physics faded manual turn requests to zero below 1.5 m/s. Since FR2b there is no fade: the rider turns at a standstill, as real riders twist the wheel (decision 3). |
| Crouch | Hold Shift or LB; knees bend (τ 0.07 s); charges a bigger hop over 0.40 s. | Re-implement crouch (Free Ride, studio backend) | `Physics::set_crouch(knee_rad)` exists (0 to 1.2 rad, no pose writes). Free Ride's input packet has a crouch field since FR2b, so crouch is a real body motion that lowers the centre of mass (and, since 2026-10-23, a tuck with less drag). |
| Hop, 180 spin, one-foot | Hop sets a launch speed of 3.0 m/s; the spin is a scripted 0.42 s heading sweep; one-foot is presentation only. | Skip | All three set kinematics directly. A physical hop needs the welded-feet rider to unweight and lift the wheel with its legs: a feasibility study in the physics first. |
| Quick reset | R puts the rider at the spawn, stopped and upright, keeps held keys, snaps the camera. | Re-implement (Free Ride; the physics defines what a reset does) | Built in the play mode: R starts a new run at once at the last clear spot (Shift+R at the world's start), the firmware booting again unthrottled, about 0.7 to 0.8 s from the key to riding. No re-pose with a running firmware. In Free Ride R is the respawn key, not the studio's restart. |
| One-shot buffer | Presses are buffered for 0.15 s, fire on the rising edge, and a press refreshes rather than queues. | Re-implement (Free Ride) | For reset, view and later hop. |
| Arming | None; the game is input-ready. | Re-implement our safety more simply (Free Ride) | Built in the play mode: no arm step and no neutral dwell. The keys ride once the firmware is up (the stand lets go 1.5 s after the build); the 250 ms watchdog stays, and older input counts as neutral, so the idle wheel balances. The first standard gamepad rides when its stick moves. |
| Rebinding | Settings rows with Change and Clear per action, stored by `KeyboardEvent.code` in localStorage; Escape reserved; pads not rebindable. | Re-implement (Free Ride) | Same model, in our `Kbd` and `Button` components. Conflicts with studio shortcuts are refused with a reason. |
| Onboarding | Three hints, cleared by doing: ride, brake, hop; worded per device. | Re-implement (Free Ride) | Ride, brake, carve. Small pills in the viewport. |
| Touch controls | Floating stick and buttons. | Skip | Desktop app. |

### Rider body

EUC Thrills builds its rider from rigid lofted parts, one group per bone, not a skinned mesh
(`src/render/rider.ts`, `src/render/blockoutKit.ts`, `src/render/coolRiderLook.ts`). That
matches our MuJoCo rider, whose poses arrive per body every frame.

| Item | What they do | Decision | Why, and how for us |
| --- | --- | --- | --- |
| Construction | Each part a loft: stacked cross-sections with a squareness exponent (2 an ellipse, about 3 a rounded box). Parts overlap past each joint; limb ends closed with hemispheres; parts sharing a joint and material merged. | Re-implement (Free Ride) | One group per MuJoCo body (torso, head, arms, thighs, shanks, feet), each part sized from that body's geoms so the silhouette matches what the physics simulates. |
| Clothing and gear | Moto jacket and trousers, accent panels, gloves with fingers, boots with soles, a double-walled helmet with a clear visor; a procedural seam atlas; colours kept lighter than real black so they do not crush. | Re-implement a generic, fictional kit (Free Ride) | No real rider's look. Muted mid-values, a reflective trim in our accent. Atlas later, if the plain kit is not enough. |
| Animation | Lean, carve, crouch, tuck, arm carriage, head stabilisation and look-into-turn, idle with a foot down: all computed from the controller's smoothed state, with analytic two-bone IK. | Draw physics poses as they are; cosmetic motion only where the physics has no joint (Free Ride) | The physics owns hips, knees, ankles and lean. Cosmetic layers only for what it lacks: head look into the turn (from steer intent, about 0.42 rad, τ 0.16 s), small hand and forearm offsets, the jacket hem. No counter-rolled upper body: it would put the drawn centre of mass somewhere the physics does not. A "Physics bodies" toggle showed the raw geoms (built, then removed on 2026-10-23 at the owner's request). |
| Crash ragdoll | 11 Verlet particles, a 0.12 s slerp from the riding pose. | Skip | Our rider falls physically. |

**Built (2026-10-22): their rider, copied.** Tarik asked for their body as it is, so the rows
above changed from "re-implement a generic kit" to a port. Claude copied the game's own fictional
rider, Cool Rider (MIT code, CC BY 4.0 look), into `app/src/vendor/euc-thrills` and adapted its
construction in `app/src/workspaces/ride/riderFigure/rig.ts`. Its 26 meshes are the game's,
geometry and materials byte for byte (checked against the game's own `createPlaceholderRider`),
and its painted tailoring sheet has the game's bytes (a test pins them).

The physics drives it (`riderFigure/skeleton.ts`, `figure.ts`):

- The articulated rider's trunk body carries the jacket, collar, panels, shoulders and head; each
  thigh, shank and foot body carries its own part. A thigh runs from a hip inside the jacket
  (the game's hip width) to the physics knee, a shin from the knee to the physics ankle, each
  stretched along the bone to the physics length; the boot is fixed to the foot body, its sole on
  the pedal (our ankle sits 8 cm above it, theirs 6 cm). The upper body is scaled so the helmet
  tops out at the physics head (0.99 for the default rider).
- The rigid rider is one body: the figure's pelvis sits at its hip height, and the knees are
  placed by two-link IK between the hip and the foot shapes, slightly bent.
- Cosmetic only, as decided: the arms ride bent and loose (since 2026-10-23 our own carriage,
  `CARRIAGE` in `figure.ts`, in place of the game's palms-forward ease at a standstill): hands a
  little ahead of the hips with the knuckles out, lower and closer at speed (from a speed
  filtered off the pelvis motion), and in a lean the high-side arm swings out and up while the
  low-side hand drops toward its knee. The head keeps 0.65 of the trunk's pitch off the eyes (at
  most 0.55 rad). No counter-roll and no rest pitch: the trunk's attitude is the physics'.
- (Removed 2026-10-23.) A Physics bodies button drew the simulated shapes instead; the street
  kit below replaced it.

The articulated physics stands with nearly straight knees at a cruise, so the figure does too;
their rider rests with soft knees. A softer-knee posture would be a physics change.

**Built (2026-10-23): the street kit, the only rider.** Tarik (Human) asked: "make the model use
clothes like i used on the photo i sent you. Make it resemble me", then "make it the default
rider. you can delete the other models like that white body and that one that we copied from the
OS repo". Claude wrote an original look, the street kit (`riderFigure/streetKit.ts`), from the
riding outfit (photos kept private, outside the repository): a black full-face helmet with a
mirrored visor, a chin bar, a small round rear-view mirror on the left and a chin strap with a
red tab; a loose black tee with bare forearms; orange gloves with black cuffs and knuckles; loose
light-blue jeans whose hems rest on the shoes; black hard knee guards with raised hexagon cells
and shin plates, strapped over the jeans; chunky white sneakers with laces. No logo or brand mark
anywhere. It is built with the game's loft kit and rig (MIT); none of its assets remain, so its
CC BY attribution went with Cool Rider's look. The Physics bodies view went too: rider bodies
draw only as the figure, and a body list the figure cannot bind draws no rider. The figure's hip
joints sit at a thigh body's origin when it is within 0.12 m of the midline (the lateral rider's
hips), else at the figure's own hip width (the sagittal rider's thighs hang from the pedals).

After a review the same day: the jeans are a mid-blue denim (the photos' mid-tones with
the shade taken out against the white shoe), baggy, with folds over the knee and stacked above
the shoe, lighter over the front and darker down the inner seam; the guards sit on their own
forms over the wider legs; the sneakers have a rounded toe and heel and a thicker, rounded sole
edge; and the arms ride in the carriage above. Then, to match the lateral rider's contact shapes
(a 5 cm knee ball, 3.5 cm shin and 5 cm calf capsules that touch the side pads), the wheel side
of the jeans is flattened to about 5.6 cm from the leg's axis and that of the guards to about
6.2 cm, so the drawn legs rest on the pads instead of sinking into them; the outside stays
baggy.

### Navigation targets (intent level)

Tarik asked for their navigation too. Their wheel is a kinematic controller: the throttle sets a
force-demand lean, the lean sets an acceleration, and the steer sets a yaw rate directly, held to
a lateral grip limit. Ours keeps the real firmware and MuJoCo, so the same feel becomes targets.
The input asks for an acceleration and a turn rate (the Free Ride view: input to intent, and its
tuning); the physics' rider controller leans and twists the rider to deliver them.
Today's mapping, where keys ask for a lean angle, stays as a "direct lean" option.

| Target | Their value | Where (EUC Thrills `src/`, commit `a66dabd`) | Our use |
| --- | --- | --- | --- |
| Force-demand lean at full throttle | 0.50 rad | `data/tuning.ts:1005` (`maxLeanPitch`) | Planned: full W or stick asks for 16·sin(0.50) = 7.7 m/s² before drag. Built: full throttle asks for the engine's `accel_cap_mps2` (below) |
| Lean response | τ 0.14 s, at most 3.0 rad/s | `data/tuning.ts:1013`, `:1021`; `shared/maths.ts:41` (`approach`); `simulation/EucController.ts:3137` | The stick's throttle ramps the same way (`DRIVE_TIMING`: 0.14 s, 6 shares a second); the keys keep their own 0.15 s lag |
| Drive | 16.0 m/s² per unit of sin(lean) | `data/tuning.ts:1027` (`leanToAccel`); `EucController.ts:3157` | Asked acceleration while the demand and the travel agree |
| Brake | 22.0 m/s² per unit of sin(lean), when the lean opposes travel above 0.05 m/s | `data/tuning.ts:1034` (`brakeAuthority`), `:1094` (`stoppedSpeed`); `EucController.ts:3155-3157` | Asked deceleration |
| Drag | 0.00867·v·\|v\| m/s² | `data/tuning.ts:1079` (`dragCoefficient`) | Not used: the physics' own drag (rolling resistance and air) is in the caps, and a released throttle asks for minus that drag, so the wheel coasts as theirs does |
| Reverse gate | Below 0.6 m/s, asked for 0.35 s; reverse at most 6.7 m/s | `data/tuning.ts:1105`, `:1114`, `:1132`; `EucController.ts:3214-3236` | Lean mode keeps FR2's `BrakeGate`. In drive mode the engine's clean stop does the job: a brake to a stop ends standing, and back pressed again from a stop rides backwards. No reverse speed cap yet |
| Yaw authority | 3.6 rad/s near a stop down to 0.85 rad/s at 9 m/s, by (v/9)^1.5 | `data/tuning.ts:1193`, `:1195`, `:1197`, `:1214`; `EucController.ts:3245-3246` | Already in FR2 (`STEER_SCHEDULE`) |
| Lateral grip | 0.75 g to 9 m/s, rising to 1.05 g at 22.25 m/s; up to 1.2 g more for a hard steer (0.55 to 0.90 of full), gone by 6 m/s | `data/tuning.ts:1225`, `:1250`, `:1261`, `:1283`, `:1300-1306`; `simulation/lateralCeiling.ts:70`; `EucController.ts:3325-3338` | Caps the asked turn rate at \|v·ω\| ≤ grip·g; the physics' own cap (FR2b: min(2.5, g·tan 35°/\|v\|)) still applies |
| Steer at a stop | None below 0.05 m/s | `EucController.ts:3272` | Ours differs: the rider turns at a stop through the physics (decision 3) |

Interface, built in two halves (engine: the physics and the Free Ride host; frontend: the Free
Ride view, FR-drive): `free_ride_input` gains `desired_accel_mps2`, along the heading,
+ forward, at most 12 m/s².
When it is present the packet is in drive mode and `desired_lean_rad` must be 0; when it is
absent the packet is direct lean, as today. The status adds `accel_cap_mps2` and
`brake_cap_mps2` (the most the rider's lean can ask at the current speed, drag included) and
`accel_mps2` (measured, filtered over about 0.1 s). A lean gives only g·tan(lean), and drive mode
allows about 20° to 22°: about 3.6 to 4.0 m/s², far below their 7.7 and 10.5 m/s². So full
throttle means the cap: the ask keeps their shape and lag, scaled to `accel_cap_mps2` or
`brake_cap_mps2`.

The frontend (`driveAsk` in `lib/rideControls.ts`) reads the caps as `g·tan(22°) ∓ drag`: their
mean is a full lean's share and half their difference is minus the drag, so the ask is
`throttle · (accel_cap + brake_cap) / 2 + (accel_cap - brake_cap) / 2`. Full throttle gives the
forward cap, full back the brake cap, and a released throttle minus the drag (a coast; an ask of
0 would hold the speed). Drive is the default; "Lean" under Riding keeps the direct lean, and an
engine without drive caps always rides that way.

### Wheel and effects

| Item | What they do | Decision | Why, and how for us |
| --- | --- | --- | --- |
| Wheel model | Lofted shell, tyre, pedals, lights, suspension stanchions whose exposed length matches the simulated travel. Spin is invisible on a plain tyre. | Mostly already ours; re-implement a spin cue (Free Ride) | Our scanned Extreme with its firmware-driven display and tail light is more exact. Add a valve or tread marks so wheel rotation (from the wheel joint angle) can be seen. |
| Dust | Pooled `THREE.Points`, deterministic by a spawn counter hash; per-surface colour fading to the ground's colour; 22 particles per landing. | Re-implement (Free Ride) | Driven by wheel speed and the surface under it (gravel, dirt, grass). |
| Pedal-scrape sparks | 150 per second from the pedal edge, located from the pose. | Re-implement (Free Ride; the contact flag from the physics) | From the real pedal or shell contact with the ground, which MuJoCo knows. Needs a contact signal in telemetry. |
| Spray | Only on spill surfaces. | Re-implement later (Free Ride) | On our wet patches and water in potholes. |
| Skid marks | None. | Skip | |

### HUD

EUC Thrills shows less than we have (no battery, PWM or temperature). Its strengths are layout and
warning discipline (`src/ui/hudModel.ts`, `src/ui/game.css`).

| Item | What they do | Decision | Why, and how for us |
| --- | --- | --- | --- |
| Layout | A grid: objective top left, status top right, centre empty, speed bottom left, cues bottom right; safe-area insets; the HUD ignores the pointer. | Re-implement (Free Ride) | In our viewport, with our tokens. |
| Speed | A large whole number with tabular figures, no smoothing ("a glance is all a rider has"); km/h or mph. | Keep ours | We already show `RollingNumber` (DESIGN.md: numbers never jitter, at most 8 updates a second). Whole km/h in Free Ride. Units toggle is an open question. |
| Battery, PWM, temperature | Not shown. | New (Free Ride) | Firmware and plant values: charge and voltage, PWM with caution and danger zones from the profile's alarm and tiltback settings (our `Gauge`), motor temperature when it rises. |
| Warnings | One pill climbing "Working hard", "Ease off", "Tilt-back"; warnings rise at once and clear after 0.7 s; colours borrowed from the wheel's rear status light. | Re-implement from firmware events (Free Ride) | The words and the moment come from the firmware's alarm, tiltback and cut-out events, never from our own thresholds. The 0.7 s hold is display-only. |
| Beep you can see | A "Max speed" pill that blinks in step with the beep period, so the warning survives muted sound. | Re-implement from real buzzer edges (Free Ride) | We have the firmware's exact tone changes. The indicator flashes on them, so it shows exactly what the firmware sounds, muted or not. |
| Accessibility | Polite live region for warnings, reduced motion keeps the warning lit, diff-only DOM writes. | Re-implement (Free Ride) | Fits DESIGN.md's reduced-motion rules. |
| Look | Their typography and colours. | Skip | DESIGN.md governs. |

**Built (2026-10-23): the motor readout.** While riding, the Free Ride HUD shows "PWM (firmware)",
the firmware's own PWM figure, and "Motor headroom (sim)", 100 % less the simulated modulation.
Only the headroom warns: at 25 % or less it turns the caution colour and reads "low" (on stock the
modulation sits at 77 to 83 % for 10 to 15 s before a cut). The PWM zones planned above are not
built: no threshold of ours draws them. The other rows (layout, gauges, pills, the beep you can
see) wait for the rest of FR6.

### Audio

| Item | What they do | Decision | Why, and how for us |
| --- | --- | --- | --- |
| Warning beeps | A 75 ms 2565 Hz sample with an invented geometric cadence (1.10 s to 0.14 s); load-ladder beeps coded but silenced by their owner. | Skip their beeps | Our buzzer already plays the firmware's own tone changes (DESIGN.md, 2026-10-16). Kept. |
| Mix | Buses (ride, warnings, master) through a limiter; warnings on their own bus and slider "so the wheel can still tell you"; every change glides with `setTargetAtTime`. | Re-implement (Free Ride) | Built: one master volume and Mute for both, and the ride sound's own opt-in switch; the ride sound ducks, the buzzer is never ducked. |
| Wind | Pink noise through a band-pass sweeping 240 Hz to 1.2 kHz with speed; gain `0.17 × ramp^1.7` from 9 m/s; ×1.3 in the air. | Re-implement synthesized (Free Ride) | From the physical speed. No samples. |
| Tyre | Per-surface noise bands (pavement 1750 Hz down to wood 330 Hz), a loop that taps once per wheel revolution, grain from suspension speed, 0.2 s equal-power crossfades between surfaces. | Re-implement synthesized (Free Ride) | Surface from `euc-world` under the wheel, taps from the wheel angle, grain from our suspension channel. |
| Motor | A whine at the electrical frequency (rotation × 15 pole pairs) with load opening a filter; built but set to zero by their owner because a real wheel is nearly silent. | Re-implement, off by default with the rest of the ride sound (Free Ride, decision 8) | We have real electrical frequency and current, so ours is honest: the pitch from the plant's pole pairs and the wheel speed, the level from the phase current. |
| Ducking | The ride bed ducks 0.22 on each beep (30 ms attack, 300 ms release). | Re-implement (Free Ride) | Duck on the real buzzer signal. |
| Recordings | Wind, offroad tyre and siren loops (CC0); crash voices (permission-only or machine-generated). | Skip | Synthesis covers wind and tyre; nothing else is needed. |

**Built (2026-10-23): FR6 S1.** The ride bed plays natively beside the buzzer, on simulated time
(`app/src-tauri/src/engine/ride_sound.rs`, fed each sample by the Free Ride host): the motor
(electrical frequency from the plant's pole pairs, level from the phase current, low), the tyre
(speed and the euc-world material under the wheel, joint ticks, wet hiss, the tread's 15 knob
repeats from `profiles/begode-extreme/geometry.toml`), the wind (speed), ducked by 0.22 under
each beep. Volume and Mute sit in the panel; a hidden window keeps only the buzzer. Nothing runs
on the frame loop. On 2026-10-23 the owner judged the synthesized ride sound poor: it is off by
default and stays as an opt-in "Ride sound (experimental)" switch for contributors to improve;
the buzzer plays as before. S2, the impacts (kerb thump, pedal scrape, crash thud), waits for a contact
digest from the physics.

### Settings and quality tiers

| Item | What they do | Decision | Why, and how for us |
| --- | --- | --- | --- |
| Quality tiers | Low (pixel ratio 1, no shadows), Medium (1.5, 1024), High (2, 2048, default), Ultra (4096 near shadow, baked far shadow, PMREM, facade normal maps). Grass and tree distances per tier. "The ride is identical at every setting." | Re-implement Low and High; skip Ultra (World, Free Ride) | Our `quality.ts` already picks low on a software renderer. High gains sky, sun shadows, grass and the outdoor environment; Low keeps the sky, drops shadows and grass and shortens the fog. Ultra's cost does not fit a tool that also runs the simulation. |
| Shader warm-up | `renderer.compile`, wait for programs, one zero-size render before play. | Re-implement (World) | No hitch when a new material appears in the stream. |
| Budget contract | Draw calls and triangles predicted from the level plan; a level over budget is regenerated. | Skip | Our world streams; we measure per frame instead (below). |
| Settings list | Quality, FOV trim, units, frame counter; master, ride and warnings volumes, mute; key rebinding; pad on/off and dead zone. Every stored option is coerced, so a bad record falls back to defaults. | Re-implement (Free Ride) | A Settings disclosure in the Free Ride panel: World detail (Auto, High, Low), FOV trim, units, view on start, ride sound and buzzer volumes, bindings, dead zone, response curve, invert. localStorage, coerced on read. |

## Performance budget

Claude's targets for High, measured with the studio's existing frame-time readout during a
Free Ride on the city world, at 1440 × 900 and at 1280 × 720:

- 60 frames per second on Apple silicon; main-thread work per frame under 8 ms.
- At most about 250 draw calls and 800 000 triangles in view.
- GPU memory for textures and targets under 200 MB.
- The simulation thread unchanged (the sky, shadows and HUD never reach it).

Each phase reports these numbers before and after, and a headless Playwright screenshot in the
style of `app/e2e/world-view.spec.ts`.

## Phases

Two tracks run in parallel: track A is the Free Ride view, track B the city world. Efforts are
Claude's estimates of working time: S under a day, M one to three days, L
three to six days. Each phase ends with tests, the numbers above, and a ride by Tarik (Human)
before the next one starts.

| Phase | Track | Content | Effort | Depends on |
| --- | --- | --- | --- | --- |
| FR2 Controls | A, Free Ride | One input module for Ride and Free Ride; key lag toward the lean level; triggers and D-pad; speed-scheduled steer within today's native caps; brake-to-stop gate; implicit arm on first push; automatic gamepad; one-shot buffer; rebinding; onboarding hints. Unit tests for every curve. | M | Nothing |
| FR2b Physics-side controls | The physics | Raise the manual turn caps (Free Ride host 30°/s, physics `turn_rate_max_rad_s` 1.0) and review `turn_lean_max_deg` 20° and `lean_rate_deg_s` 12°/s with firmware scenarios; turning at walking pace; a crouch field in `free_ride_input` to `set_crouch`; quick-reset semantics; pedal contact flag in telemetry; hop feasibility with welded feet. | M | Tarik's answers |
| FR3 Camera | A, Free Ride | Chase camera with speed arm and FOV, yaw lag table, look-ahead, bank, filtered heading and height, obstruction against stream solids, air follow, reset snap, view cycle. | M | FR2 (shares input for C) |
| FR4 Outdoor look | B, World | Painted sky, sun and hemisphere, sky PMREM environment, fog in the horizon colour, rider-following shadow box with texel snap, palette re-tune under Neutral tone mapping, contact AO pools. World mode only; scenarios keep the studio sweep. | M | Nothing |
| FR5 Ground | B, World | Per-cell mottle, the procedural detail array aligned with physics joints and setts, footprint-aware joints, then road marking ribbons. | M | FR4 |
| FR6 HUD and audio | A, Free Ride | HUD grid; battery, PWM and temperature gauges; firmware warning pills; buzzer-synced indicator; synthesized wind and tyre; optional motor sound; mix and ducking; settings disclosure. | M | FR2 |
| FR7 Buildings and skyline | B, World | Building kinds, roofs, storey bands and the facade atlas (render payload fields only); ring 2; distant silhouettes; fog pushed out to match. | L | FR4 |
| FR8 Vegetation and props | B, World | Lobed broadleaf crowns, conifers, shrubs (visual) with LOD; instanced grass with fade and sway; better furniture; new solid kinds in `euc-world` where a wheel can hit them; optional birds. | M to L | FR5 |
| FR9 Rider and effects | A, Free Ride | Lofted parts per MuJoCo body, generic kit, cosmetic head and hands, "Physics bodies" toggle (built, then removed on 2026-10-23); spin cue; dust; sparks and spray once FR2b gives the signals. | L | FR3; sparks wait for FR2b |
| FR10 Ambient life | B, World, with the physics | Parked vehicles as solids; pedestrians and traffic under the rule Tarik chooses. | L | Tarik's answer, FR8 |

Shared files: World owns lighting, background, fog and camera range in `Scene.tsx` (through a
new outdoor-environment component mounted there); Free Ride owns `CameraRig.tsx`,
`cameraTracking.ts` and the camera's field of view. Neither edits the other's files without a
message first.

## Decisions (2026-10-22)

Claude decided the open questions below on Tarik's behalf; Tarik can overrule any of them.

1. Neutral tone mapping stays; the sky is tuned for it (FR4).
2. Manual steering limits may rise toward real riding, as manual-only parameters, as long as every
   scripted scenario keeps its verdicts (FR2b).
3. The rider turns at a stop, through the physics.
4. R restarts the run where the wheel is, with a truthful firmware reboot (about 4 s) after a
   visible countdown that the rider can cancel. Nothing is teleported while a firmware runs.
   (Replaced by the play mode below: no countdown, about 0.7 s.)
5. Crouch now; a hop study after crouch works.
6. People and traffic wait for FR10.
7. A generic fictional rider kit for now.
8. A motor sound from the real electrical frequency and PWM, off by default (FR6). Changed on
   2026-10-23 to on, low, then back the same day: the whole ride sound (motor, tyre, wind) is off
   by default, an opt-in "Ride sound (experimental)" switch, because the owner judged the
   synthesized sound poor. The firmware's buzzer stays on.

Free Ride's settings preset (2026-10-23): "neutral", the factory record with level pedals (pedal
trim 4), instead of "blank", the factory-fresh record whose pedals sit 4.8° nose down (the wheel
rode 10° to 16° nose down at 17-21 km/h; on neutral the same ride sits 2° to 6° nose down).
"blank" stays available (`freeRideApi.start`).
9. Free Ride keys: R restart here, Space crouch (hop later), X step off, Esc pause. Ride's manual
   mode keeps its keys; FR2 only adds the gamepad's triggers and D-pad there.
10. km/h by default with a mph setting; a fixed afternoon sun now, a time of day later.

Later the same day Tarik (Human) asked to copy the game's body and navigation ("theirs are spot
on"). This replaces decision 7: the rider is EUC Thrills' fictional Cool Rider, with its MIT and
CC BY 4.0 notices, driven by the physics, with a Physics bodies toggle. It goes ahead of the other
phases. (Replaced on 2026-10-23 by the street kit, the owner's outfit, and no toggle: see Rider
body.)

Then Tarik asked for a play mode: "freeride experience is really poor. I have to click many
things, play, etc to test the wheel. And if i crash, it halts mid crash and i have to re-do
everything. Let the wheel crash and have a hotkey/button for quick respawn. Allow me to interact
with the wheel buttons, turn it off, click handle, whatever. Set me free to test and play. I wont
need the test pad anymore, nor any fine tuning for free ride simulations." This replaces
decision 4 and the arm step of FR2:

- Opening Free Ride starts a city ride and plays; the keys ride once the firmware is up (about
  1.5 s of simulated time). No arm step, no neutral dwell: input older than 250 ms is neutral.
- A crash never pauses: firmware, motor and world go on, and the timeline says "Stepped off" or
  "Rider detached".
- R respawns at once at the last clear spot (Shift+R at the world's start), a new run whose
  firmware boots again in about 0.7 s of wall time. The HUD has a button for each.
- P and H hold the wheel's power and handle buttons; a long press on power switches the wheel
  off, and the run goes on. L, B and M send the light, horn and pedal-mode commands.
- The test pad, its editor, the world's density sliders and every tuning control are gone from
  the Free Ride panel; old test-pad recordings still open.

The engine side is in `free_ride.rs` (committed as "free ride: play mode in the engine"); the
panel, HUD, keys and pad are in `app/src/workspaces/freeRide`.

## Open questions for Tarik (answered above for now)

1. **Tone mapping.** Keep Neutral (the wheel matches your photos) and tune the sky for it? Claude
   recommends yes.
2. **Steering limits.** May the physics raise the manual turn limits toward real riding
   (sharp turns at low speed, wider at speed), validated against firmware scenarios?
3. **Turning on the spot.** Should the rider twist the wheel when stopped?
4. **Quick reset.** Restart at the current spot (firmware boots again, about 4 s) or re-pose the
   physics with the firmware kept running? (Answered by the play mode: a new run at the last
   clear spot, about 0.7 s.)
5. **Crouch and hop.** Crouch can use the existing physics now. Do you want a hop study (it needs
   the welded-feet rider to lift the wheel with its legs)?
6. **People and traffic.** Visual-only life only beyond 12 m that fades as you near it, or wait for
   physical actors?
7. **Rider look.** A generic fictional kit, or your own gear from photos?
8. **Motor sound.** A synthesized motor from the real electrical frequency and PWM, off by default?
9. **Keys.** Space pauses or steps off today; EUC Thrills uses it for hop. R restarts the studio
   run today; EUC Thrills uses it for quick reset. Which meanings do you want in Free Ride?
10. **Units and time of day.** km/h only, or a mph option? One fixed afternoon sun, or a time-of-day
    slider?

## References

EUC Thrills files Claude read most (paths in the repository at the commit above):

- World look: `src/render/sky.ts`, `src/render/skyImage.ts`, `src/render/Renderer.ts`,
  `src/render/terrain.ts`, `src/render/groundNoise.ts`, `src/render/sharedGroundSurface.ts`,
  `src/render/markings.ts`, `src/render/facadeAtlas.ts`, `src/render/props.ts`,
  `src/render/environmentVegetation.ts`, `src/render/foliageKit.ts`,
  `src/render/ultra/groundContact.ts`, `src/app/renderTier.ts`, `src/data/surfaces.ts`,
  `src/data/props.ts`, `src/data/buildingLooks.ts`.
- World layout: `src/level/cityRing.ts`, `src/level/generateRoute.ts`, `src/level/sliceLevel.ts`,
  `src/level/townDressing.ts`, `src/level/parkDressing.ts`, `src/level/populationPlan.ts`.
- Camera and controls: `src/render/chaseCamera.ts`, `src/input/actions.ts`,
  `src/input/gamepad.ts`, `src/input/keyboard.ts`, `src/input/bindings.ts`,
  `src/simulation/EucController.ts`, `src/simulation/riderLean.ts`, `src/shared/maths.ts`,
  `src/data/tuning.ts`.
- Rider and effects: `src/render/rider.ts`, `src/render/ridingRig.ts`,
  `src/render/blockoutKit.ts`, `src/render/coolRiderLook.ts`, `src/render/euc.ts`,
  `src/render/particles.ts`.
- HUD, audio, settings: `src/ui/hud.ts`, `src/ui/hudModel.ts`, `src/ui/menus.ts`,
  `src/app/options.ts`, `src/audio/director.ts`, `src/audio/sink.ts`, `src/audio/mix.ts`,
  `src/shared/overspeed.ts`.
- Licence: `LICENSE`, `NOTICE.md`.

Ours: `docs/models/world.md`, `docs/models/body.md`, `app/DESIGN.md`,
`app/src/workspaces/freeRide/README.md`.
