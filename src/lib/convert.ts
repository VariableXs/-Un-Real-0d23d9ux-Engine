/**
 * Z-26 换算中心 —— 纯逻辑模块（AI-08 基础工具组）。
 * 与 src/lib/calc.ts 的简版换算（UNIT_TABLE/unitConvert）相互独立：
 * 本模块是独立完整版（8 大类 78 单位 + 进制 + 汇率），不改不依赖 calc.ts。
 * 红线：
 * - 不做时区转换、不做历法精确换算（月按 30 天、年按 365 天近似）
 * - 进制转换值域固定 u64（0 – 18446744073709551615），超限如实抛错，不做环绕
 * - 汇率为外部传入的离线表（相对基准 USD），本模块不联网、不内置实时数据
 * - 声速（1 马赫）按 340.29 m/s（15°C 海平面）固定近似
 */

// ---------- 单位换算表 ----------

export type ConvCatId = "length" | "mass" | "temperature" | "area" | "volume" | "speed" | "data" | "time";

export interface ConvUnit {
  /** 单位 id（显示名，中文单位直接用汉字） */
  id: string;
  zh: string;
  en: string;
  /** 相对基准单位的倍率（基准：长度 m / 质量 kg / 温度 °C / 面积 m² / 体积 L / 速度 m/s / 数据 B / 时间 s） */
  factor: number;
  /** 温度专用：val*factor + offset → 基准（摄氏） */
  offset?: number;
}

export interface ConvCategory {
  id: ConvCatId;
  zh: string;
  en: string;
  units: ConvUnit[];
}

export const CONVERT_TABLE: Record<ConvCatId, ConvCategory> = {
  length: {
    id: "length",
    zh: "长度",
    en: "Length",
    units: [
      { id: "nm", zh: "纳米", en: "nanometer", factor: 1e-9 },
      { id: "μm", zh: "微米", en: "micrometer", factor: 1e-6 },
      { id: "mm", zh: "毫米", en: "millimeter", factor: 0.001 },
      { id: "cm", zh: "厘米", en: "centimeter", factor: 0.01 },
      { id: "dm", zh: "分米", en: "decimeter", factor: 0.1 },
      { id: "m", zh: "米", en: "meter", factor: 1 },
      { id: "km", zh: "千米", en: "kilometer", factor: 1000 },
      { id: "里", zh: "市里", en: "Chinese li", factor: 500 },
      { id: "in", zh: "英寸", en: "inch", factor: 0.0254 },
      { id: "ft", zh: "英尺", en: "foot", factor: 0.3048 },
      { id: "yd", zh: "码", en: "yard", factor: 0.9144 },
      { id: "mi", zh: "英里", en: "mile", factor: 1609.344 },
      { id: "nmi", zh: "海里", en: "nautical mile", factor: 1852 },
      { id: "AU", zh: "天文单位", en: "astronomical unit", factor: 1.495978707e11 },
      { id: "ly", zh: "光年", en: "light year", factor: 9.4607304725808e15 },
    ],
  },
  mass: {
    id: "mass",
    zh: "质量",
    en: "Mass",
    units: [
      { id: "μg", zh: "微克", en: "microgram", factor: 1e-9 },
      { id: "mg", zh: "毫克", en: "milligram", factor: 1e-6 },
      { id: "g", zh: "克", en: "gram", factor: 0.001 },
      { id: "kg", zh: "千克", en: "kilogram", factor: 1 },
      { id: "t", zh: "吨", en: "tonne", factor: 1000 },
      { id: "斤", zh: "市斤", en: "Chinese jin", factor: 0.5 },
      { id: "ct", zh: "克拉", en: "carat", factor: 0.0002 },
      { id: "oz", zh: "盎司", en: "ounce", factor: 0.028349523125 },
      { id: "lb", zh: "磅", en: "pound", factor: 0.45359237 },
      { id: "st", zh: "英石", en: "stone", factor: 6.35029318 },
    ],
  },
  temperature: {
    id: "temperature",
    zh: "温度",
    en: "Temperature",
    units: [
      { id: "°C", zh: "摄氏度", en: "celsius", factor: 1, offset: 0 },
      { id: "°F", zh: "华氏度", en: "fahrenheit", factor: 5 / 9, offset: (-32 * 5) / 9 },
      { id: "K", zh: "开尔文", en: "kelvin", factor: 1, offset: -273.15 },
    ],
  },
  area: {
    id: "area",
    zh: "面积",
    en: "Area",
    units: [
      { id: "mm²", zh: "平方毫米", en: "square millimeter", factor: 1e-6 },
      { id: "cm²", zh: "平方厘米", en: "square centimeter", factor: 1e-4 },
      { id: "m²", zh: "平方米", en: "square meter", factor: 1 },
      { id: "亩", zh: "市亩", en: "Chinese mu", factor: 666.6666666666667 },
      { id: "a", zh: "公亩", en: "are", factor: 100 },
      { id: "ha", zh: "公顷", en: "hectare", factor: 1e4 },
      { id: "km²", zh: "平方千米", en: "square kilometer", factor: 1e6 },
      { id: "in²", zh: "平方英寸", en: "square inch", factor: 0.00064516 },
      { id: "ft²", zh: "平方英尺", en: "square foot", factor: 0.09290304 },
      { id: "yd²", zh: "平方码", en: "square yard", factor: 0.83612736 },
      { id: "ac", zh: "英亩", en: "acre", factor: 4046.8564224 },
      { id: "mi²", zh: "平方英里", en: "square mile", factor: 2589988.110336 },
    ],
  },
  volume: {
    id: "volume",
    zh: "体积",
    en: "Volume",
    units: [
      { id: "mL", zh: "毫升", en: "milliliter", factor: 0.001 },
      { id: "cL", zh: "厘升", en: "centiliter", factor: 0.01 },
      { id: "dL", zh: "分升", en: "deciliter", factor: 0.1 },
      { id: "L", zh: "升", en: "liter", factor: 1 },
      { id: "m³", zh: "立方米", en: "cubic meter", factor: 1000 },
      { id: "cm³", zh: "立方厘米", en: "cubic centimeter", factor: 0.001 },
      { id: "in³", zh: "立方英寸", en: "cubic inch", factor: 0.016387064 },
      { id: "ft³", zh: "立方英尺", en: "cubic foot", factor: 28.316846592 },
      { id: "yd³", zh: "立方码", en: "cubic yard", factor: 764.554857984 },
      { id: "gal(US)", zh: "美制加仑", en: "US gallon", factor: 3.785411784 },
      { id: "gal(UK)", zh: "英制加仑", en: "imperial gallon", factor: 4.54609 },
      { id: "pt(US)", zh: "美制品脱", en: "US pint", factor: 0.473176473 },
      { id: "bbl", zh: "石油桶", en: "oil barrel", factor: 158.987294928 },
    ],
  },
  speed: {
    id: "speed",
    zh: "速度",
    en: "Speed",
    units: [
      { id: "m/s", zh: "米每秒", en: "meter per second", factor: 1 },
      { id: "km/h", zh: "千米每小时", en: "kilometer per hour", factor: 1 / 3.6 },
      { id: "mph", zh: "英里每小时", en: "mile per hour", factor: 0.44704 },
      { id: "ft/s", zh: "英尺每秒", en: "foot per second", factor: 0.3048 },
      { id: "kn", zh: "节（海里/时）", en: "knot", factor: 1852 / 3600 },
      { id: "Ma", zh: "马赫（近似）", en: "mach (approx)", factor: 340.29 },
      { id: "c", zh: "光速", en: "speed of light", factor: 299792458 },
    ],
  },
  data: {
    id: "data",
    zh: "数据量",
    en: "Data",
    units: [
      { id: "bit", zh: "比特", en: "bit", factor: 0.125 },
      { id: "B", zh: "字节", en: "byte", factor: 1 },
      { id: "KB", zh: "千字节", en: "kilobyte", factor: 1024 },
      { id: "MB", zh: "兆字节", en: "megabyte", factor: 1024 ** 2 },
      { id: "GB", zh: "吉字节", en: "gigabyte", factor: 1024 ** 3 },
      { id: "TB", zh: "太字节", en: "terabyte", factor: 1024 ** 4 },
      { id: "PB", zh: "拍字节", en: "petabyte", factor: 1024 ** 5 },
      { id: "EB", zh: "艾字节", en: "exabyte", factor: 1024 ** 6 },
    ],
  },
  time: {
    id: "time",
    zh: "时间",
    en: "Time",
    units: [
      { id: "ms", zh: "毫秒", en: "millisecond", factor: 0.001 },
      { id: "s", zh: "秒", en: "second", factor: 1 },
      { id: "min", zh: "分钟", en: "minute", factor: 60 },
      { id: "h", zh: "小时", en: "hour", factor: 3600 },
      { id: "d", zh: "天", en: "day", factor: 86400 },
      { id: "wk", zh: "周", en: "week", factor: 604800 },
      // 红线：月按 30 天、年按 365 天固定近似，不做历法精确换算
      { id: "mo", zh: "月（30 天近似）", en: "month (30d approx)", factor: 2592000 },
      { id: "yr", zh: "年（365 天近似）", en: "year (365d approx)", factor: 31536000 },
      { id: "dec", zh: "十年", en: "decade", factor: 315360000 },
    ],
  },
};

/** 类别展示顺序（单位换算标签页下拉顺序）。 */
export const CONV_CATS: ConvCatId[] = [
  "length",
  "mass",
  "temperature",
  "area",
  "volume",
  "speed",
  "data",
  "time",
];

/**
 * 单位换算（纯函数）：value from → to。
 * 温度经摄氏中转（factor + offset），其余类别直接按倍率换算。
 */
export function convert(value: number, cat: ConvCatId, from: string, to: string): number {
  const table = CONVERT_TABLE[cat];
  if (!table) throw new Error(`未知类别: ${cat}`);
  const f = table.units.find((u) => u.id === from);
  const t = table.units.find((u) => u.id === to);
  if (!f || !t) throw new Error(`未知单位: ${!f ? from : to}`);
  if (cat === "temperature") {
    const base = value * f.factor + (f.offset ?? 0); // → 摄氏
    return (base - (t.offset ?? 0)) / t.factor;
  }
  return (value * f.factor) / t.factor;
}

// ---------- 进制转换（BigInt，u64 值域） ----------

const DIGITS = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/** 64 位无符号上限。 */
export const U64_MAX = 18446744073709551615n;

/**
 * 进制转换（纯函数）：解析 numStr（fromRadix 进制，可带 0x/0b/0o 前缀）→ toRadix 进制字符串（HEX 大写）。
 * 红线：值域 u64（0 – 18446744073709551615），超上限 / 负数 / 非法字符如实抛错，绝不环绕或截断。
 */
export function radixConvert(numStr: string, fromRadix: number, toRadix: number): string {
  if (
    !Number.isInteger(fromRadix) ||
    !Number.isInteger(toRadix) ||
    fromRadix < 2 ||
    fromRadix > 36 ||
    toRadix < 2 ||
    toRadix > 36
  ) {
    throw new Error("进制必须在 2–36 之间");
  }
  let s = numStr.trim();
  if (!s) throw new Error("空数字串");
  const lower = s.toLowerCase();
  if (fromRadix === 16 && lower.startsWith("0x")) s = s.slice(2);
  else if (fromRadix === 2 && lower.startsWith("0b")) s = s.slice(2);
  else if (fromRadix === 8 && lower.startsWith("0o")) s = s.slice(2);
  if (s.startsWith("+")) s = s.slice(1);
  if (s.startsWith("-")) throw new Error("不支持负数（u64 值域 0 – 18446744073709551615）");
  let v = 0n;
  for (const c of s.toUpperCase()) {
    const d = DIGITS.indexOf(c);
    if (d < 0 || d >= fromRadix) throw new Error(`非法字符 "${c}"（进制 ${fromRadix}）`);
    v = v * BigInt(fromRadix) + BigInt(d);
    if (v > U64_MAX) throw new Error("超出 64 位无符号整数上限（u64 max 18446744073709551615）");
  }
  return v.toString(toRadix).toUpperCase();
}

// ---------- 汇率换算（离线表，基准 USD） ----------

/**
 * 汇率换算（纯函数）：把 from 货币的 amount 折算为 USD 基准值。
 * rates 定义：rates[cur] = 1 USD 可兑换的 cur 数量（USD 本身应为 1）。
 * 目标货币换算 = convertRate(amount, from, rates) * rates[to]。
 */
export function convertRate(amount: number, from: string, rates: Record<string, number>): number {
  const r = rates[from];
  if (typeof r !== "number" || !Number.isFinite(r) || r <= 0) {
    throw new Error(`缺少 ${from} 的有效汇率（rates 相对基准 USD）`);
  }
  return amount / r;
}

// ---------- 结果格式化 ----------

/**
 * 换算结果格式化（纯函数）：10 位有效数字并去尾零。
 * 整数（绝对值 < 1e15）直出；超大/超小走科学计数（同样 10 位有效 + 去尾零）。
 */
export function formatConvResult(v: number): string {
  if (Number.isNaN(v)) return "—";
  if (!Number.isFinite(v)) return v > 0 ? "∞" : "-∞";
  if (v === 0) return "0";
  if (Number.isInteger(v) && Math.abs(v) < 1e15) return String(v);
  const s = v.toPrecision(10);
  if (s.includes("e")) {
    const [m, ex] = s.split("e");
    const mm = m!.includes(".") ? m!.replace(/0+$/, "").replace(/\.$/, "") : m!;
    return `${mm}e${ex}`;
  }
  return s.includes(".") ? s.replace(/0+$/, "").replace(/\.$/, "") : s;
}
