//! 深化层三 · F148 社区规则与治理（2026-09-26 深化批次三）。
//!
//! 补深仲裁运营工程面（主册 G-D-23）：裁决员负载均衡分派（最闲者
//! 先+回避方排除）、回避声明链（append-only 可溯）、公示期计时器
//! （7 天公示 → 生效日；异议冻结）、规则修订频率闸（每季一次窗口）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 裁决员分派：最闲者优先，平局按序号小者；涉事方排除（回避）
// ---------------------------------------------------------------------------

/// 分派裁决：panelists (名, 在办案件数, 是否涉事) → 选中的序号。
pub fn assign_panelist(panelists: &[(u8, u32, bool)]) -> Result<usize, &'static str> {
    if panelists.is_empty() {
        return Err("裁决员池为空");
    }
    let mut best: Option<usize> = None;
    let mut best_load = u32::MAX;
    for (i, (_, load, involved)) in panelists.iter().enumerate() {
        if *involved {
            continue;
        }
        if *load < best_load {
            best_load = *load;
            best = Some(i);
        }
    }
    best.ok_or("全员回避：案件需外部仲裁员")
}

/// 三人组构成：互异非零且无涉事（governance 基础判据的机器面）。
pub fn panel_valid(chosen: &[usize], all_involved: &[bool]) -> bool {
    if chosen.len() != 3 {
        return false;
    }
    let mut uniq: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
    for c in chosen {
        if all_involved.get(*c).copied().unwrap_or(true) {
            return false;
        }
        if uniq.contains(c) {
            return false;
        }
        uniq.push(*c);
    }
    true
}

// ---------------------------------------------------------------------------
// 回避声明链：append-only；(案件, 裁决员) 唯一声明一次
// ---------------------------------------------------------------------------

pub struct RecusalChain {
    events: alloc::vec::Vec<(u32, u8, &'static str)>, // (案件号, 裁决员, 理由)
}

impl RecusalChain {
    pub fn new() -> RecusalChain {
        RecusalChain { events: alloc::vec::Vec::new() }
    }

    pub fn declare(&mut self, case: u32, panelist: u8, reason: &'static str) -> Result<(), &'static str> {
        if reason.is_empty() {
            return Err("回避必须带理由：零静默");
        }
        if self.events.iter().any(|(c, p, _)| *c == case && *p == panelist) {
            return Err("同一案件同一裁决员重复声明");
        }
        self.events.push((case, panelist, reason));
        Ok(())
    }

    pub fn is_recused(&self, case: u32, panelist: u8) -> bool {
        self.events.iter().any(|(c, p, _)| *c == case && *p == panelist)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
}

// ---------------------------------------------------------------------------
// 公示期计时：公示 7 天 → 生效日；异议在期 → 冻结（延后生效）
// ---------------------------------------------------------------------------

pub const PUBLIC_NOTICE_DAYS: u32 = 7;

pub struct PublicNotice {
    pub start_day: u32,
    pub objections: u32,
}

impl PublicNotice {
    /// 生效日：无异议 = 起始+7；有异议 = 冻结（None——不可预知何时解冻）。
    pub fn effective_day(&self) -> Option<u32> {
        if self.objections > 0 {
            None
        } else {
            Some(self.start_day + PUBLIC_NOTICE_DAYS)
        }
    }

    pub fn frozen(&self) -> bool {
        self.objections > 0
    }
}

// ---------------------------------------------------------------------------
// 修订频率闸：每季一次——距上次修订 <90 天拒绝
// ---------------------------------------------------------------------------

pub const AMENDMENT_MIN_GAP: u32 = 90;

pub fn amendment_allowed(last_amend_day: Option<u32>, today: u32) -> Result<(), &'static str> {
    match last_amend_day {
        None => Ok(()),
        Some(d) if today >= d && today - d >= AMENDMENT_MIN_GAP => Ok(()),
        Some(d) if today < d => Err("修订日在过去之前：时间线倒置"),
        Some(_) => Err("距上次修订不足 90 天：频率闸拒绝"),
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F148F_TAG: &str = "stareco-F148-deep3";

pub fn run_f148_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F148F_TAG);

    // 分派
    let panel = [(0u8, 3u32, false), (1, 1, false), (2, 1, true), (3, 5, false)];
    set.add("f148f assign least busy", assign_panelist(&panel) == Ok(1), "最闲非涉事者当选");
    set.add("f148f tie first", assign_panelist(&[(0u8, 2u32, false), (1, 2, false)]) == Ok(0), "平局取序号小");
    set.add(
        "f148f all involved",
        assign_panelist(&[(0u8, 0u32, true)]).is_err(),
        "全员回避如实上报",
    );
    set.add("f148f empty pool", assign_panelist(&[]).is_err(), "空池拒绝");

    // 三人组
    let inv = [false, true, false, false];
    set.add("f148f panel ok", panel_valid(&[0, 2, 3], &inv), "三人互异非涉事");
    set.add("f148f panel dup", !panel_valid(&[0, 0, 3], &inv), "重复成员拒");
    set.add("f148f panel involved", !panel_valid(&[0, 1, 3], &inv), "涉事成员拒");
    set.add("f148f panel short", !panel_valid(&[0, 3], &inv), "人数不足拒");

    // 回避链
    let mut rc = RecusalChain::new();
    set.add("f148f recusal reason", rc.declare(7, 2, "").is_err(), "无理由拒绝");
    let _ = rc.declare(7, 2, "持有涉案主题股份");
    set.add("f148f recusal dup", rc.declare(7, 2, "再声明").is_err(), "重复声明拒绝");
    set.add("f148f recusal query", rc.is_recused(7, 2) && !rc.is_recused(7, 3), "按案件+人查询");
    let _ = rc.declare(8, 2, "曾参与评审");
    set.add("f148f recusal count", rc.len() == 2, "跨案件可再声明");

    // 公示期
    let n1 = PublicNotice { start_day: 100, objections: 0 };
    set.add("f148f effective", n1.effective_day() == Some(107), "7 天公示生效");
    let n2 = PublicNotice { start_day: 100, objections: 2 };
    set.add("f148f frozen", n2.effective_day().is_none() && n2.frozen(), "异议冻结不可预知");

    // 修订频率
    set.add("f148f amend first", amendment_allowed(None, 100).is_ok(), "首修放行");
    set.add("f148f amend ok", amendment_allowed(Some(1), 91).is_ok(), "90 天满放行");
    set.add("f148f amend soon", amendment_allowed(Some(10), 99).is_err(), "不足 90 拒");
    set.add("f148f amend invert", amendment_allowed(Some(200), 100).is_err(), "时间线倒置拒");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn assignment_skips_involved_even_if_free() {
        // 涉事者即使 load=0 也不得当选。
        let p = [(0u8, 0u32, true), (1, 4, false)];
        assert_eq!(assign_panelist(&p), Ok(1));
    }

    #[test]
    fn notice_boundary() {
        let n = PublicNotice { start_day: 0, objections: 0 };
        assert_eq!(n.effective_day(), Some(PUBLIC_NOTICE_DAYS));
        // 异议撤销后解冻（重新建模为新公示期——机器面不含自动解冻）。
        let n2 = PublicNotice { start_day: 50, objections: 0 };
        assert_eq!(n2.effective_day(), Some(57));
    }
}
