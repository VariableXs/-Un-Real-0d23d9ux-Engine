/**
 * 项目结构可视化引擎 — 统一类型（规范第一~六章的数据契约）。
 * Types shared by the scan → parse → narrate → generate pipeline.
 * Rust 侧 project_scan.rs 的 serde camelCase 模型与此一一对应。
 */

// ---------- Layer 1: file-system scan (Rust → TS) ----------

export interface ScanEntry {
  /** Relative path with `/` separators; root itself is `""`. */
  path: string;
  name: string;
  kind: "dir" | "file";
  ext: string | null;
  size: number;
  /** 0 = direct children of the root. */
  depth: number;
}

export interface SourceFile {
  relPath: string;
  content: string;
  truncated: boolean;
}

export interface ProjectScanResult {
  root: string;
  entries: ScanEntry[];
  sources: SourceFile[];
  truncated: boolean;
  skipped: number;
}

// ---------- Layer 2: multi-language parsing → Universal IR (spec 3.2) ----------

export type LangId =
  | "ts" | "js" | "python" | "rust" | "go" | "java" | "kotlin" | "csharp"
  | "swift" | "php" | "ruby" | "cpp" | "c" | "dart" | "html" | "css"
  | "sql" | "json" | "yaml" | "markdown" | "shell" | "generic"
  // 第四轮扩容：8 种此前未覆盖的语言（与既有条目零重复）
  | "lua" | "perl" | "scala" | "haskell" | "elixir" | "zig" | "julia" | "r";

export type UirKind =
  | "module" | "class" | "function" | "variable"
  | "import" | "call" | "condition" | "loop" | "comment";

/** One parsed symbol: class / function / top-level variable. */
export interface SymbolInfo {
  kind: "class" | "function" | "variable";
  name: string;
  /** 1-based line in the original source. */
  line: number;
  params: string[];
  /** Approximate end line (start of the next top-level symbol). */
  endLine: number;
}

export interface ImportInfo {
  /** Module specifier exactly as written. */
  from: string;
  /** Names imported (empty = side-effect / whole-module import). */
  names: string[];
  line: number;
}

/**
 * Intra-file call chain approximation: function A calls function B.
 */
export interface CallEdge {
  from: string;
  to: string;
}

/** 递归流程单元：全部分支/循环的可无限下钻树（代码分析加强章）。 */
export interface FlowUnit {
  kind: "branch" | "loop" | "switch" | "guard" | "match";
  label: string;
  /** 1-based line in the original source. */
  line: number;
  children: FlowUnit[];
}

export interface FileAnalysis {
  relPath: string;
  lang: LangId;
  /** File role bucket used for narration + colors (chapter 10.2). */
  role: FileRole;
  imports: ImportInfo[];
  symbols: SymbolInfo[];
  calls: CallEdge[];
  exports: string[];
  loc: number;
  /** Recursive branch/loop tree for full-flow subdivision (代码分析加强). */
  flow?: FlowUnit[];
  /** Parsed binary anatomy when the file is a .dll/.exe/.so/.dylib etc. */
  binary?: import("./binary").BinaryInfo;
}

export type FileRole = "source" | "config" | "doc" | "asset" | "entry";

// ---------- Layer 3: semantic domain recognition (spec 2.2) ----------

export type ProjectTypeId =
  | "react" | "next" | "vue" | "node" | "rust" | "python" | "go" | "java"
  | "android" | "ios" | "dotnet" | "php" | "ruby" | "flutter" | "unity"
  | "docker" | "generic";

export interface ProjectDetect {
  typeId: ProjectTypeId;
  /** 一句话通俗定位 (spec: "一个用网页做的应用"). */
  label: string;
  /** 所属领域 (L4 词典). */
  domain: string;
  /** 领域键（web/desktop/data/cli/mobile/game），v3.0 2.1 字典联动用。 */
  domainKey: string;
  /** Evidence chain: file/dir facts that produced the verdict. */
  evidence: string[];
  /** Human-readable tech stack (frameworks, language, tooling). */
  stack: string[];
  /** Entry-point candidates (relPaths), best first. */
  entryCandidates: string[];
  /** Primary language id used by the forward generator (chapter 6). */
  primaryLang: LangId;
  /** L7 模式识别命中的架构/风格标签（如 redux / rest / cicd）。 */
  patterns?: string[];
}

// ---------- Layer 5: mindmap graph output ----------

export type NodeKind = "root" | "branch" | "dir" | "source" | "config" | "doc" | "asset" | "flow" | "intent" | "info";

export interface GenNode {
  key: string;
  /** Pre-escaped HTML for MindNode.textHtml. */
  html: string;
  plain: string;
  kind: NodeKind;
  x: number;
  y: number;
  w: number;
  h: number;
  /** `pv:<relPath>` / `pv-root:<absRoot>` — persisted, drives double-click. */
  recordId?: string;
}

export interface GenEdge {
  from: string;
  to: string;
  /** Dependency/flow edges get the animated circuit look (spec 5.5). */
  animated: boolean;
  color: string;
  label?: string;
}

export interface GenGraph {
  nodes: GenNode[];
  edges: GenEdge[];
  rootKey: string;
  /** relPath → node key, for info cards / drill-down wiring. */
  fileKeys: Map<string, string>;
}

// ---------- chapter 6: forward generation (大白话 → 代码) ----------

export interface IntentPlan {
  matched: boolean;
  title: string;
  /** 先做什么后做什么, plain language steps (spec 6.2). */
  steps: string[];
  code: string;
  codeLang: string;
  /** Suggested paste location inside the project (spec 6.2). */
  targetFile: string;
  /** One-sentence plain-language explanation of the approach (spec 6.2). */
  explanation: string;
  /** Where the plan matched an existing file, this is its relPath. */
  anchorFile?: string;
  /** Terms the dictionary could not translate (spec 8.2 teach-in). */
  unknownTerms: string[];
}

// ---------- ch.12 独立项目档案（.project 文件，与 .mindmap 平行） ----------

/** 1.3 双向引用管道：项目分析空间 ↔ 思维导图空间的一次引用。 */
export interface CrossRef {
  id: string;
  /** "file" | "function" */
  kind: "file" | "function";
  relPath: string;
  /** Symbol name for function-level refs. */
  name?: string;
  /** Mindmap side anchor. */
  mapId: string;
  nodeId: string;
  createdAt: number;
}

/** 12.1 .project 档案文件结构。 */
export interface ProjectArchive {
  formatVersion: 1;
  /** Original project root (absolute display path). */
  root: string;
  savedAt: number;
  detect: ProjectDetect;
  /** Serialized generated graph (nodes + edges + fileKeys). */
  graph: GenGraph;
  /** 12.1 目录树快照（导入时的 entries，供文件树使用）。 */
  entries?: ScanEntry[];
  /** User annotations keyed by relPath (ch.12.1 用户自定义注释). */
  notes: Record<string, string>;
  /** Bidirectional references to mindmap nodes (ch.1.3). */
  refs: CrossRef[];
  /** 第四章/四.3：单独加入分析台的散装文件（absPath 定位，rel 为展示名）。 */
  external?: Array<{ absPath: string; rel: string }>;
}
