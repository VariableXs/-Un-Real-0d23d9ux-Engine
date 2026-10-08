/**
 * V-02 五系统图标显隐（此电脑/回收站/网络/用户的文件/控制面板）。
 * localStorage 持久化；默认仅 此电脑 + 回收站 显示（与历史现状一致）。
 * 诚实边界：网络/控制面板打开依赖宿主 Shell 能力，Variable 前端只做如实提示。
 */
export type SysIconId = "explorer" | "recycle" | "network" | "userfiles" | "controlpanel";

export const SYS_ICON_IDS: readonly SysIconId[] = [
  "explorer",
  "recycle",
  "network",
  "userfiles",
  "controlpanel",
];

/** 出厂组合：仅 此电脑 / 回收站 显示（对齐改动前的两图标现状）。 */
export const DEFAULT_SYS_ICON_VIS: Readonly<Record<SysIconId, boolean>> = {
  explorer: true,
  recycle: true,
  network: false,
  userfiles: false,
  controlpanel: false,
};

const LS_KEY = "variable:desktop:sysicons:v1";

export function loadSysIconVis(): Record<SysIconId, boolean> {
  const out: Record<SysIconId, boolean> = { ...DEFAULT_SYS_ICON_VIS };
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) {
      const p = JSON.parse(raw) as Partial<Record<SysIconId, unknown>>;
      for (const id of SYS_ICON_IDS) {
        if (typeof p[id] === "boolean") out[id] = p[id] as boolean;
      }
    }
  } catch {
    /* corrupted → defaults */
  }
  return out;
}

export function saveSysIconVis(v: Record<SysIconId, boolean>): void {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(v));
  } catch {
    /* storage blocked → 不持久化 */
  }
}

/** 「恢复默认」：一键回出厂组合并持久化。 */
export function resetSysIconVis(): Record<SysIconId, boolean> {
  const next = { ...DEFAULT_SYS_ICON_VIS };
  saveSysIconVis(next);
  return next;
}