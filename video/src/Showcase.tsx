import React from "react";
import { AbsoluteFill, Audio, Sequence, staticFile } from "remotion";
import "./fonts";
import { Caption } from "./components/Caption";
import { Footage } from "./components/Footage";
import { Close } from "./scenes/Close";
import { Title } from "./scenes/Title";
import { C } from "./theme";
import { scene, TITLE, type SceneName } from "./timeline";

const At: React.FC<{ name: SceneName; children: React.ReactNode }> = ({ name, children }) => {
  const { from, dur } = scene(name);
  return (
    <Sequence from={from} durationInFrames={dur} name={name}>
      {children}
    </Sequence>
  );
};

/** A dark band under a caption that sits on the 3D view. */
export const TopShade: React.FC = () => (
  <div
    style={{
      position: "absolute",
      inset: 0,
      bottom: "auto",
      height: 300,
      background: "linear-gradient(rgba(0,0,0,.62), rgba(0,0,0,0))",
    }}
  />
);

/** The music's gain under the firmware's beeps (public/audio/LICENSES.md): a beep cuts through. */
const MUSIC = 0.3;

// Captions are the README's own words; kickers say what runs.
export const Showcase: React.FC = () => (
  <AbsoluteFill style={{ background: C.bg }}>
    <Audio src={staticFile("audio/music.mp3")} volume={MUSIC} />
    <At name="city">
      <Footage shot="city" dur={scene("city").dur}>
        <TopShade />
        <Sequence from={TITLE.dur - 6}>
          <Caption
            kicker="Free Ride · stock firmware · a generated city"
            title="See the firmware work."
            shadow
          />
        </Sequence>
      </Footage>
    </At>
    <Sequence from={TITLE.from} durationInFrames={TITLE.dur} name="title">
      <Title />
    </Sequence>
    <At name="pothole">
      <Footage shot="pothole" dur={scene("pothole").dur} framed>
        <Caption
          kicker="Scenario · a 5 cm pothole at 20 km/h"
          title="Watch balance and motor control react to a rider in 3D"
        />
      </Footage>
    </At>
    <At name="cutoff">
      <Footage shot="cutoff" dur={scene("cutoff").dur} framed>
        <Caption
          kicker="Scenario · overlean at top speed"
          title="Tiltback and cutoffs, with scopes and an event timeline"
        />
      </Footage>
    </At>
    <At name="compare">
      {/* The Ride engine under Compare is the browser demo; its status bar stays out of view. */}
      <Footage shot="compare" dur={scene("compare").dur} framed trimBottom={40}>
        <Caption
          kicker="Compare · stock and ours-compat on the same ride"
          title="Compare stock and custom firmware"
          sub="Run both on the same ride, side by side, and find the first moment they differ."
        />
      </Footage>
    </At>
    <At name="close">
      <Close />
    </At>
  </AbsoluteFill>
);
