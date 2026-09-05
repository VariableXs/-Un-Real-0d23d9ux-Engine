/**
 * 项目可视化引擎 UI 面板（规范第五~八章）：
 * - ProjectImportOverlay：第七/八章 扫描进度 = 顶栏微进度条（不挡画布，禁止大弹窗）
 * - FileInfoCard：5.2 文件信息详情舱 + 5.4 下钻 + 8.2 教学式词典补充
 * 「大白话 → 代码」不是弹窗：它是项目分析空间内的画布工具模式（见 ProjectAnalysisView）。
 */

import { useEffect, useState } from "react";
import { X, ExternalLink, GitBranch, Loader2, AlertTriangle, FolderOpen, Link2, ChevronDown, ChevronRight } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { loadSettings, saveSetting } from "../../lib/settings";
import { pushToast } from "../../state/uiStore";
import { askPrompt } from "../../components/Modal";
import { lookupTerm } from "./dictionaries";
import { fileOneLiner, abilityList } from "./narrate";
import type { FileAnalysis } from "./types";
import type { ProjectModel } from "./generate";
import type { IngestProgress } from "./ingest";

function joinPath(root: string, rel: string): string {
  const sep = root.includes("\\") || /^[A-Za-z]:/.test(root) ? "\\" : "/";
  return `${root.replace(/[\\/]+$/, "")}${sep}${rel.replace(/\//g, sep)}`;
}

// ================= 第七章·八：扫描进度 = 顶栏微进度条（不挡画布，禁止大弹窗） =================

export interface PvImportState {
  root: string;
  progress?: IngestProgress;
  error?: string;
}

export function ProjectImportOverlay(props: { state: PvImportState | null; onCancel: () => void }): React.ReactElement | null {
  const { lang } = useI18n();
  const s = props.state;
  if (!s) return null;
  const pct = s.progress?.pct ?? 0.02;
  const detail = s.error
    ? s.error
    : s.progress?.detail ?? (lang !== "en" ? "准备中…" : "Preparing…");
  return (
    <div className={`pv-progress-strip ${s.error ? "err" : ""}`} role="status" aria-live="polite">
      {s.error
        ? <AlertTriangle size={13} className="pv-warn" />
        : <Loader2 size={13} className="pv-spin" />}
      <span className="pv-strip-text">{detail}</span>
      <div className="pv-strip-bar"><div className="pv-strip-fill" style={{ width: `${Math.round(pct * 100)}%` }} /></div>
      <button type="button" className="btn tiny ghost" onClick={props.onCancel}>
        {s.error ? (lang !== "en" ? "关闭" : "Dismiss") : (lang !== "en" ? "后台" : "Bg")}
      </button>
    </div>
  );
}

// ================= 5.2 / 5.4 / 8.2 文件信息详情舱 =================

export function FileInfoCard(props: {
  node: import("../../lib/types").MindNode;
  model: ProjectModel | null;
  onClose: () => void;
  onDrill: (analysis: FileAnalysis) => void;
  /** 5.2 引用到思维导图（双向引用管道，独立空间提供）。 */
  onRefToMindmap?: (relPath: string, name: string, label: string) => void;
  /** Already-referenced relPaths (button shows state). */
  referenced?: Set<string>;
  /** 2.5 通俗风格（生活比喻/故事叙事/工程说明）。 */
  style?: "metaphor" | "story" | "engineering";
  /** 四.2 进入逐行解剖（完整源码双轨视图）。 */
  onOpenAnatomy?: () => void;
}): React.ReactElement {
  const { lang } = useI18n();
  const record = props.node.recordId ?? "";
  // recordId 形如 `pv:<relPath>` 或 `pv:<relPath>@<projectRoot>`（独立空间引用用）。
  const body = record.startsWith("pv:") ? record.slice(3) : "";
  const at = body.lastIndexOf("@");
  const relPath = at > 0 ? body.slice(0, at) : body;
  const rootHint = at > 0 ? body.slice(at + 1) : "";
  const [analysis, setAnalysis] = useState<FileAnalysis | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [overrides, setOverrides] = useState<Record<string, string>>({});

  // Root path comes from props.model.scan.root (persisted via the pv-root
  // node's recordId in MindmapView, so cards survive reloads).

  useEffect(() => {
    void loadSettings().then((s) => setOverrides(s.pvzDictOverrides)).catch(() => {});
  }, []);

  useEffect(() => {
    let alive = true;
    setError(null);
    const cached = props.model?.analyses.get(relPath);
    if (cached) {
      setAnalysis(cached);
      return;
    }
    // Reloaded session: re-derive the analysis lazily from the source file.
    const root = props.model?.scan.root ?? rootHint;
    if (!root) {
      setError(lang !== "en" ? "找不到项目根目录（请重新导入项目）" : "Project root not found (re-import the project)");
      return;
    }
    setAnalysis(null);
    import("./ingest").then(({ reanalyzeFile }) =>
      reanalyzeFile(root, relPath).then((a) => {
        if (alive) setAnalysis(a);
      }).catch((e) => {
        if (alive) setError(errMessage(e).message);
      }),
    ).catch(() => {});
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [relPath]);

  const usedBy = props.model?.usedBy.get(relPath) ?? null;
  const ctx = { overrides, lang };
  const style = props.style ?? "metaphor";
  const root = props.model?.scan.root ?? rootHint;
  /** 四.2 源码可见性：信息卡内嵌前 10 行真实代码预览。 */
  const [preview, setPreview] = useState<string[] | null>(null);
  useEffect(() => {
    if (!analysis || !root) { setPreview(null); return; }
    let alive = true;
    void (async () => {
      try {
        const content = await ipc.projectReadFile(root, relPath);
        if (alive) setPreview(content.content.split("\n").slice(0, 10));
      } catch { if (alive) setPreview(null); }
    })();
    return () => { alive = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [!!analysis, relPath, root]);

  async function teachTerm(term: string): Promise<void> {
    const meaning = await askPrompt({
      title: lang !== "en" ? `“${term}”是什么？` : `What is "${term}"?`,
      initial: "",
    });
    if (!meaning || !meaning.trim()) return;
    const merged = { ...overrides, [term.toLowerCase().trim()]: meaning.trim() };
    setOverrides(merged);
    await saveSetting("pvzDictOverrides", merged).catch(() => {});
    pushToast("success", lang !== "en" ? "已记住你的解释" : "Learned your explanation");
  }

  function openSource(): void {
    const root = props.model?.scan.root ?? rootHint;
    if (!root) return;
    void ipc.openPath(joinPath(root, relPath)).catch((e) =>
      pushToast("error", lang !== "en" ? "打开失败" : "Open failed", errMessage(e).message));
  }

  return (
    <div className="pv-info-card card-pop" role="dialog" aria-label={lang !== "en" ? "文件解读" : "File explanation"}>
      <div className="pv-info-head">
        <FolderOpen size={14} />
        <strong>{relPath.split("/").pop()}</strong>
        <span className="pv-rel">{relPath}</span>
        <button type="button" className="icon-btn tiny pv-close" aria-label="close" onClick={props.onClose}><X size={13} /></button>
      </div>
      <div className="pv-info-body">
        {error && <p className="pv-warn-text">{error}</p>}
        {!analysis && !error && <p className="dim">{lang !== "en" ? "正在读取源码…" : "Reading source…"}</p>}
        {analysis && (
          <>
            <section>
              <h4>{lang !== "en" ? "这个文件是干嘛的（五维深度翻译）" : "What this file does (5-dim)"}</h4>
              <p>
                {analysis.role === "doc" || analysis.role === "config" || analysis.role === "asset"
                  ? (lang !== "en" ? "它是一份说明/配置/资源文件。" : "It is a doc/config/asset file.")
                  : fileOneLiner(analysis, lang, style)}
              </p>
              {preview && (
                <div className="pv-code-preview">
                  {preview.map((l, i) => (
                    <div key={i} className="pv-code-preview-row"><span className="pv-ln">{i + 1}</span>{l}</div>
                  ))}
                  <div className="dim small">… {lang !== "en" ? "仅预览前 10 行，完整源码走逐行解剖" : "first 10 lines — open anatomy for the rest"}</div>
                </div>
              )}
            </section>
            <section>
              <h4>{lang !== "en" ? "主要功能（大白话）" : "Abilities in plain words"}</h4>
              <ul className="pv-list">
                {(analysis.symbols.length === 0
                  ? [lang !== "en" ? "主要是声明和配置，没有独立的机器或图纸。" : "Mostly declarations; no standalone functions/classes."]
                  : abilityList(analysis, lang, 6, style)
                ).map((line, i) => <li key={i}>{line}</li>)}
              </ul>
            </section>
            <section>
              <h4>{lang !== "en" ? "依赖关系（它借了谁的工具）" : "Dependencies (whose tools it borrows)"}</h4>
              <ul className="pv-list">
                {(analysis.imports.length === 0
                  ? [lang !== "en" ? "不依赖其他模块，自己是独立的。" : "No dependencies."]
                  : analysis.imports.slice(0, 6).map((imp) => {
                    const base = imp.from.split(/[/:]/).filter(Boolean).pop() ?? imp.from;
                    const hit = lookupTerm(base, ctx);
                    return (
                      <li key={`${imp.from}:${imp.line}`}>
                        {lang !== "en" ? `借了工具箱 ${imp.from}` : `Borrows ${imp.from}`}
                        {hit ? <span className="pv-term"> —— {hit}</span> : (
                          <button type="button" className="btn tiny ghost pv-teach" onClick={() => void teachTerm(base)}>
                            {lang !== "en" ? "这个词我还不懂，点我补充" : "Unknown term — teach me"}
                          </button>
                        )}
                      </li>
                    );
                  })
                ).map((node, i) => <li key={i}>{node}</li>)}
              </ul>
            </section>
            <section>
              <h4>{lang !== "en" ? "被谁使用" : "Used by"}</h4>
              <p>
                {usedBy === null
                  ? (lang !== "en" ? "（重新导入项目后可计算引用关系）" : "(Re-import the project to compute reverse deps)")
                  : usedBy.length === 0
                    ? (lang !== "en" ? "项目里没有其他文件引用它。" : "Nothing else references it.")
                    : usedBy.slice(0, 6).join(lang !== "en" ? "、" : ", ")}
              </p>
            </section>
            {analysis.binary && (
              <section>
                <h4>{lang !== "en" ? "二进制解剖（可下钻结构）" : "Binary anatomy (drillable)"}</h4>
                <p>
                  {analysis.binary.format} · {analysis.binary.arch} · {analysis.binary.bits}-bit · {analysis.binary.kind}
                  {analysis.binary.timestamp ? ` · ${analysis.binary.timestamp}` : ""}
                </p>
                <ul className="pv-list">
                  {analysis.binary.sections.slice(0, 12).map((s) => (
                    <li key={s.name}>
                      {lang !== "en" ? "节区" : "Section"} <strong>{s.name}</strong> · {(s.vsize / 1024).toFixed(1)} KB
                      {s.flags.length > 0 ? ` · ${s.flags.join("/")}` : ""}
                    </li>
                  ))}
                  {analysis.binary.imports.slice(0, 8).map((im) => (
                    <li key={im.module}>
                      {lang !== "en" ? "导入" : "Import"} <strong>{im.module}</strong>
                      {im.names.length > 0 ? ` — ${im.names.slice(0, 8).join(", ")}${im.names.length > 8 ? ` +${im.names.length - 8}` : ""}` : ""}
                    </li>
                  ))}
                  {analysis.binary.exports.length > 0 && (
                    <li>
                      {lang !== "en"
                        ? `导出 ${analysis.binary.exports.length} 个符号: ${analysis.binary.exports.slice(0, 10).join(", ")}`
                        : `Exports ${analysis.binary.exports.length} symbols: ${analysis.binary.exports.slice(0, 10).join(", ")}`}
                    </li>
                  )}
                  {analysis.binary.notes.map((n, i) => <li key={i} className="dim">{n}</li>)}
                </ul>
              </section>
            )}
            {analysis.flow && analysis.flow.length > 0 && (
              <section>
                <h4>{lang !== "en" ? "全流程细分（全部分支可无限下钻）" : "Full-flow subdivision (every branch drillable)"}</h4>
                <FlowTreeView units={analysis.flow} lang={lang} />
              </section>
            )}
          </>
        )}
      </div>
      {analysis && (
        <div className="pv-info-foot">
          <button type="button" className="btn tiny ghost" onClick={openSource}>
            <ExternalLink size={12} /> {lang !== "en" ? "打开源文件" : "Open source"}
          </button>
          <button type="button" className="btn tiny primary" onClick={() => props.onDrill(analysis)}>
            <GitBranch size={12} /> {lang !== "en" ? "下钻进入（函数级）" : "Drill into functions"}
          </button>
          {props.onOpenAnatomy && (
            <button type="button" className="btn tiny ghost" onClick={props.onOpenAnatomy}>
              <FolderOpen size={12} /> {lang !== "en" ? "逐行解剖（完整源码）" : "Line anatomy"}
            </button>
          )}
          {props.onRefToMindmap && (
            <button
              type="button"
              className="btn tiny pv-ref-btn"
              onClick={() => props.onRefToMindmap?.(relPath, relPath.split("/").pop() ?? relPath, analysis.symbols[0]?.name ?? "")}
            >
              <Link2 size={12} />
              {props.referenced?.has(relPath)
                ? (lang !== "en" ? "已在导图中" : "Referenced")
                : (lang !== "en" ? "引用到思维导图" : "Ref to mindmap")}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/** 递归流程树：每个分支/循环可无限下钻，默认展开前两层。 */
export function FlowTreeView(props: { units: import("./types").FlowUnit[]; lang: "zh" | "zh-TW" | "en" }): React.ReactElement {
  const { lang } = useI18n();
  const [open, setOpen] = useState<Set<string>>(new Set());
  const keyOf = (u: import("./types").FlowUnit): string => `${u.line}:${u.kind}`;
  const toggle = (k: string): void => {
    setOpen((s) => {
      const n = new Set(s);
      if (n.has(k)) n.delete(k); else n.add(k);
      return n;
    });
  };
  const KIND_LABEL: Record<string, string> = lang === "en"
    ? { branch: "if", loop: "loop", switch: "switch", guard: "try", match: "match" }
    : { branch: "分支", loop: "循环", switch: "多路", guard: "守护", match: "匹配" };
  const render = (units: import("./types").FlowUnit[], depth: number): React.ReactElement[] =>
    units.map((u) => {
      const k = keyOf(u);
      const hasKids = u.children.length > 0;
      const expanded = depth < 2 || open.has(k);
      return (
        <div key={k} className="pv-flow-unit" style={{ marginLeft: depth * 14 }}>
          <div className="pv-flow-row" onClick={() => hasKids && toggle(k)}>
            {hasKids ? (expanded ? <ChevronDown size={11} /> : <ChevronRight size={11} />) : <span className="pv-flow-leaf" />}
            <span className={`pv-flow-kind k-${u.kind}`}>{KIND_LABEL[u.kind] ?? u.kind}</span>
            <span className="pv-flow-label ellipsis">{u.label}</span>
            <span className="dim small pv-flow-ln">L{u.line}</span>
          </div>
          {hasKids && expanded && render(u.children, depth + 1)}
        </div>
      );
    });
  if (props.units.length === 0) {
    return <p className="dim small">{lang !== "en" ? "没有识别到分支/循环结构。" : "No branches/loops detected."}</p>;
  }
  return <div className="pv-flow-tree">{render(props.units, 0)}</div>;
}
