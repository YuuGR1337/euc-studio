# Safety

An electric unicycle has no fallback: if the controller firmware stops balancing, the rider falls, often at speed.
The controllers this project targets run on 100-170 V packs and drive hundreds of amps. A firmware bug can injure the
rider, destroy the power stage or start a battery fire. Read this before you build, flash or ride anything from this
repository.

## Rules

1. **The simulator first.** Every firmware change must pass the scenario suite in the simulator, including the
   lock-step comparison against the stock firmware where it applies, before it touches hardware.
2. **The bench second.** Run new firmware on a controller on the bench, with the wheel off the ground and a
   current-limited supply, before any ride.
3. **Keep a way back.** Before the first flash of a wheel, make a full SWD backup of its flash, bootloader included.
   The vendor bootloader is not public; if an update goes wrong, SWD may be the only recovery.
4. **First flash = stock.** The flasher requires the first flash on any wheel to be the official stock image. That
   proves the update path without any firmware risk.
5. **Never ride an untested build.** Development builds are marked as such in their info block and the flasher warns
   about them. Protective gear, an empty area, low speed, and someone nearby.
6. **Do not change the drive mode or dead time** without knowing the gate driver of your board. The stock firmware has
   a hidden command that switches the motor drive mode; the flasher and the app never send it.
7. **Service commands need a hand on the wheel.** The wheel's BLE link has no pairing: any phone in range can send
   the service sequences (firmware update, IMU calibration, drive-mode toggle), and the stock firmware stops the
   motor at once, even under a rider. Our clean firmware runs them only when nobody rides (the wheel is off, on
   its side, or cut with the handle button) and the power button is pressed within 10 s. Flashing a wheel that
   runs our clean firmware therefore needs that press. The simulator shows both sides
   (`scenarios/ble-service-cut-standing.toml`): with a rider standing still, a phone sends "c" then "y"; the stock
   firmware cuts the motor 10 ms later and wheel and rider fall, while the clean firmware answers `SERVICE:DENIED`
   and keeps balancing ([docs/findings/stock-firmware.md](docs/findings/stock-firmware.md)).

## Flasher rules

The OTA flasher ([docs/spec/flasher.md](docs/spec/flasher.md)) keeps these rules in the simulator and on any real link.

1. **No real flash from this build.** `euc flash` flashes only the simulated wheel. The BLE transport is behind the
   `central` feature of `euc-ble` and the `ble` feature of `euc-flasher`. No default build and no shipped binary
   enables them.
2. **Listen-only by default.** Even with those features, every write to a real wheel is refused unless
   `EUC_BLE_REAL_WRITES` is `swd-backup-done:` followed by that wheel's BLE identifier. Set it only when a full SWD
   backup of that wheel exists and its owner approved that run. The maintainer's wheel gets no real flash before its
   SWD backup, and none without the owner's explicit go-ahead for that run.
3. **Every gate, every time.** Image, provenance, simulator dry-run, recovery image, first flash = stock, identity,
   wheel state (still for 5 s, no alarms, at least 118 V, board below 60 °C, a stable link), the typed confirmation,
   and the wheel state again. A failed gate stops the flash. Today gates 2 and 3 refuse every real flash: our images
   have no signature yet, and the 30 s emulator ride of gate 3 does not exist yet.
4. **The guard judges every byte.** Never `+` `-` `c` `y`. `!` `@` only once armed, while the application's frames
   prove it runs, with the required gap. YMODEM only to a bootloader that proved itself since the link came up, and
   only as whole, valid packets.
5. **Stock first, stock to recover.** The first flash of a wheel is its pinned stock image. A failure after the
   bootloader request restores the stock image; the flasher never tries other image variants. After two failed
   restores it stops and points to SWD.
6. **Keep the log.** A real flash writes every byte, gate and step to a JSONL log.
7. **The model is not the wheel.** The emulated bootloader is an assumption. The fault campaign proves the flasher's
   logic against that model, not against the vendor bootloader.

## Reporting a safety problem

If you find a behavior in this project's firmware, simulator or flasher that could hurt a rider, report it privately
through GitHub's "Report a vulnerability" (Security tab) instead of a public issue. Include the scenario or steps,
the firmware build and what you expected.

## No warranty

This project is provided without any warranty. You alone are responsible for what you flash and ride. See LICENSE.
