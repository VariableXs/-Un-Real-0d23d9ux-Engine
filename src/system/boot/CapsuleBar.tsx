import { useEffect, useRef, useState } from "react";

/**
 * U-03 胶囊进度条（真实进度，单调由上层契约保证；游戏级加载条重铸）。
 *
 * - 全宽 HUD 布局：百分比浮于条体右上（tabular-nums），条体撑满字标宽度。
 * - 体育场胶囊：border-radius 精确 = 高度/2（CSS calc）；12px 轨高 + 4px 圆柱渐变填充。
 * - 分段刻度槽（repeating-linear-gradient 24px 节距）：填充越过时刻度显现 —— HUD 分段感。
 * - 扫光（sheen）：填充内 25% 宽斜向高光带 2.4s 循环横扫（transform-only，随宽度自适应）；
 *   完成后停摆；停滞时减速扫掠（3.6s）呼应等待态。
 * - 前导 7px 光点：同色调 80% 透明度（非彩色霓虹）；停滞时转为呼吸等待。
 * - progress===1：一次性「满格呼吸」（brightness +8%，300ms）；右端点灯，左端点停止呼吸。
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
      {/* 顶部 HUD 行：百分比右对齐（等宽数字，加载条唯一文案） */}
      <div className="boot-capsule-head">
        <span className="boot-capsule-pct" aria-live="polite">
          {(pct * 100).toFixed(1)}%
        </span>
      </div>
      {/* 条体：端点 + 胶囊轨道 */}
      <div className="boot-capsule-body">
        <span className="boot-capsule-endpoint is-left" aria-hidden="true" />
        <div className="boot-capsule-shell">
          <div className="boot-capsule-fill" style={{ width: `${(pct * 100).toFixed(2)}%` }}>
            {/* 扫光裁剪容器（不裁前导光点） */}
            <span className="boot-capsule-clip" aria-hidden="true">
              <span className="boot-capsule-sheen" />
            </span>
            <span className="boot-capsule-glow" aria-hidden="true" />
          </div>
          {/* 分段刻度槽：覆于填充之上，填充越过后逐格点亮 */}
          <span className="boot-capsule-ticks" aria-hidden="true" />
        </div>
        <span className="boot-capsule-endpoint is-right" aria-hidden="true" />
      </div>
    </div>
  );
}
