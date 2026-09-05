import { useEffect, useRef, useState } from "react";
import { Search, X, FileText, Folder as FolderIcon, GitBranch, Box } from "lucide-react";
import { useI18n } from "../../../i18n";
import { ipc, errMessage } from "../../../lib/ipc";
import type { SearchHit } from "../../../lib/types";
import { formatDateTime } from "../../../i18n";
import { pushToast, uiStore, useUi } from "../../../state/uiStore";
import { useUninstalledOfficial } from "../../../system/launcher/official";
import { matchPinyin, isAsciiQuery } from "../../../lib/pinyin";

/** Global search across documents, folders, maps and node text. */
export function SearchOverlay(): React.ReactElement | null {
  const { t, lang } = useI18n();
  const open = useUi((s) => s.searchOpen);
  const uninstalled = useUninstalledOfficial();
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [loading, setLoading] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setHits([]);
      setTimeout(() => inputRef.current?.focus(), 30);
    }
  }, [open]);

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
        <div className="search-results">
          {loading && <p className="dim pad8">…</p>}
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
        </div>
        <div className="search-foot dim small">{t("offlineNote")}</div>
      </div>
    </div>
  );
}
