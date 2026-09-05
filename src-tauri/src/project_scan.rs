//! 项目结构可视化引擎 — 第一层：文件系统扫描与索引引擎（规范第二章）。
//! Project Visualization & Code Comprehension Engine — Layer 1: file system
//! scanning & indexing. Purely local: walks a user-dropped project folder,
//! classifies entries and reads bounded text sources for the frontend parsers.
//! No network, no data leaves the machine (chapter 11).

use crate::error::{AppError, CmdResult};
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Noise directories that never carry project semantics (spec 2.1 忽略清单).
const IGNORED_DIRS: &[&str] = &[
    "node_modules", ".git", ".svn", ".hg", "dist", "build", "out", "target",
    "__pycache__", ".venv", "venv", "env", "vendor", "coverage", ".nyc_output",
    ".next", ".turbo", ".cache", ".parcel-cache", ".gradle", "Pods",
    "DerivedData", ".idea", ".vs", ".vscode", ".mimosa", ".trash", "bin", "obj",
    ".pytest_cache", ".mypy_cache", "site-packages", "Debug", "Release",
];

/// Extensions whose contents are fed to the frontend structural parsers.
const SOURCE_EXTS: &[&str] = &[
    "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "vue", "svelte",
    "py", "rs", "go", "java", "kt", "kts", "swift", "php", "rb", "cs", "dart",
    "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx", "ino",
    "html", "htm", "css", "scss", "less", "sql", "sh", "bash", "zsh",
    "json", "yaml", "yml", "toml", "ini", "cfg", "xml", "md", "markdown",
    "txt", "gradle", "groovy", "lua", "pl", "pm", "r", "m", "proto",
];

/// Well-known entry / config / doc basenames are read first so a huge repo
/// still surfaces its identity within the byte budget (spec 2.2 证据链).
const PRIORITY_BASENAMES: &[&str] = &[
    "package.json", "cargo.toml", "pom.xml", "requirements.txt", "pyproject.toml",
    "go.mod", "go.sum", "build.gradle", "settings.gradle", "composer.json",
    "gemfile", "pubspec.yaml", "podfile", "dockerfile", "docker-compose.yml",
    "makefile", "cmakelists.txt", "readme.md", "license", "tsconfig.json",
    "vite.config.ts", "main.rs", "lib.rs", "mod.rs", "main.py", "app.py",
    "__init__.py", "index.ts", "index.js", "main.ts", "main.tsx", "index.tsx",
    "app.tsx", "app.jsx", "main.go", "main.java", "program.cs", "app.swift",
];

const MAX_DEPTH: usize = 14;
const MAX_ENTRIES: usize = 8000;
/// Per-file read cap: enough for any real module, keeps IPC bounded.
const MAX_READ_BYTES: u64 = 256 * 1024;
/// Total number of source files fed to the parsers in one scan.
const MAX_SOURCE_FILES: usize = 500;
/// Total source bytes fed to the parsers in one scan.
const MAX_TOTAL_SOURCE_BYTES: usize = 6 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanEntry {
    /// Relative path with `/` separators; the root itself is `""`.
    pub path: String,
    pub name: String,
    /// `"dir"` | `"file"`
    pub kind: String,
    pub ext: Option<String>,
    pub size: u64,
    /// 0 = root's direct children.
    pub depth: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub rel_path: String,
    pub content: String,
    /// True when the file was cut off at the byte cap.
    pub truncated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    /// Display path of the project root (no `\\?\` prefix).
    pub root: String,
    pub entries: Vec<ScanEntry>,
    pub sources: Vec<SourceFile>,
    /// Entry/dir cap hit — the tree is partial.
    pub truncated: bool,
    /// Files+dirs skipped due to caps or unreadable permissions.
    pub skipped: u64,
}

fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

fn ext_of(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
}

fn is_source_ext(ext: Option<&String>) -> bool {
    match ext {
        Some(e) => SOURCE_EXTS.contains(&e.as_str()),
        None => false,
    }
}

/// Lower sorts earlier: 0 = identity/config/entry evidence, 1 = source, 2 = rest.
fn source_priority(rel: &str) -> u8 {
    let lower = rel.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if PRIORITY_BASENAMES.contains(&name) {
        return 0;
    }
    match ext_of(name).as_deref() {
        Some(e) if matches!(e, "md" | "json" | "toml" | "yaml" | "yml" | "xml" | "gradle") => 0,
        Some(e) if SOURCE_EXTS.contains(&e) => 1,
        _ => 2,
    }
}

/// Refuse to read binary payloads: NUL byte in the head is a reliable marker.
fn looks_binary(buf: &[u8]) -> bool {
    let head = &buf[..buf.len().min(8192)];
    head.contains(&0u8)
}

/// Canonicalize the root and return (canonical, display). Rejects non-dirs.
fn canonical_root(root: &str) -> CmdResult<(PathBuf, String)> {
    let p = Path::new(root);
    if !p.is_dir() {
        return Err(AppError::not_found(format!(
            "项目目录不存在 / Project folder not found: {root}"
        )));
    }
    let canon = p
        .canonicalize()
        .map_err(|e| AppError::io(format!("无法解析项目目录 / Cannot resolve project root: {e}")))?;
    let display = display_path(&canon);
    Ok((canon, display))
}

/// Validate that a client-supplied relative path stays inside the root and has
/// no traversal components (defence in depth on top of walking ourselves).
fn resolve_inside(root_canon: &Path, rel: &str) -> CmdResult<PathBuf> {
    if rel.is_empty() {
        return Ok(root_canon.to_path_buf());
    }
    let cleaned = rel.replace('\\', "/");
    if cleaned.split('/').any(|seg| seg == ".." || seg.is_empty() || seg.contains(':')) {
        return Err(AppError::validation("路径越界 / Path escapes the project root"));
    }
    let full = root_canon.join(&cleaned);
    // Canonicalize the deepest existing ancestor to defeat symlink jumps.
    let mut ancestor = full.clone();
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match ancestor.canonicalize() {
            Ok(canon) => {
                let mut resolved = canon;
                for part in suffix.iter().rev() {
                    resolved.push(part);
                }
                if !resolved.starts_with(root_canon) {
                    return Err(AppError::validation("路径越界 / Path escapes the project root"));
                }
                return Ok(resolved);
            }
            Err(_) => match ancestor.file_name() {
                Some(name) => {
                    suffix.push(name.to_os_string());
                    if !ancestor.pop() {
                        return Err(AppError::validation("无效路径 / Invalid path"));
                    }
                }
                None => return Err(AppError::validation("无效路径 / Invalid path")),
            },
        }
    }
}

struct WalkCtx {
    out: Vec<ScanEntry>,
    skipped: u64,
    truncated: bool,
}

/// Depth-first walk. Deterministic (siblings sorted by name), skips symlinks
/// (no escape hatches out of the root), bounded by depth and entry caps.
fn walk_dir(ctx: &mut WalkCtx, abs: &Path, rel: &str, depth: u32) {
    if ctx.out.len() >= MAX_ENTRIES {
        ctx.truncated = true;
        return;
    }
    let Ok(read) = fs::read_dir(abs) else {
        ctx.skipped += 1;
        return;
    };
    let mut children: Vec<(PathBuf, std::fs::DirEntry)> = Vec::new();
    for entry in read.flatten() {
        children.push((entry.path(), entry));
    }
    children.sort_by(|a, b| {
        let an = a.1.file_name().to_string_lossy().to_lowercase();
        let bn = b.1.file_name().to_string_lossy().to_lowercase();
        an.cmp(&bn)
    });

    for (path, entry) in children {
        if ctx.out.len() >= MAX_ENTRIES {
            ctx.truncated = true;
            return;
        }
        // Symlinks never enter the scan: they can point anywhere.
        let Ok(ft) = entry.file_type() else {
            ctx.skipped += 1;
            continue;
        };
        if ft.is_symlink() {
            ctx.skipped += 1;
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let child_rel = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if ft.is_dir() {
            let lname = name.to_lowercase();
            if IGNORED_DIRS.contains(&lname.as_str()) {
                ctx.skipped += 1;
                continue;
            }
            ctx.out.push(ScanEntry {
                path: child_rel.clone(),
                name,
                kind: "dir".into(),
                ext: None,
                size: 0,
                depth,
            });
            if depth < MAX_DEPTH as u32 {
                walk_dir(ctx, &path, &child_rel, depth + 1);
            } else {
                ctx.truncated = true;
            }
        } else if ft.is_file() {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let ext = ext_of(&name);
            ctx.out.push(ScanEntry {
                path: child_rel.clone(),
                name,
                kind: "file".into(),
                ext,
                size,
                depth,
            });
        } else {
            ctx.skipped += 1;
        }
    }
}

/// 规范 2.1：递归深度扫描 + 类型识别 + 有界源码读取，一次 IPC 返回全部原料。
#[tauri::command]
pub fn project_scan(root: String) -> CmdResult<ScanResult> {
    let (root_canon, root_display) = canonical_root(&root)?;

    let mut ctx = WalkCtx {
        out: Vec::new(),
        skipped: 0,
        truncated: false,
    };
    walk_dir(&mut ctx, &root_canon, "", 0);

    // ---- bounded source read (priority: evidence → small sources → rest) ----
    let mut file_idx: Vec<usize> = ctx
        .out
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == "file" && is_source_ext(e.ext.as_ref()) && e.size <= MAX_READ_BYTES)
        .map(|(i, _)| i)
        .collect();
    file_idx.sort_by(|&a, &b| {
        let ea = &ctx.out[a];
        let eb = &ctx.out[b];
        source_priority(&ea.path)
            .cmp(&source_priority(&eb.path))
            .then_with(|| ea.depth.cmp(&eb.depth))
            .then_with(|| ea.size.cmp(&eb.size))
            .then_with(|| ea.path.cmp(&eb.path))
    });

    let mut sources: Vec<SourceFile> = Vec::new();
    let mut total = 0usize;
    let mut read_guard = HashSet::new();
    for i in file_idx {
        if sources.len() >= MAX_SOURCE_FILES || total >= MAX_TOTAL_SOURCE_BYTES {
            break;
        }
        let entry = &ctx.out[i];
        if !read_guard.insert(entry.path.clone()) {
            continue;
        }
        let abs = root_canon.join(entry.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let Ok(bytes) = fs::read(&abs) else {
            ctx.skipped += 1;
            continue;
        };
        if looks_binary(&bytes) {
            continue;
        }
        let truncated = bytes.len() as u64 > MAX_READ_BYTES;
        let take = bytes.len().min(MAX_READ_BYTES as usize);
        let slice = &bytes[..take];
        total += take;
        sources.push(SourceFile {
            rel_path: entry.path.clone(),
            content: String::from_utf8_lossy(slice).into_owned(),
            truncated,
        });
    }

    Ok(ScanResult {
        root: root_display,
        entries: ctx.out,
        sources,
        truncated: ctx.truncated,
        skipped: ctx.skipped,
    })
}

/// 通用只读文本读取（供 .project 档案等应用文件使用，非项目扫描路径）。
#[tauri::command]
pub fn read_text_file(path: String) -> CmdResult<String> {
    let p = Path::new(&path);
    let meta = fs::metadata(p).map_err(|_| AppError::not_found(format!("文件不存在 / Not found: {path}")))?;
    if !meta.is_file() {
        return Err(AppError::validation("不是文件 / Not a file"));
    }
    if meta.len() > 16 * 1024 * 1024 {
        return Err(AppError::validation("文件过大 / File too large"));
    }
    let bytes = fs::read(p).map_err(AppError::from)?;
    if looks_binary(&bytes) {
        return Err(AppError::validation("二进制文件 / Binary file"));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// 批次E：通用文本写入（仅用于设置导入/导出等用户显式选择的路径）。
#[tauri::command]
pub fn write_text_file(path: String, contents: String) -> CmdResult<()> {
    // 防误写：仅允许 .json/.txt 后缀（导入导出场景）
    let ok_ext = std::path::Path::new(&path)
        .extension()
        .map(|e| e == "json" || e == "txt" || e == "lang")
        .unwrap_or(false);
    if !ok_ext {
        return Err(AppError::validation("仅允许写入 .json/.txt / Only .json/.txt allowed"));
    }
    fs::write(&path, contents.as_bytes()).map_err(AppError::from)
}

/// 规范 5.2 / 8.2：信息卡与下钻的按需单文件读取（延迟重解析用）。
#[tauri::command]
pub fn project_read_file(root: String, rel_path: String) -> CmdResult<SourceFile> {
    let (root_canon, _) = canonical_root(&root)?;
    let abs = resolve_inside(&root_canon, &rel_path)?;
    let meta = fs::metadata(&abs).map_err(AppError::from)?;
    if !meta.is_file() {
        return Err(AppError::not_found("不是文件 / Not a file"));
    }
    if meta.len() > 4 * 1024 * 1024 {
        return Err(AppError::validation("文件过大 / File too large to display"));
    }
    let bytes = fs::read(&abs).map_err(AppError::from)?;
    if looks_binary(&bytes) {
        return Err(AppError::validation("二进制文件 / Binary file"));
    }
    let truncated = bytes.len() as u64 > MAX_READ_BYTES;
    let take = bytes.len().min(MAX_READ_BYTES as usize);
    Ok(SourceFile {
        rel_path: rel_path.replace('\\', "/"),
        content: String::from_utf8_lossy(&bytes[..take]).into_owned(),
        truncated,
    })
}

/// Binary analysis feed: bounded raw bytes of a binary file (PE/ELF/Mach-O)
/// so the frontend can parse headers/sections/imports/exports offline.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryFile {
    pub rel_path: String,
    pub bytes: Vec<u8>,
    pub size: u64,
    pub truncated: bool,
}

/// Per-file read cap for binary analysis: PE headers/tables live at the front.
const MAX_BINARY_READ: u64 = 2 * 1024 * 1024;

#[tauri::command]
pub fn project_read_bytes(root: String, rel_path: String) -> CmdResult<BinaryFile> {
    let (root_canon, _) = canonical_root(&root)?;
    let abs = resolve_inside(&root_canon, &rel_path)?;
    let meta = fs::metadata(&abs).map_err(AppError::from)?;
    if !meta.is_file() {
        return Err(AppError::not_found("不是文件 / Not a file"));
    }
    if meta.len() > 64 * 1024 * 1024 {
        return Err(AppError::validation("文件过大 / File too large to analyze"));
    }
    let bytes = fs::read(&abs).map_err(AppError::from)?;
    let truncated = bytes.len() as u64 > MAX_BINARY_READ;
    let take = bytes.len().min(MAX_BINARY_READ as usize);
    Ok(BinaryFile {
        rel_path: rel_path.replace('\\', "/"),
        bytes: bytes[..take].to_vec(),
        size: meta.len(),
        truncated,
    })
}
