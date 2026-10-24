//! VE-F3773 · R 域七维汇总与 GO 判定
//!
//! 锚点：R 域七维汇总——七维汇总（API/测试/性能/安全/无障碍/文档/预算
//! 七维终版清点汇总（复述汇总）；GO/NO-GO 判定（七维全绿才 GO（复述判定）；
//! 汇总报告（证据同源（复述证据红线）；增量复查（过期证据增量复查（复述）。
//!
//! 数据结构：七维汇总表；GO 判定；证据同源；增量复查。
//! 错误路径与降级矩阵：缺维→NO-GO（判定实测）；证据异源→立案（红线）；
//! 增量缺失→补（复述）；过期→复查（复述）。
//! 跨批对接点：F3766-F3772 七清点输入；F3774 收口输入。
//!
//! 诊断码段：独占 0xA4xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xA4xx）
// ─────────────────────────────────────────────────────────────

/// F3773 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct H9Code(pub u16);

impl H9Code {
    /// 缺维——NO-GO。
    pub const DIM_MISS: H9Code = H9Code(0xA401);
    /// 证据异源——立案。
    pub const EVIDENCE_FORK: H9Code = H9Code(0xA402);
    /// 汇总不全——补。
    pub const SUMMARY_INCOMPLETE: H9Code = H9Code(0xA403);
    /// 增量缺失——补。
    pub const INCR_MISS: H9Code = H9Code(0xA404);
    /// 过期证据——复查。
    pub const STALE: H9Code = H9Code(0xA405);
    /// GO 判定失败。
    pub const NO_GO: H9Code = H9Code(0xA406);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xA401 => "七维有缺维，判定 NO-GO。",
            0xA402 => "证据异源，需立案。",
            0xA403 => "汇总不全，需补。",
            0xA404 => "增量复查记录缺失。",
            0xA405 => "证据过期，需复查。",
            0xA406 => "GO 判定失败。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 七维汇总
// ─────────────────────────────────────────────────────────────

/// 七维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalDim {
    /// API。
    Api,
    /// 测试。
    Test,
    /// 性能。
    Perf,
    /// 安全。
    Security,
    /// 无障碍。
    A11y,
    /// 文档。
    Doc,
    /// 预算。
    Budget,
}

impl FinalDim {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            FinalDim::Api => 0,
            FinalDim::Test => 1,
            FinalDim::Perf => 2,
            FinalDim::Security => 3,
            FinalDim::A11y => 4,
            FinalDim::Doc => 5,
            FinalDim::Budget => 6,
        }
    }

    /// 全集（七维）。
    pub fn all() -> [FinalDim; 7] {
        [
            FinalDim::Api,
            FinalDim::Test,
            FinalDim::Perf,
            FinalDim::Security,
            FinalDim::A11y,
            FinalDim::Doc,
            FinalDim::Budget,
        ]
    }
}

/// 七维汇总表。
#[derive(Clone, Debug)]
pub struct DimSummary {
    /// 每维是否全绿。
    green: [bool; 7],
    /// 每维证据 id（0=无）。
    evidence: [u32; 7],
}

impl DimSummary {
    /// 空汇总。
    pub fn new() -> DimSummary {
        DimSummary {
            green: [false; 7],
            evidence: [0; 7],
        }
    }

    /// 设维状态。
    pub fn set(&mut self, dim: FinalDim, green: bool, evidence_id: u32) {
        let i = dim.index() as usize;
        self.green[i] = green;
        self.evidence[i] = evidence_id;
    }

    /// GO：七维全绿 + 有证据。
    pub fn go(&self) -> bool {
        for i in 0..7 {
            if !self.green[i] || self.evidence[i] == 0 {
                return false;
            }
        }
        true
    }

    /// 缺维清单。
    pub fn missing(&self) -> Vec<FinalDim> {
        let mut bad = Vec::new();
        for d in FinalDim::all() {
            let i = d.index() as usize;
            if !self.green[i] || self.evidence[i] == 0 {
                bad.push(d);
            }
        }
        bad
    }

    /// 证据同源：所有证据 id 一致。
    pub fn evidence_same(&self) -> bool {
        let first = self.evidence[0];
        if first == 0 {
            return false;
        }
        self.evidence.iter().all(|e| *e == first)
    }
}

impl Default for DimSummary {
    fn default() -> DimSummary {
        DimSummary::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 增量复查
// ─────────────────────────────────────────────────────────────

/// 增量记录。
#[derive(Clone, Debug)]
pub struct IncrementalCheck {
    /// 证据 id。
    pub evidence_id: u32,
    /// 过期时刻。
    pub ttl: u64,
    /// 当前时刻。
    pub now: u64,
    /// 是否已复查。
    pub rechecked: bool,
}

impl IncrementalCheck {
    /// 构造。
    pub fn new(evidence_id: u32, ttl: u64, now: u64) -> IncrementalCheck {
        IncrementalCheck {
            evidence_id,
            ttl,
            now,
            rechecked: false,
        }
    }

    /// 过期。
    pub fn stale(&self) -> bool {
        self.ttl > 0 && self.now >= self.ttl
    }

    /// 合规：未过期 或 已复查。
    pub fn ok(&self) -> bool {
        !self.stale() || self.rechecked
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3773 判据集。
pub fn run_vr09m_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3773");

    // 0xA401 七维 GO
    {
        let mut s = DimSummary::new();
        out.add("0xA401-空汇总NOGO", !s.go() && s.missing().len() == 7, "");
        for d in FinalDim::all() {
            s.set(d, true, 100);
        }
        out.add("0xA401-七维全绿GO", s.go() && s.missing().is_empty(), "");
    }

    // 0xA402 证据同源
    {
        let mut s = DimSummary::new();
        for d in FinalDim::all() {
            s.set(d, true, 100);
        }
        out.add("0xA402-同源过", s.evidence_same(), "");
        s.set(FinalDim::Perf, true, 200);
        out.add("0xA402-异源检出", !s.evidence_same(), "");
    }

    // 0xA404/0xA405 增量
    {
        let c = IncrementalCheck::new(1, 100, 50);
        out.add("0xA405-未过期过", c.ok(), "");
        let c2 = IncrementalCheck::new(2, 100, 150);
        out.add("0xA405-过期检出", !c2.ok() && c2.stale(), "");
    }

    // 0xA406 GO 判定
    {
        let mut s = DimSummary::new();
        for d in FinalDim::all() {
            s.set(d, true, 1);
        }
        out.add("0xA406-GO过", s.go(), "");
    }

    out
}
