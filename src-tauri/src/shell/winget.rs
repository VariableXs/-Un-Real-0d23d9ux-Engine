//! AI-15 V-81 开放安装器：本机 winget CLI 的 GUI 前端。
//!
//! 红线（承化境计划）：
//! - 零自带源、零自动出站 —— 网络行为全部由 winget 自身发生且逐次可见；
//! - 每次操作（安装/更新/卸载）由前端逐次确认，本模块不提供静默批量；
//! - 无 winget 时诚实降级（status 返回 available=false + 指路提示）。
//!
//! 解析：winget 交互输出不可靠（进度条用退格符重绘），统一加
//! `--disable-interactivity`，安装进度走事件 `winget://progress`（按百分比行解析）。

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::error::{AppError, CmdResult};

fn winget_bin() -> Command {
    let mut c = Command::new("winget");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    c
}

/// winget 可用性（where winget + --version）。无 = 前端诚实降级。
#[tauri::command]
pub fn winget_status() -> CmdResult<WingetStatus> {
    let probe = winget_bin().arg("--version").output();
    match probe {
        Ok(o) if o.status.success() => Ok(WingetStatus {
            available: true,
            version: String::from_utf8_lossy(&o.stdout).trim().to_string(),
        }),
        _ => Ok(WingetStatus { available: false, version: String::new() }),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WingetStatus {
    pub available: bool,
    pub version: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WingetPkg {
    pub name: String,
    pub id: String,
    pub version: String,
    pub available: String,
    pub source: String,
}

/// winget 输出列解析：表头行定位列偏移，后续行按偏移切列（tab 或多空格自适应）。
/// 纯函数（前端 vitest 有对齐镜像测试）。
pub fn parse_winget_table(out: &str) -> Vec<WingetPkg> {
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut pkgs = Vec::new();
    // 找表头（Name ... Id ... Version ...）
    let mut header_idx = None;
    for (i, l) in lines.iter().enumerate() {
        if l.contains("Name") && l.contains("Id") && (l.contains("Version") || l.contains("版本")) {
            header_idx = Some(i);
            break;
        }
    }
    let Some(hi) = header_idx else { return pkgs };
    // 列区间按表头关键词起始位置切
    let hdr = lines[hi];
    let col = |keys: &[&str]| -> Option<usize> {
        keys.iter().filter_map(|k| hdr.find(k)).min()
    };
    let (c_name, c_id, c_ver) = (col(&["Name", "名称"]), col(&["Id", "ID", "标识"]), col(&["Version", "版本"]));
    let (c_avail, c_src) = (col(&["Available", "可用"]), col(&["Source", "源"]));
    if c_name.is_none() || c_id.is_none() {
        return pkgs;
    }
    for l in lines.iter().skip(hi + 2) {
        // 跳过分隔线（- 组成）
        if l.trim_start().starts_with('-') && !l.chars().any(|c| c.is_alphanumeric()) {
            continue;
        }
        let b = l.as_bytes();
        let get = |start: Option<usize>, end: Option<usize>| -> String {
            let s = match (start, end) {
                (Some(s), Some(e)) if e > s => {
                    String::from_utf8_lossy(&b[s..e]).trim().to_string()
                }
                (Some(s), None) => String::from_utf8_lossy(&b[s..]).trim().to_string(),
                _ => String::new(),
            };
            // winget 用 "-" 表示空档位 —— 归一为空串
            if s == "-" { String::new() } else { s }
        };
        let ends = [c_ver, c_avail, c_src, Some(l.len())].into_iter().flatten().collect::<Vec<_>>();
        let next_col_after = |from: usize| -> Option<usize> {
            ends.iter().copied().filter(|e| *e > from).min()
        };
        let id_end = next_col_after(c_id.unwrap());
        let name_end = if c_name.unwrap() < c_id.unwrap() { c_id } else { next_col_after(c_name.unwrap()) };
        let pkgs_item = WingetPkg {
            name: get(c_name, name_end),
            id: get(c_id, id_end),
            version: get(c_ver, c_ver.and_then(next_col_after)),
            available: get(c_avail, c_avail.and_then(next_col_after)),
            source: get(c_src, c_src.map(|_| l.len())),
        };
        if !pkgs_item.id.is_empty() && pkgs_item.id != "-" {
            pkgs.push(pkgs_item);
        }
    }
    pkgs
}

/// 解析进度百分比行（winget 中文/英文输出的 `xx%` 片段）。
pub fn parse_progress_line(line: &str) -> Option<u8> {
    let mut best: Option<u8> = None;
    for m in line.match_indices('%') {
        let start = line[..m.0]
            .rfind(|c: char| !(c.is_ascii_digit()))
            .map(|i| i + 1)
            .unwrap_or(0);
        if let Ok(v) = line[start..m.0].parse::<u8>() {
            best = Some(v);
        }
    }
    best
}

#[tauri::command]
pub fn winget_search(query: String) -> CmdResult<Vec<WingetPkg>> {
    if query.trim().is_empty() {
        return Ok(vec![]);
    }
    let out = winget_bin()
        .args(["search", "--accept-source-agreements", "--disable-interactivity", &query])
        .output()
        .map_err(|e| AppError::io(format!("winget 调用失败: {e}")))?;
    Ok(parse_winget_table(&String::from_utf8_lossy(&out.stdout)))
}

#[tauri::command]
pub fn winget_list_installed() -> CmdResult<Vec<WingetPkg>> {
    let out = winget_bin()
        .args(["list", "--disable-interactivity"])
        .output()
        .map_err(|e| AppError::io(format!("winget 调用失败: {e}")))?;
    Ok(parse_winget_table(&String::from_utf8_lossy(&out.stdout)))
}

/// 可更新列表（winget upgrade）。
#[tauri::command]
pub fn winget_upgrade_list() -> CmdResult<Vec<WingetPkg>> {
    let out = winget_bin()
        .args(["upgrade", "--include-unknown", "--disable-interactivity"])
        .output()
        .map_err(|e| AppError::io(format!("winget 调用失败: {e}")))?;
    Ok(parse_winget_table(&String::from_utf8_lossy(&out.stdout)))
}

fn run_with_progress(
    app: &tauri::AppHandle,
    verb: &str,
    id: &str,
    extra: &[&str],
) -> CmdResult<WingetOpResult> {
    let mut cmd = winget_bin();
    cmd.args([
        verb,
        id,
        "--disable-interactivity",
        "--accept-package-agreements",
        "--accept-source-agreements",
    ]);
    cmd.args(extra);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::io(format!("winget 启动失败: {e}")))?;
    let stdout = child.stdout.take();
    if let Some(o) = stdout {
        let reader = BufReader::new(o);
        for line in reader.lines().map_while(|l| l.ok()) {
            if let Some(p) = parse_progress_line(&line) {
                let _ = app.emit(
                    "winget://progress",
                    serde_json::json!({ "op": verb, "id": id, "percent": p, "line": line }),
                );
            } else if line.contains('-') && !line.trim().is_empty() {
                // 中文 winget 进度形如  "-hängig  ██ xx% " —— 兜底再试
                let _ = app.emit(
                    "winget://line",
                    serde_json::json!({ "op": verb, "id": id, "line": line }),
                );
            }
        }
    }
    let status = child.wait().map_err(|e| AppError::io(e.to_string()))?;
    Ok(WingetOpResult { ok: status.success(), exit_code: status.code() })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WingetOpResult {
    pub ok: bool,
    pub exit_code: Option<i32>,
}

#[tauri::command]
pub fn winget_install(app: tauri::AppHandle, id: String, exact: Option<bool>) -> CmdResult<WingetOpResult> {
    let mut extra: Vec<&str> = vec![];
    if exact.unwrap_or(false) {
        extra.push("--exact");
    }
    run_with_progress(&app, "install", &id, &extra)
}

#[tauri::command]
pub fn winget_upgrade_one(app: tauri::AppHandle, id: String) -> CmdResult<WingetOpResult> {
    run_with_progress(&app, "upgrade", &id, &[])
}

#[tauri::command]
pub fn winget_uninstall(app: tauri::AppHandle, id: String) -> CmdResult<WingetOpResult> {
    run_with_progress(&app, "uninstall", &id, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_progress() {
        assert_eq!(parse_progress_line("正在下载 42%"), Some(42));
        assert_eq!(parse_progress_line("已成功安装"), None);
        assert_eq!(parse_progress_line("  7%  [                   ]"), Some(7));
        assert_eq!(parse_progress_line("100% ✓"), Some(100));
    }

    #[test]
    fn parse_table_basic() {
        let out = "\
Name                Id                   Version  Available  Source
-------------------------------------------------------------------
Microsoft Edge      Microsoft.Edge       120.0    121.0      winget
7-Zip               7zip.7zip            23.01    -          winget
";
        let pkgs = parse_winget_table(out);
        assert_eq!(pkgs.len(), 2);
        assert_eq!(pkgs[0].id, "Microsoft.Edge");
        assert_eq!(pkgs[0].version, "120.0");
        assert_eq!(pkgs[0].available, "121.0");
        assert_eq!(pkgs[1].available, "");
        assert_eq!(pkgs[1].source, "winget");
    }

    #[test]
    fn parse_table_empty() {
        assert!(parse_winget_table("no table here").is_empty());
    }
}
