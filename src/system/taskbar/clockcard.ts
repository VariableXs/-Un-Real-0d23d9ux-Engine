/**
 * M-12 时钟多时区与细节（AI-03 任务栏与托盘组）：
 * 时钟悬停卡：多时区（≤3，用户配置 IANA）+ ISO 8601 周数 + 今日未读计数（只读消费通知中心）。
 * 不做世界时钟地图、不做会议时间换算器（换算是 Z-26 的领地）。
 */

export const CLOCK_ZONES_MAX = 3;

/** IANA 时区名做最小校验（Area/Location 形态或 GMT 偏移）；防误配。 */
export function sanitizeClockZones(raw: unknown): string[] {
  if (!Array.isArray(raw)) return [];
  const out: string[] = [];
  for (const r of raw) {
    if (typeof r !== "string") continue;
    const z = r.trim();
    if (!z) continue;
    if (!/^[A-Za-z_+-]+(\/[A-Za-z_+-]+){0,2}$/.test(z)) continue;
    if (!out.includes(z)) out.push(z);
    if (out.length >= CLOCK_ZONES_MAX) break;
  }
  return out;
}

/** ISO 8601 周数（周一为一周起点；含年初/年末归属边界）。 */
export function isoWeek(d: Date): number {
  const t = new Date(Date.UTC(d.getFullYear(), d.getMonth(), d.getDate()));
  const day = t.getUTCDay() || 7; // 1..7，周一=1
  t.setUTCDate(t.getUTCDate() + 4 - day); // 本周的周四
  const yearStart = new Date(Date.UTC(t.getUTCFullYear(), 0, 1));
  return Math.ceil(((t.getTime() - yearStart.getTime()) / 86_400_000 + 1) / 7);
}

/** 按时区渲染时间（夏令时由 Intl 自动处理；非法时区返回 null，如实降级）。 */
export function timeInZone(d: Date, zone: string, hour12 = false): string | null {
  try {
    return new Intl.DateTimeFormat("en-GB", {
      timeZone: zone,
      hour: "2-digit",
      minute: "2-digit",
      hour12,
    }).format(d);
  } catch {
    return null;
  }
}
