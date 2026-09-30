//! L-1 VM 编排：Mount-VHD → New-VM（代际 2 / 关 Secure Boot / 动态内存 / DVD 直挂 vhdx）
//! → Start-VM → 等待 VM 内 agent 心跳（47631）→ VMConnect 窗口化。
//!
//! 同时承担 L-3 降级链的执行原语（档 2 VBox / 档 3 轻量直跑由上层调度）。
//! 全程零宿主注册表写入、零启动项；失败路径由 main.rs 统一给诊断包入口。

use serde::{Deserialize, Serialize};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

/// launcher.toml（与 exe 同目录）。极简手工解析（key = "value"），避免引入 toml 依赖。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    pub vhdx_path: String,
    pub vm_name: String,
    pub memory_min_mb: u64,
    pub memory_max_mb: u64,
    pub cpu_count: u32,
    pub engine_exe: String,
    pub heartbeat_port: u16,
    /// 宿主回调端口：VM 内 agent 主动连接汇报（READY/EXIT）
    pub host_callback_port: u16,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            vhdx_path: "Variable-OS.vhdx".into(),
            vm_name: "VariableOS".into(),
            memory_min_mb: 2048,
            memory_max_mb: 6144,
            cpu_count: 4,
            engine_exe: r"Variable\Variable.exe".into(),
            heartbeat_port: 47631,
            host_callback_port: 47632,
        }
    }
}

impl LauncherConfig {
    pub fn load(exe_dir: &PathBuf) -> Self {
        let mut cfg = Self::default();
        if let Ok(text) = std::fs::read_to_string(exe_dir.join("launcher.toml")) {
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('#') || line.is_empty() {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let v = v.trim().trim_matches('"').to_string();
                    match k.trim() {
                        "vhdx_path" => cfg.vhdx_path = v,
                        "vm_name" => cfg.vm_name = v,
                        "memory_min_mb" => cfg.memory_min_mb = v.parse().unwrap_or(cfg.memory_min_mb),
                        "memory_max_mb" => cfg.memory_max_mb = v.parse().unwrap_or(cfg.memory_max_mb),
                        "cpu_count" => cfg.cpu_count = v.parse().unwrap_or(cfg.cpu_count),
                        "engine_exe" => cfg.engine_exe = v,
                        "heartbeat_port" => cfg.heartbeat_port = v.parse().unwrap_or(47631),
                        "host_callback_port" => {
                            cfg.host_callback_port = v.parse().unwrap_or(47632)
                        }
                        _ => {}
                    }
                }
            }
        }
        cfg
    }
}

fn ps(script: &str) -> Result<String, String> {
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(0x0800_0000);
    let out = cmd.output().map_err(|e| format!("无法启动 PowerShell: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "PowerShell 失败(exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

pub struct VmSession {
    pub vm_name: String,
    pub vm_ip: Option<String>,
}

/// 阶段 2：挂载 VHDX（幂等——已挂载则跳过）
pub fn mount_vhdx(vhdx: &str) -> Result<(), String> {
    let abs = PathBuf::from(vhdx);
    if !abs.is_absolute() {
        return Err(format!("vhdx_path 必须为绝对路径: {vhdx}"));
    }
    if !abs.exists() {
        return Err(format!("VHDX 不存在: {vhdx}"));
    }
    // 已挂载则跳过（Get-VHD 报错 = 未挂载）
    if ps(&format!(
        "if (Get-VHD -Path '{}' -ErrorAction SilentlyContinue) {{ 'mounted' }} else {{ Mount-VHD -Path '{}' -PassThru | Out-Null; 'just-mounted' }}",
        vhdx, vhdx
    ))?
    .is_empty()
    {
        return Err("Mount-VHD 无输出".into());
    }
    Ok(())
}

/// 阶段 3：创建（幂等）并启动 VM，返回会话
pub fn create_and_start_vm(cfg: &LauncherConfig) -> Result<VmSession, String> {
    let name = &cfg.vm_name;
    let vhdx = &cfg.vhdx_path;
    ps(&format!(
        r#"$vm = Get-VM -Name '{name}' -ErrorAction SilentlyContinue
if (-not $vm) {{
  $disk = Get-Disk | Where-Object {{ $_.Location -like '*{0}*' }} | Select-Object -First 1
  if (-not $disk) {{ throw 'VHDX 已挂载但未找到对应磁盘' }}
  $letter = (Get-Partition -DiskNumber $disk.Number -ErrorAction SilentlyContinue | Select-Object -First 1).DriveLetter
  $bootName = if ($letter) {{ "$($letter):\" }} else {{ $null }}
  New-VM -Name '{name}' -MemoryStartupBytes ({1}MB) -VHDPath '{0}' -Generation 2 -SwitchName 'Default Switch' | Out-Null
  Set-VM -Name '{name}' -ProcessorCount {3} -DynamicMemory -MemoryMinimumBytes ({1}MB) -MemoryMaximumBytes ({2}MB) -CheckpointType Disabled | Out-Null
  Set-VMFirmware -VMName '{name}' -EnableSecureBoot Off | Out-Null
}}
Start-VM -Name '{name}'"#,
        vhdx, cfg.memory_min_mb, cfg.memory_max_mb, cfg.cpu_count
    ))?;
    // 取 VM IP（Default Switch DHCP，最多等 20s）
    let mut vm_ip = None;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let out = ps(&format!(
            "(Get-VM -Name '{name}').NetworkAdapters.IPAddresses | Where-Object {{ $_ -match '^\\d+\\.' }} | Select-Object -First 1"
        ))?;
        if !out.is_empty() {
            vm_ip = Some(out);
            break;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    Ok(VmSession {
        vm_name: name.clone(),
        vm_ip,
    })
}

/// 阶段 4：等待 VM 内 agent 心跳（agent 监听 VM 内 127.0.0.1:47631，
/// 宿主经 VM IP 连接；VM IP 未取到时退化为等待 agent 主动回连宿主回调端口）。
pub fn wait_heartbeat(cfg: &LauncherConfig, session: &VmSession, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    // 并行兜底：宿主回调端口监听（VM → host 方向）
    let listener = TcpListener::bind(("127.0.0.1", cfg.host_callback_port as u16))
        .map_err(|e| format!("回调端口 {} 占用: {e}", cfg.host_callback_port))?;
    listener.set_nonblocking(true).ok();

    loop {
        if let Some(ip) = &session.vm_ip {
            if let Ok(mut s) = TcpStream::connect_timeout(
                &format!("{ip}:{}", cfg.heartbeat_port).parse().unwrap(),
                Duration::from_millis(800),
            ) {
                if s.write_all(b"PING\n").is_ok() {
                    let mut buf = [0u8; 64];
                    if std::io::Read::read(&mut s, &mut buf).is_ok()
                        && buf.starts_with(b"READY")
                    {
                        return Ok(());
                    }
                }
            }
        }
        // VM → host 回连
        if let Ok((stream, _)) = listener.accept() {
            let mut s = stream;
            let mut buf = [0u8; 64];
            if std::io::Read::read(&mut s, &mut buf).is_ok() && buf.starts_with(b"READY") {
                return Ok(());
            }
        }
        if Instant::now() > deadline {
            return Err(format!(
                "等待 VM 心跳超时（{}s）。VM 内引擎可能未启动或网络未就绪。",
                timeout.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// 阶段 5：全屏窗口化 VMConnect
pub fn launch_vmconnect(vm_name: &str) -> Result<(), String> {
    let vmc = Command::new("vmconnect")
        .arg("localhost")
        .arg(vm_name)
        .spawn();
    match vmc {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("vmconnect 启动失败（Hyper-V 管理工具缺失?）: {e}")),
    }
}

use std::io::Write;

/// 引擎退出通知（agent 回发 EXIT）等待：返回 true 表示 VM 引擎已请求卸盘
pub fn wait_engine_exit(
    cfg: &LauncherConfig,
    timeout: Duration,
) -> Result<bool, String> {
    let listener = TcpListener::bind(("127.0.0.1", cfg.host_callback_port as u16))
        .map_err(|e| format!("回调端口占用: {e}"))?;
    listener.set_nonblocking(true).ok();
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok((mut s, _)) = listener.accept() {
            let mut buf = [0u8; 64];
            if std::io::Read::read(&mut s, &mut buf).is_ok() && buf.starts_with(b"EXIT") {
                return Ok(true);
            }
            if buf.starts_with(b"READY") {
                return Ok(false);
            }
        }
        if Instant::now() > deadline {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

/// 阶段 6：收尾卸盘（引擎退出后）
pub fn dismount_vhdx(vhdx: &str) -> Result<(), String> {
    ps(&format!("Dismount-VHD -Path '{vhdx}'"))?;
    Ok(())
}
