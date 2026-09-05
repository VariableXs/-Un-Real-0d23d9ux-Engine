//! container — L1 存储与容器层（BLUEPRINT 3.1 / 7.1，批次 B-2 骨架）
//!
//! 本 crate 冻结 `StorageBackend` trait 契约并提供第一个后端 `DirBackend`
//! （现状数据目录的包装，行为与直接文件系统操作等价）。
//! 后续批次按同一契约落地 `VhdxBackend`（B-16）与 `UxvBackend`（B-12…B-15）。
//!
//! 契约纪律（MASTER-PLAN 第 6 节第 7 条）：本 trait 签名与 BLUEPRINT 7.1 对应，
//! 变更必须同批修订蓝图。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 后端统一错误。后续批次扩展为细分错误（journal/加密/卷表）时保持本枚举向后兼容。
#[derive(Debug)]
pub enum ContainerError {
    NotFound(String),
    InvalidPath(String),
    Io(io::Error),
    NotImplemented(&'static str),
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerError::NotFound(p) => write!(f, "路径不存在: {p}"),
            ContainerError::InvalidPath(p) => write!(f, "非法路径（越界或含保留段）: {p}"),
            ContainerError::Io(e) => write!(f, "IO 错误: {e}"),
            ContainerError::NotImplemented(what) => write!(f, "未实现（后端骨架）: {what}"),
        }
    }
}

impl std::error::Error for ContainerError {}

impl From<io::Error> for ContainerError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::NotFound {
            ContainerError::NotFound(String::new())
        } else {
            ContainerError::Io(e)
        }
    }
}

pub type CmdResult<T> = Result<T, ContainerError>;

/// 容器内相对路径（`/` 分隔，禁止 `..`、盘符、绝对前缀）。
/// 所有后端只接受 VPath——越界访问在类型层被挡住，而不是靠后端自觉。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VPath(String);

impl VPath {
    pub fn new(raw: &str) -> CmdResult<Self> {
        let raw = raw.trim().trim_start_matches('/');
        if raw.is_empty() {
            return Err(ContainerError::InvalidPath(raw.to_string()));
        }
        let mut normalized = String::with_capacity(raw.len());
        for seg in raw.split('/') {
            match seg {
                "" | "." => {}
                ".." => return Err(ContainerError::InvalidPath(raw.to_string())),
                s => {
                    if !normalized.is_empty() {
                        normalized.push('/');
                    }
                    normalized.push_str(s);
                }
            }
        }
        if normalized.is_empty() {
            return Err(ContainerError::InvalidPath(raw.to_string()));
        }
        Ok(VPath(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for VPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 打开配置。`root` 对 DirBackend 是数据目录；对 Uxv 后端（后续批次）是容器文件路径。
#[derive(Debug, Clone)]
pub struct OpenCfg {
    pub root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct StatInfo {
    pub path: VPath,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// 快照标识。DirBackend 骨架实现为目录整拷贝；Uxv 后端将升级为 chunk 级 COW（B-26）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotId(pub String);

#[derive(Debug, Default, Clone)]
pub struct GcReport {
    pub chunks_examined: u64,
    pub bytes_reclaimed: u64,
}

/// L1 统一存储契约（BLUEPRINT 7.1）。三后端（Dir/Vhdx/Uxv）实现同一接口。
///
/// 语义约定：
/// - `write` 是事务语义（DirBackend 下即原子写：临时文件 + rename）；
/// - `seal` 是退出冲刷唯一强制刷盘点（运行中绝不逐事务全刷，附录 A 14.1）；
/// - 所有路径入参必须是 VPath，后端不得接受逃逸出容器根的任何形式。
pub trait StorageBackend: Send {
    fn open(&mut self, cfg: &OpenCfg) -> CmdResult<()>;
    fn stat(&self, path: &VPath) -> CmdResult<StatInfo>;
    fn read(&self, path: &VPath) -> CmdResult<Vec<u8>>;
    fn write(&mut self, path: &VPath, data: &[u8]) -> CmdResult<()>;
    fn list(&self, dir: &VPath) -> CmdResult<Vec<StatInfo>>;
    fn mkdir(&mut self, dir: &VPath) -> CmdResult<()>;
    fn rm(&mut self, path: &VPath) -> CmdResult<()>;
    fn rename(&mut self, from: &VPath, to: &VPath) -> CmdResult<()>;
    fn copy(&mut self, from: &VPath, to: &VPath) -> CmdResult<()>;
    fn snapshot(&mut self, label: &str) -> CmdResult<SnapshotId>;
    fn restore(&mut self, id: &SnapshotId) -> CmdResult<()>;
    fn gc(&mut self, _budget_ms: u32) -> CmdResult<GcReport> {
        Ok(GcReport::default())
    }
    fn seal(&mut self) -> CmdResult<()>;
}

/// 现状目录后端：把一个真实目录包装为容器视图（行为等价迁移的保底档）。
pub struct DirBackend {
    root: Option<PathBuf>,
}

impl DirBackend {
    pub fn new() -> Self {
        DirBackend { root: None }
    }

    fn root(&self) -> CmdResult<&Path> {
        self.root
            .as_deref()
            .ok_or(ContainerError::NotImplemented("open 未调用"))
    }

    /// VPath → 容器根内绝对路径。逐段校验，杜绝反斜杠/盘符类逃逸（纵深防御）。
    fn resolve(&self, path: &VPath) -> CmdResult<PathBuf> {
        let root = self.root()?;
        let mut out = root.to_path_buf();
        for seg in path.as_str().split('/') {
            if seg.is_empty() || seg.contains(['\\', ':', '/']) {
                return Err(ContainerError::InvalidPath(path.to_string()));
            }
            out.push(seg);
        }
        Ok(out)
    }

    fn stat_of(&self, path: &VPath, target: &Path) -> CmdResult<StatInfo> {
        let meta = fs::metadata(target).map_err(|_| ContainerError::NotFound(path.to_string()))?;
        Ok(StatInfo {
            path: path.clone(),
            is_dir: meta.is_dir(),
            size: meta.len(),
            modified: meta.modified().ok(),
        })
    }

    /// 原子写：临时文件 + 同卷 rename（崩溃时最多留下临时文件，目标不半新半旧）。
    fn atomic_write(target: &Path, data: &[u8]) -> CmdResult<()> {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = target.with_extension("tmp-bench");
        fs::write(&tmp, data)?;
        fs::rename(&tmp, target)?;
        Ok(())
    }
}

impl Default for DirBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for DirBackend {
    fn open(&mut self, cfg: &OpenCfg) -> CmdResult<()> {
        fs::create_dir_all(&cfg.root)?;
        self.root = Some(cfg.root.clone());
        Ok(())
    }

    fn stat(&self, path: &VPath) -> CmdResult<StatInfo> {
        let target = self.resolve(path)?;
        self.stat_of(path, &target)
    }

    fn read(&self, path: &VPath) -> CmdResult<Vec<u8>> {
        let target = self.resolve(path)?;
        if target.is_dir() {
            return Err(ContainerError::InvalidPath(path.to_string()));
        }
        fs::read(&target).map_err(|_| ContainerError::NotFound(path.to_string()))
    }

    fn write(&mut self, path: &VPath, data: &[u8]) -> CmdResult<()> {
        let target = self.resolve(path)?;
        Self::atomic_write(&target, data)
    }

    fn list(&self, dir: &VPath) -> CmdResult<Vec<StatInfo>> {
        let target = self.resolve(dir)?;
        if !target.is_dir() {
            return Err(ContainerError::NotFound(dir.to_string()));
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(&target)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let child = VPath::new(&format!("{dir}/{name}"))?;
            out.push(self.stat_of(&child, &entry.path())?);
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn mkdir(&mut self, dir: &VPath) -> CmdResult<()> {
        Ok(fs::create_dir_all(self.resolve(dir)?)?)
    }

    fn rm(&mut self, path: &VPath) -> CmdResult<()> {
        let target = self.resolve(path)?;
        if !target.exists() {
            return Err(ContainerError::NotFound(path.to_string()));
        }
        if target.is_dir() {
            fs::remove_dir_all(&target)?;
        } else {
            fs::remove_file(&target)?;
        }
        Ok(())
    }

    fn rename(&mut self, from: &VPath, to: &VPath) -> CmdResult<()> {
        let src = self.resolve(from)?;
        let dst = self.resolve(to)?;
        if !src.exists() {
            return Err(ContainerError::NotFound(from.to_string()));
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(fs::rename(&src, &dst)?)
    }

    fn copy(&mut self, from: &VPath, to: &VPath) -> CmdResult<()> {
        let src = self.resolve(from)?;
        let dst = self.resolve(to)?;
        if !src.exists() {
            return Err(ContainerError::NotFound(from.to_string()));
        }
        copy_recursive(&src, &dst)
    }

    /// 骨架实现：整目录拷贝到 `<root>/.snapshots/<label>/`。
    /// Uxv 后端（B-13/B-26）将替换为 journal 事务 + chunk 级 COW。
    fn snapshot(&mut self, label: &str) -> CmdResult<SnapshotId> {
        let root = self.root()?.to_path_buf();
        let snap_dir = root.join(".snapshots").join(sanitize_label(label));
        if snap_dir.exists() {
            fs::remove_dir_all(&snap_dir)?;
        }
        fs::create_dir_all(&snap_dir)?;
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let name = entry.file_name();
            if name == ".snapshots" {
                continue; // 快照不嵌套快照
            }
            copy_recursive(&entry.path(), &snap_dir.join(&name))?;
        }
        Ok(SnapshotId(sanitize_label(label)))
    }

    fn restore(&mut self, id: &SnapshotId) -> CmdResult<()> {
        let root = self.root()?.to_path_buf();
        let snap_dir = root.join(".snapshots").join(sanitize_label(&id.0));
        if !snap_dir.is_dir() {
            return Err(ContainerError::NotFound(id.0.clone()));
        }
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if entry.file_name() == ".snapshots" {
                continue;
            }
            if entry.path().is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        for entry in fs::read_dir(&snap_dir)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &root.join(entry.file_name()))?;
        }
        Ok(())
    }

    fn seal(&mut self) -> CmdResult<()> {
        // DirBackend 无 journal/写缓冲，暂无额外冲刷语义；
        // Uxv 后端在此执行 FlushFileBuffers + Footer 双副本（B-13）。
        self.root()?;
        Ok(())
    }
}

fn sanitize_label(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned
    }
}

fn copy_recursive(src: &Path, dst: &Path) -> CmdResult<()> {
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(fs::copy(src, dst).map(|_| ())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "container-test-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn opened(tag: &str) -> (DirBackend, PathBuf) {
        let root = temp_root(tag);
        let mut be = DirBackend::new();
        be.open(&OpenCfg { root: root.clone() }).unwrap();
        (be, root)
    }

    #[test]
    fn vpath_rejects_escape_and_normalizes() {
        assert!(VPath::new("../../etc/passwd").is_err());
        assert!(VPath::new("a/./b//c").unwrap().as_str() == "a/b/c");
        assert!(VPath::new("").is_err());
    }

    #[test]
    fn write_read_roundtrip_is_atomic() {
        let (mut be, _root) = opened("rw");
        be.write(&VPath::new("db/state.json").unwrap(), b"hello").unwrap();
        assert_eq!(be.read(&VPath::new("db/state.json").unwrap()).unwrap(), b"hello");
        // 覆盖写
        be.write(&VPath::new("db/state.json").unwrap(), b"world!").unwrap();
        assert_eq!(be.read(&VPath::new("db/state.json").unwrap()).unwrap(), b"world!");
    }

    #[test]
    fn read_missing_fails_cleanly() {
        let (be, _root) = opened("missing");
        assert!(matches!(
            be.read(&VPath::new("nope.txt").unwrap()),
            Err(ContainerError::NotFound(_))
        ));
    }

    #[test]
    fn list_mkdir_rename_rm_flow() {
        let (mut be, root) = opened("flow");
        be.mkdir(&VPath::new("media/videos").unwrap()).unwrap();
        be.write(&VPath::new("media/videos/a.txt").unwrap(), b"1").unwrap();
        be.write(&VPath::new("media/videos/b.txt").unwrap(), b"22").unwrap();

        let items = be.list(&VPath::new("media/videos").unwrap()).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].path.as_str(), "media/videos/a.txt");
        assert_eq!(items[1].size, 2);

        be.rename(&VPath::new("media/videos/a.txt").unwrap(), &VPath::new("media/videos/c.txt").unwrap()).unwrap();
        assert!(be.read(&VPath::new("media/videos/c.txt").unwrap()).is_ok());

        be.rm(&VPath::new("media").unwrap()).unwrap();
        assert!(!root.join("media").exists());
        assert!(matches!(
            be.read(&VPath::new("media/videos/c.txt").unwrap()),
            Err(ContainerError::NotFound(_))
        ));
    }

    #[test]
    fn copy_duplicates_tree() {
        let (mut be, root) = opened("copy");
        be.write(&VPath::new("work/a.txt").unwrap(), b"data").unwrap();
        be.mkdir(&VPath::new("work/sub").unwrap()).unwrap();
        be.write(&VPath::new("work/sub/b.txt").unwrap(), b"x").unwrap();
        be.copy(&VPath::new("work").unwrap(), &VPath::new("backup/work").unwrap()).unwrap();
        assert_eq!(be.read(&VPath::new("backup/work/sub/b.txt").unwrap()).unwrap(), b"x");
        let _ = root;
    }

    #[test]
    fn snapshot_restore_roundtrip() {
        let (mut be, _root) = opened("snap");
        be.write(&VPath::new("db/k.txt").unwrap(), b"v1").unwrap();
        let id = be.snapshot("pre-migrate-v1").unwrap();
        be.write(&VPath::new("db/k.txt").unwrap(), b"v2").unwrap();
        be.rm(&VPath::new("db").unwrap()).unwrap();
        be.restore(&id).unwrap();
        assert_eq!(be.read(&VPath::new("db/k.txt").unwrap()).unwrap(), b"v1");
    }

    #[test]
    fn operations_before_open_fail() {
        let be = DirBackend::new();
        assert!(matches!(
            be.read(&VPath::new("x").unwrap()),
            Err(ContainerError::NotImplemented(_))
        ));
    }

    #[test]
    fn seal_is_noop_but_requires_open() {
        let mut be = DirBackend::new();
        assert!(be.seal().is_err());
        let root = temp_root("seal");
        be.open(&OpenCfg { root }).unwrap();
        assert!(be.seal().is_ok());
    }
}
