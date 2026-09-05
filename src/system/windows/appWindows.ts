import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { desktopAppLabel } from "../desktop-icons/DesktopIcons";
import type { AppMode } from "../../state/uiStore";

/**
 * M4 拆窗 — 软件窗口管理：
 * - 桌面环境按需创建/聚焦四款独立软件窗口（label: app-write / app-mind / app-code / app-fate）
 * - 已存在 → 取消最小化并聚焦（不重复创建）
 * - 位置/尺寸记忆：localStorage 存逻辑像素（物理/缩放比换算），创建时按桌面窗口显示器缩放还原
 * - 卸载某软件不影响其他软件；窗口生命周期归各窗口自身（红灯 = 关自己）
 */

const LS_KEY = "variable:win:geom:v1";

interface Geom {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function appWindowLabel(app: AppMode): string {
  // mind 的窗口/入口名是 app-mind（vite 入口、托盘、桌面图标一致），而非 app-mindmap
  return app === "mindmap" ? "app-mind" : `app-${app}`;
}

function loadGeom(label: string): Geom | null {
  try {
    const all = JSON.parse(localStorage.getItem(LS_KEY) ?? "{}") as Record<string, Geom>;
    const g = all[label];
    if (g && Number.isFinite(g.x) && Number.isFinite(g.y) && g.width > 0 && g.height > 0) return g;
  } catch {
    /* corrupted → defaults */
  }
  return null;
}

function saveGeom(label: string, g: Geom): void {
  try {
    const all = JSON.parse(localStorage.getItem(LS_KEY) ?? "{}") as Record<string, Geom>;
    all[label] = g;
    localStorage.setItem(LS_KEY, JSON.stringify(all));
  } catch {
    /* storage full/blocked → geometry won't persist */
  }
}

/** 打开（或聚焦）一款独立软件窗口。由桌面环境调用。 */
export async function openAppWindow(app: AppMode): Promise<void> {
  const label = appWindowLabel(app);
  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    void existing.unminimize().catch(() => {});
    void existing.setFocus().catch(() => {});
    return;
  }
  const geom = loadGeom(label);
  new WebviewWindow(label, {
    url: `${label}.html`,
    title: desktopAppLabel(app),
    ...(geom ? { x: geom.x, y: geom.y, width: geom.width, height: geom.height } : { center: true, width: 1280, height: 800 }),
    minWidth: 960,
    minHeight: 640,
    resizable: true,
    decorations: false,
    // 批次0：与置顶桌面同层（topmost 组内按激活序），保证浮于覆盖桌面之上
    alwaysOnTop: true,
    dragDropEnabled: true,
  }).once("error", (e) => {
    console.error(`[Variable] open ${label} failed`, e);
  });
}

/**
 * M6 系统窗口（文件管理器 / 回收站）。回收站复用 explorer.html + ?view=recycle。
 * 批次B：explorer 支持 ?path= 初始定位（文件架"在文件管理器中打开"）。
 * 已存在窗口仅聚焦（不导航）——多目标定位留待后续事件协议。
 */
export async function openSystemWindow(kind: "explorer" | "recycle", path?: string): Promise<void> {
  const existing = await WebviewWindow.getByLabel(kind);
  if (existing) {
    void existing.unminimize().catch(() => {});
    void existing.setFocus().catch(() => {});
    return;
  }
  const geom = loadGeom(kind);
  const titles: Record<typeof kind, string> = {
    explorer: "Variable 文件管理器",
    recycle: "Variable 回收站",
  };
  const url =
    kind === "recycle"
      ? "explorer.html?view=recycle"
      : path
        ? `explorer.html?path=${encodeURIComponent(path)}`
        : "explorer.html";
  new WebviewWindow(kind, {
    url,
    title: titles[kind],
    ...(geom ? { x: geom.x, y: geom.y, width: geom.width, height: geom.height } : { center: true, width: 1120, height: 720 }),
    minWidth: 820,
    minHeight: 540,
    resizable: true,
    decorations: false,
    // 批次0：与置顶桌面同层（topmost 组内按激活序），保证浮于覆盖桌面之上
    alwaysOnTop: true,
  }).once("error", (e) => {
    console.error(`[Variable] open ${kind} failed`, e);
  });
}

/**
 * 批次C（规格 7.6）：Ctrl+N 新建独立文件管理器窗口（每次新窗口，不聚焦复用）。
 * label 用 explorer-<ts> 前缀区分；几何记忆按各自 label 独立存取。
 */
export async function openExplorerWindow(initialPath?: string): Promise<void> {
  const label = `explorer-${Date.now().toString(36)}`;
  const url = initialPath
    ? `explorer.html?path=${encodeURIComponent(initialPath)}`
    : "explorer.html";
  new WebviewWindow(label, {
    url,
    title: "Variable 文件管理器",
    center: true,
    width: 1120,
    height: 720,
    minWidth: 820,
    minHeight: 540,
    resizable: true,
    decorations: false,
    alwaysOnTop: true,
  }).once("error", (e) => {
    console.error(`[Variable] open ${label} failed`, e);
  });
}

/** 聚焦桌面环境窗口（软件窗口"回到桌面"按钮）。 */
export async function focusDesktop(): Promise<void> {
  const desktop = await WebviewWindow.getByLabel("desktop");
  if (desktop) {
    void desktop.unminimize().catch(() => {});
    void desktop.setFocus().catch(() => {});
  }
}

/** 批次C（规格 5.6.3）：卸载时自动关闭该软件的所有运行窗口。 */
export async function closeAppWindows(app: AppMode): Promise<void> {
  const win = await WebviewWindow.getByLabel(appWindowLabel(app));
  if (win) {
    await win.destroy().catch(() => {});
  }
}

/** 软件窗口自身几何持久化（挂载时调用；返回取消监听函数）。存逻辑像素。 */
export async function trackSelfGeom(label: string): Promise<() => void> {
  const win = getCurrentWindow();
  let timer = 0;
  const schedule = (): void => {
    window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      void (async () => {
        try {
          const [pos, size, sf] = await Promise.all([
            win.outerPosition(),
            win.outerSize(),
            win.scaleFactor(),
          ]);
          saveGeom(label, {
            x: Math.round(pos.x / sf),
            y: Math.round(pos.y / sf),
            width: Math.round(size.width / sf),
            height: Math.round(size.height / sf),
          });
        } catch {
          /* window gone */
        }
      })();
    }, 400);
  };
  const unMoved = await win.onMoved(schedule);
  const unResized = await win.onResized(schedule);
  return () => {
    window.clearTimeout(timer);
    unMoved();
    unResized();
  };
}
