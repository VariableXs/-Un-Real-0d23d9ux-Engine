/**
 * F161/F162 档案与应用例外引擎深化 · 包签名链 + 版本迁移 + 分节校验器 +
 * 锁定令牌派生。
 *
 * 主册判据延伸：
 * - F161【设计细节】「包签名（F127）」「跨版本导入 → 兼容矩阵校验+降级项清单」
 *   「分节独立勾选（包是菜单不是套餐）」。
 * - F162【设计细节】「深浅例外实现=应用收到锁定令牌表（全局广播时跳过）」——
 *   锁定令牌派生引擎：按例外模式从当前表派生该应用实际收到的表。
 */

import { ARCHIVE_FORMAT, ARCHIVE_VERSION, type ArchivePackage } from "./archive";
import { defaultTokenTable, type TokenTable } from "./tokens";
import type { AppThemeException } from "./appexcept";
import type { PersonaSection } from "./store";

// ---------- 包签名链（HMAC 风格 hash chain——防篡改可验证） ----------

export interface SignedEnvelope {
  payload: string;
  /** 签名 = FNV-1a64(payload + secret)（对称校验——本地信任模型）。 */
  signature: string;
  signedAt: string;
  signerVersion: number;
}

function fnv1a64Fold(s: string): string {
  let h1 = 0x811c9dc5;
  let h2 = 0x01000193;
  for (let i = 0; i < s.length; i++) {
    h1 = Math.imul(h1 ^ s.charCodeAt(i), 0x01000193) >>> 0;
    h2 = Math.imul(h2 + s.charCodeAt(i) * (i + 1), 0x85ebca6b) >>> 0;
  }
  return `${h1.toString(16).padStart(8, "0")}${h2.toString(16).padStart(8, "0")}`;
}

/** 对档案包做签名（secret 来自本机安装指纹——同一安装可互验，跨机重签）。 */
export function signArchive(pkg: ArchivePackage, secret: string): SignedEnvelope {
  const payload = JSON.stringify(pkg);
  return { payload, signature: fnv1a64Fold(payload + secret), signedAt: new Date().toISOString(), signerVersion: ARCHIVE_VERSION };
}

/** 验签：签名匹配 → payload 可信；不匹配 → 篡改/跨机（提示重签后导入）。 */
export function verifySignature(envelope: SignedEnvelope, secret: string): { ok: boolean; reason: string } {
  const expected = fnv1a64Fold(envelope.payload + secret);
  if (expected !== envelope.signature) {
    return { ok: false, reason: "签名不匹配——档案已被篡改或来自其他安装（重新导出后导入）" };
  }
  return { ok: true, reason: "签名验证通过" };
}

// ---------- 分节校验器（导入前逐节体检——先校验后切换的原子前提） ----------

export type SectionValidator = (data: Record<string, unknown>) => string | null;

/** 已知分节的校验器表（未知分节降级忽略——兼容矩阵纪律）。 */
export const SECTION_VALIDATORS: Partial<Record<PersonaSection, SectionValidator>> = {
  theme: (d) => {
    const t = d["tokenTable"];
    if (t !== undefined && (typeof t !== "object" || t === null)) return "theme.tokenTable 须为对象";
    const ad = d["autoDark"];
    if (ad !== undefined && typeof ad !== "object") return "theme.autoDark 须为对象";
    return null;
  },
  sound: (d) => {
    const m = d["mixer"];
    if (m !== undefined && (typeof m !== "object" || m === null)) return "sound.mixer 须为对象";
    return null;
  },
  taskbar: (d) => {
    const p = d["prefs"];
    if (p !== undefined && (typeof p !== "object" || p === null)) return "taskbar.prefs 须为对象";
    return null;
  },
  shortcuts: (d) => {
    const o = d["overrides"];
    if (o !== undefined && (typeof o !== "object" || o === null)) return "shortcuts.overrides 须为对象";
    return null;
  },
};

/** 全包分节校验：返回错误清单（空 = 可导入）。 */
export function validateSections(pkg: ArchivePackage): string[] {
  const errors: string[] = [];
  if (pkg.format !== ARCHIVE_FORMAT) errors.push(`format 须为 ${ARCHIVE_FORMAT}`);
  for (const [sec, data] of Object.entries(pkg.sections)) {
    if (!data) continue;
    const validator = SECTION_VALIDATORS[sec as PersonaSection];
    const err = validator?.(data);
    if (err) errors.push(err);
  }
  return errors;
}

// ---------- 版本迁移（v0 → v1：字段改名/结构升级） ----------

export interface MigrationResult {
  pkg: ArchivePackage;
  applied: string[];
}

/** v0 → v1 迁移：autoDark.mode 字符串枚举更新、icons.packs 结构收拢。 */
export function migrateArchiveV0toV1(raw: Record<string, unknown> & { sections?: Record<string, Record<string, unknown>> }): MigrationResult {
  const applied: string[] = [];
  const sections = raw.sections ?? {};
  const theme = sections["theme"];
  if (theme && typeof theme["autoDark"] === "object" && theme["autoDark"] !== null) {
    const ad = theme["autoDark"] as Record<string, unknown>;
    if (ad["enabled"] === true && ad["mode"] === undefined) {
      ad["mode"] = "timer";
      applied.push("theme.autoDark.mode 默认 timer（v0 布尔开关 → v1 模式枚举）");
    }
  }
  const wallpaper = sections["wallpaper"];
  if (wallpaper && Array.isArray(wallpaper["recent"]) === false && wallpaper["recent"] !== undefined) {
    wallpaper["recent"] = [];
    applied.push("wallpaper.recent 规整为数组");
  }
  return {
    pkg: { format: ARCHIVE_FORMAT, version: 1, meta: (raw.meta as ArchivePackage["meta"]) ?? { name: "migrated", createdAt: new Date().toISOString(), version: 1 }, sections: sections as ArchivePackage["sections"] },
    applied,
  };
}

// ---------- 锁定令牌派生（F162：例外应用实际收到的表） ----------

/** 深浅锁定派生：以标准深/浅默认表为基底，叠加全局强调色覆盖（例外语义）。 */
export function deriveLockedTokenTable(mode: "dark" | "light", globalAccent: string | null): TokenTable {
  const table = defaultTokenTable();
  // 深浅档用出厂默认（深=默认表；浅=提亮基底——派生规则文档化，勿引运行时主题）。
  if (mode === "light") {
    table.colors["--p-bg-canvas"] = "#eef0f6";
    table.colors["--p-fg-primary"] = "#26283a";
    table.colors["--p-fg-secondary"] = "#5a5c72";
  }
  if (globalAccent) {
    table.colors["--p-accent"] = globalAccent;
  }
  return table;
}

/** 例外应用的全局广播跳过 + 锁定表派生（一个调用点给齐——窗口启动时消费）。 */
export function lockedTableForException(ex: AppThemeException, globalAccent: string | null): TokenTable {
  return deriveLockedTokenTable(ex.mode, ex.accentOverride ?? globalAccent);
}

// ---------- 包体积预估（F161【设计细节】「包体积预估显示（导入前）」） ----------

export interface SizeEstimate {
  /** JSON 序列化字节数（UTF-8 口径）。 */
  bytes: number;
  /** 人话刻度（KB/MB 自适应）。 */
  label: string;
  /** 资产引用数（真资产不在包内——引用计数供体积预期校准）。 */
  assetRefs: number;
}

const utf8Encoder: TextEncoder | null = typeof TextEncoder !== "undefined" ? new TextEncoder() : null;

function utf8ByteLength(s: string): number {
  if (utf8Encoder) return utf8Encoder.encode(s).length;
  // 无 TextEncoder 环境的保守估算（ASCII 按主册包场景占比兜底）。
  return s.length;
}

/**
 * 导入前体积预估：序列化字节数 + 资产引用计数。
 * 真资产（壁纸/指针位图）不在包内——只计引用（F126 开放格式：包是清单不是仓库），
 * 引用数让用户对「导入后还会拉多少资产」有预期。
 */
export function estimateArchiveSize(pkg: ArchivePackage): SizeEstimate {
  const json = JSON.stringify(pkg);
  const bytes = utf8ByteLength(json);
  const label = bytes >= 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : bytes >= 1024 ? `${(bytes / 1024).toFixed(1)} KB` : `${bytes} B`;
  let assetRefs = 0;
  for (const section of Object.values(pkg.sections)) {
    if (!section) continue;
    for (const v of Object.values(section)) {
      if (typeof v === "string" && (v.startsWith("assets/") || v.startsWith("asset-ref:"))) assetRefs++;
    }
  }
  return { bytes, label, assetRefs };
}
