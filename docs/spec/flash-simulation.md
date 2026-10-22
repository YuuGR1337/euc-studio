# Flash simulation foundation (M6)

## Scope and assumptions

`euc-flash-sim` models one application slot in memory. It has no hardware access or transport.
The vendor OTA wire protocol is **UNKNOWN**. This model defines no Begode bootloader protocol or BLE commands.
It models application storage transactions for later software scenarios.

The caller supplies a validated wheel profile and synthetic flash geometry.
The geometry gives the flash base, flash size, and erase page size.
The profile gives the application slot and settings page addresses.
All flash below the application slot represents the protected bootloader.
Each settings page spans one erase page. All bytes outside the application slot stay protected.
These geometry rules are model assumptions. They do not establish a vendor flash layout or erase algorithm.

The model starts with erased synthetic storage and an explicit `RecoverableBootloader` state.
A successful transaction can establish an initial application before a later update.
The model stores no vendor code or fixtures.
The bootloader state represents the ability to accept another complete transaction.
It does not run bootloader machine code.

## Transaction and boot gate

1. Validate the request, fault plan, and image before any storage change.
2. Enter `RecoverableBootloader` and invalidate the previous boot permission.
3. Erase each page in the whole application slot.
4. Write the complete image in ordered chunks.
5. Compare all image bytes and check the erased slot tail.
6. Run the existing `euc-image` gate on the stored bytes.
7. Repeat the complete-image checks at reboot, then allow `Application` state.

Input rejection preserves the current storage and boot state.
An interruption, exhausted retry limit, or failed verification leaves `RecoverableBootloader` state.
The caller can start a new transaction with the complete image after any such result.
A new transaction erases the whole application slot again. It does not resume a partial write.

The image gate uses the existing wheel profile and `euc-image::validate`.
It accepts the same unsealed development images as the existing image gate.
Byte equality adds a completeness check for these images.
The model does not require the future seal API or check signatures.
It does not implement production safety approval, first-stock policy, or physical confirmation.

The model has one application slot and no backup slot.
Erasure can destroy the previous application. Recovery means that the simulated bootloader accepts another image.
Recovery does not mean that the previous application remains available or that rollback exists.

Boot permission is abstract model metadata outside the writable application bytes.
The model invalidates it before erasure and grants it only after the final boot gate.
`power_cycle` preserves this metadata and refuses incomplete transactions.
This assumes reliable metadata and protected bootloader storage. It does not prove physical power-loss safety.

## Deterministic faults and retry limits

Each fault names a phase, a zero-based operation index, and an optional one-based attempt number.
Erase indices select pages. Write indices select chunks. Verify and reboot each use index zero.
An absent attempt number applies the fault to every attempt of that operation.
The model rejects overlapping fault rules and coordinates outside the transaction.

| Effect | Behavior |
|---|---|
| `Transient` | Fail before the operation and retry within the configured limit |
| `Interrupt` | Apply the first half of an erase or write operation, then stop in the bootloader |
| `CorruptWrite` | Complete one write, then flip one stored bit; verification must reject the result |

For interruption, the first half includes the middle byte of an odd length.
Verify and reboot interruptions change no storage and leave the bootloader active.
`CorruptWrite` applies only to write operations. It represents a storage error, with no claim about physical bit transitions.

Each operation permits one through 32 attempts, including its first attempt.
The default permits three attempts and uses 256-byte write chunks.
An interruption never retries automatically. A storage mismatch never retries automatically.
Every accepted transaction has a finite number of pages, chunks, and attempts.
The same initial state, image, options, and fault plan produce the same report and storage.

## API and structured results

`FlashSim::new` checks the profile, geometry, page alignment, region bounds, and region overlap.
The model limits storage allocation to 16 MiB. This is a software resource limit.
`regions` exposes the checked address ranges. `read` gives read-only access to simulated storage.
The API exposes no direct erase or write method.
`update` accepts an `Image`, transaction options, and fault rules.
`state` reports the boot state. `power_cycle` repeats the committed application checks.

Each update report includes the outcome, final boot state, input gate, and final stored-image gate when available.
It also includes operation attempt counts, triggered fault coordinates, and the number of bytes written.
Outcomes distinguish boot success, request rejection, interruption, retry exhaustion, and verification failure.
Reports contain no image bytes. Callers must keep private input data out of public reports and fixtures.

## Integration and proposed verification

The parent will add the crate to workspace default members and update the lockfile.
The parent can connect this library to a later software scenario runner.
Any future transport needs separate protocol evidence and a separate safety design.

Proposed scenarios use generated images with synthetic profiles or valid development info blocks.
They cover successful updates, every fault phase, persistent transient faults, corruption, and a clean recovery transaction.
Boundary cases cover exact slot size, short final chunks, invalid profiles, region overflow, and overlapping settings pages.
They also compare protected storage before and after each transaction.
Each interrupted case needs a power cycle check that refuses the partial application.
Later verification needs repeated fault sweeps and stable report comparisons.
This task adds and runs no tests. Build and lint checks do not establish the M6 scenario completion criteria.
