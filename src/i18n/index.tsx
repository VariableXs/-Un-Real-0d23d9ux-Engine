﻿import { createContext, useContext } from "react";
import { translate, type Lang } from "./dictionaries";

export interface I18nCtx {
  lang: Lang;
  setLang: (l: Lang) => void;
  t: (key: string, params?: Record<string, string | number>) => string;
}

export const I18nContext = createContext<I18nCtx>({
  lang: "zh",
  setLang: () => {},
  t: (k) => k,
});

export function useI18n(): I18nCtx {
  return useContext(I18nContext);
}

export function makeT(lang: Lang): I18nCtx["t"] {
  return (key, params) => translate(lang, key, params);
}

export const WEEKDAYS: Record<Lang, string[]> = {
  en: ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"],
  zh: ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"],
  "zh-TW": ["週日", "週一", "週二", "週三", "週四", "週五", "週六"],
};

export const MONTHS: Record<Lang, string[]> = {
  en: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
  zh: ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
  "zh-TW": ["1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月"],
};

export function formatLongDate(ts: number | Date, lang: Lang): string {
  const d = ts instanceof Date ? ts : new Date(ts);
  const wd = WEEKDAYS[lang][d.getDay()];
  const mo = MONTHS[lang][d.getMonth()];
  if (lang !== "en") return `${d.getFullYear()}年${mo}${d.getDate()}日 · ${wd}`;
  return `${wd}, ${mo} ${d.getDate()}`;
}

export function formatClock(d: Date): string {
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function formatDateTime(ts: number, lang: Lang): string {
  const d = new Date(ts);
  const pad = (n: number) => String(n).padStart(2, "0");
  const date = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const time = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  return lang !== "en" ? `${date} ${time}` : `${date} ${time}`;
}
