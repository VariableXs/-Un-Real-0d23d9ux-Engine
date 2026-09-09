/**
 * 壁纸中心（本地 Wallpaper UI，Variable 界面内运行，零 Steam 依赖）核心数据层。
 *
 * 三源合并：当前壁纸（settings.customBg）+ Wallpaper Engine 扫描（wp_engine_scan）
 * + 本地（目录枚举 wp_list_images / 手动添加图片），按 path 去重。
 *
 * 同窗 overlay（portal → body）与 DesktopShell 之间的通信走 window CustomEvent：
 * - `ai04:wallpaper-apply` { detail: { patch: { wallpaperMode, customBg } } }
 *   —— 消费者在 DesktopShell（本计划首次接通，工坊同事件共用同一消费者）；
 * - `wpcenter://open-state` { detail: { open } } —— 中心开合 → DesktopShell
 *   给 WallpaperLayer 下发 suppress（GPU 自律：全屏浮层之下壁纸暂停动画）；
 * - `wpcenter://playlist-changed` —— 中心改播放列表 → DesktopShell runner 重排程。
 *
 * 播放列表/目录/手动图片存 localStorage（与设置存储同源，跨重启保留）。
 */

import type { Shell } from "../../../lib/ipc";

// ---- 事件契约（与 mount.ts 的 ai04:* 协议同风格的 window CustomEvent） ----

export const WP_APPLY_EVENT = "ai04:wallpaper-apply";
export const WP_CENTER_OPEN_STATE = "wpcenter://open-state";
export const WP_CENTER_PLAYLIST_CHANGED = "wpcenter://playlist-changed";

export interface WpApplyPatch {
  wallpaperMode: string;
  customBg?: Record<string, unknown>;
}

/** 派发应用请求（DesktopShell 统一消费；工坊 requestApply 同款协议）。 */
export function dispatchWpApply(patch: WpApplyPatch): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(WP_APPLY_EVENT, { detail: { patch } }));
}

/** 中心开合状态广播（GPU 自律信号）。 */
export function dispatchCenterOpen(open: boolean): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(WP_CENTER_OPEN_STATE, { detail: { open } }));
}

/** 播放列表变更广播（runner 重排程信号）。 */
export function dispatchPlaylistChanged(): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(WP_CENTER_PLAYLIST_CHANGED));
}

// ---- 库项模型 ----

export interface LibraryItem {
  id: string;
  title: string;
  kind: "image" | "video" | "shader" | "web" | "current";
  /** 应用入口路径（image→imagePath；video→videoPath；web→htmlPath；shader→shaderPath） */
  path: string;
  preview: string | null;
  source: string;
  supported: boolean;
}

const MANUAL_KEY = "variable:wpcenter:manual:v1";
const DIRS_KEY = "variable:wpcenter:dirs:v1";

export function loadManualImages(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(MANUAL_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function saveManualImages(list: string[]): void {
  try {
    localStorage.setItem(MANUAL_KEY, JSON.stringify(list));
  } catch {
    /* 存储满：如实丢弃（下次打开仍在内存） */
  }
}

export function loadDirs(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(DIRS_KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function saveDirs(list: string[]): void {
  try {
    localStorage.setItem(DIRS_KEY, JSON.stringify(list));
  } catch {
    /* 同上 */
  }
}

/** WE 扫描项 → 库项（kind 映射；unsupported 保留展示但不可应用）。 */
export function fromEngineItem(e: Shell.WpEngineItem): LibraryItem {
  const kind = e.kind === "video" ? "video"
    : e.kind === "web" ? "web"
    : e.kind === "scene" ? "shader"
    : "image";
  return {
    id: `we:${e.id}`,
    title: e.title || e.id,
    kind,
    path: e.file ?? "",
    preview: e.preview ?? null,
    source: e.source,
    supported: e.supported && !!e.file,
  };
}

/** 本地图片文件 → 库项。 */
export function fromImageFile(p: string, name?: string): LibraryItem {
  const n = name ?? p.split(/[\\/]/).pop() ?? p;
  return {
    id: `img:${p}`,
    title: n,
    kind: "image",
    path: p,
    preview: p,
    source: "local",
    supported: true,
  };
}

/** 当前壁纸 → 库项（固定在最前，selected 默认命中）。 */
export function currentItem(customBg: {
  imagePath: string;
  videoPath: string;
  htmlPath: string;
  shaderPath: string;
}): LibraryItem | null {
  if (customBg.shaderPath) {
    return { id: "cur", title: "当前壁纸", kind: "shader", path: customBg.shaderPath, preview: customBg.imagePath || null, source: "current", supported: true };
  }
  if (customBg.htmlPath) {
    return { id: "cur", title: "当前壁纸", kind: "web", path: customBg.htmlPath, preview: null, source: "current", supported: true };
  }
  if (customBg.videoPath) {
    return { id: "cur", title: "当前壁纸", kind: "video", path: customBg.videoPath, preview: null, source: "current", supported: true };
  }
  if (customBg.imagePath) {
    return { id: "cur", title: "当前壁纸", kind: "image", path: customBg.imagePath, preview: customBg.imagePath, source: "current", supported: true };
  }
  return null;
}

/** 纯合并 + 去重（path 相同保留先到；顺序：current → manual → dir → we）。 */
export function mergeLibrary(items: LibraryItem[]): LibraryItem[] {
  const seen = new Set<string>();
  const out: LibraryItem[] = [];
  for (const it of items) {
    if (it.id === "cur" || seen.has(it.path)) continue;
    seen.add(it.path);
    out.push(it);
  }
  return out;
}

// ---- 播放列表（localStorage 持久；runner 在 DesktopShell） ----

export interface PlaylistEntry {
  title: string;
  kind: "image" | "video" | "web" | "shader";
  path: string;
}

export interface PlaylistState {
  enabled: boolean;
  intervalMin: number;
  shuffle: boolean;
  cursor: number;
}

export const PLAYLIST_KEY = "variable:wpcenter:playlist:v1";
export const PLAYLIST_STATE_KEY = "variable:wpcenter:playlist-state:v1";

export const DEFAULT_PLAYLIST_STATE: PlaylistState = {
  enabled: false,
  intervalMin: 15,
  shuffle: true,
  cursor: 0,
};

export function loadPlaylist(): PlaylistEntry[] {
  try {
    const raw = JSON.parse(localStorage.getItem(PLAYLIST_KEY) ?? "[]");
    return Array.isArray(raw)
      ? raw.filter(
          (e): e is PlaylistEntry =>
            !!e && typeof e.path === "string" && typeof e.kind === "string",
        )
      : [];
  } catch {
    return [];
  }
}

export function savePlaylist(list: PlaylistEntry[]): void {
  try {
    localStorage.setItem(PLAYLIST_KEY, JSON.stringify(list));
  } catch {
    /* 同上 */
  }
  dispatchPlaylistChanged();
}

export function loadPlaylistState(): PlaylistState {
  try {
    return {
      ...DEFAULT_PLAYLIST_STATE,
      ...JSON.parse(localStorage.getItem(PLAYLIST_STATE_KEY) ?? "{}"),
    };
  } catch {
    return DEFAULT_PLAYLIST_STATE;
  }
}

export function savePlaylistState(s: PlaylistState): void {
  try {
    localStorage.setItem(PLAYLIST_STATE_KEY, JSON.stringify(s));
  } catch {
    /* 同上 */
  }
  dispatchPlaylistChanged();
}

/** 纯函数：下一个索引（shuffle 用时间做种——轮换间隔分钟级，够散列；不重复当前）。 */
export function nextIndex(len: number, cursor: number, shuffle: boolean): number {
  if (len <= 0) return 0;
  if (!shuffle) return (cursor + 1) % len;
  if (len === 1) return 0;
  const seed = (Date.now() / 1000) | 0;
  let i = (seed * 2654435761) % len;
  if (i < 0) i += len;
  return i === cursor ? (i + 1) % len : i;
}

/** 播放列表条目 → 应用 patch（kind 与壁纸模式一一对应）。 */
export function entryPatch(e: PlaylistEntry): WpApplyPatch {
  switch (e.kind) {
    case "video":
      return { wallpaperMode: "video", customBg: { videoPath: e.path, playVideo: true } };
    case "web":
      return { wallpaperMode: "web", customBg: { htmlPath: e.path } };
    case "shader":
      return { wallpaperMode: "shader", customBg: { shaderPath: e.path } };
    default:
      return { wallpaperMode: "living", customBg: { imagePath: e.path } };
  }
}
