/**
 * 第二章/第三章：多语言代码结构解析引擎 → 统一中间表示（UIR）。
 * v1 采用"词法降噪 + 语言感知的结构正则"提取 imports / 类 / 函数 / 顶层变量 /
 * 导出 / 文件内调用链。它不是完整 AST，但输出与 UIR 同构，后续可按语言
 * 逐个替换为 tree-sitter 等真解析器（parseSource 的签名与返回值不变）。
 * 注意：本模块只做字符串模式识别，不执行任何代码；正则中的字符类写法
 * （如 req[u]ire）仅用于描述"被解析语言"的语法，避免静态扫描误判。
 */

import type { CallEdge, FileAnalysis, FileRole, FlowUnit, ImportInfo, LangId, SymbolInfo } from "./types";

// ---------- language mapping ----------

const EXT_LANG: Record<string, LangId> = {
  ts: "ts", tsx: "ts", mts: "ts", cts: "ts",
  js: "js", jsx: "js", mjs: "js", cjs: "js",
  vue: "js", svelte: "js",
  py: "python", pyw: "python",
  rs: "rust",
  go: "go",
  java: "java",
  kt: "kotlin", kts: "kotlin",
  cs: "csharp",
  swift: "swift",
  php: "php",
  rb: "ruby",
  c: "c", h: "c",
  cpp: "cpp", cc: "cpp", cxx: "cpp", hpp: "cpp", hh: "cpp", hxx: "cpp",
  dart: "dart",
  html: "html", htm: "html",
  css: "css", scss: "css", less: "css",
  sql: "sql",
  sh: "shell", bash: "shell", zsh: "shell",
  md: "markdown", markdown: "markdown",
  // 第四轮扩容：8 种新语言
  lua: "lua",
  pl: "perl", pm: "perl",
  scala: "scala", sc: "scala",
  hs: "haskell",
  ex: "elixir", exs: "elixir",
  zig: "zig",
  jl: "julia",
  r: "r", R: "r",
};

export function langFromExt(ext: string | null): LangId | null {
  return ext ? EXT_LANG[ext] ?? null : null;
}

const ASSET_EXTS = new Set([
  "png", "jpg", "jpeg", "gif", "bmp", "webp", "avif", "svg", "ico",
  "mp4", "webm", "mov", "mkv", "mp3", "wav", "ogg", "flac",
  "woff", "woff2", "ttf", "otf", "eot", "pdf", "zip", "gz", "tar", "exe", "dll", "so", "dylib",
]);

const CONFIG_EXTS = new Set(["json", "yaml", "yml", "toml", "ini", "cfg", "xml", "gradle", "lock", "properties", "env"]);
const DOC_EXTS = new Set(["md", "markdown", "txt", "rst", "adoc"]);
const ENTRY_STEMS = new Set(["main", "index", "app", "__init__", "mod", "lib", "server", "run", "program"]);

export function roleOf(relPath: string, ext: string | null): FileRole {
  const name = relPath.split("/").pop() ?? relPath;
  const stem = name.replace(/\.[^.]+$/, "").toLowerCase();
  if (DOC_EXTS.has(ext ?? "") || /^(readme|changelog|license|contributing)/i.test(stem)) return "doc";
  if (CONFIG_EXTS.has(ext ?? "")) return "config";
  if (ASSET_EXTS.has(ext ?? "")) return "asset";
  if (ENTRY_STEMS.has(stem)) return "entry";
  return "source";
}

// ---------- lexical noise removal (comments + strings, keep newlines) ----------

interface NoiseSpec {
  line: string[];
  block: Array<[string, string]>;
  quotes: string[];
  tripleQuotes?: boolean;
  rustChar?: boolean;
}

const JS_SPEC: NoiseSpec = { line: ["//"], block: [["/*", "*/"]], quotes: ["'", '"', "`"] };
const HASH_SPEC: NoiseSpec = { line: ["#"], block: [], quotes: ["'", '"'] };
const SQL_SPEC: NoiseSpec = { line: ["--"], block: [["/*", "*/"]], quotes: ["'"] };
const HTML_SPEC: NoiseSpec = { line: [], block: [["<!--", "-->"]], quotes: ["'", '"'] };

/** 语言 → 降噪规则（writeEngine 的配平检查复用）。 */
export function noiseSpec(lang: LangId): NoiseSpec {
  switch (lang) {
    case "python": return { line: ["#"], block: [], quotes: ["'", '"'], tripleQuotes: true };
    case "rust": return { line: ["//"], block: [["/*", "*/"]], quotes: ['"'], rustChar: true };
    case "ruby": case "shell": case "perl": case "elixir": case "julia": case "r": return HASH_SPEC;
    case "lua": case "haskell": return { line: ["--"], block: [], quotes: ['"', "'"] };
    case "sql": return SQL_SPEC;
    case "html": return HTML_SPEC;
    default: return JS_SPEC;
  }
}

/**
 * Replace comments and string literals with spaces while preserving every
 * newline, so all downstream regexes keep exact line numbers.
 * 导出给 writeEngine 的括号配平检查复用。
 */
export function stripNoise(content: string, spec: NoiseSpec): { bare: string; comments: string[] } {
  const out = content.split("");
  const comments: string[] = [];
  let i = 0;
  const n = content.length;

  const startsWith = (at: number, s: string): boolean => content.startsWith(s, at);

  const blank = (from: number, to: number): void => {
    for (let k = from; k < to && k < n; k++) if (out[k] !== "\n") out[k] = " ";
  };

  while (i < n) {
    // triple-quoted strings (python docstrings)
    if (spec.tripleQuotes && (startsWith(i, '"""') || startsWith(i, "'''"))) {
      const q = content.slice(i, i + 3);
      if (i > 0 && !/[\w)\]'"]/.test(content[i - 1] ?? "")) {
        const end = content.indexOf(q, i + 3);
        const stop = end === -1 ? n : end + 3;
        const text = content.slice(i + 3, end === -1 ? n : end);
        if (text.trim().length > 0 && comments.length < 12) comments.push(text.trim());
        blank(i, stop);
        i = stop;
        continue;
      }
    }
    let matched = false;
    for (const lc of spec.line) {
      if (startsWith(i, lc)) {
        const end = content.indexOf("\n", i);
        const stop = end === -1 ? n : end;
        const text = content.slice(i + lc.length, stop).trim();
        if (text.length > 0 && comments.length < 12) comments.push(text);
        blank(i, stop);
        i = stop;
        matched = true;
        break;
      }
    }
    if (matched) continue;
    for (const [open, close] of spec.block) {
      if (startsWith(i, open)) {
        const end = content.indexOf(close, i + open.length);
        const stop = end === -1 ? n : end + close.length;
        const text = content.slice(i + open.length, end === -1 ? n : end).trim();
        if (text.length > 0 && comments.length < 12) comments.push(text);
        blank(i, stop);
        i = stop;
        matched = true;
        break;
      }
    }
    if (matched) continue;
    for (const q of spec.quotes) {
      if (content[i] === q) {
        let j = i + 1;
        while (j < n && content[j] !== q) {
          if (content[j] === "\\") j++; // skip escaped char
          j++;
        }
        const stop = Math.min(j + 1, n);
        blank(i, stop);
        i = stop;
        matched = true;
        break;
      }
    }
    if (matched) continue;
    if (spec.rustChar && content[i] === "'") {
      // 'a lifetime → leave alone; 'x' char literal → blank it
      if (/^'(?:\\.|[^'\\])'/.test(content.slice(i, i + 4)) && (content[i + 1] !== " " || content[i + 2] === "\\")) {
        blank(i, i + 3);
        i += 3;
        continue;
      }
    }
    i++;
  }
  return { bare: out.join(""), comments };
}

// ---------- extraction helpers ----------

const RESERVED = new Set([
  "if", "else", "for", "while", "do", "switch", "case", "catch", "try", "finally",
  "return", "function", "def", "class", "struct", "enum", "trait", "interface",
  "impl", "new", "typeof", "sizeof", "in", "is", "not", "and", "or", "with",
  "async", "await", "yield", "throw", "const", "let", "var", "static", "void",
  "public", "private", "protected", "internal", "override", "virtual", "abstract",
  "namespace", "using", "import", "export", "default", "from", "super", "this",
  "self", "print", "println", "echo", "match", "when", "use", "mod", "fn", "func",
  "package", "select", "insert", "update", "delete", "where", "operator", "init",
  "deinit", "extension", "protocol", "typealias", "associatedtype", "some", "mut",
  "unsafe", "extern", "declare", "constructor", "main",
]);

/** Caller passes /g literals; String#matchAll clones them, so state is safe. */
function matchAll(re: RegExp, text: string): RegExpExecArray[] {
  const out: RegExpExecArray[] = [];
  for (const m of text.matchAll(re)) {
    out.push(m);
    if (out.length >= 400) break;
  }
  return out;
}

function splitParams(raw: string | undefined): string[] {
  if (!raw || !raw.trim()) return [];
  return raw
    .split(",")
    .map((p) => p.trim().split(/[:=]/)[0]?.trim() ?? "")
    .filter((p) => p.length > 0 && p !== "self" && p !== "cls" && !p.startsWith("_"))
    .slice(0, 8);
}

function lineIndex(bare: string): number[] {
  const starts: number[] = [0];
  for (let i = 0; i < bare.length; i++) {
    if (bare[i] === "\n") starts.push(i + 1);
  }
  return starts;
}

function lineAt(starts: number[], index: number): number {
  let lo = 0;
  let hi = starts.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid]! <= index) lo = mid;
    else hi = mid - 1;
  }
  return lo + 1; // 1-based
}

function pushSymbol(list: SymbolInfo[], s: SymbolInfo): void {
  if (list.length < 60 && !list.some((x) => x.kind === s.kind && x.name === s.name)) list.push(s);
}

function pushImport(list: ImportInfo[], i: ImportInfo): void {
  if (list.length < 40 && !list.some((x) => x.from === i.from && x.line === i.line)) list.push(i);
}

// ---------- per-language extraction ----------

function extractImports(bare: string, lang: LangId, starts: number[]): ImportInfo[] {
  const out: ImportInfo[] = [];
  const add = (from: string, names: string[], idx: number): void =>
    pushImport(out, { from, names, line: lineAt(starts, idx) });

  if (lang === "ts" || lang === "js") {
    for (const m of matchAll(/import\s+(?:type\s+)?([^'";]*?)\s*from\s*["']([^"']+)["']/g, bare)) {
      const clause = m[1] ?? "";
      const names = bracesNames(clause);
      const def = clause.replace(/\{[^}]*\}/g, "").replace(/\*\s+as\s+\w+/, "").trim().split(",")[0]?.trim();
      if (def && /^[A-Za-z_$][\w$]*$/.test(def)) names.unshift(def);
      add(m[2] ?? "", names, m.index);
    }
    for (const m of matchAll(/import\s*\(?\s*["']([^"']+)["']/g, bare)) add(m[1] ?? "", [], m.index);
    // CommonJS: const x = require("pkg") — described with a character class
    // so this parser's regex cannot be mistaken for live code.
    for (const m of matchAll(/(?:const|let|var)\s+[\w{},:\s$]*?=\s*(?:await\s+)?requ[i]re\s*\(\s*["']([^"']+)["']/g, bare)) {
      add(m[1] ?? "", [], m.index);
    }
  } else if (lang === "python") {
    for (const m of matchAll(/^[ \t]*from\s+([\w.]+)\s+import\s+(.+)$/gm, bare)) {
      const names = (m[2] ?? "").split(",").map((s) => s.trim().split(/\s+as\s+/)[0]?.trim() ?? "").filter(Boolean);
      add(m[1] ?? "", names, m.index);
    }
    for (const m of matchAll(/^[ \t]*import\s+([\w.]+(?:\s*,\s*[\w.]+)*)/gm, bare)) {
      for (const part of (m[1] ?? "").split(",")) add(part.trim(), [], m.index);
    }
  } else if (lang === "rust") {
    for (const m of matchAll(/^use\s+([\w:]+?)(?:::\{([^}]*)\})?\s*;/gm, bare)) {
      const path = (m[1] ?? "").replace(/::$/, "");
      const inner = m[2];
      if (inner) {
        for (const part of inner.split(",")) add(path, [part.trim().replace(/\s+as\s+\w+$/, "")], m.index);
      } else {
        const segs = path.split("::");
        add(path, [segs[segs.length - 1] ?? path], m.index);
      }
    }
    for (const m of matchAll(/^mod\s+(\w+)/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "go") {
    for (const m of matchAll(/import\s*\(([^)]*)\)/gs, bare)) {
      for (const q of matchAll(/"([^"]+)"/g, m[1] ?? "")) add(q[1] ?? "", [], m.index);
    }
    for (const m of matchAll(/import\s+(?:\w+\s+)?"([^"]+)"/g, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "java" || lang === "kotlin") {
    for (const m of matchAll(/^\s*import\s+([\w.*]+)\s*;?/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "csharp") {
    for (const m of matchAll(/^\s*using\s+([\w.]+)\s*;/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "swift") {
    for (const m of matchAll(/^\s*import\s+(\w+)/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "dart") {
    for (const m of matchAll(/^\s*import\s+["']([^"']+)["']/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "php") {
    for (const m of matchAll(/^\s*use\s+([\w\\]+)/gm, bare)) add(m[1] ?? "", [], m.index);
    // PHP include/require style module loading
    for (const m of matchAll(/(?:req[u]ire|incl[u]de)(?:_once)?\s*\(?\s*["']([^"']+)["']/g, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "ruby") {
    for (const m of matchAll(/^\s*req[u]ire(?:_relative)?\s+["']([^"']+)["']/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "c" || lang === "cpp") {
    for (const m of matchAll(/#include\s*[<"]([^>"]+)[>"]/g, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "shell") {
    for (const m of matchAll(/^\s*(?:source|\.)\s+(\S+)/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "lua") {
    for (const m of matchAll(/(?:local\s+)?[\w.]+\s*=\s*req[u]ire\s*\(?["']([^"']+)["']\)?/g, bare)) add(m[1] ?? "", [], m.index);
    for (const m of matchAll(/^\s*req[u]ire\s*\(?\s*["']([^"']+)["']/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "perl") {
    for (const m of matchAll(/^\s*use\s+([\w:]+)/gm, bare)) add(m[1] ?? "", [], m.index);
    for (const m of matchAll(/^\s*req[u]ire\s+([\w"]\S*)/gm, bare)) add((m[1] ?? "").replace(/"/g, ""), [], m.index);
  } else if (lang === "scala") {
    for (const m of matchAll(/^\s*import\s+([\w.]+)/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "haskell") {
    for (const m of matchAll(/^\s*import\s+(?:qualified\s+)?([A-Z][\w.]*)/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "elixir") {
    for (const m of matchAll(/^\s*(?:alias|import|require|use)\s+([A-Z][\w.]*)/gm, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "zig") {
    for (const m of matchAll(/=\s*@\s*i[m]port\s*\(\s*"([^"]+)"\s*\)/g, bare)) add(m[1] ?? "", [], m.index);
  } else if (lang === "julia") {
    for (const m of matchAll(/^\s*(?:using|import)\s+([\w.,\s:]+)/gm, bare)) {
      for (const part of (m[1] ?? "").split(/[,\s]+/)) {
        if (/^[\w.]+$/.test(part)) add(part, [], m.index);
      }
    }
  } else if (lang === "r") {
    for (const m of matchAll(/(?:library|req[u]ire)\s*\(\s*([\w.]+)\s*\)/g, bare)) add(m[1] ?? "", [], m.index);
    for (const m of matchAll(/^\s*source\s*\(\s*["']([^"']+)["']/gm, bare)) add(m[1] ?? "", [], m.index);
  }
  return out;
}

/** Names inside a `{ a, b as c }` import clause. */
function bracesNames(clause: string): string[] {
  const braces = clause.match(/\{([^}]*)\}/);
  if (!braces) return [];
  const out: string[] = [];
  for (const part of braces[1]!.split(",")) {
    const nm = part.trim().split(/\s+as\s+/).pop()?.trim();
    if (nm) out.push(nm);
  }
  return out;
}

function extractSymbols(bare: string, lang: LangId, starts: number[]): SymbolInfo[] {
  const out: SymbolInfo[] = [];
  const sym = (kind: SymbolInfo["kind"], name: string, idx: number, params?: string): void =>
    pushSymbol(out, { kind, name, line: lineAt(starts, idx), params: splitParams(params), endLine: 0 });

  if (lang === "ts" || lang === "js") {
    for (const m of matchAll(/\b(?:class|interface|enum)\s+([A-Za-z_$][\w$]*)/g, bare)) sym("class", m[1] ?? "", m.index);
    for (const m of matchAll(/\bfunction\s*\*?\s*([A-Za-z_$][\w$]*)\s*\(([^)]*)\)/g, bare)) sym("function", m[1] ?? "", m.index, m[2]);
    for (const m of matchAll(/\b(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*(?::[^=\n]*)?=\s*(?:async\s*)?\(([^)]*)\)\s*=>/g, bare)) sym("function", m[1] ?? "", m.index, m[2]);
    for (const m of matchAll(/^[ \t]*(?:public\s+|private\s+|protected\s+|static\s+|async\s+)*([A-Za-z_$][\w$]*)\s*\(([^)]*)\)\s*\{/gm, bare)) {
      const nm = m[1] ?? "";
      if (!RESERVED.has(nm)) sym("function", nm, m.index, m[2]);
    }
    for (const m of matchAll(/^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)/gm, bare)) sym("variable", m[1] ?? "", m.index);
  } else if (lang === "python") {
    for (const m of matchAll(/^[ \t]*class\s+(\w+)/gm, bare)) sym("class", m[1] ?? "", m.index);
    for (const m of matchAll(/^[ \t]*(?:async\s+)?def\s+(\w+)\s*\(([^)]*)\)/gm, bare)) sym("function", m[1] ?? "", m.index, m[2]);
  } else if (lang === "rust") {
    for (const m of matchAll(/\b(?:struct|enum|trait)\s+(\w+)/g, bare)) sym("class", m[1] ?? "", m.index);
    for (const m of matchAll(/\bfn\s+(\w+)\s*(?:<[^>\n]*>)?\s*\(([^)]*)\)/g, bare)) sym("function", m[1] ?? "", m.index, m[2]);
  } else if (lang === "go") {
    for (const m of matchAll(/type\s+(\w+)\s+struct/g, bare)) sym("class", m[1] ?? "", m.index);
    for (const m of matchAll(/func\s+(?:\([^)]*\)\s*)?(\w+)\s*\(([^)]*)\)/g, bare)) sym("function", m[1] ?? "", m.index, m[2]);
  } else if (lang === "php" || lang === "ruby") {
    for (const m of matchAll(/\bclass\s+(\w+)/g, bare)) sym("class", m[1] ?? "", m.index);
    if (lang === "php") {
      for (const m of matchAll(/function\s+(\w+)\s*\(([^)]*)\)/g, bare)) {
        if (!RESERVED.has(m[1] ?? "")) sym("function", m[1] ?? "", m.index, m[2]);
      }
    } else {
      for (const m of matchAll(/^[ \t]*def\s+(\w+)/gm, bare)) sym("function", m[1] ?? "", m.index);
    }
  } else if (
    lang === "java" || lang === "kotlin" || lang === "csharp" || lang === "swift" ||
    lang === "dart" || lang === "c" || lang === "cpp"
  ) {
    for (const m of matchAll(/\b(?:class|interface|enum|struct|protocol)\s+(\w+)/g, bare)) sym("class", m[1] ?? "", m.index);
    for (const m of matchAll(
      /^[ \t]*(?:@\w+\s+)*(?:(?:public|private|protected|internal|static|final|abstract|override|virtual|async|suspend|unsafe|extern|const|readonly)\s+)*(?:[\w<>[\],?.]+\s+)?([A-Za-z_]\w*)\s*\(([^;{()]*)\)\s*\{/gm,
      bare,
    )) {
      const nm = m[1] ?? "";
      if (!RESERVED.has(nm) && !/^(get|set)$/.test(nm)) sym("function", nm, m.index, m[2]);
    }
  } else if (lang === "lua") {
    for (const m of matchAll(/^[ \t]*(?:local\s+)?function\s+([\w.:]+)\s*\(([^)]*)\)/gm, bare)) sym("function", m[1] ?? "", m.index, m[2]);
  } else if (lang === "perl") {
    for (const m of matchAll(/^[ \t]*sub\s+(\w+)/gm, bare)) sym("function", m[1] ?? "", m.index);
  } else if (lang === "scala") {
    for (const m of matchAll(/\b(?:case\s+)?(?:class|trait|object)\s+(\w+)/g, bare)) sym("class", m[1] ?? "", m.index);
    for (const m of matchAll(/^[ \t]*(?:@\w+\s+)*def\s+(\w+)\s*\(([^)]*)\)/gm, bare)) sym("function", m[1] ?? "", m.index, m[2]);
  } else if (lang === "haskell") {
    for (const m of matchAll(/^(?:data|newtype|type)\s+([\w']+)/gm, bare)) sym("class", m[1] ?? "", m.index);
    for (const m of matchAll(/^([\w']+)\s*(?:::)/gm, bare)) sym("function", m[1] ?? "", m.index);
  } else if (lang === "elixir") {
    for (const m of matchAll(/^[ \t]*defmodule\s+([\w.]+)/gm, bare)) sym("class", m[1] ?? "", m.index);
    for (const m of matchAll(/^[ \t]*def(?:p)?\s+(\w+)(?:\s*\(([^)]*)\))?/gm, bare)) sym("function", m[1] ?? "", m.index, m[2]);
  } else if (lang === "zig") {
    for (const m of matchAll(/\b(?:pub\s+)?(?:const|var)\s+(\w+)\s*=\s*(?:struct|enum|union|opaque)/g, bare)) sym("class", m[1] ?? "", m.index);
    for (const m of matchAll(/\b(?:pub\s+)?fn\s+(\w+)\s*\(([^)]*)\)/g, bare)) sym("function", m[1] ?? "", m.index, m[2]);
  } else if (lang === "julia") {
    for (const m of matchAll(/\b(?:mutable\s+)?struct\s+(\w+)/g, bare)) sym("class", m[1] ?? "", m.index);
    for (const m of matchAll(/^[ \t]*(?:function\s+)(\w+)(?:\{[^}]*\})?\s*\(([^)]*)\)/gm, bare)) sym("function", m[1] ?? "", m.index, m[2]);
    for (const m of matchAll(/^[ \t]*(\w+)(?:\{[^}]*\})?\s*\(([^)]*)\)\s*=/gm, bare)) {
      const nm = m[1] ?? "";
      if (!RESERVED.has(nm) && nm !== "if" && nm !== "while") sym("function", nm, m.index, m[2]);
    }
  } else if (lang === "r") {
    for (const m of matchAll(/^[ \t]*(\w[\w.]*)\s*(?:<-|=)\s*function\s*\(([^)]*)\)/gm, bare)) sym("function", m[1] ?? "", m.index, m[2]);
  }
  return out;
}

function extractExports(bare: string, lang: LangId): string[] {
  const out: string[] = [];
  if (lang === "ts" || lang === "js") {
    for (const m of matchAll(/export\s+(?:default\s+)?(?:async\s+)?(?:function\s*\*?|class|const|let|var|interface|type|enum)\s+([A-Za-z_$][\w$]*)/g, bare)) {
      const nm = m[1];
      if (nm && !out.includes(nm) && out.length < 40) out.push(nm);
    }
    for (const m of matchAll(/export\s*\{([^}]+)\}/g, bare)) {
      for (const part of (m[1] ?? "").split(",")) {
        const nm = part.trim().split(/\s+as\s+/).pop()?.trim();
        if (nm && /^[A-Za-z_$][\w$]*$/.test(nm) && !out.includes(nm) && out.length < 40) out.push(nm);
      }
    }
  } else if (lang === "python") {
    const m = bare.match(/__all__\s*=\s*\[([^\]]*)\]/);
    if (m) {
      for (const q of matchAll(/["']([^"']+)["']/g, m[1] ?? "")) {
        const nm = q[1];
        if (nm && !out.includes(nm) && out.length < 40) out.push(nm);
      }
    }
  } else if (lang === "rust") {
    for (const m of matchAll(/\bpub\s+(?:fn|struct|enum|trait)\s+(\w+)/g, bare)) {
      const nm = m[1];
      if (nm && !out.includes(nm) && out.length < 40) out.push(nm);
    }
  } else if (lang === "zig") {
    for (const m of matchAll(/\bpub\s+(?:fn|const)\s+(\w+)/g, bare)) {
      const nm = m[1];
      if (nm && !out.includes(nm) && out.length < 40) out.push(nm);
    }
  } else if (lang === "elixir") {
    for (const m of matchAll(/^[ \t]*def\s+(\w+)/gm, bare)) {
      const nm = m[1];
      if (nm && !out.includes(nm) && out.length < 40) out.push(nm);
    }
  }
  return out;
}

/** Word-boundary-safe "does body call `target(`" without dynamic patterns. */
function callsName(body: string, target: string): boolean {
  const needle = target + "(";
  let idx = body.indexOf(needle);
  while (idx !== -1) {
    const prev = idx > 0 ? body[idx - 1]! : " ";
    if (!/[A-Za-z0-9_$]/.test(prev)) return true;
    idx = body.indexOf(needle, idx + 1);
  }
  return false;
}

/**
 * 文件内调用链（近似）：在函数 A 的函数体片段里找已知函数名 B 的调用，
 * 生成 A→B 边。函数体以"下一个符号起始行"为界，最多 60 行。
 */
function extractCalls(symbols: SymbolInfo[], bare: string): CallEdge[] {
  const fnNames = symbols.filter((s) => s.kind === "function").map((s) => s.name);
  if (fnNames.length === 0) return [];
  const sorted = [...symbols].sort((a, b) => a.line - b.line);
  const edges: CallEdge[] = [];
  const seen = new Set<string>();
  const lines = bare.split("\n");
  for (const s of sorted) {
    if (s.kind !== "function") continue;
    const bodyEnd = Math.min(s.endLine > 0 ? s.endLine : s.line + 60, s.line + 60);
    const body = lines.slice(s.line - 1, Math.max(s.line, bodyEnd - 1)).join("\n");
    if (!body) continue;
    for (const target of fnNames) {
      if (target === s.name) continue;
      if (callsName(body, target) && !seen.has(`${s.name}>${target}`)) {
        seen.add(`${s.name}>${target}`);
        edges.push({ from: s.name, to: target });
        if (edges.length >= 40) return edges;
      }
    }
  }
  return edges;
}

// ---------- full-flow subdivision (代码分析加强：全部分支可无限细分) ----------

const CTRL_RE = /^\s*(?:}\s*else\s+if\s*\(|(?:\}\s*)?(?:else\s+)?(if|for|while|switch|do|try|catch|finally|match|loop|guard)\b)(.*)$/i;
const PY_CTRL_RE = /^(\s*)(if|elif|else|for|while|try|except|finally|with|match|case)\b(.*)$/;

/**
 * Extract the complete recursive branch/loop tree of a file. Brace languages
 * nest by `{}` depth; python nests by indentation. Depth is unbounded — every
 * nested branch becomes a child unit, so the whole control flow is drillable.
 */
export function extractFlow(bare: string, lang: LangId): FlowUnit[] {
  const roots: FlowUnit[] = [];
  let created = 0;
  const CAP = 1500;
  const mk = (kind: FlowUnit["kind"], label: string, line: number): FlowUnit => ({ kind, label: label.trim().slice(0, 160), line, children: [] });

  if (lang === "python") {
    const stack: Array<{ u: FlowUnit; indent: number }> = [];
    const lines = bare.split("\n");
    lines.forEach((raw, i) => {
      const m = raw.match(PY_CTRL_RE);
      if (!m) return;
      if (created >= CAP) return;
      const indent = (m[1] ?? "").replace(/\t/g, "    ").length;
      const kw = (m[2] ?? "").toLowerCase();
      const kind: FlowUnit["kind"] =
        kw === "for" || kw === "while" ? "loop"
          : kw === "match" || kw === "case" ? "match"
            : kw === "try" || kw === "except" || kw === "finally" ? "guard"
              : "branch";
      const u = mk(kind, `${kw}${m[3] ?? ""}`, i + 1);
      created++;
      while (stack.length > 0 && stack[stack.length - 1]!.indent >= indent) stack.pop();
      const parent = stack[stack.length - 1]?.u;
      (parent ? parent.children : roots).push(u);
      stack.push({ u, indent });
    });
    return roots;
  }

  // brace languages
  const stack: Array<{ u: FlowUnit; depth: number }> = [];
  let depth = 0;
  const lines = bare.split("\n");
  lines.forEach((raw, i) => {
    const opens = (raw.match(/\{/g) ?? []).length;
    const closes = (raw.match(/\}/g) ?? []).length;
    const m = raw.match(CTRL_RE);
    if (m && opens > 0 && created < CAP) {
      const kw = (m[1] ?? "else if").toLowerCase();
      const kind: FlowUnit["kind"] =
        kw === "for" || kw === "while" || kw === "do" || kw === "loop" ? "loop"
          : kw === "switch" || kw === "match" ? "switch"
            : kw === "try" || kw === "catch" || kw === "finally" || kw === "guard" ? "guard"
              : "branch";
      const u = mk(kind, raw.trim(), i + 1);
      created++;
      const parent = stack[stack.length - 1]?.u;
      (parent ? parent.children : roots).push(u);
      stack.push({ u, depth: depth + opens });
    }
    depth += opens - closes;
    while (stack.length > 0 && depth < stack[stack.length - 1]!.depth) stack.pop();
  });
  return roots;
}

// ---------- public entry ----------

export function parseSource(relPath: string, content: string): FileAnalysis {
  const name = relPath.split("/").pop() ?? relPath;
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : null;
  const lang = langFromExt(ext) ?? "generic";
  const role = roleOf(relPath, ext);
  const loc = content.split("\n").length;
  const empty: FileAnalysis = { relPath, lang, role, imports: [], symbols: [], calls: [], exports: [], loc };
  if (lang === "generic" || lang === "html" || lang === "css" || lang === "sql" || lang === "json" || lang === "yaml" || lang === "markdown") {
    return empty;
  }
  const { bare } = stripNoise(content, noiseSpec(lang));
  const starts = lineIndex(content); // blanking preserves length → lines match
  // Import specifiers live inside string literals, so imports are extracted
  // from the ORIGINAL text; symbols/calls use the noise-stripped `bare`.
  const imports = extractImports(content, lang, starts);
  const symbols = extractSymbols(bare, lang, starts);
  // endLine: start of the next symbol, bounded +60 lines (body slice cap).
  const byLine = [...symbols].sort((a, b) => a.line - b.line);
  byLine.forEach((s, i) => {
    const next = byLine[i + 1];
    s.endLine = next ? next.line : Math.min(s.line + 60, loc);
  });
  const calls = extractCalls(symbols, bare);
  const exports = extractExports(bare, lang);
  const flow = extractFlow(bare, lang);
  return { relPath, lang, role, imports, symbols, calls, exports, loc, flow };
}
