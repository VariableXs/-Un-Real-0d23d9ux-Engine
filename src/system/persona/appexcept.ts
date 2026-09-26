/**
 * F162 每应用主题例外 · 完整设计。
 *
 * 主册判据：全局切换时例外应用保持（20 次切换实测）；上限 10 生效；
 * 未接入令牌应用降级路径实测。
 *
 * 【功能定义】个别应用可脱离全局主题：例外清单上限 10 项、每项可指定独立深浅/
 * 强调色；全局一致优先、例外显式可控——共性是默认，个性是特权。
 *
 * 【状态与异常】例外应用卸载 → 自动清条目；应用不支持主题令牌（旧应用）→ 例外
 * 标注「该应用未接入令牌，仅深浅生效」诚实降级；冲突例外（同应用重复）→ 合并提示。
 *
 * 【设计细节】例外粒度=应用级（非窗口级——复杂度红线）；深浅例外实现=应用收到
 * 锁定令牌表（全局广播时跳过）；强调色例外只影响该应用自身控件；例外生效标识=
 * 应用标题栏小圆点；F162 与 E8 动效档独立（动效全局统一不做例外——简化纪律）。
 */

import { personaStore } from "./store";

export const SECTION = "theme";
export const MAX_EXCEPTIONS = 10;

export type ExceptionMode = "dark" | "light";

export interface AppThemeException {
  appId: string;
  appName: string;
  mode: ExceptionMode;
  /** 强调色覆盖（hex）；null=跟随全局。 */
  accentOverride: string | null;
  /** 应用是否接入令牌体系（false=仅深浅生效——诚实降级标注）。 */
  tokenAware: boolean;
}

export interface AppExceptionConfig {
  exceptions: AppThemeException[];
}

export function loadAppExceptions(): AppExceptionConfig {
  const stored = personaStore.getWith(SECTION, "appExceptions", undefined) as Partial<AppExceptionConfig> | undefined;
  return { exceptions: Array.isArray(stored?.exceptions) ? (stored?.exceptions as AppThemeException[]) : [] };
}

export function saveAppExceptions(c: AppExceptionConfig): void {
  personaStore.set(SECTION, { appExceptions: c });
}

const HEX_RE = /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/;

export interface AddResult {
  ok: boolean;
  reason: string;
  config: AppExceptionConfig;
  /** 冲突合并提示（同应用重复 → 合并）。 */
  merged: boolean;
}

/** 添加例外：上限 10 强制（超出提示精简——防清单腐化）；同应用重复合并。 */
export function addException(config: AppExceptionConfig, item: Omit<AppThemeException, "tokenAware"> & { tokenAware?: boolean }): AddResult {
  if (config.exceptions.length >= MAX_EXCEPTIONS) {
    return { ok: false, reason: `例外清单已达上限 ${MAX_EXCEPTIONS} 项，请先精简`, config, merged: false };
  }
  if (item.accentOverride !== null && !HEX_RE.test(item.accentOverride)) {
    return { ok: false, reason: "强调色格式须为 #rrggbb", config, merged: false };
  }
  const existing = config.exceptions.find((e) => e.appId === item.appId);
  if (existing) {
    // 冲突例外：合并（保留原条目 id，更新模式/强调色）。
    const exceptions = config.exceptions.map((e) =>
      e.appId === item.appId ? { ...e, mode: item.mode, accentOverride: item.accentOverride } : e,
    );
    return { ok: true, reason: `「${item.appName}」已有例外，已合并更新`, config: { exceptions }, merged: true };
  }
  return {
    ok: true,
    reason: "已添加例外",
    merged: false,
    config: { exceptions: [...config.exceptions, { ...item, tokenAware: item.tokenAware ?? true }] },
  };
}

export function removeException(config: AppExceptionConfig, appId: string): AppExceptionConfig {
  return { exceptions: config.exceptions.filter((e) => e.appId !== appId) };
}

/** 应用卸载 → 自动清条目（返回被清理的名单供提示）。 */
export function pruneUninstalled(config: AppExceptionConfig, installedAppIds: Set<string>): { config: AppExceptionConfig; removed: string[] } {
  const removed: string[] = [];
  const exceptions = config.exceptions.filter((e) => {
    if (installedAppIds.has(e.appId)) return true;
    removed.push(e.appName);
    return false;
  });
  return { config: { exceptions }, removed };
}

/**
 * 全局广播时的例外过滤：返回「应跳过本次广播的应用」集合。
 * 深浅例外实现=应用收到锁定令牌表（全局广播时跳过）——本函数即跳过判定。
 */
export function skipOnGlobalBroadcast(exceptions: AppThemeException[]): Set<string> {
  return new Set(exceptions.map((e) => e.appId));
}

/** 例外应用的锁定令牌侧（应用窗口启动时取）。 */
export function lockedModeFor(exceptions: AppThemeException[], appId: string): ExceptionMode | null {
  return exceptions.find((e) => e.appId === appId)?.mode ?? null;
}

/** 未接入令牌应用的诚实降级标注文案键。 */
export function degradationLabel(e: AppThemeException): string | null {
  return e.tokenAware ? null : "该应用未接入令牌，仅深浅生效";
}
