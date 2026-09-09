import { solarToLunar, lunarSummary, solarTermOfDay, festivalOfDay, getLunarMonthTable } from "../src/lib/lunar";

const at = (y: number, m: number, d: number): Date => new Date(Date.UTC(y, m - 1, d, 4)); // 北京中午 = UTC 04:00

const cases: [number, number, number, string][] = [
  [2025, 1, 29, "春节: 2025-01-29 应为 正月初一"],
  [2023, 1, 22, "春节: 2023-01-22 正月初一"],
  [2020, 1, 25, "春节: 2020-01-25 正月初一"],
  [2028, 1, 27, "春节: 2028-01-27 正月初一"],
  [2025, 10, 6, "中秋: 2025-10-06 八月十五"],
  [2025, 7, 25, "2025-07-25 应为 闰六月初一"],
  [2023, 3, 22, "2023-03-22 应为 闰二月初一"],
  [2020, 5, 23, "2020-05-23 应为 闰四月初一"],
  [2028, 6, 24, "2028-06-24 应为 闰五月初一"],
  [2024, 12, 21, "冬至 2024-12-21"],
];
for (const [y, m, d, label] of cases) {
  const l = solarToLunar(at(y, m, d));
  console.log(label, "→", l ? (l.leap ? "闰" : "") + l.month + "月" + l.day + "日 (" + l.year + ")" : "null");
}
console.log("立春2025:", solarTermOfDay(at(2025, 2, 3)), "| 清明2025:", solarTermOfDay(at(2025, 4, 4)), "| 冬至2025:", solarTermOfDay(at(2025, 12, 21)), "| 夏至2025:", solarTermOfDay(at(2025, 6, 21)));
console.log("节日 2025-10-06:", festivalOfDay(at(2025, 10, 6)), "| 2025-01-29:", festivalOfDay(at(2025, 1, 29)), "| 2025-10-01:", festivalOfDay(at(2025, 10, 1)));
console.log("summary 2025-07-25:", JSON.stringify(lunarSummary(at(2025, 7, 25))));
const t = getLunarMonthTable();
console.log("rows:", t.length, "| first:", JSON.stringify(t[0]), "| last:", JSON.stringify(t[t.length - 1]));
