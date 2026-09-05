import type {
  AttachmentView,
  BackupInfo,
  BootstrapInfo,
  DocumentFull,
  DocumentInput,
  EdgeDirection,
  ExportResult,
  Folder,
  ImportSummary,
  LineStyle,
  MindEdge,
  MindNode,
  Mindmap,
  MindmapData,
  NodeShape,
  PathStyle,
  RecoveryEntry,
  SearchHit,
  WsEntry,
} from "./types";
import type * as Pv from "../apps/code/types";

export interface IpcError {
  code: string;
  message: string;
}

function isIpcError(e: unknown): e is IpcError {
  return typeof e === "object" && e !== null && "code" in e && "message" in e;
}

export function errMessage(e: unknown): { code: string; message: string } {
  if (isIpcError(e)) return { code: e.code, message: e.message };
  if (e instanceof Error) return { code: "RUNTIME", message: e.message };
  return { code: "UNKNOWN", message: String(e) };
}

// Lazy import so vitest (pure logic tests) never loads @tauri-apps/api.
async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const mod = await import("@tauri-apps/api/core");
  return mod.invoke<T>(cmd, args);
}

export interface ListFilterT {
  view?: "all" | "favorites" | "trash";
  folderId?: string | null;
  query?: string;
  tag?: string;
  sort?: "updated" | "created";
}

export const ipc = {
  bootstrap: () => invoke<BootstrapInfo>("app_bootstrap"),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  checkPaths: (paths: string[]) => invoke<{ path: string; exists: boolean; kind: string | null }[]>("check_paths_exist", { paths }),
  log: (level: string, message: string) => invoke<void>("log_frontend", { level, message }),
  saveTextFile: (path: string, contents: string, allowOverwrite?: boolean) =>
    invoke<string>("save_text_file", { path, contents, allowOverwrite }),

  listFolders: () => invoke<Folder[]>("list_folders"),
  createFolder: (name: string, parentId: string | null) => invoke<Folder>("create_folder", { name, parentId }),
  renameFolder: (id: string, name: string) => invoke<void>("rename_folder", { id, name }),
  moveFolder: (id: string, newParentId: string | null) => invoke<void>("move_folder", { id, newParentId }),
  trashFolder: (id: string) => invoke<void>("trash_folder", { id }),
  restoreFolder: (id: string) => invoke<void>("restore_folder", { id }),
  purgeFolder: (id: string) => invoke<void>("purge_folder", { id }),

  listDocuments: (filter: ListFilterT) => invoke<import("./types").DocumentMeta[]>("list_documents", { filter }),
  createDocument: (folderId: string | null, title?: string) =>
    invoke<DocumentFull>("create_document", { folderId, title }),
  getDocument: (id: string) => invoke<DocumentFull>("get_document", { id }),
  saveDocument: (input: DocumentInput) => invoke<DocumentFull>("save_document", { input }),
  moveDocument: (id: string, folderId: string | null) => invoke<void>("move_document", { id, folderId }),
  setFavorite: (id: string, favorite: boolean) => invoke<void>("set_document_favorite", { id, favorite }),
  setTags: (id: string, tags: string[]) => invoke<string[]>("set_document_tags", { id, tags }),
  listAllTags: () => invoke<string[]>("list_document_tags"),
  trashDocument: (id: string) => invoke<void>("trash_document", { id }),
  restoreDocument: (id: string) => invoke<void>("restore_document", { id }),
  purgeDocuments: (ids: string[]) => invoke<void>("purge_documents", { ids }),
  emptyTrash: () => invoke<number>("empty_trash"),
  searchAll: (query: string) => invoke<SearchHit[]>("search_all", { query }),

  listMindmaps: () => invoke<Mindmap[]>("list_mindmaps"),
  createMindmap: (name?: string, folderId?: string | null) =>
    invoke<Mindmap>("create_mindmap", { name, folderId }),
  getMindmap: (id: string) => invoke<MindmapData>("get_mindmap", { id }),
  updateMindmap: (u: { id: string; viewportX?: number; viewportY?: number; zoom?: number; gridEnabled?: boolean; snapEnabled?: boolean }) =>
    invoke<void>("update_mindmap", { update: u }),
  renameMindmap: (id: string, name: string) => invoke<void>("rename_mindmap", { id, name }),
  trashMindmap: (id: string) => invoke<void>("trash_mindmap", { id }),
  saveNodes: (nodes: MindNode[]) => invoke<MindNode[]>("save_nodes", { nodes }),
  deleteNodes: (ids: string[]) => invoke<void>("delete_nodes", { ids }),
  /** 批次C（规格 5.7.3）：按 id 查节点版本（引用更新检测）。 */
  nodesVersions: (ids: string[]) => invoke<{ id: string; updated_at: number }[]>("nodes_versions", { ids }),
  saveEdge: (edge: MindEdge) => invoke<MindEdge>("save_edge", { edge }),
  deleteEdges: (ids: string[]) => invoke<void>("delete_edges", { ids }),

  importMedia: (req: { paths: string[]; mode: "copy" | "reference"; documentId?: string | null; nodeId?: string | null }) =>
    invoke<AttachmentView[]>("import_media", { req }),
  importDataUrl: (dataUrl: string, suggestedName?: string) =>
    invoke<AttachmentView>("import_data_url", { dataUrl, suggestedName }),
  listAttachments: (documentId?: string | null, nodeId?: string | null) =>
    invoke<AttachmentView[]>("list_attachments", { documentId, nodeId }),
  resolveMediaPath: (attachmentId: string, newPath: string) =>
    invoke<AttachmentView>("resolve_media_path", { attachmentId, newPath }),
  deleteMedia: (mediaId: string) => invoke<void>("delete_media", { mediaId }),

  getSettings: () => invoke<Record<string, string>>("get_all_settings"),
  setSettings: (entries: Record<string, string>) => invoke<void>("set_settings", { entries }),
  resetUiSettings: () => invoke<void>("reset_ui_settings"),
  writeRecoveryFile: (p: { savedAt: number; title: string; contentHtml: string; contentText: string }) =>
    invoke<string>("write_recovery_file", { payload: p }),
  listRecoveryFiles: () => invoke<RecoveryEntry[]>("list_recovery_files"),
  readRecoveryFile: (id: string) =>
    invoke<{ savedAt: number; title: string; contentHtml: string; contentText: string }>("read_recovery_file", { id }),
  deleteRecoveryFile: (id: string) => invoke<void>("delete_recovery_file", { id }),
  recoverToDocument: (id: string) => invoke<string>("recover_to_document", { id }),

  createBackup: (source?: string) => invoke<BackupInfo>("create_backup", { source }),
  listBackups: () => invoke<BackupInfo[]>("list_backups"),
  restoreBackup: (fileName: string) => invoke<void>("restore_backup", { fileName }),
  deleteBackup: (fileName: string) => invoke<void>("delete_backup", { fileName }),
  exportBackup: (fileName: string, destPath: string) => invoke<string>("export_backup", { fileName, destPath }),

  exportDocuments: (ids: string[], format: "md" | "html" | "txt" | "json", destPath: string) =>
    invoke<ExportResult>("export_documents", { ids, format, destPath }),
  exportMindmapJson: (id: string, destPath: string) => invoke<ExportResult>("export_mindmap_json", { id, destPath }),
  exportWorkspace: (destDir: string) => invoke<ExportResult>("export_workspace", { destDir }),
  importWorkspace: (srcFile: string) => invoke<ImportSummary>("import_workspace", { srcFile }),

  // ---- local workspace folder panel (built-in file management) ----
  wsDefaultDir: () => invoke<string>("ws_default_dir"),
  wsList: (root: string) => invoke<WsEntry[]>("ws_list", { root }),
  wsReadText: (path: string) => invoke<string>("ws_read_text", { path }),
  wsCreateDir: (root: string, parentDir: string, name: string) =>
    invoke<string>("ws_create_dir", { root, parentDir, name }),
  wsRename: (root: string, path: string, newName: string) =>
    invoke<string>("ws_rename", { root, path, newName }),
  wsMove: (root: string, src: string, destDir: string) => invoke<string>("ws_move", { root, src, destDir }),
  wsCopyIn: (root: string, paths: string[], destDir: string) =>
    invoke<string[]>("ws_copy_in", { root, paths, destDir }),
  wsDeleteTrash: (root: string, path: string) => invoke<string>("ws_delete_trash", { root, path }),

  // ---- project visualization engine (spec chapter 2: bounded local scan) ----
  projectScan: (root: string) => invoke<Pv.ProjectScanResult>("project_scan", { root }),
  projectReadFile: (root: string, relPath: string) =>
    invoke<Pv.SourceFile>("project_read_file", { root, relPath }),
  projectReadBytes: (root: string, relPath: string) =>
    invoke<{ relPath: string; bytes: number[]; size: number; truncated: boolean }>("project_read_bytes", { root, relPath }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  writeTextFile: (path: string, contents: string) => invoke<void>("write_text_file", { path, contents }),

  // ---- M5 shell: hardware & privacy (local Windows APIs, zero network) ----
  privacyUsage: () => invoke<Shell.DeviceUsage[]>("privacy_usage"),
  audioGet: () => invoke<Shell.AudioState>("audio_get"),
  audioSet: (volume: number, muted?: boolean) =>
    invoke<Shell.AudioState>("audio_set", { volume, muted: muted ?? null }),
  wifiGet: () => invoke<Shell.WifiState>("wifi_get"),
  bluetoothGet: () => invoke<Shell.BluetoothState>("bluetooth_get"),
  // 批次C 硬件面板（规格 6.1-6.6）：蓝牙开关/设备、Wi-Fi 扫描/断开、音频设备切换、亮度/电池
  bluetoothSet: (enabled: boolean) => invoke<Shell.BluetoothState>("bluetooth_set", { enabled }),
  btDevices: () => invoke<Shell.BtDevice[]>("bt_devices"),
  // 批次E（规格 6.1.3/6.2.1）：Wi-Fi 无线电开关 + 蓝牙设备连接/断开
  wifiSet: (enabled: boolean) => invoke<Shell.WifiState>("wifi_set", { enabled }),
  btConnect: (id: string) => invoke<void>("bt_connect", { id }),
  btDisconnect: (id: string) => invoke<void>("bt_disconnect", { id }),
  wifiScan: () => invoke<Shell.WifiNetwork[]>("wifi_scan"),
  wifiDisconnect: () => invoke<void>("wifi_disconnect"),
  audioDevices: () => invoke<Shell.AudioDeviceInfo[]>("audio_devices"),
  audioSetDefault: (deviceId: string) => invoke<void>("audio_set_default", { deviceId }),
  batteryGet: () => invoke<Shell.BatteryState>("battery_get"),
  brightnessGet: () => invoke<Shell.BrightnessState>("brightness_get"),
  brightnessSet: (level: number) => invoke<Shell.BrightnessState>("brightness_set", { level }),

  // ---- M6 shell: file explorer & global recycle bin (local FS, zero network) ----
  exHome: () => invoke<string>("ex_home"),
  // 批次E（规格 7.2）：Variable 数据目录节点组（侧栏）
  exVariableDirs: () => invoke<Shell.ExVarDir[]>("ex_variable_dirs"),
  exDrives: () => invoke<Shell.ExDrive[]>("ex_drives"),
  exList: (path: string) => invoke<Shell.ExListing>("ex_list", { path }),
  exMkdir: (parent: string, name: string) => invoke<string>("ex_mkdir", { parent, name }),
  exRename: (path: string, newName: string) => invoke<string>("ex_rename", { path, newName }),
  exMove: (src: string, destDir: string, mode?: Shell.ExCopyMode) =>
    invoke<string>("ex_move", { src, destDir, mode: mode ?? null }),
  exCopy: (src: string, destDir: string, mode?: Shell.ExCopyMode) =>
    invoke<string>("ex_copy", { src, destDir, mode: mode ?? null }),
  exTrash: (paths: string[]) => invoke<string[]>("ex_trash", { paths }),
  // 批次C：搜索 / 冲突 / 彻底删除 / 收藏夹 / 缩略图
  exSearch: (path: string, query: string) => invoke<Shell.ExSearchResult>("ex_search", { path, query }),
  exConflicts: (srcs: string[], destDir: string) => invoke<string[]>("ex_conflicts", { srcs, destDir }),
  exPurge: (paths: string[]) => invoke<number>("ex_purge", { paths }),
  exFavList: () => invoke<string[]>("ex_fav_list"),
  exFavAdd: (path: string) => invoke<string[]>("ex_fav_add", { path }),
  exFavRemove: (path: string) => invoke<string[]>("ex_fav_remove", { path }),
  exThumbnail: (path: string) => invoke<string>("ex_thumbnail", { path }),
  recList: () => invoke<Shell.RecItem[]>("rec_list"),
  recRestore: (id: string, source: string) => invoke<void>("rec_restore", { id, source }),
  recPurge: (id: string, source: string) => invoke<void>("rec_purge", { id, source }),
  recEmpty: () => invoke<number>("rec_empty"),
  recCount: () => invoke<number>("rec_count"),

  // ---- M7 shell: third-party launcher (independent OS processes, zero network) ----
  tpAdd: (path: string, name?: string, grade?: string) =>
    invoke<Shell.ThirdApp>("tp_add", { path, name: name ?? null, grade: grade ?? null }),
  tpList: () => invoke<Shell.ThirdApp[]>("tp_list"),
  tpRemove: (id: string) => invoke<void>("tp_remove", { id }),
  tpPurge: (id: string) => invoke<void>("tp_purge", { id }),
  tpSetGrade: (id: string, grade: string) => invoke<Shell.ThirdApp>("tp_set_grade", { id, grade }),
  tpRename: (id: string, name: string) => invoke<Shell.ThirdApp>("tp_rename", { id, name }),
  tpLaunch: (id: string) => invoke<void>("tp_launch", { id }),
  // 批次B：自定义图标（.ico/.png ≤512KB → base64 dataURL 存登记表）+ 以管理员运行
  tpSetIcon: (id: string, iconPath: string | null) => invoke<Shell.ThirdApp>("tp_set_icon", { id, iconPath }),
  // 批次E（规格 5.9.2/5.9.3）：开始菜单扫描 + 便携化
  tpScanStartMenu: () => invoke<Shell.TpScanCandidate[]>("tp_scan_start_menu"),
  tpPortableize: (id: string) => invoke<Shell.ThirdApp>("tp_portableize", { id }),
  tpLaunchAdmin: (id: string) => invoke<void>("tp_launch_admin", { id }),
  iconDataurl: (path: string) => invoke<string>("icon_dataurl", { path }),

  // ---- 批次C: 运行态检测 + 预装软件卸载（规格 5.5-5.6） ----
  tpRunning: () => invoke<string[]>("tp_running"),
  officialUsage: (app: string) => invoke<Shell.OfficialUsage>("official_usage", { app }),
  officialPurge: (app: string) => invoke<void>("official_purge", { app }),

  // ---- M8 shell: USB full portability (pack / verify / removal watcher) ----
  usbStatus: () => invoke<Shell.UsbStatus>("usb_status"),
  usbPack: (target: string) => invoke<string>("usb_pack", { target }),
  usbVerify: (dir: string) => invoke<Shell.FileCheck[]>("usb_verify", { dir }),

  // ---- 批次0: 联网确认策略存储（默认零联网；任何联网前必须经 netGuard 确认） ----
  netConsentCheck: (host: string) => invoke<Shell.NetPolicy | null>("net_consent_check", { host }),
  netConsentSet: (host: string, policy: Shell.NetPolicy) => invoke<void>("net_consent_set", { host, policy }),

  // ---- 批次D: 桌面窗口管理（红绿灯补全）+ 任务栏小组件（本地 sysinfo，零网络） ----
  winSetAvoidTaskbar: (avoid: boolean) => invoke<void>("win_set_avoid_taskbar", { avoid }),
  winHideToTray: () => invoke<void>("win_hide_to_tray"),
  sysBrief: () => invoke<Shell.SysBrief>("sys_brief"),
  sysDisks: () => invoke<Shell.SysDisk[]>("sys_disks"),

  // ---- 批次E: 任务栏/开始菜单增强（本机用户名 + 电源操作，零网络） ----
  sysUser: () => invoke<string>("sys_user"),
  netIp: () => invoke<string | null>("net_ip"),
  powerAction: (action: "lock" | "logoff" | "reboot" | "shutdown") =>
    invoke<void>("power_action", { action }),
  // 批次E（规格 4.7）：整表应用快捷键（unregister_all → 重注册），返回注册失败的 accel
  shortcutsApply: (binds: { action: string; accel: string }[]) =>
    invoke<string[]>("shortcuts_apply", { binds }),

  // ---- 批次E-6: 壁纸细节（IDesktopWallpaper 多显示器 + 本地缓存每日换，零网络） ----
  wpMonitors: () => invoke<Shell.WpMonitor[]>("wp_monitors"),
  wpSetMonitor: (monitor: string, path: string) => invoke<void>("wp_set_monitor", { monitor, path }),
  wpPickDaily: (dir: string, mode: "date" | "next") =>
    invoke<string | null>("wp_pick_daily", { dir, mode }),

  // ---- 批次E-7: 数据隐私（保险箱 AES-256-GCM / 焚毁 / 自检，全部本机） ----
  vaultStatus: () => invoke<Shell.VaultStatus>("vault_status"),
  vaultInit: (password: string) => invoke<void>("vault_init", { password }),
  vaultUnlock: (password: string) => invoke<void>("vault_unlock", { password }),
  vaultLock: () => invoke<void>("vault_lock"),
  vaultImport: (path: string, shredSource: boolean) =>
    invoke<Shell.VaultItem>("vault_import", { path, shredSource }),
  vaultList: () => invoke<Shell.VaultItem[]>("vault_list"),
  vaultExport: (name: string, destDir: string) =>
    invoke<string>("vault_export", { name, destDir }),
  vaultDestroy: (name: string) => invoke<void>("vault_destroy", { name }),
  privacyShred: (path: string) => invoke<void>("privacy_shred", { path }),
  privacyAudit: () => invoke<Shell.AuditFinding[]>("privacy_audit"),
};

/** Shell 命令的返回结构（与 src-tauri/src/shell/hardware.rs 序列化字段一一对应）。 */
namespace Shell {
  export interface DeviceUsage {
    kind: "microphone" | "webcam";
    app: string;
  }
  export interface AudioState {
    volume: number;
    muted: boolean;
  }
  export interface WifiState {
    connected: boolean;
    ssid: string | null;
    signal: number | null;
    /** 批次E（规格 6.2.1）：无线电开关状态（null = 读取失败/无无线电）。 */
    radio_on: boolean | null;
  }
  export interface BluetoothState {
    available: boolean;
    enabled: boolean;
  }
  /** 批次C（规格 6.1.2）+ 批次E（id 供连接/断开操作）：已配对蓝牙设备 + 连接状态。 */
  export interface BtDevice {
    name: string;
    id: string;
    connected: boolean;
  }
  /** 批次C（规格 6.2.2）：扫描到的 Wi-Fi 网络（仅用户点击"扫描"时获取）。 */
  export interface WifiNetwork {
    ssid: string;
    signal: number;
    secured: boolean;
  }
  /** 批次C（规格 6.3.1）：音频端点（render=输出 / capture=输入）。 */
  export interface AudioDeviceInfo {
    id: string;
    name: string;
    kind: "render" | "capture";
    default: boolean;
  }
  /** 批次C（规格 6.6.2）：电池状态（percent/lifetimeSecs 未知 = null）。 */
  export interface BatteryState {
    hasBattery: boolean;
    acOnline: boolean;
    percent: number | null;
    lifetimeSecs: number | null;
  }
  /** 批次C（规格 6.6.1）：屏幕亮度（supported=false = 台式机/外接屏，不伪造可调）。 */
  export interface BrightnessState {
    supported: boolean;
    level: number;
  }
  export interface ExEntry {
    name: string;
    path: string;
    kind: "dir" | "file";
    ext: string | null;
    size: number;
    updatedAt: number;
    createdAt: number;
    hidden: boolean;
  }
  export interface ExListing {
    path: string;
    parent: string | null;
    entries: ExEntry[];
  }
  /** 批次C：目录内搜索结果（规格 7.4.3）。 */
  export interface ExSearchResult {
    entries: ExEntry[];
    scanned: number;
    truncated: boolean;
  }
  /** 批次C：复制/移动冲突解决（规格 7.7）。replace=覆盖，keep=保留两者，缺省=自动后缀。 */
  export type ExCopyMode = "replace" | "keep";
  export interface ExDrive {
    letter: string;
    path: string;
  }
  export type RecSource = "doc" | "folder" | "mindmap" | "ws-file" | "fs-item";
  export interface RecItem {
    id: string;
    source: RecSource;
    title: string;
    origin: string | null;
    deletedAt: number;
    kind: "file" | "dir" | "doc" | "folder" | "mindmap";
    size: number;
  }
  export type TpGrade = "portable" | "standalone" | "shortcut";
  /** 批次E-6：显示器（IDesktopWallpaper 视角，id 用于 SetWallpaper）。 */
  export interface WpMonitor {
    id: string;
    primary: boolean;
    x: number;
    y: number;
    width: number;
    height: number;
  }
  /** 批次E-7：隐私保险箱状态。 */
  export interface VaultStatus {
    initialized: boolean;
    unlocked: boolean;
    count: number;
    bytes: number;
  }
  /** 批次E-7：保险箱条目（明文名 + 明文大小；内容密文落盘）。 */
  export interface VaultItem {
    name: string;
    size: number;
    addedAt: number;
  }
  /** 批次E-7：隐私自检发现项（level = pass | warn）。 */
  export interface AuditFinding {
    id: string;
    level: "pass" | "warn";
    detail: string;
  }
  /** 批次E（规格 7.2）：Variable 数据目录侧栏节点。 */
  /** 批次E（规格 5.9.2）：开始菜单扫描候选。 */
  export interface TpScanCandidate {
    name: string;
    lnk: string;
    target: string;
  }
  export interface ExVarDir {
    key: "root" | "workspace" | "apps" | "recycle";
    path: string;
  }
  export interface ThirdApp {
    id: string;
    name: string;
    path: string;
    grade: TpGrade;
    addedAt: number;
    lastLaunch: number | null;
    /** 批次B：自定义图标（data URL；null = 使用默认占位图标）。 */
    icon: string | null;
    /** 批次E（规格 5.9.4）：.lnk 解析出的目标 exe；非 lnk 登记为 null。 */
    target: string | null;
  }
  export interface UsbStatus {
    portable: boolean;
    dataDir: string;
    driveRemovable: boolean;
    manifestExists: boolean;
  }
  export interface FileCheck {
    path: string;
    ok: boolean;
    expected: string;
    actual: string;
    size: number;
  }
  export interface PackProgress {
    phase: "collect" | "copy" | "exe" | "manifest" | "done";
    done: number;
    total: number;
    current: string | null;
  }
  export type NetPolicy = "allow" | "deny";
  export interface NetConsentEntry {
    host: string;
    policy: NetPolicy;
    updatedAt: number;
  }
  /** 批次C：预装软件数据占用（bytes=null = 数据在工作区，无法按软件切分）。 */
  export interface OfficialUsage {
    items: number;
    bytes: number | null;
    purgeable: boolean;
  }
  /** 批次D：CPU/内存简报（sys_brief；cpu=百分比，mem 为字节）。 */
  export interface SysBrief {
    cpu: number;
    memUsed: number;
    memTotal: number;
  }
  /** 批次D：本地盘符容量（sys_disks）。 */
  export interface SysDisk {
    letter: string;
    path: string;
    total: number;
    free: number;
  }
}

// 供外部模块 import type 使用（namespace 不导出，这里做类型别名导出）。
export type DeviceUsage = Shell.DeviceUsage;
export type AudioState = Shell.AudioState;
export type WifiState = Shell.WifiState;
export type BluetoothState = Shell.BluetoothState;
export type BtDevice = Shell.BtDevice;
export type WifiNetwork = Shell.WifiNetwork;
export type AudioDeviceInfo = Shell.AudioDeviceInfo;
export type BatteryState = Shell.BatteryState;
export type BrightnessState = Shell.BrightnessState;
export type ExEntry = Shell.ExEntry;
export type ExListing = Shell.ExListing;
export type ExSearchResult = Shell.ExSearchResult;
export type ExCopyMode = Shell.ExCopyMode;
export type ExDrive = Shell.ExDrive;
export type ExVarDir = Shell.ExVarDir;
export type TpScanCandidate = Shell.TpScanCandidate;
export type RecSource = Shell.RecSource;
export type RecItem = Shell.RecItem;
export type TpGrade = Shell.TpGrade;
export type ThirdApp = Shell.ThirdApp;
export type UsbStatus = Shell.UsbStatus;
export type FileCheck = Shell.FileCheck;
export type PackProgress = Shell.PackProgress;
export type NetPolicy = Shell.NetPolicy;
export type NetConsentEntry = Shell.NetConsentEntry;
export type OfficialUsage = Shell.OfficialUsage;
export type SysBrief = Shell.SysBrief;
export type SysDisk = Shell.SysDisk;
export type WpMonitor = Shell.WpMonitor;
export type VaultStatus = Shell.VaultStatus;
export type VaultItem = Shell.VaultItem;
export type AuditFinding = Shell.AuditFinding;

export type EdgeStylePatch = Partial<Pick<MindEdge, "direction" | "lineStyle" | "pathStyle" | "color" | "width" | "label" | "animated">>;
export type ShapeKind = NodeShape;
export type Dir = EdgeDirection;
export type LStyle = LineStyle;
export type PStyle = PathStyle;
