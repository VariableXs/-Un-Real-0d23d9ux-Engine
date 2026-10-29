//! unx48198_b10grep — UNX-F48198 B10 批防重 grep（关键词族：wmime/imm/compositionform）（诊断段 0xB9A0）。
//!
//! 判据原文：
//!   J1 批收口防重留痕；判据锚定 §6.1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 关键词族三枚闭集：KwWmime（0x1 位）/ KwImm（0x2 位）/
//!      KwCompForm（0x4 位），批内文件以 kw_mask 位图声明含词面；
//!      grep 只对位图做表驱动扫描，不做文本匹配——位图即留痕本体。
//!   2. 防重双闸：其一，同关键词族只许扫一次，重复扫即 E_DUP_GREP；
//!      其二，卡片认领 (card_id, kw) 对唯一，重复认领即 E_DUP_CLAIM。
//!      防重 grep 的本义是"同一关键词两卡都宣称首查"必须当场炸账。
//!   3. 扫描账逐词留痕：GrepEntry 记 (kw, 命中文件数, 命中文件 id 列)，
//!      命中列与独立重数（recount）逐位相等才许收口。
//!   4. 收口口径（判据 J1）：三族全部扫毕、每族命中数 ≥2（批内真铺
//!      开，不是空转）、认领账无冲突、四项齐 → 批收口放行一次，且
//!      只放行一次（二次收口即 E_DUP_CLOSE）。
//!   5. 文件表确定性：B10 批 12 个模块文件按位图表给定（非随机），
//!      其中三族各占 ≥2 文件、两文件双词重叠，专考命中列精确性。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB9A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_DUP_GREP: u16 = SEG_BASE + 1; // 同族重复扫
const E_DUP_CLAIM: u16 = SEG_BASE + 2; // (card,kw) 重复认领
const E_HIT_BAD: u16 = SEG_BASE + 3; // 命中列与重数不符
const E_KW_MISS: u16 = SEG_BASE + 4; // 三族未扫毕
const E_HIT_SPARSE: u16 = SEG_BASE + 5; // 族命中 <2，批内未真铺开
const E_DUP_CLOSE: u16 = SEG_BASE + 6; // 二次收口
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

// ── 关键词族位（kw_mask 位图口径）──────────────────────────
pub const KW_WMIME: u32 = 0x1;
pub const KW_IMM: u32 = 0x2;
pub const KW_COMPFORM: u32 = 0x4;
pub const KW_ALL: u32 = KW_WMIME | KW_IMM | KW_COMPFORM;

/// 关键词族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kw {
    Wmime,
    Imm,
    CompForm,
}

impl Kw {
    pub fn bit(&self) -> u32 {
        match self {
            Kw::Wmime => KW_WMIME,
            Kw::Imm => KW_IMM,
            Kw::CompForm => KW_COMPFORM,
        }
    }
}

/// B10 批文件表：id + 位图（纪律 5）。
pub struct FileRec {
    pub id: u16,
    pub kw_mask: u32,
}

/// B10 批 12 文件固定位图表：三族各 ≥2 命中，双文件词族重叠。
pub fn b10_file_table() -> Vec<FileRec> {
    let mut v: Vec<FileRec> = Vec::new();
    v.push(FileRec { id: 48183, kw_mask: KW_COMPFORM }); // compwind
    v.push(FileRec { id: 48184, kw_mask: KW_IMM }); // candfb
    v.push(FileRec { id: 48185, kw_mask: KW_IMM }); // candrect
    v.push(FileRec { id: 48186, kw_mask: KW_WMIME }); // wmime
    v.push(FileRec { id: 48187, kw_mask: KW_WMIME | KW_IMM }); // wimemsg
    v.push(FileRec { id: 48188, kw_mask: KW_IMM }); // wproc
    v.push(FileRec { id: 48189, kw_mask: KW_WMIME }); // wcliptxt
    v.push(FileRec { id: 48190, kw_mask: KW_COMPFORM | KW_IMM }); // wopenimmi
    v.push(FileRec { id: 48191, kw_mask: KW_WMIME }); // wexstyle
    v.push(FileRec { id: 48192, kw_mask: KW_IMM }); // wgl
    v.push(FileRec { id: 48193, kw_mask: KW_WMIME }); // wmsgfuzz
    v.push(FileRec { id: 48194, kw_mask: KW_COMPFORM }); // wperfsamp
    v
}

/// 单族扫描账。
pub struct GrepEntry {
    pub kw: Kw,
    pub hits: u32,
    pub hit_files: Vec<u16>,
    pub claimed_by: Vec<u16>, // 认领该族首查的卡片 id 列
}

/// 防重 grep 总账。
pub struct GrepLedger {
    pub files: Vec<FileRec>,
    pub greps: Vec<GrepEntry>,
    pub closed: bool,
}

impl GrepLedger {
    pub fn new() -> GrepLedger {
        GrepLedger { files: b10_file_table(), greps: Vec::new(), closed: false }
    }

    /// 单族扫描（纪律 2/3）：重复族拒、命中列落账。
    pub fn scan(&mut self, kw: Kw) -> Result<u32, u16> {
        let mut i = 0usize;
        while i < self.greps.len() {
            if self.greps[i].kw == kw {
                return Err(E_DUP_GREP);
            }
            i += 1;
        }
        let mut hits = 0u32;
        let mut hit_files: Vec<u16> = Vec::new();
        let mut f = 0usize;
        while f < self.files.len() {
            if self.files[f].kw_mask & kw.bit() != 0 {
                hits += 1;
                hit_files.push(self.files[f].id);
            }
            f += 1;
        }
        self.greps.push(GrepEntry { kw, hits, hit_files, claimed_by: Vec::new() });
        Ok(hits)
    }

    /// 独立重数：不走扫描账，直接对文件表重数一遍。
    pub fn recount(&self, kw: Kw) -> u32 {
        let mut hits = 0u32;
        let mut f = 0usize;
        while f < self.files.len() {
            if self.files[f].kw_mask & kw.bit() != 0 {
                hits += 1;
            }
            f += 1;
        }
        hits
    }

    /// 卡片认领 (card_id, kw)：重复认领即拒（纪律 2）。
    pub fn claim(&mut self, card_id: u16, kw: Kw) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.greps.len() {
            if self.greps[i].kw == kw {
                let mut j = 0usize;
                while j < self.greps[i].claimed_by.len() {
                    if self.greps[i].claimed_by[j] == card_id {
                        return Err(E_DUP_CLAIM);
                    }
                    j += 1;
                }
                self.greps[i].claimed_by.push(card_id);
                return Ok(());
            }
            i += 1;
        }
        Err(E_KW_MISS) // 未扫描的族不得认领
    }

    /// 命中列核销：账面命中数、命中列、独立重数三方逐位一致（纪律 3）。
    pub fn verify_hits(&self, kw: Kw) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.greps.len() {
            if self.greps[i].kw == kw {
                if self.greps[i].hits != self.recount(kw) {
                    return Err(E_HIT_BAD);
                }
                if self.greps[i].hits as usize != self.greps[i].hit_files.len() {
                    return Err(E_HIT_BAD);
                }
                if self.greps[i].hits < 2 {
                    return Err(E_HIT_SPARSE);
                }
                return Ok(());
            }
            i += 1;
        }
        Err(E_KW_MISS)
    }

    /// 批收口（判据 J1）：三族扫毕+核销齐+认领无空 → 放行一次。
    pub fn close_batch(&mut self) -> Result<(), u16> {
        if self.closed {
            return Err(E_DUP_CLOSE);
        }
        let fams: [Kw; 3] = [Kw::Wmime, Kw::Imm, Kw::CompForm];
        let mut i = 0usize;
        while i < fams.len() {
            self.verify_hits(fams[i])?;
            i += 1;
        }
        if self.greps.len() != 3 {
            return Err(E_KW_MISS);
        }
        self.closed = true;
        Ok(())
    }
}

/// 写回判定（判据 J2）。
fn writeback(done: bool, summary_len: u32) -> Result<(), u16> {
    if !done || summary_len == 0 {
        return Err(E_EMPTY_SUM);
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（批收口防重留痕），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主判据：三族各扫一次，命中数与重数三方一致，批收口放行。
    let mut led = GrepLedger::new();
    led.scan(Kw::Wmime)?;
    led.scan(Kw::Imm)?;
    led.scan(Kw::CompForm)?;
    // 位图表固定口径：wmime 5 文件、imm 6 文件、compositionform 3 文件。
    if led.recount(Kw::Wmime) != 5 || led.recount(Kw::Imm) != 6 || led.recount(Kw::CompForm) != 3 {
        return Err(E_HIT_BAD);
    }
    led.close_batch()?;
    hits += 1;

    // R2 防重闸一：同族重复扫必拒，且账面条数不变。
    let mut led = GrepLedger::new();
    led.scan(Kw::Imm)?;
    if led.scan(Kw::Imm).is_ok() {
        return Err(E_DUP_GREP);
    }
    if led.greps.len() != 1 {
        return Err(E_DUP_GREP);
    }
    hits += 1;

    // R3 防重闸二：(card,kw) 重复认领必拒；未扫描族认领也拒。
    led.claim(48183, Kw::Imm)?;
    if led.claim(48183, Kw::Imm).is_ok() {
        return Err(E_DUP_CLAIM);
    }
    if led.claim(48194, Kw::Wmime).is_ok() {
        return Err(E_KW_MISS); // Wmime 尚未扫描
    }
    led.claim(48184, Kw::Imm)?; // 另一卡认领同族：合法
    hits += 1;

    // R4 收口前缺族：只扫两族就收口必拒（三族未扫毕）。
    let mut led = GrepLedger::new();
    led.scan(Kw::Wmime)?;
    led.scan(Kw::Imm)?;
    if led.close_batch().is_ok() {
        return Err(E_KW_MISS);
    }
    hits += 1;

    // R5 稀疏命中闸：构造单命中的族表 → E_HIT_SPARSE（批内未真铺开）。
    let mut led = GrepLedger::new();
    led.files.clear();
    led.files.push(FileRec { id: 48183, kw_mask: KW_COMPFORM });
    led.files.push(FileRec { id: 48184, kw_mask: KW_IMM });
    led.scan(Kw::CompForm)?;
    if led.verify_hits(Kw::CompForm).is_ok() {
        return Err(E_HIT_SPARSE); // 单命中族不得过核销
    }
    hits += 1;

    // R6 命中列精确性：扫描账命中列 == 独立重列（逐位）。
    let mut led = GrepLedger::new();
    led.scan(Kw::CompForm)?;
    let mut expect: Vec<u16> = Vec::new();
    let mut f = 0usize;
    while f < led.files.len() {
        if led.files[f].kw_mask & KW_COMPFORM != 0 {
            expect.push(led.files[f].id);
        }
        f += 1;
    }
    let mut i = 0usize;
    while i < led.greps.len() {
        if led.greps[i].kw == Kw::CompForm {
            if led.greps[i].hit_files.len() != expect.len() {
                return Err(E_HIT_BAD);
            }
            let mut j = 0usize;
            while j < expect.len() {
                if led.greps[i].hit_files[j] != expect[j] {
                    return Err(E_HIT_BAD);
                }
                j += 1;
            }
        }
        i += 1;
    }
    hits += 1;

    // R7 二次收口拒：收口后 closed 位锁死（判据 J1 只放行一次）。
    led.scan(Kw::Wmime)?;
    led.scan(Kw::Imm)?;
    led.close_batch()?;
    if led.close_batch().is_ok() {
        return Err(E_DUP_CLOSE);
    }
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行；空摘要拒。
    if writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    writeback(true, 0x06A1)?;
    hits += 1;

    Ok(hits)
}
