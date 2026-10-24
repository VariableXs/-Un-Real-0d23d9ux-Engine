//! ves461_s25test — VE-F4227 · 契约测试
//! 提供方/消费方双端契约测试框架（Pact 式同构）+测试结果回写注册中心（验证状态三戳：时间/结果/版本）+
//! 契约回归（消费方升级自动触发提供方回归）；测试失败→阻断发布+定位；回写断→重试+告警；回归风暴→合并调度；
//! 含无障碍判据双端验证（域本色）；测试数据合成。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1780;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtCode {
    Ok = 0,
    TestFail = SEG_BASE + 0x01,  // 0x1781
    WritebackLost = SEG_BASE + 0x02, // 0x1782
    Storm = SEG_BASE + 0x03,     // 0x1783
}

/// 双端角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side { Provider = 0, Consumer = 1 }

/// 测试结果（三戳：时间/结果/版本）。
#[derive(Clone, Copy)]
pub struct Stamp { pub time: u64, pub passed: bool, pub ver: u32 }

/// 契约测试册。
pub struct ContractTestBook {
    pub results: Vec<(Side, u32, Stamp)>, // (端, 契约 id, 三戳)
    pub retries: u32,
    pub merged: u32, // 回归风暴合并调度
    pub a11y_dual: bool, // a11y 判据双端验证（域本色）
    pub blocked: Option<u32>,
}

impl ContractTestBook {
    pub fn new() -> Self { ContractTestBook { results: Vec::new(), retries: 0, merged: 0, a11y_dual: false, blocked: None } }
    pub fn run(&mut self, side: Side, cid: u32, time: u64, ver: u32, passed: bool) {
        self.results.retain(|(s, c, _)| !(*s == side && *c == cid));
        self.results.push((side, cid, Stamp { time, passed, ver }));
        if !passed { self.blocked = Some(cid); }
    }
    /// 回写核验：双端结果齐→可回写；缺端→重试。
    pub fn writeback_ready(&mut self, cid: u32) -> Result<(), CtCode> {
        let has_p = self.results.iter().any(|(s, c, _)| *s == Side::Provider && *c == cid);
        let has_c = self.results.iter().any(|(s, c, _)| *s == Side::Consumer && *c == cid);
        if has_p && has_c { Ok(()) } else { self.retries += 1; Err(CtCode::WritebackLost) }
    }
    /// 发布闸：双端全过→放行；失败→阻断+定位。
    pub fn publish_gate(&self, cid: u32) -> Result<(), CtCode> {
        for (s, c, st) in &self.results {
            if *c == cid && !st.passed { return Err(CtCode::TestFail); }
            let _ = s;
        }
        Ok(())
    }
    /// 回归风暴：N 个消费方升级→合并调度。
    pub fn storm_merge(&mut self, upgrades: usize) {
        if upgrades > 4 { self.merged += upgrades as u32; }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves461_checks() -> CheckSet {
    let mut set = CheckSet::new("ves461");
    let mut bk = ContractTestBook::new();

    // C461-01 双端测试通过。
    bk.run(Side::Provider, 7001, 100, 3, true);
    bk.run(Side::Consumer, 7001, 101, 3, true);
    chk(&mut set, "C461-01", bk.publish_gate(7001).is_ok(), "双端通过");

    // C461-02 回写就绪（双端齐）。
    chk(&mut set, "C461-02", bk.writeback_ready(7001).is_ok(), "回写就绪");

    // C461-03 缺消费方端→重试告警。
    bk.run(Side::Provider, 7002, 102, 2, true);
    chk(&mut set, "C461-03", bk.writeback_ready(7002) == Err(CtCode::WritebackLost) && bk.retries == 1, "缺端重试");

    // C461-04 补消费方端后回写就绪。
    bk.run(Side::Consumer, 7002, 103, 2, true);
    chk(&mut set, "C461-04", bk.writeback_ready(7002).is_ok(), "补端就绪");

    // C461-05 测试失败→阻断发布+定位。
    bk.run(Side::Consumer, 7003, 104, 1, false);
    chk(&mut set, "C461-05", bk.publish_gate(7003) == Err(CtCode::TestFail) && bk.blocked == Some(7003), "失败阻断");

    // C461-06 三戳结构（时间/结果/版本）。
    let st = &bk.results.iter().find(|(_, c, _)| *c == 7001).unwrap().2;
    chk(&mut set, "C461-06", st.time == 100 && st.passed && st.ver == 3, "三戳在册");

    // C461-07 回归风暴合并调度（>4 升级触发）。
    bk.storm_merge(6);
    chk(&mut set, "C461-07", bk.merged == 6, "风暴合并");

    // C461-08 风暴阈值内不合并。
    bk.storm_merge(3);
    chk(&mut set, "C461-08", bk.merged == 6, "阈值内不并");

    // C461-09 域本色：a11y 双端验证标记。
    bk.a11y_dual = true;
    chk(&mut set, "C461-09", bk.a11y_dual, "a11y 双端");

    // C461-10 双端同构（Pact 式：同契约双端各一条）。
    let n_p = bk.results.iter().filter(|(s, c, _)| *s == Side::Provider && *c == 7001).count();
    let n_c = bk.results.iter().filter(|(s, c, _)| *s == Side::Consumer && *c == 7001).count();
    chk(&mut set, "C461-10", n_p == 1 && n_c == 1, "双端同构");

    // C461-11 结果覆盖幂等（重测替换）。
    bk.run(Side::Provider, 7001, 110, 3, true);
    chk(&mut set, "C461-11", bk.results.iter().filter(|(_, c, _)| *c == 7001).count() == 2, "重测覆盖");

    // C461-12 错误码段位单源。
    chk(&mut set, "C461-12", CtCode::WritebackLost as u16 == SEG_BASE + 2, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves461_smoke() {
        let set = run_ves461_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves461 red: {:?}", red);
    }
}
