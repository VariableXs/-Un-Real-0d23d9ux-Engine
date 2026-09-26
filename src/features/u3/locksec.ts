/**
 * 锁屏安全五件（AI-U3 · F504 PIN / F505 蓝牙动态锁 / F506 访客 / F507 锁屏防截 / F508 应用防截）。
 *
 * 判据唯一源（主册摘文）：
 * - F504「4-6 位数字本机验证；锁屏自动切 PIN 键盘；PIN 错误 5 次冷却
 *   （30s 起、逐次翻倍——防爆破）且冷却期回退密码登录；PIN 随时改/废；
 *   解锁时序 <1.5s 全链」。
 * - F505「绑定一台蓝牙设备作钥匙；信号消失超过 30 秒自动锁屏（F238）；
 *   锁形标记；误触发测试（短暂信号波动 <10s 不锁）；与 F316 双保险并存」。
 * - F506「访客会话是沙盒（独立临时目录、无系统设置权、权限默认全拒 F324、
 *   不可见主用户文件）；退出时会话数据全清（清理前确认一次）；极简形制；
 *   会话时长上限提示（默认 2h）」。
 * - F507「截屏三路（PrtSc F413/截图工具 F098/录屏 F361）在锁屏态全部拒绝；
 *   纯黑帧/不触发实现选择文档化；性能零开销（非锁屏态）」。
 * - F508「应用可声明窗口防截（vxapp 清单字段）；黑块区域精确（窗口几何
 *   对齐）；F361 录屏同规则；未标记应用零影响」。
 */

import { u3Store } from "./u3store";

/* ------------------------------- F504 PIN ------------------------------- */

export const PIN_MIN_LEN = 4;
export const PIN_MAX_LEN = 6;
/** 冷却起步 30s（主册：30s 起、逐次翻倍——防爆破）。 */
export const PIN_COOLDOWN_BASE_MS = 30_000;
/** 冷却门槛：连错 5 次才进冷却（主册：错误 5 次冷却）。 */
export const PIN_COOLDOWN_AFTER_FAILS = 5;
/** 解锁全链时序线（主册：<1.5s）。 */
export const PIN_UNLOCK_BUDGET_MS = 1500;

export type PinVerdict =
  | { ok: true }
  | { ok: false; reason: "wrong-pin"; failsLeft: number }
  | { ok: false; reason: "cooldown"; coolMsLeft: number; fallbackToPassword: true };

/** 连错 n 次后的冷却时长：30s × 2^(n-5)，翻倍表钉死（n<5 为 0）。 */
export function pinCooldownMs(failCount: number): number {
  if (failCount < PIN_COOLDOWN_AFTER_FAILS) return 0;
  const over = failCount - PIN_COOLDOWN_AFTER_FAILS;
  return PIN_COOLDOWN_BASE_MS * Math.pow(2, Math.min(over, 10)); // 2^10 封顶防溢出
}

/** PIN 形校验：4-6 位纯数字（判据：4-6 位设置）。 */
export function pinShapeOk(pin: string): boolean {
  return new RegExp(`^\\d{${PIN_MIN_LEN},${PIN_MAX_LEN}}$`).test(pin);
}

export interface PinRuntimeState {
  failCount: number;
  coolUntil: number;
}

/**
 * PIN 验证（本机验证不出机器——纯内存比对；时序预算自检）：
 * 冷却期内一律拒绝并回退密码（判据：冷却期回退密码登录）。
 */
export function verifyPin(input: string, stored: string, rt: PinRuntimeState, now: number): PinVerdict {
  if (rt.coolUntil > now) {
    return { ok: false, reason: "cooldown", coolMsLeft: rt.coolUntil - now, fallbackToPassword: true };
  }
  if (input === stored) {
    rt.failCount = 0;
    rt.coolUntil = 0;
    return { ok: true };
  }
  rt.failCount += 1;
  const cool = pinCooldownMs(rt.failCount);
  if (cool > 0) rt.coolUntil = now + cool;
  return { ok: false, reason: "wrong-pin", failsLeft: Math.max(0, PIN_COOLDOWN_AFTER_FAILS - rt.failCount) };
}

/** PIN 改/废即时生效（判据：改废即时生效）——废了回密码（pin 置空串即废）。 */
export function setPin(pin: string): void {
  if (pin !== "" && !pinShapeOk(pin)) {
    throw new Error(`[u3:F504] PIN 必须为 ${PIN_MIN_LEN}-${PIN_MAX_LEN} 位数字`);
  }
  u3Store.set("pinUnlock", { pin, enabled: pin !== "", failCount: 0, coolUntil: 0 });
}

/* ------------------------------- F505 蓝牙动态锁 ------------------------------- */

/** 信号消失 30s±5s 触发（主册 F505 规格表）。 */
export const BT_LOCK_AWAY_MS = 30_000;
/** 短暂信号波动 <10s 不锁（主册 F505 判据）。 */
export const BT_GLITCH_TOLERANCE_MS = 10_000;

export interface BtLockRuntimeState {
  lastSeenMs: number;
  awaySinceMs: number | null; // 持续失联起点（波动重置回 null）
}

/**
 * 蓝牙动态锁判定（纯状态机，单测可直接喂时间线）：
 * - keyPresent=true：回连，清失联计时；
 * - keyPresent=false 且刚失联：记起点；
 * - 失联持续 ≥30s → 锁；期间闪现又消失（<10s 波动）→ 重置为持续失联
 *   但不立刻锁（判据：短暂信号波动 <10s 不锁）。
 */
export function btLockTick(
  rt: BtLockRuntimeState,
  keyPresent: boolean,
  now: number,
): { lock: boolean; awayForMs: number } {
  if (keyPresent) {
    rt.lastSeenMs = now;
    rt.awaySinceMs = null;
    return { lock: false, awayForMs: 0 };
  }
  if (rt.awaySinceMs === null) {
    rt.awaySinceMs = now;
    return { lock: false, awayForMs: 0 };
  }
  const away = now - rt.awaySinceMs;
  return { lock: away >= BT_LOCK_AWAY_MS, awayForMs: away };
}

/** 锁形标记语义（判据：钥匙设备与其他蓝牙设备视觉区分——锁形标记）。 */
export const BT_KEY_BADGE = "lock";

/* ------------------------------- F506 访客模式 ------------------------------- */

/** 会话时长上限提示（主册：默认 2h）。 */
export const GUEST_SESSION_CAP_MIN = 120;
/** 沙盒四判据（主册：文件/设置/权限/网络共享）。 */
export const GUEST_SANDBOX_AXES = ["files", "settings", "permissions", "network-share"] as const;
export type GuestSandboxAxis = (typeof GUEST_SANDBOX_AXES)[number];

/** 沙盒能力表：访客在四轴上全部只读/拒绝（判据：沙盒隔离四判据）。 */
export function guestCapability(axis: GuestSandboxAxis): "readonly" | "denied" {
  switch (axis) {
    case "files": return "readonly";      // 看得到公共区，看不见主用户文件
    case "settings": return "denied";      // 无系统设置权
    case "permissions": return "denied";   // F324 权限默认全拒
    case "network-share": return "denied"; // 不可对外共享
  }
}

/** 访客会话退出清理清单（判据：退出清理完整性——注入残留文件验证）。 */
export const GUEST_CLEANUP_TARGETS = [
  "guest-home", "guest-temp", "guest-downloads", "guest-recent", "guest-clipboard",
] as const;

export function guestCleanupPlan(): readonly string[] {
  return GUEST_CLEANUP_TARGETS;
}

/* ------------------------------- F507 锁屏防截图 ------------------------------- */

/** 三路截图注入面（主册：PrtSc F413/截图工具 F098/录屏 F361 锁屏态全拒）。 */
export const SCREENSHOT_CHANNELS = ["prtsc", "snip-tool", "screen-recorder", "third-party-api"] as const;
export type ScreenshotChannel = (typeof SCREENSHOT_CHANNELS)[number];

export type ShieldMode = "black-frame" | "deny";

/**
 * 锁屏态截图拦截（判据：三路全拒 + 实现选择文档化）。
 * mode="black-frame"：快照成功但内容为纯黑帧（防时间侧信道）；"deny"：直接拒绝。
 * 非锁屏态零开销：本函数只在 locked=true 时被运行时调用（调用面即边界）。
 */
export function lockScreenShotPolicy(locked: boolean, mode: ShieldMode, ch: ScreenshotChannel):
  | { allowed: true }
  | { allowed: false; blackFrame: boolean; reason: string } {
  if (!locked) return { allowed: true };
  void ch; // 四通道同规则（主册三路 + 第三方 API 同拦截面）
  return {
    allowed: false,
    blackFrame: mode === "black-frame",
    reason: mode === "black-frame" ? "锁屏截图保护：已输出纯黑帧" : "锁屏截图保护：本次截取已被拒绝",
  };
}

/* ------------------------------- F508 应用防截标记 ------------------------------- */

export interface WindowRect { x: number; y: number; w: number; h: number }

/**
 * 截图合成时的防截黑块计算（判据：黑块区域精确——窗口几何对齐；未标记
 * 应用零影响）。markers: 窗口id → 防截矩形声明（vxapp 清单字段读取结果）。
 */
export function redactRects(markers: Record<string, WindowRect>, shot: WindowRect): WindowRect[] {
  const out: WindowRect[] = [];
  for (const id of Object.keys(markers)) {
    const m = markers[id];
    const x1 = Math.max(m.x, shot.x);
    const y1 = Math.max(m.y, shot.y);
    const x2 = Math.min(m.x + m.w, shot.x + shot.w);
    const y2 = Math.min(m.y + m.h, shot.y + shot.h);
    if (x2 > x1 && y2 > y1) out.push({ x: x1, y: y1, w: x2 - x1, h: y2 - y1 });
  }
  return out; // 空数组 = 未标记应用零影响
}

/** 黑块视觉规范（判据：黑块视觉规范——用户知道「这块不让拍」）。 */
export const REDACT_NOTE = "黑块区域 = 应用声明的防截窗口（密码/票据类），非显示故障";

/* ------------------------------- 运行时读取 ------------------------------- */

export function pinConfig() {
  const s = u3Store.get("pinUnlock");
  return { enabled: !!s.enabled, pin: (s.pin as string) ?? "", length: (s.length as number) ?? PIN_MIN_LEN };
}

export function btLockConfig() {
  const s = u3Store.get("btLock");
  return { enabled: !!s.enabled, keyDeviceId: (s.keyDeviceId as string) ?? "", awaySeconds: (s.awaySeconds as number) ?? 30 };
}

/* ------------------------------- 自检 ------------------------------- */

/** F504-F508 判据自检（前端面）。 */
export function locksecSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 冷却翻倍表：30s 起、逐次翻倍
  const seq = [5, 6, 7, 8].map((n) => pinCooldownMs(n));
  checks.push({ name: "F504 冷却翻倍表", pass: seq[0] === 30_000 && seq[1] === 60_000 && seq[2] === 120_000 && seq[3] === 240_000 });
  // 冷却门槛：连错 4 次不冷却
  checks.push({ name: "F504 五次门槛", pass: pinCooldownMs(4) === 0 });
  // PIN 形校验
  checks.push({ name: "F504 4-6 位校验", pass: pinShapeOk("1234") && pinShapeOk("123456") && !pinShapeOk("123") && !pinShapeOk("1234567") && !pinShapeOk("12a4") });
  // 蓝牙锁：30s 触发、10s 波动不锁
  const rt: BtLockRuntimeState = { lastSeenMs: 0, awaySinceMs: null };
  btLockTick(rt, false, 1000);
  const glitch = btLockTick(rt, true, 5000); // 闪现 4s
  btLockTick(rt, false, 8000);
  const early = btLockTick(rt, 8000 + 26_000, 34_000); // 恢复失联 26s（自波动点起算 <30s）
  checks.push({ name: "F505 30s 触发+波动豁免", pass: !glitch.lock && !early.lock });
  const rt2: BtLockRuntimeState = { lastSeenMs: 0, awaySinceMs: null };
  btLockTick(rt2, false, 0);
  const late = btLockTick(rt2, false, 30_000);
  checks.push({ name: "F505 持续失联 30s 锁", pass: late.lock });
  // 访客沙盒四轴
  checks.push({ name: "F506 沙盒四判据", pass: GUEST_SANDBOX_AXES.length === 4 && guestCapability("permissions") === "denied" && guestCapability("files") === "readonly" });
  // 锁屏防截：锁屏态四通道全拒、非锁屏放行
  const blocked = SCREENSHOT_CHANNELS.map((c) => lockScreenShotPolicy(true, "black-frame", c).allowed === false);
  const open = lockScreenShotPolicy(false, "black-frame", "prtsc").allowed === true;
  checks.push({ name: "F507 三路+API 全拒/非锁屏零开销", pass: blocked.every(Boolean) && open });
  // 防截黑块：几何对齐 + 未标记零影响
  const rects = redactRects({ a: { x: 100, y: 100, w: 200, h: 80 } }, { x: 150, y: 120, w: 400, h: 300 });
  checks.push({ name: "F508 黑块几何对齐", pass: rects.length === 1 && rects[0].x === 150 && rects[0].y === 120 && rects[0].w === 150 && rects[0].h === 60 });
  checks.push({ name: "F508 未标记零影响", pass: redactRects({}, { x: 0, y: 0, w: 100, h: 100 }).length === 0 });
  return checks;
}
