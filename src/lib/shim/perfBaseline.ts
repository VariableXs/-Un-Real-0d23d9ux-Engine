/**
 * 任务 29（AI-V）· 降级提示文案映射 + 性能采样基线。
 *
 * 降级面：垫片三段式结果（MAPPED_ERR/MISSING）与能力位缺失 → 用户可读 i18n 词条
 * （dictionaries.degrade*，zh/en 全量；zh-TW 走 zh 基底繁体转换）。禁裸错误码直达用户。
 *
 * 性能基线：首帧耗时 + 交互延迟采样（performance.now 真实时钟），持久化
 * localStorage `variable:perf:baseline:v1`，供任务 71 U 盘场景基准与回归门禁读取。
 * 零采集红线兼容：不采集任何内容数据，只记时长数字。
 */
import type { Lang } from "../../i18n/dictionaries";
import { translate } from "../../i18n/dictionaries";
import type { ShimCapability, ShimErrorCode } from "./protocol";

/** 降级提示键（与 dictionaries.degrade* 一一对应）。 */
export type DegradeKey =
  | "degradeBackendDown"
  | "degradeUnsupported"
  | "degradeMissing"
  | "degradeTimeout"
  | "degradeVersionMismatch"
  | "degradePermDenied"
  | "degradeInternal"
  | "degradeInvalidArgs"
  | "degradeCapMissing";

/** 降级码 = 七个 ShimErrorCode + 合成的 MISSING（协议第三段独立于错误码枚举）。 */
export type DegradeCode = ShimErrorCode | "SHIM_MISSING";

/** 错误码 → 词条 key（全量覆盖七个 ShimErrorCode + MISSING，无遗漏分支）。 */
const CODE_TO_KEY: Record<DegradeCode, DegradeKey> = {
  SHIM_BACKEND_DOWN: "degradeBackendDown",
  SHIM_UNSUPPORTED: "degradeUnsupported",
  SHIM_MISSING: "degradeMissing",
  SHIM_TIMEOUT: "degradeTimeout",
  SHIM_VERSION_MISMATCH: "degradeVersionMismatch",
  SHIM_PERM_DENIED: "degradePermDenied",
  SHIM_INTERNAL: "degradeInternal",
  SHIM_INVALID_ARGS: "degradeInvalidArgs",
};

/** MISSING 场景（命令未接入）的合成码——调用方把 ShimMissingError 归一为它。 */
export const MISSING_AS_CODE: DegradeCode = "SHIM_MISSING";

/**
 * 降级提示文案（用户可读；{cmd}/{cap}/{front}/{back} 插值由 translate 完成）。
 * lang 跟随当前界面语言；词条缺失时 translate 回落 zh，再回落 key 本身（可测）。
 */
export function degradeMessage(
  lang: Lang,
  code: DegradeCode,
  params?: { cmd?: string; cap?: string; front?: number; back?: number },
): string {
  const key = CODE_TO_KEY[code];
  return translate(lang, key, {
    cmd: params?.cmd ?? "",
    cap: params?.cap ?? "",
    front: params?.front ?? 0,
    back: params?.back ?? 0,
  });
}

/** 能力位缺失降级提示（capability 不满足时调用方用）。 */
export function degradeCapMissing(lang: Lang, cap: ShimCapability): string {
  return translate(lang, "degradeCapMissing", { cap });
}

// ---------- 性能采样基线（首帧 / 交互延迟 P95） ----------

export const PERF_BASELINE_KEY = "variable:perf:baseline:v1";
/** 交互延迟样本上限（滚动窗口；防无限膨胀）。 */
const SAMPLE_CAP = 200;

export interface PerfBaseline {
  /** 首帧耗时 ms（模块安装 → 首个 rAF 回调；一次会话一个值）。 */
  firstFrameMs: number | null;
  /** 交互延迟样本 ms（keydown/pointerdown → 下一帧 rAF）。 */
  interactionSamples: number[];
  /** 首帧记录时刻（距 Unix 纪元 ms）。 */
  recordedAt: number | null;
}

function emptyBaseline(): PerfBaseline {
  return { firstFrameMs: null, interactionSamples: [], recordedAt: null };
}

export function loadPerfBaseline(storage?: Pick<Storage, "getItem" | "setItem">): PerfBaseline {
  const st = storage ?? safeStorage();
  if (!st) return emptyBaseline();
  try {
    const raw = JSON.parse(st.getItem(PERF_BASELINE_KEY) ?? "") as Partial<PerfBaseline> | null;
    if (!raw || typeof raw !== "object") return emptyBaseline();
    const samples = Array.isArray(raw.interactionSamples)
      ? raw.interactionSamples.filter((n): n is number => typeof n === "number" && n >= 0 && Number.isFinite(n)).slice(-SAMPLE_CAP)
      : [];
    return {
      firstFrameMs: typeof raw.firstFrameMs === "number" && raw.firstFrameMs >= 0 ? raw.firstFrameMs : null,
      interactionSamples: samples,
      recordedAt: typeof raw.recordedAt === "number" ? raw.recordedAt : null,
    };
  } catch {
    return emptyBaseline();
  }
}

export function savePerfBaseline(b: PerfBaseline, storage?: Pick<Storage, "getItem" | "setItem">): void {
  const st = storage ?? safeStorage();
  if (!st) return;
  try {
    st.setItem(PERF_BASELINE_KEY, JSON.stringify({ ...b, interactionSamples: b.interactionSamples.slice(-SAMPLE_CAP) }));
  } catch {
    /* storage full/blocked → 基线不持久化（零功能影响） */
  }
}

function safeStorage(): Pick<Storage, "getItem" | "setItem"> | null {
  try {
    if (typeof localStorage === "undefined") return null;
    return localStorage;
  } catch {
    return null;
  }
}

/** P95（与 engineModel.latencyP95 同口径：线性插值；独立实现避免跨层依赖）。 */
export function perfP95(samples: number[]): number {
  if (samples.length === 0) return 0;
  const xs = [...samples].sort((a, b) => a - b);
  const idx = (xs.length - 1) * 0.95;
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  if (lo === hi) return xs[lo]!;
  return xs[lo]! + (xs[hi]! - xs[lo]!) * (idx - lo);
}

/**
 * 性能采样器：绑定真实 DOM 时钟（rAF + 事件监听）。
 * - markFirstFrame：在首个 requestAnimationFrame 回调里落 firstFrameMs。
 * - 交互延迟：pointerdown/keydown → 下一帧 rAF 差值入滚动窗口。
 * 非浏览器环境（vitest node）安全：rAF 缺失时不采样。
 */
export function installPerfSampler(opts?: {
  storage?: Pick<Storage, "getItem" | "setItem">;
  now?: () => number;
}): () => void {
  const w = typeof window !== "undefined" ? window : null;
  if (!w || typeof w.requestAnimationFrame !== "function") return () => {};
  const now = opts?.now ?? (() => performance.now());
  const storage = opts?.storage;

  let firstDone = loadPerfBaseline(storage).firstFrameMs !== null;
  const pendingAt: number[] = [];

  const onFrame = (): void => {
    const t = now();
    if (!firstDone) {
      firstDone = true;
      const b = loadPerfBaseline(storage);
      b.firstFrameMs = t - installPerfSampler.epoch;
      b.recordedAt = Date.now();
      savePerfBaseline(b, storage);
    }
    if (pendingAt.length > 0) {
      const at = pendingAt.shift()!;
      const b = loadPerfBaseline(storage);
      b.interactionSamples = [...b.interactionSamples, Math.max(0, t - at)].slice(-SAMPLE_CAP);
      savePerfBaseline(b, storage);
    }
  };

  const mark = (): void => {
    pendingAt.push(now());
    w.requestAnimationFrame(onFrame);
  };

  installPerfSampler.epoch = now();
  w.requestAnimationFrame(onFrame);
  w.addEventListener("keydown", mark, { capture: true, passive: true });
  w.addEventListener("pointerdown", mark, { capture: true, passive: true });
  return () => {
    w.removeEventListener("keydown", mark, { capture: true });
    w.removeEventListener("pointerdown", mark, { capture: true });
  };
}
/** 采样器安装时刻（首帧耗时基准点；模块级共享，多实例幂等）。 */
export namespace installPerfSampler {
  export let epoch = 0;
}
