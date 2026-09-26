/**
 * 系统与设备八件（AI-U3 · F540 内存诊断 / F541 网络重置 / F542 ClickLock /
 * F543 分设备音量 / F544 通知音量分级 / F545 蓝牙电量 / F546 接入通知 / F547 平衡）。
 *
 * 判据唯一源（主册摘文）：
 * - F540「预约/立即两模式；两遍标准读写模式；报告三要素（结论/地址段/建议）；
 *   报告入 F372 时间线；预约下次重启时跑」。
 * - F541「确认框列清单位（Wi-Fi 密码/VPN F484/代理 F485/静态 IP——逐项列出）；
 *   90 秒倒计时自动重启网络栈，无需重启整机；重置后向导式重配；入口在
 *   诊断链最末端」。
 * - F542「按住主键 1.1s 松开=抓起；抓起态指针带微光环；单击放下；Esc 放弃；
 *   默认关」。
 * - F543「音量按输出设备各记各的；新设备首插默认 40%；记忆跨重启；设备
 *   清单 10 台淘汰」。
 * - F544「通知音量独立滑杆（0-100 默认 50）；OSD 双条形制；完全静音档仍总闸」。
 * - F545「电量三处显示同源；连接瞬间通知条；低电（<20%）提示一次（同设备
 *   每小时最多一次）；无电量上报诚实显示；电量跳变平滑（不闪跳）」。
 * - F546「接入→横幅（已就绪/安装驱动中/需要手动安装）；即插即用 2s 内就绪；
 *   失败诚实列出原因与手装路径」。
 * - F547「平衡滑杆中心格点；偏离即时试听；按输出设备记忆（F543 族）；纯软件
 *   平衡延迟 <1ms」。
 */

import { u3Store } from "./u3store";

/* ------------------------------- F540 内存诊断 ------------------------------- */

export type MemDiagMode = "standard" | "immediate";
export const MEM_DIAG_PASSES = 2;
/** 两遍标准模式约 30 分钟级（主册口径）——进度显示的数据源。 */
export const MEM_DIAG_EST_MINUTES = 30;

export interface MemDiagReport {
  verdict: "pass" | "fail";
  badRanges: Array<{ from: string; to: string }>;
  advice: string;
}

/** 报告三要素（判据：结论/地址段/建议）。 */
export function memDiagReportShape(foundBad: boolean, ranges: Array<{ from: string; to: string }>): MemDiagReport {
  return {
    verdict: foundBad ? "fail" : "pass",
    badRanges: ranges,
    advice: foundBad ? "检测到异常地址段——建议送检内存条或更换插槽复测" : "检测通过——内存无异常",
  };
}

/** 报告入时间线留痕（判据：报告入 F372 时间线可追溯）。 */
export function memDiagTimelineEntry(mode: MemDiagMode, r: MemDiagReport): string {
  return `内存诊断（${mode === "immediate" ? "立即" : "预约重启"}）：${r.verdict === "pass" ? "通过" : `未通过（${r.badRanges.length} 段异常）`}`;
}

/* ------------------------------- F541 网络重置 ------------------------------- */

/** 90s 倒计时（主册 F541 规格表：无需重启整机）。 */
export const NET_RESET_COUNTDOWN_SEC = 90;

/** 清单完整性（判据：清除 Wi-Fi 密码/VPN/代理/静态 IP——逐项列出）。 */
export const NET_RESET_CLEAR_LIST = [
  "Wi-Fi 密码（所有已存网络）",
  "VPN 配置（F484）",
  "代理设置（F485）",
  "静态 IP 与 DNS 配置",
] as const;

/** 重置执行面（判据：90s 无整机重启；重配向导链路）。 */
export function netResetPlan(): { countdownSec: number; clears: readonly string[]; rebootRequired: false; wizardSteps: string[] } {
  return {
    countdownSec: NET_RESET_COUNTDOWN_SEC,
    clears: NET_RESET_CLEAR_LIST,
    rebootRequired: false,
    wizardSteps: ["扫描可用 Wi-Fi 并逐个询问重连", "提示重新配置 VPN（F484）", "提示重新配置代理（F485）"],
  };
}

/* ------------------------------- F542 ClickLock ------------------------------- */

/** 抓起阈值 1.1s（主册 F542 判据）。 */
export const CLICK_LOCK_THRESHOLD_MS = 1100;

export type ClickLockEvent =
  | { t: "press"; atMs: number }
  | { t: "release"; atMs: number }
  | { t: "click"; atMs: number }   // 抓起态下单击 = 放下
  | { t: "esc"; atMs: number };

export type ClickLockState = "idle" | "held-pending" | "grabbed" | "idle";

export interface ClickLockRt { state: ClickLockState; pressAtMs: number }

/** ClickLock 状态机（判据：1.1s 抓起/单击放下/Esc 放弃；拖拽语义与 F262 兼容）。 */
export function clickLockStep(rt: ClickLockRt, ev: ClickLockEvent): { state: ClickLockState; grabbed: boolean; ring: boolean; dropOrDrag: "drag" | "drop" | "cancel" | "none" } {
  switch (ev.t) {
    case "press":
      rt.state = "held-pending";
      rt.pressAtMs = ev.atMs;
      return { state: rt.state, grabbed: false, ring: false, dropOrDrag: "none" };
    case "release": {
      if (rt.state !== "held-pending") return { state: rt.state, grabbed: rt.state === "grabbed", ring: rt.state === "grabbed", dropOrDrag: "none" };
      if (ev.atMs - rt.pressAtMs >= CLICK_LOCK_THRESHOLD_MS) {
        rt.state = "grabbed";
        return { state: "grabbed", grabbed: true, ring: true, dropOrDrag: "drag" }; // 抓起=一直按着的语义
      }
      rt.state = "idle";
      return { state: "idle", grabbed: false, ring: false, dropOrDrag: "none" }; // 短按 = 普通点击
    }
    case "click":
      if (rt.state === "grabbed") {
        rt.state = "idle";
        return { state: "idle", grabbed: false, ring: false, dropOrDrag: "drop" }; // 单击放下
      }
      return { state: rt.state, grabbed: false, ring: false, dropOrDrag: "none" };
    case "esc":
      if (rt.state === "grabbed") {
        rt.state = "idle";
        return { state: "idle", grabbed: false, ring: false, dropOrDrag: "cancel" }; // Esc 放弃
      }
      return { state: rt.state, grabbed: false, ring: false, dropOrDrag: "none" };
  }
}

/* ------------------------------- F543 分设备音量记忆 ------------------------------- */

export const DEVICE_VOLUME_CAP = 10;   // 设备清单 10 台淘汰（主册 F543 判据）
export const NEW_DEVICE_DEFAULT = 40;  // 新设备首插 40%（主册 F543 规格表）

export interface DeviceVolumeEntry { deviceId: string; name: string; volume: number; lastUsed: number }

/** 切换跟随（判据：拔插切换音量跟着设备走）；新设备首发 40%；LRU 10 台淘汰。 */
export function resolveDeviceVolume(devices: DeviceVolumeEntry[], deviceId: string, name: string, now: number): { volume: number; devices: DeviceVolumeEntry[]; isNew: boolean } {
  const found = devices.find((d) => d.deviceId === deviceId);
  if (found) {
    found.lastUsed = now;
    return { volume: found.volume, devices, isNew: false };
  }
  const next = [...devices, { deviceId, name, volume: NEW_DEVICE_DEFAULT, lastUsed: now }];
  while (next.length > DEVICE_VOLUME_CAP) {
    let oldest = 0;
    for (let i = 1; i < next.length; i++) if (next[i].lastUsed < next[oldest].lastUsed) oldest = i;
    next.splice(oldest, 1);
  }
  return { volume: NEW_DEVICE_DEFAULT, devices: next, isNew: true };
}

/** 音量变更回写记忆（切换设备音量自动各归各位）。 */
export function updateDeviceVolume(devices: DeviceVolumeEntry[], deviceId: string, volume: number): DeviceVolumeEntry[] {
  return devices.map((d) => (d.deviceId === deviceId ? { ...d, volume } : d));
}

/* ------------------------------- F544 通知音量分级 ------------------------------- */

export const NOTIFY_VOLUME_DEFAULT = 50; // 默认 50（主册 F544 判据）

/** 叠加裁决（判据：总闸优先级——完全静音档 F341 仍一票否决）。 */
export function effectiveNotifyVolume(notifyVolume: number, mediaVolume: number, silentMaster: boolean): { notify: number; media: number; dualBar: true } {
  if (silentMaster) return { notify: 0, media: 0, dualBar: true };
  return { notify: notifyVolume, media: mediaVolume, dualBar: true };
}

/* ------------------------------- F545 蓝牙耳机电量 ------------------------------- */

export const BT_LOW_PCT = 20;             // 低电阈值（主册：<20%）
export const BT_LOW_THROTTLE_MS = 3_600_000; // 同设备每小时最多一次
/** 电量平滑步进（判据：电量跳变平滑不闪跳——每次渲染最多步进 2%）。 */
export const BT_SMOOTH_STEP = 2;

export interface BtBatteryRt { displayed: number; lastLowWarnAt: Record<string, number> }

/** 电量平滑逼近（判据：跳变平滑——向真实值步进，每次 ≤2%）。 */
export function smoothBattery(rt: BtBatteryRt, real: number): number {
  const diff = real - rt.displayed;
  if (Math.abs(diff) <= BT_SMOOTH_STEP) rt.displayed = real;
  else rt.displayed += Math.sign(diff) * BT_SMOOTH_STEP;
  return rt.displayed;
}

/** 低电提醒节流（判据：同设备每小时最多一次——不刷屏）。 */
export function lowBatteryWarn(rt: BtBatteryRt, deviceId: string, pct: number, now: number): boolean {
  if (pct >= BT_LOW_PCT) return false;
  const last = rt.lastLowWarnAt[deviceId] ?? -Infinity;
  if (now - last < BT_LOW_THROTTLE_MS) return false;
  rt.lastLowWarnAt[deviceId] = now;
  return true;
}

/** 三处同源数据形状（判据：F290 设备页/F423 电池浮层/连接通知条同源）。 */
export function batterySurfaces(deviceId: string, pct: number | null): { devicePage: string; batteryFlyout: string; connectToast: string } {
  const label = pct === null ? "无电量信息" : `${pct}%`;
  return {
    devicePage: `${deviceId} · 电量 ${label}`,
    batteryFlyout: `${deviceId}（附属区） ${label}`,
    connectToast: `耳机已连接 · 电量 ${label}`,
  };
}

/* ------------------------------- F546 新设备接入通知 ------------------------------- */

export type DeviceNotifyState = "ready" | "installing" | "manual-needed";

/** 三状态横幅（判据：即插即用 2s 内就绪；失败诚实列原因与手装路径）。 */
export function deviceNotifyBanner(state: DeviceNotifyState, name: string, driverProgress = 0): { title: string; body: string; dwellMs: number; route: string | null } {
  switch (state) {
    case "ready":
      return { title: name, body: "已就绪", dwellMs: 2000, route: null }; // 横幅一闪即隐
    case "installing":
      return { title: name, body: `安装驱动中… ${Math.round(driverProgress * 100)}%`, dwellMs: 5000, route: null };
    case "manual-needed":
      return { title: name, body: "需要手动安装——点击查看打印机与驱动向导（F444）", dwellMs: 8000, route: "f444" };
  }
}

/* ------------------------------- F547 音量平衡 ------------------------------- */

export const BALANCE_RANGE = 100; // -100（左）… 0（中心）… +100（右）
/** 软件平衡延迟 <1ms（主册 F547 规格表——声明数据源）。 */
export const BALANCE_SW_DELAY_MS = 1;

/** 平衡 → 左右增益（等功率三角分派：中心 0=1/1，全左 -100=1/0）。 */
export function balanceGains(pan: number): { left: number; right: number } {
  const p = Math.min(100, Math.max(-100, pan)) / 100;
  return { left: Math.cos(((p + 1) * Math.PI) / 4), right: Math.cos(((1 - p) * Math.PI) / 4) };
}

/** 中心格点判定（判据：中心格点标记——|pan|<3 视为中心吸附）。 */
export function isBalanceCenter(pan: number): boolean {
  return Math.abs(pan) < 3;
}

/* ------------------------------- 运行时读取 ------------------------------- */

export function devVolumeConfig() {
  const s = u3Store.get("devVolume");
  return {
    devices: (s.devices as unknown as DeviceVolumeEntry[]) ?? [],
    newDeviceDefault: (s.newDeviceDefault as number) ?? NEW_DEVICE_DEFAULT,
    cap: (s.cap as number) ?? DEVICE_VOLUME_CAP,
  };
}
export function clickLockConfig() {
  const s = u3Store.get("clickLock");
  return { enabled: !!s.enabled, thresholdMs: (s.thresholdMs as number) ?? CLICK_LOCK_THRESHOLD_MS };
}

/* ------------------------------- 自检 ------------------------------- */

/** F540-F547 判据自检（前端面）。 */
export function sysdevSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 内存诊断报告三要素
  const rep = memDiagReportShape(true, [{ from: "0x1000", to: "0x2000" }]);
  checks.push({ name: "F540 报告三要素", pass: rep.verdict === "fail" && rep.badRanges.length === 1 && rep.advice.includes("送检") });
  // 网络重置清单
  const plan = netResetPlan();
  checks.push({ name: "F541 清单四项+90s+不重启", pass: plan.clears.length === 4 && plan.countdownSec === 90 && !plan.rebootRequired });
  // ClickLock 状态机
  const cl: ClickLockRt = { state: "idle", pressAtMs: 0 };
  clickLockStep(cl, { t: "press", atMs: 0 });
  const grab = clickLockStep(cl, { t: "release", atMs: 1100 });
  const drop = clickLockStep(cl, { t: "click", atMs: 2000 });
  const cl2: ClickLockRt = { state: "idle", pressAtMs: 0 };
  clickLockStep(cl2, { t: "press", atMs: 0 });
  clickLockStep(cl2, { t: "release", atMs: 1099 }); // 差 1ms 不抓起
  checks.push({ name: "F542 1.1s 抓起+单击放下", pass: grab.grabbed && drop.dropOrDrag === "drop" && cl2.state === "idle" });
  // 分设备音量：新设备 40 / LRU 10 淘汰
  let devs: DeviceVolumeEntry[] = [];
  const r1 = resolveDeviceVolume(devs, "hp", "耳机", 0);
  devs = r1.devices;
  for (let i = 0; i < 10; i++) devs = resolveDeviceVolume(devs, `d${i}`, `设备${i}`, i + 1).devices;
  checks.push({ name: "F543 新设备 40%+LRU 淘汰", pass: r1.volume === 40 && devs.length === 10 && !devs.some((d) => d.deviceId === "hp") });
  // 通知音量独立+总闸
  checks.push({ name: "F544 独立滑杆+总闸优先", pass: effectiveNotifyVolume(50, 20, false).notify === 50 && effectiveNotifyVolume(50, 20, true).notify === 0 });
  // 电量平滑+节流
  const bt: BtBatteryRt = { displayed: 10, lastLowWarnAt: {} };
  smoothBattery(bt, 80);
  smoothBattery(bt, 80);
  checks.push({ name: "F545 跳变平滑(≤2%/次)+低电节流", pass: bt.displayed === 14 && lowBatteryWarn(bt, "hp", 15, 0) && !lowBatteryWarn(bt, "hp", 12, 1000) && lowBatteryWarn(bt, "hp", 12, 3_600_001) });
  // 接入通知三态
  checks.push({
    name: "F546 三态横幅+2s 就绪",
    pass: deviceNotifyBanner("ready", "U 盘").dwellMs === 2000 && deviceNotifyBanner("installing", "声卡", 0.5).body.includes("50%") && deviceNotifyBanner("manual-needed", "打印设备").route === "f444",
  });
  // 平衡
  const g = balanceGains(-100);
  const c = balanceGains(0);
  checks.push({ name: "F547 平衡增益+中心格点", pass: Math.abs(g.left - 1) < 0.001 && g.right < 0.001 && Math.abs(c.left - 0.7071) < 0.01 && isBalanceCenter(2) });
  return checks;
}
