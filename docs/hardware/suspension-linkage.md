# Rear suspension linkage: readings considered

The owner cannot put a ruler on the rear linkage, and the 3D scan shows an older linkage and shock.
So the layout comes from the owner's photos and the scan's frame. `docs/hardware/geometry.md` ("Rear
linkage") has the current result and its numbers, and `profiles/begode-extreme/geometry.toml`
`[suspension]` the positions. This page records the readings that were considered, in order, and
why each was kept or dropped. Frame as everywhere in this repo: x forward, z up, origin on the axle
centre, suspension at full extension; millimetres on this page.

## Evidence, in the order it arrived

1. **2026-10-16 photos** (the wheel lying on its side, seen from behind and from below; a top view of
   the rear; a product picture of a three-hole rocker and a two-hole link). They show the topology:
   a rocker with the shock's top eye, a lower cross bar, plates with a pointed tip and a small hole.
   They do not show where the parts sit in the wheel.
2. **The scan** (an older system): two cross shafts 203.5 mm apart on a sloped rear tray. The first
   readings used these shafts as anchors.
3. **2026-10-22 photos, side pad cover removed.** A straight side view, with pack rivets, frame bolts
   and the rear handle in the frame, so the camera pose can be solved from scan points (11 points,
   8 px RMS, about 1 mm at the linkage). They measure the top eye (-185, 378), the lower cross bar
   (-177, 313), an empty hole beside the top eye (-172.5, 393), and the shock's line: forward and
   down, 44-52 degrees from vertical. The scan's rear tray and bottom shaft are gone: the floor shows
   there.
4. **2026-10-22 photos, seat removed.** Top and rear views: the top eye sits under the rear roller;
   at the front end of the plates a steel pin crosses between two frame blocks, at about (-45 +-20,
   400 +-15). The spring's preload collar shows 5-6 threads, which does not give the preload without
   the collar's zero position.
5. **A retailer's parts diagram** (not in the repo). Each part is photographed at its own scale, so
   only sizes within one part are used, each scaled by a measured size of that part. The rocker
   plate has four holes and three windows; three of the holes fit the photo-measured top eye, empty
   hole and lower bar (scale error 2 %, residuals under 0.8 mm), which places the fourth, the frame
   pivot, at (-66.6, 398.1) +-3: the seat-off steel pin. The link is a straight bar, 156 mm hole to
   hole at the rocker plate's scale, with the pointed tip and small hole of the photos.
6. **The link's direction, back to the cover-off photo.** With the solved camera, the link's two
   edges, back-projected onto its plane, run 16 +-2 degrees below horizontal forward from the lower
   bar. With the diagram's 156 mm, its lower pin is at about (-27, 270) +-8.

## Readings

**A. Shock on the scan's shafts, link up to the lower bar (first nominal, superseded).** The shock
stood on the old bottom shaft (-226.5, 183.5), leaning 23.5 degrees forward; the link ran from that
shaft up to the lower bar behind the spring; the frame pivot sat forward of the top eye, placed by
the owner's sag. Curve: 1.75 at rest rising to 2.38 at 103.5 mm. Dropped: the cover-off photos put
the top eye 40 mm further back and show no shaft at the old bottom position.

**B. Lower bar as the frame pivot, link hanging from the rocker's front corner (alternative,
superseded).** Read from the product picture's proportions: the ratio falls from 2.0 at rest to 1.1,
77 mm of travel. Dropped: the cover-off photos show the lower bar on the rocker plate next to the
link's pointed tip, and the plates are not the product picture's part.

**C. Photo-measured, link on the shock's bottom shaft (superseded).** The rocker plate carries the
top eye and the lower bar (the rocker pin); the shock runs forward and down to its bottom eye on the
carriage, near (-34, 247); the link joins the rocker pin to the same carriage shaft. The frame pivot
was hidden: with it anywhere between the rear packs (x -128 to -81, z 376 to 413), the ratio at rest
was 2.1-2.7 and only 41-55 mm of the stroke was used. The steel pin seemed to lock the linkage as
its pivot, but that came from the pin's rough x (-45 +-20). Dropped: the parts diagram places the
pivot on the steel pin, and the photographed link points 9 degrees flatter than a link to the
shock's bottom shaft.

**D. Located, the rocker turns (reading (a), drawn by default).** Pivot on the steel pin (-66.6,
398.1), from the parts diagram; the shock's bottom eye on the carriage near (-34, 247) as in C; the
link from the lower bar to its own carriage pin near (-27, 270), 23 mm above the shock's bottom eye.

- The ratio falls through the travel: 2.34 at rest, 1.84 at 75 mm, 1.70 at 93.5 mm; the 55 mm stroke
  ends at 107.5 mm. Band over the points' uncertainties: 2.0-2.5 at rest, 1.3-2.1 at 93.5 mm (1.9-2.7
  and 1.0-2.4 over a coarser grid), the stroke ending at 89-121 mm.
- Matching the owner's sag needs about 4-9 mm of spring preload (6-11 mm in C).
- **Candidate physics if the video shows the rocker turning** (not applied):
  `ratio_curve = [[0, 2.34], [15, 2.23], [30, 2.13], [45, 2.03], [60, 1.94], [75, 1.84], [93.5,
  1.70], [107.5, 1.63]]` and `preload_mm = 7` (ESTIMATE, band 4-9), stroke 55 mm, so the axle
  travel becomes 107.5 mm (93.5 now). The owner then sags about 15 mm and the default 80 kg rider
  21 mm (19 % of the travel, the same share as today). Side effects: the wheel rate at rest falls
  from k/1.7^2 to k/2.34^2 (about 45 to 24 N/mm), the bounce from 3.4 to about 2.6 Hz, and the
  damping ratio from 0.18 to about 0.14 unless the damping rises to about 2800 N s/m. Before it
  goes in: the scenario matrix and the compat-vs-stock compare, together with the pedal-deck
  collision change (one physics update).

**E. Located, the rocker fixed (reading (b)).** The same points, but the link's lower pin is on the
frame: the pivot, the link and the frame form a rigid triangle, so the rocker is a fixed shock
bracket with two positions (the empty hole) and only the carriage moves the shock's bottom eye. The
ratio is the shock's angle alone and constant: 1.52 at the drawn 49 degrees (1.34-1.62 over the
photographed 42-52 degrees; 1.70 at 54 degrees). It matches the owner's sag with little preload
(14 mm at the axle with no preload at 1.52, 14 mm with 2 mm at 1.70), which is today's physics
(constant 1.7, 2 mm). At 1.52 the 55 mm stroke ends at 84 mm of axle travel.

The physics keeps the constant 1.7 of `plant.toml` until the video decides between D and E. The
render model draws D; `LINKAGE_READING` in `app/src/workspaces/ride/shockLinkage.ts` switches the
drawing to E (rocker, link and the link's lower pin still).

## Ruled out in every reading

- **The shock's bottom eye on the frame, the rocker driven by the carriage.** No pivot position lets
  the carriage compress the shock within the rods' travel: the shock extends or the linkage locks.
- **The ring on the slider rod as the bump stop.** It is a pipe O-ring (the parts diagram lists pipe
  O-rings with the slider pipes), a travel indicator; the frame's top plate (132 mm above the
  carriage) is the hard limit.

## What would settle it

- A short video from the side, pad cover off, while pressing the seat down: do the rocker plates
  turn about the steel pin, and which way? It separates D from E and shows the ratio (axle rise
  against shock shortening).
- The sag: push a slider pipe's O-ring down to its seal, stand on the wheel, step off, and measure
  how far it rose. Or the spring preload: how far the collar is wound down from the spring's free
  length.
