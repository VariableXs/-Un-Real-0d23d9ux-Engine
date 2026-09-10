//! 工具链便携部署（B-21，BLUEPRINT 3.7 runtime/）——Python / Go / Rust。
//!
//! - Python：官方 embeddable zip（免安装），runtime/python/；
//! - Go：官方 zip，runtime/go/（bin 在 go/bin）；
//! - Rust：rustup-init.exe -y --no-modify-path，CARGO_HOME/RUSTUP_HOME 指向容器
//!   runtime/cargo、runtime/rustup（绝不碰宿主 PATH/用户目录）；
//! - PATH 注入由 exec::spawn_profiled 统一处理（runtime_path_prefix，B-21 冻结顺序）；
//! - 出站：python.org / go.dev / static.rust-lang.org 走 netconsent 授权口径；
//! - 离线：runtime/<id>-download.(zip|exe) 存在即跳过下载。
//!
//! 版本锁：见 locks/toolchains.md（升版需同步改本文件 URL 与锁文件）。

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::error::AppError;
use crate::state::AppState;

type CmdResult<T> = Result<T, AppError>;

const PY_URL: &str = "https://www.python.org/ftp/python/3.12.7/python-3.12.7-embed-amd64.zip";
const GO_URL: &str = "https://go.dev/dl/go1.23.2.windows-amd64.zip";
const RUSTUP_URL: &str = "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolchainStatus {
    pub id: String,
    pub deployed: bool,
    pub home: String,
}

fn py_dir(d: &Path) -> PathBuf { d.join("runtime").join("python") }
fn go_dir(d: &Path) -> PathBuf { d.join("runtime").join("go") }
fn cargo_dir(d: &Path) -> PathBuf { d.join("runtime").join("cargo") }
fn rustup_dir(d: &Path) -> PathBuf { d.join("runtime").join("rustup") }

#[tauri::command(async)]
pub fn toolchain_status(st: tauri::State<AppState>) -> CmdResult<Vec<ToolchainStatus>> {
    let d = &st.data_dir;
    Ok(vec![
        ToolchainStatus { id: "node".into(), deployed: d.join("runtime/node/node.exe").is_file(), home: d.join("runtime/node").to_string_lossy().into_owned() },
        ToolchainStatus { id: "python".into(), deployed: py_dir(d).join("python.exe").is_file(), home: py_dir(d).to_string_lossy().into_owned() },
        ToolchainStatus { id: "go".into(), deployed: go_dir(d).join("bin/go.exe").is_file(), home: go_dir(d).to_string_lossy().into_owned() },
        ToolchainStatus { id: "rust".into(), deployed: cargo_dir(d).join("bin/cargo.exe").is_file(), home: cargo_dir(d).to_string_lossy().into_owned() },
    ])
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TcProgress {
    id: String,
    phase: String,
    done: u64,
    total: u64,
    message: String,
}

fn hidden(program: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new(program);
        c.creation_flags(0x0800_0000);
        c
    }
    #[cfg(not(windows))]
    std::process::Command::new(program)
}

fn download(url: &str, dest: &Path, step: &dyn Fn(u64, u64, &str)) -> CmdResult<()> {
    let mut child = hidden("curl.exe")
        .args(["-fL", "-o"])
        .arg(dest)
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
                let done = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
                step(done, 0, url);
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(e) => return Err(AppError::io(format!("下载中断: {e}"))),
        }
    }
    Ok(())
}

fn extract_zip(zip: &Path, stage: &Path) -> CmdResult<()> {
    let ps = format!(
        "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
        zip.display(),
        stage.display()
    );
    let out = hidden("powershell")
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .map_err(|e| AppError::io(format!("解压失败: {e}")))?;
    if !out.status.success() {
        return Err(AppError::io(format!(
            "解压失败: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(())
}

#[tauri::command(async)]
pub fn toolchain_deploy(
    st: tauri::State<AppState>,
    app: tauri::AppHandle,
    id: String,
) -> CmdResult<()> {
    use tauri::Emitter;
    let d = st.data_dir.clone();
    let step = move |tc: &str, phase: &str, done: u64, total: u64, msg: &str| {
        let _ = app.emit(
            "toolchain://progress",
            TcProgress { id: tc.into(), phase: phase.into(), done, total, message: msg.into() },
        );
    };
    let data_dir = d.clone();
    let id2 = id.clone();
    std::thread::spawn(move || {
        let result = (|| -> CmdResult<()> {
            let runtime = data_dir.join("runtime");
            std::fs::create_dir_all(&runtime).map_err(|e| AppError::io(e.to_string()))?;
            match id2.as_str() {
                "python" => {
                    let target = py_dir(&data_dir);
                    if target.join("python.exe").is_file() {
                        return Ok(()); // 幂等
                    }
                    let zip = runtime.join("python-download.zip");
                    if !zip.is_file() {
                        step("python", "download", 0, 0, PY_URL);
                        download(PY_URL, &zip, &|a, b, m| step("python", "download", a, b, m))?;
                    }
                    step("python", "extract", 0, 0, "Expand-Archive");
                    extract_zip(&zip, &target)?;
                    // embeddable python 默认禁用 site-packages：解禁以支持 pip
                    let pth = target.join("python312._pth");
                    if pth.is_file() {
                        std::fs::write(&pth, b"python312.zip\n.\nLib\\site-packages\nimport site\n")
                            .map_err(|e| AppError::io(e.to_string()))?;
                    }
                    let _ = std::fs::remove_file(&zip);
                    step("python", "done", 1, 1, "Python 就绪（get-pip 可选装）");
                }
                "go" => {
                    let target = go_dir(&data_dir);
                    if target.join("bin").join("go.exe").is_file() {
                        return Ok(());
                    }
                    let zip = runtime.join("go-download.zip");
                    if !zip.is_file() {
                        step("go", "download", 0, 0, GO_URL);
                        download(GO_URL, &zip, &|a, b, m| step("go", "download", a, b, m))?;
                    }
                    let stage = runtime.join("_go-stage");
                    let _ = std::fs::remove_dir_all(&stage);
                    step("go", "extract", 0, 0, "Expand-Archive");
                    extract_zip(&zip, &stage)?;
                    let inner = stage.join("go");
                    std::fs::rename(&inner, &target).map_err(|e| AppError::io(e.to_string()))?;
                    let _ = std::fs::remove_dir_all(&stage);
                    let _ = std::fs::remove_file(&zip);
                    step("go", "done", 1, 1, "Go 就绪");
                }
                "rust" => {
                    let cargo = cargo_dir(&data_dir);
                    let rustup = rustup_dir(&data_dir);
                    if cargo.join("bin").join("cargo.exe").is_file() {
                        return Ok(());
                    }
                    let init = runtime.join("rustup-init.exe");
                    if !init.is_file() {
                        step("rust", "download", 0, 0, RUSTUP_URL);
                        download(RUSTUP_URL, &init, &|a, b, m| step("rust", "download", a, b, m))?;
                    }
                    step("rust", "install", 0, 0, "rustup-init -y --no-modify-path");
                    let out = hidden(init.to_string_lossy().as_ref())
                        .args(["-y", "--no-modify-path", "--default-toolchain", "stable"])
                        .env("CARGO_HOME", &cargo)
                        .env("RUSTUP_HOME", &rustup)
                        .output()
                        .map_err(|e| AppError::io(format!("rustup 启动失败: {e}")))?;
                    if !out.status.success() || !cargo.join("bin").join("cargo.exe").is_file() {
                        return Err(AppError::io(format!(
                            "rustup 安装失败: {}",
                            String::from_utf8_lossy(&out.stderr)
                        )));
                    }
                    let _ = std::fs::remove_file(&init);
                    step("rust", "done", 1, 1, "Rust 就绪（CARGO_HOME 在容器）");
                }
                other => return Err(AppError::validation(format!("未知工具链: {other}"))),
            }
            Ok(())
        })();
        if let Err(e) = result {
            step(&id2, "error", 0, 0, &e.to_string());
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_is_honest_on_empty_dir() {
        let d = std::env::temp_dir().join(format!("tc-b21-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        // 直接断言路径判定逻辑（不构造 State）
        assert!(!py_dir(&d).join("python.exe").is_file());
        assert!(!go_dir(&d).join("bin").join("go.exe").is_file());
        assert!(!cargo_dir(&d).join("bin").join("cargo.exe").is_file());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn toolchain_paths_are_under_runtime() {
        let d = Path::new("C:\\data");
        assert!(py_dir(d).starts_with(d.join("runtime")));
        assert!(cargo_dir(d).starts_with(d.join("runtime")));
        assert!(rustup_dir(d).starts_with(d.join("runtime")));
    }
}
