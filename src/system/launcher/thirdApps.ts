import { errMessage, ipc, type ThirdApp } from "../../lib/ipc";
import { uiStore } from "../../state/uiStore";
import { createStore, useStore } from "../../lib/store";

/**
 * M7 第三方软件登记（桌面窗口内共享状态）：
 * - 单一数据源 tpStore；DesktopIcons / StartMenu / LauncherManager 共用
 * - 由 DesktopShell 挂载时加载一次，增删改后调用 reloadThirdApps()
 * - 无法捕获的启动 = 软件按独立窗口运行，静默让路（M2/R9，不弹占位卡不弹 toast）
 */

const tpStore = createStore<{ apps: ThirdApp[] }>({ apps: [] });

export async function reloadThirdApps(): Promise<void> {
  try {
    const apps = await ipc.tpList();
    tpStore.setState({ apps });
    // 批次E-16：未自定义图标的第三方应用自动提取 Windows 原生图标
    // （实机反馈升级：128px 高清 + 后端持久化，一次成本不再每会话重提）
    void fillNativeIcons(apps);
  } catch (e) {
    console.warn("[launcher] tp_list failed", errMessage(e).message);
  }
}

/**
 * 为缺少图标的登记项批量补齐 128px 高清图标（实机反馈：图标清晰度不够）。
 * 后端 tp_ensure_icons 提取并持久化到登记表（每批 ≤8 个防单命令过长）；
 * 有补齐才重载一次列表。失败项保持占位图标（诚实降级）。
 */
async function fillNativeIcons(apps: ThirdApp[]): Promise<void> {
  const missing = apps.filter((a) => !a.icon).map((a) => a.id);
  if (missing.length === 0) return;
  const CHUNK = 8;
  let changed = 0;
  for (let i = 0; i < missing.length; i += CHUNK) {
    const ids = missing.slice(i, i + CHUNK);
    try {
      changed += await ipc.tpEnsureIcons(ids);
    } catch (e) {
      console.warn("[launcher] tp_ensure_icons failed", errMessage(e).message);
    }
  }
  if (changed > 0) {
    try {
      tpStore.setState({ apps: await ipc.tpList() });
    } catch {
      /* 重载失败保持现状（下次挂载再补） */
    }
  }
}

export function useThirdApps(): ThirdApp[] {
  return useStore(tpStore, (s) => s.apps);
}

/** 批次D：非响应式读取（Win+数字 快速启动用）。 */
export function getThirdApps(): ThirdApp[] {
  return tpStore.getState().apps;
}

/**
 * 批次E-16：第三方应用一律在环境内打开 —— 先开虚拟窗口（占位），
 * 再由后端启动并把原生窗口 SetParent 嵌进来（从任务栏/Alt+Tab 消失）。
 * 批次W-1：占位窗口实例 id 作为 embed_id 传给后端注册中心（多嵌入并发，
 * 每次启动独立进程一一对应新虚拟窗口）。
 * 无法嵌入（UWP/管理员权限等）→ 软件按独立窗口正常运行，静默关闭占位窗口。
 */
export async function launchThirdApp(id: string, _name: string, arg?: string): Promise<void> {
  // 阶段6（三体 AI-2 S3.7）：engine 通道软件走拉起协议（占位卡 + 唤醒 + 就绪自动开）。
  // 引擎开关关闭或设置读取失败 → 既有嵌入路径诚实回落（不静默丢通道语义）。
  const appMeta = getThirdApps().find((a) => a.id === id);
  if (appMeta?.channel === "engine") {
    try {
      const { loadSettings } = await import("../../lib/settings");
      const settings = await loadSettings();
      if (settings.engineEnabled) {
        const { requestEngineApp } = await import("../engine/engineSessions");
        requestEngineApp(id);
        return;
      }
    } catch {
      /* 设置读取失败 → 既有嵌入路径 */
    }
  }
  const { openVwmTpNew, closeVwmWin } = await import("../windows/vwm");
  const { setEmbedMeta } = await import("../windows/embedState");
  const tpApp = `tp:${id}` as Parameters<typeof openVwmTpNew>[0];
  const winId = openVwmTpNew(tpApp);
  try {
    // 批次B-27：arg = 文件关联「打开方式」传入的文件路径（普通启动为空）
    const r = await ipc.embedLaunch(id, winId, arg);
    if (r.reason === "steam:handoff") {
      // R4-B2 修复：Steam .url 快捷方式已转交收编看护通道（steam:// →
      // embed://popup），看护会另开占位窗收编；本占位窗立即关闭避免双窗。
      closeVwmWin(winId);
      return;
    }
    if (r.attached) {
      setEmbedMeta(winId, { tpId: id, rootPid: r.rootPid ?? 0 });
    } else {
      // M2（R9）：捕获失败 = 软件按独立窗口在系统桌面正常运行（不是错误）。
      // Variable 静默让路：直接关闭占位窗，不弹占位卡、不弹 toast（用户 R7 硬约束）。
      closeVwmWin(winId);
    }
  } catch {
    // M2（R9）：embed_launch 本身失败（如目标已卸载）—— 同样静默关占位窗。
    closeVwmWin(winId);
  }
}

/** 管理器开关（桌面窗口内渲染 LauncherManager 模态）。tab: 第三方 / 已安装软件。 */
export function openLauncherManager(tab: "third" | "installed" = "third"): void {
  uiStore.setState({ launcherOpen: true, launcherTab: tab });
}

// ---------- 批次F：软件收件箱（专属文件夹，环境启动自动登记） ----------

/** 收件箱导入结果（added = 本次新增登记数；path = 收件箱绝对路径）。 */
export interface InboxImportResult {
  added: number;
  path: string;
}

/**
 * 批次F：导入软件收件箱（<数据目录>\SoftwareInbox）。
 * - 后端负责扫描/去重/登记/128px 图标提取，此处只编排：
 *   有新增才重载列表（桌面/开始菜单/任务栏即时可见）
 * - 失败向上抛（调用方 toast 兜底）；无新增静默（启动路径不制造噪音）
 */
export async function importSoftwareInbox(): Promise<InboxImportResult> {
  const r = await ipc.tpInboxImport();
  if (r.added > 0) {
    try {
      await reloadThirdApps();
    } catch {
      /* 重载失败不影响计数（下次挂载再补） */
    }
  }
  return { added: r.added, path: r.path };
}

// ---------- 批次F：拖入软件文件夹 → 智能扫描自动登记 ----------

/** 拖放路径分类（纯逻辑，可单测）：exe/lnk/bat/cmd 直接登记，其余交给文件夹扫描。 */
export function classifyDropPaths(paths: string[]): { files: string[]; rest: string[] } {
  const files: string[] = [];
  const rest: string[] = [];
  for (const p of paths) {
    if (/\.(exe|lnk|bat|cmd)$/i.test(p)) files.push(p);
    else rest.push(p);
  }
  return { files, rest };
}

/** 文件夹登记结果（found = 候选 exe 总数；skipped = 找到但未达推荐线的辅助组件数）。 */
export interface FolderRegisterResult {
  isFolder: boolean;
  added: number;
  found: number;
}

/**
 * 批次F：扫描拖入的软件文件夹并自动登记推荐候选（主程序）。
 * - 显示名用 FileDescription（任意语言软件的本地化名称）
 * - 后端已过滤卸载器/更新器等辅助进程并按主程序可能性排序
 * - 普通文件（isFolder=false）如实返回，调用方静默忽略
 * - 单个登记失败不中断批次（目标可能被移动/锁定），如实计数
 */
export async function registerDroppedFolder(dir: string): Promise<FolderRegisterResult> {
  const report = await ipc.tpScanFolder(dir);
  if (!report.isFolder) {
    return { isFolder: false, added: 0, found: 0 };
  }
  const picked = report.candidates.filter((c) => c.recommended);
  let added = 0;
  for (const c of picked) {
    try {
      await ipc.tpAdd(c.path, c.name);
      added++;
    } catch (e) {
      console.warn("[launcher] folder tp_add failed", c.path, errMessage(e).message);
    }
  }
  return { isFolder: true, added, found: report.candidates.length };
}

// ---------- 批次C（规格 5.5）：任务栏固定 ----------

const PINS_KEY = "variable:taskbar:pins:v1";

function loadPins(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(PINS_KEY) ?? "[]") as unknown;
    if (Array.isArray(raw)) return raw.filter((x): x is string => typeof x === "string");
  } catch {
    /* corrupted → defaults */
  }
  return [];
}

export function useTaskbarPins(): string[] {
  return useStore(pinStore, (s) => s.pins);
}

const pinStore = createStore<{ pins: string[] }>({ pins: loadPins() });

function savePins(pins: string[]): void {
  pinStore.setState({ pins });
  try {
    localStorage.setItem(PINS_KEY, JSON.stringify(pins));
  } catch {
    /* storage full/blocked */
  }
}

export function toggleTaskbarPin(id: string): void {
  const cur = pinStore.getState().pins;
  savePins(cur.includes(id) ? cur.filter((p) => p !== id) : [...cur, id]);
}
