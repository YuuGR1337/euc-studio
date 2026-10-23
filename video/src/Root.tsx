import React from "react";
import { AbsoluteFill, Composition, Sequence, Still } from "remotion";
import { Footage } from "./components/Footage";
import { Mark } from "./components/Mark";
import { Showcase, TopShade } from "./Showcase";
import { C, FONT, H, W } from "./theme";
import { FPS, scene, TOTAL } from "./timeline";

/** The poster: the city ride mid-turn under the name and the README's first line. */
const Poster: React.FC = () => (
  <AbsoluteFill style={{ background: C.bg }}>
    <Sequence from={-150}>
      <Footage shot="city" dur={scene("city").dur}>
        <TopShade />
      </Footage>
    </Sequence>
    <div style={{ position: "absolute", top: 54, left: 0, right: 0, textAlign: "center" }}>
      <div style={{ display: "inline-flex", alignItems: "center", gap: 20 }}>
        <Mark size={68} />
        <div style={{ font: `700 68px/1 ${FONT.ui}`, color: C.fg, letterSpacing: "-.02em" }}>
          EUC Studio
        </div>
      </div>
      <div style={{ marginTop: 18, font: `500 28px ${FONT.ui}`, color: C.fg, opacity: 0.86 }}>
        Run real electric-unicycle firmware in a 3D simulation with a physical rider.
      </div>
    </div>
  </AbsoluteFill>
);

export const Root: React.FC = () => (
  <>
    <Composition
      id="Showcase"
      component={Showcase}
      durationInFrames={TOTAL}
      fps={FPS}
      width={W}
      height={H}
    />
    <Still id="Poster" component={Poster} width={W} height={H} />
  </>
);
