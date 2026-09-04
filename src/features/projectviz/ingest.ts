/**
 * 编排器：把第一~五层引擎串成一条管线（规范第七章 7.1 导入流程）。
 * 解析按批让出事件循环，保证大项目导入时 UI 不卡死。
 */

import { ipc } from "../../lib/ipc";
import { detectProject } from "./detect";
import { parseSource } from "./parsers";
import { isBinaryExt, parseBinary } from "./binary";
import { buildGraph, type ProjectModel } from "./generate";
import type { FileAnalysis, GenGraph, ProjectScanResult } from "./types";

export type IngestPhase = "scan" | "detect" | "parse" | "generate" | "done" | "error";

export interface IngestProgress {
  phase: IngestPhase;
  /** 0..1 overall. */
  pct: number;
  detail: string;
}

export interface IngestResult {
  scan: ProjectScanResult;
  graph: GenGraph;
  model: ProjectModel;
}

export async function ingestProject(
  root: string,
  origin: { x: number; y: number },
  lang: "zh" | "en",
  onProgress: (p: IngestProgress) => void,
): Promise<IngestResult> {
  onProgress({ phase: "scan", pct: 0.05, detail: lang === "zh" ? "正在扫描项目文件…" : "Scanning project files…" });
  const scan = await ipc.projectScan(root);
  if (scan.entries.length === 0) {
    throw new Error(lang === "zh" ? "这个文件夹里没有可识别的文件" : "No recognizable files in this folder");
  }

  onProgress({ phase: "detect", pct: 0.25, detail: lang === "zh" ? "正在识别项目类型…" : "Detecting project type…" });
  const detect = detectProject(scan);

  onProgress({ phase: "parse", pct: 0.35, detail: lang === "zh" ? "正在解析源代码…" : "Parsing source code…" });
  const analyses = new Map<string, FileAnalysis>();
  const sources = scan.sources;
  let done = 0;
  for (const src of sources) {
    analyses.set(src.relPath, parseSource(src.relPath, src.content));
    done++;
    if (done % 40 === 0) {
      onProgress({
        phase: "parse",
        pct: 0.35 + 0.4 * (done / Math.max(1, sources.length)),
        detail: lang === "zh" ? `已解析 ${done}/${sources.length} 个文件` : `Parsed ${done}/${sources.length} files`,
      });
      // Yield so the progress overlay can paint.
      await new Promise<void>((r) => window.setTimeout(r, 0));
    }
  }

  // Binary anatomy: .dll/.exe/.so/.dylib etc. get a real header-level parse
  // (format/arch/sections/imports/exports) with the same drill-down surface.
  const binaries = scan.entries.filter((e) => e.kind === "file" && isBinaryExt(e.ext)).slice(0, 24);
  for (const be of binaries) {
    try {
      analyses.set(be.path, await analyzeBinaryFile(root, be.path));
      done++;
      if (done % 4 === 0) {
        onProgress({
          phase: "parse",
          pct: 0.35 + 0.4 * (done / Math.max(1, sources.length + binaries.length)),
          detail: lang === "zh" ? `已解析二进制 ${done}` : `Parsed binaries ${done}`,
        });
        await new Promise<void>((r) => window.setTimeout(r, 0));
      }
    } catch { /* unreadable binary → skip */ }
  }

  // Reverse dependency index: who imports each internal file (spec 5.2 被谁使用).
  const usedBy = new Map<string, string[]>();
  const relSet = new Set(analyses.keys());
  for (const analysis of analyses.values()) {
    for (const imp of analysis.imports) {
      for (const candidate of matchCandidates(analysis.relPath, imp.from)) {
        for (const rel of relSet) {
          if (rel === candidate || rel.endsWith(candidate)) {
            const list = usedBy.get(rel) ?? [];
            if (!list.includes(analysis.relPath)) list.push(analysis.relPath);
            usedBy.set(rel, list);
          }
        }
      }
    }
  }

  onProgress({ phase: "generate", pct: 0.85, detail: lang === "zh" ? "正在生成思维导图…" : "Generating mind map…" });
  const model: ProjectModel = { scan, detect, analyses, usedBy };
  const graph = buildGraph(model, origin, lang);
  if (graph.nodes.length === 0) {
    throw new Error(lang === "zh" ? "项目太大，导图节点超出上限" : "Project too large: node cap reached");
  }
  onProgress({ phase: "done", pct: 1, detail: lang === "zh" ? `完成：${graph.nodes.length} 个节点` : `Done: ${graph.nodes.length} nodes` });
  return { scan, graph, model };
}

function matchCandidates(fromRel: string, spec: string): string[] {
  if (!spec.startsWith(".")) return [];
  const base = normalizeRel(fromRel, spec);
  const out = [base];
  for (const ext of ["ts", "tsx", "js", "jsx", "py", "rs", "go", "java", "cs", "dart", "php", "rb"]) {
    out.push(`${base}.${ext}`);
  }
  out.push(`${base}/index.ts`, `${base}/index.js`, `${base}/mod.rs`, `${base}/__init__.py`);
  return out;
}

export function normalizeRel(fromRel: string, spec: string): string {
  const dirParts = fromRel.includes("/") ? fromRel.slice(0, fromRel.lastIndexOf("/")).split("/") : [];
  const parts = spec.replace(/^\.\//, "").split("/");
  const stack = [...dirParts];
  for (const p of parts) {
    if (p === "." || p === "") continue;
    else if (p === "..") stack.pop();
    else stack.push(p);
  }
  return stack.join("/").replace(/\.[^.]+$/, "");
}

/** 单文件延迟重解析（信息卡 / 下钻 / 8.2 学习后刷新）。 */
export async function reanalyzeFile(root: string, relPath: string): Promise<FileAnalysis> {
  const ext = relPath.includes(".") ? relPath.split(".").pop()!.toLowerCase() : null;
  if (isBinaryExt(ext)) return analyzeBinaryFile(root, relPath);
  const src = await ipc.projectReadFile(root, relPath);
  return parseSource(relPath, src.content);
}

/** Binary anatomy → a FileAnalysis whose imports/exports feed the same graph. */
export async function analyzeBinaryFile(root: string, relPath: string): Promise<FileAnalysis> {
  const raw = await ipc.projectReadBytes(root, relPath);
  const bytes = new Uint8Array(raw.bytes);
  const info = parseBinary(bytes, raw.size, raw.truncated);
  const base: FileAnalysis = {
    relPath,
    lang: "generic",
    role: "asset",
    imports: (info?.imports ?? []).map((im) => ({ from: im.module, names: im.names, line: 0 })),
    symbols: (info?.exports ?? []).slice(0, 60).map((n) => ({ kind: "function" as const, name: n, line: 0, params: [], endLine: 0 })),
    calls: [],
    exports: info?.exports ?? [],
    loc: 0,
  };
  return info ? { ...base, binary: info } : base;
}
