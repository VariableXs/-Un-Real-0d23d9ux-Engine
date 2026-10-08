import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";

/**
 * U-18 迷你应用：世界时钟条带。
 * - 3-4 城市并排，秒级跳动（1s setInterval 重渲染一次，迷你窗口内开销可忽略）
 * - UTC 偏移实时刷新：Intl.DateTimeFormat(timeZoneName: "longOffset") 按「当前
 *   时刻」解析目标时区偏移，夏令时切换自然生效（无需手写规则表）
 * - 设计取舍：偏移展示走 Intl 结果（GMT+08:00 → +8），不自己做日期运算，
 *   避免 DST 边界（春季跳变 23/25 小时日）算错。
 */

interface City {
  id: string;
  tz: string;
  nameKey: string;
}

const CITIES: City[] = [
  { id: "beijing", tz: "Asia/Shanghai", nameKey: "miniClockBeijing" },
  { id: "london", tz: "Europe/London", nameKey: "miniClockLondon" },
  { id: "newyork", tz: "America/New_York", nameKey: "miniClockNewYork" },
  { id: "tokyo", tz: "Asia/Tokyo", nameKey: "miniClockTokyo" },
];

/** 解析城市时间与 UTC 偏移（如 +8 / -4 / +5:30）。 */
function cityParts(now: Date, tz: string): { time: string; offset: string } {
  try {
    const parts = new Intl.DateTimeFormat("en-GB", {
      timeZone: tz,
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
      timeZoneName: "longOffset",
    }).formatToParts(now);
    const h = parts.find((p) => p.type === "hour")?.value ?? "00";
    const m = parts.find((p) => p.type === "minute")?.value ?? "00";
    const s = parts.find((p) => p.type === "second")?.value ?? "00";
    const raw = parts.find((p) => p.type === "timeZoneName")?.value ?? "GMT"; // "GMT+08:00" | "GMT" | "GMT+5:30"
    const mm = /^GMT([+-])(\d{2}):(\d{2})$/.exec(raw);
    let offset = "0";
    if (mm) {
      const [, sign, hh, mo] = mm;
      offset = mo === "00" ? `${sign}${Number(hh)}` : `${sign}${Number(hh)}:${mo}`;
    }
    return { time: `${h}:${m}:${s}`, offset };
  } catch {
    // 时区数据不可用（极端环境兜底）：时间未知、偏移不显示
    return { time: "--:--:--", offset: "" };
  }
}

export function MiniWorldClock(): React.ReactElement {
  const { t } = useI18n();
  const [now, setNow] = useState(() => new Date());

  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 1000);
    return () => window.clearInterval(id);
  }, []);

  return (
    <div className="mini-clock" role="timer">
      {CITIES.map((c) => {
        const { time, offset } = cityParts(now, c.tz);
        return (
          <div key={c.id} className="mini-clock-city">
            <div className="mini-clock-name small dim">{t(c.nameKey)}</div>
            <div className="mini-clock-time">{time}</div>
            <div className="mini-clock-off small dim">{offset ? `UTC${offset}` : "—"}</div>
          </div>
        );
      })}
    </div>
  );
}
