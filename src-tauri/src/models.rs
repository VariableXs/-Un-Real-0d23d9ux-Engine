use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub sort_order: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub folder_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub content_html: String,
    #[serde(default)]
    pub content_text: String,
    pub favorite: bool,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentInput {
    pub id: String,
    pub title: String,
    pub content_html: String,
    pub content_text: String,
    pub favorite: Option<bool>,
    pub folder_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListFilter {
    pub view: Option<String>, // all | favorites | trash
    pub folder_id: Option<String>,
    pub query: Option<String>,
    pub tag: Option<String>,
    pub sort: Option<String>, // updated | created
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mindmap {
    pub id: String,
    pub folder_id: Option<String>,
    pub name: String,
    pub viewport_x: f64,
    pub viewport_y: f64,
    pub zoom: f64,
    pub grid_enabled: bool,
    pub snap_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    pub mindmap_id: String,
    pub text_html: String,
    pub text_plain: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub shape: String,
    pub border_radius: f64,
    pub border_color: String,
    pub fill_color: String,
    pub font_size: f64,
    pub opacity: f64,
    pub locked: bool,
    pub z_index: i64,
    pub record_id: Option<String>,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub preset: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub id: String,
    pub mindmap_id: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub direction: String,
    pub line_style: String,
    pub path_style: String,
    pub color: String,
    pub width: f64,
    pub label: String,
    pub animated: bool,
    #[serde(default)]
    pub glow: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    pub file_name: String,
    pub original_path: String,
    pub copied: bool,
    pub checksum: String,
    pub media_type: String,
    pub size: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub document_id: Option<String>,
    pub node_id: Option<String>,
    pub media_id: String,
    pub display_name: String,
    pub rel_path: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentView {
    pub id: String,
    pub media_id: String,
    pub media_type: String,
    pub display_name: String,
    pub rel_path: String,
    pub abs_path: String,
    pub original_path: String,
    pub copied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub id: String,
    pub file_name: String,
    pub source: String,
    pub size: i64,
    pub checksum: String,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryEntry {
    pub id: String,
    pub saved_at: i64,
    pub title: String,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: String, // document | folder | mindmap | node
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub snippet: String,
    pub updated_at: i64,
}
