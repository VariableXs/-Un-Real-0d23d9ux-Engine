/**
 * F163 桌面小组件深化 · 数据模型 + 更新调度 + 新鲜度执法。
 *
 * 主册判据延伸：
 * - F163「时钟秒级准确」「天气数据源一致（与 F101 面板同值）」——
 *   数据面独立成引擎：任何数据源进来先归一化（一处一事实的归一层），
 *   面板只消费归一模型——「同值」是结构保证不是巧合；
 * - 「更新调度表（15s 抖动）」——调度器真实实现：基准间隔 + 抖动窗 +
 *   退避（失败指数退避，上限封顶）+ 断网降级（「数据截至」标注）；
 * - 性能保护执法（widgets-engine 判定）的数据侧输入：每源的开销计量。
 */

// ---------- 时钟（秒级准确的日历数学） ----------

export interface ClockFace {
  h: number;
  m: number;
  s: number;
  /** 12 小时制的上午/下午（zh 面板文案侧消费）。 */
  meridiem: "AM" | "PM";
  /** 当月天数（月历格数）。 */
  daysInMonth: number;
  /** 1 号是星期几（0=周日——月历首行空格数）。 */
  firstWeekday: number;
  /** ISO 日期 YYYY-MM-DD（跨源对账键——F101 同值判定的对拍面）。 */
  isoDate: string;
}

export function clockFace(d: Date): ClockFace {
  const y = d.getFullYear();
  const mo = d.getMonth();
  return {
    h: d.getHours(),
    m: d.getMinutes(),
    s: d.getSeconds(),
    meridiem: d.getHours() < 12 ? "AM" : "PM",
    daysInMonth: new Date(y, mo + 1, 0).getDate(),
    firstWeekday: new Date(y, mo, 1).getDay(),
    isoDate: `${y}-${String(mo + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`,
  };
}

/** 闰年（月历渲染与万年历对照 F078 同源判定）。 */
export function isLeapYear(y: number): boolean {
  return (y % 4 === 0 && y % 100 !== 0) || y % 400 === 0;
}

/** 时钟漂移检测：本地时钟与可信源差 > 容差时建议校时（F187 联动判定）。 */
export function clockDrift(localMs: number, authoritativeMs: number, toleranceMs: number): { drifted: boolean; deltaMs: number } {
  const deltaMs = localMs - authoritativeMs;
  return { drifted: Math.abs(deltaMs) > toleranceMs, deltaMs };
}

// ---------- 天气归一（外部输入全清洗——多源同值判定的结构面） ----------

export interface WeatherNow {
  /** 温度 ℃（一律转摄氏——源给华氏由归一层换算）。 */
  tempC: number;
  /** 体感 ℃。 */
  feelsC: number;
  /** 湿度 0..100。 */
  humidity: number;
  /** 风速 km/h（一律转 km/h）。 */
  windKmh: number;
  /** 代码化天气（归一层自定枚举——面板图标只认这枚举）。 */
  condition: "sunny" | "cloudy" | "overcast" | "rain" | "snow" | "fog" | "storm";
  fetchedAt: number;
}

export interface WeatherSourcePayload {
  source: string;
  /** 原始字段（源给什么是什么——归一层不信任源结构）。 */
  raw: Record<string, unknown>;
}

const VALID_CONDITIONS = new Set(["sunny", "cloudy", "overcast", "rain", "snow", "fog", "storm"]);

/**
 * 天气载荷归一：字段缺失/越界逐项清洗——宁可「数据不完整」显性化，
 * 不给面板递可疑值（粘贴清洗同源纪律）。
 */
export function normalizeWeather(p: WeatherSourcePayload, now: number): { ok: true; data: WeatherNow } | { ok: false; reasons: string[] } {
  const reasons: string[] = [];
  const raw = p.raw ?? {};
  const num = (k: string): number | null => {
    const v = raw[k];
    if (typeof v === "number" && Number.isFinite(v)) return v;
    if (typeof v === "string" && v.trim() !== "" && Number.isFinite(Number(v))) return Number(v);
    return null;
  };
  let tempC = num("temp");
  if (tempC !== null && raw.unit === "F") tempC = ((tempC - 32) * 5) / 9;
  if (tempC === null) reasons.push("temp 缺失或非法");
  else if (tempC < -90 || tempC > 60) reasons.push(`temp 越界（${tempC}℃）`);
  let feelsC = num("feels");
  if (feelsC !== null && raw.unit === "F") feelsC = ((feelsC - 32) * 5) / 9;
  if (feelsC === null) {
    feelsC = tempC ?? 0; // 体感缺失回退实际温度（诚实降级——面板标注用默认值）。
    reasons.length; // 不算失败——回退是合法路径。
  }
  const humidity = num("humidity");
  if (humidity === null) reasons.push("humidity 缺失");
  else if (humidity < 0 || humidity > 100) reasons.push(`humidity 越界（${humidity}）`);
  const windKmh = num("wind");
  if (windKmh === null) reasons.push("wind 缺失");
  else if (windKmh < 0 || windKmh > 300) reasons.push(`wind 越界（${windKmh}）`);
  const cond = typeof raw.condition === "string" ? raw.condition.toLowerCase() : "";
  if (!VALID_CONDITIONS.has(cond)) reasons.push(`condition 非法（${cond || "空"}）`);
  if (reasons.length > 0) return { ok: false, reasons };
  return {
    ok: true,
    data: {
      tempC: Math.round(tempC! * 10) / 10,
      feelsC: Math.round(feelsC! * 10) / 10,
      humidity: Math.round(humidity!),
      windKmh: Math.round(windKmh! * 10) / 10,
      condition: cond as WeatherNow["condition"],
      fetchedAt: now,
    },
  };
}

// ---------- 系统采样模型（监控小组件的数据侧） ----------

export interface SysSample {
  cpuPct: number; // 0..100
  memPct: number; // 0..100
  diskPct: number; // 0..100
  netKbps: number;
}

/** 采样清洗：负值/NaN/越界全部钳制并记录（面板不显示脏数）。 */
export function sanitizeSample(s: Partial<SysSample>): { clean: SysSample; fixed: string[] } {
  const fixed: string[] = [];
  const clamp = (v: unknown, max: number, name: string): number => {
    const n = typeof v === "number" && Number.isFinite(v) ? v : 0;
    if (n !== v) fixed.push(name);
    if (n < 0) {
      fixed.push(`${name}<0`);
      return 0;
    }
    if (n > max) {
      fixed.push(`${name}>${max}`);
      return max;
    }
    return Math.round(n * 10) / 10;
  };
  return {
    clean: { cpuPct: clamp(s.cpuPct, 100, "cpu"), memPct: clamp(s.memPct, 100, "mem"), diskPct: clamp(s.diskPct, 100, "disk"), netKbps: clamp(s.netKbps, 10_000_000, "net") },
    fixed,
  };
}

// ---------- 更新调度（15s 抖动 + 指数退避 + 预算执法） ----------

export interface WidgetSourcePlan {
  sourceId: string;
  /** 基准间隔 ms。 */
  intervalMs: number;
  /** 抖动半宽 ms（防齐步——所有组件同一秒齐刷刷更新的调度风暴）。 */
  jitterMs: number;
  /** 失败次数（调度器状态）。 */
  failures: number;
  /** 上次成功时间。 */
  lastOkAt: number | null;
}

/** 下次拉取时刻：基准 + 随机抖动 + 失败退避（2^n，上限 8 倍基准）。 */
export function nextFetchAt(plan: WidgetSourcePlan, now: number, jitter: () => number): number {
  const backoff = Math.min(8, 2 ** plan.failures);
  const base = plan.intervalMs * backoff;
  const j = Math.round((jitter() * 2 - 1) * plan.jitterMs);
  return now + base + j;
}

/** 失败登记（退避状态推进——连续失败升级触发 F189 同源纪律）。 */
export function recordFetchFailure(plan: WidgetSourcePlan, _now: number): WidgetSourcePlan {
  return { ...plan, failures: Math.min(10, plan.failures + 1) };
}

export function recordFetchSuccess(plan: WidgetSourcePlan, now: number): WidgetSourcePlan {
  return { ...plan, failures: 0, lastOkAt: now };
}

// ---------- 新鲜度执法（断网降级「数据截至」的人话面） ----------

export type Freshness = "live" | "stale" | "frozen" | "never";

export interface FreshnessVerdict {
  level: Freshness;
  /** 面板标注文案（「数据截至 12:03」——诚实不装新）。 */
  label: string;
  /** 距上次成功 ms（never = null）。 */
  ageMs: number | null;
}

/**
 * 新鲜度分级：live（<2×间隔）→ stale（>2×间隔，灰显）→ frozen（>10×间隔，
 * 降级文案）→ never（从未成功，占位引导）。
 */
export function freshness(plan: WidgetSourcePlan, now: number, fmtTime: (t: number) => string): FreshnessVerdict {
  if (plan.lastOkAt === null) return { level: "never", label: "暂无数据——等下一次更新或检查网络", ageMs: null };
  const ageMs = now - plan.lastOkAt;
  if (ageMs <= plan.intervalMs * 2) return { level: "live", label: "实时", ageMs };
  if (ageMs <= plan.intervalMs * 10) return { level: "stale", label: `数据截至 ${fmtTime(plan.lastOkAt)}`, ageMs };
  return { level: "frozen", label: `连接中断——数据截至 ${fmtTime(plan.lastOkAt)}`, ageMs };
}

// ---------- 数据开销计量（性能执法的输入面） ----------

export interface SourceCostLedger {
  sourceId: string;
  samples: number[];
  /** 滑窗上限（内存恒定——F228 虚拟化同源纪律）。 */
  windowSize: number;
}

export function newCostLedger(sourceId: string, windowSize = 60): SourceCostLedger {
  return { sourceId, samples: [], windowSize };
}

export function recordCost(l: SourceCostLedger, costMs: number): SourceCostLedger {
  const samples = [...l.samples, costMs];
  if (samples.length > l.windowSize) samples.splice(0, samples.length - l.windowSize);
  return { ...l, samples };
}

/** P95 开销（widgets-engine 性能执法判定的数据源——超预算降透明/关闭）。 */
export function costP95(l: SourceCostLedger): number {
  if (l.samples.length === 0) return 0;
  const s = [...l.samples].sort((a, b) => a - b);
  const idx = Math.min(s.length - 1, Math.ceil(s.length * 0.95) - 1);
  return s[idx]!;
}
