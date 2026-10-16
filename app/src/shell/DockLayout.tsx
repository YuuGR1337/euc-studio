import { type ReactNode, useEffect, useRef, useState } from "react";
import {
  Group,
  type Layout,
  type LayoutChangedMeta,
  Panel,
  Separator,
  usePanelRef,
} from "react-resizable-panels";
import { motion as motionTokens } from "../design/tokens.ts";
import { cx } from "../lib/cx.ts";
import { readStored, writeStored } from "../lib/storage.ts";
import { type DockId, type WorkspaceId, useUi } from "../state/ui.ts";

const DEFAULTS = {
  h: { main: 70, right: 30 },
  v: { center: 72, bottom: 28 },
} satisfies Record<"h" | "v", Layout>;

const ANIMATION_MS = motionTokens.smooth.duration * 1000 + 50;

export interface DockLayoutProps {
  workspace: WorkspaceId;
  center: ReactNode;
  right?: ReactNode;
  bottom?: ReactNode;
}

/**
 * Center + right dock + bottom dock. Sizes persist per workspace in localStorage; opening and
 * closing a dock animates flex-grow with the smooth token (never while dragging).
 */
export function DockLayout({ workspace, center, right, bottom }: DockLayoutProps) {
  const docks = useUi((s) => s.docks[workspace]);
  const toggleDock = useUi((s) => s.toggleDock);
  const epoch = useUi((s) => s.layoutEpoch);
  const rightRef = usePanelRef();
  const bottomRef = usePanelRef();
  const [animating, setAnimating] = useState(false);
  const timer = useRef(0);

  const keyH = `layout.${workspace}.h`;
  const keyV = `layout.${workspace}.v`;
  const [initial] = useState(() => ({
    h: readStored<Layout>(keyH, DEFAULTS.h),
    v: readStored<Layout>(keyV, DEFAULTS.v),
  }));

  const animate = (fn: () => void) => {
    setAnimating(true);
    window.clearTimeout(timer.current);
    // Let the class apply before the sizes change so the transition runs.
    requestAnimationFrame(() => {
      fn();
      timer.current = window.setTimeout(() => setAnimating(false), ANIMATION_MS);
    });
  };

  // Store → panels: open/close docks when the setting changes (shortcut, palette, button).
  useEffect(() => {
    const sync = (ref: typeof rightRef, open: boolean) => {
      const p = ref.current;
      if (!p) return;
      if (open && p.isCollapsed()) animate(() => p.expand());
      if (!open && !p.isCollapsed()) animate(() => p.collapse());
    };
    sync(rightRef, docks.right);
    sync(bottomRef, docks.bottom);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docks.right, docks.bottom]);

  // Reset layout.
  const first = useRef(true);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    animate(() => {
      rightRef.current?.resize(`${DEFAULTS.h.right}%`);
      bottomRef.current?.resize(`${DEFAULTS.v.bottom}%`);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [epoch]);

  // Panels → store: dragging a dock shut (or open) updates the setting. Only user drags count;
  // imperative collapse/expand and re-registration must not echo back into the store.
  const syncFromLayout =
    (dock: DockId, key: string) => (layout: Layout, meta: LayoutChangedMeta) => {
      writeStored(key, layout);
      if (!meta.isUserInteraction) return;
      const open = (layout[dock] ?? 0) > 0.5;
      if (useUi.getState().docks[workspace][dock] !== open) toggleDock(dock, open);
    };

  // Separators are the 8px gutters between cards; a closed dock takes its gutter with it.
  const sepH = "shrink-0 transition-[width] duration-(--dur-smooth) ease-smooth";
  const sepV = "shrink-0 transition-[height] duration-(--dur-smooth) ease-smooth";

  return (
    <Group
      orientation="horizontal"
      id={`${workspace}-h`}
      defaultLayout={initial.h}
      onLayoutChanged={syncFromLayout("right", keyH)}
      className={cx("h-full w-full p-8", animating && "dock-animate")}
    >
      <Panel id="main" minSize="40%">
        <Group
          orientation="vertical"
          id={`${workspace}-v`}
          defaultLayout={initial.v}
          onLayoutChanged={syncFromLayout("bottom", keyV)}
          className={cx("h-full w-full", animating && "dock-animate")}
        >
          <Panel id="center" minSize="30%">
            {center}
          </Panel>
          {bottom && (
            <>
              <Separator
                className={cx(sepV, docks.bottom ? "h-[8px]" : "pointer-events-none h-0")}
              />
              <Panel
                id="bottom"
                data-testid="dock-bottom"
                panelRef={bottomRef}
                collapsible
                collapsedSize={0}
                minSize={140}
                maxSize="60%"
                className="h-full"
                style={{ overflow: "hidden" }}
              >
                {bottom}
              </Panel>
            </>
          )}
        </Group>
      </Panel>
      {right && (
        <>
          <Separator className={cx(sepH, docks.right ? "w-[8px]" : "pointer-events-none w-0")} />
          <Panel
            id="right"
            data-testid="dock-right"
            panelRef={rightRef}
            collapsible
            collapsedSize={0}
            minSize={300}
            maxSize="50%"
            style={{ overflow: "hidden" }}
          >
            {right}
          </Panel>
        </>
      )}
    </Group>
  );
}
