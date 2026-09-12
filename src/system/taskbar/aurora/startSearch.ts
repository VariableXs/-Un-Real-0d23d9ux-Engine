/**
 * AURORA-10000 领域04 · 族0083 开始菜单搜索（AI-17 批次，勿删）。
 * 即时搜索管线：应用/设置/计算/单位/时区/颜色/emoji/命令前缀 + 拼音与容错。
 */
import { getD4 } from "./prefs";
import { initialsOf as pinyinInitials } from "../../../lib/pinyin";

export interface SearchHit {
  id: string;
  kind: "app" | "setting" | "file" | "command" | "calc" | "unit" | "timezone" | "color" | "emoji";
  title: string;
  subtitle?: string;
  /** 内联答案直接展示（F02055/56/57/59）。 */
  inline?: string;
  score: number;
}

export interface SearchSource {
  apps: readonly { id: string; label: string; pinyin?: string }[];
  settings: readonly { id: string; label: string }[];
  commands: readonly { id: string; label: string }[];
}

/** 简易单位换算表（F02056）。 */
const UNITS: Readonly<Record<string, number>> = {
  m: 1, km: 1000, cm: 0.01, mm: 0.001, mi: 1609.344, ft: 0.3048, in: 0.0254,
  kg: 1, g: 0.001, t: 1000, lb: 0.45359237, oz: 0.028349523,
};

/** 安全四则求值（F02055）：仅数字与 + - * / ( ) 。 */
export function evalCalc(expr: string): number | null {
  if (!/^[\d\s+\-*/().]+$/.test(expr)) return null;
  try {
    // eslint-disable-next-line no-new-func
    const v = Function(`"use strict";return (${expr})`)() as unknown;
    return typeof v === "number" && Number.isFinite(v) ? v : null;
  } catch { return null; }
}

/** 单位换算解析（F02056）：如「3km to ft」。 */
export function unitConvert(q: string): string | null {
  const m = /^([\d.]+)\s*([a-zA-Z]+)\s*(?:to|in|→)\s*([a-zA-Z]+)$/i.exec(q.trim());
  if (!m || m[1] == null || m[2] == null || m[3] == null) return null;
  const v = parseFloat(m[1]);
  const from = UNITS[m[2].toLowerCase()]; const to = UNITS[m[3].toLowerCase()];
  if (from == null || to == null || from === to || !Number.isFinite(v)) return null;
  return `${v * from / to} ${m[3]}`;
}

/** 时区换算（F02057）：如「14:00 CST to UTC」用固定偏移表简化。 */
const TZ_OFFSET: Readonly<Record<string, number>> = { UTC: 0, CST: 8, JST: 9, IST: 5.5, EST: -5, PST: -8, CET: 1 };
export function tzConvert(q: string): string | null {
  const m = /^(\d{1,2}):(\d{2})\s+([A-Za-z]{2,4})\s*(?:to|→)\s*([A-Za-z]{2,4})$/i.exec(q.trim());
  if (!m || m[1] == null || m[2] == null || m[3] == null || m[4] == null) return null;
  const from = TZ_OFFSET[m[3].toUpperCase()]; const to = TZ_OFFSET[m[4].toUpperCase()];
  if (from == null || to == null) return null;
  let mins = parseInt(m[1], 10) * 60 + parseInt(m[2], 10) + Math.round((to - from) * 60);
  mins = ((mins % 1440) + 1440) % 1440;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(Math.floor(mins / 60))}:${pad(mins % 60)} ${m[4].toUpperCase()}`;
}

/** 颜色预览（F02059）。 */
export function colorPreview(q: string): string | null {
  return /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.test(q.trim()) ? q.trim() : null;
}

/** 拼音容错（F02066/67）：标签 / 全拼 / 首字母串命中即算。 */
function matchTerm(label: string, pinyin: string | undefined, q: string): boolean {
  const s = q.toLowerCase();
  if (label.toLowerCase().includes(s)) return true;
  if (!pinyin) return false;
  if (pinyin.toLowerCase().includes(s)) return true;
  return pinyinInitials(pinyin).includes(s);
}

/** 主搜索（F02051/53/55/56/57/59/61/66）。 */
export function searchStart(qRaw: string, src: SearchSource): SearchHit[] {
  const q = qRaw.trim();
  if (!q) return [];
  const hits: SearchHit[] = [];
  // > 前缀命令（F02061）
  if (q.startsWith(">")) {
    const c = q.slice(1).trim().toLowerCase();
    for (const cmd of src.commands) {
      if (cmd.label.toLowerCase().includes(c)) {
        hits.push({ id: cmd.id, kind: "command", title: cmd.label, score: 100 });
      }
    }
    return hits;
  }
  // 内联答案类
  const calc = evalCalc(q);
  if (calc != null && /[+\-*/]/.test(q)) hits.push({ id: "calc", kind: "calc", title: q, inline: String(calc), score: 120 });
  const unit = unitConvert(q);
  if (unit) hits.push({ id: "unit", kind: "unit", title: q, inline: unit, score: 120 });
  const tz = tzConvert(q);
  if (tz) hits.push({ id: "tz", kind: "timezone", title: q, inline: tz, score: 120 });
  const color = colorPreview(q);
  if (color) hits.push({ id: "color", kind: "color", title: q, inline: color, score: 120 });
  // 应用
  if (getD4<boolean>("F02051") ?? true) {
    for (const a of src.apps) {
      if (matchTerm(a.label, a.pinyin, q)) hits.push({ id: a.id, kind: "app", title: a.label, score: 80 });
    }
  }
  // 设置直达
  if (getD4<boolean>("F02053") ?? true) {
    for (const st of src.settings) {
      if (st.label.toLowerCase().includes(q.toLowerCase())) hits.push({ id: st.id, kind: "setting", title: st.label, score: 60 });
    }
  }
  return hits.sort((a, b) => b.score - a.score).slice(0, 20);
}
