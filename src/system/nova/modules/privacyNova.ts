/**
 * NOVA-200 · S10 隐私叙事路（AI-10）—— 域10 安全与隐私（W-114…W-126）。
 *
 * 边界（全景 §10）：Q-64 访问日志管**审计留档**（W-117 是访问当下的实时气泡，双源
 * 同事件）；U-35 信任链管**信任分基座**（W-120 是透明衰减曲线与复审建议）；U-26
 * 标签体系管文件标注（W-118 是剪贴板显式标记即焚）；V-90 沙盒管通用隔离执行
 * （W-121 是教育向泄露剧场，零真实数据）；Z-68 零闪白门禁不涉本域（无全屏转场）。
 *
 * 纪律：
 * - 零侵入：不改写剪贴板/回收站/设置/ singularity 任何内部逻辑；全部为 DOM 叠层 +
 *   CSS 类名挂载 + `nova://privacy-*` 自定义事件摄入（访问日志/信任链/DNS/ACL 侧
 *   接线由 S17 在既有文件按 wiringHint 补齐，本模块 API/事件契约已备好）；
 * - 前缀：类名 `nova-`、事件 `nova://privacy-*`、localStorage 键 `nova.privacy.*`；
 * - 开关：只读消费 S0 注册表（novaOn），无注册表时无开关即不激活；
 * - 降级：reduce-motion / safeMode / static 下眼罩呼吸/焚毁动画/气泡浮入归零
 *   （语义保留：蒙眼仍蒙眼、焚毁仍焚毁）；
 * - 诚实（分工图验收）：
 *   · 密码内容零记录 —— W-115 只算分不落盘，建议文本永不包含密码本身；
 *   · 焚毁 = 剪贴板 + 历史（本域痕迹簿）彻底清除，焚毁后残留清点必须为 0；
 *   · 剧场排演零真实数据 —— 所有剧料强制 sandbox 标记，未标记摄入直接拒绝；
 *   · ACL 不支持时如实标注 NOT AVAILABLE，绝不编造族谱；
 *   · DNS 未收录域名如实「未收录」，可疑红标只依启发式并可解释。
 */

import { novaMotionOK, novaOn } from "../registry";
import { subscribeNova } from "../registry";

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

function lsDel(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    /* ignore */
  }
}

const NS = "nova.privacy";

/** 功能开关：只读消费 S0 注册表（nova.registry.v1 单一事实源）。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

/** 派发 `nova://privacy-*` 事件（SSR/测试环境安全）。 */
export function privacyEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://privacy-${name}`, { detail }));
}

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

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
  /** 需要 S17 在既有文件补接线的行为（本模块 API/事件已备好）。 */
  wiringHint?: string;
  /** reduce-motion / safeMode / static 降级说明。 */
  degrade: string;
}

export const PRIVACY_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-114",
    titleZh: "隐私体检报告",
    titleEn: "Privacy Checkup",
    descZh: "季度三轴体检（留痕/权限/出口）：依本域真实痕迹簿打分出 A–D 等级与整改入口，逐轴给可执行整改项；距上次体检不足 90 天不重复打扰。",
    defaultOn: true,
    overlay: "nova-checkup",
    wiringHint: "痕迹摄入走本域各事件；体检报告面板已备好（数据全部本地）",
    degrade: "静态报告（无逐轴揭示动效）；分数与整改项语义不变",
  },
  {
    id: "W-115",
    titleZh: "密码力度合奏",
    titleEn: "Password Ensemble",
    descZh: "密码框获焦即时出三轨力度条（长度/字符种/熵余量）与具体指导（再长几位/加符号/去连续重复）；只算分不落盘，建议永不复述密码内容。",
    defaultOn: true,
    wiringHint: "密码框 input 派发 nova://privacy-pw {score} 由本模块 API 计算后回显；明文永不进入本域",
    degrade: "静态三轨条；计算纯本地零网络",
  },
  {
    id: "W-116",
    titleZh: "摄像头眼罩",
    titleEn: "Camera Blindfold",
    descZh: "摄像头启用瞬间浮出眼罩贴片，双击即物理遮蔽画面并挂「已蒙眼」徽标；再双击摘罩；蒙眼状态全程可一键确认。",
    defaultOn: true,
    wiringHint: "摄像头启用派发 nova://privacy-camera {active, device}；遮蔽层已备好（双击交互）",
    degrade: "静态眼罩与徽标（无呼吸动效）；蒙眼语义不变",
  },
  {
    id: "W-117",
    titleZh: "敏感文件气泡",
    titleEn: "Sensitive Bubble",
    descZh: "敏感文件（凭据/密钥/账本类特征）被访问当下浮 2s 实时气泡：谁在读、读什么、何时；与 Q-64 访问日志同源双用，气泡即焚不落盘。",
    defaultOn: true,
    wiringHint: "访问哨兵派发 nova://privacy-access {path, app}（与 Q-64 同事件源）；气泡已备好",
    degrade: "静态气泡（无浮入动效）；2s 时限不变",
  },
  {
    id: "W-118",
    titleZh: "阅后即焚剪贴板",
    titleEn: "Burn-After-Read",
    descZh: "显式标记的剪贴板内容一次读取即焚：首读交付、再读为空，标记内容绝不写入任何持久层；焚毁动作含内存与引用双清。",
    defaultOn: true,
    wiringHint: "标记入口派发 nova://privacy-burn-mark {text}；读取消费 nova://privacy-burn-read",
    degrade: "即焚语义与动效无关，全程无降级差异",
  },
  {
    id: "W-119",
    titleZh: "后台心跳墙",
    titleEn: "Heartbeat Wall",
    descZh: "后台应用按三电平（静息/呼吸/活跃）平行上墙：一眼看清谁在做什么强度的事；墙面数据全部来自真实采样事件，无采样即空墙。",
    defaultOn: true,
    overlay: "nova-pulsewall",
    wiringHint: "采样器派发 nova://privacy-heartbeat {apps:[{name, pct}]}；心跳墙面板已备好",
    degrade: "静态墙（无心跳脉冲动效）；三电平判定不变",
  },
  {
    id: "W-120",
    titleZh: "信任衰减曲线",
    titleEn: "Trust Decay",
    descZh: "插件信任分透明衰减：base − 天数×0.5 − 事故×12，公式全公开可手算；低于 60 分或 60 天未复审即出复审建议卡，与 U-35 信任链同源对接。",
    defaultOn: true,
    wiringHint: "信任链派发 nova://privacy-trust {plugin, base, incidents}；曲线与建议卡已备好",
    degrade: "静态曲线（无描线动画）；公式与阈值不变",
  },
  {
    id: "W-121",
    titleZh: "隐私剧场排演",
    titleEn: "Leak Theater",
    descZh: "教育向沙盒泄露剧场：固定合成剧料逐幕演示「一次随意粘贴会泄露什么」，闭幕自动销毁全部剧料并清点残留归零；未带 sandbox 标记的剧料一律拒收。",
    defaultOn: false,
    wiringHint: "排演入口调 stageRun() API；剧料摄入走 stageIngest（强制 sandbox 校验）",
    degrade: "静态幕布（无舞台动效）；销毁与清点语义不变",
  },
  {
    id: "W-122",
    titleZh: "DNS 白话簿",
    titleEn: "DNS Ledger",
    descZh: "DNS 请求白话账本：内置词典把常用域名翻成人话（字体服务/更新源…），未收录如实标注；可疑特征（punycode/裸 IP/多连字符/跟踪词根）红标并给出理由。",
    defaultOn: true,
    overlay: "nova-dns",
    wiringHint: "DNS 解析派发 nova://privacy-dns {domain}；账本面板已备好（仅存域名不存内容）",
    degrade: "静态账本（无行入动效）；红标理由逐条可解释",
  },
  {
    id: "W-123",
    titleZh: "指纹黑匣",
    titleEn: "Fingerprint Vault",
    descZh: "环境特征单向哈希入黑匣（FNV-1a，不可逆）：迁移后比对相似度辅助辨认「是不是原来那台」；全程 opt-in，默认关闭，特征向量绝不落盘。",
    defaultOn: false,
    wiringHint: "特征向量采集后调 fingerprintOf() 入匣；比对走 fingerprintMigration()",
    degrade: "纯计算功能，无动效可降级",
  },
  {
    id: "W-124",
    titleZh: "安全红线通知",
    titleEn: "Redline Notice",
    descZh: "安全事件金边红字置顶通行权：越过一切浮层直达视线，点按确认为止；通知只留标题与时间元数据，正文即焚不落盘。",
    defaultOn: true,
    wiringHint: "安全事件派发 nova://privacy-redline {title, body}；红线卡已备好（金边红字置顶）",
    degrade: "静态红线卡（无脉冲动效）；置顶通行权不变",
  },
  {
    id: "W-125",
    titleZh: "权限族谱",
    titleEn: "Permission Tree",
    descZh: "文件 ACL 继承链族谱视图：根→叶逐级列出继承来源与权限项；环境不支持 ACL 时如实标注 NOT AVAILABLE，绝不编造族谱。",
    defaultOn: true,
    wiringHint: "ACL 读取派发 nova://privacy-acl {path, chain?, supported?}；族谱卡已备好",
    degrade: "静态族谱；不支持即如实标注（诚实边界）",
  },
  {
    id: "W-126",
    titleZh: "焚毁仪式",
    titleEn: "Burn Ritual",
    descZh: "销毁动作 1.2s 蓄力焚毁仪式：金红幕布蓄满后执行——剪贴板清空 + 本域痕迹历史彻底清除 + 残留清点必须归零；reduce-motion 直焚不演。",
    defaultOn: true,
    wiringHint: "销毁入口派发 nova://privacy-burn {targets}；剪贴板清空需 S17 接系统 API（事件已备好）",
    degrade: "reduce-motion 下 0ms 直焚（无蓄力动画）；清除语义完整保留",
  },
];

export const privacyNovaDomain = {
  id: "S10",
  nameZh: "隐私叙事",
  nameEn: "Privacy Narrative",
  route: "AI-10",
  features: PRIVACY_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// W-114 隐私体检报告（季度三轴）
// ---------------------------------------------------------------------------

export const CHECKUP_INTERVAL_DAYS = 90;

export interface CheckupInputs {
  /** 未焚即焚标记残留数（W-118）。 */
  burnLeftover: number;
  /** 敏感文件访问次数（W-117）。 */
  sensitiveHits: number;
  /** 摄像头未蒙眼在用（W-116）。 */
  cameraLive: boolean;
  /** DNS 可疑请求数（W-122）。 */
  dnsSuspicious: number;
  /** 无标记剪贴板写入次数。 */
  clipboardPlain: number;
  /** 最低插件信任分（W-120，无插件为 100）。 */
  trustMin: number;
}

export type CheckupAxis = "traces" | "access" | "egress";

export interface CheckupAxisScore {
  axis: CheckupAxis;
  score: number;
  fixes: string[];
}

export interface CheckupReport {
  axes: CheckupAxisScore[];
  grade: "A" | "B" | "C" | "D";
  at: number;
}

export const CHECKUP_FIXES: Record<CheckupAxis, string> = {
  traces: "焚毁残留与可疑解析清零：对痕迹簿执行一次焚毁仪式（W-126）",
  access: "收拢权限：给摄像头戴上眼罩（W-116），复审低信任插件（W-120）",
  egress: "管住出口：敏感文件访问走阅后即焚标记（W-118），减少明文剪贴板",
};

/** 距上次体检是否到期（从未体检即到期）。 */
export function checkupDue(lastAt: number | null, now: number): boolean {
  if (lastAt == null) return true;
  return now - lastAt >= CHECKUP_INTERVAL_DAYS * DAY;
}

/** 单轴计分（clamp 0–100；分越高越健康）。 */
function axisScore(negatives: number[]): number {
  let s = 100;
  for (const n of negatives) s -= n;
  return clamp(Math.round(s), 0, 100);
}

/** 生成三轴体检报告（纯函数；分数只依赖输入，不读任何持久层）。 */
export function checkupRun(inputs: CheckupInputs, now: number): CheckupReport {
  const traces = axisScore([inputs.burnLeftover * 10, inputs.dnsSuspicious * 4]);
  const access = axisScore([inputs.cameraLive ? 25 : 0, inputs.trustMin < 60 ? 25 : 0, inputs.trustMin < 30 ? 15 : 0]);
  const egress = axisScore([inputs.sensitiveHits * 3, inputs.clipboardPlain * 2]);
  const axes: CheckupAxisScore[] = [
    { axis: "traces", score: traces, fixes: traces < 100 ? [CHECKUP_FIXES.traces] : [] },
    { axis: "access", score: access, fixes: access < 100 ? [CHECKUP_FIXES.access] : [] },
    { axis: "egress", score: egress, fixes: egress < 100 ? [CHECKUP_FIXES.egress] : [] },
  ];
  const avg = Math.round((traces + access + egress) / 3);
  const grade = avg >= 90 ? "A" : avg >= 75 ? "B" : avg >= 60 ? "C" : "D";
  return { axes, grade, at: now };
}

export function checkupGradeText(r: CheckupReport): string {
  const axes = r.axes.map((a) => `${a.axis.toUpperCase()} ${a.score}`).join(" · ");
  return `PRIVACY CHECKUP ${r.grade} — ${axes}`;
}

// ---------------------------------------------------------------------------
// W-115 密码力度合奏（三轨；内容零记录）
// ---------------------------------------------------------------------------

export const PASSWORD_MIN_LEN = 8;
export const PASSWORD_TRACK_WEIGHTS = { length: 0.4, variety: 0.3, entropy: 0.3 } as const;

export interface PasswordTracks {
  length: number;
  variety: number;
  entropy: number;
  total: number;
}

function charClasses(pw: string): number {
  let n = 0;
  if (/[a-z]/.test(pw)) n++;
  if (/[A-Z]/.test(pw)) n++;
  if (/[0-9]/.test(pw)) n++;
  if (/[^a-zA-Z0-9]/.test(pw)) n++;
  return n;
}

/** 重复字符占比（0–1；用于熵轨扣分）。 */
function repeatRatio(pw: string): number {
  if (pw.length === 0) return 0;
  const seen = new Map<string, number>();
  for (const ch of pw) seen.set(ch, (seen.get(ch) ?? 0) + 1);
  let repeats = 0;
  for (const c of seen.values()) if (c > 1) repeats += c - 1;
  return repeats / pw.length;
}

/** 连续序列长度（abc/123 等顺子；用于熵轨扣分）。 */
function longestRun(pw: string): number {
  let best = 0;
  let run = 1;
  for (let i = 1; i < pw.length; i++) {
    const d = (pw.charCodeAt(i) ?? 0) - (pw.charCodeAt(i - 1) ?? 0);
    if (d === 1 || d === -1) {
      run++;
      best = Math.max(best, run);
    } else {
      run = 1;
    }
  }
  return Math.max(best, pw.length > 0 ? 1 : 0);
}

/**
 * 三轨力度（纯计算；结果只有 0–100 分数，密码本身不出现在任何返回值中）。
 * 长度轨：8 位起评、32 位满分；字符种轨：4 类均分；熵轨：重复与顺子扣分。
 */
export function passwordTracks(pw: string): PasswordTracks {
  const length = clamp(Math.round(((pw.length - PASSWORD_MIN_LEN) / 24) * 100), 0, 100);
  const variety = clamp(Math.round((charClasses(pw) / 4) * 100), 0, 100);
  const entropy = clamp(Math.round(100 - repeatRatio(pw) * 60 - Math.max(0, longestRun(pw) - 3) * 10), 0, 100);
  const total = Math.round(length * PASSWORD_TRACK_WEIGHTS.length + variety * PASSWORD_TRACK_WEIGHTS.variety + entropy * PASSWORD_TRACK_WEIGHTS.entropy);
  return { length, variety, entropy, total };
}

/** 具体指导（逐条可执行；永不包含密码内容 —— 验收红线）。 */
export function passwordAdvice(pw: string): string[] {
  const t = passwordTracks(pw);
  const out: string[] = [];
  if (pw.length < PASSWORD_MIN_LEN) out.push(`至少 ${PASSWORD_MIN_LEN} 位：当前不足，先补长度`);
  else if (t.length < 100) out.push(`再长 ${Math.ceil(((100 - t.length) / 100) * 24)} 位可满分`);
  if (charClasses(pw) < 3) out.push("加入大写字母、数字或符号中的至少一类");
  if (longestRun(pw) >= 4) out.push("避免 abc/1234 这类连续序列");
  if (repeatRatio(pw) > 0.3) out.push("同一字符重复过多，换些不同的字符");
  if (out.length === 0) out.push("三轨全绿 —— 这把钥匙很结实");
  return out;
}

// ---------------------------------------------------------------------------
// W-116 摄像头眼罩
// ---------------------------------------------------------------------------

export const CAMERA_BADGE_MASKED = "已蒙眼";
export const CAMERA_BADGE_LIVE = "摄像头使用中";

/** 蒙眼状态翻转（纯）。 */
export function cameraMaskToggle(masked: boolean): boolean {
  return !masked;
}

export function cameraBadgeText(masked: boolean): string {
  return masked ? CAMERA_BADGE_MASKED : CAMERA_BADGE_LIVE;
}

// ---------------------------------------------------------------------------
// W-117 敏感文件气泡
// ---------------------------------------------------------------------------

export const SENSITIVE_PATTERNS: RegExp[] = [
  /id_rsa|id_ed25519|id_ecdsa/i,
  /\.pem|\.key|\.keystore|\.pfx/i,
  /\.env(\.|$)/i,
  /passwo?rd|passwd|credential|secret|token/i,
  /wallet|keystore|mnemonic/i,
  /密码|账本|凭据|密钥/i,
];

/** 敏感文件判定（特征表；逐条可解释）。 */
export function isSensitivePath(path: string): boolean {
  return SENSITIVE_PATTERNS.some((re) => re.test(path));
}

/** 命中的敏感特征（解释用）。 */
export function sensitiveHitReason(path: string): string | null {
  for (const re of SENSITIVE_PATTERNS) {
    const m = re.exec(path);
    if (m) return m[0] ?? null;
  }
  return null;
}

/** 气泡文案（2s 即焚，不落盘）。 */
export function sensitiveBubbleText(path: string, app: string): string {
  const base = path.split(/[\\/]/).pop() ?? path;
  return `「${base}」正被 ${app} 读取`;
}

export const SENSITIVE_BUBBLE_MS = 2000;

// ---------------------------------------------------------------------------
// W-118 阅后即焚剪贴板
// ---------------------------------------------------------------------------

export interface BurnEnv {
  /** 显式标记。 */
  marked: true;
  text: string;
  at: number;
  /** 已被读过（一次即焚）。 */
  consumed: boolean;
}

export const BURN_MARK_TEXT = "阅后即焚";

/** 显式标记入封套（仅内存态，绝不持久化）。 */
export function burnMarkEnv(text: string, at: number): BurnEnv {
  return { marked: true, text, at, consumed: false };
}

/** 一次即焚读取：首读交付并烧毁，再读为空。 */
export function burnReadEnv(env: BurnEnv): { text: string } | null {
  if (env.consumed) return null;
  env.consumed = true;
  const text = env.text;
  env.text = ""; // 引用内容同步清空
  return { text };
}

/** 彻底清除（焚毁仪式复用）：封套归零。 */
export function burnPurgeEnv(env: BurnEnv | null): boolean {
  if (!env) return true;
  env.text = "";
  env.consumed = true;
  return env.text === "";
}

// ---------------------------------------------------------------------------
// W-119 后台心跳墙（三电平）
// ---------------------------------------------------------------------------

export const HEARTBEAT_QUIET_PCT = 2;
export const HEARTBEAT_BUSY_PCT = 15;

export interface HeartbeatApp {
  name: string;
  pct: number;
}

export type HeartbeatLevel = 0 | 1 | 2;
export const HEARTBEAT_LEVEL_TEXT: Record<HeartbeatLevel, string> = {
  0: "静息",
  1: "呼吸",
  2: "活跃",
};

/** 三电平判定：静息(0) / 呼吸(1) / 活跃(2)。 */
export function heartbeatLevel(pct: number): HeartbeatLevel {
  if (pct >= HEARTBEAT_BUSY_PCT) return 2;
  if (pct >= HEARTBEAT_QUIET_PCT) return 1;
  return 0;
}

export interface HeartbeatRow extends HeartbeatApp {
  level: HeartbeatLevel;
}

/** 心跳墙行：按占用降序（同分按名字稳定序）。 */
export function heartbeatWall(apps: HeartbeatApp[]): HeartbeatRow[] {
  return [...apps]
    .sort((a, b) => b.pct - a.pct || a.name.localeCompare(b.name))
    .map((a) => ({ ...a, level: heartbeatLevel(a.pct) }));
}

export function heartbeatRowText(row: HeartbeatRow): string {
  return `${row.name} — ${HEARTBEAT_LEVEL_TEXT[row.level]} ${row.pct.toFixed(1)}%`;
}

// ---------------------------------------------------------------------------
// W-120 信任衰减曲线（透明公式）
// ---------------------------------------------------------------------------

export const TRUST_DECAY_PER_DAY = 0.5;
export const TRUST_INCIDENT_PENALTY = 12;
export const TRUST_REREVIEW_DAYS = 60;
export const TRUST_REREVIEW_SCORE = 60;

/**
 * 透明衰减公式（全公开可手算）：
 *   score = clamp(base − 天数×0.5 − 事故×12, 0, 100)
 */
export function trustScore(base: number, days: number, incidents: number): number {
  return clamp(Math.round(base - days * TRUST_DECAY_PER_DAY - incidents * TRUST_INCIDENT_PENALTY), 0, 100);
}

/** 复审建议：低于 60 分或 60 天未复审。 */
export function trustReReview(score: number, days: number): boolean {
  return score < TRUST_REREVIEW_SCORE || days >= TRUST_REREVIEW_DAYS;
}

export function trustReviewText(plugin: string, score: number, days: number): string {
  return `「${plugin}」信任 ${score} 分 · 距上次复审 ${days} 天 — 建议复审`;
}

/** 曲线采样（0..days 逐日；可视化用）。 */
export function trustCurve(base: number, incidents: number, days: number): number[] {
  const out: number[] = [];
  for (let d = 0; d <= days; d++) out.push(trustScore(base, d, incidents));
  return out;
}

// ---------------------------------------------------------------------------
// W-121 隐私剧场排演（沙盒；零真实数据）
// ---------------------------------------------------------------------------

export interface StageRecord {
  /** 强制沙盒标记：未标记一律拒收。 */
  sandbox: true;
  scene: string;
  text: string;
  at: number;
}

export const STAGE_REJECT_REAL = "STAGE REJECT: 非 sandbox 剧料拒收（零真实数据红线）";

/** 拒收未标记剧料（零真实数据守卫）。 */
export function stageIngest(rec: { sandbox?: boolean; scene?: string; text?: string; at?: number }): StageRecord | null {
  if (rec.sandbox !== true || typeof rec.scene !== "string" || typeof rec.text !== "string") return null;
  return { sandbox: true, scene: rec.scene, text: rec.text, at: rec.at ?? 0 };
}

/** 固定合成剧本（教育向五幕；全部合成人物，无任何真实数据）。 */
export function stageScript(): StageRecord[] {
  const base = Date.now();
  const scenes: Array<[string, string]> = [
    ["第一幕 · 一段随手复制", "演员A 复制了一段含密钥的文本进剪贴板，忘了它还在那里"],
    ["第二幕 · 无声的访问", "演员B 的工具读取了剪贴板 —— 没有任何提示"],
    ["第三幕 · 敏感文件路过", "演员A 的账本文件被扫描器点名，气泡只闪了 2 秒"],
    ["第四幕 · 权限的族谱", "回头看：那份文件从根目录继承来的权限，比想象的多"],
    ["第五幕 · 闭幕焚毁", "仪式开始：剪贴板、痕迹、历史，一并归零"],
  ];
  return scenes.map(([scene, text], i) => ({ sandbox: true as const, scene, text, at: base + i * 1000 }));
}

export const STAGE_DESTROY_NOTICE = "THEATER CLOSED — 剧料已全部销毁，残留清点：0";

/** 残留清点（必须为 0 才算闭幕）。 */
export function stageResidue(records: StageRecord[]): number {
  return records.filter((r) => r.text !== "" || r.scene !== "").length;
}

/** 自动销毁（清空文本与场景；返回清点结果）。 */
export function stageDestroy(records: StageRecord[]): number {
  for (const r of records) {
    r.text = "";
    r.scene = "";
  }
  return stageResidue(records);
}

// ---------------------------------------------------------------------------
// W-122 DNS 白话簿
// ---------------------------------------------------------------------------

export const DNS_GLOSSARY: Record<string, string> = {
  "fonts.googleapis.com": "字体服务",
  "fonts.gstatic.com": "字体文件",
  "api.github.com": "代码托管接口",
  "github.com": "代码托管",
  "registry.npmjs.org": "npm 包仓库",
  "cdn.jsdelivr.net": "前端镜像源",
  "update.microsoft.com": "系统更新源",
  "windowsupdate.com": "系统更新源",
  "dns.google": "公共解析器",
  "cloudflare-dns.com": "公共解析器",
  "telemetry.example.com": "遥测上报（示例）",
  "localhost": "本机",
};

/** 风险词根与特征（红标理由逐条可解释）。 */
export const DNS_TRACKER_WORDS = ["track", "ads.", "adserver", "telemetry", "metrics", "analytics"];
export const DNS_RISKY_TLDS = ["zip", "mov", "top", "xyz", "click", "country", "gq", "tk", "cf", "ml"];

export interface DnsAnnotation {
  domain: string;
  plain: string;
  suspicious: boolean;
  reasons: string[];
}

/** 白话标注 + 可疑红标（启发式全部可解释）。 */
export function dnsAnnotate(domain: string): DnsAnnotation {
  const d = domain.trim().toLowerCase();
  const reasons: string[] = [];
  const plain = DNS_GLOSSARY[d] ?? "未收录";
  if (/^xn--/.test(d) || /\.xn--/.test(d)) reasons.push("punycode 编码域名（易仿冒）");
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(d)) reasons.push("裸 IP 直连（无域名）");
  const labels = d.split(".");
  if (labels.length >= 4 && labels.filter((l) => l.includes("-")).length >= 2) reasons.push("多级连字符子域（疑似生成域）");
  for (const w of DNS_TRACKER_WORDS) {
    if (d.includes(w)) {
      reasons.push(`跟踪词根「${w}」`);
      break;
    }
  }
  const tld = labels[labels.length - 1] ?? "";
  if (DNS_RISKY_TLDS.includes(tld)) reasons.push(`高风险后缀 .${tld}`);
  return { domain: d, plain, suspicious: reasons.length > 0, reasons };
}

export const DNS_LEDGER_MAX = 200;

export interface DnsReq {
  domain: string;
  at: number;
}

/** 账本追加（仅存域名不存内容；容量滚动）。 */
export function dnsLedgerAppend(ledger: DnsReq[], req: DnsReq): DnsReq[] {
  const next = [...ledger, req];
  return next.length > DNS_LEDGER_MAX ? next.slice(-DNS_LEDGER_MAX) : next;
}

export function dnsRowText(req: DnsReq): string {
  const a = dnsAnnotate(req.domain);
  return `${a.domain} — ${a.plain}${a.suspicious ? ` ⚠ ${a.reasons.join("；")}` : ""}`;
}

// ---------------------------------------------------------------------------
// W-123 指纹黑匣（单向哈希；opt-in）
// ---------------------------------------------------------------------------

/** FNV-1a 32bit（单向；不可逆）。 */
export function fpHash(input: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    h ^= input.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16).padStart(8, "0");
}

/** 特征向量 → 单向指纹（向量本身绝不落盘）。 */
export function fingerprintOf(vector: string[]): string {
  return fpHash(vector.join("\u0000"));
}

/** 十六进制相似度（逐位一致比例 0–1）。 */
export function fingerprintSimilarity(a: string, b: string): number {
  if (a.length === 0 || a.length !== b.length) return 0;
  let same = 0;
  for (let i = 0; i < a.length; i++) {
    if (a[i] === b[i]) same++;
  }
  return same / a.length;
}

export const FINGERPRINT_MIGRATE_MATCH = 0.75;

export interface FingerprintMigration {
  same: boolean;
  similarity: number;
}

/** 迁移比对：相似度 ≥ 0.75 判同一环境。 */
export function fingerprintMigration(oldFp: string, curFp: string): FingerprintMigration {
  const similarity = fingerprintSimilarity(oldFp, curFp);
  return { same: similarity >= FINGERPRINT_MIGRATE_MATCH, similarity };
}

/** opt-in 守卫（默认关闭；未开启绝不采集）。 */
export function fingerprintAllowed(): boolean {
  return flagOn("W-123");
}

// ---------------------------------------------------------------------------
// W-124 安全红线通知（金边红字置顶）
// ---------------------------------------------------------------------------

export const REDLINE_Z = 2147483400;
export const REDLINE_LOG_MAX = 50;

export interface RedlineCard {
  title: string;
  body: string;
  at: number;
}

export interface RedlineLogEntry {
  /** 仅标题与时间元数据；正文即焚不落盘（验收红线）。 */
  title: string;
  at: number;
}

export function redlineCardText(card: RedlineCard): string {
  return `${card.title}\n${card.body}`;
}

/** 日志净化：只留标题与时间。 */
export function redlineSanitize(card: RedlineCard): RedlineLogEntry {
  return { title: card.title, at: card.at };
}

export function redlineLogAppend(log: RedlineLogEntry[], e: RedlineLogEntry): RedlineLogEntry[] {
  const next = [...log, e];
  return next.length > REDLINE_LOG_MAX ? next.slice(-REDLINE_LOG_MAX) : next;
}

// ---------------------------------------------------------------------------
// W-125 权限族谱（ACL 继承链）
// ---------------------------------------------------------------------------

export interface AclEntry {
  /** 继承来源（根为 null）。 */
  from: string | null;
  name: string;
  rights: string[];
}

export interface AclChain {
  path: string;
  chain: AclEntry[];
}

export const ACL_UNSUPPORTED_NOTE = "ACL NOT AVAILABLE — 环境未提供 ACL，族谱如实标注";

/** 族谱文本：根→叶逐级缩进。 */
export function aclChainText(c: AclChain): string {
  const lines: string[] = [c.path];
  c.chain.forEach((e, i) => {
    const pad = "  ".repeat(i + 1);
    lines.push(`${pad}↳ ${e.name} [${e.rights.join(", ")}]${e.from != null ? ` ← ${e.from}` : ""}`);
  });
  return lines.join("\n");
}

/** 不支持时的诚实族谱（唯一合法输出）。 */
export function aclUnsupportedText(path: string): string {
  return `${path}\n  ↳ ${ACL_UNSUPPORTED_NOTE}`;
}

// ---------------------------------------------------------------------------
// W-126 焚毁仪式（1.2s 蓄力 + 彻底清除）
// ---------------------------------------------------------------------------

export const BURN_CHARGE_MS = 1200;

export type BurnTarget = "clipboard" | "history" | "ledger";

export const BURN_TARGET_ALL: BurnTarget[] = ["clipboard", "history", "ledger"];

export const BURN_STEP_TEXT: Record<BurnTarget, string> = {
  clipboard: "剪贴板已清空",
  history: "痕迹历史已彻底清除",
  ledger: "DNS 账本已清空",
};

/** 焚毁计划：按固定顺序裁剪目标。 */
export function burnPlan(targets: BurnTarget[]): BurnTarget[] {
  return BURN_TARGET_ALL.filter((t) => targets.includes(t));
}

/** 本域痕迹键清单（焚毁范围 = 全部 nova.privacy.* 持久键）。 */
export function burnHistoryKeys(): string[] {
  const keys: string[] = [];
  try {
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k && k.startsWith(`${NS}.`)) keys.push(k);
    }
  } catch {
    /* 无 storage：范围即空 */
  }
  return keys;
}

/** 残留清点（焚毁后必须为 0 —— 验收红线）。 */
export function burnResidue(): number {
  return burnHistoryKeys().length;
}

// ---------------------------------------------------------------------------
// 行为层（DOM 叠层 + 事件摄入；纯逻辑已全部可测）
// ---------------------------------------------------------------------------

const STYLE_ID = "nova-privacy-style";

function ensureStyle(): void {
  if (typeof document === "undefined" || document.getElementById(STYLE_ID)) return;
  const css = `
@keyframes nova-privacy-breathe { 0%,100% { opacity:.5 } 50% { opacity:1 } }
@keyframes nova-privacy-bubble { from { opacity:0; transform:translateY(6px) } to { opacity:1; transform:translateY(0) } }
.nova-privacy-bubble { position:fixed; left:50%; bottom:76px; transform:translateX(-50%); z-index:2147483000;
  padding:8px 14px; border-radius:999px; background:rgba(120,40,160,.88); color:#fff;
  font-size:12px; letter-spacing:.04em; white-space:nowrap; }
.nova-privacy-bubble[data-motion="1"] { animation: nova-privacy-bubble .18s ease-out; }
.nova-privacy-mask { position:fixed; right:18px; top:64px; z-index:2147483000; width:120px; height:120px;
  border-radius:50%; display:flex; align-items:center; justify-content:center; flex-direction:column; gap:6px;
  background:rgba(10,10,14,.92); color:#fff; cursor:pointer; user-select:none;
  box-shadow:0 2px 12px rgba(0,0,0,.4); font-size:12px; letter-spacing:.08em; text-align:center; }
.nova-privacy-mask[data-masked="1"] { background:rgba(30,30,34,.98); }
.nova-privacy-mask .eye { font-size:34px; line-height:1; }
.nova-privacy-mask[data-motion="1"] { animation: nova-privacy-breathe 2.2s ease-in-out infinite; }
.nova-privacy-redline { position:fixed; left:50%; top:44px; transform:translateX(-50%); z-index:2147483400;
  min-width:280px; max-width:440px; padding:10px 16px; border-radius:10px; cursor:pointer;
  background:rgba(60,8,8,.94); color:#ff5a52; border:2px solid #d4af37;
  font-size:12px; line-height:1.7; letter-spacing:.04em; white-space:pre-line;
  box-shadow:0 4px 18px rgba(0,0,0,.5); }
.nova-privacy-redline .t { font-size:9px; letter-spacing:.22em; color:#d4af37; margin-bottom:4px; }
.nova-privacy-burn { position:fixed; inset:0; z-index:2147483300; display:flex; align-items:center; justify-content:center;
  background:rgba(8,4,2,.86); color:#ffb37a; font-size:13px; letter-spacing:.3em; flex-direction:column; gap:14px; }
.nova-privacy-burn .gauge { width:220px; height:3px; background:rgba(255,179,122,.2); border-radius:2px; overflow:hidden; }
.nova-privacy-burn .gauge > i { display:block; height:100%; width:0%; background:#ff7a3c; transition:width linear; }
.nova-privacy-panel { position:fixed; right:18px; top:56px; z-index:2147483000; width:400px; max-height:72vh;
  overflow:auto; padding:14px 16px; border-radius:12px; background:var(--nova-glass,rgba(18,16,22,.88));
  backdrop-filter:blur(14px); border:1px solid rgba(255,255,255,.1); color:inherit;
  font-size:12px; line-height:1.7; letter-spacing:.02em; }
.nova-privacy-panel h3 { margin:10px 0 4px; font-size:11px; letter-spacing:.14em; opacity:.85; }
.nova-privacy-panel h3:first-child { margin-top:0; }
.nova-privacy-panel .muted { opacity:.55; font-size:11px; }
.nova-privacy-panel pre { margin:4px 0; padding:8px; border-radius:8px; background:rgba(127,127,127,.1);
  font-size:11px; line-height:1.6; white-space:pre-wrap; }
.nova-privacy-track { display:flex; align-items:center; gap:8px; margin:4px 0; }
.nova-privacy-track .bar { flex:1; height:4px; border-radius:2px; background:rgba(127,127,127,.18); overflow:hidden; }
.nova-privacy-track .bar > i { display:block; height:100%; background:#7ac0ff; }
.nova-privacy-grade-a { color:#4caf7d; }
.nova-privacy-grade-b { color:#7ac0ff; }
.nova-privacy-grade-c { color:#d9a13b; }
.nova-privacy-grade-d { color:#e0564b; }
.nova-privacy-suspicious { color:#e0564b; }
`;
  const style = document.createElement("style");
  style.id = STYLE_ID;
  style.textContent = css;
  document.head.appendChild(style);
}

/** 气泡元素（2s 自动消失）。 */
function bubble(text: string, ttlMs: number): void {
  if (typeof document === "undefined") return;
  const el = document.createElement("div");
  el.className = "nova-privacy-bubble";
  el.dataset.motion = motionOK() ? "1" : "0";
  el.textContent = text;
  document.body.appendChild(el);
  window.setTimeout(() => el.remove(), ttlMs);
}

/** 通用瞬态卡（W-120 复审建议等）。 */
function flashCard(title: string, body: string, ttlMs: number): void {
  if (typeof document === "undefined") return;
  const el = document.createElement("div");
  el.className = "nova-privacy-bubble";
  el.dataset.motion = motionOK() ? "1" : "0";
  el.style.whiteSpace = "pre-line";
  const t = document.createElement("div");
  t.style.cssText = "font-size:9px;letter-spacing:.22em;opacity:.55;";
  t.textContent = title;
  el.appendChild(t);
  const b = document.createElement("div");
  b.textContent = body;
  el.appendChild(b);
  document.body.appendChild(el);
  window.setTimeout(() => el.remove(), ttlMs);
}

// ---- W-116 眼罩运行态 ----

let maskEl: HTMLDivElement | null = null;
let maskState = false;

function maskRender(): void {
  if (typeof document === "undefined" || !flagOn("W-116")) return;
  if (!maskEl) {
    maskEl = document.createElement("div");
    maskEl.className = "nova-privacy-mask";
    maskEl.id = "nova-privacy-mask";
    const eye = document.createElement("div");
    eye.className = "eye";
    const badge = document.createElement("div");
    badge.className = "badge";
    maskEl.appendChild(eye);
    maskEl.appendChild(badge);
    maskEl.addEventListener("dblclick", () => {
      maskState = cameraMaskToggle(maskState);
      maskPaint();
      privacyEvent("camera-mask", { masked: maskState });
    });
    document.body.appendChild(maskEl);
  }
  maskPaint();
}

function maskPaint(): void {
  if (!maskEl) return;
  maskEl.dataset.masked = maskState ? "1" : "0";
  maskEl.dataset.motion = motionOK() && !maskState ? "1" : "0";
  const eye = maskEl.querySelector(".eye");
  const badge = maskEl.querySelector(".badge");
  if (eye) eye.textContent = maskState ? "🕶" : "👁";
  if (badge) badge.textContent = cameraBadgeText(maskState);
}

function maskRemove(): void {
  maskEl?.remove();
  maskEl = null;
  if (maskState) {
    maskState = false;
    privacyEvent("camera-mask", { masked: false });
  }
}

// ---- W-124 红线卡运行态 ----

function redlineShow(card: RedlineCard): void {
  if (typeof document === "undefined" || !flagOn("W-124")) return;
  const el = document.createElement("div");
  el.className = "nova-privacy-redline";
  const t = document.createElement("div");
  t.className = "t";
  t.textContent = "SECURITY REDLINE · 安全红线";
  el.appendChild(t);
  const b = document.createElement("div");
  b.textContent = redlineCardText(card);
  el.appendChild(b);
  el.addEventListener("click", () => el.remove());
  document.body.appendChild(el);
  redlineLog = redlineLogAppend(redlineLog, redlineSanitize(card));
  lsSet(`${NS}.redline-log`, redlineLog);
}

let redlineLog: RedlineLogEntry[] = lsGet<RedlineLogEntry[]>(`${NS}.redline-log`, []);

// ---- W-118 即焚封套运行态（仅内存） ----

let burnEnv: BurnEnv | null = null;

// ---- W-119 心跳墙数据（会话态） ----

let heartbeatRows: HeartbeatRow[] = [];

// ---- W-122 DNS 账本（持久，容量滚动） ----

let dnsLedger: DnsReq[] = lsGet<DnsReq[]>(`${NS}.dns-ledger`, []);

// ---- W-114 体检上次时间（持久） ----

let checkupLastAt: number | null = lsGet<number | null>(`${NS}.checkup-last`, null);

// ---- W-126 焚毁运行态 ----

let burnEl: HTMLDivElement | null = null;

function burnRun(targets: BurnTarget[]): void {
  if (typeof document === "undefined" || !flagOn("W-126")) return;
  const plan = burnPlan(targets);
  if (plan.length === 0) return;
  const charge = motionOK() ? BURN_CHARGE_MS : 0;
  if (charge > 0) {
    burnEl?.remove();
    const el = document.createElement("div");
    el.className = "nova-privacy-burn";
    const label = document.createElement("div");
    label.textContent = "BURN RITUAL";
    const gauge = document.createElement("div");
    gauge.className = "gauge";
    const fill = document.createElement("i");
    gauge.appendChild(fill);
    el.appendChild(label);
    el.appendChild(gauge);
    document.body.appendChild(el);
    burnEl = el;
    window.requestAnimationFrame(() => {
      fill.style.transitionDuration = `${BURN_CHARGE_MS}ms`;
      fill.style.width = "100%";
    });
  }
  window.setTimeout(() => {
    // 1) 剪贴板：尽力清空（浏览器能力内），并广播给宿主接系统 API
    if (plan.includes("clipboard")) {
      try {
        void navigator.clipboard?.writeText("").catch(() => undefined);
      } catch {
        /* 能力缺失：广播兜底 */
      }
      burnEnv = burnEnv && burnPurgeEnv(burnEnv) ? null : burnEnv;
      privacyEvent("burn-clear-clipboard", {});
    }
    // 2) 痕迹历史：本域全部持久键彻底清除
    if (plan.includes("history")) {
      for (const k of burnHistoryKeys()) lsDel(k);
      redlineLog = [];
      checkupLastAt = null;
    }
    // 3) DNS 账本
    if (plan.includes("ledger")) {
      dnsLedger = [];
    }
    privacyEvent("burn-done", { targets: plan, residue: burnResidue() });
    burnEl?.remove();
    burnEl = null;
  }, charge);
}

// ---- W-114 体检报告面板 ----

function checkupInputsNow(): CheckupInputs {
  const suspicious = dnsLedger.filter((r) => dnsAnnotate(r.domain).suspicious).length;
  return {
    burnLeftover: burnEnv && !burnEnv.consumed ? 1 : 0,
    sensitiveHits: 0, // 气泡即焚不落盘：体检只认会话内计数
    cameraLive: !maskState,
    dnsSuspicious: suspicious,
    clipboardPlain: 0,
    trustMin: 100,
  };
}

let sessionSensitiveHits = 0;
let sessionClipboardPlain = 0;

function panelShell(title: string): HTMLDivElement {
  closePanel();
  const el = document.createElement("div");
  el.className = "nova-privacy-panel";
  el.id = "nova-privacy-panel";
  const h = document.createElement("h3");
  h.textContent = title;
  el.appendChild(h);
  return el;
}

type PanelKind = "nova-checkup" | "nova-pulsewall" | "nova-dns";
let openPanel: PanelKind | null = null;

function closePanel(): void {
  document.getElementById("nova-privacy-panel")?.remove();
  openPanel = null;
}

function panelCheckup(): void {
  if (typeof document === "undefined") return;
  const el = panelShell("PRIVACY CHECKUP · 隐私体检报告");
  const inputs = checkupInputsNow();
  inputs.sensitiveHits = sessionSensitiveHits;
  inputs.clipboardPlain = sessionClipboardPlain;
  const r = checkupRun(inputs, Date.now());
  const grade = document.createElement("div");
  grade.className = `nova-privacy-grade-${r.grade.toLowerCase()}`;
  grade.textContent = checkupGradeText(r);
  el.appendChild(grade);
  for (const ax of r.axes) {
    const h = document.createElement("h3");
    h.textContent = `${ax.axis.toUpperCase()} ${ax.score}`;
    el.appendChild(h);
    const bar = document.createElement("div");
    bar.className = "bar";
    bar.innerHTML = `<i style="width:${ax.score}%"></i>`;
    el.appendChild(bar);
    for (const f of ax.fixes) {
      const p = document.createElement("div");
      p.textContent = `· 整改：${f}`;
      el.appendChild(p);
    }
  }
  const due = document.createElement("div");
  due.className = "muted";
  due.textContent = checkupDue(checkupLastAt, Date.now())
    ? "本次为到期体检；报告仅本地生成"
    : "距上次体检不足 90 天（面板直开不受限）";
  el.appendChild(due);
  checkupLastAt = Date.now();
  lsSet(`${NS}.checkup-last`, checkupLastAt);
  document.body.appendChild(el);
  openPanel = "nova-checkup";
}

function panelPulsewall(): void {
  if (typeof document === "undefined") return;
  const el = panelShell("HEARTBEAT WALL · 后台心跳墙");
  if (heartbeatRows.length === 0) {
    const p = document.createElement("div");
    p.className = "muted";
    p.textContent = "暂无采样 —— 心跳由 nova://privacy-heartbeat 摄入";
    el.appendChild(p);
  }
  for (const row of heartbeatRows) {
    const p = document.createElement("div");
    p.textContent = heartbeatRowText(row);
    if (row.level === 2) p.className = "nova-privacy-suspicious";
    el.appendChild(p);
  }
  document.body.appendChild(el);
  openPanel = "nova-pulsewall";
}

function panelDns(): void {
  if (typeof document === "undefined") return;
  const el = panelShell("DNS LEDGER · DNS 白话簿");
  if (dnsLedger.length === 0) {
    const p = document.createElement("div");
    p.className = "muted";
    p.textContent = "账本为空 —— 解析由 nova://privacy-dns 摄入（仅存域名）";
    el.appendChild(p);
  }
  for (const req of [...dnsLedger].reverse()) {
    const p = document.createElement("div");
    p.textContent = dnsRowText(req);
    if (dnsAnnotate(req.domain).suspicious) p.className = "nova-privacy-suspicious";
    el.appendChild(p);
  }
  document.body.appendChild(el);
  openPanel = "nova-dns";
}

// ---- W-115 密码三轨即时回显（外部算分摄入，明文不进本域） ----

function pwTracksShow(tracks: PasswordTracks): void {
  if (typeof document === "undefined" || !flagOn("W-115")) return;
  let box = document.getElementById("nova-privacy-pw");
  if (!box) {
    box = document.createElement("div");
    box.className = "nova-privacy-panel";
    box.id = "nova-privacy-pw";
    box.style.top = "auto";
    box.style.bottom = "120px";
    document.body.appendChild(box);
    window.setTimeout(() => box?.remove(), 8000);
  }
  box.innerHTML = "";
  const h = document.createElement("h3");
  h.textContent = `PASSWORD ENSEMBLE ${tracks.total}`;
  box.appendChild(h);
  const rows: Array<[string, number]> = [
    ["长度", tracks.length],
    ["字符种", tracks.variety],
    ["熵余量", tracks.entropy],
  ];
  for (const [name, v] of rows) {
    const row = document.createElement("div");
    row.className = "nova-privacy-track";
    const label = document.createElement("span");
    label.style.cssText = "width:52px;opacity:.7;";
    label.textContent = name;
    const bar = document.createElement("div");
    bar.className = "bar";
    bar.innerHTML = `<i style="width:${v}%"></i>`;
    const num = document.createElement("span");
    num.style.cssText = "width:28px;text-align:right;opacity:.7;";
    num.textContent = String(v);
    row.appendChild(label);
    row.appendChild(bar);
    row.appendChild(num);
    box.appendChild(row);
  }
}

// ---- 注册表订阅 shim（开关即时生效/失效） ----

function subscribeNovaShim(): () => void {
  return subscribeNova(() => {
    if (!flagOn("W-116")) maskRemove();
    else if (maskEl) maskPaint();
    if (openPanel && !flagOn(openPanel === "nova-checkup" ? "W-114" : openPanel === "nova-pulsewall" ? "W-119" : "W-122")) {
      closePanel();
    }
    const pwBox = document.getElementById("nova-privacy-pw");
    if (pwBox && !flagOn("W-115")) pwBox.remove();
  });
}

// ---------------------------------------------------------------------------
// 激活 / 卸载
// ---------------------------------------------------------------------------

let active = false;
let bag: Array<() => void> = [];

function resetVolatile(): void {
  burnEnv = null;
  heartbeatRows = [];
  sessionSensitiveHits = 0;
  sessionClipboardPlain = 0;
  maskRemove();
  burnEl?.remove();
  burnEl = null;
}

export function activatePrivacyNova(): void {
  if (active || typeof window === "undefined") return;
  active = true;
  ensureStyle();
  redlineLog = lsGet<RedlineLogEntry[]>(`${NS}.redline-log`, []);
  dnsLedger = lsGet<DnsReq[]>(`${NS}.dns-ledger`, []);
  checkupLastAt = lsGet<number | null>(`${NS}.checkup-last`, null);

  // W-115 密码三轨（外部算分摄入：{tracks} —— 明文永不进入本域）
  const onPw = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { tracks?: PasswordTracks } | undefined;
    if (!d?.tracks || typeof d.tracks.total !== "number") return;
    pwTracksShow(d.tracks);
  };
  window.addEventListener("nova://privacy-pw", onPw);
  bag.push(() => window.removeEventListener("nova://privacy-pw", onPw));

  // W-116 摄像头眼罩
  const onCamera = (ev: Event): void => {
    if (!flagOn("W-116")) return;
    const d = (ev as CustomEvent).detail as { active?: boolean } | undefined;
    if (d?.active === true) maskRender();
    else maskRemove();
  };
  window.addEventListener("nova://privacy-camera", onCamera);
  bag.push(() => window.removeEventListener("nova://privacy-camera", onCamera));

  // W-117 敏感文件气泡（Q-64 访问日志同源事件）
  const onAccess = (ev: Event): void => {
    if (!flagOn("W-117")) return;
    const d = (ev as CustomEvent).detail as { path?: string; app?: string } | undefined;
    if (!d?.path || !d.app) return;
    if (!isSensitivePath(d.path)) return;
    sessionSensitiveHits++;
    bubble(sensitiveBubbleText(d.path, d.app), SENSITIVE_BUBBLE_MS);
    privacyEvent("sensitive-hit", { path: d.path, app: d.app });
  };
  window.addEventListener("nova://privacy-access", onAccess);
  bag.push(() => window.removeEventListener("nova://privacy-access", onAccess));

  // W-118 阅后即焚
  const onBurnMark = (ev: Event): void => {
    if (!flagOn("W-118")) return;
    const d = (ev as CustomEvent).detail as { text?: string } | undefined;
    if (typeof d?.text !== "string" || d.text === "") return;
    burnEnv = burnMarkEnv(d.text, Date.now());
    bubble(`已标记${BURN_MARK_TEXT}：一次读取后自动焚毁`, 2400);
  };
  window.addEventListener("nova://privacy-burn-mark", onBurnMark);
  bag.push(() => window.removeEventListener("nova://privacy-burn-mark", onBurnMark));

  const onBurnRead = (): void => {
    if (!flagOn("W-118") || !burnEnv) return;
    const r = burnReadEnv(burnEnv);
    privacyEvent("burn-deliver", r ? { text: r.text, final: false } : { text: null, final: true });
  };
  window.addEventListener("nova://privacy-burn-read", onBurnRead);
  bag.push(() => window.removeEventListener("nova://privacy-burn-read", onBurnRead));

  // W-119 心跳墙
  const onHeartbeat = (ev: Event): void => {
    if (!flagOn("W-119")) return;
    const d = (ev as CustomEvent).detail as { apps?: HeartbeatApp[] } | undefined;
    if (!Array.isArray(d?.apps)) return;
    heartbeatRows = heartbeatWall(d.apps as HeartbeatApp[]);
    if (openPanel === "nova-pulsewall") panelPulsewall();
  };
  window.addEventListener("nova://privacy-heartbeat", onHeartbeat);
  bag.push(() => window.removeEventListener("nova://privacy-heartbeat", onHeartbeat));

  // W-120 信任衰减
  const onTrust = (ev: Event): void => {
    if (!flagOn("W-120")) return;
    const d = (ev as CustomEvent).detail as { plugin?: string; base?: number; incidents?: number; days?: number } | undefined;
    if (!d?.plugin || typeof d.base !== "number") return;
    const days = d.days ?? 0;
    const score = trustScore(d.base, days, d.incidents ?? 0);
    privacyEvent("trust-score", { plugin: d.plugin, score });
    if (trustReReview(score, days)) flashCard("TRUST REVIEW", trustReviewText(d.plugin, score, days), 6000);
  };
  window.addEventListener("nova://privacy-trust", onTrust);
  bag.push(() => window.removeEventListener("nova://privacy-trust", onTrust));

  // W-121 剧场（排演入口事件）
  const onTheater = (): void => {
    if (!flagOn("W-121")) return;
    const script = stageScript();
    const ok = script.every((s) => stageIngest(s) != null);
    if (!ok) return;
    const residue = stageDestroy(script);
    flashCard("LEAK THEATER", `${STAGE_DESTROY_NOTICE}（幕数 ${script.length}，残留 ${residue}）`, 7000);
    privacyEvent("theater-done", { scenes: script.length, residue });
  };
  window.addEventListener("nova://privacy-theater-run", onTheater);
  bag.push(() => window.removeEventListener("nova://privacy-theater-run", onTheater));

  // W-122 DNS 账本
  const onDns = (ev: Event): void => {
    if (!flagOn("W-122")) return;
    const d = (ev as CustomEvent).detail as { domain?: string; at?: number } | undefined;
    if (!d?.domain) return;
    const a = dnsAnnotate(d.domain);
    dnsLedger = dnsLedgerAppend(dnsLedger, { domain: a.domain, at: d.at ?? Date.now() });
    lsSet(`${NS}.dns-ledger`, dnsLedger);
    if (a.suspicious) bubble(`DNS 可疑：${a.domain} — ${a.reasons.join("；")}`, 3000);
  };
  window.addEventListener("nova://privacy-dns", onDns);
  bag.push(() => window.removeEventListener("nova://privacy-dns", onDns));

  // W-124 红线通知
  const onRedline = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { title?: string; body?: string; at?: number } | undefined;
    if (!d?.title) return;
    redlineShow({ title: d.title, body: d.body ?? "", at: d.at ?? Date.now() });
  };
  window.addEventListener("nova://privacy-redline", onRedline);
  bag.push(() => window.removeEventListener("nova://privacy-redline", onRedline));

  // W-125 权限族谱
  const onAcl = (ev: Event): void => {
    if (!flagOn("W-125")) return;
    const d = (ev as CustomEvent).detail as { path?: string; chain?: AclEntry[]; supported?: boolean } | undefined;
    if (!d?.path) return;
    const text = d.supported === true && Array.isArray(d.chain)
      ? aclChainText({ path: d.path, chain: d.chain })
      : aclUnsupportedText(d.path);
    flashCard("PERMISSION TREE", text, 6000);
  };
  window.addEventListener("nova://privacy-acl", onAcl);
  bag.push(() => window.removeEventListener("nova://privacy-acl", onAcl));

  // W-126 焚毁仪式
  const onBurn = (ev: Event): void => {
    const d = (ev as CustomEvent).detail as { targets?: BurnTarget[] } | undefined;
    burnRun(Array.isArray(d?.targets) && d.targets.length > 0 ? (d.targets as BurnTarget[]) : BURN_TARGET_ALL);
  };
  window.addEventListener("nova://privacy-burn", onBurn);
  bag.push(() => window.removeEventListener("nova://privacy-burn", onBurn));

  // Hub overlay 直达（ai04 协议：nova-checkup / nova-pulsewall / nova-dns）
  const onOpenFeature = (ev: Event): void => {
    const f = (ev as CustomEvent).detail?.feature;
    if (f === "nova-checkup" && flagOn("W-114")) (openPanel === "nova-checkup" ? closePanel : panelCheckup)();
    if (f === "nova-pulsewall" && flagOn("W-119")) (openPanel === "nova-pulsewall" ? closePanel : panelPulsewall)();
    if (f === "nova-dns" && flagOn("W-122")) (openPanel === "nova-dns" ? closePanel : panelDns)();
  };
  window.addEventListener("ai04:open-feature", onOpenFeature);
  bag.push(() => window.removeEventListener("ai04:open-feature", onOpenFeature));

  // S0 注册表变化 → 即时生效/失效
  bag.push(subscribeNovaShim());
}

export function deactivatePrivacyNova(): void {
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
  resetVolatile();
  closePanel();
  document.getElementById("nova-privacy-pw")?.remove();
  document
    .querySelectorAll(".nova-privacy-bubble,.nova-privacy-redline,.nova-privacy-mask,.nova-privacy-burn,.nova-privacy-panel")
    .forEach((e) => e.remove());
}

export function isPrivacyNovaActive(): boolean {
  return active;
}
