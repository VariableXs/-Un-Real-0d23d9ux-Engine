/**
 * AI-17 · Z-65 拖动清晰度策略（Drag Clarity）
 * 拖动中：关闭亚克力采样与阴影重算（body.dragging-clarity，interactions.css），
 * 位图缓存直接搬运；松手后一帧内恢复全质量重绘。
 * 高性能机器全程满清：tier="high" 时不进入降质态。
 */

export type PerfTier = "high" | "medium" | "low";

const DEFAULT_THRESHOLD_MS = 80;

/**
 * 进入拖动降质态（tier=high 直接跳过——满清拖动）。
 * 返回退出函数；调用方在 pointerup/dragend 时调用（一帧内恢复）。
 */
export function beginDragClarity(tier: PerfTier = "medium", thresholdMs = DEFAULT_THRESHOLD_MS): () => void {
  if (tier === "high" || typeof document === "undefined") return () => {};
  document.body.classList.add("dragging-clarity");
  // 稳定超过 thresholdMs 仍未退出（卡死兜底）：强制恢复
  const guard = setTimeout(() => document.body.classList.remove("dragging-clarity"), thresholdMs * 100);
  return () => {
    clearTimeout(guard);
    document.body.classList.remove("dragging-clarity");
  };
}

/** 查询当前是否处于拖动降质态。 */
export function isDragClarityActive(): boolean {
  if (typeof document === "undefined") return false;
  return document.body.classList.contains("dragging-clarity");
}
