//! L3 shell — 终端服务（批次 B-7，BLUEPRINT 3.7）：
//! V1 = 嵌入 Windows Terminal Portable（复用 embed 通道，零渲染代码）。
//! 终端以第三方登记项形式注册（执行档：VARIABLE_ENV=terminal），与四软件平级；
//! V2 自绘 xterm.js + ConPTY 会话保活为后续批次（本模块预留 session 登记位）。
//!
//! 部署约定（零网络默认）：<dataDir>/runtime/wt/WindowsTerminal.exe
//! 由用户从 Windows Terminal Portable 放入（或经 B-8 通道授权下载后解压）。

use serde::Serialize;
use std::path::PathBuf;

use crate::error::{AppError, CmdResult};
use crate::state::AppState;

pub const TERMINAL_ID: &str = "variable-terminal";
pub const TERMINAL_NAME: &str = "Variable Terminal";

/// 便携 Windows Terminal 的容器内部署位置。
pub fn wt_exe(st: &AppState) -> PathBuf {
    st.data_dir.join("runtime").join("wt").join("WindowsTerminal.exe")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStatus {
    deployed: bool,
    path: Option<String>,
    registered: bool,
}

/// 终端就绪状态（AI Hub / 任务栏提示用）。
#[tauri::command(async)]
pub fn term_status(st: tauri::State<AppState>) -> CmdResult<TerminalStatus> {
    let exe = wt_exe(&st);
    let deployed = exe.is_file();
    let registered = crate::shell::launcher::load_registry(&st)
        .iter()
        .any(|a| a.id == TERMINAL_ID);
    Ok(TerminalStatus {
        deployed,
        path: deployed.then(|| exe.to_string_lossy().into_owned()),
        registered,
    })
}

/// 确保「Variable Terminal」已登记为第三方启动项（幂等）。
/// 登记后前端走 launchThirdApp 既有链路：VWM 占位窗口 + embed 嵌入。
pub fn ensure_terminal_registered(st: &AppState) -> CmdResult<()> {
    let exe = wt_exe(&st);
    if !exe.is_file() {
        return Err(AppError::not_found(
            "未找到便携 Windows Terminal：请将 WindowsTerminal.exe 放入 runtime/wt/（或先在 AI Hub 安装）",
        ));
    }
    let mut apps = crate::shell::launcher::load_registry(st);
    if apps.iter().any(|a| a.id == TERMINAL_ID) {
        // 已登记：路径漂移时纠正
        if let Some(slot) = apps.iter_mut().find(|a| a.id == TERMINAL_ID) {
            let cur = exe.to_string_lossy().into_owned();
            if slot.path != cur {
                slot.path = cur;
                crate::shell::launcher::save_registry(st, &apps)?;
            }
        }
        return Ok(());
    }
    apps.push(crate::shell::launcher::ThirdApp {
        dpi_fix: false,
        compat: Default::default(),
        id: TERMINAL_ID.to_string(),
        name: TERMINAL_NAME.to_string(),
        path: exe.to_string_lossy().into_owned(),
        grade: crate::shell::launcher::GRADE_PORTABLE.to_string(),
        added_at: 0,
        last_launch: None,
        icon: None,
        target: None,
        profile: crate::exec::PortableProfile {
            env_set: [("VARIABLE_ENV".to_string(), "terminal".to_string())].into_iter().collect(),
            ..Default::default()
        },
    });
    crate::shell::launcher::save_registry(st, &apps)
}

/// 打开终端（幂等登记 + 返回登记项 id，前端走 embed 通道嵌入 VWM）。
#[tauri::command(async)]
pub fn term_open(st: tauri::State<AppState>) -> CmdResult<String> {
    ensure_terminal_registered(&st)?;
    Ok(TERMINAL_ID.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn term_paths_under_runtime() {
        let st = crate::state::AppState::bootstrap_dirs_at(std::env::temp_dir().join(format!(
            "term-test-{}",
            std::process::id()
        )))
        .unwrap();
        assert!(wt_exe(&st).starts_with(st.data_dir.join("runtime").join("wt")));
    }

    #[test]
    fn ensure_register_fails_honestly_without_wt() {
        let st = crate::state::AppState::bootstrap_dirs_at(std::env::temp_dir().join(format!(
            "term-test2-{}",
            std::process::id()
        )))
        .unwrap();
        let err = ensure_terminal_registered(&st);
        assert!(err.is_err(), "WT 未部署时必须如实报错");
    }
}