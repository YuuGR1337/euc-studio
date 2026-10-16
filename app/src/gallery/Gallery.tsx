import { spring } from "motion";
import { motion } from "motion/react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { bouncy, data as dataT, smooth, snappy } from "../design/motion.ts";
import { contrast } from "../design/color.ts";
import {
  MIN_RATIO,
  accents,
  type AccentName,
  contrastPairs,
  hover,
  motion as motionTokens,
  radius,
  semantic,
  series,
  spacing,
  surface,
  surfaceStep,
  text,
  typeScale,
} from "../design/tokens.ts";
import { cx } from "../lib/cx.ts";
import { useUi } from "../state/ui.ts";
import { Button, IconButton } from "../ui/Button.tsx";
import { type PaletteCommand, PaletteBody } from "../ui/CommandPalette.tsx";
import { Gauge } from "../ui/Gauge.tsx";
import {
  Activity,
  Camera,
  Check,
  Cpu,
  Keyboard,
  LayoutGrid,
  PanelBottom,
  PanelRight,
  Pause,
  Play,
  RefreshCw,
  RotateCcw,
  Timer,
  Unicycle,
  Zap,
} from "../ui/icons.tsx";
import { Kbd, Shortcut } from "../ui/Kbd.tsx";
import { Panel } from "../ui/Panel.tsx";
import { Pill } from "../ui/Pill.tsx";
import { RollingNumber } from "../ui/RollingNumber.tsx";
import { SegmentedControl } from "../ui/SegmentedControl.tsx";
import { Select } from "../ui/Select.tsx";
import { Slider } from "../ui/Slider.tsx";
import { Sparkline } from "../ui/Sparkline.tsx";
import { Tabs } from "../ui/Tabs.tsx";
import { ToastCard } from "../ui/Toast.tsx";
import { toast } from "../ui/toast-store.ts";
import { Toggle } from "../ui/Toggle.tsx";
import { TooltipBubble } from "../ui/Tooltip.tsx";
import { Wait } from "../ui/Wait.tsx";
import { PitchProtractor } from "../workspaces/ride/PitchProtractor.tsx";

const SECTIONS = [
  ["color", "Color"],
  ["contrast", "Contrast"],
  ["type", "Type"],
  ["space", "Space and radius"],
  ["motion", "Motion"],
  ["buttons", "Buttons"],
  ["controls", "Controls"],
  ["feedback", "Feedback"],
  ["data", "Data"],
  ["overlays", "Overlays"],
] as const;

type SectionId = (typeof SECTIONS)[number][0];

/* ------------------------------------------------------------------ layout helpers */

function Section({
  id,
  title,
  lede,
  children,
}: {
  id: SectionId;
  title: string;
  lede: string;
  children: ReactNode;
}) {
  return (
    <section
      id={id}
      data-section={id}
      className="scroll-mt-24 border-t border-hairline pt-32 pb-48 first:border-t-0 first:pt-0"
    >
      <h2 className="text-20 font-semibold text-fg">{title}</h2>
      <p className="mt-4 max-w-[640px] text-13 text-fg-2">{lede}</p>
      <div className="mt-24 flex flex-col gap-32">{children}</div>
    </section>
  );
}

function Group({
  title,
  children,
  className,
}: {
  title: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div>
      <h3 className="mb-16 text-13 font-semibold text-fg">{title}</h3>
      <div className={className}>{children}</div>
    </div>
  );
}

/** One state of a component, captioned. */
function State({
  label,
  children,
  className,
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx("flex flex-col items-start gap-8", className)}>
      <div className="flex min-h-[40px] items-center">{children}</div>
      <span className="text-11 text-fg-3">{label}</span>
    </div>
  );
}

const STATES = ["rest", "hover", "press", "focus", "disabled"] as const;
type StateName = (typeof STATES)[number];
const force = (s: StateName) => (s === "rest" || s === "disabled" ? undefined : s);

/* ------------------------------------------------------------------ color */

function Swatch({
  name,
  value,
  note,
  onClick,
  selected,
}: {
  name: string;
  value: string;
  note?: string;
  onClick?: () => void;
  selected?: boolean;
}) {
  const content = (
    <>
      <span
        className="block h-[56px] rounded-8 border border-hairline"
        style={{ background: value }}
      />
      <span className="mt-8 flex items-center justify-between gap-8">
        <span className="truncate text-12 font-semibold text-fg">{name}</span>
        {selected && <Check className="size-[14px] text-accent-text" />}
      </span>
      <span className="num block text-11 text-fg-2">{value}</span>
      {note && <span className="block text-11 text-fg-3">{note}</span>}
    </>
  );
  return onClick ? (
    <button
      type="button"
      onClick={onClick}
      className="pressable rounded-12 p-8 text-left live:hover:bg-card"
    >
      {content}
    </button>
  ) : (
    <div className="p-8">{content}</div>
  );
}

function ColorSection() {
  const accent = useUi((s) => s.accent);
  const setAccent = useUi((s) => s.setAccent);
  return (
    <Section
      id="color"
      title="Color"
      lede="Black surfaces that rise one step on hover, one accent, semantic yellow, red and green, and fixed colors per signal. Hairlines are the only borders."
    >
      <Group title="Surfaces" className="grid grid-cols-5 gap-8">
        {Object.entries(surface).map(([k, v]) => (
          <Swatch
            key={k}
            name={k}
            value={v}
            note={k === "lift" ? "top step" : `hover → ${surfaceStep[k as keyof typeof surface]}`}
          />
        ))}
      </Group>
      <Group title="Text" className="grid grid-cols-5 gap-8">
        {Object.entries(text).map(([k, v]) => (
          <Swatch key={k} name={k} value={v} />
        ))}
      </Group>
      <Group title="Accent (click to apply; ⌘K also changes it)" className="grid grid-cols-5 gap-8">
        {(Object.keys(accents) as AccentName[]).map((k) => (
          <Swatch
            key={k}
            name={accents[k].label}
            value={accents[k].accent}
            note={`text ${accents[k].text}`}
            selected={accent === k}
            onClick={() => setAccent(k)}
          />
        ))}
      </Group>
      <Group title="Semantic and tints" className="grid grid-cols-5 gap-8">
        {Object.entries(semantic).map(([k, v]) => (
          <Swatch key={k} name={k} value={v} />
        ))}
      </Group>
      <Group title="Signals (fixed, never follow the accent)" className="grid grid-cols-6 gap-8">
        {Object.entries(series).map(([k, v]) => (
          <Swatch key={k} name={k} value={v} />
        ))}
      </Group>
    </Section>
  );
}

function ContrastSection() {
  return (
    <Section
      id="contrast"
      title="Contrast"
      lede="Every text and surface pair the tokens declare, checked against WCAG AA in CI (pnpm contrast). Text needs 4.5:1; graphics such as the focus ring and scope traces need 3:1."
    >
      <div className="grid grid-cols-2 gap-x-32">
        {contrastPairs.map((p) => {
          const r = contrast(p.fg, p.bg);
          const ok = r >= MIN_RATIO[p.kind];
          return (
            <div
              key={`${p.fgName}/${p.bgName}`}
              className="grid h-[40px] grid-cols-[44px_1fr_auto_auto] items-center gap-16 border-b border-hairline"
            >
              <span
                className="grid h-[28px] place-items-center rounded-8 border border-hairline text-13 font-bold"
                style={{ background: p.bg, color: p.fg }}
              >
                Aa
              </span>
              <span className="min-w-0">
                <span className="block truncate text-12 font-medium text-fg">
                  {p.fgName} on {p.bgName}
                </span>
                <span className="block truncate text-11 text-fg-3">{p.use}</span>
              </span>
              <span className="num text-12 text-fg">{r.toFixed(2)}:1</span>
              <Pill tone={ok ? "ok" : "danger"}>{ok ? `AA ${p.kind}` : "fails"}</Pill>
            </div>
          );
        })}
      </div>
    </Section>
  );
}

/* ------------------------------------------------------------------ type, space */

function TypeSection() {
  const sizes = Object.keys(typeScale)
    .map(Number)
    .sort((a, b) => b - a);
  return (
    <Section
      id="type"
      title="Type"
      lede="SF Pro Rounded (ui-rounded) for the interface, falling back to the system face; JetBrains Mono with tabular figures for every number, so digits never shift."
    >
      <div className="flex flex-col">
        {sizes.map((s) => {
          const [lh] = typeScale[s as keyof typeof typeScale];
          return (
            <div
              key={s}
              className="grid grid-cols-[96px_1fr] items-baseline gap-16 border-b border-hairline py-8"
            >
              <span className="num text-11 text-fg-2">
                {s}/{lh}
              </span>
              <span
                className="truncate text-fg"
                style={{ fontSize: s, lineHeight: `${lh}px`, fontWeight: s >= 20 ? 600 : 400 }}
              >
                {s >= 28
                  ? "Balance at 20 km/h"
                  : "The firmware holds the pedals level while the rider stands still."}
              </span>
            </div>
          );
        })}
      </div>
      <div className="grid grid-cols-3 gap-16">
        <div className="rounded-12 border border-hairline bg-card p-16">
          <span className="label">Label</span>
          <p className="mt-8 text-12 text-fg-2">
            11 px, weight 700, 1 px tracking. Panel titles and HUD captions only.
          </p>
        </div>
        <div className="rounded-12 border border-hairline bg-card p-16">
          <span className="num text-28 font-medium text-fg">0123456789</span>
          <p className="mt-8 text-12 text-fg-2">Tabular numbers: every digit has the same width.</p>
        </div>
        <div className="rounded-12 border border-hairline bg-card p-16">
          <span className="text-15 font-semibold text-fg">Weights 400 / 500 / 600 / 700</span>
          <p className="mt-8 text-12 text-fg-2">
            Regular for body, medium for values, semibold for titles, bold for the app name.
          </p>
        </div>
      </div>
    </Section>
  );
}

function SpaceSection() {
  return (
    <Section
      id="space"
      title="Space and radius"
      lede="Seven spacing steps replace Tailwind's scale; a class name is its size in pixels (p-16 is 16 px). Radii nest: a 16 px card holds 12 px panels and 8 px controls; pills are fully round."
    >
      <div className="grid grid-cols-2 gap-32">
        <div className="flex flex-col gap-8">
          {spacing
            .filter((s) => s > 0)
            .map((s) => (
              <div key={s} className="grid grid-cols-[40px_1fr] items-center gap-16">
                <span className="num text-12 text-fg-2">{s}</span>
                <span className="h-[12px] rounded-4 bg-accent" style={{ width: s * 4 }} />
              </div>
            ))}
        </div>
        <div className="flex flex-wrap items-end gap-16">
          {Object.entries(radius).map(([k, v]) => (
            <div key={k} className="flex flex-col items-center gap-8">
              <span
                className="size-[72px] border border-hairline-strong bg-card"
                style={{ borderRadius: v }}
              />
              <span className="num text-12 text-fg-2">{k}</span>
            </div>
          ))}
        </div>
      </div>
    </Section>
  );
}

/* ------------------------------------------------------------------ motion */

function Curve({ duration, bounce }: { duration?: number; bounce?: number }) {
  const path = useMemo(() => {
    const pts: string[] = [];
    if (duration === undefined || bounce === undefined) {
      pts.push("0,60", "200,0");
    } else {
      const g = spring({ keyframes: [0, 1], duration: duration * 1000, bounce });
      for (let i = 0; i <= 60; i++) {
        const v = g.next((duration * 1000 * i) / 60).value;
        pts.push(`${(i / 60) * 200},${(60 - v * 60).toFixed(2)}`);
      }
    }
    return pts.join(" ");
  }, [duration, bounce]);
  return (
    <svg viewBox="-4 -16 208 84" className="h-[84px] w-full" aria-hidden="true">
      <line x1="0" x2="200" y1="0" y2="0" stroke="rgba(255,255,255,0.08)" strokeDasharray="2 3" />
      <line x1="0" x2="200" y1="60" y2="60" stroke="rgba(255,255,255,0.08)" />
      <polyline
        points={path}
        fill="none"
        stroke="var(--color-accent)"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function MotionCard({
  name,
  use,
  spec,
  transition,
  curve,
}: {
  name: string;
  use: string;
  spec: string;
  transition: object;
  curve: { duration?: number; bounce?: number };
}) {
  const [on, setOn] = useState(false);
  return (
    <div className="flex flex-col rounded-12 border border-hairline bg-card p-16">
      <span className="text-15 font-semibold text-fg">{name}</span>
      <span className="num mt-2 text-11 text-fg-2">{spec}</span>
      <p className="mt-8 text-12 text-fg-2">{use}</p>
      <Curve {...curve} />
      <div className="relative mt-8 h-[36px] w-full rounded-pill bg-bg">
        {/* The ball travels across an inner track that is one ball narrower than the pill. */}
        <div className="absolute inset-y-[4px] right-[32px] left-[4px]">
          <motion.span
            className="absolute top-0 size-[28px] rounded-pill bg-accent"
            initial={false}
            animate={{ left: on ? "100%" : "0%" }}
            transition={transition}
          />
        </div>
      </div>
      <div className="mt-16">
        <Button size="sm" icon={<RotateCcw />} onClick={() => setOn((v) => !v)}>
          Replay
        </Button>
      </div>
    </div>
  );
}

function RollingDemo() {
  const [v, setV] = useState(18.4);
  return (
    <div className="flex flex-col rounded-12 border border-hairline bg-card p-16">
      <span className="text-15 font-semibold text-fg">Rolling digits</span>
      <p className="mt-4 text-12 text-fg-2">
        Each digit has a slot. New digits rise in 0.34 s with a blur; old ones leave in 0.26 s.
        Direction follows the value; at most 8 updates a second.
      </p>
      <span className="mt-16 flex items-baseline gap-4">
        <RollingNumber value={v} decimals={1} className="text-44 font-medium text-fg" />
        <span className="text-13 font-semibold text-fg-2">km/h</span>
      </span>
      <div className="mt-16 flex gap-8">
        <Button size="sm" onClick={() => setV((x) => Math.min(99.9, x + 1 + Math.random() * 9))}>
          Up
        </Button>
        <Button size="sm" onClick={() => setV((x) => Math.max(0, x - 1 - Math.random() * 9))}>
          Down
        </Button>
      </div>
    </div>
  );
}

function HoverDemo() {
  return (
    <div className="flex flex-col rounded-12 border border-hairline bg-card p-16">
      <div className="flex items-baseline justify-between">
        <span className="text-15 font-semibold text-fg">Hover curve</span>
        <span className="num text-11 text-fg-2">{hover.duration * 1000} ms</span>
      </div>
      <p className="mt-4 text-12 text-fg-2">
        CSS only, {hover.ease}. Hover raises a surface one step; press scales to 0.97 with the
        snappy spring.
      </p>
      <div className="mt-16 grid grid-cols-3 gap-8">
        {(["bg", "card", "raised"] as const).map((s) => (
          <button
            key={s}
            type="button"
            className="pressable grid h-[56px] place-items-center rounded-8 border border-hairline text-12 font-medium text-fg"
            style={{ background: surface[s] }}
            onMouseEnter={(e) => (e.currentTarget.style.background = surface[surfaceStep[s]])}
            onMouseLeave={(e) => (e.currentTarget.style.background = surface[s])}
          >
            {s}
          </button>
        ))}
      </div>
    </div>
  );
}

function MotionSection() {
  return (
    <Section
      id="motion"
      title="Motion"
      lede="Four tokens, nothing else. CSS uses the same springs, sampled into linear() easings by the token generator. With reduced motion, transforms drop, digits cross-fade and camera moves cut."
    >
      <div className="grid grid-cols-4 gap-16">
        <MotionCard
          name="snappy"
          use="Buttons, toggles, pills, knobs."
          spec="spring 0.3 s, bounce 0.25"
          transition={snappy}
          curve={motionTokens.snappy}
        />
        <MotionCard
          name="smooth"
          use="Panels, sheets, workspace changes."
          spec="spring 0.55 s, bounce 0"
          transition={smooth}
          curve={motionTokens.smooth}
        />
        <MotionCard
          name="bouncy"
          use="Success moments only."
          spec="spring 0.5 s, bounce 0.45"
          transition={bouncy}
          curve={motionTokens.bouncy}
        />
        <MotionCard
          name="data"
          use="Gauges and live values."
          spec="linear 0.25 s"
          transition={dataT}
          curve={{}}
        />
      </div>
      <div className="grid grid-cols-2 gap-16">
        <RollingDemo />
        <HoverDemo />
      </div>
    </Section>
  );
}

/* ------------------------------------------------------------------ components */

function ButtonsSection() {
  const variants = ["primary", "secondary", "ghost", "danger"] as const;
  return (
    <Section
      id="buttons"
      title="Buttons"
      lede="Four variants in three sizes. A disabled button keeps its tooltip and says why it is disabled. Hover, press and focus are shown here statically."
    >
      <div className="grid grid-cols-[96px_repeat(5,minmax(0,1fr))] items-center gap-y-16">
        <span />
        {STATES.map((s) => (
          <span key={s} className="text-11 text-fg-3">
            {s}
          </span>
        ))}
        {variants.map((v) => (
          <Row key={v} label={v}>
            {STATES.map((s) => (
              <div key={s}>
                <Button
                  variant={v}
                  icon={v === "primary" ? <Play /> : v === "danger" ? <Zap /> : undefined}
                  force={force(s)}
                  disabledReason={s === "disabled" ? "Pause the run first" : undefined}
                >
                  {v === "primary"
                    ? "Run"
                    : v === "danger"
                      ? "Cut power"
                      : v === "ghost"
                        ? "Cancel"
                        : "Restart"}
                </Button>
              </div>
            ))}
          </Row>
        ))}
        <Row label="icon">
          {STATES.map((s) => (
            <div key={s} className="flex gap-8">
              <IconButton
                label="Restart"
                icon={<RotateCcw />}
                force={force(s)}
                disabledReason={s === "disabled" ? "Nothing to restart" : undefined}
              />
              <IconButton
                label="Right dock"
                icon={<PanelRight />}
                active
                force={force(s)}
                disabledReason={s === "disabled" ? "No dock here" : undefined}
              />
              <IconButton
                label="Camera"
                icon={<Camera />}
                variant="secondary"
                force={force(s)}
                disabledReason={s === "disabled" ? "No viewport" : undefined}
              />
            </div>
          ))}
        </Row>
      </div>
      <Group title="Sizes" className="flex items-center gap-16">
        <Button size="sm" variant="primary" icon={<Play />}>
          Small
        </Button>
        <Button size="md" variant="primary" icon={<Play />}>
          Medium
        </Button>
        <Button size="lg" variant="primary" icon={<Play />}>
          Large
        </Button>
        <IconButton size="sm" label="Small" icon={<Pause />} variant="secondary" />
        <IconButton size="md" label="Medium" icon={<Pause />} variant="secondary" />
        <IconButton size="lg" label="Large" icon={<Pause />} variant="secondary" />
      </Group>
    </Section>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <span className="text-12 font-medium text-fg-2">{label}</span>
      {children}
    </>
  );
}

function ControlsSection() {
  const [fw, setFw] = useState<"stock" | "compat" | "clean">("stock");
  const [on, setOn] = useState(true);
  const [kp, setKp] = useState(42);
  const [tab, setTab] = useState<"scope" | "vars" | "log">("scope");
  const [scen, setScen] = useState<"ride" | "idle">("ride");
  const fwOptions = [
    { value: "stock" as const, label: "Stock" },
    { value: "compat" as const, label: "Ours-compat" },
    { value: "clean" as const, label: "Ours-clean" },
  ];
  return (
    <Section
      id="controls"
      title="Controls"
      lede="Segmented controls slide a knob, toggles stretch their thumb while pressed, sliders show their value while dragged. Every control has a visible focus ring."
    >
      <Group title="Segmented control" className="flex flex-wrap gap-32">
        <State label="rest (live)">
          <SegmentedControl
            aria-label="Firmware"
            value={fw}
            onValueChange={setFw}
            options={fwOptions}
          />
        </State>
        <State label="hover on Ours-clean">
          <SegmentedControl
            aria-label="Firmware"
            value="stock"
            onValueChange={() => {}}
            options={fwOptions}
            force={{ value: "clean", state: "hover" }}
          />
        </State>
        <State label="focus">
          <SegmentedControl
            aria-label="Firmware"
            value="stock"
            onValueChange={() => {}}
            options={fwOptions}
            force={{ value: "stock", state: "focus" }}
          />
        </State>
        <State label="small, one option disabled">
          <SegmentedControl
            aria-label="Window"
            size="sm"
            value="10"
            onValueChange={() => {}}
            options={[
              { value: "5", label: "5 s" },
              { value: "10", label: "10 s" },
              { value: "60", label: "60 s", disabledReason: "History holds 30 s" },
            ]}
          />
        </State>
      </Group>
      <Group title="Toggle" className="grid grid-cols-3 gap-x-32 gap-y-16">
        <Toggle label="Show ghost wheel" description="Live" checked={on} onCheckedChange={setOn} />
        <Toggle label="Off" checked={false} onCheckedChange={() => {}} />
        <Toggle label="Hover" checked onCheckedChange={() => {}} force="hover" />
        <Toggle label="Press (thumb stretches)" checked onCheckedChange={() => {}} force="press" />
        <Toggle label="Focus" checked={false} onCheckedChange={() => {}} force="focus" />
        <Toggle
          label="Disabled"
          description="Tooltip says why"
          checked={false}
          onCheckedChange={() => {}}
          disabledReason="Only in Ours-clean"
        />
      </Group>
      <Group title="Slider" className="grid grid-cols-3 gap-x-32 gap-y-24">
        <Slider label="Balance gain (live)" value={kp} onValueChange={setKp} unit="%" />
        <Slider label="Hover" value={30} onValueChange={() => {}} force="hover" unit="%" />
        <Slider label="Dragging" value={64} onValueChange={() => {}} force="press" unit="%" />
        <Slider label="Focus" value={50} onValueChange={() => {}} force="focus" unit="%" />
        <Slider
          label="Disabled"
          value={20}
          onValueChange={() => {}}
          disabledReason="Locked while flashing"
          unit="%"
        />
      </Group>
      <Group title="Tabs and select" className="flex flex-wrap items-start gap-48">
        <State label="tabs (live)">
          <Tabs
            aria-label="Demo tabs"
            value={tab}
            onValueChange={setTab}
            tabs={[
              { value: "scope", label: "Scope", count: 3 },
              { value: "vars", label: "Variables", count: 128 },
              { value: "log", label: "Log" },
            ]}
          />
        </State>
        <State label="tabs, hover and disabled">
          <Tabs
            aria-label="Demo tabs"
            value="scope"
            onValueChange={() => {}}
            force={{ value: "vars", state: "hover" }}
            tabs={[
              { value: "scope", label: "Scope" },
              { value: "vars", label: "Variables" },
              { value: "log", label: "Log", disabledReason: "No log yet" },
            ]}
          />
        </State>
        <State label="select (live)">
          <Select
            aria-label="Scenario"
            value={scen}
            onValueChange={setScen}
            className="w-[220px]"
            options={[
              { value: "ride", label: "Ride to 20 km/h", detail: "Accelerate, cruise, brake" },
              { value: "idle", label: "Idle balance", detail: "Rider stands still" },
            ]}
          />
        </State>
        <State label="select, hover">
          <Select
            aria-label="Scenario"
            value="idle"
            onValueChange={() => {}}
            className="w-[220px]"
            force="hover"
            options={[{ value: "idle", label: "Idle balance" }]}
          />
        </State>
        <State label="select, focus">
          <Select
            aria-label="Scenario"
            value="idle"
            onValueChange={() => {}}
            className="w-[220px]"
            force="focus"
            options={[{ value: "idle", label: "Idle balance" }]}
          />
        </State>
      </Group>
      <Group title="Keys" className="flex items-center gap-24">
        <Shortcut keys="mod+k" />
        <Shortcut keys="mod+1" />
        <Shortcut keys="space" />
        <Kbd>?</Kbd>
        <Shortcut keys="mod+shift+b" />
      </Group>
    </Section>
  );
}

function FeedbackSection() {
  const tones = ["neutral", "accent", "ok", "caution", "danger", "info"] as const;
  return (
    <Section
      id="feedback"
      title="Feedback"
      lede="Pills name a state. Toasts confirm an action in its own words. A wait always names its step and, after 10 s, offers Show log, Retry and Cancel."
    >
      <Group title="Pills" className="flex flex-col gap-16">
        <div className="flex flex-wrap gap-8">
          {tones.map((t) => (
            <Pill key={t} tone={t}>
              {t}
            </Pill>
          ))}
        </div>
        <div className="flex flex-wrap gap-8">
          <Pill tone="ok" dot="live">
            Running
          </Pill>
          <Pill tone="caution" dot>
            Paused
          </Pill>
          <Pill tone="danger" dot>
            Cutout
          </Pill>
          <Pill tone="info" icon={<Cpu />} size="md">
            Ours-clean
          </Pill>
          <Pill tone="accent" size="md">
            M4
          </Pill>
        </div>
      </Group>
      <Group title="Tooltip" className="flex items-start gap-32">
        <State label="label">
          <TooltipBubble content="Restart from t = 0" />
        </State>
        <State label="with shortcut">
          <TooltipBubble content="Command palette" shortcut="mod+k" />
        </State>
        <State label="disabled reason">
          <TooltipBubble content="Needs our firmware running in the simulator (M4)" />
        </State>
        <State label="live (hover me)">
          <IconButton
            label="Keyboard shortcuts"
            shortcut="?"
            icon={<Keyboard />}
            variant="secondary"
          />
        </State>
      </Group>
      <Group title="Toasts" className="flex flex-wrap gap-16">
        <ToastCard
          item={{
            tone: "success",
            title: "Layout reset",
            description: "Docks are back to their default sizes.",
          }}
          onClose={() => {}}
        />
        <ToastCard
          item={{
            tone: "info",
            title: "Firmware switched to Ours-clean",
            description: "The run restarted at t = 0.",
          }}
          onClose={() => {}}
        />
        <ToastCard
          item={{
            tone: "caution",
            title: "Telemetry is behind",
            description: "Showing every second frame until the engine catches up.",
          }}
          onClose={() => {}}
        />
        <ToastCard
          item={{
            tone: "danger",
            title: "Stock image hash does not match",
            description: "The download was refused. Nothing was flashed.",
            action: { label: "Show log", run: () => {} },
          }}
          onClose={() => {}}
        />
        <div className="flex items-center">
          <Button
            onClick={() =>
              toast({
                tone: "success",
                title: "Saved",
                description: "A live toast, with the bouncy success icon.",
              })
            }
          >
            Show a live toast
          </Button>
        </div>
      </Group>
      <Group title="Named waits" className="flex flex-wrap gap-48">
        <State label="indeterminate">
          <Wait step="Fetching the stock image" detail="begode-extreme, 41.6 KB" />
        </State>
        <State label="determinate">
          <Wait step="Sending block 37 of 58" progress={0.64} />
        </State>
        <State label="stuck after 10 s">
          <Wait step="Waiting for the bootloader" stuck />
        </State>
      </Group>
    </Section>
  );
}

function DataSection() {
  const wave = useMemo(
    () => Array.from({ length: 120 }, (_, i) => 50 + 20 * Math.sin(i / 9) + 8 * Math.sin(i / 2.7)),
    [],
  );
  const ramp = useMemo(
    () => Array.from({ length: 120 }, (_, i) => 129 - i * 0.03 - 3 * Math.max(0, Math.sin(i / 14))),
    [],
  );
  const [tick, setTick] = useState(0);
  const ref = useRef(0);
  useEffect(() => {
    const id = window.setInterval(() => setTick(++ref.current), 1600);
    return () => window.clearInterval(id);
  }, []);
  const demo = [34, 76, 91, 52][tick % 4] ?? 34;
  return (
    <Section
      id="data"
      title="Data"
      lede="Live values move with the data token. Gauges turn caution or danger when they cross a marked threshold; sparklines draw on canvas from ring buffers without re-rendering React."
    >
      <Group title="Gauges" className="flex flex-wrap items-end gap-32">
        <State label="normal">
          <Gauge
            value={34}
            label="PWM"
            unit="%"
            zones={[
              { from: 70, tone: "caution" },
              { from: 85, tone: "danger" },
            ]}
            color={series.pwm}
          />
        </State>
        <State label="caution">
          <Gauge
            value={76}
            label="PWM"
            unit="%"
            zones={[
              { from: 70, tone: "caution" },
              { from: 85, tone: "danger" },
            ]}
            color={series.pwm}
          />
        </State>
        <State label="danger">
          <Gauge
            value={91}
            label="PWM"
            unit="%"
            zones={[
              { from: 70, tone: "caution" },
              { from: 85, tone: "danger" },
            ]}
            color={series.pwm}
          />
        </State>
        <State label="live">
          <Gauge
            value={demo}
            label="PWM"
            unit="%"
            zones={[
              { from: 70, tone: "caution" },
              { from: 85, tone: "danger" },
            ]}
            color={series.pwm}
          />
        </State>
        <State label="small, accent">
          <Gauge value={62} label="ISR" unit="%" size={72} />
        </State>
        <State label="pitch protractor">
          <PitchProtractor pitch={[1.4, 9.2, -13.1, 4.8][tick % 4] ?? 0} width={200} />
        </State>
      </Group>
      <Group title="Sparklines" className="grid grid-cols-3 gap-16">
        {[
          { name: "PWM", color: series.pwm, data: wave },
          { name: "Battery", color: series.voltage, data: ramp },
          { name: "Speed", color: series.speed, data: wave.map((v) => v * 0.4) },
        ].map((s) => (
          <div key={s.name} className="rounded-12 border border-hairline bg-card p-16">
            <span className="text-12 font-semibold text-fg-2">{s.name}</span>
            <Sparkline data={s.data} color={s.color} height={48} className="mt-8" />
          </div>
        ))}
      </Group>
      <Group title="Panel" className="h-[160px] w-[420px]">
        <Panel
          title="Telemetry"
          icon={<Activity />}
          actions={
            <Pill tone="ok" dot="live">
              Live
            </Pill>
          }
        >
          <div className="p-16 text-13 text-fg-2">
            A dock panel: 40 px header with a label and actions, then a scrollable body.
          </div>
        </Panel>
      </Group>
    </Section>
  );
}

const SAMPLE_COMMANDS: PaletteCommand[] = [
  {
    id: "a",
    title: "Go to Ride",
    group: "Workspaces",
    icon: <Unicycle />,
    shortcut: "mod+1",
    hint: "Current",
    run: () => {},
  },
  {
    id: "b",
    title: "Open component gallery",
    group: "Workspaces",
    icon: <LayoutGrid />,
    run: () => {},
  },
  {
    id: "c",
    title: "Pause simulation",
    group: "Simulation",
    icon: <Pause />,
    shortcut: "space",
    run: () => {},
  },
  {
    id: "d",
    title: "Faster (double the real-time factor)",
    group: "Simulation",
    icon: <Timer />,
    shortcut: "]",
    run: () => {},
  },
  {
    id: "e",
    title: "Hide bottom dock",
    group: "View",
    icon: <PanelBottom />,
    shortcut: "mod+j",
    run: () => {},
  },
  {
    id: "f",
    title: "Restart from t = 0",
    group: "Simulation",
    icon: <RefreshCw />,
    shortcut: "r",
    run: () => {},
  },
];

function OverlaysSection() {
  const setPaletteOpen = useUi((s) => s.setPaletteOpen);
  const setShortcutsOpen = useUi((s) => s.setShortcutsOpen);
  return (
    <Section
      id="overlays"
      title="Overlays"
      lede="The command palette holds every action with its shortcut; the shortcut sheet lists them by group. Both rise in with the smooth token over a dimmed app."
    >
      <div className="grid grid-cols-[minmax(0,560px)_1fr] items-start gap-32">
        <div className="flex h-[420px] flex-col overflow-hidden rounded-16 border border-hairline-strong bg-card">
          {/* cmdk scrolls its selected item into view on mount; mounting the preview only when
              it is on screen keeps that from yanking the gallery down on load. */}
          <WhenVisible>
            <PaletteBody commands={SAMPLE_COMMANDS} />
          </WhenVisible>
        </div>
        <div className="flex flex-col items-start gap-8">
          <Button variant="primary" icon={<LayoutGrid />} onClick={() => setPaletteOpen(true)}>
            Open the command palette
          </Button>
          <Button icon={<Keyboard />} onClick={() => setShortcutsOpen(true)}>
            Show keyboard shortcuts
          </Button>
        </div>
      </div>
    </Section>
  );
}

function WhenVisible({ children }: { children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const io = new IntersectionObserver(([e]) => {
      if (e?.isIntersecting) {
        setVisible(true);
        io.disconnect();
      }
    });
    io.observe(el);
    return () => io.disconnect();
  }, []);
  return (
    <div ref={ref} className="flex min-h-0 flex-1 flex-col">
      {visible && children}
    </div>
  );
}

/* ------------------------------------------------------------------ page */

export function Gallery() {
  const navigate = useUi((s) => s.navigate);
  const scroller = useRef<HTMLDivElement>(null);
  const [current, setCurrent] = useState<SectionId>("color");

  // Scroll spy: the current section is the last one whose top has passed the upper fifth.
  useEffect(() => {
    const root = scroller.current;
    if (!root) return;
    const sections = [...root.querySelectorAll<HTMLElement>("[data-section]")];
    const update = () => {
      const line = root.scrollTop + root.clientHeight * 0.2;
      let id = sections[0]?.dataset.section;
      for (const s of sections) if (s.offsetTop <= line) id = s.dataset.section;
      if (root.scrollTop + root.clientHeight >= root.scrollHeight - 2)
        id = sections.at(-1)?.dataset.section;
      if (id) setCurrent(id as SectionId);
    };
    update();
    root.addEventListener("scroll", update, { passive: true });
    return () => root.removeEventListener("scroll", update);
  }, []);

  return (
    <div className="h-full p-8">
      <div className="flex h-full overflow-hidden rounded-12 border border-hairline bg-bg">
        <nav
          aria-label="Gallery sections"
          className="flex w-[200px] shrink-0 flex-col gap-2 border-r border-hairline p-16"
        >
          <span className="mb-8 px-8 text-13 font-semibold text-fg">Design system</span>
          {SECTIONS.map(([id, label]) => (
            <a
              key={id}
              href={`#/gallery`}
              onClick={(e) => {
                e.preventDefault();
                scroller.current?.querySelector(`#${id}`)?.scrollIntoView({ behavior: "smooth" });
              }}
              className={cx(
                "relative flex h-[28px] items-center rounded-8 px-8 text-13 transition-colors",
                current === id ? "text-fg" : "text-fg-2 hover:bg-card hover:text-fg",
              )}
            >
              {current === id && (
                <motion.span
                  layoutId="gallery-nav"
                  transition={snappy}
                  className="absolute inset-0 rounded-8 bg-raised"
                />
              )}
              <span className="relative">{label}</span>
            </a>
          ))}
          <div className="flex-1" />
          <Button
            variant="ghost"
            size="sm"
            icon={<Unicycle />}
            onClick={() => navigate("ride")}
            className="justify-start"
          >
            Back to Ride
          </Button>
        </nav>
        <div ref={scroller} className="min-w-0 flex-1 overflow-y-auto" data-testid="gallery-scroll">
          <div className="mx-auto max-w-[1120px] px-48 py-48">
            <header className="mb-48">
              <h1 className="text-28 font-bold text-fg">EUC Studio design system</h1>
              <p className="mt-8 max-w-[640px] text-15 text-fg-2">
                Every token and component in every state. Colors, type, spacing and motion come from
                one file, src/design/tokens.ts; CI checks the generated CSS and every contrast pair.
              </p>
            </header>
            <ColorSection />
            <ContrastSection />
            <TypeSection />
            <SpaceSection />
            <MotionSection />
            <ButtonsSection />
            <ControlsSection />
            <FeedbackSection />
            <DataSection />
            <OverlaysSection />
          </div>
        </div>
      </div>
    </div>
  );
}
