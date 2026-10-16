import { forwardWebviewLogs } from "./tauri-log.ts";

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

if (inTauri) {
  forwardWebviewLogs();
  // macOS uses an overlay title bar; the run bar leaves room for the traffic lights and is the
  // drag region.
  if (/Mac/.test(navigator.userAgent)) document.documentElement.dataset.titlebar = "overlay";
}
