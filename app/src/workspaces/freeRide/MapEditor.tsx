import { useState } from "react";
import {
  flatMap,
  mapProblem,
  type GeneratedMapSpec,
  type HeightMap,
  type MapFeature,
} from "../../engine/freeRide.ts";
import { Button, IconButton } from "../../ui/Button.tsx";
import { ChevronRight, Info, X } from "../../ui/icons.tsx";
import { Select } from "../../ui/Select.tsx";
import { Slider } from "../../ui/Slider.tsx";

interface Props {
  applied: GeneratedMapSpec | null;
  map: HeightMap | null;
  disabledReason: string | undefined;
  onApply: (map: GeneratedMapSpec) => void;
  onPause?: () => void;
}
function NumberField({
  label,
  value,
  min,
  max,
  step,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
}) {
  return (
    <label className="flex min-w-0 flex-col gap-8 text-12 text-fg-2">
      {label}
      <input
        className="num h-(--control-md) w-full min-w-0 rounded-8 border border-hairline bg-raised px-8 text-13 text-fg disabled:opacity-40"
        type="number"
        value={Number.isFinite(value) ? value : ""}
        min={min}
        max={max}
        step={step}
        onChange={(event) => onChange(event.currentTarget.valueAsNumber)}
      />
    </label>
  );
}

/** Footprints only. The native generator owns the actual height surface. */
export function MapEditor({ applied, map, disabledReason, onApply, onPause }: Props) {
  const [draft, setDraft] = useState<GeneratedMapSpec>(() => structuredClone(applied ?? flatMap()));
  const [selected, setSelected] = useState<number | null>(applied?.features.length ? 0 : null);
  const [advanced, setAdvanced] = useState(false);
  const problem = mapProblem(draft);
  const selectedFeature = selected === null ? undefined : draft.features[selected];
  const preset = draft.features.length
    ? "custom"
    : draft.grade_pct[0] === 0 && draft.grade_pct[1] === 0
      ? "flat"
      : draft.grade_pct[0] === 5 && draft.grade_pct[1] === 0
        ? "slope"
        : "custom";
  function featurePatch(index: number, patch: Partial<MapFeature>) {
    setDraft((previous) => ({
      ...previous,
      features: previous.features.map((feature, i) =>
        i === index ? { ...feature, ...patch } : feature,
      ),
    }));
  }
  function add(kind: MapFeature["kind"]) {
    if (disabledReason || draft.features.length >= 32) return;
    const index = draft.features.length;
    setDraft({
      ...draft,
      features: [
        ...draft.features,
        {
          kind,
          centre_xy_m: [4 + (index % 3) * 4, -12 + Math.floor(index / 3) * 2],
          width_xy_m: [2, 2],
          size_m: 0.05,
        },
      ],
    });
    setSelected(index);
  }
  function grade(axis: 0 | 1, value: number) {
    const next: [number, number] = [...draft.grade_pct];
    next[axis] = value;
    setDraft({ ...draft, grade_pct: next });
  }
  return (
    <section aria-label="Generated map editor" className="flex flex-col gap-16">
      {disabledReason && (
        <div className="flex items-center justify-between gap-8">
          <p className="text-12 text-fg-2">{disabledReason}</p>
          {onPause && (
            <Button size="sm" onClick={onPause}>
              Pause to edit
            </Button>
          )}
        </div>
      )}
      <fieldset
        disabled={!!disabledReason}
        title={disabledReason}
        className="flex min-w-0 flex-col gap-24 [&_button:disabled]:opacity-40"
      >
        <legend className="sr-only">Map parameters</legend>
        <div>
          <span className="mb-8 block text-12 text-fg-2">Preset</span>
          <Select
            aria-label="Map preset"
            value={preset}
            className="w-full"
            options={[
              { value: "flat", label: "Flat" },
              { value: "slope", label: "Gentle slope" },
              { value: "custom", label: "Custom" },
            ]}
            onValueChange={(value) => {
              if (disabledReason || value === "custom") return;
              const base = flatMap(Number.isInteger(draft.seed) ? draft.seed : 0);
              setDraft(value === "slope" ? { ...base, grade_pct: [5, 0] } : base);
              setSelected(null);
            }}
          />
        </div>
        <div className="flex flex-col gap-8">
          <div className="flex items-center justify-between gap-8">
            <span className="text-12 text-fg-2">Map preview</span>
            <span className="num text-11 text-fg-2">32 × 32 m</span>
            <IconButton
              size="sm"
              icon={<Info />}
              label="This preview shows feature outlines. Apply the map to get the actual height surface."
            />
          </div>
          <svg
            viewBox="-18 -18 36 36"
            className="h-[160px] w-full rounded-8 bg-bg"
            role="img"
            aria-label="Draft map footprints. World x points right. World y points up. The centre circle protects the spawn zone."
            onClick={(event) => {
              if (disabledReason || selected === null || !selectedFeature) return;
              const matrix = event.currentTarget.getScreenCTM();
              if (!matrix) return;
              const point = new DOMPoint(event.clientX, event.clientY).matrixTransform(
                matrix.inverse(),
              );
              const x = Math.round(point.x * 4) / 4;
              const y = Math.round(-point.y * 4) / 4;
              featurePatch(selected, { centre_xy_m: [x, y] });
            }}
          >
            <rect
              x="-16"
              y="-16"
              width="32"
              height="32"
              fill="var(--color-card)"
              stroke="var(--color-fg-3)"
              strokeWidth="0.1"
            />
            {[-12, -8, -4, 0, 4, 8, 12].map((n) => (
              <g key={n} stroke="var(--color-hairline-strong)" strokeWidth="0.05">
                <path d={`M${n},-16 V16 M-16,${n} H16`} />
              </g>
            ))}
            <circle
              r="2"
              fill="var(--color-caution-tint)"
              stroke="var(--color-caution)"
              strokeWidth="0.1"
            />
            <circle r="1" fill="none" stroke="var(--color-fg-3)" strokeWidth="0.08" />
            <path
              d="M0,0 H1.5 M1,0.4 L1.5,0 L1,-0.4"
              fill="none"
              stroke="var(--color-fg)"
              strokeWidth="0.12"
            />
            {draft.features.map((feature, index) => {
              if (![...feature.centre_xy_m, ...feature.width_xy_m].every(Number.isFinite))
                return null;
              const [x, y] = feature.centre_xy_m;
              const [w, h] = feature.width_xy_m;
              return (
                <g
                  key={index}
                  onClick={(event) => {
                    event.stopPropagation();
                    if (!disabledReason) setSelected(index);
                  }}
                >
                  <rect
                    x={x - w / 2}
                    y={-y - h / 2}
                    width={Math.max(0, w)}
                    height={Math.max(0, h)}
                    fill={
                      feature.kind === "bump"
                        ? "var(--color-accent-tint)"
                        : "var(--color-info-tint)"
                    }
                    stroke={
                      selected === index
                        ? "var(--color-fg)"
                        : feature.kind === "bump"
                          ? "var(--color-accent)"
                          : "var(--color-info)"
                    }
                    strokeWidth={selected === index ? 0.2 : 0.1}
                  />
                  <text
                    x={x}
                    y={-y}
                    textAnchor="middle"
                    dominantBaseline="middle"
                    fill="var(--color-fg)"
                    fontSize="0.8"
                  >
                    {index + 1}
                  </text>
                </g>
              );
            })}
            <text x="14" y="17.4" fill="var(--color-fg-2-safe)" fontSize="1">
              +x
            </text>
            <text x="-17.4" y="-14" fill="var(--color-fg-2-safe)" fontSize="1">
              +y
            </text>
          </svg>
        </div>
        <div className="flex flex-col gap-16">
          {([0, 1] as const).map((axis) => (
            <Slider
              key={axis}
              label={axis === 0 ? "Forward slope (X)" : "Side slope (Y)"}
              value={Number.isFinite(draft.grade_pct[axis]) ? draft.grade_pct[axis] : 0}
              min={-20}
              max={20}
              step={0.5}
              unit="%"
              decimals={1}
              disabledReason={disabledReason}
              onValueChange={(value) => grade(axis, value)}
            />
          ))}
        </div>
        <div className="flex flex-col gap-16 border-t border-hairline pt-16">
          <div className="flex flex-wrap items-center gap-8">
            <h3 className="mr-auto text-13 font-semibold">
              Features{" "}
              <span className="num text-12 font-normal text-fg-2">{draft.features.length}/32</span>
            </h3>
            <Button
              size="sm"
              disabledReason={
                disabledReason ??
                (draft.features.length >= 32 ? "Use at most 32 features." : undefined)
              }
              onClick={() => add("bump")}
            >
              Add bump
            </Button>
            <Button
              size="sm"
              disabledReason={
                disabledReason ??
                (draft.features.length >= 32 ? "Use at most 32 features." : undefined)
              }
              onClick={() => add("pothole")}
            >
              Add pothole
            </Button>
          </div>
          {draft.features.length > 0 && (
            <div className="flex items-center gap-8">
              <Select
                aria-label="Feature"
                className="flex-1"
                value={selected === null ? "none" : String(selected)}
                options={[
                  { value: "none", label: "Select a feature" },
                  ...draft.features.map((feature, index) => ({
                    value: String(index),
                    label: `${index + 1}. ${feature.kind === "bump" ? "Bump" : "Pothole"}`,
                  })),
                ]}
                onValueChange={(value) => {
                  if (!disabledReason) setSelected(value === "none" ? null : Number(value));
                }}
              />
              <IconButton
                size="sm"
                label="Remove selected feature"
                icon={<X />}
                disabledReason={
                  disabledReason ?? (selected === null ? "Select a feature first." : undefined)
                }
                onClick={() => {
                  setDraft({
                    ...draft,
                    features: draft.features.filter((_, index) => index !== selected),
                  });
                  setSelected(null);
                }}
              />
            </div>
          )}
          {selectedFeature && selected !== null && (
            <div className="flex flex-col gap-16">
              <Select
                aria-label="Feature kind"
                className="w-full"
                value={selectedFeature.kind}
                options={[
                  { value: "bump", label: "Bump" },
                  { value: "pothole", label: "Pothole" },
                ]}
                onValueChange={(kind) => {
                  if (!disabledReason) featurePatch(selected, { kind });
                }}
              />
              <Slider
                label={selectedFeature.kind === "bump" ? "Height" : "Depth"}
                value={Number.isFinite(selectedFeature.size_m) ? selectedFeature.size_m * 1000 : 1}
                min={1}
                max={300}
                step={1}
                unit="mm"
                disabledReason={disabledReason}
                onValueChange={(value) => featurePatch(selected, { size_m: value / 1000 })}
              />
              <div className="grid grid-cols-2 gap-16">
                {([0, 1] as const).map((axis) => (
                  <NumberField
                    key={`centre${axis}`}
                    label={`Centre ${axis === 0 ? "x" : "y"} (m)`}
                    value={selectedFeature.centre_xy_m[axis]}
                    min={-16}
                    max={16}
                    step={0.25}
                    onChange={(value) => {
                      const centre: [number, number] = [...selectedFeature.centre_xy_m];
                      centre[axis] = value;
                      featurePatch(selected, { centre_xy_m: centre });
                    }}
                  />
                ))}
                {([0, 1] as const).map((axis) => (
                  <NumberField
                    key={`width${axis}`}
                    label={`${axis === 0 ? "X" : "Y"} width (m)`}
                    value={selectedFeature.width_xy_m[axis]}
                    min={1}
                    max={32}
                    step={0.25}
                    onChange={(value) => {
                      const width: [number, number] = [...selectedFeature.width_xy_m];
                      width[axis] = value;
                      featurePatch(selected, { width_xy_m: width });
                    }}
                  />
                ))}
              </div>
            </div>
          )}
        </div>
        <div className="flex flex-col gap-16">
          <Button
            size="sm"
            variant="ghost"
            className="justify-start"
            aria-expanded={advanced}
            icon={<ChevronRight className={advanced ? "rotate-90" : undefined} />}
            onClick={() => setAdvanced(!advanced)}
          >
            Map details
          </Button>
          {advanced && (
            <div className="flex flex-col gap-16">
              <div className="grid grid-cols-2 gap-16">
                {([0, 1] as const).map((axis) => (
                  <NumberField
                    key={axis}
                    label={`Exact ${axis === 0 ? "x" : "y"} grade (%)`}
                    value={draft.grade_pct[axis]}
                    min={-20}
                    max={20}
                    step={0.1}
                    onChange={(value) => grade(axis, value)}
                  />
                ))}
              </div>
              <NumberField
                label="Seed"
                value={draft.seed}
                min={0}
                max={4294967295}
                step={1}
                onChange={(seed) => setDraft({ ...draft, seed })}
              />
              <dl className="flex flex-col gap-8 text-12">
                {(
                  [
                    ["Combined grade", `${Math.hypot(...draft.grade_pct).toFixed(1)}% / 20%`],
                    ["Grid", "129 × 129 / 0.25 m"],
                    ["Origin", "−16, −16 m"],
                    ["Level spawn / protected zone", "1 m / 2 m"],
                    ["Native revision", map ? String(map.revision) : "Unavailable"],
                  ] as const
                ).map(([label, value]) => (
                  <div key={label} className="flex justify-between gap-16">
                    <dt className="text-fg-2">{label}</dt>
                    <dd className="num">{value}</dd>
                  </div>
                ))}
              </dl>
              <p className="text-12 text-fg-2">
                Presets add no hidden features. The native generator checks combined heights and
                grades.
              </p>
            </div>
          )}
        </div>
      </fieldset>
      {!applied && map && (
        <p className="text-12 text-caution">
          Current map parameters are unavailable. This draft starts flat. Cancel keeps the current
          map.
        </p>
      )}
      {problem && (
        <p role="status" className="text-12 text-caution">
          {problem}
        </p>
      )}
      <div className="flex items-center gap-8 border-t border-hairline pt-16">
        <Button
          variant="primary"
          className="flex-1"
          disabledReason={disabledReason ?? problem ?? undefined}
          onClick={() => onApply(structuredClone(draft))}
        >
          Apply map and reset
        </Button>
        <Button
          variant="ghost"
          disabledReason={disabledReason}
          onClick={() => {
            setDraft(structuredClone(applied ?? flatMap()));
            setSelected(applied?.features.length ? 0 : null);
          }}
        >
          Cancel
        </Button>
      </div>
    </section>
  );
}
