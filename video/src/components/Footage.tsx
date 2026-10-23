import React from "react";
import { Audio, OffthreadVideo, staticFile, useCurrentFrame } from "remotion";
import { blurFade, enter, lin } from "../anim";
import { C, H, W } from "../theme";
import { FPS, SHOTS, XF, type ShotId } from "../timeline";

/** The run's own sound from the footage's first frame, faded in and out over the cross-dissolves. */
const RunSound: React.FC<{ shot: ShotId; dur: number }> = ({ shot, dur }) => {
  const s = SHOTS[shot];
  const startFrom = Math.max(0, Math.round((s.t0 - s.audioT0) * FPS));
  return (
    <Audio
      src={staticFile(s.audio)}
      startFrom={startFrom}
      endAt={startFrom + dur}
      volume={(f) => lin(f, [0, XF, dur - XF, dur], [0, 1, 1, 0])}
    />
  );
};

/** Box the window shots sit in, below their caption: 80 % of the frame. */
export const BOX = { w: W * 0.8, h: H * 0.8, top: 176 };

/**
 * One captured shot. `framed`: the studio's whole window, in a box under the caption;
 * otherwise the shot fills the frame (the 3D view alone). `trimBottom`: source rows (of 1080)
 * the box leaves out at the bottom, scaling the shot up to keep its shape.
 */
export const Footage: React.FC<{
  shot: ShotId;
  dur: number;
  framed?: boolean;
  trimBottom?: number;
  children?: React.ReactNode;
}> = ({ shot, dur, framed, trimBottom = 0, children }) => {
  const f = useCurrentFrame();
  const style = blurFade(enter(f, 0, XF), 0, 0, 10);
  const grow = H / (H - trimBottom);
  const video = (
    <OffthreadVideo
      src={staticFile(SHOTS[shot].video)}
      muted
      style={{
        position: "absolute",
        top: 0,
        left: `${(-(grow - 1) / 2) * 100}%`,
        width: `${grow * 100}%`,
        height: `${grow * 100}%`,
        objectFit: "cover",
      }}
    />
  );
  return (
    <div style={{ position: "absolute", inset: 0, background: C.bg, ...style }}>
      {framed ? (
        <div
          style={{
            position: "absolute",
            left: (W - BOX.w) / 2,
            top: BOX.top,
            width: BOX.w,
            height: BOX.h,
            borderRadius: 16,
            overflow: "hidden",
            boxShadow: `0 0 0 1px ${C.hairlineStrong}, 0 30px 80px -20px rgba(0,0,0,.8)`,
          }}
        >
          {video}
        </div>
      ) : (
        <div style={{ position: "absolute", inset: 0 }}>{video}</div>
      )}
      {children}
      <RunSound shot={shot} dur={dur} />
    </div>
  );
};
