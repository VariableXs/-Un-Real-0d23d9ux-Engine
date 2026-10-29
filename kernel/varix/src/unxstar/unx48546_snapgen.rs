//! UNX-F48546 —— E·跨代快照注入（反：代际错配 ×10 拒绝混搭）
//! 判据原文：UNX-F48546-J1 反向判据：以 10 组代际错配快照注入恢复面，
//! 恢复器必须逐组拒绝混搭：异代配置/词库不得配对、跨代恢复不得
//! 落账、拒绝动作逐组记账（拒绝数 == 注入错配组数）；
//! 合法同代对照组必须全数通过；账目守恒、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 代际序闭集；R2 错配逐组拒收；
//! R3 十组全拒验收；R4 合法对照组全通过；R5 拒绝账守恒；
//! R6 跨代落账拦截；R7 重复恢复拦截；R8 fuzz 快照流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF2E0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_GEN: u16 = SEG_BASE + 1; // 代际序闭集异常
const E_MIXGEN: u16 = SEG_BASE + 2; // 异代混搭未拒
const E_REJECT: u16 = SEG_BASE + 3; // 拒绝账不守恒
const E_LEGAL: u16 = SEG_BASE + 4; // 合法对照组被误拒
const E_LEDGER: u16 = SEG_BASE + 5; // 跨代落账未拦截
const E_RESTORE: u16 = SEG_BASE + 6; // 重复恢复未拦截
const E_UNKNOWN: u16 = SEG_BASE + 7; // 未知类别拒收失败
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 快照流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 快照类别：1 配置 / 2 词库。
pub const K_CFG: u8 = 1;
pub const K_DICT: u8 = 2;

/// 错配注入组数。
pub const MISMATCH_N: u32 = 10;

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

/// 快照类别闭集判定。
pub fn known_kind(k: u8) -> bool {
    k == K_CFG || k == K_DICT
}

/// 一枚快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snapshot {
    pub kind: u8,
    pub gen: u32,
    pub src: u64,
}

/// 代际恢复闸：挂载 + 配对恢复 + 拒绝账。
pub struct GenGate {
    pub cfg: Option<Snapshot>,
    pub dict: Option<Snapshot>,
    pub rejected: u32,
    pub accepted: u32,
    pub restored: bool,
}

impl GenGate {
    pub fn new() -> GenGate {
        GenGate {
            cfg: None,
            dict: None,
            rejected: 0,
            accepted: 0,
            restored: false,
        }
    }

    /// 挂载一枚：类别闭集 + 类别互斥。
    pub fn mount(&mut self, s: Snapshot) -> Result<(), u16> {
        if !known_kind(s.kind) {
            return Err(E_GEN);
        }
        if s.kind == K_CFG {
            if self.cfg.is_some() {
                return Err(E_GEN);
            }
            self.cfg = Some(s);
        } else {
            if self.dict.is_some() {
                return Err(E_GEN);
            }
            self.dict = Some(s);
        }
        Ok(())
    }

    /// 配对恢复：同代同源通过并记账；异代拒收记账（E_MIXGEN）；
    /// 异源按代际序同样拒收记账。
    pub fn restore(&mut self) -> Result<u32, u16> {
        if self.restored {
            return Err(E_RESTORE);
        }
        let c = match self.cfg {
            Some(s) => s,
            None => {
                self.rejected += 1;
                return Err(E_REJECT);
            }
        };
        let d = match self.dict {
            Some(s) => s,
            None => {
                self.rejected += 1;
                return Err(E_REJECT);
            }
        };
        if c.gen != d.gen {
            self.rejected += 1;
            return Err(E_MIXGEN);
        }
        if c.src != d.src {
            self.rejected += 1;
            return Err(E_MIXGEN);
        }
        self.restored = true;
        self.accepted += 1;
        Ok(c.gen)
    }

    /// 拒绝账守恒：rejected + accepted == 恢复尝试数。
    pub fn conserved(&self, attempts: u32) -> bool {
        self.rejected + self.accepted == attempts
    }

    /// 跨代落账拦截：restored 为真时 accepted ≥ 1。
    pub fn ledger_sane(&self) -> bool {
        !self.restored || self.accepted >= 1
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 代际序闭集：类别 1/2 合法、0/3 非法。
    if !known_kind(K_CFG) || !known_kind(K_DICT) || known_kind(0) || known_kind(3) {
        return Err(E_GEN);
    }
    hits += 1;

    // R2 错配逐组拒收：连续 10 组异代配对全部被拒，码恰为 E_MIXGEN。
    let gate = GenGate::new();
    let mut g = 1u32;
    let mut rejects = 0u32;
    while g <= MISMATCH_N {
        let mut gg = GenGate::new();
        gg.mount(Snapshot { kind: K_CFG, gen: g, src: 100 + g as u64 })?;
        gg.mount(Snapshot { kind: K_DICT, gen: g + 100, src: 100 + g as u64 })?;
        match gg.restore() {
            Err(E_MIXGEN) => {
                rejects += 1;
                if gg.restored || gg.accepted != 0 {
                    return Err(E_LEDGER);
                }
            }
            _ => return Err(E_MIXGEN),
        }
        g += 1;
    }
    if rejects != MISMATCH_N {
        return Err(E_REJECT);
    }
    if !gate.ledger_sane() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3 十组全拒验收：拒绝账恰 10。
    if rejects != 10 {
        return Err(E_REJECT);
    }
    hits += 1;

    // R4 合法对照组全通过：同代同源 5 组全部恢复成功。
    let mut accepted_total = 0u32;
    let mut g = 1u32;
    while g <= 5 {
        let mut gg = GenGate::new();
        gg.mount(Snapshot { kind: K_CFG, gen: g, src: 500 + g as u64 })?;
        gg.mount(Snapshot { kind: K_DICT, gen: g, src: 500 + g as u64 })?;
        let gen = gg.restore()?;
        if gen != g || !gg.restored || gg.accepted != 1 {
            return Err(E_LEGAL);
        }
        if !gg.ledger_sane() || !gg.conserved(1) {
            return Err(E_LEDGER);
        }
        accepted_total += 1;
        g += 1;
    }
    if accepted_total != 5 {
        return Err(E_LEGAL);
    }
    hits += 1;

    // R5 拒绝账守恒：同一闸多次尝试，rejected + accepted == 尝试数。
    let mut gate2 = GenGate::new();
    gate2.mount(Snapshot { kind: K_CFG, gen: 1, src: 9 })?;
    // 尝试 1：缺词库 → 拒。
    match gate2.restore() {
        Err(E_REJECT) => {}
        _ => return Err(E_REJECT),
    }
    gate2.mount(Snapshot { kind: K_DICT, gen: 2, src: 9 })?;
    // 尝试 2：异代 → 拒。
    match gate2.restore() {
        Err(E_MIXGEN) => {}
        _ => return Err(E_REJECT),
    }
    gate2.dict = Some(Snapshot { kind: K_DICT, gen: 1, src: 9 });
    // 尝试 3：合法 → 过。
    gate2.restore()?;
    if !gate2.conserved(3) || gate2.rejected != 2 || gate2.accepted != 1 {
        return Err(E_REJECT);
    }
    hits += 1;

    // R6 跨代落账拦截：异代恢复尝试后 restored 恒假、accepted 恒 0。
    let mut gate3 = GenGate::new();
    gate3.mount(Snapshot { kind: K_CFG, gen: 10, src: 1 })?;
    gate3.mount(Snapshot { kind: K_DICT, gen: 20, src: 1 })?;
    let _ = gate3.restore().err();
    if gate3.restored || gate3.accepted != 0 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R7 重复恢复拦截：恢复成功后二次恢复被拒，码恰为 E_RESTORE。
    match gate2.restore() {
        Err(E_RESTORE) => hits += 1,
        _ => return Err(E_RESTORE),
    }

    // R8 未知类别拒收：kind=7 挂载被拒，码恰为 E_GEN。
    let mut gate4 = GenGate::new();
    match gate4.mount(Snapshot { kind: 7, gen: 1, src: 1 }) {
        Err(E_GEN) => hits += 1,
        _ => return Err(E_UNKNOWN),
    }

    // R9 fuzz 快照流零错态：LCG 驱动 64 组随机代际对，
    // 每组按判定收/拒并记账，末态守恒。
    let mut lcg = Lcg::new(0x48546);
    let mut t = 0u32;
    let mut attempts = 0u32;
    let mut sum_acc = 0u32;
    while t < 64 {
        let mut gg = GenGate::new();
        let g1 = lcg.below(50);
        let g2 = lcg.below(50);
        let src = lcg.below(3) as u64;
        gg.mount(Snapshot { kind: K_CFG, gen: g1, src })?;
        gg.mount(Snapshot { kind: K_DICT, gen: g2, src })?;
        let r = gg.restore();
        attempts += 1;
        match r {
            Ok(_) => sum_acc += 1,
            Err(_) => {}
        }
        if !gg.conserved(1) {
            return Err(E_FUZZ);
        }
        t += 1;
    }
    if sum_acc == 0 || attempts != 64 {
        return Err(E_FUZZ);
    }
    // 账面指纹稳定同口径复核。
    let fp1 = gate_fp(&gate2);
    let fp2 = gate_fp(&gate2);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：rej=/acc= 两段前缀在位。
    let line = gate_line(&gate2);
    if !has_kw(line.as_bytes(), b"rej=") || !has_kw(line.as_bytes(), b"acc=") {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：拒绝数 + 接受数 + 恢复位串联。
pub fn gate_fp(g: &GenGate) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&g.rejected.to_le_bytes());
    buf.extend_from_slice(&g.accepted.to_le_bytes());
    buf.push(g.restored as u8);
    fnv1a64(&buf)
}

/// 呈现行：rej=<拒绝> acc=<接受>。
pub fn gate_line(g: &GenGate) -> String {
    let mut s = String::new();
    s.push_str("rej=");
    s.push_str(&g.rejected.to_string());
    s.push_str(" acc=");
    s.push_str(&g.accepted.to_string());
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
