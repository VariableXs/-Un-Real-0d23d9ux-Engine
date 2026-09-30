//! L-1 / V-1：VM 内 Agent（src-tauri feature `vm-agent`）。
//!
//! 引擎在 VM 内运行时：
//! 1. 监听 VM 内 127.0.0.1:47631 —— 宿主引导器（SystemLauncher）连接探测「引擎已就绪」心跳；
//! 2. 主动回连宿主（env VAR_HOST_ADDR 提供的 host:47632）发送 READY；引擎退出时发送 EXIT，
//!    通知宿主安全卸盘（Dismount-VHD）。
//!
//! 零残留：纯 TCP，不写注册表、不留启动项。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

pub const HEARTBEAT_PORT: u16 = 47631;
pub const HOST_CALLBACK_PORT: u16 = 47632;

/// 启动 agent（应用 setup 阶段调用一次；非 VM 环境下端口被占则静默跳过）
pub fn spawn() {
    std::thread::spawn(|| {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", HEARTBEAT_PORT)) {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buf = [0u8; 16];
                if matches!(s.read(&mut buf), Ok(n) if n > 0 && buf.starts_with(b"PING")) {
                    let _ = s.write_all(format!("READY pid={}\n", std::process::id()).as_bytes());
                }
            }
        }
        // 端口占用 = 非引导器编排环境，静默不作为
    });
}

/// 通知宿主：引擎已就绪（READY）/ 即将退出（EXIT）
pub fn notify_host(msg: &str) -> bool {
    let Some(host) = std::env::var("VAR_HOST_ADDR").ok() else { return false };
    let addr = format!("{host}:{HOST_CALLBACK_PORT}");
    match TcpStream::connect(&addr) {
        Ok(mut s) => {
            let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
            s.write_all(msg.as_bytes()).is_ok()
        }
        Err(_) => false,
    }
}

/// 引擎退出时调用（RunEvent::Exit / 退出时序），失败不阻塞退出
pub fn notify_host_exit() {
    let _ = notify_host(b"EXIT\n");
}
