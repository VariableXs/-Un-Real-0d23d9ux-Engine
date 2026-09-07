//! L-3 Hyper-V 自动化与降级链（终极形态施工总计划 5.3）。
//!
//! 档 1：Hyper-V 完整 VM（vmrun.rs 编排）
//! 档 2：VirtualBox Portable（U 盘自带 runtime/vbox/，VBoxManage headless）
//! 档 3：轻量直跑（Variable 直接跑宿主机 + 执行档重定向容器，隔离弱但功能全）
//!
//! 每档附能力诚实清单；Hyper-V 未启用时提供提权启用流程（pending-state.json 续跑，
//! 不写宿主启动项——重启后由用户手动双击引导器继续）。全程用户可取消。

use crate::vmrun::LauncherConfig;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeTier {
    /// 档 1：Hyper-V 完整 VM（隔离最强）
    Vm,
    /// 档 2：VirtualBox Portable
    Vbox,
    /// 档 3：轻量直跑
    Light,
}

impl RuntimeTier {
    /// 能力诚实清单（设置→存储→运行环境 同口径）
    pub fn capability_list(self) -> &'static [&'static str] {
        match self {
            RuntimeTier::Vm => &[
                "系统级隔离 ✓（独立内核，宿主零污染）",
                "零残留 ✓（关机即还原/差分重置）",
                "GPU 加速 ✗（虚拟显卡，B 档渲染）",
                "USB 直通需手动挂载",
            ],
            RuntimeTier::Vbox => &[
                "系统级隔离 ✓（独立内核）",
                "零残留 ✓（差异化快照）",
                "GPU 加速 ✗（VBoxSVGA，B 档渲染）",
                "无缝模式/剪贴板可选",
            ],
            RuntimeTier::Light => &[
                "系统级隔离 ✗（与宿主共内核）",
                "零残留 ✓（执行档重定向兜底，residue-check --expect-clean）",
                "GPU 加速 ✓（原生档位，S/A 档可用）",
                "注册表/服务类安装软件受限",
            ],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RuntimeTier::Vm => "Hyper-V 完整 VM",
            RuntimeTier::Vbox => "VirtualBox Portable",
            RuntimeTier::Light => "轻量直跑",
        }
    }
}

/// pending-state.json（U 盘根目录）：Hyper-V 启用流程的重启续跑标记
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PendingState {
    /// "enable-hyperv" = 等待用户重启后双击引导器续跑
    pub action: String,
    /// 发起时间（unix secs）
    pub at_secs: u64,
    /// 发起时记录的宿主标识（换机则作废）
    pub host_key: String,
}

pub fn pending_state_path(exe_dir: &PathBuf) -> PathBuf {
    exe_dir.join("pending-state.json")
}

pub fn read_pending_state(exe_dir: &PathBuf) -> Option<PendingState> {
    let text = std::fs::read_to_string(pending_state_path(exe_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_pending_state(exe_dir: &PathBuf, action: &str, host_key: &str) -> Result<(), String> {
    let st = PendingState {
        action: action.to_string(),
        at_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        host_key: host_key.to_string(),
    };
    let json = serde_json::to_string_pretty(&st).map_err(|e| e.to_string())?;
    std::fs::write(pending_state_path(exe_dir), json).map_err(|e| format!("写 pending-state 失败: {e}"))
}

pub fn clear_pending_state(exe_dir: &PathBuf) {
    let _ = std::fs::remove_file(pending_state_path(exe_dir));
}

/// 提权启用 Hyper-V（弹确认 → 管理员 PowerShell 执行 Enable-WindowsOptionalFeature）。
/// 返回 Ok(true)=已发起（需重启）；Ok(false)=用户取消；Err=失败。
pub fn prompt_enable_hyperv(host_key: &str, exe_dir: &PathBuf) -> Result<bool, String> {
    let msg = "未检测到 Hyper-V。\n\n是否以管理员身份启用 Hyper-V 功能？（约 5-10 分钟，需要重启）\n\n同意：将弹出 UAC 提权窗口并执行系统功能启用命令。\n重启后请再次双击本引导器继续（本引导器不会写入开机启动项）。\n\n[是] 启用  [否] 取消（可使用轻量直跑模式）";
    let wide: Vec<u16> = msg.encode_utf16().chain([0]).collect();
    let title: Vec<u16> = "Variable 引导器".encode_utf16().chain([0]).collect();
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONQUESTION, MB_YESNO};
        let ret = unsafe {
            MessageBoxW(
                None,
                windows::core::PCWSTR(wide.as_ptr()),
                windows::core::PCWSTR(title.as_ptr()),
                MB_YESNO | MB_ICONQUESTION,
            )
        };
        if ret.0 != 6 {
            return Ok(false); // IDNO
        }
    }
    write_pending_state(exe_dir, "enable-hyperv", host_key)?;
    let script = "Start-Process powershell -Verb RunAs -ArgumentList '-NoProfile','-Command','Enable-WindowsOptionalFeature -Online -FeatureName Microsoft-Hyper-V -All -NoRestart'";
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000);
    let out = cmd.output().map_err(|e| format!("提权启动失败: {e}"))?;
    if !out.status.success() {
        clear_pending_state(exe_dir);
        return Err("启用 Hyper-V 被取消或失败（UAC 拒绝?）。系统未被修改。".into());
    }
    Ok(true)
}

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

/// 档 2 探测：U 盘自带 runtime/vbox/VBoxManage.exe
pub fn vbox_available(exe_dir: &PathBuf) -> Option<PathBuf> {
    let p = exe_dir.join(r"runtime\vbox\VBoxManage.exe");
    p.is_file().then_some(p)
}

/// 档 2：VirtualBox Portable headless 启动 + 全屏
pub fn run_vbox(exe_dir: &PathBuf, cfg: &LauncherConfig) -> Result<(), String> {
    let vbox = vbox_available(exe_dir).ok_or("未找到 runtime/vbox/VBoxManage.exe（档 2 不可用）")?;
    let vhdx = &cfg.vhdx_path;
    let name = &cfg.vm_name;
    let run = |args: &[&str]| -> Result<String, String> {
        let out = std::process::Command::new(&vbox)
            .args(args)
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| format!("VBoxManage 启动失败: {e}"))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        } else {
            Err(format!(
                "VBoxManage {:?} 失败: {}",
                args,
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    };
    // 注册（幂等）+ 启动
    let _ = run(&["unregistervm", name, "--delete"]);
    run(&[
        "createvm",
        "--name",
        name,
        "--register",
    ])?;
    run(&[
        "modifyvm",
        name,
        "--memory",
        &cfg.memory_max_mb.to_string(),
        "--cpus",
        &cfg.cpu_count.to_string(),
        "--usb",
        "off",
        "--audio",
        "none",
    ])?;
    run(&["storagectl", name, "--name", "SATA", "--add", "sata"])?;
    run(&[
        "storageattach",
        name,
        "--storagectl",
        "SATA",
        "--port",
        "0",
        "--device",
        "0",
        "--type",
        "hdd",
        "--medium",
        vhdx,
    ])?;
    run(&["startvm", name, "--type", "headless"])?;
    // 全屏 GUI（GUI 已随 headless? 需要单独前端）：启动 GUI 附加会话
    let gui = exe_dir.join(r"runtime\vbox\VirtualBoxVM.exe");
    if gui.is_file() {
        let _ = std::process::Command::new(&gui)
            .args(["--comment", name, "--startvm", name])
            .spawn();
    }
    Ok(())
}

/// 档 3：轻量直跑——直接拉起引擎（执行档容器由引擎内部兜底）
pub fn run_light(engine_abs: &PathBuf, host_addr: Option<&str>) -> Result<(), String> {
    if !engine_abs.is_file() {
        return Err(format!("引擎不存在: {}", engine_abs.display()));
    }
    let mut cmd = std::process::Command::new(engine_abs);
    cmd.env("VAR_RUNTIME_MODE", "light");
    if let Some(h) = host_addr {
        cmd.env("VAR_HOST_ADDR", h);
    }
    cmd.spawn().map_err(|e| format!("引擎启动失败: {e}"))?;
    Ok(())
}

/// 决策：按可用性选档（UI 显示当前档）
pub fn decide_tier(exe_dir: &PathBuf, hyperv_tools: bool) -> RuntimeTier {
    if hyperv_tools {
        RuntimeTier::Vm
    } else if vbox_available(exe_dir).is_some() {
        RuntimeTier::Vbox
    } else {
        RuntimeTier::Light
    }
}
