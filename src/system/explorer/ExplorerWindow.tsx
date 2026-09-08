import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ArrowLeft, ArrowRight, ArrowUp, Columns3, Copy, File as FileIcon, FileArchive, Folder, FolderOpen, FolderPlus,
  GitCompare, HardDrive, Image as ImageIcon, LayoutGrid, List, Network, Pin, PinOff, Printer, RefreshCw, Search, Send, Share2, ShieldCheck, Star, Trash2, X,
} from "lucide-react";
import { askChoice, askConfirm, askPrompt, ConfirmHost, NetConsentHost, PromptHost } from "../../components/Modal";
import { ContextMenuHost, openContextMenu, type MenuItem } from "../../components/ContextMenu";
import { ToastHost } from "../../components/ToastHost";
import { WindowControls } from "../../components/WindowControls";
import { useI18n } from "../../i18n";
import {
  errMessage, ipc, type ArchiveEntry, type ExCopyMode, type ExEntry, type ExListing, type ExSearchResult, type ExVarDir,
  type LockHolder, type NetDrive, type SendToItem,
} from "../../lib/ipc";
import {
  fuzzySuggestions, fromSavedView, highlightParts, nameIssues, pageSlice, sameTypeGroup, toSavedView,
  type ExViewMode,
} from "./exFeatures";
import { beginXDrag } from "../../lib/xflow";
import { pushToast } from "../../state/uiStore";
import { openExplorerWindow, trackSelfGeom } from "../windows/appWindows";
import { getThirdApps } from "../launcher/thirdApps";
import { openVwmSystem } from "../windows/vwm";
import { RecycleView } from "../recycle/RecycleView";
import { showNativeContextMenu } from "../compat/ShellProxy";
import { QuickPreview, type QuickPreviewTarget } from "./QuickPreview";
import { loadCtxConfig, loadDeleteTier, type CtxItemId } from "./ctxMenu";

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
type SortKey = "name" | "modified" | "created" | "type" | "size";

/** U-16：分页虚拟化页大小（超出才分页）。 */
const PAGE_SIZE = 500;

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
    <div className="ex-window" data-view={view} data-testid="explorer-window">
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
  // F-2.6 空格快速预览
  const [preview, setPreview] = useState<QuickPreviewTarget | null>(null);
  const [filter, setFilter] = useState("");
  const [search, setSearch] = useState<ExSearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const [view, setView] = useState<ExViewMode>("list");
  const [sort, setSort] = useState<{ key: SortKey; dir: 1 | -1 }>({ key: "name", dir: 1 });
  const [favs, setFavs] = useState<string[]>([]);
  const [home, setHome] = useState<string | null>(null);
  const [drives, setDrives] = useState<{ letter: string; path: string }[]>([]);
  // AI-09 Z-31：网络驱动器（侧栏如实显示，断连置灰）
  const [netDrives, setNetDrives] = useState<NetDrive[]>([]);
  // AI-09 Z-35：发送到目标（系统 + 自定义）
  const [sendto, setSendto] = useState<SendToItem[]>([]);
  // AI-09 M-20：多选集合（Ctrl+点击）
  const [selSet, setSelSet] = useState<Set<string>>(new Set());
  // AI-09 M-25：占用查看对话框
  const [lockView, setLockView] = useState<{ path: string; holders: LockHolder[] } | null>(null);
  // 批次E（规格 7.2）：Variable 数据目录节点组
  const [varDirs, setVarDirs] = useState<ExVarDir[]>([]);
  const [addrEdit, setAddrEdit] = useState<string | null>(null);
  const [conflict, setConflict] = useState<ConflictState | null>(null);
  const [clipboardTick, setClipboardTick] = useState(0); // 触发剪贴板高亮重渲染
  // AI-10 U-16：分页虚拟化 + 双栏
  const [page, setPage] = useState(0);
  const [dual, setDual] = useState(false);
  const [pane2, setPane2] = useState<{ path: string; listing: ExListing | null }>({ path: "", listing: null });
  // AI-10 V-34：拖拽计数徽标（多选拖拽时跟随光标）
  const [dragBadge, setDragBadge] = useState<{ n: number; x: number; y: number } | null>(null);
  // AI-10 V-35：两文件属性对比
  const [cmp, setCmp] = useState<[ExEntry, ExEntry] | null>(null);
  // AI-10 V-38：预览锁定（锁定后空格/导航不替换当前预览）
  const [previewPin, setPreviewPin] = useState(false);
  // AI-10 V-39：地址栏模糊建议选中项
  const [addrSugIdx, setAddrSugIdx] = useState(-1);
  // AI-10 V-40：压缩包提取向导
  const [extract, setExtract] = useState<{ archive: ExEntry; entries: ArchiveEntry[] } | null>(null);
  const [extractBusy, setExtractBusy] = useState(false);
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
      setSelSet(new Set());
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
    // AI-09 Z-31/Z-35：网络驱动器 + 发送到目标（失败静默——侧栏/菜单保持现状）
    void ipc
      .netDrives()
      .then((d) => {
        if (alive.current) setNetDrives(d);
      })
      .catch(() => {});
    void ipc
      .sendtoList()
      .then((d) => {
        if (alive.current) setSendto(d);
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

  // AI-10 V-31：路径变化 → 恢复该目录记忆的视图（LRU 持久化在后端）
  useEffect(() => {
    if (!currentPath) return;
    void ipc
      .exViewGet(currentPath)
      .then((v) => {
        const m = v ? fromSavedView(v) : null;
        if (m) setView(m);
      })
      .catch(() => {});
    setPage(0);
    setAddrSugIdx(-1);
  }, [currentPath]);

  // 过滤变化 → 回到第一页
  useEffect(() => {
    setPage(0);
  }, [filter]);

  // AI-10 V-31：用户切换视图 → 持久化（thumbs 无后端枚举，退化为 icon）
  const changeView = useCallback((v: ExViewMode): void => {
    setView(v);
    const p = pathRef.current;
    if (p) void ipc.exViewSet(p, toSavedView(v)).catch(() => {});
  }, []);

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

  // AI-10 U-16：分页虚拟化（>PAGE_SIZE 才分页，切片渲染）
  const paged = useMemo(() => pageSlice(visible, page, PAGE_SIZE), [visible, page]);
  const rows = paged.rows;

  // AI-10 V-32：过滤高亮（语法查询不高亮——结果是递归搜索而非子串过滤）
  const hlQuery = filter && !isSyntaxQuery(filter) ? filter.trim() : "";
  const hlName = (name: string): ReactNode => {
    const parts = highlightParts(name, hlQuery);
    if (!parts) return name;
    return (
      <>
        {parts[0]}
        <mark className="ex-hl">{parts[1]}</mark>
        {parts[2]}
      </>
    );
  };

  // AI-10 V-39：地址栏模糊建议池（收藏/历史/盘符/网络驱动器/环境目录）
  const addrSuggestions = useMemo(() => {
    if (addrEdit === null || !addrEdit.trim()) return [];
    const pool = [
      ...favs,
      home ?? "",
      ...drives.map((d) => d.path),
      ...netDrives.map((d) => d.path),
      ...varDirs.map((v) => v.path),
      ...tabs.flatMap((tb) => tb.history),
    ].filter(Boolean);
    return fuzzySuggestions(addrEdit, pool, 6);
  }, [addrEdit, favs, home, drives, netDrives, varDirs, tabs]);

  // AI-10 U-16：双栏第二面板加载
  useEffect(() => {
    if (!dual) return;
    setPane2((p) => (p.path ? p : { path: pathRef.current, listing: null }));
  }, [dual]);
  useEffect(() => {
    if (!dual || !pane2.path) return undefined;
    let ok = true;
    void ipc
      .exList(pane2.path)
      .then((l) => {
        if (ok) setPane2((p) => ({ ...p, listing: l }));
      })
      .catch(() => {});
    return () => {
      ok = false;
    };
  }, [dual, pane2.path]);

  // AI-10 V-34：拖拽徽标跟随光标（document 级，dragend/drop 收尾）
  const dragging = dragBadge !== null;
  useEffect(() => {
    if (!dragging) return undefined;
    const move = (ev: DragEvent): void => setDragBadge((d) => (d ? { ...d, x: ev.clientX, y: ev.clientY } : d));
    const end = (): void => setDragBadge(null);
    document.addEventListener("dragover", move);
    document.addEventListener("dragend", end);
    document.addEventListener("drop", end);
    return () => {
      document.removeEventListener("dragover", move);
      document.removeEventListener("dragend", end);
      document.removeEventListener("drop", end);
    };
  }, [dragging]);

  // AI-10 V-33：复制为路径（多选 = 每行一条）
  const copyPaths = useCallback(
    (paths: string[]): void => {
      if (paths.length === 0) return;
      void navigator.clipboard.writeText(paths.join("\r\n")).then(
        () =>
          pushToast(
            "success",
            t("exCopyPath"),
            paths.length > 1 ? t("items", { n: paths.length }) : (paths[0] ?? ""),
          ),
        () => pushToast("error", t("exCopyPath"), t("failedLoad")),
      );
    },
    [t],
  );

  // AI-10 V-37：按类型选取（与基准条目同类型全部选中）
  const selectSameType = useCallback(
    (base: ExEntry): void => {
      setSelSet(new Set(visible.filter((v) => sameTypeGroup(base, v)).map((v) => v.path)));
      setSelected(base.path);
    },
    [visible],
  );

  // AI-10 V-36：新建/重命名防呆（非法字符/保留名/结尾点空格/超长名 → 显式确认后才放行）
  const confirmNameIssues = useCallback(
    async (name: string): Promise<boolean> => {
      const issues = nameIssues(name);
      if (issues.length === 0) return true;
      return askConfirm({
        title: t("exNameWarnTitle"),
        body: t("exNameWarnBody", { issues: issues.join(" · ") }),
        okLabel: t("ok"),
      });
    },
    [t],
  );

  // AI-10 V-40：提取向导 —— 逐项解包到当前目录（后端 archive_extract_one）
  const runExtractAll = useCallback(async (): Promise<void> => {
    if (!extract) return;
    setExtractBusy(true);
    const dest = pathRef.current || "C:\\";
    let done = 0;
    try {
      for (const en of extract.entries) {
        if (en.isDir) continue;
        await ipc.archiveExtractOne(extract.archive.path, en.innerPath);
        done++;
      }
      pushToast("success", t("exExtractTitle"), t("exExtractDone", { n: done, dir: dest }));
      setExtract(null);
      await refresh();
    } catch (err) {
      pushToast("error", t("exExtractTitle"), `${errMessage(err).message} (${done})`);
    } finally {
      setExtractBusy(false);
    }
  }, [extract, t, refresh]);

  // ---------- 操作 ----------

  const openEntry = useCallback(
    (e: ExEntry): void => {
      if (e.kind === "dir") {
        nav(e.path);
        return;
      }
      // 批次B-27 环境内文件关联表：有记忆默认 → 直接以登记软件打开
      //（嵌入通道复用 C-*）；未知格式 → 「打开方式」选择器（已登记软件 + 用宿主打开），
      // 选择后记忆默认。容器内文件用宿主打开 = open_path（宿主默认程序）。
      void (async () => {
        const dot = e.path.lastIndexOf(".");
        const ext = dot > 0 ? e.path.slice(dot + 1).toLowerCase() : "";
        try {
          if (ext) {
            const assoc = await ipc.fileAssocResolve(ext);
            if (assoc) {
              const { launchThirdApp } = await import("../launcher/thirdApps");
              await launchThirdApp(assoc.appId, assoc.appName, e.path);
              return;
            }
          }
        } catch {
          /* 关联表不可用 → 走选择器 */
        }
        const apps = getThirdApps();
        const options = [
          ...apps.slice(0, 12).map((a) => ({ value: `app:${a.id}`, label: a.name })),
          { value: "host", label: t("assocHostOpen") },
        ];
        const choice = await askChoice({
          title: t("assocTitle"),
          body: `${t("assocBody")} ${e.path.split(/[\\/]/).pop() ?? e.path}`,
          options,
        });
        if (!choice) return;
        if (choice === "host") {
          await ipc.openPath(e.path).catch((err) => pushToast("error", t("openFailed"), errMessage(err).message));
          return;
        }
        const id = choice.slice(4);
        const app = apps.find((a) => a.id === id);
        try {
          if (ext) await ipc.fileAssocSet(ext, id, app?.name ?? id);
        } catch {
          /* 记忆失败不影响本次打开 */
        }
        const { launchThirdApp } = await import("../launcher/thirdApps");
        await launchThirdApp(id, app?.name ?? id, e.path);
      })();
    },
    [nav, t],
  );

  const mkdir = useCallback(async (): Promise<void> => {
    const p = pathRef.current;
    if (!p) return;
    const name = await askPrompt({ title: t("newFolder"), initial: "" });
    if (!name) return;
    // V-36：特殊名防呆（非法字符/保留名/结尾点空格/超长 → 显式确认）
    if (!(await confirmNameIssues(name))) return;
    try {
      await ipc.exMkdir(p, name);
      await refresh();
    } catch (e) {
      pushToast("error", t("newFolder"), errMessage(e).message);
    }
  }, [t, refresh, confirmNameIssues]);

  const rename = useCallback(
    async (e: ExEntry): Promise<void> => {
      const name = await askPrompt({ title: t("exRenameTitle"), initial: e.name });
      if (!name || name === e.name) return;
      // V-36：特殊名防呆
      if (!(await confirmNameIssues(name))) return;
      try {
        const newPath = await ipc.exRename(e.path, name);
        setSelected(newPath);
        await refresh();
      } catch (err) {
        pushToast("error", t("exRename"), errMessage(err).message);
      }
    },
    [t, refresh, confirmNameIssues],
  );

  const trash = useCallback(
    async (entries: ExEntry[]): Promise<void> => {
      if (entries.length === 0) return;
      // M-26：删除档位 —— 默认「环境回收站」= 现状；「询问」= 每次显式选择处置方式
      let usePurge = false;
      if (loadDeleteTier() === "ask") {
        const choice = await askChoice({
          title: t("trashTitle"),
          body:
            entries.length === 1
              ? t("trashBody", { name: entries[0]?.name ?? "" })
              : t("trashBodyMulti", { n: entries.length }),
          options: [
            { value: "recycle", label: t("exDelete") },
            { value: "purge", label: t("exPurgeAction") },
          ],
        });
        if (!choice) return;
        usePurge = choice === "purge";
      } else {
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
      }
      try {
        if (usePurge) await ipc.exPurge(entries.map((e) => e.path));
        else await ipc.exTrash(entries.map((e) => e.path));
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

  // ---------- AI-09：多选 / 发送到 / 占用查看 ----------

  /** M-20：当前生效的多选条目（含锚点；顺序按可见列表）。 */
  const selEntries = useMemo(
    () => visible.filter((e) => selSet.has(e.path) || e.path === selected),
    [visible, selSet, selected],
  );

  const copySel = useCallback((cut: boolean): void => {
    // M-20：多选时整批进剪贴板
    const paths = selEntries.length > 1 ? selEntries.map((e) => e.path) : selEntry ? [selEntry.path] : [];
    if (paths.length === 0) return;
    exClipboard = { paths, cut };
    clipRef.current = exClipboard;
    setClipboardTick((n) => n + 1);
  }, [selEntries, selEntry]);

  const pasteSel = useCallback((): void => {
    const clip = clipRef.current;
    if (clip && pathRef.current) void pasteInto(pathRef.current, clip);
  }, [pasteInto]);

  // ---------- AI-09：发送到 / 占用查看 ----------

  /** M-20：Ctrl+点击切换多选；普通点击单选。 */
  const clickRow = useCallback((ev: React.MouseEvent, e: ExEntry): void => {
    if (ev.ctrlKey || ev.metaKey) {
      setSelSet((s) => {
        const n = new Set(s);
        if (n.has(e.path)) n.delete(e.path);
        else n.add(e.path);
        return n;
      });
      setSelected(e.path);
    } else {
      setSelected(e.path);
      setSelSet(new Set());
    }
  }, []);

  /** Z-35：发送到 —— 复制文件到目标目录（显式动作，复制而非移动）。 */
  const sendTo = useCallback(
    (src: string, target: string, name: string): void => {
      void ipc
        .sendtoCopy(src, target)
        .then((dst) => {
          pushToast("success", t("exSendTo"), `${name} → ${dst}`);
        })
        .catch((e: unknown) => pushToast("error", t("exSendTo"), errMessage(e).message));
    },
    [t],
  );

  /** Z-35：把目录登记为自定义发送到目标。 */
  const sendtoAddCustom = useCallback(
    (dir: string): void => {
      void ipc
        .sendtoCustomAdd(dir)
        .then(() => ipc.sendtoList())
        .then(setSendto)
        .catch((e: unknown) => pushToast("error", t("exSendTo"), errMessage(e).message));
    },
    [t],
  );

  /** M-25：查看占用者（只读；无强拆按钮，系统未披露时如实显示）。 */
  const whoLocks = useCallback(
    (path: string): void => {
      void ipc
        .whoLocks(path)
        .then((holders) => setLockView({ path, holders }))
        .catch((e: unknown) => pushToast("error", t("exWhoLocks"), errMessage(e).message));
    },
    [t],
  );

  /** AI-08 V-97：右键打印 —— 关联判断走系统注册表（无关联如实置灰）；
   * 执行走系统默认打印关联（ShellExecute print 动词）；批量 >5 个文件需确认。 */
  const printFiles = useCallback(
    async (paths: string[]): Promise<void> => {
      if (paths.length === 0) return;
      try {
        const assoc = await ipc.printAssocCheck(paths);
        const printable = paths.filter((_, i) => assoc[i]);
        if (printable.length === 0) {
          pushToast("info", t("pqPrintAction"), t("pqNoAssoc"));
          return;
        }
        if (printable.length > 5) {
          const ok = await askConfirm({
            title: t("pqPrintAction"),
            body: t("pqBatchConfirm", { n: printable.length }),
          });
          if (!ok) return;
        }
        const errs = await ipc.printFiles(printable);
        if (errs.length > 0) pushToast("error", t("pqPrintAction"), errs.join("\n"));
        else pushToast("success", t("pqPrintAction"), t("pqPrintSent", { n: printable.length }));
      } catch (e: unknown) {
        // 脱机/缺纸等失败透传系统错误（如实）
        pushToast("error", t("pqPrintAction"), errMessage(e).message);
      }
    },
    [t],
  );

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
    setView((v) => (v === "list" ? "icons" : v === "icons" ? "thumbs" : v === "thumbs" ? "column" : "list"));
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
      } else if (e.key === "Delete" && (selEntry || selEntries.length > 0)) {
        e.preventDefault();
        const batch = selEntries.length > 1 ? selEntries : selEntry ? [selEntry] : [];
        void (e.shiftKey ? purge(batch) : trash(batch));
      } else if (e.key === "Enter" && selEntry) {
        e.preventDefault();
        openEntry(selEntry);
      } else if (e.key === "Escape") {
        // V-38：预览锁定时 Esc 不关预览（只取消选择）；未锁定按原逻辑关闭
        if (preview && !previewPin) setPreview(null);
        setSelected(null);
        setSelSet(new Set());
      } else if (e.code === "Space" && selEntry) {
        // F-2.6 快速预览：空格呼出（焦点在输入框/文本域时不触发）
        // V-38：预览锁定时空格不替换当前预览目标
        const ae = document.activeElement;
        const typing = ae instanceof HTMLElement && (ae.isContentEditable || ae.tagName === "INPUT" || ae.tagName === "TEXTAREA");
        if (!typing && !previewPin) {
          e.preventDefault();
          setPreview({ name: selEntry.name, path: selEntry.path, kind: selEntry.kind, ext: selEntry.ext, size: selEntry.size });
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    goBack, goForward, goUp, refresh, newTab, cycleTab, closeTab, activeId, mkdir,
    copySel, pasteSel, selEntry, selEntries, rename, trash, purge, openEntry,
    copyPaths, selectSameType, preview, previewPin,
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

  const openRowMenu = async (ev: React.MouseEvent, e: ExEntry): Promise<void> => {
    ev.preventDefault();
    ev.stopPropagation();
    setSelected(e.path);

    // Let Explorer and installed shell extensions (7-Zip/Git/Tortoise/etc.)
    // own the menu first.  Non-Windows and unsupported shell items return
    // shown=false and keep the existing safe Variable menu as a fallback.
    const native = await showNativeContextMenu([e.path], { x: ev.screenX, y: ev.screenY }).catch(() => null);
    if (native?.shown) return;

    const isFav = favs.includes(e.path);
    // M-19：右键菜单注册表 —— 按用户配置（显隐 + 排序）组装；仅注册表内安全动作
    const cfg = loadCtxConfig();
    const builders: Record<CtxItemId, MenuItem | undefined> = {
      open: { label: t("exOpen"), onClick: () => openEntry(e) },
      fav:
        e.kind === "dir"
          ? {
              label: isFav ? t("exFavRemove") : t("exFavAdd"),
              icon: <Star size={13} />,
              onClick: () => toggleFav(e.path),
            }
          : undefined,
      copy: { label: t("exCopyAction"), onClick: () => copySel(false) },
      cut: { label: t("exCut"), onClick: () => copySel(true) },
      // Z-35：发送到（系统目标 + 自定义目标；目录可登记为新目标）
      sendto: {
        label: t("exSendTo"),
        icon: <Send size={13} />,
        children: [
          ...sendto.map((s) => ({
            label: s.name,
            onClick: () => sendTo(e.path, s.target, e.name),
          })),
          ...(sendto.length > 0 && e.kind === "dir" ? [{ separator: true } as MenuItem] : []),
          ...(e.kind === "dir"
            ? [{
                label: t("exSendToAddCustom"),
                icon: <FolderPlus size={13} />,
                onClick: () => sendtoAddCustom(e.path),
              }]
            : []),
        ],
      },
      // AI-08 V-97：右键打印（文件；关联判断异步做，无关联时动作内如实提示）
      print:
        e.kind === "file"
          ? {
              label: t("pqPrintAction"),
              icon: <Printer size={13} />,
              onClick: () =>
                void printFiles(
                  selEntries.length > 1 && selEntries.some((s) => s.path === e.path)
                    ? selEntries.filter((s) => s.kind === "file").map((s) => s.path)
                    : [e.path],
                ),
            }
          : undefined,
      // M-25：文件占用侦探（只读，无强拆）
      wholocks:
        e.kind === "file"
          ? { label: t("exWhoLocks"), onClick: () => whoLocks(e.path) }
          : undefined,
      rename: { label: t("exRename"), onClick: () => void rename(e) },
      delete: { label: t("exDelete"), icon: <Trash2 size={14} />, danger: true, onClick: () => void trash([e]) },
      purge: { label: t("exPurgeAction"), danger: true, onClick: () => void purge([e]) },
      // 批次E-7：焚毁（覆写删除，仅文件；不经回收站）
      shred:
        e.kind === "file"
          ? { label: t("shredTitle"), danger: true, onClick: () => void shred(e) }
          : undefined,
      // 批次C（规格 5.7.2）：共享到其他软件 —— 拖到 Write 编辑器嵌入图片/附件
      share:
        e.kind === "file"
          ? {
              label: t("xfShare"),
              icon: <Share2 size={13} />,
              onClick: () => {
                void beginXDrag("file", e.name, {
                  path: e.path,
                  name: e.name,
                  isImage: /\.(png|jpe?g|webp|gif|bmp|avif)$/i.test(e.path),
                });
              },
            }
          : undefined,
      reveal: {
        label: t("showInExplorer"),
        onClick: () => void ipc.revealPath(e.path).catch(() => {}),
      },
    };
    const items: MenuItem[] = [];
    for (const id of cfg.order) {
      if (cfg.hidden.includes(id)) continue;
      const it = builders[id];
      if (!it) continue;
      // 分组分隔线：动作组 / 危险组 / 共享组 之间
      if ((id === "copy" || id === "rename" || id === "share") && items.length > 0) {
        items.push({ separator: true });
      }
      items.push(it);
    }
    // ---------- AI-10 增补动作（V-33/V-35/V-37/V-40） ----------
    const batchSel = selEntries.length > 1 && selEntries.some((s) => s.path === e.path);
    if (items.length > 0) items.push({ separator: true });
    items.push(
      // V-33：复制为路径（多选 = 每行一条）
      {
        label: t("exCopyPath"),
        icon: <Copy size={13} />,
        onClick: () => copyPaths(batchSel ? selEntries.map((s) => s.path) : [e.path]),
      },
      // V-37：按类型选取
      {
        label: t("exSelectSameType"),
        onClick: () => selectSameType(e),
      },
    );
    // V-35：两文件属性对比（恰好选中两项时可用）
    items.push({
      label: t("exCompareProps"),
      icon: <GitCompare size={13} />,
      disabled: !(selEntries.length === 2 && selEntries.some((s) => s.path === e.path)),
      onClick: () => {
        const [a, b] = selEntries;
        if (a && b) setCmp([a, b]);
      },
    });
    // V-40：压缩包提取向导（zip；复用 AI-09 只读浏览后端逐项解包）
    if (e.kind === "file" && (e.ext === "zip" || e.name.toLowerCase().endsWith(".zip"))) {
      items.push({
        label: t("exExtractTitle"),
        icon: <FileArchive size={13} />,
        onClick: () => {
          void ipc
            .archiveLs(e.path)
            .then((l) => setExtract({ archive: e, entries: l.entries }))
            .catch((err: unknown) =>
              pushToast("error", t("exExtractTitle"), errMessage(err).message),
            );
        },
      });
    }
    openContextMenu(ev.clientX, ev.clientY, items);
  };

  // ---------- 拖拽（规格 7.6：移动（同盘）/ 复制（跨盘）；Ctrl 复制 / Shift 移动） ----------

  const onRowDragStart = (ev: React.DragEvent, e: ExEntry): void => {
    // V-34：多选拖拽 —— 整批进 dataTransfer；计数徽标跟随光标
    const batch = selEntries.length > 1 && selEntries.some((s) => s.path === e.path) ? selEntries : [e];
    ev.dataTransfer.setData("application/x-variable-ex", JSON.stringify(batch.map((x) => x.path)));
    ev.dataTransfer.effectAllowed = "copyMove";
    if (batch.length > 1) setDragBadge({ n: batch.length, x: ev.clientX, y: ev.clientY });
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

  // M-20：多选批量汇总（项数 + 总大小）
  const selSummary =
    selEntries.length > 1
      ? t("exSelSummary", {
          n: selEntries.length,
          size: fmtSize(selEntries.reduce((s, e) => s + e.size, 0)),
        })
      : null;

  // M-22 键导航：预览在可见列表中的前后邻居
  const previewIdx = preview ? visible.findIndex((e) => e.path === preview.path) : -1;
  const prevEntry = previewIdx > 0 ? visible[previewIdx - 1] : null;
  const nextEntry = previewIdx >= 0 && previewIdx < visible.length - 1 ? visible[previewIdx + 1] : null;
  const toPreview = (en: ExEntry): QuickPreviewTarget => ({
    name: en.name,
    path: en.path,
    kind: en.kind,
    ext: en.ext,
    size: en.size,
  });

  return (
    <>
      {!embedded && <ExTitlebar title={title} />}
      {preview && (
        <QuickPreview
          target={preview}
          onClose={() => setPreview(null)}
          onPrev={prevEntry ? () => setPreview(toPreview(prevEntry)) : undefined}
          onNext={nextEntry ? () => setPreview(toPreview(nextEntry)) : undefined}
        />
      )}
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
            {/* Z-31：网络驱动器（断连置灰并如实标注；SMB 路径仍拒绝写入） */}
            {netDrives.length > 0 && (
              <>
                <p className="ex-side-head">{t("exNetDrives")}</p>
                {netDrives.map((d) => (
                  <button
                    key={d.letter}
                    type="button"
                    className={`ex-side-btn${currentPath === d.path ? " active" : ""}${d.available ? "" : " ex-side-offline"}`}
                    title={d.available ? d.unc ?? d.path : t("exNetOffline")}
                    onClick={() => nav(d.path)}
                  >
                    <Network size={15} strokeWidth={1.7} />
                    <span>
                      {d.kind === "unc" ? pathTail(d.path) : `${d.letter}:`} {!d.available && `· ${t("exNetOffline")}`}
                    </span>
                  </button>
                ))}
              </>
            )}
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
              {/* AI-10：数据安全中心入口（独立系统窗口） */}
              <button
                type="button"
                className="ex-tool-btn"
                aria-label={t("exOpenDataVault")}
                title={t("exOpenDataVault")}
                onClick={() => {
                  void ipc.openDatavault().catch(() => {});
                }}
              >
                <ShieldCheck size={15} />
              </button>
              {/* U-16：双栏开关 */}
              <button
                type="button"
                className={`ex-tool-btn${dual ? " on" : ""}`}
                aria-pressed={dual}
                aria-label={t("exDualPane")}
                title={t("exDualPane")}
                onClick={() => setDual((d) => !d)}
              >
                <Columns3 size={15} />
              </button>
              <div className="ex-view-switch" role="group" aria-label={t("exViewMode")}>
                <button
                  type="button"
                  className={`ex-view-btn${view === "list" ? " on" : ""}`}
                  aria-label={t("viewList")}
                  title={`${t("viewList")} (Ctrl+滚轮)`}
                  onClick={() => changeView("list")}
                >
                  <List size={14} />
                </button>
                <button
                  type="button"
                  className={`ex-view-btn${view === "icons" ? " on" : ""}`}
                  aria-label={t("viewIcons")}
                  title={t("viewIcons")}
                  onClick={() => changeView("icons")}
                >
                  <LayoutGrid size={14} />
                </button>
                <button
                  type="button"
                  className={`ex-view-btn${view === "thumbs" ? " on" : ""}`}
                  aria-label={t("viewThumbs")}
                  title={t("viewThumbs")}
                  onClick={() => changeView("thumbs")}
                >
                  <ImageIcon size={14} />
                </button>
                {/* U-16：分栏视图 */}
                <button
                  type="button"
                  className={`ex-view-btn${view === "column" ? " on" : ""}`}
                  aria-label={t("viewColumn")}
                  title={t("viewColumn")}
                  onClick={() => changeView("column")}
                >
                  <Columns3 size={14} />
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

            {/* 地址栏：面包屑 ↔ 编辑（F4/Ctrl+L；粘贴路径 Enter 跳转；V-39 模糊建议） */}
            <div className="ex-crumbs" aria-label="breadcrumb">
              {addrEdit !== null ? (
                <div className="ex-addr-edit-wrap">
                  <input
                    ref={addrRef}
                    className="ex-addr-input"
                    value={addrEdit}
                    autoFocus
                    spellCheck={false}
                    onChange={(e) => {
                      setAddrEdit(e.target.value);
                      setAddrSugIdx(-1);
                    }}
                    onBlur={() => setAddrEdit(null)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        // V-39：↑↓ 选中建议时 Enter 跳到建议路径，否则按输入内容跳转
                        const sug = addrSugIdx >= 0 ? addrSuggestions[addrSugIdx] : undefined;
                        const p = (sug ?? addrEdit).trim();
                        if (p) nav(p);
                        else setAddrEdit(null);
                      } else if (e.key === "Escape") {
                        setAddrEdit(null);
                      } else if (e.key === "ArrowDown" && addrSuggestions.length > 0) {
                        e.preventDefault();
                        setAddrSugIdx((i) => Math.min(i + 1, addrSuggestions.length - 1));
                      } else if (e.key === "ArrowUp" && addrSuggestions.length > 0) {
                        e.preventDefault();
                        setAddrSugIdx((i) => Math.max(i - 1, -1));
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
                  {/* V-39：模糊跳转建议（收藏/历史/盘符/网络驱动器/环境目录） */}
                  {addrSuggestions.length > 0 && (
                    <div className="ex-addr-sug" role="listbox" aria-label={t("exFuzzyHint")}>
                      {addrSuggestions.map((p, i) => (
                        <button
                          key={p}
                          type="button"
                          role="option"
                          aria-selected={i === addrSugIdx}
                          className={`ex-addr-sug-item${i === addrSugIdx ? " active" : ""}`}
                          onMouseDown={(ev) => {
                            // preventDefault 保持焦点，先跳转再收起
                            ev.preventDefault();
                            nav(p);
                            setAddrEdit(null);
                          }}
                        >
                          {p}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
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

            {/* M-24：书签条（收藏夹快捷条；右键移除） */}
            {favs.length > 0 && (
              <div className="ex-bookmarks" role="toolbar" aria-label={t("favorites")}>
                {favs.map((f) => (
                  <button
                    key={f}
                    type="button"
                    className={`ex-bm-chip${currentPath === f ? " active" : ""}`}
                    title={f}
                    onClick={() => nav(f)}
                    onContextMenu={(ev) => {
                      ev.preventDefault();
                      openContextMenu(ev.clientX, ev.clientY, [
                        { label: t("exFavRemove"), icon: <Star size={13} />, onClick: () => toggleFav(f) },
                      ]);
                    }}
                  >
                    <Star size={12} strokeWidth={1.7} />
                    {pathTail(f)}
                  </button>
                ))}
              </div>
            )}

            <div className="ex-main-row">
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
                    rows.map((e) => {
                      const cut = exClipboard?.paths.includes(e.path) && exClipboard.cut;
                      void clipboardTick;
                      return (
                        <div
                          key={e.path}
                          className={`ex-row${selected === e.path || selSet.has(e.path) ? " selected" : ""}${e.hidden ? " hidden-entry" : ""}${cut ? " cut" : ""}`}
                          onClick={(ev) => clickRow(ev, e)}
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
                <div className={`ex-grid${view === "column" ? " ex-cols" : ""}`}>
                  {rows.map((e) => {
                    const cut = exClipboard?.paths.includes(e.path) && exClipboard.cut;
                    void clipboardTick;
                    const isImg = e.kind === "file" && e.ext != null && THUMB_EXTS.has(e.ext);
                    const isVid = e.kind === "file" && e.ext != null && VIDEO_EXTS.has(e.ext);
                    return (
                      <div
                        key={e.path}
                        className={`ex-tile${selected === e.path || selSet.has(e.path) ? " selected" : ""}${cut ? " cut" : ""}`}
                        onClick={(ev) => clickRow(ev, e)}
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
                        <span className="ex-tile-name" title={e.name}>{hlName(e.name)}</span>
                      </div>
                    );
                  })}
                </div>
              )}
              </div>

              {/* U-16：双栏第二面板（独立导航；接收主面板拖入 = 复制/Shift 移动） */}
              {dual && (
                <div className="ex-pane2" aria-label={t("exDualPane")}>
                  <div className="ex-pane2-head">
                    <button
                      type="button"
                      className="ex-tool-btn"
                      aria-label={t("navUp")}
                      onClick={() => {
                        const parent = pane2.listing?.parent;
                        if (parent) setPane2({ path: parent, listing: null });
                      }}
                    >
                      <ArrowUp size={13} />
                    </button>
                    <span className="ex-pane2-path" title={pane2.path}>{pane2.path ? pathTail(pane2.path) : t("thisPC")}</span>
                    <button
                      type="button"
                      className="ex-tool-btn"
                      aria-label={t("exRefresh")}
                      onClick={() => setPane2((p) => ({ ...p, listing: null }))}
                    >
                      <RefreshCw size={13} />
                    </button>
                  </div>
                  <div
                    className="ex-pane2-list"
                    onDragOver={(ev) => {
                      if (ev.dataTransfer.types.includes("application/x-variable-ex")) ev.preventDefault();
                    }}
                    onDrop={(ev) => {
                      ev.preventDefault();
                      if (!pane2.path) return;
                      try {
                        const paths = JSON.parse(ev.dataTransfer.getData("application/x-variable-ex")) as string[];
                        if (Array.isArray(paths) && paths.length > 0) {
                          // Shift = 移动；默认复制（跨面板最安全语义）
                          void pasteInto(pane2.path, { paths, cut: ev.shiftKey });
                        }
                      } catch {
                        /* 非 Variable 拖拽忽略 */
                      }
                    }}
                  >
                    {(pane2.listing?.entries ?? []).map((e) => (
                      <div
                        key={e.path}
                        className="ex-pane2-row"
                        title={e.path}
                        onDoubleClick={() => {
                          if (e.kind === "dir") setPane2({ path: e.path, listing: null });
                        }}
                      >
                        {e.kind === "dir" ? <Folder size={14} /> : <FileIcon size={14} className="dim" />}
                        <span className="ex-pane2-name">{e.name}</span>
                        <span className="dim small">{e.kind === "dir" ? "—" : fmtSize(e.size)}</span>
                      </div>
                    ))}
                    {pane2.listing === null && <div className="ex-hint dim" aria-busy="true" />}
                    {pane2.listing !== null && (pane2.listing?.entries ?? []).length === 0 && (
                      <div className="ex-hint dim">{t("emptyDirHint")}</div>
                    )}
                  </div>
                </div>
              )}
            </div>

            {/* AI-10 V-34：拖拽计数徽标（fixed 定位跟随光标） */}
            {dragBadge && (
              <div className="ex-drag-badge" role="status" aria-label={t("items", { n: dragBadge.n })}>
                {dragBadge.n}
              </div>
            )}

            <div className="ex-status">
              {statusText}
              {selSummary && <span className="ex-status-sel">{selSummary}</span>}
              {/* U-16：分页虚拟化（>500 项时切片渲染） */}
              {paged.pages > 1 && (
                <span className="ex-pager">
                  <button
                    type="button"
                    className="ex-tool-btn"
                    disabled={paged.page === 0}
                    aria-label={t("exPrevPage")}
                    onClick={() => setPage((p) => Math.max(0, p - 1))}
                  >
                    ‹
                  </button>
                  <span className="dim small">
                    {paged.page * PAGE_SIZE + 1}–{Math.min((paged.page + 1) * PAGE_SIZE, visible.length)} / {visible.length}
                  </span>
                  <button
                    type="button"
                    className="ex-tool-btn"
                    disabled={paged.page >= paged.pages - 1}
                    aria-label={t("exNextPage")}
                    onClick={() => setPage((p) => p + 1)}
                  >
                    ›
                  </button>
                </span>
              )}
              {/* V-38：预览锁定开关（预览打开时可见） */}
              {preview && (
                <button
                  type="button"
                  className={`ex-pin-btn${previewPin ? " on" : ""}`}
                  aria-pressed={previewPin}
                  aria-label={t("exPreviewPin")}
                  title={`${t("exPreviewPin")} (Ctrl+P)`}
                  onClick={() => setPreviewPin((p) => !p)}
                >
                  {previewPin ? <PinOff size={13} /> : <Pin size={13} />}
                </button>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* AI-10 V-35：两文件属性对比对话框 */}
      {cmp && (
        <div
          className="ex-dlg-overlay"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) setCmp(null);
          }}
        >
          <div className="ex-dlg ex-cmp" role="dialog" aria-label={t("exCompareProps")}>
            <p className="ex-dlg-title">{t("exCompareProps")}</p>
            <table className="ex-cmp-table">
              <tbody>
                <tr><th>{t("colName")}</th><td>{cmp[0].name}</td><td>{cmp[1].name}</td></tr>
                <tr><th>{t("colType")}</th><td>{typeLabel(cmp[0])}</td><td>{typeLabel(cmp[1])}</td></tr>
                <tr><th>{t("colSize")}</th><td>{cmp[0].kind === "dir" ? "—" : fmtSize(cmp[0].size)}</td><td>{cmp[1].kind === "dir" ? "—" : fmtSize(cmp[1].size)}</td></tr>
                <tr><th>{t("colModified")}</th><td>{cmp[0].updatedAt ? new Date(cmp[0].updatedAt).toLocaleString() : "—"}</td><td>{cmp[1].updatedAt ? new Date(cmp[1].updatedAt).toLocaleString() : "—"}</td></tr>
                <tr><th>{t("colCreated")}</th><td>{cmp[0].createdAt ? new Date(cmp[0].createdAt).toLocaleString() : "—"}</td><td>{cmp[1].createdAt ? new Date(cmp[1].createdAt).toLocaleString() : "—"}</td></tr>
                <tr className="ex-cmp-path"><th>{t("exCopyPath")}</th><td colSpan={2}>{cmp[0].path}<br />{cmp[1].path}</td></tr>
              </tbody>
            </table>
            <div className="ex-dlg-actions">
              <button type="button" className="dim" onClick={() => setCmp(null)}>
                {t("close")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* AI-10 V-40：压缩包提取向导（zip；预览条目 → 全部解包到当前目录） */}
      {extract && (
        <div
          className="ex-dlg-overlay"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget && !extractBusy) setExtract(null);
          }}
        >
          <div className="ex-dlg ex-extract" role="dialog" aria-label={t("exExtractTitle")}>
            <p className="ex-dlg-title">{t("exExtractTitle")}</p>
            <p className="ex-dlg-body">
              {extract.archive.name} · {t("items", { n: extract.entries.length })}
            </p>
            <ul className="ex-extract-list">
              {extract.entries.slice(0, 200).map((en) => (
                <li key={en.innerPath}>
                  <span className="ex-extract-name" title={en.innerPath}>{en.innerPath}</span>
                  <span className="dim small">{en.isDir ? "—" : fmtSize(en.size)}</span>
                </li>
              ))}
              {extract.entries.length > 200 && (
                <li className="dim small">… +{extract.entries.length - 200}</li>
              )}
            </ul>
            <div className="ex-dlg-actions">
              <button type="button" disabled={extractBusy} onClick={() => void runExtractAll()}>
                {t("exExtractAll")}
              </button>
              <button type="button" className="dim" disabled={extractBusy} onClick={() => setExtract(null)}>
                {t("cancel")}
              </button>
            </div>
          </div>
        </div>
      )}

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

      {/* M-25：占用查看对话框（只读，无强拆按钮；空 = 系统未披露占用者） */}
      {lockView && (
        <div
          className="ex-dlg-overlay"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) setLockView(null);
          }}
        >
          <div className="ex-dlg" role="dialog" aria-label={t("exWhoLocks")}>
            <p className="ex-dlg-title">{t("exWhoLocks")}</p>
            <p className="ex-dlg-body ex-lock-path">{lockView.path}</p>
            {lockView.holders.length === 0 ? (
              <p className="dim">{t("exLocksNone")}</p>
            ) : (
              <ul className="ex-lock-list">
                {lockView.holders.map((h) => (
                  <li key={h.pid}>
                    <span className="ex-lock-name">{h.name}</span>
                    <span className="dim small">PID {h.pid}{h.title ? ` · ${h.title}` : ""}</span>
                  </li>
                ))}
              </ul>
            )}
            <div className="ex-dlg-actions">
              <button type="button" className="dim" onClick={() => setLockView(null)}>
                {t("close")}
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
