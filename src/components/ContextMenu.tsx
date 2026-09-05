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
  /** 批次B（规格 4.6/36）：子菜单 —— 悬停/ArrowRight 展开，边缘自动翻转。 */
  children?: MenuItem[];
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

// ---------- 批次B：键盘导航辅助 ----------

/** sel = 从根到当前高亮层的索引路径：[] → 无高亮；[2] → 根第 2 项；[2,0] → 根 2 的子菜单第 0 项。 */
type SelPath = number[];

function enabledIdx(items: MenuItem[]): number[] {
  const out: number[] = [];
  items.forEach((it, i) => {
    if (!it.separator && !it.disabled) out.push(i);
  });
  return out;
}

function step(items: MenuItem[], sel: SelPath, dir: 1 | -1): SelPath {
  const depth = sel.length - 1;
  const list = resolveList(items, sel.slice(0, depth));
  const idx = sel[depth] ?? -1;
  const en = enabledIdx(list);
  if (en.length === 0) return sel;
  const first = en[0] ?? -1;
  const last = en[en.length - 1] ?? -1;
  if (idx < 0) return [...sel.slice(0, depth), dir === 1 ? first : last];
  const at = en.indexOf(idx);
  const next = en[(at + dir + en.length) % en.length] ?? first;
  return [...sel.slice(0, depth), next];
}

function resolveList(items: MenuItem[], open: number[]): MenuItem[] {
  let list = items;
  for (const i of open) {
    const it = list[i];
    if (!it?.children) break;
    list = it.children;
  }
  return list;
}

// ---------- 递归菜单层（根 + 任意深度子菜单共用） ----------

function MenuLevel(props: {
  items: MenuItem[];
  prefix: SelPath;
  sel: SelPath;
  setSel: (s: SelPath) => void;
  onActivate: (item: MenuItem) => void;
}): React.ReactElement {
  const { items, prefix, sel, setSel, onActivate } = props;
  const btnRefs = useRef(new Map<number, HTMLButtonElement>());
  const subRef = useRef<HTMLDivElement>(null);
  const [subPos, setSubPos] = useState<{ x: number; y: number } | null>(null);

  const depth = prefix.length;
  const curIdx = sel.length > depth ? sel[depth] ?? -1 : -1;
  const curItem = curIdx >= 0 ? items[curIdx] : undefined;
  const subOpen = curItem?.children !== undefined && curItem.children.length > 0;

  // 子菜单定位 + 边缘翻转（右溢出→翻左侧；下溢出→上移钳制）
  useLayoutEffect(() => {
    if (!subOpen || !subRef.current) {
      setSubPos(null);
      return;
    }
    const btn = btnRefs.current.get(curIdx);
    const sub = subRef.current;
    if (!btn) return;
    const br = btn.getBoundingClientRect();
    const sr = sub.getBoundingClientRect();
    const M = 8;
    let x = br.right - 3;
    if (x + sr.width > window.innerWidth - M) x = Math.max(M, br.left - sr.width + 3);
    let y = br.top - 4;
    if (y + sr.height > window.innerHeight - M) y = Math.max(M, window.innerHeight - M - sr.height);
    setSubPos({ x, y });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [subOpen, curIdx, items]);

  return (
    <>
      {items.map((item, i) =>
        item.separator ? (
          <hr key={`sep-${i}`} className="ctx-sep" />
        ) : (
          <button
            key={item.label ?? i}
            ref={(el) => {
              if (el) btnRefs.current.set(i, el);
              else btnRefs.current.delete(i);
            }}
            type="button"
            role="menuitem"
            aria-haspopup={item.children ? "menu" : undefined}
            className={`ctx-item ${item.danger ? "danger" : ""}${i === curIdx ? " hl" : ""}`}
            disabled={item.disabled}
            onMouseEnter={() => setSel([...prefix, i])}
            onClick={(e) => {
              e.stopPropagation();
              if (item.children?.length) {
                // 点击父项 = 展开/收起
                if (subOpen && sel.length > depth + 1) setSel([...prefix, i]);
                else {
                  const en = enabledIdx(item.children);
                  const first = en[0];
                  setSel(first === undefined ? [...prefix, i] : [...prefix, i, first]);
                }
              } else {
                onActivate(item);
              }
            }}
          >
            {item.icon && <span className="ctx-icon">{item.icon}</span>}
            <span>{item.label}</span>
            {item.checked && <span className="ctx-check">✓</span>}
            {item.children?.length ? <span className="ctx-sub-arrow">▸</span> : null}
          </button>
        ),
      )}
      {subOpen && subPos && (
        <div
          ref={subRef}
          className="ctx-menu ctx-sub"
          style={{ left: subPos.x, top: subPos.y }}
          role="menu"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={(e) => e.stopPropagation()}
        >
          <MenuLevel
            items={curItem!.children!}
            prefix={[...prefix, curIdx]}
            sel={sel}
            setSel={setSel}
            onActivate={onActivate}
          />
        </div>
      )}
    </>
  );
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
  /** 批次B：键盘高亮路径（悬停与键盘共用同一 sel 状态）；ref 供窗口级键盘处理器同步读取。 */
  const [sel, setSel] = useState<SelPath>([]);
  const selRef = useRef<SelPath>([]);
  useEffect(() => {
    selRef.current = sel;
  }, [sel]);

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
    setSel([]);
    const onKey = (e: KeyboardEvent): void => {
      // 批次B：方向键/Enter 导航（菜单打开时拦截）
      if (e.key === "Escape") {
        e.preventDefault();
        dismiss();
        return;
      }
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSel((s) => step(menu.items, s, 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSel((s) => step(menu.items, s, -1));
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        setSel((s) => {
          const depth = s.length - 1;
          const list = resolveList(menu.items, s.slice(0, depth));
          const item = list[s[depth] ?? -1];
          if (!item?.children?.length) return s;
          const en = enabledIdx(item.children);
          const first = en[0];
          return first === undefined ? s : [...s, first];
        });
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        setSel((s) => (s.length > 1 ? s.slice(0, -1) : s));
      } else if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        const s = selRef.current;
        const depth = s.length - 1;
        const list = resolveList(menu.items, s.slice(0, depth));
        const item = list[s[depth] ?? -1];
        if (!item || item.disabled) return;
        if (item.children?.length) {
          const en = enabledIdx(item.children);
          const first = en[0];
          if (first !== undefined) setSel([...s, first]);
          else setSel(s.slice(0, depth + 1));
        } else {
          dismiss();
          window.setTimeout(() => item.onClick?.(), 0);
        }
      }
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
      <MenuLevel
        items={menu.items}
        prefix={[]}
        sel={sel}
        setSel={setSel}
        onActivate={(item) => {
          dismiss();
          item.onClick?.();
        }}
      />
    </div>
  );
}
