//! Variable SystemLauncher（L-1）：插上 U 盘双击引导器 → 探测硬件 → 编排 VM → 引擎接管。
//!
//! 四阶段：探测 → 挂载 → 启动 → 接管；每阶段失败给明确文案 + 诊断指引，
//! 从不自动删除任何东西；宿主零注册表写入、零启动项。

#![cfg_attr(not(windows), allow(dead_code))]
#![windows_subsystem = "windows"]

mod probe;
#[cfg(windows)]
mod ui;
mod vmrun;
mod degrade;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use vmrun::LauncherConfig;

#[cfg(windows)]
use ui::{ProgressWindow, SharedUi, UiState};

const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(90);
const EXIT_WAIT: Duration = Duration::from_secs(365 * 24 * 3600);

fn main() {
    #[cfg(not(windows))]
    {
        eprintln!("Variable Launcher 目前仅支持 Windows");
        return;
    }
    #[cfg(windows)]
    {
        run();
    }
}

#[cfg(windows)]
fn run() {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let cfg = LauncherConfig::load(&exe_dir);

    let state: SharedUi = Arc::new(Mutex::new(UiState::default()));
    let win = match ProgressWindow::new(state.clone()) {
        Ok(w) => w,
        Err(_) => {
            // 窗口失败不阻塞启动流程（无头兜底：直接走控制台语义）
            eprintln!("进度窗创建失败，继续无头启动");
            return;
        }
    };

    let ui = UiHandle { state };
    // UI 线程 = 主线程；施工在后台线程
    let ui_t = ui.clone();
    std::thread::spawn(move || {
        if let Err(e) = orchestrate(&exe_dir, &cfg, &ui_t) {
            ui_t.fail(e);
        }
    });
    win.run_loop();
}

#[cfg(windows)]
#[derive(Clone)]
struct UiHandle {
    state: SharedUi,
}

#[cfg(windows)]
impl UiHandle {
    fn stage(&self, i: usize, detail: &str) {
        if let Ok(mut s) = self.state.lock() {
            s.stage = i;
            s.detail = detail.to_string();
            s.error = None;
        }
    }
    fn detail(&self, d: &str) {
        if let Ok(mut s) = self.state.lock() {
            s.detail = d.to_string();
        }
    }
    fn fail(&self, msg: String) {
        if let Ok(mut s) = self.state.lock() {
            s.error = Some(format!(
                "{msg}\n日志: launcher-log.txt（打开诊断目录请查日志，不删除任何文件）"
            ));
        }
    }
}

/// 主编排：返回 Err 时已携带面向用户的文案
#[cfg(windows)]
fn orchestrate(
    exe_dir: &PathBuf,
    cfg: &LauncherConfig,
    ui: &UiHandle,
) -> Result<(), String> {
    // 阶段 0：探测（缓存命中 < 3s）
    ui.stage(0, "读取硬件信息");
    let report = probe::probe_with_cache().map_err(|e| format!("硬件探测失败: {e}\n可直接双击 Variable-OS.vhdx 手动挂载后用 Test-VM.ps1 启动。"))?;
    ui.detail(&format!(
        "{} · {}核 · {}MB 可用 · GPU×{} · 显示器×{}",
        report.os_display,
        report.cpu_cores,
        report.avail_mem_mb,
        report.gpus.len(),
        report.monitors.len()
    ));

    // L-3 重启续跑（优先于启用提示）：pending-state.json（enable-hyperv）
    if let Some(st) = degrade::read_pending_state(exe_dir) {
        if st.action == "enable-hyperv" {
            if st.host_key == report.host_key && report.hyperv_tools {
                degrade::clear_pending_state(exe_dir);
            } else if st.host_key != report.host_key {
                degrade::clear_pending_state(exe_dir); // 换机作废
            } else {
                return Err(
                    "Hyper-V 功能启用尚未完成或需要重启。\n请重启 Windows 后再次双击本引导器（不会自动开机启动，需手动）。"
                        .to_string(),
                );
            }
        }
    }

    // L-3：Hyper-V 未启用 → 弹确认提权启用（用户拒绝则记住选择，直接走降级链；
    // 重启续跑标记由 pending-state.json 承载，绝不写宿主启动项）
    let mut hyperv_ok = report.hyperv_tools;
    if !hyperv_ok {
        let declined = matches!(
            degrade::read_pending_state(exe_dir),
            Some(ref st) if st.action == "hyperv-declined" && st.host_key == report.host_key
        );
        if !declined {
            match degrade::prompt_enable_hyperv(&report.host_key, exe_dir) {
                Ok(true) => {
                    return Err(
                        "已发起 Hyper-V 功能启用。\n请重启 Windows 后再次双击本引导器继续（不会自动开机启动，需手动）。"
                            .to_string(),
                    );
                }
                Ok(false) => {
                    let _ = degrade::write_pending_state(exe_dir, "hyperv-declined", &report.host_key);
                }
                Err(e) => return Err(e),
            }
        }
        hyperv_ok = false;
    }

    // L-3 降级链决策（每档能力诚实清单见 degrade::capability_list）
    let tier = degrade::decide_tier(exe_dir, hyperv_ok);
    let vhdx = {
        let p = PathBuf::from(&cfg.vhdx_path);
        if p.is_absolute() {
            cfg.vhdx_path.clone()
        } else {
            exe_dir.join(&cfg.vhdx_path).to_string_lossy().to_string()
        }
    };

    match tier {
        degrade::RuntimeTier::Vm => {
            // 阶段 1：挂载
            ui.stage(1, "Mount-VHD");
            vmrun::mount_vhdx(&vhdx)?;

            // 阶段 2：启动 VM
            ui.stage(2, "创建/启动虚拟机");
            let session = vmrun::create_and_start_vm(cfg)?;

            // 阶段 3：等心跳 + VMConnect
            ui.stage(3, "等待引擎心跳");
            vmrun::wait_heartbeat(cfg, &session, HEARTBEAT_TIMEOUT)?;
            ui.detail("启动 VMConnect 全屏接管");
            let _ = vmrun::launch_vmconnect(&session.vm_name);

            // 等引擎退出（agent 回发 EXIT）→ 卸盘收尾
            ui.detail("运行中——引擎退出后自动卸盘");
            if vmrun::wait_engine_exit(cfg, EXIT_WAIT)? {
                ui.detail("卸载 VHDX…");
                let _ = vmrun::dismount_vhdx(&vhdx);
            }
            std::process::exit(0);
        }
        degrade::RuntimeTier::Vbox => {
            ui.stage(1, "VirtualBox Portable");
            ui.detail(degrade::RuntimeTier::Vbox.label());
            degrade::run_vbox(exe_dir, cfg)?;
            ui.stage(3, "等待引擎心跳");
            let session = vmrun::VmSession {
                vm_name: cfg.vm_name.clone(),
                vm_ip: None,
            };
            vmrun::wait_heartbeat(cfg, &session, HEARTBEAT_TIMEOUT)?;
            std::process::exit(0);
        }
        degrade::RuntimeTier::Light => {
            ui.stage(1, "轻量直跑模式");
            ui.detail("隔离弱但功能全（执行档兜底零残留）");
            let engine = exe_dir.join(&cfg.engine_exe);
            degrade::run_light(&engine, None)?;
            std::process::exit(0);
        }
    }
}
