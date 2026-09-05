//! L3 shell — U 盘完全便携（M8）：
//! - 一键全量打包：数据目录 → 便携包（`.portable` 标记 + `data/` + `Variable.exe` + `manifest.json`）
//! - SHA-256 完整性校验：manifest.json 基线（打包时刻），用于拷贝后/换机后核验
//! - 路径重映射：数据库自 v1 起就存相对路径（`media/<file>` / attachments rel_path /
//!   Workspace 相对树），便携拷贝后天然无需改库 —— 仅 manifest 用相对路径记录
//! - 拔出保护（仅便携模式）：1s 轮询数据卷 + 周期性 WAL checkpoint（减小意外拔出损失面），
//!   卷消失 → 发 `usb://removed` 事件；下次启动由 boot.rs 从最新备份自动恢复
//! - 零网络 / 零遥测：一切仅本机文件操作

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

pub const MANIFEST_FILE: &str = "manifest.json";
pub const MARKER_FILE: &str = ".portable";
pub const EXE_NAME: &str = "Variable.exe";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    /// 相对便携包根的路径（`/` 分隔）：`data/db/variable.db`、`Variable.exe`
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub generated_at: u64,
    pub app_version: String,
    pub files: Vec<ManifestEntry>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsbStatus {
    pub portable: bool,
    pub data_dir: String,
    pub drive_removable: bool,
    pub manifest_exists: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileCheck {
    pub path: String,
    pub ok: bool,
    pub expected: String,
    pub actual: String,
    pub size: u64,
}

/// 打包进度（真实文件计数，非时间线）。事件：`usb://progress`
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PackProgress {
    pub phase: String, // collect | copy | exe | manifest | done
    pub done: usize,
    pub total: usize,
    pub current: Option<String>,
}

// ---------- 内部工具 ----------

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// 前缀包含判断（带 `\` 边界，防同级目录误判）。
fn contains(a: &Path, b: &Path) -> bool {
    let (sa, sb) = (norm(a), norm(b));
    sa.starts_with(&sb) && sa.as_bytes().get(sb.len()) == Some(&b'\\')
}

fn sha256_file(p: &Path) -> CmdResult<(String, u64)> {
    let mut f = fs::File::open(p).map_err(|e| AppError::io(format!("无法读取文件 / Cannot read file {}: {e}", p.display())))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut f, &mut hasher)
        .map_err(|e| AppError::io(format!("哈希失败 / Hash failed {}: {e}", p.display())))?;
    let size = f.metadata().map_err(|e| AppError::io(e.to_string()))?.len();
    Ok((format!("{:x}", hasher.finalize()), size))
}

/// 递归收集目录下全部文件（排序保证 manifest 稳定）。
fn collect_files(root: &Path) -> Vec<(PathBuf, String)> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(PathBuf, String)>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for item in rd.flatten() {
            let p = item.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else if p.is_file() {
                let rel = p
                    .strip_prefix(base)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((p, rel));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

// ---------- 状态 ----------

/// 数据卷是否为可移动介质（U 盘）。
#[cfg(windows)]
fn drive_is_removable(p: &Path) -> bool {
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::Win32::System::WindowsProgramming::DRIVE_REMOVABLE;
    let s = p.to_string_lossy();
    let bytes = s.as_bytes();
    // 取 "X:\" 盘符根
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' || (bytes[2] != b'\\' && bytes[2] != b'/') {
        return false;
    }
    let root: String = s[..3].to_string();
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { GetDriveTypeW(windows::core::PCWSTR(wide.as_ptr())) == DRIVE_REMOVABLE }
}

#[cfg(not(windows))]
fn drive_is_removable(_p: &Path) -> bool {
    false
}

#[tauri::command]
pub fn usb_status(st: tauri::State<AppState>) -> CmdResult<UsbStatus> {
    let portable = crate::state::is_portable();
    let manifest_exists = portable
        && std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join(MANIFEST_FILE)))
            .map(|m| m.exists())
            .unwrap_or(false);
    Ok(UsbStatus {
        portable,
        data_dir: st.data_dir.to_string_lossy().to_string(),
        drive_removable: drive_is_removable(&st.data_dir),
        manifest_exists,
    })
}

// ---------- 打包 ----------

fn flush_wal(st: &AppState) -> CmdResult<()> {
    st.with_conn(|c| {
        c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| AppError::db(format!("WAL checkpoint 失败 / checkpoint failed: {e}")))
    })
}

/// 打包核心（emit 为 None 时不发事件，供测试使用）。
fn pack_impl(
    st: &AppState,
    emit: Option<&dyn Fn(PackProgress)>,
    target: &Path,
) -> CmdResult<String> {
    let send = |phase: &str, done: usize, total: usize, current: Option<String>| {
        if let Some(f) = emit {
            f(PackProgress {
                phase: phase.into(),
                done,
                total,
                current,
            });
        }
    };

    // ---- 校验目标 ----
    if target.exists() {
        let empty = fs::read_dir(target)
            .map(|mut rd| rd.next().is_none())
            .unwrap_or(false);
        if !empty {
            return Err(AppError::validation(
                "目标文件夹必须为空（避免覆盖已有数据）/ Target folder must be empty",
            ));
        }
    } else {
        fs::create_dir_all(target).map_err(|e| {
            AppError::io(format!("无法创建目标文件夹 / Cannot create target: {e}"))
        })?;
    }
    if contains(target, &st.data_dir) || contains(&st.data_dir, target) {
        return Err(AppError::validation(
            "目标文件夹不能在数据目录内部（或包含数据目录）/ Target must not overlap the data directory",
        ));
    }

    // ---- 冲刷 WAL，确保 db 文件一致（conn 未开则先打开，幂等） ----
    st.open_database()?;
    flush_wal(st)?;

    // ---- 收集数据目录全部文件 ----
    send("collect", 0, 0, None);
    let files = collect_files(&st.data_dir);
    let total = files.len();
    send("copy", 0, total, None);

    let data_root = target.join("data");
    fs::create_dir_all(&data_root).map_err(|e| AppError::io(e.to_string()))?;

    let mut entries: Vec<ManifestEntry> = Vec::new();
    for (i, (src, rel)) in files.iter().enumerate() {
        let dest = data_root.join(rel.replace('/', "\\"));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(e.to_string()))?;
        }
        fs::copy(src, &dest)
            .map_err(|e| AppError::io(format!("复制失败 / Copy failed {}: {e}", src.display())))?;
        // 校验"实际写入的内容"（哈希目的地）
        let (hash, size) = sha256_file(&dest)?;
        entries.push(ManifestEntry {
            path: format!("data/{rel}"),
            size,
            sha256: hash,
        });
        if i % 16 == 0 || i + 1 == total {
            send("copy", i + 1, total, Some(rel.clone()));
        }
    }

    // ---- 便携标记 ----
    fs::write(target.join(MARKER_FILE), b"")
        .map_err(|e| AppError::io(format!("写入标记失败 / Marker failed: {e}")))?;

    // ---- 复制当前程序本体（任意 Windows 电脑秒开）----
    send("exe", 0, 1, None);
    let exe = std::env::current_exe()
        .map_err(|e| AppError::io(format!("无法定位程序本体 / Cannot locate exe: {e}")))?;
    let dest_exe = target.join(EXE_NAME);
    fs::copy(&exe, &dest_exe)
        .map_err(|e| AppError::io(format!("复制程序失败 / Copy exe failed: {e}")))?;
    let (hash, size) = sha256_file(&dest_exe)?;
    entries.push(ManifestEntry {
        path: EXE_NAME.to_string(),
        size,
        sha256: hash,
    });
    send("exe", 1, 1, Some(EXE_NAME.to_string()));

    // ---- 写 manifest ----
    send("manifest", 0, 1, None);
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let manifest = Manifest {
        generated_at: now_ms(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        files: entries,
    };
    let bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| AppError::io(format!("序列化 manifest 失败 / Serialize manifest failed: {e}")))?;
    fs::write(target.join(MANIFEST_FILE), bytes)
        .map_err(|e| AppError::io(format!("写入 manifest 失败 / Write manifest failed: {e}")))?;
    send("manifest", 1, 1, Some(MANIFEST_FILE.to_string()));
    send("done", 1, 1, None);

    Ok(target.to_string_lossy().to_string())
}

/// 一键全量打包到目标文件夹（真实进度事件 `usb://progress`）。
#[tauri::command]
pub fn usb_pack(
    st: tauri::State<AppState>,
    app: tauri::AppHandle,
    target: String,
) -> CmdResult<String> {
    let target = PathBuf::from(&target);
    let handle = app.clone();
    pack_impl(&st, Some(&move |p: PackProgress| {
        let _ = tauri::Emitter::emit(&handle, "usb://progress", &p);
    }), &target)
}

// ---------- 校验 ----------

/// 按打包基线校验便携包完整性（用于拷贝到 U 盘后 / 换机后核验）。
#[tauri::command]
pub fn usb_verify(dir: String) -> CmdResult<Vec<FileCheck>> {
    let root = PathBuf::from(&dir);
    let mpath = root.join(MANIFEST_FILE);
    if !mpath.is_file() {
        return Err(AppError::not_found(
            "未找到 manifest.json（不是便携包目录？）/ manifest.json not found (not a portable bundle?)",
        ));
    }
    let bytes = fs::read(&mpath).map_err(|e| AppError::io(e.to_string()))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::io(format!("manifest.json 解析失败 / Parse failed: {e}")))?;

    let mut out = Vec::new();
    for entry in &manifest.files {
        let p = root.join(entry.path.replace('/', "\\"));
        let (ok, actual) = if !p.is_file() {
            (false, "MISSING".to_string())
        } else {
            match sha256_file(&p) {
                Ok((hash, _)) => (hash == entry.sha256, hash),
                Err(_) => (false, "UNREADABLE".to_string()),
            }
        };
        out.push(FileCheck {
            path: entry.path.clone(),
            ok,
            expected: entry.sha256.clone(),
            actual,
            size: entry.size,
        });
    }
    // 异常项排前，便于 UI 直接展示
    out.sort_by_key(|c| if c.ok { 1 } else { 0 });
    Ok(out)
}

// ---------- 拔出保护 ----------

/// 仅便携模式启动：1s 轮询数据卷；存活时周期 PASSIVE checkpoint（减小 WAL，
/// 让意外拔出时的风险面最小）；卷消失 → 发 `usb://removed` 后退出线程。
pub fn spawn_removal_watcher(handle: tauri::AppHandle) {
    use tauri::Manager;
    std::thread::spawn(move || {
        if !crate::state::is_portable() {
            return;
        }
        let Some(st) = handle.try_state::<AppState>() else { return };
        let root = st.data_dir.clone();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if root.exists() {
                // 每 2s 冲一次 WAL（PASSIVE 不阻塞业务），把"意外拔出丢失窗口"压到 1-2 秒
                if st.data_dir.join("db/variable.db").exists() {
                    let _ = st.with_conn(|c| {
                        c.execute_batch("PRAGMA wal_checkpoint(PASSIVE);").map_err(AppError::from)
                    });
                }
                continue;
            }
            let _ = tauri::Emitter::emit(
                &handle,
                "usb://removed",
                serde_json::json!({ "dataDir": root.to_string_lossy() }),
            );
            break;
        }
    });
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_verify_roundtrip_and_corruption_detection() {
        let base = std::env::temp_dir().join(format!("variable-usb-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let st = AppState::bootstrap_dirs_at(base.join("data")).unwrap();

        // 数据目录里放点真实文件
        fs::write(st.data_dir.join("root.txt"), b"hello variable").unwrap();
        fs::create_dir_all(st.data_dir.join("media")).unwrap();
        fs::write(st.data_dir.join("media\\pic.bin"), [0u8, 1, 2, 3, 4, 250]).unwrap();

        let target = base.join("bundle");
        let out = pack_impl(&st, None, &target).unwrap();

        // 产物齐全：标记 + data + manifest + exe
        assert!(target.join(MARKER_FILE).exists());
        assert!(target.join(MANIFEST_FILE).exists());
        assert!(target.join(EXE_NAME).exists());
        assert!(target.join("data/root.txt").exists());
        assert_eq!(out, target.to_string_lossy().to_string());

        // 全量校验通过
        let checks = usb_verify(target.to_string_lossy().to_string()).unwrap();
        assert!(!checks.is_empty());
        assert!(checks.iter().all(|c| c.ok), "所有文件应通过校验");

        // 篡改一个文件 → 校验失败且排前
        fs::write(target.join("data/root.txt"), b"corrupted!").unwrap();
        let checks = usb_verify(target.to_string_lossy().to_string()).unwrap();
        assert!(!checks[0].ok, "被篡改文件应校验失败");
        assert_eq!(checks[0].path, "data/root.txt");
        assert!(checks.iter().filter(|c| c.ok).count() == checks.len() - 1);

        // manifest 缺失 → 明确报错
        fs::remove_file(target.join(MANIFEST_FILE)).unwrap();
        assert!(usb_verify(target.to_string_lossy().to_string()).is_err());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn pack_rejects_non_empty_and_overlapping_target() {
        let base = std::env::temp_dir().join(format!("variable-usb-test2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let st = AppState::bootstrap_dirs_at(base.join("data")).unwrap();
        fs::write(st.data_dir.join("f.txt"), b"x").unwrap();

        // 非空目标
        let target = base.join("t1");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("occupied.txt"), b"n").unwrap();
        assert!(pack_impl(&st, None, &target).is_err());

        // 目标在数据目录内部
        let target2 = st.data_dir.join("inside");
        assert!(pack_impl(&st, None, &target2).is_err());

        // 目标包含数据目录（父目录）
        let target3 = base.clone();
        assert!(pack_impl(&st, None, &target3).is_err());

        let _ = fs::remove_dir_all(&base);
    }
}
