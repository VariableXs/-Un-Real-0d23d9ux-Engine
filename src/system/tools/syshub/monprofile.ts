/**
 * AI-11 U-43 多显示器布局档案（纯逻辑模块）。
 *
 * 档案 = 一次快照：device → { x, y, w, h }。重接入同名显示器时
 * 由「恢复建议」输出当前布局与档案的差异（只读提示，不代用户改系统设置）。
 */

export interface MonRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface MonProfile {
  at: number;
  rects: Record<string, MonRect>;
}

const KEY = "variable:ai11:monprofile";

export function loadMonProfile(): MonProfile | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const p = JSON.parse(raw) as MonProfile;
    return p && typeof p.rects === "object" ? p : null;
  } catch {
    return null;
  }
}

/** 保存当前布局快照（显式按钮触发）。 */
export function saveMonProfile(mons: { device: string; x: number; y: number; w: number; h: number }[], nowMs: number): MonProfile {
  const rects: Record<string, MonRect> = {};
  for (const m of mons) rects[m.device] = { x: m.x, y: m.y, w: m.w, h: m.h };
  const p: MonProfile = { at: nowMs, rects };
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {
    /* storage blocked */
  }
  return p;
}

export function clearMonProfile(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* storage blocked */
  }
}

/** 与档案差异：返回 device + kind（moved/resized/missing/added）列表；空 = 一致。 */
export function diffMonProfile(
  profile: MonProfile,
  mons: { device: string; x: number; y: number; w: number; h: number }[],
): { device: string; kind: "moved" | "resized" | "missing" | "added" }[] {
  const out: { device: string; kind: "moved" | "resized" | "missing" | "added" }[] = [];
  for (const m of mons) {
    const saved = profile.rects[m.device];
    if (!saved) {
      out.push({ device: m.device, kind: "added" });
    } else if (saved.x !== m.x || saved.y !== m.y) {
      out.push({ device: m.device, kind: "moved" });
    } else if (saved.w !== m.w || saved.h !== m.h) {
      out.push({ device: m.device, kind: "resized" });
    }
  }
  const cur = new Set(mons.map((m) => m.device));
  for (const dev of Object.keys(profile.rects)) {
    if (!cur.has(dev)) out.push({ device: dev, kind: "missing" });
  }
  return out;
}
