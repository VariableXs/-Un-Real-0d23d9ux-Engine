//! VE-F4282 · 一致性×CI 深化（U05 工具集成组）
//! 职责：CI 三阶段闸（提交/合并/发布）+ 增量扫描（只扫变更影响面——复用
//! U02 影响分析）+ 结果回写三型（检查态/注解/阻断——注解型不打断节奏）
//! + 扫描超时分片降级仅红线 + 回写失败重试三次入死信。
//! 段位：0x29A0（续表 idx516）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x29A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ci2Code {
    /// 扫描超时（降级仅红线）。
    ScanOver = SEG_BASE,
    /// 基线漂移（对拍后重扫）。
    BaselineDrift = SEG_BASE + 0x01,
    /// 回写死信。
    DeadLetter = SEG_BASE + 0x02,
}

/// 三阶段闸。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Stage {
    Commit = 0,
    Merge = 1,
    Release = 2,
}
pub const STAGE_N: usize = 3;

/// 回写型三。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WbKind {
    Status = 0,   // 检查态
    Annotation = 1, // 注解（不打断）
    Block = 2,    // 阻断
}

/// CI 深化器。
pub struct CiDeep {
    /// 各阶段闸开。
    pub gates: [bool; STAGE_N],
    /// 影响面（U02 影响分析结果）。
    pub impact_scope: Vec<u16>,
    /// 回写账：(目标, 型, 重试)。
    pub writebacks: Vec<(u16, WbKind, u8)>,
    /// 死信。
    pub dead: Vec<u16>,
    /// 红线规则集（降级扫描仅红线）。
    pub redlines: Vec<u16>,
    pub degraded: bool,
}

impl CiDeep {
    pub fn new(redlines: Vec<u16>) -> Self {
        CiDeep { gates: [true; STAGE_N], impact_scope: Vec::new(), writebacks: Vec::new(), dead: Vec::new(), redlines, degraded: false }
    }

    /// 影响面导入（增量扫描计划）。
    pub fn set_impact(&mut self, scope: Vec<u16>) {
        self.impact_scope = scope;
        self.degraded = false;
    }

    /// 扫描：正常=影响面全量；超时降级=仅红线∩影响面。
    pub fn scan(&mut self, timeout: bool) -> Vec<u16> {
        if timeout {
            self.degraded = true;
            return self.impact_scope.iter().filter(|r| self.redlines.contains(r)).copied().collect();
        }
        self.impact_scope.clone()
    }

    /// 回写（注解型不打断开发节奏——语义在消费侧）。
    pub fn writeback(&mut self, target: u16, kind: WbKind, ok: bool) -> Result<(), Ci2Code> {
        if ok {
            self.writebacks.push((target, kind, 0));
            return Ok(());
        }
        // 重试三次后入死信
        if let Some(e) = self.writebacks.iter_mut().find(|(t, k, _)| *t == target && *k == kind) {
            e.2 += 1;
            if e.2 >= 3 {
                self.dead.push(target);
                self.writebacks.retain(|(t, k, _)| !(*t == target && *k == kind));
                return Err(Ci2Code::DeadLetter);
            }
        } else {
            self.writebacks.push((target, kind, 1));
        }
        Ok(())
    }

    /// 阶段闸放行：发布闸必须非降级。
    pub fn gate_ok(&self, s: Stage) -> bool {
        self.gates[s as usize] && !(s == Stage::Release && self.degraded)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C516-01..12。
pub fn run_ves516_checks() -> CheckSet {
    let mut set = CheckSet::new("ves516");

    let mut ci = CiDeep::new(alloc::vec![10, 20]);
    ci.set_impact(alloc::vec![10, 20, 30, 40]);
    chk(&mut set, "C516-01", ci.scan(false).len() == 4, "增量扫描影响面全量 4");
    chk(&mut set, "C516-02", ci.scan(true) == alloc::vec![10, 20], "超时降级仅红线∩影响面");
    chk(&mut set, "C516-03", ci.degraded, "降级标记在册");
    chk(&mut set, "C516-04", !ci.gate_ok(Stage::Release), "降级状态发布闸拒绝");
    chk(&mut set, "C516-05", ci.gate_ok(Stage::Commit), "提交闸照常");
    ci.set_impact(alloc::vec![10, 20, 30, 40]);
    chk(&mut set, "C516-06", !ci.degraded && ci.gate_ok(Stage::Release), "重扫后降级解除");

    // 三型回写
    let _ = ci.writeback(100, WbKind::Status, true);
    let _ = ci.writeback(101, WbKind::Annotation, true);
    let _ = ci.writeback(102, WbKind::Block, true);
    chk(&mut set, "C516-07", ci.writebacks.len() == 3, "三型回写在册");

    // 回写失败重试三次入死信
    for _ in 0..2 {
        let _ = ci.writeback(100, WbKind::Status, false);
    }
    chk(&mut set, "C516-08", ci.writebacks.iter().find(|(t, _, _)| *t == 100).map(|e| e.2) == Some(2), "重试计数 2");
    chk(&mut set, "C516-09", ci.writeback(100, WbKind::Status, false) == Err(Ci2Code::DeadLetter), "第三次失败入死信");
    chk(&mut set, "C516-10", ci.dead == alloc::vec![100], "死信账在册");
    chk(&mut set, "C516-11", !ci.writebacks.iter().any(|(t, _, _)| *t == 100), "死信移出活动回写");

    chk(&mut set, "C516-12", STAGE_N == 3 && WbKind::Block as u8 == 2, "三阶段闸与三型枚举完备");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_deep_scan() {
        let mut ci = CiDeep::new(alloc::vec![1]);
        ci.set_impact(alloc::vec![1, 2]);
        assert_eq!(ci.scan(true), alloc::vec![1]);
        assert!(ci.degraded);
        assert!(!ci.gate_ok(Stage::Release));
    }
}
