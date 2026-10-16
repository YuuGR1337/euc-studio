import { useState } from "react";
import { series } from "../../design/tokens.ts";
import { engine, useLive } from "../../sim/engine.ts";
import { CHANNELS, type ChannelId, SAMPLE_RATE } from "../../sim/synthetic.ts";
import { Activity } from "../../ui/icons.tsx";
import { Panel } from "../../ui/Panel.tsx";
import { RollingNumber } from "../../ui/RollingNumber.tsx";
import { SegmentedControl } from "../../ui/SegmentedControl.tsx";
import { Sparkline } from "../../ui/Sparkline.tsx";
import { Scope } from "./Scope.tsx";

const LANES: { id: ChannelId; color: string }[] = [
  { id: "pwm", color: series.pwm },
  { id: "speed", color: series.speed },
  { id: "current", color: series.current },
];

const CHANNEL_ROWS: { id: ChannelId; color: string }[] = [
  { id: "voltage", color: series.voltage },
  { id: "pitch", color: series.pitch },
  { id: "temp", color: series.temp },
];

function LiveValue({ id }: { id: ChannelId }) {
  const info = CHANNELS[id];
  const v = useLive((l) => l[id as keyof typeof l] as number, 8, info.decimals);
  return (
    <span className="flex items-baseline gap-4">
      <RollingNumber value={v} decimals={info.decimals} className="text-13 font-medium text-fg" />
      <span className="w-[28px] text-11 font-semibold text-fg-2">{info.unit}</span>
    </span>
  );
}

function Lane({
  id,
  color,
  seconds,
  last,
}: {
  id: ChannelId;
  color: string;
  seconds: number;
  last: boolean;
}) {
  const info = CHANNELS[id];
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-[24px] shrink-0 items-center gap-8 px-16">
        <span className="size-[8px] rounded-pill" style={{ background: color }} />
        <span className="text-12 font-semibold text-fg">{info.label}</span>
        <span className="ml-auto">
          <LiveValue id={id} />
        </span>
      </div>
      <div className="min-h-0 flex-1 pr-8 pl-4">
        <Scope
          time={engine.time}
          values={engine.channels[id]}
          color={color}
          min={info.min}
          max={info.max}
          seconds={seconds}
          showTime={last}
          unit={info.unit}
        />
      </div>
    </div>
  );
}

type WindowSecs = "5" | "10" | "30";

export function TelemetryPanel() {
  const [win, setWin] = useState<WindowSecs>("10");
  const seconds = Number(win);
  return (
    <Panel
      title="Telemetry"
      icon={<Activity />}
      className="min-w-[300px]"
      bodyClassName="flex flex-col overflow-hidden"
      actions={
        <SegmentedControl<WindowSecs>
          aria-label="Time window"
          size="sm"
          value={win}
          onValueChange={setWin}
          options={[
            { value: "5", label: "5 s" },
            { value: "10", label: "10 s" },
            { value: "30", label: "30 s" },
          ]}
        />
      }
    >
      <div className="flex min-h-[300px] flex-1 flex-col gap-8 py-8">
        {LANES.map((l, i) => (
          <Lane
            key={l.id}
            id={l.id}
            color={l.color}
            seconds={seconds}
            last={i === LANES.length - 1}
          />
        ))}
      </div>
      <div className="shrink-0 border-t border-hairline px-16 py-8">
        <div className="flex h-[24px] items-center justify-between">
          <span className="label">Channels</span>
          <span className="num text-11 text-fg-2">{SAMPLE_RATE} Hz</span>
        </div>
        <ul className="flex flex-col">
          {CHANNEL_ROWS.map((c) => (
            <li
              key={c.id}
              className="grid h-[40px] grid-cols-[96px_1fr_auto] items-center gap-16 border-b border-hairline last:border-b-0"
            >
              <span className="flex items-center gap-8 text-12 font-medium text-fg-2">
                <span className="size-[6px] rounded-pill" style={{ background: c.color }} />
                {CHANNELS[c.id].label}
              </span>
              <Sparkline
                ring={engine.channels[c.id]}
                window={seconds * SAMPLE_RATE}
                color={c.color}
                height={24}
              />
              <LiveValue id={c.id} />
            </li>
          ))}
        </ul>
      </div>
    </Panel>
  );
}
