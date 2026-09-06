//! VhdxBackend — VHDX 快速档（BLUEPRINT 3.1 快速档，B-16）。
//!
//! 机制：`Mount-VHD` 挂载 VHDX → 取盘符 → 委托 DirBackend 操作挂载点 →
//! seal 时 `Dismount-VHD`。需管理员或默认允许挂载的宿主（蓝图权限阶梯）。
//!
//! 如实边界（风险表第 1 行）：
//! - 宿主无 Hyper-V PowerShell 模块 / 无管理员 → `probe()` 返回不可用，
//!   上层降级 UxvBackend（终态档，免挂载权限）——本模块绝不静默提权；
//! - 挂载失败路径不删任何文件、不杀进程，错误如实上抛。

use crate::{CmdResult, ContainerError, OpenCfg};
use std::path::{Path, PathBuf};

/// 宿主能力探测结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VhdxProbe {
    pub is_admin: bool,
    pub mount_vhd_available: bool,
    pub usable: bool,
}

#[cfg(windows)]
mod imp {
    /// 管理员探测：whoami /groups 查 S-1-16-12288（高完整性级）。
    /// 纯子进程探测，不弹 UAC、不提权。
    pub fn is_admin() -> bool {
        let out = std::process::Command::new("whoami")
            .args(["/groups", "/fo", "csv", "/nh"])
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).contains("S-1-16-12288"),
            Err(_) => false,
        }
    }

    /// Mount-VHD 可用性：PowerShell Get-Command 探测（不实际挂载）。
    pub fn mount_vhd_available() -> bool {
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "if (Get-Command Mount-VHD -ErrorAction SilentlyContinue) { 'YES' } else { 'NO' }"])
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).contains("YES"),
            Err(_) => false,
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn is_admin() -> bool {
        false
    }
    pub fn mount_vhd_available() -> bool {
        false
    }
}

/// 探测（≤300ms 预算内两次子进程；引导器能力矩阵复用）。
pub fn probe() -> VhdxProbe {
    let is_admin = imp::is_admin();
    let mount_vhd_available = imp::mount_vhd_available();
    VhdxProbe {
        is_admin,
        mount_vhd_available,
        usable: is_admin && mount_vhd_available,
    }
}

/// 挂载 VHDX 并返回挂载盘根（如 `E:\`）。失败如实上抛（PowerShell stderr 透传）。
pub fn mount(vhdx: &Path) -> CmdResult<PathBuf> {
    let script = format!(
        "$d = Mount-VHD -Path '{}' -PassThru; ($d | Get-Disk | Get-Partition | Get-Volume).DriveLetter",
        vhdx.display()
    );
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map_err(|e| ContainerError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, e)))?;
    if !out.status.success() {
        return Err(ContainerError::Corrupted(format!(
            "Mount-VHD 失败: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let letter = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if letter.is_empty() {
        return Err(ContainerError::Corrupted("Mount-VHD 未返回盘符".into()));
    }
    Ok(PathBuf::from(format!("{letter}:\\")))
}

/// 卸载。退出冲刷协议的 VHDX 侧终点。
pub fn dismount(vhdx: &Path) -> CmdResult<()> {
    let script = format!("Dismount-VHD -Path '{}'", vhdx.display());
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .map_err(|e| ContainerError::Io(e))?;
    if !out.status.success() {
        return Err(ContainerError::Corrupted(format!(
            "Dismount-VHD 失败: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

/// VHDX 后端 = 挂载点上的 DirBackend（行为与目录档完全一致，蓝图同接口承诺）。
pub struct VhdxBackend {
    vhdx: Option<PathBuf>,
    inner: crate::DirBackend,
    mounted_root: Option<PathBuf>,
}

impl VhdxBackend {
    pub fn new() -> Self {
        VhdxBackend { vhdx: None, inner: crate::DirBackend::new(), mounted_root: None }
    }

    /// 测试/引导器专用：跳过真实挂载，直接把已存在目录当挂载点
    /// （探测降级路径的机制验证；生产路径必须经 `mount`）。
    pub fn with_premounted(root: PathBuf) -> Self {
        VhdxBackend {
            vhdx: None,
            inner: crate::DirBackend::new(),
            mounted_root: Some(root),
        }
    }
}

impl Default for VhdxBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::StorageBackend for VhdxBackend {
    fn open(&mut self, cfg: &OpenCfg) -> CmdResult<()> {
        let p = probe();
        if !p.usable && self.mounted_root.is_none() {
            return Err(ContainerError::NotImplemented(
                "VHDX 快速档不可用（无管理员或缺 Mount-VHD）——降级 UxvBackend",
            ));
        }
        let root = match &self.mounted_root {
            Some(r) => r.clone(), // 预挂载（测试/引导器已解析路径）
            None => mount(&cfg.root)?,
        };
        self.inner.open(&OpenCfg { root: root.clone(), extra_volumes: Vec::new() })?;
        if self.mounted_root.is_none() {
            self.vhdx = Some(cfg.root.clone()); // 真实挂载才登记卸载责任
        }
        self.mounted_root = Some(root);
        Ok(())
    }

    fn stat(&self, path: &crate::VPath) -> CmdResult<crate::StatInfo> {
        self.inner.stat(path)
    }
    fn read(&self, path: &crate::VPath) -> CmdResult<Vec<u8>> {
        self.inner.read(path)
    }
    fn read_range(&self, path: &crate::VPath, off: u64, len: u64) -> CmdResult<Vec<u8>> {
        self.inner.read_range(path, off, len)
    }
    fn stream(&self, path: &crate::VPath) -> CmdResult<Box<dyn crate::uxv::ReadSeek + '_>> {
        self.inner.stream(path)
    }
    fn write(&mut self, path: &crate::VPath, data: &[u8]) -> CmdResult<()> {
        self.inner.write(path, data)
    }
    fn list(&self, dir: &crate::VPath) -> CmdResult<Vec<crate::StatInfo>> {
        self.inner.list(dir)
    }
    fn mkdir(&mut self, dir: &crate::VPath) -> CmdResult<()> {
        self.inner.mkdir(dir)
    }
    fn rm(&mut self, path: &crate::VPath) -> CmdResult<()> {
        self.inner.rm(path)
    }
    fn rename(&mut self, from: &crate::VPath, to: &crate::VPath) -> CmdResult<()> {
        self.inner.rename(from, to)
    }
    fn copy(&mut self, from: &crate::VPath, to: &crate::VPath) -> CmdResult<()> {
        self.inner.copy(from, to)
    }
    fn snapshot(&mut self, label: &str) -> CmdResult<crate::SnapshotId> {
        self.inner.snapshot(label)
    }
    fn restore(&mut self, id: &crate::SnapshotId) -> CmdResult<()> {
        self.inner.restore(id)
    }
    fn seal(&mut self) -> CmdResult<()> {
        self.inner.seal()?;
        // 挂载态卸载（失败不阻塞退出冲刷——数据已 sync，如实返回错误即可）
        if let Some(vhdx) = self.vhdx.take() {
            dismount(&vhdx)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reports_boolean_capabilities() {
        let p = probe();
        // 探测本身必须成功返回（值随宿主而定，CI 无管理员属常态）
        assert!(!p.usable || p.is_admin);
        println!("vhdx probe: {p:?}");
    }

    #[test]
    fn premounted_backend_delegates_to_dir_semantics() {
        // 降级链路的机制证明：挂载点解析后的行为 = DirBackend
        let root = std::env::temp_dir().join(format!("vhdx-deleg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut be = VhdxBackend::with_premounted(root.clone());
        be.open(&OpenCfg { root: root.clone(), extra_volumes: Vec::new() })
            .unwrap_or_else(|_| {
                // with_premounted 仍走 probe 门禁——不可用宿主上如实报错即为正确行为
                panic!("premounted 应绕过挂载（probe 门禁只在真实 open 生效）");
            });
        use crate::{StorageBackend, VPath};
        be.write(&VPath::new("a.txt").unwrap(), b"vhdx-deleg").unwrap();
        assert_eq!(be.read(&VPath::new("a.txt").unwrap()).unwrap(), b"vhdx-deleg");
        be.seal().unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }
}
