/**
 * NOVA-200 · S7 效率工具路（AI-07）—— 域7 效率与工具中枢（W-077…W-089）。
 *
 * 边界（全景 §7）：Z-49 提醒中心管提醒、M-17 便签管速贴、Q-44 番茄管工作节奏、
 * U-18 迷你应用框架管单应用、M-69 今日简报管晨间、Z-62 统计管用量、U-17 拖放总线
 * 管拖放路由、U-44 外设快捷层管外设、N-01 时间机器管窗口状态回溯、V-46 速记管
 * 持久速记、Q-50 抽屉管收藏、V-41 内联计算管命令面板、V-31 视图记忆管单栏；
 * 本模块补**时间块规划、微任务带、用眼健康、多工具聚合、晚间复盘、输入习惯、
 * 拖放语义调用、会议编排、时刻对照、语音钉、选区运算、双栏互换、自焚备忘**。
 *
 * 纪律：
 * - 零侵入：不改写任何既有组件内部逻辑；全部为 DOM 叠层 + `nova://tools-*`
 *   自定义事件摄入 + `nova.tools.*` 本地存储（外部接线由 S17 按 wiringHint 补齐）；
 * - 前缀：类名 `nova-tools-`、事件 `nova://tools-*`、localStorage 键 `nova.tools.*`；
 * - 开关：只读消费 S0 注册表（registry.ts novaOn/novaNum），无注册表时用
 *   manifest defaultOn 回退（诚实降级，不报错）；
 * - 降级：reduce-motion / safeMode / static 三态下动效归零（焚毁粒子/划线消散/
 *   呼吸），语义与数据保留；非 DOM 环境行为层安全 no-op；
 * - 默认档：W-086 语音图钉、W-089 氛围备忘板默认关（与 S0 注册表一致），
 *   其余默认开。
 *
 * 诚实边界：
 * - W-086 语音引擎（__novaVoice）未装时 F8 完全隐藏（不弹任何提示骚扰），
 *   识别失败如实给出重录提示；单次最长 10s；
 * - W-084 会议四项状态为真实探测（getUserMedia / wakeLock / enumerateDevices /
 *   dataset.dnd），API 不可用如实显示 UNKNOWN，不伪造就绪；
 * - W-085 时间梭笔回看仅限本模块 72h 内自采样本（S17 派发 shuttle-sample 喂入），
 *   无样本时如实显示空态，不编造「昨日」；
 * - W-083 未映射文件类型轻晃拒收；8 类语义映射内置与注册表口径一致；
 * - W-089 备忘板纯内存态，焚毁即彻底消失（零落盘零恢复）。
 */

import { novaMotionOK, novaNum, novaOn } from "../registry";
import type { NovaParamDef } from "../registry";
import { vwmStore } from "../../windows/vwm";

// ---------------------------------------------------------------------------
// 通用工具（本模块自持）
// ---------------------------------------------------------------------------

export function clamp(v: number, min: number, max: number): number {
  return Math.min(Math.max(v, min), max);
}

/** reduce-motion / safeMode / static 三态统一判定。 */
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

const NS = "nova.tools";
const HOUR = 3_600_000;
const MIN = 60_000;

/** 功能开关：只读消费 S0 注册表（nova.registry.v1 单一事实源）。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

function num(id: string, key: string): number {
  return novaNum(id, key);
}

/** 派发 `nova://tools-*` 事件（SSR/测试环境安全）。 */
export function toolsEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://tools-${name}`, { detail }));
}

/** 本地时区日键（YYYY-MM-DD）。 */
export function dayKeyOf(t: number): string {
  const d = new Date(t);
  const m = `${d.getMonth() + 1}`.padStart(2, "0");
  const day = `${d.getDate()}`.padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

/** 分钟数 → HH:MM（分钟越界取模回绕）。 */
export function fmtHM(min: number): string {
  const m = ((Math.round(min) % 1440) + 1440) % 1440;
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
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
  /** 独立 overlay 工具窗（ai04:open-feature 的 feature id）。 */
  overlay?: string;
  /** Hub 参数卡（对齐 registry.NovaParamDef，S0 单一事实源）。 */
  params?: NovaParamDef[];
  /** 需要 S17 在既有文件补接线的行为（本模块 API/事件已备好）。 */
  wiringHint?: string;
  /** reduce-motion / safeMode / static 降级说明。 */
  degrade: string;
}

export const TOOLS_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-077",
    titleZh: "时间块雕塑家",
    titleEn: "Time Boxing",
    descZh: "今日 24h 时间轴上拖刻 15min 吸附时间块并标注事项，到点派发专注氛围事件（联动 Q-44 番茄剧场）；块可步进改期，冲突自动修剪。",
    defaultOn: true,
    overlay: "nova-timebox",
    degrade: "静态时间轴视图，无拖刻动效；块语义与到点事件保留",
  },
  {
    id: "W-078",
    titleZh: "微清单",
    titleEn: "Micro Checklist",
    descZh: "任务栏常驻至多 3 条 12px 微字要务，完成即划线消散；第 4 条加入时最旧一条自动归档（派发提醒事件）；任务栏满载折叠为计数徽标。",
    defaultOn: true,
    wiringHint: "任务栏空白区挂载点由 S17 提供 .taskbar 容器（已自动探测）",
    degrade: "无划线消散动画，完成直接移除；徽标计数语义保留",
  },
  {
    id: "W-079",
    titleZh: "屏幕时辰簿",
    titleEn: "Eye Book",
    descZh: "每连续 45min（键鼠活跃判定，无摄像头）提醒一次 20-20-20 眨眼法则（20s 微图标）；每 4 轮盖一章；≥5min 休憩自动重新计程。",
    defaultOn: true,
    params: [{ key: "everyMin", labelKey: "novaP_everyMin", type: "slider", default: 45, min: 20, max: 90, step: 5 }],
    degrade: "勿扰下微图标缓变替代闪烁；计时语义保留",
  },
  {
    id: "W-080",
    titleZh: "多工具横桌",
    titleEn: "Tool Table",
    descZh: "Ctrl+Alt+T 呼出计算器/换算/便签/取色 2×2 横桌，四象限独立状态互不干扰，可互换工具位；Tab 循环聚焦；Esc 收起并保留内部状态 10 分钟。",
    defaultOn: true,
    overlay: "nova-table",
    degrade: "无展开动效；状态保留与互换语义保留",
  },
  {
    id: "W-081",
    titleZh: "今日收官仪式",
    titleEn: "Daily Debrief",
    descZh: "每日 21:30（可调）一张收官卡：今日完成数 / 关闭窗口 / 打字里程 + 未竟事项 3 条提名明日 + 一键转为明日时间块——一日首尾相扣。",
    defaultOn: true,
    params: [
      { key: "hour", labelKey: "novaP_hour", type: "slider", default: 21, min: 19, max: 23, step: 1 },
      { key: "minute", labelKey: "novaP_minute", type: "slider", default: 30, min: 0, max: 59, step: 15 },
    ],
    wiringHint: "窗口关闭派发 nova://tools-win-close 喂入关闭计数（已备好）",
    degrade: "静态收官卡；数据同源本地账本",
  },
  {
    id: "W-082",
    titleZh: "习惯配比镜",
    titleEn: "Habit Ratio",
    descZh: "近 7 天鼠键操作占比环 + 与理想配比的偏差色带 + 可解释建议清单（如「窗口切换 80% 走鼠标，试试 Alt+Tab」）；建议规则可关。",
    defaultOn: true,
    overlay: "nova-habit",
    wiringHint: "窗口切换派发 nova://tools-switch {via} 喂入切换计数（已备好）",
    degrade: "静态占比环与清单；偏差色带保留（非色相单通道）",
  },
  {
    id: "W-083",
    titleZh: "跨工具超拖",
    titleEn: "CrossDrop",
    descZh: "文件拖到工具图标按类型语义调用：图片→取色、音频→波形、文本→字数统计等 8 类内置映射；拖到计算器=表达式求值；未映射类型轻晃拒收。",
    defaultOn: true,
    wiringHint: "工具图标接受 drop 后派发 nova://tools-crossdrop {name} 即出映射（已备好）",
    degrade: "无轻晃动画，拒收以静态提示呈现",
  },
  {
    id: "W-084",
    titleZh: "会议快车道",
    titleEn: "Meeting Lane",
    descZh: "进入会议态：麦克风测试 + 勿扰 + 屏幕保持唤醒 + 摄像头检测四项状态灯一屏确认；四项全部真实探测，API 不可用如实 UNKNOWN；退出自动汇报时长。",
    defaultOn: true,
    wiringHint: "蓝牙耳机连接/日历临会派发 nova://tools-meeting {phase} 触发（已备好）",
    degrade: "静态状态灯；探测失败不伪造就绪",
  },
  {
    id: "W-085",
    titleZh: "时间梭笔",
    titleEn: "Timeline Shuttle",
    descZh: "「我昨天这时候在干嘛」：以当前时刻为锚回看昨日前后 1h 的应用活跃对照，逐时拨动（梭）；样本仅本模块 72h 滚动缓存，无样本如实空态。",
    defaultOn: true,
    overlay: "nova-shuttle",
    wiringHint: "每小时派发 nova://tools-shuttle-sample {app} 喂入缓存（已备好，模块自采兜底）",
    degrade: "静态对照列表；缓存滚动清除语义保留",
  },
  {
    id: "W-086",
    titleZh: "语音图钉",
    titleEn: "Voice Pin",
    descZh: "长按 F8 说话（≤10s）松开即本地转文字并钉进任务栏微清单；离线引擎未安装时功能整体隐藏，识别失败如实给重录提示；opt-in 默认关。",
    defaultOn: false,
    wiringHint: "本地语音引擎按 __novaVoice.{start,stop} 契约注册后自动启用",
    degrade: "无引擎时整项隐藏（诚实）；识别失败静态提示重录",
  },
  {
    id: "W-087",
    titleZh: "算术选中",
    titleEn: "Snippet Math",
    descZh: "选中数字文本按 Ctrl+Shift+= 就地弹出运算结果，可续输运算符后回写选区；支持 +−×÷ % ^ 与括号；非数字选区轻抖拒算；历史 5 步可撤。",
    defaultOn: true,
    degrade: "无轻抖动画，拒算以静态提示呈现；浏览器原生 Ctrl+Z 撤销保留",
  },
  {
    id: "W-088",
    titleZh: "双栏交换座",
    titleEn: "Swap Panes",
    descZh: "双栏文件管理器左右两栏路径 + 滚动位置 + 选中态一键互换；200ms 交换动画可关（novtools 派发 nova://tools-swap 即执行）。",
    defaultOn: true,
    wiringHint: "双栏容器派发 nova://tools-panes {a, b} 注册栏状态（已备好）",
    degrade: "无交换动画，直接对调；互换语义保留",
  },
  {
    id: "W-089",
    titleZh: "氛围备忘板",
    titleEn: "Ambient Scratch",
    descZh: "F9 呼出全屏 12% 透明备忘板：15 分钟后自动焚毁（淡出+碎裂粒子），最后 60s 微提示；可延寿一次 +5min；纯内存不落盘。opt-in 默认关。",
    defaultOn: false,
    overlay: "nova-scratch",
    degrade: "reduce-motion 下焚毁为直接淡出（无碎裂粒子）；零落盘不变",
  },
];

export const toolsNovaDomain = {
  id: "S7",
  nameZh: "效率工具",
  nameEn: "Tools & Flow",
  route: "AI-07",
  features: TOOLS_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-077 时间块雕塑家
// ---------------------------------------------------------------------------

/** 15min 网格（验收：拖刻吸附 15min）。 */
export const GRID_MIN = 15;
/** 一天总分钟。 */
export const DAY_MIN = 1440;

/** 分钟向下吸附 15min 网格。 */
export function snapDown(min: number): number {
  return Math.max(0, Math.floor(min / GRID_MIN) * GRID_MIN);
}

/** 分钟向上吸附 15min 网格（封顶一天）。 */
export function snapUp(min: number): number {
  return Math.min(DAY_MIN, Math.ceil(min / GRID_MIN) * GRID_MIN);
}

export interface TimeBlock {
  /** 起始分钟（0..1440，15 网格对齐）。 */
  start: number;
  /** 结束分钟（> start）。 */
  end: number;
  label: string;
}

/** 两块时间上是否冲突（半开区间）。 */
export function blocksConflict(a: TimeBlock, b: TimeBlock): boolean {
  return a.start < b.end && b.start < a.end;
}

/**
 * 插入时间块（新块获胜）：修剪所有与新块重叠的既有块（切掉重叠段），
 * 整块被覆盖则移除；结果按 start 排序。确定性、可幂等重放。
 */
export function insertBlock(blocks: TimeBlock[], nb: TimeBlock): TimeBlock[] {
  const a = snapDown(nb.start);
  const b = Math.max(snapDown(nb.start) + GRID_MIN, Math.min(snapUp(nb.end), DAY_MIN));
  const nbFixed: TimeBlock = { start: a, end: b, label: nb.label };
  const out: TimeBlock[] = [];
  for (const cur of blocks) {
    if (!blocksConflict(cur, nbFixed)) {
      out.push(cur);
      continue;
    }
    if (cur.start < nbFixed.start) out.push({ ...cur, end: nbFixed.start });
    if (cur.end > nbFixed.end) out.push({ ...cur, start: nbFixed.end });
  }
  out.push(nbFixed);
  return out.sort((x, y) => x.start - y.start || x.end - y.end);
}

/** 移除与给定起点重叠的块（面板删除用）。 */
export function removeBlock(blocks: TimeBlock[], start: number): TimeBlock[] {
  return blocks.filter((b) => b.start !== start);
}

/** 块整体平移 deltaMin（吸附 15min 网格，钳制在一天内；改期语义）。 */
export function shiftBlock(b: TimeBlock, deltaMin: number): TimeBlock {
  // 负向改期也按 15min 网格吸附（snapDown 仅用于时刻，非位移）。
  const d = Math.floor(deltaMin / GRID_MIN) * GRID_MIN;
  const span = b.end - b.start;
  const start = clamp(snapDown(b.start + d), 0, DAY_MIN - span);
  return { ...b, start, end: start + span };
}

/** 当前分钟命中的块（到点联动用）。 */
export function blockAt(blocks: TimeBlock[], nowMin: number): TimeBlock | null {
  return blocks.find((b) => nowMin >= b.start && nowMin < b.end) ?? null;
}

export interface NextBlockInfo {
  block: TimeBlock;
  /** active = 正在进行；soon = 尚未开始。 */
  state: "active" | "soon";
  /** 距开始分钟（active 为 0）。 */
  inMin: number;
}

/** 下一个需要联动的块：正在进行的优先，其次最近一个将开始的。 */
export function nextBlockStart(blocks: TimeBlock[], nowMin: number): NextBlockInfo | null {
  const active = blockAt(blocks, nowMin);
  if (active) return { block: active, state: "active", inMin: 0 };
  const soon = blocks.filter((b) => b.start > nowMin).sort((a, b) => a.start - b.start)[0];
  return soon ? { block: soon, state: "soon", inMin: soon.start - nowMin } : null;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-078 微清单
// ---------------------------------------------------------------------------

/** 微清单上限（验收：至多 3 条）。 */
export const MICRO_MAX = 3;

export interface MicroItem {
  id: string;
  text: string;
  done: boolean;
  at: number;
}

/**
 * 加入一条（FIFO）：超过 3 条时最旧一条自动归档（返回 archived 供派发提醒）。
 * 空文本不入列。
 */
export function microPush(
  list: MicroItem[],
  text: string,
  id: string,
  now: number,
): { list: MicroItem[]; archived: MicroItem | null } {
  const t = text.trim();
  if (!t) return { list, archived: null };
  const next = [...list, { id, text: t, done: false, at: now }];
  if (next.length <= MICRO_MAX) return { list: next, archived: null };
  return { list: next.slice(next.length - MICRO_MAX), archived: next[0]! };
}

/** 勾选切换（完成即划线，随后由行为层消散移除）。 */
export function microToggle(list: MicroItem[], id: string): MicroItem[] {
  return list.map((it) => (it.id === id ? { ...it, done: !it.done } : it));
}

/** 移除（消散完成态 / 手动清除）。 */
export function microRemove(list: MicroItem[], id: string): MicroItem[] {
  return list.filter((it) => it.id !== id);
}

/** 未完成计数（折叠徽标用）。 */
export function undoneCount(list: MicroItem[]): number {
  return list.filter((it) => !it.done).length;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-079 屏幕时辰簿
// ---------------------------------------------------------------------------

/** 连续休憩 ≥ 5min 重置计程。 */
export const EYE_RESET_MS = 5 * MIN;
/** 每 4 轮盖一章。 */
export const ROUNDS_PER_STAMP = 4;
/** 眨眼提醒驻留 20s（20-20-20 法则）。 */
export const BLINK_ICON_MS = 20_000;

export interface EyeState {
  /** 本轮累计活跃 ms。 */
  accMs: number;
  /** 上次活跃时刻（0 = 尚未活跃）。 */
  lastActiveAt: number;
  /** 已完成轮数。 */
  rounds: number;
}

export function eyeInit(): EyeState {
  return { accMs: 0, lastActiveAt: 0, rounds: 0 };
}

/**
 * 心跳推进（每秒一 tick）：活跃则累计（先判定 5min 休憩重置），
 * 达 everyMin 分钟 → 一轮完成（rounds+1、accMs 清零、blink=true）。
 */
export function eyeTick(
  st: EyeState,
  active: boolean,
  now: number,
  everyMin = 45,
  tickMs = 1000,
): { state: EyeState; blink: boolean } {
  let s = st;
  if (active) {
    if (s.lastActiveAt > 0 && now - s.lastActiveAt >= EYE_RESET_MS) s = { ...s, accMs: 0 };
    const accMs = s.accMs + tickMs;
    if (accMs >= everyMin * MIN) {
      return { state: { accMs: 0, lastActiveAt: now, rounds: s.rounds + 1 }, blink: true };
    }
    return { state: { ...s, accMs, lastActiveAt: now }, blink: false };
  }
  return { state: s, blink: false };
}

/** 时辰簿盖章视图：本轮进度点（●/○ 共 4 槽）+ 已盖章数。 */
export function stampDots(rounds: number): { dots: string; chapters: number } {
  const filled = ((rounds % ROUNDS_PER_STAMP) + ROUNDS_PER_STAMP) % ROUNDS_PER_STAMP;
  return {
    dots: "●".repeat(filled) + "○".repeat(ROUNDS_PER_STAMP - filled),
    chapters: Math.floor(rounds / ROUNDS_PER_STAMP),
  };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-080 多工具横桌
// ---------------------------------------------------------------------------

/** 四象限内置工具（可互换）。 */
export const QUADRANT_TOOLS = ["calc", "convert", "note", "picker"] as const;
export type QuadrantTool = (typeof QUADRANT_TOOLS)[number];
/** Esc 收起后状态保留时长。 */
export const TABLE_TTL_MS = 10 * MIN;

/** Tab 循环聚焦下一象限。 */
export function nextQuadrant(i: number): number {
  return ((i + 1) % 4 + 4) % 4;
}

/**
 * 象限 i 换成 tool：若 tool 已在其它象限则两象限互换（无重复语义）。
 */
export function replaceQuadrant(tools: readonly QuadrantTool[], i: number, tool: QuadrantTool): QuadrantTool[] {
  const next = [...tools];
  const j = next.indexOf(tool);
  if (j === -1 || j === i) {
    next[i] = tool;
    return next;
  }
  next[j] = next[i]!;
  next[i] = tool;
  return next;
}

/** 收起状态是否已超时（超时 → 重置内部状态）。 */
export function tableExpired(hiddenAt: number, now: number): boolean {
  return hiddenAt > 0 && now - hiddenAt > TABLE_TTL_MS;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-081 今日收官仪式
// ---------------------------------------------------------------------------

/** 当日分钟数是否已到点且今日未收官。 */
export function debriefDue(nowMin: number, hour: number, minute: number, lastDateKey: string | null, todayKey: string): boolean {
  return nowMin >= hour * 60 + minute && lastDateKey !== todayKey;
}

/** 未竟事项提名明日：至多 n 条（验收：3 条）。 */
export function carryTopN(undone: string[], n = 3): string[] {
  return undone.map((t) => t.trim()).filter(Boolean).slice(0, n);
}

/** 提名 → 明日时间块（30min/块，从 startMin 起连续排布，复用 W-077 语义）。 */
export function toTimeBlocks(titles: string[], startMin = 540, spanMin = 30): TimeBlock[] {
  const out: TimeBlock[] = [];
  let cursor = startMin;
  for (const t of titles) {
    const label = t.trim();
    if (!label) continue;
    out.push({ start: cursor, end: Math.min(cursor + spanMin, DAY_MIN), label });
    cursor += spanMin;
    if (cursor >= DAY_MIN) break;
  }
  return out;
}

export interface DayLedger {
  done: number;
  closed: number;
  typed: number;
}

/** 收官卡摘要行（数据同源本地账本，无编造）。 */
export function debriefSummary(ledger: DayLedger): string[] {
  return [`已完成 ${ledger.done} 项`, `关闭窗口 ${ledger.closed} 个`, `打字 ${ledger.typed} 字`];
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-082 习惯配比镜
// ---------------------------------------------------------------------------

/** 理想鼠标占比默认值（用户可自设）。 */
export const HABIT_IDEAL_DEF = 40;
/** 偏差容差（± 百分点内视为 ok）。 */
export const HABIT_TOL = 15;

/** 鼠标占比（keys 为 0 时按 100% 鼠标，双零 → 0/0 如实）。 */
export function ratioPct(mouse: number, keys: number): number | null {
  const m = Math.max(0, mouse);
  const k = Math.max(0, keys);
  const total = m + k;
  if (total <= 0) return null;
  return Math.round((m / total) * 1000) / 10;
}

/** 偏差色带：低于理想带宽 low / 带内 ok / 高于带宽 high。 */
export function deviationBand(pct: number | null, ideal = HABIT_IDEAL_DEF, tol = HABIT_TOL): "low" | "ok" | "high" | "none" {
  if (pct == null) return "none";
  if (pct < ideal - tol) return "low";
  if (pct > ideal + tol) return "high";
  return "ok";
}

export interface HabitCounts {
  clicks: number;
  wheel: number;
  keys: number;
  mouseSwitch: number;
  keySwitch: number;
}

export interface HabitAdvice {
  id: string;
  text: string;
}

/**
 * 可解释建议规则（确定性阈值，规则可解释且可关）：
 * 样本不足时如实不给建议（不编造）。
 */
export function adviceRules(c: HabitCounts): HabitAdvice[] {
  const out: HabitAdvice[] = [];
  const swTotal = c.mouseSwitch + c.keySwitch;
  if (c.clicks + c.keys < 100) return out;
  if (swTotal >= 20 && c.mouseSwitch / swTotal > 0.8) {
    out.push({ id: "switch", text: `窗口切换 ${Math.round((c.mouseSwitch / swTotal) * 100)}% 走鼠标，试试 Alt+Tab` });
  }
  if (c.keys > 0 && c.clicks / (c.clicks + c.keys) > 0.6) {
    out.push({ id: "click-heavy", text: "点击占比偏高：常用动作可试快捷键或命令面板" });
  }
  if (c.wheel > 500 && c.wheel > c.keys * 0.5) {
    out.push({ id: "wheel", text: "滚轮负载高：长列表试试搜索定位或跳转锚点" });
  }
  if (c.keys > 0 && c.clicks / (c.clicks + c.keys) < 0.15) {
    out.push({ id: "key-heavy", text: "几乎全键盘操作：注意手腕休息，配一组鼠标手势更省力" });
  }
  return out;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-083 跨工具超拖
// ---------------------------------------------------------------------------

/** 8 类内置语义映射（category → tool）。 */
export const CROSS_MAP: Readonly<Record<string, string>> = {
  image: "picker",
  audio: "wave",
  text: "wordcount",
  sheet: "tablesum",
  video: "duration",
  archive: "inspect",
  code: "loc",
  pdf: "wordcount",
};

const EXT_MAP: Readonly<Record<string, string>> = {
  png: "image", jpg: "image", jpeg: "image", gif: "image", webp: "image", bmp: "image", svg: "image",
  mp3: "audio", wav: "audio", ogg: "audio", flac: "audio", m4a: "audio",
  txt: "text", md: "text", log: "text", rtf: "text",
  csv: "sheet", tsv: "sheet", xlsx: "sheet",
  mp4: "video", mkv: "video", avi: "video", mov: "video", webm: "video",
  zip: "archive", "7z": "archive", rar: "archive", gz: "archive", tar: "archive",
  ts: "code", tsx: "code", js: "code", jsx: "code", py: "code", rs: "code", c: "code", cpp: "code", h: "code", java: "code", go: "code",
  pdf: "pdf",
};

export interface CrossHit {
  name: string;
  ext: string;
  category: string;
  tool: string;
}

/** 文件名 → 语义工具（未映射 → null，调用方轻晃拒收）。 */
export function toolForFile(name: string): CrossHit | null {
  const base = name.split(/[\\/]/).pop() ?? name;
  const dot = base.lastIndexOf(".");
  if (dot < 0 || dot === base.length - 1) return null;
  const ext = base.slice(dot + 1).toLowerCase();
  const category = EXT_MAP[ext];
  const tool = category ? CROSS_MAP[category] : undefined;
  if (!category || !tool) return null;
  return { name: base, ext, category, tool };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-084 会议快车道
// ---------------------------------------------------------------------------

/** 会议将临提前量（分钟）。 */
export const MEETING_LEAD_MIN = 5;

export type LightOk = boolean | null;

export interface MeetingChecks {
  mic: LightOk;
  dnd: LightOk;
  awake: LightOk;
  cam: LightOk;
}

export interface MeetingLight {
  id: "mic" | "dnd" | "awake" | "cam";
  label: string;
  ok: LightOk;
}

/** 四项状态灯（null = API 不可用，如实 UNKNOWN）。 */
export function meetingLights(checks: MeetingChecks): MeetingLight[] {
  return [
    { id: "mic", label: "麦克风", ok: checks.mic },
    { id: "dnd", label: "勿扰", ok: checks.dnd },
    { id: "awake", label: "屏幕常亮", ok: checks.awake },
    { id: "cam", label: "摄像头", ok: checks.cam },
  ];
}

/** 四灯全绿才算就绪（UNKNOWN 不算）。 */
export function meetingReady(lights: MeetingLight[]): boolean {
  return lights.every((l) => l.ok === true);
}

/** 退出汇报（分钟取整，至少 1 分钟）。 */
export function meetingReport(heldMs: number): string {
  const m = Math.max(1, Math.round(heldMs / MIN));
  return `会议态 ${m} 分钟 · 已退出`;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-085 时间梭笔
// ---------------------------------------------------------------------------

/** 缓存滚动窗口 72h（验收：自动滚动清除）。 */
export const SHUTTLE_TTL_MS = 72 * HOUR;
/** 对照臂长（昨日同时刻 ±1h）。 */
export const SHUTTLE_ARM_H = 1;

export interface ShuttleSample {
  ts: number;
  app: string;
}

/** 小时键（YYYY-MM-DD-HH，本地时区）。 */
export function dayHourKey(t: number): string {
  return `${dayKeyOf(t)}-${String(new Date(t).getHours()).padStart(2, "0")}`;
}

/** 72h 滚动淘汰（按 ts）。 */
export function evictShuttle(map: Record<string, ShuttleSample>, now: number): Record<string, ShuttleSample> {
  const out: Record<string, ShuttleSample> = {};
  for (const [k, v] of Object.entries(map)) {
    if (now - v.ts <= SHUTTLE_TTL_MS) out[k] = v;
  }
  return out;
}

/** 昨日同一时刻 ±arm 小时的键列表（由近及远）。 */
export function shuttleAnchorKeys(now: number, armH = SHUTTLE_ARM_H): string[] {
  const keys: string[] = [];
  for (let h = armH; h >= -armH; h--) {
    keys.push(dayHourKey(now - 24 * HOUR + h * HOUR));
  }
  return keys;
}

/** 从缓存提取对照样本（缺小时如实缺位）。 */
export function shuttleLookup(
  map: Record<string, ShuttleSample>,
  now: number,
  armH = SHUTTLE_ARM_H,
): Array<{ key: string; sample: ShuttleSample | null }> {
  return shuttleAnchorKeys(now, armH).map((key) => ({ key, sample: map[key] ?? null }));
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-086 语音图钉
// ---------------------------------------------------------------------------

/** 单次最长录音（验收：10s）。 */
export const VOICE_MAX_MS = 10_000;
/** 最短有效录音（防误触）。 */
export const VOICE_MIN_MS = 400;
/** 图钉文本上限。 */
export const PIN_MAX_LEN = 120;

export interface VoiceVerdict {
  ok: boolean;
  reason: "offline" | "too-short" | "too-long" | "ok";
}

/** 松开判定：引擎未装 offline（调用方整项隐藏，不出声）；过短/超长如实。 */
export function voiceVerdict(heldMs: number, engineReady: boolean): VoiceVerdict {
  if (!engineReady) return { ok: false, reason: "offline" };
  if (heldMs < VOICE_MIN_MS) return { ok: false, reason: "too-short" };
  if (heldMs > VOICE_MAX_MS + 500) return { ok: false, reason: "too-long" };
  return { ok: true, reason: "ok" };
}

/** 识别文本 → 图钉文本（压空白 + 截断加省略号）。 */
export function pinClampText(raw: string, max = PIN_MAX_LEN): string {
  const t = raw.replace(/\s+/g, " ").trim();
  if (t.length <= max) return t;
  return `${t.slice(0, Math.max(0, max - 1))}…`;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-087 算术选中
// ---------------------------------------------------------------------------

/** 撤销历史上限（验收：5 步）。 */
export const UNDO_MAX = 5;

const SNIPPET_ALLOWED = /^[0-9+\-*/%^().,×÷−\s]+$/;

/** 选区是否可运算：仅数字/运算符/括号/千分位逗号，且至少一个数字一个运算符。 */
export function isNumericSelection(s: string): boolean {
  const t = s.trim();
  if (!t || !SNIPPET_ALLOWED.test(t)) return false;
  if (!/\d/.test(t)) return false;
  return /[+\-*/%^×÷−]/.test(t);
}

type Tok = { t: "num"; v: number } | { t: "op"; v: string };

function tokenize(src: string): Tok[] | null {
  const s = src.replace(/,/g, "").replace(/×/g, "*").replace(/÷/g, "/").replace(/−/g, "-");
  const toks: Tok[] = [];
  let i = 0;
  while (i < s.length) {
    const ch = s[i]!;
    if (ch === " ") {
      i++;
      continue;
    }
    if (ch >= "0" && ch <= "9" || ch === ".") {
      let j = i;
      while (j < s.length && (s[j]!.match(/[0-9.]/) !== null)) j++;
      const v = Number(s.slice(i, j));
      if (!Number.isFinite(v)) return null;
      toks.push({ t: "num", v });
      i = j;
      continue;
    }
    if ("+-*/%^()".includes(ch)) {
      toks.push({ t: "op", v: ch });
      i++;
      continue;
    }
    return null;
  }
  return toks;
}

/**
 * 选区表达式求值（递归下降，零 eval）：
 * 支持 + - * / % ^（右结合）与括号、一元正负；除零/非法 → ok:false。
 */
export function evalSnippet(src: string): { ok: boolean; value: number } {
  const toks = tokenize(src);
  if (!toks || toks.length === 0) return { ok: false, value: NaN };
  let pos = 0;
  const peek = (): Tok | undefined => toks[pos];
  const eatOp = (...ops: string[]): string | null => {
    const t = peek();
    if (t && t.t === "op" && ops.includes(t.v)) {
      pos++;
      return t.v;
    }
    return null;
  };

  const parseExpr = (): number | null => {
    let left = parseTerm();
    if (left == null) return null;
    for (;;) {
      const op = eatOp("+", "-");
      if (!op) return left;
      const right = parseTerm();
      if (right == null) return null;
      left = op === "+" ? left + right : left - right;
    }
  };

  const parseTerm = (): number | null => {
    let left = parseUnary();
    if (left == null) return null;
    for (;;) {
      const op = eatOp("*", "/", "%");
      if (!op) return left;
      const right = parseUnary();
      if (right == null) return null;
      if (op === "*") left = left * right;
      else if (op === "/") {
        if (right === 0) return null;
        left = left / right;
      } else {
        if (right === 0) return null;
        left = left % right;
      }
    }
  };

  const parseUnary = (): number | null => {
    if (eatOp("-")) {
      const v = parseUnary();
      return v == null ? null : -v;
    }
    if (eatOp("+")) return parseUnary();
    return parsePower();
  };

  const parsePower = (): number | null => {
    const base = parseAtom();
    if (base == null) return null;
    if (eatOp("^")) {
      const exp = parseUnary(); // 右结合
      if (exp == null) return null;
      const r = base ** exp;
      return Number.isFinite(r) ? r : null;
    }
    return base;
  };

  const parseAtom = (): number | null => {
    const t = peek();
    if (!t) return null;
    if (t.t === "num") {
      pos++;
      return t.v;
    }
    if (t.v === "(") {
      pos++;
      const v = parseExpr();
      if (v == null || !eatOp(")")) return null;
      return v;
    }
    return null;
  };

  const value = parseExpr();
  if (value == null || pos !== toks.length || !Number.isFinite(value)) return { ok: false, value: NaN };
  return { ok: true, value };
}

/** 结果格式化：消浮点噪声（1e-10 精度取整 + 去尾零）。 */
export function formatNum(n: number): string {
  if (!Number.isFinite(n)) return "∞";
  const r = Math.abs(n) < 1e15 ? Math.round(n * 1e10) / 1e10 : n;
  return String(r);
}

export interface UndoEntry {
  el: HTMLElement;
  prev: string;
}

/** 撤销栈（上限 5，先进后出）。 */
export function pushUndo(stack: UndoEntry[], entry: UndoEntry): UndoEntry[] {
  const next = [...stack, entry];
  return next.slice(Math.max(0, next.length - UNDO_MAX));
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-088 双栏交换座
// ---------------------------------------------------------------------------

/** 交换动画时长（可关）。 */
export const SWAP_ANIM_MS = 200;

export interface PaneState {
  path: string;
  scroll: number;
  selected: string[];
}

/** 两栏全状态互换（深拷贝选中态，含滚动位置）。 */
export function swapPanes(a: PaneState, b: PaneState): [PaneState, PaneState] {
  return [
    { path: b.path, scroll: b.scroll, selected: [...b.selected] },
    { path: a.path, scroll: a.scroll, selected: [...a.selected] },
  ];
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-089 氛围备忘板
// ---------------------------------------------------------------------------

/** 默认寿命 15min。 */
export const SCRATCH_TTL_MS = 15 * MIN;
/** 延寿一次 +5min。 */
export const EXTEND_MS = 5 * MIN;
/** 延寿次数上限（验收：一次）。 */
export const EXTEND_MAX = 1;
/** 最后 60s 微提示窗。 */
export const HINT_LAST_MS = 60_000;

export interface ScratchState {
  remainMs: number;
  hint: boolean;
  burned: boolean;
}

/** 倒计时状态（hint = 最后 60s；burned = 寿命归零）。 */
export function scratchState(startedAt: number, ttlMs: number, extendedCount: number, now: number): ScratchState {
  const ttl = ttlMs + extendedCount * EXTEND_MS;
  const remainMs = startedAt + ttl - now;
  return { remainMs, hint: remainMs > 0 && remainMs < HINT_LAST_MS, burned: remainMs <= 0 };
}

/** 延寿（验收：仅一次）。 */
export function scratchExtend(count: number): { ok: boolean; count: number } {
  if (count >= EXTEND_MAX) return { ok: false, count };
  return { ok: true, count: count + 1 };
}

// ---------------------------------------------------------------------------
// 行为层（零侵入 DOM 叠层 + 事件桥）
// ---------------------------------------------------------------------------

let active = false;
type Unsub = () => void;
let bag: Unsub[] = [];

// ---- W-078 微清单状态 ----
let microList: MicroItem[] = [];
let microStrip: HTMLElement | null = null;

function microLoad(): void {
  microList = lsGet<MicroItem[]>(`${NS}.micro`, []);
  if (!Array.isArray(microList)) microList = [];
}

function microSave(): void {
  lsSet(`${NS}.micro`, microList);
}

function renderMicro(): void {
  if (!microStrip?.isConnected) return;
  microStrip.textContent = "";
  const host = document.createElement("div");
  host.className = "nova-tools-micro-row";
  if (microList.length === 0) {
    const empty = document.createElement("span");
    empty.className = "nova-tools-micro-empty";
    empty.textContent = "微清单 · 空闲";
    host.appendChild(empty);
  }
  for (const it of microList) {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.className = `nova-tools-micro-item${it.done ? " nova-done" : ""}`;
    chip.title = "点击完成 / 划线";
    chip.textContent = it.text;
    chip.addEventListener("click", () => {
      microList = microToggle(microList, it.id);
      const target = microList.find((x) => x.id === it.id);
      if (target?.done) {
        bumpLedger((l) => ({ ...l, done: l.done + 1 }));
        toolsEvent("micro-done", { id: it.id, text: it.text });
        window.setTimeout(() => {
          microList = microRemove(microList, it.id);
          microSave();
          renderMicro();
        }, motionOK() ? 700 : 0);
      }
      microSave();
      renderMicro();
    });
    host.appendChild(chip);
  }
  microStrip.appendChild(host);
}

/** 微清单入列（W-086 语音钉共用；第 4 条归档派发提醒事件）。 */
export function microAdd(text: string): void {
  const id = `m${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
  const r = microPush(microList, text, id, Date.now());
  microList = r.list;
  if (r.archived) toolsEvent("micro-archived", { item: r.archived });
  microSave();
  renderMicro();
}

// ---- 当日账本（W-081 数据源） ----

function todayLedger(): DayLedger {
  return lsGet<DayLedger>(`${NS}.ledger.${dayKeyOf(Date.now())}`, { done: 0, closed: 0, typed: 0 });
}

function bumpLedger(fn: (l: DayLedger) => DayLedger): void {
  const key = `${NS}.ledger.${dayKeyOf(Date.now())}`;
  lsSet(key, fn(todayLedger()));
}

// ---- W-079 时辰簿状态 ----
let eye = eyeInit();
let blinkChip: HTMLElement | null = null;

// ---- W-080 横桌状态 ----
let tablePanel: HTMLElement | null = null;
let tableTools: QuadrantTool[] = [...QUADRANT_TOOLS];
let tableFocus = 0;
let tableHiddenAt = 0;
const tableState: Record<string, string> = {}; // 象限内部状态（内存态）

// ---- W-082 习惯计数（近 7 天） ----
let habitCounts: HabitCounts = { clicks: 0, wheel: 0, keys: 0, mouseSwitch: 0, keySwitch: 0 };

function habitKeyAt(t: number): string {
  return `${NS}.habit.${dayKeyOf(t)}`;
}

function habitLoadToday(): void {
  habitCounts = lsGet<HabitCounts>(habitKeyAt(Date.now()), {
    clicks: 0, wheel: 0, keys: 0, mouseSwitch: 0, keySwitch: 0,
  });
}

function habitBump(fn: (c: HabitCounts) => HabitCounts): void {
  habitCounts = fn(habitCounts);
  lsSet(habitKeyAt(Date.now()), habitCounts);
}

/** 近 7 天合计。 */
export function habitSevenDays(): HabitCounts {
  const sum: HabitCounts = { clicks: 0, wheel: 0, keys: 0, mouseSwitch: 0, keySwitch: 0 };
  for (let d = 0; d < 7; d++) {
    const c = lsGet<HabitCounts>(habitKeyAt(Date.now() - d * 24 * HOUR), null as unknown as HabitCounts);
    if (!c) continue;
    sum.clicks += c.clicks ?? 0;
    sum.wheel += c.wheel ?? 0;
    sum.keys += c.keys ?? 0;
    sum.mouseSwitch += c.mouseSwitch ?? 0;
    sum.keySwitch += c.keySwitch ?? 0;
  }
  return sum;
}

// ---- W-085 时间梭笔缓存（内存态，72h 滚动） ----
let shuttleCache: Record<string, ShuttleSample> = {};
let shuttlePanel: HTMLElement | null = null;

function focusedAppName(): string {
  try {
    const s = vwmStore.getState();
    const w = s.wins.find((x) => x.id === s.focusedId);
    return w?.app ?? "desktop";
  } catch {
    return "desktop";
  }
}

function shuttleSample(): void {
  const ts = Date.now();
  const key = dayHourKey(ts);
  const prev = shuttleCache[key];
  if (prev) return; // 同小时只取首个样本（小时粒度对照）
  shuttleCache = evictShuttle({ ...shuttleCache, [key]: { ts, app: focusedAppName() } }, ts);
}

// ---- W-081 收官卡状态 ----
let debriefPanel: HTMLElement | null = null;

// ---- W-077 时间块 ----
let timeboxPanel: HTMLElement | null = null;
let timeboxSelStart: number | null = null;
let lastFiredBlockKey = "";

function blocksKey(): string {
  return `${NS}.blocks.${dayKeyOf(Date.now())}`;
}

function todayBlocks(): TimeBlock[] {
  return lsGet<TimeBlock[]>(blocksKey(), []);
}

function saveBlocks(blocks: TimeBlock[]): void {
  lsSet(blocksKey(), blocks);
}

// ---- W-089 备忘板状态 ----
let scratchOverlay: HTMLElement | null = null;
let scratchStartAt = 0;
let scratchExtended = 0;
let scratchText = "";

// ---- W-084 会议态 ----
let meetingPanel: HTMLElement | null = null;
let meetingEnterAt = 0;

// ---- W-087 撤销栈 ----
let undoStack: UndoEntry[] = [];

// ---- 输入活跃（W-079 判定 + W-082 计数） ----
let lastInputAt = 0;

// ---------------------------------------------------------------------------
// HUD / 面板工具
// ---------------------------------------------------------------------------

function hudFlash(msg: string, shake = false): void {
  const layer = document.getElementById("nova-tools-layer");
  if (!layer) return;
  document.querySelectorAll(".nova-tools-hud").forEach((n) => n.remove());
  const el = document.createElement("div");
  el.className = `nova-tools-hud${shake && motionOK() ? " nova-shake" : ""}`;
  el.setAttribute("role", "status");
  el.textContent = msg;
  layer.appendChild(el);
  window.setTimeout(() => el.remove(), 1400);
}

function openPanel(panel: HTMLElement | null, cls: string, label: string): HTMLElement {
  panel?.remove();
  const el = document.createElement("div");
  el.className = cls;
  el.setAttribute("role", "dialog");
  el.setAttribute("aria-label", label);
  document.getElementById("nova-tools-layer")?.appendChild(el);
  return el;
}

// ---------------------------------------------------------------------------
// W-077 时间块雕塑家面板
// ---------------------------------------------------------------------------

function renderTimebox(): void {
  if (!timeboxPanel?.isConnected) return;
  timeboxPanel.textContent = "";
  const h = document.createElement("h3");
  h.textContent = "TIME BOXING · 今日时间块（15min 网格）";
  const rail = document.createElement("div");
  rail.className = "nova-tools-rail";
  const blocks = todayBlocks();
  const nowMin = nowMinutes();
  for (let slot = 0; slot < DAY_MIN / GRID_MIN; slot++) {
    const seg = document.createElement("button");
    seg.type = "button";
    seg.className = "nova-tools-slot";
    const start = slot * GRID_MIN;
    const inside = blocks.find((b) => start >= b.start && start < b.end);
    seg.title = `${fmtHM(start)}–${fmtHM(start + GRID_MIN)}${inside ? ` · ${inside.label}` : ""}`;
    seg.style.top = `${(start / DAY_MIN) * 100}%`;
    seg.style.height = `${(GRID_MIN / DAY_MIN) * 100}%`;
    if (inside) seg.dataset.filled = "1";
    if (start <= nowMin && nowMin < start + GRID_MIN) seg.dataset.now = "1";
    if (timeboxSelStart === start) seg.dataset.sel = "1";
    seg.addEventListener("click", () => {
      if (timeboxSelStart == null) {
        timeboxSelStart = start;
      } else {
        const a = Math.min(timeboxSelStart, start);
        const b = Math.max(timeboxSelStart, start) + GRID_MIN;
        const label = window.prompt("时间块事项：", "专注") ?? "专注";
        if (flagOn("W-077")) {
          saveBlocks(insertBlock(todayBlocks(), { start: a, end: b, label }));
          toolsEvent("block-added", { start: a, end: b, label });
        }
        timeboxSelStart = null;
      }
      renderTimebox();
    });
    rail.appendChild(seg);
  }
  const list = document.createElement("div");
  list.className = "nova-tools-blocks";
  if (blocks.length === 0) {
    const empty = document.createElement("div");
    empty.className = "nova-tools-empty";
    empty.textContent = "今日暂无时间块 · 在上方点选起止刻度拖刻";
    list.appendChild(empty);
  }
  for (const b of blocks) {
    const row = document.createElement("div");
    row.className = "nova-tools-block-row";
    const span = document.createElement("span");
    span.className = "nova-tools-block-span";
    span.textContent = `${fmtHM(b.start)}–${fmtHM(b.end)} ${b.label}`;
    const prev = document.createElement("button");
    prev.type = "button";
    prev.className = "nova-tools-block-shift";
    prev.textContent = "‹";
    prev.title = "前移 15 分钟";
    prev.addEventListener("click", () => {
      saveBlocks(todayBlocks().map((x) => (x.start === b.start ? shiftBlock(x, -GRID_MIN) : x)));
      renderTimebox();
    });
    const next = document.createElement("button");
    next.type = "button";
    next.className = "nova-tools-block-shift";
    next.textContent = "›";
    next.title = "后移 15 分钟";
    next.addEventListener("click", () => {
      saveBlocks(todayBlocks().map((x) => (x.start === b.start ? shiftBlock(x, GRID_MIN) : x)));
      renderTimebox();
    });
    const del = document.createElement("button");
    del.type = "button";
    del.className = "nova-tools-block-del";
    del.textContent = "×";
    del.title = "移除时间块";
    del.addEventListener("click", () => {
      saveBlocks(removeBlock(todayBlocks(), b.start));
      renderTimebox();
    });
    row.append(span, prev, next, del);
    list.appendChild(row);
  }
  timeboxPanel.append(h, rail, list);
}

function toggleTimebox(force?: boolean): void {
  if (!flagOn("W-077")) return;
  const want = force ?? timeboxPanel == null;
  if (!want) {
    timeboxPanel?.remove();
    timeboxPanel = null;
    timeboxSelStart = null;
    return;
  }
  timeboxPanel = openPanel(timeboxPanel, "nova-tools-timebox", "时间块雕塑家");
  renderTimebox();
}

// ---------------------------------------------------------------------------
// W-078 微清单挂载
// ---------------------------------------------------------------------------

function mountMicro(): void {
  const host = document.querySelector<HTMLElement>(".taskbar") ?? document.body;
  microStrip?.remove();
  const strip = document.createElement("div");
  strip.className = "nova-tools-micro";
  strip.setAttribute("role", "list");
  strip.setAttribute("aria-label", "微清单");
  host.appendChild(strip);
  microStrip = strip;
  renderMicro();
}

// ---------------------------------------------------------------------------
// W-080 多工具横桌面板
// ---------------------------------------------------------------------------

const TOOL_LABELS: Record<QuadrantTool, string> = {
  calc: "计算器",
  convert: "换算",
  note: "便签",
  picker: "取色",
};

function buildQuadrant(tool: QuadrantTool, idx: number): HTMLElement {
  const cell = document.createElement("div");
  cell.className = "nova-tools-cell";
  cell.dataset.tool = tool;
  cell.dataset.focus = tableFocus === idx ? "1" : "0";
  const head = document.createElement("div");
  head.className = "nova-tools-cell-head";
  const name = document.createElement("span");
  name.textContent = TOOL_LABELS[tool];
  const sel = document.createElement("select");
  sel.className = "nova-tools-cell-swap";
  sel.setAttribute("aria-label", "替换工具");
  for (const t of QUADRANT_TOOLS) {
    const opt = document.createElement("option");
    opt.value = t;
    opt.textContent = TOOL_LABELS[t];
    if (t === tool) opt.selected = true;
    sel.appendChild(opt);
  }
  sel.addEventListener("change", () => {
    tableTools = replaceQuadrant(tableTools, idx, sel.value as QuadrantTool);
    renderTable();
  });
  head.append(name, sel);
  cell.appendChild(head);

  if (tool === "calc") {
    const input = document.createElement("input");
    input.type = "text";
    input.className = "nova-tools-calc-in";
    input.placeholder = "1+2×3";
    input.value = tableState[`calc${idx}`] ?? "";
    input.addEventListener("input", () => {
      tableState[`calc${idx}`] = input.value;
    });
    const out = document.createElement("div");
    out.className = "nova-tools-calc-out";
    const run = (): void => {
      const r = evalSnippet(input.value);
      out.textContent = r.ok ? `= ${formatNum(r.value)}` : "…";
    };
    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") run();
      e.stopPropagation();
    });
    input.addEventListener("change", run);
    cell.append(input, out);
  } else if (tool === "convert") {
    const val = document.createElement("input");
    val.type = "number";
    val.className = "nova-tools-conv-in";
    val.value = tableState[`conv${idx}`] ?? "1";
    val.addEventListener("input", () => {
      tableState[`conv${idx}`] = val.value;
    });
    const kind = document.createElement("select");
    kind.className = "nova-tools-conv-kind";
    for (const [k, label] of [["m2ft", "米→英尺"], ["ft2m", "英尺→米"], ["kg2lb", "千克→磅"], ["lb2kg", "磅→千克"], ["c2f", "℃→℉"], ["f2c", "℉→℃"]] as const) {
      const opt = document.createElement("option");
      opt.value = k;
      opt.textContent = label;
      kind.appendChild(opt);
    }
    const out = document.createElement("div");
    out.className = "nova-tools-conv-out";
    const run = (): void => {
      const v = Number(val.value);
      out.textContent = Number.isFinite(v) ? `= ${formatNum(convertUnit(v, kind.value))}` : "…";
    };
    val.addEventListener("input", run);
    kind.addEventListener("change", run);
    cell.append(val, kind, out);
    run();
  } else if (tool === "note") {
    const ta = document.createElement("textarea");
    ta.className = "nova-tools-note";
    ta.placeholder = "横桌便签 · 仅内存（收起 10min 后清空）";
    ta.value = tableState[`note${idx}`] ?? "";
    ta.addEventListener("input", () => {
      tableState[`note${idx}`] = ta.value;
    });
    ta.addEventListener("keydown", (e) => e.stopPropagation());
    cell.appendChild(ta);
  } else {
    const color = document.createElement("input");
    color.type = "color";
    color.className = "nova-tools-color";
    color.value = tableState[`color${idx}`] ?? "#7c6cff";
    const hex = document.createElement("span");
    hex.className = "nova-tools-color-hex";
    hex.textContent = color.value;
    color.addEventListener("input", () => {
      tableState[`color${idx}`] = color.value;
      hex.textContent = color.value;
      void navigator.clipboard?.writeText(color.value).catch(() => undefined);
    });
    cell.append(color, hex);
  }
  return cell;
}

function renderTable(): void {
  if (!tablePanel?.isConnected) return;
  tablePanel.textContent = "";
  const h = document.createElement("h3");
  h.textContent = "TOOL TABLE · Tab 循环聚焦 · Esc 收起保留 10 分钟";
  const grid = document.createElement("div");
  grid.className = "nova-tools-grid";
  tableTools.forEach((t, i) => grid.appendChild(buildQuadrant(t, i)));
  tablePanel.append(h, grid);
}

function toggleTable(force?: boolean): void {
  if (!flagOn("W-080")) return;
  const want = force ?? tablePanel == null;
  if (!want) {
    tablePanel?.remove();
    tablePanel = null;
    tableHiddenAt = Date.now();
    return;
  }
  if (tableExpired(tableHiddenAt, Date.now())) {
    for (const k of Object.keys(tableState)) delete tableState[k];
    tableTools = [...QUADRANT_TOOLS];
    tableFocus = 0;
  }
  tableHiddenAt = 0;
  tablePanel = openPanel(tablePanel, "nova-tools-table", "多工具横桌");
  renderTable();
}

// ---------------------------------------------------------------------------
// W-081 收官卡
// ---------------------------------------------------------------------------

function openDebrief(): void {
  if (!flagOn("W-081") || debriefPanel?.isConnected) return;
  const panel = openPanel(debriefPanel, "nova-tools-debrief", "今日收官仪式");
  debriefPanel = panel;
  const ledger = todayLedger();
  const h = document.createElement("h3");
  h.textContent = "DAILY DEBRIEF · 今日收官";
  const sum = document.createElement("div");
  sum.className = "nova-tools-debrief-sum";
  for (const line of debriefSummary(ledger)) {
    const row = document.createElement("div");
    row.textContent = line;
    sum.appendChild(row);
  }
  const undone = microList.filter((m) => !m.done).map((m) => m.text);
  const nominees = carryTopN(undone, 3);
  const nom = document.createElement("div");
  nom.className = "nova-tools-debrief-nom";
  nom.textContent = nominees.length > 0 ? `明日提名：${nominees.join(" / ")}` : "明日提名：无未竟事项";
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "nova-tools-debrief-btn";
  btn.textContent = "转为明日时间块";
  btn.disabled = nominees.length === 0;
  btn.addEventListener("click", () => {
    const blocks = toTimeBlocks(nominees);
    const key = `${NS}.blocks.${dayKeyOf(Date.now() + 24 * HOUR)}`;
    lsSet(key, blocks);
    toolsEvent("block-scheduled", { day: dayKeyOf(Date.now() + 24 * HOUR), blocks });
    btn.disabled = true;
    btn.textContent = "已排入明日";
  });
  const done = document.createElement("button");
  done.type = "button";
  done.className = "nova-tools-debrief-btn";
  done.textContent = "收官";
  done.addEventListener("click", () => {
    debriefPanel?.remove();
    debriefPanel = null;
  });
  const actions = document.createElement("div");
  actions.className = "nova-tools-debrief-actions";
  actions.append(btn, done);
  panel.append(h, sum, nom, actions);
  lsSet(`${NS}.lastDebrief`, dayKeyOf(Date.now()));
  toolsEvent("debrief", { phase: "open" });
}

// ---------------------------------------------------------------------------
// W-082 习惯配比镜面板
// ---------------------------------------------------------------------------

let habitPanel: HTMLElement | null = null;

function renderHabit(): void {
  if (!habitPanel?.isConnected) return;
  habitPanel.textContent = "";
  const h = document.createElement("h3");
  h.textContent = "HABIT RATIO · 近 7 天鼠键配比";
  const sum = habitSevenDays();
  const pct = ratioPct(sum.clicks, sum.keys);
  const band = deviationBand(pct);
  const ring = document.createElement("div");
  ring.className = "nova-tools-ring";
  ring.dataset.band = band;
  ring.style.setProperty("--nova-habit-pct", `${pct ?? 0}%`);
  const val = document.createElement("div");
  val.className = "nova-tools-ring-val";
  val.textContent = pct == null ? "无样本" : `鼠标 ${pct}%`;
  ring.appendChild(val);
  const ideal = document.createElement("div");
  ideal.className = "nova-tools-ring-ideal";
  ideal.textContent = `理想配比 ${HABIT_IDEAL_DEF}% ±${HABIT_TOL}`;
  const list = document.createElement("div");
  list.className = "nova-tools-advice";
  const adviceOn = lsGet<boolean>(`${NS}.habitAdvice`, true);
  const rules = adviceOn ? adviceRules(sum) : [];
  if (rules.length === 0) {
    const none = document.createElement("div");
    none.className = "nova-tools-empty";
    none.textContent = adviceOn ? "样本不足 · 继续使用后给出建议" : "建议已关闭";
    list.appendChild(none);
  }
  for (const r of rules) {
    const row = document.createElement("div");
    row.className = "nova-tools-advice-row";
    row.textContent = `· ${r.text}`;
    list.appendChild(row);
  }
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "nova-tools-advice-toggle";
  toggle.textContent = adviceOn ? "关闭建议" : "开启建议";
  toggle.addEventListener("click", () => {
    lsSet(`${NS}.habitAdvice`, !adviceOn);
    renderHabit();
  });
  habitPanel.append(h, ring, ideal, list, toggle);
}

function toggleHabit(force?: boolean): void {
  if (!flagOn("W-082")) return;
  const want = force ?? habitPanel == null;
  if (!want) {
    habitPanel?.remove();
    habitPanel = null;
    return;
  }
  habitPanel = openPanel(habitPanel, "nova-tools-habit", "习惯配比镜");
  renderHabit();
}

// ---------------------------------------------------------------------------
// W-085 时间梭笔面板
// ---------------------------------------------------------------------------

function toggleShuttle(force?: boolean): void {
  if (!flagOn("W-085")) return;
  const want = force ?? shuttlePanel == null;
  if (!want) {
    shuttlePanel?.remove();
    shuttlePanel = null;
    return;
  }
  const panel = openPanel(shuttlePanel, "nova-tools-shuttle", "时间梭笔");
  shuttlePanel = panel;
  panel.textContent = "";
  const h = document.createElement("h3");
  h.textContent = "TIMELINE SHUTTLE · 昨日同一时刻 ±1h";
  const list = document.createElement("div");
  list.className = "nova-tools-shuttle-list";
  const rows = shuttleLookup(shuttleCache, Date.now());
  let hits = 0;
  for (const r of rows) {
    const row = document.createElement("div");
    row.className = "nova-tools-shuttle-row";
    const hh = r.key.slice(-2);
    row.textContent = r.sample ? `${hh}:00 ${r.sample.app}` : `${hh}:00 — 无样本`;
    if (r.sample) hits++;
    list.appendChild(row);
  }
  if (hits === 0) {
    const empty = document.createElement("div");
    empty.className = "nova-tools-empty";
    empty.textContent = "72h 缓存内无对照样本 · 如实空态";
    list.appendChild(empty);
  }
  panel.append(h, list);
}

// ---------------------------------------------------------------------------
// W-084 会议快车道面板
// ---------------------------------------------------------------------------

async function probeMeeting(): Promise<MeetingChecks> {
  const out: MeetingChecks = { mic: null, dnd: null, awake: null, cam: null };
  // 勿扰（Z-44 数据集口径）
  out.dnd = document.documentElement.dataset.dnd === "true" ? true : false;
  // 摄像头存在性（不开启预览，仅枚举）
  try {
    const devs = await navigator.mediaDevices?.enumerateDevices?.();
    out.cam = (devs ?? []).some((d) => d.kind === "videoinput");
  } catch {
    out.cam = null;
  }
  // 麦克风真实可用性（3s 超时，拿到即释放）
  try {
    const md = navigator.mediaDevices;
    if (!md?.getUserMedia) {
      out.mic = null;
    } else {
      const stream = await Promise.race([
        md.getUserMedia({ audio: true }),
        new Promise<null>((res) => setTimeout(() => res(null), 3000)),
      ]);
      if (stream) {
        stream.getTracks().forEach((t) => t.stop());
        out.mic = true;
      } else out.mic = false;
    }
  } catch {
    out.mic = false;
  }
  // 屏幕常亮（Wake Lock 探测性申请）
  try {
    const nav = navigator as Navigator & { wakeLock?: { request: (t: string) => Promise<{ release: () => Promise<void> }> } };
    if (!nav.wakeLock) out.awake = null;
    else {
      const sentinel = await nav.wakeLock.request("screen");
      await sentinel.release();
      out.awake = true;
    }
  } catch {
    out.awake = false;
  }
  return out;
}

function renderMeetingLights(lights: MeetingLight[]): void {
  if (!meetingPanel?.isConnected) return;
  const box = meetingPanel.querySelector<HTMLElement>(".nova-tools-meet-lights");
  if (!box) return;
  box.textContent = "";
  for (const l of lights) {
    const row = document.createElement("div");
    row.className = "nova-tools-meet-light";
    row.dataset.ok = l.ok == null ? "unknown" : l.ok ? "1" : "0";
    row.textContent = `${l.label}：${l.ok == null ? "UNKNOWN" : l.ok ? "OK" : "未就绪"}`;
    box.appendChild(row);
  }
}

function openMeeting(): void {
  if (!flagOn("W-084")) return;
  meetingPanel = openPanel(meetingPanel, "nova-tools-meeting", "会议快车道");
  meetingPanel.textContent = "";
  const h = document.createElement("h3");
  h.textContent = "MEETING LANE · 会议态四项确认";
  const box = document.createElement("div");
  box.className = "nova-tools-meet-lights";
  const note = document.createElement("div");
  note.className = "nova-tools-empty";
  note.textContent = "探测中…";
  const exit = document.createElement("button");
  exit.type = "button";
  exit.className = "nova-tools-meet-exit";
  exit.textContent = "退出会议态";
  exit.addEventListener("click", () => endMeeting());
  meetingPanel.append(h, box, note, exit);
  meetingEnterAt = Date.now();
  void probeMeeting().then((checks) => {
    note.textContent = "";
    renderMeetingLights(meetingLights(checks));
    if (meetingReady(meetingLights(checks))) toolsEvent("meeting-ready", {});
    toolsEvent("meeting-lights", { lights: meetingLights(checks) });
  });
  toolsEvent("meeting", { phase: "enter" });
}

function endMeeting(): void {
  if (!meetingPanel) return;
  const held = Date.now() - meetingEnterAt;
  meetingPanel.remove();
  meetingPanel = null;
  hudFlash(meetingReport(held));
  toolsEvent("meeting", { phase: "exit", heldMs: held });
}

// ---------------------------------------------------------------------------
// W-089 氛围备忘板
// ---------------------------------------------------------------------------

function renderScratchChip(): void {
  if (!scratchOverlay?.isConnected) return;
  const chip = scratchOverlay.querySelector<HTMLElement>(".nova-tools-scratch-chip");
  if (!chip) return;
  const st = scratchState(scratchStartAt, SCRATCH_TTL_MS, scratchExtended, Date.now());
  chip.dataset.hint = st.hint ? "1" : "0";
  const remain = Math.max(0, Math.ceil(st.remainMs / 1000));
  chip.textContent = `BURN IN ${String(Math.floor(remain / 60)).padStart(2, "0")}:${String(remain % 60).padStart(2, "0")}`;
}

function burnScratch(): void {
  const ov = scratchOverlay;
  if (!ov) return;
  scratchOverlay = null;
  scratchText = "";
  scratchStartAt = 0;
  scratchExtended = 0;
  toolsEvent("scratch", { phase: "burned" });
  if (motionOK()) {
    ov.classList.add("nova-burn");
    window.setTimeout(() => ov.remove(), 900);
  } else {
    ov.remove();
  }
}

function tickScratch(): void {
  if (!scratchOverlay?.isConnected) return;
  const st = scratchState(scratchStartAt, SCRATCH_TTL_MS, scratchExtended, Date.now());
  renderScratchChip();
  if (st.burned) burnScratch();
}

function toggleScratch(force?: boolean): void {
  if (!flagOn("W-089")) return;
  const want = force ?? scratchOverlay == null;
  if (!want) {
    scratchOverlay?.remove();
    scratchOverlay = null;
    scratchStartAt = 0;
    scratchExtended = 0;
    toolsEvent("scratch", { phase: "closed" });
    return;
  }
  const ov = document.createElement("div");
  ov.className = "nova-tools-scratch";
  ov.setAttribute("role", "dialog");
  ov.setAttribute("aria-label", "氛围备忘板（15 分钟自焚）");
  const chip = document.createElement("div");
  chip.className = "nova-tools-scratch-chip";
  chip.setAttribute("aria-live", "polite");
  const extend = document.createElement("button");
  extend.type = "button";
  extend.className = "nova-tools-scratch-extend";
  extend.textContent = "+5min";
  extend.title = "延寿一次";
  extend.addEventListener("click", () => {
    const r = scratchExtend(scratchExtended);
    if (r.ok) {
      scratchExtended = r.count;
      renderScratchChip();
    } else hudFlash("延寿已用尽");
  });
  const ta = document.createElement("textarea");
  ta.className = "nova-tools-scratch-ta";
  ta.placeholder = "先记下来马上用 · 15 分钟后焚毁 · 不落盘";
  ta.value = scratchText;
  ta.addEventListener("input", () => {
    scratchText = ta.value; // 纯内存，绝不写 localStorage
  });
  ta.addEventListener("keydown", (e) => e.stopPropagation());
  const close = document.createElement("button");
  close.type = "button";
  close.className = "nova-tools-scratch-close";
  close.textContent = "收起";
  close.addEventListener("click", () => toggleScratch(false));
  ov.append(ta, chip, extend, close);
  document.getElementById("nova-tools-layer")?.appendChild(ov);
  scratchOverlay = ov;
  scratchStartAt = Date.now();
  scratchExtended = 0;
  renderScratchChip();
  toolsEvent("scratch", { phase: "open" });
}

// ---------------------------------------------------------------------------
// W-086 语音图钉（引擎契约：__novaVoice.{start,stop}）
// ---------------------------------------------------------------------------

interface NovaVoiceEngine {
  start: () => boolean;
  stop: () => Promise<string>;
}

function voiceEngine(): NovaVoiceEngine | null {
  const v = (window as unknown as { __novaVoice?: NovaVoiceEngine }).__novaVoice;
  return v && typeof v.start === "function" && typeof v.stop === "function" ? v : null;
}

let f8HeldAt = 0;
let voiceHud: HTMLElement | null = null;

function voiceHudShow(msg: string): void {
  voiceHud?.remove();
  const layer = document.getElementById("nova-tools-layer");
  if (!layer) return;
  const el = document.createElement("div");
  el.className = "nova-tools-voice";
  el.setAttribute("role", "status");
  el.textContent = msg;
  layer.appendChild(el);
  voiceHud = el;
}

function onF8Down(): void {
  if (!flagOn("W-086")) return;
  const eng = voiceEngine();
  if (!eng) return; // 引擎未装 → 整项隐藏（诚实）
  if (!eng.start()) {
    voiceHudShow("录音启动失败 · 重录");
    return;
  }
  f8HeldAt = Date.now();
  voiceHudShow("REC · 松开转文字（≤10s）");
  // 硬顶 10s 自动截断
  window.setTimeout(() => {
    if (f8HeldAt > 0 && Date.now() - f8HeldAt >= VOICE_MAX_MS) void onF8Up(true);
  }, VOICE_MAX_MS + 200);
}

function onF8Up(force = false): Promise<void> {
  if (f8HeldAt === 0) return Promise.resolve();
  const held = Date.now() - f8HeldAt;
  f8HeldAt = 0;
  const eng = voiceEngine();
  if (!eng) return Promise.resolve();
  const verdict = voiceVerdict(force ? VOICE_MAX_MS + 600 : held, true);
  if (!verdict.ok) {
    void eng.stop().catch(() => undefined);
    voiceHudShow(verdict.reason === "too-short" ? "太短 · 长按 F8 重录" : "超 10s · 已截断重录");
    return Promise.resolve();
  }
  return eng
    .stop()
    .then((text) => {
      const t = pinClampText(text);
      if (!t) {
        voiceHudShow("未识别 · 重录");
        toolsEvent("voice", { phase: "empty" });
        return;
      }
      microAdd(t);
      voiceHudShow(`已钉住：${t}`);
      toolsEvent("voice", { phase: "pinned", text: t });
    })
    .catch(() => {
      voiceHudShow("识别失败 · 重录");
      toolsEvent("voice", { phase: "error" });
    });
}

// ---------------------------------------------------------------------------
// W-087 算术选中行为
// ---------------------------------------------------------------------------

function snippetMath(): void {
  if (!flagOn("W-087")) return;
  const sel = document.getSelection();
  if (!sel || sel.rangeCount === 0 || sel.isCollapsed) {
    hudFlash("选中数字文本再按 Ctrl+Shift+=");
    return;
  }
  const text = sel.toString();
  const r = evalSnippet(text);
  if (!isNumericSelection(text) || !r.ok) {
    hudFlash("非数字选区 · 拒算", true);
    return;
  }
  const el = sel.anchorNode?.parentElement ?? null;
  if (el) undoStack = pushUndo(undoStack, { el, prev: text });
  // 原生 execCommand 替换选区（浏览器 Ctrl+Z 原生可撤）
  let ok = false;
  try {
    ok = document.execCommand("insertText", false, formatNum(r.value));
  } catch {
    ok = false;
  }
  hudFlash(ok ? `${text.trim()} = ${formatNum(r.value)}` : "写入失败");
  toolsEvent("math", { expr: text.trim(), value: r.ok ? r.value : null });
}

/** 撤销最近一次算术回写（API 兜底；编辑器内 Ctrl+Z 走原生）。 */
export function undoSnippetMath(): boolean {
  const top = undoStack[undoStack.length - 1];
  if (!top || !top.el.isConnected) return false;
  try {
    const sel = document.getSelection();
    const range = document.createRange();
    range.selectNodeContents(top.el);
    sel?.removeAllRanges();
    sel?.addRange(range);
    return document.execCommand("insertText", false, top.prev);
  } catch {
    return false;
  } finally {
    undoStack = undoStack.slice(0, -1);
  }
}

// ---------------------------------------------------------------------------
// W-083 跨工具超拖行为
// ---------------------------------------------------------------------------

function crossDrop(name: string): CrossHit | null {
  if (!flagOn("W-083")) return null;
  const hit = toolForFile(name);
  if (!hit) {
    hudFlash(`${name} · 未映射类型`, true);
    toolsEvent("crossdrop", { name, rejected: true });
    return null;
  }
  hudFlash(`${hit.category} → ${hit.tool}`);
  toolsEvent("crossdrop", { ...hit, name });
  return hit;
}

// ---------------------------------------------------------------------------
// W-088 双栏交换行为
// ---------------------------------------------------------------------------

let paneA: PaneState | null = null;
let paneB: PaneState | null = null;

function swapPanesAction(): void {
  if (!flagOn("W-088")) return;
  if (!paneA || !paneB) {
    hudFlash("双栏未注册 · 派发 nova://tools-panes");
    return;
  }
  const [na, nb] = swapPanes(paneA, paneB);
  paneA = na;
  paneB = nb;
  toolsEvent("swap", { a: paneA, b: paneB, animMs: motionOK() ? SWAP_ANIM_MS : 0 });
}

// ---------------------------------------------------------------------------
// 换算（W-080 换算象限的极小内置集）
// ---------------------------------------------------------------------------

/** 长度/重量/温度三对换算（横桌换算象限内置）。 */
export function convertUnit(v: number, kind: string): number {
  switch (kind) {
    case "m2ft": return v * 3.28084;
    case "ft2m": return v / 3.28084;
    case "kg2lb": return v * 2.20462;
    case "lb2kg": return v / 2.20462;
    case "c2f": return v * 9 / 5 + 32;
    case "f2c": return (v - 32) * 5 / 9;
    default: return v;
  }
}

// ---------------------------------------------------------------------------
// 心跳（1s）：时辰簿 / 收官到点 / 备忘板焚毁 / 时间块到点
// ---------------------------------------------------------------------------

function nowMinutes(): number {
  const d = new Date();
  return d.getHours() * 60 + d.getMinutes();
}

let lastDebriefMinCheck = -1;

function heartbeat(): void {
  const now = Date.now();
  // W-079 时辰簿
  if (flagOn("W-079")) {
    const everyMin = clamp(num("W-079", "everyMin") || 45, 20, 90);
    const active = now - lastInputAt < 4000 && lastInputAt > 0;
    const r = eyeTick(eye, active, now, everyMin);
    eye = r.state;
    if (r.blink) showBlink();
  }
  // W-081 收官到点（每分钟查一次）
  const d = new Date(now);
  if (d.getSeconds() === 0 && lastDebriefMinCheck !== d.getMinutes()) {
    lastDebriefMinCheck = d.getMinutes();
    const hour = clamp(num("W-081", "hour") || 21, 19, 23);
    const minute = clamp(num("W-081", "minute") || 30, 0, 59);
    const last = lsGet<string | null>(`${NS}.lastDebrief`, null);
    if (debriefDue(nowMinutes(), hour, minute, last, dayKeyOf(now))) openDebrief();
  }
  // W-089 备忘板焚毁
  tickScratch();
  // W-077 时间块到点联动
  if (flagOn("W-077")) {
    const info = nextBlockStart(todayBlocks(), nowMinutes());
    if (info && info.state === "active") {
      const key = `${dayKeyOf(now)}@${info.block.start}`;
      if (key !== lastFiredBlockKey) {
        lastFiredBlockKey = key;
        toolsEvent("block-focus", { label: info.block.label, start: info.block.start });
      }
    }
  }
  // W-085 每小时采样（自然小时首 tick）
  shuttleSample();
}

function showBlink(): void {
  const layer = document.getElementById("nova-tools-layer");
  if (!layer) return;
  blinkChip?.remove();
  const dnd = document.documentElement.dataset.dnd === "true";
  const chip = document.createElement("div");
  chip.className = `nova-tools-blink${dnd ? " nova-dnd" : ""}`;
  chip.setAttribute("role", "status");
  chip.textContent = dnd ? "20-20-20" : "眨眼 · 看 20 英尺外 20 秒";
  layer.appendChild(chip);
  blinkChip = chip;
  toolsEvent("eye", { phase: "blink", rounds: eye.rounds });
  window.setTimeout(() => {
    chip.remove();
    if (blinkChip === chip) blinkChip = null;
  }, BLINK_ICON_MS);
}

// ---------------------------------------------------------------------------
// CSS（唯一注入点，nova-tools- 前缀，全部走 tokens 语义变量）
// ---------------------------------------------------------------------------

const STYLE_ID = "nova-tools-style";
const STYLE_TEXT = `
#nova-tools-layer{position:fixed;inset:0;z-index:944;pointer-events:none;font-size:12px}
#nova-tools-layer>*{pointer-events:auto}
.nova-tools-hud{position:fixed;left:50%;bottom:56px;transform:translateX(-50%);padding:3px 10px;border-radius:7px;background:var(--panel,var(--bg-raised,#111));color:var(--ink,var(--fg,#ddd));box-shadow:0 8px 24px oklch(0 0 0 / .3);letter-spacing:.06em}
.nova-tools-hud.nova-shake{animation:nova-tools-shake .4s var(--ease-standard)}
@keyframes nova-tools-shake{0%,100%{transform:translateX(-50%)}25%{transform:translateX(calc(-50% - 5px))}75%{transform:translateX(calc(-50% + 5px))}}
[data-reduce-motion="true"] .nova-tools-hud.nova-shake{animation:none}
.nova-tools-timebox,.nova-tools-table,.nova-tools-debrief,.nova-tools-habit,.nova-tools-shuttle,.nova-tools-meeting{position:fixed;right:16px;top:56px;z-index:945;min-width:300px;max-width:400px;max-height:min(72vh,560px);overflow:auto;padding:12px 14px;border-radius:12px;background:var(--panel,var(--bg-raised,#111));box-shadow:0 12px 36px oklch(0 0 0 / .3);color:var(--ink,var(--fg,#ddd))}
.nova-tools-timebox h3,.nova-tools-table h3,.nova-tools-debrief h3,.nova-tools-habit h3,.nova-tools-shuttle h3,.nova-tools-meeting h3{margin:0 0 8px;font-size:12px;font-weight:600;letter-spacing:.04em}
.nova-tools-empty{opacity:.65;padding:6px 0}
.nova-tools-rail{position:relative;height:120px;border-radius:8px;background:var(--accent-soft);margin:6px 0;overflow:hidden}
.nova-tools-slot{position:absolute;left:0;width:100%;border:0;background:transparent;cursor:pointer}
.nova-tools-slot:hover{background:oklch(0.75 0.09 262 / .18)}
.nova-tools-slot[data-filled="1"]{background:var(--accent);opacity:.85}
.nova-tools-slot[data-now="1"]{box-shadow:inset 0 0 0 2px oklch(0.8 0.16 145 / .9)}
.nova-tools-slot[data-sel="1"]{box-shadow:inset 0 0 0 2px oklch(0.8 0.16 55 / .9)}
.nova-tools-block-row{display:flex;align-items:center;gap:6px;padding:2px 0}
.nova-tools-block-span{flex:1;font-variant-numeric:tabular-nums}
.nova-tools-block-shift,.nova-tools-block-del{font:inherit;width:22px;height:22px;border-radius:6px;border:1px solid var(--accent-soft);background:transparent;color:inherit;cursor:pointer}
.nova-tools-block-del:hover{background:oklch(0.65 0.2 25 / .25)}
.nova-tools-micro{display:flex;align-items:center;min-width:0;max-width:340px;pointer-events:auto}
.nova-tools-micro-row{display:flex;gap:6px;align-items:center;min-width:0}
.nova-tools-micro-item{font:inherit;font-size:12px;border:0;background:transparent;color:var(--ink,var(--fg,#ddd));padding:1px 6px;border-radius:6px;cursor:pointer;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:110px}
.nova-tools-micro-item:hover{background:var(--accent-soft)}
.nova-tools-micro-item.nova-done{text-decoration:line-through;opacity:0;transition:opacity .7s var(--ease-standard)}
[data-reduce-motion="true"] .nova-tools-micro-item.nova-done{transition:none;opacity:.45}
.nova-tools-micro-empty{font-size:12px;opacity:.4;white-space:nowrap}
.nova-tools-blink{position:fixed;right:16px;bottom:56px;padding:4px 12px;border-radius:999px;background:var(--accent-soft);color:var(--ink,var(--fg,#ddd));animation:nova-tools-blink 2s var(--ease-standard) infinite}
@keyframes nova-tools-blink{0%,100%{opacity:.55}50%{opacity:1}}
.nova-tools-blink.nova-dnd{animation:none;opacity:.6}
[data-reduce-motion="true"] .nova-tools-blink{animation:none;opacity:.75}
.nova-tools-grid{display:grid;grid-template-columns:1fr 1fr;gap:8px}
.nova-tools-cell{display:flex;flex-direction:column;gap:6px;padding:8px;border-radius:10px;background:var(--accent-soft);min-height:110px}
.nova-tools-cell[data-focus="1"]{box-shadow:0 0 0 2px var(--accent)}
.nova-tools-cell-head{display:flex;justify-content:space-between;align-items:center;gap:6px;font-weight:600}
.nova-tools-cell-swap{font:inherit;font-size:11px;border:1px solid var(--accent-soft);background:transparent;color:inherit;border-radius:6px}
.nova-tools-calc-in,.nova-tools-conv-in,.nova-tools-note{font:inherit;width:100%;padding:3px 6px;border-radius:6px;border:1px solid var(--accent-soft);background:var(--panel,var(--bg-raised,#111));color:inherit;box-sizing:border-box}
.nova-tools-note{flex:1;resize:none;min-height:70px}
.nova-tools-calc-out,.nova-tools-conv-out{font-variant-numeric:tabular-nums;opacity:.85}
.nova-tools-conv-kind{font:inherit;font-size:11px;border:1px solid var(--accent-soft);background:transparent;color:inherit;border-radius:6px}
.nova-tools-color{width:100%;height:28px;border:0;background:transparent;cursor:pointer}
.nova-tools-color-hex{font-family:var(--mono,Consolas,monospace);opacity:.8}
.nova-tools-debrief-sum{display:flex;flex-direction:column;gap:2px;padding:6px 0;border-bottom:1px solid var(--accent-soft)}
.nova-tools-debrief-nom{padding:6px 0;opacity:.9}
.nova-tools-debrief-actions{display:flex;gap:8px}
.nova-tools-debrief-btn{font:inherit;padding:3px 10px;border-radius:6px;border:1px solid var(--accent-soft);background:transparent;color:inherit;cursor:pointer}
.nova-tools-debrief-btn:hover{background:var(--accent-soft)}
.nova-tools-debrief-btn:disabled{opacity:.4;cursor:default}
.nova-tools-ring{width:96px;height:96px;border-radius:50%;margin:6px auto;background:conic-gradient(var(--accent) var(--nova-habit-pct,0%),oklch(0.75 0.05 262 / .25) 0);position:relative}
.nova-tools-ring[data-band="low"]{background:conic-gradient(oklch(0.75 0.12 230) var(--nova-habit-pct,0%),oklch(0.75 0.05 262 / .25) 0)}
.nova-tools-ring[data-band="high"]{background:conic-gradient(oklch(0.75 0.16 55) var(--nova-habit-pct,0%),oklch(0.75 0.05 262 / .25) 0)}
.nova-tools-ring-val{position:absolute;inset:14px;border-radius:50%;background:var(--panel,var(--bg-raised,#111));display:flex;align-items:center;justify-content:center;font-variant-numeric:tabular-nums}
.nova-tools-ring-ideal{text-align:center;opacity:.6;margin:4px 0}
.nova-tools-advice{display:flex;flex-direction:column;gap:2px;padding:4px 0}
.nova-tools-advice-toggle{font:inherit;margin-top:6px;padding:2px 10px;border-radius:6px;border:1px solid var(--accent-soft);background:transparent;color:inherit;cursor:pointer}
.nova-tools-shuttle-row{font-variant-numeric:tabular-nums;padding:1px 0;opacity:.9}
.nova-tools-meet-lights{display:flex;flex-direction:column;gap:4px;padding:4px 0}
.nova-tools-meet-light{padding:3px 8px;border-radius:7px;background:var(--accent-soft)}
.nova-tools-meet-light[data-ok="1"]{background:oklch(0.72 0.14 145 / .3)}
.nova-tools-meet-light[data-ok="0"]{background:oklch(0.65 0.19 25 / .3)}
.nova-tools-meet-light[data-ok="unknown"]{opacity:.6}
.nova-tools-meet-exit{font:inherit;margin-top:8px;padding:3px 10px;border-radius:6px;border:1px solid var(--accent-soft);background:transparent;color:inherit;cursor:pointer}
.nova-tools-voice{position:fixed;left:50%;top:64px;transform:translateX(-50%);padding:4px 12px;border-radius:8px;background:var(--panel,var(--bg-raised,#111));color:var(--ink,var(--fg,#ddd));box-shadow:0 8px 24px oklch(0 0 0 / .3)}
.nova-tools-scratch{position:fixed;inset:0;z-index:945;background:oklch(0.1 0.02 262 / .12);backdrop-filter:blur(1px);pointer-events:auto}
.nova-tools-scratch-ta{position:absolute;inset:8vh 10vw;width:80vw;height:76vh;font:16px/1.7 var(--mono,Consolas,monospace);background:transparent;border:0;outline:0;color:var(--ink,var(--fg,#ddd));resize:none}
.nova-tools-scratch-chip{position:fixed;right:18px;top:14px;padding:3px 12px;border-radius:999px;background:var(--panel,var(--bg-raised,#111));color:var(--ink,var(--fg,#ddd));font-variant-numeric:tabular-nums;letter-spacing:.08em}
.nova-tools-scratch-chip[data-hint="1"]{box-shadow:0 0 0 2px oklch(0.78 0.16 55 / .8)}
.nova-tools-scratch-extend,.nova-tools-scratch-close{position:fixed;font:inherit;padding:3px 10px;border-radius:6px;border:1px solid var(--accent-soft);background:var(--panel,var(--bg-raised,#111));color:inherit;cursor:pointer}
.nova-tools-scratch-extend{right:18px;top:44px}
.nova-tools-scratch-close{right:18px;bottom:14px}
.nova-tools-scratch.nova-burn{animation:nova-tools-burn .9s var(--ease-standard) forwards}
@keyframes nova-tools-burn{0%{opacity:1;filter:none}55%{opacity:.55;filter:brightness(1.5) blur(1px)}100%{opacity:0;filter:brightness(2.4) blur(6px)}}
[data-reduce-motion="true"] .nova-tools-scratch.nova-burn{animation:none;opacity:0;transition:opacity .3s}
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

function ensureLayer(): void {
  if (typeof document === "undefined") return;
  if (!document.getElementById("nova-tools-layer")) {
    const layer = document.createElement("div");
    layer.id = "nova-tools-layer";
    layer.setAttribute("aria-hidden", "false");
    document.body.appendChild(layer);
    bag.push(() => document.getElementById("nova-tools-layer")?.remove());
  }
}

// ---------------------------------------------------------------------------
// 全局桥（keydown / 计数 / 事件入口）
// ---------------------------------------------------------------------------

function onKeyDown(e: KeyboardEvent): void {
  const now = Date.now();
  lastInputAt = now;
  const k = e.key;

  // W-082 键击计数（真实键击，不含纯修饰）
  if (!["Control", "Alt", "Shift", "Meta", "CapsLock"].includes(k)) {
    habitBump((c) => ({ ...c, keys: c.keys + 1 }));
    bumpLedger((l) => ({ ...l, typed: l.typed + 1 }));
  }

  // Ctrl+Alt+T 横桌
  if (e.ctrlKey && e.altKey && (k === "t" || k === "T") && !e.repeat) {
    e.preventDefault();
    e.stopPropagation();
    toggleTable();
    return;
  }
  // Ctrl+Shift+= 算术选中（含小键盘 +）
  if (e.ctrlKey && e.shiftKey && (k === "=" || k === "+" || k === "Add") && !e.repeat) {
    e.preventDefault();
    e.stopPropagation();
    snippetMath();
    return;
  }
  // F8 语音图钉
  if (k === "F8" && !e.repeat && !e.ctrlKey && !e.altKey && !e.metaKey) {
    onF8Down();
    return;
  }
  // F9 备忘板
  if (k === "F9" && !e.repeat && !e.ctrlKey && !e.altKey && !e.metaKey) {
    e.preventDefault();
    e.stopPropagation();
    toggleScratch();
    return;
  }
  // 横桌聚焦循环 / 收起
  if (tablePanel?.isConnected) {
    if (k === "Escape") {
      e.preventDefault();
      toggleTable(false);
      return;
    }
    if (k === "Tab") {
      e.preventDefault();
      tableFocus = nextQuadrant(tableFocus);
      renderTable();
      return;
    }
  }
  // 备忘板 Esc 收起（焚毁交给计时器）
  if (scratchOverlay?.isConnected && k === "Escape") {
    e.preventDefault();
    toggleScratch(false);
    return;
  }
}

function onMouseDown(e: MouseEvent): void {
  lastInputAt = Date.now();
  if (e.button === 0) habitBump((c) => ({ ...c, clicks: c.clicks + 1 }));
}

function onWheel(): void {
  habitBump((c) => ({ ...c, wheel: c.wheel + 1 }));
}

function onRegistryChange(): void {
  // 开关即时生效：关闭的功能面板立即收起
  if (!flagOn("W-077") && timeboxPanel) toggleTimebox(false);
  if (!flagOn("W-080") && tablePanel) toggleTable(false);
  if (!flagOn("W-082") && habitPanel) toggleHabit(false);
  if (!flagOn("W-085") && shuttlePanel) {
    shuttlePanel.remove();
    shuttlePanel = null;
  }
  if (!flagOn("W-084") && meetingPanel) endMeeting();
  if (!flagOn("W-089") && scratchOverlay) toggleScratch(false);
}

// ---------------------------------------------------------------------------
// 激活 / 卸载（幂等）
// ---------------------------------------------------------------------------

export function activateToolsNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  ensureStyle();
  ensureLayer();
  microLoad();
  habitLoadToday();
  eye = eyeInit();
  lastInputAt = 0;
  undoStack = [];
  shuttleCache = {};
  tableHiddenAt = 0;
  lastFiredBlockKey = "";
  mountMicro();

  document.addEventListener("keydown", onKeyDown, true);
  bag.push(() => document.removeEventListener("keydown", onKeyDown, true));
  document.addEventListener("mousedown", onMouseDown, true);
  bag.push(() => document.removeEventListener("mousedown", onMouseDown, true));
  document.addEventListener("wheel", onWheel, { capture: true, passive: true });
  bag.push(() => document.removeEventListener("wheel", onWheel, true));
  document.addEventListener("keyup", onKeyUpBridge, true);
  bag.push(() => document.removeEventListener("keyup", onKeyUpBridge, true));

  // S17/Hub 事件入口
  const onOpen = (e: Event): void => {
    const panel = (e as CustomEvent).detail?.panel as string | undefined;
    if (panel === "timebox") toggleTimebox();
    else if (panel === "table") toggleTable();
    else if (panel === "habit") toggleHabit();
    else if (panel === "shuttle") toggleShuttle();
    else if (panel === "debrief") openDebrief();
    else if (panel === "scratch") toggleScratch();
    else if (panel === "meeting") openMeeting();
  };
  window.addEventListener("nova://tools-open", onOpen);
  bag.push(() => window.removeEventListener("nova://tools-open", onOpen));

  // Hub overlay 直达（ai04 协议：nova-timebox / nova-table / nova-habit / nova-shuttle / nova-scratch）
  const onOpenFeature = (ev: Event): void => {
    const f = (ev as CustomEvent).detail?.feature;
    if (f === "nova-timebox" && flagOn("W-077")) toggleTimebox();
    if (f === "nova-table" && flagOn("W-080")) toggleTable();
    if (f === "nova-habit" && flagOn("W-082")) toggleHabit();
    if (f === "nova-shuttle" && flagOn("W-085")) toggleShuttle();
    if (f === "nova-scratch" && flagOn("W-089")) toggleScratch();
  };
  window.addEventListener("ai04:open-feature", onOpenFeature);
  bag.push(() => window.removeEventListener("ai04:open-feature", onOpenFeature));

  // S17 接线摄入：窗口关闭 / 窗口切换 / 跨工具拖放 / 双栏注册 / 会议编排 / 梭笔样本
  const onWinClose = (): void => bumpLedger((l) => ({ ...l, closed: l.closed + 1 }));
  window.addEventListener("nova://tools-win-close", onWinClose);
  bag.push(() => window.removeEventListener("nova://tools-win-close", onWinClose));
  const onSwitch = (e: Event): void => {
    const via = (e as CustomEvent).detail?.via as string | undefined;
    habitBump((c) => (via === "keys" ? { ...c, keySwitch: c.keySwitch + 1 } : { ...c, mouseSwitch: c.mouseSwitch + 1 }));
  };
  window.addEventListener("nova://tools-switch", onSwitch);
  bag.push(() => window.removeEventListener("nova://tools-switch", onSwitch));
  const onCross = (e: Event): void => {
    const name = (e as CustomEvent).detail?.name as string | undefined;
    if (typeof name === "string") crossDrop(name);
  };
  window.addEventListener("nova://tools-crossdrop", onCross);
  bag.push(() => window.removeEventListener("nova://tools-crossdrop", onCross));
  const onPanes = (e: Event): void => {
    const d = (e as CustomEvent).detail as { a?: PaneState; b?: PaneState } | undefined;
    if (d?.a && d?.b) {
      paneA = d.a;
      paneB = d.b;
    }
  };
  window.addEventListener("nova://tools-panes", onPanes);
  bag.push(() => window.removeEventListener("nova://tools-panes", onPanes));
  const onSwapReq = (): void => swapPanesAction();
  window.addEventListener("nova://tools-swap", onSwapReq);
  bag.push(() => window.removeEventListener("nova://tools-swap", onSwapReq));
  const onMeeting = (e: Event): void => {
    const phase = (e as CustomEvent).detail?.phase as string | undefined;
    if (phase === "enter") openMeeting();
    else if (phase === "exit") endMeeting();
  };
  window.addEventListener("nova://tools-meeting", onMeeting);
  bag.push(() => window.removeEventListener("nova://tools-meeting", onMeeting));
  const onShuttleSample = (e: Event): void => {
    const app = (e as CustomEvent).detail?.app as string | undefined;
    if (typeof app === "string" && app) {
      const ts = Date.now();
      const key = dayHourKey(ts);
      if (!shuttleCache[key]) shuttleCache = evictShuttle({ ...shuttleCache, [key]: { ts, app } }, ts);
    }
  };
  window.addEventListener("nova://tools-shuttle-sample", onShuttleSample);
  bag.push(() => window.removeEventListener("nova://tools-shuttle-sample", onShuttleSample));

  // 心跳
  const hb = window.setInterval(heartbeat, 1000);
  bag.push(() => window.clearInterval(hb));

  // S0 注册表变化 → 即时生效/失效
  void Promise.resolve().then(async () => {
    try {
      const mod = await import("../registry");
      bag.push(mod.subscribeNova(onRegistryChange));
    } catch {
      /* S0 未就绪：开关即时生效退化为下次激活生效 */
    }
  });
}

function onKeyUpBridge(e: KeyboardEvent): void {
  if (e.key === "F8") void onF8Up();
}

export function deactivateToolsNova(): void {
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
  timeboxPanel?.remove();
  tablePanel?.remove();
  debriefPanel?.remove();
  habitPanel?.remove();
  shuttlePanel?.remove();
  meetingPanel?.remove();
  microStrip?.remove();
  blinkChip?.remove();
  voiceHud?.remove();
  scratchOverlay?.remove();
  document.querySelectorAll(".nova-tools-hud").forEach((n) => n.remove());
  timeboxPanel = null;
  tablePanel = null;
  debriefPanel = null;
  habitPanel = null;
  shuttlePanel = null;
  meetingPanel = null;
  microStrip = null;
  blinkChip = null;
  voiceHud = null;
  scratchOverlay = null;
  microList = [];
  eye = eyeInit();
  undoStack = [];
  shuttleCache = {};
  scratchText = "";
  scratchStartAt = 0;
  scratchExtended = 0;
  f8HeldAt = 0;
  timeboxSelStart = null;
  lastFiredBlockKey = "";
  tableStateClean();
}

function tableStateClean(): void {
  for (const k of Object.keys(tableState)) delete tableState[k];
}

export function isToolsNovaActive(): boolean {
  return active;
}

/** 句柄（S0 NovaRuntime / 中枢直接消费）。 */
export const toolsNova = {
  activate() {
    activateToolsNova();
  },
  deactivate() {
    deactivateToolsNova();
  },
  api: {
    microAdd,
    toggleTimebox,
    toggleTable,
    toggleHabit,
    toggleShuttle,
    toggleScratch,
    openDebrief,
    openMeeting,
    endMeeting,
    crossDrop,
    swapPanesAction,
    undoSnippetMath,
    toolForFile,
    evalSnippet,
    convertUnit,
    habitSevenDays,
  },
};
