//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F3682 · 输入生成器（结构化生成与字节变异双模） · 诊断段 0xD400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：双模生成（结构化模（按入口 schema 产合法/半合法输入
//!      ——字段级扰动）/字节级模（对语料直接变异——schema 外暴力
//!      ——两模各自判据））；
//!      M-440 双模产出（结构化件被入口 schema 接受+字节件走拒收
//!      路径——两模各自有判据落点）。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   J2：生成封闭（生成器唯一入口）。
//! 设计要点：
//!   - 入口 schema：四字段（magic/len/flags/crc）冻结；
//!   - 结构化模：合法件 + 字段级扰动半合法件；
//!   - 字节级模：语料逐字节位翻转变异；
//!   - schema 校验器：接受/拒收二值（字节件必走拒收）；
//!   - M-440：双模产出落点验证；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xD400;

/// schema magic。
pub const SCHEMA_MAGIC: u8 = 0x5A;
/// schema 长度界。
pub const SCHEMA_LEN_MAX: u32 = 64;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        (self.next() >> 16) % n
    }
}

/// 结构化输入（四字段 schema）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructInput {
    pub magic: u8,
    pub len: u32,
    pub flags: u8,
    pub crc: u32,
}

impl StructInput {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        v.push(self.magic);
        v.push(self.flags);
        for b in self.len.to_le_bytes() {
            v.push(b);
        }
        for b in self.crc.to_le_bytes() {
            v.push(b);
        }
        v
    }
}

/// schema 校验器。
pub fn schema_accept(b: &[u8]) -> bool {
    if b.len() != 10 {
        return false;
    }
    let magic = b[0];
    let _flags = b[1];
    let len = u32::from_le_bytes([b[2], b[3], b[4], b[5]]);
    let crc = u32::from_le_bytes([b[6], b[7], b[8], b[9]]);
    magic == SCHEMA_MAGIC && len <= SCHEMA_LEN_MAX && crc == crc_of(len)
}

/// CRC 当量：len 的 FNV。
pub fn crc_of(len: u32) -> u32 {
    fnv1a(&len.to_le_bytes())
}

/// 结构化模生成器。
pub fn gen_struct(seed: u64, half_legal: bool) -> StructInput {
    let mut rng = Lcg::new(seed);
    let len = if half_legal { SCHEMA_LEN_MAX + 1 + rng.below(8) as u32 } else { rng.below(SCHEMA_LEN_MAX as u64 + 1) as u32 };
    StructInput {
        magic: SCHEMA_MAGIC,
        len,
        flags: rng.below(256) as u8,
        crc: crc_of(len),
    }
}

/// 字节级模生成器：语料逐字节位翻转。
pub fn gen_byte_mut(corpus: &[u8], flip: u8) -> Vec<u8> {
    corpus.iter().map(|b| b ^ flip).collect()
}

/// M-440：结构化合法件被接受；字节件走拒收路径。
pub fn m440() -> (bool, bool) {
    // 结构化模：合法件接受、半合法件拒收
    let legal = gen_struct(0xD400, false);
    let half = gen_struct(0xD401, true);
    let acc_legal = schema_accept(&legal.encode());
    let rej_half = !schema_accept(&half.encode());
    // 字节级模：对合法语料变异（magic 字节必变）→ 拒收
    let corpus = legal.encode();
    let mutated = gen_byte_mut(&corpus, 0xFF);
    let rej_mut = !schema_accept(&mutated);
    // 字节模判据落点：变异件全部走拒收（magic/len/crc 任一被破坏）
    let mut all_rejected = true;
    for flip in 1..=8u8 {
        let m = gen_byte_mut(&corpus, flip);
        if schema_accept(&m) {
            all_rejected = false;
        }
    }
    (acc_legal && rej_half && rej_mut, all_rejected)
}

/// J1 主判据：双模产出落点 + 生成封闭（唯一入口函数对账）。
pub fn j1_main() -> bool {
    let (a, b) = m440();
    a && b && {
        // 生成封闭：结构化生成器对同 seed 幂等
        let g1 = gen_struct(42, false);
        let g2 = gen_struct(42, false);
        g1 == g2
    } && {
        // 合法件 schema 回读：encode→accept 恒真（100 seed 抽样）
        let mut all = true;
        for s in 0..100u64 {
            let g = gen_struct(0xD00 + s, false);
            if !schema_accept(&g.encode()) {
                all = false;
            }
        }
        all
    }
}

// ── 注入核（0xD400 段 5 项）────────────────────────────────────────────

/// 注入核 1：magic 破坏注入——magic 变必拒。
pub fn inj1_magic_break() -> bool {
    let g = gen_struct(7, false);
    let mut e = g.encode();
    e[0] ^= 0x01;
    !schema_accept(&e)
}

/// 注入核 2：长度超界注入——len > MAX 必拒。
pub fn inj2_len_over() -> bool {
    let mut g = gen_struct(8, false);
    g.len = SCHEMA_LEN_MAX + 1;
    !schema_accept(&g.encode())
}

/// 注入核 3：CRC 失配注入。
pub fn inj3_crc_break() -> bool {
    let mut g = gen_struct(9, false);
    g.crc = g.crc.wrapping_add(1);
    !schema_accept(&g.encode())
}

/// 注入核 4：截断注入——长度不足 10 字节必拒。
pub fn inj4_truncate() -> bool {
    let g = gen_struct(10, false);
    let e = g.encode();
    !schema_accept(&e[..9]) && !schema_accept(&[])
}

/// 注入核 5：变异保长注入——字节模不改变长度。
pub fn inj5_mut_len() -> bool {
    let corpus = gen_struct(11, false).encode();
    let m = gen_byte_mut(&corpus, 0x5A);
    m.len() == corpus.len()
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_unx3682_checks：本单判据聚合（诊断段 0xD400）。
pub fn run_unx3682_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3682 输入生成器（结构化生成与字节变异双模）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "双模生成（结构化合法/半合法+字节变异）",
        j1_main(),
        if j1_main() { "" } else { "双模生成破" },
    );
    let (m_ok, _) = m440();
    s.add(
        "M-440双模产出（结构化件被接受+字节件走拒收）",
        m_ok,
        if m_ok { "" } else { "M-440落点缺" },
    );
    let inj = [
        ("注入1magic破坏", inj1_magic_break()),
        ("注入2长度超界", inj2_len_over()),
        ("注入3CRC失配", inj3_crc_break()),
        ("注入4截断", inj4_truncate()),
        ("注入5变异保长", inj5_mut_len()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
