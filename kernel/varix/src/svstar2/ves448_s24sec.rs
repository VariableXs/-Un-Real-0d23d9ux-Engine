//! ves448_s24sec — VE-F4214 · 一致性安全
//! 三道：契约中心防护（篡改检测+签名）+规则注入面审查（外部规则源白名单）+图谱投毒防护（变更双人审）；
//! 签名错→拒载+立案；白名单外规则→拒绝执行；可疑变更→冻结待审；
//! 规则注入审查含无障碍规则防降级（不得注入弱化无障碍的规则——域本色红线）；无新增面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x14E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    Ok = 0,
    SigWrong = SEG_BASE + 0x01,   // 0x14E1
    NotWhitelisted = SEG_BASE + 0x02, // 0x14E2
    PoisonSuspect = SEG_BASE + 0x03,  // 0x14E3
    A11yWeaken = SEG_BASE + 0x04,     // 0x14E4 防降级红线
}

/// FNV-1a 签名。
pub fn fnv(data: &[u32]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for d in data { h ^= *d; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 契约中心防护。
pub struct ContractGuard { pub payload: Vec<u32>, pub sig: u32, pub loaded: bool }

impl ContractGuard {
    pub fn new(payload: Vec<u32>) -> Self {
        let sig = fnv(&payload);
        ContractGuard { payload, sig, loaded: false }
    }
    /// 验签：通过→载入；错→拒载+立案。
    pub fn load(&mut self) -> Result<(), ScCode> {
        if fnv(&self.payload) != self.sig {
            return Err(ScCode::SigWrong);
        }
        self.loaded = true;
        Ok(())
    }
    pub fn tamper(&mut self, idx: usize, v: u32) { if idx < self.payload.len() { self.payload[idx] = v; } }
}

/// 外部规则源白名单审查（含 a11y 防降级——域本色红线）。
pub struct RuleInjector {
    pub whitelist: Vec<&'static str>,
    pub rejected: u32,
    pub filed: u32,
}

impl RuleInjector {
    pub fn new(wl: Vec<&'static str>) -> Self { RuleInjector { whitelist: wl, rejected: 0, filed: 0 } }
    /// 规则源审查：白名单外→拒绝执行；弱化 a11y 的规则→红线拒。
    pub fn review(&mut self, src: &'static str, weakens_a11y: bool) -> Result<(), ScCode> {
        if weakens_a11y { self.filed += 1; return Err(ScCode::A11yWeaken); }
        if !self.whitelist.contains(&src) { self.rejected += 1; return Err(ScCode::NotWhitelisted); }
        Ok(())
    }
}

/// 图谱变更双人审（投毒防护）。
pub struct DualReview { pub reviewers: [&'static str; 2], pub frozen: bool }

impl DualReview {
    pub fn review(&mut self, a: &'static str, b: &'static str) -> Result<(), ScCode> {
        if a == b { self.frozen = true; return Err(ScCode::PoisonSuspect); }
        self.reviewers = [a, b];
        self.frozen = false;
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves448_checks() -> CheckSet {
    let mut set = CheckSet::new("ves448");

    // C448-01 签名一致→载入。
    let mut g = ContractGuard::new(vec![10, 20, 30]);
    chk(&mut set, "C448-01", g.load().is_ok() && g.loaded, "验签载入");

    // C448-02 篡改检出→拒载。
    g.tamper(1, 99);
    chk(&mut set, "C448-02", g.load() == Err(ScCode::SigWrong), "篡改拒载");

    // C448-03 白名单内规则源→放行。
    let mut inj = RuleInjector::new(vec!["core-a11y", "family-lex"]);
    chk(&mut set, "C448-03", inj.review("core-a11y", false).is_ok(), "白名单放行");

    // C448-04 白名单外→拒绝执行+计数。
    chk(&mut set, "C448-04", inj.review("rogue-src", false) == Err(ScCode::NotWhitelisted)
        && inj.rejected == 1, "白名单外拒");

    // C448-05 域本色红线：弱化无障碍规则→防降级拒（优先于白名单）。
    chk(&mut set, "C448-05", inj.review("core-a11y", true) == Err(ScCode::A11yWeaken)
        && inj.filed == 1, "防降级红线");

    // C448-06 双人审：同人→冻结待审。
    let mut dr = DualReview { reviewers: ["", ""], frozen: false };
    chk(&mut set, "C448-06", dr.review("wb-a", "wb-a") == Err(ScCode::PoisonSuspect) && dr.frozen, "同人冻结");

    // C448-07 双人异签→过审解冻。
    chk(&mut set, "C448-07", dr.review("wb-a", "wb-b").is_ok() && !dr.frozen, "双签过审");

    // C448-08 签名可复现。
    chk(&mut set, "C448-08", fnv(&[1, 2]) == fnv(&[1, 2]) && fnv(&[1, 2]) != fnv(&[2, 1]), "签名可复现");

    // C448-09 空载荷签名非零。
    chk(&mut set, "C448-09", fnv(&[]) == 0x811C_9DC5, "FNV 偏移基");

    // C448-10 拒载后载入位保持 false。
    chk(&mut set, "C448-10", !g.loaded, "拒载不置位");

    // C448-11 白名单审查幂等。
    inj.review("family-lex", false).unwrap();
    chk(&mut set, "C448-11", inj.rejected == 1 && inj.filed == 1, "计数稳定");

    // C448-12 三道单源断言。
    chk(&mut set, "C448-12", ScCode::SigWrong as u16 == SEG_BASE + 1
        && ScCode::A11yWeaken as u16 == SEG_BASE + 4, "三道单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves448_smoke() {
        let set = run_ves448_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves448 red: {:?}", red);
    }
}
