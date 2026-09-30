/**
 * NOVA-200 · S8 硬件感知路（AI-08）—— 域8 系统集成与硬件（W-090…W-101）。
 *
 * 边界（全景 §8）：Q-51 GPU 画布管 GPU→极光、N-19/N-20/N-22/N-23 管聚合面板/电源管理/
 * 存储健康/设备中心、Q-52 电池呼吸管电量环境光、Q-57 风扇感知管降载、Z-43 逐应用音量
 * 管记忆、Z-48 麦克风指示灯管视觉；本模块补**CPU 潮汐、电池月度叙事、麦克风启用风铃、
 * 内存川流带、分核合唱、磁盘巡逻灯、插拔电流仪式、屏幕工时簿、风扇声音补偿、
 * USB 电流印、硬件生日书、环境分贝伴飞**。
 *
 * 纪律：
 * - 零侵入：不改写 Taskbar/托盘/壁纸内部逻辑；全部为 DOM 叠层 + CSS 变量 +
 *   类名挂载 + 事件消费（nova://hw-mic-state / nova://hw-power / nova://hw-usb /
 *   nova://hw-install-dates）+ 数据源只读（nova_pulse_ex / nova_audio_env，缺省诚实隐藏）；
 * - 前缀：类名 `nova-hw-`、事件 `nova://hw-*`、localStorage 键 `nova.hw.*`；
 * - 开关：只读消费 S0 注册表（registry.ts，novaOn/novaNum/novaStr）；
 * - 降级：reduce-motion / safeMode / static 下动效归零（潮汐/川流/落雷/合唱波动），
 *   语义与信息卡保留；无电池/无传感器/无数据源全部如实隐藏（不假装有读数）；
 * - 开销：川流带纯 CSS 合成器动画（JS 只 2s 改一次变量）；合唱卡离屏零采样；
 *   巡逻灯闪频限流 4Hz；分贝只在内存滚动 3s，零存储零网络。
 */

import { novaMotionOK, novaNum, novaOn, novaStr } from "../registry";
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

const NS = "nova.hw";

/** 功能开关：只读消费 S0 注册表（nova.registry.v1 单一事实源）。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

function num(id: string, key: string): number {
  return novaNum(id, key);
}

function str(id: string, key: string): string {
  return novaStr(id, key);
}

/** 派发 `nova://hw-*` 事件（SSR/测试环境/残缺 window 存根安全）。 */
export function novaEvent(name: string, detail?: unknown): void {
  try {
    if (typeof window === "undefined" || typeof window.dispatchEvent !== "function" || typeof CustomEvent === "undefined") return;
    window.dispatchEvent(new CustomEvent(`nova://hw-${name}`, { detail }));
  } catch {
    /* 事件总线不可用：静默跳过 */
  }
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
  params?: NovaParamDef[];
  /** 独立 overlay 工具窗（与 registry.overlay 对齐）。 */
  overlay?: string;
  /** 需要 S17/Rust 地基补接线的行为（本模块 API/事件已备好）。 */
  wiringHint?: string;
  /** reduce-motion / safeMode / static 降级说明。 */
  degrade: string;
}

export const HW_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-090",
    titleZh: "CPU 潮汐壁纸",
    titleEn: "CPU Tide",
    descZh: "CPU 负载 2s 采样映射壁纸粒子潮汐密度：轻载退潮稀疏、重载涨潮澎湃；映射单调，4 核以下设备降为三档离散潮位；仅 living 壁纸叠加，壁纸关闭即隐藏。",
    defaultOn: false,
    params: [{ key: "intensity", labelKey: "novaP_intensity", type: "slider", default: 60, min: 0, max: 100, step: 10 }] as never,
    wiringHint: "living 壁纸粒子层订阅 nova://hw-tide 消费密度（Q-51 GPU 画布管极光，两轴不重叠）",
    degrade: "reduce-motion / safeMode / static 下不派发潮汐事件（粒子保持静态密度）",
  },
  {
    id: "W-091",
    titleZh: "电池健康叙事",
    titleEn: "Battery Story",
    descZh: "电池月度故事卡：本月充放循环 / 健康度趋势线 / 一句人话结论（由真实数据模板生成，无编造）+ 三条数据驱动的习惯建议。",
    defaultOn: true,
    overlay: "nova-battery",
    wiringHint: "月度样本由 nova_pulse_ex 电池轴提供；台式机（无电池）整卡如实隐藏",
    degrade: "无动效，纯信息卡",
  },
  {
    id: "W-092",
    titleZh: "麦克风风铃",
    titleEn: "Mic Chime",
    descZh: "麦克风每次启用瞬间：一声 120ms 木质风铃（音量恒定 20%），与 Z-48 视觉指示灯构成双通道警示；勿扰下风铃静默但灯保留。",
    defaultOn: true,
    wiringHint: "启用事件经 nova://hw-mic-state 消费（Z-48 麦克风门同步派发）",
    degrade: "reduce-motion 不影响（纯声音通道，本就无动效）",
  },
  {
    id: "W-093",
    titleZh: "内存川流带",
    titleEn: "RAM Stream",
    descZh: "任务栏顶端 1px 川流光带：内存占用映射流速与亮度（高压 ≥85% 急流泛白），悬停显示精确占用；渲染纯 CSS 合成器动画，JS 每 2s 只改一次变量。",
    defaultOn: true,
    wiringHint: "内存采样待 nova_pulse_ex 接线；无数据时光带如实隐藏",
    degrade: "reduce-motion / static 下光带静止（保留亮度映射，停止流动动画）",
  },
  {
    id: "W-094",
    titleZh: "核心合唱",
    titleEn: "Core Chorus",
    descZh: "托盘 CPU 悬停浮出每核心 4px 波形柱，N 核并列如合唱谱；超 32 逻辑核自动聚合超线程对；点击展开最近 60s 完整时间线；采样 1s，离屏零开销。",
    defaultOn: true,
    wiringHint: "分核采样待 nova_pulse_ex 接线；数据源未就绪时合唱卡如实显示等待文案",
    degrade: "reduce-motion 下波形柱直接显示当前值（无波动过渡）",
  },
  {
    id: "W-095",
    titleZh: "磁盘巡逻灯",
    titleEn: "Disk LED",
    descZh: "托盘区为每块物理盘一枚巡逻灯：读写活动 120ms 微闪（现代版硬盘红灯），悬停显示盘名与瞬时速率；闪频限流 4Hz 防闪癔，超 4 盘聚合单灯+数字。",
    defaultOn: true,
    wiringHint: "磁盘活动采样待 nova_pulse_ex 接线；无数据时巡逻灯整组隐藏",
    degrade: "reduce-motion 下改为常亮指示（不闪烁），语义保留",
  },
  {
    id: "W-096",
    titleZh: "电流仪式",
    titleEn: "Bolt Ritual",
    descZh: "接电瞬间右下角 200ms 落雷微光 + 一次提示音；拔出反向细流；制式三选（雷击/瀑布/静默），插拔 2s 防抖，夜间联动 W-004 静音规则。",
    defaultOn: true,
    params: [
      {
        key: "bolt",
        labelKey: "novaP_bolt",
        type: "select",
        default: "thunder",
        options: [
          { value: "thunder", labelKey: "novaO_thunder" },
          { value: "waterfall", labelKey: "novaO_waterfall" },
          { value: "silence", labelKey: "novaO_silence" },
        ],
      },
    ] as never,
    wiringHint: "插拔事件经 nova://hw-power 消费（Q-55 电源画廊卡同源派发）",
    degrade: "reduce-motion / safeMode / static 下微光关闭，仅保留静默感知",
  },
  {
    id: "W-097",
    titleZh: "屏幕工时簿",
    titleEn: "Panel Hours",
    descZh: "每显示器独立工时簿：累计点亮小时数 + OLED 烧屏风险提示（静态内容驻留提醒与任务栏位置轮换建议）；非 OLED 跳过烧屏建议；数据可一键清零。",
    defaultOn: true,
    overlay: "nova-panelhours",
    wiringHint: "多屏逐屏点亮时间待 S17 多屏壳窗；当前单窗口壳如实只记主屏",
    degrade: "无动效，纯账本",
  },
  {
    id: "W-098",
    titleZh: "风扇声纹补偿",
    titleEn: "Fan Compensate",
    descZh: "风扇高速（≥70%，Q-57 同源判定带迟滞回线）时系统提示音自动切换穿透制式（+6dB 高频段补偿），风扇回落恢复；夜间不补偿避免扰民；补偿曲线可整体关闭。",
    defaultOn: false,
    wiringHint: "音量总线消费 nova://hw-compensate 事件应用增益（Q-57 风扇判定同源同去）",
    degrade: "无动效，纯增益事件（默认 0dB 零输出）",
  },
  {
    id: "W-099",
    titleZh: "USB 电流印",
    titleEn: "Current Print",
    descZh: "USB 设备插入卡追加电流行：协商电流与实际取用（mA），一眼识别吃电大户（≥900mA 高亮）；不可读如实 N/A；最近 5 次插入对比小图。",
    defaultOn: true,
    wiringHint: "电流读数经 nova://hw-usb 消费（Q-55 USB 画廊卡同源派发）",
    degrade: "无动效，纯信息卡",
  },
  {
    id: "W-100",
    titleZh: "硬件生日书",
    titleEn: "Hardware Birthday",
    descZh: "硬件装机纪念日：SSD/内存条周年当日托盘一枚蛋糕微标 + 该硬件健康一句话（联动 N-22）；装机日期读不到就不展示（绝不编造）。",
    defaultOn: true,
    wiringHint: "装机日期经 nova://hw-install-dates 播种（SMART/系统读取，N-22 健康行同源）",
    degrade: "无动效，纯微标",
  },
  {
    id: "W-101",
    titleZh: "环境分贝伴飞",
    titleEn: "Decibel Companion",
    descZh: "麦克风秒级采样本地推算环境声级（不存储不上传，内存只滚动 3s）：噪声 >65dB 通知音量 +20%、<35dB 媒体音量微降——音量贴环境；可一键暂停采样。",
    defaultOn: false,
    wiringHint: "声级推算待 nova_audio_env 接线；麦克风占用状态与 Z-48 指示联动如实显示",
    degrade: "无动效，纯增益事件（暂停时零采样零输出）",
  },
];

export const hwNovaDomain = {
  id: "S8",
  nameZh: "硬件感知",
  nameEn: "Hardware Sense",
  route: "AI-08",
  features: HW_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

// ---- W-090 CPU 潮汐 ----

export const TIDE_SAMPLE_MS = 2000;
export const TIDE_DISCRETE_CORES = 4;

/**
 * 潮汐密度（0..1）：轻载退潮、重载涨潮。单调映射：
 * density = 0.15 + (cpu/100) × (intensity/100) × 0.85 —— intensity=0 也有基线呼吸。
 */
export function tideDensity(cpuPct: number, intensity: number): number {
  const cpu = clamp(cpuPct, 0, 100) / 100;
  const amp = clamp(intensity, 0, 100) / 100;
  return Math.round((0.15 + cpu * amp * 0.85) * 1000) / 1000;
}

/** 三档离散潮位（4 核以下设备降档）：轻载 low / 中载 mid / 重载 high。 */
export function tideLevelDiscrete(cpuPct: number): "low" | "mid" | "high" {
  const cpu = clamp(cpuPct, 0, 100);
  if (cpu < 33) return "low";
  if (cpu < 66) return "mid";
  return "high";
}

/** 粒子数量映射：密度 0 → 30% 基线，密度 1 → 满编制（单调、不为负）。 */
export function tideParticleCount(baseCount: number, density: number): number {
  if (baseCount <= 0) return 0;
  return Math.round(baseCount * (0.3 + 0.7 * clamp(density, 0, 1)));
}

// ---- W-091 电池健康叙事 ----

export interface BatterySample {
  ts: number;
  /** 电量百分比 0..100。 */
  pct: number;
  charging: boolean;
}

/**
 * 充放循环数估算（行业标准口径）：累计充入电量每满 100% 记 1 循环
 * （跨多次浅充累加，放电段不重置）。样本按 ts 升序传入；空样本如实返回 0。
 */
export function countChargeCycles(samples: BatterySample[]): number {
  let acc = 0;
  let prev: BatterySample | null = null;
  for (const s of samples) {
    if (prev && s.charging && prev.charging && s.pct > prev.pct) acc += s.pct - prev.pct;
    prev = s;
  }
  return Math.floor(acc / 100);
}

export interface BatteryStoryInput {
  cyclesThisMonth: number;
  healthNowPct: number;
  healthPrevPct: number | null;
  /** 日均满放深度百分比（无样本为 null）。 */
  avgFullDrainPerDayPct: number | null;
}

export interface BatteryStory {
  headline: string;
  trendPct: number | null;
  trendText: string;
  tips: string[];
}

/**
 * 月度故事卡：结论全部由真实数据模板生成（无编造）。
 * 上月健康度缺失 → 趋势如实写「样本不足」。
 */
export function batteryStory(i: BatteryStoryInput): BatteryStory {
  let headline: string;
  let trendPct: number | null = null;
  let trendText: string;
  if (i.healthPrevPct == null) {
    headline = "这个月的电池故事刚开头";
    trendText = "上月健康度样本不足，暂时无法比较（不猜测）";
  } else {
    trendPct = Math.round((i.healthNowPct - i.healthPrevPct) * 10) / 10;
    if (trendPct <= -2) headline = `你的电池比你上个月更累了 ${Math.abs(trendPct)}%`;
    else if (trendPct < 0) headline = `电池健康度微降 ${Math.abs(trendPct)}%，属正常损耗`;
    else if (trendPct === 0) headline = "电池状态平稳，和你上次见面时一样";
    else headline = `校准后读数回升 ${trendPct}%，状态不错`;
    trendText = `健康度 ${i.healthNowPct}%（上月 ${i.healthPrevPct}%）`;
  }
  const tips: string[] = [];
  if (i.cyclesThisMonth > 60) tips.push("本月循环偏多，长期插电场景试试 60% 充电上限");
  if (i.avgFullDrainPerDayPct != null && i.avgFullDrainPerDayPct > 60) tips.push("日均满放偏深，浅充浅放（20–80%）更省寿命");
  if (i.healthNowPct < 80) tips.push("健康度已低于 80%，换电池前先做一次完整校准读数");
  if (tips.length === 0) tips.push("当前数据没有异常项，保持 20–80% 区间充放即可");
  return { headline, trendPct, trendText, tips: tips.slice(0, 3) };
}

// ---- W-092 麦克风风铃 ----

export const CHIME_MS = 120;
export const CHIME_VOLUME = 0.2;
export type ChimeMaterial = "wood" | "bamboo" | "metal";

/** 风铃基频对（木质双分音）。 */
export function chimeFreqs(material: ChimeMaterial): [number, number] {
  switch (material) {
    case "bamboo":
      return [740, 1110];
    case "metal":
      return [1046, 1568];
    default:
      return [880, 1320];
  }
}

/** 启用瞬间风铃计划：勿扰静默（灯保留）、未启用不响。 */
export function micChimePlan(opts: { enabled: boolean; dnd: boolean }): {
  play: boolean;
  ms: number;
  volume: number;
  reason?: string;
} {
  if (!opts.enabled) return { play: false, ms: 0, volume: 0, reason: "not-enabled" };
  if (opts.dnd) return { play: false, ms: 0, volume: 0, reason: "dnd" };
  return { play: true, ms: CHIME_MS, volume: CHIME_VOLUME };
}

// ---- W-093 内存川流带 ----

export const STREAM_SAMPLE_MS = 2000;
export const STREAM_WASH_PCT = 85;

/**
 * 川流带映射：占用升高 → 流速加快（12s→3s/循环）与亮度增强（0.35→1）。
 * ≥85% 急流泛白（wash）。全部 CSS 变量直改，无逐帧 JS。
 */
export function streamBand(memPct: number): { speedSec: number; brightness: number; wash: boolean } {
  const pct = clamp(memPct, 0, 100);
  const speedSec = Math.round((12 - 9 * (pct / 100)) * 10) / 10;
  const brightness = Math.round((0.35 + 0.65 * (pct / 100)) * 100) / 100;
  return { speedSec, brightness, wash: pct >= STREAM_WASH_PCT };
}

// ---- W-094 核心合唱 ----

export const CHORUS_SAMPLE_MS = 1000;
export const CHORUS_MAX_COLUMNS = 32;
export const CHORUS_BAR_MIN_PX = 2;
export const CHORUS_BAR_MAX_PX = 14;
export const CHORUS_TIMELINE_S = 60;

export interface ChorusLayout {
  /** 波形柱列数（≤32）。 */
  columns: number;
  /** 是否发生了超线程对聚合。 */
  paired: boolean;
  /** 列 → 逻辑核下标组（取均值画柱）。 */
  groups: number[][];
}

/**
 * 分核合唱谱布局：≤32 逻辑核一核一柱；超 32 自动聚合（相邻对取均值，
 * 逻辑核为偶数时按超线程对称 pairing：i 与 i+N/2 同物理核）。
 */
export function chorusColumns(logicalCores: number): ChorusLayout {
  const n = Math.max(0, Math.floor(logicalCores));
  if (n <= CHORUS_MAX_COLUMNS) {
    return { columns: n, paired: false, groups: Array.from({ length: n }, (_, i) => [i]) };
  }
  const paired = n % 2 === 0;
  const groups: number[][] = [];
  if (paired) {
    const half = n / 2;
    for (let i = 0; i < half; i++) groups.push([i, i + half]);
    if (groups.length > CHORUS_MAX_COLUMNS) {
      // 超过 32 物理核：相邻再聚合（诚实极限，柱均分）
      return { columns: CHORUS_MAX_COLUMNS, paired: true, groups: aggregateGroups(n, CHORUS_MAX_COLUMNS) };
    }
    return { columns: groups.length, paired: true, groups };
  }
  return { columns: CHORUS_MAX_COLUMNS, paired: false, groups: aggregateGroups(n, CHORUS_MAX_COLUMNS) };
}

/** 超上限聚合：n 核均匀铺进 columns 列（列内取均值画柱）。 */
export function aggregateGroups(n: number, columns: number): number[][] {
  const groups: number[][] = Array.from({ length: columns }, () => []);
  for (let i = 0; i < n; i++) groups[i % columns]!.push(i);
  return groups;
}

/** 波形柱高（px）：2–14px 线性映射。 */
export function coreBarHeight(pct: number): number {
  const p = clamp(pct, 0, 100);
  return Math.round((CHORUS_BAR_MIN_PX + (p / 100) * (CHORUS_BAR_MAX_PX - CHORUS_BAR_MIN_PX)) * 10) / 10;
}

/** 列均值（组内逻辑核占用均值；空组为 0）。 */
export function chorusColumnPct(corePcts: number[], group: number[]): number {
  if (group.length === 0) return 0;
  let sum = 0;
  for (const i of group) sum += clamp(corePcts[i] ?? 0, 0, 100);
  return Math.round((sum / group.length) * 10) / 10;
}

// ---- W-095 磁盘巡逻灯 ----

export const LED_FLASH_MS = 120;
/** 闪频限流：最密 4Hz（250ms 一闪）防闪癔。 */
export const LED_MIN_INTERVAL_MS = 250;
export const LED_AGGREGATE_MAX = 4;

export interface DiskActivity {
  name: string;
  /** 本采样周期内有读写活动。 */
  busy: boolean;
  /** 瞬时速率（B/s，可空）。 */
  rateBps?: number | null;
}

/** 巡逻灯闪烁判定：活动 且 距上次点亮 ≥250ms（限流 4Hz）。 */
export function diskLedFlash(busy: boolean, lastFlashAt: number, now: number): boolean {
  if (!busy) return false;
  return now - lastFlashAt >= LED_MIN_INTERVAL_MS;
}

/** 灯位计划：≤4 盘逐盘点亮；>4 盘聚合单灯 + 数字（诚实容量）。 */
export function diskLampPlan(
  disks: DiskActivity[],
  max = LED_AGGREGATE_MAX,
): { aggregate: boolean; shown: DiskActivity[]; extra: number } {
  if (disks.length <= max) return { aggregate: false, shown: disks, extra: 0 };
  return { aggregate: true, shown: disks.slice(0, 1), extra: disks.length - 1 };
}

/** 悬停速率文案（不可读如实 N/A）。 */
export function diskRateText(rateBps?: number | null): string {
  if (rateBps == null) return "N/A";
  if (rateBps < 1024) return `${Math.round(rateBps)} B/s`;
  if (rateBps < 1024 * 1024) return `${(rateBps / 1024).toFixed(1)} KB/s`;
  return `${(rateBps / (1024 * 1024)).toFixed(1)} MB/s`;
}

// ---- W-096 电流仪式 ----

export const BOLT_FLASH_MS = 200;
export const BOLT_DEBOUNCE_MS = 2000;
export type BoltStyle = "thunder" | "waterfall" | "silence";

/** 夜间时段（22:00–07:00，本地时区）——W-004 静音规则联动。 */
export function isNightHour(hour: number): boolean {
  return hour >= 22 || hour < 7;
}

export interface BoltPlan {
  /** 落雷（插入）/细流（拔出）；silence 制式无光。 */
  flash: "thunder" | "trickle" | null;
  ms: number;
  sound: boolean;
}

/**
 * 仪式计划：插入→落雷+提示音；拔出→反向细流（无声）。
 * 提示音制式三选：thunder/waterfall 有声、silence 静默；夜间一律不响（W-004 联动）。
 */
export function boltPlan(kind: "plug" | "unplug", style: BoltStyle, night: boolean): BoltPlan {
  const flash = style === "silence" ? null : kind === "plug" ? "thunder" : "trickle";
  const sound = !night && style !== "silence" && kind === "plug";
  return { flash, ms: flash ? BOLT_FLASH_MS : 0, sound };
}

/** 插拔 2s 防抖（重复插拔不连放仪式）。 */
export function boltDebounced(now: number, lastAt: number | null, debounceMs = BOLT_DEBOUNCE_MS): boolean {
  if (lastAt == null) return true;
  return now - lastAt >= debounceMs;
}

// ---- W-097 屏幕工时簿 ----

export const PANEL_TICK_MS = 60_000;
export const OLED_DWELL_HINT_MS = 4 * 3_600_000;

export interface PanelRecord {
  /** 累计点亮小时（两位小数）。 */
  hours: number;
  oled: boolean;
  /** 当前静态内容起始时刻（null=内容在动）。 */
  staticSinceMs: number | null;
  updated: number;
}

/** 工时累计（每 60s 一格；幂等加法）。 */
export function panelAccumulate(rec: PanelRecord, seconds: number, now: number): PanelRecord {
  return { ...rec, hours: Math.round((rec.hours + seconds / 3600) * 100) / 100, updated: now };
}

/** 工时簿清零（验收要求可清零）。 */
export function panelReset(rec: PanelRecord, now: number): PanelRecord {
  return { ...rec, hours: 0, staticSinceMs: null, updated: now };
}

/**
 * OLED 烧屏提示：非 OLED 如实返回空；驻留超 4h 建议轮换静态任务栏位置/壁纸。
 */
export function oledHints(rec: PanelRecord, now: number): string[] {
  if (!rec.oled) return [];
  const out: string[] = [];
  if (rec.staticSinceMs != null && now - rec.staticSinceMs >= OLED_DWELL_HINT_MS) {
    out.push("静态内容已驻留超 4 小时，建议轮换壁纸或移动任务栏位置");
  }
  if (rec.hours >= 8) out.push("今日点亮已超 8 小时，OLED 长亮时可留意高对比静态元素");
  return out;
}

// ---- W-098 风扇声纹补偿 ----

export const FAN_HIGH_PCT = 70;
export const FAN_LOW_PCT = 55;
export const FAN_GAIN_DB = 6;

/** 补偿曲线：高速 ≥70% → +6dB；夜间（22–07）不补偿；总开关关闭恒 0。 */
export function fanCompensateDb(fanPct: number, night: boolean, enabled: boolean): number {
  if (!enabled || night) return 0;
  return clamp(fanPct, 0, 100) >= FAN_HIGH_PCT ? FAN_GAIN_DB : 0;
}

/** 迟滞回线（Q-57 同源判定）：70% 触发、回落 55% 才解除，防临界抖动。 */
export function fanHysteresis(on: boolean, fanPct: number): boolean {
  return on ? fanPct > FAN_LOW_PCT : fanPct >= FAN_HIGH_PCT;
}

// ---- W-099 USB 电流印 ----

export const USB_HISTORY_MAX = 5;
export const USB_HUNGRY_MA = 900;

export interface UsbCurrentRow {
  name: string;
  negotiatedMa: number | null;
  actualMa: number | null;
  /** 一行文案：「协商 900mA · 实际 480mA」；不可读一侧如实 N/A。 */
  row: string;
  /** 吃电大户（实际取用 ≥900mA）。 */
  hungry: boolean;
}

function maText(v: number | null): string {
  return v == null ? "N/A" : `${Math.round(v)}mA`;
}

/** 电流行：读数来自 USB API；不可读一侧如实 N/A（不编造）。 */
export function currentPrint(name: string, negotiatedMa: number | null, actualMa: number | null): UsbCurrentRow {
  return {
    name,
    negotiatedMa,
    actualMa,
    row: `协商 ${maText(negotiatedMa)} · 实际 ${maText(actualMa)}`,
    hungry: actualMa != null && actualMa >= USB_HUNGRY_MA,
  };
}

/** 历史 5 次对比（FIFO，超出挤掉最旧）。 */
export function usbHistoryPush(list: UsbCurrentRow[], entry: UsbCurrentRow, max = USB_HISTORY_MAX): UsbCurrentRow[] {
  const next = [...list, entry];
  return next.slice(Math.max(0, next.length - max));
}

// ---- W-100 硬件生日书 ----

export interface HwBirthdayEntry {
  id: string;
  name: string;
  /** 装机时刻（系统/SMART 真实读取；读不到不入场）。 */
  installMs: number;
  /** 健康一句话（N-22 同源；可缺省）。 */
  healthLine?: string;
}

export interface HwAnniversary {
  id: string;
  name: string;
  years: number;
  /** 蛋糕微标文案。 */
  line: string;
  healthLine?: string;
}

/** 周年判定：今日为装机月/日 且 年数 ≥1。读不到装机日期的条目永不入册。 */
export function birthdayCheck(list: HwBirthdayEntry[], now: number): HwAnniversary[] {
  const d = new Date(now);
  const out: HwAnniversary[] = [];
  for (const e of list) {
    if (!Number.isFinite(e.installMs)) continue;
    const b = new Date(e.installMs);
    if (b.getMonth() !== d.getMonth() || b.getDate() !== d.getDate()) continue;
    const years = Math.floor((now - e.installMs) / (365.25 * 86_400_000));
    if (years < 1) continue;
    out.push({ id: e.id, name: e.name, years, line: `${e.name} 装机 ${years} 周年`, healthLine: e.healthLine });
  }
  return out;
}

// ---- W-101 环境分贝伴飞 ----

export const DB_SAMPLE_MS = 1000;
export const DB_RING_MS = 3000;
export const DB_HIGH = 65;
export const DB_LOW = 35;

export interface DbSample {
  db: number;
  ts: number;
}

/** 3s 滚动环形缓冲（纯内存，零存储零网络；过期样本丢弃）。 */
export function dbRingPush(ring: DbSample[], db: number, now: number, windowMs = DB_RING_MS): DbSample[] {
  const next = [...ring, { db, ts: now }];
  const floor = now - windowMs;
  let i = 0;
  while (i < next.length && next[i]!.ts < floor) i++;
  return next.slice(i);
}

/** 3s 均值声级（样本不足时如实返回 null）。 */
export function dbRingAvg(ring: DbSample[]): number | null {
  if (ring.length === 0) return null;
  const sum = ring.reduce((a, s) => a + s.db, 0);
  return Math.round((sum / ring.length) * 10) / 10;
}

export interface DbAdjust {
  /** 噪声 >65dB：通知音量 +20%。 */
  notifyBoost: boolean;
  /** 安静 <35dB：媒体音量微降。 */
  mediaDuck: boolean;
}

/** 音量自适应判定（3s 均值入判，秒级采样防瞬时噪声误触发）。 */
export function dbAdjust(avgDb: number | null): DbAdjust {
  if (avgDb == null) return { notifyBoost: false, mediaDuck: false };
  return { notifyBoost: avgDb > DB_HIGH, mediaDuck: avgDb < DB_LOW };
}

// ---------------------------------------------------------------------------
// 行为层（零侵入 DOM 叠层 + 事件 + 诚实数据源）
// ---------------------------------------------------------------------------

let active = false;
type Unsub = () => void;
let bag: Unsub[] = [];

// ---- CSS（唯一注入点，nova-hw- 前缀） ----

const STYLE_ID = "nova-hw-style";
const STYLE_TEXT = `
/* W-093 内存川流带（1px，纯 CSS 合成器动画；无数据时 display:none） */
.nova-hw-band{position:absolute;left:0;right:0;top:0;height:1px;pointer-events:none;overflow:hidden;opacity:var(--nova-hw-band-bright,.35)}
.nova-hw-band::before{content:"";position:absolute;inset:0;background:linear-gradient(90deg,transparent 0%,var(--accent,#5b8cff) 50%,transparent 100%);background-size:200% 100%;animation:nova-hw-stream var(--nova-hw-band-speed,12s) linear infinite}
.nova-hw-band.nova-wash::before{background:linear-gradient(90deg,transparent 0%,rgb(255 255 255 / .9) 50%,transparent 100%);background-size:200% 100%}
.nova-hw-band.nova-still::before{animation:none}
[data-reduce-motion="true"] .nova-hw-band::before,[data-safe-mode="true"] .nova-hw-band::before,[data-static-mode="true"] .nova-hw-band::before{animation:none}
@keyframes nova-hw-stream{from{background-position:0% 0}to{background-position:-200% 0}}
.nova-hw-band-wrap{position:absolute;inset:0;pointer-events:none}
.nova-hw-band-hit{position:absolute;left:0;right:0;top:0;height:8px;pointer-events:auto;cursor:default}
/* W-095 磁盘巡逻灯 */
.nova-hw-leds{display:flex;gap:3px;align-items:center;margin:0 4px}
.nova-hw-leds:empty{display:none}
.nova-hw-led{width:6px;height:6px;border-radius:50%;background:var(--accent-soft,rgb(255 255 255 / .16));position:relative;cursor:default}
.nova-hw-led.nova-on{background:var(--accent,#5b8cff);box-shadow:0 0 6px var(--accent,#5b8cff)}
.nova-hw-led.nova-solid{background:var(--accent,#5b8cff);box-shadow:none}
.nova-hw-led .nova-hw-led-extra{position:absolute;right:-2px;top:-8px;font-size:9px;opacity:.8;font-family:Consolas,monospace}
/* W-096 电流仪式微光 */
.nova-hw-bolt{position:fixed;right:16px;bottom:64px;width:140px;height:2px;z-index:950;pointer-events:none;opacity:0}
.nova-hw-bolt.nova-thunder{background:linear-gradient(270deg,transparent,var(--accent,#8ab4ff),rgb(255 255 255 / .9),var(--accent,#8ab4ff),transparent);animation:nova-hw-thunder 200ms var(--ease-standard)}
.nova-hw-bolt.nova-trickle{background:linear-gradient(90deg,transparent,rgb(255 255 255 / .5),transparent);animation:nova-hw-trickle 200ms var(--ease-standard)}
@keyframes nova-hw-thunder{0%{opacity:0;transform:scaleX(.2)}30%{opacity:1;transform:scaleX(1)}100%{opacity:0;transform:scaleX(1)}}
@keyframes nova-hw-trickle{0%{opacity:0;transform:scaleX(1)}60%{opacity:.7;transform:scaleX(.5)}100%{opacity:0;transform:scaleX(.1)}}
[data-reduce-motion="true"] .nova-hw-bolt,[data-safe-mode="true"] .nova-hw-bolt,[data-static-mode="true"] .nova-hw-bolt{display:none}
/* W-094 合唱 / W-091 故事卡 / W-097 工时簿 / W-099 电流印（共用卡片） */
.nova-hw-card{position:fixed;z-index:950;max-width:320px;padding:10px 12px;border-radius:12px;background:var(--panel,var(--bg-raised,#141a26));border:1px solid var(--accent-soft,rgb(255 255 255 / .1));box-shadow:0 12px 36px rgb(0 0 0 / .3);font-size:12px;color:var(--ink,var(--fg,#d9e4f5))}
.nova-hw-card h4{margin:0 0 4px;font-size:12px;font-weight:600;line-height:1.35}
.nova-hw-card .nova-hw-title{display:flex;justify-content:space-between;align-items:baseline;margin-bottom:6px;opacity:.85;letter-spacing:.04em;font-weight:600}
.nova-hw-card .nova-hw-meta{margin-top:6px;opacity:.62;font-size:11px;line-height:1.5}
.nova-hw-card .nova-hw-dim{opacity:.55;font-size:11px;margin-top:6px}
.nova-hw-card .nova-hw-row{display:flex;justify-content:space-between;gap:12px;margin-top:4px}
.nova-hw-card .nova-hw-row b{font-weight:600}
.nova-hw-card .nova-hw-row.nova-hungry{color:rgb(255 176 32)}
.nova-hw-chorus{display:flex;align-items:flex-end;gap:2px;height:16px;margin-top:6px}
.nova-hw-chorus i{flex:1 1 0;min-width:2px;max-width:6px;background:var(--accent,#5b8cff);border-radius:1px 1px 0 0;opacity:.85;transition:height 160ms var(--ease-standard)}
[data-reduce-motion="true"] .nova-hw-chorus i,[data-safe-mode="true"] .nova-hw-chorus i,[data-static-mode="true"] .nova-hw-chorus i{transition:none}
.nova-hw-tips{margin:4px 0 0;padding-left:16px}
.nova-hw-tips li{margin:2px 0;opacity:.82;line-height:1.45}
.nova-hw-actions{display:flex;gap:6px;margin-top:8px}
.nova-hw-actions button{font:inherit;font-size:11px;padding:3px 8px;border-radius:6px;border:1px solid var(--accent-soft);background:transparent;color:inherit;cursor:pointer}
.nova-hw-actions button:hover{background:var(--accent-soft)}
/* W-097 工时簿迷你条 */
.nova-hw-hours{display:flex;align-items:center;gap:8px}
.nova-hw-hours .nova-hw-hourbar{flex:1;height:4px;border-radius:2px;background:var(--accent-soft,rgb(255 255 255 / .12));overflow:hidden}
.nova-hw-hours .nova-hw-hourbar i{display:block;height:100%;background:var(--accent,#5b8cff);border-radius:2px}
/* W-100 蛋糕微标 */
.nova-hw-cake{display:inline-flex;align-items:center;gap:3px;margin:0 4px;font-size:11px;opacity:.9;cursor:default}
/* W-093 悬停精确占用 */
.nova-hw-band-card{position:fixed;z-index:950;padding:6px 10px;border-radius:8px;background:var(--panel,var(--bg-raised,#141a26));border:1px solid var(--accent-soft,rgb(255 255 255 / .1));font-size:11px;color:var(--ink,var(--fg,#d9e4f5));box-shadow:0 8px 24px rgb(0 0 0 / .3)}
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

function trayEl(): HTMLElement | null {
  if (typeof document === "undefined") return null;
  return document.querySelector<HTMLElement>(".taskbar .tray-area");
}

// ---- 诚实数据源：nova_pulse_ex / nova_audio_env（缺省诚实隐藏，不假装有读数） ----

export interface HwPulseSample {
  cpuPct: number;
  corePcts: number[];
  memPct: number | null;
  disks: DiskActivity[];
  fanPct: number | null;
  battery: { present: boolean; pct: number | null; healthPct: number | null; cycleCount: number | null };
}

type PulseInvoke = (cmd: string, args?: unknown) => Promise<unknown>;

let pulseInvoke: PulseInvoke | null = null;
let pulseBroken = false;

/** 动态接 Tauri invoke（浏览器/测试环境无 Tauri → null，不抛错）。 */
async function bindPulse(): Promise<PulseInvoke | null> {
  if (pulseInvoke) return pulseInvoke;
  if (pulseBroken) return null;
  try {
    const mod = (await import("@tauri-apps/api/core")) as { invoke: PulseInvoke };
    pulseInvoke = mod.invoke;
    return pulseInvoke;
  } catch {
    pulseBroken = true;
    return null;
  }
}

export function __bindPulseForTests(fn: PulseInvoke | null): void {
  pulseInvoke = fn;
  pulseBroken = false;
}

/** 一次脉搏采样：命令不存在/失败 → null（调用方如实隐藏）。 */
export async function samplePulse(): Promise<HwPulseSample | null> {
  const invoke = await bindPulse();
  if (!invoke) return null;
  try {
    const raw = (await invoke("nova_pulse_ex")) as Partial<HwPulseSample> | null;
    if (!raw || typeof raw.cpuPct !== "number") return null;
    return {
      cpuPct: raw.cpuPct,
      corePcts: Array.isArray(raw.corePcts) ? raw.corePcts : [],
      memPct: typeof raw.memPct === "number" ? raw.memPct : null,
      disks: Array.isArray(raw.disks) ? raw.disks : [],
      fanPct: typeof raw.fanPct === "number" ? raw.fanPct : null,
      battery: {
        present: raw.battery?.present === true,
        pct: typeof raw.battery?.pct === "number" ? raw.battery.pct : null,
        healthPct: typeof raw.battery?.healthPct === "number" ? raw.battery.healthPct : null,
        cycleCount: typeof raw.battery?.cycleCount === "number" ? raw.battery.cycleCount : null,
      },
    };
  } catch {
    return null;
  }
}

/** 环境声级采样（nova_audio_env；内存滚动 3s，零存储）。 */
export async function sampleAudioEnv(): Promise<number | null> {
  const invoke = await bindPulse();
  if (!invoke) return null;
  try {
    const raw = (await invoke("nova_audio_env")) as { db?: unknown } | null;
    return typeof raw?.db === "number" ? raw.db : null;
  } catch {
    return null;
  }
}

// ---- W-090 CPU 潮汐（事件出口；粒子渲染归 living 壁纸层） ----

function tideEmit(sample: HwPulseSample): void {
  if (!flagOn("W-090")) return;
  if (!motionOK()) return; // static / reduce-motion：粒子保持静态密度，不派发
  const intensity = num("W-090", "intensity");
  const discrete = sample.corePcts.length > 0 && sample.corePcts.length <= TIDE_DISCRETE_CORES;
  novaEvent("tide", {
    density: tideDensity(sample.cpuPct, intensity),
    level: discrete ? tideLevelDiscrete(sample.cpuPct) : null,
    discrete,
  });
}

// ---- W-093 内存川流带 ----

let bandHost: HTMLElement | null = null;
let bandCard: HTMLElement | null = null;

function bandMount(): void {
  const bar = taskbarEl();
  if (!bar || bandHost?.isConnected) return;
  const wrap = document.createElement("div");
  wrap.className = "nova-hw-band-wrap";
  const band = document.createElement("div");
  band.className = "nova-hw-band";
  band.setAttribute("aria-hidden", "true");
  const hit = document.createElement("div");
  hit.className = "nova-hw-band-hit";
  hit.setAttribute("role", "status");
  hit.setAttribute("aria-label", "内存川流带 RAM stream");
  hit.addEventListener("pointerenter", () => bandShowCard());
  hit.addEventListener("pointerleave", () => bandHideCard());
  wrap.append(band, hit);
  bar.appendChild(wrap);
  bandHost = wrap;
  bag.push(() => {
    wrap.remove();
    if (bandHost === wrap) bandHost = null;
    bandHideCard();
  });
}

function bandApply(sample: HwPulseSample): void {
  const bar = taskbarEl();
  if (!bar || !bandHost?.isConnected) return;
  if (!flagOn("W-093") || sample.memPct == null) {
    bandHost.style.display = "none"; // 无数据如实隐藏
    return;
  }
  bandHost.style.display = "";
  const band = bandHost.querySelector<HTMLElement>(".nova-hw-band");
  if (!band) return;
  const s = streamBand(sample.memPct);
  band.style.setProperty("--nova-hw-band-speed", `${s.speedSec}s`);
  band.style.setProperty("--nova-hw-band-bright", String(s.brightness));
  band.classList.toggle("nova-wash", s.wash);
  band.classList.toggle("nova-still", !motionOK());
  bandHost.dataset.mem = String(Math.round(sample.memPct));
}

function bandShowCard(): void {
  const host = bandHost;
  if (!host || !flagOn("W-093") || host.style.display === "none") return;
  const mem = host.dataset.mem;
  bandHideCard();
  const card = document.createElement("div");
  card.className = "nova-hw-band-card";
  card.setAttribute("role", "status");
  card.textContent = mem != null ? `内存占用 ${mem}% · ${streamBand(Number(mem)).speedSec}s/循环` : "内存川流（等待采样）";
  const bar = taskbarEl();
  card.style.left = "12px";
  card.style.top = `${Math.max(8, (bar?.getBoundingClientRect().top ?? window.innerHeight - 54) - 32)}px`;
  document.body.appendChild(card);
  bandCard = card;
  bag.push(() => {
    card.remove();
    if (bandCard === card) bandCard = null;
  });
}

function bandHideCard(): void {
  bandCard?.remove();
  bandCard = null;
}

// ---- W-094 核心合唱 ----

let chorusCard: HTMLElement | null = null;
let chorusBars: HTMLElement[] = [];
let chorusTimeline: Array<{ ts: number; pcts: number[] }> = [];
let chorusExpanded = false;

function chorusOpen(): void {
  if (!flagOn("W-094")) return;
  chorusClose();
  const card = document.createElement("div");
  card.className = "nova-hw-card nova-chorus";
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-label", "核心合唱 Core chorus");
  const title = document.createElement("div");
  title.className = "nova-hw-title";
  title.textContent = "核心合唱 · CORE CHORUS";
  card.appendChild(title);
  const row = document.createElement("div");
  row.className = "nova-hw-chorus";
  row.setAttribute("aria-hidden", "true");
  card.appendChild(row);
  const meta = document.createElement("div");
  meta.className = "nova-hw-meta";
  card.appendChild(meta);
  card.addEventListener("click", () => chorusExpanded = !chorusExpanded);
  const bar = taskbarEl();
  const tray = trayEl();
  const anchor = tray?.getBoundingClientRect();
  card.style.right = anchor ? `${Math.max(8, window.innerWidth - anchor.right)}px` : "12px";
  card.style.top = `${Math.max(8, (bar?.getBoundingClientRect().top ?? window.innerHeight - 54) - 96)}px`;
  document.body.appendChild(card);
  chorusCard = card;
  bag.push(() => {
    card.remove();
    if (chorusCard === card) {
      chorusCard = null;
      chorusBars = [];
      chorusTimeline = [];
    }
  });
}

function chorusClose(): void {
  chorusCard?.remove();
  chorusCard = null;
  chorusBars = [];
  chorusTimeline = [];
}

function chorusApply(sample: HwPulseSample, now: number): void {
  if (!chorusCard?.isConnected) return; // 离屏零采样开销（数据仍来，画只在开时）
  const layout = chorusColumns(Math.max(1, sample.corePcts.length));
  const row = chorusCard.querySelector<HTMLElement>(".nova-hw-chorus");
  const meta = chorusCard.querySelector<HTMLElement>(".nova-hw-meta");
  if (!row || !meta) return;
  if (sample.corePcts.length === 0) {
    meta.textContent = "等待 nova_pulse_ex 分核采样（数据源未就绪，不假装有读数）";
    row.textContent = "";
    chorusBars = [];
    return;
  }
  if (chorusBars.length !== layout.columns) {
    row.textContent = "";
    chorusBars = Array.from({ length: layout.columns }, () => {
      const i = document.createElement("i");
      i.style.height = `${CHORUS_BAR_MIN_PX}px`;
      row.appendChild(i);
      return i;
    });
  }
  chorusTimeline.push({ ts: now, pcts: sample.corePcts });
  const floor = now - CHORUS_TIMELINE_S * 1000;
  chorusTimeline = chorusTimeline.filter((t) => t.ts >= floor);
  if (chorusExpanded) {
    // 展开态：最近 60s 均值按列横向重排（诚实聚合视图）
    meta.textContent = `最近 ${CHORUS_TIMELINE_S}s 时间线 · ${layout.columns} 列${layout.paired ? "（超线程对聚合）" : ""} · 点击收起`;
    const n = Math.min(layout.columns, 16);
    row.textContent = "";
    chorusBars = Array.from({ length: n }, (_, col) => {
      const group = layout.groups[Math.floor((col * layout.groups.length) / n)] ?? [];
      const avg = chorusTimeline.reduce((a, t) => a + chorusColumnPct(t.pcts, group), 0) / Math.max(1, chorusTimeline.length);
      const i = document.createElement("i");
      i.style.height = `${coreBarHeight(avg)}px`;
      row.appendChild(i);
      return i;
    });
    return;
  }
  meta.textContent = `${sample.corePcts.length} 逻辑核${layout.paired ? ` · 聚合为 ${layout.columns} 列（超线程对）` : ""} · 点击展开 60s 时间线`;
  layout.groups.forEach((group, col) => {
    const bar = chorusBars[col];
    if (!bar) return;
    const pct = chorusColumnPct(sample.corePcts, group);
    bar.style.height = `${coreBarHeight(pct)}px`;
  });
}

// ---- W-095 磁盘巡逻灯 ----

let ledsHost: HTMLElement | null = null;
let ledLastFlash: number[] = [];

function ledsMount(): void {
  const tray = trayEl();
  if (!tray || ledsHost?.isConnected) return;
  const host = document.createElement("div");
  host.className = "nova-hw-leds";
  host.setAttribute("role", "status");
  host.setAttribute("aria-label", "磁盘巡逻灯 Disk LEDs");
  tray.insertBefore(host, tray.firstChild);
  ledsHost = host;
  bag.push(() => {
    host.remove();
    if (ledsHost === host) {
      ledsHost = null;
      ledLastFlash = [];
    }
  });
}

function ledsApply(sample: HwPulseSample, now: number): void {
  const tray = trayEl();
  if (!tray || !ledsHost?.isConnected) return;
  const disks = sample.disks ?? [];
  if (!flagOn("W-095") || disks.length === 0) {
    ledsHost.textContent = "";
    ledsHost.style.display = "none"; // 无数据如实隐藏
    return;
  }
  ledsHost.style.display = "";
  const plan = diskLampPlan(disks);
  const solid = !motionOK(); // reduce-motion：常亮指示不闪烁
  const lamps: DiskActivity[] = plan.aggregate
    ? [{ name: `${plan.shown[0]?.name ?? "DISK"} +${plan.extra}`, busy: disks.some((d) => d.busy), rateBps: null }]
    : plan.shown;
  if (ledsHost.childElementCount !== lamps.length) {
    ledsHost.textContent = "";
    ledLastFlash = lamps.map(() => 0);
    lamps.forEach((l) => {
      const led = document.createElement("span");
      led.className = "nova-hw-led";
      led.title = `${l.name} · ${diskRateText(l.rateBps)}`;
      if (plan.aggregate) {
        const extra = document.createElement("span");
        extra.className = "nova-hw-led-extra";
        extra.textContent = `+${plan.extra}`;
        led.appendChild(extra);
      }
      led.addEventListener("pointerenter", () => ledTooltip(l));
      ledsHost?.appendChild(led);
    });
  }
  lamps.forEach((l, idx) => {
    const led = ledsHost?.children[idx] as HTMLElement | undefined;
    if (!led) return;
    const flash = diskLedFlash(l.busy, ledLastFlash[idx] ?? 0, now);
    if (flash) ledLastFlash[idx] = now;
    led.classList.toggle("nova-on", solid ? l.busy : flash);
    led.classList.toggle("nova-solid", solid && l.busy);
    led.title = `${l.name} · ${diskRateText(l.rateBps)}`;
  });
}

function ledTooltip(l: DiskActivity): void {
  novaEvent("led-hover", { name: l.name, rate: diskRateText(l.rateBps) });
}

// ---- W-096 电流仪式 ----

let boltEl: HTMLElement | null = null;
let boltLastAt: number | null = null;

function boltPlay(kind: "plug" | "unplug"): void {
  if (!flagOn("W-096")) return;
  const now = Date.now();
  if (!boltDebounced(now, boltLastAt)) return; // 2s 防抖
  boltLastAt = now;
  const style = (str("W-096", "bolt") || "thunder") as BoltStyle;
  const night = isNightHour(new Date(now).getHours());
  const plan = boltPlan(kind, style, night);
  novaEvent("bolt", { kind, style, ...plan });
  if (plan.flash && motionOK()) {
    boltEl?.remove();
    const el = document.createElement("div");
    el.className = `nova-hw-bolt nova-${plan.flash}`;
    el.setAttribute("aria-hidden", "true");
    document.body.appendChild(el);
    boltEl = el;
    bag.push(() => {
      el.remove();
      if (boltEl === el) boltEl = null;
    });
    setTimeout(() => {
      el.remove();
      if (boltEl === el) boltEl = null;
    }, plan.ms + 60);
  }
  if (plan.sound) chimePlay([660, 990] as [number, number], 180, 0.24);
}

// ---- W-092 麦克风风铃 + WebAudio ----

let audioCtx: AudioContext | null = null;

/** 木质风铃：双分音正弦 + 指数衰减（120ms，音量恒定 20%）。 */
export function chimePlay(freqs: [number, number], ms: number, volume: number): void {
  if (typeof window === "undefined") return;
  try {
    type Ctor = { new (): AudioContext };
    const Ctx = window.AudioContext ?? (window as unknown as { webkitAudioContext?: Ctor }).webkitAudioContext;
    if (!Ctx) return;
    audioCtx = audioCtx ?? new Ctx();
    const ctx = audioCtx;
    if (ctx.state === "suspended") void ctx.resume();
    const t0 = ctx.currentTime;
    const dur = Math.max(0.05, ms / 1000);
    freqs.forEach((f, i) => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = f;
      const amp = volume * (i === 0 ? 1 : 0.5);
      gain.gain.setValueAtTime(amp, t0);
      gain.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
      osc.connect(gain).connect(ctx.destination);
      osc.start(t0);
      osc.stop(t0 + dur + 0.02);
    });
  } catch {
    /* 音频不可用：静默跳过（灯通道仍在） */
  }
}

function onMicState(e: Event): void {
  const d = (e as CustomEvent).detail as { on?: boolean; dnd?: boolean } | undefined;
  if (!flagOn("W-092")) return;
  const plan = micChimePlan({ enabled: d?.on === true, dnd: d?.dnd === true });
  if (plan.play) {
    const [f1, f2] = chimeFreqs("wood");
    chimePlay([f1, f2] as [number, number], plan.ms, plan.volume);
  }
  novaEvent("mic-chime", plan);
}

function onPower(e: Event): void {
  const d = (e as CustomEvent).detail as { kind?: string } | undefined;
  if (d?.kind === "plug") boltPlay("plug");
  else if (d?.kind === "unplug") boltPlay("unplug");
}

// ---- W-091 电池故事卡 ----

let batteryCard: HTMLElement | null = null;
let batteryStoryCache: BatteryStory | null = null;
let batteryMeta: string | null = null;

/** 月度故事计算（缓存供卡片渲染与测试注入）。 */
export function computeBatteryStory(input: BatteryStoryInput): BatteryStory {
  batteryStoryCache = batteryStory(input);
  return batteryStoryCache;
}

export function computeBatteryFromSamples(samples: BatterySample[], healthNow: number, healthPrev: number | null): BatteryStory {
  const drains: number[] = [];
  let dayStart = 0;
  let dayMin = 100;
  for (const s of samples) {
    const k = Math.floor(s.ts / 86_400_000);
    if (k !== dayStart) {
      if (dayStart) drains.push(dayMin);
      dayStart = k;
      dayMin = 100;
    }
    if (!s.charging) dayMin = Math.min(dayMin, s.pct);
  }
  if (dayStart) drains.push(dayMin);
  const avgDrain = drains.length ? Math.round((drains.reduce((a, b) => a + b, 0) / drains.length) * 10) / 10 : null;
  batteryMeta = `本月循环 ${countChargeCycles(samples)} 次`;
  return computeBatteryStory({
    cyclesThisMonth: countChargeCycles(samples),
    healthNowPct: healthNow,
    healthPrevPct: healthPrev,
    avgFullDrainPerDayPct: avgDrain,
  });
}

function batteryOpen(): void {
  if (!flagOn("W-091")) return;
  batteryClose();
  const card = document.createElement("div");
  card.className = "nova-hw-card nova-battery";
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-label", "电池健康叙事 Battery story");
  const title = document.createElement("div");
  title.className = "nova-hw-title";
  title.textContent = "电池故事 · BATTERY STORY";
  card.appendChild(title);
  const body = document.createElement("div");
  card.appendChild(body);
  const bar = taskbarEl();
  card.style.left = "12px";
  card.style.top = `${Math.max(8, (bar?.getBoundingClientRect().top ?? window.innerHeight - 54) - 120)}px`;
  document.body.appendChild(card);
  batteryCard = card;
  bag.push(() => {
    card.remove();
    if (batteryCard === card) batteryCard = null;
  });
  batteryRender();
}

function batteryRender(): void {
  if (!batteryCard?.isConnected) return;
  const body = batteryCard.querySelector("div:nth-child(2)");
  if (!body) return;
  body.textContent = "";
  if (batteryStoryCache == null) {
    const p = document.createElement("p");
    p.className = "nova-hw-meta";
    p.textContent = "等待 nova_pulse_ex 电池样本（台式机无电池时本卡如实隐藏）";
    body.appendChild(p);
    return;
  }
  const h = document.createElement("h4");
  h.textContent = batteryStoryCache.headline;
  body.appendChild(h);
  const m = document.createElement("div");
  m.className = "nova-hw-meta";
  m.textContent = [batteryStoryCache.trendText, batteryMeta].filter(Boolean).join(" · ");
  body.appendChild(m);
  const tips = document.createElement("ul");
  tips.className = "nova-hw-tips";
  for (const t of batteryStoryCache.tips) {
    const li = document.createElement("li");
    li.textContent = t;
    tips.appendChild(li);
  }
  body.appendChild(tips);
}

function batteryClose(): void {
  batteryCard?.remove();
  batteryCard = null;
}

// ---- W-097 屏幕工时簿 ----

const PANEL_KEY = `${NS}.panelhours.v1`;

interface PanelStore {
  primary: PanelRecord;
}

function loadPanels(): PanelStore {
  return lsGet<PanelStore>(PANEL_KEY, {
    primary: { hours: 0, oled: false, staticSinceMs: null, updated: 0 },
  });
}

function savePanels(s: PanelStore): void {
  lsSet(PANEL_KEY, s);
}

export function panelStore(): PanelStore {
  return loadPanels();
}

export function panelSetOled(oled: boolean): void {
  const s = loadPanels();
  s.primary.oled = oled;
  savePanels(s);
}

export function panelZero(): void {
  const s = loadPanels();
  savePanels({ primary: panelReset(s.primary, Date.now()) });
  novaEvent("panelhours", { reset: true });
}

let panelTimer: ReturnType<typeof setInterval> | null = null;

function panelStart(): void {
  if (panelTimer) return;
  panelTimer = setInterval(() => {
    if (!flagOn("W-097") || typeof document === "undefined" || document.hidden) return;
    const s = loadPanels();
    savePanels({ primary: panelAccumulate(s.primary, PANEL_TICK_MS / 1000, Date.now()) });
  }, PANEL_TICK_MS);
  bag.push(() => {
    if (panelTimer) clearInterval(panelTimer);
    panelTimer = null;
  });
}

function panelHoursOpen(): void {
  if (!flagOn("W-097")) return;
  panelHoursClose();
  const card = document.createElement("div");
  card.className = "nova-hw-card nova-panelhours";
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-label", "屏幕工时簿 Panel hours");
  const title = document.createElement("div");
  title.className = "nova-hw-title";
  title.textContent = "屏幕工时簿 · PANEL HOURS";
  card.appendChild(title);
  const body = document.createElement("div");
  card.appendChild(body);
  const bar = taskbarEl();
  card.style.right = "12px";
  card.style.top = `${Math.max(8, (bar?.getBoundingClientRect().top ?? window.innerHeight - 54) - 120)}px`;
  document.body.appendChild(card);
  bag.push(() => card.remove());
  const s = loadPanels();
  const h = document.createElement("h4");
  h.textContent = `主屏累计点亮 ${s.primary.hours.toFixed(1)} 小时`;
  body.appendChild(h);
  const wrap = document.createElement("div");
  wrap.className = "nova-hw-hours";
  const hb = document.createElement("div");
  hb.className = "nova-hw-hourbar";
  const fill = document.createElement("i");
  fill.style.width = `${Math.min(100, (s.primary.hours / 1000) * 100)}%`;
  hb.appendChild(fill);
  wrap.appendChild(hb);
  body.appendChild(wrap);
  const hints = oledHints({ ...s.primary, staticSinceMs: s.primary.staticSinceMs }, Date.now());
  for (const t of hints) {
    const p = document.createElement("div");
    p.className = "nova-hw-meta";
    p.textContent = t;
    body.appendChild(p);
  }
  if (!s.primary.oled) {
    const p = document.createElement("div");
    p.className = "nova-hw-dim";
    p.textContent = "非 OLED 面板：烧屏建议已如实跳过";
    body.appendChild(p);
  }
  const actions = document.createElement("div");
  actions.className = "nova-hw-actions";
  const zero = document.createElement("button");
  zero.type = "button";
  zero.textContent = "工时清零";
  zero.addEventListener("click", () => {
    panelZero();
    panelHoursClose();
  });
  actions.appendChild(zero);
  body.appendChild(actions);
}

function panelHoursClose(): void {
  document.querySelector<HTMLElement>(".nova-hw-card.nova-panelhours")?.remove();
}

// ---- W-099 USB 电流印 ----

const USB_KEY = `${NS}.usb.v1`;
let usbCard: HTMLElement | null = null;

function loadUsb(): UsbCurrentRow[] {
  return lsGet<UsbCurrentRow[]>(USB_KEY, []);
}

function onUsb(e: Event): void {
  if (!flagOn("W-099")) return;
  const d = (e as CustomEvent).detail as { name?: string; negotiatedMa?: number | null; actualMa?: number | null } | undefined;
  const row = currentPrint(d?.name ?? "USB 设备", d?.negotiatedMa ?? null, d?.actualMa ?? null);
  lsSet(USB_KEY, usbHistoryPush(loadUsb(), row));
  novaEvent("usb", row);
  if (usbCard?.isConnected) usbRender();
}

function usbOpen(): void {
  if (!flagOn("W-099")) return;
  usbClose();
  const card = document.createElement("div");
  card.className = "nova-hw-card nova-usb";
  card.setAttribute("role", "dialog");
  card.setAttribute("aria-label", "USB 电流印 Current print");
  card.innerHTML = "";
  const title = document.createElement("div");
  title.className = "nova-hw-title";
  title.textContent = "USB 电流印 · CURRENT PRINT";
  card.appendChild(title);
  const body = document.createElement("div");
  card.appendChild(body);
  const bar = taskbarEl();
  card.style.right = "12px";
  card.style.top = `${Math.max(8, (bar?.getBoundingClientRect().top ?? window.innerHeight - 54) - 160)}px`;
  document.body.appendChild(card);
  usbCard = card;
  bag.push(() => {
    card.remove();
    if (usbCard === card) usbCard = null;
  });
  usbRender();
}

function usbRender(): void {
  if (!usbCard?.isConnected) return;
  const body = usbCard.querySelector("div:nth-child(2)");
  if (!body) return;
  body.textContent = "";
  const hist = loadUsb();
  if (hist.length === 0) {
    const p = document.createElement("div");
    p.className = "nova-hw-meta";
    p.textContent = "暂无插入记录（读数来自 nova://hw-usb，不可读时如实 N/A）";
    body.appendChild(p);
    return;
  }
  for (const row of [...hist].reverse()) {
    const r = document.createElement("div");
    r.className = "nova-hw-row";
    if (row.hungry) r.classList.add("nova-hungry");
    const name = document.createElement("span");
    name.textContent = row.name;
    const val = document.createElement("b");
    val.textContent = row.row;
    r.append(name, val);
    body.appendChild(r);
  }
  const spark = document.createElement("div");
  spark.className = "nova-hw-meta";
  spark.textContent = `最近 ${hist.length} 次对比 · ≥${USB_HUNGRY_MA}mA 记为吃电大户`;
  body.appendChild(spark);
}

function usbClose(): void {
  usbCard?.remove();
  usbCard = null;
}

// ---- W-100 硬件生日书 ----

const BIRTHDAY_KEY = `${NS}.birthday.v1`;
let birthdayDoneDay = "";

function loadBirthdays(): HwBirthdayEntry[] {
  return lsGet<HwBirthdayEntry[]>(BIRTHDAY_KEY, []);
}

export function seedBirthdays(list: HwBirthdayEntry[]): void {
  const cur = loadBirthdays();
  const byId = new Map(cur.map((e) => [e.id, e]));
  for (const e of list) byId.set(e.id, e);
  lsSet(BIRTHDAY_KEY, [...byId.values()]);
  novaEvent("birthday-seed", { count: list.length });
}

function birthdayTick(): void {
  if (!flagOn("W-100")) return;
  const now = Date.now();
  const day = new Date(now).toDateString();
  if (day === birthdayDoneDay) return;
  birthdayDoneDay = day;
  const list = birthdayCheck(loadBirthdays(), now);
  if (list.length === 0) return;
  novaEvent("birthday", list);
  const tray = trayEl();
  if (!tray || document.querySelector(".nova-hw-cake")) return;
  const cake = document.createElement("span");
  cake.className = "nova-hw-cake";
  cake.setAttribute("role", "status");
  cake.title = list.map((a) => [a.line, a.healthLine].filter(Boolean).join(" · ")).join("\n");
  cake.textContent = `🎂 ×${list.length}`;
  tray.appendChild(cake);
  bag.push(() => cake.remove());
}

function onInstallDates(e: Event): void {
  const d = (e as CustomEvent).detail as { entries?: HwBirthdayEntry[] } | undefined;
  if (Array.isArray(d?.entries)) {
    seedBirthdays(d.entries);
    birthdayDoneDay = ""; // 新装机日入册后当日重查
    birthdayTick();
  }
}

// ---- W-101 环境分贝伴飞 ----

let dbRing: DbSample[] = [];
let dbPaused = false;
let dbLastAvg: number | null = null;

export function dbCompanionPaused(): boolean {
  return dbPaused;
}

export function dbCompanionSetPaused(paused: boolean): void {
  dbPaused = paused;
  if (paused) {
    dbRing = [];
    dbLastAvg = null;
  }
  novaEvent("db-pause", { paused });
}

export function dbCompanionState(): { avg: number | null; adjust: DbAdjust; ring: number } {
  return { avg: dbLastAvg, adjust: dbAdjust(dbLastAvg), ring: dbRing.length };
}

function dbTick(now: number): void {
  if (!flagOn("W-101") || dbPaused) return;
  void (async () => {
    const db = await sampleAudioEnv();
    if (db == null) return; // 声级源未就绪：如实零输出
    dbRing = dbRingPush(dbRing, db, now);
    const avg = dbRingAvg(dbRing);
    if (avg == null || avg === dbLastAvg) return;
    dbLastAvg = avg;
    novaEvent("db-adjust", { avgDb: avg, ...dbAdjust(avg) });
  })();
}

// ---- W-098 风扇补偿（事件出口） ----

let fanOn = false;

function fanApply(sample: HwPulseSample): void {
  if (!flagOn("W-098")) {
    if (fanOn) {
      fanOn = false;
      novaEvent("compensate", { gainDb: 0, reason: "off" });
    }
    return;
  }
  const fanPct = sample.fanPct;
  if (fanPct == null) return; // 风扇轴未就绪：如实零补偿
  const night = isNightHour(new Date().getHours());
  fanOn = fanHysteresis(fanOn, fanPct);
  const gainDb = fanCompensateDb(fanPct, night, true);
  novaEvent("compensate", { gainDb: fanOn ? gainDb : 0, fanPct, night });
}

// ---- 轮询主循环（2s；全部依赖开关与数据源，缺一即静默） ----

let pollTimer: ReturnType<typeof setInterval> | null = null;

function pollOnce(): void {
  void (async () => {
    const needPulse = flagOn("W-090") || flagOn("W-093") || flagOn("W-094") || flagOn("W-095") || flagOn("W-098");
    if (!needPulse) return;
    const sample = await samplePulse();
    if (!sample) return; // 数据源未就绪：相关 UI 保持诚实隐藏
    const now = Date.now();
    tideEmit(sample);
    bandApply(sample);
    chorusApply(sample, now);
    ledsApply(sample, now);
    fanApply(sample);
    if (flagOn("W-091") && sample.battery.present && sample.battery.healthPct != null) {
      // 月度叙事：健康趋势由脉冲轴给出（上月值由存储滚动）
      const histKey = `${NS}.battery.healthprev.v1`;
      const prev = lsGet<number | null>(histKey, null);
      computeBatteryFromSamples([], sample.battery.healthPct, prev);
      if (batteryCard?.isConnected) batteryRender();
    } else if (flagOn("W-091") && !sample.battery.present) {
      batteryClose(); // 台式机无电池：整卡如实隐藏
    }
  })();
}

function pollStart(): void {
  if (pollTimer) return;
  pollTimer = setInterval(pollOnce, TIDE_SAMPLE_MS);
  bag.push(() => {
    if (pollTimer) clearInterval(pollTimer);
    pollTimer = null;
  });
}

// ---- W-101 分贝伴飞秒级循环 ----

let dbTimer: ReturnType<typeof setInterval> | null = null;

function dbStart(): void {
  if (dbTimer) return;
  dbTimer = setInterval(() => dbTick(Date.now()), DB_SAMPLE_MS);
  bag.push(() => {
    if (dbTimer) clearInterval(dbTimer);
    dbTimer = null;
  });
}

// ---- 激活 / 卸载（幂等） ----

function onRegistryChange(): void {
  if (!active) return;
  if (flagOn("W-093")) bandMount();
  if (flagOn("W-095")) ledsMount();
  if (flagOn("W-097")) panelStart();
  if (flagOn("W-101")) dbStart();
}

/** 事件入口（Hub/命令面板经 CustomEvent 打开对应卡片）。 */
function onOpenFeature(e: Event): void {
  const d = (e as CustomEvent).detail as { card?: string } | undefined;
  switch (d?.card) {
    case "battery":
      batteryOpen();
      break;
    case "chorus":
      chorusOpen();
      break;
    case "panelhours":
      panelHoursOpen();
      break;
    case "usb":
      usbOpen();
      break;
    default:
      break;
  }
}

export function activateHwNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  ensureStyle();

  // 事件消费（W-092 麦克风 / W-096 插拔 / W-099 USB / W-100 装机日 / 卡片入口）
  window.addEventListener("nova://hw-mic-state", onMicState);
  bag.push(() => window.removeEventListener("nova://hw-mic-state", onMicState));
  window.addEventListener("nova://hw-power", onPower);
  bag.push(() => window.removeEventListener("nova://hw-power", onPower));
  window.addEventListener("nova://hw-usb", onUsb);
  bag.push(() => window.removeEventListener("nova://hw-usb", onUsb));
  window.addEventListener("nova://hw-install-dates", onInstallDates);
  bag.push(() => window.removeEventListener("nova://hw-install-dates", onInstallDates));
  window.addEventListener("nova://hw-open-card", onOpenFeature);
  bag.push(() => window.removeEventListener("nova://hw-open-card", onOpenFeature));
  window.addEventListener("nova://hw-chorus-open", chorusOpen);
  bag.push(() => window.removeEventListener("nova://hw-chorus-open", chorusOpen));
  window.addEventListener("nova://hw-battery-open", batteryOpen);
  bag.push(() => window.removeEventListener("nova://hw-battery-open", batteryOpen));
  window.addEventListener("nova://hw-panelhours-open", panelHoursOpen);
  bag.push(() => window.removeEventListener("nova://hw-panelhours-open", panelHoursOpen));
  window.addEventListener("nova://hw-usb-open", usbOpen);
  bag.push(() => window.removeEventListener("nova://hw-usb-open", usbOpen));

  // W-093 川流带挂载 / W-095 巡逻灯挂载 / W-097 工时 / W-101 分贝
  if (flagOn("W-093")) bandMount();
  if (flagOn("W-095")) ledsMount();
  if (flagOn("W-097")) panelStart();
  if (flagOn("W-101")) dbStart();

  // 脉搏轮询（W-090/093/094/095/098 共用）
  pollStart();

  // W-100 生日书当日首查（自节流：每日一次）
  birthdayTick();

  // S0 注册表变化 → 即时生效/失效
  bag.push(subscribeNovaShim());
}

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

export function deactivateHwNova(): void {
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
  chorusClose();
  batteryClose();
  usbClose();
  bandHideCard();
  boltEl?.remove();
  boltEl = null;
  boltLastAt = null;
  dbRing = [];
  dbLastAvg = null;
  dbPaused = false;
  fanOn = false;
  birthdayDoneDay = "";
  document.querySelectorAll(".nova-hw-card,.nova-hw-band-card,.nova-hw-cake").forEach((n) => n.remove());
}

export function isHwNovaActive(): boolean {
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
