/**
 * NOVA-200 · S6 文件叙事路（AI-06）—— 域6 文件与数据能力（W-064…W-076）。
 *
 * 边界（全景 §6）：M-27 目录哨兵管变更告警、Z-34 管空间占用分析、Q-50 收藏抽屉
 * 管异构收藏（不回还）、V-55 大文件雷达管全盘扫描、U-30 数据血缘管数据衍生、
 * U-26 全局标签管标签体系、U-55 空状态规范管空态、U-28 传输指挥台管传输控制、
 * M-90 午夜基座管跨午夜正确性、U-27 回收站 2.0 管回收站体系、Z-31/M-24 管固定
 * 位置与手动书签；本模块补**文件活性、人口普查、借阅闭环、墨阶排版、增长外推、
 * 迁途轨迹、标签代数、末态幽灵页、传输律动、子夜更钟、季节年轮、删除溯源、
 * 近踪动港**。
 *
 * 纪律：
 * - 零侵入：不改写 Explorer/QuickPreview/回收站内部逻辑；全部为 DOM 叠层 +
 *   CSS 变量/类名挂载 + `nova://files-*` 自定义事件摄入（文件系统侧接线由
 *   S17 在既有文件按 wiringHint 补齐，本模块 API/事件契约已备好）；
 * - 前缀：类名 `nova-`、事件 `nova://files-*`、localStorage 键 `nova.files.*`；
 * - 开关：只读消费 S0 注册表（registry.ts，novaOn/novaNum），无注册表时用 defaultOn 回退；
 * - 降级：reduce-motion / safeMode / static 下动效归零（脉搏呼吸/幽灵回弹/律动声
 *   节拍），语义与数据保留；声音功能在静音环境（用户开关）如实跳过；
 * - 诚实：纯本地数学，零联网；环境尚未派发的事件在面板空态如实说明，
 *   删除方无法识别时如实显示 SYSTEM，不伪造出身。
 */

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

const NS = "nova.files";

/** 功能开关：只读消费 S0 注册表（nova.registry.v1 单一事实源）。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

function num(id: string, key: string): number {
  return novaNum(id, key);
}

/** 派发 `nova://files-*` 事件（SSR/测试环境安全）。 */
export function filesEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://files-${name}`, { detail }));
}

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

/** 本地时区日键（YYYY-MM-DD，子夜结算/更钟用）。 */
export function dayKeyOf(t: number): string {
  const d = new Date(t);
  const m = `${d.getMonth() + 1}`.padStart(2, "0");
  const day = `${d.getDate()}`.padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
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

export const FILES_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-064",
    titleZh: "文件脉搏",
    titleEn: "File Pulse",
    descZh: "正在被写入的文件（日志/下载/渲染输出）图标 2s 周期脉搏微光——一眼识别哪些文件活着；写入停止 5s 后脉搏停息。每目录 ≤ 1 监听句柄。",
    defaultOn: true,
    wiringHint: "文件行补 data-nova-path 并在写入时派发 nova://files-write {path}，类与样式已备好",
    degrade: "reduce-motion 下呼吸动画归零，改为恒定微光标记（活性语义保留）",
  },
  {
    id: "W-065",
    titleZh: "目录人口普查",
    titleEn: "Census Report",
    descZh: "目录「人口普查」一页报告：类型构成饼图 + Top5 类型清单 + 平均文件年龄 + 空文件/超大文件极值；纯本地扫描，报告可导出文本。",
    defaultOn: true,
    overlay: "nova-census",
    wiringHint: "explorer 右键『人口普查』派发 nova://files-census {dir, entries} 即出报告（面板已备好）",
    degrade: "静态报告无动效；万文件目录后台扫描不阻塞 UI",
  },
  {
    id: "W-066",
    titleZh: "借阅书架",
    titleEn: "Borrow Shelf",
    descZh: "侧栏借阅书架：拖文件上架 = 借出（记录来源目录），用毕归还回原位；48h 未归还淡化为逾期色——为「临时抓几个文件来用」建立秩序。书架 24 册上限。",
    defaultOn: true,
    wiringHint: "拖拽上架派发 nova://files-borrow {path, from}；归还真实移动需 S17 接 FS move（事件已备好）",
    degrade: "无动效；归还时原目录已删如实提示降级为复制（事件负载含 originGone）",
  },
  {
    id: "W-067",
    titleZh: "文件墨阶",
    titleEn: "Size Ink",
    descZh: "文件名排版重量随体量分级加深：MB 级 300 字重、GB 级 500 字重 + 8% 灰底、≥10GB 巨石附微图标——用排版直觉传达体量；色弱安全（非纯色相区分）。",
    defaultOn: true,
    wiringHint: "文件行补 data-size 字节数属性即自动套用墨阶（观察器已备好）",
    degrade: "无动效（纯排版叠加）；与 V-32 过滤叠加正常",
  },
  {
    id: "W-068",
    titleZh: "目录增长预报",
    titleEn: "Growth Forecast",
    descZh: "读最近 30 天增长曲线外推未来 30 天（线性 + 近期加权双模型），预报到达红色水位（剩余空间 10%）的日期；诚实显示置信带；样本 < 14 天显示数据不足。纯本地数学。",
    defaultOn: true,
    wiringHint: "explorer 定期派发 nova://files-growth {dir, bytes} 采样；右键『增长预报』开面板",
    degrade: "静态预报卡；数据不足如实显示 INSUFFICIENT DATA，不硬算",
  },
  {
    id: "W-069",
    titleZh: "文件迁途志",
    titleEn: "Move Journal",
    descZh: "环境内每次移动/重命名记录轨迹链（旧路径→新路径 + 时间）；「迁途志」查看该文件完整路径演变——找回那个被我挪走的文件。90 天滚动保留。",
    defaultOn: true,
    overlay: "nova-journal",
    wiringHint: "explorer 移动/重命名派发 nova://files-move {from, to}；系统外移动如实断链",
    degrade: "静态轨迹链视图；仅记录环境内操作（诚实边界）",
  },
  {
    id: "W-070",
    titleZh: "标签代数",
    titleEn: "Tag Algebra",
    descZh: "标签集合运算层：任意交/并/差组合（如 工作 ∩ 未完成 - 已归档）保存为「代数文件夹」实时物化；语法错误即时高亮出错位置；增量刷新。",
    defaultOn: true,
    wiringHint: "标签索引派发 nova://files-tags {tag, paths} 喂入；代数文件夹面板已备好",
    degrade: "静态求值；语法错误即时高亮（非动画）",
  },
  {
    id: "W-071",
    titleZh: "预览幽灵页",
    titleEn: "Ghost Page",
    descZh: "长文档（≥3 页）预览末页之后渲染 12% 透明度幽灵页轮廓——暗示内容到此为止，避免滚动空转；不可滚入（橡皮筋一次回弹）；图片/音视频不适用。",
    defaultOn: true,
    wiringHint: "QuickPreview 容器补 data-nova-preview='doc' data-pages=N 即自动挂幽灵页",
    degrade: "reduce-motion 下回弹动画归零，幽灵页静止显示（末态语义保留）",
  },
  {
    id: "W-072",
    titleZh: "传输律动声",
    titleEn: "Transfer Groove",
    descZh: "大文件（≥100MB）传输时与速率联动的环境律动声：速率→节拍密度单调映射，完成→终止和弦，失败→不和谐音如实；静音环境跳过。",
    defaultOn: true,
    params: [{ key: "minMB", labelKey: "novaP_minMB", type: "slider", default: 100, min: 0, max: 1024, step: 50 }] as never,
    wiringHint: "传输指挥台派发 nova://files-transfer {id, total, rate, state}（事件已备好）",
    degrade: "静音环境/无 AudioContext 如实跳过；失败音为不和谐双音（诚实反馈）",
  },
  {
    id: "W-073",
    titleZh: "子夜更钟",
    titleEn: "Midnight Delta",
    descZh: "每日 00:00 结算点：3s 轻提示卡汇报今日环境数据增量（新增/删除文件数 + 净变化字节数），一天的数据潮汐一目了然；勿扰时段自动转为明晨首现。",
    defaultOn: true,
    wiringHint: "每日增量由 nova://files-delta {added, removed, bytesAdded, bytesRemoved} 事件累加；结算卡自动出现",
    degrade: "无动效卡片直显；结算精确到字节（复用 M-90 午夜基座语义）",
  },
  {
    id: "W-074",
    titleZh: "目录季节环",
    titleEn: "Season Ring",
    descZh: "目录属性「季节环」：12 扇区环形图显示全年文件创建月分布（哪些月份长出来的），点击扇区按月过滤——目录的年轮。空目录如实显示空态。",
    defaultOn: true,
    overlay: "nova-seasonring",
    wiringHint: "目录属性派发 nova://files-season {dir, createdAts}；面板已备好",
    degrade: "静态 SVG 环（无动画）；渲染 ≤ 300ms",
  },
  {
    id: "W-075",
    titleZh: "回收站出身簿",
    titleEn: "Bin Provenance",
    descZh: "回收站每项附出身徽标：由哪个应用删除 + 原路径 + 入站时间；支持按出身应用筛选——刚才那个软件删了什么一查便知；无法识别如实显示 SYSTEM。",
    defaultOn: true,
    wiringHint: "删除入站派发 nova://files-trashed {id, name, by, origin}；徽标渲染需回收站行补 data-bin-id",
    degrade: "静态徽标；出身簿不拦截还原（还原路径零影响）",
  },
  {
    id: "W-076",
    titleZh: "近踪动港",
    titleEn: "Recent Haven",
    descZh: "侧栏「最近用过的文件夹」动态区：最近 5 个真实浏览过的目录自动浮列并按使用频率排序，一周未访问即淡出；隐身会话不记录——动线缓存防膨胀。",
    defaultOn: true,
    wiringHint: "文件管理器路由变化派发 nova://files-visit {path}；隐身会话派发 nova://files-stealth {on:true}",
    degrade: "无动效浮列；仅记录真实访问（诚实边界），上限 5 枚",
  },
];

export const filesNovaDomain = {
  id: "S6",
  nameZh: "文件叙事",
  nameEn: "File Narrative",
  route: "AI-06",
  features: FILES_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

// ---- W-064 文件脉搏 ----

export const PULSE_PERIOD_MS = 2000;
export const PULSE_LINGER_MS = 5000;

/** 文件仍处「活着」窗口：最后一次写入距今 ≤ 5s（写入停止 5s 后脉搏停息）。 */
export function pulseActive(lastWriteAt: number, now: number, linger = PULSE_LINGER_MS): boolean {
  return now - lastWriteAt <= linger;
}

/** 脉搏微光强度 0..1（2s 余弦呼吸；离场窗口外恒 0）。 */
export function pulseGlow(lastWriteAt: number, now: number, period = PULSE_PERIOD_MS): number {
  if (!pulseActive(lastWriteAt, now)) return 0;
  const phase = ((now - lastWriteAt) % period) / period;
  const glow = 0.5 - 0.5 * Math.cos(2 * Math.PI * phase);
  return Math.round(glow * 1000) / 1000;
}

/** 剔除已停息的路径（纯函数：返回新表）。 */
export function prunePulsePaths(
  paths: Record<string, number>,
  now: number,
  linger = PULSE_LINGER_MS,
): Record<string, number> {
  const next: Record<string, number> = {};
  for (const [p, at] of Object.entries(paths)) {
    if (pulseActive(at, now, linger)) next[p] = at;
  }
  return next;
}

// ---- W-065 目录人口普查 ----

export interface CensusEntry {
  name: string;
  size: number;
  mtime: number;
}

export interface CensusTypeStat {
  type: string;
  count: number;
  /** 占比 %（1 位小数）。 */
  pct: number;
}

export interface CensusReport {
  total: number;
  types: CensusTypeStat[];
  top5: CensusTypeStat[];
  /** 平均文件年龄（天，1 位小数）。 */
  avgAgeDays: number;
  /** 空文件数（size === 0）。 */
  emptyCount: number;
  /** 最大文件极值（空目录为 null）。 */
  largest: { name: string; size: number } | null;
}

/** 扩展名归一：无扩展名/点文件 → "FILE"（如实归类，不猜类型）。 */
export function censusExt(name: string): string {
  const i = name.lastIndexOf(".");
  if (i <= 0) return "FILE";
  const ext = name.slice(i + 1).trim().toLowerCase();
  return ext.length > 0 ? ext : "FILE";
}

function round1(v: number): number {
  return Math.round(v * 10) / 10;
}

export function censusReport(entries: CensusEntry[], now: number): CensusReport {
  const total = entries.length;
  if (total === 0) {
    return { total: 0, types: [], top5: [], avgAgeDays: 0, emptyCount: 0, largest: null };
  }
  const byType = new Map<string, number>();
  let ageSum = 0;
  let emptyCount = 0;
  let largest: CensusReport["largest"] = null;
  for (const e of entries) {
    const t = censusExt(e.name);
    byType.set(t, (byType.get(t) ?? 0) + 1);
    ageSum += now - e.mtime;
    if (e.size === 0) emptyCount++;
    if (largest === null || e.size > largest.size) largest = { name: e.name, size: e.size };
  }
  const types: CensusTypeStat[] = [...byType.entries()]
    .map(([type, count]) => ({ type, count, pct: round1((count / total) * 100) }))
    .sort((a, b) => b.count - a.count || a.type.localeCompare(b.type));
  return {
    total,
    types,
    top5: types.slice(0, 5),
    avgAgeDays: round1(ageSum / total / DAY),
    emptyCount,
    largest,
  };
}

/** 人口普查纯文本报告（可导出）。 */
export function censusTextReport(dir: string, report: CensusReport, now: number): string {
  const lines: string[] = [];
  lines.push(`CENSUS REPORT · ${dir}`);
  lines.push(`GENERATED ${new Date(now).toLocaleString()}`);
  lines.push(`FILES ${report.total} · AVG AGE ${report.avgAgeDays}d · EMPTY ${report.emptyCount}`);
  if (report.largest) lines.push(`LARGEST ${report.largest.name} (${report.largest.size} B)`);
  for (const t of report.types) lines.push(`${t.type.padEnd(10, " ")} ${`${t.count}`.padStart(5, " ")} ${t.pct}%`);
  return lines.join("\n");
}

// ---- W-066 借阅书架 ----

export const SHELF_CAPACITY = 24;
export const BORROW_OVERDUE_MS = 48 * HOUR;

export interface BorrowRecord {
  /** 身份（默认取路径）。 */
  id: string;
  name: string;
  /** 来源目录（归还目标）。 */
  from: string;
  borrowedAt: number;
}

export type BorrowResult =
  | { ok: true; shelf: BorrowRecord[] }
  | { ok: false; reason: "dup" | "full"; shelf: BorrowRecord[] };

/** 借出：重复借出拒绝（dup）、书架满拒绝（full，24 册上限）。 */
export function borrowBook(
  shelf: BorrowRecord[],
  book: { id: string; name: string; from: string },
  now: number,
): BorrowResult {
  if (shelf.some((r) => r.id === book.id)) return { ok: false, reason: "dup", shelf };
  if (shelf.length >= SHELF_CAPACITY) return { ok: false, reason: "full", shelf };
  return { ok: true, shelf: [...shelf, { ...book, borrowedAt: now }] };
}

/** 归还：移出书架并带回记录（真实移动回 from 目录由行为层/S17 执行）。 */
export function returnBook(shelf: BorrowRecord[], id: string): { shelf: BorrowRecord[]; record: BorrowRecord | null } {
  const record = shelf.find((r) => r.id === id) ?? null;
  return { shelf: shelf.filter((r) => r.id !== id), record };
}

/** 逾期书目 id（> 48h 未归还 → 淡化逾期色）。 */
export function overdueIds(shelf: BorrowRecord[], now: number, limit = BORROW_OVERDUE_MS): string[] {
  return shelf.filter((r) => now - r.borrowedAt > limit).map((r) => r.id);
}

// ---- W-067 文件墨阶 ----

export const INK_MEGABYTE = 1024 * 1024;
export const INK_GIGABYTE = 1024 * INK_MEGABYTE;
export const INK_MEGALITH_BYTES = 10 * INK_GIGABYTE;

export interface InkTier {
  /** 0 常规 / 1 MB 级 / 2 GB 级（单调递增的视觉强调档）。 */
  tier: 0 | 1 | 2;
  /** CSS font-weight。 */
  weight: 400 | 300 | 500;
  /** 8% 灰底（GB 级）。 */
  gray: boolean;
  /** 巨石微图标（≥10GB）。 */
  megalith: boolean;
}

/** 体量→墨阶三档（+巨石标）；输入负数按 0 处理。 */
export function inkTier(size: number): InkTier {
  const s = Math.max(0, size);
  if (s >= INK_GIGABYTE) return { tier: 2, weight: 500, gray: true, megalith: s >= INK_MEGALITH_BYTES };
  if (s >= INK_MEGABYTE) return { tier: 1, weight: 300, gray: false, megalith: false };
  return { tier: 0, weight: 400, gray: false, megalith: false };
}

// ---- W-068 目录增长预报 ----

export const GROWTH_MIN_DAYS = 14;
export const GROWTH_HORIZON_DAYS = 30;
export const WATERMARK_FREE_PCT = 0.1;

export interface GrowthSample {
  t: number;
  bytes: number;
}

export interface GrowthForecast {
  insufficient: boolean;
  /** 线性模型斜率（字节/天）。 */
  perDayLinear: number;
  /** 近期加权模型斜率（字节/天，投影采用）。 */
  perDayWeighted: number;
  /** 加权模型外推 horizon 天后的占用字节。 */
  projectedBytes: number;
  /** 置信带半宽（字节，诚实显示）。 */
  bandBytes: number;
}

/** 样本是否足够（跨度 ≥ 14 天且 ≥ 2 点，否则如实显示数据不足；对输入顺序鲁棒）。 */
export function enoughGrowthSamples(samples: GrowthSample[], minDays = GROWTH_MIN_DAYS): boolean {
  if (samples.length < 2) return false;
  const tMin = Math.min(...samples.map((s) => s.t));
  const tMax = Math.max(...samples.map((s) => s.t));
  return tMax - tMin >= minDays * DAY;
}

/** 线性 + 近期加权双模型外推（加权 = 逐点权重随时间线性递增；内部按时间升序）。 */
export function forecastGrowth(samples: GrowthSample[], horizonDays = GROWTH_HORIZON_DAYS): GrowthForecast {
  const sorted = [...samples].sort((a, b) => a.t - b.t);
  const lastBytes = sorted.length > 0 ? sorted[sorted.length - 1]!.bytes : 0;
  if (!enoughGrowthSamples(sorted)) {
    return { insufficient: true, perDayLinear: 0, perDayWeighted: 0, projectedBytes: lastBytes, bandBytes: 0 };
  }
  const t0 = sorted[0]!.t;
  const xs = sorted.map((s) => (s.t - t0) / DAY);
  const ys = sorted.map((s) => s.bytes);
  const n = sorted.length;

  // 线性 OLS
  const mx = xs.reduce((a, b) => a + b, 0) / n;
  const my = ys.reduce((a, b) => a + b, 0) / n;
  let sxy = 0;
  let sxx = 0;
  for (let i = 0; i < n; i++) {
    sxy += (xs[i]! - mx) * (ys[i]! - my);
    sxx += (xs[i]! - mx) ** 2;
  }
  const linear = sxx > 0 ? sxy / sxx : 0;

  // 近期加权 WLS（w_i = i+1）
  let wSum = 0;
  let wmx = 0;
  let wmy = 0;
  for (let i = 0; i < n; i++) {
    const w = i + 1;
    wSum += w;
    wmx += w * xs[i]!;
    wmy += w * ys[i]!;
  }
  wmx /= wSum;
  wmy /= wSum;
  let wsxy = 0;
  let wsxx = 0;
  for (let i = 0; i < n; i++) {
    const w = i + 1;
    wsxy += w * (xs[i]! - wmx) * (ys[i]! - wmy);
    wsxx += w * (xs[i]! - wmx) ** 2;
  }
  const weighted = wsxx > 0 ? wsxy / wsxx : 0;

  // 置信带半宽：加权残差方差 → 斜率标准误 × 1.96 × horizon（n<3 退化为斜率半程）
  let band = Math.abs(weighted) * horizonDays * 0.5;
  if (n >= 3) {
    let sse = 0;
    for (let i = 0; i < n; i++) {
      const r = ys[i]! - (wmy + weighted * (xs[i]! - wmx));
      sse += (i + 1) * r * r;
    }
    const dof = Math.max(1, n - 2);
    const se = Math.sqrt(sse / dof / Math.max(wsxx, 1e-9));
    band = 1.96 * se * horizonDays;
  }

  return {
    insufficient: false,
    perDayLinear: linear,
    perDayWeighted: weighted,
    projectedBytes: Math.max(0, Math.round(lastBytes + weighted * horizonDays)),
    bandBytes: Math.round(band),
  };
}

export interface WatermarkVerdict {
  insufficient: boolean;
  /** 到达红色水位（剩余 10%）的时刻；perDay ≤ 0 → null（按当前趋势永不到达）。 */
  dueAt: number | null;
}

export function watermarkDue(
  samples: GrowthSample[],
  capacityBytes: number,
  now: number,
): WatermarkVerdict {
  const f = forecastGrowth(samples);
  if (f.insufficient) return { insufficient: true, dueAt: null };
  const threshold = capacityBytes * (1 - WATERMARK_FREE_PCT);
  // 最新样本 = t 最大的点（对输入顺序鲁棒）
  const last = samples.reduce((a, b) => (b.t > a.t ? b : a));
  if (last.bytes >= threshold) return { insufficient: false, dueAt: now };
  if (f.perDayWeighted <= 0) return { insufficient: false, dueAt: null };
  const days = (threshold - last.bytes) / f.perDayWeighted;
  return { insufficient: false, dueAt: last.t + Math.ceil(days * DAY) };
}

// ---- W-069 文件迁途志 ----

export const JOURNAL_DAYS = 90;
export const JOURNAL_MAX = 500;

export interface MoveEdge {
  from: string;
  to: string;
  at: number;
}

/** 90 天滚动 + 500 条上限裁剪。 */
export function pruneJournal(journal: MoveEdge[], now: number, days = JOURNAL_DAYS): MoveEdge[] {
  const cut = now - days * DAY;
  return journal.filter((e) => e.at >= cut).slice(-JOURNAL_MAX);
}

/** 记录一次环境内移动/重命名（系统外移动不产生事件 → 如实断链）。 */
export function recordMove(journal: MoveEdge[], from: string, to: string, at: number): MoveEdge[] {
  return pruneJournal([...journal, { from, to, at }], at);
}

/** 以当前路径反查完整轨迹链（旧→新有序；自环/分叉防护）。 */
export function journalTrail(journal: MoveEdge[], path: string): MoveEdge[] {
  const used = new Set<number>();
  const trail: MoveEdge[] = [];
  let cur = path;
  for (;;) {
    let best = -1;
    for (let i = 0; i < journal.length; i++) {
      if (used.has(i)) continue;
      if (journal[i]!.to === cur) best = i;
    }
    if (best < 0) break;
    used.add(best);
    const e = journal[best]!;
    trail.unshift(e);
    if (e.from === cur) break;
    cur = e.from;
  }
  return trail;
}

// ---- W-070 标签代数 ----

export type AlgNode =
  | { k: "tag"; name: string }
  | { k: "union"; l: AlgNode; r: AlgNode }
  | { k: "inter"; l: AlgNode; r: AlgNode }
  | { k: "diff"; l: AlgNode; r: AlgNode };

export interface AlgOk {
  ok: true;
  ast: AlgNode;
}

export interface AlgErr {
  ok: false;
  /** 出错字符下标（即时高亮定位）。 */
  errorAt: number;
  message: string;
}

interface AlgTok {
  k: "tag" | "∪" | "∩" | "-" | "(" | ")";
  v: string;
  at: number;
}

/** 词法：标签为非运算符/非括号/非空白的连续段；未知符号报错定位。 */
export function algebraTokenize(expr: string): AlgTok[] | AlgErr {
  const toks: AlgTok[] = [];
  let i = 0;
  while (i < expr.length) {
    const ch = expr[i]!;
    if (ch === " " || ch === "\t" || ch === "\n") {
      i++;
      continue;
    }
    if (ch === "(" || ch === "（") {
      toks.push({ k: "(", v: ch, at: i });
      i++;
      continue;
    }
    if (ch === ")" || ch === "）") {
      toks.push({ k: ")", v: ch, at: i });
      i++;
      continue;
    }
    if (ch === "∩" || ch === "∪" || ch === "-") {
      toks.push({ k: ch, v: ch, at: i });
      i++;
      continue;
    }
    if (ch === "&" || ch === "|" || ch === "!" || ch === "+" || ch === "~") {
      return { ok: false, errorAt: i, message: `未知符号 "${ch}"（运算符仅 ∩ ∪ -）` };
    }
    let j = i + 1;
    while (j < expr.length) {
      const c = expr[j]!;
      if (c === " " || c === "\t" || c === "\n") break;
      if (c === "(" || c === ")" || c === "（" || c === "）" || c === "∩" || c === "∪" || c === "-") break;
      if (c === "&" || c === "|" || c === "!" || c === "+" || c === "~") break;
      j++;
    }
    toks.push({ k: "tag", v: expr.slice(i, j).trim(), at: i });
    i = j;
  }
  return toks;
}

/**
 * 解析标签代数表达式。优先级：∩ 与 - 同级左结合，∪ 最低。
 * `工作 ∩ 未完成 - 已归档` = (工作∩未完成) - 已归档。
 */
export function parseAlgebra(expr: string): AlgOk | AlgErr {
  const tk = algebraTokenize(expr);
  if (!Array.isArray(tk)) return tk;
  const toks = tk;
  let i = 0;

  const parseUnion = (): AlgOk | AlgErr => {
    let left = parseTerm();
    if (!left.ok) return left;
    while (i < toks.length && toks[i]!.k === "∪") {
      i++;
      const right = parseTerm();
      if (!right.ok) return right;
      left = { ok: true, ast: { k: "union", l: left.ast, r: right.ast } };
    }
    return left;
  };

  const parseTerm = (): AlgOk | AlgErr => {
    let left = parseFactor();
    if (!left.ok) return left;
    while (i < toks.length && (toks[i]!.k === "∩" || toks[i]!.k === "-")) {
      const op = toks[i]!.k;
      i++;
      const right = parseFactor();
      if (!right.ok) return right;
      left = { ok: true, ast: { k: op === "∩" ? "inter" : "diff", l: left.ast, r: right.ast } };
    }
    return left;
  };

  const parseFactor = (): AlgOk | AlgErr => {
    const t = toks[i];
    if (!t) return { ok: false, errorAt: expr.length, message: "表达式意外结束" };
    if (t.k === "(") {
      i++;
      const inner = parseUnion();
      if (!inner.ok) return inner;
      const close = toks[i];
      if (!close || close.k !== ")") {
        return { ok: false, errorAt: close ? close.at : expr.length, message: "括号未闭合" };
      }
      i++;
      return inner;
    }
    if (t.k === "tag") {
      i++;
      return { ok: true, ast: { k: "tag", name: t.v } };
    }
    return { ok: false, errorAt: t.at, message: `此处不应为 "${t.v}"` };
  };

  if (toks.length === 0) return { ok: false, errorAt: 0, message: "空表达式" };
  const top = parseUnion();
  if (!top.ok) return top;
  if (i < toks.length) {
    return { ok: false, errorAt: toks[i]!.at, message: `此处不应为 "${toks[i]!.v}"` };
  }
  return top;
}

/** 代数求值：标签→路径集合的交/并/差，输出排序路径。 */
export function evalAlgebra(node: AlgNode, tags: Record<string, string[]>): string[] {
  const walk = (n: AlgNode): Set<string> => {
    switch (n.k) {
      case "tag":
        return new Set(tags[n.name] ?? []);
      case "union": {
        const s = walk(n.l);
        for (const v of walk(n.r)) s.add(v);
        return s;
      }
      case "inter": {
        const s = walk(n.l);
        const r = walk(n.r);
        return new Set([...s].filter((v) => r.has(v)));
      }
      case "diff": {
        const s = walk(n.l);
        const r = walk(n.r);
        return new Set([...s].filter((v) => !r.has(v)));
      }
    }
  };
  return [...walk(node)].sort();
}

export const ALGEBRA_MAX = 32;

export interface AlgebraFolder {
  name: string;
  expr: string;
  createdAt: number;
}

/** 保存/更新代数文件夹（同名覆盖，上限 32 个）。 */
export function upsertAlgebraFolder(
  list: AlgebraFolder[],
  name: string,
  expr: string,
  now: number,
): AlgebraFolder[] {
  const next = list.filter((f) => f.name !== name);
  next.push({ name, expr, createdAt: now });
  return next.slice(-ALGEBRA_MAX);
}

// ---- W-071 预览幽灵页 ----

export const GHOST_OPACITY = 0.12;
export const GHOST_MIN_PAGES = 3;

/** 长文档（≥3 页）末页之后渲染幽灵页。 */
export function ghostPageVisible(pageCount: number, min = GHOST_MIN_PAGES): boolean {
  return pageCount >= min;
}

/** 橡皮筋阻尼：拉距 → 实际位移（0..max，越拉越硬），用于「不可滚入」。 */
export function ghostOverscroll(px: number, max = 120): number {
  if (px <= 0) return 0;
  return Math.round(max * (px / (px + max)) * 100) / 100;
}

export type PreviewKind = "doc" | "image" | "audio" | "video";

/** 仅文档预览适用（图片/音视频如实不适用）。 */
export function ghostApplies(kind: PreviewKind): boolean {
  return kind === "doc";
}

// ---- W-072 传输律动声 ----

export const GROOVE_MIN_BYTES = 100 * 1024 * 1024;
export const GROOVE_FLOOR_MS = 240;
export const GROOVE_CEIL_MS = 960;
export const GROOVE_RATE_LOW = 64 * 1024;
export const GROOVE_RATE_HIGH = 64 * 1024 * 1024;

/** 只对 ≥ 阈值（默认 100MB）的传输开启。 */
export function grooveEligible(totalBytes: number, min = GROOVE_MIN_BYTES): boolean {
  return totalBytes >= min;
}

/** 速率→节拍间隔：速率越高节拍越密（960ms→240ms 单调；对数刻度）。 */
export function beatIntervalMs(rateBps: number, floor = GROOVE_FLOOR_MS, ceil = GROOVE_CEIL_MS): number {
  const r = Math.max(0, rateBps);
  if (r <= GROOVE_RATE_LOW) return ceil;
  if (r >= GROOVE_RATE_HIGH) return floor;
  const lo = Math.log2(GROOVE_RATE_LOW);
  const hi = Math.log2(GROOVE_RATE_HIGH);
  const t = (Math.log2(r) - lo) / (hi - lo);
  return Math.round(ceil - clamp(t, 0, 1) * (ceil - floor));
}

/** 终止音组：完成 = C-E-G 大三和弦；失败 = 不和谐双音（如实）；进行中 = 空。 */
export function grooveChord(state: "active" | "done" | "failed"): number[] {
  if (state === "done") return [523.25, 659.25, 783.99];
  if (state === "failed") return [466.16, 622.25];
  return [];
}

// ---- W-073 子夜更钟 ----

export interface DayTally {
  added: number;
  removed: number;
  bytesAdded: number;
  bytesRemoved: number;
}

export interface MidnightDelta {
  addedFiles: number;
  removedFiles: number;
  netFiles: number;
  netBytes: number;
}

/** 结算：精确到字节（复用 M-90 午夜基座语义）。 */
export function settleMidnight(t: DayTally): MidnightDelta {
  return {
    addedFiles: t.added,
    removedFiles: t.removed,
    netFiles: t.added - t.removed,
    netBytes: t.bytesAdded - t.bytesRemoved,
  };
}

/** 距下一个本地结算点（默认 00:00）的毫秒数。 */
export function msUntilSettle(now: number, hour = 0): number {
  const d = new Date(now);
  let t = new Date(d.getFullYear(), d.getMonth(), d.getDate(), hour, 0, 0, 0).getTime();
  if (t <= now) t += DAY;
  return t - now;
}

/** 勿扰时段（默认 22:00–07:00，跨午夜区间）。 */
export function inDndHours(now: number, from = 22, to = 7): boolean {
  const h = new Date(now).getHours();
  if (from > to) return h >= from || h < to;
  return h >= from && h < to;
}

/** 明晨首现时刻（默认 08:00；若今日该点未到则取今日）。 */
export function nextMorningAt(now: number, hour = 8): number {
  const d = new Date(now);
  let t = new Date(d.getFullYear(), d.getMonth(), d.getDate(), hour, 0, 0, 0).getTime();
  if (t <= now) t += DAY;
  return t;
}

// ---- W-074 目录季节环 ----

/** 创建时刻 → 12 月计数（本地时区月份）。 */
export function ringMonths(createdAts: number[]): number[] {
  const months = new Array<number>(12).fill(0);
  for (const t of createdAts) {
    const m = new Date(t).getMonth();
    if (m >= 0 && m < 12) months[m]! += 1;
  }
  return months;
}

export function ringTotal(counts: number[]): number {
  return counts.reduce((a, b) => a + b, 0);
}

export interface RingSector {
  month: number;
  startDeg: number;
  sweepDeg: number;
}

/** 12 扇区角（0 月从正上方 -90° 起顺时针；空环全 0 扇区 → 空态）。 */
export function sectorAngles(counts: number[]): RingSector[] {
  const total = ringTotal(counts);
  const out: RingSector[] = [];
  let start = -90;
  for (let m = 0; m < 12; m++) {
    const sweep = total > 0 ? ((counts[m] ?? 0) / total) * 360 : 0;
    out.push({ month: m, startDeg: Math.round(start * 100) / 100, sweepDeg: Math.round(sweep * 100) / 100 });
    start += sweep;
  }
  return out;
}

// ---- W-075 回收站出身簿 ----

export const BIN_MAX = 200;

export interface BinRecord {
  id: string;
  name: string;
  /** 删除方应用标识；空/未知 → 显示 SYSTEM。 */
  by: string;
  origin: string;
  at: number;
}

/** 出身标签：无法识别时如实显示 SYSTEM（不伪造）。 */
export function byLabel(rec: Pick<BinRecord, "by">): string {
  const by = rec.by.trim();
  return by.length > 0 ? by : "SYSTEM";
}

/** 入站登记：去重置顶、上限 200 条滚动。 */
export function recordBin(records: BinRecord[], rec: BinRecord): BinRecord[] {
  const next = records.filter((r) => r.id !== rec.id);
  next.unshift(rec);
  return next.slice(0, BIN_MAX);
}

export function provenanceOf(records: BinRecord[], id: string): BinRecord | null {
  return records.find((r) => r.id === id) ?? null;
}

/** 按出身应用筛选（SYSTEM 归一比较）。 */
export function binByApp(records: BinRecord[], by: string): BinRecord[] {
  const key = byLabel({ by });
  return records.filter((r) => byLabel(r) === key);
}

/** 出站清理（还原/彻底删除后移除出身记录）。 */
export function dropBin(records: BinRecord[], ids: string[]): BinRecord[] {
  const s = new Set(ids);
  return records.filter((r) => !s.has(r.id));
}

// ---- W-076 近踪动港 ----

export const HAVEN_CAP = 5;
export const HAVEN_FADE_MS = 7 * DAY;

export interface HavenEntry {
  path: string;
  lastAt: number;
  count: number;
}

/** 一周未访问 → 淡出（移出浮列）。 */
export function havenStale(e: HavenEntry, now: number, fade = HAVEN_FADE_MS): boolean {
  return now - e.lastAt > fade;
}

/** 记录一次真实浏览：去旧 + 计数 + 按频率排序 + 上限 5 枚防膨胀。 */
export function havenVisit(list: HavenEntry[], path: string, now: number): HavenEntry[] {
  const kept = list.filter((e) => e.path !== path && !havenStale(e, now));
  const prev = list.find((e) => e.path === path);
  const entry: HavenEntry = { path, lastAt: now, count: (prev?.count ?? 0) + 1 };
  return [...kept, entry]
    .sort((a, b) => b.count - a.count || b.lastAt - a.lastAt)
    .slice(0, HAVEN_CAP);
}

// ---------------------------------------------------------------------------
// 行为层（DOM 叠层 + 事件摄入；零侵入既有组件）
// ---------------------------------------------------------------------------

let active = false;
let bag: Array<() => void> = [];

function makeEl(cls: string): HTMLElement {
  const el = document.createElement("div");
  el.className = cls;
  return el;
}

function novaLayer(): HTMLElement | null {
  if (typeof document === "undefined") return null;
  let layer = document.getElementById("nova-layer");
  if (!layer) {
    layer = document.createElement("div");
    layer.id = "nova-layer";
    document.body.appendChild(layer);
  }
  return layer;
}

// ---- 样式注入（域内专属；面板底座复用 nova.css 的 nova-panel） ----

const STYLE_ID = "nova-files-style";

function ensureStyle(): void {
  if (typeof document === "undefined" || document.getElementById(STYLE_ID)) return;
  const css = `
@keyframes nova-files-pulse { 0%,100% { opacity:.25 } 50% { opacity:1 } }
.nova-pulse-on { position:relative; animation: nova-files-pulse 2s ease-in-out infinite; }
.nova-pulse-on::after { content:""; position:absolute; inset:-3px; border-radius:50%;
  box-shadow:0 0 7px 1px currentColor; opacity:.55; pointer-events:none; }
.nova-pulse-static { position:relative; }
.nova-pulse-static::after { content:""; position:absolute; inset:-3px; border-radius:50%;
  box-shadow:0 0 5px 0 currentColor; opacity:.4; pointer-events:none; }
.nova-ink-1 { font-weight:300; }
.nova-ink-2 { font-weight:500; background:rgba(127,127,127,.08); }
.nova-ink-3::before { content:"◈"; font-size:.82em; margin-right:4px; opacity:.85; }
.nova-ghost-page { opacity:.12; border:1px dashed currentColor; border-radius:6px;
  min-height:240px; margin-top:8px; pointer-events:none; display:flex;
  align-items:center; justify-content:center; font-size:12px; letter-spacing:.2em; }
.nova-shelf-overdue { opacity:.45; filter:saturate(.35); }
.nova-haven { position:absolute; left:8px; bottom:64px; display:flex; flex-direction:column; gap:2px;
  padding:6px 8px; border-radius:8px; background:var(--nova-glass,rgba(20,20,24,.72));
  backdrop-filter:blur(10px); border:1px solid rgba(255,255,255,.08); max-width:240px; }
.nova-haven-row { font-size:11px; letter-spacing:.02em; color:inherit; opacity:.85;
  white-space:nowrap; overflow:hidden; text-overflow:ellipsis; cursor:default; }
.nova-haven-title { font-size:9px; letter-spacing:.18em; opacity:.5; margin-bottom:2px; }
.nova-midnight { position:absolute; left:50%; bottom:72px; transform:translateX(-50%);
  padding:10px 16px; border-radius:10px; background:var(--nova-glass,rgba(20,20,24,.8));
  backdrop-filter:blur(12px); border:1px solid rgba(255,255,255,.1); font-size:12px;
  letter-spacing:.06em; text-align:center; }
.nova-midnight-title { font-size:9px; letter-spacing:.22em; opacity:.55; margin-bottom:4px; }
.nova-seasonring text { font-size:10px; fill:currentColor; opacity:.7; pointer-events:none; }
.nova-seasonring path { cursor:pointer; transition:opacity .15s; }
.nova-seasonring path:hover { opacity:.75; }
.nova-journal-trail { font-size:11px; line-height:1.7; letter-spacing:.02em; }
.nova-journal-arrow { opacity:.45; margin:0 4px; }
`;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = css;
  document.head.appendChild(style);
}

// ---- W-064 脉搏（内存活性表 + 类名挂载约定） ----

let pulsePaths: Record<string, number> = {};

function pulseTick(): void {
  if (!flagOn("W-064")) return;
  const now = Date.now();
  const before = Object.keys(pulsePaths).length;
  pulsePaths = prunePulsePaths(pulsePaths, now);
  if (Object.keys(pulsePaths).length !== before) {
    filesEvent("pulse-state", { active: Object.keys(pulsePaths) });
  }
}

// ---- W-065 人口普查 overlay ----

let censusData: { dir: string; report: CensusReport; at: number } | null = null;

function toggleCensus(force?: boolean): void {
  const layer = novaLayer();
  if (!layer) return;
  const existing = layer.querySelector(".nova-census");
  if (existing && force !== true) {
    existing.remove();
    return;
  }
  if (existing) existing.remove();
  const panel = makeEl("nova-panel nova-census");
  panel.style.right = "16px";
  panel.style.bottom = "56px";
  panel.style.maxWidth = "360px";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "W-065 DIRECTORY CENSUS");

  const title = makeEl("nova-panel-title");
  title.textContent = "W-065 CENSUS REPORT";
  panel.appendChild(title);

  if (!censusData) {
    const empty = makeEl("nova-empty");
    empty.textContent = "NO CENSUS DATA · DISPATCH nova://files-census {dir, entries}";
    panel.appendChild(empty);
  } else {
    const { dir, report } = censusData;
    const head = makeEl("nova-journal-trail");
    head.textContent = dir;
    panel.appendChild(head);

    // 类型构成横条（饼图的条形诚实版：宽度 = 占比）
    for (const t of report.top5) {
      const row = makeEl("nova-haven-row");
      row.textContent = `${t.type.toUpperCase()} ${t.count} (${t.pct}%)`;
      row.style.setProperty("--nova-census-w", `${clamp(t.pct, 2, 100)}%`);
      row.style.borderLeft = `3px solid hsl(${(t.type.length * 47) % 360} 60% 55%)`;
      panel.appendChild(row);
    }
    const meta = makeEl("nova-haven-row");
    meta.textContent = `FILES ${report.total} · AVG AGE ${report.avgAgeDays}D · EMPTY ${report.emptyCount}${
      report.largest ? ` · MAX ${report.largest.name}` : ""
    }`;
    panel.appendChild(meta);

    const copy = document.createElement("button");
    copy.type = "button";
    copy.className = "nova-btn";
    copy.textContent = "COPY REPORT";
    copy.addEventListener("click", () => {
      const text = censusTextReport(dir, report, censusData!.at);
      void navigator.clipboard?.writeText(text).catch(() => filesEvent("census-copy-failed"));
    });
    panel.appendChild(copy);
  }
  layer.appendChild(panel);
  filesEvent("census-open", { hasData: censusData !== null });
}

function onCensusData(ev: Event): void {
  if (!flagOn("W-065")) return;
  const d = (ev as CustomEvent).detail as { dir?: string; entries?: CensusEntry[] } | undefined;
  if (!d?.dir || !Array.isArray(d.entries)) return;
  const now = Date.now();
  censusData = { dir: d.dir, report: censusReport(d.entries, now), at: now };
  if (document.querySelector(".nova-census")) toggleCensus(true);
}

// ---- W-066 借阅书架面板 ----

function shelfLoad(): BorrowRecord[] {
  return lsGet<BorrowRecord[]>(`${NS}.shelf.v1`, []);
}

function shelfSave(shelf: BorrowRecord[]): void {
  lsSet(`${NS}.shelf.v1`, shelf);
}

function toggleShelf(force?: boolean): void {
  const layer = novaLayer();
  if (!layer) return;
  const existing = layer.querySelector(".nova-shelf");
  if (existing && force !== true) {
    existing.remove();
    return;
  }
  if (existing) existing.remove();
  const now = Date.now();
  const shelf = shelfLoad();
  const overdue = new Set(overdueIds(shelf, now));
  const panel = makeEl("nova-panel nova-shelf");
  panel.style.right = "16px";
  panel.style.bottom = "56px";
  panel.style.maxWidth = "320px";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "W-066 BORROW SHELF");
  const title = makeEl("nova-panel-title");
  title.textContent = `W-066 BORROW SHELF · ${shelf.length}/${SHELF_CAPACITY}`;
  panel.appendChild(title);
  if (shelf.length === 0) {
    const empty = makeEl("nova-empty");
    empty.textContent = "SHELF EMPTY · DRAG FILES TO BORROW";
    panel.appendChild(empty);
  }
  for (const r of shelf) {
    const row = makeEl("nova-haven-row");
    if (overdue.has(r.id)) row.classList.add("nova-shelf-overdue");
    row.textContent = `${r.name} ← ${r.from}`;
    row.title = overdue.has(r.id) ? "OVERDUE (48H+)" : `BORROWED ${new Date(r.borrowedAt).toLocaleString()}`;
    const back = document.createElement("button");
    back.type = "button";
    back.className = "nova-btn";
    back.textContent = "RETURN";
    back.addEventListener("click", () => {
      const cur = shelfLoad();
      const res = returnBook(cur, r.id);
      shelfSave(res.shelf);
      if (res.record) filesEvent("return", { record: res.record, originGone: false });
      toggleShelf(true);
    });
    row.appendChild(back);
    panel.appendChild(row);
  }
  layer.appendChild(panel);
  filesEvent("shelf-open", { count: shelf.length, overdue: overdue.size });
}

function onBorrow(ev: Event): void {
  if (!flagOn("W-066")) return;
  const d = (ev as CustomEvent).detail as { path?: string; from?: string; name?: string } | undefined;
  if (!d?.path) return;
  const name = d.name ?? d.path.split(/[\\/]/).pop() ?? d.path;
  const res = borrowBook(shelfLoad(), { id: d.path, name, from: d.from ?? "" }, Date.now());
  if (res.ok) shelfSave(res.shelf);
  filesEvent("borrow-result", { ok: res.ok, reason: res.ok ? null : res.reason });
  if (document.querySelector(".nova-shelf")) toggleShelf(true);
}

// ---- W-068 增长预报面板 ----

function growthLoad(): Record<string, GrowthSample[]> {
  return lsGet<Record<string, GrowthSample[]>>(`${NS}.growth.v1`, {});
}

function toggleForecast(dir?: string): void {
  const layer = novaLayer();
  if (!layer) return;
  const existing = layer.querySelector(".nova-forecast");
  if (existing) existing.remove();
  const panel = makeEl("nova-panel nova-forecast");
  panel.style.right = "16px";
  panel.style.bottom = "56px";
  panel.style.maxWidth = "340px";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "W-068 GROWTH FORECAST");
  const title = makeEl("nova-panel-title");
  title.textContent = "W-068 GROWTH FORECAST";
  panel.appendChild(title);

  const store = growthLoad();
  const dirs = Object.keys(store);
  const target = dir ?? dirs.sort((a, b) => (store[b]!.length - store[a]!.length))[0];
  if (!target || !store[target] || store[target]!.length < 2) {
    const empty = makeEl("nova-empty");
    empty.textContent = `INSUFFICIENT DATA · NEED ≥${GROWTH_MIN_DAYS}D SPAN · DISPATCH nova://files-growth`;
    panel.appendChild(empty);
  } else {
    const samples = store[target]!;
    const f = forecastGrowth(samples);
    const body = makeEl("nova-journal-trail");
    if (f.insufficient) {
      body.textContent = `INSUFFICIENT DATA (${samples.length} SAMPLES, <${GROWTH_MIN_DAYS}D)`;
    } else {
      const lin = f.perDayLinear;
      const w = f.perDayWeighted;
      body.textContent =
        `${target}\n` +
        `LINEAR ${lin >= 0 ? "+" : ""}${(lin / INK_MEGABYTE).toFixed(2)} MB/D · ` +
        `WEIGHTED ${w >= 0 ? "+" : ""}${(w / INK_MEGABYTE).toFixed(2)} MB/D\n` +
        `+${GROWTH_HORIZON_DAYS}D → ${(f.projectedBytes / INK_GIGABYTE).toFixed(2)} GB ` +
        `±${(f.bandBytes / INK_GIGABYTE).toFixed(2)} GB`;
    }
    panel.appendChild(body);
  }
  layer.appendChild(panel);
  filesEvent("forecast-open", { dir: target ?? null });
}

function onGrowthSample(ev: Event): void {
  if (!flagOn("W-068")) return;
  const d = (ev as CustomEvent).detail as { dir?: string; bytes?: number; t?: number } | undefined;
  if (!d?.dir || typeof d.bytes !== "number") return;
  const store = growthLoad();
  const list = store[d.dir] ?? [];
  const t = d.t ?? Date.now();
  const next = [...list.filter((s) => s.t !== t), { t, bytes: d.bytes }].sort((a, b) => a.t - b.t).slice(-120);
  store[d.dir] = next;
  lsSet(`${NS}.growth.v1`, store);
}

// ---- W-069 迁途志 overlay ----

function journalLoad(): MoveEdge[] {
  return lsGet<MoveEdge[]>(`${NS}.journal.v1`, []);
}

function toggleJournal(force?: boolean): void {
  const layer = novaLayer();
  if (!layer) return;
  const existing = layer.querySelector(".nova-journal");
  if (existing && force !== true) {
    existing.remove();
    return;
  }
  if (existing) existing.remove();
  const panel = makeEl("nova-panel nova-journal");
  panel.style.right = "16px";
  panel.style.bottom = "56px";
  panel.style.maxWidth = "420px";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "W-069 MOVE JOURNAL");
  const title = makeEl("nova-panel-title");
  title.textContent = `W-069 MOVE JOURNAL · ${JOURNAL_DAYS}D ROLLING`;
  panel.appendChild(title);

  const journal = journalLoad();
  if (journal.length === 0) {
    const empty = makeEl("nova-empty");
    empty.textContent = "NO MOVES RECORDED · DISPATCH nova://files-move {from, to}";
    panel.appendChild(empty);
  } else {
    // 汇总各当前路径的完整轨迹链（最新优先展示 8 条）
    const byCur = new Map<string, MoveEdge[]>();
    for (const e of journal) byCur.set(e.to, journalTrail(journal, e.to));
    const trails = [...byCur.entries()].slice(0, 8);
    for (const [cur, trail] of trails) {
      const row = makeEl("nova-journal-trail");
      const parts: string[] = [];
      for (const e of trail) {
        parts.push(e.from);
        const arrow = document.createElement("span");
        arrow.className = "nova-journal-arrow";
        arrow.textContent = "→";
        row.appendChild(document.createTextNode(parts.pop() ?? ""));
        row.appendChild(arrow);
      }
      row.appendChild(document.createTextNode(cur));
      panel.appendChild(row);
    }
  }
  layer.appendChild(panel);
  filesEvent("journal-open", { edges: journal.length });
}

function onMove(ev: Event): void {
  if (!flagOn("W-069")) return;
  const d = (ev as CustomEvent).detail as { from?: string; to?: string } | undefined;
  if (!d?.from || !d.to) return;
  const next = recordMove(journalLoad(), d.from, d.to, Date.now());
  lsSet(`${NS}.journal.v1`, next);
  filesEvent("journal-recorded", { from: d.from, to: d.to });
  if (document.querySelector(".nova-journal")) toggleJournal(true);
}

// ---- W-070 标签代数面板 ----

let stealthMode = false;

function tagsLoad(): Record<string, string[]> {
  return lsGet<Record<string, string[]>>(`${NS}.tags.v1`, {});
}

function toggleAlgebra(force?: boolean): void {
  const layer = novaLayer();
  if (!layer) return;
  const existing = layer.querySelector(".nova-algebra");
  if (existing && force !== true) {
    existing.remove();
    return;
  }
  if (existing) existing.remove();
  const panel = makeEl("nova-panel nova-algebra");
  panel.style.right = "16px";
  panel.style.bottom = "56px";
  panel.style.maxWidth = "380px";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "W-070 TAG ALGEBRA");
  const title = makeEl("nova-panel-title");
  title.textContent = "W-070 TAG ALGEBRA · ∩ ∪ - ( )";
  panel.appendChild(title);

  const input = document.createElement("input");
  input.type = "text";
  input.spellcheck = false;
  input.placeholder = "工作 ∩ 未完成 - 已归档";
  input.style.cssText =
    "width:100%;box-sizing:border-box;margin:4px 0;padding:4px 6px;font:12px/1.4 Consolas,monospace;color:inherit;background:rgba(127,127,127,.1);border:1px solid rgba(127,127,127,.3);border-radius:4px;";
  const status = makeEl("nova-haven-row");
  status.textContent = "TYPE AN EXPRESSION";
  const result = makeEl("nova-haven-row");

  const evaluate = (): void => {
    const expr = input.value;
    const parsed = parseAlgebra(expr);
    if (!parsed.ok) {
      status.textContent = `✕ @${parsed.errorAt} ${parsed.message}`;
      input.style.borderColor = "rgba(255,96,96,.9)";
      result.textContent = "";
      return;
    }
    input.style.borderColor = "";
    const paths = evalAlgebra(parsed.ast, tagsLoad());
    status.textContent = `✓ VALID · ${paths.length} PATH(S)`;
    result.textContent = paths.slice(0, 5).join("\n") || "(EMPTY SET)";
  };
  input.addEventListener("input", evaluate);

  const save = document.createElement("button");
  save.type = "button";
  save.className = "nova-btn";
  save.textContent = "SAVE AS FOLDER";
  save.addEventListener("click", () => {
    const parsed = parseAlgebra(input.value);
    if (!parsed.ok) return;
    const name = `ALGEBRA ${new Date().toLocaleDateString()}`;
    const list = upsertAlgebraFolder(lsGet<AlgebraFolder[]>(`${NS}.algebra.v1`, []), name, input.value, Date.now());
    lsSet(`${NS}.algebra.v1`, list);
    filesEvent("algebra-saved", { name, expr: input.value });
    status.textContent = `SAVED "${name}"`;
  });

  panel.append(input, status, result, save);
  layer.appendChild(panel);
  filesEvent("algebra-open", {});
}

function onTags(ev: Event): void {
  if (!flagOn("W-070")) return;
  const d = (ev as CustomEvent).detail as { tag?: string; paths?: string[] } | undefined;
  if (!d?.tag || !Array.isArray(d.paths)) return;
  const tags = tagsLoad();
  const merged = new Set(tags[d.tag] ?? []);
  for (const p of d.paths) merged.add(p);
  tags[d.tag] = [...merged].slice(-500);
  lsSet(`${NS}.tags.v1`, tags);
}

function onStealth(ev: Event): void {
  const d = (ev as CustomEvent).detail as { on?: boolean } | undefined;
  stealthMode = d?.on === true;
  filesEvent("stealth-state", { on: stealthMode });
}

// ---- W-071 幽灵页挂载（观察 [data-nova-preview] 容器） ----

let ghostObserver: MutationObserver | null = null;

function mountGhostPage(container: HTMLElement): void {
  const kind = (container.dataset.novaPreview ?? "doc") as PreviewKind;
  const pages = Number(container.dataset.pages ?? "0");
  if (!ghostApplies(kind) || !ghostPageVisible(pages)) {
    container.querySelector(".nova-ghost-page")?.remove();
    return;
  }
  let ghost = container.querySelector<HTMLElement>(".nova-ghost-page");
  if (!ghost) {
    ghost = makeEl("nova-ghost-page");
    ghost.textContent = "GHOST PAGE · END OF DOCUMENT";
    container.appendChild(ghost);
  }
  ghost.style.opacity = String(GHOST_OPACITY); // 12% 幽灵语义（诚实按规格）
}

function ghostScan(): void {
  if (!flagOn("W-071")) return;
  document.querySelectorAll<HTMLElement>("[data-nova-preview]").forEach((c) => mountGhostPage(c));
}

function ghostEnsureObserver(): void {
  if (ghostObserver || typeof MutationObserver === "undefined") return;
  let queued = false;
  ghostObserver = new MutationObserver(() => {
    if (queued) return;
    queued = true;
    queueMicrotask(() => {
      queued = false;
      ghostScan();
    });
  });
  ghostObserver.observe(document.body, { childList: true, subtree: true });
}

// ---- W-072 传输律动声 ----

type AudioCtor = new () => AudioContext;

let audioCtx: AudioContext | null = null;
const grooveTimers = new Map<string, number>();

function audioCtor(): AudioCtor | null {
  if (typeof window === "undefined") return null;
  const w = window as unknown as { AudioContext?: AudioCtor; webkitAudioContext?: AudioCtor };
  return w.AudioContext ?? w.webkitAudioContext ?? null;
}

/** 惰性创建 AudioContext（无音频环境/构造失败 → 如实返回 false 跳过）。 */
function ensureAudio(): boolean {
  if (audioCtx) return true;
  const Ctor = audioCtor();
  if (!Ctor) return false;
  try {
    audioCtx = new Ctor();
    return true;
  } catch {
    return false;
  }
}

function grooveTone(freq: number, durMs: number, whenMs = 0, gain = 0.05): void {
  if (!audioCtx) return;
  try {
    const t0 = audioCtx.currentTime + whenMs / 1000;
    const osc = audioCtx.createOscillator();
    const g = audioCtx.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.exponentialRampToValueAtTime(gain, t0 + 0.02);
    g.gain.exponentialRampToValueAtTime(0.0001, t0 + durMs / 1000);
    osc.connect(g);
    g.connect(audioCtx.destination);
    osc.start(t0);
    osc.stop(t0 + durMs / 1000 + 0.05);
  } catch {
    /* 音频失败静默（诚实跳过） */
  }
}

function onTransfer(ev: Event): void {
  if (!flagOn("W-072") || stealthMode) return;
  const d = (ev as CustomEvent).detail as
    | { id?: string; total?: number; rate?: number; state?: "active" | "done" | "failed" }
    | undefined;
  if (!d?.id) return;
  const minBytes = Math.max(0, num("W-072", "minMB")) * INK_MEGABYTE || GROOVE_MIN_BYTES;
  if (!grooveEligible(d.total ?? 0, minBytes)) return;
  if (!ensureAudio()) return; // 无音频环境如实跳过

  const stopTimer = (): void => {
    const t = grooveTimers.get(d.id!);
    if (t !== undefined) {
      clearInterval(t);
      grooveTimers.delete(d.id!);
    }
  };

  if (d.state === "done" || d.state === "failed") {
    stopTimer();
    grooveChord(d.state).forEach((f, i) => grooveTone(f, d.state === "done" ? 420 : 300, i * 90));
    return;
  }
  if (d.state !== "active") return;

  // 活性：按真实速率重排节拍（单调映射）
  const interval = beatIntervalMs(d.rate ?? 0);
  stopTimer();
  if (typeof window === "undefined") return;
  grooveTimers.set(
    d.id,
    window.setInterval(() => grooveTone(330, 90, 0, 0.035), interval),
  );
}

// ---- W-073 子夜更钟 ----

interface DayStore {
  dayKey: string;
  tally: DayTally;
  /** 勿扰延期的明晨首现时刻。 */
  pendingShowAt: number | null;
}

function dayLoad(): DayStore {
  return lsGet<DayStore>(`${NS}.day.v1`, {
    dayKey: dayKeyOf(Date.now()),
    tally: { added: 0, removed: 0, bytesAdded: 0, bytesRemoved: 0 },
    pendingShowAt: null,
  });
}

function showMidnightCard(delta: MidnightDelta): void {
  const layer = novaLayer();
  if (!layer) return;
  document.querySelector(".nova-midnight")?.remove();
  const card = makeEl("nova-midnight");
  card.setAttribute("role", "status");
  card.setAttribute("aria-label", "W-073 MIDNIGHT DELTA");
  const title = makeEl("nova-midnight-title");
  title.textContent = "W-073 MIDNIGHT DELTA · DAILY TIDE";
  const body = document.createElement("div");
  const sign = (v: number): string => (v >= 0 ? "+" : "");
  body.textContent =
    `+${delta.addedFiles} / -${delta.removedFiles} FILES · ` +
    `${sign(delta.netBytes)}${(delta.netBytes / INK_MEGABYTE).toFixed(1)} MB NET`;
  card.append(title, body);
  layer.appendChild(card);
  if (typeof window !== "undefined") {
    window.setTimeout(() => card.remove(), 3000);
  }
}

function midnightTick(): void {
  if (!flagOn("W-073")) return;
  const now = Date.now();
  const store = dayLoad();
  const zero: DayTally = { added: 0, removed: 0, bytesAdded: 0, bytesRemoved: 0 };
  // 勿扰延期 → 明晨首现（展示后才推进日键，避免重复结算）
  if (store.pendingShowAt !== null) {
    if (now >= store.pendingShowAt) {
      showMidnightCard(settleMidnight(store.tally));
      store.pendingShowAt = null;
      store.dayKey = dayKeyOf(now);
      store.tally = zero;
      lsSet(`${NS}.day.v1`, store);
    }
    return;
  }
  // 跨结算点（日键变化）→ 结算昨日潮汐
  const key = dayKeyOf(now);
  if (key !== store.dayKey) {
    const delta = settleMidnight(store.tally);
    if (inDndHours(now)) {
      store.pendingShowAt = nextMorningAt(now);
    } else {
      showMidnightCard(delta);
      store.dayKey = key;
      store.tally = zero;
    }
    lsSet(`${NS}.day.v1`, store);
  }
}

function onDelta(ev: Event): void {
  if (!flagOn("W-073")) return;
  const d = (ev as CustomEvent).detail as Partial<DayTally> | undefined;
  if (!d) return;
  const store = dayLoad();
  store.tally = {
    added: store.tally.added + (d.added ?? 0),
    removed: store.tally.removed + (d.removed ?? 0),
    bytesAdded: store.tally.bytesAdded + (d.bytesAdded ?? 0),
    bytesRemoved: store.tally.bytesRemoved + (d.bytesRemoved ?? 0),
  };
  lsSet(`${NS}.day.v1`, store);
}

// ---- W-074 季节环 overlay ----

function seasonLoad(): Record<string, { months: number[]; at: number }> {
  return lsGet<Record<string, { months: number[]; at: number }>>(`${NS}.season.v1`, {});
}

function toggleSeasonRing(force?: boolean): void {
  const layer = novaLayer();
  if (!layer) return;
  const existing = layer.querySelector(".nova-seasonring");
  if (existing && force !== true) {
    existing.remove();
    return;
  }
  if (existing) existing.remove();
  const panel = makeEl("nova-panel nova-seasonring");
  panel.style.right = "16px";
  panel.style.bottom = "56px";
  panel.style.width = "280px";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "W-074 SEASON RING");
  const title = makeEl("nova-panel-title");
  title.textContent = "W-074 SEASON RING · CREATED BY MONTH";
  panel.appendChild(title);

  const store = seasonLoad();
  const dirs = Object.keys(store).sort((a, b) => store[b]!.at - store[a]!.at);
  const target = dirs[0];
  if (!target) {
    const empty = makeEl("nova-empty");
    empty.textContent = "NO SEASON DATA · DISPATCH nova://files-season {dir, createdAts}";
    panel.appendChild(empty);
  } else {
    const months = store[target]!.months;
    const total = ringTotal(months);
    const svgNS = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(svgNS, "svg");
    svg.setAttribute("viewBox", "0 0 200 200");
    svg.setAttribute("width", "200");
    svg.setAttribute("height", "200");
    const cx = 100;
    const cy = 100;
    const r = 70;
    const stroke = 22;
    if (total === 0) {
      const empty = makeEl("nova-empty");
      empty.textContent = "EMPTY DIRECTORY";
      panel.appendChild(empty);
    } else {
      const sectors = sectorAngles(months);
      const circ = 2 * Math.PI * r;
      for (const s of sectors) {
        if (s.sweepDeg <= 0) continue;
        const path = document.createElementNS(svgNS, "path");
        path.setAttribute("d", describeArc(cx, cy, r, s.startDeg, s.startDeg + s.sweepDeg));
        path.setAttribute("fill", "none");
        path.setAttribute("stroke", `hsl(${(s.month * 30 + 200) % 360} 62% 55%)`);
        path.setAttribute("stroke-width", String(stroke));
        const len = (s.sweepDeg / 360) * circ;
        path.setAttribute("stroke-dasharray", `${len} ${circ - len}`);
        path.setAttribute("transform", `rotate(90 ${cx} ${cy})`);
        path.addEventListener("click", () => filesEvent("season-filter", { dir: target, month: s.month }));
        svg.appendChild(path);
      }
      const label = document.createElementNS(svgNS, "text");
      label.setAttribute("x", String(cx));
      label.setAttribute("y", String(cy + 4));
      label.setAttribute("text-anchor", "middle");
      label.textContent = `${total} FILES`;
      svg.appendChild(label);
      panel.appendChild(svg);
      const hint = makeEl("nova-haven-row");
      hint.textContent = `${target} · CLICK SECTOR TO FILTER`;
      panel.appendChild(hint);
    }
  }
  layer.appendChild(panel);
  filesEvent("season-open", { dir: target ?? null });
}

/** 极角→SVG 弧路径（0° = 正上方，屏幕坐标顺时针）。 */
function describeArc(cx: number, cy: number, r: number, startDeg: number, endDeg: number): string {
  const pol = (deg: number): [number, number] => {
    const rad = (deg * Math.PI) / 180;
    return [cx + r * Math.cos(rad), cy + r * Math.sin(rad)];
  };
  const [x0, y0] = pol(startDeg);
  const [x1, y1] = pol(endDeg);
  const large = endDeg - startDeg > 180 ? 1 : 0;
  return `M ${x0.toFixed(2)} ${y0.toFixed(2)} A ${r} ${r} 0 ${large} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
}

function onSeason(ev: Event): void {
  if (!flagOn("W-074")) return;
  const d = (ev as CustomEvent).detail as { dir?: string; createdAts?: number[] } | undefined;
  if (!d?.dir || !Array.isArray(d.createdAts)) return;
  const store = seasonLoad();
  store[d.dir] = { months: ringMonths(d.createdAts), at: Date.now() };
  lsSet(`${NS}.season.v1`, store);
  if (document.querySelector(".nova-seasonring")) toggleSeasonRing(true);
}

// ---- W-075 出身簿面板 ----

function binLoad(): BinRecord[] {
  return lsGet<BinRecord[]>(`${NS}.bin.v1`, []);
}

function toggleBinBook(force?: boolean): void {
  const layer = novaLayer();
  if (!layer) return;
  const existing = layer.querySelector(".nova-binbook");
  if (existing && force !== true) {
    existing.remove();
    return;
  }
  if (existing) existing.remove();
  const panel = makeEl("nova-panel nova-binbook");
  panel.style.right = "16px";
  panel.style.bottom = "56px";
  panel.style.maxWidth = "360px";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", "W-075 BIN PROVENANCE");
  const title = makeEl("nova-panel-title");
  title.textContent = `W-075 BIN PROVENANCE · ${binLoad().length}/${BIN_MAX}`;
  panel.appendChild(title);

  const records = binLoad();
  if (records.length === 0) {
    const empty = makeEl("nova-empty");
    empty.textContent = "BIN EMPTY · DISPATCH nova://files-trashed {id, name, by, origin}";
    panel.appendChild(empty);
  }
  for (const r of records.slice(0, 10)) {
    const row = makeEl("nova-haven-row");
    row.textContent = `[${byLabel(r)}] ${r.name} ← ${r.origin}`;
    row.title = `TRASHED ${new Date(r.at).toLocaleString()}`;
    panel.appendChild(row);
  }
  layer.appendChild(panel);
  filesEvent("bin-open", { count: records.length });
}

function onTrashed(ev: Event): void {
  if (!flagOn("W-075")) return;
  const d = (ev as CustomEvent).detail as { id?: string; name?: string; by?: string; origin?: string } | undefined;
  if (!d?.id) return;
  const next = recordBin(binLoad(), {
    id: d.id,
    name: d.name ?? d.id,
    by: d.by ?? "",
    origin: d.origin ?? "",
    at: Date.now(),
  });
  lsSet(`${NS}.bin.v1`, next);
  if (document.querySelector(".nova-binbook")) toggleBinBook(true);
}

// ---- W-076 近踪动港浮列 ----

function havenLoad(): HavenEntry[] {
  return lsGet<HavenEntry[]>(`${NS}.haven.v1`, []);
}

function renderHaven(): void {
  if (!flagOn("W-076")) return;
  const layer = novaLayer();
  if (!layer) return;
  document.querySelector(".nova-haven")?.remove();
  const list = havenLoad();
  if (list.length === 0 || stealthMode) return;
  const box = makeEl("nova-haven");
  box.setAttribute("role", "navigation");
  box.setAttribute("aria-label", "W-076 RECENT HAVEN");
  const title = makeEl("nova-haven-title");
  title.textContent = `RECENT HAVEN · TOP ${HAVEN_CAP}`;
  box.appendChild(title);
  for (const e of list) {
    const row = makeEl("nova-haven-row");
    row.textContent = `${e.count}× ${e.path}`;
    row.title = `LAST ${new Date(e.lastAt).toLocaleString()} · CLICK TO REOPEN`;
    row.addEventListener("click", () => filesEvent("haven-open", { path: e.path }));
    box.appendChild(row);
  }
  layer.appendChild(box);
}

function onVisit(ev: Event): void {
  if (!flagOn("W-076") || stealthMode) return; // 隐身会话不记录（诚实边界）
  const d = (ev as CustomEvent).detail as { path?: string } | undefined;
  if (!d?.path) return;
  const next = havenVisit(havenLoad(), d.path, Date.now());
  lsSet(`${NS}.haven.v1`, next);
  renderHaven();
}

// ---- W-067 墨阶扫描（行补 data-size 即自动套用） ----

function inkScan(): void {
  if (!flagOn("W-067")) return;
  document.querySelectorAll<HTMLElement>("[data-size]").forEach((el) => {
    const size = Number(el.dataset.size ?? "0");
    const tier = inkTier(size);
    el.classList.toggle("nova-ink-1", tier.tier === 1);
    el.classList.toggle("nova-ink-2", tier.tier === 2);
    el.classList.toggle("nova-ink-3", tier.megalith);
  });
}

// ---- 激活 / 卸载 ----

let pulseTimer = 0;
let midnightTimer = 0;
let inkTimer = 0;

/** S0 注册表订阅（registry.ts subscribeNova；独立 shim 便于测试环境容错）。 */
function subscribeNovaShim(): () => void {
  try {
    return subscribeNovaImpl();
  } catch {
    return () => {};
  }
}

let subscribeNovaImpl: () => () => void = () => () => {};
export function __bindRegistrySubscribe(fn: () => () => void): void {
  subscribeNovaImpl = fn;
}

function onRegistryChange(): void {
  if (!active) return;
  if (!flagOn("W-076")) document.querySelector(".nova-haven")?.remove();
}

export function activateFilesNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  ensureStyle();

  // W-064 脉搏摄入 + 巡检
  const onWrite = (ev: Event): void => {
    if (!flagOn("W-064")) return;
    const d = (ev as CustomEvent).detail as { path?: string } | undefined;
    if (d?.path) pulsePaths[d.path] = Date.now();
  };
  window.addEventListener("nova://files-write", onWrite);
  bag.push(() => window.removeEventListener("nova://files-write", onWrite));
  pulseTimer = window.setInterval(pulseTick, 1000);
  bag.push(() => clearInterval(pulseTimer));

  // W-065 普查数据 + overlay
  window.addEventListener("nova://files-census", onCensusData);
  bag.push(() => window.removeEventListener("nova://files-census", onCensusData));

  // W-066 借阅
  window.addEventListener("nova://files-borrow", onBorrow);
  bag.push(() => window.removeEventListener("nova://files-borrow", onBorrow));
  const onShelfToggle = (): void => toggleShelf();
  window.addEventListener("nova://files-shelf-toggle", onShelfToggle);
  bag.push(() => window.removeEventListener("nova://files-shelf-toggle", onShelfToggle));

  // W-068 增长采样 + 面板事件入口
  window.addEventListener("nova://files-growth", onGrowthSample);
  bag.push(() => window.removeEventListener("nova://files-growth", onGrowthSample));
  const onForecast = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { dir?: string } | undefined;
    toggleForecast(d?.dir);
  };
  window.addEventListener("nova://files-forecast", onForecast);
  bag.push(() => window.removeEventListener("nova://files-forecast", onForecast));

  // W-069 迁途
  window.addEventListener("nova://files-move", onMove);
  bag.push(() => window.removeEventListener("nova://files-move", onMove));

  // W-070 标签索引 + 代数面板
  window.addEventListener("nova://files-tags", onTags);
  bag.push(() => window.removeEventListener("nova://files-tags", onTags));
  const onAlgebraToggle = (): void => toggleAlgebra();
  window.addEventListener("nova://files-algebra-toggle", onAlgebraToggle);
  bag.push(() => window.removeEventListener("nova://files-algebra-toggle", onAlgebraToggle));

  // 隐身会话开关（W-072/076 尊重）
  window.addEventListener("nova://files-stealth", onStealth);
  bag.push(() => window.removeEventListener("nova://files-stealth", onStealth));

  // W-072 传输律动
  window.addEventListener("nova://files-transfer", onTransfer);
  bag.push(() => window.removeEventListener("nova://files-transfer", onTransfer));

  // W-073 更钟
  window.addEventListener("nova://files-delta", onDelta);
  bag.push(() => window.removeEventListener("nova://files-delta", onDelta));
  midnightTimer = window.setInterval(midnightTick, 30_000);
  bag.push(() => clearInterval(midnightTimer));

  // W-074 季节环
  window.addEventListener("nova://files-season", onSeason);
  bag.push(() => window.removeEventListener("nova://files-season", onSeason));

  // W-075 出身簿
  window.addEventListener("nova://files-trashed", onTrashed);
  bag.push(() => window.removeEventListener("nova://files-trashed", onTrashed));
  const onBinToggle = (): void => toggleBinBook();
  window.addEventListener("nova://files-bin-toggle", onBinToggle);
  bag.push(() => window.removeEventListener("nova://files-bin-toggle", onBinToggle));

  // W-076 近踪
  window.addEventListener("nova://files-visit", onVisit);
  bag.push(() => window.removeEventListener("nova://files-visit", onVisit));

  // W-067 墨阶扫描（低频；行补 data-size 即生效）
  inkTimer = window.setInterval(inkScan, 1600);
  bag.push(() => clearInterval(inkTimer));
  inkScan();

  // W-071 幽灵页观察器
  ghostEnsureObserver();
  ghostScan();

  // Hub overlay 直达（ai04 协议：nova-census / nova-journal / nova-seasonring）
  const onOpenFeature = (ev: Event): void => {
    const f = (ev as CustomEvent).detail?.feature;
    if (f === "nova-census" && flagOn("W-065")) toggleCensus();
    if (f === "nova-journal" && flagOn("W-069")) toggleJournal();
    if (f === "nova-seasonring" && flagOn("W-074")) toggleSeasonRing();
  };
  window.addEventListener("ai04:open-feature", onOpenFeature);
  bag.push(() => window.removeEventListener("ai04:open-feature", onOpenFeature));

  // S0 注册表变化 → 即时生效/失效
  bag.push(subscribeNovaShim());
}

export function deactivateFilesNova(): void {
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
  ghostObserver?.disconnect();
  ghostObserver = null;
  for (const t of grooveTimers.values()) clearInterval(t);
  grooveTimers.clear();
  audioCtx?.close().catch(() => undefined);
  audioCtx = null;
  pulsePaths = {};
  document
    .querySelectorAll(
      ".nova-census,.nova-shelf,.nova-forecast,.nova-journal,.nova-algebra," +
        ".nova-midnight,.nova-seasonring,.nova-binbook,.nova-haven,.nova-ghost-page",
    )
    .forEach((e) => e.remove());
}

export function isFilesNovaActive(): boolean {
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
