import { createStore, useStore } from "../lib/store";
import {
  classifySource,
  loadLearn,
  recordAction,
  saveLearn,
  setOverride,
  type LearnTable,
  type NotifyStack,
} from "./notifySmart";

/**
 * 通知中心（M5 + AI-16 升级）：
 * - 数据只存内存（本会话），零网络、零持久化上报（存档走 Z-47 后端 SQLite，另行明示）；
 * - 来源：隐私占用提示（摄像头/麦克风）、硬件状态变化、系统级事件（托盘等）、提醒（Z-49）；
 * - U-51 通知交互进化：操作通知（actions）/ 进度通知（progress）/ 持久组（groupId 折叠）；
 * - U-51 排队礼仪：勿扰期间 action/progress 优先级进回放队列，勿扰结束后有序回放；
 * - Z-44 勿扰日程：手动勿扰优先于日程；勿扰期间通知静默入档（Z-47），提醒类可豁免；
 * - N-32 智能整理：notifySmart 学习表（划掉/点开）驱动分堆（即时/摘要/静默）。
 */

export interface NotifyItem {
  id: number;
  time: number;
  kind: "privacy" | "hardware" | "system" | "reminder";
  title: string;
  body: string;
  read: boolean;
  /** F-5.1：通知动作按钮（最多 2 个，规格 21.5）；无则不渲染按钮区。 */
  actions?: NotifyAction[];
  /** U-51：进度通知（常驻胶囊迷你条实时跟随任务；完成后自动转操作通知）。 */
  progress?: { current: number; total: number; label?: string } | null;
  /** U-51：持久组 id（同类通知折叠为组，如「3 个传输完成」）。 */
  groupId?: string;
  /** U-51：优先级（回放排序：action > progress > normal）。 */
  priority: "action" | "progress" | "normal";
  /** N-32：智能分堆结果（即时/摘要/静默）。 */
  stack?: NotifyStack;
  /** N-32：来源（学习键；缺省用 kind）。 */
  source?: string;
  /** Z-44：勿扰期间静默入档的标记（回放依据）。 */
  queued?: boolean;
}

/** F-5.1：通知动作 —— open-app（VWM 应用）/ open-third（登记的第三方软件）/ open-path（VWM 文件管理器）/ dismiss（忽略）。 */
export interface NotifyAction {
  label: string;
  type: "open-app" | "open-third" | "open-path" | "dismiss";
  /** open-app = VwmToolApp/AppMode；open-third = 第三方软件名；open-path = 容器内绝对路径。 */
  data?: string;
}

interface NotifyState {
  items: NotifyItem[];
  nextId: number;
  /** 批次C（规格 6.5.3）：勿扰模式（手动） —— 横幅静默，通知照常入历史，Ctrl+Shift+M 切换。 */
  dnd: boolean;
  /** Z-44：日程勿扰（NotifyRuntime 每分钟按 isDndActive 刷新；手动 ∪ 日程 = 生效勿扰）。 */
  schedDnd: boolean;
  /** U-51：勿扰/焦点舱期间的回放队列（按优先级有序回放）。 */
  replayQueue: NotifyItem[];
  /** N-32：本地学习表（划掉/点开频次）。 */
  learn: LearnTable;
}

const MAX_ITEMS = 50;

export const notifyStore = createStore<NotifyState>({
  items: [],
  nextId: 1,
  dnd: false,
  schedDnd: false,
  replayQueue: [],
  learn: loadLearn(),
});

export function useNotifications(): NotifyItem[] {
  return useStore(notifyStore, (s) => s.items);
}

/** 勿扰状态（手动开关；日程勿扰另行叠加，见 effectiveDnd）。 */
export function useDnd(): boolean {
  return useStore(notifyStore, (s) => s.dnd);
}

/** Z-44：生效勿扰（手动 ∪ 日程；UI 徽标与提示统一用此口径）。 */
export function useDndEffective(): boolean {
  return useStore(notifyStore, (s) => s.dnd || s.schedDnd);
}

/** Z-44：NotifyRuntime 每分钟回写日程勿扰判定；日程结束时触发回放。 */
export function setSchedDnd(active: boolean): void {
  const prev = notifyStore.getState().dnd || notifyStore.getState().schedDnd;
  notifyStore.setState({ schedDnd: active });
  const now = notifyStore.getState().dnd || notifyStore.getState().schedDnd;
  if (prev && !now) flushReplay();
}

export function toggleDnd(): boolean {
  const dnd = !notifyStore.getState().dnd;
  notifyStore.setState({ dnd });
  if (!dnd && !notifyStore.getState().schedDnd) flushReplay();
  return dnd;
}

export function useUnreadCount(): number {
  return useStore(notifyStore, (s) => s.items.reduce((n, it) => n + (it.read ? 0 : 1), 0));
}

export function useLearnTable(): LearnTable {
  return useStore(notifyStore, (s) => s.learn);
}

/** N-32：记录用户动作（点开/划掉）——学习信号。 */
export function recordNotifyAction(item: NotifyItem, action: "opened" | "dismissed"): void {
  const source = item.source ?? item.kind;
  const learn = recordAction(notifyStore.getState().learn, source, action);
  notifyStore.setState({ learn });
  saveLearn(learn);
}

/** N-32：用户覆盖某来源分堆（决定权在用户；null = 清除覆盖回到学习结论）。 */
export function overrideNotifyStack(source: string, stack: NotifyStack | null): void {
  const learn = setOverride(notifyStore.getState().learn, source, stack);
  notifyStore.setState({ learn });
  saveLearn(learn);
}

const priorityRank = { action: 0, progress: 1, normal: 2 } as const;

/**
 * 推送通知（U-51 全量参数；向后兼容旧三参调用）。
 * - 勿扰生效时（手动或日程，由调用方 dndEffective 判定）：入历史 + 入档 + 进回放队列（不响声音）；
 * - N-32 开启时按学习结论分堆：silent 堆不进 items（静默，仅入档）；
 * - Z-47：同步入后端存档（fire-and-forget，失败静默——存档是增益不是依赖）。
 */
export function pushNotify(
  kind: NotifyItem["kind"],
  title: string,
  body = "",
  actions?: NotifyAction[],
  opts?: {
    progress?: NotifyItem["progress"];
    groupId?: string;
    source?: string;
    /** Z-44 综合勿扰判定结果（手动 ∪ 日程）；缺省 = 仅手动开关。 */
    dndEffective?: boolean;
    /** Z-44：提醒类豁免（提醒/闹钟在勿扰下仍提示）。 */
    exempt?: boolean;
    /** N-32 开关（settings.notifySmart）。 */
    smart?: boolean;
    /** U-52/Z-45 声景参数。 */
    sound?: { theme?: import("../lib/settings").SoundThemeId; nightDamp?: boolean };
  },
): void {
  const state = notifyStore.getState();
  const id = state.nextId;
  const source = opts?.source ?? kind;
  const stack = opts?.smart === true ? classifySource(source, state.learn.sources[source], state.learn.overrides[source]) : undefined;
  const priority: NotifyItem["priority"] = actions && actions.length > 0 ? "action" : opts?.progress ? "progress" : "normal";
  const item: NotifyItem = {
    id,
    time: Date.now(),
    kind,
    title,
    body,
    read: false,
    actions,
    progress: opts?.progress ?? null,
    groupId: opts?.groupId,
    priority,
    stack,
    source,
  };

  const dndActive = opts?.dndEffective ?? (state.dnd || state.schedDnd);
  const silenced = dndActive && !(opts?.exempt === true || kind === "reminder");

  notifyStore.setState((s) => ({
    items:
      stack === "silent"
        ? s.items // N-32 静默堆：不进列表（仅入档）
        : [...s.items, { ...item, queued: silenced }].slice(-MAX_ITEMS),
    nextId: id + 1,
    replayQueue: silenced ? [...s.replayQueue, item] : s.replayQueue,
  }));

  // Z-47 通知存档（含动作 JSON，历史通知的动作可由发送方支持重新执行）
  void (async () => {
    try {
      const { ipc } = await import("../lib/ipc");
      await ipc.notifyArchiveInsert({
        app: source,
        title,
        body,
        kind,
        actions: actions ? JSON.stringify(actions) : "",
        ts: Date.now(),
      });
    } catch {
      /* 浏览器 dev 模式或失败：静默 */
    }
  })();

  // A-4：通知横幅音（勿扰/静默堆不响；音量/静音/主题读设置，失败静默）
  if (!silenced && stack !== "silent") {
    void (async () => {
      try {
        const { playSound } = await import("../lib/sounds");
        const s = await import("../lib/settings").then((m) => m.loadSettings());
        playSound("notify", {
          volume: s.soundVolume,
          muted: s.soundMuted,
          dnd: dndActive,
          theme: opts?.sound?.theme ?? s.soundTheme,
          nightDamp: opts?.sound?.nightDamp ?? s.soundNightDamp,
        });
      } catch {
        /* 静默 */
      }
    })();
  }
}

/** U-51：进度通知更新（推送带 progress 的通知后，实时跟随任务进度）。 */
export function updateNotifyProgress(id: number, progress: { current: number; total: number; label?: string }): void {
  notifyStore.setState((s) => ({
    items: s.items.map((it) => (it.id === id ? { ...it, progress } : it)),
  }));
}

/** U-51：进度通知完成 → 转操作通知（清 progress，附加可选动作）。 */
export function completeNotifyProgress(id: number, actions?: NotifyAction[]): void {
  notifyStore.setState((s) => ({
    items: s.items.map((it) =>
      it.id === id ? { ...it, progress: null, actions: actions ?? it.actions, priority: "action" } : it,
    ),
  }));
}

/**
 * U-51 排队礼仪：勿扰结束后按重要度有序回放（action > progress > normal）。
 * 回放 = 置顶呈现 + 响一声汇总提示；一键全部清除见 clearNotifications。
 */
export function flushReplay(): void {
  const q = [...notifyStore.getState().replayQueue];
  if (q.length === 0) return;
  q.sort((a, b) => priorityRank[a.priority] - priorityRank[b.priority]);
  notifyStore.setState((s) => ({
    items: [...q, ...s.items].slice(0, MAX_ITEMS),
    replayQueue: [],
  }));
  // 汇总卡片：「勿扰期间 N 条通知」
  const summary: NotifyItem = {
    id: notifyStore.getState().nextId,
    time: Date.now(),
    kind: "system",
    title: q.length > 1 ? `${q.length} 条通知回放` : "1 条通知回放",
    body: [...new Set(q.map((it) => it.source ?? it.kind))].join(" / "),
    read: false,
    priority: "normal",
  };
  notifyStore.setState((s) => ({ items: [...s.items, summary].slice(-MAX_ITEMS), nextId: s.nextId + 1 }));
  void (async () => {
    try {
      const { playSound } = await import("../lib/sounds");
      const s = await import("../lib/settings").then((m) => m.loadSettings());
      playSound("notify", { volume: s.soundVolume, muted: s.soundMuted, theme: s.soundTheme, nightDamp: s.soundNightDamp });
    } catch {
      /* 静默 */
    }
  })();
}

/** F-5.1：内置动作执行 —— 分发全局事件，DesktopShell 统一接住（open-app/open-path）。 */
export function fireNotifyAction(action: NotifyAction): void {
  window.dispatchEvent(new CustomEvent("variable:notify-action", { detail: action }));
}

export function markAllRead(): void {
  notifyStore.setState((s) => ({
    items: s.items.map((it) => (it.read ? it : { ...it, read: true })),
  }));
}

export function clearNotifications(): void {
  notifyStore.setState({ items: [], replayQueue: [] });
}
