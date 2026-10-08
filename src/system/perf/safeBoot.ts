/**
 * AI-13 Z-57 安全模式启动（Safe Boot Mode）
 *
 * 红线：安全模式不得修改用户配置；跳过全部第三方资源加载（主题/连接器/事件流/手势/方案）；
 * 角标明示。
 */

export interface SafeBootCheck {
  safe: boolean;
  /** 触发来源 */
  source: "param" | "setting" | "crash-streak" | "none";
  /** 被跳过的加载器清单（角标与诊断呈现） */
  skipped: string[];
}

/** 明确必跳的第三方/可延迟加载器（与 App.tsx 既有 safeMode 分支对齐补全） */
export const SAFE_SKIPPED_LOADERS = [
  "wallpaper-video",
  "wallpaper-shader",
  "custom-cursor-pack",
  "third-party-connectors",
  "event-stream",
  "gestures",
  "typing-sounds",
  "widget-third-party",
] as const;

/** 判定是否安全模式：URL 参数 ?safe=1 优先，其次设置位，其次崩溃连击标记。 */
export function detectSafeBoot(search: string, settingSafeMode: boolean, crashStreak?: boolean): SafeBootCheck {
  const params = new URLSearchParams(search);
  if (params.get("safe") === "1") {
    return { safe: true, source: "param", skipped: [...SAFE_SKIPPED_LOADERS] };
  }
  if (settingSafeMode) {
    return { safe: true, source: "setting", skipped: [...SAFE_SKIPPED_LOADERS] };
  }
  if (crashStreak) {
    return { safe: true, source: "crash-streak", skipped: [...SAFE_SKIPPED_LOADERS] };
  }
  return { safe: false, source: "none", skipped: [] };
}

/** 失败自动建议（U-23 联动）：连续异常退出 → 建议进入安全模式（只建议，不自动改配置）。 */
export function suggestSafeBoot(recentCrashes: number): { suggest: boolean; messageKey: string } {
  return recentCrashes >= 2
    ? { suggest: true, messageKey: "pfSafeSuggest" }
    : { suggest: false, messageKey: "" };
}

/** 安全模式合法操作白名单（不改用户配置红线的机器可判定形态）：只允许只读与运行态覆盖。 */
export function safeModeAllowedOps(op: string): boolean {
  const ALLOWED = new Set(["read-settings", "runtime-theme-override", "log", "diag", "benchmark"]);
  return ALLOWED.has(op);
}
