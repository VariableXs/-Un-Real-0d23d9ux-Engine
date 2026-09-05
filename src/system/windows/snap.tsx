import { useEffect, useState } from "react";
import {
  currentMonitor,
  getCurrentWindow,
  PhysicalPosition,
  PhysicalSize,
} from "@tauri-apps/api/window";
import { emit, listen } from "@tauri-apps/api/event";
import { isTauriRuntime } from "../../entries/runtime";

/**
 * 批次D（规格 4.5）— 窗口分屏与贴靠：
 * - Win+方向键：applySnap（全局快捷键 → 各窗口自行判断自己是否焦点）
 * - 拖拽贴靠：标题栏拖动期间轮询窗口位置 → 接近屏幕边缘显示贴靠预览 →
 *   松手（位置停止变化）贴靠。预览通过 `snap://preview` 事件交给桌面窗口渲染。
 *
 * 桌面环境窗口全屏覆盖，不适合被贴靠 —— applySnap / 拖拽贴靠跳过 desktop 窗口。
 */

export type SnapDir = "left" | "right" | "up" | "down";

export interface SnapRect {
  /** 物理像素 */
  x: number;
  y: number;
  w: number;
  h: number;
  /** 缩放比（预览层换算 CSS 像素用） */
  sf: number;
}

interface WorkArea {
  x: number;
  y: number;
  width: number;
  height: number;
}

const EDGE = 10; // 贴靠判定边距（物理像素）

async function getWorkArea(): Promise<WorkArea | null> {
  try {
    const mon = await currentMonitor();
    if (!mon) return null;
    // JS Monitor.workArea = { x, y, width, height }（物理像素）；缺省回退整屏
    const wa = (mon as unknown as { workArea?: WorkArea }).workArea;
    if (wa && wa.width > 0) return wa;
    return { x: mon.position.x, y: mon.position.y, width: mon.size.width, height: mon.size.height };
  } catch {
    return null;
  }
}

async function applyRect(win: ReturnType<typeof getCurrentWindow>, r: WorkArea): Promise<void> {
  await win.setPosition(new PhysicalPosition(r.x, r.y));
  await win.setSize(new PhysicalSize(Math.round(r.width), Math.round(r.height)));
}

/** Win+方向键：仅当本窗口持有焦点时执行（事件广播给所有窗口，谁焦点谁吃）。 */
export async function applySnap(dir: SnapDir): Promise<void> {
  if (!isTauriRuntime()) return;
  try {
    const win = getCurrentWindow();
    if (win.label === "desktop") return; // 桌面常驻覆盖，不参与贴靠
    if (!(await win.isFocused().catch(() => false))) return;
    const wa = await getWorkArea();
    if (!wa) return;
    const target = snapZoneFor(dir, wa);
    if (dir === "down") {
      const prev = restore.pop();
      if (prev) await applyRect(win, prev);
      else await win.minimize();
      return;
    }
    // 保存当前几何用于 Win+↓ 还原
    const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()]);
    restore.push({ x: pos.x, y: pos.y, width: size.width, height: size.height });
    if (restore.length > 4) restore.shift();
    await applyRect(win, target);
  } catch {
    /* window gone / API unavailable */
  }
}

const restore: WorkArea[] = [];

function snapZoneFor(dir: SnapDir, wa: WorkArea): WorkArea {
  const half = Math.round(wa.width / 2);
  if (dir === "left") return { x: wa.x, y: wa.y, width: half, height: wa.height };
  if (dir === "right") return { x: wa.x + wa.width - half, y: wa.y, width: half, height: wa.height };
  return { x: wa.x, y: wa.y, width: wa.width, height: wa.height }; // up
}

// ---------- 拖拽贴靠 ----------

interface DragTracker {
  timer: number;
  startX: number;
  startY: number;
  moved: boolean;
  still: number;
  zone: WorkArea | null;
}

let tracker: DragTracker | null = null;

/** 由拖拽位置推贴靠区：顶=最大化、左右边缘=半屏、四角=1/4。 */
function zoneFromPos(px: number, py: number, w: number, wa: WorkArea): WorkArea | null {
  const nearTop = py <= wa.y + EDGE;
  const nearBottom = py + 40 >= wa.y + wa.height - EDGE;
  const nearLeft = px <= wa.x + EDGE;
  const nearRight = px + w >= wa.x + wa.width - EDGE;
  const half = Math.round(wa.width / 2);
  const halfH = Math.round(wa.height / 2);
  if (nearTop && nearLeft) return { x: wa.x, y: wa.y, width: half, height: halfH };
  if (nearTop && nearRight) return { x: wa.x + wa.width - half, y: wa.y, width: half, height: halfH };
  if (nearBottom && nearLeft) return { x: wa.x, y: wa.y + wa.height - halfH, width: half, height: halfH };
  if (nearBottom && nearRight) return { x: wa.x + wa.width - half, y: wa.y + wa.height - halfH, width: half, height: halfH };
  if (nearTop) return { x: wa.x, y: wa.y, width: wa.width, height: wa.height };
  if (nearLeft) return { x: wa.x, y: wa.y, width: half, height: wa.height };
  if (nearRight) return { x: wa.x + wa.width - half, y: wa.y, width: half, height: wa.height };
  return null;
}

/** 标题栏按下时开始跟踪（TitleBar pointerdown 调用；松手由"位置停止变化"启发判定）。 */
export function beginDragSnap(): void {
  if (!isTauriRuntime() || tracker) return;
  const win = getCurrentWindow();
  if (win.label === "desktop") return; // 桌面窗口不可拖动
  void (async () => {
    const wa = await getWorkArea();
    if (!wa) return;
    const t: DragTracker = { timer: 0, startX: 0, startY: 0, moved: false, still: 99, zone: null };
    tracker = t;
    t.timer = window.setInterval(() => {
      void (async () => {
        try {
          const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()]);
          if (!t.moved) {
            if (t.still === 99) {
              t.startX = pos.x;
              t.startY = pos.y;
              t.still = 0;
              return;
            }
            if (Math.abs(pos.x - t.startX) + Math.abs(pos.y - t.startY) > 4) t.moved = true;
          }
          if (!t.moved) return;
          // 位置连续 3 次不变 → 视为松手
          if (pos.x === t.startX && pos.y === t.startY) {
            t.still += 1;
            if (t.still >= 3) {
              endDragSnap(true);
              return;
            }
          } else {
            t.still = 0;
            t.startX = pos.x;
            t.startY = pos.y;
          }
          const zone = zoneFromPos(pos.x, pos.y, size.width, wa);
          if (JSON.stringify(zone) !== JSON.stringify(t.zone)) {
            t.zone = zone;
            void emit("snap://preview", zone ? { x: zone.x, y: zone.y, w: zone.width, h: zone.height, sf: 0 } : null).catch(() => {});
          }
        } catch {
          endDragSnap(false);
        }
      })();
    }, 50);
  })();
}

/** 结束拖拽跟踪。apply=true 且当前有贴靠区 → 应用几何。 */
export function endDragSnap(apply: boolean): void {
  const t = tracker;
  if (!t) return;
  tracker = null;
  window.clearInterval(t.timer);
  if (apply && t.zone) {
    const win = getCurrentWindow();
    void applyRect(win, t.zone).catch(() => {});
  }
  void emit("snap://preview", null).catch(() => {});
}

/** 贴靠预览浮层（桌面窗口渲染；坐标物理像素 → CSS 像素）。 */
export function SnapPreviewHost(): React.ReactElement | null {
  const [rect, setRect] = useState<(SnapRect & { sf: number }) | null>(null);
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let un: (() => void) | undefined;
    void listen<SnapRect | null>("snap://preview", async (e) => {
      if (!e.payload) {
        setRect(null);
        return;
      }
      const sf = await getCurrentWindow().scaleFactor().catch(() => 1);
      setRect({ ...e.payload, sf });
    }).then((f) => {
      un = f;
    });
    return () => un?.();
  }, []);
  if (!rect || rect.w <= 0 || rect.h <= 0) return null;
  const sf = rect.sf || 1;
  return (
    <div
      className="snap-preview"
      aria-hidden
      style={{
        left: rect.x / sf,
        top: rect.y / sf,
        width: rect.w / sf,
        height: rect.h / sf,
      }}
    />
  );
}
