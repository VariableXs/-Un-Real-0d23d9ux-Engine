//! AI-V1 隔离舱（svcabin）：`#[path]` 直挂 `kernel/varix/src/svstar/`
//! 真实文件——主树被其它分队并行施工占用时，本舱独立编译/测试 V1 域。
//! 舱内零拷贝零副本：引用即真实交付文件，舱绿 = 文件绿。

extern crate alloc;

#[path = "../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../varix/src/svstar/vbase.rs"]
pub mod vbase;

#[path = "../../varix/src/svstar/magnifier.rs"]
pub mod magnifier;

#[path = "../../varix/src/svstar/narrator.rs"]
pub mod narrator;

#[path = "../../varix/src/svstar/highcontrast.rs"]
pub mod highcontrast;

#[path = "../../varix/src/svstar/colorfilter.rs"]
pub mod colorfilter;

#[path = "../../varix/src/svstar/focusmode.rs"]
pub mod focusmode;

#[path = "../../varix/src/svstar/nightlight.rs"]
pub mod nightlight;

#[path = "../../varix/src/svstar/oobe.rs"]
pub mod oobe;

#[path = "../../varix/src/svstar/welcome.rs"]
pub mod welcome;

#[path = "../../varix/src/svstar/helpcenter.rs"]
pub mod helpcenter;

#[path = "../../varix/src/svstar/diagcenter.rs"]
pub mod diagcenter;

#[path = "../../varix/src/svstar/restorept.rs"]
pub mod restorept;

#[path = "../../varix/src/svstar/updateux.rs"]
pub mod updateux;

#[path = "../../varix/src/svstar/aboutpage.rs"]
pub mod aboutpage;

#[path = "../../varix/src/svstar/motioncore.rs"]
pub mod motioncore;

#[path = "../../varix/src/svstar/walkcheck.rs"]
pub mod walkcheck;

#[path = "../../varix/src/svstar/openformat.rs"]
pub mod openformat;

#[path = "../../varix/src/svstar/vxapp.rs"]
pub mod vxapp;

#[path = "../../varix/src/svstar/stardata.rs"]
pub mod stardata;

#[path = "../../varix/src/svstar/casesub.rs"]
pub mod casesub;

#[path = "../../varix/src/svstar/ossreg.rs"]
pub mod ossreg;

#[path = "../../varix/src/svstar/mod.rs"]
pub mod svstar_mod;

/// 舱内门面：让模块内 `crate::svstar::xxx` 引用在舱内同样成立
/// （主树由 svstar/mod.rs 提供同名路径——两处编译语义一致）。
pub mod svstar {
    pub use crate::vbase;
    pub use crate::magnifier;
    pub use crate::narrator;
    pub use crate::highcontrast;
    pub use crate::colorfilter;
    pub use crate::focusmode;
    pub use crate::nightlight;
    pub use crate::oobe;
    pub use crate::welcome;
    pub use crate::helpcenter;
    pub use crate::diagcenter;
    pub use crate::restorept;
    pub use crate::updateux;
    pub use crate::aboutpage;
    pub use crate::motioncore;
    pub use crate::walkcheck;
    pub use crate::openformat;
    pub use crate::vxapp;
    pub use crate::stardata;
    pub use crate::casesub;
    pub use crate::ossreg;
}

/// 域聚合器（mod.rs 本体——#[path] 挂载后 run_svstar_checks 在舱内可用；
/// mod.rs 内 `crate::checks` 引用与本舱 checks 模块同源）。
pub use svstar_mod::run_svstar_checks;
pub use svstar_mod::V1_DOMAIN;

#[cfg(test)]
mod cabin_report {
    use alloc::string::String;

    fn report(domain: &str, set: crate::checks::CheckSet) {
        let mut bad = String::new();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    bad.push_str(c.name);
                    bad.push_str(" || ");
                }
            }
        }
        println!(
            "{}: {}/{} {}",
            domain,
            set.tally().0,
            set.len(),
            if bad.is_empty() { "GREEN" } else { bad.as_str() }
        );
    }

    #[test]
    fn all_domains_report() {
        report("vbase", crate::vbase::run_vbase_checks());
        report("F111", crate::magnifier::run_magnifier_checks());
        report("F112", crate::narrator::run_narrator_checks());
        report("F113", crate::highcontrast::run_highcontrast_checks());
        report("F114", crate::colorfilter::run_colorfilter_checks());
        report("F115", crate::focusmode::run_focusmode_checks());
        report("F116", crate::nightlight::run_nightlight_checks());
        report("F117", crate::oobe::run_oobe_checks());
        report("F118", crate::welcome::run_welcome_checks());
        report("F119", crate::helpcenter::run_helpcenter_checks());
        report("F120", crate::diagcenter::run_diagcenter_checks());
        report("F121", crate::restorept::run_restorept_checks());
        report("F122", crate::updateux::run_updateux_checks());
        report("F123", crate::aboutpage::run_aboutpage_checks());
        report("F124", crate::motioncore::run_motioncore_checks());
        report("F125", crate::walkcheck::run_walkcheck_checks());
        report("F126", crate::openformat::run_openformat_checks());
        report("F127", crate::vxapp::run_vxapp_checks());
        report("F128", crate::stardata::run_stardata_checks());
        report("F129", crate::casesub::run_casesub_checks());
        report("F130", crate::ossreg::run_ossreg_checks());
        report("AGG", crate::svstar_mod::run_svstar_checks());
    }
}
