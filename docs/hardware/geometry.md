# Body geometry (Begode Extreme)

`profiles/begode-extreme/geometry.toml` holds the size and layout of the wheel's body: tire, rim,
shell, seat, handle, display window, lights, pedal mounts and the collision shapes proposed for the
physics model. The numbers come from a 3D scan of a real Extreme, made by Tarik Caramanico. The raw
scan and the intermediate files stay in the private research folder; the cleaned render model is
part of this repo (`app/public/models/begode-extreme/`, see "The render model" below).

Evidence tags in the TOML: **MEASURED** (fitted or read on the scan), **INFERRED** (measurements plus
one stated assumption), **ESTIMATE** (the part is not in the scan), **DEFAULT** (kept from
`plant.toml` or the MJCF).

## The scan

- Binary STL from a GOM structured-light scanner, 1,610,052 triangles, millimetres, "GOM aligned".
  Vertex spacing is about 1 mm; flat panels are meshed coarser (edges up to 20 mm).
- **No colour.** The header carries `COLOR=` with RGBA 255,255,255,255 (the Materialise default
  colour) and every face has the attribute value `0x8000`: bit 15 set, RGB bits zero. In the
  Materialise convention bit 15 means "use the default colour", in the VisCAM convention it means
  "this face is black". Either way all faces have the same colour, so there is nothing to keep.
- The wheel stood on its tire; the scan stops 89 mm above the floor, so the contact patch is
  missing. The pedals were off the wheel (only their hinge brackets are there).
- After welding, the mesh splits into 198 connected pieces. The largest is the body (frame, packs,
  seat, handle, lights); the tire comes out as two halves and the motor's side covers and a rim
  flange as separate pieces. That split is what makes the wheel easy to isolate.

## The frame

Same as the MJCF: x forward, y left, z up, metres, origin on the axle centre in the wheel's
mid-plane.

1. **Rough axle.** A circle fit to the tread's outer envelope (the five farthest points in each
   1-degree bin around a guessed centre) gives the axle to within a few millimetres.
2. **Wheel faces.** Faces outside the body piece, within 46 mm of the wheel plane and inside the
   tread radius.
3. **Axle line.** On a surface of revolution every normal line crosses the axis. The axis (point and
   direction) minimises the sum of `((p - c) . (a x n))^2` over the motor covers and rim faces (radius
   under 147 mm, 95,000 faces, soft-L1 loss). Median residual 0.42 mm. Fitting the tire sidewalls
   instead moves the centre by about 2 mm (the tire is not a perfect solid of revolution), so the
   rigid motor is the reference.
4. **Mid-plane.** Halfway between the outer faces of the two sidewalls at their widest (they sit at
   +-40.7 mm from it).
5. **Up.** The normal of the shell's flat bottom face (1,800 faces, plane fit RMS 0.03 mm). The
   scanner's alignment already had z normal to it to 0.007 degrees.
6. **Forward.** The end with the four-lens headlight, the carry handle and the display window; the
   shock and its spring sit at the other end. The display window's orientation agrees: the display
   doc records the top of the digits pointing forward.

The axle ends up 0.13 degrees yawed and 0.25 degrees rolled against the shell, which is play in the
slider and bearings of the scanned wheel, not something to model. The tread's knob tips lie at
229.0-230.4 mm from this axle at every angle the scan covers, which checks the centre.

## Measurements and methods

| Quantity | Value | Method | Confidence |
|---|---|---|---|
| Tire size | 3.00-12, code J802-02 | moulded text on the sidewall, read on a height map of the unwrapped sidewall; the owner's photo confirms it | high |
| Tread | 15 repeats of 24 degrees, staggered rectangular knobs | autocorrelation of the crown band (0.99 at 15) | high |
| Tire outer diameter | 459.3 mm | median of the knob-tip radius over 51 bins of 4 degrees, about the fitted axle | +-1 mm, unloaded |
| Tread base radius | 225.0 mm | lower peak of the radius histogram in the tread centre band | +-0.5 mm |
| Tire width | 81.4 mm | sidewall extremes at radius 170-205 mm | +-0.5 mm |
| Crown curvature | 58 mm | circle fit to the tread envelope in the axial-radial plane | +-3 mm |
| Rim | bead 304.8 mm (12 in), flange 320 mm, flanges 61.4 mm apart | sidewall end and flange faces in axial-radial sections | +-3 mm |
| Motor width | 80.5 mm | the two side-cover planes | +-0.5 mm |
| Axle height | 229.6 mm unloaded | equals the free tire radius (no floor in the scan) | +-1 mm |
| Packs | x +-124.3, y 44.3-101.3 (left) and 45.8-102.3 (right), z -29.8 to 370.3 mm | area-weighted histograms of axis-aligned faces | +-0.5 mm |
| Lowest body point | z -51.8 mm | axle block under the slider channel | +-0.5 mm |
| Seat top | z 469.5 mm, 699 mm above the ground | 99.5th percentile of the seat cover on the centre line | +-1 mm |
| Handle tube | centre x 132.3, z 473.8 mm, diameter 20.0 mm, 138 mm long | circle fit in the x-z plane (median residual 0.02 mm) | +-0.5 mm |
| Display window | flat at z 438.3 mm, x 41-127, y -54 to 52 mm, tilt 0.15 degrees | plane fit to the flat faces in front of the seat | window +-1 mm, board place +-10 mm |
| Headlight | 4 lenses of 22 mm at y -45.7, -15.7, 14.3, 44.3 mm, z 390.5, face at x 175.1 | 2 mm lens recesses in the bar's front face | +-0.5 mm |
| Pedal pivots | pin along x at y +-114.8, z +7.5 mm, 6 mm diameter | Hough vote of normal lines in the hinge brackets | +-1 mm |
| Pedal deck | 290 x 135 x 9 mm, top at z +12 mm (242 mm above the ground) | not in the scan; see "Pedals" below | estimate: length +-30, width +-15, top +-5 mm |
| Rear roller | tube along y at x -186.8, z 449.1 mm, radius 26.1 mm; end collars from y +-37.5 mm; speaker grille faces at y +-50.5 mm (annulus r 12-22 mm) | circle fit (0.67 mm); radius profile along y; recessed end faces with radial windows | +-1 mm |
| Overall | 479 x 270 x 715 mm (length x width x height above the ground) | body bounding box plus the tread | +-1 mm |

What the scan cannot give:

- **Pedal decks.** Estimated from photos (below). One measured pedal replaces the estimate.
- **Inside the rear roller.** The owner confirms what the tube holds: the controller's Bluetooth data
  board (a blue status LED at the top of the centre band), the two RGB tail-light LED rings (the
  translucent halves glow orange-red on each side of a black centre band) and a speaker in each end
  cap, driven by a separate Bluetooth audio device. The scan shows the tube, the collars and the
  grilles, but no seam for the band: its width (15.6 mm) and the LED spot come from the owner's video
  (`lights.toml`).
- **IMU.** The controller board is inside the body. The findings default (0.15 m above the axle)
  stays. If the board sits in the top box under the seat (z 0.37-0.44 m) the IMU is about 0.4 m above
  the axle, which changes the tangential accelerations it feels; a photo of the open wheel settles it.
- **Display board.** The window is measured; the board is assumed centred under it.

## Suspension

On each side the axle sits in a carriage that slides on a vertical rod inside the channel between the
fore and aft packs. The rods are 30.0 mm (cylinder fit, spread 0.1 mm) at x -0.2, y +63.6 and -64.9 mm,
vertical within 0.15 degrees (top 0.13 degrees rearward). The carriage block's top face is at z
251 mm; the rod is free above it for 108.5 mm up to a ring on the rod (lower face at 359.5 mm, likely
a bump stop or boot) and 132 mm up to the frame's top plate (383 mm). Below the axle a frame block
holds the rods (z -52 to -20 mm). The scanned wheel stood unloaded, so the carriage is taken to be at
the bottom of its travel (topped out); the shock's stroke, not the rod, limits the real travel.

What moves with the axle (INFERRED from the layout): the carriage, the axle, the whole motor, rim and
tire, and the shock's bottom eye (below). The pedal hinge brackets sit outside the packs at the axle
height with their plate at the packs' bottom plane (z -29.8 mm), so they are taken to be on the frame
(sprung). The controller board is not visible in the scan.

### The scanned (older) linkage

The scan shows an older suspension system (the owner, 2026-10-20): a shock with a piggyback
reservoir on two 20 mm cross shafts, top (x -139.5, z 367.5 mm) and bottom (x -226.5, z 183.5 mm),
203.5 mm apart, on a sloped rear tray, with plates between the shafts. The owner's photos of
2026-10-22 show none of these parts: where the scan has the rear tray and the bottom shaft, the photo
shows the floor. `geometry.toml` keeps the old positions as `scan_linkage_pivots_xz_m` for the record;
the render model no longer draws them. Only the slider rods, the carriage and the frame stay valid
from the scan.

### Rear linkage (current, from the owner's photos)

The owner cannot measure inside the linkage. The layout comes from the owner's photos: the wheel
lying on its side (2026-10-16), the rear with the side pad cover removed (2026-10-22) and the rear
with the seat removed (2026-10-22). Positions are measured by solving each photo's camera pose from
points that are in the scan (pack rivets, frame bolts, the rear handle's roller bolt): the side view
with the cover off fits 11 scan points to 8 px RMS, about 1 mm at the linkage. The derivation lives
in the private research folder (`research/linkage/`).

- **The rocker (MEASURED shape).** One black, textured, laser-cut steel plate on each side, outside
  the shock, with one large window and three holes in view: the shock's top eye on a cross bar (the
  red rebound knob beside it), an empty hole 20 mm from it, up and forward (likely a second shock
  position), and a lower cross bar 66 mm below the top eye. The plate continues forward under the rear packs' edge,
  where no photo reaches it.
- **The shock (MEASURED line).** It runs from its top eye, under the rear roller, forward and down
  toward the carriage: a decal on the shock body places the line 44-52 degrees forward of vertical.
  So its bottom eye, 200 mm away, sits on the carriage just behind the slider channel, above the
  tire (INFERRED, +-10 mm), hidden behind the rear packs in every photo.
- **The link (INFERRED).** Plates leave the lower cross bar forward and down, along the shock,
  toward the bottom eye's shaft; past the bar they end in a pointed tip with a small hole. So the
  lower cross bar is the link-to-rocker pin, and the link joins it to the carriage.
- **The rocker's frame pivot (not located).** It is hidden in every photo. With the seat off, a
  steel pin crosses between two frame blocks at the front end of the plates, at about (x -45 +-20,
  z 400 +-15 mm). As the pivot of this rocker it would lock the linkage: every geometry with the
  pivot there runs within 25 degrees of a straight link-to-rocker line (a toggle) before the shock
  is compressed. So its role is open (a stop, a mount, or the pivot of another part). A pivot works
  from about x -80 mm rearward; the render model draws it at (x -100, z 395 mm), the sound point
  nearest that pin (ESTIMATE).
- **The parts photo** (a product picture of a three-hole rocker and a two-hole link) shows a
  smooth CNC part with three windows; the owner's plates are not that part, so its sizes are not
  used.

| Point (full extension) | x mm | z mm | Evidence |
| --- | --- | --- | --- |
| Shock top eye (on the rocker) | -185 | 378 | MEASURED (photo pose, +-3) |
| Rocker pin (link upper hole) | -177 | 313 | MEASURED (photo pose, +-3) |
| Empty hole beside the top eye | -172.5 | 393 | MEASURED (photo pose) |
| Shock bottom eye and link lower hole, on the carriage | -34 | 247 | INFERRED (shock line, 200 mm), +-10 |
| Steel pin between two frame blocks | -45 | 400 | MEASURED roughly (seat-off photos), +-20 / +-15 |
| Rocker frame pivot | -100 | 395 | ESTIMATE (drawing only), band below |

**The leverage (ESTIMATE, not used by the physics yet).** A Monte Carlo over the measured points
(+-2 mm), the shock line (44-52 degrees), the link's direction (18-32 degrees below the rocker pin,
from the plates) and its lower pin on the carriage, with the frame pivot anywhere the photos leave
room for it (between the rear packs, under the frame top), keeps 24,000 of 160,000 sampled geometries
(link and rocker at least 20 degrees from a straight line, the shock compressed monotonically). Their pivots
lie at x -128 to -81 mm, z 376 to 413 mm (5th to 95th percentile). They agree on three things:

- the ratio at rest is 2.1-2.7 (median 2.4) axle mm per shock mm, not the 1.6-1.9 that the owner's
  sag gives with the 0-2 mm of preload assumed so far. To match the owner's sag at that ratio, the
  spring needs about 6-11 mm of preload;
- the rods' stop (108.5 mm of axle travel above the carriage) ends the travel before the shock's
  stroke in 87 % of them: only 41-55 mm (median 47) of the 55 mm stroke is used;
- the ratio near the end of the travel rises (median 2.6 at 100 mm, band 1.6-3.6).

The physics keeps the constant `linkage_ratio` 1.7 of `plant.toml` until two inputs arrive: where
the rocker's front end is bolted (or whether the steel pin turns when the seat is pressed), and the
spring preload (or a sag reading on an O-ring on the shock shaft). With them the curve becomes a
measured `ratio_curve`.

**The drawn linkage.** The render model draws the measured points with the pivot ESTIMATE above.
Over the physics travel (93.5 mm with the constant 1.7) its shock shortens 38 mm where the physics'
shock travel is 55 mm: the drawing's shock length differs from the `shock_mm` channel by up to 17 mm
(2 mm at 10 mm of travel, 11 mm at 60 mm). The rocker turns 11 degrees at most. The studio poses the
drawing from `suspension_mm`, so all parts stay joined; the shock's length follows the drawing.

**Superseded.** The first estimate (2026-10-22, before the cover-off and seat-off photos) read the
2026-10-16 photos with the scan's old shafts as anchors: a shock leaning 23.5 degrees forward from
the old bottom shaft, a frame pivot forward of the top eye, and a rising curve from 1.75 at rest to
2.38 at 103.5 mm. The new photos contradict its positions (the top eye sits 40 mm further back and
the old bottom shaft is gone), so it is not used. Its scenario runs (no verdict changed against the
constant 1.7) say nothing about the real linkage. `docs/hardware/suspension-linkage.md` records the
readings considered and why each was kept or dropped.

## Pad covers

The owner's photos of 2026-10-22 show a removed side pad cover: a C-shaped part of textured black
molded plastic, with two arms joined by a spine, raised blocks at the arms' ends (one with the
maker's lettering, one with a round recess), diagonal grooves and four screw holes. The scan does
not contain it: the scanned packs show their bare outer faces with rivet rows. Its outline is traced
from the photo (the private pipeline's `pad_cover()`), but two things are open: its size (the screw
heads give 0.14-0.19 mm per photo pixel, so its length is 370-510 mm) and where it sits on the wheel
(the photo of it mounted shows only part of it). So the render model does not draw it yet.

Where the rider's calves would touch it is open for the same reason. The raised blocks are the
likely contact faces; once the cover is placed, `geometry.toml` gets their outer faces as contact
boxes for a leg-pad contact model.

## Rolling radius and the electrical model

`docs/models/electrical.md` takes the rolling radius from the firmware's 0.0505 m per electrical turn
and 26 pole pairs: 0.2090 m, a 16.5 inch tire. The scanned tire is a **3.00-12** with knob tips at
0.2296 m, 20.6 mm (9.9 %) larger. Only one whole pole-pair count fits it:

| Pole pairs | Rolling radius (p x 0.0505 / 2 pi) | Against the free radius 0.2296 m |
|---|---|---|
| 26 | 0.2090 m | 20.6 mm smaller: impossible for this tire |
| 28 | 0.2251 m | 4.5 mm smaller: the usual squash of a loaded knobby tire |
| 30 | 0.2411 m | larger than the free tire: impossible |

So the scan points to **28 pole pairs and a rolling radius of 0.2251 m** (`geometry.toml` records both
as INFERRED). The firmware does not care: its speed and odometer come from electrical turns times
0.0505 m, and the force at the ground per amp, 1.5 psi I / (0.0505 / 2 pi), does not depend on the
pole pairs either, so the motor flux from the no-load speed stays the same. What changes is the
mechanics: the wheel's size, its spin speed for a given road speed (7 % lower), the torque per amp
(8 % higher) and the contact geometry. The "16 x 3 inch (published)" note in the electrical model
conflicts with the tire on the scanned wheel; the owner's photo confirms the 3.00-12 tire.

## Pedals

**ESTIMATE, to be updated from the owner's measurements** (pedal height, size and position; the owner
also says the drawn design does not match his pedals). The CNC decks were off the wheel for the scan. The model draws our own deck in their style: a black
anodised aluminium plate with a D-shaped outer edge and a bright chamfer, hexagonal through-cut-outs in
the front and rear thirds, a plain solid band in the middle, gold grip pins on the cell junctions
(26 per deck) and a polished hinge pin. Left and right are mirror images. The sizes are estimates:

- **Hinge.** Measured: the 6 mm pin runs fore-aft at y +-114.8 mm, z +7.5 mm. The deck's notch must
  clear the scanned bracket (46 mm long, its outer face 20.3 mm past the pin), so the notch is 50 mm
  long and 21 mm deep. The plate (9 mm) is centred on the pin, so its top is at z +12 mm.
- **Length and width.** Shop photos give the proportions: the hinge notch is 0.16 of the deck length
  and the width (hinge edge to outer edge) is 0.47 of it. A 50 mm notch makes the deck about 300 mm
  long; with a little margin around the bracket, 290 x 135 mm. Uncertainty: +-30 mm length, +-15 mm
  width, +-5 mm deck height. One tape measure on a real pedal settles it.
- **Fold.** Each deck is its own node with its origin on the pin, so the app folds it about x: the
  left deck up with +90 degrees, the right with -90 degrees (glTF axes).

The collision boxes `pedal_left` and `pedal_right` follow these sizes: 290 x 135 x 9 mm, centred on
the pins at y +-176.3 mm.

## Collision shapes

`[collision]` proposes primitives in the MJCF frame:

- **Tire.** A torus with a 58 mm tube at 171.6 mm from the axle has the right outer radius and the
  real crown curvature; it stays within 1.5 mm of the tread envelope up to +-37 mm off the centre
  plane, about 40 degrees of lean. MuJoCo has no torus geom: a ring of 48 capsules (0.37 mm ripple per
  segment) or the `mujoco.sdf.torus` plugin. The current ellipsoid has the right outer radius but a
  7 mm crown radius instead of 58 mm, which matters only once the model leans in turns. A second
  torus (40.7 mm tube) keeps the true width with a sharper crown.
- **Packs.** One box per side over both packs and the slider channel, bottom at the lip (-41.5 mm).
- **Top and front.** A box over the seat and the top frame; a box over the handle, headlight and
  display chamfer.
- **Rear roller and shock.** Capsules along their axes.
- **Pedals.** The bracket boxes are measured; the decks use the estimated sizes above.

## The render model

`app/public/models/begode-extreme/` holds three levels of detail (LOD0 294k triangles 2.5 MB, LOD1
114k 1.1 MB, LOD2 41k 0.37 MB, meshopt compressed). Its README lists the nodes. How it is made from
the scan (the tools live in the private research folder):

1. **Parts.** Simple rules split the scan: tire (wheel pieces that reach the tread), hub, seat
   (above 405 mm between the rear roller and the handle), display window (the flat faces at 438 mm),
   handle, headlight, shock, pedal mounts and shell. Pieces under 25 faces are dropped.
2. **Body clean-up.** Duplicate and null faces, non-manifold edges and holes up to 120 border edges;
   feature-preserving smoothing (normals, then vertices). On the hard parts (shell, handle, headlight,
   pedal mounts) 63 planes are found from the face normals and offsets, and every face within 8 degrees
   and 0.7 mm of one goes onto it, so panels are flat and their edges straight (moves under 0.7 mm).
   The seat and shock stay curved. The handle tube is redrawn on its fitted cylinder.
3. **Tire.** Rebuilt as a closed ring: the scanned tire becomes a height field around a section centre
   at 180 mm, its tread repeat (15 x 24 degrees) is found by autocorrelation, all scanned repeats are
   merged by median into one clean pitch (0.15 mm from the scan) and tiled round the whole wheel. This
   fills the part under the floor and inside the shell and removes noise. The sidewalls are made
   smooth surfaces of revolution (the size text goes).
4. **Hub and rim.** Modelled from the measured profile: matte black motor covers with a ring of socket
   screws, and the rim flange as an orange ring (the owner's photo). The scan had holes there.
5. **Modelled parts.** Display window (flat, with UVs for the T16-007 panel), headlight lenses, axle
   stub, pedal decks (above), and the rear roller: its tube is redrawn as two smoked grey sleeves
   around a black centre band with the BLE status LED; the scanned end caps become the two speakers.
6. **Packs, shock and rear handle.** The four battery packs are modelled as boxes with 4 mm rounded
   edges and 8 mm chamfers on the faces measured above, with the rivet rows read on a height map of
   the side (four groups of five near each outer end). The rear shock is a DNM coil-over without
   reservoir: **200 mm eye to eye and 750 lb/in, MEASURED by the owner**; a 56 mm x 115 mm spring
   (DNM size chart for 200 mm), 8 mm x 24 mm eyelets, threaded body with a slotted collar at the
   bottom eye, red rebound knob at the top eye; stroke ESTIMATE (55 mm). The shock and the **rear
   linkage** are drawn to the owner's photos of 2026-10-22 (see "Rear linkage" above): per side a
   rocker plate with the photographed outline, window and empty hole, plus its hidden forward arm
   to the pivot ESTIMATE; a link (a bar with the pointed tip past the rocker pin) to the carriage;
   cross bars at the top eye and the rocker pin (black tubes with spacers either side of the shock
   eye, chrome button heads with washers); the plates sit at |y| 46.5-51 mm (the rear photo's camera
   pose fits 50-54 mm for their outer faces); the carriage's clevis and bottom shaft (hidden in
   every photo, ESTIMATE); the pivot shaft; and the steel pin between two frame blocks at the front.
   Nothing of the scan's older system is drawn: its shafts, plates and rear tray are removed. Each
   moving part is its own node (README), and the studio poses them from the Suspension channel.
   The **rear handle** is the orange CNC U-handle of the photos: each arm runs from a foot at the
   frame top (x -113 mm) up through the roller axis to the grip bolt, with three through-windows on
   the lower part and a blind pocket on the upper part; the windows, the foot and the grip (63.5 mm
   past the roller axis) are MEASURED on the cover-off photo (camera pose from scan points), the
   outer face at |y| 58.7 mm on the scan, the black grip bar (26 mm) is an ESTIMATE. The physics'
   collision solid for the grip (`[collision] rear_handle_grip`) keeps the earlier estimate, about
   30 mm higher than the photo's grip; the rider rarely touches it. The scanned faces these models
   replace are removed; what remains of the scan there (brackets, a sloped base plate) is cleaned
   (loose flaps under 150 mm2 dropped, holes closed, smoothed). The roller is cut out of the seat
   before decimation, and its end caps (speakers) are modelled from their measured profile.
7. **No trademarks.** The brand emblem pressed into the seat top is flattened (a quadratic surface
   fitted to the plain ring around it), the pedal band stays plain, and the shock carries no brand
   marks.
8. **Levels of detail.** Hard parts: vertex clustering, smoothing, quadric collapse, then back onto
   the planes. Curved parts (seat, scanned brackets): a 1 mm clustering pass, smoothing, quadric
   collapse with optimal placement, smoothing again. Quadric collapse alone on a raw scan with
   100,000 open border edges builds slivers hundreds of mm long. Loose scanned shards (under 60, 150
   and 400 mm2 for LOD0, LOD1, LOD2) are dropped.
9. **Looks.** The scan has no colour. Materials follow the owner's photos: textured dark-grey shell
   (a tileable grain normal map on LOD0 and LOD1), black rubber tire, matte black motor covers, orange
   flange ring, textured matte black linkage plates, smoked grey tail-light sleeves (they glow with
   the LEDs in the studio), satin aluminium handle tube, black anodised pedals with gold pins,
   smoked display window. Ambient occlusion is baked into the vertex colours. The anodised orange
   is set from the owner's photo (mean sRGB 184, 91, 47 on the handle). Under the studio's lights
   (neutral tone mapping, neutral rim light) the handle, the chrome bolt heads and the shell render
   within 26, 51 and 13 sRGB units of the photo's mean colours in the same view; with the earlier
   ACES tone mapping and warm lights they were 91, 206 and 43 units off (the handle red, the chrome
   copper, the shell brown).

## Open

What the owner can measure to close the remaining gaps (centre to centre, in mm):

- **Rocker linkage.** Where the rocker plate's front end is bolted to the frame: with the seat off,
  whether the steel pin at the front turns when the seat is pressed down (then it is the pivot) or
  the plates slide past it; a short video while pressing the seat answers it and shows the ratio.
  And the spring preload: how far the collar is wound down from the spring's free length, or the
  sag read on an O-ring on the shock shaft. With both, the leverage becomes a measured curve.
- **Pad covers.** The length of a removed cover with a tape, and one photo of it mounted, from the
  side.
- **Shock stroke.** Printed on the shock or in its listing (55 mm is an ESTIMATE, 50-57 mm range).
- **Pedals.** Width at the front, middle and back, and the thickness. A tape photo reads the length
  at about 300 mm (model: 290 mm ESTIMATE); the outline and pin layout are not yet taken from it.
- **Rear handle.** Distance from the roller axis to the grip centre.
- **Controller.** Where the controller board sits (for the IMU height above the axle).
