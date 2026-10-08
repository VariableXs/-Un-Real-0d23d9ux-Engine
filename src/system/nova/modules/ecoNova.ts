/**
 * NOVA-200 · S11 生态基因路（AI-11）—— 域11 开放生态（W-127…W-138）。
 *
 * 边界（全景 §11）：Q-68 权限说明书管对账显示、W-127 是运行时基因编辑；
 * W-72 插件体检管单插件评分、W-128 是全生态宏观雷达；M-57 webhook 桥管数据外发
 * 通道、W-129 是用户声邮的产品化（知情投递）；M-85 依赖审计管安全审计、W-130 管
 * 版本冲突诊疗；Q-71 营养标签管安装前预估、W-131 管已装后实测；N-36 跨设备接力
 * 管账号体系、W-132 是无网物理驿传；N-28 网关管网关本体、W-133 是限流可视化；
 * Q-79 主题精品店管整件试穿、W-135 管逐 token 移植；N-27 市场管市场本体、W-136
 * 是策展层；V-90 沙盒管沙盒环境、W-137 是逐动作剧本回放；V-85 卸载善后管数据
 * 残留清理、W-138 是服务叙事告别页。
 *
 * 纪律：
 * - 零侵入：不改写任何既有组件内部逻辑；全部为 DOM 叠层 + `nova://eco-*`
 *   自定义事件摄入 + `nova.eco.*` 本地存储（外部接线由 S17 按 wiringHint 补齐）；
 * - 前缀：类名 `nova-eco-`、事件 `nova://eco-*`、localStorage 键 `nova.eco.*`；
 * - 开关：只读消费 S0 注册表（registry.ts novaOn），无注册表时用 manifest
 *   defaultOn 回退（诚实降级，不报错）；
 * - 降级：reduce-motion / safeMode / static 三态下动效归零（雷达展开/驿传帧动画/
 *   列车呼吸），语义与数据保留；非 DOM 环境行为层安全 no-op；
 * - 默认档：W-132 二维码驿传默认关（与 S0 注册表一致），其余默认开。
 *
 * 诚实边界：
 * - W-127 基因关闭的调用得到**明确拒绝**（GeneVerdict.code="GENE_OFF"，
 *   非静默失败）；未知基因如实 UNKNOWN_GENE；
 * - W-128 六轴数据同源 `nova://eco-radar-sample` 真实摄入，无样本如实空态雷达；
 * - W-129 起草全程离线；投递仅用户手动触发（nova://eco-post-dispatch 一次性
 *   移交），拒绝投递不产生任何草稿外副本；模块自身零网络；
 * - W-130 依赖图与真实 package 数据一致（nova://eco-deps 摄入），无冲突隐藏入口；
 * - W-131 评分实测采样 24h 滚动窗口，跑分不外传（本地），报告可导出 JSON；
 * - W-132 校验和（FNV-1a 32bit）逐包验证；≤2MB / 60s 预算可行才发车，失败续传
 *   按 missing 分片；本模块只做**本地生成**与校验，屏幕扫码对传由 S17 呈现；
 * - W-133 车厢数据 nova://eco-train 实时摄入（500ms 刷新），配额规则与网关同源；
 * - W-134 未知权限如实显示原文 + 警示（NOT IN LEXICON 语义），不编造翻译；
 * - W-135 diff 与真实 token 变更一致（nova://eco-theme 摄入），部分采纳持久化
 *   为「我的混成主题」（nova.eco.hybrid.v1）；
 * - W-136 双货架独立排序；体检分过滤联动 W-131 真实数据，无跑分如实沉底；
 * - W-137 剧本为**纯内存态**（含被沙盒拦截的尝试），随沙盒销毁（零落盘），
 *   回放可逐条暂停审阅；
 * - W-138 讣告数据来自 nova://eco-service-stats 本地真实统计；零统计的新装
 *   插件显示简短版讣告。
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

const NS = "nova.eco";
const DAY = 86_400_000;

/** 功能开关：只读消费 S0 注册表。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

/** 派发 `nova://eco-*` 事件（SSR/测试环境安全）。 */
export function ecoEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://eco-${name}`, { detail }));
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

export const ECO_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-127",
    titleZh: "插件基因编辑",
    titleEn: "Plugin Genes",
    descZh: "插件设置页「基因」标签：每项能力基因（网络/文件/通知/快捷键…）逐项开关，插件带伤服役而不必一刀切；调用已关基因得到明确拒绝（GENE_OFF）。",
    defaultOn: true,
    overlay: "nova-genes",
    wiringHint: "插件运行时（N-26 只读接口）调用 ecoGeneGuard(pluginId, gene) 获得裁决；基因清单经 nova://eco-genes 喂入",
    degrade: "静态开关矩阵，语义与拒绝错误完全保留",
  },
  {
    id: "W-128",
    titleZh: "生态雷达图",
    titleEn: "Eco Radar",
    descZh: "生态中心六轴雷达（插件数/主题数/活跃度/更新频率/安全分/社区贡献），本机生态健康一图尽览，月度对比叠加；六轴数据同源各真实模块。",
    defaultOn: true,
    overlay: "nova-radar",
    wiringHint: "各真实模块经 nova://eco-radar-sample {axis,value} 喂入采样",
    degrade: "静态雷达条形展开，无扫描动画；空态如实呈现",
  },
  {
    id: "W-129",
    titleZh: "社区声邮",
    titleEn: "Voice Post",
    descZh: "意见反馈声邮筒：本地离线起草存为待寄包裹，投递前展示完整内容清单，用户手动知情批量投递；默认零网络，反馈也不偷跑。",
    defaultOn: true,
    wiringHint: "投递按钮触发 nova://eco-post-dispatch {drafts}，由 S17/M-57 决定真实通道",
    degrade: "静态声邮清单；离线语义与知情投递流程不变",
  },
  {
    id: "W-130",
    titleZh: "依赖树医生",
    titleEn: "Dep Doctor",
    descZh: "插件依赖冲突以树+红结呈现，附升A/降B/装双版本三种解法与调用面影响预估；依赖图与真实 package 数据一致，无冲突隐藏入口。",
    defaultOn: true,
    wiringHint: "已装插件 package 数据经 nova://eco-deps {nodes} 喂入",
    degrade: "静态冲突树与解法卡；预估逻辑不变",
  },
  {
    id: "W-131",
    titleZh: "本地灯塔",
    titleEn: "Plugin Lighthouse",
    descZh: "已装插件 CPU/内存/启动拖累/事件订阅四维实测评分（0–100）+ 与生态中位数对比条；24h 滚动窗口，跑分不外传，报告可导出。",
    defaultOn: true,
    overlay: "nova-lighthouse",
    wiringHint: "采样经 nova://eco-lh-sample {pluginId,cpuPct,memMb,bootMs,eventsPerMin} 喂入",
    degrade: "静态评分卡与对比条；滚动窗口语义保留",
  },
  {
    id: "W-132",
    titleZh: "二维码驿传",
    titleEn: "QR Relay",
    descZh: "本机配置/主题/布局打包为本地生成的二维码序列跨机对传，全程零网络零云；≤2MB 60s 内传完（纠错分片），校验和验证，失败可续传。opt-in 默认关。",
    defaultOn: false,
    wiringHint: "帧画面呈现由 S17 接二维码渲染层；本模块产出分片计划与校验（relayPlan/packRelay）",
    degrade: "无帧推进动画，直接呈现分片计划与校验结果",
  },
  {
    id: "W-133",
    titleZh: "限流列车",
    titleEn: "Rate Train",
    descZh: "本地 API 网关限流配额以车厢载客呈现：使用率映射载客率，≥85% 黄灯、超载红灯；500ms 实时刷新，配额规则与网关真实规则同源。",
    defaultOn: true,
    wiringHint: "网关（N-28）经 nova://eco-train {cars} 喂入各调用方 {id,used,quota}",
    degrade: "静态车厢条与色标；刷新语义保留",
  },
  {
    id: "W-134",
    titleZh: "权限语言墙",
    titleEn: "Perm Decoder",
    descZh: "插件权限声明白话翻译墙：fs.read~/* →「能读你所有文件」逐条人话 + 低/中/高风险色标；未知权限如实显示原文加警示，不编造翻译。",
    defaultOn: true,
    wiringHint: "安装页权限声明经 nova://eco-perms {pluginId,perms} 喂入",
    degrade: "静态翻译墙与色标",
  },
  {
    id: "W-135",
    titleZh: "主题换器官预览",
    titleEn: "Theme Surgery",
    descZh: "主题应用前逐 token diff（颜色/圆角/字重/间距/字体五器官对照），可逐器官勾选采纳（只换色不换字重）；部分采纳持久化为「我的混成主题」。",
    defaultOn: true,
    wiringHint: "候选主题对经 nova://eco-theme {base,next} 喂入；采纳结果派发 nova://eco-hybrid-apply",
    degrade: "静态 diff 对照与勾选清单",
  },
  {
    id: "W-136",
    titleZh: "市场策展货架",
    titleEn: "Curator Shelf",
    descZh: "官方策展（质量门槛 ≥80 背书）与社区热度（投票排序）双货架并列独立排序；社区货支持按 W-131 体检分过滤，无跑分如实沉底。",
    defaultOn: true,
    wiringHint: "市场条目（含 official/quality/votes/score）经 nova://eco-shelf {items} 喂入",
    degrade: "静态双货架与过滤清单",
  },
  {
    id: "W-137",
    titleZh: "行为剧作 replay",
    titleEn: "Sandbox Replay",
    descZh: "沙盒试用（V-90）全部行为录制为可回放剧本：时间线+逐动作清单（含被拦截的尝试），逐条暂停审阅后再决定装不装；剧本随沙盒销毁零落盘。",
    defaultOn: true,
    wiringHint: "V-90 经 nova://eco-sandbox-start/action/end 三事件喂入录制流",
    degrade: "静态剧本清单与步进回放；销毁语义不变",
  },
  {
    id: "W-138",
    titleZh: "插件讣告",
    titleEn: "Plugin Obituary",
    descZh: "卸载插件的服务叙事告别页：服役时长/完成快捷动作次数/评分星数 + 温情告别文案；数据来自本地真实统计，零统计的新装插件显示简短版。",
    defaultOn: true,
    wiringHint: "服务统计经 nova://eco-service-stats 持续累计；卸载经 nova://eco-uninstall 触发",
    degrade: "静态讣告卡",
  },
];

export const ecoNovaDomain = {
  id: "S11",
  nameZh: "开放生态",
  nameEn: "Open Ecosystem",
  route: "AI-11",
  features: ECO_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-127 插件基因编辑
// ---------------------------------------------------------------------------

export type GeneKey = "net" | "fs" | "notify" | "hotkey" | "clipboard" | "autostart";

export interface GeneDef {
  key: GeneKey;
  zh: string;
  en: string;
}

/** 基因目录（生态规范全集；新增基因须先入目录）。 */
export const GENE_CATALOG: GeneDef[] = [
  { key: "net", zh: "网络", en: "Network" },
  { key: "fs", zh: "文件", en: "Files" },
  { key: "notify", zh: "通知", en: "Notify" },
  { key: "hotkey", zh: "快捷键", en: "Hotkey" },
  { key: "clipboard", zh: "剪贴板", en: "Clipboard" },
  { key: "autostart", zh: "自启", en: "Autostart" },
];

/** 基因默认态（自启默认关，克制）。 */
export const GENE_DEFAULT: Record<GeneKey, boolean> = {
  net: true,
  fs: true,
  notify: true,
  hotkey: true,
  clipboard: true,
  autostart: false,
};

export type GeneOverride = Partial<Record<GeneKey, boolean>>;
/** 用户逐基因覆盖：pluginId → 基因覆盖。 */
export type GeneState = Record<string, GeneOverride>;

export const GENE_UNKNOWN = "UNKNOWN_GENE" as const;
export const GENE_OFF = "GENE_OFF" as const;

export interface GeneVerdict {
  ok: boolean;
  /** OK ｜ GENE_OFF（明确拒绝）｜ UNKNOWN_GENE（未入目录的基因）。 */
  code: typeof GENE_OFF | typeof GENE_UNKNOWN | "OK";
  gene: string;
  pluginId: string;
  message: string;
}

/** 生效基因 = 用户覆盖 → 插件声明 → 目录默认。 */
export function effectiveGene(declared: boolean | undefined, override: boolean | undefined, gene: GeneKey): boolean {
  if (typeof override === "boolean") return override;
  if (typeof declared === "boolean") return declared;
  return GENE_DEFAULT[gene];
}

/**
 * 基因裁决（W-127 验收核心）：已关基因返回**明确拒绝**（GENE_OFF，非静默失败），
 * 未入目录的基因如实 UNKNOWN_GENE。
 */
export function geneVerdict(
  pluginId: string,
  gene: string,
  override: boolean | undefined,
  declared: boolean | undefined,
): GeneVerdict {
  const known = GENE_CATALOG.find((g) => g.key === gene);
  if (!known) {
    return { ok: false, code: GENE_UNKNOWN, gene, pluginId, message: `未知基因 ${gene}（未入生态基因目录，拒绝执行）` };
  }
  if (!effectiveGene(declared, override, known.key)) {
    return {
      ok: false,
      code: GENE_OFF,
      gene,
      pluginId,
      message: `插件 ${pluginId} 的 ${known.zh}基因已被关闭，调用被明确拒绝（GENE_OFF）`,
    };
  }
  return { ok: true, code: "OK", gene, pluginId, message: "OK" };
}

export function setGene(state: GeneState, pluginId: string, gene: GeneKey, on: boolean): GeneState {
  const cur = state[pluginId] ?? {};
  if (cur[gene] === on) return state;
  return { ...state, [pluginId]: { ...cur, [gene]: on } };
}

/** 基因调用守卫（行为层 API；S17/N-26 接线点）。返回裁决并派发事件。 */
export function ecoGeneGuard(pluginId: string, gene: string): GeneVerdict {
  const st = lsGet<GeneState>(`${NS}.genes.v1`, {});
  const declared = geneDeclarations[pluginId]?.[gene as GeneKey];
  const v = geneVerdict(pluginId, gene, st[pluginId]?.[gene as GeneKey], declared);
  ecoEvent("gene-verdict", v);
  return v;
}

/** 插件基因声明表（nova://eco-genes 摄入的内存镜像；N-26 只读接口同源）。 */
const geneDeclarations: Record<string, GeneOverride> = {};

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-128 生态雷达图
// ---------------------------------------------------------------------------

export const RADAR_AXES = ["plugins", "themes", "activity", "updates", "security", "community"] as const;
export type RadarAxis = (typeof RADAR_AXES)[number];

export const RADAR_AXIS_DEF: Record<RadarAxis, { zh: string; en: string; cap: number }> = {
  plugins: { zh: "插件数", en: "Plugins", cap: 40 },
  themes: { zh: "主题数", en: "Themes", cap: 30 },
  activity: { zh: "活跃度", en: "Activity", cap: 200 },
  updates: { zh: "更新频率", en: "Updates", cap: 50 },
  security: { zh: "安全分", en: "Security", cap: 100 },
  community: { zh: "社区贡献", en: "Community", cap: 100 },
};

export interface RadarSample {
  axis: RadarAxis;
  value: number;
  ts: number;
}

/** 轴值归一（cap 满分；越界钳制 0..100）。 */
export function normalizeAxis(axis: RadarAxis, value: number): number {
  return clamp(Math.round((value / RADAR_AXIS_DEF[axis].cap) * 100), 0, 100);
}

/** 月键（YYYY-MM，月度对比分组用）。 */
export function monthKeyOf(t: number): string {
  const d = new Date(t);
  return `${d.getFullYear()}-${`${d.getMonth() + 1}`.padStart(2, "0")}`;
}

/** 六轴雷达读数：指定月（默认当月）各轴样本均值归一；无样本轴返回 null（空态）。 */
export function radarRead(samples: RadarSample[], now: number, monthOffset = 0): Array<{ axis: RadarAxis; value: number | null }> {
  const target = new Date(now);
  target.setDate(1);
  target.setMonth(target.getMonth() + monthOffset);
  const key = monthKeyOf(target.getTime());
  return RADAR_AXES.map((axis) => {
    const vs = samples.filter((s) => s.axis === axis && monthKeyOf(s.ts) === key).map((s) => s.value);
    if (vs.length === 0) return { axis, value: null };
    const avg = vs.reduce((a, b) => a + b, 0) / vs.length;
    return { axis, value: normalizeAxis(axis, avg) };
  });
}

/** 月度对比差值（本月 − 上月；null 轴跳过）。 */
export function monthDelta(
  cur: Array<{ axis: RadarAxis; value: number | null }>,
  prev: Array<{ axis: RadarAxis; value: number | null }>,
): Array<{ axis: RadarAxis; delta: number | null }> {
  const p = new Map(prev.map((x) => [x.axis, x.value]));
  return cur.map((c) => {
    const pv = p.get(c.axis);
    return { axis: c.axis, delta: c.value == null || pv == null ? null : c.value - pv };
  });
}

/** 雷达空态（六轴全无样本 → 如实空态）。 */
export function radarEmpty(samples: RadarSample[]): boolean {
  return samples.length === 0;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-129 社区声邮
// ---------------------------------------------------------------------------

export interface PostDraft {
  id: string;
  kind: "text" | "voice";
  text: string;
  chars: number;
  createdAt: number;
}

export const POST_MAX = 20;
export const DRAFT_MAX = 30;
export const POST_TEXT_MAX = 2000;

let postSeq = 0;

/** 本地起草（全程离线；语音稿为转写文本，kind=voice 保留来源语义）。 */
export function draftPost(kind: "text" | "voice", text: string, now: number): PostDraft | null {
  const t = text.trim();
  if (!t) return null;
  postSeq += 1;
  const chars = [...t].length;
  return {
    id: `vp-${now.toString(36)}-${postSeq}`,
    kind,
    text: t.slice(0, POST_TEXT_MAX),
    chars: Math.min(chars, POST_TEXT_MAX),
    createdAt: now,
  };
}

/** 追加草稿（封顶 DRAFT_MAX，超出淘汰最旧）。 */
export function pushDraft(drafts: PostDraft[], d: PostDraft): PostDraft[] {
  const out = [...drafts, d];
  return out.length > DRAFT_MAX ? out.slice(out.length - DRAFT_MAX) : out;
}

/** 投递前完整内容清单（知情投递：每封的类型/字数/首行预览）。 */
export function postManifest(drafts: PostDraft[]): string[] {
  return drafts.map((d, i) => {
    const head = [...d.text].slice(0, 24).join("");
    return `${i + 1}. [${d.kind === "voice" ? "语音" : "文字"}] ${d.chars} 字 ｜ ${head}${d.chars > 24 ? "…" : ""}`;
  });
}

/**
 * 知情投递封签：confirm=true 才移交（一次性、至多 POST_MAX 封）；
 * confirm=false 拒绝投递 —— 草稿原样保留，不产生任何草稿外副本。
 */
export function sealDispatch(
  drafts: PostDraft[],
  confirm: boolean,
): { posted: PostDraft[]; kept: PostDraft[] } {
  if (!confirm) return { posted: [], kept: drafts };
  return { posted: drafts.slice(0, POST_MAX), kept: drafts.slice(POST_MAX) };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-130 依赖树医生
// ---------------------------------------------------------------------------

export type Semver = [number, number, number];

export function semverParse(v: string): Semver {
  const m = /^(\d+)\.(\d+)\.(\d+)/.exec(v.trim());
  if (!m) return [0, 0, 0];
  return [Number(m[1]), Number(m[2]), Number(m[3])];
}

export function semverCmp(a: Semver, b: Semver): number {
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return (a[i] ?? 0) - (b[i] ?? 0);
  }
  return 0;
}

/** 区间语义：min（含）… maxExcl（不含）；null = 无上界。 */
export interface SemRange {
  min: Semver;
  maxExcl: Semver | null;
}

/** 支持 `*`、精确 `1.2.3`、脱字符 `^1.2.3`、`>=1.0.0`；不可解析如实按 `*`。 */
export function parseRange(range: string): SemRange {
  const r = range.trim();
  if (!r || r === "*" || r === "latest") return { min: [0, 0, 0], maxExcl: null };
  if (r.startsWith("^")) {
    const min = semverParse(r.slice(1));
    return { min, maxExcl: [(min[0] ?? 0) + 1, 0, 0] };
  }
  if (r.startsWith(">=")) return { min: semverParse(r.slice(2)), maxExcl: null };
  const exact = semverParse(r);
  if (exact[2] !== 0 || /\d+\.\d+\.\d+/.test(r)) {
    return { min: exact, maxExcl: [exact[0] ?? 0, exact[1] ?? 0, (exact[2] ?? 0) + 1] };
  }
  return { min: [0, 0, 0], maxExcl: null };
}

export function semverInRange(v: string, range: string): boolean {
  const s = semverParse(v);
  const r = parseRange(range);
  if (semverCmp(s, r.min) < 0) return false;
  return r.maxExcl == null || semverCmp(s, r.maxExcl) < 0;
}

export interface DepNode {
  /** 插件 id。 */
  id: string;
  version: string;
  /** 依赖声明：depId → 版本区间。 */
  deps: Record<string, string>;
}

export interface DepConflict {
  dep: string;
  demandors: Array<{ id: string; range: string }>;
}

/**
 * 冲突检测：同一依赖 ≥2 个声明方，且**不存在**同时满足全部区间的版本
 * （候选集 = 各方 min 版本，确定性判定）。
 */
export function findConflicts(nodes: DepNode[]): DepConflict[] {
  const byDep = new Map<string, Array<{ id: string; range: string }>>();
  for (const n of nodes) {
    for (const [dep, range] of Object.entries(n.deps)) {
      const arr = byDep.get(dep) ?? [];
      arr.push({ id: n.id, range });
      byDep.set(dep, arr);
    }
  }
  const out: DepConflict[] = [];
  for (const [dep, demandors] of byDep) {
    if (demandors.length < 2) continue;
    const candidates = demandors.map((d) => parseRange(d.range).min);
    const satisfiable = candidates.some((c) =>
      demandors.every((d) => semverInRange(`${c[0]}.${c[1]}.${c[2]}`, d.range)),
    );
    if (!satisfiable) out.push({ dep, demandors });
  }
  return out.sort((a, b) => a.dep.localeCompare(b.dep));
}

export type RemedyKind = "upgrade" | "downgrade" | "dual";

export interface Remedy {
  kind: RemedyKind;
  zh: string;
  /** 建议落点版本。 */
  target: string;
  /** 影响面：区间不容纳 target 的声明方。 */
  affected: string[];
  /** 成本（影响面 + 双版本固有成本）。 */
  cost: number;
}

function fmtVer(v: Semver): string {
  return `${v[0]}.${v[1]}.${v[2]}`;
}

/** 三解法预估（验收：升A/降B/装双版本，各自影响面基于调用面区间分析）。 */
export function threeRemedies(c: DepConflict): Remedy[] {
  const mins = c.demandors.map((d) => parseRange(d.range).min);
  const sorted = [...mins].sort(semverCmp);
  const hi = sorted[sorted.length - 1] ?? ([0, 0, 0] as Semver);
  const lo = sorted[0] ?? ([0, 0, 0] as Semver);
  const affectedFor = (target: Semver): string[] =>
    c.demandors.filter((d) => !semverInRange(fmtVer(target), d.range)).map((d) => d.id);
  const upAffected = affectedFor(hi);
  const downAffected = affectedFor(lo);
  return [
    { kind: "upgrade", zh: `统一升级至 ${fmtVer(hi)}`, target: fmtVer(hi), affected: upAffected, cost: upAffected.length },
    { kind: "downgrade", zh: `统一下调至 ${fmtVer(lo)}`, target: fmtVer(lo), affected: downAffected, cost: downAffected.length },
    { kind: "dual", zh: "安装双版本并存", target: `${fmtVer(lo)} + ${fmtVer(hi)}`, affected: [], cost: 3 },
  ];
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-131 本地灯塔
// ---------------------------------------------------------------------------

export interface LHSample {
  pluginId: string;
  ts: number;
  /** CPU 平均占用 %。 */
  cpuPct: number;
  /** 常驻内存 MB。 */
  memMb: number;
  /** 启动拖累 ms。 */
  bootMs: number;
  /** 事件订阅触发 次/分。 */
  eventsPerMin: number;
}

export const LH_WINDOW_MS = 24 * 3_600_000;

/** 24h 滚动窗口淘汰。 */
export function evictLH(samples: LHSample[], now: number): LHSample[] {
  return samples.filter((s) => now - s.ts <= LH_WINDOW_MS);
}

export interface LHScore {
  cpu: number;
  mem: number;
  boot: number;
  events: number;
  overall: number;
}

/** 四维评分 0–100（越高越省）：成本越低分越高；无样本返回 null（诚实）。 */
export function scoreLighthouse(samples: LHSample[]): Record<string, LHScore> | null {
  if (samples.length === 0) return null;
  const byPlugin = new Map<string, LHSample[]>();
  for (const s of samples) {
    const arr = byPlugin.get(s.pluginId) ?? [];
    arr.push(s);
    byPlugin.set(s.pluginId, arr);
  }
  const out: Record<string, LHScore> = {};
  for (const [pid, arr] of byPlugin) {
    const avg = (f: (s: LHSample) => number): number => arr.reduce((a, s) => a + f(s), 0) / arr.length;
    const cpu = clamp(Math.round(100 - avg((s) => s.cpuPct) * 4), 0, 100);
    const mem = clamp(Math.round(100 - avg((s) => s.memMb) / 2), 0, 100);
    const boot = clamp(Math.round(100 - avg((s) => s.bootMs) / 10), 0, 100);
    const events = clamp(Math.round(100 - avg((s) => s.eventsPerMin) * 2), 0, 100);
    out[pid] = { cpu, mem, boot, events, overall: Math.round((cpu + mem + boot + events) / 4) };
  }
  return out;
}

export function median(nums: number[]): number {
  if (nums.length === 0) return 0;
  const s = [...nums].sort((a, b) => a - b);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 === 1 ? (s[mid] ?? 0) : Math.round(((s[mid - 1] ?? 0) + (s[mid] ?? 0)) / 2);
}

export interface LHCompare {
  pid: string;
  dim: keyof Omit<LHScore, "overall">;
  mine: number;
  med: number;
  delta: number;
}

/**
 * 与生态中位数对比条：指定插件（缺省为字典序首个）四维逐项对比
 * 全体已评插件的中位数（delta = 我 − 中位）。
 */
export function medianCompare(scores: Record<string, LHScore>, pluginId?: string): LHCompare[] {
  const all = Object.entries(scores);
  if (all.length === 0) return [];
  const pid = pluginId ?? [...all.map(([k]) => k)].sort()[0]!;
  const mine = scores[pid];
  if (!mine) return [];
  const dims: Array<keyof Omit<LHScore, "overall">> = ["cpu", "mem", "boot", "events"];
  return dims.map((dim) => {
    const med = median(all.map(([, s]) => s[dim]));
    return { pid, dim, mine: mine[dim], med, delta: mine[dim] - med };
  });
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-132 二维码驿传
// ---------------------------------------------------------------------------

export const RELAY_MAX_BYTES = 2 * 1024 * 1024;
export const RELAY_BUDGET_MS = 60_000;
export const RELAY_CHUNK_BYTES = 1200;

/** FNV-1a 32bit 校验和（8 位十六进制）。 */
export function fnv1a(bytes: Uint8Array): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < bytes.length; i++) {
    h ^= bytes[i] ?? 0;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, "0");
}

export interface RelayChunk {
  index: number;
  count: number;
  body: Uint8Array;
}

export interface RelayPack {
  checksum: string;
  count: number;
  chunks: RelayChunk[];
}

/** 分片打包（纠错分片；全包校验和随首帧元数据走）。 */
export function packRelay(data: Uint8Array): RelayPack {
  const count = Math.max(1, Math.ceil(data.length / RELAY_CHUNK_BYTES));
  const chunks: RelayChunk[] = [];
  for (let i = 0; i < count; i++) {
    chunks.push({ index: i, count, body: data.subarray(i * RELAY_CHUNK_BYTES, Math.min((i + 1) * RELAY_CHUNK_BYTES, data.length)) });
  }
  return { checksum: fnv1a(data), count, chunks };
}

export interface RelayPlan {
  ok: boolean;
  bytes: number;
  count: number;
  /** 每帧预算 ms（60s 均摊）。 */
  perChunkMs: number;
  reason: string;
}

/** 发车预检：≤2MB 且 60s 内按分片数均摊可传完。 */
export function relayPlan(bytes: number): RelayPlan {
  if (bytes > RELAY_MAX_BYTES) {
    return { ok: false, bytes, count: 0, perChunkMs: 0, reason: `OVER BUDGET（${bytes} > ${RELAY_MAX_BYTES} 字节）` };
  }
  const count = Math.max(1, Math.ceil(bytes / RELAY_CHUNK_BYTES));
  const perChunkMs = Math.floor(RELAY_BUDGET_MS / count);
  if (perChunkMs < 1) {
    return { ok: false, bytes, count, perChunkMs, reason: "TIME BUDGET EXHAUSTED（60s 内无法传完）" };
  }
  return { ok: true, bytes, count, perChunkMs, reason: "OK" };
}

export interface RelayReassemble {
  ok: boolean;
  data: Uint8Array | null;
  /** 缺失分片索引（续传依据）。 */
  missing: number[];
  checksumOk: boolean | null;
}

/** 收端重组：分片齐 → 拼接 → 校验和验证；失败给出 missing 续传清单。 */
export function reassemble(pack: RelayPack, parts: RelayChunk[]): RelayReassemble {
  const have = new Map(parts.map((p) => [p.index, p]));
  const missing: number[] = [];
  for (let i = 0; i < pack.count; i++) {
    const c = have.get(i);
    if (!c || c.body.length === 0) missing.push(i);
  }
  if (missing.length > 0) return { ok: false, data: null, missing, checksumOk: null };
  const total = parts.reduce((a, p) => a + p.body.length, 0);
  const data = new Uint8Array(total);
  let off = 0;
  for (let i = 0; i < pack.count; i++) {
    const p = have.get(i)!;
    data.set(p.body, off);
    off += p.body.length;
  }
  const ok = fnv1a(data) === pack.checksum;
  return { ok, data: ok ? data : null, missing: [], checksumOk: ok };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-133 限流列车
// ---------------------------------------------------------------------------

export interface TrainCar {
  /** 调用方（插件）id。 */
  id: string;
  /** 已用配额。 */
  used: number;
  /** 配额上限。 */
  quota: number;
}

export type CarStatus = "idle" | "ok" | "warm" | "overload";

export const CAR_WARM_PCT = 85;
export const TRAIN_REFRESH_MS = 500;

export function carLoadPct(c: TrainCar): number {
  if (c.quota <= 0) return 0;
  return (c.used / c.quota) * 100;
}

/** 车厢状态：空载 / 正常 / ≥85% 黄灯 / 超载红灯。 */
export function carStatus(c: TrainCar): CarStatus {
  if (c.used <= 0 || c.quota <= 0) return "idle";
  const pct = carLoadPct(c);
  if (pct > 100) return "overload";
  if (pct >= CAR_WARM_PCT) return "warm";
  return "ok";
}

export interface TrainReport {
  cars: Array<TrainCar & { pct: number; status: CarStatus }>;
  overloaded: number;
  warm: number;
}

/** 列车时刻表：载客率降序；红灯/黄灯计数。 */
export function trainReport(cars: TrainCar[]): TrainReport {
  const rows = cars
    .map((c) => ({ ...c, pct: Math.round(carLoadPct(c) * 10) / 10, status: carStatus(c) }))
    .sort((a, b) => b.pct - a.pct || a.id.localeCompare(b.id));
  return {
    cars: rows,
    overloaded: rows.filter((r) => r.status === "overload").length,
    warm: rows.filter((r) => r.status === "warm").length,
  };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-134 权限语言墙
// ---------------------------------------------------------------------------

export type RiskLevel = "low" | "mid" | "high";

export interface PermLexEntry {
  zh: string;
  en: string;
  risk: RiskLevel;
}

/** 权限动词翻译库（生态规范全集；新增动词先入库）。 */
export const PERM_LEXICON: Record<string, PermLexEntry> = {
  "fs.read": { zh: "能读取你的文件", en: "Reads your files", risk: "mid" },
  "fs.write": { zh: "能写入与修改你的文件", en: "Writes your files", risk: "mid" },
  "fs.delete": { zh: "能删除你的文件", en: "Deletes your files", risk: "high" },
  "net.send": { zh: "能向外部发送网络请求", en: "Sends network requests", risk: "mid" },
  "net.listen": { zh: "能在本机监听网络端口", en: "Listens on local ports", risk: "mid" },
  notify: { zh: "能给你发系统通知", en: "Sends notifications", risk: "low" },
  hotkey: { zh: "能注册全局快捷键", en: "Registers global hotkeys", risk: "low" },
  "clipboard.read": { zh: "能读取你的剪贴板内容", en: "Reads your clipboard", risk: "high" },
  "clipboard.write": { zh: "能写入剪贴板", en: "Writes your clipboard", risk: "low" },
  autostart: { zh: "能随系统自动启动", en: "Starts with the system", risk: "mid" },
  "process.list": { zh: "能看到正在运行的应用清单", en: "Lists running apps", risk: "mid" },
  "shell.exec": { zh: "能执行任意系统命令", en: "Executes system commands", risk: "high" },
  "media.mic": { zh: "能使用你的麦克风", en: "Uses your microphone", risk: "high" },
  "media.cam": { zh: "能使用你的摄像头", en: "Uses your camera", risk: "high" },
  location: { zh: "能获取你的地理位置", en: "Gets your location", risk: "high" },
};

export interface PermDecode {
  raw: string;
  plainZh: string;
  plainEn: string;
  risk: RiskLevel | "unknown";
  /** 未知权限警示（如实显示原文）。 */
  warn: boolean;
}

/** 单条解码：精确匹配 → 通配剥壳（`fs.read~/*`）→ 前缀最长匹配 → 未知警示。 */
export function decodePerm(raw: string): PermDecode {
  const key = raw.trim();
  const exact = PERM_LEXICON[key];
  if (exact) return { raw, plainZh: exact.zh, plainEn: exact.en, risk: exact.risk, warn: false };
  const shell = key.replace(/~\/\*$/, "").replace(/\/\*$/, "");
  if (shell !== key) {
    const hit = PERM_LEXICON[shell];
    if (hit) {
      const scope = hit.zh.replace(/^能/, "能按其声明范围");
      return { raw, plainZh: scope, plainEn: `${hit.en} (scoped)`, risk: hit.risk, warn: false };
    }
  }
  let best: { k: string; e: PermLexEntry } | null = null;
  for (const [k, e] of Object.entries(PERM_LEXICON)) {
    if (key.startsWith(`${k}.`) && (!best || k.length > best.k.length)) best = { k, e };
  }
  if (best) return { raw, plainZh: best.e.zh, plainEn: best.e.en, risk: best.e.risk, warn: false };
  return { raw, plainZh: `未知权限（NOT IN LEXICON）：${key}`, plainEn: `Unknown permission: ${key}`, risk: "unknown", warn: true };
}

export function decodeWall(perms: string[]): PermDecode[] {
  return perms.map(decodePerm);
}

/** 风险色标变量名（nova.css 消费）。 */
export function riskColor(risk: RiskLevel | "unknown"): string {
  return `var(--nova-eco-risk-${risk})`;
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-135 主题换器官预览
// ---------------------------------------------------------------------------

export type OrganKind = "color" | "radius" | "weight" | "spacing" | "font" | "other";

export const ORGAN_ZH: Record<OrganKind, string> = {
  color: "颜色",
  radius: "圆角",
  weight: "字重",
  spacing: "间距",
  font: "字体",
  other: "其他",
};

/** token 键 → 器官归类。 */
export function organOf(tokenKey: string): OrganKind {
  const k = tokenKey.toLowerCase();
  if (/(^|[^a-z])(color|bg|fg|accent|border)/.test(k)) return "color";
  if (k.includes("radius")) return "radius";
  if (k.includes("weight")) return "weight";
  if (/(gap|space|margin|padding)/.test(k)) return "spacing";
  if (k.includes("font") && !k.includes("weight")) return "font";
  return "other";
}

export interface TokenDiffEntry {
  key: string;
  organ: OrganKind;
  from: string;
  to: string;
}

/** 逐 token diff（只列变更项，键名字典序）。 */
export function tokenDiff(base: Record<string, string>, next: Record<string, string>): TokenDiffEntry[] {
  const keys = new Set([...Object.keys(base), ...Object.keys(next)]);
  const out: TokenDiffEntry[] = [];
  for (const k of [...keys].sort()) {
    const b = base[k];
    const n = next[k];
    if (b !== n) out.push({ key: k, organ: organOf(k), from: b ?? "∅", to: n ?? "∅" });
  }
  return out;
}

/** 器官级勾选移植：只替换选中 token，其余保持本色。 */
export function applySelected(
  base: Record<string, string>,
  next: Record<string, string>,
  selected: string[],
): Record<string, string> {
  const out = { ...base };
  const sel = new Set(selected);
  for (const k of Object.keys(next)) {
    if (sel.has(k) && next[k] !== base[k]) out[k] = next[k]!;
  }
  return out;
}

export const HYBRID_NAME = "我的混成主题";

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-136 市场策展货架
// ---------------------------------------------------------------------------

export interface ShelfItem {
  id: string;
  name: string;
  official: boolean;
  votes: number;
  /** 官方质量门槛分（官方货架排序依据）。 */
  quality: number;
  /** W-131 本地灯塔体检分（本地实测；未装为 null）。 */
  score?: number | null;
}

export const SHELF_QUALITY_GATE = 80;

export interface CuratedShelf {
  official: ShelfItem[];
  community: ShelfItem[];
}

/** 双货架策展：官方按质量门槛分降序（≥80 入架）；社区按投票降序（质量破平）。 */
export function curateShelf(items: ShelfItem[]): CuratedShelf {
  const official = items
    .filter((i) => i.official && i.quality >= SHELF_QUALITY_GATE)
    .sort((a, b) => b.quality - a.quality || a.name.localeCompare(b.name));
  const community = items
    .filter((i) => !i.official)
    .sort((a, b) => b.votes - a.votes || b.quality - a.quality || a.name.localeCompare(b.name));
  return { official, community };
}

/** 体检分过滤（真实联动 W-131：无跑分如实沉底/滤除）。 */
export function filterByScore(items: ShelfItem[], min: number): ShelfItem[] {
  if (min <= 0) return [...items];
  return items.filter((i) => typeof i.score === "number" && i.score >= min);
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-137 行为剧作 replay
// ---------------------------------------------------------------------------

export type ScriptKind = "net" | "fs" | "notify" | "clipboard" | "ui" | "blocked";

export interface ScriptAction {
  t: number;
  kind: ScriptKind;
  detail: string;
  /** 被沙盒拦截的尝试（录制完整，含拦截）。 */
  blocked?: boolean;
}

export interface SandboxScript {
  sandboxId: string;
  startedAt: number;
  actions: ScriptAction[];
}

export const SCRIPT_CAP = 500;

export function scriptOf(sandboxId: string, startedAt: number): SandboxScript {
  return { sandboxId, startedAt, actions: [] };
}

/** 录制一条动作（封顶 SCRIPT_CAP，滚动淘汰最旧；纯内存，零落盘）。 */
export function recordAction(s: SandboxScript, a: ScriptAction): SandboxScript {
  const actions = [...s.actions, a];
  return { ...s, actions: actions.length > SCRIPT_CAP ? actions.slice(actions.length - SCRIPT_CAP) : actions };
}

export interface ScriptSummary {
  total: number;
  blocked: number;
  kinds: Partial<Record<ScriptKind, number>>;
}

export function scriptSummary(s: SandboxScript): ScriptSummary {
  const kinds: Partial<Record<ScriptKind, number>> = {};
  let blocked = 0;
  for (const a of s.actions) {
    kinds[a.kind] = (kinds[a.kind] ?? 0) + 1;
    if (a.blocked) blocked += 1;
  }
  return { total: s.actions.length, blocked, kinds };
}

export interface ReplayStep {
  index: number;
  cur: ScriptAction | null;
  prev: ScriptAction | null;
  next: ScriptAction | null;
  done: boolean;
}

/** 逐条暂停审阅（index 向下取整、负值如实钳 0；越界如实 done）。 */
export function replayAt(s: SandboxScript, index: number): ReplayStep {
  const i = Math.max(0, Math.floor(index));
  const cur = s.actions[i] ?? null;
  return { index: i, cur, prev: s.actions[i - 1] ?? null, next: s.actions[i + 1] ?? null, done: cur == null };
}

// ---------------------------------------------------------------------------
// 纯逻辑 —— W-138 插件讣告
// ---------------------------------------------------------------------------

export interface PluginServiceStats {
  pluginId: string;
  installedAt: number;
  /** 卸载时刻（0 = 仍在役）。 */
  uninstalledAt: number;
  /** 快捷动作完成次数（本地真实统计）。 */
  runs: number;
  /** 评分 0..5（未评分为 null）。 */
  rating: number | null;
}

export const OBIT_DAY_MS = DAY;

export function servedDays(s: PluginServiceStats, now: number): number {
  const end = s.uninstalledAt > 0 ? s.uninstalledAt : now;
  return Math.max(0, Math.floor((end - s.installedAt) / OBIT_DAY_MS));
}

export function ratingText(rating: number | null): string {
  if (rating == null) return "UNRATED";
  const n = clamp(Math.round(rating), 0, 5);
  return `${"★".repeat(n)}${"☆".repeat(5 - n)} (${n}/5)`;
}

/** 告别文案（评分/次数确定性选段；零统计简短版）。 */
export function farewellFor(runs: number, rating: number | null): string {
  if (runs <= 0) return "它安静地来，安静地走，未及留下足迹。";
  if ((rating ?? 0) >= 4.5 && runs >= 100) return "它是最可靠的老伙计：上百次出手，从不失手。一路顺风。";
  if (runs >= 10) return "它一次次接住你的快捷指令，此刻功成身退。谢谢它。";
  return "它只陪你走了短短一程，但每一件事都认真做完了。再会。";
}

export interface Obit {
  pluginId: string;
  brief: boolean;
  servedDays: number;
  runs: number;
  ratingText: string;
  farewell: string;
}

/** 讣告（验收：数据来自本地真实统计；零统计 → 简短版）。 */
export function obituaryOf(s: PluginServiceStats, now: number): Obit {
  const brief = s.runs <= 0;
  return {
    pluginId: s.pluginId,
    brief,
    servedDays: servedDays(s, now),
    runs: s.runs,
    ratingText: ratingText(s.rating),
    farewell: farewellFor(s.runs, s.rating),
  };
}

// ---------------------------------------------------------------------------
// 行为层：状态 / 事件摄入 / 面板
// ---------------------------------------------------------------------------

let active = false;
let bag: Array<() => void> = [];

const STYLE_ID = "nova-eco-style";
const LAYER_ID = "nova-eco-layer";
const PANEL_ID = "nova-eco-panel";

const STYLE_TEXT = `
#${LAYER_ID} { position: fixed; inset: 0; z-index: 960; pointer-events: none; }
#${PANEL_ID} {
  position: absolute; right: 18px; bottom: 56px; width: min(430px, 92vw);
  max-height: min(74vh, 680px); overflow: auto; pointer-events: auto;
  background: var(--bg-raised); border: 1px solid var(--stroke);
  border-radius: var(--r-card, 12px); box-shadow: var(--elev-3, 0 12px 40px rgba(0,0,0,.35));
  color: var(--text-primary); font-size: 12px; line-height: 1.55;
  backdrop-filter: blur(18px) saturate(1.15);
}
#${PANEL_ID} * { box-sizing: border-box; }
.nova-eco-head { display: flex; align-items: center; gap: 8px; padding: 10px 12px 6px; }
.nova-eco-title { font: 600 11px/1.2 var(--font-mono, Consolas, monospace); letter-spacing: .14em; text-transform: uppercase; color: var(--text-secondary); flex: 1; }
.nova-eco-x { appearance: none; border: 1px solid var(--stroke); background: var(--bg-surface); color: var(--text-primary); border-radius: 6px; width: 22px; height: 22px; cursor: pointer; font-size: 12px; }
.nova-eco-tabs { display: flex; flex-wrap: wrap; gap: 4px; padding: 0 12px 8px; border-bottom: 1px solid var(--stroke); }
.nova-eco-tab { appearance: none; border: 1px solid transparent; background: transparent; color: var(--text-secondary); border-radius: 6px; padding: 3px 8px; font-size: 11px; cursor: pointer; }
.nova-eco-tab[aria-selected="true"] { border-color: var(--stroke); background: var(--bg-surface); color: var(--text-primary); }
.nova-eco-body { padding: 10px 12px 14px; display: grid; gap: 8px; }
.nova-eco-h { font: 600 11px/1.3 var(--font-mono, Consolas, monospace); letter-spacing: .1em; text-transform: uppercase; color: var(--text-secondary); margin: 2px 0; }
.nova-eco-row { display: flex; align-items: center; gap: 8px; justify-content: space-between; }
.nova-eco-bar { position: relative; flex: 1; height: 8px; border-radius: 4px; background: var(--bg-surface); border: 1px solid var(--stroke); overflow: hidden; }
.nova-eco-fill { position: absolute; inset: 0 auto 0 0; background: var(--accent, #6ea8fe); border-radius: 4px; }
.nova-eco-fill[data-warm="true"] { background: var(--warn, #d9a13c); }
.nova-eco-fill[data-over="true"] { background: var(--danger, #d95c4a); }
.nova-eco-pill { font: 600 10px/1 var(--font-mono, Consolas, monospace); letter-spacing: .08em; padding: 2px 6px; border-radius: 999px; border: 1px solid var(--stroke); }
.nova-eco-pill[data-risk="low"] { color: var(--text-primary); }
.nova-eco-pill[data-risk="mid"] { color: var(--warn, #d9a13c); border-color: var(--warn, #d9a13c); }
.nova-eco-pill[data-risk="high"] { color: var(--danger, #d95c4a); border-color: var(--danger, #d95c4a); }
.nova-eco-pill[data-risk="unknown"] { color: var(--text-secondary); border-style: dashed; }
.nova-eco-card { border: 1px solid var(--stroke); border-radius: 8px; padding: 8px 10px; display: grid; gap: 6px; background: var(--bg-surface); }
.nova-eco-muted { color: var(--text-secondary); }
.nova-eco-btn { appearance: none; border: 1px solid var(--stroke); background: var(--bg-surface); color: var(--text-primary); border-radius: var(--r-control, 8px); padding: 4px 10px; font-size: 12px; cursor: pointer; }
.nova-eco-btn:disabled { opacity: .45; cursor: default; }
.nova-eco-sw { appearance: none; width: 30px; height: 16px; border-radius: 999px; border: 1px solid var(--stroke); background: var(--bg-surface); position: relative; cursor: pointer; }
.nova-eco-sw::after { content: ""; position: absolute; top: 1px; left: 1px; width: 12px; height: 12px; border-radius: 50%; background: var(--text-secondary); transition: transform .15s ease; }
.nova-eco-sw[aria-checked="true"]::after { transform: translateX(14px); background: var(--accent, #6ea8fe); }
.nova-eco-ta { width: 100%; min-height: 64px; resize: vertical; border: 1px solid var(--stroke); border-radius: 8px; background: var(--bg-surface); color: var(--text-primary); font: inherit; padding: 6px 8px; }
.nova-eco-kv { display: grid; grid-template-columns: auto 1fr; gap: 2px 10px; }
.nova-eco-mono { font-family: var(--font-mono, Consolas, monospace); font-size: 11px; }
`;

type TabId =
  | "genes"
  | "radar"
  | "post"
  | "deps"
  | "lighthouse"
  | "relay"
  | "train"
  | "perms"
  | "surgery"
  | "shelf"
  | "replay"
  | "obituary";

const TABS: Array<{ id: TabId; zh: string }> = [
  { id: "genes", zh: "基因" },
  { id: "radar", zh: "雷达" },
  { id: "post", zh: "声邮" },
  { id: "deps", zh: "依赖" },
  { id: "lighthouse", zh: "灯塔" },
  { id: "relay", zh: "驿传" },
  { id: "train", zh: "列车" },
  { id: "perms", zh: "权限" },
  { id: "surgery", zh: "换器" },
  { id: "shelf", zh: "货架" },
  { id: "replay", zh: "剧作" },
  { id: "obituary", zh: "讣告" },
];

let panel: HTMLElement | null = null;
let curTab: TabId = "genes";
let trainTimer = 0;

// --- 行为层状态（摄入镜像 + 持久化） ---

interface ThemePair {
  base: Record<string, string>;
  next: Record<string, string>;
}

interface PermBag {
  pluginId: string;
  perms: string[];
}

let radarSamples: RadarSample[] = [];
let drafts: PostDraft[] = [];
let depNodes: DepNode[] = [];
let lhSamples: LHSample[] = [];
let relayPack: RelayPack | null = null;
let relayParts: RelayChunk[] = [];
let relayPos = 0;
let trainCars: TrainCar[] = [];
let permBag: PermBag | null = null;
let themePair: ThemePair | null = null;
let surgerySel = new Set<string>();
let shelfItems: ShelfItem[] = [];
let shelfScoreMin = 0;
let liveScript: SandboxScript | null = null;
let replayIdx = 0;
let serviceStats: Record<string, PluginServiceStats> = {};

function loadState(): void {
  radarSamples = lsGet<RadarSample[]>(`${NS}.radar.v1`, []);
  drafts = lsGet<PostDraft[]>(`${NS}.post.v1`, []);
  depNodes = lsGet<DepNode[]>(`${NS}.deps.v1`, []);
  lhSamples = evictLH(lsGet<LHSample[]>(`${NS}.lh.v1`, []), Date.now());
  trainCars = lsGet<TrainCar[]>(`${NS}.train.v1`, []);
  serviceStats = lsGet<Record<string, PluginServiceStats>>(`${NS}.service.v1`, {});
}

// --- 各域渲染（每 tab 一段，全部真实数据 + 诚实空态） ---

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function barHtml(pct: number, label: string, value: string): string {
  const w = clamp(Math.round(pct), 0, 100);
  return `<div class="nova-eco-row"><span style="width:64px" class="nova-eco-muted">${esc(label)}</span><div class="nova-eco-bar"><div class="nova-eco-fill" style="width:${w}%"></div></div><span class="nova-eco-mono" style="width:44px;text-align:right">${esc(value)}</span></div>`;
}

function renderGenes(body: HTMLElement): void {
  const st = lsGet<GeneState>(`${NS}.genes.v1`, {});
  const ids = Object.keys(geneDeclarations).length > 0 ? Object.keys(geneDeclarations) : Object.keys(st);
  if (ids.length === 0) {
    body.innerHTML = `<div class="nova-eco-muted">暂无插件基因声明（等待 nova://eco-genes 摄入）。</div>`;
    return;
  }
  const rows = ids
    .sort()
    .map((pid) => {
      const decl = geneDeclarations[pid] ?? {};
      const cells = GENE_CATALOG.map((g) => {
        const on = effectiveGene(decl[g.key], st[pid]?.[g.key], g.key);
        return `<span class="nova-eco-row" style="gap:4px"><span class="nova-eco-muted">${esc(g.zh)}</span><button class="nova-eco-sw" role="switch" aria-checked="${on}" data-gene-pid="${esc(pid)}" data-gene-key="${g.key}" aria-label="${esc(pid)} ${esc(g.zh)}"></button></span>`;
      }).join("");
      return `<div class="nova-eco-card"><div class="nova-eco-mono">${esc(pid)}</div><div style="display:grid;grid-template-columns:repeat(3,1fr);gap:4px">${cells}</div></div>`;
    })
    .join("");
  body.innerHTML = `<div class="nova-eco-h">能力基因矩阵（关基因 → 调用明确拒绝 GENE_OFF）</div>${rows}`;
  body.querySelectorAll<HTMLButtonElement>("[data-gene-pid]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const pid = btn.dataset.genePid ?? "";
      const key = (btn.dataset.geneKey ?? "net") as GeneKey;
      const on = btn.getAttribute("aria-checked") !== "true";
      const next = setGene(lsGet<GeneState>(`${NS}.genes.v1`, {}), pid, key, on);
      lsSet(`${NS}.genes.v1`, next);
      ecoEvent("gene-change", { pluginId: pid, gene: key, on });
      renderPanel();
    });
  });
}

function renderRadar(body: HTMLElement): void {
  if (radarEmpty(radarSamples)) {
    body.innerHTML = `<div class="nova-eco-muted">空态雷达：六轴暂无真实采样（等待 nova://eco-radar-sample）。</div>`;
    return;
  }
  const now = Date.now();
  const cur = radarRead(radarSamples, now, 0);
  const prev = radarRead(radarSamples, now, -1);
  const deltas = monthDelta(cur, prev);
  const rows = cur
    .map((c) => {
      const d = deltas.find((x) => x.axis === c.axis)?.delta ?? null;
      const dv = d == null ? "—" : `${d > 0 ? "+" : ""}${d}`;
      return barHtml(c.value ?? 0, RADAR_AXIS_DEF[c.axis].zh, c.value == null ? "∅" : `${c.value} (${dv})`);
    })
    .join("");
  body.innerHTML = `<div class="nova-eco-h">六轴生态健康（括号为月度对比）</div>${rows}`;
}

function renderPost(body: HTMLElement): void {
  const manifest = postManifest(drafts);
  const list = manifest.length > 0 ? `<div class="nova-eco-card"><div class="nova-eco-h">待寄包裹（${drafts.length} 封，封顶 ${POST_MAX}）</div><div class="nova-eco-mono">${manifest.map(esc).join("<br>")}</div></div>` : "";
  body.innerHTML = `
    <div class="nova-eco-h">声邮筒（起草全程离线；投递需手动知情确认）</div>
    <textarea class="nova-eco-ta" id="nova-eco-post-ta" maxlength="${POST_TEXT_MAX}" placeholder="写给社区的话…（本地保存，零网络）"></textarea>
    <div class="nova-eco-row">
      <button class="nova-eco-btn" id="nova-eco-post-add">存为待寄</button>
      <button class="nova-eco-btn" id="nova-eco-post-send" ${drafts.length === 0 ? "disabled" : ""}>知情投递（${Math.min(drafts.length, POST_MAX)} 封）</button>
    </div>
    ${list}`;
  body.querySelector<HTMLButtonElement>("#nova-eco-post-add")?.addEventListener("click", () => {
    const ta = body.querySelector<HTMLTextAreaElement>("#nova-eco-post-ta");
    const text = ta?.value ?? "";
    const d = draftPost("text", text, Date.now());
    if (d) {
      drafts = pushDraft(drafts, d);
      lsSet(`${NS}.post.v1`, drafts);
    }
    renderPanel();
  });
  body.querySelector<HTMLButtonElement>("#nova-eco-post-send")?.addEventListener("click", () => {
    const { posted, kept } = sealDispatch(drafts, true);
    if (posted.length > 0) {
      ecoEvent("post-dispatch", { drafts: posted });
      drafts = kept;
      lsSet(`${NS}.post.v1`, drafts);
    }
    renderPanel();
  });
}

function renderDeps(body: HTMLElement): void {
  const conflicts = findConflicts(depNodes);
  if (depNodes.length === 0) {
    body.innerHTML = `<div class="nova-eco-muted">依赖图空态（等待 nova://eco-deps {nodes} 摄入真实 package 数据）。</div>`;
    return;
  }
  if (conflicts.length === 0) {
    body.innerHTML = `<div class="nova-eco-muted">依赖树健康：${depNodes.length} 个插件、无版本冲突（入口按验收隐藏）。</div>`;
    return;
  }
  const trees = conflicts
    .map((c) => {
      const remedies = threeRemedies(c)
        .map(
          (r) =>
            `<div class="nova-eco-row"><span>${esc(r.zh)}</span><span class="nova-eco-muted nova-eco-mono">影响 ${r.affected.length} 方${r.affected.length ? `（${esc(r.affected.join(", "))}）` : ""} · 成本 ${r.cost}</span></div>`,
        )
        .join("");
      const demandors = c.demandors.map((d) => `<div class="nova-eco-mono">├ ${esc(d.id)} → ${esc(c.dep)}@${esc(d.range)}</div>`).join("");
      return `<div class="nova-eco-card"><div class="nova-eco-h">冲突：${esc(c.dep)}</div>${demandors}<div class="nova-eco-h" style="color:var(--danger,#d95c4a)">红结 · 三解法预估</div>${remedies}</div>`;
    })
    .join("");
  body.innerHTML = `<div class="nova-eco-h">依赖树医生（${conflicts.length} 处冲突）</div>${trees}`;
}

function renderLighthouse(body: HTMLElement): void {
  const scores = scoreLighthouse(lhSamples);
  if (!scores) {
    body.innerHTML = `<div class="nova-eco-muted">灯塔空态：24h 窗口内无实测采样（等待 nova://eco-lh-sample）。</div>`;
    return;
  }
  const pids = Object.keys(scores).sort();
  const rows = pids
    .map((pid) => {
      const s = scores[pid]!;
      return `<div class="nova-eco-card"><div class="nova-eco-row"><span class="nova-eco-mono">${esc(pid)}</span><span class="nova-eco-pill">总评 ${s.overall}</span></div>${barHtml(s.cpu, "CPU", `${s.cpu}`)}${barHtml(s.mem, "内存", `${s.mem}`)}${barHtml(s.boot, "启动", `${s.boot}`)}${barHtml(s.events, "事件", `${s.events}`)}</div>`;
    })
    .join("");
  const cmp = medianCompare(scores)
    .map((c) => barHtml(c.mine, c.dim, `${c.delta >= 0 ? "+" : ""}${c.delta}`))
    .join("");
  body.innerHTML = `<div class="nova-eco-h">四维实测（24h 滚动 · 本地不外传）</div>${rows}<div class="nova-eco-h">对生态中位数</div>${cmp}<div class="nova-eco-row"><button class="nova-eco-btn" id="nova-eco-lh-export">导出报告 JSON</button></div>`;
  body.querySelector<HTMLButtonElement>("#nova-eco-lh-export")?.addEventListener("click", () => {
    try {
      const blob = new Blob([JSON.stringify({ at: Date.now(), scores, medians: medianCompare(scores) }, null, 2)], {
        type: "application/json",
      });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `nova-eco-lighthouse-${Date.now()}.json`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 4000);
    } catch {
      /* 导出不可用：如实静默（评分卡仍在） */
    }
  });
}

function renderRelay(body: HTMLElement): void {
  if (!flagOn("W-132")) {
    body.innerHTML = `<div class="nova-eco-muted">二维码驿传默认关（opt-in）：在 Hub 打开 W-132 后发车。</div>`;
    return;
  }
  const payload = new TextEncoder().encode(
    JSON.stringify({ genes: lsGet(`${NS}.genes.v1`, {}), posts: drafts, hybrid: lsGet(`${NS}.hybrid.v1`, null), at: Date.now() }),
  );
  const plan = relayPlan(payload.length);
  const packed = packRelay(payload);
  const have = relayParts.length;
  const stepBtn =
    plan.ok && relayPack
      ? `<button class="nova-eco-btn" id="nova-eco-relay-step" ${relayPos >= packed.count ? "disabled" : ""}>逐帧发车（${have}/${packed.count}）</button>`
      : "";
  body.innerHTML = `
    <div class="nova-eco-h">驿传预检（零网络零云 · FNV-1a 校验和）</div>
    <div class="nova-eco-kv nova-eco-mono"><span>payload</span><span>${payload.length} B</span><span>分片</span><span>${plan.count} × ${RELAY_CHUNK_BYTES} B</span><span>帧预算</span><span>${plan.perChunkMs} ms/帧（60s）</span><span>校验和</span><span>${packed.checksum}</span></div>
    <div class="nova-eco-row"><span class="nova-eco-pill" data-risk="${plan.ok ? "low" : "high"}">${plan.ok ? "READY" : plan.reason}</span>${stepBtn}</div>
    ${relayPack ? `<div class="nova-eco-progress nova-eco-muted">已发出 ${relayPos} 帧${relayPos >= packed.count ? " · 全部帧已出，等待收端扫描重组" : ""}</div>` : ""}`;
  body.querySelector<HTMLButtonElement>("#nova-eco-relay-step")?.addEventListener("click", () => {
    if (!relayPack) return;
    const burst = Math.max(1, Math.floor(relayPack.count / 20));
    relayPos = Math.min(relayPack.count, relayPos + burst);
    relayParts = relayPack.chunks.slice(0, relayPos);
    if (relayPos >= relayPack.count) {
      const r = reassemble(relayPack, relayParts);
      ecoEvent("relay-done", { ok: r.ok, checksum: relayPack.checksum });
    }
    renderPanel();
  });
  if (plan.ok && !relayPack) {
    relayPack = packed;
    relayParts = [];
    relayPos = 0;
  }
  if (!plan.ok) {
    relayPack = null;
    relayParts = [];
    relayPos = 0;
  }
}

function renderTrain(body: HTMLElement): void {
  if (trainCars.length === 0) {
    body.innerHTML = `<div class="nova-eco-muted">列车空态：网关（N-28）尚未喂入 nova://eco-train {cars}。</div>`;
    return;
  }
  const rep = trainReport(trainCars);
  const rows = rep.cars
    .map((c) => {
      const label = c.status === "overload" ? "超载" : c.status === "warm" ? "黄灯" : c.status === "ok" ? "正常" : "空载";
      return `<div class="nova-eco-row"><span class="nova-eco-mono" style="width:96px;overflow:hidden;text-overflow:ellipsis">${esc(c.id)}</span><div class="nova-eco-bar"><div class="nova-eco-fill" data-over="${c.status === "overload"}" data-warm="${c.status === "warm"}" style="width:${clamp(Math.round(c.pct), 0, 100)}%"></div></div><span class="nova-eco-pill" data-risk="${c.status === "overload" ? "high" : c.status === "warm" ? "mid" : "low"}">${label} ${c.pct}%</span></div>`;
    })
    .join("");
  body.innerHTML = `<div class="nova-eco-h">车厢时刻表（500ms 刷新 · 红灯 ${rep.overloaded} · 黄灯 ${rep.warm}）</div>${rows}`;
}

function renderPerms(body: HTMLElement): void {
  if (!permBag || permBag.perms.length === 0) {
    body.innerHTML = `<div class="nova-eco-muted">权限墙空态（等待 nova://eco-perms {pluginId,perms}）。</div>`;
    return;
  }
  const rows = decodeWall(permBag.perms)
    .map(
      (d) =>
        `<div class="nova-eco-row"><span class="nova-eco-mono" style="width:118px;overflow:hidden;text-overflow:ellipsis">${esc(d.raw)}</span><span style="flex:1">${esc(d.plainZh)}</span><span class="nova-eco-pill" data-risk="${d.risk}">${d.risk === "unknown" ? "未知" : d.risk === "high" ? "高" : d.risk === "mid" ? "中" : "低"}</span></div>`,
    )
    .join("");
  body.innerHTML = `<div class="nova-eco-h">权限白话翻译墙 · ${esc(permBag.pluginId)}</div>${rows}`;
}

function renderSurgery(body: HTMLElement): void {
  if (!themePair) {
    body.innerHTML = `<div class="nova-eco-muted">换器官空态（等待 nova://eco-theme {base,next} 喂入主题对）。</div>`;
    return;
  }
  const diff = tokenDiff(themePair.base, themePair.next);
  if (diff.length === 0) {
    body.innerHTML = `<div class="nova-eco-muted">两主题 token 完全一致，无需手术。</div>`;
    return;
  }
  const rows = diff
    .map(
      (d) =>
        `<label class="nova-eco-row"><span style="flex:1"><input type="checkbox" data-tok="${esc(d.key)}" ${surgerySel.has(d.key) ? "checked" : ""}/> <span class="nova-eco-pill" data-risk="low">${ORGAN_ZH[d.organ]}</span> <span class="nova-eco-mono">${esc(d.key)}</span></span><span class="nova-eco-mono nova-eco-muted">${esc(d.from)} → ${esc(d.to)}</span></label>`,
    )
    .join("");
  body.innerHTML = `<div class="nova-eco-h">逐 token diff（${diff.length} 项变更 · 勾选即移植）</div>${rows}<div class="nova-eco-row"><button class="nova-eco-btn" id="nova-eco-surg-apply">移植所选 → ${HYBRID_NAME}</button></div>`;
  body.querySelectorAll<HTMLInputElement>("[data-tok]").forEach((cb) => {
    cb.addEventListener("change", () => {
      if (cb.checked) surgerySel.add(cb.dataset.tok ?? "");
      else surgerySel.delete(cb.dataset.tok ?? "");
    });
  });
  body.querySelector<HTMLButtonElement>("#nova-eco-surg-apply")?.addEventListener("click", () => {
    if (!themePair) return;
    const merged = applySelected(themePair.base, themePair.next, [...surgerySel]);
    lsSet(`${NS}.hybrid.v1`, { name: HYBRID_NAME, tokens: merged });
    ecoEvent("hybrid-apply", { name: HYBRID_NAME, tokens: merged });
    renderPanel();
  });
}

function renderShelf(body: HTMLElement): void {
  if (shelfItems.length === 0) {
    body.innerHTML = `<div class="nova-eco-muted">货架空态（等待 nova://eco-shelf {items}）。</div>`;
    return;
  }
  const { official, community } = curateShelf(shelfItems);
  const commFiltered = filterByScore(community, shelfScoreMin);
  const cell = (i: ShelfItem): string =>
    `<div class="nova-eco-row"><span>${esc(i.name)}</span><span class="nova-eco-mono nova-eco-muted">${i.official ? `Q${i.quality}` : `▲${i.votes}`}${typeof i.score === "number" ? ` · 灯塔 ${i.score}` : ""}</span></div>`;
  const offHtml = official.length > 0 ? official.map(cell).join("") : `<div class="nova-eco-muted">官方货架暂无过门槛（≥${SHELF_QUALITY_GATE}）条目。</div>`;
  const comHtml = commFiltered.length > 0 ? commFiltered.map(cell).join("") : `<div class="nova-eco-muted">无符合体检分 ≥ ${shelfScoreMin} 的社区条目（无跑分如实滤除）。</div>`;
  body.innerHTML = `
    <div class="nova-eco-h">官方策展（质量门槛 ${SHELF_QUALITY_GATE}）</div><div class="nova-eco-card">${offHtml}</div>
    <div class="nova-eco-h">社区热度（可按 W-131 体检分过滤）</div>
    <div class="nova-eco-row"><input type="range" min="0" max="100" step="5" value="${shelfScoreMin}" id="nova-eco-shelf-min" style="flex:1"/><span class="nova-eco-mono">≥ ${shelfScoreMin}</span></div>
    <div class="nova-eco-card">${comHtml}</div>`;
  body.querySelector<HTMLInputElement>("#nova-eco-shelf-min")?.addEventListener("input", (ev) => {
    shelfScoreMin = Number((ev.target as HTMLInputElement).value) || 0;
    renderPanel();
  });
}

function renderReplay(body: HTMLElement): void {
  if (!liveScript) {
    body.innerHTML = `<div class="nova-eco-muted">剧作空态：沙盒（V-90）未开演。开演经 nova://eco-sandbox-start，动作经 nova://eco-sandbox-action 喂入（含被拦截的尝试）。</div>`;
    return;
  }
  const sum = scriptSummary(liveScript);
  const step = replayAt(liveScript, replayIdx);
  const curText = step.cur ? `#${step.index + 1} [${step.cur.kind}${step.cur.blocked ? "·拦截" : ""}] ${esc(step.cur.detail)}` : "—— 全剧终 ——";
  body.innerHTML = `
    <div class="nova-eco-h">剧本 ${esc(liveScript.sandboxId)} · ${sum.total} 动作（拦截 ${sum.blocked} · 封顶 ${SCRIPT_CAP} · 纯内存）</div>
    <div class="nova-eco-card"><div class="nova-eco-mono">${curText}</div></div>
    <div class="nova-eco-row">
      <button class="nova-eco-btn" id="nova-eco-rep-prev" ${replayIdx <= 0 ? "disabled" : ""}>◀ 上一条</button>
      <button class="nova-eco-btn" id="nova-eco-rep-next" ${step.done ? "disabled" : ""}>下一条 ▶</button>
      <button class="nova-eco-btn" id="nova-eco-rep-end">销毁剧本（随沙盒）</button>
    </div>
    <div class="nova-eco-mono nova-eco-muted">kinds: ${Object.entries(sum.kinds).map(([k, v]) => `${k}×${v}`).join(" · ") || "—"}</div>`;
  body.querySelector<HTMLButtonElement>("#nova-eco-rep-prev")?.addEventListener("click", () => {
    replayIdx = Math.max(0, replayIdx - 1);
    renderPanel();
  });
  body.querySelector<HTMLButtonElement>("#nova-eco-rep-next")?.addEventListener("click", () => {
    if (liveScript) replayIdx = Math.min(liveScript.actions.length, replayIdx + 1);
    renderPanel();
  });
  body.querySelector<HTMLButtonElement>("#nova-eco-rep-end")?.addEventListener("click", () => destroyScript());
}

function renderObituary(body: HTMLElement): void {
  const ids = Object.keys(serviceStats);
  if (ids.length === 0) {
    body.innerHTML = `<div class="nova-eco-muted">讣告空态：本地服务统计为空（等待 nova://eco-service-stats 摄入）。</div>`;
    return;
  }
  const now = Date.now();
  const rows = ids
    .sort()
    .map((pid) => {
      const o = obituaryOf(serviceStats[pid]!, now);
      return `<div class="nova-eco-card"><div class="nova-eco-row"><span class="nova-eco-mono">${esc(pid)}</span><span class="nova-eco-pill" data-risk="${o.brief ? "unknown" : "low"}">${o.brief ? "简短版" : "在册"}</span></div><div>服役 ${o.servedDays} 天 · 快捷动作 ${o.runs} 次 · ${esc(o.ratingText)}</div><div class="nova-eco-muted">${esc(o.farewell)}</div></div>`;
    })
    .join("");
  body.innerHTML = `<div class="nova-eco-h">插件讣告（数据来自本地真实统计）</div>${rows}`;
}

const RENDERERS: Record<TabId, (b: HTMLElement) => void> = {
  genes: renderGenes,
  radar: renderRadar,
  post: renderPost,
  deps: renderDeps,
  lighthouse: renderLighthouse,
  relay: renderRelay,
  train: renderTrain,
  perms: renderPerms,
  surgery: renderSurgery,
  shelf: renderShelf,
  replay: renderReplay,
  obituary: renderObituary,
};

function renderPanel(): void {
  if (!panel) return;
  const tabs = panel.querySelector<HTMLElement>(".nova-eco-tabs");
  const body = panel.querySelector<HTMLElement>(".nova-eco-body");
  if (!tabs || !body) return;
  tabs.querySelectorAll<HTMLButtonElement>(".nova-eco-tab").forEach((t) => {
    t.setAttribute("aria-selected", String(t.dataset.tab === curTab));
  });
  RENDERERS[curTab](body);
}

function buildPanel(): HTMLElement {
  const el = document.createElement("div");
  el.id = PANEL_ID;
  el.setAttribute("role", "dialog");
  el.setAttribute("aria-label", "开放生态");
  el.innerHTML = `
    <div class="nova-eco-head"><span class="nova-eco-title">Open Ecosystem · S11</span><button class="nova-eco-x" id="nova-eco-close" aria-label="关闭">✕</button></div>
    <div class="nova-eco-tabs" role="tablist">${TABS.map((t) => `<button class="nova-eco-tab" role="tab" data-tab="${t.id}">${t.zh}</button>`).join("")}</div>
    <div class="nova-eco-body"></div>`;
  el.querySelector<HTMLButtonElement>("#nova-eco-close")?.addEventListener("click", () => togglePanel(false));
  el.querySelectorAll<HTMLButtonElement>(".nova-eco-tab").forEach((t) => {
    t.addEventListener("click", () => {
      curTab = (t.dataset.tab ?? "genes") as TabId;
      renderPanel();
    });
  });
  return el;
}

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
  if (!document.getElementById(LAYER_ID)) {
    const layer = document.createElement("div");
    layer.id = LAYER_ID;
    layer.setAttribute("aria-hidden", "false");
    document.body.appendChild(layer);
    bag.push(() => document.getElementById(LAYER_ID)?.remove());
  }
}

export function isPanelOpen(): boolean {
  return panel != null;
}

/** 面板开关（幂等；open=false 强制收起）。 */
export function togglePanel(open?: boolean): void {
  if (typeof document === "undefined") return;
  const want = open ?? panel == null;
  if (want && !panel) {
    ensureLayer();
    panel = buildPanel();
    document.getElementById(LAYER_ID)?.appendChild(panel);
  }
  if (!want && panel) {
    panel.remove();
    panel = null;
  }
  if (trainTimer === 0 && want && flagOn("W-133")) {
    trainTimer = window.setInterval(() => {
      if (panel && curTab === "train") renderPanel();
    }, TRAIN_REFRESH_MS);
    bag.push(() => {
      window.clearInterval(trainTimer);
      trainTimer = 0;
    });
  }
  if (panel) renderPanel();
}

// --- 沙盒剧作（W-137 纯内存；随沙盒销毁） ---

function destroyScript(): void {
  const had = liveScript != null;
  liveScript = null;
  replayIdx = 0;
  if (had) ecoEvent("script-destroyed", {});
  renderPanel();
}

// --- 事件摄入（S17 喂数入口） ---

function bindIntakes(): void {
  const on = (name: string, fn: (d: unknown) => void): void => {
    const h = (e: Event): void => fn((e as CustomEvent).detail);
    window.addEventListener(`nova://eco-${name}`, h);
    bag.push(() => window.removeEventListener(`nova://eco-${name}`, h));
  };

  on("genes", (d) => {
    const p = d as { pluginId?: string; genes?: GeneOverride } | undefined;
    if (typeof p?.pluginId === "string" && p.genes && typeof p.genes === "object") {
      geneDeclarations[p.pluginId] = { ...geneDeclarations[p.pluginId], ...p.genes };
      renderPanel();
    }
  });
  on("radar-sample", (d) => {
    const p = d as { axis?: string; value?: number } | undefined;
    const axis = p?.axis as RadarAxis | undefined;
    if (axis && RADAR_AXES.includes(axis) && typeof p?.value === "number" && Number.isFinite(p.value)) {
      radarSamples = [...radarSamples.slice(-499), { axis, value: p.value, ts: Date.now() }];
      lsSet(`${NS}.radar.v1`, radarSamples);
      renderPanel();
    }
  });
  on("deps", (d) => {
    const nodes = (d as { nodes?: DepNode[] } | undefined)?.nodes;
    if (Array.isArray(nodes)) {
      depNodes = nodes;
      lsSet(`${NS}.deps.v1`, depNodes);
      renderPanel();
    }
  });
  on("lh-sample", (d) => {
    const p = d as Partial<LHSample> | undefined;
    if (
      typeof p?.pluginId === "string" &&
      [p.cpuPct, p.memMb, p.bootMs, p.eventsPerMin].every((v) => typeof v === "number" && Number.isFinite(v))
    ) {
      lhSamples = evictLH(
        [...lhSamples, { pluginId: p.pluginId, ts: Date.now(), cpuPct: p.cpuPct!, memMb: p.memMb!, bootMs: p.bootMs!, eventsPerMin: p.eventsPerMin! }],
        Date.now(),
      );
      lsSet(`${NS}.lh.v1`, lhSamples.slice(-600));
      renderPanel();
    }
  });
  on("train", (d) => {
    const cars = (d as { cars?: TrainCar[] } | undefined)?.cars;
    if (Array.isArray(cars)) {
      trainCars = cars;
      lsSet(`${NS}.train.v1`, trainCars);
      renderPanel();
    }
  });
  on("perms", (d) => {
    const p = d as { pluginId?: string; perms?: string[] } | undefined;
    if (typeof p?.pluginId === "string" && Array.isArray(p.perms)) {
      permBag = { pluginId: p.pluginId, perms: p.perms };
      renderPanel();
    }
  });
  on("theme", (d) => {
    const p = d as { base?: Record<string, string>; next?: Record<string, string> } | undefined;
    if (p?.base && p?.next && typeof p.base === "object" && typeof p.next === "object") {
      themePair = { base: p.base, next: p.next };
      surgerySel = new Set(tokenDiff(p.base, p.next).map((x) => x.key));
      renderPanel();
    }
  });
  on("shelf", (d) => {
    const items = (d as { items?: ShelfItem[] } | undefined)?.items;
    if (Array.isArray(items)) {
      shelfItems = items;
      renderPanel();
    }
  });
  on("sandbox-start", (d) => {
    const p = d as { sandboxId?: string } | undefined;
    if (typeof p?.sandboxId === "string") {
      liveScript = scriptOf(p.sandboxId, Date.now());
      replayIdx = 0;
      renderPanel();
    }
  });
  on("sandbox-action", (d) => {
    const a = (d as { action?: ScriptAction } | undefined)?.action;
    if (liveScript && a && typeof a.t === "number" && typeof a.kind === "string" && typeof a.detail === "string") {
      liveScript = recordAction(liveScript, a);
      renderPanel();
    }
  });
  on("sandbox-end", () => {
    // 剧本随沙盒销毁（验收：零落盘，事件知会）
    destroyScript();
  });
  on("service-stats", (d) => {
    const s = (d as { stats?: PluginServiceStats } | undefined)?.stats;
    if (s && typeof s.pluginId === "string" && typeof s.installedAt === "number") {
      serviceStats = { ...serviceStats, [s.pluginId]: s };
      lsSet(`${NS}.service.v1`, serviceStats);
      renderPanel();
    }
  });
  on("uninstall", (d) => {
    const p = d as { pluginId?: string } | undefined;
    if (typeof p?.pluginId === "string" && serviceStats[p.pluginId]) {
      const s = { ...serviceStats[p.pluginId]!, uninstalledAt: Date.now() };
      serviceStats = { ...serviceStats, [p.pluginId]: s };
      lsSet(`${NS}.service.v1`, serviceStats);
      curTab = "obituary";
      if (panel) renderPanel();
      ecoEvent("obituary-show", obituaryOf(s, Date.now()));
    }
  });
}

// --- S0 注册表联动 ---

function onRegistryChange(): void {
  if (typeof document === "undefined") return;
  const anyOn = ECO_NOVA_FEATURES.some((f) => flagOn(f.id));
  if (!anyOn) togglePanel(false);
}

// ---------------------------------------------------------------------------
// 激活 / 卸载（幂等）
// ---------------------------------------------------------------------------

export function activateEcoNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  loadState();
  ensureStyle();

  const onOpen = (): void => togglePanel();
  window.addEventListener("nova://eco-open", onOpen);
  bag.push(() => window.removeEventListener("nova://eco-open", onOpen));

  // Hub overlay 直达（ai04 协议：nova-genes / nova-radar / nova-lighthouse）
  const onOpenFeature = (ev: Event): void => {
    const f = (ev as CustomEvent).detail?.feature;
    if (f === "nova-genes" && flagOn("W-127")) {
      curTab = "genes";
      togglePanel(true);
    }
    if (f === "nova-radar" && flagOn("W-128")) {
      curTab = "radar";
      togglePanel(true);
    }
    if (f === "nova-lighthouse" && flagOn("W-131")) {
      curTab = "lighthouse";
      togglePanel(true);
    }
  };
  window.addEventListener("ai04:open-feature", onOpenFeature);
  bag.push(() => window.removeEventListener("ai04:open-feature", onOpenFeature));

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

export function deactivateEcoNova(): void {
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
  trainTimer = 0;
  relayPack = null;
  relayParts = [];
  relayPos = 0;
  liveScript = null; // 剧本随模块卸载销毁（纯内存语义）
  replayIdx = 0;
  permBag = null;
  themePair = null;
  surgerySel = new Set();
  document.querySelectorAll(".nova-eco-hud").forEach((n) => n.remove());
}

export function isEcoNovaActive(): boolean {
  return active;
}
