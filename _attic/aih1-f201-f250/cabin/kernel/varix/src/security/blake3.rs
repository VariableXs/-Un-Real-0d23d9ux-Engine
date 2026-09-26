//! BLAKE3（内核自实现 · 任务35 Uxv 交换格式校验依赖）。
//!
//! 纪律同 security.rs（SHA-256/ChaCha20）：安全模块里的哈希必须是真哈希，
//! 占位哈希比没有更糟。实现为 BLAKE3 官方规范的 keyless 哈希（一次成型，
//! chunk 链 + 父节点树 + ROOT 扩展），与官方 crate（dev-dependency blake3）
//! 随机长度交叉验证 + 官方向量断言（见文件尾测试）。
//!
//! 面向内核只读校验场景：`hash(data) -> [u8; 32]`。不做 keyed/derive_key
//! （Uxv 校验用不到，少一条攻击面）。

const OUT_LEN: usize = 32;
const BLOCK_LEN: usize = 64;
const CHUNK_LEN: usize = 1024;

const IV: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

const MSG_PERMUTATION: [usize; 16] = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];

const CHUNK_START: u32 = 1 << 0;
const CHUNK_END: u32 = 1 << 1;
const PARENT: u32 = 1 << 2;
const ROOT: u32 = 1 << 3;

fn g(state: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, mx: u32, my: u32) {
    state[a] = state[a].wrapping_add(state[b].wrapping_add(mx));
    state[d] = (state[d] ^ state[a]).rotate_right(16);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(12);
    state[a] = state[a].wrapping_add(state[b].wrapping_add(my));
    state[d] = (state[d] ^ state[a]).rotate_right(8);
    state[c] = state[c].wrapping_add(state[d]);
    state[b] = (state[b] ^ state[c]).rotate_right(7);
}

fn round(state: &mut [u32; 16], m: &[u32; 16]) {
    // 列混合
    g(state, 0, 4, 8, 12, m[0], m[1]);
    g(state, 1, 5, 9, 13, m[2], m[3]);
    g(state, 2, 6, 10, 14, m[4], m[5]);
    g(state, 3, 7, 11, 15, m[6], m[7]);
    // 对角混合
    g(state, 0, 5, 10, 15, m[8], m[9]);
    g(state, 1, 6, 11, 12, m[10], m[11]);
    g(state, 2, 7, 8, 13, m[12], m[13]);
    g(state, 3, 4, 9, 14, m[14], m[15]);
}

fn permute(m: &mut [u32; 16]) {
    let old = *m;
    for i in 0..16 {
        m[i] = old[MSG_PERMUTATION[i]];
    }
}

fn compress(cv: &[u32; 8], block: &[u8; BLOCK_LEN], counter: u64, block_len: u32, flags: u32) -> [u32; 16] {
    let mut m = [0u32; 16];
    for i in 0..16 {
        m[i] = u32::from_le_bytes(block[i * 4..i * 4 + 4].try_into().expect("定长切片"));
    }
    let mut state: [u32; 16] = [
        cv[0], cv[1], cv[2], cv[3], cv[4], cv[5], cv[6], cv[7],
        IV[0], IV[1], IV[2], IV[3],
        counter as u32, (counter >> 32) as u32, block_len, flags,
    ];
    for r in 0..7 {
        if r > 0 {
            permute(&mut m);
        }
        round(&mut state, &m);
    }
    for i in 0..8 {
        state[i] ^= state[i + 8];
        state[i + 8] ^= cv[i];
    }
    state
}

fn first_8_words(compression_output: [u32; 16]) -> [u32; 8] {
    let mut out = [0u32; 8];
    out.copy_from_slice(&compression_output[0..8]);
    out
}

/// 一次压缩输出的惰性终化：非 ROOT 输出取链接值，ROOT 输出可扩至任意长度。
struct Output {
    input_cv: [u32; 8],
    block: [u8; BLOCK_LEN],
    counter: u64,
    block_len: u32,
    flags: u32,
}

impl Output {
    fn chaining_value(&self) -> [u32; 8] {
        first_8_words(compress(&self.input_cv, &self.block, self.counter, self.block_len, self.flags))
    }
    fn root_output_bytes(&self, out: &mut [u8]) {
        let mut output_block_counter = 0u64;
        for out_block in out.chunks_mut(2 * OUT_LEN) {
            let words = compress(
                &self.input_cv,
                &self.block,
                output_block_counter,
                self.block_len,
                self.flags | ROOT,
            );
            for (word, out_word) in words.iter().zip(out_block.chunks_mut(4)) {
                out_word.copy_from_slice(&word.to_le_bytes()[..out_word.len()]);
            }
            output_block_counter += 1;
        }
    }
}

struct ChunkState {
    cv: [u32; 8],
    chunk_counter: u64,
    block: [u8; BLOCK_LEN],
    block_len: u8,
    blocks_compressed: u8,
    flags: u32,
}

impl ChunkState {
    fn new(key_words: [u32; 8], chunk_counter: u64, flags: u32) -> Self {
        ChunkState { cv: key_words, chunk_counter, block: [0; BLOCK_LEN], block_len: 0, blocks_compressed: 0, flags }
    }
    fn len(&self) -> usize {
        BLOCK_LEN * self.blocks_compressed as usize + self.block_len as usize
    }
    fn start_flag(&self) -> u32 {
        if self.blocks_compressed == 0 { CHUNK_START } else { 0 }
    }
    fn update(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            if self.block_len as usize == BLOCK_LEN {
                let cv = self.chaining_value_via_compress();
                self.cv = cv;
                self.blocks_compressed += 1;
                self.block = [0; BLOCK_LEN];
                self.block_len = 0;
            }
            let want = BLOCK_LEN - self.block_len as usize;
            let take = want.min(input.len());
            self.block[self.block_len as usize..self.block_len as usize + take]
                .copy_from_slice(&input[..take]);
            self.block_len += take as u8;
            input = &input[take..];
        }
    }
    fn chaining_value_via_compress(&mut self) -> [u32; 8] {
        first_8_words(compress(
            &self.cv,
            &self.block,
            self.chunk_counter,
            self.block_len as u32,
            self.flags | self.start_flag(),
        ))
    }
    fn output(&self) -> Output {
        Output {
            input_cv: self.cv,
            block: self.block,
            counter: self.chunk_counter,
            block_len: self.block_len as u32,
            flags: self.flags | self.start_flag() | CHUNK_END,
        }
    }
}

fn parent_output(left_cv: [u32; 8], right_cv: [u32; 8], key_words: [u32; 8], flags: u32) -> Output {
    let mut block = [0u8; BLOCK_LEN];
    for (i, w) in left_cv.iter().chain(right_cv.iter()).enumerate() {
        block[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    Output { input_cv: key_words, block, counter: 0, block_len: BLOCK_LEN as u32, flags: PARENT | flags }
}

fn parent_cv(left: [u32; 8], right: [u32; 8], key: [u32; 8], flags: u32) -> [u32; 8] {
    parent_output(left, right, key, flags).chaining_value()
}

/// CV 栈容量：2^64 个 chunk 只需 64 层；定容 52 层足够任何实际输入。
const CV_STACK_CAP: usize = 54;

struct Hasher {
    chunk_state: ChunkState,
    key_words: [u32; 8],
    cv_stack: [[u32; 8]; CV_STACK_CAP],
    cv_stack_len: u8,
    flags: u32,
}

impl Hasher {
    fn new(key_words: [u32; 8], flags: u32) -> Self {
        Hasher {
            chunk_state: ChunkState::new(key_words, 0, flags),
            key_words,
            cv_stack: [[0; 8]; CV_STACK_CAP],
            cv_stack_len: 0,
            flags,
        }
    }
    fn push_stack(&mut self, cv: [u32; 8]) {
        self.cv_stack[self.cv_stack_len as usize] = cv;
        self.cv_stack_len += 1;
    }
    fn pop_stack(&mut self) -> [u32; 8] {
        self.cv_stack_len -= 1;
        self.cv_stack[self.cv_stack_len as usize]
    }
    /// 新 chunk 即将开始：把当前满 chunk 链合并进树（total_len 低位置 1 的合并序）。
    fn add_chunk_chaining_value(&mut self, mut new_cv: [u32; 8], mut total_chunks: u64) {
        while total_chunks & 1 == 0 {
            new_cv = parent_cv(self.pop_stack(), new_cv, self.key_words, self.flags);
            total_chunks >>= 1;
        }
        self.push_stack(new_cv);
    }
    fn update(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            if self.chunk_state.len() == CHUNK_LEN {
                // 中间 chunk 并树必须带 CHUNK_END 终态（output() 含 END）。
                let chunk_cv = self.chunk_state.output().chaining_value();
                let total_chunks = self.chunk_state.chunk_counter + 1;
                self.add_chunk_chaining_value(chunk_cv, total_chunks);
                self.chunk_state = ChunkState::new(self.key_words, total_chunks, self.flags);
            }
            let want = CHUNK_LEN - self.chunk_state.len();
            let take = want.min(input.len());
            self.chunk_state.update(&input[..take]);
            input = &input[take..];
        }
    }
    fn finalize(&self, out_slice: &mut [u8]) {
        let mut output = self.chunk_state.output();
        let mut parent_nodes_remaining = self.cv_stack_len as usize;
        while parent_nodes_remaining > 0 {
            parent_nodes_remaining -= 1;
            output = parent_output(
                self.cv_stack[parent_nodes_remaining],
                output.chaining_value(),
                self.key_words,
                self.flags,
            );
        }
        output.root_output_bytes(out_slice);
    }
}

/// BLAKE3 keyless 哈希（一次成型）。
pub fn hash(data: &[u8]) -> [u8; OUT_LEN] {
    let mut hasher = Hasher::new(IV, 0);
    hasher.update(data);
    let mut out = [0u8; OUT_LEN];
    hasher.finalize(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方测试向量首项（BLAKE3 spec testvectors.json，input "abc"）。
    const ABC: [u8; 32] = [
        0x64, 0x37, 0xb3, 0xac, 0x38, 0x46, 0x51, 0x33, 0xff, 0xb6, 0x3b, 0x75, 0x27, 0x3a, 0x8d,
        0xb5, 0x48, 0xc5, 0x58, 0x46, 0x5d, 0x79, 0xdb, 0x03, 0xfd, 0x35, 0x9c, 0x6c, 0xd5, 0xbd,
        0x9d, 0x85,
    ];

    #[test]
    fn official_vector_abc() {
        assert_eq!(hash(b"abc"), ABC);
    }

    #[test]
    fn official_vector_empty() {
        // testvectors.json input_len=0 的首 32 字节。
        let expected: [u8; 32] = [
            0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
            0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
            0xe4, 0x1f, 0x32, 0x62,
        ];
        assert_eq!(hash(b""), expected);
    }

    /// 与官方 crate 全长度段交叉验证：覆盖 chunk 边界（1023/1024/1025）、
    /// 父节点各级边界（2048/4096/…）与随机长度。
    #[test]
    fn cross_check_against_official_crate() {
        // 确定性伪随机流（LCG），覆盖 0..=4200 内全部边界长度 + 两个大长度。
        let mut data = Vec::with_capacity(1 << 20);
        let mut x: u32 = 0x1234_5678;
        for _ in 0..(1 << 20) {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            data.push((x >> 24) as u8);
        }
        let mut lens: Vec<usize> = (0..=130).map(|i| i * 32).collect();
        for extra in [1023usize, 1024, 1025, 2047, 2048, 2049, 4095, 4096, 8191, 8192, 16384, 1 << 20] {
            lens.push(extra);
        }
        for len in lens {
            assert_eq!(hash(&data[..len]).len(), 32, "len={len}");
            let a = hash(&data[..len]);
            let b = blake3::hash(&data[..len]);
            assert_eq!(&a, b.as_bytes(), "len={} mine={:02x?} official={:02x?}", len, a, b.as_slice());
        }
    }
}
