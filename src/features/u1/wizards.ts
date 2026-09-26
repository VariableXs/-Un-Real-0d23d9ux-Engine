/**
 * 向导四件（AI-U1 · F441 计划任务 / F442 还原点 / F443 蓝牙配对 /
 * F444 打印机安装）——前端生效面。
 *
 * 判据唯一源（主册摘文，与 kernel/varix/src/uni1/ v1 同参数）：
 * - F441「三步流程用例；频率选择器全型（一次性/每日/每周/每月）；执行
 *   留痕与失败归因；清单联动；空闲条件判定」。
 * - F442「创建时长 <30s；列表信息完整性；手动/自动标注；删除与轮替规则；
 *   创建期间可用性」。
 * - F443「发现/配对/确认全链；确认码核对判据（不跳过）；失败归因映射表；
 *   自动重连时长；改名持久化」。
 * - F444「USB 自动链路用例；手动搜索匹配；驱动来源三态标注；失败出路
 *   引导；测试页一键；与 F289 队列衔接」。
 */

import { u1Store } from "./u1store";

/* ------------------------------- F441 计划任务 ------------------------------- */

export type Schedule =
  | { kind: "once"; atMin: number }
  | { kind: "daily"; hour: number; minute: number }
  | { kind: "weekly"; weekday: number; hour: number; minute: number }
  | { kind: "monthly"; day: number; hour: number; minute: number };

export type RunCondition = { kind: "always" } | { kind: "idle"; minutes: number } | { kind: "on-power" };

export interface TriggerCtx { nowMin: number; weekday: number; dayOfMonth: number; idleMs: number; onPower: boolean }

/** 频率选择器人话标签（判据：选的是人话不是表达式）。 */
export function scheduleLabel(s: Schedule): string {
  const hm = (h: number, m: number) => `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
  switch (s.kind) {
    case "once": return `一次性：${hm(Math.floor(s.atMin / 60) % 24, s.atMin % 60)}`;
    case "daily": return `每天 ${hm(s.hour, s.minute)}`;
    case "weekly": return `每周${"日一二三四五六"[s.weekday]} ${hm(s.hour, s.minute)}`;
    case "monthly": return `每月 ${s.day} 日 ${hm(s.hour, s.minute)}`;
  }
}

/** 触发判定（与内核 schedtask due 同式：时刻+日期/星期命中 + 条件门）。 */
export function taskDue(s: Schedule, cond: RunCondition, ctx: TriggerCtx): boolean {
  const dayMin = ctx.nowMin % 1440;
  const hmHit = (h: number, m: number) => dayMin === h * 60 + m;
  const hit = s.kind === "once" ? ctx.nowMin === s.atMin
    : s.kind === "daily" ? hmHit(s.hour, s.minute)
    : s.kind === "weekly" ? ctx.weekday === s.weekday && hmHit(s.hour, s.minute)
    : ctx.dayOfMonth === s.day && hmHit(s.hour, s.minute);
  if (!hit) return false;
  if (cond.kind === "always") return true;
  if (cond.kind === "idle") return ctx.idleMs >= cond.minutes * 60 * 1000;
  return ctx.onPower;
}

/* ------------------------------- F442 还原点 ------------------------------- */

export const RESTORE_BUDGET_MS = 30000;
export const RESTORE_CAP = 10;

export interface RestorePoint { name: string; kind: "auto" | "manual"; atMs: number; sizeKb: number }

/** 默认名（判据：手动-日期时间）。 */
export function restoreDefaultName(kind: "auto" | "manual", atMs: number): string {
  const d = new Date(atMs);
  const stamp = `${d.getFullYear()}${String(d.getMonth() + 1).padStart(2, "0")}${String(d.getDate()).padStart(2, "0")}-${String(d.getHours()).padStart(2, "0")}${String(d.getMinutes()).padStart(2, "0")}${String(d.getSeconds()).padStart(2, "0")}`;
  return `${kind === "manual" ? "手动" : "自动"}-${stamp}`;
}

/** 轮替：超上限删最旧，最近一个永留（F325 同源）。 */
export function restoreRotate(points: RestorePoint[]): RestorePoint[] {
  const next = [...points].sort((a, b) => b.atMs - a.atMs); // 最新在前
  while (next.length > RESTORE_CAP) next.pop();
  return next;
}

/** 删除确认：确认位 + 最近一个永留保护。 */
export function restoreDelete(points: RestorePoint[], idx: number, confirmed: boolean): { ok: boolean; reason?: string } {
  if (!confirmed) return { ok: false, reason: "需要确认——删除还原点不可恢复" };
  if (idx === 0) return { ok: false, reason: "最近的还原点受保护——轮替自动处理，不手动删" };
  if (idx >= points.length) return { ok: false, reason: "还原点不存在——列表可能已刷新" };
  points.splice(idx, 1);
  return { ok: true };
}

/* ------------------------------- F443 蓝牙配对 ------------------------------- */

export const BT_RECONNECT_BUDGET_MS = 3000;

export type BtState = "discovered" | "awaiting-confirm" | "paired" | "failed";

export interface BtDevice { id: number; name: string; rssi: number; state: BtState; expectCode: number | null; customName: string | null }

/** 确认码核对（判据：不跳过——None/不一致都到不了 paired）。 */
export function btConfirm(d: BtDevice, input: number): boolean {
  const ok = d.expectCode !== null && d.expectCode === input;
  d.state = ok ? "paired" : "failed";
  if (ok) d.expectCode = null;
  return ok;
}

/** 失败归因映射表（与内核 failure_cause 同表：码 → 人话 + 下一步）。 */
export function btFailureCause(code: number): [string, string] {
  switch (code) {
    case 1: return ["设备未进入配对模式", "查看设备说明书长按配对键后重试"];
    case 2: return ["确认码不匹配", "重新配对并核对两端显示的数字"];
    case 3: return ["设备已连接到其他主机", "在原主机上断开连接后再配对"];
    case 4: return ["超出有效范围", "把设备靠近本机（10 米内）再试"];
    default: return ["配对失败", "关闭设备电源重开再试"];
  }
}

/* ------------------------------- F444 打印机安装 ------------------------------- */

export type DriverSource = "builtin" | "vendor" | "manual";

export const DRIVER_SOURCE_LABELS: Record<DriverSource, string> = {
  builtin: "驱动：内置库",
  vendor: "驱动：厂商包",
  manual: "驱动：需手动安装",
};

export interface Printer { model: string; source: DriverSource; state: "installing" | "ready" | "failed"; queueRegistered: boolean }

/** 手动搜索：型号子串匹配。 */
export function printerSearch(catalog: { model: string; source: DriverSource }[], query: string): { model: string; source: DriverSource }[] {
  return catalog.filter((e) => e.model.includes(query));
}

/** NeedsManual 不假装装好（诚实判据）。 */
export function printerInstall(catalog: { model: string; source: DriverSource }[], model: string, printers: Printer[]): boolean {
  const entry = catalog.find((e) => e.model === model);
  if (!entry || entry.source === "manual") return false;
  printers.push({ model, source: entry.source, state: "installing", queueRegistered: false });
  return true;
}

/** 装好即入 F289 打印队列（衔接判据）。 */
export function printerFinish(printers: Printer[], model: string): boolean {
  const p = printers.find((x) => x.model === model && x.state === "installing");
  if (!p) return false;
  p.state = "ready";
  p.queueRegistered = true;
  return true;
}

/** 失败出路引导（人话：哪找驱动、怎么装）。 */
export function printerFailureGuidance(source: DriverSource): string {
  switch (source) {
    case "builtin": return "内置库驱动安装失败——重启系统后重试一次";
    case "vendor": return "厂商包安装失败——到 vx 星图搜索该型号的厂商包重装";
    case "manual": return "此型号需要厂商驱动——请从厂商官网获取后用 vxapp 安装";
  }
}

/* ------------------------------- 面板读数 ------------------------------- */

export function taskIdleMinutes(): number {
  const cfg = u1Store.get<{ taskIdleMinutes?: number }>("wizards");
  return cfg.taskIdleMinutes ?? 15;
}
