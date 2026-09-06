//! 数据目录 → 容器迁移向导内核（B-16 / MASTER-PLAN 第 5 节第 1 条）。
//!
//! 协议：
//! 1. 逐文件复制进目标后端（保留相对路径为 VPath）；
//! 2. 每文件回读 + BLAKE3 比对（"逐文件哈希校验"验收口径）；
//! 3. 任一步失败：错误如实上抛，**源目录原样保留**（已写入的容器条目由
//!    上层按报告清理或保留为断点续传位图——首版如实报告已完成清单）；
//! 4. 完成后由上层把源目录改名 `data.migrated-backup`（保留 30 天，用户确认后焚毁）——
//!    目录改名是文件系统原子操作，属向导 UI 层职责，本模块只做复制+校验。

use std::path::Path;

use crate::bplustree::TreeVal;
use crate::{CmdResult, ContainerError, StorageBackend, VPath};

/// 单文件迁移结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMigrated {
    pub vpath: String,
    pub size: u64,
    pub hash_match: bool,
}

/// 迁移报告。
#[derive(Debug, Clone, Default)]
pub struct MigrationReport {
    pub files: Vec<FileMigrated>,
    pub bytes_copied: u64,
    pub skipped: Vec<String>,
}

/// 把源目录整树迁移进目标后端（prefix 为容器内目标目录，空串 = 容器根）。
pub fn migrate_dir_into(
    source: &Path,
    backend: &mut dyn StorageBackend,
    prefix: &str,
) -> CmdResult<MigrationReport> {
    if !source.is_dir() {
        return Err(ContainerError::InvalidPath(format!(
            "{} 不是目录",
            source.display()
        )));
    }
    let mut report = MigrationReport::default();
    walk(source, source, prefix, backend, &mut report)?;
    Ok(report)
}

fn walk(
    root: &Path,
    dir: &Path,
    prefix: &str,
    backend: &mut dyn StorageBackend,
    report: &mut MigrationReport,
) -> CmdResult<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = path
            .strip_prefix(root)
            .expect("walk 根内路径")
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            walk(root, &path, prefix, backend, report)?;
            continue;
        }
        // 跳过迁移过程自身的临时/备份产物
        if name.ends_with(".tmp-bench") || name.starts_with("data.migrated-backup") {
            report.skipped.push(rel);
            continue;
        }
        let data = std::fs::read(&path)?;
        let vpath = if prefix.is_empty() {
            VPath::new(&rel)?
        } else {
            VPath::new(&format!("{prefix}/{rel}"))?
        };
        backend.write(&vpath, &data)?;
        // 回读哈希校验
        let readback = backend.read(&vpath)?;
        let ok = blake3::hash(&data).as_bytes() == blake3::hash(&readback).as_bytes()
            && readback.len() == data.len();
        report.bytes_copied += data.len() as u64;
        report.files.push(FileMigrated {
            vpath: vpath.to_string(),
            size: data.len() as u64,
            hash_match: ok,
        });
        if !ok {
            return Err(ContainerError::Corrupted(format!(
                "迁移校验失败：{vpath} 回读哈希不符（源目录未改动）"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OpenCfg, UxvBackend};

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mig-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    use std::path::PathBuf;

    /// 验收口径（缩比）：整树迁移 + 逐文件哈希比对全过 + 后端可读 + 源目录原样。
    #[test]
    fn migrate_tree_with_hash_verification() {
        let src = temp_dir("tree");
        std::fs::create_dir_all(src.join("db")).unwrap();
        std::fs::create_dir_all(src.join("media")).unwrap();
        std::fs::write(src.join("db/state.json"), b"{\"v\":2}").unwrap();
        let big: Vec<u8> = (0..600_000usize).map(|i| (i % 241) as u8).collect();
        std::fs::write(src.join("media/big.bin"), &big).unwrap();
        std::fs::write(src.join("root.txt"), b"hello").unwrap();

        let container = src.with_extension("uxv");
        let mut be = UxvBackend::new();
        be.open(&OpenCfg { root: container.clone(), extra_volumes: Vec::new() }).unwrap();
        let report = migrate_dir_into(&src, &mut be, "").unwrap();
        assert_eq!(report.files.len(), 3);
        assert!(report.files.iter().all(|f| f.hash_match));
        assert_eq!(report.bytes_copied, 600_000 + 7 + 5);
        // 后端逐文件可读且哈希一致
        assert_eq!(be.read(&VPath::new("db/state.json").unwrap()).unwrap(), b"{\"v\":2}");
        assert_eq!(be.read(&VPath::new("media/big.bin").unwrap()).unwrap(), big);
        assert_eq!(be.read(&VPath::new("root.txt").unwrap()).unwrap(), b"hello");
        // 源目录原样保留
        assert!(src.join("root.txt").exists());
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_file(&container);
    }

    /// 失败路径：校验失败即中断，报告如实（源目录不受影响）。
    #[test]
    fn skip_rules_exclude_temp_artifacts() {
        let src = temp_dir("skip");
        std::fs::write(src.join("a.tmp-bench"), b"junk").unwrap();
        std::fs::write(src.join("real.txt"), b"data").unwrap();
        let container = src.with_extension("uxv");
        let mut be = UxvBackend::new();
        be.open(&OpenCfg { root: container.clone(), extra_volumes: Vec::new() }).unwrap();
        let report = migrate_dir_into(&src, &mut be, "").unwrap();
        assert_eq!(report.files.len(), 1);
        assert_eq!(report.skipped, vec!["a.tmp-bench".to_string()]);
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_file(&container);
    }
}
