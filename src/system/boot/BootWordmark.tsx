import { GLYPHS, WORDMARK_LETTERS } from "../../assets/wordmark/glyphs";

/**
 * U-02 VARIABLE 字标（八个手绘字形，逐字母映射真实进度）。
 *
 * - 每字母owns 12.5% 进度区间：letterProgress(i) = clamp(progress*8 - i, 0, 1)；
 *   dasharray = 校准长度常量，dashoffset 逐帧由上层 rAF 平滑值驱动（本组件不加过渡）。
 * - 颜色全部 currentColor（继承 CSS var(--text-primary) 等），无逐字母渐变。
 * - 描边 1.5px + non-scaling-stroke：24px 高度下依旧发丝清晰。
 * - 底衬（12% 实心）入场级联 40ms 间隔；ready 退出编排时实心化（见 boot.css）。
 */
export function BootWordmark(props: {
  /** 宽度倍率（1 = 撑满容器）。 */
  scale?: number;
  /** light=深底浅字（默认）；dark=浅底深字。 */
  tone?: "light" | "dark";
  /** 0..1 真实进度（上层已做单调保证 + rAF 平滑）。 */
  progress: number;
}): React.ReactElement {
  const { scale = 1, tone = "light", progress } = props;

  return (
    <div
      className={`boot-wordmark tone-${tone}`}
      style={{ width: `${(scale * 100).toFixed(3)}%` }}
      role="img"
      aria-label="VARIABLE"
    >
      {WORDMARK_LETTERS.map((letter, i) => {
        const g = GLYPHS[letter];
        if (!g) return null;
        const p = Math.min(1, Math.max(0, progress * 8 - i));
        return (
          <svg key={`${letter}-${i}`} className="boot-wm-letter" viewBox={g.viewBox} aria-hidden="true">
            {/* 12% 实心底衬：入场级联（40ms 间隔，inline delay），退出编排时实心化 */}
            <path
              className="boot-wm-underlay"
              d={g.path}
              style={{ animationDelay: `${i * 40}ms` }}
            />
            {/* 真实进度描边：无 CSS 过渡，dashoffset 由 rAF 逐帧驱动 */}
            <path
              className="boot-wm-draw"
              d={g.path}
              fill="none"
              stroke="currentColor"
              strokeWidth={1.5}
              strokeDasharray={g.length}
              strokeDashoffset={g.length * (1 - p)}
              vectorEffect="non-scaling-stroke"
            />
          </svg>
        );
      })}
    </div>
  );
}
