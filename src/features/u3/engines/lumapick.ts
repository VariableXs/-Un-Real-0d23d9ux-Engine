/**
 * F501/F502 深化引擎 · 壁纸亮度采样与图标文字渲染（AI-U3 · lumapick）。
 *
 * 判据唯一源（主册摘文）：
 * - F501「文字双层渲染（柔和投影+微描边，投影透明度 40%、模糊 2px）、深浅壁纸
 *   自动选字色（壁纸亮度分析选白/黑字——F297 压暗协同）、选中态文字底衬（半
 *   透明胶囊底）；五档亮度壁纸×两字色自动选择用例；投影参数实测；4K 渲染精度；
 *   与 F297 压暗联动一致性」。
 * - F502「最多两行、每行约 8 个全角字符、超出第二行尾部省略号；省略的全文看
 *   Tooltip/重命名/属性三条路可达；中英混排换行（不在英文单词中间断）；居中
 *   对齐；Tooltip 联动」。
 *
 * 深化点（v1/v2 只做了判定与换行，本引擎补齐「分析」与「渲染管线」层）：
 * 1. 壁纸亮度不是单点判定而是**矩阵采样**：5×5 网格取感知亮度，截尾均值
 *    （去掉最亮 5% 与最暗 5%）抗高光/暗角失真——这是「壁纸亮度分析」的实义。
 * 2. 判定边界加**滞后带**（hysteresis）：壁纸亮度在阈值附近抖动时字色不闪烁。
 * 3. 4K 渲染精度：投影/描边参数按 DPR 缩放（物理像素口径），高分屏放大不糊。
 * 4. F502 省略全文三路可达的**语义闭环验证**：三路任一被禁用时给出显性降级。
 *
 * 纯函数实现（零 DOM 依赖），供实验室面板与单测共用同一事实源。
 */

/* ------------------------------ F501 采样层 ------------------------------ */

/** 采样网格边长（5×5=25 样点——覆盖中心与四角，避免单点以偏概全）。 */
export const LUMA_GRID = 5;
/** 截尾比例：两端各去掉 10%（高光/暗角抗扰——极端样点不主导判定）。 */
export const LUMA_TRIM_RATIO = 0.1;

/**
 * 从壁纸像素样点序列计算稳健平均亮度。
 * 输入按行优先 5×5 排列的 [r,g,b]（0-1）；输出截尾均值。
 * 非法长度诚实抛错（异常零静默——不给默认值掩盖采样缺陷）。
 */
export function robustWallpaperLuma(samples: Array<[number, number, number]>): number {
  if (samples.length !== LUMA_GRID * LUMA_GRID) {
    throw new Error(`[u3:F501] 采样点必须 ${LUMA_GRID}×${LUMA_GRID}=${LUMA_GRID * LUMA_GRID} 个，实收 ${samples.length}`);
  }
  const lumas = samples
    .map(([r, g, b]) => lumaOf(r, g, b))
    .sort((a, b) => a - b);
  const trim = Math.floor(lumas.length * LUMA_TRIM_RATIO);
  const kept = lumas.slice(trim, lumas.length - trim);
  return kept.reduce((a, v) => a + v, 0) / kept.length;
}

/** Rec.709 感知亮度（sRGB→线性后加权；与 deskicons.luma 同式独立实现——引擎不反向依赖域文件）。 */
export function lumaOf(r: number, g: number, b: number): number {
  const lin = (v: number) => {
    const c = Math.min(1, Math.max(0, v));
    return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** 生成 5×5 确定性样点（验收判据「五档亮度壁纸」的构造器：给定目标档位产出合规样点集）。
 *  目标档按感知亮度口径，样点为 sRGB 值——先做线性→sRGB 逆变换再注入 ±4% 纹理扰动。 */
export function synthWallpaperSamples(targetLuma: number): Array<[number, number, number]> {
  const v = Math.min(1, Math.max(0, targetLuma));
  const srgb = (c: number) => (c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);
  const base = srgb(v);
  const out: Array<[number, number, number]> = [];
  for (let y = 0; y < LUMA_GRID; y++) {
    for (let x = 0; x < LUMA_GRID; x++) {
      // ±4% 微扰模拟真实壁纸纹理（截尾均值后仍回到目标档）
      const jitter = (((x * 7 + y * 13) % 9) - 4) * 0.01;
      const c = Math.min(1, Math.max(0, base + jitter));
      out.push([c, c, c]);
    }
  }
  return out;
}

/* ------------------------------ F501 判定层 ------------------------------ */

export const LUMA_THRESHOLD = 0.5;
/** 滞后带半宽（±0.04）：亮度在带内时维持上次判定——字色不抖动。 */
export const LUMA_HYSTERESIS = 0.04;

export type IconTextColor = "light" | "dark";

/**
 * 带滞后的字色判定。prev 传上一次判定结果（首次传 undefined 走无滞后路径）。
 * 判据「自动选字色」的工程实义：换壁纸瞬间判定一次，而非每帧重判——滞后带
 * 保证壁纸亮度微变（窗口阴影掠过/压暗过渡）不引起文字反色闪烁。
 */
export function pickTextColorHysteresis(effectiveLuma: number, prev?: IconTextColor): IconTextColor {
  if (prev === "light") return effectiveLuma >= LUMA_THRESHOLD + LUMA_HYSTERESIS ? "dark" : "light";
  if (prev === "dark") return effectiveLuma <= LUMA_THRESHOLD - LUMA_HYSTERESIS ? "light" : "dark";
  return effectiveLuma >= LUMA_THRESHOLD ? "dark" : "light";
}

/** F297 压暗折算：压暗 30%（与 deskicons 同源口径），联动一致性由同式保证。 */
export function applyDarkOverlay(wallpaperLuma: number, overlayOn: boolean): number {
  return overlayOn ? wallpaperLuma * 0.7 : wallpaperLuma;
}

/* --------------------------- F501 渲染精度层 --------------------------- */

export interface IconTextRenderSpec {
  colorVar: string;
  shadowOpacity: number;
  shadowBlurCssPx: number;
  outlineCssPx: number;
  /** 4K 走查口径：DPR=2 时的物理像素投影模糊（放大三倍不糊的数字证据）。 */
  shadowBlurDevicePxAt2x: number;
}

/**
 * 4K 精度渲染规格：CSS px 参数 × DPR 得物理 px（判据「4K 渲染精度」的
 * 数字化口径——DPR=2 时 2px CSS 模糊 = 4 物理px，放大三倍检查不糊不锯齿）。
 */
export function iconTextRenderSpec(textColor: IconTextColor, dpr: number): IconTextRenderSpec {
  if (!(dpr >= 1 && dpr <= 4)) {
    throw new Error(`[u3:F501] DPR 越界 ${dpr}——支持 1-4（覆盖 100%-400% 四档走查口径）`);
  }
  const colorVar = textColor === "light" ? "var(--vx-icon-text-light, #ffffff)" : "var(--vx-icon-text-dark, #1b1b1b)";
  return {
    colorVar,
    shadowOpacity: 0.4,
    shadowBlurCssPx: 2,
    outlineCssPx: 1,
    shadowBlurDevicePxAt2x: Math.round(2 * Math.max(2, dpr)),
  };
}

/** 选中态胶囊底衬几何（半透明胶囊：高度=两行文字高+上下内边距，圆角=全高）。 */
export function selectPillGeometry(lineHeightPx: number, lines: number): { heightPx: number; radiusPx: number; padYPx: number } {
  const padY = 4;
  const height = lines * lineHeightPx + padY * 2;
  return { heightPx: height, radiusPx: height / 2, padYPx: padY };
}

/* ------------------------------ F502 语义层 ------------------------------ */

export const LABEL_FULLTEXT_ROUTES = ["tooltip", "rename", "properties"] as const;
export type LabelFulltextRoute = (typeof LABEL_FULLTEXT_ROUTES)[number];

/**
 * 省略全文三路可达的语义闭环（判据：三条路可达）。
 * 输入各路可用性；全禁用=隔离失败级缺陷（显性化，不许静默）。
 * 返回第一条可达路与可达数；零可达抛错——宁可炸在开发期不可哑在用户手边。
 */
export function fulltextRouteVerdict(
  availability: Record<LabelFulltextRoute, boolean>,
): { reachable: Array<LabelFulltextRoute>; primary: LabelFulltextRoute } {
  const reachable = LABEL_FULLTEXT_ROUTES.filter((r) => availability[r]);
  if (reachable.length === 0) {
    throw new Error("[u3:F502] 省略全文三路（Tooltip/重命名/属性）全部不可达——判据红线，禁止交付");
  }
  return { reachable, primary: reachable[0]! };
}

/**
 * Tooltip 联动规则（判据：Tooltip 联动）：仅当标签被截断时 tooltip 才装载
 * 全文；未截断的标签出 tooltip 反而挡路（五章微观手感：不该出现时不挡路）。
 */
export function tooltipShouldShowFulltext(truncated: boolean, tooltipEnabled: boolean): boolean {
  return truncated && tooltipEnabled;
}

/* ------------------------------ 自检 ------------------------------ */

export function lumapickSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 五档亮度构造样点 → 截尾均值回到档位 → 判定与档位一致
  const fiveLumas = [0.05, 0.25, 0.5, 0.75, 0.95];
  const sampledOk = fiveLumas.every((t) => {
    const l = robustWallpaperLuma(synthWallpaperSamples(t));
    return Math.abs(l - t) < 0.02;
  });
  checks.push({ name: "F501 五档样点截尾均值回位", pass: sampledOk });
  // 采样数非法诚实抛错
  let threw = false;
  try { robustWallpaperLuma([[0.5, 0.5, 0.5]]); } catch { threw = true; }
  checks.push({ name: "F501 采样数非法显性报错", pass: threw });
  // 滞后带：亮字维持到阈值+0.04 才翻
  checks.push({ name: "F501 滞后带防抖", pass: pickTextColorHysteresis(0.52, "light") === "light" && pickTextColorHysteresis(0.55, "light") === "dark" && pickTextColorHysteresis(0.47, "dark") === "dark" && pickTextColorHysteresis(0.45, "dark") === "light" });
  // F297 联动：0.55 亮壁纸压暗后翻浅字
  checks.push({ name: "F501 F297 压暗联动", pass: applyDarkOverlay(0.55, true) < LUMA_THRESHOLD && applyDarkOverlay(0.55, false) >= LUMA_THRESHOLD });
  // 4K 精度：DPR=2 物理 px 翻倍
  checks.push({ name: "F501 4K DPR 物理像素翻倍", pass: iconTextRenderSpec("light", 2).shadowBlurDevicePxAt2x === 4 });
  // DPR 越界抛错
  let dprThrew = false;
  try { iconTextRenderSpec("light", 0.5); } catch { dprThrew = true; }
  checks.push({ name: "F501 DPR 越界显性报错", pass: dprThrew });
  // 胶囊几何：两行 20px 行高 → 高 48、圆角 24
  const pill = selectPillGeometry(20, 2);
  checks.push({ name: "F501 选中胶囊几何", pass: pill.heightPx === 48 && pill.radiusPx === 24 });
  // F502 三路闭环：全禁用抛错；部分可用给主路
  let routeThrew = false;
  try { fulltextRouteVerdict({ tooltip: false, rename: false, properties: false }); } catch { routeThrew = true; }
  const partly = fulltextRouteVerdict({ tooltip: false, rename: true, properties: false });
  checks.push({ name: "F502 三路闭环显性化", pass: routeThrew && partly.primary === "rename" });
  // Tooltip 联动：未截断不出
  checks.push({ name: "F502 Tooltip 截断联动", pass: !tooltipShouldShowFulltext(false, true) && tooltipShouldShowFulltext(true, true) && !tooltipShouldShowFulltext(true, false) });
  return checks;
}
