//! cga136_compat — CGPU-F0136 A 域兼容承诺。
//!
//! 判据原文（施工书锚点 CGPU-F0136）：A 域十年承诺（F0393 范式）：
//! 算子 API（F0125 冻结）十年不变、确定性语义承诺（同输入同输出跨
//! 版本）、基准判据语义稳定。不承诺：内部实现自由演进、性能数值随
//! 硬件演进。判据：承诺清单、语义测试、废弃流程、不承诺显性、重申。
//!
//! **设计定盘**：承诺合同账：承诺条目三族（API 签名十年不变 / 确定性
//! 语义跨版本 / 基准判据语义稳定），每条承诺挂语义测试指纹（判据
//! "语义测试"——没有锁定测试的承诺是空话）。不承诺面显性登记（判据
//! "不承诺显性"——内部实现演进 / 性能数值随硬件，两族闭集，不许
//! 隐式）。废弃流程衔接（判据"废弃流程"）：承诺期内算子不得进入
//! Sunset 之后阶段（Deprecated 可见但 Removed 违约）。重申 = 承诺
//! 续期登记（续期计数 + 续期指纹）。
//!
//! 诊断码段 0x0270~0x027F（0x02 段第 8 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA136_VERSION: &str = "cga136-compat-v1";

/// 诊断子区基值（0x02 段第 8 子区）。
pub const SEG_BASE: u16 = 0x0270;

/// 承诺期（十年——按主版本代际记账，一代际 = 一个主版本）。
pub const PROMISE_MAJOR_SPAN: u32 = 10;

/// 承诺条目三族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PledgeKind {
    /// API 签名十年不变。
    ApiFrozen,
    /// 确定性语义跨版本。
    DetSemantics,
    /// 基准判据语义稳定。
    BenchSemantics,
}

impl PledgeKind {
    /// 全集稳定序。
    pub const ALL: [PledgeKind; 3] = [
        PledgeKind::ApiFrozen,
        PledgeKind::DetSemantics,
        PledgeKind::BenchSemantics,
    ];

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            PledgeKind::ApiFrozen => 0,
            PledgeKind::DetSemantics => 1,
            PledgeKind::BenchSemantics => 2,
        }
    }
}

/// 不承诺族闭集（判据"不承诺显性"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NonPledge {
    /// 内部实现自由演进。
    InternalEvolution,
    /// 性能数值随硬件演进。
    PerfVaries,
}

impl NonPledge {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            NonPledge::InternalEvolution => SEG_BASE + 4,
            NonPledge::PerfVaries => SEG_BASE + 5,
        }
    }
}

/// 一条承诺条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pledge {
    /// 承诺主体（算子号或判据号）。
    pub subject: u32,
    /// 承诺族。
    pub kind: PledgeKind,
    /// 语义测试指纹（零=无锁定测试=空话承诺）。
    pub test_fp: u64,
    /// 起始主版本。
    pub since_major: u32,
}

/// 兼容合同账。
pub struct CompatContract {
    /// 承诺条目。
    pledges: Vec<Pledge>,
    /// 不承诺显性面。
    non_pledges: Vec<NonPledge>,
    /// 续期账（代际 → 续期指纹）。
    renewals: Vec<(u32, u64)>,
    /// 废弃衔接提示（进入 Sunset 的算子——承诺期内不许 Removed）。
    sunset_watch: Vec<u32>,
    /// 签章。
    sealed: Option<u64>,
}

impl CompatContract {
    /// 空合同。
    pub fn new() -> Self {
        CompatContract {
            pledges: Vec::new(),
            non_pledges: Vec::new(),
            renewals: Vec::new(),
            sunset_watch: Vec::new(),
            sealed: None,
        }
    }

    /// 承诺登记（判据"承诺清单"：同主体同族重复即拒；无测试指纹即拒；
    /// 起始版本为零即拒）。
    pub fn pledge(
        &mut self,
        subject: u32,
        kind: PledgeKind,
        test_fp: u64,
        since_major: u32,
    ) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        if subject == 0 || test_fp == 0 || since_major == 0 {
            return Err(SEG_BASE + 7);
        }
        if self
            .pledges
            .iter()
            .any(|p| p.subject == subject && p.kind == kind)
        {
            return Err(SEG_BASE + 8);
        }
        self.pledges.push(Pledge {
            subject,
            kind,
            test_fp,
            since_major,
        });
        Ok(())
    }

    /// 不承诺显性登记（判据"不承诺显性"：两族全登记——只显一半
    /// 等于另一半被隐式承诺，即拒）。
    pub fn declare_non_pledge(&mut self, np: NonPledge) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        if self.non_pledges.contains(&np) {
            return Err(SEG_BASE + 9);
        }
        self.non_pledges.push(np);
        Ok(())
    }

    /// 三族全覆盖核验（判据"承诺清单"：三族每族至少一条承诺）。
    pub fn kinds_covered(&self) -> Result<(), u16> {
        for k in PledgeKind::ALL {
            if !self.pledges.iter().any(|p| p.kind == k) {
                return Err(k.code());
            }
        }
        Ok(())
    }

    /// 废弃衔接登记（判据"废弃流程"：进入 Sunset 的算子列入监视）。
    pub fn watch_sunset(&mut self, subject: u32) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        if subject == 0 || self.sunset_watch.contains(&subject) {
            return Err(SEG_BASE + 10);
        }
        self.sunset_watch.push(subject);
        Ok(())
    }

    /// 承诺期违约核验（判据"十年承诺"：监视中算子的承诺期 =
    /// 起始版本 + PROMISE_MAJOR_SPAN；当前版本超出承诺期才许 Removed
    /// ——本核验由 Removed 事件触发方消费，在期内即违约红）。
    pub fn promise_active(&self, subject: u32, current_major: u32) -> Result<bool, u16> {
        let p = self
            .pledges
            .iter()
            .find(|p| p.subject == subject)
            .ok_or(SEG_BASE + 11)?;
        Ok(current_major < p.since_major.saturating_add(PROMISE_MAJOR_SPAN))
    }

    /// 续期登记（判据"重申"：代际续期，同代际重复即拒）。
    pub fn renew(&mut self, generation: u32, fp: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 6);
        }
        if generation == 0 || fp == 0 {
            return Err(SEG_BASE + 12);
        }
        if self.renewals.iter().any(|(g, _)| *g == generation) {
            return Err(SEG_BASE + 13);
        }
        self.renewals.push((generation, fp));
        Ok(())
    }

    /// 续期数。
    pub fn renewal_count(&self) -> usize {
        self.renewals.len()
    }

    /// 承诺条数。
    pub fn pledge_count(&self) -> usize {
        self.pledges.len()
    }

    /// 签章（判据"重申归档"：三族覆盖 + 不承诺两族全显才能签）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 14);
        }
        self.kinds_covered()?;
        if self.non_pledges.len() != 2 {
            return Err(SEG_BASE + 15);
        }
        if self.pledges.is_empty() {
            return Err(SEG_BASE + 16);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for p in &self.pledges {
            fp = fp.wrapping_mul(0x100_0000_01b3)
                ^ (p.subject as u64)
                ^ (p.kind as u64)
                ^ p.test_fp;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for CompatContract {
    fn default() -> Self {
        Self::new()
    }
}

impl CompatContract {
    /// 承诺族计数投影（判据"承诺清单"的账面：三族各几条）。
    pub fn kind_census(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for p in &self.pledges {
            out[p.kind.ordinal() as usize] += 1;
        }
        out
    }

    /// 承诺查询（主体在承诺期内的全部条目）。
    pub fn pledges_of(&self, subject: u32) -> Vec<&Pledge> {
        self.pledges.iter().filter(|p| p.subject == subject).collect()
    }

    /// 不承诺投影（判据"不承诺显性"的读取面）。
    pub fn non_pledge_list(&self) -> Vec<NonPledge> {
        self.non_pledges.clone()
    }

    /// 废弃监视数。
    pub fn sunset_watch_count(&self) -> usize {
        self.sunset_watch.len()
    }

    /// 承诺期余量（判据"十年承诺"的量化面：当前版本距承诺到期还有
    /// 几个主版本代际；无承诺条目返回 None）。
    pub fn promise_headroom(&self, subject: u32, current_major: u32) -> Option<u32> {
        let p = self.pledges.iter().find(|p| p.subject == subject)?;
        Some(
            p.since_major
                .saturating_add(PROMISE_MAJOR_SPAN)
                .saturating_sub(current_major),
        )
    }

    /// 签章指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }

    /// 续期代际投影（判据"重申"的轨迹面）。
    pub fn renewal_generations(&self) -> Vec<u32> {
        self.renewals.iter().map(|(g, _)| *g).collect()
    }
}
