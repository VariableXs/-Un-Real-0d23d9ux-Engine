//! unx48291_mgrep — UNX-F48291 M 段防重总 grep（七批关键词族总扫零撞）（诊断段 0xC540）。
//!
//! 判据原文：
//!   J1 总扫留痕零撞号；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 七族关键词闭集：B09..B15 七批各持一族关键词（每族 3 词），
//!      总扫 = 七族 × 5 模块全组合（105 对），一对一条账。
//!   2. 防重指纹：每对 (file, keyword) FNV-1a 64 位指纹入账；同指纹
//!      二次入账即判重（E_DUP_HIT）——总扫账不可重复贴。
//!   3. 零撞号口径（J1）：首批 105 对两两指纹互异（零撞号）；重扫
//!      留痕不删账——第二轮全量重扫必 105 对全判重、账本纹丝不动。
//!   4. 族覆盖对账：逐族账条数恰 15（3 词 × 5 模块），缺一对即覆盖
//!      缺口（E_COVER_MISS），总账 105 闭合。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC540;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_DUP_HIT: u16 = SEG_BASE + 1; // 同指纹重复入账
const E_COVER_MISS: u16 = SEG_BASE + 2; // 族覆盖缺口
const E_FP_COLLIDE: u16 = SEG_BASE + 3; // 指纹撞号
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 总扫账不守恒
const E_STATE_BAD: u16 = SEG_BASE + 5; // 总扫器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 七族关键词（B09..B15 各 3 词）。
pub const KW_B09: [&str; 3] = ["b09_switch", "b09_drop", "b09_ledger"];
pub const KW_B10: [&str; 3] = ["b10_wine", "b10_msgpump", "b10_seal"];
pub const KW_B11: [&str; 3] = ["b11_xim", "b11_threewin", "b11_clipdecl"];
pub const KW_B12: [&str; 3] = ["b12_statem", "b12_shift", "b12_matrix"];
pub const KW_B13: [(&str); 3] = ["b13_fcitx5", "b13_engplug", "b13_fdict"];
pub const KW_B14: [&str; 3] = ["b14_migrate", "b14_verpin", "b14_sla"];
pub const KW_B15: [&str; 3] = ["b15_mgate", "b15_merrtab", "b15_defexit"];

/// 扫描模块（5 个）。
pub const FILES: [&str; 5] = [
    "unx48283_latdiag",
    "unx48285_soak24",
    "unx48287_mregate",
    "unx48289_mexplic",
    "unx48291_mgrep",
];

/// 口径：7 族 × 3 词 × 5 模块 = 105 对。
pub const PAIRS_PER_FAM: u32 = 15;
pub const PAIRS_TOTAL: u32 = 105;

/// FNV-1a 64 位指纹（file 与 kw 以 0xFF 缝合）。
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

/// 总扫账条目。
#[derive(Clone, Copy, Debug)]
pub struct GrepRow {
    pub fp: u64,
    pub fam: u8,
    pub kw_idx: u8,
    pub file_idx: u8,
    pub pass_no: u32,
}

/// 防重总扫账本。
pub struct MGrepLedger {
    rows: Vec<GrepRow>,
    pub dup_total: u32,
    pub pass_total: u32,
}

impl MGrepLedger {
    pub fn new() -> MGrepLedger {
        MGrepLedger { rows: Vec::new(), dup_total: 0, pass_total: 0 }
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 单对入账：指纹撞账即判重（拒入、计数），否则落账留痕。
    pub fn record(&mut self, fp: u64, fam: u8, kw_idx: u8, file_idx: u8, pass_no: u32) -> Result<(), u16> {
        if pass_no == 0 || pass_no > self.pass_total + 1 {
            return Err(E_STATE_BAD);
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

    /// 一轮全量总扫：七族 × 3 词 × 5 模块，返回本轮判重数。
    pub fn run_pass(&mut self, pass_no: u32) -> Result<u32, u16> {
        self.pass_total += 1;
        if pass_no != self.pass_total {
            return Err(E_STATE_BAD);
        }
        let fams: [[&str; 3]; 7] = [
            KW_B09, KW_B10, KW_B11, KW_B12, KW_B13, KW_B14, KW_B15,
        ];
        let mut dups = 0u32;
        let mut f = 0usize;
        while f < 7 {
            let mut k = 0usize;
            while k < 3 {
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

    /// 族覆盖对账：逐族恰 15 条、总账 105、账条四元组与指纹重算自洽。
    pub fn audit_coverage(&self) -> Result<(), u16> {
        if self.row_count() as u32 != PAIRS_TOTAL {
            return Err(E_COVER_MISS);
        }
        let fams: [[&str; 3]; 7] = [
            KW_B09, KW_B10, KW_B11, KW_B12, KW_B13, KW_B14, KW_B15,
        ];
        let mut per_fam = [0u32; 7];
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            if r.fam > 6 || r.kw_idx > 2 || r.file_idx as usize >= FILES.len() {
                return Err(E_LEDGER_BAD);
            }
            if r.fp != fp_of(FILES[r.file_idx as usize], fams[r.fam as usize][r.kw_idx as usize]) {
                return Err(E_LEDGER_BAD); // 留痕自洽
            }
            per_fam[r.fam as usize] += 1;
            i += 1;
        }
        let mut f = 0usize;
        while f < 7 {
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

/// 自例程判据：逐条落实 J1（总扫留痕零撞号），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 首轮总扫：105 对全部首次入账、零判重、逐族覆盖恰 15。
    let mut gl = MGrepLedger::new();
    let dups = gl.run_pass(1)?;
    if dups != 0 || gl.row_count() as u32 != PAIRS_TOTAL {
        return Err(E_COVER_MISS);
    }
    gl.audit_coverage()?;
    hits += 1;

    // R2 零撞号终判（J1 主体）：105 对两两指纹互异。
    gl.audit_no_collide()?;
    hits += 1;

    // R3 防重留痕：重扫一轮 → 105 对全判重、账本纹丝不动、
    //    判重账闭合（dup_total = 105）。
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
    let mut gl2 = MGrepLedger::new();
    let mut m = 0usize;
    while m < FILES.len() {
        let mut k = 0usize;
        while k < 3 {
            gl2.record(fp_of(FILES[m], KW_B09[k]), 0, k as u8, m as u8, 1)?;
            k += 1;
        }
        m += 1;
    }
    if gl2.audit_coverage().is_ok() {
        return Err(E_COVER_MISS); // 只有一族，缺六族
    }
    if gl2.row_count() as u32 != PAIRS_PER_FAM {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 指纹异面抽核：不同对指纹互异抽核 6 对；同对双算恒同（确定性）。
    let f1 = fp_of(FILES[0], KW_B10[1]);
    let f2 = fp_of(FILES[0], KW_B12[1]);
    let f3 = fp_of(FILES[3], KW_B15[2]);
    if f1 == f2 || f1 == f3 || f2 == f3 {
        return Err(E_FP_COLLIDE);
    }
    if fp_of(FILES[3], KW_B15[2]) != f3 {
        return Err(E_STATE_BAD); // 同对重算漂移
    }
    hits += 1;

    // R6 判重注入哨兵：首批内任取一对二次入账 → E_DUP_HIT、账本不动。
    let fp0 = fp_of(FILES[1], KW_B11[2]);
    let before = gl.row_count();
    if gl.record(fp0, 2, 2, 1, 2).is_ok() {
        return Err(E_DUP_HIT);
    }
    if gl.dup_total != PAIRS_TOTAL + 1 || gl.row_count() != before {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 轮次账守门：pass_no 越轮 → E_STATE_BAD；账本闭合写回放行。
    if gl.record(fp_of(FILES[2], KW_B13[0]), 4, 0, 2, 9).is_ok() {
        return Err(E_STATE_BAD);
    }
    if gl.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    gl.writeback(true, 0x69)?;
    hits += 1;

    Ok(hits)
}
