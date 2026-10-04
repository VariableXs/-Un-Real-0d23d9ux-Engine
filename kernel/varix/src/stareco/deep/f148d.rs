//! 深化层 · F148 社区规则与治理（2026-09-26 回炉补深化）。
//!
//! 补深：复核池同池校验（F129 复核池共用——轮值成员资格）、规则
//! 版本册（修订留痕）、公示渲染（裁决编号可查）、回避制度（利益
//! 相关者不进三人组）。

use crate::checks::CheckSet;
use crate::stareco::ebase::TraceId;
use crate::stareco::governance::{Governance, CaseStage, GOV_DOCS, ARBITRATION_SLA_DAYS};

// ---------------------------------------------------------------------------
// 复核池同池校验（F129 复核池共用——轮值资格）
// ---------------------------------------------------------------------------

/// 轮值成员资格：登记在池 + 本季未回避 + 服务次数未超上限（防垄断）。
pub struct PoolMember {
    pub id: u32,
    pub in_pool: bool,
    pub conflicted_case: Option<u32>,
    pub served_this_season: u32,
}

pub const MAX_SERVE_PER_SEASON: u32 = 3;

impl PoolMember {
    /// 可入三人组判定。
    pub fn eligible(&self, case: u32) -> bool {
        self.in_pool
            && self.conflicted_case != Some(case)
            && self.served_this_season < MAX_SERVE_PER_SEASON
    }
}

/// 从池中选三人（互异 + 全部合格）；不足三人 = 池枯，案件挂起。
pub fn select_panel(pool: &[PoolMember], case: u32) -> Result<[u32; 3], &'static str> {
    let mut picked: [u32; 3] = [0; 3];
    let mut n = 0;
    for m in pool {
        if n == 3 {
            break;
        }
        if m.eligible(case) && !picked.contains(&m.id) {
            picked[n] = m.id;
            n += 1;
        }
    }
    if n < 3 {
        return Err("复核池枯：案件挂起等轮值补充");
    }
    Ok(picked)
}

// ---------------------------------------------------------------------------
// 回避制度
// ---------------------------------------------------------------------------

/// 利益相关判定：当事人/提交者同池成员 → 必须回避。
pub fn must_recline(member_id: u32, party_ids: &[u32]) -> bool {
    party_ids.contains(&member_id)
}

// ---------------------------------------------------------------------------
// 规则版本册（修订留痕）
// ---------------------------------------------------------------------------

pub struct RuleVersion {
    pub ver: u32,
    pub adr_ref: &'static str,
    pub day: u32,
}

/// 版本册校验：严格递增、每版必须挂 ADR。
pub fn rule_versions_ok(vs: &[RuleVersion]) -> bool {
    vs.windows(2).all(|w| w[1].ver > w[0].ver && w[1].day >= w[0].day)
        && vs.iter().all(|v| !v.adr_ref.is_empty())
}

// ---------------------------------------------------------------------------
// 公示渲染（裁决编号可查）
// ---------------------------------------------------------------------------

pub struct Publication {
    pub case_id: TraceId,
    pub verdict: u8,
    pub line: alloc::string::String,
}

/// 公示行：裁决编号 + 结论（1=维持 2=改判）——公开可查的渲染面。
pub fn render_publication(case_id: TraceId, verdict: u8) -> Publication {
    let mut buf = [0u8; 32];
    let n = case_id.render(&mut buf);
    let id_str = alloc::string::String::from_utf8_lossy(&buf[..n]).into_owned();
    let word = match verdict {
        1 => "维持",
        2 => "改判",
        _ => "未决",
    };
    Publication {
        case_id,
        verdict,
        line: alloc::format!("裁决 {}：{}（14 天内可申诉）", id_str, word),
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F148D_TAG: &str = "stareco-F148-deep";

pub fn run_f148_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F148D_TAG);

    // 复核池资格
    let pool = [
        PoolMember { id: 1, in_pool: true, conflicted_case: None, served_this_season: 0 },
        PoolMember { id: 2, in_pool: true, conflicted_case: Some(7), served_this_season: 0 },
        PoolMember { id: 3, in_pool: true, conflicted_case: None, served_this_season: 3 },
        PoolMember { id: 4, in_pool: false, conflicted_case: None, served_this_season: 0 },
        PoolMember { id: 5, in_pool: true, conflicted_case: None, served_this_season: 1 },
    ];
    let panel = select_panel(&pool, 7);
    set.add(
        "f148d panel skips conflicted/capped/off-pool",
        panel.is_err(),
        "case7：2 回避、3 次数满、4 不在池 → 只剩两人 → 池枯挂起",
    );
    let panel_ok = select_panel(&pool, 9);
    set.add(
        "f148d panel for clean case full",
        panel_ok == Ok([1, 2, 5]),
        "case9：2 无冲突可入 → 三人齐",
    );
    let rich_pool = [
        PoolMember { id: 1, in_pool: true, conflicted_case: None, served_this_season: 0 },
        PoolMember { id: 2, in_pool: true, conflicted_case: None, served_this_season: 0 },
        PoolMember { id: 3, in_pool: true, conflicted_case: None, served_this_season: 0 },
    ];
    set.add(
        "f148d panel full when pool healthy",
        select_panel(&rich_pool, 9) == Ok([1, 2, 3]),
        "三人齐",
    );

    // 回避
    set.add(
        "f148d recline law",
        must_recline(2, &[2, 8]) && !must_recline(2, &[8, 9]),
        "当事人回避",
    );

    // 规则版本册
    let vs = [
        RuleVersion { ver: 1, adr_ref: "ADR-G-01", day: 100 },
        RuleVersion { ver: 2, adr_ref: "ADR-G-02", day: 200 },
    ];
    set.add(
        "f148d rule versions monotonic",
        rule_versions_ok(&vs) && !rule_versions_ok(&[RuleVersion { ver: 2, adr_ref: "", day: 300 }]),
        "递增+ADR 必挂",
    );

    // 公示渲染
    let id = TraceId::new("GOV", 20260926, 1);
    let pub1 = render_publication(id, 1);
    let pub0 = render_publication(id, 0);
    set.add(
        "f148d publication rendering",
        pub1.line.contains("维持") && pub1.line.contains("GOV-20260926-1") && pub0.line.contains("未决"),
        "编号可查",
    );

    // 治理本体联动：三文档 + SLA 常量复诵
    let mut g = Governance::new();
    for d in GOV_DOCS.iter() {
        g.register_doc(d).ok();
    }
    set.add("f148d governance ready", g.docs_complete(), "三文档齐");
    set.add("f148d sla constant", ARBITRATION_SLA_DAYS == 14, "14 天");

    // 案件立案后推进到公示的完整链（深化层复诵）
    let cid = g.file_case(20260926, "F134-entry#9").expect("file");
    g.advance(cid, CaseStage::Reviewing, 20260927, [0; 3], 0).ok();
    g.advance(cid, CaseStage::PanelDeciding, 20260928, [1, 2, 3], 0).ok();
    g.advance(cid, CaseStage::Published, 20260929, [0; 3], 2).ok();
    set.add("f148d full case published", g.published == 1 && g.records_queryable(), "链完整可查");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn pool_exhaustion() {
        let pool = [PoolMember { id: 1, in_pool: true, conflicted_case: None, served_this_season: 9 }];
        assert!(select_panel(&pool, 5).is_err());
    }
}
