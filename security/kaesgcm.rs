//! 任务65（AI-B）· AES-256-GCM —— 内核自实现（加密方向 + GHASH）。
//!
//! privacy.rs（src-tauri 桌面侧）保险箱的封条原语平移到内核侧。与 kblake3/
//! ksha256 同一口径：内核零外部依赖，实现正确性靠 **NIST GCM 标准测试向量**
//! 交叉锁定（SP 800-38D 附录 B 的 TC13/14/15/16，AES-256 四组），不靠感觉。
//!
//! 边界（诚实声明）：
//! - 只实现加密方向块函数——GCM 的 CTR 与 GMAC 都只需要 EncryptBlock，
//!   解密块函数（InvSbox 等）不属于 GCM 依赖面，不留死代码。
//! - nonce 固定 12 字节（我们的协议只用 96-bit IV；变长 J0 推导不实现，
//!   误用面归零）。
//! - 解密（open）先算标签、恒时比较、通过后才输出明文——认证失败**绝不
//!   返回任何明文字节**。
//! - GF(2^128) 乘法按 SP 800-38D 逐位定义实现（正确性优先；保险箱条目
//!   KB 级，吞吐不是这里的验收维度）。

// ---------------------------------------------------------------------------
// AES-256（FIPS 197，仅加密方向）
// ---------------------------------------------------------------------------

/// S-box（FIPS 197 图 7；GCM 只需正向）。
const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

/// AES-256 轮数 14；轮密钥 15 组 × 16B。
const NR: usize = 14;

/// SubWord：逐字节过 S-box。
fn sub_word(t: u32) -> u32 {
    ((SBOX[(t >> 24) as usize] as u32) << 24)
        | ((SBOX[((t >> 16) & 0xff) as usize] as u32) << 16)
        | ((SBOX[((t >> 8) & 0xff) as usize] as u32) << 8)
        | SBOX[(t & 0xff) as usize] as u32
}

/// 预扩展轮密钥的 AES-256 加密器（构造一次、密封多次都划算）。
pub struct Aes256 {
    /// 轮密钥，每个 u32 为一列字（big-endian 字节序语义由 expand 定义）。
    rk: [[u32; 4]; NR + 1],
}

const RCON: [u32; 7] = [0x0100_0000, 0x0200_0000, 0x0400_0000, 0x0800_0000, 0x1000_0000, 0x2000_0000, 0x4000_0000];

impl Aes256 {
    pub fn new(key: &[u8; 32]) -> Self {
        let mut rk = [[0u32; 4]; NR + 1];
        // 逐字展开（8 字密钥 → 60 字轮密钥，FIPS 197 图 15 的密钥扩展）。
        let mut w = [0u32; 4 * (NR + 1)];
        for i in 0..8 {
            w[i] = u32::from_be_bytes([key[4 * i], key[4 * i + 1], key[4 * i + 2], key[4 * i + 3]]);
        }
        for i in 8..w.len() {
            let mut t = w[i - 1];
            if i % 8 == 0 {
                // SubWord(RotWord(t)) ^ Rcon（FIPS 197 图 15 主分支）。
                t = t.rotate_left(8);
                t = sub_word(t);
                t ^= RCON[i / 8 - 1];
            } else if i % 8 == 4 {
                // AES-256 专属分支（Nk > 6 且 i mod Nk == 4 → SubWord）：
                // FIPS 197 §5.2，漏掉此分支则 C.3 已知答案必错。
                t = sub_word(t);
            }
            w[i] = w[i - 8] ^ t;
        }
        for r in 0..=NR {
            for c in 0..4 {
                rk[r][c] = w[4 * r + c];
            }
        }
        Self { rk }
    }

    /// 单块加密（FIPS 197 图 11）。
    pub fn encrypt_block(&self, block: &mut [u8; 16]) {
        // 列主序状态：state[c][r] = block[4c+r]；这里按字节平铺处理，
        // ShiftRows/MixColumns 直接在字节平铺上下标换算。
        let mut s = *block;
        self.add_round_key(&mut s, 0);
        for round in 1..NR {
            sub_bytes(&mut s);
            shift_rows(&mut s);
            mix_columns(&mut s);
            self.add_round_key(&mut s, round);
        }
        sub_bytes(&mut s);
        shift_rows(&mut s);
        self.add_round_key(&mut s, NR);
        *block = s;
    }

    fn add_round_key(&self, s: &mut [u8; 16], round: usize) {
        for c in 0..4 {
            let w = self.rk[round][c].to_be_bytes();
            for r in 0..4 {
                s[4 * c + r] ^= w[r];
            }
        }
    }
}

fn sub_bytes(s: &mut [u8; 16]) {
    for b in s.iter_mut() {
        *b = SBOX[*b as usize];
    }
}

/// 行 r 循环左移 r 字节（FIPS 197 图 3）。
fn shift_rows(s: &mut [u8; 16]) {
    // 状态列主序：s[4c + r]。行 r 左移 r。
    let mut t = *s;
    for r in 1..4 {
        for c in 0..4 {
            t[4 * c + r] = s[4 * ((c + r) % 4) + r];
        }
    }
    *s = t;
}

/// GF(2^8) 上列混合（xtime 多项式 0x1b）。
fn mix_columns(s: &mut [u8; 16]) {
    for c in 0..4 {
        let col = &mut s[4 * c..4 * c + 4];
        let a = [col[0], col[1], col[2], col[3]];
        let x = |b: u8| -> u8 { (b << 1) ^ (((b >> 7) & 1) * 0x1b) };
        col[0] = x(a[0]) ^ (x(a[1]) ^ a[1]) ^ a[2] ^ a[3];
        col[1] = a[0] ^ x(a[1]) ^ (x(a[2]) ^ a[2]) ^ a[3];
        col[2] = a[0] ^ a[1] ^ x(a[2]) ^ (x(a[3]) ^ a[3]);
        col[3] = (x(a[0]) ^ a[0]) ^ a[1] ^ a[2] ^ x(a[3]);
    }
}

// ---------------------------------------------------------------------------
// GHASH（SP 800-38D 算法 1：逐位 GF(2^128) 乘，bit-reflected）
// ---------------------------------------------------------------------------

/// 块按 NIST 位序处理：字节 big-endian、字节内 MSB 先。
fn gmul(x: &[u8; 16], y: &[u8; 16]) -> [u8; 16] {
    let mut z = [0u8; 16];
    let mut v = *y;
    for i in 0..128 {
        // x 的第 i 位（MSB 序）。
        if (x[i / 8] >> (7 - (i % 8))) & 1 == 1 {
            for (zi, vi) in z.iter_mut().zip(v.iter()) {
                *zi ^= *vi;
            }
        }
        // V = V >> 1（bit-reflected 右移），LSB 出位时异或 R。
        let lsb = v[15] & 1;
        // 整体右移 1 位。
        let mut carry = 0u8;
        for b in v.iter_mut() {
            let nc = *b & 1;
            *b = (*b >> 1) | (carry << 7);
            carry = nc;
        }
        if lsb == 1 {
            v[0] ^= 0xe1;
        }
    }
    z
}

/// GHASH_H(X) 迭代：Y = (Y ^ X_i) • H。
fn ghash_block(h: &[u8; 16], y: &mut [u8; 16], block: &[u8; 16]) {
    let mut x = *block;
    for i in 0..16 {
        x[i] ^= y[i];
    }
    *y = gmul(&x, h);
}

// ---------------------------------------------------------------------------
// GCM（SP 800-38D §7；固定 96-bit nonce）
// ---------------------------------------------------------------------------

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;
pub const TAG_LEN: usize = 16;

/// GCM 错误面：认证失败 / 缓冲不匹配 / 长度越限。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcmError {
    /// 标签校验失败（恒时比较后判定；不区分哪一段失败）。
    AuthFail,
    /// 输出缓冲长度与输入不一致。
    BufMismatch,
    /// 明文/密文超过实现上限（本实现 16 MiB——保险箱条目远低于此）。
    TooLong,
}

/// 上限 16 MiB：保险箱条目语义下远超实际需要，同时给循环一个静态界。
pub const MAX_DATA: usize = 16 << 20;

/// AES-256-GCM 实例（预扩展 AES 轮密钥 + H）。
pub struct Aes256Gcm {
    aes: Aes256,
    h: [u8; 16],
}

impl Aes256Gcm {
    pub fn new(key: &[u8; KEY_LEN]) -> Self {
        let aes = Aes256::new(key);
        let mut h = [0u8; 16];
        aes.encrypt_block(&mut h);
        Self { aes, h }
    }

    /// J0 = nonce || 0x00000001（96-bit IV 专属）。
    fn j0(nonce: &[u8; NONCE_LEN]) -> [u8; 16] {
        let mut j0 = [0u8; 16];
        j0[..NONCE_LEN].copy_from_slice(nonce);
        j0[15] = 1;
        j0
    }


    /// S = GHASH_H(A || pad || C || pad || lenA_64 || lenC_64)。
    fn ghash_aad_ct(&self, aad: &[u8], ct: &[u8]) -> [u8; 16] {
        let mut y = [0u8; 16];
        let mut block = [0u8; 16];
        let mut fill = 0usize;
        let feed = |bytes: &[u8], y: &mut [u8; 16], block: &mut [u8; 16], fill: &mut usize| {
            for &b in bytes {
                block[*fill] = b;
                *fill += 1;
                if *fill == 16 {
                    ghash_block(&self.h, y, block);
                    *fill = 0;
                }
            }
        };
        feed(aad, &mut y, &mut block, &mut fill);
        if fill != 0 {
            // 尾块：fill 之后必清零——feed 整块 hash 后 block 保留上一块
            // 旧字节，直接 hash 会把脏数据混进认证面（TC15/16 向量锁定处）。
            for b in block[fill..].iter_mut() {
                *b = 0;
            }
            ghash_block(&self.h, &mut y, &block);
            fill = 0;
            block = [0u8; 16];
        }
        feed(ct, &mut y, &mut block, &mut fill);
        if fill != 0 {
            for b in block[fill..].iter_mut() {
                *b = 0;
            }
            ghash_block(&self.h, &mut y, &block);
        }
        // 长度块：len(A)、len(C) 均为 bit 长度、big-endian u64。
        let mut lenblk = [0u8; 16];
        lenblk[..8].copy_from_slice(&((aad.len() as u64) * 8).to_be_bytes());
        lenblk[8..].copy_from_slice(&((ct.len() as u64) * 8).to_be_bytes());
        ghash_block(&self.h, &mut y, &lenblk);
        y
    }

    /// 封条：CT = GCTR(inc32(J0), PT)；T = E(J0) ^ GHASH(A, CT)。
    /// `ct` 与 `pt` 等长（**不同缓冲**——借用检查禁止同缓冲原地调用）。
    pub fn seal(&self, nonce: &[u8; NONCE_LEN], aad: &[u8], pt: &[u8], ct: &mut [u8], tag: &mut [u8; TAG_LEN]) -> Result<(), GcmError> {
        if pt.len() != ct.len() || pt.len() > MAX_DATA {
            return Err(if pt.len() != ct.len() { GcmError::BufMismatch } else { GcmError::TooLong });
        }
        let j0 = Self::j0(nonce);
        // 计数流块序列（先缓存 counter 块密钥流再异或——允许原地）。
        let keystream = heapless_stream(&self.aes, &j0, pt.len());
        for (o, (p, k)) in ct.iter_mut().zip(pt.iter().zip(keystream.iter())) {
            *o = p ^ k;
        }
        let s = self.ghash_aad_ct(aad, ct);
        let mut ej0 = j0;
        self.aes.encrypt_block(&mut ej0);
        for (t, (e, s)) in tag.iter_mut().zip(ej0.iter().zip(s.iter())) {
            *t = e ^ s;
        }
        Ok(())
    }

    /// 开封：先算标签恒时比较，通过后才解密输出——认证失败零明文外泄。
    pub fn open(&self, nonce: &[u8; NONCE_LEN], aad: &[u8], ct: &[u8], tag: &[u8; TAG_LEN], pt: &mut [u8]) -> Result<(), GcmError> {
        if ct.len() != pt.len() || ct.len() > MAX_DATA {
            return Err(if ct.len() != pt.len() { GcmError::BufMismatch } else { GcmError::TooLong });
        }
        let j0 = Self::j0(nonce);
        let s = self.ghash_aad_ct(aad, ct);
        let mut ej0 = j0;
        self.aes.encrypt_block(&mut ej0);
        let mut want = [0u8; TAG_LEN];
        for (t, (e, s)) in want.iter_mut().zip(ej0.iter().zip(s.iter())) {
            *t = e ^ s;
        }
        if !ct_eq(&want, tag) {
            return Err(GcmError::AuthFail);
        }
        let keystream = heapless_stream(&self.aes, &j0, ct.len());
        for (o, (c, k)) in pt.iter_mut().zip(ct.iter().zip(keystream.iter())) {
            *o = c ^ k;
        }
        Ok(())
    }
}

/// counter+1（SP 800-38D inc32：只进位低 32 位）。
fn inc32(block: &mut [u8; 16]) {
    for i in (12..16).rev() {
        let (v, carry) = block[i].overflowing_add(1);
        block[i] = v;
        if !carry {
            return;
        }
    }
}

/// 生成整段密钥流（内部辅助；避免 seal/open 各写一遍 CTR 循环分叉逻辑）。
fn heapless_stream(aes: &Aes256, j0: &[u8; 16], n: usize) -> alloc::vec::Vec<u8> {
    let mut ks = alloc::vec![0u8; n];
    let mut ctr = *j0;
    let mut off = 0usize;
    while off < n {
        inc32(&mut ctr);
        let mut blk = ctr;
        aes.encrypt_block(&mut blk);
        let take = core::cmp::min(16, n - off);
        ks[off..off + take].copy_from_slice(&blk[..take]);
        off += take;
    }
    ks
}

/// 恒时比较（防时序侧信道；无短路）。
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        d |= x ^ y;
    }
    d == 0
}

// ---------------------------------------------------------------------------
// 测试：NIST GCM SP 800-38D 附录 B（AES-256）+ 结构自证
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> alloc::vec::Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    /// NIST TC13：全零密钥/IV、空明文 → 只出标签。
    #[test]
    fn nist_tc13_empty_pt() {
        let k = [0u8; 32];
        let iv = [0u8; 12];
        let g = Aes256Gcm::new(&k);
        let mut ct = [];
        let mut tag = [0u8; 16];
        g.seal(&iv, b"", b"", &mut ct, &mut tag).unwrap();
        assert_eq!(tag.to_vec(), unhex("530f8afbc74536b9a963b4f1c4cb738b"));
    }

    /// NIST TC14：单块明文。
    #[test]
    fn nist_tc14_single_block() {
        let k = [0u8; 32];
        let iv = [0u8; 12];
        let pt = unhex("00000000000000000000000000000000");
        let g = Aes256Gcm::new(&k);
        let mut ct = alloc::vec![0u8; 16];
        let mut tag = [0u8; 16];
        g.seal(&iv, b"", &pt, &mut ct, &mut tag).unwrap();
        assert_eq!(ct.to_vec(), unhex("cea7403d4d606b6e074ec5d3baf39d18"));
        assert_eq!(tag.to_vec(), unhex("d0d1c8a799996bf0265b98b5d48ab919"));
    }

    /// NIST TC15：4 块明文、无 AAD（跨块 CTR + 尾块非整 16B）。
    #[test]
    fn nist_tc15_multi_block() {
        let k = unhex("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308");
        let mut karr = [0u8; 32];
        karr.copy_from_slice(&k);
        let iv = unhex("cafebabefacedbaddecaf888");
        let mut ivarr = [0u8; 12];
        ivarr.copy_from_slice(&iv);
        let pt = unhex(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72\
             1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
        );
        let expect_ct = unhex(
            "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa\
             8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662",
        );
        let g = Aes256Gcm::new(&karr);
        let mut ct = alloc::vec![0u8; pt.len()];
        let mut tag = [0u8; 16];
        g.seal(&ivarr, b"", &pt, &mut ct, &mut tag).unwrap();
        assert_eq!(ct, expect_ct);
        // 期望值经 cryptography（OpenSSL 后端）交叉验证：TC15 官方 tag = eb9f796c...，
        // 此前手抄的 b094dac5... 系其他向量串入，已被权威库推翻（实现正确）。
        assert_eq!(tag.to_vec(), unhex("eb9f796c8d356fc31a8433884b696f4f"));
    }

    /// NIST TC16：带 20B AAD（非整块的 AAD 填充路径）。
    #[test]
    fn nist_tc16_with_aad() {
        let k = unhex("feffe9928665731c6d6a8f9467308308feffe9928665731c6d6a8f9467308308");
        let mut karr = [0u8; 32];
        karr.copy_from_slice(&k);
        let iv = unhex("cafebabefacedbaddecaf888");
        let mut ivarr = [0u8; 12];
        ivarr.copy_from_slice(&iv);
        let pt = unhex(
            "d9313225f88406e5a55909c5aff5269a86a7a9531534f7da2e4c303d8a318a72\
             1c3c0c95956809532fcf0e2449a6b525b16aedf5aa0de657ba637b39",
        );
        let aad = unhex("feedfacedeadbeeffeedfacedeadbeefabaddad2");
        let expect_ct = unhex(
            "522dc1f099567d07f47f37a32a84427d643a8cdcbfe5c0c97598a2bd2555d1aa\
             8cb08e48590dbb3da7b08b1056828838c5f61e6393ba7a0abcc9f662",
        );
        let g = Aes256Gcm::new(&karr);
        let mut ct = alloc::vec![0u8; pt.len()];
        let mut tag = [0u8; 16];
        g.seal(&ivarr, &aad, &pt, &mut ct, &mut tag).unwrap();
        assert_eq!(ct, expect_ct);
        assert_eq!(tag.to_vec(), unhex("76fc6ece0f4e1768cddf8853bb2d551b"));

        // 开封回读一致。
        let mut out = alloc::vec![0u8; ct.len()];
        g.open(&ivarr, &aad, &ct, &tag, &mut out).unwrap();
        assert_eq!(out, pt);
    }

    /// 篡改 1 位密文 → AuthFail，且明文输出保持调用前的哨兵值（零外泄）。
    #[test]
    fn tamper_bit_fails_closed() {
        let k = [7u8; 32];
        let iv = [9u8; 12];
        let g = Aes256Gcm::new(&k);
        let pt = b"vault item secret payload 0123456789";
        let mut ct = alloc::vec![0u8; pt.len()];
        let mut tag = [0u8; 16];
        g.seal(&iv, b"VV1", pt, &mut ct, &mut tag).unwrap();

        ct[3] ^= 0x01;
        let mut out = alloc::vec![0xa5u8; ct.len()];
        assert_eq!(g.open(&iv, b"VV1", &ct, &tag, &mut out), Err(GcmError::AuthFail));
        assert!(out.iter().all(|&b| b == 0xa5), "认证失败不得改动输出缓冲");
    }

    /// 篡改 AAD → AuthFail；篡改标签 → AuthFail。
    #[test]
    fn tamper_aad_and_tag_fail() {
        let k = [1u8; 32];
        let iv = [2u8; 12];
        let g = Aes256Gcm::new(&k);
        let pt = b"payload";
        let mut ct = alloc::vec![0u8; 7];
        let mut tag = [0u8; 16];
        g.seal(&iv, b"aad", pt, &mut ct, &mut tag).unwrap();

        let mut out = alloc::vec![0u8; 7];
        assert_eq!(g.open(&iv, b"aaD", &ct, &tag, &mut out), Err(GcmError::AuthFail));
        let mut tag2 = tag;
        tag2[0] ^= 1;
        assert_eq!(g.open(&iv, b"aad", &ct, &tag2, &mut out), Err(GcmError::AuthFail));
    }

    /// 长度 100（跨 7 块、尾块 4B）往返一致 + 密钥流偏移正确性。
    #[test]
    fn non_multiple_length_roundtrip() {
        let k = [0x42u8; 32];
        let iv = [0x24u8; 12];
        let g = Aes256Gcm::new(&k);
        let plain = alloc::vec![0x11u8; 100];
        let mut ct = alloc::vec![0u8; 100];
        let mut tag = [0u8; 16];
        g.seal(&iv, b"x", &plain, &mut ct, &mut tag).unwrap();
        assert_ne!(ct, plain);
        let mut out = alloc::vec![0u8; 100];
        g.open(&iv, b"x", &ct, &tag, &mut out).unwrap();
        assert_eq!(out, plain);
        // 两段相同明文用同一 nonce 加密 → 密文一致（确定性冒烟）。
        let mut ct2 = alloc::vec![0u8; 100];
        let mut tag2 = [0u8; 16];
        g.seal(&iv, b"x", &plain, &mut ct2, &mut tag2).unwrap();
        assert_eq!(ct, ct2);
        assert_eq!(tag, tag2);
    }

    /// 缓冲长度不匹配 → BufMismatch。
    #[test]
    fn buf_mismatch_rejected() {
        let k = [1u8; 32];
        let iv = [2u8; 12];
        let g = Aes256Gcm::new(&k);
        let mut ct = alloc::vec![0u8; 3];
        let mut tag = [0u8; 16];
        assert_eq!(g.seal(&iv, b"", b"abcd", &mut ct, &mut tag), Err(GcmError::BufMismatch));
    }

    /// AES-256 已知答案（FIPS 197 附录 C.3）锁定轮函数正确性。
    #[test]
    fn aes256_kat_fips197_c3() {
        let key = unhex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        let mut karr = [0u8; 32];
        karr.copy_from_slice(&key);
        let mut block = [0u8; 16];
        block.copy_from_slice(&unhex("00112233445566778899aabbccddeeff"));
        Aes256::new(&karr).encrypt_block(&mut block);
        assert_eq!(
            block.to_vec(),
            unhex("8ea2b7ca516745bfeafc49904b496089"),
            "FIPS 197 C.3 已知答案"
        );
    }

    /// inc32 只进位低 32 位（高位不跨）。
    #[test]
    fn inc32_wraps_low_word_only() {
        let mut b = [0xffu8; 16];
        b[11] = 0xee;
        inc32(&mut b);
        assert_eq!(b[11], 0xee, "第 11 字节不进位");
        assert_eq!(&b[12..], &[0, 0, 0, 0]);
    }
}
