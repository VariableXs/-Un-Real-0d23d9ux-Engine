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
  // ---- AI-12 兼容纵深组（Z-15…Z-21、M-37…M-45 支撑）----
  compatUwpList: () => invoke<{ name: string; appId: string }[]>("compat_uwp_list"),
  compatElevationProbe: (path: string) =>
    invoke<{ requiresAdmin: boolean; manifestFound: boolean }>("compat_elevation_probe", { path }),
  compatDriverScan: () => invoke<string[]>("compat_driver_scan"),
  compatHostProbe: () =>
    invoke<{ remoteSession: boolean; vmSignals: string[]; hostKind: "remote" | "vm" | "native" }>("compat_host_probe"),
  compatShimReport: (hit: string) => invoke<number>("compat_shim_report", { hit }),
  compatShimStats: () => invoke<Record<string, number>>("compat_shim_stats"),
  compatIconProbe: (path: string) =>
    invoke<{ exists: boolean; mtimeMs: number; size: number }>("compat_icon_probe", { path }),
  compatVolumes: () => invoke<{ guidPath: string; mountPoints: string[] }[]>("compat_volumes"),
  compatHealPaths: (entries: { path: string; volumeGuid: string }[]) =>
    invoke<{ path: string; healed: string | null }[]>("compat_heal_paths", { entries }),

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
  exViewGet: (path: string) => invoke<string | null>("ex_view_get", { path }),
  exViewSet: (path: string, view: string) => invoke<void>("ex_view_set", { path, view }),
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
  /** AI-01 M-04 窗口体检：按 pid 列表返回其中确认无响应（IsHungAppWindow）的子集。 */
  winHealthScan: (pids: number[]) => invoke<number[]>("win_health_scan", { pids }),
  /** AI-01 M-06 窗口挂起/恢复（ntdll NtSuspend/ResumeProcess；非 Windows 或失败 → false）。 */
  procSuspend: (pid: number) => invoke<boolean>("win_suspend", { pid }),
  procResume: (pid: number) => invoke<boolean>("win_resume", { pid }),
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

  // ---- AI-07 效率中枢：N-15 剪贴板历史（后端录制/DPAPI 落盘/敏感名单/关闭即焚） ----
  cliphistList: () => invoke<Shell.ClipEntry[]>("cliphist_list"),
  cliphistPin: (id: string, pinned: boolean) => invoke<void>("cliphist_pin", { id, pinned }),
  cliphistRemove: (id: string) => invoke<void>("cliphist_remove", { id }),
  cliphistClear: (keepPinned: boolean) => invoke<void>("cliphist_clear", { keepPinned }),
  cliphistBurn: () => invoke<void>("cliphist_burn"),
  cliphistConfigGet: () => invoke<Shell.ClipConfig>("cliphist_config_get"),
  cliphistConfigSet: (config: Shell.ClipConfig) => invoke<void>("cliphist_config_set", { config }),
  cliphistWriteBack: (id: string) => invoke<boolean>("cliphist_write_back", { id }),

  // ---- AI-07 效率中枢：N-18 宏引擎（急停护栏 / 触发器登记 / 键鼠模拟） ----
  macroEmergencyStop: () => invoke<void>("macro_emergency_stop"),
  macroEmergencyClear: () => invoke<void>("macro_emergency_clear"),
  macroIsStopped: () => invoke<boolean>("macro_is_stopped"),
  macroUacForeground: () => invoke<boolean>("macro_uac_foreground"),
  macroUpsertTrigger: (def: Shell.MacroTriggerDef) => invoke<void>("macro_upsert_trigger", { def }),
  macroRemoveTrigger: (macroId: string) => invoke<void>("macro_remove_trigger", { macroId }),
  macroListTriggers: () => invoke<Shell.MacroTriggerDef[]>("macro_list_triggers"),
  macroSendText: (text: string, passwordFocus: boolean) =>
    invoke<boolean>("macro_send_text", { text, passwordFocus }),

  // ---- AI-10 数据中枢：U-24 版本时间机 ----
  verWatch: (path: string) => invoke<void>("ver_watch", { path }),
  verWatchedList: () => invoke<string[]>("ver_watched_list"),
  verSnapshot: (path: string) => invoke<Shell.VerInfo | null>("ver_snapshot", { path }),
  verList: (path: string) => invoke<Shell.VerInfo[]>("ver_list", { path }),
  verDiff: (path: string, oldId: string, newId: string) =>
    invoke<Shell.VerDiff>("ver_diff", { path, oldId, newId }),
  verRestore: (path: string, versionId: string) => invoke<void>("ver_restore", { path, versionId }),
  verGc: () => invoke<Shell.VerGcReport>("ver_gc"),
  verPolicySet: (keepVersions: number | null, keepDays: number | null) =>
    invoke<void>("ver_policy_set", { keepVersions, keepDays }),

  // ---- AI-10：U-25 全局标签 / 智能文件夹 ----
  tagAll: () => invoke<string[]>("tag_all"),
  tagFilter: (tag: string) => invoke<string[]>("tag_filter", { tag }),
  tagSmartList: () => invoke<Shell.SmartFolder[]>("tag_smart_list"),
  tagSmartAdd: (name: string, query: string) => invoke<Shell.SmartFolder>("tag_smart_add", { name, query }),
  tagSmartRemove: (id: string) => invoke<void>("tag_smart_remove", { id }),

  // ---- AI-10：U-26 回收站 2.0 自动清理策略 ----
  recPolicyGet: () => invoke<Shell.RecPolicy>("rec_policy_get"),
  recPolicySet: (policy: Shell.RecPolicy) => invoke<void>("rec_policy_set", { policy }),
  recPolicyPreview: () => invoke<Shell.RecItem[]>("rec_policy_preview"),
  recPolicyApply: () => invoke<number>("rec_policy_apply"),

  // ---- AI-10：U-28 传输指挥台 ----
  trEnqueue: (srcs: string[], destDir: string, kind: "copy" | "move", onConflict?: string) =>
    invoke<Shell.TrItem[]>("tr_enqueue", { srcs, destDir, kind, onConflict }),
  trList: () => invoke<Shell.TrItem[]>("tr_list"),
  trPause: (id: string) => invoke<Shell.TrItem[]>("tr_pause", { id }),
  trResume: (id: string) => invoke<Shell.TrItem[]>("tr_resume", { id }),
  trCancel: (id: string) => invoke<Shell.TrItem[]>("tr_cancel", { id }),
  trRetry: (id: string) => invoke<Shell.TrItem[]>("tr_retry", { id }),
  trClearDone: () => invoke<Shell.TrItem[]>("tr_clear_done"),

  // ---- AI-10：U-30 信任链中心 ----
  trustRegister: (path: string, origin: string) => invoke<void>("trust_register", { path, origin }),
  trustVerify: (path: string) => invoke<Shell.TrustVerdict>("trust_verify", { path }),
  trustWall: () => invoke<Shell.TrustWall>("trust_wall"),
  trustReverifyAll: () => invoke<Shell.TrustWall>("trust_reverify_all"),
  trustRemove: (path: string) => invoke<void>("trust_remove", { path }),

  // ---- AI-10：U-31 数据血缘 ----
  linList: (path: string | null) => invoke<Shell.LinEvent[]>("lin_list", { path }),
  linStats: () => invoke<Shell.LinStats>("lin_stats"),
  linExport: (destDir: string) => invoke<string>("lin_export", { destDir }),
  linBurn: () => invoke<boolean>("lin_burn"),

  // ---- AI-10：U-35 本地使用洞察 ----
  insDashboard: (range: string) => invoke<Shell.InsDashboard>("ins_dashboard", { range }),
  insSuggestions: () => invoke<Shell.InsSuggestion[]>("ins_suggestions"),
  insBurn: () => invoke<boolean>("ins_burn"),

  // ---- AI-10：U-27 隐私仪表盘（审计时间线 + 金丝雀） ----
  privTimeline: (days?: number) => invoke<Shell.AuditTimeline>("priv_timeline", { days }),
  canaryList: () => invoke<Shell.CanaryOverview>("canary_list"),
  canaryPlant: (dir: string, template: string) =>
    invoke<Shell.CanaryFile>("canary_plant", { dir, template }),
  canaryRemove: (id: string) => invoke<void>("canary_remove", { id }),

  // ---- AI-10：U-29 存档柜（.vxa zstd 归档） ----
  archCreate: (name: string, srcs: string[]) => invoke<Shell.ArcCard>("arch_create", { name, srcs }),
  archList: () => invoke<Shell.ArcCard[]>("arch_list"),
  archBrowse: (id: string, subpath: string) => invoke<Shell.ArcNode[]>("arch_browse", { id, subpath }),
  archAudit: (id: string) => invoke<Shell.ArcAudit>("arch_audit", { id }),
  archExtract: (id: string, destDir: string) => invoke<number>("arch_extract", { id, destDir }),
  archRepair: (id: string) => invoke<Shell.ArcAudit>("arch_repair", { id }),
  archRemove: (id: string) => invoke<void>("arch_remove", { id }),

  // ---- AI-10：U-33 隐身会话 ----
  incStatus: () => invoke<Shell.IncStatus>("inc_status"),
  incStart: () => invoke<Shell.IncSession>("inc_start"),
  incEnd: () => invoke<number>("inc_end"),
  incList: () => invoke<string[]>("inc_list"),
  incWrite: (name: string, contents: string) => invoke<string>("inc_write", { name, contents }),

  // ---- AI-10：U-32 应用防火墙 2.0 ----
  fwProfiles: () => invoke<Shell.FwProfile[]>("fw_profiles"),
  fwProfileSet: (profile: Shell.FwProfile) => invoke<void>("fw_profile_set", { profile }),
  fwAlerts: () => invoke<Shell.FwAlertCenter>("fw_alerts"),
  fwAlertResolve: (alertId: string, allow: boolean) =>
    invoke<void>("fw_alert_resolve", { alertId, allow }),

  // ---- AI-10：U-34 紧急擦拭 ----
  panicConfigGet: () => invoke<Shell.PanicConfig>("panic_config_get"),
  panicConfigSet: (config: Shell.PanicConfig) => invoke<void>("panic_config_set", { config }),
  panicTrigger: (level: string, dryRun: boolean) =>
    invoke<Shell.PanicRunReport>("panic_trigger", { level, dryRun }),
  panicDrill: () => invoke<Shell.PanicRunReport>("panic_drill"),

  // ---- AI-10：数据安全中心独立窗口 ----
  openDatavault: () => invoke<void>("open_datavault"),

  // ---- AI-13 性能与长跑组（U-19/U-20/U-22、M-46…M-48/M-53/M-54、N-35/N-36）----
  perfMemSnapshot: () => invoke<Shell.MemSnapshot>("perf_mem_snapshot"),
  perfMemWardenStatus: () => invoke<Shell.WardenStatus>("perf_mem_warden_status"),
  perfIoCopy: (from: string, to: string) => invoke<{ id: number }>("perf_io_copy", { from, to }),
  perfIoPause: (id: number) => invoke<void>("perf_io_pause", { id }),
  perfIoResume: (id: number) => invoke<void>("perf_io_resume", { id }),
  perfIoCancel: (id: number) => invoke<void>("perf_io_cancel", { id }),
  perfIoProgress: () => invoke<Shell.IoProgress>("perf_io_progress"),
  perfLogUsage: () => invoke<Shell.LogUsage>("perf_log_usage"),
  perfLogRotate: () => invoke<Shell.RotateReport>("perf_log_rotate"),
  perfSettingsPreflight: (knownKeys: string[]) =>
    invoke<Shell.PreflightReport>("perf_settings_preflight", { knownKeys }),
  perfDbCompact: () => invoke<Shell.CompactReport>("perf_db_compact"),
  perfInstanceList: () => invoke<Shell.InstanceInfo[]>("perf_instance_list"),
  perfInstanceCreate: (name: string, takesDesktop: boolean) =>
    invoke<Shell.InstanceInfo>("perf_instance_create", { name, takesDesktop }),
  perfInstanceDelete: (name: string) => invoke<void>("perf_instance_delete", { name }),
  perfInstanceHeartbeat: (name: string) => invoke<void>("perf_instance_heartbeat", { name }),
  perfRelayExport: (rels: string[], outFile: string) =>
    invoke<Shell.RelayManifestView>("perf_relay_export", { rels, outFile }),
  perfRelayImport: (relayFile: string, targetDir: string) =>
    invoke<Shell.RelayImportReport>("perf_relay_import", { relayFile, targetDir }),
  perfCpuQuotaSet: (pid: number, tier: number) =>
    invoke<Shell.QuotaResult>("perf_cpu_quota_set", { pid, tier }),
  perfCrashDumps: () => invoke<Shell.CrashDumpInfo[]>("perf_crash_dumps"),
  perfBootStage: (name: string, priority: number) => invoke<void>("perf_boot_stage", { name, priority }),
  perfBootStages: () => invoke<{ name: string; ts: number; priority: number }[]>("perf_boot_stages"),

  // ---- AI-15 开放工具组（M-57/59/63、V-81..V-90）----
  // M-57 出站桥
  webhookRulesGet: () => invoke<Shell.WebhookConfigView>("webhook_rules_get"),
  webhookRulesSet: (config: Shell.WebhookConfigView) =>
    invoke<Shell.WebhookConfigView>("webhook_rules_set", { config }),
  webhookDispatch: (event: string, payload: string) => invoke<number>("webhook_dispatch", { event, payload }),
  webhookTest: (url: string) => invoke<boolean>("webhook_test", { url }),
  webhookLogList: (limit?: number) => invoke<Shell.WebhookLogView[]>("webhook_log_list", { limit }),
  // M-59 嵌入声明协议 / M-63 资源包安全扫描
  embedManifestScan: (exePath: string) => invoke<Shell.EmbedManifestView | null>("embed_manifest_scan", { exePath }),
  vxsScan: (path: string) => invoke<Shell.VxsScanReportView>("vxs_scan_cmd", { path }),
  // V-89 配置对比 / V-90 沙盒试用
  cfgDiff: (a: string, b: string) => invoke<Shell.CfgDiffEntryView[]>("cfg_diff", { a, b }),
  sandboxTrialBegin: (kind: string, id: string, prevValue: string) =>
    invoke<void>("sandbox_trial_begin", { kind, id, prevValue }),
  sandboxTrialEnd: (kind: string, id: string) =>
    invoke<Shell.SandboxTrialView[]>("sandbox_trial_end", { kind, id }),
  sandboxTrialList: () => invoke<Shell.SandboxTrialView[]>("sandbox_trial_list"),
  // V-81 开放安装器（winget）
  wingetStatus: () => invoke<Shell.WingetStatusView>("winget_status"),
  wingetSearch: (query: string) => invoke<Shell.WingetPkgView[]>("winget_search", { query }),
  wingetListInstalled: () => invoke<Shell.WingetPkgView[]>("winget_list_installed"),
  wingetUpgradeList: () => invoke<Shell.WingetPkgView[]>("winget_upgrade_list"),
  wingetInstall: (id: string, exact?: boolean) => invoke<Shell.WingetOpResultView>("winget_install", { id, exact }),
  wingetUpgradeOne: (id: string) => invoke<Shell.WingetOpResultView>("winget_upgrade_one", { id }),
  wingetUninstall: (id: string) => invoke<Shell.WingetOpResultView>("winget_uninstall", { id }),
  // V-82 环境变量编辑器
  envOverview: () => invoke<Shell.EnvOverviewView>("env_overview"),
  envBackupList: () => invoke<Shell.EnvBackupView[]>("env_backup_list"),
  envVarSet: (name: string, value: string, expand: boolean) =>
    invoke<void>("env_var_set", { name, value, expand }),
  envVarDelete: (name: string) => invoke<void>("env_var_delete", { name }),
  envRestoreBackup: (backupId: string) => invoke<number>("env_restore_backup", { backupId }),
  // V-83 计划任务工坊 / V-86 启动延迟编排
  schedList: () => invoke<Shell.SchedTaskView[]>("sched_list"),
  schedUpsert: (task: Shell.SchedTaskView) => invoke<Shell.SchedTaskView[]>("sched_upsert", { task }),
  schedRemove: (id: string) => invoke<Shell.SchedTaskView[]>("sched_remove", { id }),
  schedToggle: (id: string, enabled: boolean) => invoke<Shell.SchedTaskView[]>("sched_toggle", { id, enabled }),
  schedLogList: () => invoke<Shell.SchedLogView[]>("sched_log_list"),
  schedRunNow: (id: string) => invoke<void>("sched_run_now", { id }),
  startdelayGet: () => invoke<Shell.StartDelayConfigView>("startdelay_get"),
  startdelaySet: (config: Shell.StartDelayConfigView) =>
    invoke<Shell.StartDelayConfigView>("startdelay_set", { config }),
  startdelayTimeline: () => invoke<Shell.StartDelayTimelineView[]>("startdelay_timeline"),
  // V-84 关联快照 / V-85 卸载善后
  assocSnapshotTake: (name: string) => invoke<Shell.AssocSnapshotView>("assoc_snapshot_take", { name }),
  assocSnapshotList: () => invoke<Shell.AssocSnapshotView[]>("assoc_snapshot_list"),
  assocSnapshotRemove: (id: string) => invoke<void>("assoc_snapshot_remove", { id }),
  assocSnapshotDiff: (idA: string, idB: string) =>
    invoke<Shell.AssocDiffEntryView[]>("assoc_snapshot_diff", { idA, idB }),
  assocSnapshotRestore: (id: string) => invoke<[number, number]>("assoc_snapshot_restore", { id }),
  residueScanApp: (appName: string) => invoke<Shell.ResidueReportView>("residue_scan_app", { appName }),
  residueDelete: (paths: string[]) => invoke<string[]>("residue_delete", { paths }),
  // V-87 服务依赖图
  svcGraph: () => invoke<Shell.SvcNodeView[]>("svc_graph"),
  svcImpact: (target: string) => invoke<Shell.SvcImpactView>("svc_impact", { target }),
  svcTopo: () => invoke<Shell.SvcTopoView>("svc_topo"),
};

/** Shell 命令的返回结构（与 src-tauri/src/shell/hardware.rs 序列化字段一一对应）。 */
export namespace Shell {
  // ---- AI-13 性能与长跑组 ----
  /** U-20 内存快照（tier: 0 normal / 1 watch / 2 critical） */
  export interface MemSnapshot {
    load_pct: number;
    total_bytes: number;
    avail_bytes: number;
    tier: number;
  }
  export interface WardenStatus {
    running: boolean;
    points: number;
    leak_suspect: boolean;
    tier: number;
    history: { ts: number; load_pct: number }[];
  }
  export interface IoJobProgress {
    id: number;
    from: string;
    to: string;
    copied: number;
    total: number;
    state: number;
  }
  export interface IoProgress {
    jobs: IoJobProgress[];
  }
  export interface LogUsage {
    active_bytes: number;
    archived_bytes: number;
    archived_count: number;
  }
  export interface RotateReport {
    rotated: boolean;
    deleted: number;
    active_bytes: number;
  }
  export interface PreflightReport {
    total: number;
    unknown_keys: string[];
    corrupt_keys: string[];
  }
  export interface CompactReport {
    before_bytes: number;
    after_bytes: number;
    snapshot_path: string;
    ms: number;
  }
  export interface InstanceInfo {
    name: string;
    heartbeat: number;
    takes_desktop: boolean;
  }
  export interface RelayManifestView {
    kind: string;
    version: number;
    created_at: number;
    files: { rel: string; size: number; sha256: string }[];
  }
  export interface RelayImportReport {
    imported: string[];
    missing: string[];
  }
  export interface QuotaResult {
    pid: number;
    applied: boolean;
    tier: number;
  }
  export interface CrashDumpInfo {
    file: string;
    size: number;
    ts: number;
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
export type FwProfile = Shell.FwProfile;
export type FwAlert = Shell.FwAlert;
export type FwAlertCenter = Shell.FwAlertCenter;
export type PanicConfig = Shell.PanicConfig;
export type PanicRunReport = Shell.PanicRunReport;
export type ArcCard = Shell.ArcCard;
export type ArcNode = Shell.ArcNode;
export type ArcAudit = Shell.ArcAudit;
export type IncSession = Shell.IncSession;
export type IncStatus = Shell.IncStatus;
export type VerInfo = Shell.VerInfo;
export type DiffLine = Shell.DiffLine;
export type VerDiff = Shell.VerDiff;
export type VerGcReport = Shell.VerGcReport;
export type SmartFolder = Shell.SmartFolder;
export type RecPolicy = Shell.RecPolicy;
export type TrItem = Shell.TrItem;
export type TrustVerdict = Shell.TrustVerdict;
export type TrustEntry = Shell.TrustEntry;
export type TrustWall = Shell.TrustWall;
export type LinEvent = Shell.LinEvent;
export type LinStats = Shell.LinStats;
export type InsAppTime = Shell.InsAppTime;
export type InsFocus = Shell.InsFocus;
export type InsNotif = Shell.InsNotif;
export type InsSearch = Shell.InsSearch;
export type InsDashboard = Shell.InsDashboard;
export type InsSuggestion = Shell.InsSuggestion;
export type AuditEvent = Shell.AuditEvent;
export type AuditGap = Shell.AuditGap;
export type AuditTimeline = Shell.AuditTimeline;
export type CanaryTrigger = Shell.CanaryTrigger;
export type CanaryFile = Shell.CanaryFile;
export type CanaryOverview = Shell.CanaryOverview;
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
