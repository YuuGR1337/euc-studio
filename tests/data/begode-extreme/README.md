# Begode Extreme test data

## `idle-2026-10-15.hex`

Telemetry from a real Begode Extreme (the maintainer's own wheel), recorded on 2026-10-15 with a macOS CoreBluetooth
central subscribed to characteristic `FFE1` of service `FFE0`. The wheel stood still, powered on, battery at about 87 %.

- Format: one BLE notification per line, `<seconds since start> <hex bytes>`. Lines keep the real packet splits
  (frames arrive as 20 + 4 byte notifications), so decoders must reassemble a byte stream.
- 456 notifications in 25 s, which reassemble into 380 frames of 24 bytes: `55 AA`, 16 payload bytes, type at byte 18,
  sub-index at byte 19, footer `5A 5A 5A 5A`. One frame group (types 00, 01, 02 or 03, 04, 07) every ~310 ms.
- Values at the time: pack 129.7 V, two packs of 32 cells at about 4.07 V, board temperature about 25 °C, odometer
  1,468,670 m, firmware banner `GW1825020`, drive mode 1.
- It holds telemetry values only: no BLE name, address or other identifiers.
