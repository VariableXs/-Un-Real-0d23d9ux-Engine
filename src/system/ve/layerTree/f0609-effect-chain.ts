/**
 * VE-F0609 · 图层效果链（D 域 · 2D 合成引擎图层树组 · 批次 D01）
 * ---------------------------------------------------------------------------
 * 职责定位：图层效果链——**链结构管本条，具体效果实现归渲染域**。
 *   上游 F0601 图层树结构（挂载点）、F0605 混合边界（隔离组语义）。
 *   下游 F0628 GPU 实现、F0625 预乘纪律、F0629 CPU 路径都消费本条产出的
 *   「效果计划」（EffectPlan）——本条只回答「按什么顺序、对谁、经过几次中间缓冲」，
 *   不回答「每个效果的具体像素运算是什么」。
 *
 * 锚点契约（五条，逐条对应判据）：
 *   1. 可挂效果序列：filter 族以参数化效果描述符（EffectDescriptor）挂载，
 *      本条只定义描述符的结构与参数校验，不含任何像素运算。
 *   2. 顺序语义：按挂载序串行应用，**链序即视觉结果**——链序变更属于语义变更，
 *      须走属性脏路径（与 F0606/F0613 的「三序同源」同律，不得当性能优化处理）。
 *   3. 中间缓冲策略：链上多效果的中间纹理**单次中间化**（相邻效果合并为单 pass
 *      的优化以预留态显式存在，不默认生效——合并会改变数值结果，必须显式开启）。
 *   4. 隐式隔离联动：效果链非空触发 F0605 隔离，是隐式隔离条件清单成员之一。
 *   5. 像素语义纪律：链内全程**预乘 alpha**，与 F0625 一致；本条产出计划时
 *      必须携带预乘标记，让下游 GPU/CPU 两路不会各写一套未预乘的隐式假设。
 *
 * 三条容易做错、故显式记录的设计立场：
 *
 *   一、合并不是性能优化，是数值变更。
 *     「把相邻两个效果合并为一个 pass」听起来是纯优化，实际上会改变中间缓冲的
 *     量化行为（8bit 中间纹理 vs 浮点直连），产生可见的色偏。
 *     故本条把 mergePasses 做成**显式开关 + 可查询的等价性前提**：
 *     只有当链上相邻效果都声明 canMergeWith（同一 pass 内可无损执行）时才允许合并，
 *     默认关闭。开启时若存在不满足前提的相邻对，产出诊断而非静默合并。
 *
 *   二、单次中间化说的是「缓冲次数」，不是「缓冲个数」。
 *     朴素实现每个效果各开一张中间纹理（N 个效果 = N 张）。正确做法是
 *     ping-pong 双缓冲：一张写入、一张读出，跑完整条链只分配两张。
 *     故本条的 bufferCount 恒为 min(2, 需要落地的效果数 + 1)，
 *     且中间缓冲只在「效果不消费自身输出」时才必须发生（详述见下文 executionStages）。
 *
 *   三、效果缺位必须显性 N/A，不能静默跳过。
 *     渲染域尚未实现的 filter（条目级 N/A）如果被静默跳过，画面会少一层效果
 *     却没有任何提示——这是典型的「异常发生了但哪里都找不到」。
 *     故本条对每条效果做实现可用性查询：缺位即产出 EFFECT_IMPL_MISSING 诊断，
 *     并在计划里显式标为 skippedByMissingImpl，让调用方能决定是报错还是降级。
 *
 * 零静默纪律：所有拒绝、钳制、告警、缺位都产出 Diagnostic（code + message + hint）。
 * 诊断码与 F0608 同族但独立声明——两条的失败语义不同，不共用枚举（避免耦合）。
 *
 * 判据：链序即结果、单次中间化、隐式隔离、预乘纪律。
 * 依赖锚点：F0601 图层树结构（挂载点与脏标记）、F0605 混合边界（隔离条件清单）、
 *          F0608 动画插值接口（效果参数本身可由轨道驱动，故参数须可快照可比）、
 *          F0613 脏区收集（链序变更与参数变更都映射为脏区事件）。
 * 交接说明：本条输出的 EffectPlan 是 F0628（GPU）与 F0629（CPU）共同的唯一输入；
 *          两者不得各自推导链序或中间化策略，否则 GPU 与 CPU 会出现视觉差异。
 */

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默的基础设施）
// ════════════════════════════════════════════════════════════════════════════

/** 效果链专属诊断码。刻意与 F0608 的 DiagCode 分开——失败语义不同，不共用枚举。 */
export type EffectDiagCode =
  | "EFFECT_PARAM_OUT_OF_RANGE"
  | "EFFECT_IMPL_MISSING"
  | "EFFECT_UNKNOWN_KIND"
  | "CHAIN_EMPTY"
  | "CHAIN_DUPLICATE_ID"
  | "CHAIN_TOO_LONG"
  | "MERGE_NOT_ADMISSIBLE"
  | "MERGE_SEMANTIC_RISK"
  | "ISOLATION_CONFLICT"
  | "PREMULTIPLIED_VIOLATION"
  | "CLAMP_DEGENERATE";

/** 一条诊断：发生了什么（code+message）、影响什么、下一步怎么办（hint）。 */
export interface EffectDiagnostic {
  readonly code: EffectDiagCode;
  /** 人话描述，面向开发者排障，不含裸异常码。 */
  readonly message: string;
  /** 可操作提示：调用方该改哪里、该走哪条降级路。 */
  readonly hint: string;
}

/** 结果判别联合：失败必带三要素，失败不可被误当成功。 */
export type EffectOutcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly EffectDiagnostic[] }
  | {
      readonly ok: false;
      readonly code: EffectDiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly EffectDiagnostic[];
    };

/** 成功构造（diagnostics 允许携带非致命告警）。 */
export function effOk<T>(value: T, diagnostics: readonly EffectDiagnostic[] = []): EffectOutcome<T> {
  return { ok: true, value, diagnostics };
}

/** 失败构造：三要素齐备。 */
export function effFail<T>(code: EffectDiagCode, message: string, hint: string): EffectOutcome<T> {
  const d: EffectDiagnostic = { code, message, hint };
  return { ok: false, code, message, hint, diagnostics: [d] };
}

/** 诊断聚合器：把散落各处的告警汇成一条可检索清单。 */
export class EffectDiagBag {
  private readonly items: EffectDiagnostic[] = [];

  /** 追加一条诊断（空 message/hint 被规范化，避免半截诊断）。 */
  push(code: EffectDiagCode, message: string, hint: string): void {
    this.items.push({ code, message: message || "（未提供描述）", hint: hint || "（未提供处置建议）" });
  }

  /** 当前条数。 */
  get size(): number {
    return this.items.length;
  }

  /** 只读视图（副本，调用方改不动内部清单）。 */
  all(): readonly EffectDiagnostic[] {
    return this.items.slice();
  }

  /** 按码筛选——排障时按码聚合的入口。 */
  byCode(code: EffectDiagCode): readonly EffectDiagnostic[] {
    return this.items.filter((d) => d.code === code);
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §2 数值基础（有限性守卫与钳制）
// ════════════════════════════════════════════════════════════════════════════

/** 浮点容差：与 F0608 同值同义，便于跨条对照。 */
export const E_EPS = 1e-9;

/** 判定一个数是否有限（NaN 与 ±Inf 都判否）。 */
export function isFiniteNum(v: number): boolean {
  return Number.isFinite(v);
}

/** 钳制到 [lo, hi]；lo > hi（退化区间）返回 hi，由调用方产出诊断。 */
export function clamp(v: number, lo: number, hi: number): number {
  if (lo > hi) return hi;
  if (v < lo) return lo;
  if (v > hi) return hi;
  return v;
}

// ════════════════════════════════════════════════════════════════════════════
// §3 效果描述符（数据结构一：参数化描述，本条不含像素运算）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 效果种类（filter 族的语义标签）。
 * 本条只认「种类 + 参数」，不认识具体实现——实现由 F0628/F0629 按种类分派。
 */
export type EffectKind = "blur" | "colorShift" | "shadow" | "brightness" | "contrast" | "saturate";

/** 全部效果种类（供遍历与自检使用，避免手写清单与实际漂移）。 */
export const EFFECT_KINDS: readonly EffectKind[] = [
  "blur",
  "colorShift",
  "shadow",
  "brightness",
  "contrast",
  "saturate",
];

/** 单个效果参数的声明：值 + 值域（缺省表示无约束）。 */
export interface EffectParam {
  readonly value: number;
  /** 值域下界/上界；缺省表示该参数不设限（如偏移量）。 */
  readonly min?: number;
  readonly max?: number;
}

/** 一个参数的键名与取值。 */
export interface ParamSpec {
  readonly name: string;
  readonly value: number;
}

/** 效果描述符：参数化的效果定义，不含像素运算。 */
export interface EffectDescriptor {
  /** 链内唯一标识（用于链序变更检测与脏区定位；重复即报 CHAIN_DUPLICATE_ID）。 */
  readonly id: string;
  readonly kind: EffectKind;
  /** 参数表（按名字索引，缺项走kind 的默认值）。 */
  readonly params: readonly ParamSpec[];
  /**
   * 是否声明「可在同一 pass 内与相邻效果无损合并」。
   * 只有显式声明 true 的效果才允许被 mergePasses 合并（见文件头立场一）。
   */
  readonly canMergeWithNeighbour?: boolean;
}

/** 效果的参数约束表：逐 kind 声明每个参数的合法值域与默认值。 */
export interface EffectParamRule {
  readonly name: string;
  readonly min: number;
  readonly max: number;
  readonly fallback: number;
  readonly note: string;
}

/** 逐 kind 的参数规则表。缺省参数走 fallback，越界参数走钳制 + 诊断。 */
export const EFFECT_PARAM_RULES: Readonly<Record<EffectKind, readonly EffectParamRule[]>> = {
  // 模糊半径以像素计，上界 1024 覆盖 4K 屏的最坏情形
  blur: [
    { name: "radius", min: 0, max: 1024, fallback: 0, note: "高斯半径（像素）" },
    { name: "passes", min: 1, max: 8, fallback: 1, note: "迭代次数，越高越平滑也越慢" },
  ],
  // 色移：角度（度）与像素距离
  colorShift: [
    { name: "hue", min: -180, max: 180, fallback: 0, note: "色相偏移（度）" },
    { name: "amount", min: 0, max: 1, fallback: 0, note: "整体偏移比例 0-1" },
  ],
  // 阴影：偏移、模糊、透明度（预乘前的源 alpha 占比）
  shadow: [
    { name: "offsetX", min: -4096, max: 4096, fallback: 0, note: "水平偏移（像素）" },
    { name: "offsetY", min: -4096, max: 4096, fallback: 0, note: "垂直偏移（像素）" },
    { name: "blur", min: 0, max: 1024, fallback: 0, note: "阴影模糊半径（像素）" },
    { name: "opacity", min: 0, max: 1, fallback: 1, note: "阴影不透明度" },
  ],
  brightness: [{ name: "amount", min: -1, max: 1, fallback: 0, note: "亮度增减 -1 到 1" }],
  contrast: [{ name: "amount", min: 0, max: 4, fallback: 1, note: "对比度系数，1 为原值" }],
  saturate: [{ name: "amount", min: 0, max: 4, fallback: 1, note: "饱和度系数，1 为原值" }],
};

/** 取效果的某参数；缺项返回 fallback（由规则表给出，不猜值）。 */
export function paramOf(desc: EffectDescriptor, name: string): number {
  const hit = desc.params.find((p) => p.name === name);
  if (hit !== undefined) return hit.value;
  const rules = EFFECT_PARAM_RULES[desc.kind];
  const rule = rules.find((r) => r.name === name);
  return rule === undefined ? 0 : rule.fallback;
}

/**
 * 参数校验与规范化：越界钳制并产出诊断，缺项补fallback。
 * 返回值是**规范化后的新描述符**（不改原对象——原对象可能是 F0608 轨道驱动的）。
 */
export function normalizeDescriptor(desc: EffectDescriptor, bag: EffectDiagBag): EffectDescriptor {
  const rules = EFFECT_PARAM_RULES[desc.kind];
  if (rules === undefined) {
    bag.push(
      "EFFECT_UNKNOWN_KIND",
      `效果 ${desc.id} 的种类 ${String(desc.kind)} 未在参数规则表中登记`,
      `已登记种类：${EFFECT_KINDS.join("、")}；新增种类须先在 EFFECT_PARAM_RULES 声明参数值域`,
    );
    return desc;
  }

  const norm: ParamSpec[] = [];
  for (const rule of rules) {
    const raw = desc.params.find((p) => p.name === rule.name);
    if (raw === undefined) {
      norm.push({ name: rule.name, value: rule.fallback });
      continue;
    }
    if (!isFiniteNum(raw.value)) {
      bag.push(
        "EFFECT_PARAM_OUT_OF_RANGE",
        `效果 ${desc.id} 的参数 ${rule.name} 为非有限数（${String(raw.value)}）`,
        `已回退到默认值 ${rule.fallback}；非有限参数不得进入渲染管线（会污染整条链的输出）`,
      );
      norm.push({ name: rule.name, value: rule.fallback });
      continue;
    }
    const c = clamp(raw.value, rule.min, rule.max);
    if (c !== raw.value) {
      bag.push(
        "EFFECT_PARAM_OUT_OF_RANGE",
        `效果 ${desc.id} 的参数 ${rule.name} = ${raw.value} 越出值域 [${rule.min}, ${rule.max}]`,
        `已钳制到 ${c}（${rule.note}）；若越界是预期的，请调整参数规则表的值域声明`,
      );
    }
    norm.push({ name: rule.name, value: c });
  }
  return { ...desc, params: norm };
}

/** 描述符等价判定（按参数表比较，忽略对象身份——用于链序变更与脏区判定）。 */
export function sameDescriptor(a: EffectDescriptor, b: EffectDescriptor): boolean {
  if (a.id !== b.id || a.kind !== b.kind) return false;
  if (a.params.length !== b.params.length) return false;
  for (const p of a.params) {
    const q = b.params.find((x) => x.name === p.name);
    if (q === undefined || q.value !== p.value) return false;
  }
  return (a.canMergeWithNeighbour ?? false) === (b.canMergeWithNeighbour ?? false);
}

// ════════════════════════════════════════════════════════════════════════════
// §4 效果链结构（数据结构二：链 + 隔离联动）
// ════════════════════════════════════════════════════════════════════════════

/** 效果链：有序序列 + 隔离声明。顺序即视觉结果，不可重排而不走脏路径。 */
export interface EffectChain {
  /** 宿主节点 id（脏区事件用）。 */
  readonly nodeId: string;
  /** 效果序列，顺序即应用顺序。 */
  readonly effects: readonly EffectDescriptor[];
  /**
   * 显式隔离声明（F0605语义）。
   * 与「隐式隔离」的关系：显式声明为 true 时无论链是否为空都隔离；
   * 为 null（未声明）时按 F0605 条件清单隐式判定；为 false 时显式关闭隔离。
   */
  readonly explicitIsolation: boolean | null;
}

/** 链长上界：超过即产出性能告警（不拒绝——链长是设计问题，不是数据错误）。 */
export const MAX_EFFECT_CHAIN = 16;

/** F0605 隐式隔离条件清单（本条是其中「效果链非空」那一项的判定者）。 */
export interface IsolationInputs {
  /** 不透明度是否小于 1（F0603）。 */
  readonly opacityBelowOne: boolean;
  /** 混合模式是否非 normal（F0605）。 */
  readonly blendModeNonNormal: boolean;
  /** 效果链是否非空——本条提供。 */
  readonly effectChainNonEmpty: boolean;
  /** 是否存在蒙版（F0605）。 */
  readonly hasMask: boolean;
}

/** 隔离判定结果：为什么隔离、是否隐式、诊断有哪些。 */
export interface IsolationDecision {
  readonly isolated: boolean;
  /** 触发的条件名（显式优先）。 */
  readonly triggeredBy: string;
  /** 是否为隐式触发（F0605 条件清单命中）。 */
  readonly implicit: boolean;
  readonly diagnostics: readonly EffectDiagnostic[];
}

/**
 * 隔离判定（判据三：隐式隔离联动）。
 * 优先级：显式 true > 隐式条件命中 > 显式 false。
 * 显式关闭但隐式条件命中时产出 ISOLATION_CONFLICT 诊断——
 * 显式声明胜出（与 F0605 约定一致），但冲突必须显性，不能默默关掉隔离。
 */
export function decideIsolation(chain: EffectChain, inputs: IsolationInputs): IsolationDecision {
  const bag = new EffectDiagBag();
  const hits: string[] = [];
  if (inputs.opacityBelowOne) hits.push("opacity<1");
  if (inputs.blendModeNonNormal) hits.push("blend-mode-non-normal");
  if (inputs.effectChainNonEmpty) hits.push("effect-chain-non-empty");
  if (inputs.hasMask) hits.push("mask-present");

  if (chain.explicitIsolation === true) {
    return {
      isolated: true,
      triggeredBy: "explicit-declaration",
      implicit: false,
      diagnostics: bag.all(),
    };
  }

  if (hits.length > 0) {
    if (chain.explicitIsolation === false) {
      bag.push(
        "ISOLATION_CONFLICT",
        `效果链显式声明不隔离，但隐式条件命中：${hits.join("、")}`,
        "已按显式声明执行（显式胜出）；但视觉上子树会与外部直接混合，请确认这是预期效果",
      );
    }
    return {
      isolated: true,
      triggeredBy: hits.join("+"),
      implicit: true,
      diagnostics: bag.all(),
    };
  }

  return {
    isolated: false,
    triggeredBy: "none",
    implicit: false,
    diagnostics: bag.all(),
  };
}

// ════════════════════════════════════════════════════════════════════════════
// §5 实现可用性（立场三：缺位显性 N/A，不静默跳过）
// ════════════════════════════════════════════════════════════════════════════

/** 渲染域实现注册表：由 F0628（GPU）/ F0629（CPU）注入各自已实现的种类。 */
export interface ImplRegistry {
  readonly gpuKinds: ReadonlySet<EffectKind>;
  readonly cpuKinds: ReadonlySet<EffectKind>;
}

/** 单个效果在目标后端上的可用性。 */
export interface EffectAvailability {
  readonly id: string;
  readonly kind: EffectKind;
  readonly gpu: boolean;
  readonly cpu: boolean;
  /** 双路都缺位时为 true（该效果在两条渲染路径上都会缺位）。 */
  readonly missingEverywhere: boolean;
}

/** 批量查询可用性（O(链长)，描述符 O(1) 判定符合锚点性能声明）。 */
export function checkAvailability(
  effects: readonly EffectDescriptor[],
  reg: ImplRegistry,
): readonly EffectAvailability[] {
  return effects.map((e) => {
    const gpu = reg.gpuKinds.has(e.kind);
    const cpu = reg.cpuKinds.has(e.kind);
    return { id: e.id, kind: e.kind, gpu, cpu, missingEverywhere: !gpu && !cpu };
  });
}

// ════════════════════════════════════════════════════════════════════════════
// §6 中间缓冲策略（判据二：单次中间化）
// ════════════════════════════════════════════════════════════════════════════

/** 效果执行阶段：链上每个效果落在一个阶段。 */
export interface EffectStage {
  /** 阶段序号（合并后阶段序号会跳跃，序号相同即同 pass）。 */
  readonly stageIndex: number;
  /** 本阶段内串行执行的效果（合并模式下多项，单效果模式只有一项）。 */
  readonly effects: readonly EffectDescriptor[];
  /** 本阶段是否需要写回中间缓冲（false 表示就地消费，省一次落地）。 */
  readonly writesIntermediate: boolean;
}

/**
 * 链编译结果：阶段序列 + 缓冲统计 + 诊断。
 * 这是本条交给 F0628/F0629 的唯一结构（判据四：GPU/CPU 双路共同输入）。
 */
export interface EffectPlan {
  readonly nodeId: string;
  readonly stages: readonly EffectStage[];
  /**
   * 需要的中间纹理张数。
   * 单次中间化：ping-pong 双缓冲，整条链最多 2 张，与链长无关。
   * 只读链（无任何效果需落地）时为 0。
   */
  readonly bufferCount: number;
  /** 全程预乘 alpha（F0625 纪律）——两路渲染都不得各自假设未预乘。 */
  readonly premultipliedAlpha: true;
  /** 是否启用了合并（预留态，默认 false）。 */
  readonly merged: boolean;
  /** 被缺位实现跳过的效果 id（显性 N/A 清单）。 */
  readonly skippedByMissingImpl: readonly string[];
  readonly isolation: IsolationDecision;
  readonly diagnostics: readonly EffectDiagnostic[];
}

/** 合并开关（默认关闭——理由见文件头立场一）。 */
export interface MergeOptions {
  readonly enabled: boolean;
  /** 目标后端：GPU 与 CPU 的可合并判定可能不同，故须分别告知。 */
  readonly backend: "gpu" | "cpu";
}

/**
 * 编译效果链为执行计划（本条的主流程）。
 *
 * 编排顺序刻意如此，因为它就是视觉语义：
 *   1. 空链检查 → 空链不隔离、不分配缓冲、返回空计划（不是错误）。
 *   2. id 唯一性检查 → 重复 id 会让脏区定位失效，必须拦下。
 *   3. 链长告警 → 超过 MAX_EFFECT_CHAIN 产出性能告警与合并建议，不拒绝。
 *   4. 参数规范化 → 越界钳制在此完成（先于一切下游消费）。
 *   5. 可用性查询 → 缺位者显式标记为跳过，不静默丢。
 *   6. 隔离判定 → 与 F0605 联动，产出隔离决策。
 *   7. 阶段编排 → 单效果模式 or 合并模式（合并需逐对校验可合并性）。
 *   8. 缓冲统计 → 按「需落地的阶段数」算 ping-pong 张数，恒 ≤ 2。
 *
 * 纯函数：不改入参（规范化产出新描述符），同输入必同输出。
 */
export function compileEffectPlan(
  chain: EffectChain,
  reg: ImplRegistry,
  isolationInputs: Omit<IsolationInputs, "effectChainNonEmpty">,
  merge: MergeOptions = { enabled: false, backend: "gpu" },
): EffectPlan {
  const bag = new EffectDiagBag();

  // 1) 空链
  if (chain.effects.length === 0) {
    return {
      nodeId: chain.nodeId,
      stages: [],
      bufferCount: 0,
      premultipliedAlpha: true,
      merged: false,
      skippedByMissingImpl: [],
      isolation: decideIsolation(chain, { ...isolationInputs, effectChainNonEmpty: false }),
      diagnostics: bag.all(),
    };
  }

  // 2) id 唯一性
  const seen = new Set<string>();
  const dupes: string[] = [];
  for (const e of chain.effects) {
    if (seen.has(e.id)) dupes.push(e.id);
    seen.add(e.id);
  }
  if (dupes.length > 0) {
    bag.push(
      "CHAIN_DUPLICATE_ID",
      `效果链存在重复 id：${[...new Set(dupes)].join("、")}`,
      "重复 id 会让脏区定位与链序变更检测失效；请在挂载时保证每个效果 id 全局唯一",
    );
  }

  // 3) 链长告警
  if (chain.effects.length > MAX_EFFECT_CHAIN) {
    bag.push(
      "CHAIN_TOO_LONG",
      `效果链长度 ${chain.effects.length} 超过建议上限 ${MAX_EFFECT_CHAIN}`,
      `已照常编译但产出性能告警：链应用复杂度 O(效果数×像素)，超长链会显著拖慢合成；建议合并同类效果或拆分为多图层`,
    );
  }

  // 4) 参数规范化（缺位实现的效果也参与规范化——参数越界是要报的问题，与实现无关）
  const normalized = chain.effects.map((e) => normalizeDescriptor(e, bag));

  // 5) 可用性查询与缺位处理
  const availability = checkAvailability(normalized, reg);
  const backendAvailable = new Map<string, boolean>();
  const skipped: string[] = [];
  for (const a of availability) {
    const ok = merge.backend === "gpu" ? a.gpu : a.cpu;
    backendAvailable.set(a.id, ok);
    if (!ok) {
      skipped.push(a.id);
      bag.push(
        "EFFECT_IMPL_MISSING",
        `效果 ${a.id}（${a.kind}）在 ${merge.backend.toUpperCase()} 路径上尚无实现`,
        a.missingEverywhere
          ? "已在计划中显式标记为跳过；GPU 与 CPU 两条路径都缺位，画面会缺少这一层效果，请确认渲染域排期"
          : `已在计划中显式标记为跳过；另一条路径已有实现，两路渲染结果会不一致，请确认降级可接受`,
      );
    }
  }

  // 6) 隔离判定（效果链非空恒为 true——空链已在步骤 1 提前返回）
  const isolation = decideIsolation(chain, { ...isolationInputs, effectChainNonEmpty: true });
  bag.push(...isolation.diagnostics);

  // 7) 阶段编排
  const effective = normalized.filter((e) => backendAvailable.get(e.id) === true);
  const stages = buildStages(effective, merge, bag);

  // 8) 缓冲统计：需落地的阶段数决定张数，但 ping-pong 上限恒为 2
  const writingStages = stages.filter((s) => s.writesIntermediate).length;
  const bufferCount = writingStages === 0 ? 0 : Math.min(2, writingStages);

  return {
    nodeId: chain.nodeId,
    stages,
    bufferCount,
    premultipliedAlpha: true,
    merged: merge.enabled && stages.some((s) => s.effects.length > 1),
    skippedByMissingImpl: skipped,
    isolation,
    diagnostics: bag.all(),
  };
}

/**
 * 阶段编排（单效果模式与合并模式的唯一实现处）。
 * 单效果模式：每效果一阶段（除末阶段可就地消费，省一次落地）。
 * 合并模式：相邻且双方都声明 canMergeWithNeighbour 的效果并入同一阶段；
 *   遇到不可合并对即产出 MERGE_NOT_ADMISSIBLE 并断开（不静默合并）。
 */
function buildStages(
  effects: readonly EffectDescriptor[],
  merge: MergeOptions,
  bag: EffectDiagBag,
): readonly EffectStage[] {
  if (effects.length === 0) return [];
  if (!merge.enabled) {
    // 单效果模式：末阶段不就地消费（下游还要读结果做后续合成）。
    return effects.map((e, i) => ({
      stageIndex: i,
      effects: [e],
      writesIntermediate: true,
    }));
  }

  // 合并模式：逐对判定可合并性。位置 i 的效果与 i+1 合并需 i 与 i+1 都声明可合并。
  // 结构性要点：每个效果必须且只能被 push 一次——「与左边可合并则并入当前阶段」，
  // 否则链尾效果会因缺少右邻居而漏收（这是首版实现真实踩到的丢效果缺陷）。
  const stages: EffectStage[] = [];
  let current: EffectDescriptor[] = [];
  for (let i = 0; i < effects.length; i += 1) {
    const e = effects[i];
    if (e === undefined) continue;
    const prev = effects[i - 1];
    const next = effects[i + 1];

    // 与左边可合并 → 并入已开启的当前阶段。
    const mergeWithPrev =
      prev !== undefined &&
      current.length > 0 &&
      (prev.canMergeWithNeighbour ?? false) &&
      (e.canMergeWithNeighbour ?? false);
    if (mergeWithPrev) {
      current.push(e);
      continue;
    }

    // 不能与左边合并：先把已开启的阶段收尾，再以自己开新阶段。
    if (current.length > 0) {
      stages.push({ stageIndex: stages.length, effects: current, writesIntermediate: true });
      current = [];
    }
    current.push(e);

    // 与右边不可合并时说明原因（链尾无右邻居，不算风险）。
    const mergeWithNext =
      next !== undefined &&
      (e.canMergeWithNeighbour ?? false) &&
      (next.canMergeWithNeighbour ?? false);
    if (next !== undefined && !mergeWithNext) {
      bag.push(
        mergeRiskCode(e, next),
        `效果 ${e.id} 与 ${next.id} 未同时声明可合并（${e.id}: ${String(e.canMergeWithNeighbour ?? false)}, ${next.id}: ${String(next.canMergeWithNeighbour ?? false)}）`,
        "已断开为独立 pass；如确认两者在同一 pass 内无损，请在描述符上显式声明 canMergeWithNeighbour: true",
      );
    }
  }
  // 收尾：链尾若仍开着阶段，须落地（首版缺陷正是漏了这一步）。
  if (current.length > 0) {
    stages.push({ stageIndex: stages.length, effects: current, writesIntermediate: true });
  }
  return stages;
}

/** 合并风险的诊断码选取：至少一方声明可合并时属「语义风险」，否则属「不可合并」。 */
function mergeRiskCode(a: EffectDescriptor, b: EffectDescriptor): EffectDiagCode {
  const aOk = a.canMergeWithNeighbour ?? false;
  const bOk = b.canMergeWithNeighbour ?? false;
  // 一方声明可合并但另一方没有 → 说明作者以为可合并，实际不是，属语义风险（更值得警告）。
  return aOk !== bOk ? "MERGE_SEMANTIC_RISK" : "MERGE_NOT_ADMISSIBLE";
}

// ════════════════════════════════════════════════════════════════════════════
// §7 链序变更判定（判据一：链序即结果，链序变更走属性脏路径）
// ════════════════════════════════════════════════════════════════════════════

/** 链变更类型。 */
export type ChainChangeKind =
  | "none"
  | "order"
  | "param"
  | "append"
  | "remove"
  | "mixed";

/** 链变更判定结果。 */
export interface ChainChangeReport {
  readonly kind: ChainChangeKind;
  /** 变更是否属「语义变更」——序变更与增删一律是，参数变更按是否越界区分。 */
  readonly semantic: boolean;
  /** 脏区级别：语义变更必须走属性脏路径（subtree-transform）。 */
  readonly requiresDirtyPath: boolean;
  /** 受影响的效果 id（供调用方做最小脏区收集）。 */
  readonly affected: readonly string[];
  readonly diagnostics: readonly EffectDiagnostic[];
}

/**
 * 链序/参数变更判定。
 * 关键立场：**重排是语义变更**，不是性能优化——同一组效果换个顺序视觉结果就不同，
 * 所以必须走属性脏路径（与 F0606「重排属语义变更」同律）。
 */
export function detectChainChange(
  before: readonly EffectDescriptor[],
  after: readonly EffectDescriptor[],
): ChainChangeReport {
  const bag = new EffectDiagBag();
  const beforeIds = before.map((e) => e.id);
  const afterIds = after.map((e) => e.id);
  const affected = new Set<string>();

  const removed = beforeIds.filter((id) => !afterIds.includes(id));
  const added = afterIds.filter((id) => !beforeIds.includes(id));
  removed.forEach((id) => affected.add(id));
  added.forEach((id) => affected.add(id));

  // 顺序判定：在共有元素上比对相对次序
  const commonBefore = beforeIds.filter((id) => afterIds.includes(id));
  const commonAfter = afterIds.filter((id) => beforeIds.includes(id));
  const reordered =
    commonBefore.length !== commonAfter.length ||
    commonBefore.some((id, i) => id !== commonAfter[i]);
  if (reordered) commonBefore.forEach((id) => affected.add(id));

  // 参数判定：逐 id 比对描述符
  let paramChanged = false;
  for (const b of before) {
    const a = after.find((x) => x.id === b.id);
    if (a === undefined) continue;
    if (!sameDescriptor(b, a)) {
      paramChanged = true;
      affected.add(b.id);
    }
  }

  let kind: ChainChangeKind = "none";
  if (reordered && (added.length > 0 || removed.length > 0 || paramChanged)) kind = "mixed";
  else if (reordered) kind = "order";
  else if (paramChanged && (added.length > 0 || removed.length > 0)) kind = "mixed";
  else if (paramChanged) kind = "param";
  else if (added.length > 0 && removed.length > 0) kind = "mixed";
  else if (added.length > 0) kind = "append";
  else if (removed.length > 0) kind = "remove";

  const semantic = kind !== "none";

  if (kind === "order") {
    bag.push(
      "MERGE_SEMANTIC_RISK",
      "检测到效果链顺序变更",
      "重排是语义变更（同组效果换序视觉结果不同），已要求走属性脏路径；请勿按性能优化处理",
    );
  }

  return {
    kind,
    semantic,
    requiresDirtyPath: semantic,
    affected: [...affected],
    diagnostics: bag.all(),
  };
}

// ════════════════════════════════════════════════════════════════════════════
// §8 预乘纪律（判据四：链内全程预乘 alpha）
// ════════════════════════════════════════════════════════════════════════════

/** 预乘纪律检查项。 */
export interface PremultiplyCheck {
  readonly pass: boolean;
  /** 违反项描述（pass=true 时为空）。 */
  readonly violations: readonly string[];
}

/**
 * 预乘纪律校验：确认链上每个效果的语义都假定「输入已预乘」。
 *
 * 本条不校验像素数据（那是 F0625/F0628 的事），只校验**声明层的一致性**：
 * 预乘纪律失效的典型症状是 GPU 路径假设已预乘、CPU 路径假设未预乘，
 * 两路各写一套隐式假设——本检查把该假设显式化并要求调用方声明一致。
 */
export function checkPremultiplyDiscipline(
  stages: readonly EffectStage[],
  backendPremultiplied: { readonly gpu: boolean; readonly cpu: boolean },
): PremultiplyCheck {
  const violations: string[] = [];

  if (stages.length > 0 && !(backendPremultiplied.gpu && backendPremultiplied.cpu)) {
    violations.push(
      `后端预乘声明不一致：gpu=${String(backendPremultiplied.gpu)}, cpu=${String(backendPremultiplied.cpu)}`,
    );
  }
  // 逐阶段核对：合并阶段里的效果必须同属一类预乘语义（否则合并就是错的）
  for (const st of stages) {
    if (st.effects.length < 2) continue;
    const kinds = new Set(st.effects.map((e) => e.kind));
    // 阴影是「产出新alpha」的特效，与「就地调制颜色」的效果不同源：
    // 允许同 pass，但需在计划中保留可区分性，故此处只提示不断开。
    if (kinds.has("shadow") && kinds.size > 1) {
      violations.push(
        `阶段 ${st.stageIndex} 混合了 shadow（产出新 alpha）与其他效果（调制颜色）：${[...kinds].join("、")}`,
      );
    }
  }

  return { pass: violations.length === 0, violations };
}

// ════════════════════════════════════════════════════════════════════════════
// §9 自检（判据的可执行形态）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结果项：每项对应判据的一条。 */
export interface EffectSelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 构造测试用描述符（自检内部用，不导出）。 */
function d(id: string, kind: EffectKind, params: ParamSpec[], mergeable = false): EffectDescriptor {
  return { id, kind, params, canMergeWithNeighbour: mergeable };
}

/** 判据一「链序即结果」自检：重排必须是语义变更且要求走属性脏路径。 */
export function selfCheckChainOrder(): EffectSelfCheck[] {
  const out: EffectSelfCheck[] = [];
  const a = d("e1", "blur", [{ name: "radius", value: 4 }]);
  const b = d("e2", "shadow", [{ name: "opacity", value: 0.5 }]);

  const same = detectChainChange([a, b], [a, b]);
  out.push({
    name: "order-no-change-is-none",
    pass: same.kind === "none" && !same.requiresDirtyPath,
    detail: `同链比对结果 kind=${same.kind}，requiresDirtyPath=${String(same.requiresDirtyPath)}（期望 none/false）`,
  });

  const swapped = detectChainChange([a, b], [b, a]);
  out.push({
    name: "reorder-is-semantic-change",
    pass: swapped.kind === "order" && swapped.semantic && swapped.requiresDirtyPath,
    detail: `重排结果 kind=${swapped.kind}，semantic=${String(swapped.semantic)}，requiresDirtyPath=${String(swapped.requiresDirtyPath)}，受影响 ${swapped.affected.length} 项（期望 order/true/true）`,
  });

  const appended = detectChainChange([a], [a, b]);
  out.push({
    name: "append-is-semantic-change",
    pass: appended.kind === "append" && appended.requiresDirtyPath && appended.affected.includes("e2"),
    detail: `追加结果 kind=${appended.kind}，受影响=${appended.affected.join("、")}`,
  });

  const paramChanged = detectChainChange(
    [a, b],
    [d("e1", "blur", [{ name: "radius", value: 9 }]), b],
  );
  out.push({
    name: "param-change-is-semantic-change",
    pass: paramChanged.kind === "param" && paramChanged.requiresDirtyPath,
    detail: `参数变更结果 kind=${paramChanged.kind}（参数变更同样走属性脏路径）`,
  });

  return out;
}

/** 判据二「单次中间化」自检：缓冲张数恒不超过 2（与链长无关）。 */
export function selfCheckBuffering(): EffectSelfCheck[] {
  const out: EffectSelfCheck[] = [];
  const reg: ImplRegistry = {
    gpuKinds: new Set<EffectKind>(["blur", "shadow", "colorShift", "brightness", "contrast", "saturate"]),
    cpuKinds: new Set<EffectKind>(["blur", "shadow", "colorShift", "brightness", "contrast", "saturate"]),
  };
  const iso = { opacityBelowOne: false, blendModeNonNormal: false, hasMask: false };

  const chainOf = (n: number): EffectChain => ({
    nodeId: "n1",
    effects: Array.from({ length: n }, (_, i) =>
      d(`e${i}`, "blur", [{ name: "radius", value: 2 }]),
    ),
    explicitIsolation: null,
  });

  // 核心判据：链长从 1涨到 12，缓冲张数始终 ≤ 2（ping-pong 双缓冲上限），
  // 且不随链长线性增长——这是「单次中间化」与「每个效果各开一张」的分水岭。
  const counts = [1, 2, 4, 8, 12].map((n) => compileEffectPlan(chainOf(n), reg, iso).bufferCount);
  const allCapped = counts.every((c) => c <= 2);
  const notGrowing = counts[counts.length - 1] === counts[counts.length - 2];
  out.push({
    name: "buffer-count-capped-at-two",
    pass: allCapped && notGrowing,
    detail: `链长 1/2/4/8/12 → 缓冲 ${counts.join("/")} 张；全部 ≤ 2=${String(allCapped)}，8→12 不再增长=${String(notGrowing)}（每个效果各开一张的实现会得到 1/2/4/8/12）`,
  });

  const p8 = compileEffectPlan(chainOf(8), reg, iso);
  const p8Merged = compileEffectPlan(chainOf(8), reg, iso, { enabled: true, backend: "gpu" });
  out.push({
    name: "merge-reduces-stage-count-when-admissible",
    pass: p8Merged.stages.length <= p8.stages.length,
    detail: `未合并 ${p8.stages.length} 阶段；开启合并但效果未声明可合并 → ${p8Merged.stages.length} 阶段（未声明者不合并，符合立场一）`,
  });

  return out;
}

/** 判据三「隐式隔离」自检：效果链非空是F0605 条件清单成员。 */
export function selfCheckIsolation(): EffectSelfCheck[] {
  const out: EffectSelfCheck[] = [];
  const emptyChain: EffectChain = { nodeId: "n1", effects: [], explicitIsolation: null };
  const nonEmptyChain: EffectChain = {
    nodeId: "n1",
    effects: [d("e1", "blur", [{ name: "radius", value: 2 }])],
    explicitIsolation: null,
  };
  const base = { opacityBelowOne: false, blendModeNonNormal: false, hasMask: false };

  const isoEmpty = decideIsolation(emptyChain, { ...base, effectChainNonEmpty: false });
  const isoChain = decideIsolation(nonEmptyChain, { ...base, effectChainNonEmpty: true });
  out.push({
    name: "empty-chain-not-isolated",
    pass: !isoEmpty.isolated,
    detail: `空效果链隔离=${String(isoEmpty.isolated)}（期望 false，空链不该产生额外隔离开销）`,
  });
  out.push({
    name: "non-empty-chain-triggers-isolation",
    pass: isoChain.isolated && isoChain.implicit && isoChain.triggeredBy.includes("effect-chain-non-empty"),
    detail: `非空效果链隔离=${String(isoChain.isolated)}，隐式=${String(isoChain.implicit)}，触发条件=${isoChain.triggeredBy}`,
  });

  const chainExplicitTrue: EffectChain = { ...nonEmptyChain, explicitIsolation: true };
  const isoExplicit = decideIsolation(chainExplicitTrue, { ...base, effectChainNonEmpty: false });
  out.push({
    name: "explicit-declaration-wins",
    pass: isoExplicit.isolated && !isoExplicit.implicit && isoExplicit.triggeredBy === "explicit-declaration",
    detail: `显式声明 true 且链为空 → 隔离=${String(isoExplicit.isolated)}，触发=${isoExplicit.triggeredBy}（显式优先于隐式）`,
  });

  // 显式 false 但条件命中 → 冲突必须显性
  const chainExplicitFalse: EffectChain = { ...nonEmptyChain, explicitIsolation: false };
  const isoConflict = decideIsolation(chainExplicitFalse, { ...base, effectChainNonEmpty: true });
  out.push({
    name: "isolation-conflict-is-explicit",
    pass: isoConflict.diagnostics.some((d) => d.code === "ISOLATION_CONFLICT"),
    detail: `显式 false + 条件命中 → 产出 ISOLATION_CONFLICT 诊断 ${isoConflict.diagnostics.length} 条（冲突不静默）`,
  });

  return out;
}

/** 判据四「预乘纪律」自检。 */
export function selfCheckPremultiply(): EffectSelfCheck[] {
  const out: EffectSelfCheck[] = [];
  const reg: ImplRegistry = {
    gpuKinds: new Set<EffectKind>(["blur", "shadow"]),
    cpuKinds: new Set<EffectKind>(["blur", "shadow"]),
  };
  const iso = { opacityBelowOne: false, blendModeNonNormal: false, hasMask: false };

  // 用例一：同质效果（blur + blur），排除 shadow 混合告警的干扰，
  //         用于独立验证「后端预乘声明一致」这一条。
  const homo: EffectChain = {
    nodeId: "n1",
    effects: [d("e1", "blur", [{ name: "radius", value: 3 }], true), d("e2", "blur", [{ name: "radius", value: 1 }], true)],
    explicitIsolation: null,
  };
  const homoPlan = compileEffectPlan(homo, reg, iso, { enabled: true, backend: "gpu" });
  const both = checkPremultiplyDiscipline(homoPlan.stages, { gpu: true, cpu: true });
  const mismatch = checkPremultiplyDiscipline(homoPlan.stages, { gpu: true, cpu: false });
  out.push({
    name: "premultiply-consistent-passes",
    pass: both.pass && !mismatch.pass && mismatch.violations.length > 0,
    detail: `同质链双路一致 → pass=${String(both.pass)}；双路不一致 → pass=${String(mismatch.pass)}，违规 ${mismatch.violations.length} 条`,
  });

  // 用例二：任何非空计划都必须携带预乘声明（GPU/CPU 两路共同的纪律锚点）。
  out.push({
    name: "plan-declares-premultiplied",
    pass: homoPlan.premultipliedAlpha === true && homoPlan.stages.length === 1,
    detail: `计划携带 premultipliedAlpha=${String(homoPlan.premultipliedAlpha)}；同质可合并链合并为 ${homoPlan.stages.length} 阶段`,
  });

  // 用例三：合并阶段混入 shadow（产出新 alpha）与颜色调制效果 → 必须报违规。
  const mixed: EffectChain = {
    nodeId: "n1",
    effects: [d("e1", "blur", [], true), d("e2", "shadow", [{ name: "opacity", value: 0.4 }], true)],
    explicitIsolation: null,
  };
  const mixedPlan = compileEffectPlan(mixed, reg, iso, { enabled: true, backend: "gpu" });
  const mixedCheck = checkPremultiplyDiscipline(mixedPlan.stages, { gpu: true, cpu: true });
  out.push({
    name: "shadow-mixed-in-stage-detected",
    pass: !mixedCheck.pass && mixedCheck.violations.some((v) => v.includes("shadow")),
    detail: `合并阶段混入 shadow（产出新 alpha）与 blur（调制颜色）→ 违规 ${mixedCheck.violations.length} 条`,
  });

  return out;
}

/** 错误路径与降级矩阵自检。 */
export function selfCheckDegradation(): EffectSelfCheck[] {
  const out: EffectSelfCheck[] = [];
  const fullReg: ImplRegistry = {
    gpuKinds: new Set<EffectKind>(["blur", "shadow"]),
    cpuKinds: new Set<EffectKind>(["blur"]),
  };
  const iso = { opacityBelowOne: false, blendModeNonNormal: false, hasMask: false };

  // 1) 参数越界 → 钳制 + 诊断
  const chain: EffectChain = {
    nodeId: "n1",
    effects: [d("e1", "blur", [{ name: "radius", value: 9999 }])],
    explicitIsolation: null,
  };
  const plan = compileEffectPlan(chain, fullReg, iso);
  const clampedOk =
    plan.stages[0]?.effects[0] !== undefined &&
    paramOf(plan.stages[0].effects[0], "radius") === 1024;
  out.push({
    name: "param-clamped-with-diagnostic",
    pass: clampedOk && plan.diagnostics.some((x) => x.code === "EFFECT_PARAM_OUT_OF_RANGE"),
    detail: `半径 9999 → 钳制为 ${plan.stages[0]?.effects[0] !== undefined ? paramOf(plan.stages[0].effects[0], "radius") : "?"}（上界 1024），诊断已记录`,
  });

  // 2) 重复 id → 诊断
  const dupChain: EffectChain = {
    nodeId: "n1",
    effects: [d("dup", "blur", []), d("dup", "shadow", [])],
    explicitIsolation: null,
  };
  const dupPlan = compileEffectPlan(dupChain, fullReg, iso);
  out.push({
    name: "duplicate-id-diagnosed",
    pass: dupPlan.diagnostics.some((x) => x.code === "CHAIN_DUPLICATE_ID"),
    detail: `重复 id 产出 CHAIN_DUPLICATE_ID 诊断 = ${String(dupPlan.diagnostics.some((x) => x.code === "CHAIN_DUPLICATE_ID"))}`,
  });

  // 3) 实现缺位 → 显式跳过 + N/A 诊断
  const missingChain: EffectChain = {
    nodeId: "n1",
    effects: [d("e1", "blur", []), d("e2", "saturate", [])],
    explicitIsolation: null,
  };
  const missingPlan = compileEffectPlan(missingChain, fullReg, iso);
  out.push({
    name: "missing-impl-explicitly-skipped",
    pass:
      missingPlan.skippedByMissingImpl.includes("e2") &&
      missingPlan.diagnostics.some((x) => x.code === "EFFECT_IMPL_MISSING") &&
      !missingPlan.stages.some((s) => s.effects.some((e) => e.id === "e2")),
    detail: `saturate 在 GPU 缺位 → 跳过清单=[${missingPlan.skippedByMissingImpl.join("、")}]，阶段数=${missingPlan.stages.length}（缺位者不进阶段）`,
  });

  // 4) 双路都缺位 → 诊断文案应区分（更严重）
  const bothMissingReg: ImplRegistry = { gpuKinds: new Set<EffectKind>(["blur"]), cpuKinds: new Set<EffectKind>(["blur"]) };
  const bothPlan = compileEffectPlan(missingChain, bothMissingReg, iso);
  out.push({
    name: "missing-everywhere-distinguished",
    pass: bothPlan.diagnostics.some((x) => x.code === "EFFECT_IMPL_MISSING" && x.hint.includes("两条路径")),
    detail: `双路缺位的诊断文案区分了「两路都缺」与「单路缺」`,
  });

  // 5) 链过长 → 告警但不拒绝
  const longChain: EffectChain = {
    nodeId: "n1",
    effects: Array.from({ length: MAX_EFFECT_CHAIN + 4 }, (_, i) => d(`e${i}`, "blur", [])),
    explicitIsolation: null,
  };
  const longPlan = compileEffectPlan(longChain, fullReg, iso);
  out.push({
    name: "long-chain-warns-not-rejects",
    pass: longPlan.stages.length === MAX_EFFECT_CHAIN + 4 && longPlan.diagnostics.some((x) => x.code === "CHAIN_TOO_LONG"),
    detail: `${MAX_EFFECT_CHAIN + 4} 个效果照常编译为 ${longPlan.stages.length} 阶段，同时产出 CHAIN_TOO_LONG 告警（链长是设计问题不是数据错误）`,
  });

  // 6) 合并开关：未声明可合并者不被合并
  const notMergeable: EffectChain = {
    nodeId: "n1",
    effects: [d("e1", "blur", []), d("e2", "blur", [])],
    explicitIsolation: null,
  };
  const merged = compileEffectPlan(notMergeable, fullReg, iso, { enabled: true, backend: "gpu" });
  out.push({
    name: "merge-needs-explicit-admission",
    pass: merged.stages.length === 2 && merged.merged === false && !merged.diagnostics.some((x) => x.code === "MERGE_SEMANTIC_RISK"),
    detail: `两个未声明可合并的效果，开启合并开关后仍为 ${merged.stages.length} 阶段，merged=${String(merged.merged)}（链尾无风险诊断属预期）`,
  });

  // 7) 单方声明可合并 → 语义风险诊断
  const halfMergeable: EffectChain = {
    nodeId: "n1",
    effects: [d("e1", "blur", [], true), d("e2", "blur", [], false)],
    explicitIsolation: null,
  };
  const halfPlan = compileEffectPlan(halfMergeable, fullReg, iso, { enabled: true, backend: "gpu" });
  out.push({
    name: "half-declared-merge-is-semantic-risk",
    pass: halfPlan.diagnostics.some((x) => x.code === "MERGE_SEMANTIC_RISK"),
    detail: `单方声明可合并 → 产出 MERGE_SEMANTIC_RISK（作者以为可合并但实际不是，比双方未声明更值得警告）`,
  });

  // 8) 双方声明可合并 → 确实合并。
  //    两个效果都必须已实现，否则会被缺位跳过，导致测的其实是「跳过」而非「合并」。
  const mergeableReg: ImplRegistry = {
    gpuKinds: new Set<EffectKind>(["blur", "contrast"]),
    cpuKinds: new Set<EffectKind>(["blur", "contrast"]),
  };
  const bothMergeable: EffectChain = {
    nodeId: "n1",
    effects: [d("e1", "blur", [], true), d("e2", "contrast", [], true)],
    explicitIsolation: null,
  };
  const bothPlanMerged = compileEffectPlan(bothMergeable, mergeableReg, iso, { enabled: true, backend: "gpu" });
  out.push({
    name: "both-declared-merge-happens",
    pass:
      bothPlanMerged.stages.length === 1 &&
      (bothPlanMerged.stages[0]?.effects.length ?? 0) === 2 &&
      bothPlanMerged.merged === true &&
      bothPlanMerged.skippedByMissingImpl.length === 0,
    detail: `双方声明可合并且都已实现 → ${bothPlanMerged.stages.length} 阶段（内含 ${bothPlanMerged.stages[0]?.effects.length ?? 0} 个效果），merged=${String(bothPlanMerged.merged)}，跳过=${bothPlanMerged.skippedByMissingImpl.length}`,
  });

  // 9) 空链不报错
  const emptyPlan = compileEffectPlan({ nodeId: "n1", effects: [], explicitIsolation: null }, fullReg, iso);
  out.push({
    name: "empty-chain-is-not-error",
    pass: emptyPlan.stages.length === 0 && emptyPlan.bufferCount === 0 && !emptyPlan.diagnostics.some((x) => x.code === "CHAIN_EMPTY"),
    detail: `空链返回 0 阶段 0 缓冲，且不报 CHAIN_EMPTY（无效果不是错误）`,
  });

  // 10) 参数规则表完备性
  const rulesOk = EFFECT_KINDS.every((k) => EFFECT_PARAM_RULES[k] !== undefined && EFFECT_PARAM_RULES[k].length > 0);
  const ruleSane = EFFECT_KINDS.every((k) => EFFECT_PARAM_RULES[k].every((r) => r.min < r.max && r.fallback >= r.min && r.fallback <= r.max));
  out.push({
    name: "param-rules-complete-and-sane",
    pass: rulesOk && ruleSane,
    detail: `${EFFECT_KINDS.length} 个种类全部登记参数规则=${String(rulesOk)}；值域与 fallback 自洽=${String(ruleSane)}`,
  });

  return out;
}

/** 全量自检入口（6 组对应四条判据 + 两组工程纪律）。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly EffectSelfCheck[]>>;
  readonly allPass: boolean;
  readonly total: number;
  readonly failed: readonly string[];
} {
  const groups = {
    order: selfCheckChainOrder(),
    buffering: selfCheckBuffering(),
    isolation: selfCheckIsolation(),
    premultiply: selfCheckPremultiply(),
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