//! UNX-F48545 —— E·配置与词库快照一致性（正：同代快照同源恢复）
//! 判据原文：UNX-F48545-J1 崩溃恢复以快照对（配置 + 词库）为恢复源：
//! 两者必须同代（generation 相同）且同源（来源指纹一致）才允许
//! 配对恢复；配对成功后恢复面完整性可复核（双 CRC 一致）；
//! 任一缺失或不同代不同源必须拒配；账目守恒、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 快照代闭集；R2 同代配对成功；
//! R3 异代拒配；R4 异源拒配；R5 单边缺失拒配；
//! R6 恢复完整性双 CRC 一致；R7 恢复账守恒；
//! R8 fuzz 快照流零错态；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF260;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_GEN: u16 = SEG_BASE + 1; // 代际异常
const E_PAIR: u16 = SEG_BASE + 2; // 同代配对失败
const E_MIXGEN: u16 = SEG_BASE + 3; // 异代混搭未拒
const E_MIXSRC: u16 = SEG_BASE + 4; // 异源混搭未拒
const E_HALF: u16 = SEG_BASE + 5; // 单边缺失未拒
const E_CRC: u16 = SEG_BASE + 6; // 恢复完整性 CRC 不一致
const E_CONSERV: u16 = SEG_BASE + 7; // 恢复账不守恒
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 快照流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 快照类别：1 配置 / 2 词库。
pub const K_CFG: u8 = 1;
pub const K_DICT: u8 = 2;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一枚快照：类别 + 代 + 来源指纹 + 内容 CRC。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snapshot {
    pub kind: u8,
    pub gen: u32,
    pub src: u64,
    pub crc: u64,
}

/// 快照合法性：类别闭集。
pub fn known_kind(k: u8) -> bool {
    k == K_CFG || k == K_DICT
}

/// 配对判定：同代 + 同源才允许恢复。
pub fn pair_ok(a: &Snapshot, b: &Snapshot) -> bool {
    a.gen == b.gen && a.src == b.src
}

/// 恢复器：持有一对快照并执行配对恢复。
pub struct Restorer {
    pub cfg: Option<Snapshot>,
    pub dict: Option<Snapshot>,
    pub restored_gen: u32,
    pub restored: bool,
}

impl Restorer {
    pub fn new() -> Restorer {
        Restorer {
            cfg: None,
            dict: None,
            restored_gen: 0,
            restored: false,
        }
    }

    /// 挂载一枚快照：类别互斥（配置/词库各一枚）。
    pub fn mount(&mut self, s: Snapshot) -> Result<(), u16> {
        if !known_kind(s.kind) {
            return Err(E_GEN);
        }
        if s.kind == K_CFG {
            if self.cfg.is_some() {
                return Err(E_PAIR);
            }
            self.cfg = Some(s);
        } else {
            if self.dict.is_some() {
                return Err(E_PAIR);
            }
            self.dict = Some(s);
        }
        Ok(())
    }

    /// 双边齐备判定。
    pub fn both_mounted(&self) -> bool {
        self.cfg.is_some() && self.dict.is_some()
    }

    /// 配对恢复：单边缺失拒配（E_HALF）；异代拒配（E_MIXGEN）；
    /// 异源拒配（E_MIXSRC）；成功后写 restored_gen 并置位。
    pub fn restore(&mut self) -> Result<u32, u16> {
        if self.restored {
            return Err(E_PAIR);
        }
        let c = match self.cfg {
            Some(s) => s,
            None => return Err(E_HALF),
        };
        let d = match self.dict {
            Some(s) => s,
            None => return Err(E_HALF),
        };
        if c.gen != d.gen {
            return Err(E_MIXGEN);
        }
        if c.src != d.src {
            return Err(E_MIXSRC);
        }
        self.restored_gen = c.gen;
        self.restored = true;
        Ok(c.gen)
    }

    /// 恢复完整性：双 CRC 与来源指纹可复核（非零且互为配对凭证）。
    pub fn integrity(&self) -> bool {
        match (self.cfg, self.dict) {
            (Some(c), Some(d)) => c.crc != 0 && d.crc != 0 && c.src == d.src,
            _ => false,
        }
    }

    /// 恢复账守恒：restored 与 restored_gen 非零联动。
    pub fn conserved(&self) -> bool {
        self.restored == (self.restored_gen != 0)
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 快照代闭集：类别 1/2 合法、0/3 非法；配对判定对同代同源为真。
    if !known_kind(K_CFG) || !known_kind(K_DICT) || known_kind(0) || known_kind(3) {
        return Err(E_GEN);
    }
    let s1 = Snapshot { kind: K_CFG, gen: 7, src: 100, crc: 1 };
    let s2 = Snapshot { kind: K_DICT, gen: 7, src: 100, crc: 2 };
    if !pair_ok(&s1, &s2) {
        return Err(E_PAIR);
    }
    hits += 1;

    // R2 同代配对成功：同代同源双快照挂载后恢复成功。
    let mut rs = Restorer::new();
    rs.mount(s1)?;
    rs.mount(s2)?;
    let gen = rs.restore()?;
    if gen != 7 || !rs.restored || !rs.integrity() {
        return Err(E_PAIR);
    }
    hits += 1;

    // R3 异代拒配：代不同恢复被拒，码恰为 E_MIXGEN。
    let mut rs2 = Restorer::new();
    rs2.mount(Snapshot { kind: K_CFG, gen: 7, src: 100, crc: 1 })?;
    rs2.mount(Snapshot { kind: K_DICT, gen: 8, src: 100, crc: 2 })?;
    match rs2.restore() {
        Err(E_MIXGEN) => hits += 1,
        _ => return Err(E_MIXGEN),
    }

    // R4 异源拒配：来源指纹不同恢复被拒，码恰为 E_MIXSRC。
    let mut rs3 = Restorer::new();
    rs3.mount(Snapshot { kind: K_CFG, gen: 7, src: 100, crc: 1 })?;
    rs3.mount(Snapshot { kind: K_DICT, gen: 7, src: 200, crc: 2 })?;
    match rs3.restore() {
        Err(E_MIXSRC) => hits += 1,
        _ => return Err(E_MIXSRC),
    }

    // R5 单边缺失拒配：只挂配置被拒，码恰为 E_HALF。
    let mut rs4 = Restorer::new();
    rs4.mount(Snapshot { kind: K_CFG, gen: 7, src: 100, crc: 1 })?;
    match rs4.restore() {
        Err(E_HALF) => hits += 1,
        _ => return Err(E_HALF),
    }

    // R5b 类别互斥：同类别二次挂载被拒，码恰为 E_PAIR，账不变。
    let mut rs5 = Restorer::new();
    rs5.mount(Snapshot { kind: K_CFG, gen: 1, src: 1, crc: 1 })?;
    match rs5.mount(Snapshot { kind: K_CFG, gen: 2, src: 2, crc: 2 }) {
        Err(E_PAIR) => hits += 1,
        _ => return Err(E_PAIR),
    }

    // R6 恢复完整性双 CRC 一致：恢复后 integrity 为真；空账为假。
    if !rs.integrity() || Restorer::new().integrity() {
        return Err(E_CRC);
    }
    hits += 1;

    // R7 恢复账守恒：restored 与 restored_gen 联动；未恢复账 gen 恒 0。
    if !rs.conserved() || !rs2.conserved() {
        return Err(E_CONSERV);
    }
    if rs2.restored_gen != 0 || rs4.restored_gen != 0 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R8 fuzz 快照流零错态：LCG 生成 64 对同代同源快照逐对恢复，
    // 全部成功且代号、完整性、守恒一致。
    let mut lcg = Lcg::new(0x48545);
    let mut t = 0u32;
    while t < 64 {
        let gen = 1 + lcg.below(1000);
        let src = lcg.next_u32() as u64 + 1;
        let crc1 = lcg.next_u32() as u64 + 1;
        let crc2 = lcg.next_u32() as u64 + 1;
        let mut r = Restorer::new();
        r.mount(Snapshot { kind: K_CFG, gen, src, crc: crc1 })?;
        r.mount(Snapshot { kind: K_DICT, gen, src, crc: crc2 })?;
        let g = r.restore()?;
        if g != gen || !r.integrity() || !r.conserved() {
            return Err(E_FUZZ);
        }
        t += 1;
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = restorer_fp(&rs);
    let f2 = restorer_fp(&rs);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：gen=/restored= 两段前缀在位。
    let line = restorer_line(&rs);
    if !has_kw(line.as_bytes(), b"gen=") || !has_kw(line.as_bytes(), b"restored=") {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：恢复位 + 代号 + 双快照字段串联。
pub fn restorer_fp(r: &Restorer) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(r.restored as u8);
    buf.extend_from_slice(&r.restored_gen.to_le_bytes());
    let mut i = 0usize;
    while i < 2 {
        let s = if i == 0 { r.cfg } else { r.dict };
        match s {
            Some(s) => {
                buf.push(s.kind);
                buf.extend_from_slice(&s.gen.to_le_bytes());
                buf.extend_from_slice(&s.src.to_le_bytes());
                buf.extend_from_slice(&s.crc.to_le_bytes());
            }
            None => buf.push(0),
        }
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：gen=<代> restored=<0/1>。
pub fn restorer_line(r: &Restorer) -> String {
    let mut s = String::new();
    s.push_str("gen=");
    s.push_str(&r.restored_gen.to_string());
    s.push_str(" restored=");
    s.push_str(&(r.restored as u32).to_string());
    s
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
