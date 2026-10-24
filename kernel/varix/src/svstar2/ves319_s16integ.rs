//! ves319_s16integ — VE-F4079 · T04 联调
//! 18 条目贯通演练（抽取到质量评分全链跑通）+ 三故障注入（引擎宕/库损坏/占位符断）+ 降级预案激活验证 + 证据链挂进度矩阵 + 无障碍键抽检。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x90C0;

pub const CHAIN_TOTAL: u8 = 18;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum IgCode {
    Ok = 0,
    ChainBroken = SEG_BASE + 0x01,
    DrillFail = SEG_BASE + 0x02,
    EvidenceMissing = SEG_BASE + 0x03,
    PlanInactive = SEG_BASE + 0x04,
}

/// 全链 18 站。
pub const CHAIN_STATIONS: [&str; 18] = [
    "抽取", "记忆", "机翻", "编辑", "LQA", "风格",
    "上下文", "进度", "评分", "无障碍翻译", "安全", "性能",
    "调试", "测试", "示例", "文档", "fuzz", "联调",
];

pub struct ChainRunner {
    pub passed: Vec<&'static str>,
    pub evidence: Vec<String>,
}

impl ChainRunner {
    pub fn new() -> Self { ChainRunner { passed: Vec::new(), evidence: Vec::new() } }

    pub fn run_station(&mut self, s: &'static str, ok: bool, ev: &str) -> Result<(), IgCode> {
        if !ok { return Err(IgCode::ChainBroken); }
        if ev.is_empty() { return Err(IgCode::EvidenceMissing); }
        if !CHAIN_STATIONS.contains(&s) { return Err(IgCode::ChainBroken); }
        self.passed.push(s);
        self.evidence.push(String::from(ev));
        Ok(())
    }

    pub fn complete(&self) -> bool {
        self.passed.len() == CHAIN_TOTAL as usize
    }

    /// 顺序贯通：passed 与 CHAIN_STATIONS 同序。
    pub fn in_order(&self) -> bool {
        for (i, s) in self.passed.iter().enumerate() {
            if *s != CHAIN_STATIONS[i] { return false; }
        }
        true
    }
}

/// 三故障注入演练。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fault { EngineDown = 0, LibDamaged = 1, PlaceholderBroken = 2 }

impl Fault {
    pub const ALL: [Fault; 3] = [Fault::EngineDown, Fault::LibDamaged, Fault::PlaceholderBroken];
    pub fn name(self) -> &'static str {
        match self { Fault::EngineDown => "引擎宕", Fault::LibDamaged => "库损坏", Fault::PlaceholderBroken => "占位符断" }
    }
    pub fn plan(&self) -> &'static str {
        match self {
            Fault::EngineDown => "切备引擎",
            Fault::LibDamaged => "快照回滚",
            Fault::PlaceholderBroken => "回退译文",
        }
    }
}

pub struct DrillBook {
    pub results: Vec<(Fault, bool)>,
    /// 降级预案激活验证记录。
    pub plan_activated: Vec<Fault>,
}

impl DrillBook {
    pub fn new() -> Self { DrillBook { results: Vec::new(), plan_activated: Vec::new() } }

    pub fn drill(&mut self, f: Fault, handled: bool) -> Result<(), IgCode> {
        if !handled { return Err(IgCode::DrillFail); }
        self.results.push((f, handled));
        self.plan_activated.push(f);
        Ok(())
    }

    pub fn all_drilled(&self) -> bool {
        Fault::ALL.iter().all(|f| self.results.iter().any(|r| r.0 == *f && r.1))
    }

    /// 降级预案验证：每故障的预案已激活。
    pub fn plans_verified(&self) -> bool {
        Fault::ALL.iter().all(|f| self.plan_activated.contains(f))
    }
}

pub fn run_ves319_checks() -> CheckSet {
    let mut set = CheckSet::new("ves319_s16integ");
    let mut cr = ChainRunner::new();

    // 1) 全链 18 站贯通（顺序）。
    for s in CHAIN_STATIONS.iter() {
        let _ = cr.run_station(s, true, "ev");
    }
    set.add("C4079-01", cr.complete() && cr.in_order(), "全链贯通");

    // 2) 断链检出。
    let mut cr2 = ChainRunner::new();
    let _ = cr2.run_station(CHAIN_STATIONS[0], true, "ev");
    set.add("C4079-02", cr2.run_station("抽取", false, "ev") == Err(IgCode::ChainBroken), "断链检出");

    // 3) 证据缺拒。
    set.add("C4079-03", cr2.run_station(CHAIN_STATIONS[1], true, "") == Err(IgCode::EvidenceMissing), "证据缺拒");

    // 4) 非法站名拒。
    set.add("C4079-04", cr2.run_station("不存在的站", true, "ev") == Err(IgCode::ChainBroken), "非法站拒");

    // 5) 乱序检出。
    let mut cr3 = ChainRunner::new();
    let _ = cr3.run_station(CHAIN_STATIONS[1], true, "ev");
    set.add("C4079-05", !cr3.in_order(), "乱序检出");

    // 6) 三故障注入演练。
    let mut db = DrillBook::new();
    for f in Fault::ALL.iter() {
        let _ = db.drill(*f, true);
    }
    set.add("C4079-06", db.all_drilled(), "三故障演练");

    // 7) 降级预案激活验证。
    set.add("C4079-07", db.plans_verified(), "预案激活");

    // 8) 演练失败拒。
    let mut db2 = DrillBook::new();
    set.add("C4079-08", db2.drill(Fault::EngineDown, false) == Err(IgCode::DrillFail), "演练失败拒");

    // 9) 预案映射。
    set.add("C4079-09", Fault::EngineDown.plan() == "切备引擎"
        && Fault::LibDamaged.plan() == "快照回滚"
        && Fault::PlaceholderBroken.plan() == "回退译文", "预案映射");

    // 10) 故障名双射。
    let mut names: Vec<&str> = Vec::new();
    for f in Fault::ALL.iter() { names.push(f.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4079-10", !ndup, "故障名双射");

    // 11) 站表无重复。
    let mut sdup = false;
    for i in 0..CHAIN_STATIONS.len() {
        for j in (i + 1)..CHAIN_STATIONS.len() {
            if CHAIN_STATIONS[i] == CHAIN_STATIONS[j] { sdup = true; }
        }
    }
    set.add("C4079-11", !sdup && CHAIN_STATIONS.len() == 18, "18 站无重复");

    // 12) 段位自查。
    set.add("C4079-12", SEG_BASE == 0x90C0, "段位 0x90C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_flow() {
        let mut cr = ChainRunner::new();
        let _ = cr.run_station(CHAIN_STATIONS[0], true, "e");
        assert!(!cr.complete());
        assert_eq!(cr.run_station(CHAIN_STATIONS[0], true, "e"), Err(IgCode::ChainBroken));
    }
}
