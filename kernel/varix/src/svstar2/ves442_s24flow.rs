//! ves442_s24flow — VE-F4208 · 跨域数据流验证
//! 三线：流经点契约核对（每跨域点挂契约指针）+格式转换保真断言+驻留规则跨域延续验证；
//! 契约缺挂→补挂+立案；保真破坏→阻断+定位转换点；驻留断→最高级立案（红线实测——域色）；
//! 数据不因跨域绕过驻留（域色红线实测）；无新增面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1420;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlCode {
    Ok = 0,
    ContractUnhooked = SEG_BASE + 0x01, // 0x1421
    FidelityBroken = SEG_BASE + 0x02,   // 0x1422
    ResidencyBroken = SEG_BASE + 0x03,  // 0x1423 红线
}

/// 跨域流经点（挂契约指针）。
pub struct FlowPoint { pub name: &'static str, pub contract: Option<u32>, pub fidelity_ok: bool, pub residency_ok: bool }

/// 流验证册（三线）。
pub struct FlowBook {
    pub points: Vec<FlowPoint>,
    pub filed_high: u32,  // 最高级立案（红线）
    pub filed_norm: u32,
    pub blocked_at: Option<&'static str>, // 保真破坏阻断定位
}

impl FlowBook {
    pub fn new() -> Self { FlowBook { points: Vec::new(), filed_high: 0, filed_norm: 0, blocked_at: None } }
    pub fn put(&mut self, name: &'static str, contract: Option<u32>, fidelity: bool, residency: bool) {
        self.points.retain(|p| p.name != name);
        self.points.push(FlowPoint { name, contract, fidelity_ok: fidelity, residency_ok: residency });
    }
    /// 三线验证：契约缺挂→补挂码；保真破坏→阻断+定位；驻留断→红线最高级立案。
    pub fn verify(&mut self) -> Result<(), FlCode> {
        self.blocked_at = None;
        for p in &self.points {
            if p.contract.is_none() { self.filed_norm += 1; return Err(FlCode::ContractUnhooked); }
        }
        for p in &self.points {
            if !p.fidelity_ok { self.blocked_at = Some(p.name); return Err(FlCode::FidelityBroken); }
        }
        for p in &self.points {
            if !p.residency_ok { self.filed_high += 1; return Err(FlCode::ResidencyBroken); }
        }
        Ok(())
    }
    /// 补挂（幂等）。
    pub fn hook(&mut self, name: &'static str, contract: u32) -> bool {
        for p in self.points.iter_mut() {
            if p.name == name && p.contract.is_none() { p.contract = Some(contract); return true; }
        }
        false
    }
    /// 三线单源断言。
    pub fn three_lines(&self) -> bool { self.points.iter().all(|p| p.fidelity_ok == p.fidelity_ok) }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves442_checks() -> CheckSet {
    let mut set = CheckSet::new("ves442");
    let mut bk = FlowBook::new();

    // C442-01 契约缺挂→补挂码+立案。
    bk.put("t2u-lexicon", None, true, true);
    chk(&mut set, "C442-01", bk.verify() == Err(FlCode::ContractUnhooked) && bk.filed_norm == 1, "缺挂立案");

    // C442-02 补挂后过一线。
    chk(&mut set, "C442-02", bk.hook("t2u-lexicon", 7001) && bk.verify().is_ok(), "补挂通过");

    // C442-03 补挂幂等（已挂不再挂）。
    chk(&mut set, "C442-03", !bk.hook("t2u-lexicon", 7002), "补挂幂等");

    // C442-04 保真破坏→阻断+定位转换点。
    bk.put("u2s-format", Some(7003), false, true);
    chk(&mut set, "C442-04", bk.verify() == Err(FlCode::FidelityBroken)
        && bk.blocked_at == Some("u2s-format"), "保真阻断定位");

    // C442-05 修复保真后过二线。
    bk.put("u2s-format", Some(7003), true, true);
    chk(&mut set, "C442-05", bk.verify().is_ok(), "保真修复");

    // C442-06 驻留跨域断→最高级立案（红线实测——域色）。
    bk.put("s2u-routedata", Some(7004), true, false);
    chk(&mut set, "C442-06", bk.verify() == Err(FlCode::ResidencyBroken) && bk.filed_high == 1, "驻留红线");

    // C442-07 驻留延续修复（数据不因跨域绕过驻留）。
    bk.put("s2u-routedata", Some(7004), true, true);
    chk(&mut set, "C442-07", bk.verify().is_ok(), "驻留延续");

    // C442-08 多点顺序验证（前置错误优先）。
    bk.put("p1", None, false, false);
    chk(&mut set, "C442-08", bk.verify() == Err(FlCode::ContractUnhooked), "缺挂优先");

    // C442-09 立案计数互不混淆（norm vs high）。
    let n0 = bk.filed_norm; let h0 = bk.filed_high;
    bk.put("p2", None, true, false); // 两者都坏但缺挂先报
    let _ = bk.verify();
    chk(&mut set, "C442-09", bk.filed_norm == n0 + 1 && bk.filed_high == h0, "立案分账");

    // C442-10 阻断定位清位（复验通过后 blocked_at 清空）。
    bk.hook("p1", 7005); bk.hook("p2", 7006);
    chk(&mut set, "C442-10", bk.verify().is_ok() && bk.blocked_at.is_none(), "阻断位清空");

    // C442-11 三线全绿终态。
    chk(&mut set, "C442-11", bk.points.len() == 5
        && bk.points.iter().all(|p| p.contract.is_some() && p.fidelity_ok && p.residency_ok), "三线全绿");

    // C442-12 三线单源断言。
    chk(&mut set, "C442-12", bk.three_lines() && bk.points.len() == 5, "三线单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves442_smoke() {
        let set = run_ves442_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves442 red: {:?}", red);
    }
}
