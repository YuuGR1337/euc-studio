import type { ReactNode } from "react";
import { Pill } from "../ui/Pill.tsx";

export interface Feature {
  icon: ReactNode;
  title: string;
  text: string;
}

export interface EmptyWorkspaceProps {
  icon: ReactNode;
  name: string;
  title: string;
  lede: string;
  milestone: string;
  features: Feature[];
  actions: ReactNode;
  preview: ReactNode;
  previewLabel: string;
}

/** A workspace that is not built yet: what will live here, when, and what to do meanwhile. */
export function EmptyWorkspace({
  icon,
  name,
  title,
  lede,
  milestone,
  features,
  actions,
  preview,
  previewLabel,
}: EmptyWorkspaceProps) {
  return (
    <div className="h-full p-8">
      <div className="flex h-full overflow-auto rounded-12 border border-hairline bg-bg">
        <div className="m-auto grid w-full max-w-[1040px] grid-cols-[minmax(0,1fr)_minmax(0,1fr)] items-center gap-48 px-48 py-32 max-[1279px]:gap-32 max-[1279px]:px-32">
          <div className="flex flex-col">
            <div className="flex items-center gap-8">
              <span className="grid size-[32px] place-items-center rounded-8 border border-hairline bg-card text-accent-text [&_svg]:size-[18px]">
                {icon}
              </span>
              <span className="text-13 font-semibold text-fg-2">{name}</span>
              <Pill tone="neutral" className="ml-4">
                {milestone}
              </Pill>
            </div>
            <h1 className="mt-24 text-28 font-bold text-balance text-fg">{title}</h1>
            <p className="mt-8 max-w-[440px] text-15 text-pretty text-fg-2">{lede}</p>
            <ul className="mt-24 flex flex-col gap-16">
              {features.map((f) => (
                <li key={f.title} className="flex gap-16">
                  <span className="mt-2 text-fg-3 [&_svg]:size-[16px]">{f.icon}</span>
                  <span className="flex flex-col">
                    <span className="text-13 font-semibold text-fg">{f.title}</span>
                    <span className="text-13 text-pretty text-fg-2">{f.text}</span>
                  </span>
                </li>
              ))}
            </ul>
            <div className="mt-32 flex flex-wrap items-center gap-8">{actions}</div>
          </div>
          <figure className="flex flex-col gap-8">
            <div className="overflow-hidden rounded-16 border border-hairline bg-card">
              {preview}
            </div>
            <figcaption className="px-4 text-12 text-fg-3">{previewLabel}</figcaption>
          </figure>
        </div>
      </div>
    </div>
  );
}
