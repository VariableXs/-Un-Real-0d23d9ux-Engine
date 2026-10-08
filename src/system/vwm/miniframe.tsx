import { useRef } from "react";
import { Pin, PinOff, X } from "lucide-react";
import { useI18n } from "../../i18n";
import { createStore, useStore } from "../../lib/store";
import { getMiniApp } from "../../apps/mini/registry";
import "../../styles/ai08-mini.css";

/**
 * U-18 迷你窗口框架与图层管理（miniframe）：
 * - openMiniApp / closeMiniApp / pinMiniApp + MiniAppsLayer（挂载于桌面壳层）
 * - 每个打开的 mini：无边框圆角胶囊头（拖动移动 / 置顶切换 / 关闭 ×），
 *   宽取自注册表（280–360），高度自适应内容
 * - 图层语义：整个迷你层默认 always-on-top——zIndex 550 高于桌面/任务栏
 *   （VWM 窗口带 ≤80+）低于模态（600/700/1000）；层内「置顶」= 置顶带
 *   （z + PIN_BOOST）恒浮于普通迷你窗口之上，层间相对序保留
 * - Esc 关闭（焦点在窗口内时）：keydown 自窗口内容冒泡而来即命中；
 *   关闭后 stopPropagation，不再向下传给文档级监听
 * - 位置持久化：localStorage variable:mini:layout:v1（拖拽结束/关窗时落盘，
 *   重开按记忆位置回归，出屏钳制回视口内）
 * - 重复 open → 聚焦（置于最上层）；未注册 id → 静默拒绝
 * - 设计取舍：不复用 VWM 的 VirtualWindowFrame（那套是最小化/贴靠/多开
 *   的重窗口语义）；迷你窗口是「桌面挂件」轻语义，独立小 store。
 *
 * 循环依赖说明：registry → MiniNotes → 本文件 → registry。所有跨环引用
 * 均为函数声明（hoisting 安全）且仅在运行时（事件/渲染）调用。
 */

const LAYOUT_KEY = "variable:mini:layout:v1";
/** 置顶带偏移：置顶窗口恒浮于普通迷你窗口之上。 */
const PIN_BOOST = 100000;

export interface MiniWin {
  id: string;
  x: number;
  y: number;
  /** 层内基础 z（渲染时按 pinned 加带）。 */
  z: number;
  pinned: boolean;
}

export interface MiniLayerState {
  wins: MiniWin[];
  topZ: number;
  focusedId: string | null;
}

export const miniStore = createStore<MiniLayerState>({
  wins: [],
  topZ: 0,
  focusedId: null,
});

// ---------- 位置持久化 ----------

function loadLayout(): Record<string, { x: number; y: number }> {
  try {
    const raw = JSON.parse(localStorage.getItem(LAYOUT_KEY) ?? "{}") as Record<string, { x: number; y: number }>;
    return raw && typeof raw === "object" ? raw : {};
  } catch {
    return {}; // 损坏数据 → 全部回到默认摆位
  }
}

function persistLayout(): void {
  try {
    // 合并写入：保留「已记忆但当前未打开」的条目（关窗记忆不被清掉）
    const all = loadLayout();
    for (const w of miniStore.getState().wins) all[w.id] = { x: Math.round(w.x), y: Math.round(w.y) };
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(all));
  } catch {
    /* storage 满/被禁 → 本次不落盘 */
  }
}

/** 记录单个窗口位置（关窗时保存「最后位置」，下次 open 回原位）。 */
function saveOne(id: string, x: number, y: number): void {
  try {
    const all = loadLayout();
    all[id] = { x: Math.round(x), y: Math.round(y) };
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(all));
  } catch {
    /* storage 满/被禁 → 本次不落盘 */
  }
}

/** 视口（node 测试环境无窗口尺寸 → 兜底 1280×800）。 */
function viewport(): { w: number; h: number } {
  if (typeof window === "undefined" || !window.innerWidth) return { w: 1280, h: 800 };
  return { w: window.innerWidth, h: window.innerHeight };
}

/** 位置钳制：完整留在视口内（顶栏之上留 8px，底部留出 96px 任务栏区）。 */
function clampPos(x: number, y: number, width: number, vw: number, vh: number): { x: number; y: number } {
  return {
    x: Math.min(Math.max(x, 8), Math.max(8, vw - width - 8)),
    y: Math.min(Math.max(y, 8), Math.max(8, vh - 96)),
  };
}

// ---------- 图层动作 ----------

/** 打开迷你应用：已打开 → 聚焦（置顶）；未注册 → 静默拒绝。 */
export function openMiniApp(id: string): void {
  const s = miniStore.getState();
  if (s.wins.some((w) => w.id === id)) {
    focusMiniApp(id);
    return;
  }
  const def = getMiniApp(id);
  if (!def) return;
  const { w: vw, h: vh } = viewport();
  const saved = loadLayout()[id];
  const n = s.wins.length;
  // 无记忆位置 → 右上瀑布错位摆开，避免多开完全重叠
  const base = saved ?? { x: vw - def.width - 40 - n * 28, y: 72 + n * 36 };
  const pos = clampPos(base.x, base.y, def.width, vw, vh);
  const z = s.topZ + 1;
  miniStore.setState((st) => ({
    wins: [...st.wins, { id, x: pos.x, y: pos.y, z, pinned: false }],
    topZ: z,
    focusedId: id,
  }));
}

/** 关闭：关窗即记忆最后位置（下次 open 回原位，与 VWM 几何记忆语义一致）。 */
export function closeMiniApp(id: string): void {
  const closing = miniStore.getState().wins.find((w) => w.id === id);
  miniStore.setState((st) => {
    const wins = st.wins.filter((w) => w.id !== id);
    if (wins.length === st.wins.length) return {};
    const next = wins.length > 0 ? wins[wins.length - 1] : undefined;
    return { wins, focusedId: st.focusedId === id ? next?.id ?? null : st.focusedId };
  });
  if (closing) saveOne(id, closing.x, closing.y);
}

/** 聚焦：置于最上层（置顶带之下）。 */
export function focusMiniApp(id: string): void {
  const s = miniStore.getState();
  if (!s.wins.some((w) => w.id === id)) return;
  const z = s.topZ + 1;
  miniStore.setState((st) => ({
    wins: st.wins.map((w) => (w.id === id ? { ...w, z } : w)),
    topZ: z,
    focusedId: id,
  }));
}

/** 指针聚焦：已聚焦不重排（避免拖拽/点击时无谓 z 抖动）。 */
function pointerFocusMini(id: string): void {
  const s = miniStore.getState();
  if (s.focusedId === id) return;
  focusMiniApp(id);
}

/** 置顶切换（便签等应用也可调用；开启即抬到置顶带最上）。 */
export function pinMiniApp(id: string, on: boolean): void {
  const s = miniStore.getState();
  const w = s.wins.find((x) => x.id === id);
  if (!w || w.pinned === on) return;
  if (!on) {
    miniStore.setState((st) => ({ wins: st.wins.map((x) => (x.id === id ? { ...x, pinned: false } : x)) }));
    return;
  }
  const z = s.topZ + 1;
  miniStore.setState((st) => ({
    wins: st.wins.map((x) => (x.id === id ? { ...x, pinned: true, z } : x)),
    topZ: z,
  }));
}

/** 移动（钳制在视口内；拖拽期间每次 pointermove 调用）。 */
export function moveMiniApp(id: string, x: number, y: number): void {
  const def = getMiniApp(id);
  const { w: vw, h: vh } = viewport();
  const pos = clampPos(x, y, def?.width ?? 300, vw, vh);
  miniStore.setState((st) => ({
    wins: st.wins.map((w) => (w.id === id ? { ...w, x: Math.round(pos.x), y: Math.round(pos.y) } : w)),
  }));
}

// ---------- 渲染 ----------

/** 单个迷你窗口。 */
function MiniFrame(props: { id: string }): React.ReactElement | null {
  const { t } = useI18n();
  const win = useStore(miniStore, (s) => s.wins.find((w) => w.id === props.id) ?? null);
  const focused = useStore(miniStore, (s) => s.focusedId === props.id);
  const dragRef = useRef<{ px: number; py: number; ox: number; oy: number } | null>(null);
  const app = getMiniApp(props.id);
  if (!win || !app) return null;

  const onHeaderDown = (e: React.PointerEvent<HTMLElement>): void => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return; // 按钮区不触发拖动
    pointerFocusMini(props.id);
    dragRef.current = { px: e.clientX, py: e.clientY, ox: win.x, oy: win.y };
    e.currentTarget.setPointerCapture(e.pointerId); // 快速甩动不丢事件
  };

  const onHeaderMove = (e: React.PointerEvent<HTMLElement>): void => {
    const d = dragRef.current;
    if (!d) return;
    moveMiniApp(props.id, d.ox + (e.clientX - d.px), d.oy + (e.clientY - d.py));
  };

  const onHeaderUp = (): void => {
    if (!dragRef.current) return;
    dragRef.current = null;
    persistLayout(); // 拖拽结束落盘
  };

  // Esc 关闭：事件自窗口内容冒泡而来（= 焦点在窗口内）；消费掉不再下传
  const onKeyDown = (e: React.KeyboardEvent<HTMLElement>): void => {
    if (e.key !== "Escape") return;
    closeMiniApp(props.id);
    e.stopPropagation();
  };

  const Body = app.render;
  return (
    <section
      className={`mini-frame${focused ? " focused" : ""}${win.pinned ? " pinned" : ""}`}
      style={{ left: win.x, top: win.y, width: app.width, zIndex: win.pinned ? win.z + PIN_BOOST : win.z }}
      role="dialog"
      aria-label={t(app.titleKey)}
      onPointerDown={() => pointerFocusMini(props.id)}
      onKeyDown={onKeyDown}
    >
      <header
        className="mini-cap"
        onPointerDown={onHeaderDown}
        onPointerMove={onHeaderMove}
        onPointerUp={onHeaderUp}
        onPointerCancel={onHeaderUp}
      >
        <span className="mini-cap-dot" />
        <span className="mini-cap-title ellipsis">{t(app.titleKey)}</span>
        <button
          type="button"
          className="mini-cap-btn"
          onClick={() => pinMiniApp(props.id, !win.pinned)}
          aria-label={win.pinned ? t("miniUnpin") : t("miniPin")}
          title={win.pinned ? t("miniUnpin") : t("miniPin")}
        >
          {win.pinned ? <Pin size={13} /> : <PinOff size={13} />}
        </button>
        <button
          type="button"
          className="mini-cap-btn mini-cap-close"
          onClick={() => closeMiniApp(props.id)}
          aria-label={t("miniClose")}
          title={t("miniClose")}
        >
          <X size={13} />
        </button>
      </header>
      <div className="mini-body">
        <Body />
      </div>
    </section>
  );
}

/**
 * 迷你应用图层（挂载于桌面壳层；无打开项时不渲染任何节点）。
 * 订阅收敛到「打开的 id 序列」字符串：拖动/置顶不触发整层重渲染，
 * 各窗口自行订阅自身几何。
 */
export function MiniAppsLayer(): React.ReactElement | null {
  const ids = useStore(miniStore, (s) => s.wins.map((w) => w.id).join("\n"));
  if (!ids) return null;
  return (
    <div className="mini-layer">
      {ids.split("\n").map((id) => (
        <MiniFrame key={id} id={id} />
      ))}
    </div>
  );
}
