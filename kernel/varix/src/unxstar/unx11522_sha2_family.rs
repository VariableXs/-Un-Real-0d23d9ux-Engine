//! UNX-F11522 sha256sum/sha512sum SHA-2 族
//!
//! SHA-2 是一个**族**而不是一个算法：SHA-256 用 32 位字、512 位分块、64 轮；
//! SHA-512 用 64 位字、1024 位分块、80 轮。两者的骨架完全相同——消息扩展、
//! 八寄存器压缩、`Ch`/`Maj` 选择与择多、大端填长——差别只在字宽、轮数和常数表。
//! 本实现把两套压缩函数并排放，共用同一个增量缓冲 `Sha`，因此「一次喂完」与
//! 「分片喂」对两种算法都必须逐字节一致。
//!
//! 填长的细节是 SHA-2 最容易写错的地方：先补 `0x80`，再补零到 `块长 - 长度域`，
//! 最后写**大端比特数**。SHA-256 的长度域 8 字节、SHA-512 的 16 字节（高 8 字节
//! 在本实现恒为 0，因为单次输入不可能到 2^64 比特）。补零时若正好越过块尾，就
//! 会多压一块——这条边界是测试的重点。
//!
//! 校验行与 md5sum 同构：`<摘要><空格><模式><名字>`，模式空格为文本、`*` 为
//! 二进制。**摘要长度由算法决定**：64 个十六进制字符给 SHA-256，128 个给
//! SHA-512；长度对不上不是「格式错」而是「族内混用」，记 `SA_EMIX`，这样报错
//! 时才分得清是写错了还是拿错了算法。
//!
//! 闸门次序（`parse_line`）：行长 → 分隔空格 → 十六进制 → 摘要长度 → 模式字符
//! → 名字非空。先定位字段边界，再判字段内容。
//!
//! 账本语义：十四格 = 八格成功（两种摘要长度、名字长、校验通过、二进制标志、
//! 四向量、两种首字节）+ 六格失败（未知算法、非十六进制、族内混用、模式、空
//! 名、重名）。`audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比
//! 对，`freeze` 后登记一律拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf1b0;

/// 算法名未知。
pub const SA_EALG: u32 = SEG_BASE + 0x01;
/// 摘要字段含非十六进制。
pub const SA_EHEX: u32 = SEG_BASE + 0x02;
/// 行超过长度上限。
pub const SA_ELEN: u32 = SEG_BASE + 0x03;
/// 分隔或模式字符非法。
pub const SA_EFMT: u32 = SEG_BASE + 0x04;
/// 名字为空。
pub const SA_ENAME: u32 = SEG_BASE + 0x05;
/// 摘要长度与算法不符（族内混用）。
pub const SA_EMIX: u32 = SEG_BASE + 0x06;
/// 同名重复校验。
pub const SA_EDUP: u32 = SEG_BASE + 0x07;
/// 摘要与实测不符。
pub const SA_ESUM: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const SA_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const SA_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const SA_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const SA_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const SA_ENOBS: u32 = SEG_BASE + 0x0d;

/// SHA-256。
pub const ALG_SHA256: u8 = 0;
/// SHA-512。
pub const ALG_SHA512: u8 = 1;

/// 校验行长度上限。
pub const LINE_MAX: usize = 256;
/// 校验清单容量。
pub const CHECKS: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：摘要。
pub const OP_DIGEST: u8 = 1;
/// 操作码：解析。
pub const OP_PARSE: u8 = 2;
/// 操作码：校验。
pub const OP_VERIFY: u8 = 3;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// SHA-256 常数表。
const K256: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4, 0xab1c_5ed5,
    0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe, 0x9bdc_06a7, 0xc19b_f174,
    0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f, 0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da,
    0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7, 0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967,
    0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc, 0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85,
    0xa2bf_e8a1, 0xa81a_664b, 0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070,
    0x19a4_c116, 0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7, 0xc671_78f2,
];

/// SHA-512 常数表。
const K512: [u64; 80] = [
    0x428a_2f98_d728_ae22, 0x7137_4491_23ef_65cd, 0xb5c0_fbcf_ec4d_3b2f, 0xe9b5_dba5_8189_dbbc,
    0x3956_c25b_f348_b538, 0x59f1_11f1_b605_d019, 0x923f_82a4_af19_4f9b, 0xab1c_5ed5_da6d_8118,
    0xd807_aa98_a303_0242, 0x1283_5b01_4570_6fbe, 0x2431_85be_4ee4_b28c, 0x550c_7dc3_d5ff_b4e2,
    0x72be_5d74_f27b_896f, 0x80de_b1fe_3b16_96b1, 0x9bdc_06a7_25c7_1235, 0xc19b_f174_cf69_2694,
    0xe49b_69c1_9ef1_4ad2, 0xefbe_4786_384f_25e3, 0x0fc1_9dc6_8b8c_d5b5, 0x240c_a1cc_77ac_9c65,
    0x2de9_2c6f_592b_0275, 0x4a74_84aa_6ea6_e483, 0x5cb0_a9dc_bd41_fbd4, 0x76f9_88da_8311_53b5,
    0x983e_5152_ee66_dfab, 0xa831_c66d_2db4_3210, 0xb003_27c8_98fb_213f, 0xbf59_7fc7_beef_0ee4,
    0xc6e0_0bf3_3da8_8fc2, 0xd5a7_9147_930a_a725, 0x06ca_6351_e003_826f, 0x1429_2967_0a0e_6e70,
    0x27b7_0a85_46d2_2ffc, 0x2e1b_2138_5c26_c926, 0x4d2c_6dfc_5ac4_2aed, 0x5338_0d13_9d95_b3df,
    0x650a_7354_8baf_63de, 0x766a_0abb_3c77_b2a8, 0x81c2_c92e_47ed_aee6, 0x9272_2c85_1482_353b,
    0xa2bf_e8a1_4cf1_0364, 0xa81a_664b_bc42_3001, 0xc24b_8b70_d0f8_9791, 0xc76c_51a3_0654_be30,
    0xd192_e819_d6ef_5218, 0xd699_0624_5565_a910, 0xf40e_3585_5771_202a, 0x106a_a070_32bb_d1b8,
    0x19a4_c116_b8d2_d0c8, 0x1e37_6c08_5141_ab53, 0x2748_774c_df8e_eb99, 0x34b0_bcb5_e19b_48a8,
    0x391c_0cb3_c5c9_5a63, 0x4ed8_aa4a_e341_8acb, 0x5b9c_ca4f_7763_e373, 0x682e_6ff3_d6b2_b8a3,
    0x748f_82ee_5def_b2fc, 0x78a5_636f_4317_2f60, 0x84c8_7814_a1f0_ab72, 0x8cc7_0208_1a64_39ec,
    0x90be_fffa_2363_1e28, 0xa450_6ceb_de82_bde9, 0xbef9_a3f7_b2c6_7915, 0xc671_78f2_e372_532b,
    0xca27_3ece_ea26_619c, 0xd186_b8c7_21c0_c207, 0xeada_7dd6_cde0_eb1e, 0xf57d_4f7f_ee6e_d178,
    0x06f0_67aa_7217_6fba, 0x0a63_7dc5_a2c8_98a6, 0x113f_9804_bef9_0dae, 0x1b71_0b35_131c_471b,
    0x28db_77f5_2304_7d84, 0x32ca_ab7b_40c7_2493, 0x3c9e_be0a_15c9_bebc, 0x431d_67c4_9c10_0d4c,
    0x4cc5_d4be_cb3e_42b6, 0x597f_299c_fc65_7e2a, 0x5fcb_6fab_3ad6_faec, 0x6c44_198c_4a47_5817,
];

/// 大端取 32 位字。
fn be32(b: &[u8], i: usize) -> u32 {
    let mut v = 0u32;
    let mut k = 0usize;
    while k < 4 {
        let byte = match b.get(i + k) {
            Some(x) => *x as u32,
            None => 0,
        };
        v = (v << 8) | byte;
        k += 1;
    }
    v
}

/// 大端取 64 位字。
fn be64(b: &[u8], i: usize) -> u64 {
    let mut v = 0u64;
    let mut k = 0usize;
    while k < 8 {
        let byte = match b.get(i + k) {
            Some(x) => *x as u64,
            None => 0,
        };
        v = (v << 8) | byte;
        k += 1;
    }
    v
}

/// 压一块 SHA-256。
fn compress256(st: &mut [u64; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    let mut i = 0usize;
    while i < 16 {
        w[i] = be32(block, i * 4);
        i += 1;
    }
    while i < 64 {
        let x = w[i - 15];
        let y = w[i - 2];
        let s0 = x.rotate_right(7) ^ x.rotate_right(18) ^ (x >> 3);
        let s1 = y.rotate_right(17) ^ y.rotate_right(19) ^ (y >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
        i += 1;
    }
    let mut h = [0u32; 8];
    let mut j = 0usize;
    while j < 8 {
        h[j] = st[j] as u32;
        j += 1;
    }
    let mut i = 0usize;
    while i < 64 {
        let s1 = h[4].rotate_right(6) ^ h[4].rotate_right(11) ^ h[4].rotate_right(25);
        let ch = (h[4] & h[5]) ^ (!h[4] & h[6]);
        let k = match K256.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let t1 = h[7]
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(k)
            .wrapping_add(w[i]);
        let s0 = h[0].rotate_right(2) ^ h[0].rotate_right(13) ^ h[0].rotate_right(22);
        let maj = (h[0] & h[1]) ^ (h[0] & h[2]) ^ (h[1] & h[2]);
        let t2 = s0.wrapping_add(maj);
        h[7] = h[6];
        h[6] = h[5];
        h[5] = h[4];
        h[4] = h[3].wrapping_add(t1);
        h[3] = h[2];
        h[2] = h[1];
        h[1] = h[0];
        h[0] = t1.wrapping_add(t2);
        i += 1;
    }
    let mut j = 0usize;
    while j < 8 {
        // 32 位字：进位必须丢弃，否则 64 位累加器会把高位带到摘要里。
        st[j] = st[j].wrapping_add(h[j] as u64) & 0xffff_ffff;
        j += 1;
    }
}

/// 压一块 SHA-512。
fn compress512(st: &mut [u64; 8], block: &[u8; 128]) {
    let mut w = [0u64; 80];
    let mut i = 0usize;
    while i < 16 {
        w[i] = be64(block, i * 8);
        i += 1;
    }
    while i < 80 {
        let x = w[i - 15];
        let y = w[i - 2];
        let s0 = x.rotate_right(1) ^ x.rotate_right(8) ^ (x >> 7);
        let s1 = y.rotate_right(19) ^ y.rotate_right(61) ^ (y >> 6);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
        i += 1;
    }
    let mut h = [0u64; 8];
    let mut j = 0usize;
    while j < 8 {
        h[j] = st[j];
        j += 1;
    }
    let mut i = 0usize;
    while i < 80 {
        let s1 = h[4].rotate_right(14) ^ h[4].rotate_right(18) ^ h[4].rotate_right(41);
        let ch = (h[4] & h[5]) ^ (!h[4] & h[6]);
        let k = match K512.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let t1 = h[7]
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(k)
            .wrapping_add(w[i]);
        let s0 = h[0].rotate_right(28) ^ h[0].rotate_right(34) ^ h[0].rotate_right(39);
        let maj = (h[0] & h[1]) ^ (h[0] & h[2]) ^ (h[1] & h[2]);
        let t2 = s0.wrapping_add(maj);
        h[7] = h[6];
        h[6] = h[5];
        h[5] = h[4];
        h[4] = h[3].wrapping_add(t1);
        h[3] = h[2];
        h[2] = h[1];
        h[1] = h[0];
        h[0] = t1.wrapping_add(t2);
        i += 1;
    }
    let mut j = 0usize;
    while j < 8 {
        st[j] = st[j].wrapping_add(h[j]);
        j += 1;
    }
}

/// 摘要字节数。
pub fn digest_len(alg: u8) -> usize {
    if alg == ALG_SHA512 {
        64
    } else {
        32
    }
}

/// 算法名 → 编号。
pub fn by_name(name: &str) -> Result<u8, u32> {
    match name {
        "sha256" | "sha256sum" => Ok(ALG_SHA256),
        "sha512" | "sha512sum" => Ok(ALG_SHA512),
        _ => Err(SA_EALG),
    }
}

/// 十六进制小写。
fn nibble(v: u8) -> u8 {
    if v < 10 {
        b'0' + v
    } else {
        b'a' + (v - 10)
    }
}

/// 十六进制值。
fn hexval(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// 摘要（最长 64 字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sum {
    bytes: [u8; 64],
    len: usize,
}

impl Sum {
    /// 空白摘要。
    pub const fn blank() -> Sum {
        Sum { bytes: [0; 64], len: 0 }
    }

    /// 字节数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 原始字节。
    pub fn bytes(&self) -> &[u8] {
        match self.bytes.get(..self.len) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 第 i 字节。
    pub fn byte(&self, i: usize) -> u8 {
        match self.bytes.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 小写十六进制（返回缓冲区与有效长度）。
    pub fn hex(&self) -> ([u8; 128], usize) {
        let mut out = [0u8; 128];
        let mut i = 0usize;
        while i < self.len {
            let b = self.byte(i);
            if let Some(dst) = out.get_mut(i * 2) {
                *dst = nibble(b >> 4);
            }
            if let Some(dst) = out.get_mut(i * 2 + 1) {
                *dst = nibble(b & 0x0f);
            }
            i += 1;
        }
        (out, self.len * 2)
    }

    /// 与十六进制字面量比对。
    pub fn eq_hex(&self, want: &[u8]) -> bool {
        if want.len() != self.len * 2 {
            return false;
        }
        let (buf, n) = self.hex();
        let mut i = 0usize;
        while i < n {
            match (buf.get(i), want.get(i)) {
                (Some(x), Some(y)) => {
                    if *x != *y {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 增量摘要器（两种算法共用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sha {
    alg: u8,
    st: [u64; 8],
    len: u64,
    buf: [u8; 128],
    fill: usize,
}

impl Sha {
    /// 按算法初始化。
    pub const fn new(alg: u8) -> Result<Sha, u32> {
        if alg > ALG_SHA512 {
            return Err(SA_EALG);
        }
        let st = if alg == ALG_SHA512 {
            [
                0x6a09_e667_f3bc_c908, 0xbb67_ae85_84ca_a73b,
                0x3c6e_f372_fe94_f82b, 0xa54f_f53a_5f1d_36f1,
                0x510e_527f_ade6_82d1, 0x9b05_688c_2b3e_6c1f,
                0x1f83_d9ab_fb41_bd6b, 0x5be0_cd19_137e_2179,
            ]
        } else {
            [
                0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a,
                0x510e_527f, 0x9b05_688c, 0x1f83_d9ab, 0x5be0_cd19,
            ]
        };
        Ok(Sha { alg, st, len: 0, buf: [0; 128], fill: 0 })
    }

    /// 算法。
    pub fn alg(&self) -> u8 {
        self.alg
    }

    /// 块长。
    pub fn block_size(&self) -> usize {
        if self.alg == ALG_SHA512 {
            128
        } else {
            64
        }
    }

    /// 已喂字节。
    pub fn len(&self) -> u64 {
        self.len
    }

    /// 是否还没喂。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 压当前缓冲。
    fn flush(&mut self) {
        if self.alg == ALG_SHA512 {
            let mut block = [0u8; 128];
            let mut i = 0usize;
            while i < 128 {
                if let Some(v) = self.buf.get(i) {
                    block[i] = *v;
                }
                i += 1;
            }
            compress512(&mut self.st, &block);
        } else {
            let mut block = [0u8; 64];
            let mut i = 0usize;
            while i < 64 {
                if let Some(v) = self.buf.get(i) {
                    block[i] = *v;
                }
                i += 1;
            }
            compress256(&mut self.st, &block);
        }
        self.fill = 0;
    }

    /// 喂入数据。
    pub fn update(&mut self, data: &[u8]) {
        let bs = self.block_size();
        let mut i = 0usize;
        while i < data.len() {
            let room = bs.saturating_sub(self.fill);
            let left = data.len().saturating_sub(i);
            let take = if room < left { room } else { left };
            let mut k = 0usize;
            while k < take {
                if let (Some(dst), Some(src)) = (self.buf.get_mut(self.fill + k), data.get(i + k)) {
                    *dst = *src;
                }
                k += 1;
            }
            self.fill = self.fill.saturating_add(take);
            i = i.saturating_add(take);
            if self.fill == bs {
                self.flush();
            }
        }
        self.len = self.len.saturating_add(data.len() as u64);
    }

    /// 收尾。
    pub fn finish(mut self) -> Sum {
        let bs = self.block_size();
        let lenbytes = if self.alg == ALG_SHA512 { 16 } else { 8 };
        let bits = self.len.wrapping_mul(8);
        self.update(&[0x80]);
        while self.fill != bs.saturating_sub(lenbytes) {
            self.update(&[0]);
        }
        let mut lenb = [0u8; 16];
        let mut i = 0usize;
        while i < 8 {
            if let Some(dst) = lenb.get_mut(15 - i) {
                *dst = ((bits >> (8 * i as u32)) & 0xff) as u8;
            }
            i += 1;
        }
        // SHA-256 的长度域 8 字节，SHA-512 的是 16 字节（高 8 字节恒零）。
        if self.alg == ALG_SHA512 {
            self.update(&lenb);
        } else if let Some(lo) = lenb.get(8..16) {
            self.update(lo);
        }
        let dl = digest_len(self.alg);
        // SHA-256 每字 4 字节，SHA-512 每字 8 字节。
        let wb = if self.alg == ALG_SHA512 { 8 } else { 4 };
        let mut out = Sum { bytes: [0; 64], len: dl };
        let mut j = 0usize;
        while j < 8 {
            let w = match self.st.get(j) {
                Some(v) => *v,
                None => 0,
            };
            let mut k = 0usize;
            while k < wb {
                let idx = j * wb + k;
                if idx < dl {
                    if let Some(dst) = out.bytes.get_mut(idx) {
                        *dst = ((w >> (8 * (wb - 1 - k) as u32)) & 0xff) as u8;
                    }
                }
                k += 1;
            }
            j += 1;
        }
        out
    }
}

/// 一次算完。
pub fn sum(alg: u8, data: &[u8]) -> Result<Sum, u32> {
    let mut s = Sha::new(alg)?;
    s.update(data);
    Ok(s.finish())
}

/// 解析后的一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SumLine {
    /// 摘要。
    pub digest: Sum,
    /// 二进制模式。
    pub binary: bool,
    /// 名字带转义。
    pub escaped: bool,
    /// 原始名字。
    pub name: &'static str,
}

impl SumLine {
    /// 名字字节数。
    pub fn name_len(&self) -> usize {
        self.name.len()
    }
}

/// 解析一行校验。
pub fn parse_line(alg: u8, line: &'static str) -> Result<SumLine, u32> {
    if alg > ALG_SHA512 {
        return Err(SA_EALG);
    }
    if line.len() > LINE_MAX {
        return Err(SA_ELEN);
    }
    let b = line.as_bytes();
    let mut at = 0usize;
    let mut escaped = false;
    if b.get(0) == Some(&b'\\') {
        escaped = true;
        at = 1;
    }
    let mut sep = at;
    while sep < b.len() && b.get(sep) != Some(&b' ') {
        sep += 1;
    }
    if sep >= b.len() || sep <= at {
        return Err(SA_EFMT);
    }
    let dl = digest_len(alg);
    let mut i = 0usize;
    while i < sep - at {
        match b.get(at + i).and_then(|c| hexval(*c)) {
            Some(_) => {}
            None => return Err(SA_EHEX),
        }
        i += 1;
    }
    if (sep - at) != dl * 2 {
        return Err(SA_EMIX);
    }
    let mode = match b.get(sep + 1) {
        Some(v) => *v,
        None => return Err(SA_EFMT),
    };
    let binary = match mode {
        b' ' => false,
        b'*' => true,
        _ => return Err(SA_EFMT),
    };
    // 边解析边把两位十六进制合成一字节。
    let mut bytes = [0u8; 64];
    let mut k = 0usize;
    while k < dl {
        let hi = match b.get(at + k * 2).and_then(|c| hexval(*c)) {
            Some(v) => v,
            None => return Err(SA_EHEX),
        };
        let lo = match b.get(at + k * 2 + 1).and_then(|c| hexval(*c)) {
            Some(v) => v,
            None => return Err(SA_EHEX),
        };
        if let Some(dst) = bytes.get_mut(k) {
            *dst = (hi << 4) | lo;
        }
        k += 1;
    }
    let name = match line.get(sep + 2..) {
        Some(v) => v,
        None => return Err(SA_ENAME),
    };
    if name.is_empty() {
        return Err(SA_ENAME);
    }
    Ok(SumLine { digest: Sum { bytes, len: dl }, binary, escaped, name })
}

/// 校验清单。
#[derive(Clone, Copy, Debug)]
pub struct Checker {
    alg: u8,
    names: [&'static str; CHECKS],
    n: usize,
    ok: usize,
}

impl Checker {
    /// 指定算法的空清单。
    pub const fn new(alg: u8) -> Checker {
        Checker { alg, names: [""; CHECKS], n: 0, ok: 0 }
    }

    /// 算法。
    pub fn alg(&self) -> u8 {
        self.alg
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 通过条数。
    pub fn ok_count(&self) -> usize {
        self.ok
    }

    /// 校验一行。
    pub fn check(&mut self, line: &'static str, data: &[u8]) -> Result<(), u32> {
        let l = parse_line(self.alg, line)?;
        let mut i = 0usize;
        while i < self.n {
            if self.names.get(i) == Some(&l.name) {
                return Err(SA_EDUP);
            }
            i += 1;
        }
        if self.n >= CHECKS {
            return Err(SA_ELEN);
        }
        let got = sum(self.alg, data)?;
        if got != l.digest {
            return Err(SA_ESUM);
        }
        if let Some(dst) = self.names.get_mut(self.n) {
            *dst = l.name;
        }
        self.n += 1;
        self.ok += 1;
        Ok(())
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SA_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账：八成功值 + 六失败码，且槽位连续。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(SA_EBAL);
        }
        const VALS: [u32; 8] = [64, 128, 3, 1, 1, 4, 0xba, 0xdd];
        const CODES: [u32; 6] =
            [SA_EALG, SA_EHEX, SA_EMIX, SA_EFMT, SA_ENAME, SA_EDUP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SA_EBAL),
            };
            if s.slot as usize != i {
                return Err(SA_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SA_EVALUE);
                        }
                    }
                    None => return Err(SA_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SA_EVALUE);
                        }
                    }
                    None => return Err(SA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SA_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SA_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(SA_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// SHA-256 文本模式校验行（"abc"）。
const L256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  abc";
/// SHA-512 文本模式校验行（"abc"）。
const L512: &str = concat!(
    "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a",
    "2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f  abc"
);
/// SHA-256 二进制模式行。
const L256B: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad *abc";

/// 四组向量：算法、输入、期望十六进制。
const VEC: [(u8, &str, &[u8]); 4] = [
    (ALG_SHA256, "", b"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
    (ALG_SHA256, "abc", b"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
    (ALG_SHA512, "", b"cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"),
    (ALG_SHA512, "abc", b"ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"),
];

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_DIGEST, Ok(digest_len(ALG_SHA256) as u32 * 2)));
    let _ = l.register(word_cell(1, OP_DIGEST, Ok(digest_len(ALG_SHA512) as u32 * 2)));
    let _ = l.register(word_cell(
        2,
        OP_PARSE,
        parse_line(ALG_SHA256, L256).map(|x| x.name_len() as u32),
    ));
    let _ = l.register(word_cell(3, OP_VERIFY, {
        let mut c = Checker::new(ALG_SHA256);
        c.check(L256, b"abc").map(|_| c.len() as u32)
    }));
    let _ = l.register(word_cell(
        4,
        OP_PARSE,
        parse_line(ALG_SHA256, L256B).map(|x| x.binary as u32),
    ));
    let _ = l.register(word_cell(5, OP_VERIFY, {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < VEC.len() {
            match VEC.get(i) {
                Some((alg, data, want)) => {
                    if sum(*alg, data.as_bytes()).map(|x| x.eq_hex(want)) == Ok(true) {
                        n = n.saturating_add(1);
                    }
                }
                None => break,
            }
            i += 1;
        }
        Ok(n)
    }));
    let _ = l.register(word_cell(6, OP_DIGEST, sum(ALG_SHA256, b"abc").map(|s| s.byte(0) as u32)));
    let _ = l.register(word_cell(7, OP_DIGEST, sum(ALG_SHA512, b"abc").map(|s| s.byte(0) as u32)));

    let _ = l.register(word_cell(8, OP_PARSE, by_name("sha1").map(|a| a as u32)));
    let _ = l.register(word_cell(9, OP_PARSE, {
        let bad: &'static str =
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015az  abc";
        parse_line(ALG_SHA256, bad).map(|x| x.name_len() as u32)
    }));
    let _ = l.register(word_cell(10, OP_PARSE, parse_line(ALG_SHA256, L512).map(|x| x.name_len() as u32)));
    let _ = l.register(word_cell(11, OP_PARSE, {
        let bad: &'static str =
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad xabc";
        parse_line(ALG_SHA256, bad).map(|x| x.name_len() as u32)
    }));
    let _ = l.register(word_cell(12, OP_PARSE, {
        let bad: &'static str =
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  ";
        parse_line(ALG_SHA256, bad).map(|x| x.name_len() as u32)
    }));
    let _ = l.register(word_cell(13, OP_VERIFY, {
        let mut c = Checker::new(ALG_SHA256);
        let _ = c.check(L256, b"abc");
        c.check(L256B, b"abc").map(|_| c.len() as u32)
    }));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：四组固定向量。
    let mut i = 0usize;
    let mut all = true;
    while i < VEC.len() {
        match VEC.get(i) {
            Some((alg, data, want)) => match sum(*alg, data.as_bytes()) {
                Ok(s) => {
                    if !s.eq_hex(want) {
                        all = false;
                    }
                }
                Err(_) => all = false,
            },
            None => all = false,
        }
        i += 1;
    }
    out[1] = all;

    // 二路：增量与一次喂完一致（跨 64/128 字节边界）。
    let mut buf = [0u8; 130];
    let mut k = 0usize;
    while k < buf.len() {
        buf[k] = (k as u8).wrapping_mul(13).wrapping_add(1);
        k += 1;
    }
    let one = sum(ALG_SHA256, &buf);
    let two = sum(ALG_SHA512, &buf);
    let mut a = match Sha::new(ALG_SHA256) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut b = match Sha::new(ALG_SHA512) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut j = 0usize;
    while j < buf.len() {
        let end = if j + 5 < buf.len() { j + 5 } else { buf.len() };
        match buf.get(j..end) {
            Some(s) => {
                a.update(s);
                b.update(s);
            }
            None => {
                a.update(&[]);
                b.update(&[]);
            }
        }
        j = end;
    }
    out[2] = one == Ok(a.finish())
        && two == Ok(b.finish())
        && digest_len(ALG_SHA256) == 32
        && digest_len(ALG_SHA512) == 64
        && one.map(|s| s.len()) == Ok(32)
        && two.map(|s| s.len()) == Ok(64);

    // 三路：校验行闸门。
    let bad_hex: &'static str =
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015az  abc";
    let bad_mode: &'static str =
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad xabc";
    let no_name: &'static str =
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  ";
    out[3] = parse_line(ALG_SHA256, bad_hex) == Err(SA_EHEX)
        && parse_line(ALG_SHA256, bad_mode) == Err(SA_EFMT)
        && parse_line(ALG_SHA256, no_name) == Err(SA_ENAME)
        && parse_line(ALG_SHA256, L512) == Err(SA_EMIX)
        && parse_line(ALG_SHA512, L256) == Err(SA_EMIX)
        && by_name("sha1") == Err(SA_EALG)
        && Sha::new(2) == Err(SA_EALG)
        && by_name("sha512sum") == Ok(ALG_SHA512)
        && {
            let mut c = Checker::new(ALG_SHA256);
            c.check(L256, b"abc").is_ok()
                && c.check(L256B, b"abc") == Err(SA_EDUP)
                && c.check(L256, b"abcd") == Err(SA_EDUP)
        };

    out
}

/// 十三项判据。
pub fn run_unx11522_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11522.sha2_family");
    let l = build_ledger();

    s.add_dyn(
        "SA1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "SA2-SHA256固定向量",
        {
            let a = sum(ALG_SHA256, b"abc");
            let e = sum(ALG_SHA256, b"");
            a.map(|s| s.eq_hex(b"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"))
                == Ok(true)
                && e.map(|s| s.eq_hex(b"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"))
                    == Ok(true)
                && a != e
        },
        "static",
    );

    s.add(
        "SA3-SHA512固定向量",
        {
            let a = sum(ALG_SHA512, b"abc");
            let e = sum(ALG_SHA512, b"");
            a.map(|x| {
                x.eq_hex(L512_HEX)
            }) == Ok(true)
                && e.map(|x| x.eq_hex(L512_EMPTY)) == Ok(true)
                && a.map(|x| x.len()) == Ok(64)
        },
        "static",
    );

    s.add(
        "SA4-摘要长度与十六进制",
        {
            let a = sum(ALG_SHA256, b"abc");
            let b = sum(ALG_SHA512, b"abc");
            a.map(|x| x.hex().1) == Ok(64)
                && b.map(|x| x.hex().1) == Ok(128)
                && a.map(|x| x.hex().0.get(0).copied()) == Ok(Some(b'b'))
                && b.map(|x| x.hex().0.get(0).copied()) == Ok(Some(b'd'))
                && l.sample(0).map(|x| x.value) == Some(64)
                && l.sample(1).map(|x| x.value) == Some(128)
        },
        "static",
    );

    s.add(
        "SA5-分块一致",
        {
            let mut all = true;
            let mut n = 0usize;
            while n <= 130 {
                let mut buf = [0u8; 130];
                let mut k = 0usize;
                while k < n {
                    buf[k] = (k as u8).wrapping_mul(17).wrapping_add(3);
                    k += 1;
                }
                let one = sum(ALG_SHA256, &buf[..n]);
                let two = sum(ALG_SHA512, &buf[..n]);
                let mut a = match Sha::new(ALG_SHA256) {
                    Ok(v) => v,
                    Err(_) => return s,
                };
                let mut b = match Sha::new(ALG_SHA512) {
                    Ok(v) => v,
                    Err(_) => return s,
                };
                let mut j = 0usize;
                while j < n {
                    let end = if j + 3 < n { j + 3 } else { n };
                    if let Some(x) = buf.get(j..end) {
                        a.update(x);
                        b.update(x);
                    }
                    j = end;
                }
                if one != Ok(a.finish()) || two != Ok(b.finish()) {
                    all = false;
                }
                n += 1;
            }
            all && l.sample(5).map(|x| x.value) == Some(4)
        },
        "static",
    );

    s.add(
        "SA6-算法名与编号",
        by_name("sha256") == Ok(ALG_SHA256)
            && by_name("sha256sum") == Ok(ALG_SHA256)
            && by_name("sha512") == Ok(ALG_SHA512)
            && by_name("sha512sum") == Ok(ALG_SHA512)
            && by_name("sha1") == Err(SA_EALG)
            && by_name("md5") == Err(SA_EALG)
            && Sha::new(2) == Err(SA_EALG)
            && l.sample(8).map(|x| x.code) == Some(SA_EALG),
        "static",
    );

    s.add(
        "SA7-校验行两种模式",
        parse_line(ALG_SHA256, L256).map(|x| (x.binary, x.name)) == Ok((false, "abc"))
            && parse_line(ALG_SHA256, L256B).map(|x| x.binary) == Ok(true)
            && parse_line(ALG_SHA512, L512).map(|x| x.name_len()) == Ok(3)
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(4).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "SA8-长度不符即混用",
        parse_line(ALG_SHA256, L512) == Err(SA_EMIX)
            && parse_line(ALG_SHA512, L256) == Err(SA_EMIX)
            && parse_line(ALG_SHA512, L256B) == Err(SA_EMIX)
            && l.sample(10).map(|x| x.code) == Some(SA_EMIX),
        "static",
    );

    s.add(
        "SA9-十六进制与分隔",
        {
            let bad_hex: &'static str =
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015az  abc";
            let bad_mode: &'static str =
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad xabc";
            let no_name: &'static str =
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  ";
            let no_sep: &'static str =
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015adabc";
            parse_line(ALG_SHA256, bad_hex) == Err(SA_EHEX)
                && parse_line(ALG_SHA256, bad_mode) == Err(SA_EFMT)
                && parse_line(ALG_SHA256, no_name) == Err(SA_ENAME)
                && parse_line(ALG_SHA256, no_sep) == Err(SA_EFMT)
                && l.sample(9).map(|x| x.code) == Some(SA_EHEX)
                && l.sample(11).map(|x| x.code) == Some(SA_EFMT)
                && l.sample(12).map(|x| x.code) == Some(SA_ENAME)
        },
        "static",
    );

    s.add(
        "SA10-重复与不符",
        {
            let mut c = Checker::new(ALG_SHA256);
            c.check(L256, b"abc").is_ok()
                && c.check(L256B, b"abc") == Err(SA_EDUP)
                && c.len() == 1
                && {
                    let mut d = Checker::new(ALG_SHA512);
                    d.check(L512, b"abc").is_ok() && d.check(L512, b"abc") == Err(SA_EDUP)
                }
                && l.sample(3).map(|x| x.value) == Some(1)
                && l.sample(13).map(|x| x.code) == Some(SA_EDUP)
        },
        "static",
    );

    s.add(
        "SA11-行长上限",
        parse_line(ALG_SHA256, LONG_LINE) == Err(SA_ELEN)
            && parse_line(ALG_SHA256, L256).is_ok()
            && LINE_MAX == 256,
        "static",
    );

    s.add(
        "SA12-两族各二向量",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < VEC.len() {
                match VEC.get(i) {
                    Some((alg, data, want)) => match sum(*alg, data.as_bytes()) {
                        Ok(x) => {
                            if !x.eq_hex(want) {
                                ok = false;
                            }
                        }
                        Err(_) => ok = false,
                    },
                    None => ok = false,
                }
                i += 1;
            }
            ok && l.sample(6).map(|x| x.value) == Some(0xba)
                && l.sample(7).map(|x| x.value) == Some(0xdd)
        },
        "static",
    );

    s.add(
        "SA13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && inj == [true; 4]
        },
        "static",
    );

    s
}

/// SHA-512("abc") 的十六进制字面量。
const L512_HEX: &[u8] = b"ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f";
/// SHA-512("") 的十六进制字面量。
const L512_EMPTY: &[u8] = b"cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e";
/// 超长行（257 字节）。
const LONG_LINE: &str = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.rejects(), 0);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11522_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn known_sha256() {
        assert_eq!(
            sum(ALG_SHA256, b"abc").map(|s| s.eq_hex(b"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")),
            Ok(true)
        );
        assert_eq!(
            sum(ALG_SHA256, b"").map(|s| s.eq_hex(b"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")),
            Ok(true)
        );
    }

    #[test]
    fn known_sha512() {
        assert_eq!(sum(ALG_SHA512, b"abc").map(|s| s.eq_hex(L512_HEX)), Ok(true));
        assert_eq!(sum(ALG_SHA512, b"").map(|s| s.eq_hex(L512_EMPTY)), Ok(true));
    }

    #[test]
    fn incremental_matches() {
        let mut buf = [0u8; 200];
        for i in 0..200 {
            buf[i] = i as u8;
        }
        let mut a = Sha::new(ALG_SHA256).unwrap();
        let mut b = Sha::new(ALG_SHA512).unwrap();
        let mut j = 0;
        while j < 200 {
            let end = if j + 9 < 200 { j + 9 } else { 200 };
            a.update(&buf[j..end]);
            b.update(&buf[j..end]);
            j = end;
        }
        assert_eq!(a.finish(), sum(ALG_SHA256, &buf).unwrap());
        assert_eq!(b.finish(), sum(ALG_SHA512, &buf).unwrap());
    }

    #[test]
    fn line_gates() {
        assert_eq!(
            parse_line(ALG_SHA256, L256).map(|x| x.name),
            Ok("abc")
        );
        assert_eq!(parse_line(ALG_SHA512, L256), Err(SA_EMIX));
        assert_eq!(
            parse_line(ALG_SHA256, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad xabc"),
            Err(SA_EFMT)
        );
        assert_eq!(by_name("sha1"), Err(SA_EALG));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
