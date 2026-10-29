//! unx48217_b11grep — UNX-F48217 B11 批防重 grep（关键词族：xim/xwayland/textinputv3）（诊断段 0xBC00）。
//!
//! 判据原文：
//!   J1 批收口防重留痕；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 关键词族三系：xim / xwayland / textinputv3 各一族，每族 4 关键词，
//!      扫描 6 个批内模块 → 每族 24 对 (file, keyword)，三族合计 72 对，
//!      一对一账条，批收口全覆盖。
//!   2. 防重指纹：每对 (file, keyword) 以 FNV-1a 64 位指纹入账；同指纹
//!      二次入账即判重（E_DUP_HIT）并计入 dup_total——账本不可重复贴。
//!   3. 留痕纪律：重扫不删账、不改账；第二轮全量重扫必须 72 对全部判重、
//!      账本长度纹丝不动——防重即留痕，留痕即不可变。
//!   4. 覆盖对账：族内每关键词 × 每模块的账条数逐族核对（24/族），
//!      缺一对即覆盖缺口（E_COVER_MISS），多一对即防重失守。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBC00;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_DUP_HIT: u16 = SEG_BASE + 1; // 同指纹重复入账
const E_COVER_MISS: u16 = SEG_BASE + 2; // 族覆盖缺口
const E_FP_COLLIDE: u16 = SEG_BASE + 3; // 指纹碰撞（不同对不同指纹须异）
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 账本守恒破
const E_PASS_BAD: u16 = SEG_BASE + 5; // 扫描轮次号非法
const E_STATE_BAD: u16 = SEG_BASE + 6; // 收口器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 关键词族（三系 × 4 词）。
pub const FAM_XIM: [&str; 4] = ["xim_connect", "xim_ic", "xim_focus", "xim_preedit"];
pub const FAM_XWAYLAND: [&str; 4] =
    ["xwayland_surface", "xwayland_key", "xwayland_focus", "xwayland_drop"];
pub const FAM_TEXTINPUTV3: [&str; 4] =
    ["tiv3_enable", "tiv3_commit", "tiv3_preedit", "tiv3_surrounding"];

/// B11 批内模块清单（6 个）。
pub const FILES: [&str; 6] = [
    "unx48213_xcosign",
    "unx48214_xdropkey",
    "unx48215_xregsent",
    "unx48216_xmulticli",
    "unx48218_threewin",
    "unx48219_clipdecl",
];

/// 每族账条数 = 6 模块 × 4 关键词。
pub const PAIRS_PER_FAM: u32 = 24;
/// 三族合计。
pub const PAIRS_TOTAL: u32 = 72;

/// FNV-1a 64 位指纹（分两段喂入，中缝 0xFF 分隔符）。
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

/// 账本一条：(指纹, 族号, 词号, 模块号, 轮次号) 全留痕。
#[derive(Clone, Copy, Debug)]
pub struct GrepEntry {
    pub fp: u64,
    pub fam: u8,
    pub kw_idx: u8,
    pub file_idx: u8,
    pub pass_no: u32,
}

/// 防重 grep 账本。
pub struct GrepLedger {
    entries: Vec<GrepEntry>,
    pub dup_total: u32,
    pub pass_total: u32,
}

impl GrepLedger {
    pub fn new() -> GrepLedger {
        GrepLedger { entries: Vec::new(), dup_total: 0, pass_total: 0 }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 单对入账：指纹已在册即判重（拒入账、计 dup）；否则落账。
    pub fn record(&mut self, fp: u64, fam: u8, kw_idx: u8, file_idx: u8, pass_no: u32) -> Result<(), u16> {
        if pass_no == 0 || pass_no > self.pass_total + 1 {
            return Err(E_PASS_BAD);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].fp == fp {
                self.dup_total += 1;
                return Err(E_DUP_HIT);
            }
            i += 1;
        }
        self.entries.push(GrepEntry { fp, fam, kw_idx, file_idx, pass_no });
        Ok(())
    }

    /// 一轮全量 grep：三族 × 4 词 × 6 模块逐对入账，返回本轮判重数。
    pub fn run_pass(&mut self, pass_no: u32) -> Result<u32, u16> {
        self.pass_total += 1;
        if pass_no != self.pass_total {
            return Err(E_PASS_BAD);
        }
        let mut dups = 0u32;
        let fams: [[&str; 4]; 3] = [FAM_XIM, FAM_XWAYLAND, FAM_TEXTINPUTV3];
        let mut f = 0usize;
        while f < fams.len() {
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

    /// 覆盖对账：逐族账条数必须恰 24，且族号/词号/模块号与账条自洽。
    pub fn audit_coverage(&self) -> Result<(), u16> {
        if self.entry_count() as u32 != PAIRS_TOTAL {
            return Err(E_COVER_MISS);
        }
        let mut per_fam = [0u32; 3];
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = self.entries[i];
            if e.fam > 2 || e.kw_idx > 3 || e.file_idx as usize >= FILES.len() {
                return Err(E_LEDGER_BAD);
            }
            per_fam[e.fam as usize] += 1;
            // 账条指纹必须与四元组重算一致（留痕自洽）。
            let fams: [[&str; 4]; 3] = [FAM_XIM, FAM_XWAYLAND, FAM_TEXTINPUTV3];
            if e.fp != fp_of(FILES[e.file_idx as usize], fams[e.fam as usize][e.kw_idx as usize]) {
                return Err(E_LEDGER_BAD);
            }
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

    /// 指纹两两相异审计（首批 72 对零碰撞）。
    pub fn audit_no_collide(&self) -> Result<(), u16> {
        let mut a = 0usize;
        while a < self.entries.len() {
            let mut b = a + 1;
            while b < self.entries.len() {
                if self.entries[a].fp == self.entries[b].fp
                    && (self.entries[a].fam != self.entries[b].fam
                        || self.entries[a].kw_idx != self.entries[b].kw_idx
                        || self.entries[a].file_idx != self.entries[b].file_idx)
                {
                    return Err(E_FP_COLLIDE);
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：覆盖闭合 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.entry_count() as u32 != PAIRS_TOTAL {
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

    // R1 首轮全量 grep：72 对全部首次入账、零判重、覆盖逐族恰 24。
    let mut gl = GrepLedger::new();
    let dups = gl.run_pass(1)?;
    if dups != 0 || gl.entry_count() as u32 != PAIRS_TOTAL {
        return Err(E_COVER_MISS);
    }
    gl.audit_coverage()?;
    hits += 1;

    // R2 单对判重负例：首批中任取一对（file0 × xim_connect）重扫 →
    //    E_DUP_HIT、dup_total+1、账本长度不动。
    let fp0 = fp_of(FILES[0], FAM_XIM[0]);
    let before = gl.entry_count();
    if gl.record(fp0, 0, 0, 0, 1).is_ok() {
        return Err(E_DUP_HIT);
    }
    if gl.dup_total != 1 || gl.entry_count() != before {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 防重留痕（J1 主体）：第二轮全量重扫 → 72 对全部判重、账本
    //    长度纹丝不动、轮次账闭合（pass_total=2）。
    let dups2 = gl.run_pass(2)?;
    if dups2 != PAIRS_TOTAL {
        return Err(E_DUP_HIT);
    }
    if gl.entry_count() as u32 != PAIRS_TOTAL || gl.pass_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    if gl.dup_total != 1 + PAIRS_TOTAL {
        return Err(E_LEDGER_BAD); // R2 的 1 次 + 本轮 72 次
    }
    gl.audit_coverage()?;
    hits += 1;

    // R4 覆盖缺口负例：半账本（只扫一族）对账必报 E_COVER_MISS，
    //    缺口即收口失败——不许带缺口收官。
    let mut gl2 = GrepLedger::new();
    let mut m = 0usize;
    while m < FILES.len() {
        let mut k = 0usize;
        while k < 4 {
            gl2.record(fp_of(FILES[m], FAM_XIM[k]), 0, k as u8, m as u8, 1)?;
            k += 1;
        }
        m += 1;
    }
    if gl2.audit_coverage().is_ok() {
        return Err(E_COVER_MISS); // 只有一族，必缺两族
    }
    if gl2.entry_count() as u32 != PAIRS_PER_FAM {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 指纹两两相异：首批 72 对零碰撞；不同对同指纹即 E_FP_COLLIDE。
    gl.audit_no_collide()?;
    let fa = fp_of(FILES[1], FAM_XIM[1]);
    let fb = fp_of(FILES[1], FAM_XWAYLAND[1]);
    if fa == fb {
        return Err(E_FP_COLLIDE);
    }
    hits += 1;

    // R6 留痕自洽：账条指纹与 (file, kw) 四元组重算逐条一致（审计内嵌），
    //    轮次号越界入账必拒（E_PASS_BAD）。
    gl.audit_coverage()?;
    if gl.record(fp_of(FILES[2], FAM_XIM[2]), 0, 2, 2, 0).is_ok() {
        return Err(E_PASS_BAD); // pass_no=0 非法
    }
    if gl.record(fp_of(FILES[2], FAM_XIM[2]), 0, 2, 2, 9).is_ok() {
        return Err(E_PASS_BAD); // pass_no 越过当前轮
    }
    hits += 1;

    // R7 写回（判据 J2）：覆盖未闭合不写回、空摘要拒、闭合+非空摘要放行。
    let mut gl3 = GrepLedger::new();
    if gl3.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD); // 零账条不得写回
    }
    gl3.run_pass(1)?;
    if gl3.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    gl3.writeback(true, 0x18)?;
    hits += 1;

    Ok(hits)
}
