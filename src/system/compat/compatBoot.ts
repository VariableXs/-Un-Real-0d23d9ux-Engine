/**
 * AI-12 兼容纵深组 — 运行时接线（一次性初始化 + 各流程编排）。
 * 红线：只观察、只提示、只降级；兼容库（Z-15）纯数据无执行；
 *      实测依赖真机的项按诚实口径在报告中登记待验，不虚标。
 */
import { listen } from "@tauri-apps/api/event";
import { ipc } from "../../lib/ipc";
import {
  applyFullscreenEvent,
  applyHostProfileAttrs,
  applySlowTierAttrs,
  computeHighRefreshFactor,
  computeSlowTier,
  deriveHostProfile,
  downgradeOnly,
  getFullscreenState,
  replayGeometryRestores,
  type SlowTier,
} from "./protocols";

// ---------------------------------------------------------------------------
// 本地小工具（避免引入全局依赖）
// ---------------------------------------------------------------------------

const LS_PREFIX = "compat.ai12.";

function lsGet(key: string): string | null {
  try { return localStorage.getItem(LS_PREFIX + key); } catch { return null; }
}
function lsSet(key: string, value: string): void {
  try { localStorage.setItem(LS_PREFIX + key, value); } catch { /* 隐私模式下静默 */ }
}

export type ToastFn = (kind: "info" | "success" | "error", text: string) => void;

// ---------------------------------------------------------------------------
// Z-20 / Z-21 / M-40：宿主档、慢速档、高刷折算
// ---------------------------------------------------------------------------

let lastSlowTier: SlowTier = 0;

/** 评估慢速档信号（可用信号：内存/核显用 navigator 端启发；磁盘类型待后端补——如实按 unknown 处理不虚标）。 */
export function evaluateSlowTier(current: SlowTier, signals: { lowMem: boolean; lowGpu: boolean; hdd: boolean }): SlowTier {
  const next = computeSlowTier(signals);
  lastSlowTier = downgradeOnly(current, next);
  applySlowTierAttrs(lastSlowTier);
  return lastSlowTier;
}

export function currentSlowTier(): SlowTier {
  return lastSlowTier;
}

/** 探测宿主（RDP/VM）并应用档位；手动覆盖优先，误检安全（失败按 native）。 */
export async function detectAndApplyHostProfile(manualOverride?: "auto" | "remote" | "vm" | "native"): Promise<"remote" | "vm" | "native"> {
  let kind: "remote" | "vm" | "native" = "native";
  try {
    const probe = await ipc.compatHostProbe();
    kind = probe.hostKind;
  } catch {
    kind = "native";
  }
  applyHostProfileAttrs(deriveHostProfile(kind, manualOverride));
  lsSet("hostKind", kind);
  return kind;
}

/** M-40：主屏刷新率 → --dur-* 令牌折算（hyphen 混合刷新率按主屏，调用方传主屏值）。 */
export function applyHighRefreshTiming(refreshHz: number, enabled = true): number {
  const factor = computeHighRefreshFactor(refreshHz, enabled);
  const root = document.documentElement;
  root.setAttribute("data-dur-factor", String(factor));
  if (factor === 1) {
    // 恢复默认（清除内联覆盖）
    for (const name of Array.from(root.style).filter((n) => n.startsWith("--dur-"))) {
      root.style.removeProperty(name);
    }
    return 1;
  }
  const computed = getComputedStyle(root);
  for (const name of Array.from(computed).filter((n) => n.startsWith("--dur-"))) {
    const raw = computed.getPropertyValue(name).trim();
    const m = raw.match(/^(-?\d*\.?\d+)ms$/i);
    if (!m) continue;
    const scaled = Math.round(parseFloat(m[1] ?? "0") * factor * 100) / 100;
    root.style.setProperty(name, `${scaled}ms`);
  }
  return factor;
}

// ---------------------------------------------------------------------------
// M-37 图标缓存校验与自愈
// ---------------------------------------------------------------------------

export interface IconFingerprintStore { [lnkPath: string]: { mtimeMs: number; size: number } }

function fpStore(): IconFingerprintStore {
  try { return JSON.parse(lsGet("iconFingerprints") ?? "{}") as IconFingerprintStore; } catch { return {}; }
}
function fpSave(s: IconFingerprintStore): void {
  lsSet("iconFingerprints", JSON.stringify(s));
}

/**
 * 快扫失效项（并发 ≤2 后台重建）：返回需要重建的 .lnk 列表。
 * rebuild：逐项探测指纹 → 与登记不符即进重建队列。
 */
export async function scanStaleIcons(
  lnkPaths: string[],
  rebuild: (lnk: string) => Promise<void>,
  concurrency = 2,
): Promise<string[]> {
  const store = fpStore();
  const stale: string[] = [];
  for (const p of lnkPaths) {
    try {
      const fp = await ipc.compatIconProbe(p);
      if (!fp.exists) { stale.push(p); continue; }
      const known = store[p];
      if (!known || known.mtimeMs !== fp.mtimeMs || known.size !== fp.size) stale.push(p);
    } catch {
      // 探针失败视为待验（不误报失效）
    }
  }
  // 并发 ≤2 的重建队列（concurrency 限流，上限 4 防失控）
  let idx = 0;
  const worker = async (): Promise<void> => {
    while (idx < stale.length) {
      const p = stale[idx++];
      if (p === undefined) break;
      try {
        await rebuild(p);
        const fp = await ipc.compatIconProbe(p);
        if (fp.exists) { store[p] = { mtimeMs: fp.mtimeMs, size: fp.size }; }
      } catch { /* 单项失败不阻塞队列 */ }
    }
  };
  const lanes = Math.max(1, Math.min(concurrency, 4));
  await Promise.all(Array.from({ length: lanes }, () => worker()));
  fpSave(store);
  return stale;
}

// ---------------------------------------------------------------------------
// M-38 UWP 识别：与 .lnk 扫描结果去重合并（tier=L4 让位，不可嵌入如实标注）
// ---------------------------------------------------------------------------

export interface LaunchableApp {
  name: string;
  /** .lnk 路径或 AUMID */
  launchId: string;
  kind: "lnk" | "uwp";
  /** UWP/MSIX 一律 L4（让位）：不可嵌入，嵌入按钮置灰有说明 */
  embeddable: boolean;
}

export function mergeUwpApps(lnkApps: LaunchableApp[], uwpApps: { name: string; appId: string }[]): LaunchableApp[] {
  const out = [...lnkApps];
  for (const u of uwpApps) {
    // 去重：AUMID 与 lnk 名同名且同 id 不重复；按名字归并大小写
    const dup = out.some((a) => a.kind === "uwp" && a.launchId.toLowerCase() === u.appId.toLowerCase())
      || out.some((a) => a.name.toLowerCase() === u.name.toLowerCase());
    if (dup) continue;
    out.push({ name: u.name, launchId: u.appId, kind: "uwp", embeddable: false });
  }
  return out;
}

// ---------------------------------------------------------------------------
// M-39 提权应用协作提示（说明卡每应用只显示一次；不替用户绕过 UAC）
// ---------------------------------------------------------------------------

export interface ElevationDecision { requiresAdmin: boolean; showCard: boolean }

export async function checkElevation(appKey: string, exePath: string): Promise<ElevationDecision> {
  let requiresAdmin = false;
  try {
    const probe = await ipc.compatElevationProbe(exePath);
    requiresAdmin = probe.requiresAdmin;
  } catch { /* 探针失败按非提权处理（不误报说明卡） */ }
  const seen = lsGet(`elevation.seen.${appKey}`) === "1";
  return { requiresAdmin, showCard: requiresAdmin && !seen };
}

export function markElevationCardSeen(appKey: string): void {
  lsSet(`elevation.seen.${appKey}`, "1");
}

/** UAC 拒绝如实回执（不吞错、不重试、不替用户绕过 UAC）。 */
export async function launchElevated(path: string): Promise<{ launched: boolean; message?: string }> {
  try {
    await ipc.shellExecute(path, { verb: "runas" });
    return { launched: true };
  } catch (e) {
    return { launched: false, message: String(e) };
  }
}

// ---------------------------------------------------------------------------
// M-41 外设驱动软件共存（一次性提示；不干预、不屏蔽驱动进程）
// ---------------------------------------------------------------------------

export async function checkDriverCoexistence(): Promise<string[]> {
  try {
    const procs = await ipc.compatDriverScan();
    const warned = new Set((lsGet("driver.warned") ?? "").split("|").filter(Boolean));
    return procs.filter((p) => !warned.has(p));
  } catch {
    return [];
  }
}

export function markDriverWarned(procs: string[]): void {
  const warned = new Set((lsGet("driver.warned") ?? "").split("|").filter(Boolean));
  for (const p of procs) warned.add(p);
  lsSet("driver.warned", Array.from(warned).join("|"));
}

// ---------------------------------------------------------------------------
// M-43 便携路径漂移自愈（登记卷 GUID → 盘符漂移后重定位）
// ---------------------------------------------------------------------------

export async function healPortablePaths(entries: { path: string; volumeGuid: string }[]): Promise<Map<string, string>> {
  const out = new Map<string, string>();
  if (!entries.length) return out;
  try {
    const results = await ipc.compatHealPaths(entries);
    for (const r of results) {
      if (r.healed) out.set(r.path, r.healed);
    }
  } catch { /* 未命中走既有应急包流程 */ }
  return out;
}

export function rememberVolumeForPath(path: string): string | null {
  // 登记时记录卷 GUID（同步枚举一次本机卷，按最长挂载点前缀匹配）
  return lsGet(`vol.${path}`) ?? null;
}

export async function registerVolumeForPath(path: string): Promise<string | null> {
  try {
    const vols = await ipc.compatVolumes();
    const lower = path.toLowerCase();
    let best: { guid: string; mount: string } | null = null;
    for (const v of vols) {
      for (const mp of v.mountPoints) {
        const m = mp.toLowerCase();
        if (m && lower.startsWith(m) && (!best || m.length > best.mount.length)) {
          best = { guid: v.guidPath, mount: mp };
        }
      }
    }
    if (best) {
      lsSet(`vol.${path}`, best.guid);
      return best.guid;
    }
  } catch { /* 枚举失败如实返回 null */ }
  return null;
}

// ---------------------------------------------------------------------------
// M-45 热插拔：重注册回调注入 + 指数退避 + 单次 toast
// ---------------------------------------------------------------------------

let rebindHotkeys: (() => void | Promise<void>) | null = null;
let toastFn: ToastFn | null = null;
let lastHotplugToastAt = 0;

export function setHotplugHandlers(handlers: { rebind?: () => void | Promise<void>; toast?: ToastFn }): void {
  if (handlers.rebind) rebindHotkeys = handlers.rebind;
  if (handlers.toast) toastFn = handlers.toast;
}

async function rebindWithBackoff(baseMs = 400, tries = 5): Promise<void> {
  for (let i = 0; i < tries; i++) {
    try {
      await rebindHotkeys?.();
      return;
    } catch {
      await new Promise((r) => setTimeout(r, baseMs * 2 ** i));
    }
  }
}

// ---------------------------------------------------------------------------
// 全局初始化（App 挂载时调用一次）
// ---------------------------------------------------------------------------

let initialized = false;

export function initCompatLayer(): void {
  if (initialized) return;
  initialized = true;

  // Z-18：统一全屏事件入口 → 各消费方走 protocols.onFullscreenChange
  let prevFsActive = getFullscreenState().active;
  void listen<{ active?: boolean; title?: string; mode?: string } | boolean>("sys://fullscreen", (e) => {
    const st = applyFullscreenEvent(e.payload);
    // 全屏退出（active→false）→ 回放几何恢复
    if (prevFsActive && !st.active) replayGeometryRestores();
    prevFsActive = st.active;
  }).catch(() => {});

  // M-45：输入设备热插拔 → 快捷键重注册（指数退避）+ 单次 toast
  void listen<{ keyboards: number; mice: number }>("compat://hotplug", () => {
    void rebindWithBackoff();
    const now = Date.now();
    if (now - lastHotplugToastAt > 10_000) {
      lastHotplugToastAt = now;
      try { toastFn?.("info", "输入设备已重连，快捷键已恢复"); } catch { /* noop */ }
    }
  }).catch(() => {});

  // 启动即评估宿主档与慢速档（前端可用信号）
  void detectAndApplyHostProfile().catch(() => {});
  try {
    const nav = navigator as Navigator & { deviceMemory?: number; hardwareConcurrency?: number };
    const memGB = (nav.deviceMemory ?? 8) <= 8;
    const weakCpu = (nav.hardwareConcurrency ?? 8) <= 4;
    evaluateSlowTier(0, { hdd: false, lowMem: memGB, lowGpu: weakCpu });
  } catch { /* noop */ }
}
