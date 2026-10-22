import { useState } from "react";
import {
  DEFAULT_DENSITIES,
  randomSeed,
  worldProblem,
  type WorldChoice,
  type WorldDensities,
  type WorldFrame,
} from "../../engine/world.ts";
import { Button } from "../../ui/Button.tsx";
import { ChevronRight } from "../../ui/icons.tsx";
import { Slider } from "../../ui/Slider.tsx";

interface Props {
  applied: WorldChoice | null;
  /** The current run's world, when it rides one. */
  current: WorldFrame | null;
  disabledReason: string | undefined;
  onApply: (world: WorldChoice) => void;
}

/** A density control: `scale` turns the engine's value into the slider's (fractions as %). */
interface Density {
  key: keyof WorldDensities;
  label: string;
  max: number;
  step: number;
  unit: string;
  scale: number;
  decimals: number;
}

const MAIN: Density[] = [
  {
    key: "potholes",
    label: "Potholes",
    max: 10,
    step: 0.25,
    unit: "/1000 m²",
    scale: 1,
    decimals: 2,
  },
  { key: "cracks", label: "Cracks", max: 40, step: 1, unit: "/1000 m²", scale: 1, decimals: 0 },
  {
    key: "speed_bumps",
    label: "Speed bumps",
    max: 100,
    step: 5,
    unit: "% of streets",
    scale: 100,
    decimals: 0,
  },
  {
    key: "plazas",
    label: "Plazas with stairs",
    max: 100,
    step: 5,
    unit: "weight %",
    scale: 100,
    decimals: 0,
  },
  {
    key: "props",
    label: "Street furniture and trees",
    max: 100,
    step: 5,
    unit: "%",
    scale: 100,
    decimals: 0,
  },
];
const MORE: Density[] = [
  {
    key: "manholes",
    label: "Manholes",
    max: 3,
    step: 0.1,
    unit: "/1000 m²",
    scale: 1,
    decimals: 1,
  },
  {
    key: "rough_patches",
    label: "Rough patches",
    max: 6,
    step: 0.1,
    unit: "/1000 m²",
    scale: 1,
    decimals: 1,
  },
  {
    key: "wet_patches",
    label: "Wet patches",
    max: 9,
    step: 0.25,
    unit: "/1000 m²",
    scale: 1,
    decimals: 2,
  },
  {
    key: "crosswalks",
    label: "Crosswalks",
    max: 100,
    step: 5,
    unit: "% of crossings",
    scale: 100,
    decimals: 0,
  },
  {
    key: "driveways",
    label: "Driveways",
    max: 5,
    step: 0.25,
    unit: "/100 m",
    scale: 1,
    decimals: 2,
  },
];

const fresh = (): WorldChoice => ({ seed: 42, densities: { ...DEFAULT_DENSITIES } });

/** Seed and densities of the streaming city. The engine generates it; nothing is drawn here. */
export function WorldEditor({ applied, current, disabledReason, onApply }: Props) {
  const [draft, setDraft] = useState<WorldChoice>(() => structuredClone(applied ?? fresh()));
  const [more, setMore] = useState(false);
  const problem = worldProblem(draft);
  const density = (d: Density) => (
    <Slider
      key={d.key}
      label={d.label}
      unit={d.unit}
      min={0}
      max={d.max}
      step={d.step}
      decimals={d.decimals}
      value={draft.densities[d.key] * d.scale}
      disabledReason={disabledReason}
      onValueChange={(value) =>
        setDraft({ ...draft, densities: { ...draft.densities, [d.key]: value / d.scale } })
      }
    />
  );
  return (
    <section className="flex flex-col gap-16" aria-label="World">
      <p className="text-12 text-fg-2">
        A city that grows around the wheel as you ride: streets, curbs, crosswalks, potholes, stairs
        and ramps. The same seed always builds the same city.
      </p>
      <div className="flex items-end gap-8">
        <label className="flex min-w-0 flex-1 flex-col gap-8 text-12 text-fg-2">
          Seed
          <input
            className="num h-(--control-md) w-full min-w-0 rounded-8 border border-hairline bg-raised px-8 text-13 text-fg disabled:opacity-40"
            type="number"
            aria-label="World seed"
            value={Number.isFinite(draft.seed) ? draft.seed : ""}
            min={0}
            max={4294967295}
            step={1}
            disabled={!!disabledReason}
            onChange={(event) => setDraft({ ...draft, seed: event.currentTarget.valueAsNumber })}
          />
        </label>
        <Button
          disabledReason={disabledReason ?? worldProblem({ ...draft, seed: 0 }) ?? undefined}
          onClick={() => {
            const world = { ...structuredClone(draft), seed: randomSeed() };
            setDraft(world);
            onApply(world);
          }}
        >
          New world
        </Button>
      </div>
      <div className="flex flex-col gap-16">{MAIN.map(density)}</div>
      <Button
        size="sm"
        variant="ghost"
        className="justify-start"
        aria-expanded={more}
        icon={<ChevronRight className={more ? "rotate-90" : undefined} />}
        onClick={() => setMore(!more)}
      >
        More densities
      </Button>
      {more && <div className="flex flex-col gap-16">{MORE.map(density)}</div>}
      <dl className="flex flex-col gap-8 text-12">
        <div className="flex justify-between gap-16">
          <dt className="text-fg-2">Current world</dt>
          <dd className="num">{current ? `seed ${current.spec.seed}` : "None"}</dd>
        </div>
        <div className="flex justify-between gap-16">
          <dt className="text-fg-2">Physics ground</dt>
          <dd className="num">3.1 cm around the wheel</dd>
        </div>
      </dl>
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
          Ride this world
        </Button>
        <Button
          variant="ghost"
          disabledReason={disabledReason}
          onClick={() => setDraft({ ...fresh(), seed: draft.seed })}
        >
          Default densities
        </Button>
      </div>
    </section>
  );
}
