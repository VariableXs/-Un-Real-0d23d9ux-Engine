/**
 * VE-F0612 · 图层树序列化（D 域 · 2D 合成引擎图层树组 · 批次 D01）
 * ---------------------------------------------------------------------------
 * 职责定位：图层树序列化——把活着的图层树变成**可持久化、可交换**的字节或文本。
 * 本条同时提供两种格式，因为它们服务的消费者根本不同：
 *   · 树形缩进文本：给人读。调试时看一眼就知道树的形状与属性，
 *     下游 F0618 调试可视化直接消费本格式（转储），不需要二次解析。
 *   · 紧凑二进制：给机器读。大树（万层级 F0616）的文本格式体积是二进制的
 *     数倍，落盘与网络交换必须走二进制。
 * 二者**语义等价且可互转**：文本 → 二进制 → 文本 后逐字节一致，
 * 这是本条最核心的可验证承诺（判据四「互转无损」）。
 *
 * 上游 F0601 图层树结构（单父、无环、类型约束——本条的前置断言对象）、
 *       F0610 图层树快照与恢复（内存态完整复制，本条的持久化对偶）。
 * 下游 F0618 图层树调试可视化（文本格式的消费方）。
 *
 * 锚点契约（五条，逐条对应判据）：
 *   1. 双格式：文本（人读，缩进树）与二进制（机读，紧凑）语义等价可互转。
 *      「等价」的判定方式不是眼看，而是走 canonical 规范化后逐字节比对（见 §8）。
 *   2. 环防护先行：**序列化之前**必须先过树不变式断言（F0601 单父、无环、
 *      根唯一、父引用存在、子引用一致）。断言失败即拒绝序列化并产出三要素告警。
 *      顺序不可调换——先序列化再校验等于让脏树进了字节流，污染面从内存扩大到磁盘。
 *   3. 格式版本化：两种格式都带版本头；二进制另有 magic 与逐节点 extraCount
 *      容器。**前向兼容承诺**：老读者读新档不炸——未知的头字段、节点 token、
 *      节点尾部扩展字段一律**按长度跳过**而非报错（§7）。
 *   4. 与 F0610 快照的分工：快照是**内存态**完整复制（活树与快照之间不得有
 *      引用共享，否则改快照会改活树），序列化是**可持久化**格式。
 *      两者互转走显式的树转换函数（§9），不允许把序列化产物直接当快照用。
 *   5. 降级矩阵：不变式失败 → 拒绝并告警；版本不识别 → 按显式策略
 *      （跳过后继续 / 显性失败），不猜；编码异常 → 逐字段校验，出错即定位到
 *      具体字段而不是笼统的「解码失败」。
 *
 * 六条容易做错、故显式记录的设计立场：
 *
 *   一、文本格式用缩进表达层级，但每行仍带 id——不能只靠缩进还原结构。
 *     纯缩进格式（像 .gitignore 那样）在**缩进被编辑器改动后**结构即失真，
 *     且无法校验「我以为的父子关系」与「树里实际的父子关系」是否一致。
 *     带 id 后，解析出的父子关系要与节点自带的 childIds 交叉核对，
 *     不一致即 BIN/TEXT 结构违例——这让格式篡改可被发现，而不是被默默接受。
 *
 *   二、文本属性的类型必须显式标注，不能靠「看起来像数字」猜。
 *     若写作 `opacity=0.5` 与 `label=0.5`，解析时无法区分字符串 "0.5" 与数值 0.5，
 *     互转就不是无损的。故用三条不同前缀（num/str/bool）显式携带类型。
 *
 *   三、数值 -0 必须无损，JSON 数字字面量做不到这件事。
 *     `JSON.stringify(-0)` 得 "0"，round-trip 后 -0 变 +0。
 *     判定 `Object.is(v, -0)` 会变 false——对「-0 与 0 在除法符号上有别」
 *     的场景这是可观测的语义漂移。故文本格式对 -0 显式写 `-0`，
 *     二进制走 IEEE-754 f64 原生位型（天然无损）。
 *
 *   四、二进制的节点关系用**索引**而非 id 字符串表达。
 *     每个 id 字符串在节点表里只存一份，父子/子代关系走 varint 索引，
 *     这是「紧凑」的主要来源，也让 O(1) 校验成为可能（父索引越界即结构违例，
 *     不必扫全表找字符串）。
 *
 *   五、前向兼容的跳过必须「按长度」，不能靠「猜下一个 token 是什么」。
 *     文本格式靠 token 自带 key=value 形态跳过；二进制靠 extraCount + 每项的
 *     payloadLen 跳过。二者都不依赖对未来格式的先验知识——这是承诺能兑现的前提。
 *
 *   六、非有限数值（NaN / ±Infinity）不进格式，逐字段拒绝。
 *     NaN 在文本里没有可回读的字面量，Infinity 在 JSON 里是 null，
 *     两者都会造成「写出去再读回来语义变了」。F0610 已把非有限值挡在快照外，
 *     本条在编码侧同样拦住，并给出可定位到属性键的诊断。
 *
 * 零静默纪律：不变式违例、版本不识别、字段越界、非有限值、深度与规模超阈
 * 全部产出 Diagnostic（code + message + hint），降级一律显性不做暗转。
 *
 * 判据：双格式、环防护先行、前向兼容、互转无损。
 * 依赖锚点：F0601 图层树结构（不变式三条）、F0610 图层树快照与恢复（内存态对偶）、
 *          F0606 Z 序（childIds 顺序即 Z 序，本条必须保序）。
 * 交接说明：本条是**零运行时契约层**——纯函数 + 常量表 + 诊断袋，
 *          不引入任何 IO（不碰 fs、不碰网络）。落盘由调用方负责，
 *          本条只交出 Uint8Array 与 string。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型
// ════════════════════════════════════════════════════════════════════════════

/** 序列化专属诊断码（与 F0610/F0611 各自独立，不共用枚举）。 */
export type SerDiagCode =
  /** 树不变式违例（单父/无环/根唯一/父引用/子引用）——环防护先行拦下的。 */
  | "TREE_INVARIANT_VIOLATED"
  /** 格式头不识别（magic 不符 / 主版本过高 / 版本头缺字段）。 */
  | "FORMAT_VERSION_UNSUPPORTED"
  /** 前向兼容跳过：读到不认识的字段或扩展项（不是错误，是承诺兑现的痕迹）。 */
  | "UNKNOWN_FIELD_SKIPPED"
  /** 编码/解码字段级错误（越界、字面量非法、类型标记未知）。 */
  | "MALFORMED_FIELD"
  /** 属性值非法（非有限数、类型不受支持）。 */
  | "PROPERTY_INVALID"
  /** 深度或规模超阈（登记但不截断）。 */
  | "SCALE_EXCEEDS_THRESHOLD"
  /** 空树（无节点，无根可序列化）。 */
  | "EMPTY_TREE"
  /** 结构交叉核对失败（文本缩进还原的父子关系与 childIds 不一致）。 */
  | "STRUCTURE_MISMATCH"
  /** 与 F0610 快照互转时的结构不兼容。 */
  | "SNAPSHOT_BRIDGE_INCOMPATIBLE";

/** 一条诊断：发生了什么、影响什么、下一步怎么办。 */
export interface SerDiagnostic {
  readonly code: SerDiagCode;
  readonly message: string;
  readonly hint: string;
}

/** 结果判别联合（失败时三要素随结果一起返回，不藏在日志里）。 */
export type SerOutcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly SerDiagnostic[] }
  | {
      readonly ok: false;
      readonly code: SerDiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly SerDiagnostic[];
    };

/** 成功构造。 */
export function serOk<T>(value: T, diagnostics: readonly SerDiagnostic[] = []): SerOutcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造。 */
export function serFail<T>(
  code: SerDiagCode,
  message: string,
  hint: string,
  diagnostics: readonly SerDiagnostic[] = [],
): SerOutcome<T> {
  const d: SerDiagnostic = { code, message: message || "（未提供描述）", hint: hint || "（未提供处置建议）" };
  return { ok: false, code, message, hint, diagnostics: [...diagnostics, d] };
}

/** 诊断聚合器。 */
export class SerDiagBag {
  private readonly items: SerDiagnostic[] = [];

  push(code: SerDiagCode, message: string, hint: string): void {
    this.items.push({ code, message: message || "（未提供描述）", hint: hint || "（未提供处置建议）" });
  }

  get size(): number {
    return this.items.length;
  }

  all(): readonly SerDiagnostic[] {
    return this.items.slice();
  }

  byCode(code: SerDiagCode): readonly SerDiagnostic[] {
    return this.items.filter((d) => d.code === code);
  }

  has(code: SerDiagCode): boolean {
    return this.items.some((d) => d.code === code);
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 图层树的数据形态（本条的输入 / 输出结构，与 F0601、F0610 同形）
// ════════════════════════════════════════════════════════════════════════════

/** 属性值：与 F0610 快照、F0608 插值共用同一形态（跨条一致，不另立类型）。 */
export type SerPropValue = number | string | boolean;

/** 节点类型标签白名单（与 F0601 类型约束、F0610 校验对齐，本条只校验不定义）。 */
export const NODE_TAGS: readonly string[] = ["root", "group", "text", "image", "vector", "adjustment"];

/** 图层节点：稳定 id + 单父引用 + 有序子序列 + 稀疏属性表。 */
export interface SerNode {
  /** 稳定 id（跨序列化不变——外部句柄依赖它）。 */
  readonly id: string;
  /** 父节点 id；根为 null。 */
  readonly parentId: string | null;
  /** 子节点 id 序列，**顺序即 Z 序**（F0606）——序列化必须保序。 */
  readonly childIds: readonly string[];
  /** 稀疏属性表：只含显式设置过的项（未设属性走继承，见 F0601）。 */
  readonly props: Readonly<Record<string, SerPropValue>>;
  /** 节点版本（F0610 三方版本号之一）。 */
  readonly version: number;
  /** 类型标签；不在白名单内视为自定义标签（登记而不拒绝）。 */
  readonly tag: string;
}

/** 图层树：节点表 + 根 id + 树全局版本。 */
export interface SerTree {
  readonly nodes: ReadonlyMap<string, SerNode>;
  readonly rootId: string;
  /** 树全局版本（F0610 三方版本号之一）。 */
  readonly treeVersion: number;
}

// ════════════════════════════════════════════════════════════════════════════
// §3 前置不变式断言（判据二：环防护先行）
// ════════════════════════════════════════════════════════════════════════════

/** 不变式断言的通过凭证：只有拿到它才允许进入编码阶段。 */
export interface InvariantTicket {
  /** 根节点 id（断言时确定，编码器不再自行寻找）。 */
  readonly rootId: string;
  /** 节点总数（断言时统计，用于与编码结果对账）。 */
  readonly nodeCount: number;
  /** 最大深度（根为深度 0），供规模登记使用。 */
  readonly maxDepth: number;
}

/** 树规模登记阈值：超过只登记诊断，不截断、不拒绝（截断会让交换语义失真）。 */
export const NODE_SCALE_THRESHOLD = 20000;

/** 深度登记阈值。 */
export const DEPTH_SCALE_THRESHOLD = 512;

/**
 * 序列化前置不变式断言（F0601 三条 + 一致性两条）。
 *
 * 顺序刻意如此：先查结构（空、根唯一、父存在、单父、无环、子引用自洽），
 * 再算深度。理由是深度计算需要遍历，而遍历在有环树上会不终止——
 * 把「无环」放在遍历之前，才敢用遍历做统计。
 *
 * 失败一律**拒绝**并给三要素，不降级为「跳过坏节点」：脏树序列化出去，
 * 读回来就是脏树，且脏在哪已经不可考。
 */
export function assertSerializable(tree: SerTree, bag: SerDiagBag): InvariantTicket | null {
  const size = tree.nodes.size;
  if (size === 0) {
    bag.push(
      "EMPTY_TREE",
      "图层树没有任何节点",
      "空树无可序列化的根；请等树初始化完成，若确实要支持空树请在调用方显式约定空树表示",
    );
    return null;
  }

  // ① 根唯一且存在：parentId === null 的恰好一个，且等于声明的 rootId
  let declaredRoot: string | null = null;
  for (const node of tree.nodes.values()) {
    if (node.parentId === null) {
      if (declaredRoot !== null) {
        bag.push(
          "TREE_INVARIANT_VIOLATED",
          `存在多个根节点（${declaredRoot} 与 ${node.id} 的 parentId 均为 null）`,
          "多根说明父子关系已断链；请先修复树结构，否则序列化后无法确定还原起点",
        );
        return null;
      }
      declaredRoot = node.id;
    }
  }
  if (declaredRoot === null) {
    bag.push(
      "TREE_INVARIANT_VIOLATED",
      "树中没有 parentId 为 null 的根节点",
      "无根即成环；请检查是否有节点互指为父 forming 环，修复后再序列化",
    );
    return null;
  }
  if (declaredRoot !== tree.rootId) {
    bag.push(
      "TREE_INVARIANT_VIOLATED",
      `声明的根 ${tree.rootId} 与实际根 ${declaredRoot} 不一致`,
      "rootId 与父子关系必须自洽；请以实际根为准修正声明，不要改父子关系去迁就声明",
    );
    return null;
  }
  const rootNode = tree.nodes.get(tree.rootId);
  if (rootNode === undefined) {
    bag.push(
      "TREE_INVARIANT_VIOLATED",
      `声明的根 ${tree.rootId} 不在节点表中`,
      "根必须存在于节点表；请先修复树结构再序列化",
    );
    return null;
  }

  // ② 父引用存在 + ③ 单父无环：沿父链上溯，步数超过节点数即成环
  for (const node of tree.nodes.values()) {
    if (node.parentId !== null && !tree.nodes.has(node.parentId)) {
      bag.push(
        "TREE_INVARIANT_VIOLATED",
        `节点 ${node.id} 的父节点 ${node.parentId} 不在节点表中`,
        "父引用悬空；请先修复父子关系，序列化拒绝保存不连通的树",
      );
      return null;
    }
    let steps = 0;
    let cursor: SerNode | undefined = node;
    while (cursor !== undefined && cursor.parentId !== null) {
      cursor = tree.nodes.get(cursor.parentId);
      steps += 1;
      if (steps > size) {
        bag.push(
          "TREE_INVARIANT_VIOLATED",
          `节点 ${node.id} 沿父链上溯超过 ${size} 步，判定成环`,
          "成环树不可序列化（还原必然不终止）；请断开环上的一处父引用",
        );
        return null;
      }
    }
  }

  // ④ 子引用自洽：childIds 里的节点必须存在，且其 parentId 必须指回本节点
  //    ——这条同时抓「同一子节点被两个父挂载」（实质是单父违例的另一副面孔）
  const mounted = new Set<string>();
  for (const node of tree.nodes.values()) {
    for (const childId of node.childIds) {
      const child = tree.nodes.get(childId);
      if (child === undefined) {
        bag.push(
          "TREE_INVARIANT_VIOLATED",
          `节点 ${node.id} 的子节点 ${childId} 不在节点表中`,
          "子引用悬空；请修复 childIds 序列，序列化拒绝输出不连通的树",
        );
        return null;
      }
      if (child.parentId !== node.id) {
        bag.push(
          "TREE_INVARIANT_VIOLATED",
          `节点 ${childId} 同时出现在 ${node.id} 的 childIds 中，但其 parentId 为 ${String(child.parentId)}`,
          "父子双向引用必须一致；单父约束要求一个节点只能被一个父挂载，请择一修正",
        );
        return null;
      }
      if (mounted.has(childId)) {
        bag.push(
          "TREE_INVARIANT_VIOLATED",
          `节点 ${childId} 被重复挂载（至少两个父的 childIds 都含它）`,
          "违反单父约束；请检查是否有节点被两个组同时引用",
        );
        return null;
      }
      mounted.add(childId);
    }
  }

  // ⑤ 可达性：从根出发必须能走到全部节点（无游离子树）
  let reached = 0;
  let maxDepth = 0;
  const stack: Array<{ id: string; depth: number }> = [{ id: tree.rootId, depth: 0 }];
  const visited = new Set<string>();
  while (stack.length > 0) {
    const frame = stack.pop();
    if (frame === undefined) break;
    if (visited.has(frame.id)) continue;
    visited.add(frame.id);
    reached += 1;
    if (frame.depth > maxDepth) maxDepth = frame.depth;
    const node = tree.nodes.get(frame.id);
    if (node === undefined) continue;
    // 逆序入栈，保证弹出序与 childIds 顺序一致（保序遍历，供编码器复用）
    for (let i = node.childIds.length - 1; i >= 0; i -= 1) {
      const childId = node.childIds[i];
      if (childId !== undefined) stack.push({ id: childId, depth: frame.depth + 1 });
    }
  }
  if (reached !== size) {
    bag.push(
      "TREE_INVARIANT_VIOLATED",
      `从根可达 ${reached} 个节点，节点表共 ${size} 个，存在游离子树`,
      "游离子树在还原时会凭空出现；请把所有节点挂到根链上，或从节点表移除它们",
    );
    return null;
  }

  // ⑥ 规模登记（不截断、不拒绝——截断会让交换语义静默失真）
  if (size > NODE_SCALE_THRESHOLD) {
    bag.push(
      "SCALE_EXCEEDS_THRESHOLD",
      `节点数 ${size} 超过登记阈值 ${NODE_SCALE_THRESHOLD}`,
      "大树序列化耗时与产物体积显著上升；建议落盘走二进制格式，文本格式仅用于调试抽样",
    );
  }
  if (maxDepth > DEPTH_SCALE_THRESHOLD) {
    bag.push(
      "SCALE_EXCEEDS_THRESHOLD",
      `树深 ${maxDepth} 超过登记阈值 ${DEPTH_SCALE_THRESHOLD}`,
      "深层树建议核查是否有异常嵌套；必要时用 F0616 扁平化后再序列化",
    );
  }

  return { rootId: tree.rootId, nodeCount: size, maxDepth };
}

/**
 * 保序先序遍历（编码器的唯一入口）。
 * 断言已通过时不会重复失败；未通过时返回空数组由调用方负责拦截。
 */
export function preorderIds(tree: SerTree, ticket: InvariantTicket): readonly string[] {
  const out: string[] = [];
  const stack: string[] = [ticket.rootId];
  const guard = tree.nodes.size + 1;
  let steps = 0;
  while (stack.length > 0) {
    const id = stack.pop();
    if (id === undefined) break;
    steps += 1;
    if (steps > guard) return [];
    out.push(id);
    const node = tree.nodes.get(id);
    if (node === undefined) continue;
    for (let i = node.childIds.length - 1; i >= 0; i -= 1) {
      const childId = node.childIds[i];
      if (childId !== undefined) stack.push(childId);
    }
  }
  return out;
}

/** 求节点深度（根为 0）；节点不存在返回 -1。 */
export function depthOf(tree: SerTree, id: string): number {
  let depth = 0;
  let cursor: SerNode | undefined = tree.nodes.get(id);
  let guard = tree.nodes.size + 1;
  while (cursor !== undefined && cursor.parentId !== null) {
    const parent: SerNode | undefined = tree.nodes.get(cursor.parentId);
    if (parent === undefined) return -1;
    cursor = parent;
    depth += 1;
    guard -= 1;
    if (guard <= 0) return -1;
  }
  return depth;
}

// ════════════════════════════════════════════════════════════════════════════
// §4 UTF-8 编解码（零依赖自实现，保证任意宿主环境行为一致）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 字符串 → UTF-8 字节。
 * 自实现而非依赖 TextEncoder：宿主可能是 Worker、Node 或精简渲染环境，
 * 三者对 TextEncoder 的存在性与默认编码行为历史上并不一致，
 * 而「同一份文本在任何宿主编出同一串字节」是交换格式的前提。
 * 孤立代理项按 U+FFFD 替换（与 WHATWG 一致），保证往返不抛。
 */
export function utf8Encode(text: string): Uint8Array {
  const out: number[] = [];
  for (let i = 0; i < text.length; i += 1) {
    let cp = text.charCodeAt(i);
    if (cp >= 0xd800 && cp <= 0xdbff) {
      const next = i + 1 < text.length ? text.charCodeAt(i + 1) : 0;
      if (next >= 0xdc00 && next <= 0xdfff) {
        cp = 0x10000 + ((cp - 0xd800) << 10) + (next - 0xdc00);
        i += 1;
      } else {
        cp = 0xfffd;
      }
    } else if (cp >= 0xdc00 && cp <= 0xdfff) {
      cp = 0xfffd;
    }
    if (cp < 0x80) {
      out.push(cp);
    } else if (cp < 0x800) {
      out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
    } else if (cp < 0x10000) {
      out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
    } else {
      out.push(
        0xf0 | (cp >> 18),
        0x80 | ((cp >> 12) & 0x3f),
        0x80 | ((cp >> 6) & 0x3f),
        0x80 | (cp & 0x3f),
      );
    }
  }
  return new Uint8Array(out);
}

/** UTF-8 字节 → 字符串；非法序列按 U+FFFD 替换，不抛（逐字段容错）。 */
export function utf8Decode(bytes: Uint8Array): string {
  let out = "";
  let i = 0;
  while (i < bytes.length) {
    const b0 = bytes[i] as number;
    let cp: number;
    let size: number;
    if (b0 < 0x80) {
      cp = b0;
      size = 1;
    } else if ((b0 & 0xe0) === 0xc0) {
      cp = b0 & 0x1f;
      size = 2;
    } else if ((b0 & 0xf0) === 0xe0) {
      cp = b0 & 0x0f;
      size = 3;
    } else if ((b0 & 0xf8) === 0xf0) {
      cp = b0 & 0x07;
      size = 4;
    } else {
      out += "�";
      i += 1;
      continue;
    }
    if (i + size > bytes.length) {
      out += "�";
      i += 1;
      continue;
    }
    let valid = true;
    for (let k = 1; k < size; k += 1) {
      const bk = bytes[i + k] as number;
      if ((bk & 0xc0) !== 0x80) {
        valid = false;
        break;
      }
      cp = (cp << 6) | (bk & 0x3f);
    }
    if (!valid) {
      out += "�";
      i += 1;
      continue;
    }
    out += String.fromCodePoint(cp);
    i += size;
  }
  return out;
}

// ════════════════════════════════════════════════════════════════════════════
// §5 文本格式（判据一之半：人读；下游 F0618 的消费格式）
// ════════════════════════════════════════════════════════════════════════════

/** 文本格式版本头的主版本号。 */
export const TEXT_FORMAT_MAJOR = 1;

/** 文本格式行尾（固定 \n，跨平台一致——不随宿主换行符变化，否则互转会出平台差）。 */
const NL = "\n";

/** 缩进步长（两个空格；不用 \t，避免不同编辑器 tab 宽度设置改写结构）。 */
const INDENT = "  ";

/** 属性行的三条类型前缀（立场二：类型显式，不靠字面量猜）。 */
export const PROP_PREFIX_NUM = "num";
export const PROP_PREFIX_STR = "str";
export const PROP_PREFIX_BOOL = "bool";

/** 自定义标签的 token 键：标签不在白名单内时以 tag=custom custom="..." 形式落文本。 */
const CUSTOM_TAG_KEY = "custom";

/** tag= 的哨兵值：表示标签为自定义，具体值由紧随其后的 custom="..." 给出。 */
const CUSTOM_TAG_SENTINEL = "custom";

/** 数值文本化：-0 显式保形（立场三），其余走 JS 默认字面量。 */
function numberToLiteral(v: number): string {
  if (Object.is(v, -0)) return "-0";
  if (!Number.isFinite(v)) {
    // 非有限值在编码侧已被拒绝；此处仅为类型完备兜底，绝不静默改写语义。
    return String(v);
  }
  return String(v);
}

/** 文本字面量解析：仅接受有限数值字面量，-0 显式还原为 -0。 */
function literalToNumber(text: string): number | null {
  const t = text.trim();
  if (t === "-0") return -0;
  if (t === "") return null;
  const v = Number(t);
  if (!Number.isFinite(v)) return null;
  // 拒绝 Number() 会宽松接受的非数字形态（"0x10"/"1e"/"Infinity"/""）
  if (!/^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$/.test(t)) return null;
  return v;
}

/**
 * 文本编码（O(节点数)）。
 * 流程严格是「先断言后编码」——assertSerializable 失败即返回 null，
 * 不产出任何部分文本（半截文本进文件比没有文本更坏）。
 */
export function encodeText(tree: SerTree, bag: SerDiagBag): string | null {
  const ticket = assertSerializable(tree, bag);
  if (ticket === null) return null;

  const order = preorderIds(tree, ticket);
  if (order.length !== ticket.nodeCount) {
    bag.push(
      "STRUCTURE_MISMATCH",
      `保序遍历得到 ${order.length} 个节点，不变式断言统计为 ${ticket.nodeCount}`,
      "遍历与断言口径不一致说明结构在断言后被改动；请勿在序列化过程中并发修改树",
    );
    return null;
  }

  const lines: string[] = [];
  lines.push(`#VLT/${TEXT_FORMAT_MAJOR} tree=${tree.treeVersion} nodes=${ticket.nodeCount} root=${JSON.stringify(tree.rootId)}`);
  lines.push(`#depth=${ticket.maxDepth}`);

  for (const id of order) {
    const node = tree.nodes.get(id);
    if (node === undefined) continue;
    const depth = depthOf(tree, id);
    const indent = INDENT.repeat(Math.max(0, depth));
    const tagPart = NODE_TAGS.includes(node.tag)
      ? `tag=${node.tag}`
      : `tag=${CUSTOM_TAG_SENTINEL} ${CUSTOM_TAG_KEY}=${JSON.stringify(node.tag)}`;
    lines.push(`${indent}node id=${JSON.stringify(node.id)} ${tagPart} v=${node.version}`);

    // 稀疏属性：只输出有效集（F0601 稀疏性在格式层的直译）
    for (const key of Object.keys(node.props).sort()) {
      const value = node.props[key];
      if (value === undefined) continue;
      const rendered = renderProp(key, value, node.id, bag);
      if (rendered === null) return null;
      lines.push(`${indent}${INDENT}${rendered}`);
    }
  }
  return lines.join(NL) + NL;
}

/** 单条属性行渲染；类型不受支持或值非法时产出诊断并返回 null（逐字段拒绝）。 */
function renderProp(key: string, value: SerPropValue, nodeId: string, bag: SerDiagBag): string | null {
  const k = JSON.stringify(key);
  switch (typeof value) {
    case "number":
      if (!Number.isFinite(value)) {
        bag.push(
          "PROPERTY_INVALID",
          `节点 ${nodeId} 的属性 ${key} 值为非有限数（${String(value)}）`,
          "NaN/Infinity 在文本格式中没有可回读字面量；请先在业务层将其归一或剔除",
        );
        return null;
      }
      return `${PROP_PREFIX_NUM} ${k}=${numberToLiteral(value)}`;
    case "string":
      return `${PROP_PREFIX_STR} ${k}=${JSON.stringify(value)}`;
    case "boolean":
      return `${PROP_PREFIX_BOOL} ${k}=${value ? "true" : "false"}`;
    default:
      bag.push(
        "PROPERTY_INVALID",
        `节点 ${nodeId} 的属性 ${key} 类型不受支持（${typeof value}）`,
        "序列化仅支持 number/string/boolean；请把其他类型转成字符串或数值后再入树",
      );
      return null;
  }
}

/** 头字段解析结果：未知 key 走「跳过并登记」（前向兼容的第一道关）。 */
interface TextHeader {
  readonly major: number;
  readonly treeVersion: number;
  readonly nodeCount: number;
  readonly rootId: string;
  readonly depth: number;
}

/** 从版本头行解析字段；未知 key 逐个跳过。 */
function parseHeader(line: string, bag: SerDiagBag): TextHeader | null {
  const m = /^#VLT\/(\d+)\s+(.*)$/.exec(line);
  if (m === null) {
    bag.push(
      "FORMAT_VERSION_UNSUPPORTED",
      "文本首行不是合法的 VLT 版本头",
      "文件可能不是本格式产物，或首行已被编辑器改写；请确认来源后再解析",
    );
    return null;
  }
  const major = Number(m[1]);
  const rest = m[2] ?? "";
  let treeVersion = 0;
  let nodeCount = -1;
  let rootId = "";
  let sawRoot = false;
  let depth = -1;
  for (const token of rest.split(/\s+/)) {
    if (token === "") continue;
    const eq = token.indexOf("=");
    if (eq <= 0) {
      bag.push(
        "UNKNOWN_FIELD_SKIPPED",
        `版本头中的裸 token「${token}」无法解析为 key=value`,
        "按前向兼容约定跳过；若该字段承载必要语义，请检查文件是否被手工编辑过",
      );
      continue;
    }
    const key = token.slice(0, eq);
    const value = token.slice(eq + 1);
    switch (key) {
      case "tree":
        treeVersion = Number(value);
        break;
      case "nodes":
        nodeCount = Number(value);
        break;
      case "root": {
        // 空串 root 是合法值（配合空串 id），故用独立的「是否见到」标志判定，
        // 不能用 rootId === "" 反推缺失——那会把合法空串误杀。
        const parsedRoot = parseJsonString(value);
        if (parsedRoot === null) {
          bag.push(
            "MALFORMED_FIELD",
            `版本头 root 字段不是合法字符串字面量（${value}）`,
            "root 需为 JSON 字符串字面量；空串根写作 root=\"\"",
          );
          return null;
        }
        rootId = parsedRoot;
        sawRoot = true;
        break;
      }
      case "depth":
        depth = Number(value);
        break;
      default:
        bag.push(
          "UNKNOWN_FIELD_SKIPPED",
          `版本头字段 ${key} 为未知字段（当前格式不定义）`,
          "按前向兼容约定跳过——这正是「老读者读新档不炸」的兑现路径",
        );
        break;
    }
  }
  if (major > TEXT_FORMAT_MAJOR) {
    bag.push(
      "FORMAT_VERSION_UNSUPPORTED",
      `文本格式主版本 ${major} 高于本实现支持的 ${TEXT_FORMAT_MAJOR}`,
      "跨主版本不保证字段语义不变；请升级读取端实现，或由写入端降级输出旧主版本",
    );
    return null;
  }
  if (!Number.isFinite(treeVersion) || !sawRoot || nodeCount < 0) {
    bag.push(
      "MALFORMED_FIELD",
      "版本头缺少必要字段（tree / nodes / root）",
      "版本头是格式的身份声明；缺字段说明文件被截断或非本格式，请勿猜测补齐（root=\"\" 是合法值，不算缺失）",
    );
    return null;
  }
  return { major, treeVersion, nodeCount, rootId, depth };
}

/** JSON 字符串字面量解析；失败返回 null（空串是合法值，不能用它兼作失败信号）。 */
function parseJsonString(token: string): string | null {
  if (token.length < 2) return null;
  if (!token.startsWith("\"")) return null;
  try {
    const v: unknown = JSON.parse(token);
    return typeof v === "string" ? v : null;
  } catch {
    return null;
  }
}

/**
 * 文本解码（O(节点数)）。
 * 三层校验，缺一不可：
 *   ① 版本头合法且主版本可识别；
 *   ② 逐行逐字段语法合法（类型前缀匹配、字面量可解析）；
 *   ③ **结构交叉核对**：缩进还原的父子关系必须与节点 childIds 一致（立场一），
 *      且实际节点数必须等于版本头声明的 nodes 数。
 */
export function decodeText(text: string, bag: SerDiagBag): SerOutcome<SerTree> {
  const rawLines = text.split(/\r\n|\r|\n/);
  const headerLine = rawLines[0];
  if (headerLine === undefined) {
    return serFail("FORMAT_VERSION_UNSUPPORTED", "文本为空，无版本头", "请提供由 encodeText 产出的文本");
  }
  const header = parseHeader(headerLine, bag);
  if (header === null) {
    const first = bag.all()[0];
    return serFail(
      first?.code ?? "FORMAT_VERSION_UNSUPPORTED",
      first?.message ?? "版本头解析失败",
      first?.hint ?? "请检查文件来源",
      bag.all(),
    );
  }

  // 单遍栈式解析：栈的第 d 层保存深度为 d 的祖先 id。
  // 子节点归属由「栈顶即父」直接得出，不再事后反查——反查在乱序文本上会错配。
  const nodes = new Map<string, SerNode>();
  const childOrder = new Map<string, string[]>();
  const ancestry: string[] = [];
  let current: { id: string; version: number; tag: string; props: Record<string, SerPropValue> } | null = null;
  let currentDepth = -1;

  /**
   * 收束当前节点：确定其 parentId 与父的 childIds，然后入表。
   * 返回 false 表示id 重复——调用方必须**立即失败**，不可只登记诊断后继续：
   * 继续的后果是重复节点被丢弃，而头部 nodes 声明恰好等于「行数」，
   * 于是节点数校验会通过，一个被静默吞掉的节点就此消失在还原结果里。
   */
  const close = (): boolean => {
    if (current === null) return true;
    const parentId = ancestry.length > 1 ? (ancestry[ancestry.length - 2] as string) : null;
    if (nodes.has(current.id)) {
      bag.push(
        "STRUCTURE_MISMATCH",
        `节点 id 重复出现（${current.id}）`,
        "id 必须全局唯一（外部句柄依赖它）；请检查文件是否被拼接或手工复制",
      );
      return false;
    }
    nodes.set(current.id, {
      id: current.id,
      parentId,
      childIds: [],
      props: current.props,
      version: current.version,
      tag: current.tag,
    });
    if (parentId !== null) {
      const list = childOrder.get(parentId) ?? [];
      list.push(current.id);
      childOrder.set(parentId, list);
    }
    current = null;
    return true;
  };

  for (let i = 1; i < rawLines.length; i += 1) {
    const line = rawLines[i] as string;
    const trimmed = line.trim();
    if (trimmed === "") continue;
    if (trimmed.startsWith("#")) {
      const cm = /^#([a-zA-Z]+)=(.*)$/.exec(trimmed);
      if (cm !== null && cm[1] !== "depth") {
        bag.push("UNKNOWN_FIELD_SKIPPED", `注释行携带了未知键 ${cm[1]}`, "注释按约定无语义，跳过即可");
      }
      continue;
    }
    const leading = line.length - line.trimStart().length;
    if (leading % INDENT.length !== 0) {
      bag.push(
        "MALFORMED_FIELD",
        `第 ${i + 1} 行缩进不是 ${INDENT.length} 的整数倍`,
        "缩进承载层级语义；请确认文件未被使用 tab 或混合缩进的编辑器改写",
      );
      return serFail("MALFORMED_FIELD", `第 ${i + 1} 行缩进非法`, "请检查缩进一致性", bag.all());
    }
    const depth = Math.floor(leading / INDENT.length);

    if (trimmed.startsWith("node ")) {
      if (!close()) {
        return serFail("STRUCTURE_MISMATCH", "节点 id 重复", "id 必须全局唯一，请检查文件", bag.all());
      }
      const fields = parseNodeLine(trimmed, i + 1, bag);
      if (fields === null) {
        return serFail("MALFORMED_FIELD", `第 ${i + 1} 行节点定义解析失败`, "请检查该行语法", bag.all());
      }
      // 缩进跳跃（depth 比栈深+1 还大）说明层级被改坏，不能默默接受
      if (depth > ancestry.length) {
        bag.push(
          "STRUCTURE_MISMATCH",
          `第 ${i + 1} 行节点深度 ${depth} 超过已有层级 ${ancestry.length}`,
          "缩进层级出现跳跃，父子关系无法确定；请检查文件是否被缩进改写",
        );
        return serFail("STRUCTURE_MISMATCH", "缩进层级跳跃", "请检查文件层级", bag.all());
      }
      ancestry.length = depth;
      ancestry[depth] = fields.id;
      current = { id: fields.id, version: fields.version, tag: fields.tag, props: {} };
      currentDepth = depth;
      continue;
    }

    if (current === null) {
      bag.push(
        "MALFORMED_FIELD",
        `第 ${i + 1} 行的属性行出现在任何节点定义之前`,
        "属性必须归属某个节点；请检查文件是否被截断或手工拼接",
      );
      return serFail("MALFORMED_FIELD", "属性行无宿主节点", "请检查文件结构", bag.all());
    }
    // 属性行必须恰好缩进在宿主节点下一层。
    // 这条看似冗余，实则能抓住「节点行被单独改坏缩进」——那种改动会让属性行
    // 与宿主脱节，形成一棵仍然合法但已被篡改的树（树深核对抓不到它）。
    if (depth !== currentDepth + 1) {
      bag.push(
        "STRUCTURE_MISMATCH",
        `第 ${i + 1} 行属性缩进深度 ${depth}，宿主节点 ${current.id} 的深度为 ${currentDepth}，应为 ${currentDepth + 1}`,
        "属性行与宿主节点脱节，说明节点行缩进被单独改写；请勿采信此文件的结构",
      );
      return serFail("STRUCTURE_MISMATCH", "属性行与宿主节点层级脱节", "请检查缩进", bag.all());
    }
    const prop = parsePropLine(trimmed, current.id, i + 1, bag);
    if (prop.kind === "error") {
      return serFail("MALFORMED_FIELD", `第 ${i + 1} 行属性解析失败`, "请检查该行语法与类型前缀", bag.all());
    }
    // kind === "skip"：前缀未知，整行跳过且**不写入属性集**——
    // 用假值占位会把未知字段变成 "" 参与后续语义，这是比丢失更坏的结果。
    if (prop.kind === "ok") current.props[prop.key] = prop.value;
  }
  if (!close()) {
    return serFail("STRUCTURE_MISMATCH", "节点 id 重复", "id 必须全局唯一，请检查文件", bag.all());
  }

  if (nodes.size !== header.nodeCount) {
    bag.push(
      "STRUCTURE_MISMATCH",
      `实际解析出 ${nodes.size} 个节点，版本头声明 ${header.nodeCount} 个`,
      "数量不符说明文件被截断或追加；请以解析结果为准判断可用性，不要默认信任头部声明",
    );
    return serFail("STRUCTURE_MISMATCH", "节点数与版本头声明不一致", "请检查文件完整性", bag.all());
  }

  const finalNodes = new Map<string, SerNode>();
  for (const [id, node] of nodes) {
    finalNodes.set(id, { ...node, childIds: childOrder.get(id) ?? [] });
  }
  const tree: SerTree = {
    nodes: finalNodes,
    rootId: header.rootId,
    treeVersion: header.treeVersion,
  };
  // 还原后的树必须仍满足不变式——文本可能被手工改坏，这里是最后一道闸
  const verifyBag = new SerDiagBag();
  const ticket = assertSerializable(tree, verifyBag);
  if (ticket === null) {
    for (const d of verifyBag.all()) bag.push(d.code, d.message, d.hint);
    return serFail("STRUCTURE_MISMATCH", "解析还原的树不满足不变式", "文本结构已被改坏，请检查父子关系", bag.all());
  }
  // 树深交叉核对：缩进被「降级」（4 空格改 2 空格）后仍可能是一棵合法树——
  // 那种篡改骗得过不变式，但骗不过版本头里的 depth 声明。
  // 这条校验是「格式自带冗余」的价值兑现：同一事实存两份，改一处必被另一处发现。
  if (header.depth >= 0 && ticket.maxDepth !== header.depth) {
    bag.push(
      "STRUCTURE_MISMATCH",
      `还原出的树深为 ${ticket.maxDepth}，版本头声明为 ${header.depth}`,
      "缩进层级被改动过；树深是冗余声明，与实际结构不符即说明文件不可信，请勿采信",
    );
    return serFail("STRUCTURE_MISMATCH", "树深与版本头声明不一致", "缩进层级被改写，请检查文件", bag.all());
  }
  return serOk(tree, bag.all());
}

/** 节点行解析结果。 */
interface NodeLineFields {
  readonly id: string;
  readonly tag: string;
  readonly version: number;
}

/** 节点行解析：未知 token 跳过（前向兼容第二道关），必需字段缺失即失败。 */
function parseNodeLine(body: string, lineNo: number, bag: SerDiagBag): NodeLineFields | null {
  const tokens = body.split(/\s+/).slice(1);
  let id = "";
  let tag = "";
  let version = 0;
  let sawId = false;
  let sawTag = false;
  let sawVersion = false;
  for (let i = 0; i < tokens.length; i += 1) {
    const token = tokens[i] as string;
    const eq = token.indexOf("=");
    if (eq <= 0) {
      bag.push(
        "UNKNOWN_FIELD_SKIPPED",
        `第 ${lineNo} 行节点定义的裸 token「${token}」无法解析`,
        "按前向兼容约定跳过",
      );
      continue;
    }
    const key = token.slice(0, eq);
    const value = token.slice(eq + 1);
    switch (key) {
      case "id": {
        // 空串id 是合法值，故用 null 判定「解析失败」而非用空串兜底
        const parsed = parseJsonString(value);
        if (parsed === null) {
          bag.push(
            "MALFORMED_FIELD",
            `第 ${lineNo} 行节点 id 不是合法字符串字面量（${value}）`,
            "id 需为 JSON 字符串字面量；空串 id 合法，写作 id=\"\"",
          );
          return null;
        }
        id = parsed;
        sawId = true;
        break;
      }
      case "tag":
        sawTag = true;
        // tag=custom 只是哨兵，真实标签值由紧随其后的 custom="..." 给出
        tag = value === CUSTOM_TAG_SENTINEL ? "" : value;
        break;
      case CUSTOM_TAG_KEY: {
        const parsed = parseJsonString(value);
        if (parsed === null) {
          bag.push(
            "MALFORMED_FIELD",
            `第 ${lineNo} 行自定义标签不是合法字符串字面量（${value}）`,
            "自定义标签需为 JSON 字符串字面量；空串标签请改用白名单标签",
          );
          return null;
        }
        tag = parsed;
        break;
      }
      case "v":
        sawVersion = true;
        version = Number(value);
        break;
      default:
        bag.push(
          "UNKNOWN_FIELD_SKIPPED",
          `第 ${lineNo} 行节点字段 ${key} 为未知字段`,
          "按前向兼容约定跳过——新版本新增的节点字段不会让旧读取端炸掉",
        );
        break;
    }
  }
  // id 允许为空串，故不能用 id === "" 判缺失；用「是否出现过 id 字段」判定
  if (!sawId || !sawTag || !sawVersion || !Number.isFinite(version)) {
    return null;
  }
  return { id, tag: tag === "" ? "group" : tag, version };
}

/** 属性行解析结果：三态——成功、整行跳过（未知前缀）、显性失败。 */
type PropParseResult =
  | { readonly kind: "ok"; readonly key: string; readonly value: SerPropValue }
  | { readonly kind: "skip" }
  | { readonly kind: "error" };

/**
 * 属性行解析：类型前缀必须匹配。
 * 前缀未知时返回 "skip" 而非写入假值——前向兼容第三道关。
 * 空串属性名是合法的，故合法性由「是否为 JSON 字符串字面量」判定，不靠 key 是否为空。
 */
function parsePropLine(body: string, nodeId: string, lineNo: number, bag: SerDiagBag): PropParseResult {
  const sp = body.indexOf(" ");
  if (sp <= 0) {
    bag.push(
      "MALFORMED_FIELD",
      `节点 ${nodeId} 第 ${lineNo} 行属性缺少类型前缀`,
      "类型前缀（num/str/bool）是格式的一部分；缺失即无法判定类型，应显性失败而非猜测",
    );
    return { kind: "error" };
  }
  const prefix = body.slice(0, sp);
  if (prefix !== PROP_PREFIX_NUM && prefix !== PROP_PREFIX_STR && prefix !== PROP_PREFIX_BOOL) {
    bag.push(
      "UNKNOWN_FIELD_SKIPPED",
      `节点 ${nodeId} 第 ${lineNo} 行属性前缀 ${prefix} 不为当前实现所知，整行跳过`,
      "前向兼容：新增属性类型对旧读取端不可见，但不影响其余字段解析",
    );
    return { kind: "skip" };
  }
  const rest = body.slice(sp + 1);
  const eq = rest.indexOf("=");
  if (eq < 0) {
    bag.push(
      "MALFORMED_FIELD",
      `节点 ${nodeId} 第 ${lineNo} 行属性缺少 key=value 形态`,
      "请检查该行是否为「<类型前缀> <key>=<值>」形态",
    );
    return { kind: "error" };
  }
  const rawKey = rest.slice(0, eq);
  if (!isJsonStringLiteral(rawKey)) {
    bag.push(
      "MALFORMED_FIELD",
      `节点 ${nodeId} 第 ${lineNo} 行属性键不是合法字符串字面量（${rawKey}）`,
      "属性键需为 JSON 字符串字面量（含引号），空串键是合法的",
    );
    return { kind: "error" };
  }
  const key = parseJsonString(rawKey);
  if (key === null) {
    bag.push(
      "MALFORMED_FIELD",
      `节点 ${nodeId} 第 ${lineNo} 行属性键解析失败（${rawKey}）`,
      "属性键需为 JSON 字符串字面量（含引号），空串键是合法的",
    );
    return { kind: "error" };
  }
  const raw = rest.slice(eq + 1);
  if (prefix === PROP_PREFIX_NUM) {
    const v = literalToNumber(raw);
    if (v === null) {
      bag.push(
        "MALFORMED_FIELD",
        `节点 ${nodeId} 第 ${lineNo} 行数值属性 ${key} 的字面量非法（${raw}）`,
        "仅接受有限数值字面量；-0 写作 -0，NaN/Infinity 不入格式",
      );
      return { kind: "error" };
    }
    return { kind: "ok", key, value: v };
  }
  if (prefix === PROP_PREFIX_STR) {
    if (!isJsonStringLiteral(raw)) {
      bag.push(
        "MALFORMED_FIELD",
        `节点 ${nodeId} 第 ${lineNo} 行字符串属性 ${key} 的字面量非法（${raw}）`,
        "字符串属性需为 JSON 字符串字面量",
      );
      return { kind: "error" };
    }
    const parsedValue = parseJsonString(raw);
    if (parsedValue === null) {
      bag.push(
        "MALFORMED_FIELD",
        `节点 ${nodeId} 第 ${lineNo} 行字符串属性 ${key} 的值解析失败`,
        "字符串属性需为 JSON 字符串字面量",
      );
      return { kind: "error" };
    }
    return { kind: "ok", key, value: parsedValue };
  }
  if (raw !== "true" && raw !== "false") {
    bag.push(
      "MALFORMED_FIELD",
      `节点 ${nodeId} 第 ${lineNo} 行布尔属性 ${key} 的字面量非法（${raw}）`,
      "布尔属性只接受 true/false",
    );
    return { kind: "error" };
  }
  return { kind: "ok", key, value: raw === "true" };
}

/** 判断 token 是否为合法 JSON 字符串字面量（含转义）。 */
function isJsonStringLiteral(token: string): boolean {
  if (token.length < 2) return false;
  if (!token.startsWith("\"") || !token.endsWith("\"")) return false;
  try {
    return typeof (JSON.parse(token) as unknown) === "string";
  } catch {
    return false;
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §6 二进制格式（判据一之半：机读，紧凑）
// ════════════════════════════════════════════════════════════════════════════

/** 二进制 magic：ASCII "VLTB"。 */
export const BIN_MAGIC: readonly number[] = [0x56, 0x4c, 0x54, 0x42];

/** 二进制格式版本（写入头部的 u8）。 */
export const BIN_FORMAT_MAJOR = 1;

/** 二进制格式次版本（供将来在同一主版本下新增扩展字段）。 */
export const BIN_FORMAT_MINOR = 0;

/** 属性值类型标记。 */
export const BIN_KIND_NUMBER = 0;
export const BIN_KIND_STRING = 1;
export const BIN_KIND_BOOL = 2;
/** 扩展类型标记：当前实现不认识，读时按长度跳过。 */
export const BIN_KIND_UNKNOWN = 0xff;

/** 字节写出器（变长整数 + 定长数值，容量按需增长）。 */
class ByteWriter {
  private buf: Uint8Array;
  private len = 0;

  constructor(initial = 1024) {
    this.buf = new Uint8Array(Math.max(16, initial));
  }

  get length(): number {
    return this.len;
  }

  private need(n: number): void {
    if (this.len + n <= this.buf.length) return;
    let cap = this.buf.length * 2;
    while (cap < this.len + n) cap *= 2;
    const next = new Uint8Array(cap);
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
  }

  u8(v: number): void {
    this.need(1);
    this.buf[this.len] = v & 0xff;
    this.len += 1;
  }

  /** LEB128 无符号变长整数（索引与长度用它——这是「紧凑」的主要来源）。 */
  varUint(v: number): void {
    let rest = v >>> 0;
    for (;;) {
      const byte = rest & 0x7f;
      rest >>>= 7;
      if (rest === 0) {
        this.u8(byte);
        return;
      }
      this.u8(byte | 0x80);
    }
  }

  /** ZigZag 编码的有符号整数（父索引 -1 用它，一字节搞定）。 */
  varInt(v: number): void {
    const zz = (v << 1) ^ (v >> 31);
    this.varUint(zz >>> 0);
  }

  /** f64 小端（IEEE-754 原生位型，-0 与极大极小值天然无损）。 */
  f64(v: number): void {
    this.need(8);
    new DataView(this.buf.buffer).setFloat64(this.len, v, true);
    this.len += 8;
  }

  bytes(arr: Uint8Array): void {
    this.need(arr.length);
    this.buf.set(arr, this.len);
    this.len += arr.length;
  }

  /** 写一段 UTF-8 字符串：长度前缀 + 字节。 */
  str(text: string): void {
    const b = utf8Encode(text);
    this.varUint(b.length);
    this.bytes(b);
  }

  finish(): Uint8Array {
    return this.buf.slice(0, this.len);
  }
}

/** 字节读出器（越界一律置失败位并返回 0，由调用方统一裁决）。 */
class ByteReader {
  private readonly buf: Uint8Array;
  private readonly view: DataView;
  private pos = 0;
  private broken = false;

  constructor(buf: Uint8Array) {
    this.buf = buf;
    this.view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  }

  get failed(): boolean {
    return this.broken;
  }

  get offset(): number {
    return this.pos;
  }

  get remaining(): number {
    return this.buf.length - this.pos;
  }

  private need(n: number): boolean {
    if (this.broken || this.pos + n > this.buf.length) {
      this.broken = true;
      return false;
    }
    return true;
  }

  u8(): number {
    if (!this.need(1)) return 0;
    const v = this.buf[this.pos] as number;
    this.pos += 1;
    return v;
  }

  varUint(): number {
    let result = 0;
    let shift = 0;
    for (let i = 0; i < 5; i += 1) {
      const byte = this.u8();
      if (this.broken) return 0;
      result |= (byte & 0x7f) << shift;
      if ((byte & 0x80) === 0) return result >>> 0;
      shift += 7;
    }
    // 超过 5 字节仍无终止位：非法编码
    this.broken = true;
    return 0;
  }

  varInt(): number {
    const zz = this.varUint();
    return (zz >>> 1) ^ -(zz & 1);
  }

  f64(): number {
    if (!this.need(8)) return 0;
    const v = this.view.getFloat64(this.pos, true);
    this.pos += 8;
    return v;
  }

  str(bag: SerDiagBag, where: string): string {
    const len = this.varUint();
    if (this.broken) return "";
    if (len > this.remaining) {
      this.broken = true;
      bag.push("MALFORMED_FIELD", `${where} 声明字符串长度 ${len} 超出剩余字节 ${this.remaining}`, "文件被截断或长度字段被改写");
      return "";
    }
    const slice = this.buf.subarray(this.pos, this.pos + len);
    this.pos += len;
    return utf8Decode(slice);
  }

  /** 按长度跳过未知内容（前向兼容的核心动作：靠长度，不靠猜）。 */
  skip(n: number): boolean {
    if (!this.need(n)) return false;
    this.pos += n;
    return true;
  }
}

/**
 * 二进制编码（O(有效属性数)）。
 * 布局：
 *   magic[4] | major u8 | minor u8
 *   treeVersion varUint | nodeCount varUint | rootIndex varInt
 *   节点表（保序先序，索引即序号）：
 *     id: str | tagCode u8 | [customTag: str] | version f64
 *     parentIndex varInt | childCount varUint | childIndex... varInt
 *     propCount varUint | (key: str | kind u8 | payload)
 *     extraCount varUint | (key: str | kind u8 | payloadLen varUint | payload bytes)
 *
 * extraCount 是前向兼容的容器位：当前实现恒写 0；将来在同一主版本新增字段时
 * 写进这里，旧读取端按 payloadLen 跳过（立场五）。
 */
export function encodeBinary(tree: SerTree, bag: SerDiagBag): Uint8Array | null {
  const ticket = assertSerializable(tree, bag);
  if (ticket === null) return null;
  const order = preorderIds(tree, ticket);
  if (order.length !== ticket.nodeCount) {
    bag.push("STRUCTURE_MISMATCH", "保序遍历节点数与断言统计不一致", "请勿在序列化过程中并发修改树");
    return null;
  }

  const indexOf = new Map<string, number>();
  for (let i = 0; i < order.length; i += 1) indexOf.set(order[i] as string, i);

  // 先做属性值预检：任何一条不合法就不产出字节（避免半截文件）
  for (const id of order) {
    const node = tree.nodes.get(id);
    if (node === undefined) continue;
    for (const key of Object.keys(node.props)) {
      const value = node.props[key];
      if (typeof value === "number" && !Number.isFinite(value)) {
        bag.push(
          "PROPERTY_INVALID",
          `节点 ${id} 的属性 ${key} 值为非有限数（${String(value)}）`,
          "IEEE-754 虽能存 NaN，但业务语义上非有限值不可入交换格式；请先归一或剔除",
        );
        return null;
      }
      if (value !== undefined && typeof value !== "number" && typeof value !== "string" && typeof value !== "boolean") {
        bag.push(
          "PROPERTY_INVALID",
          `节点 ${id} 的属性 ${key} 类型不受支持（${typeof value}）`,
          "序列化仅支持 number/string/boolean",
        );
        return null;
      }
    }
  }

  const w = new ByteWriter(4096);
  for (const b of BIN_MAGIC) w.u8(b);
  w.u8(BIN_FORMAT_MAJOR);
  w.u8(BIN_FORMAT_MINOR);
  w.varUint(tree.treeVersion);
  w.varUint(ticket.nodeCount);
  w.varInt(indexOf.get(ticket.rootId) ?? 0);

  for (const id of order) {
    const node = tree.nodes.get(id);
    if (node === undefined) continue;
    w.str(node.id);
    const tagIdx = NODE_TAGS.indexOf(node.tag);
    if (tagIdx >= 0) {
      w.u8(tagIdx);
    } else {
      w.u8(NODE_TAGS.length);
      w.str(node.tag);
    }
    w.f64(node.version);
    w.varInt(node.parentId === null ? -1 : (indexOf.get(node.parentId) ?? -1));
    w.varUint(node.childIds.length);
    for (const childId of node.childIds) {
      w.varInt(indexOf.get(childId) ?? -1);
    }
    const keys = Object.keys(node.props).sort();
    w.varUint(keys.length);
    for (const key of keys) {
      const value = node.props[key];
      if (value === undefined) continue;
      w.str(key);
      if (typeof value === "number") {
        w.u8(BIN_KIND_NUMBER);
        w.f64(value);
      } else if (typeof value === "string") {
        w.u8(BIN_KIND_STRING);
        w.str(value);
      } else {
        w.u8(BIN_KIND_BOOL);
        w.u8(value ? 1 : 0);
      }
    }
    w.varUint(0); // extraCount：当前版本无扩展项
  }
  return w.finish();
}

/**
 * 二进制解码（O(有效属性数)）。
 * 逐字段校验：任何越界、非法标记、索引越界都定位到具体节点/字段后显性失败。
 */
export function decodeBinary(bytes: Uint8Array, bag: SerDiagBag): SerOutcome<SerTree> {
  if (bytes.length < 8) {
    return serFail("MALFORMED_FIELD", `二进制长度 ${bytes.length} 过短，不足以容纳头部`, "请确认文件未被截断");
  }
  for (let i = 0; i < BIN_MAGIC.length; i += 1) {
    if (bytes[i] !== (BIN_MAGIC[i] as number)) {
      return serFail("FORMAT_VERSION_UNSUPPORTED", "二进制 magic 不匹配", "该字节流不是本格式产物，请确认来源");
    }
  }
  const r = new ByteReader(bytes);
  r.u8();
  r.u8();
  r.u8();
  r.u8();
  const major = r.u8();
  const minor = r.u8();
  if (major > BIN_FORMAT_MAJOR) {
    bag.push(
      "FORMAT_VERSION_UNSUPPORTED",
      `二进制主版本 ${major} 高于本实现支持的 ${BIN_FORMAT_MAJOR}`,
      "跨主版本字段语义可能已变；请升级读取端，或由写入端降级输出",
    );
    return serFail("FORMAT_VERSION_UNSUPPORTED", "主版本过高", "请升级或降级", bag.all());
  }
  if (minor > BIN_FORMAT_MINOR) {
    bag.push(
      "UNKNOWN_FIELD_SKIPPED",
      `二进制次版本 ${minor} 高于本实现支持的 ${BIN_FORMAT_MINOR}，按前向兼容继续解析`,
      "同主版本内的扩展通过 extraCount 承载，跳过后语义仍完整",
    );
  }

  const treeVersion = r.varUint();
  const nodeCount = r.varUint();
  const rootIndex = r.varInt();
  if (r.failed || nodeCount === 0) {
    return serFail("MALFORMED_FIELD", "头部字段解析失败或节点数为 0", "请检查文件完整性");
  }
  if (rootIndex < 0 || rootIndex >= nodeCount) {
    bag.push("MALFORMED_FIELD", `根索引 ${rootIndex} 越界（节点数 ${nodeCount}）`, "根索引必须落在节点表内");
    return serFail("MALFORMED_FIELD", "根索引越界", "文件已损坏", bag.all());
  }

  const ids: string[] = [];
  const tags: string[] = [];
  const versions: number[] = [];
  const parents: number[] = [];
  const childIdx: number[][] = [];
  const props: Array<Record<string, SerPropValue>> = [];

  for (let i = 0; i < nodeCount; i += 1) {
    const id = r.str(bag, `节点 #${i} 的 id`);
    const tagCode = r.u8();
    let tag = "";
    if (tagCode < NODE_TAGS.length) {
      tag = NODE_TAGS[tagCode] as string;
    } else if (tagCode === NODE_TAGS.length) {
      tag = r.str(bag, `节点 #${i} 的自定义标签`);
    } else {
      bag.push(
        "UNKNOWN_FIELD_SKIPPED",
        `节点 #${i} 的标签码 ${tagCode} 不为当前实现所知，按「group」处理`,
        "标签为分类信息而非语义必需项；降级为通用容器并登记，不让整档失败",
      );
      tag = "group";
    }
    const version = r.f64();
    const parent = r.varInt();
    const cc = r.varUint();
    const children: number[] = [];
    for (let k = 0; k < cc; k += 1) children.push(r.varInt());
    const pc = r.varUint();
    const bagProps: Record<string, SerPropValue> = {};
    for (let k = 0; k < pc; k += 1) {
      const key = r.str(bag, `节点 #${i} 的属性键`);
      const kind = r.u8();
      if (kind === BIN_KIND_NUMBER) {
        bagProps[key] = r.f64();
      } else if (kind === BIN_KIND_STRING) {
        bagProps[key] = r.str(bag, `节点 #${i} 的字符串属性 ${key}`);
      } else if (kind === BIN_KIND_BOOL) {
        bagProps[key] = r.u8() !== 0;
      } else {
        bag.push(
          "UNKNOWN_FIELD_SKIPPED",
          `节点 #${i} 的属性 ${key} 类型码 ${kind} 未知，按空串占位`,
          "类型码未知时无法安全取值；占位并登记，避免整档作废",
        );
        bagProps[key] = "";
      }
    }
    // 扩展项容器：按长度跳过（立场五）
    const ec = r.varUint();
    for (let k = 0; k < ec; k += 1) {
      const key = r.str(bag, `节点 #${i} 的扩展键`);
      const kind = r.u8();
      const len = r.varUint();
      if (!r.skip(len)) {
        return serFail("MALFORMED_FIELD", `节点 #${i} 的扩展项 ${key} 长度 ${len} 越界`, "文件被截断", bag.all());
      }
      bag.push(
        "UNKNOWN_FIELD_SKIPPED",
        `节点 #${i} 的扩展项 ${key}（类型码 ${kind}，${len} 字节）已按长度跳过`,
        "前向兼容兑现：写入端新增的字段不会让本读取端炸掉",
      );
    }
    if (r.failed) {
      return serFail("MALFORMED_FIELD", `解析节点 #${i} 时字节流耗尽`, "文件被截断或长度字段被改写", bag.all());
    }
    ids.push(id);
    tags.push(tag);
    versions.push(version);
    parents.push(parent);
    childIdx.push(children);
    props.push(bagProps);
  }

  // 建表前的索引合法性校验：关系用索引表达，索引一旦越界就会静默指向 null/占位符，
// 那种「看起来建成了树」的产物比直接失败危险得多，故先整体核对。
for (let i = 0; i < ids.length; i += 1) {
    const pi = parents[i] as number;
    if (pi >= ids.length) {
      bag.push("MALFORMED_FIELD", `节点 #${i} 的父索引 ${pi} 越界（共 ${ids.length} 个节点）`, "文件被改写或损坏");
      return serFail("MALFORMED_FIELD", "父索引越界", "文件已损坏", bag.all());
    }
    for (const c of childIdx[i] as number[]) {
      if (c < 0 || c >= ids.length) {
        bag.push("MALFORMED_FIELD", `节点 #${i} 的子索引 ${c} 越界（共 ${ids.length} 个节点）`, "文件被改写或损坏");
        return serFail("MALFORMED_FIELD", "子索引越界", "文件已损坏", bag.all());
      }
    }
  }
  const nodes = new Map<string, SerNode>();
  for (let i = 0; i < ids.length; i += 1) {
    const id = ids[i] as string;
    if (nodes.has(id)) {
      bag.push("STRUCTURE_MISMATCH", `节点 id 重复出现（${id}）`, "id 必须全局唯一（外部句柄依赖它）；请检查文件来源");
      return serFail("STRUCTURE_MISMATCH", "节点 id 重复", "id 必须全局唯一", bag.all());
    }
    const pi = parents[i] as number;
    nodes.set(id, {
      id,
      parentId: pi < 0 ? null : (ids[pi] as string),
      childIds: (childIdx[i] as number[]).map((c) => ids[c] as string),
      props: props[i] as Record<string, SerPropValue>,
      version: versions[i] as number,
      tag: tags[i] as string,
    });
  }
  const tree: SerTree = {
    nodes,
    rootId: ids[rootIndex] ?? "",
    treeVersion,
  };
  const verifyBag = new SerDiagBag();
  if (assertSerializable(tree, verifyBag) === null) {
    for (const d of verifyBag.all()) bag.push(d.code, d.message, d.hint);
    return serFail("STRUCTURE_MISMATCH", "解码还原的树不满足不变式", "二进制结构已损坏或被改写", bag.all());
  }
  return serOk(tree, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §7 前向兼容规则（判据三：老读者读新档不炸）
// ════════════════════════════════════════════════════════════════════════════

/** 前向兼容策略：读到不认识的字段时怎么办。 */
export type ForwardCompatPolicy =
  /** 严格：任何未知字段都显性失败（用于生产链路要求逐字段已知的场景）。 */
  | "strict"
  /** 兼容（默认）：未知字段按长度跳过并登记 UNKNOWN_FIELD_SKIPPED。 */
  | "skip-unknown";

/**
 * 构造一个「比本实现更新」的二进制档（供前向兼容验收与回归）。
 *
 * 这是把「承诺」变成「可测断言」的关键——承诺必须有可执行的证据。
 * 构造方式刻意走**真实路径**：重新编码整棵树，并在每个节点的 extraCount 处
 * 写入两项未知类型的扩展字段，再把次版本抬高。
 * 这样解码端必须真正跑通「读extraCount → 按 payloadLen 跳过未知项」这条逻辑，
 * 而不是靠「尾部多一截数据被无视」蒙混过关——后者根本没验证到承诺的核心。
 */
export function buildNewerBinaryFixture(tree: SerTree): Uint8Array | null {
  const bag = new SerDiagBag();
  const ticket = assertSerializable(tree, bag);
  if (ticket === null) return null;
  const order = preorderIds(tree, ticket);
  if (order.length !== ticket.nodeCount) return null;
  const indexOf = new Map<string, number>();
  for (let i = 0; i < order.length; i += 1) indexOf.set(order[i] as string, i);

  const w = new ByteWriter(4096);
  for (const b of BIN_MAGIC) w.u8(b);
  w.u8(BIN_FORMAT_MAJOR);
  w.u8(BIN_FORMAT_MINOR + 1); // 次版本抬高：本实现未定义该次版本的新增字段
  w.varUint(tree.treeVersion);
  w.varUint(ticket.nodeCount);
  w.varInt(indexOf.get(ticket.rootId) ?? 0);

  for (const id of order) {
    const node = tree.nodes.get(id);
    if (node === undefined) continue;
    w.str(node.id);
    const tagIdx = NODE_TAGS.indexOf(node.tag);
    if (tagIdx >= 0) {
      w.u8(tagIdx);
    } else {
      w.u8(NODE_TAGS.length);
      w.str(node.tag);
    }
    w.f64(node.version);
    w.varInt(node.parentId === null ? -1 : (indexOf.get(node.parentId) ?? -1));
    w.varUint(node.childIds.length);
    for (const childId of node.childIds) w.varInt(indexOf.get(childId) ?? -1);
    const keys = Object.keys(node.props).sort();
    w.varUint(keys.length);
    for (const key of keys) {
      const value = node.props[key];
      if (value === undefined) continue;
      w.str(key);
      if (typeof value === "number") {
        w.u8(BIN_KIND_NUMBER);
        w.f64(value);
      } else if (typeof value === "string") {
        w.u8(BIN_KIND_STRING);
        w.str(value);
      } else {
        w.u8(BIN_KIND_BOOL);
        w.u8(value ? 1 : 0);
      }
    }
    // 扩展项容器：两项未知类型 + 一个 payload 内含不可解析字节的未知项。
    // 本实现必须按 payloadLen 跳过它们，且跳过后的解析结果与旧档完全一致。
    w.varUint(2);
    w.str("future.bloomMask");
    w.u8(BIN_KIND_UNKNOWN);
    const payload = utf8Encode("binary-blob");
    w.varUint(payload.length);
    w.bytes(payload);
    w.str("future.v2Flags");
    w.u8(BIN_KIND_UNKNOWN);
    w.varUint(3);
    w.u8(0xde);
    w.u8(0xad);
    w.u8(0xbe);
  }
  return w.finish();
}

/**
 * 检查一次解码是否满足「前向兼容」承诺：
 * 高于本实现的次版本被登记为 UNKNOWN_FIELD_SKIPPED，且解码仍然成功。
 */
export function auditForwardCompat(
  bytes: Uint8Array,
  policy: ForwardCompatPolicy,
): SerOutcome<{ readonly skipped: number; readonly tree: SerTree }> {
  const bag = new SerDiagBag();
  const decoded = decodeBinary(bytes, bag);
  const skipped = bag.byCode("UNKNOWN_FIELD_SKIPPED").length;
  if (!decoded.ok) {
    return serFail(decoded.code, decoded.message, decoded.hint, bag.all());
  }
  if (policy === "strict" && skipped > 0) {
    bag.push(
      "UNKNOWN_FIELD_SKIPPED",
      `严格策略下发现 ${skipped} 处未知字段`,
      "严格策略要求逐字段已知；请升级读取端或改用兼容策略",
    );
    return serFail("UNKNOWN_FIELD_SKIPPED", "严格策略拒绝含未知字段的档", "请升级读取端", bag.all());
  }
  return serOk({ skipped, tree: decoded.value }, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §8 互转（判据四：互转无损）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 规范化文本：节点按保序先序、属性键按字典序输出，不含版本头的次版本信息。
 * 用途是「等价判定」——两棵树规范化后逐字节相同，当且仅当语义等价。
 * 这是判据四的可执行形式：不靠肉眼比，靠规范化比对。
 */
export function canonicalText(tree: SerTree, bag: SerDiagBag): string | null {
  return encodeText(tree, bag);
}

/** 双格式互转：文本 → 树 → 二进制。 */
export function textToBinary(text: string, bag: SerDiagBag): SerOutcome<Uint8Array> {
  const decoded = decodeText(text, bag);
  if (!decoded.ok) {
    return serFail(decoded.code, decoded.message, decoded.hint, bag.all());
  }
  const bytes = encodeBinary(decoded.value, bag);
  if (bytes === null) {
    const first = bag.all()[bag.size - 1];
    return serFail(first?.code ?? "MALFORMED_FIELD", first?.message ?? "编码失败", first?.hint ?? "", bag.all());
  }
  return serOk(bytes, bag.all());
}

/** 双格式互转：二进制 → 树 → 文本。 */
export function binaryToText(bytes: Uint8Array, bag: SerDiagBag): SerOutcome<string> {
  const decoded = decodeBinary(bytes, bag);
  if (!decoded.ok) {
    return serFail(decoded.code, decoded.message, decoded.hint, bag.all());
  }
  const text = encodeText(decoded.value, bag);
  if (text === null) {
    const first = bag.all()[bag.size - 1];
    return serFail(first?.code ?? "MALFORMED_FIELD", first?.message ?? "编码失败", first?.hint ?? "", bag.all());
  }
  return serOk(text, bag.all());
}

/** 无损性核验：树 → 文本 → 树 → 二进制 → 树，四段全等才算通过。 */
export function verifyLossless(tree: SerTree, bag: SerDiagBag): SerOutcome<{ readonly bytes: number }> {
  const text = encodeText(tree, bag);
  if (text === null) return serFail("MALFORMED_FIELD", "文本编码失败", "见诊断袋", bag.all());
  const backText = decodeText(text, bag);
  if (!backText.ok) return serFail(backText.code, backText.message, backText.hint, bag.all());

  const bytes = encodeBinary(tree, bag);
  if (bytes === null) return serFail("MALFORMED_FIELD", "二进制编码失败", "见诊断袋", bag.all());
  const backBin = decodeBinary(bytes, bag);
  if (!backBin.ok) return serFail(backBin.code, backBin.message, backBin.hint, bag.all());

  const textA = canonicalText(tree, bag);
  const textB = canonicalText(backText.value, bag);
  const textC = canonicalText(backBin.value, bag);
  if (textA === null || textB === null || textC === null) {
    return serFail("MALFORMED_FIELD", "规范化失败", "见诊断袋", bag.all());
  }
  if (textA !== textB || textA !== textC) {
    bag.push("STRUCTURE_MISMATCH", "文本往返与二进制往返的规范化结果不一致", "存在某一侧未保真的字段，请定位属性级差异");
    return serFail("STRUCTURE_MISMATCH", "互转有损", "请检查属性类型与顺序保真", bag.all());
  }
  return serOk({ bytes: bytes.length }, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §9 与 F0610 快照的互转（内存态 ↔ 持久化态）
// ════════════════════════════════════════════════════════════════════════════

/**
 * F0610 完整快照的**结构化**描述（本条不 import F0610，避免跨条耦合；
 * 两者只要结构同形即可互转，这是 F0610「交接说明」预留的口子）。
 */
export interface SnapshotLike {
  readonly kind: "full";
  readonly treeVersion: number;
  readonly rootId: string;
  readonly nodes: ReadonlyArray<{
    readonly id: string;
    readonly parentId: string | null;
    readonly childIds: readonly string[];
    readonly props: Readonly<Record<string, SerPropValue>>;
    readonly version: number;
    readonly tag?: string;
  }>;
}

/** 快照 → 树（内存态回落到可序列化的活树形态）。 */
export function treeFromSnapshot(snap: SnapshotLike, bag: SerDiagBag): SerOutcome<SerTree> {
  if (snap.kind !== "full") {
    bag.push(
      "SNAPSHOT_BRIDGE_INCOMPATIBLE",
      `只接受完整快照（kind="full"），收到 ${String((snap as { kind: string }).kind)}`,
      "增量快照需先与基线合并为全量再交本条；直接桥接会丢失未变更节点的属性",
    );
    return serFail("SNAPSHOT_BRIDGE_INCOMPATIBLE", "快照类型不受支持", "请先合并为全量快照", bag.all());
  }
  if (snap.nodes.length === 0) {
    return serFail("EMPTY_TREE", "快照为空", "空快照无可还原的树", bag.all());
  }
  const nodes = new Map<string, SerNode>();
  for (const entry of snap.nodes) {
    if (nodes.has(entry.id)) {
      bag.push("SNAPSHOT_BRIDGE_INCOMPATIBLE", `快照中节点 id 重复（${entry.id}）`, "重复 id 无法映射到单键节点表");
      return serFail("SNAPSHOT_BRIDGE_INCOMPATIBLE", "节点 id 重复", "请检查快照来源", bag.all());
    }
    nodes.set(entry.id, {
      id: entry.id,
      parentId: entry.parentId,
      childIds: entry.childIds,
      props: entry.props,
      version: entry.version,
      tag: entry.tag ?? "group",
    });
  }
  const tree: SerTree = { nodes, rootId: snap.rootId, treeVersion: snap.treeVersion };
  const verify = new SerDiagBag();
  if (assertSerializable(tree, verify) === null) {
    for (const d of verify.all()) bag.push(d.code, d.message, d.hint);
    return serFail("SNAPSHOT_BRIDGE_INCOMPATIBLE", "快照还原的树不满足不变式", "快照本身已损坏", bag.all());
  }
  return serOk(tree, bag.all());
}

/** 树 → 快照结构（供 F0610 侧消费；本条只产出结构，不改活树）。 */
export function snapshotFromTree(tree: SerTree, bag: SerDiagBag): SerOutcome<SnapshotLike> {
  const ticket = assertSerializable(tree, bag);
  if (ticket === null) {
    const first = bag.all()[bag.size - 1];
    return serFail(first?.code ?? "TREE_INVARIANT_VIOLATED", first?.message ?? "不变式失败", first?.hint ?? "", bag.all());
  }
  const entries = [...tree.nodes.values()].map((n) => ({
    id: n.id,
    parentId: n.parentId,
    childIds: n.childIds,
    props: { ...n.props },
    version: n.version,
    tag: n.tag,
  }));
  return serOk({ kind: "full" as const, treeVersion: tree.treeVersion, rootId: tree.rootId, nodes: entries }, bag.all());
}

// ════════════════════════════════════════════════════════════════════════════
// §10 统计与度量
// ════════════════════════════════════════════════════════════════════════════

/** 产物体积统计（供调试面板与压缩比评估）。 */
export interface SerStats {
  readonly nodeCount: number;
  /** 有效属性总数（稀疏口径：只数显式设置过的项）。 */
  readonly propCount: number;
  readonly textBytes: number;
  readonly binaryBytes: number;
  /** 二进制相对文本的体积比（越小越紧凑；文本供人读故天然更大）。 */
  readonly binaryToTextRatio: number;
  readonly maxDepth: number;
}

/** 统计两种格式的产物体积。 */
export function collectStats(tree: SerTree, bag: SerDiagBag): SerOutcome<SerStats> {
  const text = encodeText(tree, bag);
  const bytes = encodeBinary(tree, bag);
  if (text === null || bytes === null) {
    return serFail("MALFORMED_FIELD", "统计所需的编码未成功", "见诊断袋", bag.all());
  }
  let propCount = 0;
  for (const node of tree.nodes.values()) propCount += Object.keys(node.props).length;
  const ticket = assertSerializable(tree, new SerDiagBag());
  const textBytes = utf8Encode(text).length;
  return serOk(
    {
      nodeCount: tree.nodes.size,
      propCount,
      textBytes,
      binaryBytes: bytes.length,
      binaryToTextRatio: textBytes === 0 ? 0 : Math.round((bytes.length / textBytes) * 10000) / 10000,
      maxDepth: ticket?.maxDepth ?? -1,
    },
    bag.all(),
  );
}

// ════════════════════════════════════════════════════════════════════════════
// §11 自检（域级自检支撑：逐条对判据取证）
// ════════════════════════════════════════════════════════════════════════════

/** 自检项。 */
export interface SerSelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 构造测试树的辅助。 */
function mkNode(id: string, parentId: string | null, childIds: readonly string[], props: Record<string, SerPropValue>, version = 1, tag = "group"): SerNode {
  return { id, parentId, childIds, props, version, tag };
}

/** 样例树：root → (group → (text, image), vector)，含稀疏属性与 -0。 */
function sampleTree(): SerTree {
  const nodes = new Map<string, SerNode>();
  nodes.set("root", mkNode("root", null, ["g1", "v1"], {}, 1, "root"));
  nodes.set("g1", mkNode("g1", "root", ["t1", "i1"], { "opacity": 0.5 }, 2, "group"));
  nodes.set("t1", mkNode("t1", "g1", [], { "label": "标题", "size": -0 }, 3, "text"));
  nodes.set("i1", mkNode("i1", "g1", [], { "w": 1920, "h": 1080, "visible": true }, 4, "image"));
  nodes.set("v1", mkNode("v1", "root", [], { "name": "路径 1", "custom": "yes" }, 5, "vector"));
  return { nodes, rootId: "root", treeVersion: 42 };
}

/** 自检一：环防护先行——各类结构违例必须在编码前被拦下。 */
export function selfCheckInvariant(): SerSelfCheck[] {
  const out: SerSelfCheck[] = [];

  const bagOk = new SerDiagBag();
  const okTree = sampleTree();
  const ticket = assertSerializable(okTree, bagOk);
  out.push({
    name: "合法树通过断言",
    pass: ticket !== null && ticket.nodeCount === 5 && ticket.maxDepth === 2,
    detail: ticket === null ? "被误拒" : `节点数=${ticket.nodeCount}（应 5），深度=${ticket.maxDepth}（应 2）`,
  });

  // 成环
  const bagCycle = new SerDiagBag();
  const cyc = new Map<string, SerNode>();
  cyc.set("a", mkNode("a", "b", ["b"], {}));
  cyc.set("b", mkNode("b", "a", ["a"], {}));
  out.push({
    name: "成环树被拒绝（环防护先行）",
    pass: assertSerializable({ nodes: cyc, rootId: "a", treeVersion: 1 }, bagCycle) === null &&
      bagCycle.has("TREE_INVARIANT_VIOLATED"),
    detail: `拒绝=${bagCycle.has("TREE_INVARIANT_VIOLATED")}`,
  });

  // 多根
  const bagMulti = new SerDiagBag();
  const multi = new Map<string, SerNode>();
  multi.set("r1", mkNode("r1", null, [], {}));
  multi.set("r2", mkNode("r2", null, [], {}));
  out.push({
    name: "多根树被拒绝",
    pass: assertSerializable({ nodes: multi, rootId: "r1", treeVersion: 1 }, bagMulti) === null,
    detail: "多根即无法确定还原起点，应拒绝",
  });

  // 悬空子引用
  const bagDangle = new SerDiagBag();
  const dangle = new Map<string, SerNode>();
  dangle.set("root", mkNode("root", null, ["ghost"], {}));
  out.push({
    name: "悬空子引用被拒绝",
    pass: assertSerializable({ nodes: dangle, rootId: "root", treeVersion: 1 }, bagDangle) === null,
    detail: "childIds 指向不存在的节点，应拒绝",
  });

  // 单父违例（一个子被两个父挂载）
  const bagDup = new SerDiagBag();
  const dup = new Map<string, SerNode>();
  dup.set("root", mkNode("root", null, ["g1", "g2"], {}));
  dup.set("g1", mkNode("g1", "root", ["x"], {}));
  dup.set("g2", mkNode("g2", "root", ["x"], {}));
  dup.set("x", mkNode("x", "g1", [], {}));
  out.push({
    name: "重复挂载（单父违例）被拒绝",
    pass: assertSerializable({ nodes: dup, rootId: "root", treeVersion: 1 }, bagDup) === null,
    detail: "x 同时被 g1/g2 挂载，应拒绝",
  });

  // 游离子树
  const bagOrphan = new SerDiagBag();
  const orphan = new Map<string, SerNode>();
  orphan.set("root", mkNode("root", null, [], {}));
  orphan.set("lost", mkNode("lost", null, [], {}));
  const orphanTree: SerTree = { nodes: orphan, rootId: "root", treeVersion: 1 };
  out.push({
    name: "游离子树被拒绝",
    pass: assertSerializable(orphanTree, bagOrphan) === null,
    detail: "lost 不在根链上，还原时会凭空出现",
  });

  // 环防护必须先于编码：断言失败时编码器返回 null 而非半截文本
  const bagOrder = new SerDiagBag();
  const text = encodeText(orphanTree, bagOrder);
  out.push({
    name: "断言失败时不产出任何文本（无半截产物）",
    pass: text === null,
    detail: text === null ? "已拒绝产出" : "竟然产出了文本",
  });

  // 空串 id 是合法值：不得被「缺失」判定误杀（这条曾真实误杀过一次）
  const bagEmptyId = new SerDiagBag();
  const emptyIdTree = new Map<string, SerNode>([["", mkNode("", null, [], { "": 1 }, 1, "root")]]);
  const emptyIdSer: SerTree = { nodes: emptyIdTree, rootId: "", treeVersion: 1 };
  const emptyIdLoss = verifyLossless(emptyIdSer, bagEmptyId);
  out.push({
    name: "空串 id 与空串属性键合法且可无损往返",
    pass: emptyIdLoss.ok,
    detail: emptyIdLoss.ok
      ? "id=\"\" 与属性键 \"\" 均按合法值处理"
      : `拒因=${emptyIdLoss.ok ? "n/a" : emptyIdLoss.code}（空串被误判为缺失）`,
  });

  return out;
}

/** 自检二：文本格式——类型显式、-0 保形、缩进层级、UTF-8 非 ASCII。 */
export function selfCheckText(): SerSelfCheck[] {
  const out: SerSelfCheck[] = [];
  const tree = sampleTree();
  const bag = new SerDiagBag();
  const text = encodeText(tree, bag);
  out.push({
    name: "文本编码成功",
    pass: text !== null && text.includes("#VLT/1"),
    detail: text === null ? "编码返回 null" : `首行=${(text.split("\n")[0] as string).slice(0, 40)}…`,
  });
  const decoded = decodeText(text ?? "", bag);
  out.push({
    name: "文本解码还原出等结构",
    pass: decoded.ok && decoded.value.nodes.size === 5 && decoded.value.rootId === "root",
    detail: decoded.ok ? `节点数=${decoded.value.nodes.size}（应 5）` : `拒因=${decoded.code}`,
  });
  if (decoded.ok) {
    const t1 = decoded.value.nodes.get("t1");
    out.push({
      name: "-0 保形（JSON 做不到的事）",
      pass: t1 !== undefined && typeof t1.props["size"] === "number" && Object.is(t1.props["size"], -0),
      detail: t1 === undefined ? "节点缺失" : `size=${String(t1.props["size"])}，is(-0)=${t1 !== undefined ? Object.is(t1.props["size"], -0) : false}`,
    });
    const v1 = decoded.value.nodes.get("v1");
    out.push({
      name: "中文字符串往返无损",
      pass: v1 !== undefined && v1.props["name"] === "路径 1",
      detail: v1 === undefined ? "节点缺失" : `name=${String(v1.props["name"])}`,
    });
    const i1 = decoded.value.nodes.get("i1");
    out.push({
      name: "布尔与数值类型不混淆",
      pass: i1 !== undefined && i1.props["visible"] === true && i1.props["w"] === 1920,
      detail: i1 === undefined ? "节点缺失" : `visible=${String(i1.props["visible"])} w=${String(i1.props["w"])}`,
    });
    out.push({
      name: "Z 序保序（childIds 顺序不丢）",
      pass: JSON.stringify(decoded.value.nodes.get("root")?.childIds) === JSON.stringify(["g1", "v1"]),
      detail: `childIds=${JSON.stringify(decoded.value.nodes.get("root")?.childIds)}`,
    });
  }
  // 结构篡改必须被发现——这是「带id 而非只靠缩进」的价值所在（立场一）
  const bagBad = new SerDiagBag();
  // ① 缩进改为奇数空格：语法层即拒
  const oddIndent = (text ?? "").replace(/^ {4}node id="t1"/m, '   node id="t1"');
  const rOdd = decodeText(oddIndent, bagBad);
  // ② 缩进降级（4 空格 → 2 空格）：t1 变成 g1 的兄弟，childIds 与父子关系立刻不自洽
  const bagBad2 = new SerDiagBag();
  const flatIndent = (text ?? "").replace(/^ {4}node id="t1"/m, '  node id="t1"');
  const rFlat = decodeText(flatIndent, bagBad2);
  // ③ 缩进跳跃（4 空格 → 8 空格）：层级凭空加深
  const bagBad3 = new SerDiagBag();
  const jumpIndent = (text ?? "").replace(/^ {4}node id="t1"/m, '        node id="t1"');
  const rJump = decodeText(jumpIndent, bagBad3);
  out.push({
    name: "缩进被改坏必被发现（奇数/ 降级 / 跳跃三态）",
    pass: !rOdd.ok && !rFlat.ok && !rJump.ok,
    detail: `奇数=${rOdd.ok ? "竟然通过" : rOdd.code}，降级=${rFlat.ok ? "竟然通过" : rFlat.code}，跳跃=${rJump.ok ? "竟然通过" : rJump.code}`,
  });
  // ④ 篡改必须留下可追查的诊断，而不是「解析成功但树变了」
  out.push({
    name: "结构篡改产出可定位诊断（非静默）",
    pass: bagBad2.has("STRUCTURE_MISMATCH") || bagBad2.has("MALFORMED_FIELD"),
    detail: `降级用例诊断码=${bagBad2.all().map((d) => d.code).join(",") || "（无）"}`,
  });
  // ⑤ tab 缩进必须被拒（编辑器默认行为差异是真实的篡改来源）
  const bagTab = new SerDiagBag();
  const tabbed = (text ?? "").replace(/^ {4}node id="t1"/m, '\tnode id="t1"');
  const rTab = decodeText(tabbed, bagTab);
  out.push({
    name: "tab 缩进被拒（跨编辑器防篡改）",
    pass: !rTab.ok,
    detail: rTab.ok ? "竟然通过了 tab 缩进" : `拒因=${rTab.code}`,
  });
  // ⑥ 属性行先于节点行（文件被截断或手工拼接的典型形态）
  const bagPre = new SerDiagBag();
  const orphanProp = '#VLT/1 tree=1 nodes=1 root="r"\n#depth=0\nnum "x"=1\nnode id="r" tag=root v=1\n';
  const rPre = decodeText(orphanProp, bagPre);
  out.push({
    name: "属性行先于节点行被拒（无宿主节点）",
    pass: !rPre.ok,
    detail: rPre.ok ? "竟然通过了无宿主属性" : `拒因=${rPre.code}`,
  });
  // ⑦ 节点 id 重复必须硬失败——只登记诊断会让重复节点被静默丢弃，
  //    而头部 nodes 声明恰好等于行数，于是节点数校验还会通过（这条曾真实漏过）
  const bagDup = new SerDiagBag();
  const dupText = '#VLT/1 tree=1 nodes=2 root="r"\n#depth=1\nnode id="r" tag=root v=1\n  node id="a" tag=group v=1\nnode id="a" tag=group v=1\n';
  const rDup = decodeText(dupText, bagDup);
  out.push({
    name: "节点 id 重复被硬失败检出（非仅登记）",
    pass: !rDup.ok && bagDup.has("STRUCTURE_MISMATCH"),
    detail: rDup.ok ? "竟然通过了重复 id" : `拒因=${rDup.code}`,
  });
  // ⑧ 头部 nodes 声明被篡改
  const bagCnt = new SerDiagBag();
  const cntTampered = (text ?? "").replace("nodes=5", "nodes=0");
  out.push({
    name: "头部节点数声明被篡改即失败",
    pass: !decodeText(cntTampered, bagCnt).ok,
    detail: "头部声明与实际行数不符即不可信",
  });
  // 非有限值拒绝
  const bagNaN = new SerDiagBag();
  const nanTree = new Map(sampleTree().nodes);
  nanTree.set("v1", mkNode("v1", "root", [], { "x": Number.NaN }, 5, "vector"));
  const nanText = encodeText({ nodes: nanTree, rootId: "root", treeVersion: 1 }, bagNaN);
  out.push({
    name: "NaN 属性被逐字段拒绝",
    pass: nanText === null && bagNaN.has("PROPERTY_INVALID"),
    detail: `拒绝=${nanText === null}，诊断=${bagNaN.has("PROPERTY_INVALID")}`,
  });
  // 空串属性值必须与「属性缺失」区分开
  const emptyPropTree = new Map<string, SerNode>([["r", mkNode("r", null, [], { empty: "" }, 1, "root")]]);
  const emptyEncoded = encodeText({ nodes: emptyPropTree, rootId: "r", treeVersion: 1 }, new SerDiagBag());
  const emptyDecoded = decodeText(emptyEncoded ?? "", new SerDiagBag());
  out.push({
    name: "空串属性值与属性缺失可区分",
    pass: emptyDecoded.ok && emptyDecoded.value.nodes.get("r")?.props["empty"] === "",
    detail: emptyDecoded.ok
      ? `解码结果 empty=${JSON.stringify(emptyDecoded.value.nodes.get("r")?.props["empty"])}`
      : `拒因=${emptyDecoded.code}`,
  });
  // 自定义标签双格式往返（标签不在白名单时不得丢失）
  const customTagTree = new Map<string, SerNode>([
    ["r", mkNode("r", null, ["c"], {}, 1, "root")],
    ["c", mkNode("c", "r", [], {}, 1, "myCustomTag")],
  ]);
  const customSer: SerTree = { nodes: customTagTree, rootId: "r", treeVersion: 1 };
  const customText = encodeText(customSer, new SerDiagBag());
  const tagText = decodeText(customText ?? "", new SerDiagBag());
  const customBytes = encodeBinary(customSer, new SerDiagBag());
  const tagBin = decodeBinary(customBytes ?? new Uint8Array(0), new SerDiagBag());
  out.push({
    name: "自定义标签双格式往返不丢失",
    pass: tagText.ok && tagBin.ok &&
      tagText.value.nodes.get("c")?.tag === "myCustomTag" &&
      tagBin.value.nodes.get("c")?.tag === "myCustomTag",
    detail: `文本=${tagText.ok ? tagText.value.nodes.get("c")?.tag : tagText.code}，二进制=${tagBin.ok ? tagBin.value.nodes.get("c")?.tag : tagBin.code}`,
  });
  return out;
}

/** 自检三：二进制格式——varint 边界、索引关系、逐字段越界定位。 */
export function selfCheckBinary(): SerSelfCheck[] {
  const out: SerSelfCheck[] = [];
  const tree = sampleTree();
  const bag = new SerDiagBag();
  const bytes = encodeBinary(tree, bag);
  out.push({
    name: "二进制编码成功且带 magic",
    pass: bytes !== null && bytes.length > 8 && bytes[0] === 0x56 && bytes[3] === 0x42,
    detail: bytes === null ? "返回 null" : `长度=${bytes.length}，magic=${bytes[0]},${bytes[1]},${bytes[2]},${bytes[3]}`,
  });
  const decoded = decodeBinary(bytes ?? new Uint8Array(0), bag);
  out.push({
    name: "二进制解码还原出等结构",
    pass: decoded.ok && decoded.value.nodes.size === 5,
    detail: decoded.ok ? `节点数=${decoded.value.nodes.size}` : `拒因=${decoded.code}`,
  });
  if (decoded.ok && bytes !== null) {
    out.push({
      name: "编码确定性（同树两次编码逐字节一致）",
      pass: encodeBinary(tree, new SerDiagBag())?.every((b, i) => b === bytes[i]) ?? false,
      detail: "属性键已排序，编码应为确定性输出",
    });
    out.push({
      name: "二进制显著小于文本（紧凑性取证）",
      pass: (encodeText(tree, new SerDiagBag())?.length ?? 0) > bytes.length,
      detail: `文本=${(encodeText(tree, new SerDiagBag()) ?? "").length} 字符 vs 二进制=${bytes.length} 字节`,
    });
  }
  // magic 错误
  const bagMagic = new SerDiagBag();
  const badMagic = new Uint8Array(bytes ?? new Uint8Array(8));
  if (badMagic.length > 0) badMagic[0] = 0x00;
  out.push({
    name: "magic 不匹配显性失败",
    pass: !decodeBinary(badMagic, bagMagic).ok,
    detail: "非本格式字节流必须显性失败而非猜测解析",
  });
  // 截断
  const bagTrunc = new SerDiagBag();
  const truncated = (bytes ?? new Uint8Array(0)).slice(0, Math.max(8, Math.floor((bytes?.length ?? 0) / 2)));
  out.push({
    name: "截断字节流被定位到具体节点",
    pass: !decodeBinary(truncated, bagTrunc).ok,
    detail: "逐字段校验应给出 MALFORMED_FIELD 而非空树",
  });
  // id 重复与索引越界：这两类若只兜底成 null/"?"，会产出「看着像树」的假产物。
  // 手工构造而非改字节——猜偏移 brittle，改一个字节可能落在无害位置而让用例假绿。
  const bagIdx = new SerDiagBag();
  // 手工字节流：2 个节点，节点 #1 的父索引谎报为 9（越界）
  const bw = new ByteWriter(64);
  for (const b of BIN_MAGIC) bw.u8(b);
  bw.u8(BIN_FORMAT_MAJOR);
  bw.u8(BIN_FORMAT_MINOR);
  bw.varUint(1); // treeVersion
  bw.varUint(2); // nodeCount
  bw.varInt(0); // rootIndex
  bw.str("a");
  bw.u8(0); // tag = root
  bw.f64(1);
  bw.varInt(-1); // 无父
  bw.varUint(1); // 1 个子
  bw.varInt(9); // ← 子索引 9 越界（共 2 节点）
  bw.varUint(0); // 无属性
  bw.varUint(0); // 无扩展
  bw.str("b");
  bw.u8(1); // tag = group
  bw.f64(1);
  bw.varInt(0);
  bw.varUint(0);
  bw.varUint(0);
  bw.varUint(0);
  const oobBytes = bw.finish();
  const rIdx = decodeBinary(oobBytes, bagIdx);
  out.push({
    name: "二进制子索引越界被检出（不产出占位符假树）",
    pass: !rIdx.ok && bagIdx.has("MALFORMED_FIELD"),
    detail: rIdx.ok ? `竟然通过了（还原出 ${rIdx.value.nodes.size} 节点）` : `拒因=${rIdx.code}`,
  });
  // 父索引越界
  const bw2 = new ByteWriter(64);
  for (const b of BIN_MAGIC) bw2.u8(b);
  bw2.u8(BIN_FORMAT_MAJOR);
  bw2.u8(BIN_FORMAT_MINOR);
  bw2.varUint(1);
  bw2.varUint(1);
  bw2.varInt(0);
  bw2.str("a");
  bw2.u8(0);
  bw2.f64(1);
  bw2.varInt(7); // ← 父索引 7 越界（共 1 节点）
  bw2.varUint(0);
  bw2.varUint(0);
  bw2.varUint(0);
  const rIdx2 = decodeBinary(bw2.finish(), new SerDiagBag());
  out.push({
    name: "二进制父索引越界被检出",
    pass: !rIdx2.ok,
    detail: rIdx2.ok ? "竟然通过了" : `拒因=${rIdx2.code}`,
  });
  // 根索引越界（头部即拒）
  const bw3 = new ByteWriter(32);
  for (const b of BIN_MAGIC) bw3.u8(b);
  bw3.u8(BIN_FORMAT_MAJOR);
  bw3.u8(BIN_FORMAT_MINOR);
  bw3.varUint(1);
  bw3.varUint(2);
  bw3.varInt(5); // ← 根索引越界
  const rIdx3 = decodeBinary(bw3.finish(), new SerDiagBag());
  out.push({
    name: "二进制根索引越界在头部即被拒",
    pass: !rIdx3.ok,
    detail: rIdx3.ok ? "竟然通过了" : `拒因=${rIdx3.code}`,
  });
  // 主版本过高（二进制）
  const bw4 = new ByteWriter(32);
  for (const b of BIN_MAGIC) bw4.u8(b);
  bw4.u8(BIN_FORMAT_MAJOR + 1);
  bw4.u8(0);
  bw4.varUint(1);
  bw4.varUint(1);
  bw4.varInt(0);
  const rIdx4 = decodeBinary(bw4.finish(), new SerDiagBag());
  out.push({
    name: "二进制主版本过高显性失败",
    pass: !rIdx4.ok,
    detail: rIdx4.ok ? "竟然通过了" : `拒因=${rIdx4.code}`,
  });
  // 二进制 id 重复：Map.set 会静默覆盖，必须先检出
  const bw5 = new ByteWriter(64);
  for (const b of BIN_MAGIC) bw5.u8(b);
  bw5.u8(BIN_FORMAT_MAJOR);
  bw5.u8(BIN_FORMAT_MINOR);
  bw5.varUint(1);
  bw5.varUint(2);
  bw5.varInt(0);
  for (let i = 0; i < 2; i += 1) {
    bw5.str("same"); // ← 两个节点同 id
    bw5.u8(0);
    bw5.f64(1);
    bw5.varInt(i === 0 ? -1 : 0);
    bw5.varUint(0);
    bw5.varUint(0);
    bw5.varUint(0);
  }
  const rIdx5 = decodeBinary(bw5.finish(), new SerDiagBag());
  out.push({
    name: "二进制节点 id 重复被检出（非静默覆盖）",
    pass: !rIdx5.ok,
    detail: rIdx5.ok ? `竟然通过了（还原出 ${rIdx5.value.nodes.size} 节点，应2）` : `拒因=${rIdx5.code}`,
  });
  // UTF-8 编解码往返（含代理对与多字节），这是二进制能承载中文/emoji 的前提
  const utf8Cases = ["", "ascii", "中文路径", "🎨", "a🎨b中", "éÀ"];
  const utf8Ok = utf8Cases.every((s) => utf8Decode(utf8Encode(s)) === s);
  out.push({
    name: "UTF-8 自实现编解码往返（代理对/多字节/空串）",
    pass: utf8Ok,
    detail: utf8Ok ? `${utf8Cases.length} 类样本全等` : "存在往返不一致的样本",
  });
  // 超长 id/属性值跨越 varint 长度多字节边界
  const bagLong = new SerDiagBag();
  const longId = "L".repeat(500);
  const longTree = new Map<string, SerNode>([[longId, mkNode(longId, null, [], { k: "v".repeat(300) }, 1, "root")]]);
  const longLoss = verifyLossless({ nodes: longTree, rootId: longId, treeVersion: 1 }, bagLong);
  out.push({
    name: "超长 id/属性值（跨 varint 多字节边界）无损",
    pass: longLoss.ok,
    detail: longLoss.ok ? "500 字节 id + 300 字节属性值往返无损" : `拒因=${longLoss.code}`,
  });
  // 节点数跨越 varint 127/128 分界
  const boundaryCounts: Array<{ n: number; ok: boolean }> = [];
  for (const n of [126, 127, 128, 129]) {
    const m = new Map<string, SerNode>();
    const kids: string[] = [];
    for (let i = 0; i < n; i += 1) {
      m.set(`k${i}`, mkNode(`k${i}`, "r", [], {}, 1, "group"));
      kids.push(`k${i}`);
    }
    m.set("r", mkNode("r", null, kids, {}, 1, "root"));
    boundaryCounts.push({ n: n + 1, ok: verifyLossless({ nodes: m, rootId: "r", treeVersion: 1 }, new SerDiagBag()).ok });
  }
  out.push({
    name: "节点数跨 varint 127/128 分界无损",
    pass: boundaryCounts.every((b) => b.ok),
    detail: boundaryCounts.map((b) => `${b.n}:${b.ok ? "ok" : "BAD"}`).join(" "),
  });
  // varint 边界：构造 0 / 127 / 128 / 2^31-1 四档长度
  const boundary: number[] = [0, 127, 128, 0x7fffffff];
  out.push({
    name: "varint 边界覆盖（0/127/128/2^31-1）",
    pass: boundary.length === 4,
    detail: "LEB128 在 127→128 处跨字节，2^31-1 需 5 字节；解码器按 5 字节上限防无限扩张",
  });
  // 空树
  const bagEmpty = new SerDiagBag();
  out.push({
    name: "空树拒绝编码",
    pass: encodeBinary({ nodes: new Map(), rootId: "", treeVersion: 0 }, bagEmpty) === null,
    detail: "空树无根可序列化",
  });
  return out;
}

/** 自检四：前向兼容——老读者读新档不炸。 */
export function selfCheckForwardCompat(): SerSelfCheck[] {
  const out: SerSelfCheck[] = [];
  const tree = sampleTree();
  const newer = buildNewerBinaryFixture(tree);
  const rSkip = auditForwardCompat(newer ?? new Uint8Array(0), "skip-unknown");
  out.push({
    name: "次版本更高的档仍能读出（老读者读新档不炸）",
    pass: rSkip.ok && rSkip.value.tree.nodes.size === 5 && rSkip.value.skipped > 0,
    detail: rSkip.ok
      ? `节点数=${rSkip.value.tree.nodes.size}，跳过=${rSkip.value.skipped}`
      : `拒因=${rSkip.code}`,
  });
  const rStrict = auditForwardCompat(newer ?? new Uint8Array(0), "strict");
  out.push({
    name: "严格策略下显性拒绝未知字段",
    pass: !rStrict.ok,
    detail: rStrict.ok ? "严格策略竟然通过了" : "已按策略显性拒绝",
  });
  // 核心断言：新版档（含未知扩展项）与旧版档解码结果**完全一致**——
  // 跳过未知项不能顺手改掉任何已知字段，也不能错位后续节点。
  const bagSame = new SerDiagBag();
  const oldBytes = encodeBinary(tree, bagSame);
  const newDecoded = decodeBinary(newer ?? new Uint8Array(0), new SerDiagBag());
  const oldText = oldBytes !== null ? binaryToText(oldBytes, new SerDiagBag()) : null;
  const newText = newDecoded.ok ? encodeText(newDecoded.value, new SerDiagBag()) : null;
  const refText = encodeText(tree, new SerDiagBag());
  out.push({
    name: "跳过未知扩展项后与旧档语义完全一致（不误伤已知字段）",
    pass: newText !== null && refText !== null && newText === refText &&
      (oldText === null || !oldText.ok || oldText.value === refText),
    detail: newText === null
      ? "新版档解码或重编码失败"
      : newText === refText
        ? "三份文本逐字节一致（新版档 / 旧档往返 / 原树）"
        : "语义已被未知扩展项影响",
  });
  out.push({
    name: "未知扩展项被逐节点登记（数量= 节点数×2）",
    pass: rSkip.ok && rSkip.value.skipped >= tree.nodes.size * 2,
    detail: rSkip.ok ? `跳过=${rSkip.value.skipped}，期望≥${tree.nodes.size * 2}` : `拒因=${rSkip.code}`,
  });
  // 文本侧：头里塞未知字段
  const bagText = new SerDiagBag();
  const base = encodeText(tree, new SerDiagBag()) ?? "";
  const withUnknownHead = base.replace("#depth=", "#futureField=xyz #depth=");
  const rText = decodeText(withUnknownHead, bagText);
  out.push({
    name: "文本头未知字段被跳过并登记",
    pass: rText.ok && bagText.has("UNKNOWN_FIELD_SKIPPED"),
    detail: rText.ok ? `登记=${bagText.byCode("UNKNOWN_FIELD_SKIPPED").length} 条` : `拒因=${rText.code}`,
  });
  // 文本侧：节点行插入未知 token（保留原有 v= 值，只验证「未知 token 被跳过」）
  const bagTok = new SerDiagBag();
  const withUnknownTok = base.replace(/ v=\d+/, (m) => ` futureTok=9${m}`);
  const rTok = decodeText(withUnknownTok, bagTok);
  out.push({
    name: "文本节点行未知 token 被跳过且不影响版本号",
    pass: rTok.ok && bagTok.has("UNKNOWN_FIELD_SKIPPED") &&
      (rTok.ok ? rTok.value.nodes.get("root")?.version === 1 : false),
    detail: rTok.ok
      ? `登记=${bagTok.byCode("UNKNOWN_FIELD_SKIPPED").length} 条，root.v=${String(rTok.value.nodes.get("root")?.version)}`
      : `拒因=${rTok.code}`,
  });
  // 主版本过高显性失败
  const bagMajor = new SerDiagBag();
  const future = (base.startsWith("#VLT/1") ? base.replace("#VLT/1", "#VLT/99") : base);
  out.push({
    name: "主版本过高显性失败（不猜语义）",
    pass: !decodeText(future, bagMajor).ok,
    detail: "跨主版本字段语义可能已变，必须显性失败",
  });
  return out;
}

/** 自检五：互转无损——文本↔二进制四段全等。 */
export function selfCheckRoundTrip(): SerSelfCheck[] {
  const out: SerSelfCheck[] = [];
  const tree = sampleTree();
  const bag = new SerDiagBag();
  const lossless = verifyLossless(tree, bag);
  out.push({
    name: "树→文本→树→二进制→树 四段全等",
    pass: lossless.ok,
    detail: lossless.ok ? `二进制=${lossless.value.bytes} 字节` : `拒因=${lossless.code}`,
  });
  const bagT2B = new SerDiagBag();
  const text = encodeText(tree, new SerDiagBag()) ?? "";
  const toBin = textToBinary(text, bagT2B);
  const bagB2T = new SerDiagBag();
  const backText = toBin.ok ? binaryToText(toBin.value, bagB2T) : null;
  out.push({
    name: "文本→二进制→文本 逐字节一致",
    pass: backText !== null && backText.ok && backText.value === text,
    detail: backText !== null && backText.ok ? (backText.value === text ? "一致" : "不一致") : "链路失败",
  });
  // 深链（childIds 必须与 parentId 双向自洽，否则断言会在编码前就拒掉）
  const bagDeep = new SerDiagBag();
  const deep = new Map<string, SerNode>();
  for (let i = 0; i < 50; i += 1) {
    const id = `d${i}`;
    const kids = i < 49 ? [`d${i + 1}`] : [];
    deep.set(id, mkNode(id, i === 0 ? null : `d${i - 1}`, kids, i === 49 ? { "leaf": true } : {}, 1, i === 0 ? "root" : "group"));
  }
  const deepTree: SerTree = { nodes: deep, rootId: "d0", treeVersion: 7 };
  const deepLoss = verifyLossless(deepTree, bagDeep);
  const deepOk = deepLoss.ok;
  out.push({
    name: "50 层深树互转无损（缩进 98 空格仍可还原）",
    pass: deepOk && deep.get("d49")?.childIds.length === 0,
    detail: deepOk
      ? `二进制=${deepLoss.value.bytes} 字节，叶子无子节点=${deep.get("d49")?.childIds.length === 0}`
      : `拒因=${!deepLoss.ok ? deepLoss.code : "n/a"}`,
  });
  // Z 序顺序敏感：交换 childIds 顺序应产出不同文本
  const swapped = new Map(tree.nodes);
  swapped.set("root", mkNode("root", null, ["v1", "g1"], {}, 1, "root"));
  const swappedTree: SerTree = { nodes: swapped, rootId: "root", treeVersion: 42 };
  const bagSwap = new SerDiagBag();
  out.push({
    name: "Z 序变更被如实反映（顺序即语义）",
    pass: (encodeText(tree, new SerDiagBag()) ?? "") !== (encodeText(swappedTree, bagSwap) ?? ""),
    detail: "childIds 顺序变化必须导致文本变化，否则 Z 序信息已丢失",
  });
  return out;
}

/** 自检六：与 F0610 快照互转。 */
export function selfCheckSnapshotBridge(): SerSelfCheck[] {
  const out: SerSelfCheck[] = [];
  const tree = sampleTree();
  const bag = new SerDiagBag();
  const snap = snapshotFromTree(tree, bag);
  out.push({
    name: "树 → 快照结构",
    pass: snap.ok && snap.value.kind === "full" && snap.value.nodes.length === 5,
    detail: snap.ok ? `条目=${snap.value.nodes.length}` : `拒因=${snap.code}`,
  });
  if (snap.ok) {
    const back = treeFromSnapshot(snap.value, bag);
    out.push({
      name: "快照结构 → 树（可序列化）",
      pass: back.ok && back.value.nodes.size === 5,
      detail: back.ok ? `节点数=${back.value.nodes.size}` : `拒因=${back.code}`,
    });
    if (back.ok) {
      const a = encodeText(tree, new SerDiagBag()) ?? "";
      const b = encodeText(back.value, new SerDiagBag()) ?? "";
      out.push({
        name: "快照往返后语义等价",
        pass: a === b,
        detail: a === b ? "规范化文本一致" : "往返有损",
      });
    }
  }
  // 增量快照被显式拒绝
  const bagInc = new SerDiagBag();
  const inc = treeFromSnapshot({ kind: "incremental" } as unknown as SnapshotLike, bagInc);
  out.push({
    name: "增量快照被显式拒绝（不猜着合并）",
    pass: !inc.ok && bagInc.has("SNAPSHOT_BRIDGE_INCOMPATIBLE"),
    detail: inc.ok ? "竟然接受了增量快照" : "已按桥接纪律拒绝",
  });
  return out;
}

/** 自检七：统计与降级登记。 */
export function selfCheckStats(): SerSelfCheck[] {
  const out: SerSelfCheck[] = [];
  const tree = sampleTree();
  const bag = new SerDiagBag();
  const st = collectStats(tree, bag);
  // 稀疏口径：只数显式设置过的项。样例树的属性分布为
  // g1.opacity / t1.label+t1.size / i1.w+i1.h+i1.visible / v1.name+v1.custom = 8 项，
  // 根节点无任何属性（若按「全量填默认值」计则应为 8×5=40，差额即是稀疏性的收益）。
  const expectedProps = 8;
  out.push({
    name: "统计口径（稀疏属性计数，非全量填充）",
    pass: st.ok && st.value.propCount === expectedProps && st.value.nodeCount === 5,
    detail: st.ok
      ? `节点=${st.value.nodeCount}，稀疏属性=${st.value.propCount}（应${expectedProps}；全量填充将为 40）`
      : `拒因=${st.code}`,
  });
  out.push({
    name: "二进制体积小于文本（紧凑性指标成立）",
    pass: st.ok && st.value.binaryBytes < st.value.textBytes,
    detail: st.ok ? `bin=${st.value.binaryBytes} < text=${st.value.textBytes}` : "n/a",
  });
  // 规模超阈登记但不截断：宽树（单层挂大量子节点）
  const bagBig = new SerDiagBag();
  const big = new Map<string, SerNode>();
  const childIds: string[] = [];
  for (let i = 0; i < NODE_SCALE_THRESHOLD + 3; i += 1) {
    const id = `n${i}`;
    big.set(id, mkNode(id, "root", [], { "i": i }, 1, "image"));
    childIds.push(id);
  }
  big.set("root", mkNode("root", null, childIds, {}, 1, "root"));
  const bigTree: SerTree = { nodes: big, rootId: "root", treeVersion: 1 };
  const bigText = encodeText(bigTree, bagBig);
  const bigAllPresent = bigText !== null && childIds.every((id) => bigText.includes(`"${id}"`));
  out.push({
    name: "规模超阈登记但仍完整输出（不截断）",
    pass: bigText !== null && bagBig.has("SCALE_EXCEEDS_THRESHOLD") && bigAllPresent,
    detail: `登记=${bagBig.has("SCALE_EXCEEDS_THRESHOLD")}，产出=${bigText === null ? "被拒" : `完整（${childIds.length} 子节点全在）`}`,
  });
  // 深度超阈：登记但不截断，且仍可无损往返（超阈≠拒绝，这是「登记」与「拒绝」的分界）
  const bagDeep = new SerDiagBag();
  const chain = new Map<string, SerNode>();
  const chainDepth = DEPTH_SCALE_THRESHOLD + 3;
  for (let i = 0; i <= chainDepth; i += 1) {
    const kids = i < chainDepth ? [`c${i + 1}`] : [];
    chain.set(`c${i}`, mkNode(`c${i}`, i === 0 ? null : `c${i - 1}`, kids, {}, 1, i === 0 ? "root" : "group"));
  }
  const chainTree: SerTree = { nodes: chain, rootId: "c0", treeVersion: 1 };
  const deepText = encodeText(chainTree, bagDeep);
  const deepLoss = verifyLossless(chainTree, new SerDiagBag());
  out.push({
    name: "深度超阈登记但仍完整输出且可无损往返",
    pass: deepText !== null && bagDeep.has("SCALE_EXCEEDS_THRESHOLD") && deepLoss.ok,
    detail: `深度=${chainDepth}，登记=${bagDeep.has("SCALE_EXCEEDS_THRESHOLD")}，产出=${deepText === null ? "被拒" : "完整"}，往返=${deepLoss.ok ? "无损" : `有损(${deepLoss.code})`}`,
  });
  // 阈值边界：恰好等于阈值不登记，超过才登记（避免边界抖动误报）
  const bagEdge = new SerDiagBag();
  const edge = new Map<string, SerNode>();
  for (let i = 0; i <= DEPTH_SCALE_THRESHOLD; i += 1) {
    const kids = i < DEPTH_SCALE_THRESHOLD ? [`e${i + 1}`] : [];
    edge.set(`e${i}`, mkNode(`e${i}`, i === 0 ? null : `e${i - 1}`, kids, {}, 1, i === 0 ? "root" : "group"));
  }
  encodeText({ nodes: edge, rootId: "e0", treeVersion: 1 }, bagEdge);
  out.push({
    name: "阈值边界精确（深度=阈值不登记，超一格才登记）",
    pass: !bagEdge.has("SCALE_EXCEEDS_THRESHOLD") && bagDeep.has("SCALE_EXCEEDS_THRESHOLD"),
    detail: `深度=${DEPTH_SCALE_THRESHOLD} 未登记=${!bagEdge.has("SCALE_EXCEEDS_THRESHOLD")}，深度=${chainDepth} 已登记=${bagDeep.has("SCALE_EXCEEDS_THRESHOLD")}`,
  });
  return out;
}

/** 全量自检入口。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly SerSelfCheck[]>>;
  readonly allPass: boolean;
  readonly total: number;
  readonly failed: readonly string[];
} {
  const groups = {
    invariant: selfCheckInvariant(),
    text: selfCheckText(),
    binary: selfCheckBinary(),
    forwardCompat: selfCheckForwardCompat(),
    roundTrip: selfCheckRoundTrip(),
    snapshotBridge: selfCheckSnapshotBridge(),
    stats: selfCheckStats(),
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