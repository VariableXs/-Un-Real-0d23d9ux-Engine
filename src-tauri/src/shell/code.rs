//! VS Code Portable 一键部署（B-20，BLUEPRINT 3.7 / M5）。
//!
//! - 部署：下载 win32-x64 系统 zip（curl 真实字节进度事件，出站走 netconsent
//!   授权口径，与 B-8 Node 部署同款）→ Expand-Archive → runtime/vscode/ →
//!   创建 `data/` 目录进入 **Portable 模式**（user-data/extensions 全在容器）；
//! - 登记：自动 upsert ThirdApp id="vscode"（执行档：HOME/USERPROFILE 镜像容器）；
//! - 启动：复用 embed_launch 整条嵌入通道——Electron 窗口嵌入与既有第三方
//!   完全同路径，回归安全性由"零新增嵌入代码"保证；
//! - 幂等：已部署/已登记时直接返回。
//!
//! 如实边界：下载依赖 update.code.visualstudio.com 可达（离线宿主提示用户
//! 手动放置 zip 到 runtime/vscode-download.zip，检测到即跳过下载）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tauri::Emitter;

use crate::error::AppError;
use crate::state::AppState;

type CmdResult<T> = Result<T, AppError>;

pub const VSCODE_ID: &str = "vscode";

fn vscode_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("vscode")
}

fn code_exe(data_dir: &Path) -> PathBuf {
    vscode_dir(data_dir).join("Code.exe")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeStatus {
    pub deployed: bool,
    pub exe: String,
    pub registered: bool,
    pub portable_data: bool,
}

#[tauri::command(async)]
pub fn code_status(st: tauri::State<AppState>) -> CmdResult<CodeStatus> {
    let exe = code_exe(&st.data_dir);
    let registered = crate::shell::launcher::registry_snapshot(&st)
        .iter()
        .any(|a| a.id == VSCODE_ID);
    Ok(CodeStatus {
        deployed: exe.is_file(),
        exe: exe.to_string_lossy().into_owned(),
        registered,
        portable_data: vscode_dir(&st.data_dir).join("data").is_dir(),
    })
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CodeProgress {
    phase: String,
    done: u64,
    total: u64,
    message: String,
}

fn hidden_command(program: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new(program);
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    }
    #[cfg(not(windows))]
    std::process::Command::new(program)
}

/// 一键部署（幂等）。zip 已存在于 runtime/vscode-download.zip 时跳过下载。
#[tauri::command(async)]
pub fn code_deploy(st: tauri::State<AppState>, app: tauri::AppHandle) -> CmdResult<()> {
    if code_exe(&st.data_dir).is_file() {
        return Ok(()); // 幂等
    }
    let data_dir = st.data_dir.clone();
    std::thread::spawn(move || {
        let step = |phase: &str, done: u64, total: u64, msg: &str| {
            let _ = app.emit(
                "code://progress",
                CodeProgress {
                    phase: phase.into(),
                    done,
                    total,
                    message: msg.into(),
                },
            );
        };
        let result = (|| -> CmdResult<()> {
            let target = vscode_dir(&data_dir);
            let _ = std::fs::remove_dir_all(&target);
            let stage = data_dir.join("runtime").join("_vscode-stage");
            let _ = std::fs::remove_dir_all(&stage);
            std::fs::create_dir_all(&stage).map_err(|e| AppError::io(e.to_string()))?;

            let tmp_zip = data_dir.join("runtime").join("vscode-download.zip");
            let local_zip = data_dir.join("runtime").join("vscode-download.zip");
            if !local_zip.is_file() && !tmp_zip.is_file() {
                let url = "https://update.code.visualstudio.com/latest/win32-x64/stable";
                step("code-download", 0, 0, url);
                let mut child = hidden_command("curl.exe")
                    .args(["-fL", "-o"])
                    .arg(&tmp_zip)
                    .arg(url)
                    .spawn()
                    .map_err(|e| AppError::io(format!("下载启动失败: {e}")))?;
                loop {
                    match child.try_wait() {
                        Ok(Some(code)) => {
                            if !code.success() {
                                return Err(AppError::io(format!("下载失败（curl exit {code}）")));
                            }
                            break;
                        }
                        Ok(None) => {
                            let done =
                                std::fs::metadata(&tmp_zip).map(|m| m.len()).unwrap_or(0);
                            step("code-download", done, 0, "win32-x64 stable");
                            std::thread::sleep(Duration::from_millis(500));
                        }
                        Err(e) => return Err(AppError::io(format!("下载中断: {e}"))),
                    }
                }
            }
            let zip = if tmp_zip.is_file() { tmp_zip } else { local_zip };
            let total = std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0);
            step("code-extract", 0, total, "Expand-Archive");
            let ps = format!(
                "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                zip.display(),
                stage.display()
            );
            let out = hidden_command("powershell")
                .args(["-NoProfile", "-Command", &ps])
                .output()
                .map_err(|e| AppError::io(format!("解压失败: {e}")))?;
            if !out.status.success() {
                return Err(AppError::io(format!(
                    "解压失败: {}",
                    String::from_utf8_lossy(&out.stderr)
                )));
            }
            // 内层目录（VSCode-win32-x64-*）或直接解出 Code.exe
            let inner = if stage.join("Code.exe").is_file() {
                stage.clone()
            } else {
                std::fs::read_dir(&stage)?
                    .filter_map(|e| e.ok())
                    .find(|e| e.path().join("Code.exe").is_file())
                    .map(|e| e.path())
                    .ok_or_else(|| AppError::io("解压产物缺少 Code.exe"))?
            };
            std::fs::rename(&inner, &target).map_err(|e| AppError::io(e.to_string()))?;
            let _ = std::fs::remove_dir_all(&stage);
            let _ = std::fs::remove_file(&zip);
            // Portable 模式：data/ 目录存在即全部用户数据落在容器
            std::fs::create_dir_all(target.join("data")).map_err(|e| AppError::io(e.to_string()))?;
            step("done", 1, 1, "VS Code Portable 就绪");
            Ok(())
        })();
        if let Err(e) = result {
            step("error", 0, 0, &e.to_string());
        }
    });
    Ok(())
}

/// 自动登记 ThirdApp（幂等 upsert）——登记后即出现在任务栏/启动器/嵌入通道。
#[tauri::command(async)]
pub fn code_register(st: tauri::State<AppState>) -> CmdResult<()> {
    let exe = code_exe(&st.data_dir);
    if !exe.is_file() {
        return Err(AppError::not_found("VS Code 尚未部署（先执行一键部署）"));
    }
    let mut apps = crate::shell::launcher::load_registry(&st);
    if let Some(existing) = apps.iter_mut().find(|a| a.id == VSCODE_ID) {
        existing.path = exe.to_string_lossy().into_owned();
        existing.grade = "portable".into();
    } else {
        apps.push(crate::shell::launcher::ThirdApp {
            id: VSCODE_ID.into(),
            name: "VS Code".into(),
            path: exe.to_string_lossy().into_owned(),
            grade: "portable".into(),
            added_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            last_launch: None,
            icon: None,
            target: None,
            profile: crate::exec::PortableProfile {
                env_redirect: [
                    ("HOME".to_string(), "{envhome}".to_string()),
                    ("USERPROFILE".to_string(), "{envhome}".to_string()),
                ]
                .into_iter()
                .collect(),
                env_set: [("VARIABLE_APP".to_string(), VSCODE_ID.to_string())]
                    .into_iter()
                    .collect(),
                net_allow: Vec::new(),
                sensitive: false,
            },
            dpi_fix: false,
            compat: Default::default(),
        });
    }
    crate::shell::launcher::save_registry(&st, &apps)
}

/// 启动并嵌入（复用 embed_launch 整条通道——Electron 嵌入回归由此保证）。
#[tauri::command(async)]
pub async fn code_launch(
    st: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> CmdResult<crate::shell::embed::EmbedResult> {
    let registered = crate::shell::launcher::registry_snapshot(&st)
        .iter()
        .any(|a| a.id == VSCODE_ID);
    if !registered {
        code_register(st.clone())?;
    }
    // W-1：缺省 embed_id → "0" 兼容槽位（设置卡直启无 VWM 占位窗口；
    // 经启动器 tp:vscode 打开时走 launchThirdApp 的独立 embed_id）
    crate::shell::embed::embed_launch(st, app, VSCODE_ID.into(), None, None).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_layout_uses_data_dir() {
        // Portable 模式契约：Code.exe 同级的 data/ 目录即用户数据根
        let dir = std::env::temp_dir().join(format!("code-b20-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).unwrap();
        assert!(dir.join("data").is_dir());
        assert_eq!(code_exe(&dir.parent().unwrap().join(dir.file_name().unwrap())), code_exe(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_is_honest_when_absent() {
        // 未部署时 status 不撒谎（deployed=false）
        let missing = std::env::temp_dir().join(format!("code-b20-none-{}", std::process::id()));
        let exe = code_exe(&missing);
        assert!(!exe.is_file());
    }
}
