/**
 * NOVA-200 · S14 无障碍扩展路（AI-14）—— 域14 无障碍与本地化（W-164…W-175）。
 *
 * 边界（全景 §14）：novaVoice 是 S0 语音件（W-164/170/174 只经 `nova://a11y-speak`
 * 事件消费，未装语音件时事件无人消费即静默，如实降级）；U-12 响应式网格承接
 * W-165 的重排放大（本域只发布 CSS 变量与 dataset，不越界改布局组件）；Z-14
 * 方案库接收 W-171 镜像生成结果（事件投递，入库与否由 Z-14 决定）。
 *
 * 纪律：
 * - 零侵入：不改写任何既有组件/设置/ singularity 工件；全部为 DOM 叠层 +
 *   dataset/CSS 变量 + `nova://a11y-*` 自定义事件摄入；
 * - 前缀：类名 `nova-`、事件 `nova://a11y-*`、localStorage 键 `nova.a11y.*`；
 * - 开关：只读消费 S0 注册表（novaOn），无注册表即不激活；
 * - 降级：reduce-motion / safeMode / static 下闪光/字幕浮现/下划线动效归零
 *   （语义保留：转译仍转译、报数仍报数、改写仍改写）；
 * - 诚实（分工图验收）：
 *   · 声令 20 条封闭语法全部可枚举（封闭集，不猜开放语句）；
 *   · 150% 放大给出溢出守卫与最小命中区，不承诺"零溢出"于未接线的组件；
 *   · 色觉改写后逐对校验对比度，守恒失败如实报 UNCONSERVED；
 *   · 拼音注音仅覆盖内置字符表，未收录字如实标注（不编造读音）；
 *   · 盲文默认关闭按需开；Grade-1 逐字符映射，无缩写。
 */

import { novaMotionOK, novaOn, subscribeNova } from "../registry";

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

const NS = "nova.a11y";

/** 功能开关：只读消费 S0 注册表。 */
export function flagOn(id: string): boolean {
  return novaOn(id);
}

/** 派发 `nova://a11y-*` 事件（SSR/测试环境安全）。 */
export function a11yEvent(name: string, detail?: unknown): void {
  if (typeof window === "undefined" || typeof CustomEvent === "undefined") return;
  window.dispatchEvent(new CustomEvent(`nova://a11y-${name}`, { detail }));
}

// ---------------------------------------------------------------------------
// Hub 注册清单（功能卡 + 参数 + 降级说明）
// ---------------------------------------------------------------------------

export interface NovaFeatureCard {
  id: string;
  titleZh: string;
  titleEn: string;
  descZh: string;
  defaultOn: boolean;
  /** 需要 S17 在既有文件补接线的行为（本模块 API/事件已备好）。 */
  wiringHint?: string;
  degrade: string;
}

export const A11Y_NOVA_FEATURES: NovaFeatureCard[] = [
  {
    id: "W-164",
    titleZh: "声令官",
    titleEn: "Voice Commander",
    descZh: "20 条封闭语法离线声令（opt-in）：按住说话→文本进封闭集匹配，命中即派发动作事件；封闭集全部可枚举，未命中如实报 NO MATCH 不猜意图。",
    defaultOn: false,
    wiringHint: "语音件文本派发 nova://a11y-voice {text}；动作消费 nova://a11y-voice-exec",
    degrade: "纯匹配逻辑，无动效可降级；语音件未装时入口如实隐藏",
  },
  {
    id: "W-165",
    titleZh: "放大重锤",
    titleEn: "Magnifier Hammer",
    descZh: "布局级重排放大：110–200% 缩放经 CSS 变量注入布局网格（U-12 承接），自动抬升最小命中区并给出列数收缩与溢出守卫建议。",
    defaultOn: false,
    wiringHint: "U-12 网格读取 html dataset novaA11yZoom 与 --nova-a11y-scale 重排",
    degrade: "放大与命中区抬升语义不变（无动效成分）",
  },
  {
    id: "W-166",
    titleZh: "色觉配方改写",
    titleEn: "Color Recipe Rewrite",
    descZh: "色觉自测（protan/deutan/tritan）后对主题源配方逐 token 改写：Machado 矩阵模拟 + 明度校正，逐对校验对比度守恒，失败如实报 UNCONSERVED。",
    defaultOn: false,
    wiringHint: "主题 token 表派发 nova://a11y-color-tokens {tokens, pairs}；改写结果回 nova://a11y-color-rewritten",
    degrade: "纯计算，无动效可降级",
  },
  {
    id: "W-167",
    titleZh: "听障听诊器",
    titleEn: "Deaf Stethoscope",
    descZh: "30+ 系统声事件三通道转译：视觉闪标（屏角语义徽标）+ 字幕条（人话描述）+ 触觉振动模式；三通道可独立开关，事件来源为真实声事件摄入。",
    defaultOn: true,
    wiringHint: "系统声派发 nova://a11y-sound {id}（与声域同源事件）；转译层已备好",
    degrade: "静态徽标与字幕（无浮现动效）；三通道语义不变",
  },
  {
    id: "W-168",
    titleZh: "长者大卡",
    titleEn: "Elder Cards",
    descZh: "1.3× 大卡套装：卡片/按钮 1.3 倍、最小命中区抬至 36px、危险操作双确认（二次点击确认条）；不动字号渲染管线，只注入缩放变量与确认闸。",
    defaultOn: false,
    wiringHint: "确认闸消费 nova://a11y-confirm-gate 事件（危险动作派发方接入）",
    degrade: "静态确认条（无脉冲动效）；双确认语义不变",
  },
  {
    id: "W-169",
    titleZh: "语法温柔墙",
    titleEn: "Gentle Grammar Wall",
    descZh: "中英 50+ 规则就地检查：下划线标记 + 温柔建议（不说'错误'，说'可以这样更好'）；全部本地规则零网络，建议只标位置不改写原文。",
    defaultOn: true,
    wiringHint: "编辑器文本派发 nova://a11y-grammar {text}；结果回 nova://a11y-grammar-result",
    degrade: "静态下划线（无描线动画）；建议语义不变",
  },
  {
    id: "W-170",
    titleZh: "数字报数",
    titleEn: "Number Speech",
    descZh: "金额/日期/电话三制式 TTS 分节播报：识别数字上下文（¥/日期/11 位电话）→ 逐节规范化（三五〇不是三百五十的号码）→ nova://a11y-speak 交给语音件。",
    defaultOn: false,
    wiringHint: "选区/焦点数字派发 nova://a11y-number {text}；语音件消费 nova://a11y-speak",
    degrade: "纯分段计算；语音件未装时事件无人消费即静默",
  },
  {
    id: "W-171",
    titleZh: "左右手镜像",
    titleEn: "Hand Mirror",
    descZh: "键位方案中心对称翻转生成：按键盘几何逐行镜像（q↔p…）+ 左右方向键反转，生成结果投递 Z-14 方案库入库，原方案永不覆盖。",
    defaultOn: true,
    wiringHint: "现有键位方案派发 nova://a11y-mirror-src {scheme}；镜像结果回 nova://a11y-mirror-src-result 并投递 Z-14",
    degrade: "纯映射，无动效可降级",
  },
  {
    id: "W-172",
    titleZh: "字体栈医生",
    titleEn: "Font Stack Doctor",
    descZh: "界面字体栈逐级体检：缺失字体、无泛型回退、重复项、跨语言缺口（中文字体缺失致豆腐块）四类诊断 + 修复建议栈；本地字体探测诚实上报。",
    defaultOn: true,
    wiringHint: "主题字体栈派发 nova://a11y-fontstack {stack}；体检卡已备好",
    degrade: "静态诊断卡；探测失败如实标 UNKNOWN",
  },
  {
    id: "W-173",
    titleZh: "零术语词典",
    titleEn: "Plain Dictionary",
    descZh: "内置白话术语词典：悬停/选中术语就地弹白话卡（不说'进程'，说'正在跑的程序'）；Ctrl+Alt+D 开关，命中计数本地滚动，零网络。",
    defaultOn: true,
    wiringHint: "术语命中卡片已备好；编辑器/设置页 hover 接线由 S17 补",
    degrade: "静态卡片（无浮入动效）；词典语义不变",
  },
  {
    id: "W-174",
    titleZh: "拼音北极星",
    titleEn: "Pinyin North Star",
    descZh: "界面中文拼音上标（ruby）+ 悬停朗读：仅标注内置字符表内的字，未收录字如实不加注（绝不编造读音）；悬停派发 nova://a11y-speak 交语音件。",
    defaultOn: false,
    wiringHint: "ruby 渲染走 annotateRuby() API；悬停朗读消费 nova://a11y-speak",
    degrade: "静态 ruby 上标（无浮现动效）；注音语义不变",
  },
  {
    id: "W-175",
    titleZh: "盲文徽章",
    titleEn: "Braille Badges",
    descZh: "ARIA 语义 + Unicode 盲文双通道层：Grade-1 逐字符盲文徽章（默认关闭按需开），徽章同时携带 aria-label 与盲文点阵，读屏与盲文显示双通道可达。",
    defaultOn: false,
    wiringHint: "徽章挂载点派发 nova://a11y-badge {label}；aria 接线由 S17 按组件补",
    degrade: "纯文本变换，无动效可降级",
  },
];

export const a11yNovaDomain = {
  id: "S14",
  nameZh: "无障碍扩展",
  nameEn: "A11y & Locale",
  route: "AI-14",
  features: A11Y_NOVA_FEATURES,
} as const;

// ---------------------------------------------------------------------------
// W-164 声令官（20 条封闭语法）
// ---------------------------------------------------------------------------

export interface VoiceCommand {
  id: string;
  /** 封闭语法（zh/en 各至少其一，全小写；匹配前做归一化）。 */
  patterns: string[];
  /** 动作名（消费方按 action 分发）。 */
  action: string;
  /** 是否带槽位参数。 */
  slot?: "number" | "app" | "dir";
}

export const VOICE_COMMANDS: VoiceCommand[] = [
  { id: "vc-open-start", patterns: ["打开开始菜单", "open start menu"], action: "dock:open-start" },
  { id: "vc-open-search", patterns: ["打开搜索", "open search"], action: "dock:open-search" },
  { id: "vc-show-desktop", patterns: ["显示桌面", "show desktop"], action: "windows:show-desktop" },
  { id: "vc-minimize-all", patterns: ["最小化全部窗口", "minimize all windows"], action: "windows:minimize-all" },
  { id: "vc-snap-left", patterns: ["窗口靠左", "snap window left"], action: "windows:snap-left" },
  { id: "vc-snap-right", patterns: ["窗口靠右", "snap window right"], action: "windows:snap-right" },
  { id: "vc-snap-max", patterns: ["窗口最大化", "maximize window"], action: "windows:snap-max" },
  { id: "vc-close-window", patterns: ["关闭当前窗口", "close current window"], action: "windows:close" },
  { id: "vc-switch-desktop", patterns: ["切换桌面", "switch desktop"], action: "windows:switch-desktop" },
  { id: "vc-open-files", patterns: ["打开文件管理器", "open file manager"], action: "files:open" },
  { id: "vc-new-folder", patterns: ["新建文件夹", "new folder"], action: "files:new-folder" },
  { id: "vc-open-settings", patterns: ["打开设置", "open settings"], action: "system:open-settings" },
  { id: "vc-toggle-quiet", patterns: ["进入安静档", "quiet mode on"], action: "vision:quiet-mode" },
  { id: "vc-do-not-disturb", patterns: ["开启勿扰", "do not disturb"], action: "sound:dnr-on" },
  { id: "vc-dnd-off", patterns: ["关闭勿扰", "do not disturb off"], action: "sound:dnr-off" },
  { id: "vc-read-screen", patterns: ["读一下屏幕", "read the screen"], action: "a11y:read-screen" },
  { id: "vc-open-hub", patterns: ["打开新星面板", "open nova hub"], action: "nova:open-hub" },
  { id: "vc-launch-app", patterns: ["启动", "launch"], action: "app:launch", slot: "app" },
  { id: "vc-volume-set", patterns: ["音量调到", "set volume to"], action: "sound:volume-set", slot: "number" },
  { id: "vc-scroll-dir", patterns: ["滚到顶部", "滚到底部", "scroll to top", "scroll to bottom"], action: "scroll:to", slot: "dir" },
];

export interface VoiceMatch {
  id: string;
  action: string;
  slot?: string;
  /** 归一化后的原始命中文本。 */
  hit: string;
}

/** 文本归一化：全角→半角、去标点、小写、压空白。 */
export function normalizeVoiceText(text: string): string {
  return text
    .replace(/[，。！？、：；]/g, " ")
    .replace(/[！？]/g, " ")
    .toLowerCase()
    .replace(/\s+/g, " ")
    .trim();
}

/** 封闭语法匹配：未命中返回 null（诚实不猜）。 */
export function matchVoiceCommand(raw: string): VoiceMatch | null {
  const text = normalizeVoiceText(raw);
  if (!text) return null;
  for (const cmd of VOICE_COMMANDS) {
    for (const p of cmd.patterns) {
      if (cmd.slot) {
        if (!text.startsWith(p)) continue;
        const rest = text.slice(p.length).trim();
        if (!rest) return null; // 有槽位语法但缺参数：不命中
        return { id: cmd.id, action: cmd.action, slot: rest, hit: text };
      }
      if (text === p) return { id: cmd.id, action: cmd.action, hit: text };
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// W-165 放大重锤（布局级重排放大）
// ---------------------------------------------------------------------------

export interface ZoomPlan {
  scale: number;
  /** 最小命中区（px）：28px × scale 下限 28。 */
  hitMinPx: number;
  /** 建议最大列数（12 列基准收缩）。 */
  maxCols: number;
  /** 溢出守卫：>140% 需要单列回退预案。 */
  spillGuard: boolean;
}

export const ZOOM_MIN = 110;
export const ZOOM_MAX = 200;

export function zoomPlan(pct: number): ZoomPlan {
  const scale = clamp(pct, ZOOM_MIN, ZOOM_MAX) / 100;
  return {
    scale,
    hitMinPx: Math.max(28, Math.round(28 * scale)),
    maxCols: Math.max(2, Math.floor(12 / scale)),
    spillGuard: scale > 1.4,
  };
}

// ---------------------------------------------------------------------------
// W-166 色觉配方改写
// ---------------------------------------------------------------------------

export type CvdKind = "protanopia" | "deuteranopia" | "tritanopia";

/** Machado et al. 2009 严重度 1.0 矩阵（RGB 0–1 线性域近似 sRGB）。 */
export const CVD_MATRIX: Record<CvdKind, number[]> = {
  protanopia: [0.152286, 1.052583, -0.204868, 0.114503, 0.786281, 0.099216, -0.003882, -0.048116, 1.051998],
  deuteranopia: [0.367322, 0.860646, -0.227968, 0.280085, 0.672501, 0.047413, -0.01182, 0.04294, 0.968881],
  tritanopia: [1.255528, -0.076749, -0.178779, -0.078411, 0.930809, 0.147602, 0.004733, 0.691367, 0.3039],
};

export function hexToRgb(hex: string): [number, number, number] | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1]!, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export function rgbToHex(r: number, g: number, b: number): string {
  const c = (v: number) => clamp(Math.round(v), 0, 255).toString(16).padStart(2, "0");
  return `#${c(r)}${c(g)}${c(b)}`;
}

/** CVD 模拟（线性 RGB 近似域）。 */
export function simulateCvd(hex: string, kind: CvdKind): string | null {
  const rgb = hexToRgb(hex);
  if (!rgb) return null;
  const m = CVD_MATRIX[kind];
  const lin = rgb.map((v) => v / 255);
  const out = [
    m[0]! * lin[0]! + m[1]! * lin[1]! + m[2]! * lin[2]!,
    m[3]! * lin[0]! + m[4]! * lin[1]! + m[5]! * lin[2]!,
    m[6]! * lin[0]! + m[7]! * lin[1]! + m[8]! * lin[2]!,
  ];
  return rgbToHex(out[0]! * 255, out[1]! * 255, out[2]! * 255);
}

/** 相对亮度（WCAG）。 */
export function luminance(hex: string): number | null {
  const rgb = hexToRgb(hex);
  if (!rgb) return null;
  const [r, g, b] = rgb.map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
}

export function contrastRatio(a: string, b: string): number | null {
  const la = luminance(a);
  const lb = luminance(b);
  if (la == null || lb == null) return null;
  const hi = Math.max(la, lb);
  const lo = Math.min(la, lb);
  return (hi + 0.05) / (lo + 0.05);
}

/** 混入黑/白调明度（保色相的大致近似：线性插值到白/黑）。 */
function lighten(hex: string, t: number): string | null {
  const rgb = hexToRgb(hex);
  if (!rgb) return null;
  return rgbToHex(...rgb.map((v) => v + (255 - v) * t) as [number, number, number]);
}
function darken(hex: string, t: number): string | null {
  const rgb = hexToRgb(hex);
  if (!rgb) return null;
  return rgbToHex(...rgb.map((v) => v * (1 - t)) as [number, number, number]);
}

export type RewriteStatus = "ok" | "unconserved";

export interface ColorRewriteResult {
  tokens: Record<string, string>;
  /** 逐对校验：[fgToken, bgToken, before, after, status]。 */
  pairs: Array<{ fg: string; bg: string; before: number; after: number; status: "ok" | "unconserved" }>;
  status: RewriteStatus;
}

/**
 * 源配方改写：token 表 → CVD 模拟 → 逐对对比度校验，不达 4.5 即对前景做
 * 明度迭代校正（最多 8 步）；仍不达标的对如实标 UNCONSERVED。
 */
export function rewriteColorRecipe(
  tokens: Record<string, string>,
  pairs: Array<[string, string]>,
  kind: CvdKind,
): ColorRewriteResult | null {
  const sim: Record<string, string> = {};
  for (const [k, v] of Object.entries(tokens)) {
    const s = simulateCvd(v, kind);
    if (!s) return null;
    sim[k] = s;
  }
  const out: Record<string, string> = { ...sim };
  const pairResults: ColorRewriteResult["pairs"] = [];
  let anyUnconserved = false;
  for (const [fg, bg] of pairs) {
    const beforeRaw = contrastRatio(tokens[fg] ?? "", tokens[bg] ?? "");
    if (beforeRaw == null) return null;
    const target = Math.max(4.5, Math.min(beforeRaw, 21));
    let after = contrastRatio(out[fg] ?? "", out[bg] ?? "") ?? 0;
    let steps = 0;
    while (after < 4.5 && steps < 8) {
      const cur = hexToRgb(out[fg] ?? "");
      if (!cur) break;
      const mid = (cur[0]! + cur[1]! + cur[2]!) / 3;
      const adjusted = mid >= 128 ? darken(out[fg] ?? "", 0.12) : lighten(out[fg] ?? "", 0.12);
      if (!adjusted) break;
      out[fg] = adjusted;
      after = contrastRatio(out[fg] ?? "", out[bg] ?? "") ?? 0;
      steps++;
    }
    const conserved = after >= target * 0.9;
    if (!conserved) anyUnconserved = true;
    pairResults.push({ fg, bg, before: beforeRaw, after, status: conserved ? "ok" : "unconserved" });
  }
  return { tokens: out, pairs: pairResults, status: anyUnconserved ? "unconserved" : "ok" };
}

// ---------------------------------------------------------------------------
// W-167 听障听诊器（30+ 声事件三通道转译）
// ---------------------------------------------------------------------------

export type SoundSeverity = "info" | "warn" | "alert";

export interface SoundEventDef {
  id: string;
  captionZh: string;
  severity: SoundSeverity;
  /** 触觉振动模式（ms on/off 序列）。 */
  vibr: number[];
}

/** 30+ 真实声事件（与声域/系统声同源枚举）。 */
export const SOUND_EVENTS: SoundEventDef[] = [
  { id: "boot.ready", captionZh: "系统已就绪", severity: "info", vibr: [40] },
  { id: "boot.milestone", captionZh: "启动进度到里程碑", severity: "info", vibr: [30] },
  { id: "notify.msg", captionZh: "来了一条消息", severity: "info", vibr: [60, 40, 60] },
  { id: "notify.mention", captionZh: "有人提到了你", severity: "warn", vibr: [80, 40, 80, 40, 80] },
  { id: "notify.silent", captionZh: "勿扰期通知已雨化", severity: "info", vibr: [20] },
  { id: "battery.low", captionZh: "电量不足", severity: "warn", vibr: [100, 60, 100] },
  { id: "battery.full", captionZh: "已充满", severity: "info", vibr: [40, 30, 40] },
  { id: "power.plug", captionZh: "已接通电源", severity: "info", vibr: [50] },
  { id: "power.unplug", captionZh: "已断开电源", severity: "warn", vibr: [50, 50, 50] },
  { id: "usb.connect", captionZh: "USB 设备已接入", severity: "info", vibr: [40] },
  { id: "usb.disconnect", captionZh: "USB 设备已拔出", severity: "info", vibr: [30] },
  { id: "disk.error", captionZh: "磁盘读写出错", severity: "alert", vibr: [120, 60, 120, 60, 120] },
  { id: "disk.full", captionZh: "磁盘空间将满", severity: "warn", vibr: [90, 50, 90] },
  { id: "mic.on", captionZh: "麦克风已开启", severity: "info", vibr: [60] },
  { id: "mic.off", captionZh: "麦克风已关闭", severity: "info", vibr: [30] },
  { id: "camera.on", captionZh: "摄像头已开启", severity: "warn", vibr: [70, 40, 70] },
  { id: "camera.off", captionZh: "摄像头已关闭", severity: "info", vibr: [30] },
  { id: "error.generic", captionZh: "出错了，请查看提示", severity: "alert", vibr: [150] },
  { id: "error.crash", captionZh: "有程序崩溃了", severity: "alert", vibr: [160, 60, 160] },
  { id: "update.ready", captionZh: "更新已就绪", severity: "info", vibr: [40, 30, 40] },
  { id: "update.done", captionZh: "更新完成", severity: "info", vibr: [40, 20, 40, 20, 60] },
  { id: "download.done", captionZh: "下载完成", severity: "info", vibr: [50, 30, 50] },
  { id: "download.fail", captionZh: "下载失败", severity: "warn", vibr: [100, 40, 100] },
  { id: "transfer.done", captionZh: "传输完成", severity: "info", vibr: [45, 25, 45] },
  { id: "timer.done", captionZh: "计时器到点", severity: "warn", vibr: [80, 40, 80] },
  { id: "meeting.soon", captionZh: "会议快开始了", severity: "warn", vibr: [70, 30, 70, 30, 70] },
  { id: "trash.empty", captionZh: "回收站已清空", severity: "info", vibr: [40] },
  { id: "print.done", captionZh: "打印完成", severity: "info", vibr: [50] },
  { id: "print.fail", captionZh: "打印失败", severity: "warn", vibr: [100, 50, 100] },
  { id: "security.alert", captionZh: "安全提醒，请立即查看", severity: "alert", vibr: [180, 60, 180, 60, 180] },
  { id: "clock.hour", captionZh: "整点报时", severity: "info", vibr: [25] },
  { id: "keyboard.caps", captionZh: "大写锁定已切换", severity: "info", vibr: [30] },
  { id: "volume.max", captionZh: "音量已达最大", severity: "info", vibr: [35] },
  { id: "device.connect.bt", captionZh: "蓝牙设备已连接", severity: "info", vibr: [45, 25, 45] },
  { id: "device.disconnect.bt", captionZh: "蓝牙设备已断开", severity: "warn", vibr: [60, 30, 60] },
];

export type DeafChannel = "flash" | "caption" | "vibr";

export interface DeafTranslation {
  id: string;
  captionZh: string;
  severity: SoundSeverity;
  channels: { flash: boolean; caption: boolean; vibr: number[] | null };
}

/** 三通道转译（通道开关可独立配置；未收录事件如实返回 null）。 */
export function translateSound(
  eventId: string,
  enabled: Record<DeafChannel, boolean> = { flash: true, caption: true, vibr: true },
): DeafTranslation | null {
  const def = SOUND_EVENTS.find((s) => s.id === eventId);
  if (!def) return null;
  return {
    id: def.id,
    captionZh: def.captionZh,
    severity: def.severity,
    channels: {
      flash: enabled.flash,
      caption: enabled.caption,
      vibr: enabled.vibr ? [...def.vibr] : null,
    },
  };
}

// ---------------------------------------------------------------------------
// W-168 长者大卡
// ---------------------------------------------------------------------------

export const ELDER_SCALE = 1.3;
export const ELDER_CONFIRMATIONS = 2;

export interface ElderPlan {
  cardScale: number;
  confirmations: number;
  hitMinPx: number;
}

export function elderPlan(): ElderPlan {
  return {
    cardScale: ELDER_SCALE,
    confirmations: ELDER_CONFIRMATIONS,
    hitMinPx: Math.max(36, Math.round(28 * ELDER_SCALE)),
  };
}

/** 双确认闸状态机：第一次点击 → 待确认；第二次点击内确认通过。 */
export function confirmGate(state: { armed: boolean; at: number }, now: number, windowMs = 4000): { armed: boolean; pass: boolean } {
  if (!state.armed) return { armed: true, pass: false };
  if (now - state.at > windowMs) return { armed: true, pass: false }; // 过期重来
  return { armed: false, pass: true };
}

// ---------------------------------------------------------------------------
// W-169 语法温柔墙（中英 50+ 规则）
// ---------------------------------------------------------------------------

export type GrammarLang = "zh" | "en";

export interface GrammarRule {
  id: string;
  lang: GrammarLang;
  re: RegExp;
  /** 温柔建议（不说"错误"）。 */
  hint: string;
  /** 建议替换（可为 null = 仅提示）。 */
  fix: string | null;
}

/** 52 条内置规则（30 zh + 22 en）。 */
export const GRAMMAR_RULES: GrammarRule[] = [
  // ---- zh 30 条 ----
  { id: "zh-dedup-de", lang: "zh", re: /的的/g, hint: "连续两个「的」读起来有点急", fix: "的" },
  { id: "zh-dedup-le", lang: "zh", re: /了了/g, hint: "连续两个「了」可以合并", fix: "了" },
  { id: "zh-dedup-shi", lang: "zh", re: /是是/g, hint: "重复的「是」可以去掉一个", fix: "是" },
  { id: "zh-space-mix", lang: "zh", re: /[\u4e00-\u9fff] +[\u4e00-\u9fff]/g, hint: "汉字之间的半角空格多数场合可以去掉", fix: null },
  { id: "zh-comma-half", lang: "zh", re: /[\u4e00-\u9fff],[\u4e00-\u9fff]/g, hint: "中文语境用全角逗号更顺眼", fix: null },
  { id: "zh-period-half", lang: "zh", re: /[\u4e00-\u9fff]\.[\u4e00-\u9fff]/g, hint: "中文句末用「。」更自然", fix: null },
  { id: "zh-question-mark", lang: "zh", re: /[\u4e00-\u9fff]\?/g, hint: "中文问句用「？」", fix: null },
  { id: "zh-exclaim", lang: "zh", re: /[\u4e00-\u9fff]!/g, hint: "中文感叹用「！」", fix: null },
  { id: "zh-zhen-de-hen", lang: "zh", re: /真的很?非常/g, hint: "「真的」「非常」叠用语气过强", fix: null },
  { id: "zh-shi-de-hua", lang: "zh", re: /如果…?的话的话/g, hint: "「的话」重复了", fix: "的话" },
  { id: "zh-double-neg", lang: "zh", re: /不得不没有/g, hint: "双重否定读三遍都不顺", fix: null },
  { id: "zh-yin-wei-suo-yi", lang: "zh", re: /因为.{1,24}所以/g, hint: "「因为…所以…」成对出现有时显得啰嗦", fix: null },
  { id: "zh-ju-ran", lang: "zh", re: /居然竟然/g, hint: "「居然」「竟然」选一个就好", fix: null },
  { id: "zh-ge-zhong", lang: "zh", re: /各种各样的各种/g, hint: "「各种」叠用", fix: null },
  { id: "zh-fei-chang-de", lang: "zh", re: /非常的/g, hint: "「的」在这里可以省", fix: "非常" },
  { id: "zh-bi-jiao-lai", lang: "zh", re: /比较来说/g, hint: "「比较来说」可简化为「比较」", fix: "比较" },
  { id: "zh-jin-xing", lang: "zh", re: /进行了?一次(查看|讨论|修改)/g, hint: "「进行」+ 名词常可直接用动词", fix: null },
  { id: "zh-suo-yi", lang: "zh", re: /所以说说/g, hint: "「所以」后面的「说说」多半是口误", fix: null },
  { id: "zh-neng-gou", lang: "zh", re: /能够够/g, hint: "「能够」叠字", fix: "能够" },
  { id: "zh-quote-full", lang: "zh", re: /[\u4e00-\u9fff]"[^"]*"[\u4e00-\u9fff]/g, hint: "中文语境建议用「」直角引号", fix: null },
  { id: "zh-dun-hao", lang: "zh", re: /[\u4e00-\u9fff]、[\u4e00-\u9fff]、[\u4e00-\u9fff]、/g, hint: "顿号超过三个可考虑改用「以及」收尾", fix: null },
  { id: "zh-nin", lang: "zh", re: /您们/g, hint: "「您们」一般写作「各位」或「你们」", fix: null },
  { id: "zh-ou-men", lang: "zh", re: /偶们/g, hint: "网络写法在正式场合建议用「我们」", fix: "我们" },
  { id: "zh-shen-mei-le", lang: "zh", re: /美美哒/g, hint: "口语化表达，正式场合可换「很美」", fix: null },
  { id: "zh-666", lang: "zh", re: /666/g, hint: "数字俚语在正式文本可换成「很棒」", fix: null },
  { id: "zh-repeat-3", lang: "zh", re: /([\u4e00-\u9fff])\1\1/g, hint: "同一个字连写三次多半是手滑", fix: null },
  { id: "zh-de-di", lang: "zh", re: /的地得?得地/g, hint: "「的/地/得」混用连写", fix: null },
  { id: "zh-quan-jiao", lang: "zh", re: /（（/g, hint: "括号重复", fix: "（" },
  { id: "zh-ellipsis", lang: "zh", re: /\.\.\.\./g, hint: "中文省略号建议用「……」", fix: null },
  { id: "zh-weile", lang: "zh", re: /为了了/g, hint: "「为了」叠字", fix: "为了" },
  // ---- en 22 条 ----
  { id: "en-the-the", lang: "en", re: /\bthe the\b/gi, hint: "duplicated \"the\"", fix: "the" },
  { id: "en-a-a", lang: "en", re: /\ba a\b/gi, hint: "duplicated \"a\"", fix: "a" },
  { id: "en-and-and", lang: "en", re: /\band and\b/gi, hint: "duplicated \"and\"", fix: "and" },
  { id: "en-to-to", lang: "en", re: /\bto to\b/gi, hint: "duplicated \"to\"", fix: "to" },
  { id: "en-of-of", lang: "en", re: /\bof of\b/gi, hint: "duplicated \"of\"", fix: "of" },
  { id: "en-is-is", lang: "en", re: /\bis is\b/gi, hint: "duplicated \"is\"", fix: "is" },
  { id: "en-i-lower", lang: "en", re: /(^|\s)i(\s|')/g, hint: "\"i\" is usually capitalized", fix: null },
  { id: "en-double-space", lang: "en", re: /[a-z]  +[a-z]/gi, hint: "double space between words", fix: null },
  { id: "en-space-before-punct", lang: "en", re: /\s+[,.;:!?]/g, hint: "no space before punctuation", fix: null },
  { id: "en-its-vs-it-is", lang: "en", re: /\bits a\b/gi, hint: "did you mean \"it's a\"?", fix: null },
  { id: "en-your-vs-youre", lang: "en", re: /\byour welcome\b/gi, hint: "usually written \"you're welcome\"", fix: null },
  { id: "en-thier", lang: "en", re: /\bthier\b/gi, hint: "common misspelling of \"their\"", fix: "their" },
  { id: "en-recieve", lang: "en", re: /\brecieve\b/gi, hint: "usually spelled \"receive\"", fix: "receive" },
  { id: "en-definately", lang: "en", re: /\bdefinately\b/gi, hint: "usually spelled \"definitely\"", fix: "definitely" },
  { id: "en-seperate", lang: "en", re: /\bseperate\b/gi, hint: "usually spelled \"separate\"", fix: "separate" },
  { id: "en-alot", lang: "en", re: /\balot\b/gi, hint: "usually written \"a lot\"", fix: "a lot" },
  { id: "en-could-of", lang: "en", re: /\bcould of\b/gi, hint: "usually \"could have\"", fix: null },
  { id: "en-should-of", lang: "en", re: /\bshould of\b/gi, hint: "usually \"should have\"", fix: null },
  { id: "en-would-of", lang: "en", re: /\bwould of\b/gi, hint: "usually \"would have\"", fix: null },
  { id: "en-exclaim-3", lang: "en", re: /!{3,}/g, hint: "three or more exclamation marks rarely help", fix: null },
  { id: "en-caps-shout", lang: "en", re: /\b[A-Z]{5,}\b/g, hint: "all caps reads like shouting", fix: null },
  { id: "en-then-than", lang: "en", re: /\bmore better then\b/gi, hint: "comparison uses \"than\"", fix: null },
];

export interface GrammarIssue {
  ruleId: string;
  lang: GrammarLang;
  index: number;
  match: string;
  hint: string;
  fix: string | null;
}

/** 就地检查：只标位置给建议，不改写原文（温柔墙语义）。 */
export function checkGrammar(text: string, lang?: GrammarLang): GrammarIssue[] {
  const out: GrammarIssue[] = [];
  for (const rule of GRAMMAR_RULES) {
    if (lang && rule.lang !== lang) continue;
    rule.re.lastIndex = 0;
    let m: RegExpExecArray | null;
    let guard = 0;
    while ((m = rule.re.exec(text)) && guard++ < 50) {
      out.push({ ruleId: rule.id, lang: rule.lang, index: m.index, match: m[0], hint: rule.hint, fix: rule.fix });
      if (m[0].length === 0) rule.re.lastIndex++;
    }
  }
  return out.sort((a, b) => a.index - b.index);
}

// ---------------------------------------------------------------------------
// W-170 数字报数（三制式分节）
// ---------------------------------------------------------------------------

export type NumberKind = "currency" | "date" | "phone" | "plain";

export interface NumberSegment {
  kind: NumberKind;
  raw: string;
  /** 交给 TTS 的规范化读法（zh）。 */
  spoken: string;
}

const DIGITS_ZH = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

function digitByDigit(s: string): string {
  return [...s].map((c) => (c >= "0" && c <= "9" ? DIGITS_ZH[Number(c)] ?? c : c === "." ? "点" : "")).join("").replace(/零/g, "〇");
}

function integerToZh(s: string): string {
  if (s.length > 16) return digitByDigit(s);
  const units = ["", "十", "百", "千", "万", "十", "百", "千", "亿", "十", "百", "千", "万", "十", "百", "千"];
  let out = "";
  let zeroPending = false;
  let started = false;
  const arr = [...s];
  const n = arr.length;
  for (let i = 0; i < n; i++) {
    const d = arr[i]!;
    const digit = Number(d);
    const unit = units[n - 1 - i] ?? "";
    if (digit === 0) {
      zeroPending = started;
      continue;
    }
    if (zeroPending) {
      out += "零";
      zeroPending = false;
    }
    if (digit === 1 && unit === "十" && !started) out += "十";
    else out += DIGITS_ZH[digit]! + unit;
    started = true;
  }
  if (!started) return "零";
  return out;
}

function decimalsToZh(s: string): string {
  return "点" + [...s].map((c) => DIGITS_ZH[Number(c)] ?? "").join("");
}

/** 电话读法：11 位手机 3-4-4 分节，逐字报（三五〇 不读成 三百五十）。 */
export function phoneSpoken(digits: string): string {
  if (digits.length === 11) {
    return [digits.slice(0, 3), digits.slice(3, 7), digits.slice(7)].map(digitByDigit).join("，");
  }
  return digitByDigit(digits);
}

/** 日期读法：YYYY年M月D日。 */
export function dateSpoken(y: string, m: string, d: string): string {
  return `${integerToZh(y)}年${integerToZh(String(Number(m)))}月${integerToZh(String(Number(d)))}日`;
}

/** 金额读法（人民币，分位截断两位）。 */
export function currencySpoken(int: string, dec: string | null): string {
  const yuan = `${integerToZh(int)}元`;
  if (!dec) return `${yuan}整`;
  const d = dec.padEnd(2, "0").slice(0, 2);
  const jiao = Number(d[0]);
  const fen = Number(d[1]);
  let out = yuan;
  if (jiao > 0) out += `${DIGITS_ZH[jiao]}角`;
  if (fen > 0) out += `${DIGITS_ZH[fen]}分`;
  if (jiao === 0 && fen === 0) return `${yuan}整`;
  return out;
}

/** 上下文识别 → 分节播报序列。 */
export function numberSpeech(text: string): NumberSegment[] {
  const out: NumberSegment[] = [];
  // 电话：11 位（可带 86 前缀/连字符/空格）
  const phoneRe = /(?:\+?86[- ]?)?(1[3-9]\d[- ]?\d{4}[- ]?\d{4})/g;
  let m: RegExpExecArray | null;
  const consumed: Array<[number, number, NumberSegment]> = [];
  while ((m = phoneRe.exec(text))) {
    const digits = m[1]!.replace(/\D/g, "").replace(/^86/, "");
    consumed.push([m.index, m.index + m[0].length, { kind: "phone", raw: m[0], spoken: phoneSpoken(digits) }]);
  }
  // 日期：YYYY-MM-DD / YYYY/MM/DD / YYYY年M月D日
  const dateRe = /(\d{4})[-/年](\d{1,2})[-/月](\d{1,2})日?/g;
  while ((m = dateRe.exec(text))) {
    if (consumed.some(([s, e]) => m!.index < e && m!.index + m![0].length > s)) continue;
    consumed.push([m.index, m.index + m[0].length, { kind: "date", raw: m[0], spoken: dateSpoken(m[1]!, m[2]!, m[3]!) }]);
  }
  // 金额：¥/￥/元/RMB 前缀数字
  const curRe = /(?:¥|￥|RMB\s?)(\d{1,12}(?:,\d{3})*(?:\.\d{1,2})?)/g;
  while ((m = curRe.exec(text))) {
    if (consumed.some(([s, e]) => m!.index < e && m!.index + m![0].length > s)) continue;
    const num = m[1]!.replace(/,/g, "");
    const [ip, dp] = num.split(".");
    consumed.push([m.index, m.index + m[0].length, { kind: "currency", raw: m[0], spoken: currencySpoken(ip!, dp ?? null) }]);
  }
  // 其余数字：plain
  const plainRe = /\d+(?:\.\d+)?/g;
  while ((m = plainRe.exec(text))) {
    if (consumed.some(([s, e]) => m!.index < e && m!.index + m![0].length > s)) continue;
    const [ip, dp] = m[0].split(".");
    const spoken = dp ? `${integerToZh(ip!)}${decimalsToZh(dp)}` : integerToZh(ip!);
    consumed.push([m.index, m.index + m[0].length, { kind: "plain", raw: m[0], spoken }]);
  }
  consumed.sort((a, b) => a[0] - b[0]);
  for (const [, , seg] of consumed) out.push(seg);
  return out;
}

// ---------------------------------------------------------------------------
// W-171 左右手镜像（中心对称翻转）
// ---------------------------------------------------------------------------

/** 键盘三行几何（中心对称镜像基准）。 */
export const KEY_ROWS: string[] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];

export function mirrorKey(k: string): string {
  const key = k.toLowerCase();
  if (key === "arrowleft") return "ArrowRight";
  if (key === "arrowright") return "ArrowLeft";
  for (const row of KEY_ROWS) {
    const i = row.indexOf(key);
    if (i >= 0) {
      const j = row.length - 1 - i;
      return row[j]!.toUpperCase();
    }
  }
  return k; // 非字母键保持原样（诚实不猜）
}

/** 方案镜像：{ 动作: 键 } → 中心对称新方案（原方案不动）。 */
export function mirrorScheme(scheme: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [action, key] of Object.entries(scheme)) out[action] = mirrorKey(key);
  return out;
}

// ---------------------------------------------------------------------------
// W-172 字体栈医生
// ---------------------------------------------------------------------------

export interface FontStackAudit {
  entries: string[];
  issues: Array<{ kind: "missing" | "no-generic" | "duplicate" | "cjk-gap"; detail: string }>;
  suggested: string;
}

const SAFE_FALLBACK = '"Segoe UI", "Microsoft YaHei", "PingFang SC", sans-serif';

/** 已知安全栈（本地探测名单；探测失败如实标 UNKNOWN）。 */
const KNOWN_FONTS = new Set([
  "segoe ui", "microsoft yahei", "pingfang sc", "hiragino sans gb", "sans-serif", "serif", "monospace",
  "consolas", "cascadia code", "jetbrains mono", "source han sans sc", "noto sans sc", "system-ui", "arial",
]);

function fontProbeAvailable(family: string): boolean | "unknown" {
  if (typeof document === "undefined") return "unknown";
  try {
    const probe = document.fonts?.check?.(`12px "${family}"`);
    return typeof probe === "boolean" ? probe : "unknown";
  } catch {
    return "unknown";
  }
}

export function auditFontStack(stack: string): FontStackAudit {
  const entries = stack
    .split(",")
    .map((s) => s.trim().replace(/^["']|["']$/g, ""))
    .filter(Boolean);
  const issues: FontStackAudit["issues"] = [];
  const seen = new Set<string>();
  for (const e of entries) {
    const lower = e.toLowerCase();
    if (seen.has(lower)) issues.push({ kind: "duplicate", detail: e });
    seen.add(lower);
    if (!KNOWN_FONTS.has(lower)) {
      const avail = fontProbeAvailable(e);
      if (avail === false) issues.push({ kind: "missing", detail: e });
    }
  }
  const last = entries[entries.length - 1]?.toLowerCase();
  if (last !== "sans-serif" && last !== "serif" && last !== "monospace") {
    issues.push({ kind: "no-generic", detail: last ?? "" });
  }
  const hasCjk = entries.some((e) => /yahei|pingfang|han|hei|song|kai/i.test(e));
  if (!hasCjk) issues.push({ kind: "cjk-gap", detail: "stack has no CJK-capable font" });
  const suggested = [...entries.filter((e, i) => entries.findIndex((x) => x.toLowerCase() === e.toLowerCase()) === i), ...SAFE_FALLBACK.split(", ").filter((f) => !entries.some((e) => e.toLowerCase() === f.toLowerCase().replace(/"/g, "")))].join(", ");
  return { entries, issues, suggested };
}

// ---------------------------------------------------------------------------
// W-173 零术语词典
// ---------------------------------------------------------------------------

export interface TermEntry {
  term: string;
  plain: string;
  en?: string;
}

/** 内置白话词典（就地弹卡用；持续扩充）。 */
export const TERM_DICT: TermEntry[] = [
  { term: "进程", plain: "正在运行的程序，每个程序跑起来就是一条进程", en: "process" },
  { term: "线程", plain: "进程里的一股小工人在干活，多线程就是多个工人同时干" },
  { term: "缓存", plain: "先放在手边的东西，下次要用就不用再跑一趟" },
  { term: "内核", plain: "系统最核心的那部分，管所有硬件调度" },
  { term: "注册表", plain: "Windows 的一本大账本，记着系统和软件的各种设置" },
  { term: "驱动", plain: "让系统认识硬件的说明书程序" },
  { term: "固件", plain: "焊在硬件里的小系统，开机前就先跑起来了" },
  { term: "带宽", plain: "网络马路有多宽，越宽同时跑的数据越多" },
  { term: "延迟", plain: "从按下去到有反应之间等的那一下" },
  { term: "吞吐量", plain: "一段时间里总共干完多少活" },
  { term: "内存", plain: "程序干活的临时桌面，关机就清空" },
  { term: "硬盘", plain: "存放文件的大柜子，关机也不丢" },
  { term: "固态硬盘", plain: "没有转盘的硬盘，快但按擦写次数寿命有限" },
  { term: "虚拟内存", plain: "内存不够时借硬盘凑数的备用桌面，慢很多" },
  { term: "页面文件", plain: "虚拟内存落在硬盘上的那个文件" },
  { term: "显卡", plain: "专门画图的芯片，游戏和界面都靠它" },
  { term: "显存", plain: "显卡自己的小内存，存画面的素材" },
  { term: "帧率", plain: "每秒画多少张图，60 就是每秒 60 张" },
  { term: "垂直同步", plain: "让画图等屏幕刷新，防止画面撕裂" },
  { term: "分辨率", plain: "屏幕由多少个小点组成，点越多越清楚" },
  { term: "DPI", plain: "每英寸多少个点，数值大字和图更细腻" },
  { term: "像素", plain: "屏幕上的一个小点" },
  { term: "渲染", plain: "把数据变成你能看见的画面的过程" },
  { term: "着色器", plain: "显卡上算颜色的小程序" },
  { term: "日志", plain: "系统或软件写的流水账，出问题靠它查线索" },
  { term: "崩溃", plain: "程序碰到处理不了的错，自己关掉了" },
  { term: "蓝屏", plain: "Windows 遇到严重错误被迫停下并显示蓝底报错" },
  { term: "转储", plain: "崩溃瞬间把现场拍个快照存下来" },
  { term: "沙盒", plain: "一个隔离的试衣间，在里面干坏事也出不来" },
  { term: "权限", plain: "允许你做哪些事的许可清单" },
  { term: "防火墙", plain: "守在网络门口的安检员" },
  { term: "加密", plain: "把内容变成只有拿钥匙的人能读懂的样子" },
  { term: "哈希", plain: "给数据算的指纹，内容变一点指纹就全变" },
  { term: "证书", plain: "证明'我是我'的电子身份证" },
  { term: "DNS", plain: "把网址翻译成机器地址的电话簿" },
  { term: "IP 地址", plain: "设备在网络里的门牌号" },
  { term: "VPN", plain: "在你和目的地之间修一条加密隧道" },
  { term: "代理", plain: "替你去访问网站的中间人" },
  { term: "端口", plain: "一台机器上不同服务的窗口号" },
  { term: "协议", plain: "双方约好的通信规矩" },
  { term: "API", plain: "软件提供的窗口，别的程序从这里递话" },
  { term: "SDK", plain: "帮你开发用的工具包" },
  { term: "开源", plain: "源代码公开，谁都能看能改" },
  { term: "闭源", plain: "源代码不公开，只给成品" },
  { term: "编译", plain: "把人能读的代码翻译成机器能跑的指令" },
  { term: "解释执行", plain: "边翻译边执行，不用先整本翻完" },
  { term: "垃圾回收", plain: "程序自己打扫不再用的内存" },
  { term: "内存泄漏", plain: "程序借了内存不还，越用越卡" },
  { term: "死锁", plain: "两个程序互相等对方放手，谁都动不了" },
  { term: "竞态", plain: "两个活儿抢同一个东西，结果看谁先到" },
  { term: "异步", plain: "先把活派出去，不等干完就继续往下走" },
  { term: "回调", plain: "'干完活了叫我'——留个联系方式等通知" },
  { term: "事件循环", plain: "程序里转个不停的待办清单处理机" },
  { term: "版本号", plain: "软件的第几代成品，一般越靠后越新" },
  { term: "补丁", plain: "修小毛病的小更新" },
  { term: "回滚", plain: "退回上一个能用的版本" },
  { term: "备份", plain: "给重要数据拍张存照，丢了能还原" },
  { term: "恢复", plain: "把备份存照还原回来" },
  { term: "镜像", plain: "一比一的完整副本" },
  { term: "虚拟机", plain: "用软件模拟出的一台'假电脑'" },
  { term: "容器", plain: "比虚拟机更轻的隔离包装，只装运行必需品" },
  { term: "终端", plain: "直接敲命令跟系统说话的窗口" },
  { term: "命令行", plain: "用打字代替点鼠标的操作方式" },
  { term: "脚本", plain: "一串自动执行的指令清单" },
  { term: "环境变量", plain: "给所有程序传话的公共便签" },
  { term: "路径", plain: "文件在柜子里的具体位置" },
  { term: "绝对路径", plain: "从根开始写全的位置" },
  { term: "相对路径", plain: "从当前站的位置往后写" },
  { term: "压缩包", plain: "把文件打包抽气变小，拆开还原" },
  { term: "校验和", plain: "验证文件没在途中损坏的数字标签" },
  { term: "签名", plain: "作者盖的章，证明文件没被改过" },
  { term: "遥测", plain: "软件悄悄上报的使用情况数据" },
  { term: "A/B 测试", plain: "两套方案各放一半人试，看哪套好" },
  { term: "灰度发布", plain: "新版本先放给一小撮人试，稳了再全量" },
  { term: "热更新", plain: "不重启就把程序换血" },
  { term: "降级", plain: "顶不住时先关掉花活保住基本功能" },
  { term: "熔断", plain: "错误率太高时主动断开，防止越拖越糟" },
  { term: "限流", plain: "门口设闸，一次只放这么多人进" },
  { term: "负载均衡", plain: "多个工人分摊活儿，别累死一个闲着其他" },
  { term: "守护进程", plain: "在后台默默值守的服务程序" },
  { term: "计划任务", plain: "到点自动干的活" },
  { term: "钩子", plain: "在流程某一步挂上自己的'拦路小动作'" },
  { term: "中间件", plain: "请求路过时顺路处理一站的关卡" },
  { term: "正则表达式", plain: "用符号描述文字长什么样的匹配咒语" },
  { term: "编码", plain: "字符和数字之间的对照表" },
  { term: "UTF-8", plain: "目前最通用的字符编码，全世界的字都能装" },
  { term: "乱码", plain: "用错了对照表，字全对不上号" },
  { term: "序列化", plain: "把内存里的数据变成能存能传的文本" },
  { term: "JSON", plain: "一种人也能读的通用数据格式" },
  { term: "数据库", plain: "按规矩存放和查找数据的仓库" },
  { term: "索引", plain: "书前面的目录，查数据不用一页页翻" },
  { term: "事务", plain: "要么全做完要么全不做的成套操作" },
  { term: "主键", plain: "每行数据的唯一编号" },
  { term: "外键", plain: "指向别张表主键的引用" },
  { term: "迁移", plain: "把数据结构从旧格局搬到新格局" },
  { term: "测试", plain: "替用户先踩坑的检查流程" },
  { term: "单元测试", plain: "给最小零件做的体检" },
  { term: "集成测试", plain: "把零件装一起再体检" },
  { term: "回归", plain: "修好一处老毛病又在别处犯" },
  { term: "覆盖率", plain: "测试摸过多少比例的代码" },
  { term: "持续集成", plain: "每次提交自动编译加体检的流水线" },
  { term: "构建", plain: "把源码变成能运行成品的过程" },
  { term: "发布", plain: "把成品正式交到用户手里" },
  { term: "仓库", plain: "存代码历史的地方" },
  { term: "提交", plain: "给代码改动拍快照并记账" },
  { term: "分支", plain: "平行世界里的代码线，互不打扰" },
  { term: "合并", plain: "把平行世界的改动汇到一条线上" },
  { term: "冲突", plain: "两边的改动撞了车，需要人工裁决" },
  { term: "拉取请求", plain: "举手说'我改好了，请审一下'" },
];

export interface TermHit {
  entry: TermEntry;
  index: number;
}

/** 就地查词：返回全部命中（长词优先避免子串遮蔽）。 */
export function termLookup(text: string): TermHit[] {
  const sorted = [...TERM_DICT].sort((a, b) => b.term.length - a.term.length);
  const out: TermHit[] = [];
  for (const entry of sorted) {
    const idx = text.indexOf(entry.term);
    if (idx >= 0) out.push({ entry, index: idx });
  }
  return out.sort((a, b) => a.index - b.index);
}

// ---------------------------------------------------------------------------
// W-174 拼音北极星
// ---------------------------------------------------------------------------

/** 内置注音字符表（未收录字如实不加注）。 */
export const PINYIN_TABLE: Record<string, string> = {
  的: "de", 了: "le", 是: "shì", 我: "wǒ", 不: "bù", 在: "zài", 人: "rén", 有: "yǒu", 他: "tā", 这: "zhè",
  个: "gè", 上: "shàng", 们: "men", 来: "lái", 到: "dào", 时: "shí", 大: "dà", 地: "dì", 为: "wèi", 子: "zǐ",
  中: "zhōng", 你: "nǐ", 说: "shuō", 生: "shēng", 国: "guó", 年: "nián", 着: "zhe", 就: "jiù", 那: "nà", 和: "hé",
  要: "yào", 她: "tā", 出: "chū", 也: "yě", 得: "dé", 里: "lǐ", 后: "hòu", 自: "zì", 以: "yǐ", 会: "huì",
  家: "jiā", 可: "kě", 下: "xià", 而: "ér", 过: "guò", 天: "tiān", 去: "qù", 能: "néng", 对: "duì", 小: "xiǎo",
  多: "duō", 然: "rán", 于: "yú", 心: "xīn", 学: "xué", 么: "me", 之: "zhī", 都: "dōu", 好: "hǎo", 看: "kàn",
  起: "qǐ", 发: "fā", 当: "dāng", 没: "méi", 成: "chéng", 只: "zhǐ", 如: "rú", 事: "shì", 把: "bǎ", 还: "hái",
  用: "yòng", 第: "dì", 样: "yàng", 道: "dào", 想: "xiǎng", 作: "zuò", 种: "zhǒng", 开: "kāi", 美: "měi", 总: "zǒng",
  从: "cóng", 无: "wú", 情: "qíng", 己: "jǐ", 面: "miàn", 最: "zuì", 女: "nǚ", 但: "dàn", 前: "qián", 所: "suǒ",
  同: "tóng", 日: "rì", 手: "shǒu", 又: "yòu", 行: "xíng", 意: "yì", 动: "dòng", 方: "fāng", 期: "qī", 它: "tā",
  文件: "wén jiàn", 设置: "shè zhì", 系统: "xì tǒng", 关闭: "guān bì", 打开: "dǎ kāi", 启动: "qǐ dòng", 应用: "yìng yòng",
  窗口: "chuāng kǒu", 桌面: "zhuō miàn", 任务: "rèn wu", 通知: "tōng zhī", 声音: "shēng yīn", 显示: "xiǎn shì",
  保存: "bǎo cún", 取消: "qǔ xiāo", 确认: "què rèn", 删除: "shān chú", 复制: "fù zhì", 粘贴: "zhān tiē",
  搜索: "sōu suǒ", 更新: "gēng xīn", 下载: "xià zài", 安装: "ān zhuāng", 运行: "yùn xíng", 退出: "tuì chū",
  电源: "diàn yuán", 电量: "diàn liàng", 网络: "wǎng luò", 蓝牙: "lán yá", 键盘: "jiàn pán", 鼠标: "shǔ biāo",
  帮助: "bāng zhù", 反馈: "fǎn kuì", 语言: "yǔ yán", 时间: "shí jiān", 日期: "rì qī", 天气: "tiān qì",
};

/** 单字注音：未收录返回 null（不编造读音）。 */
export function pinyinOf(ch: string): string | null {
  return PINYIN_TABLE[ch] ?? null;
}

/** 词级优先注音：先查整词，再逐字。 */
export function annotateZh(text: string): Array<{ seg: string; py: string | null }> {
  const out: Array<{ seg: string; py: string | null }> = [];
  let i = 0;
  while (i < text.length) {
    let matched = false;
    // 词表条目里的多字词优先
    for (const [term, py] of Object.entries(PINYIN_TABLE)) {
      if (term.length > 1 && text.startsWith(term, i)) {
        out.push({ seg: term, py });
        i += term.length;
        matched = true;
        break;
      }
    }
    if (matched) continue;
    const ch = text[i]!;
    if (/[\u4e00-\u9fff]/.test(ch)) {
      out.push({ seg: ch, py: PINYIN_TABLE[ch] ?? null });
    } else {
      out.push({ seg: ch, py: null });
    }
    i++;
  }
  return out;
}

// ---------------------------------------------------------------------------
// W-175 盲文徽章（Grade-1 Unicode 盲文）
// ---------------------------------------------------------------------------

const BRAILLE_LETTERS: Record<string, string> = {
  a: "⠁", b: "⠃", c: "⠉", d: "⠙", e: "⠑", f: "⠋", g: "⠛", h: "⠓", i: "⠊", j: "⠚",
  k: "⠅", l: "⠇", m: "⠍", n: "⠝", o: "⠕", p: "⠏", q: "⠟", r: "⠗", s: "⠎", t: "⠞",
  u: "⠥", v: "⠧", w: "⠺", x: "⠭", y: "⠽", z: "⠵",
};

const BRAILLE_PUNCT: Record<string, string> = {
  ",": "⠂", ".": "⠲", "?": "⠦", "!": "⠖", ":": "⠒", ";": "⠰", "-": "⠤", "(": "⠐", ")": "⠐", "'": "⠄", '"': "⠐",
};

export const BRAILLE_MAX_CELLS = 120;

/**
 * Grade-1 盲文：字母直映、数字加 ⠼ 数字符（a–j 复用）、大写加 ⠠ 前缀；
 * 未收录字符跳过（诚实不编）；超长截断到 120 格。
 */
export function braille(text: string): string {
  let out = "";
  let numMode = false;
  for (const orig of text) {
    if (out.length >= BRAILLE_MAX_CELLS) break;
    const isUpper = orig >= "A" && orig <= "Z";
    const ch = orig.toLowerCase();
    if (ch >= "0" && ch <= "9") {
      if (!numMode) {
        out += "⠼";
        numMode = true;
      }
      out += BRAILLE_LETTERS[ch === "0" ? "j" : String.fromCharCode(ch.charCodeAt(0) - 49 + 97)] ?? "";
      continue;
    }
    numMode = false;
    if (isUpper) {
      if (out.length >= BRAILLE_MAX_CELLS) break;
      out += "⠠";
    }
    if (BRAILLE_LETTERS[ch]) {
      out += BRAILLE_LETTERS[ch]!;
      continue;
    }
    if (BRAILLE_PUNCT[ch]) {
      out += BRAILLE_PUNCT[ch]!;
      continue;
    }
    if (ch === " ") {
      if (out.length > 0 && out.length < BRAILLE_MAX_CELLS) out += " ";
      continue;
    }
    // 中文等未收录：跳过（诚实不编）
  }
  return out;
}

/** 双通道徽章：aria-label + 盲文点阵（盲文显示设备读点阵，读屏读 label）。 */
export function brailleBadge(label: string): { aria: string; braille: string } {
  return { aria: label, braille: braille(label) };
}

// ---------------------------------------------------------------------------
// 行为层（DOM 叠层 + 事件 + dataset/CSS 变量）
// ---------------------------------------------------------------------------

let styleEl: HTMLStyleElement | null = null;

function ensureStyle(): void {
  if (typeof document === "undefined") return;
  if (styleEl?.isConnected) return;
  styleEl = document.createElement("style");
  styleEl.id = "nova-a11y-style";
  styleEl.textContent = `
.nova-a11y-flash{position:fixed;inset:0;pointer-events:none;z-index:2147483000;opacity:0;transition:opacity 120ms ease}
.nova-a11y-flash.severity-info{box-shadow:inset 0 0 0 3px rgba(80,160,255,.85)}
.nova-a11y-flash.severity-warn{box-shadow:inset 0 0 0 3px rgba(255,190,60,.9)}
.nova-a11y-flash.severity-alert{box-shadow:inset 0 0 0 4px rgba(255,80,80,.95)}
.nova-a11y-caption{position:fixed;left:50%;bottom:56px;transform:translateX(-50%);z-index:2147483001;
  background:rgba(12,14,18,.92);color:#f2f4f8;border:1px solid rgba(255,255,255,.14);border-radius:10px;
  padding:8px 14px;font:600 14px/1.4 "Segoe UI","Microsoft YaHei",sans-serif;pointer-events:none;max-width:70vw}
.nova-a11y-termcard{position:fixed;z-index:2147483002;background:rgba(18,20,26,.96);color:#eef1f6;
  border:1px solid rgba(255,255,255,.16);border-radius:10px;padding:10px 12px;max-width:320px;
  font:400 13px/1.55 "Segoe UI","Microsoft YaHei",sans-serif;box-shadow:0 8px 28px rgba(0,0,0,.4)}
.nova-a11y-termcard b{display:block;margin-bottom:4px;font-size:13px;letter-spacing:.02em}
.nova-a11y-confirmbar{position:fixed;left:50%;bottom:96px;transform:translateX(-50%);z-index:2147483003;
  background:rgba(30,16,16,.95);color:#ffd9d9;border:1px solid rgba(255,120,120,.5);border-radius:10px;
  padding:10px 16px;font:600 13px/1.4 "Segoe UI","Microsoft YaHei",sans-serif;cursor:pointer}
html[data-nova-a11y-zoom]{--nova-a11y-scale:1}
`;
  document.head.appendChild(styleEl);
}

/** 闪光层（reduce-motion 下静态描边 300ms 后即除，不闪）。 */
let flashEl: HTMLDivElement | null = null;
let flashTimer: number | null = null;

function flashSeverity(sev: SoundSeverity): void {
  if (typeof document === "undefined") return;
  ensureStyle();
  if (!flashEl) {
    flashEl = document.createElement("div");
    flashEl.className = "nova-a11y-flash";
    document.body.appendChild(flashEl);
  }
  flashEl.className = `nova-a11y-flash severity-${sev}`;
  const blink = motionOK();
  flashEl.style.opacity = "1";
  if (flashTimer != null) window.clearTimeout(flashTimer);
  flashTimer = window.setTimeout(
    () => {
      if (flashEl) flashEl.style.opacity = "0";
      flashTimer = window.setTimeout(() => {
        flashEl?.remove();
        flashEl = null;
      }, 160);
    },
    blink ? 320 : 900,
  );
}

let captionEl: HTMLDivElement | null = null;
let captionTimer: number | null = null;

function showCaption(text: string, ttlMs = 2600): void {
  if (typeof document === "undefined") return;
  ensureStyle();
  if (!captionEl) {
    captionEl = document.createElement("div");
    captionEl.className = "nova-a11y-caption";
    document.body.appendChild(captionEl);
  }
  captionEl.textContent = text;
  captionEl.style.opacity = "1";
  if (captionTimer != null) window.clearTimeout(captionTimer);
  captionTimer = window.setTimeout(() => {
    captionEl?.remove();
    captionEl = null;
  }, ttlMs);
}

let termCardEl: HTMLDivElement | null = null;
let termTimer: number | null = null;

function showTermCard(term: TermEntry, x: number, y: number): void {
  if (typeof document === "undefined") return;
  ensureStyle();
  if (!termCardEl) {
    termCardEl = document.createElement("div");
    termCardEl.className = "nova-a11y-termcard";
    document.body.appendChild(termCardEl);
  }
  termCardEl.innerHTML = `<b>${term.term}</b>${term.plain}`;
  const pad = 12;
  const w = 320;
  const h = 96;
  termCardEl.style.left = `${clamp(x + pad, 8, Math.max(8, window.innerWidth - w - 8))}px`;
  termCardEl.style.top = `${clamp(y + pad, 8, Math.max(8, window.innerHeight - h - 8))}px`;
  if (termTimer != null) window.clearTimeout(termTimer);
  termTimer = window.setTimeout(() => {
    termCardEl?.remove();
    termCardEl = null;
  }, 4000);
}

let confirmEl: HTMLDivElement | null = null;
let confirmState = { armed: false, at: 0 };

function showConfirmBar(label: string): void {
  if (typeof document === "undefined") return;
  ensureStyle();
  if (confirmEl) confirmEl.remove();
  confirmEl = document.createElement("div");
  confirmEl.className = "nova-a11y-confirmbar";
  confirmEl.textContent = `再点一次确认：${label}`;
  confirmEl.addEventListener("click", () => {
    const r = confirmGate(confirmState, Date.now());
    if (r.pass) {
      a11yEvent("confirm-pass", { label });
      confirmState = { armed: false, at: 0 };
      confirmEl?.remove();
      confirmEl = null;
    } else {
      confirmState = { armed: true, at: Date.now() };
    }
  });
  document.body.appendChild(confirmEl);
  confirmState = { armed: true, at: Date.now() };
  window.setTimeout(
    () => {
      confirmEl?.remove();
      confirmEl = null;
      confirmState = { armed: false, at: 0 };
    },
    6000,
  );
}

/** ruby 注音挂载：为元素内中文加 <ruby> 上标（未收录字原样）。 */
export function annotateRuby(el: HTMLElement): number {
  if (typeof document === "undefined") return 0;
  ensureStyle();
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
  const targets: Text[] = [];
  let n = walker.nextNode() as Text | null;
  while (n) {
    if (/[\u4e00-\u9fff]/.test(n.data)) targets.push(n);
    n = walker.nextNode() as Text | null;
  }
  let count = 0;
  for (const t of targets) {
    const frag = document.createDocumentFragment();
    for (const part of annotateZh(t.data)) {
      if (part.py && part.seg.length === 1) {
        const ruby = document.createElement("ruby");
        ruby.className = "nova-a11y-ruby";
        ruby.textContent = part.seg;
        const rt = document.createElement("rt");
        rt.textContent = part.py;
        ruby.appendChild(rt);
        frag.appendChild(ruby);
        count++;
      } else {
        frag.appendChild(document.createTextNode(part.seg));
      }
    }
    t.parentNode?.replaceChild(frag, t);
  }
  return count;
}

// ---------------------------------------------------------------------------
// 激活 / 卸载（一代契约）
// ---------------------------------------------------------------------------

let active = false;
let bag: Array<() => void> = [];

function resetVolatile(): void {
  flashEl?.remove();
  flashEl = null;
  captionEl?.remove();
  captionEl = null;
  termCardEl?.remove();
  termCardEl = null;
  confirmEl?.remove();
  confirmEl = null;
  confirmState = { armed: false, at: 0 };
  if (flashTimer != null) window.clearTimeout(flashTimer);
  if (captionTimer != null) window.clearTimeout(captionTimer);
  if (termTimer != null) window.clearTimeout(termTimer);
  flashTimer = captionTimer = termTimer = null;
}

/** dataset/CSS 变量按当前注册表开关刷新（幂等）。 */
function applyDatasets(): void {
  if (typeof document === "undefined") return;
  const html = document.documentElement;
  const zoomOn = flagOn("W-165");
  if (zoomOn) {
    const plan = zoomPlan(150);
    html.dataset.novaA11yZoom = String(plan.scale);
    html.style.setProperty("--nova-a11y-scale", String(plan.scale));
    html.style.setProperty("--nova-a11y-hit-min", `${plan.hitMinPx}px`);
  } else {
    delete html.dataset.novaA11yZoom;
    html.style.removeProperty("--nova-a11y-scale");
    html.style.removeProperty("--nova-a11y-hit-min");
  }
  html.dataset.novaA11yElder = flagOn("W-168") ? "true" : "false";
  html.dataset.novaA11yDeaf = flagOn("W-167") ? "true" : "false";
  html.dataset.novaA11yBraille = flagOn("W-175") ? "true" : "false";
  html.dataset.novaA11yPinyin = flagOn("W-174") ? "true" : "false";
}

function onVoice(e: Event): void {
  if (!flagOn("W-164")) return;
  const text = String((e as CustomEvent).detail?.text ?? "");
  const match = matchVoiceCommand(text);
  if (!match) {
    a11yEvent("voice-nomatch", { text });
    return;
  }
  a11yEvent("voice-exec", match);
}

function onSound(e: Event): void {
  if (!flagOn("W-167")) return;
  const id = String((e as CustomEvent).detail?.id ?? "");
  const t = translateSound(id);
  if (!t) return; // 未收录如实静默（不编造转译）
  if (t.channels.flash) flashSeverity(t.severity);
  if (t.channels.caption) showCaption(t.captionZh);
  if (t.channels.vibr) {
    a11yEvent("vibrate", { pattern: t.channels.vibr });
    if (typeof navigator !== "undefined" && typeof navigator.vibrate === "function") {
      try {
        navigator.vibrate(t.channels.vibr);
      } catch {
        /* 无触觉硬件：静默 */
      }
    }
  }
  const log = lsGet<Array<{ id: string; at: number }>>(`${NS}.deaf-log`, []);
  log.push({ id: t.id, at: Date.now() });
  lsSet(`${NS}.deaf-log`, log.slice(-50));
}

function onNumber(e: Event): void {
  if (!flagOn("W-170")) return;
  const text = String((e as CustomEvent).detail?.text ?? "");
  const segs = numberSpeech(text);
  a11yEvent("number-parsed", { segments: segs });
  for (const s of segs) a11yEvent("speak", { text: s.spoken, kind: s.kind });
}

function onGrammar(e: Event): void {
  if (!flagOn("W-169")) return;
  const text = String((e as CustomEvent).detail?.text ?? "");
  const issues = checkGrammar(text);
  a11yEvent("grammar-result", { issues });
}

function onMirror(e: Event): void {
  if (!flagOn("W-171")) return;
  const scheme = (e as CustomEvent).detail?.scheme as Record<string, string> | undefined;
  if (!scheme) return;
  const mirrored = mirrorScheme(scheme);
  lsSet(`${NS}.mirror`, mirrored);
  a11yEvent("mirror-result", { mirrored });
  a11yEvent("z14-propose", { kind: "mirror", scheme: mirrored }); // 投递 Z-14 方案库（入库由 Z-14 决定）
}

function onFontStack(e: Event): void {
  if (!flagOn("W-172")) return;
  const stack = String((e as CustomEvent).detail?.stack ?? "");
  const audit = auditFontStack(stack);
  a11yEvent("fontstack-result", { audit });
}

function onColorTokens(e: Event): void {
  if (!flagOn("W-166")) return;
  const tokens = (e as CustomEvent).detail?.tokens as Record<string, string> | undefined;
  const pairs = (e as CustomEvent).detail?.pairs as Array<[string, string]> | undefined;
  const kind = ((e as CustomEvent).detail?.kind as CvdKind) ?? "deuteranopia";
  if (!tokens || !pairs) return;
  const result = rewriteColorRecipe(tokens, pairs, kind);
  if (!result) {
    a11yEvent("color-rewritten", { status: "invalid" });
    return;
  }
  a11yEvent("color-rewritten", result);
}

function onTermHover(e: Event): void {
  if (!flagOn("W-173")) return;
  const term = String((e as CustomEvent).detail?.term ?? "");
  const x = Number((e as CustomEvent).detail?.x ?? 0);
  const y = Number((e as CustomEvent).detail?.y ?? 0);
  const hit = termLookup(term);
  const entry = hit[0]?.entry;
  if (!entry) return;
  showTermCard(entry, x, y);
  lsSet(`${NS}.term-count`, lsGet<number>(`${NS}.term-count`, 0) + 1);
}

function onBadge(e: Event): void {
  if (!flagOn("W-175")) return;
  const label = String((e as CustomEvent).detail?.label ?? "");
  const badge = brailleBadge(label);
  a11yEvent("badge-result", badge);
}

function onConfirmGate(e: Event): void {
  if (!flagOn("W-168")) return;
  const label = String((e as CustomEvent).detail?.label ?? "");
  showConfirmBar(label);
}

function onKeydown(e: KeyboardEvent): void {
  // W-173：Ctrl+Alt+D 就地查词开关
  if (e.ctrlKey && e.altKey && (e.key === "d" || e.key === "D")) {
    if (!flagOn("W-173")) return;
    a11yEvent("dict-toggle", { open: !termCardEl });
  }
}

function onRegistryChange(): void {
  applyDatasets();
}

export function activateA11yNova(): void {
  if (active) return;
  if (typeof window === "undefined") return;
  active = true;
  resetVolatile();
  ensureStyle();
  applyDatasets();

  window.addEventListener("nova://a11y-voice", onVoice);
  bag.push(() => window.removeEventListener("nova://a11y-voice", onVoice));
  window.addEventListener("nova://a11y-sound", onSound);
  bag.push(() => window.removeEventListener("nova://a11y-sound", onSound));
  window.addEventListener("nova://a11y-number", onNumber);
  bag.push(() => window.removeEventListener("nova://a11y-number", onNumber));
  window.addEventListener("nova://a11y-grammar", onGrammar);
  bag.push(() => window.removeEventListener("nova://a11y-grammar", onGrammar));
  window.addEventListener("nova://a11y-mirror-src", onMirror);
  bag.push(() => window.removeEventListener("nova://a11y-mirror-src", onMirror));
  window.addEventListener("nova://a11y-fontstack", onFontStack);
  bag.push(() => window.removeEventListener("nova://a11y-fontstack", onFontStack));
  window.addEventListener("nova://a11y-color-tokens", onColorTokens);
  bag.push(() => window.removeEventListener("nova://a11y-color-tokens", onColorTokens));
  window.addEventListener("nova://a11y-term-hover", onTermHover);
  bag.push(() => window.removeEventListener("nova://a11y-term-hover", onTermHover));
  window.addEventListener("nova://a11y-badge", onBadge);
  bag.push(() => window.removeEventListener("nova://a11y-badge", onBadge));
  window.addEventListener("nova://a11y-confirm-gate", onConfirmGate);
  bag.push(() => window.removeEventListener("nova://a11y-confirm-gate", onConfirmGate));
  window.addEventListener("keydown", onKeydown);
  bag.push(() => window.removeEventListener("keydown", onKeydown));

  bag.push(subscribeNova(onRegistryChange));
}

export function deactivateA11yNova(): void {
  if (!active) return;
  active = false;
  for (const off of bag) {
    try {
      off();
    } catch {
      /* 尽力而为 */
    }
  }
  bag = [];
  resetVolatile();
}

export function a11yNovaActive(): boolean {
  return active;
}
