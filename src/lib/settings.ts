import { errMessage, ipc } from "./ipc";
import type { Lang } from "../i18n/dictionaries";
import { coerceInputFeel, DEFAULT_INPUT_FEEL, type InputFeelSettings } from "./inputFeel";
import { DEFAULT_AMBIENCE, coerceAmbience, type AmbienceSettings } from "../system/ambience/schema";

export type ThemeId = "deep-space" | "paper" | "minimal-black" | "high-contrast" | "custom";
export type PerfMode = "high" | "balanced" | "eco" | "static" | "auto";
export type BgType = "nebula" | "color" | "gradient" | "image" | "video";
/** 桌面环境壁纸模式（M2）：纯黑 / 3D 引力场 / 视频 / 图片 / 混合（媒体+星空叠加）。
 * living = 活化图片（实机反馈：Windows 动态壁纸在 Variable 里变静态）——图片之上
 * 叠加粒子活化层 + Ken Burns 缓动，任何静态图都有呼吸感；reduce-motion 自动降级静态。 */
export type WallpaperMode = "solid" | "gravity" | "video" | "image" | "living" | "hybrid" | "web" | "shader" | "system";
/**
 * 启动动画（批次A，规格 14.2.1）：控制真实加载完成后的过渡编排。
 * - full：字母落位任务栏 + 任务栏展开 + 图标淡入（阶段4/5 完整编排）
 * - simple：快速交叉淡入
 * - none：直接进入桌面
 * 进度/日志本身始终是真实事件，此设置只影响 ready 之后的过渡形式。
 */
export type BootAnim = "full" | "simple" | "none";
/**
 * U-06 仪式节奏（bootPacing）：管「走多慢」，与 bootAnim（管「怎么走」）正交。
 * - cinematic：影院（enter 900ms / readyHold 1200ms，默认）
 * - brisk：轻快（500ms / 400ms）
 * - instant：直通（0/0，等价 bootAnim=none 快路径）
 */
export type BootPacing = "cinematic" | "brisk" | "instant";
/** U-52 声景主题：default（玻璃质感基线）/ wood（木质）/ midnight（暗夜）。 */
export type SoundThemeId = "default" | "wood" | "midnight";
/** 桌面图标三档大小（批次B，规格 4.2.4）：像素为图标底座边长。 */
export type IconSize = 32 | 48 | 64;
/** 窗口控制按钮位置（批次D，规格 4.3.5）：Mac 圆点（默认）/ Windows 风格。 */
export type WinControls = "mac" | "windows";
/** 任务栏停靠位置（批次E，规格 4.4）：底（默认）/ 左 / 右 / 顶。 */
export type TaskbarPos = "bottom" | "left" | "right" | "top";

export interface CustomBg {
  type: BgType;
  color: string;
  gradientFrom: string;
  gradientTo: string;
  imagePath: string;
  videoPath: string;
  blur: number;      // 0..24 px
  brightness: number; // 0.2..1.4
  vignette: number;   // 0..1
  saturation: number; // 0..2
  maskOpacity: number;// 0..1 (center darkening over writing area)
  dynamicStrength: number; // 0..1
  parallaxStrength: number; // 0..1
  playVideo: boolean;
  /** 批次E-15：网页壁纸（Wallpaper Engine web 型项目）入口 html 绝对路径。 */
  htmlPath: string;
  /** 实机反馈：scene 着色器壁纸（WE 片元着色器）本地 WebGL 渲染入口文件绝对路径；
   *  imagePath 同时保留预览图，着色器编译失败时回退静态图（绝不黑屏）。 */
  shaderPath: string;
  /** 壁纸中心·静态图活化属性：粒子密度 0..1.5（0=关）。 */
  livingIntensity: number;
  /** 壁纸中心·Ken Burns 漂移幅度 0..1（0≈静止，1=全幅慢漂）。 */
  livingDrift: number;
  /** 壁纸中心·粒子风格。 */
  particleStyle: "dust" | "bokeh" | "mixed";
}

export type GridMode = "dot" | "grid" | "iso" | "none";

export interface MindDefaults {
  gridEnabled: boolean;
  snapEnabled: boolean;
  gridMode: GridMode;
  gridColor: string;
  gridOpacity: number; // 0..1
  guidesEnabled: boolean;
  defaultShape: import("../lib/types").NodeShape;
  resizeSensitivity: number; // px threshold for handle activation
  edgeStyle: import("../lib/types").LineStyle;
  edgeAnim: boolean;
  wasdSpeed: number; // px/s base
}

export interface Settings {
  language: Lang;
  theme: ThemeId;
  /** 桌面壁纸模式（桌面环境 L0 显示层；与四软件内部主题互不影响）。 */
  wallpaperMode: WallpaperMode;
  /** 启动动画过渡形式（真实加载完成后的阶段4/5 编排开关）。 */
  bootAnim: BootAnim;
  /** 桌面图标大小三档（32/48/64）。 */
  iconSize: IconSize;
  /** 窗口控制按钮位置（桌面红绿灯 + 软件窗口控制条，规格 4.3.5）。 */
  winControls: WinControls;
  /** 任务栏停靠位置（批次E，规格 4.4）。 */
  taskbarPos: TaskbarPos;
  /** AI-03 V-18：运行指示样式三选（dot=Win11 圆点 / underline=Win10 下划线 / capsule=胶囊）。 */
  runIndicator: "dot" | "underline" | "capsule";
  /** AI-03 M-16：媒体呼吸（播放时时钟旁 2% 幅度 / 4s 周期；默认关；reduce-motion 自动停用）。 */
  mediaBreath: boolean;
  /** AI-03 M-12：时钟多时区（IANA 名，最多 3 个）。 */
  clockZones: string[];
  /** 快捷键自定义（批次E，规格 4.7）：action → accel；空 = 全默认。冲突检测在前端设置页。 */
  shortcutBinds: Record<string, string>;
  /** M-30 鼠标侧键编程：XBUTTON1/2 → 动作 id（默认 back/forward 语义）。 */
  xBinds: { xbutton1: string; xbutton2: string };
  /** M-31 启动槽应用归属：launch1..launch9 → appKey（空 = 未分配）。 */
  launchApps: Record<string, string>;
  /** M-33 按键回显开关（只回显功能组合，打字内容永不出现）。 */
  keycast: boolean;
  /** M-33 按键回显位置（四角可选）。 */
  keycastCorner: "tl" | "tr" | "bl" | "br";
  /** M-35 任务栏滚轮调音量（默认关；开启后任务栏滚轮 = 音量步进 + 浮标）。 */
  wheelVolume: boolean;
  /** Z-13 命令提示条开关（默认开；容器高度 > 600px 才显示）。 */
  commandHintBar: boolean;
  /** Z-14 当前键位方案 id。 */
  keymapProfile: string;
  /** M-28 键位使用统计（本地，只记 action id + 次数，隐私口径见 keymap/telemetry.ts）。 */
  keyStats: Record<string, number>;
  /** 🟢 绿灯状态（批次D，规格 4.3.4）：true = 避让 Windows 任务栏。 */
  avoidTaskbar: boolean;
  /** 首次启动欢迎向导已完成（完成后不再显示）。 */
  wizardDone: boolean;
  /** AI-20 V-93：Windows 偏好搬家向导已完成（首启触发一次；设置页可手动重开）。 */
  prefsImportDone: boolean;
  /** B-32 OOBE 首次初始化向导已完成（口令/三模板/介质体检/导览）。 */
  oobeDone: boolean;
  /** B-32：OOBE 建卷的容器文件路径（空 = 未创建）。 */
  oobeContainerPath: string;
  /** B-32：OOBE 容器是否启用口令加密。 */
  oobeContainerEncrypted: boolean;
  /** B-32：OOBE 选择的 AI 工具模板（claude-code/codex/zcode）。 */
  oobeTools: string[];
  /** 批次E-6：每日自动换壁纸（本地缓存目录按日期取图，零网络）。 */
  wallpaperDaily: boolean;
  /** 批次E-6：壁纸本地缓存目录（Bing 缓存等自备图片文件夹）。 */
  wallpaperPoolDir: string;
  /** 批次E-6：Win+Tab 多窗口切换器开关（true = Variable 接管 Win+Tab）。 */
  winTabSwitcher: boolean;
  /** S-1 防截屏模式（WDA_EXCLUDEFROMCAPTURE；仅防系统截屏 API，边界见设置页声明）。 */
  privacyShield: boolean;
  perfMode: PerfMode;
  showStatusBar: boolean;
  editorWidthPct: number; // 58..72
  editorAlign: "center" | "left" | "right";
  fontFamily: string;
  fontSize: number; // 14..22
  lineHeight: number; // 1.5..2.2
  autosaveDelayMs: number; // 400..3000
  uiZoom: number; // 0.85..1.3
  /** F-1 夜灯模式：0 = 关闭；10..70 = 色温遮罩强度（遮罩层，不动系统色温）。 */
  nightLight: number;
  reduceMotion: boolean;
  safeMode: boolean;
  /** A-4 声音设计：系统音音量 0-1；全局静音。 */
  soundVolume: number;
  soundMuted: boolean;
  /** Anime starfield performance tier: 1..10 fixed, 0 = smart auto monitor. */
  bgTier: number;
  /** 8.2 用户教学式词典：大白话解释，key 为小写术语。 */
  pvzDictOverrides: Record<string, string>;
  /** AI-06 输入手感组（U-58/U-59、V-61…V-70）：默认全部关闭或等于现状。 */
  inputFeel: InputFeelSettings;
  /** AI-01 Z-36：标题栏透明度/置顶微控总开关（默认 off）。 */
  winFeelOpacity: boolean;
  /** AI-01 M-01：摇一摇最小化（Aero Shake，默认 off）。 */
  winShake: boolean;
  /** AI-01 M-07：拖拽对齐参考线与轻吸附（默认 off；Alt 临时禁用）。 */
  winGuides: boolean;
  /** AI-01 Z-41：鼠标手势最小集（右键拖 下=关窗 / 上=最小化，默认 off）。 */
  winGestures: boolean;
  /** AI-01 Z-38：Alt+滚轮在置顶窗口间循环（默认 off）。 */
  altWheelTopmost: boolean;
  /** AI-01 M-08：精炼 Alt+Tab 过滤（off / app=同应用 / monitor=同屏；Ctrl+Alt+Tab 触发）。 */
  altTabFilter: "off" | "app" | "monitor";
  /** AI-01 M-09：悬停聚焦 X-Mouse（0=关 / 1=仅聚焦 / 2=聚焦并置顶）。 */
  xmouse: 0 | 1 | 2;
  /** AI-01 Z-42：右缘热区呼出桌面切换预览（0=关 / 8..32 px 宽度）。 */
  desktopHotzone: number;
  /** AI-12 Z-16：遗留协议兼容 Shim（默认开，只记录不改行为）。 */
  compatLegacyShim: boolean;
  /** AI-12 M-40：高刷（≥120Hz）动效时长 0.85× 折算（默认开）。 */
  compatHighRefresh: boolean;
  /** AI-12 Z-20：宿主档手动覆盖（auto 服从检测；误检安全=不确定按 native）。 */
  compatHostOverride: "auto" | "native" | "vm" | "remote";
  /** AI-12 Z-21：慢速档手动覆盖（-1=auto；0..3=L0..L3；只降不升）。 */
  compatSlowOverride: -1 | 0 | 1 | 2 | 3;
  /** AI-13 Z-58：UI 线程健康采集（默认关 = 零采集红线）。 */
  perfThreadHealth: boolean;
  /** AI-13 Z-59：空闲渲染冻结（最小化/遮挡暂停 rAF；默认关）。 */
  perfIdleFreeze: boolean;
  /** AI-13 Z-62：本地使用统计（零外发，只写本地；默认关）。 */
  perfUsageStats: boolean;
  /** AI-13 Z-61：更新通道（stable/beta）。 */
  updateChannel: "stable" | "beta";
  // ---- AI-19 无障碍与本地化组（U-40/U-41、M-73…M-78；默认全部关闭或等于现状）----
  /** M-74：系统高对比度跟随（默认关；开启后系统 HC 切换环境即时跟随）。 */
  themeFollowHc: boolean;
  /** M-75：动效时长全局缩放（0.5 / 1；reduceMotion 优先级最高不受影响）。 */
  motionScale: 0.5 | 1;
  /** U-40：色觉模拟器（开发者向预览；默认 off）。 */
  cvdSim: "off" | "protanopia" | "deuteranopia" | "tritanopia" | "achromatopsia";
  /** U-40：焦点跟随提示（Tab 移动读出目标控件名称；默认关）。 */
  focusAnnounce: boolean;
  /** M-77：简繁转换用户词表（键=简体词，值=目标串；上限 500 条）。 */
  s2tLexicon: Record<string, string>;
  /** U-41：i18n 运行时覆盖表 { lang: { key: value } }（工作台编辑写这里）。 */
  i18nOverrides: Record<string, Record<string, string>>;
  /** U-41：伪本地化模式（dev 检查硬编码文案与溢出布局；默认关）。 */
  pseudoLocale: boolean;
  /** U-41：RTL 试点（设置/通知中心两处面板正确渲染；默认关）。 */
  rtlPilot: boolean;
  /** M-78：日期/时间/数字区域格式跟随系统（默认关 = 现状硬编码）。 */
  localeFormatFollow: boolean;
  /** M-78：容量单位口径（auto=系统口径即二进制 / binary / decimal）。 */
  byteUnit: "auto" | "binary" | "decimal";
  // ---- AI-16 启动与声音通知组（U-05/U-06/U-52、Z-44/Z-47、N-32）----
  /** U-06 bootPacing：仪式节奏（cinematic 影院 / brisk 轻快 / instant 直通）。 */
  bootPacing: BootPacing;
  /** U-05 启动交响（full 完整三层 / mute 静音 / chime-only 仅就绪音）。 */
  bootSoundMode: "full" | "mute" | "chime-only";
  /** U-52 声景主题（default 玻璃质感基线 / wood 木质 / midnight 暗夜）。 */
  soundTheme: SoundThemeId;
  /** U-52 深夜（22:00–6:00）自动整体音量 ×0.5（默认开；可关闭此智能行为）。 */
  soundNightDamp: boolean;
  /** Z-44 勿扰日程启用（手动勿扰优先级高于日程）。 */
  dndScheduleEnabled: boolean;
  /** Z-44 每日自动勿扰开始 "HH:MM"（支持跨午夜，如 22:00→07:00）。 */
  dndScheduleStart: string;
  /** Z-44 每日自动勿扰结束 "HH:MM"。 */
  dndScheduleEnd: string;
  /** Z-44 提醒类豁免勿扰（闹钟/提醒仍响；普通通知静默入档）。 */
  dndReminderExempt: boolean;
  /** Z-47 通知存档保留策略（30/90/0=永久）。 */
  notifyRetentionDays: 0 | 30 | 90;
  /** N-32 智能通知整理（本地学习分堆；默认关 = 全即时堆，7 天学习期）。 */
  notifySmart: boolean;
  /** N-32 摘要堆定点呈现时刻（24h 制，默认 12:00 与 18:00 两次）。 */
  notifyDigestTimes: string[];
  /** AI-18 氛围与个性化组（默认与现状分毫不差）。 */
  ambience: AmbienceSettings;
  customBg: CustomBg;
  mindDefaults: MindDefaults;
}

export const DEFAULT_SETTINGS: Settings = {
  language: "zh",
  theme: "deep-space",
  wallpaperMode: "gravity",
  bootAnim: "full",
  iconSize: 48,
  winControls: "mac",
  taskbarPos: "bottom",
  runIndicator: "dot",
  mediaBreath: false,
  clockZones: [],
  shortcutBinds: {},
  xBinds: { xbutton1: "back", xbutton2: "forward" },
  launchApps: {},
  keycast: false,
  keycastCorner: "br",
  wheelVolume: false,
  commandHintBar: true,
  keymapProfile: "default",
  keyStats: {},
  avoidTaskbar: false,
  wizardDone: false,
  prefsImportDone: false,
  oobeDone: false,
  soundVolume: 0.5,
  soundMuted: false,
  oobeContainerPath: "",
  oobeContainerEncrypted: false,
  oobeTools: [],
  wallpaperDaily: false,
  wallpaperPoolDir: "",
  winTabSwitcher: true,
  privacyShield: false,
  perfMode: "high",
  showStatusBar: true,
  editorWidthPct: 64,
  editorAlign: "center",
  fontFamily: `"Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif`,
  fontSize: 16,
  lineHeight: 1.75,
  autosaveDelayMs: 1200,
  uiZoom: 1,
  nightLight: 0,
  reduceMotion: false,
  safeMode: false,
  bgTier: 0,
  pvzDictOverrides: {},
  inputFeel: structuredClone(DEFAULT_INPUT_FEEL),
  winFeelOpacity: false,
  winShake: false,
  winGuides: false,
  winGestures: false,
  altWheelTopmost: false,
  altTabFilter: "off",
  xmouse: 0,
  desktopHotzone: 0,
  perfThreadHealth: false,
  perfIdleFreeze: false,
  perfUsageStats: false,
  updateChannel: "stable",
  // AI-19 无障碍与本地化组（默认全部关闭或等于现状）
  themeFollowHc: false,
  motionScale: 1,
  cvdSim: "off",
  focusAnnounce: false,
  s2tLexicon: {},
  i18nOverrides: {},
  pseudoLocale: false,
  rtlPilot: false,
  localeFormatFollow: false,
  byteUnit: "auto",
  // AI-16 启动与声音通知组（默认全部等于现状/保守值）
  bootPacing: "cinematic",
  bootSoundMode: "full",
  soundTheme: "default",
  soundNightDamp: true,
  dndScheduleEnabled: false,
  dndScheduleStart: "22:00",
  dndScheduleEnd: "07:00",
  dndReminderExempt: true,
  notifyRetentionDays: 30,
  notifySmart: false,
  notifyDigestTimes: ["12:00", "18:00"],
  customBg: {
    type: "nebula",
    color: "#0a1226",
    gradientFrom: "#0a1638",
    gradientTo: "#04070f",
    imagePath: "",
    videoPath: "",
    blur: 6,
    brightness: 0.9,
    vignette: 0.55,
    saturation: 1,
    maskOpacity: 0.35,
    dynamicStrength: 0.5,
    parallaxStrength: 0.4,
    playVideo: true,
    htmlPath: "",
    shaderPath: "",
    livingIntensity: 0.8,
    livingDrift: 0.6,
    particleStyle: "mixed",
  },
  ambience: DEFAULT_AMBIENCE,
  mindDefaults: {
    gridEnabled: true,
    snapEnabled: true,
    gridMode: "grid",
    gridColor: "#1e3a8a", // chapter 6.3: WASD grid dots as faint ice-blue star dust
    gridOpacity: 0.16,
    guidesEnabled: true,
    defaultShape: "rounded",
    resizeSensitivity: 8,
    edgeStyle: "solid",
    edgeAnim: true,
    wasdSpeed: 520,
  },
  // AI-12 兼容纵深组
  compatLegacyShim: true,
  compatHighRefresh: true,
  compatHostOverride: "auto",
  compatSlowOverride: -1,
};

/**
 * AI-20 M-81：设置迁移（coerce）公开导出 —— 供 fixtures/settings/*.json
 * 版本矩阵漂移测试逐版断言（新版本必须补「上一版→新版」用例才能合入，
 * 见 CONTRIBUTING 与 tools/release.ps1 门禁）。
 */
export function coerceSettings(raw: Record<string, string>): Settings {
  return coerce(raw);
}

function coerce(raw: Record<string, string>): Settings {
  const s: Settings = structuredClone(DEFAULT_SETTINGS);
  try {
    if (raw["language"] === "en" || raw["language"] === "zh-TW" || raw["language"] === "zh") {
      s.language = raw["language"];
    }
    if (raw["theme"]) s.theme = raw["theme"] as ThemeId;
    if (raw["wallpaperMode"]) s.wallpaperMode = raw["wallpaperMode"] as WallpaperMode;
    if (raw["bootAnim"]) {
      const v = raw["bootAnim"];
      s.bootAnim = v === "simple" || v === "none" ? v : v === "full" ? "full" : s.bootAnim;
    }
    if (raw["iconSize"]) {
      const n = Number(raw["iconSize"]);
      s.iconSize = n === 32 || n === 64 ? (n as IconSize) : n === 48 ? 48 : s.iconSize;
    }
    if (raw["wizardDone"] !== undefined) s.wizardDone = raw["wizardDone"] === "1";
    if (raw["prefsImportDone"] !== undefined) s.prefsImportDone = raw["prefsImportDone"] === "1";
    if (raw["oobeDone"] !== undefined) s.oobeDone = raw["oobeDone"] === "1";
    if (raw["oobeContainerPath"] !== undefined) s.oobeContainerPath = raw["oobeContainerPath"];
    if (raw["oobeContainerEncrypted"] !== undefined) s.oobeContainerEncrypted = raw["oobeContainerEncrypted"] === "1";
    if (raw["oobeTools"] !== undefined) {
      try { s.oobeTools = JSON.parse(raw["oobeTools"]) as string[]; } catch { /* 保留默认 */ }
    }
    if (raw["winControls"]) s.winControls = raw["winControls"] === "windows" ? "windows" : "mac";
    if (raw["avoidTaskbar"] !== undefined) s.avoidTaskbar = raw["avoidTaskbar"] === "1";
    if (raw["wallpaperDaily"] !== undefined) s.wallpaperDaily = raw["wallpaperDaily"] === "1";
    if (raw["wallpaperPoolDir"]) s.wallpaperPoolDir = raw["wallpaperPoolDir"];
    if (raw["winTabSwitcher"] !== undefined) s.winTabSwitcher = raw["winTabSwitcher"] !== "0";
    if (raw["perfMode"]) s.perfMode = raw["perfMode"] as PerfMode;
    if (raw["showStatusBar"] !== undefined) s.showStatusBar = raw["showStatusBar"] === "1";
    if (raw["reduceMotion"] !== undefined) s.reduceMotion = raw["reduceMotion"] === "1";
    if (raw["safeMode"] !== undefined) s.safeMode = raw["safeMode"] === "1";
    if (raw["editorWidthPct"]) s.editorWidthPct = clamp(Number(raw["editorWidthPct"]) || DEFAULT_SETTINGS.editorWidthPct, 58, 72);
    if (raw["editorAlign"]) s.editorAlign = raw["editorAlign"] as Settings["editorAlign"];
    if (raw["fontFamily"]) s.fontFamily = raw["fontFamily"];
    if (raw["fontSize"]) s.fontSize = clamp(Number(raw["fontSize"]) || 16, 12, 26);
    if (raw["lineHeight"]) s.lineHeight = clamp(Number(raw["lineHeight"]) || 1.75, 1.3, 2.4);
    if (raw["autosaveDelayMs"]) s.autosaveDelayMs = clamp(Number(raw["autosaveDelayMs"]) || 1200, 300, 5000);
    if (raw["uiZoom"]) s.uiZoom = clamp(Number(raw["uiZoom"]) || 1, 0.8, 1.5);
    if (raw["soundVolume"] !== undefined) s.soundVolume = clamp(Number(raw["soundVolume"]) || 0, 0, 1);
    if (raw["soundMuted"] !== undefined) s.soundMuted = raw["soundMuted"] === "1";
    if (raw["nightLight"] !== undefined) s.nightLight = clamp(Number(raw["nightLight"]) || 0, 0, 70);
    if (raw["bgTier"] !== undefined) s.bgTier = clamp(Number(raw["bgTier"]) || 0, 0, 17);
    if (raw["pvzDictOverrides"]) {
      const parsed = JSON.parse(raw["pvzDictOverrides"]) as unknown;
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
          if (typeof v === "string" && v.trim()) s.pvzDictOverrides[k.toLowerCase().trim()] = v.trim();
        }
      }
    }
    if (raw["customBg"]) s.customBg = { ...s.customBg, ...JSON.parse(raw["customBg"]) };
    if (raw["inputFeel"]) {
      try { s.inputFeel = coerceInputFeel(JSON.parse(raw["inputFeel"])); } catch { /* 保留默认 */ }
    }
    // AI-01 窗口手感组（全部保守默认，写坏值回落 off/0）
    if (raw["winFeelOpacity"] !== undefined) s.winFeelOpacity = raw["winFeelOpacity"] === "1";
    if (raw["winShake"] !== undefined) s.winShake = raw["winShake"] === "1";
    if (raw["winGuides"] !== undefined) s.winGuides = raw["winGuides"] === "1";
    if (raw["winGestures"] !== undefined) s.winGestures = raw["winGestures"] === "1";
    if (raw["altWheelTopmost"] !== undefined) s.altWheelTopmost = raw["altWheelTopmost"] === "1";
    if (raw["altTabFilter"]) s.altTabFilter = raw["altTabFilter"] === "app" ? "app" : raw["altTabFilter"] === "monitor" ? "monitor" : "off";
    if (raw["xmouse"] !== undefined) {
      const n = Number(raw["xmouse"]);
      s.xmouse = n === 1 || n === 2 ? (n as Settings["xmouse"]) : 0;
    }
    if (raw["desktopHotzone"] !== undefined) s.desktopHotzone = clamp(Number(raw["desktopHotzone"]) || 0, 0, 32);
    // AI-12 兼容纵深组（写坏值回落默认）
    if (raw["compatLegacyShim"] !== undefined) s.compatLegacyShim = raw["compatLegacyShim"] !== "0";
    if (raw["compatHighRefresh"] !== undefined) s.compatHighRefresh = raw["compatHighRefresh"] !== "0";
    if (raw["compatHostOverride"]) {
      const v = raw["compatHostOverride"];
      s.compatHostOverride = v === "native" || v === "vm" || v === "remote" ? v : "auto";
    }
    if (raw["compatSlowOverride"] !== undefined) {
      const n = Number(raw["compatSlowOverride"]);
      s.compatSlowOverride = n >= 0 && n <= 3 ? (n as Settings["compatSlowOverride"]) : -1;
    }
    // AI-13 性能与长跑组（写坏值回落默认；默认全部关闭）
    if (raw["perfThreadHealth"] !== undefined) s.perfThreadHealth = raw["perfThreadHealth"] === "1";
    if (raw["perfIdleFreeze"] !== undefined) s.perfIdleFreeze = raw["perfIdleFreeze"] === "1";
    if (raw["perfUsageStats"] !== undefined) s.perfUsageStats = raw["perfUsageStats"] === "1";
    if (raw["updateChannel"]) s.updateChannel = raw["updateChannel"] === "beta" ? "beta" : "stable";
    // AI-19 无障碍与本地化组（写坏值回落默认）
    if (raw["themeFollowHc"] !== undefined) s.themeFollowHc = raw["themeFollowHc"] === "1";
    if (raw["motionScale"] !== undefined) s.motionScale = Number(raw["motionScale"]) === 0.5 ? 0.5 : 1;
    if (raw["cvdSim"]) {
      const v = raw["cvdSim"];
      s.cvdSim = v === "protanopia" || v === "deuteranopia" || v === "tritanopia" || v === "achromatopsia" ? v : "off";
    }
    if (raw["focusAnnounce"] !== undefined) s.focusAnnounce = raw["focusAnnounce"] === "1";
    if (raw["s2tLexicon"]) {
      try {
        const parsed = JSON.parse(raw["s2tLexicon"]) as Record<string, unknown>;
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
          const lex: Record<string, string> = {};
          for (const [k, v] of Object.entries(parsed)) {
            if (typeof v === "string" && k.trim() && v.trim() && Object.keys(lex).length < 500) {
              lex[k.trim()] = v.trim();
            }
          }
          s.s2tLexicon = lex;
        }
      } catch { /* 保留默认 */ }
    }
    if (raw["i18nOverrides"]) {
      try {
        const parsed = JSON.parse(raw["i18nOverrides"]) as Record<string, Record<string, string>>;
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
          const ov: Record<string, Record<string, string>> = {};
          for (const [lang, kv] of Object.entries(parsed)) {
            if (kv && typeof kv === "object" && !Array.isArray(kv)) ov[lang] = { ...kv };
          }
          s.i18nOverrides = ov;
        }
      } catch { /* 保留默认 */ }
    }
    if (raw["pseudoLocale"] !== undefined) s.pseudoLocale = raw["pseudoLocale"] === "1";
    if (raw["rtlPilot"] !== undefined) s.rtlPilot = raw["rtlPilot"] === "1";
    if (raw["localeFormatFollow"] !== undefined) s.localeFormatFollow = raw["localeFormatFollow"] === "1";
    if (raw["byteUnit"]) {
      const v = raw["byteUnit"];
      s.byteUnit = v === "binary" || v === "decimal" ? v : "auto";
    }
    // AI-16 启动与声音通知组（写坏值回落默认）
    if (raw["bootPacing"]) {
      const v = raw["bootPacing"];
      s.bootPacing = v === "brisk" || v === "instant" || v === "cinematic" ? v : "cinematic";
    }
    if (raw["bootSoundMode"]) {
      const v = raw["bootSoundMode"];
      s.bootSoundMode = v === "mute" || v === "chime-only" ? v : "full";
    }
    if (raw["soundTheme"]) {
      const v = raw["soundTheme"];
      s.soundTheme = v === "wood" || v === "midnight" ? v : "default";
    }
    if (raw["soundNightDamp"] !== undefined) s.soundNightDamp = raw["soundNightDamp"] === "1";
    if (raw["dndScheduleEnabled"] !== undefined) s.dndScheduleEnabled = raw["dndScheduleEnabled"] === "1";
    if (raw["dndScheduleStart"] && /^\d{2}:\d{2}$/.test(raw["dndScheduleStart"])) s.dndScheduleStart = raw["dndScheduleStart"];
    if (raw["dndScheduleEnd"] && /^\d{2}:\d{2}$/.test(raw["dndScheduleEnd"])) s.dndScheduleEnd = raw["dndScheduleEnd"];
    if (raw["dndReminderExempt"] !== undefined) s.dndReminderExempt = raw["dndReminderExempt"] === "1";
    if (raw["notifyRetentionDays"] !== undefined) {
      const n = Number(raw["notifyRetentionDays"]);
      s.notifyRetentionDays = n === 0 || n === 90 ? (n as 0 | 90) : 30;
    }
    if (raw["notifySmart"] !== undefined) s.notifySmart = raw["notifySmart"] === "1";
    if (raw["notifyDigestTimes"]) {
      try {
        const arr = JSON.parse(raw["notifyDigestTimes"]) as unknown;
        if (Array.isArray(arr)) {
          const times = arr.filter((t): t is string => typeof t === "string" && /^\d{2}:\d{2}$/.test(t)).slice(0, 4);
          if (times.length > 0) s.notifyDigestTimes = times;
        }
      } catch { /* 保留默认 */ }
    }
    // AI-18 氛围与个性化组（写坏值回落默认：氛围项绝不允许坏值弄巧成拙）
    if (raw["ambience"]) {
      try { s.ambience = coerceAmbience(JSON.parse(raw["ambience"])); } catch { /* 保留默认 */ }
    }
    if (raw["mindDefaults"]) {
      const md = { ...s.mindDefaults, ...JSON.parse(raw["mindDefaults"]) };
      // Clamp numeric ranges so a corrupt stored value (e.g. wasdSpeed 0) can
      // never silently kill WASD navigation or handle activation.
      md.wasdSpeed = clamp(Number(md.wasdSpeed) || DEFAULT_SETTINGS.mindDefaults.wasdSpeed, 200, 1200);
      md.resizeSensitivity = clamp(Number(md.resizeSensitivity) || DEFAULT_SETTINGS.mindDefaults.resizeSensitivity, 2, 40);
      md.gridOpacity = clamp(Number(md.gridOpacity) || DEFAULT_SETTINGS.mindDefaults.gridOpacity, 0, 1);
      s.mindDefaults = md;
    }
  } catch {
    // Corrupt settings fall back to defaults for the affected keys.
  }
  return s;
}

export function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}

const pendingWrites = new Map<string, string>();
let flushTimer: ReturnType<typeof setTimeout> | null = null;

async function flush(): Promise<void> {
  if (pendingWrites.size === 0) return;
  const entries = Object.fromEntries(pendingWrites);
  pendingWrites.clear();
  await ipc.setSettings(entries);
}

export async function loadSettings(): Promise<Settings> {
  try {
    const raw = await ipc.getSettings();
    return coerce(raw);
  } catch {
    return structuredClone(DEFAULT_SETTINGS);
  }
}

/** Persist one setting; failures are surfaced via returned error message key. */
export async function saveSetting(key: keyof Settings & string, value: unknown): Promise<void> {
  let serialized: string;
  if (typeof value === "boolean") serialized = value ? "1" : "0";
  else if (typeof value === "object") serialized = JSON.stringify(value);
  else serialized = String(value);
  pendingWrites.set(key, serialized);
  if (flushTimer) clearTimeout(flushTimer);
  flushTimer = setTimeout(() => {
    flush().catch((e) => {
      const { code } = errMessage(e);
      console.error(`[settings] persist failed (${code})`, e);
      window.dispatchEvent(new CustomEvent("variable:settings-error", { detail: code }));
    });
  }, 350);
  await new Promise<void>((r) => void r());
}
