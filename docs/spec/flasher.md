# OTA flasher (M6)

## Status

The flasher runs end to end against the simulated wheel. It cannot flash a real wheel.
No default build contains a transport to a real wheel, and `euc flash` refuses anything but the simulator.
Gates 2 and 3 refuse every real flash today:
our images need a signature that this project cannot make yet,
and gate 3 needs a 30 s emulator ride that is not implemented yet.
SAFETY.md, "Flasher rules", lists what must hold before the first real flash.

The vendor bootloader is not public and was never dumped. Its behaviour here is a model.
Every statement below about the bootloader is an assumption unless it says otherwise.

## Parts

| Part | What it does |
|---|---|
| `crates/euc-ymodem` | Sans-IO YMODEM-1K with CRC-16: a sender and a receiver state machine. No I/O, no clock. |
| `crates/euc-ble` | The outbound byte guard, the real-write policy, and a CoreBluetooth central behind the `central` feature. |
| `crates/euc-flasher` | Gates 1-7, the session state machine, recovery, the JSONL log, the ledger, the simulated wheel, the fault campaign. `BleTransport` sits behind the `ble` feature. |
| `euc flash` | The command line: `--sim`, `--dry-run`, `--campaign`. |

`crates/euc-flash-sim` (the application-slot flash model) stores the simulated flash. The flasher does not change it.

## Gates

Every gate result goes to the log. A failed gate stops the session before the next step.

| Gate | Checks | Simulator | Real wheel |
|---|---|---|---|
| 1 Image | Size, initial stack pointer, Thumb vectors inside the image, load address, info block | same | same |
| 2 Provenance | A pinned stock build, or our sealed image | pins from any profile source; our sealed unsigned images | built-in pins only; our images need a verified Ed25519 signature (not implemented: refused) |
| 3 Dry-run | A simulator dry-run of the same sha256 on this machine | skipped: this run is the dry-run | a ledger entry with flash, re-entry and a 30 s ride (the ride is not implemented: refused) |
| R Recovery image | The stock image for recovery is at hand and is a pinned stock build | same | built-in pin only |
| 7 First flash | The first flash of a wheel is a stock image | per simulated wheel | per BLE identifier, from the ledger |
| 4 Identity | Begode frames, a `V` banner, `NAME:` from `N` that matches the profile, a 32S pack voltage (96.0 to 134.4 V) | same | same |
| 5 Wheel state | Five seconds without writes: speed 0, no alert flags, battery at least 118 V, board below 60 °C, no frame gap over 1.5 s | same | same |
| 6 Confirmation | The person types `FLASH EXTREME` (the model name from the profile) | scripted | typed |
| 5 again | The same window right before the bootloader request | same | same |

Gates 1, 2, 3, R and 7 run before the link carries any byte.
Gates 4, 5 and 6 write only `V` and `N`.

## Session

1. Offline gates 1, 2, 3, R, 7.
2. Listen 1.5 s, ask `V`, then `N` (each at most twice, 2.5 s each). Gate 4.
3. Watch 5.1 s without writing. Gate 5. Ask for the typed confirmation. Gate 6. Watch again. Gate 5.
4. Arm the guard. Send `!`, wait 300 ms, send `@`.
5. Wait up to 12 s for the bootloader. Our clean firmware first answers `SERVICE:CONFIRM`:
   the flasher then asks for a short press of the power button and waits again.
   `SERVICE:DENIED`, or frames that do not stop, end the session as *aborted*: the application still runs.
6. YMODEM transfer of the image (below).
7. Wait up to 15 s for frames from the new image, then ask `V` and `N` (each up to four times). Success is *flashed*.
8. In the simulator, `euc flash --sim` then checks re-entry: `!` `@` again and a second transfer of the same image.

A failure after step 4 starts recovery.
Recovery waits for the bootloader, sends two `CAN` to end any half session, and transfers the stock image.
It tries twice. Success is *restored*.
After two failures the flasher stops (*stopped*) and names what it saw last: the bootloader, an application, or nothing.
If no YMODEM byte went out and an application still answers, the outcome is *aborted* instead.
The flasher never retries other image variants.

| Outcome | Meaning |
|---|---|
| `flashed` | The image was delivered, booted and answered `V` and `N`. |
| `refused` | A gate refused. The wheel was not changed. |
| `aborted` | The bootloader request did not take. The previous application still runs. |
| `restored` | The image failed after the bootloader took over. The stock image now runs. |
| `stopped` | Recovery failed. See "Recovery" below. |

## Outbound guard

`euc_ble::Guard` judges every write, in the simulator and on a real link. It sees every notification.

- `+`, `-`, `c`, `y`: never sent, in any state.
- Commands: only `V` and `N`, at least 20 ms apart, and never while the bootloader listens.
- `!` only after the flasher armed the entry, while frames arrived within 2 s, and 250 ms after the previous write.
  After `!` only `@` may follow, at least 250 ms later. The entry is then spent.
- YMODEM only while the bootloader proved itself: no frame for 2 s, and a notification that holds only receiver bytes
  (`C`, ACK, NAK, CAN) since the last frame and since the link came up.
  Only whole, valid transmissions pass: a block with a correct CRC, `EOT`, or two `CAN`.
- A reconnect clears all evidence. An application that boots sends no frame for 2 s.

The session applies the same rule before it sends YMODEM, so a guard refusal means the two disagree (a bug).
The fault campaign counts guard refusals as failures.

## Real writes

The real-write path has three locks:

1. The `central` feature of `euc-ble` and the `ble` feature of `euc-flasher`. No default build enables them.
   `euc` does not enable them and has no command that reaches them.
2. `WritePolicy`: every write is refused unless `EUC_BLE_REAL_WRITES` equals `swd-backup-done:<BLE identifier>`.
   Setting it states that a full SWD backup of that wheel exists and that its owner approved this run.
   It names one wheel; it cannot arm another.
3. The guard and gates 1-7.

The CoreBluetooth central was compiled and linted, never run against a wheel. Linux and Windows get a stub that reports
`Unsupported`.

## YMODEM

- Batch YMODEM with CRC-16/XMODEM. Block 0 carries `app.bin`, NUL, the size in decimal.
- 1024-byte `STX` blocks; a tail of at most 128 bytes goes in a 128-byte `SOH` block. Padding is `0x1A`.
- The receiver refuses the first `EOT` and accepts the second. An empty block 0 closes the batch.
- Two `CAN` cancel from either side.
- Sender: 10 transmissions per packet; 3 s answer timeout in the flasher.
  The timer restarts only when the sender transmits or its state changes, so `C` polls cannot hold off a retransmission.
- An `ACK` of the `EOT` is tentative until the receiver asks for the next file with `C`.
  A `NAK` instead means the `ACK` answered an earlier packet; the `EOT` goes out again.
- The flasher drops receiver bytes, except `CAN`, that arrive before its latest packet finished leaving.
  They answer an earlier packet.
- Three `C` polls while a data block or the `EOT` is out mean the receiver started over. The flasher cancels.
- After the image is delivered, the batch close gets one try.

## Emulated bootloader (model assumptions)

From docs/protocol/begode.md: `!` `@` at standstill stops the motor, makes the application erase one settings page,
and resets; the bootloader then takes a new image over the same UART (inferred).
The model adds these assumptions; none of them is confirmed.

- With the request page erased, or without a committed application, the bootloader stays and polls `C` once a second.
- It takes one file by YMODEM-1K with CRC-16, with a 2 s receive timeout and 10 errors before it cancels.
- It refuses a header without a size or larger than the 48 KiB slot.
- It keeps the whole file, then checks it with this project's image gate and writes it through `euc-flash-sim`
  (erase every page, write 1 KiB chunks, compare, gate again). A real bootloader probably writes while it receives;
  the outcome classes are the same because the request page stays erased until the commit succeeds.
- It commits after the batch close, or 3 s after the end of file.
- A refused or failed write keeps the bootloader active: it sends two `CAN` and polls again.
- A power loss during the commit leaves the wheel off; at the next power-on the bootloader stays.

The vendor bootloader may check less. A bootloader that boots an unchecked image could leave a wheel that only SWD
can recover. The flasher's own gates are the defence against that, not the model.

## Simulated wheel and link

- Application: one group of five frames every 304 ms, first group 2.0 s after reset, `V` answers `GW1825020`,
  `N` answers `NAME:EXTREME`, every received byte pauses telemetry for two groups and takes the one command slot.
  `!` then `@` at least 202 ms later, at standstill, resets into the bootloader after 1.2 s.
  The clean variant answers `SERVICE:CONFIRM` and needs a power-button press within 10 s.
- Link: 20-byte writes every 10 ms, 15 to 25 ms latency, in order. A 48 KiB image takes about 30 s of virtual time.
- Time is virtual. A full session takes milliseconds of wall time.

## Fault campaign

`crates/euc-flasher/tests/campaign.rs` runs 320 seeded sessions (seed `0x5EED_2026_1022`).
Each run draws an image (55 % stock, 30 % ours with a stock flash before, 5 % ours first, 10 % bad images),
the stock or clean application, and zero to three faults:

| Fault kind | Injection |
|---|---|
| `drop-write`, `corrupt-write` | One BLE write in the transfer lost or changed |
| `drop-notification`, `corrupt-notification` | Wheel-to-phone notifications lost or changed |
| `stall` | One notification, and everything behind it, 2 to 9 s late |
| `link-loss`, `link-loss-early` | The link drops for 1 to 25 s, in the transfer or among the commands |
| `power-loss-transfer` | The wheel loses power during the transfer |
| `power-loss-erase`, `power-loss-write` | Power loss during the erase or the write of the target or recovery commit |
| `flash-transient`, `flash-corrupt` | A flash write fails once, or stores a wrong bit |
| `wrong-image-on-wire` | One data block arrives with other bytes and a matching CRC |
| `slow-bootloader` | The bootloader polls 3 to 10 s late |
| `wheel-moving`, `low-battery`, `hot-board`, `alarm` | Gate 5 conditions |
| `entry-ignored` | The application ignores `!` `@` |
| `wrong-confirmation` | The person types the wrong text |
| `restore-fails` | Every commit stores a wrong bit: the flasher must stop |
| `operator-absent` | Nobody presses the button or switches the wheel off and on |

Each run is judged on the simulated wheel, not on the flasher's word:

- *success*: `flashed` and the wheel runs exactly that image;
- *recovered*: `restored` and the wheel runs the stock image;
- *safe stop*: `refused` or `aborted` with the original image still running, or `stopped` with the wheel recoverable:
  after a power-on it runs a committed image, or its bootloader takes a clean stock restore;
- *unrecoverable*: anything else, including a false success, a changed bootloader region or settings page,
  a command byte other than `V`, `N`, `!`, `@` reaching the application, a service sequence other than `!` `@`,
  or a write the guard refused.

Result on 2026-10-22: 320 runs, 3.4 simulated hours, **0 unrecoverable**.

| Verdict | Runs |
|---|---|
| success | 108 |
| recovered | 35 |
| safe stop | 177 (138 refused, 26 aborted, 13 stopped and then restored cleanly) |
| unrecoverable | 0 |

Two more seeds with 2,000 runs each (`euc flash --campaign 2000 --seed 77` and `--seed 78`) also ended with 0
unrecoverable runs. A deliberate bug (skipping the boot check) made 156 of the 320 runs unrecoverable, so the judge
detects false successes.

The campaign proves the flasher's logic against the modelled bootloader and link. It does not prove the vendor
bootloader, real BLE timing, or real flash behaviour.

## Recovery

When the flasher stops:

1. If the wheel was switched off, switch it on.
2. If its bootloader polls (the flasher reports `bootloader`), run the stock restore again.
3. If an application runs, flash again normally; the flasher cannot tell which image runs.
4. If nothing answers, or the bootloader does not take the stock image, use SWD with the backup made before the first
   flash (SAFETY.md, rule 3). The SWD procedure belongs to M8 and is not written yet.

## Log and ledger

The JSONL log has one object per line, `type` first: `start`, `gate`, `step`, `tx` (hex, `command` or `ymodem`),
`rx` (hex, arrival time), `link` (`lost`, `reconnected`, `reconnect-failed`, `held`, `not-sent`), `operator`,
`outcome`. Times are microseconds on the transport's clock. `euc flash --sim --log FILE` writes it; a real flash
must always write it. A log of a stock flash holds the stock image in hex: keep such logs out of the repository
(the vendor guard refuses them).

The ledger (`EUC_FLASH_LEDGER`, default `<data dir>/euc-studio/flasher-ledger.json`) keeps dry-runs for gate 3 and
flash history for gate 7. `euc flash --sim` records a dry-run only for a real profile image that flashed.

## Commands

```sh
euc flash --sim --synthetic                       # the simulator's own stock image, no vendor image needed
euc flash --sim IMAGE [--stock-first]             # real profile; the cached stock build is the recovery image
euc flash --sim IMAGE --inject power-loss-erase --seed 3 --log flash.jsonl
euc flash --dry-run IMAGE                         # offline gates for a real wheel; connects to nothing
euc flash --campaign 300 --seed 7                 # the fault campaign and its distribution
euc flash IMAGE                                   # refused: this build cannot flash a real wheel
```

## Open items

- Ed25519 signing and verification for our images (gate 2 on a real wheel).
- The 30 s emulator ride of the image for gate 3 (`euc sim run --scenario` with the image).
- Bootloader facts: its checks, size limit, timing, request-page format. They need the SWD dump.
- A run of the central against `euc sim serve --ble` on a second machine, listen-only, before any real wheel.
- The flasher wizard in the app.
