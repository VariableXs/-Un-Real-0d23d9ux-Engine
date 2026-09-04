import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createStore, useStore } from "../lib/store";
import { uiStore } from "../state/uiStore";

export interface MenuItem {
  label?: string;
  icon?: ReactNode;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
  separator?: boolean;
  onClick?: () => void;
}

interface MenuState {
  x: number;
  y: number;
  items: MenuItem[];
}

const menuStore = createStore<{ current: MenuState | null }>({ current: null });

/** Identity of the menu currently playing its fade-out (module-level because
 *  the host is a singleton; keyed per menu so concurrent fades of successive
 *  menus never block each other's dismissal). */
let fadingId: MenuState | null = null;

/** Mirror of the state machine's `activeContextMenu` flag. */
function useContextMenuActive(): string | null {
  return useStore(uiStore, (s) => s.activeContextMenu);
}

export function openContextMenu(x: number, y: number, items: MenuItem[]): void {
  closeContextMenu();
  menuStore.setState({ current: { x, y, items } });
  // Mirror into the global canvas state machine so a blank-canvas click can
  // force-destroy the menu through `resetGlobalCanvasInteraction()`.
  uiStore.setState({ activeContextMenu: `ctx-${Date.now()}-${Math.floor(Math.random() * 1e6)}` });
}

export function closeContextMenu(): void {
  if (menuStore.getState().current) {
    menuStore.setState({ current: null });
    if (uiStore.getState().activeContextMenu !== null) {
      uiStore.setState({ activeContextMenu: null });
    }
  }
}

/** Global context-menu host; clamps to window so menus are never clipped.
 *  Closing always routes through a short fade (module-4 dismissal protocol);
 *  the fade only completes if the store still holds the SAME menu, so a rapid
 *  reopen during the fade can never be swallowed. */
export function ContextMenuHost(): React.ReactElement | null {
  const menu = useStore(menuStore, (s) => s.current);
  const activeCtx = useContextMenuActive();
  const ref = useRef<HTMLDivElement>(null);
  /** Starts at the cursor so the pre-measure frame is already in the right
   *  neighborhood; the layout effect below flips/clamps BEFORE paint. */
  const [pos, setPos] = useState({ x: 0, y: 0 });
  /** Identity of the menu currently playing its fade-out (mirrored from the
   *  module-level fadingId for rendering). Keying the fade to the menu OBJECT
   *  (not a boolean) makes a submenu swap / rapid reopen race-proof: a
   *  brand-new menu can never inherit the previous fade class, not even for a
   *  single frame. */
  const [fadingMenu, setFadingMenu] = useState<MenuState | null>(null);

  const dismiss = (): void => {
    const cur = menuStore.getState().current;
    if (!cur || fadingId === cur) return;
    fadingId = cur;
    setFadingMenu(cur);
    window.setTimeout(() => {
      if (menuStore.getState().current === cur) closeContextMenu();
      if (fadingId === cur) {
        fadingId = null;
        setFadingMenu(null);
      }
    }, 130);
  };

  // State-machine channel: when the global machine nulls `activeContextMenu`
  // (blank-canvas pointer down), the menu runs its fade-out protocol.
  useEffect(() => {
    if (menu && activeCtx === null) dismiss();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeCtx, menu]);

  useEffect(() => {
    if (!menu) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") dismiss();
    };
    // Use capture+timeout so the opening click doesn't immediately close it.
    // CAPTURE phase is mandatory: a bubble-phase window listener would never
    // fire if any element between the target and window calls stopPropagation
    // — capture runs FIRST, before any handler in the tree can swallow it.
    const t = setTimeout(() => {
      window.addEventListener("mousedown", onMouseDown, true);
      window.addEventListener("pointerdown", onMouseDown, true);
      window.addEventListener("wheel", dismiss, { passive: true });
    }, 0);
    const onMouseDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) dismiss();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", dismiss);
    return () => {
      clearTimeout(t);
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("pointerdown", onMouseDown, true);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", dismiss);
      window.removeEventListener("wheel", dismiss);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menu]);

  // Bounding-collision layout (runs BEFORE paint → no visible jump/flash):
  //  - bottom overflow  → the menu flips to open UPWARD from the cursor;
  //  - right overflow   → shifted left of the cursor;
  //  - always clamped into [4, viewport-4]; CSS max-height + scroll guards
  //    menus taller than the viewport itself.
  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    const M = 8; // safety margin against the window edges
    let x = menu.x;
    let y = menu.y;
    if (y + r.height > window.innerHeight - M) {
      // flip: grow upward from the anchor point
      y = menu.y - r.height - 12;
    }
    if (y < M) y = M;
    if (x + r.width > window.innerWidth - M) {
      x = menu.x - r.width - 6;
    }
    if (x < M) x = M;
    setPos({ x, y });
  }, [menu]);

  if (!menu) return null;
  return (
    <div
      ref={ref}
      className={`ctx-menu ${fadingMenu === menu ? "fading" : ""}`}
      style={{ left: pos.x, top: pos.y }}
      role="menu"
      // Event-bubbling isolation: pointer events INSIDE the menu never reach
      // the canvas root's dismissal handler. Clicks OUTSIDE the menu are never
      // intercepted — the window-level mousedown above lets them pass through
      // untouched so blank-canvas cleanup fires with 100% reliability.
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => e.stopPropagation()}
    >
      {menu.items.map((item, i) =>
        item.separator ? (
          <hr key={`sep-${i}`} className="ctx-sep" />
        ) : (
          <button
            key={item.label ?? i}
            type="button"
            role="menuitem"
            className={`ctx-item ${item.danger ? "danger" : ""}`}
            disabled={item.disabled}
            onClick={(e) => {
              e.stopPropagation(); // menu-item clicks must not trigger canvas cleanup
              dismiss();
              item.onClick?.();
            }}
          >
            {item.icon && <span className="ctx-icon">{item.icon}</span>}
            <span>{item.label}</span>
            {item.checked && <span className="ctx-check">✓</span>}
          </button>
        ),
      )}
    </div>
  );
}
