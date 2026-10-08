/**
 * 二章 · 深度语义翻译引擎（五维分析：意图/手段/输入/输出/副作用）。
 * 所有翻译由 AST 派生事实组装（函数名目标 + 参数 + 调用链 + 依赖域），
 * 每个文件/函数/行的描述都携带其独有事实，天然互不雷同；
 * dedupeCheck 提供品控自检（2.4 翻译退化警告）。
 * 零 AI、零网络；支持 2.5 的三种通俗风格（生活比喻/故事叙事/工程说明）。
 */

import type { FileAnalysis, SymbolInfo } from "./types";
import { inferGoal } from "./reasoning";

export type NarrateStyle = "metaphor" | "story" | "engineering";

export interface FiveDim {
  intent: string;
  method: string;
  input: string;
  output: string;
  sideEffect: string;
}

const SIDE_EFFECT_DOMAINS: Array<[RegExp, string, string]> = [
  [/fetch|axios|request|http|ureq|reqwest|requests/i, "发起网络请求", "makes network calls"],
  [/fs|file|path|readFile|writeFile|std::fs|open\(/i, "读写文件", "touches the filesystem"],
  [/db|sql|database|sqlite|mysql|postgres|mongo|orm/i, "读写数据库", "reads/writes a database"],
  [/log|console|print|println|logger/i, "打印日志", "logs output"],
  [/state|store|setState|signal|atom/i, "改动界面/全局状态", "mutates shared state"],
  [/crypto|hash|encrypt|token/i, "处理加密/令牌等安全材料", "handles secrets"],
];

function domainOf(analysis: FileAnalysis): string[] {
  const hay = analysis.imports.map((i) => i.from).join(" ") + " " + analysis.exports.join(" ");
  const out: string[] = [];
  for (const [re, zh] of SIDE_EFFECT_DOMAINS) {
    if (re.test(hay) && !out.includes(zh)) out.push(zh);
  }
  return out;
}

function sideEffectOf(analysis: FileAnalysis, sym?: SymbolInfo): string {
  const domains = domainOf(analysis);
  if (domains.length === 0) return "没有隐藏动作，产出即全部";
  const scope = sym ? `机器 ${sym.name}()` : "这个文件";
  return `${scope}还会悄悄${domains.slice(0, 2).join("、")}`;
}

/** 五维拆解：意图/手段/输入/输出/副作用（2.2）。 */
export function fiveDim(analysis: FileAnalysis, sym?: SymbolInfo): FiveDim {
  const calls = sym ? analysis.calls.filter((c) => c.from === sym.name) : analysis.calls.slice(0, 4);
  const method = calls.length > 0
    ? `靠依次开动 ${calls.slice(0, 3).map((c) => `${c.to}()`).join("、")} 这几台机器来完成`
    : "步骤都在自己体内逐步完成";
  const input = sym
    ? (sym.params.length > 0 ? `原材料是 ${sym.params.join("、")}` : "不需要外部原料")
    : (analysis.imports.length > 0 ? `借用了 ${analysis.imports.slice(0, 3).map((i) => i.from).join("、")} 等工具箱` : "自给自足");
  const output = sym
    ? (analysis.exports.includes(sym.name) ? "成品对外公开，供其他文件取用" : "成品交给直接调用它的机器")
    : (analysis.exports.length > 0 ? `对外提供 ${analysis.exports.slice(0, 3).join("、")}` : "以内部协作为主");
  const goal = sym ? inferGoal(sym.name, analysis.lang) : goalFromFile(analysis);
  return { intent: goal, method, input, output, sideEffect: sideEffectOf(analysis, sym) };
}

/** 从文件内的函数目标聚合出“这个文件专门负责…”（2.3 文件级独特翻译）。 */
export function goalFromFile(analysis: FileAnalysis): string {
  const fns = analysis.symbols.filter((s) => s.kind === "function").slice(0, 3);
  if (fns.length === 0) {
    const cls = analysis.symbols.find((s) => s.kind === "class");
    if (cls) return `定义并维护图纸《${cls.name}》的全部行为`;
    if (analysis.role === "config") return "保管项目运行所需的各项参数与开关";
    if (analysis.role === "doc") return "向人类读者解释这个项目/模块";
    return "承载基础声明与工具性代码";
  }
  const goals = fns.map((f) => inferGoal(f.name, analysis.lang));
  const unique = [...new Set(goals)];
  return unique.length === 1 ? unique[0]! : `${unique[0]}，并${unique[1] ?? "支撑周边流程"}`;
}

function joinNames(names: string[]): string {
  if (names.length === 0) return "";
  if (names.length === 1) return names[0]!;
  return `${names.slice(0, -1).join("、")} 和 ${names[names.length - 1]!}`;
}

/**
 * 函数级五维翻译（2.3）：由函数名/参数/调用链组装，天然独一无二。
 */
export function describeFunctionDeep(analysis: FileAnalysis, sym: SymbolInfo, style: NarrateStyle = "metaphor"): string {
  const d = fiveDim(analysis, sym);
  const calls = analysis.calls.filter((c) => c.from === sym.name).map((c) => `${c.to}()`);
  if (style === "engineering") {
    return `此函数 ${sym.name}() 用于${d.intent}：接收 ${sym.params.length > 0 ? sym.params.join(", ") : "无参数"}，${d.method}，${d.output}。`;
  }
  if (style === "story") {
    const steps = calls.slice(0, 3).map((c, i) => `第 ${i + 1} 步走进 ${c}`);
    const tail = steps.length > 0 ? `${steps.join("，")}，最后把结果交回去` : "在体内一步步把事情办妥，最后交出结果";
    return `${sym.name}() 的任务${d.intent.startsWith("定义") ? "藏在图纸里" : `是${d.intent}`}：它接过 ${sym.params.length > 0 ? sym.params.join("、") : "空手"}，${tail}。`;
  }
  // metaphor（默认）：机器/厨房叙事
  const material = sym.params.length > 0 ? `原料是${joinNames(sym.params)}` : "不用投料";
  const work = calls.length > 0 ? `中途会调用 ${joinNames(calls.slice(0, 3))} 帮忙加工` : "加工全程自己在肚子里完成";
  return `机器 ${sym.name}() 专门用来${d.intent}；${material}，${work}，成品${analysis.exports.includes(sym.name) ? "还挂在对外的货架上" : "直接交回给调用它的人"}。`;
}

/**
 * 文件级五维翻译（2.3）：意图汇总 + 明星函数点名 + 依赖域，保证每个文件一句独有描述。
 */
export function describeFileDeep(analysis: FileAnalysis, style: NarrateStyle = "metaphor"): string {
  const d = fiveDim(analysis);
  const fns = analysis.symbols.filter((s) => s.kind === "function").slice(0, 3);
  const classes = analysis.symbols.filter((s) => s.kind === "class").slice(0, 2);
  const name = analysis.relPath.split("/").pop() ?? analysis.relPath;
  if (style === "engineering") {
    const parts = [
      `${name}：${d.intent}。`,
      fns.length > 0 ? `核心函数：${fns.map((f) => `${f.name}()`).join("、")}。` : "",
      analysis.imports.length > 0 ? `依赖：${analysis.imports.slice(0, 3).map((i) => i.from).join("、")}。` : "",
      d.sideEffect,
    ];
    return parts.filter(Boolean).join(" ");
  }
  if (style === "story") {
    const steps = fns.map((f, i) => `第 ${i + 1} 站是 ${f.name}()，负责${inferGoal(f.name, analysis.lang)}`);
    return `翻开 ${name}：${d.intent}。${steps.length > 0 ? `${steps.join("；")}。` : ""}${d.sideEffect}。`;
  }
  // metaphor：点名明星函数，句子携带文件独有事实
  const cast = [
    ...(classes.length > 0 ? [`图纸《${classes.map((c) => c.name).join("》《")}》`] : []),
    ...fns.map((f) => `机器 ${f.name}()`),
  ];
  const deps = analysis.imports.length > 0 ? `它常去 ${joinNames(analysis.imports.slice(0, 3).map((i) => i.from))} 这些工具箱借东西` : "它基本自给自足";
  return cast.length > 0
    ? `${name} 是个专门${d.intent}的车间：里面有 ${joinNames(cast)}；${deps}。`
    : `${name} 是个安静的车间——${d.intent}，没有独立运行的机器；${deps}。`;
}

/**
 * 2.4 翻译质量自检：规范化后做词级 Jaccard 相似度，
 * 相似度 > 0.9 视为“翻译退化”，返回重复对供上层警告/重生成。
 */
export function dedupeCheck(texts: string[]): Array<[number, number]> {
  const norm = texts.map((t) => new Set(
    t.toLowerCase().replace(/[，。；：、？！,.:;?!()\[\]{}"'`\s]/g, " ").split(/\s+/).filter((w) => w.length > 1),
  ));
  const dup: Array<[number, number]> = [];
  for (let i = 0; i < norm.length; i++) {
    for (let j = i + 1; j < norm.length; j++) {
      const a = norm[i]!;
      const b = norm[j]!;
      if (a.size === 0 && b.size === 0) { dup.push([i, j]); continue; }
      let inter = 0;
      for (const w of a) if (b.has(w)) inter++;
      const union = a.size + b.size - inter;
      if (union > 0 && inter / union > 0.9) dup.push([i, j]);
    }
  }
  return dup;
}
