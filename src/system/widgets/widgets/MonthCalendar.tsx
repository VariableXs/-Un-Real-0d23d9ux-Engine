/** N-09 本日历：当月网格，今日高亮，零数据依赖。 */
import { useState } from "react";
import { LABELS, useLaneLang } from "../labels";
import { useRefresh } from "./common";

export default function MonthCalendar({ paused }: { paused: boolean }): React.ReactElement {
  const lang = useLaneLang();
  const t = LABELS[lang];
  const [, tick] = useState(0);
  useRefresh(() => tick((n) => n + 1), 600_000, paused);

  const now = new Date();
  const year = now.getFullYear();
  const month = now.getMonth();
  const first = new Date(year, month, 1).getDay();
  const days = new Date(year, month + 1, 0).getDate();
  const title = lang === "en"
    ? now.toLocaleDateString("en-US", { month: "long", year: "numeric" })
    : `${year}年${month + 1}月`;

  return (
    <div className="wgt-cal">
      <div className="wgt-cal-title">{title}</div>
      <div className="wgt-cal-grid">
        {(lang === "en" ? ["S", "M", "T", "W", "T", "F", "S"] : ["日", "一", "二", "三", "四", "五", "六"]).map((d, i) => (
          <span key={`${d}${i}`} className="wgt-cal-wd">{d}</span>
        ))}
        {Array.from({ length: first }, (_, i) => (
          <span key={`pad${i}`} />
        ))}
        {Array.from({ length: days }, (_, i) => (
          <span key={i} className={`wgt-cal-day ${i + 1 === now.getDate() ? "today" : ""}`}>
            {i + 1}
          </span>
        ))}
      </div>
      <div className="visually-hidden">{t.calendar}</div>
    </div>
  );
}