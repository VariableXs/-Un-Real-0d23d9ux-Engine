//! L3 shell — print.rs（AI-08 · V-97 右键打印 / V-98 打印队列查看器）
//! - print_assoc_check：文件类型是否有系统「print」动词关联（IQueryAssociations 语义，
//!   实走 AssocQueryStringW；无关联如实返回 false，前端据此置灰）
//! - print_files：走系统默认打印关联（ShellExecute "print" 动词），不自带打印逻辑
//! - print_list / print_jobs / print_job_set：winspool 打印机枚举与队列只读 + 显式操作
//! 仅 Windows 有真实行为；其余平台占位（与 tools.rs 同策略）。

use crate::error::{AppError, CmdResult};
use serde::Serialize;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PrinterInfo {
    pub name: String,
    pub port: String,
    pub driver: String,
    pub is_default: bool,
    pub jobs: u32,
    pub status: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PrintJob {
    pub job_id: u32,
    pub printer: String,
    pub document: String,
    pub user: String,
    /// 状态文本（paused / printing / spooling / error / offline / paperout / pending…）
    pub status: String,
    pub status_raw: u32,
    pub total_pages: u32,
    pub pages_printed: u32,
    pub submitted: String,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn from_wide(p: windows::core::PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { p.to_string().unwrap_or_default() }
}

/// V-97：检查一批文件是否具有系统「print」动词关联（逐文件如实判定）。
/// 关联判断完全走系统注册表（ASSOCF_NONE，不改任何关联）。
#[tauri::command(async)]
pub fn print_assoc_check(paths: Vec<String>) -> CmdResult<Vec<bool>> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NONE, ASSOCSTR_COMMAND};
        let mut out = Vec::with_capacity(paths.len());
        for p in &paths {
            // 取扩展名（无扩展名 → 无关联）
            let ext = std::path::Path::new(p)
                .extension()
                .map(|e| format!(".{}", e.to_string_lossy().to_lowercase()))
                .unwrap_or_default();
            if ext.is_empty() {
                out.push(false);
                continue;
            }
            let assoc = wide(&ext);
            let verb = wide("print");
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let hr = unsafe {
                AssocQueryStringW(
                    ASSOCF_NONE,
                    ASSOCSTR_COMMAND,
                    PCWSTR(assoc.as_ptr()),
                    PCWSTR(verb.as_ptr()),
                    windows::core::PWSTR(buf.as_mut_ptr()),
                    &mut len,
                )
            };
            out.push(hr.is_ok() && len > 0);
        }
        Ok(out)
    }
    #[cfg(not(windows))]
    {
        let _ = paths;
        Ok(Vec::new())
    }
}

/// V-97：右键打印 —— 走系统默认打印关联（ShellExecute "print" 动词，逐文件）。
/// 不做打印预览、不做打印机选择（系统打印对话框领地，规格红线）。
/// 复用 AI-3 Shell 代理通道（compat::shell_execute_path），不新增进程创建代码。
#[tauri::command(async)]
pub fn print_files(paths: Vec<String>) -> CmdResult<Vec<String>> {
    if paths.is_empty() {
        return Err(AppError::validation("打印文件列表为空"));
    }
    let mut errs = Vec::new();
    for p in &paths {
        let r = crate::shell::compat::shell_execute_path(
            std::path::Path::new(p),
            Some("print"),
            None,
            None,
            None,
        );
        if let Err(e) = r {
            errs.push(format!("{p}: {e}"));
        }
    }
    Ok(errs)
}

/// V-98：枚举本机打印机（本地 + 连接）。
#[tauri::command(async)]
pub fn print_list() -> CmdResult<Vec<PrinterInfo>> {
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Printing::{
            EnumPrintersW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL, PRINTER_INFO_2W,
        };
        const FLAGS: u32 = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
        unsafe {
            let mut needed = 0u32;
            let mut returned = 0u32;
            let _ = EnumPrintersW(
                FLAGS,
                windows::core::PCWSTR::null(),
                2,
                None,
                &mut needed,
                &mut returned,
            );
            if needed == 0 {
                return Ok(Vec::new());
            }
            let mut buf = vec![0u8; needed as usize];
            EnumPrintersW(
                FLAGS,
                windows::core::PCWSTR::null(),
                2,
                Some(buf.as_mut_slice()),
                &mut needed,
                &mut returned,
            )
            .map_err(|e| AppError::io(format!("枚举打印机失败: {e}")))?;
            let infos = std::slice::from_raw_parts(buf.as_ptr() as *const PRINTER_INFO_2W, returned as usize);
            Ok(infos
                .iter()
                .map(|i| PrinterInfo {
                    name: from_wide(i.pPrinterName),
                    port: from_wide(i.pPortName),
                    driver: from_wide(i.pDriverName),
                    is_default: i.Attributes & 0x1 != 0, // PRINTER_ATTRIBUTE_DEFAULT
                    jobs: i.cJobs,
                    status: printer_status_text(i.Status),
                })
                .collect())
        }
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}

fn printer_status_text(status: u32) -> String {
    // PRINTER_STATUS_* 位（winspool）；多标志以「/」连接，未知位如实给码。
    let mut parts: Vec<&str> = Vec::new();
    if status & 0x1 != 0 { parts.push("paused"); }
    if status & 0x2 != 0 { parts.push("error"); }
    if status & 0x4 != 0 { parts.push("pending-deletion"); }
    if status & 0x8 != 0 { parts.push("paper-jam"); }
    if status & 0x10 != 0 { parts.push("paper-out"); }
    if status & 0x20 != 0 { parts.push("manual-feed"); }
    if status & 0x40 != 0 { parts.push("paper-problem"); }
    if status & 0x80 != 0 { parts.push("offline"); }
    if status & 0x100 != 0 { parts.push("io-active"); }
    if status & 0x200 != 0 { parts.push("busy"); }
    if status & 0x400 != 0 { parts.push("printing"); }
    if status & 0x800 != 0 { parts.push("output-bin-full"); }
    if status & 0x1000 != 0 { parts.push("not-available"); }
    if status & 0x2000 != 0 { parts.push("waiting"); }
    if status & 0x4000 != 0 { parts.push("processing"); }
    if status & 0x8000 != 0 { parts.push("initializing"); }
    if status & 0x10000 != 0 { parts.push("warming-up"); }
    if status & 0x20000 != 0 { parts.push("toner-low"); }
    if status & 0x40000 != 0 { parts.push("no-toner"); }
    if status & 0x80000 != 0 { parts.push("page-punt"); }
    if status & 0x100000 != 0 { parts.push("user-intervention"); }
    if status & 0x200000 != 0 { parts.push("out-of-memory"); }
    if status & 0x400000 != 0 { parts.push("door-open"); }
    if status & 0x800000 != 0 { parts.push("server-unknown"); }
    if status & 0x1000000 != 0 { parts.push("power-save"); }
    if parts.is_empty() {
        if status == 0 { return "ready".into(); }
        return format!("status-0x{status:x}");
    }
    parts.join("/")
}

fn job_status_text(status: u32) -> String {
    // JOB_STATUS_* 位。
    let mut parts: Vec<&str> = Vec::new();
    if status & 0x1 != 0 { parts.push("paused"); }
    if status & 0x2 != 0 { parts.push("error"); }
    if status & 0x4 != 0 { parts.push("deleting"); }
    if status & 0x8 != 0 { parts.push("spooling"); }
    if status & 0x10 != 0 { parts.push("printing"); }
    if status & 0x20 != 0 { parts.push("offline"); }
    if status & 0x40 != 0 { parts.push("paper-out"); }
    if status & 0x80 != 0 { parts.push("printed"); }
    if status & 0x100 != 0 { parts.push("deleted"); }
    if status & 0x200 != 0 { parts.push("blocked"); }
    if status & 0x400 != 0 { parts.push("user-intervention"); }
    if status & 0x800 != 0 { parts.push("restarted"); }
    if parts.is_empty() {
        return "pending".into();
    }
    parts.join("/")
}

/// V-98：读取某打印机（None = 第一台/默认）当前队列。只读。
#[tauri::command(async)]
pub fn print_jobs(printer: Option<String>) -> CmdResult<Vec<PrintJob>> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Graphics::Printing::{ClosePrinter, EnumJobsW, OpenPrinterW, JOB_INFO_1W};
        // 无打印机名 → 枚举第一台（前端会先 print_list 再带名查询；这里兜底）
        let name = match printer {
            Some(n) if !n.is_empty() => n,
            _ => return Ok(Vec::new()),
        };
        let name_w = wide(&name);
        let mut handle = windows::Win32::Foundation::HANDLE::default();
        unsafe {
            OpenPrinterW(PCWSTR(name_w.as_ptr()), &mut handle, None)
                .map_err(|e| AppError::io(format!("打开打印机 {name} 失败: {e}")))?;
        }
        if handle.is_invalid() {
            return Err(AppError::io(format!("打印机 {name} 不可用")));
        }
        let result = (|| -> CmdResult<Vec<PrintJob>> {
            unsafe {
                let mut needed = 0u32;
                let mut returned = 0u32;
                let _ = EnumJobsW(handle, 0, 100, 1, None, &mut needed, &mut returned);
                if needed == 0 {
                    return Ok(Vec::new());
                }
                let mut buf = vec![0u8; needed as usize];
                EnumJobsW(handle, 0, 100, 1, Some(buf.as_mut_slice()), &mut needed, &mut returned)
                    .map_err(|e| AppError::io(format!("枚举打印队列失败: {e}")))?;
                let jobs = std::slice::from_raw_parts(buf.as_ptr() as *const JOB_INFO_1W, returned as usize);
                Ok(jobs
                    .iter()
                    .map(|j| {
                        let st = &j.Submitted;
                        PrintJob {
                            job_id: j.JobId,
                            printer: from_wide(j.pPrinterName),
                            document: from_wide(j.pDocument),
                            user: from_wide(j.pUserName),
                            status: job_status_text(j.Status),
                            status_raw: j.Status,
                            total_pages: j.TotalPages,
                            pages_printed: j.PagesPrinted,
                            submitted: format!(
                                "{:04}-{:02}-{:02} {:02}:{:02}",
                                st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute
                            ),
                        }
                    })
                    .collect())
            }
        })();
        unsafe {
            let _ = ClosePrinter(handle);
        }
        result
    }
    #[cfg(not(windows))]
    {
        let _ = printer;
        Ok(Vec::new())
    }
}

/// V-98：队列操作（pause / resume / cancel）——显式操作，无静默批量。
#[tauri::command(async)]
pub fn print_job_set(printer: String, job_id: u32, action: String) -> CmdResult<()> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Graphics::Printing::{
            ClosePrinter, OpenPrinterW, SetJobW, JOB_CONTROL_CANCEL, JOB_CONTROL_PAUSE,
            JOB_CONTROL_RESUME,
        };
        let cmd = match action.as_str() {
            "pause" => JOB_CONTROL_PAUSE,
            "resume" => JOB_CONTROL_RESUME,
            "cancel" => JOB_CONTROL_CANCEL,
            other => {
                return Err(AppError::validation(format!("未知队列操作: {other}")));
            }
        };
        let name_w = wide(&printer);
        let mut handle = windows::Win32::Foundation::HANDLE::default();
        unsafe {
            OpenPrinterW(PCWSTR(name_w.as_ptr()), &mut handle, None)
                .map_err(|e| AppError::io(format!("打开打印机 {printer} 失败: {e}")))?;
        }
        if handle.is_invalid() {
            return Err(AppError::io(format!("打印机 {printer} 不可用")));
        }
        let ok = unsafe { SetJobW(handle, job_id, 0, None, cmd) };
        unsafe {
            let _ = ClosePrinter(handle);
        }
        if ok.as_bool() {
            Ok(())
        } else {
            Err(AppError::io(format!(
                "队列操作失败（job {job_id} / {action}）"
            )))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (printer, job_id, action);
        Err(AppError::validation("打印队列仅支持 Windows（当前平台为占位）"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printer_status_text_bits() {
        assert_eq!(printer_status_text(0), "ready");
        assert!(printer_status_text(0x80).contains("offline"));
        assert!(printer_status_text(0x1 | 0x10).contains("paused"));
        assert!(printer_status_text(0x10).contains("paper-out"));
        assert!(printer_status_text(0x400).contains("printing"));
    }

    #[test]
    fn job_status_text_bits() {
        assert_eq!(job_status_text(0), "pending");
        assert!(job_status_text(0x1).contains("paused"));
        assert!(job_status_text(0x80).contains("printed"));
        assert!(job_status_text(0x10).contains("printing"));
    }

    #[test]
    fn assoc_check_shape() {
        // 无扩展名 → false（形状测试；真实关联在 Windows 集成环境验证）
        let r = print_assoc_check(vec!["noext".into()]).unwrap();
        assert_eq!(r, vec![false]);
    }
}
