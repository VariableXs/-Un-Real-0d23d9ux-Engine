/**
 * VE-F0413 · 条件编译求值（VE-C 域 · 着色器系统 · 词法组 C01 第 13 项）
 * ---------------------------------------------------------------------------
 * 职责定位：条件编译指令求值—— 表达式求值（整型常量语义、未定义宏按假处理
 * **显性**）、分支选择（ifdef / ifndef / if / elif / else 全族）、嵌套栈管理
 * （未闭合报错**逐层**指向各层开指令）、求值结果记录供跳过段快速掠过。
 *
 * 本条只做「条件编译这一个动作」：给定一串已切行的源行与一张宏表，回答两问——
 *   问一：每一行是否进入代码流（active）？
 *   问二：不进入的那些行，为什么不进入（哪个分支判定把门关了）？
 * 词法切分（行延续归并、指令识别）由 F0411 提供，本条消费其产物；宏展开与宏表
 * 由 F0412 提供，本条只读不写；include 协同由 F0414 承接，本条输出「跳过段」
 * 记录作为其快速掠过的输入。
 *
 * 为什么「跳过段只做词法」必须是显式声明而不是实现细节（判据四·语义显性）：
 *   一段被 `#if 0` 关掉的代码里可以有任何东西——包括语法错误、包括引用了尚未
 *   声明的类型、包括为别的后端写的分支。它**不应该**产生任何诊断。但「不报」
 *   极易被实现成「懒得看」：一旦跳过扫描器顺手做了词法或语法分析，被关掉的
 *   代码就会疯狂刷错误，而作者无能为力（他改不了那段代码——那段代码根本不参与
 *   编译）。所以本条把跳过段的处理面**钉死成三件事**且仅三件：
 *     ① 行首指令识别（为了配平嵌套）；
 *     ② 条件指令的压栈/弹栈记账（为了不错过 #endif）；
 *     ③ 段边界与原因记录（为了 F0414 快扫与诊断回溯）。
 *   任何超出的行为都被 `SkippedSegment.policy` 显式登记，越界即断言失败。
 *   对称地：跳过段里**不诊断**，但也**不假装读过**——`stats.suppressedInSkipped`
 *   如实记账「本可以报但按语义故意不报」的条数。
 *
 * 「整型常量语义」（判据一）为什么必须精确到 bit：
 *   条件编译的产物是要送进后端的代码，送错了就是「该编译的分支没编、不该编的
 *   编了」，且往往在真机上才炸。所以本条用 `bigint` 承载 64 位整型、带符号性与
 *   位宽三个维度，**全程不丢精度**，并在每一处可能溢出的运算上做**范围断言**
 *   （而非 C 那种「溢出后静默回绕」）。理由：静默回绕是最难查的一类 bug——
 *   `#if (1 << 31) > 0` 在 32 位有符号下应该是假，回绕后成真，于是整段反编译，
 *   作者却什么线索都拿不到。本条的立场是：**宁可报错，不许静默回绕**。
 *
 * 零静默纪律：未定义宏→ 0 并产出 note（不是无声当假）；未闭合 → 逐层报错；
 *   else 多重 → 报错；表达式含非整型 → 报错且带类型说明；除零 → 报错；
 *   移位越界 → 报错；解析深度/嵌套深度超限 → 报错。
 *   本模块不抛异常、不吞诊断、无静默分支、无全局可变状态、零 DOM 依赖。
 *
 * 判据：整型语义、嵌套栈、跳过快扫、语义显性。
 * 交接说明：纯契约层。可在浏览器 / Worker / Node 校验脚本中原样引入。
 *         上游消费 F0411 的行切分产物与 F0412 的宏表视图（见 §9 HANDOFF）。
 *         下游 F0414（include 解析）消费本条的 `skipped` 段记录做快速掠过。
 */

// ════════════════════════════════════════════════════════════════════════════
// §0 诊断基础设施（零静默第一层；与 F1602 / F1801 同纪律，此处自包含不跨条 import）
// ════════════════════════════════════════════════════════════════════════════

/** 严重度三级（与 F0416 词法错误报告的分级对齐；错误 / 警告 / 注记）。 */
export type Severity = "error" | "warning" | "note";

/**
 * 诊断码：每种拒绝独立可检索，**绝不合并成通用错误**。
 * 纪律：处置方向相反的状态不得共用码（哈希「待补」≠ 漂移，同理
 * 「表达式非法」与「跳过段故意不报」是相反处置，必须分码）。
 */
export type DiagCode =
  // ── 表达式求值族（判据一：整型语义）──
  /** 表达式为空（`#if` 后无内容）。 */
  | "EXPR_EMPTY"
  /** 表达式结尾有多余记号（解析完主表达式后仍有残渣）。 */
  | "EXPR_TRAILING_GARBAGE"
  /** 圆括号不配对：缺右括号。 */
  | "EXPR_UNCLOSED_LPAREN"
  /** 多出一个右括号。 */
  | "EXPR_UNEXPECTED_RPAREN"
  /** 表达式含非整型操作数（浮点/字符串/向量等），带类型说明。 */
  | "EXPR_NOT_INTEGER"
  /** 除以零 / 取模零。 */
  | "EXPR_DIV_ZERO"
  /** 移位量非法（负数或 ≥ 位宽）。 */
  | "EXPR_SHIFT_OUT_OF_RANGE"
  /** 整型运算溢出（显式报错，不静默回绕）。 */
  | "EXPR_OVERFLOW"
  /** 数值字面量非法（尾缀错、进制错、字符常量转义错）。 */
  | "EXPR_BAD_LITERAL"
  /** 三目运算符中段缺省（GNU`,` 扩展不支持，显式拒绝而非猜）。 */
  | "EXPR_TERNARY_MIDDLE_MISSING"
  /** 表达式解析嵌套过深（递归护栏）。 */
  | "EXPR_TOO_DEEP"
  /** 记号化失败（未闭合注释、非法字符等词法面问题）。 */
  | "EXPR_LEX_FAILED"
  /** 指令体超长（超MAX_DIRECTIVE_BODY；与「表达式为空」处置方向不同，单独成码）。 */
  | "EXPR_BODY_TOO_LONG"
  // ── 分支选择族（判据二：全族齐备）──
  /** 条件指令缺表达式（`#ifdef` 未给宏名等）。 */
  | "COND_DIRECTIVE_MISSING_OPERAND"
  /** 同一层出现第二个 `#else`。 */
  | "COND_ELSE_AFTER_ELSE"
  /** `#elif` 出现在 `#else` 之后。 */
  | "COND_ELIF_AFTER_ELSE"
  /** `#else` / `#elif` / `#endif` 没有配对的 `#if`。 */
  | "COND_UNMATCHED_DIRECTIVE"
  /** 指令名不在条件编译族内。 */
  | "COND_UNKNOWN_DIRECTIVE"
  /** 指令体未识别（无法判定归属）。 */
  | "COND_BODY_UNPARSED"
  // ── 嵌套栈族（判据二：嵌套栈）──
  /** 条件栈在流末仍未清空（逐层报错，每层一条）。 */
  | "COND_UNCLOSED"
  /** 嵌套深度超上限。 */
  | "COND_NESTING_TOO_DEEP"
  // ── 语义显性族（判据四：未定义宏不无声当假）──
  /** 标识符未定义 → 按 0 处理，同时产出本note（显性而非静默）。 */
  | "COND_UNDEFINED_IDENT_AS_ZERO"
  /** 跳过段收尾时仍未闭合（段记录 endLine = -1）。 */
  | "SKIP_SEGMENT_UNTERMINATED";

/** 位置三元式：文件名 + 行列 + 字节偏移（F0416 消费；此处产出入参结构）。 */
export interface SourceSpan {
  readonly file: string;
  readonly startLine: number;
  readonly startCol: number;
  readonly endLine: number;
  readonly endCol: number;
  readonly startOffset: number;
  readonly endOffset: number;
}

/** 诊断条目：三要素齐备（发生了什么 / 为什么 / 下一步）。 */
export interface Diagnostic {
  readonly code: DiagCode;
  readonly severity: Severity;
  readonly message: string;
  readonly hint: string;
  readonly span: SourceSpan | null;
  /** 双侧定位（错误点 + 责任点，如 `#endif` 指向对应 `#if`）。 */
  readonly related: readonly SourceSpan[];
}

/** 结果封装：永不抛异常，成功也携带诊断（note 也要带出去，不静默吞）。 */
export type Outcome<T> =
  | { readonly ok: true; readonly value: T; readonly diagnostics: readonly Diagnostic[] }
  | {
      readonly ok: false;
      readonly code: DiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly Diagnostic[];
    };

export function ok<T>(value: T, diagnostics: readonly Diagnostic[] = []): Outcome<T> {
  return { ok: true, value, diagnostics };
}

export function fail<T>(
  code: DiagCode,
  message: string,
  hint: string,
  diagnostics: readonly Diagnostic[] = [],
): Outcome<T> {
  return { ok: false, code, message, hint, diagnostics };
}

/** 诊断袋：可追加、可按严重度计数、错误优先返回 Outcome。 */
export class DiagBag {
  private readonly items: Diagnostic[] = [];

  add(d: Diagnostic): void {
    this.items.push(d);
  }

  error(code: DiagCode, message: string, hint: string, span: SourceSpan | null, related: readonly SourceSpan[] = []): void {
    this.items.push({ code, severity: "error", message, hint, span, related });
  }

  warn(code: DiagCode, message: string, hint: string, span: SourceSpan | null, related: readonly SourceSpan[] = []): void {
    this.items.push({ code, severity: "warning", message, hint, span, related });
  }

  note(code: DiagCode, message: string, hint: string, span: SourceSpan | null, related: readonly SourceSpan[] = []): void {
    this.items.push({ code, severity: "note", message, hint, span, related });
  }

  get all(): readonly Diagnostic[] {
    return this.items;
  }

  get errorCount(): number {
    return this.items.filter((d) => d.severity === "error").length;
  }

  get warningCount(): number {
    return this.items.filter((d) => d.severity === "warning").length;
  }

  get noteCount(): number {
    return this.items.filter((d) => d.severity === "note").length;
  }

  get hasError(): boolean {
    return this.errorCount > 0;
  }

  /** 首个错误（无错误则null）；用于 Outcome.fail 的代表错误。 */
  firstError(): Diagnostic | null {
    for (const d of this.items) if (d.severity === "error") return d;
    return null;
  }

  /** 有错则 fail（携带首个错误三要素），否则 ok（携带全部诊断含 note）。 */
  seal<T>(value: T): Outcome<T> {
    const e = this.firstError();
    if (e === null) return ok(value, this.items);
    return fail<T>(e.code, e.message, e.hint, this.items);
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §1 基础类型、常量与位置构造
// ════════════════════════════════════════════════════════════════════════════

/** 整型位宽：VE-Shade 条件编译的整型常量语义为 32/64 双档（对齐后端）。 */
export type IntWidth = 32 | 64;

/** 整型常量值：全程 bigint 承载，**不丢精度**（判据一）。 */
export interface IntValue {
  /** 规范化后的数值（有符号时为负数）。 */
  readonly value: bigint;
  /** 是否无符号（影响除法/比较/移位的常规算术转换）。 */
  readonly unsigned: boolean;
  /** 位宽档。 */
  readonly width: IntWidth;
}

export const DEFAULT_INT_WIDTH: IntWidth = 32;
export const MAX_INT_WIDTH: IntWidth = 64;

/** 条件栈嵌套深度上限（边界防护，超限报错而非爆栈）。 */
export const MAX_COND_NESTING = 256;
/** 表达式解析递归深度上限（三目 + 括号链的护栏）。 */
export const MAX_EXPR_DEPTH = 64;
/** 单条指令体的最大字符数（防超长行拖垮解析器；超限显式报错）。 */
export const MAX_DIRECTIVE_BODY = 8192;

/** 条件编译指令全族（判据二：全族齐备，一个不缺）。 */
export const COND_DIRECTIVES = ["if", "ifdef", "ifndef", "elif", "else", "endif"] as const;
export type CondDirective = (typeof COND_DIRECTIVES)[number];

const COND_DIRECTIVE_SET: ReadonlySet<string> = new Set<string>(COND_DIRECTIVES);

/** 判断某指令名是否属条件编译族。 */
export function isCondDirective(name: string): name is CondDirective {
  return COND_DIRECTIVE_SET.has(name);
}

/**
 * VE-Shade 预定义宏（语言内建，非用户 `#define`）。
 * 显式登记而非隐式特判：`true` / `false` 是现代语言的基本常量，不是「未定义」。
 */
export const PREDEFINED_MACROS: Readonly<Record<string, bigint>> = {
  true: 1n,
  false: 0n,
  __VE_SHADE__: 1n,
};

/** 构造 SourceSpan（行列 + 字节偏移三元式齐备）。 */
export function span(
  file: string,
  startLine: number,
  startCol: number,
  endLine: number,
  endCol: number,
  startOffset: number,
  endOffset: number,
): SourceSpan {
  return { file, startLine, startCol, endLine, endCol, startOffset, endOffset };
}

/** 空位置（无位置信息的诊断用；显式 null 而非伪造 0 行 0 列）。 */
export const NO_SPAN: SourceSpan | null = null;

// ════════════════════════════════════════════════════════════════════════════
// §2 宏表视图（F0412 上游契约，本条只读）
// ════════════════════════════════════════════════════════════════════════════

/** F0412 宏表对外暴露的只读视图（本条的结构化依赖面，不 import 跨条实现）。 */
export interface MacroTableView {
  /** 宏是否存在（`#ifdef` / `defined` 只需存在性，不需取值）。 */
  has(name: string): boolean;
  /** 取宏的整型替换值；非整型替换体返回 null（由调用方报「非整型」）。 */
  valueOf(name: string): bigint | null;
  /** 是否函数式宏（`#if` 中函数式宏不预展开，须按规范裁定拒绝或忽略）。 */
  isFunctionLike(name: string): boolean;
  /** 已登记宏名（供诊断列「合法集」用）。 */
  names(): readonly string[];
}

/** 空宏表（无任何宏；`#ifdef` 全假，`#if 0` 外皆走未定义路径）。 */
export function emptyMacroTable(): MacroTableView {
  return {
    has: () => false,
    valueOf: () => null,
    isFunctionLike: () => false,
    names: () => [],
  };
}

// ════════════════════════════════════════════════════════════════════════════
// §3 整型常量语义（判据一：位精确 +溢出断言，绝不静默回绕）
// ════════════════════════════════════════════════════════════════════════════

/** 取位宽对应的模数 2^width。 */
function modulusOf(width: IntWidth): bigint {
  return 1n << BigInt(width);
}

/** 规范化到目标位宽/符号性：先按位与截断，再按符号性解释。 */
export function normalizeInt(value: bigint, unsigned: boolean, width: IntWidth): IntValue {
  const m = modulusOf(width);
  let v = value % m;
  if (v < 0n) v += m;
  const signBit = 1n << BigInt(width - 1);
  if (!unsigned && v >= signBit) v -= m;
  return { value: v, unsigned, width };
}

/** 该值在目标位宽/符号性下的可表示区间。 */
export function intRange(unsigned: boolean, width: IntWidth): { lo: bigint; hi: bigint } {
  const m = modulusOf(width);
  if (unsigned) return { lo: 0n, hi: m - 1n };
  const signBit = 1n << BigInt(width - 1);
  return { lo: -signBit, hi: signBit - 1n };
}

/** 常规算术转换（C 语义）：任一无符号 → 结果无符号；位宽取两者较大。 */
export function usualArith(a: IntValue, b: IntValue): { unsigned: boolean; width: IntWidth } {
  const unsigned = a.unsigned || b.unsigned;
  const width = (a.width >= b.width ? a.width : b.width) as IntWidth;
  return { unsigned, width };
}

/** 按转换规则把某值投到目标符号性/位宽。 */
export function convertTo(v: IntValue, unsigned: boolean, width: IntWidth): IntValue {
  return normalizeInt(v.value, unsigned, width);
}

/** 区间可表示性判定（溢出断言的依据）。 */
export function fitsInt(value: bigint, unsigned: boolean, width: IntWidth): boolean {
  const r = intRange(unsigned, width);
  return value >= r.lo && value <= r.hi;
}

/** 构造布尔型整型（比较/逻辑运算的产物恒为 0/1 的有符号 int）。 */
export function boolInt(b: boolean, width: IntWidth = DEFAULT_INT_WIDTH): IntValue {
  return { value: b ? 1n : 0n, unsigned: false, width };
}

/** 文本化（诊断用；十进制，带 u/L 后缀标注）。 */
export function intToText(v: IntValue): string {
  const suffix = `${v.unsigned ? "u" : ""}${v.width}`;
  return `${v.value.toString(10)}${suffix}`;
}

/** 二元算术结果（含溢出判定）。溢出返回 null 交由调用方报错。 */
export function arith(
  op: "+" | "-" | "*",
  a: IntValue,
  b: IntValue,
): { readonly value: IntValue } | { readonly overflow: true; readonly exact: bigint } {
  const t = usualArith(a, b);
  const x = convertTo(a, t.unsigned, t.width);
  const y = convertTo(b, t.unsigned, t.width);
  const exact = op === "+" ? x.value + y.value : op === "-" ? x.value - y.value : x.value * y.value;
  // 关键分流（C 语义）：**无符号**运算是模 2^width 环绕，**定义良好**，不报溢出；
  // **有符号**溢出才是未定义行为，必须显式拦截（否则 `#if 1u - 2u > 0` 会得到
  // 荒谬的假值，且作者无从察觉）。曾把两者混为一谈，导致 1u-2u 误报溢出。
  if (t.unsigned) {
    return { value: normalizeInt(exact, true, t.width) };
  }
  if (!fitsInt(exact, false, t.width)) {
    return { overflow: true, exact };
  }
  return { value: normalizeInt(exact, false, t.width) };
}

/** 整数除/模（截断向零，C 语义）。除零返回除零标记。 */
export function intDiv(
  op: "/" | "%",
  a: IntValue,
  b: IntValue,
): { readonly value: IntValue } | { readonly divZero: true } {
  const t = usualArith(a, b);
  const x = convertTo(a, t.unsigned, t.width);
  const y = convertTo(b, t.unsigned, t.width);
  if (y.value === 0n) return { divZero: true };
  // 无符号：bigint 的 / 与 % 对正数即无符号语义。
  // 有符号：bigint 的 % 是欧几里得余数（可负），C 要截断向零 —— 必须修正。
  const q = x.value / y.value;
  let r = x.value % y.value;
  if (!t.unsigned && r !== 0n && (r < 0n) !== (x.value < 0n)) r += y.value;
  const exact = op === "/" ? q : r;
  return { value: normalizeInt(exact, t.unsigned, t.width) };
}

/** 移位。左移溢出记为溢出；右移负数按算术移位（C 实现定义，本域显式选算术）。 */
export function shift(
  op: "<<" | ">>",
  a: IntValue,
  sh: IntValue,
): { readonly value: IntValue } | { readonly bad: true; readonly reason: string } {
  if (sh.value < 0n) {
    return { bad: true, reason: `移位量为负（${sh.value.toString(10)}）` };
  }
  const n = sh.value;
  if (n >= BigInt(a.width)) {
    return { bad: true, reason: `移位量 ${n.toString(10)} ≥ 位宽 ${a.width}` };
  }
  if (op === "<<") {
    // C 语义：左移在**无符号**对应类型上进行（模2^width 环绕，定义良好），
    // 再按目标符号性解释。故 `1 << 31`（32 位）= -2147483648 是良定义值，
    // 只有「移位量本身越界」才是错误。此前把结果溢出当错误，误杀了合法表达式。
    const unsignedExact = BigInt.asUintN(a.width, a.value) << n;
    return { value: normalizeInt(unsignedExact, a.unsigned, a.width) };
  }
  // 右移：算术右移（负数保号），与主流后端一致。
  return { value: normalizeInt(a.value >> n, a.unsigned, a.width) };
}

/** 逻辑取反/按位取反/算术取负。 */
export function unary(op: "+" | "-" | "~" | "!", a: IntValue): IntValue {
  if (op === "!") return boolInt(a.value === 0n);
  if (op === "+") return a;
  if (op === "-") return normalizeInt(-a.value, a.unsigned, a.width);
  return normalizeInt(~a.value, a.unsigned, a.width);
}

/** 比较（无符号时有符号负数不参与；本域直接按转换后数值比）。 */
export function compare(op: "<" | ">" | "<=" | ">=", a: IntValue, b: IntValue): IntValue {
  const t = usualArith(a, b);
  const x = convertTo(a, t.unsigned, t.width);
  const y = convertTo(b, t.unsigned, t.width);
  switch (op) {
    case "<":
      return boolInt(x.value < y.value);
    case ">":
      return boolInt(x.value > y.value);
    case "<=":
      return boolInt(x.value <= y.value);
    default:
      return boolInt(x.value >= y.value);
  }
}

/** 相等/不等（== 与 != 不做常规算术转换，只比值与符号无关）。 */
export function equal(op: "==" | "!=", a: IntValue, b: IntValue): IntValue {
  const x = convertTo(a, a.unsigned, a.width);
  const y = convertTo(b, b.unsigned, b.width);
  return boolInt(op === "==" ? x.value === y.value : x.value !== y.value);
}

/** 位运算（& | ^）—— 常规算术转换后逐位算。 */
export function bitwise(op: "&" | "|" | "^", a: IntValue, b: IntValue): IntValue {
  const t = usualArith(a, b);
  const x = convertTo(a, t.unsigned, t.width);
  const y = convertTo(b, t.unsigned, t.width);
  const exact = op === "&" ? x.value & y.value : op === "|" ? x.value | y.value : x.value ^ y.value;
  return normalizeInt(exact, t.unsigned, t.width);
}

// ════════════════════════════════════════════════════════════════════════════
// §4 表达式记号化（词法面；失败显式报错，不猜测）
// ════════════════════════════════════════════════════════════════════════════

export type ExprTokKind = "num" | "ident" | "char" | "str" | "punct" | "eof";

export interface ExprTok {
  readonly kind: ExprTokKind;
  readonly text: string;
  readonly start: number;
  /** num：整型值（已按字面量语义规范化）。 */
  readonly int?: IntValue;
  /** num：是否为浮点字面量（是则整型求值阶段报「非整型」并带类型说明）。 */
  readonly isFloat?: boolean;
  /** num：字面量文本类型说明（诊断用，如 "f32 浮点字面量"）。 */
  readonly typeNote?: string;
  /** char：字符常量的整数值。 */
  readonly charValue?: bigint;
}

const PUNCTUATORS: readonly string[] = [
  "<<=", ">>=", "...",
  "<<", ">>", "<=", ">=", "==", "!=", "&&", "||",
  "+", "-", "*", "/", "%", "&", "|", "^", "~", "!", "<", ">", "?", ":", "(", ")", ",", "#",
];

function isDigit(c: string): boolean {
  return c >= "0" && c <= "9";
}

function isIdentStart(c: string): boolean {
  return (c >= "a" && c <= "z") || (c >= "A" && c <= "Z") || c === "_";
}

function isIdentPart(c: string): boolean {
  return isIdentStart(c) || isDigit(c);
}

function isSpace(c: string): boolean {
  return c === " " || c === "\t" || c === "\r";
}

/** 转义字符 → 整数值（不可解析返回 null）。 */
function escapeValue(c: string, out: { readonly value: bigint }): bigint | null {
  switch (c) {
    case "n":
      return 10n;
    case "t":
      return 9n;
    case "r":
      return 13n;
    case "0":
      return 0n;
    case "a":
      return 7n;
    case "b":
      return 8n;
    case "f":
      return 12n;
    case "v":
      return 11n;
    case "\\":
      return 92n;
    case "'":
      return 39n;
    case '"':
      return 34n;
    case "x": {
      // \xHH 由调用方已消费两位
      void out;
      return null;
    }
    default:
      return null;
  }
}

/** 解析数值字面量：整型（十/十六/八/二进+ 后缀）与浮点（判为非整型）。 */
function parseNumber(text: string, base: number): IntValue | { readonly isFloat: true; readonly note: string } | null {
  // 浮点判别：含'.' 或含 e/E（十进制）或 p/P（十六进制浮点）。
  const lower = text.toLowerCase();
  const hasDot = lower.includes(".");
  const hasExp = lower.startsWith("0x") ? lower.includes("p") : lower.includes("e");
  if (hasDot || hasExp) {
    const note = lower.includes("f") ? "f32 浮点字面量" : hasExp ? "科学计数浮点字面量" : "小数浮点字面量";
    return { isFloat: true, note };
  }
  // 剥后缀
  let i = text.length;
  let unsigned = false;
  let longCount = 0;
  while (i > 0) {
    const c = text.charAt(i - 1).toLowerCase();
    if (c === "u") {
      unsigned = true;
      i -= 1;
    } else if (c === "l") {
      longCount += 1;
      i -= 1;
    } else {
      break;
    }
  }
  const body = text.slice(0, i);
  if (body === "") return null;
  let digits = body;
  if (base === 16) {
    if (!digits.toLowerCase().startsWith("0x")) return null;
    digits = digits.slice(2);
  } else if (base === 8) {
    if (!digits.startsWith("0")) return null;
    digits = digits.slice(1);
  } else if (base === 2) {
    if (!digits.toLowerCase().startsWith("0b")) return null;
    digits = digits.slice(2);
  }
  if (digits === "") return null;
  const width: IntWidth = longCount >= 2 || base === 16 ? MAX_INT_WIDTH : DEFAULT_INT_WIDTH;
  try {
    // 关键：BigInt(str) 只认十进制前缀，必须按进制重建前缀，
    // 否则 0x10 会被当成十进制 10、010 当成 10、0b1011 当成 1011（真实缺陷）。
    // 非法数位（如八进制里的 8）由 BigInt 抛错捕获 → 返回 null → 报 EXPR_BAD_LITERAL。
    const v =
      base === 16
        ? BigInt(`0x${digits}`)
        : base === 8
          ? BigInt(`0o${digits}`)
          : base === 2
            ? BigInt(`0b${digits}`)
            : BigInt(digits);
    // 8/2/16 进制字面量在 C 里自动归无符号；此处显式保留该语义。
    const autoUnsigned = unsigned || base === 8 || base === 2 || base === 16;
    return normalizeInt(v, autoUnsigned, width);
  } catch {
    return null;
  }
}

/**
 * 表达式记号化。返回记号流或失败诊断。
 * 失败一律带 code + message + hint，不返回部分结果（不做「尽力解析」）。
 */
export function tokenizeExpr(
  src: string,
  file: string,
  line: number,
  colBase: number,
): { readonly ok: true; readonly toks: readonly ExprTok[] } | { readonly ok: false; readonly code: DiagCode; readonly message: string; readonly hint: string; readonly span: SourceSpan } {
  const toks: ExprTok[] = [];
  const n = src.length;
  let i = 0;
  while (i < n) {
    const c = src.charAt(i);
    if (isSpace(c)) {
      i += 1;
      continue;
    }
    // 行注释
    if (c === "/" && src.charAt(i + 1) === "/") {
      while (i < n && src.charAt(i) !== "\n") i += 1;
      continue;
    }
    // 块注释（不嵌套，语义与 F0408 一致）
    if (c === "/" && src.charAt(i + 1) === "*") {
      const open = i;
      i += 2;
      let closed = false;
      while (i < n) {
        if (src.charAt(i) === "*" && src.charAt(i + 1) === "/") {
          i += 2;
          closed = true;
          break;
        }
        i += 1;
      }
      if (!closed) {
        return {
          ok: false,
          code: "EXPR_LEX_FAILED",
          message: "块注释未闭合",
          hint: "补齐 */ 或删去孤立的 /*",
          span: span(file, line, colBase + open, line, colBase + n, open, n),
        };
      }
      continue;
    }
    // 数值字面量
    if (isDigit(c) || (c === "." && isDigit(src.charAt(i + 1)))) {
      const start = i;
      if (c === "0" && (src.charAt(i + 1) === "x" || src.charAt(i + 1) === "X")) {
        i += 2;
        while (i < n && /[0-9a-fA-F]/.test(src.charAt(i))) i += 1;
      } else if (c === "0" && (src.charAt(i + 1) === "b" || src.charAt(i + 1) === "B")) {
        i += 2;
        while (i < n && (src.charAt(i) === "0" || src.charAt(i) === "1")) i += 1;
      } else {
        while (i < n && /[0-9]/.test(src.charAt(i))) i += 1;
        if (src.charAt(i) === ".") {
          i += 1;
          while (i < n && /[0-9]/.test(src.charAt(i))) i += 1;
        }
        if (/[eE]/.test(src.charAt(i) ?? "")) {
          i += 1;
          if (src.charAt(i) === "+" || src.charAt(i) === "-") i += 1;
          while (i < n && /[0-9]/.test(src.charAt(i))) i += 1;
        }
      }
      while (i < n && /[0-9a-zA-Z._]/.test(src.charAt(i))) i += 1;
      const text = src.slice(start, i);
      let base = 10;
      const two = text.slice(0, 2).toLowerCase();
      if (two === "0x") base = 16;
      else if (two === "0b") base = 2;
      else if (text.length > 1 && text.charAt(0) === "0") base = 8;
      const parsed = parseNumber(text, base);
      if (parsed === null) {
        return {
          ok: false,
          code: "EXPR_BAD_LITERAL",
          message: `数值字面量非法：${text}`,
          hint: "检查进制前缀（0x/0b/0）、数位与后缀（u/l）是否匹配",
          span: span(file, line, colBase + start, line, colBase + i, start, i),
        };
      }
      if ("isFloat" in parsed) {
        toks.push({ kind: "num", text, start, isFloat: true, typeNote: parsed.note });
      } else {
        toks.push({ kind: "num", text, start, int: parsed });
      }
      continue;
    }
    // 标识符
    if (isIdentStart(c)) {
      const start = i;
      while (i < n && isIdentPart(src.charAt(i))) i += 1;
      toks.push({ kind: "ident", text: src.slice(start, i), start });
      continue;
    }
    // 字符常量
    if (c === "'") {
      const start = i;
      i += 1;
      let value: bigint | null = null;
      let bad = false;
      if (src.charAt(i) === "\\") {
        i += 1;
        const e = src.charAt(i);
        if (e === "x") {
          const hex = src.slice(i + 1, i + 3);
          if (/^[0-9a-fA-F]{2}$/.test(hex)) {
            value = BigInt(parseInt(hex, 16));
            i += 3;
          } else {
            bad = true;
            i += 1;
          }
        } else {
          const v = escapeValue(e, { value: 0n });
          if (v === null) {
            bad = true;
            i += 1;
          } else {
            value = v;
            i += 1;
          }
        }
      } else if (i < n && src.charAt(i) !== "'") {
        value = BigInt(src.codePointAt(i) ?? 0);
        i += 1;
      } else {
        bad = true;
        i += 1;
      }
      if (src.charAt(i) === "'") i += 1;
      else bad = true;
      if (bad || value === null) {
        return {
          ok: false,
          code: "EXPR_BAD_LITERAL",
          message: `字符常量非法：${src.slice(start, i)}`,
          hint: "字符常量须形如 'a' 或 '\\n'，转义后须紧跟收尾单引号",
          span: span(file, line, colBase + start, line, colBase + i, start, i),
        };
      }
      toks.push({ kind: "char", text: src.slice(start, i), start, charValue: value });
      continue;
    }
    // 字符串字面量（`#if` 中非法 → 保留记号交由求值阶段报「非整型」并带类型说明）
    if (c === '"') {
      const start = i;
      i += 1;
      while (i < n && src.charAt(i) !== '"') {
        if (src.charAt(i) === "\\") i += 1;
        i += 1;
      }
      if (i < n) i += 1;
      toks.push({ kind: "str", text: src.slice(start, i), start });
      continue;
    }
    // 标点（最长匹配）
    let matched: string | null = null;
    for (const p of PUNCTUATORS) {
      if (src.startsWith(p, i)) {
        matched = p;
        break;
      }
    }
    if (matched === null) {
      return {
        ok: false,
        code: "EXPR_LEX_FAILED",
        message: `表达式含非法字符：${JSON.stringify(c)}`,
        hint: "条件编译表达式只接受整型常量、标识符、defined 与整型运算符",
        span: span(file, line, colBase + i, line, colBase + i + 1, i, i + 1),
      };
    }
    toks.push({ kind: "punct", text: matched, start: i });
    i += matched.length;
  }
  toks.push({ kind: "eof", text: "", start: n });
  return { ok: true, toks };
}

// ════════════════════════════════════════════════════════════════════════════
// §5 表达式求值器（递归下降；整型常量语义 + 未定义宏显性）
// ════════════════════════════════════════════════════════════════════════════

/** 求值期错误（内部结构；由 `evaluateExpr` 转成三要素诊断）。 */
interface EvalError {
  readonly code: DiagCode;
  readonly message: string;
  readonly hint: string;
  readonly at: number;
}

type EvalResult = { readonly ok: true; readonly value: IntValue } | { readonly ok: false; readonly err: EvalError };

/** 求值上下文：宏表 + 诊断袋 + 记号流游标。 */
class Parser {
  private pos = 0;
  private depth = 0;
  private readonly toks: readonly ExprTok[];
  private readonly macros: MacroTableView;
  private readonly bag: DiagBag;
  private readonly file: string;
  private readonly line: number;
  private readonly colBase: number;
  /** 短路抑制计数（`0 && 未定义(x)` 右侧不产生 note，见 `COND_UNDEFINED_IDENT_AS_ZERO`）。 */
  private muted = 0;

  constructor(toks: readonly ExprTok[], macros: MacroTableView, bag: DiagBag, file: string, line: number, colBase: number) {
    this.toks = toks;
    this.macros = macros;
    this.bag = bag;
    this.file = file;
    this.line = line;
    this.colBase = colBase;
  }

  private peek(offset = 0): ExprTok {
    const idx = this.pos + offset;
    const t = this.toks[idx];
    return t ?? { kind: "eof", text: "", start: 0 };
  }

  private at(text: string): boolean {
    const t = this.peek();
    return (t.kind === "punct" || t.kind === "ident") && t.text === text;
  }

  private advance(): ExprTok {
    const t = this.peek();
    if (this.pos < this.toks.length - 1) this.pos += 1;
    return t;
  }

  private err(code: DiagCode, message: string, hint: string, at: number): { readonly ok: false; readonly err: EvalError } {
    return { ok: false, err: { code, message, hint, at } };
  }

  private spanAt(at: number, len = 1): SourceSpan {
    return span(this.file, this.line, this.colBase + at, this.line, this.colBase + at + len, at, at + len);
  }

  /** 静默note（短路区内抑制未定义宏提示，但记入统计由外部处理）。 */
  private note(code: DiagCode, message: string, hint: string, sp: SourceSpan): void {
    if (this.muted > 0) return;
    this.bag.note(code, message, hint, sp);
  }

  /** 解析入口：expression 后须紧跟 eof。 */
  parseTop(): EvalResult {
    const r = this.parseExpr();
    if (!r.ok) return r;
    const t = this.peek();
    if (t.kind !== "eof") {
      // 多余右括号有专属诊断码（比「残渣」更可检索），不得混用。
      if (t.kind === "punct" && t.text === ")") {
        return this.err("EXPR_UNEXPECTED_RPAREN", "表达式多出一个右括号 `)`", "删去多余右括号，或补上对应的 `(`", t.start);
      }
      return this.err(
        "EXPR_TRAILING_GARBAGE",
        `表达式在${JSON.stringify(t.text)} 处有多余记号`,
        "`#if` 只接受单个整型常量表达式，检查是否漏了运算符或多了括号",
        t.start,
      );
    }
    return r;
  }

  /** expression := conditional */
  private parseExpr(): EvalResult {
    if (this.depth > MAX_EXPR_DEPTH) {
      return this.err("EXPR_TOO_DEEP", `表达式嵌套深度超上限 ${MAX_EXPR_DEPTH}`, "拆分为多个宏或简化括号", this.peek().start);
    }
    this.depth += 1;
    const cond = this.parseOr();
    this.depth -= 1;
    if (!cond.ok) return cond;
    if (!this.at("?")) return cond;
    this.advance();
    // C 允许 `a ?: b`（GNU 扩展，中段缺省）；本域显式拒绝，不猜作者意图。
    if (this.at(":")) {
      return this.err(
        "EXPR_TERNARY_MIDDLE_MISSING",
        "三目运算符中段缺省（`?:` 形式）",
        "写出完整三段 `cond ? a : b`；缺省中段扩展不在本域语义内",
        this.peek().start,
      );
    }
    const mid = this.parseExpr();
    if (!mid.ok) return mid;
    if (!this.at(":")) {
      return this.err("EXPR_TRAILING_GARBAGE", "三目运算符缺 `:`", "补 `:` 与false 分支", this.peek().start);
    }
    this.advance();
    const rhs = this.parseExpr();
    if (!rhs.ok) return rhs;
    // 三目结果类型：常规算术转换（无分支参与求值的短路已由 parse 内部保证）。
    const t = usualArith(cond.value, rhs.value);
    return { ok: true, value: normalizeInt(cond.value.value !== 0n ? mid.value.value : rhs.value.value, t.unsigned, t.width) };
  }

  private parseOr(): EvalResult {
    let left = this.parseAnd();
    if (!left.ok) return left;
    while (this.at("||")) {
      this.advance();
      // 短路：左侧为真则右侧不求值（不报未定义宏 note）。
      const rhs: EvalResult = left.value.value !== 0n ? this.parseAndMuted() : this.parseAnd();
      if (!rhs.ok) return rhs;
      left = { ok: true, value: boolInt(left.value.value !== 0n || rhs.value.value !== 0n) };
    }
    return left;
  }

  private parseAnd(): EvalResult {
    let left = this.parseBitOr();
    if (!left.ok) return left;
    while (this.at("&&")) {
      this.advance();
      const rhs: EvalResult = left.value.value === 0n ? this.parseBitOrMuted() : this.parseBitOr();
      if (!rhs.ok) return rhs;
      left = { ok: true, value: boolInt(left.value.value !== 0n && rhs.value.value !== 0n) };
    }
    return left;
  }

  private parseBitOr(): EvalResult {
    let left = this.parseBitXor();
    if (!left.ok) return left;
    while (this.at("|")) {
      this.advance();
      const r = this.parseBitXor();
      if (!r.ok) return r;
      left = { ok: true, value: bitwise("|", left.value, r.value) };
    }
    return left;
  }

  private parseBitXor(): EvalResult {
    let left = this.parseBitAnd();
    if (!left.ok) return left;
    while (this.at("^")) {
      this.advance();
      const r = this.parseBitAnd();
      if (!r.ok) return r;
      left = { ok: true, value: bitwise("^", left.value, r.value) };
    }
    return left;
  }

  private parseBitAnd(): EvalResult {
    let left = this.parseEquality();
    if (!left.ok) return left;
    while (this.at("&")) {
      this.advance();
      const r = this.parseEquality();
      if (!r.ok) return r;
      left = { ok: true, value: bitwise("&", left.value, r.value) };
    }
    return left;
  }

  private parseEquality(): EvalResult {
    let left = this.parseRelational();
    if (!left.ok) return left;
    while (this.at("==") || this.at("!=")) {
      const op = this.advance().text === "==" ? "==" : "!=";
      const r = this.parseRelational();
      if (!r.ok) return r;
      left = { ok: true, value: equal(op, left.value, r.value) };
    }
    return left;
  }

  private parseRelational(): EvalResult {
    let left = this.parseShift();
    if (!left.ok) return left;
    while (this.at("<") || this.at(">") || this.at("<=") || this.at(">=")) {
      const op = this.advance().text as "<" | ">" | "<=" | ">=";
      const r = this.parseShift();
      if (!r.ok) return r;
      left = { ok: true, value: compare(op, left.value, r.value) };
    }
    return left;
  }

  private parseShift(): EvalResult {
    let left = this.parseAdditive();
    if (!left.ok) return left;
    while (this.at("<<") || this.at(">>")) {
      const op = this.advance().text as "<<" | ">>";
      const r = this.parseAdditive();
      if (!r.ok) return r;
      const s = shift(op, left.value, r.value);
      if ("bad" in s) {
        return this.err("EXPR_SHIFT_OUT_OF_RANGE", `移位越界：${s.reason}`, "移位量须在 [0, 位宽) 内", left.value.width === 64 ? 0 : 0);
      }
      left = { ok: true, value: s.value };
    }
    return left;
  }

  private parseAdditive(): EvalResult {
    let left = this.parseMultiplicative();
    if (!left.ok) return left;
    while (this.at("+") || this.at("-")) {
      const op = this.advance().text as "+" | "-";
      const r = this.parseMultiplicative();
      if (!r.ok) return r;
      const a = arith(op, left.value, r.value);
      if ("overflow" in a) {
        return this.err("EXPR_OVERFLOW", `整型溢出：${intToText(left.value)} ${op} ${intToText(r.value)} = ${a.exact.toString(10)} 超出可表示范围`, "改用更宽位宽（l/ul 后缀）或缩小常量", 0);
      }
      left = { ok: true, value: a.value };
    }
    return left;
  }

  private parseMultiplicative(): EvalResult {
    let left = this.parseUnary();
    if (!left.ok) return left;
    while (this.at("*") || this.at("/") || this.at("%")) {
      const op = this.advance().text as "*" | "/" | "%";
      const r = this.parseUnary();
      if (!r.ok) return r;
      if (op === "*") {
        const a = arith("*", left.value, r.value);
        if ("overflow" in a) {
          return this.err("EXPR_OVERFLOW", `整型溢出：${intToText(left.value)} * ${intToText(r.value)} = ${a.exact.toString(10)} 超出可表示范围`, "改用更宽位宽（l/ul 后缀）或缩小常量", 0);
        }
        left = { ok: true, value: a.value };
        continue;
      }
      const d = intDiv(op, left.value, r.value);
      if ("divZero" in d) {
        return this.err("EXPR_DIV_ZERO", `整数${op === "/" ? "除" : "取模"}以零为除数`, "零做除数在条件编译期是错误而非未定义；改写为非零常量或加保护条件", 0);
      }
      left = { ok: true, value: d.value };
    }
    return left;
  }

  private parseUnary(): EvalResult {
    if (this.at("!") || this.at("~") || this.at("-") || this.at("+")) {
      const op = this.advance().text as "!" | "~" | "-" | "+";
      const operand = this.parseUnary();
      if (!operand.ok) return operand;
      return { ok: true, value: unary(op, operand.value) };
    }
    return this.parsePrimary();
  }

  private parsePrimary(): EvalResult {
    const t = this.peek();
    if (t.kind === "eof") {
      return this.err("EXPR_TRAILING_GARBAGE", "表达式在此处提前结束", "补齐操作数或运算符", t.start);
    }
    if (t.kind === "num") {
      this.advance();
      if (t.isFloat === true) {
        return this.err(
          "EXPR_NOT_INTEGER",
          `条件编译只接受整型常量表达式，遇${t.typeNote ?? "浮点字面量"}：${t.text}`,
          "整型语义是本域硬约束（送错分支即后端行为错误）；把浮点比较移到运行期或改用整数阈值",
          t.start,
        );
      }
      if (t.int === undefined) {
        return this.err("EXPR_BAD_LITERAL", `数值字面量不可表示：${t.text}`, "检查数值是否超出可表示范围", t.start);
      }
      return { ok: true, value: t.int };
    }
    if (t.kind === "char") {
      this.advance();
      const v = t.charValue ?? 0n;
      return { ok: true, value: normalizeInt(v, false, DEFAULT_INT_WIDTH) };
    }
    if (t.kind === "str") {
      this.advance();
      return this.err(
        "EXPR_NOT_INTEGER",
        `条件编译不接受字符串字面量：${t.text}`,
        "字符串语义属运行期，编译期条件只能是整型；改用整数标志位表达该分支",
        t.start,
      );
    }
    if (t.kind === "punct" && t.text === "(") {
      this.advance();
      if (this.depth > MAX_EXPR_DEPTH) {
        return this.err("EXPR_TOO_DEEP", `括号嵌套深度超上限 ${MAX_EXPR_DEPTH}`, "拆分为多个宏或简化括号", t.start);
      }
      this.depth += 1;
      const inner = this.parseExpr();
      this.depth -= 1;
      if (!inner.ok) return inner;
      if (!this.at(")")) {
        return this.err("EXPR_UNCLOSED_LPAREN", "表达式缺右括号 `)`", "补齐右括号", this.peek().start);
      }
      this.advance();
      return inner;
    }
    if (t.kind === "punct" && t.text === ")") {
      this.advance();
      return this.err("EXPR_UNEXPECTED_RPAREN", "表达式多出一个右括号 `)`", "删去多余右括号", t.start);
    }
    if (t.kind === "ident") {
      this.advance();
      if (t.text === "defined") {
        // defined X / defined(X)：不受宏预展开影响，单独识别。
        let paren = false;
        if (this.at("(")) {
          this.advance();
          paren = true;
        }
        const nameTok = this.peek();
        if (nameTok.kind !== "ident") {
          return this.err("EXPR_TRAILING_GARBAGE", "`defined` 之后须跟宏名", "写成 `defined(FOO)` 或 `defined FOO`", nameTok.start);
        }
        this.advance();
        if (paren) {
          if (!this.at(")")) {
            return this.err("EXPR_UNCLOSED_LPAREN", "`defined(` 缺右括号", "补齐右括号 `)`", this.peek().start);
          }
          this.advance();
        }
        return { ok: true, value: boolInt(this.macros.has(nameTok.text)) };
      }
      if (this.at("(")) {
        // 函数式宏在 `#if` 中须先预展开（F0412 职责）；本条不预展开，显式拒绝而非当标识符。
        return this.err(
          "EXPR_NOT_INTEGER",
          `函数式宏 ${t.text}(...) 不能直接出现在条件表达式中`,
          "函数式宏须由 F0412 展开引擎预展开为整型表达式；请改用对象宏表达该条件",
          t.start,
        );
      }
      const pre = PREDEFINED_MACROS[t.text];
      if (pre !== undefined) {
        return { ok: true, value: normalizeInt(pre, false, DEFAULT_INT_WIDTH) };
      }
      if (this.macros.has(t.text)) {
        if (this.macros.isFunctionLike(t.text)) {
          return this.err(
            "EXPR_NOT_INTEGER",
            `函数式宏 ${t.text} 在条件表达式中缺调用括号`,
            "函数式宏须写成 ${t.text}(实参) 并由 F0412 预展开；此处按整型语义无法求值",
            t.start,
          );
        }
        const v = this.macros.valueOf(t.text);
        if (v === null) {
          return this.err(
            "EXPR_NOT_INTEGER",
            `宏 ${t.text} 的替换体非整型，无法参与条件编译求值`,
            "条件编译只接受整型宏；把非整型判定改为运行期分支或另定义一个整型开关宏",
            t.start,
          );
        }
        return { ok: true, value: normalizeInt(v, false, DEFAULT_INT_WIDTH) };
      }
      // 未定义宏 → 按0（假）处理，但**显性**产出 note（判据四）。
      this.note(
        "COND_UNDEFINED_IDENT_AS_ZERO",
        `未定义标识符 ${t.text} 在条件表达式中按0（假）处理`,
        "若依赖外部构建系统注入该宏，请确认注入时机早于本文件；否则该分支恒不生效",
        this.spanAt(t.start, t.text.length),
      );
      return { ok: true, value: normalizeInt(0n, false, DEFAULT_INT_WIDTH) };
    }
    return this.err("EXPR_TRAILING_GARBAGE", `表达式中出现不可用记号：${t.text}`, "条件编译表达式只支持整型运算", t.start);
  }

  // ── 静默变体：短路右侧使用，抑制 note 但不改变求值语义 ──
  private parseAndMuted(): EvalResult {
    this.muted += 1;
    const r = this.parseAnd();
    this.muted -= 1;
    return r;
  }

  private parseBitOrMuted(): EvalResult {
    this.muted += 1;
    const r = this.parseBitOr();
    this.muted -= 1;
    return r;
  }
}

/** 求值统计（性能判据的可机检证据：求值 O(表达式长度)）。 */
export interface ExprStats {
  readonly tokenCount: number;
  readonly bodyLength: number;
  readonly undefinedIdents: number;
}

/**
 * 表达式求值结果：判别联合 + 统计。
 * 显式声明为两臂判别联合（而非 `Outcome<IntValue> & { stats }` 交叉类型）——
 * 交叉类型在展开后 `ok` 判别式仍成立但 TS 无法窄化，调用方读 `.code` 会被
 * 误判为「属性不存在」。此处显式列出两臂，窄化即可用（零强转纪律）。
 */
export type ExprOutcome =
  | { readonly ok: true; readonly value: IntValue; readonly diagnostics: readonly Diagnostic[]; readonly stats: ExprStats }
  | {
      readonly ok: false;
      readonly code: DiagCode;
      readonly message: string;
      readonly hint: string;
      readonly diagnostics: readonly Diagnostic[];
      readonly stats: ExprStats;
    };

/** 把 DiagBag 的 Outcome 封成带统计的 ExprOutcome（唯一构造入口，杜绝强转）。 */
function sealExpr(bag: DiagBag, value: IntValue, tokenCount: number, bodyLength: number): ExprOutcome {
  const undefinedIdents = bag.all.filter((d) => d.code === "COND_UNDEFINED_IDENT_AS_ZERO").length;
  const stats: ExprStats = { tokenCount, bodyLength, undefinedIdents };
  const sealed = bag.seal<IntValue>(value);
  if (sealed.ok) {
    return { ok: true, value: sealed.value, diagnostics: sealed.diagnostics, stats };
  }
  return { ok: false, code: sealed.code, message: sealed.message, hint: sealed.hint, diagnostics: sealed.diagnostics, stats };
}

/** 求值入口：整型常量表达式 → ExprOutcome。 */
export function evaluateExpr(
  body: string,
  macros: MacroTableView,
  file: string,
  line: number,
  colBase: number,
): ExprOutcome {
  const bag = new DiagBag();
  const fallback = (): IntValue => normalizeInt(0n, false, DEFAULT_INT_WIDTH);
  if (body.trim() === "") {
    bag.error("EXPR_EMPTY", "条件指令缺表达式", "写成 `#if <整型常量表达式>`，例如 `#if VERSION >= 2`", NO_SPAN);
    return sealExpr(bag, fallback(), 0, body.length);
  }
  if (body.length > MAX_DIRECTIVE_BODY) {
    bag.error(
      "EXPR_BODY_TOO_LONG",
      `指令体长度 ${body.length.toString(10)} 超上限 ${MAX_DIRECTIVE_BODY.toString(10)}`,
      "拆分为多个宏定义再引用；超长指令体通常意味着该用宏表达",
      span(file, line, colBase, line, colBase + MAX_DIRECTIVE_BODY, 0, MAX_DIRECTIVE_BODY),
    );
    return sealExpr(bag, fallback(), 0, body.length);
  }
  const lex = tokenizeExpr(body, file, line, colBase);
  if (!lex.ok) {
    bag.error(lex.code, lex.message, lex.hint, lex.span);
    return sealExpr(bag, fallback(), 0, body.length);
  }
  const p = new Parser(lex.toks, macros, bag, file, line, colBase);
  const r = p.parseTop();
  if (!r.ok) {
    bag.error(
      r.err.code,
      r.err.message,
      r.err.hint,
      span(file, line, colBase + r.err.at, line, colBase + r.err.at + 1, r.err.at, r.err.at + 1),
    );
    return sealExpr(bag, fallback(), lex.toks.length, body.length);
  }
  return sealExpr(bag, r.value, lex.toks.length, body.length);
}

// ════════════════════════════════════════════════════════════════════════════
// §6 条件栈（判据二：嵌套 × 分支状态，压弹 O(1)）
// ════════════════════════════════════════════════════════════════════════════

/** 帧内分支状态机：`#if` → (`#elif`)* → `#else` → `#endif`。 */
export type BranchState = "no-branch-yet" | "taken" | "skipped" | "else-taken" | "else-skipped";

export interface CondFrame {
  /** 嵌套深度（1 起）。 */
  readonly depth: number;
  /** 开指令位置（未闭合报错逐层指向此处）。 */
  readonly openSpan: SourceSpan;
  /** 开指令原文（`#if X > 1` 全形）。 */
  readonly openText: string;
  /** 本帧是否有分支被选中过（`#elif` 短路依据）。 */
  branchTaken: boolean;
  /** 本帧是否已见 `#else`（`#elif` after else 报错依据）。 */
  elseSeen: boolean;
  /** 本层是否处于选中态（父层不活跃时恒假）。 */
  active: boolean;
  /** 开指令求值结果（`#ifdef` 记存在性；供诊断回溯）。开指令求值后回填一次。 */
  openTruth: boolean;
}

export class CondStack {
  private readonly frames: CondFrame[] = [];

  get depth(): number {
    return this.frames.length;
  }

  get top(): CondFrame | null {
    const f = this.frames[this.frames.length - 1];
    return f === undefined ? null : f;
  }

  /** 压栈 O(1)。越限返回 null（调用方报错，不静默截断）。 */
  push(openSpan: SourceSpan, openText: string, openTruth: boolean, active: boolean): CondFrame | null {
    if (this.frames.length >= MAX_COND_NESTING) return null;
    const frame: CondFrame = {
      depth: this.frames.length + 1,
      openSpan,
      openText,
      branchTaken: active,
      elseSeen: false,
      active,
      openTruth,
    };
    this.frames.push(frame);
    return frame;
  }

  /** 弹栈 O(1)，返回被弹出的帧（用于 `#endif` 记账与诊断）。 */
  pop(): CondFrame | null {
    const f = this.frames.pop();
    return f === undefined ? null : f;
  }

  /** 栈快照（自检与诊断消费；拷贝 O(depth)，不参与主流程复杂度）。 */
  snapshot(): readonly CondFrame[] {
    return this.frames.map((f) => ({ ...f }));
  }
}

// ════════════════════════════════════════════════════════════════════════════
// §7 行模型与跳过段记录（判据三：跳过快扫 + 判据四：语义显性）
// ════════════════════════════════════════════════════════════════════════════

/** F0411 产出的源行（本条的输入单元）。 */
export interface SourceLine {
  /** 1 基行号。 */
  readonly line: number;
  /** 行全文（已由 F0411 完成行延续归并，故此处不含续行反斜杠）。 */
  readonly text: string;
  /** 是否指令行（`#` 在首个非空白位）。 */
  readonly isDirective: boolean;
  /** 指令名（小写，无`#`）；非指令行为空串。 */
  readonly directiveName: string;
  /** 指令体（指令名之后的部分，已 trim 首空白）。 */
  readonly directiveBody: string;
  /** 该行在本文件中的字节偏移。 */
  readonly offset: number;
}

/** 跳过原因（谁把门关了）。 */
export type SkipReason = "if-false" | "if-unknown" | "elif-after-taken" | "else-after-taken" | "nested-inactive";

/**
 * 跳过段记录（供 F0414 快扫与诊断回溯；判据三的产物）。
 *
 * 边界规则（下游 F0414 快扫依赖此契约，不得随意改动）：
 *   · `startLine` = 关门判定所在的那条指令行（故startLine === causeLine，
 *     段自带锚点，诊断不必反查）；
 *   · `endLine`   = **最后一条被排除的行**；
 *   · 收尾的 `#endif` **不计入本段**——它是被预处理器正常消费的指令，不是被排除
 *     的代码。故`#if 0 / a / b / #endif` 的段是 `[1,3]`，不是 `[1,4]`。
 *   · 未闭合时 `endLine = -1`。
 */
export interface SkippedSegment {
  /** 段起始行（含）= 关门指令行。 */
  readonly startLine: number;
  /** 段结束行（含，最后一条被排除的行）；流末仍未闭合时为 -1。 */
  readonly endLine: number;
  /** 关门的判定所在行。 */
  readonly causeLine: number;
  /** 关门判定的原文。 */
  readonly causeText: string;
  readonly reason: SkipReason;
  /** 段起始时的嵌套深度。 */
  readonly depth: number;
  /** 段内行数（含指令行）。 */
  readonly lineCount: number;
  /**
   * 跳过策略（判据四·显性登记）：本段只做三件事——
   * 行首指令识别 / 条件压弹记账 / 段边界记录。**不做**词法语法分析，
   * 因此段内语法错误一律不报（`suppressedDiagnostics` 如实记账）。
   */
  readonly policy: "lexical-only";
  /** 本段内「本可诊断但按语义故意不报」的条数。 */
  readonly suppressedDiagnostics: number;
}

/** 逐指令判定记录（求值结果记录；判据一/二的产物）。 */
export interface BranchDecision {
  readonly line: number;
  readonly directive: CondDirective;
  /** 指令体原文。 */
  readonly body: string;
  /** 判定依据：`ifdef` 记宏存在性，`if` 记整型值。 */
  readonly evidence: string;
  /** 求值结果（true = 该分支被选中）。 */
  readonly truth: boolean;
  /** 该指令之后的有效态。 */
  readonly activeAfter: boolean;
  /** 判定所在嵌套深度。 */
  readonly depth: number;
  readonly span: SourceSpan;
}

/** 统计（性能判据的可机检证据）。 */
export interface CondStats {
  /** 总扫描行数（O(源行数)）。 */
  readonly linesScanned: number;
  /** 条件指令数。 */
  readonly directives: number;
  /** 实际求值的表达式条数（跳过段内恒为 0 —— 这是「跳过快扫」的机检证据）。 */
  readonly expressionsEvaluated: number;
  /** 跳过段内被抑制的诊断条数。 */
  readonly suppressedInSkipped: number;
  /** 跳过段数。 */
  readonly skippedSegments: number;
  /** 跳过总行数。 */
  readonly skippedLines: number;
  /** 峰值嵌套深度。 */
  readonly maxDepth: number;
  /** 表达式求值累计字符数（O(表达式长度) 之和）。 */
  readonly exprChars: number;
}

// ════════════════════════════════════════════════════════════════════════════
// §8 主流程：条件编译求值
// ════════════════════════════════════════════════════════════════════════════

export interface CondOptions {
  /** 虚拟文件名（诊断用）。 */
  readonly file?: string;
}

/**
 * 全量求值报告：**恒带结果**，不论有无错误。
 * 为什么需要它：`Outcome<T>` 在失败时丢弃 value，但本函数的失败态恰恰携带
 * 下游最需要的东西——残留条件栈（逐层未闭合定位）与跳过段记录（F0414 快扫、
 * F0416 报告都要用）。若只给Outcome，调用方在错误路径上就什么都拿不到，
 * 只能把诊断文本当字符串猜结构。故本条显式提供「报告形态」：
 *   · 工具链/报告类消费（F0414/F0416）→ 用本报告形态（错误路径也有数据）；
 *   · 断言类消费（自检）→ 用 `evaluateConditionals` 的 Outcome 形态。
 */
export interface CondReport {
  /** 无 error 级诊断时为 true。 */
  readonly ok: boolean;
  /** 恒存在：含activeLines / skipped / decisions / residualStack / stats。 */
  readonly result: CondResult;
  readonly diagnostics: readonly Diagnostic[];
}

export interface CondResult {
  /** 进入代码流的行号（升序）。 */
  readonly activeLines: readonly number[];
  /** 跳过段记录（判据三产物）。 */
  readonly skipped: readonly SkippedSegment[];
  /** 逐指令判定记录。 */
  readonly decisions: readonly BranchDecision[];
  /** 判定后的条件栈（正常结束时空）。 */
  readonly residualStack: readonly CondFrame[];
  readonly stats: CondStats;
  readonly diagnostics: readonly Diagnostic[];
}

interface MutableSkip {
  startLine: number;
  endLine: number;
  causeLine: number;
  causeText: string;
  reason: SkipReason;
  depth: number;
  lineCount: number;
  suppressed: number;
}

function toSkipSeg(m: MutableSkip): SkippedSegment {
  return {
    startLine: m.startLine,
    endLine: m.endLine,
    causeLine: m.causeLine,
    causeText: m.causeText,
    reason: m.reason,
    depth: m.depth,
    lineCount: m.lineCount,
    policy: "lexical-only",
    suppressedDiagnostics: m.suppressed,
  };
}

/**
 * 条件编译求值主流程。
 * 复杂度：扫描 O(源行数)；压弹 O(1)/指令；求值 O(表达式长度)；跳过段 O(段长) 快扫。
 */
export function evaluateConditionalsReport(
  lines: readonly SourceLine[],
  macros: MacroTableView,
  options: CondOptions = {},
): CondReport {
  const file = options.file ?? "<shader>";
  const bag = new DiagBag();
  const stack = new CondStack();
  const activeLines: number[] = [];
  const decisions: BranchDecision[] = [];
  const skips: SkippedSegment[] = [];
  // 用盒对象持有当前跳过段：beginSkip/endSkip 是闭包，直接用 let current 会被 TS
  // 按控制流窄化误判为 never（闭包内赋值对窄化不可见）。
  const segBox: { cur: MutableSkip | null } = { cur: null };
  let linesScanned = 0;
  let directives = 0;
  let exprEvaluated = 0;
  let suppressed = 0;
  let skippedLines = 0;
  let maxDepth = 0;
  let exprChars = 0;

  /**
   * 记入代码流。**只有非指令行**入流——条件指令由预处理器自身消费完就消失，
   * 绝不能流到 F0421 语法解析器（它见到 `#endif` 会当语法错误）。
   * 指令行的行号信息不丢：开/续/终指令都在 `decisions` 与 `skipped` 段里留痕。
   */
  const emitActive = (line: SourceLine): void => {
    if (line.isDirective) return;
    activeLines.push(line.line);
  };

  const parentActive = (): boolean => {
    if (stack.depth === 0) return true;
    const f = stack.top;
    return f === null ? true : f.active;
  };

  /** 开一段跳过（幂等：已在跳过中则只记行数）。 */
  const beginSkip = (line: SourceLine, reason: SkipReason): void => {
    if (segBox.cur !== null) {
      // endLine 恒等于「最近一条被排除的行」——由 beginSkip 维护，
      // 不在 endSkip 里改写（endSkip 触发时那一行是**活跃**行，写进去会
      // 让段边界多含一行，下游 F0414 快扫会连带跳过一条本该保留的指令）。
      segBox.cur.endLine = line.line;
      segBox.cur.lineCount += 1;
      skippedLines += 1;
      return;
    }
    segBox.cur = {
      startLine: line.line,
      endLine: line.line,
      causeLine: line.line,
      causeText: line.text.trim(),
      reason,
      depth: stack.depth,
      lineCount: 1,
      suppressed: 0,
    };
    skippedLines += 1;
  };

  /** 结束当前跳过段（只封口，不再动 endLine）。 */
  const endSkip = (): void => {
    if (segBox.cur === null) return;
    skips.push(toSkipSeg(segBox.cur));
    segBox.cur = null;
  };

  for (const line of lines) {
    linesScanned += 1;
    const inSkip = parentActive() === false;
    const lineSpan = span(file, line.line, 1, line.line, line.text.length + 1, line.offset, line.offset + line.text.length);

    // ── 非指令行：活跃则入代码流，跳过则并入段（不做任何分析）──
    if (!line.isDirective) {
      if (inSkip) {
        beginSkip(line, "nested-inactive");
        continue;
      }
      if (segBox.cur !== null) endSkip();
      emitActive(line);
      continue;
    }

    const name = line.directiveName;

    // ── 非条件编译指令：在活跃区照常透传（不是本条职责，不判语义）；
    //    在跳过区只做「不处理」记账（判据四：不做词法语法分析）。
    if (!isCondDirective(name)) {
      if (inSkip) {
        // 跳过段内：本可诊断（未知指令 / define 语法错）但按语义故意不报。
        suppressed += 1;
        if (segBox.cur !== null) segBox.cur.suppressed += 1;
        beginSkip(line, "nested-inactive");
        continue;
      }
      if (segBox.cur !== null) endSkip();
      emitActive(line);
      continue;
    }

    directives += 1;
    const parent = parentActive();

    // ══ 分支开启：#if / #ifdef / #ifndef ══
    if (name === "if" || name === "ifdef" || name === "ifndef") {
      const frame = stack.push(lineSpan, line.text.trim(), false, false);
      if (frame === null) {
        bag.error(
          "COND_NESTING_TOO_DEEP",
          `条件嵌套深度超上限 ${MAX_COND_NESTING.toString(10)}`,
          "用 `#define` 开关折叠嵌套层级（同一开关的组合写成单个整型表达式）",
          lineSpan,
        );
        // 深度超限后不再压栈（避免无界增长），后续指令按不活跃处理。
        suppressed += 1;
        beginSkip(line, "nested-inactive");
        continue;
      }
      if (stack.depth > maxDepth) maxDepth = stack.depth;

      let truth = false;
      let evidence = "";
      if (!parent) {
        //父层已 inactive：本帧不求值（跳过快扫的又一处体现）。
        frame.active = false;
        frame.branchTaken = false;
        evidence = "父层不活跃，未求值";
        suppressed += 1;
        if (segBox.cur !== null) segBox.cur.suppressed += 1;
        beginSkip(line, "nested-inactive");
        decisions.push({ line: line.line, directive: name, body: line.directiveBody, evidence, truth: false, activeAfter: false, depth: frame.depth, span: lineSpan });
        continue;
      }

      if (name === "if") {
        const r = evaluateExpr(line.directiveBody, macros, file, line.line, 1);
        exprEvaluated += 1;
        exprChars += line.directiveBody.length;
        for (const d of r.diagnostics) bag.add(d);
        if (r.ok) {
          truth = r.value.value !== 0n;
          evidence = `${r.value.value.toString(10)}（${intToText(r.value)}）`;
        } else {
          evidence = "求值失败，按假处理并报错";
        }
      } else {
        const macroName = line.directiveBody.trim();
        if (macroName === "" || /\s/.test(macroName)) {
          bag.error(
            "COND_DIRECTIVE_MISSING_OPERAND",
            `\`#${name}\` 缺合法宏名（收到 ${JSON.stringify(line.directiveBody)}）`,
            "写成 `#ifdef FEATURE_NAME`，宏名须为单个标识符",
            lineSpan,
          );
          evidence = "操作数非法";
        } else {
          const present = macros.has(macroName) || PREDEFINED_MACROS[macroName] !== undefined;
          truth = name === "ifdef" ? present : !present;
          evidence = `${macroName} ${present ? "已定义" : "未定义"}`;
        }
      }

      frame.active = truth;
      frame.branchTaken = truth;
      frame.openTruth = truth;
      decisions.push({ line: line.line, directive: name, body: line.directiveBody, evidence, truth, activeAfter: truth, depth: frame.depth, span: lineSpan });
      if (truth) {
        if (segBox.cur !== null) endSkip();
        emitActive(line);
      } else {
        beginSkip(line, name === "if" ? "if-false" : "if-unknown");
      }
      continue;
    }

    // ══ 分支续接：#elif ══
    if (name === "elif") {
      const frame = stack.top;
      if (frame === null) {
        bag.error("COND_UNMATCHED_DIRECTIVE", "`#elif` 没有配对的 `#if`", "补一个 `#if`，或删去多余的 `#elif`", lineSpan);
        suppressed += 1;
        beginSkip(line, "nested-inactive");
        continue;
      }
      if (frame.elseSeen) {
        bag.error(
          "COND_ELIF_AFTER_ELSE",
          "`#elif` 出现在本层 `#else` 之后",
          "`#else` 之后不再接受 `#elif`；把这支合并进 `#else` 之前的分支链",
          lineSpan,
          [frame.openSpan],
        );
        suppressed += 1;
        if (segBox.cur !== null) segBox.cur.suppressed += 1;
        beginSkip(line, "else-after-taken");
        continue;
      }
      if (frame.branchTaken) {
        // 本层已有分支被选中 → 本支恒不选中，**不求值**（短路）。
        frame.active = false;
        suppressed += 1;
        if (segBox.cur !== null) segBox.cur.suppressed += 1;
        beginSkip(line, "elif-after-taken");
        decisions.push({ line: line.line, directive: "elif", body: line.directiveBody, evidence: "本层已有分支被选中，未求值", truth: false, activeAfter: false, depth: frame.depth, span: lineSpan });
        continue;
      }
      const r = evaluateExpr(line.directiveBody, macros, file, line.line, 1);
      exprEvaluated += 1;
      exprChars += line.directiveBody.length;
      for (const d of r.diagnostics) bag.add(d);
      const truth = r.ok && r.value.value !== 0n;
      const evidence = r.ok ? `${r.value.value.toString(10)}（${intToText(r.value)}）` : "求值失败，按假处理并报错";
      frame.active = truth;
      frame.branchTaken = truth;
      decisions.push({ line: line.line, directive: "elif", body: line.directiveBody, evidence, truth, activeAfter: truth, depth: frame.depth, span: lineSpan });
      if (truth) {
        if (segBox.cur !== null) endSkip();
        emitActive(line);
      } else {
        beginSkip(line, "if-false");
      }
      continue;
    }

    // ══ 分支终结：#else ══
    if (name === "else") {
      const frame = stack.top;
      if (frame === null) {
        bag.error("COND_UNMATCHED_DIRECTIVE", "`#else` 没有配对的 `#if`", "补一个 `#if`，或删去多余的 `#else`", lineSpan);
        suppressed += 1;
        beginSkip(line, "nested-inactive");
        continue;
      }
      if (frame.elseSeen) {
        bag.error(
          "COND_ELSE_AFTER_ELSE",
          "同一层出现第二个 `#else`",
          "一层条件编译至多一个 `#else`；把两支合并为一支或拆成两层 `#if`",
          lineSpan,
          [frame.openSpan],
        );
        suppressed += 1;
        if (segBox.cur !== null) segBox.cur.suppressed += 1;
        beginSkip(line, "else-after-taken");
        continue;
      }
      frame.elseSeen = true;
      const priorTaken = frame.branchTaken;
      const truth = !priorTaken;
      frame.active = truth;
      frame.branchTaken = truth;
      decisions.push({ line: line.line, directive: "else", body: "", evidence: priorTaken ? "前序分支已选中" : "前序分支均未选中", truth, activeAfter: truth, depth: frame.depth, span: lineSpan });
      if (truth) {
        if (segBox.cur !== null) endSkip();
        emitActive(line);
      } else {
        beginSkip(line, "else-after-taken");
      }
      continue;
    }

    // ══ 层级终结：#endif ══
    const frame = stack.pop();
    if (frame === null) {
      bag.error("COND_UNMATCHED_DIRECTIVE", "`#endif` 没有配对的 `#if`", "删去多余的 `#endif`，或补上对应层级的 `#if`", lineSpan);
      suppressed += 1;
      beginSkip(line, "nested-inactive");
      continue;
    }
    const nowActive = parentActive();
    if (nowActive) {
      if (segBox.cur !== null) endSkip();
      emitActive(line);
    } else {
      beginSkip(line, "nested-inactive");
    }
  }

  // ── 未闭合：逐层报错，每层一条，指向各自开指令（判据二）──
  for (const frame of stack.snapshot()) {
    bag.error(
      "COND_UNCLOSED",
      `第 ${frame.depth.toString(10)} 层条件编译未闭合（开指令在第 ${frame.openSpan.startLine.toString(10)} 行）`,
      `补上配对的 #endif，或删去开指令 ${frame.openText}`,
      frame.openSpan,
    );
  }
  if (segBox.cur !== null) {
    segBox.cur.endLine = -1;
    bag.error(
      "SKIP_SEGMENT_UNTERMINATED",
      `跳过段自第 ${segBox.cur.startLine.toString(10)} 行起至流末仍未闭合`,
      "这是上面未闭合条件的直接后果；先补 `#endif` 再看本条",
      span(file, segBox.cur.startLine, 1, segBox.cur.startLine, 1, 0, 0),
    );
    skips.push(toSkipSeg(segBox.cur));
    segBox.cur = null;
  }

  const stats: CondStats = {
    linesScanned,
    directives,
    expressionsEvaluated: exprEvaluated,
    suppressedInSkipped: suppressed,
    skippedSegments: skips.length,
    skippedLines,
    maxDepth,
    exprChars,
  };
  const result: CondResult = {
    activeLines,
    skipped: skips,
    decisions,
    residualStack: stack.snapshot(),
    stats,
    diagnostics: bag.all,
  };
  return { ok: !bag.hasError, result, diagnostics: bag.all };
}

/**
 * 求值主流程（Outcome 形态）：无错误时给value，有错误时给首个错误三要素。
 * 需要错误路径上的残留栈/跳过段时请改用 `evaluateConditionalsReport`。
 */
export function evaluateConditionals(
  lines: readonly SourceLine[],
  macros: MacroTableView,
  options: CondOptions = {},
): Outcome<CondResult> {
  const report = evaluateConditionalsReport(lines, macros, options);
  if (report.ok) return ok(report.result, report.diagnostics);
  const e = firstErrorOf(report.diagnostics);
  if (e === null) return ok(report.result, report.diagnostics);
  return fail<CondResult>(e.code, e.message, e.hint, report.diagnostics);
}

/** 取首个 error 级诊断（无则 null）。 */
function firstErrorOf(diagnostics: readonly Diagnostic[]): Diagnostic | null {
  for (const d of diagnostics) if (d.severity === "error") return d;
  return null;
}

// ════════════════════════════════════════════════════════════════════════════
// §9 跨批对接（HANDOFF）
// ════════════════════════════════════════════════════════════════════════════

/** 上游消费面（判据：跨批对接点逐条落实）。 */
export interface UpstreamContract {
  /** F0411：行切分产物（本条输入单元）。 */
  readonly fromF0411: "SourceLine[]（行延续已归并、指令已识别）";
  /** F0412：宏表只读视图（本条只读不写）。 */
  readonly fromF0412: "MacroTableView（has / valueOf / isFunctionLike / names）";
  /** F0408/F0409：不跨条 import 实现，仅约定记号层由 F0411 产出。 */
  readonly sharedLexical: "注释不嵌套语义与运算符表由 F0408/F0409 单点定义";
}

/** 下游产出面。 */
export interface DownstreamContract {
  /** F0414（include 解析）：消费 `skipped` 段记录做快速掠过。 */
  readonly toF0414: "SkippedSegment[]（起止行 + 关门原因 + lexical-only 策略）";
  /** F0416（词法错误报告）：消费 `diagnostics` 三要素 + 双侧定位 related。 */
  readonly toF0416: "Diagnostic[]（含 related 双侧定位）";
  /** F0421（语法解析）：消费 `activeLines` 作为唯一代码流。 */
  readonly toF0421: "activeLines（升序行号）";
}

export const UPSTREAM_CONTRACT: UpstreamContract = {
  fromF0411: "SourceLine[]（行延续已归并、指令已识别）",
  fromF0412: "MacroTableView（has / valueOf / isFunctionLike / names）",
  sharedLexical: "注释不嵌套语义与运算符表由 F0408/F0409 单点定义",
};

export const DOWNSTREAM_CONTRACT: DownstreamContract = {
  toF0414: "SkippedSegment[]（起止行 + 关门原因 + lexical-only 策略）",
  toF0416: "Diagnostic[]（含 related 双侧定位）",
  toF0421: "activeLines（升序行号）",
};

// ════════════════════════════════════════════════════════════════════════════
// §10 自检（判据逐条机检；全部为真运行时断言，非类型断言）
// ════════════════════════════════════════════════════════════════════════════

export interface SelfCheck {
  readonly name: string;
  readonly pass: boolean;
  readonly detail: string;
}

/** 行构造器（自检夹具用；生产路径由 F0411 产出）。 */
function L(line: number, text: string): SourceLine {
  const t = text.trim();
  const hash = t.indexOf("#");
  if (!t.startsWith("#")) {
    return { line, text, isDirective: false, directiveName: "", directiveBody: "", offset: 0 };
  }
  const rest = t.slice(1);
  const m = /^([A-Za-z_][A-Za-z0-9_]*)\s*([\s\S]*)$/.exec(rest);
  if (m === null) {
    return { line, text, isDirective: true, directiveName: "", directiveBody: "", offset: 0 };
  }
  void hash;
  return {
    line,
    text,
    isDirective: true,
    directiveName: (m[1] ?? "").toLowerCase(),
    directiveBody: (m[2] ?? "").trim(),
    offset: 0,
  };
}

function mkMacros(entries: Readonly<Record<string, bigint>>): MacroTableView {
  return {
    has: (name) => Object.prototype.hasOwnProperty.call(entries, name) || PREDEFINED_MACROS[name] !== undefined,
    valueOf: (name) => {
      if (Object.prototype.hasOwnProperty.call(entries, name)) return entries[name] ?? 0n;
      if (PREDEFINED_MACROS[name] !== undefined) return PREDEFINED_MACROS[name] ?? 0n;
      return null;
    },
    isFunctionLike: () => false,
    names: () => Object.keys(entries),
  };
}

/** 判据一自检：整型常量语义（位精确、溢出断言、非整型报错、除零、移位）。 */
export function selfCheckIntegerSemantics(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const m = mkMacros({ V: 3n, BIG: 0x7fffffffn });

  // 整型算术与优先级：1 + 2 * 3 == 7。
  const a = evaluateExpr("1 + 2 * 3", m, "t", 1, 1);
  out.push({
    name: "int-arith-precedence",
    pass: a.ok && a.value.value === 7n,
    detail: a.ok ? `1 + 2 * 3 = ${a.value.value.toString(10)}（优先级正确）` : `求值失败：${a.message}`,
  });

  // 位精确：32 位下 1 << 31 为负（不静默回绕成正）。
  const sh = evaluateExpr("1 << 31", m, "t", 1, 1);
  out.push({
    name: "int-bit-exact-32bit",
    pass: sh.ok && sh.value.value === -2147483648n,
    detail: sh.ok ? `1 << 31 = ${sh.value.value.toString(10)}（32 位有符号精确，未回绕成 ${(2147483648n).toString(10)}）` : "求值失败",
  });

  // 十六进制字面量 64 位档，不溢出。
  const hex = evaluateExpr("0xFFFFFFFFFFFFFFFF", m, "t", 1, 1);
  const hexMax = BigInt.asUintN(64, 0xFFFFFFFFFFFFFFFFn);
  out.push({
    name: "int-hex-literal-64bit",
    pass: hex.ok && hex.value.value === hexMax && hex.value.unsigned && hex.value.width === 64,
    detail: hex.ok
      ? `0xFFFFFFFFFFFFFFFF = ${hex.value.value.toString(10)}（16/8/2 进制字面量按C 语义自动归无符号，位宽 ${hex.value.width}）`
      : "求值失败",
  });

  // 溢出必须报错（不静默回绕）：2147483647 + 1 在 32 位溢出。
  const ovf = evaluateExpr("2147483647 + 1", m, "t", 1, 1);
  out.push({
    name: "int-overflow-rejected-not-wrapped",
    pass: !ovf.ok && ovf.code === "EXPR_OVERFLOW",
    detail: ovf.ok ? "32 位溢出竟被放行（静默回绕，最难查的一类缺陷）" : "32 位溢出显式报错 EXPR_OVERFLOW，不静默回绕",
  });

  // 左移在无符号对应类型上进行（C 语义）→1<<31 良定义为 -2147483648。
  const shl = evaluateExpr("1 << 31", m, "t", 1, 1);
  out.push({
    name: "int-leftshift-c-semantics",
    pass: shl.ok && shl.value.value === -2147483648n,
    detail: shl.ok ? `1 << 31 = ${shl.value.value.toString(10)}（按无符号对应类型移位后回译，良定义）` : `左移被误拒：${shl.code}`,
  });

  // 除零报错。
  const dz = evaluateExpr("V / 0", m, "t", 1, 1);
  out.push({
    name: "int-div-zero-rejected",
    pass: !dz.ok && dz.code === "EXPR_DIV_ZERO",
    detail: dz.ok ? "除零竟被放行" : "除零显式报错 EXPR_DIV_ZERO",
  });

  // 移位越界报错。
  const sbr = evaluateExpr("1 << 32", m, "t", 1, 1);
  out.push({
    name: "int-shift-out-of-range-rejected",
    pass: !sbr.ok && sbr.code === "EXPR_SHIFT_OUT_OF_RANGE",
    detail: sbr.ok ? "移位越界竟被放行" : "移位量 32 ≥ 位宽 32 显式报错",
  });

  // 浮点字面量报「非整型」并带类型说明（判据一 + 判据四）。
  const fl = evaluateExpr("1.5 > 1", m, "t", 1, 1);
  const flDiag = fl.diagnostics.find((d) => d.code === "EXPR_NOT_INTEGER");
  out.push({
    name: "float-literal-rejected-with-type-note",
    pass: !fl.ok && flDiag !== undefined && (flDiag.message.includes("浮点") || (flDiag.hint.includes("整型") ?? false)),
    detail: flDiag === undefined ? "浮点字面量未产出类型说明" : `浮点被拒且带类型说明：「${flDiag.message}」`,
  });

  // 无符号环绕是**定义良好**行为（C 语义），不得报溢出。
  const uw = evaluateExpr("1u - 2u", m, "t", 1, 1);
  out.push({
    name: "int-unsigned-wraps-defined",
    pass: uw.ok && uw.value.value === 4294967295n && uw.value.unsigned,
    detail: uw.ok
      ? `1u - 2u = ${uw.value.value.toString(10)}（无符号模 2^32 环绕，定义良好，不报溢出）`
      : `无符号环绕被误报为溢出：${uw.code}`,
  });

  // 三目 + 短路求值。
  const tern = evaluateExpr("V > 2 ? 10 : 20", m, "t", 1, 1);
  out.push({
    name: "int-ternary-and-short-circuit",
    pass: tern.ok && tern.value.value === 10n,
    detail: tern.ok ? `3 > 2 ? 10 : 20 = ${tern.value.value.toString(10)}` : "三目求值失败",
  });

  // 有符号取模须截断向零（C 语义），非欧几里得余数。
  const mod = evaluateExpr("-7 % 3", m, "t", 1, 1);
  out.push({
    name: "int-modulo-truncates-toward-zero",
    pass: mod.ok && mod.value.value === -1n,
    detail: mod.ok ? `-7 % 3 = ${mod.value.value.toString(10)}（截断向零，非欧几里得 -2）` : "取模求值失败",
  });

  return out;
}

/** 判据二自检：分支全族 + 嵌套栈 + 逐层未闭合报错。 */
export function selfCheckBranchFamilyAndStack(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const m = mkMacros({ HAS_A: 1n, ZERO: 0n });

  // 全族：if / ifdef / ifndef / elif / else / endif 齐备。
  const all = [
    L(1, "#if HAS_A"),
    L(2, "a1"),
    L(3, "#elif ZERO"),
    L(4, "a2"),
    L(5, "#else"),
    L(6, "a3"),
    L(7, "#endif"),
  ];
  const rAll = evaluateConditionals(all, m, { file: "t" });
  out.push({
    name: "branch-family-full-set",
    pass: rAll.ok && rAll.value.activeLines.join(",") === "2" && rAll.value.decisions.length === 3,
    detail: rAll.ok
      ? `全族求值：活跃**代码行** ${rAll.value.activeLines.join(",")}（#if 命中 → a1 保留、elif/else 两支跳过），分支判定记录 ${rAll.value.decisions.length.toString(10)} 条（#endif 只闭合不产判定；指令行不入代码流）`
      : `全族求值失败：${rAll.message}`,
  });

  // ifndef 语义。
  const nd = evaluateConditionals([L(1, "#ifndef NOPE"), L(2, "keep"), L(3, "#endif")], m, { file: "t" });
  out.push({
    name: "ifndef-true-when-undefined",
    pass: nd.ok && nd.value.activeLines.join(",") === "2",
    detail: nd.ok ? `未定义宏的 #ifndef 为真：活跃代码行 ${nd.value.activeLines.join(",")}（指令行不入流）` : "ifndef 语义错误",
  });

  // else 多重报错 + 双侧定位指向开指令。
  const dbl = evaluateConditionals([L(1, "#if 1"), L(2, "#else"), L(3, "#else"), L(4, "#endif")], m, { file: "t" });
  const dblDiag = dbl.diagnostics.find((d) => d.code === "COND_ELSE_AFTER_ELSE");
  out.push({
    name: "else-after-else-rejected",
    pass: !dbl.ok && dblDiag !== undefined && dblDiag.related.length === 1 && (dblDiag.related[0]?.startLine ?? 0) === 1,
    detail: dblDiag === undefined ? "第二个 #else 未报错" : `第二个 #else 被拒，双侧定位指向第 ${String(dblDiag.related[0]?.startLine ?? -1)} 行开指令`,
  });

  // elif 在 else 之后报错。
  const ea = evaluateConditionals([L(1, "#if 1"), L(2, "#else"), L(3, "#elif 1"), L(4, "#endif")], m, { file: "t" });
  out.push({
    name: "elif-after-else-rejected",
    pass: !ea.ok && ea.diagnostics.some((d) => d.code === "COND_ELIF_AFTER_ELSE"),
    detail: ea.ok ? "#else 后的 #elif 未报错" : "#else 后的 #elif 显式报错",
  });

  // 未闭合：逐层报错，两层两条，各自指向本层开指令。
  const unclosed = evaluateConditionalsReport([L(1, "#if 1"), L(2, "x"), L(3, "#if 0"), L(4, "y")], m, { file: "t" });
  const uc = unclosed.diagnostics.filter((d) => d.code === "COND_UNCLOSED");
  const ucLines = uc.map((d) => d.span?.startLine ?? -1).sort((a, b) => a - b);
  const residualDepth = unclosed.result.residualStack.length;
  out.push({
    name: "unclosed-reported-per-layer",
    pass: !unclosed.ok && uc.length === 2 && ucLines.join(",") === "1,3" && residualDepth === 2,
    detail: `未闭合逐层报错 ${uc.length.toString(10)} 条，分别指向第 ${ucLines.join(" / ")} 行开指令；残留栈深 ${residualDepth.toString(10)}`,
  });

  // 未匹配指令（endif 无配对）。
  const un = evaluateConditionals([L(1, "x"), L(2, "#endif")], m, { file: "t" });
  out.push({
    name: "unmatched-endif-rejected",
    pass: !un.ok && un.diagnostics.some((d) => d.code === "COND_UNMATCHED_DIRECTIVE"),
    detail: un.ok ? "无配对 #endif 未报错" : "无配对的 #endif 显式报错",
  });

  // 嵌套栈：内层命中不影响外层未命中层的跳过。
  const nest = evaluateConditionals(
    [L(1, "#if 0"), L(2, "#if 1"), L(3, "dead"), L(4, "#endif"), L(5, "#endif"), L(6, "alive")],
    m,
    { file: "t" },
  );
  out.push({
    name: "nested-stack-inner-not-resurrected",
    pass: nest.ok && nest.value.activeLines.join(",") === "6" && nest.value.stats.maxDepth === 2,
    detail: nest.ok
      ? `外层 #if 0 关闭后，内层 #if 1 不会复活代码：唯一活跃代码行为第 ${nest.value.activeLines.join(",")} 行，峰值深度 ${nest.value.stats.maxDepth.toString(10)}`
      : "嵌套栈语义错误",
  });

  return out;
}

/** 判据三自检：跳过快扫（O(段长) 快扫 + 段内零求值 + 段记录完整）。 */
export function selfCheckSkipFastScan(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const m = mkMacros({ ON: 1n });

  // 跳过段内塞满语法垃圾：不得报错（判据四：跳过段只做词法）。
  const garbage = [
    L(1, "#if 0"),
    L(2, "))) ))) unbalanced"),
    L(3, "#define ???broken"),
    L(4, "fn main() { let x: Undefined = 1 +++ }"),
    L(5, "#include <nonexistent/does/not/exist.h>"),
    L(6, "#if"),
    L(7, "#endif"),
    L(8, "#endif"),
    L(9, "fn real() -> i32 { 0 }"),
  ];
  const g = evaluateConditionals(garbage, m, { file: "t" });
  out.push({
    name: "skipped-segment-no-diagnostics",
    pass: g.ok && g.value.activeLines.join(",") === "9",
    detail: g.ok
      ? `跳过段内 7 行语法垃圾零诊断（唯一入流代码行为第 9 行），故意抑制 ${g.value.stats.suppressedInSkipped.toString(10)} 条`
      : `跳过段竟报错：${g.diagnostics.map((d) => d.code).join(",")}`,
  });

  // 快扫的机检证据：跳过段内表达式求值次数为 0。
  const nestedInactive = evaluateConditionals(
    [L(1, "#if 0"), L(2, "#if 1 + 1"), L(3, "dead"), L(4, "#endif"), L(5, "#endif")],
    m,
    { file: "t" },
  );
  out.push({
    name: "skip-fast-scan-zero-eval",
    pass: nestedInactive.ok && nestedInactive.value.stats.expressionsEvaluated === 1,
    detail: nestedInactive.ok
      ? `外层关闭后，内层 #if 表达式不求值：全程仅求值 ${nestedInactive.value.stats.expressionsEvaluated.toString(10)} 次（外层 1 次），跳过 ${nestedInactive.value.stats.skippedLines.toString(10)} 行`
      : "快扫统计错误",
  });

  // 已取分支后的 #elif 不求值（短路）。
  const shortCircuit = evaluateConditionals(
    [L(1, "#if 1"), L(2, "first"), L(3, "#elif UNDEFINED_THING"), L(4, "second"), L(5, "#endif")],
    m,
    { file: "t" },
  );
  const scNote = shortCircuit.diagnostics.some((d) => d.code === "COND_UNDEFINED_IDENT_AS_ZERO");
  out.push({
    name: "elif-short-circuits-after-taken",
    pass: shortCircuit.ok && shortCircuit.value.activeLines.join(",") === "2" && !scNote,
    detail: shortCircuit.ok
      ? `首分支命中后 #elif 不求值（未定义宏note 也被短路抑制），活跃代码行 ${shortCircuit.value.activeLines.join(",")}`
      : "短路语义错误",
  });

  // 段记录：起止行 + 关门原因 + lexical-only 策略齐备。
  const seg = evaluateConditionals(
    [L(1, "#if 0"), L(2, "a"), L(3, "b"), L(4, "#endif"), L(5, "c")],
    m,
    { file: "t" },
  );
  const s0 = seg.ok ? seg.value.skipped[0] : undefined;
  out.push({
    name: "skip-segment-record-complete",
    pass:
      seg.ok &&
      s0 !== undefined &&
      s0.startLine === 1 &&
      s0.endLine === 3 &&
      s0.reason === "if-false" &&
      s0.policy === "lexical-only" &&
      s0.lineCount === 3,
    detail: s0 === undefined ? "未产出段记录" : `段记录：第 ${s0.startLine.toString(10)}~${s0.endLine.toString(10)} 行（收尾 #endif 不计入段），因 ${s0.reason} 关闭，策略 ${s0.policy}，含 ${s0.lineCount.toString(10)} 行`,
  });

  // 未闭合段的 endLine = -1且带专项诊断。
  const unterm = evaluateConditionalsReport([L(1, "#if 0"), L(2, "a")], m, { file: "t" });
  const us = unterm.result.skipped[0];
  out.push({
    name: "skip-segment-unterminated-marked",
    pass: !unterm.ok && us !== undefined && us.endLine === -1 && unterm.diagnostics.some((d) => d.code === "SKIP_SEGMENT_UNTERMINATED"),
    detail: us === undefined ? "未产出段记录" : `未闭合段 endLine = ${us.endLine.toString(10)} 并产出 SKIP_SEGMENT_UNTERMINATED`,
  });

  // 扫描复杂度：行数与被扫行数一致（无隐藏的二次扫描）。
  const big: SourceLine[] = [L(1, "#if 0")];
  for (let i = 2; i <= 201; i += 1) big.push(L(i, `dead_${i.toString(10)}`));
  big.push(L(202, "#endif"), L(203, "live"));
  const bg = evaluateConditionals(big, m, { file: "t" });
  out.push({
    name: "skip-scan-linear",
    pass: bg.ok && bg.value.stats.linesScanned === 203 && bg.value.stats.skippedLines === 201 && bg.value.activeLines.join(",") === "203",
    detail: bg.ok
      ? `203 行单遍扫过（linesScanned=${bg.value.stats.linesScanned.toString(10)}），跳过 ${bg.value.stats.skippedLines.toString(10)} 行，O(段长) 快扫成立`
      : "复杂度断言失败",
  });

  return out;
}

/** 判据四自检：语义显性（未定义宏显性、非法表达式显性、诊断三要素齐备）。 */
export function selfCheckExplicitness(): SelfCheck[] {
  const out: SelfCheck[] = [];
  const m = mkMacros({ ON: 1n });

  // 未定义宏 → 按假 +产出 note（不是无声当假）。
  const undef = evaluateConditionals([L(1, "#if NOT_DEFINED"), L(2, "x"), L(3, "#endif"), L(4, "y")], m, { file: "t" });
  const unNote = undef.diagnostics.find((d) => d.code === "COND_UNDEFINED_IDENT_AS_ZERO");
  out.push({
    name: "undefined-ident-zero-is-explicit",
    pass: undef.ok && unNote !== undefined && undef.value.activeLines.join(",") === "4",
    detail: unNote === undefined
      ? "未定义标识符被无声当假（判据四失败）"
      : `未定义 NOT_DEFINED 按0（假）处理且产出 note：活跃行 ${undef.ok ? undef.value.activeLines.join(",") : "（求值失败）"}`,
  });

  // 三要素齐备：所有诊断都有非空 message 与 hint。
  const bad = evaluateConditionals(
    [
      L(1, "#ifdef"),
      L(2, "#endif"),
      L(3, "#if 1 +"),
      L(4, "#endif"),
      L(5, "#if 1 +"),
      L(6, "#endif"),
      L(7, "#endif"),
    ],
    m,
    { file: "t" },
  );
  const allThree = bad.diagnostics.every((d) => d.message.trim() !== "" && d.hint.trim() !== "" && d.code.trim() !== "");
  out.push({
    name: "all-diagnostics-three-elements",
    pass: !bad.ok && bad.diagnostics.length >= 3 && allThree,
    detail: `产出 ${bad.diagnostics.length.toString(10)} 条诊断（缺宏名 / 表达式残缺 ×2 / 多余 endif），三要素齐备：${allThree ? "是" : "否"}：${bad.diagnostics.map((d) => d.code).join(" / ")}`,
  });

  // 空表达式报错而非默认真。
  const empty = evaluateExpr("", m, "t", 1, 1);
  out.push({
    name: "empty-expression-rejected",
    pass: !empty.ok && empty.code === "EXPR_EMPTY",
    detail: empty.ok ? "空表达式竟被放行" : "空表达式显式报错 EXPR_EMPTY（不默认当真）",
  });

  // 字符串字面量报「非整型」并带类型说明。
  const str = evaluateExpr('"abc"', m, "t", 1, 1);
  out.push({
    name: "string-literal-rejected-with-type-note",
    pass: !str.ok && str.code === "EXPR_NOT_INTEGER" && str.message.includes("字符串"),
    detail: str.ok ? "字符串字面量竟被放行" : `字符串被拒并带类型说明：「${str.message}」`,
  });

  // 残缺表达式报错（`1 +`）。
  const inc = evaluateExpr("1 +", m, "t", 1, 1);
  out.push({
    name: "incomplete-expression-rejected",
    pass: !inc.ok,
    detail: inc.ok ? "残缺表达式 `1 +` 竟被放行" : `残缺表达式被拒：${inc.code}`,
  });

  // 短路区内未定义宏不产note（`0 && X` 的 X 不参与判定，不该刷屏）。
  const quiet = evaluateExpr("0 && NEVER_DEFINED", m, "t", 1, 1);
  out.push({
    name: "short-circuit-suppresses-undefined-note",
    pass: quiet.ok && quiet.value.value === 0n && !quiet.diagnostics.some((d) => d.code === "COND_UNDEFINED_IDENT_AS_ZERO"),
    detail: quiet.ok ? "`0 && NEVER_DEFINED` = 0，短路区未产出未定义宏 note（不刷屏）" : "短路求值失败",
  });

  // 诊断可被下游检索：不同类别的错误不得合并成一个通用码。
  // （同一码可合法重复出现——两处残缺表达式本就是同一个码。）
  const codes = new Set(bad.diagnostics.map((d) => d.code));
  const merged = codes.size === 1;
  out.push({
    name: "diagnostic-codes-not-merged",
    pass: codes.size >= 3 && !merged,
    detail: `${bad.diagnostics.length.toString(10)} 条诊断覆盖 ${codes.size.toString(10)} 个互异 code（${[...codes].join(" / ")}）——未被合并成单一通用错误`,
  });

  return out;
}

/** 全量自检入口。 */
export function runSelfCheck(): {
  readonly groups: Readonly<Record<string, readonly SelfCheck[]>>;
  readonly allPass: boolean;
  readonly failed: readonly string[];
} {
  const groups = {
    integerSemantics: selfCheckIntegerSemantics(),
    branchFamily: selfCheckBranchFamilyAndStack(),
    skipFastScan: selfCheckSkipFastScan(),
    explicitness: selfCheckExplicitness(),
  };
  const failed: string[] = [];
  for (const [g, items] of Object.entries(groups)) {
    for (const it of items) {
      if (!it.pass) failed.push(`${g}.${it.name}: ${it.detail}`);
    }
  }
  return { groups, allPass: failed.length === 0, failed };
}
