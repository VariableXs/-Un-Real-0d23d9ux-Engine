/**
 * N-11 昼夜引擎（纯函数，功能全景 L899-901）。
 *
 * - sunPhase：晨蓝→正午白→暮金→夜靛四段色温。
 *   无经纬度：按本机时钟粗分（6-11 晨 / 11-16 午 / 16-19 暮 / 其余夜）；
 *   有纬度（可选手填）：NOAA 近似式算日出日落，再切四段 —— 近似口径，
 *   不引入天文库（诚实边界：极圈/高纬有极昼极夜时回退时钟粗分）。
 * - seasonAccent：四季 OKLCH hue 偏移（春芽/夏碧/秋赭/冬霜），
 *   仅装饰语义（--scene-accent-shift），不改可读性 token（L901）。
 */

export type SunPhase = "dawn" | "noon" | "dusk" | "night";

/** 四段色温 tint（hex，主色；遮罩透明度见 PHASE_MASK_ALPHA）。 */
export const PHASE_TINT: Record<SunPhase, string> = {
  dawn: "#7fa8e6", // 晨蓝
  noon: "#ffffff", // 正午白
  dusk: "#e6a35a", // 暮金
  night: "#232a5c", // 夜靛
};

/** 各段遮罩透明度（夜靛最重，正午几乎不可见）。 */
export const PHASE_MASK_ALPHA: Record<SunPhase, number> = {
  dawn: 0.14,
  noon: 0.05,
  dusk: 0.16,
  night: 0.22,
};

export type Season = "spring" | "summer" | "autumn" | "winter";

/** OKLCH hue 偏移（deg）：作用于 accent 的装饰性旋转量。 */
export const SEASON_HUE_SHIFT: Record<Season, number> = {
  spring: 25, // 春芽
  summer: 70, // 夏碧
  autumn: -35, // 秋赭
  winter: 150, // 冬霜
};

function hoursOf(d: Date): number {
  return d.getHours() + d.getMinutes() / 60 + d.getSeconds() / 3600;
}

/** 时钟粗分（无经纬度口径，规格 L902 的 6-11/11-16/16-19/其余）。 */
function phaseByClock(h: number): SunPhase {
  if (h >= 6 && h < 11) return "dawn";
  if (h >= 11 && h < 16) return "noon";
  if (h >= 16 && h < 19) return "dusk";
  return "night";
}

/**
 * 太阳相位。lat 缺省/非法/极昼极夜 → 回退时钟粗分（确定性可单测）。
 */
export function sunPhase(now: Date, lat?: number | null): { phase: SunPhase; tint: string } {
  const h = hoursOf(now);
  if (typeof lat !== "number" || !Number.isFinite(lat) || Math.abs(lat) > 90) {
    return { phase: phaseByClock(h), tint: PHASE_TINT[phaseByClock(h)] };
  }
  const rad = Math.PI / 180;
  const doy = Math.floor(
    (now.getTime() - new Date(now.getFullYear(), 0, 0).getTime()) / 86_400_000,
  );
  const decl = 23.44 * Math.sin((2 * Math.PI / 365) * (284 + doy)) * rad;
  const cosH = -Math.tan(lat * rad) * Math.tan(decl);
  if (cosH > 1) return { phase: "night", tint: PHASE_TINT.night }; // 极夜
  if (cosH < -1) return { phase: "noon", tint: PHASE_TINT.noon }; // 极昼
  const halfDay = Math.acos(cosH) / rad / 15; // 日出正午间隔（小时）
  const sunrise = 12 - halfDay;
  const sunset = 12 + halfDay;
  // 四段窗口：晨 [日出-1.5, 日出+3)，午 [日出+3, 日落-3)，暮 [日落-3, 日落+1.5)，夜其余
  let phase: SunPhase;
  if (h >= sunrise - 1.5 && h < sunrise + 3) phase = "dawn";
  else if (h >= sunrise + 3 && h < sunset - 3) phase = "noon";
  else if (h >= sunset - 3 && h < sunset + 1.5) phase = "dusk";
  else phase = "night";
  return { phase, tint: PHASE_TINT[phase] };
}

/** hex → rgba() 遮罩串（--scene-tint 消费）。 */
export function tintToRgba(hex: string, alpha: number): string {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex);
  if (!m) return `rgba(0,0,0,${alpha})`;
  return `rgba(${parseInt(m[1] as string, 16)},${parseInt(m[2] as string, 16)},${parseInt(m[3] as string, 16)},${alpha})`;
}

/** 季节 accent 偏移（月份口径：3-5 春 / 6-8 夏 / 9-11 秋 / 其余冬）。 */
export function seasonAccent(now: Date): { season: Season; hueShift: number } {
  const m = now.getMonth() + 1;
  const season: Season = m >= 3 && m <= 5 ? "spring" : m >= 6 && m <= 8 ? "summer" : m >= 9 && m <= 11 ? "autumn" : "winter";
  return { season, hueShift: SEASON_HUE_SHIFT[season] };
}