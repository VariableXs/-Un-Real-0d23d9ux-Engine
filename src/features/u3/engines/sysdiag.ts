/**
 * F540-F547 深化引擎 · 系统与设备八件（AI-U3 · sysdiag）。
 *
 * 判据唯一源（主册摘文）：
 * - F540 诊断报告三要素；F541 网络重置清单完整性；F542 ClickLock 阈值；
 * - F543 分设备音量跟随；F544 双滑杆独立；F545 电量三处同源；
 * - F546 接入通知三态；F547 平衡试听实时。
 *
 * 深化点：
 * 1. 诊断运行器：检查项 → 三要素报告（发生了什么/为什么/下一步），不裸抛
 *    错误码（九章纪律的引擎化）。
 * 2. 网络重置 90s 倒计时账：步骤清单完整性（适配器/DNS/Winsock/路由四族）
 *    + 可取消窗口。
 * 3. ClickLock 阈值验证器：1100ms 基线 ± 实测容差，边界三点钉死。
 * 4. 电量三处同源对账：状态栏/设置页/诊断页同值；跳变平滑（F545 缓变）。
 * 5. 设备接入三态通知（就绪/驱动缺失/初始化中）+ 2s 就绪驻留判据。
 */

/* ------------------------------ F540 诊断三要素 ------------------------------ */

export interface DiagFinding {
  code: string;
  what: string;
  why: string;
  next: string;
  severity: "info" | "warn" | "error";
}

/** 三要素完整校验（判据「诊断报告三要素」——缺一要素即报告缺陷）。 */
export function diagFindingComplete(f: DiagFinding): boolean {
  return f.what.length > 0 && f.why.length > 0 && f.next.length > 0;
}

/** 诊断运行器：把原始检查结果转成三要素报告（错误码收进详情——九章）。 */
export function toDiagReport(raw: Array<{ code: string; ok: boolean; detail: string }>): Array<DiagFinding> {
  return raw.map((r) => ({
    code: r.code,
    severity: r.ok ? "info" : "error",
    what: r.ok ? `${r.code} 检查通过` : `${r.code} 检查未通过：${r.detail}`,
    why: r.ok ? "该组件状态正常" : "组件自检读数超出正常范围",
    next: r.ok ? "无需处理" : "按「下一步」建议处理；技术细节可在详情中展开",
  }));
}

/* ------------------------------ F541 网络重置 ------------------------------ */

export const NET_RESET_COUNTDOWN_SEC = 90;
/** 重置步骤清单（判据「清单完整性」：四族步骤在册）。 */
export const NET_RESET_STEPS = [
  "禁用并重新启用网络适配器",
  "重置 DNS 缓存",
  "重置 Winsock 目录",
  "清除并重建路由表默认项",
] as const;

/** 步骤完整性审计（缺一步骤=审计红）。 */
export function netResetStepsComplete(executed: readonly string[]): { complete: boolean; missing: string[] } {
  const missing = NET_RESET_STEPS.filter((s) => !executed.includes(s));
  return { complete: missing.length === 0, missing: [...missing] };
}

/** 倒计时可取消窗口（判据「countdownSec 90」：取消在窗口内即时生效）。 */
export function netResetCancellable(elapsedSec: number): boolean {
  return elapsedSec < NET_RESET_COUNTDOWN_SEC;
}

/* ------------------------------ F542 ClickLock ------------------------------ */

export const CLICKLOCK_THRESHOLD_MS = 1100;

/** 阈值验证（判据「ClickLock 阈值」）：按住 ≥1100ms 激活，边界三点。 */
export function clickLockActivate(holdMs: number, enabled: boolean): boolean {
  return enabled && holdMs >= CLICKLOCK_THRESHOLD_MS;
}

/* ------------------------------ F543/F544 音量 ------------------------------ */

export interface DeviceVolumeEntry {
  deviceId: string;
  volume: number;
  /** 跟随主音量比例（判据「分设备音量跟随」）。 */
  followMaster: boolean;
}

/** 主音量变化时跟随设备同步（不跟随的设备独立）。 */
export function applyMasterVolume(devices: DeviceVolumeEntry[], masterDelta: number): DeviceVolumeEntry[] {
  return devices.map((d) =>
    d.followMaster
      ? { ...d, volume: Math.min(100, Math.max(0, d.volume + masterDelta)) }
      : d,
  );
}

/** 双滑杆独立（判据「双滑杆独立」）：主音量与通知音量互不牵连。 */
export function dualSliderIndependent(master: number, notify: number, masterDelta: number): { master: number; notify: number; independent: boolean } {
  return { master: Math.min(100, Math.max(0, master + masterDelta)), notify, independent: true };
}

/* ------------------------------ F545 电量同源 ------------------------------ */

export interface BatteryReading {
  pct: number;
  source: "statusbar" | "settings" | "diag";
}

/** 三处同源对账（判据「电量三处同源」）。 */
export function batterySourcesAgree(readings: BatteryReading[]): { agree: boolean; value: number | null } {
  if (readings.length === 0) return { agree: true, value: null };
  const v = readings[0]!.pct;
  return { agree: readings.every((r) => r.pct === v), value: v };
}

/** 电量跳变平滑（F545 缓变——单次上报变化超过 smoothStep 时分步缓报）。 */
export function smoothBatteryStep(previous: number, raw: number, smoothStep: number): number {
  const delta = raw - previous;
  if (Math.abs(delta) <= smoothStep) return raw;
  return previous + Math.sign(delta) * smoothStep;
}

/* ------------------------------ F546 接入三态 ------------------------------ */

export type DeviceArrivalState = "initializing" | "ready" | "driver-missing";
export const DEVICE_READY_DWELL_MS = 2000;

/** 接入通知三态（判据「接入通知三态」+ 2s 就绪驻留）。 */
export function deviceArrivalNotice(state: DeviceArrivalState, dwellMs: number): { notify: boolean; text: string } {
  if (state === "driver-missing") return { notify: true, text: "设备已接入，但驱动缺失——打开诊断中心查看下一步" };
  if (state === "initializing") return { notify: false, text: "" }; // 初始化中不扰——就绪再报
  return { notify: dwellMs >= DEVICE_READY_DWELL_MS, text: dwellMs >= DEVICE_READY_DWELL_MS ? "设备就绪，可以使用" : "" };
}

/* ------------------------------ F547 平衡试听 ------------------------------ */

export const BALANCE_RANGE = { min: -100, max: 100 } as const;

/** 平衡值钳制（-100 全左 / +100 全右）。 */
export function clampBalance(pan: number): number {
  return Math.min(BALANCE_RANGE.max, Math.max(BALANCE_RANGE.min, pan));
}

/** 双声道增益（试听实时：pan → 左右增益比例，线性/等功率折中）。 */
export function channelGains(pan: number): { left: number; right: number } {
  const p = clampBalance(pan) / 100;
  return { left: Math.cos(((p + 1) * Math.PI) / 4), right: Math.sin(((p + 1) * Math.PI) / 4) };
}

/* ------------------------------ 自检 ------------------------------ */

export function sysdiagSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 三要素：缺要素判缺陷
  checks.push({ name: "F540 三要素校验", pass: diagFindingComplete({ code: "D1", what: "a", why: "b", next: "c", severity: "info" }) && !diagFindingComplete({ code: "D1", what: "a", why: "", next: "c", severity: "info" }) });
  // 报告器：失败项转 error 级
  const rep = toDiagReport([{ code: "NET", ok: false, detail: "DNS 无响应" }]);
  checks.push({ name: "F540 报告器三要素", pass: rep.length === 1 && rep[0]!.severity === "error" && rep[0]!.next.length > 0 });
  // 重置清单完整性
  const partial = netResetStepsComplete(["禁用并重新启用网络适配器", "重置 DNS 缓存"]);
  checks.push({ name: "F541 清单完整性审计", pass: !partial.complete && partial.missing.length === 2 && netResetStepsComplete([...NET_RESET_STEPS]).complete });
  // 取消窗口
  checks.push({ name: "F541 90s 取消窗口", pass: netResetCancellable(89) && !netResetCancellable(90) });
  // ClickLock 边界三点
  checks.push({ name: "F542 1100ms 边界三点", pass: !clickLockActivate(1099, true) && clickLockActivate(1100, true) && clickLockActivate(1101, true) && !clickLockActivate(1100, false) });
  // 分设备跟随
  const devs: DeviceVolumeEntry[] = [
    { deviceId: "spk", volume: 50, followMaster: true },
    { deviceId: "bt", volume: 30, followMaster: false },
  ];
  const after = applyMasterVolume(devs, +10);
  checks.push({ name: "F543 分设备跟随", pass: after[0]!.volume === 60 && after[1]!.volume === 30 });
  // 双滑杆独立
  const d = dualSliderIndependent(50, 40, +20);
  checks.push({ name: "F544 双滑杆独立", pass: d.master === 70 && d.notify === 40 });
  // 电量三处同源 + 平滑
  checks.push({ name: "F545 三处同源", pass: batterySourcesAgree([{ pct: 80, source: "statusbar" }, { pct: 80, source: "settings" }, { pct: 80, source: "diag" }]).agree && !batterySourcesAgree([{ pct: 80, source: "statusbar" }, { pct: 79, source: "diag" }]).agree });
  checks.push({ name: "F545 跳变平滑", pass: smoothBatteryStep(50, 60, 2) === 52 && smoothBatteryStep(50, 51, 2) === 51 });
  // 接入三态
  checks.push({ name: "F546 接入三态", pass: deviceArrivalNotice("initializing", 3000).notify === false && deviceArrivalNotice("ready", 1999).notify === false && deviceArrivalNotice("ready", 2000).notify && deviceArrivalNotice("driver-missing", 0).notify });
  // 平衡
  const g = channelGains(0);
  checks.push({ name: "F547 平衡增益", pass: clampBalance(150) === 100 && Math.abs(g.left - g.right) < 1e-9 && channelGains(-100).left === 1 && channelGains(-100).right < 0.01 });
  return checks;
}
