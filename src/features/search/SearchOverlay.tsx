import { useEffect, useRef, useState } from "react";
import { Search, X, FileText, Folder as FolderIcon, GitBranch, Box } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import type { SearchHit } from "../../lib/types";
import { formatDateTime } from "../../i18n";
import { pushToast, uiStore, useUi } from "../../state/uiStore";

/** Global search across documents, folders, maps and node text. */
export function SearchOverlay(): React.ReactElement | null {
  const { t, lang } = useI18n();
  const open = useUi((s) => s.searchOpen);
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
        setHits(await ipc.searchAll(query.trim()));
      } catch (e) {
        pushToast("error", lang === "zh" ? "搜索失败" : "Search failed", errMessage(e).message);
        setHits([]);
      } finally {
        setLoading(false);
      }
    }, 220);
    return () => clearTimeout(timer);
  }, [query, open, lang]);

  if (!open) return null;

  function activate(hit: SearchHit): void {
    switch (hit.kind) {
      case "document":
        uiStore.setState({ currentDocId: hit.id, mode: "write", searchOpen: false });
        break;
      case "mindmap":
        uiStore.setState({ currentMapId: hit.id, mode: "mindmap", searchOpen: false });
        break;
      case "folder":
        // Open the folder context by showing sidebar and closing overlay.
        uiStore.setState({ sidebarOpen: true, searchOpen: false });
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
              if (e.key === "Enter" && hits[0]) activate(hits[0]);
            }}
          />
          <button type="button" className="icon-btn tiny" aria-label={t("close")} onClick={() => uiStore.setState({ searchOpen: false })}><X size={14} /></button>
        </div>
        <div className="search-results">
          {loading && <p className="dim pad8">…</p>}
          {!loading && query.trim() && hits.length === 0 && (
            <p className="dim pad8">{t("noResults")}</p>
          )}
          {hits.map((h) => (
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
