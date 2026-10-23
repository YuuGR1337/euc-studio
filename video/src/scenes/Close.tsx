import React from "react";
import { useCurrentFrame } from "remotion";
import { blurFade, enter } from "../anim";
import { Mark } from "../components/Mark";
import { C, FONT } from "../theme";
import { XF } from "../timeline";

/** The name, where it lives, and the README's own limit on what the simulator shows. */
export const Close: React.FC = () => {
  const f = useCurrentFrame();
  const part = (delay: number) => blurFade(enter(f, XF + delay, 22));
  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        background: C.bg,
        opacity: enter(f, 0, XF),
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      <div style={{ ...part(0), display: "flex", alignItems: "center", gap: 22 }}>
        <Mark size={76} />
        <div style={{ font: `700 76px/1 ${FONT.ui}`, color: C.fg, letterSpacing: "-.02em" }}>
          EUC Studio
        </div>
      </div>
      <div style={{ ...part(6), marginTop: 28, font: `500 30px ${FONT.mono}`, color: C.accent }}>
        github.com/tarikbc/euc-studio
      </div>
      <div
        style={{
          ...part(12),
          marginTop: 44,
          font: `500 24px/1.4 ${FONT.ui}`,
          color: C.fg2,
          textAlign: "center",
        }}
      >
        The simulator helps inspect behavior under specified conditions.
        <br />
        It does not establish real riding safety.
      </div>
    </div>
  );
};
