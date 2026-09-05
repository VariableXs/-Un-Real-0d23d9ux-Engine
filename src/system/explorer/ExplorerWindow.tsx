﻿import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowLeft, ArrowRight, ArrowUp, File as FileIcon, Folder, FolderOpen, FolderPlus,
  HardDrive, Image as ImageIcon, LayoutGrid, List, RefreshCw, Search, Share2, Star, Trash2, X,
} from "lucide-react";
import { askConfirm, askPrompt, ConfirmHost, NetConsentHost, PromptHost } from "../../components/Modal";
import { ContextMenuHost, openContextMenu, type MenuItem } from "../../components/ContextMenu";
import { ToastHost } from "../../components/ToastHost";
import { WindowControls } from "../../components/WindowControls";
import { useI18n } from "../../i18n";
import { errMessage, ipc, type ExCopyMode, type ExEntry, type ExListing, type ExSearchResult, type ExVarDir } from "../../lib/ipc";
import { beginXDrag } from "../../lib/xflow";
import { pushToast } from "../../state/uiStore";
import { openExplorerWindow, trackSelfGeom } from "../windows/appWindows";
import { openVwmSystem } from "../windows/vwm";
import { RecycleView } from "../recycle/RecycleView";

/**
 * 批次C 系统窗口：文件管理器完整版（explorer.html，?view=recycle 时载入回收站）。
 * 规格 7.1-7.8：
 * - 标签页（Ctrl+T / Ctrl+Tab / Ctrl+W）+ 每标签页独立历史（Alt+←/→ 后退前进）
 * - 三种视图（列表/图标/缩略图，Ctrl+滚轮切换）；列头点击排序（升/降序）
 * - 搜索：本地即输即滤；通配符 *.jpg 与布尔 AND/OR/NOT → 后端递归搜索（深度 4、上限 500）
 * - 收藏夹持久化（ex_fav_*）；地址栏面包屑 + F4/Ctrl+L 编辑路径、粘贴路径直接跳转
 * - 剪贴板（Ctrl+C/X/V）+ 冲突对话框（替换/跳过/保留两者/全部应用）
 * - 行拖拽到文件夹 = 移动（同盘）/复制（跨盘），Ctrl 强制复制、Shift 强制移动
 * - 快捷键全集：F2/Delete/Shift+Delete/Enter/F5/Ctrl+Shift+N/Ctrl+N
 * - 缩略图异步加载（并发受限 + 会话内缓存），不阻塞列表；视频无解码器如实显示图标
 */

type SysView = "explorer" | "recycle";
type ExViewMode = "list" | "icons" | "thumbs";
type SortKey = "name" | "modified" | "created" | "type" | "size";

interface Tab {
  id: number;
  /** 历史栈（hi 指向当前）。 */
  history: string[];
  hi: number;
}

// ---------- 会话内剪贴板 / 缩略图缓存（每窗口实例独立） ----------

let exClipboard: { paths: string[]; cut: boolean } | null = null;

const thumbCache = new Map<string, string | null>();
const THUMB_CACHE_MAX = 400;
let thumbActive = 0;
const thumbQueue: (() => void)[] = [];

function scheduleThumb(fn: () => void): void {
  if (thumbActive < 4) {
    thumbActive++;
    fn();
  } else {
    thumbQueue.push(fn);
  }
}

function thumbDone(): void {
  thumbActive--;
  const next = thumbQueue.shift();
  if (next) {
    thumbActive++;
    next();
  }
}

function thumbKey(e: ExEntry): string {
  return `${e.path}:${e.updatedAt}`;
}

/** 冲突解决选择（规格 7.7）。-all 后缀 = 应用到全部。 */
type ConflictChoice = "replace" | "keep" | "skip" | "replace-all" | "keep-all" | "cancel";

interface ConflictState {
  name: string;
  multiple: boolean;
  resolve: (c: ConflictChoice) => void;
}

// ---------- helpers ----------

function fmtSize(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

/** 面包屑分段："C:\Users\v" → [{C: → C:\}, {Users → C:\Users\}, {v → C:\Users\v}] */
function crumbs(path: string): { label: string; path: string }[] {
  const parts = path.split("\\").filter(Boolean);
  const out: { label: string; path: string }[] = [];
  let acc = "";
  parts.forEach((p, i) => {
    acc = i === 0 ? `${p}\\` : `${acc}${p}${i < parts.length - 1 ? "\\" : ""}`;
    out.push({ label: p, path: acc });
  });
  return out;
}

function pathTail(p: string): string {
  const seg = p.split("\\").filter(Boolean);
  return seg[seg.length - 1] ?? p;
}

/** 通配符 / 布尔语法 → 走后端递归搜索；普通子串 → 本地即输即滤。 */
function isSyntaxQuery(q: string): boolean {
  return /[*?]/.test(q) || /(^|\s)(AND|OR|NOT)(\s|$)/i.test(q.trim());
}

const THUMB_EXTS = new Set(["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"]);
const VIDEO_EXTS = new Set(["mp4", "mkv", "avi", "mov", "webm", "wmv", "flv", "m4v"]);

// ---------- 外壳 ----------

export function ExplorerWindow(props: {
  initialView?: SysView;
  /** VWM 内嵌模式：标题栏/几何记忆/全局宿主由桌面壳层接管，只渲染视图本体。 */
  embedded?: boolean;
  /** 初始定位路径（内嵌模式传入；独立窗口走 ?path= URL 参数）。 */
  initialPath?: string;
}): React.ReactElement {
  const view: SysView = props.initialView ?? "explorer";

  // 窗口几何记忆（label 与 WebviewWindow label 一致；内嵌模式由 VWM 负责）
  useEffect(() => {
    if (props.embedded) return;
    const un = trackSelfGeom(view);
    return () => {
      void un.then((f) => f()).catch(() => {});
    };
  }, [view, props.embedded]);

  if (props.embedded) {
    return (
      <div className="ex-window ex-embedded" data-view={view}>
        {view === "recycle" ? <RecycleShell embedded /> : <ExplorerShell embedded initialPath={props.initialPath} />}
      </div>
    );
  }

  return (
    <div className="ex-window" data-view={view}>
      {view === "recycle" ? <RecycleShell /> : <ExplorerShell />}
    </div>
  );
}

/** 红灯 = 关闭自己（系统窗口无未保存数据）。 */
function ExTitlebar(props: { title: string }): React.ReactElement {
  const win = getCurrentWindow();
  return (
    <header className="ex-titlebar" data-tauri-drag-region>
      <span className="ex-title" data-tauri-drag-region>
        {props.title}
      </span>
      <WindowControls onCloseRequested={() => void win.close().catch(() => {})} />
    </header>
  );
}

/** 本窗口是独立入口，全局宿主需在此挂载。内嵌模式下桌面壳层已挂载同名宿主，跳过防重复。 */
function ExHosts(embedded = false): ReactNode {
  if (embedded) return null;
  return (
    <>
      <ToastHost />
      <ContextMenuHost />
      <ConfirmHost />
      <NetConsentHost />
      <PromptHost />
    </>
  );
}

// ---------- 回收站窗口 ----------

function RecycleShell(props?: { embedded?: boolean }): React.ReactElement {
  const { t } = useI18n();
  const embedded = props?.embedded ?? false;
  return (
    <>
      {!embedded && <ExTitlebar title={t("recycleBin")} />}
      <div className="ex-body">
        <RecycleView />
      </div>
      {ExHosts(embedded)}
    </>
  );
}

// ---------- 文件管理器窗口 ----------

let nextTabId = 1;

function ExplorerShell(props?: { embedded?: boolean; initialPath?: string }): React.ReactElement {
  const { t, lang } = useI18n();
  const embedded = props?.embedded ?? false;
  const bootTabId = useRef(nextTabId++);
  const [tabs, setTabs] = useState<Tab[]>(() => [{ id: bootTabId.current, history: [""], hi: 0 }]);
  const [activeId, setActiveId] = useState<number>(bootTabId.current);
  const [listing, setListing] = useState<ExListing | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [search, setSearch] = useState<ExSearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const [view, setView] = useState<ExViewMode>("list");
  const [sort, setSort] = useState<{ key: SortKey; dir: 1 | -1 }>({ key: "name", dir: 1 });
  const [favs, setFavs] = useState<string[]>([]);
  const [home, setHome] = useState<string | null>(null);
  const [drives, setDrives] = useState<{ letter: string; path: string }[]>([]);
  // 批次E（规格 7.2）：Variable 数据目录节点组
  const [varDirs, setVarDirs] = useState<ExVarDir[]>([]);
  const [addrEdit, setAddrEdit] = useState<string | null>(null);
  const [conflict, setConflict] = useState<ConflictState | null>(null);
  const [clipboardTick, setClipboardTick] = useState(0); // 触发剪贴板高亮重渲染
  const alive = useRef(true);
  const listRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const addrRef = useRef<HTMLInputElement>(null);
  const clipRef = useRef(exClipboard);

  const activeTab = tabs.find((tb) => tb.id === activeId) ?? tabs[0];
  const currentPath = activeTab ? (activeTab.history[activeTab.hi] ?? "") : "";
  const pathRef = useRef(currentPath);
  pathRef.current = currentPath;
  const searchActiveRef = useRef(false);
  searchActiveRef.current = search !== null || searching;

  const openConflict = useCallback((name: string, multiple: boolean): Promise<ConflictChoice> => {
    return new Promise((resolve) => setConflict({ name, multiple, resolve }));
  }, []);

  const loadPath = useCallback(
    async (p: string): Promise<void> => {
      try {
        const l = await ipc.exList(p);
        if (!alive.current) return;
        setListing(l);
        setLoadErr(null);
        setSelected(null);
      } catch (e) {
        if (!alive.current) return;
        setListing(null);
        setLoadErr(errMessage(e).message);
        pushToast("error", t("explorerWin"), errMessage(e).message);
      }
    },
    [t],
  );

  /** 导航（压历史栈 + 清搜索/过滤/选中）。 */
  const nav = useCallback(
    (p: string): void => {
      setTabs((tbs) =>
        tbs.map((tb) =>
          tb.id === activeId
            ? { ...tb, history: [...tb.history.slice(0, tb.hi + 1), p], hi: tb.hi + 1 }
            : tb,
        ),
      );
      setFilter("");
      setSearch(null);
      setAddrEdit(null);
      void loadPath(p);
    },
    [activeId, loadPath],
  );

  const goBack = useCallback((): void => {
    const tb = tabs.find((x) => x.id === activeId);
    if (!tb || tb.hi === 0) return;
    const p = tb.history[tb.hi - 1];
    if (p === undefined) return;
    setTabs((tbs) => tbs.map((x) => (x.id === tb.id ? { ...x, hi: tb.hi - 1 } : x)));
    void loadPath(p);
  }, [tabs, activeId, loadPath]);

  const goForward = useCallback((): void => {
    const tb = tabs.find((x) => x.id === activeId);
    if (!tb || tb.hi >= tb.history.length - 1) return;
    const p = tb.history[tb.hi + 1];
    if (p === undefined) return;
    setTabs((tbs) => tbs.map((x) => (x.id === tb.id ? { ...x, hi: tb.hi + 1 } : x)));
    void loadPath(p);
  }, [tabs, activeId, loadPath]);

  const goUp = useCallback((): void => {
    if (listing?.parent) nav(listing.parent);
  }, [listing, nav]);

  /** 静默刷新（轮询/写操作后）：保留选中。 */
  const refresh = useCallback(async (): Promise<void> => {
    const p = pathRef.current;
    if (!p) return;
    try {
      const l = await ipc.exList(p);
      if (!alive.current) return;
      setListing(l);
      setLoadErr(null);
      setSelected((sel) => (sel && l.entries.some((e) => e.path === sel) ? sel : null));
    } catch (e) {
      if (!alive.current) return;
      setLoadErr(errMessage(e).message);
    }
  }, []);

  // 初始加载 + 轮询
  useEffect(() => {
    alive.current = true;
    void (async () => {
      // ?path= / props.initialPath（VWM 内嵌）初始定位，失败回退 home
      const initial = props?.initialPath ?? new URLSearchParams(window.location.search).get("path") ?? "";
      let h = "C:\\";
      try {
        h = await ipc.exHome();
      } catch {
        /* 主目录不可得 → 兜底 C:\ */
      }
      if (!alive.current) return;
      setHome(h);
      setTabs([{ id: bootTabId.current, history: [initial || h], hi: 0 }]);
    })();
    void ipc
      .exDrives()
      .then((d) => {
        if (alive.current) setDrives(d);
      })
      .catch(() => {});
    void ipc
      .exVariableDirs()
      .then((d) => {
        if (alive.current) setVarDirs(d);
      })
      .catch(() => {});
    void ipc
      .exFavList()
      .then((f) => {
        if (alive.current) setFavs(f);
      })
      .catch(() => {});
    const id = window.setInterval(() => {
      if (!document.hidden && !searchActiveRef.current) void refresh();
    }, 5000);
    return () => {
      alive.current = false;
      window.clearInterval(id);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadPath, refresh]);

  // 当前标签页路径变化 → 加载（含首次与切换）
  useEffect(() => {
    if (!currentPath || listing?.path === currentPath) return;
    void loadPath(currentPath);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentPath]);

  // 搜索（防抖）：语法查询走后端递归
  useEffect(() => {
    if (!isSyntaxQuery(filter)) {
      setSearch(null);
      setSearching(false);
      return;
    }
    if (!filter.trim() || !currentPath) return;
    setSearching(true);
    const id = window.setTimeout(() => {
      void ipc
        .exSearch(currentPath, filter.trim())
        .then((r) => {
          if (!alive.current) return;
          setSearch(r);
          setSearching(false);
        })
        .catch(() => {
          if (!alive.current) return;
          setSearch(null);
          setSearching(false);
        });
    }, 350);
    return () => window.clearTimeout(id);
  }, [filter, currentPath]);

  // 排序（目录永远在前）
  const visible = useMemo(() => {
    const base = search ? search.entries : listing?.entries ?? [];
    const q = filter.trim().toLowerCase();
    const filtered =
      q && !isSyntaxQuery(q) ? base.filter((e) => e.name.toLowerCase().includes(q)) : base;
    const { key, dir } = sort;
    const val = (e: ExEntry): string | number =>
      key === "name"
        ? e.name.toLowerCase()
        : key === "size"
          ? e.size
          : key === "modified"
            ? e.updatedAt
            : key === "created"
              ? e.createdAt
              : e.ext ?? "";
    return [...filtered].sort((a, b) => {
      const da = a.kind === "dir";
      const db = b.kind === "dir";
      if (da !== db) return db ? 1 : -1;
      const va = val(a);
      const vb = val(b);
      if (va < vb) return -dir;
      if (va > vb) return dir;
      return 0;
    });
  }, [search, listing, filter, sort]);

  const selEntry = useMemo(
    () => visible.find((e) => e.path === selected) ?? listing?.entries.find((e) => e.path === selected) ?? null,
    [visible, listing, selected],
  );

  // ---------- 操作 ----------

  const openEntry = useCallback(
    (e: ExEntry): void => {
      if (e.kind === "dir") nav(e.path);
      else
        void ipc.openPath(e.path).catch((err) => {
          pushToast("error", t("openFailed"), errMessage(err).message);
        });
    },
    [nav, t],
  );

  const mkdir = useCallback(async (): Promise<void> => {
    const p = pathRef.current;
    if (!p) return;
    const name = await askPrompt({ title: t("newFolder"), initial: "" });
    if (!name) return;
    try {
      await ipc.exMkdir(p, name);
      await refresh();
    } catch (e) {
      pushToast("error", t("newFolder"), errMessage(e).message);
    }
  }, [t, refresh]);

  const rename = useCallback(
    async (e: ExEntry): Promise<void> => {
      const name = await askPrompt({ title: t("exRenameTitle"), initial: e.name });
      if (!name || name === e.name) return;
      try {
        const newPath = await ipc.exRename(e.path, name);
        setSelected(newPath);
        await refresh();
      } catch (err) {
        pushToast("error", t("exRename"), errMessage(err).message);
      }
    },
    [t, refresh],
  );

  const trash = useCallback(
    async (entries: ExEntry[]): Promise<void> => {
      if (entries.length === 0) return;
      const ok = await askConfirm({
        title: t("trashTitle"),
        body:
          entries.length === 1
            ? t("trashBody", { name: entries[0]?.name ?? "" })
            : t("trashBodyMulti", { n: entries.length }),
        danger: true,
        okLabel: t("exDelete"),
      });
      if (!ok) return;
      try {
        await ipc.exTrash(entries.map((e) => e.path));
        pushToast("success", t("deletedToast"), entries.map((e) => e.name).join(", "));
        await refresh();
      } catch (err) {
        pushToast("error", t("exDelete"), errMessage(err).message);
      }
    },
    [t, refresh],
  );

  const purge = useCallback(
    async (entries: ExEntry[]): Promise<void> => {
      if (entries.length === 0) return;
      const ok = await askConfirm({
        title: t("purgeConfirmTitle"),
        body:
          entries.length === 1
            ? t("purgeConfirmBody", { name: entries[0]?.name ?? "" })
            : t("purgeBodyMulti", { n: entries.length }),
        danger: true,
        okLabel: t("purgeSel"),
      });
      if (!ok) return;
      try {
        await ipc.exPurge(entries.map((e) => e.path));
        pushToast("success", t("purgedToast"), String(entries.length));
        await refresh();
      } catch (err) {
        pushToast("error", t("purgeConfirmTitle"), errMessage(err).message);
      }
    },
    [t, refresh],
  );

  /** 批次E-7：敏感文件焚毁（多次覆写 + 随机改名 + 删除，不经回收站）。 */
  const shred = useCallback(
    async (entry: ExEntry): Promise<void> => {
      const ok = await askConfirm({
        title: t("shredTitle"),
        body: t("shredConfirmBody", { name: entry.name }),
        danger: true,
        okLabel: t("shredTitle"),
      });
      if (!ok) return;
      try {
        await ipc.privacyShred(entry.path);
        pushToast("success", t("shredTitle"), entry.name);
        await refresh();
      } catch (err) {
        pushToast("error", t("shredTitle"), errMessage(err).message);
      }
    },
    [t, refresh],
  );

  /** 粘贴到目录（含冲突流程，规格 7.7）。 */
  const pasteInto = useCallback(
    async (destDir: string, clip: { paths: string[]; cut: boolean }): Promise<void> => {
      const conflicts = new Set(await ipc.exConflicts(clip.paths, destDir).catch(() => []));
      let allMode: ExCopyMode | null = null;
      for (let i = 0; i < clip.paths.length; i++) {
        const p = clip.paths[i] ?? "";
        let mode: ExCopyMode | null = allMode;
        if (conflicts.has(p) && !mode) {
          const choice = await openConflict(pathTail(p), clip.paths.length > 1);
          if (choice === "cancel") return;
          if (choice === "skip") continue;
          if (choice === "replace-all") {
            allMode = "replace";
            mode = "replace";
          } else if (choice === "keep-all") {
            allMode = "keep";
            mode = "keep";
          } else {
            mode = choice;
          }
        }
        try {
          if (clip.cut) await ipc.exMove(p, destDir, mode ?? undefined);
          else await ipc.exCopy(p, destDir, mode ?? undefined);
        } catch (e) {
          pushToast("error", clip.cut ? t("exMoveFail") : t("exCopyFail"), errMessage(e).message);
        }
      }
      if (clip.cut) {
        exClipboard = null;
        clipRef.current = null;
        setClipboardTick((n) => n + 1);
      }
      await refresh();
    },
    [openConflict, t, refresh],
  );

  const copySel = useCallback((cut: boolean): void => {
    if (!selEntry) return;
    exClipboard = { paths: [selEntry.path], cut };
    clipRef.current = exClipboard;
    setClipboardTick((n) => n + 1);
  }, [selEntry]);

  const pasteSel = useCallback((): void => {
    const clip = clipRef.current;
    if (clip && pathRef.current) void pasteInto(pathRef.current, clip);
  }, [pasteInto]);

  const toggleFav = useCallback(
    (p: string): void => {
      const isFav = favs.includes(p);
      void (isFav ? ipc.exFavRemove(p) : ipc.exFavAdd(p))
        .then(setFavs)
        .catch((e) => pushToast("error", t("favorites"), errMessage(e).message));
    },
    [favs, t],
  );

  /** 新标签页（复制当前路径）。 */
  const newTab = useCallback((): void => {
    const p = pathRef.current;
    const tb: Tab = { id: nextTabId++, history: [p], hi: 0 };
    setTabs((tbs) => [...tbs, tb]);
    setActiveId(tb.id);
    setFilter("");
    setSearch(null);
    void loadPath(p);
  }, [loadPath]);

  const closeTab = useCallback(
    (id: number): void => {
      if (tabs.length === 1) {
        // 最后一个标签 → 关闭窗口（Windows 习惯）
        void getCurrentWindow().close().catch(() => {});
        return;
      }
      const idx = tabs.findIndex((x) => x.id === id);
      const rest = tabs.filter((x) => x.id !== id);
      setTabs(rest);
      if (id === activeId) {
        const next = rest[Math.min(idx, rest.length - 1)];
        if (next) {
          setActiveId(next.id);
          setFilter("");
          setSearch(null);
          void loadPath(next.history[next.hi] ?? "");
        }
      }
    },
    [tabs, activeId, loadPath],
  );

  const cycleTab = useCallback((): void => {
    if (tabs.length < 2) return;
    const idx = tabs.findIndex((x) => x.id === activeId);
    const next = tabs[(idx + 1) % tabs.length];
    if (next) {
      setActiveId(next.id);
      setFilter("");
      setSearch(null);
      void loadPath(next.history[next.hi] ?? "");
    }
  }, [tabs, activeId, loadPath]);

  // ---------- 视图 / 排序 ----------

  const cycleView = useCallback((): void => {
    setView((v) => (v === "list" ? "icons" : v === "icons" ? "thumbs" : "list"));
  }, []);

  const clickSort = useCallback(
    (key: SortKey): void => {
      setSort((s) => (s.key === key ? { key, dir: s.dir === 1 ? -1 : 1 } : { key, dir: 1 }));
    },
    [],
  );

  // Ctrl+滚轮 切换视图（原生监听，preventDefault 需要 passive:false）
  useEffect(() => {
    const el = listRef.current;
    if (!el) return undefined;
    const onWheel = (e: WheelEvent): void => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      cycleView();
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [cycleView]);

  // ---------- 键盘全集（规格 7.6） ----------

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      const el = e.target as HTMLElement | null;
      const inInput =
        el?.tagName === "INPUT" || el?.tagName === "TEXTAREA" || el?.isContentEditable === true;
      if (inInput) {
        if (e.key === "Escape") (el as HTMLInputElement).blur();
        return;
      }
      const k = e.key.toLowerCase();
      const ctrl = e.ctrlKey;
      if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        goBack();
      } else if (e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        goForward();
      } else if (e.altKey && e.key === "ArrowUp") {
        e.preventDefault();
        goUp();
      } else if (e.key === "Backspace") {
        e.preventDefault();
        goBack();
      } else if (e.key === "F5") {
        e.preventDefault();
        void refresh();
      } else if (e.key === "F4" || (ctrl && k === "l")) {
        e.preventDefault();
        setAddrEdit(pathRef.current);
        window.setTimeout(() => addrRef.current?.select(), 30);
      } else if (ctrl && k === "f") {
        e.preventDefault();
        searchRef.current?.focus();
      } else if (ctrl && !e.shiftKey && k === "t") {
        e.preventDefault();
        newTab();
      } else if (ctrl && e.key === "Tab") {
        e.preventDefault();
        cycleTab();
      } else if (ctrl && k === "w") {
        e.preventDefault();
        closeTab(activeId);
      } else if (ctrl && !e.shiftKey && k === "n") {
        e.preventDefault();
        // VWM 内嵌时 Ctrl+N 开新的虚拟窗口实例（留在环境内）；独立窗口走 OS 拆窗
        if (embedded) openVwmSystem("explorer", pathRef.current);
        else void openExplorerWindow(pathRef.current);
      } else if (ctrl && e.shiftKey && k === "n") {
        e.preventDefault();
        void mkdir();
      } else if (ctrl && k === "c") {
        e.preventDefault();
        copySel(false);
      } else if (ctrl && k === "x") {
        e.preventDefault();
        copySel(true);
      } else if (ctrl && k === "v") {
        e.preventDefault();
        pasteSel();
      } else if (e.key === "F2" && selEntry) {
        void rename(selEntry);
      } else if (e.key === "Delete" && selEntry) {
        e.preventDefault();
        void (e.shiftKey ? purge([selEntry]) : trash([selEntry]));
      } else if (e.key === "Enter" && selEntry) {
        e.preventDefault();
        openEntry(selEntry);
      } else if (e.key === "Escape") {
        setSelected(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    goBack, goForward, goUp, refresh, newTab, cycleTab, closeTab, activeId, mkdir,
    copySel, pasteSel, selEntry, rename, trash, purge, openEntry,
  ]);

  // ---------- 右键菜单 ----------

  const openBlankMenu = (ev: React.MouseEvent): void => {
    if (ev.button !== 0) ev.preventDefault();
    const items: MenuItem[] = [
      { label: t("newFolder"), icon: <FolderPlus size={14} />, onClick: () => void mkdir() },
      { label: t("exPaste"), disabled: clipRef.current === null, onClick: pasteSel },
      { separator: true },
      { label: t("exRefresh"), icon: <RefreshCw size={14} />, onClick: () => void refresh() },
    ];
    openContextMenu(ev.clientX, ev.clientY, items);
  };

  const openRowMenu = (ev: React.MouseEvent, e: ExEntry): void => {
    ev.preventDefault();
    ev.stopPropagation();
    setSelected(e.path);
    const isFav = favs.includes(e.path);
    const items: MenuItem[] = [
      { label: t("exOpen"), onClick: () => openEntry(e) },
      ...(e.kind === "dir"
        ? [{
            label: isFav ? t("exFavRemove") : t("exFavAdd"),
            icon: <Star size={13} />,
            onClick: () => toggleFav(e.path),
          }]
        : []),
      { separator: true },
      { label: t("exCopyAction"), onClick: () => copySel(false) },
      { label: t("exCut"), onClick: () => copySel(true) },
      { separator: true },
      { label: t("exRename"), onClick: () => void rename(e) },
      { label: t("exDelete"), icon: <Trash2 size={14} />, danger: true, onClick: () => void trash([e]) },
      {
        label: t("exPurgeAction"),
        danger: true,
        onClick: () => void purge([e]),
      },
      // 批次E-7：焚毁（覆写删除，仅文件；不经回收站）
      ...(e.kind === "file"
        ? [{ label: t("shredTitle"), danger: true, onClick: () => void shred(e) }]
        : []),
      { separator: true },
      // 批次C（规格 5.7.2）：共享到其他软件 —— 拖到 Write 编辑器嵌入图片/附件
      ...(
        e.kind === "file"
          ? [{
              label: t("xfShare"),
              icon: <Share2 size={13} />,
              onClick: () => {
                void beginXDrag("file", e.name, {
                  path: e.path,
                  name: e.name,
                  isImage: /\.(png|jpe?g|webp|gif|bmp|avif)$/i.test(e.path),
                });
              },
            }]
          : []
      ),
      {
        label: t("showInExplorer"),
        onClick: () => void ipc.revealPath(e.path).catch(() => {}),
      },
    ];
    openContextMenu(ev.clientX, ev.clientY, items);
  };

  // ---------- 拖拽（规格 7.6：移动（同盘）/ 复制（跨盘）；Ctrl 复制 / Shift 移动） ----------

  const onRowDragStart = (ev: React.DragEvent, e: ExEntry): void => {
    ev.dataTransfer.setData("application/x-variable-ex", JSON.stringify([e.path]));
    ev.dataTransfer.effectAllowed = "copyMove";
  };

  const onDirDrop = (ev: React.DragEvent, target: ExEntry): void => {
    ev.preventDefault();
    ev.stopPropagation();
    const raw = ev.dataTransfer.getData("application/x-variable-ex");
    let paths: string[] = [];
    try {
      paths = JSON.parse(raw) as string[];
    } catch {
      return;
    }
    if (!Array.isArray(paths) || paths.length === 0) return;
    const force = ev.ctrlKey ? "copy" : ev.shiftKey ? "move" : null;
    const src = paths[0] ?? "";
    const sameDrive = src[0]?.toLowerCase() === target.path[0]?.toLowerCase();
    const action = force ?? (sameDrive ? "move" : "copy");
    const clip = { paths, cut: action === "move" };
    void pasteInto(target.path, clip);
  };

  // ---------- 渲染 ----------

  const typeLabel = (e: ExEntry): string => {
    if (e.kind === "dir") return t("kindFolder");
    if (e.ext) return t("typeFileExt", { ext: e.ext.toUpperCase() });
    return lang !== "en" ? "文件" : "File";
  };

  const sortArrow = (key: SortKey): string => (sort.key === key ? (sort.dir === 1 ? " ↑" : " ↓") : "");

  const title = currentPath ? crumbs(currentPath).slice(-1)[0]?.label || t("thisPC") : t("explorerWin");

  const canBack = activeTab ? activeTab.hi > 0 : false;
  const canForward = activeTab ? activeTab.hi < activeTab.history.length - 1 : false;
  const statusText = searching
    ? t("exSearching")
    : search
      ? search.truncated
        ? t("exSearchTruncated", { n: search.entries.length })
        : t("exSearchDone", { n: search.entries.length, folders: search.scanned })
      : t("items", { n: listing?.entries.length ?? 0 });

  return (
    <>
      {!embedded && <ExTitlebar title={title} />}
      <div className="ex-body">
        <div className="ex-explorer">
          {/* 标签页（规格 7.7） */}
          <div className="ex-tabs" role="tablist">
            {tabs.map((tb) => (
              <div
                key={tb.id}
                role="tab"
                aria-selected={tb.id === activeId}
                className={`ex-tab${tb.id === activeId ? " active" : ""}`}
                onClick={() => {
                  if (tb.id !== activeId) {
                    setActiveId(tb.id);
                    setFilter("");
                    setSearch(null);
                    void loadPath(tb.history[tb.hi] ?? "");
                  }
                }}
                >
                  <Folder size={13} strokeWidth={1.7} />
                  <span className="ex-tab-name">{pathTail(tb.history[tb.hi] ?? "") || t("thisPC")}</span>
                <button
                  type="button"
                  className="ex-tab-close"
                  aria-label={t("close")}
                  onClick={(ev) => {
                    ev.stopPropagation();
                    closeTab(tb.id);
                  }}
                >
                  <X size={11} />
                </button>
              </div>
            ))}
            <button type="button" className="ex-tab-new" aria-label={t("exNewTab")} title={t("exNewTab")} onClick={newTab}>
              +
            </button>
          </div>

          <aside className="ex-side">
            <p className="ex-side-head">{t("quickAccess")}</p>
            {home && (
              <button
                type="button"
                className={`ex-side-btn${currentPath === home ? " active" : ""}`}
                onClick={() => nav(home)}
              >
                <FolderOpen size={15} strokeWidth={1.7} />
                <span>{t("homeDir")}</span>
              </button>
            )}
            {["Desktop", "Documents", "Downloads"].map((sub) =>
              home ? (
                <button
                  key={sub}
                  type="button"
                  className={`ex-side-btn${currentPath === `${home}\\${sub}` ? " active" : ""}`}
                  onClick={() => nav(`${home}\\${sub}`)}
                >
                  <FolderOpen size={15} strokeWidth={1.7} />
                  <span>{t(sub === "Desktop" ? "desktopDir" : sub === "Documents" ? "documentsDir" : "downloadsDir")}</span>
                </button>
              ) : null,
            )}
            <p className="ex-side-head">{t("favorites")}</p>
            {favs.length === 0 ? (
              <p className="ex-side-hint dim">{t("favHint")}</p>
            ) : (
              favs.map((f) => (
                <button
                  key={f}
                  type="button"
                  className={`ex-side-btn${currentPath === f ? " active" : ""}`}
                  title={f}
                  onClick={() => nav(f)}
                  onContextMenu={(ev) => {
                    ev.preventDefault();
                    openContextMenu(ev.clientX, ev.clientY, [
                      { label: t("exFavRemove"), icon: <Star size={13} />, onClick: () => toggleFav(f) },
                    ]);
                  }}
                >
                  <Star size={15} strokeWidth={1.7} />
                  <span>{pathTail(f)}</span>
                </button>
              ))
            )}
            <p className="ex-side-head">{t("variableDir")}</p>
            {varDirs.length === 0 ? (
              <p className="ex-side-hint dim">{t("varDirHint")}</p>
            ) : (
              varDirs.map((v) => (
                <button
                  key={v.key}
                  type="button"
                  className={`ex-side-btn${currentPath === v.path ? " active" : ""}`}
                  title={v.path}
                  onClick={() => nav(v.path)}
                >
                  <FolderOpen size={15} strokeWidth={1.7} />
                  <span>
                    {v.key === "root"
                      ? "Variable"
                      : v.key === "workspace"
                        ? t("varWorkspace")
                        : v.key === "apps"
                          ? t("varApps")
                          : t("varRecycle")}
                  </span>
                </button>
              ))
            )}
            <p className="ex-side-head">{t("thisPC")}</p>
            {drives.map((d) => (
              <button
                key={d.letter}
                type="button"
                className={`ex-side-btn${currentPath === d.path ? " active" : ""}`}
                onClick={() => nav(d.path)}
              >
                <HardDrive size={15} strokeWidth={1.7} />
                <span>
                  {t("localDisk")} ({d.letter}:)
                </span>
              </button>
            ))}
          </aside>

          <div className="ex-main">
            <div className="ex-toolbar">
              <button
                type="button"
                className="ex-tool-btn"
                disabled={!canBack}
                onClick={goBack}
                aria-label={t("navBack")}
                title={`${t("navBack")} (Alt+←)`}
              >
                <ArrowLeft size={15} />
              </button>
              <button
                type="button"
                className="ex-tool-btn"
                disabled={!canForward}
                onClick={goForward}
                aria-label={t("navForward")}
                title={`${t("navForward")} (Alt+→)`}
              >
                <ArrowRight size={15} />
              </button>
              <button
                type="button"
                className="ex-tool-btn"
                disabled={!listing?.parent}
                onClick={goUp}
                aria-label={t("navUp")}
                title={`${t("navUp")} (Alt+↑)`}
              >
                <ArrowUp size={15} />
              </button>
              <button type="button" className="ex-tool-btn" disabled={!listing} onClick={() => void mkdir()}>
                <FolderPlus size={15} /> {t("newFolder")}
              </button>
              <button type="button" className="ex-tool-btn" disabled={!listing} onClick={() => void refresh()}>
                <RefreshCw size={15} /> {t("exRefresh")}
              </button>
              <span className="ex-flex1" />
              <div className="ex-view-switch" role="group" aria-label={t("exViewMode")}>
                <button
                  type="button"
                  className={`ex-view-btn${view === "list" ? " on" : ""}`}
                  aria-label={t("viewList")}
                  title={`${t("viewList")} (Ctrl+滚轮)`}
                  onClick={() => setView("list")}
                >
                  <List size={14} />
                </button>
                <button
                  type="button"
                  className={`ex-view-btn${view === "icons" ? " on" : ""}`}
                  aria-label={t("viewIcons")}
                  title={t("viewIcons")}
                  onClick={() => setView("icons")}
                >
                  <LayoutGrid size={14} />
                </button>
                <button
                  type="button"
                  className={`ex-view-btn${view === "thumbs" ? " on" : ""}`}
                  aria-label={t("viewThumbs")}
                  title={t("viewThumbs")}
                  onClick={() => setView("thumbs")}
                >
                  <ImageIcon size={14} />
                </button>
              </div>
              <label className="ex-search">
                <Search size={14} />
                <input
                  ref={searchRef}
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                  placeholder={t("searchHintSyntax")}
                  spellCheck={false}
                />
              </label>
            </div>

            {/* 地址栏：面包屑 ↔ 编辑（F4/Ctrl+L；粘贴路径 Enter 跳转） */}
            <div className="ex-crumbs" aria-label="breadcrumb">
              {addrEdit !== null ? (
                <input
                  ref={addrRef}
                  className="ex-addr-input"
                  value={addrEdit}
                  autoFocus
                  spellCheck={false}
                  onChange={(e) => setAddrEdit(e.target.value)}
                  onBlur={() => setAddrEdit(null)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      const p = addrEdit.trim();
                      if (p) nav(p);
                      else setAddrEdit(null);
                    } else if (e.key === "Escape") {
                      setAddrEdit(null);
                    }
                  }}
                  onPaste={(e) => {
                    // 规格 7.7：地址栏粘贴路径 → 直接跳转
                    const text = e.clipboardData.getData("text");
                    if (text.includes("\\") || text.includes("/")) {
                      e.preventDefault();
                      nav(text.trim());
                    }
                  }}
                />
              ) : (
                <>
                  {currentPath
                    ? crumbs(currentPath).map((c, i) => (
                        <span key={c.path} className="ex-crumb-seg">
                          {i > 0 && <span className="ex-crumb-sep">›</span>}
                          <button type="button" className="ex-crumb" onClick={() => nav(c.path)}>
                            {c.label}
                          </button>
                        </span>
                      ))
                    : null}
                  <button
                    type="button"
                    className="ex-crumb-edit"
                    aria-label={t("exEditAddr")}
                    title={`${t("exEditAddr")} (F4)`}
                    onClick={() => {
                      setAddrEdit(currentPath);
                      window.setTimeout(() => addrRef.current?.select(), 30);
                    }}
                  >
                    ✎
                  </button>
                </>
              )}
            </div>

            <div
              ref={listRef}
              className={`ex-list view-${view}`}
              tabIndex={0}
              onContextMenu={(e) => {
                if ((e.target as HTMLElement).closest(".ex-row, .ex-tile") === null) openBlankMenu(e);
              }}
            >
              {view === "list" ? (
                <>
                  <div className="ex-head-row">
                    <span onClick={() => clickSort("name")}>{t("colName")}{sortArrow("name")}</span>
                    <span onClick={() => clickSort("modified")}>{t("colModified")}{sortArrow("modified")}</span>
                    <span onClick={() => clickSort("type")}>{t("colType")}{sortArrow("type")}</span>
                    <span onClick={() => clickSort("size")}>{t("colSize")}{sortArrow("size")}</span>
                    <span onClick={() => clickSort("created")}>{t("colCreated")}{sortArrow("created")}</span>
                  </div>
                  {!listing && !loadErr ? (
                    <div className="ex-hint dim" aria-busy="true" />
                  ) : loadErr ? (
                    <div className="ex-hint dim">{t("failedLoad")}</div>
                  ) : visible.length === 0 ? (
                    <div className="ex-hint dim">{search ? t("noResults") : t("emptyDirHint")}</div>
                  ) : (
                    visible.map((e) => {
                      const cut = exClipboard?.paths.includes(e.path) && exClipboard.cut;
                      void clipboardTick;
                      return (
                        <div
                          key={e.path}
                          className={`ex-row${selected === e.path ? " selected" : ""}${e.hidden ? " hidden-entry" : ""}${cut ? " cut" : ""}`}
                          onClick={() => setSelected(e.path)}
                          onDoubleClick={() => openEntry(e)}
                          onContextMenu={(ev) => openRowMenu(ev, e)}
                          draggable
                          onDragStart={(ev) => onRowDragStart(ev, e)}
                          onDragOver={(ev) => {
                            if (e.kind === "dir" && ev.dataTransfer.types.includes("application/x-variable-ex")) {
                              ev.preventDefault();
                            }
                          }}
                          onDrop={(ev) => {
                            if (e.kind === "dir") onDirDrop(ev, e);
                          }}
                        >
                          <span className="ex-col-name">
                            {e.kind === "dir" ? (
                              <Folder size={16} className="ex-ic" />
                            ) : (
                              <FileIcon size={16} className="ex-ic dim" />
                            )}
                            <span className="ex-name-text">{e.name}</span>
                          </span>
                          <span className="ex-col-date">
                            {e.updatedAt ? new Date(e.updatedAt).toLocaleString(lang === "en" ? "en-US" : "zh-CN") : "—"}
                          </span>
                          <span className="ex-col-type dim">{typeLabel(e)}</span>
                          <span className="ex-col-size dim">{e.kind === "dir" ? "—" : fmtSize(e.size)}</span>
                          <span className="ex-col-date dim">
                            {e.createdAt ? new Date(e.createdAt).toLocaleDateString(lang === "en" ? "en-US" : "zh-CN") : "—"}
                          </span>
                        </div>
                      );
                    })
                  )}
                </>
              ) : (
                <div className="ex-grid">
                  {visible.map((e) => {
                    const cut = exClipboard?.paths.includes(e.path) && exClipboard.cut;
                    void clipboardTick;
                    const isImg = e.kind === "file" && e.ext != null && THUMB_EXTS.has(e.ext);
                    const isVid = e.kind === "file" && e.ext != null && VIDEO_EXTS.has(e.ext);
                    return (
                      <div
                        key={e.path}
                        className={`ex-tile${selected === e.path ? " selected" : ""}${cut ? " cut" : ""}`}
                        onClick={() => setSelected(e.path)}
                        onDoubleClick={() => openEntry(e)}
                        onContextMenu={(ev) => openRowMenu(ev, e)}
                        draggable
                        onDragStart={(ev) => onRowDragStart(ev, e)}
                        onDragOver={(ev) => {
                          if (e.kind === "dir" && ev.dataTransfer.types.includes("application/x-variable-ex")) {
                            ev.preventDefault();
                          }
                        }}
                        onDrop={(ev) => {
                          if (e.kind === "dir") onDirDrop(ev, e);
                        }}
                      >
                        {view === "thumbs" && isImg ? (
                          <ExThumb entry={e} />
                        ) : e.kind === "dir" ? (
                          <Folder size={40} strokeWidth={1.3} className="ex-ic" />
                        ) : isVid ? (
                          <FileIcon size={40} strokeWidth={1.3} className="ex-ic dim" />
                        ) : (
                          <FileIcon size={40} strokeWidth={1.3} className="ex-ic dim" />
                        )}
                        <span className="ex-tile-name" title={e.name}>{e.name}</span>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>

            <div className="ex-status">{statusText}</div>
          </div>
        </div>
      </div>

      {/* 冲突对话框（规格 7.7：替换 / 跳过 / 保留两者 / 全部应用） */}
      {conflict && (
        <div
          className="ex-dlg-overlay"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) {
              conflict.resolve("cancel");
              setConflict(null);
            }
          }}
        >
          <div className="ex-dlg" role="dialog" aria-label={t("conflictTitle")}>
            <p className="ex-dlg-title">{t("conflictTitle")}</p>
            <p className="ex-dlg-body">{t("conflictBody", { name: conflict.name })}</p>
            <div className="ex-dlg-actions">
              <button type="button" onClick={() => {
                conflict.resolve(conflict.multiple ? "replace-all" : "replace");
                setConflict(null);
              }}>
                {conflict.multiple ? t("conflictReplaceAll") : t("conflictReplace")}
              </button>
              <button type="button" onClick={() => {
                conflict.resolve(conflict.multiple ? "keep-all" : "keep");
                setConflict(null);
              }}>
                {conflict.multiple ? t("conflictKeepAll") : t("conflictKeep")}
              </button>
              {conflict.multiple && (
                <button type="button" onClick={() => {
                  conflict.resolve("skip");
                  setConflict(null);
                }}>
                  {t("conflictSkip")}
                </button>
              )}
              <button type="button" className="dim" onClick={() => {
                conflict.resolve("cancel");
                setConflict(null);
              }}>
                {t("cancel")}
              </button>
            </div>
          </div>
        </div>
      )}

      {ExHosts(embedded)}
    </>
  );
}

/** 缩略图（异步加载，会话内缓存；失败如实显示图标）。 */
function ExThumb(props: { entry: ExEntry }): React.ReactElement {
  const key = thumbKey(props.entry);
  const cached = thumbCache.get(key);
  const [url, setUrl] = useState<string | null>(cached ?? null);
  const [failed, setFailed] = useState(cached === null);

  useEffect(() => {
    if (url || failed) return undefined;
    let alive = true;
    scheduleThumb(() => {
      void ipc
        .exThumbnail(props.entry.path)
        .then((d) => {
          if (thumbCache.size > THUMB_CACHE_MAX) thumbCache.clear();
          thumbCache.set(key, d);
          if (alive) setUrl(d);
        })
        .catch(() => {
          thumbCache.set(key, null);
          if (alive) setFailed(true);
        })
        .finally(() => thumbDone());
    });
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  if (url) {
    return <img className="ex-thumb" src={url} alt="" draggable={false} />;
  }
  if (failed) {
    return <FileIcon size={40} strokeWidth={1.3} className="ex-ic dim" />;
  }
  return <div className="ex-thumb loading" aria-busy="true" />;
}
