import React from "react";
import { useCurrentFrame } from "remotion";
import { blurFade, enter } from "../anim";
import { C, FONT } from "../theme";

export type CaptionProps = {
  /** What runs, in the studio's label style: mono, uppercase, accent. */
  kicker: string;
  /** The README's own words. */
  title: React.ReactNode;
  sub?: React.ReactNode;
  /** Frame the caption starts entering. */
  at?: number;
  top?: number;
  shadow?: boolean;
};

/** A kicker, a headline and an optional subline, centred, each blur-fading in turn. */
export const Caption: React.FC<CaptionProps> = ({
  kicker,
  title,
  sub,
  at = 6,
  top = 36,
  shadow,
}) => {
  const f = useCurrentFrame();
  const part = (delay: number) => blurFade(enter(f, at + delay));
  const textShadow = shadow ? "0 2px 18px rgba(0,0,0,.65), 0 1px 3px rgba(0,0,0,.6)" : undefined;
  return (
    <div style={{ position: "absolute", top, left: 0, right: 0, textAlign: "center", textShadow }}>
      <div
        style={{
          ...part(0),
          font: `700 19px ${FONT.mono}`,
          letterSpacing: "0.12em",
          textTransform: "uppercase",
          color: C.accent,
        }}
      >
        {kicker}
      </div>
      <div
        style={{
          ...part(4),
          marginTop: 12,
          font: `700 44px/1.1 ${FONT.ui}`,
          color: C.fg,
          letterSpacing: "-.01em",
        }}
      >
        {title}
      </div>
      {sub ? (
        <div
          style={{ ...part(9), marginTop: 10, font: `500 25px/1.3 ${FONT.ui}`, color: C.fg2Safe }}
        >
          {sub}
        </div>
      ) : null}
    </div>
  );
};
