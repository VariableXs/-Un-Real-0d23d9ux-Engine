/**
 * F507/F508 深化引擎 · 截图防护与防截黑块几何（AI-U3 · capguard）。
 *
 * 判据唯一源（主册摘文）：
 * - F507「三路截图注入测试（锁屏态全拒）；第三方 API 拦截；解锁后恢复；纯黑
 *   帧/不触发实现选择文档化；性能零开销（非锁屏态）」。
 * - F508「清单字段声明读取；黑块区域精确性（窗口几何对齐）；F361 录屏同规则；
 *   未标记应用零影响；黑块视觉规范」。
 *
 * 深化点：
 * 1. 三路通道（PrintScreen 合成/第三方截屏 API/录屏管线）统一进**裁决器**：
 *    锁屏态全拒（三路同判）、标记应用区域出黑块、其余放行——一个裁决点,
 *    三路不可能各判各的（一致性来自结构）。
 * 2. 黑块几何合成器：窗口矩形 ∩ 标记区域 → 黑块矩形集（多窗口按 Z 序遮挡
 *    裁剪——被上窗遮住的部分不出黑块，精确对齐判据）。
 * 3. F361 同规则：录屏与截图走同一裁决函数，规则单点。
 * 4. 非锁屏零开销路径：无锁屏且无标记时返回 fast-path 标记（调用方可短路）。
 */

/* ------------------------------ F507 裁决器 ------------------------------ */

export type CaptureChannel = "printscreen" | "thirdparty-api" | "screen-record";
export const CAPTURE_CHANNELS: readonly CaptureChannel[] = ["printscreen", "thirdparty-api", "screen-record"];

/** 防截实现选择（判据「纯黑帧/不触发实现选择文档化」——登记项即文档）。 */
export const CAPGUARD_IMPLEMENTATION_CHOICE = "纯黑帧合成（不触发通道会话保持）——登记于主册 F507 实现选择条款" as const;

export type CaptureVerdict =
  | { allow: true; fastPath: boolean }
  | { allow: false; reason: "lockscreen" }
  | { allow: true; masked: true; reason: "app-shield" };

/**
 * 截图/录屏统一裁决（三路同函数——F361 同规则的结构保证）。
 * @param lockscreenActive 锁屏是否激活
 * @param hasShieldMarkers 全屏是否存在任何防截标记应用
 */
export function adjudicateCapture(channel: CaptureChannel, lockscreenActive: boolean, hasShieldMarkers: boolean): CaptureVerdict {
  if (!CAPTURE_CHANNELS.includes(channel)) {
    throw new Error(`[u3:F507] 未知截图通道 ${String(channel)}`);
  }
  if (lockscreenActive) return { allow: false, reason: "lockscreen" };
  if (hasShieldMarkers) return { allow: true, masked: true, reason: "app-shield" };
  return { allow: true, fastPath: true }; // 零开销路径：无锁屏无标记，调用方短路
}

/* ------------------------------ F508 黑块几何 ------------------------------ */

export interface Rect { x: number; y: number; w: number; h: number }

/** 应用防截标记声明（判据「清单字段声明读取」的最小字段集）。 */
export interface ShieldMarker {
  /** 应用进程/窗口标识。 */
  appId: string;
  /** 防截敏感区（窗口内坐标）。 */
  region: Rect;
  /** 视觉规范：黑块边缘羽化 px（0=硬边）。 */
  featherPx: number;
}

/** 两矩形相交；不相交返回 null。 */
export function intersectRect(a: Rect, b: Rect): Rect | null {
  const x1 = Math.max(a.x, b.x);
  const y1 = Math.max(a.y, b.y);
  const x2 = Math.min(a.x + a.w, b.x + b.w);
  const y2 = Math.min(a.y + a.h, b.y + b.h);
  if (x2 <= x1 || y2 <= y1) return null;
  return { x: x1, y: y1, w: x2 - x1, h: y2 - y1 };
}

/** 矩形减法：a - b（b 完全覆盖 a 的部分剔除，最多出 4 块）。 */
export function subtractRect(a: Rect, b: Rect): Rect[] {
  const inter = intersectRect(a, b);
  if (!inter) return [a];
  const out: Rect[] = [];
  if (inter.y > a.y) out.push({ x: a.x, y: a.y, w: a.w, h: inter.y - a.y });
  if (inter.y + inter.h < a.y + a.h) out.push({ x: a.x, y: inter.y + inter.h, w: a.w, h: a.y + a.h - (inter.y + inter.h) });
  if (inter.x > a.x) out.push({ x: a.x, y: inter.y, w: inter.x - a.x, h: inter.h });
  if (inter.x + inter.w < a.x + a.w) out.push({ x: inter.x + inter.w, y: inter.y, w: a.x + a.w - (inter.x + inter.w), h: inter.h });
  return out;
}

/**
 * 黑块几何合成（判据「黑块区域精确性（窗口几何对齐）」）：
 * 输入 Z 序窗口列表（index 0 最顶）与各窗口标记；输出合成画面中黑块矩形集。
 * 规则：标记区域先与自身窗口矩形求交（区域声明越界=声明缺陷，裁剪对齐），
 * 再减去其上所有窗口的可见区——被遮挡部分不重复出黑块。
 */
export function composeShieldBlocks(
  zOrderedWindows: Array<{ rect: Rect; markers: ShieldMarker[] }>,
): Array<{ rect: Rect; appId: string; featherPx: number }> {
  const blocks: Array<{ rect: Rect; appId: string; featherPx: number }> = [];
  zOrderedWindows.forEach((win, z) => {
    for (const m of win.markers) {
      const clipped = intersectRect(m.region, win.rect);
      if (!clipped) continue; // 标记声明越出窗口：裁剪（诚实对齐而非整块照搬）
      let visible: Rect[] = [clipped];
      for (let above = z - 1; above >= 0 && visible.length > 0; above--) {
        const cover = zOrderedWindows[above]!.rect;
        visible = visible.flatMap((r) => subtractRect(r, cover));
      }
      for (const r of visible) blocks.push({ rect: r, appId: m.appId, featherPx: m.featherPx });
    }
  });
  return blocks;
}

/** 黑块视觉规范（判据「黑块视觉规范」的具名常量）。 */
export const SHIELD_BLOCK_COLOR = "#000000";
export const SHIELD_BLOCK_MIN_ALPHA = 1.0;

/* ------------------------------ 自检 ------------------------------ */

export function capguardSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 三路锁屏全拒
  const allDenied = CAPTURE_CHANNELS.every((c) => !adjudicateCapture(c, true, false).allow);
  checks.push({ name: "F507 三路锁屏全拒", pass: allDenied });
  // 解锁后恢复：无标记 fast path
  const free = adjudicateCapture("printscreen", false, false);
  const marked = adjudicateCapture("thirdparty-api", false, true);
  checks.push({ name: "F507 解锁恢复+零开销路径", pass: "fastPath" in free && free.fastPath === true && marked.allow && "reason" in marked && marked.reason === "app-shield" });
  // 未知通道抛错
  let threw = false;
  try { adjudicateCapture("unknown" as CaptureChannel, false, false); } catch { threw = true; }
  checks.push({ name: "F507 未知通道显性报错", pass: threw });
  // 黑块几何：窗口(0,0,800,600) 标记区(100,100,200,100)，顶窗遮挡 (150,100,100,600)
  const win = { rect: { x: 0, y: 0, w: 800, h: 600 }, markers: [{ appId: "bank", region: { x: 100, y: 100, w: 200, h: 100 }, featherPx: 0 }] };
  const cover = { rect: { x: 150, y: 100, w: 100, h: 600 }, markers: [] };
  const blocks = composeShieldBlocks([cover, win]);
  // 可见黑块应为两块：左侧 (100,100,50,100) 与右侧 (250,100,50,100)
  const totalArea = blocks.reduce((s, b) => s + b.rect.w * b.rect.h, 0);
  const left = blocks.find((b) => b.rect.x === 100 && b.rect.w === 50);
  const right = blocks.find((b) => b.rect.x === 250 && b.rect.w === 50);
  checks.push({ name: "F508 黑块 Z 序遮挡精确", pass: blocks.length === 2 && totalArea === 10000 && !!left && !!right });
  // 未标记应用零影响：无标记窗口合成出空集
  const none = composeShieldBlocks([{ rect: { x: 0, y: 0, w: 400, h: 400 }, markers: [] }]);
  checks.push({ name: "F508 未标记应用零影响", pass: none.length === 0 });
  // 区域越界裁剪：标记区超出窗口的部分被裁
  const over = composeShieldBlocks([{ rect: { x: 0, y: 0, w: 100, h: 100 }, markers: [{ appId: "a", region: { x: 50, y: 50, w: 200, h: 200 }, featherPx: 0 }] }]);
  checks.push({ name: "F508 越界声明裁剪对齐", pass: over.length === 1 && over[0]!.rect.w === 50 && over[0]!.rect.h === 50 });
  return checks;
}
