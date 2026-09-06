//! UxvBackend — 自研单文件容器（BLUEPRINT 3.1 终态档，B-12 chunk 层）。
//!
//! 文件布局（schemaVersion=1）：
//! ```text
//! [SuperBlock 64B][追加区：chunk 记录 + index checkpoint blob][Footer A][Footer B]
//! ```
//! - chunk 记录：`[len u32][codec u8][blake3 32B][payload]`，内容寻址；文件按 4MiB
//!   定长切分（小文件靠段内连续打包合并，大文件天然按 4MiB 流式）；
//! - 去重：同内容 chunk 全容器只存一份，引用计数在 ChunkIndex 内维护；
//! - 索引：文件表（VPath → FileInfo）与 ChunkIndex（HashKey → ChunkLoc）两棵 B+ 树，
//!   seal/checkpoint 时序列化为 blob 追加，Footer 双副本记录位置与 BLAKE3 校验；
//! - 快照：追加区永不覆盖 ⇒ 索引状态拷贝即时间点快照，restore 零成本（chunk 仍在）；
//! - 如实边界（B-13 补齐）：写路径尚无 journal——未 seal 即断电会丢失最近一次
//!   checkpoint 之后的事务，打开时报 Corrupted；压缩/加密由 B-14 扩展 codec 字段。
//!
//! 契约纪律：trait 签名 = BLUEPRINT 7.1（本批新增 read_range/stream，与蓝图同提交）。

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::bplustree::{BPlusTree, HashKey, TreeVal};
use crate::{sanitize_label, CmdResult, ContainerError, GcReport, OpenCfg, SnapshotId, StatInfo, StorageBackend, VPath};

/// 大文件流式切分尺寸（蓝图 3.1：4MiB）。
pub const CHUNK_SIZE: usize = 4 * 1024 * 1024;
const SUPERBLOCK_LEN: u64 = 64;
const FOOTER_LEN: usize = 96;
const CHUNK_HDR_LEN: usize = 37; // len(4) + codec(1) + blake3(32)
const MAGIC: &[u8; 8] = b"UXVSTR01";
/// schemaVersion（B-31 迁移协议在此字段上演进）。
pub const SCHEMA_VERSION: u32 = 1;
/// codec 0 = 原样存储（B-14 扩展 LZ4/Zstd/XChaCha20 编号）。
pub const CODEC_RAW: u8 = 0;

// ---------- 索引值类型 ----------

/// 文件条目：按序 chunk 清单（4MiB 定长切分，末尾 chunk 可短）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub size: u64,
    pub mtime_ms: u64,
    pub chunks: Vec<[u8; 32]>,
}

impl TreeVal for FileInfo {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.size.to_le_bytes());
        out.extend_from_slice(&self.mtime_ms.to_le_bytes());
        out.extend_from_slice(&(self.chunks.len() as u32).to_le_bytes());
        for c in &self.chunks {
            out.extend_from_slice(c);
        }
    }
    fn decode(buf: &[u8], pos: &mut usize) -> Option<Self> {
        let rd = |n: usize, p: &mut usize| -> Option<Vec<u8>> {
            let b = buf.get(*p..*p + n)?.to_vec();
            *p += n;
            Some(b)
        };
        let size = u64::from_le_bytes(rd(8, pos)?.try_into().ok()?);
        let mtime_ms = u64::from_le_bytes(rd(8, pos)?.try_into().ok()?);
        let n = u32::from_le_bytes(rd(4, pos)?.try_into().ok()?) as usize;
        let mut chunks = Vec::with_capacity(n);
        for _ in 0..n {
            let mut c = [0u8; 32];
            c.copy_from_slice(&rd(32, pos)?);
            chunks.push(c);
        }
        Some(FileInfo { size, mtime_ms, chunks })
    }
}

/// chunk 位置：容器内偏移 + 载荷长度 + codec + 引用计数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkLoc {
    pub offset: u64,
    pub len: u32,
    pub codec: u8,
    pub refs: u64,
}

impl TreeVal for ChunkLoc {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.offset.to_le_bytes());
        out.extend_from_slice(&self.len.to_le_bytes());
        out.push(self.codec);
        out.extend_from_slice(&self.refs.to_le_bytes());
    }
    fn decode(buf: &[u8], pos: &mut usize) -> Option<Self> {
        let rd = |n: usize, p: &mut usize| -> Option<Vec<u8>> {
            let b = buf.get(*p..*p + n)?.to_vec();
            *p += n;
            Some(b)
        };
        let offset = u64::from_le_bytes(rd(8, pos)?.try_into().ok()?);
        let len = u32::from_le_bytes(rd(4, pos)?.try_into().ok()?);
        let codec = *rd(1, pos)?.first()?;
        let refs = u64::from_le_bytes(rd(8, pos)?.try_into().ok()?);
        Some(ChunkLoc { offset, len, codec, refs })
    }
}

// ---------- Footer ----------

#[derive(Debug, Clone, PartialEq, Eq)]
struct Footer {
    schema_version: u32,
    index_offset: u64,
    index_len: u64,
    index_hash: [u8; 32],
}

impl Footer {
    fn encode(&self) -> [u8; FOOTER_LEN] {
        let mut b = [0u8; FOOTER_LEN];
        b[0..4].copy_from_slice(&self.schema_version.to_le_bytes());
        b[4..12].copy_from_slice(&self.index_offset.to_le_bytes());
        b[12..20].copy_from_slice(&self.index_len.to_le_bytes());
        b[20..52].copy_from_slice(&self.index_hash);
        b[52..60].copy_from_slice(MAGIC);
        b
    }

    fn decode(b: &[u8]) -> Option<Footer> {
        if b.len() < FOOTER_LEN || &b[52..60] != MAGIC {
            return None;
        }
        Some(Footer {
            schema_version: u32::from_le_bytes(b[0..4].try_into().ok()?),
            index_offset: u64::from_le_bytes(b[4..12].try_into().ok()?),
            index_len: u64::from_le_bytes(b[12..20].try_into().ok()?),
            index_hash: b[20..52].try_into().ok()?,
        })
    }
}

// ---------- 流式读取 ----------

/// 大文件流句柄（Read + Seek），打开时定位 0。
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

struct ChunkStreamReader {
    file: File,
    chunks: Vec<ChunkLoc>,
    logical: u64,
    size: u64,
}

impl Read for ChunkStreamReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.logical >= self.size || buf.is_empty() {
            return Ok(0);
        }
        let chunk_idx = (self.logical / CHUNK_SIZE as u64) as usize;
        let within = (self.logical % CHUNK_SIZE as u64) as usize;
        let loc = self.chunks.get(chunk_idx).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "chunk 清单越界")
        })?;
        let want = buf
            .len()
            .min(loc.len as usize - within)
            .min((self.size - self.logical) as usize);
        self.file.seek(SeekFrom::Start(loc.offset + CHUNK_HDR_LEN as u64 + within as u64))?;
        self.file.read_exact(&mut buf[..want])?;
        self.logical += want as u64;
        Ok(want)
    }
}

impl Seek for ChunkStreamReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let target: i64 = match pos {
            SeekFrom::Start(o) => o as i64,
            SeekFrom::End(o) => self.size as i64 + o,
            SeekFrom::Current(o) => self.logical as i64 + o,
        };
        self.logical = target.clamp(0, self.size as i64) as u64;
        Ok(self.logical)
    }
}

// ---------- 后端本体 ----------

pub struct UxvBackend {
    path: Option<PathBuf>,
    file: Option<std::cell::RefCell<File>>,
    data_tail: u64,
    files: BPlusTree<String, FileInfo>,
    chunks: BPlusTree<HashKey, ChunkLoc>,
    snapshots: std::collections::HashMap<String, SnapshotState>,
}

#[derive(Clone)]
struct SnapshotState {
    data_tail: u64,
    files: BPlusTree<String, FileInfo>,
    chunks: BPlusTree<HashKey, ChunkLoc>,
}

impl UxvBackend {
    pub fn new() -> Self {
        UxvBackend {
            path: None,
            file: None,
            data_tail: SUPERBLOCK_LEN,
            files: BPlusTree::new(),
            chunks: BPlusTree::new(),
            snapshots: std::collections::HashMap::new(),
        }
    }

    fn handle(&self) -> CmdResult<&std::cell::RefCell<File>> {
        self.file
            .as_ref()
            .ok_or(ContainerError::NotImplemented("open 未调用"))
    }

    /// 追加一个 chunk（头部 + 载荷），返回其位置。
    fn append_chunk(&mut self, hash: [u8; 32], payload: &[u8]) -> CmdResult<ChunkLoc> {
        let offset = self.data_tail;
        {
            let f = self.handle()?;
            let mut f = f.borrow_mut();
            f.seek(SeekFrom::Start(offset))?;
            f.write_all(&(payload.len() as u32).to_le_bytes())?;
            f.write_all(&[CODEC_RAW])?;
            f.write_all(&hash)?;
            f.write_all(payload)?;
        }
        self.data_tail += CHUNK_HDR_LEN as u64 + payload.len() as u64;
        Ok(ChunkLoc { offset, len: payload.len() as u32, codec: CODEC_RAW, refs: 1 })
    }

    fn read_chunk_payload(&self, loc: &ChunkLoc) -> CmdResult<Vec<u8>> {
        let f = self.handle()?;
        let mut f = f.borrow_mut();
        f.seek(SeekFrom::Start(loc.offset))?;
        let mut hdr = [0u8; CHUNK_HDR_LEN];
        f.read_exact(&mut hdr)?;
        let len = u32::from_le_bytes(hdr[0..4].try_into().expect("定长"));
        let codec = hdr[4];
        let expect: [u8; 32] = hdr[5..37].try_into().expect("定长");
        if len != loc.len || codec != loc.codec {
            return Err(ContainerError::Corrupted(format!(
                "chunk 头部与索引不符（off={}）",
                loc.offset
            )));
        }
        let mut payload = vec![0u8; len as usize];
        f.read_exact(&mut payload)?;
        let got = blake3::hash(&payload);
        if got.as_bytes() != &expect {
            return Err(ContainerError::Corrupted(format!(
                "chunk 校验失败（off={}，期望 {}，实际 {got}）",
                loc.offset,
                HashKey(expect)
            )));
        }
        Ok(payload)
    }

    /// 引用减一；归零即摘除索引项（物理空间由 GC/B-15 回收，本批已知取舍）。
    fn unref_chunk(&mut self, hash: [u8; 32]) {
        let zeroed = match self.chunks.get_mut(&HashKey(hash)) {
            Some(loc) => {
                loc.refs -= 1;
                loc.refs == 0
            }
            None => return,
        };
        if zeroed {
            self.chunks.remove(&HashKey(hash));
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn stat_of(&self, path: &VPath, info: &FileInfo) -> StatInfo {
        StatInfo {
            path: path.clone(),
            is_dir: false,
            size: info.size,
            modified: Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(info.mtime_ms)),
        }
    }

    /// 前缀下的直接子项（隐式目录模型：目录由文件路径派生）。
    fn children_of(all: &[(String, FileInfo)], dir: &str) -> Vec<(String, bool, u64)> {
        let mut out: std::collections::BTreeMap<String, (bool, u64)> = Default::default();
        for (p, info) in all {
            if !p.starts_with(dir) {
                continue;
            }
            let rest = p[dir.len()..].trim_start_matches('/');
            if rest.is_empty() {
                continue;
            }
            match rest.split_once('/') {
                Some((seg, _)) => {
                    out.insert(seg.to_string(), (true, 0));
                }
                None => {
                    out.insert(rest.to_string(), (false, info.size));
                }
            }
        }
        out.into_iter().map(|(n, (d, s))| (n, d, s)).collect()
    }

    // ---------- 索引持久化 ----------

    /// 两棵树序列化追加进容器 + Footer 双副本（seal 共用）。
    fn checkpoint(&mut self) -> CmdResult<()> {
        let files_blob = self.files.encode();
        let mut blob = (files_blob.len() as u64).to_le_bytes().to_vec();
        blob.extend_from_slice(&files_blob);
        blob.extend_from_slice(&self.chunks.encode());
        let hash = blake3::hash(&blob);
        let index_offset = self.data_tail;
        {
            let f = self.handle()?;
            let mut f = f.borrow_mut();
            f.seek(SeekFrom::Start(index_offset))?;
            f.write_all(&blob)?;
            let footer = Footer {
                schema_version: SCHEMA_VERSION,
                index_offset,
                index_len: blob.len() as u64,
                index_hash: *hash.as_bytes(),
            }
            .encode();
            f.write_all(&footer)?; // 副本 A
            f.write_all(&footer)?; // 副本 B
            f.flush()?;
            f.sync_all()?; // FlushFileBuffers 语义（附录 A 14.1）
        }
        self.data_tail += blob.len() as u64 + FOOTER_LEN as u64 * 2;
        Ok(())
    }

    /// 打开时从 Footer 双副本定位并校验最近一次 checkpoint 的索引。
    /// 返回 (索引起点 = 容器逻辑尾, 文件表, ChunkIndex)。
    fn load_index(
        f: &mut File,
    ) -> CmdResult<(u64, BPlusTree<String, FileInfo>, BPlusTree<HashKey, ChunkLoc>)> {
        let len = f.metadata()?.len();
        if len < SUPERBLOCK_LEN + FOOTER_LEN as u64 * 2 {
            return Err(ContainerError::Corrupted(
                "容器缺少 Footer（未正常 seal；journal 恢复属 B-13）".into(),
            ));
        }
        f.seek(SeekFrom::End(-(FOOTER_LEN as i64) * 2))?;
        let mut both = [0u8; FOOTER_LEN * 2];
        f.read_exact(&mut both)?;
        // 副本 B（后写）优先，副本 A 兜底——任一完整可读即可定位索引。
        let (a, b) = both.split_at(FOOTER_LEN);
        let footer = Footer::decode(b)
            .or_else(|| Footer::decode(a))
            .ok_or_else(|| {
                ContainerError::Corrupted("Footer 双副本均不可读（恢复模式属 B-33）".into())
            })?;
        if footer.schema_version != SCHEMA_VERSION {
            return Err(ContainerError::Corrupted(format!(
                "schemaVersion 不符：容器 {} ≠ 引擎 {SCHEMA_VERSION}（迁移协议属 B-31）",
                footer.schema_version
            )));
        }
        if footer.index_offset < SUPERBLOCK_LEN
            || footer.index_offset + footer.index_len + FOOTER_LEN as u64 * 2 > len
        {
            return Err(ContainerError::Corrupted("索引 blob 越界".into()));
        }
        f.seek(SeekFrom::Start(footer.index_offset))?;
        let mut blob = vec![0u8; footer.index_len as usize];
        f.read_exact(&mut blob)?;
        if blake3::hash(&blob).as_bytes() != &footer.index_hash {
            return Err(ContainerError::Corrupted("索引 blob 校验失败".into()));
        }
        let files_len = {
            let mut b8 = [0u8; 8];
            b8.copy_from_slice(blob.get(0..8).ok_or_else(|| {
                ContainerError::Corrupted("索引 blob 截断".into())
            })?);
            u64::from_le_bytes(b8) as usize
        };
        if 8 + files_len > blob.len() {
            return Err(ContainerError::Corrupted("文件表长度越界".into()));
        }
        let files = BPlusTree::<String, FileInfo>::decode(&blob[8..8 + files_len])
            .ok_or_else(|| ContainerError::Corrupted("文件表解码失败".into()))?;
        let chunks = BPlusTree::<HashKey, ChunkLoc>::decode(&blob[8 + files_len..])
            .ok_or_else(|| ContainerError::Corrupted("ChunkIndex 解码失败".into()))?;
        Ok((footer.index_offset, files, chunks))
    }
}

impl Default for UxvBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for UxvBackend {
    fn open(&mut self, cfg: &OpenCfg) -> CmdResult<()> {
        let mut f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&cfg.root)?;
        let len = f.metadata()?.len();
        let (data_tail, files, chunks) = if len == 0 {
            // 全新容器：写 SuperBlock。
            let mut sb = [0u8; SUPERBLOCK_LEN as usize];
            sb[0..8].copy_from_slice(MAGIC);
            sb[8..12].copy_from_slice(&SCHEMA_VERSION.to_le_bytes());
            f.write_all(&sb)?;
            f.sync_all()?;
            (SUPERBLOCK_LEN, BPlusTree::new(), BPlusTree::new())
        } else {
            let mut sb = [0u8; SUPERBLOCK_LEN as usize];
            f.seek(SeekFrom::Start(0))?;
            f.read_exact(&mut sb)?;
            if &sb[0..8] != MAGIC {
                return Err(ContainerError::Corrupted(format!(
                    "魔数不符：{} 不是 .uxv 容器",
                    cfg.root.display()
                )));
            }
            Self::load_index(&mut f)?
        };
        self.path = Some(cfg.root.clone());
        self.file = Some(std::cell::RefCell::new(f));
        self.files = files;
        self.chunks = chunks;
        self.data_tail = data_tail;
        Ok(())
    }

    fn stat(&self, path: &VPath) -> CmdResult<StatInfo> {
        if let Some(info) = self.files.get(path.as_str()) {
            return Ok(self.stat_of(path, &info));
        }
        // 隐式目录：任何文件路径以 `dir/` 开头即视作目录。
        let prefix = format!("{}/", path.as_str());
        if self.files.iter().iter().any(|(p, _)| p.starts_with(&prefix)) {
            return Ok(StatInfo {
                path: path.clone(),
                is_dir: true,
                size: 0,
                modified: None,
            });
        }
        Err(ContainerError::NotFound(path.to_string()))
    }

    fn read(&self, path: &VPath) -> CmdResult<Vec<u8>> {
        let info = self
            .files
            .get(path.as_str())
            .ok_or_else(|| ContainerError::NotFound(path.to_string()))?;
        let mut out = Vec::with_capacity(info.size as usize);
        for c in &info.chunks {
            let loc = self
                .chunks
                .get(&HashKey(*c))
                .ok_or_else(|| ContainerError::Corrupted(format!("{path} 引用的 chunk 缺失")))?;
            out.extend_from_slice(&self.read_chunk_payload(&loc)?);
        }
        if out.len() as u64 != info.size {
            return Err(ContainerError::Corrupted(format!(
                "{path} 尺寸不符（索引 {} ≠ 实际 {}）",
                info.size,
                out.len()
            )));
        }
        Ok(out)
    }

    fn read_range(&self, path: &VPath, off: u64, len: u64) -> CmdResult<Vec<u8>> {
        let info = self
            .files
            .get(path.as_str())
            .ok_or_else(|| ContainerError::NotFound(path.to_string()))?;
        if off >= info.size || len == 0 {
            return Ok(Vec::new());
        }
        let end = (off + len).min(info.size);
        let mut out = Vec::with_capacity((end - off) as usize);
        for (i, c) in info.chunks.iter().enumerate() {
            let chunk_start = i as u64 * CHUNK_SIZE as u64;
            let loc = self
                .chunks
                .get(&HashKey(*c))
                .ok_or_else(|| ContainerError::Corrupted(format!("{path} 引用的 chunk 缺失")))?;
            let chunk_end = chunk_start + loc.len as u64;
            let from = off.max(chunk_start);
            let to = end.min(chunk_end);
            if from < to {
                let payload = self.read_chunk_payload(&loc)?;
                out.extend_from_slice(
                    &payload[(from - chunk_start) as usize..(to - chunk_start) as usize],
                );
            }
            if chunk_start >= end {
                break;
            }
        }
        Ok(out)
    }

    fn stream(&self, path: &VPath) -> CmdResult<Box<dyn ReadSeek + '_>> {
        let info = self
            .files
            .get(path.as_str())
            .ok_or_else(|| ContainerError::NotFound(path.to_string()))?;
        let mut chunks = Vec::with_capacity(info.chunks.len());
        for c in &info.chunks {
            chunks.push(
                self.chunks
                    .get(&HashKey(*c))
                    .ok_or_else(|| ContainerError::Corrupted(format!("{path} 引用的 chunk 缺失")))?,
            );
        }
        let file = self.handle()?.borrow_mut().try_clone()?;
        Ok(Box::new(ChunkStreamReader {
            file,
            chunks,
            logical: 0,
            size: info.size,
        }))
    }

    fn write(&mut self, path: &VPath, data: &[u8]) -> CmdResult<()> {
        // 1) 切 chunk 写入；去重命中则仅引用 +1。
        let mut new_chunks = Vec::new();
        for piece in data.chunks(CHUNK_SIZE) {
            let hash = *blake3::hash(piece).as_bytes();
            if let Some(loc) = self.chunks.get_mut(&HashKey(hash)) {
                loc.refs += 1;
            } else {
                let loc = self.append_chunk(hash, piece)?;
                self.chunks.insert(HashKey(hash), loc);
            }
            new_chunks.push(hash);
        }
        // 2) 旧版本 chunk 解引用。
        if let Some(old) = self.files.get(path.as_str()) {
            for c in &old.chunks {
                self.unref_chunk(*c);
            }
        }
        // 3) 文件表指向新 chunk 序列。
        self.files.insert(
            path.as_str().to_string(),
            FileInfo {
                size: data.len() as u64,
                mtime_ms: Self::now_ms(),
                chunks: new_chunks,
            },
        );
        Ok(())
    }

    fn list(&self, dir: &VPath) -> CmdResult<Vec<StatInfo>> {
        let d = dir.as_str();
        let all: Vec<(String, FileInfo)> = self.files.iter();
        let mut out = Vec::new();
        for (name, is_dir, _size) in Self::children_of(&all, d) {
            let child = VPath::new(&format!("{d}/{name}"))?;
            if is_dir {
                out.push(StatInfo {
                    path: child,
                    is_dir: true,
                    size: 0,
                    modified: None,
                });
            } else {
                let info = self
                    .files
                    .get(child.as_str())
                    .expect("children_of 来源即现有文件条目");
                out.push(self.stat_of(&child, &info));
            }
        }
        Ok(out)
    }

    fn mkdir(&mut self, _dir: &VPath) -> CmdResult<()> {
        // 隐式目录模型：路径由文件条目派生，无需物理节点。
        self.handle()?;
        Ok(())
    }

    fn rm(&mut self, path: &VPath) -> CmdResult<()> {
        let key = path.as_str().to_string();
        if self.files.contains(&key) {
            let info = self.files.get(&key).expect("contains 已判定存在");
            for c in &info.chunks {
                self.unref_chunk(*c);
            }
            self.files.remove(&key);
            return Ok(());
        }
        // 目录：前缀删除。
        let prefix = format!("{key}/");
        let victims: Vec<String> = self
            .files
            .iter()
            .into_iter()
            .map(|(p, _)| p)
            .filter(|p| p.starts_with(&prefix))
            .collect();
        if victims.is_empty() {
            return Err(ContainerError::NotFound(path.to_string()));
        }
        for v in victims {
            let info = self.files.get(&v).expect("来源即现有文件条目");
            for c in &info.chunks {
                self.unref_chunk(*c);
            }
            self.files.remove(&v);
        }
        Ok(())
    }

    fn rename(&mut self, from: &VPath, to: &VPath) -> CmdResult<()> {
        let src = from.as_str().to_string();
        let dst = to.as_str().to_string();
        if self.files.contains(&src) {
            let info = self.files.get(&src).expect("contains 已判定存在");
            self.files.insert(dst, info);
            self.files.remove(&src);
            return Ok(());
        }
        let prefix = format!("{src}/");
        let victims: Vec<String> = self
            .files
            .iter()
            .into_iter()
            .map(|(p, _)| p)
            .filter(|p| p.starts_with(&prefix))
            .collect();
        if victims.is_empty() {
            return Err(ContainerError::NotFound(from.to_string()));
        }
        for v in victims {
            let rest = v[prefix.len()..].to_string();
            let info = self.files.get(&v).expect("来源即现有文件条目");
            self.files.insert(format!("{dst}/{rest}"), info);
            self.files.remove(&v);
        }
        Ok(())
    }

    fn copy(&mut self, from: &VPath, to: &VPath) -> CmdResult<()> {
        let data = self.read(from)?;
        self.write(to, &data)
    }

    /// 追加区不可变 ⇒ 索引状态拷贝即时间点快照（文件级 COW；chunk 级零拷贝克隆属 B-26）。
    fn snapshot(&mut self, label: &str) -> CmdResult<SnapshotId> {
        self.handle()?;
        let id = SnapshotId(sanitize_label(label));
        self.snapshots.insert(
            id.0.clone(),
            SnapshotState {
                data_tail: self.data_tail,
                files: self.files.clone(),
                chunks: self.chunks.clone(),
            },
        );
        Ok(id)
    }

    fn restore(&mut self, id: &SnapshotId) -> CmdResult<()> {
        let st = self
            .snapshots
            .get(&id.0)
            .ok_or_else(|| ContainerError::NotFound(format!("快照 {}", id.0)))?
            .clone();
        // 恢复 = 回退文件表；chunk refcount 按快照文件清单重建
        //（追加区物理数据从未消失，此后被淘汰的 chunk 引用重新成立）。
        let files = st.files;
        let mut chunks: BPlusTree<HashKey, ChunkLoc> = BPlusTree::new();
        let mut by_key: std::collections::HashMap<HashKey, ChunkLoc> = st
            .chunks
            .iter()
            .into_iter()
            .map(|(k, v)| (k, v))
            .collect();
        for (_p, info) in files.iter() {
            for c in &info.chunks {
                let k = HashKey(*c);
                match chunks.get_mut(&k) {
                    Some(l) => l.refs += 1,
                    None => {
                        let mut loc = by_key.remove(&k).ok_or_else(|| {
                            ContainerError::Corrupted("快照引用的 chunk 缺失".into())
                        })?;
                        loc.refs = 1;
                        chunks.insert(k, loc);
                    }
                }
            }
        }
        self.data_tail = st.data_tail;
        self.files = files;
        self.chunks = chunks;
        Ok(())
    }

    fn gc(&mut self, _budget_ms: u32) -> CmdResult<GcReport> {
        // B-12 边界：段回收（物理 chunk 扫描 + 空间挪移）属 B-15。
        self.handle()?;
        Ok(GcReport::default())
    }

    fn seal(&mut self) -> CmdResult<()> {
        self.checkpoint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OpenCfg as Cfg, StorageBackend as Backend};

    fn opened(tag: &str) -> (UxvBackend, PathBuf) {
        let path = std::env::temp_dir().join(format!("uxv-b12-{tag}-{}.uxv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut be = UxvBackend::new();
        be.open(&Cfg { root: path.clone() }).unwrap();
        (be, path)
    }

    fn big(n: usize, seed: u8) -> Vec<u8> {
        let mut v = vec![0u8; n];
        for (i, b) in v.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(seed).wrapping_add(seed);
        }
        v
    }

    #[test]
    fn write_read_roundtrip_small_and_large() {
        let (mut be, _p) = opened("roundtrip");
        be.write(&VPath::new("home/a.txt").unwrap(), b"hello").unwrap();
        assert_eq!(be.read(&VPath::new("home/a.txt").unwrap()).unwrap(), b"hello");
        // > 4MiB 跨 chunk
        let data = big(4 * 1024 * 1024 + 777, 1);
        be.write(&VPath::new("media/big.bin").unwrap(), &data).unwrap();
        assert_eq!(be.read(&VPath::new("media/big.bin").unwrap()).unwrap(), data);
        // read_range 抽段
        assert_eq!(
            be.read_range(&VPath::new("media/big.bin").unwrap(), 4 * 1024 * 1024, 777).unwrap(),
            &data[4 * 1024 * 1024..]
        );
        assert_eq!(
            be.read_range(&VPath::new("media/big.bin").unwrap(), 100, 50).unwrap(),
            &data[100..150]
        );
    }

    #[test]
    fn dedup_shares_identical_chunks_and_counts_refs() {
        let (mut be, _p) = opened("dedup");
        let data = b"same-content-chunk";
        be.write(&VPath::new("a").unwrap(), data).unwrap();
        be.write(&VPath::new("b").unwrap(), data).unwrap();
        assert_eq!(be.chunks.len(), 1, "同内容只存一份");
        // 同文件同内容覆盖：旧引用-1 新引用+1，净不变
        be.write(&VPath::new("a").unwrap(), data).unwrap();
        assert_eq!(be.chunks.len(), 1);
        be.rm(&VPath::new("a").unwrap()).unwrap();
        be.rm(&VPath::new("b").unwrap()).unwrap();
        assert!(be.chunks.is_empty(), "引用归零即摘除索引项");
    }

    #[test]
    fn overwrite_replaces_chunk_list() {
        let (mut be, _p) = opened("overwrite");
        be.write(&VPath::new("f").unwrap(), b"v1").unwrap();
        be.write(&VPath::new("f").unwrap(), b"version-2-longer").unwrap();
        assert_eq!(be.read(&VPath::new("f").unwrap()).unwrap(), b"version-2-longer");
        assert_eq!(be.chunks.len(), 1);
    }

    #[test]
    fn stream_reads_like_read() {
        let (mut be, _p) = opened("stream");
        let data = big(3 * 1024 * 1024 + 5, 7);
        be.write(&VPath::new("v/f.bin").unwrap(), &data).unwrap();
        use std::io::Read;
        let mut r = be.stream(&VPath::new("v/f.bin").unwrap()).unwrap();
        let mut got = Vec::new();
        r.read_to_end(&mut got).unwrap();
        assert_eq!(got, data);
        use std::io::Seek;
        r.seek(std::io::SeekFrom::Start(1024)).unwrap();
        let mut mid = vec![0u8; 10];
        r.read_exact(&mut mid).unwrap();
        assert_eq!(mid, &data[1024..1034]);
    }

    #[test]
    fn seal_reopen_persists_index_and_dedup() {
        let path = std::env::temp_dir().join(format!("uxv-b12-reopen-{}.uxv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let mut be = UxvBackend::new();
            be.open(&Cfg { root: path.clone() }).unwrap();
            let d = b"persisted";
            be.write(&VPath::new("x/f1").unwrap(), d).unwrap();
            be.write(&VPath::new("x/f2").unwrap(), d).unwrap();
            be.write(&VPath::new("y/g").unwrap(), &big(1024, 3)).unwrap();
            be.seal().unwrap();
        }
        let mut be = UxvBackend::new();
        be.open(&Cfg { root: path.clone() }).unwrap();
        assert_eq!(be.read(&VPath::new("x/f1").unwrap()).unwrap(), b"persisted");
        assert_eq!(be.read(&VPath::new("x/f2").unwrap()).unwrap(), b"persisted");
        assert_eq!(be.chunks.len(), 2, "去重跨 seal 保持");
        let items = be.list(&VPath::new("x").unwrap()).unwrap();
        assert_eq!(items.len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unsealed_container_reports_corrupted() {
        let path = std::env::temp_dir().join(format!("uxv-b12-dirty-{}.uxv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut be = UxvBackend::new();
        be.open(&Cfg { root: path.clone() }).unwrap();
        be.write(&VPath::new("f").unwrap(), b"x").unwrap();
        drop(be); // 未 seal 即"断电"
        let mut be2 = UxvBackend::new();
        let err = be2.open(&Cfg { root: path }).unwrap_err();
        assert!(matches!(err, ContainerError::Corrupted(_)), "未 seal 打开必须报 Corrupted（journal 属 B-13）");
    }

    #[test]
    fn snapshot_restore_rolls_back_file_table() {
        let (mut be, _p) = opened("snapshot");
        be.write(&VPath::new("a").unwrap(), b"v1").unwrap();
        let id = be.snapshot("pre").unwrap();
        be.write(&VPath::new("a").unwrap(), b"v2-changed").unwrap();
        be.write(&VPath::new("b").unwrap(), b"new").unwrap();
        be.rm(&VPath::new("a").unwrap()).unwrap();
        be.restore(&id).unwrap();
        assert_eq!(be.read(&VPath::new("a").unwrap()).unwrap(), b"v1");
        assert!(be.read(&VPath::new("b").unwrap()).is_err(), "快照后的新增文件随回退消失");
    }

    #[test]
    fn corrupted_chunk_detected_by_hash() {
        let (mut be, path) = opened("corrupt");
        be.write(&VPath::new("f").unwrap(), &big(4096, 9)).unwrap();
        be.seal().unwrap();
        drop(be);
        // 直接篡改容器文件中的 payload 字节
        let mut f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        use std::io::{Seek, SeekFrom};
        f.seek(SeekFrom::Start(64 + 37)).unwrap();
        f.write_all(&[0xFF, 0xFF]).unwrap();
        drop(f);
        let mut be = UxvBackend::new();
        be.open(&Cfg { root: path }).unwrap();
        let err = be.read(&VPath::new("f").unwrap()).unwrap_err();
        assert!(matches!(err, ContainerError::Corrupted(_)));
    }

    /// 验收口径：热 chunk 随机读平均 < 20ms（蓝图表 444 行）。
    #[test]
    fn hot_random_read_under_20ms() {
        let (mut be, _p) = opened("perf");
        let data = big(16 * 1024 * 1024, 5); // 16MiB = 4 chunks
        be.write(&VPath::new("hot.bin").unwrap(), &data).unwrap();
        be.seal().unwrap();
        let mut rng: u64 = 0x1234;
        let t0 = std::time::Instant::now();
        let n = 200;
        for i in 0..n {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let off = rng % (data.len() as u64 - 4096);
            let got = be.read_range(&VPath::new("hot.bin").unwrap(), off, 4096).unwrap_or_else(|e| panic!("read {i} @ {off}: {e}"));
            assert_eq!(&got[..], &data[off as usize..off as usize + 4096]);
        }
        let avg = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
        assert!(avg < 20.0, "热 chunk 随机读平均 {avg:.3}ms ≥ 20ms");
    }
}
