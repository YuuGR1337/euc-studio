import React from "react";
import { useCurrentFrame } from "remotion";
import { blurFade, enter, leave } from "../anim";
import { Mark } from "../components/Mark";
import { C, FONT } from "../theme";
import { TITLE } from "../timeline";

/** The name and the README's first line over the city ride's start, blurred, then clearing. */
export const Title: React.FC = () => {
  const f = useCurrentFrame();
  const out = leave(f, TITLE.dur - 20, 20);
  const part = (delay: number) => blurFade(enter(f, 4 + delay, 22), out);
  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        background: `rgba(0,0,0,${0.62 * (1 - out)})`,
        backdropFilter: `blur(${22 * (1 - out)}px)`,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <div style={{ ...part(0), display: "flex", alignItems: "center", gap: 26 }}>
        <Mark size={92} />
        <div style={{ font: `700 92px/1 ${FONT.ui}`, color: C.fg, letterSpacing: "-.02em" }}>
          EUC Studio
        </div>
      </div>
      <div style={{ ...part(7), marginTop: 30, font: `500 34px/1.3 ${FONT.ui}`, color: C.fg2Safe }}>
        Run real electric-unicycle firmware in a 3D simulation with a physical rider.
      </div>
    </div>
  );
};
