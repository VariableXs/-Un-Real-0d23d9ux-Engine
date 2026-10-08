// AURORA-10000：AI-01~AI-05 批次，勿删。
// params.ts — 领域01 启动与品牌剧场：每项功能（F00001~F00625）都是独立参数档。
// 所有参数由 ID 确定性派生（同 ID 恒同参，跨会话可复现），颜色/时长最终仍须
// 消费 src/design/tokens.css 令牌；此处只产出「档位差异」，不落地硬编码样式。

import { findTheaterItem, type TheaterKind } from "./registry";

/* ---------- 确定性哈希：ID → [0,1) ---------- */
function hash01(id: string, salt: number): number {
  let h = 2166136261 ^ salt;
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return ((h >>> 0) % 100000) / 100000;
}

/* ---------- 通用视觉参数（光弧/呼吸/转场/刻度等渲染层共用） ---------- */
export interface ItemParams {
  /** 主色相（0~360）；与主题 accent 无关的剧场自选档。 */
  hue: number;
  /** 时长缩放（0.6~1.4），最终乘到 tokens --dur-*。 */
  durScale: number;
  /** 强度（0.5~1.5）：透明度/辉光/粒子数缩放。 */
  intensity: number;
  /** 曲线档：0=标准 1=强调 2=弹性。 */
  easing: 0 | 1 | 2;
}

export function itemParams(id: string): ItemParams {
  return {
    hue: Math.floor(hash01(id, 1) * 360),
    durScale: 0.6 + hash01(id, 2) * 0.8,
    intensity: 0.5 + hash01(id, 3),
    easing: (Math.floor(hash01(id, 4) * 3) as 0 | 1 | 2),
  };
}

/* ---------- 族0005 启动色彩科学：色彩管线档（F00101~F00125） ---------- */
export interface ColorPipeline {
  /** 作用在启动画面内容上的 CSS filter（全量走 filter，不碰令牌底色）。 */
  filter: string;
  /** 是否强制黑底（OLED 省电档）。 */
  blackBase: boolean;
}

const COLOR_PIPELINES: Record<string, ColorPipeline> = {
  F00101: { filter: "none", blackBase: false },
  F00102: { filter: "saturate(1.15)", blackBase: false },
  F00103: { filter: "saturate(1.1) contrast(1.06)", blackBase: false },
  F00104: { filter: "saturate(0.96) contrast(1.03)", blackBase: false },
  F00105: { filter: "saturate(1.3) contrast(1.02)", blackBase: false },
  F00106: { filter: "brightness(1.08) contrast(1.12)", blackBase: false },
  F00107: { filter: "contrast(1.1) brightness(1.04)", blackBase: false },
  F00108: { filter: "saturate(1.6) contrast(1.15) brightness(0.92)", blackBase: false },
  F00109: { filter: "saturate(1.35) contrast(1.1)", blackBase: false },
  F00110: { filter: "grayscale(1) contrast(1.15)", blackBase: false },
  F00111: { filter: "sepia(1) hue-rotate(170deg) saturate(2.2)", blackBase: false },
  F00112: { filter: "sepia(0.85) saturate(1.2)", blackBase: false },
  F00113: { filter: "grayscale(0.7) sepia(0.4) hue-rotate(300deg)", blackBase: false },
  F00114: { filter: "saturate(1.25) hue-rotate(-10deg) contrast(1.08)", blackBase: false },
  F00115: { filter: "saturate(1.8) hue-rotate(250deg) contrast(1.15)", blackBase: false },
  F00116: { filter: "grayscale(1) contrast(1.2)", blackBase: false },
  F00117: { filter: "sepia(0.3) saturate(1.3) brightness(1.05)", blackBase: false },
  F00118: { filter: "sepia(0.45) saturate(1.5) brightness(0.98)", blackBase: false },
  F00119: { filter: "saturate(0.7) hue-rotate(200deg) brightness(0.9)", blackBase: false },
  F00120: { filter: "saturate(0.9) hue-rotate(90deg) brightness(0.95)", blackBase: false },
  F00121: { filter: "sepia(0.6) saturate(1.6) brightness(1.02)", blackBase: false },
  F00122: { filter: "saturate(1.2) hue-rotate(320deg) brightness(1.04)", blackBase: false },
  F00123: { filter: "sepia(0.15) brightness(1.06) saturate(0.85)", blackBase: false },
  F00124: { filter: "saturate(0.8) contrast(1.05) brightness(1.03)", blackBase: false },
  F00125: { filter: "none", blackBase: true },
};

export function colorPipeline(id: string): ColorPipeline {
  return COLOR_PIPELINES[id] ?? { filter: "none", blackBase: false };
}

/* ---------- 族0003 启动进度叙事：25 个叙事文案包（F00051~F00075） ---------- */
/** 每包五幕（固件/内核/驱动/服务/桌面），文案与全景图逐包对应。 */
export const NARRATIVE_PACKS: readonly { id: string; stages: readonly [string, string, string, string, string] }[] = [
  { id: "F00051", stages: ["「千门万户曈曈日」——固件自检", "「天工人巧日争新」——内核装载", "「绝知此事要躬行」——驱动就位", "「万紫千红总是春」——服务编排", "「轻舟已过万重山」——桌面启幕"] },
  { id: "F00052", stages: ["观测：恒星核聚变稳定（固件）", "观测：行星核心开始分层（内核）", "观测：大气环流成形（驱动）", "观测：生命分子自组装（服务）", "观测：文明曙光照亮地表（桌面）"] },
  { id: "F00053", stages: ["我是固件，先把地基打好", "我是内核，神经中枢上线", "我是驱动，四肢开始听使唤", "我是服务，后勤团队就位", "我是桌面，欢迎回家"] },
  { id: "F00054", stages: ["远征日志 01：营地搭建完成", "远征日志 02：引擎点火", "远征日志 03：装备逐一核对", "远征日志 04：补给线贯通", "远征日志 05：抵达目的地"] },
  { id: "F00055", stages: ["旅客们请注意，固件站台发车", "列车驶入内核段，请扶稳坐好", "驱动站到站，全部车门开启", "服务枢纽站，换乘通道打开", "终点站桌面到了，欢迎光临"] },
  { id: "F00056", stages: ["固件区晴，自检顺利", "内核区多云转晴，装载正常", "驱动区局部小雨，加载无误", "服务区东风三级，编排完成", "桌面区今日宜开机"] },
  { id: "F00057", stages: ["调频 88.1，固件频道播报", "调频 92.5，内核波段清晰", "调频 96.8，驱动信号良好", "调频 101.3，服务节目单就绪", "调频 106.7，桌面时刻已到"] },
  { id: "F00058", stages: ["展品一：固件，年代久远而可靠", "展品二：内核，系统的中枢神经", "展品三：驱动，硬件的翻译官", "展品四：服务，看不见的管家", "展品五：桌面，您正在使用的界面"] },
  { id: "F00059", stages: ["深度 10 米：入水检查完毕", "深度 200 米：压力舱正常", "深度 1000 米：声呐巡检通过", "深度 4000 米：灯光逐组点亮", "深度 8000 米：抵达海底基地"] },
  { id: "F00060", stages: ["大本营：装备清点完成", "一号营地：氧气补充就绪", "二号营地：绳索固定完毕", "三号营地：冲顶物资分配", "峰顶：登顶成功，桌面就绪"] },
  { id: "F00061", stages: ["曲率引擎一级点火：固件", "曲率引擎二级点火：内核", "曲率引擎三级点火：驱动", "曲率引擎四级点火：服务", "曲率跃迁完成：进入桌面星域"] },
  { id: "F00062", stages: ["实验 01：固件自检……记录正常", "实验 02：内核装载……记录正常", "实验 03：驱动加载……记录正常", "实验 04：服务编排……记录正常", "实验 05：桌面渲染……记录完美"] },
  { id: "F00063", stages: ["索引卡 A-01：固件已归档", "索引卡 A-02：内核已归档", "索引卡 A-03：驱动已归档", "索引卡 A-04：服务已归档", "索引卡 A-05：桌面已上架开放"] },
  { id: "F00064", stages: ["惊蛰：固件破土自检", "立夏：内核拔节生长", "芒种：驱动播种就位", "白露：服务凝实收拢", "冬至：桌面围炉开张"] },
  { id: "F00065", stages: ["方位 N：固件就位", "方位 NE：内核锁定", "方位 E：驱动校准", "方位 SE：服务列队", "方位 S：桌面敞开大门"] },
  { id: "F00066", stages: ["起锚！固件检查完毕", "升帆！内核满舵前进", "瞭望！驱动海况良好", "掌舵！服务航线校正", "靠港！桌面欢迎登岸"] },
  { id: "F00067", stages: ["K0 出发：固件检查通过", "K25 通过：内核运转正常", "K50 通过：驱动轴温良好", "K75 通过：服务调度准点", "K100 到达：桌面站台开启"] },
  { id: "F00068", stages: ["对焦中：固件轮廓清晰", "寻星中：内核锁定目标", "跟踪中：驱动驱动云台", "采集中：服务堆叠曝光", "成像完成：桌面首视图就绪"] },
  { id: "F00069", stages: ["拣字：固件字符入盘", "排字：内核版面成形", "校对：驱动逐行核对", "上墨：服务滚筒转动", "开印：桌面第一页新鲜出炉"] },
  { id: "F00070", stages: ["窑温 200°C：固件素坯就位", "窑温 600°C：内核釉水挂匀", "窑温 1000°C：驱动纹样显影", "窑温 1300°C：服务釉色定格", "开窑：桌面成品出窑"] },
  { id: "F00071", stages: ["一锻：固件去杂提纯", "二锻：内核定骨成型", "三锻：驱动开刃淬火", "四锻：服务装配校直", "成器：桌面寒光初现"] },
  { id: "F00072", stages: ["游丝装配：固件基准校准", "擒纵就位：内核节拍锁定", "上弦完成：驱动动力储蓄", "指针安装：服务轮系啮合", "报时：桌面整点开张"] },
  { id: "F00073", stages: ["生豆入锅：固件预热", "脱水完成：内核转黄", "一爆开始：驱动香气上扬", "二爆边缘：服务风味定型", "出炉冷却：桌面醇香开杯"] },
  { id: "F00074", stages: ["取料：固件琉璃液就绪", "吹泡：内核鼓起骨架", "塑形：驱动修出轮廓", "退火：服务缓慢降温", "成品：桌面透亮登场"] },
  { id: "F00075", stages: ["5km 补给站：固件通过", "15km 补给站：内核配速稳定", "25km 补给站：驱动步频不变", "35km 补给站：服务补给到位", "终点冲线：桌面完赛"] },
];

/** 取叙事行：progress∈[0,1] → 该包当前幕文案；id 非 F00051~F00075 时回退空串。 */
export function narrativeLine(id: string, progress: number): string {
  const pack = NARRATIVE_PACKS.find((p) => p.id === id);
  if (!pack) return "";
  const stage = Math.min(4, Math.max(0, Math.floor(progress * 5)));
  return pack.stages[stage] ?? "";
}

/* ---------- 族0004 开机音景 / 族0023 品牌声音 ID：合成参数档 ---------- */
export interface SoundProfile {
  /** 合成法：tones=振荡器和弦 / noise=噪声声景 / bell=铃体 / pluck=拨弦。 */
  synth: "tones" | "noise" | "bell" | "pluck";
  /** 基频组（Hz）。 */
  freqs: number[];
  /** 时长 ms。 */
  durMs: number;
  /** 噪声色（仅 noise）。 */
  noise: "white" | "pink" | "brown";
  /** 音量包络攻击比例 0~0.5。 */
  attack: number;
}

/** 由 ID 确定性合成一组和声音高（五声阶约束，保证不刺耳）。 */
function pentaFreqs(id: string, base: number, n: number): number[] {
  const steps = [0, 2, 4, 7, 9];
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    const st = steps[Math.floor(hash01(id, 10 + i) * 5)] ?? 0;
    out.push(Math.round(base * Math.pow(2, (st + 12 * Math.floor(hash01(id, 20 + i) * 2)) / 12)));
  }
  return out;
}

export function soundProfile(id: string): SoundProfile {
  const item = findTheaterItem(id);
  const kind: TheaterKind | undefined = undefined;
  void kind;
  // F00100 绝对静音：真无声档。
  if (id === "F00100") return { synth: "tones", freqs: [], durMs: 0, noise: "white", attack: 0 };
  const isId = id.startsWith("F005") && Number(id.slice(1)) >= 551 && Number(id.slice(1)) <= 575;
  const base = isId ? 440 : 220;
  const noiseKind = (["white", "pink", "brown"] as const)[Math.floor(hash01(id, 35) * 3)] ?? "white";
  const profile: SoundProfile = {
    synth: hash01(id, 30) < 0.25 ? "noise" : hash01(id, 31) < 0.5 ? "bell" : hash01(id, 32) < 0.75 ? "pluck" : "tones",
    freqs: pentaFreqs(id, base, 2 + Math.floor(hash01(id, 33) * 3)),
    durMs: 600 + Math.round(hash01(id, 34) * 1800),
    noise: noiseKind,
    attack: 0.05 + hash01(id, 36) * 0.3,
  };
  if (item && item.name.includes("静音")) profile.freqs = [];
  return profile;
}

/* ---------- 族0021 启动配速器：配速档（F00501~F00525） ---------- */
export interface PacingProfile {
  /** 入场编排时长倍率（乘 pacingTimings().enter）。 */
  enterScale: number;
  /** readyHold 倍率。 */
  holdScale: number;
  /** 动画总量保留比例（低端/静默档裁剪）。 */
  animKeep: number;
  /** 音效是否保留。 */
  sound: boolean;
}

const PACING: Record<string, PacingProfile> = {
  F00501: { enterScale: 0, holdScale: 0, animKeep: 0, sound: false },
  F00502: { enterScale: 0.5, holdScale: 0.5, animKeep: 0.5, sound: true },
  F00503: { enterScale: 1, holdScale: 1, animKeep: 1, sound: true },
  F00504: { enterScale: 1.25, holdScale: 1.25, animKeep: 1, sound: true },
  F00505: { enterScale: 1.5, holdScale: 1.5, animKeep: 1, sound: true },
  F00506: { enterScale: 1.5, holdScale: 1.2, animKeep: 1, sound: true },
  F00507: { enterScale: 0.4, holdScale: 0.4, animKeep: 0.3, sound: false },
  F00508: { enterScale: 1.2, holdScale: 1.1, animKeep: 1, sound: true },
  F00509: { enterScale: 1, holdScale: 1, animKeep: 1, sound: true },
  F00510: { enterScale: 2, holdScale: 1.6, animKeep: 1, sound: true },
  F00511: { enterScale: 1.4, holdScale: 1.2, animKeep: 1, sound: true },
  F00512: { enterScale: 0.5, holdScale: 0.5, animKeep: 1, sound: true },
  F00513: { enterScale: 2, holdScale: 2, animKeep: 1, sound: true },
  F00514: { enterScale: 0.7, holdScale: 1.3, animKeep: 1, sound: true },
  F00515: { enterScale: 1.3, holdScale: 0.7, animKeep: 1, sound: true },
  F00516: { enterScale: 1.3, holdScale: 1.3, animKeep: 1, sound: true },
  F00517: { enterScale: 1.2, holdScale: 1.2, animKeep: 1, sound: true },
  F00518: { enterScale: 1.6, holdScale: 1.6, animKeep: 0.8, sound: true },
  F00519: { enterScale: 1.4, holdScale: 1.4, animKeep: 1, sound: true },
  F00520: { enterScale: 1.1, holdScale: 1.1, animKeep: 1, sound: true },
  F00521: { enterScale: 1.1, holdScale: 1, animKeep: 1, sound: true },
  F00522: { enterScale: 0.6, holdScale: 0.6, animKeep: 0.5, sound: false },
  F00523: { enterScale: 0.6, holdScale: 0.6, animKeep: 0.5, sound: false },
  F00524: { enterScale: 0.5, holdScale: 0.5, animKeep: 0.6, sound: false },
  F00525: { enterScale: 1, holdScale: 1, animKeep: 1, sound: true },
};

export function pacingProfile(id: string): PacingProfile {
  return PACING[id] ?? { enterScale: 1, holdScale: 1, animKeep: 1, sound: true };
}

/* ---------- 族0022 无障碍开机：能力开关档（F00526~F00550） ---------- */
export interface A11yBootFlags {
  announce: boolean;
  largeText: boolean;
  highContrast: boolean;
  slowMotion: boolean;
  noFlicker: boolean;
  haptics: boolean;
  singleSwitch: boolean;
  subtitles: boolean;
  plainLanguage: boolean;
  skipAll: boolean;
}

const A11Y_FALSE: A11yBootFlags = {
  announce: false, largeText: false, highContrast: false, slowMotion: false, noFlicker: false,
  haptics: false, singleSwitch: false, subtitles: false, plainLanguage: false, skipAll: false,
};

export function a11yBootFlags(id: string): A11yBootFlags {
  const f = { ...A11Y_FALSE };
  switch (id) {
    case "F00526": f.announce = true; break;
    case "F00527": f.announce = true; break;
    case "F00528": f.largeText = true; f.highContrast = true; break;
    case "F00529": f.subtitles = true; break;
    case "F00530": f.slowMotion = true; break;
    case "F00531": f.noFlicker = true; break;
    case "F00532": f.haptics = true; break;
    case "F00533": f.singleSwitch = true; break;
    case "F00535": f.announce = true; break;
    case "F00536": f.highContrast = true; break;
    case "F00537": f.noFlicker = true; break;
    case "F00538": f.plainLanguage = true; break;
    case "F00539": f.plainLanguage = true; break;
    case "F00541": f.announce = true; break;
    case "F00543": f.subtitles = true; break;
    case "F00546": f.plainLanguage = true; f.slowMotion = true; break;
    case "F00547": f.slowMotion = true; break;
    case "F00549": f.skipAll = true; break;
    default: break;
  }
  return f;
}

/* ---------- 族0024 启动彩蛋层：触发方式档（F00576~F00600） ---------- */
export interface EggSpec {
  trigger: "auto-low" | "date" | "combo" | "milestone" | "manual";
  /** 低频概率（仅 auto-low），其余档忽略。 */
  chance: number;
  /** 总控开关（F00600）。 */
  master: boolean;
}

export function eggSpec(id: string): EggSpec {
  if (id === "F00600") return { trigger: "manual", chance: 0, master: true };
  if (id === "F00591") return { trigger: "combo", chance: 0, master: false };
  if (id === "F00579" || id === "F00598") return { trigger: "milestone", chance: 0, master: false };
  if (["F00580", "F00581", "F00582", "F00583", "F00584", "F00585", "F00586", "F00587", "F00588"].includes(id)) {
    return { trigger: "date", chance: 0, master: false };
  }
  const chance = 0.02 + hash01(id, 40) * 0.03; // 2%~5% 低频
  return { trigger: "auto-low", chance, master: false };
}

/* ---------- 族0025 开机自检报告卡：报告样式档（F00601~F00625） ---------- */
export interface ReportStyle {
  /** 版式：compact 摘要卡 / table 表格 / ticker 行情带 / stamp 印章式 / plain 纯文本。 */
  layout: "compact" | "table" | "ticker" | "stamp" | "plain";
  /** 装饰语（样式名自带的题头）。 */
  masthead: string;
}

const REPORT_STYLES: Record<string, ReportStyle> = {
  F00601: { layout: "compact", masthead: "一页摘要" },
  F00602: { layout: "table", masthead: "系统体检单" },
  F00603: { layout: "stamp", masthead: "年检合格贴" },
  F00604: { layout: "table", masthead: "营养成分表" },
  F00605: { layout: "compact", masthead: "今日系统天气" },
  F00606: { layout: "ticker", masthead: "开机行情" },
  F00607: { layout: "table", masthead: "成绩单" },
  F00608: { layout: "plain", masthead: "航海志" },
  F00609: { layout: "table", masthead: "航前检查单" },
  F00610: { layout: "table", masthead: "入院记录单" },
  F00611: { layout: "compact", masthead: "维修工单" },
  F00612: { layout: "table", masthead: "化验报告" },
  F00613: { layout: "stamp", masthead: "每日一签" },
  F00614: { layout: "stamp", masthead: "牌阵解读" },
  F00615: { layout: "table", masthead: "老黄历 · 宜开机" },
  F00616: { layout: "compact", masthead: "绿皮书" },
  F00617: { layout: "stamp", masthead: "入境章" },
  F00618: { layout: "compact", masthead: "成就解锁" },
  F00619: { layout: "compact", masthead: "奖杯室" },
  F00620: { layout: "compact", masthead: "任务简报" },
  F00621: { layout: "ticker", masthead: "启动物流跟踪" },
  F00622: { layout: "table", masthead: "航班信息" },
  F00623: { layout: "stamp", masthead: "演出票根" },
  F00624: { layout: "stamp", masthead: "集邮册" },
  F00625: { layout: "plain", masthead: "" },
};

export function reportStyle(id: string): ReportStyle {
  return REPORT_STYLES[id] ?? { layout: "compact", masthead: "启动报告" };
}
