import { useEffect, useRef, useState } from "react";
import {
  Activity, AppWindow, Calculator, CalendarClock, Camera, Clock, ClipboardList, Files, Fingerprint, FolderOpen,
  HardDrive,
  Flame, FolderMinus, Info, Lock, LogOut, Moon, PackagePlus, Pencil, Pin, PinOff, Power, Printer, RotateCcw,
  ShieldCheck, Settings as SettingsIcon, Search, Smile, StickyNote, Trash2, X, ZoomIn, ArrowLeftRight,
} from "lucide-react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { matchPinyin } from "../../lib/pinyin";
import type { AppMode } from "../../state/uiStore";
import { pushToast } from "../../state/uiStore";
import { askConfirm, askPrompt } from "../../components/Modal";
import { openContextMenu, type MenuItem } from "../../components/ContextMenu";
import { desktopIconDefs, desktopAppLabel } from "../desktop-icons/DesktopIcons";
import {
  launchThirdApp, openLauncherManager, reloadThirdApps, toggleTaskbarPin, useTaskbarPins, useThirdApps,
} from "../launcher/thirdApps";
import { useUninstalledOfficial } from "../launcher/official";
import { openVwmApp, openVwmSystem, vwmStore, VWM_TOOLS } from "../windows/vwm";
import { pushRecent, useRecent } from "./recent";
import { bumpUsage, subscribeUsage, usageCount } from "./usage";
import { HIGH_FREQ_TOP_N, highFreqEnabled, recentlyAdded, setHighFreqEnabled, topUsedItems } from "./groups";
import {
  createFolder, disbandFolder, folderAddItem, folderGridId, folderIdOfGrid, folderRemoveItem,
  getFolders, isFolderGridId, mergeFolderItems, renameFolder, saveFolders, useFolders, type Folder,
} from "./folders";
import { INDEX_THRESHOLD, letterGroups } from "./indexBar";
import { startLabels } from "./labels";
import { StartPropsPanel, type StartPropsInfo } from "./StartPropsPanel";

/**
 * 开始菜单（M3 → 批次E，桌面环境 L1，Windows 11 习惯 + V 品牌入口）：
 * - 顶部搜索框 → 打开全局搜索（复用 SearchOverlay，零重复实现）
 * - 最近使用（批次E，规格 4.6.2）：纯本地 localStorage 记录，最近 8 项
 * - 已固定网格：四款独立软件 + 系统入口 + 第三方，支持拖拽重排（localStorage 持久化）
 *   （批次C：已卸载的预装软件不再显示；卸载入口在「软件管理 → 已安装软件」）
 * - 底栏：本机用户名（批次E）+ 品牌标识 + 电源完整菜单（锁定/注销/重启/关机/退出）
 * - Esc / 点击菜单外关闭
 *
 * 化境 V-11…V-14（车道 S）增量：
 * - V-11 字母索引条：应用数 >30 出现，A–Z + 拼音首字母（initialsOf）归组，
 *   点击/拖动跳转 + 中央大字提示，键盘可达（Tab 聚焦后字母键直达）。
 *   取舍注明：索引模式下网格临时按字母分组渲染（用户手动 order 数据原样保留、
 *   不被覆盖——因此索引模式下拖拽重排暂缓，文件夹合并不受影响）。
 * - V-12 最近添加/高频分组：顶部两个自动分组；高频默认关（localStorage 开关，
 *   正式开关位在设置页属 AI05/16 领地，故就近提供小按钮并如实注明）；分组可整体隐藏（会话级）。
 * - V-13 固定文件夹：拖拽到目标图标中央 60% 区域合并（≤24 项如实提示），
 *   文件夹图标 = 前 4 项 2×2 合成，点击展开浮层，可重命名/拖出解散/右键解散。
 * - V-14 右键高级操作：固定到开始（已固定）/ 固定到任务栏 / 卸载 / 打开文件位置 /
 *   管理员运行 / 属性 —— 官方软件无对应能力处诚实提示（详见 openAppMenu 注释）。
 */

const ORDER_KEY = "variable:start:order:v1";

/** F-2 工具集：标题词典 key 与图标（开始菜单/VWM 共用语义）。
 *  注：键集按 VwmToolApp 索引，另含其他 AI 工具组的预置词条（运行时按 VWM_TOOLS 过滤），故用宽松键型。 */
const TOOL_DEFS: Record<string, { key: string; icon: React.ReactElement }> = {
  calc: { key: "toolCalc", icon: <Calculator size={22} strokeWidth={1.6} /> },
  notes: { key: "toolNotes", icon: <StickyNote size={22} strokeWidth={1.6} /> },
  calendar: { key: "toolCalendar", icon: <CalendarClock size={22} strokeWidth={1.6} /> },
  snapshot: { key: "toolSnapshot", icon: <Camera size={22} strokeWidth={1.6} /> },
  clipboard: { key: "toolClipboard", icon: <ClipboardList size={22} strokeWidth={1.6} /> },
  // AI-09 文件操作四工具
  rename: { key: "toolRename", icon: <Pencil size={22} strokeWidth={1.6} /> },
  dupe: { key: "toolDupe", icon: <Files size={22} strokeWidth={1.6} /> },
  space: { key: "toolSpace", icon: <HardDrive size={22} strokeWidth={1.6} /> },
  checksum: { key: "toolChecksum", icon: <Fingerprint size={22} strokeWidth={1.6} /> },
  // AI-08 基础工具组六件（Z-22/Z-24/Z-25/Z-26/Z-27/V-98；Z-23 天气在任务栏、Z-28 运行框走热键）
  clockhub: { key: "toolClockhub", icon: <Clock size={22} strokeWidth={1.6} /> },
  emoji: { key: "toolEmoji", icon: <Smile size={22} strokeWidth={1.6} /> },
  magnifier: { key: "toolMagnifier", icon: <ZoomIn size={22} strokeWidth={1.6} /> },
  convert: { key: "toolConvert", icon: <ArrowLeftRight size={22} strokeWidth={1.6} /> },
  sysinfo: { key: "toolSysinfo", icon: <Info size={22} strokeWidth={1.6} /> },
  printqueue: { key: "toolPrintqueue", icon: <Printer size={22} strokeWidth={1.6} /> },
};

function loadOrder(): string[] {
  try {
    const raw = localStorage.getItem(ORDER_KEY);
    return raw ? (JSON.parse(raw) as string[]) : [];
  } catch {
    return [];
  }
}

function saveOrder(ids: string[]): void {
  try {
    localStorage.setItem(ORDER_KEY, JSON.stringify(ids));
  } catch {
    /* storage full — 忽略 */
  }
}

export function StartMenu(props: {
  open: boolean;
  onClose: () => void;
  onOpenApp: (app: AppMode) => void;
  onOpenSettings: () => void;
  onOpenLauncher: () => void;
  onOpenSearch: () => void;
  onExit: () => void;
}): React.ReactElement | null {
  const { t, lang } = useI18n();
  const L = startLabels(lang);
  const uninstalled = useUninstalledOfficial();
  const thirds = useThirdApps();
  const recent = useRecent();
  const pins = useTaskbarPins();
  const folders = useFolders();
  const defs = desktopIconDefs().filter((d) => uninstalled[d.app] === undefined);
  const [userName, setUserName] = useState<string>("");
  // 批次E-8：开始菜单搜索框（拼音/首字母过滤，Enter 转全局搜索）
  const [q, setQ] = useState<string>("");
  const [powerOpen, setPowerOpen] = useState(false);
  // AI-03 V-20：开机时长（电源菜单悬停显示；只读 sysBrief）
  const [uptimeSecs, setUptimeSecs] = useState<number | null>(null);
  useEffect(() => {
    if (!powerOpen) return;
    void ipc.sysBrief().then((b) => setUptimeSecs((b as { uptimeSecs?: number }).uptimeSecs ?? null)).catch(() => setUptimeSecs(null));
  }, [powerOpen]);
  const [order, setOrder] = useState<string[]>(() => loadOrder());
  const [dragOverId, setDragOverId] = useState<string | null>(null);
  const dragId = useRef<string | null>(null);
  // ---- 化境 V-11…V-14 组件态 ----
  const [, setUsageTick] = useState(0); // 高频分组响应本地计数变化
  const [hfreqOn, setHfreqOn] = useState<boolean>(() => highFreqEnabled());
  // 分组隐藏 = 会话级（规格 V-12 只要求「可整体隐藏」；持久化开关位在设置页属 AI05/16 领地）
  const [grpHidden, setGrpHidden] = useState<{ recentAdded: boolean; highFreq: boolean }>({ recentAdded: false, highFreq: false });
  const [openFolderId, setOpenFolderId] = useState<string | null>(null);
  const [propsInfo, setPropsInfo] = useState<StartPropsInfo | null>(null);
  const [idxFlash, setIdxFlash] = useState<string | null>(null);
  const [dragMode, setDragMode] = useState<"reorder" | "merge">("reorder");
  const idxFlashTimer = useRef<number | null>(null);
  const barDragging = useRef(false);
  const menuRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => subscribeUsage(() => setUsageTick((v) => v + 1)), []);

  useEffect(() => {
    if (!props.open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.stopPropagation();
        props.onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [props.open, props]);

  // 批次E：本机用户名（打开时读一次即可）
  useEffect(() => {
    if (!props.open || userName) return;
    void ipc
      .sysUser()
      .then(setUserName)
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open]);

  // V-13/V-11：菜单关闭 = 新会话 —— 复位文件夹浮层 / 索引大字 / 拖拽态
  useEffect(() => {
    if (props.open) return;
    setOpenFolderId(null);
    setIdxFlash(null);
    setDragOverId(null);
    setDragMode("reorder");
    barDragging.current = false;
  }, [props.open]);

  // 文件夹浮层外点击关闭（浮层内/文件夹按钮除外）
  useEffect(() => {
    if (!openFolderId) return;
    const onDown = (e: MouseEvent): void => {
      const el = e.target as HTMLElement | null;
      if (el?.closest?.(".start-folder-flyout") || el?.closest?.("[data-folder-btn='1']")) return;
      setOpenFolderId(null);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [openFolderId]);

  // 诚实清理：第三方登记被删除后，手动排序与文件夹成员不留幽灵占位
  const tpIdsKey = thirds.map((a) => a.id).join("|");
  useEffect(() => {
    if (!props.open) return;
    const valid = new Set<string>([
      ...desktopIconDefs().map((d) => `app-${d.app}`),
      "sys-explorer", "sys-recycle", "sys-taskman", "sys-launcher", "sys-settings",
      ...VWM_TOOLS.map((tool) => `tool-${tool}`),
      ...thirds.map((a) => `tp-${a.id}`),
    ]);
    let changed = false;
    const nextFolders = getFolders()
      .map((f) => {
        const rest = f.items.filter((id) => valid.has(id));
        if (rest.length === f.items.length) return f;
        changed = true;
        return { ...f, items: rest };
      })
      .filter((f) => {
        if (f.items.length > 0) return true;
        changed = true;
        return false; // 清理后为空的文件夹自动解散
      });
    if (changed) saveFolders(nextFolders);
    const orderNow = loadOrder();
    const orderNext = orderNow.filter(
      (id) => valid.has(id) || (isFolderGridId(id) && nextFolders.some((f) => folderGridId(f.id) === id)),
    );
    if (orderNext.length !== orderNow.length) {
      setOrder(orderNext);
      saveOrder(orderNext);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.open, tpIdsKey]);

  if (!props.open) return null;

  // ---- 批次E：固定网格拖拽重排 ----
  interface GridItem {
    id: string;
    label: string;
    hue: string;
    icon: React.ReactNode;
    onClick: () => void;
    title?: string;
    custom?: React.ReactNode;
    // 化境 V-11…V-14：右键/分组/文件夹所需元数据
    kind: "app" | "sys" | "tool" | "tp" | "folder";
    tpId?: string;    // 第三方登记 id（V-14 卸载/管理员运行/任务栏固定）
    path?: string;    // 第三方路径（V-14 打开文件位置）
    addedAt?: number; // 第三方登记时间（V-12 最近添加）
    folderId?: string; // 文件夹条目（V-13）
  }
  const items: GridItem[] = [
    ...defs.map((d) => {
      const Icon = d.icon;
      return {
        id: `app-${d.app}`,
        label: desktopAppLabel(d.app),
        hue: String(d.hue),
        kind: "app" as const,
        icon: <Icon size={22} strokeWidth={1.6} />,
        onClick: () => {
          pushRecent("app", d.app, desktopAppLabel(d.app));
          props.onOpenApp(d.app);
        },
      };
    }),
    {
      id: "sys-explorer",
      label: t("explorerWin"),
      hue: "210",
      kind: "sys" as const,
      icon: <FolderOpen size={22} strokeWidth={1.6} />,
      onClick: () => {
        pushRecent("sys", "explorer", t("explorerWin"));
        props.onClose();
        openVwmSystem("explorer");
      },
      title: t("explorerWin"),
    },
    ...thirds.map((a) => ({
      id: `tp-${a.id}`,
      label: a.name,
      hue: "158",
      kind: "tp" as const,
      tpId: a.id,
      path: a.path,
      addedAt: a.addedAt,
      icon: a.icon ? (
        <img src={a.icon} alt="" className="tb-custom-icon" />
      ) : (
        <AppWindow size={22} strokeWidth={1.6} />
      ),
      onClick: () => {
        pushRecent("tp", a.id, a.name);
        props.onClose();
        void launchThirdApp(a.id, a.name);
      },
      title: a.path,
    })),
    {
      id: "sys-recycle",
      label: t("recycleBin"),
      hue: "0",
      kind: "sys" as const,
      icon: <Trash2 size={22} strokeWidth={1.6} />,
      onClick: () => {
        pushRecent("sys", "recycle", t("recycleBin"));
        props.onClose();
        openVwmSystem("recycle");
      },
      title: t("recycleBin"),
    },
    {
      id: "sys-taskman",
      label: t("tmTitle"),
      hue: "16",
      kind: "sys" as const,
      icon: <Activity size={22} strokeWidth={1.6} />,
      onClick: () => {
        pushRecent("sys", "taskman", t("tmTitle"));
        props.onClose();
        openVwmSystem("taskman");
      },
      title: t("tmTitle"),
    },
    // F-2 实用工具集（VWM 虚拟窗口应用：贴靠/保活/多开语义与四软件一致）
    ...VWM_TOOLS.map((tool) => ({
      id: `tool-${tool}`,
      label: t(TOOL_DEFS[tool]!.key),
      hue: "268",
      kind: "tool" as const,
      icon: TOOL_DEFS[tool]!.icon,
      onClick: () => {
        pushRecent("sys", `tool-${tool}`, t(TOOL_DEFS[tool]!.key));
        props.onClose();
        openVwmApp(tool);
      },
      title: t(TOOL_DEFS[tool]!.key),
    })),
    {
      id: "sys-launcher",
      label: t("launcherTitle"),
      hue: "158",
      kind: "sys" as const,
      icon: <PackagePlus size={22} strokeWidth={1.6} />,
      onClick: props.onOpenLauncher,
      title: t("launcherTitle"),
    },
    {
      id: "sys-settings",
      label: t("settings"),
      hue: "210",
      kind: "sys" as const,
      icon: <SettingsIcon size={22} strokeWidth={1.6} />,
      onClick: props.onOpenSettings,
      title: t("settings"),
    },
  ];

  // ---- V-13 文件夹条目：作为独立条目参与 order 排序；图标 = 前 4 项 2×2 合成 ----
  const itemById0 = new Map(items.map((i) => [i.id, i] as const));
  const folderIcon = (f: Folder): React.ReactNode => {
    const cells: React.ReactNode[] = f.items
      .map((id) => itemById0.get(id)?.icon)
      .filter((n): n is React.ReactNode => Boolean(n))
      .slice(0, 4);
    while (cells.length < 4) cells.push(<AppWindow size={14} strokeWidth={1.6} />);
    return (
      <span className="start-folder-mini">
        {cells.map((n, i) => (
          <span key={i} className="start-folder-mini-cell">{n}</span>
        ))}
      </span>
    );
  };
  const folderItems: GridItem[] = folders.map((f) => ({
    id: folderGridId(f.id),
    label: f.name,
    hue: "32",
    kind: "folder" as const,
    folderId: f.id,
    icon: folderIcon(f),
    onClick: () => setOpenFolderId(openFolderId === f.id ? null : f.id),
    title: f.name,
  }));
  const allItems = [...items, ...folderItems];

  // V-12 使用统计：本地计数（usage.ts），只在开始菜单点击应用时累加；
  // 文件夹容器本身不是应用，不计入。
  for (const it of allItems) {
    if (it.kind === "folder") continue;
    const orig = it.onClick;
    it.onClick = () => {
      bumpUsage(it.id);
      orig();
    };
  }

  const orderIdx = new Map(order.map((id, i) => [id, i]));
  allItems.sort((a, b) => (orderIdx.get(a.id) ?? 1e9) - (orderIdx.get(b.id) ?? 1e9));
  const itemById = new Map(allItems.map((i) => [i.id, i] as const));

  // 批次E-8（规格 N5）：拼音/首字母即时过滤 —— "sz" 命中"设置"、"swdt" 命中"思维导图"
  const filtered = q.trim()
    ? allItems.filter((it) => matchPinyin(it.label, q) || matchPinyin(it.title ?? it.label, q))
    : allItems;
  const filteredRecent = q.trim() ? recent.filter((r) => matchPinyin(r.name, q)) : recent;

  // ---- V-11 索引模式：应用数 >30 才出现（文件夹不计入应用数） ----
  const appCount = allItems.filter((it) => it.kind !== "folder").length;
  const indexMode = !q.trim() && appCount > INDEX_THRESHOLD;
  const groups = indexMode ? letterGroups(filtered) : [];

  // ---- V-12 自动分组数据（搜索时隐藏，避免噪声） ----
  const recentAddedItems =
    !q.trim() && !grpHidden.recentAdded
      ? recentlyAdded(
          thirds.map((a) => ({ id: a.id, name: a.name, addedAt: a.addedAt })),
          recent,
          Date.now(),
        ).filter((e) => itemById.has(e.gridId))
      : [];
  const topList =
    !q.trim() && !grpHidden.highFreq && hfreqOn
      ? topUsedItems(allItems, HIGH_FREQ_TOP_N).filter(({ item }) => item.kind !== "folder")
      : [];

  // ---- V-13：order 维护工具 ----
  const removeFromOrder = (gridId: string): void => {
    const next = loadOrder().filter((id) => id !== gridId);
    setOrder(next);
    saveOrder(next);
  };
  /** 解散并把成员放回 order 原文件夹位置（Windows 习惯：原位归还）。返回成员数。 */
  const disbandAndRestore = (folderId: string): number => {
    const fid = folderGridId(folderId);
    const freed = disbandFolder(folderId);
    const cur = loadOrder();
    const at = cur.indexOf(fid);
    const next = cur.filter((id) => id !== fid && !freed.includes(id));
    if (at >= 0) next.splice(Math.min(at, next.length), 0, ...freed);
    else next.push(...freed);
    setOrder(next);
    saveOrder(next);
    if (openFolderId === folderId) setOpenFolderId(null);
    return freed.length;
  };

  /** V-13 合并：应用→文件夹 / 文件夹→文件夹 / 应用→应用（60% 重叠判定见 onDragOver）。 */
  const mergeInto = (fromId: string, targetId: string): void => {
    if (fromId === targetId) return;
    const targetFolderId = folderIdOfGrid(targetId);
    const fromFolderId = folderIdOfGrid(fromId);
    if (targetFolderId && fromFolderId) {
      const target = getFolders().find((f) => f.id === targetFolderId);
      const from = getFolders().find((f) => f.id === fromFolderId);
      if (!target || !from) return;
      const merged = mergeFolderItems(target.items, from.items);
      if (!merged.ok) {
        pushToast("info", target.name, L.folderMergeFull); // 上限 24 如实提示，绝不静默丢弃
        return;
      }
      saveFolders(getFolders().map((f) => (f.id === targetFolderId ? { ...f, items: merged.items } : f)));
      disbandFolder(fromFolderId); // 成员已并入目标，移除空壳
      removeFromOrder(fromId);
      return;
    }
    if (targetFolderId) {
      const r = folderAddItem(targetFolderId, fromId);
      if (r === "full") {
        pushToast("info", getFolders().find((f) => f.id === targetFolderId)?.name ?? "", L.folderFull);
        return;
      }
      if (r === "ok") removeFromOrder(fromId);
      return;
    }
    if (fromFolderId) {
      const r = folderAddItem(fromFolderId, targetId);
      if (r === "full") {
        pushToast("info", getFolders().find((f) => f.id === fromFolderId)?.name ?? "", L.folderFull);
        return;
      }
      if (r === "ok") removeFromOrder(targetId);
      return;
    }
    // 应用 → 应用：新建文件夹（默认名，浮层内可重命名；不做智能归类——绝不替用户决定）
    createFolder(L.folderNew, [targetId, fromId]);
    removeFromOrder(targetId);
    removeFromOrder(fromId);
  };

  const onDropTo = (targetId: string): void => {
    const from = dragId.current;
    dragId.current = null;
    if (!from || from === targetId) return;
    if (dragMode === "merge") {
      mergeInto(from, targetId);
      setDragMode("reorder");
      return;
    }
    // V-11 取舍：索引模式下列表临时按字母分组渲染，此时写回 order 会把用户手动
    // 排序覆盖成字母序 —— 索引模式放弃重排（合并入文件夹不受影响），order 原样保留。
    if (indexMode) return;
    const ids = allItems.map((i) => i.id);
    const fi = ids.indexOf(from);
    const ti = ids.indexOf(targetId);
    if (fi < 0 || ti < 0) return;
    ids.splice(ti, 0, ids.splice(fi, 1)[0] as string);
    setOrder(ids);
    saveOrder(ids);
  };

  // V-13 拖出解散：文件夹浮层内图标拖到网格空白处 = 移出文件夹（按钮级 drop 已 stopPropagation）
  const gridDropProps = {
    onDragOver: (e: React.DragEvent<HTMLDivElement>): void => {
      if (openFolderId && dragId.current) e.preventDefault();
    },
    onDrop: (e: React.DragEvent<HTMLDivElement>): void => {
      if (!openFolderId || !dragId.current) return;
      const onBtn = (e.target as HTMLElement).closest?.(".start-app");
      if (onBtn) return;
      const memberId = dragId.current;
      dragId.current = null;
      folderRemoveItem(openFolderId, memberId);
    },
  };

  // ---- V-11：跳转到字母分组 + 中央大字提示（Win10 同款） ----
  const jumpTo = (letter: string): void => {
    setIdxFlash(letter);
    if (idxFlashTimer.current) window.clearTimeout(idxFlashTimer.current);
    idxFlashTimer.current = window.setTimeout(() => setIdxFlash(null), 600);
    menuRef.current?.querySelector<HTMLElement>(`.start-alpha-group[data-letter="${letter}"]`)
      ?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  };

  // ---- V-13 文件夹浮层动作 ----
  const doRenameFolder = async (fid: string): Promise<void> => {
    const cur = getFolders().find((f) => f.id === fid);
    const name = await askPrompt({ title: L.folderRenameTitle, initial: cur?.name ?? "" });
    if (name === null) return;
    renameFolder(fid, name);
  };
  const doDisband = (fid: string): void => {
    const n = disbandAndRestore(fid);
    pushToast("info", L.folderDisband, L.folderDisbanded.replace("{n}", String(n)));
  };

  // ---- V-14 第三方移除登记（确认后走既有 tpRemove；软件本体不被卸载——文案如实） ----
  const doRemoveTp = async (it: GridItem): Promise<void> => {
    if (!it.tpId) return;
    const ok = await askConfirm({
      title: t("tpRemove"),
      body: t("tpRemoveBody", { name: it.label }),
      danger: true,
      okLabel: t("tpRemove"),
    });
    if (!ok) return;
    try {
      await ipc.tpRemove(it.tpId);
      removeFromOrder(it.id);
      saveFolders(
        getFolders()
          .map((f) => ({ ...f, items: f.items.filter((m) => m !== it.id) }))
          .filter((f) => f.items.length > 0),
      );
      await reloadThirdApps();
    } catch (e) {
      pushToast("error", it.label, errMessage(e).message);
    }
  };

  const kindKeyOf = (it: GridItem): string =>
    it.kind === "folder" ? "kindFolder" : it.kind === "app" ? "kindApp" : it.kind === "tp" ? "kindTp" : it.kind === "tool" ? "kindTool" : "kindSys";

  /**
   * V-14 右键高级操作（六项）。诚实边界就地注明：
   * - 固定到开始：开始菜单即本网格 → 恒为「已固定」（禁用项如实呈现）。
   * - 固定到任务栏：第三方 = 既有 toggleTaskbarPin（真实能力）；官方无接口 →
   *   点击如实提示「暂未接入」（ipc/taskbar 现无官方固定通道，正式能力见后续版本）。
   * - 卸载：官方 → 既有「软件管理 → 已安装软件」入口；第三方 → 既有 tpRemove
   *   （移除登记语义，确认框文案如实说明软件本体不被删除）。
   * - 打开文件位置：第三方 path → openVwmSystem("explorer", path)；官方无独立路径 → 如实提示。
   * - 管理员运行：第三方 → ipc.tpLaunchAdmin；官方不支持 → 如实提示。
   * - 属性：本机只读信息（StartPropsPanel）。
   */
  const openAppMenu = (x: number, y: number, it: GridItem): void => {
    if (it.kind === "folder" && it.folderId) {
      const fid = it.folderId;
      openContextMenu(x, y, [
        { label: L.folderOpen, icon: <FolderOpen size={13} />, onClick: () => setOpenFolderId(fid) },
        { label: t("rename"), icon: <Pencil size={13} />, onClick: () => void doRenameFolder(fid) },
        { separator: true },
        { label: L.folderDisband, icon: <FolderMinus size={13} />, danger: true, onClick: () => doDisband(fid) },
      ]);
      return;
    }
    const isTp = it.kind === "tp" && Boolean(it.tpId);
    const taskbarPinned = isTp ? pins.includes(it.tpId as string) : false;
    const menu: MenuItem[] = [
      { label: L.ctxPinnedStart, disabled: true },
      isTp
        ? {
            label: taskbarPinned ? t("tbUnpin") : t("tbPin"),
            icon: taskbarPinned ? <PinOff size={13} /> : <Pin size={13} />,
            onClick: () => toggleTaskbarPin(it.tpId as string),
          }
        : {
            label: L.ctxPinTaskbarNA,
            icon: <Pin size={13} />,
            onClick: () => pushToast("info", it.label, L.naTaskbarDetail),
          },
      { separator: true },
      isTp
        ? { label: t("tpRemove"), icon: <Trash2 size={13} />, danger: true, onClick: () => void doRemoveTp(it) }
        : { label: t("uninstallMenu"), icon: <Trash2 size={13} />, onClick: () => { props.onClose(); openLauncherManager("installed"); } },
      {
        label: L.ctxOpenFileLoc,
        icon: <FolderOpen size={13} />,
        onClick: () => {
          if (it.path) {
            props.onClose();
            openVwmSystem("explorer", it.path);
          } else {
            pushToast("info", it.label, L.naFileLocDetail);
          }
        },
      },
      isTp
        ? {
            label: t("runAsAdmin"),
            icon: <ShieldCheck size={13} />,
            onClick: () => {
              void ipc
                .tpLaunchAdmin(it.tpId as string)
                .catch((e) => pushToast("error", it.label, errMessage(e).message));
            },
          }
        : {
            label: t("runAsAdmin"),
            icon: <ShieldCheck size={13} />,
            onClick: () => pushToast("info", it.label, L.naAdminDetail),
          },
      { separator: true },
      {
        label: t("properties"),
        icon: <Info size={13} />,
        onClick: () =>
          setPropsInfo({
            gridId: it.id,
            name: it.label,
            kindKey: kindKeyOf(it),
            path: it.path ?? null,
            addedAt: it.addedAt ?? null,
            usage: usageCount(it.id),
          }),
      },
    ];
    openContextMenu(x, y, menu);
  };

  const renderGridButton = (it: GridItem): React.ReactElement => {
    const isFolder = it.kind === "folder";
    const btn = (
      <button
        key={it.id}
        type="button"
        data-folder-btn={isFolder ? "1" : undefined}
        onClick={it.onClick}
        title={it.title ?? it.label}
        draggable
        onDragStart={() => {
          dragId.current = it.id;
        }}
        onDragOver={(e) => {
          e.preventDefault();
          // V-13 合并判定（60% 重叠语义）：落点在目标图标中央 60% 区域 → 合并；边缘 → 排序
          const r = e.currentTarget.getBoundingClientRect();
          const fx = (e.clientX - r.left) / Math.max(1, r.width);
          const fy = (e.clientY - r.top) / Math.max(1, r.height);
          const merge = fx > 0.2 && fx < 0.8 && fy > 0.2 && fy < 0.8;
          setDragMode(merge ? "merge" : "reorder");
          setDragOverId(it.id);
        }}
        onDrop={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onDropTo(it.id);
        }}
        onDragEnd={() => {
          dragId.current = null;
          setDragOverId(null);
          setDragMode("reorder");
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          openAppMenu(e.clientX, e.clientY, it);
        }}
        className={`start-app${
          dragOverId === it.id ? (dragMode === "merge" ? " drag-merge" : " drag-over") : ""
        }`}
      >
        <span
          className={`desktop-icon-tile${isFolder ? " start-folder-tile" : ""}`}
          style={{ ["--hue" as string]: it.hue }}
        >
          {it.icon}
        </span>
        <span className="start-app-name">{it.label}</span>
      </button>
    );
    if (!isFolder || !it.folderId) return btn;
    const fid = it.folderId;
    const members = (getFolders().find((f) => f.id === fid)?.items ?? [])
      .map((id) => itemById.get(id))
      .filter((m): m is GridItem => Boolean(m));
    return (
      <span key={it.id} className="start-folder-wrap">
        {btn}
        {openFolderId === fid && (
          <div
            className="start-folder-flyout card-pop"
            role="dialog"
            aria-label={it.label}
            onPointerDown={(e) => e.stopPropagation()}
          >
            <div className="start-folder-flyout-head">
              <span className="start-folder-flyout-title">{it.label}</span>
              <span className="start-foot-spacer" />
              <button type="button" className="icon-btn tiny" title={t("rename")} aria-label={t("rename")} onClick={() => void doRenameFolder(fid)}>
                <Pencil size={13} />
              </button>
              <button type="button" className="icon-btn tiny" title={L.folderDisband} aria-label={L.folderDisband} onClick={() => doDisband(fid)}>
                <FolderMinus size={13} />
              </button>
              <button type="button" className="icon-btn tiny" aria-label={t("close")} onClick={() => setOpenFolderId(null)}>
                <X size={13} />
              </button>
            </div>
            <div className="start-grid">
              {members.map((m) => (
                <button
                  key={m.id}
                  type="button"
                  className="start-app"
                  title={m.title ?? m.label}
                  draggable
                  onDragStart={() => {
                    dragId.current = m.id;
                  }}
                  onClick={m.onClick}
                >
                  <span className="desktop-icon-tile" style={{ ["--hue" as string]: m.hue }}>
                    {m.icon}
                  </span>
                  <span className="start-app-name">{m.label}</span>
                </button>
              ))}
            </div>
          </div>
        )}
      </span>
    );
  };

  // ---- 批次E：最近使用行（点击直达） ----
  const recentLabel = (kind: string, name: string): string =>
    kind === "tp" ? name : name;

  // ---- 批次E：电源完整菜单（规格 4.6.3） ----
  // AI-03 V-19/V-20：关机/重启前会话清单 + 30 分钟不再询问 + 睡眠入口
  const power = async (action: "lock" | "logoff" | "reboot" | "shutdown" | "sleep"): Promise<void> => {
    setPowerOpen(false);
    if (action === "reboot" || action === "shutdown") {
      const label = action === "reboot" ? t("powerRestart") : t("powerShutdown");
      // V-19：勾选「仍要关机」记忆 30 分钟内不再询问
      let suppressed = false;
      try {
        const last = Number(localStorage.getItem("variable:power:confirm:v1") ?? "0");
        suppressed = Number.isFinite(last) && Date.now() - last < 30 * 60 * 1000;
      } catch { /* ignore */ }
      if (!suppressed) {
        // V-19：关机前会话清单（活动 VWM 窗口，只读；未完成传输/速记数据源未就绪，首版如实不带）
        const wins = vwmStore.getState().wins;
        const lines = wins.slice(0, 10).map((w) => `· ${w.app}${w.app.startsWith("tp:") ? "" : ` (${w.id.slice(-4)})`}`);
        if (wins.length > 10) lines.push(`… +${wins.length - 10}`);
        const session = lines.length > 0 ? `${t("powerSessions")}\n${lines.join("\n")}` : "";
        const ok = await askConfirm({
          title: label,
          body: `${t("powerConfirmBody", { action: label })}${session ? `\n\n${session}` : ""}`,
          danger: true,
          okLabel: label,
        });
        if (!ok) return;
        try { localStorage.setItem("variable:power:confirm:v1", String(Date.now())); } catch { /* ignore */ }
      }
    }
    await ipc
      .powerAction(action as "lock" | "logoff" | "reboot" | "shutdown")
      .catch((e) => pushToast("error", t("powerMenu"), errMessage(e).message));
  };

  return (
    <div
      className="start-overlay"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) props.onClose();
      }}
    >
      <div className="start-menu" role="dialog" aria-label={t("startMenu")} ref={menuRef}>
        <div className="start-search">
          <Search size={16} className="dim" />
          <input
            value={q}
            placeholder={t("startSearchHint")}
            onChange={(e) => setQ(e.target.value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Escape") {
                if (q) setQ("");
                else props.onClose();
              }
              if (e.key === "Enter" && q.trim()) {
                setQ("");
                props.onOpenSearch();
              }
            }}
          />
          {q && (
            <button type="button" className="icon-btn tiny" aria-label={t("close")} onClick={() => setQ("")}>
              <X size={13} />
            </button>
          )}
        </div>

        {recent.length > 0 && (
          <>
            <p className="start-section dim small">{t("recent")}</p>
            <div className="start-recent">
              {filteredRecent.map((r) => (
                <button
                  key={`${r.kind}-${r.id}`}
                  type="button"
                  className="start-recent-chip"
                  title={recentLabel(r.kind, r.name)}
                  onClick={() => {
                    if (r.kind === "app") {
                      props.onOpenApp(r.id as AppMode);
                    } else if (r.kind === "tp") {
                      props.onClose();
                      void launchThirdApp(r.id, r.name);
                    } else if (r.kind === "sys") {
                      props.onClose();
                      openVwmSystem(r.id as "explorer" | "recycle");
                    } else {
                      props.onClose();
                    }
                  }}
                >
                  {r.name}
                </button>
              ))}
            </div>
          </>
        )}

        {/* ---- V-12 「最近添加」自动分组（会话级可隐藏） ---- */}
        {recentAddedItems.length > 0 && (
          <>
            <p className="start-section start-group-head dim small">
              <span>{L.grpRecentAdded}</span>
              <button
                type="button"
                className="start-group-btn"
                title={L.grpHide}
                aria-label={L.grpHide}
                onClick={() => {
                  setGrpHidden((h) => ({ ...h, recentAdded: true }));
                  pushToast("info", L.grpRecentAdded, L.grpHideNote.replace("{name}", L.grpRecentAdded));
                }}
              >
                <X size={12} />
              </button>
            </p>
            <div className="start-recent">
              {recentAddedItems.map((e) => (
                <button
                  key={e.gridId}
                  type="button"
                  className="start-recent-chip"
                  title={e.label}
                  onClick={() => itemById.get(e.gridId)?.onClick()}
                >
                  {e.label}
                </button>
              ))}
            </div>
          </>
        )}

        {/* ---- V-12 「高频使用」自动分组（默认关；关闭后分组零界面痕迹） ---- */}
        {hfreqOn && !grpHidden.highFreq && (
          <>
            <p className="start-section start-group-head dim small">
              <span>{L.grpHighFreq}</span>
              <button
                type="button"
                className="start-group-btn on"
                title={L.hfreqOnTitle}
                aria-label={L.hfreqOnTitle}
                onClick={() => {
                  setHfreqOn(false);
                  setHighFreqEnabled(false);
                }}
              >
                <Flame size={12} />
              </button>
              <button
                type="button"
                className="start-group-btn"
                title={L.grpHide}
                aria-label={L.grpHide}
                onClick={() => {
                  setGrpHidden((h) => ({ ...h, highFreq: true }));
                  pushToast("info", L.grpHighFreq, L.grpHideNote.replace("{name}", L.grpHighFreq));
                }}
              >
                <X size={12} />
              </button>
            </p>
            {topList.length > 0 && (
              <div className="start-recent">
                {topList.map(({ item, count }) => (
                  <button
                    key={item.id}
                    type="button"
                    className="start-recent-chip"
                    title={`${item.label} · ${count}`}
                    onClick={() => itemById.get(item.id)?.onClick()}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            )}
          </>
        )}

        <p className="start-section start-group-head dim small">
          <span>{t("pinned")}</span>
          {/* V-12：高频分组默认关 —— 开关就近放在「已固定」标题旁（正式开关位在设置页，属 AI05/16 领地） */}
          {!hfreqOn && !grpHidden.highFreq && (
            <button
              type="button"
              className="start-group-btn"
              title={L.hfreqOffTitle}
              aria-label={L.hfreqOffTitle}
              onClick={() => {
                setHfreqOn(true);
                setHighFreqEnabled(true);
              }}
            >
              <Flame size={12} />
            </button>
          )}
        </p>
        {indexMode ? (
          <div className="start-index-wrap" {...gridDropProps}>
            <div
              className="start-idx-bar"
              role="navigation"
              aria-label={L.idxBarLabel}
              onPointerDown={() => {
                barDragging.current = true;
              }}
              onPointerUp={() => {
                barDragging.current = false;
              }}
              onPointerLeave={() => {
                barDragging.current = false;
                setIdxFlash(null);
              }}
              onKeyDown={(e) => {
                // 键盘可达（规格 V-11）：Tab 聚焦后字母键直达
                const k = e.key.toUpperCase();
                if (k.length === 1 && k >= "A" && k <= "Z") {
                  e.preventDefault();
                  jumpTo(k);
                }
              }}
            >
              {groups.map((g) => (
                <button
                  key={g.letter}
                  type="button"
                  className="start-idx-btn"
                  aria-label={`${L.idxBarLabel} ${g.letter}`}
                  onClick={() => jumpTo(g.letter)}
                  onPointerEnter={() => {
                    if (barDragging.current) jumpTo(g.letter);
                  }}
                >
                  {g.letter}
                </button>
              ))}
            </div>
            <div className="start-index-grid">
              {groups.map((g) => (
                <div key={g.letter} className="start-alpha-group" data-letter={g.letter}>
                  <p className="start-alpha-head dim small">{g.letter}</p>
                  <div className="start-grid">{g.items.map(renderGridButton)}</div>
                </div>
              ))}
            </div>
          </div>
        ) : (
          <div className="start-grid" {...gridDropProps}>
            {filtered.map(renderGridButton)}
          </div>
        )}

        {idxFlash && (
          <div className="start-idx-flash" aria-hidden>
            {idxFlash}
          </div>
        )}

        <div className="start-foot">
          <span className="start-user" title={userName || undefined}>
            <span className="start-avatar" aria-hidden>
              {(userName || "U").slice(0, 1).toUpperCase()}
            </span>
            <span className="start-user-name">{userName || "…"}</span>
          </span>
          <span className="start-foot-spacer" />
          <span className="start-brand">
            <span className="start-brand-v">V</span> Variable
          </span>
          <div className="start-power-wrap">
            {powerOpen && (
              <div className="start-power-menu card-pop" role="menu" aria-label={t("powerMenu")}>
                {/* AI-03 V-20：开机时长（只读，无新窗口） */}
                {uptimeSecs !== null && (
                  <span className="dim small start-power-uptime" title={t("powerUptime")}>
                    {t("powerUptime")}: {Math.floor(uptimeSecs / 3600)}h {Math.floor((uptimeSecs % 3600) / 60)}m
                  </span>
                )}
                <button type="button" role="menuitem" onClick={() => void power("sleep")}>
                  <Moon size={14} /> {t("powerSleep")}
                </button>
                <button type="button" role="menuitem" onClick={() => void power("lock")}>
                  <Lock size={14} /> {t("powerLock")}
                </button>
                <button type="button" role="menuitem" onClick={() => void power("logoff")}>
                  <LogOut size={14} /> {t("powerLogoff")}
                </button>
                <button type="button" role="menuitem" onClick={() => void power("reboot")}>
                  <RotateCcw size={14} /> {t("powerRestart")}
                </button>
                <button type="button" role="menuitem" onClick={() => void power("shutdown")}>
                  <Power size={14} /> {t("powerShutdown")}
                </button>
                <button type="button" role="menuitem" className="danger" onClick={props.onExit}>
                  <Power size={14} /> {t("exitVariable")}
                </button>
              </div>
            )}
            <button
              type="button"
              className={`tb-btn start-power${powerOpen ? " active" : ""}`}
              aria-label={t("powerMenu")}
              title={t("powerMenu")}
              onClick={() => setPowerOpen(!powerOpen)}
            >
              <Power size={18} strokeWidth={1.7} />
            </button>
          </div>
        </div>
      </div>

      {/* V-14 属性面板（本机只读信息） */}
      <StartPropsPanel info={propsInfo} onClose={() => setPropsInfo(null)} />
    </div>
  );
}
