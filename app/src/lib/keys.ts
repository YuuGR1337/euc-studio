type KeyLike = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">;

/**
 * Normalizes a keydown into the form used by command shortcuts: "mod+k", "space", "?", "r".
 * `mod` is ⌘ on macOS and Ctrl elsewhere.
 */
export function comboFromEvent(e: KeyLike, mac: boolean): string {
  const parts: string[] = [];
  if (mac ? e.metaKey : e.ctrlKey) parts.push("mod");
  if (e.altKey) parts.push("alt");
  let key = e.key.toLowerCase();
  if (key === " ") key = "space";
  // A shifted symbol ("?") already says shift; letters, digits and space keep it explicit.
  if (e.shiftKey && /^([a-z0-9]|space)$/.test(key)) parts.push("shift");
  parts.push(key);
  return parts.join("+");
}
