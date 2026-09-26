/**
 * J 鼠标域 · F615 快捷键录制器（v4）。
 *
 * 侧键「快捷键组合」映射目标此前只能手填字符串（面板示例位是硬编码样例）——
 * 录制器把「按下你要的组合」变成真实输入：keydown 捕获 → 归一化
 * （Ctrl+Alt+Shift+Meta 修饰键定序 + 主键规范化）→ 冲突预检（与 F244 同源
 * 字段比对）→ 回填映射。Esc = 取消（交互状态机完整出路——章三）。
 *
 * 归一化与系统快捷键词典同构：主键字母大写、数字原样、方向键/功能键全名、
 * 其余按键用 e.key 原样（不可打印键即其全名，如 "F5"、"Home"）。
 */

export type RecorderState = "idle" | "recording";

/** 修饰键定序（全系统快捷键词典同款：Ctrl+Alt+Shift+Meta）。 */
const MOD_ORDER: { key: string; prop: "ctrlKey" | "altKey" | "shiftKey" | "metaKey" }[] = [
  { key: "Ctrl", prop: "ctrlKey" },
  { key: "Alt", prop: "altKey" },
  { key: "Shift", prop: "shiftKey" },
  { key: "Meta", prop: "metaKey" },
];

/** 主键规范化：字母大写；其余按 e.key 原样（空格统一为 "Space"）。 */
export function normalizeMainKey(key: string): string {
  if (key === " ") return "Space";
  if (key.length === 1) return key.toUpperCase();
  return key;
}

/**
 * 键事件 → 归一化组合串（纯函数，测试同源）。
 * 只按了修饰键（key 是另一修饰键名）返回 null——组合必须有主键（半套不收）。
 * Esc 返回 "Escape"（取消语义由状态机裁决，串照常产出）。
 */
export function normalizeKeyCombo(e: {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}): string | null {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return null;
  const main = normalizeMainKey(e.key);
  const mods = MOD_ORDER.filter((m) => e[m.prop]).map((m) => m.key);
  return [...mods, main].join("+");
}

/** 录制结果（回填映射用）。 */
export interface RecordedCombo {
  keys: string;
  /** 冲突预检结果（F244 注册行同源比对由调用方注入）。 */
  conflicts: string[];
}

/**
 * 快捷键录制器状态机：idle →（start）recording →（捕获）完成回调 → idle；
 * Esc = 取消回 idle；组件卸载/ dispose 自动摘监听（无幽灵回调）。
 * 冲突预检：existingRows 注入（F244 注册行），命中同串即冲突（不静默）。
 */
export class ShortcutRecorder {
  state: RecorderState = "idle";
  lastError: string | null = null;
  private onDone: ((r: RecordedCombo | null) => void) | null = null;
  private existingRows: { keys: string; action: string }[] = [];

  private onKeyDown = (e: KeyboardEvent): void => {
    if (this.state !== "recording") return;
    e.preventDefault();
    e.stopPropagation();
    const combo = normalizeKeyCombo(e);
    if (e.key === "Escape") {
      this.finish(null);
      return;
    }
    if (!combo) return; // 尚无主键：继续等
    this.finish({ keys: combo, conflicts: this.existingRows.filter((r) => r.keys === combo).map((r) => r.action) });
  };

  /**
   * 开始录制。
   * @param existingRows 冲突预检数据源（F244 注册行；缺省无冲突）
   */
  start(existingRows: { keys: string; action: string }[] = [], onDone: (r: RecordedCombo | null) => void): void {
    if (this.state === "recording") this.finish(null);
    this.existingRows = existingRows;
    this.onDone = onDone;
    this.lastError = null;
    this.state = "recording";
    // 捕获阶段监听：抢在系统快捷键路由之前（录制的输入是「素材」不是「指令」）。
    window.addEventListener("keydown", this.onKeyDown, { capture: true });
  }

  /** 取消（Esc 或调用方触发）。 */
  cancel(): void {
    if (this.state !== "recording") return;
    this.finish(null);
  }

  private finish(result: RecordedCombo | null): void {
    this.state = "idle";
    window.removeEventListener("keydown", this.onKeyDown, { capture: true } as EventListenerOptions);
    const cb = this.onDone;
    this.onDone = null;
    cb?.(result);
  }

  dispose(): void {
    if (this.state === "recording") this.finish(null);
  }

  /** 无 DOM 环境守卫下的状态查询（测试与审计面）。 */
  get recording(): boolean {
    return this.state === "recording";
  }
}
