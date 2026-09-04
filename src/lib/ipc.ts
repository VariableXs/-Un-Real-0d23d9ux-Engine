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
import type * as Pv from "../features/projectviz/types";

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
};

export type EdgeStylePatch = Partial<Pick<MindEdge, "direction" | "lineStyle" | "pathStyle" | "color" | "width" | "label" | "animated">>;
export type ShapeKind = NodeShape;
export type Dir = EdgeDirection;
export type LStyle = LineStyle;
export type PStyle = PathStyle;
