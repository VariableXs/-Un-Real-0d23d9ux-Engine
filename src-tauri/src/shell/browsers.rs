//! 浏览器矩阵（B-18/B-19，BLUEPRINT 3.4 / 7.6 browsers/）。
//!
//! - 安装探测：App Paths 注册表 + 常见安装路径（chrome/edge/brave/vivaldi/firefox）；
//! - 便携模板：Chrome 系 `--user-data-dir`、Firefox `-profile`，全部指向
//!   容器 `browsers/<profile>/`——Cookie/扩展/登录态零落宿主；
//! - Profile 管理器：列表/新建/克隆/改名/删除（可焚毁：覆写后删除）；
//! - 首次导入（B-19）：书签 HTML / 密码 CSV 文件**复制进容器**（绝不读取
//!   宿主浏览器运行数据），书签在导入时解析出可读清单。
//!
//! 浏览器只是"携带数据目录参数的普通第三方软件"——启动一律走执行档通道。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::exec::{expand_placeholders, ExecProfile};
use crate::state::AppState;

/// 本次会话内经 browser_profile_launch 启动的 pid（profile_id → pids）。
/// 任务栏运行态判定 = 登记的 pid 在进程快照中仍存活。
static LAUNCHED: std::sync::Mutex<Option<std::collections::HashMap<String, Vec<u32>>>> =
    std::sync::Mutex::new(None);

fn record_pid(profile_id: &str, pid: u32) {
    let mut g = LAUNCHED.lock().unwrap_or_else(|e| e.into_inner());
    g.get_or_insert_with(Default::default)
        .entry(profile_id.to_string())
        .or_default()
        .push(pid);
}

/// 任务栏运行态（B-19 分组）：每个存活 profile 是独立分组项。
#[tauri::command]
pub fn browser_running(st: tauri::State<AppState>) -> CmdResult<Vec<String>> {
    let g = LAUNCHED.lock().unwrap_or_else(|e| e.into_inner());
    let map = match g.as_ref() {
        Some(m) => m,
        None => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    for (id, pids) in map {
        if pids.iter().any(|p| pid_alive(*p)) {
            out.push(id.clone());
        }
    }
    Ok(out)
}

fn pid_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let _ = CloseHandle(h);
            true
        } else {
            false
        }
    }
}

type CmdResult<T> = Result<T, AppError>;

// ---------- 已知浏览器定义 ----------

struct KnownBrowser {
    id: &'static str,
    name: &'static str,
    exe_names: &'static [&'static str],
    /// chrome 系（--user-data-dir）或 firefox 系（-profile）
    family: Family,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Chrome,
    Firefox,
}

const KNOWN: &[KnownBrowser] = &[
    KnownBrowser { id: "chrome", name: "Google Chrome", exe_names: &["chrome.exe"], family: Family::Chrome },
    KnownBrowser { id: "edge", name: "Microsoft Edge", exe_names: &["msedge.exe"], family: Family::Chrome },
    KnownBrowser { id: "brave", name: "Brave", exe_names: &["brave.exe"], family: Family::Chrome },
    KnownBrowser { id: "vivaldi", name: "Vivaldi", exe_names: &["vivaldi.exe"], family: Family::Chrome },
    KnownBrowser { id: "firefox", name: "Firefox", exe_names: &["firefox.exe"], family: Family::Firefox },
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedBrowser {
    pub id: String,
    pub name: String,
    pub exe: String,
    pub family: Family,
}

/// 探测：App Paths 注册表 → 常见安装路径。纯本机，零网络。
pub fn detect_installed() -> Vec<DetectedBrowser> {
    let mut out = Vec::new();
    for b in KNOWN {
        if let Some(exe) = locate(b.exe_names[0]) {
            out.push(DetectedBrowser {
                id: b.id.to_string(),
                name: b.name.to_string(),
                exe,
                family: b.family,
            });
        }
    }
    out
}

fn locate(exe_name: &str) -> Option<String> {
    // 1) App Paths（HKLM/HKCU/WOW64）
    for key in [
        format!(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe_name}"),
        format!(r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe_name}"),
        format!(r"HKLM\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths\{exe_name}"),
    ] {
        if let Ok(out) = std::process::Command::new("reg")
            .args(["query", &key, "/ve"])
            .output()
        {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                if let Some(line) = text.lines().find(|l| l.contains("REG_SZ")) {
                    if let Some(p) = line.rsplit("    ").next() {
                        let p = p.trim();
                        if !p.is_empty() && Path::new(p).is_file() {
                            return Some(p.to_string());
                        }
                    }
                }
            }
        }
    }
    // 2) 常见安装路径兜底
    for base in [
        std::env::var("ProgramFiles").unwrap_or_default(),
        std::env::var("ProgramFiles(x86)").unwrap_or_default(),
        std::env::var("LocalAppData").unwrap_or_default(),
    ] {
        if base.is_empty() {
            continue;
        }
        for rel in [
            format!(r"\Google\Chrome\Application\{exe_name}"),
            format!(r"\Microsoft\Edge\Application\{exe_name}"),
            format!(r"\BraveSoftware\Brave-Browser\Application\{exe_name}"),
            format!(r"\Vivaldi\Application\{exe_name}"),
            format!(r"\Mozilla Firefox\{exe_name}"),
        ] {
            let p = PathBuf::from(format!("{base}{rel}"));
            if p.is_file() {
                return Some(p.to_string_lossy().into_owned());
            }
        }
    }
    None
}

// ---------- Profile 存储与 CRUD ----------

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProfile {
    pub id: String,
    pub browser_id: String,
    pub name: String,
    pub exe: String,
    pub family: Family,
    /// 容器内数据目录（绝对路径，指向 <data_dir>/browsers/<id>/）
    pub data_dir: String,
    pub created_at: u64,
}

fn profiles_path(st: &AppState) -> PathBuf {
    st.data_dir.join("browsers.json")
}

fn load_profiles(st: &AppState) -> Vec<BrowserProfile> {
    match std::fs::read(profiles_path(st)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_profiles(st: &AppState, profiles: &[BrowserProfile]) -> CmdResult<()> {
    let bytes = serde_json::to_vec_pretty(profiles).map_err(|e| AppError::io(e.to_string()))?;
    crate::fsutil::atomic_write(profiles_path(st), bytes).map_err(|e| AppError::io(e.to_string()))?;
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn slugify(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_lowercase();
    if s.is_empty() { "profile".into() } else { s }
}

#[tauri::command]
pub fn browser_detect() -> CmdResult<Vec<DetectedBrowser>> {
    Ok(detect_installed())
}

#[tauri::command]
pub fn browser_profiles(st: tauri::State<AppState>) -> CmdResult<Vec<BrowserProfile>> {
    browser_profiles_inner(&st)
}

pub fn browser_profiles_inner(st: &AppState) -> CmdResult<Vec<BrowserProfile>> {
    Ok(load_profiles(st))
}

#[tauri::command]
pub fn browser_profile_add(
    st: tauri::State<AppState>,
    browser_id: String,
    exe: String,
    name: String,
) -> CmdResult<BrowserProfile> {
    browser_profile_add_inner(&st, browser_id, exe, name)
}

pub fn browser_profile_add_inner(
    st: &AppState,
    browser_id: String,
    exe: String,
    name: String,
) -> CmdResult<BrowserProfile> {
    let known = KNOWN.iter().find(|b| b.id == browser_id);
    let family = known.map(|b| b.family).unwrap_or(Family::Chrome);
    let mut profiles = load_profiles(&st);
    let id = format!(
        "{}-{}",
        browser_id,
        slugify(&name)
    );
    if profiles.iter().any(|p| p.id == id) {
        return Err(AppError::validation(format!("profile 已存在: {id}")));
    }
    let data_dir = st.data_dir.join("browsers").join(&id);
    std::fs::create_dir_all(&data_dir).map_err(|e| AppError::io(e.to_string()))?;
    let profile = BrowserProfile {
        id: id.clone(),
        browser_id,
        name,
        exe,
        family,
        data_dir: data_dir.to_string_lossy().into_owned(),
        created_at: now_ms(),
    };
    profiles.push(profile.clone());
    save_profiles(&st, &profiles)?;
    Ok(profile)
}

#[tauri::command]
pub fn browser_profile_rename(st: tauri::State<AppState>, id: String, name: String) -> CmdResult<()> {
    browser_profile_rename_inner(&st, id, name)
}

pub fn browser_profile_rename_inner(st: &AppState, id: String, name: String) -> CmdResult<()> {
    let mut profiles = load_profiles(&st);
    let p = profiles
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到 profile: {id}")))?;
    p.name = name;
    save_profiles(&st, &profiles)
}

/// 克隆 = 数据目录整拷贝（含 Cookie/扩展/登录态）。
#[tauri::command]
pub fn browser_profile_clone(
    st: tauri::State<AppState>,
    id: String,
    new_name: String,
) -> CmdResult<BrowserProfile> {
    browser_profile_clone_inner(&st, id, new_name)
}

pub fn browser_profile_clone_inner(
    st: &AppState,
    id: String,
    new_name: String,
) -> CmdResult<BrowserProfile> {
    let profiles = load_profiles(&st);
    let src = profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到 profile: {id}")))?
        .clone();
    let mut all = profiles;
    let new_id = format!("{}-{}-{}", src.browser_id, slugify(&new_name), now_ms() % 100_000);
    let dst_dir = st.data_dir.join("browsers").join(&new_id);
    copy_dir_all(Path::new(&src.data_dir), &dst_dir)?;
    let profile = BrowserProfile {
        id: new_id.clone(),
        browser_id: src.browser_id.clone(),
        name: new_name,
        exe: src.exe.clone(),
        family: src.family,
        data_dir: dst_dir.to_string_lossy().into_owned(),
        created_at: now_ms(),
    };
    all.push(profile.clone());
    save_profiles(&st, &all)?;
    Ok(profile)
}

/// 删除；shred=true 先覆写再删（焚毁，H3 公用机口径）。
#[tauri::command]
pub fn browser_profile_delete(
    st: tauri::State<AppState>,
    id: String,
    shred: bool,
) -> CmdResult<()> {
    browser_profile_delete_inner(&st, id, shred)
}

pub fn browser_profile_delete_inner(st: &AppState, id: String, shred: bool) -> CmdResult<()> {
    let mut profiles = load_profiles(&st);
    let idx = profiles
        .iter()
        .position(|p| p.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到 profile: {id}")))?;
    let p = profiles.remove(idx);
    save_profiles(&st, &profiles)?;
    let dir = PathBuf::from(&p.data_dir);
    if dir.exists() {
        if shred {
            shred_dir(&dir)?;
        }
        std::fs::remove_dir_all(&dir).map_err(|e| AppError::io(e.to_string()))?;
    }
    Ok(())
}

fn shred_dir(dir: &Path) -> CmdResult<()> {
    fn walk(d: &Path) -> CmdResult<Vec<PathBuf>> {
        let mut files = Vec::new();
        for e in std::fs::read_dir(d).map_err(|e| AppError::io(e.to_string()))? {
            let e = e.map_err(|e| AppError::io(e.to_string()))?;
            let p = e.path();
            if p.is_dir() {
                files.extend(walk(&p)?);
            } else {
                files.push(p);
            }
        }
        Ok(files)
    }
    for f in walk(dir)? {
        if let Ok(meta) = std::fs::metadata(&f) {
            let size = meta.len();
            if size > 0 {
                // 单次覆写零块（防简单恢复；标准级擦除属磁盘层职责，如实边界）
                if let Ok(mut fh) = std::fs::OpenOptions::new().write(true).open(&f) {
                    use std::io::{Seek, SeekFrom, Write};
                    let zeros = vec![0u8; 64 * 1024];
                    let mut left = size;
                    let _ = fh.seek(SeekFrom::Start(0));
                    while left > 0 {
                        let n = left.min(zeros.len() as u64) as usize;
                        if fh.write_all(&zeros[..n]).is_err() {
                            break;
                        }
                        left -= n as u64;
                    }
                    let _ = fh.sync_all();
                }
            }
        }
    }
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> CmdResult<()> {
    std::fs::create_dir_all(dst).map_err(|e| AppError::io(e.to_string()))?;
    for e in std::fs::read_dir(src).map_err(|e| AppError::io(e.to_string()))? {
        let e = e.map_err(|e| AppError::io(e.to_string()))?;
        let p = e.path();
        let d = dst.join(e.file_name());
        if p.is_dir() {
            copy_dir_all(&p, &d)?;
        } else {
            std::fs::copy(&p, &d).map_err(|e| AppError::io(e.to_string()))?;
        }
    }
    Ok(())
}

// ---------- 启动（便携模板 = 数据目录参数 + 执行档通道） ----------

/// 便携模板：按 family 生成启动参数（user data 指向容器内 profile 目录）。
pub fn launch_args(family: Family, data_dir: &Path, url: Option<&str>) -> Vec<String> {
    let dd = data_dir.to_string_lossy().into_owned();
    match family {
        Family::Chrome => {
            let mut v = vec![
                format!("--user-data-dir={dd}"),
                "--no-first-run".into(),
                "--no-default-browser-check".into(),
            ];
            if let Some(u) = url {
                v.push(u.to_string());
            }
            v
        }
        Family::Firefox => {
            let mut v = vec!["-profile".into(), dd, "-no-remote".into()];
            if let Some(u) = url {
                v.push(u.to_string());
            }
            v
        }
    }
}

#[tauri::command]
pub fn browser_profile_launch(
    st: tauri::State<AppState>,
    id: String,
    url: Option<String>,
) -> CmdResult<u32> {
    let profiles = load_profiles(&st);
    let p = profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到 profile: {id}")))?;
    let container_root = st.data_dir.clone();
    let mut env_redirect = std::collections::BTreeMap::new();
    // 执行档语义：HOME/USERPROFILE 镜像容器（与 M1 模板库一致）
    env_redirect.insert("HOME".to_string(), "{envhome}".to_string());
    env_redirect.insert("USERPROFILE".to_string(), "{envhome}".to_string());
    let profile = ExecProfile {
        id: id.clone(),
        env_redirect,
        env_set: {
            let mut m = std::collections::BTreeMap::new();
            m.insert("VARIABLE_PROFILE_ID".to_string(), id.clone());
            m
        },
        net_allow: Vec::new(),
        sensitive: false,
    };
    let args = launch_args(p.family, Path::new(&expand_placeholders(&p.data_dir, &container_root)), url.as_deref());
    let pid = crate::exec::spawn_profiled(
        &container_root,
        &profile,
        Path::new(&p.exe),
        &args,
    )
    .map_err(|e| AppError::io(e.to_string()))?
    .ok_or_else(|| AppError::new("SPAWN", "进程启动失败（返回空 pid）"))?;
    record_pid(&id, pid);
    Ok(pid)
}

// ---------- B-19 首次导入：书签 HTML / 密码 CSV（文件复制进容器） ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub bookmarks: usize,
    pub passwords: usize,
    pub copied_files: Vec<String>,
}

/// 导入：把导出文件复制进容器 profile 目录 `imports/`，并解析书签计数。
/// 绝不读取宿主浏览器运行数据（蓝图 3.4 边界文案）。
#[tauri::command]
pub fn browser_import(
    st: tauri::State<AppState>,
    id: String,
    bookmark_html: Option<String>,
    password_csv: Option<String>,
) -> CmdResult<ImportReport> {
    let profiles = load_profiles(&st);
    let p = profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到 profile: {id}")))?;
    let imports_dir = PathBuf::from(&p.data_dir).join("imports");
    std::fs::create_dir_all(&imports_dir).map_err(|e| AppError::io(e.to_string()))?;
    let mut report = ImportReport { bookmarks: 0, passwords: 0, copied_files: Vec::new() };
    if let Some(src) = bookmark_html {
        let html = std::fs::read_to_string(&src).map_err(|e| AppError::io(e.to_string()))?;
        report.bookmarks = count_bookmarks(&html);
        let dst = imports_dir.join("bookmarks.html");
        std::fs::copy(&src, &dst).map_err(|e| AppError::io(e.to_string()))?;
        report.copied_files.push(dst.to_string_lossy().into_owned());
    }
    if let Some(src) = password_csv {
        let bytes = std::fs::read(&src).map_err(|e| AppError::io(e.to_string()))?;
        report.passwords = String::from_utf8_lossy(&bytes).lines().count().saturating_sub(1);
        let dst = imports_dir.join("passwords.csv");
        std::fs::write(&dst, &bytes).map_err(|e| AppError::io(e.to_string()))?;
        report.copied_files.push(dst.to_string_lossy().into_owned());
    }
    Ok(report)
}

/// 极简 Netscape 书签解析：统计 `<A HREF=` 条目（忽略文件夹层级）。
pub fn count_bookmarks(html: &str) -> usize {
    html.to_lowercase().matches("<a href=").count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_args_chrome_vs_firefox() {
        let d = Path::new(r"C:\container\browsers\chrome-work");
        let chrome = launch_args(Family::Chrome, d, Some("https://example.com"));
        assert!(chrome[0].starts_with("--user-data-dir=C:\\container"));
        assert!(chrome.iter().any(|a| a == "--no-first-run"));
        assert!(chrome.last().unwrap().starts_with("https://"));
        let ff = launch_args(Family::Firefox, d, None);
        assert_eq!(ff[0], "-profile");
        assert_eq!(ff[1], d.to_string_lossy());
        assert!(ff.contains(&"-no-remote".to_string()));
    }

    #[test]
    fn slugify_and_uniqueness() {
        assert!(slugify("工作 Profile!").contains("profile"));
        // 非字母数字一律变 '-'（防路径注入）
        assert_eq!(slugify("a/b:c"), "a-b-c");
        assert_eq!(slugify(""), "profile");
    }

    #[test]
    fn count_bookmarks_counts_anchors() {
        let html = r#"<DT><A HREF="https://a">A</A><DT><a href="https://b">B</A>"#;
        assert_eq!(count_bookmarks(html), 2);
        assert_eq!(count_bookmarks("<div>no links</div>"), 0);
    }
}

#[cfg(test)]
mod crud_tests {
    use super::*;
    use crate::state::AppState;

    fn temp_state(tag: &str) -> (AppState, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "browsers-crud-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let st = AppState {
            conn: std::sync::Mutex::new(None),
            data_dir: dir.clone(),
            db_dir: dir.clone(),
            media_dir: dir.clone(),
            attachments_dir: dir.clone(),
            backups_dir: dir.clone(),
            recovery_dir: dir.clone(),
            logs_dir: dir.clone(),
        };
        (st, dir)
    }

    #[test]
    fn profile_add_rename_clone_delete_shred() {
        let (mut st, dir) = temp_state("crud");
        // 新建（CJK 保留：slugify 只拦路径注入字符）
        let p = browser_profile_add_inner(
            &mut st,
            "chrome".into(),
            "C:/fake/chrome.exe".into(),
            "工作".into(),
        )
        .unwrap();
        assert!(p.id.starts_with("chrome-"));
        assert!(PathBuf::from(&p.data_dir).is_dir());
        // 种子数据（登录态模拟）
        std::fs::write(PathBuf::from(&p.data_dir).join("Cookies"), b"cookie-blob").unwrap();
        // 重名拒绝
        assert!(browser_profile_add_inner(
            &mut st,
            "chrome".into(),
            "C:/fake/chrome.exe".into(),
            "工作".into()
        )
        .is_err());
        // 克隆：数据目录整拷贝
        let c = browser_profile_clone_inner(&mut st, p.id.clone(), "副本".into()).unwrap();
        assert!(PathBuf::from(&c.data_dir).join("Cookies").is_file());
        // 改名
        browser_profile_rename_inner(&mut st, p.id.clone(), "工作2".into()).unwrap();
        let list = browser_profiles_inner(&st).unwrap();
        assert_eq!(list.iter().find(|x| x.id == p.id).unwrap().name, "工作2");
        // 焚毁删除
        browser_profile_delete_inner(&mut st, c.id.clone(), true).unwrap();
        assert!(!PathBuf::from(&c.data_dir).exists());
        assert_eq!(browser_profiles_inner(&st).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
