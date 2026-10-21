import { useId } from "react";
import { Popover } from "radix-ui";
import { Button } from "../../ui/Button.tsx";
import { Pill } from "../../ui/Pill.tsx";
import { Select } from "../../ui/Select.tsx";
import { ChevronDown, Info } from "../../ui/icons.tsx";
import { triggerValid, type Capture, type TriggerConfig, type TriggerMode } from "./triggers.ts";

export function TriggerControls({
  config,
  selected,
  thresholdText,
  onMode,
  onSource,
  onThreshold,
  onPostSeconds,
  capture,
  canHold,
  hold,
  rearm,
  cursorOffset,
  onCursor,
}: {
  config: TriggerConfig;
  selected: string[];
  thresholdText: string;
  onMode: (mode: TriggerMode) => void;
  onSource: (name: string) => void;
  onThreshold: (text: string) => void;
  onPostSeconds: (seconds: number) => void;
  capture: Capture | null;
  canHold: boolean;
  hold: () => void;
  rearm: () => void;
  cursorOffset: number | null;
  onCursor: (offset: number | null) => void;
}) {
  const thresholdId = useId();
  const cursorId = useId();
  const valid = triggerValid(config, selected);
  return (
    <section aria-label="Scope trigger" className="shrink-0 border-b border-hairline bg-card p-8">
      <div className="flex flex-wrap items-center gap-8">
        <span className="text-12 text-fg-2">Trigger</span>
        <Select
          aria-label="Trigger mode"
          value={config.mode}
          onValueChange={onMode}
          options={[
            { value: "free", label: "Free" },
            { value: "rising", label: "Rising" },
            { value: "falling", label: "Falling" },
          ]}
        />
        {selected.length > 0 ? (
          <Select
            aria-label="Trigger variable"
            value={config.source}
            onValueChange={onSource}
            options={selected.map((name) => ({ value: name, label: name }))}
            className="max-w-[160px]"
          />
        ) : (
          <span className="text-12 text-fg-2">Select a scope variable.</span>
        )}
        <label htmlFor={thresholdId} className="text-12 text-fg-2">
          Raw threshold
        </label>
        <input
          id={thresholdId}
          type="number"
          step="any"
          value={thresholdText}
          onChange={(event) => onThreshold(event.target.value)}
          className="num h-(--control-md) w-[96px] rounded-8 border border-hairline bg-raised px-8 text-13"
          aria-invalid={!valid && config.mode !== "free"}
        />
        {config.mode !== "free" && (
          <>
            <span className="text-12 text-fg-2">After trigger</span>
            <Select
              aria-label="Capture time after trigger"
              value={String(config.postSeconds)}
              onValueChange={(value) => onPostSeconds(Number(value))}
              options={[0, 1, 5, 10, 15].map((seconds) => ({
                value: String(seconds),
                label: `${seconds} s`,
              }))}
            />
          </>
        )}
        <Button
          size="sm"
          onClick={hold}
          disabledReason={!canHold ? "Wait for a selected scope sample." : undefined}
        >
          Hold
        </Button>
        <Button
          size="sm"
          onClick={rearm}
          disabledReason={
            !canHold || !valid ? "Select a variable and enter a finite threshold." : undefined
          }
        >
          Rearm
        </Button>
        <Pill tone={capture?.phase === "held" ? "caution" : "neutral"}>
          {!valid
            ? "Invalid trigger"
            : (capture?.phase ?? (config.mode === "free" ? "free" : "armed"))}
        </Pill>
        {capture?.at != null && (
          <span
            className="num text-11 text-fg-2"
            title={capture.automatic ? "Sampled crossing" : "Manual hold"}
          >
            {capture.automatic ? "Crossing" : "Hold"} at {capture.at.toFixed(3)} s
          </span>
        )}
        <Popover.Root>
          <Popover.Trigger asChild>
            <Button
              size="sm"
              variant="ghost"
              className="group ml-auto gap-4 text-fg-2"
              icon={<Info size={14} aria-hidden="true" />}
            >
              Trigger help
              <ChevronDown
                size={14}
                aria-hidden="true"
                className="group-data-[state=open]:rotate-180"
              />
            </Button>
          </Popover.Trigger>
          <Popover.Portal>
            <Popover.Content
              side="bottom"
              align="end"
              sideOffset={8}
              collisionPadding={8}
              hideWhenDetached
              className="pop z-50 max-h-(--radix-popover-content-available-height) w-[360px] max-w-[calc(100vw-16px)] origin-(--radix-popover-content-transform-origin) overflow-auto rounded-12 border border-hairline-strong bg-card p-16 text-12 text-fg-2 shadow-overlay"
            >
              <Popover.Title className="sr-only">Trigger help</Popover.Title>
              <p>Rising crosses from below to the threshold or above.</p>
              <p className="mt-8">Falling crosses from above to the threshold or below.</p>
              <p className="mt-8">
                Hold freezes scopes only. Polling can miss edges between samples.
              </p>
              <p className="mt-8">
                Rearm waits for a new crossing. Thresholds use raw variable units.
              </p>
              <p className="mt-8">
                Capture continues for the selected simulation time after a crossing. Pausing also
                pauses capture time.
              </p>
              <p className="mt-8">
                Each variable holds at most 240 samples. A sample limit or ended run produces an
                incomplete capture.
              </p>
            </Popover.Content>
          </Popover.Portal>
        </Popover.Root>
      </div>
      {(capture?.reason === "sample_limit" || capture?.reason === "run_ended") && (
        <p role="status" className="mt-8 text-12 text-caution">
          Incomplete capture.{" "}
          {capture.reason === "sample_limit"
            ? "The 240-sample limit was reached."
            : "The run ended before the requested capture time."}
        </p>
      )}
      {!valid && (
        <p role="status" className="mt-8 text-12 text-caution">
          Select a scope variable and enter a finite threshold.
        </p>
      )}
      <div className="mt-8 flex flex-wrap items-center gap-8">
        <label htmlFor={cursorId} className="text-12 text-fg-2">
          Shared cursor
        </label>
        <input
          id={cursorId}
          type="range"
          min="-30"
          max="0"
          step="0.05"
          value={cursorOffset ?? 0}
          onChange={(event) => onCursor(Number(event.target.value))}
          aria-valuetext={
            cursorOffset === null ? "Off" : `${cursorOffset.toFixed(2)} seconds from scope end`
          }
          className="min-w-[96px] max-w-[240px] flex-1 accent-accent"
        />
        <span className="num min-w-[112px] text-11 text-fg-2">
          {cursorOffset === null ? "Off" : `${cursorOffset.toFixed(2)} s from end`}
        </span>
        <Button size="sm" variant="ghost" onClick={() => onCursor(null)}>
          Clear cursor
        </Button>
      </div>
    </section>
  );
}
