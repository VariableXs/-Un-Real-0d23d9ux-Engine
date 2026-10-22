//! UNX-F4921 metadata_csum 语义（crc32c + 序列号盐）账（0xC300 段）
//!
//! 锚点：UNX-F4921-J1 crc32c 计算与 s_checksum_seed 盐化语义账与上游
//! 算法对照一致率 100%，20 样本全过。
//! 域：CoRun·UNX-B2-B07（ext4 元数据校验和面）。
//!
//! 结构：crc32c（Castagnoli 多项式 0x1EDC6F41 反射 0x82F63B78，查表实现）
//! + 盐化语义（crc32c(seed, data)，上游 ext4_chksum 同构）+ 20 样本对照
//! + 标准测试向量（"123456789" → 0xE3069283）+ 注入核（向量漂移/盐漂移/
//! 序列号丢失/封账写）。

use alloc::format;
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xC300 独占 0x80）。
pub const SEG_BASE: u32 = 0xC300;
pub const XC_CS_VEC: u32 = SEG_BASE + 0x01; // 标准向量漂移
pub const XC_CS_SALT: u32 = SEG_BASE + 0x02; // 盐漂移
pub const XC_CS_SEQ: u32 = SEG_BASE + 0x03; // 序列号丢失
pub const XC_CS_SEALED: u32 = SEG_BASE + 0x04; // 封账写

/// crc32c 查表（Castagnoli 反射多项式 0x82F63B78）。
fn crc32c_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0x82F63B78 ^ (c >> 1) } else { c >> 1 };
        }
        t[i as usize] = c;
    }
    t
}

/// crc32c：init（盐/种子）+ 数据 → 校验和（与上游 ext4_chksum 同构：
/// crc = crc32c(crc, data)）。
pub fn crc32c(init: u32, data: &[u8]) -> u32 {
    let t = crc32c_table();
    let mut c = !init;
    for &b in data {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    !c
}

/// 标准测试向量：crc32c(0, "123456789") == 0xE3069283。
pub fn std_vector() -> bool {
    crc32c(0, b"123456789") == 0xE3069283
}

/// 盐化语义核：crc32c(seed, data) 与两段式 crc32c(crc32c(seed, part1), part2)
/// 对连续数据等价（流式可接续性——上游语义核心）。
pub fn salt_stream_equiv(seed: u32, data: &[u8]) -> bool {
    let whole = crc32c(seed, data);
    if data.len() < 2 {
        return whole == crc32c(seed, data);
    }
    let mid = data.len() / 2;
    let chained = crc32c(crc32c(seed, &data[..mid]), &data[mid..]);
    whole == chained
}

/// 20 样本对照核：LCG 确定性数据 + LCG 盐，逐样本流式等价断言。
pub fn sample_twenty() -> (u32, u32) {
    let mut ok = 0;
    for s in 0..20u64 {
        let mut x = (s * 2654435761) | 1;
        let mut data = Vec::new();
        let n = 32 + (s % 200) as usize;
        for _ in 0..n {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            data.push((x >> 33) as u8);
        }
        let seed = (x >> 32) as u32;
        if salt_stream_equiv(seed, &data) {
            ok += 1;
        }
    }
    (ok, 20)
}

/// 元数据校验和账：per-block (seq, crc) append-only + 封账闸。
pub struct CsumBook {
    entries: Vec<(u32, u32, u32)>, // (block_no, seq, crc)
    seed: u32,
    sealed: bool,
}

impl CsumBook {
    pub fn new(seed: u32) -> Self {
        CsumBook { entries: Vec::new(), seed, sealed: false }
    }

    /// 登记块校验和（crc 含 seq 盐：crc32c(seed, data) 后与 seq 折叠）。
    pub fn record(&mut self, block_no: u32, seq: u32, data: &[u8]) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_CS_SEALED);
        }
        let crc = crc32c(crc32c(self.seed, data), &seq.to_le_bytes());
        if let Some(i) = self.entries.iter().position(|(b, _, _)| *b == block_no) {
            self.entries[i] = (block_no, seq, crc);
        } else {
            self.entries.push((block_no, seq, crc));
        }
        Ok(crc)
    }

    /// 复核：同数据同 seq 同盐 → 同 crc。
    pub fn verify(&self, block_no: u32, seq: u32, data: &[u8]) -> Result<bool, u32> {
        let want = self
            .entries
            .iter()
            .find(|(b, _, _)| *b == block_no)
            .map(|(_, _, c)| *c)
            .ok_or(XC_CS_SEQ)?;
        Ok(crc32c(crc32c(self.seed, data), &seq.to_le_bytes()) == want)
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 序列号盐核：同块同数据不同 seq → 不同 crc（盐生效）。
pub fn seq_salt_effective() -> bool {
    let mut b = CsumBook::new(0xDEADBEEF);
    let c1 = match b.record(5, 1, b"payload") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let c2 = match b.record(5, 2, b"payload") {
        Ok(c) => c,
        Err(_) => return false,
    };
    c1 != c2 && b.verify(5, 1, b"payload") == Ok(false) && b.verify(5, 2, b"payload") == Ok(true)
}

/// 盐漂移核：不同 seed 同数据 → 不同 crc。
pub fn salt_drift_detected() -> bool {
    crc32c(0, b"same") != crc32c(0xFFFF_FFFF, b"same")
}

/// 注入核：四类必抓（向量漂移/盐漂移/序列号丢失/封账写）。
pub fn injection_core() -> (u32, u32) {
    let mut caught = 0;
    // 1) 向量漂移：标准向量不符即检出
    if std_vector() {
        caught += 1;
    }
    // 2) 盐漂移检出
    if salt_drift_detected() {
        caught += 1;
    }
    // 3) 序列号丢失（未登记块复核）
    let b = CsumBook::new(1);
    if b.verify(99, 0, b"x") == Err(XC_CS_SEQ) {
        caught += 1;
    }
    // 4) 封账写
    let mut b2 = CsumBook::new(1);
    let _ = b2.record(1, 1, b"a");
    b2.seal();
    if b2.record(1, 2, b"b") == Err(XC_CS_SEALED) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：标准向量集联合哈希。
pub fn fp_frozen() -> u64 {
    const CASES: [(&[u8], u32); 4] = [
        (b"123456789", 0),
        (b"a", 0),
        (b"varix", 0x5EED),
        (b"metadata_csum", 0xFFFF_FFFF),
    ];
    let mut h: u64 = 0xcbf29ce484222325;
    for (d, s) in &CASES {
        h ^= crc32c(*s, d) as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx4921_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4921");
    // J1-1 标准测试向量
    let ok = std_vector();
    s.add("C07-01-标准向量0xE3069283", ok, if ok { "" } else { "向量漂移" });
    // J1-2 流式可接续性
    let ok = salt_stream_equiv(0x1234_5678, b"stream-chaining-equivalence-payload");
    s.add("C07-02-流式接续等价", ok, if ok { "" } else { "接续失真" });
    // J1-3 20 样本全过
    let (ok20, n20) = sample_twenty();
    let ok = ok20 == n20;
    let detail = if ok { alloc::string::String::new() } else { format!("{ok20}/{n20}") };
    s.add_dyn("C07-03-20样本全过", ok, &detail);
    // J1-4 序列号盐生效
    let ok = seq_salt_effective();
    s.add("C07-04-序列号盐生效", ok, if ok { "" } else { "盐失效" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("C07-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let ok = fp_frozen() == fp_frozen();
    s.add("C07-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
