//! C14 · VFS 读写桥（写回通道的壳C 实现层）。
//!
//! 三壳等价铁律：**同一份相对路径写请求，三端各换实现层**——
//! 壳A/壳B：`ca-core::shell::WriteChannel` 磁盘直写；
//! 壳C：本模块把同一份 `rel` 翻译成内核 VFS 挂载点 `/app/codeanalysis/<rel>`
//! 下的 `open/read/write/close`（[`varix_std::io`]）。
//!
//! 路径防逃逸规则与 core 侧 `safe_rel` 同源：拒绝空路径、绝对路径、
//! `..` 回溯、反斜杠与盘符——内核 VFS 前缀不允许任何逃逸出应用沙箱。
//!
//! 宿主测试用 [`LogBridge`] 记录计划中的操作序列（与内核实现走同一套
//! 路径与顺序逻辑）；真实 syscall 收在 [`KernelBridge`]（`target_os = "none"`）。

use crate::manifest::{CAP_FS_READ, CAP_FS_WRITE};

/// 内核 VFS 挂载点（应用沙箱根）。
pub const VFS_MOUNT: &str = "/app/codeanalysis";

/// 相对路径 → 内核 VFS 绝对路径；非法（逃逸/盘符/反斜杠/空）返回 `None`。
pub fn safe_vfs_path(rel: &str) -> Option<String> {
    if rel.is_empty()
        || rel.starts_with('/')
        || rel.starts_with('\\')
        || rel.contains("..")
        || rel.contains('\\')
        || rel.contains(':')
    {
        return None;
    }
    // 分段拒绝 "." 与空段（"a//b"），与 core 侧规则对齐。
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." {
            return None;
        }
    }
    Some(format!("{VFS_MOUNT}/{rel}"))
}

/// 桥后端抽象：内核实现走 syscall，宿主实现记录操作。
pub trait FsBridge {
    fn read(&mut self, rel: &str) -> Result<Vec<u8>, BridgeError>;
    fn write(&mut self, rel: &str, data: &[u8]) -> Result<usize, BridgeError>;
}

/// 桥层错误（内核错误码的桥侧投影，避免宿主测试依赖 ABI）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeError {
    BadPath,
    NotFound,
    NoCap,
    Io,
}

/// 宿主桩：记录操作序列，内存作为存储介质。
#[derive(Default)]
pub struct LogBridge {
    pub ops: Vec<&'static str>,
    store: Vec<(String, Vec<u8>)>,
}

impl LogBridge {
    pub fn new() -> Self {
        Self::default()
    }
}

impl FsBridge for LogBridge {
    fn read(&mut self, rel: &str) -> Result<Vec<u8>, BridgeError> {
        let path = safe_vfs_path(rel).ok_or(BridgeError::BadPath)?;
        self.ops.push("read");
        let _ = path;
        self.store
            .iter()
            .find(|(p, _)| *p == rel)
            .map(|(_, d)| d.clone())
            .ok_or(BridgeError::NotFound)
    }

    fn write(&mut self, rel: &str, data: &[u8]) -> Result<usize, BridgeError> {
        let path = safe_vfs_path(rel).ok_or(BridgeError::BadPath)?;
        self.ops.push("write");
        let _ = path;
        match self.store.iter_mut().find(|(p, _)| *p == rel) {
            Some(slot) => {
                slot.1 = data.to_vec();
            }
            None => self.store.push((rel.to_string(), data.to_vec())),
        }
        Ok(data.len())
    }
}

/// 内核实现：把路径翻译进 VFS 沙箱后走 varix-std io（仅 none 目标）。
pub struct KernelBridge {
    /// 能力位（来自 [`crate::manifest`]，内核注册时授予）。
    pub caps: u32,
}

impl KernelBridge {
    #[cfg(target_os = "none")]
    pub fn new(caps: u32) -> Self {
        KernelBridge { caps }
    }

    fn check_caps(&self, need: u32) -> Result<(), BridgeError> {
        if self.caps & need == need {
            Ok(())
        } else {
            Err(BridgeError::NoCap)
        }
    }
}

impl FsBridge for KernelBridge {
    #[cfg(target_os = "none")]
    fn read(&mut self, rel: &str) -> Result<Vec<u8>, BridgeError> {
        use varix_std::io;
        self.check_caps(CAP_FS_READ)?;
        let path = safe_vfs_path(rel).ok_or(BridgeError::BadPath)?;
        let fd = io::open(&path, io::R_ONLY).map_err(|_| BridgeError::Io)?;
        let mut out = Vec::new();
        let mut chunk = [0u8; 256];
        loop {
            match io::read(fd, &mut chunk) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&chunk[..n]),
                Err(_) => {
                    let _ = io::close(fd);
                    return Err(BridgeError::Io);
                }
            }
        }
        let _ = io::close(fd);
        Ok(out)
    }

    #[cfg(not(target_os = "none"))]
    fn read(&mut self, rel: &str) -> Result<Vec<u8>, BridgeError> {
        self.check_caps(CAP_FS_READ)?;
        let _ = rel;
        Err(BridgeError::Io)
    }

    #[cfg(target_os = "none")]
    fn write(&mut self, rel: &str, data: &[u8]) -> Result<usize, BridgeError> {
        self.check_caps(CAP_FS_WRITE)?;
        let path = safe_vfs_path(rel).ok_or(BridgeError::BadPath)?;
        let fd = io::open(&path, io::W_ONLY).map_err(|_| BridgeError::Io)?;
        let n = match io::write_all(fd, data) {
            Ok(n) => n,
            Err(_) => {
                let _ = io::close(fd);
                return Err(BridgeError::Io);
            }
        };
        let _ = io::close(fd);
        Ok(n)
    }

    #[cfg(not(target_os = "none"))]
    fn write(&mut self, rel: &str, data: &[u8]) -> Result<usize, BridgeError> {
        self.check_caps(CAP_FS_WRITE)?;
        let _ = rel;
        let _ = data;
        Err(BridgeError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_land_in_sandbox() {
        assert_eq!(
            safe_vfs_path("src/main.rs").as_deref(),
            Some("/app/codeanalysis/src/main.rs")
        );
        assert_eq!(safe_vfs_path(""), None);
        assert_eq!(safe_vfs_path("/abs"), None);
        assert_eq!(safe_vfs_path("../esc"), None);
        assert_eq!(safe_vfs_path("a/../b"), None);
        assert_eq!(safe_vfs_path("C:\\x"), None);
        assert_eq!(safe_vfs_path("a//b"), None);
        assert_eq!(safe_vfs_path("./x"), None);
    }

    #[test]
    fn log_bridge_roundtrip() {
        let mut b = LogBridge::new();
        assert_eq!(b.write("notes/a.md", b"hello").unwrap(), 5);
        assert_eq!(b.read("notes/a.md").unwrap(), b"hello");
        assert_eq!(b.write("notes/a.md", b"hi!").unwrap(), 3);
        assert_eq!(b.read("notes/a.md").unwrap(), b"hi!");
        assert_eq!(b.ops, vec!["write", "read", "write", "read"]);
    }

    #[test]
    fn log_bridge_rejects_escape() {
        let mut b = LogBridge::new();
        assert_eq!(b.write("../esc", b"x"), Err(BridgeError::BadPath));
        assert_eq!(b.read(""), Err(BridgeError::BadPath));
    }

    #[test]
    fn kernel_bridge_enforces_caps_on_host() {
        let mut no_caps = KernelBridge { caps: 0 };
        assert_eq!(no_caps.write("a.txt", b"x"), Err(BridgeError::NoCap));
        assert_eq!(no_caps.read("a.txt"), Err(BridgeError::NoCap));
        // 有能力位但宿主无内核 → Io（不是 NoCap）。
        let mut full = KernelBridge {
            caps: CAP_FS_READ | CAP_FS_WRITE,
        };
        assert_eq!(full.write("a.txt", b"x"), Err(BridgeError::Io));
    }
}
