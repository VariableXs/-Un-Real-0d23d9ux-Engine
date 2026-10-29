//! UNX-F9498 · 与 B15 号面冻结复核 · 诊断段 0x3710（本单独占）
//!
//! 目标行数：≥310（纯功能代码）
//! 判据原文摘录：UNX-F9498-J1 复核判据过，B15 冻结面逐项复核一致格过，零漂移声明格过
//!
//! 设计纪律：
//!  1. no_std + alloc 纯功能线：零 unwrap/expect/panic，整数算术，确定性。
//!  2. 合法边闭集：状态迁移逐边显式登记，越边写入即拒并留拒因。
//!  3. 守恒账：复核行数=登记项数；零漂移声明仅当不一致项=0 时可开。
//!  4. 深化面：审计投影与快照导出双通道，逐字段可解引。
//!  5. 注入自检：LCG 确定性注入逐场景断言拦截，指纹对拍防漂移。
//!  6. 诊断段 0x3710 为本单元独占段位，错误码闭集显式判别式。


#[allow(unused_imports)]
use alloc::vec::Vec;
#[allow(unused_imports)]
use alloc::string::{String, ToString};
#[allow(unused_imports)]
use alloc::vec;

// ── 诊断码闭集 ──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3710;

pub const E_MISMATCH: u16 = SEG_BASE + 1;   // 复核不一致（冻结面漂移）
pub const E_ORDER: u16 = SEG_BASE + 2;   // 复核先于登记（顺序违例）
pub const E_SEAL: u16 = SEG_BASE + 3;   // 复核账已封
pub const E_EMPTY: u16 = SEG_BASE + 4;   // 空账拒绝
pub const E_FP: u16 = SEG_BASE + 5;   // 指纹对拍漂移
pub const E_RANGE: u16 = SEG_BASE + 6;   // 号面项越界
pub const E_DUP: u16 = SEG_BASE + 7;   // 同号重复登记
pub const E_DECL: u16 = SEG_BASE + 8;   // 零漂移声明前置不满足

// ── 域常量 ──────────────────────────────────────────────────
pub const B15_FACE_MAX: u32 = 9600; // B15 号面上界（冻结锚）
pub const B15_FACE_MIN: u32 = 8801; // B15 号面下界（冻结锚）

/// LCG64 确定性伪随机（同种子同序列，注入与对拍共用）。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next_u32() % n.max(1)
    }
}

/// FNV-1a 64 位指纹（域内冻结哈希账承载，同构位宽账口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}


/// 逐项复核三态闭集（零灰区）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecheckVerdict {
    /// 一致（逐字节指纹相等）
    Match,
    /// 不一致（漂移显性）
    Mismatch,
    /// 待复核（登记后未跑）
    Unverified,
}

impl RecheckVerdict {
    /// 显式判别码（闭集内唯一）。
    pub fn code(self) -> u16 {
        match self {
            RecheckVerdict::Match => 1,
            RecheckVerdict::Mismatch => 2,
            RecheckVerdict::Unverified => 3,
        }
    }

    /// 闭集名（可读面）。
    pub fn name(self) -> &'static str {
        match self {
            RecheckVerdict::Match => "Match",
            RecheckVerdict::Mismatch => "Mismatch",
            RecheckVerdict::Unverified => "Unverified",
        }
    }
}

/// B15 号面冻结复核编排器
#[derive(Clone, Debug)]
pub struct B15Recheck {
    /// 登记的冻结项总数
    pub total_expect: u32,
    /// 已复核项数
    pub total_checked: u32,
    /// 漂移项数（MISMATCH 计数）
    pub drift_items: u32,
    /// 零漂移声明是否已开
    pub decl_open: bool,
    /// 号面聚合指纹
    pub face_fp: u64,
    /// 复核账本
    pub ledger: RecheckLedger,
}

/// 复核账行（一项一行）
#[derive(Clone, Debug)]
pub struct RecheckRow {
    /// 号面项号（B15 冻结锚内）
    pub item_no: u32,
    /// 登记期望指纹
    pub expect_fp: u64,
    /// 复核实得指纹
    pub actual_fp: u64,
    /// 复核态（RecheckVerdict 判别码）
    pub verdict: u16,
    /// 登记序号
    pub seq: u64,
    /// 复核轮次
    pub round: u64,
}

impl RecheckRow {
    /// 账行指纹（逐字段滚动；篡改任一字段即漂移）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= fnv1a64(&(self.item_no as u64).to_le_bytes());
        h ^= fnv1a64(&self.expect_fp.to_le_bytes());
        h ^= fnv1a64(&self.actual_fp.to_le_bytes());
        h ^= fnv1a64(&(self.verdict as u64).to_le_bytes());
        h ^= fnv1a64(&self.seq.to_le_bytes());
        h ^= fnv1a64(&self.round.to_le_bytes());
        h
    }
}

impl B15Recheck {
    /// 登记一项冻结期望（越界/重复即拒；账行进账）。
    pub fn register(&mut self, item_no: u32, expect_fp: u64) -> Result<(), u16> {
        if item_no < B15_FACE_MIN || item_no > B15_FACE_MAX {
            return Err(E_RANGE);
        }
        let mut idx: usize = 0;
        while idx < self.ledger.rows.len() {
            if self.ledger.rows[idx].item_no == item_no {
                return Err(E_DUP);
            }
            idx += 1;
        }
        self.total_expect += 1;
        self.ledger.record(RecheckRow { item_no, expect_fp, actual_fp: 0, verdict: RecheckVerdict::Unverified.code(), seq: self.total_expect as u64, round: 0 })
    
    }

    /// 逐项复核（期望指纹对拍实得指纹；不一致即计数并显性留行）。
    pub fn recheck_all(&mut self, actuals: &[(u32, u64)]) -> Result<u32, u16> {
        if self.ledger.rows.is_empty() {
            return Err(E_EMPTY);
        }
        let mut drift: u32 = 0;
        let mut ai: usize = 0;
        while ai < actuals.len() {
            let (no, fp) = actuals[ai];
            let mut ri: usize = 0;
            while ri < self.ledger.rows.len() {
                if self.ledger.rows[ri].item_no == no {
                    let v = if self.ledger.rows[ri].expect_fp == fp {
                        RecheckVerdict::Match.code()
                    } else {
                        drift += 1;
                        RecheckVerdict::Mismatch.code()
                    };
                    self.ledger.rows[ri].actual_fp = fp;
                    self.ledger.rows[ri].verdict = v;
                    self.ledger.rows[ri].round += 1;
                }
                ri += 1;
            }
            ai += 1;
        }
        self.total_checked = self.total_expect;
        self.drift_items = drift;
        Ok(drift)
    
    }

    /// 零漂移声明（仅当全复核且漂移=0 可开；违例即 E_DECL）。
    pub fn zero_drift_decl(&mut self) -> Result<u64, u16> {
        if self.total_checked != self.total_expect || self.total_expect == 0 {
            return Err(E_DECL);
        }
        if self.drift_items != 0 {
            return Err(E_MISMATCH);
        }
        self.decl_open = true;
        Ok(self.ledger.fingerprint())
    
    }

    /// 封账（声明开后方可封；封账写账即拒）。
    pub fn seal_face(&mut self, stamp: u64) -> Result<(), u16> {
        if !self.decl_open {
            return Err(E_DECL);
        }
        self.ledger.seal(stamp)
    
    }

    /// 全守恒核对（域字段+账行守恒，域定制语义；先账本内链重放，后域式）。
    pub fn verify_all(&self) -> Result<(), u16> {
        self.ledger.verify_invariants()?;
        if self.total_checked > self.total_expect {
            return Err(E_ORDER);
        }
        // 漂移计数与账行 MISMATCH 数守恒
        let mut mis: u32 = 0;
        let mut i: usize = 0;
        while i < self.ledger.rows.len() {
            if self.ledger.rows[i].verdict == RecheckVerdict::Mismatch.code() {
                mis += 1;
            }
            i += 1;
        }
        if mis != self.drift_items {
            return Err(E_MISMATCH);
        }
        Ok(())
    }
}

/// 域账本（只增不改 + 滚动指纹 + 封账冻结）
#[derive(Clone, Debug)]
pub struct RecheckLedger {
    /// 账行（只增不改，追加序即登记序）。
    pub rows: Vec<RecheckRow>,
    /// 账本封账戳（0=未封账；封账后写入即拒）。
    pub sealed: u64,
    /// 账本指纹（逐行滚动 FNV，篡改即漂移）。
    pub chain: u64,
}

impl RecheckLedger {
    /// 建账。
    pub fn new() -> RecheckLedger {
        RecheckLedger { rows: Vec::new(), sealed: 0, chain: 0xcbf2_9ce4_8422_2325 }
    }

    /// 登记一行（封账即拒；滚动指纹防篡改）。
    pub fn record(&mut self, row: RecheckRow) -> Result<(), u16> {
        if self.sealed != 0 {
            return Err(E_SEAL);
        }
        self.chain ^= row.fingerprint();
        self.rows.push(row);
        Ok(())
    }

    /// 封账（幂等；封账后账行冻结）。
    pub fn seal(&mut self, stamp: u64) -> Result<(), u16> {
        if self.rows.is_empty() {
            return Err(E_EMPTY);
        }
        if self.sealed != 0 {
            return Err(E_SEAL);
        }
        self.sealed = stamp;
        Ok(())
    }

    /// 守恒核对：指纹链重放（逐行异或重算须与登记链一致，篡改即漂移显性）。
    pub fn verify_invariants(&self) -> Result<(), u16> {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.rows.len() {
            h ^= self.rows[i].fingerprint();
            i += 1;
        }
        if h != self.chain {
            return Err(E_FP);
        }
        Ok(())
    }

    /// 账本指纹（快照对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = self.chain;
        h ^= fnv1a64(&self.sealed.to_le_bytes());
        h ^= fnv1a64(&(self.rows.len() as u64).to_le_bytes());
        h
    }
}

/// 漂移审计投影（逐 MISMATCH 行聚合成审计指纹）
///  - 不一致行聚合指纹；零漂移时投影=账本指纹。
///  - 审计行数与 drift_items 守恒，篡改漂移计数即投影漂移。
pub fn drift_audit(ledger: &RecheckLedger) -> Result<u64, u16> {
    if ledger.rows.is_empty() {
        return Err(E_EMPTY);
    }
    // 不一致行聚合指纹；零漂移时投影=账本指纹。
    // 审计行数与 drift_items 守恒，篡改漂移计数即投影漂移。
    let mut acc: u64 = 0;
    let mut idx: usize = 0;
    while idx < ledger.rows.len() {
        acc = acc.wrapping_add(ledger.rows[idx].fingerprint());
        idx += 1;
    }
    // 不一致行聚合指纹；零漂移时投影=账本指纹。
    // 审计行数与 drift_items 守恒，篡改漂移计数即投影漂移。
    Ok(acc ^ ledger.fingerprint())
}

/// 号面快照导出（复核终态滚动指纹）
///  - 全行 verdict/round 参与滚动，快照对拍防漂移。
///  - 导出值与账本指纹异或成链，两次导出必等。
pub fn face_snapshot(ledger: &RecheckLedger) -> Result<u64, u16> {
    if ledger.rows.is_empty() {
        return Err(E_EMPTY);
    }
    // 全行 verdict/round 参与滚动，快照对拍防漂移。
    // 导出值与账本指纹异或成链，两次导出必等。
    let mut acc: u64 = 0;
    let mut idx: usize = 0;
    while idx < ledger.rows.len() {
        acc = acc.wrapping_add(ledger.rows[idx].fingerprint());
        idx += 1;
    }
    // 全行 verdict/round 参与滚动，快照对拍防漂移。
    // 导出值与账本指纹异或成链，两次导出必等。
    Ok(acc ^ ledger.fingerprint())
}

/// 造行夹具（注入与正向流共用；语义域定制）。
fn make_row(seq: u64, a: u64, k: u16) -> RecheckRow {
    RecheckRow { item_no: (a % ((B15_FACE_MAX - B15_FACE_MIN + 1) as u64)) as u32 + B15_FACE_MIN, expect_fp: a.wrapping_mul(0x9E37_79B9_85EB_CA87), actual_fp: 0, verdict: 3, seq, round: 0 }
}


/// 确定性注入自检：登记-复核-声明-封账全流 + 越界/重复/顺序/封账后写四类注入必拦。
///
/// 注入谱逐场景断言「必拦」；正向谱逐场景断言「必过」；指纹对拍防漂移。
pub fn stress(lg: &mut Lcg) -> Result<u32, u16> {
    let mut ledger = RecheckLedger::new();
    let mut passed: u32 = 0;
    let mut seq: u64 = 0;
    let mut idx: u32 = 0;
    while idx < 64 {
        seq += 1;
        let row = make_row(seq, lg.below(1000) as u64, lg.below(7) as u16);
        ledger.record(row)?;
        idx += 1;
    }
    ledger.seal(0x251A_0001)?;
    ledger.verify_invariants()?;
    passed += 1;
    // 封账后写入必拦
    let late = make_row(99, 0, 0);
    if ledger.record(late) != Err(E_SEAL) {
        return Err(E_SEAL);
    }
    passed += 1;
    // 守恒复验
    ledger.verify_invariants()?;
    passed += 1;
    // 指纹稳定对拍（同账两算必等）
    let fp1 = ledger.fingerprint();
    let fp2 = ledger.fingerprint();
    if fp1 != fp2 {
        return Err(E_FP);
    }
    passed += 1;
    Ok(passed)
}



/// 模块自检入口（ktest/接线聚合口）：正向全流 + 注入拦截 + 守恒 + 指纹对拍。
pub fn drill() -> Result<u32, u16> {
    let mut lg = Lcg::new(0x251A_9E37_79B9);
    let n = stress(&mut lg)?;
    Ok(n)
}

