import { useState } from "react";
import { engine, useEngineData } from "../../engine/index.ts";
import { size as controlSize } from "../../design/tokens.ts";
import { prepareFirmwareAudio } from "../../engine/buzzerAudio.ts";
import { cx } from "../../lib/cx.ts";
import { useUi } from "../../state/ui.ts";
import { Bell, BellOff } from "../../ui/icons.tsx";
import { Button } from "../../ui/Button.tsx";
import { DisplayReplica } from "./DisplayReplica.tsx";
import { SevenSegment } from "./SevenSegment.tsx";
import { ledCss, litCount } from "./ledColor.ts";
import { TailLight } from "./TailLight.tsx";
import { HeadlightPreview } from "./HeadlightPreview.tsx";
import { headlightOutput } from "./headlightOutput.ts";
import { useBuzzerSound } from "./useBuzzerSound.ts";
import { WheelButtons } from "./WheelButtons.tsx";

/** The display leads at normal widths; small viewports place the previews alongside it. */
export function WheelOutputs({ className }: { className?: string }) {
  useEngineData();
  const source = engine;
  const preference = useUi((s) => s.buzzerSound);
  const native = source.audio?.backend === "native" ? source.audio : null;
  const sound = native?.enabled ?? preference;
  const [failure, setFailure] = useState<{
    source: typeof engine;
    epoch: number | undefined;
    audio: typeof engine.audio;
    text: string;
  } | null>(null);
  const setSound = useUi((s) => s.setBuzzerSound);
  const audio = useBuzzerSound();
  const o = engine.outputs;
  const lights = engine.lights;
  const output = headlightOutput(engine.kind === "demo" ? null : o?.headlight_duty);
  const leds = (o?.leds ?? []) as [number, number, number][];
  const total = lights?.leds.length ?? leds.length;
  const lit = lights ? litCount(lights, leds) : leds.filter(([r, g, b]) => r + g + b > 0).length;
  const problem =
    failure?.source === source &&
    failure.epoch === source.status.runEpoch &&
    failure.audio === source.audio
      ? failure.text
      : null;
  const audioDisabled =
    source.kind !== "live"
      ? (audio?.detail ?? "Firmware audio is unavailable.")
      : !source.audio
        ? "Audio capability is pending."
        : native && !source.setSound
          ? "Native audio control is unavailable."
          : undefined;
  const soundLabel = audioDisabled
    ? "No audio"
    : problem
      ? "Audio error"
      : native
        ? (audio?.label ?? "Audio pending")
        : audio?.blocked
          ? "Enable audio"
          : sound
            ? "Sound on"
            : "Sound off";
  const toggleSound = async () => {
    const epoch = source.status.runEpoch;
    setFailure(null);
    if (native) {
      if (!source.setSound) return;
      const enabled = !native.enabled;
      try {
        await source.setSound(enabled);
        if (source === engine && source.status.runEpoch === epoch) setSound(enabled);
      } catch (error) {
        if (source === engine && source.status.runEpoch === epoch)
          setFailure({ source, epoch, audio: source.audio, text: String(error) });
      }
    } else if (!sound || audio?.blocked) {
      void prepareFirmwareAudio(source);
      setSound(true);
    } else setSound(false);
  };
  const audioIssue =
    problem ??
    (native?.state === "error"
      ? (native.error ?? "Audio output failed.")
      : audio?.blocked
        ? "Audio is blocked. Select the bell to enable audio."
        : null);
  const soundControl = (
    <Button
      size="md"
      variant="ghost"
      className="w-(--control-md) px-0!"
      aria-disabled={!!audioDisabled || undefined}
      aria-label="Buzzer sound"
      aria-description={soundLabel}
      aria-pressed={sound}
      onClick={() => {
        if (!audioDisabled) void toggleSound();
      }}
    >
      {sound ? <Bell aria-hidden="true" /> : <BellOff aria-hidden="true" />}
    </Button>
  );
  return (
    <div
      className={cx(
        "flex w-[320px] max-w-full flex-col rounded-12 border border-hairline-strong bg-card/90 p-16 backdrop-blur-[12px]",
        className,
      )}
      data-testid="wheel-outputs"
    >
      <div className="flex items-center justify-between gap-8">
        <span className="label">Display</span>
        {soundControl}
      </div>
      {audioIssue && (
        <p role={audio?.blocked ? "status" : "alert"} className="text-11 text-danger-text">
          {audioIssue}
        </p>
      )}
      <div className="flex flex-col compact-outputs:flex-row compact-outputs:items-center compact-outputs:gap-16">
        <div className="flex min-h-[136px] shrink-0 items-center justify-center py-16 compact-outputs:min-h-[80px] compact-outputs:py-8 compact-outputs:[&_canvas]:h-[64px]! compact-outputs:[&_canvas]:w-auto!">
          {!o ? (
            <span className="text-12 text-fg-2">Waiting for display</span>
          ) : engine.display ? (
            <DisplayReplica layout={engine.display} outputs={o} height={104} />
          ) : (
            <SevenSegment
              grid={o.display_grid}
              on={o.display_on}
              brightness={o.display_brightness}
              size={controlSize["control-md"]}
            />
          )}
        </div>
        <div
          className="grid min-w-0 flex-1 grid-cols-2 gap-16 border-t border-hairline py-16 compact-outputs:gap-8 compact-outputs:border-t-0 compact-outputs:py-8"
          data-testid="output-lights"
        >
          <figure className="flex min-w-0 flex-col items-center gap-4">
            <figcaption className="text-12 font-medium text-fg-2">Front light</figcaption>
            <div className="flex h-[48px] w-full items-center justify-center compact-outputs:h-[32px]">
              <HeadlightPreview duty={output.duty} />
            </div>
            <span className="num text-11 text-fg-2">{output.state}</span>
          </figure>
          <figure className="flex min-w-0 flex-col items-center gap-4">
            <figcaption className="text-12 font-medium text-fg-2">Rear light</figcaption>
            <div className="flex h-[48px] w-full items-center justify-center compact-outputs:h-[32px] [&>svg]:max-h-full [&>svg]:max-w-full">
              {o && lights ? (
                <TailLight layout={lights} outputs={o} width={112} />
              ) : o ? (
                <div className="flex flex-wrap gap-2" aria-label="Rear LED output">
                  {leds.map((color, i) => (
                    <span
                      key={i}
                      className="size-4 rounded-pill"
                      style={{
                        background:
                          color[0] + color[1] + color[2] > 0 ? ledCss(color) : "var(--color-track)",
                      }}
                    />
                  ))}
                </div>
              ) : (
                <span className="text-11 text-fg-2">Unknown</span>
              )}
            </div>
            <span className="num text-11 text-fg-2">{o ? `${lit}/${total} lit` : "Unknown"}</span>
          </figure>
        </div>
      </div>
      <div className="border-t border-hairline pt-16 compact-outputs:pt-8">
        <WheelButtons />
      </div>
    </div>
  );
}
