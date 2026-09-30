/**
 * AI-07 · N-18 自动化宏引擎（模型 + 执行器 + 护栏，前端同源模型）：
 * - 动作库：环境命令（N-13 注册表直连）+ 键鼠模拟（仅前台护栏）+ 通知
 * - 触发器五类：热键 / 进程事件 / 时间（cron 子集）/ 剪贴板正则 / 场景切换
 * - 护栏（首日落地）：全局急停（长按 Ctrl+Esc 1s）；UAC/密码框焦点智能判停；
 *   运行角标（事件通知）；导入敏感动作逐条确认
 * - 双形态同源：可视化步骤列表 ↔ YAML 脚本（.vmacro 分享）
 * 红线：安全分叉（密码框判停等）硬编码，宏/规则/插件不可触碰。
 */

// ---------- 模型 ----------

export type MacroActionType =
  | "command" // N-13 注册表命令执行
  | "sendText" // 键鼠模拟：输入文本（仅前台窗口）
  | "clipboardWrite" // 写剪贴板
  | "notify"; // 提示通知

export interface MacroAction {
  type: MacroActionType;
  /** command: 命令 id；sendText/clipboardWrite/notify: 文本。 */
  value: string;
}

export type MacroTriggerType = "hotkey" | "process" | "time" | "clipboardRegex" | "scene";

export interface MacroTrigger {
  type: MacroTriggerType;
  /** hotkey: accel；process: 进程名；time: cron 子集 "m h dom mon dow"；clipboardRegex: 正则；scene: 场景 id。 */
  value: string;
}

export interface Macro {
  id: string;
  name: string;
  enabled: boolean;
  trigger: MacroTrigger;
  actions: MacroAction[];
  /** 含键鼠模拟的宏 = 敏感（导入时逐条确认；UAC/密码框自动判停）。 */
  createdAt: number;
}

export function isSensitiveMacro(m: Macro): boolean {
  return m.actions.some((a) => a.type === "sendText");
}

// ---------- cron 子集校验（m h dom mon dow，* 与数字与 , 与 -） ----------

export function isValidCron(expr: string): boolean {
  const fields = expr.trim().split(/\s+/);
  if (fields.length !== 5) return false;
  const ranges = [
    [0, 59], [0, 23], [1, 31], [1, 12], [0, 6],
  ] as const;
  return fields.every((f, i) => {
    const [lo, hi] = ranges[i]!;
    if (f === "*") return true;
    return f.split(",").every((part) => {
      if (/^\d+$/.test(part)) {
        const n = Number(part);
        return n >= lo && n <= hi;
      }
      const m = part.match(/^(\d+)-(\d+)$/);
      if (!m) return false;
      const a = Number(m[1]);
      const b = Number(m[2]);
      return a >= lo && b <= hi && a <= b;
    });
  });
}

// ---------- 护栏 ----------

export interface GuardContext {
  /** 当前前台窗口是 UAC 提权窗口（必须判停）。 */
  uacForeground: boolean;
  /** 当前焦点是密码框（必须判停）。 */
  passwordFocus: boolean;
}

/** 智能判停（硬编码安全分叉：宏不可绕过）。 */
export function shouldPauseMacros(ctx: GuardContext): boolean {
  return ctx.uacForeground || ctx.passwordFocus;
}

/** 急停状态（全局 Ctrl+Esc 长按 1s 置位；100% 生效验收项）。 */
export class EmergencyStop {
  private stopped = false;
  stop(): void {
    this.stopped = true;
  }
  clear(): void {
    this.stopped = false;
  }
  isStopped(): boolean {
    return this.stopped;
  }
}

// ---------- 执行器 ----------

export interface RunContext extends GuardContext {
  /** 命令执行器（N-13 注册表直连；注入便于测试）。 */
  runCommand: (id: string) => Promise<unknown>;
  /** 通知管道。 */
  notify: (text: string) => void;
  /** 写剪贴板。 */
  writeClipboard: (text: string) => Promise<void>;
  /** 键鼠模拟（仅前台窗口护栏；测试注入）。 */
  sendText: (text: string) => Promise<void>;
  emergency: EmergencyStop;
}

export interface RunResult {
  ok: boolean;
  /** 失败时停在第几步（1 起）。 */
  failedAtStep?: number;
  reason?: "emergency-stop" | "guard-pause" | "command-error" | "disabled";
}

/** 执行宏：逐动作串行；护栏/急停首检 + 每步间复检（键鼠宏失控防线）。 */
export async function runMacro(m: Macro, ctx: RunContext): Promise<RunResult> {
  if (!m.enabled) return { ok: false, reason: "disabled" };
  if (ctx.emergency.isStopped()) return { ok: false, reason: "emergency-stop" };
  if (shouldPauseMacros({ uacForeground: ctx.uacForeground, passwordFocus: ctx.passwordFocus })) {
    return { ok: false, reason: "guard-pause" };
  }
  for (let i = 0; i < m.actions.length; i++) {
    // 每步之间复检急停（回放失败即停并报告步骤号）
    if (ctx.emergency.isStopped()) return { ok: false, failedAtStep: i + 1, reason: "emergency-stop" };
    const a = m.actions[i]!;
    try {
      switch (a.type) {
        case "command":
          await ctx.runCommand(a.value);
          break;
        case "sendText":
          // 仅前台窗口护栏：执行前再判一次焦点安全
          if (shouldPauseMacros({ uacForeground: ctx.uacForeground, passwordFocus: ctx.passwordFocus })) {
            return { ok: false, failedAtStep: i + 1, reason: "guard-pause" };
          }
          await ctx.sendText(a.value);
          break;
        case "clipboardWrite":
          await ctx.writeClipboard(a.value);
          break;
        case "notify":
          ctx.notify(a.value);
          break;
      }
    } catch {
      return { ok: false, failedAtStep: i + 1, reason: "command-error" };
    }
  }
  return { ok: true };
}

// ---------- 录制回放（坐标 → 窗口相对坐标参数化） ----------

export interface RecordedStep {
  /** 原始绝对坐标。 */
  x: number;
  y: number;
  /** 录制时目标窗口原点（用于相对化）。 */
  originX: number;
  originY: number;
  text?: string;
}

/** 参数化：绝对坐标 → 窗口相对坐标（回放时按当前窗口原点换算）。 */
export function relativizeSteps(steps: RecordedStep[]): { dx: number; dy: number; text?: string }[] {
  return steps.map((s) => ({ dx: s.x - s.originX, dy: s.y - s.originY, text: s.text }));
}

// ---------- 双形态同源：YAML 子集 ----------

function err(msg: string): never {
  throw new Error(`[.vmacro] ${msg}`);
}

const TYPE_IDS: Record<MacroTriggerType, string> = {
  hotkey: "hotkey", process: "process", time: "time", clipboardRegex: "clipboard_regex", scene: "scene",
};
const TRIGGER_IDS: Record<string, MacroTriggerType> = Object.fromEntries(
  Object.entries(TYPE_IDS).map(([k, v]) => [v, k as MacroTriggerType]),
);
const ACTION_IDS: Record<MacroActionType, string> = {
  command: "command", sendText: "send_text", clipboardWrite: "clipboard_write", notify: "notify",
};
const ACTION_IDS_REV: Record<string, MacroActionType> = Object.fromEntries(
  Object.entries(ACTION_IDS).map(([k, v]) => [v, k as MacroActionType]),
);

function esc(s: string): string {
  return /[:#"'{}\[\],&*?|>%@`]/.test(s) ? JSON.stringify(s) : s;
}

/** 宏 → YAML 脚本（.vmacro 分享格式；高级用户可编辑）。 */
export function macroToYaml(m: Macro): string {
  const lines = [
    `id: ${esc(m.id)}`,
    `name: ${esc(m.name)}`,
    `enabled: ${m.enabled}`,
    `trigger:`,
    `  type: ${TYPE_IDS[m.trigger.type]}`,
    `  value: ${esc(m.trigger.value)}`,
    `actions:`,
  ];
  for (const a of m.actions) lines.push(`  - type: ${ACTION_IDS[a.type]}`, `    value: ${esc(a.value)}`);
  return lines.join("\n") + "\n";
}

/** YAML 脚本 → 宏（严格子集解析；行级错误报告）。 */
export function yamlToMacro(src: string): Macro {
  const m: Macro = { id: "", name: "", enabled: true, trigger: { type: "hotkey", value: "" }, actions: [], createdAt: Date.now() };
  let inTrigger = false;
  let inActions = false;
  let curAction: MacroAction | null = null;
  for (const rawLine of src.split(/\r?\n/)) {
    const line = rawLine.replace(/\s+$/, "");
    if (!line || line.startsWith("#")) continue;
    // 动作列表项：`  - type: command`
    const listItem = line.match(/^(\s*)-\s+type:\s*(\S+)\s*$/);
    if (listItem) {
      const t = ACTION_IDS_REV[listItem[2]!];
      if (!t) err(`未知动作类型: ${listItem[2]}`);
      if (!inActions) err("动作列表项出现在 actions 之外");
      curAction = { type: t, value: "" };
      m.actions.push(curAction);
      continue;
    }
    const kv = line.match(/^(\s*)([a-z_]+):\s*(.*)$/);
    if (!kv) err(`无法解析的行: ${line}`);
    const [, indent, key, rawVal] = kv;
    // 值去引号（macroToYaml 对含特殊字符的值 JSON.stringify）
    let val = rawVal;
    if (val && /^".*"$/.test(val)) {
      try {
        const parsed = JSON.parse(val) as string;
        if (typeof parsed === "string") val = parsed;
      } catch {
        /* 保留原文，后续校验如实报错 */
      }
    }
    const deep = indent!.length >= 2;
    if (!deep && key !== "type" && key !== "value") {
      inTrigger = key === "trigger";
      inActions = key === "actions";
      curAction = null;
      switch (key) {
        case "id": m.id = val!; break;
        case "name": m.name = val!; break;
        case "enabled": m.enabled = val === "true"; break;
        case "trigger": case "actions": break;
        default: err(`未知字段: ${key}`);
      }
      continue;
    }
    if (inTrigger) {
      if (key === "type") {
        const t = TRIGGER_IDS[val!];
        if (!t) err(`未知触发器类型: ${val}`);
        m.trigger.type = t;
      } else if (key === "value") m.trigger.value = val!;
      else err(`触发器未知字段: ${key}`);
    } else if (inActions) {
      if (line.trim().startsWith("- type:")) {
        const t = ACTION_IDS_REV[line.trim().slice(2).trim().replace("type:", "").trim()];
        if (!t) err(`未知动作类型: ${line}`);
        curAction = { type: t, value: "" };
        m.actions.push(curAction);
      } else if (key === "type") {
        const t = ACTION_IDS_REV[val!];
        if (!t) err(`未知动作类型: ${val}`);
        curAction = { type: t, value: "" };
        m.actions.push(curAction);
      } else if (key === "value") {
        if (!curAction) err("value 出现在动作上下文之外");
        curAction.value = val!;
      } else err(`动作未知字段: ${key}`);
    } else err(`顶层缩进错误: ${line}`);
  }
  if (m.trigger.type === "time" && m.trigger.value && !isValidCron(m.trigger.value)) err(`非法 cron: ${m.trigger.value}`);
  if (!m.id || !m.name) err("缺少 id/name");
  return m;
}

// ---------- V-45 宏收藏（命令面板顶部「常用宏」区） ----------
// 钉选位复用 commands/registry 的 pinCommand（cap 8 同口径），
// 宏删除时钉选位同步清理由宏库层调用 unpinCommand("macro:<id>")。

export function macroCommandId(macroId: string): string {
  return `macro:${macroId}`;
}
