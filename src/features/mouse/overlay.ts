/**
 * J 鼠标域 · F620 指针衬底与投影。
 *
 * 复杂壁纸上的指针可见性三件套：可选 1px 反色描边（自动取指针色相反色——
 * 任何底色都看得清）、可选柔投影（4px 偏移 30% 透明）、可选高对比衬圈
 * （3px 圆衬）；三件独立开关、渲染走 F335 优先平面零帧耗；与 F348 加粗档位
 * 叠加兼容（加粗大指针同样可带衬底）。
 *
 * 默认态=描边开、投影关、衬圈关（可见性底线自动兜住、其余不加戏）。
 *
 * 判据锚点：
 * - 反色描边对拍（深/浅/花三底色走查）→ invertColor()（任意底色取反色）
 * - 投影参数实测 → SHADOW_PRESET（4px 偏移 30% 透明）
 * - 三开关独立 / 默认态审计 → DEFAULT_OVERLAY
 * - 优先平面零帧耗 → 产出为纯 CSS（GPU 合成层，无逐帧 JS）
 */

export interface PointerOverlayConfig {
  outline: boolean;
  shadow: boolean;
  ring: boolean;
}

/** 默认态：描边开（可见性底线）、投影关、衬圈关。 */
export const DEFAULT_OVERLAY: PointerOverlayConfig = { outline: true, shadow: false, ring: false };

/** 投影固定参数（4px 偏移 30% 透明——判据原文，不开放调参）。 */
export const SHADOW_PRESET = { offsetX: 4, offsetY: 4, blur: 6, alpha: 0.3 } as const;
/** 衬圈固定 3px（判据原文）。 */
export const RING_PX = 3;
/** 描边固定 1px（判据原文）。 */
export const OUTLINE_PX = 1;

/** 指针主体色（与 F348 加粗档同源——加粗大指针同样可带衬底）。 */
export const POINTER_CORE_COLOR = "#ffffff";

/**
 * 反色描边色：指针色相反色（HSL 亮度反转——深底出浅描边、浅底出深描边、
 * 花色底取平均亮度反差）。输入任意 CSS 十六进制色。
 */
export function invertColor(hex: string): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return "#000000"; // 解析失败兜底深描边（白指针场景正确）——异常不静默
  const n = parseInt(m[1]!, 16);
  const r = (n >> 16) & 0xff;
  const g = (n >> 8) & 0xff;
  const b = n & 0xff;
  // 相对亮度反转（感知亮度权重，比逐通道反转更贴近「看得清」的目标）。
  const lum = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  const t = lum > 0.5 ? 0 : 255;
  const to2 = (v: number): string => v.toString(16).padStart(2, "0");
  return `#${to2(t)}${to2(t)}${to2(t)}`;
}

export interface OverlayStyle {
  /** F335 优先平面 CSS（filter/drop-shadow 只作用于指针层，内容零重绘）。 */
  cssFilter: string;
  /** 衬底圈用 box-shadow（画在指针外围，不占布局）。 */
  boxShadow: string;
  /** 是否需要任何附加层（三件全关时 false——零开销）。 */
  active: boolean;
}

/**
 * 三件套合成：三开关独立，各自贡献自己的层；全关时 active=false（运行时
 * 直接不挂层——零帧耗的机械保证）。
 */
export function composeOverlay(cfg: PointerOverlayConfig, coreColor: string = POINTER_CORE_COLOR): OverlayStyle {
  const layers: string[] = [];
  if (cfg.outline) {
    // 1px 反色描边：drop-shadow 两向叠加形成描边（GPU 合成零逐帧开销）。
    const c = invertColor(coreColor);
    layers.push(`drop-shadow(1px 0 0 ${c})`, `drop-shadow(-1px 0 0 ${c})`, `drop-shadow(0 1px 0 ${c})`, `drop-shadow(0 -1px 0 ${c})`);
  }
  const shadows: string[] = [];
  if (cfg.shadow) {
    shadows.push(`${SHADOW_PRESET.offsetX}px ${SHADOW_PRESET.offsetY}px ${SHADOW_PRESET.blur}px rgba(0,0,0,${SHADOW_PRESET.alpha})`);
  }
  if (cfg.ring) {
    // 3px 高对比衬圈（反色，双圈增强在花色底上的分辨力）。
    const c = invertColor(coreColor);
    shadows.push(`0 0 0 ${RING_PX}px ${c}`);
  }
  return { cssFilter: layers.join(" "), boxShadow: shadows.join(", "), active: cfg.outline || cfg.shadow || cfg.ring };
}

/** 三底色走查样本（深/浅/花——面板与对拍脚本同源）。 */
export const OVERLAY_WALKTHROUGH_BACKGROUNDS = [
  { id: "deep", name: "深色底", color: "#101418" },
  { id: "light", name: "浅色底", color: "#f5f2ec" },
  { id: "floral", name: "花色底", color: "#7a6a8f" },
] as const;
