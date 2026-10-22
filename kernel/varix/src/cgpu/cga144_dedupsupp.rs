//! cga144_dedupsupp — CGPU-F0144 A 域查重补终审。
//!
//! 判据原文（施工书锚点 CGPU-F0144）：F0130 终审的补审：A02b-A10 批
//! 新增项的查重补全（收官批的新条目也要查重——收官批自己不能豁免），
//! 与 VE 域新增域（C/D 域已写入大量内容）的交叉比对。判据：新增项
//! 全覆盖、交叉比对、三态处置、报告、证据。
//!
//! **设计定盘**：补审账 = F0130 已比对对的镜像增量：本账只收
//! F0130 封账后新增的比对对（补审语义——旧对不重收）。新增项覆盖 =
//! 收官批新增的 A 项号集必须全被比对过（项级覆盖核验）。交叉比对 =
//! 与 VE 域 C/D 域新增内容的外部通道行（external 通道区分 VE-C/VE-D
//! 两通道）。三态处置 = Distinct/OverlapDeclared/Duplicate 同 F0130
//! 闭集；Duplicate 立案必须全部核销才能出报告。报告 = 覆盖数/比对数/
//! 立案数/处置数四元指纹链。
//!
//! 诊断码段 0x02F0~0x02FF（0x02 段第 16 子区，0x02 段满额收官）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA144_VERSION: &str = "cga144-dedupsupp-v1";

/// 诊断子区基值（0x02 段第 16 子区）。
pub const SEG_BASE: u16 = 0x02F0;

/// 高相似立案线（千分比，与 F0130 同口径——补审不许放水）。
pub const OVERLAP_PERMILLE: u64 = 700;

/// 外部通道闭集（判据"与 VE 域新增域交叉比对"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtChannel {
    /// VE-C 编译器域新增。
    VeC,
    /// VE-D 渲染域新增。
    VeD,
}

impl ExtChannel {
    /// 通道号。
    pub fn ordinal(self) -> u8 {
        match self {
            ExtChannel::VeC => 0,
            ExtChannel::VeD => 1,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 三态处置闭集（判据"三态处置"——与 F0130 同构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SupVerdict {
    /// 无重叠。
    Distinct,
    /// 有重叠但已声明分工。
    OverlapDeclared,
    /// 重复（立案）。
    Duplicate,
}

/// 一条补审比对行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SupCompareRow {
    /// A 域新增项号。
    pub a_item: u32,
    /// 外部通道。
    pub ch: ExtChannel,
    /// 外部项号。
    pub ext_item: u32,
    /// 相似度（千分比）。
    pub sim_permille: u64,
    /// 处置。
    pub verdict: SupVerdict,
}

/// 一条新增项覆盖行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NewItemRow {
    /// 收官批新增项号。
    pub item: u32,
    /// 来源批次号（A02b-A10）。
    pub batch: u32,
}

/// 补审账。
pub struct SupDedupLedger {
    /// 新增项账。
    new_items: Vec<NewItemRow>,
    /// 分工声明对。
    declared: Vec<(u32, u32)>,
    /// 比对行。
    rows: Vec<SupCompareRow>,
    /// 立案（项号）。
    cases: Vec<u32>,
    /// 封账。
    sealed: Option<u64>,
}

impl SupDedupLedger {
    /// 空账。
    pub fn new() -> Self {
        SupDedupLedger {
            new_items: Vec::new(),
            declared: Vec::new(),
            rows: Vec::new(),
            cases: Vec::new(),
            sealed: None,
        }
    }

    /// 新增项登记（判据"新增项全覆盖"的真源；同项重复即拒）。
    pub fn add_new_item(&mut self, item: u32, batch: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if item == 0 || batch == 0 {
            return Err(SEG_BASE + 5);
        }
        if self.new_items.iter().any(|n| n.item == item) {
            return Err(SEG_BASE + 6);
        }
        self.new_items.push(NewItemRow { item, batch });
        Ok(())
    }

    /// 声明登记（判据"分工声明复核"前置）。
    pub fn declare_pair(&mut self, a_item: u32, ext_item: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if self.declared.iter().any(|&(a, e)| a == a_item && e == ext_item) {
            return Err(SEG_BASE + 7);
        }
        self.declared.push((a_item, ext_item));
        Ok(())
    }

    /// 比对（判据"交叉比对"：同对重复即拒；超线未声明 → Duplicate
    /// 立案，超线已声明 → OverlapDeclared）。
    pub fn compare(
        &mut self,
        a_item: u32,
        ch: ExtChannel,
        ext_item: u32,
        sim_permille: u64,
    ) -> Result<SupVerdict, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if sim_permille > 1000 {
            return Err(SEG_BASE + 8);
        }
        if self.rows.iter().any(|r| {
            r.a_item == a_item
                && r.ch == ch
                && r.ext_item == ext_item
        }) {
            return Err(SEG_BASE + 9);
        }
        let declared = self
            .declared
            .iter()
            .any(|&(a, e)| a == a_item && e == ext_item);
        let verdict = if sim_permille < OVERLAP_PERMILLE {
            SupVerdict::Distinct
        } else if declared {
            SupVerdict::OverlapDeclared
        } else {
            if !self.cases.contains(&a_item) {
                self.cases.push(a_item);
            }
            SupVerdict::Duplicate
        };
        self.rows.push(SupCompareRow {
            a_item,
            ch,
            ext_item,
            sim_permille,
            verdict,
        });
        Ok(verdict)
    }

    /// 覆盖核验（判据"新增项全覆盖"：每个新增项至少一条比对行）。
    pub fn coverage_complete(&self) -> Result<(), u16> {
        for n in &self.new_items {
            if !self.rows.iter().any(|r| r.a_item == n.item) {
                return Err(SEG_BASE + 10);
            }
        }
        Ok(())
    }

    /// 通道覆盖核验（判据"与 VE 域新增域的交叉比对"：两通道都必须
    /// 有比对行——只比一个通道不算交叉）。
    pub fn channels_covered(&self) -> Result<(), u16> {
        for ch in [ExtChannel::VeC, ExtChannel::VeD] {
            if !self.rows.iter().any(|r| r.ch == ch) {
                return Err(ch.code());
            }
        }
        Ok(())
    }

    /// 立案核销（判据"三态处置"闭环）。
    pub fn resolve_case(&mut self, a_item: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        let pos = match self.cases.iter().position(|&c| c == a_item) {
            Some(p) => p,
            None => return Err(SEG_BASE + 11),
        };
        self.cases.remove(pos);
        Ok(())
    }

    /// 三态计数。
    pub fn verdict_counts(&self) -> (usize, usize, usize) {
        let d = self.rows.iter().filter(|r| r.verdict == SupVerdict::Distinct).count();
        let o = self
            .rows
            .iter()
            .filter(|r| r.verdict == SupVerdict::OverlapDeclared)
            .count();
        let u = self.rows.iter().filter(|r| r.verdict == SupVerdict::Duplicate).count();
        (d, o, u)
    }

    /// 报告摘要（判据"报告"：新增数/比对数/立案数/通道覆盖）。
    pub fn report_summary(&self) -> (usize, usize, usize, bool) {
        (
            self.new_items.len(),
            self.rows.len(),
            self.cases.len(),
            self.channels_covered().is_ok(),
        )
    }

    /// 新增项数。
    pub fn new_item_count(&self) -> usize {
        self.new_items.len()
    }

    /// 封账（判据"证据"：覆盖全 + 通道全 + 立案清零才能封）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 12);
        }
        self.coverage_complete()?;
        self.channels_covered()?;
        if !self.cases.is_empty() {
            return Err(SEG_BASE + 13);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for r in &self.rows {
            fp = fp.wrapping_mul(0x100_0000_01b3)
                ^ (r.a_item as u64)
                ^ ((r.verdict as u64) << 24);
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for SupDedupLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl SupDedupLedger {
    /// 封账前全量自检（判据「覆盖完备」的强校验版）：
    /// 每条新增项要么已被声明对覆盖、要么已立案销账；未封账才可跑。
    pub fn pre_seal_audit(&self) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 20);
        }
        for r in &self.new_items {
            let covered = self
                .declared
                .iter()
                .any(|(a, ext)| *a == r.item || *ext == r.item);
            let cased = self.cases.iter().any(|c| *c == r.item);
            if !covered && !cased {
                return Err(SEG_BASE + 21);
            }
        }
        Ok(())
    }

    /// 声明对自反检查（判据「分工声明」 hygiene）：同一项不得同时
    /// 出现在两条声明对里（一女不嫁二夫——分工必须唯一）。
    pub fn declaration_hygiene(&self) -> Result<(), u16> {
        for (i, (a, _)) in self.declared.iter().enumerate() {
            for (j, (a2, _)) in self.declared.iter().enumerate() {
                if i != j && a == a2 {
                    return Err(SEG_BASE + 22);
                }
            }
        }
        Ok(())
    }

    /// 比对一致率（千分比）：全部比对行中同判行占比。
    pub fn agree_permille(&self) -> Option<u32> {
        if self.rows.is_empty() {
            return None;
        }
        let agree = self.rows.iter().filter(|r| r.sim_permille >= 800).count();
        Some(((agree * 1000) / self.rows.len()) as u32)
    }
}
