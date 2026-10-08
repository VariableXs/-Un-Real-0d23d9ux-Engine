/**
 * AI-12 兼容纵深组 — 前端纯逻辑模块集合（Z-16…Z-21、M-37…M-45）。
 * 原则：只观察、只提示、只降级，绝不越权干预；真机依赖项按诚实口径登记待验。
 */

// ---------------------------------------------------------------------------
// Z-16 遗留协议兼容 Shim（Legacy Protocol Shim）
// 前端侧：开关状态 + 命中上报（后端计数）+ 本地诊断环形日志。
// ---------------------------------------------------------------------------

export interface ShimLogItem { hit: string; at: number; count: number }

const shimLog: ShimLogItem[] = [];
let shimEnabled = true;

export function setLegacyShimEnabled(v: boolean): void {
  shimEnabled = v;
}
export function isLegacyShimEnabled(): boolean {
  return shimEnabled;
}

export interface ShimReporter {
  (hit: string): Promise<number>;
}

/**
 * 记录一次旧协议命中（旧托盘回调转译/气球通知映射/DPI 菜单坐标换算）。
 * 诊断模式下可通过 getShimLog() 查看命中次数。失败静默（兼容层不打扰主流程）。
 */
export function reportShimHit(hit: string, reporter?: ShimReporter): ShimLogItem | null {
  if (!shimEnabled || !hit.trim()) return null;
  const now = Date.now();
  const existing = shimLog.find((x) => x.hit === hit && now - x.at < 60_000);
  if (existing) {
    existing.count += 1;
    existing.at = now;
    void reporter?.(hit).catch(() => {});
    return existing;
  }
  const item: ShimLogItem = { hit, at: now, count: 1 };
  shimLog.push(item);
  if (shimLog.length > 50) shimLog.shift();
  void reporter?.(hit).catch(() => {});
  return item;
}

export function getShimLog(): readonly ShimLogItem[] {
  return shimLog;
}

/** 旧协议气球通知 → 原生通知中心条目的映射（纯转换）。 */
export function mapBalloonToNotification(payload: { title?: string; text?: string; icon?: string }): {
  title: string; body: string; source: "legacy-shim" } {
  return {
    title: payload.title?.trim() || "（未知来源应用）",
    body: payload.text?.trim() || "",
    source: "legacy-shim",
  };
}

// ---------------------------------------------------------------------------
// Z-17 IME 深度兼容（IME Compatibility）
// ---------------------------------------------------------------------------

/** 输入法系统窗口识别（VWM 过滤清单）：这些窗口不判为普通窗口、不进入窗口编排。 */
const IME_WINDOW_PATTERNS: RegExp[] = [
  /microsoft\s*ime/i,
  /candidate\s*window/i,
  /候选窗/i,
  /chinese\s*\(simplified\)/i,
  /输入法/i,
  / composition /i,
  /sogou.*floating/i,
  /qqpinyin/i,
  /weasel/i, // 小狼毫
];

export function isImeSystemWindow(title: string, className?: string): boolean {
  const hay = `${title ?? ""} ${className ?? ""}`;
  return IME_WINDOW_PATTERNS.some((re) => re.test(hay));
}

let imeSessionActive = false;
const imeSubs = new Set<(active: boolean) => void>();

export function setImeSessionActive(active: boolean): void {
  if (imeSessionActive === active) return;
  imeSessionActive = active;
  imeSubs.forEach((fn) => fn(active));
}
export function isImeSessionActive(): boolean {
  return imeSessionActive;
}
export function onImeSessionChange(fn: (active: boolean) => void): () => void {
  imeSubs.add(fn);
  return () => imeSubs.delete(fn);
}

/** 单键快捷键让位（联动 Z-10）：IME 组合期间，单字母/数字/标点 + 单修饰的快捷键全部让位。 */
export function shouldYieldToIme(combo: string): boolean {
  if (!imeSessionActive) return false;
  // 组合键含 Ctrl 或 Alt/Win 的多修饰系统键仍让位（IME 吃掉的是主键位），
  // 但功能键（F1-F12）不冲突不避让。
  if (/^F\d{1,2}$/i.test(combo)) return false;
  return true;
}

// ---------------------------------------------------------------------------
// Z-18 全屏与独占模式协议（Fullscreen & Exclusive Protocol）
// 统一输出接口（APEX C-3 契约）：全屏状态只从 sys://fullscreen 事件进来，
// AI-01/AI-17 各消费方一律走本模块订阅，不再各自判定。
// ---------------------------------------------------------------------------

export interface FullscreenState {
  active: boolean;
  title?: string;
  /** "exclusive" | "borderless" | "video"（后端能给的证据，未知为 undefined） */
  mode?: string;
}

let fullscreenState: FullscreenState = { active: false };
const fsSubs = new Set<(s: FullscreenState) => void>();

export function getFullscreenState(): FullscreenState {
  return fullscreenState;
}

export function applyFullscreenEvent(payload: { active?: boolean; title?: string; mode?: string } | boolean): FullscreenState {
  const next: FullscreenState = typeof payload === "boolean"
    ? { active: payload }
    : { active: payload.active ?? false, title: payload.title, mode: payload.mode };
  if (next.active === fullscreenState.active && next.title === fullscreenState.title) return fullscreenState;
  fullscreenState = next;
  fsSubs.forEach((fn) => fn(next));
  return next;
}

export function onFullscreenChange(fn: (s: FullscreenState) => void): () => void {
  fsSubs.add(fn);
  return () => fsSubs.delete(fn);
}

/** 全屏时的覆盖层策略：HUD/横幅自动抑制，热区禁用，徽标类降级为角标。 */
export type OverlayKind = "hud" | "banner" | "badge" | "hotzone" | "altTab";
export function overlayPolicy(kind: OverlayKind, state: FullscreenState = fullscreenState): "suppress" | "badge" | "normal" {
  if (!state.active) return "normal";
  switch (kind) {
    case "hud":
    case "banner":
    case "hotzone":
      return "suppress";
    case "badge":
      return "badge";
    case "altTab":
      // Alt-Tab 让位原生（可配置，这里输出策略，消费方决定是否服从）。
      return "suppress";
    default:
      return "normal";
  }
}

/** 全屏退出后的窗口几何恢复登记（key → 恢复函数），退出时逐个回放。 */
const geometryRestores = new Map<string, () => void>();

export function registerGeometryRestore(key: string, restore: () => void): void {
  geometryRestores.set(key, restore);
}
export function unregisterGeometryRestore(key: string): void {
  geometryRestores.delete(key);
}
/** 全屏退出（active→false）时调用；返回执行的恢复项数量。 */
export function replayGeometryRestores(): number {
  const n = geometryRestores.size;
  geometryRestores.forEach((fn) => {
    try { fn(); } catch { /* 单项失败不阻塞其他恢复 */ }
  });
  return n;
}

// ---------------------------------------------------------------------------
// Z-19 多屏混合 DPI 兼容（Mixed-DPI Handling）
// 物理像素锚定：跨屏拖动时物理尺寸不变，逻辑几何按目标屏 DPI 重算，误差 ≤1px。
// ---------------------------------------------------------------------------

export function toPhysical(logical: number, dpr: number): number {
  return Math.round(logical * dpr);
}
export function toLogical(physical: number, dpr: number): number {
  return physical / dpr;
}

export interface Rect { x: number; y: number; w: number; h: number }

/**
 * 跨屏重锚定：物理像素锚定——先换算成源屏物理像素，再按目标屏 DPR 落回逻辑坐标。
 * 避免连续跨屏时「变大/变小又回弹」的累计误差（每步都以物理像素为真值）。
 */
export function reanchorRect(rect: Rect, dprFrom: number, dprTo: number): Rect {
  if (dprFrom <= 0 || dprTo <= 0) return rect;
  const px = { x: rect.x * dprFrom, y: rect.y * dprFrom, w: rect.w * dprFrom, h: rect.h * dprFrom };
  return {
    x: Math.round(px.x / dprTo),
    y: Math.round(px.y / dprTo),
    w: Math.max(1, Math.round(px.w / dprTo)),
    h: Math.max(1, Math.round(px.h / dprTo)),
  };
}

/** 每屏独立的位图缓存命名空间 key（联动 Z-04 图标缓存按 DPI 分域）。 */
export function dpiCacheKey(screenId: string, dpr: number): string {
  return `${screenId}@${dpr.toFixed(2).replace(/\.?0+$/, "")}x`;
}

// ---------------------------------------------------------------------------
// Z-20 远程与虚拟宿主模式（Remote & VM Host Profile）
// ---------------------------------------------------------------------------

export type HostKind = "remote" | "vm" | "native";

export interface HostProfile {
  /** 动效 0.25× */
  motionScale: number;
  /** 壁纸静态化 */
  staticWallpaper: boolean;
  /** 透明度关闭（RDP 下亚克力开销大） */
  transparencyOff: boolean;
  /** 禁位图缓存 */
  bitmapCacheOff: boolean;
}

/** 误检安全：kind 不确定时按本机模式跑（宁可多耗不可变丑）。 */
export function deriveHostProfile(kind: HostKind, manualOverride?: HostKind | "auto"): HostProfile {
  const effective = !manualOverride || manualOverride === "auto" ? kind : manualOverride;
  if (effective === "remote") {
    return { motionScale: 0.25, staticWallpaper: true, transparencyOff: true, bitmapCacheOff: true };
  }
  if (effective === "vm") {
    return { motionScale: 0.25, staticWallpaper: true, transparencyOff: true, bitmapCacheOff: false };
  }
  return { motionScale: 1, staticWallpaper: false, transparencyOff: false, bitmapCacheOff: false };
}

export function applyHostProfileAttrs(profile: HostProfile, doc: Document = document): void {
  doc.documentElement.setAttribute("data-host-motion", String(profile.motionScale));
  doc.documentElement.toggleAttribute("data-host-static-wallpaper", profile.staticWallpaper);
  doc.documentElement.toggleAttribute("data-host-no-transparency", profile.transparencyOff);
  doc.documentElement.toggleAttribute("data-host-no-bitmap-cache", profile.bitmapCacheOff);
}

// ---------------------------------------------------------------------------
// Z-21 慢速设备模式（Slow Device Mode）
// 降级阶梯只降不升（避免抖动），重启或用户手动后重评估。
// ---------------------------------------------------------------------------

export type SlowTier = 0 | 1 | 2 | 3;

export interface SlowSignals {
  /** 机械硬盘（磁盘类型信号） */
  hdd: boolean;
  /** 可用内存 < 2GB 或总内存 ≤ 8GB */
  lowMem: boolean;
  /** 低端核显（GPU 特征） */
  lowGpu: boolean;
}

export function computeSlowTier(signals: SlowSignals): SlowTier {
  let tier: SlowTier = 0;
  if (signals.hdd) tier = 1 as SlowTier;
  if (signals.hdd && signals.lowMem) tier = 2 as SlowTier;
  if (signals.hdd && signals.lowMem && signals.lowGpu) tier = 3 as SlowTier;
  return tier;
}

/** 只降不升：newTier 只能维持或加深，除非手动 reset。 */
export function downgradeOnly(current: SlowTier, next: SlowTier): SlowTier {
  return next > current ? next : current;
}

const TIER_LABEL: Record<SlowTier, string> = {
  0: "L0 全功能",
  1: "L1 关位图缓存与模糊",
  2: "L2 关动效与 WebGL 壁纸",
  3: "L3 纯色背景 + 最小动效",
};

export function slowTierLabel(tier: SlowTier): string {
  return TIER_LABEL[tier];
}

export function applySlowTierAttrs(tier: SlowTier, doc: Document = document): void {
  doc.documentElement.setAttribute("data-slow-tier", String(tier));
}

// ---------------------------------------------------------------------------
// M-40 高刷自适应时长（High-Refresh Timing）
// ---------------------------------------------------------------------------

/** ≥120Hz 动效时长按 0.85× 折算（只调时长不加速度曲线），60Hz 保持 1。 */
export function computeHighRefreshFactor(refreshHz: number, enabled = true): number {
  if (!enabled) return 1;
  return refreshHz >= 120 ? 0.85 : 1;
}

/** 把 --dur-* 令牌值（"240ms"）乘系数；非 ms 值原样返回。 */
export function scaleDurValue(value: string, factor: number): string | null {
  const m = value.trim().match(/^(-?\d*\.?\d+)ms$/i);
  if (!m || factor === 1) return null;
  const scaled = Math.round(parseFloat(m[1] ?? "0") * factor * 100) / 100;
  return `${scaled}ms`;
}

// ---------------------------------------------------------------------------
// M-42 多实例应用任务栏区分（Instance Badging）
// ---------------------------------------------------------------------------

export interface TaskWindowLite { exePath: string; title: string }

export interface InstanceBadge { count: number; badge: string; tooltip: string }

/** 按 exe path 聚合计数；≤3 显示数字，>3 显示 3+；tooltip 取区分性窗口标题（首个非空标题）。 */
export function badgeInstances(windows: TaskWindowLite[]): Map<string, InstanceBadge> {
  const groups = new Map<string, TaskWindowLite[]>();
  for (const w of windows) {
    const key = w.exePath.toLowerCase();
    const arr = groups.get(key) ?? [];
    arr.push(w);
    groups.set(key, arr);
  }
  const out = new Map<string, InstanceBadge>();
  for (const [key, arr] of groups) {
    const count = arr.length;
    out.set(key, {
      count,
      badge: count <= 3 ? String(count) : "3+",
      tooltip: count > 1 ? (arr.find((w) => w.title.trim())?.title ?? arr[0]?.title ?? "") : arr[0]?.title ?? "",
    });
  }
  return out;
}

// ---------------------------------------------------------------------------
// M-44 嵌入应用崩溃善后（Embed Crash Aftercare）
// 不自动重启（用户决定）；同应用 60s 内合并卡片。
// ---------------------------------------------------------------------------

export interface CrashRecord { appKey: string; ts: number; pid?: number; exitCode?: number }

export function isWithinMergeWindow(prev: CrashRecord | undefined, ts: number, windowMs = 60_000): boolean {
  return prev !== undefined && ts - prev.ts < windowMs;
}

/** 取证日志（本地，最多 50 条）。 */
export function appendCrashLog(log: CrashRecord[], rec: CrashRecord): CrashRecord[] {
  const next = [...log, rec];
  return next.length > 50 ? next.slice(next.length - 50) : next;
}

// ---------------------------------------------------------------------------
// M-45 输入设备热插拔稳定（Input Hot-Plug Stability）
// ---------------------------------------------------------------------------

/** 指数退避重注册延迟表（5 次）：400,800,1600,3200,6400ms，总窗口 ≤ 12s。 */
export function hotplugBackoffDelays(baseMs = 400, tries = 5): number[] {
  return Array.from({ length: tries }, (_, i) => baseMs * 2 ** i);
}

export interface QueuedAction { at: number; run: () => void }

/** 重挂期间 dispatch 队列：窗口期 ≤1s，超期的动作丢弃（不堆积不重放旧动作）。 */
export function drainFreshActions(queue: QueuedAction[], now: number, windowMs = 1000): QueuedAction[] {
  const fresh: QueuedAction[] = [];
  for (const a of queue) {
    if (now - a.at <= windowMs) fresh.push(a);
  }
  return fresh;
}
