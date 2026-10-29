//! UNX-F46412 版本跟随联动评估接口（AI-94 评估触发 → S1/S2 最小自举回归 7 日 SLA）（0x400B0 段）
//!
//! 锚点：UNX-F46412-J1 触发→回归→结论链路演练 ×2 全通且 ≤7 日。
//! 域：CoRun·UNX-S1-B01（工具链域·跟随 SLA 面）。
//!
//! 结构：跟随触发账（上游版本事件 → 评估单落账）+ 回归执行账（S1/S2
//! 双域最小自举，日序推进）+ SLA 核（触发日 → 结论日 ≤ 7 日）+
//! 结论三态（跟进/豁免/升级）+ 注入核（空事件/超 SLA/结论越闭集/
//! 重复评估单/未回归先结论必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x400B0 独占 0x10）。
pub const SEG_BASE: u32 = 0x400B0;
pub const FW_EVT_EMPTY: u32 = SEG_BASE + 0x01; // 触发事件为空
pub const FW_DUP_CASE: u32 = SEG_BASE + 0x02; // 重复评估单
pub const FW_SLA_BREACH: u32 = SEG_BASE + 0x03; // SLA 越限（>7 日）
pub const FW_VERDICT_OOB: u32 = SEG_BASE + 0x04; // 结论越闭集
pub const FW_REGRESS_MISS: u32 = SEG_BASE + 0x05; // 未回归先结论

/// SLA 常量：7 日。
pub const SLA_DAYS: u32 = 7;

/// 结论三态闭集。
pub const V_FOLLOW: u8 = 0; // 跟进（升级钉版）
pub const V_WAIVE: u8 = 1; // 豁免（评估后不跟随）
pub const V_ESCALATE: u8 = 2; // 升级（进 AI-79 流水线）
pub const V_COUNT: usize = 3;

pub const VERDICT_NAMES: [&str; V_COUNT] = ["跟进", "豁免", "升级"];

/// 评估单：触发 → 回归日序 → 结论。
pub struct FollowCase {
    pub id: u32,
    pub event: String,
    pub trigger_day: u32,
    /// 双域回归落日（0 = 未回归）。
    pub regress_day_s1: u32,
    pub regress_day_s2: u32,
    pub verdict: Option<u8>,
    pub verdict_day: u32,
}

impl FollowCase {
    /// 评估单构造（事件非空 + 触发日 ≥1）。
    pub fn new(id: u32, event: &str, trigger_day: u32) -> Result<Self, u32> {
        if event.is_empty() {
            return Err(FW_EVT_EMPTY);
        }
        if trigger_day == 0 {
            return Err(FW_EVT_EMPTY);
        }
        Ok(FollowCase {
            id,
            event: event.to_string(),
            trigger_day,
            regress_day_s1: 0,
            regress_day_s2: 0,
            verdict: None,
            verdict_day: 0,
        })
    }

    /// 双域回归落日登记（日序必须 ≥ 触发日）。
    pub fn mark_regress(&mut self, which: usize, day: u32) -> Result<(), u32> {
        if day < self.trigger_day {
            return Err(FW_REGRESS_MISS);
        }
        match which {
            0 => self.regress_day_s1 = day,
            1 => self.regress_day_s2 = day,
            _ => return Err(FW_VERDICT_OOB),
        }
        Ok(())
    }

    /// 双域回归完成核。
    pub fn regressed(&self) -> bool {
        self.regress_day_s1 > 0 && self.regress_day_s2 > 0
    }

    /// 结论落账（未回归先结论拒绝；结论越闭集拒绝；SLA 越限拒绝）。
    pub fn conclude(&mut self, v: u8, day: u32) -> Result<(), u32> {
        if v as usize >= V_COUNT {
            return Err(FW_VERDICT_OOB);
        }
        if !self.regressed() {
            return Err(FW_REGRESS_MISS);
        }
        if day.saturating_sub(self.trigger_day) > SLA_DAYS {
            return Err(FW_SLA_BREACH);
        }
        self.verdict = Some(v);
        self.verdict_day = day;
        Ok(())
    }

    /// SLA 余量（结论日 - 触发日；未结论返回 None）。
    pub fn sla_used(&self) -> Option<u32> {
        if self.verdict.is_some() {
            Some(self.verdict_day - self.trigger_day)
        } else {
            None
        }
    }
}

/// 跟随账（评估单登记册，单号唯一）。
pub struct FollowBook {
    pub cases: Vec<FollowCase>,
}

impl FollowBook {
    pub fn new() -> Self {
        FollowBook { cases: Vec::new() }
    }

    /// 触发落单（单号重复拒绝）。
    pub fn trigger(&mut self, id: u32, event: &str, day: u32) -> Result<(), u32> {
        if self.cases.iter().any(|c| c.id == id) {
            return Err(FW_DUP_CASE);
        }
        let c = FollowCase::new(id, event, day)?;
        self.cases.push(c);
        Ok(())
    }

    /// 单查询。
    pub fn case_mut(&mut self, id: u32) -> Option<&mut FollowCase> {
        self.cases.iter_mut().find(|c| c.id == id)
    }

    /// J1 全链核：全部评估单已结论、双域已回归、SLA 全部 ≤7 日。
    pub fn all_within_sla(&self) -> bool {
        !self.cases.is_empty()
            && self.cases.iter().all(|c| c.verdict.is_some() && c.regressed() && c.sla_used().unwrap_or(u32::MAX) <= SLA_DAYS)
    }
}

/// 标准夹具：两次链路演练（评估单 1/2，触发日 1，回归日 3，结论日 4）。
pub fn standard_book() -> FollowBook {
    let mut b = FollowBook::new();
    for id in [1u32, 2u32] {
        b.trigger(id, "上游 glibc 2.39 发布", 1).expect("fixture trigger");
        let c = b.case_mut(id).expect("fixture case");
        c.mark_regress(0, 3).expect("s1 regress");
        c.mark_regress(1, 3).expect("s2 regress");
        c.conclude(V_FOLLOW, 4).expect("conclude");
    }
    b
}

/// 注入核：6 注入必抓（空事件/重复单/超 SLA/结论越闭集/未回归先结论/越域回归位）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空事件必抓
    if FollowCase::new(1, "", 1).err() == Some(FW_EVT_EMPTY) {
        caught += 1;
    }
    // 2) 重复单必抓
    let mut b = FollowBook::new();
    b.trigger(1, "事件 A", 1).expect("first");
    if b.trigger(1, "事件 A 重放", 2) == Err(FW_DUP_CASE) {
        caught += 1;
    }
    // 3) 超 SLA 必抓（回归日 5，结论日 12 → 11 日 > 7）
    let mut b2 = FollowBook::new();
    b2.trigger(2, "事件 B", 1).expect("t2");
    {
        let c = b2.case_mut(2).expect("c2");
        c.mark_regress(0, 5).expect("r0");
        c.mark_regress(1, 5).expect("r1");
    }
    let mut c2 = b2.case_mut(2).expect("m2");
    if c2.conclude(V_WAIVE, 12) == Err(FW_SLA_BREACH) {
        caught += 1;
    }
    // 4) 结论越闭集必抓
    if c2.conclude(9, 6) == Err(FW_VERDICT_OOB) {
        caught += 1;
    }
    // 5) 未回归先结论必抓
    let mut b3 = FollowBook::new();
    b3.trigger(3, "事件 C", 1).expect("t3");
    let mut c3 = b3.case_mut(3).expect("m3");
    if c3.conclude(V_FOLLOW, 2) == Err(FW_REGRESS_MISS) {
        caught += 1;
    }
    // 6) 回归日早于触发日必抓
    let mut b4 = FollowBook::new();
    b4.trigger(4, "事件 D", 5).expect("t4");
    let mut c4 = b4.case_mut(4).expect("m4");
    if c4.mark_regress(0, 2) == Err(FW_REGRESS_MISS) {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口（CheckSet）。
pub fn run_unx46412_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F46412");
    // J1-1 链路演练 ×2 全通（触发→回归→结论）
    let b = standard_book();
    let ok = b.all_within_sla() && b.cases.len() == 2
        && b.cases.iter().all(|c| c.verdict == Some(V_FOLLOW));
    s.add("46412-J1-链路演练×2", ok, if ok { "" } else { "链路漂移" });
    // J1-2 SLA ≤7 日（两单都用 3 日）
    let ok = b.cases.iter().all(|c| c.sla_used() == Some(3));
    s.add("46412-J1-SLA七日", ok, if ok { "" } else { "SLA 漂移" });
    // J1-3 结论三态闭集渲染
    let ok = VERDICT_NAMES.len() == 3 && VERDICT_NAMES[V_FOLLOW as usize] == "跟进";
    s.add("46412-J1-结论闭集", ok, if ok { "" } else { "结论闭集漂移" });
    // 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("46412-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx46412_all_checks_green() {
        let set = run_unx46412_checks();
        assert!(set.all_passed(), "UNX-F46412 checks must be green: {}", set.red_items_text());
    }
}
