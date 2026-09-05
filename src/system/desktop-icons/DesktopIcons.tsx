import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import {
  AppWindow,
  Folder,
  FolderSearch,
  Map as MapIcon,
  Monitor,
  PenLine,
  Sparkles,
  Trash2,
  X,
  type LucideIcon,
} from "lucide-react";
import { openContextMenu, type MenuItem } from "../../components/ContextMenu";
import { askConfirm, askPrompt } from "../../components/Modal";
import { errMessage, ipc, type ThirdApp } from "../../lib/ipc";
import type { CustomBg, IconSize, Settings, WallpaperMode } from "../../lib/settings";
import { useI18n } from "../../i18n";
import { pushToast, uiStore, type AppMode } from "../../state/uiStore";
import { launchThirdApp, reloadThirdApps, useThirdApps } from "../launcher/thirdApps";
import { useUninstalledOfficial } from "../launcher/official";
import { openSystemWindow } from "../windows/appWindows";
import { ShelfFlyout, type FlyItem } from "./ShelfFlyout";
import {
  allShelfMembers,
  loadDesktopLayout,
  newShelfId,
  popRedo,
  popUndo,
  pushRedo,
  pushUndo,
  saveDesktopLayout,
  shelfWouldCycle,
  type Cell,
  type DesktopLayout,
  type IconClip,
  type ShelfColor,
  type ShelfDef,
  type SortMode,
} from "./layout";

/**
 * 桌面图标网格（对齐 Windows 习惯；批次B = 规格 4.2.4–4.2.8 + 4.6/4.7）：
 * - 左侧列优先排列（从上到下，再下一列）；系统图标（此电脑/回收站）默认排最前
 * - 三档图标大小（32/48/64，设置存储）；排序方式：类型 / 名称
 * - 多选：Ctrl+单击、Shift+单击（区间）、Ctrl+A、空白拖拽框选（蓝色选择框）
 * - 键盘：Enter 打开 / Delete·Shift+Delete 删除 / F2 重命名 / Ctrl+C·X·V 剪贴板
 *   / Ctrl+Z·Y 撤销重做 / Alt+Enter 属性 / Ctrl+A 全选 / Esc 清除
 * - 拖拽：松手吸附最近网格（占用格交换，多选集群放置）；拖到文件架=归入；
 *   拖到回收站=移除第三方登记（官方软件不可删除，如实提示）
 * - 长按 600ms → 抖动编辑模式（第三方/文件架出现移除角标），点空白/Esc 退出
 * - 更换图标：.ico/.png ≤512KB（第三方存登记表随 U 盘走；文件架存本地 UI 层）
 * - 文件架：启动器分组容器（嵌套/徽标/6 色/可选链接文件夹 → 文件管理器定位）
 * - 右键完整版（4.6）：桌面空白 = 新建/查看/排序/添加应用/切换壁纸/关于；
 *   图标 = 打开/管理员运行/更换图标/重命名/剪切复制/归架/移除/属性
 *
 * 诚实边界（不提供假功能）：固定到任务栏/开始菜单（#29 未做）、移动到 Variable
 * 目录（#41 未做）暂不出现于菜单；图片缩略图需资产协议，暂以颜色+首字母呈现。
 */

interface AppIconDef {
  id: string;
  app: AppMode;
  icon: LucideIcon;
  hue: number; // 图标底座色相（低饱和冷调）
}

/** M6 系统图标：打开系统窗口（非软件、无特权，同样可拖拽排列）。 */
interface SysIconDef {
  id: string;
  sys: "explorer" | "recycle";
  icon: LucideIcon;
  hue: number;
}

/** M7 第三方软件图标（独立 OS 进程，与四款软件平级，无特权）。 */
interface ThirdIconDef {
  id: string;
  third: ThirdApp;
  icon: LucideIcon;
  hue: number;
}

/** 批次B 文件架图标（桌面启动器分组容器）。 */
interface ShelfIconDef {
  id: string;
  shelfId: string;
  shelf: ShelfDef;
  icon: LucideIcon;
  hue: number;
}

type IconDef = AppIconDef | SysIconDef | ThirdIconDef | ShelfIconDef;

const GRID_PAD = 14;
const DRAG_THRESHOLD = 5;
const LONG_PRESS_MS = 600;

/** 三档图标大小（规格 4.2.4）→ 网格/底座/线条尺寸。 */
const SIZE_TIERS: Record<IconSize, { w: number; h: number; tile: number; icon: number }> = {
  32: { w: 78, h: 92, tile: 40, icon: 20 },
  48: { w: 94, h: 106, tile: 56, icon: 26 },
  64: { w: 112, h: 122, tile: 72, icon: 34 },
};

export function desktopIconDefs(): AppIconDef[] {
  return [
    { id: "app-write", app: "write", icon: PenLine, hue: 224 },
    { id: "app-mind", app: "mindmap", icon: MapIcon, hue: 192 },
    { id: "app-code", app: "project", icon: FolderSearch, hue: 258 },
    { id: "app-fate", app: "fate", icon: Sparkles, hue: 206 },
  ];
}

/** M6 系统图标定义（此电脑 / 回收站）。 */
export function sysIconDefs(): SysIconDef[] {
  return [
    { id: "sys-explorer", sys: "explorer", icon: Monitor, hue: 210 },
    { id: "sys-recycle", sys: "recycle", icon: Trash2, hue: 0 },
  ];
}

function thirdIconDef(a: ThirdApp): ThirdIconDef {
  return { id: `tp-${a.id}`, third: a, icon: AppWindow, hue: 158 };
}

/** 四款软件显示名（独立软件，恰好由 Variable 官方出品）。 */
export function desktopAppLabel(app: AppMode): string {
  const map: Record<AppMode, string> = {
    write: "Variable Write",
    mindmap: "Variable Mind",
    project: "Variable Code",
    fate: "Variable Fate",
  };
  return map[app];
}

export function DesktopIcons(props: {
  onOpenApp: (app: AppMode) => void;
  onOpenSystem: (kind: "explorer" | "recycle") => void;
  onOpenSettings: () => void;
  /** 批次B：三档图标大小（设置 → 外观 / 桌面右键查看子菜单）。 */
  iconSize: IconSize;
  /** 当前壁纸模式（右键"切换壁纸"子菜单勾选态）。 */
  wallpaperMode: WallpaperMode;
  /** 批次E-6：每日换壁纸缓存目录（右键"下一张壁纸"取图处）。 */
  wallpaperPoolDir: string;
  /** 当前壁纸自定义配置（右键"下一张壁纸"回写 imagePath）。 */
  customBg: CustomBg;
  /** 批次B：设置补丁（图标大小 / 壁纸切换子菜单）。 */
  onPatchSettings: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const { t } = useI18n();
  const [layout, setLayout] = useState<DesktopLayout>(() => loadDesktopLayout());
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const [drag, setDrag] = useState<{ ids: string[]; primary: string; dx: number; dy: number; moved: boolean } | null>(null);
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const [edit, setEdit] = useState(false); // 长按抖动编辑模式
  const [clip, setClip] = useState<IconClip | null>(null);
  const [flyoutId, setFlyoutId] = useState<string | null>(null);
  const [propsFor, setPropsFor] = useState<string | null>(null);
  const [about, setAbout] = useState(false);
  const [rows, setRows] = useState(6);

  const dragRef = useRef(drag);
  dragRef.current = drag;
  const iconOrigin = useRef<{ x: number; y: number } | null>(null);
  const pressTimer = useRef(0);
  const marqueeStart = useRef<{ x: number; y: number; base: Set<string> | null } | null>(null);
  const anchorIdx = useRef(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const selectedRef = useRef(selected);
  selectedRef.current = selected;
  const layoutRef = useRef(layout);
  layoutRef.current = layout;

  const tier = SIZE_TIERS[props.iconSize] ?? SIZE_TIERS[48];

  // ---- 定义集合：系统 + 四款软件 + 第三方 + 文件架 ----
  const thirds = useThirdApps();
  const uninstalled = useUninstalledOfficial();
  const shelfDefs = useMemo<ShelfIconDef[]>(
    () =>
      Object.entries(layout.shelves).map(([shelfId, s]) => ({
        id: shelfId,
        shelfId,
        shelf: s,
        icon: Folder,
        hue: 268,
      })),
    [layout.shelves],
  );

  const labelOf = useCallback(
    (d: IconDef): string => {
      if ("sys" in d) return d.sys === "explorer" ? t("explorerWin") : t("recycleBin");
      if ("third" in d) return d.third.name;
      if ("shelfId" in d) return layout.shelves[d.shelfId]?.name ?? "";
      return desktopAppLabel(d.app);
    },
    [t, layout.shelves],
  );

  /** 排序：type = 系统/官方/第三方/文件架；name = 按显示名（稳定）。 */
  const orderedDefs = useMemo<IconDef[]>(() => {
    const sys = sysIconDefs();
    // 批次C（规格 5.6.1）：已卸载的预装软件不显示（数据保留，恢复入口在软件管理）
    const apps = desktopIconDefs().filter((d) => uninstalled[d.app] === undefined);
    const tp = thirds.map(thirdIconDef);
    return layout.sort === "name"
      ? [...sys, ...apps, ...tp, ...shelfDefs].sort((a, b) => labelOf(a).localeCompare(labelOf(b), "zh"))
      : [...sys, ...apps, ...tp, ...shelfDefs];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [thirds, shelfDefs, layout.sort, labelOf, uninstalled]);

  const defById = useMemo(() => new Map(orderedDefs.map((d) => [d.id, d])), [orderedDefs]);

  /** 被任何文件架收容的图标 → 不在主网格显示。 */
  const hiddenIds = useMemo(() => allShelfMembers(layout), [layout]);
  const visibleDefs = useMemo(() => orderedDefs.filter((d) => !hiddenIds.has(d.id)), [orderedDefs, hiddenIds]);

  // ---- 布局 ----
  const persist = useCallback((next: DesktopLayout): void => {
    setLayout(next);
    saveDesktopLayout(next);
  }, []);

  const commit = useCallback(
    (next: DesktopLayout): void => {
      pushUndo(layoutRef.current);
      persist(next);
    },
    [persist],
  );

  const undo = useCallback((): void => {
    const prev = popUndo();
    if (prev) {
      pushRedo(layoutRef.current);
      persist(prev);
    }
  }, [persist]);

  const redo = useCallback((): void => {
    const next = popRedo();
    if (next) {
      pushUndo(layoutRef.current);
      persist(next);
    }
  }, [persist]);

  // 视口高度 → 每列行数（列优先打包的基础）。
  useEffect(() => {
    const compute = (): void =>
      setRows(Math.max(1, Math.floor((window.innerHeight - GRID_PAD * 2) / tier.h)));
    compute();
    window.addEventListener("resize", compute);
    return () => window.removeEventListener("resize", compute);
  }, [tier.h]);

  /** 当前布局：自动排列 → 列优先按定义顺序；手动 → 存储位置 + 空位补齐。 */
  const placed = useMemo(() => {
    const map = new Map<string, Cell>();
    const occupied = new Set<string>();
    const packColumnMajor = (id: string, idx: number): void => {
      const c = Math.floor(idx / rows);
      const r = idx % rows;
      map.set(id, { c, r });
      occupied.add(`${c}:${r}`);
    };
    if (layout.autoArrange) {
      visibleDefs.forEach((d, i) => packColumnMajor(d.id, i));
    } else {
      for (const d of visibleDefs) {
        const p = layout.positions[d.id];
        if (p && p.c >= 0 && p.r >= 0 && p.r < rows && !occupied.has(`${p.c}:${p.r}`)) {
          map.set(d.id, { c: p.c, r: p.r });
          occupied.add(`${p.c}:${p.r}`);
        }
      }
      let idx = 0;
      for (const d of visibleDefs) {
        if (map.has(d.id)) continue;
        for (;; idx++) {
          const c = Math.floor(idx / rows);
          const r = idx % rows;
          if (!occupied.has(`${c}:${r}`)) {
            map.set(d.id, { c, r });
            occupied.add(`${c}:${r}`);
            idx++;
            break;
          }
          idx++;
        }
      }
    }
    return map;
  }, [layout, visibleDefs, rows]);

  /** 从某格起（列优先）找第一个空闲格。 */
  const findFree = (occupied: Set<string>, startIdx: number): { c: number; r: number; idx: number } => {
    let idx = startIdx;
    for (;; idx++) {
      const c = Math.floor(idx / rows);
      const r = idx % rows;
      if (!occupied.has(`${c}:${r}`)) return { c, r, idx };
    }
  };

  const occupiedSet = useMemo(() => new Set([...placed.values()].map((p) => `${p.c}:${p.r}`)), [placed]);
  void occupiedSet;

  const openItem = useCallback(
    (d: IconDef): void => {
      if (edit) return;
      if ("sys" in d) props.onOpenSystem(d.sys);
      else if ("third" in d) void launchThirdApp(d.third.id, d.third.name);
      else if ("shelfId" in d) setFlyoutId(d.shelfId);
      else props.onOpenApp(d.app);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [props, edit],
  );

  /** 移除第三方登记（仅删登记，不卸载软件本身）。 */
  const removeThird = useCallback(
    async (a: ThirdApp): Promise<void> => {
      const ok = await askConfirm({
        title: t("tpRemove"),
        body: t("tpRemoveBody", { name: a.name }),
        danger: true,
        okLabel: t("tpRemove"),
      });
      if (!ok) return;
      try {
        await ipc.tpRemove(a.id);
        await reloadThirdApps();
        setSelected((s) => {
          const n = new Set(s);
          n.delete(`tp-${a.id}`);
          return n;
        });
      } catch (e) {
        pushToast("error", t("tpRemove"), errMessage(e).message);
      }
    },
    [t],
  );

  // ---- 文件架操作 ----
  const createShelf = useCallback((): void => {
    const l = layoutRef.current;
    const id = newShelfId();
    const base = t("newShelf");
    let name = base;
    let n = 2;
    const names = new Set(Object.values(l.shelves).map((s) => s.name));
    while (names.has(name)) name = `${base} ${n++}`;
    const occupied = new Set([...placed.values()].map((p) => `${p.c}:${p.r}`));
    const cell = findFree(occupied, 0);
    const shelves = { ...l.shelves, [id]: { name, color: "blue" as ShelfColor, members: [] } };
    commit({
      ...l,
      shelves,
      autoArrange: false,
      positions: { ...l.positions, [id]: { c: cell.c, r: cell.r } },
    });
    setSelected(new Set([id]));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t, placed, commit, rows]);

  const moveToShelf = useCallback(
    (ids: string[], shelfId: string): void => {
      const l = layoutRef.current;
      const shelves = { ...l.shelves };
      for (const [sid, s] of Object.entries(shelves)) {
        const filtered = s.members.filter((m) => !ids.includes(m));
        if (filtered.length !== s.members.length) shelves[sid] = { ...s, members: filtered };
      }
      const target = shelves[shelfId];
      if (!target) return;
      const add = ids.filter((id) => {
        if (id === shelfId) return false;
        if (id.startsWith("sys-")) return false; // 此电脑/回收站不入架
        if (id.startsWith("shelf-") && shelfWouldCycle(l, id, shelfId)) return false;
        return true;
      });
      if (add.length === 0) return;
      shelves[shelfId] = { ...target, members: [...target.members, ...add] };
      commit({ ...l, shelves });
      setFlyoutId(shelfId);
    },
    [commit],
  );

  const moveOutOfShelf = useCallback(
    (id: string): void => {
      const l = layoutRef.current;
      const shelves = { ...l.shelves };
      for (const [sid, s] of Object.entries(shelves)) {
        if (s.members.includes(id)) shelves[sid] = { ...s, members: s.members.filter((m) => m !== id) };
      }
      // 回到网格：原位被占则找空格
      const positions = { ...l.positions };
      const occ = new Set([...placed.entries()].filter(([k]) => k !== id && !hiddenIds.has(k)).map(([, p]) => `${p.c}:${p.r}`));
      const old = positions[id];
      if (old && !occ.has(`${old.c}:${old.r}`)) {
        // 原位可用
      } else {
        const cell = findFree(occ, 0);
        positions[id] = { c: cell.c, r: cell.r };
      }
      commit({ ...l, shelves, positions, autoArrange: false });
      // eslint-disable-next-line react-hooks/exhaustive-deps
    },
    [commit, placed, hiddenIds, rows],
  );

  const deleteShelf = useCallback(
    (shelfId: string): void => {
      const l = layoutRef.current;
      const shelves = { ...l.shelves };
      delete shelves[shelfId];
      // 嵌套子架上浮（成员位置由 placed 空位补齐逻辑处理）
      commit({ ...l, shelves });
      setSelected((s) => {
        const n = new Set(s);
        n.delete(shelfId);
        return n;
      });
      if (flyoutId === shelfId) setFlyoutId(null);
    },
    [commit, flyoutId],
  );

  const renameShelf = useCallback(
    async (shelfId: string): Promise<void> => {
      const cur = layoutRef.current.shelves[shelfId];
      if (!cur) return;
      const name = await askPrompt({ title: t("exRenameTitle"), initial: cur.name });
      if (!name || !name.trim()) return;
      const l = layoutRef.current;
      commit({ ...l, shelves: { ...l.shelves, [shelfId]: { ...cur, name: name.trim() } } });
    },
    [t, commit],
  );

  const linkShelfFolder = useCallback(
    async (shelfId: string): Promise<void> => {
      const picked = await openFileDialog({ directory: true, multiple: false });
      if (typeof picked !== "string" || !picked) return;
      const l = layoutRef.current;
      const s = l.shelves[shelfId];
      if (!s) return;
      commit({ ...l, shelves: { ...l.shelves, [shelfId]: { ...s, linkedPath: picked } } });
      pushToast("success", t("shelfLinkFolder"), picked);
    },
    [t, commit],
  );

  // ---- 更换图标 ----
  const pickIcon = useCallback(async (): Promise<string | null> => {
    const picked = await openFileDialog({
      multiple: false,
      filters: [{ name: "Icon", extensions: ["ico", "png"] }],
    });
    return typeof picked === "string" && picked ? picked : null;
  }, []);

  const changeIcon = useCallback(
    async (d: IconDef): Promise<void> => {
      const path = await pickIcon();
      if (!path) return;
      if ("third" in d) {
        try {
          await ipc.tpSetIcon(d.third.id, path);
          await reloadThirdApps();
        } catch (e) {
          pushToast("error", t("changeIcon"), errMessage(e).message);
          return;
        }
      } else if ("shelfId" in d) {
        try {
          const url = await ipc.iconDataurl(path);
          const l = layoutRef.current;
          const s = l.shelves[d.shelfId];
          if (s) commit({ ...l, shelves: { ...l.shelves, [d.shelfId]: { ...s, icon: url } } });
        } catch (e) {
          pushToast("error", t("changeIcon"), errMessage(e).message);
          return;
        }
      } else {
        pushToast("info", t("changeIcon"), t("sysIconLocked"));
        return;
      }
      pushToast("success", t("changeIcon"), t("iconChanged"));
    },
    [t, commit],
  );

  const clearIcon = useCallback(
    async (d: IconDef): Promise<void> => {
      if ("third" in d) {
        try {
          await ipc.tpSetIcon(d.third.id, null);
          await reloadThirdApps();
        } catch (e) {
          pushToast("error", t("clearIcon"), errMessage(e).message);
          return;
        }
      } else if ("shelfId" in d) {
        const l = layoutRef.current;
        const s = l.shelves[d.shelfId];
        if (s) {
          const { icon: _drop, ...rest } = s;
          commit({ ...l, shelves: { ...l.shelves, [d.shelfId]: rest } });
        }
      }
    },
    [t, commit],
  );

  const runAsAdmin = useCallback(
    async (a: ThirdApp): Promise<void> => {
      try {
        await ipc.tpLaunchAdmin(a.id);
      } catch (e) {
        pushToast("error", t("runAsAdmin"), errMessage(e).message);
      }
    },
    [t],
  );

  const renameThird = useCallback(
    async (a: ThirdApp): Promise<void> => {
      const name = await askPrompt({ title: t("exRenameTitle"), initial: a.name });
      if (!name || !name.trim()) return;
      try {
        await ipc.tpRename(a.id, name.trim());
        await reloadThirdApps();
      } catch (e) {
        pushToast("error", t("rename"), errMessage(e).message);
      }
    },
    [t],
  );

  /** F2：重命名单个选中项（第三方登记 / 文件架）。 */
  const renameSingle = useCallback((): void => {
    if (selected.size !== 1) return;
    const first = [...selected][0];
    if (first === undefined) return;
    const d = defById.get(first);
    if (!d) return;
    if ("third" in d) void renameThird(d.third);
    else if ("shelfId" in d) void renameShelf(d.shelfId);
    else pushToast("info", t("rename"), t("sysIconLocked"));
  }, [selected, defById, renameThird, renameShelf, t]);

  /** Delete / Shift+Delete：第三方=移除登记（批量确认）；文件架=删除（成员回桌面）；系统图标=如实提示。 */
  const deleteSelection = useCallback(
    async (_permanent: boolean): Promise<void> => {
      const ids = [...selected];
      if (ids.length === 0) return;
      const thirdDefs = ids.map((id) => defById.get(id)).filter((d): d is ThirdIconDef => d !== undefined && "third" in d);
      const shelfIds = ids.filter((id) => id.startsWith("shelf-"));
      const lockedCount = ids.length - thirdDefs.length - shelfIds.length;
      if (thirdDefs.length > 0) {
        const ok = await askConfirm({
          title: t("tpRemove"),
          body:
            thirdDefs.length === 1
              ? t("tpRemoveBody", { name: thirdDefs[0]?.third.name ?? "" })
              : t("tpRemoveManyBody", { n: thirdDefs.length }),
          danger: true,
          okLabel: t("tpRemove"),
        });
        if (ok) {
          for (const d of thirdDefs) {
            try {
              await ipc.tpRemove(d.third.id);
            } catch (e) {
              pushToast("error", t("tpRemove"), errMessage(e).message);
            }
          }
          await reloadThirdApps();
          setSelected((s) => {
            const n = new Set(s);
            for (const d of thirdDefs) n.delete(d.id);
            return n;
          });
        }
      }
      if (shelfIds.length > 0) {
        const ok = await askConfirm({
          title: t("shelfDelete"),
          body: t("shelfDeleteManyBody", { n: shelfIds.length }),
          danger: true,
          okLabel: t("shelfDelete"),
        });
        if (ok) for (const id of shelfIds) deleteShelf(id);
      }
      if (lockedCount > 0) pushToast("info", t("delete"), t("sysIconLocked"));
    },
    [selected, defById, t, deleteShelf],
  );

  /** 剪贴板粘贴：剪切=移动到空格并出架；复制=仅文件架可克隆（如实提示其余项）。 */
  const pasteClip = useCallback((): void => {
    if (!clip || clip.ids.length === 0) return;
    const l = layoutRef.current;
    const occ = new Set([...placed.entries()].filter(([k]) => !clip.ids.includes(k)).map(([, p]) => `${p.c}:${p.r}`));
    let idx = 0;
    const positions = { ...l.positions };
    let shelves = { ...l.shelves };
    if (clip.mode === "cut") {
      // 出架 + 顺序放入空格
      for (const [sid, s] of Object.entries(shelves)) {
        const filtered = s.members.filter((m) => !clip.ids.includes(m));
        if (filtered.length !== s.members.length) shelves = { ...shelves, [sid]: { ...s, members: filtered } };
      }
      for (const id of clip.ids) {
        if (id.startsWith("sys-")) continue;
        const cell = findFree(occ, idx);
        idx = cell.idx + 1;
        positions[id] = { c: cell.c, r: cell.r };
        occ.add(`${cell.c}:${cell.r}`);
      }
      commit({ ...l, positions, shelves, autoArrange: false });
      setClip(null);
      return;
    }
    // copy：克隆文件架（成员随克隆；登记项不复制 —— 同路径幂等，无意义）
    let cloned = 0;
    for (const id of clip.ids) {
      const src = l.shelves[id];
      if (!src) continue;
      const nid = newShelfId();
      shelves = { ...shelves, [nid]: { ...src, name: `${src.name} ·${t("copySuffix")}`, members: [...src.members] } };
      const cell = findFree(occ, idx);
      idx = cell.idx + 1;
      positions[nid] = { c: cell.c, r: cell.r };
      occ.add(`${cell.c}:${cell.r}`);
      cloned++;
    }
    if (cloned === 0) {
      pushToast("info", t("paste"), t("clipCopyNA"));
      return;
    }
    commit({ ...l, positions, shelves, autoArrange: false });
  }, [clip, placed, commit, t, rows]);

  // ---- 指针交互 ----
  const clearPress = (): void => {
    window.clearTimeout(pressTimer.current);
    pressTimer.current = 0;
  };

  const onIconPointerDown = (e: React.PointerEvent, d: IconDef, idx: number): void => {
    if (e.button !== 0) return;
    const id = d.id;
    let next: Set<string>;
    if (e.ctrlKey || e.metaKey) {
      next = new Set(selectedRef.current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
    } else if (e.shiftKey) {
      next = new Set(selectedRef.current);
      const a = anchorIdx.current;
      const [lo, hi] = a <= idx ? [a, idx] : [idx, a];
      for (let i = lo; i <= hi; i++) {
        const dd = visibleDefs[i];
        if (dd) next.add(dd.id);
      }
    } else {
      next = selectedRef.current.has(id) ? new Set(selectedRef.current) : new Set([id]);
    }
    setSelected(next);
    setAnchor(idx);
    if (edit) return; // 编辑模式：只选择，不拖拽
    const ids = next.has(id) && next.size > 1 ? [...next] : [id];
    const el = e.currentTarget as HTMLElement;
    const rect = el.getBoundingClientRect();
    iconOrigin.current = { x: rect.left, y: rect.top };
    el.setPointerCapture(e.pointerId);
    setDrag({ ids, primary: id, dx: 0, dy: 0, moved: false });
    clearPress();
    pressTimer.current = window.setTimeout(() => {
      setEdit(true); // 长按 → 抖动编辑模式
      setDrag(null);
    }, LONG_PRESS_MS);
  };

  const setAnchor = (idx: number): void => {
    anchorIdx.current = idx;
  };

  const onIconPointerMove = (e: React.PointerEvent): void => {
    const d = dragRef.current;
    if (!d || !iconOrigin.current) return;
    const dx = e.clientX - iconOrigin.current.x;
    const dy = e.clientY - iconOrigin.current.y;
    if (!d.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
    clearPress();
    setDrag({ ...d, dx, dy, moved: true });
  };

  const onIconPointerUp = (e: React.PointerEvent): void => {
    clearPress();
    const d = dragRef.current;
    setDrag(null);
    iconOrigin.current = null;
    if (!d || !d.moved) return; // 视为点击，交给 onClick/onDoubleClick
    const c = Math.round((e.clientX - tier.w / 2 - GRID_PAD) / tier.w);
    const r = Math.round((e.clientY - tier.h / 2 - GRID_PAD) / tier.h);
    if (c < 0 || r < 0 || r >= rows) return;

    // 1) 拖到回收站 → 移除第三方登记
    const recycleCell = placed.get("sys-recycle");
    if (recycleCell && recycleCell.c === c && recycleCell.r === r) {
      const thirds = d.ids
        .map((id) => defById.get(id))
        .filter((x): x is ThirdIconDef => x !== undefined && "third" in x);
      const locked = d.ids.length - thirds.length;
      if (locked > 0) pushToast("info", t("delete"), t("sysIconLocked"));
      if (thirds.length > 0) {
        void (async () => {
          const ok = await askConfirm({
            title: t("tpRemove"),
            body:
              thirds.length === 1
                ? t("tpRemoveBody", { name: thirds[0]?.third.name ?? "" })
                : t("tpRemoveManyBody", { n: thirds.length }),
            danger: true,
            okLabel: t("tpRemove"),
          });
          if (!ok) return;
          for (const x of thirds) {
            try {
              await ipc.tpRemove(x.third.id);
            } catch (err) {
              pushToast("error", t("tpRemove"), errMessage(err).message);
            }
          }
          await reloadThirdApps();
        })();
      }
      return;
    }

    // 2) 拖到文件架 → 归入
    for (const [sid, cell] of placed) {
      if (cell.c === c && cell.r === r && sid.startsWith("shelf-") && !d.ids.includes(sid)) {
        moveToShelf(d.ids.filter((id) => id !== sid), sid);
        return;
      }
    }

    // 3) 网格移动（单选=交换；多选=集群放置）
    if (d.ids.length === 1) {
      const cur = placed.get(d.primary);
      if (!cur || (cur.c === c && cur.r === r)) return;
      const positions: DesktopLayout["positions"] = { ...layout.positions, [d.primary]: { c, r } };
      for (const [otherId, cell] of placed) {
        if (otherId !== d.primary && cell.c === c && cell.r === r) {
          positions[otherId] = { ...cur };
          break;
        }
      }
      commit({ ...layout, autoArrange: false, positions });
      return;
    }
    const positions: DesktopLayout["positions"] = { ...layout.positions };
    const occ = new Set([...placed.entries()].filter(([k]) => !d.ids.includes(k)).map(([, p]) => `${p.c}:${p.r}`));
    let startIdx = c * rows + r;
    for (const id of d.ids) {
      const cell = findFree(occ, startIdx);
      startIdx = cell.idx + 1;
      positions[id] = { c: cell.c, r: cell.r };
      occ.add(`${cell.c}:${cell.r}`);
    }
    commit({ ...layout, autoArrange: false, positions });
  };

  // ---- 框选 ----
  const onContainerPointerDown = (e: React.PointerEvent): void => {
    if (e.target !== e.currentTarget) return;
    if (edit) {
      setEdit(false);
      setSelected(new Set());
      return;
    }
    setFlyoutId(null);
    setSelected(new Set());
    if (e.button !== 0 || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    marqueeStart.current = {
      x: e.clientX - rect.left,
      y: e.clientY - rect.top,
      base: e.shiftKey ? new Set(selectedRef.current) : null,
    };
    setMarquee({ x0: marqueeStart.current.x, y0: marqueeStart.current.y, x1: marqueeStart.current.x, y1: marqueeStart.current.y });
    containerRef.current.setPointerCapture(e.pointerId);
  };

  const onContainerPointerMove = (e: React.PointerEvent): void => {
    const st = marqueeStart.current;
    if (!st || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    setMarquee((m) => (m ? { ...m, x1: e.clientX - rect.left, y1: e.clientY - rect.top } : null));
  };

  const onContainerPointerUp = (): void => {
    const st = marqueeStart.current;
    const m = marquee;
    marqueeStart.current = null;
    setMarquee(null);
    if (!st || !m) return;
    const x0 = Math.min(m.x0, m.x1);
    const x1 = Math.max(m.x0, m.x1);
    const y0 = Math.min(m.y0, m.y1);
    const y1 = Math.max(m.y0, m.y1);
    const next = st.base ? new Set(st.base) : new Set<string>();
    if (x1 - x0 > 3 || y1 - y0 > 3) {
      for (const d of visibleDefs) {
        const cell = placed.get(d.id);
        if (!cell) continue;
        const left = GRID_PAD + cell.c * tier.w;
        const top = GRID_PAD + cell.r * tier.h;
        if (left < x1 && left + tier.w > x0 && top < y1 && top + tier.h > y0) next.add(d.id);
      }
    }
    setSelected(next);
  };

  // ---- 键盘 ----
  const onKeyDown = (e: React.KeyboardEvent): void => {
    const ctrl = e.ctrlKey || e.metaKey;
    const k = e.key;
    if (k === "Escape") {
      if (flyoutId) setFlyoutId(null);
      else if (edit) setEdit(false);
      else setSelected(new Set());
      return;
    }
    if (k === "Enter" && e.altKey) {
      e.preventDefault();
      if (selected.size === 1) setPropsFor([...selected][0] ?? null);
      return;
    }
    if (k === "Enter") {
      e.preventDefault();
      for (const id of selected) {
        const d = defById.get(id);
        if (d) openItem(d);
      }
      return;
    }
    if (k === "F2") {
      e.preventDefault();
      renameSingle();
      return;
    }
    if (k === "Delete") {
      e.preventDefault();
      void deleteSelection(e.shiftKey);
      return;
    }
    if (!ctrl) return;
    const lk = k.toLowerCase();
    if (lk === "a") {
      e.preventDefault();
      setSelected(new Set(visibleDefs.map((d) => d.id)));
    } else if (lk === "c") {
      if (selected.size) setClip({ ids: [...selected], mode: "copy" });
    } else if (lk === "x") {
      if (selected.size) setClip({ ids: [...selected], mode: "cut" });
    } else if (lk === "v") {
      e.preventDefault();
      pasteClip();
    } else if (lk === "z") {
      e.preventDefault();
      undo();
    } else if (lk === "y") {
      e.preventDefault();
      redo();
    }
  };

  // ---- 右键菜单 ----
  const toggleAutoArrange = (): void => {
    commit({ ...layout, autoArrange: !layout.autoArrange });
  };

  const refresh = (): void => {
    setLayout(loadDesktopLayout());
    setSelected(new Set());
  };

  // 批次E-6：右键"下一张壁纸" —— 本地缓存池随机取一张（零网络）并立即应用
  const nextWallpaper = (): void => {
    if (!props.wallpaperPoolDir) {
      pushToast("info", t("wpNext"), t("wpNeedPoolDir"));
      props.onOpenSettings();
      return;
    }
    void ipc
      .wpPickDaily(props.wallpaperPoolDir, "next")
      .then((picked) => {
        if (!picked) {
          pushToast("info", t("wpNext"), t("wpPoolEmpty"));
          return;
        }
        props.onPatchSettings({
          wallpaperMode: "image",
          customBg: { ...props.customBg, type: "image", imagePath: picked },
        });
      })
      .catch((e) => pushToast("error", t("wpNext"), errMessage(e).message));
  };

  const openDesktopMenu = (e: React.MouseEvent): void => {
    if (e.button !== 0) e.preventDefault();
    const sizes: IconSize[] = [32, 48, 64];
    const walls = ["solid", "gravity", "image", "video", "hybrid"] as const;
    const wallKeys: Record<(typeof walls)[number], string> = {
      solid: "wpSolid",
      gravity: "wpGravity",
      image: "wpImage",
      video: "wpVideo",
      hybrid: "wpHybrid",
    };
    const items: MenuItem[] = [
      {
        label: t("newWn"),
        children: [
          { label: t("newShelf"), onClick: createShelf },
          { label: t("newRecord"), onClick: () => props.onOpenApp("write") },
          { label: t("newMindmap"), onClick: () => props.onOpenApp("mindmap") },
        ],
      },
      {
        label: t("viewMenu"),
        children: sizes.map((s) => ({
          label: `${s} × ${s}`,
          checked: props.iconSize === s,
          onClick: () => props.onPatchSettings({ iconSize: s }),
        })),
      },
      {
        label: t("sortBy"),
        children: (["type", "name"] as SortMode[]).map((m) => ({
          label: m === "type" ? t("sortType") : t("sortName"),
          checked: layout.sort === m,
          onClick: () => commit({ ...layoutRef.current, sort: m }),
        })),
      },
      { separator: true },
      { label: t("addApp"), onClick: () => uiStore.setState({ launcherOpen: true, startOpen: false }) },
      {
        label: t("wallpaperSwitch"),
        children: [
          ...walls.map((w) => ({
            label: t(wallKeys[w]),
            checked: props.wallpaperMode === w,
            onClick: () => props.onPatchSettings({ wallpaperMode: w }),
          })),
          { separator: true },
          // 批次E-6：右键快捷换一张（本地缓存池随机；未配置缓存目录时弹设置提示）
          { label: t("wpNext"), onClick: nextWallpaper },
          { label: t("wpDailySetup"), onClick: props.onOpenSettings },
        ],
      },
      { separator: true },
      { label: t("autoArrange"), checked: layout.autoArrange, onClick: toggleAutoArrange },
      { label: t("refreshDesktop"), onClick: refresh },
      { label: t("personalize"), onClick: props.onOpenSettings },
      { separator: true },
      { label: t("aboutVariable"), onClick: () => setAbout(true) },
    ];
    openContextMenu(e.clientX, e.clientY, items);
  };

  const openIconMenu = (e: React.MouseEvent, d: IconDef): void => {
    e.preventDefault();
    e.stopPropagation();
    setSelected(new Set([d.id]));
    setAnchor(visibleDefs.findIndex((v) => v.id === d.id));
    const isMember = hiddenIds.has(d.id);
    const items: MenuItem[] = [{ label: t("desktopOpen"), onClick: () => openItem(d) }];
    if ("third" in d) {
      items.push({ label: t("runAsAdmin"), onClick: () => void runAsAdmin(d.third) });
    }
    if ("shelfId" in d && d.shelf.linkedPath) {
      items.push({
        label: t("shelfOpenInExplorer"),
        onClick: () => void openSystemWindow("explorer", d.shelf.linkedPath),
      });
    }
    items.push({ separator: true });
    if ("third" in d || "shelfId" in d) {
      items.push({ label: t("changeIcon"), onClick: () => void changeIcon(d) });
      const hasCustom = ("third" in d && d.third.icon) || ("shelfId" in d && d.shelf.icon);
      if (hasCustom) items.push({ label: t("clearIcon"), onClick: () => void clearIcon(d) });
      items.push({
        label: t("rename"),
        onClick: () => {
          if ("third" in d) void renameThird(d.third);
          else void renameShelf(d.shelfId);
        },
      });
    }
    items.push({ separator: true });
    items.push({ label: t("cut"), onClick: () => setClip({ ids: [d.id], mode: "cut" }) });
    items.push({ label: t("copy"), onClick: () => setClip({ ids: [d.id], mode: "copy" }) });
    if (!("sys" in d)) {
      const l = layout;
      const shelfItems: MenuItem[] = Object.entries(l.shelves)
        .filter(([sid]) => {
          if (sid === d.id) return false;
          if (d.id.startsWith("shelf-") && shelfWouldCycle(l, d.id, sid)) return false;
          return true;
        })
        .map(([sid, s]) => ({
          label: s.name,
          onClick: () => moveToShelf([d.id], sid),
        }));
      shelfItems.push({ separator: true }, { label: t("newShelf"), onClick: () => {
        createShelf();
        // 创建后立即归入刚创建的架（同步取最新 shelves）
        window.setTimeout(() => {
          const newest = Object.entries(layoutRef.current.shelves).sort((a, b) => (a[0] < b[0] ? 1 : -1))[0];
          if (newest) moveToShelf([d.id], newest[0]);
        }, 0);
      } });
      items.push({ label: t("moveToShelf"), children: shelfItems });
    }
    if (isMember) items.push({ label: t("shelfMoveOut"), onClick: () => moveOutOfShelf(d.id) });
    items.push({ separator: true });
    if ("third" in d) items.push({ label: t("tpRemove"), danger: true, onClick: () => void removeThird(d.third) });
    if ("shelfId" in d) {
      items.push({
        label: t("shelfDelete"),
        danger: true,
        onClick: () =>
          void askConfirm({
            title: t("shelfDelete"),
            body: t("shelfDeleteBody", { name: d.shelf.name }),
            danger: true,
            okLabel: t("shelfDelete"),
          }).then((ok) => {
            if (ok) deleteShelf(d.shelfId);
          }),
      });
    }
    items.push({ label: t("properties"), onClick: () => setPropsFor(d.id) });
    openContextMenu(e.clientX, e.clientY, items);
  };

  // ---- 飞出面板数据 ----
  const resolveFly = useCallback(
    (id: string): FlyItem | null => {
      const d = defById.get(id);
      if (!d) return null;
      if ("sys" in d) return { id: d.id, label: labelOf(d), icon: d.icon, hue: d.hue, kind: "sys" };
      if ("third" in d) return { id: d.id, label: labelOf(d), icon: d.icon, img: d.third.icon, hue: d.hue, kind: "third" };
      if ("shelfId" in d) return { id: d.id, label: labelOf(d), icon: d.icon, img: d.shelf.icon, hue: d.hue, kind: "shelf" };
      return { id: d.id, label: labelOf(d), icon: d.icon, hue: d.hue, kind: "app" };
    },
    [defById, labelOf],
  );

  const flyoutDef = flyoutId ? defById.get(flyoutId) : null;
  const flyoutCell = flyoutId ? placed.get(flyoutId) : null;
  const flyoutAnchor =
    flyoutDef && flyoutCell
      ? { left: GRID_PAD + (flyoutCell.c + 1) * tier.w + 4, top: GRID_PAD + flyoutCell.r * tier.h }
      : null;

  const propsDef = propsFor ? defById.get(propsFor) : null;

  return (
    <div
      ref={containerRef}
      className={`desktop-icons${edit ? " edit-mode" : ""}`}
      onKeyDown={onKeyDown}
      tabIndex={-1}
      onPointerDown={onContainerPointerDown}
      onPointerMove={onContainerPointerMove}
      onPointerUp={onContainerPointerUp}
      onContextMenu={(e) => {
        if (e.target === e.currentTarget) openDesktopMenu(e);
      }}
    >
      {visibleDefs.map((d, idx) => {
        const cell = placed.get(d.id);
        if (!cell) return null;
        const isDragging = drag?.moved === true && d.id === drag.primary;
        const isCut = clip?.mode === "cut" && clip.ids.includes(d.id);
        const Icon = d.icon;
        const img = "third" in d ? d.third.icon : "shelfId" in d ? d.shelf.icon : null;
        const badge = "shelfId" in d ? layout.shelves[d.shelfId]?.members.length ?? 0 : null;
        return (
          <button
            key={d.id}
            type="button"
            className={`desktop-icon${selected.has(d.id) ? " selected" : ""}${isDragging ? " dragging" : ""}${isCut ? " cut" : ""}${edit ? " jiggle" : ""}`}
            style={{
              ["--i" as string]: String(Math.min(idx, 10)), // 批次A：入场交错淡入序号
              left: GRID_PAD + cell.c * tier.w,
              top: GRID_PAD + cell.r * tier.h,
              width: tier.w,
              height: tier.h,
              transform: isDragging ? `translate(${drag!.dx}px, ${drag!.dy}px)` : undefined,
            }}
            onPointerDown={(e) => onIconPointerDown(e, d, idx)}
            onPointerMove={onIconPointerMove}
            onPointerUp={onIconPointerUp}
            onClick={() => {
              if (!edit) setSelected(new Set([d.id]));
            }}
            onDoubleClick={() => {
              if (!edit) openItem(d);
            }}
            onContextMenu={(e) => openIconMenu(e, d)}
            title={labelOf(d)}
          >
            <span
              className="desktop-icon-tile"
              style={{ ["--hue" as string]: String(d.hue), ["--tile" as string]: `${tier.tile}px`, ["--icon" as string]: `${tier.icon}px` }}
            >
              {img ? <img src={img} alt="" draggable={false} /> : <Icon size={tier.icon} strokeWidth={1.6} />}
              {badge !== null && badge > 0 && <span className="icon-badge">{badge > 99 ? "99+" : badge}</span>}
            </span>
            <span className="desktop-icon-label">{labelOf(d)}</span>
            {edit && ("third" in d || "shelfId" in d) && (
              <span
                className="icon-remove"
                role="button"
                aria-label={t("delete")}
                onPointerDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  if ("third" in d) void removeThird(d.third);
                  else
                    void askConfirm({
                      title: t("shelfDelete"),
                      body: t("shelfDeleteBody", { name: d.shelf.name }),
                      danger: true,
                      okLabel: t("shelfDelete"),
                    }).then((ok) => {
                      if (ok) deleteShelf(d.shelfId);
                    });
                }}
              >
                <X size={12} />
              </span>
            )}
          </button>
        );
      })}

      {marquee && (
        <div
          className="marquee"
          style={{
            left: Math.min(marquee.x0, marquee.x1),
            top: Math.min(marquee.y0, marquee.y1),
            width: Math.abs(marquee.x1 - marquee.x0),
            height: Math.abs(marquee.y1 - marquee.y0),
          }}
        />
      )}

      {edit && (
        <div className="edit-done" onPointerDown={(e) => e.stopPropagation()}>
          <button type="button" className="btn ghost" onClick={() => setEdit(false)}>
            {t("editModeDone")}
          </button>
        </div>
      )}

      {flyoutDef && "shelfId" in flyoutDef && flyoutAnchor && (
        <ShelfFlyout
          shelfId={flyoutDef.shelfId}
          layout={layout}
          resolve={resolveFly}
          anchorPx={flyoutAnchor}
          onClose={() => setFlyoutId(null)}
          onOpenItem={(id) => {
            const d = defById.get(id);
            if (d) {
              if ("shelfId" in d) setFlyoutId(d.shelfId);
              else openItem(d);
            }
          }}
          onMoveOut={(id) => moveOutOfShelf(id)}
          onRemoveThird={(id) => {
            const d = defById.get(id);
            if (d && "third" in d) void removeThird(d.third);
          }}
          onOpenExplorer={(path) => void openSystemWindow("explorer", path)}
          onRename={(id) => void renameShelf(id)}
          onChangeIcon={(id) => {
            const d = defById.get(id);
            if (d) void changeIcon(d);
          }}
          onDelete={(id) => deleteShelf(id)}
          onSetColor={(id, color) => {
            const l = layoutRef.current;
            const s = l.shelves[id];
            if (s) commit({ ...l, shelves: { ...l.shelves, [id]: { ...s, color } } });
          }}
          onLinkFolder={(id) => void linkShelfFolder(id)}
          onClearLink={(id) => {
            const l = layoutRef.current;
            const s = l.shelves[id];
            if (s) {
              const { linkedPath: _drop, ...rest } = s;
              commit({ ...l, shelves: { ...l.shelves, [id]: rest } });
            }
          }}
        />
      )}

      {propsDef && (
        <div className="props-overlay" onPointerDown={() => setPropsFor(null)}>
          <div className="props-card" onPointerDown={(e) => e.stopPropagation()} onClick={(e) => e.stopPropagation()}>
            <header className="props-head">
              <span className="props-title">{t("properties")}</span>
              <button type="button" className="shf-x" onClick={() => setPropsFor(null)} aria-label={t("close")}>
                <X size={14} />
              </button>
            </header>
            <dl className="props-body">
              <dt>{t("propName")}</dt>
              <dd>{labelOf(propsDef)}</dd>
              <dt>{t("propType")}</dt>
              <dd>
                {"sys" in propsDef
                  ? t("propTypeSys")
                  : "third" in propsDef
                    ? t("propTypeThird")
                    : "shelfId" in propsDef
                      ? t("propTypeShelf")
                      : t("propTypeApp")}
              </dd>
              {"third" in propsDef && (
                <>
                  <dt>{t("propPath")}</dt>
                  <dd className="mono">{propsDef.third.path}</dd>
                  <dt>{t("propAdded")}</dt>
                  <dd>{new Date(propsDef.third.addedAt).toLocaleString()}</dd>
                </>
              )}
              {"shelfId" in propsDef && propsDef.shelf.linkedPath && (
                <>
                  <dt>{t("propPath")}</dt>
                  <dd className="mono">{propsDef.shelf.linkedPath}</dd>
                </>
              )}
              <dt>{t("propPos")}</dt>
              <dd>
                {(() => {
                  const c = placed.get(propsDef.id);
                  return c ? `(${c.c + 1}, ${c.r + 1})` : "—";
                })()}
              </dd>
            </dl>
          </div>
        </div>
      )}

      {about && (
        <div className="props-overlay" onPointerDown={() => setAbout(false)}>
          <div className="props-card about" onPointerDown={(e) => e.stopPropagation()} onClick={(e) => e.stopPropagation()}>
            <div className="wizard-brand small">VARIABLE</div>
            <p className="dim small" style={{ textAlign: "center", margin: "6px 0 0" }}>
              {t("aboutBody")}
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
