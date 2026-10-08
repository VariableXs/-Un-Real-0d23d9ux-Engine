/**
 * NOVA-200 · S9 兼容诊疗路（AI-09）—— 域9 兼容性防线（W-102…W-113）。
 *
 * 边界（全景 §9）：Z-15 适配矩阵管**预先沉淀**的等级库（W-102 是装机后实测观察期）、
 * M-44 嵌入崩溃善后管**崩溃后**处理（W-103 是事前预言）、Z-18 全屏协议管协议避让
 * （W-104 是卡死后逃生）、Q-61 IME 缓冲管切换瞬间（W-105 管运行中组段被吞）、
 * Z-03 字体链管环境字体（W-106 管用户文档替身）、Z-19 混合 DPI 管适配（W-107 管
 * 转场瞬间防腐）、Z-16 遗留协议 Shim 管协议兼容（W-108 是视觉/缩放诊疗）、
 * Z-18 管全屏避让（W-109 是反作弊驱动专项深度礼让）、U-23 崩溃叙事管环境自身
 * 崩溃（W-110 是系统级蓝屏检尸）、Q-59/Q-81 管认知提示与仪式（W-111 管色彩
 * 连续性技术影）、M-54 管全局资源调度（W-112 是同名实例 GPU 争抢建议仲裁）、
 * Z-61 增量更新管环境自身更新（W-113 管系统更新的前置防护）。
 *
 * 纪律：
 * - 零侵入：不改写 Explorer/嵌入/回收站/ singularity 任何内部逻辑；全部为 DOM 叠层 +
 *   CSS 类名挂载 + `nova://compat-*` 自定义事件摄入（进程/DPI/色彩空间侧接线由
 *   S17 在既有文件按 wiringHint 补齐，本模块 API/事件契约已备好）；
 * - 前缀：类名 `nova-`、事件 `nova://compat-*`、localStorage 键 `nova.compat.*`；
 * - 开关：只读消费 S0 注册表（novaOn/novaNum），无注册表时用 defaultOn 回退；
 * - 降级：reduce-motion / safeMode / static 下转影/揭幕/横幅动效归零（语义保留）；
 * - 诚实：DLL 库未收录如实 UNKNOWN、minidump 解析不了如实 INSUFFICIENT DATA、
 *   仲裁动作用户未确认绝不执行、礼让矩阵如实声明能力组、处方未收录如实无处方。
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

const NS = "nova.compat";

/** 功能开关：只读消费 S0 注册表（nova.registry.v1 单一事实源）。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

function num(id: string, key: string): number {
  return novaNum(id, key);
}

/** 派发 `nova://compat-*` 事件（SSR/测试环境安全）。 */
export function compatEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://compat-${name}`, { detail }));
}

const HOUR = 3_600_000;
const MINUTE = 60_000;

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

export const COMPAT_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-102",
    titleZh: "应用观察期",
    titleEn: "App Watch",
    descZh: "新应用前 48h 为观察期：启动/崩溃/无响应自动记入观察簿，期满出三级体检小结（稳定/偶发/高危）；高危建议沙盒运行；可提前终结观察。",
    defaultOn: true,
    overlay: "nova-watch",
    wiringHint: "应用启动/崩溃/无响应时派发 nova://compat-watch {app, kind}；观察簿面板已备好",
    degrade: "静态观察簿与小结（无动效）；数据仅本地",
  },
  {
    id: "W-103",
    titleZh: "DLL 预言者",
    titleEn: "DLL Prophet",
    descZh: "启动前 200ms 快扫导入表：常见 DLL 本地内置库比对，缺失先弹预言卡（缺什么/从哪来/怎么补）再启动——把崩溃变成预告；未收录条目如实 UNKNOWN。",
    defaultOn: true,
    wiringHint: "启动器派发 nova://compat-imports {app, dlls, missing}；预言卡已备好",
    degrade: "静态预言卡；未知 DLL 不预言（诚实边界）",
  },
  {
    id: "W-104",
    titleZh: "全屏逃生演练",
    titleEn: "Escape Drill",
    descZh: "全屏应用无响应 >10s 浮现逃生门微标：三档安全序列（优雅退出→降级窗口化→仅结束该进程）逐档兜底并留痕；系统进程永不第三档。",
    defaultOn: true,
    wiringHint: "全屏心跳派发 nova://compat-escape {pid, name, system, hangMs}；序列执行需 S17 调进程 API（事件已备好）",
    degrade: "静态微标；序列逐档执行不合并",
  },
  {
    id: "W-105",
    titleZh: "输入组段接力",
    titleEn: "IME Relay",
    descZh: "检测组段被吞特征（组段开启后未收尾且字符键直接落盘）并自动接力重放——白名单制，仅对已登记问题应用生效，启发式可解释。",
    defaultOn: true,
    wiringHint: "输入管线派发 nova://compat-ime {app, type, key}；白名单在观察簿内登记",
    degrade: "静态接力（无动画）；白名单外零干预",
  },
  {
    id: "W-106",
    titleZh: "字体替身",
    titleEn: "Font Understudy",
    descZh: "打开文档遇缺失字体：按 metric 相似度选本机最接近替身并角落浮标「替身演出中」；一键查看还原建议；打印/导出如实用替身不虚标。",
    defaultOn: true,
    wiringHint: "文档打开派发 nova://compat-font {missing, metric, candidates}；浮标与建议卡已备好",
    degrade: "静态浮标；相似度 Top1 确定性选择",
  },
  {
    id: "W-107",
    titleZh: "DPI 转场防腐",
    titleEn: "DPI Morph",
    descZh: "DPI 变更瞬间以冻结遮罩盖窗 300ms（可调 100–600ms）防模糊闪烁，重绘完成后真身揭幕——转场「防腐层」，不增加变更耗时。",
    defaultOn: true,
    params: [{ key: "freezeMs", labelKey: "novaP_freezeMs", type: "slider", default: 300, min: 100, max: 600, step: 100 }] as never,
    wiringHint: "DPI 变更派发 nova://compat-dpi {scale}；冻结截图需 S17 接窗口截图（遮罩已备好）",
    degrade: "reduce-motion 下遮罩直切（无渐变揭幕），防腐语义保留",
  },
  {
    id: "W-108",
    titleZh: "遗留应用医院",
    titleEn: "Legacy Hospital",
    descZh: "Win7 时代遗留应用专属医院：三查（DPI 模糊/字体发虚/缩放错位）→ 三治（高 DPI 虚拟化/禁用缩放/兼容垫片）→ 复查归档；内置 20+ 处方，治疗可逐一撤销。",
    defaultOn: true,
    overlay: "nova-hospital",
    wiringHint: "遗留应用启动派发 nova://compat-legacy {exe}；诊疗面板已备好",
    degrade: "静态诊疗流程；处方未收录如实显示无处方",
  },
  {
    id: "W-109",
    titleZh: "游戏反作弊礼让",
    titleEn: "Anti-Cheat Yield",
    descZh: "检测到反作弊驱动/进程（EAC/BattlEye/Vanguard/ACE/TP 特征表）：环境全面礼让——挂起钩子/悬浮层/截图直至游戏退出，退出 30s 后恢复；礼让矩阵 Q-98 可查。",
    defaultOn: true,
    wiringHint: "进程哨兵派发 nova://compat-anticheat {active, sig}；能力组挂起经 nova://compat-yield 广播",
    degrade: "礼让横幅静态（无呼吸）；礼让语义与恢复计时不变",
  },
  {
    id: "W-110",
    titleZh: "蓝屏检尸官",
    titleEn: "BSOD Forensics",
    descZh: "蓝屏重启后首启读取 minidump：本地解析出中立检尸卡（bugcheck 代码 + 肇事模块 + 发生前 5 分钟环境侧事件 + 是否与环境有关的诚实结论）；解析不了如实 INSUFFICIENT DATA。",
    defaultOn: true,
    overlay: "nova-forensics",
    wiringHint: "首启派发 nova://compat-minidump {bytes, at}；检尸卡面板已备好（解析全程本地）",
    degrade: "静态检尸卡；不可解析绝不硬猜（中立词表措辞）",
  },
  {
    id: "W-111",
    titleZh: "色彩模式转影",
    titleEn: "Color Morph",
    descZh: "HDR↔SDR 或深浅模式切换叠加 2s 色彩插值转影（旧空间→新空间连续渐变，零白闪门禁内置）；reduce-motion 直接切换。",
    defaultOn: true,
    wiringHint: "色彩模式变更派发 nova://compat-colormode {from:[r,g,b], to:[r,g,b]}；转影层已备好",
    degrade: "reduce-motion 下 0ms 直切；转影期间零白闪（路径检查内置）",
  },
  {
    id: "W-112",
    titleZh: "同名实例仲裁",
    titleEn: "GPU Arbitration",
    descZh: "同名应用双实例争抢 GPU（2s 确认窗防误报）：弹仲裁卡建议限帧 30fps 或分核亲和——用户一键确认才执行，可设永久策略。",
    defaultOn: true,
    wiringHint: "GPU 采样派发 nova://compat-gpu {name, instances:[{pid, gpuPct}]}；执行需 S17 进程 API",
    degrade: "静态仲裁卡；未确认零动作（诚实边界）",
  },
  {
    id: "W-113",
    titleZh: "系统更新护城河",
    titleEn: "Update Moat",
    descZh: "检测系统大版本更新待重启：列出更新后可能失效的环境设置（已知兼容矩阵）+ 一键备份（快照+校验和）+ 回滚指引；无更新时零打扰。",
    defaultOn: true,
    wiringHint: "系统更新状态派发 nova://compat-osupdate {pending, build}；备份/还原走本模块 API",
    degrade: "静态护城河卡；备份本地快照可还原",
  },
];

export const compatNovaDomain = {
  id: "S9",
  nameZh: "兼容诊疗",
  nameEn: "Compat Medicine",
  route: "AI-09",
  features: COMPAT_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

// ---- W-102 应用观察期 ----

export const WATCH_WINDOW_MS = 48 * HOUR;

export type WatchKind = "launch" | "crash" | "hang";
export type WatchGrade = "stable" | "occasional" | "highrisk";

export interface WatchEntry {
  kind: WatchKind;
  at: number;
}

export interface WatchSummary {
  app: string;
  /** 观察是否已满 48h（false = 仍在期内）。 */
  complete: boolean;
  crashes: number;
  hangs: number;
  launches: number;
  grade: WatchGrade;
  /** 高危 → 建议沙盒运行。 */
  recommendSandbox: boolean;
}

/** 观察期仍在窗口内（48h）。 */
export function watchOpen(firstSeenAt: number, now: number): boolean {
  return now - firstSeenAt < WATCH_WINDOW_MS;
}

/**
 * 三级体检小结（评级与实测事件一致）：
 * - crash ≥ 3 或 hang ≥ 5 → 高危；
 * - crash ≥ 1 或 hang ≥ 2 → 偶发；
 * - 其余 → 稳定。
 */
export function watchGradeOf(crashes: number, hangs: number): WatchGrade {
  if (crashes >= 3 || hangs >= 5) return "highrisk";
  if (crashes >= 1 || hangs >= 2) return "occasional";
  return "stable";
}

/** 汇总观察簿（窗口外事件不计入评级——观察期语义）。 */
export function watchSummary(app: string, events: WatchEntry[], now: number, firstSeenAt: number): WatchSummary {
  const inWindow = events.filter((e) => e.at >= firstSeenAt && e.at - firstSeenAt < WATCH_WINDOW_MS);
  const crashes = inWindow.filter((e) => e.kind === "crash").length;
  const hangs = inWindow.filter((e) => e.kind === "hang").length;
  const launches = inWindow.filter((e) => e.kind === "launch").length;
  const grade = watchGradeOf(crashes, hangs);
  return {
    app,
    complete: !watchOpen(firstSeenAt, now),
    crashes,
    hangs,
    launches,
    grade,
    recommendSandbox: grade === "highrisk",
  };
}

export const WATCH_LOG_MAX = 200;

/** 观察簿裁剪：每应用至多 200 条（内存态防膨胀）。 */
export function pruneWatchLog(events: WatchEntry[], max = WATCH_LOG_MAX): WatchEntry[] {
  return events.slice(-max);
}

const WATCH_GRADE_ZH: Record<WatchGrade, string> = { stable: "稳定", occasional: "偶发", highrisk: "高危" };

/** 一行小结文本（观察簿/面板复用）。 */
export function watchSummaryText(s: WatchSummary): string {
  const tail = s.recommendSandbox ? " · 建议沙盒运行" : "";
  return `${s.app} · ${WATCH_GRADE_ZH[s.grade]} · 崩溃 ${s.crashes} / 无响应 ${s.hangs} / 启动 ${s.launches}${tail}`;
}

// ---- W-103 DLL 预言者 ----

export type DllOrigin = "system" | "redist" | "app";
export type DllSeverity = "blocker" | "degraded";

export interface DllKnowledge {
  name: string;
  origin: DllOrigin;
  severity: DllSeverity;
  /** 一行补齐指引（预言卡「怎么补」）。 */
  fix: string;
}

/** 常见 DLL 本地内置库（大小写不敏感比对；纯本地，零联网）。 */
export const DLL_KNOWLEDGE: readonly DllKnowledge[] = [
  { name: "msvcp140.dll", origin: "redist", severity: "blocker", fix: "安装 Visual C++ 2015-2022 运行库 (x86/x64)" },
  { name: "vcruntime140.dll", origin: "redist", severity: "blocker", fix: "安装 Visual C++ 2015-2022 运行库 (x86/x64)" },
  { name: "vcruntime140_1.dll", origin: "redist", severity: "blocker", fix: "安装 Visual C++ 2015-2022 运行库 (x64)" },
  { name: "msvcr120.dll", origin: "redist", severity: "blocker", fix: "安装 Visual C++ 2013 运行库" },
  { name: "msvcp120.dll", origin: "redist", severity: "blocker", fix: "安装 Visual C++ 2013 运行库" },
  { name: "msvcr110.dll", origin: "redist", severity: "blocker", fix: "安装 Visual C++ 2012 运行库" },
  { name: "d3d9.dll", origin: "system", severity: "blocker", fix: "更新显卡驱动或启用 Direct3D 9 功能" },
  { name: "d3d11.dll", origin: "system", severity: "blocker", fix: "更新显卡驱动（Direct3D 11）" },
  { name: "dxgi.dll", origin: "system", severity: "blocker", fix: "更新显卡驱动（DXGI 层）" },
  { name: "d3dcompiler_47.dll", origin: "system", severity: "blocker", fix: "安装 DirectX 运行时（着色器编译器）" },
  { name: "d3dx9_43.dll", origin: "redist", severity: "blocker", fix: "安装 DirectX End-User Runtime（d3dx9 系列）" },
  { name: "dinput8.dll", origin: "system", severity: "degraded", fix: "手柄/输入设备驱动缺失，功能降级" },
  { name: "xinput1_4.dll", origin: "system", severity: "degraded", fix: "安装 DirectX 运行时（手柄支持）" },
  { name: "xinput9_1_0.dll", origin: "system", severity: "degraded", fix: "安装 DirectX 运行时（手柄支持）" },
  { name: "vulkan-1.dll", origin: "redist", severity: "blocker", fix: "安装 Vulkan 运行时或更新显卡驱动" },
  { name: "opengl32.dll", origin: "system", severity: "blocker", fix: "更新显卡驱动（OpenGL）" },
  { name: "openal32.dll", origin: "redist", severity: "degraded", fix: "安装 OpenAL 声音运行时" },
  { name: "dsound.dll", origin: "system", severity: "degraded", fix: "安装 DirectX 运行时（DirectSound）" },
  { name: "steam_api64.dll", origin: "app", severity: "blocker", fix: "应用目录缺 Steam API 层，校验游戏完整性" },
  { name: "steam_api.dll", origin: "app", severity: "blocker", fix: "应用目录缺 Steam API 层，校验游戏完整性" },
  { name: "libcef.dll", origin: "app", severity: "blocker", fix: "应用自带 CEF 缺失，重装该应用" },
  { name: "qt5core.dll", origin: "app", severity: "blocker", fix: "应用自带 Qt 5 缺失，重装该应用" },
];

const DLL_INDEX = new Map(DLL_KNOWLEDGE.map((k) => [k.name.toLowerCase(), k]));

export interface DllFinding {
  name: string;
  origin: DllOrigin | "unknown";
  severity: DllSeverity | "unknown";
  fix: string;
}

export interface ProphetVerdict {
  findings: DllFinding[];
  /** 收录条目数（预言置信的诚实口径：只对收录 DLL 预言）。 */
  known: number;
  /** 未收录条目数（如实 UNKNOWN，不计入预言）。 */
  unknown: number;
}

/**
 * 导入表快扫：对 missing 名单逐条比对内置库；
 * 收录 → 结构化预言；未收录 → UNKNOWN（不假装能预言）。
 */
export function prophetScan(missingDlls: string[]): ProphetVerdict {
  const findings: DllFinding[] = [];
  let known = 0;
  let unknown = 0;
  for (const raw of missingDlls) {
    const k = DLL_INDEX.get(raw.toLowerCase());
    if (k) {
      known++;
      findings.push({ name: k.name, origin: k.origin, severity: k.severity, fix: k.fix });
    } else {
      unknown++;
      findings.push({ name: raw, origin: "unknown", severity: "unknown", fix: "本地库未收录，无法预言" });
    }
  }
  return { findings, known, unknown };
}

/** 预言卡文案（缺什么/从哪来/怎么补）。 */
export function prophetCardText(app: string, v: ProphetVerdict): string {
  if (v.findings.length === 0) return `${app} · 导入表完整，放行启动`;
  const originZh: Record<string, string> = { system: "系统组件", redist: "运行库", app: "应用自带", unknown: "未收录" };
  const lines = v.findings.map((f) => `· ${f.name}（${originZh[f.origin] ?? "未收录"}）→ ${f.fix}`);
  const head = v.unknown > 0 ? `${app} · 预言 ${v.known} 项 / 未收录 ${v.unknown} 项` : `${app} · 预言 ${v.known} 项`;
  return [head, ...lines].join("\n");
}

// ---- W-104 全屏逃生演练 ----

export const ESCAPE_HANG_MS = 10_000;

export type EscapeTier = "graceful" | "windowed" | "terminate";

/** 三档安全序列（按序兜底）。 */
export const ESCAPE_TIERS: readonly EscapeTier[] = ["graceful", "windowed", "terminate"];

export interface EscapePlan {
  pid: number;
  name: string;
  /** 系统进程：永不第三档（验收硬约束）。 */
  system: boolean;
  tiers: EscapeTier[];
  hangMs: number;
}

/** 逃生计划：hangMs 达阈值才出计划；系统进程剔除 terminate。 */
export function escapePlan(pid: number, name: string, system: boolean, hangMs: number): EscapePlan | null {
  if (hangMs < ESCAPE_HANG_MS) return null;
  const tiers = system ? ESCAPE_TIERS.filter((t) => t !== "terminate") : [...ESCAPE_TIERS];
  return { pid, name, system, tiers, hangMs };
}

/** 下一档：当前档失败后推进；无档可退 → null（诚实交回用户）。 */
export function nextEscapeTier(plan: EscapePlan, done: EscapeTier[]): EscapeTier | null {
  return plan.tiers.find((t) => !done.includes(t)) ?? null;
}

export interface EscapeLogEntry {
  at: number;
  pid: number;
  name: string;
  tier: EscapeTier;
  ok: boolean;
}

export const ESCAPE_LOG_MAX = 100;

/** 留痕（验收：序列逐档执行且日志留痕）。 */
export function escapeLogAppend(
  log: EscapeLogEntry[],
  entry: Omit<EscapeLogEntry, "at">,
  now: number,
  max = ESCAPE_LOG_MAX,
): EscapeLogEntry[] {
  const next = [...log, { ...entry, at: now }];
  return next.slice(-max);
}

// ---- W-105 输入组段接力 ----

export type ImeEvKind = "comp-start" | "comp-update" | "comp-end" | "input";

export interface ImeEvent {
  kind: ImeEvKind;
  at: number;
  /** input 事件的字符（keydown 落盘特征）。 */
  ch?: string;
}

export const IME_SWALLOW_MIN_CHARS = 2;
export const IME_SWALLOW_WINDOW_MS = 1_500;

export interface SwallowVerdict {
  swallowed: boolean;
  /** 可解释证据（验收：检测启发式可解释）。 */
  reason: string;
  /** 被吞组段之后的落盘字符（接力重放素材）。 */
  orphans: string[];
}

/**
 * 组段吞字检测启发式（可解释）：
 * comp-start 后无 comp-end，且窗口内 ≥2 个字符直接 input 落盘 → 判定被吞。
 */
export function swallowDetected(events: ImeEvent[]): SwallowVerdict {
  const lastStart = [...events].reverse().find((e) => e.kind === "comp-start");
  if (!lastStart) return { swallowed: false, reason: "无组段开始", orphans: [] };
  const ended = events.some((e) => e.kind === "comp-end" && e.at >= lastStart.at);
  if (ended) return { swallowed: false, reason: "组段正常收尾", orphans: [] };
  const after = events.filter((e) => e.at > lastStart.at && e.at - lastStart.at <= IME_SWALLOW_WINDOW_MS);
  const orphans = after
    .filter((e) => e.kind === "input" && typeof e.ch === "string" && e.ch.length > 0)
    .map((e) => e.ch as string);
  if (orphans.length < IME_SWALLOW_MIN_CHARS) {
    return {
      swallowed: false,
      reason: `落盘字符 ${orphans.length} < ${IME_SWALLOW_MIN_CHARS}，证据不足`,
      orphans,
    };
  }
  return {
    swallowed: true,
    reason: `组段未收尾且 ${orphans.length} 字符直接落盘`,
    orphans,
  };
}

/** 白名单制：仅登记过的应用才允许接力（防误伤）。 */
export function relayAllowed(app: string, whitelist: readonly string[]): boolean {
  return whitelist.includes(app);
}

/** 接力重放：把被吞字符依序重放（空串 = 无需接力）。 */
export function relayReplay(verdict: SwallowVerdict): string {
  return verdict.swallowed ? verdict.orphans.join("") : "";
}

// ---- W-106 字体替身 ----

export interface FontMetric {
  name: string;
  /** 上升高度（em 单位）。 */
  ascent: number;
  /** 下降高度（em 单位）。 */
  descent: number;
  /** 平均字宽（em 单位）。 */
  avgWidth: number;
  /** 是否等宽（等宽只能替等宽）。 */
  mono: boolean;
}

/** metric 相似度 0..1：加权欧氏距离（等宽不匹配直接 0）。 */
export function understudyScore(missing: FontMetric, candidate: FontMetric): number {
  if (missing.mono !== candidate.mono) return 0;
  const d =
    0.5 * Math.abs(missing.ascent - candidate.ascent) +
    0.3 * Math.abs(missing.descent - candidate.descent) +
    0.2 * Math.abs(missing.avgWidth - candidate.avgWidth);
  return Math.round(Math.max(0, 1 - d) * 1000) / 1000;
}

/** Top1 替身（相似度最高；平分取名字典序——确定性）。 */
export function pickUnderstudy(
  missing: FontMetric,
  candidates: FontMetric[],
): { font: FontMetric; score: number } | null {
  let best: { font: FontMetric; score: number } | null = null;
  for (const c of candidates) {
    const s = understudyScore(missing, c);
    if (s <= 0) continue;
    if (!best || s > best.score || (s === best.score && c.name < best.font.name)) best = { font: c, score: s };
  }
  return best;
}

/** 浮标文案（可关由开关层管）。 */
export function understudyNotice(missing: string, sub: FontMetric, score: number): string {
  return `替身演出中：${missing} → ${sub.name}（metric 相似度 ${(score * 100).toFixed(0)}%）`;
}

// ---- W-107 DPI 转场防腐 ----

export const DPI_FREEZE_DEFAULT_MS = 300;

export interface DpiMorphState {
  /** 冻结遮罩是否应显示。 */
  frozen: boolean;
  /** 距揭幕还剩多少 ms（<=0 → 揭幕）。 */
  remainMs: number;
}

/** 防腐时序：揭幕不早于 max(变更时刻, 重绘完成时刻) + freezeMs。 */
export function dpiMorphState(changeAt: number, redrawDoneAt: number, now: number, freezeMs: number): DpiMorphState {
  const revealAt = Math.max(changeAt, redrawDoneAt) + freezeMs;
  return { frozen: now < revealAt, remainMs: Math.max(0, revealAt - now) };
}

/** 参数读取（Hub 可调 100–600）。 */
export function dpiFreezeMs(): number {
  const v = num("W-107", "freezeMs");
  return v > 0 ? clamp(v, 100, 600) : DPI_FREEZE_DEFAULT_MS;
}

// ---- W-108 遗留应用医院 ----

export type HospitalCheck = "dpi-blur" | "font-fuzzy" | "scale-misalign";
export type HospitalCure = "dpi-virtualization" | "disable-scaling" | "compat-shim";

export const HOSPITAL_CHECKS: readonly HospitalCheck[] = ["dpi-blur", "font-fuzzy", "scale-misalign"];
export const HOSPITAL_CURES: readonly HospitalCure[] = ["dpi-virtualization", "disable-scaling", "compat-shim"];

export interface HospitalPrescription {
  exe: string;
  name: string;
  checks: HospitalCheck[];
  cures: HospitalCure[];
}

/** 内置处方库（20+ 常见 Win7 时代遗留应用；exe 小写键）。 */
export const HOSPITAL_PRESCRIPTIONS: readonly HospitalPrescription[] = [
  { exe: "winmine.exe", name: "扫雷", checks: ["dpi-blur", "scale-misalign"], cures: ["dpi-virtualization", "compat-shim"] },
  { exe: "sol.exe", name: "纸牌", checks: ["dpi-blur", "scale-misalign"], cures: ["dpi-virtualization", "compat-shim"] },
  { exe: "mspaint.exe", name: "旧版画图", checks: ["dpi-blur", "font-fuzzy"], cures: ["dpi-virtualization"] },
  { exe: "notepad.exe", name: "旧版记事本", checks: ["font-fuzzy"], cures: ["dpi-virtualization"] },
  { exe: "write.exe", name: "写字板", checks: ["font-fuzzy", "dpi-blur"], cures: ["dpi-virtualization", "compat-shim"] },
  { exe: "hypertrm.exe", name: "超级终端", checks: ["dpi-blur", "font-fuzzy", "scale-misalign"], cures: ["dpi-virtualization", "compat-shim"] },
  { exe: "ttplayer.exe", name: "千千静听", checks: ["dpi-blur", "font-fuzzy"], cures: ["dpi-virtualization", "disable-scaling"] },
  { exe: "storm.exe", name: "暴风影音", checks: ["dpi-blur", "scale-misalign"], cures: ["dpi-virtualization", "disable-scaling"] },
  { exe: "qvodplayer.exe", name: "快播", checks: ["dpi-blur", "font-fuzzy"], cures: ["dpi-virtualization"] },
  { exe: "pps.exe", name: "PPS 影音", checks: ["dpi-blur", "scale-misalign"], cures: ["dpi-virtualization", "disable-scaling"] },
  { exe: "pptv.exe", name: "PPTV", checks: ["dpi-blur", "scale-misalign"], cures: ["dpi-virtualization", "disable-scaling"] },
  { exe: "thunder.exe", name: "迅雷 7", checks: ["dpi-blur", "font-fuzzy"], cures: ["dpi-virtualization", "compat-shim"] },
  { exe: "kugou.exe", name: "酷狗音乐", checks: ["dpi-blur"], cures: ["dpi-virtualization"] },
  { exe: "kuwo.exe", name: "酷我音乐", checks: ["dpi-blur"], cures: ["dpi-virtualization"] },
  { exe: "qq.exe", name: "旧版 QQ", checks: ["dpi-blur", "font-fuzzy"], cures: ["dpi-virtualization", "disable-scaling"] },
  { exe: "foxmail.exe", name: "Foxmail 7", checks: ["font-fuzzy"], cures: ["dpi-virtualization"] },
  { exe: "winrar.exe", name: "WinRAR", checks: ["dpi-blur"], cures: ["dpi-virtualization"] },
  { exe: "emule.exe", name: "eMule", checks: ["dpi-blur", "font-fuzzy", "scale-misalign"], cures: ["dpi-virtualization", "compat-shim"] },
  { exe: "flashfxp.exe", name: "FlashFXP", checks: ["font-fuzzy", "scale-misalign"], cures: ["dpi-virtualization", "disable-scaling"] },
  { exe: "ultraedit.exe", name: "UltraEdit 旧版", checks: ["font-fuzzy"], cures: ["dpi-virtualization"] },
  { exe: "editplus.exe", name: "EditPlus 3", checks: ["font-fuzzy"], cures: ["dpi-virtualization"] },
  { exe: "acrord32.exe", name: "Acrobat Reader 旧版", checks: ["dpi-blur", "font-fuzzy"], cures: ["dpi-virtualization", "compat-shim"] },
];

const RX_INDEX = new Map(HOSPITAL_PRESCRIPTIONS.map((p) => [p.exe.toLowerCase(), p]));

/** 处方查询：未收录 → null（如实无处方，不硬开药）。 */
export function hospitalRx(exe: string): HospitalPrescription | null {
  return RX_INDEX.get(exe.toLowerCase()) ?? null;
}

export interface HospitalRecord {
  exe: string;
  /** 已施加的治疗（可逐一撤销）。 */
  applied: HospitalCure[];
  /** 归档的复查记录。 */
  reviews: Array<{ at: number; note: string }>;
}

/** 施加治疗：重复施加幂等；不在处方内的治疗拒绝（不出方不下药）。 */
export function hospitalApply(rec: HospitalRecord, rx: HospitalPrescription, cure: HospitalCure): HospitalRecord {
  if (!rx.cures.includes(cure)) return rec;
  if (rec.applied.includes(cure)) return rec;
  return { ...rec, applied: [...rec.applied, cure] };
}

/** 撤销治疗（逐一可逆）。 */
export function hospitalUndo(rec: HospitalRecord, cure: HospitalCure): HospitalRecord {
  if (!rec.applied.includes(cure)) return rec;
  return { ...rec, applied: rec.applied.filter((c) => c !== cure) };
}

/** 复查归档。 */
export function hospitalReview(rec: HospitalRecord, note: string, at: number, max = 20): HospitalRecord {
  return { ...rec, reviews: [...rec.reviews, { at, note }].slice(-max) };
}

export const CHECK_ZH: Record<HospitalCheck, string> = {
  "dpi-blur": "DPI 模糊",
  "font-fuzzy": "字体发虚",
  "scale-misalign": "缩放错位",
};
export const CURE_ZH: Record<HospitalCure, string> = {
  "dpi-virtualization": "高 DPI 虚拟化",
  "disable-scaling": "禁用缩放",
  "compat-shim": "兼容垫片",
};

// ---- W-109 游戏反作弊礼让 ----

export const YIELD_RESUME_MS = 30_000;

export type YieldCapability = "hooks" | "overlays" | "screenshot" | "automation";

export interface YieldState {
  active: boolean;
  /** 触发特征（未激活为 null）。 */
  sig: string | null;
  /** 礼让中被挂起的能力组（Q-98 降级矩阵可见口径）。 */
  suspended: YieldCapability[];
}

/** 全面礼让的能力组（诚实声明范围）。 */
export const YIELD_CAPABILITIES: readonly YieldCapability[] = ["hooks", "overlays", "screenshot", "automation"];

/** 反作弊特征表（进程/驱动名片段小写；命中即礼让）。 */
export const ANTICHEAT_SIGNATURES: readonly string[] = [
  "easyanticheat",
  "eac_launcher",
  "beservice",
  "battleye",
  "vgc.exe",
  "vgtray.exe",
  "vgk.sys",
  "sguard64.exe",
  "sguard.sys",
  "ace-base.sys",
  "anticheatexpert",
  "tp3ship",
  "tenprotect",
  "faceitclient",
];

/** 特征命中判定（返回命中的特征片段）。 */
export function anticheatHit(processName: string): string | null {
  const p = processName.toLowerCase();
  for (const sig of ANTICHEAT_SIGNATURES) {
    if (p.includes(sig)) return sig;
  }
  return null;
}

/** 礼让进入（挂起全部能力组）。 */
export function yieldEnter(sig: string): YieldState {
  return { active: true, sig, suspended: [...YIELD_CAPABILITIES] };
}

/** 礼让退出防抖：退出后须满 30s 才允许恢复（防再入误恢复）。 */
export function yieldCanResume(now: number, lastExitAt: number, resumeMs = YIELD_RESUME_MS): boolean {
  return now - lastExitAt >= resumeMs;
}

/** 礼让横幅文案。 */
export function yieldBannerText(s: YieldState): string {
  if (!s.active) return "";
  return "反作弊礼让中 · 钩子/悬浮层/截图已挂起 · 退出 30s 后自动恢复";
}

// ---- W-110 蓝屏检尸官 ----

export const FORENSIC_TIMELINE_MS = 5 * MINUTE;
export const INSUFFICIENT_DATA = "INSUFFICIENT DATA";

/** minidump 头部魔数（MDMP，小端 0x504D444D）。 */
const MINIDUMP_MAGIC = 0x504d444d;

export interface MinidumpParse {
  ok: boolean;
  /** 解析失败原因（不解析就如实说，不硬猜）。 */
  reason?: string;
  /** BugCheck 代码（头 0x38 处 4 字节，小端）。 */
  bugcheck?: number;
  /** 肇事模块名（启发式 ASCII 扫描；找不到如实 undefined）。 */
  culprit?: string;
}

function u32le(bytes: Uint8Array, off: number): number {
  return (bytes[off]! | (bytes[off + 1]! << 8) | (bytes[off + 2]! << 16) | (bytes[off + 3]! << 24)) >>> 0;
}

/**
 * minidump 本地极简解析（诚实边界）：
 * - 验魔数 MDMP + 长度；失败 → INSUFFICIENT DATA；
 * - BugCheck 取头 0x38 处；模块名从 0x108 后首个 `*.sys/dll/exe` 可打印段启发式提取；
 * - BugCheck 为 0 视为结构异常（不可信），结论层如实降级。
 */
export function parseMinidump(bytes: Uint8Array): MinidumpParse {
  if (bytes.length < 0x40) return { ok: false, reason: INSUFFICIENT_DATA };
  if (u32le(bytes, 0) !== MINIDUMP_MAGIC) return { ok: false, reason: INSUFFICIENT_DATA };
  const bugcheck = u32le(bytes, 0x38);
  let culprit: string | undefined;
  let run = "";
  for (let i = 0x108; i < bytes.length; i++) {
    const b = bytes[i]!;
    if (b >= 0x20 && b <= 0x7e) {
      run += String.fromCharCode(b);
      if (!culprit) {
        const m = run.match(/[a-z0-9_.]+\.(?:sys|dll|exe)$/i);
        if (m) culprit = m[0].toLowerCase();
      }
    } else {
      run = "";
    }
  }
  return { ok: true, bugcheck, culprit };
}

/** 解析结果是否可信（ok 且 bugcheck 非零）。 */
export function minidumpTrustworthy(p: MinidumpParse): boolean {
  return p.ok && (p.bugcheck ?? 0) !== 0;
}

export interface EnvEvent {
  at: number;
  text: string;
}

/** 崩溃前 5 分钟环境侧事件（时间线升序）。 */
export function envTimeline(events: EnvEvent[], crashAt: number, windowMs = FORENSIC_TIMELINE_MS): EnvEvent[] {
  return events
    .filter((e) => e.at <= crashAt && crashAt - e.at <= windowMs)
    .sort((a, b) => a.at - b.at);
}

export type ForensicVerdictKind = "unrelated" | "possibly-related" | "insufficient-data";

/** 中立结论词表（措辞中立：不替系统背锅也不甩锅）。 */
export const NEUTRAL_LEXICON: Record<ForensicVerdictKind, string> = {
  unrelated: "现有证据未显示与环境相关，倾向系统侧因素。",
  "possibly-related": "存在时间上的邻近事件，不能排除相关性，需进一步排查。",
  "insufficient-data": `证据不足，无法给出结论（${INSUFFICIENT_DATA}）。`,
};

/** 中立结论：解析不可信 → insufficient-data（诚实）。 */
export function forensicVerdict(
  p: MinidumpParse,
  envEventsNear: EnvEvent[],
): { kind: ForensicVerdictKind; text: string } {
  if (!minidumpTrustworthy(p)) return { kind: "insufficient-data", text: NEUTRAL_LEXICON["insufficient-data"] };
  if (envEventsNear.length > 0) return { kind: "possibly-related", text: NEUTRAL_LEXICON["possibly-related"] };
  return { kind: "unrelated", text: NEUTRAL_LEXICON.unrelated };
}

/** 检尸卡文案。 */
export function forensicCardText(
  p: MinidumpParse,
  verdict: { kind: ForensicVerdictKind; text: string },
  near: EnvEvent[],
): string {
  if (!p.ok || !minidumpTrustworthy(p)) {
    return `BSOD 检尸 · ${INSUFFICIENT_DATA}\n· minidump 无法在本地解析，不猜测原因\n结论：${verdict.text}`;
  }
  const lines = [
    "BSOD 检尸",
    `· BugCheck 0x${(p.bugcheck ?? 0).toString(16).toUpperCase().padStart(8, "0")}`,
    p.culprit ? `· 肇事模块（启发式）：${p.culprit}` : "· 肇事模块：未能定位（如实声明）",
    `· 崩溃前 5 分钟环境侧事件 ${near.length} 条`,
    `结论：${verdict.text}`,
  ];
  return lines.join("\n");
}

// ---- W-111 色彩模式转影 ----

export const COLOR_MORPH_MS = 2000;

export type RGB = readonly [number, number, number];

/** 通道插值（t 会被夹到 [0,1]）。 */
export function colorLerp(a: RGB, b: RGB, t: number): RGB {
  const c = clamp(t, 0, 1);
  const r = Math.round(a[0] + (b[0] - a[0]) * c);
  const g = Math.round(a[1] + (b[1] - a[1]) * c);
  const bl = Math.round(a[2] + (b[2] - a[2]) * c);
  return [r, g, bl] as const;
}

/** 转影中间帧序列（n 段；末帧 = 目标色）。 */
export function morphStops(a: RGB, b: RGB, n: number): RGB[] {
  const count = Math.max(1, Math.floor(n));
  const out: RGB[] = [];
  for (let i = 1; i <= count; i++) out.push(colorLerp(a, b, i / count));
  return out;
}

const WHITE_THRESHOLD = 248;

function isWhite(c: RGB): boolean {
  return c.every((x) => x >= WHITE_THRESHOLD);
}

/**
 * 零白闪门禁（Z-68 同源语义）：中间帧（不含末帧端点）不得全通道 ≥248；
 * 首帧由起点色插值而来，若起点即白则 t>0 时已离开白色。
 */
export function zeroFlashOK(stops: RGB[]): boolean {
  return !stops.slice(0, -1).some(isWhite);
}

/** css rgb() 串。 */
export function rgbCss(c: RGB): string {
  return `rgb(${c[0]},${c[1]},${c[2]})`;
}

// ---- W-112 同名实例仲裁 ----

export const ARBITRATION_CONFIRM_MS = 2000;
export const ARBITRATION_GPU_PCT = 20;

export interface GpuInstance {
  pid: number;
  gpuPct: number;
}

export interface GpuSample {
  name: string;
  instances: GpuInstance[];
  at: number;
}

export interface ArbitrationCase {
  needed: boolean;
  name: string;
  /** 争抢实例 pid。 */
  pids: number[];
  reason: string;
}

/**
 * 同名实例争抢检测：同进程名 ≥2 实例、各 gpuPct ≥ 阈值、
 * 连续持续 ≥2s（确认窗防误报）才成立。
 */
export function arbitrationCase(samples: GpuSample[]): ArbitrationCase {
  if (samples.length === 0) return { needed: false, name: "", pids: [], reason: "" };
  const sorted = [...samples].sort((a, b) => a.at - b.at);
  const name = sorted[0]!.name;
  const contested = sorted.filter(
    (s) => s.name === name && s.instances.length >= 2 && s.instances.filter((i) => i.gpuPct >= ARBITRATION_GPU_PCT).length >= 2,
  );
  if (contested.length < 2) {
    return { needed: false, name, pids: [], reason: `争抢帧数不足（${contested.length} 帧，需确认窗内连续命中）` };
  }
  const span = contested[contested.length - 1]!.at - contested[0]!.at;
  if (span < ARBITRATION_CONFIRM_MS) {
    return { needed: false, name, pids: [], reason: `争抢 ${span}ms < ${ARBITRATION_CONFIRM_MS}ms 确认窗` };
  }
  const pids = [...new Set(contested[contested.length - 1]!.instances.map((i) => i.pid))];
  return { needed: true, name, pids, reason: `双实例各 ≥${ARBITRATION_GPU_PCT}% GPU 持续 ${(span / 1000).toFixed(1)}s` };
}

export type ArbitrationAction = "fps-cap" | "affinity";

export interface ArbitrationSuggestion {
  id: ArbitrationAction;
  labelZh: string;
  /** 执行描述（用户确认后由 S17 进程 API 落地）。 */
  action: string;
}

/** 仲裁建议（环境不擅自动手）。 */
export const ARBITRATION_SUGGESTIONS: readonly ArbitrationSuggestion[] = [
  { id: "fps-cap", labelZh: "后台实例限帧 30fps", action: "cap background instance to 30fps" },
  { id: "affinity", labelZh: "分核亲和隔离", action: "pin instances to disjoint CPU sets" },
];

/** 用户确认才执行（验收硬约束：未确认 → 拒绝）。 */
export function applyArbitration(action: ArbitrationAction, confirmed: boolean): { ok: boolean; action: string } {
  const s = ARBITRATION_SUGGESTIONS.find((x) => x.id === action) ?? ARBITRATION_SUGGESTIONS[0]!;
  return { ok: confirmed, action: s.action };
}

export const ARBITRATION_POLICY_KEY = `${NS}.arbitration-policy`;

/** 永久策略（按进程名记住选择）。 */
export function saveArbitrationPolicy(name: string, action: ArbitrationAction): void {
  const p = lsGet<Record<string, ArbitrationAction>>(ARBITRATION_POLICY_KEY, {});
  p[name] = action;
  lsSet(ARBITRATION_POLICY_KEY, p);
}

export function loadArbitrationPolicy(): Record<string, ArbitrationAction> {
  return lsGet<Record<string, ArbitrationAction>>(ARBITRATION_POLICY_KEY, {});
}

// ---- W-113 系统更新护城河 ----

/** 已知兼容矩阵：大版本更新后可能失效的环境设置键（真实登记）。 */
export const MOAT_RISK_KEYS: readonly string[] = [
  "wallpaper.path",
  "wallpaper.engine",
  "dock.modules",
  "dock.position",
  "hotkeys.custom",
  "theme.tokens",
  "font.fallbacks",
  "icon.cache",
  "overlay.zones",
  "input.rhythm",
];

export interface MoatBackup {
  at: number;
  /** 更新 build（若有）。 */
  build: string;
  /** 快照（仅风险键，真实存在的设置才入快照）。 */
  snapshot: Record<string, unknown>;
  /** FNV-1a 32bit 校验和（快照 JSON 串）。 */
  checksum: string;
}

/** FNV-1a 32bit 校验和（备份完整性）。 */
export function fnv1a(text: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
}

/** 护城河备份：仅备份风险清单内且真实存在的键（诚实不虚标）。 */
export function makeMoatBackup(settings: Record<string, unknown>, build: string, at: number): MoatBackup {
  const snapshot: Record<string, unknown> = {};
  for (const k of MOAT_RISK_KEYS) {
    if (k in settings) snapshot[k] = settings[k];
  }
  const checksum = fnv1a(JSON.stringify(snapshot));
  return { at, build, snapshot, checksum };
}

/** 备份完整性校验。 */
export function moatBackupIntact(b: MoatBackup): boolean {
  return fnv1a(JSON.stringify(b.snapshot)) === b.checksum;
}

/** 还原：返回快照补丁（应用侧 merge；校验和不符 → 空补丁并如实拒绝）。 */
export function restoreMoatBackup(b: MoatBackup): Record<string, unknown> {
  if (!moatBackupIntact(b)) return {};
  return { ...b.snapshot };
}

/** 回滚指引（固定三步）。 */
export const MOAT_ROLLBACK_STEPS: readonly string[] = [
  "1. 更新完成后打开 新星中枢 · 兼容诊疗路 · 护城河",
  "2. 点击「还原设置」应用更新前快照（校验和自动校验）",
  "3. 逐项核对失效项并在兼容矩阵登记新条目",
];

// ---------------------------------------------------------------------------
// 行为层（事件摄入 + 面板/遮罩/横幅挂载；激活幂等）
// ---------------------------------------------------------------------------

const STYLE_ID = "nova-compat-style";

let active = false;
let bag: Array<() => void> = [];

function ensureStyle(): void {
  if (typeof document === "undefined" || document.getElementById(STYLE_ID)) return;
  const css = `
@keyframes nova-compat-breathe { 0%,100% { opacity:.45 } 50% { opacity:1 } }
.nova-compat-escape { position:fixed; right:18px; top:18px; z-index:2147483000;
  width:26px; height:26px; border-radius:50%; display:flex; align-items:center; justify-content:center;
  background:rgba(220,60,40,.85); color:#fff; font-size:14px; cursor:pointer;
  box-shadow:0 2px 10px rgba(0,0,0,.35); }
.nova-compat-escape[data-motion="1"] { animation: nova-compat-breathe 1.6s ease-in-out infinite; }
.nova-compat-card { position:fixed; right:18px; bottom:18px; z-index:2147483000; max-width:360px;
  padding:12px 14px; border-radius:10px; background:var(--nova-glass,rgba(20,20,24,.82));
  backdrop-filter:blur(12px); border:1px solid rgba(255,255,255,.1); color:inherit;
  font-size:12px; line-height:1.7; letter-spacing:.02em; white-space:pre-line; }
.nova-compat-card-title { font-size:9px; letter-spacing:.22em; opacity:.55; margin-bottom:4px; }
.nova-compat-card-row { display:flex; gap:8px; margin-top:8px; }
.nova-compat-card-btn { flex:1; padding:4px 8px; border-radius:6px; border:1px solid rgba(255,255,255,.16);
  background:transparent; color:inherit; cursor:pointer; font-size:11px; letter-spacing:.04em; }
.nova-compat-yield { position:fixed; left:50%; top:14px; transform:translateX(-50%); z-index:2147483000;
  padding:6px 14px; border-radius:999px; background:rgba(180,120,20,.88); color:#fff;
  font-size:11px; letter-spacing:.14em; }
.nova-compat-yield[data-motion="1"] { animation: nova-compat-breathe 2.4s ease-in-out infinite; }
.nova-compat-dpi { position:fixed; inset:0; z-index:2147482900; pointer-events:none; transition:opacity .18s ease; }
.nova-compat-colormorph { position:fixed; inset:0; z-index:2147482800; pointer-events:none; }
.nova-compat-panel { position:fixed; right:18px; top:56px; z-index:2147483000; width:380px; max-height:70vh;
  overflow:auto; padding:14px 16px; border-radius:12px; background:var(--nova-glass,rgba(20,20,24,.86));
  backdrop-filter:blur(14px); border:1px solid rgba(255,255,255,.1); color:inherit;
  font-size:12px; line-height:1.7; letter-spacing:.02em; }
.nova-compat-panel h3 { margin:10px 0 4px; font-size:11px; letter-spacing:.14em; opacity:.85; }
.nova-compat-panel h3:first-child { margin-top:0; }
.nova-compat-panel .muted { opacity:.55; font-size:11px; }
.nova-compat-panel pre { margin:4px 0; padding:8px; border-radius:8px; background:rgba(127,127,127,.1);
  font-size:11px; line-height:1.6; white-space:pre-wrap; }
.nova-compat-grade-stable { color:#4caf7d; }
.nova-compat-grade-occasional { color:#d9a13b; }
.nova-compat-grade-highrisk { color:#e0564b; }
`;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = css;
  document.head.appendChild(style);
}

/** 卡片瞬时元素（自动消失；可选按钮行）。 */
function flashCard(
  title: string,
  body: string,
  ttlMs: number,
  buttons?: Array<{ label: string; onClick: () => void }>,
): void {
  if (typeof document === "undefined") return;
  const el = document.createElement("div");
  el.className = "nova-compat-card";
  const t = document.createElement("div");
  t.className = "nova-compat-card-title";
  t.textContent = title;
  el.appendChild(t);
  const b = document.createElement("div");
  b.textContent = body;
  el.appendChild(b);
  if (buttons && buttons.length > 0) {
    const row = document.createElement("div");
    row.className = "nova-compat-card-row";
    for (const btn of buttons) {
      const bt = document.createElement("button");
      bt.className = "nova-compat-card-btn";
      bt.textContent = btn.label;
      bt.addEventListener("click", () => {
        btn.onClick();
        el.remove();
      });
      row.appendChild(bt);
    }
    el.appendChild(row);
  }
  document.body.appendChild(el);
  window.setTimeout(() => el.remove(), ttlMs);
}

// ---- W-102 观察簿（持久态 + overlay） ----

interface WatchApp {
  firstSeenAt: number;
  events: WatchEntry[];
  /** 用户提前终结。 */
  closed: boolean;
}

const WATCH_KEY = `${NS}.watchbook`;

function loadWatchBook(): Record<string, WatchApp> {
  return lsGet<Record<string, WatchApp>>(WATCH_KEY, {});
}

function saveWatchBook(book: Record<string, WatchApp>): void {
  lsSet(WATCH_KEY, book);
}

function recordWatch(app: string, kind: WatchKind, at: number): void {
  const book = loadWatchBook();
  const rec = book[app] ?? { firstSeenAt: at, events: [], closed: false };
  rec.events = pruneWatchLog([...rec.events, { kind, at }]);
  book[app] = rec;
  saveWatchBook(book);
  compatEvent("watch-record", { app, kind, at });
}

// ---- W-105 IME 白名单 ----

const IME_WL_KEY = `${NS}.ime-whitelist`;

export function imeWhitelist(): string[] {
  return lsGet<string[]>(IME_WL_KEY, []);
}

export function imeWhitelistAdd(app: string): void {
  const wl = imeWhitelist();
  if (!wl.includes(app)) lsSet(IME_WL_KEY, [...wl, app]);
}

// ---- W-110 检尸事件缓存 ----

export const ENV_EVENTS_MAX = 300;
let envEvents: EnvEvent[] = [];

function recordEnvEvent(text: string, at: number): void {
  envEvents = [...envEvents, { at, text }].slice(-ENV_EVENTS_MAX);
}

// ---- W-109 礼让运行态 ----

let yieldTimer: number | null = null;
let yieldOn = false;

function enterYield(sig: string): void {
  if (!flagOn("W-109") || yieldOn) return;
  yieldOn = true;
  if (typeof document !== "undefined") {
    document.documentElement.dataset.novaCompatYield = "on";
    const banner = document.createElement("div");
    banner.className = "nova-compat-yield";
    banner.id = "nova-compat-yield-banner";
    banner.dataset.motion = motionOK() ? "1" : "0";
    banner.textContent = yieldBannerText(yieldEnter(sig));
    document.body.appendChild(banner);
  }
  compatEvent("yield", { active: true, sig, suspended: YIELD_CAPABILITIES });
}

function scheduleYieldResume(): void {
  if (!yieldOn) return;
  if (yieldTimer != null) window.clearTimeout(yieldTimer);
  yieldTimer = window.setTimeout(exitYieldNow, YIELD_RESUME_MS);
}

function exitYieldNow(): void {
  if (!yieldOn) return;
  yieldOn = false;
  if (yieldTimer != null) {
    window.clearTimeout(yieldTimer);
    yieldTimer = null;
  }
  if (typeof document !== "undefined") {
    delete document.documentElement.dataset.novaCompatYield;
    document.getElementById("nova-compat-yield-banner")?.remove();
  }
  compatEvent("yield", { active: false, sig: null, suspended: [] });
}

// ---- W-107 DPI 遮罩 / W-111 色彩转影 ----

let dpiMask: HTMLDivElement | null = null;

function dpiMaskShow(freezeMs: number): void {
  if (typeof document === "undefined" || !flagOn("W-107")) return;
  dpiMask?.remove();
  const mask = document.createElement("div");
  mask.className = "nova-compat-dpi";
  mask.style.background = "var(--nova-glass, rgba(16,16,20,0.92))";
  mask.style.opacity = "1";
  document.body.appendChild(mask);
  dpiMask = mask;
  window.setTimeout(() => {
    mask.style.opacity = "0";
    window.setTimeout(() => mask.remove(), 220);
    if (dpiMask === mask) dpiMask = null;
  }, freezeMs);
}

let morphEl: HTMLDivElement | null = null;

function colorMorphRun(from: RGB, to: RGB): void {
  if (typeof document === "undefined" || !flagOn("W-111")) return;
  if (!motionOK()) return; // reduce-motion → 直接切换（转影取消，语义保留）
  const steps = 20;
  const stops = morphStops(from, to, steps);
  if (!zeroFlashOK(stops)) return; // 门禁：可能白闪 → 放弃转影直接切换（诚实）
  morphEl?.remove();
  const el = document.createElement("div");
  el.className = "nova-compat-colormorph";
  document.body.appendChild(el);
  morphEl = el;
  let i = 0;
  const stepMs = COLOR_MORPH_MS / steps;
  const tick = (): void => {
    if (!morphEl) return;
    if (i >= stops.length) {
      el.remove();
      if (morphEl === el) morphEl = null;
      return;
    }
    el.style.background = rgbCss(stops[i]!);
    i++;
    window.setTimeout(tick, stepMs);
  };
  tick();
}

// ---- overlay 面板（nova-watch / nova-hospital / nova-forensics） ----

type PanelKind = "nova-watch" | "nova-hospital" | "nova-forensics";
let openPanel: PanelKind | null = null;

function closePanel(): void {
  document.getElementById("nova-compat-panel")?.remove();
  openPanel = null;
}

function panelShell(title: string): HTMLDivElement {
  closePanel();
  const el = document.createElement("div");
  el.className = "nova-compat-panel";
  el.id = "nova-compat-panel";
  const h = document.createElement("h3");
  h.textContent = title;
  el.appendChild(h);
  return el;
}

function panelWatch(): void {
  if (typeof document === "undefined") return;
  const el = panelShell("APP WATCH · 应用观察簿");
  const book = loadWatchBook();
  const apps = Object.entries(book);
  if (apps.length === 0) {
    const p = document.createElement("div");
    p.className = "muted";
    p.textContent = "暂无观察对象 —— 应用启动事件由 nova://compat-watch 摄入";
    el.appendChild(p);
  }
  const now = Date.now();
  for (const [app, rec] of apps) {
    const s = watchSummary(app, rec.events, now, rec.firstSeenAt);
    const h = document.createElement("h3");
    const grade = document.createElement("span");
    grade.className = `nova-compat-grade-${s.grade}`;
    grade.textContent = watchSummaryText(s);
    h.textContent = rec.closed ? "（已提前终结）" : s.complete ? "（期满）" : "（观察中）";
    h.appendChild(grade);
    el.appendChild(h);
    if (!rec.closed) {
      const btn = document.createElement("button");
      btn.className = "nova-compat-card-btn";
      btn.textContent = "提前终结观察期";
      btn.addEventListener("click", () => {
        const b2 = loadWatchBook();
        const cur = b2[app];
        if (cur) {
          cur.closed = true;
          saveWatchBook(b2);
        }
        panelWatch();
      });
      el.appendChild(btn);
    }
  }
  document.body.appendChild(el);
  openPanel = "nova-watch";
}

function panelHospital(): void {
  if (typeof document === "undefined") return;
  const el = panelShell("LEGACY HOSPITAL · 遗留应用医院");
  const recs = lsGet<Record<string, HospitalRecord>>(`${NS}.hospital`, {});
  const names = new Set<string>([...Object.keys(recs), ...HOSPITAL_PRESCRIPTIONS.map((p) => p.exe)]);
  for (const exe of [...names].sort()) {
    const rx = hospitalRx(exe);
    const rec = recs[exe] ?? { exe, applied: [], reviews: [] };
    const h = document.createElement("h3");
    h.textContent = rx ? `${rx.name}（${exe}）` : `${exe}（无处方 · 如实标注）`;
    el.appendChild(h);
    if (!rx) continue;
    const info = document.createElement("div");
    info.textContent = `三查：${rx.checks.map((c) => CHECK_ZH[c]).join(" / ")} ｜ 三治：${rx.cures.map((c) => CURE_ZH[c]).join(" / ")}`;
    el.appendChild(info);
    const applied = document.createElement("div");
    applied.className = rec.applied.length > 0 ? "" : "muted";
    applied.textContent =
      rec.applied.length > 0 ? `已施加：${rec.applied.map((c) => CURE_ZH[c]).join("、")}` : "未施加任何治疗";
    el.appendChild(applied);
    const row = document.createElement("div");
    row.className = "nova-compat-card-row";
    for (const cure of rx.cures) {
      const isOn = rec.applied.includes(cure);
      const bt = document.createElement("button");
      bt.className = "nova-compat-card-btn";
      bt.textContent = isOn ? `撤销·${CURE_ZH[cure]}` : `施加·${CURE_ZH[cure]}`;
      bt.addEventListener("click", () => {
        const all = lsGet<Record<string, HospitalRecord>>(`${NS}.hospital`, {});
        const cur = all[exe] ?? { exe, applied: [], reviews: [] };
        all[exe] = isOn ? hospitalUndo(cur, cure) : hospitalApply(cur, rx, cure);
        lsSet(`${NS}.hospital`, all);
        panelHospital();
      });
      row.appendChild(bt);
    }
    el.appendChild(row);
  }
  document.body.appendChild(el);
  openPanel = "nova-hospital";
}

function panelForensics(): void {
  if (typeof document === "undefined") return;
  const el = panelShell("BSOD FORENSICS · 检尸卡");
  const last = lsGet<string | null>(`${NS}.forensics-card`, null);
  if (!last) {
    const p = document.createElement("div");
    p.className = "muted";
    p.textContent = "尚无检尸记录 —— 蓝屏后首启由 nova://compat-minidump 摄入（解析全程本地）";
    el.appendChild(p);
  } else {
    const pre = document.createElement("pre");
    pre.textContent = last;
    el.appendChild(pre);
  }
  document.body.appendChild(el);
  openPanel = "nova-forensics";
}

function runForensics(bytes: Uint8Array, crashAt: number): void {
  const p = parseMinidump(bytes);
  const near = envTimeline(envEvents, crashAt);
  const v = forensicVerdict(p, near);
  const text = forensicCardText(p, v, near);
  lsSet(`${NS}.forensics-card`, text);
  if (flagOn("W-110")) {
    flashCard("BSOD FORENSICS", text.split("\n").slice(0, 2).join("\n"), 8000, [
      { label: "查看完整检尸卡", onClick: () => panelForensics() },
    ]);
  }
  compatEvent("forensics-done", { kind: v.kind, culprit: p.culprit ?? null });
}

// ---- W-104 逃生门运行态 ----

interface EscapeRuntime {
  plan: EscapePlan;
  done: EscapeTier[];
  el: HTMLDivElement | null;
}

let escapeRun: EscapeRuntime | null = null;

function escapeBegin(pid: number, name: string, system: boolean, hangMs: number): void {
  if (!flagOn("W-104") || escapeRun) return;
  const plan = escapePlan(pid, name, system, hangMs);
  if (!plan) return;
  let el: HTMLDivElement | null = null;
  if (typeof document !== "undefined") {
    el = document.createElement("div");
    el.className = "nova-compat-escape";
    el.dataset.motion = motionOK() ? "1" : "0";
    el.title = "全屏逃生门";
    el.textContent = "⏻";
    el.addEventListener("click", () => escapeStep());
    document.body.appendChild(el);
  }
  escapeRun = { plan, done: [], el };
}

function escapeStep(): void {
  if (!escapeRun) return;
  const tier = nextEscapeTier(escapeRun.plan, escapeRun.done);
  if (!tier) {
    const pid = escapeRun.plan.pid;
    escapeEnd();
    compatEvent("escape-exhausted", { pid });
    return;
  }
  // 逐档执行：真实进程操作由 S17 接线（本模块只派发序列事件 + 留痕）
  compatEvent("escape-tier", {
    pid: escapeRun.plan.pid,
    tier,
    name: escapeRun.plan.name,
    system: escapeRun.plan.system,
  });
  escapeRun.done = [...escapeRun.done, tier];
}

function escapeEnd(): void {
  escapeRun?.el?.remove();
  escapeRun = null;
}

// ---- W-112 仲裁运行态 ----

let gpuSamples: GpuSample[] = [];

function gpuSampleIngest(s: GpuSample): void {
  if (!flagOn("W-112")) return;
  if (loadArbitrationPolicy()[s.name]) return; // 永久策略已设：不再打扰
  gpuSamples = [...gpuSamples, s].filter((x) => x.name === s.name).slice(-20);
  const c = arbitrationCase(gpuSamples);
  if (!c.needed) return;
  gpuSamples = [];
  flashCard("GPU ARBITRATION", `${c.name} 双实例争抢 GPU\n${c.reason}`, 15000, [
    {
      label: "限帧 30fps",
      onClick: () => {
        applyArbitration("fps-cap", true);
        compatEvent("arbitration-apply", { name: c.name, pids: c.pids, action: "fps-cap" });
      },
    },
    {
      label: "分核亲和",
      onClick: () => {
        applyArbitration("affinity", true);
        compatEvent("arbitration-apply", { name: c.name, pids: c.pids, action: "affinity" });
      },
    },
    { label: "本次忽略", onClick: () => undefined },
  ]);
}

// ---- W-113 护城河 ----

const MOAT_KEY = `${NS}.moat-backup`;

function osUpdateIngest(detail: { pending?: boolean; build?: string }): void {
  if (!flagOn("W-113")) return;
  if (detail.pending !== true) return; // 无更新零打扰
  const build = typeof detail.build === "string" ? detail.build : "unknown";
  void (async () => {
    try {
      const { loadSettings } = await import("../../../lib/settings");
      const s = (await loadSettings()) as unknown as Record<string, unknown> | null;
      const backup = makeMoatBackup(s ?? {}, build, Date.now());
      lsSet(MOAT_KEY, backup);
      flashCard("UPDATE MOAT", `系统大版本更新待重启（${build}）\n已备份 ${Object.keys(backup.snapshot).length} 项风险设置`, 12000, [
        {
          label: "查看回滚指引",
          onClick: () => flashCard("UPDATE MOAT · 回滚指引", MOAT_ROLLBACK_STEPS.join("\n"), 20000),
        },
      ]);
      compatEvent("moat-backup", { build, keys: Object.keys(backup.snapshot) });
    } catch {
      /* 设置不可读：如实不打扰、不伪造备份 */
    }
  })();
}

/** 手动还原护城河备份（回滚指引消费）。 */
export function moatRestore(): Record<string, unknown> {
  const b = lsGet<MoatBackup | null>(MOAT_KEY, null);
  if (!b) return {};
  return restoreMoatBackup(b);
}

// ---- S0 注册表订阅（惰性绑定；与 filesNova 同构） ----

type RegistrySubscribe = (fn: () => void) => () => void;
let registrySubscribe: RegistrySubscribe | null = null;

function subscribeNovaShim(): () => void {
  return registrySubscribe ? registrySubscribe(onRegistryChange) : () => undefined;
}

function onRegistryChange(): void {
  // 开关关闭即时生效：礼让撤销、逃生门收起、面板收起
  if (!flagOn("W-109")) exitYieldNow();
  if (!flagOn("W-104")) escapeEnd();
  if (openPanel) {
    const overlayByPanel: Record<PanelKind, string> = {
      "nova-watch": "W-102",
      "nova-hospital": "W-108",
      "nova-forensics": "W-110",
    };
    if (!flagOn(overlayByPanel[openPanel])) closePanel();
  }
}

// ---- 运行态缓存（卸载/测试重置） ----

let imeBuffer: Array<ImeEvent & { app: string }> = [];

/** 测试/热重载辅助：清空模块级易变态（不改持久化）。 */
export function compatResetVolatile(): void {
  imeBuffer = [];
  gpuSamples = [];
  envEvents = [];
  escapeEnd();
}

// ---- 激活 / 卸载 ----

/**
 * 激活（幂等）：事件摄入 + overlay 直达 + 注册表订阅。
 * 全部监听入 bag，卸载逐一移除。
 */
export function activateCompatNova(): void {
  if (active || typeof window === "undefined" || typeof document === "undefined") return;
  active = true;
  ensureStyle();

  // W-102 观察簿摄入
  const onWatch = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { app?: string; kind?: WatchKind; at?: number } | undefined;
    if (d?.app && d.kind) recordWatch(d.app, d.kind, d.at ?? Date.now());
  };
  window.addEventListener("nova://compat-watch", onWatch);
  bag.push(() => window.removeEventListener("nova://compat-watch", onWatch));

  // W-103 导入表预言
  const onImports = (ev: Event): void => {
    if (!flagOn("W-103")) return;
    const d = (ev as CustomEvent).detail as { app?: string; missing?: string[] } | undefined;
    if (!d?.app || !Array.isArray(d.missing)) return;
    const v = prophetScan(d.missing);
    compatEvent("prophet-verdict", { app: d.app, ...v });
    if (v.findings.length > 0) {
      flashCard("DLL PROPHET", prophetCardText(d.app, v), 10000);
    }
  };
  window.addEventListener("nova://compat-imports", onImports);
  bag.push(() => window.removeEventListener("nova://compat-imports", onImports));

  // W-104 全屏心跳
  const onEscape = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { pid?: number; name?: string; system?: boolean; hangMs?: number } | undefined;
    if (typeof d?.pid !== "number" || typeof d.name !== "string") return;
    if (d.hangMs != null && d.hangMs >= ESCAPE_HANG_MS) escapeBegin(d.pid, d.name, d.system === true, d.hangMs);
    else escapeEnd(); // 恢复响应 → 收起逃生门
  };
  window.addEventListener("nova://compat-escape", onEscape);
  bag.push(() => window.removeEventListener("nova://compat-escape", onEscape));

  // W-105 IME 组段
  const onIme = (ev: Event): void => {
    if (!flagOn("W-105")) return;
    const d = (ev as CustomEvent).detail as { app?: string; type?: ImeEvKind; key?: string; at?: number } | undefined;
    if (!d?.app || !d.type) return;
    imeBuffer.push({ app: d.app, kind: d.type, ch: d.key, at: d.at ?? Date.now() });
    if (imeBuffer.length > 64) imeBuffer = imeBuffer.slice(-64);
    if (!relayAllowed(d.app, imeWhitelist())) return; // 白名单外零干预
    const appEvents = imeBuffer.filter((e) => e.app === d.app);
    const verdict = swallowDetected(appEvents);
    if (verdict.swallowed) {
      const replay = relayReplay(verdict);
      compatEvent("ime-relay", { app: d.app, replay, reason: verdict.reason });
      imeBuffer = imeBuffer.filter((e) => e.app !== d.app);
    }
  };
  window.addEventListener("nova://compat-ime", onIme);
  bag.push(() => window.removeEventListener("nova://compat-ime", onIme));

  // W-106 字体替身
  const onFont = (ev: Event): void => {
    if (!flagOn("W-106")) return;
    const d = (ev as CustomEvent).detail as { missing?: string; metric?: FontMetric; candidates?: FontMetric[] } | undefined;
    if (!d?.missing || !d.metric || !Array.isArray(d.candidates)) return;
    const pick = pickUnderstudy(d.metric, d.candidates);
    compatEvent("font-understudy", { missing: d.missing, pick: pick?.font.name ?? null, score: pick?.score ?? 0 });
    if (pick) {
      flashCard("FONT UNDERSTUDY", understudyNotice(d.missing, pick.font, pick.score), 7000);
    }
  };
  window.addEventListener("nova://compat-font", onFont);
  bag.push(() => window.removeEventListener("nova://compat-font", onFont));

  // W-107 DPI 防腐
  const onDpi = (): void => {
    dpiMaskShow(dpiFreezeMs());
  };
  window.addEventListener("nova://compat-dpi", onDpi);
  bag.push(() => window.removeEventListener("nova://compat-dpi", onDpi));

  // W-108 医院摄入
  const onLegacy = (ev: Event): void => {
    if (!flagOn("W-108")) return;
    const d = (ev as CustomEvent).detail as { exe?: string } | undefined;
    if (!d?.exe) return;
    const rx = hospitalRx(d.exe);
    if (!rx) return; // 未收录：不打扰（医院面板可查，如实无处方）
    compatEvent("legacy-rx", { exe: d.exe, name: rx.name, checks: rx.checks, cures: rx.cures });
  };
  window.addEventListener("nova://compat-legacy", onLegacy);
  bag.push(() => window.removeEventListener("nova://compat-legacy", onLegacy));

  // W-109 礼让
  const onAnticheat = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { active?: boolean; sig?: string } | undefined;
    if (d?.active === true) enterYield(d.sig ?? "unknown");
    else scheduleYieldResume();
  };
  window.addEventListener("nova://compat-anticheat", onAnticheat);
  bag.push(() => window.removeEventListener("nova://compat-anticheat", onAnticheat));

  // W-110 检尸
  const onMinidump = (ev: Event): void => {
    if (!flagOn("W-110")) return;
    const d = (ev as CustomEvent).detail as { bytes?: Uint8Array | number[]; at?: number } | undefined;
    if (!d?.bytes) return;
    const bytes = d.bytes instanceof Uint8Array ? d.bytes : new Uint8Array(d.bytes);
    runForensics(bytes, d.at ?? Date.now());
  };
  window.addEventListener("nova://compat-minidump", onMinidump);
  bag.push(() => window.removeEventListener("nova://compat-minidump", onMinidump));

  // W-111 色彩转影
  const onColorMode = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { from?: RGB; to?: RGB } | undefined;
    if (Array.isArray(d?.from) && Array.isArray(d?.to)) colorMorphRun(d.from as RGB, d.to as RGB);
  };
  window.addEventListener("nova://compat-colormode", onColorMode);
  bag.push(() => window.removeEventListener("nova://compat-colormode", onColorMode));

  // W-112 GPU 仲裁
  const onGpu = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { name?: string; instances?: GpuInstance[]; at?: number } | undefined;
    if (typeof d?.name !== "string" || !Array.isArray(d.instances)) return;
    gpuSampleIngest({ name: d.name, instances: d.instances, at: d.at ?? Date.now() });
  };
  window.addEventListener("nova://compat-gpu", onGpu);
  bag.push(() => window.removeEventListener("nova://compat-gpu", onGpu));

  // W-113 护城河
  const onOsUpdate = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { pending?: boolean; build?: string } | undefined;
    osUpdateIngest(d ?? {});
  };
  window.addEventListener("nova://compat-osupdate", onOsUpdate);
  bag.push(() => window.removeEventListener("nova://compat-osupdate", onOsUpdate));

  // 环境侧事件时间线（W-110 检尸素材；低频摄入）
  const onEnvEvent = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { text?: string; at?: number } | undefined;
    if (d?.text) recordEnvEvent(d.text, d.at ?? Date.now());
  };
  window.addEventListener("nova://compat-env-event", onEnvEvent);
  bag.push(() => window.removeEventListener("nova://compat-env-event", onEnvEvent));

  // Hub overlay 直达（ai04 协议：nova-watch / nova-hospital / nova-forensics）
  const onOpenFeature = (ev: Event): void => {
    const f = (ev as CustomEvent).detail?.feature;
    if (f === "nova-watch" && flagOn("W-102")) (openPanel === "nova-watch" ? closePanel : panelWatch)();
    if (f === "nova-hospital" && flagOn("W-108")) (openPanel === "nova-hospital" ? closePanel : panelHospital)();
    if (f === "nova-forensics" && flagOn("W-110")) (openPanel === "nova-forensics" ? closePanel : panelForensics)();
  };
  window.addEventListener("ai04:open-feature", onOpenFeature);
  bag.push(() => window.removeEventListener("ai04:open-feature", onOpenFeature));

  // S0 注册表变化 → 即时生效/失效
  bag.push(subscribeNovaShim());
}

export function deactivateCompatNova(): void {
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
  exitYieldNow();
  escapeEnd();
  dpiMask?.remove();
  dpiMask = null;
  morphEl?.remove();
  morphEl = null;
  closePanel();
  compatResetVolatile();
  document
    .querySelectorAll(".nova-compat-card,.nova-compat-yield,.nova-compat-escape,.nova-compat-panel")
    .forEach((e) => e.remove());
}

export function isCompatNovaActive(): boolean {
  return active;
}

// 惰性绑定 S0 注册表订阅（避免测试环境无 window 时报错）
if (typeof window !== "undefined") {
  void Promise.resolve().then(async () => {
    try {
      const mod = await import("../registry");
      registrySubscribe = (fn: () => void) => mod.subscribeNova(fn);
      if (active) bag.push(subscribeNovaShim());
    } catch {
      /* S0 未就绪：开关即时生效退化为下次激活生效 */
    }
  });
}
