import sys
p = 'crates/container/src/uxv.rs'
s = open(p, encoding='utf-8').read()
def rep(old, new, tag):
    global s
    if old not in s:
        print("MISS", tag); sys.exit(1)
    s = s.replace(old, new); print("ok", tag)

# P4
rep('''        let files_blob = self.files.encode();
        let mut blob = (files_blob.len() as u64).to_le_bytes().to_vec();
        blob.extend_from_slice(&files_blob);
        blob.extend_from_slice(&self.chunks.encode());
        let hash = blake3::hash(&blob);''',
'''        let files_blob = self.files.encode();
        let mut blob = (files_blob.len() as u64).to_le_bytes().to_vec();
        blob.extend_from_slice(&files_blob);
        blob.extend_from_slice(&self.chunks.encode());
        // vault 态：索引 blob 整体加密（文件名不可枚举），nonce 前置；
        // blake3 覆盖最终存储字节。
        if let Some(v) = &self.vault {
            blob = v.seal_bytes(&blob);
        }
        let hash = blake3::hash(&blob);''', "P4a")
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
            f.seek(SeekFrom::Start(0))?;''', "P4b")

# P5
rep('''    fn load_index(
        f: &mut File,
    ) -> CmdResult<(u64, BPlusTree<String, FileInfo>, BPlusTree<HashKey, ChunkLoc>)> {''',
'''    fn load_index(
        f: &mut File,
        vault: Option<&crate::vault::Vault>,
    ) -> CmdResult<(u64, BPlusTree<String, FileInfo>, BPlusTree<HashKey, ChunkLoc>)> {''', "P5a")
rep('''        if blake3::hash(&blob).as_bytes() != &footer.index_hash {
            return Err(ContainerError::Corrupted("索引 blob 校验失败".into()));
        }''',
'''        if blake3::hash(&blob).as_bytes() != &footer.index_hash {
            return Err(ContainerError::Corrupted("索引 blob 校验失败".into()));
        }
        if let Some(v) = vault {
            blob = v.open_bytes(&blob)?;
        }''', "P5b")
rep("Self::load_index(&mut f)?", "Self::load_index(&mut f, self.vault.as_ref())?", "P5c")

# P6
rep('''    fn journal_append(&mut self, kind: u8, payload: &[u8]) -> CmdResult<()> {
        let tail = self.data_tail;''',
'''    fn journal_append(&mut self, kind: u8, payload: &[u8]) -> CmdResult<()> {
        // vault 态：journal payload 一并加密（blake3 覆盖最终存储字节）。
        let stored = match &self.vault {
            Some(v) => v.seal_bytes(payload),
            None => payload.to_vec(),
        };
        let payload: &[u8] = &stored;
        let tail = self.data_tail;''', "P6a")
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
    ) -> CmdResult<u64> {''', "P6b")
rep('''        if blake3::hash(&payload).as_bytes() != &expect {
            break; // 校验失败 = 撕裂/损坏，视为事务边界
        }
        pos += Self::JN_HDR as u64 + plen as u64;
        if kind == Self::JN_COMMIT {''',
'''        if blake3::hash(&payload).as_bytes() != &expect {
            break; // 校验失败 = 撕裂/损坏，视为事务边界
        }
        pos += Self::JN_HDR as u64 + plen as u64;
        let payload = match vault {
            Some(v) => v.open_bytes(&payload)?,
            None => payload,
        };
        if kind == Self::JN_COMMIT {''', "P6c")

open(p, 'w', encoding='utf-8').write(s)
print("P4-P6 done")
