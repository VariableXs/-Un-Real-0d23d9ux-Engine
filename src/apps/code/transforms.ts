/**
 * 第四章 · 无 AI 依赖的语义现场编辑引擎（L5 意图变换字典）。
 * 「意图字典 + 模式匹配 + 文本变换」三件套：大白话 → 精确代码改动预览。
 * 首批意图（4.2）：改颜色 / 加日志 / 加注释 / 改数字(端口·超时) / 重命名 /
 * 删行 / 改文字(引号替换) / 加异常处理。模糊同义（4.4）+ 不确定追问（4.5）。
 * 全程纯函数：输出统一 EditPreview 列表 + 新文件全文，由 UI 决定是否落盘。
 * 本模块只做字符串变换，不执行任何代码。
 */

import type { LangId } from "./types";
import { checkBalanced } from "./writeEngine";

export interface EditPreview {
  /** 1-based inclusive */
  startLine: number;
  endLine: number;
  before: string;
  after: string;
  note: string;
}

export interface IntentResult {
  matched: boolean;
  intent: string;
  previews: EditPreview[];
  /** 应用全部预览后的完整文件内容（matched=false 时为空串）。 */
  content: string;
  note: string;
  /** 4.5 追问：不确定时给用户点选的澄清问题。 */
  clarify?: string;
  /** 第二层依赖完整性提示（如"用到了 X，注意引入"）。 */
  warnings: string[];
  /** 第三层语义一致性提示（命名风格冲突等）。 */
  styleNotes: string[];
}

function linesOf(content: string): string[] {
  return content.split("\n");
}

function splice(lines: string[], start: number, end: number, replacement: string[]): string[] {
  return [...lines.slice(0, start - 1), ...replacement, ...lines.slice(end)];
}

function indentOf(line: string): string {
  const m = line.match(/^[ \t]*/);
  return m?.[0] ?? "";
}

/** 4.4 模糊同义触发组。 */
const SYNONYM = {
  color: /改色|变色|换成蓝|改成蓝|变蓝|颜色|colour|color/i,
  log: /日志|打个印|加个打印|输出一下|打印一下|log\b|print\b|console/i,
  comment: /注释|说明一下|解释一下|加个说明|remark/i,
  number: /端口|超时|数量|大小改成|改成\s*\d+|设为\s*\d+|设置为?\s*\d+|timeout|port/i,
  rename: /改名叫|重命名|改名为|叫成|改名/i,
  delete: /删掉|删除|移除|去掉|不要了|删了/i,
  text: /[“"'”][^“"'”]+[“"'”]/,
  tryCatch: /异常|出错|报错|try|catch|失败|万一/i,
};

const COLOR_MAP: Array<[RegExp, string]> = [
  [/蓝色|蓝的|变蓝|blue/i, "#3b82f6"],
  [/红色|红的|red/i, "#ef4444"],
  [/绿色|绿的|green/i, "#22c55e"],
  [/粉色|樱粉|粉的|pink/i, "#f5c6d8"],
  [/白色|白的|white/i, "#f8fafc"],
  [/黑色|黑的|black/i, "#0f172a"],
  [/紫色|紫的|purple/i, "#a78bfa"],
  [/青色|冰青|cyan/i, "#22d3ee"],
  [/灰色|灰的|gray|grey/i, "#94a3b8"],
];

const COLOR_TOKEN = /(#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b|\b(?:red|blue|green|white|black|pink|purple|cyan|gray|grey|orange|yellow)\b)/g;

function hexFromText(text: string): string | null {
  const explicit = text.match(/#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b/);
  if (explicit) return explicit[0]!;
  for (const [re, hex] of COLOR_MAP) {
    if (re.test(text)) return hex;
  }
  return null;
}

function commentToken(lang: LangId): string {
  if (lang === "python" || lang === "ruby" || lang === "shell") return "#";
  if (lang === "sql") return "--";
  if (lang === "html") return "<!--";
  return "//";
}

function closeComment(lang: LangId): string {
  return lang === "html" ? " -->" : "";
}

function logStatement(lang: LangId, message: string): string {
  const msg = message.replace(/"/g, "'");
  if (lang === "python") return `print("${msg}")`;
  if (lang === "rust") return `println!("${msg}");`;
  if (lang === "go") return `fmt.Println("${msg}")`;
  return `console.log("${msg}");`;
}

function tryWrap(lang: LangId, line: string, indent: string): string[] {
  if (lang === "python") {
    return [
      `${indent}try:`,
      `${indent}    ${line.trim()}`,
      `${indent}except Exception as e:`,
      `${indent}    print(f"出错了: {e}")`,
    ];
  }
  return [
    `${indent}try {`,
    `${indent}  ${line.trim()}`,
    `${indent}} catch (err) {`,
    `${indent}  console.error("出错了:", err);`,
    `${indent}}`,
  ];
}

function detectNamingStyle(content: string): "camel" | "snake" | "none" {
  const camel = (content.match(/[a-z][A-Z]/g) ?? []).length;
  const snake = (content.match(/[a-z]_[a-z]/g) ?? []).length;
  if (camel > snake * 2) return "camel";
  if (snake > camel * 2) return "snake";
  return "none";
}

/**
 * 意图字典主入口：把一句大白话变成精确的代码改动预览。
 * @param target.line 选中的目标行（1-based，可选——多数意图强烈依赖它）。
 */
export function applyIntent(
  text: string,
  target: { content: string; lang: LangId; line?: number; relPath?: string },
): IntentResult {
  const lines = linesOf(target.content);
  const warnings: string[] = [];
  const styleNotes: string[] = [];
  const fail = (intent: string, note: string, clarify?: string): IntentResult =>
    ({ matched: false, intent, previews: [], content: "", note, clarify, warnings, styleNotes });

  const ok = (intent: string, previews: EditPreview[], note: string, content: string): IntentResult => {
    // 第三层：命名风格一致性
    const style = detectNamingStyle(target.content);
    const newNames = content.match(/[A-Za-z_$][\w$]*/g) ?? [];
    void newNames;
    if (style === "camel" && /_[a-z]/.test(note) === false) {
      const snakeNew = text.match(/[a-z]+_[a-z]+/);
      if (snakeNew && style === "camel") styleNotes.push(`项目整体用驼峰命名，而「${snakeNew[0]}」是下划线风格——已按你的原话执行，如需统一可再说“重命名”。`);
    }
    // 第一层：语法守卫（括号配平）
    const bal = checkBalanced(content, target.lang);
    if (!bal.ok) {
      return { matched: true, intent, previews, content: "", note: `这一句我看不懂（${bal.detail}），请再说清楚一点`, warnings, styleNotes };
    }
    return { matched: true, intent, previews, content, note, warnings, styleNotes };
  };

  const t = text.trim();
  const at = target.line;

  // ---------- 改颜色 ----------
  if (SYNONYM.color.test(t) && hexFromText(t)) {
    const hex = hexFromText(t)!;
    const previews: EditPreview[] = [];
    const scope = at !== undefined ? [at] : lines.map((_, i) => i + 1);
    let out = lines;
    for (const ln of scope) {
      const line = out[ln - 1] ?? "";
      let changed = false;
      const next = line.replace(COLOR_TOKEN, (m0) => {
        if (m0.toLowerCase() === hex.toLowerCase()) return m0;
        changed = true;
        return hex;
      });
      if (!changed) continue;
      out = splice(out, ln, ln, [next]);
      previews.push({ startLine: ln, endLine: ln, before: line, after: next, note: `颜色 → ${hex}` });
      if (previews.length >= 20) break;
    }
    if (previews.length === 0) return fail("改颜色", "这个文件里没有找到可以改的颜色值");
    return ok("改颜色", previews, `已把 ${previews.length} 处颜色改为 ${hex}`, out.join("\n"));
  }

  // ---------- 加日志 ----------
  if (SYNONYM.log.test(t)) {
    const quoted = t.match(/[“'"]([^“'"]+)[”'"]/);
    const message = quoted?.[1] ?? (at !== undefined ? `第 ${at} 行执行完毕` : "执行到此处");
    const lineNo = at ?? lines.length;
    const anchor = lines[lineNo - 1] ?? "";
    const stmt = `${indentOf(anchor)}${logStatement(target.lang, message)}`;
    const out = splice(lines, lineNo + 1, lineNo, [stmt]);
    return ok("加日志", [{ startLine: lineNo + 1, endLine: lineNo, before: "", after: stmt, note: `在目标行后插入日志` }], `已在第 ${lineNo} 行后插入日志语句`, out.join("\n"));
  }

  // ---------- 加注释 ----------
  if (SYNONYM.comment.test(t)) {
    if (at === undefined) return fail("加注释", "请先选中要解释的那一行", "你想给哪一行加注释？先在画布里点中它再试一次。");
    const anchor = lines[at - 1] ?? "";
    const cmt = `${indentOf(anchor)}${commentToken(target.lang)} 大白话：${t.replace(SYNONYM.comment, "").trim() || "说明"}${closeComment(target.lang)}`;
    const out = splice(lines, at, at - 1, [cmt]);
    return ok("加注释", [{ startLine: at, endLine: at - 1, before: "", after: cmt, note: "在目标行上方插入注释" }], `已在第 ${at} 行上方加注释`, out.join("\n"));
  }

  // ---------- 重命名 ----------
  const rn = t.match(/(?:把|将)?\s*([A-Za-z_$][\w$]*)\s*(?:改名叫|重命名为|改名为|叫成|更名为)\s*([A-Za-z_$][\w$]*)/);
  if (rn && rn[1] && rn[2]) {
    const from = rn[1]!;
    const to = rn[2]!;
    if (!new RegExp(`\\b${from}\\b`).test(target.content)) {
      return fail("重命名", `文件里没有找到 ${from}`);
    }
    const previews: EditPreview[] = [];
    const out = lines.map((line, i) => {
      if (!line.includes(from)) return line;
      let next = "";
      let rest = line;
      let pos = rest.indexOf(from);
      while (pos !== -1) {
        const prevOk = pos === 0 || !/[A-Za-z0-9_$]/.test(rest[pos - 1]!);
        const end = pos + from.length;
        const nextOk = end >= rest.length || !/[A-Za-z0-9_$]/.test(rest[end]!);
        if (prevOk && nextOk) {
          next += rest.slice(0, pos) + to;
          rest = rest.slice(end);
          if (previews.length < 10) previews.push({ startLine: i + 1, endLine: i + 1, before: line, after: "(见后)", note: `${from} → ${to}` });
          pos = rest.indexOf(from);
        } else {
          next += rest.slice(0, pos + from.length);
          rest = rest.slice(pos + from.length);
          pos = rest.indexOf(from);
        }
      }
      return next + rest;
    });
    return ok("重命名", previews, `已把 ${from} 全部改名为 ${to}（同步所有引用）`, out.join("\n"));
  }

  // ---------- 删行 ----------
  if (SYNONYM.delete.test(t)) {
    const n = t.match(/第\s*(\d+)\s*行/);
    const lineNo = n ? Number(n[1]) : at;
    if (lineNo === undefined) return fail("删行", "请先选中要删除的行，或写明“删除第 N 行”", "要删哪一行？点中它再试一次。");
    const removed = lines[lineNo - 1] ?? "";
    const out = splice(lines, lineNo, lineNo, []);
    return ok("删行", [{ startLine: lineNo, endLine: lineNo, before: removed, after: "", note: "整行删除" }], `已删除第 ${lineNo} 行`, out.join("\n"));
  }

  // ---------- 改文字（引号内容替换） ----------
  const sub = t.match(/[“'"]([^“'”"]+)[”'”"].{0,8}?(?:改成|换成|变为)\s*[“'"]([^“'”"]+)[”'”"]/);
  if (sub && sub[1] && sub[2]) {
    const fromStr = sub[1]!;
    const toStr = sub[2]!;
    let replaced = 0;
    const out = lines.map((line) => {
      if (!line.includes(fromStr) || replaced >= 10) return line;
      replaced++;
      const next = line.split(fromStr).join(toStr);
      return next;
    });
    if (replaced === 0) return fail("改文字", `文件里没有找到“${fromStr}”`);
    const previews: EditPreview[] = [];
    let seen = 0;
    out.forEach((line, i) => {
      if (line.includes(toStr) && lines[i] !== line && seen < 10) {
        seen++;
        previews.push({ startLine: i + 1, endLine: i + 1, before: lines[i] ?? "", after: line, note: `“${fromStr}” → “${toStr}”` });
      }
    });
    return ok("改文字", previews, `已把“${fromStr}”替换为“${toStr}”`, out.join("\n"));
  }

  // ---------- 改数字（端口 / 超时 / 指定行数值） ----------
  if (SYNONYM.number.test(t)) {
    const num = t.match(/\d+/);
    if (!num) return fail("改数字", "没有看到目标数值，例如“把端口改成 8080”");
    const keyword = /端口|port/i.test(t) ? "port" : /超时|timeout/i.test(t) ? "timeout" : null;
    const candidates = keyword
      ? lines.map((l, i) => ({ l, i })).filter(({ l }) => new RegExp(`${keyword}`, "i").test(l))
      : at !== undefined ? [{ l: lines[at - 1] ?? "", i: at - 1 }] : [];
    if (candidates.length === 0) return fail("改数字", keyword ? `文件里没有找到与 ${keyword} 相关的配置` : "请先选中要改的那一行");
    const previews: EditPreview[] = [];
    let out = lines;
    for (const { i } of candidates.slice(0, 5)) {
      const line = out[i] ?? "";
      const next = line.replace(/\d+/, num[0]!);
      if (next !== line) {
        out = splice(out, i + 1, i + 1, [next]);
        previews.push({ startLine: i + 1, endLine: i + 1, before: line, after: next, note: `数值 → ${num[0]}` });
      }
    }
    if (previews.length === 0) return fail("改数字", "那一行没有可替换的数字");
    return ok("改数字", previews, `已把数值改为 ${num[0]}`, out.join("\n"));
  }

  // ---------- 加异常处理 ----------
  if (SYNONYM.tryCatch.test(t)) {
    if (at === undefined) return fail("加异常处理", "请先选中要加保险的行", "要给哪一行加异常处理？点中它再试一次。");
    const anchor = lines[at - 1] ?? "";
    const wrapped = tryWrap(target.lang, anchor, indentOf(anchor));
    const out = splice(lines, at, at, wrapped);
    return ok("加异常处理", [{ startLine: at, endLine: at, before: anchor, after: wrapped.join("\n"), note: "包一层 try/except 保险丝" }], `已为第 ${at} 行加上异常处理`, out.join("\n"));
  }

  // ---------- 4.5 追问 / 未命中 ----------
  if (/变大|变大点| bigger/i.test(t)) {
    return fail("改尺寸", "", "你是想让【字体】变大，还是让【控件尺寸】变大？说清楚一点，比如“字体设成 18px”。");
  }
  return fail(
    "未识别",
    "",
    "我还不确定你想改什么。可以试试：改颜色 / 加日志 / 加注释 / 改数字（端口·超时）/ 重命名 / 删掉这一行 / 改文字 / 加异常处理。",
  );
}
