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

**C. Photo-measured (current).** The rocker plate carries the top eye and the lower bar (the rocker
pin); the shock runs forward and down to its bottom eye on the carriage, near (-34, 247); the link
joins the rocker pin to the same carriage shaft. The rocker's frame pivot is hidden in every photo.

- The steel pin seen with the seat off cannot be that pivot: with the pivot there, every geometry
  runs into a toggle (link and rocker within 25 degrees of a straight line) before the shock is
  compressed. Its role is open.
- With the pivot anywhere the photos leave room for it (between the rear packs, under the frame
  top: x -128 to -81, z 376 to 413), the ratio at rest is 2.1-2.7, the rods' stop ends the travel
  before the full stroke (only 41-55 mm of the 55 mm stroke is used), and the ratio rises near the
  end. Matching the owner's sag at that ratio needs about 6-11 mm of spring preload.
- So the physics keeps the constant 1.7 of `plant.toml` until the pivot and the preload are known.
  The render model draws reading C with the pivot at (-100, 395), the sound point nearest the steel
  pin (ESTIMATE).

## Ruled out in every reading

- **The shock's bottom eye on the frame, the rocker driven by the carriage.** No pivot position lets
  the carriage compress the shock within the rods' travel: the shock extends or the linkage locks.

## What would settle it

- With the seat off: does the steel pin at the front turn when the seat is pressed down (it is the
  pivot), or do the plates slide past it? A short video while pressing the seat would answer this and
  show the ratio (axle rise against shock shortening).
- The spring preload: how far the collar is wound down from the spring's free length, or the sag read
  on an O-ring on the shock shaft.
