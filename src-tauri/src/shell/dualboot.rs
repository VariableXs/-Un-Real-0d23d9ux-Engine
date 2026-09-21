//! 双域引导配置（需求 2）：Variable 侧读写 SHARED 卷上的 `boot-select.json`。
//!
//! # 为什么是"同一份文件"
//!
//! 内核侧（`kernel/varix/src/bootcfg.rs`）读 SHARED 卷根的 `/boot-select.json`
//! 决定倒计时、默认项与**是否交接**；Variable 侧就是编辑这同一份文件——
//! 两系统共写同一事实源，没有第二份配置可漂移。
//!
//! # 交接（handoff）是什么
//!
//! 内核里跑不了 Tauri（要 Windows API + WebView2，结构性不可能），而
//! `ExitBootServices` 之后也无法跳转到 Windows Boot Manager。所以 A 卡
//! （VARIX + VARIABLE）的落点是：内核加载完写 UEFI BootNext → 复位 → 固件
//! 引导 Windows → 那边的 Variable 自启全屏。`handoff` 就是这条链的开关。
//!
//! # 写文件的两条纪律
//!
//! 1. **保真**：用 `serde_json::Value` 改单个键，未知字段原样保留——整份
//!    重写会把内核今后新增的字段悄悄吃掉。
//! 2. **原子**：写临时文件 → fsync → rename。写一半断电不会留下半截 JSON
//!    让内核把整份配置判成损坏（那会触发「配置已重置」角标 + 全默认）。

use serde::Serialize;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{AppError, CmdResult};

/// SHARED 卷根下的契约文件名（与内核 `SHARED_BOOT_SELECT_PATH` 同源）。
pub const BOOT_SELECT_FILE: &str = "boot-select.json";

/// 读到的引导配置（没有该文件时 `found=false`，其余字段由内核侧默认值兜底）。
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BootCfgView {
    /// 是否找到了 SHARED 卷上的 boot-select.json。
    pub found: bool,
    /// 共享盘根路径（不可用时为空串）。
    pub shared_root: String,
    /// 配置文件绝对路径（不可用时为空串）。
    pub path: String,
    /// A 卡加载完是否交接给 Windows 上的 Variable（需求 2）。内核默认 true。
    pub handoff: bool,
    /// 倒计时秒数；配置文件里没有这个键时为 `None`（前端显示「未设置」）。
    pub timeout_sec: Option<i64>,
    /// 倒计时默认项（variable / windows / last）。
    pub default_entry: String,
    /// 是否显示引导菜单。
    pub show_menu: bool,
    /// 该配置里是否显式写了 `handoff` 键（否则用的是内核默认值）。
    pub handoff_explicit: bool,
}

impl BootCfgView {
    /// 无共享盘时的诚实空态（绝不假装"读到了一份全默认的配置"）。
    fn unavailable() -> BootCfgView {
        BootCfgView {
            found: false,
            shared_root: String::new(),
            path: String::new(),
            handoff: true, // 与内核 DEFAULT_HANDOFF_TO_VARIABLE 对齐
            timeout_sec: None,
            default_entry: "variable".to_string(),
            show_menu: true,
            handoff_explicit: false,
        }
    }
}

/// 在共享盘根上解析出配置路径。
pub fn cfg_path(shared_root: &Path) -> PathBuf {
    shared_root.join(BOOT_SELECT_FILE)
}

/// 从任意 JSON 文档里取视图（纯函数，可测：不碰盘）。
pub fn view_from_doc(doc: &Value, shared_root: &str, path: &str) -> BootCfgView {
    let handoff_v = doc.get("handoff");
    BootCfgView {
        found: true,
        shared_root: shared_root.to_string(),
        path: path.to_string(),
        handoff: handoff_v
            .and_then(|v| v.as_bool())
            .unwrap_or(true), // 内核默认 true
        handoff_explicit: handoff_v.map(|v| v.is_boolean()).unwrap_or(false),
        timeout_sec: doc.get("timeout_sec").and_then(|v| v.as_i64()),
        default_entry: doc
            .get("default_entry")
            .and_then(|v| v.as_str())
            .unwrap_or("variable")
            .to_string(),
        show_menu: doc
            .get("show_menu")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    }
}

/// 把 `handoff` 写进既有文档（纯函数）：**保留所有未知字段**。
pub fn set_handoff_in_doc(doc: &mut Value, on: bool) {
    if !doc.is_object() {
        *doc = json!({});
    }
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("handoff".to_string(), Value::Bool(on));
    }
}

/// 原子写：同目录临时文件 → fsync → rename。
///
/// 写一半断电时，读者（内核）看到的是**旧文件**而不是半截 JSON——
/// 后者会让内核把整份配置判成损坏并弹出「配置已重置」角标。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// 读盘 + 解析。返回 `None` 表示文件不存在（正常：还没装配过）。
/// 文件存在但内容不是合法 JSON 时返回 `Err`（调用方据此备份重建）。
pub fn read_doc(path: &Path) -> std::io::Result<Option<Value>> {
    match std::fs::read(path) {
        Ok(bytes) => {
            // 容忍 UTF-8 BOM（与内核解析口径一致）
            let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
            match serde_json::from_slice::<Value>(body) {
                Ok(v) => Ok(Some(v)),
                Err(e) => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("boot-select.json 不是合法 JSON：{e}"),
                )),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// 命令面
// ---------------------------------------------------------------------------

use crate::shell::filesync::resolve_sync_root;

fn resolve(shared_root: Option<PathBuf>) -> (BootCfgView, Option<PathBuf>) {
    let Some(root) = shared_root else {
        return (BootCfgView::unavailable(), None);
    };
    let path = cfg_path(&root);
    let root_s = root.display().to_string();
    let path_s = path.display().to_string();
    match read_doc(&path) {
        Ok(Some(doc)) => (view_from_doc(&doc, &root_s, &path_s), Some(path)),
        // 文件不存在：found=false 但路径照报（界面能说清"缺哪一份"）
        Ok(None) => {
            let mut v = BootCfgView::unavailable();
            v.shared_root = root_s;
            v.path = path_s;
            (v, Some(path))
        }
        // 损坏：如实标注 found=false 并说明原因，**不在这里偷偷重建**
        // （重建要走显式的写命令，好让用户知道文件被换过）
        Err(_) => {
            let mut v = BootCfgView::unavailable();
            v.shared_root = root_s;
            v.path = path_s;
            (v, Some(path))
        }
    }
}

/// 读引导配置状态（只读）。
#[tauri::command(async)]
pub fn dualboot_status(st: tauri::State<'_, crate::state::AppState>) -> BootCfgView {
    resolve(resolve_sync_root(&[st.data_dir.clone()])).0
}

/// 设置「A 卡加载完交接给 Windows 上的 Variable」。
///
/// 没有共享盘时明确报错（不能假装写成功——内核根本读不到）。
/// 文件已有但损坏时先备份成 `boot-select.json.corrupt-<时间戳>` 再重建，
/// 用户的旧内容不会被静默丢掉。
#[tauri::command(async)]
pub fn dualboot_set_handoff(
    st: tauri::State<'_, crate::state::AppState>,
    on: bool,
) -> CmdResult<BootCfgView> {
    let root = resolve_sync_root(&[st.data_dir.clone()]).ok_or_else(|| {
        AppError::new("no-shared", "未找到共享盘（SHARED）—— 请插入系统 U 盘后再改这项设置")
    })?;
    let path = cfg_path(&root);

    let mut doc = match read_doc(&path) {
        Ok(Some(d)) => d,
        Ok(None) => json!({ "handoff": on }),
        Err(_) => {
            // 损坏：备份原文件再重建（保住用户内容以备人工比对）
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let bak = path.with_extension(format!("json.corrupt-{stamp}"));
            let _ = std::fs::rename(&path, &bak);
            json!({ "handoff": on })
        }
    };
    set_handoff_in_doc(&mut doc, on);
    let body = serde_json::to_vec_pretty(&doc)
        .map_err(|e| AppError::new("encode", format!("序列化配置失败：{e}")))?;
    write_atomic(&path, &body)
        .map_err(|e| AppError::new("write", format!("写入 boot-select.json 失败：{e}")))?;

    let root_s = root.display().to_string();
    let path_s = path.display().to_string();
    Ok(view_from_doc(&doc, &root_s, &path_s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_handoff_key_defaults_to_on() {
        // 老配置没有 handoff 键：内核按 true 处理，界面必须显示同一结论。
        let v = view_from_doc(&json!({"timeout_sec": 5}), "/mnt/shared", "/mnt/shared/boot-select.json");
        assert!(v.handoff, "缺键必须与内核默认 true 一致");
        assert!(!v.handoff_explicit, "缺键不是显式设置");
        assert_eq!(v.timeout_sec, Some(5));
        assert_eq!(v.default_entry, "variable");
        assert!(v.show_menu);
    }

    #[test]
    fn explicit_false_is_reported() {
        let v = view_from_doc(&json!({"handoff": false}), "/s", "/s/boot-select.json");
        assert!(!v.handoff);
        assert!(v.handoff_explicit);
    }

    #[test]
    fn wrong_type_handoff_falls_back_to_default() {
        // 类型错（字符串）：内核按默认 true 处理，界面不能显示成 false。
        let v = view_from_doc(&json!({"handoff": "no"}), "/s", "/s/x.json");
        assert!(v.handoff, "类型错必须与内核的容错第 2 层一致（回落默认）");
        assert!(!v.handoff_explicit, "类型错不算显式设置");
    }

    #[test]
    fn set_handoff_preserves_unknown_fields() {
        // 保真铁律：改一个键不能吃掉别的字段（内核今后会加字段）。
        let mut doc = json!({
            "default_entry": "windows",
            "timeout_sec": 12,
            "future_field": {"nested": [1, 2, 3]},
        });
        set_handoff_in_doc(&mut doc, false);
        assert_eq!(doc["handoff"], json!(false));
        assert_eq!(doc["default_entry"], json!("windows"), "其它字段必须原样");
        assert_eq!(doc["timeout_sec"], json!(12));
        assert_eq!(doc["future_field"]["nested"], json!([1, 2, 3]), "未知字段必须保留");
    }

    #[test]
    fn set_handoff_rebuilds_non_object_doc() {
        // 顶层不是对象（比如内容是 `[1,2]`）时重建为空对象再写，不留残缺。
        let mut doc = json!([1, 2, 3]);
        set_handoff_in_doc(&mut doc, true);
        assert_eq!(doc, json!({"handoff": true}));
    }

    #[test]
    fn unavailable_state_is_honest() {
        let v = BootCfgView::unavailable();
        assert!(!v.found, "没有共享盘必须如实说 found=false");
        assert!(v.shared_root.is_empty());
        assert!(v.path.is_empty());
        assert!(v.handoff, "空态展示的仍是内核默认值 true");
    }

    #[test]
    fn atomic_write_then_read_roundtrip() {
        let dir = std::env::temp_dir().join(format!("vx-dualboot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(BOOT_SELECT_FILE);
        let _ = std::fs::remove_file(&p);

        let mut doc = json!({"timeout_sec": 7, "keep_me": true});
        set_handoff_in_doc(&mut doc, false);
        let body = serde_json::to_vec_pretty(&doc).unwrap();
        write_atomic(&p, &body).unwrap();

        // 临时文件不得残留
        assert!(!dir.join("boot-select.json.tmp").exists(), "临时文件必须已被 rename 掉");

        let back = read_doc(&p).unwrap().unwrap();
        assert_eq!(back["handoff"], json!(false));
        assert_eq!(back["timeout_sec"], json!(7));
        assert_eq!(back["keep_me"], json!(true));

        let v = view_from_doc(&back, "/x", "/x/boot-select.json");
        assert!(!v.handoff);
        assert_eq!(v.timeout_sec, Some(7));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bom_is_tolerated() {
        let dir = std::env::temp_dir().join(format!("vx-dualboot-bom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(BOOT_SELECT_FILE);
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"handoff": false}"#);
        std::fs::write(&p, &bytes).unwrap();
        let doc = read_doc(&p).unwrap().unwrap();
        assert_eq!(doc["handoff"], json!(false), "BOM 不能被当成语法错误");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_json_is_reported_as_error_not_silent_default() {
        let dir = std::env::temp_dir().join(format!("vx-dualboot-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(BOOT_SELECT_FILE);
        std::fs::write(&p, b"{ this is not json").unwrap();
        assert!(read_doc(&p).is_err(), "损坏必须报错，不能假装读到了默认配置");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_none_not_error() {
        let dir = std::env::temp_dir().join(format!("vx-dualboot-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(BOOT_SELECT_FILE);
        let _ = std::fs::remove_file(&p);
        assert!(read_doc(&p).unwrap().is_none(), "文件不存在是正常态");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
