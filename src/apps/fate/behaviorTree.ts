/**
 * FTPE 行为树执行器（行为树命运引擎的运行时）。
 * 把"性格如何驱动决策"表达为一棵可实际 tick 的行为树：
 *   composite（顺序/择优/并行/随机/优先级）→ decorator（条件/反向/重试/冷却/
 *   限时/机率/计步/坚韧）→ leaf（稳守本业、放手一搏等词典条目）。
 * 与词典的关系：BEHAVIOR_TREE_NODES 提供节点类型与 bias（标签概率乘子），
 * 本模块负责让这些节点真正"执行"——纯函数、无副作用、完全确定性：
 * 随机/机率节点只消费调用方传入的 rng（mulberry32），同一种子两次 tick
 * 产生完全相同的轨迹（与 engine.ts 的 13.1 可重现性同一契约）。
 */
import { BEHAVIOR_TREE_NODES, type BehaviorTreeNode } from "./dictionaries";

export type BtStatus = "success" | "failure";

/** 跨 tick 的执行记忆：计数器（计步之尺）、上次运行 tick（冷却计时）。 */
export interface BtBlackboard {
  /** 标签 → -100..100 强度（来自人格/性格/主义三库的 trait 值）。 */
  traits: Map<string, number>;
  tick: number;
  counters: Record<string, number>;
  lastRun: Record<string, number>;
  /** 本次 tick 命中的叶节点（按执行顺序），供画布/详情展示。 */
  trace: string[];
}

export interface BtNode {
  /** 稳定 id：blackboard 计数与冷却以此为键。 */
  id: string;
  kind: BtKind;
  name?: string;
  children?: BtNode[];
  /** condition：要达到的标签与阈值（>= threshold 视为满足）。 */
  tag?: string;
  threshold?: number;
  /** retry：最多重试次数；limit：最多成功次数；cooldown/timelimit：tick 数。 */
  retries?: number;
  limit?: number;
  cooldown?: number;
  duration?: number;
  /** probability：0..1 放行概率（由 rng 判定）。 */
  probability?: number;
  /** action：命中后的行为名（与词典条目 name 对应，可为自构行为）。 */
  action?: string;
  /** invert/retry 等装饰的子节点（children[0]）。 */
}

export type BtKind =
  | "sequence" | "selector" | "parallel" | "random" | "priority"
  | "condition" | "invert" | "retry" | "cooldown" | "timelimit"
  | "probability" | "limit" | "guard" | "action";

export interface BtTickResult {
  status: BtStatus;
  /** 依序执行的叶节点名（含失败分支的尝试）。 */
  trace: string[];
}

export function emptyBlackboard(traits: Map<string, number> = new Map()): BtBlackboard {
  return { traits, tick: 0, counters: {}, lastRun: {}, trace: [] };
}

/** 把"标签 → 强度"表折叠进 blackboard（人格/性格/主义的滑块输出）。 */
export function blackboardFromTraits(values: Record<string, number> = {}): BtBlackboard {
  const m = new Map<string, number>();
  for (const [k, v] of Object.entries(values)) m.set(k, v);
  return emptyBlackboard(m);
}

const traitOf = (bb: BtBlackboard, tag: string | undefined): number =>
  tag === undefined ? 0 : bb.traits.get(tag) ?? 0;

function run(node: BtNode, bb: BtBlackboard, rng: () => number): BtStatus {
  const remember = (name: string): void => { bb.trace.push(name); };
  switch (node.kind) {
    case "sequence": {
      for (const c of node.children ?? []) {
        if (run(c, bb, rng) === "failure") return "failure";
      }
      return "success";
    }
    case "priority":
    case "selector": {
      for (const c of node.children ?? []) {
        if (run(c, bb, rng) === "success") return "success";
      }
      return "failure";
    }
    case "parallel": {
      if ((node.children ?? []).length === 0) return "success";
      return (node.children ?? []).every((c) => run(c, bb, rng) === "success") ? "success" : "failure";
    }
    case "random": {
      const kids = node.children ?? [];
      if (kids.length === 0) return "success";
      return run(kids[Math.floor(rng() * kids.length)]!, bb, rng);
    }
    case "condition": {
      const v = traitOf(bb, node.tag);
      return v >= (node.threshold ?? 0) ? "success" : "failure";
    }
    case "invert": {
      const child = node.children?.[0];
      return child && run(child, bb, rng) === "success" ? "failure" : "success";
    }
    case "guard":
    case "action": {
      if (node.id !== "") bb.lastRun[node.id] = bb.tick;
      if (node.action) remember(node.action);
      return "success";
    }
    case "retry": {
      const child = node.children?.[0];
      if (!child) return "success";
      for (let i = 0; i < Math.max(1, node.retries ?? 1); i++) {
        if (run(child, bb, rng) === "success") return "success";
      }
      return "failure";
    }
    case "cooldown": {
      const child = node.children?.[0];
      if (!child) return "success";
      const cd = node.cooldown ?? 1;
      const last = bb.lastRun[node.id];
      if (last !== undefined && bb.tick - last < cd) return "failure";
      const st = run(child, bb, rng);
      if (st === "success") bb.lastRun[node.id] = bb.tick;
      return st;
    }
    case "timelimit": {
      const child = node.children?.[0];
      if (!child) return "success";
      // 预算以"已尝试次数 ≤ duration"近似：超过窗口即认输。
      const tried = bb.counters[`tl:${node.id}`] ?? 0;
      if (tried >= Math.max(1, node.duration ?? 1)) return "failure";
      bb.counters[`tl:${node.id}`] = tried + 1;
      return run(child, bb, rng);
    }
    case "probability": {
      const child = node.children?.[0];
      const p = node.probability ?? 1;
      if (rng() >= p) return "failure";
      return child ? run(child, bb, rng) : "success";
    }
    case "limit": {
      const child = node.children?.[0];
      if (!child) return "success";
      const cap = Math.max(1, node.limit ?? 1);
      const done = bb.counters[node.id] ?? 0;
      if (done >= cap) return "failure";
      const st = run(child, bb, rng);
      if (st === "success") bb.counters[node.id] = done + 1;
      return st;
    }
  }
}

/**
 * 执行一次 tick：推进 bb.tick 并返回状态与轨迹。
 * 行为树"正常实行"的入口——每一推演步调用一次即可。
 */
export function tickBT(root: BtNode, bb: BtBlackboard, rng: () => number): BtTickResult {
  bb.tick += 1;
  bb.trace = [];
  return { status: run(root, bb, rng), trace: [...bb.trace] };
}

// ---------- 默认命运策略树 ----------

/** 阈值：哪个标签越过界限，就走对应的行为分支。 */
const HIGH = 40;
const LOW = -35;

/**
 * 默认命运策略树（与词典条目一一对应，全部为抽象自构概念）：
 * 择优执行 [
 *   顺序执行[ 反向[条件(健康 >= LOW)] → 调养身心 ]  —— 先保命（缺省 0 不触发）
 *   顺序执行[ 条件(冒险 > HIGH) → 放手一搏 ]     —— 冒险者先冲
 *   顺序执行[ 条件(守成 > HIGH) → 稳守本业 ]     —— 守成者先稳
 *   顺序执行[ 条件(家庭 > HIGH) → 守护家人 ]     —— 恋家者先顾家
 *   顺序执行[ 条件(社交 > HIGH) → 广结善缘 ]
 *   顺序执行[ 条件(冲突 > HIGH) → 直面冲突 ]
 *   顺序执行[ 反向开关[条件(觉醒 > HIGH)] → 突破茧壳 ]  —— 觉醒尚浅时逼自己破壳
 *   审时度势                                    —— 兜底：先看清局势
 * ]
 */
export function defaultFatePolicyTree(): BtNode {
  const cond = (tag: string, threshold: number): BtNode => ({ id: `cond:${tag}:${threshold}`, kind: "condition", tag, threshold });
  const act = (action: string): BtNode => ({ id: `act:${action}`, kind: "action", action });
  const branch = (tag: string, threshold: number, action: string): BtNode => ({
    id: `br:${tag}:${threshold}:${action}`,
    kind: "sequence",
    children: [cond(tag, threshold), act(action)],
  });
  return {
    id: "fate:root",
    kind: "selector",
    children: [
      // 先保命：健康跌破 LOW（反向判断——缺省值 0 不应误触发）
      { id: "fate:health", kind: "sequence", children: [{ id: "fate:health-low", kind: "invert", children: [cond("健康", LOW)] }, act("调养身心")] },
      branch("冒险", HIGH, "放手一搏"),
      branch("守成", HIGH, "稳守本业"),
      branch("家庭", HIGH, "守护家人"),
      branch("社交", HIGH, "广结善缘"),
      branch("冲突", HIGH, "直面冲突"),
      { id: "fate:break-shell", kind: "sequence", children: [{ id: "fate:not-awake", kind: "invert", children: [cond("觉醒", HIGH)] }, act("突破茧壳")] },
      { id: "fate:assess", kind: "action", action: "审时度势" },
    ],
  };
}

/** 名字 → 词典条目（供 bias 查询；自定义行为名不在词典则返回 undefined）。 */
export function behaviorByName(name: string): BehaviorTreeNode | undefined {
  return BEHAVIOR_TREE_NODES.find((b) => b.name === name);
}

/**
 * 把本次 tick 选中的行为折叠成"标签 → 概率乘子"函数（与 makeBuffFn 同量纲，
 * 叠乘后钳制 0.05..4）。行为树由此真正参与事件概率的生成：
 * selectBehavior → makeBtBiasFn → engine.eventProbability 的 buffFn 入口。
 */
export function makeBtBiasFn(selected: string[]): (tag: string) => number {
  const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
  return (tag: string): number => {
    let m = 1;
    for (const name of selected) {
      const b = behaviorByName(name);
      const k = b?.bias[tag as keyof BehaviorTreeNode["bias"]];
      if (typeof k === "number") m *= k;
    }
    return clamp(m, 0.05, 4);
  };
}

/**
 * 一步推演的行为决策：用给定 traits 构造 blackboard，tick 默认策略树，
 * 返回命中的行为名与轨迹（确定性：同 traits 同 rng 种子结果一致）。
 */
export function selectBehavior(
  values: Record<string, number>,
  rng: () => number,
  tree: BtNode = defaultFatePolicyTree(),
): BtTickResult {
  const bb = blackboardFromTraits(values);
  const r = tickBT(tree, bb, rng);
  return { status: r.status, trace: r.trace };
}
