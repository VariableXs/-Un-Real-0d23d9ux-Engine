import { useEffect, useRef, useState } from "react";

/**
 * U-03 胶囊进度条（真实进度，单调由上层契约保证）。
 *
 * - 体育场胶囊：border-radius 精确 = 高度/2（CSS calc）。
 * - 外圈 1px 半透明描边（color-mix on var(--text-primary)，无裸 hex）；
 *   内部 3px 填充轨道，填充 = progress × 内宽，白填充 var(--text-primary)。
 * - 前导 6px 光点：同色调 80% 透明度（非彩色霓虹）；停滞时转为呼吸等待。
 * - progress===1：一次性「满格呼吸」（brightness +8%，300ms）；右端点灯，左端点停止呼吸。
 * - 百分比小号等宽字浮于胶囊右侧，aria-live="polite"。
 */
export function CapsuleBar(props: { progress: number; stalled?: boolean }): React.ReactElement {
  const { progress, stalled = false } = props;
  const pct = Math.min(1, Math.max(0, progress));
  const full = pct >= 1;

  const [breath, setBreath] = useState(false);
  const breathedRef = useRef(false);
  useEffect(() => {
    if (pct < 1 || breathedRef.current) return;
    breathedRef.current = true;
    setBreath(true);
    const t = window.setTimeout(() => setBreath(false), 300);
    return () => window.clearTimeout(t);
  }, [pct]);

  return (
    <div
      className={`boot-capsule${full ? " is-complete" : ""}${breath ? " is-breath" : ""}${stalled ? " is-stalled" : ""}`}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(pct * 100)}
    >
      <span className="boot-capsule-endpoint is-left" aria-hidden="true" />
      <div className="boot-capsule-shell">
        <div className="boot-capsule-fill" style={{ width: `${(pct * 100).toFixed(2)}%` }}>
          <span className="boot-capsule-glow" aria-hidden="true" />
        </div>
      </div>
      <span className="boot-capsule-endpoint is-right" aria-hidden="true" />
      <span className="boot-capsule-pct" aria-live="polite">
        {(pct * 100).toFixed(1)}%
      </span>
    </div>
  );
}
