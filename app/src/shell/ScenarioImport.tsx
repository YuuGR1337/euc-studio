import { useRef, useState } from "react";
import { engine } from "../engine/index.ts";
import { FileCode2 } from "../ui/icons.tsx";
import { Button, IconButton } from "../ui/Button.tsx";

/** Loads explicit user-selected scenario text. No local path reaches the simulator. */
export function ScenarioImport() {
  const input = useRef<HTMLInputElement>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);

  return (
    <div className="relative flex shrink-0 items-center">
      <IconButton
        label="Open scenario file"
        icon={<FileCode2 />}
        variant="ghost"
        disabledReason={
          !engine.importScenario
            ? "Scenario import needs the desktop engine"
            : busy
              ? "Loading the scenario"
              : undefined
        }
        onClick={() => input.current?.click()}
      />
      <input
        ref={input}
        type="file"
        accept=".toml,text/plain"
        className="hidden"
        aria-label="Open a scenario TOML file"
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          event.currentTarget.value = "";
          if (!file || !engine.importScenario) return;
          const source = engine;
          const importScenario = engine.importScenario.bind(engine);
          if (file.size > 1_048_576) {
            setError("Scenario files must be at most 1 MiB");
            return;
          }
          setBusy(true);
          setError("");
          void file
            .text()
            .then((text) => {
              if (engine !== source)
                throw new Error("The active engine changed. Select the scenario file again.");
              return importScenario(text);
            })
            .catch((cause: unknown) => {
              setError(String(cause));
            })
            .finally(() => setBusy(false));
        }}
      />
      {error && (
        <div
          role="alert"
          className="absolute top-full right-0 z-50 mt-8 w-[300px] rounded-8 border border-hairline-strong bg-card p-16 text-12 text-danger-text"
        >
          {error}
          <Button size="sm" variant="ghost" className="mt-8" onClick={() => setError("")}>
            Close
          </Button>
        </div>
      )}
    </div>
  );
}
