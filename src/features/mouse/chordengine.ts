/**
 * J 鼠标域 · F615 侧键编程 · 和弦引擎（批次八）。
 *
 * v3/v4 的侧键是「一键一动作」+ 应用覆盖。真实工作流里按键数量是
 * 硬件上限（通常 2 个侧键），和弦（双键组合）是零硬件成本扩容：
 *
 * 1. 同时按下与顺序按压两种和弦形态——同时（窗内两键都处于按下态）
 *    与顺序（A 按下后窗内 B 跟进）判据不同，错误形态的映射不该误触；
 *
 * 2. 单键消歧延迟——配置了「A+B 和弦」后，单独按下 A 必须等
 *    CHORD_WINDOW_MS 确认没有 B 跟进才触发 A 的单键动作
 *    （这是和弦的代价：单键延迟；只在配置了含该键的和弦时才付，
 *    纯单键配置零延迟——不为不存在的功能付延迟税）；
 *
 * 3. 冲突裁决——同一键既在单键映射又在和弦里：和弦赢（配置了和弦
 *    即声明了意图）；运行时把「受影响的单键」显式列出（设置页警示）。
 *
 * 判据锚点：
 * - 同时窗 vs 顺序窗 → matchChord()
 * - 消歧延迟 → needsDisambiguation()
 * - 冲突显式化 → conflicts()
 */

/** 和弦时间窗（ms）：两键间隔/重叠在此窗内才算和弦。 */
export const CHORD_WINDOW_MS = 180;

/** 侧键枚举（与 sideButtons.ts 的 back/forward 键位同源）。 */
export type SideKey = "back" | "forward";

/** 和弦：两键的组合与触发动作。 */
export interface ChordBinding {
  /** 和弦 id（映射登记与审计用）。 */
  id: string;
  /** 参与键（两键；顺序无关录入——形态由 kind 决定）。 */
  keys: [SideKey, SideKey];
  /** "simultaneous" = 窗内重叠按下；"sequence" = 第一键按下后窗内第二键跟进。 */
  kind: "simultaneous" | "sequence";
  /** 触发动作（actions 路由同源）。 */
  action: string;
}

/** 键位事件流（运行时喂入）。 */
export interface KeyEvent {
  key: SideKey;
  /** "down" | "up"。 */
  kind: "down" | "up";
  atMs: number;
}

/* ------------------------------- 和弦匹配（纯函数） ------------------------------- */

/**
 * 从事件流尾部匹配和弦：返回命中的和弦 id 或 null。
 * - simultaneous：两键的 down 事件都存在且 down 时刻差 ≤ 窗（无 up 介入）；
 * - sequence：第一键 down 后，窗内第二键 down（第一键可未 up）。
 * 事件流约定：只保留最近 2s（调用方裁剪——本函数不裁剪不修改）。
 */
export function matchChord(events: KeyEvent[], chords: ChordBinding[]): string | null {
  const downs = events.filter((e) => e.kind === "down");
  if (downs.length < 2) return null;
  const last = downs[downs.length - 1]!;
  const prev = downs[downs.length - 2]!;
  if (last.atMs - prev.atMs > CHORD_WINDOW_MS) return null;
  const pair: [SideKey, SideKey] = [prev.key, last.key];
  for (const c of chords) {
    if (c.kind === "simultaneous") {
      // 重叠判据：两 down 都在窗内，且先按的键尚未 up（downs 里无中间 up）。
      const upBetween = events.some((e) => e.kind === "up" && e.key === pair[0] && e.atMs > prev.atMs && e.atMs < last.atMs);
      if (!upBetween && samePair(pair, c.keys)) return c.id;
    } else {
      // 顺序判据：有序配对命中即可（先按=keys[0]）。
      if (pair[0] === c.keys[0] && pair[1] === c.keys[1]) return c.id;
    }
  }
  return null;
}

function samePair(a: [SideKey, SideKey], b: [SideKey, SideKey]): boolean {
  return (a[0] === b[0] && a[1] === b[1]) || (a[0] === b[1] && a[1] === b[0]);
}

/* ------------------------------- 消歧与冲突 ------------------------------- */

/**
 * 该键是否需要消歧延迟：参与了任一和弦 → 单键动作要等窗过期。
 * 纯单键配置（无和弦）返回 false——零延迟税。
 */
export function needsDisambiguation(key: SideKey, chords: ChordBinding[]): boolean {
  return chords.some((c) => c.keys[0] === key || c.keys[1] === key);
}

/**
 * 冲突清单：参与了和弦的单键映射（和弦赢，但设置页必须显式警示）。
 * 返回受影响键位——空数组 = 无冲突。
 */
export function conflicts(chords: ChordBinding[]): SideKey[] {
  const touched = new Set<SideKey>();
  for (const c of chords) {
    touched.add(c.keys[0]);
    touched.add(c.keys[1]);
  }
  return [...touched];
}

/* ------------------------------- 运行时状态机 ------------------------------- */

/**
 * 和弦运行时：事件流喂入，单键动作延迟触发由本机裁决。
 * 宿主不再自行判单键——onSideButton 全部经此机（一处一事实）。
 */
export class ChordRuntime {
  private events: KeyEvent[] = [];
  private pendingSingle: { key: SideKey; timerMs: number; action: string } | null = null;

  constructor(private readonly chords: () => ChordBinding[]) {}

  /**
   * 喂一个键位事件。
   * @returns 本事件应立即执行的动作，或 null（可能等待和弦/消歧）。
   * 宿主拿 null 时若此前有 pendingSingle 过期，会经 flushSingle 吐出。
   */
  feed(ev: KeyEvent, singleAction: (key: SideKey) => string | null): { immediate: string | null; deferred: string | null } {
    this.trim(ev.atMs);
    // 消歧到期检查：新事件到来时先结算到期的单键（单键动作不丢）。
    let deferred: string | null = null;
    if (this.pendingSingle && ev.atMs - this.pendingSingle.timerMs > CHORD_WINDOW_MS) {
      deferred = this.pendingSingle.action;
      this.pendingSingle = null;
    }

    if (ev.kind === "down") {
      // 先试和弦匹配（含本事件）。
      this.events.push(ev);
      const hit = matchChord(this.events, this.chords());
      if (hit) {
        const chord = this.chords().find((c) => c.id === hit);
        this.pendingSingle = null; // 和弦赢：取消挂起单键
        return { immediate: chord?.action ?? null, deferred };
      }
      // 和弦未中：若该键参与和弦 → 挂起单键等消歧；否则立即执行。
      if (needsDisambiguation(ev.key, this.chords())) {
        const action = singleAction(ev.key);
        if (action) this.pendingSingle = { key: ev.key, timerMs: ev.atMs, action };
        return { immediate: null, deferred };
      }
      return { immediate: singleAction(ev.key), deferred };
    }
    // up 事件：仅记录（重叠判据消费它），无动作。
    this.events.push(ev);
    return { immediate: null, deferred };
  }

  /** 宿主定时器调（CHORD_WINDOW_MS 后）：结算挂起单键。 */
  flushSingle(atMs: number): string | null {
    if (this.pendingSingle && atMs - this.pendingSingle.timerMs > CHORD_WINDOW_MS) {
      const action = this.pendingSingle.action;
      this.pendingSingle = null;
      return action;
    }
    return null;
  }

  get pendingKey(): SideKey | null {
    return this.pendingSingle?.key ?? null;
  }

  /** 事件流裁剪（2s 窗——内存上限纪律）。 */
  private trim(atMs: number): void {
    this.events = this.events.filter((e) => atMs - e.atMs <= 2000);
  }

  reset(): void {
    this.events = [];
    this.pendingSingle = null;
  }
}
