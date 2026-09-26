/**
 * F151 令牌模式深化 · JSON Schema 生成 + CSS 变量双向桥 + 无障碍缩放变体。
 *
 * 主册判据延伸：
 * - F151「JSON schema 校验双向通过」——tokens.ts 的 validateTokenTable 是
 *   系统内侧校验；本模块生成**正式 JSON Schema 文档**（第三方主题作者
 *   用任意 JSON Schema 工具即可预检自己的主题包——开放性十四章）；
 * - F151「24 色令牌全系统覆盖」的机制面：令牌 → CSS 变量导出是全系统
 *   消费的唯一通道（组件只认 --p-* 变量，硬编码扫描器 B-1104 守门）；
 * - 无障碍联动（二十维度 14）：字号四档整体缩放变体（系统级大字号
 *   ——不逐槽手调，缩放系数保证层级比例不破）。
 */

import {
  COLOR_TOKENS,
  FONT_DEFAULTS,
  FONT_KEYS,
  MOTION_CURVES,
  MOTION_DURATIONS,
  RADIUS_KEYS,
  SPACING_TOKENS,
  TOKEN_TABLE_VERSION,
  defaultTokenTable,
  type FontTable,
  type MotionCurve,
  type MotionDuration,
  type RadiusTable,
  type TokenTable,
} from "./tokens";

// ---------- JSON Schema 生成（Draft-07——第三方工具链兼容） ----------

export interface SchemaMeta {
  /** 生成时间（ISO）。 */
  generatedAt: string;
  /** 面向的令牌表版本。 */
  tableVersion: number;
}

/**
 * 生成 TokenTable 的 JSON Schema 文档：
 * - 颜色键从 COLOR_TOKENS 逐键展开（枚举 + hex 模式约束）；
 * - 圆角/字号带数值范围（<0 与超大值在 schema 层就拒——错误前置）；
 * - 动效曲线/时长用 enum（Magic 值零容忍——schema 即宪法）。
 */
export function generateJsonSchema(meta?: Partial<SchemaMeta>): { schema: Record<string, unknown>; meta: SchemaMeta } {
  const m: SchemaMeta = { generatedAt: new Date().toISOString(), tableVersion: TOKEN_TABLE_VERSION, ...meta };
  const colorProps: Record<string, unknown> = {};
  for (const c of COLOR_TOKENS) {
    colorProps[c.key] = {
      type: "string",
      pattern: "^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$",
      description: `${c.zh} / ${c.en}${c.translucent ? "（可半透明 #rrggbbaa）" : ""}`,
    };
  }
  const radiusProps: Record<string, unknown> = {};
  for (const r of RADIUS_KEYS) {
    radiusProps[r.slot] = { type: "integer", minimum: 0, maximum: 48, description: `${r.key}px 档` };
  }
  const fontProps: Record<string, unknown> = {};
  for (const f of FONT_KEYS) {
    fontProps[f.slot] = { type: "integer", minimum: 9, maximum: 96, description: `${f.key}px 档` };
  }
  const motionProps: Record<string, unknown> = {};
  for (const curve of Object.keys(MOTION_CURVES) as MotionCurve[]) {
    motionProps[curve] = {
      type: "object",
      properties: {
        curve: { enum: Object.keys(MOTION_CURVES) },
        duration: { enum: Object.keys(MOTION_DURATIONS) },
      },
      additionalProperties: false,
      required: ["curve", "duration"],
    };
  }
  const schema = {
    $schema: "http://json-schema.org/draft-07/schema#",
    $id: "vx:persona/token-table",
    title: "Varix Persona Token Table",
    description: `个性化域令牌表 v${m.tableVersion}——24 语义色 + 圆角三档 + 字号四档 + 动效五曲线三时长。第三方主题作者只需改 24 个颜色即可产出完整主题（F151 开放性承诺）。`,
    type: "object",
    additionalProperties: false,
    required: ["colors", "radius", "font", "motion", "version"],
    properties: {
      colors: { type: "object", propertyNames: { enum: COLOR_TOKENS.map((c) => c.key) }, properties: colorProps, additionalProperties: false },
      radius: { type: "object", properties: radiusProps, additionalProperties: false, required: RADIUS_KEYS.map((r) => r.slot) },
      font: { type: "object", properties: fontProps, additionalProperties: false, required: FONT_KEYS.map((f) => f.slot) },
      motion: { type: "object", properties: motionProps, additionalProperties: false, required: Object.keys(MOTION_CURVES) },
      version: { type: "integer", const: m.tableVersion },
    },
  };
  return { schema, meta: m };
}

// ---------- CSS 变量双向桥（全系统消费的唯一通道） ----------

/** 令牌表 → --p-* CSS 变量表（组件消费面——一处一事实的出口；键名对齐 tokens.css 既有约定）。 */
export function toCssVariables(table: TokenTable): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(table.colors)) out[k] = v;
  for (const r of RADIUS_KEYS) out[r.key] = `${table.radius[r.slot]}px`; // key 即全名（--p-r-*）。
  for (const f of FONT_KEYS) out[f.key] = `${table.font[f.slot]}px`; // key 即全名（--p-fs-*）。
  for (const curve of Object.keys(table.motion) as MotionCurve[]) {
    out[`--p-ease-${curve}`] = MOTION_CURVES[curve].bezier; // 曲线（宪法值）。
  }
  for (const d of Object.keys(MOTION_DURATIONS) as MotionDuration[]) {
    out[`--p-motion-${d}`] = `${MOTION_DURATIONS[d].ms}ms`; // 时长按档位命名（ui 层 --p-motion-micro 惯例）。
  }
  // 间距档（乙-1 六档——只读档不随表变，但导出完整变量集供预览环境）。
  for (const s of SPACING_TOKENS) out[s.key] = `${s.px}px`; // key 即全名（--p-sp-*）。
  return out;
}

const HEX_RE = /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/;
const RADIUS_VAR = /^--p-r-(control|card|window)$/;
const FONT_VAR = /^--p-fs-(caption|body|title|display)$/;

/**
 * CSS 变量表 → 令牌表补丁（反向桥——从运行时环境回收用户改动）：
 * 只收已知键、非法值逐项拒绝并列出（粘贴清洗同源纪律）。
 */
export function fromCssVariables(css: Record<string, string>): { patch: Partial<TokenTable>; rejected: Array<{ key: string; value: string; reason: string }> } {
  const patch: Partial<TokenTable> = {};
  const rejected: Array<{ key: string; value: string; reason: string }> = [];
  const colors: Record<string, string> = {};
  let hasColor = false;
  const radius: Partial<RadiusTable> = {};
  let hasRadius = false;
  const font: Partial<FontTable> = {};
  let hasFont = false;
  for (const [k, v] of Object.entries(css)) {
    // 只读派生变量：ease 曲线（宪法值不随表改）、时长档（由曲线表派生）与
    // 间距档（乙-1 固定六档）——导出集包含它们供预览环境完整，回收时跳过
    // （不是拒绝——它们不是用户可改面）。
    if (k.startsWith("--p-ease-") || k.startsWith("--p-space-") || k.startsWith("--p-motion-") || k.startsWith("--p-sp-")) continue;
    if (COLOR_TOKENS.some((c) => c.key === k)) {
      if (!HEX_RE.test(v)) {
        rejected.push({ key: k, value: v, reason: "非法 hex（需 #rrggbb 或 #rrggbbaa）" });
        continue;
      }
      colors[k] = v;
      hasColor = true;
      continue;
    }
    const rm = RADIUS_VAR.exec(k);
    if (rm) {
      const n = Number(v.replace(/px$/, ""));
      if (!Number.isInteger(n) || n < 0 || n > 48) {
        rejected.push({ key: k, value: v, reason: "圆角需 0-48 整数 px" });
        continue;
      }
      radius[rm[1] as keyof RadiusTable] = n;
      hasRadius = true;
      continue;
    }
    const fm = FONT_VAR.exec(k);
    if (fm) {
      const n = Number(v.replace(/px$/, ""));
      if (!Number.isInteger(n) || n < 9 || n > 96) {
        rejected.push({ key: k, value: v, reason: "字号需 9-96 整数 px" });
        continue;
      }
      font[fm[1] as keyof FontTable] = n;
      hasFont = true;
      continue;
    }
    // 未知 --p-* 键：显性拒绝（不静默丢——导出面板逐条展示）。
    if (k.startsWith("--p-")) rejected.push({ key: k, value: v, reason: "未知 --p-* 变量（不在令牌表）" });
  }
  if (hasColor) patch.colors = colors;
  if (hasRadius) patch.radius = radius as RadiusTable;
  if (hasFont) patch.font = font as FontTable;
  return { patch, rejected };
}

/** 补丁应用（default 为底——补丁只覆盖出现的段，段内逐键覆盖）。 */
export function applyPatch(base: TokenTable, patch: Partial<TokenTable>): TokenTable {
  const out: TokenTable = JSON.parse(JSON.stringify(base));
  if (patch.colors) out.colors = { ...out.colors, ...patch.colors };
  if (patch.radius) out.radius = { ...out.radius, ...patch.radius };
  if (patch.font) out.font = { ...out.font, ...patch.font };
  if (patch.motion) out.motion = { ...out.motion, ...patch.motion };
  out.version = TOKEN_TABLE_VERSION;
  return out;
}

// ---------- 无障碍缩放变体（系统级大字号——层级比例不破） ----------

export interface FontScaleResult {
  table: TokenTable;
  /** 各档缩放后值（面板预览——放大前先看数字）。 */
  scaled: FontTable;
  /** 可读性校验：caption 缩放后仍 ≥9px（低于下限钳制并报告）。 */
  clamped: string[];
}

/**
 * 字号整体缩放（1.0/1.25/1.5——用户系统字体缩放 fourteen 章「窗口与环境」）：
 * 等比缩放保持四档层级比例；caption 低于 9px 钳制（可读性底线优先于等比）。
 */
export function scaleFontTable(table: TokenTable, factor: number): FontScaleResult {
  if (!Number.isFinite(factor) || factor <= 0) throw new Error("缩放系数必须为正有限数");
  const clamped: string[] = [];
  const scaled = { ...FONT_DEFAULTS };
  const out: TokenTable = JSON.parse(JSON.stringify(table));
  for (const f of FONT_KEYS) {
    const target = Math.round(table.font[f.slot] * factor);
    if (target < 9) {
      scaled[f.slot] = 9;
      clamped.push(f.slot);
    } else if (target > 96) {
      scaled[f.slot] = 96;
      clamped.push(f.slot);
    } else {
      scaled[f.slot] = target;
    }
    out.font[f.slot] = scaled[f.slot];
  }
  return { table: out, scaled, clamped };
}

// ---------- 表完整性自证（F170 联动——变量集数量契约） ----------

/** 导出变量数契约：24 色 + 3 圆角 + 4 字号 + 5 曲线 + 3 时长档 + 6 间距 = 45。 */
export const CSS_VAR_EXPECTED_COUNT = 45;

export function verifyVariableContract(table: TokenTable): { ok: boolean; actual: number; expected: number; missing: string[] } {
  const vars = toCssVariables(table);
  const expected = defaultTokenTable();
  const missing: string[] = [];
  for (const k of Object.keys(toCssVariables(expected))) {
    if (!(k in vars)) missing.push(k);
  }
  return { ok: missing.length === 0 && Object.keys(vars).length === CSS_VAR_EXPECTED_COUNT, actual: Object.keys(vars).length, expected: CSS_VAR_EXPECTED_COUNT, missing };
}
