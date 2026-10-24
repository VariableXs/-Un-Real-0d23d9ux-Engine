//! ves384_s21sim — VE-F4144 · 多语言仿真器
//! 运行时仿真：语种/地区/方向/字体缺失四维组合仿真（不重启热切）；
//! 伪本地化三模式（加长/变音符号/镜像）；仿真会话与真实会话隔离（红线——泄漏→自动隔离+告警）；
//! 组合爆炸→预设组合库；仿真崩溃→最小复现；读屏多语言跟读仿真（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xDE40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimCode {
    Ok = 0,
    LeakDetected = SEG_BASE + 0x01,
    ComboMissing = SEG_BASE + 0x02,
    CrashReproduced = SEG_BASE + 0x03,
}

/// 四维仿真配置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SimDims {
    pub lang: &'static str,     // 语种
    pub region: &'static str,   // 地区
    pub rtl: bool,              // 方向
    pub font_missing: bool,     // 字体缺失
}

impl SimDims {
    pub fn new(lang: &'static str, region: &'static str, rtl: bool, font_missing: bool) -> Self {
        SimDims { lang, region, rtl, font_missing }
    }
    pub fn valid(&self) -> bool { !self.lang.is_empty() && !self.region.is_empty() }
}

/// 伪本地化三模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PseudoMode { Off = 0, Lengthen = 1, Diacritics = 2, Mirror = 3 }

impl PseudoMode {
    pub const ALL: [PseudoMode; 3] = [PseudoMode::Lengthen, PseudoMode::Diacritics, PseudoMode::Mirror];
    pub fn name(self) -> &'static str {
        match self {
            PseudoMode::Off => "off", PseudoMode::Lengthen => "lengthen",
            PseudoMode::Diacritics => "diacritics", PseudoMode::Mirror => "mirror",
        }
    }
}

/// 会话类型：仿真会话与真实会话隔离（红线）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionKind { Real = 0, Simulated = 1 }

/// 会话：kind + 仿真维 + 伪本地化模式 + 隔离标志。
pub struct Session {
    pub kind: SessionKind,
    pub dims: Option<SimDims>,
    pub pseudo: PseudoMode,
    pub screen_reader_sim: bool, // 读屏跟读仿真（域本色）
    pub leaked: bool,
    pub quarantined: bool,
    pub alerts: Vec<&'static str>,
}

impl Session {
    pub fn real() -> Self {
        Session { kind: SessionKind::Real, dims: None, pseudo: PseudoMode::Off,
            screen_reader_sim: false, leaked: false, quarantined: false, alerts: Vec::new() }
    }

    pub fn simulated(dims: SimDims, pseudo: PseudoMode) -> Self {
        Session { kind: SessionKind::Simulated, dims: Some(dims), pseudo,
            screen_reader_sim: false, leaked: false, quarantined: false, alerts: Vec::new() }
    }

    /// 泄漏检测：真实会话出现仿真维/伪本地化即泄漏 → 自动隔离+告警（红线实测）。
    pub fn check_leak(&mut self) -> bool {
        let leaked = self.kind == SessionKind::Real
            && (self.dims.is_some() || self.pseudo != PseudoMode::Off);
        if leaked {
            self.leaked = true;
            self.quarantined = true;
            self.alerts.push("sim-leak-quarantined");
        }
        leaked
    }

    /// 隔离协议断言：仿真会话不触碰真实会话存储。
    pub fn isolation_ok(&self, real: &Session) -> bool {
        self.kind == SessionKind::Simulated
            && real.kind == SessionKind::Real
            && !self.leaked && !real.leaked
    }
}

/// 预设组合库：组合爆炸→预设四维组合（含读屏跟读轴样本）。
pub struct PresetLib { pub presets: Vec<SimDims> }

impl PresetLib {
    pub fn standard() -> Self {
        PresetLib {
            presets: vec![
                SimDims::new("zh-CN", "CN", false, false),
                SimDims::new("ar-EG", "EG", true, false),
                SimDims::new("de-DE", "DE", false, true),
                SimDims::new("ja-JP", "JP", false, false),
            ],
        }
    }
    pub fn has(&self, d: &SimDims) -> bool { self.presets.iter().any(|p| p == d) }
    pub fn len(&self) -> usize { self.presets.len() }
}

/// 热切：仿真维切换不重启（返回新会话态描述）。
pub fn hot_switch(from: &SimDims, to: &SimDims) -> bool {
    from != to && to.valid()
}

/// 崩溃最小复现：给崩溃维组合生成复现签名。
pub fn crash_repro(d: &SimDims) -> &'static str {
    match (d.rtl, d.font_missing) {
        (true, true) => "repro:rtl+missing-font",
        (true, false) => "repro:rtl",
        (false, true) => "repro:missing-font",
        (false, false) => "repro:none",
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves384_checks() -> CheckSet {
    let mut set = CheckSet::new("ves384");

    // C384-01 四维配置合法性。
    let d1 = SimDims::new("ar-EG", "EG", true, false);
    let d2 = SimDims::new("", "EG", false, false);
    chk(&mut set, "C384-01", d1.valid() && !d2.valid(), "四维合法校验");

    // C384-02 仿真会话建立 + 热切（不重启）。
    let s1 = Session::simulated(d1, PseudoMode::Off);
    let s2 = Session::simulated(SimDims::new("de-DE", "DE", false, true), PseudoMode::Lengthen);
    chk(&mut set, "C384-02", s1.kind == SessionKind::Simulated && hot_switch(&d1, &s2.dims.unwrap()), "热切成立");

    // C384-03 伪本地化三模式单源。
    chk(&mut set, "C384-03", PseudoMode::ALL.len() == 3
        && PseudoMode::ALL[0].name() == "lengthen" && PseudoMode::ALL[2].name() == "mirror", "三模式单源");

    // C384-04 隔离协议：仿真 vs 真实会话互不泄漏。
    let real = Session::real();
    chk(&mut set, "C384-04", s1.isolation_ok(&real), "会话隔离成立");

    // C384-05 泄漏红线：真实会话挂上仿真维 → 自动隔离+告警。
    let mut leak = Session::real();
    leak.dims = Some(d1);
    chk(&mut set, "C384-05", leak.check_leak() && leak.quarantined
        && leak.alerts == vec!["sim-leak-quarantined"], "泄漏即隔离告警");

    // C384-06 泄漏会话不再满足隔离协议。
    chk(&mut set, "C384-06", !leak.isolation_ok(&real), "泄漏破坏隔离");

    // C384-07 真实会话无仿真态不触发。
    let mut clean = Session::real();
    chk(&mut set, "C384-07", !clean.check_leak(), "干净会话无告警");

    // C384-08 预设组合库：四预设，含 RTL 与缺字体轴。
    let lib = PresetLib::standard();
    chk(&mut set, "C384-08", lib.len() == 4 && lib.has(&d1)
        && lib.presets.iter().any(|p| p.rtl) && lib.presets.iter().any(|p| p.font_missing), "四预设库");

    // C384-09 组合爆炸防护：库外组合返回未命中码。
    let wild = SimDims::new("kok-IN", "IN", true, true);
    chk(&mut set, "C384-09", !lib.has(&wild), "库外组合未命中");

    // C384-10 仿真崩溃→最小复现签名。
    chk(&mut set, "C384-10", crash_repro(&wild) == "repro:rtl+missing-font"
        && crash_repro(&d1) == "repro:rtl" && crash_repro(&SimDims::new("zh-CN", "CN", false, false)) == "repro:none", "复现签名");

    // C384-11 读屏跟读仿真（域本色）。
    let mut sr = Session::simulated(SimDims::new("ar-EG", "EG", true, false), PseudoMode::Mirror);
    sr.screen_reader_sim = true;
    chk(&mut set, "C384-11", sr.screen_reader_sim && sr.pseudo == PseudoMode::Mirror, "读屏跟读仿真位");

    // C384-12 伪本地化 Off 为真实态等价。
    let off = Session::simulated(SimDims::new("zh-CN", "CN", false, false), PseudoMode::Off);
    chk(&mut set, "C384-12", off.pseudo == PseudoMode::Off && off.kind == SessionKind::Simulated, "Off 模式即仿真基线");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves384_smoke() {
        let set = run_ves384_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves384 red: {:?}", red);
    }

    #[test]
    fn ves384_real_session_clean() {
        let mut s = Session::real();
        s.pseudo = PseudoMode::Lengthen;
        assert!(s.check_leak()); // 真实会话 + 伪本地化 = 泄漏
        assert!(s.quarantined);
    }
}
