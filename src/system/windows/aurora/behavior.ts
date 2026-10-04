/**
 * AURORA-10000 · 行为层（AI-06/07/09 行为族）：
 * - 族0029 空间手势（F00701~F00725）→ 手势绑定注册表
 * - 族0031 虚拟桌面系统（F00751~F00775）→ 虚拟桌面 reducer
 * - 族0032 窗口分组标签（F00776~F00800）→ 分组 reducer
 * - 族0033 焦点与注意力（F00801~F00825）→ 焦点策略
 * - 族0041 窗口收纳坞（F01026~F01050）→ 收纳坞 reducer
 * - 族0044 窗口放映模式（F01101~F01125）→ 放映状态机
 * 全部纯函数/纯数据，可直接单测；UI 调用方负责落地。
 */

// ---------- 族0029 空间手势 ----------

/** 手势绑定：触发器 → 动作。 */
export interface GestureBinding {
  /** 触发器标识（如 swipe3-left / corner-tl / draw-C）。 */
  trigger: string;
  /** 动作标识（VWM 可执行动作）。 */
  action: string;
  /** 是否默认启用（教学类默认开，摇一摇等易误触默认关）。 */
  enabled: boolean;
}

/** 族0029：25 种手势绑定（ID 升序）。 */
export const GESTURE_BINDINGS: readonly GestureBinding[] = Object.freeze([
  { trigger: "drag3-hold", action: "window.move", enabled: true }, // F00701 三指拖窗
  { trigger: "swipe4-left", action: "vdesk.next", enabled: true }, // F00702 四指切桌
  { trigger: "edge-swipe", action: "hotzone.open", enabled: true }, // F00703 边缘热区
  { trigger: "corner-hover", action: "hotcorner.fire", enabled: false }, // F00704 角落热键
  { trigger: "shake", action: "window.solo", enabled: false }, // F00705 摇一摇
  { trigger: "pinch2", action: "window.resize", enabled: true }, // F00706 双指缩放
  { trigger: "longpress-400", action: "snap.pad", enabled: true }, // F00707 长按吸附
  { trigger: "flick-edge", action: "dock.throw", enabled: true }, // F00708 甩动入坞
  { trigger: "flick-cross", action: "monitor.throw", enabled: true }, // F00709 抛掷跨屏
  { trigger: "draw-C", action: "window.close", enabled: false }, // F00710 画C关窗
  { trigger: "draw-V", action: "window.snap-left", enabled: false }, // F00711 画V贴左
  { trigger: "draw-H", action: "window.snap-half", enabled: false }, // F00712 画H半屏
  { trigger: "draw-Z", action: "layout.undo", enabled: false }, // F00713 画Z撤销
  { trigger: "swipe2-down", action: "window.minimize", enabled: true }, // F00714 下滑最小化
  { trigger: "swipe3-up", action: "taskview.open", enabled: true }, // F00715 上滑任务视图
  { trigger: "knock2", action: "window.pintop", enabled: false }, // F00716 指节置顶
  { trigger: "rotate2", action: "window.opacity", enabled: false }, // F00717 旋转调透明
  { trigger: "pinch-in-all", action: "layout.gather", enabled: false }, // F00718 捏合聚拢
  { trigger: "pinch-out-all", action: "layout.spread", enabled: false }, // F00719 张开散开
  { trigger: "throw-invalid", action: "throw.rebound", enabled: true }, // F00720 抛物回弹
  { trigger: "drag-beyond", action: "edge.rebound", enabled: true }, // F00721 边界反弹
  { trigger: "flick-bottom", action: "recycle.drop", enabled: true }, // F00722 甩入回收
  { trigger: "hover-800", action: "preview.show", enabled: true }, // F00723 悬停预览
  { trigger: "speed-adaptive", action: "gesture.calibrate", enabled: true }, // F00724 手速学习
  { trigger: "first-fire", action: "gesture.tutorial", enabled: true }, // F00725 教学浮层
]);

// ---------- 族0031 虚拟桌面系统 ----------

/** 虚拟桌面。 */
export interface VDesk {
  id: number;
  name: string;
  /** 独立壁纸（空=跟随全局）。 */
  wallpaper: string | null;
  /** 固定应用（全桌可见）。 */
  pinned: string[];
  /** 隐藏（隐私桌）。 */
  hidden: boolean;
  /** 锁（密码桌）。 */
  locked: boolean;
  /** 时区偏移（分钟，null=本地）。 */
  tzOffset: number | null;
  /** 停留时长（ms，累计）。 */
  dwellMs: number;
}

export interface VDeskState {
  desks: VDesk[];
  active: number;
  /** 待办挂钩（桌 id → 待办列表）。 */
  todos: Record<number, string[]>;
}

export const VDESK_INITIAL: VDeskState = {
  desks: [{ id: 0, name: "桌面 1", wallpaper: null, pinned: [], hidden: false, locked: false, tzOffset: null, dwellMs: 0 }],
  active: 0,
  todos: {},
};

/** 虚拟桌面操作。 */
export type VDeskOp =
  | { t: "create"; name?: string; template?: "focus" | "life" | "study" | null }
  | { t: "close"; id: number }
  | { t: "switch"; id: number }
  | { t: "rename"; id: number; name: string }
  | { t: "pin"; id: number; app: string }
  | { t: "unpin"; id: number; app: string }
  | { t: "hide"; id: number; hidden: boolean }
  | { t: "lock"; id: number; locked: boolean }
  | { t: "wallpaper"; id: number; wallpaper: string | null }
  | { t: "tz"; id: number; tzOffset: number | null }
  | { t: "todo"; id: number; items: string[] }
  | { t: "dwell"; id: number; ms: number };

/** 族0031 reducer：模板/固定/隐藏/锁/时区/待办/统计全覆盖。 */
export function vdeskReduce(s: VDeskState, op: VDeskOp): VDeskState {
  switch (op.t) {
    case "create": {
      if (s.desks.length >= 9) return s; // F00769 上限提示由 UI 呈现，此处封顶 9
      const id = Math.max(...s.desks.map((d) => d.id)) + 1;
      const tpl = op.template;
      return {
        ...s,
        desks: [
          ...s.desks,
          {
            id,
            name: op.name ?? (tpl === "focus" ? "专注" : tpl === "life" ? "生活" : tpl === "study" ? "学习" : `桌面 ${id + 1}`),
            wallpaper: null,
            pinned: tpl === "study" ? ["write", "mind"] : [],
            hidden: false,
            locked: false,
            tzOffset: null,
            dwellMs: 0,
          },
        ],
        active: id,
      };
    }
    case "close": {
      if (s.desks.length <= 1) return s;
      const desks = s.desks.filter((d) => d.id !== op.id);
      return { ...s, desks, active: s.active === op.id ? desks[0]!.id : s.active };
    }
    case "switch": {
      const d = s.desks.find((x) => x.id === op.id);
      if (!d) return s;
      return {
        ...s,
        active: op.id,
        desks: s.desks.map((x) => (x.id === op.id ? { ...x, dwellMs: x.dwellMs } : x)),
      };
    }
    case "rename":
      return { ...s, desks: s.desks.map((d) => (d.id === op.id ? { ...d, name: op.name } : d)) };
    case "pin":
      return {
        ...s,
        desks: s.desks.map((d) =>
          d.id === op.id && !d.pinned.includes(op.app) ? { ...d, pinned: [...d.pinned, op.app] } : d,
        ),
      };
    case "unpin":
      return {
        ...s,
        desks: s.desks.map((d) => (d.id === op.id ? { ...d, pinned: d.pinned.filter((a) => a !== op.app) } : d)),
      };
    case "hide":
      return { ...s, desks: s.desks.map((d) => (d.id === op.id ? { ...d, hidden: op.hidden } : d)) };
    case "lock":
      return { ...s, desks: s.desks.map((d) => (d.id === op.id ? { ...d, locked: op.locked } : d)) };
    case "wallpaper":
      return { ...s, desks: s.desks.map((d) => (d.id === op.id ? { ...d, wallpaper: op.wallpaper } : d)) };
    case "tz":
      return { ...s, desks: s.desks.map((d) => (d.id === op.id ? { ...d, tzOffset: op.tzOffset } : d)) };
    case "todo":
      return { ...s, todos: { ...s.todos, [op.id]: op.items } };
    case "dwell":
      return { ...s, desks: s.desks.map((d) => (d.id === op.id ? { ...d, dwellMs: d.dwellMs + op.ms } : d)) };
    default:
      return s;
  }
}

// ---------- 族0032 窗口分组标签 ----------

export interface WinGroup {
  id: number;
  name: string;
  color: string;
  /** 组内窗口 id（有序 = 标签顺序）。 */
  wins: string[];
  locked: boolean;
  /** 组内平铺。 */
  tiled: boolean;
  muted: string[];
}

export interface GroupState {
  groups: WinGroup[];
  /** 窗 id → 组 id。 */
  member: Record<string, number>;
}

export const GROUP_INITIAL: GroupState = { groups: [], member: {} };

export type GroupOp =
  | { t: "make"; win: string; name?: string }
  | { t: "join"; win: string; group: number }
  | { t: "leave"; win: string }
  | { t: "reorder"; group: number; wins: string[] }
  | { t: "rename"; group: number; name: string }
  | { t: "color"; group: number; color: string }
  | { t: "lock"; group: number; locked: boolean }
  | { t: "tile"; group: number; tiled: boolean }
  | { t: "mute"; group: number; win: string; muted: boolean };

/** 族0032 reducer：成组/拆分/重排/锁/平铺/静音。 */
export function groupReduce(s: GroupState, op: GroupOp): GroupState {
  switch (op.t) {
    case "make": {
      const id = s.groups.length ? Math.max(...s.groups.map((g) => g.id)) + 1 : 1;
      return {
        groups: [...s.groups, { id, name: op.name ?? `组 ${id}`, color: "accent", wins: [op.win], locked: false, tiled: false, muted: [] }],
        member: { ...s.member, [op.win]: id },
      };
    }
    case "join": {
      if (!s.groups.some((g) => g.id === op.group)) return s;
      const prev = s.member[op.win];
      const groups = s.groups.map((g) => {
        if (g.id === prev) return { ...g, wins: g.wins.filter((w) => w !== op.win) };
        if (g.id === op.group && !g.wins.includes(op.win)) return { ...g, wins: [...g.wins, op.win] };
        return g;
      });
      return { groups: groups.filter((g) => g.wins.length > 0), member: { ...s.member, [op.win]: op.group } };
    }
    case "leave": {
      const gid = s.member[op.win];
      if (gid == null) return s;
      const groups = s.groups
        .map((g) => (g.id === gid ? { ...g, wins: g.wins.filter((w) => w !== op.win) } : g))
        .filter((g) => g.wins.length > 0);
      const member = { ...s.member };
      delete member[op.win];
      return { groups, member };
    }
    case "reorder":
      return { ...s, groups: s.groups.map((g) => (g.id === op.group ? { ...g, wins: op.wins } : g)) };
    case "rename":
      return { ...s, groups: s.groups.map((g) => (g.id === op.group ? { ...g, name: op.name } : g)) };
    case "color":
      return { ...s, groups: s.groups.map((g) => (g.id === op.group ? { ...g, color: op.color } : g)) };
    case "lock":
      return { ...s, groups: s.groups.map((g) => (g.id === op.group ? { ...g, locked: op.locked } : g)) };
    case "tile":
      return { ...s, groups: s.groups.map((g) => (g.id === op.group ? { ...g, tiled: op.tiled } : g)) };
    case "mute":
      return {
        ...s,
        groups: s.groups.map((g) =>
          g.id === op.group
            ? { ...g, muted: op.muted ? [...new Set([...g.muted, op.win])] : g.muted.filter((w) => w !== op.win) }
            : g,
        ),
      };
    default:
      return s;
  }
}

// ---------- 族0033 焦点与注意力 ----------

export interface FocusPolicy {
  /** 跟随鼠标（可关）。 */
  followMouse: boolean;
  /** 防抢协议。 */
  antiSteal: boolean;
  /** 失焦降噪透明度（0~1，1=不变暗）。 */
  dimOthers: number;
  /** 打断保护（深度工作期）。 */
  protect: boolean;
  /** 置顶白名单。 */
  allowTop: string[];
  /** 番茄钟分钟数（0=关）。 */
  pomodoroMin: number;
}

/** 族0033：25 种焦点档（ID 升序）。 */
export const FOCUS_POLICIES: readonly (FocusPolicy & { id: string })[] = Object.freeze([
  { id: "F00801", followMouse: true, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00802", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00803", followMouse: false, antiSteal: true, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00804", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00805", followMouse: false, antiSteal: false, dimOthers: 0.85, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00806", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: ["pip", "calc"], pomodoroMin: 0 },
  { id: "F00807", followMouse: false, antiSteal: true, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00808", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00809", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00810", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00811", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00812", followMouse: true, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00813", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00814", followMouse: false, antiSteal: true, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00815", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00816", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00817", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00818", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00819", followMouse: false, antiSteal: true, dimOthers: 1, protect: true, allowTop: [], pomodoroMin: 0 },
  { id: "F00820", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 25 },
  { id: "F00821", followMouse: false, antiSteal: true, dimOthers: 1, protect: true, allowTop: [], pomodoroMin: 0 },
  { id: "F00822", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00823", followMouse: false, antiSteal: false, dimOthers: 0.9, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00824", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
  { id: "F00825", followMouse: false, antiSteal: false, dimOthers: 1, protect: false, allowTop: [], pomodoroMin: 0 },
]);

/** 焦点策略判定：后台窗请求抢焦是否放行。 */
export function allowFocusSteal(policy: FocusPolicy, req: { app: string; topmost: boolean; urgent: boolean }): boolean {
  if (policy.antiSteal && !policy.allowTop.includes(req.app)) return false;
  return true;
}

/** 注意力评分：0~100，切换越频繁越低。 */
export function attentionScore(switchesPerMin: number, interrupts: number): number {
  const base = 100 - Math.min(60, switchesPerMin * 2) - Math.min(40, interrupts * 4);
  return Math.max(0, Math.round(base));
}

// ---------- 族0041 窗口收纳坞 ----------

export interface DockItem {
  win: string;
  pinned: boolean;
  /** 分类键（app/task/project）。 */
  bucket: string;
}

export interface DockState {
  items: DockItem[];
  capacity: number;
  side: "left" | "right";
  autoHide: boolean;
}

export const DOCK_INITIAL: DockState = { items: [], capacity: 24, side: "right", autoHide: true };

export type DockOp =
  | { t: "add"; win: string; bucket?: string }
  | { t: "remove"; win: string }
  | { t: "pin"; win: string; pinned: boolean }
  | { t: "sort"; by: "pinned" | "bucket" | "time" }
  | { t: "clear" }
  | { t: "capacity"; n: number }
  | { t: "side"; side: "left" | "right" }
  | { t: "autoHide"; on: boolean };

/** 族0041 reducer：收纳/拖出/置顶/分类/容量/分屏坞。 */
export function dockReduce(s: DockState, op: DockOp): DockState {
  switch (op.t) {
    case "add": {
      if (s.items.some((i) => i.win === op.win)) return s;
      if (s.items.length >= s.capacity) return s;
      return { ...s, items: [...s.items, { win: op.win, pinned: false, bucket: op.bucket ?? "app" }] };
    }
    case "remove":
      return { ...s, items: s.items.filter((i) => i.win !== op.win) };
    case "pin":
      return { ...s, items: s.items.map((i) => (i.win === op.win ? { ...i, pinned: op.pinned } : i)) };
    case "sort": {
      const items = [...s.items];
      if (op.by === "pinned") items.sort((a, b) => Number(b.pinned) - Number(a.pinned));
      else if (op.by === "bucket") items.sort((a, b) => a.bucket.localeCompare(b.bucket) || Number(b.pinned) - Number(a.pinned));
      else items.sort((a, b) => Number(b.pinned) - Number(a.pinned) || a.win.localeCompare(b.win));
      return { ...s, items };
    }
    case "clear":
      return { ...s, items: s.items.filter((i) => i.pinned) };
    case "capacity":
      return { ...s, capacity: Math.max(1, op.n), items: s.items.slice(0, Math.max(1, op.n)) };
    case "side":
      return { ...s, side: op.side };
    case "autoHide":
      return { ...s, autoHide: op.on };
    default:
      return s;
  }
}

// ---------- 族0044 窗口放映模式 ----------

export interface PresentStep {
  win: string;
  /** 备注（讲者视图）。 */
  note: string;
}

export interface PresentState {
  /** 是否在放映。 */
  active: boolean;
  steps: PresentStep[];
  index: number;
  /** 自动步进秒数（0=手动）。 */
  autoSec: number;
  /** 计时开始时刻（ms epoch，0=未开始）。 */
  startedAt: number;
  /** 观众模式（只读）。 */
  audience: boolean;
  /** 隐藏私人窗。 */
  hidePrivate: boolean;
  /** 录屏联动。 */
  recording: boolean;
}

export const PRESENT_INITIAL: PresentState = {
  active: false,
  steps: [],
  index: 0,
  autoSec: 0,
  startedAt: 0,
  audience: false,
  hidePrivate: true,
  recording: false,
};

export type PresentOp =
  | { t: "enter"; steps: PresentStep[]; autoSec?: number; audience?: boolean }
  | { t: "next" }
  | { t: "prev" }
  | { t: "goto"; index: number }
  | { t: "exit" };

/** 族0044 状态机：进入/步进/退出，退出时由调用方恢复原布局（F01118）。 */
export function presentReduce(s: PresentState, op: PresentOp, now: number): PresentState {
  switch (op.t) {
    case "enter":
      if (op.steps.length === 0) return s;
      return {
        ...s,
        active: true,
        steps: op.steps,
        index: 0,
        autoSec: op.autoSec ?? 0,
        startedAt: now,
        audience: op.audience ?? false,
      };
    case "next":
      return s.active && s.index < s.steps.length - 1 ? { ...s, index: s.index + 1 } : s;
    case "prev":
      return s.active && s.index > 0 ? { ...s, index: s.index - 1 } : s;
    case "goto":
      return s.active && op.index >= 0 && op.index < s.steps.length ? { ...s, index: op.index } : s;
    case "exit":
      return { ...PRESENT_INITIAL };
    default:
      return s;
  }
}
