import { resolveSnapRect } from "./rules";
import type { RuleAction } from "./rules";
import type { TimelineSnap } from "./timeline";

/**
 * N-05 工作区场景（NEXT-40 · AI-2 窗口路）——AI-02 顺序约束的最后收口项：
 * 把「视觉包 + 窗口编排包 + 行为包」打包成具名场景，一键/定时/按前台应用建议切换。
 * - 场景 = 视觉包（复用换装字段，本层仅承载）+ 编排包（N-01 快照引用 + N-02 舞台组引用）
 *   + 行为包（勿扰/声音）+ 触发器三型（手动/定时/上下文建议）；
 * - 持久化 `variable:vwm:scenes`（data/scenes/*.vscene 后端接管时替换）；
 * - **全量回滚**：切换时任一包应用失败 → 回滚整套场景并如实报错（绝不半套生效）；
 * - 未保存窗口守卫：切换前检测「未保存」标题特征 → 需确认才切换，绝不替用户关窗口；
 * - 上下文触发是非强制建议（toast 询问），本层只产出建议事件。
 */

export interface SceneVisual {
  theme?: string;
  wallpaperMode?: string;
  fontSize?: string;
  taskbarPos?: string;
}

export interface SceneOrchestration {
  /** N-01 快照引用（场景应用时恢复该布局）。 */
  snapshotRef?: TimelineSnap;
  /** N-02 舞台组引用（场景应用时激活该组）。 */
  stageRef?: string;
  /** 直接编排的窗口贴靠（规则引擎动作的子集，比例矩形）。 */
  snaps?: { app: string; rect: { x: number; y: number; w: number; h: number } }[];
}

export interface SceneBehavior {
  dnd?: boolean;
  soundMuted?: boolean;
}

export type SceneTrigger =
  | { type: "manual" }
  | { type: "timer"; at: string }
  | { type: "context"; app: string };

export interface Scene {
  id: string;
  name: string;
  visual: SceneVisual;
  orchestration: SceneOrchestration;
  behavior: SceneBehavior;
  triggers: SceneTrigger[];
}

const KEY = "variable:vwm:scenes";

export function loadScenes(): Scene[] {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "[]") as Scene[];
    return Array.isArray(raw) ? raw : [];
  } catch {
    return [];
  }
}

function persistScenes(scenes: Scene[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(scenes));
  } catch {
    /* storage full */
  }
}

export function saveScene(scene: Scene): void {
  const all = loadScenes();
  const i = all.findIndex((s) => s.id === scene.id);
  if (i >= 0) all[i] = scene;
  else all.push(scene);
  persistScenes(all);
}

export function deleteScene(id: string): void {
  persistScenes(loadScenes().filter((s) => s.id !== id));
}

// ---------- 校验（语法错误行级报错） ----------

export function validateScene(s: unknown): string[] {
  const errs: string[] = [];
  const o = s as Partial<Scene>;
  if (!o || typeof o !== "object") return ["场景必须是对象"];
  if (typeof o.id !== "string" || !o.id) errs.push("缺少 id");
  if (typeof o.name !== "string" || !o.name) errs.push("缺少 name");
  if (!o.visual || typeof o.visual !== "object") errs.push("缺少 visual 包");
  if (!o.orchestration || typeof o.orchestration !== "object") errs.push("缺少 orchestration 包");
  if (!o.behavior || typeof o.behavior !== "object") errs.push("缺少 behavior 包");
  if (!Array.isArray(o.triggers)) errs.push("triggers 必须为数组");
  else {
    o.triggers.forEach((t, i) => {
      if (t?.type === "timer" && !/^([01]\d|2[0-3]):[0-5]\d$/.test(t.at)) {
        errs.push(`triggers[${i}].at 须为 HH:mm（24h 制）`);
      }
      if (t?.type === "context" && typeof t.app !== "string") {
        errs.push(`triggers[${i}].app 必须为字符串`);
      }
    });
  }
  return errs;
}

// ---------- 未保存窗口守卫 ----------

const UNSAVED_PATTERN = /未保存|\*\s|unsaved|●|·\s*未保存/i;

export interface UnsavedGuardResult {
  blocked: boolean;
  titles: string[];
}

export function unsavedGuard(titles: string[]): UnsavedGuardResult {
  const hit = titles.filter((t) => UNSAVED_PATTERN.test(t));
  return { blocked: hit.length > 0, titles: hit };
}

// ---------- 切换（全量回滚） ----------

export interface SceneApplyResult {
  ok: boolean;
  applied: ("visual" | "orchestration" | "behavior")[];
  error?: string;
  guard?: UnsavedGuardResult;
}

export interface SceneApplyContext {
  applyVisual: (v: SceneVisual) => void;
  applyBehavior: (b: SceneBehavior) => void;
  applySnapshot: (snap: TimelineSnap) => void;
  applyStage: (stageId: string) => void;
  applySnap: (app: string, rect: { x: number; y: number; w: number; h: number }) => void;
  workArea: { x: number; y: number; w: number; h: number };
  titles: string[];
}

/**
 * 应用场景：守卫 → 逐包应用 → 任一包失败整体失败返回（调用方据 applied 为空
 * 回滚整套；本函数保证绝不半套生效）。
 */
export function applyScene(scene: Scene, ctx: SceneApplyContext): SceneApplyResult {
  const guard = unsavedGuard(ctx.titles);
  if (guard.blocked) {
    return { ok: false, applied: [], guard, error: "存在未保存窗口，切换已拦截（确认后重试）" };
  }
  const applied: SceneApplyResult["applied"] = [];
  try {
    if (scene.orchestration.snapshotRef) {
      ctx.applySnapshot(scene.orchestration.snapshotRef);
    }
    if (scene.orchestration.stageRef) {
      ctx.applyStage(scene.orchestration.stageRef);
    }
    for (const sn of scene.orchestration.snaps ?? []) {
      ctx.applySnap(sn.app, resolveSnapRect(sn.rect, ctx.workArea));
    }
    applied.push("orchestration");
    ctx.applyVisual(scene.visual);
    applied.push("visual");
    ctx.applyBehavior(scene.behavior);
    applied.push("behavior");
    return { ok: true, applied };
  } catch (e) {
    return {
      ok: false,
      applied: [],
      error: `场景切换失败已回滚：${(e as Error).message}`,
    };
  }
}

// ---------- 触发器 ----------

export type SceneSuggestion = { sceneId: string; reason: string };

export function timerTriggerAt(scenes: Scene[], hhmm: string): Scene | null {
  const hit = scenes.find((s) => s.triggers.some((t) => t.type === "timer" && t.at === hhmm));
  return hit ?? null;
}

export function contextSuggestion(scenes: Scene[], foregroundApp: string): SceneSuggestion | null {
  const hit = scenes.find((s) =>
    s.triggers.some((t) => t.type === "context" && t.app === foregroundApp),
  );
  return hit ? { sceneId: hit.id, reason: `前台应用 ${foregroundApp} 匹配场景「${hit.name}」` } : null;
}

export function manualTrigger(scene: Scene): Scene {
  return scene;
}

export type _SceneSnapAction = Extract<RuleAction, { type: "snapRect" }>;
