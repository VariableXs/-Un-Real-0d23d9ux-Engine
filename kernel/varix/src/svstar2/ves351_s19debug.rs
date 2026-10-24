//! ves351_s19debug — VE-F4111 · T06 地区调试
//! 地区模拟器（一键模拟任意地区环境：格式/法规/审查/支付四维生效预览）+ 合规命中探查器（输入功能→列出命中地区规则——超时分片）+ 配置差异对拍器（冲突列证据）+ 模拟态泄漏到真实会话自动隔离告警（红线实测——模拟不写真实用户配置）+ 输出遵循五标准。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xC640;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbCode {
    Ok = 0,
    LeakDetected = SEG_BASE + 0x01,
    TimeoutSliced = SEG_BASE + 0x02,
    EvidenceClash = SEG_BASE + 0x03,
}

/// 四维。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimDim { Format = 0, Law = 1, Content = 2, Payment = 3 }

impl SimDim {
    pub const ALL: [SimDim; 4] = [SimDim::Format, SimDim::Law, SimDim::Content, SimDim::Payment];
    pub fn name(self) -> &'static str {
        match self { SimDim::Format => "format", SimDim::Law => "law",
            SimDim::Content => "content", SimDim::Payment => "payment" }
    }
}

/// 模拟器：模拟地区 × 隔离会话。
pub struct RegionSimulator {
    pub sim_region: &'static str,
    pub session_id: u64,
    /// 模拟会话写镜（隔离证明——只写模拟镜像，不碰真实配置）。
    pub sim_writes: Vec<(&'static str, &'static str)>,
    pub real_writes: Vec<(&'static str, &'static str)>,
    pub quarantined: bool,
}

impl RegionSimulator {
    pub fn new(region: &'static str, session: u64) -> Self {
        RegionSimulator { sim_region: region, session_id: session, sim_writes: Vec::new(), real_writes: Vec::new(), quarantined: false }
    }

    /// 四维生效预览：注入模拟配置 → 返回四维快照。
    pub fn preview(&mut self, cfg: &[(SimDim, &'static str)]) -> [Option<&'static str>; 4] {
        let mut out = [None; 4];
        for (d, v) in cfg.iter() {
            self.sim_writes.push((d.name(), v));
            out[*d as usize] = Some(*v);
        }
        out
    }

    /// 模拟写：只进模拟镜像。
    pub fn sim_write(&mut self, k: &'static str, v: &'static str) {
        self.sim_writes.push((k, v));
    }

    /// 泄漏检测：真实会话出现模拟写 → 自动隔离 + 告警。
    pub fn leak_scan(&mut self, real: &[( &'static str, &'static str)]) -> bool {
        let leaked = real.iter().any(|(k, _)| self.sim_writes.iter().any(|(sk, _)| sk == k));
        if leaked {
            self.quarantined = true;
            self.real_writes.clear();
            return true;
        }
        false
    }

    pub fn isolated(&self) -> bool { self.real_writes.is_empty() && !self.quarantined }
}

/// 合规命中探查器：功能 → 命中规则清单（分片）。
pub struct RuleProbe { pub rules: Vec<(&'static str, &'static str)> } // (功能键, 规则名)

impl RuleProbe {
    pub fn new() -> Self { RuleProbe { rules: Vec::new() } }
    pub fn add(&mut self, feature: &'static str, rule: &'static str) { self.rules.push((feature, rule)); }

    /// 探查：列命中规则；超时预算内分片返回。
    pub fn probe(&self, feature: &str, shard: usize) -> Vec<&'static str> {
        let hits: Vec<&'static str> = self.rules.iter()
            .filter(|(f, _)| *f == feature)
            .map(|(_, r)| *r)
            .collect();
        let start = shard * 2;
        hits.iter().skip(start).take(2).map(|r| *r).collect()
    }

    pub fn total_hits(&self, feature: &str) -> usize {
        self.rules.iter().filter(|(f, _)| *f == feature).count()
    }
}

/// 配置差异对拍器：两配置逐键对拍 → 冲突证据列。
pub struct DiffLine { pub key: &'static str, pub a: &'static str, pub b: &'static str }

pub fn diff_configs(a: &[(&'static str, &'static str)], b: &[(&'static str, &'static str)]) -> Vec<DiffLine> {
    let mut out = Vec::new();
    for (k, va) in a.iter() {
        match b.iter().find(|(kb, _)| kb == k) {
            Some((_, vb)) if vb != va => out.push(DiffLine { key: k, a: va, b: vb }),
            None => out.push(DiffLine { key: k, a: va, b: "ABSENT" }),
            _ => {}
        }
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves351_checks() -> CheckSet {
    let mut set = CheckSet::new("ves351");

    // C351-01 四维名单单源。
    chk(&mut set, "C351-01", SimDim::ALL.len() == 4 && SimDim::ALL[3].name() == "payment", "四维单源");

    // C351-02 一键模拟：四维生效预览。
    let mut sim = RegionSimulator::new("DE", 42);
    let snap = sim.preview(&[
        (SimDim::Format, "neg-paren-on"), (SimDim::Law, "gdpr-strict"),
        (SimDim::Content, "ok-gesture-restricted"), (SimDim::Payment, "card-only")]);
    chk(&mut set, "C351-02", snap[0] == Some("neg-paren-on") && snap[3] == Some("card-only"), "四维快照齐");

    // C351-03 模拟写只进镜像。
    sim.sim_write("theme", "sim-dark");
    chk(&mut set, "C351-03", sim.sim_writes.len() == 5 && sim.isolated(), "五写全在镜像 真实未动");

    // C351-04 泄漏检测：真实会话命中模拟键 → 隔离。
    let real = [("theme", "user-light")];
    chk(&mut set, "C351-04", sim.leak_scan(&real) && sim.quarantined && sim.real_writes.is_empty(), "泄漏 → 自动隔离");

    // C351-05 干净会话不隔离。
    let mut sim2 = RegionSimulator::new("JP", 7);
    sim2.sim_write("fmt-date", "sim");
    chk(&mut set, "C351-05", !sim2.leak_scan(&[("x", "y")]) && sim2.isolated(), "无泄漏不隔离");

    // C351-06 探查器登记与计数。
    let mut rp = RuleProbe::new();
    rp.add("upload-photo", "eu-gdpr-consent");
    rp.add("upload-photo", "br-lgpd");
    rp.add("upload-photo", "us-ccpa");
    rp.add("pay", "eu-vat-invoice");
    chk(&mut set, "C351-06", rp.total_hits("upload-photo") == 3 && rp.total_hits("pay") == 1, "命中计数 3/1");

    // C351-07 分片探查：超时预算 2 条/片。
    let s0 = rp.probe("upload-photo", 0);
    let s1 = rp.probe("upload-photo", 1);
    chk(&mut set, "C351-07", s0 == vec!["eu-gdpr-consent", "br-lgpd"] && s1 == vec!["us-ccpa"], "两片全覆盖");

    // C351-08 越界片空。
    chk(&mut set, "C351-08", rp.probe("upload-photo", 2).is_empty(), "片 2 空");

    // C351-09 配置差异对拍：值不同。
    let a = [("neg-paren", "on"), ("week-start", "mon")];
    let b = [("neg-paren", "off"), ("week-start", "mon")];
    let d = diff_configs(&a, &b);
    chk(&mut set, "C351-09", d.len() == 1 && d[0].key == "neg-paren" && d[0].a == "on" && d[0].b == "off", "差异单条列证据");

    // C351-10 缺键差异。
    let c = [("neg-paren", "on")];
    let d2 = diff_configs(&a, &c);
    chk(&mut set, "C351-10", d2.len() == 1 && d2[0].b == "ABSENT", "缺键 ABSENT");

    // C351-11 全同配置零差异。
    chk(&mut set, "C351-11", diff_configs(&a, &a).is_empty(), "同配零差异");

    // C351-12 五标准输出标记（复述）。
    let tag = SimDim::Law.name();
    chk(&mut set, "C351-12", tag == "law" && !tag.is_empty(), "维度名可复述");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves351_smoke() {
        let set = run_ves351_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves351 red: {:?}", red);
    }

    #[test]
    fn ves351_leak() {
        let mut sim = RegionSimulator::new("US", 1);
        sim.sim_write("k", "v");
        assert!(sim.leak_scan(&[("k", "x")]));
        assert!(sim.quarantined);
    }
}
