/**
 * L-2 硬件自动分级渲染（终极形态施工总计划 5.2）。
 *
 * 分级规则：
 * - S 档（独显 ≥4GB / 新架构集显）：完整星空 + 极光 + 毛玻璃动效
 * - A 档（集显 / 内存 ≤8GB）：星空开、极光半分辨率、动效简化
 * - B 档（低配核显 / VM 无 GPU 加速）：静态星空单帧 + 视频壁纸降帧 30fps + 关粒子
 * - C 档（SafeMode / 渲染超时）：全静态
 *
 * 优先级：手动覆盖（bgTier / reduceMotion）优先；否则自动档。
 * 渲染超时看门狗：首帧 > 3s 自动降档一次，并按 GPU 描述哈希持久记忆（localStorage，
 * 宿主机维度；引擎内无宿主文件写权限，不落盘介质以保护 U 盘寿命）。
 */

export type AutoTier = "S" | "A" | "B" | "C";

const LS_OVERRIDE_KEY = "var.autotier.override"; // 用户在设置页的显式选择（覆盖自动）
const LS_DOWNGRADE_PREFIX = "var.autotier.dg."; // + GPU 哈希 → 降档后的档位
const DOWNGRADE_WATCHDOG_MS = 3000;

export interface AutoTierResult {
  tier: AutoTier;
  gpuHash: string;
  gpuName: string;
  reason: string;
}

function djb2(s: string): string {
  let h = 5381;
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) >>> 0;
  return h.toString(16);
}

function getWebglRenderer(): string {
  try {
    const canvas = document.createElement("canvas");
    const gl = (canvas.getContext("webgl2") ||
      canvas.getContext("webgl")) as WebGLRenderingContext | null;
    if (!gl) return "";
    const ext = gl.getExtension("WEBGL_debug_renderer_info");
    const name = ext
      ? String(gl.getParameter(ext.UNMASKED_RENDERER_WEBGL))
      : String(gl.getParameter(gl.RENDERER) ?? "");
    const lose = gl.getExtension("WEBGL_lose_context");
    lose?.loseContext();
    return name;
  } catch {
    return "";
  }
}

function hasWebGpu(): boolean {
  return typeof (navigator as unknown as { gpu?: unknown }).gpu !== "undefined";
}

/** 引擎自查（无引导器探测结果时）：一次探测 <50ms，不做任何渲染尝试 */
export function detectAutoTier(safeMode: boolean): AutoTierResult {
  const gpuName = getWebglRenderer();
  const gpuHash = djb2(gpuName || "no-gpu");
  const nav = navigator as Navigator & { deviceMemory?: number };
  const memGb = nav.deviceMemory ?? 0;

  // C 档：SafeMode 或无任何 GPU 加速且 WebGPU 也不可用
  if (safeMode) {
    return { tier: "C", gpuHash, gpuName, reason: "SafeMode" };
  }
  const noGpu = !gpuName && !hasWebGpu();
  if (noGpu) {
    return { tier: "C", gpuHash, gpuName, reason: "无 GPU 加速" };
  }

  // B 档：VM/虚拟显卡、低配核显、低内存设备
  const vmLike = /hyper-v|vmvga|vbox|virtual|swiftshader|llvmpipe|software/i.test(gpuName);
  const lowMem = memGb > 0 && memGb <= 4;
  const lowEndGpu = /intel.*(hd|uhd) graphics ([2-6])\d{2}\b|basic render|angle \(/i.test(gpuName);
  if (vmLike || lowMem || lowEndGpu) {
    return {
      tier: "B",
      gpuHash,
      gpuName,
      reason: vmLike ? "VM 虚拟显卡" : lowMem ? `内存 ${memGb}GB` : "低配核显",
    };
  }

  // S 档：WebGPU 可用 + 独显/新架构（NVIDIA/AMD/Apple，或 Intel Arc/Iris）
  const dgpu = /nvidia|geforce|rtx|gtx|quadro|radeon rx|apple m\d|arc|iris xe/i.test(gpuName);
  if (hasWebGpu() && dgpu) {
    return { tier: "S", gpuHash, gpuName, reason: `独显/WebGPU（${gpuName}）` };
  }

  // 其余（含集显、内存 ≤8GB）→ A 档
  return { tier: "A", gpuHash, gpuName, reason: `集显/常规（${gpuName || "集成"}）` };
}

/** 渲染超时看门狗：从 start 起算，首帧超时返回 true（调用方执行降档） */
export function startFirstFrameWatchdog(
  onTimeout: () => void,
  markFirstFrame: () => void,
): () => void {
  const timer = window.setTimeout(() => {
    onTimeout();
  }, DOWNGRADE_WATCHDOG_MS);
  return () => {
    window.clearTimeout(timer);
    markFirstFrame();
  };
}

/** 降档持久记忆（宿主机维度 = GPU 哈希维度） */
export function persistDowngrade(gpuHash: string, tier: AutoTier): void {
  try {
    localStorage.setItem(LS_DOWNGRADE_PREFIX + gpuHash, tier);
  } catch {
    /* 隐私模式等场景忽略 */
  }
}

function readDowngrade(gpuHash: string): AutoTier | null {
  try {
    return localStorage.getItem(LS_DOWNGRADE_PREFIX + gpuHash) as AutoTier | null;
  } catch {
    return null;
  }
}

/** 手动覆盖存取（设置页「改手动入口」） */
export function setManualOverride(tier: AutoTier | null): void {
  try {
    if (tier === null) localStorage.removeItem(LS_OVERRIDE_KEY);
    else localStorage.setItem(LS_OVERRIDE_KEY, tier);
  } catch {
    /* ignore */
  }
}
export function getManualOverride(): AutoTier | null {
  try {
    return localStorage.getItem(LS_OVERRIDE_KEY) as AutoTier | null;
  } catch {
    return null;
  }
}

const ORDER: AutoTier[] = ["C", "B", "A", "S"];
function minTier(a: AutoTier, b: AutoTier): AutoTier {
  return ORDER.indexOf(a) <= ORDER.indexOf(b) ? a : b;
}

/**
 * 综合解析最终档位：手动覆盖 > 看门狗降档记忆 > 自动探测。
 * @param manualBgTier 现有 settings.bgTier（≥1 = 手动档位覆盖）
 * @param reduceMotion 现有 reduceMotion（true = 用户主动要求静态）
 */
export function resolveAutoTier(
  safeMode: boolean,
  manualBgTier: number,
  reduceMotion: boolean,
): AutoTierResult {
  const auto = detectAutoTier(safeMode);
  // 手动覆盖优先：bgTier ≥1 视为手动档（沿用十档矩阵的粗映射），reduceMotion 同理
  if (manualBgTier >= 1 || reduceMotion) {
    return { ...auto, tier: auto.tier, reason: "手动覆盖优先（自动档被抑制）" };
  }
  const dg = readDowngrade(auto.gpuHash);
  if (dg) {
    return { ...auto, tier: minTier(auto.tier, dg), reason: `${auto.reason}（曾超时降档）` };
  }
  return auto;
}

/** 档位 → 现有十档 bgTier 矩阵取值（供 CosmicBackground 消费） */
export function tierToBgTier(tier: AutoTier): number {
  switch (tier) {
    case "S":
      return 10;
    case "A":
      return 6;
    case "B":
      return 2;
    case "C":
      return 0;
  }
}

/** 极光半分辨率（A 档）与视频壁纸降帧（B 档）提示，全局消费 */
export function applyTierHints(tier: AutoTier): void {
  const root = document.documentElement;
  root.dataset.autoTier = tier;
  root.dataset.auroraHalfRes = tier === "A" ? "1" : "0";
  root.dataset.videoWallpaperFps = tier === "B" || tier === "C" ? "30" : "0";
  root.dataset.particlesOff = tier === "B" || tier === "C" ? "1" : "0";
}
