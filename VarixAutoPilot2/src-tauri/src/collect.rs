//! 三源采集：对话列表 / 模型 / 工作目录。
//!
//! 三个数据源，全部实测可读（不是推演）：
//! ① 对话列表  → CDP `div.conversation-item`（含 `data-conversation-id`）
//! ② 模型     → CDP `button.cr-model-selector__trigger[title]`
//! ③ 工作目录 → `~/.workbuddy/sessions/*.json` 的 `cwd`
//!
//! ★ 关键：为什么工作目录不从 DOM 读★
//! 实测 DOM 里**没有**任何 data-cwd / data-workspace 属性，title 里也没有路径。
//! 硬编选择器只会得空。而 `conversation-item` 带 `data-conversation-id`，
//! **该值就是 sessionId**（三样本逐一验证过），于是可精确配对。
//!
//! ★ 错配的教训 ★
//! Node 版第一版按「最近更新的会话顺序」硬对齐，把当前对话配到了
//! `_attic/…/cdp`（那只是跑脚本的目录）。**顺序对齐在多会话并行时必然错配，
//! 而错配出来的路径比没有路径更危险——它看起来是真的。**
//! 查不到的那条如实标「未查到」，不编。

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::cdp::Cdp;

/// 一个对话的完整信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvInfo {
    pub index: usize,
    /// 配对键：data-conversation-id（=== sessionId）
    pub conv_id: String,
    pub title: String,
    /// 相对时间（"7小时前"）
    pub rel_time: String,
    /// 模型名。**只有当前对话读得到**（顶栏选择器），其余为空——
    /// 这是 WorkBuddy 的信息限制，不是采集缺陷。
    pub model: String,
    pub selected: bool,
    pub cwd: String,
    /// 配对可信度：精确 / 未查到 / 无 id
    pub cwd_confidence: String,
}

/// 一次采集的完整快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub at: u64,
    pub version: String,
    pub conversation_title: String,
    pub current_model: String,
    pub sending: bool,
    pub send_label: String,
    pub editor_chars: i64,
    pub editor_visible: bool,
    pub convs: Vec<ConvInfo>,
    pub session_count: usize,
    /// cwd 分布（降序，最多 6 项）
    pub cwd_histogram: Vec<CwdCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CwdCount {
    pub cwd: String,
    pub sessions: usize,
}

/// 会话文件缓存项。
#[derive(Clone)]
struct CwdEntry {
    cwd: String,
    mtime: SystemTime,
    /// 缓存命中时直接复用 sid，避免重新反序列化
    sid: String,
}

/// 会话文件里只需要的字段。
#[derive(Deserialize)]
struct SessionMeta {
    #[serde(default)]
    cwd: String,
    #[serde(rename = "sessionId", default)]
    session_id: String,
}

/// 刷新产物。
pub struct CwdIndex {
    pub by_sid: HashMap<String, String>,
    pub by_cwd: HashMap<String, usize>,
}

/// cwd 缓存。**mtime 判失效**，避免每 1.5 秒重读 276 个文件。
///
/// ★ 性能关键 ★
/// Node 版每次快照都 `readdir` + 逐个 `read_to_string` + 反序列化，
/// 实测 276 个会话 → 每 1.5s 读盘 276 次 ≈ 每天 28MB 纯浪费。
/// 而 cwd 几乎不变。改用 mtime 判失效后，
/// 稳态下每轮只做 276 次 `metadata()`（约 1–3ms），不再读文件内容。
pub struct CwdCache {
    dir: Option<PathBuf>,
    map: HashMap<String, CwdEntry>,
}

impl CwdCache {
    pub fn new() -> Self {
        CwdCache {
            dir: std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|h| PathBuf::from(h).join(".workbuddy").join("sessions")),
            map: HashMap::new(),
        }
    }

    /// 刷新缓存。
    ///
    /// - `by_sid`：sessionId → cwd（与 DOM 的 convId 精确配对）
    /// - `by_cwd`：cwd → 会话数（分布统计）
    pub fn refresh(&mut self) -> CwdIndex {
        let mut by_sid: HashMap<String, String> = HashMap::new();
        let mut by_cwd: HashMap<String, usize> = HashMap::new();

        let Some(dir) = self.dir.clone() else {
            return CwdIndex { by_sid, by_cwd };
        };
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return CwdIndex { by_sid, by_cwd };
        };

        // 本轮见到的文件名，用于清理已删除会话的缓存
        let mut alive: Vec<String> = Vec::new();

        for e in rd.flatten() {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Ok(mt) = path.metadata().and_then(|m| m.modified()) else {
                continue;
            };
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(String::from) else {
                continue;
            };
            if stem.is_empty() {
                continue;
            }
            alive.push(stem.clone());

            // ★ 缓存命中判定：mtime 未变就跳过读盘 ★
            let (cwd, sid) = match self.map.get(&stem) {
                Some(hit) if hit.mtime == mt => (hit.cwd.clone(), hit.sid.clone()),
                _ => {
                    let Ok(text) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    let Ok(meta) = serde_json::from_str::<SessionMeta>(&text) else {
                        continue;   // 单个坏文件不拖垮整轮
                    };
                    if meta.cwd.is_empty() {
                        continue;
                    }
                    (meta.cwd, meta.session_id)
                }
            };

            self.map.insert(
                stem,
                CwdEntry {
                    cwd: cwd.clone(),
                    mtime: mt,
                    sid: sid.clone(),
                },
            );
            if !sid.is_empty() {
                by_sid.insert(sid, cwd.clone());
            }
            *by_cwd.entry(cwd).or_insert(0) += 1;
        }

        // 清理已删除会话的缓存，否则无限增长
        self.map.retain(|k, _| alive.contains(k));

        CwdIndex { by_sid, by_cwd }
    }
}

// ---------------------------------------------------------------------------
// 页面探针（在浏览器上下文里跑）
// ---------------------------------------------------------------------------

/// 生成页面探针 JS。
///
/// 用字符串而非外部 .js 文件：Tauri 打包时少一个资源依赖，
/// 且这段代码要注入 `Runtime.evaluate`，内联最直接。
fn page_probe_js() -> String {
    r#"
(() => {
  const txt = (e) => (e && (e.innerText || e.textContent) || '').replace(/\s+/g, ' ').trim();

  // ① 对话项
  const items = Array.from(document.querySelectorAll('div.conversation-item'));
  const convs = items.map((e, idx) => {
    const cls = (typeof e.className === 'string') ? e.className : '';
    const selected = /active|selected|current/i.test(cls) ||
      !!e.querySelector('[class*="active"],[class*="selected"]');
    // ★ 配对键：实测该属性值 === sessions/*.json 的 sessionId ★
    const convId = e.getAttribute('data-conversation-id') || '';

    const raw = (e.innerText || '').replace(/\r/g, '');
    const lines = raw.split('\n').map(s => s.trim()).filter(Boolean);
    let relTime = '';
    let titleLines = lines;
    const last = lines[lines.length - 1] || '';
    if (/^\d+\s*(秒|分钟|小时|天|周)前$/.test(last) || last === '刚刚') {
      relTime = last;
      titleLines = lines.slice(0, -1);
    }
    const title = (titleLines[0] || '(无标题)').replace(/\s+/g, ' ').trim().slice(0, 120);
    return { index: idx, conv_id: convId, title: title, rel_time: relTime, selected: selected };
  });

  // ② 当前对话模型（唯一可靠来源：顶栏选择器）
  const mb = document.querySelector('button.cr-model-selector__trigger');
  const currentModel = mb ? (mb.getAttribute('title') || txt(mb)) : '';

  // ③ 当前对话标题
  const tt = document.querySelector('span.workbuddy-topbar-title');
  const conversationTitle = tt ? txt(tt) : '';

  // ④ 发送键状态：--sending 时它的语义是「停止」，不能发
  const sb = document.querySelector('button.cr-send-button');
  const sbCls = sb ? ((typeof sb.className === 'string') ? sb.className : '') : '';
  const sending = /--sending/.test(sbCls);
  const sendLabel = sb ? (sb.getAttribute('aria-label') || txt(sb) || '') : '';

  // ⑤ 编辑器
  const ed = document.querySelector('div[data-slate-editor="true"][contenteditable="true"]');
  let edChars = -1, edVisible = false;
  if (ed) {
    // ★★ 必须排除 placeholder ★★
    // 实测：Slate 的 placeholder（「今天帮你做些什么？…」）是**真实子元素**，
    // 所以 innerText 与 textContent **都**会把它算进去（实测均为 25）。
    // 正解：克隆 DOM → 删掉 [data-slate-placeholder] → 再数。
    // 详见 engine.rs 的 EDITOR_CHARS_JS 注释（含完整 DOM 结构）。
    const clone = ed.cloneNode(true);
    clone.querySelectorAll('[data-slate-placeholder]').forEach(n => n.remove());
    edChars = (clone.textContent || '').trim().length;
    const r = ed.getBoundingClientRect();
    edVisible = r.width > 40 && r.height > 18;
  }

  // ⑥ 版本
  const vb = document.querySelector('.conversation-list-version-badge');
  const version = vb ? txt(vb) : '';

  return {
    convs: convs, current_model: currentModel, conversation_title: conversationTitle,
    sending: sending, send_label: sendLabel,
    editor_chars: edChars, editor_visible: edVisible, version: version
  };
})()
"#
    .to_string()
}

/// 采集一份快照（cwd 索引由调用方提供）。
pub async fn probe_with_index(cdp: &Cdp, idx: CwdIndex) -> Result<Snapshot> {
    let raw = cdp.eval(&page_probe_js()).await?;

    let now_ms = std::time::SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let mut convs: Vec<ConvInfo> = Vec::new();
    if let Some(arr) = raw.get("convs").and_then(|v| v.as_array()) {
        for (i, c) in arr.iter().enumerate() {
            let conv_id = c
                .get("conv_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let selected = c.get("selected").and_then(|v| v.as_bool()).unwrap_or(false);

            // ★ 精确配对：用 convId 查表，不做任何排序猜测 ★
            let (cwd, confidence) = if conv_id.is_empty() {
                (String::new(), "无 conversation-id".to_string())
            } else if let Some(w) = idx.by_sid.get(&conv_id) {
                (w.clone(), "精确（conversation-id 命中）".to_string())
            } else {
                (String::new(), "未查到该 session".to_string())
            };

            convs.push(ConvInfo {
                index: i,
                conv_id,
                title: c
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(无标题)")
                    .to_string(),
                rel_time: c
                    .get("rel_time")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                // 模型：只有当前对话读得到（顶栏），其余如实留空
                model: if selected {
                    raw.get("current_model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                } else {
                    String::new()
                },
                selected,
                cwd,
                cwd_confidence: confidence,
            });
        }
    }

    let mut hist: Vec<CwdCount> = idx
        .by_cwd
        .iter()
        .map(|(c, n)| CwdCount {
            cwd: c.clone(),
            sessions: *n,
        })
        .collect();
    hist.sort_by(|a, b| b.sessions.cmp(&a.sessions));
    hist.truncate(6);

    Ok(Snapshot {
        at: now_ms,
        version: raw
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        conversation_title: raw
            .get("conversation_title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        current_model: raw
            .get("current_model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        sending: raw.get("sending").and_then(|v| v.as_bool()).unwrap_or(false),
        send_label: raw
            .get("send_label")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        editor_chars: raw.get("editor_chars").and_then(|v| v.as_i64()).unwrap_or(-1),
        editor_visible: raw
            .get("editor_visible")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        session_count: idx.by_cwd.values().sum(),
        convs,
        cwd_histogram: hist,
    })
}
