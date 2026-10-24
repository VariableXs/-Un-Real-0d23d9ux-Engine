//! ves475_s26arch — VE-F4241 · 验证平台总架构
//! U03 定位：六类一致性验证统一平台（静态/动态/视觉/交互/性能/无障碍）；
//! 架构声明：规则引擎（F4204）驱动+契约（F4203）为判据源——平台不自带规则（单源红线）；
//! 四段流水线（调度→执行→归一→报告）；段间失配→对拍；平台私规检出→移除+立案；调度漂移→对版；
//! 平台含无障碍一致性验证专线（永不跳过——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x19E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArCode {
    Ok = 0,
    StageMismatch = SEG_BASE + 0x01, // 0x19E1
    PrivateRule = SEG_BASE + 0x02,   // 0x19E2 单源红线
    SchedDrift = SEG_BASE + 0x03,    // 0x19E3
}

/// 四段流水线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage { Sched = 0, Execute = 1, Normalize = 2, Report = 3 }

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Sched, Stage::Execute, Stage::Normalize, Stage::Report];
}

/// 六类验证。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyKind { Static = 0, Dynamic = 1, Visual = 2, Interact = 3, Perf = 4, A11y = 5 }

impl VerifyKind {
    pub const ALL: [VerifyKind; 6] = [
        VerifyKind::Static, VerifyKind::Dynamic, VerifyKind::Visual,
        VerifyKind::Interact, VerifyKind::Perf, VerifyKind::A11y];
}

/// 平台架构册。
pub struct PlatformBook {
    pub stages: Vec<Stage>,
    pub kinds: Vec<VerifyKind>,     // 已接入验证类
    pub rules: Vec<(u32, bool)>,    // 平台内规则 (id, 私规标记)
    pub engine_rev: u64,            // 规则引擎版本（调度基准）
    pub sched_rev: u64,             // 调度使用版本
    pub a11y_always: bool,          // a11y 专线永不跳过（域本色）
}

impl PlatformBook {
    pub fn new() -> Self { PlatformBook { stages: Vec::new(), kinds: Vec::new(), rules: Vec::new(), engine_rev: 0, sched_rev: 0, a11y_always: false } }
    pub fn advance(&mut self, s: Stage) -> Result<(), ArCode> {
        if s as usize != self.stages.len() { return Err(ArCode::StageMismatch); }
        self.stages.push(s);
        Ok(())
    }
    pub fn register_kind(&mut self, k: VerifyKind) { if !self.kinds.contains(&k) { self.kinds.push(k); } }
    pub fn add_rule(&mut self, id: u32, private: bool) { self.rules.push((id, private)); }
    /// 单源红线：平台私规检出→移除+立案。
    pub fn single_source_audit(&mut self) -> Result<(), ArCode> {
        let before = self.rules.len();
        self.rules.retain(|(_, p)| !p);
        if self.rules.len() != before {
            return Err(ArCode::PrivateRule); // 已移除
        }
        Ok(())
    }
    /// 调度对版：引擎版本前移→漂移检出→对齐。
    pub fn sched_check(&mut self, engine_rev: u64) -> Result<(), ArCode> {
        self.engine_rev = engine_rev;
        if self.sched_rev != engine_rev {
            self.sched_rev = engine_rev;
            return Err(ArCode::SchedDrift);
        }
        Ok(())
    }
    /// 六类统一+四段齐+单源+专线（域本色）→就绪。
    pub fn ready(&self) -> bool {
        self.stages.len() == 4 && self.kinds.len() == 6
            && self.single_source_ok() && self.a11y_always
    }
    fn single_source_ok(&self) -> bool { self.rules.iter().all(|(_, p)| !p) }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves475_checks() -> CheckSet {
    let mut set = CheckSet::new("ves475");
    let mut bk = PlatformBook::new();

    // C475-01 越段→失配。
    chk(&mut set, "C475-01", bk.advance(Stage::Execute) == Err(ArCode::StageMismatch), "越段失配");

    // C475-02 四段贯通。
    for s in Stage::ALL { bk.advance(s).unwrap(); }
    chk(&mut set, "C475-02", bk.stages.len() == 4, "四段贯通");

    // C475-03 六类接入。
    for k in VerifyKind::ALL { bk.register_kind(k); }
    chk(&mut set, "C475-03", bk.kinds.len() == 6, "六类统一");

    // C475-04 单源红线：私规检出→移除+码。
    bk.add_rule(101, false);
    bk.add_rule(102, true); // 私规
    chk(&mut set, "C475-04", bk.single_source_audit() == Err(ArCode::PrivateRule)
        && bk.rules.len() == 1, "私规移除");

    // C475-05 复检单源过。
    chk(&mut set, "C475-05", bk.single_source_audit().is_ok(), "单源复检");

    // C475-06 调度漂移检出→对版。
    bk.sched_rev = 1;
    chk(&mut set, "C475-06", bk.sched_check(2) == Err(ArCode::SchedDrift) && bk.sched_rev == 2, "漂移对版");

    // C475-07 再核过。
    chk(&mut set, "C475-07", bk.sched_check(2).is_ok(), "对版后过");

    // C475-08 a11y 专线缺→未就绪（域本色）。
    chk(&mut set, "C475-08", !bk.ready(), "专线缺");

    // C475-09 专线置位→就绪。
    bk.a11y_always = true;
    chk(&mut set, "C475-09", bk.ready(), "平台就绪");

    // C475-10 六类单源断言（a11y 永在）。
    chk(&mut set, "C475-10", VerifyKind::ALL.len() == 6
        && VerifyKind::ALL[5] == VerifyKind::A11y, "六类单源");

    // C475-11 类登记幂等。
    bk.register_kind(VerifyKind::A11y);
    chk(&mut set, "C475-11", bk.kinds.len() == 6, "类幂等");

    // C475-12 错误码段位单源。
    chk(&mut set, "C475-12", ArCode::PrivateRule as u16 == SEG_BASE + 2, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves475_smoke() {
        let set = run_ves475_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves475 red: {:?}", red);
    }
}
