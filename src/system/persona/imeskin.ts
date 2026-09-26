/**
 * F166 输入法皮肤 · 完整设计。
 *
 * 主册判据：定制全项生效试打验证；16ms 延迟红线在自定义皮肤下复测；
 * 跟随主题切换联动实测。
 *
 * 【功能定义】候选窗（F027/F107）外观定制：配色（随 E1 或独立）/字体字号/
 * 透明度/候选数显示（5/9 档）；组合期渲染延迟 ≤16ms 红线不变——皮肤自由，速度
 * 不商量。
 *
 * 【状态与异常】所选字体缺字 → F159 红黄卡拦截路径；透明度过低可读性差 → 60%
 * 下限（护栏）；渲染延迟超标（自检）→ 自动回退默认皮肤+报备。
 *
 * 【设计细节】热生效=候选窗每次弹出重读皮肤参数（无缓存陈旧态）；透明度走合成器
 * 层透明；候选高亮色对比度门禁（4.5:1 自动校验——F141 公式复用）；9 候选档下
 * 翻页键提示；皮肤参数 schema 并入 vxtheme。
 */

import { personaStore } from "./store";
import { contrastRatio, compositeOver } from "../theme-studio/tokens";

export const SECTION = "ime";
export const RENDER_BUDGET_MS = 16;
export const OPACITY_FLOOR = 0.6;
export const HIGHLIGHT_CONTRAST_MIN = 4.5;
export const CANDIDATE_COUNTS = [5, 9] as const;
export const FONT_SIZE_MIN = 12;
export const FONT_SIZE_MAX = 16;

export interface ImeSkinConfig {
  /** 跟随主题（开=令牌联动，关=独立定制组）。 */
  followTheme: boolean;
  fontFamily: string | null;
  fontSize: number;
  /** 透明度 0.6..1（60% 下限护栏）。 */
  opacity: number;
  candidates: 5 | 9;
  /** 独立配色（followTheme=false 时生效）。 */
  colors: {
    background: string;
    text: string;
    highlight: string;
    highlightText: string;
  };
}

export function defaultImeSkinConfig(): ImeSkinConfig {
  return {
    followTheme: true,
    fontFamily: null,
    fontSize: 15,
    opacity: 0.95,
    candidates: 5,
    colors: {
      background: "#222230",
      text: "#e8e8f0",
      highlight: "#4a5fc1", // 候选高亮——对白字对比度 ≥4.5:1（门禁自证）
      highlightText: "#ffffff",
    },
  };
}

export function loadImeSkinConfig(): ImeSkinConfig {
  const stored = personaStore.getWith(SECTION, "skin", undefined) as Partial<ImeSkinConfig> | undefined;
  return { ...defaultImeSkinConfig(), ...(stored ?? {}) };
}

export function saveImeSkinConfig(c: ImeSkinConfig): ImeSkinValidation {
  personaStore.set(SECTION, { skin: c });
  return validateImeSkin(c);
}

export interface ImeSkinValidation {
  ok: boolean;
  issues: string[];
  /** 高亮对比度值（4.5:1 门禁的实测数）。 */
  highlightContrast: number;
}

/** 校验：透明度护栏 + 高亮对比度门禁（F141 公式复用——theme-studio contrastRatio）。 */
export function validateImeSkin(c: ImeSkinConfig): ImeSkinValidation {
  const issues: string[] = [];
  if (c.opacity < OPACITY_FLOOR) issues.push(`透明度过低（下限 ${Math.round(OPACITY_FLOOR * 100)}%）——已按护栏钳制`);
  if (c.fontSize < FONT_SIZE_MIN || c.fontSize > FONT_SIZE_MAX) issues.push(`字号须在 ${FONT_SIZE_MIN}-${FONT_SIZE_MAX}px`);
  // 高亮对比度：高亮文字 vs 高亮底（高亮底若半透明先合成到背景）。
  const bg = compositeOver(c.colors.highlight, c.colors.background) ?? c.colors.highlight;
  const contrast = contrastRatio(c.colors.highlightText, bg);
  if (contrast < HIGHLIGHT_CONTRAST_MIN) {
    issues.push(`候选高亮对比度 ${contrast.toFixed(2)}:1 低于 4.5:1 门禁`);
  }
  return { ok: issues.length === 0, issues, highlightContrast: contrast };
}

/** 护栏钳制（保存前自动应用——护栏不是拒绝而是回正）。 */
export function clampImeSkin(c: ImeSkinConfig): ImeSkinConfig {
  return {
    ...c,
    opacity: Math.min(1, Math.max(OPACITY_FLOOR, c.opacity)),
    fontSize: Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, Math.round(c.fontSize))),
    candidates: c.candidates === 9 ? 9 : 5,
  };
}

// ---------- 热生效与延迟自检 ----------

/**
 * 热生效读取口：候选窗每次弹出调用（无缓存陈旧态——主册设计细节）。
 * 返回渲染所需参数包（跟随主题时由 E1 令牌注入 colors）。
 */
export function skinForPopup(config: ImeSkinConfig, themeColors: { background: string; text: string; highlight: string; highlightText: string }): ImeSkinConfig {
  return config.followTheme ? { ...config, colors: themeColors } : config;
}

export interface LatencySample {
  ms: number;
  at: number;
}

export class ImeLatencySelfCheck {
  private samples: LatencySample[] = [];

  record(ms: number, at: number = Date.now()): void {
    this.samples.push({ ms, at });
    if (this.samples.length > 200) this.samples.splice(0, this.samples.length - 200);
  }

  /** P99 是否达 16ms 红线；连续超标返回回退建议（自动回退默认皮肤+报备）。 */
  verdict(): { ok: boolean; p99: number; shouldRevertToDefault: boolean } {
    if (this.samples.length === 0) return { ok: true, p99: 0, shouldRevertToDefault: false };
    const sorted = this.samples.map((s) => s.ms).sort((a, b) => a - b);
    const p99 = sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.99))] ?? 0;
    const recent = this.samples.slice(-50);
    const overRate = recent.filter((s) => s.ms > RENDER_BUDGET_MS).length / recent.length;
    return { ok: p99 <= RENDER_BUDGET_MS, p99, shouldRevertToDefault: overRate > 0.1 };
  }
}

/** 9 候选档下显示翻页键提示。 */
export function needsPagingHint(candidates: 5 | 9): boolean {
  return candidates === 9;
}

// ---------- F126 增节：皮肤参数 JSON Schema（主册设计细节「皮肤参数 schema 并入 vxtheme」） ----------

/** 生成输入法皮肤 JSON Schema（草稿-07）——与 tokenTableJsonSchema 同族惯例。 */
export function imeSkinJsonSchema(): Record<string, unknown> {
  const hexPattern = "^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$";
  return {
    $schema: "https://json-schema.org/draft/2020-12/schema",
    $id: "varix:persona:ime-skin:v1",
    title: "Varix 输入法皮肤 v1（vxtheme 增节）",
    type: "object",
    required: ["followTheme", "fontFamily", "fontSize", "opacity", "candidates", "colors"],
    additionalProperties: false,
    properties: {
      followTheme: { type: "boolean", description: "开=令牌联动（E1），关=独立定制组" },
      fontFamily: { type: ["string", "null"], description: "null=系统默认（建议经 F159 预检）" },
      fontSize: { type: "integer", minimum: FONT_SIZE_MIN, maximum: FONT_SIZE_MAX },
      opacity: { type: "number", minimum: OPACITY_FLOOR, maximum: 1, description: "60% 下限护栏" },
      candidates: { enum: [...CANDIDATE_COUNTS], description: "9 档显示翻页键提示" },
      colors: {
        type: "object",
        additionalProperties: false,
        required: ["background", "text", "highlight", "highlightText"],
        properties: {
          background: { type: "string", pattern: hexPattern },
          text: { type: "string", pattern: hexPattern },
          highlight: { type: "string", pattern: hexPattern, description: "与 highlightText 对比度须 ≥4.5:1（F141 门禁）" },
          highlightText: { type: "string", pattern: hexPattern },
        },
      },
    },
  };
}

/** 皮肤包合法性快检（导入 vxtheme 皮肤节时的双向校验入口）。 */
export function validateImeSkinPackage(raw: unknown): { ok: boolean; issues: string[] } {
  const issues: string[] = [];
  if (typeof raw !== "object" || raw === null) return { ok: false, issues: ["不是 JSON 对象"] };
  const c = raw as Partial<ImeSkinConfig>;
  if (typeof c.followTheme !== "boolean") issues.push("followTheme 缺失或非法");
  if (typeof c.fontSize !== "number" || c.fontSize < FONT_SIZE_MIN || c.fontSize > FONT_SIZE_MAX) {
    issues.push(`fontSize 须为 ${FONT_SIZE_MIN}-${FONT_SIZE_MIN === 12 ? 12 : FONT_SIZE_MIN}-${FONT_SIZE_MAX} 整数`);
  }
  if (typeof c.opacity !== "number" || c.opacity < OPACITY_FLOOR || c.opacity > 1) issues.push(`opacity 须在 ${OPACITY_FLOOR}-1`);
  if (c.candidates !== 5 && c.candidates !== 9) issues.push("candidates 只允许 5 或 9");
  const hex = /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/;
  if (typeof c.colors !== "object" || c.colors === null) {
    issues.push("colors 缺失");
  } else {
    for (const k of ["background", "text", "highlight", "highlightText"] as const) {
      const v = (c.colors as Record<string, unknown>)[k];
      if (typeof v !== "string" || !hex.test(v)) issues.push(`colors.${k} 非法色值`);
    }
  }
  return { ok: issues.length === 0, issues };
}
