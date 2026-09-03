/**
 * Dependency-free syntax highlighter for embedded node code blocks.
 *
 * Input is ALWAYS escaped before token spans are wrapped around it, so the
 * output is safe HTML by construction (it only contains <span> tags). The
 * result is display-only: it is never persisted in node HTML — the stored
 * form keeps the plain-text code inside its <pre>.
 */

export interface Tok {
  cls: string; // tok-kw | tok-str | tok-com | tok-num | tok-fn | tok-op
  text: string;
}

const KW_CLIKE = new Set([
  "abstract", "as", "async", "await", "break", "case", "catch", "class", "const",
  "continue", "debugger", "default", "delete", "do", "else", "enum", "export",
  "extends", "false", "finally", "for", "from", "function", "get", "if",
  "implements", "import", "in", "instanceof", "interface", "let", "new", "null",
  "of", "private", "protected", "public", "readonly", "return", "satisfies",
  "set", "static", "super", "switch", "this", "throw", "true", "try", "type",
  "typeof", "undefined", "var", "void", "while", "yield", "struct", "impl",
  "fn", "mut", "pub", "use", "match", "go", "defer", "package",
]);
const KW_PY = new Set([
  "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
  "del", "elif", "else", "except", "False", "finally", "for", "from", "global",
  "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or",
  "pass", "raise", "return", "True", "try", "while", "with", "yield", "self",
]);
const KW_SH = new Set([
  "if", "then", "else", "elif", "fi", "for", "in", "do", "done", "while",
  "case", "esac", "function", "return", "export", "local", "echo", "cd",
  "exit", "set", "source", "alias", "sudo", "rm", "cp", "mv", "mkdir",
]);
const KW_SQL = new Set([
  "select", "from", "where", "insert", "into", "values", "update", "set",
  "delete", "create", "table", "drop", "alter", "join", "left", "right",
  "inner", "outer", "on", "group", "by", "order", "limit", "and", "or",
  "not", "null", "primary", "key", "foreign", "references", "as", "distinct",
]);

function keywordsFor(lang: string): Set<string> | null {
  const l = lang.toLowerCase();
  if (["py", "python"].includes(l)) return KW_PY;
  if (["sh", "bash", "zsh", "shell", "console"].includes(l)) return KW_SH;
  if (["sql", "sqlite", "psql"].includes(l)) return KW_SQL;
  if (["js", "jsx", "ts", "tsx", "javascript", "typescript", "java", "c", "cpp",
    "c++", "cs", "csharp", "go", "rust", "rs", "swift", "kotlin", "scala",
    "dart", "json", "jsonc", ""].includes(l)) return KW_CLIKE;
  return null; // unknown language → strings/numbers/comments only
}

/** Tokenize one source string. Line-oriented, comment-style aware. */
export function tokenizeCode(src: string, lang: string): Tok[] {
  const kw = keywordsFor(lang);
  const l = lang.toLowerCase();
  const lineCom = ["py", "python", "sh", "bash", "zsh", "shell", "console", "sql", "sqlite", "psql"].includes(l) ? "#" : "//";
  const toks: Tok[] = [];
  let i = 0;
  const n = src.length;
  const push = (cls: string, text: string): void => {
    const prev = toks[toks.length - 1];
    if (prev && prev.cls === cls) prev.text += text;
    else toks.push({ cls, text });
  };
  const plain = (text: string): void => {
    let rest = text;
    while (rest.length > 0) {
      // identifier / keyword
      const idm = rest.match(/^[A-Za-z_$][A-Za-z0-9_$]*/);
      if (idm) {
        const w = idm[0];
        push(kw?.has(w) ? "tok-kw" : /^[A-Z][A-Za-z0-9_]*$/.test(w) ? "tok-type" : "tok-id", w);
        rest = rest.slice(w.length);
        continue;
      }
      // number
      const nm = rest.match(/^(?:0[xXbBoO][0-9a-fA-F_]+|\d[\d_]*(?:\.\d+)?(?:[eE][+-]?\d+)?)/);
      if (nm) {
        push("tok-num", nm[0]);
        rest = rest.slice(nm[0].length);
        continue;
      }
      // operator / punctuation
      const om = rest.match(/^[+\-*/%=<>!&|^~?:;,.()[\]{}]+/);
      if (om) {
        push("tok-op", om[0]);
        rest = rest.slice(om[0].length);
        continue;
      }
      push("tok-id", rest[0]!);
      rest = rest.slice(1);
    }
  };
  while (i < n) {
    const rest = src.slice(i);
    // block comments
    if (rest.startsWith("/*")) {
      const end = src.indexOf("*/", i + 2);
      const stop = end === -1 ? n : end + 2;
      push("tok-com", src.slice(i, stop));
      i = stop;
      continue;
    }
    if (rest.startsWith('"""') || rest.startsWith("'''")) {
      const q = rest.slice(0, 3);
      const end = src.indexOf(q, i + 3);
      const stop = end === -1 ? n : end + 3;
      push("tok-str", src.slice(i, stop));
      i = stop;
      continue;
    }
    // line comments
    if (rest.startsWith(lineCom)) {
      let end = src.indexOf("\n", i);
      if (end === -1) end = n;
      push("tok-com", src.slice(i, end));
      i = end;
      continue;
    }
    // strings (single/double/backtick, with escapes)
    const sq = rest.match(/^(["'`])/);
    if (sq) {
      const q = sq[1]!;
      let j = 1;
      while (j < rest.length) {
        if (rest[j] === "\\") j += 2;
        else if (rest[j] === q || rest[j] === "\n") { j++; break; }
        else j++;
      }
      push("tok-str", rest.slice(0, j));
      i += j;
      continue;
    }
    // anything else up to the next interesting char
    let j = 0;
    while (j < rest.length && !"\"'`".includes(rest[j]!)
      && !rest.startsWith("/*", j) && !rest.startsWith(lineCom, j)) j++;
    if (j === 0) j = 1;
    // highlight call sites ident(
    const seg = rest.slice(0, j);
    const callRe = /([A-Za-z_$][A-Za-z0-9_$]*)(\s*\()/g;
    let last = 0;
    let m: RegExpExecArray | null;
    while ((m = callRe.exec(seg)) !== null) {
      plain(seg.slice(last, m.index));
      push(kw?.has(m[1]!) ? "tok-kw" : "tok-fn", m[1]!);
      plain(m[2]!);
      last = m.index + m[0].length;
    }
    plain(seg.slice(last));
    i += j;
  }
  return toks;
}

/** Highlight to an HTML fragment of <span class="tok-*"> inside <code>. */
export function highlightCode(src: string, lang: string): string {
  const esc = (s: string): string =>
    s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const toks = tokenizeCode(src, lang);
  return `<code>${toks.map((t) =>
    t.cls === "tok-id" || t.cls === "tok-op"
      ? esc(t.text)
      : `<span class="${t.cls}">${esc(t.text)}</span>`,
  ).join("")}</code>`;
}
