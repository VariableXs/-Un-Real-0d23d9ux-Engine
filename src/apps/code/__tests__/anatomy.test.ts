import { describe, expect, it } from "vitest";
import { chunkBody, CODE_TERMS, explainLine, structuralBlocks, blockRanges } from "../anatomy";
import { buildReasoning } from "../reasoning";
import { applyIntent } from "../transforms";
import { EditHistory, backupPath, checkBalanced } from "../writeEngine";
import { detectPatterns } from "../detect";
import { lookupTerm, EXPANSION_COUNT, EXPANSION_PACKS, L1_UNIVERSAL, L2_LANGUAGE, L3_FRAMEWORK, L4_DOMAIN, L6_IDIOM, L7_PATTERN } from "../dictionaries";
import { dedupeCheck, describeFileDeep, describeFunctionDeep } from "../deepDescribe";
import type { FileAnalysis } from "../types";

function analysis(rel: string, over: Partial<FileAnalysis> = {}): FileAnalysis {
  return { relPath: rel, lang: "ts", role: "source", imports: [], symbols: [], calls: [], exports: [], loc: 20, ...over };
}

describe("anatomy.explainLine (chapter 1)", () => {
  it("translates an assignment into box wording", () => {
    const card = explainLine("let count = 0;", "ts", 12);
    expect(card.plain).toContain("count");
    expect(card.plain).toContain("盒子");
    expect(card.why).toContain("count");
    expect(card.editable).toBe(true);
    expect(card.line).toBe(12);
  });

  it("explains a guard clause with the blocking rationale", () => {
    const card = explainLine("if (user == null) return;", "ts", 5);
    expect(card.plain).toContain("挡住");
    expect(card.why).toContain("崩溃");
  });

  it("marks security-related lines as cautious (1.2 可编辑标记)", () => {
    const card = explainLine('const token = "abc";', "ts", 3);
    expect(card.editable).toBe(false);
    expect(card.caution).toBeDefined();
  });

  it("detects hover terms (1.2 术语解释)", () => {
    const card = explainLine('const r = await fetch(url);', "ts", 8);
    expect(card.terms.some((t) => t.term === "fetch")).toBe(true);
    expect(card.terms.some((t) => t.term === "await")).toBe(true);
    expect(card.terms.find((t) => t.term === "fetch")!.explain).toBe(CODE_TERMS.fetch);
  });

  it("passes comments through as author notes", () => {
    const card = explainLine("// 初始化配置", "ts", 2);
    expect(card.role).toBe("comment");
    expect(card.plain).toContain("作者备注");
  });

  it("chunks a function body into logical paragraphs (L3)", () => {
    const body = ["a;", "b;", "", "c;", "d;", "e;", "f;", "g;", "h;", ""];
    const chunks = chunkBody(body);
    expect(chunks.length).toBeGreaterThanOrEqual(2);
    expect(chunks[0]!.start).toBe(1);
  });
});

describe("reasoning.buildReasoning (chapter 2)", () => {
  const a = analysis("src/login.ts", {
    symbols: [
      { kind: "function", name: "login", line: 10, params: [], endLine: 20 },
      { kind: "function", name: "validate", line: 2, params: [], endLine: 8 },
      { kind: "function", name: "issueToken", line: 22, params: [], endLine: 30 },
    ],
    calls: [
      { from: "login", to: "validate" },
      { from: "login", to: "issueToken" },
    ],
  });

  it("builds the vertical chain from the goal down to mapped steps", () => {
    const r = buildReasoning(a, "login")!;
    expect(r.vertical.goal).toContain("职责");
    expect(r.vertical.steps.length).toBe(2);
    expect(r.vertical.steps[0]!.line).toBe(10);
  });

  it("builds the horizontal chain with deps / usedBy / peers", () => {
    const r = buildReasoning(a, "login", ["src/app.ts"])!;
    expect(r.horizontal.deps.some((d) => d.title.includes("validate"))).toBe(true);
    expect(r.horizontal.usedBy.some((u) => u.title.includes("src/app.ts"))).toBe(true);
    expect(r.horizontal.peers).toContain("validate");
  });

  it("builds the temporal chain with entry and ordered steps", () => {
    const r = buildReasoning(a, "login")!;
    expect(r.temporal.steps[0]!.title).toContain("validate");
    expect(r.temporal.result).toContain("产出");
  });

  it("returns null for unknown symbols", () => {
    expect(buildReasoning(a, "nope")).toBeNull();
  });
});

describe("transforms.applyIntent (chapter 4 intent dictionary)", () => {
  const CSS = ".btn {\n  background: #ff0000;\n  color: white;\n}\n";
  const PY = "def run(port):\n    start(port)\n";

  it("recolors hex values with fuzzy color words (改颜色)", () => {
    const r = applyIntent("把这个按钮改成蓝色", { content: CSS, lang: "css", relPath: "Button.css" });
    expect(r.matched).toBe(true);
    expect(r.previews.length).toBeGreaterThanOrEqual(1);
    expect(r.content).toContain("#3b82f6");
    expect(r.content).not.toContain("#ff0000");
  });

  it("inserts a language-aware log line after the target (加日志)", () => {
    const r = applyIntent("在这里加个日志，写上'用户尝试登录'", { content: PY, lang: "python", line: 2 });
    expect(r.matched).toBe(true);
    expect(r.content.split("\n")[2]).toContain('print("用户尝试登录")');
  });

  it("renames an identifier everywhere with boundary safety (重命名)", () => {
    const code = "let userName = 1;\nsave(userName);\nlet userNameX = 2;\n";
    const r = applyIntent("把 userName 改名叫 userId", { content: code, lang: "ts" });
    expect(r.matched).toBe(true);
    expect(r.content).toContain("let userId = 1;");
    expect(r.content).toContain("save(userId);");
    expect(r.content).toContain("userNameX"); // 不误伤
    expect(r.content).not.toMatch(/\buserName\b/);
  });

  it("changes numbers for port/timeout keywords (改数字)", () => {
    const r = applyIntent("把端口改成 8080", { content: "const port = 3000;\n", lang: "ts" });
    expect(r.matched).toBe(true);
    expect(r.content).toContain("8080");
  });

  it("wraps a line in try/catch (加异常处理)", () => {
    const r = applyIntent("如果失败了就提示", { content: "load();\n", lang: "ts", line: 1 });
    expect(r.matched).toBe(true);
    expect(r.content).toContain("try {");
    expect(r.content).toContain("load();");
  });

  it("asks a clarifying question when uncertain (4.5 追问)", () => {
    const r = applyIntent("让它变大", { content: CSS, lang: "css" });
    expect(r.matched).toBe(false);
    expect(r.clarify).toContain("字体");
  });

  it("rejects unbalanced syntax with a plain-language reason (第一层检查)", () => {
    const r = applyIntent("把这个按钮改成蓝色", { content: ".a {\n  color: blue;\n", lang: "css" });
    expect(r.matched).toBe(true);
    expect(r.content).toBe(""); // 语法守卫拒绝
    expect(r.note).toContain("看不懂");
  });
});

describe("universal drill-down: structuralBlocks (chapter 1.2 fix)", () => {
  it("splits markdown by headings", () => {
    const md = "# 标题一\n正文\n## 子标题\n更多正文\n";
    const blocks = structuralBlocks("README.md", md, "markdown");
    expect(blocks.length).toBe(2);
    expect(blocks[0]!.name).toContain("标题一");
    expect(blocks[1]!.name).toContain("子标题");
  });

  it("splits css by selector rules", () => {
    const css = ".a {\n  color: red;\n}\n.b {\n  color: blue;\n}\n";
    const blocks = structuralBlocks("styles.css", css, "css");
    expect(blocks.map((b) => b.name)).toEqual([".a", ".b"]);
    expect(blocks[0]!.kindLabel).toContain("选择器");
  });

  it("splits json by top-level keys", () => {
    const json = '{\n  "name": "x",\n  "version": "1"\n}\n';
    const blocks = structuralBlocks("package.json", json, "json");
    expect(blocks.map((b) => b.name)).toEqual(["name", "version"]);
  });

  it("never refuses: unknown languages still produce 20-line chunks", () => {
    const code = Array.from({ length: 45 }, (_, i) => `line ${i}`).join("\n");
    const blocks = structuralBlocks("app.xyz", code, "generic");
    expect(blocks.length).toBe(3);
    expect(blocks[0]!.kindLabel).toContain("未识别");
  });
});

describe("L7 pattern recognition (detectPatterns)", () => {
  it("detects rest/express and spa markers", () => {
    const hits = detectPatterns([
      { relPath: "server.js", content: "const app = express();\napp.get('/');\n", truncated: false },
      { relPath: "main.tsx", content: "createRoot(document.getElementById('root'))\n", truncated: false },
    ]);
    expect(hits).toContain("rest");
    expect(hits).toContain("spa");
  });

  it("returns empty for unrelated code", () => {
    expect(detectPatterns([{ relPath: "a.py", content: "print(1)\n", truncated: false }])).toEqual([]);
  });
});

describe("L6 logic-block ranges (chapter 1.2 level 6)", () => {
  it("maps if/for blocks by brace matching", () => {
    const code = "function a() {\n  if (x) {\n    work();\n  }\n  for (const y of ys) {\n    go(y);\n  }\n}\n";
    const r = blockRanges(code.split("\n"), "ts");
    expect(r.get(2)).toBe(4);
    expect(r.get(5)).toBe(7);
  });

  it("maps python blocks by indentation", () => {
    const code = "def a():\n    if x:\n        work()\n    done()\n";
    const r = blockRanges(code.split("\n"), "python");
    expect(r.get(2)).toBe(3);
  });
});

describe("deep semantic translation (chapter 2)", () => {
  const a1 = analysis("src/login.ts", {
    symbols: [
      { kind: "function", name: "validate", line: 2, params: ["user"], endLine: 8 },
      { kind: "function", name: "issueToken", line: 10, params: ["user"], endLine: 18 },
    ],
    imports: [{ from: "crypto", names: [], line: 1 }],
    calls: [
      { from: "login", to: "validate" },
      { from: "issueToken", to: "validate" },
    ],
    exports: ["issueToken"],
  });
  const a2 = analysis("src/report.ts", {
    symbols: [{ kind: "function", name: "renderChart", line: 3, params: ["rows"], endLine: 20 }],
  });

  it("produces unique file descriptions that name the file's own functions", () => {
    const t1 = describeFileDeep(a1);
    const t2 = describeFileDeep(a2);
    expect(t1).toContain("validate()");
    expect(t2).toContain("renderChart()");
    expect(t1).not.toBe(t2);
  });

  it("supports three narration styles for functions", () => {
    const sym = a1.symbols.find((s) => s.name === "issueToken")!;
    const m = describeFunctionDeep(a1, sym, "metaphor");
    const s = describeFunctionDeep(a1, sym, "story");
    const e = describeFunctionDeep(a1, sym, "engineering");
    expect(m).toContain("机器 issueToken()");
    expect(s).toContain("第 1 步");
    expect(e).toContain("此函数 issueToken()");
  });

  it("dedupeCheck flags near-identical texts (2.4 品控)", () => {
    const dup = dedupeCheck(["alpha beta gamma", "alpha beta gamma!"]);
    expect(dup.length).toBe(1);
    expect(dedupeCheck(["alpha beta gamma", "completely different words here"]).length).toBe(0);
  });
});

describe("dictionary v2 scale & quality (chapter 3)", () => {
  const allTables = [L1_UNIVERSAL, L3_FRAMEWORK, L4_DOMAIN, L6_IDIOM, L7_PATTERN, ...Object.values(EXPANSION_PACKS), L2_LANGUAGE.jsts ?? {}, L2_LANGUAGE.python ?? {}, L2_LANGUAGE.rust ?? {}, L2_LANGUAGE.jvm ?? {}, L2_LANGUAGE.go ?? {}];
  const allTerms = allTables.flatMap((t) => Object.entries(t));

  it("expanded packs add 300+ new entries across 15 categories", () => {
    // 扩容只会增类不会减类：至少 15 类（第四轮起为 17 类）
    expect(Object.keys(EXPANSION_PACKS).length).toBeGreaterThanOrEqual(15);
    expect(EXPANSION_COUNT).toBeGreaterThanOrEqual(220);
  });

  it("no two entries share the same metaphor text ( uniqueness)", () => {
    const seen = new Map<string, string>();
    const dup: string[] = [];
    for (const [term, hit] of allTerms) {
      const prev = seen.get(hit.zh);
      if (prev) dup.push(`${prev}~${term}`);
      else seen.set(hit.zh, term);
    }
    expect(dup).toEqual([]);
  });

  it("flagship terms carry multi-style variants (三.2)", () => {
    expect(lookupTerm("variable", { overrides: {}, lang: "zh", style: "story" })).toContain("小机器人");
    expect(lookupTerm("variable", { overrides: {}, lang: "zh", style: "engineering" })).toContain("命名内存单元");
    expect(lookupTerm("function", { overrides: {}, lang: "zh", style: "story" })).toContain("小工人");
  });

  it("expansion packs are reachable through lookupTerm", () => {
    expect(lookupTerm("transaction", { overrides: {}, lang: "zh" })).toContain("整桌菜");
    expect(lookupTerm("container", { overrides: {}, lang: "zh" })).toContain("迷你房间");
    expect(lookupTerm("magic number", { overrides: {}, lang: "zh" })).toContain("神秘数字");
  });
});

describe("L6 idiom dictionary", () => {
  it("translates fallback/idempotent into plain words", () => {
    expect(lookupTerm("fallback", { overrides: {}, lang: "zh" })).toContain("备用方案");
    expect(lookupTerm("idempotent", { overrides: {}, lang: "zh" })).toContain("结果一样");
  });
});

describe("writeEngine (chapter 3 write safety)", () => {
  it("builds a timestamped backup path inside .backups (3.3)", () => {
    const p = backupPath("C:/proj/src/app.ts", new Date(2026, 0, 2, 3, 4, 5, 6).getTime());
    expect(p).toBe("C:/proj/src/.backups/app.ts.20260102-030405-006.bak");
  });

  it("detects unbalanced brackets after noise stripping (3.2.1)", () => {
    expect(checkBalanced("function a() { return 1; }", "ts").ok).toBe(true);
    expect(checkBalanced("function a() { return 1;", "ts").ok).toBe(false);
    // 字符串里的括号不算
    expect(checkBalanced('const s = "({";', "ts").ok).toBe(true);
  });

  it("keeps an undo stack with per-file tail lookup (4.6)", () => {
    const h = new EditHistory();
    h.push({ absPath: "a.ts", relPath: "a.ts", before: "x", after: "y", backupPath: "b", at: 1, utterance: "u" });
    h.push({ absPath: "b.ts", relPath: "b.ts", before: "1", after: "2", backupPath: "b", at: 2, utterance: "u" });
    expect(h.tailFor("a.ts")!.after).toBe("y");
    expect(h.pop()!.absPath).toBe("b.ts");
    expect(h.size).toBe(1);
  });
});
