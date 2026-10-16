// Inside Tauri, forward uncaught errors and console errors/warnings to the terminal that started
// the app. Outside Tauri this does nothing.

import { invoke } from "@tauri-apps/api/core";

export function forwardWebviewLogs(): void {
  const send = (level: string, parts: unknown[]) => {
    const message = parts
      .map((p) => (p instanceof Error ? `${p.message}\n${p.stack ?? ""}` : String(p)))
      .join(" ");
    invoke("frontend_log", { level, message }).catch(() => {});
  };
  window.addEventListener("error", (e) => send("error", [e.error ?? e.message]));
  window.addEventListener("unhandledrejection", (e) => send("error", [e.reason]));
  document.addEventListener("securitypolicyviolation", (e) =>
    send("csp", [`${e.violatedDirective} blocked ${e.blockedURI || "inline"}`]),
  );
  for (const level of ["error", "warn"] as const) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      original(...args);
      send(level, args);
    };
  }
}
