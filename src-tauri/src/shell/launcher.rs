//! L3 shell — 第三方软件启动器（M7）：
//! - 登记任意第三方 Windows 软件（exe/lnk/bat/cmd），登记表仅存本机 <dataDir>/apps.json
//! - 便携性三级：portable(🟢 完全便携，位于数据目录内随 Variable 走) /
//!   standalone(🟡 半便携，自包含单文件但绑定本机路径) / shortcut(🔴 仅快捷方式，安装型)
//! - 启动 = 独立 OS 进程（DETACHED spawn），与 Variable 无 WebView 关系；
//!   预装四软件与之平级，本模块不给任何软件特权
//! - 零网络 / 零遥测：一切数据仅本机读写

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

pub const GRADE_PORTABLE: &str = "portable";
pub const GRADE_STANDALONE: &str = "standalone";
pub const GRADE_SHORTCUT: &str = "shortcut";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ThirdApp {
    pub id: String,
    pub name: String,
    pub path: String,
    /// portable | standalone | shortcut
    pub grade: String,
    pub added_at: u64,
    pub last_launch: Option<u64>,
    /// 批次B：自定义图标（data URL，.ico/.png ≤512KB；None = 默认占位图标）
    #[serde(default)]
    pub icon: Option<String>,
    /// 批次E（规格 5.9.4 深度检测）：.lnk 登记项解析出的目标 exe 绝对路径
    /// （exe/bat/cmd 直接登记时为 None）。运行态匹配与便携化都以此为准。
    #[serde(default)]
    pub target: Option<String>,
}

// ---------- 登记表持久化 ----------

fn registry_path(st: &AppState) -> PathBuf {
    st.data_dir.join("apps.json")
}

fn load_registry(st: &AppState) -> Vec<ThirdApp> {
    let Ok(bytes) = fs::read(registry_path(st)) else {
        return Vec::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// 批次C：登记表快照（appman 运行检测用）。
pub fn registry_snapshot(st: &AppState) -> Vec<ThirdApp> {
    load_registry(st)
}

fn save_registry(st: &AppState, apps: &[ThirdApp]) -> CmdResult<()> {
    let bytes = serde_json::to_vec_pretty(apps)
        .map_err(|e| AppError::io(format!("序列化登记表失败 / Serialize registry failed: {e}")))?;
    fs::write(registry_path(st), bytes)
        .map_err(|e| AppError::io(format!("写入登记表失败 / Write registry failed: {e}")))?;
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let c = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("tp-{nanos:x}-{c:x}")
}

// ---------- 便携性分级 ----------

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// 自动分级（用户可在管理器中覆盖）：
/// - 数据目录内 → 🟢 portable（随 Variable / U 盘整体迁移）
/// - Program Files / Windows 目录 → 🔴 shortcut（安装型，依赖系统注册表等）
/// - 其他位置 → 🟡 standalone（自包含但绑定本机路径）
pub fn detect_grade(path: &Path, data_dir: &Path) -> &'static str {
    let s = norm(path);
    let d = norm(data_dir);
    if !d.is_empty() {
        let inside = s.starts_with(&d)
            && (s.len() == d.len() || s.as_bytes().get(d.len()) == Some(&b'\\'));
        if inside {
            return GRADE_PORTABLE;
        }
    }
    if s.contains("\\program files\\")
        || s.contains("\\program files (x86)\\")
        || s.contains("\\windows\\")
    {
        return GRADE_SHORTCUT;
    }
    GRADE_STANDALONE
}

fn valid_grade(g: &str) -> bool {
    matches!(g, GRADE_PORTABLE | GRADE_STANDALONE | GRADE_SHORTCUT)
}

fn valid_target(p: &Path) -> bool {
    p.is_file()
        && p.extension()
            .map(|e| {
                let e = e.to_string_lossy().to_lowercase();
                matches!(e.as_str(), "exe" | "lnk" | "bat" | "cmd")
            })
            .unwrap_or(false)
}

// ---------- 批次E：.lnk 解析（规格 5.9.1/5.9.4） ----------

/// 解析 .lnk 快捷方式的目标路径（COM IShellLinkW；失败如实返回 None）。
/// 仅本机 COM 调用，零网络。
#[cfg(windows)]
pub fn resolve_lnk(path: &Path) -> Option<PathBuf> {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
        IPersistFile, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::Win32::Storage::FileSystem::GetFullPathNameW;

    let wide: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
    // COM 初始化失败时仍尝试调用（可能已被初始化）
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let need_uninit = hr.is_ok();
    let result = (|| -> Option<PathBuf> {
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            // Load 需要 IPersistFile
            let persist: IPersistFile = link.cast().ok()?;
            persist.Load(PCWSTR(wide.as_ptr()), STGM_READ).ok()?;
            let mut buf = [0u16; 1024];
            let mut find = windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW::default();
            let flags = 0u32;
            link.GetPath(&mut buf, &mut find, flags).ok()?;
            let end = buf.iter().position(|&c| c == 0).unwrap_or(0);
            let raw = String::from_utf16_lossy(&buf[..end]);
            if raw.is_empty() {
                return None;
            }
            // 展开相对路径（GetPath 可能返回相对路径）
            let raw_wide: Vec<u16> = raw.encode_utf16().chain([0]).collect();
            let mut out = [0u16; 1024];
            let n = GetFullPathNameW(PCWSTR(raw_wide.as_ptr()), Some(&mut out), None);
            if n == 0 {
                return Some(PathBuf::from(raw));
            }
            let end = out.iter().position(|&c| c == 0).unwrap_or(n as usize);
            let full = String::from_utf16_lossy(&out[..end]);
            (!full.is_empty()).then(|| PathBuf::from(full))
        }
    })();
    if need_uninit {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(not(windows))]
pub fn resolve_lnk(_path: &Path) -> Option<PathBuf> {
    None
}

// ---------- 命令 ----------

/// 登记第三方软件（同路径重复添加 → 幂等返回已有项）。
#[tauri::command]
pub fn tp_add(
    st: tauri::State<AppState>,
    path: String,
    name: Option<String>,
    grade: Option<String>,
) -> CmdResult<ThirdApp> {
    let p = PathBuf::from(&path);
    if !valid_target(&p) {
        return Err(AppError::validation(
            "目标必须是存在的 .exe / .lnk / .bat / .cmd 文件 / Target must be an existing .exe / .lnk / .bat / .cmd file",
        ));
    }
    let mut apps = load_registry(&st);
    if let Some(existing) = apps.iter().find(|a| norm(Path::new(&a.path)) == norm(&p)) {
        return Ok(existing.clone());
    }
    let display = name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            p.file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "App".into())
        });
    let g = match grade.as_deref() {
        Some(g) if valid_grade(g) => g.to_string(),
        _ => detect_grade(&p, &st.data_dir).to_string(),
    };
    // 批次E（规格 5.9.4）：.lnk 登记时即解析目标 exe（运行检测/便携化以此为准）
    let target = if p.extension().map(|e| e.to_string_lossy().to_lowercase() == "lnk").unwrap_or(false) {
        resolve_lnk(&p).map(|t| t.to_string_lossy().to_string())
    } else {
        None
    };
    let app = ThirdApp {
        id: new_id(),
        name: display,
        path,
        grade: g,
        added_at: now_ms(),
        last_launch: None,
        icon: None,
        target,
    };
    apps.push(app.clone());
    save_registry(&st, &apps)?;
    Ok(app)
}

#[tauri::command]
pub fn tp_list(st: tauri::State<AppState>) -> CmdResult<Vec<ThirdApp>> {
    let mut apps = load_registry(&st);
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(apps)
}

#[tauri::command]
pub fn tp_remove(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    let mut apps = load_registry(&st);
    let before = apps.len();
    apps.retain(|a| a.id != id);
    if apps.len() == before {
        return Err(AppError::not_found(format!("未找到登记项 / Not found: {id}")));
    }
    save_registry(&st, &apps)
}

/// 批次C（规格 5.6.2）：移除登记并彻底删除文件。
/// 护栏：仅数据目录内的文件允许删除（防误删系统软件）；删除失败（如软件
/// 正在运行、文件被锁）则报错并保留登记，用户关闭软件后可重试。
#[tauri::command]
pub fn tp_purge(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    tp_purge_inner(&st, &id)
}

fn tp_purge_inner(st: &AppState, id: &str) -> CmdResult<()> {
    let mut apps = load_registry(st);
    let Some(pos) = apps.iter().position(|a| a.id == id) else {
        return Err(AppError::not_found(format!("未找到登记项 / Not found: {id}")));
    };
    let app = apps[pos].clone();
    let p = PathBuf::from(&app.path);
    if p.is_file() {
        if !norm(&p).starts_with(&norm(&st.data_dir)) {
            return Err(AppError::validation(
                "仅数据目录内的便携软件可彻底删除文件；其余请使用「移除登记」/ Only files inside the data directory can be purged; use \"Unregister\" otherwise",
            ));
        }
        fs::remove_file(&p).map_err(|e| {
            AppError::io(format!(
                "删除文件失败（软件可能正在运行）/ Delete failed (app may be running): {e}"
            ))
        })?;
        // 顺带清理删空的直接父目录（仍限数据目录内，绝不触碰数据目录本身）
        if let Some(parent) = p.parent() {
            if parent != st.data_dir && norm(parent).starts_with(&norm(&st.data_dir)) {
                let _ = fs::remove_dir(parent);
            }
        }
    }
    apps.remove(pos);
    save_registry(st, &apps)
}

/// 修改便携性分级（用户覆盖自动判定）。
#[tauri::command]
pub fn tp_set_grade(st: tauri::State<AppState>, id: String, grade: String) -> CmdResult<ThirdApp> {
    if !valid_grade(&grade) {
        return Err(AppError::validation(format!("无效分级 / Invalid grade: {grade}")));
    }
    let mut apps = load_registry(&st);
    let app = apps
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到登记项 / Not found: {id}")))?;
    app.grade = grade;
    let out = app.clone();
    save_registry(&st, &apps)?;
    Ok(out)
}

#[tauri::command]
pub fn tp_rename(st: tauri::State<AppState>, id: String, name: String) -> CmdResult<ThirdApp> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation("名称不能为空 / Name cannot be empty"));
    }
    let mut apps = load_registry(&st);
    let app = apps
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到登记项 / Not found: {id}")))?;
    app.name = trimmed.to_string();
    let out = app.clone();
    save_registry(&st, &apps)?;
    Ok(out)
}

/// 启动：独立 OS 进程（DETACHED），更新 last_launch。
/// 批次0：桌面窗口默认置顶覆盖（规格 10.1），启动第三方软件时暂时撤销置顶，
/// 让其窗口浮于桌面之上（规格 10.3）；用户回到桌面时由 on_window_event 自动恢复。
#[tauri::command]
pub fn tp_launch(
    st: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: String,
) -> CmdResult<()> {
    let mut apps = load_registry(&st);
    let app_item = apps
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到登记项 / Not found: {id}")))?
        .clone();
    let p = PathBuf::from(&app_item.path);
    if !p.is_file() {
        return Err(AppError::not_found(
            "目标文件不存在，可能已被移动或卸载 / Target missing (moved or uninstalled?)",
        ));
    }
    let mut cmd = if p
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase() == "lnk")
        .unwrap_or(false)
    {
        // .lnk 需经 shell 解析
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", &app_item.path]);
        c
    } else {
        let mut c = std::process::Command::new(&p);
        if let Some(parent) = p.parent() {
            c.current_dir(parent);
        }
        c
    };
    spawn_detached(&mut cmd)
        .map_err(|e| AppError::io(format!("启动失败 / Launch failed: {e}")))?;

    // 撤销桌面置顶，让第三方窗口浮于桌面之上（回到桌面自动恢复，见 lib.rs）
    {
        use tauri::Manager;
        if let Some(desktop) = app.get_webview_window("desktop") {
            let _ = desktop.set_always_on_top(false);
        }
    }

    if let Some(slot) = apps.iter_mut().find(|a| a.id == id) {
        slot.last_launch = Some(now_ms());
    }
    save_registry(&st, &apps)?;
    Ok(())
}

#[cfg(windows)]
fn spawn_detached(cmd: &mut std::process::Command) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(|_| ())
}

#[cfg(not(windows))]
fn spawn_detached(cmd: &mut std::process::Command) -> std::io::Result<()> {
    cmd.spawn().map(|_| ())
}

// ---------- 批次B：自定义图标 / 管理员运行 ----------

const ICON_MAX_BYTES: u64 = 512 * 1024;

/// 手写 base64（RFC 4648 标准 alphabet，含 padding）。零新依赖。
pub(crate) fn b64_encode(data: &[u8]) -> String {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TBL[(n >> 18) as usize & 63] as char);
        out.push(TBL[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TBL[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TBL[n as usize & 63] as char } else { '=' });
    }
    out
}

/// 读取 .ico/.png → data URL（仅本机文件读取，零网络）。
fn encode_icon(path: &str) -> Result<String, AppError> {
    let p = PathBuf::from(path);
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mime = match ext.as_str() {
        "ico" => "image/x-icon",
        "png" => "image/png",
        _ => {
            return Err(AppError::validation(
                "仅支持 .ico / .png 图标文件 / Only .ico / .png icons are supported",
            ))
        }
    };
    let meta = fs::metadata(&p)
        .map_err(|_| AppError::not_found("图标文件不存在 / Icon file not found"))?;
    if !meta.is_file() || meta.len() > ICON_MAX_BYTES {
        return Err(AppError::validation(
            "图标必须是 ≤512KB 的文件 / Icon must be a file ≤512KB",
        ));
    }
    let bytes = fs::read(&p)
        .map_err(|e| AppError::io(format!("读取图标失败 / Read icon failed: {e}")))?;
    Ok(format!("data:{mime};base64,{}", b64_encode(&bytes)))
}

/// 更换第三方软件图标（None/空串 = 恢复默认占位图标）。
#[tauri::command]
pub fn tp_set_icon(
    st: tauri::State<AppState>,
    id: String,
    icon_path: Option<String>,
) -> CmdResult<ThirdApp> {
    set_icon_inner(&st, &id, icon_path)
}

fn set_icon_inner(st: &AppState, id: &str, icon_path: Option<String>) -> CmdResult<ThirdApp> {
    let icon = match icon_path {
        None => None,
        Some(p) if p.trim().is_empty() => None,
        Some(p) => Some(encode_icon(&p)?),
    };
    let mut apps = load_registry(st);
    let app = apps
        .iter_mut()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到登记项 / Not found: {id}")))?;
    app.icon = icon;
    let out = app.clone();
    save_registry(st, &apps)?;
    Ok(out)
}

// ---------- 批次E（规格 5.9.2）：扫描开始菜单快捷方式 ----------

/// 扫描候选（未登记项）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TpScanCandidate {
    /// 快捷方式显示名（去 .lnk）
    pub name: String,
    /// .lnk 路径
    pub lnk: String,
    /// 解析出的目标 exe（已校验存在）
    pub target: String,
}

const SCAN_SKIP_PAT: &[&str] = &["uninstall", "unins", "setup", "更新", "升级", "卸载", "help", "readme", "eula"];

/// 递归收集 .lnk（深度限制，跳过明显非软件项）。
#[cfg(windows)]
fn collect_lnks(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    if depth > 6 || out.len() > 512 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else { return };
    for item in rd.flatten() {
        let p = item.path();
        if p.is_dir() {
            collect_lnks(&p, depth + 1, out);
        } else if p.extension().map(|e| e.to_string_lossy().to_lowercase() == "lnk").unwrap_or(false) {
            out.push(p);
        }
    }
}

/// 扫描开始菜单（系统 + 用户两处）里的软件快捷方式，解析出目标 exe。
/// 已登记的项（按目标路径去重）不返回，前端无需再过滤。
#[tauri::command]
pub fn tp_scan_start_menu(st: tauri::State<AppState>) -> CmdResult<Vec<TpScanCandidate>> {
    let mut lnk_dirs = Vec::new();
    if let Ok(pd) = std::env::var("ProgramData") {
        lnk_dirs.push(PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(ad) = std::env::var("APPDATA") {
        lnk_dirs.push(PathBuf::from(ad).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    let mut lnks = Vec::new();
    for d in &lnk_dirs {
        if d.is_dir() {
            collect_lnks(d, 0, &mut lnks);
        }
    }
    // 已登记目标（去重用）
    let registered: Vec<String> = load_registry(&st)
        .iter()
        .filter_map(|a| a.target.clone().or_else(|| Some(a.path.clone())))
        .map(|p| norm(Path::new(&p)))
        .collect();

    let mut seen: Vec<String> = registered;
    let mut out = Vec::new();
    for lnk in lnks {
        let name = lnk
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let low = name.to_lowercase();
        if SCAN_SKIP_PAT.iter().any(|k| low.contains(k)) {
            continue;
        }
        let Some(target) = resolve_lnk(&lnk) else { continue };
        if !target.is_file() {
            continue;
        }
        if !target
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase() == "exe")
            .unwrap_or(false)
        {
            continue;
        }
        // 目标在 Windows 目录 → 多为帮助/运行库，跳过
        if norm(&target).contains("\\windows\\") {
            continue;
        }
        let key = norm(&target);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(TpScanCandidate {
            name,
            lnk: lnk.to_string_lossy().to_string(),
            target: target.to_string_lossy().to_string(),
        });
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(out)
}

// ---------- 批次E（规格 5.9.3）：便携化 ----------

/// 把已登记的 standalone/shortcut 软件整目录复制进数据目录（Apps/<目录名>），
/// 登记项转为 🟢 portable 并指向副本 exe。复制失败如实报错、登记不变。
#[tauri::command]
pub fn tp_portableize(st: tauri::State<AppState>, id: String) -> CmdResult<ThirdApp> {
    portableize_inner(&st, &id)
}

fn portableize_inner(st: &AppState, id: &str) -> CmdResult<ThirdApp> {
    let mut apps = load_registry(st);
    let Some(pos) = apps.iter().position(|a| a.id == id) else {
        return Err(AppError::not_found(format!("未找到登记项 / Not found: {id}")));
    };
    let app = apps[pos].clone();
    if app.grade == GRADE_PORTABLE {
        return Err(AppError::validation(
            "已是便携软件 / Already portable",
        ));
    }
    // 以解析目标为准（.lnk → target；exe → 自身）
    let exe = PathBuf::from(app.target.clone().unwrap_or_else(|| app.path.clone()));
    if !exe.is_file() {
        return Err(AppError::not_found(
            "目标 exe 不存在（快捷方式失效？）/ Target exe missing",
        ));
    }
    let Some(src_dir) = exe.parent().map(Path::to_path_buf) else {
        return Err(AppError::validation("无法确定安装目录 / Cannot determine install dir"));
    };
    // 护栏：已在数据目录内 → 本来就是 portable；数据目录本身绝不能是复制源
    let src_n = norm(&src_dir);
    let data_n = norm(&st.data_dir);
    if !data_n.is_empty() && (src_n == data_n || src_n.starts_with(&format!("{data_n}\\"))) {
        return Err(AppError::validation(
            "目标已在数据目录内 / Target is already inside the data directory",
        ));
    }
    // 复制整个安装目录（ portable 判定按目录自包含；跨盘/锁定文件如实报错）
    let dir_name = src_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "App".into());
    let apps_root = st.data_dir.join("Apps");
    fs::create_dir_all(&apps_root)?;
    let mut dest = apps_root.join(&dir_name);
    let mut i = 0u32;
    while dest.exists() {
        i += 1;
        dest = apps_root.join(format!("{dir_name}-{i}"));
    }
    crate::shell::recycle::copy_recursive_pub(&src_dir, &dest)
        .map_err(|e| AppError::io(format!("复制安装目录失败（软件可能正在运行）/ Copy failed: {e}")))?;
    let new_exe = dest.join(exe.file_name().unwrap_or_default());
    if !new_exe.is_file() {
        let _ = fs::remove_dir_all(&dest);
        return Err(AppError::io("复制后未找到 exe / Copy incomplete"));
    }
    let slot = &mut apps[pos];
    slot.path = new_exe.to_string_lossy().to_string();
    slot.grade = GRADE_PORTABLE.into();
    slot.target = None;
    let out = slot.clone();
    save_registry(st, &apps)?;
    Ok(out)
}

/// 读取图标为 data URL（前端用：文件架等 UI 层图标；不落盘）。
#[tauri::command]
pub fn icon_dataurl(path: String) -> CmdResult<String> {
    encode_icon(&path)
}

/// 以管理员身份运行（Windows：PowerShell Start-Process -Verb RunAs，弹 UAC）。
/// .lnk 不支持 RunAs（诚实报错）；用户取消 UAC → 报错提示。
#[tauri::command]
pub fn tp_launch_admin(st: tauri::State<AppState>, id: String) -> CmdResult<()> {
    let apps = load_registry(&st);
    let app_item = apps
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到登记项 / Not found: {id}")))?;
    let p = PathBuf::from(&app_item.path);
    if !p.is_file() {
        return Err(AppError::not_found(
            "目标文件不存在，可能已被移动或卸载 / Target missing (moved or uninstalled?)",
        ));
    }
    if p.extension()
        .map(|e| e.to_string_lossy().to_lowercase() == "lnk")
        .unwrap_or(false)
    {
        return Err(AppError::validation(
            "快捷方式不支持管理员运行，请选择其目标 exe / Shortcuts cannot run elevated; pick the target .exe",
        ));
    }
    #[cfg(windows)]
    {
        let mut cmd = std::process::Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &format!("Start-Process -FilePath '{}' -Verb RunAs", app_item.path.replace('\'', "''")),
        ]);
        cmd.spawn()
            .map_err(|e| AppError::io(format!("启动失败 / Launch failed: {e}")))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = &app_item;
        Err(AppError::validation(
            "当前平台不支持管理员运行 / Elevated launch unsupported on this platform",
        ))
    }
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_detection_three_tiers() {
        let data = Path::new("C:\\Users\\t\\AppData\\Roaming\\com.variable.app");
        // 🟢 数据目录内（含 Apps 子目录）
        assert_eq!(
            detect_grade(&data.join("Apps\\Foo\\foo.exe"), data),
            GRADE_PORTABLE
        );
        assert_eq!(detect_grade(&data.join("tool.exe"), data), GRADE_PORTABLE);
        // 数据目录同级目录不算内部
        assert_eq!(
            detect_grade(&Path::new("C:\\Users\\t\\AppData\\Roaming\\com.variable.app2\\a.exe"), data),
            GRADE_STANDALONE
        );
        // 🔴 安装目录
        assert_eq!(
            detect_grade(Path::new("C:\\Program Files\\SomeApp\\app.exe"), data),
            GRADE_SHORTCUT
        );
        assert_eq!(
            detect_grade(Path::new("C:\\Program Files (x86)\\Legacy\\l.exe"), data),
            GRADE_SHORTCUT
        );
        // 🟡 其他位置
        assert_eq!(
            detect_grade(Path::new("D:\\Tools\\myapp.exe"), data),
            GRADE_STANDALONE
        );
    }

    #[test]
    fn registry_roundtrip_and_idempotent_add() {
        let tmp = std::env::temp_dir().join(format!("variable-launcher-test-{}", std::process::id()));
        // 造一个真实存在的假 exe（is_file 校验需要）
        let exe = tmp.join("demo.exe");
        fs::create_dir_all(&tmp).unwrap();
        fs::write(&exe, b"MZ").unwrap();

        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        let mut apps = load_registry(&st);
        assert!(apps.is_empty());

        let app = ThirdApp {
            id: new_id(),
            name: "Demo".into(),
            path: exe.to_string_lossy().to_string(),
            grade: detect_grade(&exe, &st.data_dir).to_string(),
            added_at: now_ms(),
            last_launch: None,
            icon: None,
            target: None,
        };
        apps.push(app.clone());
        save_registry(&st, &apps).unwrap();

        let loaded = load_registry(&st);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, app.id);
        assert_eq!(loaded[0].grade, GRADE_PORTABLE); // 在数据目录内 → 🟢
        assert!(loaded[0].last_launch.is_none());

        // 清理
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn b64_encode_rfc4648_vectors() {
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"f"), "Zg==");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
        assert_eq!(b64_encode(b"foo"), "Zm9v");
        assert_eq!(b64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(b64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn tp_purge_guards_and_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("variable-purge-test-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();

        // 🟢 数据目录内 → 允许彻底删除
        let inner_exe = st.data_dir.join("Apps").join("portable.exe");
        fs::create_dir_all(inner_exe.parent().unwrap()).unwrap();
        fs::write(&inner_exe, b"MZ").unwrap();
        let mut apps = load_registry(&st);
        apps.push(ThirdApp {
            id: "p1".into(),
            name: "Portable".into(),
            path: inner_exe.to_string_lossy().to_string(),
            grade: GRADE_PORTABLE.into(),
            added_at: now_ms(),
            last_launch: None,
            icon: None,
            target: None,
        });
        save_registry(&st, &apps).unwrap();
        tp_purge_inner(&st, "p1").unwrap();
        assert!(!inner_exe.exists(), "文件应被删除");
        assert!(load_registry(&st).is_empty(), "登记应被移除");
        // 删空的 Apps 父目录被顺带清理
        assert!(!inner_exe.parent().unwrap().exists(), "空父目录应被清理");
        assert!(st.data_dir.exists(), "数据目录本身绝不能被触碰");

        // 🟡 数据目录外 → 拒绝删除文件
        let outer = std::env::temp_dir().join("variable-purge-outer.exe");
        fs::write(&outer, b"MZ").unwrap();
        let mut apps = load_registry(&st);
        apps.push(ThirdApp {
            id: "p2".into(),
            name: "Outer".into(),
            path: outer.to_string_lossy().to_string(),
            grade: GRADE_STANDALONE.into(),
            added_at: now_ms(),
            last_launch: None,
            icon: None,
            target: None,
        });
        save_registry(&st, &apps).unwrap();
        assert!(tp_purge_inner(&st, "p2").is_err(), "数据目录外应拒绝");
        assert!(outer.exists(), "外部文件不应被删除");
        assert!(load_registry(&st).len() == 1, "失败时登记应保留");

        // 未知 id → not_found
        assert!(tp_purge_inner(&st, "nope").is_err());

        let _ = fs::remove_dir_all(&tmp);
        let _ = fs::remove_file(&outer);
    }

    #[test]
    fn set_icon_roundtrip_and_clear() {
        let tmp = std::env::temp_dir().join(format!("variable-icon-test-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let exe = tmp.join("demo.exe");
        fs::write(&exe, b"MZ").unwrap();
        // 最小 PNG 头（1x1 灰度 PNG 文件签名 + 数据不重要，仅验证编码链路）
        let png = tmp.join("icon.png");
        let png_bytes: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x01, 0x02, 0x03,
        ];
        fs::write(&png, png_bytes).unwrap();

        let st = AppState::bootstrap_dirs_at(tmp.clone()).unwrap();
        let id = {
            let mut apps = load_registry(&st);
            let app = ThirdApp {
                id: new_id(),
                name: "Demo".into(),
                path: exe.to_string_lossy().to_string(),
                grade: GRADE_PORTABLE.into(),
                added_at: now_ms(),
                last_launch: None,
                icon: None,
            target: None,
            };
            apps.push(app.clone());
            save_registry(&st, &apps).unwrap();
            app.id
        };

        // 设置 → data URL 正确
        let out = set_icon_inner(&st, &id, Some(png.to_string_lossy().to_string())).unwrap();
        assert!(out.icon.unwrap_or_default().starts_with("data:image/png;base64,"));

        // 非法扩展 → 校验错误
        let bad = tmp.join("icon.gif");
        fs::write(&bad, b"GIF").unwrap();
        assert!(set_icon_inner(&st, &id, Some(bad.to_string_lossy().to_string())).is_err());

        // 清除（空串 → None）
        let out = set_icon_inner(&st, &id, Some(String::new())).unwrap();
        assert!(out.icon.is_none());

        let _ = fs::remove_dir_all(&tmp);
    }
}
