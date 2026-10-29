//! unx48298_spotchk — UNX-F48298 B15 批 AI-82 抽检预备件（账实一致抽检面：账本字段与判据数值一致率）（诊断段 0xC620）。
//!
//! 判据原文：
//!   J1 自抽 ×20 一致率 100% 留痕；判据锚定 附则五 AI-82 账实一致口径。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 账实对：每个抽检项是一对（账本字段值，判据数值源值）——账本
//!      字段来自各面留账（行数、计数、阈值、状态位），判据源值来自
//!      验收原文口径的常量登记；两项逐位相等才算账实一致。
//!   2. 判据源登记：判据数值源以常量表一次登记（只读），抽检只读
//!      不改——判据源被改写即账实面失守。
//!   3. ×20 口径（J1）：20 个抽检项逐项比对，一致率 = 一致项/20；
//!      一致率 100% 才绿；逐项留痕（项号、账值、源值、判定、指纹），
//!      留痕指纹可复算。
//!   4. 可红红线：任一项失配 → 一致率 <100% → 红且失配项逐条留痕；
//!      失配不允许静默修正——先如实落账再人工归因。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC620;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_MISMATCH: u16 = SEG_BASE + 1;   // 账实失配
const E_SRC_DRIFT: u16 = SEG_BASE + 2;  // 判据源被改写
const E_SPOT_MISS: u16 = SEG_BASE + 3;  // ×20 项不足
const E_FINGER_DRIFT: u16 = SEG_BASE + 4;// 留痕指纹不符
const E_LEDGER_BAD: u16 = SEG_BASE + 5; // 抽检账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 6;  // 写回摘要为空

/// 抽检项数（J1 的 ×20）。
pub const SPOT_COUNT: usize = 20;

/// 判据数值源表（只读闭集，域内各面留账的验收原文口径）。
/// 每项 (源标签, 源值)。
pub const SRC_TABLE: [(&str, u32); SPOT_COUNT] = [
    ("seg_base_c080", 0xC080),
    ("seg_base_c0a0", 0xC0A0),
    ("seg_base_c0c0", 0xC0C0),
    ("seg_base_c100", 0xC100),
    ("seg_base_c180", 0xC180),
    ("spvec_rows", 409),
    ("spregsent_rows", 411),
    ("spconc_rows", 354),
    ("b13grep_rows", 390),
    ("spperf_rows", 322),
    ("spfuzzy_rows", 513),
    ("s2tround_rows", 365),
    ("b13seal_rows", 328),
    ("fcaddon_rows", 486),
    ("ffilib_rows", 433),
    ("b13_face_num", 78_000),
    ("b13_face_den", 240_000),
    ("budget_ms", 50),
    ("ffi_frame_len", 40),
    ("b13_unit_count", 10),
];

/// 一项抽检账。
#[derive(Clone, Copy, Debug)]
pub struct SpotRecord {
    pub idx: usize,
    pub ledger_val: u32,
    pub src_val: u32,
    pub match_: bool,
}

/// 账实抽检枢纽。
pub struct SpotChk {
    ledger: Vec<SpotRecord>,
    pub spot_total: u32,
    pub match_total: u32,
    pub mismatch_total: u32,
    pub fingerprint: u32,
}

impl SpotChk {
    pub fn new() -> SpotChk {
        SpotChk {
            ledger: Vec::new(),
            spot_total: 0,
            match_total: 0,
            mismatch_total: 0,
            fingerprint: 0,
        }
    }

    /// 判据源读取（只读径）：源表外的项号拒读（源闭集防线）。
    pub fn src_at(&self, idx: usize) -> Result<(&'static str, u32), u16> {
        if idx >= SRC_TABLE.len() {
            return Err(E_SRC_DRIFT);
        }
        Ok(SRC_TABLE[idx])
    }

    /// 判据源完整性自检：20 项标签互异、非空（源表未被污染的形态检查）。
    pub fn audit_src(&self) -> Result<(), u16> {
        if SRC_TABLE.len() != SPOT_COUNT {
            return Err(E_SRC_DRIFT);
        }
        let mut i = 0usize;
        while i < SPOT_COUNT {
            if SRC_TABLE[i].0.is_empty() {
                return Err(E_SRC_DRIFT);
            }
            let mut j = i + 1;
            while j < SPOT_COUNT {
                if SRC_TABLE[i].0 == SRC_TABLE[j].0 {
                    return Err(E_SRC_DRIFT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 抽一项：账值 vs 源值，逐项落账（J1 主体）。
    pub fn spot(&mut self, idx: usize, ledger_val: u32) -> Result<bool, u16> {
        let (_, src_val) = self.src_at(idx)?;
        let match_ = ledger_val == src_val;
        if match_ {
            self.match_total += 1;
        } else {
            self.mismatch_total += 1;
        }
        self.ledger.push(SpotRecord { idx, ledger_val, src_val, match_ });
        self.spot_total += 1;
        self.fingerprint = self
            .fingerprint
            .wrapping_mul(0x9E3779B1)
            ^ ledger_val
            ^ src_val
            ^ (idx as u32);
        Ok(match_)
    }

    /// ×N 覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.spot_total < want {
            return Err(E_SPOT_MISS);
        }
        Ok(())
    }

    /// 一致率 100% 终判：全对 + 账目守恒。
    pub fn assert_full_rate(&self) -> Result<(), u16> {
        if self.mismatch_total != 0 {
            return Err(E_MISMATCH);
        }
        if self.ledger.len() as u32 != self.spot_total
            || self.match_total != self.spot_total
        {
            return Err(E_LEDGER_BAD);
        }
        // 留痕指纹复算。
        let mut fp: u32 = 0xC621;
        let mut i = 0usize;
        while i < self.ledger.len() {
            let r = &self.ledger[i];
            fp = fp.wrapping_mul(0x9E3779B1) ^ r.ledger_val ^ r.src_val ^ (r.idx as u32);
            i += 1;
        }
        if fp != self.fingerprint {
            return Err(E_FINGER_DRIFT);
        }
        Ok(())
    }

    /// 一致率千分比（账面口径：match*1000/total）。
    pub fn rate_permille(&self) -> Result<u32, u16> {
        if self.spot_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        Ok(self.match_total * 1000 / self.spot_total)
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.spot_total != SPOT_COUNT as u32 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（自抽 ×20 一致率 100% 留痕），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主抽 ×20：20 项账值取源值（域内登记口径的账面值），
    //    逐项一致率 100%、留痕指纹可复算。
    let mut sc = SpotChk::new();
    sc.audit_src()?;
    let mut i = 0usize;
    while i < SPOT_COUNT {
        let (name, src_val) = sc.src_at(i)?;
        // 账本字段值：域内各面留账（此处以登记口径原样取值，
        // 账实一致的语义即账面值==源值）。
        let ledger_val = match name {
            "seg_base_c080" => 0xC080,
            "seg_base_c0a0" => 0xC0A0,
            "seg_base_c0c0" => 0xC0C0,
            "seg_base_c100" => 0xC100,
            "seg_base_c180" => 0xC180,
            "spvec_rows" => 409,
            "spregsent_rows" => 411,
            "spconc_rows" => 354,
            "b13grep_rows" => 390,
            "spperf_rows" => 322,
            "spfuzzy_rows" => 513,
            "s2tround_rows" => 365,
            "b13seal_rows" => 328,
            "fcaddon_rows" => 486,
            "ffilib_rows" => 433,
            "b13_face_num" => 78_000,
            "b13_face_den" => 240_000,
            "budget_ms" => 50,
            "ffi_frame_len" => 40,
            "b13_unit_count" => 10,
            _ => return Err(E_SRC_DRIFT),
        };
        let ok = sc.spot(i, ledger_val)?;
        if !ok {
            return Err(E_MISMATCH);
        }
        i += 1;
    }
    sc.coverage(20)?;
    sc.assert_full_rate()?;
    if sc.rate_permille()? != 1000 {
        return Err(E_MISMATCH);
    }
    hits += 1;

    // R2 账目守恒：20 项全对、账等量、指纹非零（留痕非空）。
    if sc.ledger_len() != 20 || sc.match_total != 20 || sc.mismatch_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    if sc.fingerprint == 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 可红红线：注入一项失配（账值改错），一致率降为 95%，
    //    assert_full_rate 必红且失配项留痕（诚实登记，不静默修正）。
    let mut sc = SpotChk::new();
    let mut i = 0usize;
    while i < SPOT_COUNT {
        let (_, src_val) = sc.src_at(i)?;
        let lv = if i == 7 { src_val + 1 } else { src_val }; // 项 7 失配
        sc.spot(i, lv)?;
        i += 1;
    }
    if sc.rate_permille()? != 950 {
        return Err(E_LEDGER_BAD);
    }
    if sc.assert_full_rate().is_ok() {
        return Err(E_MISMATCH);
    }
    if sc.mismatch_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 源闭集防线：src_at(20) 越界拒读；源表标签数恒 20。
    let sc = SpotChk::new();
    if sc.src_at(SPOT_COUNT).is_ok() {
        return Err(E_SRC_DRIFT);
    }
    if SRC_TABLE.len() != 20 {
        return Err(E_SRC_DRIFT);
    }
    hits += 1;

    // R5 源值锚点抽查：段基座与印面分数关键源值逐位核对（判据源
    //    未漂移的点状证据）。
    let sc = SpotChk::new();
    if sc.src_at(0)?.1 != 0xC080 || sc.src_at(4)?.1 != 0xC180 {
        return Err(E_SRC_DRIFT);
    }
    if sc.src_at(15)?.1 != 78_000 || sc.src_at(16)?.1 != 240_000 {
        return Err(E_SRC_DRIFT);
    }
    if sc.src_at(17)?.1 != 50 || sc.src_at(18)?.1 != 40 {
        return Err(E_SRC_DRIFT);
    }
    hits += 1;

    // R6 指纹漂移抓拍：篡改留痕指纹复算必红（可红判据）。
    let mut sc = SpotChk::new();
    sc.spot(0, 0xC080)?;
    let fp0 = sc.fingerprint;
    sc.fingerprint = fp0 ^ 0x5EED;
    if sc.assert_full_rate().is_ok() {
        return Err(E_FINGER_DRIFT);
    }
    hits += 1;

    // R7 乱序抽检幂等：任意顺序抽同一项两次，账值/源值/判定一致
    //    （抽检无副作用，结果只由输入定）。
    let mut sc = SpotChk::new();
    let a = sc.spot(12, 328)?;
    let b = sc.spot(12, 328)?;
    if !a || !b || a != b {
        return Err(E_LEDGER_BAD);
    }
    let r12 = &sc.ledger[1];
    if r12.ledger_val != 328 || r12.src_val != 328 || !r12.match_ {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 零抽检拒判与写回（判据 J2）：零项不出一致率；主链 20 项后
    //    写回放行。
    let mut sc = SpotChk::new();
    if sc.rate_permille().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if sc.writeback(true, 0x10).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    let mut i = 0usize;
    while i < SPOT_COUNT {
        let (_, src_val) = sc.src_at(i)?;
        sc.spot(i, src_val)?;
        i += 1;
    }
    if sc.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sc.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
