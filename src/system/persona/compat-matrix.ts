/**
 * F151/F161 令牌兼容矩阵 · 旧主题迁移引擎。
 *
 * 主册判据延伸：
 * - F151【设计细节】「令牌表带版本戳（旧主题兼容矩阵文档化）」「令牌缺失（旧版
 *   主题）→ 默认值兜底+覆盖度标注」。
 * - F161【状态与异常】「跨版本导入 → 兼容矩阵校验+降级项清单」。
 * - 三铁律「随时可退」：迁移必须可逆——v0 原始输入随迁移结果一并返回（回滚不丢
 *   作者原始数据）。
 *
 * v0 = theme-studio 时代的 11 变量旧主题包（--bg-canvas 族）；v1 = 本域 24 色
 * 语义令牌表。桥接关系与 theme-engine.deriveTableFromVthemeColors 同源
 * （一处一事实：桥表定义在此，theme-engine 消费）。
 */

import { TOKEN_TABLE_VERSION, defaultTokenTable, type TokenTable } from "./tokens";

export const COMPAT_MATRIX_FORMAT = "varix:persona:token-compat:v1";

/** 兼容级别（矩阵校验的三态输出）。 */
export type CompatLevel =
  | "native"        // 当前版本——直接加载
  | "migratable"    // 旧版本——可迁移（附降级清单）
  | "unsupported";  // 未来版本/未知格式——拒绝并说明

export interface CompatVerdict {
  level: CompatLevel;
  fromVersion: number | null;
  toVersion: number;
  /** 迁移将降级/放弃的项（人话清单——迁移不是静默的）。 */
  degradations: string[];
  reason: string;
}

/** 版本判定（矩阵主入口——导入前先问这一句）。 */
export function compatibilityVerdict(rawVersion: unknown): CompatVerdict {
  const v = typeof rawVersion === "number" ? rawVersion : null;
  if (v === null) {
    return {
      level: "unsupported", fromVersion: null, toVersion: TOKEN_TABLE_VERSION,
      degradations: [], reason: "包缺少版本戳——拒绝猜测（门禁要确定性）",
    };
  }
  if (v === TOKEN_TABLE_VERSION) {
    return { level: "native", fromVersion: v, toVersion: TOKEN_TABLE_VERSION, degradations: [], reason: "当前版本，直接加载" };
  }
  if (v >= 0 && v < TOKEN_TABLE_VERSION) {
    return {
      level: "migratable", fromVersion: v, toVersion: TOKEN_TABLE_VERSION,
      degradations: legacyMigrationDegradations(v),
      reason: `v${v} → v${TOKEN_TABLE_VERSION} 迁移可用（降级项见清单）`,
    };
  }
  return {
    level: "unsupported", fromVersion: v, toVersion: TOKEN_TABLE_VERSION,
    degradations: [], reason: `包版本 v${v} 高于当前 v${TOKEN_TABLE_VERSION}——请升级系统后再导入`,
  };
}

/** 各旧版本的降级项清单（矩阵文档化——迁移前用户看得见将失去什么）。 */
function legacyMigrationDegradations(from: number): string[] {
  const base = ["v1 新增的动效时长档按默认兜底", "未映射的语义色按出厂主题兜底（覆盖度报告可查）"];
  if (from === 0) base.unshift("v0 的 11 变量旧主题：仅颜色可桥接，圆角/字号/动效全部走默认");
  return base;
}

// ---------- v0 → v1 迁移 ----------

/** v0 旧变量 → v1 语义令牌桥表（与 theme-engine 双向桥同源——一处一事实）。 */
export const V0_BRIDGE: Readonly<Record<string, string>> = Object.freeze({
  "--bg-canvas": "--p-bg-canvas",
  "--bg-surface": "--p-bg-surface",
  "--bg-raised": "--p-bg-raised",
  "--text-primary": "--p-fg-primary",
  "--text-secondary": "--p-fg-secondary",
  "--accent": "--p-accent",
  "--accent-soft": "--p-accent-soft",
  "--success": "--p-success",
  "--warn": "--p-warn",
  "--danger": "--p-danger",
  "--stroke": "--p-border-regular",
});

export interface MigrationOutcome {
  ok: boolean;
  reason: string;
  /** 迁移后的 v1 表（ok=true 时非空）。 */
  table: TokenTable | null;
  /** 成功桥接的旧变量（迁移不是黑盒——逐项可查）。 */
  mapped: { from: string; to: string }[];
  /** 无法桥接被放弃的旧键（降级清单的实证）。 */
  dropped: string[];
  /** 迁移前原始输入（随时可退——回滚不丢作者数据）。 */
  source: unknown;
}

const HEX_RE = /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/;

/**
 * v0 主题包迁移：只接受颜色（旧包只有颜色层）；非法值拒绝——不静默清洗
 * （主册【状态与异常】「非法值 → 校验拦截+指出」同源纪律）。
 */
export function migrateTokenTableV0(raw: unknown): MigrationOutcome {
  const base: TokenTable = defaultTokenTable();
  const verdict = compatibilityVerdict((raw as { version?: unknown } | null)?.version ?? 0);
  if (verdict.level === "unsupported") {
    return { ok: false, reason: verdict.reason, table: null, mapped: [], dropped: [], source: raw };
  }
  const colors = (raw as { colors?: Record<string, unknown> } | null)?.colors;
  if (typeof colors !== "object" || colors === null) {
    return { ok: false, reason: "v0 包缺少 colors 层——结构不合预期，拒绝迁移", table: null, mapped: [], dropped: [], source: raw };
  }
  const mapped: { from: string; to: string }[] = [];
  const dropped: string[] = [];
  const table: TokenTable = JSON.parse(JSON.stringify(base)) as TokenTable;
  for (const [k, v] of Object.entries(colors)) {
    const target = V0_BRIDGE[k];
    if (!target || typeof v !== "string" || !HEX_RE.test(v)) {
      dropped.push(k);
      continue;
    }
    table.colors[target] = v;
    mapped.push({ from: k, to: target });
  }
  if (mapped.length === 0) {
    return { ok: false, reason: "没有任何旧变量可桥接——不是 v0 主题包（诚实拒绝，不产出空迁移）", table: null, mapped: [], dropped, source: raw };
  }
  return {
    ok: true,
    reason: `v0→v1 迁移完成：桥接 ${mapped.length} 项${dropped.length > 0 ? `，放弃 ${dropped.length} 项（${dropped.slice(0, 4).join(" ")}${dropped.length > 4 ? "…" : ""}）` : ""}`,
    table, mapped, dropped, source: raw,
  };
}
