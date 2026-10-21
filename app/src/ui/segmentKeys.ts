// Keyboard moves of a segmented control, as Radix's roving focus makes them: Left and Right step
// through the enabled options and wrap around; Home and Page Up go to the first, End and Page Down
// to the last.

/**
 * The option `key` moves the selection to from `value`, skipping disabled options; null for any
 * other key, or when nothing can be chosen.
 */
export function keyTarget<T extends string>(
  key: string,
  value: T,
  options: { value: T; disabledReason?: string | undefined }[],
): T | null {
  const enabled = options.filter((o) => !o.disabledReason).map((o) => o.value);
  const n = enabled.length;
  if (n === 0) return null;
  const i = enabled.indexOf(value);
  switch (key) {
    case "ArrowRight":
      return enabled[(i + 1) % n] ?? null;
    case "ArrowLeft":
      return enabled[i < 0 ? n - 1 : (i - 1 + n) % n] ?? null;
    case "Home":
    case "PageUp":
      return enabled[0] ?? null;
    case "End":
    case "PageDown":
      return enabled[n - 1] ?? null;
    default:
      return null;
  }
}
