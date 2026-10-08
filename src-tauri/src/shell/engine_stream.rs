//! 阶段 6 · S3.3/S3.4 引擎画面流通道 v1（三体 AI-2）。
//!
//! 设计（施工总案 6.3/6.4 的 v1 落地 + 开放性冻结）：
//!   - **传输抽象冻结**：画质三档（office/balanced/gaming）+ 全屏/RemoteApp
//!     双模式在此层定版；真实传输 v1 = Windows 内置 mstsc（RDP 协议），
//!     Spice/自研流后续可替换——接口不变（总案"流协议封装单模块可替换"）。
//!   - **复用 embed 既有语义**：mstsc 是真实 Windows 窗口，经既有 embed
//!     管线收编进 VWM（边框/贴靠/几何持久化零新代码）——总案"与 embed.rs
//!     既有语义复用不重写"。断流=进程死亡 → embed 占位卡语义天然兜底；
//!     重连=再次 open（幂等）。
//!   - **S3.5 输入注记**：mstsc 传输下键鼠/IME 经 RDP 会话原生直达引擎
//!     （注入通道=会话本身），Variable 侧不做二次注入；快速打字 30s 无丢键
//!     与 IME 全流程为 S1.2 后的实机验收项。
//!
//! 数据隔离红线（RDP 文件级硬门禁，测试看护）：
//!   - **宿主盘绝不重定向**（drivestoredirect=0）——SHARED 是唯一互通面；
//!   - 打印机/智能卡/串口/即插即用设备全部关闭；
//!   - 剪贴板开启（能力对照表已声明"剪贴板直通 ✓"）。
//!
//! 幂等：同 appKey 重复 open 返回既有会话不双开（与引擎编排同哲学）。
//! 零 unwrap：生产路径显式错误。
//!
//! 真实端到端（VM 在场、mstsc 连通、缩放同步 ×20）待 S1.2——本批交付
//! 代码级 + 纯函数单测。

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

use crate::error::{AppError, CmdResult};

/// 画质三档（与前端 engineModel StreamQualityTier 同名同义）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamQuality {
    Office,
    Balanced,
    Gaming,
}

impl StreamQuality {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "office" => Some(StreamQuality::Office),
            "gaming" => Some(StreamQuality::Gaming),
            "balanced" => Some(StreamQuality::Balanced),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            StreamQuality::Office => "office",
            StreamQuality::Balanced => "balanced",
            StreamQuality::Gaming => "gaming",
        }
    }

    /// 画质 → RDP 会话参数（v1 映射定版；换传输时映射重写、接口不动）。
    fn rdp_quality(&self) -> QualityParams {
        match self {
            StreamQuality::Office => QualityParams {
                bpp: 16,
                compression: 1,
                disable_wallpaper: 1,
                disable_full_window_drag: 1,
                disable_menu_anims: 1,
                disable_themes: 1,
            },
            StreamQuality::Balanced => QualityParams {
                bpp: 32,
                compression: 1,
                disable_wallpaper: 0,
                disable_full_window_drag: 0,
                disable_menu_anims: 0,
                disable_themes: 0,
            },
            StreamQuality::Gaming => QualityParams {
                bpp: 32,
                compression: 0,
                disable_wallpaper: 0,
                disable_full_window_drag: 0,
                disable_menu_anims: 0,
                disable_themes: 0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct QualityParams {
    bpp: u32,
    compression: u32,
    disable_wallpaper: u32,
    disable_full_window_drag: u32,
    disable_menu_anims: u32,
    disable_themes: u32,
}

// ---------------------------------------------------------------- RDP 文件生成（纯函数）

/// 流会话参数。
#[derive(Debug, Clone)]
pub struct StreamParams {
    /// 引擎 VM 地址（host 或 ip；端口由 RDP 默认 3389， discover 阶段不带端口）。
    pub address: String,
    /// RemoteApp 目标（None = 全屏桌面会话；Some(exe/alias) = 单应用窗口）。
    pub remote_app: Option<String>,
}

/// 生成 mstsc RDP 连接文件内容（纯函数；ASCII；安全键全量显式声明）。
pub fn build_rdp_file(p: &StreamParams, quality: StreamQuality) -> String {
    let q = quality.rdp_quality();
    let mut f = String::new();
    // —— 会话形态 ——
    match &p.remote_app {
        Some(app) => {
            f.push_str("remoteapplicationmode:i:1\n");
            f.push_str(&format!("remoteapplicationprogram:s:{app}\n"));
            // RemoteApp 窗口随内容自适应（总案 6.4 缩放双向同步的 v1 语义）。
            f.push_str("remoteapplicationexpandcmdline:i:0\n");
        }
        None => {
            f.push_str("remoteapplicationmode:i:0\n");
            f.push_str("screen mode id:i:2\n"); // 全屏（总案 6.3 全屏先行）
            f.push_str("smart sizing:i:1\n"); // 窗口缩放自适应
        }
    }
    f.push_str(&format!("full address:s:{}\n", p.address));
    // —— 画质三档 ——
    f.push_str(&format!("session bpp:i:{}\n", q.bpp));
    f.push_str(&format!("compression:i:{}\n", q.compression));
    f.push_str(&format!("disable wallpapaper:i:{}\n", q.disable_wallpaper));
    f.push_str(&format!("disable full window drag:i:{}\n", q.disable_full_window_drag));
    f.push_str(&format!("disable menu anims:i:{}\n", q.disable_menu_anims));
    f.push_str(&format!("disable themes:i:{}\n", q.disable_themes));
    f.push_str("networkautodetect:i:1\n");
    f.push_str("bandwidthautodetect:i:1\n");
    // —— 数据隔离红线（全量显式声明，测试逐键看护） ——
    f.push_str("drivestoredirect:s:\n"); // 宿主盘绝不重定向（SHARED 是唯一互通面）
    f.push_str("redirectprinters:i:0\n");
    f.push_str("redirectsmartcards:i:0\n");
    f.push_str("redirectcomports:i:0\n");
    f.push_str("redirectposdevices:i:0\n");
    f.push_str("devicestoredirect:s:\n"); // PnP 设备不透传
    f.push_str("redirectclipboard:i:1\n"); // 剪贴板直通（能力对照表声明项）
    f.push_str("redirectports:i:0\n");
    // —— 通用 ——
    f.push_str("authentication level:i:0\n"); // 引擎 VM 为本机可信端点
    f.push_str("prompt for credentials:i:0\n");
    f.push_str("negotiate security layer:i:1\n");
    f.push_str("allow font smoothing:i:1\n");
    f.push_str("allow desktop composition:i:1\n");
    f.push_str("autoreconnection enabled:i:1\n"); // 断流自动重连（总案 6.3）
    f.push_str("connection type:i:7\n"); // 自动侦测
    f
}

// ---------------------------------------------------------------- VM 地址发现

/// 引擎 VM 的 IPv4（Hyper-V 默认交换机 NAT 下的 DHCP 租约）。
#[cfg(windows)]
fn discover_vm_address(vm_name: &str) -> Result<String, String> {
    let script = format!(
        "$a=(Get-VM -Name '{n}' -ErrorAction Stop | Get-VMNetworkAdapter).IPAddresses | Where-Object {{ $_ -match '^\\d+\\.\\d+\\.\\d+\\.\\d+$' }} | Select-Object -First 1; \
         if ($a) {{ Write-Output $a }}",
        n = vm_name
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("PowerShell 启动失败：{e}"))?;
    if !out.status.success() {
        return Err(
            "引擎 VM 地址不可得。发生了什么=Hyper-V 查询失败；\
             为什么=VM 不存在或 Hyper-V 模块不可用；\
             下一步=先拉起引擎（engine_wake）再连接画面流"
                .to_string(),
        );
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        return Err(
            "引擎 VM 地址不可得。发生了什么=VM 尚未获得网络地址；\
             为什么=引擎未完成引导或未连接虚拟交换机；\
             下一步=等待引擎就绪（心跳 READY）后重试"
                .to_string(),
        );
    }
    Ok(s)
}

#[cfg(not(windows))]
fn discover_vm_address(_vm_name: &str) -> Result<String, String> {
    Err("引擎通道仅支持 Windows 宿主".to_string())
}

// ---------------------------------------------------------------- 会话注册表（幂等生命周期）

/// 活跃流会话（appKey 唯一；幂等不双开）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSession {
    pub app_key: String,
    pub quality: String,
    /// 全屏桌面 / RemoteApp 程序名。
    pub mode: String,
    pub mstsc_pid: u32,
}

fn registry() -> &'static Mutex<HashMap<String, StreamSession>> {
    static REG: OnceLock<Mutex<HashMap<String, StreamSession>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 纯状态机步进：登记会话（已存在则返回既有——幂等语义，供测试与 open 共用）。
fn registry_put(s: StreamSession) -> StreamSession {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    match reg.get(&s.app_key) {
        Some(existing) => existing.clone(),
        None => {
            reg.insert(s.app_key.clone(), s.clone());
            s
        }
    }
}

/// 纯状态机步进：撤会话；返回被撤者（无则 None）。
fn registry_take(app_key: &str) -> Option<StreamSession> {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(app_key)
}

fn registry_list() -> Vec<StreamSession> {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .cloned()
        .collect()
}

// ---------------------------------------------------------------- 命令层

/// 打开画面流（幂等：同 appKey 已在流中则返回既有会话）。
/// mode：desktop=全屏桌面；其余=RemoteApp 程序名。
#[tauri::command(async)]
pub fn engine_stream_open(
    app_key: String,
    quality: String,
    mode: String,
) -> CmdResult<StreamSession> {
    let q = StreamQuality::parse(&quality).ok_or_else(|| {
        AppError::validation(format!(
            "画质档位无效：{quality}（合法值 office|balanced|gaming）"
        ))
    })?;
    // 幂等：已在流中 → 返回既有会话（不双开）。
    if let Some(existing) = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&app_key)
        .cloned()
    {
        return Ok(existing);
    }
    // VM 地址发现（引擎未就绪时在此如实失败）。
    let address = discover_vm_address("VARIX-Engine")
        .map_err(|e| AppError::new("ENGINE_STREAM_ADDRESS", e))?;
    let remote_app = if mode == "desktop" { None } else { Some(mode.clone()) };
    let params = StreamParams { address, remote_app };
    let content = build_rdp_file(&params, q);
    // RDP 文件落 TEMP（会话关闭时删除；TEMP 本身随系统清理——非 U 盘路径，
    // 与"U 盘零残留"无交集）。
    let rdp_path = std::env::temp_dir().join(format!(
        "varix-engine-{}.rdp",
        sanitize_key(&app_key)
    ));
    std::fs::write(&rdp_path, content)
        .map_err(|e| AppError::io(format!("RDP 连接文件写入失败：{e}")))?;
    let child = std::process::Command::new("mstsc")
        .arg(&rdp_path)
        .spawn()
        .map_err(|e| {
            let _ = std::fs::remove_file(&rdp_path);
            AppError::new(
                "ENGINE_STREAM_SPAWN",
                format!(
                    "mstsc 启动失败：{e}。发生了什么=远程桌面客户端未能启动；\
                     为什么=系统组件缺失或被策略限制；\
                     下一步=检查 mstsc.exe 可用性后重试"
                ),
            )
        })?;
    let session = StreamSession {
        app_key,
        quality: q.as_str().to_string(),
        mode,
        mstsc_pid: child.id(),
    };
    Ok(registry_put(session))
}

/// 关闭画面流（停 mstsc + 清连接文件 + 撤会话；幂等）。
#[tauri::command(async)]
pub fn engine_stream_close(app_key: String) -> CmdResult<()> {
    if let Some(s) = registry_take(&app_key) {
        #[cfg(windows)]
        {
            // 尽力终止 mstsc（进程可能已被用户手动关闭）。
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &s.mstsc_pid.to_string(), "/F"])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .output();
        }
        let rdp_path = std::env::temp_dir().join(format!(
            "varix-engine-{}.rdp",
            sanitize_key(&s.app_key)
        ));
        let _ = std::fs::remove_file(&rdp_path);
    }
    Ok(())
}

/// 活跃流会话列表（公示面）。
#[tauri::command(async)]
pub fn engine_stream_status() -> CmdResult<Vec<StreamSession>> {
    Ok(registry_list())
}

fn sanitize_key(k: &str) -> String {
    k.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

// ---------------------------------------------------------------- 测试（纯函数全覆盖）

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdp_file_fullscreen_desktop() {
        let p = StreamParams {
            address: "192.168.1.50".into(),
            remote_app: None,
        };
        let f = build_rdp_file(&p, StreamQuality::Balanced);
        assert!(f.contains("screen mode id:i:2"), "桌面模式必须全屏");
        assert!(f.contains("remoteapplicationmode:i:0"));
        assert!(f.contains("full address:s:192.168.1.50"));
        assert!(f.contains("smart sizing:i:1"));
        assert!(f.contains("session bpp:i:32"));
        assert!(f.contains("compression:i:1"));
    }

    #[test]
    fn rdp_file_remoteapp_mode() {
        let p = StreamParams {
            address: "10.0.0.9".into(),
            remote_app: Some("C:\\Apps\\edge.exe".into()),
        };
        let f = build_rdp_file(&p, StreamQuality::Office);
        assert!(f.contains("remoteapplicationmode:i:1"));
        assert!(f.contains("remoteapplicationprogram:s:C:\\Apps\\edge.exe"));
        assert!(!f.contains("screen mode id"), "RemoteApp 不声明全屏键");
    }

    #[test]
    fn rdp_file_isolation_redlines_guarded() {
        // 数据隔离红线：宿主盘/打印机/智能卡/串口/PnP 全关；剪贴板开。
        let p = StreamParams {
            address: "1.2.3.4".into(),
            remote_app: None,
        };
        for q in [StreamQuality::Office, StreamQuality::Balanced, StreamQuality::Gaming] {
            let f = build_rdp_file(&p, q);
            assert!(f.contains("drivestoredirect:s:\n"), "宿主盘重定向必须为空（红线）");
            assert!(f.contains("redirectprinters:i:0"));
            assert!(f.contains("redirectsmartcards:i:0"));
            assert!(f.contains("redirectcomports:i:0"));
            assert!(f.contains("redirectposdevices:i:0"));
            assert!(f.contains("devicestoredirect:s:\n"));
            assert!(f.contains("redirectclipboard:i:1"), "剪贴板直通是声明能力");
        }
    }

    #[test]
    fn quality_mapping_three_tiers() {
        let p = StreamParams { address: "a".into(), remote_app: None };
        let office = build_rdp_file(&p, StreamQuality::Office);
        let gaming = build_rdp_file(&p, StreamQuality::Gaming);
        assert!(office.contains("session bpp:i:16"), "办公档降色深省带宽");
        assert!(office.contains("disable wallpapaper:i:1"));
        assert!(gaming.contains("session bpp:i:32"));
        assert!(gaming.contains("compression:i:0"), "游戏档不压缩保帧率");
    }

    #[test]
    fn autoreconnect_always_on() {
        let p = StreamParams { address: "a".into(), remote_app: None };
        let f = build_rdp_file(&p, StreamQuality::Office);
        assert!(f.contains("autoreconnection enabled:i:1"), "断流自动重连（总案 6.3）");
    }

    #[test]
    fn quality_parse_strict() {
        assert_eq!(StreamQuality::parse("office"), Some(StreamQuality::Office));
        assert_eq!(StreamQuality::parse("gaming"), Some(StreamQuality::Gaming));
        assert_eq!(StreamQuality::parse("balanced"), Some(StreamQuality::Balanced));
        assert_eq!(StreamQuality::parse("ultra"), None, "未知档位拒绝（不静默回落）");
    }

    #[test]
    fn registry_idempotent_no_double_session() {
        let s1 = registry_put(StreamSession {
            app_key: "test.app".into(),
            quality: "balanced".into(),
            mode: "desktop".into(),
            mstsc_pid: 111,
        });
        let s2 = registry_put(StreamSession {
            app_key: "test.app".into(),
            quality: "gaming".into(),
            mode: "desktop".into(),
            mstsc_pid: 222,
        });
        assert_eq!(s1.mstsc_pid, s2.mstsc_pid, "重复登记返回既有会话（幂等）");
        assert_eq!(registry_list().iter().filter(|s| s.app_key == "test.app").count(), 1);
        let taken = registry_take("test.app");
        assert!(taken.is_some());
        assert!(registry_take("test.app").is_none(), "二次撤除 = None（幂等）");
    }

    #[test]
    fn sanitize_key_neutralizes_path_tricks() {
        assert_eq!(sanitize_key("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_key("app-1_2"), "app-1_2");
    }
}
