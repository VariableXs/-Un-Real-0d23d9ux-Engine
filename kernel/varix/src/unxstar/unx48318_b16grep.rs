//! unx48318_b16grep — UNX-F48318 B16 批防重 grep（关键词族：imecfg/profile/configtree）（诊断段 0xC8A0）。
//!
//! 判据原文：
//!   J1 批收口防重留痕；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 关键词族三系：imecfg / profile / configtree 各一族，每族 4 词，
//!      扫 4 个批内模块 → 每族 16 对，三族合计 48 对，一对一账条。
//!   2. 防重指纹：每对 (file, keyword) FNV-1a 64 位指纹入账；同指纹
//!      二次入账即判重（E_DUP_HIT）并计 dup——批收口账不可重复贴。
//!   3. 留痕不可变：重扫不删账不改账；第二轮全量重扫必 48 对全判重、
//!      账本纹丝不动（留痕即防重）。
//!   4. 覆盖对账：逐族账条恰 16、总账 48 闭合；账条指纹与 (file, kw)
//!      四元组重算自洽。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC8A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_DUP_HIT: u16 = SEG_BASE + 1; // 同指纹重复入账
const E_COVER_MISS: u16 = SEG_BASE + 2; // 族覆盖缺口
const E_FP_COLLIDE: u16 = SEG_BASE + 3; // 指纹撞号
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 账本守恒破
const E_PASS_BAD: u16 = SEG_BASE + 5; // 扫描轮次号非法
const E_STATE_BAD: u16 = SEG_BASE + 6; // 收口器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 三族关键词（各 4 词）。
pub const FAM_IMECFG: [&str; 4] = ["imecfg_load", "imecfg_save", "imecfg_overlay", "imecfg_baseline"];
pub const FAM_PROFILE: [&str; 4] = ["profile_switch", "profile_list", "profile_default", "profile_freeze"];
pub const FAM_CONFIGTREE: [&str; 4] =
    ["configtree_node", "configtree_path", "configtree_merge", "configtree_diff"];

/// B16 批内模块（4 个）。
pub const FILES: [&str; 4] = [
    "unx48313_cfgmigs",
    "unx48314_cfgpriv",
    "unx48316_cfgconc",
    "unx48318_b16grep",
];

/// 口径：3 族 × 4 词 × 4 模块 = 48 对。
pub const PAIRS_PER_FAM: u32 = 16;
pub const PAIRS_TOTAL: u32 = 48;

/// FNV-1a 64 位指纹。
pub fn fp_of(file: &str, kw: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let fb = file.as_bytes();
    let mut i = 0usize;
    while i < fb.len() {
        h ^= fb[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h ^= 0xFF;
    h = h.wrapping_mul(0x0000_0100_0000_01b3);
    let kb = kw.as_bytes();
    i = 0;
    while i < kb.len() {
        h ^= kb[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 账条一条。
#[derive(Clone, Copy, Debug)]
pub struct GrepRow {
    pub fp: u64,
    pub fam: u8,
    pub kw_idx: u8,
    pub file_idx: u8,
    pub pass_no: u32,
}

/// B16 防重 grep 账本。
pub struct B16GrepLedger {
    rows: Vec<GrepRow>,
    pub dup_total: u32,
    pub pass_total: u32,
}

impl B16GrepLedger {
    pub fn new() -> B16GrepLedger {
        B16GrepLedger { rows: Vec::new(), dup_total: 0, pass_total: 0 }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 单对入账：指纹撞账即判重（拒入、计数），否则落账留痕。
    pub fn record(&mut self, fp: u64, fam: u8, kw_idx: u8, file_idx: u8, pass_no: u32) -> Result<(), u16> {
        if pass_no == 0 || pass_no > self.pass_total + 1 {
            return Err(E_PASS_BAD);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].fp == fp {
                self.dup_total += 1;
                return Err(E_DUP_HIT);
            }
            i += 1;
        }
        self.rows.push(GrepRow { fp, fam, kw_idx, file_idx, pass_no });
        Ok(())
    }

    /// 一轮全量 grep：三族 × 4 词 × 4 模块，返回本轮判重数。
    pub fn run_pass(&mut self, pass_no: u32) -> Result<u32, u16> {
        self.pass_total += 1;
        if pass_no != self.pass_total {
            return Err(E_PASS_BAD);
        }
        let fams: [[&str; 4]; 3] = [FAM_IMECFG, FAM_PROFILE, FAM_CONFIGTREE];
        let mut dups = 0u32;
        let mut f = 0usize;
        while f < 3 {
            let mut k = 0usize;
            while k < 4 {
                let mut m = 0usize;
                while m < FILES.len() {
                    let fp = fp_of(FILES[m], fams[f][k]);
                    match self.record(fp, f as u8, k as u8, m as u8, pass_no) {
                        Ok(()) => {}
                        Err(E_DUP_HIT) => dups += 1,
                        Err(e) => return Err(e),
                    }
                    m += 1;
                }
                k += 1;
            }
            f += 1;
        }
        Ok(dups)
    }

    /// 覆盖对账：逐族恰 16、总账 48、账条指纹与四元组重算自洽。
    pub fn audit_coverage(&self) -> Result<(), u16> {
        if self.row_count() as u32 != PAIRS_TOTAL {
            return Err(E_COVER_MISS);
        }
        let fams: [[&str; 4]; 3] = [FAM_IMECFG, FAM_PROFILE, FAM_CONFIGTREE];
        let mut per_fam = [0u32; 3];
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            if r.fam > 2 || r.kw_idx > 3 || r.file_idx as usize >= FILES.len() {
                return Err(E_LEDGER_BAD);
            }
            if r.fp != fp_of(FILES[r.file_idx as usize], fams[r.fam as usize][r.kw_idx as usize]) {
                return Err(E_LEDGER_BAD);
            }
            per_fam[r.fam as usize] += 1;
            i += 1;
        }
        let mut f = 0usize;
        while f < 3 {
            if per_fam[f] != PAIRS_PER_FAM {
                return Err(E_COVER_MISS);
            }
            f += 1;
        }
        Ok(())
    }

    /// 零撞号审计：账内两两指纹互异。
    pub fn audit_no_collide(&self) -> Result<(), u16> {
        let mut a = 0usize;
        while a < self.rows.len() {
            let mut b = a + 1;
            while b < self.rows.len() {
                if self.rows[a].fp == self.rows[b].fp {
                    return Err(E_FP_COLLIDE);
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：总账闭合 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.row_count() as u32 != PAIRS_TOTAL {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（批收口防重留痕），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 首轮全量：48 对全首次入账、零判重、逐族覆盖恰 16。
    let mut gl = B16GrepLedger::new();
    let dups = gl.run_pass(1)?;
    if dups != 0 || gl.row_count() as u32 != PAIRS_TOTAL {
        return Err(E_COVER_MISS);
    }
    gl.audit_coverage()?;
    hits += 1;

    // R2 零撞号：48 对两两指纹互异。
    gl.audit_no_collide()?;
    hits += 1;

    // R3 防重留痕（J1 主体）：重扫一轮 → 48 对全判重、账本纹丝不动、
    //    dup_total 闭合（=48）。
    let dups2 = gl.run_pass(2)?;
    if dups2 != PAIRS_TOTAL {
        return Err(E_DUP_HIT);
    }
    if gl.row_count() as u32 != PAIRS_TOTAL || gl.dup_total != PAIRS_TOTAL {
        return Err(E_LEDGER_BAD);
    }
    gl.audit_coverage()?;
    hits += 1;

    // R4 覆盖缺口负例：只扫一族 → audit 必报 E_COVER_MISS。
    let mut gl2 = B16GrepLedger::new();
    let mut m = 0usize;
    while m < FILES.len() {
        let mut k = 0usize;
        while k < 4 {
            gl2.record(fp_of(FILES[m], FAM_IMECFG[k]), 0, k as u8, m as u8, 1)?;
            k += 1;
        }
        m += 1;
    }
    if gl2.audit_coverage().is_ok() {
        return Err(E_COVER_MISS);
    }
    if gl2.row_count() as u32 != PAIRS_PER_FAM {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 判重注入哨兵：首批内任取一对二次入账 → E_DUP_HIT、账本不动。
    let fp0 = fp_of(FILES[2], FAM_PROFILE[1]);
    let before = gl.row_count();
    if gl.record(fp0, 1, 1, 2, 2).is_ok() {
        return Err(E_DUP_HIT);
    }
    if gl.dup_total != PAIRS_TOTAL + 1 || gl.row_count() != before {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 轮次守门：pass_no 越轮 → E_PASS_BAD。
    if gl.record(fp_of(FILES[3], FAM_CONFIGTREE[0]), 2, 0, 3, 9).is_ok() {
        return Err(E_PASS_BAD);
    }
    hits += 1;

    // R7 写回（判据 J2）：零账条不写回、空摘要拒、闭合+非空摘要放行。
    let mut gl3 = B16GrepLedger::new();
    if gl3.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    gl3.run_pass(1)?;
    if gl3.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    gl3.writeback(true, 0x30)?;
    hits += 1;

    Ok(hits)
}
