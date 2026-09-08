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
  // AI-06 输入手感组（V-61/V-69）：鼠标参数读/写回/回滚 + 指针临时降速
  mouseParamsGet: () => invoke<Shell.MouseParamsState>("mouse_params_get"),
  mouseParamsWrite: (p: { speed: number; doubleClickMs: number; wheelLines: number; swapButtons: boolean }) =>
    invoke<Shell.MouseParamsState>("mouse_params_write", p),
  mouseParamsRollback: () => invoke<boolean>("mouse_params_rollback"),
  pointerSpeedTemp: (ratio: number) => invoke<void>("pointer_speed_temp", { ratio }),
  pointerSpeedRestore: () => invoke<void>("pointer_speed_restore"),
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

  // ---- AI-09 文件操作组（M-21/Z-29..Z-35/M-19..M-27）----
  checksum: (path: string, algo: string, opId: string) =>
    invoke<Shell.ChecksumResult>("checksum", { path, algo, opId }),
  checksumCancel: (opId: string) => invoke<void>("checksum_cancel", { opId }),
  dupeScan: (path: string, minSize?: number | null) =>
    invoke<Shell.DupeReport>("dupe_scan", { path, minSize: minSize ?? null }),
  spaceScan: (path: string) => invoke<Shell.SpaceReport>("space_scan", { path }),
  batchRenamePreview: (items: Shell.RenameItem[], rules: Shell.RenameRule[]) =>
    invoke<Shell.RenamePreview>("batch_rename_preview", { items, rules }),
  batchRenameApply: (items: Shell.RenameItem[], rules: Shell.RenameRule[]) =>
    invoke<Shell.RenameApplyResult>("batch_rename_apply", { items, rules }),
  batchRenameUndo: (undoId: string) => invoke<number>("batch_rename_undo", { undoId }),
  sendtoList: () => invoke<Shell.SendToItem[]>("sendto_list"),
  sendtoCustomAdd: (path: string) => invoke<string[]>("sendto_custom_add", { path }),
  sendtoCustomRemove: (path: string) => invoke<string[]>("sendto_custom_remove", { path }),
  sendtoCopy: (src: string, targetDir: string) => invoke<string>("sendto_copy", { src, targetDir }),
  netDrives: () => invoke<Shell.NetDrive[]>("net_drives"),
  whoLocks: (path: string) => invoke<Shell.LockHolder[]>("who_locks", { path }),
  archiveLs: (path: string) => invoke<Shell.ArchiveListing>("archive_ls", { path }),
  archiveExtractOne: (archive: string, innerPath: string) =>
    invoke<string>("archive_extract_one", { archive, innerPath }),
  sentinelList: () => invoke<Shell.SentinelCfg[]>("sentinel_list"),
  sentinelAdd: (path: string, quietStart?: number | null, quietEnd?: number | null) =>
    invoke<Shell.SentinelCfg[]>("sentinel_add", {
      path,
      quietStart: quietStart ?? null,
      quietEnd: quietEnd ?? null,
    }),
  sentinelRemove: (id: string) => invoke<Shell.SentinelCfg[]>("sentinel_remove", { id }),
  sentinelToggle: (id: string, enabled: boolean) =>
    invoke<Shell.SentinelCfg[]>("sentinel_toggle", { id, enabled }),

  // ---- M7 shell: third-party launcher (independent OS processes, zero network) ----
  tpAdd: (path: string, name?: string, grade?: string) =>
    invoke<Shell.ThirdApp>("tp_add", { path, name: name ?? null, grade: grade ?? null }),
  tpList: () => invoke<Shell.ThirdApp[]>("tp_list"),
  tpRemove: (id: string) => invoke<void>("tp_remove", { id }),
  tpPurge: (id: string) => invoke<void>("tp_purge", { id }),
  tpSetGrade: (id: string, grade: string) => invoke<Shell.ThirdApp>("tp_set_grade", { id, grade }),
  /** 批次W-2：登记/取消 DPI 例外（不响应 DPI 消息的应用，按主屏渲染）。 */
  tpSetDpiFix: (id: string, dpiFix: boolean) =>
    invoke<Shell.ThirdApp>("tp_set_dpi_fix", { id, dpiFix }),
  /** 批次C-6：用户强制兼容层级（null = 恢复自动探测）。 */
  compatSetOverride: (id: string, tier: Shell.CompatTier | null) =>
    invoke<void>("compat_set_override", { id, tier }),
  tpRename: (id: string, name: string) => invoke<Shell.ThirdApp>("tp_rename", { id, name }),
  tpLaunch: (id: string) => invoke<void>("tp_launch", { id }),
  // 批次B：自定义图标（.ico/.png ≤512KB → base64 dataURL 存登记表）+ 以管理员运行
  tpSetIcon: (id: string, iconPath: string | null) => invoke<Shell.ThirdApp>("tp_set_icon", { id, iconPath }),
  // 批次E（规格 5.9.2/5.9.3）：开始菜单扫描 + 便携化
  tpScanStartMenu: () => invoke<Shell.TpScanCandidate[]>("tp_scan_start_menu"),
  tpPortableize: (id: string) => invoke<Shell.ThirdApp>("tp_portableize", { id }),
  tpLaunchAdmin: (id: string) => invoke<void>("tp_launch_admin", { id }),
  iconDataurl: (path: string) => invoke<string>("icon_dataurl", { path }),

  // ---- 批次B-5/B-6（M1 执行档）：模板套用 / 手工编辑 / 干跑 / 残留扫描 ----
  profileTemplates: () => invoke<Shell.ProfileTemplateDto[]>("profile_templates"),
  profileApply: (id: string, templateId: string) =>
    invoke<Shell.ThirdApp>("profile_apply", { id, templateId }),
  profileSet: (id: string, envRedirect: Record<string, string>, envSet: Record<string, string>, sensitive: boolean) =>
    invoke<Shell.ThirdApp>("profile_set", { id, envRedirect, envSet, sensitive }),
  profileDryrun: (id: string) => invoke<Shell.ProfileDryRun>("profile_dryrun", { id }),
  residueScan: () => invoke<Shell.ResidueEntry[]>("residue_scan"),
  // E-1：安装模式执行档 + 默认值推断（AI-1 批次，勿删）
  profileInfer: (id: string) => invoke<Shell.ProfileInfer>("profile_infer", { id }),
  installModeLaunch: (exe: string, name: string | null) =>
    invoke<Shell.InstallSession>("install_mode_launch", { exe, name }),
  installList: () => invoke<Shell.InstallSession[]>("install_list"),
  installAnalyze: (id: string) => invoke<Shell.InstallReport>("install_analyze", { id }),
  installCommit: (id: string, appName: string, entry: string | null) =>
    invoke<Shell.ThirdApp>("install_commit", { id, appName, entry }),
  installDiscard: (id: string) => invoke<void>("install_discard", { id }),
  // E-2：残留清理与白名单
  residueResolve: (path: string) => invoke<void>("residue_resolve", { path }),
  residueWhitelistAdd: (pattern: string) => invoke<string[]>("residue_whitelist_add", { pattern }),
  residueWhitelistList: () => invoke<{ builtin: string[]; user: string[] }>("residue_whitelist_list"),
  // E-3：退出总时序——checkpoint → 断代理 → 残留扫描（AI-1 批次，勿删）
  exitPrepare: () =>
    invoke<{ steps: { step: string; ok: boolean; detail: string }[]; residues: Shell.ResidueEntry[] }>("exit_prepare"),

  // ---- 批次B-7…B-11（M3 终端与云 AI 矩阵） ----
  termStatus: () => invoke<Shell.TerminalStatus>("term_status"),
  termOpen: () => invoke<string>("term_open"),
  aiToolStatus: () => invoke<Shell.AiToolStatus[]>("ai_tool_status"),
  aiInstallNode: () => invoke<void>("ai_install_node"),
  aiInstallTool: (toolId: string) => invoke<void>("ai_install_tool", { toolId }),
  identityList: () => invoke<Shell.AiIdentityView[]>("identity_list"),
  identityAdd: (tool: string, label: string, token: string, note: string) =>
    invoke<Shell.AiIdentityView>("identity_add", { tool, label, token, note }),
  identityRemove: (id: string) => invoke<void>("identity_remove", { id }),
  aiLaunch: (toolId: string, identityId?: string) =>
    invoke<string>("ai_launch", { toolId, identityId: identityId ?? null }),
  aiVerify: () => invoke<Shell.AiVerifyRow[]>("ai_verify"),

  // ---- B-18/B-19 浏览器矩阵 ----
  browserDetect: () => invoke<Shell.DetectedBrowser[]>("browser_detect"),
  browserProfiles: () => invoke<Shell.BrowserProfileDto[]>("browser_profiles"),
  browserProfileAdd: (browserId: string, exe: string, name: string) =>
    invoke<Shell.BrowserProfileDto>("browser_profile_add", { browserId, exe, name }),
  browserProfileRename: (id: string, name: string) =>
    invoke<void>("browser_profile_rename", { id, name }),
  browserProfileClone: (id: string, newName: string) =>
    invoke<Shell.BrowserProfileDto>("browser_profile_clone", { id, newName }),
  browserProfileDelete: (id: string, shred: boolean) =>
    invoke<void>("browser_profile_delete", { id, shred }),
  browserProfileLaunch: (id: string, url?: string) =>
    invoke<number>("browser_profile_launch", { id, url: url ?? null }),
  browserRunning: () => invoke<string[]>("browser_running"),
  browserImport: (id: string, bookmarkHtml?: string, passwordCsv?: string) =>
    invoke<Shell.BrowserImportReport>("browser_import", {
      id,
      bookmarkHtml: bookmarkHtml ?? null,
      passwordCsv: passwordCsv ?? null,
    }),

  // ---- B-20 VS Code Portable ----
  codeStatus: () => invoke<Shell.CodeStatus>("code_status"),
  codeDeploy: () => invoke<void>("code_deploy"),
  codeRegister: () => invoke<void>("code_register"),
  codeLaunch: () => invoke<{ attached: boolean; reason: string }>("code_launch"),

  // ---- B-27 应用生态 2.0 ----
  portabilityAssess: (exe: string) => invoke<Shell.PortabilityCard>("portability_assess", { exe }),
  ecosystemMigrate: (exe: string, name: string) =>
    invoke<Shell.MigrateReport>("ecosystem_migrate", { exe, name }),
  /** 批次B-27：256px Jumbo 图标（资源管理器大图标同源）→ PNG data URL（调用方缓存）。 */
  iconJumboDataurl: (path: string) => invoke<string>("icon_jumbo_dataurl", { path }),
  steamLibraryScan: () => invoke<Shell.SteamGame[]>("steam_library_scan"),
  steamLaunch: (appId: string) => invoke<void>("steam_launch", { appId }),
  aumidLaunch: (aumid: string) => invoke<void>("aumid_launch", { aumid }),
  fileAssocList: () => invoke<Shell.FileAssoc[]>("file_assoc_list"),
  fileAssocSet: (ext: string, appId: string, appName: string) =>
    invoke<void>("file_assoc_set", { ext, appId, appName }),
  fileAssocResolve: (ext: string) =>
    invoke<Shell.FileAssoc | null>("file_assoc_resolve", { ext }),
  fileAssocRemove: (ext: string) => invoke<void>("file_assoc_remove", { ext }),

  // ---- B-29 安全工作台 ----
  peAnalyze: (path: string) => invoke<Shell.PeAnalysis>("pe_analyze", { path }),
  disasmEntry: (path: string, count: number) =>
    invoke<Shell.DisasmLine[]>("disasm_entry", { path, count }),
  sandboxProbe: () => invoke<Shell.SandboxProbe>("sandbox_probe"),
  sandboxWsbGenerate: (sample: string) => invoke<string>("sandbox_wsb_generate", { sample }),
  securityReportExport: (path: string, out: string) =>
    invoke<string>("security_report_export", { path, out }),
  securityEnvPreset: () => invoke<string>("security_env_preset"),

  // ---- S-1 防截屏模式 ----
  shieldSet: (on: boolean) => invoke<{ on: boolean; tagged: number }>("shield_set", { on }),
  shieldGet: () => invoke<boolean>("shield_get"),

  // ---- X-1…X-3 扩展生态 ----
  extList: () => invoke<Shell.ExtView[]>("ext_list"),
  extRescan: () => invoke<Shell.ExtView[]>("ext_rescan"),
  extSetEnabled: (id: string, on: boolean) => invoke<void>("ext_set_enabled", { id, on }),
  extOpenWeb: (id: string) => invoke<string>("ext_open_web", { id }),
  extClose: (id: string) => invoke<void>("ext_close", { id }),
  extAudit: () => invoke<{ lines: string[] }>("ext_audit"),
  extInstallExample: () => invoke<string>("ext_install_example"),
  // X-6 分发与商店
  extMarketList: () => invoke<Shell.MarketPackView[]>("ext_market_list"),
  extMarketImport: (path: string) => invoke<string>("ext_market_import", { path }),
  extMarketInstall: (file: string) => invoke<string>("ext_market_install", { file }),
  extMarketRemove: (file: string) => invoke<void>("ext_market_remove", { file }),
  // X-4/X-5 插件与守护进程
  extPluginLoad: (id: string, libPath: string) => invoke<string>("ext_plugin_load", { id, libPath }),
  extPluginUnload: (id: string) => invoke<void>("ext_plugin_unload", { id }),
  extDaemonStart: (id: string, cmd: string, args: string[]) =>
    invoke<number>("ext_daemon_start", { id, cmd, args }),
  extDaemonStatus: () => invoke<{ id: string; port: number; stopped: boolean; restarts: number }[]>("ext_daemon_status"),
  extDaemonExample: () => invoke<string>("ext_daemon_example"),

  // ---- B-24 子环境档 ----
  envList: () => invoke<Shell.EnvView[]>("env_list"),
  envCreate: (name: string) => invoke<Shell.EnvView>("env_create", { name }),
  envSwitch: (id: string, currentSettings?: Record<string, unknown>) =>
    invoke<unknown>("env_switch", { id, currentSettings: currentSettings ?? null }),
  envDelete: (id: string) => invoke<void>("env_delete", { id }),
  envClone: (id: string, newName: string) =>
    invoke<Shell.EnvCloneReport>("env_clone", { id, newName }),
  envNested: (id: string) => invoke<number>("env_nested", { id }),
  // B-26：试验档 diff / 丢弃 / 合并（三方冲突裁决）
  envDiff: (id: string) =>
    invoke<{ cloneId: string; parentId: string; clean: boolean; entries: { path: string; status: string }[] }>("env_diff", { id }),
  envDiscard: (id: string) => invoke<void>("env_discard", { id }),
  envMerge: (id: string, keepClone: string[]) =>
    invoke<{ merged: number; deleted: number; conflictsResolved: number; conflictsKeptMain: number }>("env_merge", { id, keepClone }),

  // ---- B-28 网络层 ----
  netStatus: () => invoke<Shell.NetStatusView>("net_status"),
  netProxyStart: () => invoke<number>("net_proxy_start"),
  netProxyStop: () => invoke<void>("net_proxy_stop"),
  netKillSwitch: (on: boolean) => invoke<Shell.KillSwitchResult>("net_kill_switch", { on }),
  netRulesList: () => invoke<Shell.NetRule[]>("net_rules_list"),
  netRuleGrant: (domain: string, profile: string) =>
    invoke<Shell.NetRule[]>("net_rule_grant", { domain, profile }),
  netRuleRevoke: (domain: string) => invoke<Shell.NetRule[]>("net_rule_revoke", { domain }),

  // ---- B-33：吊销清单 + 诊断标记 ----
  revocationListExport: (out: string) =>
    invoke<Shell.RevocationReportView>("revocation_list_export", { out }),
  diagFlags: () => invoke<Shell.DiagFlags>("diag_flags"),

  // ---- B-34 诊断包 ----
  diagnosticExport: (out: string) => invoke<Shell.DiagReport>("diagnostic_export", { out }),
  demoCapsule: () => invoke<string>("demo_capsule"),

  // ---- B-22 Git 面板（只读）+ SSH 金库 ----
  gitStatus: (repo: string) => invoke<Shell.GitStatusView>("git_status", { repo }),
  gitLog: (repo: string, limit?: number) =>
    invoke<Shell.GitCommitView[]>("git_log", { repo, limit: limit ?? null }),
  gitBranches: (repo: string) => invoke<string[]>("git_branches", { repo }),
  sshKeys: () => invoke<Shell.SshKeyView[]>("ssh_keys"),
  sshKeyGenerate: (label: string) => invoke<Shell.SshKeyView>("ssh_key_generate", { label }),
  sshKeyDelete: (id: string) => invoke<void>("ssh_key_delete", { id }),

  // ---- B-23 搜索/大文件/行级跳转 ----
  workspaceSearch: (root: string, query: string) =>
    invoke<Shell.SearchReport>("workspace_search", { root, query }),
  bigfileSlice: (path: string, offset: number, len: number) =>
    invoke<Shell.FileSlice>("bigfile_slice", { path, offset, len }),
  editorGoto: (path: string, line: number) =>
    invoke<string>("editor_goto", { path, line }),

  // ---- B-21 工具链 ----
  toolchainStatus: () => invoke<Shell.ToolchainStatusView[]>("toolchain_status"),
  toolchainDeploy: (id: string) => invoke<void>("toolchain_deploy", { id }),

  // ---- B-33 应急能力包（M2 半包）+ B-17 仪表 + B-32 OOBE 建卷 ----
  containerDiag: (path: string) => invoke<Shell.ContainerDiag>("container_diag", { path }),
  containerRepair: (path: string, passphrase?: string) =>
    invoke<Shell.ContainerRepairReport>("container_repair", { path, passphrase: passphrase ?? null }),
  containerRescueExport: (path: string, outDir: string, passphrase?: string) =>
    invoke<Shell.ContainerRescueReport>("container_rescue_export", {
      path,
      outDir,
      passphrase: passphrase ?? null,
    }),
  containerInit: (path: string, passphrase?: string) =>
    invoke<Shell.ContainerRepairReport>("container_init", { path, passphrase: passphrase ?? null }),
  containerStats: (path: string, passphrase?: string) =>
    invoke<Shell.ContainerStatsView>("container_stats", { path, passphrase: passphrase ?? null }),
  vhdxProbe: () => invoke<Shell.VhdxProbeView>("vhdx_probe"),

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
  /** V-3：磁盘磨损/健康读数（按需拉取，无计数器字段为 null）。 */
  sysDiskHealth: () => invoke<Shell.DiskHealth[]>("sys_disk_health"),
  /** D-2：直跑档用户级 Shell 覆盖（实验性，默认关闭）。 */
  directShellStatus: () => invoke<Shell.DirectShellStatus>("directshell_status"),
  directShellSet: (enable: boolean) => invoke<void>("directshell_set", { enable }),
  sysDisks: () => invoke<Shell.SysDisk[]>("sys_disks"),

  // ---- 批次E: 任务栏/开始菜单增强（本机用户名 + 电源操作，零网络） ----
  sysUser: () => invoke<string>("sys_user"),
  // ---- F-1 系统设置中心：环境系统数据源（运行档/显示器/时区/电源计划） ----
  sysenvOverview: () => invoke<Shell.SysEnvOverview>("sysenv_overview"),
  sysenvDisplaySet: (device: string, width: number, height: number, hz: number) =>
    invoke<string>("sysenv_display_set", { device, width, height, hz }),
  // ---- F-2 实用工具集：工具数据落盘（tools/<name>.json）+ DPAPI 加密 + 截屏 ----
  toolDataRead: (name: string) => invoke<string | null>("tool_data_read", { name }),
  toolDataWrite: (name: string, content: string) => invoke<void>("tool_data_write", { name, content }),
  toolSecureRead: (name: string) => invoke<string | null>("tool_secure_read", { name }),
  toolSecureWrite: (name: string, content: string) => invoke<void>("tool_secure_write", { name, content }),
  /** 抓取虚拟屏，返回 BMP 字节（base64 data URL 供 canvas 加载；区域裁剪在前端）。 */
  snapshotCapture: () => invoke<number[]>("snapshot_capture"),
  // ---- AI-08 基础工具组（Z-22…Z-28 支撑 + V-97/98 打印双件） ----
  /** Z-27：Variable 自身信息（版本/运行档/运行时长/数据目录占用）。 */
  sysSelfInfo: () => invoke<Shell.SysSelfInfo>("sys_self_info"),
  /** Z-25：当前光标物理屏幕坐标（虚拟屏坐标系）。 */
  cursorPos: () => invoke<Shell.CursorPos>("cursor_pos"),
  /** Z-23/Z-26：受限 HTTP GET（curl 隐藏窗口）；出站必须先经 requestNetConsent 获得用户同意。 */
  httpFetch: (url: string) => invoke<string>("http_fetch", { url }),
  /** V-97：文件是否具有系统「print」动词关联（无关联 → false，前端据此置灰）。 */
  printAssocCheck: (paths: string[]) => invoke<boolean[]>("print_assoc_check", { paths }),
  /** V-97：走系统打印关联（ShellExecute print 动词）；返回逐文件失败清单（空 = 全部成功）。 */
  printFiles: (paths: string[]) => invoke<string[]>("print_files", { paths }),
  /** V-98：枚举本机打印机（本地 + 连接）。 */
  printList: () => invoke<Shell.PrinterInfo[]>("print_list"),
  /** V-98：读取打印机当前队列（只读）。 */
  printJobs: (printer: string | null) => invoke<Shell.PrintJob[]>("print_jobs", { printer }),
  /** V-98：队列操作（pause / resume / cancel，显式操作无静默批量）。 */
  printJobSet: (printer: string, jobId: number, action: "pause" | "resume" | "cancel") =>
    invoke<void>("print_job_set", { printer, jobId, action }),
  // ---- F-3 任务管理器增强：进程/启动项/服务（护栏见 taskman.rs） ----
  procList: () => invoke<Shell.ProcInfo[]>("proc_list"),
  procKill: (pid: number, force: boolean) => invoke<string>("proc_kill", { pid, force }),
  perfCpu: () => invoke<number[]>("perf_cpu"),
  startupList: () => invoke<Shell.StartupItem[]>("startup_list"),
  startupDisable: (item: Shell.StartupItem) => invoke<void>("startup_disable", { item }),
  serviceList: () => invoke<Shell.ServiceItem[]>("service_list"),
  serviceSet: (name: string, start: boolean) => invoke<string>("service_set", { name, start }),
  // ---- F-4 全局文件搜索：容器内文件名索引（内存快照 + 1.2s 轮询增量重建） ----
  fsIndexStatus: () => invoke<Shell.FsIndexStatus>("fsindex_status"),
  fsIndexQuery: (opts: {
    query: string;
    ext?: string;
    kind?: "all" | "file" | "dir";
    minSize?: number;
    newerDays?: number;
    limit?: number;
  }) =>
    invoke<Shell.FsHit[]>("fsindex_query", {
      query: opts.query,
      ext: opts.ext ?? null,
      kind: opts.kind ?? "all",
      minSize: opts.minSize ?? 0,
      newerDays: opts.newerDays ?? null,
      limit: opts.limit ?? 500,
    }),
  // ---- F-5 音量合成器 + IME 指示 + 媒体控制（audioime.rs） ----
  mixerList: () => invoke<Shell.MixerSession[]>("mixer_list"),
  mixerSet: (pid: number, volume: number, muted: boolean) => invoke<void>("mixer_set", { pid, volume, muted }),
  imeStatus: () => invoke<Shell.ImeStatus>("ime_status"),
  imeList: () => invoke<Shell.ImeLayout[]>("ime_list"),
  imeSwitch: (langId: string) => invoke<void>("ime_switch", { langId }),
  mediaStatus: () => invoke<Shell.MediaStatus | null>("media_status"),
  // ---- F-6 计划备份 + 自更新 + 自检扩展（sysmaint.rs） ----
  backupScheduleGet: () => invoke<Shell.BackupSchedule>("backup_schedule_get"),
  backupScheduleSet: (freq: "none" | "daily" | "weekly", hour: number) =>
    invoke<Shell.BackupSchedule>("backup_schedule_set", { freq, hour }),
  backupRunNow: () => invoke<string>("backup_run_now"),
  updateScan: () => invoke<Shell.UpdateCandidate | null>("update_scan"),
  updateApply: () => invoke<string>("update_apply"),
  maintainSelfcheck: () => invoke<Shell.MaintainFinding[]>("maintain_selfcheck"),
  netIp: () => invoke<string | null>("net_ip"),
  powerAction: (action: "lock" | "logoff" | "reboot" | "shutdown") =>
    invoke<void>("power_action", { action }),
  // 批次E（规格 4.7）：整表应用快捷键（unregister_all → 重注册），返回注册失败的 accel
  shortcutsApply: (binds: { action: string; accel: string }[]) =>
    invoke<{ failed: string[]; remapped: { from: string; to: string }[] }>("shortcuts_apply", { binds }),

  // ---- 批次E-6: 壁纸细节（IDesktopWallpaper 多显示器 + 本地缓存每日换，零网络） ----
  wpMonitors: () => invoke<Shell.WpMonitor[]>("wp_monitors"),
  wpSetMonitor: (monitor: string, path: string) => invoke<void>("wp_set_monitor", { monitor, path }),
  wpPickDaily: (dir: string, mode: "date" | "next") =>
    invoke<string | null>("wp_pick_daily", { dir, mode }),
  /** 批次E-12：扫描 Wallpaper Engine 壁纸项目（root 空 = 自动探测 Steam 库）。 */
  wpEngineScan: (root = "") => invoke<Shell.WpEngineItem[]>("wp_engine_scan", { root }),
  /** 实机反馈：scene 着色器壁纸本地渲染 —— 读主片元着色器并递归展开 #include。 */
  wpSceneShader: (entry: string) => invoke<string>("wp_scene_shader", { entry }),

  // ---- 兼容层：Wallpaper Engine 冲突检测与缓解（libcef 0x80000003） ----
  compatCheck: () =>
    invoke<Shell.CompatStatus>("compat_check"),
  compatApply: () => invoke<Shell.CompatStatus>("compat_apply"),
  compatRestore: () => invoke<Shell.CompatStatus>("compat_restore"),

  // ---- AI-3：Windows Shell 代理（真实 ShellExecute / Shell item / IContextMenu） ----
  shellExecute: (
    path: string,
    options?: { verb?: string; arguments?: string | null; cwd?: string | null; show?: number | null },
  ) => invoke<Shell.ShellExecuteResult>("shell_execute", {
    path,
    verb: options?.verb ?? "open",
    arguments: options?.arguments ?? null,
    cwd: options?.cwd ?? null,
    show: options?.show ?? null,
  }),
  shellActivateApplication: (aumid: string) =>
    invoke<Shell.ShellExecuteResult>("shell_activate_application", { aumid }),
  shellItemIcon: (path: string) => invoke<Shell.ShellIconResult>("shell_item_icon", { path }),
  shellContextMenu: (paths: string[], x: number, y: number) =>
    invoke<Shell.ShellContextMenuResult>("shell_context_menu", { paths, x, y }),
  shellForwardGesture: (gesture: Shell.WindowsShellGesture) =>
    invoke<void>("shell_forward_gesture", { gesture }),

  // ---- 批次E-16：第三方应用嵌入环境（SetParent 子窗口 + 边界跟随） ----
  // 批次W-1：多嵌入并发 —— embedId = VWM 虚拟窗口实例 id；缺省映射 "0" 兼容旧单嵌。
  /** 启动并把主窗口嵌入桌面窗口。attached=false = 已回退为独立窗口运行（rootPid 供框选收编）。
   *  批次C-4：capture=true = L3 画面捕获会话（前端经 embed-frame 事件合成 + embedInput 转发）。 */
  embedLaunch: (id: string, embedId: string, arg?: string) =>
    invoke<{ attached: boolean; reason: string; rootPid?: number; capture?: boolean }>("embed_launch", { id, embedId, arg }),
  /** 批次C-1：收编同进程树新弹出的主窗口（WinEventHook 探测 → 前端开占位窗后调用）。 */
  embedAdopt: (tpId: string, hwnd: number, rootPid: number, embedId: string) =>
    invoke<boolean>("embed_adopt", { tpId, hwnd, rootPid, embedId }),
  /** 批次C-2：手动框选窗口（等待左键按下取光标下根窗口；null = 超时未选中）。 */
  embedPick: (timeoutMs?: number) =>
    invoke<number | null>("embed_pick_window", { timeoutMs: timeoutMs ?? null }),
  // ---- D-3 全域软件接管看门狗 ----
  watchGetSettings: () =>
    invoke<{ enabled: boolean; policy: string; ignored: string[] }>("watch_get_settings"),
  watchSetSettings: (enabled: boolean, policy: string) =>
    invoke<void>("watch_set_settings", { enabled, policy }),
  /** 询问卡处置回执：once（本次保持在桌面）| always（总是忽略该软件）。 */
  watchDismiss: (image: string, action: "once" | "always") =>
    invoke<void>("watch_dismiss", { image, action }),
  embedBounds: (embedId: string, x: number, y: number, w: number, h: number) =>
    invoke<void>("embed_bounds", { embedId, x, y, w, h }),
  embedVisible: (embedId: string, visible: boolean) =>
    invoke<void>("embed_visible", { embedId, visible }),
  embedClose: (embedId: string) => invoke<void>("embed_close", { embedId }),
  /** W-1 退出会话：全部嵌入窗口发 WM_CLOSE（30s 超时者留在桌面，绝不强杀）。 */
  embedCloseAll: () => invoke<number>("embed_close_all"),
  embedFocus: (embedId: string) => invoke<void>("embed_focus", { embedId }),
  /** 批次C-4：L3 输入转发 —— 归一化坐标(0..1) PostMessage 直注屏外真实窗口。 */
  embedInput: (
    embedId: string,
    kind: "move" | "down" | "up" | "dbl" | "wheel" | "key" | "char",
    x: number,
    y: number,
    button?: string,
    key?: number,
    delta?: number,
  ) => invoke<void>("embed_input", { embedId, kind, x, y, button, key, delta }),

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
export namespace Shell {
  export interface DeviceUsage {
    kind: "microphone" | "webcam";
    app: string;
  }
  export interface AudioState {
    volume: number;
    muted: boolean;
  }
  /** AI-06 V-61：系统鼠标四参数（camelCase 对齐 Rust DTO）。 */
  export interface MouseParamsState {
    speed: number;
    doubleClickMs: number;
    wheelLines: number;
    swapButtons: boolean;
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
  // ---- AI-09 文件操作组类型（M-21/Z-29..Z-35/M-19..M-27）----
  /** M-21 校验和结果。 */
  export interface ChecksumResult {
    opId: string;
    algo: string;
    hex: string;
    bytes: number;
    cancelled: boolean;
  }
  export interface ChecksumProgress {
    opId: string;
    done: number;
    total: number;
  }
  /** Z-33 重复文件报告（只报告不删除）。 */
  export interface DupeFile {
    path: string;
    size: number;
    modified: number;
  }
  export interface DupeGroup {
    hash: string;
    size: number;
    files: DupeFile[];
    wasted: number;
  }
  export interface DupeReport {
    groups: DupeGroup[];
    scanned: number;
    truncated: boolean;
  }
  export interface DupeProgress {
    phase: string;
    done: number;
    total: number;
  }
  /** Z-34 空间分析。 */
  export interface SpaceNode {
    name: string;
    path: string;
    size: number;
    fileCount: number;
    dirCount: number;
    children: SpaceNode[];
  }
  export interface SpaceReport {
    root: SpaceNode;
    scanned: number;
    truncated: boolean;
  }
  /** Z-32 批量重命名规则管线。 */
  export type RenameRule =
    | { type: "replace"; find: string; replace: string }
    | { type: "number"; start: number; step: number; pad: number }
    | { type: "case"; mode: string }
    | { type: "ext"; from: string; to: string };
  export interface RenameItem {
    path: string;
    name: string;
  }
  export interface RenamePreviewRow {
    path: string;
    oldName: string;
    newName: string;
    conflict: boolean;
    reason: string;
  }
  export interface RenamePreview {
    rows: RenamePreviewRow[];
  }
  export interface RenameApplyResult {
    renamed: number;
    undoId: string;
  }
  /** Z-35 发送到。 */
  export interface SendToItem {
    kind: string;
    name: string;
    target: string;
  }
  /** Z-31 网络驱动器。 */
  export interface NetDrive {
    letter: string;
    path: string;
    kind: string;
    available: boolean;
    unc: string | null;
  }
  /** M-25 文件锁定侦探（无强拆按钮；空列表=系统未披露占用者）。 */
  export interface LockHolder {
    pid: number;
    name: string;
    title: string;
  }
  /** M-23 压缩包只读浏览。 */
  export interface ArchiveEntry {
    name: string;
    innerPath: string;
    size: number;
    compressedSize: number;
    isDir: boolean;
  }
  export interface ArchiveListing {
    path: string;
    entries: ArchiveEntry[];
  }
  /** M-27 目录监控哨兵。 */
  export interface SentinelCfg {
    id: string;
    path: string;
    enabled: boolean;
    quietStart: number;
    quietEnd: number;
  }
  export interface SentinelChange {
    kind: string;
    name: string;
  }
  export interface SentinelEvent {
    id: string;
    path: string;
    changes: SentinelChange[];
  }
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
  /** 兼容层：Wallpaper Engine 共存状态。 */
  export interface CompatStatus {
    wallpaperEngineRunning: boolean;
    processes: string[];
    compatActive: boolean;
    recommendation: string;
    severity: "none" | "high" | "mitigated" | string;
  }
  /** AI-3 ShellExecuteExW result. A missing PID is valid for URI/UWP launches. */
  export interface ShellExecuteResult {
    launched: boolean;
    processId: number | null;
    backend: "shellExecuteEx" | "applicationActivationManager" | "fallback" | string;
    errorCode: number | null;
  }
  /** Explorer-compatible 64px shell icon returned as a PNG data URL. */
  export interface ShellIconResult {
    dataUrl: string;
    size: number;
    source: "shellItemImageFactory" | "embeddedIcon" | "fallback" | string;
  }
  /** Native IContextMenu is modal by design; false means the caller should use its safe fallback menu. */
  export interface ShellContextMenuResult {
    shown: boolean;
    invoked: boolean;
    commandId: number | null;
  }
  export type WindowsShellGesture = "showDesktop" | "altTab" | "snapLeft" | "snapRight" | "snapUp" | "snapDown";
  /** 批次E-12：Wallpaper Engine 壁纸项目（scene/web 类型如实 supported=false）。 */
  export interface WpEngineItem {
    id: string;
    title: string;
    kind: string;
    file: string | null;
    preview: string | null;
    supported: boolean;
    source: string;
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
  /** F-1：显示器显示模式。 */
  export interface SysDisplayMode {
    width: number;
    height: number;
    bits: number;
    hz: number;
  }
  /** F-1：显示器（当前模式 + 可用刷新率/分辨率集合）。 */
  export interface SysDisplay {
    device: string;
    name: string;
    primary: boolean;
    current: SysDisplayMode | null;
    refresh_rates: number[];
    resolutions: [number, number][];
  }
  /** F-1：环境系统总览（vm = VM 档真实可写；否则只读如实降级）。 */
  export interface SysEnvOverview {
    vm: boolean;
    vm_reason: string;
    displays: SysDisplay[];
    timezone: string;
    utc_offset_minutes: number;
    power_scheme: string;
    power_scheme_name: string;
    username: string;
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
    /** 批次B-3（M1）：隔离执行档（apps.json v2；v1 文件读出为空档）。 */
    profile: PortableProfile;
    /** 批次W-2：DPI 例外（不响应 WM_DPICHANGED 的应用按主屏渲染）。 */
    dpiFix: boolean;
    /** 批次C-6：兼容分级（自动探测 + 用户覆盖；旧 apps.json 读出为缺省档）。 */
    compat: {
      tier: CompatTier | null;
      overrideTier: CompatTier | null;
      probedAt: number | null;
      evidence: Record<string, unknown>;
      /** 批次C-5：L4 让位归因（"fullscreen" | "anticheat" | null = 非 L4）。 */
      hint: string | null;
      exeMtime: number | null;
    };
  }
  /** 批次C-6：四层兼容层级。 */
  export type CompatTier = "L1" | "L2" | "L3" | "L4" | "Native";
  /** 批次B-3（M1，BLUEPRINT 3.3/7.2）：隔离执行档。 */
  export interface PortableProfile {
    envRedirect: Record<string, string>;
    envSet: Record<string, string>;
    /** 出站白名单建议（M8 网络层启用前仅登记）。 */
    netAllow: string[];
    sensitive: boolean;
  }
  /** 批次B-5：重定向模板（.uxpack AI 提供方包雏形）。 */
  export interface ProfileTemplateDto {
    id: string;
    name: string;
    description: string;
    envRedirect: Record<string, string>;
    envSet: Record<string, string>;
    netAllow: string[];
    sensitive: boolean;
  }
  /** 批次B-6：干跑结果（「验证重定向」）。 */
  export interface ProfileDryRun {
    id: string;
    name: string;
    sensitive: boolean;
    envRedirect: Record<string, string>;
  }
  /** 批次B-6：宿主残留条目。 */
  export interface ResidueEntry {
    path: string;
    size: number;
    modifiedMs: number;
    /** E-2：file = 文件落盘；reg = HKCU\Software 新增键 */
    kind?: "file" | "reg";
  }
  /** E-1：默认值推断建议。 */
  export interface ProfileInfer {
    id: string;
    name: string;
    envRedirect: Record<string, string>;
    note: string;
  }
  /** E-1：安装模式暂存会话。 */
  export interface InstallSession {
    id: string;
    name: string;
    exe: string;
    createdAt: number;
  }
  /** E-1：落点分析报告。 */
  export interface InstallReport {
    id: string;
    areas: { area: string; files: number; bytes: number }[];
    exeCandidates: string[];
    totalFiles: number;
    totalBytes: number;
    empty: boolean;
  }
  /** 批次B-7：终端就绪状态。 */
  export interface TerminalStatus {
    deployed: boolean;
    path: string | null;
    registered: boolean;
  }
  /** 批次B-9：AI 工具三态卡片数据源。 */
  export interface AiToolStatus {
    id: string;
    name: string;
    npmPackage: string;
    nodeInstalled: boolean;
    installed: boolean;
    /** 登录态为容器配置标记推断，非读取凭据本体。 */
    loggedIn: boolean;
    lastActivityMs: number | null;
    domains: string[];
  }
  /** 批次B-8：安装进度事件载荷。 */
  export interface AiProgress {
    tool: string;
    phase: "node-download" | "node-extract" | "npm-install" | "done" | "error";
    done: number;
    total: number;
    message: string;
  }
  /** 批次B-10：身份条目视图（凭据只回显尾 4 位）。 */
  export interface AiIdentityView {
    id: string;
    tool: string;
    label: string;
    note: string;
    createdAt: number;
    tokenTail: string;
  }
  /** B-24：环境档。 */
  export interface EnvView {
    id: string;
    name: string;
    active: boolean;
    createdAt: number;
  }
  /** B-34：诊断包。 */
  export interface DiagReport {
    out: string;
    sections: number;
  }
  /** B-28：网络层状态。 */
  export interface NetStatusView {
    proxyRunning: boolean;
    proxyPort: number;
    killSwitch: boolean;
    ruleCount: number;
    bytesRelayed: number;
    connsAllowed: number;
    connsDenied: number;
  }
  export interface KillSwitchResult {
    on: boolean;
  }
  export interface NetRule {
    domain: string;
    profile: string;
    grantedAt: number;
  }
  /** B-33：吊销清单导出。 */
  export interface RevocationReportView {
    out: string;
    entries: number;
  }
  export interface DiagFlags {
    forceRaster: boolean;
  }
  /** B-27：可移植性评估卡。 */
  export interface PortabilityCard {
    verdict: "green" | "yellow" | "red" | string;
    reasons: string[];
    exeSizeBytes: number;
    dirWritable: boolean;
    uninstallEntry: string | null;
  }
  /** B-27：搬迁报告。 */
  export interface MigrateReport {
    appId: string;
    destExe: string;
    bytesCopied: number;
    portableReg: string | null;
    registered: boolean;
  }
  /** B-27：Steam 游戏。 */
  export interface SteamGame {
    appId: string;
    name: string;
  }
  /** B-27：文件关联。 */
  export interface FileAssoc {
    ext: string;
    appId: string;
    appName: string;
  }
  /** B-29：PE 静态分析。 */
  export interface PeAnalysis {
    isPe: boolean;
    machine: string;
    entryRva: number;
    sections: { name: string; rawSize: number; entropy: number; suspiciousEntropy: boolean }[];
    imports: { dll: string; functions: number }[];
    signed: boolean;
    suspiciousHits: string[];
    blake3: string;
    stringsTop: string[];
    sampleNote: string;
  }
  export interface DisasmLine {
    rva: number;
    bytesHex: string;
    text: string;
  }
  export interface SandboxProbe {
    available: boolean;
    detail: string;
  }
  /** X-1…X-3：扩展包视图。 */
  export interface ExtView {
    id: string;
    name: string;
    version: string;
    kind: string;
    permissions: string[];
    description: string | null;
    enabled: boolean;
    signed: boolean;
    crashed: boolean;
    running: boolean;
    csp: string | null;
  }
  /** X-6：市场 .uxpack 包视图。 */
  export interface MarketPackView {
    file: string;
    id: string;
    name: string;
    version: string;
    kind: string;
    description: string | null;
    permissions: string[];
    installed: boolean;
    signed: boolean;
    sizeBytes: number;
  }
  /** B-26：环境克隆报告。 */
  export interface EnvCloneReport {
    sourceId: string;
    cloneId: string;
    cloneName: string;
    bytesCopied: number;
  }
  /** B-23：搜索/大文件。 */
  export interface SearchReport {
    filesScanned: number;
    filesSkippedBinary: number;
    filesSkippedSize: number;
    truncated: boolean;
    hits: { path: string; lines: { lineNo: number; text: string }[] }[];
    elapsedMs: number;
  }
  export interface FileSlice {
    offset: number;
    size: number;
    total: number;
    textLossy: string;
  }
  /** B-22：Git 只读面板。 */
  export interface GitStatusView {
    headBranch: string;
    headCommit: string | null;
    entries: GitEntry[];
    ahead: number;
    behind: number;
    hasUpstream: boolean;
    isRepo: boolean;
  }
  export interface GitEntry {
    path: string;
    state: string;
  }
  export interface GitCommitView {
    id: string;
    summary: string;
    author: string;
    timeMs: number;
  }
  export interface SshKeyView {
    id: string;
    label: string;
    publicKey: string;
  }
  /** B-21：工具链状态。 */
  export interface ToolchainStatusView {
    id: string;
    deployed: boolean;
    home: string;
  }
  /** B-20：VS Code Portable 状态。 */
  export interface CodeStatus {
    deployed: boolean;
    exe: string;
    registered: boolean;
    portableData: boolean;
  }
  /** B-18：检测到的本机浏览器。 */
  export interface DetectedBrowser {
    id: string;
    name: string;
    exe: string;
    family: "chrome" | "firefox";
  }
  /** B-18：浏览器 profile（数据目录在容器 browsers/ 下）。 */
  export interface BrowserProfileDto {
    id: string;
    browserId: string;
    name: string;
    exe: string;
    family: "chrome" | "firefox";
    dataDir: string;
    createdAt: number;
  }
  /** B-19：导入报告（书签/密码文件复制进容器）。 */
  export interface BrowserImportReport {
    bookmarks: number;
    passwords: number;
    copiedFiles: string[];
  }
  /** B-33：容器诊断（恢复模式入口判定）。 */
  export interface ContainerDiag {
    exists: boolean;
    magicOk: boolean;
    containerVersion: number;
    engineVersion: number;
    needsMigration: boolean;
    downgradeRequired: boolean;
    sizeBytes: number;
    openable: boolean;
    openError: string | null;
  }
  /** B-33：journal 重放固化 / 建卷报告。 */
  export interface ContainerRepairReport {
    repaired: boolean;
    message: string;
    fileCount: number;
    chunkCount: number;
  }
  /** B-33：两级救援导出报告。 */
  export interface ContainerRescueReport {
    mode: "files" | "chunks" | string;
    filesRescued: string[];
    chunksRescued: number;
    bytesRescued: number;
    errors: string[];
  }
  /** B-17：容器仪表快照。 */
  export interface ContainerStatsView {
    volumes: { path: string; usedBytes: number; declaredCapacity: number }[];
    writeAmplification: number;
    fileCount: number;
    chunkCount: number;
    logicalWritten: number;
    physicalWritten: number;
  }
  /** B-32：介质体检（VHDX 快速档能力）。 */
  export interface VhdxProbeView {
    isAdmin: boolean;
    mountVhdAvailable: boolean;
    usable: boolean;
  }
  /** 批次B-11：凭据零落宿主断言行。 */
  export interface AiVerifyRow {
    id: string;
    name: string;
    shimInContainer: boolean;
    configInContainer: boolean;
    hostResidue: string[];
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
  /** 批次D：CPU/内存简报（sys_brief；cpu=百分比，mem 为字节）。L-3：runtimeMode 运行档。 */
  export interface SysBrief {
    cpu: number;
    memUsed: number;
    memTotal: number;
    runtimeMode: "vm" | "light" | "direct";
  }
  /** AI-08 Z-27：Variable 自身信息（sys_self_info）。dataDirCapped = 大小统计 >2GB 截断。 */
  export interface SysSelfInfo {
    version: string;
    runtimeMode: string;
    uptimeSecs: number;
    dataDir: string;
    dataDirBytes: number;
    dataDirCapped: boolean;
    osVersion: string;
  }
  /** AI-08 Z-25：光标物理屏幕坐标（虚拟屏坐标系）。 */
  export interface CursorPos {
    x: number;
    y: number;
  }
  /** AI-08 V-98：本机打印机（print_list）。 */
  export interface PrinterInfo {
    name: string;
    port: string;
    driver: string;
    isDefault: boolean;
    jobs: number;
    status: string;
  }
  /** AI-08 V-98：打印队列任务（print_jobs）。 */
  export interface PrintJob {
    jobId: number;
    printer: string;
    document: string;
    user: string;
    status: string;
    statusRaw: number;
    totalPages: number;
    pagesPrinted: number;
    submitted: string;
  }
  /** 批次D：本地盘符容量（sys_disks）。 */
  export interface SysDisk {
    letter: string;
    path: string;
    total: number;
    free: number;
  }
  /** V-3：磁盘磨损/健康读数（按需拉取，无计数器字段为 null）。 */
  export interface DiskHealth {
    name: string;
    media: string;
    bus: string;
    health: string;
    wearPct: number | null;
    tempC: number | null;
    powerOnHours: number | null;
  }
  /** D-2：直跑档 Shell 覆盖状态 + 一键还原脚本路径。 */
  export interface DirectShellStatus {
    enabled: boolean;
    restoreScript: string | null;
  }
  /** F-3：进程快照（family=variable 引擎家族 / host 宿主；critical=系统关键禁结束）。 */
  export interface ProcInfo {
    pid: number;
    ppid: number | null;
    name: string;
    mem: number;
    cpu: number;
    family: "variable" | "host";
    critical: boolean;
  }
  /** F-3：启动项（注册表 Run 键）。 */
  export interface StartupItem {
    name: string;
    cmd: string;
    hive: "HKCU" | "HKLM";
  }
  /** F-3：服务（VM 档 PowerShell）。 */
  export interface ServiceItem {
    name: string;
    display: string;
    status: string;
    startType: string;
  }
  /** F-4：文件索引命中（容器内文件名/路径索引）。 */
  export interface FsHit {
    name: string;
    path: string;
    isDir: boolean;
    size: number;
    mtime: number;
  }
  /** F-4：索引状态（count=条数；building=后台重建中；containerOnly=仅容器内）。 */
  export interface FsIndexStatus {
    count: number;
    builtAt: number;
    building: boolean;
    containerOnly: boolean;
  }
  /** F-5.2：合成器会话（pid + 进程名；volume 0-1）。 */
  export interface MixerSession {
    pid: number;
    name: string;
    volume: number;
    muted: boolean;
  }
  /** F-5.3：IME 状态（langId 如 "0804"；chinese null = 无法读取）。 */
  export interface ImeStatus {
    langId: string;
    chinese: boolean | null;
  }
  /** F-5.3：键盘布局项。 */
  export interface ImeLayout {
    langId: string;
    name: string;
  }
  /** F-5.4：媒体会话（null = 探测不到，前端简化）。 */
  export interface MediaStatus {
    title: string;
    artist: string;
    positionSec: number;
    durationSec: number;
    status: "playing" | "paused" | "other";
  }
  /** F-6：计划备份配置。 */
  export interface BackupSchedule {
    freq: "none" | "daily" | "weekly" | string;
    hour: number;
    lastRunMs: number;
    lastSource: string;
    missed: boolean;
  }
  /** F-6：本地更新包候选（null = 无更新包）。 */
  export interface UpdateCandidate {
    version: string;
    files: number;
    minVersion: string | null;
  }
  /** F-6：自检发现（ok/warn/info）。 */
  export interface MaintainFinding {
    id: string;
    level: string;
    message: string;
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
export type FileAssoc = Shell.FileAssoc;
export type InstallSession = Shell.InstallSession;
export type InstallReport = Shell.InstallReport;
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
export type ChecksumResult = Shell.ChecksumResult;
export type ChecksumProgress = Shell.ChecksumProgress;
export type DupeReport = Shell.DupeReport;
export type DupeGroup = Shell.DupeGroup;
export type DupeFile = Shell.DupeFile;
export type DupeProgress = Shell.DupeProgress;
export type SpaceReport = Shell.SpaceReport;
export type SpaceNode = Shell.SpaceNode;
export type RenameRule = Shell.RenameRule;
export type RenameItem = Shell.RenameItem;
export type RenamePreview = Shell.RenamePreview;
export type RenamePreviewRow = Shell.RenamePreviewRow;
export type RenameApplyResult = Shell.RenameApplyResult;
export type SendToItem = Shell.SendToItem;
export type NetDrive = Shell.NetDrive;
export type LockHolder = Shell.LockHolder;
export type ArchiveListing = Shell.ArchiveListing;
export type ArchiveEntry = Shell.ArchiveEntry;
export type SentinelCfg = Shell.SentinelCfg;
export type SentinelChange = Shell.SentinelChange;
export type SentinelEvent = Shell.SentinelEvent;
export type TpGrade = Shell.TpGrade;
export type ThirdApp = Shell.ThirdApp;
export type PortableProfile = Shell.PortableProfile;
export type ProfileTemplateDto = Shell.ProfileTemplateDto;
export type ProfileDryRun = Shell.ProfileDryRun;
export type ResidueEntry = Shell.ResidueEntry;
export type TerminalStatus = Shell.TerminalStatus;
export type AiToolStatus = Shell.AiToolStatus;
export type AiProgress = Shell.AiProgress;
export type AiIdentityView = Shell.AiIdentityView;
export type AiVerifyRow = Shell.AiVerifyRow;
export type ContainerDiag = Shell.ContainerDiag;
export type GitStatusView = Shell.GitStatusView;
export type GitCommitView = Shell.GitCommitView;
export type SshKeyView = Shell.SshKeyView;
export type GitEntry = Shell.GitEntry;
export type DetectedBrowser = Shell.DetectedBrowser;
export type BrowserProfileDto = Shell.BrowserProfileDto;
export type BrowserImportReport = Shell.BrowserImportReport;
export type ContainerRepairReport = Shell.ContainerRepairReport;
export type ContainerRescueReport = Shell.ContainerRescueReport;
export type ContainerStatsView = Shell.ContainerStatsView;
export type VhdxProbeView = Shell.VhdxProbeView;
export type UsbStatus = Shell.UsbStatus;
export type FileCheck = Shell.FileCheck;
export type PackProgress = Shell.PackProgress;
export type NetPolicy = Shell.NetPolicy;
export type NetConsentEntry = Shell.NetConsentEntry;
export type OfficialUsage = Shell.OfficialUsage;
export type SysBrief = Shell.SysBrief;
export type SysDisk = Shell.SysDisk;
export type WpMonitor = Shell.WpMonitor;
export type CompatStatus = Shell.CompatStatus;
export type ShellExecuteResult = Shell.ShellExecuteResult;
export type SysDisplayMode = Shell.SysDisplayMode;
export type SysDisplay = Shell.SysDisplay;
export type SysEnvOverview = Shell.SysEnvOverview;
export type ShellIconResult = Shell.ShellIconResult;
export type ShellContextMenuResult = Shell.ShellContextMenuResult;
export type WindowsShellGesture = Shell.WindowsShellGesture;
export type WpEngineItem = Shell.WpEngineItem;
export type VaultStatus = Shell.VaultStatus;
export type VaultItem = Shell.VaultItem;
export type AuditFinding = Shell.AuditFinding;

export type EdgeStylePatch = Partial<Pick<MindEdge, "direction" | "lineStyle" | "pathStyle" | "color" | "width" | "label" | "animated">>;
export type ShapeKind = NodeShape;
export type Dir = EdgeDirection;
export type LStyle = LineStyle;
export type PStyle = PathStyle;
