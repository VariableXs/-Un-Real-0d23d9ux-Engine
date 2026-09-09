/**
 * NOVA-200 · S1 启动剧场路（AI-01）—— 域1 启动与品牌剧场（W-001…W-012）。
 *
 * 边界（全景 §1）：U-01…U-06 冷启动剧场 / Q-01 唤醒握手 / Q-93 火焰图（事后诊断）
 * 为存量；本模块补**启动的物理感（制式）、预报性（ forecast 闭环）、
 * 情绪性（lastExit 差异化）与声音维（里程碑音/仪式音）**。
 *
 * 纪律：
 * - 零侵入：只读消费 S0 注册表（`../registry` 只读 API）与既有真实信号
 *   （boot_replay 事件、powerGateStore、perf_crash_dumps、availableMonitors），
 *   不改写任何既有组件内部逻辑，不碰 singularity 工件；
 * - 前缀：类名 `nova-`、事件 `nova://boot-*`、localStorage 键 `nova.boot.*`；
 * - 诚实：本模块由 NovaRuntime 在桌面 shell 挂载时激活（真实加载完成后），
 *   因此 W-002 的「第 1 秒预报」v0 呈现为 reveal 时刻的对账行（EXPECTED/ACTUAL/ERR），
 *   W-009 v0 以真实里程碑的收束复述呈现（数据来自 boot_replay，不虚构进度）；
 *   进入加载期的早挂载需 S17 在 App 层接线（见 BOOT_NOVA_FEATURES.wiringHint）；
 * - 降级：`[data-reduce-motion="true"]` 下点火退化为 120ms 亮度过渡、接力无交错、
 *   粒子/星点装饰静止；音频（W-009/W-010）为非运动产物不受运动降级钳制；
 * - 诚实自证：所有源流戳/情绪判定链可悬停查证（W-012），LEARNING 态不编造数字（W-002）。
 */

import {
  NOVA_FEATURES,
  novaBool,
  novaNum,
  novaOn,
  novaStr,
  subscribeNova,
  type NovaFeatureDef,
} from "../registry";
import { novaT } from "../labels";
import { powerGateStore } from "../../power/powerGate";

// ---------------------------------------------------------------------------
// 通用工具（本模块自持，不依赖 S0 行为工件）
// ---------------------------------------------------------------------------

export function clamp(v: number, min: number, max: number): number {
  return Math.min(Math.max(v, min), max);
}

/** reduce-motion 运行时标记（App 写 root.dataset.reduceMotion）→ 动效全关。 */
export function motionOK(): boolean {
  if (typeof document === "undefined") return false;
  return document.documentElement.dataset.reduceMotion !== "true";
}

/** localStorage 安全读（node 环境无 localStorage → fallback）。 */
function lsGet<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(`nova.boot.${key}`);
    if (raw == null) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

/** localStorage 安全写。 */
function lsSet(key: string, value: unknown): void {
  try {
    localStorage.setItem(`nova.boot.${key}`, JSON.stringify(value));
  } catch {
    /* 配额/隐私模式：不持久化，诚实跳过 */
  }
}

function lsDel(key: string): void {
  try {
    localStorage.removeItem(`nova.boot.${key}`);
  } catch {
    /* 尽力而为 */
  }
}

/** 派发 `nova://boot-*` 事件（SSR/测试环境安全）。 */
export function emitBoot(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://boot-${name}`, { detail }));
}

/** Rust 命令懒封装：任何失败如实返回 null（浏览器 dev / 命令缺失不编造）。 */
async function invoke<T>(cmd: string): Promise<T | null> {
  try {
    const mod = await import("@tauri-apps/api/core");
    return await mod.invoke<T>(cmd);
  } catch {
    return null;
  }
}

/** #nova-layer 宿主（nova.css §0 定义；多域模块共享，缺失时兜底创建）。 */
function novaLayer(): HTMLElement | null {
  if (typeof document === "undefined") return null;
  let el = document.getElementById("nova-layer");
  if (!el) {
    el = document.createElement("div");
    el.id = "nova-layer";
    document.body.appendChild(el);
  }
  return el;
}

function makeEl(className: string): HTMLDivElement {
  const el = document.createElement("div");
  el.className = className;
  return el;
}

/** WebAudio 单音（里程碑微音/仪式音共用；本地合成零素材，失败如实静默）。 */
function tone(freq: number, durMs: number, gain = 0.06, delayMs = 0): void {
  try {
    const AC = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AC) return;
    const ac = new AC();
    const osc = ac.createOscillator();
    const g = ac.createGain();
    osc.type = "sine";
    osc.frequency.value = freq;
    const t0 = ac.currentTime + delayMs / 1000;
    g.gain.setValueAtTime(0, t0);
    g.gain.linearRampToValueAtTime(gain, t0 + 0.008);
    g.gain.exponentialRampToValueAtTime(0.0001, t0 + durMs / 1000);
    osc.connect(g).connect(ac.destination);
    osc.start(t0);
    osc.stop(t0 + durMs / 1000 + 0.02);
    setTimeout(() => void ac.close(), delayMs + durMs + 120);
  } catch {
    /* 音频设备不可用：如实静默 */
  }
}

/** HUD 微行显示（工程腔自报，全英文纪律；文本型产物不设运动闸）。 */
function hudline(cls: string, text: string, ms: number): void {
  const layer = novaLayer();
  if (!layer) return;
  const el = makeEl(`nova-hudline ${cls} nova-show`);
  el.textContent = text;
  layer.appendChild(el);
  window.setTimeout(() => {
    el.classList.remove("nova-show");
    window.setTimeout(() => el.remove(), 700);
  }, ms);
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
  /** 需要 S17 在既有文件补接线的行为（本模块 API/事件已备好）。 */
  wiringHint?: string;
  /** reduce-motion / safeMode / static 降级说明。 */
  degrade: string;
}

export const BOOT_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-001",
    titleZh: "启动温度制式",
    titleEn: "Boot Temperature",
    descZh: "按会话来源区分点火制式：冷启熔炉升温 2.4s、快启余烬复燃 0.9s、会话恢复直接进入；检视版先押后 1.2s 再进剧场。",
    defaultOn: true,
    degrade: "reduce-motion 下退化为 120ms 亮度过渡（验收口径）",
  },
  {
    id: "W-002",
    titleZh: "启动天气预报",
    titleEn: "Boot Forecast",
    descZh: "以最近 20 次真实启动均值为基线预报，本次启动结束时回写误差（EXPECTED/ACTUAL/ERR 对账行）；样本 <5 次如实显示 LEARNING 不编造。",
    defaultOn: true,
    wiringHint: "v0 在 reveal 时刻对账；进入加载期第 1 秒显示需 S17 在 App 层早挂载（1 行）",
    degrade: "无动效，纯文本对账行",
  },
  {
    id: "W-003",
    titleZh: "跳映启动",
    titleEn: "Skip-Start",
    descZh: "启动中 Esc/空格只压缩纯视觉段，真实加载如实保留；桌面就绪后自报 SKIPPED CINEMATICS ONLY。",
    defaultOn: true,
    wiringHint: "自报行事件已备（nova://boot-skip-report）；BootScreen 跳过分支补 1 行 dispatch 即接通",
    degrade: "纯文本自报，无动效",
  },
  {
    id: "W-004",
    titleZh: "启动声场景感知",
    titleEn: "Sound Scene Sense",
    descZh: "启动音双因子自适应：上次会话末音量 ≤10% → 30% 轻响；23:00–06:00 + 深夜静默参数 → 静默改 60ms 微振视觉；本会话用户显式调大音量后不再干预。规则在 W-009/W-010 通道生效。",
    defaultOn: true,
    degrade: "纯规则引擎，无动效",
  },
  {
    id: "W-005",
    titleZh: "多屏顺次点火",
    titleEn: "Cascade Ignite",
    descZh: "多显示器时按真实屏幕清单交错点亮接力光幕（stagger 参数 0–400ms）；单屏环境零行为。",
    defaultOn: true,
    degrade: "reduce-motion 下取消交错（各屏同时点亮），装饰光幕关闭",
  },
  {
    id: "W-006",
    titleZh: "启动足迹墙",
    titleEn: "Boot Footprints",
    descZh: "最近 20 次启动每枚一行光点，耗时映射色温（快=冷白、慢=暖红）；清零按钮真实清空。数据同源 boot_replay 实测。",
    defaultOn: true,
    wiringHint: "阶段细分悬停待 boot 时间线持久化（S0.4 nova.rs 可补）；当前行内为总耗时+时刻",
    degrade: "无动效，纯面板",
  },
  {
    id: "W-007",
    titleZh: "下次启动预览",
    titleEn: "Next-Boot Preview",
    descZh: "关机/重启倒计时期间浮出预演卡：预估启动时长 + 上次实际值；无自启登记时如实显示 CLEAN BOOT。数据来自 powerGateStore 真实倒计时。",
    defaultOn: true,
    wiringHint: "自启清单源待 S0.4 nova_autostart_list；当前无登记源 → 一律 CLEAN BOOT（诚实）",
    degrade: "无动效，纯卡片",
  },
  {
    id: "W-008",
    titleZh: "品牌星座",
    titleEn: "Brand Constellation",
    descZh: "启动尾声品牌字标化作 8 枚可交互星点，点星展开对应本版能力（数据读 W 注册表 changelog 真源）；窗口期后归位。",
    defaultOn: true,
    degrade: "reduce-motion 下星点无浮动过渡，点击与卡片完整保留（交互非运动）",
  },
  {
    id: "W-009",
    titleZh: "启动里程碑微音",
    titleEn: "Milestone Notes",
    descZh: "25/50/75/100% 四个真实里程碑 → Do-Re-Mi-Sol 上行微音（各 90ms），为视觉不便用户「听进度」；v0 以真实里程碑收束复述呈现，早挂载后为实时触发。",
    defaultOn: true,
    wiringHint: "实时逐里程碑触发需 S17 App 层早挂载；boot://event 监听已备",
    degrade: "音频不受运动降级钳制；进度点脉冲视觉在 reduce-motion 下静止",
  },
  {
    id: "W-010",
    titleZh: "电源仪式音",
    titleEn: "Power Rite Music",
    descZh: "关机/重启倒计时执行瞬间播放 1.2s 下行收束告别音（弦乐收束/钟摆止息/静默三制式），与谢幕视觉事件同步。",
    defaultOn: true,
    wiringHint: "force/script 通道绕过倒计时门禁，仪式音不拦（明示通道不骚扰）",
    degrade: "静音设置/深夜静默下自动静默",
  },
  {
    id: "W-011",
    titleZh: "启动情绪测温",
    titleEn: "Exit Mood Thermometer",
    descZh: "按上次会话结束方式差异化本次启动：正常→标准；崩溃（crash dump 证实）→安抚版（素净灰白 + RECOVERED CLEANLY）；断电/未收尾→检视版（押后进剧场）。判定链真实可查。",
    defaultOn: true,
    degrade: "安抚版素净化不掩盖任何自检结果（仅观感层）",
  },
  {
    id: "W-012",
    titleZh: "启动源流戳",
    titleEn: "Startup Source Stamp",
    descZh: "桌面就绪后右下角 16px 源流戳 COLD/FAST/RESUME/RECOVER，悬停展开完整判定链（导航重载/干净退出心跳/存活心跳三条证据）；dev 模式常显，普通模式 2s 淡出。",
    defaultOn: true,
    degrade: "纯文本戳，无动效",
  },
];

export const bootNovaDomain = {
  id: "S1",
  nameZh: "启动剧场",
  nameEn: "Boot Theater",
  route: "AI-01",
  features: BOOT_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// 纯逻辑（可测）
// ---------------------------------------------------------------------------

// ---- W-012 源流戳 / W-001 制式（同一判定链） ----

export type BootSource = "COLD" | "FAST" | "RESUME" | "RECOVER";
export type IgnitionRegime = "furnace" | "ember" | "none";

/** 干净退出后的快速复燃认定窗口（窗口外退回 COLD）。 */
export const FAST_WINDOW_MS = 10 * 60_000;

export interface BootEvidence {
  /** 上次会话存活心跳（nova.boot.alive；无记录 = 首次运行）。 */
  aliveTs: number | null;
  /** 上次会话干净退出标记（nova.boot.exit；崩溃/断电不留下）。 */
  exitTs: number | null;
  /** 本次导航为 webview 重载（performance navigation type=reload → 会话恢复）。 */
  navReload: boolean;
  now: number;
}

/** 毫秒年龄 → 英文链用短文案（HUD 纪律全大写）。 */
export function fmtAge(ms: number): string {
  if (ms < 60_000) return `${Math.max(1, Math.round(ms / 1000))}S AGO`;
  if (ms < 3_600_000) return `${Math.round(ms / 60_000)}M AGO`;
  if (ms < 86_400_000) return `${Math.round(ms / 3_600_000)}H AGO`;
  return `${Math.round(ms / 86_400_000)}D AGO`;
}

/**
 * 源流判定链（互斥，顺序即优先级，全部真实证据）：
 * 1. webview 重载 → RESUME（同进程会话恢复）；
 * 2. 无任何历史 → COLD（首次运行）；
 * 3. 干净退出标记新鲜（≤10min）→ FAST（正常退出后快速复燃）；
 * 4. 有存活心跳但无干净退出标记 → RECOVER（上次会话未正常收尾：崩溃/断电）；
 * 5. 标记陈旧 → COLD。
 */
export function judgeBootSource(ev: BootEvidence): { source: BootSource; chain: string[] } {
  const chain: string[] = [`NAV RELOAD ${ev.navReload ? "YES" : "NO"}`];
  if (ev.navReload) {
    chain.push("→ RESUME");
    return { source: "RESUME", chain };
  }
  const { aliveTs, exitTs, now } = ev;
  if (aliveTs === null && exitTs === null) {
    chain.push("HISTORY NONE", "→ COLD");
    return { source: "COLD", chain };
  }
  if (exitTs !== null) {
    const age = now - exitTs;
    if (age <= FAST_WINDOW_MS) {
      chain.push(`CLEAN EXIT FRESH (${fmtAge(age)})`, "→ FAST");
      return { source: "FAST", chain };
    }
    chain.push(`CLEAN EXIT STALE (${fmtAge(age)})`, "→ COLD");
    return { source: "COLD", chain };
  }
  if (aliveTs === null) {
    chain.push("HISTORY NONE", "→ COLD");
    return { source: "COLD", chain };
  }
  // exitTs === null && aliveTs !== null
  chain.push(`LAST ALIVE ${fmtAge(now - aliveTs)}`, "CLEAN EXIT NONE", "→ RECOVER");
  return { source: "RECOVER", chain };
}

/** W-001 制式映射：冷启熔炉、快启余烬、会话恢复无点火；RECOVER 属真实冷启 → 熔炉（检视版押后）。 */
export function ignitionRegime(source: BootSource): IgnitionRegime {
  if (source === "COLD" || source === "RECOVER") return "furnace";
  if (source === "FAST") return "ember";
  return "none";
}

// ---- W-011 情绪测温 ----

export type BootMood = "standard" | "soothing" | "inspect";

/**
 * 情绪档：RECOVER 会话中，崩溃（crash dump 证实）→ 安抚版；
 * 断电/无法证实 → 检视版（先自检再剧场）；其余 → 标准版。
 * crashProven 为 null 表示探针不可用（浏览器 dev / IPC 失败）→ 保守取检视版。
 */
export function moodFromEvidence(source: BootSource, crashProven: boolean | null): BootMood {
  if (source !== "RECOVER") return "standard";
  return crashProven === true ? "soothing" : "inspect";
}

// ---- W-002 预报模型 ----

export const FORECAST_MIN_SAMPLES = 5;
export const FORECAST_MAX_SAMPLES = 20;

export interface ForecastModel {
  ms: number | null;
  learning: boolean;
  /** 是否叠加了磁盘繁忙项（探针不可用时不叠加且如实标注）。 */
  diskTerm: boolean;
}

/**
 * 预报 = 最近 20 次均值 × (1 + 磁盘偏移)。
 * 磁盘偏移 = (busy - 0.5) × 2 × (diskFactor/100) × 0.35（满因子 ±35%）；
 * busy=null（无探针）→ 不叠加；样本 <5 → learning（ms=null，不编造数字）。
 */
export function forecastBootMs(
  samplesMs: readonly number[],
  diskBusy01: number | null,
  diskFactorPct: number,
): ForecastModel {
  if (samplesMs.length < FORECAST_MIN_SAMPLES) {
    return { ms: null, learning: true, diskTerm: false };
  }
  const mean = samplesMs.reduce((a, b) => a + b, 0) / samplesMs.length;
  let factor = 1;
  const diskTerm = diskBusy01 !== null;
  if (diskTerm) {
    const w = clamp(diskFactorPct, 0, 100) / 100;
    factor += (clamp(diskBusy01, 0, 1) - 0.5) * 2 * w * 0.35;
  }
  return { ms: Math.round(mean * factor), learning: false, diskTerm };
}

/** 误差百分数（expected 为 null / actual 非法 → null）。 */
export function forecastErrorPct(expectedMs: number | null, actualMs: number): number | null {
  if (expectedMs === null || !(expectedMs > 0) || !(actualMs >= 0)) return null;
  return Math.round(((actualMs - expectedMs) / expectedMs) * 1000) / 10;
}

/** 对账行文案（HUD 英文纪律；样本 <5 如实 LEARNING）。 */
export function forecastLine(expectedMs: number | null, actualMs: number, sampleCount: number): string {
  const act = `${(actualMs / 1000).toFixed(1)}S`;
  if (expectedMs === null || sampleCount < FORECAST_MIN_SAMPLES) {
    return `EXPECTED LEARNING (${sampleCount}/${FORECAST_MIN_SAMPLES}) · ACTUAL ${act}`;
  }
  const err = forecastErrorPct(expectedMs, actualMs);
  const exp = `${(expectedMs / 1000).toFixed(1)}S`;
  return err === null ? `EXPECTED ${exp} · ACTUAL ${act}` : `EXPECTED ${exp} · ACTUAL ${act} · ERR ${err > 0 ? "+" : ""}${err}%`;
}

// ---- W-009 里程碑 ----

/** 四音上行（Do-Re-Mi-Sol，C5/D5/E5/G5，各 90ms）。 */
export const MILESTONE_FREQS = [523.25, 587.33, 659.25, 783.99] as const;
export const MILESTONE_CROSSING = [0.25, 0.5, 0.75, 1] as const;

export interface MilestoneHit {
  at: (typeof MILESTONE_CROSSING)[number];
  elapsedMs: number;
}

/**
 * 从真实 boot 事件流提取里程碑首跨时刻（单调进度，不虚构）。
 * progress ∈ 0..1（boot://event 口径），elapsedMs 为该事件实测耗时。
 */
export function milestoneTimeline(
  events: ReadonlyArray<{ progress: number; elapsedMs: number }>,
): MilestoneHit[] {
  const hits: MilestoneHit[] = [];
  for (const m of MILESTONE_CROSSING) {
    const first = events.find((e) => e.progress >= m);
    if (first) hits.push({ at: m, elapsedMs: first.elapsedMs });
  }
  return hits;
}

// ---- W-004 启动声场景规则 ----

export interface BootSoundCtx {
  /** 上次会话末音量（0–100；muted 视为 0；未知传 null）。 */
  lastVolumePct: number | null;
  hour: number;
  /** W-004 参数：深夜自动静默。 */
  nightSilent: boolean;
  /** 本会话用户显式调大过音量 → 不再干预。 */
  userRaised: boolean;
}

export interface BootSoundPlan {
  /** 0 = 静默；0.3 = 轻响；1 = 标准。 */
  gainScale: number;
  /** 深夜静默的 60ms 微振视觉替身。 */
  haptic: boolean;
  reason: string;
}

/** 双因子规则（可解释；userRaised 最高优先）。 */
export function bootSoundPlan(ctx: BootSoundCtx): BootSoundPlan {
  if (ctx.userRaised) return { gainScale: 1, haptic: false, reason: "USER OVERRIDE" };
  if (ctx.hour >= 23 || ctx.hour < 6) {
    if (ctx.nightSilent) return { gainScale: 0, haptic: true, reason: "NIGHT SILENCE" };
  }
  if (ctx.lastVolumePct !== null && ctx.lastVolumePct <= 10) {
    return { gainScale: 0.3, haptic: false, reason: "QUIET ENV" };
  }
  return { gainScale: 1, haptic: false, reason: "STANDARD" };
}

// ---- W-005 多屏接力 ----

/** 各屏点亮延迟序列：第 i 屏 i×stagger；单屏零行为（[0] 由调用方裁断）。 */
export function cascadeDelays(screenCount: number, staggerMs: number): number[] {
  const n = Math.max(0, Math.floor(screenCount));
  const s = clamp(staggerMs, 0, 400);
  return Array.from({ length: n }, (_, i) => i * s);
}

// ---- W-006 足迹墙 ----

export const FOOTPRINT_MAX = 20;

export interface BootFootprint {
  ts: number;
  ms: number;
}

/** 环形追加（最新在前，定长）。 */
export function pushFootprint<T extends { ts: number }>(list: readonly T[], rec: T, max = FOOTPRINT_MAX): T[] {
  return [rec, ...list].slice(0, max);
}

/**
 * 耗时 → 色温（快=冷白 hue≈250，慢=暖红 hue≈30）。
 * 以当次集合的最快/最慢为标尺线性映射（单样本 → 冷白）。
 */
export function hueFromDuration(ms: number, fastestMs: number, slowestMs: number): number {
  if (!(slowestMs > fastestMs)) return 250;
  const t = clamp((ms - fastestMs) / (slowestMs - fastestMs), 0, 1);
  return Math.round(250 - 220 * t);
}

// ---- W-007 下次启动预览 ----

/** 预演卡行（HUD 英文纪律；无自启登记 → CLEAN BOOT 如实）。 */
export function nextBootPreviewLines(
  autostartItems: readonly string[],
  forecastMs: number | null,
  lastActualMs: number | null,
): string[] {
  const lines: string[] = [];
  lines.push(
    autostartItems.length === 0
      ? "AUTOSTART · CLEAN BOOT"
      : `AUTOSTART · ${autostartItems.length} APP${autostartItems.length > 1 ? "S" : ""}`,
  );
  for (const it of autostartItems.slice(0, 4)) lines.push(`· ${it}`);
  if (autostartItems.length > 4) lines.push(`… +${autostartItems.length - 4}`);
  lines.push(forecastMs === null ? "EXPECTED LEARNING" : `EXPECTED ${(forecastMs / 1000).toFixed(1)}S`);
  lines.push(lastActualMs === null ? "LAST —" : `LAST ${(lastActualMs / 1000).toFixed(1)}S`);
  return lines;
}

// ---- W-008 品牌星座 ----

/** 星点确定性布局（横跨字标区的正弦弧；百分比坐标）。 */
export function constellationPoints(n: number): Array<{ xPct: number; yPct: number }> {
  const count = clamp(Math.floor(n), 1, 24);
  const pts: Array<{ xPct: number; yPct: number }> = [];
  for (let i = 0; i < count; i++) {
    const t = count === 1 ? 0.5 : i / (count - 1);
    pts.push({ xPct: 14 + 72 * t, yPct: 40 + Math.sin(t * Math.PI * 2) * 7 });
  }
  return pts;
}

/** 星点数据源：从 200 项注册表等距采样（全域覆盖、确定性、逐项带 changelog 真源）。 */
export function pickConstellationFeatures(n: number): NovaFeatureDef[] {
  const count = clamp(Math.floor(n), 1, NOVA_FEATURES.length);
  const step = NOVA_FEATURES.length / count;
  const out: NovaFeatureDef[] = [];
  for (let i = 0; i < count; i++) out.push(NOVA_FEATURES[Math.floor(i * step)]!);
  return out;
}

// ---- W-010 电源仪式音 ----

export type RiteStyle = "strings" | "pendulum" | "silence";

export interface RiteTone {
  freq: number;
  durMs: number;
  delayMs: number;
}

/** 1.2s 下行收束：弦乐三音叠置 / 钟摆双脉冲止息 / 静默空序列。 */
export function riteToneSeq(style: RiteStyle): RiteTone[] {
  if (style === "strings") {
    return [
      { freq: 440, durMs: 420, delayMs: 0 }, // A4
      { freq: 329.63, durMs: 460, delayMs: 280 }, // E4
      { freq: 220, durMs: 520, delayMs: 560 }, // A3
    ];
  }
  if (style === "pendulum") {
    return [
      { freq: 330, durMs: 520, delayMs: 0 },
      { freq: 247.5, durMs: 620, delayMs: 540 },
    ];
  }
  return [];
}

export function parseRiteStyle(raw: string): RiteStyle {
  return raw === "pendulum" || raw === "silence" ? raw : "strings";
}

// ---------------------------------------------------------------------------
// 行为层（激活/卸载幂等；所有产物挂 #nova-layer 或 <html> dataset）
// ---------------------------------------------------------------------------

interface ForecastStore {
  samplesMs: number[];
  lastForecastMs: number | null;
  errs: number[];
}

interface BootSession {
  source: BootSource;
  mood: BootMood;
  regime: IgnitionRegime;
  chain: string[];
  startedAt: number;
}

let active = false;
let bag: Array<() => void> = [];
let session: BootSession | null = null;
let heartbeatTimer = 0;
let ritePlayedAt = 0;
let previewOpen = false;
let firedAction: "shutdown" | "reboot" | null = null;
let lastRemain = 0;

/** webview 重载检测（performance navigation；不可用环境如实 false）。 */
function navIsReload(): boolean {
  try {
    const nav = performance.getEntriesByType("navigation")[0] as PerformanceNavigationTiming | undefined;
    return nav?.type === "reload";
  } catch {
    return false;
  }
}

function writeCleanExit(): void {
  lsSet("exit", { ts: Date.now(), kind: "clean" });
}

/** 崩溃证实探针：数据目录 crash dump 时间戳晚于上次心跳 → 崩溃成立。 */
async function probeCrashAfterDeath(aliveTs: number | null): Promise<boolean | null> {
  if (aliveTs === null) return false;
  const dumps = await invoke<Array<{ file: string; ts: number }>>("perf_crash_dumps");
  if (!dumps) return null; // 探针不可用（诚实：交由调用方保守处理）
  return dumps.some((d) => d.ts >= aliveTs - 5000);
}

/** 上次会话末音量（settings 真源；不可用 → null）。 */
async function lastVolumePct(): Promise<number | null> {
  try {
    const { loadSettings } = await import("../../../lib/settings");
    const s = await loadSettings();
    if (!s) return null;
    return s.soundMuted ? 0 : Math.round(clamp(s.soundVolume, 0, 1) * 100);
  } catch {
    return null;
  }
}

/** 当前声音计划（W-004 规则引擎的真实输入组装）。 */
async function currentSoundPlan(): Promise<BootSoundPlan> {
  return bootSoundPlan({
    lastVolumePct: await lastVolumePct(),
    hour: new Date().getHours(),
    nightSilent: novaBool("W-004", "nightSilent"),
    userRaised: false, // v0：启动时刻尚无用户交互；会话内干预为后续接线
  });
}

// ---- W-001 点火制式 ----

function playIgnition(regime: IgnitionRegime, mood: BootMood): void {
  const layer = novaLayer();
  if (!layer) return;
  const html = document.documentElement;
  const run = (): void => {
    html.dataset.novaRegime = regime;
    emitBoot("ignite", { regime, mood });
    if (!motionOK()) {
      // 降级：120ms 亮度过渡（验收口径；内联样式覆盖 CSS 动画）
      const veil = makeEl("nova-ignite-veil");
      veil.style.animation = "none";
      veil.style.transition = "opacity 120ms linear";
      veil.style.opacity = "0.45";
      layer.appendChild(veil);
      window.setTimeout(() => (veil.style.opacity = "0"), 130);
      window.setTimeout(() => veil.remove(), 500);
      return;
    }
    const veil = makeEl("nova-ignite-veil");
    layer.appendChild(veil);
    window.setTimeout(() => veil.remove(), regime === "furnace" ? 2600 : 1200);
  };
  // 检视版：先自检再剧场（押后 1.2s；自检本体属 V-59 存量，不越界代跑）
  if (mood === "inspect") {
    hudline("nova-inspect-line", "SELF-CHECK PENDING · THEATER HELD", 4000);
    window.setTimeout(run, 1200);
    return;
  }
  run();
}

// ---- W-012 源流戳 ----

function mountStamp(source: BootSource, chain: string[]): void {
  const layer = novaLayer();
  if (!layer) return;
  const html = document.documentElement;
  const isDev = typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
  if (isDev) html.dataset.novaDev = "true";
  const stamp = makeEl("nova-stamp");
  stamp.setAttribute("role", "note");
  const label = document.createElement("span");
  label.textContent = source;
  const chainEl = makeEl("nova-stamp-chain");
  chainEl.textContent = chain.join("\n");
  stamp.append(label, chainEl);
  layer.appendChild(stamp);
  emitBoot("stamp", { source });
  if (!isDev) {
    window.setTimeout(() => {
      stamp.style.transition = "opacity 600ms var(--ease-standard)";
      stamp.style.opacity = "0";
      window.setTimeout(() => stamp.remove(), 700);
    }, 2000);
  }
}

// ---- W-002 预报对账 + W-006 足迹 + W-009 里程碑（boot_replay 真源处理） ----

interface BootEventLite {
  progress: number;
  elapsedMs: number;
}

async function processBootTimeline(): Promise<void> {
  const events = await invoke<BootEventLite[]>("boot_replay");
  if (!events || events.length === 0) return; // 浏览器 dev / 无后端：如实无对账
  const actualMs = events.reduce((mx, e) => Math.max(mx, e.elapsedMs), 0);
  const now = Date.now();

  // W-006 足迹（数据侧无条件记录，展示由开关控制）
  const fps = lsGet<BootFootprint[]>("footprints", []);
  lsSet("footprints", pushFootprint(fps, { ts: now, ms: actualMs }));

  // W-002 预报对账闭环
  if (novaOn("W-002")) {
    const fc = lsGet<ForecastStore>("forecast", { samplesMs: [], lastForecastMs: null, errs: [] });
    const expected = fc.lastForecastMs;
    const sampleCount = fc.samplesMs.length;
    const errPct = forecastErrorPct(expected, actualMs);
    if (errPct !== null) {
      fc.errs = [...fc.errs, errPct].slice(-FOOTPRINT_MAX);
    }
    fc.samplesMs = [...fc.samplesMs, actualMs].slice(-FORECAST_MAX_SAMPLES);
    fc.lastForecastMs = forecastBootMs(fc.samplesMs, null, novaNum("W-002", "diskFactor")).ms;
    lsSet("forecast", fc);
    hudline("nova-forecast", forecastLine(expected, actualMs, sampleCount), 5000);
    emitBoot("forecast", { expected, actualMs, errPct });
  }

  // W-009 里程碑（真实首跨时刻 → 收束复述；早挂载后为实时触发）
  if (novaOn("W-009")) {
    const hits = milestoneTimeline(events);
    if (hits.length > 0) void playMilestoneRecap(hits.length, actualMs);
  }
}

async function playMilestoneRecap(count: number, actualMs: number): Promise<void> {
  const plan = await currentSoundPlan();
  hudline("nova-milestones", `MILESTONES ${count}/4 · ${(actualMs / 1000).toFixed(1)}S`, 4000);
  for (let i = 0; i < count; i++) {
    const f = MILESTONE_FREQS[i];
    if (!f) continue;
    if (plan.gainScale > 0) tone(f, 90, 0.06 * plan.gainScale, i * 110);
  }
  if (motionOK()) {
    const layer = novaLayer();
    if (!layer) return;
    for (let i = 0; i < count; i++) {
      window.setTimeout(() => {
        const pip = makeEl("nova-milestone-pip");
        pip.style.left = `${(i + 1) * 25}%`;
        pip.style.bottom = "20%";
        layer.appendChild(pip);
        window.setTimeout(() => pip.remove(), 500);
      }, i * 120);
    }
  }
  emitBoot("milestones", { count, actualMs, reason: plan.reason });
}

// ---- W-005 多屏接力 ----

async function playCascade(): Promise<void> {
  if (!novaOn("W-005")) return;
  try {
    const win = await import("@tauri-apps/api/window");
    const list = await win.availableMonitors();
    if (!list || list.length <= 1) return; // 单屏零行为（验收）
    if (!motionOK()) return; // 降级：同时点亮（真实挂载即点亮），装饰光幕关闭
    const delays = cascadeDelays(list.length, novaNum("W-005", "staggerMs"));
    let idx = 0;
    try {
      const cur = await win.currentMonitor();
      if (cur) idx = Math.max(0, list.findIndex((m) => m.name === cur.name));
    } catch {
      idx = 0;
    }
    window.setTimeout(() => {
      const layer = novaLayer();
      if (!layer) return;
      const glow = makeEl("nova-cascade nova-ignite");
      layer.appendChild(glow);
      window.setTimeout(() => glow.remove(), 560);
      emitBoot("cascade", { screens: list.length, delay: delays[idx] ?? 0 });
    }, delays[idx] ?? 0);
  } catch {
    /* 浏览器 dev / 屏幕枚举失败：如实零行为 */
  }
}

// ---- W-008 品牌星座 ----

async function scheduleConstellation(): Promise<void> {
  if (!novaOn("W-008")) return;
  try {
    const { loadSettings } = await import("../../../lib/settings");
    const s = await loadSettings();
    if (s?.bootAnim === "none") return; // 验收：bootAnim=none 时不出现
  } catch {
    /* 设置不可读：不因判断缺失而跳过（保守展示） */
  }
  window.setTimeout(() => {
    if (novaOn("W-008")) mountConstellation();
  }, 1500);
}

function mountConstellation(): void {
  const layer = novaLayer();
  if (!layer || layer.querySelector(".nova-constellation")) return;
  const box = makeEl("nova-constellation");
  const feats = pickConstellationFeatures(8);
  const pts = constellationPoints(feats.length);

  // 连星细线（SVG，纯装饰；soothing 情绪下由 CSS 整体抑制）
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("class", "nova-constellation-links");
  svg.setAttribute("viewBox", "0 0 100 100");
  svg.setAttribute("preserveAspectRatio", "none");
  const poly = document.createElementNS(NS, "polyline");
  poly.setAttribute("points", pts.map((p) => `${p.xPct},${p.yPct}`).join(" "));
  poly.setAttribute("fill", "none");
  poly.setAttribute("stroke", "var(--accent)");
  poly.setAttribute("stroke-width", "0.15");
  svg.appendChild(poly);
  box.appendChild(svg);

  let card: HTMLDivElement | null = null;
  const closeCard = (): void => {
    card?.remove();
    card = null;
    box.querySelector(".nova-star-active")?.classList.remove("nova-star-active");
  };

  feats.forEach((f, i) => {
    const pt = pts[i];
    if (!pt) return;
    const star = makeEl("nova-star");
    star.style.left = `${pt.xPct}%`;
    star.style.top = `${pt.yPct}%`;
    star.setAttribute("role", "button");
    star.setAttribute("tabindex", "0");
    star.setAttribute("aria-label", `${f.id} ${novaT(f.id, "zh")}`);
    const open = (): void => {
      closeCard();
      star.classList.add("nova-star-active");
      card = makeEl("nova-star-card");
      card.style.left = `${clamp(pt.xPct, 4, 74)}%`;
      card.style.top = `${clamp(pt.yPct + 5, 8, 82)}%`;
      const id = makeEl("nova-star-id");
      id.textContent = f.id;
      const body = document.createElement("div");
      body.textContent = `${novaT(f.id, "zh")} — ${f.changelog}`;
      card.append(id, body);
      box.appendChild(card);
      emitBoot("constellation-open", { id: f.id });
    };
    star.addEventListener("click", open);
    star.addEventListener("keydown", (e) => {
      if ((e as KeyboardEvent).key === "Enter") open();
    });
    box.appendChild(star);
  });

  layer.appendChild(box);
  emitBoot("constellation", { stars: feats.length });

  const windowSec = novaNum("W-008", "windowSec");
  const onKey = (e: Event): void => {
    if ((e as KeyboardEvent).key === "Escape") {
      window.clearTimeout(dismiss);
      box.remove();
      window.removeEventListener("keydown", onKey);
    }
  };
  const dismiss = window.setTimeout(() => {
    box.remove();
    window.removeEventListener("keydown", onKey);
  }, windowSec * 1000);
  window.addEventListener("keydown", onKey);
  bag.push(() => {
    window.clearTimeout(dismiss);
    window.removeEventListener("keydown", onKey);
    box.remove();
  });
}

// ---- W-006 足迹墙面板（overlay：nova-footprints） ----

function toggleFootprints(force?: boolean): void {
  const layer = novaLayer();
  if (!layer) return;
  const existing = layer.querySelector(".nova-footprints");
  if (existing && force !== true) {
    existing.remove();
    return;
  }
  if (existing) existing.remove();
  const panel = makeEl("nova-panel nova-footprints");
  panel.style.right = "16px";
  panel.style.bottom = "56px";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", novaT("W-006", "zh"));

  const title = makeEl("nova-panel-title");
  title.textContent = `W-006 BOOT FOOTPRINTS · LAST ${FOOTPRINT_MAX}`;
  panel.appendChild(title);

  const fps = lsGet<BootFootprint[]>("footprints", []);
  if (fps.length === 0) {
    const empty = makeEl("nova-empty");
    empty.textContent = "NO BOOT RECORDS YET";
    panel.appendChild(empty);
  } else {
    const fastest = Math.min(...fps.map((f) => f.ms));
    const slowest = Math.max(...fps.map((f) => f.ms));
    for (const f of fps) {
      const row = makeEl("nova-footprint-row");
      const dot = makeEl("nova-footprint-dot");
      dot.style.setProperty("--nova-hue", String(hueFromDuration(f.ms, fastest, slowest)));
      const msEl = document.createElement("span");
      msEl.className = "nova-footprint-ms";
      msEl.textContent = `${(f.ms / 1000).toFixed(1)}S · ${new Date(f.ts).toLocaleString()}`;
      row.append(dot, msEl);
      panel.appendChild(row);
    }
  }

  const clear = document.createElement("button");
  clear.type = "button";
  clear.className = "nova-btn";
  clear.textContent = "CLEAR";
  clear.addEventListener("click", () => {
    lsDel("footprints");
    toggleFootprints(true); // 真实清空并重渲染
  });
  panel.appendChild(clear);
  layer.appendChild(panel);
  emitBoot("footprints", { count: fps.length });
}

// ---- W-007 预演卡 + W-010 仪式音（powerGate 真实倒计时联动） ----

function openNextBootPreview(): void {
  const layer = novaLayer();
  if (!layer || previewOpen) return;
  previewOpen = true;
  const panel = makeEl("nova-panel nova-nextboot");
  panel.style.right = "16px";
  panel.style.bottom = "56px";
  panel.setAttribute("role", "dialog");
  panel.setAttribute("aria-label", novaT("W-007", "zh"));
  const title = makeEl("nova-panel-title");
  title.textContent = "W-007 NEXT BOOT";
  panel.appendChild(title);
  const fc = lsGet<ForecastStore>("forecast", { samplesMs: [], lastForecastMs: null, errs: [] });
  const fps = lsGet<BootFootprint[]>("footprints", []);
  // 自启登记源未接入（S0.4 nova_autostart_list）→ 一律 CLEAN BOOT（诚实）
  for (const line of nextBootPreviewLines([], fc.lastForecastMs, fps[0]?.ms ?? null)) {
    const row = makeEl("nova-hint");
    row.style.font = "500 11px/1.6 var(--font-mono, Consolas, monospace)";
    row.textContent = line;
    panel.appendChild(row);
  }
  layer.appendChild(panel);
  emitBoot("nextboot-preview", {});
}

function closeNextBootPreview(): void {
  document.querySelector("#nova-layer .nova-nextboot")?.remove();
  previewOpen = false;
}

function onPowerGate(): void {
  const p = powerGateStore.getState().pending;
  if (p) {
    lastRemain = p.remainSec;
    firedAction = p.action;
    if (novaOn("W-007")) openNextBootPreview();
    return;
  }
  // pending 结束：remainSec ≤ 1 → 真实执行（rite + 干净退出标记）；>1 → 用户取消
  const action = firedAction;
  const executed = lastRemain <= 1 && action !== null;
  closeNextBootPreview();
  if (action !== null && executed) {
    writeCleanExit();
    if (novaOn("W-010")) void playRite(action);
  }
  firedAction = null;
  lastRemain = 0;
}

async function playRite(action: "shutdown" | "reboot"): Promise<void> {
  if (Date.now() - ritePlayedAt < 4000) return; // 5s 内至多一次（singu 谢幕事件不叠加）
  ritePlayedAt = Date.now();
  const style = parseRiteStyle(novaStr("W-010", "rite"));
  const plan = await currentSoundPlan();
  const scale = plan.gainScale;
  for (const t of riteToneSeq(style)) {
    if (scale > 0) tone(t.freq, t.durMs, 0.07 * scale, t.delayMs);
  }
  if (plan.haptic) hudline("nova-rite-haptic", "·", 60); // 60ms 微振视觉替身
  emitBoot("rite", { action, style, reason: plan.reason });
}

// ---- W-011 情绪产物 ----

function mountMoodLine(mood: BootMood): void {
  if (mood === "standard") return;
  if (mood === "soothing") {
    hudline("nova-recovered-line", "RECOVERED CLEANLY", 4000);
  }
  // inspect 行由 playIgnition 的押后路径呈现（SELF-CHECK PENDING）
}

// ---- 注册表联动（全关 → 零常驻开销） ----

function bootAnyOn(): boolean {
  return NOVA_FEATURES.some((f) => f.domain === "boot" && novaOn(f.id));
}

function teardownArtifacts(): void {
  delete document.documentElement.dataset.novaRegime;
  delete document.documentElement.dataset.novaMood;
  document.querySelector("#nova-layer .nova-ignite-veil")?.remove();
  document.querySelector("#nova-layer .nova-stamp")?.remove();
  document.querySelector("#nova-layer .nova-constellation")?.remove();
  document.querySelector("#nova-layer .nova-footprints")?.remove();
  document.querySelector("#nova-layer .nova-nextboot")?.remove();
  previewOpen = false;
}

/**
 * 激活（幂等）。NovaRuntime 于桌面 shell 挂载时调用一次；
 * 证据采集 → 源流/情绪判定 → 制式点火 → 对账/接力/星座 → 电源流联动。
 */
export function activateBootNova(): void {
  if (active || typeof document === "undefined") return;
  active = true;
  bag = [];

  // 存活心跳（15s；RECOVER/源流判定的真实证据）
  lsSet("alive", Date.now());
  heartbeatTimer = window.setInterval(() => lsSet("alive", Date.now()), 15_000);
  bag.push(() => window.clearInterval(heartbeatTimer));

  // 干净退出标记（pagehide 覆盖刷新/关窗；powerGate 执行路径另写）
  const onHide = (): void => writeCleanExit();
  window.addEventListener("pagehide", onHide);
  bag.push(() => window.removeEventListener("pagehide", onHide));

  // 注册表联动
  bag.push(
    subscribeNova(() => {
      if (!bootAnyOn()) teardownArtifacts();
    }),
  );

  // overlay 事件（Hub 触发足迹墙）
  const onFootprints = (): void => {
    if (novaOn("W-006")) toggleFootprints();
  };
  window.addEventListener("nova://boot-footprints", onFootprints);
  bag.push(() => window.removeEventListener("nova://boot-footprints", onFootprints));

  // 与 Q-02 谢幕视觉同步的仪式音（singu/ai04 双通道只听不改）
  const onCurtain = (): void => {
    if (novaOn("W-010")) void playRite("shutdown");
  };
  window.addEventListener("singu:curtain-call", onCurtain);
  window.addEventListener("ai04:curtain-call", onCurtain);
  bag.push(() => window.removeEventListener("singu:curtain-call", onCurtain));
  bag.push(() => window.removeEventListener("ai04:curtain-call", onCurtain));

  // W-003 自报行事件（S17 接线 BootScreen 后即生效；本模块只呈现不拦键）
  const onSkipReport = (): void => {
    if (!novaOn("W-003")) return;
    hudline("nova-skip-report", "SKIPPED CINEMATICS ONLY", 4200);
  };
  window.addEventListener("nova://boot-skip-report", onSkipReport);
  bag.push(() => window.removeEventListener("nova://boot-skip-report", onSkipReport));

  // 电源流联动（W-007/W-010）
  bag.push(powerGateStore.subscribe(onPowerGate));

  // 主流程：证据 → 判定 → 剧场
  void (async () => {
    const evidence: BootEvidence = {
      aliveTs: lsGet<number | null>("alive", null),
      exitTs: lsGet<{ ts: number } | null>("exit", null)?.ts ?? null,
      navReload: navIsReload(),
      now: Date.now(),
    };
    // 判定先读上次心跳，再落本次首个心跳（顺序不可颠倒，否则 RECOVER 永不可判）
    lsSet("alive", Date.now());
    const { source, chain } = judgeBootSource(evidence);
    const regime = ignitionRegime(source);
    const crashProven = source === "RECOVER" ? await probeCrashAfterDeath(evidence.aliveTs) : null;
    const mood = moodFromEvidence(source, crashProven);
    session = { source, mood, regime, chain, startedAt: Date.now() };

    if (mood !== "standard") document.documentElement.dataset.novaMood = mood;
    if (novaOn("W-011")) mountMoodLine(mood);
    if (novaOn("W-001") && regime !== "none") playIgnition(regime, mood);
    if (novaOn("W-012")) mountStamp(source, chain);
    void processBootTimeline();
    void playCascade();
    void scheduleConstellation();
  })();
}

/** 卸载（测试/热重载用）：回收全部监听与产物。 */
export function deactivateBootNova(): void {
  if (!active) return;
  active = false;
  for (const off of bag.splice(0)) {
    try {
      off();
    } catch {
      /* 卸载尽力而为 */
    }
  }
  if (typeof document !== "undefined") teardownArtifacts();
  session = null;
  firedAction = null;
  lastRemain = 0;
  ritePlayedAt = 0;
}

export function isBootNovaActive(): boolean {
  return active;
}

/** 当前会话判定结果（hub/测试观察用；未激活 → null）。 */
export function bootSessionInfo(): BootSession | null {
  return session;
}
