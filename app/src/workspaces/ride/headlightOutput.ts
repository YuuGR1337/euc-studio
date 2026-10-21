/** GPIO drive, not measured luminous output. Missing and non-finite values stay unknown. */
export function headlightOutput(duty: number | null | undefined): {
  duty: number | null;
  state: "On" | "Off" | "Unknown";
} {
  if (typeof duty !== "number" || !Number.isFinite(duty)) return { duty: null, state: "Unknown" };
  const bounded = Math.min(1, Math.max(0, duty));
  return { duty: bounded, state: bounded > 0 ? "On" : "Off" };
}

/** Front view, from the owner's direct evidence: four circles in one horizontal row. */
const LENSES = [21, 39, 57, 75].map((cx) => ({ cx, cy: 24, radius: 6 }));

export function headlightPreviewModel(duty: number | null | undefined) {
  const output = headlightOutput(duty);
  return {
    ...output,
    unknown: output.duty === null,
    beamOpacity: output.duty ?? 0,
    lenses: LENSES.map((lens) => ({ ...lens, opacity: output.duty ?? 0 })),
  };
}
