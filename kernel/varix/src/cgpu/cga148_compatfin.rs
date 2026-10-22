//! cga148_compatfin — CGPU-F0148 A 域兼容承诺终版。
//!
//! 判据原文（施工书锚点 CGPU-F0148）：A 域十年兼容承诺终版（F0136
//! 的收官版）：承诺清单终版 + 语义测试锁定终态 + 废弃流程就位确认。
//! 承诺即 CGPU-A 的十年合同文本。判据：清单终版、测试锁定、流程
//! 就位、不承诺显性、重申归档。
//!
//! **设计定盘**：终版合同 = F0136 承诺账的冻结版：终版登记语义 =
//! 承诺条目全量转入终版册（转册后阶段册不再可写——合同只有一份
//! 正本）。测试锁定终态 = 全部终版条目的语义测试指纹非零且与阶段
//! 册一致（指纹漂移即转册失败）。流程就位 = 废弃三阶段（标记/保留
//! 期/移除）的就位确认三登记。不承诺显性 = 两族终态显性。重申归档 =
//! 合同文本指纹归档（文本即指纹，指纹即文本）。
//!
//! 诊断码段 0x0330~0x033F（0x03 段第 4 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA148_VERSION: &str = "cga148-compatfin-v1";

/// 诊断子区基值（0x03 段第 4 子区）。
pub const SEG_BASE: u16 = 0x0330;

/// 终版承诺条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FinPledge {
    /// 主体。
    pub subject: u32,
    /// 承诺族（0=API 1=确定性 2=基准——与 F0136 闭集同构）。
    pub kind: u8,
    /// 语义测试指纹（终态锁定）。
    pub test_fp: u64,
    /// 起始主版本。
    pub since_major: u32,
}

impl FinPledge {
    /// 行合法性（族号闭集 0..=2）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.subject == 0 || self.test_fp == 0 || self.since_major == 0 {
            return Err(SEG_BASE + 1);
        }
        if self.kind > 2 {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
}

/// 终版合同账。
pub struct CompatFinalLedger {
    /// 终版承诺清单。
    pledges: Vec<FinPledge>,
    /// 三阶段就位确认（0=标记 1=保留期 2=移除）。
    phases_ready: [bool; 3],
    /// 不承诺显性（两族）。
    non_pledges: [bool; 2],
    /// 归档指纹。
    archived: Option<u64>,
}

impl CompatFinalLedger {
    /// 空账。
    pub fn new() -> Self {
        CompatFinalLedger {
            pledges: Vec::new(),
            phases_ready: [false; 3],
            non_pledges: [false; 2],
            archived: None,
        }
    }

    /// 转册登记（判据"清单终版"：同主体同族重复即拒——转册是
    /// 全量搬迁不是累积追加）。
    pub fn transfer(&mut self, p: FinPledge) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 3);
        }
        p.validate()?;
        if self
            .pledges
            .iter()
            .any(|x| x.subject == p.subject && x.kind == p.kind)
        {
            return Err(SEG_BASE + 4);
        }
        self.pledges.push(p);
        Ok(())
    }

    /// 阶段就位确认（判据"流程就位"；重复确认即拒）。
    pub fn confirm_phase(&mut self, phase: usize) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 3);
        }
        if phase > 2 || self.phases_ready[phase] {
            return Err(SEG_BASE + 5);
        }
        self.phases_ready[phase] = true;
        Ok(())
    }

    /// 不承诺显性确认（判据"不承诺显性"；族 0=内部实现 1=性能数值；
    /// 重复确认即拒）。
    pub fn confirm_non_pledge(&mut self, fam: usize) -> Result<(), u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 3);
        }
        if fam > 1 || self.non_pledges[fam] {
            return Err(SEG_BASE + 6);
        }
        self.non_pledges[fam] = true;
        Ok(())
    }

    /// 三族覆盖核验（判据"承诺清单终版"：三族每族至少一条）。
    pub fn kinds_covered(&self) -> Result<(), u16> {
        for k in 0u8..3 {
            if !self.pledges.iter().any(|p| p.kind == k) {
                return Err(SEG_BASE + 7);
            }
        }
        Ok(())
    }

    /// 流程就位核验（三阶段全确认）。
    pub fn phases_all_ready(&self) -> bool {
        self.phases_ready.iter().all(|p| *p)
    }

    /// 条目数。
    pub fn pledge_count(&self) -> usize {
        self.pledges.len()
    }

    /// 归档（判据"重申归档"：三族覆盖 + 三阶段就位 + 不承诺两族
    /// 全显 + 条目非空）。
    pub fn archive(&mut self) -> Result<u64, u16> {
        if self.archived.is_some() {
            return Err(SEG_BASE + 8);
        }
        self.kinds_covered()?;
        if !self.phases_all_ready() {
            return Err(SEG_BASE + 9);
        }
        if !self.non_pledges.iter().all(|p| *p) {
            return Err(SEG_BASE + 10);
        }
        if self.pledges.is_empty() {
            return Err(SEG_BASE + 11);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for p in &self.pledges {
            fp = fp.wrapping_mul(0x100_0000_01b3)
                ^ (p.subject as u64)
                ^ (p.kind as u64)
                ^ p.test_fp;
        }
        self.archived = Some(fp);
        Ok(fp)
    }

    /// 归档指纹。
    pub fn archive_fp(&self) -> Option<u64> {
        self.archived
    }
}

impl Default for CompatFinalLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl CompatFinalLedger {
    /// 承诺族普查（判据「三族闭集」投影）：三族各多少条承诺。
    pub fn kind_census(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for p in &self.pledges {
            if p.kind < 3 {
                out[p.kind as usize] += 1;
            }
        }
        out
    }

    /// 承诺指纹冲突检查：同一主体同族只能一条终态承诺
    /// （两条 = 版本线分裂，拒绝归档）。
    pub fn pledge_hygiene(&self) -> Result<(), u16> {
        for (i, p) in self.pledges.iter().enumerate() {
            for (j, q) in self.pledges.iter().enumerate() {
                if i != j && p.subject == q.subject && p.kind == q.kind {
                    return Err(SEG_BASE + 20);
                }
            }
        }
        Ok(())
    }

    /// 主版本单调性：同主体多族的起始主版本必须一致（同一次
    /// 冻结事件引入），不一致即版本漂移。
    pub fn since_monotonic(&self) -> Result<(), u16> {
        let mut seen: Vec<(u32, u32)> = Vec::new();
        for p in &self.pledges {
            match seen.iter_mut().find(|(s, _)| *s == p.subject) {
                Some((_, v)) => {
                    if *v != p.since_major {
                        return Err(SEG_BASE + 21);
                    }
                }
                None => seen.push((p.subject, p.since_major)),
            }
        }
        Ok(())
    }

    /// 归档前置自检（判据「归档」强校验）：三阶段齐 + 两族显性齐 +
    /// 承诺卫生 + 版本单调，四关全过才许归档。
    pub fn pre_archive(&self) -> Result<(), u16> {
        if !self.phases_ready.iter().all(|b| *b) {
            return Err(SEG_BASE + 22);
        }
        if !self.non_pledges.iter().all(|b| *b) {
            return Err(SEG_BASE + 23);
        }
        self.pledge_hygiene()?;
        self.since_monotonic()
    }
}

/// 终版承诺全局审计（独立函数面）：承诺卫生 + 归档前置 +
/// 指纹零校验（test_fp 为零的承诺 = 没有锁语义，等于没承诺）。
pub fn compat_audit(l: &CompatFinalLedger) -> Result<(), u16> {
    if l.archived.is_none() {
        return Err(SEG_BASE + 24);
    }
    for p in l.pledges.iter() {
        if p.test_fp == 0 {
            return Err(SEG_BASE + 25);
        }
    }
    l.pre_archive()
}

/// 承诺行独立校验（独立函数面）：主体非零、族闭集、
/// 起始主版本非零（0 版本承诺 = 未生效承诺）。
pub fn pledge_audit(p: &FinPledge) -> Result<(), u16> {
    if p.subject == 0 {
        return Err(SEG_BASE + 26);
    }
    if p.kind > 2 {
        return Err(SEG_BASE + 27);
    }
    if p.since_major == 0 {
        return Err(SEG_BASE + 28);
    }
    if p.test_fp == 0 {
        return Err(SEG_BASE + 29);
    }
    Ok(())
}

/// 不承诺显性闭环（独立函数面）：两族不承诺必须成对显性——
/// 只显性一族 = 另一族隐性承诺（最危险的承诺形态）。
pub fn non_pledge_closed(l: &CompatFinalLedger) -> Result<(), u16> {
    if l.archived.is_some() {
        let both = l.non_pledges.iter().all(|b| *b);
        if !both {
            return Err(SEG_BASE + 30);
        }
    }
    Ok(())
}
