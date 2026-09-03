import { useCallback, useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import {
  ChevronRight, ChevronDown, Folder as FolderIcon, FolderPlus, Star, Trash2,
  RotateCcw, XCircle, MoreVertical, FileText, Map as MapIcon, Pencil, Download, FolderInput,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import type { DocumentMeta, Folder } from "../../lib/types";
import { validateItemName } from "../../lib/pathGuard";
import { bumpDocList, bumpMapList, pushToast, uiStore, useUi } from "../../state/uiStore";
import { openContextMenu, type MenuItem } from "../../components/ContextMenu";
import { askConfirm, askPrompt } from "../../components/Modal";
import { WorkspacePanel } from "../workspace/WorkspacePanel";

type Tab = "all" | "favorites" | "trash" | "workspace";

/** Last mouse position for submenu anchoring. */
const lastMouse = { x: 200, y: 200 };
if (typeof window !== "undefined") {
  window.addEventListener("mousemove", (e) => {
    lastMouse.x = e.clientX;
    lastMouse.y = e.clientY;
  }, { passive: true });
}

interface FlatFolder extends Folder {
  depth: number;
}

export function Sidebar(): React.ReactElement | null {
  const { t, lang } = useI18n();
  const open = useUi((s) => s.sidebarOpen);
  const docListVersion = useUi((s) => s.docListVersion);
  const mapListVersion = useUi((s) => s.mapListVersion);
  const currentDocId = useUi((s) => s.currentDocId);
  const currentMapId = useUi((s) => s.currentMapId);

  const [tab, setTab] = useState<Tab>("all");
  const [folders, setFolders] = useState<Folder[]>([]);
  const [docs, setDocs] = useState<DocumentMeta[]>([]);
  const [maps, setMaps] = useState<{ id: string; name: string; updatedAt: number }[]>([]);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [sort, setSort] = useState<"updated" | "created">("updated");
  const [tagFilter, setTagFilter] = useState("");
  const [allTags, setAllTags] = useState<string[]>([]);

  const refreshFolders = useCallback(async () => {
    try {
      setFolders(await ipc.listFolders());
    } catch (e) {
      pushToast("error", "Load folders failed", errMessage(e).message);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    void refreshFolders();
  }, [open, docListVersion, refreshFolders]);

  useEffect(() => {
    if (!open || tab === "trash" || tab === "workspace") return;
    void ipc
      .listDocuments({ view: tab, sort, tag: tagFilter || undefined })
      .then(setDocs)
      .catch((e) => pushToast("error", "Load documents failed", errMessage(e).message));
  }, [open, tab, sort, tagFilter, docListVersion]);

  useEffect(() => {
    if (!open || tab !== "trash") return;
    void ipc
      .listDocuments({ view: "trash" })
      .then(setDocs)
      .catch(() => setDocs([]));
  }, [open, tab, docListVersion]);

  useEffect(() => {
    void ipc.listAllTags().then(setAllTags).catch(() => {});
  }, [docListVersion]);

  useEffect(() => {
    if (!open) return;
    void ipc.listMindmaps().then(setMaps).catch(() => {});
  }, [open, mapListVersion]);

  const flatFolders = useMemo<FlatFolder[]>(() => {
    const byParent = new Map<string | null, Folder[]>();
    for (const f of folders) {
      const key = f.parentId ?? null;
      byParent.get(key)?.push(f) ?? byParent.set(key, [f]);
    }
    const out: FlatFolder[] = [];
    const walk = (parent: string | null, depth: number) => {
      for (const f of (byParent.get(parent) ?? []).sort((a, b) => a.sortOrder - b.sortOrder)) {
        out.push({ ...f, depth });
        if (!collapsed.has(f.id)) walk(f.id, depth + 1);
      }
    };
    walk(null, 0);
    return out;
  }, [folders, collapsed]);

  async function newFolder(parentId: string | null): Promise<void> {
    const v = await askPrompt({
      title: t("newFolder"),
      initial: lang === "zh" ? "新文件夹" : "New folder",
      validate: (v2) => {
        const c = validateItemName(v2);
        return c.ok ? null : c.reason === "empty"
          ? lang === "zh" ? "名称不能为空" : "Name cannot be empty"
          : lang === "zh" ? "名称过长或含非法字符" : "Too long or invalid characters";
      },
    });
    if (!v) return;
    try {
      await ipc.createFolder(v.trim(), parentId);
      await refreshFolders();
      bumpDocList();
    } catch (e) {
      pushToast("error", t("newFolder"), errMessage(e).message);
    }
  }

  function folderMenu(f: FlatFolder): MenuItem[] {
    return [
      { label: t("newSubfolder"), icon: <FolderPlus size={14} />, onClick: () => void newFolder(f.id) },
      {
        label: t("rename"),
        icon: <Pencil size={14} />,
        onClick: () =>
          void askPrompt({
            title: t("rename"),
            initial: f.name,
            validate: (v) => {
              const c = validateItemName(v);
              return c.ok ? null : lang === "zh" ? "名称无效" : "Invalid name";
            },
          }).then(async (v) => {
            if (!v) return;
            try {
              await ipc.renameFolder(f.id, v.trim());
              await refreshFolders();
              bumpDocList();
            } catch (e) {
              pushToast("error", t("rename"), errMessage(e).message);
            }
          }),
      },
      {
        label: t("exportFolder"),
        icon: <Download size={14} />,
        onClick: () =>
          void save({
            defaultPath: `${f.name}.json`,
            filters: [{ name: "JSON", extensions: ["json"] }],
          }).then(async (p) => {
            if (typeof p !== "string") return;
            // Export folder content as a workspace-style JSON of its docs.
            const docsIn = await ipc.listDocuments({ folderId: f.id });
            const path = p;
            try {
              const result = await ipc.exportDocuments(
                docsIn.map((d) => d.id),
                "json",
                path,
              );
              pushToast("success", t("exportedOk"), `${result.count} → ${path}`);
            } catch (e) {
              pushToast("error", t("export"), errMessage(e).message);
            }
          }),
      },
      { separator: true },
      {
        label: t("deleteToTrash"),
        icon: <Trash2 size={14} />,
        danger: true,
        onClick: () =>
          void askConfirm({
            title: t("deleteToTrash"),
            body: t("deleteFolderConfirm", { name: f.name }),
            danger: true,
            okLabel: t("delete"),
          }).then(async (ok) => {
            if (!ok) return;
            try {
              await ipc.trashFolder(f.id);
              await refreshFolders();
              bumpDocList();
              bumpMapList();
            } catch (e) {
              pushToast("error", t("deleteToTrash"), errMessage(e).message);
            }
          }),
      },
    ];
  }

  async function purgeItems(ids: string[], kindLabel: string): Promise<void> {
    const ok = await askConfirm({
      title: t("purgeItem"),
      body: t("permanentDeleteConfirm", { n: ids.length }),
      danger: true,
    });
    if (!ok) return;
    try {
      await ipc.purgeDocuments(ids);
      bumpDocList();
      bumpMapList();
      pushToast("info", kindLabel, `${ids.length}`);
    } catch (e) {
      pushToast("error", t("purgeItem"), errMessage(e).message);
    }
  }

  function docMenu(d: DocumentMeta, trashed: boolean): MenuItem[] {
    if (trashed) {
      return [
        { label: t("restoreItem"), icon: <RotateCcw size={14} />, onClick: () => void ipc.restoreDocument(d.id).then(bumpDocList).catch((e) => pushToast("error", "Restore failed", errMessage(e).message)) },
        { label: t("purgeItem"), icon: <XCircle size={14} />, danger: true, onClick: () => void purgeItems([d.id], d.title) },
      ];
    }
    return [
      { label: lang === "zh" ? "打开" : "Open", icon: <FileText size={14} />, onClick: () => uiStore.setState({ currentDocId: d.id, mode: "write" }) },
      {
        label: d.favorite ? t("unfavorite") : t("favorite"),
        icon: <Star size={14} />,
        onClick: () => void ipc.setFavorite(d.id, !d.favorite).then(bumpDocList).catch(() => {}),
      },
      {
        label: t("createNodeFromDoc"),
        icon: <MapIcon size={14} />,
        onClick: () => window.dispatchEvent(new CustomEvent("variable:create-node-from-doc", { detail: { docId: d.id, title: d.title || (lang === "zh" ? "无标题" : "Untitled") } })),
      },
      {
        label: t("exportDoc"),
        icon: <Download size={14} />,
        onClick: () =>
          void save({
            defaultPath: `${(d.title || "untitled").replace(/[\\/:*?"<>|]/g, "_").slice(0, 60)}.md`,
            filters: [{ name: "Markdown", extensions: ["md"] }],
          }).then(async (p) => {
            if (typeof p !== "string") return;
            await ipc.exportDocuments([d.id], "md", p).then(() => pushToast("success", t("exportedOk"), p)).catch((e) => pushToast("error", t("export"), errMessage(e).message));
          }),
      },
      { separator: true },
      {
        label: t("moveToFolder"),
        icon: <FolderInput size={14} />,
        onClick: () => {
          void ipc.listFolders().then((foldersAll) => {
            openContextMenu(lastMouse.x, lastMouse.y, [
              { label: t("noFolder"), onClick: () => void ipc.moveDocument(d.id, null).then(bumpDocList).catch(() => {}) },
              ...(foldersAll.length > 0 ? ([{ separator: true }] as MenuItem[]) : []),
              ...foldersAll.map((f) => ({
                label: f.name,
                onClick: () => void ipc.moveDocument(d.id, f.id).then(bumpDocList).catch(() => {}),
              })),
            ]);
          });
        },
      },
      { separator: true },
      {
        label: t("deleteToTrash"),
        icon: <Trash2 size={14} />,
        danger: true,
        onClick: () =>
          void askConfirm({
            title: t("deleteToTrash"),
            body: `「${d.title || (lang === "zh" ? "无标题" : "Untitled")}」→ ${t("trashView")}？`,
            danger: true,
            okLabel: t("delete"),
          }).then(async (ok) => {
            if (!ok) return;
            await ipc.trashDocument(d.id).catch((e) => pushToast("error", "Trash failed", errMessage(e).message));
            if (uiStore.getState().currentDocId === d.id) uiStore.setState({ currentDocId: null });
            bumpDocList();
          }),
      },
    ];
  }

  if (!open) return null;

  return (
    <aside className="sidebar">
      <div className="side-tabs" role="tablist">
        <button type="button" role="tab" aria-selected={tab === "all"} className={tab === "all" ? "on" : ""} onClick={() => setTab("all")}>{t("allRecords")}</button>
        <button type="button" role="tab" aria-selected={tab === "favorites"} className={tab === "favorites" ? "on" : ""} onClick={() => setTab("favorites")}>{t("favorites")}</button>
        <button type="button" role="tab" aria-selected={tab === "trash"} className={tab === "trash" ? "on" : ""} onClick={() => setTab("trash")}>{t("trashView")}</button>
        <button type="button" role="tab" aria-selected={tab === "workspace"} className={tab === "workspace" ? "on" : ""} onClick={() => setTab("workspace")}>{t("wsTab")}</button>
      </div>

      {tab === "workspace" && <WorkspacePanel />}

      {tab !== "trash" && tab !== "workspace" && (
        <>
          <div className="side-section-head">
            <span>{t("folders")}</span>
            <span className="flex-1" />
            <button type="button" className="icon-btn tiny" data-tip={t("newFolder")} aria-label={t("newFolder")} onClick={() => void newFolder(null)}>
              <FolderPlus size={14} />
            </button>
          </div>
          <div className="folder-tree">
            {flatFolders.map((f) => (
              <div
                key={f.id}
                className={`folder-row depth-${f.depth}`}
                onClick={(e) => {
                  if ((e.target as HTMLElement).closest(".row-actions")) return;
                  setCollapsed((prev) => {
                    const next = new Set(prev);
                    if (next.has(f.id)) next.delete(f.id);
                    else next.add(f.id);
                    return next;
                  });
                }}
              >
                <button
                  type="button"
                  className="twisty"
                  aria-label={collapsed.has(f.id) ? "expand" : "collapse"}
                  onClick={(e) => {
                    e.stopPropagation();
                    setCollapsed((prev) => {
                      const next = new Set(prev);
                      if (next.has(f.id)) next.delete(f.id);
                      else next.add(f.id);
                      return next;
                    });
                  }}
                >
                  {collapsed.has(f.id) ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
                </button>
                <FolderIcon size={14} className="dim" />
                <span className="ellipsis">{f.name}</span>
                <span className="flex-1" />
                <button
                  type="button"
                  className="icon-btn tiny row-actions"
                  aria-label={`${f.name} menu`}
                  onClick={(e) => {
                    e.stopPropagation();
                    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                    openContextMenu(r.left, r.bottom + 4, folderMenu(f));
                  }}
                >
                  <MoreVertical size={13} />
                </button>
              </div>
            ))}
            {flatFolders.length === 0 && <p className="dim small pad8">—</p>}
          </div>
        </>
      )}

      {tab === "trash" && (
        <div className="side-section-head trash-head">
          <span>{t("deletedItems")}</span>
          <span className="flex-1" />
          <button
            type="button"
            className="btn tiny ghost"
            onClick={() =>
              void askConfirm({ title: t("emptyTrash"), body: t("emptyTrashConfirm"), danger: true }).then(async (ok) => {
                if (!ok) return;
                try {
                  const n = await ipc.emptyTrash();
                  bumpDocList();
                  pushToast("info", t("emptyTrash"), String(n));
                } catch (e) {
                  pushToast("error", t("emptyTrash"), errMessage(e).message);
                }
              })
            }
          >
            {t("emptyTrash")}
          </button>
        </div>
      )}

      {tab !== "trash" && tab !== "workspace" && (
        <div className="filter-row">
          <select value={sort} onChange={(e) => setSort(e.target.value as "updated" | "created")} aria-label={t("sortByUpdated")}>
            <option value="updated">{t("sortByUpdated")}</option>
            <option value="created">{t("sortByCreated")}</option>
          </select>
          <select value={tagFilter} onChange={(e) => setTagFilter(e.target.value)} aria-label={t("filterTag")}>
            <option value="">{t("filterTag")}…</option>
            {allTags.map((tg) => (
              <option key={tg} value={tg}>#{tg}</option>
            ))}
          </select>
        </div>
      )}

      {tab !== "workspace" && (
        <div className="doc-list" role="list">
          {docs.map((d) => (
          <div
            key={d.id}
            role="listitem"
            className={`doc-row ${currentDocId === d.id ? "current" : ""}`}
            onClick={() => {
              if (tab === "trash") return;
              uiStore.setState({ currentDocId: d.id, mode: "write" });
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              openContextMenu(e.clientX, e.clientY, docMenu(d, tab === "trash"));
            }}
          >
            {d.favorite && <Star size={12} className="star" fill="currentColor" />}
            <FileText size={13} className="dim" />
            <span className="ellipsis doc-title">{d.title || (lang === "zh" ? "无标题" : "Untitled")}</span>
            <time className="small dim">{fmtDate(d.updatedAt)}</time>
            <button
              type="button"
              className="icon-btn tiny row-actions"
              aria-label="menu"
              onClick={(e) => {
                e.stopPropagation();
                const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                openContextMenu(r.left - 40, r.bottom + 4, docMenu(d, tab === "trash"));
              }}
            >
              <MoreVertical size={13} />
            </button>
          </div>
        ))}
        {docs.length === 0 && <p className="dim small pad8">{lang === "zh" ? "（空）" : "(empty)"}</p>}
        </div>
      )}

      {tab !== "trash" && tab !== "workspace" && (
        <>
        <div className="side-section-head">
          <span><MapIcon size={13} /> {lang === "zh" ? "思维导图" : "Mind maps"}</span>
          <span className="flex-1" />
          <button
            type="button"
            className="icon-btn tiny"
            data-tip={lang === "zh" ? "新建导图" : "New map"}
            aria-label={lang === "zh" ? "新建导图" : "New map"}
            onClick={async () => {
              const v = await askPrompt({ title: lang === "zh" ? "新建导图" : "New mind map", initial: t("untitledMap"), validate: (x) => (validateItemName(x).ok ? null : "×") });
              if (!v) return;
              const m = await ipc.createMindmap(v.trim()).catch((e) => {
                pushToast("error", "Create map failed", errMessage(e).message);
                return null;
              });
              if (m) {
                bumpMapList();
                uiStore.setState({ currentMapId: m.id, mode: "mindmap" });
              }
            }}
          >
            +
          </button>
        </div>
        <div className="map-list">
            {maps.map((m) => (
              <div key={m.id} className={`map-row ${currentMapId === m.id ? "current" : ""}`}>
                <button
                  type="button"
                  className="map-open"
                  onClick={() => uiStore.setState({ currentMapId: m.id, mode: "mindmap" })}
                >
                  <MapIcon size={13} />
                  <span className="ellipsis">{m.name}</span>
                </button>
                <button
                  type="button"
                  className="icon-btn tiny row-actions"
                  aria-label="menu"
                  onClick={async (e) => {
                    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                    openContextMenu(r.left - 120, r.bottom + 4, [
                      {
                        label: t("rename"),
                        icon: <Pencil size={13} />,
                        onClick: () =>
                          void askPrompt({ title: t("rename"), initial: m.name, validate: (v) => (validateItemName(v).ok ? null : "×") }).then(async (v) => {
                            if (!v) return;
                            await ipc.renameMindmap(m.id, v.trim()).then(bumpMapList).catch((er) => pushToast("error", t("rename"), errMessage(er).message));
                          }),
                      },
                      {
                        label: t("deleteToTrash"),
                        icon: <Trash2 size={13} />,
                        danger: true,
                        onClick: () =>
                          void askConfirm({ title: t("deleteToTrash"), body: m.name, danger: true }).then(async (ok) => {
                            if (!ok) return;
                            await ipc.trashMindmap(m.id).catch(() => {});
                            if (uiStore.getState().currentMapId === m.id) uiStore.setState({ currentMapId: null });
                            bumpMapList();
                          }),
                      },
                    ]);
                  }}
                >
                  <MoreVertical size={13} />
                </button>
              </div>
            ))}
        </div>
        </>
      )}
    </aside>
  );
}

function fmtDate(ts: number): string {
  const d = new Date(ts);
  return `${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}


