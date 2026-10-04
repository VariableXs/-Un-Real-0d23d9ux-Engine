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
    /// 最近一次写配置后的引导分区（U 盘 ESP）副本同步结果：
    /// true=已同步；false=未同步或从未尝试（`esp_sync_note` 说明原因）。
    pub esp_synced: bool,
    /// 同步结果的人话说明（空串=本次操作没有同步语义，如只读状态查询）。
    pub esp_sync_note: String,
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
            esp_synced: false,
            esp_sync_note: String::new(),
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
        esp_synced: false,
        esp_sync_note: String::new(),
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

// ---------------------------------------------------------------------------
// ESP 副本同步（S0.2 配置桥 · 提权通道）
// ---------------------------------------------------------------------------
//
// 内核在引导期经 Limine internal module 读**引导卷根**的 boot-select.json
// 副本（SHARED 真盘 FS 尚未落地，这是 S0.2 方案 A 的实机通道）。所以
// Variable 写完 SHARED 真相源后，要把副本刷到同一块 U 盘的 ESP 分区上。
//
// 写 ESP 需要管理员（assign 盘符）+ 落点实证闸门：目标分区必须是
// 「同一物理盘上的 ESP 且根下有 limine.conf」——两条任一不满足就退出不动，
// 内置盘在物理上就够不着（我们只按 SHARED 所在盘号找同盘 ESP）。

/// 一次同步尝试的结论（如实三态，绝不把"没同步"说成"同步了"）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EspSyncOutcome {
    /// 副本已刷新且哈希一致。
    Synced,
    /// 用户拒绝了管理员授权（UAC 取消）——SHARED 已写好，ESP 副本保持旧值。
    Declined,
    /// 非 Windows 宿主（开发/跨平台构建）：没有 ESP 概念，跳过。
    NotWindows,
    /// 同步失败（原因入账，SHARED 真相源不受影响）。
    Failed(String),
}

/// 同步日志标记（helper .ps1 与 Rust 侧的契约）。
const LOG_OK: &str = "ESP-SYNC-OK";
const LOG_FAIL: &str = "ESP-SYNC-FAIL";

/// helper 脚本与日志的落点（temp 目录，随用随建，幂等覆盖）。
#[cfg(windows)]
fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

/// 生成提权 helper 脚本（纯函数，可测）。ASCII-only 纪律：PowerShell 对
/// 无 BOM 非 ASCII 内容会被 GBK 吞引号（实测教训），脚本正文全部英文，
/// 共享盘根只允许 ASCII 路径（盘符根天然满足），否则拒绝同步。
#[cfg(windows)]
pub fn esp_sync_helper_script(shared_root: &str) -> Result<String, EspSyncOutcome> {
    if !shared_root.bytes().all(|b| b.is_ascii() && b != 0) {
        return Err(EspSyncOutcome::Failed(
            "shared root path is not ASCII-safe".to_string(),
        ));
    }
    // 统一成不带尾反斜杠的盘根形态（如 E:），注入脚本前剥掉可能的引号
    let root = shared_root.trim_end_matches(['"', '\\']);
    let log = temp_file("vx-esp-sync.log");
    Ok(format!(
        r#"$ErrorActionPreference = 'Stop'
$log = '{log}'
function Say($m) {{ Add-Content -Path $log -Value $m }}
$assigned = $false
try {{
  $src = '{root}\\boot-select.json'
  if (-not (Test-Path $src)) {{ throw 'source boot-select.json not found on SHARED' }}
  $srcHash = (Get-FileHash $src -Algorithm SHA256).Hash
  # The partition that hosts SHARED (by its drive letter), then the ESP on the SAME disk.
  $letter = (Get-Item $src).PSDrive.Name
  $part = Get-Partition -DriveLetter $letter
  $disk = $part.DiskNumber
  $esp = Get-Partition -DiskNumber $disk | Where-Object {{
    $_.GptType -eq '{{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}}'
  }} | Select-Object -First 1
  if (-not $esp) {{ throw 'no ESP partition on the SHARED disk' }}
  $let = $null
  foreach ($c in @('L','N','O','P','Q','R')) {{
    if (-not (Get-PSDrive -Name $c -ErrorAction SilentlyContinue)) {{ $let = $c; break }}
  }}
  if (-not $let) {{ throw 'no free drive letter' }}
  $dp = Join-Path $env:TEMP 'vx-esp-assign.txt'
  [IO.File]::WriteAllText($dp, "select disk $disk`r`nselect partition $($esp.PartitionNumber)`r`nassign letter=$let`r`n", [Text.Encoding]::ASCII)
  diskpart /s $dp | Out-Null
  $assigned = $true
  $espRoot = "${{let}}:"
  # Placement gate: the target ESP must be the VARIX ESP (limine.conf at root).
  # Anything else = wrong target: remove the letter and refuse to touch it.
  if (-not (Test-Path (Join-Path $espRoot 'limine.conf'))) {{
    throw 'target ESP has no limine.conf - refusing to touch a non-VARIX ESP'
  }}
  Copy-Item $src (Join-Path $espRoot 'boot-select.json') -Force
  $espHash = (Get-FileHash (Join-Path $espRoot 'boot-select.json') -Algorithm SHA256).Hash
  if ($srcHash -ne $espHash) {{ throw 'hash mismatch after copy' }}
  Say "{LOG_OK} $espHash"
}} catch {{
  Say "{LOG_FAIL} $($_.Exception.Message)"
}} finally {{
  if ($assigned) {{
    try {{
      $rm = Join-Path $env:TEMP 'vx-esp-remove.txt'
      [IO.File]::WriteAllText($rm, "select disk $disk`r`nselect partition $($esp.PartitionNumber)`r`nremove letter=$let`r`n", [Text.Encoding]::ASCII)
      diskpart /s $rm | Out-Null
    }} catch {{ }}
  }}
}}
"#,
        log = log.display(),
        root = root,
    ))
}

/// 解析 helper 日志 → 结论（纯函数）。
pub fn parse_esp_sync_log(content: &str) -> EspSyncOutcome {
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix(LOG_OK) {
            let _ = rest; // 哈希前缀仅作证据留档
            return EspSyncOutcome::Synced;
        }
    }
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix(LOG_FAIL) {
            return EspSyncOutcome::Failed(rest.trim().to_string());
        }
    }
    // 无任何标记：提权进程根本没跑起来（UAC 被取消的最典型表象）
    EspSyncOutcome::Declined
}

/// 结论 → 用户可读的一句话（与 HandoffCard 的中文文案同一语言口径）。
pub fn esp_sync_note(o: &EspSyncOutcome) -> String {
    match o {
        EspSyncOutcome::Synced => "已同步到引导分区副本（下次引导即生效）".to_string(),
        EspSyncOutcome::Declined => {
            "引导分区副本未同步（需要管理员授权）——下次引导将沿用旧副本；重新开关一次可重试".to_string()
        }
        EspSyncOutcome::NotWindows => String::new(), // 非 Windows 宿主静默：界面无此语义
        EspSyncOutcome::Failed(e) => format!("引导分区副本同步失败：{e}（SHARED 上的配置已保存）"),
    }
}

#[cfg(windows)]
fn run_elevated_helper(ps1: &Path, log: &Path) -> Result<(), String> {
    use std::process::{Command, Stdio};
    // 先清掉旧日志：空日志 + 无标记 = UAC 被取消
    let _ = std::fs::remove_file(log);
    // 提权拉起：Start-Process -Verb RunAs 弹 UAC；-Wait 等内层跑完。
    let inner = format!(
        "Start-Process powershell.exe -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','{}'",
        ps1.display().to_string().replace('\'', "''")
    );
    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &inner])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("launch failed: {e}"))?;
    // 有界等待：UAC 弹窗 + diskpart 两次 + 拷贝，2 分钟绰绰有余
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    return Err("elevated helper timed out".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    }
}

/// 写完 SHARED 后刷新引导卷 ESP 副本（best-effort：真相源在 SHARED，
/// 同步失败只如实上报，不影响写配置的成功语义）。
#[cfg(windows)]
pub fn sync_esp_copy(shared_root: &Path) -> EspSyncOutcome {
    let root_s = shared_root.display().to_string();
    let script = match esp_sync_helper_script(&root_s) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let ps1 = temp_file("vx-esp-sync.ps1");
    // BOM + ASCII 正文：PS5.1 对无 BOM 文件的编码猜测是踩过的坑
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(script.as_bytes());
    if std::fs::write(&ps1, &bytes).is_err() {
        return EspSyncOutcome::Failed("cannot write helper script".to_string());
    }
    let log = temp_file("vx-esp-sync.log");
    if let Err(e) = run_elevated_helper(&ps1, &log) {
        return EspSyncOutcome::Failed(e);
    }
    let content = std::fs::read_to_string(&log).unwrap_or_default();
    parse_esp_sync_log(&content)
}

/// 非 Windows 宿主：没有 ESP 概念，如实跳过。
#[cfg(not(windows))]
pub fn sync_esp_copy(_shared_root: &Path) -> EspSyncOutcome {
    EspSyncOutcome::NotWindows
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
        assert!(!v.esp_synced, "空态没有同步语义");
        assert!(v.esp_sync_note.is_empty());
    }

    // ---- S0.2 ESP 副本同步（提权通道） -------------------------------------

    #[test]
    fn helper_script_is_ascii_and_carries_all_gates() {
        let s = esp_sync_helper_script("E:\\").unwrap();
        // ASCII-only 纪律（PS5.1 无 BOM 非 ASCII 会吞引号——实测教训）
        assert!(s.bytes().all(|b| b.is_ascii()), "helper 必须纯 ASCII");
        // 落点实证闸门：目标 ESP 必须有 limine.conf
        assert!(s.contains("limine.conf"), "必须有 VARIX ESP 闸门");
        // 同盘定位：SHARED 所在盘号 → 同盘 ESP（内置盘物理够不着）
        assert!(s.contains("Get-Partition -DriveLetter"), "必须从 SHARED 盘符反查");
        assert!(
            s.contains("c12a7328-f81f-11d2-ba4b-00a0c93ec93b"),
            "必须钉死 GPT ESP 分区类型"
        );
        assert!(s.contains("Get-Partition -DiskNumber $disk"), "必须限同盘");
        // 哈希回读
        assert!(s.contains("hash mismatch"), "必须哈希比对");
        // 日志契约标记
        assert!(s.contains(LOG_OK) && s.contains(LOG_FAIL));
        // 根路径注入（去掉尾反斜杠后拼接）
        assert!(s.contains("E:"), "共享盘根必须注入脚本");
        // finally 摘字母
        assert!(s.contains("remove letter"), "用后必须摘盘符");
    }

    #[test]
    fn helper_script_rejects_non_ascii_root() {
        let r = esp_sync_helper_script("E:\\中文\\");
        assert!(matches!(r, Err(EspSyncOutcome::Failed(_))));
    }

    #[test]
    fn sync_log_parses_to_honest_outcomes() {
        assert_eq!(
            parse_esp_sync_log("ESP-SYNC-OK 3FA9…"),
            EspSyncOutcome::Synced
        );
        assert_eq!(
            parse_esp_sync_log("ESP-SYNC-FAIL hash mismatch after copy"),
            EspSyncOutcome::Failed("hash mismatch after copy".to_string())
        );
        // 空日志（UAC 被取消的典型表象）→ Declined，不是 Failed
        assert_eq!(parse_esp_sync_log(""), EspSyncOutcome::Declined);
    }

    #[test]
    fn sync_notes_are_actionable_not_blamey() {
        let n = esp_sync_note(&EspSyncOutcome::Synced);
        assert!(n.contains("已同步"), "成功要说清已同步");
        let d = esp_sync_note(&EspSyncOutcome::Declined);
        assert!(d.contains("未同步") && d.contains("重试"), "拒绝要说明后果与出路");
        let f = esp_sync_note(&EspSyncOutcome::Failed("boom".into()));
        assert!(f.contains("已保存"), "失败必须说明 SHARED 真相源未受影响");
        assert!(esp_sync_note(&EspSyncOutcome::NotWindows).is_empty());
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
