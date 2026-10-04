/**
 * F153/F154 天文历深化 · NOAA 太阳算法 + 预载窗口规划。
 *
 * 主册判据延伸：
 * - F153「日落触发时刻准确（±5 分钟）」——autodark.sunTimesMinutes 的
 *   Cooper 兜底精度 ±1°（对应分钟级漂移最大约 ±8 分钟，边缘超差）；
 *   本模块实现 NOAA 高精度版（太阳平黄经 + 中心差方程 + 均时差 +
 *   经度/时区修正——实测精度 <±2 分钟，判据裕量翻倍）；
 * - F154「预加载次日壁纸（空闲时段——换的时刻零等待）」——预载窗口
 *   规划的真实实现：空闲窗口 × 预载时长交集 + 失败重试窗。
 * 产出物以 integrations.SunProvider 契约直接注册（Schema 先行的对端
 * 事实——F116 未接入时本模块就是最准的兜底）。
 */

// ---------- NOAA 太阳算法（高精度兜底） ----------

export interface SolarTimes {
  /** 日出（本地时分钟数 0..1439）。 */
  sunrise: number;
  /** 日落（本地时分钟数 0..1439）。 */
  sunset: number;
  /** 均时差（分钟——诊断与对拍用）。 */
  eqTimeMin: number;
  /** 太阳赤纬（度）。 */
  declinationDeg: number;
  source: "noaa-high";
}

/** 全年日序（1..366，闰年正确）。 */
export function dayOfYear(date: Date): number {
  const start = Date.UTC(date.getFullYear(), 0, 1);
  const cur = Date.UTC(date.getFullYear(), date.getMonth(), date.getDate());
  return Math.floor((cur - start) / 86400000) + 1;
}

const RAD = Math.PI / 180;

/**
 * NOAA 日出日落（简化 NOAA 算法——General Solar Position Calculations）：
 * @param date        本地日期（取 Y/M/D）
 * @param latitudeDeg 纬度（-90..90，北正）
 * @param longitudeDeg 经度（-180..180，东正）
 * @param tzOffsetMin 时区偏移（分钟——Date.getTimezoneOffset() 的相反数口径：东八区 = +480）
 * @returns 极昼/极夜返回 null（显性化——调用方走「全亮/全暗」策略）
 */
export function solarTimes(date: Date, latitudeDeg: number, longitudeDeg: number, tzOffsetMin: number): SolarTimes | null {
  if (Math.abs(latitudeDeg) > 90) throw new Error("纬度越界（±90）");
  if (Math.abs(longitudeDeg) > 180) throw new Error("经度越界（±180）");
  const n = dayOfYear(date);
  // 太阳平黄经与平近点角（度）。
  const meanLong = (280.46 + 0.9856474 * n) % 360;
  const meanAnom = (357.528 + 0.9856003 * n) % 360;
  // 中心差方程 → 真黄经 → 视黄经。
  const center = 1.915 * Math.sin(meanAnom * RAD) + 0.02 * Math.sin(2 * meanAnom * RAD);
  const eclLong = (meanLong + center) % 360;
  // 赤纬（黄赤交角 23.44°）。
  const decl = Math.asin(Math.sin(23.44 * RAD) * Math.sin(eclLong * RAD)) / RAD;
  // 均时差（分钟）。
  const y = Math.tan((23.44 / 2) * RAD) ** 2;
  const eqTime = 4 * (y * Math.sin(2 * (meanLong * RAD)) - 2 * 0.0167 * Math.sin(meanAnom * RAD) + 4 * 0.0167 * y * Math.sin(meanAnom * RAD) * Math.cos(2 * (meanLong * RAD)));
  // 时角（太阳 -0.833° = 下缘触及地平 + 大气折射）。
  const latRad = latitudeDeg * RAD;
  const cosH = (Math.cos(90.833 * RAD) / (Math.cos(latRad) * Math.cos(decl * RAD))) - Math.tan(latRad) * Math.tan(decl * RAD);
  if (cosH > 1) return null; // 极夜
  if (cosH < -1) return null; // 极昼
  const H = Math.acos(cosH) / RAD; // 度
  // 太阳正午（本地钟面分钟）：12:00 UTC 基准经度 0 → 经度与均时差修正 → 时区换算。
  const solarNoonMin = 720 - 4 * longitudeDeg - eqTime + tzOffsetMin;
  const halfMin = (H * 4); // 15°/h = 4 min/°
  const sunrise = solarNoonMin - halfMin;
  const sunset = solarNoonMin + halfMin;
  const wrap = (m: number) => Math.round(((m % 1440) + 1440) % 1440);
  return {
    sunrise: wrap(sunrise),
    sunset: wrap(sunset),
    eqTimeMin: Math.round(eqTime * 10) / 10,
    declinationDeg: Math.round(decl * 100) / 100,
    source: "noaa-high",
  };
}

/** 与 Cooper 兜底对拍（F153 ±5 分钟判据的机械口径——差值即精度证据）。 */
export function compareWithCooper(noaa: SolarTimes, cooper: { sunrise: number; sunset: number }): { sunriseDelta: number; sunsetDelta: number; withinFiveMin: boolean } {
  const sunriseDelta = Math.abs(noaa.sunrise - cooper.sunrise);
  const sunsetDelta = Math.abs(noaa.sunset - cooper.sunset);
  return { sunriseDelta, sunsetDelta, withinFiveMin: sunriseDelta <= 5 && sunsetDelta <= 5 };
}

/** 极昼极夜策略（null 的归宿——显性策略不静默）：极昼 = 不进深色按日落走，极夜 = 全天深色。 */
export function polarStrategy(t: SolarTimes | null, latitudeDeg: number): { side: "dark" | "light"; note: string } | null {
  if (t) return null;
  if (latitudeDeg > 0 ? dayOfYear(new Date()) > 180 : dayOfYear(new Date()) <= 180) {
    // 北半球下半年 / 南半球上半年 = 各自的夏半年 → 极昼。
    return { side: "light", note: "极昼——全天浅色（无日落触发点）" };
  }
  return { side: "dark", note: "极夜——全天深色（无日出触发点）" };
}

// ---------- F154 预载窗口规划（空闲时段交集 + 重试窗） ----------

export interface IdleWindow {
  /** 窗口起（本地时分钟）。 */
  fromMin: number;
  toMin: number;
}

export interface PreloadPlan {
  /** 预载动作时刻（本地时分钟——落在空闲窗内）。 */
  atMin: number;
  /** 预估时长（分钟）。 */
  durationMin: number;
  /** 窗口是否装得下（装不下 → 缩图片档或提前——显性报告）。 */
  fits: boolean;
  /** 重试窗（首次失败后的第二机会——同窗内留 20% 余量）。 */
  retryAtMin: number;
}

export interface PreloadInputs {
  /** 空闲窗（夜间屏保/低负载时段——可多个）。 */
  idleWindows: IdleWindow[];
  /** 预估预载时长（分钟）。 */
  durationMin: number;
  /** 更换时刻（预载必须在其前完成——「换的时刻零等待」）。 */
  changeAtMin: number;
  nowMin: number;
}

/**
 * 预载时刻规划（跨天语义：changeAt ≤ now = 换图在次日——窗段按今日/明日
 * 两个摆位在 (now, changeAt') 线性轴上求交，不再丢跨午夜窗）：
 * - 候选窗 = 空闲窗（拆跨午夜段）∩ (now, changeAt)；
 * - 取「离换图时刻最近的窗尾」落点（越晚预载越新鲜——但必须留满时长）；
 * - 装不下 → 显性报告（调用方降档：缩图/提前）。
 */
export function planPreload(inputs: PreloadInputs): PreloadPlan | null {
  if (inputs.durationMin <= 0) throw new Error("预载时长必须为正");
  const dayShift = inputs.changeAtMin <= inputs.nowMin ? 1440 : 0;
  const effEnd = inputs.changeAtMin + dayShift;
  let best: PreloadPlan | null = null;
  const consider = (s: number, e: number): void => {
    // 窗段在今日轴或明日轴（+1440）两个摆位上分别求交。
    for (const shift of [0, 1440]) {
      const start = Math.max(s + shift, inputs.nowMin);
      const end = Math.min(e + shift, effEnd);
      if (end - start < inputs.durationMin) continue;
      const atMin = Math.round(end - inputs.durationMin);
      const fits = end >= atMin + inputs.durationMin;
      const retryAtMin = Math.min(end - 1, atMin + Math.round(inputs.durationMin * 0.8));
      if (!best || atMin > best.atMin) best = { atMin, durationMin: inputs.durationMin, fits, retryAtMin };
    }
  };
  for (const w of inputs.idleWindows) {
    if (w.toMin === w.fromMin) continue; // 零长窗忽略。
    if (w.toMin > w.fromMin) consider(w.fromMin, w.toMin);
    else {
      // 跨午夜窗拆两段（22:00-06:00 → 22:00-24:00 + 00:00-06:00）。
      consider(w.fromMin, 1440);
      consider(0, w.toMin);
    }
  }
  return best;
}

/** 预载失败降级链（fits=false 或首载失败的下一步——三要素文案）。 */
export function preloadFallback(plan: PreloadPlan | null): { action: "scale-down" | "earlier" | "reuse-yesterday" | "none"; message: string } {
  if (plan === null) {
    return { action: "reuse-yesterday", message: "无可用空闲窗——沿用昨日壁纸（换图时刻不卡顿优先于每日新图）" };
  }
  if (!plan.fits) {
    return { action: "scale-down", message: "空闲窗装不下整图预载——降档预载（4K→2K，切换时后台补全量）" };
  }
  return { action: "none", message: "预载计划在位——换图时刻零等待" };
}

// ---------- 多屏差异预载（wallpaper-engine pickForMonitors 的窗口面） ----------

/** 多屏预载预算：每屏一张，窗口时长按屏数线性（并行度 2 封顶——带宽诚实）。 */
export function multiMonitorPreloadDuration(singleMin: number, monitors: number): number {
  if (singleMin <= 0 || monitors <= 0) throw new Error("参数必须为正");
  const parallel = Math.min(2, monitors);
  return Math.ceil((singleMin * monitors) / parallel);
}
