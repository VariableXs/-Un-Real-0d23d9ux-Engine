//! L3 shell — explorer.rs（M6）
//! 桌面环境文件管理器后端：浏览整个 Windows 文件系统（只读浏览 + 受控写操作）。
//! - 浏览/盘符：真实 read_dir / 逻辑盘枚举，零网络
//! - 写操作：新建文件夹 / 重命名 / 移动 / 复制 / 删除（删除 → Variable 全局回收站）
//! - 护栏：拒绝操作驱动器根目录与数据目录自身/祖先；名称非法字符清洗；
//!   可执行文件不通过 Variable 打开（复用 system.rs 的黑名单语义）

use crate::error::{AppError, CmdResult};
use crate::state::AppState;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExEntry {
    pub name: String,
    pub path: String,
    /// "dir" | "file"
    pub kind: String,
    pub ext: Option<String>,
    pub size: u64,
    /// ms since epoch
    pub updated_at: u64,
    /// ms since epoch（批次C 规格版 7.4.1：可选"创建时间"列）
    pub created_at: u64,
    /// Windows FILE_ATTRIBUTE_HIDDEN
    pub hidden: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<ExEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExDrive {
    /// "C"
    pub letter: String,
    /// "C:\"
    pub path: String,
}

fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// 批次C（规格 7.8）：Windows 长路径支持 —— 绝对路径加 `\\?\` 前缀突破 260 字符限制。
/// 已有前缀或 UNC（`\\server\…`）原样返回。
fn long_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if s.len() >= 240 && p.is_absolute() && !s.starts_with(r"\\?\") && !s.starts_with(r"\\") {
        PathBuf::from(format!(r"\\?\{s}"))
    } else {
        p.to_path_buf()
    }
}

fn ms_since_epoch(t: std::io::Result<std::time::SystemTime>) -> u64 {
    t.ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(windows)]
fn is_hidden(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x2 != 0 // FILE_ATTRIBUTE_HIDDEN
}

#[cfg(not(windows))]
fn is_hidden(_meta: &fs::Metadata) -> bool {
    false
}

/// 驱动器根 / 顶层路径（parent 为 None）禁止删除、重命名与移出。
fn is_root_like(p: &Path) -> bool {
    p.parent().is_none()
}

/// 名称清洗：去控制字符、替换 Windows 非法字符（不限制扩展名）。
fn sanitize_component(name: &str) -> CmdResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err(AppError::validation("名称无效 / Invalid name"));
    }
    if trimmed.chars().count() > 150 || trimmed.ends_with('.') {
        return Err(AppError::validation("名称无效 / Invalid name"));
    }
    let cleaned: String = trimmed
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();
    if cleaned.trim().is_empty() {
        return Err(AppError::validation("名称无效 / Invalid name"));
    }
    Ok(cleaned)
}

/// 操作目标护栏：必须存在；不得是驱动器根；不得威胁 Variable 数据目录
/// （目标 = 数据目录自身、数据目录祖先，或回收站目录内部）。
fn guard_target(st: &AppState, p: &Path) -> CmdResult<()> {
    if !p.exists() {
        return Err(AppError::not_found(format!("路径不存在 / Path not found: {}", display_path(p))));
    }
    if is_root_like(p) {
        return Err(AppError::validation("不能对驱动器根目录执行此操作 / Cannot operate on a drive root"));
    }
    let data = st.data_dir.canonicalize().unwrap_or_else(|_| st.data_dir.clone());
    let canon = p
        .canonicalize()
        .unwrap_or_else(|_| p.to_path_buf());
    let recycle = st.data_dir.join("recycle");
    if canon == data || data.starts_with(&canon) || canon.starts_with(&recycle) {
        return Err(AppError::validation("不能对 Variable 数据目录执行此操作 / Cannot operate on the Variable data directory"));
    }
    Ok(())
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let ext = Path::new(name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let mut i = 0u32;
    loop {
        let cand = if i == 0 {
            dir.join(format!("{stem}{ext}"))
        } else {
            dir.join(format!("{stem}-{i}{ext}"))
        };
        if !cand.exists() {
            return cand;
        }
        i += 1;
    }
}

fn entry_from_path(p: &Path) -> Option<ExEntry> {
    let lp = long_path(p);
    let meta = fs::metadata(&lp).ok()?;
    let name = p.file_name()?.to_string_lossy().to_string();
    let hidden = is_hidden(&meta);
    let (kind, ext, size) = if meta.is_dir() {
        ("dir", None, 0)
    } else {
        (
            "file",
            p.extension().map(|e| e.to_string_lossy().to_lowercase()),
            meta.len(),
        )
    };
    Some(ExEntry {
        name,
        path: display_path(p),
        kind: kind.into(),
        ext,
        size,
        updated_at: ms_since_epoch(meta.modified()),
        created_at: ms_since_epoch(meta.created()),
        hidden,
    })
}

/// 用户主目录（Windows: %USERPROFILE%）。
#[tauri::command]
pub fn ex_home(_st: tauri::State<AppState>) -> CmdResult<String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| AppError::not_found("无法定位主目录 / Cannot resolve home directory"))?;
    Ok(home)
}

/// 逻辑驱动器枚举（A:–Z: 中真实存在的盘）。
#[tauri::command]
pub fn ex_drives(_st: tauri::State<AppState>) -> CmdResult<Vec<ExDrive>> {
    let mut out = Vec::new();
    for b in b'A'..=b'Z' {
        let letter = (b as char).to_string();
        let root = format!("{letter}:\\");
        if Path::new(&root).is_dir() {
            out.push(ExDrive { letter, path: root });
        }
    }
    Ok(out)
}

/// 批次E（规格 7.2）：Variable 数据目录节点组（文件管理器侧栏「Variable」区）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExVarDir {
    /// 节点键：workspace | apps | recycle | root
    pub key: String,
    pub path: String,
}

/// 返回数据目录根 + 已存在的关键子目录（工作区 / 便携应用 / 回收站）。
#[tauri::command]
pub fn ex_variable_dirs(st: tauri::State<AppState>) -> CmdResult<Vec<ExVarDir>> {
    let mut out = vec![ExVarDir {
        key: "root".into(),
        path: st.data_dir.to_string_lossy().to_string(),
    }];
    for (sub, key) in [("Workspace", "workspace"), ("Apps", "apps"), ("recycle", "recycle")] {
        let p = st.data_dir.join(sub);
        if p.exists() {
            out.push(ExVarDir {
                key: key.into(),
                path: p.to_string_lossy().to_string(),
            });
        }
    }
    Ok(out)
}

/// 列出目录内容（全部文件类型；目录在前、按名称不区分大小写排序）。
#[tauri::command]
pub fn ex_list(_st: tauri::State<AppState>, path: String) -> CmdResult<ExListing> {
    let dir = PathBuf::from(&path);
    let lp = long_path(&dir);
    if !lp.is_dir() {
        return Err(AppError::not_found(format!("目录不存在 / Directory not found: {path}")));
    }
    let mut entries: Vec<ExEntry> = Vec::new();
    let rd = fs::read_dir(&lp).map_err(|e| AppError::io(format!("读取目录失败 / Read dir failed: {e}")))?;
    for item in rd.flatten() {
        if let Some(e) = entry_from_path(&item.path()) {
            entries.push(e);
        }
    }
    entries.sort_by(|a, b| {
        let (da, db) = (a.kind == "dir", b.kind == "dir");
        db.cmp(&da)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let parent = if is_root_like(&dir) {
        None
    } else {
        dir.parent().map(|p| display_path(p))
    };
    Ok(ExListing {
        path: display_path(&dir),
        parent,
        entries,
    })
}

#[tauri::command]
pub fn ex_mkdir(st: tauri::State<AppState>, parent: String, name: String) -> CmdResult<String> {
    let dir = long_path(Path::new(&parent));
    if !dir.is_dir() {
        return Err(AppError::not_found("父文件夹不存在 / Parent folder not found"));
    }
    let clean = sanitize_component(&name)?;
    let dest = unique_path(&dir, &clean);
    fs::create_dir_all(&dest)?;
    let _ = guard_target(&st, &dest); // 目标在数据目录内也允许创建（仅拦截删除/改数据目录祖先）
    Ok(display_path(&dest))
}

#[tauri::command]
pub fn ex_rename(st: tauri::State<AppState>, path: String, new_name: String) -> CmdResult<String> {
    let src = PathBuf::from(&path);
    guard_target(&st, &src)?;
    let clean = sanitize_component(&new_name)?;
    if src.file_name().map(|n| n.to_string_lossy() == clean).unwrap_or(false) {
        return Ok(display_path(&src));
    }
    let parent = src.parent().ok_or_else(|| AppError::validation("无效路径 / Invalid path"))?;
    let dest = unique_path(parent, &clean);
    fs::rename(long_path(&src), long_path(&dest))?;
    Ok(display_path(&dest))
}

fn copy_recursive(src: &Path, dest: &Path, depth: u8) -> CmdResult<u64> {
    if depth > 12 {
        return Err(AppError::validation("目录层级过深 / Directory nesting too deep"));
    }
    let lp_src = long_path(src);
    let meta = fs::symlink_metadata(&lp_src)
        .map_err(|e| AppError::io(format!("读取失败 / Stat failed: {e}")))?;
    if meta.is_dir() {
        fs::create_dir_all(long_path(dest))?;
        let mut total = 0u64;
        for item in fs::read_dir(&lp_src)?.flatten() {
            total += copy_recursive(&item.path(), &dest.join(item.file_name()), depth + 1)?;
        }
        Ok(total)
    } else if meta.is_file() {
        let n = fs::copy(lp_src, long_path(dest))?;
        Ok(n)
    } else {
        Ok(0) // symlink 等特殊项跳过
    }
}

/// 批次C（规格 7.7 冲突）：复制/移动冲突解决模式。
/// - `auto`（默认）：重名自动加后缀（keep both）
/// - `replace`：目标已存在则先移除再写入
/// - `keep`：显式保留两者（与 auto 等价，语义由前端冲突对话框传达）
fn resolve_dest(from: &Path, to_dir: &Path, mode: &str) -> CmdResult<PathBuf> {
    let name = from
        .file_name()
        .ok_or_else(|| AppError::validation("无效路径 / Invalid path"))?
        .to_string_lossy()
        .to_string();
    let dest = to_dir.join(&name);
    if dest.exists() {
        match mode {
            "replace" => {
                let lp = long_path(&dest);
                if dest.is_dir() {
                    fs::remove_dir_all(&lp)?;
                } else {
                    fs::remove_file(&lp)?;
                }
                Ok(dest)
            }
            // auto / keep / 未知值都走防覆盖后缀（后端不做静默覆盖）
            _ => Ok(unique_path(to_dir, &name)),
        }
    } else {
        Ok(dest)
    }
}

/// 复制文件/文件夹到目标目录（跨盘安全；mode = auto|replace|keep）。
#[tauri::command]
pub fn ex_copy(
    st: tauri::State<AppState>,
    src: String,
    dest_dir: String,
    mode: Option<String>,
) -> CmdResult<String> {
    let from = PathBuf::from(&src);
    let to_dir = long_path(Path::new(&dest_dir));
    if !from.exists() {
        return Err(AppError::not_found("源不存在 / Source not found"));
    }
    if !to_dir.is_dir() {
        return Err(AppError::not_found("目标文件夹不存在 / Destination folder not found"));
    }
    let _ = guard_target(&st, &from);
    if from.is_dir() && to_dir.starts_with(&from) {
        return Err(AppError::validation("不能复制到自身内部 / Cannot copy into itself"));
    }
    let dest = resolve_dest(&from, &to_dir, mode.as_deref().unwrap_or("auto"))?;
    copy_recursive(&from, &dest, 0)?;
    Ok(display_path(&dest))
}

/// 移动文件/文件夹（同盘 rename；跨盘 copy + 删除源；mode = auto|replace|keep）。
#[tauri::command]
pub fn ex_move(
    st: tauri::State<AppState>,
    src: String,
    dest_dir: String,
    mode: Option<String>,
) -> CmdResult<String> {
    let from = PathBuf::from(&src);
    let to_dir = long_path(Path::new(&dest_dir));
    if !from.exists() {
        return Err(AppError::not_found("源不存在 / Source not found"));
    }
    if !to_dir.is_dir() {
        return Err(AppError::not_found("目标文件夹不存在 / Destination folder not found"));
    }
    guard_target(&st, &from)?;
    if from.is_dir() && to_dir.starts_with(&from) {
        return Err(AppError::validation("不能移动到自身内部 / Cannot move into itself"));
    }
    let dest = resolve_dest(&from, &to_dir, mode.as_deref().unwrap_or("auto"))?;
    match fs::rename(long_path(&from), long_path(&dest)) {
        Ok(()) => Ok(display_path(&dest)),
        Err(e) => {
            // 跨盘移动：Windows ERROR_NOT_SAME_DEVICE = 17
            if e.raw_os_error() == Some(17) {
                copy_recursive(&from, &dest, 0)?;
                let lp = long_path(&from);
                if from.is_dir() {
                    fs::remove_dir_all(&lp)?;
                } else {
                    fs::remove_file(&lp)?;
                }
                Ok(display_path(&dest))
            } else {
                Err(AppError::io(format!("移动失败 / Move failed: {e}")))
            }
        }
    }
}

/// 批次C（规格 7.7 冲突检测）：哪些源在目标目录会撞名。
#[tauri::command]
pub fn ex_conflicts(_st: tauri::State<AppState>, srcs: Vec<String>, dest_dir: String) -> CmdResult<Vec<String>> {
    let to_dir = long_path(Path::new(&dest_dir));
    if !to_dir.is_dir() {
        return Err(AppError::not_found("目标文件夹不存在 / Destination folder not found"));
    }
    let mut out = Vec::new();
    for s in srcs {
        let from = Path::new(&s);
        if let Some(name) = from.file_name() {
            if to_dir.join(name).exists() {
                out.push(s);
            }
        }
    }
    Ok(out)
}

/// 删除 → 移入 Variable 全局回收站（可还原）。返回回收站条目 id 列表。
#[tauri::command]
pub fn ex_trash(st: tauri::State<AppState>, paths: Vec<String>) -> CmdResult<Vec<String>> {
    let mut ids = Vec::new();
    for p in paths {
        let path = Path::new(&p);
        guard_target(&st, path)?;
        ids.push(crate::shell::recycle::intern_path(&st, path)?);
    }
    Ok(ids)
}

/// 批次C（规格 7.6 Shift+Delete）：彻底删除（不入回收站，二次确认由前端负责）。
#[tauri::command]
pub fn ex_purge(st: tauri::State<AppState>, paths: Vec<String>) -> CmdResult<u32> {
    let mut n = 0u32;
    for p in paths {
        let path = PathBuf::from(&p);
        guard_target(&st, &path)?;
        let lp = long_path(&path);
        if path.is_dir() {
            fs::remove_dir_all(&lp)?;
        } else {
            fs::remove_file(&lp)?;
        }
        n += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// 批次C（规格 7.3.4 收藏夹 / 7.4.3 搜索 / 7.7 缩略图）
// ---------------------------------------------------------------------------

fn favorites_file(st: &AppState) -> PathBuf {
    st.data_dir.join("explorer_favorites.json")
}

#[tauri::command]
pub fn ex_fav_list(st: tauri::State<AppState>) -> CmdResult<Vec<String>> {
    let f = favorites_file(&st);
    if !f.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&f)?;
    let list: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
    Ok(list.into_iter().filter(|p| Path::new(p).is_dir()).collect())
}

#[tauri::command]
pub fn ex_fav_add(st: tauri::State<AppState>, path: String) -> CmdResult<Vec<String>> {
    if !Path::new(&path).is_dir() {
        return Err(AppError::not_found("文件夹不存在 / Folder not found"));
    }
    let f = favorites_file(&st);
    let mut list: Vec<String> = fs::read_to_string(&f)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    if !list.contains(&path) {
        list.push(path);
        fs::write(&f, serde_json::to_string(&list)?)?;
    }
    Ok(list)
}

#[tauri::command]
pub fn ex_fav_remove(st: tauri::State<AppState>, path: String) -> CmdResult<Vec<String>> {
    let f = favorites_file(&st);
    let mut list: Vec<String> = fs::read_to_string(&f)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    list.retain(|p| p != &path);
    fs::write(&f, serde_json::to_string(&list)?)?;
    Ok(list)
}

/// 批次C（规格 7.4.3 搜索语法）：目录内递归搜索。
/// - 通配符：`*.jpg` `report_*.pdf`（`*` 任意串、`?` 单字符，大小写不敏感）
/// - 布尔：`kw1 AND kw2`、`kw1 OR kw2`、`NOT kw1`；词条间默认 AND
/// - 深度 ≤ 4 层，上限 500 条；权限不足的子目录跳过（不静默失败整次搜索）
pub mod search_syntax {
    //! 查询表达式 → 谓词。独立 pub 以便单元测试。

    #[derive(Debug, PartialEq)]
    pub enum Pred {
        /// 子串匹配（无通配符时）
        Sub(String),
        /// 通配符匹配（含 * ?）
        Glob(String),
        Not(Box<Pred>),
        And(Box<Pred>, Box<Pred>),
        Or(Box<Pred>, Box<Pred>),
    }

    impl Pred {
        pub fn matches(&self, name: &str) -> bool {
            let lower = name.to_lowercase();
            match self {
                Pred::Sub(s) => lower.contains(&s.to_lowercase()),
                Pred::Glob(g) => glob_match(&g.to_lowercase(), &lower),
                Pred::Not(p) => !p.matches(name),
                Pred::And(a, b) => a.matches(name) && b.matches(name),
                Pred::Or(a, b) => a.matches(name) || b.matches(name),
            }
        }
    }

    /// 极简 glob：`*` = 任意串，`?` = 单字符；其余逐字比较。
    pub fn glob_match(pat: &str, text: &str) -> bool {
        // 迭代回溯（避免深层递归）：p/text 指针 + 星号回退点
        let pc: Vec<char> = pat.chars().collect();
        let tc: Vec<char> = text.chars().collect();
        let (mut p, mut t) = (0usize, 0usize);
        let (mut star, mut star_t) = (usize::MAX, 0usize);
        while t < tc.len() {
            if p < pc.len() && (pc[p] == '?' || pc[p] == tc[t]) {
                p += 1;
                t += 1;
            } else if p < pc.len() && pc[p] == '*' {
                star = p;
                star_t = t;
                p += 1;
            } else if star != usize::MAX {
                p = star + 1;
                star_t += 1;
                t = star_t;
            } else {
                return false;
            }
        }
        while p < pc.len() && pc[p] == '*' {
            p += 1;
        }
        p == pc.len()
    }

    /// 解析（递归下降）：or := and (OR and)*; and := not ((AND)? not)*; not := NOT not | term
    /// 运算符位置出现孤立 AND/OR/NOT → None（严格语法，不静默当词条）。
    pub fn parse(query: &str) -> Option<Pred> {
        let tokens: Vec<String> = query.split_whitespace().map(|s| s.to_string()).collect();
        if tokens.is_empty() {
            return None;
        }
        let mut pos = 0usize;
        let pred = parse_or(&tokens, &mut pos)?;
        if pos < tokens.len() {
            return None; // 语法残留
        }
        Some(pred)
    }

    fn is_or(t: &str) -> bool {
        t.eq_ignore_ascii_case("OR")
    }
    fn is_and(t: &str) -> bool {
        t.eq_ignore_ascii_case("AND")
    }

    fn parse_or(tokens: &[String], pos: &mut usize) -> Option<Pred> {
        let mut left = parse_and(tokens, pos)?;
        while *pos < tokens.len() && is_or(&tokens[*pos]) {
            *pos += 1;
            let right = parse_and(tokens, pos)?;
            left = Pred::Or(Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn parse_and(tokens: &[String], pos: &mut usize) -> Option<Pred> {
        let mut left = parse_not(tokens, pos)?;
        while *pos < tokens.len() && !is_or(&tokens[*pos]) {
            if is_and(&tokens[*pos]) {
                *pos += 1;
            }
            let right = parse_not(tokens, pos)?;
            left = Pred::And(Box::new(left), Box::new(right));
        }
        Some(left)
    }

    fn parse_not(tokens: &[String], pos: &mut usize) -> Option<Pred> {
        if *pos < tokens.len() && tokens[*pos].eq_ignore_ascii_case("NOT") {
            *pos += 1;
            let inner = parse_not(tokens, pos)?;
            return Some(Pred::Not(Box::new(inner)));
        }
        if *pos >= tokens.len() {
            return None; // 悬空 NOT / 尾随 AND
        }
        let term = tokens[*pos].clone();
        if is_or(&term) || is_and(&term) {
            return None; // 运算符不能当词条（如 "kw OR OR kw2"）
        }
        *pos += 1;
        if term.contains('*') || term.contains('?') {
            Some(Pred::Glob(term))
        } else {
            Some(Pred::Sub(term))
        }
    }
}

use search_syntax::Pred;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExSearchResult {
    pub entries: Vec<ExEntry>,
    /// 已扫描的目录数（前端状态栏"共扫描 n 个文件夹"）
    pub scanned: u32,
    /// 因达到上限而提前停止
    pub truncated: bool,
}

const SEARCH_MAX: usize = 500;
const SEARCH_DEPTH: u8 = 4;

fn search_dir(
    dir: &Path,
    pred: &Pred,
    out: &mut Vec<ExEntry>,
    scanned: &mut u32,
    truncated: &mut bool,
    depth: u8,
) {
    if *truncated || depth > SEARCH_DEPTH {
        return;
    }
    let rd = match fs::read_dir(long_path(dir)) {
        Ok(rd) => rd,
        Err(_) => return, // 权限不足/读取失败：跳过该子目录
    };
    *scanned += 1;
    for item in rd.flatten() {
        if out.len() >= SEARCH_MAX {
            *truncated = true;
            return;
        }
        let p = item.path();
        let name = item.file_name().to_string_lossy().to_string();
        if pred.matches(&name) {
            if let Some(e) = entry_from_path(&p) {
                out.push(e);
            }
        }
        if p.is_dir() {
            search_dir(&p, pred, out, scanned, truncated, depth + 1);
            if out.len() >= SEARCH_MAX {
                *truncated = true;
                return;
            }
        }
    }
}

/// 目录内搜索（规格 7.4.3：通配符 + AND/OR/NOT）。空查询返回空结果。
#[tauri::command]
pub fn ex_search(_st: tauri::State<AppState>, path: String, query: String) -> CmdResult<ExSearchResult> {
    let dir = PathBuf::from(&path);
    if !long_path(&dir).is_dir() {
        return Err(AppError::not_found(format!("目录不存在 / Directory not found: {path}")));
    }
    let Some(pred) = search_syntax::parse(&query) else {
        return Ok(ExSearchResult { entries: Vec::new(), scanned: 0, truncated: false });
    };
    let mut out = Vec::new();
    let mut scanned = 0u32;
    let mut truncated = false;
    search_dir(&dir, &pred, &mut out, &mut scanned, &mut truncated, 0);
    out.sort_by(|a, b| {
        let (da, db) = (a.kind == "dir", b.kind == "dir");
        db.cmp(&da).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(ExSearchResult { entries: out, scanned, truncated })
}

const THUMB_EXTS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"];
const THUMB_MAX_BYTES: u64 = 15 * 1024 * 1024;

/// 批次C（规格 7.7）：图片缩略图 → data URL（WebView 直接渲染）。
/// 仅支持 WebView 原生可解码的位图格式；超过 15MB 的文件如实返回错误（不伪造缩略图）。
/// 视频无解码器，不提供假缩略图 —— 前端对视频显示文件图标。
#[tauri::command]
pub fn ex_thumbnail(_st: tauri::State<AppState>, path: String) -> CmdResult<String> {
    let p = PathBuf::from(&path);
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !THUMB_EXTS.contains(&ext.as_str()) {
        return Err(AppError::validation("不支持的缩略图格式 / Thumbnail format not supported"));
    }
    let meta = fs::metadata(long_path(&p)).map_err(|e| AppError::io(format!("读取失败 / Stat failed: {e}")))?;
    if meta.len() > THUMB_MAX_BYTES {
        return Err(AppError::validation("文件过大，未生成缩略图 / File too large for a thumbnail"));
    }
    let bytes = fs::read(long_path(&p))?;
    // base64（复用 launcher 的手写实现，零新依赖）
    let b64 = crate::shell::launcher::b64_encode(&bytes);
    let mime = match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "image/x-icon",
    };
    Ok(format!("data:{mime};base64,{b64}"))
}

#[cfg(test)]
mod tests {
    use super::search_syntax::{glob_match, parse};

    #[test]
    fn glob_matches_wildcards() {
        assert!(glob_match("*.jpg", "photo.jpg"));
        // 大小写不敏感在 Pred 层完成（glob_match 本身逐字比较）
        assert!(parse("*.jpg").unwrap().matches("PHOTO.JPG"));
        assert!(!glob_match("*.jpg", "photo.png"));
        assert!(glob_match("report_*.pdf", "report_2026.pdf"));
        assert!(!glob_match("report_*.pdf", "summary_2026.pdf"));
        assert!(glob_match("file?.txt", "file1.txt"));
        assert!(!glob_match("file?.txt", "file10.txt"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn boolean_search_semantics() {
        // 隐式 AND
        let p = parse("report 2026").unwrap();
        assert!(p.matches("report_2026_final.docx"));
        assert!(!p.matches("report_final.docx"));
        // OR
        let p = parse("jpg OR png").unwrap();
        assert!(p.matches("a.jpg"));
        assert!(p.matches("b.png"));
        assert!(!p.matches("c.gif"));
        // NOT
        let p = parse("NOT tmp").unwrap();
        assert!(p.matches("keep.docx"));
        assert!(!p.matches("keep.tmp"));
        // 组合：通配符 AND NOT
        let p = parse("*.jpg NOT draft").unwrap();
        assert!(p.matches("final.jpg"));
        assert!(!p.matches("draft.jpg"));
        // 语法错误 → None
        assert!(parse("").is_none());
        assert!(parse("kw AND").is_none()); // 尾随 AND 悬空
        assert!(parse("kw NOT").is_none()); // 悬空 NOT
        assert!(parse("kw OR OR kw2").is_none()); // 孤立 OR
    }
}
