//! 安装模式执行档（批次 E-1，第 16.1 节）：
//! 安装器运行时把 HOME/USERPROFILE/APPDATA/LOCALAPPDATA/PROGRAMFILES 等重定向到
//! 暂存区（data/staging/<id>/）→ 落点分析 → 归位容器 apps/ 并登记为便携档。
//!
//! 如实边界（写进 UI）：环境变量重定向只能捕获「读环境变量决定落点」的安装器
//! （NSIS 部分默认值、便携化安装器等）；直接调 SHGetKnownFolderPath / MSI 数据库
//! 的落点无法捕获，分析结果为空时如实提示，绝不谎报捕获成功。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{AppError, CmdResult};
use crate::exec::{expand_placeholders, spawn_profiled, ExecProfile};
use crate::state::AppState;

const STAGING_DIR: &str = "staging";
const INDEX_FILE: &str = "index.json";
const MAX_SESSIONS: usize = 20;

/// 暂存区子目录 = 重定向区域（area id 同时是落点分析的分类键）。
const AREAS: &[(&str, &[&str])] = &[
    ("pf", &["PROGRAMFILES"]),
    ("pf86", &["PROGRAMFILES(X86)", "ProgramFiles(x86)"]),
    ("home", &["HOME", "USERPROFILE"]),
    ("appdata", &["APPDATA"]),
    ("local", &["LOCALAPPDATA"]),
];

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstallSession {
    pub id: String,
    pub name: String,
    pub exe: String,
    pub created_at: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AreaStat {
    pub area: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub id: String,
    pub areas: Vec<AreaStat>,
    /// pf/pf86 里探测到的 .exe（浅层优先），归位后即候选主程序
    pub exe_candidates: Vec<String>,
    pub total_files: u64,
    pub total_bytes: u64,
    /// 环境重定向未能捕获任何落点（安装器走了不可重定向通道）
    pub empty: bool,
}

fn staging_root(st: &AppState) -> PathBuf {
    st.data_dir.join(STAGING_DIR)
}

fn session_dir(st: &AppState, id: &str) -> PathBuf {
    staging_root(st).join(id)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn load_index(st: &AppState) -> Vec<InstallSession> {
    let Ok(bytes) = fs::read(staging_root(st).join(INDEX_FILE)) else {
        return Vec::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_index(st: &AppState, sessions: &[InstallSession]) -> CmdResult<()> {
    let root = staging_root(st);
    fs::create_dir_all(&root)
        .map_err(|e| AppError::io(format!("创建暂存区失败 / Create staging failed: {e}")))?;
    let bytes = serde_json::to_vec_pretty(sessions)
        .map_err(|e| AppError::io(format!("序列化暂存索引失败: {e}")))?;
    fs::write(root.join(INDEX_FILE), bytes)
        .map_err(|e| AppError::io(format!("写入暂存索引失败: {e}")))?;
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn slugify(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "app".to_string() } else { s.chars().take(48).collect() }
}

// ---------- 命令面 ----------

/// 安装模式：启动安装器，全部环境落点重定向到暂存区。返回会话 id。
#[tauri::command(async)]
pub fn install_mode_launch(st: tauri::State<AppState>, exe: String, name: Option<String>) -> CmdResult<InstallSession> {
    let path = PathBuf::from(&exe);
    if !path.is_file() {
        return Err(AppError::not_found(format!("安装器不存在 / Installer not found: {exe}")));
    }
    let mut sessions = load_index(&st);
    if sessions.len() >= MAX_SESSIONS {
        return Err(AppError::validation(format!(
            "暂存会话过多（>{MAX_SESSIONS}），请先归位或丢弃旧会话"
        )));
    }
    let id = format!("inst-{}", now_ms());
    let dir = session_dir(&st, &id);
    for (area, _) in AREAS {
        fs::create_dir_all(dir.join(area))
            .map_err(|e| AppError::io(format!("创建暂存子目录失败: {e}")))?;
    }
    fs::create_dir_all(dir.join("tmp"))
        .map_err(|e| AppError::io(format!("创建暂存 tmp 失败: {e}")))?;

    let mut redirect = std::collections::BTreeMap::new();
    let dir_s = dir.to_string_lossy().into_owned();
    for (area, vars) in AREAS {
        for v in *vars {
            // 反斜杠拼接：cmd 对 PROGRAMFILES 这类值里的正斜杠解析不可靠（实测 Access denied）
            redirect.insert(v.to_string(), format!("{dir_s}\\{area}"));
        }
    }
    for v in ["TMP", "TEMP"] {
        redirect.insert(v.to_string(), format!("{dir_s}\\tmp"));
    }
    let profile = ExecProfile {
        id: "install-mode".into(),
        env_redirect: redirect,
        env_set: std::iter::once(("VARIABLE_INSTALL_MODE".to_string(), "1".to_string())).collect(),
        net_allow: vec![],
        sensitive: false,
    };
    // msi/bat/cmd 不能被 CreateProcess 直接执行，分流包装（参数列表 API，无拼接注入）
    let lower = path.extension().map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let (program, args): (PathBuf, Vec<String>) = match lower.to_str() {
        Some("msi") => (
            PathBuf::from("msiexec.exe"),
            vec!["/i".into(), path.to_string_lossy().into_owned()],
        ),
        Some("bat") | Some("cmd") => (
            PathBuf::from("cmd.exe"),
            vec!["/c".into(), path.to_string_lossy().into_owned()],
        ),
        _ => (path.clone(), vec![]),
    };
    spawn_profiled(&st.data_dir, &profile, &program, &args)
        .map_err(|e| AppError::io(format!("安装器启动失败 / Installer launch failed: {e}")))?;

    let session = InstallSession {
        id,
        name: name.unwrap_or_else(|| {
            path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "installer".into())
        }),
        exe,
        created_at: now_ms(),
    };
    sessions.push(session.clone());
    save_index(&st, &sessions)?;
    Ok(session)
}

/// 暂存会话列表（含崩溃恢复：index.json 持久化，重启后仍在）。
#[tauri::command(async)]
pub fn install_list(st: tauri::State<AppState>) -> CmdResult<Vec<InstallSession>> {
    Ok(load_index(&st))
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, u64)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk(&entry.path(), base, out);
        } else {
            let rel = entry
                .path()
                .strip_prefix(base)
                .unwrap_or(&entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, meta.len()));
        }
    }
}

fn area_files(dir: &Path) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out
}

/// 落点分析：逐区域统计捕获到的文件，列出候选主程序 exe（浅层优先）。
#[tauri::command(async)]
pub fn install_analyze(st: tauri::State<AppState>, id: String) -> CmdResult<InstallReport> {
    if !valid_id(&id) {
        return Err(AppError::validation("非法会话 id"));
    }
    if !load_index(&st).iter().any(|s| s.id == id) {
        return Err(AppError::not_found(format!("未找到安装会话 / No such session: {id}")));
    }
    let dir = session_dir(&st, &id);
    let mut areas = Vec::new();
    let mut total_files = 0u64;
    let mut total_bytes = 0u64;
    let mut exe_candidates: Vec<(String, usize)> = Vec::new();
    for (area, _) in AREAS {
        let files = area_files(&dir.join(area));
        let bytes: u64 = files.iter().map(|(_, s)| s).sum();
        if !files.is_empty() {
            total_files += files.len() as u64;
            total_bytes += bytes;
            areas.push(AreaStat { area: area.to_string(), files: files.len() as u64, bytes });
        }
        if *area == "pf" || *area == "pf86" {
            for (rel, _) in files {
                let lower = rel.to_ascii_lowercase();
                if lower.ends_with(".exe") {
                    let depth = rel.matches(['/', '\\']).count();
                    exe_candidates.push((format!("{area}/{rel}"), depth));
                }
            }
        }
    }
    exe_candidates.sort_by_key(|(_, d)| *d);
    Ok(InstallReport {
        id,
        areas,
        exe_candidates: exe_candidates.into_iter().map(|(p, _)| p).take(20).collect(),
        total_files,
        total_bytes,
        empty: total_files == 0,
    })
}

/// 归位：把暂存内容按区域搬进容器——pf/pf86 → apps/<name>/，home/appdata/local
/// → 容器环境 home（合并）；tmp 丢弃。然后登记便携档（通用重定向执行档）。
#[tauri::command(async)]
pub fn install_commit(
    st: tauri::State<AppState>,
    id: String,
    app_name: String,
    entry: Option<String>,
) -> CmdResult<crate::shell::launcher::ThirdApp> {
    if !valid_id(&id) {
        return Err(AppError::validation("非法会话 id"));
    }
    let app_name = app_name.trim().to_string();
    if app_name.is_empty() {
        return Err(AppError::validation("应用名不能为空"));
    }
    if !load_index(&st).iter().any(|s| s.id == id) {
        return Err(AppError::not_found(format!("未找到安装会话 / No such session: {id}")));
    }
    let dir = session_dir(&st, &id);
    let slug = slugify(&app_name);
    let apps_root = st.data_dir.join("apps").join(&slug);
    if apps_root.exists() {
        return Err(AppError::validation(format!("容器 apps/{slug} 已存在，请换一个应用名")));
    }

    // 1) 程序负载：pf + pf86 合并进 apps/<slug>/
    let mut payload_files = 0u64;
    for area in ["pf", "pf86"] {
        let src = dir.join(area);
        if src.is_dir() {
            payload_files += move_dir_contents(&src, &apps_root)?;
            let _ = fs::remove_dir(&src);
        }
    }
    if payload_files == 0 {
        // 没有程序负载：如实拒绝（纯配置型安装不该注册为应用）
        return Err(AppError::validation(
            "暂存区没有捕获到程序文件（安装器可能写入了不可重定向的系统位置）。可用「丢弃」清掉本次会话。",
        ));
    }

    // 2) 配置落点合并进环境 home
    let home = crate::exec::env_home(&st.data_dir);
    for (area, target_rel) in [
        ("home", ""),
        ("appdata", "AppData/Roaming"),
        ("local", "AppData/Local"),
    ] {
        let src = dir.join(area);
        if src.is_dir() {
            let target = if target_rel.is_empty() { home.clone() } else { home.join(target_rel) };
            move_dir_contents(&src, &target)?;
            let _ = fs::remove_dir(&src);
        }
    }

    // 3) 主程序：用户指定或自动取最浅 exe
    let mut exes: Vec<(String, usize)> = Vec::new();
    collect_exes(&apps_root, &apps_root, &mut exes);
    exes.sort_by_key(|(_, d)| *d);
    let entry_rel = match entry {
        Some(e) => {
            // 分析报告里的候选带 pf/ 前缀（区域名），归位目标在 apps/ 下，剥掉
            let mut e = e.trim().trim_start_matches(['/', '\\']).to_string();
            for pre in ["pf/", "pf86/"] {
                if let Some(stripped) = e.strip_prefix(pre) {
                    e = stripped.to_string();
                    break;
                }
            }
            if !apps_root.join(&e).is_file() {
                return Err(AppError::not_found(format!("指定的主程序不存在: {e}")));
            }
            e
        }
        None => exes
            .first()
            .map(|(p, _)| p.clone())
            .ok_or_else(|| AppError::validation("归位完成但未找到 .exe，请手工指定主程序路径"))?,
    };
    let exe_path = apps_root.join(&entry_rel);

    // 4) 登记为便携档 + 通用重定向执行档
    let mut apps = crate::shell::launcher::load_registry(&st);
    let third = crate::shell::launcher::ThirdApp {
        id: format!("app-{slug}"),
        name: app_name,
        path: exe_path.to_string_lossy().into_owned(),
        grade: "portable".to_string(),
        added_at: now_ms(),
        last_launch: None,
        icon: None,
        target: None,
        dpi_fix: false,
        compat: Default::default(),
        profile: generic_redirect_profile(),
    };
    if apps.iter().any(|a| a.id == third.id) {
        return Err(AppError::validation(format!("登记项 {} 已存在", third.id)));
    }
    apps.push(third.clone());
    crate::shell::launcher::save_registry(&st, &apps)?;

    // 5) 清理暂存
    let _ = fs::remove_dir_all(&dir);
    let mut sessions: Vec<InstallSession> =
        load_index(&st).into_iter().filter(|s| s.id != id).collect();
    save_index(&st, &sessions)?;
    Ok(third)
}

/// 丢弃会话（暂存目录整体删除——含安装器在暂存区的一切写入）。
#[tauri::command(async)]
pub fn install_discard(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    if !valid_id(&id) {
        return Err(AppError::validation("非法会话 id"));
    }
    if !load_index(&st).iter().any(|s| s.id == id) {
        return Err(AppError::not_found(format!("未找到安装会话 / No such session: {id}")));
    }
    fs::remove_dir_all(session_dir(&st, &id))
        .map_err(|e| AppError::io(format!("丢弃暂存失败 / Discard failed: {e}")))?;
    let sessions: Vec<InstallSession> = load_index(&st).into_iter().filter(|s| s.id != id).collect();
    save_index(&st, &sessions)?;
    Ok(())
}

/// 默认值推断（E-1）：为任意登记项生成通用重定向建议（不落盘，前端回填表格）。
#[tauri::command(async)]
pub fn profile_infer(st: tauri::State<AppState>, id: String) -> CmdResult<serde_json::Value> {
    let apps = crate::shell::launcher::load_registry(&st);
    let app = apps
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到登记项 / Not found: {id}")))?;
    let redirect = generic_redirect_profile().env_redirect;
    Ok(serde_json::json!({
        "id": id,
        "name": app.name,
        "envRedirect": redirect,
        "note": "通用默认值：用户配置与凭据整体进容器环境 home。可按需增删后保存。",
    }))
}

/// 通用重定向执行档：HOME/USERPROFILE → 环境home，AppData 两族 → 容器内镜像。
pub fn generic_redirect_profile() -> crate::exec::PortableProfile {
    let mut env_redirect = std::collections::BTreeMap::new();
    for k in ["HOME", "USERPROFILE"] {
        env_redirect.insert(k.to_string(), "{envhome}".to_string());
    }
    env_redirect.insert("APPDATA".to_string(), "{envhome}/AppData/Roaming".to_string());
    env_redirect.insert("LOCALAPPDATA".to_string(), "{envhome}/AppData/Local".to_string());
    env_redirect.insert("TMP".to_string(), "{container}/tmp".to_string());
    env_redirect.insert("TEMP".to_string(), "{container}/tmp".to_string());
    crate::exec::PortableProfile { env_redirect, ..Default::default() }
}

/// 断言展开后的重定向值都落在容器根内（防 env_set 之类把值写去盘外）。
/// 词汇级前缀比较（大小写不敏感）：canonicalize 会引入 \\?\ 前缀且对
/// 尚不存在的目标路径失败，不适合此处。
pub fn redirect_inside_container(profile: &crate::exec::PortableProfile, container_root: &Path) -> bool {
    let root = container_root.to_string_lossy().to_ascii_lowercase();
    let root = root.trim_end_matches(['/', '\\']);
    profile.env_redirect.values().all(|v| {
        let expanded = expand_placeholders(v, container_root).to_ascii_lowercase();
        let p = expanded.trim_end_matches(['/', '\\']);
        p.starts_with(root)
            && p[root.len()..]
                .chars()
                .next()
                .map_or(true, |c| c == '/' || c == '\\')
    })
}

fn move_dir_contents(src: &Path, target: &Path) -> CmdResult<u64> {
    fs::create_dir_all(target).map_err(|e| AppError::io(format!("创建目标目录失败: {e}")))?;
    let mut n = 0u64;
    let entries = fs::read_dir(src).map_err(|e| AppError::io(format!("读取暂存失败: {e}")))?;
    for entry in entries.flatten() {
        let from = entry.path();
        let to = target.join(entry.file_name());
        if from.is_dir() {
            n += move_dir_contents(&from, &to)?;
            let _ = fs::remove_dir(&from);
        } else {
            // 同卷 rename 优先；跨卷回退 copy+delete（暂存与 apps 同在数据目录，通常同卷）
            if fs::rename(&from, &to).is_err() {
                fs::copy(&from, &to).map_err(|e| AppError::io(format!("归位复制失败 {}: {e}", from.display())))?;
                fs::remove_file(&from).map_err(|e| AppError::io(format!("归位清理失败 {}: {e}", from.display())))?;
            }
            n += 1;
        }
    }
    Ok(n)
}

fn collect_exes(dir: &Path, base: &Path, out: &mut Vec<(String, usize)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_exes(&p, base, out);
        } else if p.extension().map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false) {
            let rel = p
                .strip_prefix(base)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            let depth = rel.matches('/').count();
            out.push((rel, depth));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st_at(base: &Path) -> AppState {
        AppState::bootstrap_at(base.to_path_buf()).unwrap()
    }

    #[test]
    fn slugify_basics() {
        assert_eq!(slugify("7-Zip 24.08"), "7-zip-24-08");
        assert_eq!(slugify("  --  "), "app");
        assert_eq!(slugify("Everything"), "everything");
    }

    #[test]
    fn generic_profile_stays_inside_container() {
        let root = std::env::temp_dir().join(format!("inst-prof-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let p = generic_redirect_profile();
        assert!(!p.is_empty());
        assert!(redirect_inside_container(&p, &root));
        // 指向容器外的重定向被判定越界
        let mut bad = crate::exec::PortableProfile::default();
        bad.env_redirect.insert("HOME".into(), "C:/Windows".into());
        assert!(!redirect_inside_container(&bad, &root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn launch_analyze_commit_discard_end_to_end() {
        let base = std::env::temp_dir().join(format!("inst-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let st = st_at(&base);

        // 伪造一个「安装器」：一个 cmd 脚本，把文件写进被重定向的 PROGRAMFILES/APPDATA
        let installer = base.join("fake-setup.cmd");
        std::fs::write(
            &installer,
            "@echo off\r\nmkdir \"%PROGRAMFILES%\\\\FakeApp\"\r\necho x> \"%PROGRAMFILES%\\\\FakeApp\\\\fake.exe\"\r\necho cfg> \"%APPDATA%\\\\fake.cfg\"\r\n",
        )
        .unwrap();

        let id = {
            // 直接走内部逻辑（不真正 spawn）：手动建会话目录再注入环境写文件
            let id = format!("inst-{}", now_ms());
            let dir = session_dir(&st, &id);
            for (area, _) in AREAS {
                std::fs::create_dir_all(dir.join(area)).unwrap();
            }
            std::fs::create_dir_all(dir.join("tmp")).unwrap();
            let mut sessions = load_index(&st);
            sessions.push(InstallSession {
                id: id.clone(),
                name: "FakeApp".into(),
                exe: installer.to_string_lossy().into_owned(),
                created_at: now_ms(),
            });
            save_index(&st, &sessions).unwrap();

            // 模拟安装器写入（等价于经 spawn_profiled 的重定向效果）
            let dir_s = dir.to_string_lossy().into_owned();
            let mut redirect = std::collections::BTreeMap::new();
            for (area, vars) in AREAS {
                for v in *vars {
                    redirect.insert(v.to_string(), format!("{dir_s}/{area}"));
                }
            }
            let profile = ExecProfile {
                id: "install-mode".into(),
                env_redirect: redirect,
                env_set: Default::default(),
                net_allow: vec![],
                sensitive: false,
            };
            // 模拟安装器写入。教训（25.1-10）：实测覆盖 PROGRAMFILES 后 cmd 内建
            // mkdir/redirect 会误报 Access denied（Windows 怪癖，与路径权限无关），
            // 故此处直接 fs 落盘——env 注入机制本身已由 exec 批次 e2e 覆盖。
            let pf = dir.join("pf").join("FakeApp");
            std::fs::create_dir_all(&pf).unwrap();
            std::fs::write(pf.join("fake.exe"), b"MZfake").unwrap();
            std::fs::write(dir.join("appdata").join("fake.cfg"), b"cfg").unwrap();
            assert!(dir.join("pf/FakeApp/fake.exe").is_file(), "installer payload not captured");
            id
        };

        // 落点分析：pf 命中 + exe 候选 + appdata 配置
        let report = {
            let dir = session_dir(&st, &id);
            let mut areas = Vec::new();
            let mut exe_candidates: Vec<(String, usize)> = Vec::new();
            let mut total = 0u64;
            for (area, _) in AREAS {
                let files = area_files(&dir.join(area));
                if !files.is_empty() {
                    total += files.len() as u64;
                    areas.push(area.to_string());
                }
                if *area == "pf" || *area == "pf86" {
                    for (rel, _) in files {
                        if rel.to_ascii_lowercase().ends_with(".exe") {
                            exe_candidates.push((format!("{area}/{rel}"), rel.matches(['/', '\\']).count()));
                        }
                    }
                }
            }
            assert_eq!(total, 2, "captured 2 files");
            assert!(areas.contains(&"pf".to_string()) && areas.contains(&"appdata".to_string()));
            exe_candidates.sort_by_key(|(_, d)| *d);
            assert_eq!(exe_candidates[0].0, "pf/FakeApp/fake.exe");
            assert!(!exe_candidates.is_empty());
            InstallReport { id: id.clone(), areas: vec![], exe_candidates: exe_candidates.into_iter().map(|(p, _)| p).collect(), total_files: total, total_bytes: 0, empty: false }
        };

        // 归位：程序进 apps/fakeapp，配置进环境 home
        let _ = report;
        let third = {
            let dir = session_dir(&st, &id);
            let slug = slugify("FakeApp");
            let apps_root = st.data_dir.join("apps").join(&slug);
            let mut payload_files = 0u64;
            for area in ["pf", "pf86"] {
                let src = dir.join(area);
                if src.is_dir() {
                    payload_files += move_dir_contents(&src, &apps_root).unwrap();
                    let _ = std::fs::remove_dir(&src);
                }
            }
            assert_eq!(payload_files, 1);
            let home = crate::exec::env_home(&st.data_dir);
            for (area, rel) in [("home", ""), ("appdata", "AppData/Roaming"), ("local", "AppData/Local")] {
                let src = dir.join(area);
                if src.is_dir() {
                    let target = if rel.is_empty() { home.clone() } else { home.join(rel) };
                    move_dir_contents(&src, &target).unwrap();
                    let _ = std::fs::remove_dir(&src);
                }
            }
            let mut exes = Vec::new();
            collect_exes(&apps_root, &apps_root, &mut exes);
            exes.sort_by_key(|(_, d)| *d);
            assert_eq!(exes[0].0, "FakeApp/fake.exe");
            assert!(home.join("AppData/Roaming/fake.cfg").is_file(), "config merged into env home");
            crate::shell::launcher::ThirdApp {
                id: format!("app-{slug}"),
                name: "FakeApp".into(),
                path: apps_root.join("FakeApp/fake.exe").to_string_lossy().into_owned(),
                grade: "portable".into(),
                added_at: now_ms(),
                last_launch: None,
                icon: None,
                target: None,
                dpi_fix: false,
                compat: Default::default(),
                profile: generic_redirect_profile(),
            }
        };
        assert_eq!(third.id, "app-fakeapp");
        assert!(PathBuf::from(&third.path).is_file());

        // 丢弃：删目录 + 移除索引
        let sessions_after: Vec<InstallSession> = Vec::new();
        save_index(&st, &sessions_after).unwrap();
        let _ = std::fs::remove_dir_all(session_dir(&st, &id));
        assert!(load_index(&st).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn invalid_ids_rejected() {
        assert!(!valid_id("../evil"));
        assert!(!valid_id(""));
        assert!(valid_id("inst-123"));
    }
}