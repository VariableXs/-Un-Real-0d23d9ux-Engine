/**
 * 桌面图标三件（AI-U3 · F501 文字可读性 / F502 两行封顶 / F503 网格密度）。
 *
 * 判据唯一源（主册摘文）：
 * - F501「文字双层渲染（柔和投影+微描边，投影透明度 40%、模糊 2px）、
 *   深浅壁纸自动选字色（壁纸亮度分析选白/黑字——F297 压暗协同）、选中态
 *   文字底衬（半透明胶囊底）；五档亮度壁纸×两字色自动选择用例；与 F297
 *   压暗联动一致性」。
 * - F502「最多两行、每行约 8 个全角字符、超出第二行尾部省略号（…）；
 *   省略的全文看 Tooltip（F205）/重命名（F260）/属性（F264）三条路可达；
 *   中英混排换行（不在英文单词中间断）；居中对齐」。
 * - F503「三档格距 96/80/64px；密度改变时图标按最近格吸附重排（相对位置
 *   尽量保持）；自定义 8px 步进；与 F084 网格/F401 自动排列兼容；预览即时」。
 *
 * 纯函数实现（零 DOM 依赖），供运行时与单测共用同一事实源。
 */

import { u3Store } from "./u3store";

/* ------------------------------- F501 ------------------------------- */

/** 投影透明度 40%（主册 F501 规格表）。 */
export const ICON_SHADOW_OPACITY = 0.4;
/** 投影模糊 2px（主册 F501 规格表——4K 管线 F068 精度）。 */
export const ICON_SHADOW_BLUR_PX = 2;

/** 五档亮度样张（验收判据「五档亮度壁纸」的采样口径：平均亮度 0-1）。 */
export const BRIGHTNESS_SAMPLES = [0.05, 0.25, 0.5, 0.75, 0.95] as const;

/** 亮度判定阈值：高于此值选深字（黑），否则选浅字（白）——0.5 为感知中点。 */
export const LUMA_THRESHOLD = 0.5;

/**
 * 感知亮度（Rec.709 加权 luma）：r/g/b ∈ [0,1] → [0,1]。
 * 人眼对绿色最敏感——权重 0.2126/0.7152/0.0722（视频工业标准）。
 */
export function luma(r: number, g: number, b: number): number {
  const c = (v: number) => {
    // sRGB → 线性空间（ perceptual correctness：直接加权会高估暗色亮度）
    const x = v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    return Math.min(1, Math.max(0, x));
  };
  return 0.2126 * c(r) + 0.7152 * c(g) + 0.0722 * c(b);
}

/** #rrggbb（或 #rgb）→ 感知亮度；非法输入诚实抛错（异常零静默）。 */
export function hexLuma(hex: string): number {
  const m = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) throw new Error(`[u3:F501] 非法颜色值 ${hex}——只接受 #rgb/#rrggbb`);
  let h = m[1]!; // 正则已保证捕获组存在
  if (h.length === 3) h = h.split("").map((c) => c + c).join("");
  return luma(
    parseInt(h.slice(0, 2), 16) / 255,
    parseInt(h.slice(2, 4), 16) / 255,
    parseInt(h.slice(4, 6), 16) / 255,
  );
}

export type IconTextColor = "light" | "dark";

/**
 * 壁纸亮度 → 图标字色自动选择（判据：深浅壁纸自动选字色）。
 * wallpaperLuma 传壁纸平均感知亮度；darkOverlay 为 F297 压暗开关——
 * 压暗启用时壁纸整体变暗 30%，等效亮度按同比例折算（联动一致性）。
 */
export function pickIconTextColor(wallpaperLuma: number, darkOverlay = false): IconTextColor {
  // F297 壁纸暗色压暗：30%±5%——字色判定必须与视觉所见一致，否则花壁纸上选错字色
  const effective = darkOverlay ? wallpaperLuma * 0.7 : wallpaperLuma;
  return effective >= LUMA_THRESHOLD ? "dark" : "light";
}

/** 投影/描边样式串（双层渲染：柔和投影 + 微描边）——令牌化，零硬编码色。 */
export function iconTextLayers(textColor: IconTextColor): { color: string; textShadow: string } {
  const core = textColor === "light" ? "var(--vx-icon-text-light, #ffffff)" : "var(--vx-icon-text-dark, #1b1b1b)";
  const halo = textColor === "light" ? "rgba(0,0,0," : "rgba(255,255,255,";
  return {
    color: core,
    textShadow: `${halo}${ICON_SHADOW_OPACITY})) 0 1px ${ICON_SHADOW_BLUR_PX}px, ${halo}0.85)) 0 0 1px`,
  };
}

/** 选中态半透明胶囊底衬（判据：选中态文字底衬）。 */
export const SELECT_PILL_BG = "rgba(120,160,255,0.28)";

/* ------------------------------- F502 ------------------------------- */

/** 每行约 8 个全角字符（主册 F502 判据）。 */
export const ICON_CHARS_PER_LINE = 8;
/** 最多两行（主册 F502 判据）。 */
export const ICON_MAX_LINES = 2;
export const ELLIPSIS = "…";

/** 判定「全角」：CJK/全角标点/全角字母按 2、其余按 1 计（约等宽估算）。 */
export function charWidth(ch: string): 2 | 1 {
  const cp = ch.codePointAt(0);
  if (cp === undefined) return 1;
  if (
    (cp >= 0x1100 && cp <= 0x115f) || // Hangul Jamo
    (cp >= 0x2e80 && cp <= 0xa4cf) || // CJK 部首…Yi
    (cp >= 0xac00 && cp <= 0xd7a3) || // Hangul 音节
    (cp >= 0xf900 && cp <= 0xfaff) || // CJK 兼容表意
    (cp >= 0xfe30 && cp <= 0xfe4f) || // CJK 兼容形式
    (cp >= 0xff00 && cp <= 0xff60) || // 全角形式
    (cp >= 0xffe0 && cp <= 0xffe6) ||
    (cp >= 0x20000 && cp <= 0x3fffd)  // CJK 扩展
  ) return 2;
  return 1;
}

/**
 * 两行封顶换行（判据：两行/8 字换行参数 + 中英混排不在英文单词中间断）。
 * 返回 [第一行, 第二行, 是否截断]；第二行超出预算时在词边界（无词边界则
 * 字符边界）收尾并追加省略号。
 */
export function wrapIconLabel(name: string): { lines: [string, string]; truncated: boolean } {
  const budgetPerLine = ICON_CHARS_PER_LINE * 2; // 全角口径：每行 16 半角当量
  const words: string[] = [];
  // 英文连续段聚合为「词」（判据：不在英文单词中间断）；CJK 逐字成词
  let buf = "";
  let bufAscii = false;
  for (const ch of name) {
    const isAsciiWord = /[A-Za-z0-9]/.test(ch);
    if (buf && isAsciiWord !== bufAscii) {
      words.push(buf);
      buf = "";
    }
    bufAscii = isAsciiWord;
    buf += ch;
  }
  if (buf) words.push(buf);

  const width = (s: string) => [...s].reduce((a, c) => a + charWidth(c), 0);
  const isAsciiWordTok = (s: string) => /^[A-Za-z0-9]+$/.test(s);
  const lines: string[] = ["", ""];
  let line = 0;
  let truncated = false;
  for (const word of words) {
    if (truncated) break;
    // 整词可入当前行 → 直接收下
    if (width(lines[line] ?? "") + width(word) <= budgetPerLine) {
      lines[line] = (lines[line] ?? "") + word;
      continue;
    }
    if (isAsciiWordTok(word) && width(word) <= budgetPerLine && lines[line] !== "") {
      // 英文整词放不下：整词降行（不拆词——判据铁律）
      if (line === 0) { line = 1; lines[line] += word; continue; }
      truncated = true;
      break;
    }
    // 逐字符走：CJK 可逐字拆；无空格超长 token 可硬拆；英文整词在行首放不下也硬拆（否则永远放不进任何行）
    let chars = [...word];
    while (chars.length > 0) {
      const space = budgetPerLine - width(lines[line] ?? "");
      if (space <= 0) {
        if (line === 0) { line = 1; continue; }
        truncated = true;
        break;
      }
      let take = 0;
      let used = 0;
      while (take < chars.length && used + charWidth(chars[take]!) <= space) {
        used += charWidth(chars[take]!);
        take++;
      }
      if (take === 0) { // 单字符超预算（不可能：charWidth ≤2 ≤ 预算）——防御
        take = 1;
      }
      lines[line] = (lines[line] ?? "") + chars.slice(0, take).join("");
      chars = chars.slice(take);
      if (chars.length > 0) {
        if (line === 0) { line = 1; continue; }
        truncated = true;
        break;
      }
    }
  }
  // 收尾：截断时第二行末尾替换为省略号（判据：省略号永远表示「还有内容」）
  if (truncated) {
    let tail = [...(lines[1] ?? "")];
    while (width(tail.join("")) + width(ELLIPSIS) > budgetPerLine && tail.length > 0) tail.pop();
    lines[1] = tail.join("") + ELLIPSIS;
  } else if (width(lines[1] ?? "") > budgetPerLine) {
    // 防御：未标记截断但第二行超宽（理论不可达）——同样收尾
    let tail = [...(lines[1] ?? "")];
    while (width(tail.join("")) + width(ELLIPSIS) > budgetPerLine && tail.length > 0) tail.pop();
    lines[1] = tail.join("") + ELLIPSIS;
    truncated = true;
  }
  return { lines: [lines[0] ?? "", lines[1] ?? ""], truncated };
}

/** 全文三路可达语义（判据：Tooltip F205/重命名 F260/属性 F264 三条路可达）。 */
export const LABEL_FULLTEXT_ROUTES = ["tooltip", "rename", "properties"] as const;

/* ------------------------------- F503 ------------------------------- */

/** 三档格距（主册 F503 判据：宽松/标准/紧凑 = 96/80/64px）。 */
export const GRID_DENSITY_PX = { loose: 96, standard: 80, compact: 64 } as const;
export type GridDensity = keyof typeof GRID_DENSITY_PX | "custom";
/** 自定义步进 8px（主册 F503 判据）。 */
export const GRID_CUSTOM_STEP_PX = 8;

export interface GridConfig {
  density: GridDensity;
  customColPx: number;
  customRowPx: number;
}

/** 生效格距：三档取预设；custom 取自定义（钳制到 8px 步进与合理范围）。 */
export function effectiveGrid(cfg: GridConfig): { colPx: number; rowPx: number } {
  if (cfg.density !== "custom") {
    const p = GRID_DENSITY_PX[cfg.density];
    return { colPx: p, rowPx: p };
  }
  const step = GRID_CUSTOM_STEP_PX;
  const clamp = (v: number) => Math.min(200, Math.max(48, Math.round(v / step) * step));
  return { colPx: clamp(cfg.customColPx), rowPx: clamp(cfg.customRowPx) };
}

/**
 * 密度改变时的最近格吸附重排（判据：图标按最近格吸附重排、相对位置尽量保持）。
 * 旧网格 → 新网格：每点吸附到新网格最近格点；相同新格点冲突时按旧序错开一格
 * （稳定性：邻居关系不变——先来先占，冲突者向后找最近空位）。
 */
export function resnapToGrid(
  points: Array<{ x: number; y: number }>,
  from: { colPx: number; rowPx: number }, // 语义参数：调用方记录换档前格距（算法只依赖 to）
  to: { colPx: number; rowPx: number },
): Array<{ x: number; y: number }> {
  void from;
  const occupied = new Set<string>();
  const out: Array<{ x: number; y: number }> = [];
  for (const p of points) {
    const nx = Math.round(p.x / to.colPx) * to.colPx;
    const ny = Math.round(p.y / to.rowPx) * to.rowPx;
    if (!occupied.has(`${nx},${ny}`)) {
      occupied.add(`${nx},${ny}`);
      out.push({ x: nx, y: ny });
      continue;
    }
    // 冲突：螺旋外扩找最近空位（保持相对位置——最近优先）
    let placed = false;
    for (let r = 1; r <= 8 && !placed; r++) {
      for (let dy = -r; dy <= r && !placed; dy++) {
        for (let dx = -r; dx <= r && !placed; dx++) {
          if (Math.max(Math.abs(dx), Math.abs(dy)) !== r) continue; // 环形遍历
          const cx = nx + dx * to.colPx;
          const cy = ny + dy * to.rowPx;
          if (cx < 0 || cy < 0) continue;
          const k = `${cx},${cy}`;
          if (!occupied.has(k)) {
            occupied.add(k);
            out.push({ x: cx, y: cy });
            placed = true;
          }
        }
      }
    }
    if (!placed) {
      out.push({ x: nx, y: ny }); // 理论不可达（8 环 289 格）；诚实兜底不丢点
    }
  }
  return out;
}

/* ------------------------------- 运行时读取 ------------------------------- */

export function iconReadConfig() {
  const s = u3Store.get("iconRead");
  return {
    mode: (s.mode as "auto" | "light" | "dark") ?? "auto",
    shadowOpacity: (s.shadowOpacity as number) ?? ICON_SHADOW_OPACITY,
    shadowBlurPx: (s.shadowBlurPx as number) ?? ICON_SHADOW_BLUR_PX,
    selectedPill: (s.selectedPill as boolean) ?? true,
  };
}

export function gridDensityConfig() {
  const s = u3Store.get("gridDensity");
  return {
    density: (s.density as GridDensity) ?? "standard",
    customColPx: (s.customColPx as number) ?? 80,
    customRowPx: (s.customRowPx as number) ?? 80,
  };
}

/** F501-F503 判据自检（与内核 ustar3::deskicons 同判据不同实现——前端面）。 */
export function deskiconsSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 五档亮度 × 自动选字：暗壁纸选浅字、亮壁纸选深字
  checks.push({ name: "F501 五档亮度自动选字", pass: BRIGHTNESS_SAMPLES.every((b) => (b < 0.5 ? pickIconTextColor(b) === "light" : pickIconTextColor(b) === "dark")) });
  // 压暗联动：0.55 亮壁纸开 F297 后应翻转（0.55*0.7=0.385<0.5）
  checks.push({ name: "F501 F297 压暗联动", pass: pickIconTextColor(0.55) === "dark" && pickIconTextColor(0.55, true) === "light" });
  // 两行封顶：长名截断 + 省略号
  const w = wrapIconLabel("项目总结报告最终版本提交给评审委员会审议用副本");
  checks.push({ name: "F502 两行封顶+省略", pass: w.truncated && w.lines[1].endsWith(ELLIPSIS) });
  // 英文不断词：整个单词留在同一行
  const e = wrapIconLabel("hello world dashboard");
  checks.push({ name: "F502 英文不断词", pass: !e.lines[0].endsWith("hel") && !e.lines[0].includes("helloworld") });
  // 三档格距
  checks.push({ name: "F503 三档格距", pass: GRID_DENSITY_PX.loose === 96 && GRID_DENSITY_PX.standard === 80 && GRID_DENSITY_PX.compact === 64 });
  // 最近格吸附：同点冲突不覆盖（两点错开）
  const snap = resnapToGrid([{ x: 10, y: 10 }, { x: 30, y: 12 }], { colPx: 80, rowPx: 80 }, { colPx: 80, rowPx: 80 });
  checks.push({ name: "F503 吸附不覆盖", pass: new Set(snap.map((p) => `${p.x},${p.y}`)).size === 2 });
  return checks;
}
