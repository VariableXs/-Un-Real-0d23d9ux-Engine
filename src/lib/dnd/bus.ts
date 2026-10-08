import { createStore } from "../store";

/**
 * U-17 全局拖放总线（纯逻辑 + 轻量事件，可在 node 环境测试）：
 * - 三类负载（files / text / window）+ Drop 目标注册表 + 兼容性判定
 * - 拖拽会话态：startDrag → 悬停目标（hoverId）→ endDrag
 * - 收藏托盘（单槽）：跨虚拟桌面 / 跨场景拖放的「断链」用寄存解决——
 *   抓起 → 靠边停放（stash）→ 切换桌面 → 取出（unstash）→ 重新进入拖拽态。
 *   设计取舍：不做多槽队列，一次只寄存一份，语义简单且托盘 UI 足够小。
 *
 * 视觉层（DragGhost.tsx 的 DndLayer）只订阅本 store，不持有业务状态；
 * 目标组件通过 registerDropTarget + DOM 属性 data-dnd-target="<id>" 与总线关联。
 */

/** 拖放负载：kind 决定兼容性判定，其余为可选数据载荷。 */
export interface DndPayload {
  kind: "files" | "text" | "window";
  files?: string[];
  text?: string;
  winId?: string;
  /** 来源标识（拖拽幽灵上显示，如「写作 / 桌面图标」）。 */
  sourceLabel: string;
  /** 来源色条颜色（缺省用主题 accent）。 */
  sourceColor?: string;
}

/** 目标可接受的负载类型集合。 */
export type DndAccepts = Array<"files" | "text" | "window">;

export interface DropTarget {
  id: string;
  accepts: DndAccepts;
}

/** 兼容性判定结果：accept = 可放置；forbidden = 显示「禁止」态。 */
export type DndCompat = "accept" | "forbidden";

export interface DndState {
  /** 当前拖拽会话负载（null = 无会话）。 */
  payload: DndPayload | null;
  /** 拖拽中悬停命中的目标 id（null = 空白处 / 不兼容）。 */
  hoverId: string | null;
  /** 收藏托盘寄存物（单槽；null = 空）。 */
  stash: DndPayload | null;
  /** Drop 目标注册表（id → 目标定义）。 */
  targets: Record<string, DropTarget>;
}

export const dndStore = createStore<DndState>({
  payload: null,
  hoverId: null,
  stash: null,
  targets: {},
});

// ---------- selectors（视觉层/外部工具订阅用） ----------

export const selectDndActive = (s: DndState): boolean => s.payload !== null;
export const selectDndPayload = (s: DndState): DndPayload | null => s.payload;
export const selectDndHover = (s: DndState): string | null => s.hoverId;
export const selectDndStash = (s: DndState): DndPayload | null => s.stash;
export const selectDndTargets = (s: DndState): Record<string, DropTarget> => s.targets;
/** 托盘是否需要显示（有寄存物即显示）。 */
export const selectDndTrayVisible = (s: DndState): boolean => s.stash !== null;

// ---------- 目标注册表 ----------

/** 注册 Drop 目标（重复注册同 id = 更新 accepts）。 */
export function registerDropTarget(id: string, accepts: DndAccepts): void {
  if (!id) return;
  dndStore.setState((s) => ({
    targets: { ...s.targets, [id]: { id, accepts: [...accepts] } },
  }));
}

/** 注销目标（悬停恰好命中该目标时同步清除悬停态）。 */
export function unregisterDropTarget(id: string): void {
  dndStore.setState((s) => {
    if (!(id in s.targets)) return {};
    const targets = { ...s.targets };
    delete targets[id];
    return { targets, hoverId: s.hoverId === id ? null : s.hoverId };
  });
}

/** 兼容性矩阵判定（纯函数）。 */
export function compatibility(payload: DndPayload, accepts: DndAccepts): DndCompat {
  return accepts.includes(payload.kind) ? "accept" : "forbidden";
}

// ---------- 会话态 ----------

/** 开始一次拖拽会话（悬停态清零，寄存托盘不受影响）。 */
export function startDrag(payload: DndPayload): void {
  dndStore.setState({ payload, hoverId: null });
}

/** 结束会话（无论落点是否命中目标，都由视觉层调用）。 */
export function endDrag(): void {
  dndStore.setState({ payload: null, hoverId: null });
}

/** 视觉层 pointermove 命中/离开目标时更新悬停态（无变化不触发订阅）。 */
export function setDndHover(id: string | null): void {
  dndStore.setState((s) => (s.hoverId === id ? {} : { hoverId: id }));
}

// ---------- 收藏托盘（单槽） ----------

/**
 * 寄存当前拖拽（抓起 → 靠边停放）。
 * 无进行中的会话或槽位已被占用 → 拒绝（返回 false）。
 */
export function stash(): boolean {
  const s = dndStore.getState();
  if (!s.payload) return false;
  if (s.stash) return false; // 单槽：寄存期间再次 stash 拒绝
  dndStore.setState({ stash: s.payload, payload: null, hoverId: null });
  return true;
}

/**
 * 取出寄存物（点击托盘 → 重新进入拖拽态由调用方 startDrag 完成）。
 * 空槽返回 null。
 */
export function unstash(): DndPayload | null {
  const s = dndStore.getState();
  if (!s.stash) return null;
  dndStore.setState({ stash: null });
  return s.stash;
}
