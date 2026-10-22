import { Slider as S } from "radix-ui";
import { useState } from "react";
import { closeRecording, type PlaybackEngine, useLive, useReplay } from "../engine/index.ts";
import { IconButton } from "../ui/Button.tsx";
import { X } from "../ui/icons.tsx";
import { Pill } from "../ui/Pill.tsx";
import { Tooltip } from "../ui/Tooltip.tsx";

/** `83.4` as `1:23.4`. */
function clock(t: number): string {
  const s = Math.max(0, t);
  const minutes = Math.floor(s / 60);
  return `${minutes}:${(s - minutes * 60).toFixed(1).padStart(4, "0")}`;
}

/** Seek bar of a recording that plays: where it is, its length, and a way back to the live run. */
function Replay({ replay }: { replay: PlaybackEngine }) {
  const t = useLive((l) => l.t, 8, 1);
  const [dragging, setDragging] = useState<number | null>(null);
  const { start, end } = replay.recording;
  const shown = dragging ?? Math.min(Math.max(t, start), end);
  const file = replay.recording.name;
  return (
    <div className="flex items-center gap-8">
      <Tooltip
        content={`${file}${replay.recording.truncated ? " (cut short: it plays up to its last whole second)" : ""}`}
      >
        <span>
          <Pill tone={replay.recording.truncated ? "caution" : "info"}>Recording</Pill>
        </span>
      </Tooltip>
      <S.Root
        value={[shown]}
        min={start}
        max={Math.max(end, start + 0.01)}
        step={0.01}
        aria-label="Seek in the recording"
        onValueChange={([v]) => v !== undefined && setDragging(v)}
        onValueCommit={([v]) => {
          if (v !== undefined) replay.seek(v);
          setDragging(null);
        }}
        className="group relative flex h-[20px] w-[200px] touch-none items-center select-none max-[1279px]:w-[140px]"
      >
        <S.Track className="relative h-[4px] grow overflow-hidden rounded-pill bg-track">
          <S.Range className="absolute h-full rounded-pill bg-accent" />
        </S.Track>
        <S.Thumb className="relative block size-[14px] rounded-pill border border-hairline-strong bg-fg shadow-[0_1px_3px_rgba(0,0,0,0.5)] transition-transform duration-(--dur-snappy) ease-snappy hover:scale-[1.15] active:scale-[1.25]" />
      </S.Root>
      <span className="num w-[96px] text-12 text-fg-2">
        {clock(shown)} / {clock(end)}
      </span>
      <IconButton label="Close recording" icon={<X />} size="sm" onClick={() => closeRecording()} />
    </div>
  );
}

/** The recording seek controls; capture lives in the Recordings workspace. */
export function ReplayBar() {
  const replay = useReplay();
  return replay ? <Replay replay={replay} /> : null;
}
