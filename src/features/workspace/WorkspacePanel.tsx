import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  ChevronRight, ChevronDown, Folder as FolderIcon, FileJson, MoreVertical,
  FolderPlus, RefreshCw, HardDrive, Pencil, Trash2, FolderInput, ExternalLink,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import type { WsEntry } from "../../lib/types";
import { parseMindmapFile } from "../../lib/mindmapFile";
import { pushToast, uiStore } from "../../state/uiStore";
import { openContextMenu, type MenuItem } from "../../components/ContextMenu";
import { askConfirm, askPrompt } from "../../components/Modal";

const norm = (p: string): string => p.replace(/\\/g, "/");

function parentOf(p: string): string {
  const n = norm(p);
  const i = n.lastIndexOf("/");
  return i <= 0 ? "" : n.slice(0, i);
}

const MINDMAP_FILE_RE = /\.(mindmap|json)$/i;
/** 12.2 `.project` 档案与 `.mindmap` 平行存在。 */
const PROJECT_FILE_RE = /\.project$/i;
/** .md / .txt / .markdown 归写作空间（文档上下文）。 */
const MD_FILE_RE = /\.(md|markdown|txt)$/i;
/** .fatetree 归命运推演空间。 */
const FATE_FILE_RE = /\.fatetree$/i;

interface Row extends WsEntry {
  depth: number;
}

/**
 * Built-in local workspace folder panel (spec II-2 / II-3):
 *  - live tree of the workspace directory (polled every 3 s + on focus),
 *  - pointer-driven drag of files into folders / root,
 *  - OS-level drag of `.mindmap` files INTO the panel (auto copy + load),
 *  - drag a file onto the canvas to spawn a sub-map anchor node.
 */
export function WorkspacePanel(): React.ReactElement {
  const { t, lang } = useI18n();
  const [root, setRoot] = useState("");
  const [entries, setEntries] = useState<WsEntry[]>([]);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [ghost, setGhost] = useState<{ name: string; x: number; y: number } | null>(null);
  /** "" means the tree root itself is hovered. */
  const [dropHover, setDropHover] = useState<string | null>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const entriesSig = useRef("");
  const rootRef = useRef(root);
  rootRef.current = root;
  const dragState = useRef<{ path: string; name: string; sx: number; sy: number; active: boolean } | null>(null);

  // ---------- listing & monitoring ----------
  const refresh = useCallback(async (rootPath: string): Promise<void> => {
    if (!rootPath) return;
    try {
      const list = await ipc.wsList(rootPath);
      const sig = JSON.stringify(list);
      if (sig !== entriesSig.current) {
        entriesSig.current = sig;
        setEntries(list);
      }
      setError("");
    } catch (e) {
      setError(errMessage(e).message);
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const raw = await ipc.getSettings();
        const dir = raw["workspaceDir"] || "";
        if (!cancelled && dir) {
          setRoot(dir);
          return;
        }
      } catch { /* fall through */ }
      try {
        const d = await ipc.wsDefaultDir();
        if (!cancelled) setRoot(d);
      } catch { /* panel shows error state */ }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!root) return;
    void refresh(root);
    const iv = window.setInterval(() => void refresh(root), 3000);
    const onFocus = (): void => void refresh(root);
    window.addEventListener("focus", onFocus);
    return () => {
      window.clearInterval(iv);
      window.removeEventListener("focus", onFocus);
    };
  }, [root, refresh]);

  // ---------- tree rows ----------
  const rows = useMemo<Row[]>(() => {
    const byParent = new Map<string, WsEntry[]>();
    for (const e of entries) {
      const k = parentOf(e.path);
      const arr = byParent.get(k);
      if (arr) arr.push(e);
      else byParent.set(k, [e]);
    }
    const out: Row[] = [];
    const walk = (parent: string, depth: number): void => {
      for (const e of byParent.get(parent) ?? []) {
        out.push({ ...e, depth });
        if (e.kind === "dir" && !collapsed.has(norm(e.path))) walk(norm(e.path), depth + 1);
      }
    };
    walk("", 0);
    return out;
  }, [entries, collapsed]);

  function toggleCollapsed(path: string): void {
    const key = norm(path);
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  // ---------- header actions ----------
  async function chooseRoot(): Promise<void> {
    try {
      const p = await openDialog({ directory: true, multiple: false });
      if (typeof p !== "string" || !p || norm(p) === norm(root)) return;
      await ipc.setSettings({ workspaceDir: p });
      entriesSig.current = "";
      setEntries([]);
      setCollapsed(new Set());
      setSelectedPath(null);
      setRoot(p);
    } catch (e) {
      pushToast("error", t("wsChooseRoot"), errMessage(e).message);
    }
  }

  async function newFolder(parentDir: string): Promise<void> {
    const v = await askPrompt({
      title: t("wsNewFolder"),
      initial: lang === "zh" ? "新文件夹" : "New folder",
      validate: (x) => (x.trim().length === 0 || /[\\/:*?"<>|]/.test(x))
        ? (lang === "zh" ? "名称无效" : "Invalid name")
        : null,
    });
    if (!v) return;
    try {
      await ipc.wsCreateDir(rootRef.current, parentDir, v.trim());
      await refresh(rootRef.current);
    } catch (e) {
      pushToast("error", t("wsNewFolder"), errMessage(e).message);
    }
  }

  async function renameEntry(entry: WsEntry): Promise<void> {
    const v = await askPrompt({ title: t("wsRename"), initial: entry.name });
    if (!v || v.trim() === entry.name) return;
    try {
      await ipc.wsRename(rootRef.current, entry.path, v.trim());
      await refresh(rootRef.current);
    } catch (e) {
      pushToast("error", t("wsRename"), errMessage(e).message);
    }
  }

  async function deleteEntry(entry: WsEntry): Promise<void> {
    const ok = await askConfirm({
      title: t("wsDelete"),
      body: lang === "zh" ? `「${entry.name}」将移入工作区回收目录。` : `"${entry.name}" will be moved to the workspace recycle folder.`,
      danger: true,
      okLabel: t("wsDelete"),
    });
    if (!ok) return;
    try {
      await ipc.wsDeleteTrash(rootRef.current, entry.path);
      await refresh(rootRef.current);
    } catch (e) {
      pushToast("error", t("wsDelete"), errMessage(e).message);
    }
  }

  // ---------- open / insert ----------
  /** 定位：展开文件所在目录链、选中并滚动到该行（"打开时定位到所属项目"）。 */
  function locateInTree(path: string): void {
    const target = norm(path);
    // expand every ancestor folder on the way to the file
    setCollapsed((prev) => {
      const next = new Set(prev);
      const parts = target.split("/");
      for (let i = 1; i < parts.length; i++) {
        const dir = parts.slice(0, i).join("/");
        next.delete(dir);
      }
      return next;
    });
    setSelectedPath(target);
    // wait for the re-render, then scroll the row into view
    window.setTimeout(() => {
      bodyRef.current
        ?.querySelector(`[data-ws-row="${CSS.escape(target)}"]`)
        ?.scrollIntoView({ block: "center" });
    }, 60);
  }

  function openAsMap(path: string): void {
    locateInTree(path);
    uiStore.setState({ mode: "mindmap" });
    window.setTimeout(() => {
      window.dispatchEvent(new CustomEvent("variable:mm-open-file", { detail: { path } }));
    }, 60);
  }

  /** .fatetree → 命运推演空间挂载后经 fatePendingOpen 完整载入（无 80ms 竞态）。 */
  function openFateArchive(path: string): void {
    locateInTree(path);
    uiStore.setState({ mode: "fate", fatePendingOpen: path });
  }

  /** .md/.txt → 写作空间挂载后经 writePendingOpen 载入为文档。 */
  function openAsDocument(path: string): void {
    locateInTree(path);
    uiStore.setState({ mode: "write", writePendingOpen: path });
  }

  /** 双击/菜单共用的默认打开方式：按扩展名路由到正确的项目空间。 */
  function openDefault(path: string, name: string): void {
    if (PROJECT_FILE_RE.test(name)) openProjectArchive(path);
    else if (FATE_FILE_RE.test(name)) openFateArchive(path);
    else if (MD_FILE_RE.test(name)) openAsDocument(path);
    else openAsMap(path);
  }

  /** 12.2 双击 .project 直接进入项目分析空间。 */
  function openProjectArchive(path: string): void {
    uiStore.setState({ mode: "project", pvPendingOpen: path });
  }

  async function insertIntoCanvas(entry: WsEntry): Promise<void> {
    try {
      const text = await ipc.wsReadText(entry.path);
      const parsed = parseMindmapFile(text);
      if (!parsed) {
        pushToast("error", t("invalidFile"));
        return;
      }
      uiStore.setState({ mode: "mindmap" });
      window.setTimeout(() => {
        window.dispatchEvent(new CustomEvent("variable:mm-import-nodes", { detail: parsed }));
      }, 60);
    } catch (e) {
      pushToast("error", t("invalidFile"), errMessage(e).message);
    }
  }

  function rowMenu(row: Row, x: number, y: number): void {
    if (row.kind === "file" && PROJECT_FILE_RE.test(row.name)) {
      openContextMenu(x, y, [
        { label: lang === "zh" ? "打开项目分析" : "Open in project analysis", icon: <FileJson size={14} />, onClick: () => openProjectArchive(row.path) },
        { label: t("wsReveal"), icon: <ExternalLink size={14} />, onClick: () => void ipc.revealPath(row.path).catch((e) => pushToast("error", t("wsReveal"), errMessage(e).message)) },
        { separator: true },
        { label: t("wsRename"), icon: <Pencil size={14} />, onClick: () => void renameEntry(row) },
        { label: t("wsDelete"), icon: <Trash2 size={14} />, danger: true, onClick: () => void deleteEntry(row) },
      ]);
      return;
    }
    const items: MenuItem[] =
      row.kind === "dir"
        ? [
            { label: t("wsNewFolder"), icon: <FolderPlus size={14} />, onClick: () => void newFolder(row.path) },
            { label: t("wsRename"), icon: <Pencil size={14} />, onClick: () => void renameEntry(row) },
            { label: t("wsReveal"), icon: <ExternalLink size={14} />, onClick: () => void ipc.revealPath(row.path).catch((e) => pushToast("error", t("wsReveal"), errMessage(e).message)) },
            { separator: true },
            { label: t("wsDelete"), icon: <Trash2 size={14} />, danger: true, onClick: () => void deleteEntry(row) },
          ]
        : [
            { label: t("wsOpen"), icon: <FileJson size={14} />, onClick: () => openDefault(row.path, row.name) },
            { label: t("wsOpenAsMap"), icon: <FileJson size={14} />, onClick: () => openAsMap(row.path) },
            { label: t("wsInsertCanvas"), icon: <FolderInput size={14} />, onClick: () => void insertIntoCanvas(row) },
            { label: t("wsReveal"), icon: <ExternalLink size={14} />, onClick: () => void ipc.revealPath(row.path).catch((e) => pushToast("error", t("wsReveal"), errMessage(e).message)) },
            { separator: true },
            { label: t("wsRename"), icon: <Pencil size={14} />, onClick: () => void renameEntry(row) },
            { label: t("wsDelete"), icon: <Trash2 size={14} />, danger: true, onClick: () => void deleteEntry(row) },
          ];
    openContextMenu(x, y, items);
  }

  // ---------- pointer-driven drag (panel → folder/root/canvas) ----------
  useEffect(() => {
    const hitDest = (x: number, y: number): { kind: "dir"; path: string } | { kind: "root" } | { kind: "canvas" } | null => {
      const el = document.elementFromPoint(x, y) as HTMLElement | null;
      if (!el) return null;
      if (el.closest(".mm-canvas")) return { kind: "canvas" };
      const dirEl = el.closest("[data-ws-dir]") as HTMLElement | null;
      if (dirEl?.dataset.wsDir) return { kind: "dir", path: dirEl.dataset.wsDir };
      if (el.closest("[data-ws-rootdrop]")) return { kind: "root" };
      return null;
    };

    const cancelDrag = (): void => {
      if (!dragState.current) return;
      dragState.current = null;
      setGhost(null);
      setDropHover(null);
    };

    const onMove = (e: MouseEvent): void => {
      const ds = dragState.current;
      if (!ds) return;
      // Safety net for a mouseup that happened outside the window (never
      // delivered): the moment the left button is no longer held, the drag is
      // dead — otherwise the ghost would resurrect on the next mousemove and
      // a later click could commit an unintended move.
      if ((e.buttons & 1) === 0) {
        cancelDrag();
        return;
      }
      if (!ds.active) {
        if (Math.hypot(e.clientX - ds.sx, e.clientY - ds.sy) < 5) return;
        ds.active = true;
      }
      setGhost({ name: ds.name, x: e.clientX, y: e.clientY });
      const dest = hitDest(e.clientX, e.clientY);
      if (dest?.kind === "dir") setDropHover(norm(dest.path));
      else if (dest?.kind === "root") setDropHover("");
      else setDropHover(null);
    };

    const onUp = (e: MouseEvent): void => {
      const ds = dragState.current;
      dragState.current = null;
      setGhost(null);
      setDropHover(null);
      if (!ds || !ds.active) return;

      const dest = hitDest(e.clientX, e.clientY);
      if (!dest) return;
      if (dest.kind === "canvas") {
        // Drop onto canvas → sub-mindmap anchor node at the drop point.
        window.dispatchEvent(new CustomEvent("variable:mm-import-anchor", {
          detail: { path: ds.path, clientX: e.clientX, clientY: e.clientY },
        }));
        return;
      }
      const destDir = dest.kind === "root" ? rootRef.current : dest.path;
      if (norm(destDir) === norm(parentOf(ds.path))) return; // already there
      void (async () => {
        try {
          await ipc.wsMove(rootRef.current, ds.path, destDir);
          await refresh(rootRef.current);
        } catch (err) {
          pushToast("error", lang === "zh" ? "移动失败" : "Move failed", errMessage(err).message);
        }
      })();
    };

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    window.addEventListener("blur", cancelDrag);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("blur", cancelDrag);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refresh]);

  function onRowMouseDown(e: React.MouseEvent, row: Row): void {
    if (e.button !== 0 || row.kind !== "file") return;
    dragState.current = { path: row.path, name: row.name, sx: e.clientX, sy: e.clientY, active: false };
  }

  // ---------- external OS files dragged INTO the panel ----------
  useEffect(() => {
    let disposed = false;
    let un: (() => void) | undefined;
    const p = getCurrentWebview().onDragDropEvent(async (event) => {
      try {
        if (event.payload.type !== "drop") return;
        const panel = panelRef.current;
        if (!panel || !rootRef.current) return;
        const scale = await getCurrentWindow().scaleFactor().catch(() => 1);
        const x = event.payload.position.x / scale;
        const y = event.payload.position.y / scale;
        const r = panel.getBoundingClientRect();
        if (x < r.left || x > r.right || y < r.top || y > r.bottom) return;
        const paths = [...(event.payload.paths ?? [])];
        const files = paths.filter((f) => MINDMAP_FILE_RE.test(f));
        if (files.length === 0) {
          pushToast("info", t("wsOnlyMindmap"));
          return;
        }
        const copied = await ipc.wsCopyIn(rootRef.current, files, rootRef.current);
        await refresh(rootRef.current);
        if (copied.length > 0) {
          pushToast("success", lang === "zh" ? `已收入工作区（${copied.length}）` : `Imported into workspace (${copied.length})`);
          const first = copied[0];
          if (copied.length === 1 && first) {
            uiStore.setState({ mode: "mindmap" });
            window.setTimeout(() => {
              window.dispatchEvent(new CustomEvent("variable:mm-open-file", { detail: { path: first } }));
            }, 60);
          }
        }
      } catch (e) {
        pushToast("error", t("wsCopiedIn"), errMessage(e).message);
      }
    });
    void p.then((u2) => {
      if (disposed) u2();
      else un = u2;
    }).catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refresh]);

  // ---------- render ----------
  return (
    <div className="ws-panel" ref={panelRef}>
      <div className="side-section-head">
        <span className="ws-title"><HardDrive size={12} /> {t("wsTitle")}</span>
        <span className="flex-1" />
        <button type="button" className="icon-btn tiny" data-tip={t("wsChooseRoot")} aria-label={t("wsChooseRoot")} onClick={() => void chooseRoot()}>
          <FolderInput size={13} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={t("wsNewFolder")} aria-label={t("wsNewFolder")} onClick={() => void newFolder(root)}>
          <FolderPlus size={13} />
        </button>
        <button type="button" className="icon-btn tiny" data-tip={t("wsRefresh")} aria-label={t("wsRefresh")} onClick={() => void refresh(root)}>
          <RefreshCw size={13} />
        </button>
      </div>

      {!root ? (
        <p className="dim small pad8">…</p>
      ) : (
        <>
          <div className="ws-path ellipsis" title={root}>{root}</div>
          <div
            ref={bodyRef}
            className={`ws-tree ${dropHover === "" ? "drop-root" : ""}`}
            data-ws-rootdrop=""
          >
            {rows.map((row) => (
              <div
                key={row.path}
                className={[
                  "ws-row",
                  row.kind === "dir" ? "dir" : "file",
                  selectedPath === row.path ? "current" : "",
                  dropHover === norm(row.path) ? "drop-hover" : "",
                ].filter(Boolean).join(" ")}
                style={{ paddingLeft: 6 + row.depth * 14 }}
                data-ws-dir={row.kind === "dir" ? row.path : undefined}
                data-ws-row={norm(row.path)}
                onMouseDown={(e) => onRowMouseDown(e, row)}
                onClick={() => {
                  setSelectedPath(row.path);
                  if (row.kind === "dir") toggleCollapsed(row.path);
                }}
                onDoubleClick={() => {
                  if (row.kind !== "file") return;
                  openDefault(row.path, row.name);
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  setSelectedPath(row.path);
                  rowMenu(row, e.clientX, e.clientY);
                }}
              >
                {row.kind === "dir" ? (
                  <span className="twisty">{collapsed.has(norm(row.path)) ? <ChevronRight size={12} /> : <ChevronDown size={12} />}</span>
                ) : (
                  <span className="twisty" />
                )}
                {row.kind === "dir" ? <FolderIcon size={13} className="dim" /> : <FileJson size={13} className="dim" />}
                <span className="ellipsis ws-name">{row.name}</span>
                <button
                  type="button"
                  className="icon-btn tiny row-actions"
                  aria-label={`${row.name} menu`}
                  onClick={(e) => {
                    e.stopPropagation();
                    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                    rowMenu(row, r.left, r.bottom + 4);
                  }}
                >
                  <MoreVertical size={12} />
                </button>
              </div>
            ))}
            {rows.length === 0 && (
              <p className="dim small pad8">{error || (lang === "zh" ? "（空）拖入 .mindmap / .project 文件即可收入。" : "(empty) Drop .mindmap / .project files here.")}</p>
            )}
          </div>
        </>
      )}

      {ghost && (
        <div className="ws-drag-ghost" style={{ left: ghost.x + 12, top: ghost.y + 10 }}>
          <FileJson size={12} />
          <span className="ellipsis">{ghost.name}</span>
        </div>
      )}
    </div>
  );
}
