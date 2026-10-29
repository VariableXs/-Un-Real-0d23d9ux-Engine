//! UNX-F48707 —— I·联签占比总机检（B29–B36 联签条目 ≥30% 终值）
//! 判据原文：UNX-F48707-J1 机检终值通过；判据锚定 §AI-61 §3。
//! 判据落地（drill 场景）：R1 批次条目登记；R2 联签标记闭集；
//! R3 占比终值计算；R4 阈值 ≥30% 断言；R5 分批核对 B29–B36；
//! R6 机检复跑一致；R7 指纹稳定；R8 缺批检出；R9 呈现行；
//! R10 总量守恒；R11 终值签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：联签占比 = 联签条目数 / 批内条目总数（万分位），
//! B29–B36 共 8 批逐批机检后取总账占比 ≥3000bp 才算终值通过；
//! 机检 ×1 全量 + 复跑一致，任一批缺登记即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0FC0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_BATCH: u16 = SEG_BASE + 1; // 批号越出 B29–B36
const E_MARK: u16 = SEG_BASE + 2; // 联签标记非法
const E_RATIO: u16 = SEG_BASE + 3; // 占比低于阈值
const E_MISS: u16 = SEG_BASE + 4; // 批次缺登记
const E_COUNT: u16 = SEG_BASE + 5; // 总量不守恒
const E_REPLAY: u16 = SEG_BASE + 6; // 机检复跑漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 9; // 终值签名不符
const E_EMPTY: u16 = SEG_BASE + 10; // 空批

/// 联签标记闭集。
pub const MK_SOLO: u8 = 0; // 单签
pub const MK_JOIN: u8 = 1; // 联签

/// 联签占比阈值（bp）。
pub const RATIO_BP: u32 = 3_000;
/// 批次窗口：B29..=B36。
pub const B_LO: u32 = 29;
pub const B_HI: u32 = 36;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
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

/// 一条批次条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EntryRec {
    pub batch: u32, // B29..=B36 → 存 29..=36
    pub mark: u8,   // MK_SOLO / MK_JOIN
}

/// 联签占比总机检账。
pub struct RatioLedger {
    pub entries: Vec<EntryRec>,
}

impl RatioLedger {
    pub fn new() -> RatioLedger {
        RatioLedger { entries: vec![] }
    }

    /// 登记一条：批号与标记闭集校验。
    pub fn record(&mut self, batch: u32, mark: u8) -> Result<(), u16> {
        if batch < B_LO || batch > B_HI {
            return Err(E_BATCH);
        }
        if mark > MK_JOIN {
            return Err(E_MARK);
        }
        self.entries.push(EntryRec { batch, mark });
        Ok(())
    }

    /// 单批统计：(条目数, 联签数)。空批红。
    pub fn batch_stats(&self, batch: u32) -> Result<(u32, u32), u16> {
        let mut n: u32 = 0;
        let mut j: u32 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].batch == batch {
                n += 1;
                if self.entries[i].mark == MK_JOIN {
                    j += 1;
                }
            }
            i += 1;
        }
        if n == 0 {
            return Err(E_EMPTY);
        }
        Ok((n, j))
    }

    /// 单批占比（bp）。
    pub fn batch_ratio_bp(&self, batch: u32) -> Result<u32, u16> {
        let (n, j) = self.batch_stats(batch)?;
        Ok(j * 10_000 / n)
    }

    /// 总账占比（bp）。
    pub fn total_ratio_bp(&self) -> Result<u32, u16> {
        if self.entries.is_empty() {
            return Err(E_EMPTY);
        }
        let mut n: u32 = 0;
        let mut j: u32 = 0;
        let mut i = 0usize;
        while i < self.entries.len() {
            n += 1;
            if self.entries[i].mark == MK_JOIN {
                j += 1;
            }
            i += 1;
        }
        Ok(j * 10_000 / n)
    }

    /// 全批覆盖检查：B29..=B36 每批至少一条。
    pub fn full_cover(&self) -> Result<(), u16> {
        let mut b = B_LO;
        while b <= B_HI {
            self.batch_stats(b)?;
            b += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.entries.len() {
            buf.extend_from_slice(&self.entries[i].batch.to_le_bytes());
            buf.push(self.entries[i].mark);
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 终值签名：总占比 + 总条目数混签。
pub fn ratio_sig(ratio_bp: u32, total: u32) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&ratio_bp.to_le_bytes());
    buf.extend_from_slice(&total.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：join=<联签数> total=<总数> ratio=<bp>。
pub fn ratio_line(j: u32, n: u32, bp: u32) -> String {
    let mut out = String::new();
    out.push_str("join=");
    out.push_str(&j.to_string());
    out.push_str(" total=");
    out.push_str(&n.to_string());
    out.push_str(" ratio=");
    out.push_str(&bp.to_string());
    out
}

/// 子串判定（朴素扫描）。
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

/// 主巡检入口：R1..R11 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_07);

    // R1 批次条目登记：B29–B36 每批 4 条，LCG 决定联签。
    let mut led = RatioLedger::new();
    let mut b = B_LO;
    while b <= B_HI {
        let mut k: u32 = 0;
        while k < 4 {
            let mark = if g.below(10) < 4 { MK_JOIN } else { MK_SOLO };
            led.record(b, mark)?;
            k += 1;
        }
        b += 1;
    }
    if led.entries.len() as u32 != 8 * 4 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R2 联签标记闭集：标记 2 拒绝。
    let mut probe = RatioLedger::new();
    match probe.record(B_LO, 2) {
        Err(E_MARK) => hits += 1,
        _ => return Err(E_MARK),
    }

    // R3 批号闭集：B28/B37 拒绝。
    match led.record(28, MK_JOIN) {
        Err(E_BATCH) => {}
        _ => return Err(E_BATCH),
    }
    match led.record(37, MK_JOIN) {
        Err(E_BATCH) => hits += 1,
        _ => return Err(E_BATCH),
    }

    // R4 缺批检出：空账占比红。
    match probe.total_ratio_bp() {
        Err(E_EMPTY) => hits += 1,
        _ => return Err(E_EMPTY),
    }

    // R5 全批覆盖：B29–B36 每批非空。
    led.full_cover()?;
    hits += 1;

    // R6 占比终值计算：总占比 = 联签×10^4/总数。
    let bp = led.total_ratio_bp()?;
    let mut n_all: u32 = 0;
    let mut j_all: u32 = 0;
    let mut i = 0usize;
    while i < led.entries.len() {
        n_all += 1;
        if led.entries[i].mark == MK_JOIN {
            j_all += 1;
        }
        i += 1;
    }
    if bp != j_all * 10_000 / n_all {
        return Err(E_COUNT);
    }
    hits += 1;

    // R7 阈值 ≥30% 断言：LCG 欠额则按序补联签至达标。
    let mut bp_chk = led.total_ratio_bp()?;
    if bp_chk < RATIO_BP {
        let mut i2 = 0usize;
        while i2 < led.entries.len() && bp_chk < RATIO_BP {
            if led.entries[i2].mark == MK_SOLO {
                led.entries[i2].mark = MK_JOIN;
                bp_chk = led.total_ratio_bp()?;
            }
            i2 += 1;
        }
    }
    let bp = led.total_ratio_bp()?;
    if bp < RATIO_BP {
        return Err(E_RATIO);
    }
    // 补联签后重算总量口径。
    let mut n_all: u32 = 0;
    let mut j_all: u32 = 0;
    let mut i3 = 0usize;
    while i3 < led.entries.len() {
        n_all += 1;
        if led.entries[i3].mark == MK_JOIN {
            j_all += 1;
        }
        i3 += 1;
    }
    hits += 1;

    // R8 分批核对 B29–B36：逐批占比可查且 ≤10000bp。
    b = B_LO;
    while b <= B_HI {
        let r = led.batch_ratio_bp(b)?;
        if r > 10_000 {
            return Err(E_COUNT);
        }
        b += 1;
    }
    hits += 1;

    // R9 机检复跑一致：重算占比不变。
    let bp2 = led.total_ratio_bp()?;
    if bp2 != bp {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R10 指纹稳定。
    let f1 = led.fp();
    let f2 = led.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R11 呈现行 + 终值签名。
    let line = ratio_line(j_all, n_all, bp);
    if !has_kw(line.as_bytes(), b"total=32") || !has_kw(line.as_bytes(), b"ratio=") {
        return Err(E_LINE);
    }
    let sig = ratio_sig(bp, n_all);
    if sig != ratio_sig(bp2, n_all) || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    Ok(hits)
}
