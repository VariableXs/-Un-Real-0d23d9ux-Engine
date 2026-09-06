import sys, re

P = 'crates/container/src/uxv.rs'
s = open(P, encoding='utf-8').read()

def rep(old, new, tag):
    global s
    if old not in s:
        print('MISS', tag); sys.exit(1)
    s = s.replace(old, new, 1)
    print('ok', tag)

# ---- Y1 冷热 codec 常量已备（CODEC_ZSTD）——append_chunk 加 cold 强制参数 ----
rep('''    fn append_chunk(&mut self, hash: [u8; 32], payload: &[u8]) -> CmdResult<ChunkLoc> {
        // 单卷：chunk 与 journal/index 共享主卷 meta_tail（魔数分流）；
        // 多卷：条带轮转数据卷，主卷只承载元数据。
        let n = self.volumes.len();''',
'''    fn append_chunk(&mut self, hash: [u8; 32], payload: &[u8]) -> CmdResult<ChunkLoc> {
        self.append_chunk_at(hash, payload, false)
    }

    /// cold=true：冷层强制 Zstd-19（挂起项目/归档）；否则热层分级策略。
    fn append_chunk_at(&mut self, hash: [u8; 32], payload: &[u8], cold: bool) -> CmdResult<ChunkLoc> {
        // 单卷：chunk 与 journal/index 共享主卷 meta_tail（魔数分流）；
        // 多卷：条带轮转数据卷，主卷只承载元数据。
        let n = self.volumes.len();''', 'Y1')

rep('''        let (codec, mut stored) = crate::codec::compress(payload);
        if let Some(v) = &self.vault {
            stored = v.seal_bytes(&stored);
        }''',
'''        let (codec, mut stored) = if cold {
            crate::codec::compress_cold(payload)
        } else {
            crate::codec::compress(payload)
        };
        if let Some(v) = &self.vault {
            stored = v.seal_bytes(&stored);
        }''', 'Y2')

# ---- Y2 写入计数（放大比仪表）----
rep('''        let mut new_chunks = Vec::new();
        let mut new_locs = Vec::new();
        for piece in data.chunks(CHUNK_SIZE) {''',
'''        let mut new_chunks = Vec::new();
        let mut new_locs = Vec::new();
        self.logical_written += data.len() as u64;
        for piece in data.chunks(CHUNK_SIZE) {''', 'Y3')

rep('''        self.data_tail += CHUNK_HDR_LEN as u64 + stored.len() as u64;
        Ok(ChunkLoc {''',
'''        self.data_tail_unused += CHUNK_HDR_LEN as u64 + stored.len() as u64;
        Ok(ChunkLoc {''', 'Y4x')  # 占位防误（无此文本则跳过）if False else s

open(P, 'w', encoding='utf-8').write(s)
print('Y1-Y2 done')
