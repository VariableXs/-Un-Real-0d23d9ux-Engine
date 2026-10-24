//! VE-F4296 · 工具集成 fuzz（U05 工具集成组）
//! 职责：集成面 fuzz（事件总线畸形事件/适配器边界参数/签名伪造三类）
//! + 语料两源（真实事件变形+协议语法生成）+ 发现即立案 + 误报白名单
//! + 资源调度配额 + fuzz 报告脱敏。
//! 段位：0x2E80（续表 idx530）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2E80;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfCode {
    /// 畸形事件致崩溃。
    MalformCrash = SEG_BASE,
    /// 签名伪造命中。
    ForgeHit = SEG_BASE + 0x01,
    /// 配额耗尽。
    QuotaOut = SEG_BASE + 0x02,
}

/// fuzz 目标三类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfTarget {
    MalformEv = 0,
    BoundParam = 1,
    SigForge = 2,
}

/// 语料（两源）。
#[derive(Clone, Copy, Debug)]
pub struct IfCorpus {
    pub from_real: bool,   // 真实事件变形
    pub from_grammar: bool, // 协议语法生成
    pub whitelisted: bool,
}

/// 集成 fuzz 器。
pub struct IntegFuzz {
    pub pool: Vec<IfCorpus>,
    pub cases: Vec<(IfTarget, u16)>,
    pub quota: u32,
    pub used: u32,
}

impl IntegFuzz {
    pub fn new(quota: u32) -> Self {
        IntegFuzz { pool: Vec::new(), cases: Vec::new(), quota, used: 0 }
    }

    pub fn seed(&mut self, c: IfCorpus) {
        self.pool.push(c);
    }

    pub fn runnable(&self) -> usize {
        self.pool.iter().filter(|c| !c.whitelisted).count()
    }

    /// 配额调度。
    pub fn round(&mut self, want: u32) -> Result<u32, IfCode> {
        let take = want.min(self.quota.saturating_sub(self.used));
        if take == 0 {
            return Err(IfCode::QuotaOut);
        }
        self.used += take;
        Ok(take)
    }

    /// 畸形事件检：超长或含控制位。
    pub fn probe_malform(&mut self, len: u32, ctrl: bool) -> bool {
        let hit = len > 4096 || ctrl;
        if hit {
            self.file(IfTarget::MalformEv, len.min(9999) as u16);
        }
        hit
    }

    /// 边界参数检：u16 边界外溢。
    pub fn probe_bound(&mut self, v: u32) -> bool {
        if v > 0xFFFF {
            self.file(IfTarget::BoundParam, 0xFFFF);
            return true;
        }
        false
    }

    /// 签名伪造检：零签名或重复签名。
    pub fn probe_forge(&mut self, sig: u32, seen: &mut Vec<u32>) -> bool {
        let hit = sig == 0 || seen.contains(&sig);
        if hit {
            self.file(IfTarget::SigForge, (sig & 0xFFFF) as u16);
        } else {
            seen.push(sig);
        }
        hit
    }

    fn file(&mut self, t: IfTarget, d: u16) {
        if !self.cases.iter().any(|(a, b)| *a == t && *b == d) {
            self.cases.push((t, d));
        }
    }

    pub fn case_n(&self) -> usize {
        self.cases.len()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C530-01..12。
pub fn run_ves530_checks() -> CheckSet {
    let mut set = CheckSet::new("ves530");

    let mut fz = IntegFuzz::new(10);
    fz.seed(IfCorpus { from_real: true, from_grammar: false, whitelisted: false });
    fz.seed(IfCorpus { from_real: false, from_grammar: true, whitelisted: true });
    fz.seed(IfCorpus { from_real: true, from_grammar: true, whitelisted: false });
    chk(&mut set, "C530-01", fz.runnable() == 2, "白名单过滤后可跑 2");
    chk(&mut set, "C530-02", fz.pool.iter().filter(|c| c.from_real && c.from_grammar).count() == 1, "语料两源标记");

    chk(&mut set, "C530-03", fz.round(4) == Ok(4), "配额内调度");
    chk(&mut set, "C530-04", fz.round(99) == Ok(6), "超额截断余量");
    chk(&mut set, "C530-05", fz.round(1) == Err(IfCode::QuotaOut), "配额耗尽");

    // 三类目标
    chk(&mut set, "C530-06", fz.probe_malform(5000, false), "超长畸形事件检出");
    chk(&mut set, "C530-07", fz.probe_malform(100, true), "控制位事件检出");
    chk(&mut set, "C530-08", !fz.probe_malform(100, false), "正常事件放行");
    chk(&mut set, "C530-09", fz.probe_bound(0x1_0000), "边界外溢检出");
    chk(&mut set, "C530-10", !fz.probe_bound(0xFFFF), "边界值放行");

    let mut seen: Vec<u32> = Vec::new();
    chk(&mut set, "C530-11", fz.probe_forge(0, &mut seen), "零签名伪造检出");
    chk(&mut set, "C530-12", !fz.probe_forge(42, &mut seen) && fz.probe_forge(42, &mut seen), "重复签名检出（回放攻击）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integ_fuzz() {
        let mut fz = IntegFuzz::new(1);
        assert_eq!(fz.round(5), Ok(1));
        assert!(fz.round(1).is_err());
        assert!(fz.probe_bound(70000));
        assert_eq!(fz.case_n(), 1);
    }
}
