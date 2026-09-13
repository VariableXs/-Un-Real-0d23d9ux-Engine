/**
 * UNREAL-X AI-18 · 族0175 撤销历史 2.0 + 族0176 自动化输入 2.0 + 族0177 聚焦书写 2.0
 * （X04351~X04450）。
 *
 * 撤销：事务栈/合并窗口/分支历史（redo 树）/断点续作。
 * 自动化：宏录制与回放/节拍/循环上限守护。
 * 聚焦书写：专注时段计时/干扰计数/心流叙事。
 */

/* ============================== 族0175 撤销历史 2.0 ============================== */

export const UNDO_PROFILES = [
  { id: "basic", name: "基础", stack: 50, mergeMs: 0, branch: false },
  { id: "light", name: "轻量", stack: 200, mergeMs: 400, branch: false },
  { id: "balanced", name: "均衡", stack: 500, mergeMs: 600, branch: true },
  { id: "deep", name: "深档", stack: 2000, mergeMs: 800, branch: true },
  { id: "full", name: "全量", stack: 10000, mergeMs: 1000, branch: true },
] as const;
export type UndoProfileId = (typeof UNDO_PROFILES)[number]["id"];
export const DEFAULT_UNDO_ID: UndoProfileId = "balanced";

export function findUndo(id: string): (typeof UNDO_PROFILES)[number] {
  return UNDO_PROFILES.find((p) => p.id === id) ?? UNDO_PROFILES[2]!;
}

export interface UndoTx {
  label: string;
  atMs: number;
  /** 反向操作（演示用闭包快照字段）。 */
  payload: string;
}

/** 撤销栈：合并窗口 + 上限裁剪 + 分支历史（redo 树简化为双侧栈）。 */
export class UndoStack {
  profileId: UndoProfileId;
  undo: UndoTx[] = [];
  redo: UndoTx[] = [];
  branchCount = 0;
  clamped = 0;

  constructor(profileId: string = DEFAULT_UNDO_ID) {
    const known = UNDO_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findUndo(profileId).id as UndoProfileId;
  }

  /** 提交一个事务；mergeMs 内同类事务合并（打字合并惯例）。 */
  commit(label: string, atMs: number, payload: string): void {
    const p = findUndo(this.profileId);
    const top = this.undo[this.undo.length - 1];
    if (p.mergeMs > 0 && top && top.label === label && atMs - top.atMs <= p.mergeMs) {
      this.undo[this.undo.length - 1] = { label, atMs, payload };
      if (this.undo.length > p.stack) { this.undo.shift(); this.clamped += 1; }
      return;
    }
    if (this.redo.length > 0) this.branchCount += 1; // 分叉历史
    this.redo = [];
    this.undo.push({ label, atMs, payload });
    while (this.undo.length > p.stack) { this.undo.shift(); this.clamped += 1; }
  }

  canUndo(): boolean { return this.undo.length > 0; }
  canRedo(): boolean { return this.redo.length > 0; }

  /** 撤销：返回被撤销事务（无则 null）。 */
  undoOne(): UndoTx | null {
    const tx = this.undo.pop();
    if (!tx) return null;
    this.redo.push(tx);
    return tx;
  }

  /** 重做：返回被重做事务（无则 null）。 */
  redoOne(): UndoTx | null {
    const tx = this.redo.pop();
    if (!tx) return null;
    this.undo.push(tx);
    return tx;
  }

  /** 断点续作：把栈顶标记为半成品（崩溃恢复语义）。 */
  pending(): UndoTx | null {
    return this.undo[this.undo.length - 1] ?? null;
  }

  /** 回滚净身。 */
  reset(): void {
    this.undo = [];
    this.redo = [];
    this.branchCount = 0;
    this.clamped = 0;
  }
}

/* ============================== 族0176 自动化输入 2.0 ============================== */

export type MacroStep =
  | { kind: "type"; text: string }
  | { kind: "key"; key: string }
  | { kind: "wait"; ms: number }
  | { kind: "loop"; times: number };

/** 自动化五档。 */
export const AUTO_INPUT_PROFILES = [
  { id: "off", name: "关闭", maxSteps: 0, loopMax: 0, minStepMs: 0 },
  { id: "basic", name: "基础", maxSteps: 32, loopMax: 2, minStepMs: 40 },
  { id: "balanced", name: "均衡", maxSteps: 128, loopMax: 10, minStepMs: 20 },
  { id: "power", name: "强力", maxSteps: 512, loopMax: 100, minStepMs: 10 },
  { id: "script", name: "脚本", maxSteps: 4096, loopMax: 1000, minStepMs: 0 },
] as const;
export type AutoInputProfileId = (typeof AUTO_INPUT_PROFILES)[number]["id"];
export const DEFAULT_AUTO_INPUT_ID: AutoInputProfileId = "balanced";

export function findAutoInput(id: string): (typeof AUTO_INPUT_PROFILES)[number] {
  return AUTO_INPUT_PROFILES.find((p) => p.id === id) ?? AUTO_INPUT_PROFILES[2]!;
}

/** 宏回放器：把步骤展开为动作序列（守护：步数/循环上限/节拍下限）。 */
export function expandMacro(steps: MacroStep[], profileId: string): { actions: string[]; clamped: number; narratives: string[] } {
  const p = findAutoInput(profileId);
  const actions: string[] = [];
  const narratives: string[] = [];
  let clamped = 0;
  if (p.maxSteps === 0) {
    narratives.push("AI-401");
    return { actions, clamped, narratives };
  }
  for (const s of steps) {
    if (actions.length >= p.maxSteps) { clamped += 1; narratives.push("AI-402"); break; }
    switch (s.kind) {
      case "type":
        for (const ch of [...s.text].slice(0, p.maxSteps - actions.length)) actions.push(`type:${ch}`);
        break;
      case "key": actions.push(`key:${s.key}`); break;
      case "wait": {
        const ms = Math.max(p.minStepMs, Math.min(60000, s.ms));
        if (ms !== s.ms) clamped += 1;
        actions.push(`wait:${ms}`);
        break;
      }
      case "loop": {
        const times = Math.min(p.loopMax, Math.max(0, s.times));
        if (times !== s.times) { clamped += 1; narratives.push("AI-403"); }
        actions.push(`loop:${times}`);
        break;
      }
    }
  }
  return { actions, clamped, narratives };
}

/** 宏录制器：采样用户动作成步骤（重复键去抖合并）。 */
export class MacroRecorder {
  profileId: AutoInputProfileId;
  steps: MacroStep[] = [];
  recording = false;
  clamped = 0;

  constructor(profileId: string = DEFAULT_AUTO_INPUT_ID) {
    const known = AUTO_INPUT_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findAutoInput(profileId).id as AutoInputProfileId;
  }

  start(): void { this.recording = true; }

  type(text: string): void {
    if (!this.recording) return;
    const max = findAutoInput(this.profileId).maxSteps;
    if (this.steps.length >= max) { this.clamped += 1; return; }
    const last = this.steps[this.steps.length - 1];
    if (last && last.kind === "type") last.text += text;
    else this.steps.push({ kind: "type", text });
  }

  key(key: string): void {
    if (!this.recording) return;
    if (this.steps.length >= findAutoInput(this.profileId).maxSteps) { this.clamped += 1; return; }
    this.steps.push({ kind: "key", key });
  }

  stop(): MacroStep[] {
    this.recording = false;
    return this.steps;
  }

  reset(): void {
    this.steps = [];
    this.recording = false;
    this.clamped = 0;
  }
}

/* ============================== 族0177 聚焦书写 2.0 ============================== */

export const FOCUS_PROFILES = [
  { id: "off", name: "关闭", minutes: 0, veil: false, quietKeys: false },
  { id: "sprint", name: "冲刺", minutes: 15, veil: true, quietKeys: false },
  { id: "balanced", name: "均衡", minutes: 25, veil: true, quietKeys: true },
  { id: "deep", name: "深潜", minutes: 50, veil: true, quietKeys: true },
  { id: "flow", name: "心流", minutes: 90, veil: true, quietKeys: true },
] as const;
export type FocusProfileId = (typeof FOCUS_PROFILES)[number]["id"];
export const DEFAULT_FOCUS_ID: FocusProfileId = "balanced";

export function findFocus(id: string): (typeof FOCUS_PROFILES)[number] {
  return FOCUS_PROFILES.find((p) => p.id === id) ?? FOCUS_PROFILES[2]!;
}

/** 聚焦书写会话：计时 + 干扰计数 + 完成叙事（tick 驱动，秒精度）。 */
export class FocusSession {
  profileId: FocusProfileId;
  phase: "idle" | "running" | "paused" | "done" = "idle";
  elapsedSec = 0;
  distractions = 0;
  words = 0;
  clamped = 0;

  constructor(profileId: string = DEFAULT_FOCUS_ID) {
    const known = FOCUS_PROFILES.some((p) => p.id === profileId);
    if (!known) this.clamped += 1;
    this.profileId = findFocus(profileId).id as FocusProfileId;
  }

  start(): void { this.phase = "running"; }

  /** 每秒 tick；文字增量计词（CJK 按字、拉丁按空格分词近似）。 */
  tick(textDelta: string, distracted: boolean): void {
    if (this.phase !== "running") return;
    this.elapsedSec += 1;
    if (distracted) this.distractions += 1;
    const cjk = (textDelta.match(/[\u4e00-\u9fff]/g) ?? []).length;
    const latin = (textDelta.match(/[a-zA-Z]+/g) ?? []).length;
    this.words += cjk + latin;
    const total = findFocus(this.profileId).minutes * 60;
    if (total > 0 && this.elapsedSec >= total) this.phase = "done";
  }

  pause(): void { if (this.phase === "running") this.phase = "paused"; }
  resume(): boolean { if (this.phase !== "paused") return false; this.phase = "running"; return true; }

  /** 完成度 0..100。 */
  percent(): number {
    const total = findFocus(this.profileId).minutes * 60;
    if (total <= 0) return 0;
    return Math.min(100, Math.round((this.elapsedSec / total) * 100));
  }

  /** 心流叙事：完成文案（微文案口径）。 */
  narrative(): string {
    const p = findFocus(this.profileId);
    if (this.phase !== "done") return `专注中 · ${p.name} ${p.minutes} 分钟`;
    return `${p.name}完成：${this.words} 字，干扰 ${this.distractions} 次。`;
  }

  reset(): void {
    this.phase = "idle";
    this.elapsedSec = 0;
    this.distractions = 0;
    this.words = 0;
    this.clamped = 0;
  }
}
