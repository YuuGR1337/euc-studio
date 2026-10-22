# Recording integration acceptance

Updated 2026-10-21.
This report describes the unified .eucrec implementation on m7-human-rider.
The older EUCR-only completion-branch report no longer describes this tree.

## Capture and replay

RecordingState binds metadata before the first sample.
The runner records samples before the lossy telemetry outbox.
Completion, halt, replacement, and shutdown finish the captured segment.
An unsaved capture survives a run replacement.

The runner integration test first failed without this connection.
It now preserves the initial sample, terminal sample, events, BLE frames, and output changes.
The completed capture survives restart without mixing runs.

The desktop app performed its real Downloads save command.
The native file picker then opened the same file.
Playback restored the event timeline, phone frames, telemetry, display, and lights.
The live run stayed paused during playback and after return.

Python read the same desktop capture.
It returned 1,575 samples, 20 events, 406 BLE frames, and 516 output changes.
The context retained scenario mode and the rigid rider ID.

## Input limits and review corrections

The independent review reproduced repeated-index amplification.
It also found that unknown channels escaped the desktop decoded-value budget.

The canonical reader now validates physical chunk coverage, unique index offsets, counts, and sample bounds.
ReadLimits checks cumulative samples, stored values, sparse bytes, and item counts before allocation.
Unknown channels still count toward the two-million-value desktop limit.
Unfinished files use the same limits during recovery.
Their complete valid chunks remain readable.

Desktop scenario context now records the actual rider and external aids.
A cached-stock regression failed when context was absent and passes with the corrected header.

Saved flat scenario ground remains distinct from unavailable ground.
Optional scenario reconstruction accepts at most 10 km and 128 terrain features.
Larger or invalid geometry remains unavailable while valid telemetry stays readable.

## Playback rules

The selected file controls sample time, poses, events, phone frames, and board outputs.
Backward seeks rebuild those tracks from the saved timeline.
Firmware commands and physical buttons remain unavailable during playback.
Closing playback restores the paused live source.
Opening the Recordings sheet releases scenario manual controls.

The app reconstructs display geometry from its installed wheel profile.
It does not preserve an exact historical profile bundle.
Buzzer frequencies are recorded, but precise audio playback remains unimplemented.

## Remaining limits

Disk-full and flush-failure injection remain open.
The tests do not prove long-term storage reliability or physical accuracy.
The interactive Python Session retains the GIL and stays on its creating thread.
Full run_scenario calls still release the GIL.
