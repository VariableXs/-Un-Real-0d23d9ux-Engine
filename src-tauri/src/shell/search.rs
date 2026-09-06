//! 并行全库搜索 + 大文件分块读 + 行级跳转（B-23，M5；蓝图 3.7 / 性能表 444 行）。
//!
//! - 搜索：标准库 scoped threads 并行扫描容器内工作区；跳过二进制（NUL 探测）
//!   与超大文件（默认 >8MB，防随机写介质卡死）；结果上限 500 行命中；
//!   只搜容器内路径（ensure-in-container 同 B-22 口径）；
//! - 大文件分块读：`bigfile_slice` 按 offset/len 读（前端分页虚拟化，
//!   100MB 打开 <1.5s 口径由"只读当前页"保证）；
//! - 行级跳转：`editor_goto` 借 VS Code Portable `--goto file:line` 打开
//!   （PVCCE ↔ VS Code 双向跳转的 VS Code 侧；PVCCE 侧已有 xref.ts）。

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;

use crate::error::AppError;
use crate::state::AppState;

type CmdResult<T> = Result<T, AppError>;

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_MATCH_LINES: usize = 500;
const MAX_FILE_RESULTS: usize = 60;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchLine {
    pub line_no: u64,
    pub text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchFileHits {
    pub path: String,
    pub lines: Vec<SearchLine>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchReport {
    pub query: String,
    pub files_scanned: usize,
    pub files_skipped_binary: usize,
    pub files_skipped_size: usize,
    pub truncated: bool,
    pub hits: Vec<SearchFileHits>,
    pub elapsed_ms: u64,
}

fn is_binary(buf: &[u8]) -> bool {
    buf[..buf.len().min(4096)].contains(&0)
}

fn search_one_file(path: &Path, query: &str, results: &mut Vec<SearchFileHits>) -> (bool, bool) {
    // 返回 (是否扫描, 是否二进制)
    let Ok(mut f) = std::fs::File::open(path) else {
        return (false, false);
    };
    let Ok(meta) = f.metadata() else { return (false, false) };
    if meta.len() > MAX_FILE_BYTES {
        return (false, false);
    }
    let mut buf = Vec::with_capacity(meta.len() as usize);
    if f.read_to_end(&mut buf).is_err() {
        return (false, false);
    }
    if is_binary(&buf) {
        return (true, true);
    }
    let lower_q = query.to_lowercase();
    let text = String::from_utf8_lossy(&buf);
    let mut lines = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if lines.len() >= 20 {
            break; // 单文件命中上限
        }
        if line.to_lowercase().contains(&lower_q) {
            // 裁剪长行：命中位置前后各 80 字符
            let lower_line = line.to_lowercase();
            let pos = lower_line.find(&lower_q).unwrap_or(0);
            let start = pos.saturating_sub(80);
            let end = (pos + query.len() + 80).min(line.len());
            let clipped = format!(
                "{}{}{}",
                if start > 0 { "…" } else { "" },
                &line[start..end],
                if end < line.len() { "…" } else { "" }
            );
            lines.push(SearchLine {
                line_no: (i + 1) as u64,
                text: clipped,
            });
        }
    }
    if !lines.is_empty() {
        results.push(SearchFileHits {
            path: path.to_string_lossy().into_owned(),
            lines,
        });
    }
    (true, false)
}

pub(crate) fn search_tree(
    root: &Path,
    query: &str,
) -> CmdResult<SearchReport> {
    let started = std::time::Instant::now();
    if query.trim().is_empty() {
        return Err(AppError::validation("搜索词为空"));
    }
    // 收集文件清单（单线程走目录树，随后并行读文件）
    let mut files: Vec<PathBuf> = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    let skipped_binary = AtomicUsize::new(0);
    let skipped_size = AtomicUsize::new(0);
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                // 跳过 VCS 与依赖目录（口径与主流编辑器一致）
                let name = e.file_name().to_string_lossy().into_owned();
                if matches!(name.as_str(), ".git" | "node_modules" | "target" | "__pycache__") {
                    continue;
                }
                stack.push(p);
            } else {
                files.push(p);
            }
        }
    }
    files.truncate(20_000); // 防失控（超大工作区如实截断文件数）
    let counter = AtomicUsize::new(0);
    let truncated = AtomicUsize::new(0);
    let results = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        let chunk = (files.len() / 8).max(1);
        for group in files.chunks(chunk) {
            let q = query.to_string();
            let counter_ref = &counter;
            let truncated_ref = &truncated;
            let sb = &skipped_binary;
            let ss = &skipped_size;
            handles.push(scope.spawn(move || {
                let mut local: Vec<SearchFileHits> = Vec::new();
                for p in group {
                    if counter_ref.load(Ordering::Relaxed) >= MAX_FILE_RESULTS {
                        truncated_ref.store(1, Ordering::Relaxed);
                        break;
                    }
                    let (scanned, binary) = search_one_file(p, &q, &mut local);
                    if binary {
                        sb.fetch_add(1, Ordering::Relaxed);
                    } else if !scanned {
                        ss.fetch_add(1, Ordering::Relaxed);
                    }
                    if !local.is_empty() {
                        let n: usize = local.iter().map(|f| f.lines.len()).sum();
                        counter_ref.fetch_add(n, Ordering::Relaxed);
                    }
                }
                local
            }));
        }
        let mut all: Vec<SearchFileHits> = handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .flatten()
            .collect();
        all.sort_by(|a, b| a.path.cmp(&b.path));
        all
    });
    Ok(SearchReport {
        query: query.to_string(),
        files_scanned: files.len(),
        files_skipped_binary: skipped_binary.load(Ordering::Relaxed),
        files_skipped_size: skipped_size.load(Ordering::Relaxed),
        truncated: truncated.load(Ordering::Relaxed) == 1,
        hits: results,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[tauri::command]
pub fn workspace_search(st: tauri::State<AppState>, root: String, query: String) -> CmdResult<SearchReport> {
    // 只搜容器内路径
    let canonical_root = st
        .data_dir
        .canonicalize()
        .map_err(|e| AppError::io(e.to_string()))?;
    let p = PathBuf::from(&root);
    let canonical = p.canonicalize().map_err(|e| AppError::io(e.to_string()))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(AppError::validation(format!(
            "搜索根不在容器内: {root}"
        )));
    }
    search_tree(&canonical, &query)
}

// ---------- 大文件分块读 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSlice {
    pub offset: u64,
    pub size: u64,
    pub total: u64,
    pub text_lossy: String,
}

#[tauri::command]
pub fn bigfile_slice(path: String, offset: u64, len: u32) -> CmdResult<FileSlice> {
    let mut f = std::fs::File::open(&path).map_err(|e| AppError::io(e.to_string()))?;
    let total = f.metadata()?.len();
    if offset >= total {
        return Ok(FileSlice {
            offset,
            size: 0,
            total,
            text_lossy: String::new(),
        });
    }
    f.seek(SeekFrom::Start(offset))
        .map_err(|e| AppError::io(e.to_string()))?;
    let mut buf = vec![0u8; (len as u64).min(total - offset) as usize];
    f.read_exact(&mut buf).map_err(|e| AppError::io(e.to_string()))?;
    Ok(FileSlice {
        offset,
        size: buf.len() as u64,
        total,
        text_lossy: String::from_utf8_lossy(&buf).into_owned(),
    })
}

// ---------- 行级跳转（PVCCE → VS Code） ----------

/// `code --goto file:line`：VS Code Portable 已部署时打开并定位行。
#[tauri::command]
pub fn editor_goto(
    st: tauri::State<AppState>,
    path: String,
    line: u32,
) -> CmdResult<String> {
    let exe = st.data_dir.join("runtime").join("vscode").join("Code.exe");
    if !exe.is_file() {
        return Err(AppError::not_found(
            "VS Code 未部署（设置 → 编码 → 一键部署）",
        ));
    }
    // 行级跳转参数：file:line（VS Code CLI --goto）
    let arg = format!("{}:{}", path, line);
    let child = std::process::Command::new(&exe)
        .arg("--goto")
        .arg(&arg)
        .spawn()
        .map_err(|e| AppError::io(e.to_string()))?;
    Ok(format!("spawned pid {}", child.id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("srch-b23-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn search_finds_matches_and_skips_binary_and_node_modules() {
        let root = temp_dir("tree");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("src/a.rs"), "fn main() {\n    let alpha = 1;\n}\n").unwrap();
        std::fs::write(root.join("src/deep.txt"), "alpha here\nnope\nALPHA UPPER\n").unwrap();
        std::fs::write(root.join("node_modules/pkg/x.js"), "alpha in deps").unwrap();
        // 二进制文件（含 NUL）
        std::fs::write(root.join("bin.dat"), b"\0\0alpha\0").unwrap();
        // 超大文件（>8MB 跳过）
        std::fs::write(root.join("big.txt"), vec![b'a'; 9 * 1024 * 1024]).unwrap();

        let report = search_tree(&root, "alpha").unwrap();
        assert_eq!(report.files_scanned, 4); // node_modules 整目录被跳过，不计入
        // node_modules 与二进制与大文件都不出结果
        assert!(!report.hits.iter().any(|h| h.path.contains("node_modules")));
        assert!(!report.hits.iter().any(|h| h.path.contains("bin.dat")));
        assert!(!report.hits.iter().any(|h| h.path.contains("big.txt")));
        assert!(report.files_skipped_binary >= 1);
        assert!(report.files_skipped_size >= 1);
        // 大小写不敏感：deep.txt 命中 2 行
        let deep = report.hits.iter().find(|h| h.path.contains("deep.txt")).unwrap();
        assert_eq!(deep.lines.len(), 2);
        assert_eq!(deep.lines[1].line_no, 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_query_rejected() {
        let root = temp_dir("empty");
        assert!(search_tree(&root, "").is_err());
        assert!(search_tree(&root, "  ").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bigfile_slice_reads_window() {
        let p = temp_dir("slice").join("f.txt");
        std::fs::write(&p, b"0123456789abcdef").unwrap();
        let s = bigfile_slice(p.to_string_lossy().into_owned(), 4, 6).unwrap();
        assert_eq!(s.size, 6);
        assert_eq!(s.text_lossy, "456789");
        // 越界截断
        let s = bigfile_slice(p.to_string_lossy().into_owned(), 12, 100).unwrap();
        assert_eq!(s.size, 4);
        assert_eq!(s.text_lossy, "cdef");
        // 越界起点
        let s = bigfile_slice(p.to_string_lossy().into_owned(), 99, 10).unwrap();
        assert_eq!(s.size, 0);
        let _ = std::fs::remove_file(&p);
    }
}
