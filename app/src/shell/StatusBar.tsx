import { cx } from "../lib/cx.ts";
import { engine, useEngineVersion, useLive } from "../sim/engine.ts";
import { FIRMWARES, SAMPLE_RATE } from "../sim/synthetic.ts";
import { BluetoothOff, Cpu } from "../ui/icons.tsx";
import { RollingNumber } from "../ui/RollingNumber.tsx";
import { Tooltip } from "../ui/Tooltip.tsx";

function Item({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <span className={cx("flex h-full items-center gap-4 [&_svg]:size-[12px]", className)}>
      {children}
    </span>
  );
}

function IsrLoad() {
  const isr = useLive((l) => l.isr, 4, 0);
  const tone = isr >= 85 ? "bg-danger" : isr >= 70 ? "bg-caution" : "bg-ok";
  return (
    <Tooltip content="Share of CPU time spent in interrupt handlers" side="top">
      <Item className="cursor-default">
        <Cpu className="text-fg-3" />
        ISR
        <span className="relative ml-2 h-[4px] w-[32px] overflow-hidden rounded-pill bg-track">
          <span
            className={cx(
              "absolute inset-y-0 left-0 rounded-pill transition-[width] duration-(--dur-data) ease-data",
              tone,
            )}
            style={{ width: `${Math.min(100, isr)}%` }}
          />
        </span>
        <span className="num w-[32px] text-right text-fg-2">
          <RollingNumber value={isr} />%
        </span>
      </Item>
    </Tooltip>
  );
}

export function StatusBar() {
  useEngineVersion();
  const fw = FIRMWARES.find((f) => f.id === engine.firmware);
  return (
    <footer className="flex h-(--statusbar) shrink-0 items-center gap-16 border-t border-hairline bg-bg px-16 text-12 text-fg-2">
      <Tooltip content="Synthetic signals until the Rust engine lands (M1)" side="top">
        <Item className="cursor-default">
          <span
            className={cx(
              "size-[6px] rounded-pill",
              engine.playing
                ? "animate-[live-pulse_1.6s_ease-in-out_infinite] bg-ok"
                : "bg-caution",
            )}
          />
          <span className="text-fg">Demo engine</span>
          <span>{engine.playing ? "running" : "paused"}</span>
        </Item>
      </Tooltip>
      <Item>{fw?.label} firmware</Item>
      <IsrLoad />
      <Tooltip content="Connect a wheel in the Flash workspace (M6)" side="top">
        <Item className="cursor-default">
          <BluetoothOff className="text-fg-3" />
          No wheel connected
        </Item>
      </Tooltip>
      <span className="flex-1" />
      <Item>
        <span className="num">{SAMPLE_RATE} Hz</span> telemetry
      </Item>
      <Item className="text-fg-3">
        EUC Studio <span className="num">{__APP_VERSION__}</span>
      </Item>
    </footer>
  );
}
