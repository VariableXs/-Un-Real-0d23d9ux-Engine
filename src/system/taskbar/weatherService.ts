/**
 * Z-23 天气信息卡（Weather Card）—— 纯逻辑层（零 React / 零 IPC，可单测）。
 *
 * 职责：
 * - 把两类上游 JSON（wttr.in ?format=j1 / open-meteo 手动经纬度）解析并归一化为统一
 *   WeatherData（WMO 天气码为统一码表，描述文本统一走 wmoText）；
 * - localStorage 缓存读写（30 分钟 TTL，键 variable:weather:v1）；
 * - 配置读写与校验（{enabled, provider, location}）。
 *
 * 设计取舍（规格红线）：
 * - 纯函数：一切副作用（发请求 / 弹授权框）都在组件层，本模块只做解析/校验/存储抽象，
 *   Storage 以接口注入，测试无需真 localStorage；
 * - 不做逐小时预报、不做天气雷达图、不做定位（wttr 用城市名，open-meteo 手动 lat,lon，
 *   绝不读系统定位）；
 * - 出错策略：解析函数对坏输入抛错（由调用方优雅降级），存储函数永不抛。
 */

export type WeatherProvider = "wttr" | "openmeteo";

/** 统一预报日（今日 + 未来两日）。 */
export interface WeatherDay {
  /** YYYY-MM-DD */
  date: string;
  /** WMO 天气码 */
  code: number;
  minC: number;
  maxC: number;
}

/** 统一天气数据。 */
export interface WeatherData {
  provider: WeatherProvider;
  /** 抓取时刻（Date.now()），缓存 TTL 判据 */
  fetchedAt: number;
  /** 当前温度（摄氏） */
  tempC: number;
  /** 当前 WMO 天气码 */
  code: number;
  days: WeatherDay[];
}

export interface WeatherConfig {
  enabled: boolean;
  provider: WeatherProvider;
  /** wttr：城市名（如 Beijing）；openmeteo：「纬度,经度」（如 39.9,116.4） */
  location: string;
}

/** 最小存储抽象（localStorage / 测试桩皆可注入）。 */
export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export const WEATHER_CACHE_KEY = "variable:weather:v1";
export const WEATHER_CFG_KEY = "variable:weather:cfg:v1";
export const WEATHER_TTL_MS = 30 * 60 * 1000;
export const DEFAULT_LOCATION_WTTR = "Beijing";
export const DEFAULT_LOCATION_OM = "39.9,116.4";

export function defaultWeatherConfig(): WeatherConfig {
  return { enabled: false, provider: "wttr", location: DEFAULT_LOCATION_WTTR };
}

// ================= WMO 天气码统一文案 / 图标归类 =================

/** WMO weather code → 简短中文描述（两家供应商共用一套码表，直接归一化）。 */
export function wmoText(code: number): string {
  if (code === 0) return "晴";
  if (code === 1) return "基本晴";
  if (code === 2) return "多云";
  if (code === 3) return "阴";
  if (code === 45 || code === 48) return "雾";
  if (code >= 51 && code <= 57) return "毛毛雨";
  if (code >= 61 && code <= 67) return "雨";
  if (code >= 71 && code <= 77) return "雪";
  if (code >= 80 && code <= 82) return "阵雨";
  if (code === 85 || code === 86) return "阵雪";
  if (code >= 95 && code <= 99) return "雷雨"; // WMO 码表止于 99，区间外的如实归「未知」
  return "未知";
}

/** WMO weather code → 图标类别（组件层据此选 lucide 图标）。 */
export type WeatherIconKind = "sun" | "partly" | "cloud" | "fog" | "rain" | "snow" | "storm";
export function wmoIconKind(code: number): WeatherIconKind {
  if (code === 0 || code === 1) return "sun";
  if (code === 2) return "partly";
  if (code === 3) return "cloud";
  if (code === 45 || code === 48) return "fog";
  if ((code >= 51 && code <= 67) || (code >= 80 && code <= 82)) return "rain";
  if ((code >= 71 && code <= 77) || code === 85 || code === 86) return "snow";
  if (code >= 95) return "storm";
  return "cloud";
}

// ================= 解析归一化 =================

const num = (v: unknown): number | null => {
  const n = typeof v === "string" ? Number(v) : typeof v === "number" ? v : NaN;
  return Number.isFinite(n) ? n : null;
};

const str = (v: unknown): string | null => (typeof v === "string" && v ? v : null);

function pickDay(code: unknown, min: unknown, max: unknown, date: unknown, idx: number): WeatherDay {
  const c = num(code);
  const lo = num(min);
  const hi = num(max);
  const d = str(date);
  if (c === null || lo === null || hi === null || d === null) throw new Error(`weather: bad day entry #${idx}`);
  return { date: d, code: c, minC: lo, maxC: hi };
}

/** wttr.in `?format=j1` JSON → WeatherData。坏结构抛错（调用方降级为占位文本）。 */
export function parseWttr(json: string): WeatherData {
  const raw: unknown = JSON.parse(json);
  if (typeof raw !== "object" || raw === null) throw new Error("weather: wttr payload not an object");
  const r = raw as { current_condition?: unknown; weather?: unknown };
  const cur = Array.isArray(r.current_condition) ? r.current_condition[0] : undefined;
  if (typeof cur !== "object" || cur === null) throw new Error("weather: wttr missing current_condition");
  const c = cur as Record<string, unknown>;
  const tempC = num(c.temp_C);
  const code = num(c.weatherCode);
  if (tempC === null || code === null) throw new Error("weather: wttr bad current fields");
  if (!Array.isArray(r.weather)) throw new Error("weather: wttr missing weather array");
  const days = r.weather.slice(0, 3).map((w, i) => {
    if (typeof w !== "object" || w === null) throw new Error(`weather: wttr bad day #${i}`);
    return pickDay((w as Record<string, unknown>).weatherCode, (w as Record<string, unknown>).mintempC, (w as Record<string, unknown>).maxtempC, (w as Record<string, unknown>).date, i);
  });
  return { provider: "wttr", fetchedAt: Date.now(), tempC, code, days };
}

/** open-meteo（current + daily, forecast_days=3）→ WeatherData。 */
export function parseOpenMeteo(json: string): WeatherData {
  const raw: unknown = JSON.parse(json);
  if (typeof raw !== "object" || raw === null) throw new Error("weather: open-meteo payload not an object");
  const r = raw as { current?: unknown; daily?: unknown };
  const cur = r.current;
  if (typeof cur !== "object" || cur === null) throw new Error("weather: open-meteo missing current");
  const c = cur as Record<string, unknown>;
  const tempC = num(c.temperature_2m);
  const code = num(c.weather_code);
  if (tempC === null || code === null) throw new Error("weather: open-meteo bad current fields");
  const d = r.daily;
  if (typeof d !== "object" || d === null) throw new Error("weather: open-meteo missing daily");
  const dd = d as Record<string, unknown>;
  const time = Array.isArray(dd.time) ? dd.time : [];
  const codes = Array.isArray(dd.weather_code) ? dd.weather_code : [];
  const tmax = Array.isArray(dd.temperature_2m_max) ? dd.temperature_2m_max : [];
  const tmin = Array.isArray(dd.temperature_2m_min) ? dd.temperature_2m_min : [];
  if (time.length === 0) throw new Error("weather: open-meteo empty daily");
  const days = time.slice(0, 3).map((_, i) => pickDay(codes[i], tmin[i], tmax[i], time[i], i));
  return { provider: "openmeteo", fetchedAt: Date.now(), tempC, code, days };
}

// ================= 缓存（30 分钟 TTL） =================

function validWeatherData(v: unknown): v is WeatherData {
  if (typeof v !== "object" || v === null) return false;
  const w = v as Record<string, unknown>;
  if (w.provider !== "wttr" && w.provider !== "openmeteo") return false;
  if (typeof w.fetchedAt !== "number" || !Number.isFinite(w.fetchedAt)) return false;
  if (typeof w.tempC !== "number" || !Number.isFinite(w.tempC)) return false;
  if (typeof w.code !== "number" || !Number.isInteger(w.code)) return false;
  if (!Array.isArray(w.days) || w.days.length === 0) return false;
  return w.days.every(
    (d) =>
      typeof d === "object" && d !== null &&
      typeof (d as WeatherDay).date === "string" &&
      typeof (d as WeatherDay).code === "number" &&
      typeof (d as WeatherDay).minC === "number" &&
      typeof (d as WeatherDay).maxC === "number",
  );
}

/** 读缓存：结构非法或超过 TTL → null（视为未命中，组件据此决定是否重新取数）。 */
export function cacheRead(ls: StorageLike, now: number = Date.now()): WeatherData | null {
  try {
    const raw = ls.getItem(WEATHER_CACHE_KEY);
    if (!raw) return null;
    const v: unknown = JSON.parse(raw);
    if (!validWeatherData(v)) return null;
    if (now - v.fetchedAt >= WEATHER_TTL_MS) return null;
    return v;
  } catch {
    return null;
  }
}

/** 写缓存：永不抛（配额满/序列化失败一律静默丢弃，下次重取）。 */
export function cacheWrite(ls: StorageLike, data: WeatherData): void {
  try {
    ls.setItem(WEATHER_CACHE_KEY, JSON.stringify(data));
  } catch {
    /* 私隐模式/配额满 → 放弃缓存，功能照常 */
  }
}

// ================= 配置 =================

const OM_LOC = /^-?\d{1,2}(\.\d+)?\s*,\s*-?\d{1,3}(\.\d+)?$/;

/** 配置校验：字段非法逐级回退默认；openmeteo 的 location 必须是「纬度,经度」，否则回退默认坐标。 */
export function sanitizeConfig(raw: unknown): WeatherConfig {
  const def = defaultWeatherConfig();
  if (typeof raw !== "object" || raw === null) return def;
  const r = raw as Record<string, unknown>;
  const provider: WeatherProvider = r.provider === "openmeteo" ? "openmeteo" : "wttr";
  let location = typeof r.location === "string" ? r.location.trim().slice(0, 64) : "";
  if (!location) location = provider === "openmeteo" ? DEFAULT_LOCATION_OM : DEFAULT_LOCATION_WTTR;
  if (provider === "openmeteo" && !OM_LOC.test(location)) location = DEFAULT_LOCATION_OM;
  return { enabled: r.enabled === true, provider, location };
}

export function readConfig(ls: StorageLike): WeatherConfig {
  try {
    const raw = ls.getItem(WEATHER_CFG_KEY);
    if (!raw) return defaultWeatherConfig();
    return sanitizeConfig(JSON.parse(raw));
  } catch {
    return defaultWeatherConfig();
  }
}

/** 写配置：永不抛。 */
export function writeConfig(ls: StorageLike, cfg: WeatherConfig): void {
  try {
    ls.setItem(WEATHER_CFG_KEY, JSON.stringify(sanitizeConfig(cfg)));
  } catch {
    /* 存不进去就只影响持久化，本次会话仍生效 */
  }
}

// ================= 请求 URL 构造（纯函数） =================

/** 按配置构造上游 URL（组件层拿它先走 requestNetConsent 再 ipc.httpFetch）。 */
export function weatherUrl(cfg: WeatherConfig): string {
  const c = sanitizeConfig(cfg);
  if (c.provider === "openmeteo") {
    const [lat, lon] = c.location.split(",").map((x) => x.trim());
    return `https://api.open-meteo.com/v1/forecast?latitude=${encodeURIComponent(lat ?? "")}&longitude=${encodeURIComponent(lon ?? "")}&current=temperature_2m,weather_code&daily=weather_code,temperature_2m_max,temperature_2m_min&forecast_days=3&timezone=auto`;
  }
  return `https://wttr.in/${encodeURIComponent(c.location)}?format=j1`;
}
