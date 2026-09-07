/**
 * F-2.1 计算器引擎（零依赖，引擎本地计算）：
 * - 标准/科学模式：递归下降求值器（+ - × ÷ 幂 ｜ 一元负号 ｜ 百分比 ｜ 括号 ｜ 函数 ｜ 阶乘）
 * - 程序员模式：64 位有符号整数（BigInt）位运算 + 多进制
 * - 单位换算：长度/质量/温度/数据/时间/面积/速度
 */

// ---------- 标准与科学模式 ----------

export type CalcAngle = "deg" | "rad";

interface Token {
  t: "num" | "op" | "lp" | "rp" | "id";
  v: string;
}

function tokenize(src: string): Token[] {
  const out: Token[] = [];
  let i = 0;
  while (i < src.length) {
    const c = src[i]!;
    if (c === " ") {
      i++;
      continue;
    }
    if (/[0-9.]/.test(c)) {
      let j = i;
      while (j < src.length && /[0-9.]/.test(src[j]!)) j++;
      out.push({ t: "num", v: src.slice(i, j) });
      i = j;
      continue;
    }
    if (/[a-z]/i.test(c)) {
      let j = i;
      while (j < src.length && /[a-z0-9]/i.test(src[j]!)) j++;
      out.push({ t: "id", v: src.slice(i, j).toLowerCase() });
      i = j;
      continue;
    }
    if ("+-*/^%!".includes(c)) {
      out.push({ t: "op", v: c === "×" ? "*" : c === "÷" ? "/" : c });
      i++;
      continue;
    }
    if (c === "(") {
      out.push({ t: "lp", v: c });
      i++;
      continue;
    }
    if (c === ")") {
      out.push({ t: "rp", v: c });
      i++;
      continue;
    }
    throw new Error(`非法字符 ${c}`);
  }
  return out;
}

const FUNCS: Record<string, (x: number, angle: CalcAngle) => number> = {
  sin: (x, a) => Math.sin(a === "deg" ? (x * Math.PI) / 180 : x),
  cos: (x, a) => Math.cos(a === "deg" ? (x * Math.PI) / 180 : x),
  tan: (x, a) => Math.tan(a === "deg" ? (x * Math.PI) / 180 : x),
  ln: (x) => Math.log(x),
  log: (x) => Math.log10(x),
  sqrt: (x) => Math.sqrt(x),
  abs: (x) => Math.abs(x),
};

function factorial(n: number): number {
  if (n < 0 || !Number.isInteger(n)) throw new Error("阶乘仅支持非负整数");
  if (n > 170) return Infinity;
  let r = 1;
  for (let k = 2; k <= n; k++) r *= k;
  return r;
}

/** 递归下降求值：expr → term (('+'|'-') term)*；term → unary (('*'|'/') unary)*；unary → ('-'|'+')* postfix；postfix → primary ('!'|'%')*；primary → num | func(expr) | '(' expr ')' | const。 */
function parseExpr(ts: Token[], pos: { i: number }, angle: CalcAngle): number {
  let left = parseTerm(ts, pos, angle);
  while (pos.i < ts.length && (ts[pos.i]!.v === "+" || ts[pos.i]!.v === "-") && ts[pos.i]!.t === "op") {
    const op = ts[pos.i++]!.v;
    const right = parseTerm(ts, pos, angle);
    left = op === "+" ? left + right : left - right;
  }
  return left;
}

function parseTerm(ts: Token[], pos: { i: number }, angle: CalcAngle): number {
  let left = parseUnary(ts, pos, angle);
  while (pos.i < ts.length && ts[pos.i]!.t === "op" && "*/^".includes(ts[pos.i]!.v)) {
    const op = ts[pos.i++]!.v;
    if (op === "^") {
      // 幂右结合：2^3^2 = 2^(3^2)
      const right = parseTerm(ts, pos, angle);
      left = Math.pow(left, right);
      break;
    }
    const right = parseUnary(ts, pos, angle);
    if (op === "*") left = left * right;
    else left = left / right;
  }
  return left;
}

function parseUnary(ts: Token[], pos: { i: number }, angle: CalcAngle): number {
  if (pos.i < ts.length && ts[pos.i]!.t === "op" && (ts[pos.i]!.v === "-" || ts[pos.i]!.v === "+")) {
    const op = ts[pos.i++]!.v;
    const v = parseUnary(ts, pos, angle);
    return op === "-" ? -v : v;
  }
  return parsePostfix(ts, pos, angle);
}

function parsePostfix(ts: Token[], pos: { i: number }, angle: CalcAngle): number {
  let v = parsePrimary(ts, pos, angle);
  while (pos.i < ts.length && ts[pos.i]!.t === "op" && (ts[pos.i]!.v === "!" || ts[pos.i]!.v === "%")) {
    const op = ts[pos.i++]!.v;
    v = op === "!" ? factorial(v) : v / 100;
  }
  return v;
}

function parsePrimary(ts: Token[], pos: { i: number }, angle: CalcAngle): number {
  if (pos.i >= ts.length) throw new Error("表达式不完整");
  const tk = ts[pos.i++]!;
  if (tk.t === "num") {
    const n = Number(tk.v);
    if (Number.isNaN(n)) throw new Error(`非法数字 ${tk.v}`);
    return n;
  }
  if (tk.t === "lp") {
    const v = parseExpr(ts, pos, angle);
    if (pos.i >= ts.length || ts[pos.i]!.t !== "rp") throw new Error("缺少右括号");
    pos.i++;
    return v;
  }
  if (tk.t === "id") {
    if (tk.v === "pi") return Math.PI;
    if (tk.v === "e") return Math.E;
    const fn = FUNCS[tk.v];
    if (!fn) throw new Error(`未知函数 ${tk.v}`);
    if (pos.i >= ts.length || ts[pos.i]!.t !== "lp") throw new Error(`函数 ${tk.v} 缺少参数`);
    pos.i++;
    const arg = parseExpr(ts, pos, angle);
    if (pos.i >= ts.length || ts[pos.i]!.t !== "rp") throw new Error("缺少右括号");
    pos.i++;
    return fn(arg, angle);
  }
  throw new Error(`意外的记号 ${tk.v}`);
}

/** 求值标准/科学表达式（抛错 = 表达式非法，UI 如实展示）。 */
export function calcEval(expr: string, angle: CalcAngle = "deg"): number {
  const ts = tokenize(expr);
  const pos = { i: 0 };
  const v = parseExpr(ts, pos, angle);
  if (pos.i !== ts.length) throw new Error("表达式尾部有多余记号");
  return v;
}

// ---------- 程序员模式（64 位有符号整数） ----------

export type ProgBase = 2 | 8 | 10 | 16;

export type ProgOp =
  | "and" | "or" | "xor" | "not"
  | "shl" | "shr"
  | "add" | "sub" | "mul" | "div" | "mod";

function toBigInt(v: bigint, base: ProgBase): string {
  const neg = v < 0n;
  const abs = (neg ? -v : v).toString(base);
  return neg ? `-${abs}` : abs;
}

/** 按指定进制解析整数（非法字符抛错）。 */
export function progParse(s: string, base: ProgBase): bigint {
  const t = s.trim();
  if (!/^[+-]?[0-9a-f]+$/i.test(t)) throw new Error("非法数字");
  return base === 10 ? BigInt(t) : parseBase(t, base);
}

function parseBase(t: string, base: ProgBase): bigint {
  const neg = t.startsWith("-");
  const body = t.replace(/^[-+]/, "").toLowerCase();
  const digits = "0123456789abcdef".slice(0, base);
  let v = 0n;
  for (const ch of body) {
    const d = digits.indexOf(ch);
    if (d < 0) throw new Error(`进制 ${base} 不支持字符 ${ch}`);
    v = v * BigInt(base) + BigInt(d);
  }
  return neg ? -v : v;
}

function applyOp(a: bigint, op: ProgOp, b: bigint): bigint {
  switch (op) {
    case "and": return a & b;
    case "or": return a | b;
    case "xor": return a ^ b;
    case "not": return ~a; // b 忽略
    case "shl": return a << b;
    case "shr": return a >> b;
    case "add": return a + b;
    case "sub": return a - b;
    case "mul": return a * b;
    case "div": return b === 0n ? (() => { throw new Error("除数为 0"); })() : a / b;
    case "mod": return b === 0n ? (() => { throw new Error("模数为 0"); })() : a % b;
  }
}

/** 程序员模式一次二元/一元运算（64 位环绕到有符号范围）。 */
export function progEval(a: bigint, op: ProgOp, b = 0n): bigint {
  const v = BigInt.asIntN(64, applyOp(BigInt.asIntN(64, a), op, BigInt.asIntN(64, b)));
  return v;
}

/** 同值多进制展示（hex/dec/oct/bin）。 */
export function progFormat(v: bigint): { hex: string; dec: string; oct: string; bin: string } {
  const x = BigInt.asIntN(64, v);
  return {
    hex: toBigInt(x, 16),
    dec: toBigInt(x, 10),
    oct: toBigInt(x, 8),
    bin: toBigInt(x, 2),
  };
}

// ---------- 单位换算 ----------

export type UnitCategory = "length" | "mass" | "temperature" | "data" | "time" | "area" | "speed";

export interface UnitDef {
  id: string;
  /** 相对基准单位的倍率（温度类特殊处理）。 */
  factor: number;
  offset?: number; // 温度：val*factor + offset → 基准（摄氏）
}

export const UNIT_TABLE: Record<UnitCategory, { label: string; units: UnitDef[] }> = {
  length: {
    label: "长度",
    units: [
      { id: "mm", factor: 0.001 },
      { id: "cm", factor: 0.01 },
      { id: "m", factor: 1 },
      { id: "km", factor: 1000 },
      { id: "in", factor: 0.0254 },
      { id: "ft", factor: 0.3048 },
      { id: "mi", factor: 1609.344 },
    ],
  },
  mass: {
    label: "质量",
    units: [
      { id: "g", factor: 0.001 },
      { id: "kg", factor: 1 },
      { id: "t", factor: 1000 },
      { id: "oz", factor: 0.028349523125 },
      { id: "lb", factor: 0.45359237 },
    ],
  },
  temperature: {
    label: "温度",
    units: [
      { id: "°C", factor: 1, offset: 0 },
      { id: "°F", factor: 5 / 9, offset: -32 * (5 / 9) },
      { id: "K", factor: 1, offset: -273.15 },
    ],
  },
  data: {
    label: "数据",
    units: [
      { id: "B", factor: 1 },
      { id: "KB", factor: 1024 },
      { id: "MB", factor: 1024 ** 2 },
      { id: "GB", factor: 1024 ** 3 },
      { id: "TB", factor: 1024 ** 4 },
    ],
  },
  time: {
    label: "时间",
    units: [
      { id: "ms", factor: 0.001 },
      { id: "s", factor: 1 },
      { id: "min", factor: 60 },
      { id: "h", factor: 3600 },
      { id: "d", factor: 86400 },
    ],
  },
  area: {
    label: "面积",
    units: [
      { id: "m²", factor: 1 },
      { id: "km²", factor: 1e6 },
      { id: "亩", factor: 666.6666666666667 },
      { id: "公顷", factor: 10000 },
    ],
  },
  speed: {
    label: "速度",
    units: [
      { id: "m/s", factor: 1 },
      { id: "km/h", factor: 1 / 3.6 },
      { id: "mph", factor: 0.44704 },
      { id: "kn", factor: 0.514444 },
    ],
  },
};

/** 单位换算：from → to（温度经摄氏中转）。 */
export function unitConvert(value: number, cat: UnitCategory, from: string, to: string): number {
  const table = UNIT_TABLE[cat];
  const f = table.units.find((u) => u.id === from);
  const t = table.units.find((u) => u.id === to);
  if (!f || !t) throw new Error("未知单位");
  if (cat === "temperature") {
    const c = value * f.factor + (f.offset ?? 0); // → 摄氏
    return (c - (t.offset ?? 0)) / t.factor;
  }
  return (value * f.factor) / t.factor;
}

/** 结果格式化：整数直出，小数保留 10 位有效并去尾零。 */
export function fmtResult(v: number): string {
  if (!Number.isFinite(v)) return v > 0 ? "∞" : Number.isNaN(v) ? "错误" : "-∞";
  if (Number.isInteger(v) && Math.abs(v) < 1e15) return String(v);
  const s = v.toPrecision(12).replace(/\.?0+$/, "");
  return s;
}
