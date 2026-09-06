import sys
p = 'crates/container/src/uxv.rs'
s = open(p, encoding='utf-8').read()
def rep(old, new, tag):
    global s
    if old not in s:
        print("MISS", tag); sys.exit(1)
    s = s.replace(old, new); print("ok", tag)

# P8 read_range：逻辑边界按原文 4MiB 定长（存储长度≠原文长度）
rep('''            let chunk_start = i as u64 * CHUNK_SIZE as u64;
            let chunk_end = chunk_start + loc.len as u64;
            let from = off.max(chunk_start);''',
'''            // 逻辑边界按原文 4MiB 定长切分（存储长度经压缩/加密后 ≠ 原文长度）。
            let chunk_start = i as u64 * CHUNK_SIZE as u64;
            let chunk_end = ((i + 1) as u64 * CHUNK_SIZE as u64).min(info.size);
            let from = off.max(chunk_start);''', "P8")

# P9 流式读取器：按逻辑窗口解码（整 chunk 解压/解密后缓存）
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
    /// 当前已解码窗口缓存（跨读复用，避免每次 seek 重解码）。
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
}''', "P9a")

# Read/Seek 实现替换为窗口缓存版
start = s.index('impl Read for ChunkStreamReader {')
end = s.index('impl Default for UxvBackend {')
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
s = s[:start] + new_impl + s[end:]

# stream() 构造适配
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
        }))''', "P9b")

open(p, 'w', encoding='utf-8').write(s)
print("P8-P9 done")
