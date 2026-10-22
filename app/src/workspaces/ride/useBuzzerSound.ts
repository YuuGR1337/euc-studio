import { useEffect, useState } from "react";
import { BuzzerAudio, firmwareAudioHold, usesRendererAudio } from "../../engine/buzzerAudio.ts";
import { engine, useEngineVersion, type EngineClient } from "../../engine/index.ts";
import { useUi } from "../../state/ui.ts";

export interface SoundHold {
  label: string;
  detail: string;
  blocked?: boolean;
}
function sourceReason(source: EngineClient): SoundHold | null {
  if (source.kind === "playback")
    return { label: "No audio", detail: "This recording contains no buzzer audio." };
  if (source.kind !== "live")
    return { label: "No audio", detail: "The demo does not run firmware buzzer outputs." };
  return firmwareAudioHold(source);
}

/** Native audio owns desktop output. Only an explicit unsupported backend uses Web Audio. */
export function useBuzzerSound(): SoundHold | null {
  useEngineVersion();
  const source = engine;
  const on = useUi((s) => s.buzzerSound);
  const rendererRoute = usesRendererAudio(source);
  const [state, setState] = useState<{ source: EngineClient; reason: SoundHold | null } | null>(
    null,
  );

  useEffect(() => {
    if (!on || source.kind !== "live" || !rendererRoute) return;
    const audio = new BuzzerAudio();
    let unavailable = false;
    try {
      audio.start();
    } catch {
      unavailable = true;
      audio.stop();
    }
    const resume = () => audio.resume();
    window.addEventListener("pointerdown", resume);
    window.addEventListener("keydown", resume);
    let shown: string | undefined;
    let timeVersion = source.time.version;
    let epoch = source.status.runEpoch;
    let scenario = source.scenarioId;
    let firmware = source.firmware;
    let simTime = source.latest.t;
    // This is a truthful explanation, not a queue of old tones or an inferred boot beep.
    let lateStart = source.latest.t > 0;
    let firstReady = audio.audible;
    const check = () => {
      if (!usesRendererAudio(source)) {
        audio.stop();
        return false;
      }
      const reset =
        (source.time.length === 0 && source.time.version !== timeVersion) ||
        source.latest.t < simTime ||
        source.status.runEpoch !== epoch ||
        source.scenarioId !== scenario ||
        source.firmware !== firmware;
      timeVersion = source.time.version;
      simTime = source.latest.t;
      epoch = source.status.runEpoch;
      scenario = source.scenarioId;
      firmware = source.firmware;
      if (reset) {
        lateStart = false;
        firstReady = audio.audible;
      }
      if (!firstReady && source.latest.t > 0) lateStart = true;
      if (audio.audible) firstReady = true;
      if (reset || source !== engine || !useUi.getState().buzzerSound) audio.silence();
      if (source !== engine || !useUi.getState().buzzerSound) return false;
      const hold: SoundHold | null = unavailable
        ? { label: "No audio", detail: "The browser cannot open Web Audio." }
        : !audio.audible
          ? {
              label: "Enable audio",
              detail:
                "Web Audio is blocked until a user gesture. Enable audio to hear new firmware tones. Earlier tones are unavailable.",
              blocked: true,
            }
          : sourceReason(source);
      const reason = hold ?? {
        label: "Sound on",
        detail: lateStart
          ? "Web Audio starts during this run. Earlier firmware tones are unavailable. New tones play at their firmware frequency."
          : "Web Audio plays real firmware tones. Native audio capability is not active in this renderer route.",
      };
      if (hold) audio.silence();
      if (shown !== reason.detail) {
        shown = reason.detail;
        setState({ source, reason });
      }
      return !hold;
    };
    const offTones = source.subscribeTones((tones) => {
      if (check()) audio.play(tones, source.latest.t);
    });
    const offStatus = source.subscribe(check);
    const offAudio = audio.subscribeState(check);
    check();
    return () => {
      offTones();
      offStatus();
      offAudio();
      window.removeEventListener("pointerdown", resume);
      window.removeEventListener("keydown", resume);
      audio.stop();
    };
  }, [on, source, rendererRoute]);

  if (source.kind !== "live") return sourceReason(source);
  if (!source.audio)
    return {
      label: "Audio pending",
      detail: "The simulator has not reported its audio capability.",
    };
  if (source.audio.backend === "native") {
    const native = source.audio;
    if (native.state === "error")
      return {
        label: "Audio error",
        detail: native.error ?? "Native audio is unavailable. Web Audio remains off.",
      };
    if (!native.enabled || native.state === "muted")
      return { label: "Sound off", detail: "Native firmware audio is muted." };
    if (native.state !== "ready")
      return { label: "No audio", detail: native.error ?? "Native audio is unavailable." };
    const hold = sourceReason(source);
    if (hold) return hold;
    return {
      label: "Sound on",
      detail: "Native audio plays real firmware tones. Web Audio remains off.",
    };
  }
  return on ? (state?.source === source ? state.reason : sourceReason(source)) : null;
}
