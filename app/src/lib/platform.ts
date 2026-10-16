// Platform details for shortcuts: ⌘ on macOS, Ctrl elsewhere.

export const IS_MAC =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

const GLYPHS: Record<string, string> = {
  mod: IS_MAC ? "⌘" : "Ctrl",
  shift: IS_MAC ? "⇧" : "Shift",
  alt: IS_MAC ? "⌥" : "Alt",
  enter: "↵",
  esc: "Esc",
  up: "↑",
  down: "↓",
  space: "Space",
};

/** "mod+k" → ["⌘", "K"] on macOS, ["Ctrl", "K"] elsewhere. */
export function shortcutKeys(shortcut: string): string[] {
  return shortcut.split("+").map((k) => GLYPHS[k] ?? (k.length === 1 ? k.toUpperCase() : k));
}
