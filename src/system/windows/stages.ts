/**
 * N-02 舞台管理器（NEXT-40 · AI-2 窗口路）：
 * 类 macOS Stage Manager 的窗口编组——当前工作的一组窗口居中在「舞台」，
 * 其余窗口按组收拢到左侧「侧幕」，点组即整组上台。
 * - 组（Stage）= VWM 窗口的命名集合（成员可手动增删）；
 * - 持久化 `variable:vwm:stages`（data/stages.json 后端接管时替换），跨重启存活；
 * - 组间轮换 `Ctrl+Shift+←/→`（经键位注册表注册后生效，本层只暴露 API）；
 * - 上台动效走 N-06 编排器（spring 模型），侧幕热区与贴靠热区错开 24px；
 * - 一个组可被 N-05 场景引用（scene.orchestration.stage_ref）。
 */

import type { VwmWin } from "./vwm";

export interface StageGroup {
  id: string;
  name: string;
  /** 成员窗口实例 id 列表。 */
  members: string[];
}

const KEY = "variable:vwm:stages";
/** 侧幕热区与贴靠热区错开距离（px，进 tokens 前先以常量约束）。 */
export const STAGE_HOTZONE_OFFSET_PX = 24;

export function loadStages(): StageGroup[] {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "[]") as StageGroup[];
    return Array.isArray(raw) ? raw.filter((g) => g && typeof g.id === "string" && Array.isArray(g.members)) : [];
  } catch {
    return [];
  }
}

function persistStages(groups: StageGroup[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(groups));
  } catch {
    /* storage full → 本次不持久 */
  }
}

let nextSeq = 0;

/** 创建命名组（空组也允许——先建组后拖窗入组）。 */
export function createStage(name: string): StageGroup {
  const groups = loadStages();
  const g: StageGroup = { id: `stage-${Date.now().toString(36)}-${nextSeq++}`, name, members: [] };
  groups.push(g);
  persistStages(groups);
  return g;
}

/** 删除组（成员窗口本身不受影响）。 */
export function deleteStage(id: string): void {
  persistStages(loadStages().filter((g) => g.id !== id));
}

/** 重命名组。 */
export function renameStage(id: string, name: string): void {
  persistStages(loadStages().map((g) => (g.id === id ? { ...g, name } : g)));
}

/** 把窗口加入组（一个窗口同时只属一个舞台组——入组即从其它组移除）。 */
export function addToStage(stageId: string, winId: string): void {
  const groups = loadStages().map((g) => {
    const members = g.members.filter((m) => m !== winId);
    if (g.id === stageId && !members.includes(winId)) members.push(winId);
    return { ...g, members };
  });
  persistStages(groups);
}

/** 把窗口移出组。 */
export function removeFromStage(stageId: string, winId: string): void {
  persistStages(
    loadStages().map((g) => (g.id === stageId ? { ...g, members: g.members.filter((m) => m !== winId) } : g)),
  );
}

/** 解散组。 */
export function disbandStage(id: string): void {
  deleteStage(id);
}

export interface StageActivatePlan {
  /** 上台成员（聚焦/恢复顺序：按成员列表序）。 */
  focusOrder: string[];
  /** 非成员窗口（收拢到侧幕：最小化）。 */
  sideline: string[];
}

/**
 * 生成「整组上台」计划：成员全部上台（取消最小化 + 聚焦），非成员最小化收拢。
 * 纯计算——调用方（侧幕气泡双击 / 键盘轮换 / 场景切换）负责应用回 VWM store。
 */
export function planStageActivate(stageId: string, wins: VwmWin[]): StageActivatePlan {
  const group = loadStages().find((g) => g.id === stageId);
  if (!group) return { focusOrder: [], sideline: [] };
  const memberSet = new Set(group.members);
  const focusOrder = group.members.filter((id) => wins.some((w) => w.id === id));
  const sideline = wins.filter((w) => !memberSet.has(w.id)).map((w) => w.id);
  return { focusOrder, sideline };
}

/** 组间轮换：给定当前激活组 id 与方向，返回下一组 id（不循环——到端点返回 null，行为可预期）。 */
export function nextStageId(currentId: string | null, backward = false): string | null {
  const groups = loadStages();
  if (groups.length === 0) return null;
  const idx = currentId ? groups.findIndex((g) => g.id === currentId) : -1;
  const next = idx < 0 ? (backward ? groups.length - 1 : 0) : idx + (backward ? -1 : 1);
  if (next < 0 || next >= groups.length) return null;
  return groups[next].id;
}
