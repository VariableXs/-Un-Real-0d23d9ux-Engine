import { useEffect, useRef, useState } from "react";
import { Search, X, FileText, Folder as FolderIcon, GitBranch, Box, File } from "lucide-react";
import { useI18n } from "../../../i18n";
import { ipc, errMessage } from "../../../lib/ipc";
import type { SearchHit } from "../../../lib/types";
import { formatDateTime } from "../../../i18n";
import { pushToast, uiStore, useUi } from "../../../state/uiStore";
import { useUninstalledOfficial } from "../../../system/launcher/official";
import { matchPinyin, isAsciiQuery } from "../../../lib/pinyin";
import { CloseLight } from "../../../components/CloseLight";
import { openVwmSystem } from "../../../system/windows/vwm";

/** F-4 文件命中（容器内索引）。 */
type FsTab = "content" | "files";

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(1)} ${units[i]}`;
}

/** Global search across documents, folders, maps, node text and container files. */
export function SearchOverlay(): React.ReactElement | null {
  const { t, lang } = useI18n();
  const open = useUi((s) => s.searchOpen);
  const uninstalled = useUninstalledOfficial();
  const [tab, setTab] = useState<FsTab>("content");
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  // F-4 文件页
  const [fsHits, setFsHits] = useState<import("../../../lib/ipc").Shell.FsHit[]>([]);
  const [fsCount, setFsCount] = useState<number | null>(null);
  const [fsExt, setFsExt] = useState("");
  const [fsKind, setFsKind] = useState<"all" | "file" | "dir">("all");
  const [fsMin, setFsMin] = useState<"any" | "kb" | "mb" | "gb">("any");
  const [fsBuilding, setFsBuilding] = useState(false);

  useEffect(() => {
    if (open) {
      setQuery("");
      setHits([]);
      setFsHits([]);
      setTimeout(() => inputRef.current?.focus(), 30);
    }
  }, [open]);

  // F-4 文件页查询（防抖 220ms；拼音回退：ASCII 查询取大集后按拼音/首字母过滤）
  useEffect(() => {
    if (!open || tab !== "files") return;
    if (!query.trim() && fsExt.trim() === "" && fsKind === "all" && fsMin === "any") {
      setFsHits([]);
      return;
    }
    setLoading(true);
    const timer = setTimeout(async () => {
      const minSize = fsMin === "kb" ? 1024 : fsMin === "mb" ? 1024 ** 2 : fsMin === "gb" ? 1024 ** 3 : 0;
      const limit = isAsciiQuery(query) ? 2000 : 200;
      try {
        let list = await ipc.fsIndexQuery({
          query: query.trim(),
          ext: fsExt,
          kind: fsKind,
          minSize,
          limit,
        });
        // 拼音/首字母回退（F-4.4）：对文件名做拼音匹配，优先展示拼音命中
        if (isAsciiQuery(query) && list.length > 0) {
          const py = list.filter((h) => matchPinyin(h.name, query.trim()));
          if (py.length > 0) list = [...py, ...list.filter((h) => !py.includes(h))];
        }
        setFsHits(list.slice(0, 100));
        try {
          const st = await ipc.fsIndexStatus();
          setFsCount(st.count);
          setFsBuilding(st.building);
        } catch {
          /* 状态展示失败不影响结果 */
        }
      } catch (e) {
        pushToast("error", t("searchTabFiles"), errMessage(e).message);
        setFsHits([]);
      } finally {
        setLoading(false);
      }
    }, 220);
    return () => clearTimeout(timer);
  }, [query, open, tab, fsExt, fsKind, fsMin, t]);

  /** 双击/点击文件命中 → 必在 VWM 文件管理器打开所在目录（绝不落 Windows 桌面）。 */
  function openFsHit(h: import("../../../lib/ipc").Shell.FsHit): void {
    const dir = h.path.replace(/[\\/][^\\/]+$/, "") || h.path;
    uiStore.setState({ searchOpen: false });
    openVwmSystem("explorer", dir);
  }

  useEffect(() => {
    if (!open) return;
    if (!query.trim()) {
      setHits([]);
      return;
    }
    setLoading(true);
    const timer = setTimeout(async () => {
      try {
        let found = await ipc.searchAll(query.trim());
        // 批次E-8（规格 N5）：拼音/首字母回退 —— 纯字母查询时对记录/导图标题做拼音匹配
        if (isAsciiQuery(query)) {
          const [docs, maps] = await Promise.all([
            ipc.listDocuments({ view: "all" }).catch(() => []),
            ipc.listMindmaps().catch(() => []),
          ]);
          const extra: SearchHit[] = [
            ...docs
              .filter((d) => matchPinyin(d.title, query))
              .map((d) => ({ kind: "document" as const, id: d.id, parentId: null, title: d.title, snippet: "", updatedAt: d.updatedAt })),
            ...maps
              .filter((m) => matchPinyin(m.name, query))
              .map((m) => ({ kind: "mindmap" as const, id: m.id, parentId: null, title: m.name, snippet: "", updatedAt: m.updatedAt })),
          ];
          const seen = new Set(found.map((h) => `${h.kind}-${h.id}`));
          found = [...found, ...extra.filter((h) => !seen.has(`${h.kind}-${h.id}`))];
        }
        setHits(found);
      } catch (e) {
        pushToast("error", lang !== "en" ? "搜索失败" : "Search failed", errMessage(e).message);
        setHits([]);
      } finally {
        setLoading(false);
      }
    }, 220);
    return () => clearTimeout(timer);
  }, [query, open, lang]);

  if (!open) return null;

  function activate(hit: SearchHit): void {
    // 批次C（规格 5.6.1）：已卸载软件的命中不激活（入口已隐藏，如实无操作）
    const target: "write" | "mindmap" | null =
      hit.kind === "document" || hit.kind === "folder"
        ? "write"
        : hit.kind === "mindmap" || hit.kind === "node"
          ? "mindmap"
          : null;
    if (target && uninstalled[target] !== undefined) return;
    switch (hit.kind) {
      case "document":
        uiStore.setState({ currentDocId: hit.id, mode: "write", searchOpen: false });
        break;
      case "mindmap":
        uiStore.setState({ currentMapId: hit.id, mode: "mindmap", searchOpen: false });
        break;
      case "folder":
        // 文件夹属于 Write 软件的概念：打开 Write 并展开侧栏（桌面模式同样适用）。
        uiStore.setState({ sidebarOpen: true, mode: "write", searchOpen: false });
        break;
      case "node": {
        if (hit.parentId) {
          uiStore.setState({ currentMapId: hit.parentId, mode: "mindmap", searchOpen: false });
          window.dispatchEvent(
            new CustomEvent("variable:mm-focus-node", { detail: hit.id }),
          );
        }
        break;
      }
    }
  }

  const kindIcon = (k: SearchHit["kind"]) =>
    k === "document" ? <FileText size={14} /> : k === "folder" ? <FolderIcon size={14} /> : k === "mindmap" ? <GitBranch size={14} /> : <Box size={14} />;

  // 批次C：已卸载软件的内容不参与搜索展示（文档/文件夹 → Write，导图/节点 → Mind）
  const visibleHits = hits.filter((h) => {
    if (h.kind === "document" || h.kind === "folder") return uninstalled.write === undefined;
    if (h.kind === "mindmap" || h.kind === "node") return uninstalled.mindmap === undefined;
    return true;
  });

  return (
    <div className="modal-overlay search-overlay" onMouseDown={(e) => e.target === e.currentTarget && uiStore.setState({ searchOpen: false })}>
      <div className="search-panel card-pop" role="dialog" aria-label={t("globalSearch")}>
        <div className="search-head">
          <CloseLight onClose={() => uiStore.setState({ searchOpen: false })} />
          <Search size={17} className="dim" />
          <input
            ref={inputRef}
            value={query}
            placeholder={t("searchAllContent")}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Escape") uiStore.setState({ searchOpen: false });
              if (e.key === "Enter" && visibleHits[0]) activate(visibleHits[0]);
            }}
          />
          <button type="button" className="icon-btn tiny" aria-label={t("close")} onClick={() => uiStore.setState({ searchOpen: false })}><X size={14} /></button>
        </div>
        {/* F-4：内容 / 文件 双页 */}
        <div className="search-tabs" role="tablist">
          <button type="button" role="tab" aria-selected={tab === "content"} className={`search-tab${tab === "content" ? " active" : ""}`} onClick={() => setTab("content")}>
            {t("searchTabContent")}
          </button>
          <button type="button" role="tab" aria-selected={tab === "files"} className={`search-tab${tab === "files" ? " active" : ""}`} onClick={() => setTab("files")}>
            {t("searchTabFiles")}
          </button>
        </div>
        {tab === "files" && (
          <div className="search-filters" role="group" aria-label={t("fsFilter")}>
            <input
              className="fs-ext"
              value={fsExt}
              onChange={(e) => setFsExt(e.target.value)}
              placeholder={t("fsExtPh")}
              aria-label={t("fsExtPh")}
            />
            <select value={fsKind} onChange={(e) => setFsKind(e.target.value as "all" | "file" | "dir")} aria-label={t("fsKind")}>
              <option value="all">{t("fsKindAll")}</option>
              <option value="file">{t("fsKindFile")}</option>
              <option value="dir">{t("fsKindDir")}</option>
            </select>
            <select value={fsMin} onChange={(e) => setFsMin(e.target.value as "any" | "kb" | "mb" | "gb")} aria-label={t("fsMinSize")}>
              <option value="any">{t("fsSizeAny")}</option>
              <option value="kb">≥ 1 KB</option>
              <option value="mb">≥ 1 MB</option>
              <option value="gb">≥ 1 GB</option>
            </select>
            <span className="dim small ellipsis">
              {fsBuilding ? t("fsIndexing") : fsCount !== null ? t("fsIndexed").replace("{n}", String(fsCount)) : ""}
            </span>
          </div>
        )}
        <div className="search-results">
          {loading && <p className="dim pad8">…</p>}
          {tab === "content" && (
            <>
              {!loading && query.trim() && visibleHits.length === 0 && (
                <p className="dim pad8">{t("noResults")}</p>
              )}
              {visibleHits.map((h) => (
                <button key={`${h.kind}-${h.id}`} type="button" className="search-hit" onClick={() => activate(h)}>
                  <span className="kind">{kindIcon(h.kind)}</span>
                  <span className="texts">
                    <span className="title ellipsis">{h.title}</span>
                    {h.snippet && <span className="snippet ellipsis">{h.snippet}</span>}
                  </span>
                  <span className="meta dim small">
                    {t(`kind${h.kind.charAt(0).toUpperCase()}${h.kind.slice(1)}`)} · {formatDateTime(h.updatedAt, lang)}
                  </span>
                </button>
              ))}
            </>
          )}
          {tab === "files" && (
            <>
              {!loading && fsHits.length === 0 && (
                <p className="dim pad8">{t("fsEmpty")}</p>
              )}
              {fsHits.map((h) => (
                <button key={h.path} type="button" className="search-hit" onClick={() => openFsHit(h)} title={h.path}>
                  <span className="kind">{h.isDir ? <FolderIcon size={14} /> : <File size={14} />}</span>
                  <span className="texts">
                    <span className="title ellipsis">{h.name}</span>
                    <span className="snippet ellipsis">{h.path}</span>
                  </span>
                  <span className="meta dim small">
                    {h.isDir ? t("fsKindDir") : fmtBytes(h.size)} · {formatDateTime(h.mtime, lang)}
                  </span>
                </button>
              ))}
            </>
          )}
        </div>
        <div className="search-foot dim small">{tab === "files" ? t("fsBoundaryNote") : t("offlineNote")}</div>
      </div>
    </div>
  );
}
