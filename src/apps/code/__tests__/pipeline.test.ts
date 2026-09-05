import { describe, expect, it } from "vitest";
import { detectProject } from "../detect";
import { buildGraph, resolveOverlaps, routeEdge, CROSS_REF_COLOR, KIND_BORDER, type ProjectModel } from "../generate";
import type { GenNode } from "../types";
import { abilityList, purposeSentence } from "../narrate";
import { lookupTerm, translateText } from "../dictionaries";
import { planIntent } from "../intent";
import type { FileAnalysis, ProjectScanResult, ScanEntry } from "../types";

function entry(path: string, kind: "dir" | "file", ext: string | null = null, depth = 0): ScanEntry {
  const name = path.split("/").pop() ?? path;
  return { path, name, kind, ext, size: 100, depth };
}

function analysis(relPath: string, over: Partial<FileAnalysis> = {}): FileAnalysis {
  return { relPath, lang: "ts", role: "source", imports: [], symbols: [], calls: [], exports: [], loc: 10, ...over };
}

function reactScan(): ProjectScanResult {
  return {
    root: "C:/demo/web",
    entries: [
      entry("package.json", "file", "json"),
      entry("src", "dir"),
      entry("src/main.tsx", "file", "tsx", 1),
      entry("src/App.tsx", "file", "tsx", 1),
      entry("README.md", "file", "md"),
      entry("assets", "dir"),
      entry("assets/logo.png", "file", "png", 1),
    ],
    sources: [
      { relPath: "package.json", content: '{"name":"web","dependencies":{"react":"^18.3.1"}}', truncated: false },
      { relPath: "src/main.tsx", content: "import App from './App';\n", truncated: false },
      { relPath: "src/App.tsx", content: "export default function App(){return null}\n", truncated: false },
    ],
    truncated: false,
    skipped: 0,
  };
}

function reactModel(): ProjectModel {
  const scan = reactScan();
  const detect = detectProject(scan);
  const analyses = new Map<string, FileAnalysis>();
  analyses.set("src/main.tsx", analysis("src/main.tsx", {
    role: "entry", imports: [{ from: "./App", names: ["App"], line: 1 }],
    symbols: [{ kind: "function", name: "main", line: 2, params: [], endLine: 4 }],
  }));
  analyses.set("src/App.tsx", analysis("src/App.tsx", {
    role: "source", exports: ["App"],
    symbols: [{ kind: "function", name: "App", line: 1, params: [], endLine: 3 }],
  }));
  analyses.set("README.md", analysis("README.md", { role: "doc", lang: "markdown" }));
  analyses.set("package.json", analysis("package.json", { role: "config", lang: "json" }));
  return { scan, detect, analyses, usedBy: new Map([["src/App.tsx", ["src/main.tsx"]]]) };
}

describe("detectProject (spec 2.2 evidence chain)", () => {
  it("recognizes a React project via package.json + react dependency", () => {
    const d = detectProject(reactScan());
    expect(d.typeId).toBe("react");
    expect(d.label).toContain("网页");
    expect(d.stack).toContain("react");
    expect(d.entryCandidates).toContain("src/main.tsx");
  });

  it("recognizes a Rust project via Cargo.toml", () => {
    const scan: ProjectScanResult = {
      root: "C:/demo/rs",
      entries: [entry("Cargo.toml", "file", "toml"), entry("src", "dir"), entry("src/main.rs", "file", "rs", 1)],
      sources: [{ relPath: "src/main.rs", content: "fn main() {}\n", truncated: false }],
      truncated: false,
      skipped: 0,
    };
    const d = detectProject(scan);
    expect(d.typeId).toBe("rust");
    expect(d.primaryLang).toBe("rust");
  });

  it("falls back to generic with an explanation", () => {
    const scan: ProjectScanResult = {
      root: "C:/demo/unknown", entries: [entry("a.xyz", "file", "xyz")], sources: [], truncated: false, skipped: 0,
    };
    const d = detectProject(scan);
    expect(d.typeId).toBe("generic");
    expect(d.evidence.length).toBeGreaterThan(0);
  });
});

describe("buildGraph (spec 5.1 / 5.3 / 10.2)", () => {
  const graph = buildGraph(reactModel(), { x: 0, y: 0 }, "zh");

  it("creates a root with three branches", () => {
    expect(graph.rootKey).not.toBe("");
    const kinds = graph.nodes.map((n) => n.kind);
    expect(kinds.filter((k) => k === "branch").length).toBe(3);
    const branchTexts = graph.nodes.filter((n) => n.kind === "branch").map((n) => n.plain);
    expect(branchTexts).toEqual(expect.arrayContaining(["是什么", "有什么", "怎么运行"]));
  });

  it("uses semantic low-saturation colors (spec 10.2)", () => {
    expect(KIND_BORDER.root).toBe("#4f709c");
    expect(KIND_BORDER.dir).toBe("#8babc6");
    expect(KIND_BORDER.source).toBe("#a8c8e8");
    expect(KIND_BORDER.config).toBe("#cfe4f8");
    expect(KIND_BORDER.doc).toBe("#dce4ec");
    expect(KIND_BORDER.intent).toBe("#f5c6d8");
    expect(CROSS_REF_COLOR).toBe("#f5c6d8");
  });

  it("maps file nodes to pv: recordIds for info cards", () => {
    expect(graph.fileKeys.get("src/App.tsx")).toBeDefined();
    const node = graph.nodes.find((n) => n.recordId === "pv:src/App.tsx");
    expect(node).toBeDefined();
    expect(node!.kind).toBe("source");
  });

  it("builds an animated flow chain from the entry following imports", () => {
    const flowNodes = graph.nodes.filter((n) => n.kind === "flow");
    expect(flowNodes.length).toBeGreaterThanOrEqual(2);
    expect(flowNodes[0]!.plain).toContain("启动");
    const rootId = graph.nodes.find((n) => n.kind === "branch")!.key;
    const animated = graph.edges.filter((e) => e.animated);
    expect(animated.length).toBeGreaterThanOrEqual(1);
    void rootId;
  });

  it("skips asset files in the 有什么 branch but keeps dirs with metaphors", () => {
    const dirNode = graph.nodes.find((n) => n.kind === "dir");
    expect(dirNode).toBeDefined();
    expect(dirNode!.recordId).toBe("pv-dir:src");
    expect(graph.nodes.some((n) => n.recordId === "pv:assets/logo.png")).toBe(false);
  });
});

describe("layout & edge routing (chapter 3 refactor)", () => {
  function node(key: string, x: number, y: number, w = 240, h = 64): GenNode {
    return { key, html: "", plain: "", kind: "source", x, y, w, h };
  }

  it("resolveOverlaps enforces the 44px vertical safety gap (三章)", () => {
    const nodes = [
      node("a", 0, 0),
      node("b", 20, 30),   // 与 a 的 x 区间相交且间距 < 44 → 被推下
      node("c", 900, 0),   // x 不相交 → 不动
    ];
    const out = resolveOverlaps(nodes);
    const b = out.find((n) => n.key === "b")!;
    expect(b.y).toBeGreaterThanOrEqual(64 + 44);
    const c = out.find((n) => n.key === "c")!;
    expect(c.y).toBe(0);
  });

  it("routeEdge keeps the bezier for clear paths (三章)", () => {
    const s = node("s", 0, 0);
    const t = node("t", 500, 0);
    const p = routeEdge(s, t, [s, t, node("far", 0, 900)]);
    expect(p.startsWith("M 240 32 C")).toBe(true);
  });

  it("routeEdge detours around blocking nodes instead of piercing them (三章)", () => {
    const s = node("s", 0, 0);
    const t = node("t", 600, 0);
    const blocker = node("b", 280, 10, 60, 48); // 正好横在连线上
    const p = routeEdge(s, t, [s, t, blocker]);
    expect(p).not.toContain("C");              // 改为直角绕行
    expect(p).toContain("L");                  // 折线路径
  });
});

describe("narrate (chapter 4 plain-language engine)", () => {
  it("describes the project purpose with domain", () => {
    const m = reactModel();
    const s = purposeSentence(m.detect, m.scan, "zh");
    expect(s).toContain("网页");
    expect(s).toContain("文件");
  });

  it("translates symbols into machine/blueprint wording", () => {
    const a = analysis("src/App.tsx", {
      symbols: [
        { kind: "class", name: "Widget", line: 1, params: [], endLine: 5 },
        { kind: "function", name: "run", line: 2, params: ["speed"], endLine: 6 },
      ],
    });
    const list = abilityList(a, "zh");
    expect(list.some((l) => l.includes("图纸") && l.includes("Widget"))).toBe(true);
    expect(list.some((l) => l.includes("机器 run()") && l.includes("speed"))).toBe(true);
  });
});

describe("dictionaries (L1-L4 + overrides + unknown terms, spec 8.2)", () => {
  it("translates universal terms", () => {
    expect(lookupTerm("function", { overrides: {}, lang: "zh" })).toContain("机器");
    expect(lookupTerm("React", { overrides: {}, lang: "zh" })).toContain("积木");
  });

  it("user overrides win over built-ins", () => {
    expect(lookupTerm("callback", { overrides: { callback: "售后服务电话" }, lang: "zh" })).toBe("售后服务电话");
  });

  it("unknown terms return null and get flagged", () => {
    const ctx = { overrides: {}, lang: "zh" as const };
    expect(lookupTerm("quantum flux capacitor", ctx)).toBeNull();
    const r = translateText("这是一段关于 function 的描述，还有 quantum flux capacitor", ["quantum flux capacitor", "function"], ctx);
    expect(r.unknown).toContain("quantum flux capacitor");
    expect(r.out).toContain("机器");
  });
});

describe("planIntent (chapter 6 forward generation)", () => {
  const detect = detectProject(reactScan());

  it("turns plain language into a button component with steps and paste location", () => {
    const plan = planIntent("我想做一个按钮，用户点了之后就弹出一个'你好'的提示框", detect, "zh");
    expect(plan.matched).toBe(true);
    expect(plan.steps.length).toBe(3);
    expect(plan.code).toContain("alert");
    expect(plan.targetFile).toMatch(/^src\/components\/\w+\.tsx$/);
  });

  it("generates fetch helpers for data requests", () => {
    const plan = planIntent("我想请求接口拿数据", detect, "zh");
    expect(plan.matched).toBe(true);
    expect(plan.code).toContain("fetch");
  });

  it("reports unmatched intents honestly with unknown terms", () => {
    const plan = planIntent("我想训练一个transformer大模型", detect, "zh");
    expect(plan.matched).toBe(false);
    expect(plan.unknownTerms.length).toBeGreaterThan(0);
  });
});
