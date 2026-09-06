//! 应用生态 2.0（B-27，M7；BLUEPRINT 3.5 / 3.5a）。
//!
//! - 可移植性评估向导：绿/黄/红评估卡（目录可写性、卸载注册表痕迹、
//!   配置文件形态三类启发式，理由逐条如实列出）；
//! - 搬迁执行器：exe 所在目录整拷贝进容器 `apps/<id>/` + 登记为 ThirdApp
//!   （grade=portable）+ 卸载注册表快照 `portable.reg`（存在则导出）；
//! - 来源全兼容：Steam 库扫描（steamapps/libraryfolders.vdf 解析）与
//!   `steam://rungameid` 协议直通、商店应用 AUMID 启动（shell:AppsFolder）；
//! - 文件关联表 `fileAssociations`：环境内"打开方式"注册 + 查询（无关联时
//!   宿主兜底打开是前端行为：查不到即调 system::open_path）。
//!
//! 如实边界：评估卡是启发式（真正判定要运行时监控），结论只做建议；
//! 反作弊游戏按蓝图默认"独立窗口不嵌入"。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

type CmdResult<T> = Result<T, AppError>;

// ---------- 可移植性评估 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortabilityCard {
    pub verdict: &'static str, // green | yellow | red
    pub reasons: Vec<String>,
    pub exe_size_bytes: u64,
    pub dir_writable: bool,
    pub uninstall_entry: Option<String>,
}

fn has_uninstall_entry(exe_dir: &Path) -> Option<String> {
    // 在卸载注册表里找与目录名匹配的项（Installed 软件的特征）
    let dir_name = exe_dir.file_name()?.to_string_lossy().into_owned();
    for key in [
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
    ] {
        if let Ok(out) = std::process::Command::new("reg")
            .args(["query", key])
            .output()
        {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    let lower = line.to_lowercase();
                    if lower.contains(&dir_name.to_lowercase()) {
                        return Some(line.trim().to_string());
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn assess(exe: &Path) -> CmdResult<PortabilityCard> {
    if !exe.is_file() {
        return Err(AppError::not_found(format!(
            "exe 不存在: {}",
            exe.display()
        )));
    }
    let dir = exe
        .parent()
        .ok_or_else(|| AppError::validation("exe 无父目录"))?;
    let mut reasons = Vec::new();
    let mut score = 0i32;

    // ① 目录可写性（便携的硬指标）
    let probe = dir.join(".var-write-probe");
    let dir_writable = std::fs::write(&probe, b"1").is_ok();
    let _ = std::fs::remove_file(&probe);
    if dir_writable {
        score += 2;
        reasons.push("✅ 程序目录可写（便携式保存配置的典型特征）".into());
    } else {
        score -= 2;
        reasons.push("❌ 程序目录不可写（很可能依赖 Program Files/注册表）".into());
    }

    // ② 卸载注册表痕迹（Installed 软件特征）
    let uninstall_entry = has_uninstall_entry(dir);
    match &uninstall_entry {
        Some(k) => {
            score -= 1;
            reasons.push(format!("⚠ 发现卸载注册表项（安装式软件）: {k}"));
        }
        None => {
            score += 1;
            reasons.push("✅ 无卸载注册表痕迹".into());
        }
    }

    // ③ 配置文件形态：目录内有 .ini/.json/.cfg → 倾向便携
    let has_local_cfg = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .any(|e| {
                    matches!(
                        e.path().extension().and_then(|x| x.to_str()),
                        Some("ini") | Some("cfg") | Some("json")
                    )
                })
        })
        .unwrap_or(false);
    if has_local_cfg {
        score += 1;
        reasons.push("✅ 目录内发现本地配置文件（.ini/.cfg/.json）".into());
    } else {
        reasons.push("— 目录内未发现本地配置文件（配置可能在 %APPDATA%）".into());
    }

    let verdict = if score >= 2 {
        "green"
    } else if score >= 0 {
        "yellow"
    } else {
        "red"
    };
    Ok(PortabilityCard {
        verdict,
        reasons,
        exe_size_bytes: std::fs::metadata(exe).map(|m| m.len()).unwrap_or(0),
        dir_writable,
        uninstall_entry,
    })
}

#[tauri::command]
pub fn portability_assess(exe: String) -> CmdResult<PortabilityCard> {
    assess(Path::new(&exe))
}

// ---------- 搬迁执行器 ----------

fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_lowercase();
    if s.is_empty() { "app".into() } else { s }
}

fn copy_dir_all(src: &Path, dst: &Path) -> CmdResult<u64> {
    let mut bytes = 0u64;
    std::fs::create_dir_all(dst).map_err(|e| AppError::io(e.to_string()))?;
    for e in std::fs::read_dir(src).map_err(|e| AppError::io(e.to_string()))? {
        let e = e.map_err(|e| AppError::io(e.to_string()))?;
        let p = e.path();
        let d = dst.join(e.file_name());
        if p.is_dir() {
            bytes += copy_dir_all(&p, &d)?;
        } else {
            bytes += std::fs::copy(&p, &d).map_err(|e| AppError::io(e.to_string()))?;
        }
    }
    Ok(bytes)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrateReport {
    pub app_id: String,
    pub dest_exe: String,
    pub bytes_copied: u64,
    pub portable_reg: Option<String>,
    pub registered: bool,
}

/// 搬迁执行器：目录整拷 → apps/<id>/ + 卸载注册表快照 + ThirdApp 登记。
#[tauri::command]
pub fn ecosystem_migrate(
    st: tauri::State<AppState>,
    exe: String,
    name: String,
) -> CmdResult<MigrateReport> {
    let src_exe = PathBuf::from(&exe);
    let src_dir = src_exe
        .parent()
        .ok_or_else(|| AppError::validation("exe 无父目录"))?
        .to_path_buf();
    let app_id = format!("{}-{}", slug(&name), now_short());
    let dest_dir = st.data_dir.join("apps").join(&app_id);
    let bytes = copy_dir_all(&src_dir, &dest_dir)?;
    let dest_exe = dest_dir.join(src_exe.file_name().unwrap_or_default());

    // portable.reg 快照：在卸载注册表里按目录名找匹配键并 reg export
    let mut portable_reg = None;
    if let Some(key) = has_uninstall_entry(&src_dir) {
        let key_path = key.split_whitespace().next().unwrap_or("").to_string();
        let out_path = dest_dir.join("portable.reg");
        if let Ok(out) = std::process::Command::new("reg")
            .args(["export", &key_path])
            .arg(&out_path)
            .args(["/y"])
            .output()
        {
            if out.status.success() {
                portable_reg = Some(out_path.to_string_lossy().into_owned());
            }
        }
    }

    // 登记（迁移后的容器内 exe，grade=portable）
    let apps = crate::shell::launcher::load_registry(&st);
    let registered = !apps.iter().any(|a| a.id == app_id);
    let mut all = apps;
    all.push(crate::shell::launcher::ThirdApp {
        id: app_id.clone(),
        name,
        path: dest_exe.to_string_lossy().into_owned(),
        grade: "portable".into(),
        added_at: now_short(),
        last_launch: None,
        icon: None,
        target: None,
        profile: Default::default(),
    });
    crate::shell::launcher::save_registry(&st, &all)?;

    Ok(MigrateReport {
        app_id,
        dest_exe: dest_exe.to_string_lossy().into_owned(),
        bytes_copied: bytes,
        portable_reg,
        registered,
    })
}

fn now_short() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64 % 100_000_000)
        .unwrap_or(0)
}

// ---------- Steam 库扫描与直通 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGame {
    pub app_id: String,
    pub name: String,
}

/// 解析 Steam libraryfolders.vdf 中的库路径（极简 VDF：只取 "path" 值）。
pub fn parse_library_paths(vdf: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in vdf.lines() {
        let t = line.trim();
        // 行形如 "path"  "D:\SteamLibrary" 或 "1"  "E:\Games\Steam"：
        // 取最后一个 tab 字段为值，值含盘符（第 2 字符为 ':'）即视为库路径。
        if !t.contains('\t') {
            continue;
        }
        let value = t.rsplit('\t').next().unwrap_or("").trim().trim_matches('"');
        if value.len() > 2 && value.as_bytes()[1] == b':' {
            out.push(PathBuf::from(value.replace("\\\\", "\\")));
        }
    }
    out
}

fn steam_root() -> Option<PathBuf> {
    let out = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains("SteamPath"))?;
    let p = line.rsplit("REG_SZ").next()?.trim().replace('/', "\\");
    Some(PathBuf::from(p))
}

/// Steam 库扫描：库目录 + appmanifest_*.acf 解析（名称 + appid）。
#[tauri::command]
pub fn steam_library_scan() -> CmdResult<Vec<SteamGame>> {
    let Some(root) = steam_root() else {
        return Err(AppError::not_found("未找到 Steam（HKCU SteamPath）"));
    };
    let mut libs = vec![root.clone()];
    let vdf = root.join("steamapps").join("libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(&vdf) {
        libs.extend(parse_library_paths(&text));
    }
    let mut games = Vec::new();
    for lib in libs {
        let dir = lib.join("steamapps");
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("appmanifest_") && name.ends_with(".acf") {
                if let Ok(acf) = std::fs::read_to_string(e.path()) {
                    let app_id = acf
                        .lines()
                        .find(|l| l.trim().starts_with("\"appid\""))
                        .and_then(|l| l.split('"').nth(3))
                        .unwrap_or("")
                        .to_string();
                    let game_name = acf
                        .lines()
                        .find(|l| l.trim().starts_with("\"name\""))
                        .and_then(|l| l.split('"').nth(3))
                        .unwrap_or("")
                        .to_string();
                    if !app_id.is_empty() {
                        games.push(SteamGame { app_id, name: game_name });
                    }
                }
            }
        }
    }
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(games)
}

/// steam:// 协议直通（rungameid / store 前台由 Steam 自管）。
#[tauri::command]
pub fn steam_launch(app_id: String) -> CmdResult<()> {
    let url = format!("steam://rungameid/{app_id}");
    std::process::Command::new("cmd")
        .args(["/C", "start", "", &url])
        .spawn()
        .map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

/// 商店应用 AUMID 启动（shell:AppsFolder 直通）。
#[tauri::command]
pub fn aumid_launch(aumid: String) -> CmdResult<()> {
    std::process::Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{aumid}"))
        .spawn()
        .map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

// ---------- 文件关联表（环境内"打开方式" + 宿主兜底） ----------

fn assoc_path(st: &AppState) -> PathBuf {
    st.data_dir.join("fileAssociations.json")
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileAssoc {
    pub ext: String,
    pub app_id: String,
    pub app_name: String,
}

fn load_assocs(st: &AppState) -> Vec<FileAssoc> {
    std::fs::read(assoc_path(st))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn file_assoc_list(st: tauri::State<AppState>) -> CmdResult<Vec<FileAssoc>> {
    Ok(load_assocs(&st))
}

#[tauri::command]
pub fn file_assoc_set(st: tauri::State<AppState>, ext: String, app_id: String, app_name: String) -> CmdResult<()> {
    let st = &st;
    let ext = ext.trim_start_matches('.').to_lowercase();
    if ext.is_empty() {
        return Err(AppError::validation("扩展名为空"));
    }
    let mut list = load_assocs(&st);
    list.retain(|a| a.ext != ext);
    list.push(FileAssoc { ext, app_id, app_name });
    let bytes = serde_json::to_vec_pretty(&list).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(assoc_path(st), bytes).map_err(|e| AppError::io(e.to_string()))
}

/// 解析：返回环境内登记的处理方；None = 宿主兜底打开（前端调 open_path）。
#[tauri::command]
pub fn file_assoc_resolve(st: tauri::State<AppState>, ext: String) -> CmdResult<Option<FileAssoc>> {
    let ext = ext.trim_start_matches('.').to_lowercase();
    Ok(load_assocs(&st).into_iter().find(|a| a.ext == ext))
}

#[tauri::command]
pub fn file_assoc_remove(st: tauri::State<AppState>, ext: String) -> CmdResult<()> {
    let st = &st;
    let ext = ext.trim_start_matches('.').to_lowercase();
    let mut list = load_assocs(&st);
    list.retain(|a| a.ext != ext);
    let bytes = serde_json::to_vec_pretty(&list).map_err(|e| AppError::io(e.to_string()))?;
    std::fs::write(assoc_path(st), bytes).map_err(|e| AppError::io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assess_rewards_writable_dir_with_local_config() {
        let dir = std::env::temp_dir().join(format!("eco-a-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.exe"), b"MZ fake").unwrap();
        std::fs::write(dir.join("config.ini"), b"[x]").unwrap();
        let card = assess(&dir.join("app.exe")).unwrap();
        // 临时目录可写、（通常）无同名卸载项、有本地配置 → 绿或至少不为红
        assert!(card.dir_writable);
        assert_ne!(card.verdict, "red");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn assess_missing_exe_is_error() {
        assert!(assess(Path::new("Z:/nope/missing.exe")).is_err());
    }

    #[test]
    fn vdf_parser_extracts_library_paths() {
        let vdf = r#"
"LibraryFolders"
{
    "TimeNextStatsReport" "1690000000"
    "path"	"D:\\SteamLibrary"
    "1"	"E:\\Games\\Steam"
}"#;
        let paths = parse_library_paths(vdf);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("D:\\SteamLibrary"));
        assert_eq!(paths[1], PathBuf::from("E:\\Games\\Steam"));
    }

    #[test]
    fn slug_is_path_safe() {
        assert_eq!(slug("7-Zip 22.01"), "7-zip-22-01");
        assert_eq!(slug(""), "app");
    }
}
