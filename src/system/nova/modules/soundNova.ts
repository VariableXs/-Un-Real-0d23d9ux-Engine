/**
 * NOVA-200 · S13 声音通知路（AI-13）—— 域13 声音与通知（W-152…W-163）。
 *
 * 边界（全景 §13）：W-009 启动里程碑微音属域1、W-092 麦克风风铃属域8、
 * W-072 传输律动声属域6 —— 本路只管「系统声纹与通知编排」：
 * W-152 声纹博物馆（导览+就地改音）、W-153 通知时间热图、W-154 晨间电台
 * （novaVoice 消费）、W-155 双耳平衡、W-156 勿扰雨滴、W-157 音量晨昏曲线、
 * W-158 工作整点钟声、W-159 勿扰触觉回声、W-160 声纹钢琴、W-161 声纹历史、
 * W-162 通知队列聚光、W-163 和声错峰。
 *
 * 纪律：
 * - 零侵入：不改写任何既有组件内部逻辑；全部为 DOM 叠层 + `nova://sound-*`
 *   自定义事件摄入 + `nova.sound.*` 本地存储（外部接线由 S17 按 wiringHint 补齐）；
 * - 前缀：类名 `nova-sound-`、事件 `nova://sound-*`、localStorage 键 `nova.sound.*`；
 * - 开关：只读消费 S0 注册表（registry.ts novaOn），无注册表时用卡片
 *   defaultOn 回退（诚实降级，不报错）；
 * - 降级：reduce-motion / safeMode / static 三态下动效归零（雨滴落体动画/
 *   聚光转场/热图扫描线），语义与数据保留；非 DOM 环境行为层安全 no-op；
 * - 默认档：W-154 晨间电台默认关（opt-in，与 S0 注册表一致），其余默认开。
 *
 * 诚实边界：
 * - W-154 本机无离线 TTS 引擎时**如实跳过**（RADIO_UNAVAILABLE），不静音假播；
 * - W-155 校准缺失时补偿 0dB（不猜）；补偿上限 ±3dB 温和原则；
 * - W-156 每条勿扰通知恰一声雨滴，雨滴计数与通知条数一致（存档核对）；
 * - W-158 只在**工作会话内**跨整点响一声，钟声每整点至多一次；
 * - W-162 队列上限 5 条，超出转存档**零丢失**（存档核对断言）；
 * - W-163 同频段并发错峰步长 60ms（感知阈内）；不同频段不延迟；
 * - 本模块**零网络、零音频合成引擎**：播放经 nova://sound-play 事件移交
 *   系统声底层（S17 接线），本路只做编排、测量与语义。
 */

import { novaMotionOK, novaNum, novaOn } from "../registry";

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

const NS = "nova.sound";
const HOUR = 3_600_000;


/** 功能开关：只读消费 S0 注册表。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

/** 派发 `nova://sound-*` 事件（SSR/测试环境安全）。 */
export function soundEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined" || typeof window.dispatchEvent !== "function") return;
  window.dispatchEvent(new CustomEvent(`nova://sound-${name}`, { detail }));
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
  /** 独立 overlay 工具窗（ai04:open-feature 的 feature id）。 */
  overlay?: string;
  /** 需要 S17 在既有文件补接线的行为（本模块 API/事件已备好）。 */
  wiringHint?: string;
  /** reduce-motion / safeMode / static 降级说明。 */
  degrade: string;
}

export const SOUND_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-152",
    titleZh: "声纹博物馆",
    titleEn: "Sound Museum",
    descZh: "3 分钟系统音导览：十二展品逐件试听并讲出来历；每件展品就地改音（音高/音量），改完即存不入库外。",
    defaultOn: true,
    overlay: "nova-museum",
    wiringHint: "改音结果经 nova://sound-tweaks {tweaks} 移交系统声底层重映射",
    degrade: "静态展签清单与试听按钮；导览进度条无扫描动画",
  },
  {
    id: "W-153",
    titleZh: "通知时间热图",
    titleEn: "Notify Heatmap",
    descZh: "7×24 打扰时间地理：真实通知时间戳沉积为周×小时热图，找出你的打扰高峰时段。",
    defaultOn: true,
    overlay: "nova-heat",
    wiringHint: "通知经 nova://sound-notify {id,at,src} 摄入（notifyStore 桥）",
    degrade: "静态色块网格，无扫描线巡游",
  },
  {
    id: "W-154",
    titleZh: "晨间电台",
    titleEn: "Morning Radio",
    descZh: "首解锁后可选的 90s 三段播报（天气/今日日程/一句好话），离线本地 TTS；无引擎如实跳过不假播（opt-in 默认关）。",
    defaultOn: false,
    wiringHint: "TTS 经 novaVoice 底层播报；数据经 nova://sound-radio-data 摄入",
    degrade: "纯文本字幕卡逐段展示，不调用任何语音",
  },
  {
    id: "W-155",
    titleZh: "双耳平衡罗盘",
    titleEn: "Balance Compass",
    descZh: "六题校准测试找出你的偏耳侧，给出 ±3dB 温和补偿并实时预览；不猜：未校准即 0dB。",
    defaultOn: true,
    wiringHint: "补偿值经 nova://sound-balance {dbL,dbR} 移交音频底层",
    degrade: "静态罗盘指针；测试与补偿逻辑不变",
  },
  {
    id: "W-156",
    titleZh: "勿扰雨滴",
    titleEn: "DND Raindrops",
    descZh: "勿扰期间通知不再弹窗打断，转为一声轻雨滴按序落下；每条通知恰一声，条数可核对。",
    defaultOn: true,
    wiringHint: "雨滴播放经 nova://sound-play {soundId:'dnd-rain'} ；DND 态经 nova://sound-dnd {on} 摄入",
    degrade: "雨滴无落体动画，仅一声提示与计数徽标",
  },
  {
    id: "W-157",
    titleZh: "音量晨昏曲线",
    titleEn: "Daypart Volume",
    descZh: "全局音量随一天三段自动起伏（晨起缓升/日间满格/入夜回落），手动调节即刻让位 60 分钟。",
    defaultOn: true,
    wiringHint: "曲线输出经 nova://sound-daypart {pct} 移交音量底层；手动让位窗口由本模块记账",
    degrade: "静态曲线图；自动调节照常（非动效）",
  },
  {
    id: "W-158",
    titleZh: "工作整点钟声",
    titleEn: "Hour Chime",
    descZh: "工作会话内跨整点响一声 200ms 软钟声，像老茶馆的报时；会话外静默。",
    defaultOn: true,
    wiringHint: "会话起止经 nova://sound-session {start,end} 摄入；播放经 nova://sound-play {soundId:'hour-chime'}",
    degrade: "钟声语义不变（声音非动效）",
  },
  {
    id: "W-159",
    titleZh: "勿扰触觉回声",
    titleEn: "Haptic Echo",
    descZh: "勿扰期通知在任务栏留一次 90ms 微颤知觉残留，看得见摸得着的「刚才有人敲门」。",
    defaultOn: true,
    wiringHint: "通知经 nova://sound-notify 摄入（DND 态联动 W-156）；颤动脉冲经 nova://sound-tremor {pulses} 移交任务栏（AI-04 只读消费）",
    degrade: "静态 1px 微光脉冲一次，无往复颤动",
  },
  {
    id: "W-160",
    titleZh: "声纹钢琴",
    titleEn: "Sound Piano",
    descZh: "12 键试音键盘：一个八度十二半音，每键映射一件系统音，弹着找你喜欢的声纹。",
    defaultOn: true,
    overlay: "nova-piano",
    wiringHint: "按键播放经 nova://sound-play {soundId} 移交底层",
    degrade: "键盘可弹（声音非动效）；无按键下沉动画",
  },
  {
    id: "W-161",
    titleZh: "声纹历史",
    titleEn: "Sound History",
    descZh: "最近 3 次系统声微史（谁在何时响了什么），一键重播任一条；只记 3 条不囤积。",
    defaultOn: true,
    wiringHint: "系统声播放事件经 nova://sound-played {soundId,at} 回流登记",
    degrade: "静态历史列表；重播语义不变",
  },
  {
    id: "W-162",
    titleZh: "通知队列聚光",
    titleEn: "Notify Spotlight",
    descZh: "通知不再齐轰：进入队列逐条聚光上演，每条独享 2.4s；队列上限 5 条，超出转存档零丢失。",
    defaultOn: true,
    wiringHint: "通知经 nova://sound-notify 摄入；聚光条渲染由本模块叠层完成",
    degrade: "聚光无淡入转场，逐条上台顺序不变",
  },
  {
    id: "W-163",
    titleZh: "声纹和声分",
    titleEn: "Harmony Split",
    descZh: "并发系统声按频段错峰：同频段撞车的声音依次让 60ms，人耳无感但不再糊成一声。",
    defaultOn: true,
    wiringHint: "播放请求经 nova://sound-played 摄入后由本模块改排 nova://sound-play {delayMs}",
    degrade: "错峰逻辑不变（延迟非动效）",
  },
];

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-152 声纹博物馆
// ---------------------------------------------------------------------------

export interface SoundProfile {
  soundId: string;
  titleZh: string;
  titleEn: string;
  /** 出身一句话（展品讲牌）。 */
  originZh: string;
  /** 基准频段（W-163 错峰用）。 */
  band: Band;
  /** 原始时长 ms。 */
  ms: number;
}

export type Band = "low" | "mid" | "high";

/** 十二展品（对应 W-160 十二键；系统音清单与底层同源）。 */
export const SOUND_LIBRARY: SoundProfile[] = [
  { soundId: "sys-boot", titleZh: "点火", titleEn: "Ignition", originZh: "每次开机第一声，熔炉制式的低音余烬", band: "low", ms: 1200 },
  { soundId: "sys-unlock", titleZh: "解锁", titleEn: "Unlock", originZh: "登录成功的木琴双音", band: "mid", ms: 400 },
  { soundId: "sys-notify", titleZh: "通知", titleEn: "Notify", originZh: "应用来信的标准敲门声", band: "mid", ms: 300 },
  { soundId: "sys-battery-low", titleZh: "低电", titleEn: "Low Battery", originZh: "电量 10% 的提醒短笛", band: "high", ms: 350 },
  { soundId: "sys-plug-in", titleZh: "接入", titleEn: "Plug In", originZh: "电源接通的落雷前奏", band: "low", ms: 500 },
  { soundId: "usb-arrive", titleZh: "到港", titleEn: "USB Arrive", originZh: "U 盘插入的到港汽笛", band: "mid", ms: 450 },
  { soundId: "usb-depart", titleZh: "离港", titleEn: "USB Depart", originZh: "安全弹出的离港汽笛", band: "mid", ms: 450 },
  { soundId: "mail-arrive", titleZh: "来信", titleEn: "Mail", originZh: "新邮件的邮差铃声", band: "high", ms: 400 },
  { soundId: "trash-empty", titleZh: "倒垃圾", titleEn: "Empty Trash", originZh: "清空回收站的揉纸声", band: "low", ms: 700 },
  { soundId: "screenshot", titleZh: "快门", titleEn: "Shutter", originZh: "截图成功的老相机快门", band: "high", ms: 200 },
  { soundId: "volume-step", titleZh: "音量阶", titleEn: "Volume Step", originZh: "音量加减的嗒声刻度", band: "high", ms: 90 },
  { soundId: "dnd-rain", titleZh: "雨滴", titleEn: "Raindrop", originZh: "勿扰期间替通知说话的一声轻雨（W-156 专属）", band: "high", ms: 180 },
];

export interface MuseumStop {
  soundId: string;
  atSec: number;
  durationSec: number;
}

/**
 * 3 分钟导览排期：十二展品均分 180s（12×15s = 180s 恰满），
 * 每站含展品试听与讲牌停留；总时长恒 ≤ 180s。
 */
export function tourTimeline(lib: SoundProfile[] = SOUND_LIBRARY): { stops: MuseumStop[]; totalSec: number } {
  const per = 15;
  const stops = lib.slice(0, 12).map((s, i) => ({ soundId: s.soundId, atSec: i * per, durationSec: per }));
  return { stops, totalSec: stops.length * per };
}

export interface SoundTweak {
  soundId: string;
  /** 音高偏移（音分）。 */
  pitchCents: number;
  /** 音量偏移 dB。 */
  gainDb: number;
}

export const TWEAK_PITCH_LIMIT = 600;
export const TWEAK_GAIN_MIN = -12;
export const TWEAK_GAIN_MAX = 6;

/** 就地改音：音高 ±600 音分、音量 -12…+6dB，超限温和夹取。 */
export function clampTweak(t: SoundTweak): SoundTweak {
  return {
    soundId: t.soundId,
    pitchCents: clamp(Math.round(t.pitchCents), -TWEAK_PITCH_LIMIT, TWEAK_PITCH_LIMIT),
    gainDb: clamp(Math.round(t.gainDb * 2) / 2, TWEAK_GAIN_MIN, TWEAK_GAIN_MAX),
  };
}

export function applyTweak(base: SoundProfile, tweak: SoundTweak | undefined): SoundProfile & { tweak: SoundTweak } {
  const tw = tweak ? clampTweak(tweak) : { soundId: base.soundId, pitchCents: 0, gainDb: 0 };
  return { ...base, tweak: tw };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-153 通知时间热图
// ---------------------------------------------------------------------------

export type HeatGrid = number[][]; // [7][24]

export function emptyGrid(): HeatGrid {
  return Array.from({ length: 7 }, () => Array<number>(24).fill(0));
}

/** 通知时间戳 → 周×小时桶沉积（0=周日，与 Date.getDay 同源）。 */
export function heatDeposit(grid: HeatGrid, atMs: number): HeatGrid {
  const d = new Date(atMs);
  const day = d.getDay();
  const hour = d.getHours();
  const out = grid.map((row) => row.slice());
  out[day]![hour] = (out[day]![hour] ?? 0) + 1;
  return out;
}

export function heatMax(grid: HeatGrid): number {
  let m = 0;
  for (const row of grid) for (const c of row) if (c > m) m = c;
  return m;
}

/** 五档热级 0…4（0=无打扰）。 */
export function heatLevel(count: number, max: number): 0 | 1 | 2 | 3 | 4 {
  if (max <= 0 || count <= 0) return 0;
  const r = count / max;
  if (r > 0.75) return 4;
  if (r > 0.5) return 3;
  if (r > 0.25) return 2;
  return 1;
}

export interface HeatPeak {
  day: number;
  hour: number;
  count: number;
}

/** 打扰高峰格（并列全列）。 */
export function heatPeaks(grid: HeatGrid): HeatPeak[] {
  const max = heatMax(grid);
  if (max <= 0) return [];
  const peaks: HeatPeak[] = [];
  for (let d = 0; d < 7; d++) for (let h = 0; h < 24; h++) if (grid[d]![h] === max) peaks.push({ day: d, hour: h, count: max });
  return peaks;
}

/** 热图 CSS 色阶变量名（nova.css 消费）。 */
export function heatColor(level: 0 | 1 | 2 | 3 | 4): string {
  return `var(--nova-sound-heat-${level})`;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-154 晨间电台
// ---------------------------------------------------------------------------

export const RADIO_BUDGET_SEC = 90;

export interface RadioSeg {
  key: "weather" | "agenda" | "quote";
  titleZh: string;
  /** 播报文案（无 TTS 时作字幕）。 */
  text: string;
  sec: number;
}

export interface RadioInput {
  weatherZh: string;
  agendaItems: string[];
  quoteZh: string;
}

/** 按 30/40/20s 三段配额裁剪文案（30+40+20=90s 恰满预算）。 */
export function radioScript(input: RadioInput): RadioSeg[] {
  const cut = (text: string, sec: number): string => {
    // 4 字/秒 播报速率，预留 2s 段间气口
    const budget = Math.max(0, sec * 4 - 2);
    return text.length > budget ? `${text.slice(0, Math.max(0, budget - 1))}…` : text;
  };
  return [
    { key: "weather", titleZh: "天气", sec: 30, text: cut(input.weatherZh, 30) },
    {
      key: "agenda",
      titleZh: "日程",
      sec: 40,
      text: cut(
        input.agendaItems.length > 0
          ? `今日有 ${input.agendaItems.length} 件事：${input.agendaItems.join("；")}`
          : "今天没有排定的日程，自由发挥。",
        40,
      ),
    },
    { key: "quote", titleZh: "一句好话", sec: 20, text: cut(input.quoteZh, 20) },
  ];
}

/** 电台总时长（秒）。 */
export function radioTotalSec(segs: RadioSeg[]): number {
  return segs.reduce((a, s) => a + s.sec, 0);
}

export type RadioVerdict =
  | { ok: true; segs: RadioSeg[] }
  | { ok: false; code: "RADIO_UNAVAILABLE" | "RADIO_OVER_BUDGET"; reason: string };

/** 播报裁决：无 TTS 引擎如实跳过（不假播）；超 90s 拒播。 */
export function radioVerdict(hasTts: boolean, segs: RadioSeg[]): RadioVerdict {
  if (!hasTts) return { ok: false, code: "RADIO_UNAVAILABLE", reason: "本机无离线 TTS 引擎，晨间电台如实跳过" };
  if (radioTotalSec(segs) > RADIO_BUDGET_SEC) {
    return { ok: false, code: "RADIO_OVER_BUDGET", reason: `脚本超预算（${radioTotalSec(segs)}s > ${RADIO_BUDGET_SEC}s）` };
  }
  return { ok: true, segs };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-155 双耳平衡罗盘
// ---------------------------------------------------------------------------

export const BALANCE_DB_LIMIT = 3;

export interface BalanceTrial {
  /** 用户判断声像偏哪侧（-1 左 / 0 中 / 1 右）。 */
  said: -1 | 0 | 1;
  /** 实际声像偏移（-1…1）。 */
  actual: number;
}

/** 六题校准 → 偏耳侧因子（-1…1，正=右耳占优）。 */
export function balanceFactor(trials: BalanceTrial[]): number {
  if (trials.length === 0) return 0;
  let err = 0;
  for (const t of trials) err += t.actual - t.said;
  return clamp(err / trials.length, -1, 1);
}

/** 因子 → ±3dB 补偿（弱耳侧补，0.5dB 步进；0 因子不补偿）。 */
export function balanceDb(factor: number): { dbL: number; dbR: number } {
  const db = Math.round(Math.abs(factor) * BALANCE_DB_LIMIT * 2) / 2;
  if (db === 0) return { dbL: 0, dbR: 0 };
  return factor > 0
    ? { dbL: db, dbR: 0 } // 右占优 → 补左
    : { dbL: 0, dbR: db };
}

/** 预览：左右声道最终增益（基线 gain ± 补偿）。 */
export function applyBalance(gainDb: number, comp: { dbL: number; dbR: number }): { dbL: number; dbR: number } {
  return { dbL: gainDb + comp.dbL, dbR: gainDb + comp.dbR };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-156 勿扰雨滴
// ---------------------------------------------------------------------------

export const RAIN_DROP_GAP_MS = 420;

export interface RainDrop {
  ntfId: string;
  atMs: number;
}

/** 每条勿扰通知恰一声雨滴，按到达序 420ms 间隔排落（条数守恒）。 */
export function rainDrops(notifs: Array<{ id: string; at: number }>): RainDrop[] {
  return notifs.map((n, i) => ({ ntfId: n.id, atMs: n.at + i * RAIN_DROP_GAP_MS }));
}

/** 雨滴音量（W-156 gain 参数 0…40 → 0…0.4 归一）。 */
export function rainGain(gainParam: number): number {
  return clamp(gainParam, 0, 40) / 100;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-157 音量晨昏曲线
// ---------------------------------------------------------------------------

export const DAYPART_MANUAL_YIELD_MS = 60 * 60_000;

export interface DaypartPoint {
  hour: number;
  /** 0…1 曲线系数。 */
  k: number;
}

/** 三段日曲线：晨起缓升(5-9) / 日间满格(9-18) / 入夜回落(18-23) / 夜间地板 0.4。 */
export function dayCurve(hour: number): number {
  const h = ((Math.round(hour) % 24) + 24) % 24;
  if (h < 5) return 0.4;
  if (h < 9) return 0.4 + ((h - 5) / 4) * 0.6; // 0.4 → 1.0
  if (h < 18) return 1.0;
  if (h < 23) return 1.0 - ((h - 18) / 5) * 0.6; // 1.0 → 0.4
  return 0.4;
}

export interface DaypartState {
  /** 用户基准音量 0…100。 */
  base: number;
  /** 手动调节时间戳（让位窗口内不自动调）。 */
  manualAt: number;
}

/** 手动让位：60 分钟内手动优先，之后曲线接管。 */
export function daypartPct(state: DaypartState, nowMs: number): { pct: number; source: "curve" | "manual" } {
  if (state.manualAt > 0 && nowMs - state.manualAt < DAYPART_MANUAL_YIELD_MS) {
    return { pct: state.base, source: "manual" };
  }
  const k = dayCurve(new Date(nowMs).getHours());
  return { pct: Math.round(state.base * k), source: "curve" };
}

/** 手动调节登记（产生让位窗口）。 */
export function daypartManual(_prev: DaypartState, pct: number, nowMs: number): DaypartState {
  return { base: clamp(Math.round(pct), 0, 100), manualAt: nowMs };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-158 工作整点钟声
// ---------------------------------------------------------------------------

export const CHIME_MS = 200;

export interface ChimeState {
  /** 会话开始时间戳（0=无会话）。 */
  sessionStart: number;
  /** 会话结束时间戳（0=进行中）。 */
  sessionEnd: number;
  /** 上次钟声的整点时间戳。 */
  lastChimeHour: number;
}

/** 所在整点地板（时间戳对齐）。 */
export function hourFloor(ms: number): number {
  return Math.floor(ms / HOUR) * HOUR;
}

/** 会话内跨整点 → 恰响一声；每整点至多一次；会话外静默；会话开始的整点不报。 */
export function chimeDue(st: ChimeState, nowMs: number): boolean {
  if (st.sessionStart <= 0) return false;
  if (nowMs < st.sessionStart) return false;
  if (st.sessionEnd > 0 && nowMs >= st.sessionEnd) return false;
  const h = hourFloor(nowMs);
  if (h === hourFloor(st.sessionStart)) return false;
  return h > st.lastChimeHour;
}

/** 响铃后登记（防重）。 */
export function chimeMark(st: ChimeState, nowMs: number): ChimeState {
  return { ...st, lastChimeHour: hourFloor(nowMs) };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-159 勿扰触觉回声
// ---------------------------------------------------------------------------

export const TREMOR_PULSE_MS = 90;
export const TREMOR_MAX_PULSES = 4;

/** 勿扰通知 → 任务栏微颤脉冲数（1 条 2 脉冲，封顶 4）。 */
export function tremorPulses(notifCount: number): number {
  if (notifCount <= 0) return 0;
  return Math.min(TREMOR_MAX_PULSES, 1 + notifCount);
}

/** 脉冲时序（ms 相对起点）。 */
export function tremorTimeline(pulses: number): number[] {
  return Array.from({ length: Math.max(0, pulses) }, (_, i) => i * TREMOR_PULSE_MS);
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-160 声纹钢琴
// ---------------------------------------------------------------------------

export const PIANO_NOTE_ZH: string[] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/** 12 半音键位 → 展品音（semitone i 对应 SOUND_LIBRARY[i]）。 */
export function pianoKeys(): Array<{ key: string; semitone: number; soundId: string; note: string }> {
  return SOUND_LIBRARY.slice(0, 12).map((s, i) => ({ key: `Key${i + 1}`, semitone: i, soundId: s.soundId, note: PIANO_NOTE_ZH[i] ?? String(i) }));
}

export function pianoSoundId(semitone: number): string | null {
  const i = Math.round(semitone);
  if (i < 0 || i > 11) return null;
  return SOUND_LIBRARY[i]?.soundId ?? null;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-161 声纹历史
// ---------------------------------------------------------------------------

export interface SoundEvent {
  soundId: string;
  at: number;
}

export const HISTORY_CAP = 3;

/** 最近 3 声微史（新在前，FIFO 淘汰）。 */
export function pushHistory(hist: SoundEvent[], ev: SoundEvent, cap: number = HISTORY_CAP): SoundEvent[] {
  return [ev, ...hist.filter((h) => !(h.soundId === ev.soundId && h.at === ev.at))].slice(0, cap);
}

/** 重播目标解析。 */
export function historyLookup(hist: SoundEvent[], idx: number): SoundEvent | null {
  return hist[idx] ?? null;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-162 通知队列聚光
// ---------------------------------------------------------------------------

export const SPOTLIGHT_CAP = 5;
export const SPOTLIGHT_STAGE_MS = 2400;

export interface SpotlightItem {
  ntfId: string;
  title: string;
  body: string;
  at: number;
}

export interface SpotlightLedger {
  queue: SpotlightItem[];
  archive: SpotlightItem[];
}

/** 入队：上限 5 条，超出转存档（零丢失：queue+archive 总数守恒）。 */
export function spotlightEnqueue(l: SpotlightLedger, item: SpotlightItem): SpotlightLedger {
  if (l.queue.some((q) => q.ntfId === item.ntfId)) return l; // 幂等去重
  if (l.queue.length < SPOTLIGHT_CAP) return { ...l, queue: [...l.queue, item] };
  return { ...l, archive: [...l.archive, item] };
}

/** 逐条聚光：队首上台，其余候场。 */
export function spotlightDequeue(l: SpotlightLedger): { item: SpotlightItem | null; ledger: SpotlightLedger } {
  const head = l.queue[0];
  if (!head) return { item: null, ledger: l };
  return { item: head, ledger: { ...l, queue: l.queue.slice(1) } };
}

/** 存档核对：总数守恒断言依据。 */
export function spotlightTotal(l: SpotlightLedger): number {
  return l.queue.length + l.archive.length;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-163 声纹和声分
// ---------------------------------------------------------------------------

export const STAGGER_MS = 60;

export interface StaggerRequest {
  soundId: string;
  band: Band;
}

export interface StaggerAssign {
  soundId: string;
  /** 相对第一声的延迟 ms（0 = 不让）。 */
  offsetMs: number;
}

/** 同频段并发错峰：同 band 第 i 声让 i×60ms；不同频段零延迟（60ms 感知阈内）。 */
export function staggerAssign(reqs: StaggerRequest[]): StaggerAssign[] {
  const perBand = new Map<Band, number>();
  return reqs.map((r) => {
    const n = perBand.get(r.band) ?? 0;
    perBand.set(r.band, n + 1);
    return { soundId: r.soundId, offsetMs: n * STAGGER_MS };
  });
}

/** 频段归类（未知 soundId 回 mid）。 */
export function bandOf(soundId: string): Band {
  return SOUND_LIBRARY.find((s) => s.soundId === soundId)?.band ?? "mid";
}

// ---------------------------------------------------------------------------
// 行为层（DOM 叠层 + 事件摄入）
// ---------------------------------------------------------------------------

let active = false;
let bag: Array<() => void> = [];
let panel: HTMLDivElement | null = null;
let curTab: "museum" | "heat" | "radio" | "balance" | "piano" = "museum";
let styleEl: HTMLStyleElement | null = null;

// 记账态
let tweaks: Record<string, SoundTweak> = {};
let heat: HeatGrid = emptyGrid();
let heatCount = 0;
let radio: { script: RadioSeg[]; idx: number; timer: number } = { script: [], idx: 0, timer: 0 };
let balanceComp: { dbL: number; dbR: number } = { dbL: 0, dbR: 0 };
let dndOn = false;
let dndNotifs: Array<{ id: string; at: number }> = [];
let daypart: DaypartState = { base: 50, manualAt: 0 };
let chime: ChimeState = { sessionStart: 0, sessionEnd: 0, lastChimeHour: 0 };
let history: SoundEvent[] = [];
let spotlight: SpotlightLedger = { queue: [], archive: [] };
let spotTimer = 0;
let spotEl: HTMLDivElement | null = null;
let tremorEl: HTMLDivElement | null = null;

function loadState(): void {
  tweaks = lsGet<Record<string, SoundTweak>>(`${NS}.tweaks.v1`, {});
  balanceComp = lsGet(`${NS}.balance.v1`, { dbL: 0, dbR: 0 });
  daypart = lsGet(`${NS}.daypart.v1`, { base: 50, manualAt: 0 });
  chime = lsGet(`${NS}.chime.v1`, { sessionStart: 0, sessionEnd: 0, lastChimeHour: 0 });
  history = lsGet<SoundEvent[]>(`${NS}.history.v1`, []);
  heat = lsGet<HeatGrid>(`${NS}.heat.v1`, emptyGrid());
  heatCount = lsGet(`${NS}.heat.count.v1`, 0);
}

// --- 样式 ---

function ensureStyle(): void {
  if (styleEl || typeof document === "undefined") return;
  styleEl = document.createElement("style");
  styleEl.id = "nova-sound-style";
  styleEl.textContent = `
.nova-sound-panel{position:fixed;right:16px;bottom:64px;width:520px;max-height:70vh;overflow:auto;
  background:var(--nova-sound-bg,#16181d);color:var(--nova-sound-fg,#e8eaf0);border:1px solid #2a2d35;
  border-radius:12px;font:12px/1.6 system-ui,sans-serif;z-index:2147000000;box-shadow:0 12px 40px #0008}
.nova-sound-head{display:flex;align-items:center;gap:8px;padding:10px 14px;border-bottom:1px solid #2a2d35}
.nova-sound-title{font-weight:600;letter-spacing:.02em}
.nova-sound-tabs{display:flex;gap:2px;padding:8px 10px 0}
.nova-sound-tab{padding:4px 10px;border-radius:6px;cursor:pointer;color:#9aa3b2}
.nova-sound-tab[data-on="1"]{background:#2a2d35;color:#fff}
.nova-sound-body{padding:12px 14px}
.nova-sound-card{border:1px solid #2a2d35;border-radius:8px;padding:8px 10px;margin-bottom:8px}
.nova-sound-card b{font-weight:600}
.nova-sound-muted{color:#9aa3b2}
.nova-sound-stop{display:flex;justify-content:space-between;gap:8px;padding:3px 0;cursor:pointer}
.nova-sound-stop:hover{color:#fff}
.nova-sound-heat{display:grid;grid-template-columns:auto repeat(24,1fr);gap:2px;font-size:10px}
.nova-sound-cell{width:12px;height:12px;border-radius:2px;background:var(--nova-sound-heat-0,#1b1e24)}
.nova-sound-piano{display:grid;grid-template-columns:repeat(12,1fr);gap:3px;margin-top:6px}
.nova-sound-key{height:64px;border-radius:0 0 5px 5px;background:#22252c;border:1px solid #2a2d35;
  display:flex;align-items:flex-end;justify-content:center;padding-bottom:4px;font-size:9px;color:#9aa3b2;cursor:pointer}
.nova-sound-key:active,.nova-sound-key[data-hit="1"]{background:#3a4254;color:#fff}
.nova-sound-spot{position:fixed;left:50%;transform:translateX(-50%);top:72px;width:380px;z-index:2147000001;
  background:#1c1f26;border:1px solid #3a4254;border-radius:12px;padding:12px 16px;box-shadow:0 16px 48px #000a}
.nova-sound-spot b{display:block;margin-bottom:4px;font-size:13px}
.nova-sound-tremor{position:fixed;left:50%;transform:translateX(-50%);bottom:52px;height:2px;width:120px;z-index:2147000000;
  background:var(--nova-sound-pulse,#7aa2ff);opacity:0;border-radius:1px;pointer-events:none}
.nova-sound-tremor[data-on="1"]{opacity:.8}
.nova-sound-prog{height:3px;border-radius:2px;background:#2a2d35;overflow:hidden;margin:6px 0}
.nova-sound-prog i{display:block;height:100%;background:#7aa2ff}
`;
  document.head.appendChild(styleEl);
}

// --- 面板 ---

function togglePanel(force?: boolean): void {
  if (typeof document === "undefined") return;
  const want = force ?? !panel;
  if (want && !panel) {
    panel = document.createElement("div");
    panel.className = "nova-sound-panel";
    document.body.appendChild(panel);
    bindPanel();
    renderPanel();
  } else if (!want && panel) {
    panel.remove();
    panel = null;
  }
}

function esc(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c] ?? c);
}

function renderPanel(): void {
  if (!panel) return;
  const tabs: Array<[typeof curTab, string]> = [
    ["museum", "声纹馆"],
    ["heat", "时间热图"],
    ["radio", "晨间电台"],
    ["balance", "双耳罗盘"],
    ["piano", "声纹钢琴"],
  ];
  const head = `<div class="nova-sound-head"><span class="nova-sound-title">◈ 声音与通知 · NOVA</span>
    <span style="flex:1"></span><span class="nova-sound-muted" data-act="close" style="cursor:pointer">✕</span></div>
    <div class="nova-sound-tabs">${tabs
      .map(([k, zh]) => `<span class="nova-sound-tab" data-tab="${k}" data-on="${curTab === k ? 1 : 0}">${zh}</span>`)
      .join("")}</div><div class="nova-sound-body">`;
  let body = "";
  if (curTab === "museum") {
    const { stops, totalSec } = tourTimeline();
    body = `<div class="nova-sound-card"><b>3 分钟导览</b> <span class="nova-sound-muted">共 ${stops.length} 站 · ${totalSec}s</span>
      <div class="nova-sound-prog"><i style="width:0%"></i></div>
      <div class="nova-sound-muted" data-act="tour">▶ 开始导览（点击展品可就地改音）</div></div>` +
      SOUND_LIBRARY.map((s) => {
        const tw = tweaks[s.soundId];
        return `<div class="nova-sound-card" data-stop="${s.soundId}"><b>${esc(s.titleZh)}</b> <span class="nova-sound-muted">${esc(s.titleEn)} · ${s.band}</span>
        <div class="nova-sound-muted">${esc(s.originZh)}</div>
        <div class="nova-sound-muted">改音：${tw ? `${tw.pitchCents >= 0 ? "+" : ""}${tw.pitchCents}¢ / ${tw.gainDb >= 0 ? "+" : ""}${tw.gainDb}dB` : "原声"} <span data-act="tweak" data-sound="${s.soundId}" style="cursor:pointer;text-decoration:underline">改</span> <span data-act="play" data-sound="${s.soundId}" style="cursor:pointer;text-decoration:underline">试听</span></div></div>`;
      }).join("");
  } else if (curTab === "heat") {
    const max = heatMax(heat);
    const peaks = heatPeaks(heat);
    const daysZh = ["日", "一", "二", "三", "四", "五", "六"];
    body = `<div class="nova-sound-card"><b>7×24 通知时间热图</b> <span class="nova-sound-muted">已沉积 ${heatCount} 条</span></div>
      <div class="nova-sound-heat"><span></span>${Array.from({ length: 24 }, (_, h) => `<span class="nova-sound-muted">${h % 6 === 0 ? h : ""}</span>`).join("")}
      ${heat
        .map((row, d) =>
          `<span class="nova-sound-muted">周${daysZh[d]}</span>` +
          row
            .map((c, h) => `<span class="nova-sound-cell" style="background:${heatColor(heatLevel(c, max))}" title="周${daysZh[d]} ${h2(h)}:00 · ${c} 条"></span>`)
            .join(""))
        .join("")}</div>
      <div class="nova-sound-card" style="margin-top:8px">${peaks.length ? `<b>高峰</b>：<span class="nova-sound-muted">${peaks.slice(0, 4).map((p) => `周${daysZh[p.day]} ${p.hour}:00（${p.count} 条）`).join("、")}${peaks.length > 4 ? " …" : ""}</span>` : `<span class="nova-sound-muted">尚无沉积：通知经 nova://sound-notify 摄入后成图</span>`}</div>`;
  } else if (curTab === "radio") {
    body = `<div class="nova-sound-card"><b>晨间电台（opt-in）</b> <span class="nova-sound-muted">90s 三段 · 离线 TTS</span>
      <div class="nova-sound-muted">无 TTS 引擎时如实跳过（RADIO_UNAVAILABLE），不假播。</div>
      <div data-act="radio-dry" style="cursor:pointer;text-decoration:underline;margin-top:4px">试生成 90s 脚本（字幕模式）</div></div>
      <div data-radio-out>${radio.script.map((s) => `<div class="nova-sound-card"><b>${s.titleZh} · ${s.sec}s</b><div class="nova-sound-muted">${esc(s.text)}</div></div>`).join("") || `<div class="nova-sound-muted">尚未生成脚本。</div>`}</div>`;
  } else if (curTab === "balance") {
    body = `<div class="nova-sound-card"><b>双耳平衡罗盘</b> <span class="nova-sound-muted">±3dB 温和补偿</span></div>
      <div class="nova-sound-card">当前补偿：<b>L ${balanceComp.dbL >= 0 ? "+" : ""}${balanceComp.dbL}dB / R ${balanceComp.dbR >= 0 ? "+" : ""}${balanceComp.dbR}dB</b>
      <div class="nova-sound-muted">校准：闭眼听六题声像，答偏哪侧；未校准 = 0dB（不猜）。</div>
      <div data-act="balance-demo" style="cursor:pointer;text-decoration:underline;margin-top:4px">演示校准（3 题右偏样本）</div>
      <div data-act="balance-reset" style="cursor:pointer;text-decoration:underline;margin-top:4px">重置为 0dB</div></div>`;
  } else {
    const keys = pianoKeys();
    body = `<div class="nova-sound-card"><b>声纹钢琴</b> <span class="nova-sound-muted">12 半音 · 每键一件系统音</span></div>
      <div class="nova-sound-piano">${keys
        .map((k) => `<div class="nova-sound-key" data-key="${k.semitone}" title="${esc(k.soundId)}">${k.note}</div>`)
        .join("")}</div>
      <div class="nova-sound-card" style="margin-top:8px"><b>声纹历史</b> <span class="nova-sound-muted">最近 3 声</span>
      ${history.length === 0 ? `<div class="nova-sound-muted">暂无。</div>` : history
        .map((h, i) => `<div class="nova-sound-stop" data-act="replay" data-idx="${i}"><span>${esc(h.soundId)}</span><span class="nova-sound-muted">${new Date(h.at).toLocaleTimeString()} · 重播</span></div>`)
        .join("")}</div>`;
  }
  panel.innerHTML = head + body + `</div>`;
}

function h2(h: number): string {
  return String(h).padStart(2, "0");
}

// --- 聚光条 / 颤脉 ---

function showSpotlight(): void {
  if (typeof document === "undefined") return;
  if (!spotEl) {
    spotEl = document.createElement("div");
    spotEl.className = "nova-sound-spot";
    document.body.appendChild(spotEl);
  }
  const { item } = spotlightDequeue(spotlight);
  if (!item) {
    spotEl.remove();
    spotEl = null;
    return;
  }
  const anim = motionOK();
  spotEl.style.opacity = anim ? "0" : "1";
  spotEl.style.transition = anim ? "opacity 240ms ease" : "none";
  spotEl.innerHTML = `<b>${esc(item.title)}</b><span>${esc(item.body)}</span><div class="nova-sound-muted" style="margin-top:6px">聚光上演 · 候场 ${spotlight.queue.length} 条 / 存档 ${spotlight.archive.length} 条</div>`;
  requestAnimationFrame(() => {
    if (spotEl) spotEl.style.opacity = "1";
  });
  spotTimer = window.setTimeout(showSpotlight, SPOTLIGHT_STAGE_MS);
}

function fireTremor(count: number): void {
  if (typeof document === "undefined" || count <= 0) return;
  if (!tremorEl) {
    tremorEl = document.createElement("div");
    tremorEl.className = "nova-sound-tremor";
    document.body.appendChild(tremorEl);
  }
  const pulses = tremorTimeline(tremorPulses(count));
  soundEvent("tremor", { pulses: pulses.length });
  if (!motionOK()) {
    // 降级：静态 1px 微光一次
    tremorEl.dataset.on = "1";
    window.setTimeout(() => tremorEl && (tremorEl.dataset.on = "0"), TREMOR_PULSE_MS);
    return;
  }
  let i = 0;
  const step = (): void => {
    if (!tremorEl) return;
    tremorEl.dataset.on = "1";
    window.setTimeout(() => {
      if (tremorEl) tremorEl.dataset.on = "0";
      i++;
      if (i < pulses.length) window.setTimeout(step, TREMOR_PULSE_MS);
    }, TREMOR_PULSE_MS / 2);
  };
  step();
}

// --- 摄入处理 ---

function onNotify(d: unknown): void {
  const p = d as { id?: string; title?: string; body?: string; at?: number; src?: string } | undefined;
  if (!p || typeof p.id !== "string") return;
  const at = typeof p.at === "number" ? p.at : Date.now();
  // W-153 沉积
  heat = heatDeposit(heat, at);
  heatCount += 1;
  lsSet(`${NS}.heat.v1`, heat);
  lsSet(`${NS}.heat.count.v1`, heatCount);
  if (panel && curTab === "heat") renderPanel();
  // W-162 队列聚光（勿扰期同样入队，不弹幕齐轰）
  if (flagOn("W-162")) {
    spotlight = spotlightEnqueue(spotlight, { ntfId: p.id, title: p.title ?? p.id, body: p.body ?? "", at });
    lsSet(`${NS}.spotlight.v1`, spotlight);
    if (!spotEl && spotlight.queue.length > 0) showSpotlight();
  }
  // W-156 / W-159 勿扰分支
  if (dndOn && flagOn("W-156")) {
    dndNotifs.push({ id: p.id, at });
    const drops = rainDrops(dndNotifs);
    const last = drops[drops.length - 1]!;
    if (last) {
      soundEvent("play", { soundId: "dnd-rain", delayMs: Math.max(0, last.atMs - Date.now()), gain: rainGain(novaNum("W-156", "gain") || 15) });
      soundEvent("rain-drop", { ntfId: p.id, dropIndex: drops.length - 1, total: drops.length });
    }
  }
  if (dndOn && flagOn("W-159")) fireTremor(1);
}

function bindIntakes(): void {
  const on = (name: string, fn: (d: unknown) => void): void => {
    const h = (ev: Event): void => fn((ev as CustomEvent).detail);
    window.addEventListener(`nova://sound-${name}`, h);
    bag.push(() => window.removeEventListener(`nova://sound-${name}`, h));
  };
  on("notify", onNotify);
  on("dnd", (d) => {
    const p = d as { on?: boolean } | undefined;
    dndOn = p?.on === true;
    if (!dndOn) dndNotifs = [];
  });
  on("session", (d) => {
    const p = d as { start?: number; end?: number } | undefined;
    if (typeof p?.start === "number") chime = { ...chime, sessionStart: p.start, sessionEnd: typeof p.end === "number" ? p.end : 0, lastChimeHour: 0 };
    if (typeof p?.end === "number") chime = { ...chime, sessionEnd: p.end };
    lsSet(`${NS}.chime.v1`, chime);
  });
  on("played", (d) => {
    const p = d as { soundId?: string; at?: number } | undefined;
    if (!p || typeof p.soundId !== "string") return;
    // W-161 微史
    history = pushHistory(history, { soundId: p.soundId, at: typeof p.at === "number" ? p.at : Date.now() });
    lsSet(`${NS}.history.v1`, history);
    if (panel && curTab === "piano") renderPanel();
  });
  on("radio-data", (d) => {
    const p = d as { weatherZh?: string; agendaItems?: string[]; quoteZh?: string; hasTts?: boolean } | undefined;
    if (!p) return;
    const segs = radioScript({ weatherZh: p.weatherZh ?? "", agendaItems: p.agendaItems ?? [], quoteZh: p.quoteZh ?? "" });
    const v = radioVerdict(p.hasTts !== false, segs);
    if (v.ok) {
      radio = { script: v.segs, idx: 0, timer: 0 };
      soundEvent("radio-start", { segs: v.segs });
    } else {
      radio = { script: [], idx: 0, timer: 0 };
      soundEvent("radio-skip", { code: v.code, reason: v.reason });
    }
    if (panel && curTab === "radio") renderPanel();
  });
}

// --- 面板交互 ---

function bindPanel(): void {
  if (!panel) return;
  panel.addEventListener("click", (ev) => {
    const t = (ev.target as HTMLElement).closest<HTMLElement>("[data-tab],[data-act],[data-key],[data-stop]");
    if (!t || !panel) return;
    const tab = t.dataset.tab;
    if (tab) {
      curTab = tab as typeof curTab;
      renderPanel();
      return;
    }
    const act = t.dataset.act;
    if (act === "close") togglePanel(false);
    else if (act === "play" || act === "tweak" || t.dataset.stop) {
      const sid = t.dataset.sound ?? t.dataset.stop;
      if (!sid) return;
      if (act === "tweak") {
        const cur = tweaks[sid] ?? { soundId: sid, pitchCents: 0, gainDb: 0 };
        const next = clampTweak({ soundId: sid, pitchCents: cur.pitchCents + 100, gainDb: cur.gainDb + 0.5 });
        tweaks = { ...tweaks, [sid]: next };
        lsSet(`${NS}.tweaks.v1`, tweaks);
        soundEvent("tweaks", { tweaks: Object.values(tweaks) });
        renderPanel();
      } else {
        soundEvent("play", { soundId: sid, gainDb: tweaks[sid]?.gainDb ?? 0, pitchCents: tweaks[sid]?.pitchCents ?? 0 });
      }
    } else if (act === "tour") {
      soundEvent("tour-start", tourTimeline());
    } else if (act === "radio-dry") {
      radio = { script: radioScript({ weatherZh: "今日多云，18 到 26 度，出门带件薄外套。", agendaItems: ["10:00 站会", "14:00 评审"], quoteZh: "慢即是快。" }), idx: 0, timer: 0 };
      renderPanel();
    } else if (act === "balance-demo") {
      const trials: BalanceTrial[] = [
        { said: -1, actual: 0.4 },
        { said: 0, actual: 0 },
        { said: -1, actual: 0.5 },
      ];
      balanceComp = balanceDb(balanceFactor(trials));
      lsSet(`${NS}.balance.v1`, balanceComp);
      soundEvent("balance", balanceComp);
      renderPanel();
    } else if (act === "balance-reset") {
      balanceComp = { dbL: 0, dbR: 0 };
      lsSet(`${NS}.balance.v1`, balanceComp);
      soundEvent("balance", balanceComp);
      renderPanel();
    } else if (act === "replay") {
      const ev = historyLookup(history, Number(t.dataset.idx ?? -1));
      if (ev) soundEvent("play", { soundId: ev.soundId });
    } else if (t.dataset.key != null) {
      const sid = pianoSoundId(Number(t.dataset.key));
      if (sid) {
        soundEvent("play", { soundId: sid });
        t.dataset.hit = "1";
        window.setTimeout(() => (t.dataset.hit = "0"), 160);
      }
    }
  });
}

// --- S0 注册表联动 ---

function onRegistryChange(): void {
  if (typeof document === "undefined") return;
  const anyOn = SOUND_NOVA_FEATURES.some((f) => flagOn(f.id));
  if (!anyOn) togglePanel(false);
}

// ---------------------------------------------------------------------------
// 激活 / 卸载（幂等）
// ---------------------------------------------------------------------------

export function activateSoundNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  loadState();
  ensureStyle();

  const onOpen = (): void => togglePanel();
  window.addEventListener("nova://sound-open", onOpen);
  bag.push(() => window.removeEventListener("nova://sound-open", onOpen));

  // Hub overlay 直达（ai04 协议：nova-museum / nova-heat / nova-piano）
  const onOpenFeature = (ev: Event): void => {
    const f = (ev as CustomEvent).detail?.feature;
    if (f === "nova-museum" && flagOn("W-152")) {
      curTab = "museum";
      togglePanel(true);
    }
    if (f === "nova-heat" && flagOn("W-153")) {
      curTab = "heat";
      togglePanel(true);
    }
    if (f === "nova-piano" && flagOn("W-160")) {
      curTab = "piano";
      togglePanel(true);
    }
  };
  window.addEventListener("ai04:open-feature", onOpenFeature);
  bag.push(() => window.removeEventListener("ai04:open-feature", onOpenFeature));

  bindIntakes();

  // W-158 整点钟声：30s 轻量巡检（会话内跨整点恰响一声）
  const chimeTimer = window.setInterval(() => {
    const now = Date.now();
    if (flagOn("W-158") && chimeDue(chime, now)) {
      chime = chimeMark(chime, now);
      lsSet(`${NS}.chime.v1`, chime);
      soundEvent("play", { soundId: "hour-chime", ms: CHIME_MS });
      soundEvent("chime", { at: now });
    }
  }, 30_000);
  bag.push(() => clearInterval(chimeTimer));

  // W-157 晨昏曲线：激活即对齐一次（自动调节非动效，降级不受影响）
  if (flagOn("W-157")) {
    soundEvent("daypart", daypartPct(daypart, Date.now()));
  }

  void Promise.resolve().then(async () => {
    try {
      const mod = await import("../registry");
      bag.push(mod.subscribeNova(onRegistryChange));
    } catch {
      /* S0 未就绪：开关即时生效退化为下次激活生效 */
    }
  });
}

export function deactivateSoundNova(): void {
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
  panel?.remove();
  panel = null;
  spotEl?.remove();
  spotEl = null;
  tremorEl?.remove();
  tremorEl = null;
  if (spotTimer) {
    clearTimeout(spotTimer);
    spotTimer = 0;
  }
  if (radio.timer) {
    clearTimeout(radio.timer);
    radio.timer = 0;
  }
  radio.script = [];
  dndNotifs = [];
  spotlight = { queue: [], archive: [] };
  document.querySelectorAll(".nova-sound-panel,.nova-sound-spot,.nova-sound-tremor").forEach((n) => n.remove());
}

export function isSoundNovaActive(): boolean {
  return active;
}
