import sys

P = 'crates/container/src/uxv.rs'
s = open(P, encoding='utf-8').read()

def rep(old, new, tag):
    global s
    if old not in s:
        print('MISS', tag); sys.exit(1)
    s = s.replace(old, new, 1)
    print('ok', tag)

# ---------- P1 常量 ----------
rep('pub const CODEC_RAW: u8 = 0;',
'''pub const CODEC_RAW: u8 = 0;
pub const CODEC_LZ4: u8 = 1;
pub const CODEC_ZSTD: u8 = 2;
/// SuperBlock 标志：vault 启用（sb[20]）；盐 [24..40]；验证器 [40..56]。
const SB_FLAG_VAULT: usize = 20;
const SB_SALT: std::ops::Range<usize> = 24..40;
const SB_VERIFIER: std::ops::Range<usize> = 40..56;''', 'P1')

# ---------- P2 结构体字段 ----------
old = s
import re
m = re.search(r'(    snapshots: std::collections::HashMap<String, SnapshotState>,\n)', s)
assert m, 'P2a'
s = s.replace(m.group(1), m.group(1) +
'''    /// 加密金库（B-14）：Some = 全容器加密态。
    vault: Option<crate::vault::Vault>,
    /// 持久化元数据：(salt, verifier)，随 SuperBlock 落盘。
    vault_meta: Option<([u8; 16], [u8; 16])>,
''', 1)
print('ok P2a')
m = re.search(r'(            snapshots: std::collections::HashMap::new\(\),\n)', s)
assert m, 'P2b'
s = s.replace(m.group(1), m.group(1) +
'''            vault: None,
            vault_meta: None,
''', 1)
print('ok P2b')

# ---------- P3 append_chunk ----------
rep('''    fn append_chunk(&mut self, hash: [u8; 32], payload: &[u8]) -> CmdResult<ChunkLoc> {
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
    }''',
'''    /// chunk 落盘：先压缩分级（LZ4/Zstd-19），vault 开启则整体加密。
    /// 记录头 [len u32][codec u8][blake3 32B]：头部哈希 = 最终存储字节（完整性），
    /// 索引键 = 原文内容哈希（去重），两者口径不同。
    fn append_chunk(&mut self, hash: [u8; 32], payload: &[u8]) -> CmdResult<ChunkLoc> {
        let (codec, mut stored) = crate::codec::compress(payload);
        if let Some(v) = &self.vault {
            stored = v.seal_bytes(&stored);
        }
        let offset = self.data_tail;
        {
            let f = self.handle()?;
            let mut f = f.borrow_mut();
            f.seek(SeekFrom::Start(offset))?;
            f.write_all(&(stored.len() as u32).to_le_bytes())?;
            f.write_all(&[codec])?;
            f.write_all(blake3::hash(&stored).as_bytes())?;
            f.write_all(&stored)?;
        }
        self.data_tail += CHUNK_HDR_LEN as u64 + stored.len() as u64;
        Ok(ChunkLoc { offset, len: stored.len() as u32, codec, refs: 1 })
    }''', 'P3a')

# ---------- P3b read_chunk_payload 尾部解密解压 ----------
m = re.search(r'(fn read_chunk_payload\(&self, loc: &ChunkLoc\) -> CmdResult<Vec<u8>> \{.*?\n        Ok\(payload\)\n    \})', s, re.S)
assert m, 'P3b'
body = m.group(1)
body = body.replace('''        let mut payload = vec![0u8; len as usize];
        f.read_exact(&mut payload)?;''',
'''        let mut stored = vec![0u8; len as usize];
        f.read_exact(&mut stored)?;
        drop(f);''', 1)
body = body.replace('blake3::hash(&payload)', 'blake3::hash(&stored)', 1)
body = body.replace('HashKey(expect)\n            )));\n        }\n        Ok(payload)',
'''HashKey(expect)
            )));
        }
        if let Some(v) = &self.vault {
            stored = v.open_bytes(&stored)?;
        }
        crate::codec::decompress(loc.codec, &stored)''', 1)
s = s.replace(m.group(1), body, 1)
print('ok P3b')

# ---------- P4 checkpoint ----------
rep('''        let files_blob = self.files.encode();
        let mut blob = (files_blob.len() as u64).to_le_bytes().to_vec();
        blob.extend_from_slice(&files_blob);
        blob.extend_from_slice(&self.chunks.encode());
        let hash = blake3::hash(&blob);''',
'''        let files_blob = self.files.encode();
        let mut blob = (files_blob.len() as u64).to_le_bytes().to_vec();
        blob.extend_from_slice(&files_blob);
        blob.extend_from_slice(&self.chunks.encode());
        // vault 态：索引 blob 整体加密（文件名不可枚举），nonce 前置。
        if let Some(v) = &self.vault {
            blob = v.seal_bytes(&blob);
        }
        let hash = blake3::hash(&blob);''', 'P4a')
rep('''            sb[12..20].copy_from_slice(&footer_offset.to_le_bytes());
            f.seek(SeekFrom::Start(0))?;''',
'''            sb[12..20].copy_from_slice(&footer_offset.to_le_bytes());
            match (&self.vault, self.vault_meta) {
                (Some(v), Some((salt, verifier))) => {
                    sb[SB_FLAG_VAULT] = 1;
                    sb[SB_SALT].copy_from_slice(&salt);
                    sb[SB_VERIFIER].copy_from_slice(&verifier);
                }
                _ => sb[SB_FLAG_VAULT] = 0,
            }
            f.seek(SeekFrom::Start(0))?;''', 'P4b')

# ---------- P5 load_index ----------
rep('''    fn load_index(
        f: &mut File,
    ) -> CmdResult<(u64, BPlusTree<String, FileInfo>, BPlusTree<HashKey, ChunkLoc>)> {''',
'''    fn load_index(
        f: &mut File,
        vault: Option<&crate::vault::Vault>,
    ) -> CmdResult<(u64, BPlusTree<String, FileInfo>, BPlusTree<HashKey, ChunkLoc>)> {''', 'P5a')
rep('''        if blake3::hash(&blob).as_bytes() != &footer.index_hash {
            return Err(ContainerError::Corrupted("索引 blob 校验失败".into()));
        }''',
'''        if blake3::hash(&blob).as_bytes() != &footer.index_hash {
            return Err(ContainerError::Corrupted("索引 blob 校验失败".into()));
        }
        if let Some(v) = vault {
            blob = v.open_bytes(&blob)?;
        }''', 'P5b')
rep('Self::load_index(&mut f)?', 'Self::load_index(&mut f, self.vault.as_ref())?', 'P5c')

# ---------- P6 journal ----------
rep('''    fn journal_append(&mut self, kind: u8, payload: &[u8]) -> CmdResult<()> {
        let tail = self.data_tail;''',
'''    fn journal_append(&mut self, kind: u8, payload: &[u8]) -> CmdResult<()> {
        // vault 态：journal payload 一并加密（blake3 覆盖最终存储字节）。
        let stored = match &self.vault {
            Some(v) => v.seal_bytes(payload),
            None => payload.to_vec(),
        };
        let payload: &[u8] = &stored;
        let tail = self.data_tail;''', 'P6a')
rep('''    fn journal_replay(
        f: &mut File,
        journal_base: u64,
        len: u64,
        files: &mut BPlusTree<String, FileInfo>,
        chunks: &mut BPlusTree<HashKey, ChunkLoc>,
    ) -> CmdResult<u64> {''',
'''    fn journal_replay(
        f: &mut File,
        journal_base: u64,
        len: u64,
        files: &mut BPlusTree<String, FileInfo>,
        chunks: &mut BPlusTree<HashKey, ChunkLoc>,
        vault: Option<&crate::vault::Vault>,
    ) -> CmdResult<u64> {''', 'P6b')
rep('''            pos += Self::JN_HDR as u64 + plen as u64;
            if kind == Self::JN_COMMIT {''',
'''            pos += Self::JN_HDR as u64 + plen as u64;
            let payload = match vault {
                Some(v) => v.open_bytes(&payload)?,
                None => payload,
            };
            if kind == Self::JN_COMMIT {''', 'P6c')

# ---------- P7 open → open_inner ----------
rep('''    fn open(&mut self, cfg: &OpenCfg) -> CmdResult<()> {
        let mut f = OpenOptions::new()''',
'''    fn open(&mut self, cfg: &OpenCfg) -> CmdResult<()> {
        self.open_inner(cfg, None)
    }
}

impl UxvBackend {
    /// 带口令打开/创建：空文件 + 口令 = 创建加密容器；
    /// 已加密容器必须走本入口，trait `open` 对加密容器如实报错。
    pub fn open_with_passphrase(&mut self, cfg: &OpenCfg, passphrase: &[u8]) -> CmdResult<()> {
        self.open_inner(cfg, Some(passphrase))
    }

    fn open_inner(&mut self, cfg: &OpenCfg, passphrase: Option<&[u8]>) -> CmdResult<()> {
        let mut f = OpenOptions::new()''', 'P7a')
rep('''            f.write_all(&sb)?;
            f.sync_all()?;
            (SUPERBLOCK_LEN, BPlusTree::new(), BPlusTree::new())
        } else {''',
'''            if let Some(pass) = passphrase {
                let (vault, salt, verifier) = crate::vault::Vault::create(pass);
                sb[SB_FLAG_VAULT] = 1;
                sb[SB_SALT].copy_from_slice(&salt);
                sb[SB_VERIFIER].copy_from_slice(&verifier);
                self.vault = Some(vault);
                self.vault_meta = Some((salt, verifier));
            } else {
                sb[SB_FLAG_VAULT] = 0;
            }
            f.write_all(&sb)?;
            f.sync_all()?;
            (SUPERBLOCK_LEN, BPlusTree::new(), BPlusTree::new())
        } else {''', 'P7b')
rep('''            Self::load_index(&mut f, self.vault.as_ref())?
        };''',
'''            if sb[SB_FLAG_VAULT] == 1 {
                let salt: [u8; 16] = sb[SB_SALT].try_into().expect("定长");
                let verifier: [u8; 16] = sb[SB_VERIFIER].try_into().expect("定长");
                let pass = passphrase.ok_or_else(|| {
                    ContainerError::Auth("容器已加密：请用 open_with_passphrase 解锁".into())
                })?;
                let vault = crate::vault::Vault::unlock(pass, &salt, &verifier)?;
                self.vault = Some(vault);
                self.vault_meta = Some((salt, verifier));
            } else if passphrase.is_some() {
                return Err(ContainerError::Auth("容器未加密，口令多余".into()));
            }
            Self::load_index(&mut f, self.vault.as_ref())?
        };''', 'P7c')
rep('''        self.data_tail = Self::journal_replay(&mut f, data_tail, len, &mut self.files, &mut self.chunks)?;''',
'''        self.data_tail = Self::journal_replay(
            &mut f,
            data_tail,
            len,
            &mut self.files,
            &mut self.chunks,
            self.vault.as_ref(),
        )?;''', 'P7d')

# ---------- P8 read_range 逻辑边界 ----------
rep('''            let chunk_start = i as u64 * CHUNK_SIZE as u64;
            let loc = self
                .chunks
                .get(&HashKey(*c))
                .ok_or_else(|| ContainerError::Corrupted(format!("{path} 引用的 chunk 缺失")))?;
            let chunk_end = chunk_start + loc.len as u64;''',
'''            // 逻辑边界按原文 4MiB 定长切分（存储长度经压缩/加密后 ≠ 原文长度）。
            let chunk_start = i as u64 * CHUNK_SIZE as u64;
            let chunk_end = ((i + 1) as u64 * CHUNK_SIZE as u64).min(info.size);
            let loc = self
                .chunks
                .get(&HashKey(*c))
                .ok_or_else(|| ContainerError::Corrupted(format!("{path} 引用的 chunk 缺失")))?;''', 'P8')

# ---------- P9 流式读取器重写 ----------
rep('''struct ChunkStreamReader {
    file: File,
    chunks: Vec<ChunkLoc>,
    logical: u64,
    size: u64,
}''',
'''struct ChunkStreamReader {
    file: File,
    /// 每逻辑窗口（原文 4MiB 定长，末尾可短）对应的 chunk 位置。
    chunks: Vec<ChunkLoc>,
    vault: Option<crate::vault::Vault>,
    /// 当前已解码窗口缓存（跨读复用）。
    window: Vec<u8>,
    window_idx: Option<usize>,
    logical: u64,
    size: u64,
}

impl ChunkStreamReader {
    fn ensure_window(&mut self) -> std::io::Result<()> {
        let idx = (self.logical / CHUNK_SIZE as u64) as usize;
        if self.window_idx == Some(idx) {
            return Ok(());
        }
        let loc = self.chunks.get(idx).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "chunk 清单越界")
        })?;
        let f = &mut self.file;
        f.seek(SeekFrom::Start(loc.offset))?;
        let mut hdr = [0u8; CHUNK_HDR_LEN];
        f.read_exact(&mut hdr)?;
        let mut stored = vec![0u8; loc.len as usize];
        f.read_exact(&mut stored)?;
        if let Some(v) = &self.vault {
            stored = v
                .open_bytes(&stored)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        }
        self.window = crate::codec::decompress(loc.codec, &stored)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        self.window_idx = Some(idx);
        Ok(())
    }
}''', 'P9a')

new_impl = '''impl Read for ChunkStreamReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.logical >= self.size || buf.is_empty() {
            return Ok(0);
        }
        self.ensure_window()?;
        let within = (self.logical % CHUNK_SIZE as u64) as usize;
        let want = buf
            .len()
            .min(self.window.len() - within)
            .min((self.size - self.logical) as usize);
        buf[..want].copy_from_slice(&self.window[within..within + want]);
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
'''

m2 = re.search(r'impl Read for ChunkStreamReader \{.*?\n\}\n\nimpl Seek for ChunkStreamReader \{.*?\n\}\n', s, re.S)
assert m2, 'P9b'
s = s[:m2.start()] + new_impl + s[m2.end():]
print('ok P9b')

rep('''        Ok(Box::new(ChunkStreamReader {
            file,
            chunks,
            logical: 0,
            size: info.size,
        }))''',
'''        Ok(Box::new(ChunkStreamReader {
            file,
            chunks,
            vault: self.vault.clone(),
            window: Vec::new(),
            window_idx: None,
            logical: 0,
            size: info.size,
        }))''', 'P9c')

open(P, 'w', encoding='utf-8').write(s)
print('ALL DONE')
