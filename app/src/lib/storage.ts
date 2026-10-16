// localStorage with a namespace and no throwing: private windows and blocked storage fall back
// to defaults, and the app must render correctly without it.

const PREFIX = "euc-studio.";

export function readStored<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(PREFIX + key);
    return raw == null ? fallback : (JSON.parse(raw) as T);
  } catch {
    return fallback;
  }
}

export function writeStored(key: string, value: unknown): void {
  try {
    localStorage.setItem(PREFIX + key, JSON.stringify(value));
  } catch {
    // Storage full or blocked: the setting lasts for this session only.
  }
}
