/**
 * AI-18 U-53 环境辉光 — 屏幕边缘随壁纸主色呼吸的氛围光层。
 *
 * 口径：
 * - 主色来自当前壁纸 canvas 降采样 + OKLCH 聚类（wallpaperAccent，本地计算）；
 * - 四边内发光（12px 渐隐），呼吸 8s ±6%；壁纸切换光色 1.5s 交叉过渡（CSS）；
 * - 高对比度 / reduce-motion / bgTier 低档 / 省内存模式自动关闭；
 * - 纯白壁纸亮度钳制（clusterDominant 内 0.25..0.82）防刺眼过曝。
 */

import { useEffect, useState } from "react";
import type { Settings } from "../../lib/settings";
import { oklchToCss, sampleAccentCandidates } from "./wallpaperAccent";
import { toAssetUrl } from "../../features/background/CosmicBackground";

export function glowDisabledReasons(s: Settings): string[] {
  const reasons: string[] = [];
  if (s.theme === "high-contrast") reasons.push("hc");
  if (s.reduceMotion) reasons.push("reduce-motion");
  if (s.bgTier <= 1) reasons.push("bg-tier");
  if (s.perfMode === "eco" || s.safeMode) reasons.push("perf");
  return reasons;
}

export function GlowLayer(props: { settings: Settings }): React.ReactElement | null {
  const s = props.settings;
  const mode = s.ambience.glow;
  const [color, setColor] = useState<string | null>(null);

  // 主色采样：图片类壁纸 → 采样；其余（纯色/视频/星空）→ 不出辉光（诚实边界）
  useEffect(() => {
    if (mode === "off" || glowDisabledReasons(s).length > 0) {
      setColor(null);
      return;
    }
    let alive = true;
    const path = s.wallpaperMode === "image" || s.wallpaperMode === "living" || s.wallpaperMode === "hybrid"
      ? s.customBg.imagePath
      : "";
    if (!path) {
      setColor(null);
      return;
    }
    void sampleAccentCandidates(toAssetUrl(path))
      .then((cands) => {
        if (!alive || cands.length === 0) return;
        const top = cands[0];
        if (!top) return;
        const [L, C, H] = top;
        // 辉光用略降饱和的主色（氛围光不抢戏）
        setColor(oklchToCss(Math.min(0.7, L * 0.9), C * 0.6, H));
      })
      .catch(() => {
        if (alive) setColor(null);
      });
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, s.wallpaperMode, s.customBg.imagePath, s.theme, s.reduceMotion, s.bgTier, s.perfMode, s.safeMode]);

  if (mode === "off" || color === null) return null;
  return (
    <div
      className={`ai18-glow ${mode}`}
      style={{ "--ai18-glow-color": color } as React.CSSProperties}
      aria-hidden
    />
  );
}
