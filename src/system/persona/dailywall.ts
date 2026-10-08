/**
 * F154 壁纸每日一换 · 完整设计。
 *
 * 主册判据：定时切换三连测；交叉淡入 500ms 无撕裂；构图保护 20 档分辨率实测（主体不裁头）。
 *
 * 【功能定义】壁纸轮换：本地图片池（用户选文件夹）/官方精选池（可选下载，无商店
 * 原则走 F134 目录）/每日定时换（默认 00:00）；切换动画 500ms 交叉淡入；构图保护
 * （自适应裁切 F147 同算法）。
 *
 * 【状态与异常】池空（文件夹被清）→ 回退默认壁纸+提示；官方池下载失败 → 静默用
 * 本地池；重复抽取 → 近 7 天排除（记录驱动）。
 *
 * 【设计细节】抽取算法：池内均匀随机+近 7 天排除表；预加载次日壁纸（换的时刻零
 * 等待）；「固定今天」次日自动解除；多屏前瞻接口预留（各屏独立池）。
 */

import { nextDailyFire, pickExcluding, personaStore } from "./store";

export const SECTION = "wallpaper";
export const SWITCH_FADE_MS = 500;
export const EXCLUDE_WINDOW_DAYS = 7;
export const HISTORY_LIMIT_DAYS = 30;

export type WallpaperPoolKind = "local" | "official";

export interface WallpaperPool {
  id: string;
  kind: WallpaperPoolKind;
  /** 本地池：文件夹路径列表；官方池：目录源标识（F134）。 */
  sources: string[];
  enabled: boolean;
}

export interface DailyWallConfig {
  enabled: boolean;
  pools: WallpaperPool[];
  /** 更换时刻 "HH:MM"（默认 00:00）。 */
  changeAt: string;
  /** 固定今天这张（次日自动解除——存日期键）。 */
  pinnedDate: string | null;
  /** 最近更换记录（日期键→壁纸 id），防短期重复 + 历史 30 天。 */
  history: Record<string, string>;
  /** 最近 7 天实际使用的壁纸 id（排除表输入）。 */
  recent: string[];
  /** 当前壁纸。 */
  current: string | null;
  /** 预加载的次日壁纸（换的时刻零等待）。 */
  preloaded: string | null;
}

export function defaultDailyWallConfig(): DailyWallConfig {
  return {
    enabled: false,
    pools: [{ id: "local-main", kind: "local", sources: [], enabled: true }],
    changeAt: "00:00",
    pinnedDate: null,
    history: {},
    recent: [],
    current: null,
    preloaded: null,
  };
}

export function loadDailyWallConfig(): DailyWallConfig {
  const stored = personaStore.getWith(SECTION, "daily", undefined) as Partial<DailyWallConfig> | undefined;
  return { ...defaultDailyWallConfig(), ...(stored ?? {}) };
}

export function saveDailyWallConfig(c: DailyWallConfig): void {
  personaStore.set(SECTION, { daily: c });
}

function dateKey(now: number): string {
  const d = new Date(now);
  const p = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** "HH:MM" 解析（与 F153 同规则——一处一事实：规则复述而非 import，因 F153 的
 * parseHHMM 面向主题对语义；此处保持同实现同测试口径）。 */
export function parseHHMM(s: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(s.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return h * 60 + min;
}

// ---------- 池展开与抽取 ----------

export interface PickContext {
  now: number;
  /** 各池展开出的壁纸 id 列表（IO 由调用方完成——本层纯逻辑）。 */
  poolItems: Record<string, string[]>;
  rand?: () => number;
}

export interface PickResult {
  wallpaperId: string | null;
  /** 三要素：人话原因（池空回退默认/正常抽取/今日已固定）。 */
  reason: string;
  /** 是否走了回退路径。 */
  fellBack: boolean;
}

/** 抽取算法：池内均匀随机 + 近 7 天排除表（排除后池空才允许重复）。 */
export function pickDailyWallpaper(config: DailyWallConfig, ctx: PickContext): PickResult {
  const today = dateKey(ctx.now);
  if (config.pinnedDate === today) {
    return { wallpaperId: config.current, reason: "今日已固定，不更换", fellBack: false };
  }
  const items: string[] = [];
  for (const pool of config.pools) {
    if (!pool.enabled) continue;
    items.push(...(ctx.poolItems[pool.id] ?? []));
  }
  if (items.length === 0) {
    return { wallpaperId: null, reason: "池空（文件夹被清或未选池），请回退默认壁纸并提示", fellBack: true };
  }
  const recentSet = new Set(config.recent.slice(0, EXCLUDE_WINDOW_DAYS));
  const picked = pickExcluding(items, (id) => recentSet.has(id), ctx.rand);
  if (!picked) return { wallpaperId: null, reason: "抽取失败（池为空）", fellBack: true };
  const deduped = recentSet.has(picked) ? "排除表已满，允许重复抽取" : "按近 7 天排除表抽取";
  return { wallpaperId: picked, reason: deduped, fellBack: false };
}

/** 应用抽取结果：历史入账（30 天环形）、recent 排除表更新、次日预载。 */
export function commitDailyPick(config: DailyWallConfig, picked: string, now: number): DailyWallConfig {
  const today = dateKey(now);
  const history = { ...config.history, [today]: picked };
  // 历史 30 天环形清理。
  const keys = Object.keys(history).sort();
  for (let i = 0; i < keys.length - HISTORY_LIMIT_DAYS; i++) delete history[keys[i] ?? ""];
  const recent = [...config.recent, picked].slice(-EXCLUDE_WINDOW_DAYS);
  return { ...config, current: picked, history, recent, pinnedDate: null };
}

/** 「固定今天这张」（当天不换；次日自动解除——commit 时 pinnedDate 覆盖为新日期键即解除）。 */
export function pinToday(config: DailyWallConfig, now: number): DailyWallConfig {
  return { ...config, pinnedDate: dateKey(now) };
}

/** 定时器到点判定：当前时刻过了 changeAt 且今天还没换过 → 触发。 */
export function shouldRotateNow(config: DailyWallConfig, now: number): boolean {
  const t = parseHHMM(config.changeAt);
  if (t === null) return false;
  const today = dateKey(now);
  if (config.history[today]) return false;
  const d = new Date(now);
  const cur = d.getHours() * 60 + d.getMinutes();
  return cur >= t;
}

export function nextRotateAt(config: DailyWallConfig, now: number): number | null {
  const t = parseHHMM(config.changeAt);
  if (t === null) return null;
  return nextDailyFire(now, Math.floor(t / 60), t % 60);
}

// ---------- 构图保护（F147 同算法：自适应裁切，主体不裁头） ----------

export interface CropSpec {
  /** 源图宽高。 */
  srcW: number;
  srcH: number;
  /** 目标区域宽高。 */
  dstW: number;
  dstH: number;
  /** 焦点（主体）相对位置 0..1，默认中心（0.5, 0.42——人像构图主体偏上）。 */
  focusX?: number;
  focusY?: number;
}

/** 计算 cover 裁切窗口：焦点始终保留在窗口内，短边贴满、长边按焦点偏移。 */
export function compositionCrop(spec: CropSpec): { x: number; y: number; w: number; h: number } {
  const srcRatio = spec.srcW / spec.srcH;
  const dstRatio = spec.dstW / spec.dstH;
  const fx = spec.focusX ?? 0.5;
  const fy = spec.focusY ?? 0.42;
  if (srcRatio > dstRatio) {
    // 源更宽：裁左右。
    const w = Math.round(spec.srcH * dstRatio);
    const max = spec.srcW - w;
    const x = Math.round(Math.min(max, Math.max(0, fx * spec.srcW - w / 2)));
    return { x, y: 0, w, h: spec.srcH };
  }
  // 源更高：裁上下（焦点偏上保头部）。
  const h = Math.round(spec.srcW / dstRatio);
  const max = spec.srcH - h;
  const y = Math.round(Math.min(max, Math.max(0, fy * spec.srcH - h / 2)));
  return { x: 0, y, w: spec.srcW, h };
}

/** 20 档分辨率矩阵（构图保护实测口径——判据机械化的档位清单）。 */
export const RESOLUTION_MATRIX: readonly { w: number; h: number; label: string }[] = [
  { w: 1280, h: 720, label: "720p" },
  { w: 1366, h: 768, label: "HD" },
  { w: 1440, h: 900, label: "WXGA+" },
  { w: 1600, h: 900, label: "HD+" },
  { w: 1680, h: 1050, label: "WSXGA+" },
  { w: 1920, h: 1080, label: "FHD" },
  { w: 1920, h: 1200, label: "WUXGA" },
  { w: 2048, h: 1152, label: "QWXGA" },
  { w: 2560, h: 1080, label: "UW-FHD" },
  { w: 2560, h: 1440, label: "QHD" },
  { w: 2560, h: 1600, label: "WQXGA" },
  { w: 3440, h: 1440, label: "UW-QHD" },
  { w: 3840, h: 1600, label: "UW-4K" },
  { w: 3840, h: 2160, label: "4K" },
  { w: 4096, h: 2160, label: "DCI 4K" },
  { w: 5120, h: 1440, label: "Super UW" },
  { w: 5120, h: 2880, label: "5K" },
  { w: 6016, h: 3384, label: "6K" },
  { w: 7680, h: 4320, label: "8K" },
  { w: 1080, h: 1920, label: "竖屏 FHD" },
] as const;
