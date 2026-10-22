//! cga152_opplog.rs —— CGPU-F0152 · 遗留优化机会登记
//!
//! 「不做的」也要登记：A 域识别出但明确不做/延后的优化机会清单。
//! 每条：机会描述指纹 / 预估收益 / 不做理由 / 重评条件。
//! 目的：防止未来重复评估（同指纹机会再出现时直接引用历史结论），
//! 也防止机会流失（重评条件满足时必须重新上会）。
//! 登记纪律与 F0336 风险登记同构：登记即归档、复审有节奏。
//! 诊断码段 0x0370（本席 WB1022 分配表，全域唯一）。
//! no_std + alloc 零 unwrap、零 panic 路径。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0370;

/// 收益量级（预估，账面三档 + 不适用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenefitTier {
    /// 微（<5%）：不值得动。
    Marginal,
    /// 中（5%~15%）：有条件值得。
    Moderate,
    /// 大（>15%）：除非结构性风险否则应做。
    Major,
    /// 无法估（信息不足，本身即重评理由）。
    Unknown,
}

impl BenefitTier {
    /// 全档位序（账面轮转与统计用）。
    pub const ALL: [BenefitTier; 4] = [
        BenefitTier::Marginal,
        BenefitTier::Moderate,
        BenefitTier::Major,
        BenefitTier::Unknown,
    ];

    /// 档位序号。
    pub fn rank(self) -> u8 {
        match self {
            BenefitTier::Marginal => 0,
            BenefitTier::Moderate => 1,
            BenefitTier::Major => 2,
            BenefitTier::Unknown => 3,
        }
    }
}

/// 机会状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OppState {
    /// 已登记，未裁决。
    Logged,
    /// 已裁决：明确不做（理由必须落账）。
    Rejected,
    /// 已裁决：延后（重评条件必须落账）。
    Deferred,
    /// 重评后被捞起，转入实施泳道（本册终点，转出即销账）。
    Promoted,
    /// 复审后撤销：机会前提已消失（硬件迭代/需求变更）。
    Obsoleted,
}

/// 重评条件（结构化，禁止自由文本一句话糊弄）。
#[derive(Clone, Debug)]
pub struct ReEvalCond {
    /// 条件类别：0=硬件代际 1=负载画像变化 2=依赖项就绪 3=成本阈值 4=其他(须注记)。
    pub kind: u8,
    /// 条件描述指纹（FNV-1a 64）。
    pub cond_fp: u64,
    /// 条件是否已满足。
    pub met: bool,
}

/// 一条遗留优化机会。
#[derive(Clone, Debug)]
pub struct Opportunity {
    /// 机会描述指纹（同指纹 = 同机会，防重复评估）。
    pub desc_fp: u64,
    /// 预估收益档位。
    pub benefit: BenefitTier,
    /// 不做/延后理由指纹（裁决时必填，Logged 态为 None）。
    pub reject_fp: Option<u64>,
    /// 重评条件清单（Deferred 态至少 1 条）。
    pub conds: Vec<ReEvalCond>,
    /// 状态。
    pub state: OppState,
    /// 复审节奏：距下次复审的最小 tick 数（按收益档默认，可覆写）。
    pub review_interval: u32,
    /// 上次复审 tick。
    pub last_review: u32,
}

impl Opportunity {
    /// 默认复审节奏：Unknown 最密（信息不足要勤看），Major 最密次之，
    /// Marginal 最疏（不值得常看）。
    pub fn default_interval(b: BenefitTier) -> u32 {
        match b {
            BenefitTier::Unknown => 16,
            BenefitTier::Major => 32,
            BenefitTier::Moderate => 64,
            BenefitTier::Marginal => 256,
        }
    }

    /// 结构自检（判据「条目结构」）：指纹非零、条件类别合法、
    /// Deferred 必有条件、Rejected 必有理由。
    pub fn validate(&self) -> Result<(), u16> {
        if self.desc_fp == 0 {
            return Err(SEG_BASE + 1);
        }
        for c in &self.conds {
            if c.kind > 4 {
                return Err(SEG_BASE + 2);
            }
            if c.cond_fp == 0 {
                return Err(SEG_BASE + 3);
            }
        }
        if self.state == OppState::Deferred && self.conds.is_empty() {
            return Err(SEG_BASE + 4);
        }
        if self.state == OppState::Rejected && self.reject_fp.is_none() {
            return Err(SEG_BASE + 5);
        }
        if self.review_interval == 0 {
            return Err(SEG_BASE + 6);
        }
        Ok(())
    }
}

/// FNV-1a 64 位指纹。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 遗留机会登记册。
pub struct OppRegistry {
    /// 全部条目（登记即入册，永不物理删除——防流失的第一原则）。
    entries: Vec<Opportunity>,
    /// 归档计数：转入终态（Promoted/Obsoleted）的条目数。
    archived: u32,
    /// 当前 tick（复审时钟）。
    tick: u32,
    /// 重复评估拦截计数（审计用）。
    dup_blocked: u32,
    /// 捞起计数（重评转实施）。
    promoted: u32,
}

impl OppRegistry {
    /// 空册。
    pub fn new() -> Self {
        OppRegistry {
            entries: Vec::new(),
            archived: 0,
            tick: 0,
            dup_blocked: 0,
            promoted: 0,
        }
    }

    /// 条目总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 归档数。
    pub fn archived_count(&self) -> u32 {
        self.archived
    }

    /// 重复拦截数。
    pub fn dup_blocked_count(&self) -> u32 {
        self.dup_blocked
    }

    /// 捞起数。
    pub fn promoted_count(&self) -> u32 {
        self.promoted
    }

    /// 指纹查条目。
    pub fn find(&self, desc_fp: u64) -> Option<&Opportunity> {
        self.entries.iter().find(|e| e.desc_fp == desc_fp)
    }

    /// 登记新机会（判据「登记归档」入口）：同指纹已存在即拒绝——
    /// 防重复评估的正确姿势是引用历史结论而不是再评一遍。
    pub fn log(&mut self, desc: &[u8], benefit: BenefitTier) -> Result<u64, u16> {
        let fp = fnv1a64(desc);
        if fp == 0 {
            return Err(SEG_BASE + 7);
        }
        if desc.is_empty() {
            return Err(SEG_BASE + 8);
        }
        if self.find(fp).is_some() {
            self.dup_blocked = self.dup_blocked.saturating_add(1);
            return Err(SEG_BASE + 9);
        }
        self.entries.push(Opportunity {
            desc_fp: fp,
            benefit,
            reject_fp: None,
            conds: Vec::new(),
            state: OppState::Logged,
            review_interval: Opportunity::default_interval(benefit),
            last_review: self.tick,
        });
        Ok(fp)
    }

    /// 裁决为「不做」：理由必填（判据「不做理由」）。
    /// Major 档不做必须给足理由——理由指纹为零即拒。
    pub fn reject(&mut self, desc_fp: u64, reason: &[u8]) -> Result<(), u16> {
        let e = self
            .entries
            .iter_mut()
            .find(|e| e.desc_fp == desc_fp)
            .ok_or(SEG_BASE + 10)?;
        if e.state != OppState::Logged && e.state != OppState::Deferred {
            return Err(SEG_BASE + 11);
        }
        if reason.is_empty() {
            return Err(SEG_BASE + 12);
        }
        e.reject_fp = Some(fnv1a64(reason));
        e.state = OppState::Rejected;
        e.last_review = self.tick;
        Ok(())
    }

    /// 裁决为「延后」：重评条件至少一条（判据「重评条件」）。
    pub fn defer(&mut self, desc_fp: u64, conds: Vec<ReEvalCond>) -> Result<(), u16> {
        let e = self
            .entries
            .iter_mut()
            .find(|e| e.desc_fp == desc_fp)
            .ok_or(SEG_BASE + 10)?;
        if e.state != OppState::Logged {
            return Err(SEG_BASE + 13);
        }
        if conds.is_empty() {
            return Err(SEG_BASE + 4);
        }
        for c in &conds {
            if c.kind > 4 || c.cond_fp == 0 {
                return Err(SEG_BASE + 2);
            }
        }
        e.conds = conds;
        e.state = OppState::Deferred;
        e.last_review = self.tick;
        Ok(())
    }

    /// 条件达成标记：全部条件满足的 Deferred 条目进入「待捞起」判定。
    pub fn mark_cond(&mut self, desc_fp: u64, cond_fp: u64) -> Result<bool, u16> {
        let e = self
            .entries
            .iter_mut()
            .find(|e| e.desc_fp == desc_fp)
            .ok_or(SEG_BASE + 10)?;
        let c = e
            .conds
            .iter_mut()
            .find(|c| c.cond_fp == cond_fp)
            .ok_or(SEG_BASE + 14)?;
        c.met = true;
        Ok(e.conds.iter().all(|c| c.met))
    }

    /// 捞起（判据「防止机会流失」的出口）：全条件满足的 Deferred
    /// 才可 Promote；Major 档捞起时条件数必须 ≥2（单条件太草率）。
    pub fn promote(&mut self, desc_fp: u64) -> Result<(), u16> {
        let e = self
            .entries
            .iter_mut()
            .find(|e| e.desc_fp == desc_fp)
            .ok_or(SEG_BASE + 10)?;
        if e.state != OppState::Deferred {
            return Err(SEG_BASE + 15);
        }
        if !e.conds.iter().all(|c| c.met) {
            return Err(SEG_BASE + 16);
        }
        if e.benefit.rank() >= BenefitTier::Major.rank() && e.conds.len() < 2 {
            return Err(SEG_BASE + 17);
        }
        e.state = OppState::Promoted;
        self.promoted = self.promoted.saturating_add(1);
        self.archived = self.archived.saturating_add(1);
        Ok(())
    }

    /// 撤销（前提消失）：Rejected/Deferred 均可 Obsolete，理由必填。
    pub fn obsoletes(&mut self, desc_fp: u64, reason: &[u8]) -> Result<(), u16> {
        let e = self
            .entries
            .iter_mut()
            .find(|e| e.desc_fp == desc_fp)
            .ok_or(SEG_BASE + 10)?;
        if e.state == OppState::Promoted || e.state == OppState::Obsoleted {
            return Err(SEG_BASE + 18);
        }
        if reason.is_empty() {
            return Err(SEG_BASE + 12);
        }
        e.state = OppState::Obsoleted;
        e.reject_fp = Some(fnv1a64(reason));
        self.archived = self.archived.saturating_add(1);
        Ok(())
    }

    /// 时钟步进，返回到期待复审的指纹清单（判据「复审节奏」）：
    /// last_review + interval <= tick 即到期。终态条目不再复审。
    pub fn advance(&mut self, ticks: u32) -> Vec<u64> {
        self.tick = self.tick.saturating_add(ticks);
        let t = self.tick;
        self.entries
            .iter()
            .filter(|e| {
                !matches!(e.state, OppState::Promoted | OppState::Obsoleted)
                    && t >= e.last_review.saturating_add(e.review_interval)
            })
            .map(|e| e.desc_fp)
            .collect()
    }

    /// 复审落账：到期复审完成后刷新 last_review（可顺带调节奏）。
    pub fn review_done(&mut self, desc_fp: u64, new_interval: Option<u32>) -> Result<(), u16> {
        let e = self
            .entries
            .iter_mut()
            .find(|e| e.desc_fp == desc_fp)
            .ok_or(SEG_BASE + 10)?;
        if let Some(iv) = new_interval {
            if iv == 0 {
                return Err(SEG_BASE + 6);
            }
            e.review_interval = iv;
        }
        e.last_review = self.tick;
        Ok(())
    }

    /// 状态普查（判据支撑：五态各多少条）。
    pub fn state_census(&self) -> [u32; 5] {
        let mut out = [0u32; 5];
        for e in &self.entries {
            let i = match e.state {
                OppState::Logged => 0,
                OppState::Rejected => 1,
                OppState::Deferred => 2,
                OppState::Promoted => 3,
                OppState::Obsoleted => 4,
            };
            out[i] += 1;
        }
        out
    }

    /// 收益档普查。
    pub fn benefit_census(&self) -> [u32; 4] {
        let mut out = [0u32; 4];
        for e in &self.entries {
            out[e.benefit.rank() as usize] += 1;
        }
        out
    }

    /// 活跃负载（非终态条目占比，千分比）。
    pub fn active_permille(&self) -> u32 {
        if self.entries.is_empty() {
            return 0;
        }
        let active = self
            .entries
            .iter()
            .filter(|e| !matches!(e.state, OppState::Promoted | OppState::Obsoleted))
            .count();
        ((active * 1000) / self.entries.len()) as u32
    }
}

impl Default for OppRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 复审节奏审计（判据「复审节奏」的汇总面）：
/// 终态以外条目 interval 必须落在 [16, 1024] 且为 2 的幂——
/// 节奏全部对齐时钟分频，避免碎片化唤醒。
pub fn cadence_audit(entries: &[Opportunity]) -> Result<(), u16> {
    for e in entries {
        if matches!(e.state, OppState::Promoted | OppState::Obsoleted) {
            continue;
        }
        if e.review_interval < 16 || e.review_interval > 1024 {
            return Err(SEG_BASE + 19);
        }
        if !e.review_interval.is_power_of_two() {
            return Err(SEG_BASE + 20);
        }
    }
    Ok(())
}

/// 重复评估引用（判据「防止未来重复评估」的查询面）：
/// 新评估请求进来，先查册——命中且带结论（Rejected 有理由 /
/// Obsoleted 有理由）即返回历史结论指纹，调用方直接引用，
/// 不许重评。Logged/Deferred 命中返回 Ok(None)（结论未定，可续评）。
pub fn prior_verdict(reg: &OppRegistry, desc: &[u8]) -> Result<Option<u64>, u16> {
    let fp = fnv1a64(desc);
    match reg.find(fp) {
        Some(e) => match e.state {
            OppState::Rejected | OppState::Obsoleted => Ok(e.reject_fp),
            _ => Ok(None),
        },
        None => Err(SEG_BASE + 21),
    }
}
