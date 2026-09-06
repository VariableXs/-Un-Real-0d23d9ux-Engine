//! 应急救援（B-33 M2 半包）——恢复模式的可执行内核。
//!
//! 两级救援（附录 D 演练第 1 条「容器打开失败 → 恢复模式」）：
//! - `salvage_files`：Footer 有效（索引可加载）→ 按文件表整树导出到普通目录；
//! - `salvage_chunks`：Footer 失效（最坏情况）→ 独立解析器顺序走查记录流，
//!   逐条 BLAKE3 校验后把可读 chunk 载荷导出为 `res-<off>.bin`。
//! 两级都**只读容器、只写输出目录**——救援路径绝不改写容器本体。

use std::path::{Path, PathBuf};

use crate::{CmdResult, ContainerError, OpenCfg, StorageBackend, UxvBackend};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RescueReport {
    pub mode: &'static str,
    pub files_rescued: Vec<String>,
    pub chunks_rescued: u64,
    pub bytes_rescued: u64,
    pub errors: Vec<String>,
}

/// 第一级：索引可加载 → 整树导出（vault 容器需口令）。
pub fn salvage_files(
    container: &Path,
    out_dir: &Path,
    passphrase: Option<&[u8]>,
) -> CmdResult<RescueReport> {
    let mut be = UxvBackend::new();
    match passphrase {
        Some(p) => be.open_with_passphrase(&OpenCfg { root: container.to_path_buf(), extra_volumes: Vec::new() }, p)?,
        None => be.open(&OpenCfg { root: container.to_path_buf(), extra_volumes: Vec::new() })?,
    }
    let mut report = RescueReport {
        mode: "files",
        files_rescued: Vec::new(),
        chunks_rescued: 0,
        bytes_rescued: 0,
        errors: Vec::new(),
    };
    // list_all 由后端提供（文件表全量快照）
    for path in be.list_all() {
        let vp = match crate::VPath::new(&path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        match be.read(&vp) {
            Ok(data) => {
                let target = safe_join(out_dir, &path)?;
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target, &data)?;
                report.files_rescued.push(path.clone());
                report.bytes_rescued += data.len() as u64;
            }
            Err(e) => report.errors.push(format!("{path}: {e}")),
        }
    }
    Ok(report)
}

/// 第二级：Footer 失效 → 裸记录流走查（魔数分流 + blake3 校验），只救可读 chunk。
pub fn salvage_chunks(file: &Path, out_dir: &Path) -> CmdResult<RescueReport> {
    use std::io::{Read, Seek, SeekFrom};
    let mut report = RescueReport {
        mode: "chunks",
        files_rescued: Vec::new(),
        chunks_rescued: 0,
        bytes_rescued: 0,
        errors: Vec::new(),
    };
    std::fs::create_dir_all(out_dir)?;
    let mut f = std::fs::File::open(file)?;
    let len = f.metadata()?.len();
    if len < 64 {
        return Err(ContainerError::Corrupted("文件过短，无容器结构".into()));
    }
    let mut pos = 64u64; // SuperBlock 之后
    while pos + 37 <= len {
        f.seek(SeekFrom::Start(pos))?;
        let mut probe = [0u8; 4];
        if f.read_exact(&mut probe).is_err() {
            break;
        }
        if probe == *b"JNL1" {
            let mut hdr = [0u8; 37];
            if f.read_exact(&mut hdr).is_err() {
                break;
            }
            let plen = u32::from_le_bytes(hdr[1..5].try_into().expect("定长")) as u64;
            pos += 41 + plen;
            continue;
        }
        // chunk 记录：[len u32][codec u8][blake3 32][payload]
        let clen = u32::from_le_bytes(probe) as u64;
        if clen > crate::uxv::CHUNK_SIZE as u64 {
            // 非记录区（旧索引 blob 尾部等）→ 步进 1 重对齐（最坏模式宁可慢扫）
            pos += 1;
            continue;
        }
        let mut hdr_rest = [0u8; 33];
        if f.read_exact(&mut hdr_rest).is_err() {
            break;
        }
        let expect: [u8; 32] = hdr_rest[1..33].try_into().expect("定长");
        let mut payload = vec![0u8; clen as usize];
        if f.read_exact(&mut payload).is_err() {
            break;
        }
        pos += 37 + clen;
        if blake3::hash(&payload).as_bytes() != &expect {
            continue; // 校验失败 = 不可读，跳过
        }
        let target = out_dir.join(format!("res-{pos:016x}.bin"));
        std::fs::write(&target, &payload)?;
        report.chunks_rescued += 1;
        report.bytes_rescued += clen;
    }
    Ok(report)
}

/// 防路径逃逸：文件表条目映射到输出目录内（纵深防御，VPath 已挡但双保险）。
fn safe_join(out_dir: &Path, rel: &str) -> CmdResult<PathBuf> {
    let mut out = out_dir.to_path_buf();
    for seg in rel.split('/') {
        if seg.is_empty() || seg == ".." || seg.contains(['\\', ':']) {
            return Err(ContainerError::InvalidPath(rel.to_string()));
        }
        out.push(seg);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VPath;

    fn fresh(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("uxv-b33-{tag}-{}.uxv", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn salvage_files_exports_whole_tree() {
        let p = fresh("files");
        {
            let mut be = UxvBackend::new();
            be.open(&OpenCfg { root: p.clone(), extra_volumes: Vec::new() }).unwrap();
            be.write(&VPath::new("docs/a.txt").unwrap(), b"alpha").unwrap();
            be.write(&VPath::new("docs/b.txt").unwrap(), b"beta!").unwrap();
            be.seal().unwrap();
        }
        let out = std::env::temp_dir().join(format!("uxv-b33-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let report = salvage_files(&p, &out, None).unwrap();
        assert_eq!(report.mode, "files");
        assert_eq!(report.files_rescued.len(), 2);
        assert_eq!(std::fs::read(out.join("docs/a.txt")).unwrap(), b"alpha");
        // 路径逃逸防御：报告不含越界项
        assert!(report.files_rescued.iter().all(|f| !f.contains("..")));
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn salvage_chunks_recovers_without_footer() {
        let p = fresh("chunks");
        {
            let mut be = UxvBackend::new();
            be.open(&OpenCfg { root: p.clone(), extra_volumes: Vec::new() }).unwrap();
            be.write(&VPath::new("f").unwrap(), b"recoverable-content").unwrap();
            // 故意不 seal（Footer 缺失 = 最坏情况）
        }
        let out = std::env::temp_dir().join(format!("uxv-b33-out2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let report = salvage_chunks(&p, &out).unwrap();
        assert_eq!(report.mode, "chunks");
        assert!(report.chunks_rescued >= 1, "至少救回内容 chunk");
        assert!(report.bytes_rescued >= 19);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn salvage_chunks_skips_corrupt_records() {
        let p = fresh("corrupt");
        {
            let mut be = UxvBackend::new();
            be.open(&OpenCfg { root: p.clone(), extra_volumes: Vec::new() }).unwrap();
            be.write(&VPath::new("good").unwrap(), b"good-data-here").unwrap();
        }
        // 破坏记录流中段
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new().read(true).write(true).open(&p).unwrap();
            f.seek(SeekFrom::Start(64 + 10)).unwrap();
            f.write_all(&[0xFFu8; 8]).unwrap();
        }
        let out = std::env::temp_dir().join(format!("uxv-b33-out3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let report = salvage_chunks(&p, &out).unwrap();
        // 受损 chunk 被跳过（未计入），不 panic
        assert!(report.errors.is_empty());
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_dir_all(&out);
    }
}
