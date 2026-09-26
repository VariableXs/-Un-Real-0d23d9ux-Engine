/**
 * 显示与声音三件（AI-U1 · F445 显示器排列 / F446 分辨率刷新率 /
 * F447 事件声音试听）——前端生效面。
 *
 * 判据唯一源（主册摘文，与 kernel/varix/src/uni1/ v1 同参数）：
 * - F445「编号对应（临时大号显示 3s）；吸附对齐；主屏设置；拓扑变化后
 *   窗口回流（F353 判据复用）；即时生效」。
 * - F446「支持列表真实性（EDID 读取）；确认倒计时回滚链路；每屏独立；
 *   缩放联动；切换黑屏期 <2s」。
 * - F447「试听即时性；格式/时长校验拒绝用例；默认方案清单；静音测试；
 *   恢复默认一键」。
 */

import { u1Store } from "./u1store";

/* ------------------------------- F445 显示器排列 ------------------------------- */

export const SNAP_THRESHOLD_PX = 8;
export const NUMBER_OVERLAY_MS = 3000;

export interface ScreenBlock { id: number; x: number; y: number; w: number; h: number; primary: boolean }

/** 吸附对齐（与内核 disparrange 同式：四缘 ±8px 贴齐）。 */
export function snapDrag(screens: ScreenBlock[], id: number, x: number, y: number): { x: number; y: number } | null {
  const self = screens.find((s) => s.id === id);
  if (!self) return null;
  let sx = x;
  let sy = y;
  for (const other of screens) {
    if (other.id === id) continue;
    if (Math.abs(x - (other.x + other.w)) <= SNAP_THRESHOLD_PX) sx = other.x + other.w;
    else if (Math.abs(x + self.w - other.x) <= SNAP_THRESHOLD_PX) sx = other.x - self.w;
    if (Math.abs(y - (other.y + other.h)) <= SNAP_THRESHOLD_PX) sy = other.y + other.h;
    else if (Math.abs(y + self.h - other.y) <= SNAP_THRESHOLD_PX) sy = other.y - self.h;
  }
  return { x: sx, y: sy };
}

/** 主屏互斥（判据：有且仅有一个主屏）。 */
export function setPrimary(screens: ScreenBlock[], id: number): boolean {
  if (!screens.some((s) => s.id === id)) return false;
  for (const s of screens) s.primary = s.id === id;
  return true;
}

/** 窗口回流（F353）：屏拔除 → 相对位置映射到存活屏并钳制。 */
export function windowRehome(screens: ScreenBlock[], deadId: number, rel: { x: number; y: number }): { x: number; y: number } | null {
  const dead = screens.find((s) => s.id === deadId);
  const alive = screens.find((s) => s.id !== deadId);
  if (!dead || !alive) return null;
  const ax = alive.x + Math.round(rel.x * dead.w);
  const ay = alive.y + Math.round(rel.y * dead.h);
  return { x: Math.min(alive.x + alive.w - 1, Math.max(alive.x, ax)), y: Math.min(alive.y + alive.h - 1, Math.max(alive.y, ay)) };
}

/* ------------------------------- F446 分辨率与刷新率 ------------------------------- */

export const CONFIRM_WINDOW_MS = 15000;
export const BLACKOUT_BUDGET_MS = 2000;

export interface ModeCombo { width: number; height: number; refreshCentiHz: number }

export function modeLabel(m: ModeCombo): string {
  const hz = m.refreshCentiHz % 100 === 0 ? `${m.refreshCentiHz / 100}Hz` : `${(m.refreshCentiHz / 100).toFixed(2)}Hz`;
  return `${m.width}×${m.height} @ ${hz}`;
}

/** 支持列表真实性：表外组合拒绝（不给人选错的坑）。 */
export function canSelect(supported: ModeCombo[], m: ModeCombo): boolean {
  return supported.some((s) => s.width === m.width && s.height === m.height && s.refreshCentiHz === m.refreshCentiHz);
}

/** 游戏向快捷标记：支持表中刷新率最高。 */
export function gamingPick(supported: ModeCombo[]): ModeCombo | null {
  if (supported.length === 0) return null;
  return supported.reduce((a, b) => (b.refreshCentiHz > a.refreshCentiHz ? b : a));
}

/** 倒计时推进：归零 → 自动回滚原组合（自救）。返回回滚到的组合或 null。 */
export function resTick(pending: { combo: ModeCombo; remainMs: number } | null, current: ModeCombo, elapsedMs: number, rollbacks: { n: number }): ModeCombo | null {
  if (!pending) return null;
  pending.remainMs -= elapsedMs;
  if (pending.remainMs <= 0) {
    rollbacks.n += 1;
    return current;
  }
  return null;
}

/** 缩放联动建议（F224）：4K → 150%；2K → 125%；其余 100%。 */
export function scaleSuggestion(m: ModeCombo): number {
  if (m.height >= 2160) return 150;
  if (m.height >= 1440) return 125;
  return 100;
}

/* ------------------------------- F447 事件声音试听 ------------------------------- */

export function customMaxMs(): number {
  const cfg = u1Store.get<{ customMaxMs?: number }>("displayAv");
  return cfg.customMaxMs ?? 3000;
}

/** F079 默认六事件（与内核 DEFAULT_SCHEME 同表）。 */
export const DEFAULT_SCHEME: readonly (readonly [string, string])[] = [
  ["notify", "默认-叮咚"],
  ["device-in", "默认-上行琶音"],
  ["device-out", "默认-下行琶音"],
  ["low-battery", "默认-低语提示"],
  ["error", "默认-钝响"],
  ["empty-trash", "默认-碎纸声"],
];

/** WAV 校验（RIFF/WAVE 签名 + 时长上限——防闹铃党；与内核同式）。 */
export function validateCustomWav(header: Uint8Array, dataBytes: number): { ok: true } | { ok: false; reason: string } {
  const isWav = header.length >= 12
    && header[0] === 0x52 && header[1] === 0x49 && header[2] === 0x46 && header[3] === 0x46
    && header[8] === 0x57 && header[9] === 0x41 && header[10] === 0x56 && header[11] === 0x45;
  if (!isWav) return { ok: false, reason: "不是有效的 WAV 文件——请用 16 位 PCM WAV" };
  const durationMs = Math.floor((dataBytes - 44) * 1000 / (44100 * 2));
  if (durationMs <= 0 || durationMs > customMaxMs()) return { ok: false, reason: "音频时长超出 3 秒限制——系统提示音要短促" };
  return { ok: true };
}

/** 试听（判据：不满意不落定——试听不改绑定，由调用方保证）。 */
export function audition(bindings: Map<string, string>, event: string): string | null {
  return bindings.get(event) ?? null;
}

/** 恢复默认一键。 */
export function resetScheme(bindings: Map<string, string>): void {
  for (const [ev, snd] of DEFAULT_SCHEME) bindings.set(ev, snd);
}
