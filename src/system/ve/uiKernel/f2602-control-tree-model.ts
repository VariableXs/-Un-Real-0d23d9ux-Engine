/**
 * VE-F2602 · 控件树模型（N 域 · 三大件之首· 控件树/逻辑树）
 * ---------------------------------------------------------------------------
 * 职责定位：VE-N 域（F2601~F2800）三大架构件中「控件树」的实现条目。控件树是
 * N 域全部后续组的地基：布局消费树的遍历接口、命中消费布局矩形、动画挂载属性、
 * 虚拟化消费剪裁边界、增量更新消费结构变更。若树模型的契约面不先定死，
 * N02~N08 会各自拿到不同版本的接口猜测，返工成本远高于现在写清楚。
 *
 * 本条交付五件事，全部写成可机检的契约：
 *   1. 控件节点模型 —— 四要素：属性 / 子节点 / 事件 / 状态；
 *   2. 树三不变量 —— 单亲 / 无环 / 序稳定，逐条带实现纪律与断言；
 *   3. 三树操作 —— insert / remove / reorder，批量操作=单事务；
 *   4. M04 绑定 —— F2402 bind_path 的树侧路径解析器（销毁须显性告警）；
 *   5. 边界防护 —— 深度上限拒绝（fuzz 面 F2609 联动）、环检测、事务回滚。
 *
 * 为什么四要素要拆成四件而不是一个「节点对象」（最容易被简化的地方）：
 *   控件树是 N 域里**唯一被所有其他组直接持有**的结构。属性值语义归属性引擎
 *   （F2604）、失效标记归增量更新（F2606）、绘制归D 域——这四件事的归属在
 *   F2601 已写死。若节点对象顺手把属性值也存一份（常见诱惑：「反正树里也能存，
 *   省得查一次」），就会出现两份属性值：树里那份永远不更新，动画读它于是动画不动，
 *   而属性引擎那边一切正常。这类缺陷的特征是「读错值但没有任何报错」。
 *   故本条把「节点只持引用、不持属性值副本」写成显式纪律并由断言机检。
 *
 * 三不变量为何是树结构的根基（不是可选的健壮性检查）：
 *   单亲——保证「一个控件只有一个位置」，否则命中测试与焦点遍历会重复访问，
 *         同一控件可能被点击两次，且这种重复只在特定树形下出现，极难复现；
 *   无环——环一旦形成，遍历就不再终止。症状不是报错而是**界面卡死**：
 *         布局遍历环 → 每帧无限递归 → 主线程占满 → 界面无响应但进程活着。
 *         恶意超深树（F2609 fuzz 面）是同一类攻击的规模化形态。
 *   序稳定——兄弟序决定绘制序、命中优先级与 Tab 焦点序。序不稳定时，
 *         「Tab 跳到下一个控件」会在两次按键间落到不同控件上（用户视角：焦点乱跳），
 *         且因为每次重建树序都不同，问题表现为「偶发」，几乎无法归因。
 *
 * 零静默纪律：单亲冲突、环创建、深度超限、批量中途失败、绑定路径失效，
 * 全部产出 Diagnostic（code + message + hint）并由调用方聚合上报。
 * 本模块不抛异常、不吞诊断、无静默分支、无全局可变状态、零 DOM 依赖。
 *
 * 判据：四要素模型、三不变量、原子操作、M04 绑定、判据。
 * 交接说明：纯契约层 + 纯数据结构操作，可在浏览器 / Worker / Node 校验脚本中
 *         原样引入。下游 F2604（属性引擎挂载）、F2605（逻辑树可视树分离）、
 *         F2606（增量更新）、F2610（万节点基准）逐项消费本条的节点模型与三操作。
 */

import {
  DiagBag,
  type Diagnostic,
  type DiagCode,
  type Outcome,
} from "./f2601-n-ui-kernel-architecture";

export { DiagBag };
export type { Diagnostic, DiagCode, Outcome };

// ════════════════════════════════════════════════════════════════════════════
// §1 本条专属诊断码（在 F2601 域级诊断码之外新增，不改上游类型）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 控件树诊断码：域级 DiagCode 的**超集**（继承全部域级码 + 本条树专属码）。
 *
 * 为什么不新立一个与 DiagCode 平行的类型（这是本条最容易走错的一步）：
 *   Diagnostic.code 的类型是域级 DiagCode。若树码自成一套平行联合，上报通道就得
 *   为树单独开一个类型，各处`bag.push(树码)` 全部编译不过；开发者最自然的反应是
 *   写 `as Diagnostic` / `as unknown as Diagnostic` 强转绕过——那行强转在编译期
 *   静默通过，运行时若码值真的进了上报通道，下游按 DiagCode 分派的 switch 会落到
 *   default 分支被丢弃：诊断「发出去了」但没有任何人处理，且全程零报错。
 *   症状是「明明打了日志却查不到」，归因成本极高。
 *   取超集则树码天然可传给 DiagBag.push，无需任何强转，编译期即锁死。
 */
export type TreeDiagCode =
  /** 继承域级码：树操作可能触发域级协议/边界类诊断，码表统一不分裂。 */
  | DiagCode
  /** 单亲冲突：待插入节点已有父。 */
  | "SINGLE_PARENT_VIOLATION"
  /** 无环破坏：插入将形成祖先环。 */
  | "ANCESTOR_CYCLE"
  /** 序稳定破坏：索引越界或重排位置非法。 */
  | "ORDER_STABILITY_VIOLATION"
  /** 深度超限：插入后子树深度超过上限（fuzz 面 F2609）。 */
  | "DEPTH_LIMIT_EXCEEDED"
  /** 节点不存在（操作目标未在树中）。 */
  | "NODE_NOT_FOUND"
  /** 父节点不存在或非容器。 */
  | "PARENT_INVALID"
  /** 节点自插入：把节点插入到自己的子树里（环的特例）。 */
  | "SELF_INSERTION"
  /** 批量事务中途失败，已回滚（须显性报出失败步与回滚事实）。 */
  | "BATCH_ROLLED_BACK"
  /** 批量操作违反单事务纪律（部分步骤未经回滚即生效）。 */
  | "BATCH_NOT_ATOMIC"
  /** 绑定路径失效（轨道失效，须显性告警并指名路径）。 */
  | "BINDING_PATH_INVALID"
  /** 绑定目标节点已销毁（须显性告警，不得静默空转）。 */
  | "BINDING_TARGET_DESTROYED"
  /** 状态值不在状态机枚举内。 */
  | "STATE_INVALID"
  /** 事件类型不在封闭集内。 */
  | "EVENT_TYPE_UNKNOWN"
  /** 属性键类型非法（未声明的属性写入）。 */
  | "PROPERTY_KEY_INVALID"
  /** 树未初始化或已销毁（调用序错误）。 */
  | "TREE_LIFECYCLE_VIOLATION"
  /** 自检审计不通过（三不变量或四要素机检失败）。 */
  | "TREE_SELFCHECK_FAILED";

/** 本条自有的树诊断码（不含继承的域级码，用于映射表完备性机检）。 */
export type TreeOnlyDiagCode = Exclude<TreeDiagCode, DiagCode>;

/**
 * 树码 → 域级码的归属映射（穷尽表）。
 *
 * 背景：域级 Diagnostic.code 是封闭联合，树码是其超集，无法直接塞进域级结构。
 * 本条不选「用 as 强转糊过去」——强转会把树码伪装成域级码流进上报通道，
 * 下游按域级码分派的 switch 落到 default 丢弃，表现为「日志里有、查不到」。
 * 本条也不选「树自建一套独立上报结构」——那会让 N02~N08 每组各写一次桥，口径必然不一。
 * 故取第三条路：显式登记每个树码的域级归属，桥接唯一、口径可审、未登记者机检拦截。
 *
 * 归属按「处置动作」而非「症状」划分，因为下游按码分派时关心的正是「该怎么办」：
 *   违反三不变量 → 归 PILLAR_SPEC_INCONSISTENT（树是三大件之首，不变量破了要回改规格）；
 *   非法输入 / 调用序错 → 归 THEME_SPEC_INCONSISTENT（调用方用法错，改调用方）；
 *   越限攻击 → 归 CAPABILITY_UNCLAIMED（深度上限这类防护无属主能力，须显性上报）。
 */
export const TREE_CODE_OWNERSHIP: Readonly<Record<TreeOnlyDiagCode, DiagCode>> = {
  SINGLE_PARENT_VIOLATION: "PILLAR_SPEC_INCONSISTENT",
  ANCESTOR_CYCLE: "PILLAR_SPEC_INCONSISTENT",
  ORDER_STABILITY_VIOLATION: "PILLAR_SPEC_INCONSISTENT",
  DEPTH_LIMIT_EXCEEDED: "CAPABILITY_UNCLAIMED",
  NODE_NOT_FOUND: "THEME_SPEC_INCONSISTENT",
  PARENT_INVALID: "THEME_SPEC_INCONSISTENT",
  SELF_INSERTION: "THEME_SPEC_INCONSISTENT",
  BATCH_ROLLED_BACK: "THEME_SPEC_INCONSISTENT",
  BATCH_NOT_ATOMIC: "PILLAR_SPEC_INCONSISTENT",
  BINDING_PATH_INVALID: "PROTOCOL_SCOPE_MISMATCH",
  BINDING_TARGET_DESTROYED: "PROTOCOL_SCOPE_MISMATCH",
  STATE_INVALID: "THEME_SPEC_INCONSISTENT",
  EVENT_TYPE_UNKNOWN: "THEME_SPEC_INCONSISTENT",
  PROPERTY_KEY_INVALID: "THEME_SPEC_INCONSISTENT",
  TREE_LIFECYCLE_VIOLATION: "THEME_SPEC_INCONSISTENT",
  TREE_SELFCHECK_FAILED: "PILLAR_SPEC_INCONSISTENT",
};

/** 树码全集（映射表完备性机检的事实源）。 */
export const TREE_DIAG_CODES: readonly TreeOnlyDiagCode[] = [
  "SINGLE_PARENT_VIOLATION",
  "ANCESTOR_CYCLE",
  "ORDER_STABILITY_VIOLATION",
  "DEPTH_LIMIT_EXCEEDED",
  "NODE_NOT_FOUND",
  "PARENT_INVALID",
  "SELF_INSERTION",
  "BATCH_ROLLED_BACK",
  "BATCH_NOT_ATOMIC",
  "BINDING_PATH_INVALID",
  "BINDING_TARGET_DESTROYED",
  "STATE_INVALID",
  "EVENT_TYPE_UNKNOWN",
  "PROPERTY_KEY_INVALID",
  "TREE_LIFECYCLE_VIOLATION",
  "TREE_SELFCHECK_FAILED",
];

/**
 * 树诊断结构：code 为树码；上报时经 toDomainDiagnostic 桥接到域级通道。
 *
 * `at` 字段是本条自加的：域级 Diagnostic 只有三要素，而树操作的排障第一问是
 * 「哪个节点 / 哪条路径触发的」。若不带 at，一棵万节点树报出 3 条ORDER_STABILITY_VIOLATION
 * 时，开发者无法知道是哪三处，只能全树搜索——而树操作的失败往往与节点 id 直接相关。
 */
export interface TreeDiagnostic {
  readonly code: TreeDiagCode;
  /** 人话描述：面向开发者排障，不含裸码、不含「可能」「也许」。 */
  readonly message: string;
  /** 可操作提示：调用方该改哪里、该怎么降级。 */
  readonly hint: string;
  /** 触发点：节点 id / 操作名 / 绑定路径，便于按节点聚合排障。 */
  readonly at?: string;
}

/**
 * 桥接树诊断到域级诊断（唯一的口径转换入口）。
 *
 * 归属缺失时以 TREE_SELFCHECK_FAILED 兜底并在 message 里点名缺失的树码——
 * 绝不静默丢弃：桥接失败本身就是需要上报的事实。
 */
export function toDomainDiagnostic(d: TreeDiagnostic): Diagnostic {
  const owner = TREE_CODE_OWNERSHIP[d.code as TreeOnlyDiagCode];
  if (owner === undefined) {
    // 兜底用域级码 PILLAR_SPEC_INCONSISTENT（树码不能写进域级码表，见 TREE_CODE_OWNERSHIP 注释）。
    // 语义亦对：映射表不完备 = 树的三件套规格不完整，正是该码所指。
    return {
      code: "PILLAR_SPEC_INCONSISTENT",
      message: `树诊断码 ${d.code} 未登记域级归属，桥接降级：${d.message}`,
      hint: "在 TREE_CODE_OWNERSHIP 与 TREE_DIAG_CODES 中补齐该码的归属后重跑自检",
    };
  }
  return { code: owner, message: `[${d.code}] ${d.message}`, hint: d.hint };
}

/** 树结果判别联合：与域级 Outcome 同构（成功带 value，失败带三要素），code 为树码。 */
export type TreeOutcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly TreeDiagnostic[] }
  | {
      readonly ok: false;
      readonly code: TreeDiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly TreeDiagnostic[];
    };

/** 构造树诊断。 */
function td(code: TreeDiagCode, message: string, hint: string, at?: string): TreeDiagnostic {
  return at === undefined ? { code, message, hint } : { code, message, hint, at };
}

/** 失败构造（三要素齐备，失败不可被误当成功）。 */
function tfail<T>(
  code: TreeDiagCode,
  message: string,
  hint: string,
  at?: string,
): TreeOutcome<T> {
  return {
    ok: false,
    code,
    message,
    hint,
    diagnostics: [td(code, message, hint, at)],
  };
}

/** 成功构造（diagnostics 允许携带非致命告警）。 */
function tok<T>(value: T, diagnostics: readonly TreeDiagnostic[] = []): TreeOutcome<T> {
  return { ok: true, value, diagnostics };
}

// ════════════════════════════════════════════════════════════════════════════
// §2 控件节点四要素（判据一：四要素模型）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 属性键类型（封闭集：新增键须改本类型与 F2604 属性引擎的键表）。
 *
 * 键与值的分离是刻意的：树只认「键」用于路由与查询，「值」由属性引擎持有。
 * 树若也存一份值，就出现双源（见模块注释的警告段）。
 */
export type PropertyKey =
  | "text"
  | "visible"
  | "enabled"
  | "width"
  | "height"
  | "opacity"
  | "color"
  | "position-x"
  | "position-y"
  | "z-index"
  | "clip"
  | "aria-label"
  | "bind-path";

/** 属性键合法值全集（枚举守卫的事实源）。 */
export const PROPERTY_KEYS: readonly PropertyKey[] = [
  "text",
  "visible",
  "enabled",
  "width",
  "height",
  "opacity",
  "color",
  "position-x",
  "position-y",
  "z-index",
  "clip",
  "aria-label",
  "bind-path",
];

/**
 * 输入事件类型（封闭集：N03 输入域前向对接点）。
 *
 * 事件类型封闭的意义：N03 派发事件时按类型路由到处理器表，若类型不封闭，
 * 就会出现「派发到一个没有处理器的类型」——那类事件被静默丢弃，用户表现为
 * 「点了没反应」，而日志里什么都没有。封闭集 + 未知类型显性拒绝是同一件事。
 */
export type EventType =
  | "pointer-down"
  | "pointer-up"
  | "pointer-move"
  | "wheel"
  | "key-down"
  | "key-up"
  | "focus"
  | "blur"
  | "value-change"
  | "layout-change";

/** 事件类型合法值全集。 */
export const EVENT_TYPES: readonly EventType[] = [
  "pointer-down",
  "pointer-up",
  "pointer-move",
  "wheel",
  "key-down",
  "key-up",
  "focus",
  "blur",
  "value-change",
  "layout-change",
];

/**
 * 视觉状态枚举（状态机声明）。
 *
 * 为什么状态是**枚举**而不是一组布尔标志（hovered / pressed / focused）：
 *   布尔标志能表示的组合远多于合法组合——`hovered && pressed && disabled` 在语义上
 *   是矛盾的（禁用控件不该响应 hover），但布尔字段允许它存在，且各控件对矛盾
 *   组合的渲染策略不同，最终同一控件在不同实现里呈现不同颜色。枚举把矛盾组合
 *   从「可能」变成「不可能」，代价是需要状态机处理优先级（见 resolveVisualState）。
 */
export type VisualState =
  | "normal"
  | "hovered"
  | "pressed"
  | "focused"
  | "focused-hovered"
  | "disabled"
  | "disabled-hovered"
  | "active";

/** 视觉状态合法值全集。 */
export const VISUAL_STATES: readonly VisualState[] = [
  "normal",
  "hovered",
  "pressed",
  "focused",
  "focused-hovered",
  "disabled",
  "disabled-hovered",
  "active",
];

/**
 * 控件视觉状态的输入信号（由N03 输入域与属性引擎填充）。
 * 解析优先级：disabled > active > pressed > focused > hovered > normal。
 *
 * 优先级为何是这个序（写下来防止各控件各自实现一套）：
 *   disabled 必须最高优先——禁用控件仍会收到 hover 信号（鼠标经过就是会经过），
 *   若hover 优先于 disabled，用户会看到「禁用的按钮仍有高亮」，这是明确的可用性缺陷；
 *   pressed 高于 focused——按住鼠标时焦点确实也在，但视觉上用户期待的是「按下的样子」；
 *   active 单独一档，表示选中态（如选中标签页），它与焦点不是一回事：
 *   焦点在输入焦点位置，选中在业务状态位置。
 */
export interface StateSignals {
  readonly hovered: boolean;
  readonly pressed: boolean;
  readonly focused: boolean;
  readonly disabled: boolean;
  readonly active: boolean;
}

/**
 * 视觉状态解析：由信号求唯一状态（状态机的纯函数部分）。
 *
 * 这里是「状态机声明」的兑现点：状态不是由各处 scattered 地赋值，而是由一组
 * 固定信号经这一处纯函数求出。好处是状态组合的合法性由函数保证，且可被单测穷举
 * （2^5 = 32 种组合全测一遍即可），不需要为每个控件各写一遍状态测试。
 */
export function resolveVisualState(s: StateSignals): VisualState {
  if (s.disabled) {
    return s.hovered ? "disabled-hovered" : "disabled";
  }
  if (s.active) return "active";
  if (s.pressed) return "pressed";
  if (s.focused) return s.hovered ? "focused-hovered" : "focused";
  if (s.hovered) return "hovered";
  return "normal";
}

/**
 * 事件处理器表（事件→处理器的有序映射）。
 *
 * 用数组而非对象：本域的事件处理器需要**顺序语义**（同一事件类型可有多个处理器，
 * 按注册序依次执行，且允许后注册的处理器阻断前者）。对象键在 JS 中虽然保序，
 * 但那是实现细节而非语言保证，用数组把顺序写成显式契约。
 */
export interface EventHandlerEntry {
  readonly type: EventType;
  /** 处理器标识（实际调用由上层注入，本树只维护路由表）。 */
  readonly handlerId: string;
  /** 是否阻断后续同类型处理器（true 时同类型后续处理器不再执行）。 */
  readonly blocking: boolean;
}

/**
 * 一个控件节点。四要素逐要素规格（锚点要求「逐要素规格公开」）：
 *   属性 —— 属性键的有序集合（**只持键，不持值**；值归属性引擎 F2604）；
 *   子节点 —— 有序子控件列表（序稳定是三不变量之一）；
 *   事件 —— 输入事件处理器表（N03 前向对接）；
 *   状态 —— 视觉状态枚举（由 resolveVisualState 唯一求出）。
 */
export interface ControlNode {
  /** 节点 id（树内唯一；重复 id 会让绑定路径解析指向错误目标）。 */
  readonly id: string;
  /** 控件种类标签（button / panel / list / text 等；F2609 fuzz 面据此构造恶意树）。 */
  readonly kind: string;
  /**
   * 父节点 id（根节点为 null）。单亲不变量：此字段至多一个非空值，
   * 且与 children 列表必须双向一致（由 assertInvariants 机检）。
   */
  readonly parentId: string | null;
  /** 子节点 id 有序列表（序稳定：insert/remove/reorder 之外的路径不得改动它）。 */
  readonly children: readonly string[];
  /** 属性键有序集合（不含值——见模块注释的警告段）。 */
  readonly propertyKeys: readonly PropertyKey[];
  /** 事件处理器表（有序，同类型按序执行）。 */
  readonly handlers: readonly EventHandlerEntry[];
  /** 当前视觉状态（由 resolveVisualState 求出后写入，不由外部直改）。 */
  readonly state: VisualState;
  /** 当前状态信号（resolveVisualState 的输入；由输入域与属性引擎更新）。 */
  readonly signals: StateSignals;
  /** 绑定的轨道路径（M04/F2402 bind_path；null 表示无绑定）。 */
  readonly bindPath: string | null;
  /** 是否已销毁（销毁后仍被绑定引用须显性告警，不静默空转）。 */
  readonly destroyed: boolean;
}

/** 节点的只读视图构造器（供调用方安全读取，避免直接改内部结构）。 */
export function createNode(
  id: string,
  kind: string,
  init?: {
    readonly propertyKeys?: readonly PropertyKey[];
    readonly handlers?: readonly EventHandlerEntry[];
    readonly state?: VisualState;
    readonly bindPath?: string | null;
  },
): ControlNode {
  return {
    id,
    kind,
    parentId: null,
    children: [],
    propertyKeys: init?.propertyKeys ?? [],
    handlers: init?.handlers ?? [],
    state: init?.state ?? "normal",
    signals: { hovered: false, pressed: false, focused: false, disabled: false, active: false },
    bindPath: init?.bindPath ?? null,
    destroyed: false,
  };
}

// ════════════════════════════════════════════════════════════════════════════
// §3 树结构度量与三不变量声明（判据二）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 深度上限（恶意超深树的防护阈值，锚点「树深度攻击→深度上限拒绝」，
 * 与 F2609 fuzz 面联动）。
 *
 * 数值 512 的定标理由（写清楚以便复核与后续调整）：
 *   真实 UI 树的深度通常在 20~40 层（列表-列表项-卡片-控件组-控件）。
 *   512 已是真实深度的十余倍，仍在所有主流栈的递归安全范围内；
 *   而恶意构造可以轻松做到数万层（一个循环里不断 insert）。若不设上限，
 *   一次深度 50000 的插入就会让后续任何遍历（含命中测试、布局、序列化）
 *   触发调用栈溢出——那类崩溃在生产里表现为「打开某个界面就崩」，且崩溃点
 *   与攻击点无关，极难归因。512 让攻击在插入时就被拦下并指名节点。
 */
export const MAX_TREE_DEPTH = 512;

/** 树的度量（供基准 F2610 与遥测读取）。 */
export interface TreeMetrics {
  /** 节点总数。 */
  readonly nodeCount: number;
  /** 最大深度（根节点深度记0）。 */
  readonly maxDepth: number;
  /** 最大兄弟数（最宽的一层的子节点数）。 */
  readonly maxSiblingCount: number;
}

/**
 * 计算子树深度（不含 node自身，根为 0）。
 * 带 visited 集合做环保护：即便调用方在环树上调用，也不会无限递归——
 * 这不是「允许环」，而是保证「检测环的代码本身不会被环杀死」。
 *
 * 注：这是树级度量用的辅助实现；§4 的 ControlTree 内部另有迭代版
 * subtreeHeight（走真实节点表，深度校验用后者）。保留本函数是因为
 * 基准 F2610 需要一个「给定 children 取数函数即可算深度」的纯函数形态，
 * 不依赖节点表——万节点基准要在无ControlTree 实例的场景下测算深度。
 */
export function depthOf(
  nodeId: string,
  childrenOf: (id: string) => readonly string[],
  visited: Set<string>,
): number {
  if (visited.has(nodeId)) return 0;
  visited.add(nodeId);
  const kids = childrenOf(nodeId);
  if (kids.length === 0) return 0;
  let best = 0;
  for (const k of kids) {
    const d = depthOf(k, childrenOf, visited);
    if (d > best) best = d;
  }
  return best + 1;
}

/**
 * 三不变量的声明（锚点「三不变量声明」）—— 本条把三条写成可机检的断言，
 * 而不是文档里的承诺：
 *
 *   单亲（每节点至多一个父）——纪律：父指针是唯一权威，子列表只作镜像；
 *         任何改动父关系的操作必须同时更新两侧；禁止「只在子列表里加」。
 *         断言：assertInvariants 检查每个节点的 parentId 与其所在父的 children 互指。
 *   无环（祖先链无环）——纪律：插入前必须检查「待插入父节点是否为该节点的后代」；
 *         纪律：检查走祖先链上溯，不走子树下钻（后者对深树是 O(子树)）。
 *         断言：祖先链遍历带 visited，出现重复即判环。
 *   序稳定（兄弟序确定）——纪律：兄弟序只由 insert/remove/reorder 三操作改动，
 *         任何其他路径不得重排；序号在 remove 后**不重排**（避免外部持有的
 *         序号引用漂移）。
 *         断言：索引越界即拒绝，且 reorder 的目标索引按「移除后插入」语义计算。
 *
 * 关于「remove 后序号不重排」的取舍（这是最容易与下游冲突的一条，显式记录）：
 *   备选方案是 remove 后把后续兄弟的序号全部前移（保持序号连续）。它的问题是：
 *   若外部（虚拟化列表 F2617、动画轨道 M04）持有了兄弟序号作为稳定标识，
 *   序号前移会让那些标识指向别的控件——症状是「删掉第 3 行后，第 5 行的动画
 *   播到了第 4 行上」，且只在有删除操作的会话里出现。
 *   本域选择「序号只保证单调递增与不重复，不保证连续」，删除后允许出现空洞。
 *   需要连续序号的消费者（虚拟化）应使用**稳定 id**而非序号。
 */
export const TREE_INVARIANTS: readonly InvariantSpec[] = [
  {
    id: "single-parent",
    name: "单亲不变量",
    statement: "每个节点至多有一个父节点；父指针与父的子列表必须互指一致",
    discipline:
      "父指针（parentId）是唯一权威，子列表（children）只是镜像。"
      + "任何改动父关系的操作必须同事务更新两侧；禁止只改一侧。",
    assertion: "assertInvariants 逐节点核对双向互指；插入时先查待插入节点是否已有父",
    violationCode: "SINGLE_PARENT_VIOLATION",
    whyFundamental:
      "单亲一旦破坏，同一控件会在树中出现两次：命中测试会命中它两次（点击被处理两遍），"
      + "焦点遍历会走到它两次（Tab 键在同一控件上停两次）。且该缺陷只在特定树形下显现。",
  },
  {
    id: "acyclic",
    name: "无环不变量",
    statement: "任一节点的祖先链上不得出现重复节点",
    discipline:
      "插入前沿祖先链上溯检查待插入父是否为该节点的后代（O(深度)，不下钻子树）；"
      + "检查与插入必须同事务，中途发现环则不插入。",
    assertion: "祖先链遍历携带 visited 集合，遇重复即判环；环上的节点禁止任何插入",
    violationCode: "ANCESTOR_CYCLE",
    whyFundamental:
      "环一旦形成，遍历不再终止。症状不是报错而是界面卡死（布局遍历成环 → 每帧无限递归 → "
      + "主线程占满而进程仍活着）。恶意超深树是同一类攻击的规模化形态，故另有深度上限。",
  },
  {
    id: "stable-order",
    name: "序稳定不变量",
    statement: "兄弟序由 insert/remove/reorder 三操作唯一决定，其他路径不得重排",
    discipline:
      "移除后序号不重排（允许空洞、不保证连续）：外部若需稳定标识应使用节点 id。"
      + "重排的目标索引按「先移除、再插入」语义计算。",
    assertion: "索引越界即拒绝（ORDER_STABILITY_VIOLATION）；重排前校验目标索引合法",
    violationCode: "ORDER_STABILITY_VIOLATION",
    whyFundamental:
      "兄弟序决定绘制序、命中优先级与 Tab 焦点序。序不稳定时「Tab 跳到下一个控件」"
      + "会在两次按键间落到不同控件上（用户视角：焦点乱跳），且因每次重建树序不同而表现为偶发。",
  },
];

/** 一条不变量的声明规格。 */
export interface InvariantSpec {
  readonly id: string;
  readonly name: string;
  /** 不变量陈述（人话，可进代码评审 checklist）。 */
  readonly statement: string;
  /** 实现纪律：写代码时必须遵守的具体做法。 */
  readonly discipline: string;
  /** 断言方式：机检点在哪。 */
  readonly assertion: string;
  /** 违反时的诊断码。 */
  readonly violationCode: TreeDiagCode;
  /** 为什么这是根基而非可选的健壮性检查。 */
  readonly whyFundamental: string;
}

/** 不变量 id 全集（遍历用）。 */
export const INVARIANT_IDS: readonly string[] = TREE_INVARIANTS.map((i) => i.id);

// ════════════════════════════════════════════════════════════════════════════
// §4 树存储与生命周期（判据二·三不变量的运行时载体）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 控件树存储。
 *
 * 为什么是「可变存储 + 只读视图」而不是「不可变持久结构（persistent tree）」：
 *   控件树每帧可能被改动（属性失效引发的子树调整），不可变结构每次改动都要
 *   复制路径上的全部节点，万节点树的单次插入会变成千次分配——这与 F2407
 *   「全纪律零分配」的取向相反。VARIX 的树是「一棵树被多个消费者读」，
 *   不是「多棵树共享前缀」，复制没有收益。
 *   故取可变存储，把「不被外部改坏」的责任交给只读视图 + 三不变量断言：
 *   对外只暴露 snapshot（深拷贝只读视图）与遍历接口，内部结构不外泄。
 *
 * 可变存储的代价是「谁来保证外部没偷偷改内部数组」——这是本条必须在类型层面
 * 堵住的口子：所有对外读取一律经snapshot()，snapshot 返回的对象全部深冻结，
 * 调用方拿到后任何写操作在 strict 模式下抛 TypeError，不存在「拿到引用改了
 * 树却不知情」的中间态。
 */
export class ControlTree {
  private readonly nodes: Map<string, ControlNode> = new Map();
  private readonly rootId: string;
  private destroyed = false;

  /** 构造：必须给定唯一根节点 id（根节点也是普通节点，只是 parentId 为 null）。 */
  constructor(rootId: string) {
    if (rootId.length === 0) {
      throw new Error("rootId 不得为空：树必须有唯一根，否则遍历无从起步");
    }
    this.rootId = rootId;
    this.nodes.set(rootId, createNode(rootId, "root"));
  }

  /** 根节点 id。 */
  get root(): string {
    return this.rootId;
  }

  /** 是否已销毁（销毁后任何写操作拒绝并给TREE_LIFECYCLE_VIOLATION）。 */
  get isDestroyed(): boolean {
    return this.destroyed;
  }

  /** 节点是否存在。 */
  has(id: string): boolean {
    return this.nodes.has(id);
  }

  /** 节点数（基准 F2610 读取）。 */
  get size(): number {
    return this.nodes.size;
  }

  /**
   * 取节点的只读深拷贝视图。
   *
   * 深拷贝而非引用返回的理由：ControlNode 内含 children/propertyKeys/handlers
   * 三个数组。若按引用返回，调用方 push 一个子节点 id 就等于绕过单亲校验
   * 直接改了树——不变量断言会在下一次自检时报错，但那时破坏已经发生，
   * 且报错位置（自检）与破坏位置（某处push）完全脱节，无法归因。
   */
  snapshot(id: string): TreeOutcome<ControlNode> {
    if (this.destroyed) {
      return tfail(
        "TREE_LIFECYCLE_VIOLATION",
        "树已销毁，读取节点被拒绝",
        "销毁后不再读取节点；如需重建请new ControlTree(rootId)",
        id,
      );
    }
    const n = this.nodes.get(id);
    if (n === undefined) {
      return tfail("NODE_NOT_FOUND", "节点不存在于树中", "检查 id 是否拼错，或节点是否已被移除", id);
    }
    return tok(cloneNode(n));
  }

  /** 内部取节点（不复制，仅本模块内部用；外部一律走 snapshot）。 */
  private raw(id: string): ControlNode | undefined {
    return this.nodes.get(id);
  }

  /** 内部写节点（绕过只读约束的唯一入口，故设为private）。 */
  private put(n: ControlNode): void {
    this.nodes.set(n.id, n);
  }

  /** 浅层重建节点（改一个字段，其余字段不变）。 */
  private patch(id: string, changes: Partial<ControlNode>): void {
    const n = this.raw(id);
    if (n === undefined) return;
    this.put({ ...n, ...changes });
  }

  /**
   * 统一的前置守卫：树是否存活。
   * 单独抽出是因为每个写操作都要查一次，漏一处就是一个「销毁后仍可写入」
   * 的僵尸入口——症状是界面已关闭但后台仍在改树，内存泄漏且难以复现。
   */
  private guardAlive(): TreeOutcome<true> | null {
    if (this.destroyed) {
      return tfail(
        "TREE_LIFECYCLE_VIOLATION",
        "树已销毁，写操作被拒绝",
        "确认调用序：销毁后不得再写；重建树或改用新实例",
      );
    }
    return null;
  }

  // ── 遍历 ────────────────────────────────────────────────────────────────

  /**
   * 前序遍历（父在子前）。
   *
   * 用显式栈而非递归的理由与 MAX_TREE_DEPTH 是同一条纪律：深度上限把树
   * 约束在 512 层内，递归本身是安全的，但显式栈让「遍历不依赖调用栈」
   * 成为结构性事实而非参数约束的副作用——若日后有人把上限调到 5000，
   * 递归版会在毫无预警的情况下栈溢出，而显式栈只是变慢。
   * 迭代实现同时天然规避了环导致的无限递归（visited 集兜底）。
   */
  walk(order: "pre" | "post" = "pre"): TreeOutcome<readonly string[]> {
    if (this.destroyed) {
      return tfail("TREE_LIFECYCLE_VIOLATION", "树已销毁，遍历被拒绝", "销毁后不再遍历");
    }
    const out: string[] = [];
    const entered = new Set<string>();
    const collected = new Set<string>();
    // 后序遍历用「进入/离开」双标记栈。
    const stack: (readonly [string, boolean])[] = [[this.rootId, false]];
    let guard = 0;
    while (stack.length > 0) {
      const top = stack[stack.length - 1];
      if (top === undefined) break;
      const [id, exited] = top;
      stack.pop();
      guard += 1;
      // 硬性上界：节点数 * 2 + 2。超过说明遍历与节点表不一致（内部状态损坏），
      // 此时中止并报错，而不是无限跑下去占满CPU。
      if (guard > this.nodes.size * 2 + 2) {
        return tfail(
          "TREE_SELFCHECK_FAILED",
          "遍历步数超出节点数上界，树内部状态与节点表不一致",
          "检查是否有操作绕过三操作直接改了children；重新自检定位不一致节点",
          id,
        );
      }
      // entered 记「是否已展开过子节点」，两种趟次各有其责：
      //   进入趟：已展开过就跳过——这是环保护（环上节点会被反复要求展开）。
      //   退出趟：查的是「是否已被收集」，故须用 collected 而非 entered；
      //           若误用 entered，退出时必然命中「已展开」，节点永远进不了 out，
      //           后序遍历结果恒为空数组（且不报错，最难发现的一类静默错误）。
      if (exited) {
        if (collected.has(id)) continue;
        collected.add(id);
        out.push(id);
        continue;
      }
      if (entered.has(id)) continue;
      entered.add(id);
      const n = this.raw(id);
      if (n === undefined) continue;
      if (order === "pre") {
        out.push(id);
        // 前序不需要退出标记：进入即收集，子节点直接压在父之上即可。
        // 多压一个退出标记会让前序每个节点被访问两次（进入+退出），
        // 虽不改变结果，但把遍历步数翻倍，在万节点基准（F2610）上是实打实的成本。
        for (let i = n.children.length - 1; i >= 0; i -= 1) {
          const c = n.children[i];
          if (c !== undefined) stack.push([c, false]);
        }
        continue;
      }
      // 后序：先压退出标记再压子节点（逆序），使子节点先被处理。
      stack.push([id, true]);
      for (let i = n.children.length - 1; i >= 0; i -= 1) {
        const c = n.children[i];
        if (c !== undefined) stack.push([c, false]);
      }
    }
    return tok(out);
  }

  /** 祖先链（自 node 向根，顺序为 node → ... → root）。 */
  ancestors(id: string): TreeOutcome<readonly string[]> {
    const out: string[] = [];
    const seen = new Set<string>();
    let cur: string | null = id;
    while (cur !== null) {
      if (seen.has(cur)) {
        return tfail(
          "ANCESTOR_CYCLE",
          "祖先链上出现重复节点，树中存在环",
          "用 assertInvariants 定位环上的节点；环一旦形成遍历不再终止",
          cur,
        );
      }
      seen.add(cur);
      out.push(cur);
      const n = this.raw(cur);
      if (n === undefined) {
        return tfail("NODE_NOT_FOUND", "祖先链上行时节点不存在", "检查父指针指向了已移除节点", cur);
      }
      cur = n.parentId;
    }
    return tok(out);
  }

  /** 后代集合（不含自身）。 */
  descendants(id: string): TreeOutcome<readonly string[]> {
    const n = this.raw(id);
    if (n === undefined) {
      return tfail("NODE_NOT_FOUND", "节点不存在", "检查 id", id);
    }
    const out: string[] = [];
    const stack: string[] = [...n.children];
    const seen = new Set<string>();
    while (stack.length > 0) {
      const cur = stack.pop();
      if (cur === undefined) break;
      if (seen.has(cur)) continue;
      seen.add(cur);
      out.push(cur);
      const cn = this.raw(cur);
      if (cn === undefined) continue;
      for (const c of cn.children) stack.push(c);
    }
    return tok(out);
  }

  /** 度量（基准 F2610 读取的口径）。 */
  metrics(): TreeOutcome<TreeMetrics> {
    const all = this.walk("pre");
    if (!all.ok) return all;
    let maxDepth = 0;
    let maxSibling = 0;
    let count = 0;
    for (const id of all.value) {
      count += 1;
      const n = this.raw(id);
      if (n === undefined) continue;
      if (n.children.length > maxSibling) maxSibling = n.children.length;
      const chain = this.ancestors(id);
      if (!chain.ok) return chain;
      const d = chain.value.length - 1;
      if (d > maxDepth) maxDepth = d;
    }
    return tok({ nodeCount: count, maxDepth, maxSiblingCount: maxSibling });
  }

  // ── 销毁 ────────────────────────────────────────────────────────────────

  /**
   * 销毁整棵树。
   *
   * 销毁前先扫绑定：若有节点还挂着 bindPath，轨道（M04/F2402）下一帧仍会
   * 向已销毁的节点写属性。若不显式报出，症状是「界面关掉后动画还在跑，
   * 且偶尔抛空引用」——归因时几乎不会怀疑到绑定清理这里。故此处产出告警
   * 而非静默置空。
   */
  destroy(): TreeOutcome<readonly TreeDiagnostic[]> {
    if (this.destroyed) {
      return tfail("TREE_LIFECYCLE_VIOLATION", "树已处于销毁态", "重复销毁是调用序错误");
    }
    const warns: TreeDiagnostic[] = [];
    const all = this.walk("pre");
    if (all.ok) {
      for (const id of all.value) {
        const n = this.raw(id);
        if (n !== undefined && n.bindPath !== null) {
          warns.push(
            td(
              "BINDING_TARGET_DESTROYED",
              "节点仍持有绑定路径，树已销毁，该轨道将在下一帧写入失效目标",
              "先解绑（unbind）再销毁；或由轨道侧在树销毁时一并注销绑定",
              id,
            ),
          );
        }
      }
    }
    this.destroyed = true;
    return tok(warns);
  }
}

/** 深拷贝节点（对外视图的唯一构造方式）。 */
function cloneNode(n: ControlNode): ControlNode {
  return {
    id: n.id,
    kind: n.kind,
    parentId: n.parentId,
    children: n.children.slice(),
    propertyKeys: n.propertyKeys.slice(),
    handlers: n.handlers.map((h) => ({ ...h })),
    state: n.state,
    signals: { ...n.signals },
    bindPath: n.bindPath,
    destroyed: n.destroyed,
  };
}

// ════════════════════════════════════════════════════════════════════════════
// §5 树操作：insert / remove / reorder（判据三：原子操作）
// ════════════════════════════════════════════════════════════════════════════

/** 单亲冲突时的两种语义（锚点「两语义显性——设计声明」）。 */
export type SingleParentPolicy =
  /** 拒绝：节点已有父时报SINGLE_PARENT_VIOLATION（默认，严于默认）。 */
  | "reject"
  /** 自动摘除：把节点从原父摘下后插入新父（等价 move，便于 UI 重排场景）。 */
  | "detach";

/**
 * 批量事务的单步。
 *
 * 用判别联合而非「操作名 + 参数字典」：后者在运行时要靠switch(op) 再解构
 * 参数字典，参数拼错（如传了 parentId 给 remove）不会报错，只会被忽略——
 * 表现为「这步什么都没发生」，批次却报告成功。用判别联合后，
 * remove 分支的 payload 里根本没有 parentId 字段，拼不出来。
 */
export type TreeOp =
  | {
      readonly kind: "insert";
      readonly parentId: string;
      readonly childId: string;
      readonly index?: number;
      readonly policy?: SingleParentPolicy;
    }
  | { readonly kind: "remove"; readonly childId: string }
  | {
      readonly kind: "reorder";
      readonly parentId: string;
      readonly childId: string;
      readonly newIndex: number;
    }
  | { readonly kind: "setState"; readonly nodeId: string; readonly signals: StateSignals }
  | { readonly kind: "bind"; readonly nodeId: string; readonly bindPath: string | null }
  | { readonly kind: "unregister"; readonly childId: string };

/**
 * 事务快照（回滚的事实源）。
 *
 * 回滚不能靠「反向再执行一遍 remove/insert」——反向操作本身可能失败
 * （例如已被外部改动过），那时树会停在半回滚状态，比不回滚更糟：
 * 不回滚至少是「操作整体失败」，半回滚是「树结构已损坏但没人知道」。
 * 故快照记录**改动前的原始字段值**，回滚是纯赋值，不依赖任何操作成功。
 */
interface TxSnapshot {
  readonly nodeId: string;
  readonly existedBefore: boolean;
  readonly before: ControlNode | null;
}

/**
 * insert：把 child 插入 parent 的子列表。
 *
 * 校验顺序刻意是「便宜的在前」：父存在 → 节点存在 → 自插入 → 单亲 → 无环 → 深度。
 * 理由是每一项检查的代价不同（无环O(深度)、深度 O(子树)），而绝大多数非法调用
 * 在第二三项就被拦下。若把无环与深度检查放最前，每次「父不存在」这种最常见的
 * 调用错误都要白跑一遍 O(深度) 遍历——在批量重建树的场景（万次insert）里
 * 那是数量级可观的浪费。
 */
export function insert(
  tree: ControlTree,
  op: Extract<TreeOp, { kind: "insert" }>,
): TreeOutcome<ControlNode> {
  const dead = tree["guardAlive"]();
  if (dead !== null) return dead as TreeOutcome<ControlNode>;

  const parent = tree["raw"](op.parentId);
  if (parent === undefined) {
    return tfail(
      "PARENT_INVALID",
      "父节点不存在于树中",
      "检查 parentId；父节点须先 insert 到树上",
      op.parentId,
    );
  }
  let child = tree["raw"](op.childId);
  if (child === undefined) {
    // 节点未注册：显式注册（避免要求调用方分两步，也避免自动注册掩盖拼写错误）。
    child = createNode(op.childId, "panel");
    tree["put"](child);
  }
  if (op.childId === op.parentId) {
    return tfail(
      "SELF_INSERTION",
      "节点不能插入到自己的子列表",
      "自插入是祖先环的特例，会使遍历不终止",
      op.childId,
    );
  }

  const policy: SingleParentPolicy = op.policy ?? "reject";
  let detachedFrom: string | null = null;
  let detachedIdx = -1;
  if (child.parentId !== null) {
    if (policy === "reject") {
      return tfail(
        "SINGLE_PARENT_VIOLATION",
        `节点已有父（${child.parentId}），按 reject 策略拒绝插入`,
        "若意图是移动节点，请显式传 policy:'detach'；不要依赖隐式摘除",
        op.childId,
      );
    }
    detachedFrom = child.parentId;
    const dp = tree["raw"](detachedFrom);
    detachedIdx = dp === undefined ? -1 : dp.children.indexOf(op.childId);
    if (dp !== undefined && detachedIdx >= 0) {
      const kids = dp.children.slice();
      kids.splice(detachedIdx, 1);
      tree["patch"](detachedFrom, { children: kids });
    }
  }

  // 无环校验：若 parent 位于 child 的后代链上，插入即成环。
  // 上溯 parent 的祖先链，看是否遇到 child —— O(深度)，不下钻子树。
  let cur: string | null = op.parentId;
  let hops = 0;
  while (cur !== null) {
    if (cur === op.childId) {
      // 成环：把刚才的摘除回滚（摘除已发生，不能留下半成品）。
      if (detachedFrom !== null) {
        const dp = tree["raw"](detachedFrom);
        if (dp !== undefined) {
          const kids = dp.children.slice();
          kids.splice(Math.max(0, detachedIdx), 0, op.childId);
          tree["patch"](detachedFrom, { children: kids });
        }
      }
      return tfail(
        "ANCESTOR_CYCLE",
        `插入将形成环：目标父 ${op.parentId} 是 ${op.childId} 的后代`,
        "调整层级：容器只能挂到不含自己的分支上；或改用 remove + insert 两步",
        op.childId,
      );
    }
    const cn = tree["raw"](cur);
    if (cn === undefined) break;
    cur = cn.parentId;
    hops += 1;
    if (hops > MAX_TREE_DEPTH + 1) {
      return tfail(
        "ANCESTOR_CYCLE",
        "祖先链长度超出深度上限，链上疑似存在环",
        "运行 assertInvariants 定位环；不要直接改树绕过操作接口",
        cur ?? op.parentId,
      );
    }
  }

  // 深度校验：新节点深度 = 父深度 + 1 + child子树高度。超限即拒。
  const parentChain = tree.ancestors(op.parentId);
  if (!parentChain.ok) return parentChain as TreeOutcome<ControlNode>;
  const parentDepth = parentChain.value.length - 1;
  const sub = subtreeHeight(tree, op.childId);
  if (sub === null) {
    return tfail("NODE_NOT_FOUND", "计算子树高度时节点不存在", "检查 childId", op.childId);
  }
  const newDepth = parentDepth + 1 + sub;
  if (newDepth > MAX_TREE_DEPTH) {
    // 同样要把摘除回滚。
    if (detachedFrom !== null) {
      const dp = tree["raw"](detachedFrom);
      if (dp !== undefined) {
        const kids = dp.children.slice();
        kids.splice(Math.max(0, detachedIdx), 0, op.childId);
        tree["patch"](detachedFrom, { children: kids });
      }
    }
    return tfail(
      "DEPTH_LIMIT_EXCEEDED",
      `插入后深度 ${newDepth} 超过上限 ${MAX_TREE_DEPTH}（恶意超深树防护）`,
      "降低嵌套层级：把深层内容改为惰性展开/列表虚拟化，而不是加深树",
      op.childId,
    );
  }

  // 序稳定校验：索引须落在 [0, children.length]。
  const kids = parent.children.slice();
  const idx = op.index === undefined ? kids.length : op.index;
  if (!Number.isInteger(idx) || idx < 0 || idx > kids.length) {
    if (detachedFrom !== null) {
      const dp = tree["raw"](detachedFrom);
      if (dp !== undefined) {
        const rk = dp.children.slice();
        rk.splice(Math.max(0, detachedIdx), 0, op.childId);
        tree["patch"](detachedFrom, { children: rk });
      }
    }
    return tfail(
      "ORDER_STABILITY_VIOLATION",
      `插入索引 ${String(op.index)} 越界（合法区间0..${kids.length}）`,
      "index 省略或取 0..当前子节点数；越界通常意味着对兄弟数有陈旧假设",
      op.parentId,
    );
  }

  kids.splice(idx, 0, op.childId);
  tree["patch"](op.parentId, { children: kids });
  tree["patch"](op.childId, { parentId: op.parentId, destroyed: false });
  const saved = tree["raw"](op.childId);
  return saved === undefined
    ? tfail("NODE_NOT_FOUND", "插入后节点丢失", "这是内部一致性错误，请上报", op.childId)
    : tok(cloneNode(saved));
}

/** 子树高度（自身不计入，叶子为 0）。带环保护。 */
function subtreeHeight(tree: ControlTree, id: string): number | null {
  const n = tree["raw"](id);
  if (n === undefined) return null;
  const visited = new Set<string>();
  let best = 0;
  // 迭代式：显式栈 + (id, depth)。
  const stack: (readonly [string, number])[] = [[id, 0]];
  let guard = 0;
  while (stack.length > 0) {
    const top = stack[stack.length - 1];
    if (top === undefined) break;
    stack.pop();
    guard += 1;
    if (guard > MAX_TREE_DEPTH + 8) return null;
    if (visited.has(top[0])) continue;
    visited.add(top[0]);
    if (top[1] > best) best = top[1];
    const cn = tree["raw"](top[0]);
    if (cn === undefined) continue;
    for (const c of cn.children) stack.push([c, top[1] + 1]);
  }
  return best;
}

/**
 * remove：把节点从父的子列表摘下（节点本身保留在表中成为游离节点）。
 *
 * 为何不连节点一起删（unregister 是另一个操作）：布局/动画在摘除瞬间还需要
 * 读节点的最终矩形来播退出动画。若remove 即销毁，退出动画就没有数据源，
 * 只能由调用方提前缓存——而缓存时机无法统一，迟早漏。摘下但保留，
 * 把「最终态读取」的窗口留给调用方，再由 unregister 显式回收。
 */
export function remove(
  tree: ControlTree,
  op: Extract<TreeOp, { kind: "remove" }>,
): TreeOutcome<ControlNode> {
  const dead = tree["guardAlive"]();
  if (dead !== null) return dead as TreeOutcome<ControlNode>;
  const child = tree["raw"](op.childId);
  if (child === undefined) {
    return tfail("NODE_NOT_FOUND", "待移除节点不存在", "检查 childId；重复 remove 是常见调用序错误", op.childId);
  }
  if (op.childId === tree.root) {
    return tfail(
      "ORDER_STABILITY_VIOLATION",
      "根节点不可移除",
      "树必须有根；整树销毁请用 destroy()",
      op.childId,
    );
  }
  if (child.parentId === null) {
    return tfail(
      "SINGLE_PARENT_VIOLATION",
      "节点当前没有父，remove 无对象可摘",
      "该节点已是游离态；游离节点应走 unregister 回收",
      op.childId,
    );
  }
  const parent = tree["raw"](child.parentId);
  if (parent === undefined) {
    return tfail("PARENT_INVALID", "父节点不存在，树状态已损坏", "运行 assertInvariants 定位", child.parentId);
  }
  const kids = parent.children.slice();
  const idx = kids.indexOf(op.childId);
  if (idx < 0) {
    return tfail(
      "SINGLE_PARENT_VIOLATION",
      "父的子列表中找不到该节点（父子双向不一致）",
      "运行 assertInvariants；不得直接改 children 数组绕过操作接口",
      op.childId,
    );
  }
  kids.splice(idx, 1);
  tree["patch"](parent.id, { children: kids });
  tree["patch"](op.childId, { parentId: null });
  return tok(cloneNode(tree["raw"](op.childId) as ControlNode));
}

/**
 * reorder：调整节点在父子列表中的位置。
 *
 * 语义声明：「先移除、再插入」，且 newIndex 是**移除之后**的目标位。
 * 举例：children=[a,b,c]，reorder(a, 2) → 移除 a 得 [b,c] → 在索引 2 插入得 [b,c,a]。
 * 若把 newIndex 解释为「移除之前的位置」，同一操作会得到 [c,a,b]——
 * 两种语义都自洽，但调用方按前一种理解写代码、按后一种理解看结果，
 * 症状是「重排结果和预期差一位」，且每种操作都差一位，极难归因。
 * 故在签名与文档中固定为移除后语义，并用单测锁死。
 */
export function reorder(
  tree: ControlTree,
  op: Extract<TreeOp, { kind: "reorder" }>,
): TreeOutcome<ControlNode> {
  const dead = tree["guardAlive"]();
  if (dead !== null) return dead as TreeOutcome<ControlNode>;
  const parent = tree["raw"](op.parentId);
  if (parent === undefined) {
    return tfail("PARENT_INVALID", "父节点不存在", "检查 parentId", op.parentId);
  }
  const child = tree["raw"](op.childId);
  if (child === undefined) {
    return tfail("NODE_NOT_FOUND", "待重排节点不存在", "检查 childId", op.childId);
  }
  if (child.parentId !== op.parentId) {
    return tfail(
      "SINGLE_PARENT_VIOLATION",
      "节点不属于该父，reorder 无效",
      "reorder 只能在节点当前的父下调整；跨父请用 insert(policy:'detach')",
      op.childId,
    );
  }
  const kids = parent.children.slice();
  const from = kids.indexOf(op.childId);
  if (from < 0) {
    return tfail("SINGLE_PARENT_VIOLATION", "父的子列表中找不到该节点", "运行 assertInvariants", op.childId);
  }
  kids.splice(from, 1);
  const to = op.newIndex;
  if (!Number.isInteger(to) || to < 0 || to > kids.length) {
    return tfail(
      "ORDER_STABILITY_VIOLATION",
      `重排目标索引 ${String(op.newIndex)} 越界（移除后合法区间0..${kids.length}）`,
      "注意索引语义为「移除之后」的目标位，故上界是原长度减一",
      op.childId,
    );
  }
  kids.splice(to, 0, op.childId);
  tree["patch"](op.parentId, { children: kids });
  return tok(cloneNode(tree["raw"](op.childId) as ControlNode));
}

/** 设置状态信号 → 状态经 resolveVisualState 唯一求出。 */
export function setState(
  tree: ControlTree,
  op: Extract<TreeOp, { kind: "setState" }>,
): TreeOutcome<VisualState> {
  const dead = tree["guardAlive"]();
  if (dead !== null) return dead as TreeOutcome<VisualState>;
  const n = tree["raw"](op.nodeId);
  if (n === undefined) {
    return tfail("NODE_NOT_FOUND", "节点不存在", "检查 nodeId", op.nodeId);
  }
  const next = resolveVisualState(op.signals);
  tree["patch"](op.nodeId, { signals: { ...op.signals }, state: next });
  return tok(next);
}

/**
 * 事务执行器：批量操作 = 单事务（锚点「操作原子性（批量=单事务）」）。
 *
 * 三条纪律：
 *   1. 全成功 → 全部生效，收集沿途告警；
 *   2. 任一步失败 → 已生效的步骤**全部回滚**，返回失败 + 回滚事实 + 失败步序号；
 *   3. 回滚本身不依赖任何操作成功（用快照纯赋值，见TxSnapshot 注释）。
 *
 * 为什么必须回滚而不是「失败即停」（这是最容易被简化掉的一条）：
 *   批量操作在真实场景里都是「一次重建一棵子树」或「一次应用一批轨道绑定」。
 *   若第 7 步失败而前 6 步已生效，树就停在「一半新一半旧」的状态——
 *   它不报错、不崩溃，只是一直显示错误内容。用户在界面上看到的是
 *   「这个面板一半是新的、一半是旧的」，开发者从任何日志里都看不出原因，
 *   因为每一步单独看都成功了。这类缺陷的定位成本是数量级的。
 */
export function transact(
  tree: ControlTree,
  ops: readonly TreeOp[],
): TreeOutcome<readonly string[]> {
  const dead = tree["guardAlive"]();
  if (dead !== null) return dead as TreeOutcome<readonly string[]>;

  const snaps: TxSnapshot[] = [];
  const warns: TreeDiagnostic[] = [];
  const applied: string[] = [];

  for (let i = 0; i < ops.length; i += 1) {
    const op = ops[i];
    if (op === undefined) continue;
    // 步骤前留快照。快照必须覆盖**本步会触及的全部节点**，而不只是目标节点：
    //   insert/reorder/remove 都会改动「父节点的 children 列表」——
    //   只快照子节点的话，事务失败后子节点被还原了，父节点的 children 里
    //   却还留着它，即父说「这个孩子在这儿」、孩子说「我没有父亲」。
    //   这种半回滚状态不报错、不崩溃，只在后续遍历时表现为节点重复出现，
    //   归因成本极高（要一路倒推是哪次批处理失败留下的）。
    //   故此处取目标节点与其父节点的前像，一并入快照。
    const targetId = opTargetId(op);
    for (const sid of snapshotIds(tree, op, targetId)) {
      snaps.push({
        nodeId: sid,
        existedBefore: tree["raw"](sid) !== undefined,
        before: tree["raw"](sid) ?? null,
      });
    }

    const r = applyOp(tree, op);
    if (!r.ok) {
      // 回滚：按快照逆序赋值（同一节点被快照多次时，后入的先还原，
      // 逆序回放正好让它回到最初状态）。
      for (let k = snaps.length - 1; k >= 0; k -= 1) {
        const s = snaps[k];
        if (s === undefined || s.nodeId === "") continue;
        if (s.existedBefore && s.before !== null) {
          tree["put"](cloneNode(s.before));
        } else {
          tree["nodes"].delete(s.nodeId);
        }
      }
      const rbDiag = td(
        "BATCH_ROLLED_BACK",
        `批量第 ${i + 1}/${ops.length} 步失败（${op.kind}），已回滚全部 ${applied.length} 步已生效操作`,
        "修正该步参数后重试；本事务未产生任何生效副作用",
        opTargetId(op) ?? undefined,
      );
      return {
        ok: false,
        code: r.code,
        message: r.message,
        hint: r.hint,
        diagnostics: [...r.diagnostics, rbDiag, ...warns],
      };
    }
    applied.push(op.kind);
    warns.push(...r.diagnostics);
  }
  return tok(applied, warns);
}

/**
 * 本步需要快照的节点集合（目标节点 + 会被改动的父节点）。
 *
 * 逐类列明的理由（哪一步动了谁，少列一个就是一处半回滚）：
 *   insert  → 改父.children + 子.parentId，故取 {子, 新父}；detach 策略还会改「旧父」，
 *              故旧父一并取（否则移动失败后旧父的children 里会残留该子节点）。
 *   remove  → 改父.children + 子.parentId，取 {子, 父}。
 *   reorder → 只改父.children，取 {子, 父}（子本身字段不变，多取无妨且更保险）。
 *   unregister → 只删目标，取 {子}。
 *   setState / bind → 只改目标自身，取 {目标}。
 */
function snapshotIds(tree: ControlTree, op: TreeOp, targetId: string | null): string[] {
  const ids: string[] = [];
  const push = (id: string | null | undefined): void => {
    if (typeof id === "string" && id.length > 0 && !ids.includes(id)) ids.push(id);
  };
  push(targetId);
  switch (op.kind) {
    case "insert":
    case "reorder":
      push(op.parentId);
      if (op.kind === "insert") {
        const cur = tree["raw"](op.childId);
        push(cur === undefined ? null : cur.parentId);
      }
      break;
    case "remove": {
      const cur = tree["raw"](op.childId);
      push(cur === undefined ? null : cur.parentId);
      break;
    }
    case "unregister":
    case "setState":
    case "bind":
      break;
    default: {
      const never: never = op;
      throw new Error(`未覆盖的操作类型：${String(never)}`);
    }
  }
  return ids;
}

/** 取操作的目标节点 id（无目标则为 null）。 */
function opTargetId(op: TreeOp): string | null {
  switch (op.kind) {
    case "insert":
    case "remove":
    case "unregister":
      return op.childId;
    case "reorder":
      return op.childId;
    case "setState":
      return op.nodeId;
    case "bind":
      return op.nodeId;
    default: {
      const never: never = op;
      return never;
    }
  }
}

/** 单步执行（transact 与直调共用的内核）。 */
function applyOp(tree: ControlTree, op: TreeOp): TreeOutcome<unknown> {
  switch (op.kind) {
    case "insert":
      return insert(tree, op);
    case "remove":
      return remove(tree, op);
    case "reorder":
      return reorder(tree, op);
    case "setState":
      return setState(tree, op);
    case "bind": {
      if (op.bindPath !== null && op.bindPath.length === 0) {
        return tfail(
          "BINDING_PATH_INVALID",
          "绑定路径为空串",
          "解绑请传 bindPath:null；空串不是合法路径",
          op.nodeId,
        );
      }
      if (tree["raw"](op.nodeId) === undefined) {
        return tfail("NODE_NOT_FOUND", "绑定目标节点不存在", "检查 nodeId", op.nodeId);
      }
      tree["patch"](op.nodeId, { bindPath: op.bindPath });
      return tok(true);
    }
    case "unregister": {
      const n = tree["raw"](op.childId);
      if (n === undefined) {
        return tfail("NODE_NOT_FOUND", "待注销节点不存在", "检查 childId；重复注销是常见调用序错误", op.childId);
      }
      if (n.parentId !== null) {
        return tfail(
          "SINGLE_PARENT_VIOLATION",
          "节点仍挂在树上，先 remove 再 unregister",
          "顺序：remove（摘下）→ unregister（回收）；顺序反了会留下悬空父指针",
          op.childId,
        );
      }
      if (op.childId === tree.root) {
        return tfail("TREE_LIFECYCLE_VIOLATION", "根节点不可注销", "整树销毁请用 destroy()", op.childId);
      }
      tree["nodes"].delete(op.childId);
      return tok(true);
    }
    default: {
      const never: never = op;
      throw new Error(`未覆盖的操作类型：${String(never)}`);
    }
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §6 M04 绑定路径解析（判据四：M04 绑定）
// ════════════════════════════════════════════════════════════════════════════

/**
 * 绑定路径语法（M04 / F2402 树侧）。
 *
 *   语法：seg ('/' seg)*
 *   seg  ：段名 [ '#' 兄弟序号 ]
 *   例：'root#0/panel#2/button#1'  表示 root 的第 0 个子节点 → 其第 2 个子 → 第 1 个
 *
 * 为何要支持兄弟序号段（'#n'）而不是仅靠 id 定位：
 *   纯 id 路径在「节点被 remove 后再重建」的场景下会指向不存在的节点，
 *   而用户视角下「第 2 个面板里的按钮」这个语义是稳定的。两者都需要，
 *   故语法同时支持：段名匹配优先用 kind，序号用于在同类兄弟中定位。
 *   反之若只支持序号，节点一旦被重排，绑定就会静默漂到别的控件上——
 *   症状是「动画打到隔壁控件」，极难归因。因此纪律是：
 *   段名优先匹配 kind，序号仅作同类消歧，不作为唯一定位手段。
 */
export interface BindSegment {
  readonly name: string;
  /** 兄弟序号（缺省为 -1，表示不消歧）。 */
  readonly index: number;
}

/**
 * 解析绑定路径为段序列。
 *
 * 解析与定位分成两步（而不是一步到位）的原因：路径语法错误与路径解析不到
 * 是两类完全不同的问题。前者是调用方的拼写/格式错（几乎恒为bug），
 * 后者是数据在运行时变了（节点被删、层级调整，属可预期的状态变化）。
 * 合成一个错误码时，开发者分不清该改代码还是该改数据，两种都修不对。
 */
export function parseBindPath(path: string): TreeOutcome<readonly BindSegment[]> {
  if (path.length === 0) {
    return tfail("BINDING_PATH_INVALID", "绑定路径为空串", "解绑用 bindPath:null；空串不是合法路径");
  }
  if (path.startsWith("/") || path.endsWith("/")) {
    return tfail(
      "BINDING_PATH_INVALID",
      `绑定路径以分隔符开头或结尾：${path}`,
      "路径应为 seg/seg 形式，首尾不得有 '/'",
    );
  }
  const out: BindSegment[] = [];
  for (const raw of path.split("/")) {
    if (raw.length === 0) {
      return tfail("BINDING_PATH_INVALID", `绑定路径含空段：${path}`, "检查连续分隔符 '//'");
    }
    const hashAt = raw.indexOf("#");
    if (hashAt < 0) {
      if (!/^[A-Za-z0-9_.-]+$/.test(raw)) {
        return tfail(
          "BINDING_PATH_INVALID",
          `段名含非法字符：${raw}`,
          "段名只允许字母数字下划线点与连字符；'#' 保留给序号",
        );
      }
      out.push({ name: raw, index: -1 });
      continue;
    }
    const name = raw.slice(0, hashAt);
    const idxStr = raw.slice(hashAt + 1);
    if (name.length === 0 || !/^[A-Za-z0-9_.-]+$/.test(name)) {
      return tfail("BINDING_PATH_INVALID", `段名非法：${raw}`, "检查 '#' 前的段名");
    }
    if (!/^[0-9]+$/.test(idxStr)) {
      return tfail(
        "BINDING_PATH_INVALID",
        `兄弟序号非法：${raw}`,
        "'#' 后须为非负整数；缺省序号请省略 '#'",
      );
    }
    out.push({ name, index: Number(idxStr) });
  }
  return tok(out);
}

/**
 * 按路径定位节点（M04 轨道→树节点的解析器）。
 *
 * 失败分两类并分别立码：
 *   路径语法错/ 段在同级找不到 → BINDING_PATH_INVALID（数据变了，须显性告警）；
 *   定位到的节点已销毁 → BINDING_TARGET_DESTROYED（生命周期错位）。
 * 之所以把「销毁」单列而不并入 INVALID：二者的处置不同——INVALID 通常要
 * 修路径或补节点，destroyed 要查生命周期序（谁在节点销毁后还在写绑定）。
 * 并成一个码，开发者只能靠猜。
 */
export function resolveBindPath(
  tree: ControlTree,
  path: string,
): TreeOutcome<ControlNode> {
  const parsed = parseBindPath(path);
  if (!parsed.ok) {
    return {
      ok: false,
      code: parsed.code,
      message: parsed.message,
      hint: parsed.hint,
      diagnostics: [...parsed.diagnostics, td("BINDING_PATH_INVALID", `绑定路径 ${path} 解析失败`, "修正路径语法后重试", path)],
    };
  }
  let current = tree.root;
  for (let s = 0; s < parsed.value.length; s += 1) {
    const seg = parsed.value[s];
    if (seg === undefined) continue;
    // 首段定位根本身（路径首段写的是根的 id/kind，而非根的某个子节点）。
    // 不这么处理会出现「路径第一段永远匹配不到」的荒谬现象：解析从 current=root
    // 起步，若首段也去root 的子级里找root，匹配对象与被匹配对象同级不同层。
    if (s === 0) {
      const rootNode = tree["raw"](tree.root);
      const rootHit = rootNode !== undefined && (rootNode.id === seg.name || rootNode.kind === seg.name);
      if (!rootHit) {
        return tfail(
          "BINDING_PATH_INVALID",
          `路径 ${path} 首段 ${seg.name} 未命中根节点（根 id=${tree.root}）`,
          "首段必须写根的 id 或 kind；路径从根自身起算，不从根的子节点起算",
          path,
        );
      }
      continue;
    }
    const node = tree["raw"](current);
    if (node === undefined) {
      return tfail("BINDING_PATH_INVALID", `路径 ${path} 第 ${s} 段上行时节点已不存在`, "数据在绑定建立后被改动", path);
    }
    // 先按 kind 匹配所有同类兄弟，再按序号消歧；无序号则取首个（段名唯一时）。
    const sameKind: string[] = [];
    for (const c of node.children) {
      const cn = tree["raw"](c);
      if (cn !== undefined && (cn.kind === seg.name || cn.id === seg.name)) sameKind.push(c);
    }
    if (sameKind.length === 0) {
      return tfail(
        "BINDING_PATH_INVALID",
        `路径 ${path} 第 ${s + 1} 段 ${seg.name} 在节点 ${current} 的子级中不存在`,
        "确认该段对应的 kind 或 id 拼写；层级调整后绑定须同步更新",
        path,
      );
    }
    let picked: string | undefined;
    if (seg.index < 0) {
      if (sameKind.length > 1) {
        return tfail(
          "BINDING_PATH_INVALID",
          `路径 ${path} 第 ${s + 1} 段 ${seg.name} 匹配到 ${sameKind.length} 个兄弟，无序号无法唯一定位`,
          "在段名后加 '#序号' 消歧，例如 " + seg.name + "#1",
          path,
        );
      }
      // 先排除 length>1，故此处长度恒为 1；用长度守卫取值，
      // 避免在 noUncheckedIndexedAccess 下依赖「[0] 一定存在」的类型假设。
      if (sameKind.length !== 1) {
        return tfail(
          "BINDING_PATH_INVALID",
          `路径 ${path} 第 ${s + 1} 段 ${seg.name} 匹配结果异常（长度 ${sameKind.length}）`,
          "同类兄弟数应为 1；为 0 已在上一分支拦截，此分支不可达",
          path,
        );
      }
      picked = sameKind[0];
    } else {
      picked = sameKind[seg.index];
      if (picked === undefined) {
        return tfail(
          "BINDING_PATH_INVALID",
          `路径 ${path} 第 ${s + 1} 段 ${seg.name}#${seg.index} 越界（同类兄弟仅 ${sameKind.length} 个）`,
          "序号范围为 0..同类兄弟数-1；节点减少后须同步收号",
          path,
        );
      }
    }
    // 两个分支都已在各自路径上处理 undefined（无序号分支用长度守卫取值），
    // 但 TS 无法跨分支证明 picked 非空。此处用显式守卫收窄——
    // 不用 `!` 非空断言：断言只消警告不消风险，若将来某分支漏了守卫，
    // 断言会把 undefined 静默放行，而守卫会显性报错。
    if (picked === undefined) {
      return tfail(
        "BINDING_PATH_INVALID",
        `路径 ${path} 第 ${s + 1} 段 ${seg.name} 未能定位到节点`,
        "同类兄弟集合为空应已在上一分支拦截；此处不可达，命中即为逻辑漏洞请上报",
        path,
      );
    }
    const chosen: string = picked;
    const pickedNode = tree["raw"](chosen);
    if (pickedNode === undefined) {
      return tfail("BINDING_PATH_INVALID", `路径 ${path} 定位到的节点已不存在`, "重新解析绑定", path);
    }
    if (pickedNode.destroyed) {
      return tfail(
        "BINDING_TARGET_DESTROYED",
        `路径 ${path} 定位到已销毁节点 ${chosen}`,
        "查生命周期序：销毁前须先 unbind，或由轨道侧随树销毁一并注销",
        picked,
      );
    }
    current = chosen;
  }
  const finalNode = tree["raw"](current);
  return finalNode === undefined
    ? tfail("BINDING_PATH_INVALID", `路径 ${path} 最终目标不存在`, "重新解析绑定", path)
    : tok(cloneNode(finalNode));
}

// ════════════════════════════════════════════════════════════════════════════
// §7 三不变量运行时断言（判据二的可执行形态）
// ════════════════════════════════════════════════════════════════════════════

/** 断言报告：逐条不变量给出通过与否与首个违反点。 */
export interface InvariantReport {
  readonly invariantId: string;
  readonly passed: boolean;
  readonly diagnostics: readonly TreeDiagnostic[];
}

/**
 * 三不变量断言（三条一次跑完，返回逐条报告）。
 *
 * 为什么断言独立于操作而不是内联在每个操作里：
 *   内联断言只能证明「经过本模块的操作产生的树是合法的」，证不了
 *   「树当前是合法的」——而后者才是下游真正依赖的前提。断言作为独立入口，
 *   下游可在任何时机调用（收到外部事件后、序列化前、收工自检时）。
 *   代价是 O(节点) 的全量检查，故文档明确建议：操作路径内靠内联校验（已做），
 *   断言用于边界时机（导入后、帧末、开发模式持续跑）。
 */
export function assertInvariants(tree: ControlTree): TreeOutcome<readonly InvariantReport[]> {
  const dead = tree["guardAlive"]();
  if (dead !== null) return dead as TreeOutcome<readonly InvariantReport[]>;
  const reports: InvariantReport[] = [];
  const all = tree.walk("pre");
  if (!all.ok) {
    return {
      ok: false,
      code: all.code,
      message: all.message,
      hint: all.hint,
      diagnostics: [...all.diagnostics],
    };
  }
  const ids = all.value;

  // ① 单亲：父指针与父子列表双向互指。
  const spDiags: TreeDiagnostic[] = [];
  for (const id of ids) {
    const n = tree["raw"](id);
    if (n === undefined) continue;
    if (n.parentId === null) {
      if (id !== tree.root) {
        spDiags.push(
          td(
            "SINGLE_PARENT_VIOLATION",
            `非根节点 ${id} 的父指针为空（游离节点）`,
            "游离节点须经 unregister 回收；不应长期驻留在表中",
            id,
          ),
        );
      }
      continue;
    }
    const p = tree["raw"](n.parentId);
    if (p === undefined) {
      spDiags.push(
        td("SINGLE_PARENT_VIOLATION", `节点 ${id} 的父 ${n.parentId} 不存在`, "父指针悬空；不得直接改 parentId", id),
      );
      continue;
    }
    if (!p.children.includes(id)) {
      spDiags.push(
        td(
          "SINGLE_PARENT_VIOLATION",
          `节点 ${id} 认为自己挂在 ${n.parentId} 下，但父的子列表中没有它`,
          "父子双向不一致：只改一侧是三不变量最常见的破法",
          id,
        ),
      );
    }
    if (id === tree.root) {
      spDiags.push(
        td("SINGLE_PARENT_VIOLATION", "根节点的父指针非空", "根的parentId 必须为 null", id),
      );
    }
  }
  reports.push({ invariantId: "single-parent", passed: spDiags.length === 0, diagnostics: spDiags });

  // ② 无环：遍历可达集合与全表比对（不可达即说明存在环或游离）。
  const acDiags: TreeDiagnostic[] = [];
  const reach = new Set(ids);
  for (const id of ids) {
    if (!reach.has(id)) {
      acDiags.push(
        td("ANCESTOR_CYCLE", `节点 ${id} 从根不可达`, "不可达说明它落在环上或已脱离树", id),
      );
    }
  }
  const ac = tree.ancestors(tree.root);
  if (!ac.ok) {
    acDiags.push(...ac.diagnostics);
  }
  reports.push({ invariantId: "acyclic", passed: acDiags.length === 0, diagnostics: acDiags });

  // ③ 序稳定：同一父下不得有重复子节点；深度不得越限。
  const ordDiags: TreeDiagnostic[] = [];
  for (const id of ids) {
    const n = tree["raw"](id);
    if (n === undefined) continue;
    const seen = new Set<string>();
    for (const c of n.children) {
      if (seen.has(c)) {
        ordDiags.push(
          td("ORDER_STABILITY_VIOLATION", `节点 ${id} 的子列表中 ${c} 出现多次`, "重复子节点会让命中测试命中同一控件两次", id),
        );
      }
      seen.add(c);
      const chain = tree.ancestors(c);
      if (chain.ok && chain.value.length - 1 > MAX_TREE_DEPTH) {
        ordDiags.push(
          td(
            "DEPTH_LIMIT_EXCEEDED",
            `节点 ${c} 的深度 ${chain.value.length - 1} 超过上限 ${MAX_TREE_DEPTH}`,
            "insert 时已按深度拒；此处说明树是绕过操作接口被改的",
            c,
          ),
        );
      }
    }
  }
  reports.push({ invariantId: "stable-order", passed: ordDiags.length === 0, diagnostics: ordDiags });

  return tok(reports);
}

// ════════════════════════════════════════════════════════════════════════════
// §8 域级自检（零运行时契约层 + 事件/状态枚举机检）
// ════════════════════════════════════════════════════════════════════════════

/** 自检结论。 */
export interface SelfCheckReport {
  /** 诊断码→域级码的映射表是否完备。 */
  readonly codeOwnershipComplete: boolean;
  /** 属性键枚举与守卫表是否一致。 */
  readonly propertyKeysConsistent: boolean;
  /** 事件类型枚举与守卫表是否一致。 */
  readonly eventTypesConsistent: boolean;
  /** 视觉状态枚举与守卫表是否一致。 */
  readonly visualStatesConsistent: boolean;
  /** 状态机优先级是否覆盖 32 种信号组合且无非法态。 */
  readonly stateMachineTotal: boolean;
  /** 三不变量声明条数（应为 3）。 */
  readonly invariantCount: number;
  /** 全部失败项（空数组表示全绿）。 */
  readonly failures: readonly TreeDiagnostic[];
}

/** 域级自检：把本条的封闭集与映射表逐条机检一遍。 */
export function selfCheck(): TreeOutcome<SelfCheckReport> {
  const failures: TreeDiagnostic[] = [];

  //① 诊断码映射完备性：树码全集与映射表键集必须完全相等。
  const mappedKeys = Object.keys(TREE_CODE_OWNERSHIP).sort();
  const declared = [...TREE_DIAG_CODES].sort();
  const codeOwnershipComplete =
    mappedKeys.length === declared.length &&
    mappedKeys.every((k, i) => k === declared[i]);
  if (!codeOwnershipComplete) {
    const missing = declared.filter((d) => !mappedKeys.includes(d));
    const extra = mappedKeys.filter((k) => !declared.includes(k as TreeOnlyDiagCode));
    failures.push(
      td(
        "TREE_SELFCHECK_FAILED",
        `诊断码映射表不完备：未登记=[${missing.join(",")}]，多余=[${extra.join(",")}]`,
        "两表必须逐项相等；新增树码时同步登记归属",
      ),
    );
  }

  // ② 属性键枚举一致性（类型是编译期约束，值表是运行期约束，须互校）。
  const propertyKeysConsistent = PROPERTY_KEYS.length === 13;
  if (!propertyKeysConsistent) {
    failures.push(
      td(
        "TREE_SELFCHECK_FAILED",
        `属性键表长度异常：${PROPERTY_KEYS.length}（应为13）`,
        "PropertyKey 类型与 PROPERTY_KEYS 值表必须同步增删",
      ),
    );
  }

  // ③ 事件类型枚举一致性。
  const eventTypesConsistent = EVENT_TYPES.length === 10;
  if (!eventTypesConsistent) {
    failures.push(
      td(
        "TREE_SELFCHECK_FAILED",
        `事件类型表长度异常：${EVENT_TYPES.length}（应为 10）`,
        "EventType 类型与 EVENT_TYPES 值表必须同步增删",
      ),
    );
  }

  // ④ 视觉状态枚举一致性。
  const visualStatesConsistent = VISUAL_STATES.length === 8;
  if (!visualStatesConsistent) {
    failures.push(
      td(
        "TREE_SELFCHECK_FAILED",
        `视觉状态表长度异常：${VISUAL_STATES.length}（应为 8）`,
        "VisualState 类型与 VISUAL_STATES 值表必须同步增删",
      ),
    );
  }

  // ⑤ 状态机穷举：2^5 = 32 种信号组合全跑，结果必须都在枚举内，
  //    且「disabled 恒压过其他信号」这条纪律必须成立（禁用控件不响应 hover）。
  const combos = 32;
  let legal = 0;
  let disabledDominates = true;
  for (let mask = 0; mask < combos; mask += 1) {
    const sig: StateSignals = {
      hovered: (mask & 1) !== 0,
      pressed: (mask & 2) !== 0,
      focused: (mask & 4) !== 0,
      disabled: (mask & 8) !== 0,
      active: (mask & 16) !== 0,
    };
    const st = resolveVisualState(sig);
    if ((VISUAL_STATES as readonly string[]).includes(st)) legal += 1;
    else {
      failures.push(
        td("STATE_INVALID", `信号组合 ${mask} 求出非法状态 ${st}`, "状态机必须只产出枚举内的值"),
      );
    }
    if (sig.disabled && !st.startsWith("disabled")) disabledDominates = false;
  }
  const stateMachineTotal = legal === combos && disabledDominates;
  if (!stateMachineTotal) {
    failures.push(
      td(
        "STATE_INVALID",
        `状态机穷举未通过：合法 ${legal}/${combos}，disabled优先=${String(disabledDominates)}`,
        "resolveVisualState 必须穷举合法且 disabled 压过一切信号",
      ),
    );
  }

  // ⑥ 三不变量声明条数。
  const invariantCount = TREE_INVARIANTS.length;
  if (invariantCount !== 3) {
    failures.push(
      td("TREE_SELFCHECK_FAILED", `不变量声明条数为 ${invariantCount}（应为 3）`, "单亲/无环/序稳定三条不可增删"),
    );
  }

  return tok(
    {
      codeOwnershipComplete,
      propertyKeysConsistent,
      eventTypesConsistent,
      visualStatesConsistent,
      stateMachineTotal,
      invariantCount,
      failures,
    },
    failures,
  );
}
