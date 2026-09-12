/**
 * AURORA-10000 领域04 · 族0099 任务栏本地化（AI-20 批次，勿删）。
 * 格式化/12-24 制/回退链/伪本地化/RTL/历法联动（复用 lib/lunar）。
 */

export interface LocalePrefs {
  lang: "zh" | "zh-TW" | "en";
  hour12: boolean;
  /** 千分位（F02455）。 */
  grouping: boolean;
  /** 日期样式（F02456）。 */
  dateStyle: "iso" | "cn" | "us";
  /** RTL 镜像（F02453 测试模式）。 */
  rtl: boolean;
  /** 伪本地化（F02461）。 */
  pseudo: boolean;
}

/** 数字格式（F02455）。 */
export function formatNumber(n: number, prefs: LocalePrefs): string {
  const s = prefs.grouping ? n.toLocaleString("en-US") : String(n);
  return prefs.pseudo ? pseudo(s) : s;
}

/** 日期格式（F02456）。 */
export function formatDate(d: Date, prefs: LocalePrefs): string {
  const pad = (x: number) => String(x).padStart(2, "0");
  const iso = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  const cn = `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`;
  const us = `${pad(d.getMonth() + 1)}/${pad(d.getDate())}/${d.getFullYear()}`;
  const raw = prefs.dateStyle === "cn" ? cn : prefs.dateStyle === "us" ? us : iso;
  return prefs.pseudo ? pseudo(raw) : raw;
}

/** 回退链（F02459）：zh → zh-TW → en 逐级查找。 */
export const FALLBACK_CHAIN: readonly string[] = ["zh", "zh-TW", "en"];
export function lookupKey(bundles: Readonly<Record<string, string | undefined>>, key: string, lang: string): string | undefined {
  const chain = FALLBACK_CHAIN.slice(FALLBACK_CHAIN.indexOf(lang));
  for (const l of chain) {
    const v = bundles[`${l}:${key}`];
    if (v != null) return v;
  }
  return bundles[`en:${key}`];
}

/** 伪本地化（F02461）：方括号包裹 + 元音替换。 */
export function pseudo(s: string): string {
  return `[${s.replace(/[aeiou]/g, (c) => c + c)}]`;
}

/** RTL 镜像标记（F02453/54）：仅布局方向。 */
export function dirAttr(prefs: LocalePrefs): "rtl" | "ltr" { return prefs.rtl ? "rtl" : "ltr"; }

/** 夏令时（F02471）：用 Intl 判断指定时区当前是否 DST。 */
export function isDst(timeZone: string, date = new Date()): boolean {
  const fmt = new Intl.DateTimeFormat("en-US", { timeZone, timeZoneName: "shortOffset" });
  const name = fmt.formatToParts(date).find((p) => p.type === "timeZoneName")?.value ?? "";
  return /DT$/.test(name);
}

/** 复数规则（F02464）：最简 en 复数类。 */
export function plural(n: number, one: string, many: string): string { return n === 1 ? one : many; }
