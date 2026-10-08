/**
 * AURORA-10000 领域04 · 族0090 快捷面板（AI-18 批次，勿删）。
 * 开关区登记/滑杆/自定义布局/材质与动画参数。
 */
import { getD4 } from "./prefs";

export type QuickToggleId =
  | "wifi" | "bluetooth" | "airplane" | "mic" | "camera" | "record" | "snip"
  | "nightlight" | "eyesaver" | "eco" | "perf" | "focus" | "project" | "hotspot"
  | "vpn" | "clip-sync" | "denoise";

export interface QuickToggle {
  id: QuickToggleId;
  label: string;
  on: boolean;
  group: "connect" | "media" | "system";
}

export interface QuickSlider {
  id: "brightness" | "volume" | "colortemp";
  label: string;
  value: number; // 0~100
}

/** 开关注册表（F02226/30~41）。 */
export const QUICK_TOGGLES: readonly QuickToggle[] = [
  { id: "wifi", label: "Wi-Fi", on: true, group: "connect" },
  { id: "bluetooth", label: "蓝牙", on: false, group: "connect" },
  { id: "airplane", label: "飞行模式", on: false, group: "connect" },
  { id: "hotspot", label: "热点", on: false, group: "connect" },
  { id: "vpn", label: "VPN", on: false, group: "connect" },
  { id: "mic", label: "麦克风", on: true, group: "media" },
  { id: "camera", label: "摄像头", on: false, group: "media" },
  { id: "record", label: "录屏", on: false, group: "media" },
  { id: "snip", label: "截屏", on: false, group: "media" },
  { id: "denoise", label: "降噪", on: false, group: "media" },
  { id: "nightlight", label: "夜灯", on: false, group: "system" },
  { id: "eyesaver", label: "护眼", on: false, group: "system" },
  { id: "eco", label: "省电", on: false, group: "system" },
  { id: "perf", label: "性能", on: false, group: "system" },
  { id: "focus", label: "专注", on: false, group: "system" },
  { id: "project", label: "投屏", on: false, group: "system" },
  { id: "clip-sync", label: "剪贴同步", on: false, group: "system" },
];

/** 自定义布局（F02246）：把指定 id 提到前排。 */
export function customOrder(toggles: readonly QuickToggle[], front: readonly QuickToggleId[]): QuickToggle[] {
  const rank = new Map(front.map((id, i) => [id, i]));
  return [...toggles].sort((a, b) => (rank.get(a.id) ?? 99) - (rank.get(b.id) ?? 99));
}

/** 面板参数（F02247 毛玻璃 / F02248 动画）。 */
export function panelStyle(): { glass: boolean; animated: boolean } {
  return { glass: getD4<boolean>("F02247") ?? true, animated: getD4<boolean>("F02248") ?? true };
}
