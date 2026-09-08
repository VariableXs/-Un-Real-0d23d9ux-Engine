/**
 * AI-15 开放工具组 — 前端纯函数助手（供 OpenToolsTab 与单测使用）。
 * 与后端 workshop.rs 的调度语义保持镜像（预览用途，实际触发以后端调度器为准）。
 */

/** V-83 触发器形状（与 Shell.SchedTriggerView 一致）。 */
export type SchedTriggerLike =
  | { type: "at"; hour: number; minute: number }
  | { type: "interval"; secs: number }
  | { type: "idle"; secs: number }
  | { type: "login"; secs: number };

/**
 * 下次触发时间预览（V-83）。
 * - at：今日 HH:MM（本地时区），已过则明日同一时刻；
 * - interval：上次触发 + 间隔；从未触发则 now + 间隔；
 * - idle / login：依赖运行时状态，返回 0（前端显示「待机/下次启动」语义）。
 */
export function nextFireTime(trigger: SchedTriggerLike, nowMs: number, lastFiredMs = 0): number {
  switch (trigger.type) {
    case "at": {
      const d = new Date(nowMs);
      d.setHours(trigger.hour, trigger.minute, 0, 0);
      if (d.getTime() <= nowMs) d.setDate(d.getDate() + 1);
      return d.getTime();
    }
    case "interval": {
      const base = lastFiredMs > 0 ? lastFiredMs : nowMs;
      return base + trigger.secs * 1000;
    }
    case "idle":
    case "login":
      return 0;
  }
}

/** V-86 安全类启动项关键词（与 workshop.rs SAFE_STARTUP_KEYWORDS 镜像；命中则不显示延迟选项）。 */
export const SAFE_STARTUP_KEYWORDS = [
  "antivirus", "defender", "杀毒", "安全卫士", "360", "huorong", "hips", "firewall",
  "driver", "驱动",
] as const;

/** 纯函数：是否安全类启动项（杀毒/驱动辅助——如实避嫌，不提供延迟建议）。 */
export function isSafeStartupMirror(name: string, cmd: string): boolean {
  const hay = `${name} ${cmd}`.toLowerCase();
  return SAFE_STARTUP_KEYWORDS.some((k) => hay.includes(k));
}

/** PATH 值分行（V-82：分号地狱 → 行列表）。空段过滤，保留原序。 */
export function splitPathRows(value: string): string[] {
  return value.split(";").map((s) => s.trim()).filter((s) => s.length > 0);
}

/** 字节格式化（残留报告 / 资源包扫描展示用）。 */
export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** diff 条目 → 展示色类（V-89 三色标记）。 */
export function diffKindClass(kind: string): string {
  if (kind === "add") return "ot-kind-add";
  if (kind === "del") return "ot-kind-del";
  if (kind === "mod") return "ot-kind-mod";
  return "";
}

/** 时刻格式化（时间轴 / 日志统一口径）。 */
export function fmtTime(ms: number): string {
  if (ms <= 0) return "—";
  const d = new Date(ms);
  const p = (x: number): string => String(x).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** 触发器 → 简短文本（不本地化字段词，供单测与列表预览）。 */
export function triggerSummary(trigger: SchedTriggerLike): string {
  const p = (x: number): string => String(x).padStart(2, "0");
  switch (trigger.type) {
    case "at":
      return `at ${p(trigger.hour)}:${p(trigger.minute)}`;
    case "interval":
      return `interval ${trigger.secs}s`;
    case "idle":
      return `idle ${trigger.secs}s`;
    case "login":
      return `login ${trigger.secs}s`;
  }
}
