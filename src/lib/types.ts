/// Shared TypeScript types mirroring the Rust models (serde camelCase).
export interface Folder {
  id: string;
  parentId: string | null;
  name: string;
  sortOrder: number;
  createdAt: number;
  updatedAt: number;
  deletedAt: number | null;
}

export interface DocumentMeta {
  id: string;
  folderId: string | null;
  title: string;
  favorite: boolean;
  tags: string[];
  createdAt: number;
  updatedAt: number;
  deletedAt: number | null;
}

export interface DocumentFull extends DocumentMeta {
  contentHtml: string;
  contentText: string;
}

export interface DocumentInput {
  id: string;
  title: string;
  contentHtml: string;
  contentText: string;
  favorite?: boolean;
  folderId?: string | null;
}

export type NodeShape =
  | "rect"
  | "rounded"
  | "circle"
  | "triangle"
  | "diamond"
  | "pentagon"
  | "hexagon"
  | "heptagon";

export interface MindNode {
  id: string;
  mindmapId: string;
  textHtml: string;
  textPlain: string;
  x: number;
  y: number;
  width: number;
  height: number;
  shape: NodeShape;
  borderRadius: number;
  borderColor: string;
  fillColor: string;
  fontSize: number;
  opacity: number;
  locked: boolean;
  zIndex: number;
  recordId: string | null;
  rotation: number;
  groupId: string | null;
  hidden: boolean;
  collapsed: boolean;
  preset: string;
  updatedAt: number;
}

export type EdgeDirection = "forward" | "both" | "none";
export type LineStyle = "solid" | "dashed" | "dotted";
export type PathStyle = "curve" | "straight" | "ortho";

export interface MindEdge {
  id: string;
  mindmapId: string;
  sourceNodeId: string;
  targetNodeId: string;
  direction: EdgeDirection;
  lineStyle: LineStyle;
  pathStyle: PathStyle;
  color: string;
  width: number;
  label: string;
  animated: boolean;
  glow: boolean;
  createdAt: number;
}

export interface Mindmap {
  id: string;
  folderId: string | null;
  name: string;
  viewportX: number;
  viewportY: number,
  zoom: number;
  gridEnabled: boolean;
  snapEnabled: boolean;
  createdAt: number;
  updatedAt: number;
  deletedAt: number | null;
}

export interface MindmapData {
  mindmap: Mindmap;
  nodes: MindNode[];
  edges: MindEdge[];
}

export interface AttachmentView {
  id: string;
  mediaId: string;
  mediaType: "image" | "video" | "audio" | "file";
  displayName: string;
  relPath: string;
  absPath: string;
  originalPath: string;
  copied: boolean;
}

export interface BackupInfo {
  id: string;
  fileName: string;
  source: string;
  size: number;
  checksum: string;
  status: string;
  createdAt: number;
}

export interface RecoveryEntry {
  id: string;
  savedAt: number;
  title: string;
  preview: string;
}

export interface SearchHit {
  kind: "document" | "folder" | "mindmap" | "node";
  id: string;
  parentId: string | null;
  title: string;
  snippet: string;
  updatedAt: number;
}

export interface BootstrapInfo {
  dataDir: string;
  dbPath: string;
  mediaDir: string;
  backupsDir: string;
  version: string;
  schemaVersion: number;
  portable: boolean;
}

export interface ImportSummary {
  folders: number;
  documents: number;
  mindmaps: number;
  nodes: number;
  edges: number;
  media: number;
}

export interface ExportResult {
  path: string;
  count: number;
}

/** One row of the built-in local workspace folder tree. */
export interface WsEntry {
  name: string;
  path: string;
  kind: "dir" | "file";
  ext: string | null;
  size: number;
  updatedAt: number;
}
