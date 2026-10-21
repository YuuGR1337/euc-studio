import type { Ref } from "react";
import { useIsPresent } from "motion/react";
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { currentEngine } from "../../engine/index.ts";
import { PlaybackEngine } from "../../engine/playback.ts";
import { RIDER_MODELS } from "../../engine/freeRide.ts";
import {
  captureStatus,
  desktopRecordingsAvailable,
  openRecording,
  saveCapture,
  startCapture,
  type CaptureStatus,
} from "../../engine/recording.ts";
import type { EngineClient } from "../../engine/types.ts";
import { Button } from "../../ui/Button.tsx";
import { Tabs, TabPanel } from "../../ui/Tabs.tsx";
import { Tooltip } from "../../ui/Tooltip.tsx";
import { Pill } from "../../ui/Pill.tsx";
import { Select } from "../../ui/Select.tsx";
import { Slider } from "../../ui/Slider.tsx";
import { Pause, Play, RotateCcw, StepForward, ScrollText, Info } from "../../ui/icons.tsx";

export interface RecordingPanelProps {
  /** False immediately when the sheet starts closing, before its exit animation ends. */
  active: boolean;
  openButtonRef?: Ref<HTMLButtonElement>;
  engine: EngineClient | PlaybackEngine;
  /** Parent pauses live simulation and changes the active EngineClient. */
  onOpen: (playback: PlaybackEngine) => void;
  /** Parent restores the paused simulation and refreshes active engine subscriptions. */
  onClose: () => void;
}

export function RecordingPanel({
  active,
  openButtonRef,
  engine,
  onOpen,
  onClose,
}: RecordingPanelProps) {
  useSyncExternalStore(engine.subscribe, engine.getVersion, engine.getVersion);
  // Exit animation retains the last props. Presence invalidates imports as soon as exit starts.
  const present = useIsPresent();
  const accepting = active && present;
  const desktop = desktopRecordingsAvailable();
  const playback = engine instanceof PlaybackEngine ? engine : null;
  const input = useRef<HTMLInputElement>(null);
  const imports = useRef({ active: false, revision: 0, request: 0 });
  const [capture, setCapture] = useState<CaptureStatus | null>(null);
  const [tab, setTab] = useState<"playback" | "capture">("playback");
  const [filename, setFilename] = useState("capture.eucrec");
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const [savedPath, setSavedPath] = useState("");

  useLayoutEffect(() => {
    const scope = imports.current;
    scope.active = accepting;
    scope.revision++;
    return () => {
      scope.active = false;
      scope.revision++;
    };
  }, [accepting, engine]);

  useEffect(() => {
    if (!desktop || !accepting) return;
    let polling = true;
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      try {
        const status = await captureStatus();
        if (polling) setCapture(status);
      } catch (e) {
        if (polling)
          setCapture({
            state: "error",
            can_start: false,
            from_start: false,
            samples: 0,
            start_s: null,
            end_s: null,
            message: String(e),
          });
      } finally {
        pending = false;
      }
    };
    void refresh();
    const timer = window.setInterval(() => {
      void refresh();
    }, 1000);
    return () => {
      polling = false;
      window.clearInterval(timer);
    };
  }, [accepting, desktop]);

  async function open(file: File) {
    const scope = imports.current;
    const revision = scope.revision;
    const request = ++scope.request;
    const source = engine;
    const current = () =>
      scope.active &&
      scope.revision === revision &&
      scope.request === request &&
      currentEngine() === source;
    setBusy("Opening recording");
    setError("");
    try {
      const recording = await openRecording(file);
      if (!current()) return;
      onOpen(new PlaybackEngine(recording));
    } catch (e) {
      if (current()) setError(String(e));
    } finally {
      if (scope.active && scope.request === request) setBusy("");
    }
  }
  async function start() {
    setBusy("Starting capture");
    setError("");
    setSavedPath("");
    try {
      setCapture(await startCapture());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy("");
    }
  }
  async function save() {
    setBusy("Saving captured segment to Downloads");
    setError("");
    try {
      const saved = await saveCapture(filename.trim());
      setSavedPath(saved.path);
      setCapture(await captureStatus());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy("");
    }
  }

  const unavailable = !desktop ? "Use the desktop app for recording files" : busy || undefined;
  const captureActive = capture?.state === "capturing" || capture?.state === "ready";
  return (
    <div className="min-h-0 overflow-y-auto">
      <input
        ref={input}
        type="file"
        accept=".eucrec"
        className="hidden"
        aria-label="Select EUC Studio recording"
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          event.currentTarget.value = "";
          if (file) void open(file);
        }}
      />
      <Tabs
        value={tab}
        onValueChange={setTab}
        aria-label="Recording actions"
        className="px-24 pt-8"
        tabs={[
          { value: "playback", label: "Playback" },
          {
            value: "capture",
            label: "Capture",
            ...(capture?.state === "capturing" ? { count: 1 } : {}),
          },
        ]}
      >
        <TabPanel value="playback" className="flex flex-col gap-24 py-24 text-13 outline-none">
          {playback ? (
            <>
              <div className="flex items-start justify-between gap-16">
                <div className="min-w-0">
                  <p
                    className="truncate text-15 font-semibold text-fg"
                    title={playback.recording.name}
                  >
                    {playback.recording.name}
                  </p>
                  <p className="mt-4 text-12 text-fg-2">
                    {playback.recording.header.wheel} · {playback.recording.header.firmware}
                  </p>
                </div>
                <Pill tone="info">Playback</Pill>
              </div>
              <Slider
                label="Position"
                unit="s"
                decimals={2}
                value={playback.position}
                min={playback.recording.start}
                max={Math.max(playback.recording.end, playback.recording.start + 0.001)}
                step={0.001}
                onValueChange={(time) => playback.seek(time)}
                disabledReason={
                  playback.recording.end === playback.recording.start
                    ? "This recording contains one sample"
                    : undefined
                }
              />
              <div className="flex flex-wrap items-center gap-8">
                <Button
                  variant="primary"
                  icon={playback.playing ? <Pause /> : <Play />}
                  onClick={() => playback.togglePlaying()}
                >
                  {playback.playing ? "Pause" : "Play"}
                </Button>
                <Button icon={<RotateCcw />} onClick={() => playback.restart()}>
                  Restart
                </Button>
                <Button
                  icon={<StepForward />}
                  onClick={() => playback.step()}
                  disabledReason={playback.playing ? "Pause playback before stepping" : undefined}
                >
                  Step
                </Button>
                <Select
                  aria-label="Playback speed"
                  value={String(playback.targetRtf)}
                  onValueChange={(value) => playback.setTargetRtf(Number(value))}
                  options={[0.25, 0.5, 1, 2, 4, 8].map((value) => ({
                    value: String(value),
                    label: `${value}×`,
                  }))}
                />
              </div>
              {playback.recording.truncated && (
                <p role="status" className="text-caution">
                  Partial recording: {playback.recording.truncated}
                </p>
              )}
              {playback.recording.droppedSamples > 0 && (
                <p role="status" className="text-caution">
                  Partial recording. The source discarded{" "}
                  {playback.recording.droppedSamples.toLocaleString()} samples.
                </p>
              )}
              {playback.recording.start > 1 / playback.recording.header.sample_hz && (
                <p className="text-caution">
                  The recording starts after the run began. Earlier samples are unavailable.
                </p>
              )}
              <details className="text-12 text-fg-2">
                <summary className="cursor-pointer rounded-4 hover:text-fg">
                  Recording details
                </summary>
                <dl className="mt-16 grid grid-cols-2 gap-8">
                  <dt>Rider model</dt>
                  <dd className="text-right">
                    {RIDER_MODELS.find(
                      (model) => model.value === playback.recording.header.context?.rider_model,
                    )?.label ?? "Not recorded"}
                  </dd>
                  <dt>Samples</dt>
                  <dd className="num text-right">
                    {playback.recording.times.length.toLocaleString()}
                  </dd>
                  <dt>Interval</dt>
                  <dd className="num text-right">
                    {playback.recording.start.toFixed(2)}–{playback.recording.end.toFixed(2)} s
                  </dd>
                </dl>
                <p className="mt-16">
                  Playback shows recorded telemetry, body poses, events, phone frames, and board
                  outputs. Missing tracks stay unavailable. Audio playback is not available.
                </p>
              </details>
            </>
          ) : (
            <div className="flex flex-col items-center gap-8 py-16 text-center">
              <ScrollText className="mb-8 size-[32px] text-fg-3" />
              <h3 className="text-15 font-semibold">Review a recorded run</h3>
              <p className="max-w-[280px] text-13 text-fg-2">
                Open an .eucrec file to inspect its telemetry, movement, and events.
              </p>
            </div>
          )}
          <div className="flex flex-wrap gap-8 border-t border-hairline pt-16">
            <Button
              ref={openButtonRef}
              variant={playback ? "secondary" : "primary"}
              icon={<ScrollText />}
              onClick={() => input.current?.click()}
              disabledReason={
                unavailable ||
                (capture?.state === "capturing"
                  ? "Save the captured segment before opening a file"
                  : undefined)
              }
            >
              Open recording
            </Button>
            {playback && (
              <Button variant="ghost" onClick={onClose} disabledReason={busy || undefined}>
                Return to simulation
              </Button>
            )}
          </div>
        </TabPanel>
        <TabPanel value="capture" className="flex flex-col gap-24 py-24 text-13 outline-none">
          <div className="flex items-start justify-between gap-16">
            <div>
              <h3 className="text-15 font-semibold">Capture this run</h3>
              <p className="mt-4 text-13 text-fg-2">Save telemetry and movement to Downloads.</p>
            </div>
            <Tooltip content="Capture starts at the next sample. Limits: 600 seconds, 72,000 samples, 32 MiB, and 2,000,000 signal and pose values.">
              <button
                type="button"
                aria-label="Capture limits"
                className="rounded-4 text-fg-2 hover:text-fg"
              >
                <Info size={16} />
              </button>
            </Tooltip>
          </div>
          {captureActive && (
            <div className="flex items-center justify-between gap-16">
              <Pill
                tone={capture.state === "capturing" ? "accent" : "neutral"}
                dot={capture.state === "capturing" ? "live" : true}
              >
                {capture.state === "capturing" ? "Capturing" : "Ready to save"}
              </Pill>
              <span className="num text-12 text-fg-2">
                {((capture.end_s ?? 0) - (capture.start_s ?? 0)).toFixed(1)} s ·{" "}
                {capture.samples.toLocaleString()} samples
              </span>
            </div>
          )}
          {capture?.message && (
            <p
              role="status"
              className={capture.state === "error" ? "text-danger-text" : "text-fg-2"}
            >
              {capture.message}
            </p>
          )}
          <label className="flex flex-col gap-8 text-12 text-fg-2">
            File name
            <input
              value={filename}
              onChange={(event) => setFilename(event.currentTarget.value)}
              maxLength={128}
              className="h-(--control-md) min-w-0 rounded-8 border border-hairline bg-raised px-8 text-13 text-fg"
            />
          </label>
          <div className="flex gap-8">
            <Button
              variant={captureActive ? "secondary" : "primary"}
              onClick={() => void start()}
              disabledReason={
                unavailable ||
                (engine.kind !== "live"
                  ? "Live simulation is required for capture"
                  : captureActive
                    ? "Save the current captured segment first"
                    : !capture?.can_start
                      ? capture?.message || "Waiting for capture status"
                      : undefined)
              }
            >
              Start capture
            </Button>
            <Button
              variant={captureActive ? "primary" : "secondary"}
              onClick={() => void save()}
              disabledReason={
                unavailable ||
                (!captureActive
                  ? "Start a capture before saving"
                  : capture.samples === 0
                    ? "Wait for the first captured sample"
                    : undefined)
              }
            >
              Stop and save
            </Button>
          </div>
          {savedPath && (
            <p role="status" className="break-words text-ok">
              Saved: {savedPath}
            </p>
          )}
        </TabPanel>
      </Tabs>
      {(busy || error) && (
        <div className="border-t border-hairline px-24 py-16 text-13">
          {busy && (
            <p role="status" className="text-fg-2">
              {busy}
            </p>
          )}
          {error && (
            <p role="alert" className="break-words text-danger-text">
              {error}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
