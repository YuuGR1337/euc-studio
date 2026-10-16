import { useState } from "react";
import { series } from "../design/tokens.ts";
import { useUi } from "../state/ui.ts";
import { Button } from "../ui/Button.tsx";
import {
  Activity,
  ArrowRight,
  Bluetooth,
  ChipWrite,
  Crosshair,
  Diverge,
  Eye,
  FileCode2,
  Ghost,
  Lock,
  ScrollText,
  ShieldCheck,
  SlidersHorizontal,
  Unicycle,
} from "../ui/icons.tsx";
import { Pill } from "../ui/Pill.tsx";
import { Toggle } from "../ui/Toggle.tsx";
import { EmptyWorkspace } from "./EmptyWorkspace.tsx";

function GoToRide({ label }: { label: string }) {
  const navigate = useUi((s) => s.navigate);
  return (
    <Button variant="secondary" icon={<Unicycle />} onClick={() => navigate("ride")}>
      {label}
    </Button>
  );
}

/* ---------------------------------------------------------------- Tune */

const VARS = [
  { name: "attitude.pitch", value: "+1.4", unit: "°", bar: 0.55 },
  { name: "balance.p_term", value: "0.218", unit: "", bar: 0.36 },
  { name: "motor.duty", value: "0.34", unit: "", bar: 0.34 },
  { name: "tiltback.limit", value: "35.0", unit: "km/h", bar: 0.7 },
  { name: "adc.phase_b", value: "2047", unit: "lsb", bar: 0.5 },
];

function TunePreview() {
  const [quirks, setQuirks] = useState({ ftz: true, cmd: true });
  return (
    <div className="flex flex-col">
      <div className="flex h-(--dock-header) items-center border-b border-hairline px-16">
        <span className="label">Inspector</span>
        <span className="ml-auto text-12 text-fg-2">Ours-clean</span>
      </div>
      <ul className="px-8 py-8">
        {VARS.map((v) => (
          <li
            key={v.name}
            className="grid h-[32px] grid-cols-[1fr_72px_80px] items-center gap-16 rounded-8 px-8 hover:bg-raised"
          >
            <span className="num truncate text-12 text-fg">{v.name}</span>
            <span className="relative h-[4px] overflow-hidden rounded-pill bg-track">
              <span
                className="absolute inset-y-0 left-0 rounded-pill bg-accent"
                style={{ width: `${v.bar * 100}%` }}
              />
            </span>
            <span className="num text-right text-12 text-fg-2">
              {v.value}
              {v.unit && <span className="ml-4 text-fg-3">{v.unit}</span>}
            </span>
          </li>
        ))}
      </ul>
      <div className="flex flex-col gap-16 border-t border-hairline px-16 py-16">
        <span className="label">Quirks</span>
        <Toggle
          label={<span className="num text-12">KEIL_FTZ</span>}
          description="Flush denormals to zero like the stock build"
          checked={quirks.ftz}
          onCheckedChange={(ftz) => setQuirks((q) => ({ ...q, ftz }))}
        />
        <Toggle
          label={<span className="num text-12">CMD_LAST_BYTE_WINS</span>}
          description="Stock parser keeps only the last command byte"
          checked={quirks.cmd}
          onCheckedChange={(cmd) => setQuirks((q) => ({ ...q, cmd }))}
        />
      </div>
    </div>
  );
}

export function TuneWorkspace() {
  return (
    <EmptyWorkspace
      icon={<SlidersHorizontal />}
      name="Tune"
      milestone="Arrives in M3"
      title="See inside the firmware while it rides"
      lede="Read the firmware's own variables as it runs, pin them to scopes, and switch one compat quirk at a time to see what it changes."
      features={[
        {
          icon: <Eye />,
          title: "Inspector",
          text: "Named variables and register bit fields with units. Poke a value or pin it to a scope.",
        },
        {
          icon: <Activity />,
          title: "Scopes with triggers",
          text: "Synced cursors, edge and threshold triggers, and ISR load per handler.",
        },
        {
          icon: <SlidersHorizontal />,
          title: "Quirk toggles",
          text: "Ours-clean runs with a runtime quirk mask, so each fix gets its own evidence.",
        },
      ]}
      actions={<GoToRide label="Watch the demo ride" />}
      previewLabel="Preview. Names and values are illustrative until the inspector reads real symbols."
      preview={<TunePreview />}
    />
  );
}

/* ---------------------------------------------------------------- Compare */

function trace(seed: number, diverge: number) {
  const pts: string[] = [];
  for (let i = 0; i <= 100; i++) {
    const x = i * 4;
    const base = 70 + 22 * Math.sin(i / 9) + 8 * Math.sin(i / 3.1);
    const off = i > diverge ? (i - diverge) ** 1.35 * 0.55 * seed : 0;
    pts.push(`${x},${(base + off).toFixed(1)}`);
  }
  return pts.join(" ");
}

function ComparePreview() {
  const d = 58;
  return (
    <div className="flex flex-col">
      <div className="flex h-(--dock-header) items-center gap-8 border-b border-hairline px-16">
        <span className="label">Pitch, lock-step</span>
        <span className="ml-auto flex items-center gap-8 text-12 text-fg-2">
          <span className="size-[8px] rounded-pill" style={{ background: series.speed }} /> Stock
          <span className="ml-8 size-[8px] rounded-pill" style={{ background: series.pwm }} />{" "}
          Ours-clean
        </span>
      </div>
      <svg
        viewBox="0 0 400 150"
        className="w-full"
        role="img"
        aria-label="Two pitch traces that match, then diverge"
      >
        {[30, 70, 110].map((y) => (
          <line key={y} x1="0" x2="400" y1={y} y2={y} stroke="rgba(255,255,255,0.06)" />
        ))}
        <polyline
          points={trace(1, d)}
          fill="none"
          stroke={series.speed}
          strokeWidth="1.5"
          strokeLinejoin="round"
        />
        <polyline
          points={trace(-1, d)}
          fill="none"
          stroke={series.pwm}
          strokeWidth="1.5"
          strokeLinejoin="round"
        />
        <line
          x1={d * 4}
          x2={d * 4}
          y1="8"
          y2="142"
          stroke="var(--color-danger)"
          strokeDasharray="3 3"
        />
        <circle cx={d * 4} cy="8" r="3" fill="var(--color-danger)" />
      </svg>
      <div className="flex items-center gap-8 border-t border-hairline px-16 py-8">
        <Pill tone="danger" dot>
          First divergence
        </Pill>
        <span className="num text-12 text-fg-2">t = 12.418 s</span>
        <span className="num ml-auto truncate text-12 text-fg-3">cause: KEIL_FTZ quirk</span>
      </div>
    </div>
  );
}

export function CompareWorkspace() {
  return (
    <EmptyWorkspace
      icon={<Diverge />}
      name="Compare"
      milestone="Arrives in M4"
      title="Find where two firmwares part ways"
      lede="Run stock and ours on the same scenario, seed and inputs. The studio marks the first sample where they differ and shows both wheels at once."
      features={[
        {
          icon: <Crosshair />,
          title: "Lock-step runs",
          text: "Same scenario and seed for both sides; register writes are hashed every quantum.",
        },
        {
          icon: <Ghost />,
          title: "3D ghost",
          text: "The second firmware rides as a translucent ghost next to the first.",
        },
        {
          icon: <Diverge />,
          title: "First divergence",
          text: "A marker on every scope and on the timeline, with the instruction that caused it.",
        },
      ]}
      actions={
        <>
          <Button
            variant="primary"
            icon={<Diverge />}
            disabledReason="Needs our firmware running in the simulator (M4)"
          >
            Start a comparison
          </Button>
          <GoToRide label="Watch the demo ride" />
        </>
      }
      previewLabel="Preview with illustrative traces."
      preview={<ComparePreview />}
    />
  );
}

/* ---------------------------------------------------------------- Flash */

const GATES = [
  { icon: <FileCode2 />, title: "Image check", text: "Size, vector table and link address" },
  { icon: <ShieldCheck />, title: "Provenance", text: "Pinned stock hash or our signed image" },
  { icon: <Unicycle />, title: "Sim dry-run", text: "Same image boots and rides 30 s here first" },
  { icon: <Bluetooth />, title: "Identity", text: "The wheel answers like a Begode Extreme" },
  {
    icon: <Activity />,
    title: "Wheel state",
    text: "Standing still, no alarms, battery and heat OK",
  },
  { icon: <Lock />, title: "Typed confirmation", text: "You type FLASH EXTREME to start" },
  {
    icon: <ChipWrite />,
    title: "Stock first",
    text: "The first flash on any wheel is the stock image",
  },
];

function FlashPreview() {
  return (
    <div className="flex flex-col">
      <div className="flex h-(--dock-header) items-center border-b border-hairline px-16">
        <span className="label">Safety gates</span>
        <span className="ml-auto text-12 text-fg-2">All must pass</span>
      </div>
      <ol className="flex flex-col px-8 py-8">
        {GATES.map((g, i) => (
          <li
            key={g.title}
            className="grid h-[40px] grid-cols-[24px_1fr_auto] items-center gap-8 rounded-8 px-8 hover:bg-raised"
          >
            <span className="num grid size-[20px] place-items-center rounded-pill border border-hairline-strong text-11 text-fg-2">
              {i + 1}
            </span>
            <span className="flex min-w-0 flex-col">
              <span className="truncate text-13 font-semibold text-fg">{g.title}</span>
              <span className="truncate text-12 text-fg-2">{g.text}</span>
            </span>
            <span className="text-fg-3 [&_svg]:size-[14px]">{g.icon}</span>
          </li>
        ))}
      </ol>
    </div>
  );
}

export function FlashWorkspace() {
  return (
    <EmptyWorkspace
      icon={<ChipWrite />}
      name="Flash"
      milestone="Arrives in M6"
      title="Write firmware to a wheel, safely"
      lede="The flasher talks to the wheel's bootloader over Bluetooth and will not start until every gate passes. Every byte sent is logged."
      features={[
        {
          icon: <ShieldCheck />,
          title: "Seven gates",
          text: "Checked in order and logged; one failure stops the flash before a byte is sent.",
        },
        {
          icon: <ScrollText />,
          title: "Byte log",
          text: "A JSONL record of the session, ready to attach to a bug report.",
        },
        {
          icon: <ArrowRight />,
          title: "Recovery path",
          text: "If anything fails, restore the official image; if that fails, the SWD guide.",
        },
      ]}
      actions={
        <>
          <Button
            variant="primary"
            icon={<Bluetooth />}
            disabledReason="Ships in M6, after 100 simulated flashes end in success or a recoverable state"
          >
            Connect a wheel
          </Button>
          <GoToRide label="Watch the demo ride" />
        </>
      }
      previewLabel="The gates follow the rules in SAFETY.md."
      preview={<FlashPreview />}
    />
  );
}
