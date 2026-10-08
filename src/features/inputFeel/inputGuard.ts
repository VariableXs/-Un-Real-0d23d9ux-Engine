/**
 * UNREAL-X AI-18 · 族0178 输入安全 2.0 + 族0179 按键映射 2.0 + 族0180 外设键盘
 * （X04426~X04500）。
 *
 * 输入安全：按键脱敏/敏感窗口拦截/剪贴板哨兵/键盘记录防护（防侧录语义）。
 * 按键映射：层（layer）/复合键（leader）/映射表钳制/冲突检测。
 * 外设键盘：多键盘设备画像/布局识别/每设备手感档。
 */

/* ============================== 族0178 输入安全 2.0 ============================== */

export const INPUT_SECURITY_PROFILES = [
  { id: "off", name: "关闭", maskFields: false, guardClipboard: false, blockLoggers: false },
  { id: "basic", name: "基础", maskFields: true, guardClipboard: false, blockLoggers: false },
  { id: "balanced", name: "均衡", maskFields: true, guardClipboard: true, blockLoggers: true },
  { id: "strict", name: "严格", maskFields: true, guardClipboard: true, blockLoggers: true },
  { id: "vault", name: "保险箱", maskFields: true, guardClipboard: true, blockLoggers: true },
] as const;
export type InputSecurityProfileId = (typeof INPUT_SECURITY_PROFILES)[number]["id"];
export const DEFAULT_INPUT_SECURITY_ID: InputSecurityProfileId = "balanced";

export function findInputSecurity(id: string): (typeof INPUT_SECURITY_PROFILES)[number] {
  return INPUT_SECURITY_PROFILES.find((p) => p.id === id) ?? INPUT_SECURITY_PROFILES[2]!;
}

/** 敏感字段语义（密码/卡号/验证码）：字段名启发式判定。 */
const SENSITIVE_HINTS = ["password", "passwd", "pwd", "secret", "token", "card", "cvv", "otp", "密码", "卡号", "验证码"];

export function isSensitiveField(label: string): boolean {
  const l = label.toLowerCase();
  return SENSITIVE_HINTS.some((h) => l.includes(h));
}

/** 按键脱敏：敏感上下文中把可打印键替换为掩码（保留控制键通路）。 */
export function maskKeystroke(key: string, sensitive: boolean): string {
  if (!sensitive) return key;
  if (key.length === 1) return "•";
  return key;
}

/** 剪贴板哨兵：敏感值复制时给出 N 秒自动清空与脱敏预览。 */
export function clipboardGuard(value: string, profileId: string): { masked: string; autoClearSec: number; guarded: boolean } {
  const p = findInputSecurity(profileId);
  if (!p.guardClipboard || !isSensitiveField("") && !/^\d{6,}$/.test(value)) {
    return { masked: value, autoClearSec: 0, guarded: false };
  }
  const masked = value.length <= 2 ? "••" : `${value.slice(0, 2)}${"•".repeat(Math.max(0, value.length - 4))}${value.slice(-2)}`;
  return { masked, autoClearSec: 30, guarded: true };
}

/** 键盘记录防护：把全局按键事件按会话聚合，敏感窗内零明文落盘。 */
export class KeystrokePrivacy {
  profileId: InputSecurityProfileId;
  /** 会话内计数（只计数不存明文）。 */
  counted = 0;
  sensitiveCounted = 0;
  clamped = 0;

  constructor(profileId: string = DEFAULT_INPUT_SECURITY_ID) {
    const known = INPUT_SECURITY_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findInputSecurity(profileId).id as InputSecurityProfileId;
  }

  feed(key: string, sensitive: boolean): void {
    const p = findInputSecurity(this.profileId);
    if (p.maskFields && sensitive) {
      this.sensitiveCounted += 1;
      void maskKeystroke(key, true); // 明文即刻掩码，不入任何缓冲
      return;
    }
    this.counted += 1;
  }

  reset(): void {
    this.counted = 0;
    this.sensitiveCounted = 0;
    this.clamped = 0;
  }
}

/* ============================== 族0179 按键映射 2.0 ============================== */

export const KEYMAP_PROFILES = [
  { id: "single", name: "单层", layers: 1, leader: false },
  { id: "dual", name: "双层", layers: 2, leader: false },
  { id: "balanced", name: "均衡", layers: 3, leader: true },
  { id: "pro", name: "专业", layers: 4, leader: true },
  { id: "full", name: "全层", layers: 6, leader: true },
] as const;
export type KeymapProfileId = (typeof KEYMAP_PROFILES)[number]["id"];
export const DEFAULT_KEYMAP_ID: KeymapProfileId = "balanced";

export function findKeymap(id: string): (typeof KEYMAP_PROFILES)[number] {
  return KEYMAP_PROFILES.find((p) => p.id === id) ?? KEYMAP_PROFILES[2]!;
}

/** 层选择：Fn/激活键决定当前层（层号钳制在档位内）。 */
export class LayerSelector {
  profileId: KeymapProfileId;
  active = 0;
  clamped = 0;

  constructor(profileId: string = DEFAULT_KEYMAP_ID) {
    const known = KEYMAP_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findKeymap(profileId).id as KeymapProfileId;
  }

  get maxLayer(): number {
    return findKeymap(this.profileId).layers - 1;
  }

  /** 切层：越界回 0 层（计数钳制）。 */
  activate(layer: number): number {
    if (layer < 0 || layer > this.maxLayer) {
      this.clamped += 1;
      this.active = 0;
      return 0;
    }
    this.active = layer;
    return layer;
  }
}

export interface KeyRemap { from: string; to: string; layer?: number }

/** 映射表解析：命中映射则改写；leader 复合键（前缀+后续键）。 */
export class KeyRemapper {
  profileId: KeymapProfileId;
  remaps: KeyRemap[] = [];
  leaderKey: string | null = null;
  leaderArmed = false;
  conflicts: string[] = [];
  clamped = 0;

  constructor(profileId: string = DEFAULT_KEYMAP_ID, remaps: KeyRemap[] = [], leaderKey: string | null = null) {
    const known = KEYMAP_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findKeymap(profileId).id as KeymapProfileId;
    const maxL = findKeymap(profileId).layers - 1;
    this.remaps = remaps
      .filter((r) => r.from && r.to)
      .map((r) => ({ ...r, layer: Math.max(0, Math.min(maxL, r.layer ?? 0)) }));
    const seen = new Set<string>();
    for (const r of this.remaps) {
      const id = `${r.layer}:${r.from}`;
      if (seen.has(id)) {
        this.conflicts.push(id);
      }
      seen.add(id);
    }
    const p = findKeymap(profileId);
    this.leaderKey = p.leader ? leaderKey : null;
  }

  /** 解析一次按键；leader 前缀武装后吞掉第一键。 */
  resolve(key: string, layer = 0): { emit: string; swallowed: boolean } {
    const l = Math.max(0, Math.min(this.maxLayer, layer));
    if (this.leaderKey && key === this.leaderKey && !this.leaderArmed) {
      this.leaderArmed = true;
      return { emit: "", swallowed: true };
    }
    if (this.leaderArmed) {
      this.leaderArmed = false;
      const hit = this.remaps.find((r) => r.from === `leader:${key}` && r.layer === l);
      return hit ? { emit: hit.to, swallowed: false } : { emit: key, swallowed: false };
    }
    const hit = this.remaps.find((r) => r.from === key && r.layer === l);
    return hit ? { emit: hit.to, swallowed: false } : { emit: key, swallowed: false };
  }

  get maxLayer(): number {
    return findKeymap(this.profileId).layers - 1;
  }

  reset(): void {
    this.leaderArmed = false;
    this.clamped = 0;
  }
}

/* ============================== 族0180 外设键盘 ============================== */

export interface KeyboardDevice {
  id: string;
  name: string;
  layout: "ANSI" | "ISO" | "JIS" | "unknown";
  wireless: boolean;
  /** 无线报告率 Hz（有线固定 1000）。 */
  pollingHz: number;
}

/** 外设键盘五档（按设备的输入延迟优先级）。 */
export const PERIPHERAL_PROFILES = [
  { id: "eco", name: "省电", wirelessHz: 125, wiredHz: 500, activePenaltyMs: 0 },
  { id: "light", name: "轻量", wirelessHz: 250, wiredHz: 1000, activePenaltyMs: 0 },
  { id: "balanced", name: "均衡", wirelessHz: 500, wiredHz: 1000, activePenaltyMs: 0 },
  { id: "fast", name: "极速", wirelessHz: 1000, wiredHz: 1000, activePenaltyMs: 0 },
  { id: "game", name: "电竞", wirelessHz: 8000, wiredHz: 1000, activePenaltyMs: 0 },
] as const;
export type PeripheralProfileId = (typeof PERIPHERAL_PROFILES)[number]["id"];
export const DEFAULT_PERIPHERAL_ID: PeripheralProfileId = "balanced";

export function findPeripheral(id: string): (typeof PERIPHERAL_PROFILES)[number] {
  return PERIPHERAL_PROFILES.find((p) => p.id === id) ?? PERIPHERAL_PROFILES[2]!;
}

/** 设备报告率：按档位与有线/无线决定（越界钳制）。 */
export function pollingRate(device: KeyboardDevice, profileId: string): number {
  const p = findPeripheral(profileId);
  if (!device.wireless) return Math.min(p.wiredHz, Math.max(125, device.pollingHz || p.wiredHz));
  return Math.max(31, Math.min(p.wirelessHz, device.pollingHz || p.wirelessHz));
}

/** 设备手感路由：给每个键盘分配档位（active 设备用全局档，其余用省电）。 */
export function deviceFeelRouting(devices: KeyboardDevice[], activeId: string | null, profileId: string): Map<string, PeripheralProfileId> {
  const m = new Map<string, PeripheralProfileId>();
  for (const d of devices) {
    m.set(d.id, activeId === d.id ? (findPeripheral(profileId).id as PeripheralProfileId) : "eco");
  }
  return m;
}

/** 布局识别启发式：按键名 → ANSI/ISO/JIS（回车键形描述）。 */
export function detectLayout(enterKeyShape: "wide" | "L-shape" | "upside-down-L", extraKey: boolean): KeyboardDevice["layout"] {
  if (enterKeyShape === "wide") return "ANSI";
  if (enterKeyShape === "upside-down-L") return "JIS";
  if (enterKeyShape === "L-shape") return extraKey ? "ISO" : "ANSI";
  return "unknown";
}

/** 每设备叙事：外设状态一句话。 */
export function peripheralNarrative(d: KeyboardDevice, hz: number): string {
  const kind = d.wireless ? "无线" : "有线";
  if (hz >= 1000) return `${d.name}：${kind} ${hz}Hz，全速输入。`;
  if (hz >= 250) return `${d.name}：${kind} ${hz}Hz，流畅。`;
  return `${d.name}：${kind} ${hz}Hz，省电模式。`;
}
