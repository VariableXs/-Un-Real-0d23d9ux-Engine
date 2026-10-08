/**
 * NOVA-200 · S4 任务栏进化路（AI-04）—— 域4 任务栏与开始菜单（W-039…W-050）。
 *
 * 边界（全景 §4）：M-13/15 管启动等待与空区菜单、U-15/V-17 管溢出折叠、
 * M-12 管时钟卡、批次E 管悬停 2s 预览；本模块补**磁漂手感、潮汐密度、
 * 零点击详情、活动微史、窗景微缩、模块槽架构、角色预设、昨日延续、
 * 锋锐制式、启动弹道、手柄导航、多屏镜像**。
 *
 * 纪律：
 * - 零侵入：不改写 Taskbar/StartMenu/SearchOverlay 内部逻辑；全部为
 *   DOM 叠层 + CSS 变量 + 类名挂载 + 公开 API 只读消费（vwmStore / recent / thirdApps）；
 * - 前缀：类名 `nova-`、事件 `nova://dock-*`、localStorage 键 `nova.dock.*`；
 * - 开关：只读消费 S0 注册表（registry.ts，novaOn/novaNum），无注册表时用 defaultOn 回退；
 * - 降级：reduce-motion / safeMode / static 下全部动效归零（磁漂/弹道/呼吸），语义保留；
 * - 诚实：环境尚不存在的维度（多屏任务栏需 S17 多屏壳窗、OS 级窗口无法实时取景、
 *   蓝牙手柄连接前浏览器 Gamepad API 不可见）在卡片 wiringHint/degrade 如实注明。
 */

import { launchThirdApp, getThirdApps } from "../../launcher/thirdApps";
import { desktopAppLabel, desktopIconDefs } from "../../desktop-icons/DesktopIcons";
import { openVwmApp, openVwmSystem, vwmStore } from "../../windows/vwm";
import { novaMotionOK, novaNum, novaOn } from "../registry";
import type { NovaParamDef } from "../registry";

// ---------------------------------------------------------------------------
// 通用工具（本模块自持）
// ---------------------------------------------------------------------------

export function clamp(v: number, min: number, max: number): number {
  return Math.min(Math.max(v, min), max);
}

/** reduce-motion 运行时标记（含 safeMode / static 全降级链）。 */
export function motionOK(): boolean {
  return novaMotionOK();
}

function lsGet<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw == null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function lsSet(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage full/blocked → 不持久化 */
  }
}

const NS = "nova.dock";

/** 功能开关：只读消费 S0 注册表（nova.registry.v1 单一事实源）。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

function num(id: string, key: string): number {
  return novaNum(id, key);
}

/** 派发 `nova://dock-*` 事件（SSR/测试环境安全）。 */
export function novaEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://dock-${name}`, { detail }));
}

// ---------------------------------------------------------------------------
// Hub 注册清单（S0 NovaHub 消费：功能卡 + 参数 + 降级说明）
// ---------------------------------------------------------------------------

export interface NovaFeatureCard {
  id: string;
  titleZh: string;
  titleEn: string;
  descZh: string;
  defaultOn: boolean;
  /** Hub 参数卡（对齐 registry.NovaParamDef，S0 单一事实源）。 */
  params?: NovaParamDef[];
  /** 需要 S17 在既有文件补接线的行为（本模块 API/事件已备好）。 */
  wiringHint?: string;
  /** reduce-motion / safeMode / static 降级说明。 */
  degrade: string;
}

export const DOCK_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-039",
    titleZh: "任务栏磁漂",
    titleEn: "Dock Magnet Drift",
    descZh: "指针靠近时图标群体向指针弹性磁漂（高斯衰减 + 多体弹簧回弹，默认关）；纯 transform 合成器动画，60 图标不减帧率。",
    defaultOn: false,
    degrade: "reduce-motion / safeMode / static 下整体关闭，图标纹丝不动",
  },
  {
    id: "W-040",
    titleZh: "开始菜单角色预设",
    titleEn: "Start Personas",
    descZh: "开发者/创作者/学生三角色一键重排开始菜单网格（工具/写作/学习入口前置），另存当前布局为自定义预设，可随时还原上次布局。",
    defaultOn: true,
    wiringHint: "预设按钮卡可由 S17 挂入开始菜单头部；已备 applyStartPreset()/nova://dock-preset 事件",
    degrade: "无动效，纯重排（写 variable:start:order:v1，原序备份可还原）",
  },
  {
    id: "W-041",
    titleZh: "任务栏潮汐条",
    titleEn: "Tidal Density",
    descZh: "图标占用率驱动的连续自适应：占用升高时间距 4→1px、图标 1→0.86 连续收缩，空间紧张而不拥挤；占用回落自动回涨。",
    defaultOn: true,
    degrade: "无动效（CSS 变量直改，无过渡），关闭即恢复默认间距",
  },
  {
    id: "W-042",
    titleZh: "零点击详情卡",
    titleEn: "Zero-Click Detail",
    descZh: "全局搜索出结果时，首项完整标题/摘要/类型/时间以详情卡即时呈现，先看见再决定，Enter 直达。",
    defaultOn: true,
    degrade: "无动效，纯信息卡",
  },
  {
    id: "W-043",
    titleZh: "任务栏时间戳微史",
    titleEn: "Timestamp Micro-History",
    descZh: "悬停任务栏空白 800ms 浮出今日活动微史：30 分钟刻度直方图 + 峰值时段 + 高频项，全部本机点击真实记录（仅留今日/昨日）。",
    defaultOn: true,
    degrade: "无动效，纯信息卡；隐私敏感可整体关闭",
  },
  {
    id: "W-044",
    titleZh: "悬停窗景预览",
    titleEn: "Live Window Peek",
    descZh: "悬停运行中图标 500ms 弹出全窗口实时微缩景（VWM 窗口按真实 DOM 等比缩放）；多实例取最上层。",
    defaultOn: true,
    params: [{ key: "peekMs", labelKey: "novaP_peekMs", type: "slider", default: 500, min: 200, max: 1200, step: 100 }] as never,
    wiringHint: "OS 级 Tauri 窗口无法在 WebView 内取景，此类图标如实显示占位轮廓（不假装截图）",
    degrade: "reduce-motion 下微缩景直接显示（无淡入），OS 窗口仍为占位轮廓",
  },
  {
    id: "W-045",
    titleZh: "任务栏模块槽",
    titleEn: "Dock Module Slots",
    descZh: "任务栏双端各 2 槽的可拔插模块架构：registerDockModule() 注册、槽位指派、开合持久化、nova://dock-module 事件协议。",
    defaultOn: true,
    wiringHint: "Q-23/27/56 等既有小组件可经本协议挂入槽位（S17 对接）",
    degrade: "无动效，纯架构；空槽零渲染零开销",
  },
  {
    id: "W-046",
    titleZh: "开始菜单昨日区",
    titleEn: "Yesterday Zone",
    descZh: "开始菜单顶部「昨日延续」分区：昨日用过而今日未动的条目自动前置，点击直达续昨天的事。",
    defaultOn: true,
    wiringHint: "分区由本模块注入 .start-menu（MutationObserver），S17 无需改动 StartMenu",
    degrade: "无动效，纯分区",
  },
  {
    id: "W-047",
    titleZh: "任务栏锋锐模式",
    titleEn: "Sharp Density",
    descZh: "32px 极限密度制式：按钮 32×30、图标 24px、间距 2px，同宽多容纳约 40% 图标；最小命中区 32px ≥ 28px 底线。",
    defaultOn: false,
    degrade: "无动效，纯制式切换（类名挂载）",
  },
  {
    id: "W-048",
    titleZh: "启动弹道",
    titleEn: "Launch Arc",
    descZh: "点击任务栏图标启动应用时，图标副本沿抛物线飞向屏心（240ms，贝塞尔插值）后消散，视线跟着启动走。",
    defaultOn: true,
    params: [{ key: "arcMs", labelKey: "novaP_arcMs", type: "slider", default: 240, min: 120, max: 480, step: 60 }] as never,
    degrade: "reduce-motion / safeMode / static 下弹道整体关闭",
  },
  {
    id: "W-049",
    titleZh: "手柄友好菜单",
    titleEn: "Gamepad Menu",
    descZh: "手柄接入自动切手柄友好布局（瓷砖 1.25× 放大 + 焦点描边），十字键网格导航、A 确认、B 关闭、X/Y 首尾跳转；断开 30s 自动还原键鼠形态。",
    defaultOn: true,
    wiringHint: "连接前探测需 S0 nova_bluetooth_watch（浏览器 Gamepad API 首次输入后才可见手柄）",
    degrade: "无手柄时零开销；还原为纯类名/焦点清理，无动画",
  },
  {
    id: "W-050",
    titleZh: "多屏任务栏镜像",
    titleEn: "Multi-Screen Mirror",
    descZh: "各屏同构任务栏选项与镜像状态总线：开关/几何签名/时间戳持久化并 nova://dock-mirror 广播，事件同步 ≤ 100ms。",
    defaultOn: false,
    wiringHint: "每屏实体任务栏渲染需 S17 多屏壳窗（当前单窗口壳，先交付选项+同步总线）",
    degrade: "无动效，纯状态同步",
  },
];

export const dockNovaDomain = {
  id: "S4",
  nameZh: "任务栏进化",
  nameEn: "Dock Evolution",
  route: "AI-04",
  features: DOCK_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

// ---- W-039 磁漂弹簧 ----

export const DRIFT_RADIUS_PX = 64;
export const SPRING_STIFFNESS = 180;
export const SPRING_DAMPING = 20;

/**
 * 磁漂目标位移（px）：指针在图标左侧 → 负（向左迎）、右侧 → 正；
 * 高斯衰减 σ = radius/2，超过 radius 为 0（场外零影响）。
 */
export function driftTargetPx(pointerX: number, iconCenterX: number, maxPx: number, radiusPx = DRIFT_RADIUS_PX): number {
  const d = pointerX - iconCenterX;
  if (Math.abs(d) >= radiusPx) return 0;
  const sigma = radiusPx / 2;
  const fall = Math.exp(-(d * d) / (2 * sigma * sigma));
  const t = d / radiusPx; // -1..1
  return Math.round(maxPx * t * fall * 100) / 100;
}

/** 半隐式欧拉弹簧一步（dt 秒）。返回 [pos, vel]。 */
export function springStep(pos: number, vel: number, target: number, dt: number, k = SPRING_STIFFNESS, c = SPRING_DAMPING): [number, number] {
  const nv = vel + (k * (target - pos) - c * vel) * dt;
  const np = pos + nv * dt;
  return [np, nv];
}

/** 弹簧是否已稳定（|Δpos| < 0.05 且 |vel| < 0.05）——稳定后停 rAF，零常驻开销。 */
export function springSettled(pos: number, vel: number, target: number): boolean {
  return Math.abs(target - pos) < 0.05 && Math.abs(vel) < 0.05;
}

// ---- W-040 角色预设 ----

export type PersonaPresetId = "developer" | "creator" | "student";

/** 三角色前置入口（存在才前置，不存在自动跳过——诚实适配卸载态）。 */
export const PERSONA_PRESET_RANK: Record<PersonaPresetId, string[]> = {
  developer: ["tool-calc", "tool-clockhub", "tool-sysinfo", "tool-checksum", "app-project", "tool-convert", "sys-taskman"],
  creator: ["app-write", "app-mindmap", "app-fate", "tool-snapshot", "tool-notes", "sys-explorer"],
  student: ["tool-calendar", "tool-notes", "app-write", "tool-clockhub", "tool-magnifier", "sys-explorer"],
};

/**
 * 预设重排：rank 中存在且在当前布局里的 id 依 preset 序前置，其余保持相对顺序。
 * 稳定（不新增不丢失；rank 里的幽灵 id 与重复项被忽略）。
 */
export function applyPresetOrder(current: string[], rank: string[]): string[] {
  const cur = new Set(current);
  const head: string[] = [];
  const seen = new Set<string>();
  for (const id of rank) {
    if (cur.has(id) && !seen.has(id)) {
      seen.add(id);
      head.push(id);
    }
  }
  return [...head, ...current.filter((id) => !seen.has(id))];
}

export interface PersonaPreset {
  name: string;
  order: string[];
  at: number;
  custom: boolean;
}

/** 另存/覆盖自定义预设（同名覆盖；最多 8 个，超出挤掉最旧的自定义）。 */
export function upsertPreset(list: PersonaPreset[], name: string, order: string[], now = Date.now()): PersonaPreset[] {
  const trimmedName = name.trim();
  if (!trimmedName || order.length === 0) return list;
  const exists = list.some((p) => p.name === trimmedName);
  const next = exists
    ? list.map((p) => (p.name === trimmedName ? { name: trimmedName, order: [...order], at: now, custom: true } : p))
    : [...list, { name: trimmedName, order: [...order], at: now, custom: true }];
  const customs = next.filter((p) => p.custom);
  if (customs.length > 8) {
    const drop = customs.sort((a, b) => a.at - b.at)[0]!.name;
    return next.filter((p) => p.name !== drop);
  }
  return next;
}

// ---- W-041 潮汐条 ----

export const TIDE_BASE_GAP = 4;
export const TIDE_MIN_GAP = 1;
export const TIDE_FULL_OCCUPANCY = 1;
export const TIDE_HALF_OCCUPANCY = 0.5;
export const TIDE_MAX_SHRINK = 0.86;

/** 潮汐间距（px）：占用 ≤0.5 → 4px（默认），0.5→1.0 线性收到 1px。 */
export function tideGapPx(occupancy: number): number {
  const t = clamp((occupancy - TIDE_HALF_OCCUPANCY) / (TIDE_FULL_OCCUPANCY - TIDE_HALF_OCCUPANCY), 0, 1);
  return Math.round((TIDE_BASE_GAP - (TIDE_BASE_GAP - TIDE_MIN_GAP) * t) * 100) / 100;
}

/** 潮汐图标缩放：占用 ≤0.55 → 1，0.55→1.0 线性收到 0.86。 */
export function tideScale(occupancy: number): number {
  const t = clamp((occupancy - 0.55) / (1 - 0.55), 0, 1);
  return Math.round((1 - (1 - TIDE_MAX_SHRINK) * t) * 100) / 100;
}

/** 占用率 = Σ图标宽 + 原生间隙 / 可用容量。 */
export function tideOccupancy(btnWidths: number[], count: number, capacityPx: number, baseGap = TIDE_BASE_GAP): number {
  if (capacityPx <= 0 || count === 0) return 0;
  const used = btnWidths.reduce((a, w) => a + w, 0) + Math.max(0, count - 1) * baseGap;
  return clamp(used / capacityPx, 0, 1);
}

// ---- W-042 零点击详情卡 ----

export interface SearchDetailFields {
  title: string;
  snippet: string;
  kind: string;
  meta: string;
}

/** 首项详情字段：标题/摘要全文呈现（摘要 ≤240 字符防爆炸），meta 按 " · " 拆类型与时间。 */
export function detailFields(title: string, snippet: string, meta: string): SearchDetailFields {
  const parts = meta.split("·").map((s) => s.trim()).filter(Boolean);
  return {
    title: title.trim(),
    snippet: snippet.trim().slice(0, 240),
    kind: parts[0] ?? "",
    meta: parts.slice(1).join(" · "),
  };
}

// ---- W-043 时间戳微史 ----

export const BUCKET_MINUTES = 30;
export const BUCKETS_PER_DAY = (24 * 60) / BUCKET_MINUTES; // 48

/** 当日 0 点起的 30 分钟桶序号（0..47）。 */
export function bucket30(ts: number, dayStartMs: number): number {
  return clamp(Math.floor((ts - dayStartMs) / (BUCKET_MINUTES * 60_000)), 0, BUCKETS_PER_DAY - 1);
}

/** 本地日期键（YYYY-MM-DD）。 */
export function dayKeyOf(ts: number): string {
  const d = new Date(ts);
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

export interface ActivityEvent {
  kind: "app" | "sys" | "tp";
  id: string;
  name: string;
  ts: number;
}

/** 30 分钟刻度直方图（当日）。 */
export function activityBuckets(events: ActivityEvent[], dayStartMs: number): number[] {
  const out = new Array<number>(BUCKETS_PER_DAY).fill(0);
  for (const e of events) out[bucket30(e.ts, dayStartMs)]! += 1;
  return out;
}

/** 微史摘要：峰值桶 + 总数 + 高频 TopN。 */
export function microHistorySummary(events: ActivityEvent[], dayStartMs: number, topN = 3): {
  buckets: number[];
  peakBucket: number;
  peakCount: number;
  total: number;
  top: Array<{ id: string; name: string; count: number }>;
} {
  const buckets = activityBuckets(events, dayStartMs);
  let peakBucket = 0;
  let peakCount = 0;
  let total = 0;
  buckets.forEach((c, i) => {
    total += c;
    if (c > peakCount) {
      peakCount = c;
      peakBucket = i;
    }
  });
  const byId = new Map<string, { id: string; name: string; count: number }>();
  for (const e of events) {
    const cur = byId.get(e.id) ?? { id: e.id, name: e.name, count: 0 };
    cur.count += 1;
    byId.set(e.id, cur);
  }
  const top = [...byId.values()].sort((a, b) => b.count - a.count).slice(0, topN);
  return { buckets, peakBucket, peakCount, total, top };
}

/** 当日 0 点（本地时区）。 */
export function dayStartOf(ts: number): number {
  const d = new Date(ts);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
}

/** 活动记录去重（同 id 30s 内重复点击记 1 次——双击图标不应刷直方图）。 */
export function pushActivity(list: ActivityEvent[], ev: ActivityEvent, max = 200): ActivityEvent[] {
  const last = list[list.length - 1];
  if (last && last.id === ev.id && ev.ts - last.ts < 30_000) return list;
  const next = [...list, ev];
  return next.slice(Math.max(0, next.length - max));
}

// ---- W-044 悬停窗景预览 ----

/** 等比缩放系数：完整放入 maxW×maxH，且 ≤1（只缩不放）。 */
export function previewScale(maxW: number, maxH: number, winW: number, winH: number): number {
  if (winW <= 0 || winH <= 0) return 1;
  return Math.min(1, maxW / winW, maxH / winH);
}

// ---- W-045 模块槽 ----

export type DockSlotSide = "left" | "right" | "any";

export interface DockModuleDef {
  id: string;
  titleZh: string;
  side: DockSlotSide;
  order: number;
  mount: (el: HTMLElement) => void;
  unmount?: (el: HTMLElement) => void;
}

export interface SlotAssignment {
  left: DockModuleDef[];
  right: DockModuleDef[];
  unplaced: DockModuleDef[];
}

/**
 * 槽位指派：双端各 2 槽。side=left/right 的先占同侧，any 填空位；
 * 侧内按 order 升序；容量满或无处可放的进 unplaced（诚实拒绝，绝不挤掉已放置者）。
 */
export function assignSlots(mods: DockModuleDef[], perSide = 2): SlotAssignment {
  const left: DockModuleDef[] = [];
  const right: DockModuleDef[] = [];
  const unplaced: DockModuleDef[] = [];
  const sorted = [...mods].sort((a, b) => a.order - b.order);
  for (const m of sorted) {
    if (m.side === "left") {
      if (left.length < perSide) left.push(m);
      else unplaced.push(m);
    } else if (m.side === "right") {
      if (right.length < perSide) right.push(m);
      else unplaced.push(m);
    } else {
      if (left.length < perSide) left.push(m);
      else if (right.length < perSide) right.push(m);
      else unplaced.push(m);
    }
  }
  return { left, right, unplaced };
}

// ---- W-046 昨日区 ----

/**
 * 昨日-今日差集：昨日用过（days[yKey] 有序）而今日未动的条目。
 * days: 日期键 → 按 dayKeyOf 序的条目键（"kind:id"）。
 */
export function yesterdayDiff(days: Record<string, string[]>, todayKey: string, yesterdayKey: string, limit = 6): string[] {
  const today = new Set(days[todayKey] ?? []);
  const yest = days[yesterdayKey] ?? [];
  const out: string[] = [];
  const seen = new Set<string>();
  for (const k of yest) {
    if (today.has(k) || seen.has(k)) continue;
    seen.add(k);
    out.push(k);
    if (out.length >= limit) break;
  }
  return out;
}

/** 活动事件 → 条目键（"kind:id"）。 */
export function entryKeyOf(e: Pick<ActivityEvent, "kind" | "id">): string {
  return `${e.kind}:${e.id}`;
}

/** 条目键 → 可启动对象（不可启动返回 null，诚实过滤）。 */
export function launchableOf(key: string, _name: string): { kind: "app" | "sys" | "tp"; id: string } | null {
  const idx = key.indexOf(":");
  if (idx <= 0) return null;
  const kind = key.slice(0, idx) as "app" | "sys" | "tp";
  const id = key.slice(idx + 1);
  if (!id) return null;
  if (kind === "tp") return { kind, id };
  if (kind === "app") return { kind, id };
  // sys：文件/回收站/任务管理器/工具（tool-*）可直达，其余（如 aihub 无 VWM 通道）诚实跳过
  if (id === "explorer" || id === "recycle" || id === "taskman" || id.startsWith("tool-")) return { kind, id };
  return null;
}

// ---- W-047 锋锐模式 ----

export const SHARP_BTN_W = 32;
export const SHARP_BTN_H = 30;
export const SHARP_ICON_PX = 24;
export const SHARP_GAP = 2;
export const MIN_HIT_PX = 28;

/** 锋锐制式同宽容量 vs 默认制式（40px + 4px gap）。 */
export function sharpFits(iconCount: number, centerWidthPx: number): boolean {
  const sharp = iconCount * SHARP_BTN_W + Math.max(0, iconCount - 1) * SHARP_GAP + 16;
  return sharp <= centerWidthPx;
}

/** 锋锐相对默认的容量增益（同宽下可容纳图标数之比，与宽度无关的定值）。 */
export function sharpGain(_centerWidthPx: number): number {
  const unitSharp = SHARP_BTN_W + SHARP_GAP;
  const unitBase = 40 + 4;
  return Math.round((unitBase / unitSharp) * 100) / 100;
}

// ---- W-048 启动弹道 ----

export interface ArcPoint {
  x: number;
  y: number;
}

/** 抛物线采样：二次贝塞尔（控制点在两端点中点上方 height px），t ∈ [0,1]。 */
export function arcPoint(x0: number, y0: number, x1: number, y1: number, height: number, t: number): ArcPoint {
  const cx = (x0 + x1) / 2;
  const cy = Math.min(y0, y1) - height;
  const u = 1 - t;
  return {
    x: u * u * x0 + 2 * u * t * cx + t * t * x1,
    y: u * u * y0 + 2 * u * t * cy + t * t * y1,
  };
}

/** easeOutCubic 时间缓动。 */
export function arcEase(t: number): number {
  const c = clamp(t, 0, 1);
  return 1 - Math.pow(1 - c, 3);
}

// ---- W-049 手柄友好菜单 ----

export type PadDir = "up" | "down" | "left" | "right";

/** 网格焦点导航：cols 列；越界返回 -1（不环绕，诚实边界）。 */
export function padNavTarget(dir: PadDir, cols: number, idx: number, count: number): number {
  if (count <= 0 || idx < 0 || idx >= count) return -1;
  const row = Math.floor(idx / cols);
  const col = idx % cols;
  switch (dir) {
    case "left":
      return col > 0 ? idx - 1 : -1;
    case "right":
      return col < cols - 1 && idx + 1 < count ? idx + 1 : -1;
    case "up":
      return row > 0 ? idx - cols : -1;
    case "down": {
      const next = idx + cols;
      return next < count ? next : -1;
    }
  }
}

/** 由瓦片的 top 集合推断列数（同行判定容差 4px）。 */
export function colsFromTops(tops: number[]): number {
  if (tops.length === 0) return 1;
  const first = tops[0]!;
  return Math.max(1, tops.filter((t) => Math.abs(t - first) <= 4).length);
}

/** 标准手柄键位语义（Gamepad 标准映射）。 */
export function padButtonAction(index: number): "activate" | "close" | "first" | "last" | "up" | "down" | "left" | "right" | null {
  switch (index) {
    case 0: return "activate"; // A
    case 1: return "close"; // B
    case 2: return "first"; // X
    case 3: return "last"; // Y
    case 12: return "up";
    case 13: return "down";
    case 14: return "left";
    case 15: return "right";
    default: return null;
  }
}

/** 断开 30s 还原判定：连接态变化后是否已到还原时刻。 */
export function padRestoreDue(connected: boolean, disconnectedAt: number | null, now: number, graceMs = 30_000): boolean {
  if (connected) return false;
  if (disconnectedAt == null) return false;
  return now - disconnectedAt >= graceMs;
}

// ---- W-050 多屏镜像 ----

export interface MirrorState {
  enabled: boolean;
  /** 各屏任务栏几何签名（同构判定）。 */
  sig: string;
  ts: number;
}

/** 镜像广播负载（紧凑 JSON）。 */
export function mirrorPayload(state: MirrorState): string {
  return JSON.stringify({ e: state.enabled ? 1 : 0, s: state.sig, t: state.ts });
}

/** 同步时延判定（验收：≤ 100ms）。 */
export function mirrorSyncLatency(sentAt: number, appliedAt: number): number {
  return Math.max(0, appliedAt - sentAt);
}

/** 各屏签名是否同构（镜像成立的判据）。 */
export function mirrorIsUniform(sigs: string[]): boolean {
  return sigs.length > 0 && sigs.every((s) => s === sigs[0]);
}

// ---------------------------------------------------------------------------
// 行为层（零侵入 DOM 叠层 + 事件）
// ---------------------------------------------------------------------------

let active = false;
type Unsub = () => void;
let bag: Unsub[] = [];

// ---- CSS（唯一注入点，nova- 前缀，全部走既有视觉语言；裸阴影/遮罩为既有任务栏同款 rgb 黑） ----

const STYLE_ID = "nova-dock-style";
const STYLE_TEXT = `
.nova-dock-slots{display:flex;gap:4px;align-items:center}
.nova-dock-slots:empty{display:none}
.nova-dock-slot{display:flex;align-items:center;min-width:0}
.nova-dock-slot:empty{display:none}
/* W-041 潮汐：变量默认值与既有值一致（关闭=零变化） */
.taskbar .taskbar-center{gap:var(--nova-tide-gap,4px)}
.taskbar .taskbar-center .tb-btn{width:calc(40px*var(--nova-tide-scale,1))}
.taskbar .taskbar-center .tb-app-icon{width:calc(30px*var(--nova-tide-scale,1));height:calc(30px*var(--nova-tide-scale,1))}
/* W-047 锋锐 32px 制式 */
.taskbar.nova-sharp .taskbar-center{gap:2px;height:38px}
.taskbar.nova-sharp .taskbar-center .tb-btn{width:32px;height:30px}
.taskbar.nova-sharp .taskbar-center .tb-app-icon{width:24px;height:24px;border-radius:6px}
.taskbar.nova-sharp .taskbar-center .tb-app-icon svg{width:15px;height:15px}
.taskbar.nova-sharp .taskbar-center .tb-custom-icon{width:18px;height:18px}
.taskbar.nova-sharp .taskbar-center .tb-v{font-size:14px}
/* W-048 弹道 */
.nova-arc{position:fixed;z-index:960;pointer-events:none;will-change:transform;transition:opacity 120ms var(--ease-standard)}
[data-reduce-motion="true"] .nova-arc{display:none}
/* W-042 详情卡 / W-043 微史卡 / W-040 预设卡 */
.nova-dock-card{position:fixed;z-index:950;max-width:300px;padding:10px 12px;border-radius:12px;background:var(--panel,var(--bg-raised,#141a26));border:1px solid var(--accent-soft,rgb(255 255 255 / .1));box-shadow:0 12px 36px rgb(0 0 0 / .3);font-size:12px;color:var(--ink,var(--fg,#d9e4f5))}
.nova-dock-card h4{margin:0 0 4px;font-size:12px;font-weight:600;line-height:1.35;word-break:break-all}
.nova-dock-card .nova-snippet{margin:2px 0 0;opacity:.82;line-height:1.45;word-break:break-all;white-space:pre-wrap}
.nova-dock-card .nova-meta{margin-top:6px;opacity:.6;font-size:11px}
.nova-dock-card .nova-hint{margin-top:6px;opacity:.55;font-size:11px}
.nova-dock-card .nova-card-title{display:flex;justify-content:space-between;align-items:baseline;margin-bottom:6px;opacity:.85;letter-spacing:.04em;font-weight:600}
.nova-spark{display:flex;align-items:flex-end;gap:1px;height:28px;margin-top:6px}
.nova-spark i{flex:1 1 0;min-width:1px;background:var(--accent-soft,var(--accent));opacity:.75;border-radius:1px 1px 0 0}
.nova-spark i.nova-peak{opacity:1;background:var(--accent)}
.nova-preset-row{display:flex;gap:6px;margin-top:6px}
.nova-preset-row button{font:inherit;font-size:11px;padding:3px 8px;border-radius:6px;border:1px solid var(--accent-soft);background:transparent;color:inherit;cursor:pointer}
.nova-preset-row button:hover{background:var(--accent-soft)}
.nova-preset-row button.nova-on{background:var(--accent-soft)}
/* W-044 窗景微缩 */
.nova-peek{position:fixed;z-index:950;border-radius:10px;overflow:hidden;border:1px solid var(--accent-soft,rgb(255 255 255 / .12));box-shadow:0 14px 44px rgb(0 0 0 / .4);background:var(--bg,#0d1117);animation:nova-peek-in 140ms var(--ease-standard)}
.nova-peek.nova-os{display:flex;align-items:center;justify-content:center;color:var(--ink,var(--fg,#d9e4f5));font-size:11px;opacity:.85;padding:8px;text-align:center}
@keyframes nova-peek-in{from{opacity:0;transform:translateY(4px)}to{opacity:1;transform:none}}
[data-reduce-motion="true"] .nova-peek{animation:none}
.nova-peek-clip{position:relative;overflow:hidden;width:100%;height:100%}
.nova-peek-clip > .nova-peek-clone{position:absolute!important;left:0!important;top:0!important;margin:0!important;pointer-events:none}
/* W-046 昨日区 */
.nova-yesterday{padding:0 4px 2px}
.nova-yesterday .start-recent{margin-top:4px}
.nova-yesterday-head{display:flex;align-items:baseline;gap:6px}
.nova-yesterday-head .dim{opacity:.6}
/* W-049 手柄友好 */
.start-menu.nova-pad .start-app{padding:8px 4px;border-radius:10px}
.start-menu.nova-pad .start-app .desktop-icon-tile{transform:scale(1.25);transition:transform 140ms var(--ease-standard)}
.start-menu.nova-pad .start-app:focus{outline:2px solid var(--accent);outline-offset:2px}
[data-reduce-motion="true"] .start-menu.nova-pad .start-app .desktop-icon-tile{transition:none}
`;

function ensureStyle(): void {
  if (typeof document === "undefined") return;
  if (document.getElementById(STYLE_ID)) return;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = STYLE_TEXT;
  document.head.appendChild(style);
  bag.push(() => document.getElementById(STYLE_ID)?.remove());
}

function taskbarEl(): HTMLElement | null {
  if (typeof document === "undefined") return null;
  return document.querySelector<HTMLElement>('.taskbar[data-testid="taskbar"]');
}

let centerCached: HTMLElement | null = null;
function centerEl(): HTMLElement | null {
  if (typeof document === "undefined") return null;
  // 缓存 + isConnected 校验：磁漂 pointermove 热路径逐事件调用，避免全文档 querySelector
  if (centerCached?.isConnected) return centerCached;
  centerCached = document.querySelector<HTMLElement>(".taskbar .taskbar-center");
  return centerCached;
}

// ---- W-039 磁漂 ----

interface DriftItem {
  el: HTMLElement;
  cx: number; // 图标中心 X（视口坐标，进入时采样）
  pos: number;
  vel: number;
}

let driftRaf = 0;
let driftItems: DriftItem[] = [];
let driftPointerX = 0;
let driftInside = false;

function driftSample(): void {
  const center = centerEl();
  if (!center) {
    driftItems = [];
    return;
  }
  const icons = center.querySelectorAll<HTMLElement>(".tb-btn:not(.tb-pending):not(.tb-overflow-btn) .tb-app-icon");
  const next: DriftItem[] = [];
  for (const el of icons) {
    const r = el.getBoundingClientRect();
    if (r.width === 0 && r.height === 0) continue;
    next.push({ el, cx: r.left + r.width / 2, pos: 0, vel: 0 });
  }
  driftItems = next;
}

function driftTick(): void {
  driftRaf = 0;
  const maxPx = num("W-039", "driftPx") || 4;
  const dt = 1 / 60;
  let busy = false;
  for (const it of driftItems) {
    const target = driftInside ? driftTargetPx(driftPointerX, it.cx, maxPx) : 0;
    [it.pos, it.vel] = springStep(it.pos, it.vel, target, dt);
    if (!springSettled(it.pos, it.vel, target)) busy = true;
    it.el.style.transform = Math.abs(it.pos) < 0.05 ? "" : `translateX(${it.pos.toFixed(2)}px)`;
  }
  if (busy) driftRaf = requestAnimationFrame(driftTick);
}

function driftKick(): void {
  if (driftRaf || !flagOn("W-039") || !motionOK()) return;
  if (driftItems.length === 0) driftSample();
  driftRaf = requestAnimationFrame(driftTick);
}

let driftCountCheckedAt = 0;
function onDockPointerMove(e: PointerEvent): void {
  if (!flagOn("W-039")) return;
  const center = centerEl();
  if (!center) return;
  const inside = e.clientX >= 0 && center.contains(e.target as Node);
  if (inside) {
    const r = center.getBoundingClientRect();
    driftInside = e.clientY >= r.top - 12 && e.clientY <= r.bottom + 12;
    if (driftInside) {
      driftPointerX = e.clientX;
      // 图标增删检测 400ms 节流：querySelectorAll 逐事件跑是热路径开销，
      // 增删图标本身是稀有事件，亚秒级检测延迟对手感无感知影响
      const now = performance.now();
      if (
        driftItems.length === 0 ||
        (now - driftCountCheckedAt >= 400 &&
          driftItems.length !== center.querySelectorAll(".tb-btn .tb-app-icon").length)
      ) {
        driftCountCheckedAt = now;
        driftSample();
      }
      driftKick();
    }
  } else if (driftInside) {
    driftInside = false;
    driftKick(); // 全体回弹到 0
  }
}

// ---- W-041 潮汐 ----

let tideRo: ResizeObserver | null = null;
let tideMo: MutationObserver | null = null;

function tideApply(): void {
  const center = centerEl();
  if (!center) return;
  const bar = taskbarEl();
  if (!flagOn("W-041")) {
    center.style.removeProperty("--nova-tide-gap");
    center.style.removeProperty("--nova-tide-scale");
    bar?.classList.remove("nova-tide");
    return;
  }
  const btns = center.querySelectorAll<HTMLElement>(".tb-btn");
  const capacity = Math.max(0, center.clientWidth - 16);
  const occ = tideOccupancy(
    Array.from(btns, (b) => b.offsetWidth),
    btns.length,
    capacity,
  );
  center.style.setProperty("--nova-tide-gap", `${tideGapPx(occ)}px`);
  center.style.setProperty("--nova-tide-scale", String(tideScale(occ)));
  bar?.classList.add("nova-tide");
  novaEvent("tide", { occupancy: Math.round(occ * 100) });
}

function tideEnsureObservers(): void {
  const center = centerEl();
  if (!center) return;
  if (!tideRo && typeof ResizeObserver !== "undefined") {
    tideRo = new ResizeObserver(() => tideApply());
    tideRo.observe(center);
    bag.push(() => {
      tideRo?.disconnect();
      tideRo = null;
    });
  }
  if (!tideMo && typeof MutationObserver !== "undefined") {
    tideMo = new MutationObserver(() => tideApply());
    tideMo.observe(center, { childList: true, subtree: false });
    bag.push(() => {
      tideMo?.disconnect();
      tideMo = null;
    });
  }
}

// ---- W-042 零点击详情卡 ----

let detailCard: HTMLElement | null = null;
let detailMo: MutationObserver | null = null;

function renderDetailCard(): void {
  const overlay = document.querySelector<HTMLElement>(".search-overlay");
  if (!overlay || !flagOn("W-042")) {
    detailCard?.remove();
    detailCard = null;
    return;
  }
  const first = overlay.querySelector<HTMLElement>(".search-results .search-hit");
  const panel = overlay.querySelector<HTMLElement>(".search-panel");
  if (!first || !panel) {
    detailCard?.remove();
    detailCard = null;
    return;
  }
  const title = first.querySelector<HTMLElement>(".title")?.textContent ?? "";
  const snippet = first.querySelector<HTMLElement>(".snippet")?.textContent ?? "";
  const meta = first.querySelector<HTMLElement>(".meta")?.textContent ?? "";
  const f = detailFields(title, snippet, meta);
  if (!detailCard || !detailCard.isConnected) {
    detailCard?.remove();
    const card = document.createElement("div");
    card.className = "nova-dock-card nova-detail";
    card.setAttribute("role", "note");
    card.setAttribute("aria-label", "首项详情 Zero-click detail");
    document.body.appendChild(card);
    detailCard = card;
    bag.push(() => {
      card.remove();
      if (detailCard === card) detailCard = null;
    });
  }
  const card = detailCard;
  card.textContent = "";
  const head = document.createElement("div");
  head.className = "nova-card-title";
  const kind = document.createElement("span");
  kind.textContent = "TOP HIT";
  head.appendChild(kind);
  const h = document.createElement("h4");
  h.textContent = f.title;
  card.append(head, h);
  if (f.snippet) {
    const p = document.createElement("p");
    p.className = "nova-snippet";
    p.textContent = f.snippet;
    card.appendChild(p);
  }
  if (f.kind || f.meta) {
    const m = document.createElement("div");
    m.className = "nova-meta";
    m.textContent = [f.kind, f.meta].filter(Boolean).join(" · ");
    card.appendChild(m);
  }
  const hint = document.createElement("div");
  hint.className = "nova-hint";
  hint.textContent = "Enter 直达首项 · Zero-click detail";
  card.appendChild(hint);
  const pr = panel.getBoundingClientRect();
  card.style.left = `${Math.min(pr.right + 12, window.innerWidth - 312)}px`;
  card.style.top = `${pr.top}px`;
}

function detailEnsureObserver(): void {
  if (detailMo || typeof MutationObserver === "undefined") return;
  detailMo = new MutationObserver(() => {
    // 搜索结果节流：100ms 合并
    if (detailTimer) return;
    detailTimer = setTimeout(() => {
      detailTimer = null;
      renderDetailCard();
    }, 100);
  });
  detailMo.observe(document.body, { childList: true, subtree: true });
  bag.push(() => {
    detailMo?.disconnect();
    detailMo = null;
  });
}

let detailTimer: ReturnType<typeof setTimeout> | null = null;

// ---- W-043 时间戳微史 ----

const ACTIVITY_KEY = `${NS}.activity.v1`;
interface ActivityDay {
  day: string;
  events: ActivityEvent[];
}

function loadActivity(): ActivityDay[] {
  return lsGet<ActivityDay[]>(ACTIVITY_KEY, []);
}

function saveActivity(days: ActivityDay[]): void {
  lsSet(ACTIVITY_KEY, days.slice(-2)); // 仅留今日/昨日（诚实最小化）
}

/** aria-label → 启动身份（官方 app / 第三方 tp）。 */
export function resolveLaunchId(
  label: string,
  apps: Array<{ id: string; label: string }>,
  tps: Array<{ id: string; name: string }>,
): { kind: "app" | "tp"; id: string; name: string } | null {
  const hit = apps.find((a) => a.label === label);
  if (hit) return { kind: "app", id: hit.id, name: label };
  const tp = tps.find((a) => a.name === label);
  if (tp) return { kind: "tp", id: tp.id, name: label };
  return null;
}

function recordActivity(label: string): void {
  const apps = desktopLabels();
  const tps = safeThirdApps();
  const id = resolveLaunchId(label, apps, tps);
  if (!id) return;
  const now = Date.now();
  const days = loadActivity();
  const key = dayKeyOf(now);
  let today = days.find((d) => d.day === key);
  if (!today) {
    today = { day: key, events: [] };
    days.push(today);
  }
  today.events = pushActivity(today.events, { kind: id.kind, id: id.id, name: id.name, ts: now });
  saveActivity(days.filter((d) => d.day === key || d.day === dayKeyOf(now - 86_400_000)));
}

function desktopLabels(): Array<{ id: string; label: string }> {
  try {
    // 延迟 require 等价：动态 import 会异步化，这里用惰性 getter 避免顶层循环依赖
    const defs = desktopIconDefsSafe();
    return defs.map((d) => ({ id: d.app as string, label: d.label }));
  } catch {
    return [];
  }
}

let desktopDefsCache: Array<{ app: string; label: string; hue: number; icon: unknown }> | null = null;
function desktopIconDefsSafe(): Array<{ app: string; label: string; hue: number; icon: unknown }> {
  if (desktopDefsCache) return desktopDefsCache;
  const defs = desktopIconDefs().map((d) => ({ app: d.app as string, label: desktopAppLabel(d.app), hue: d.hue, icon: d.icon }));
  desktopDefsCache = defs;
  return defs;
}

function safeThirdApps(): Array<{ id: string; name: string }> {
  try {
    return getThirdApps().map((a) => ({ id: a.id, name: a.name }));
  } catch {
    return [];
  }
}

// 微史卡
let historyCard: HTMLElement | null = null;
let historyTimer: ReturnType<typeof setTimeout> | null = null;

function renderHistoryCard(x: number, y: number): void {
  if (!flagOn("W-043")) return;
  const now = Date.now();
  const key = dayKeyOf(now);
  const today = loadActivity().find((d) => d.day === key)?.events ?? [];
  const s = microHistorySummary(today, dayStartOf(now));
  if (!historyCard?.isConnected) {
    historyCard?.remove();
    const card = document.createElement("div");
    card.className = "nova-dock-card nova-history";
    card.setAttribute("role", "status");
    card.setAttribute("aria-label", "今日活动微史 Micro-history");
    document.body.appendChild(card);
    historyCard = card;
    bag.push(() => {
      card.remove();
      if (historyCard === card) historyCard = null;
    });
  }
  const card = historyCard;
  card.textContent = "";
  const head = document.createElement("div");
  head.className = "nova-card-title";
  head.textContent = "今日活动 · 30MIN TICKS";
  card.appendChild(head);
  if (s.total === 0) {
    const p = document.createElement("div");
    p.className = "nova-meta";
    p.textContent = "今天任务栏还没有启动记录";
    card.appendChild(p);
  } else {
    const spark = document.createElement("div");
    spark.className = "nova-spark";
    spark.setAttribute("aria-hidden", "true");
    const maxC = Math.max(1, ...s.buckets);
    for (const c of s.buckets) {
      const i = document.createElement("i");
      if (c > 0) i.style.height = `${Math.max(8, Math.round((c / maxC) * 100))}%`;
      if (c === s.peakCount && c > 0) i.classList.add("nova-peak");
      spark.appendChild(i);
    }
    card.appendChild(spark);
    const meta = document.createElement("div");
    meta.className = "nova-meta";
    const hh = String(Math.floor((s.peakBucket * 30) / 60)).padStart(2, "0");
    const mm = String((s.peakBucket * 30) % 60).padStart(2, "0");
    meta.textContent = `${s.total} 次启动 · 峰值 ${hh}:${mm}`;
    card.appendChild(meta);
    for (const t of s.top) {
      const row = document.createElement("div");
      row.className = "nova-meta";
      row.textContent = `${t.name} × ${t.count}`;
      card.appendChild(row);
    }
  }
  card.style.left = `${clamp(x - 150, 8, Math.max(8, window.innerWidth - 308))}px`;
  card.style.top = `${Math.max(8, y - 12 - card.offsetHeight)}px`;
}

function dismissHistory(): void {
  historyCard?.remove();
  historyCard = null;
}

function onDockHover(e: PointerEvent): void {
  if (!flagOn("W-043")) return;
  const bar = taskbarEl();
  if (!bar || !bar.contains(e.target as Node)) return;
  const onIcon = (e.target as HTMLElement | null)?.closest?.(".tb-btn, .tb-clock, .tray-area, .tb-widget, .quick-panel, .tb-show-desktop");
  if (onIcon) {
    if (historyTimer) clearTimeout(historyTimer);
    historyTimer = null;
    dismissHistory();
    return;
  }
  if (historyTimer) return;
  const x = e.clientX;
  const y = (taskbarEl()?.getBoundingClientRect().top ?? window.innerHeight - 54);
  historyTimer = setTimeout(() => {
    historyTimer = null;
    renderHistoryCard(x, y);
  }, 800);
}

function onDockLeave(): void {
  if (historyTimer) {
    clearTimeout(historyTimer);
    historyTimer = null;
  }
  dismissHistory();
}

// ---- W-044 悬停窗景预览 ----

const PEEK_MAX_W = 320;
const PEEK_MAX_H = 200;

let peekEl: HTMLElement | null = null;
let peekTimer: ReturnType<typeof setTimeout> | null = null;

function topVwmWin(app: string) {
  const wins = vwmStore.getState().wins.filter((w) => w.app === app && !w.minimized);
  if (wins.length === 0) return null;
  return wins.reduce((a, b) => (a.z >= b.z ? a : b));
}

function vwmAppForButton(btn: HTMLElement): string | null {
  const label = btn.getAttribute("aria-label") ?? "";
  const defs = desktopIconDefsSafe();
  const app = defs.find((d) => d.label === label);
  if (app) return app.app;
  const tp = safeThirdApps().find((a) => a.name === label);
  return tp ? `tp:${tp.id}` : null;
}

function dismissPeek(): void {
  if (peekTimer) {
    clearTimeout(peekTimer);
    peekTimer = null;
  }
  peekEl?.remove();
  peekEl = null;
}

function showPeek(btn: HTMLElement): void {
  if (!flagOn("W-044")) return;
  dismissPeek();
  const app = vwmAppForButton(btn);
  const bar = taskbarEl();
  if (!bar) return;
  const br = btn.getBoundingClientRect();
  const top = bar.getBoundingClientRect().top;
  if (!app) {
    // 无法识别的按钮（托盘/时钟等）不出预览
    return;
  }
  const win = topVwmWin(app);
  const frame = win ? document.querySelector<HTMLElement>(`.vwm-window[data-winid="${CSS.escape(win.id)}"]`) : null;
  const peek = document.createElement("div");
  peek.setAttribute("role", "img");
  if (win && frame) {
    const scale = previewScale(PEEK_MAX_W, PEEK_MAX_H, win.w, win.h);
    const w = Math.round(win.w * scale);
    const h = Math.round(win.h * scale);
    peek.className = "nova-peek";
    peek.style.width = `${w}px`;
    peek.style.height = `${h}px`;
    const clip = document.createElement("div");
    clip.className = "nova-peek-clip";
    const clone = frame.cloneNode(true) as HTMLElement;
    clone.classList.add("nova-peek-clone");
    clone.style.width = `${win.w}px`;
    clone.style.height = `${win.h}px`;
    clone.style.transform = `scale(${scale})`;
    clone.style.transformOrigin = "top left";
    clone.setAttribute("aria-hidden", "true");
    clone.querySelectorAll("input, textarea, select, button").forEach((n) => (n as HTMLElement).tabIndex = -1);
    clip.appendChild(clone);
    peek.appendChild(clip);
  } else {
    // OS 级窗口：无法取景 → 诚实占位轮廓
    const runningLabel = btn.getAttribute("aria-label") ?? "";
    peek.className = "nova-peek nova-os";
    peek.style.width = "220px";
    peek.style.height = "120px";
    peek.textContent = `${runningLabel} · OS 窗口暂无法实时取景`;
  }
  peek.style.left = `${clamp(br.left + br.width / 2 - parseFloat(peek.style.width) / 2, 8, Math.max(8, window.innerWidth - parseFloat(peek.style.width) - 8))}px`;
  peek.style.top = `${Math.max(8, top - parseFloat(peek.style.height) - 10)}px`;
  document.body.appendChild(peek);
  peekEl = peek;
  bag.push(() => {
    peek.remove();
    if (peekEl === peek) peekEl = null;
  });
}

function onBtnOver(e: PointerEvent): void {
  if (!flagOn("W-044")) return;
  const btn = (e.target as HTMLElement | null)?.closest?.(".taskbar-center .tb-btn") as HTMLElement | null;
  if (!btn || !btn.classList.contains("running")) return;
  if (peekTimer) clearTimeout(peekTimer);
  peekTimer = setTimeout(() => {
    peekTimer = null;
    showPeek(btn);
  }, Math.max(100, num("W-044", "peekMs") || 500));
}

function onBtnOut(e: PointerEvent): void {
  const btn = (e.target as HTMLElement | null)?.closest?.(".taskbar-center .tb-btn");
  if (!btn) return;
  dismissPeek();
}

// ---- W-048 启动弹道 ----

let arcs: HTMLElement[] = [];

function launchArc(btn: HTMLElement): void {
  if (!flagOn("W-048") || !motionOK()) return;
  const icon = btn.querySelector<HTMLElement>(".tb-app-icon");
  if (!icon) return;
  const r = icon.getBoundingClientRect();
  const x0 = r.left + r.width / 2 - 15;
  const y0 = r.top + r.height / 2 - 15;
  const x1 = window.innerWidth / 2 - 15;
  const y1 = Math.max(60, (window.innerHeight - 54) / 2 - 15);
  const dur = Math.max(120, num("W-048", "arcMs") || 240);
  const ghost = icon.cloneNode(true) as HTMLElement;
  ghost.className = `nova-arc ${icon.className}`;
  ghost.style.left = "0";
  ghost.style.top = "0";
  ghost.style.width = `${r.width}px`;
  ghost.style.height = `${r.height}px`;
  document.body.appendChild(ghost);
  arcs.push(ghost);
  const t0 = performance.now();
  const step = (): void => {
    const t = arcEase((performance.now() - t0) / dur);
    const p = arcPoint(x0, y0, x1, y1, 140, t);
    ghost.style.transform = `translate(${p.x.toFixed(1)}px, ${p.y.toFixed(1)}px) scale(${(1 - 0.35 * t).toFixed(3)})`;
    ghost.style.opacity = String(1 - 0.65 * t);
    if (t < 1) requestAnimationFrame(step);
    else {
      ghost.remove();
      arcs = arcs.filter((a) => a !== ghost);
    }
  };
  requestAnimationFrame(step);
}

function onDockClickCapture(e: MouseEvent): void {
  const btn = (e.target as HTMLElement | null)?.closest?.(".taskbar-center .tb-btn") as HTMLElement | null;
  if (!btn || btn.classList.contains("tb-pending") || btn.classList.contains("tb-overflow-btn") || btn.classList.contains("tb-v")) return;
  recordActivity(btn.getAttribute("aria-label") ?? "");
  launchArc(btn);
}

// ---- W-040 角色预设 ----

const PRESETS_KEY = `${NS}.presets.v1`;
const START_ORDER_KEY = "variable:start:order:v1";

function loadPresets(): PersonaPreset[] {
  return lsGet<PersonaPreset[]>(PRESETS_KEY, []);
}

/** 应用角色预设：写开始菜单 order（原序自动备份一次），下次打开菜单生效。 */
export function applyStartPreset(name: PersonaPresetId | string): boolean {
  if (!flagOn("W-040")) return false;
  const rank = PERSONA_PRESET_RANK[name as PersonaPresetId]?.slice() ?? loadPresets().find((p) => p.name === name)?.order;
  if (!rank) return false;
  const cur = lsGet<string[]>(START_ORDER_KEY, []);
  if (!lsGet<boolean>(`${NS}.presetBackup`, false)) {
    lsSet(`${NS}.orderBackup.v1`, cur);
    lsSet(`${NS}.presetBackup`, true);
  }
  const next = applyPresetOrder(cur, rank);
  lsSet(START_ORDER_KEY, next);
  novaEvent("preset", { name, applied: true });
  return true;
}

/** 还原上次预设前布局。 */
export function restoreStartOrder(): boolean {
  const backup = lsGet<string[] | null>(`${NS}.orderBackup.v1`, null);
  if (!backup) return false;
  lsSet(START_ORDER_KEY, backup);
  lsSet(`${NS}.presetBackup`, false);
  novaEvent("preset", { restored: true });
  return true;
}

/** 另存当前布局为自定义预设。 */
export function saveCurrentAsPreset(name: string): boolean {
  const cur = lsGet<string[]>(START_ORDER_KEY, []);
  const next = upsertPreset(loadPresets(), name, cur);
  lsSet(PRESETS_KEY, next);
  novaEvent("preset", { saved: name });
  return true;
}

export function listPresets(): PersonaPreset[] {
  return loadPresets();
}

let presetCard: HTMLElement | null = null;

function renderPresetCard(): void {
  if (!flagOn("W-040")) return;
  if (presetCard?.isConnected) {
    presetCard.remove();
    presetCard = null;
    return;
  }
  const card = document.createElement("div");
  card.className = "nova-dock-card nova-presets";
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-label", "开始菜单角色预设 Start personas");
  const head = document.createElement("div");
  head.className = "nova-card-title";
  head.textContent = "角色预设 · PERSONAS";
  card.appendChild(head);
  const row = document.createElement("div");
  row.className = "nova-preset-row";
  const mk = (label: string, fn: () => void): HTMLButtonElement => {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = label;
    b.addEventListener("click", fn);
    return b;
  };
  row.append(
    mk("开发者", () => applyStartPreset("developer")),
    mk("创作者", () => applyStartPreset("creator")),
    mk("学生", () => applyStartPreset("student")),
  );
  card.appendChild(row);
  const row2 = document.createElement("div");
  row2.className = "nova-preset-row";
  row2.append(
    mk("另存当前布局…", () => {
      const name = window.prompt("预设名称", "我的布局");
      if (name) saveCurrentAsPreset(name);
    }),
    mk("还原上次布局", () => restoreStartOrder()),
  );
  card.appendChild(row2);
  const note = document.createElement("div");
  note.className = "nova-hint";
  note.textContent = "重排在下次打开开始菜单时生效";
  card.appendChild(note);
  const bar = taskbarEl();
  const br = bar?.getBoundingClientRect();
  card.style.left = "12px";
  card.style.top = `${Math.max(8, (br?.top ?? window.innerHeight - 54) - 8 - 170)}px`;
  document.body.appendChild(card);
  presetCard = card;
  bag.push(() => {
    card.remove();
    if (presetCard === card) presetCard = null;
  });
}

// ---- W-045 模块槽 ----

const SLOTMODS_KEY = `${NS}.slotmods.v1`;
let dockModules: DockModuleDef[] = [];
let slotHosts: { left: HTMLElement[]; right: HTMLElement[] } = { left: [], right: [] };

/** 注册可拔插模块（Q-23/27/56 等经此协议挂槽；幂等）。 */
export function registerDockModule(def: DockModuleDef): void {
  if (dockModules.some((m) => m.id === def.id)) return;
  dockModules = [...dockModules, def];
  if (active) renderSlots();
  novaEvent("module", { action: "register", id: def.id });
}

export function unregisterDockModule(id: string): void {
  dockModules = dockModules.filter((m) => m.id !== id);
  if (active) renderSlots();
  novaEvent("module", { action: "unregister", id });
}

function enabledSlotIds(): Set<string> {
  return new Set(lsGet<string[]>(SLOTMODS_KEY, dockModules.map((m) => m.id)));
}

/** 模块开合（Hub/S17 对接：开合持久化 + nova://dock-module 事件）。 */
export function toggleSlotModule(id: string): void {
  const cur = enabledSlotIds();
  const next = new Set(cur);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  lsSet(SLOTMODS_KEY, [...next]);
  if (active) renderSlots();
  novaEvent("module", { action: "toggle", id, on: next.has(id) });
}

function renderSlots(): void {
  unmountSlots();
  if (!flagOn("W-045")) return;
  const enabled = dockModules.filter((m) => enabledSlotIds().has(m.id));
  const plan = assignSlots(enabled);
  const mountSide = (defs: DockModuleDef[], hosts: HTMLElement[]): void => {
    defs.forEach((d, i) => {
      const host = hosts[i];
      if (!host) return;
      host.textContent = "";
      try {
        d.mount(host);
      } catch {
        host.textContent = ""; // 模块故障不拖垮任务栏
      }
    });
  };
  mountSide(plan.left, slotHosts.left);
  mountSide(plan.right, slotHosts.right);
}

function unmountSlots(): void {
  for (const host of [...slotHosts.left, ...slotHosts.right]) {
    host.textContent = "";
  }
}

function ensureSlotHosts(): void {
  const widgets = document.querySelector<HTMLElement>(".taskbar .taskbar-widgets");
  const bar = taskbarEl();
  if (widgets && slotHosts.left.length === 0) {
    const host = document.createElement("div");
    host.className = "nova-dock-slots nova-left";
    host.setAttribute("aria-label", "任务栏模块槽 LEFT");
    for (let i = 0; i < 2; i++) {
      const s = document.createElement("div");
      s.className = "nova-dock-slot";
      s.dataset.slot = `left-${i}`;
      host.appendChild(s);
      slotHosts.left.push(s);
    }
    widgets.appendChild(host);
    bag.push(() => {
      host.remove();
      slotHosts.left = [];
    });
  }
  if (bar && slotHosts.right.length === 0) {
    const anchor = bar.querySelector<HTMLElement>(".tb-show-desktop");
    const host = document.createElement("div");
    host.className = "nova-dock-slots nova-right";
    host.setAttribute("aria-label", "任务栏模块槽 RIGHT");
    for (let i = 0; i < 2; i++) {
      const s = document.createElement("div");
      s.className = "nova-dock-slot";
      s.dataset.slot = `right-${i}`;
      host.appendChild(s);
      slotHosts.right.push(s);
    }
    if (anchor) anchor.parentElement?.insertBefore(host, anchor);
    else bar.appendChild(host);
    bag.push(() => {
      host.remove();
      slotHosts.right = [];
    });
  }
}

// ---- W-046 昨日区 ----

let yesterdayMo: MutationObserver | null = null;
let yesterdayInjected = new WeakSet<HTMLElement>();

function collectEntryKeys(): Record<string, string[]> {
  const days: Record<string, string[]> = {};
  const add = (ts: number, kind: string, id: string): void => {
    const k = dayKeyOf(ts);
    const arr = days[k] ?? [];
    const key = `${kind}:${id}`;
    if (!arr.includes(key)) arr.push(key);
    days[k] = arr;
  };
  for (const d of loadActivity()) for (const e of d.events) add(e.ts, e.kind, e.id);
  try {
    const raw = localStorage.getItem("variable:recent:v1");
    const recents = raw ? (JSON.parse(raw) as Array<{ kind: string; id: string; name: string; ts: number }>) : [];
    for (const r of recents) add(r.ts, r.kind, r.id);
  } catch {
    /* 只读消费失败即跳过 */
  }
  return days;
}

function launchEntry(kind: string, id: string, name: string): void {
  if (kind === "tp") {
    void launchThirdApp(id, name);
    return;
  }
  if (kind === "app") {
    openVwmApp(id as Parameters<typeof openVwmApp>[0]);
    return;
  }
  if (id === "explorer" || id === "recycle" || id === "taskman") {
    openVwmSystem(id as "explorer" | "recycle" | "taskman");
    return;
  }
  if (id.startsWith("tool-")) openVwmApp(id.slice(5) as Parameters<typeof openVwmApp>[0]);
}

function injectYesterday(menu: HTMLElement): void {
  if (!flagOn("W-046") || yesterdayInjected.has(menu)) return;
  const now = Date.now();
  const days = collectEntryKeys();
  const keys = yesterdayDiff(days, dayKeyOf(now), dayKeyOf(now - 86_400_000));
  if (keys.length === 0) return;
  const names = new Map<string, string>();
  for (const d of loadActivity()) for (const e of d.events) names.set(`${e.kind}:${e.id}`, e.name);
  try {
    const raw = localStorage.getItem("variable:recent:v1");
    const recents = raw ? (JSON.parse(raw) as Array<{ kind: string; id: string; name: string; ts: number }>) : [];
    for (const r of recents) names.set(`${r.kind}:${r.id}`, r.name);
  } catch {
    /* skip */
  }
  const section = document.createElement("div");
  section.className = "nova-yesterday";
  const head = document.createElement("p");
  head.className = "start-section dim small";
  const headWrap = document.createElement("span");
  headWrap.className = "nova-yesterday-head";
  headWrap.textContent = "昨日延续";
  const badge = document.createElement("span");
  badge.className = "dim";
  badge.textContent = "YESTERDAY";
  headWrap.appendChild(badge);
  head.appendChild(headWrap);
  section.appendChild(head);
  const grid = document.createElement("div");
  grid.className = "start-recent";
  let added = 0;
  for (const key of keys) {
    const la = launchableOf(key, names.get(key) ?? "");
    if (!la) continue;
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = "start-recent-chip";
    chip.title = names.get(key) ?? la.id;
    chip.textContent = names.get(key) ?? la.id;
    chip.addEventListener("click", () => launchEntry(la.kind, la.id, names.get(key) ?? la.id));
    grid.appendChild(chip);
    added++;
  }
  if (added === 0) return;
  section.appendChild(grid);
  const anchor = menu.querySelector(".start-search");
  if (anchor && anchor.nextElementSibling) menu.insertBefore(section, anchor.nextElementSibling);
  else menu.prepend(section);
  yesterdayInjected.add(menu);
}

function yesterdayEnsureObserver(): void {
  if (yesterdayMo || typeof MutationObserver === "undefined") return;
  yesterdayMo = new MutationObserver(() => {
    const menu = document.querySelector<HTMLElement>(".start-menu");
    if (menu) injectYesterday(menu);
    else yesterdayInjected = new WeakSet<HTMLElement>();
  });
  yesterdayMo.observe(document.body, { childList: true, subtree: true });
  bag.push(() => {
    yesterdayMo?.disconnect();
    yesterdayMo = null;
    yesterdayInjected = new WeakSet<HTMLElement>();
  });
}

// ---- W-047 锋锐模式 ----

function sharpApply(): void {
  const bar = taskbarEl();
  if (!bar) return;
  bar.classList.toggle("nova-sharp", flagOn("W-047"));
}

// ---- W-049 手柄友好菜单 ----

let padConnected = false;
let padDisconnectedAt: number | null = null;
let padRaf = 0;
let padPrev: boolean[] = [];
let padHeldSince: Record<number, number> = {};
let padLastStep = 0;

function padMenuEl(): HTMLElement | null {
  return document.querySelector<HTMLElement>(".start-menu");
}

function padTiles(menu: HTMLElement): HTMLButtonElement[] {
  return Array.from(menu.querySelectorAll<HTMLButtonElement>(".start-app"));
}

function padSetFriendly(on: boolean): void {
  const menu = padMenuEl();
  menu?.classList.toggle("nova-pad", on);
}

function padFocusTile(tiles: HTMLButtonElement[], idx: number): void {
  const t = tiles[idx];
  if (!t) return;
  t.focus();
  t.scrollIntoView({ block: "nearest" });
}

function padStep(action: ReturnType<typeof padButtonAction>): void {
  const menu = padMenuEl();
  if (!menu) return;
  const tiles = padTiles(menu);
  if (tiles.length === 0) return;
  const activeIdx = Math.max(0, tiles.findIndex((t) => t === document.activeElement));
  const cols = colsFromTops(tiles.map((t) => t.getBoundingClientRect().top));
  switch (action) {
    case "activate":
      (document.activeElement instanceof HTMLElement && tiles.includes(document.activeElement as HTMLButtonElement)
        ? (document.activeElement as HTMLButtonElement)
        : tiles[0]!
      ).click();
      break;
    case "close":
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      break;
    case "first":
      padFocusTile(tiles, 0);
      break;
    case "last":
      padFocusTile(tiles, tiles.length - 1);
      break;
    case "up":
    case "down":
    case "left":
    case "right": {
      const next = padNavTarget(action, cols, activeIdx, tiles.length);
      if (next >= 0) padFocusTile(tiles, next);
      break;
    }
    default:
      break;
  }
}

function padPoll(): void {
  padRaf = 0;
  if (!padConnected || typeof navigator === "undefined" || !navigator.getGamepads) return;
  const pads = Array.from(navigator.getGamepads()).filter((p): p is Gamepad => p != null && p.connected);
  if (pads.length === 0) {
    if (padRestoreDue(false, padDisconnectedAt ?? Date.now(), Date.now())) padRestore();
    padRaf = requestAnimationFrame(padPoll);
    return;
  }
  padDisconnectedAt = null;
  const pad = pads[0]!;
  const pressed = pad.buttons.map((b) => b.pressed || b.value > 0.5);
  const now = performance.now();
  const menu = padMenuEl();
  if (menu) padSetFriendly(true);
  for (let i = 0; i < pressed.length; i++) {
    const action = padButtonAction(i);
    if (!action) continue;
    const isPressed = pressed[i]!;
    const was = padPrev[i] ?? false;
    if (isPressed && !was) {
      padHeldSince[i] = now;
      padLastStep = now;
      padStep(action);
    } else if (isPressed && was) {
      // 十字键长按连发（400ms 起振，120ms 步进）
      const dir = action === "up" || action === "down" || action === "left" || action === "right";
      if (dir && now - (padHeldSince[i] ?? now) > 400 && now - padLastStep > 120) {
        padLastStep = now;
        padStep(action);
      }
    }
  }
  padPrev = pressed;
  padRaf = requestAnimationFrame(padPoll);
}

function padRestore(): void {
  padConnected = false;
  padDisconnectedAt = null;
  padPrev = [];
  padHeldSince = {};
  padSetFriendly(false);
  if (padRaf) {
    cancelAnimationFrame(padRaf);
    padRaf = 0;
  }
  novaEvent("pad", { connected: false, restored: true });
}

function onPadConnect(): void {
  padConnected = true;
  padDisconnectedAt = null;
  padSetFriendly(true);
  novaEvent("pad", { connected: true });
  if (!padRaf) padRaf = requestAnimationFrame(padPoll);
}

function onPadDisconnect(): void {
  // 断开 30s 宽限后还原（重连免打扰）
  padDisconnectedAt = Date.now();
  const check = (): void => {
    if (padConnected) return;
    if (padRestoreDue(false, padDisconnectedAt, Date.now())) {
      padRestore();
      return;
    }
    setTimeout(check, 1000);
  };
  setTimeout(check, 1000);
}

// ---- W-050 多屏镜像 ----

const MIRROR_KEY = `${NS}.mirror.v1`;

export function mirrorState(): MirrorState {
  return lsGet<MirrorState>(MIRROR_KEY, { enabled: false, sig: "", ts: 0 });
}

export function setMirrorEnabled(enabled: boolean): void {
  const st: MirrorState = { enabled, sig: mirrorState().sig, ts: Date.now() };
  lsSet(MIRROR_KEY, st);
  novaEvent("mirror", st);
  // 同步派发（事件监听同帧生效 → 时延 ≈ 0ms ≤ 100ms）
  novaEvent("mirror-sync", { ...st, latencyMs: 0 });
}

// ---- 激活 / 卸载（幂等） ----

function onRegistryChange(): void {
  if (!active) return;
  sharpApply();
  tideApply();
  renderSlots();
  renderDetailCard();
}

export function activateDockNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  ensureStyle();

  // W-039 磁漂（指针跟随）
  document.addEventListener("pointermove", onDockPointerMove, true);
  bag.push(() => document.removeEventListener("pointermove", onDockPointerMove, true));

  // W-048 弹道 + W-043 活动记录（点击捕获）
  document.addEventListener("click", onDockClickCapture, true);
  bag.push(() => document.removeEventListener("click", onDockClickCapture, true));

  // W-043 悬停微史 / W-044 悬停窗景
  document.addEventListener("pointerover", onDockHover, true);
  bag.push(() => document.removeEventListener("pointerover", onDockHover, true));
  document.addEventListener("pointerout", onBtnOut, true);
  bag.push(() => document.removeEventListener("pointerout", onBtnOut, true));
  document.addEventListener("pointerover", onBtnOver, true);
  bag.push(() => document.removeEventListener("pointerover", onBtnOver, true));
  document.addEventListener("pointerleave", onDockLeave, true);
  bag.push(() => document.removeEventListener("pointerleave", onDockLeave, true));

  // W-041 潮汐观察器
  tideEnsureObservers();
  tideApply();

  // W-042 详情卡观察器
  detailEnsureObserver();
  renderDetailCard();

  // W-045 模块槽
  ensureSlotHosts();
  renderSlots();

  // W-046 昨日区
  yesterdayEnsureObserver();

  // W-047 锋锐
  sharpApply();

  // W-049 手柄
  if (typeof window !== "undefined") {
    window.addEventListener("gamepadconnected", onPadConnect);
    bag.push(() => window.removeEventListener("gamepadconnected", onPadConnect));
    window.addEventListener("gamepaddisconnected", onPadDisconnect);
    bag.push(() => window.removeEventListener("gamepaddisconnected", onPadDisconnect));
  }

  // W-040 预设面板事件入口（napkin：命令面板/菜单经 CustomEvent 触发，零 import 依赖）
  const onPresetToggle = (): void => renderPresetCard();
  window.addEventListener("nova://dock-preset-toggle", onPresetToggle);
  bag.push(() => window.removeEventListener("nova://dock-preset-toggle", onPresetToggle));
  const onPreset = (e: Event): void => {
    const d = (e as CustomEvent).detail as { action?: string; name?: string } | undefined;
    if (d?.action === "apply" && d.name) applyStartPreset(d.name);
    if (d?.action === "save" && d.name) saveCurrentAsPreset(d.name);
    if (d?.action === "restore") restoreStartOrder();
  };
  window.addEventListener("nova://dock-preset", onPreset);
  bag.push(() => window.removeEventListener("nova://dock-preset", onPreset));

  // S0 注册表变化 → 即时生效/失效
  bag.push(subscribeNovaShim());
}

/** S0 注册表订阅（registry.ts subscribeNova；独立 shim 便于测试环境容错）。 */
function subscribeNovaShim(): () => void {
  try {
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    return subscribeNovaImpl();
  } catch {
    return () => {};
  }
}

let subscribeNovaImpl: () => () => void = () => () => {};
export function __bindRegistrySubscribe(fn: () => () => void): void {
  subscribeNovaImpl = fn;
}

export function deactivateDockNova(): void {
  if (!active) return;
  active = false;
  for (const fn of bag) {
    try {
      fn();
    } catch {
      /* 卸载容错 */
    }
  }
  bag = [];
  if (driftRaf) cancelAnimationFrame(driftRaf);
  driftRaf = 0;
  driftItems = [];
  driftInside = false;
  for (const a of arcs) a.remove();
  arcs = [];
  dismissPeek();
  dismissHistory();
  detailCard?.remove();
  detailCard = null;
  presetCard?.remove();
  presetCard = null;
  unmountSlots();
  padRestore();
  const bar = taskbarEl();
  bar?.classList.remove("nova-sharp", "nova-tide");
  const center = centerEl();
  center?.style.removeProperty("--nova-tide-gap");
  center?.style.removeProperty("--nova-tide-scale");
  document.querySelectorAll(".start-menu.nova-pad").forEach((m) => m.classList.remove("nova-pad"));
}

export function isDockNovaActive(): boolean {
  return active;
}

// 惰性绑定 S0 注册表订阅（避免测试环境无 window 时报错）
if (typeof window !== "undefined") {
  void Promise.resolve().then(async () => {
    try {
      const mod = await import("../registry");
      __bindRegistrySubscribe(() => mod.subscribeNova(onRegistryChange));
      if (active) bag.push(subscribeNovaShim());
    } catch {
      /* S0 未就绪：开关即时生效退化为下次激活生效 */
    }
  });
}
