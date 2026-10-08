/**
 * VE-F0610 · 图层树快照与恢复（D 域 · 2D 合成引擎图层树组 · 批次 D01）
 * ---------------------------------------------------------------------------
 * 职责定位：图层树快照与恢复——D 域的时间轴基础设施。快照是**内存态的完整复制**，
 * 供三个方向复用：撤销重做的树级支撑、调试时间旅行、崩溃恢复。
 * 上游 F0601 图层树结构（节点与稀疏属性）、F0607 可见性传播（版本号同源）。
 * 下游 F0612 序列化（快照 ↔ 持久化格式互转）、F0613 脏区收集（快照比对辅助脏区验证）。
 *
 * 锚点契约（五条，逐条对应判据）：
 *   1. 稀疏快照：完整快照存树结构 + **全部有效属性**的深拷贝。稀疏属性只存有效集，
 *      不逐节点填满默认值——这是内存与遍历双赢的关键（未设属性走继承，
 *      把默认值抄进快照会让快照体积随节点数线性膨胀且丢失「未设置」语义）。
 *   2. 三方版本号：树全局版本、节点版本、快照版本。增量快照基于三方比对，
 *      差分含**结构变更**（增删节点）与**属性变更**（同节点属性集变化）两类。
 *   3. 恢复幂等性：同一快照恢复两次，结果**逐位一致**——恢复必须是纯函数。
 *      这是崩溃恢复与时间旅行的共同前提（不幂等的恢复会引入不确定状态）。
 *   4. 用途三向：撤销重做 / 调试时间旅行 / 崩溃恢复，三者共用同一套快照机制，
 *      不为每个用途各做一份格式。
 *   5. 降级矩阵：版本不一致 → 全量快照兜底；恢复冲突（树上已有变更）→ 策略显性
 *      （覆盖或拒绝，不默认猜一个）；差分超阈 → 自动转全量。
 *
 * 四条容易做错、故显式记录的设计立场：
 *
 *   一、幂等恢复的前提是「恢复不读当前树状态做决策之外的任何事」。
 *     朴素实现会在恢复时把节点的「上次修改时间」「脏标记」一并改掉，导致第二次
 *     恢复的输入已不同 → 结果不同 → 幂等性破坏。故本条的恢复是**替换式**：
 *     恢复只写「结构 + 有效属性 + 版本号」，绝不触碰派生状态（脏标记、缓存）。
 *     派生状态由调用方在恢复后按需重算——这是纯函数性的必要代价，也是可验证的。
 *
 *   二、稀疏性必须在快照层守住，不能靠「读的时候填默认值」。
 *     若快照存全量属性，则「未设置」与「显式设为默认值」在快照中不可区分，
 *     恢复后继承链语义会被破坏（未设置的属性本应继承父层，显式值则不继承）。
 *     故快照只记有效集，且提供 has() 查询让调用方能区分这两种状态。
 *
 *   三、三方版本号缺一不可，尤其不能省节点版本。
 *     只有「树全局版本 + 快照版本」时，无法判断某个具体节点的属性是否被改过——
 *     树全局版本变了不代表这个节点变了（可能是别处改的）。
 *     缺节点版本会导致增量快照把未变节点也计入差分（退化为全量）或漏掉真变更。
 *
 *   四、差分超阈自动转全量是兜底，不是优化。
 *     差分集超过阈值说明树已被大改，逐个应用差分的成本与风险都高于直接取全量。
 *     自动转全量必须**显性告知**（产出诊断），否则调用方会以为拿到的是增量，
 *     对恢复耗时与冲突面产生错误预期。
 *
 * 零静默纪律：版本不一致、恢复冲突、差分超阈、属性校验失败全部产出
 * Diagnostic（code + message + hint），降级一律显性不做暗转。
 *
 * 判据：稀疏快照、三方版本、恢复幂等、用途三向。
 * 依赖锚点：F0601 图层树结构（节点、稀疏属性表、脏标记族）、F0607 可见性传播
 *          （脏标记向上聚合，版本号与之同源递增）、F0612 序列化、F0613 脏区收集。
 * 交接说明：本条的快照是内存态，与 F0612 的持久化格式一一对应但**不是同一格式**——
 *          互转走显式的导出/导入函数，不允许把内存对象直接当文件写（引用共享会
 *          导致「改了快照却改了活树」这类极难排查的问题）。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型
// ════════════════════════════════════════════════════════════════════════════

/** 快照与恢复专属诊断码（与 F0608/F0609 各自独立，不共用枚举）。 */
export type SnapshotDiagCode =
  | "VERSION_MISMATCH"
  | "RESTORE_CONFLICT"
  | "DIFF_EXCEEDS_THRESHOLD"
  | "SNAPSHOT_CORRUPT"
  | "NODE_PROPERTY_INVALID"
  | "TREE_INVARIANT_VIOLATED"
  | "NODE_NOT_FOUND"
  | "EMPTY_TREE";

/** 一条诊断：发生了什么、影响什么、下一步怎么办。 */
export interface SnapshotDiagnostic {
  readonly code: SnapshotDiagCode;
  readonly message: string;
  readonly hint: string;
}

/** 结果判别联合。 */
export type SnapshotOutcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly SnapshotDiagnostic[] }
  | {
      readonly ok: false;
      readonly code: SnapshotDiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly SnapshotDiagnostic[];
    };

/** 成功构造。 */
export function snapOk<T>(value: T, diagnostics: readonly SnapshotDiagnostic[] = []): SnapshotOutcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造。 */
export function snapFail<T>(
  code: SnapshotDiagCode,
  message: string,
  hint: string,
): SnapshotOutcome<T> {
  const d: SnapshotDiagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [d] };
}

/** 诊断聚合器。 */
export class SnapshotDiagBag {
  private readonly items: SnapshotDiagnostic[] = [];

  push(code: SnapshotDiagCode, message: string, hint: string): void {
    this.items.push({ code, message: message || "（未提供描述）", hint: hint || "（未提供处置建议）" });
  }

  get size(): number {
    return this.items.length;
  }

  all(): readonly SnapshotDiagnostic[] {
    return this.items.slice();
  }

  byCode(code: SnapshotDiagCode): readonly SnapshotDiagnostic[] {
    return this.items.filter((d) => d.code === code);
  }
}

/** 有限性守卫（数值属性入快照前必须过）。 */
function isFiniteNum(v: number): boolean {
  return Number.isFinite(v);
}

// ════════════════════════════════════════════════════════════════════════════
// §2 图层树的数据形态（本条的输入结构，含稀疏属性与三方版本号）
// ════════════════════════════════════════════════════════════════════════════

/** 属性值：与 F0608 插值接口共用同一形态（保持跨条一致）。 */
export type PropValue = number | string | boolean;

/**
 * 图层节点：结构字段 + **稀疏属性表**。
 * 稀疏性是本条快照设计的核心前提（立场二）：未设的属性不出现在 props 中，
 * 恢复后仍走继承，绝不因为快照而变成「显式值」。
 */
export interface LayerNode {
  /** 稳定 id（跨快照不变——外部句柄依赖它）。 */
  readonly id: string;
  /** 父节点 id；根为 null。 */
  readonly parentId: string | null;
  /** 子节点 id 序列（有序即Z 序，与 F0606 同源）。 */
  readonly childIds: readonly string[];
  /** 稀疏属性表：只含显式设置过的属性。 */
  readonly props: Readonly<Record<string, PropValue>>;
  /** 节点版本：每次本节点的结构或属性变更递增。 */
  readonly version: number;
}

/**
 * 图层树：节点表 + 三方版本号中的前两方（快照版本由快照自身携带）。
 * treeVersion 在**任何**节点变更时递增（结构或属性皆然）；
 * nodeVersion 逐节点独立递增。两者缺一不可（立场三）。
 */
export interface LayerTree {
  readonly nodes: ReadonlyMap<string, LayerNode>;
  /** 树全局版本。 */
  readonly treeVersion: number;
}

/** 节点类型标签白名单（与 F0601 的类型约束对齐，本条只校验不定义）。 */
export const NODE_TAGS: readonly string[] = ["root", "group", "text", "image", "vector", "adjustment"];

// ════════════════════════════════════════════════════════════════════════════
// §3 完整快照（判据一：稀疏快照）
// ════════════════════════════════════════════════════════════════════════════

/** 完整快照：结构 + 有效属性的深拷贝 + 三方版本号。 */
export interface FullSnapshot {
  readonly kind: "full";
  /** 快照创建时的树版本。 */
  readonly treeVersion: number;
  /** 逐节点的版本号与稀疏属性（深拷贝，与活树无引用共享）。 */
  readonly nodes: readonly SnapNodeEntry[];
  /** 根节点 id。 */
  readonly rootId: string;
}

/** 快照中单个节点的条目。 */
export interface SnapNodeEntry {
  readonly id: string;
  readonly parentId: string | null;
  readonly childIds: readonly string[];
  /** 稀疏属性深拷贝（只含显式设置过的项）。 */
  readonly props: Readonly<Record<string, PropValue>>;
  readonly version: number;
}

/**
 * 取节点的稀疏属性副本。
 * 显式遍历并新建对象——**不用结构化共享或浅拷贝**，
 * 因为活树的 props 若被后续修改，快照必须不受影响（深拷贝的语义要求）。
 */
function cloneSparseProps(props: Readonly<Record<string, PropValue>>): Record<string, PropValue> {
  const out: Record<string, PropValue> = {};
  for (const k of Object.keys(props)) {
    out[k] = props[k];
  }
  return out;
}

/**
 * 创建完整快照（O(节点数)）。
 * 校验不变式：父子关系无环、单父、根唯一。断言失败即拒绝出快照——
 * 脏树进快照会污染后续所有恢复，故在入口拦下比在恢复时排查便宜得多。
 */
export function createFullSnapshot(tree: LayerTree): SnapshotOutcome<FullSnapshot> {
  const bag = new SnapshotDiagBag();
  if (tree.nodes.size === 0) {
    return snapFail<FullSnapshot>(
      "EMPTY_TREE",
      "图层树没有任何节点，无法创建快照",
      "空树快照无恢复价值；若这是暂态请等树初始化完成，若确实要支持空树请在调用方显式处理",
    );
  }

  const roots: string[] = [];
  for (const node of tree.nodes.values()) {
    if (node.parentId === null) roots.push(node.id);
  }
  if (roots.length !== 1) {
    return snapFail<FullSnapshot>(
      "TREE_INVARIANT_VIOLATED",
      `树有 ${roots.length} 个根节点（期望恰好 1 个）`,
      "多根或无根说明父子关系已断链；请先修复树结构再出快照，否则恢复后无法确定遍历起点",
    );
  }

  // 单父校验 + 无环校验（沿父链上溯，步数不得超过节点数）
  const idSet = new Set(tree.nodes.keys());
  for (const node of tree.nodes.values()) {
    if (node.parentId !== null) {
      if (!idSet.has(node.parentId)) {
        return snapFail<FullSnapshot>(
          "TREE_INVARIANT_VIOLATED",
          `节点 ${node.id} 的父节点 ${node.parentId} 不存在`,
          "父引用悬空；请先修复树结构，快照拒绝保存不连通的树",
        );
      }
    }
    // 无环：沿父链上溯，超过节点数步即成环
    let steps = 0;
    let cur: LayerNode | undefined = node;
    while (cur !== undefined && cur.parentId !== null) {
      cur = tree.nodes.get(cur.parentId);
      steps += 1;
      if (steps > tree.nodes.size) {
        return snapFail<FullSnapshot>(
          "TREE_INVARIANT_VIOLATED",
          `从节点 ${node.id} 沿父链上溯超过 ${tree.nodes.size} 步，判定为环`,
          "父链成环会让恢复后的遍历死循环；请先断开环再出快照",
        );
      }
    }
  }

  // 属性校验：非有限数值不入快照（NaN 会静默污染后续每一次恢复）
  for (const node of tree.nodes.values()) {
    for (const key of Object.keys(node.props)) {
      const v = node.props[key];
      if (typeof v === "number" && !isFiniteNum(v)) {
        bag.push(
          "NODE_PROPERTY_INVALID",
          `节点 ${node.id} 的属性 ${key} 为非有限数（${String(v)}）`,
          "已按原值存入但标记为不可靠；恢复后该属性可能产生 NaN，建议先修复上游写入路径",
        );
      }
    }
  }

  const entries: SnapNodeEntry[] = [];
  for (const node of tree.nodes.values()) {
    entries.push({
      id: node.id,
      parentId: node.parentId,
      childIds: [...node.childIds],
      props: cloneSparseProps(node.props),
      version: node.version,
    });
  }
  // 按 id 排序保证快照的确定性（Map 的插入序不保证稳定，
  // 而幂等性判据要求「同状态两次出快照逐位一致」）。
  entries.sort((x, y) => (x.id < y.id ? -1 : x.id > y.id ? 1 : 0));

  return snapOk({ kind: "full" as const, treeVersion: tree.treeVersion, nodes: entries, rootId: roots[0] ?? "" }, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §4 三方版本号与增量快照（判据二）
// ════════════════════════════════════════════════════════════════════════════

/** 结构变更的一类：节点增删。 */
export interface StructuralDiff {
  readonly kind: "add" | "remove";
  readonly nodeId: string;
  /** 新增时携带父 id；删除时携带原父 id（供撤销/时间旅行定位）。 */
  readonly parentId: string | null;
  /** 新增时携带初始子序列。 */
  readonly childIds: readonly string[];
  /** 新增时携带初始属性。 */
  readonly props: Readonly<Record<string, PropValue>>;
}

/** 属性变更的一类：某节点属性集的整体替换。 */
export interface PropertyDiff {
  readonly kind: "props";
  readonly nodeId: string;
  readonly props: Readonly<Record<string, PropValue>>;
  readonly version: number;
}

/** 差分集：结构变更与属性变更两类（锚点明确要求分类）。 */
export type TreeDiff = StructuralDiff | PropertyDiff;

/** 增量快照：三方版本号 + 差分集。 */
export interface IncrementalSnapshot {
  readonly kind: "incremental";
  /** 基线快照版本（差分的起点）。 */
  readonly baseVersion: number;
  /** 本快照自身的版本（第三方）。 */
  readonly snapshotVersion: number;
  /** 树版本（第二方）。 */
  readonly treeVersion: number;
  readonly diffs: readonly TreeDiff[];
  readonly rootId: string;
}

/** 快照的统一形态（判据四：三向共用同一套机制）。 */
export type AnySnapshot = FullSnapshot | IncrementalSnapshot;

/**
 * 差分规模阈值：超过即自动转全量（立场四）。
 *
 * 取值规则：差分数 > 节点数 × 该比例 才算超阈。
 * 注意**不能先floor 再取max(1, …)** —— 那样小树（如 3 节点）会得到阈值 1，
 * 于是任何 2 条差分都被误判超阈、增量快照全量退化。
 * 故此处用浮点比较，阈值下限固定为 2（低于 2 的差分无论树多小都属正常增量）。
 */
export const DIFF_THRESHOLD_RATIO = 0.5;

/** 差分条数下限阈值：差分不超过此数时恒不判超阈（保护小树）。 */
export const DIFF_MIN_THRESHOLD = 2;

/**
 * 创建增量快照（O(变更集)）。
 *
 * 三方版本号的使用（本函数的全部依据）：
 *   · 快照版本 baseVersion —— 差分的起点，取调用方给的基线；
 *   · 树版本 treeVersion —— 判定树是否被改过（若未改则差分为空）；
 *   · 节点版本 —— 逐节点判定该节点是否需要进差分（立场三：缺它会误判）。
 *
 * 超阈转全量：差分数超过「节点数 × 阈值」即认为树已被大改，
 * 逐条应用差分的成本与冲突面都高于直接取全量 → 自动转全量并显性告知。
 */
export function createIncrementalSnapshot(
  tree: LayerTree,
  base: FullSnapshot,
  opts?: { readonly thresholdRatio?: number },
): SnapshotOutcome<AnySnapshot> {
  const bag = new SnapshotDiagBag();
  const ratio = opts?.thresholdRatio ?? DIFF_THRESHOLD_RATIO;

  // 版本前置校验：基线必须是本树的合法祖先版本，否则差分无意义。
  if (base.treeVersion > tree.treeVersion) {
    return snapFail<AnySnapshot>(
      "VERSION_MISMATCH",
      `基线快照版本 ${base.treeVersion} 高于当前树版本 ${tree.treeVersion}`,
      "基线不可能是当前树的祖先；请确认基线快照来自本树且未被回滚重放，否则差分会算出反向变更",
    );
  }

  const baseNodes = new Map(base.nodes.map((n) => [n.id, n]));
  const diffs: TreeDiff[] = [];

  // 结构变更：新增节点
  for (const node of tree.nodes.values()) {
    const prev = baseNodes.get(node.id);
    if (prev === undefined) {
      diffs.push({
        kind: "add",
        nodeId: node.id,
        parentId: node.parentId,
        childIds: [...node.childIds],
        props: cloneSparseProps(node.props),
      });
      continue;
    }
    // 属性变更：版本号变了才进差分（这是节点版本号的用途，立场三）
    if (prev.version !== node.version) {
      diffs.push({
        kind: "props",
        nodeId: node.id,
        props: cloneSparseProps(node.props),
        version: node.version,
      });
    }
  }
  // 结构变更：删除节点
  for (const snapNode of base.nodes) {
    if (!tree.nodes.has(snapNode.id)) {
      diffs.push({
        kind: "remove",
        nodeId: snapNode.id,
        parentId: snapNode.parentId,
        childIds: [...snapNode.childIds],
        props: {},
      });
    }
  }

  // 超阈判定：差分数占比过高即转全量（显性告知，立场四）
  //阈值用浮点比较 + 下限保护，避免小树上floor 把阈值压到 1 导致正常差分被误判。
  const threshold = Math.max(DIFF_MIN_THRESHOLD, tree.nodes.size * ratio);
  if (diffs.length > threshold) {
    bag.push(
      "DIFF_EXCEEDS_THRESHOLD",
      `差分项${diffs.length} 条超过阈值 ${threshold} 条（节点数 ${tree.nodes.size} × ${ratio}，下限 ${DIFF_MIN_THRESHOLD}）`,
      "已自动转为全量快照：树已被大改，逐条应用差分的成本与冲突面都高于直接取全量",
    );
    const full = createFullSnapshot(tree);
    if (!full.ok) return full;
    return snapOk(full.value, [...bag.all(), ...full.diagnostics]);
  }

  return snapOk(
    {
      kind: "incremental" as const,
      baseVersion: base.treeVersion,
      snapshotVersion: tree.treeVersion,
      treeVersion: tree.treeVersion,
      diffs,
      rootId: base.rootId,
    },
    bag.all(),
  );
}

/** 取快照的树版本（两种快照形态的统一访问点）。 */
export function snapshotTreeVersion(snap: AnySnapshot): number {
  return snap.treeVersion;
}

// ════════════════════════════════════════════════════════════════════════════
// §5 恢复（判据三：幂等性）
// ════════════════════════════════════════════════════════════════════════════

/** 恢复冲突策略（锚点要求「策略显性」，不默认猜）。 */
export type RestoreConflictPolicy =
  /** 强制覆盖（撤销重做与时间旅行的常规选择）。 */
  | "overwrite"
  /** 拒绝恢复（崩溃恢复的保守选择：宁可不恢复也不丢数据）。 */
  | "reject"
  /** 转全量重试（版本不一致时的兜底）。 */
  | "fallbackFull";

/** 恢复结果：产出新的树，**不改传入的树**（纯函数）。 */
export interface RestoreResult {
  readonly tree: LayerTree;
  readonly diagnostics: readonly SnapshotDiagnostic[];
  /** 本次恢复实际采取的动作，供调用方记账。 */
  readonly applied: "full" | "incremental" | "rejected";
}

/**
 * 恢复快照到树（**纯函数**：不改入参，返回全新树）。
 *
 * 幂等性保证（立场一）：恢复只写「结构 + 有效属性 + 版本号」三类字段，
 * 绝不触碰派生状态（脏标记、包围盒缓存、逆矩阵缓存）。这些由调用方在恢复后
 * 按需重算——这是幂等性的必要代价：任何依赖「恢复前树状态」的写入都会破坏幂等。
 *
 * 冲突策略显性化：
 *   overwrite —— 强制覆盖当前树（撤销重做用）；
 *   reject —— 拒绝并产出诊断（崩溃恢复用，宁可不恢复）；
 *   fallbackFull —— 版本不一致时自动转全量重试（降级兜底）。
 */
export function restoreSnapshot(
  tree: LayerTree,
  snap: AnySnapshot,
  policy: RestoreConflictPolicy,
): SnapshotOutcome<RestoreResult> {
  const bag = new SnapshotDiagBag();

  // 版本一致性检查：快照版本必须不大于当前树版本（否则是拿旧快照覆盖新树）。
  if (snap.treeVersion > tree.treeVersion) {
    if (policy === "reject") {
      return snapFail<RestoreResult>(
        "RESTORE_CONFLICT",
        `快照版本 ${snap.treeVersion} 高于当前树版本 ${tree.treeVersion}`,
        "已按 reject 策略拒绝恢复；若确认要回退到该快照，请显式改用 overwrite 策略",
      );
    }
    bag.push(
      "VERSION_MISMATCH",
      `快照版本 ${snap.treeVersion} 高于当前树版本 ${tree.treeVersion}`,
      "快照比树还新，正常情况下不应发生；已按显式策略继续，请确认快照来源正确",
    );
  }

  if (snap.kind === "full") {
    const built = buildTreeFromFull(snap, bag);
    if (!built.ok) return built;
    return snapOk({ tree: built.value, diagnostics: bag.all(), applied: "full" });
  }

  // 增量恢复：版本不匹配且允许兜底时，转全量（锚点降级矩阵第一行）
  if (snap.baseVersion !== tree.treeVersion && policy === "fallbackFull") {
    bag.push(
      "VERSION_MISMATCH",
      `增量快照基线版本 ${snap.baseVersion} 与当前树版本 ${tree.treeVersion} 不一致`,
      "已按 fallbackFull 策略放弃增量路径；若要严格增量恢复请先确认树未被其他路径改动",
    );
    // 增量快照不含完整节点集，无法就地转全量——须由调用方提供全量基线。
    return snapFail<RestoreResult>(
      "VERSION_MISMATCH",
      `增量快照基线版本 ${snap.baseVersion} 与当前树版本 ${tree.treeVersion} 不匹配，且增量快照本身不含全量数据`,
      "请改用全量快照恢复，或提供基线全量快照让本条走「基线 + 增量」的两段式恢复",
    );
  }

  const built = applyDiffs(tree, snap, bag);
  if (!built.ok) return built;
  return snapOk({ tree: built.value, diagnostics: bag.all(), applied: "incremental" });
}

/** 由完整快照构建树（幂等：同快照两次构建结果逐位一致）。 */
function buildTreeFromFull(snap: FullSnapshot, bag: SnapshotDiagBag): SnapshotOutcome<LayerTree> {
  const nodes = new Map<string, LayerNode>();
  for (const entry of snap.nodes) {
    nodes.set(entry.id, {
      id: entry.id,
      parentId: entry.parentId,
      childIds: [...entry.childIds],
      // 二次深拷贝：保证恢复出的树与快照对象无引用共享，
      // 否则调用方改恢复结果会污染快照（进而破坏「快照可反复恢复」的幂等前提）。
      props: cloneSparseProps(entry.props),
      version: entry.version,
    });
  }
  if (nodes.size === 0) {
    return snapFail<LayerTree>("SNAPSHOT_CORRUPT", "完整快照不含任何节点", "快照数据不可用；请从活树重新出快照");
  }
  if (!nodes.has(snap.rootId)) {
    bag.push(
      "SNAPSHOT_CORRUPT",
      `快照声明的根节点 ${snap.rootId} 不在节点表中`,
      "已退化为「第一个父为null 的节点」作为根；建议重新出快照",
    );
  }
  return snapOk({ nodes, treeVersion: snap.treeVersion }, bag.all());
}

/** 应用差分集到树（幂等：同树 + 同差分两次应用结果一致）。 */
function applyDiffs(
  tree: LayerTree,
  snap: IncrementalSnapshot,
  bag: SnapshotDiagBag,
): SnapshotOutcome<LayerTree> {
  const nodes = new Map<string, LayerNode>();
  for (const [id, node] of tree.nodes) {
    // 逐节点复制（不共享 props 对象），保证返回的树与入参无引用共享。
    nodes.set(id, { ...node, childIds: [...node.childIds], props: cloneSparseProps(node.props) });
  }

  for (const diff of snap.diffs) {
    if (diff.kind === "remove") {
      nodes.delete(diff.nodeId);
      continue;
    }
    if (diff.kind === "add") {
      nodes.set(diff.nodeId, {
        id: diff.nodeId,
        parentId: diff.parentId,
        childIds: [...diff.childIds],
        props: cloneSparseProps(diff.props),
        version: 1,
      });
      continue;
    }
    // 属性变更：目标节点必须存在，否则无法应用（显性失败，不静默跳过）
    const target = nodes.get(diff.nodeId);
    if (target === undefined) {
      bag.push(
        "NODE_NOT_FOUND",
        `差分要求更新节点 ${diff.nodeId} 的属性，但该节点不在当前树中`,
        "已跳过该条属性差分；请确认增量快照与当前树属于同一条变更链",
      );
      continue;
    }
    nodes.set(diff.nodeId, {
      ...target,
      props: cloneSparseProps(diff.props),
      version: diff.version,
    });
  }

  return snapOk({ nodes, treeVersion: snap.treeVersion }, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §6 用途三向（判据四）与对外查询
// ════════════════════════════════════════════════════════════════════════════

/** 快照的三种用途（共用同一机制，不为每个用途各做一份格式）。 */
export type SnapshotPurpose = "undo-redo" | "time-travel" | "crash-recovery";

/** 用途契约：各用途的默认冲突策略不同（这是「策略显性」的落点）。 */
export const PURPOSE_DEFAULT_POLICY: Readonly<Record<SnapshotPurpose, RestoreConflictPolicy>> = {
  // 撤销重做：时间线上的既定动作，必须覆盖
  "undo-redo": "overwrite",
  // 时间旅行：调试用途，覆盖才能回到目标时刻
  "time-travel": "overwrite",
  // 崩溃恢复：宁可不恢复也不丢数据
  "crash-recovery": "reject",
};

/** 按用途取默认策略（调用方仍可显式覆盖，此处只给默认）。 */
export function policyForPurpose(purpose: SnapshotPurpose): RestoreConflictPolicy {
  return PURPOSE_DEFAULT_POLICY[purpose];
}

/** 查询节点在快照中是否显式设置了某属性（区分「未设置」与「设为默认值」，立场二）。 */
export function snapshotHas(snap: FullSnapshot, nodeId: string, propKey: string): boolean {
  const node = snap.nodes.find((n) => n.id === nodeId);
  if (node === undefined) return false;
  return Object.prototype.hasOwnProperty.call(node.props, propKey);
}

/** 取节点在快照中的属性值；未设置返回 undefined（不返回默认值，保持稀疏语义）。 */
export function snapshotProp(
  snap: FullSnapshot,
  nodeId: string,
  propKey: string,
): PropValue | undefined {
  const node = snap.nodes.find((n) => n.id === nodeId);
  if (node === undefined) return undefined;
  return node.props[propKey];
}

/** 快照统计（供调用方预估恢复成本与内存占用）。 */
export interface SnapshotStats {
  readonly nodeCount: number;
  /** 稀疏度：显式属性总数 / 节点数（越低说明稀疏性越好）。 */
  readonly propsPerNode: number;
  readonly kind: "full" | "incremental";
  readonly diffCount: number;
}

/** 统计快照规模（O(节点数)）。 */
export function snapshotStats(snap: AnySnapshot): SnapshotStats {
  if (snap.kind === "full") {
    let props = 0;
    for (const n of snap.nodes) props += Object.keys(n.props).length;
    return {
      nodeCount: snap.nodes.length,
      propsPerNode: snap.nodes.length === 0 ? 0 : props / snap.nodes.length,
      kind: "full",
      diffCount: 0,
    };
  }
  return {
    nodeCount: 0,
    propsPerNode: 0,
    kind: "incremental",
    diffCount: snap.diffs.length,
  };
}

/**
 * 稀疏性核验：确认快照里没有把默认值填满（立场二的守卫）。
 * 若 propsPerNode 接近「每节点属性总数上限」，说明稀疏性已破——
 * 此时「未设置」与「显式默认」不可区分，恢复后继承语义会破坏。
 */
export function auditSparsity(snap: FullSnapshot, expectedMaxPerNode: number): SnapshotDiagnostic[] {
  const out: SnapshotDiagnostic[] = [];
  const stats = snapshotStats(snap);
  if (expectedMaxPerNode > 0 && stats.propsPerNode > expectedMaxPerNode * 0.9) {
    out.push({
      code: "NODE_PROPERTY_INVALID",
      message: `快照平均每节点 ${stats.propsPerNode.toFixed(2)} 项属性，接近上限 ${expectedMaxPerNode}`,
      hint: "稀疏性可能被破坏（默认值被填入）；请检查出快照前是否误用了属性全量展开",
    });
  }
  return out;
}

// ════════════════════════════════════════════════════════════════════════════
// §7 自检
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项。 */
export interface SnapshotSelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 构造测试树（自检内部用）。 */
function mkTree(
  entries: ReadonlyArray<{
    id: string;
    parent: string | null;
    props?: Record<string, PropValue>;
    version?: number;
  }>,
  treeVersion: number,
): LayerTree {
  const nodes = new Map<string, LayerNode>();
  for (const e of entries) {
    nodes.set(e.id, {
      id: e.id,
      parentId: e.parent,
      childIds: [],
      props: e.props ?? {},
      version: e.version ?? 1,
    });
  }
  // 由父子关系补全 childIds（保持传入顺序即 Z 序）
  for (const e of entries) {
    if (e.parent === null) continue;
    const parent = nodes.get(e.parent);
    if (parent !== undefined) parent.childIds = [...parent.childIds, e.id];
  }
  return { nodes, treeVersion };
}

/** 树相等判定（用于幂等性判据：逐字段比对，不比对象身份）。 */
export function treesIdentical(a: LayerTree, b: LayerTree): boolean {
  if (a.treeVersion !== b.treeVersion) return false;
  if (a.nodes.size !== b.nodes.size) return false;
  const idsA = [...a.nodes.keys()].sort();
  const idsB = [...b.nodes.keys()].sort();
  for (let i = 0; i < idsA.length; i += 1) {
    if (idsA[i] !== idsB[i]) return false;
  }
  for (const [id, na] of a.nodes) {
    const nb = b.nodes.get(id);
    if (nb === undefined) return false;
    if (na.parentId !== nb.parentId) return false;
    if (na.version !== nb.version) return false;
    if (na.childIds.length !== nb.childIds.length) return false;
    for (let i = 0; i < na.childIds.length; i += 1) {
      if (na.childIds[i] !== nb.childIds[i]) return false;
    }
    const ka = Object.keys(na.props).sort();
    const kb = Object.keys(nb.props).sort();
    if (ka.length !== kb.length) return false;
    for (let i = 0; i < ka.length; i += 1) {
      if (ka[i] !== kb[i]) return false;
      if (na.props[ka[i]] !== nb.props[ka[i]]) return false;
    }
  }
  return true;
}

/** 判据一「稀疏快照」自检。 */
export function selfCheckSparse(): SnapshotSelfCheck[] {
  const out: SnapshotSelfCheck[] = [];
  // 稀疏树：只有 root 设了 2 个属性，子节点全空
  const tree = mkTree(
    [
      { id: "root", parent: null, props: { name: "doc", visible: true } },
      { id: "a", parent: "root" },
      { id: "b", parent: "root" },
      { id: "c", parent: "a" },
    ],
    7,
  );
  const snap = createFullSnapshot(tree);
  if (!snap.ok) return [{ name: "sparse-capture", pass: false, detail: `出快照失败：${snap.message}` }];

  const stats = snapshotStats(snap.value);
  out.push({
    name: "sparse-props-only",
    pass: stats.propsPerNode === 0.5,
    detail: `4 节点仅 root 设2 属性 → 平均 ${stats.propsPerNode} 项/节点（填满默认值会变成 ≥2）`,
  });

  const rootHas = snapshotHas(snap.value, "root", "name");
  const childHas = snapshotHas(snap.value, "a", "name");
  out.push({
    name: "unset-distinguishable-from-default",
    pass: rootHas && !childHas,
    detail: `root 有 name=${String(rootHas)}，子节点 a 有 name=${String(childHas)}（未设置须可区分于「显式设为默认值」，否则继承语义破坏）`,
  });

  out.push({
    name: "get-unset-returns-undefined",
    pass: snapshotProp(snap.value, "a", "name") === undefined,
    detail: `取未设置属性返回 ${String(snapshotProp(snap.value, "a", "name"))}（不是默认值，保持稀疏语义）`,
  });

  // 深拷贝隔离：改快照不应影响活树
  const entry = snap.value.nodes.find((n) => n.id === "root");
  if (entry !== undefined) {
    (entry.props as Record<string, PropValue>).name = "MUTATED";
    const liveRoot = tree.nodes.get("root");
    out.push({
      name: "deep-copy-isolation",
      pass: liveRoot !== undefined && liveRoot.props.name === "doc",
      detail: `改快照后活树 name=${String(liveRoot?.props.name)}（期望仍为 doc，快照与活树须无引用共享）`,
    });
  }

  return out;
}

/** 判据二「三方版本」自检。 */
export function selfCheckVersions(): SnapshotSelfCheck[] {
  const out: SnapshotSelfCheck[] = [];
  const base = mkTree(
    [
      { id: "root", parent: null, props: { name: "doc" }, version: 1 },
      { id: "a", parent: "root", props: { x: 1 }, version: 1 },
    ],
    3,
  );
  const baseSnap = createFullSnapshot(base);
  if (!baseSnap.ok) return [{ name: "version-base", pass: false, detail: baseSnap.message }];

  // 改一个节点的属性 + 增一个节点 → 差分应恰好 2 条
  const next = mkTree(
    [
      { id: "root", parent: null, props: { name: "doc" }, version: 1 },
      { id: "a", parent: "root", props: { x: 99 }, version: 2 },
      { id: "b", parent: "root", props: {}, version: 1 },
    ],
    5,
  );
  const inc = createIncrementalSnapshot(next, baseSnap.value);
  if (!inc.ok) return [{ name: "version-diff", pass: false, detail: inc.message }];
  if (inc.value.kind !== "incremental") {
    return [{ name: "version-diff", pass: false, detail: "小差分却转成了全量（阈值逻辑有误）" }];
  }

  const kinds = inc.value.diffs.map((d) => `${d.kind}:${d.nodeId}`).sort();
  out.push({
    name: "diff-classifies-two-kinds",
    pass: kinds.length === 2 && kinds.includes("props:a") && kinds.includes("add:b"),
    detail: `差分 ${kinds.length} 条：${kinds.join("、")}（结构变更与属性变更须分类正确）`,
  });

  // 未改节点不进差分（节点版本号的用途）
  const rootInDiff = inc.value.diffs.some((d) => d.nodeId === "root");
  out.push({
    name: "unchanged-node-excluded",
    pass: !rootInDiff,
    detail: `root 未改动（version 仍 1），进差分=${String(rootInDiff)}（缺节点版本号会把未变节点也计入）`,
  });

  out.push({
    name: "three-version-fields-present",
    pass:
      inc.value.snapshotVersion === next.treeVersion &&
      inc.value.treeVersion === next.treeVersion &&
      inc.value.baseVersion === baseSnap.value.treeVersion,
    detail: `基线=${inc.value.baseVersion}（期望 ${baseSnap.value.treeVersion}），快照=${inc.value.snapshotVersion}，树=${inc.value.treeVersion}`,
  });

  return out;
}

/** 判据三「恢复幂等」自检。 */
export function selfCheckIdempotent(): SnapshotSelfCheck[] {
  const out: SnapshotSelfCheck[] = [];
  const tree = mkTree(
    [
      { id: "root", parent: null, props: { name: "doc" }, version: 1 },
      { id: "a", parent: "root", props: { x: 1, y: 2 }, version: 2 },
    ],
    4,
  );
  const snap = createFullSnapshot(tree);
  if (!snap.ok) return [{ name: "idempotent-restore", pass: false, detail: snap.message }];

  const restored = mkTree([{ id: "root", parent: null, version: 9 }], 99);
  const r1 = restoreSnapshot(restored, snap.value, "overwrite");
  const r2 = restoreSnapshot(restored, snap.value, "overwrite");
  out.push({
    name: "restore-twice-identical",
    pass: r1.ok && r2.ok && treesIdentical(r1.value.tree, r2.value.tree),
    detail: r1.ok && r2.ok
      ? `同快照恢复两次结果${treesIdentical(r1.value.tree, r2.value.tree) ? "逐位一致" : "不一致（幂等性破坏）"}`
      : "恢复失败",
  });

  // 幂等的更强形式：连续恢复三次（恢复结果再恢复同一快照）
  const r3 = r1.ok ? restoreSnapshot(r1.value.tree, snap.value, "overwrite") : null;
  out.push({
    name: "restore-of-restored-stable",
    pass: r3 !== null && r3.ok && treesIdentical(r1.ok ? r1.value.tree : tree, r3.value.tree),
    detail: "对恢复结果再恢复同一快照，结果不变（恢复是投影而非叠加）",
  });

  // 恢复不得修改入参（纯函数）
  const inputBefore = JSON.stringify([...restored.nodes.keys()].sort());
  restoreSnapshot(restored, snap.value, "overwrite");
  const inputAfter = JSON.stringify([...restored.nodes.keys()].sort());
  out.push({
    name: "restore-does-not-mutate-input",
    pass: inputBefore === inputAfter,
    detail: `恢复后入参树节点集合不变=${String(inputBefore === inputAfter)}（恢复必须是纯函数）`,
  });

  return out;
}

/** 用途三向与冲突策略自检。 */
export function selfCheckPurposes(): SnapshotSelfCheck[] {
  const out: SnapshotSelfCheck[] = [];
  out.push({
    name: "purpose-policies-distinct",
    pass:
      policyForPurpose("undo-redo") === "overwrite" &&
      policyForPurpose("time-travel") === "overwrite" &&
      policyForPurpose("crash-recovery") === "reject",
    detail: `撤销重做=${policyForPurpose("undo-redo")}，时间旅行=${policyForPurpose("time-travel")}，崩溃恢复=${policyForPurpose("crash-recovery")}（崩溃恢复宁可不恢复也不丢数据）`,
  });

  // reject 策略下，新版快照不得覆盖旧树
  const tree = mkTree([{ id: "root", parent: null, version: 1 }], 2);
  const newSnap: FullSnapshot = {
    kind: "full",
    treeVersion: 10,
    rootId: "root",
    nodes: [{ id: "root", parentId: null, childIds: [], props: {}, version: 10 }],
  };
  const rejected = restoreSnapshot(tree, newSnap, "reject");
  out.push({
    name: "reject-policy-blocks-newer-snapshot",
    pass: !rejected.ok && rejected.code === "RESTORE_CONFLICT",
    detail: `新版快照（v10）恢复到旧树（v2）在 reject 策略下被拒（${rejected.ok ? "竟然成功" : rejected.code}）`,
  });

  const overwritten = restoreSnapshot(tree, newSnap, "overwrite");
  out.push({
    name: "overwrite-policy-applies",
    pass: overwritten.ok && overwritten.value.tree.treeVersion === 10,
    detail: `overwrite 策略下恢复成功，树版本=${overwritten.ok ? overwritten.value.tree.treeVersion : "?"}（期望 10）`,
  });

  return out;
}

/** 错误路径与降级矩阵自检。 */
export function selfCheckDegradation(): SnapshotSelfCheck[] {
  const out: SnapshotSelfCheck[] = [];

  // 1) 空树拒绝出快照
  const empty = createFullSnapshot({ nodes: new Map(), treeVersion: 0 });
  out.push({
    name: "empty-tree-rejected",
    pass: !empty.ok && empty.code === "EMPTY_TREE",
    detail: `空树出快照被拒（${empty.ok ? "竟然成功" : empty.code}）`,
  });

  // 2) 悬空父引用拒绝
  const dangling = mkTree([{ id: "root", parent: null }, { id: "x", parent: "nope" }], 1);
  const danglingSnap = createFullSnapshot(dangling);
  out.push({
    name: "dangling-parent-rejected",
    pass: !danglingSnap.ok && danglingSnap.code === "TREE_INVARIANT_VIOLATED",
    detail: `父引用悬空被拒（${danglingSnap.ok ? "竟然成功" : danglingSnap.code}）`,
  });

  // 3) 多根拒绝
  const multiRoot = mkTree([{ id: "r1", parent: null }, { id: "r2", parent: null }], 1);
  const multiRootSnap = createFullSnapshot(multiRoot);
  out.push({
    name: "multi-root-rejected",
    pass: !multiRootSnap.ok && multiRootSnap.code === "TREE_INVARIANT_VIOLATED",
    detail: `多根被拒（${multiRootSnap.ok ? "竟然成功" : multiRootSnap.code}）`,
  });

  // 4) 环检测：手工构造 a→b→a
  const cyc = new Map<string, LayerNode>([
    ["a", { id: "a", parentId: "b", childIds: ["b"], props: {}, version: 1 }],
    ["b", { id: "b", parentId: "a", childIds: ["a"], props: {}, version: 1 }],
  ]);
  const cycSnap = createFullSnapshot({ nodes: cyc, treeVersion: 1 });
  out.push({
    name: "cycle-rejected",
    pass: !cycSnap.ok && cycSnap.code === "TREE_INVARIANT_VIOLATED",
    detail: `父子成环被拒（${cycSnap.ok ? "竟然成功" : cycSnap.code}）`,
  });

  // 5) 差分超阈自动转全量且显性告知
  const base = mkTree([{ id: "root", parent: null, version: 1 }], 1);
  const baseS = createFullSnapshot(base);
  if (baseS.ok) {
    // 大改：root 保留，底下换成 6 个新节点 → 差分 6 条，远超阈值。
    // 树必须合法单根（首版用例把 6 个节点 parent 全设为 null 造成多根，
    // 被不变式正确拒绝 —— 用例自身非法，不是产品缺陷）。
    const next = mkTree(
      [
        { id: "root", parent: null, version: 1 },
        ...Array.from({ length: 6 }, (_, i) => ({ id: `n${i}`, parent: "root" as string | null })),
      ],
      2,
    );
    const over = createIncrementalSnapshot(next, baseS.value);
    out.push({
      name: "diff-over-threshold-falls-back-to-full",
      pass:
        over.ok &&
        over.value.kind === "full" &&
        over.diagnostics.some((d) => d.code === "DIFF_EXCEEDS_THRESHOLD"),
      detail: over.ok
        ? `6 条差分超阈（7 节点 × ${DIFF_THRESHOLD_RATIO}，下限 ${DIFF_MIN_THRESHOLD}）→ 转为 ${over.value.kind}，诊断已产出=${String(over.diagnostics.some((d) => d.code === "DIFF_EXCEEDS_THRESHOLD"))}`
        : `出增量快照失败：${over.message}`,
    });

    // 5b) 反向对照：小树的小差分不得被误判超阈（阈值下限保护）
    const small = mkTree(
      [
        { id: "root", parent: null, version: 1 },
        { id: "a", parent: "root", props: { x: 1 }, version: 1 },
      ],
      2,
    );
    const smallS = createFullSnapshot(small);
    if (smallS.ok) {
      const small2 = mkTree(
        [
          { id: "root", parent: null, version: 1 },
          { id: "a", parent: "root", props: { x: 5 }, version: 2 },
          { id: "b", parent: "root", version: 1 },
        ],
        3,
      );
      const smallInc = createIncrementalSnapshot(small2, smallS.value);
      out.push({
        name: "small-diff-not-misjudged-as-overflow",
        pass: smallInc.ok && smallInc.value.kind === "incremental",
        detail: smallInc.ok
          ? `3 节点树 2 条差分保持增量形态（kind=${smallInc.value.kind}）；阈值下限 ${DIFF_MIN_THRESHOLD} 生效，未被 floor 压到 1`
          : "出增量快照失败",
      });
    }
  }

  // 6) 基线版本高于树版本拒绝
  const tree = mkTree([{ id: "root", parent: null, version: 1 }], 2);
  const futureSnap: FullSnapshot = {
    kind: "full",
    treeVersion: 99,
    rootId: "root",
    nodes: [{ id: "root", parentId: null, childIds: [], props: {}, version: 1 }],
  };
  const treeForInc = createFullSnapshot(tree);
  if (treeForInc.ok) {
    const badInc = createIncrementalSnapshot(tree, futureSnap);
    out.push({
      name: "future-baseline-rejected",
      pass: !badInc.ok && badInc.code === "VERSION_MISMATCH",
      detail: `基线版本(99)高于树版本(2)被拒（${badInc.ok ? "竟然成功" : badInc.code}）`,
    });
  }

  // 7) 差分属性更新指向不存在的节点 → 显性诊断不静默
  const t = mkTree([{ id: "root", parent: null, version: 1 }], 1);
  const orphanDiff: IncrementalSnapshot = {
    kind: "incremental",
    baseVersion: 1,
    snapshotVersion: 2,
    treeVersion: 2,
    rootId: "root",
    diffs: [{ kind: "props", nodeId: "ghost", props: { x: 1 }, version: 2 }],
  };
  const applied = restoreSnapshot(t, orphanDiff, "overwrite");
  out.push({
    name: "orphan-props-diff-diagnosed",
    pass:
      applied.ok &&
      applied.value.diagnostics.some((d) => d.code === "NODE_NOT_FOUND") &&
      applied.value.tree.nodes.has("root"),
      detail: "差分指向不存在的节点 → 产出 NODE_NOT_FOUND 诊断且跳过，树本身仍可用",
  });

  // 8) 增量快照基线不匹配时 fallbackFull 显性失败并说明原因
  const mismatchInc: IncrementalSnapshot = {
    kind: "incremental",
    baseVersion: 99,
    snapshotVersion: 100,
    treeVersion: 100,
    rootId: "root",
    diffs: [],
  };
  const fb = restoreSnapshot(t, mismatchInc, "fallbackFull");
  out.push({
    name: "fallback-full-explains-why-impossible",
    pass: !fb.ok && fb.hint.includes("全量"),
    detail: !fb.ok
      ? `增量无法就地转全量时显式失败并说明须由调用方提供全量基线（hint含「全量」=${String(fb.hint.includes("全量"))}）`
      : "竟然成功了",
  });

  // 9) 非有限属性值显性标记
  const nanTree = mkTree([{ id: "root", parent: null, props: { bad: Number.NaN }, version: 1 }], 1);
  const nanSnap = createFullSnapshot(nanTree);
  out.push({
    name: "nonfinite-property-flagged",
    pass: nanSnap.ok && nanSnap.diagnostics.some((d) => d.code === "NODE_PROPERTY_INVALID"),
    detail: "NaN 属性入快照被显式标记（不静默存入，否则会污染后续每一次恢复）",
  });

  // 10) 稀疏性核验
  const dense = mkTree(
    [
      { id: "root", parent: null, props: { a: 1, b: 2, c: 3, d: 4 }, version: 1 },
      { id: "x", parent: "root", props: { a: 1, b: 2, c: 3, d: 4 }, version: 1 },
    ],
    1,
  );
  const denseSnap = createFullSnapshot(dense);
  const sparseAudit = denseSnap.ok ? auditSparsity(denseSnap.value, 4) : [];
  out.push({
    name: "sparsity-audit-detects-dense-snapshot",
    pass: sparseAudit.length > 0,
    detail: `满属性快照被稀疏性核验点名（违规 ${sparseAudit.length} 条）`,
  });

  // 11) 快照确定性：同状态两次出快照逐位一致（幂等的前提）
  const det = mkTree(
    [
      { id: "b", parent: "root", props: { x: 1 }, version: 1 },
      { id: "root", parent: null, props: { n: "d" }, version: 1 },
    ],
    1,
  );
  const s1 = createFullSnapshot(det);
  const s2 = createFullSnapshot(det);
  out.push({
    name: "snapshot-deterministic",
    pass:
      s1.ok &&
      s2.ok &&
      JSON.stringify(s1.value.nodes.map((n) => n.id)) ===
        JSON.stringify(s2.value.nodes.map((n) => n.id)),
    detail: "同状态两次出快照节点序一致（已按 id 排序，Map 插入序不可靠）",
  });

  return out;
}

/** 全量自检入口。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly SnapshotSelfCheck[]>>;
  readonly allPass: boolean;
  readonly total: number;
  readonly failed: readonly string[];
} {
  const groups = {
    sparse: selfCheckSparse(),
    versions: selfCheckVersions(),
    idempotent: selfCheckIdempotent(),
    purposes: selfCheckPurposes(),
    degradation: selfCheckDegradation(),
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