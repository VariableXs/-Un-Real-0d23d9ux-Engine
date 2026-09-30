/**
 * U-13 桌面配置（Profiles）：壁纸 + 图标布局 + 图标大小的可切换快照。
 * - 持久化 localStorage（variable:desktop:profiles:v1），逐项 JSON 往返；
 * - 应用走 window 事件（APPLY_EVENT）交 DesktopShell 执行：patch settings
 *   （壁纸）+ saveDesktopLayout（布局回写）+ iconPx 记忆 + 通知桌面重载；
 *   本模块只管存取，不做副作用（便于单测与解耦）。
 * 诚实边界：快捷键（hotkey）仅登记在配置内 —— ipc.shortcutsApply 无 profile
 *   通道，不注册系统热键；面板内明示此边界。
 */
import type { CustomBg, WallpaperMode } from "../../lib/settings";
import type { DesktopLayout } from "../desktop-icons/layout";

export interface DesktopProfile {
  id: string;
  name: string;
  wallpaper: { mode: WallpaperMode; customBg: CustomBg };
  /** 图标 px 记忆（null = 未记录，应用时不动图标大小）。 */
  iconPx: number | null;
  /** 深拷贝布局快照（positions/shelves/sort/locked/autoArrange）。 */
  iconLayout: DesktopLayout;
  /** 预留：开始菜单顺序（当前 Variable 开始菜单无自定义顺序，仅登记）。 */
  startOrder?: string[];
  /** 仅登记（系统热键通道未接入，见文件头）。 */
  hotkey?: string;
  ts: number;
}

const LS_KEY = "variable:desktop:profiles:v1";

/** 应用事件：detail = DesktopProfile，由 DesktopShell 监听执行。 */
export const APPLY_EVENT = "ai04:apply-profile";
/** 打开管理面板事件：detail = { mode: "save" | "manage" }。 */
export const OPEN_EVENT = "ai04:open-profiles";
/** 应用后通知 DesktopIcons 重载布局/px。 */
export const RELOAD_LAYOUT_EVENT = "ai04:reload-layout";

export function newProfileId(): string {
  return `profile-${Date.now().toString(36)}-${Math.floor(Math.random() * 1e6).toString(36)}`;
}

/** 深拷贝布局快照（JSON 往返，切断与活布局的引用共享）。 */
export function snapshotLayout(l: DesktopLayout): DesktopLayout {
  return JSON.parse(JSON.stringify(l)) as DesktopLayout;
}

function decodeList(raw: string | null): DesktopProfile[] {
  if (!raw) return [];
  try {
    const p = JSON.parse(raw) as unknown;
    if (!Array.isArray(p)) return [];
    const out: DesktopProfile[] = [];
    for (const it of p) {
      const o = it as Partial<DesktopProfile>;
      if (typeof o !== "object" || o === null) continue;
      if (typeof o.id !== "string" || typeof o.name !== "string") continue;
      if (!o.wallpaper || typeof o.wallpaper.mode !== "string" || !o.wallpaper.customBg) continue;
      if (!o.iconLayout || typeof o.iconLayout !== "object") continue;
      out.push({
        id: o.id,
        name: o.name,
        wallpaper: { mode: o.wallpaper.mode as WallpaperMode, customBg: o.wallpaper.customBg as CustomBg },
        iconPx: typeof o.iconPx === "number" && Number.isFinite(o.iconPx) ? o.iconPx : null,
        iconLayout: o.iconLayout as DesktopLayout,
        startOrder: Array.isArray(o.startOrder) ? o.startOrder.filter((x): x is string => typeof x === "string") : undefined,
        hotkey: typeof o.hotkey === "string" ? o.hotkey : undefined,
        ts: typeof o.ts === "number" ? o.ts : 0,
      });
    }
    return out;
  } catch {
    return []; // corrupted → 如当空列表（不抛错）
  }
}

export function listProfiles(): DesktopProfile[] {
  try {
    return decodeList(localStorage.getItem(LS_KEY));
  } catch {
    return [];
  }
}

/** 新增/覆盖保存（带 id 即覆盖同 id；否则新 id 追加）。返回保存后的完整档。 */
export function saveProfile(
  p: Omit<DesktopProfile, "id" | "ts"> & { id?: string },
): DesktopProfile {
  const full: DesktopProfile = { ...p, id: p.id ?? newProfileId(), ts: Date.now() };
  const rest = listProfiles().filter((x) => x.id !== full.id);
  try {
    localStorage.setItem(LS_KEY, JSON.stringify([...rest, full]));
  } catch {
    /* storage blocked → 不持久化（本次会话内仍可用） */
  }
  return full;
}

export function deleteProfile(id: string): void {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(listProfiles().filter((x) => x.id !== id)));
  } catch {
    /* storage blocked → 不持久化 */
  }
}

/** 找档（应用入口用）。 */
export function getProfile(id: string): DesktopProfile | null {
  return listProfiles().find((x) => x.id === id) ?? null;
}

export interface ProfileDiff {
  field: string;
  before: unknown;
  after: unknown;
}

/** 比较两份配置的差异（管理面板展示 / 单测断言用）。 */
export function diffProfile(a: DesktopProfile, b: DesktopProfile): ProfileDiff[] {
  const out: ProfileDiff[] = [];
  const push = (field: string, x: unknown, y: unknown): void => {
    if (JSON.stringify(x) !== JSON.stringify(y)) out.push({ field, before: x, after: y });
  };
  push("name", a.name, b.name);
  push("wallpaper.mode", a.wallpaper.mode, b.wallpaper.mode);
  push("wallpaper.customBg", a.wallpaper.customBg, b.wallpaper.customBg);
  push("iconPx", a.iconPx, b.iconPx);
  push("layout.autoArrange", a.iconLayout.autoArrange, b.iconLayout.autoArrange);
  push("layout.sort", a.iconLayout.sort, b.iconLayout.sort);
  push("layout.locked", a.iconLayout.locked ?? false, b.iconLayout.locked ?? false);
  push("layout.positions", a.iconLayout.positions, b.iconLayout.positions);
  push("layout.shelves", a.iconLayout.shelves, b.iconLayout.shelves);
  return out;
}