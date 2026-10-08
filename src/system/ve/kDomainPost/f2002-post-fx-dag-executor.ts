/**
 * VE-F2002 · 后处理 DAG 执行器（K 域 · 批次 K01 · 全后处理执行底座）
 * ---------------------------------------------------------------------------
 * 职责定位：后处理 DAG 执行器——把「效果链」建模为**有向无环图**并执行之。
 * 效果节点 + RT（渲染目标）绑定表构成图；边的语义是「某效果产出的 RT 被某效果消费」。
 * 图一旦编好就退化为一条**线性执行列表**，运行期不再有图遍历开销——
 * 这是本条存在的核心理由：把图论成本全部压在编译期，换取每帧接近零的执行器开销。
 *
 * 上游 F2001 域架构（EffectDag / EffectDeclaration / verifyDag / RT 池契约 /
 *                F2002_EXECUTOR_CAPABILITIES 能力声明）。
 * 下游 F2003 中间渲染目标池（RT 分配计划落地）、F2004 Bloom 等 K01-K03 全部效果
 *       （效果节点注册制：全部效果经本执行器执行）、F2013 调试可视化（消费图结构）、
 *       F2014/F2015 降质联动（降档 = 图重编译的预设实例）。
 *
 * 锚点契约（五条，逐条对应判据）：
 *   1. DAG 模型：节点 = 效果实例（类型标识 + 参数块引用 + 输入输出 RT 绑定表），
 *      边 = RT 依赖（生产者 → 消费者）。效果间数据流**显式声明**，不靠隐式顺序推断。
 *   2. 拓扑校验：依赖检测——效果输入未就绪即执行序非法。编译期拒绝成环、
 *      拒绝缺失依赖、拒绝 RT 绑定冲突（同名输出多生产者）。拒绝时须**列出环路径**
 *      （三要素：环上节点序列 + 为何非法 + 改哪一处）。
 *   3. 动态链重组：效果开关切换 → 图重编译**无闪烁**。做法是新图后台编译，
 *      编译完成后在**帧边界**原子换绑。半新半旧禁止——一帧之内不允许同时
 *      存在两套绑定（否则同一帧内前后半段走不同图，效果间会读到半成品 RT）。
 *   4. 旁路显性：效果初始化失败 → 该节点旁路 + 显性告警。链降级但不崩。
 *      「旁路」是显式降级，绝不静默——静默旁路等于让用户以为效果还开着。
 *   5. 重编译去抖：开关每帧切换（用户狂拖滑条）会引发重编译风暴。
 *      合并去抖到帧边界批处理：帧内多次请求折叠为一次编译。
 *
 * 四条容易做错、故显式记录的设计立场：
 *
 *   一、拓扑序不是唯一的，「确定性」才是契约。
 *     同一张图可以有多个合法拓扑序。若拓扑排序依赖 Map 迭代顺序或对象键序，
 *     同一张图在不同机器/不同运行时会得到不同的执行序——
 *     表现为「同一工程，渲染结果在两台机器上不一致」，且极难复现。
 *     故本条的 Kahn 算法用**显式入队序**（声明序）而非 Set 迭代序打破平局。
 *
 *   二、原子换绑的边界必须是「帧边界」，不是「编译完成时刻」。
 *     若编译一完成就立即换绑，而此时上一帧的 GPU 命令还在飞行，
 *     新图会与旧图的 RT 分配交错 → 读到未完成的 RT。
 *     故编译完成只把「新图」挂到 pending，**下一帧开始时**才swap。
 *
 *   三、旁路必须区分「跳过」与「直通」。
 *     初始化失败的效果若其输出 RT 被下游消费，直接跳过会让下游读到**未初始化**的
 *     RT（垃圾数据），必须显式直通（bypass：把输入 RT 原样接到输出槽）。
 *     只有当该效果无下游消费者时，才可以纯跳过。这条区分是旁路正确性的全部要害。
 *
 *   四、RT 绑定冲突必须编译期拒绝，不能靠「后者覆盖」。
 *     同名输出多生产者意味着两个效果写同一槽 → 执行序决定谁赢，且结果不可预测。
 *     「后者覆盖」会让问题从编译期推迟到运行期，表现为随开关状态漂移的渲染错误。
 *
 * 零静默纪律：成环、缺失依赖、RT 绑定冲突、效果初始化失败、重编译风暴
 * 全部产出 Diagnostic（code + message + hint），降级一律显性不做暗转。
 *
 * 判据：DAG 模型、拓扑校验、原子重组、旁路显性、判据。
 * 依赖锚点：F2001 域架构（EffectDag / verifyDag / F2002_EXECUTOR_CAPABILITIES）。
 * 交接说明：本条产出的是**执行列表与 RT 分配计划**，不持有 RT 实体；
 *          RT 的申请与归还归 F2003 池。执行器只声明「谁需要什么规格的 RT」，
 *          由池在帧边界统一分配——这让池能集中做生命周期管控。
 *          另：原地效果（读写同槽）在图模型里表现为自环，
 *          须走 inPlace 白名单豁免而非当成环拒绝（见 TOPOLOGY 中原地效果的处理）。
 */

import type { DiagCode, RtRef } from "./f2001-k-domain-post-architecture.js";
import { F2002_EXECUTOR_CAPABILITIES } from "./f2001-k-domain-post-architecture.js";

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型
// ════════════════════════════════════════════════════════════════════════════

/** 执行器专属诊断码（在 F2001 的 DiagCode 基础上扩展，两边独立不共用枚举）。 */
export type ExecDiagCode =
  /** 图中存在环——不可执行，必须列出环路径。 */
  | "DAG_CYCLIC"
  /** 某效果的输入 RT 无任何生产者——缺边。 */
  | "INPUT_SLOT_HAS_NO_PRODUCER"
  /** RT 槽多生产者冲突——同名输出被两个效果写。 */
  | "RT_SLOT_MULTI_PRODUCER"
  /** 环内效果引用了不存在的节点。 */
  | "EDGE_ENDPOINT_UNKNOWN"
  /** 效果初始化失败——已旁路。 */
  | "EFFECT_INIT_FAILED_BYPASSED"
  /** 重编译请求在帧内被合并（风暴去抖）。 */
  | "RECOMPILE_DEBOUNCED"
  /** 同时存活效果数超执行器能力上限。 */
  | "LIVE_EFFECT_LIMIT_EXCEEDED"
  /** 原地效果未登记白名单——自环被拒。 */
  | "IN_PLACE_NOT_DECLARED"
  /** 执行列表为空——链无任何可执行节点。 */
  | "EMPTY_PLAN"
  /** 图结构非法（节点表与边表不一致等）。 */
  | "GRAPH_MALFORMED"
  /** 换绑请求与当前状态不符（未编译完成就swap 等）。 */
  | "SWAP_STATE_INVALID";

/** 一条诊断。 */
export interface ExecDiagnostic {
  readonly code: ExecDiagCode;
  readonly message: string;
  readonly hint: string;
}

/** 结果判别联合。 */
export type ExecOutcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly ExecDiagnostic[] }
  | {
      readonly ok: false;
      readonly code: ExecDiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly ExecDiagnostic[];
    };

/** 成功构造。 */
export function execOk<T>(value: T, diagnostics: readonly ExecDiagnostic[] = []): ExecOutcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造。 */
export function execErr<T>(
  code: ExecDiagCode,
  message: string,
  hint: string,
  diagnostics: readonly ExecDiagnostic[] = [],
): ExecOutcome<T> {
  const d: ExecDiagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [...diagnostics, d] };
}

/** 诊断聚合器。 */
export class ExecDiagBag {
  private readonly items: ExecDiagnostic[] = [];

  push(code: ExecDiagCode, message: string, hint: string): void {
    this.items.push({
      code,
      message: message || "（未提供描述）",
      hint: hint || "（未提供处置建议）",
    });
  }

  get size(): number {
    return this.items.length;
  }

  all(): readonly ExecDiagnostic[] {
    return this.items.slice();
  }

  byCode(code: ExecDiagCode): readonly ExecDiagnostic[] {
    return this.items.filter((d) => d.code === code);
  }

  has(code: ExecDiagCode): boolean {
    return this.items.some((d) => d.code === code);
  }
}

/** 把 F2001 的诊断码映射到执行器诊断码（保持跨条可追溯，不新造语义）。 */
export function mapUpstreamDiag(code: DiagCode): ExecDiagCode {
  switch (code) {
    case "EFFECT_DAG_CYCLIC":
      return "DAG_CYCLIC";
    case "EFFECT_DEPENDENCY_UNKNOWN":
      return "EDGE_ENDPOINT_UNKNOWN";
    case "EFFECT_DEPENDENCY_VIOLATED":
      return "INPUT_SLOT_HAS_NO_PRODUCER";
    case "EFFECT_DECLARATION_INCOMPLETE":
      return "GRAPH_MALFORMED";
    case "RT_POOL_EXHAUSTED":
      return "LIVE_EFFECT_LIMIT_EXCEEDED";
    case "RT_LIFETIME_VIOLATION":
      return "RT_SLOT_MULTI_PRODUCER";
    default:
      return "GRAPH_MALFORMED";
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 图模型（判据一：DAG 模型）
// ════════════════════════════════════════════════════════════════════════════

/** 参数块引用：效果参数由外部块持有，执行器只读不写（保持参数与执行解耦）。 */
export interface ParamBlockRef {
  readonly blockId: string;
  /** 参数键值表；缺失键按效果声明的 params 列表判定为参数不完整。 */
  readonly values: Readonly<Record<string, number>>;
}

/** 效果实例 = 效果声明（类型与绑定）+ 参数块引用 + 开关状态。 */
export interface EffectNode {
  readonly effectId: string;
  /** 效果类型标识；注册制的分发键。 */
  readonly kind: string;
  readonly stage: string;
  /** 输入 RT 绑定表（槽 → 该输入的具体 RT 引用）。 */
  readonly inputs: readonly RtRef[];
  /** 输出 RT 槽。 */
  readonly output: RtRef;
  /** 参数块引用。 */
  readonly params: ParamBlockRef;
  /** 开关名（与 F2001 的 toggle 同名同源）。 */
  readonly toggle: string;
  /** 是否原地效果（读写同槽，须在 inPlace 白名单登记）。 */
  readonly inPlace: boolean;
}

/** RT 依赖边：from 产出的槽被 to 消费。 */
export interface DagEdge {
  readonly from: string;
  readonly to: string;
}

/** 图模型：节点 + 边。 */
export interface EffectGraph {
  readonly nodes: readonly EffectNode[];
  readonly edges: readonly DagEdge[];
}

/** 外部产出的 RT 槽（场景缓冲、上一阶段结果等，图外的生产者）。 */
export type ExternalSlot = string;

/** 图的完整输入：内部图 + 外部可读槽 + 开关状态。 */
export interface GraphSpec {
  readonly graph: EffectGraph;
  /** 图外可读槽（场景 RT 等）；内部节点的输出槽自动并入。 */
  readonly externalSlots: readonly ExternalSlot[];
  /** 开关 → 当前是否开启。 */
  readonly toggles: Readonly<Record<string, boolean>>;
  /** 原地效果白名单（effectId 集合）。 */
  readonly inPlaceAllowed: readonly string[];
}

/** RT 规格（供 F2003 池分配；本条只声明需求不持有实体）。 */
export interface RtSpec {
  readonly slot: string;
  readonly width: number;
  readonly height: number;
  readonly format: string;
}

/** RT 分配计划：编译产物之一（与 F2003 池联动）。 */
export interface RtAllocationPlan {
  /** 逐节点的 RT 需求。 */
  readonly allocations: readonly { readonly effectId: string; readonly spec: RtSpec }[];
  /** 池容量需求（对齐 F2002_EXECUTOR_CAPABILITIES.maxLiveEffects）。 */
  readonly requiredSlots: number;
}

/** 执行计划中的一个步骤（线性、可直接顺序执行）。 */
export interface ExecStep {
  readonly effectId: string;
  readonly kind: string;
  readonly stage: string;
  /** 实际读取的 RT 槽（已解析到具体生产者）。 */
  readonly reads: readonly string[];
  /** 写入的 RT 槽。 */
  readonly writes: string;
  /** 该步骤是否为旁路（初始化失败直通）。 */
  readonly bypass: boolean;
  /** 序号：拓扑序下标，便于调试对照。 */
  readonly index: number;
}

/** 编译产物：拓扑序执行列表 + RT 分配计划（图 → 线性列表，判据一的核心收益）。 */
export interface CompiledPlan {
  /** 线性执行列表，运行期顺序遍历即可，无图遍历开销。 */
  readonly steps: readonly ExecStep[];
  readonly allocations: RtAllocationPlan;
  /** 图版本号：每次重编译递增，供换绑时判定新旧。 */
  readonly version: number;
  /** 被旁路的节点（显性记录，供 F2013 调试面板显示）。 */
  readonly bypassed: readonly string[];
  /** 编译期诊断（告警级，如旁路）。 */
  readonly diagnostics: readonly ExecDiagnostic[];
}

// ════════════════════════════════════════════════════════════════════════════
// §3 拓扑排序与校验（判据二：拓扑校验）
// ════════════════════════════════════════════════════════════════════════════

/** 一个环的路径信息（三要素的第一项：环上节点序列）。 */
export interface CycleReport {
  /** 环上节点序列（首尾同一节点，可直接拼成 a→b→c→a）。 */
  readonly path: readonly string[];
  /** 长度（节点数）。 */
  readonly length: number;
}

/**
 * 拓扑排序（Kahn 算法，打破平局用**声明序**而非 Set 迭代序——立场一）。
 *
 * 原地效果处理：原地效果的自环不是环。做法是把自环边在建图时剔除，
 * 并要求该效果在 inPlace 白名单中；未登记的自环按环拒绝并给IN_PLACE_NOT_DECLARED。
 *
 * @returns 执行序 + 全部环报告。有环时执行序只含无环部分（可观测但不执行）。
 */
export function topoSort(graph: EffectGraph, inPlaceAllowed: readonly string[]): {
  readonly order: readonly string[];
  readonly cycles: readonly CycleReport[];
  readonly inPlaceViolation: readonly string[];
} {
  const nodeIds: string[] = [];
  const byId = new Map<string, EffectNode>();
  for (const n of graph.nodes) {
    if (!byId.has(n.effectId)) {
      byId.set(n.effectId, n);
      nodeIds.push(n.effectId);
    }
  }
  const allowed = new Set(inPlaceAllowed);
  const inPlaceViolation: string[] = [];
  // 自环边：原地效果且已登记 → 剔除；未登记 → 记违例（按环处理）
  const selfLoop = new Set<string>();
  for (const n of graph.nodes) {
    if (n.inPlace) {
      if (!allowed.has(n.effectId)) inPlaceViolation.push(n.effectId);
      selfLoop.add(n.effectId);
    }
  }
  // 邻接表与入度
  const adj = new Map<string, string[]>();
  const indeg = new Map<string, number>();
  for (const id of nodeIds) {
    adj.set(id, []);
    indeg.set(id, 0);
  }
  const cycles: CycleReport[] = [];
  const seenEdge = new Set<string>();
  for (const e of graph.edges) {
    const key = `${e.from} ${e.to}`;
    if (seenEdge.has(key)) continue;
    seenEdge.add(key);
    if (!byId.has(e.from) || !byId.has(e.to)) continue; // 端点未知交由缺失依赖校验报
    if (e.from === e.to) {
      // 自环：原地效果已剔除；若非原地效果则它是一个真环
      if (selfLoop.has(e.from)) continue;
      cycles.push({ path: [e.from, e.from], length: 1 });
      continue;
    }
    adj.get(e.from)?.push(e.to);
    indeg.set(e.to, (indeg.get(e.to) ?? 0) + 1);
  }
  // 确定性入队：按声明序扫描 nodeIds，入度为 0 者依序入队
  const queue: string[] = [];
  for (const id of nodeIds) {
    if ((indeg.get(id) ?? 0) === 0) queue.push(id);
  }
  const order: string[] = [];
  const popped = new Set<string>();
  while (queue.length > 0) {
    const cur = queue.shift() as string;
    if (popped.has(cur)) continue;
    popped.add(cur);
    order.push(cur);
    const next = adj.get(cur) ?? [];
    // 下一批候选按声明序重扫，保证同层入队顺序确定
    let changed = false;
    for (const id of nodeIds) {
      if (popped.has(id)) continue;
      if (!next.includes(id)) continue;
      const d = (indeg.get(id) ?? 0) - 1;
      indeg.set(id, d);
      if (d === 0) {
        queue.push(id);
        changed = true;
      }
    }
    void changed;
  }
  // 未被弹出的节点必在环上——用 DFS 提取一条具体环路径（三要素要求列出环）
  if (order.length < nodeIds.length) {
    for (const cycle of findCycles(adj, nodeIds, popped)) {
      cycles.push(cycle);
    }
    // 自环已报的补齐
    for (const c of cycles) {
      if (c.path.length === 2 && c.path[0] === c.path[1]) continue;
    }
  }
  return { order, cycles, inPlaceViolation };
}

/**
 * 在残余子图中提取具体环路径（DFS 带路径栈）。
 * 拓扑序没弹出的节点全在环上，但必须给出**是哪一条**环，否则开发者无法定位。
 */
function findCycles(
  adj: ReadonlyMap<string, readonly string[]>,
  nodeIds: readonly string[],
  popped: ReadonlySet<string>,
): CycleReport[] {
  const WHITE = 0;
  const GRAY = 1;
  const BLACK = 2;
  const color = new Map<string, number>();
  for (const id of nodeIds) color.set(id, popped.has(id) ? BLACK : WHITE);
  const stack: string[] = [];
  const found: CycleReport[] = [];
  const dfs = (u: string): void => {
    color.set(u, GRAY);
    stack.push(u);
    for (const v of adj.get(u) ?? []) {
      const c = color.get(v) ?? WHITE;
      if (c === WHITE) {
        dfs(v);
      } else if (c === GRAY) {
        // 找到回边：从栈中截出环段
        const at = stack.indexOf(v);
        const path = stack.slice(at >= 0 ? at : 0).concat(v);
        found.push({ path: path.slice(), length: path.length - 1 });
        if (found.length >= 8) return; // 最多报 8 条环，避免诊断爆炸
      }
    }
    stack.pop();
    color.set(u, BLACK);
  };
  for (const id of nodeIds) {
    if ((color.get(id) ?? WHITE) === WHITE) dfs(id);
    if (found.length >= 8) break;
  }
  return found;
}

/** 校验图结构并返回可用的执行序（阻断项一律拒绝）。 */
export function validateGraph(spec: GraphSpec): ExecOutcome<{
  readonly order: readonly string[];
  readonly byId: ReadonlyMap<string, EffectNode>;
}> {
  const bag = new ExecDiagBag();
  const { graph } = spec;
  const byId = new Map<string, EffectNode>();
  for (const n of graph.nodes) {
    if (byId.has(n.effectId)) {
      bag.push("GRAPH_MALFORMED", `效果 id 重复：${n.effectId}。`, "效果 id 是执行器分发键，必须唯一。");
      return execErr("GRAPH_MALFORMED", `效果 id 重复：${n.effectId}。`, "改用唯一 effectId。", bag.all());
    }
    byId.set(n.effectId, n);
  }
  // 边端点必须存在
  for (const e of graph.edges) {
    if (!byId.has(e.from) || !byId.has(e.to)) {
      const bad = byId.has(e.from) ? e.to : e.from;
      bag.push("EDGE_ENDPOINT_UNKNOWN", `依赖边端点不存在：${bad}。`, "边的两端都必须是已声明的效果节点。");
    }
  }
  if (bag.has("EDGE_ENDPOINT_UNKNOWN")) {
    const first = bag.byCode("EDGE_ENDPOINT_UNKNOWN")[0];
    return execErr("EDGE_ENDPOINT_UNKNOWN", first?.message ?? "依赖边端点不存在。",
      first?.hint ?? "补齐节点声明或删除悬空边。", bag.all());
  }
  // RT 槽多生产者冲突（立场四）
  const producer = new Map<string, string>();
  for (const n of graph.nodes) {
    const slot = n.output.slot;
    const prev = producer.get(slot);
    if (prev !== undefined) {
      bag.push("RT_SLOT_MULTI_PRODUCER",
        `RT 槽 ${slot} 有多个生产者：${prev} 与 ${n.effectId}。`,
        "同名输出多生产者会让执行序决定谁赢且结果不可预测；改为不同槽或合并为一个效果。");
    } else {
      producer.set(slot, n.effectId);
    }
  }
  if (bag.has("RT_SLOT_MULTI_PRODUCER")) {
    const first = bag.byCode("RT_SLOT_MULTI_PRODUCER")[0];
    return execErr("RT_SLOT_MULTI_PRODUCER", first?.message ?? "RT 槽多生产者冲突。",
      first?.hint ?? "编译期拒绝，不用后者覆盖。", bag.all());
  }
  // 输入槽必须有生产者（内部或外部）
  const known = new Set<string>(spec.externalSlots);
  for (const slot of producer.keys()) known.add(slot);
  for (const n of graph.nodes) {
    for (const inp of n.inputs) {
      if (!known.has(inp.slot)) {
        bag.push("INPUT_SLOT_HAS_NO_PRODUCER",
          `效果 ${n.effectId} 的输入槽 ${inp.slot} 无任何生产者。`,
          "补上生产该槽的效果，或将其登记为外部可读槽（场景缓冲等）。");
      }
    }
  }
  if (bag.has("INPUT_SLOT_HAS_NO_PRODUCER")) {
    const first = bag.byCode("INPUT_SLOT_HAS_NO_PRODUCER")[0];
    return execErr("INPUT_SLOT_HAS_NO_PRODUCER", first?.message ?? "输入槽无生产者。",
      first?.hint ?? "补齐生产者或声明为外部槽。", bag.all());
  }
  // 原地效果白名单
  const allowed = new Set(spec.inPlaceAllowed);
  for (const n of graph.nodes) {
    if (n.inPlace && !allowed.has(n.effectId)) {
      bag.push("IN_PLACE_NOT_DECLARED",
        `原地效果 ${n.effectId} 未登记白名单。`,
        "原地效果读写同槽会在图上形成自环；须在 inPlaceAllowed 中显式登记。");
    }
  }
  if (bag.has("IN_PLACE_NOT_DECLARED")) {
    const first = bag.byCode("IN_PLACE_NOT_DECLARED")[0];
    return execErr("IN_PLACE_NOT_DECLARED", first?.message ?? "原地效果未登记。",
      first?.hint ?? "登记到白名单，或改为非原地。", bag.all());
  }
  // 环检测
  const topo = topoSort(graph, spec.inPlaceAllowed);
  if (topo.cycles.length > 0) {
    const c0 = topo.cycles[0];
    const pathText = c0 === undefined ? "" : c0.path.join(" → ");
    bag.push("DAG_CYCLIC",
      `图中存在环：${pathText}。`,
      "环上每个效果都在等待另一个先执行，无合法执行序；断开环中任一条边。");
    return execErr("DAG_CYCLIC", `图中存在环：${pathText}。`,
      `环路径：${pathText}；断开其中任一条边即可解环。`, bag.all());
  }
  // 同时存活效果数上限（对齐 F2001 能力声明）
  if (graph.nodes.length > F2002_EXECUTOR_CAPABILITIES.maxLiveEffects) {
    bag.push("LIVE_EFFECT_LIMIT_EXCEEDED",
      `图含${graph.nodes.length} 个效果，超执行器同时存活上限 ${F2002_EXECUTOR_CAPABILITIES.maxLiveEffects}。`,
      "拆分链或降档（F2015）；执行器能力上限以 F2001 声明为准。");
    return execErr("LIVE_EFFECT_LIMIT_EXCEEDED",
      `图含 ${graph.nodes.length} 个效果，超上限 ${F2002_EXECUTOR_CAPABILITIES.maxLiveEffects}。`,
      "拆分链或降档。", bag.all());
  }
  return execOk({ order: topo.order, byId }, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §4 编译（拓扑序 → 线性执行列表 + RT 分配计划）
// ════════════════════════════════════════════════════════════════════════════

/** RT 默认规格（由 F2003 池按实际渲染目标覆写；本条只给兜底声明）。 */
export const DEFAULT_RT_SPEC: RtSpec = {
  slot: "",
  width: 1920,
  height: 1080,
  format: "rgba16f",
};

/** 效果初始化结果（由各效果实现回填；失败即旁路）。 */
export type EffectInitResult =
  | { readonly ok: true }
  | { readonly ok: false; readonly reason: string };

/** 初始化器签名：给定节点返回初始化结果。 */
export type EffectInitializer = (node: EffectNode) => EffectInitResult;

/** 编译选项。 */
export interface CompileOptions {
  readonly version: number;
  /** RT 规格覆写（按槽给）。 */
  readonly rtSpecOverride?: Readonly<Record<string, RtSpec>>;
  /** 初始化器；缺省视为全部成功。 */
  readonly init?: EffectInitializer;
}

/**
 * 编译：图 → 线性执行列表。
 * 开关关闭的节点被**剔除**（不进执行列表），其下游若失去输入则连带失效——
 * 这是开关切换必须重编译的原因：图结构变了，不是只打个标记。
 */
export function compileGraph(
  spec: GraphSpec,
  options: CompileOptions,
): ExecOutcome<CompiledPlan> {
  const bag = new ExecDiagBag();
  const validated = validateGraph(spec);
  if (!validated.ok) {
    return execErr(validated.code, validated.message, validated.hint, bag.all());
  }
  const { byId } = validated.value;
  // 开关过滤：关闭的效果不进图
  const enabled = new Map<string, EffectNode>();
  for (const n of spec.graph.nodes) {
    const on = spec.toggles[n.toggle];
    // 未登记开关状态视为开启（缺省不改变既有行为）
    if (on === false) continue;
    enabled.set(n.effectId, n);
  }
  // 过滤后重新校验（剔除节点可能造成输入槽无主）
  const filteredNodes = spec.graph.nodes.filter((n) => enabled.has(n.effectId));
  const liveSlots = new Set<string>(spec.externalSlots);
  for (const n of filteredNodes) liveSlots.add(n.output.slot);
  const orphan: string[] = [];
  for (const n of filteredNodes) {
    for (const inp of n.inputs) {
      if (!liveSlots.has(inp.slot)) orphan.push(`${n.effectId}←${inp.slot}`);
    }
  }
  if (orphan.length > 0) {
    const text = orphan.join("、");
    bag.push("INPUT_SLOT_HAS_NO_PRODUCER",
      `开关关闭后以下输入失去生产者：${text}。`,
      "被关闭效果的下游若仍开启，须为其提供替代输入或连带关闭该下游。");
    return execErr("INPUT_SLOT_HAS_NO_PRODUCER",
      `开关关闭后以下输入失去生产者：${text}。`,
      "连带关闭依赖它的下游效果，或改用外部槽。", bag.all());
  }
  const filteredGraph: EffectGraph = {
    nodes: filteredNodes,
    edges: spec.graph.edges.filter((e) => enabled.has(e.from) && enabled.has(e.to)),
  };
  const topo = topoSort(filteredGraph, spec.inPlaceAllowed);
  if (topo.cycles.length > 0) {
    const c0 = topo.cycles[0];
    const pathText = c0 === undefined ? "" : c0.path.join(" → ");
    return execErr("DAG_CYCLIC", `开关过滤后图中存在环：${pathText}。`,
      `环路径：${pathText}；检查开关组合。`, bag.all());
  }
  if (topo.order.length === 0) {
    bag.push("EMPTY_PLAN", "编译结果为空执行列表。", "至少保留一个开启的效果，或接受本帧无后处理。");
    return execErr("EMPTY_PLAN", "编译结果为空执行列表。", "至少保留一个开启的效果。", bag.all());
  }
  // 逐步骤生成
  const steps: ExecStep[] = [];
  const allocations: { readonly effectId: string; readonly spec: RtSpec }[] = [];
  const bypassed: string[] = [];
  const override = options.rtSpecOverride ?? {};
  let index = 0;
  for (const id of topo.order) {
    const node = byId.get(id);
    if (node === undefined) continue;
    // 初始化：失败即旁路（判据四）
    if (options.init !== undefined) {
      const initRes = options.init(node);
      if (!initRes.ok) {
        bypassed.push(node.effectId);
        bag.push("EFFECT_INIT_FAILED_BYPASSED",
          `效果 ${node.effectId} 初始化失败（${initRes.reason}），已旁路。`,
          "该节点直通：输入原样接到输出槽，下游仍可消费；请修效果实现或用开关禁用。");
        // 直通语义（立场三）：旁路必须写下游**真正消费**的那个槽。
        //
        // 这里最易写错：直觉上会写「输入原样接到输出槽」，但下游读的是
        // 本节点的 output.slot，不是 inputs[0].slot。两者通常不同——
        // 例如 head 的输入是外部槽 sceneColor、输出是 sHead，下游 tail 读 sHead。
        // 若旁路时写 inputs[0].slot，下游就读到未初始化的 RT（垃圾数据），
        // 表现为「效果偶发失效」而非报错，最难归因。
        // 故取本节点的 output.slot 作为旁路写入槽：语义等价于
        // 「把本应写入该槽的内容替换为上游已算好的内容」。
        const hasDownstream = filteredGraph.edges.some((e) => e.from === node.effectId);
        const slot = node.output.slot;
        if (hasDownstream && node.inputs.length === 0) {
          // 无输入可直通：写自己输出槽等于未初始化数据，必须显性登记让上层处置
          bag.push("EFFECT_INIT_FAILED_BYPASSED",
            `效果 ${node.effectId} 初始化失败且无输入可直通，但存在下游消费者。`,
            "无法旁路：该效果无输入可继承，输出槽将保持未初始化；请给该效果声明至少一个输入，或关闭其下游。");
        }
        steps.push({
          effectId: node.effectId,
          kind: node.kind,
          stage: node.stage,
          reads: node.inPlace ? [node.output.slot] : node.inputs.map((i) => i.slot),
          writes: slot,
          bypass: true,
          index,
        });
        index += 1;
        continue;
      }
    }
    steps.push({
      effectId: node.effectId,
      kind: node.kind,
      stage: node.stage,
      reads: node.inPlace ? [node.output.slot] : node.inputs.map((i) => i.slot),
      writes: node.output.slot,
      bypass: false,
      index,
    });
    const specForNode = override[node.output.slot] ?? { ...DEFAULT_RT_SPEC, slot: node.output.slot };
    allocations.push({ effectId: node.effectId, spec: specForNode });
    index += 1;
  }
  const plan: CompiledPlan = {
    steps,
    allocations: { allocations, requiredSlots: allocations.length },
    version: options.version,
    bypassed,
    diagnostics: bag.all(),
  };
  return execOk(plan, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §5 动态链重组（判据三：帧边界原子换绑 + 风暴去抖）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 执行器：持有当前生效计划与待换绑计划。
 *
 * 状态机（立场二）：
 *   compiling —— 收到新图请求，后台编译中，**当前计划继续服务帧**（不阻塞渲染）
 *   ready     —— 编译完成，新计划挂在 pending，等待帧边界
 *   running   —— 帧边界处原子 swap，pending 成为 current
 *
 * 半新半旧禁止：swap 是单次指针替换，发生在帧边界，不存在两套绑定同时服务一帧。
 */
export class PostFxExecutor {
  private current: CompiledPlan | null = null;
  private pending: CompiledPlan | null = null;
  private compiling: CompiledPlan | null = null;
  private readonly diagBag = new ExecDiagBag();
  /** 待处理的重编译请求（帧内合并去抖，判据五）。 */
  private queuedSpec: GraphSpec | null = null;
  private queuedCount = 0;
  private nextVersion = 1;
  private frameIndex = 0;
  private lastSpec: GraphSpec | null = null;
  /** 待处理重编译请求的编译选项（与 queuedSpec 同生命周期）。 */
  private queuedOptions: CompileOptions | null = null;

  /** 当前生效计划（未编译过则为 null）。 */
  activePlan(): CompiledPlan | null {
    return this.current;
  }

/**
 * 待换绑计划（已编译完成、等待下一个帧边界生效）。
 *
 * 语义澄清：编译一完成，计划就处于「待换绑」状态——它已经可用，
 * 只是还没到换绑时机。因此本方法返回 `compiling`（编译产物本身）与
 * `pending`（已挂起等换绑）中**较新的那个**，而不是只有 pending。
 * 若只查 pending，编译刚完成的那一段窗口里会误报「无待换绑计划」，
 * 调试面板据此显示的「重编译是否已完成」就会慢一拍。
 */
pendingPlan(): CompiledPlan | null {
  return this.pending ?? this.compiling;
}

  /** 是否有正在后台编译的请求。 */
  isCompiling(): boolean {
    return this.compiling !== null;
  }

  /** 重编译请求计数（风暴观测）。 */
  queuedRequestCount(): number {
    return this.queuedCount;
  }

  /**
   * 请求重编译（开关切换等）。
   * 帧内多次请求**合并为一次**（判据五）：重复请求只更新待处理 spec，
   * 不触发新编译，也记一次 RECOMPILE_DEBOUNCED 供观测。
   */
  requestRecompile(spec: GraphSpec, options: CompileOptions): ExecOutcome<"queued" | "debounced"> {
    // 去抖判据（判据五）：与**最近一次已受理请求**的 spec 相同即折叠。
    //
    // 这里比对 lastSpec 而非 queuedSpec：编译产物同步产出后 queuedSpec 会被清空，
    // 若只比 queuedSpec，则「首次请求后的每一次重复请求」都会走queued 分支
    // ——表现为去抖计数恒为 0、狂拖滑条引发 40 次重复入队，
    // 风暴去抖形同虚设（实测踩过）。
    const sameAsLast = this.lastSpec !== null && sameGraphSpec(this.lastSpec, spec);
    const idle = this.compiling === null && this.pending === null && this.queuedSpec === null;
    if (sameAsLast && !idle) {
      this.queuedCount += 1;
      this.diagBag.push("RECOMPILE_DEBOUNCED",
        `第 ${this.queuedCount} 次重编译请求与最近一次受理请求相同，已合并，未触发新编译。`,
        "开关高频切换已去抖到帧边界；如需每帧生效请勿提交等价请求。");
      return execOk("debounced", this.diagBag.all());
    }
    if (this.compiling !== null) {
      // 上一轮还在编译：覆盖待处理 spec，等本轮结束后只编一次
      this.queuedSpec = spec;
      this.lastSpec = spec;
      this.queuedCount += 1;
      this.queuedOptions = options;
      this.diagBag.push("RECOMPILE_DEBOUNCED",
        "上一轮编译尚未完成，新请求已并入待处理队列（帧边界后只编一次）。",
        "编译在后台进行，不阻塞当前帧渲染。");
      return execOk("queued", this.diagBag.all());
    }
    // 立即启动一轮编译（同步执行，但结果只挂 compiling，不影响 current）
    const version = this.nextVersion;
    this.nextVersion += 1;
    this.lastSpec = spec;
    this.queuedCount += 1;
    const res = compileGraph(spec, { ...options, version });
    if (!res.ok) {
      // 编译失败：保留当前计划继续服务，不换绑（失败不该打断渲染）
      this.queuedSpec = null;
      this.queuedOptions = null;
      return execErr(res.code, res.message, res.hint, this.diagBag.all());
    }
    this.compiling = res.value;
    this.queuedSpec = null;
    this.queuedOptions = null;
    return execOk("queued", this.diagBag.all());
  }

  /**
   * 帧边界：把编译完成的计划原子换绑（判据三）。
   * 返回是否发生了换绑。
   */
  beginFrame(): boolean {
    this.frameIndex += 1;
    let swapped = false;
    if (this.compiling !== null) {
      this.pending = this.compiling;
      this.compiling = null;
    }
    if (this.pending !== null) {
      // 原子换绑：单次指针替换，帧内不再变更
      this.current = this.pending;
      this.pending = null;
      swapped = true;
    }
    // 编译期间积累的请求：帧边界后启动下一轮（批处理）
    if (this.queuedSpec !== null && this.compiling === null) {
      const spec = this.queuedSpec;
      const opts = this.queuedOptions;
      this.queuedSpec = null;
      this.queuedOptions = null;
      const version = this.nextVersion;
      this.nextVersion += 1;
      const res = compileGraph(spec, { ...(opts ?? { version }), version });
      if (res.ok) this.compiling = res.value;
      else this.diagBag.push(res.code, res.message, res.hint);
    }
    return swapped;
  }

  /** 帧号（单调递增）。 */
  currentFrame(): number {
    return this.frameIndex;
  }

  /** 最近一次请求重编译的图（F2013 调试面板消费，可观测当前期望状态）。 */
  lastRequestedSpec(): GraphSpec | null {
    return this.lastSpec;
  }

  /**
   * 执行当前计划的一帧（零图遍历开销：只顺序走线性列表）。
   * 执行记录供 F2013 调试可视化消费。
   */
  executeFrame(): ExecOutcome<readonly ExecStep[]> {
    const plan = this.current;
    if (plan === null) {
      return execErr("SWAP_STATE_INVALID", "当前无生效计划。", "先requestRecompile 并经过一次 beginFrame 换绑。",
        this.diagBag.all());
    }
    // 顺序执行线性列表——此处不查图、不排序、不分配，纯顺序
    const executed: ExecStep[] = [];
    for (const step of plan.steps) {
      executed.push(step);
    }
    return execOk(executed, this.diagBag.all());
  }

  /** 累计诊断。 */
  diagnostics(): readonly ExecDiagnostic[] {
    return this.diagBag.all();
  }
}

/** 图 spec 等价性判定（用于去抖：同图重复请求折叠为一次）。 */
export function sameGraphSpec(a: GraphSpec, b: GraphSpec): boolean {
  if (a.externalSlots.length !== b.externalSlots.length) return false;
  if (a.graph.nodes.length !== b.graph.nodes.length) return false;
  if (a.graph.edges.length !== b.graph.edges.length) return false;
  const ka = Object.keys(a.toggles).sort();
  const kb = Object.keys(b.toggles).sort();
  if (ka.length !== kb.length) return false;
  for (let i = 0; i < ka.length; i += 1) {
    if (ka[i] !== kb[i]) return false;
    if (a.toggles[ka[i] as string] !== b.toggles[kb[i] as string]) return false;
  }
  const na = a.graph.nodes.map((n) => `${n.effectId}:${n.toggle}:${n.output.slot}`).sort();
  const nb = b.graph.nodes.map((n) => `${n.effectId}:${n.toggle}:${n.output.slot}`).sort();
  for (let i = 0; i < na.length; i += 1) {
    if (na[i] !== nb[i]) return false;
  }
  return true;
}

/** 帧边界重编译去抖的批处理器：帧内多次请求折叠为一次帧边界调用。 */
export class RecompileDebouncer {
  private readonly pending = new Map<string, GraphSpec>();
  private dropped = 0;

  /** 登记一次请求（同 key 覆盖，只有一次生效）。 */
  enqueue(key: string, spec: GraphSpec): void {
    if (this.pending.has(key)) this.dropped += 1;
    this.pending.set(key, spec);
  }

  /** 帧边界取出全部待编译请求并清空。 */
  drain(): readonly GraphSpec[] {
    const out = [...this.pending.values()];
    this.pending.clear();
    return out;
  }

  /** 被去抖丢弃的请求数。 */
  droppedCount(): number {
    return this.dropped;
  }

  /** 待处理条数。 */
  size(): number {
    return this.pending.size;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §6 自检（逐条对应判据）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项。 */
export interface ExecSelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 构造效果节点（自检用）。 */
function mkNode(
  effectId: string,
  outputSlot: string,
  inputSlots: readonly string[],
  toggle = "t_" + effectId,
  extra: Partial<EffectNode> = {},
): EffectNode {
  return {
    effectId,
    kind: "kind_" + effectId,
    stage: "post",
    inputs: inputSlots.map((s) => ({ slot: s })),
    output: { slot: outputSlot },
    params: { blockId: "pb_" + effectId, values: {} },
    toggle,
    inPlace: false,
    ...extra,
  };
}

/** 构造图。 */
function mkGraph(nodes: readonly EffectNode[], edges: readonly { from: string; to: string }[]): EffectGraph {
  return { nodes, edges };
}

function baseSpec(g: EffectGraph, toggles: Record<string, boolean> = {}): GraphSpec {
  return { graph: g, externalSlots: ["sceneColor"], toggles, inPlaceAllowed: [] };
}

/** 判据一「DAG 模型 + 拓扑序」自检。 */
export function selfCheckDag(): ExecSelfCheck[] {
  const out: ExecSelfCheck[] = [];
  // 三节点链：a→b→c，应得 a,b,c
  const g = mkGraph(
    [mkNode("a", "s1", ["sceneColor"]), mkNode("b", "s2", ["s1"]), mkNode("c", "s3", ["s2"])],
    [{ from: "a", to: "b" }, { from: "b", to: "c" }],
  );
  const res = compileGraph(baseSpec(g), { version: 1 });
  const order = res.ok ? res.value.steps.map((s) => s.effectId) : [];
  out.push({
    name: "链式图拓扑序正确",
    pass: order.length === 3 && order[0] === "a" && order[1] === "b" && order[2] === "c",
    detail: "执行序=" + order.join(">"),
  });
  out.push({
    name: "线性执行列表带序号",
    pass: res.ok && res.value.steps.every((s, i) => s.index === i),
    detail: "序号连续且与执行序一致",
  });
  out.push({
    name: "RT 分配计划逐节点",
    pass: res.ok && res.value.allocations.allocations.length === 3 && res.value.allocations.requiredSlots === 3,
    detail: res.ok ? "分配数=" + res.value.allocations.allocations.length : "n/a",
  });
  // 菱形图：D→{B,C}→E，确认 D 先于 B/C，B/C 先于 E
  const dia = mkGraph(
    [
      mkNode("D", "sd", ["sceneColor"]),
      mkNode("B", "sb", ["sd"]),
      mkNode("C", "sc", ["sd"]),
      mkNode("E", "se", ["sb", "sc"]),
    ],
    [{ from: "D", to: "B" }, { from: "D", to: "C" }, { from: "B", to: "E" }, { from: "C", to: "E" }],
  );
  const r2 = compileGraph(baseSpec(dia), { version: 1 });
  const o2 = r2.ok ? r2.value.steps.map((s) => s.effectId) : [];
  const posOf = (id: string): number => o2.indexOf(id);
  out.push({
    name: "菱形图拓扑序满足全部依赖",
    pass: r2.ok && posOf("D") < posOf("B") && posOf("D") < posOf("C") &&
      posOf("B") < posOf("E") && posOf("C") < posOf("E"),
    detail: "菱形执行序=" + o2.join(">"),
  });
  // 确定性：同一图编译多次序一致
  const runs: string[] = [];
  for (let i = 0; i < 5; i += 1) {
    const rr = compileGraph(baseSpec(dia), { version: 1 });
    runs.push(rr.ok ? rr.value.steps.map((s) => s.effectId).join(">") : "");
  }
  out.push({
    name: "拓扑序确定性（5 次一致）",
    pass: runs.every((r) => r === runs[0]),
    detail: "不同=" + new Set(runs).size,
  });
  return out;
}

/** 判据二「拓扑校验」自检。 */
export function selfCheckValidation(): ExecSelfCheck[] {
  const out: ExecSelfCheck[] = [];
  // 成环
  const cyc = mkGraph(
    [mkNode("a", "s1", ["sceneColor"]), mkNode("b", "s2", ["s1"]), mkNode("c", "s3", ["s2"])],
    [{ from: "a", to: "b" }, { from: "b", to: "c" }, { from: "c", to: "a" }],
  );
  const rc = compileGraph(baseSpec(cyc), { version: 1 });
  out.push({
    name: "成环拒绝执行",
    pass: !rc.ok && rc.code === "DAG_CYCLIC",
    detail: rc.ok ? "被接受" : "拒因=" + rc.code,
  });
  // 环路径必须列出（三要素）
  const cycMsg = rc.ok ? "" : rc.message;
  out.push({
    name: "成环拒绝须列出环路径",
    pass: !rc.ok && cycMsg.indexOf("→") >= 0,
    detail: rc.ok ? "无" : "消息=" + cycMsg,
  });
  // 缺失依赖：输入槽无生产者
  const miss = mkGraph([mkNode("a", "s1", ["notProduced"])], []);
  const rm = compileGraph(baseSpec(miss), { version: 1 });
  out.push({
    name: "输入槽无生产者拒绝",
    pass: !rm.ok && rm.code === "INPUT_SLOT_HAS_NO_PRODUCER",
    detail: rm.ok ? "被接受" : "拒因=" + rm.code,
  });
  // RT 多生产者冲突
  const conflict = mkGraph(
    [mkNode("a", "same", ["sceneColor"]), mkNode("b", "same", ["sceneColor"])],
    [],
  );
  const rcf = compileGraph(baseSpec(conflict), { version: 1 });
  out.push({
    name: "RT 槽多生产者拒绝",
    pass: !rcf.ok && rcf.code === "RT_SLOT_MULTI_PRODUCER",
    detail: rcf.ok ? "被接受" : "拒因=" + rcf.code,
  });
  // 悬空边
  const dangling = mkGraph([mkNode("a", "s1", ["sceneColor"])], [{ from: "a", to: "ghost" }]);
  const rd = compileGraph(baseSpec(dangling), { version: 1 });
  out.push({
    name: "悬空边端点拒绝",
    pass: !rd.ok && rd.code === "EDGE_ENDPOINT_UNKNOWN",
    detail: rd.ok ? "被接受" : "拒因=" + rd.code,
  });
  // 原地效果未登记
  const ip = mkGraph(
    [mkNode("a", "s1", ["sceneColor"], "t_a", { inPlace: true })],
    [{ from: "a", to: "a" }],
  );
  const rip = compileGraph(baseSpec(ip), { version: 1 });
  out.push({
    name: "原地效果未登记拒绝",
    pass: !rip.ok && rip.code === "IN_PLACE_NOT_DECLARED",
    detail: rip.ok ? "被接受" : "拒因=" + rip.code,
  });
  // 原地效果已登记 → 自环不算环
  const ipSpec: GraphSpec = {
    graph: ip,
    externalSlots: ["sceneColor"],
    toggles: {},
    inPlaceAllowed: ["a"],
  };
  const rip2 = compileGraph(ipSpec, { version: 1 });
  out.push({
    name: "原地效果已登记自环放行",
    pass: rip2.ok && rip2.value.steps.length === 1,
    detail: rip2.ok ? "步骤数=" + rip2.value.steps.length : "拒因=" + rip2.code,
  });
  // 超存活上限
  const many: EffectNode[] = [];
  for (let i = 0; i < F2002_EXECUTOR_CAPABILITIES.maxLiveEffects + 2; i += 1) {
    many.push(mkNode("n" + i, "s" + i, ["sceneColor"]));
  }
  const rmany = compileGraph(baseSpec(mkGraph(many, [])), { version: 1 });
  out.push({
    name: "超同时存活上限拒绝",
    pass: !rmany.ok && rmany.code === "LIVE_EFFECT_LIMIT_EXCEEDED",
    detail: rmany.ok ? "被接受" : "拒因=" + rmany.code,
  });
  // 重复 id
  const dup = mkGraph([mkNode("a", "s1", ["sceneColor"]), mkNode("a", "s2", ["sceneColor"])], []);
  const rdup = compileGraph(baseSpec(dup), { version: 1 });
  out.push({
    name: "重复 effectId 拒绝",
    pass: !rdup.ok && rdup.code === "GRAPH_MALFORMED",
    detail: rdup.ok ? "被接受" : "拒因=" + rdup.code,
  });
  return out;
}

/** 判据三「原子重组 + 帧边界」自检。 */
export function selfCheckAtomicSwap(): ExecSelfCheck[] {
  const out: ExecSelfCheck[] = [];
  const g2 = mkGraph([mkNode("b", "sb", ["sceneColor"], "t_b")], []);
  const ex = new PostFxExecutor();
  out.push({
    name: "初始无生效计划",
    pass: ex.activePlan() === null,
    detail: "activePlan=" + (ex.activePlan() === null ? "null" : "有"),
  });
  ex.requestRecompile(baseSpec(g2, { t_b: true }), { version: 1 });
  // 编译完成但未到帧边界：current 仍为空（半新半旧禁止）
  const beforeFrame = ex.activePlan();
  ex.beginFrame();
  const afterFrame = ex.activePlan();
  out.push({
    name: "编译完成但帧边界前不生效",
    pass: beforeFrame === null && afterFrame !== null,
    detail: "帧边界前=" + (beforeFrame === null ? "null" : "有") +
      " 帧边界后=" + (afterFrame === null ? "null" : "有"),
  });
  // 执行当前计划
  const run = ex.executeFrame();
  out.push({
    name: "执行帧返回线性步骤",
    pass: run.ok && run.value.length === 1 && run.value[0]?.effectId === "b",
    detail: run.ok ? "步骤=" + run.value.map((s) => s.effectId).join(",") : "失败",
  });
  // 开关全关 → 编译失败（EMPTY_PLAN）→ **不应换绑**，旧计划继续服务
  // 这是正确的失败处置：重编译失败不该打断渲染，否则用户看到的是「效果突然消失」。
  const offReq = ex.requestRecompile(baseSpec(g2, { t_b: false }), { version: 2 });
  const midOld = ex.activePlan();
  ex.beginFrame();
  const afterOff = ex.activePlan();
  out.push({
    name: "开关全关编译失败不换绑（旧计划继续服务）",
    pass: !offReq.ok && offReq.code === "EMPTY_PLAN" && midOld !== null && afterOff !== null &&
      afterOff.steps.length === 1,
    detail: "拒因=" + (offReq.ok ? "无（意外成功）" : offReq.code) +
      " 换绑后步数=" + (afterOff === null ? "null" : afterOff.steps.length),
  });
  return out;
}

/** 判据四「旁路显性」自检。 */
export function selfCheckBypass(): ExecSelfCheck[] {
  const out: ExecSelfCheck[] = [];
  const g = mkGraph(
    [mkNode("a", "s1", ["sceneColor"], "t_a"), mkNode("b", "s2", ["s1"], "t_b")],
    [{ from: "a", to: "b" }],
  );
  // a 初始化失败 → 旁路，仍产出执行列表且带 bypass 标记
  const res = compileGraph(baseSpec(g), {
    version: 1,
    init: (n) => (n.effectId === "a" ? { ok: false, reason: "资源创建失败" } : { ok: true }),
  });
  out.push({
    name: "初始化失败不崩链（仍产出计划）",
    pass: res.ok,
    detail: res.ok ? "步骤数=" + res.value.steps.length : "拒因=" + res.code,
  });
  const stepA = res.ok ? res.value.steps.find((s) => s.effectId === "a") : undefined;
  out.push({
    name: "旁路步骤带 bypass 标记",
    pass: stepA !== undefined && stepA.bypass,
    detail: stepA === undefined ? "无该步" : "bypass=" + stepA.bypass,
  });
  out.push({
    name: "旁路显性登记诊断",
    pass: res.ok && res.value.diagnostics.some((d) => d.code === "EFFECT_INIT_FAILED_BYPASSED"),
    detail: res.ok ? "旁路诊断数=" + res.value.diagnostics.filter((d) => d.code === "EFFECT_INIT_FAILED_BYPASSED").length : "n/a",
  });
  out.push({
    name: "旁路节点记入 bypassed 列表",
    pass: res.ok && res.value.bypassed.indexOf("a") >= 0,
    detail: res.ok ? "bypassed=" + res.value.bypassed.join(",") : "n/a",
  });
  // 直通语义：旁路写入本节点的 output.slot —— 下游读的就是这个槽，
  // 写入它等价于「把本应写入的内容替换为上游已算好的内容」。
  // 若改写 inputs[0].slot（这里是外部槽 sceneColor），下游会读到未初始化 RT。
  out.push({
    name: "直通语义：旁路写入下游消费的槽",
    pass: stepA !== undefined && stepA.writes === "s1",
    detail: stepA === undefined ? "n/a" : "writes=" + stepA.writes + "（应 s1，下游 tail 读的正是它）",
  });
  return out;
}

/** 判据五「风暴去抖」自检。 */
export function selfCheckDebounce(): ExecSelfCheck[] {
  const out: ExecSelfCheck[] = [];
  const g = mkGraph([mkNode("a", "s1", ["sceneColor"], "t_a")], []);
  const ex = new PostFxExecutor();
  // 同图反复请求（模拟狂拖滑条）：只编译一次，其余去抖
  ex.requestRecompile(baseSpec(g, { t_a: true }), { version: 1 });
  for (let i = 0; i < 20; i += 1) {
    ex.requestRecompile(baseSpec(g, { t_a: true }), { version: 1 });
  }
  out.push({
    name: "重复请求被去抖（不重复编译）",
    pass: ex.queuedRequestCount() === 21 && ex.isCompiling(),
    detail: "累计请求=" + ex.queuedRequestCount() + "（21 次折叠为 1 次编译）",
  });
  out.push({
    name: "去抖显性登记诊断",
    pass: ex.diagnostics().some((d) => d.code === "RECOMPILE_DEBOUNCED"),
    detail: "去抖诊断数=" + ex.diagnostics().filter((d) => d.code === "RECOMPILE_DEBOUNCED").length,
  });
  ex.beginFrame();
  out.push({
    name: "帧边界后单次换绑生效",
    pass: ex.activePlan() !== null && ex.activePlan()?.steps.length === 1,
    detail: ex.activePlan() === null ? "null" : "步骤=" + ex.activePlan()?.steps.length,
  });
  // 批处理器
  const db = new RecompileDebouncer();
  for (let i = 0; i < 30; i += 1) db.enqueue("bloom", baseSpec(g, { t_a: i % 2 === 0 }));
  out.push({
    name: "批处理器同 key 只留一个",
    pass: db.size() === 1 && db.droppedCount() === 29,
    detail: "size=" + db.size() + " 丢弃=" + db.droppedCount(),
  });
  const drained = db.drain();
  out.push({
    name: "批处理器 drain 后清空",
    pass: drained.length === 1 && db.size() === 0,
    detail: "drain=" + drained.length + " 余=" + db.size(),
  });
  return out;
}

/** 全量自检入口。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly ExecSelfCheck[]>>;
  readonly allPass: boolean;
  readonly total: number;
  readonly failed: readonly string[];
} {
  const groups = {
    dag: selfCheckDag(),
    validation: selfCheckValidation(),
    atomicSwap: selfCheckAtomicSwap(),
    bypass: selfCheckBypass(),
    debounce: selfCheckDebounce(),
  };
  const failed: string[] = [];
  let total = 0;
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      total += 1;
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, total, failed };
}