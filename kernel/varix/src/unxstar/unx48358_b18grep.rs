//! unx48358_b18grep — UNX-F48358 B18 批防重 grep（关键词族：a11yime/readeranchor/focusbit）（诊断段 0xCDA0）。
//!
//! 判据原文：
//!   J1 批收口防重留痕；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三族闭集：B18 批关键词三族——a11yime（无障碍输入法面）/
//!      readeranchor（读屏锚）／focusbit（焦点位）——每族命中账
//!      分立，族内按（卡号，文件名指纹）二元组记账。
//!   2. 防重口径：同一（族，卡，文件）三元组只记一笔，重复 grep
//!      命中拒记并留拒痕——收口账是"覆盖了谁"，不是"扫了几遍"，
//!      重复记账会虚增覆盖（假收口）。
//!   3. 收口留痕（J1）：三族各收满本批额度（各 10 卡文件），每笔
//!      留痕（族/卡号/文件指纹/序号），序号族内连续；总账 == 三
//!      族和。
//!   4. 可红：重复记账必拒（拒痕递增）；越族关键词拒收；收口不
//!      满 audit 必红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCDA0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_DUP: u16 = SEG_BASE + 1;      // 重复命中拒记
const E_FAM_BAD: u16 = SEG_BASE + 2;  // 关键词族越闭集
const E_CARD_BAD: u16 = SEG_BASE + 3; // 卡号越批
const E_LEDGER_BAD: u16 = SEG_BASE + 4;// 收口账不平
const E_NOT_CLOSED: u16 = SEG_BASE + 5;// 收口不满
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 7;// 写回摘要为空

/// 三族闭集。
pub const FAM_A11YIME: u8 = 0;     // a11yime
pub const FAM_READERANCHOR: u8 = 1;// readeranchor
pub const FAM_FOCUSBIT: u8 = 2;    // focusbit
pub const FAM_N: usize = 3;
pub const FAM_NAMES: [&str; FAM_N] = ["a11yime", "readeranchor", "focusbit"];

/// B18 批卡号闭集（48353..48362）。
pub const CARD_LO: u32 = 48353;
pub const CARD_HI: u32 = 48362;
pub const CARD_N: usize = 10;
/// 每族收口额度（每卡每族各一笔）。
pub const FAM_QUOTA: u32 = CARD_N as u32;

/// 收口留痕条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepMark {
    pub fam: u8,
    pub card: u32,
    pub file_fp: u64,
    pub seq: u32, // 族内序号（1 起连续）
}

/// 防重 grep 面。
pub struct B18Grep {
    marks: [Vec<GrepMark>; FAM_N],
    pub dup_total: u32,   // 拒记重复留痕
    pub reject_total: u32,// 越族/越卡拒收
}

impl B18Grep {
    pub fn new() -> B18Grep {
        B18Grep { marks: [Vec::new(), Vec::new(), Vec::new()], dup_total: 0, reject_total: 0 }
    }

    fn fam_idx(fam: u8) -> Result<usize, u16> {
        if fam as usize >= FAM_N {
            return Err(E_FAM_BAD);
        }
        Ok(fam as usize)
    }

    /// 记一笔命中（防重）：同（族，卡）已记则拒记留痕。
    pub fn hit(&mut self, fam: u8, card: u32, file_fp: u64) -> Result<u32, u16> {
        let idx = B18Grep::fam_idx(fam)?;
        if card < CARD_LO || card > CARD_HI {
            self.reject_total += 1;
            return Err(E_CARD_BAD);
        }
        let book = &mut self.marks[idx];
        let mut i = 0usize;
        while i < book.len() {
            if book[i].card == card {
                self.dup_total += 1;
                return Err(E_DUP);
            }
            i += 1;
        }
        let seq = book.len() as u32 + 1;
        book.push(GrepMark { fam, card, file_fp, seq });
        Ok(seq)
    }

    /// 族内收口数。
    pub fn fam_count(&self, fam: u8) -> Result<u32, u16> {
        let idx = B18Grep::fam_idx(fam)?;
        Ok(self.marks[idx].len() as u32)
    }

    /// 收口判定（J1）：三族各满额度（10/10/10），族内序号 1..n
    /// 连续，卡号各不相同（防重账面复核）。
    pub fn audit(&self) -> Result<(), u16> {
        let mut f = 0usize;
        while f < FAM_N {
            if self.marks[f].len() as u32 != FAM_QUOTA {
                return Err(E_NOT_CLOSED);
            }
            let mut seen = [0u32; CARD_N];
            let mut i = 0usize;
            while i < self.marks[f].len() {
                let m = self.marks[f][i];
                if m.seq != i as u32 + 1 {
                    return Err(E_LEDGER_BAD);
                }
                if m.card < CARD_LO || m.card > CARD_HI {
                    return Err(E_CARD_BAD);
                }
                // 卡号唯一性复核（防重账面）。
                let slot = (m.card - CARD_LO) as usize;
                if seen[slot] != 0 {
                    return Err(E_DUP);
                }
                seen[slot] = m.card;
                i += 1;
            }
            f += 1;
        }
        Ok(())
    }

    /// 三账加和守恒。
    pub fn total(&self) -> u32 {
        let mut s: u32 = 0;
        let mut f = 0usize;
        while f < FAM_N {
            s += self.marks[f].len() as u32;
            f += 1;
        }
        s
    }

    /// 全账指纹（复跑对照）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp: u64 = 0xCDA0_CDA1;
        let mut f = 0usize;
        while f < FAM_N {
            let mut i = 0usize;
            while i < self.marks[f].len() {
                let m = self.marks[f][i];
                fp = fp.rotate_left(7) ^ (m.card as u64);
                fp = fp.rotate_left(7) ^ m.file_fp;
                i += 1;
            }
            f += 1;
        }
        fp
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.total() == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 文件名指纹（确定性，供留痕）。
pub fn file_fp(card: u32) -> u64 {
    (0xCDA0_CDA2 ^ (card as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)).rotate_left(11)
}

/// 自例程判据：逐条落实 J1（批收口防重留痕），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 批收口留痕：三族 ×10 卡全收口（30 笔），序号族内连续，
    //    audit 过。
    let mut g = B18Grep::new();
    let mut f = 0u8;
    while f < 3 {
        let mut card = CARD_LO;
        while card <= CARD_HI {
            g.hit(f, card, file_fp(card))?;
            card += 1;
        }
        f += 1;
    }
    g.audit()?;
    if g.total() != 30 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 防重留痕：同（族，卡）重复命中拒记，拒痕递增；他族同卡
    //    不算重（族间分立）。
    let mut g = B18Grep::new();
    g.hit(FAM_A11YIME, CARD_LO, 1)?;
    if g.hit(FAM_A11YIME, CARD_LO, 1) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if g.hit(FAM_A11YIME, CARD_LO, 2) != Err(E_DUP) {
        return Err(E_DUP); // 同卡不同指纹也是重
    }
    g.hit(FAM_READERANCHOR, CARD_LO, 1)?;
    if g.dup_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    if g.fam_count(FAM_A11YIME)? != 1 || g.fam_count(FAM_READERANCHOR)? != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 越族/越卡闭集：族 3..=255 拒；卡号 48352/48363 越批拒。
    let mut g = B18Grep::new();
    if g.hit(3, CARD_LO, 1) != Err(E_FAM_BAD) {
        return Err(E_FAM_BAD);
    }
    if g.hit(FAM_FOCUSBIT, CARD_LO - 1, 1) != Err(E_CARD_BAD) {
        return Err(E_CARD_BAD);
    }
    if g.hit(FAM_FOCUSBIT, CARD_HI + 1, 1) != Err(E_CARD_BAD) {
        return Err(E_CARD_BAD);
    }
    if g.reject_total != 3 {
        return Err(E_LEDGER_BAD);
    }
    if g.total() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 收口不满必红：缺一族一笔 audit 必红（E_NOT_CLOSED）。
    let mut g = B18Grep::new();
    let mut f = 0u8;
    while f < 3 {
        let mut card = CARD_LO;
        while card <= CARD_HI {
            g.hit(f, card, file_fp(card)).is_ok();
            card += 1;
        }
        f += 1;
    }
    g.marks[FAM_FOCUSBIT as usize].pop(); // 拆一笔（模拟漏扫）
    if g.audit() != Err(E_NOT_CLOSED) {
        return Err(E_NOT_CLOSED);
    }
    hits += 1;

    // R5 防重账面可红：伪造同族双卡同号（绕过 hit 防重直接塞账）
    //    audit 必红——留痕账自校验不信任写入方。
    let mut g = B18Grep::new();
    let mut card = CARD_LO;
    while card <= CARD_HI {
        g.hit(FAM_A11YIME, card, file_fp(card)).is_ok();
        card += 1;
    }
    g.marks[FAM_A11YIME as usize][3].card = g.marks[FAM_A11YIME as usize][2].card;
    if g.audit() != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // R6 序号连续可红：账内序号断号 audit 必红。
    let mut g = B18Grep::new();
    let mut card = CARD_LO;
    while card <= CARD_HI {
        g.hit(FAM_A11YIME, card, file_fp(card)).is_ok();
        card += 1;
    }
    g.marks[FAM_A11YIME as usize][5].seq = 99;
    if g.audit() != Err(E_LEDGER_BAD) {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 留痕指纹可复算：file_fp 同卡同值、异卡异值；全账指纹对
    //    单笔变动敏感。
    let a = file_fp(48353);
    let b = file_fp(48353);
    let c = file_fp(48354);
    if a != b || a == c {
        return Err(E_LEDGER_BAD);
    }
    let mut g = B18Grep::new();
    g.hit(FAM_A11YIME, CARD_LO, a)?;
    let fp1 = g.fingerprint();
    g.hit(FAM_A11YIME, CARD_LO + 1, c)?;
    if g.fingerprint() == fp1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 复跑一致与写回（判据 J2）：全收口两轮复跑指纹逐位一致；
    //    写回过。
    let build = || -> Result<B18Grep, u16> {
        let mut g = B18Grep::new();
        let mut f = 0u8;
        while f < 3 {
            let mut card = CARD_LO;
            while card <= CARD_HI {
                g.hit(f, card, file_fp(card))?;
                card += 1;
            }
            f += 1;
        }
        Ok(g)
    };
    let g1 = build()?;
    let g2 = build()?;
    if g1.fingerprint() != g2.fingerprint() {
        return Err(E_REPLAY_DIFF);
    }
    g1.audit()?;
    if g1.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    g1.writeback(true, 0x18)?;
    hits += 1;

    Ok(hits)
}
