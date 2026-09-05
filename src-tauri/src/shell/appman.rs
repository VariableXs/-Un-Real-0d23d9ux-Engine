//! L3 shell — 软件运行检测与卸载（批次C / 规格 5.5-5.6）：
//! - tp_running   已登记第三方软件的运行状态（进程镜像路径匹配，Toolhelp32 快照）
//! - official_usage  预装四软件的数据占用（数据库行级统计，诚实边界：workspace
//!   文件属用户工作区，不计入单一软件名下）
//! - official_purge  彻底删除某预装软件的数据库数据（write=文章，mindmap=导图；
//!   code/fate 的档案在工作区，无独立库数据可清）
//! - 零网络 / 零遥测：一切仅本机读写

use serde::Serialize;

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

/// 预装四软件（与前端 AppMode 对应）。
const OFFICIAL_APPS: &[&str] = &["write", "mindmap", "project", "fate"];

fn valid_official(app: &str) -> bool {
    OFFICIAL_APPS.contains(&app)
}

// ---------- 运行状态匹配（纯函数，可测试） ----------

fn norm_path(p: &str) -> String {
    p.replace('/', "\\").to_lowercase()
}

/// 判定某个进程是否对应一个登记项：
/// - 完整镜像路径可查询（OpenProcess 成功）→ 以路径为准，名字相同路径不同不算
/// - 查询不到路径（系统进程等）→ 回退 exe 文件名匹配
fn process_matches(app_path: &str, proc_image: Option<&str>, proc_name: &str) -> bool {
    let target = norm_path(app_path);
    match proc_image {
        Some(img) => norm_path(img) == target,
        None => {
            let exe_name = target.rsplit('\\').next().unwrap_or(&target);
            proc_name.to_lowercase() == exe_name
        }
    }
}

// ---------- 命令：第三方软件运行检测 ----------

/// 返回当前正在运行的已登记第三方软件 id 列表。
#[tauri::command]
pub fn tp_running(st: tauri::State<AppState>) -> CmdResult<Vec<String>> {
    let registry = crate::shell::launcher::registry_snapshot(&st);
    if registry.is_empty() {
        return Ok(Vec::new());
    }
    let procs = enum_processes();
    let running: Vec<String> = registry
        .into_iter()
        .filter(|a| {
            // 批次E（规格 5.9.4 深度检测）：.lnk 登记项按解析出的目标 exe 匹配，
            // 不再误报同名进程；主路径与目标任一命中即视为运行中。
            let hit_path = procs.iter().any(|(name, image)| process_matches(&a.path, image.as_deref(), name));
            let hit_target = a.target.as_deref().is_some_and(|t| {
                procs.iter().any(|(name, image)| process_matches(t, image.as_deref(), name))
            });
            hit_path || hit_target
        })
        .map(|a| a.id)
        .collect();
    Ok(running)
}

/// 枚举系统进程：(exe 文件名, 完整镜像路径（可查询时）)。
#[cfg(windows)]
fn enum_processes() -> Vec<(String, Option<String>)> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let mut out = Vec::new();
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return out,
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let name_vec: Vec<u16> = entry
                    .szExeFile
                    .iter()
                    .take_while(|&&c| c != 0)
                    .copied()
                    .collect();
                let name = String::from_utf16_lossy(&name_vec);
                // 完整路径尽力查询（系统进程/权限不足 → None，回退按名匹配）
                let image = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, entry.th32ProcessID)
                    .ok()
                    .and_then(|h| {
                        let mut len: u32 = 1024;
                        let mut buf = [0u16; 1024];
                        let ok = QueryFullProcessImageNameW(
                            h,
                            PROCESS_NAME_WIN32,
                            PWSTR(buf.as_mut_ptr()),
                            &mut len,
                        )
                        .is_ok();
                        let _ = CloseHandle(h);
                        if ok && len > 0 {
                            Some(String::from_utf16_lossy(&buf[..len as usize]))
                        } else {
                            None
                        }
                    });
                out.push((name, image));
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
fn enum_processes() -> Vec<(String, Option<String>)> {
    Vec::new()
}

// ---------- 命令：预装软件数据占用 / 彻底删除 ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialUsage {
    /// 主条目数（write=文章数，mindmap=导图数，其余 0）
    pub items: u64,
    /// 估算数据量（字节；None = 数据以文件形式存于工作区，无法按软件切分）
    pub bytes: Option<u64>,
    /// 是否支持"彻底删除"数据库数据
    pub purgeable: bool,
}

/// 预装软件数据占用（诚实边界：workspace 用户文件不按软件切分）。
#[tauri::command]
pub fn official_usage(st: tauri::State<AppState>, app: String) -> CmdResult<OfficialUsage> {
    if !valid_official(&app) {
        return Err(AppError::validation(format!("未知软件 / Unknown app: {app}")));
    }
    let usage = match app.as_str() {
        "write" => st.with_conn(|conn| {
            let items: u64 = conn
                .query_row("SELECT COUNT(*) FROM documents", [], |r| r.get::<_, i64>(0))
                .map_err(|e| AppError::db(format!("查询失败 / Query failed: {e}")))? as u64;
            let bytes: u64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(LENGTH(title) + LENGTH(content_html) + LENGTH(content_text)), 0) FROM documents",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(|e| AppError::db(format!("查询失败 / Query failed: {e}")))? as u64;
            Ok(OfficialUsage { items, bytes: Some(bytes), purgeable: true })
        })?,
        "mindmap" => st.with_conn(|conn| {
            let items: u64 = conn
                .query_row("SELECT COUNT(*) FROM mindmaps", [], |r| r.get::<_, i64>(0))
                .map_err(|e| AppError::db(format!("查询失败 / Query failed: {e}")))? as u64;
            let bytes: u64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(LENGTH(text_plain) + LENGTH(text_html)), 0) FROM nodes",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(|e| AppError::db(format!("查询失败 / Query failed: {e}")))? as u64;
            Ok(OfficialUsage { items, bytes: Some(bytes), purgeable: true })
        })?,
        // code / fate：档案以文件形式存于用户工作区（不按软件切分、不自动删除）
        _ => OfficialUsage { items: 0, bytes: None, purgeable: false },
    };
    Ok(usage)
}

/// 彻底删除某预装软件的数据库数据（二次确认由前端负责）。
#[tauri::command]
pub fn official_purge(st: tauri::State<AppState>, app: String) -> CmdResult<()> {
    match app.as_str() {
        "write" => st.with_conn(|conn| {
            conn.execute_batch(
                "DELETE FROM document_tags;
                 DELETE FROM attachments WHERE document_id IS NOT NULL;
                 DELETE FROM documents;",
            )
            .map_err(|e| AppError::db(format!("清除失败 / Purge failed: {e}")))?;
            Ok(())
        }),
        "mindmap" => st.with_conn(|conn| {
            conn.execute_batch("DELETE FROM edges; DELETE FROM nodes; DELETE FROM mindmaps;")
                .map_err(|e| AppError::db(format!("清除失败 / Purge failed: {e}")))?;
            Ok(())
        }),
        _ => Err(AppError::validation(
            "该软件的数据以文件形式保存在工作区，不提供数据库级彻底删除 / This app keeps data as workspace files; no database purge available",
        )),
    }
}

// ---------- 测试 ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_matching_paths_and_fallback() {
        // 完整路径匹配（大小写/斜杠方向不敏感）
        assert!(process_matches(
            "C:\\Program Files\\App\\foo.exe",
            Some("c:/program files/app/FOO.exe"),
            "whatever.exe"
        ));
        // 路径不同 → 不匹配
        assert!(!process_matches(
            "C:\\Program Files\\App\\foo.exe",
            Some("C:\\Program Files\\App\\bar.exe"),
            "bar.exe"
        ));
        // 查询不到路径 → 按进程名回退匹配
        assert!(process_matches("D:\\Tools\\myapp.exe", None, "MYAPP.EXE"));
        // 名字相同但路径不同的进程不算（路径可得时以路径为准）
        assert!(!process_matches(
            "D:\\Tools\\myapp.exe",
            Some("C:\\Other\\myapp.exe"),
            "myapp.exe"
        ));
    }

    #[test]
    fn official_app_validation() {
        assert!(valid_official("write"));
        assert!(valid_official("fate"));
        assert!(!valid_official("evil"));
        assert!(!valid_official(""));
    }
}
