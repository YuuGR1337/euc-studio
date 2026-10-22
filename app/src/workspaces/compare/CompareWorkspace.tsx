import { AnimatePresence, motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { smooth } from "../../design/motion.ts";
import {
  type CompareFirmware,
  type CompareFirmwareId,
  compare,
  useCompareVersion,
} from "../../engine/compare.ts";
import { cx } from "../../lib/cx.ts";
import { readStored, writeStored } from "../../lib/storage.ts";
import { DockLayout } from "../../shell/DockLayout.tsx";
import { Button } from "../../ui/Button.tsx";
import { Diverge, Play, RotateCcw, X } from "../../ui/icons.tsx";
import { Pill } from "../../ui/Pill.tsx";
import { SegmentedControl } from "../../ui/SegmentedControl.tsx";
import { Select, type SelectOption } from "../../ui/Select.tsx";
import { Tooltip } from "../../ui/Tooltip.tsx";
import { Wait } from "../../ui/Wait.tsx";
import { LOW_FPS, type Quality } from "../ride/quality.ts";
import { normalizeQuirks, profileQuirks } from "../../engine/quirkMask.ts";
import { CompareDetails, type SideQuirks } from "./CompareDetails.tsx";
import { CompareScene } from "./CompareScene.tsx";
import { CompareScopes } from "./CompareScopes.tsx";
import { divergenceText, formatTime, overallVerdict } from "./summaryFormat.ts";
import { installRideGate } from "./rideGate.ts";

type Pace = "real" | "fast";
type Picked = { name: string; bytes: number[] } | null;

interface Setup {
  a: CompareFirmwareId;
  b: CompareFirmwareId;
  scenario: string;
  pace: Pace;
  /** The quirks each side keeps when it runs "Ours (custom quirks)". */
  quirks: SideQuirks;
}

/** What the run uses for a side: a built-in firmware, our firmware with its quirks, or a file. */
function firmwareOf(id: CompareFirmwareId, file: Picked, quirks: string[]): CompareFirmware | null {
  if (id === "ours-custom") return { kind: id, quirks: normalizeQuirks(quirks) };
  if (id !== "file") return { kind: id };
  return file ? { kind: "file", name: file.name, bytes: file.bytes } : null;
}

/** A side's custom quirks from a stored or requested list (compat when there is none). */
function quirksOf(value: unknown): string[] {
  return Array.isArray(value)
    ? normalizeQuirks(value.filter((q): q is string => typeof q === "string"))
    : profileQuirks("compat");
}

function initialSetup(): Setup {
  const stored = readStored<Partial<Setup>>("compare.setup", {});
  const quirks = { a: quirksOf(stored.quirks?.a), b: quirksOf(stored.quirks?.b) };
  const r = compare.request;
  if (r) {
    return {
      a: r.a.kind,
      b: r.b.kind,
      scenario: r.scenario,
      pace: r.fast ? "fast" : "real",
      quirks: {
        a: r.a.kind === "ours-custom" ? quirksOf(r.a.quirks) : quirks.a,
        b: r.b.kind === "ours-custom" ? quirksOf(r.b.quirks) : quirks.b,
      },
    };
  }
  return {
    a: stored.a && stored.a !== "file" ? stored.a : "stock",
    b: stored.b && stored.b !== "file" ? stored.b : "ours-compat",
    scenario: stored.scenario ?? "",
    pace: stored.pace === "fast" ? "fast" : "real",
    quirks,
  };
}

/** One side's firmware choice; "File…" opens the system's file picker. */
function FirmwarePicker({
  side,
  value,
  file,
  onChange,
}: {
  side: "A" | "B";
  value: CompareFirmwareId;
  file: Picked;
  onChange: (id: CompareFirmwareId, file?: Picked) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const options: SelectOption<CompareFirmwareId>[] = [
    ...compare.firmwares().map((f) => ({
      value: f.id,
      label: f.label,
      detail: f.detail,
      disabledReason: f.disabledReason,
    })),
    {
      value: "file",
      label: file ? file.name : "File…",
      detail: "A .bin or .elf image (an .elf names our variables)",
      disabledReason:
        compare.kind === "demo" ? "Loading a firmware file needs the desktop app" : undefined,
    },
  ];
  return (
    <span className="flex items-center gap-8">
      <span
        className={cx(
          "num grid size-[20px] shrink-0 place-items-center rounded-pill text-11 font-semibold",
          side === "A" ? "bg-fg text-bg" : "border border-dashed border-fg-2 text-fg-2",
        )}
        aria-hidden="true"
      >
        {side}
      </span>
      <Select<CompareFirmwareId>
        aria-label={`Firmware ${side}`}
        value={value}
        onValueChange={(id) => (id === "file" ? input.current?.click() : onChange(id))}
        options={options}
        className="w-[144px]"
        contentClassName="w-[300px]"
      />
      <input
        ref={input}
        type="file"
        accept=".bin,.elf"
        className="hidden"
        aria-hidden="true"
        tabIndex={-1}
        onChange={async (e) => {
          const f = e.target.files?.[0];
          e.target.value = "";
          if (!f) return;
          const bytes = Array.from(new Uint8Array(await f.arrayBuffer()));
          onChange("file", { name: f.name, bytes });
        }}
      />
    </span>
  );
}

function SetupBar({
  setup,
  files,
  onSetup,
  onFile,
}: {
  setup: Setup;
  files: { a: Picked; b: Picked };
  onSetup: (s: Partial<Setup>) => void;
  onFile: (side: "a" | "b", file: Picked) => void;
}) {
  const scenarios = compare.scenarios();
  const scenario = scenarios.some((s) => s.id === setup.scenario)
    ? setup.scenario
    : (scenarios[0]?.id ?? "");
  const running = compare.status.state === "starting" || compare.status.state === "running";
  const a = firmwareOf(setup.a, files.a, setup.quirks.a);
  const b = firmwareOf(setup.b, files.b, setup.quirks.b);
  const disabled = (id: CompareFirmwareId) =>
    compare.firmwares().find((f) => f.id === id)?.disabledReason;
  const why = !scenario
    ? "No scenario to ride"
    : !a || !b
      ? "Pick a file for the side set to File"
      : (disabled(setup.a) ?? disabled(setup.b));
  const run = () => {
    if (!a || !b) return;
    compare.start({ a, b, scenario, fast: setup.pace === "fast" });
  };
  return (
    <div className="flex h-[52px] shrink-0 items-center gap-8 border-b border-hairline px-16">
      <FirmwarePicker
        side="A"
        value={setup.a}
        file={files.a}
        onChange={(id, file) => {
          if (file !== undefined) onFile("a", file);
          onSetup({ a: id });
        }}
      />
      <span className="text-12 font-semibold text-fg-3">vs</span>
      <FirmwarePicker
        side="B"
        value={setup.b}
        file={files.b}
        onChange={(id, file) => {
          if (file !== undefined) onFile("b", file);
          onSetup({ b: id });
        }}
      />
      <span className="mx-8 h-[24px] w-px bg-hairline" />
      <Select<string>
        aria-label="Compare scenario"
        value={scenario}
        onValueChange={(s) => onSetup({ scenario: s })}
        options={scenarios.map((s) => ({ value: s.id, label: s.label, detail: s.detail }))}
        className="max-w-[260px] min-w-[120px] flex-1"
        contentClassName="w-[300px]"
      />
      <SegmentedControl<Pace>
        aria-label="Pace"
        size="sm"
        value={setup.pace}
        onValueChange={(pace) => onSetup({ pace })}
        options={[
          { value: "real", label: "1×", tooltip: "Both runs in real time" },
          { value: "fast", label: "Max", tooltip: "As fast as this machine runs both" },
        ]}
      />
      <span className="min-w-0 flex-1" />
      {running ? (
        <Button
          variant="secondary"
          icon={<X />}
          onClick={() => compare.stop()}
          className="w-[96px]"
        >
          Stop
        </Button>
      ) : (
        <Button
          variant="primary"
          icon={<Play />}
          onClick={run}
          disabledReason={why}
          className="w-[96px]"
        >
          Run
        </Button>
      )}
    </div>
  );
}

function HudCard({
  children,
  className,
  testId,
}: {
  children: React.ReactNode;
  className?: string;
  testId?: string;
}) {
  return (
    <div
      data-testid={testId}
      className={cx(
        "rounded-12 border border-hairline bg-card/75 backdrop-blur-[12px] backdrop-saturate-150",
        className,
      )}
    >
      {children}
    </div>
  );
}

/** Where B rides against A, along the road ("B 0.29 m behind"). */
function gapText(): string {
  const gap = compare.b.latest.distance - compare.a.latest.distance;
  if (!compare.a.info || !Number.isFinite(gap)) return "";
  if (Math.abs(gap) < 0.005) return "B level with A";
  return `B ${Math.abs(gap).toFixed(2)} m ${gap > 0 ? "ahead" : "behind"}`;
}

/** Who rides: A solid, B the ghost, and how far apart they are. */
function RunsCard() {
  const rows = [
    { side: "A", run: compare.a, how: "solid" },
    { side: "B", run: compare.b, how: "ghost" },
  ] as const;
  return (
    <HudCard className="flex w-[264px] flex-col gap-4 px-16 py-8" testId="compare-runs">
      {rows.map(({ side, run, how }) => (
        <span key={side} className="flex h-[24px] items-center gap-8">
          <span
            className={cx(
              "num grid size-[18px] shrink-0 place-items-center rounded-pill text-11 font-semibold",
              side === "A" ? "bg-fg text-bg" : "border border-dashed border-info text-info",
            )}
          >
            {side}
          </span>
          <span className="truncate text-13 font-semibold text-fg">{run.info?.label ?? "–"}</span>
          <span className="ml-auto text-11 text-fg-2">{how}</span>
        </span>
      ))}
      <span className="num h-[16px] text-11 text-fg-2" data-testid="compare-gap">
        {gapText()}
      </span>
    </HudCard>
  );
}

/** How far the runs have come. */
function ProgressCard() {
  const { t, duration, rtf, state } = compare.status;
  const share = duration > 0 ? Math.min(1, t / duration) : 0;
  const tone = state === "done" ? "ok" : state === "error" ? "danger" : "neutral";
  return (
    <HudCard className="flex w-[216px] flex-col gap-8 px-16 py-8">
      <span className="flex items-center justify-between">
        <span className="label">Progress</span>
        <Pill tone={tone} dot={state === "running" ? "live" : true}>
          {state}
        </Pill>
      </span>
      <span className="num flex items-baseline justify-between text-12 text-fg">
        <span>
          {t.toFixed(1)} / {duration.toFixed(1)} s
        </span>
        <span className="text-fg-2">{rtf > 0 ? `${rtf.toFixed(2)}×` : ""}</span>
      </span>
      <span className="relative h-[4px] overflow-hidden rounded-pill bg-track">
        <span
          className="absolute inset-y-0 left-0 rounded-pill bg-accent transition-[width] duration-(--dur-data) ease-data"
          style={{ width: `${share * 100}%` }}
        />
      </span>
    </HudCard>
  );
}

function DivergenceCard() {
  const summary = compare.summary;
  const first = summary?.first_divergence;
  const verdict = overallVerdict(summary);
  if (!summary) return null;
  return (
    <HudCard className="flex w-[300px] flex-col gap-4 px-16 py-8">
      <span className="flex items-center justify-between">
        <span className="label">First divergence</span>
        <Pill tone={verdict.tone}>{verdict.label}</Pill>
      </span>
      <span className="num text-20 font-medium text-fg">
        {first ? formatTime(first.t_s, 3) : "none"}
      </span>
      <span className="truncate text-12 text-fg-2">
        {first ? divergenceText(first) : "The runs agree so far"}
      </span>
    </HudCard>
  );
}

/** Before the first run, while the runs build, and after a failure. */
function Overlay({ onRun }: { onRun: () => void }) {
  const { state, message } = compare.status;
  const idle = state === "idle";
  const show = idle || state === "starting" || state === "error";
  return (
    <AnimatePresence>
      {show && (
        <motion.div
          className="absolute inset-0 z-20 grid place-items-center bg-bg/70 backdrop-blur-[2px]"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={smooth}
        >
          {state === "starting" ? (
            <Wait
              step={message || "Starting the comparison"}
              detail="Both firmwares boot in their own simulated controller"
              onRetry={onRun}
              onCancel={() => compare.stop()}
            />
          ) : (
            <div className="flex w-[380px] flex-col gap-8 rounded-12 border border-hairline-strong bg-card p-16">
              <h2 className="flex items-center gap-8 text-13 font-semibold text-fg">
                <Diverge className="size-[16px] text-accent-text" />
                {idle ? "Find where two firmwares part ways" : "The comparison stopped"}
              </h2>
              <span className="text-12 break-words text-fg-2">
                {idle
                  ? "Both firmwares ride the same scenario on the same inputs. Run B rides as a ghost over run A; the scopes and the summary mark the first sample where they differ."
                  : message}
              </span>
              {!idle && (
                <div className="flex gap-8 pt-8">
                  <Button size="sm" icon={<RotateCcw />} onClick={onRun}>
                    Run again
                  </Button>
                </div>
              )}
            </div>
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}

const SCENE_OFF =
  typeof location !== "undefined" && new URLSearchParams(location.search).get("scene") === "off";

function CompareViewport({ active, onRun }: { active: boolean; onRun: () => void }) {
  useCompareVersion();
  const [quality, setQuality] = useState<Quality | null>(null);
  const [drawn, setDrawn] = useState(false);
  return (
    <div
      className="relative min-h-0 flex-1 overflow-hidden"
      data-testid="compare-viewport"
      data-scene={SCENE_OFF ? "off" : drawn ? "ready" : "building"}
    >
      <div className="absolute inset-0">
        {SCENE_OFF ? (
          <p className="grid h-full place-items-center text-12 text-fg-3">3D view off</p>
        ) : (
          <CompareScene active={active} onReady={setQuality} onFirstFrame={() => setDrawn(true)} />
        )}
      </div>
      <Overlay onRun={onRun} />
      <div className="pointer-events-none absolute inset-0 flex flex-col justify-between p-16">
        <div className="flex items-start justify-between">
          <RunsCard />
          <ProgressCard />
        </div>
        <div className="flex items-end justify-between">
          <DivergenceCard />
          {quality === "low" && (
            <Tooltip
              side="top"
              content={`No GPU acceleration. The 3D view draws at ${LOW_FPS} fps without shadows; the comparison is unaffected.`}
            >
              <span className="pointer-events-auto">
                <Pill tone="neutral" size="md">
                  Reduced 3D quality
                </Pill>
              </span>
            </Tooltip>
          )}
        </div>
      </div>
    </div>
  );
}

/** ⌘3: two firmwares on one scenario, A against B. */
export function CompareWorkspace({ active }: { active: boolean }) {
  useCompareVersion();
  // The workspace stays mounted once visited, so the gate holds from the first visit on.
  useEffect(() => installRideGate(), []);
  const [setup, setSetupState] = useState<Setup>(initialSetup);
  const [files, setFiles] = useState<{ a: Picked; b: Picked }>({ a: null, b: null });
  const onSetup = (s: Partial<Setup>) =>
    setSetupState((prev) => {
      const next = { ...prev, ...s };
      writeStored("compare.setup", next);
      return next;
    });
  const rerun = () => {
    const r = compare.request;
    if (r) compare.start(r);
  };
  return (
    <DockLayout
      workspace="compare"
      center={
        <div className="flex h-full flex-col overflow-hidden rounded-12 border border-hairline bg-bg">
          <SetupBar
            setup={setup}
            files={files}
            onSetup={onSetup}
            onFile={(side, file) => setFiles((f) => ({ ...f, [side]: file }))}
          />
          <CompareViewport active={active} onRun={rerun} />
        </div>
      }
      right={<CompareScopes />}
      bottom={
        <CompareDetails
          a={setup.a}
          b={setup.b}
          quirks={setup.quirks}
          onQuirks={(side, ids) => onSetup({ quirks: { ...setup.quirks, [side]: ids } })}
        />
      }
    />
  );
}
