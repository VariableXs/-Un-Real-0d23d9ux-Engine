CREATE TABLE IF NOT EXISTS folders (
  id TEXT PRIMARY KEY,
  parent_id TEXT,
  name TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);

CREATE TABLE IF NOT EXISTS documents (
  id TEXT PRIMARY KEY,
  folder_id TEXT,
  title TEXT NOT NULL DEFAULT '',
  content_html TEXT NOT NULL DEFAULT '',
  content_text TEXT NOT NULL DEFAULT '',
  favorite INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_documents_folder ON documents(folder_id);
CREATE INDEX IF NOT EXISTS idx_documents_updated ON documents(updated_at);

CREATE TABLE IF NOT EXISTS tags (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS document_tags (
  document_id TEXT NOT NULL,
  tag_id TEXT NOT NULL,
  PRIMARY KEY (document_id, tag_id)
);

CREATE TABLE IF NOT EXISTS mindmaps (
  id TEXT PRIMARY KEY,
  folder_id TEXT,
  name TEXT NOT NULL,
  viewport_x REAL NOT NULL DEFAULT 0,
  viewport_y REAL NOT NULL DEFAULT 0,
  zoom REAL NOT NULL DEFAULT 1,
  grid_enabled INTEGER NOT NULL DEFAULT 1,
  snap_enabled INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);

CREATE TABLE IF NOT EXISTS nodes (
  id TEXT PRIMARY KEY,
  mindmap_id TEXT NOT NULL,
  text_html TEXT NOT NULL DEFAULT '',
  text_plain TEXT NOT NULL DEFAULT '',
  x REAL NOT NULL DEFAULT 0,
  y REAL NOT NULL DEFAULT 0,
  width REAL NOT NULL DEFAULT 220,
  height REAL NOT NULL DEFAULT 80,
  shape TEXT NOT NULL DEFAULT 'rounded',
  border_radius REAL NOT NULL DEFAULT 12,
  border_color TEXT NOT NULL DEFAULT '#5b7bd0',
  fill_color TEXT NOT NULL DEFAULT 'rgba(13,20,38,0.85)',
  font_size REAL NOT NULL DEFAULT 14,
  opacity REAL NOT NULL DEFAULT 1,
  locked INTEGER NOT NULL DEFAULT 0,
  z_index INTEGER NOT NULL DEFAULT 0,
  record_id TEXT,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_nodes_mindmap ON nodes(mindmap_id);

CREATE TABLE IF NOT EXISTS edges (
  id TEXT PRIMARY KEY,
  mindmap_id TEXT NOT NULL,
  source_node_id TEXT NOT NULL,
  target_node_id TEXT NOT NULL,
  direction TEXT NOT NULL DEFAULT 'forward',
  line_style TEXT NOT NULL DEFAULT 'solid',
  path_style TEXT NOT NULL DEFAULT 'curve',
  color TEXT NOT NULL DEFAULT '#7f9bd9',
  width REAL NOT NULL DEFAULT 1.5,
  label TEXT NOT NULL DEFAULT '',
  animated INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_edges_mindmap ON edges(mindmap_id);

CREATE TABLE IF NOT EXISTS media (
  id TEXT PRIMARY KEY,
  file_name TEXT NOT NULL,
  original_path TEXT NOT NULL DEFAULT '',
  copied INTEGER NOT NULL DEFAULT 1,
  checksum TEXT NOT NULL DEFAULT '',
  media_type TEXT NOT NULL,
  size INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS attachments (
  id TEXT PRIMARY KEY,
  document_id TEXT,
  node_id TEXT,
  media_id TEXT NOT NULL,
  display_name TEXT NOT NULL,
  rel_path TEXT NOT NULL,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS backups (
  id TEXT PRIMARY KEY,
  file_name TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'manual',
  size INTEGER NOT NULL DEFAULT 0,
  checksum TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'ok',
  created_at INTEGER NOT NULL
);
