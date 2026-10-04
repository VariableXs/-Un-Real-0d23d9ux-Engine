/**
 * NOVA-200 · S16 工程收官路（AI-16）—— 域16 工程质量、性能与收官（W-188…W-200）。
 *
 * 边界（全景 §16）：
 * - W-188 能效标签管**能耗叙事**（欧盟能效 A–G）；U-24 基准门禁管速度维；
 * - W-189 睡眠孵化器管**内存换出**深眠（工作集归零、1.5s 热还原）；
 *   M-06 手动挂起 / Z-59 渲染冻结不越界；
 * - W-190 崩溃重放器管**逐帧单步重放**（仅确定性崩溃、仅 dev）；
 *   M-53 管转储收集、Q-96 管主动注入；
 * - W-191 GPU 诊断砚管 **GPU 帧级**耗时墨沉积；Q-94 管 CPU 主线程；
 * - W-192 军棋推演管**分身间对照**（N-35 分身本体 / V-90 插件试用不越界）；
 * - W-193 内存地平线管**跨版本水位**归档；U-20 管实时守护；
 * - W-194 可复现指纹管**产物可复现**验证（内容寻址哈希树）；M-87 管流程演练；
 * - W-195 白纸视角是**首跑状态**专用沙盒（零设置零历史）；N-35 管隔离本体；
 * - W-196 性能气象站管**状态隐喻**播报（阈值与 N-19 HUD 同源）；
 * - W-197 工程星象管**当前全指标**盘面（12 星座=12 指标）；Q-100 管时间线；
 * - W-198 发版礼炮管**发版时刻**仪式（仅 dev/手动触发）；Q-100 管质量数据；
 * - W-199 不朽档案管**全指标**历史最佳；W-193 只管内存一项；
 * - W-200 测试战绩册管**成就叙事**（vitest/cargo 真实结果同源）；Z-63 管基线。
 *
 * 纪律：
 * - 零侵入：不改任何既有组件内部逻辑；全部为 DOM 叠层 + `nova://quality-*`
 *   自定义事件摄入 + `nova.quality.*` 本地存储（外部接线由 S17 按 wiringHint 补齐）；
 * - 前缀：类名 `nova-quality-`、事件 `nova://quality-*`、localStorage 键 `nova.quality.*`；
 * - 开关：只读消费 S0 注册表（registry.ts novaOn）；非 DOM 环境行为层安全 no-op；
 * - 降级：reduce-motion / safeMode / static 三态动效归零（礼炮降级为静态铭牌、
 *   星象呼吸/砚台墨晕消失），语义与数据不变；
 * - 默认档：W-190/192/195 与 S0 注册表一致默认关（dev/显式开启），其余默认开。
 *
 * 诚实边界：
 * - W-188 三轴实测（空载 CPU% × 时长）；阈值公开透明；无数据维度如实 `N/A`；
 * - W-189 入眠内存释放 ≥ 90% 达标、唤醒还原 100%（快照校验和比对）、
 *   手动唤醒优先级最高；实际换出经 `nova://quality-nest-sleep/wake` 派发由
 *   系统层执行（S17 接 Rust 工作集 API），本模块不伪造数据；
 * - W-190 仅确定性崩溃可重放（UI/时序类如实标 `NOT DETERMINISTIC`）；
 *   重放逐步状态对照、100% 复现崩溃点；仅 dev 可见；
 * - W-191 采样零干扰（rAF 计时戳）；墨图保留最近 5 分钟；浓墨对应真实掉帧；
 * - W-192 战报三方数据实测（负载注入时钟对齐校验）；非对照期零采样零开销；
 * - W-193 地平线数据随版本发布实测更新；击穿/逼近（+15%）判定可解释；
 * - W-194 指纹算法内容寻址哈希树；不可复现如实 `NOT REPRODUCIBLE` 并列差异文件；
 * - W-195 白纸沙盒与真首跑 1:1（零设置/零历史/全引导）；退出即销毁零残留；
 * - W-196 气象判定与 HUD 阈值同源（WEATHER_HUD 常量导出共享）；预报基于本地
 *   使用规律、娱乐性误差如实标注；
 * - W-197 12 指标数据同源真实门禁（`nova://quality-zodiac` 摄入）；暗星可展开
 *   明细；星座命名仅取 IAU 星座词表作盘面标签（不迷信化）；
 * - W-198 礼炮仅 dev 构建或手动触发；铭牌持久化历史版本号；
 *   reduce-motion 降级为静态铭牌；
 * - W-199 破纪录判定严格（同机器同场景才比）；档案含版本/日期/机器上下文；
 * - W-200 战绩数据同源 vitest/cargo test 真实结果（`nova://quality-tests`
 *   摄入）；徽章本地渲染零网络。
 */

import { novaMotionOK, novaOn } from "../registry";

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

const NS = "nova.quality";

/** 功能开关：只读消费 S0 注册表。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

/** 派发 `nova://quality-*` 事件（SSR/测试环境安全）。 */
export function qualityEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://quality-${name}`, { detail }));
}

// ---------------------------------------------------------------------------
// Hub 注册清单（S0 NovaHub 消费：功能卡 + 降级说明）
// ---------------------------------------------------------------------------

export interface NovaFeatureCard {
  id: string;
  titleZh: string;
  titleEn: string;
  descZh: string;
  defaultOn: boolean;
  /** 独立 overlay 工具窗（ai04:open-feature 的 feature id，nova- 前缀）。 */
  overlay?: string;
  /** 需要 S17 在既有文件补接线的行为（本模块 API/事件已备好）。 */
  wiringHint?: string;
  /** reduce-motion / safeMode / static 降级说明。 */
  degrade: string;
}

export const QUALITY_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-188",
    titleZh: "能效标签",
    titleEn: "Energy Label",
    descZh: "环境能效等级标签：空载/启动/动效三轴实测（CPU% × 时长）评定 A–G 级（欧盟能效视觉），阈值公开透明；无数据维度如实 N/A。",
    defaultOn: true,
    wiringHint: "三轴实测经 nova://quality-energy {axes:[{axis,cpuPct,ms}]} 喂入；标签呈现于诊断包（U-42 由 S17 嵌位）",
    degrade: "静态标签（无等级条动画）；等级与阈值完全保留",
  },
  {
    id: "W-189",
    titleZh: "睡眠孵化器",
    titleEn: "Hibernation Nest",
    descZh: "空闲 30 分钟的应用从冻结升级入眠：工作集换出到盘（释放 ≥ 90%、唤醒 1.5s 内热还原 100%）；睡眠列表 hub 可见可唤醒，手动唤醒优先级最高。",
    defaultOn: true,
    wiringHint: "睡眠/唤醒经 nova://quality-nest-sleep {id} / nova://quality-nest-wake {id,priority} 派发（S17 接 Rust 工作集换出）；名单经 nova://quality-nest-report 摄入",
    degrade: "入眠判定与列表照常（无动效依赖）；换出动作由系统层执行",
  },
  {
    id: "W-190",
    titleZh: "崩溃单步重放器",
    titleEn: "Crash Replay",
    descZh: "确定性崩溃（纯逻辑类）一键单步重放：逐步执行到崩溃点、每步状态对照，100% 复现案发现场；UI/时序类崩溃如实标 NOT DETERMINISTIC；仅 dev 可见。",
    defaultOn: false,
    wiringHint: "崩溃转储（M-53 同源）经 nova://quality-dump {dump} 喂入；重放器仅 dev 构建渲染",
    degrade: "重放为纯数据对照（非动效依赖）；NOT DETERMINISTIC 语义不变",
  },
  {
    id: "W-191",
    titleZh: "GPU 诊断砚",
    titleEn: "GPU Inkstone",
    descZh: "每帧 GPU 耗时以墨点沉积成砚台图（越慢墨越浓）：rAF 计时戳零干扰采样、保留最近 5 分钟、浓墨阈值对应真实掉帧——帧性能的研墨。",
    defaultOn: true,
    wiringHint: "帧耗时经 nova://quality-frames {frames:[{ms,at}]} 喂入（或 S17 接 requestAnimationFrame 计时）",
    degrade: "静态墨图（无墨晕动画）；沉积浓度与掉帧判定不变",
  },
  {
    id: "W-192",
    titleZh: "多环境军棋推演",
    titleEn: "Sandbox Wargame",
    descZh: "分身 A/B 对照运行同一负载（同一壁纸/同一插件集），产出性能/内存/能耗三方对照战报；负载注入时钟对齐校验，非对照期分身零额外开销。",
    defaultOn: false,
    wiringHint: "双方实测经 nova://quality-wargame {a,b} 喂入（N-35 分身由 S17 提供负载对齐）",
    degrade: "静态战报（三方数据表）；判定逻辑不变",
  },
  {
    id: "W-193",
    titleZh: "内存地平线",
    titleEn: "Memory Floor",
    descZh: "跨版本空载内存最低水位地平线：虚线标注 FLOOR xxxMB @version；新版本击穿（更低）即更新水位，逼近红线（+15%）即警报——内存的地质层。",
    defaultOn: true,
    wiringHint: "版本空载实测经 nova://quality-floor {version,idleMB,at} 喂入；水位线在 hub 图表常显",
    degrade: "静态地平线图表（无警报闪烁）；击穿/逼近判定不变",
  },
  {
    id: "W-194",
    titleZh: "可复现指纹",
    titleEn: "Build Fingerprint",
    descZh: "同源码双构建的内容寻址哈希树比对：一致即 REPRODUCIBLE（供应链可信），不一致如实标 NOT REPRODUCIBLE 并列出差异文件清单；指纹徽章打在发版物。",
    defaultOn: true,
    wiringHint: "构建产物清单经 nova://quality-build {builds:[{label,files:[{path,content}]}]} 喂入（两次构建对照）",
    degrade: "静态指纹徽章与差异清单",
  },
  {
    id: "W-195",
    titleZh: "白纸视角",
    titleEn: "Fresh Eyes",
    descZh: "开发者命令 nova fresh-eyes：以零设置/零历史/首次引导全流程的全新用户视角启动专用沙盒（与真首跑 1:1）——设计师的白纸；退出即销毁零残留。",
    defaultOn: false,
    wiringHint: "nova://quality-fresh 派发生成白纸档案；退出派发 nova://quality-fresh-destroy 并做残留核查",
    degrade: "白纸档案为纯数据视图（非动效依赖）；销毁语义不变",
  },
  {
    id: "W-196",
    titleZh: "性能气象站",
    titleEn: "Perf Weather",
    descZh: "性能状态的气象隐喻播报：晴/多云/雨/风暴 = 空闲/常态/繁忙/过载（阈值与 N-19 HUD 同源常量），含基于本地使用规律的未来 30 分钟预报；娱乐性预报误差如实标注。",
    defaultOn: true,
    wiringHint: "当前负载经 nova://quality-weather {cpuPct,memPct} 喂入；WEATHER_HUD 阈值常量与 HUD 共享（S17 同源接线）",
    degrade: "静态播报（无天气图标动画）；气象判定与预报不变",
  },
  {
    id: "W-197",
    titleZh: "工程星象",
    titleEn: "Eng Zodiac",
    descZh: "工程质量的十二星象盘：测试覆盖/文档比率/依赖健康/门禁通过率等 12 指标映射为 12 星座（亮暗=健康度），暗星点击展开明细——质量的趣味化 dashboard（星座仅取 IAU 词表，不迷信化）。",
    defaultOn: true,
    overlay: "nova-zodiac",
    wiringHint: "12 指标（U-24/Z-63 门禁同源）经 nova://quality-zodiac {metrics:[{key,value,detail?}]} 喂入",
    degrade: "静态星盘（无星点呼吸）；亮度与暗星判定不变",
  },
  {
    id: "W-198",
    titleZh: "发版礼炮",
    titleEn: "Release Cannon",
    descZh: "发版收官仪式：版本里程碑墙 + 2.5s 全屏礼炮动画（可跳过）+ 版本号烙印入历史铭牌；礼炮仅在 dev 构建或手动触发（不打扰用户）；reduce-motion 降级为静态铭牌。",
    defaultOn: true,
    wiringHint: "发版事件经 nova://quality-release {version,note?} 喂入；礼炮触发校验 cannonAllowed({dev,manual})",
    degrade: "礼炮动画归零，仅静态铭牌呈现；里程碑墙与持久化不变",
  },
  {
    id: "W-199",
    titleZh: "性能不朽档案",
    titleEn: "Golden Archive",
    descZh: "各指标历史最佳值入不朽档案（最快启动/最低内存/最高帧率…），被超越时档案更新并致敬 NEW RECORD；破纪录判定严格（同机器同场景），档案含版本/日期/机器上下文、可导出。",
    defaultOn: true,
    wiringHint: "候选成绩经 nova://quality-archive {metric,value,version,at,machine,scenario} 喂入",
    degrade: "静态档案馆清单（无致敬动画）；破纪录判定不变",
  },
  {
    id: "W-200",
    titleZh: "测试战绩册",
    titleEn: "Test Battle Pass",
    descZh: "测试体系的荣誉册：通过率里程碑（1000/1200/1500 用例）逐级解锁徽章，各域测试数分布功勋墙——战绩数据同源 vitest/cargo test 真实结果，徽章本地渲染零网络。",
    defaultOn: true,
    overlay: "nova-battlepass",
    wiringHint: "真实测试结果经 nova://quality-tests {total,passed,failed,domains} 喂入（S17 接 vitest/cargo test 报告）",
    degrade: "静态徽章墙（无解锁动画）；里程碑判定不变",
  },
];

export const qualityNovaDomain = {
  id: "S16",
  nameZh: "工程质量与收官",
  nameEn: "Quality & Finale",
  route: "AI-16",
  features: QUALITY_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// W-188 能效标签（Energy Label）—— 三轴 A–G 评定（纯逻辑）
// ---------------------------------------------------------------------------

export type EnergyAxis = "idle" | "boot" | "motion";

export interface EnergySample {
  axis: EnergyAxis;
  /** 实测平均 CPU 占用（%）。 */
  cpuPct: number;
  /** 实测持续时长（ms）。 */
  ms: number;
}

export type EnergyGrade = "A" | "B" | "C" | "D" | "E" | "F" | "G" | "N/A";

/**
 * 能效当量（毫核·秒 mCPU·s = CPU% × 秒）：三轴统一的能耗度量。
 * 阈值公开透明（验收）：A ≤ 120 … G > 3840，每级翻倍。
 */
export const ENERGY_BANDS: Array<{ grade: Exclude<EnergyGrade, "N/A">; maxMCpuSec: number }> = [
  { grade: "A", maxMCpuSec: 120 },
  { grade: "B", maxMCpuSec: 240 },
  { grade: "C", maxMCpuSec: 480 },
  { grade: "D", maxMCpuSec: 960 },
  { grade: "E", maxMCpuSec: 1920 },
  { grade: "F", maxMCpuSec: 3840 },
  { grade: "G", maxMCpuSec: Number.POSITIVE_INFINITY },
];

/** 实测 → 能效当量（mCPU·s；CPU% × 时长）。 */
export function mcpuSec(s: { cpuPct: number; ms: number }): number {
  if (!(s.cpuPct >= 0) || !(s.ms > 0)) return 0;
  return s.cpuPct * (s.ms / 1000);
}

/** 当量 → A–G 级（公开阈值逐级判定）。 */
export function energyGradeOf(m: number): EnergyGrade {
  if (!(m > 0)) return "N/A"; // 无数据如实 N/A
  for (const b of ENERGY_BANDS) if (m <= b.maxMCpuSec) return b.grade;
  return "G";
}

export interface EnergyLabelReport {
  axes: Array<{ axis: EnergyAxis; mcpuSec: number; grade: EnergyGrade }>;
  /** 总评 = 各轴最差等级（缺轴不虚报，全缺为 N/A）。 */
  overall: EnergyGrade;
  complete: boolean;
}

/** 三轴实测 → 能效标签（空载/启动/动效；缺维度如实 N/A）。 */
export function energyLabel(samples: EnergySample[]): EnergyLabelReport {
  const byAxis = new Map<EnergyAxis, EnergySample>();
  for (const s of samples) {
    if (s && (s.axis === "idle" || s.axis === "boot" || s.axis === "motion") && Number.isFinite(s.cpuPct) && Number.isFinite(s.ms)) {
      byAxis.set(s.axis, s);
    }
  }
  const order: EnergyAxis[] = ["idle", "boot", "motion"];
  const axes = order.map((axis) => {
    const s = byAxis.get(axis);
    if (!s) return { axis, mcpuSec: 0, grade: "N/A" as EnergyGrade };
    const m = mcpuSec(s);
    return { axis, mcpuSec: Math.round(m * 10) / 10, grade: energyGradeOf(m) };
  });
  const graded = axes.filter((a) => a.grade !== "N/A");
  let overall: EnergyGrade = "N/A";
  if (graded.length > 0) {
    overall = graded.reduce((worst, a) => (ENERGY_BANDS.findIndex((b) => b.grade === a.grade) > ENERGY_BANDS.findIndex((b) => b.grade === worst) ? a.grade : worst), graded[0]!.grade);
  }
  return { axes, overall, complete: graded.length === order.length };
}

// ---------------------------------------------------------------------------
// W-189 睡眠孵化器（Hibernation Nest）—— 空闲 30min 工作集换出（纯逻辑）
// ---------------------------------------------------------------------------

export const NEST_IDLE_MIN = 30; // 空闲 30 分钟入眠
export const NEST_RELEASE_TARGET = 0.9; // 释放 ≥ 90% 达标
export const NEST_WAKE_BUDGET_MS = 1500; // 唤醒 1.5s 内热还原

export interface NestApp {
  id: string;
  /** 连续空闲分钟。 */
  idleMin: number;
  /** 当前工作集（MB）。 */
  worksetMB: number;
  /** 快照校验和（换出前状态指纹，用于 100% 还原核验）。 */
  snapshotHash: string;
}

export interface NestEntry extends NestApp {
  asleep: boolean;
  /** 换出后工作集（MB）。 */
  swappedMB: number;
  sleptAt: number;
}

/** 入眠判定：空闲 ≥ 30 分钟（达线即入眠；手动唤醒优先级最高）。 */
export function shouldNest(app: NestApp): boolean {
  return app.idleMin >= NEST_IDLE_MIN && app.worksetMB > 0;
}

/** 释放率 =（换出前 − 换出后）/ 换出前（验收：≥ 90% 达标）。 */
export function releaseRatio(beforeMB: number, afterMB: number): number {
  if (!(beforeMB > 0) || afterMB < 0) return 0;
  return clamp((beforeMB - afterMB) / beforeMB, 0, 1);
}

/** 入眠体检：释放率达标 + 唤醒在预算内。 */
export function nestVerdict(released: number, wakeMs: number): { pass: boolean; releaseOK: boolean; wakeOK: boolean } {
  const releaseOK = released >= NEST_RELEASE_TARGET;
  const wakeOK = wakeMs > 0 && wakeMs <= NEST_WAKE_BUDGET_MS;
  return { pass: releaseOK && wakeOK, releaseOK, wakeOK };
}

/** 唤醒还原核验：快照校验和一致 = 100% 还原（窗口内容/滚动位）。 */
export function restoreIntegrity(beforeHash: string, afterHash: string): boolean {
  return typeof beforeHash === "string" && beforeHash.length > 0 && beforeHash === afterHash;
}

/** 唤醒优先级：manual（手动/用户点击）最高。 */
export function wakePriority(manual: boolean): 0 | 1 {
  return manual ? 1 : 0;
}

/** 睡眠列表（hub 可见可唤醒；按空闲时长降序 = 最久闲者最深眠）。 */
export function nestList(entries: NestEntry[]): NestEntry[] {
  return [...entries].sort((a, b) => b.idleMin - a.idleMin);
}

// ---------------------------------------------------------------------------
// W-190 崩溃单步重放器（Crash Replay）—— 确定性逐帧重放（纯逻辑）
// ---------------------------------------------------------------------------

export interface ReplayOp {
  /** 纯逻辑操作（确定性）：set/add/mul。 */
  op: "set" | "add" | "mul";
  key: string;
  value: number;
}

export interface CrashDump {
  id: string;
  /** 是否确定性可重放（UI/时序类 = false → NOT DETERMINISTIC）。 */
  deterministic: boolean;
  /** 逐步操作序列。 */
  ops: ReplayOp[];
  /** 崩溃发生在第几步（1 起）。 */
  crashAtStep: number;
  /** 录制的逐步状态（重放对照源）。 */
  recordedStates: Array<Record<string, number>>;
}

export type ReplayVerdict =
  | { kind: "not-deterministic"; label: "NOT DETERMINISTIC" }
  | { kind: "reproduced"; label: "REPRODUCED"; atStep: number; steps: number }
  | { kind: "diverged"; label: "DIVERGED"; atStep: number; expect: number; got: number };

/** 逐步执行（确定性指令集；状态为纯数值字典）。 */
export function execReplayOps(ops: ReplayOp[], seed: Record<string, number> = {}): Array<Record<string, number>> {
  let state: Record<string, number> = { ...seed };
  const states: Array<Record<string, number>> = [];
  for (const o of ops) {
    const cur = state[o.key] ?? 0;
    if (o.op === "set") state = { ...state, [o.key]: o.value };
    else if (o.op === "add") state = { ...state, [o.key]: cur + o.value };
    else state = { ...state, [o.key]: cur * o.value };
    states.push({ ...state });
  }
  return states;
}

/**
 * 单步重放：逐步状态对照录制状态 → 100% 复现崩溃点判定。
 * - 非确定性 → 如实 NOT DETERMINISTIC（不假装能重放）；
 * - 全步一致且到达崩溃步 → REPRODUCED；
 * - 任一步状态发散 → DIVERGED + 首个发散位。
 */
export function replayCrash(dump: CrashDump): ReplayVerdict {
  if (!dump.deterministic) return { kind: "not-deterministic", label: "NOT DETERMINISTIC" };
  const states = execReplayOps(dump.ops);
  const upto = Math.min(dump.crashAtStep, states.length, dump.recordedStates.length);
  for (let i = 0; i < upto; i++) {
    const rec = dump.recordedStates[i]!;
    const got = states[i]!;
    for (const k of Object.keys(rec)) {
      if (rec[k] !== (got[k] ?? 0)) {
        return { kind: "diverged", label: "DIVERGED", atStep: i + 1, expect: rec[k]!, got: got[k] ?? 0 };
      }
    }
  }
  if (dump.crashAtStep > states.length) {
    // 崩溃步超出实际步数：录制不完整，如实发散（诚实口径）
    return { kind: "diverged", label: "DIVERGED", atStep: states.length, expect: dump.crashAtStep, got: states.length };
  }
  return { kind: "reproduced", label: "REPRODUCED", atStep: dump.crashAtStep, steps: states.length };
}

/** 重放器可见性：仅 dev 构建（验收：仅 dev 可见）。 */
export function replayVisible(isDev: boolean): boolean {
  return isDev;
}

// ---------------------------------------------------------------------------
// W-191 GPU 诊断砚（GPU Inkstone）—— 帧耗时墨沉积（纯逻辑）
// ---------------------------------------------------------------------------

export interface FrameSample {
  /** 帧耗时（ms，rAF 计时戳差：零干扰）。 */
  ms: number;
  /** 采样时刻（epoch ms）。 */
  at: number;
}

export const INK_WINDOW_MS = 5 * 60 * 1000; // 墨图保留最近 5 分钟
export const INK_FRAME_BUDGET_MS = 1000 / 60; // 60fps 预算 ≈ 16.7ms
export const INK_DROP_MS = (1000 / 60) * 2; // 浓墨阈值 = 预算 2 倍（真实掉帧）
export const INK_COLS = 60; // 5 分钟 = 60 格 × 5s

/** 滚动窗口剔除（保留最近 5 分钟）。 */
export function evictInk(samples: FrameSample[], now: number): FrameSample[] {
  return samples.filter((s) => Number.isFinite(s.ms) && s.ms > 0 && Number.isFinite(s.at) && now - s.at <= INK_WINDOW_MS && s.at <= now);
}

/**
 * 墨点浓度：越慢墨越浓（预算内渐染、超预算饱和）。
 * 浓度 ∈ [0,1]：frameMs ≤ 预算一半 → 0；≥ 预算 2 倍 → 1。
 */
export function inkConcentration(frameMs: number): number {
  if (!(frameMs > 0)) return 0;
  return clamp((frameMs - INK_FRAME_BUDGET_MS * 0.5) / (INK_FRAME_BUDGET_MS * 1.5), 0, 1);
}

export interface InkCell {
  col: number;
  count: number;
  /** 格内最浓墨点。 */
  peak: number;
  /** 浓墨样本数（> INK_DROP_MS = 真实掉帧）。 */
  drops: number;
  hotspot: boolean;
}

export interface InkReport {
  kept: number;
  evicted: number;
  worstMs: number;
  cells: InkCell[];
  hotspots: number;
}

/** 帧样本 → 墨沉积图（60 格 × 5s；窗口外如实剔除）。 */
export function inkGrid(samples: FrameSample[], now: number): InkReport {
  const kept = evictInk(samples, now);
  const cells: InkCell[] = Array.from({ length: INK_COLS }, (_, col) => ({ col, count: 0, peak: 0, drops: 0, hotspot: false }));
  let worstMs = 0;
  for (const s of kept) {
    const col = clamp(Math.floor((now - s.at) / (INK_WINDOW_MS / INK_COLS)), 0, INK_COLS - 1);
    const cell = cells[col]!;
    cell.count += 1;
    worstMs = Math.max(worstMs, s.ms);
    if (s.ms > INK_DROP_MS) cell.drops += 1;
    cell.peak = Math.max(cell.peak, inkConcentration(s.ms));
  }
  for (const c of cells) c.hotspot = c.drops > 0; // 浓墨 = 真实掉帧（验收）
  return { kept: kept.length, evicted: samples.length - kept.length, worstMs: Math.round(worstMs * 100) / 100, cells, hotspots: cells.filter((c) => c.hotspot).length };
}

// ---------------------------------------------------------------------------
// W-192 多环境军棋推演（Sandbox Wargame）—— A/B 三方对照（纯逻辑）
// ---------------------------------------------------------------------------

export interface WargameSide {
  id: string;
  label: string;
  /** 实测平均帧率（fps，高者胜）。 */
  fps: number;
  /** 实测空载内存（MB，低者胜）。 */
  memMB: number;
  /** 实测平均 CPU 占用（%，低者胜 = 能耗维）。 */
  cpuPct: number;
}

export interface WargameReport {
  perf: string | null;
  mem: string | null;
  energy: string | null;
  overall: string | null;
  rows: Array<{ dim: string; zh: string; winner: string | null; detail: string }>;
}

/** 三方对照战报（性能/内存/能耗；同负载时钟对齐下的实测比对）。 */
export function wargameReport(a: WargameSide, b: WargameSide): WargameReport {
  const sides = [a, b].filter((s) => s && s.id && Number.isFinite(s.fps) && Number.isFinite(s.memMB) && Number.isFinite(s.cpuPct));
  if (sides.length < 2) return { perf: null, mem: null, energy: null, overall: null, rows: [] };
  const perf = a.fps === b.fps ? null : a.fps > b.fps ? a.id : b.id;
  const mem = a.memMB === b.memMB ? null : a.memMB < b.memMB ? a.id : b.id;
  const energy = a.cpuPct === b.cpuPct ? null : a.cpuPct < b.cpuPct ? a.id : b.id;
  const wins = new Map<string, number>();
  for (const w of [perf, mem, energy]) if (w) wins.set(w, (wins.get(w) ?? 0) + 1);
  let overall: string | null = null;
  if (wins.size === 1) overall = [...wins.keys()][0]!;
  else if (wins.size > 0) {
    // 平分秋色 → 以性能维为帅（可解释的决胜规则）
    overall = perf ?? [...wins.keys()][0]!;
  }
  const rows = [
    { dim: "perf", zh: "性能（fps 高者胜）", winner: perf, detail: `${a.label} ${a.fps} vs ${b.label} ${b.fps}` },
    { dim: "mem", zh: "内存（MB 低者胜）", winner: mem, detail: `${a.label} ${a.memMB} vs ${b.label} ${b.memMB}` },
    { dim: "energy", zh: "能耗（CPU% 低者胜）", winner: energy, detail: `${a.label} ${a.cpuPct}% vs ${b.label} ${b.cpuPct}%` },
  ];
  return { perf, mem, energy, overall, rows };
}

/** 负载注入时钟对齐校验（验收：推演负载注入同步）。 */
export function clockAligned(t1: number, t2: number, tolMs = 50): boolean {
  return Math.abs(t1 - t2) <= tolMs;
}

// ---------------------------------------------------------------------------
// W-193 内存地平线（Memory Floor）—— 跨版本最低水位线（纯逻辑）
// ---------------------------------------------------------------------------

export interface FloorRecord {
  version: string;
  idleMB: number;
  at: number;
}

export const FLOOR_APPROACH_PCT = 0.15; // 逼近红线 = +15%

export type FloorVerdict = "breached" | "approaching" | "steady";

/** 虚线标注文案（验收：FLOOR 382MB @v1.4 式）。 */
export function floorLineLabel(r: FloorRecord): string {
  return `FLOOR ${Math.round(r.idleMB)}MB @${r.version}`;
}

export function currentFloor(records: FloorRecord[]): FloorRecord | null {
  const ok = records.filter((r) => r && r.version && Number.isFinite(r.idleMB) && r.idleMB > 0);
  if (ok.length === 0) return null;
  return ok.reduce((min, r) => (r.idleMB < min.idleMB ? r : min));
}

/**
 * 地平线更新：新版本实测击穿（更低）→ 更新水位；逼近（≤ floor × 1.15）→ 警报。
 * 判定可解释：返回 verdict 与依据。
 */
export function updateFloor(
  records: FloorRecord[],
  current: FloorRecord,
): { records: FloorRecord[]; verdict: FloorVerdict; floor: FloorRecord | null; reason: string } {
  const ok = records.filter((r) => r && r.version && Number.isFinite(r.idleMB) && r.idleMB > 0);
  const floor = currentFloor(ok);
  if (!floor) {
    return { records: [current, ...ok], verdict: "breached", floor: current, reason: "首条地平线基线入档" };
  }
  if (current.idleMB < floor.idleMB) {
    return { records: [current, ...ok], verdict: "breached", floor: current, reason: `${floorLineLabel(floor)} 被击穿 → 水位更新为 ${floorLineLabel(current)}` };
  }
  if (current.idleMB <= floor.idleMB * (1 + FLOOR_APPROACH_PCT)) {
    return { records: ok, verdict: "approaching", floor, reason: `距 ${floorLineLabel(floor)} 红线 ≤ +15%（当前 ${Math.round(current.idleMB)}MB）` };
  }
  return { records: ok, verdict: "steady", floor, reason: `高于 ${floorLineLabel(floor)} 安全线` };
}

// ---------------------------------------------------------------------------
// W-194 可复现指纹（Build Fingerprint）—— 内容寻址哈希树（纯逻辑）
// ---------------------------------------------------------------------------

export interface BuildFile {
  path: string;
  content: string;
}

export interface BuildFingerprint {
  label: string;
  files: number;
  root: string;
  leaves: Array<{ path: string; hash: string }>;
}

/** FNV-1a 64 内容哈希（确定性、内容寻址；BigInt 纯计算零依赖）。 */
export function fnv1a64(input: string): string {
  const enc = new TextEncoder().encode(input);
  let h = 0xcbf29ce484222325n;
  for (const b of enc) {
    h ^= BigInt(b);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return h.toString(16).padStart(16, "0");
}

/** 文件集 → 哈希树（叶 = hash(path + "\u0000" + content)，根 = 排序叶行哈希）。 */
export function fingerprintTree(label: string, files: BuildFile[]): BuildFingerprint {
  const ok = files.filter((f) => f && typeof f.path === "string" && f.path.length > 0 && typeof f.content === "string");
  const leaves = ok
    .map((f) => ({ path: f.path, hash: fnv1a64(`${f.path}\u0000${f.content}`) }))
    .sort((x, y) => (x.path < y.path ? -1 : x.path > y.path ? 1 : 0));
  const root = leaves.length === 0 ? fnv1a64("empty") : fnv1a64(leaves.map((l) => `${l.path}:${l.hash}`).join("\n"));
  return { label, files: leaves.length, root, leaves };
}

export interface FingerprintDiff {
  path: string;
  kind: "changed" | "only-a" | "only-b";
}

export interface FingerprintCompare {
  reproducible: boolean;
  label: string; // REPRODUCIBLE / NOT REPRODUCIBLE
  diffs: FingerprintDiff[];
}

/** 双构建指纹比对：一致 = 可复现；不一致如实 NOT REPRODUCIBLE + 差异文件清单。 */
export function compareFingerprints(a: BuildFingerprint, b: BuildFingerprint): FingerprintCompare {
  const am = new Map(a.leaves.map((l) => [l.path, l.hash]));
  const bm = new Map(b.leaves.map((l) => [l.path, l.hash]));
  const diffs: FingerprintDiff[] = [];
  for (const [p, h] of am) {
    const bh = bm.get(p);
    if (bh === undefined) diffs.push({ path: p, kind: "only-a" });
    else if (bh !== h) diffs.push({ path: p, kind: "changed" });
  }
  for (const p of bm.keys()) if (!am.has(p)) diffs.push({ path: p, kind: "only-b" });
  diffs.sort((x, y) => (x.path < y.path ? -1 : 1));
  const reproducible = diffs.length === 0 && a.files === b.files;
  return { reproducible, label: reproducible ? "REPRODUCIBLE" : "NOT REPRODUCIBLE", diffs };
}

// ---------------------------------------------------------------------------
// W-195 白纸视角（Fresh Eyes）—— 首跑状态专用沙盒（纯逻辑）
// ---------------------------------------------------------------------------

export interface FreshProfile {
  /** 零设置：settings 为空（非默认值、是「无」）。 */
  settings: null;
  /** 零历史：所有历史计数为 0。 */
  history: 0;
  /** 首次引导全流程开启。 */
  onboarding: true;
  /** 首跑标志（与真首跑 1:1）。 */
  firstRun: true;
  spawnedAt: number;
}

/** 白纸档案：与真首跑 1:1（零设置/零历史/全引导）。 */
export function freshProfile(now: number): FreshProfile {
  return { settings: null, history: 0, onboarding: true, firstRun: true, spawnedAt: now };
}

export interface FreshSession {
  id: string;
  profile: FreshProfile;
  /** 沙盒键空间前缀（退出即销毁的目标）。 */
  scope: string[];
  alive: boolean;
}

/** 生成白纸沙盒会话（scope = 受控键空间；退出销毁目标明确）。 */
export function spawnFresh(now: number): FreshSession {
  return {
    id: `fresh-${now}`,
    profile: freshProfile(now),
    scope: [`${NS}.fresh.state`, `${NS}.fresh.cache`],
    alive: true,
  };
}

/** 残留核查：销毁后 scope 内任何残留键都算失败（验收：退出即销毁零残留）。 */
export function residueOf(session: FreshSession, presentKeys: string[]): string[] {
  if (session.alive) return session.scope.filter((k) => presentKeys.includes(k)); // 未销毁的会话 scope 内有键属正常
  return session.scope.filter((k) => presentKeys.includes(k));
}

/** 销毁白纸沙盒：状态清零、会话终结（残留核查交 residueOf）。 */
export function destroyFresh(session: FreshSession): FreshSession {
  return { ...session, profile: { ...session.profile, settings: null, history: 0 }, alive: false };
}

// ---------------------------------------------------------------------------
// W-196 性能气象站（Perf Weather）—— 气象隐喻 + 30min 预报（纯逻辑）
// ---------------------------------------------------------------------------

export type Weather = "sunny" | "cloudy" | "rain" | "storm";

export const WEATHER_ZH: Record<Weather, string> = { sunny: "晴", cloudy: "多云", rain: "雨", storm: "风暴" };

/**
 * 气象阈值（与 N-19 性能 HUD 同源：S17 接线时 HUD 与本站共用此常量）。
 * 晴 = 空闲；多云 = 常态；雨 = 繁忙；风暴 = 过载。
 */
export const WEATHER_HUD = { cpuBusy: 45, cpuOver: 75, memBusy: 60, memOver: 85 } as const;

export interface WeatherSample {
  at: number;
  cpuPct: number;
  memPct: number;
}

/** 负载 → 气象（阈值同源 WEATHER_HUD）。 */
export function weatherOf(cpuPct: number, memPct: number): Weather {
  const cpu = Number.isFinite(cpuPct) ? cpuPct : 0;
  const mem = Number.isFinite(memPct) ? memPct : 0;
  if (cpu >= WEATHER_HUD.cpuOver && mem >= WEATHER_HUD.memOver) return "storm";
  if (cpu >= WEATHER_HUD.cpuOver || mem >= WEATHER_HUD.memOver) return "rain";
  if (cpu >= WEATHER_HUD.cpuBusy || mem >= WEATHER_HUD.memBusy) return "cloudy";
  return "sunny";
}

export interface WeatherForecast {
  weather: Weather;
  /** 预报置信度 [0,1]（规律样本不足 → 低置信 + 诚实标注）。 */
  confidence: number;
  samples: number;
  note: string;
}

/**
 * 未来 30 分钟预报：基于本地使用规律（同小时段历史多数天气）。
 * 娱乐性预报误差如实标注（样本 < 5 → 如实「样本不足」）。
 */
export function forecastWeather(history: WeatherSample[], now: number, horizonMin = 30): WeatherForecast {
  const target = new Date(now + horizonMin * 60_000);
  const hour = target.getHours();
  const sameHour = history.filter((s) => Number.isFinite(s.at) && Number.isFinite(s.cpuPct) && Number.isFinite(s.memPct) && new Date(s.at).getHours() === hour);
  if (sameHour.length < 5) {
    const cur = history.length > 0 ? history[history.length - 1]! : null;
    return {
      weather: cur ? weatherOf(cur.cpuPct, cur.memPct) : "sunny",
      confidence: sameHour.length / 5 * 0.5,
      samples: sameHour.length,
      note: "娱乐性预报：同时段样本不足，仅供参考",
    };
  }
  const counts = new Map<Weather, number>();
  for (const s of sameHour) {
    const w = weatherOf(s.cpuPct, s.memPct);
    counts.set(w, (counts.get(w) ?? 0) + 1);
  }
  let weather: Weather = "sunny";
  let best = -1;
  for (const [w, c] of counts) {
    if (c > best) {
      best = c;
      weather = w;
    }
  }
  return {
    weather,
    confidence: clamp(best / sameHour.length, 0, 1),
    samples: sameHour.length,
    note: `娱乐性预报：基于本地 ${sameHour.length} 条同时段规律`,
  };
}

// ---------------------------------------------------------------------------
// W-197 工程星象（Eng Zodiac）—— 12 指标星象盘（纯逻辑）
// ---------------------------------------------------------------------------

export type ZodiacMetricKey =
  | "testCoverage"
  | "docRatio"
  | "depHealth"
  | "gatePass"
  | "typeSafety"
  | "lintClean"
  | "bootPerf"
  | "memFloor"
  | "frameRate"
  | "crashFree"
  | "reproBuild"
  | "a11y";

export interface ZodiacStarDef {
  key: ZodiacMetricKey;
  /** IAU 星座名（仅词表标签，不迷信化）。 */
  constellation: string;
  zh: string;
  metricZh: string;
}

export const ZODIAC_WHEEL: ZodiacStarDef[] = [
  { key: "testCoverage", constellation: "Lyra", zh: "天琴座", metricZh: "测试覆盖" },
  { key: "docRatio", constellation: "Corona", zh: "北冕座", metricZh: "文档比率" },
  { key: "depHealth", constellation: "Hydra", zh: "长蛇座", metricZh: "依赖健康" },
  { key: "gatePass", constellation: "Libra", zh: "天秤座", metricZh: "门禁通过率" },
  { key: "typeSafety", constellation: "Orion", zh: "猎户座", metricZh: "类型安全" },
  { key: "lintClean", constellation: "Vela", zh: "船帆座", metricZh: "静态检查" },
  { key: "bootPerf", constellation: "Pavo", zh: "孔雀座", metricZh: "启动性能" },
  { key: "memFloor", constellation: "Fornax", zh: "天炉座", metricZh: "内存水位" },
  { key: "frameRate", constellation: "Aquila", zh: "天鹰座", metricZh: "帧率" },
  { key: "crashFree", constellation: "Scutum", zh: "盾牌座", metricZh: "崩溃免疫" },
  { key: "reproBuild", constellation: "Horologium", zh: "时钟座", metricZh: "可复现构建" },
  { key: "a11y", constellation: "Equuleus", zh: "小马座", metricZh: "无障碍" },
];

export const ZODIAC_DARK_LINE = 0.6; // 亮度 < 0.6 = 暗星（点击展开明细）

export interface ZodiacMetric {
  key: ZodiacMetricKey;
  /** 健康度 [0,1]（同源真实门禁数据）。 */
  value: number;
  detail?: string;
}

export interface ZodiacStar extends ZodiacStarDef {
  brightness: number;
  dark: boolean;
  value: number;
  detail: string;
}

/** 12 指标 → 星象盘（亮度=健康度；暗星标暗可展开）。 */
export function zodiacOf(metrics: ZodiacMetric[]): ZodiacStar[] {
  const byKey = new Map<ZodiacMetricKey, ZodiacMetric>();
  for (const m of metrics) {
    if (m && ZODIAC_WHEEL.some((w) => w.key === m.key) && Number.isFinite(m.value)) byKey.set(m.key, m);
  }
  return ZODIAC_WHEEL.map((w) => {
    const m = byKey.get(w.key);
    const value = m ? clamp(m.value, 0, 1) : 0;
    const dark = !m || value < ZODIAC_DARK_LINE;
    return { ...w, brightness: value, value, dark, detail: m?.detail ?? (m ? "" : "暂无同源门禁数据（等待 nova://quality-zodiac 摄入）") };
  });
}

/** 暗星展开明细（点击暗星 → 指标名 + 数值 + 明细）。 */
export function starDetail(star: ZodiacStar): string {
  return `${star.constellation}（${star.zh}）· ${star.metricZh}：亮度 ${(star.brightness * 100).toFixed(0)}%${star.detail ? ` — ${star.detail}` : ""}`;
}

// ---------------------------------------------------------------------------
// W-198 发版礼炮（Release Cannon）—— 里程碑墙 + 礼炮仪式（纯逻辑）
// ---------------------------------------------------------------------------

export const CANNON_MS = 2500; // 礼炮 2.5s（可跳过）

export interface Milestone {
  version: string;
  at: number;
  note?: string;
}

/** 礼炮触发校验：仅 dev 构建或手动触发（验收：不打扰用户）。 */
export function cannonAllowed(opts: { dev: boolean; manual: boolean }): boolean {
  return opts.dev === true || opts.manual === true;
}

/** 里程碑墙（版本去重、按时间降序）。 */
export function addMilestone(milestones: Milestone[], m: Milestone): Milestone[] {
  const ok = milestones.filter((x) => x && x.version);
  if (!m.version || ok.some((x) => x.version === m.version)) return ok;
  return [{ version: m.version, at: m.at, note: m.note }, ...ok].sort((a, b) => b.at - a.at);
}

/** 历史铭牌行（持久化呈现；reduce-motion 降级为静态铭牌）。 */
export function plaqueRows(milestones: Milestone[]): string[] {
  return [...milestones]
    .sort((a, b) => b.at - a.at)
    .map((m) => `${m.version} · ${new Date(m.at).toISOString().slice(0, 10)}${m.note ? ` · ${m.note}` : ""}`);
}

/** 礼炮渲染制式：动效可用 → 动画礼炮；降级 → 静态铭牌。 */
export function cannonMode(motionOkNow: boolean): "animated" | "static" {
  return motionOkNow ? "animated" : "static";
}

// ---------------------------------------------------------------------------
// W-199 性能不朽档案（Golden Archive）—— 全指标历史最佳（纯逻辑）
// ---------------------------------------------------------------------------

export type MetricDirection = "lower" | "higher";

/** 指标方向（lower = 越低越好，如启动 ms；higher = 越高越好，如 fps）。 */
export const METRIC_DIRECTIONS: Record<string, { dir: MetricDirection; unit: string; zh: string }> = {
  bootMs: { dir: "lower", unit: "s", zh: "启动" },
  idleMemMB: { dir: "lower", unit: "MB", zh: "空载内存" },
  fps: { dir: "higher", unit: "fps", zh: "帧率" },
  testPass: { dir: "higher", unit: "cases", zh: "测试通过" },
};

export interface ArchiveEntry {
  metric: string;
  value: number;
  version: string;
  at: number;
  /** 机器上下文（严格同条件比对的维度之一）。 */
  machine: string;
  /** 场景上下文（同机同场景才比）。 */
  scenario: string;
}

/** 同条件判定：同机器 + 同场景（严格；否则不比）。 */
export function sameCondition(a: ArchiveEntry, b: ArchiveEntry): boolean {
  return a.machine === b.machine && a.scenario === b.scenario;
}

export function isBetter(metric: string, candidate: number, incumbent: number): boolean {
  const def = METRIC_DIRECTIONS[metric];
  if (!def) return false;
  return def.dir === "lower" ? candidate < incumbent : candidate > incumbent;
}

/** 致敬文案（NEW RECORD 3.1s BOOT 式）。 */
export function recordSalute(metric: string, value: number): string {
  if (metric === "bootMs") return `NEW RECORD ${(value / 1000).toFixed(1)}s BOOT`;
  const def = METRIC_DIRECTIONS[metric];
  if (!def) return `NEW RECORD ${value} ${metric}`;
  return `NEW RECORD ${value}${def.unit === "cases" ? "" : def.unit} ${def.zh}`;
}

export interface ArchiveUpdate {
  entries: ArchiveEntry[];
  record: boolean;
  salute: string | null;
  reason: string;
}

/**
 * 档案更新：破纪录判定严格（同 metric + 同机器 + 同场景才比）；
 * 不同条件的成绩分开入档不跨比（诚实口径）。
 */
export function archiveUpdate(entries: ArchiveEntry[], candidate: ArchiveEntry): ArchiveUpdate {
  const ok = entries.filter((e) => e && e.metric && Number.isFinite(e.value) && e.version && e.machine && e.scenario);
  if (!candidate.metric || !Number.isFinite(candidate.value) || !candidate.version || !candidate.machine || !candidate.scenario) {
    return { entries: ok, record: false, salute: null, reason: "候选成绩字段不全，如实不入档" };
  }
  const incumbent = ok.find((e) => e.metric === candidate.metric && sameCondition(e, candidate));
  if (!incumbent) {
    const next = [candidate, ...ok];
    return { entries: next, record: true, salute: recordSalute(candidate.metric, candidate.value), reason: "该条件首个档案（原点即纪录）" };
  }
  if (isBetter(candidate.metric, candidate.value, incumbent.value)) {
    const next = ok.map((e) => (e === incumbent ? candidate : e));
    return { entries: next, record: true, salute: recordSalute(candidate.metric, candidate.value), reason: `超越 ${incumbent.version} 的 ${incumbent.value}（同机同场景）` };
  }
  return { entries: ok, record: false, salute: null, reason: `未超越 ${incumbent.version} 的 ${incumbent.value}（同机同场景，不跨比）` };
}

/** 档案导出（JSON 文本；验收：档案可导出）。 */
export function exportArchive(entries: ArchiveEntry[]): string {
  return JSON.stringify({ kind: "nova-quality-archive", v: 1, entries }, null, 2);
}

// ---------------------------------------------------------------------------
// W-200 测试战绩册（Test Battle Pass）—— 里程碑徽章功勋墙（纯逻辑）
// ---------------------------------------------------------------------------

/** 里程碑（用例数）：逐级解锁徽章。 */
export const BATTLE_MILESTONES = [1000, 1200, 1500] as const;

export interface DomainTests {
  domain: string;
  passed: number;
  failed: number;
}

export interface BadgeRow {
  milestone: number;
  unlocked: boolean;
}

/** 通过用例数 → 徽章解锁态。 */
export function badgesOf(totalPassed: number): BadgeRow[] {
  return BATTLE_MILESTONES.map((m) => ({ milestone: m, unlocked: totalPassed >= m }));
}

/** 下一枚待解锁徽章（全解锁 → null）。 */
export function nextBadge(totalPassed: number): number | null {
  for (const m of BATTLE_MILESTONES) if (totalPassed < m) return m;
  return null;
}

export interface HonorRow {
  domain: string;
  total: number;
  failed: number;
  /** 功勋占比 [0,1]。 */
  share: number;
}

/** 各域测试分布功勋墙（按总测试数降序）。 */
export function honorWall(domains: DomainTests[]): HonorRow[] {
  const ok = domains.filter((d) => d && d.domain && Number.isFinite(d.passed) && d.passed >= 0 && Number.isFinite(d.failed) && d.failed >= 0);
  const total = ok.reduce((s, d) => s + d.passed + d.failed, 0);
  return ok
    .map((d) => ({ domain: d.domain, total: d.passed + d.failed, failed: d.failed, share: total > 0 ? (d.passed + d.failed) / total : 0 }))
    .sort((a, b) => b.total - a.total);
}

/** 战绩播报（hub 摘要行；数据同源 vitest/cargo test 真实结果）。 */
export function battleSummary(total: { passed: number; failed: number }): string {
  const t = total.passed + total.failed;
  if (t === 0) return "暂无测试结果（等待 nova://quality-tests 摄入真实 vitest/cargo 数据）";
  const rate = Math.round((total.passed / t) * 100);
  const badge = [...badgesOf(total.passed)].reverse().find((b) => b.unlocked);
  return `${total.passed} 通过 / ${total.failed} 失败（${rate}%）${badge ? ` · ${badge.milestone} 徽章在手` : ""}`;
}

// ---------------------------------------------------------------------------
// 行为层（DOM 叠层 + 事件摄入；非 DOM 环境安全 no-op）
// ---------------------------------------------------------------------------

let bag: Array<() => void> = [];
let active = false;

function esc(s: string): string {
  return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}

function ensureStyle(): void {
  if (typeof document === "undefined") return;
  if (document.getElementById("nova-quality-style")) return;
  const st = document.createElement("style");
  st.id = "nova-quality-style";
  st.textContent = `
.nova-quality-root{position:fixed;inset:auto auto 16px 16px;z-index:2147483000;display:flex;flex-direction:column;gap:8px;font:12px/1.5 Consolas,monospace;color:#e8e8ec;background:rgba(18,18,24,.92);border:1px solid rgba(255,255,255,.14);border-radius:10px;box-shadow:0 8px 28px rgba(0,0,0,.45);padding:10px 12px;max-width:400px}
.nova-quality-root[hidden]{display:none}
.nova-quality-h{font-size:11px;letter-spacing:.08em;text-transform:uppercase;color:#9aa0b0;margin:2px 0 6px}
.nova-quality-tabs{display:flex;gap:4px;flex-wrap:wrap}
.nova-quality-tab{font:11px Consolas,monospace;padding:3px 8px;border-radius:6px;border:1px solid rgba(255,255,255,.16);background:transparent;color:#c8ccd8;cursor:pointer}
.nova-quality-tab[aria-selected="true"]{background:#00b894;color:#fff;border-color:#00b894}
.nova-quality-body{max-height:320px;overflow:auto;display:flex;flex-direction:column;gap:6px;min-width:300px}
.nova-quality-muted{color:#8b91a3}
.nova-quality-mono{font:11px Consolas,monospace;white-space:pre-wrap}
.nova-quality-card{border:1px solid rgba(255,255,255,.1);border-radius:8px;padding:6px 8px}
.nova-quality-row{display:flex;align-items:center;gap:6px}
.nova-quality-bar{flex:1;height:6px;border-radius:3px;background:rgba(255,255,255,.12);overflow:hidden}
.nova-quality-fill{height:100%;border-radius:3px;background:#00b894}
.nova-quality-bad{color:#ff6b6b}
.nova-quality-ok{color:#69db7c}
.nova-quality-btn{font:11px Consolas,monospace;padding:3px 10px;border-radius:6px;border:1px solid rgba(255,255,255,.2);background:#262a36;color:#e8e8ec;cursor:pointer}
.nova-quality-btn:hover{background:#31364a}
/* W-188 能效标签（欧盟能效视觉） */
.nova-quality-ebar{display:flex;align-items:center;gap:6px;font:11px Consolas,monospace}
.nova-quality-egrade{width:22px;height:14px;border-radius:3px;display:inline-flex;align-items:center;justify-content:center;font-weight:700;color:#fff}
/* W-191 砚台墨格 */
.nova-quality-ink{display:flex;gap:1px;align-items:flex-end;height:34px}
.nova-quality-ink i{flex:1;min-height:2px;border-radius:1px 1px 0 0;background:#00b894}
/* W-197 星象盘 */
.nova-quality-zodiac{display:grid;grid-template-columns:1fr 1fr;gap:3px}
.nova-quality-star{display:flex;align-items:center;gap:4px;font:10px Consolas,monospace;color:#c8ccd8;cursor:default}
.nova-quality-dot{width:8px;height:8px;border-radius:50%;background:#ffe97a;box-shadow:0 0 6px 1px rgba(255,233,122,.8)}
.nova-quality-dot.nova-quality-star-dark{background:#4a4f63;box-shadow:none}
/* W-198 礼炮 */
.nova-quality-cannon{position:fixed;inset:0;z-index:2147483500;pointer-events:none;display:flex;align-items:center;justify-content:center;font:16px Consolas,monospace;letter-spacing:.2em;color:#ffe97a;background:radial-gradient(circle at 50% 60%,rgba(255,233,122,.12),transparent 60%)}
.nova-quality-shell{position:absolute;width:3px;height:3px;border-radius:50%;background:#ffe97a;animation:nova-quality-burst 1.2s ease-out forwards}
@keyframes nova-quality-burst{from{opacity:1;transform:translate(0,0) scale(1)}to{opacity:0;transform:translate(var(--dx,0),var(--dy,-160px)) scale(.4)}}
/* 降级：reduce-motion / safeMode / static → 动效归零（礼炮降静态铭牌） */
[data-reduce-motion="true"] .nova-quality-root,[data-safe-mode="true"] .nova-quality-root,[data-static-mode="true"] .nova-quality-root{transition:none;animation:none}
[data-reduce-motion="true"] .nova-quality-shell,[data-safe-mode="true"] .nova-quality-shell,[data-static-mode="true"] .nova-quality-shell{animation:none;opacity:0}
`;
  document.head.appendChild(st);
  bag.push(() => st.remove());
}

// --- 行为层状态（摄入镜像 + 持久化） ---

type TabId = "energy" | "nest" | "replay" | "ink" | "wargame" | "floor" | "fingerprint" | "fresh" | "weather" | "zodiac" | "cannon" | "archive" | "battlepass";

let panel: HTMLElement | null = null;
let curTab: TabId = "zodiac";
let energySamples: EnergySample[] = [];
let nestEntries: NestEntry[] = [];
let lastReplay: { dump: CrashDump; verdict: ReplayVerdict } | null = null;
let frameSamples: FrameSample[] = [];
let lastWargame: { a: WargameSide; b: WargameSide; report: WargameReport; aligned: boolean } | null = null;
let lastFingerprint: { a: BuildFingerprint; b: BuildFingerprint; compare: FingerprintCompare } | null = null;
let freshSession: FreshSession | null = null;
let weatherHistory: WeatherSample[] = [];
let lastWeather: { now: Weather; forecast: WeatherForecast } | null = null;
let zodiacMetrics: ZodiacMetric[] = [];
let cannonEl: HTMLElement | null = null;
let cannonTimer: number | null = null;
let cannonSkip: (() => void) | null = null;

function loadState(): void {
  energySamples = lsGet<Array<{ axis: string; cpuPct: number; ms: number }>>(`${NS}.energy.v1`, []).map((x) => x as EnergySample);
  nestEntries = lsGet<NestEntry[]>(`${NS}.nest.v1`, []);
  frameSamples = evictInk(lsGet<FrameSample[]>(`${NS}.ink.v1`, []), Date.now());
  weatherHistory = lsGet<WeatherSample[]>(`${NS}.weather.v1`, []);
  zodiacMetrics = lsGet<ZodiacMetric[]>(`${NS}.zodiac.v1`, []);
}

function gradeColor(g: EnergyGrade): string {
  const m: Record<string, string> = { A: "#00b894", B: "#55efc4", C: "#ffe97a", D: "#fdcb6e", E: "#fd9e3d", F: "#e17055", G: "#d63031", "N/A": "#4a4f63" };
  return m[g] ?? "#4a4f63";
}

function renderTabBody(): string {
  const now = Date.now();
  if (curTab === "energy") {
    const r = energyLabel(energySamples);
    const rows = r.axes
      .map((a) => {
        const zh = a.axis === "idle" ? "空载" : a.axis === "boot" ? "启动" : "动效";
        return `<div class="nova-quality-ebar"><span style="width:28px">${zh}</span><span class="nova-quality-egrade" style="background:${gradeColor(a.grade)}">${a.grade}</span><span class="nova-quality-muted">${a.grade === "N/A" ? "无实测数据（如实 N/A）" : `${a.mcpuSec} mCPU·s`}</span></div>`;
      })
      .join("");
    const bands = ENERGY_BANDS.slice(0, 6).map((b) => `${b.grade}≤${b.maxMCpuSec}`).join(" / ");
    return `<div class="nova-quality-h">能效标签 · 总评 ${r.overall}${r.complete ? "" : "（缺维度如实 N/A）"}</div>${rows}<div class="nova-quality-muted">公开阈值（mCPU·s）：${bands} / G&gt;3840</div>`;
  }
  if (curTab === "nest") {
    if (nestEntries.length === 0) return `<div class="nova-quality-muted">空态病房：暂无入眠名单（等待 nova://quality-nest-report 摄入；空闲 ≥ ${NEST_IDLE_MIN}min 自动入眠）。</div>`;
    const rows = nestList(nestEntries)
      .slice(0, 10)
      .map((e) => {
        const rr = releaseRatio(e.worksetMB, e.swappedMB);
        return `<div class="nova-quality-card"><span class="${e.asleep ? "nova-quality-ok" : "nova-quality-muted"}">${e.asleep ? "入眠" : "清醒"}</span> ${esc(e.id)} · 空闲 ${e.idleMin}min · 工作集 ${e.worksetMB}MB → ${e.swappedMB}MB（释放 ${(rr * 100).toFixed(0)}%${rr >= NEST_RELEASE_TARGET ? " ✓" : " ✗"}）</div>`;
      })
      .join("");
    return `<div class="nova-quality-h">睡眠孵化器 · ${nestEntries.filter((e) => e.asleep).length} 入眠</div>${rows}<div class="nova-quality-muted">换出经系统层执行；手动唤醒优先级最高</div>`;
  }
  if (curTab === "replay") {
    const isDev = typeof import.meta !== "undefined" && (import.meta as { env?: { DEV?: boolean } }).env?.DEV === true;
    if (!replayVisible(isDev)) return `<div class="nova-quality-muted">崩溃重放器仅 dev 构建可见（验收：仅 dev）。</div>`;
    if (!lastReplay) return `<div class="nova-quality-muted">空态重放器：暂无崩溃转储（等待 nova://quality-dump 摄入，M-53 同源）。</div>`;
    const v = lastReplay.verdict;
    const cls = v.kind === "reproduced" ? "nova-quality-ok" : v.kind === "not-deterministic" ? "nova-quality-muted" : "nova-quality-bad";
    return `<div class="nova-quality-h">崩溃单步重放 · ${esc(lastReplay.dump.id)}</div><div class="nova-quality-card"><span class="${cls}">${esc(v.label)}</span> · ${v.kind === "reproduced" ? `第 ${v.atStep}/${v.steps} 步复现崩溃点` : v.kind === "diverged" ? `第 ${v.atStep} 步发散（期望 ${v.expect}，实得 ${v.got}）` : "UI/时序类崩溃，如实不可确定性重放"}</div>`;
  }
  if (curTab === "ink") {
    const r = inkGrid(frameSamples, now);
    if (r.kept === 0) return `<div class="nova-quality-muted">空态砚台：暂无帧耗时采样（等待 nova://quality-frames 摄入；rAF 计时戳零干扰）。</div>`;
    const bars = r.cells
      .map((c) => {
        const h = Math.max(c.peak * 100, c.count > 0 ? 6 : 2);
        const color = c.hotspot ? "#ff6b6b" : c.peak > 0.5 ? "#fdcb6e" : "#00b894";
        return `<i style="height:${Math.min(h, 100)}%;background:${color}"></i>`;
      })
      .join("");
    return `<div class="nova-quality-h">GPU 诊断砚 · ${r.kept} 帧 / 保留 5min</div><div class="nova-quality-ink">${bars}</div><div class="nova-quality-muted">最慢 ${r.worstMs}ms · 掉帧热点 ${r.hotspots} 格（浓墨=真实掉帧，阈值 ${INK_DROP_MS.toFixed(1)}ms）${r.evicted > 0 ? ` · 剔除过期 ${r.evicted}` : ""}</div>`;
  }
  if (curTab === "wargame") {
    if (!lastWargame) return `<div class="nova-quality-muted">空态战报：暂无推演数据（等待 nova://quality-wargame 摄入 A/B 实测；N-35 分身对齐）。</div>`;
    const w = lastWargame;
    const rows = w.report.rows
      .map((x) => `<div class="nova-quality-card"><span class="nova-quality-muted">${esc(x.zh)}</span> · ${esc(x.detail)} → <span class="${x.winner ? "nova-quality-ok" : "nova-quality-muted"}">${x.winner ? esc(x.winner) : "平手"}</span></div>`)
      .join("");
    return `<div class="nova-quality-h">军棋推演 · ${w.aligned ? "负载时钟对齐 ✓" : "负载时钟未对齐 ✗（战报如实降置信）"}</div>${rows}<div class="nova-quality-muted">总评：${w.report.overall ? esc(w.report.overall) : "无显著胜者"}（非对照期分身零额外开销）</div>`;
  }
  if (curTab === "floor") {
    const records = lsGet<FloorRecord[]>(`${NS}.floor.v1`, []);
    const floor = currentFloor(records);
    if (!floor) return `<div class="nova-quality-muted">空态地平线：暂无版本水位记录（等待 nova://quality-floor 喂入各版本空载实测）。</div>`;
    const lines = [...records]
      .sort((a, b) => a.at - b.at)
      .slice(-8)
      .map((rec) => `<div class="nova-quality-mono">${esc(rec.version)} · ${Math.round(rec.idleMB)}MB ${rec.idleMB <= floor.idleMB * (1 + FLOOR_APPROACH_PCT) ? (rec.idleMB === floor.idleMB ? "← 水位线" : "← 逼近红线") : ""}</div>`)
      .join("");
    return `<div class="nova-quality-h">内存地平线 · ${esc(floorLineLabel(floor))}</div>${lines}<div class="nova-quality-muted">击穿（更低）更新水位；逼近（+15%）警报</div>`;
  }
  if (curTab === "fingerprint") {
    if (!lastFingerprint) return `<div class="nova-quality-muted">空态指纹：暂无双构建对照（等待 nova://quality-build 喂入两次构建产物清单）。</div>`;
    const f = lastFingerprint;
    const cls = f.compare.reproducible ? "nova-quality-ok" : "nova-quality-bad";
    const diffs = f.compare.diffs
      .slice(0, 8)
      .map((d) => `<div class="nova-quality-mono">${esc(d.path)} · ${d.kind === "changed" ? "内容差异" : d.kind === "only-a" ? "仅 A 构建" : "仅 B 构建"}</div>`)
      .join("");
    return `<div class="nova-quality-h">可复现指纹</div><div class="nova-quality-card"><span class="${cls}">${f.compare.label}</span> · A:${esc(f.a.root.slice(0, 8))} vs B:${esc(f.b.root.slice(0, 8))}（各 ${f.a.files} 文件）</div>${diffs || '<div class="nova-quality-muted">差异文件：无</div>'}`;
  }
  if (curTab === "fresh") {
    if (!freshSession) return `<div class="nova-quality-muted">空态白纸：暂无白纸会话（nova fresh-eyes 开发者命令 / 面板按钮派发 nova://quality-fresh）。</div>`;
    const p = freshSession.profile;
    return `<div class="nova-quality-h">白纸视角 · ${esc(freshSession.id)}${freshSession.alive ? "" : "（已销毁）"}</div><div class="nova-quality-card">settings=${p.settings === null ? "null（零设置）" : esc(String(p.settings))} · history=${p.history} · onboarding=${p.onboarding} · firstRun=${p.firstRun}</div><div class="nova-quality-muted">与真首跑 1:1；退出即销毁零残留（scope 残留核查已备）</div>`;
  }
  if (curTab === "weather") {
    if (!lastWeather) return `<div class="nova-quality-muted">空态气象站：暂无负载样本（等待 nova://quality-weather 摄入）。</div>`;
    const w = lastWeather;
    return `<div class="nova-quality-h">性能气象站 · 当前 ${WEATHER_ZH[w.now]}</div><div class="nova-quality-card">未来 30 分钟预报：${WEATHER_ZH[w.forecast.weather]}（置信 ${(w.forecast.confidence * 100).toFixed(0)}% · ${w.forecast.samples} 样本）</div><div class="nova-quality-muted">${esc(w.forecast.note)}（阈值与 N-19 HUD 同源）</div>`;
  }
  if (curTab === "zodiac") {
    const stars = zodiacOf(zodiacMetrics);
    const rows = stars
      .map((s) => `<div class="nova-quality-star" data-star="${esc(s.key)}"><span class="nova-quality-dot${s.dark ? " nova-quality-star-dark" : ""}"></span>${esc(s.constellation)} ${esc(s.zh)} · ${esc(s.metricZh)} ${(s.brightness * 100).toFixed(0)}%</div>`)
      .join("");
    const dark = stars.filter((s) => s.dark).length;
    return `<div class="nova-quality-h">工程星象 · ${stars.length} 星（暗星 ${dark}，点击展开明细）</div><div class="nova-quality-zodiac">${rows}</div><div class="nova-quality-muted">数据同源真实门禁（U-24/Z-63）；星座仅取 IAU 词表</div>`;
  }
  if (curTab === "cannon") {
    const milestones = lsGet<Milestone[]>(`${NS}.plaque.v1`, []);
    const rows = plaqueRows(milestones)
      .slice(0, 8)
      .map((x) => `<div class="nova-quality-mono">${esc(x)}</div>`)
      .join("");
    return `<div class="nova-quality-h">发版礼炮 · 里程碑墙</div>${rows || '<div class="nova-quality-muted">暂无历史铭牌（发版事件经 nova://quality-release 喂入）</div>'}<div class="nova-quality-muted">礼炮仅 dev/手动触发；reduce-motion 降静态铭牌</div>`;
  }
  if (curTab === "archive") {
    const entries = lsGet<ArchiveEntry[]>(`${NS}.archive.v1`, []);
    if (entries.length === 0) return `<div class="nova-quality-muted">空态档案馆：暂无历史最佳（候选成绩经 nova://quality-archive 喂入）。</div>`;
    const rows = entries
      .slice(0, 10)
      .map((e) => `<div class="nova-quality-card">${esc(e.metric)} · <span class="nova-quality-ok">${e.value}</span> @${esc(e.version)}（${esc(e.machine)} / ${esc(e.scenario)}）</div>`)
      .join("");
    return `<div class="nova-quality-h">性能不朽档案 · ${entries.length} 项最佳</div>${rows}<div class="nova-quality-muted">破纪录判定严格（同机同场景才比）；可导出</div>`;
  }
  const tests = lsGet<{ total: { passed: number; failed: number }; domains: DomainTests[] }>(`${NS}.tests.v1`, { total: { passed: 0, failed: 0 }, domains: [] });
  const badges = badgesOf(tests.total.passed)
    .map((b) => `<span class="nova-quality-card" style="padding:2px 6px;${b.unlocked ? "color:#ffe97a" : "color:#4a4f63"}">${b.unlocked ? "★" : "☆"} ${b.milestone}</span>`)
    .join("");
  const wall = honorWall(tests.domains)
    .slice(0, 10)
    .map((h) => `<div class="nova-quality-row"><span style="width:64px" class="nova-quality-muted">${esc(h.domain)}</span><span class="nova-quality-bar"><span class="nova-quality-fill" style="width:${Math.round(h.share * 100)}%"></span></span><span class="nova-quality-mono">${h.total}${h.failed > 0 ? ` ✗${h.failed}` : ""}</span></div>`)
    .join("");
  return `<div class="nova-quality-h">测试战绩册 · ${esc(battleSummary(tests.total))}</div><div class="nova-quality-row" style="gap:4px">${badges}</div>${wall || '<div class="nova-quality-muted">暂无各域分布</div>'}<div class="nova-quality-muted">战绩同源 vitest/cargo 真实结果；徽章本地渲染零网络</div>`;
}

function renderPanel(): void {
  if (!panel) return;
  const tabs: Array<{ id: TabId; zh: string; wid: string }> = [
    { id: "energy", zh: "能效", wid: "W-188" },
    { id: "nest", zh: "孵化", wid: "W-189" },
    { id: "replay", zh: "重放", wid: "W-190" },
    { id: "ink", zh: "砚台", wid: "W-191" },
    { id: "wargame", zh: "推演", wid: "W-192" },
    { id: "floor", zh: "地平线", wid: "W-193" },
    { id: "fingerprint", zh: "指纹", wid: "W-194" },
    { id: "fresh", zh: "白纸", wid: "W-195" },
    { id: "weather", zh: "气象", wid: "W-196" },
    { id: "zodiac", zh: "星象", wid: "W-197" },
    { id: "cannon", zh: "礼炮", wid: "W-198" },
    { id: "archive", zh: "档案", wid: "W-199" },
    { id: "battlepass", zh: "战绩", wid: "W-200" },
  ];
  panel.innerHTML = `
<div class="nova-quality-h">工程收官体检中心 · AI-16（域16 W-188…W-200）</div>
<div class="nova-quality-tabs">${tabs.map((t) => `<button class="nova-quality-tab" role="tab" aria-selected="${curTab === t.id}" data-tab="${t.id}">${esc(t.zh)} ${t.wid}</button>`).join("")}</div>
<div class="nova-quality-body">${renderTabBody()}</div>
<div class="nova-quality-row" style="justify-content:space-between">
<button class="nova-quality-btn" id="nova-quality-export">导出报告</button>
<button class="nova-quality-btn" id="nova-quality-fresh-btn">白纸视角</button>
<button class="nova-quality-btn" id="nova-quality-cannon-btn">礼炮演练</button>
<button class="nova-quality-btn" id="nova-quality-close">收起</button>
</div>`;
  panel.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((btn) => {
    btn.addEventListener("click", () => {
      curTab = (btn.dataset.tab as TabId) ?? "zodiac";
      renderPanel();
    });
  });
  panel.querySelectorAll<HTMLElement>("[data-star]").forEach((el) => {
    // 暗星点击展开明细（W-197 验收）
    el.addEventListener("click", () => {
      const star = zodiacOf(zodiacMetrics).find((s) => s.key === el.dataset.star);
      if (star && star.dark) {
        el.title = starDetail(star);
        qualityEvent("zodiac-star-detail", { key: star.key, detail: starDetail(star) });
      }
    });
  });
  panel.querySelector("#nova-quality-close")?.addEventListener("click", () => togglePanel(false));
  panel.querySelector("#nova-quality-export")?.addEventListener("click", exportAll);
  panel.querySelector("#nova-quality-fresh-btn")?.addEventListener("click", () => {
    if (!flagOn("W-195")) return; // 白纸默认 dev/显式开启
    freshSession = spawnFresh(Date.now());
    curTab = "fresh";
    renderPanel();
    qualityEvent("fresh", { id: freshSession.id }); // S17 → 零设置零历史沙盒首跑
  });
  panel.querySelector("#nova-quality-cannon-btn")?.addEventListener("click", () => {
    // 手动触发（cannonAllowed manual=true；不打扰用户）
    if (flagOn("W-198")) fireCannon({ dev: false, manual: true });
  });
}

function exportAll(): void {
  const now = Date.now();
  const report = {
    kind: "nova-quality-report",
    v: 1,
    energy: energyLabel(energySamples),
    nest: nestList(nestEntries).map((e) => ({ id: e.id, release: releaseRatio(e.worksetMB, e.swappedMB) })),
    ink: inkGrid(frameSamples, now),
    wargame: lastWargame?.report ?? null,
    floor: currentFloor(lsGet<FloorRecord[]>(`${NS}.floor.v1`, [])),
    fingerprint: lastFingerprint?.compare ?? null,
    weather: lastWeather,
    zodiac: zodiacOf(zodiacMetrics),
    archive: lsGet<ArchiveEntry[]>(`${NS}.archive.v1`, []),
    tests: lsGet(`${NS}.tests.v1`, null),
  };
  qualityEvent("report-export", report);
  if (typeof console !== "undefined") console.info("[nova-quality] 报告导出", JSON.stringify(report));
}

function togglePanel(force?: boolean): void {
  if (typeof document === "undefined") return;
  const show = force ?? panel == null;
  if (show && !panel) {
    panel = document.createElement("div");
    panel.className = "nova-quality-root";
    document.body.appendChild(panel);
  }
  if (panel) panel.hidden = !show;
  if (show) renderPanel();
}

// --- W-198 礼炮（仅 dev/手动；2.5s 可跳；reduce-motion 静态铭牌） ---

function fireCannon(opts: { dev: boolean; manual: boolean }): void {
  if (!cannonAllowed(opts)) return; // 验收：不打扰用户
  if (typeof document === "undefined") return;
  stopCannon();
  const mode = cannonMode(motionOK());
  const milestones = lsGet<Milestone[]>(`${NS}.plaque.v1`, []);
  const latest = milestones[0];
  cannonEl = document.createElement("div");
  cannonEl.className = "nova-quality-cannon";
  if (mode === "animated") {
    cannonEl.innerHTML = `<div>RELEASE CANNON${latest ? ` · ${esc(latest.version)}` : ""}</div>`;
    for (let i = 0; i < 24; i++) {
      const shell = document.createElement("span");
      shell.className = "nova-quality-shell";
      const ang = (Math.PI * 2 * i) / 24 + Math.random() * 0.3;
      shell.style.left = `${48 + Math.cos(ang) * 4}%`;
      shell.style.top = `${58 + Math.sin(ang) * 4}%`;
      shell.style.setProperty("--dx", `${Math.round(Math.cos(ang) * 220)}px`);
      shell.style.setProperty("--dy", `${Math.round(Math.sin(ang) * 220 - 60)}px`);
      cannonEl.appendChild(shell);
    }
  } else {
    // reduce-motion / safeMode / static → 静态铭牌（降级）
    cannonEl.innerHTML = `<div style="border:1px solid rgba(255,233,122,.5);border-radius:8px;padding:14px 22px;background:rgba(18,18,24,.9)">RELEASE PLAQUE${latest ? ` · ${esc(latest.version)}` : ""}</div>`;
  }
  document.body.appendChild(cannonEl);
  const skip = (): void => stopCannon();
  cannonSkip = skip;
  window.addEventListener("keydown", skip, { once: true });
  cannonTimer = window.setTimeout(stopCannon, CANNON_MS);
}

function stopCannon(): void {
  if (cannonTimer) {
    clearTimeout(cannonTimer);
    cannonTimer = null;
  }
  if (cannonSkip) {
    window.removeEventListener("keydown", cannonSkip);
    cannonSkip = null;
  }
  cannonEl?.remove();
  cannonEl = null;
}

// --- 事件摄入（S17 喂数入口） ---

function bindIntakes(): void {
  const on = (name: string, fn: (d: unknown) => void): void => {
    const h = (e: Event): void => fn((e as CustomEvent).detail);
    window.addEventListener(`nova://quality-${name}`, h);
    bag.push(() => window.removeEventListener(`nova://quality-${name}`, h));
  };

  on("energy", (d) => {
    const axes = (d as { axes?: EnergySample[] } | undefined)?.axes;
    if (Array.isArray(axes)) {
      energySamples = axes.filter((s) => s && (s.axis === "idle" || s.axis === "boot" || s.axis === "motion"));
      lsSet(`${NS}.energy.v1`, energySamples);
      renderPanel();
    }
  });
  on("nest-report", (d) => {
    const apps = (d as { apps?: NestEntry[] } | undefined)?.apps;
    if (Array.isArray(apps)) {
      nestEntries = apps.filter((a) => a && a.id);
      lsSet(`${NS}.nest.v1`, nestEntries);
      renderPanel();
    }
  });
  on("nest-command", (d) => {
    // 系统层睡眠/唤醒执行结果的回执（W-189 判定辅助；非本模块伪造）
    const cmd = d as { id?: string; action?: string; beforeMB?: number; afterMB?: number; wakeMs?: number; beforeHash?: string; afterHash?: string } | undefined;
    if (!cmd?.id) return;
    if (cmd.action === "slept" && typeof cmd.beforeMB === "number" && typeof cmd.afterMB === "number") {
      const e = nestEntries.find((x) => x.id === cmd.id);
      if (e) {
        e.asleep = true;
        e.swappedMB = cmd.afterMB;
        lsSet(`${NS}.nest.v1`, nestEntries);
        renderPanel();
      }
    } else if (cmd.action === "woke" && typeof cmd.wakeMs === "number" && cmd.beforeHash && cmd.afterHash) {
      qualityEvent("nest-verdict", {
        id: cmd.id,
        restore: restoreIntegrity(cmd.beforeHash, cmd.afterHash),
        ...nestVerdict(releaseRatio(cmd.beforeMB ?? 0, 0), cmd.wakeMs),
      });
    }
  });
  on("dump", (d) => {
    const dump = d as CrashDump | undefined;
    if (dump && dump.id && Array.isArray(dump.ops)) {
      lastReplay = { dump, verdict: replayCrash(dump) };
      qualityEvent("replay-verdict", lastReplay.verdict);
      renderPanel();
    }
  });
  on("frames", (d) => {
    const frames = (d as { frames?: FrameSample[] } | undefined)?.frames;
    if (Array.isArray(frames)) {
      const now = Date.now();
      frameSamples = evictInk([...frameSamples, ...frames.filter((f) => f && Number.isFinite(f.ms) && f.ms > 0 && Number.isFinite(f.at))], now).slice(-2000);
      lsSet(`${NS}.ink.v1`, frameSamples);
      renderPanel();
    }
  });
  on("wargame", (d) => {
    const wd = d as { a?: WargameSide; b?: WargameSide; t1?: number; t2?: number } | undefined;
    if (wd?.a && wd?.b) {
      const report = wargameReport(wd.a, wd.b);
      lastWargame = { a: wd.a, b: wd.b, report, aligned: clockAligned(wd.t1 ?? 0, wd.t2 ?? 0, 50) || wd.t1 === undefined };
      qualityEvent("wargame-report", report);
      renderPanel();
    }
  });
  on("floor", (d) => {
    const cur = d as { version?: string; idleMB?: number; at?: number } | undefined;
    if (cur?.version && typeof cur.idleMB === "number" && cur.idleMB > 0) {
      const rec: FloorRecord = { version: cur.version, idleMB: cur.idleMB, at: typeof cur.at === "number" ? cur.at : Date.now() };
      const upd = updateFloor(lsGet<FloorRecord[]>(`${NS}.floor.v1`, []), rec);
      lsSet(`${NS}.floor.v1`, upd.records);
      if (upd.verdict === "approaching") qualityEvent("floor-alarm", { floor: upd.floor, reason: upd.reason });
      renderPanel();
    }
  });
  on("build", (d) => {
    const builds = (d as { builds?: Array<{ label: string; files: BuildFile[] }> } | undefined)?.builds;
    if (Array.isArray(builds) && builds.length >= 2) {
      const fa = fingerprintTree(builds[0]!.label ?? "A", builds[0]!.files ?? []);
      const fb = fingerprintTree(builds[1]!.label ?? "B", builds[1]!.files ?? []);
      lastFingerprint = { a: fa, b: fb, compare: compareFingerprints(fa, fb) };
      qualityEvent("fingerprint", lastFingerprint.compare);
      renderPanel();
    }
  });
  on("fresh-destroy", () => {
    if (freshSession) {
      freshSession = destroyFresh(freshSession);
      qualityEvent("fresh-destroyed", { id: freshSession.id, residue: residueOf(freshSession, []) });
      renderPanel();
    }
  });
  on("weather", (d) => {
    const w = d as { cpuPct?: number; memPct?: number } | undefined;
    if (typeof w?.cpuPct === "number" && typeof w.memPct === "number") {
      const now = Date.now();
      weatherHistory = [...weatherHistory, { at: now, cpuPct: w.cpuPct, memPct: w.memPct }].slice(-500);
      lsSet(`${NS}.weather.v1`, weatherHistory);
      lastWeather = { now: weatherOf(w.cpuPct, w.memPct), forecast: forecastWeather(weatherHistory, now) };
      renderPanel();
    }
  });
  on("zodiac", (d) => {
    const metrics = (d as { metrics?: ZodiacMetric[] } | undefined)?.metrics;
    if (Array.isArray(metrics)) {
      zodiacMetrics = metrics.filter((m) => m && typeof m.key === "string" && typeof m.value === "number");
      lsSet(`${NS}.zodiac.v1`, zodiacMetrics);
      renderPanel();
    }
  });
  on("release", (d) => {
    const rel = d as { version?: string; note?: string } | undefined;
    if (!rel?.version) return;
    const milestones = addMilestone(lsGet<Milestone[]>(`${NS}.plaque.v1`, []), { version: rel.version, at: Date.now(), note: rel.note });
    lsSet(`${NS}.plaque.v1`, milestones);
    const isDev = typeof import.meta !== "undefined" && (import.meta as { env?: { DEV?: boolean } }).env?.DEV === true;
    if (cannonAllowed({ dev: isDev, manual: false })) fireCannon({ dev: isDev, manual: false });
    curTab = "cannon";
    renderPanel();
  });
  on("cannon", (d) => {
    const opts = d as { manual?: boolean } | undefined;
    if (flagOn("W-198")) fireCannon({ dev: false, manual: opts?.manual !== false });
  });
  on("archive", (d) => {
    const cand = d as ArchiveEntry | undefined;
    if (cand?.metric && typeof cand.value === "number" && cand.version && cand.machine && cand.scenario) {
      const upd = archiveUpdate(lsGet<ArchiveEntry[]>(`${NS}.archive.v1`, []), cand);
      lsSet(`${NS}.archive.v1`, upd.entries);
      if (upd.record) qualityEvent("archive-record", { salute: upd.salute, reason: upd.reason });
      renderPanel();
    }
  });
  on("tests", (d) => {
    const td = d as { total?: { passed: number; failed: number }; domains?: DomainTests[] } | undefined;
    if (td?.total && typeof td.total.passed === "number") {
      const payload = { total: td.total, domains: Array.isArray(td.domains) ? td.domains.filter((x) => x && x.domain) : [] };
      lsSet(`${NS}.tests.v1`, payload);
      renderPanel();
    }
  });
}

// --- S0 注册表联动 ---

function onRegistryChange(): void {
  if (typeof document === "undefined") return;
  const anyOn = QUALITY_NOVA_FEATURES.some((f) => flagOn(f.id));
  if (!anyOn) {
    togglePanel(false);
    stopCannon();
    if (freshSession) freshSession = destroyFresh(freshSession); // 全关即终结白纸会话
  }
}

// ---------------------------------------------------------------------------
// 激活 / 卸载（幂等）
// ---------------------------------------------------------------------------

export function activateQualityNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  loadState();
  ensureStyle();

  const onOpen = (): void => togglePanel();
  window.addEventListener("nova://quality-open", onOpen);
  bag.push(() => window.removeEventListener("nova://quality-open", onOpen));

  // Hub overlay 直达（ai04 协议；registry：W-197 nova-zodiac / W-200 nova-battlepass）
  const onOpenFeature = (ev: Event): void => {
    const f = (ev as CustomEvent).detail?.feature;
    if (f === "nova-zodiac" && flagOn("W-197")) {
      curTab = "zodiac";
      togglePanel(true);
    }
    if (f === "nova-battlepass" && flagOn("W-200")) {
      curTab = "battlepass";
      togglePanel(true);
    }
  };
  window.addEventListener("ai04:open-feature", onOpenFeature);
  bag.push(() => window.removeEventListener("ai04:open-feature", onOpenFeature));

  // W-195 开发者命令 nova fresh-eyes（S17 可映射为命令面板项）
  const onFreshCmd = (): void => {
    if (!flagOn("W-195")) return;
    freshSession = spawnFresh(Date.now());
    curTab = "fresh";
    togglePanel(true);
    qualityEvent("fresh", { id: freshSession.id });
  };
  window.addEventListener("nova://quality-fresh", onFreshCmd);
  bag.push(() => window.removeEventListener("nova://quality-fresh", onFreshCmd));

  bindIntakes();

  void Promise.resolve().then(async () => {
    try {
      const mod = await import("../registry");
      bag.push(mod.subscribeNova(onRegistryChange));
    } catch {
      /* S0 未就绪：开关即时生效退化为下次激活生效 */
    }
  });
}

export function deactivateQualityNova(): void {
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
  stopCannon();
  panel?.remove();
  panel = null;
  if (freshSession) freshSession = destroyFresh(freshSession);
  document.querySelectorAll(".nova-quality-root,.nova-quality-cannon").forEach((n) => n.remove());
}

export function isQualityNovaActive(): boolean {
  return active;
}
