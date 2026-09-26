//! AI-V2 隔离舱（#[path] 直挂真实文件——K2/D1 批同款工艺）。
//!
//! 用途：共享 crate（kernel/varix）被其他分队的在途状态挡住编译时
//! （本次：h1base u128、compatstar/winsock 等多处——均非 V2 任务面），
//! 本舱把 AI-V2 的真实模块文件原样挂进来独立编译 + 独立测试。
//! **零拷贝、零分叉**：所有 #[path] 指回仓库内的唯一事实文件，
//! 舱内任何红绿都不是另一份事实，只是同一份事实的独立验证口。
//!
//! 垫片面（仅一处，直挂真实文件而非仿写）：
//! - `crate::checks` → 直挂真实 checks.rs（判定口径同源）。
//!
//! 收口后本舱整体留 `_attic/aiv2-f131-f150/` 归档（非功能产物）。

extern crate alloc;

#[path = "../../../../kernel/varix/src/checks.rs"]
pub mod checks;

pub mod stareco {
    #[path = "../../../../../kernel/varix/src/stareco/ebase.rs"]
    pub mod ebase;
    #[path = "../../../../../kernel/varix/src/stareco/upstream.rs"]
    pub mod upstream;
    #[path = "../../../../../kernel/varix/src/stareco/difftable.rs"]
    pub mod difftable;
    #[path = "../../../../../kernel/varix/src/stareco/iconpack.rs"]
    pub mod iconpack;
    #[path = "../../../../../kernel/varix/src/stareco/themeshare.rs"]
    pub mod themeshare;
    #[path = "../../../../../kernel/varix/src/stareco/devportal.rs"]
    pub mod devportal;
    #[path = "../../../../../kernel/varix/src/stareco/examples.rs"]
    pub mod examples;
    #[path = "../../../../../kernel/varix/src/stareco/apistab.rs"]
    pub mod apistab;
    #[path = "../../../../../kernel/varix/src/stareco/releasecal.rs"]
    pub mod releasecal;
    #[path = "../../../../../kernel/varix/src/stareco/feedbackloop.rs"]
    pub mod feedbackloop;
    #[path = "../../../../../kernel/varix/src/stareco/l10nopen.rs"]
    pub mod l10nopen;
    #[path = "../../../../../kernel/varix/src/stareco/a11yopen.rs"]
    pub mod a11yopen;
    #[path = "../../../../../kernel/varix/src/stareco/secdisclose.rs"]
    pub mod secdisclose;
    #[path = "../../../../../kernel/varix/src/stareco/brandkit.rs"]
    pub mod brandkit;
    #[path = "../../../../../kernel/varix/src/stareco/craftbadge.rs"]
    pub mod craftbadge;
    #[path = "../../../../../kernel/varix/src/stareco/eduportfolio.rs"]
    pub mod eduportfolio;
    #[path = "../../../../../kernel/varix/src/stareco/starmapprov.rs"]
    pub mod starmapprov;
    #[path = "../../../../../kernel/varix/src/stareco/syncroam.rs"]
    pub mod syncroam;
    #[path = "../../../../../kernel/varix/src/stareco/governance.rs"]
    pub mod governance;
    #[path = "../../../../../kernel/varix/src/stareco/ecoreport.rs"]
    pub mod ecoreport;
    #[path = "../../../../../kernel/varix/src/stareco/ecogate.rs"]
    pub mod ecogate;

    pub mod deep {
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f131d.rs"]
        pub mod f131d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f132d.rs"]
        pub mod f132d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f133d.rs"]
        pub mod f133d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f134d.rs"]
        pub mod f134d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f135d.rs"]
        pub mod f135d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f136d.rs"]
        pub mod f136d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f137d.rs"]
        pub mod f137d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f138d.rs"]
        pub mod f138d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f139d.rs"]
        pub mod f139d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f140d.rs"]
        pub mod f140d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f141d.rs"]
        pub mod f141d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f142d.rs"]
        pub mod f142d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f143d.rs"]
        pub mod f143d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f144d.rs"]
        pub mod f144d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f145d.rs"]
        pub mod f145d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f146d.rs"]
        pub mod f146d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f147d.rs"]
        pub mod f147d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f148d.rs"]
        pub mod f148d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f149d.rs"]
        pub mod f149d;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f150d.rs"]
        pub mod f150d;
    }
}

#[cfg(test)]
mod diag {
    fn dump(name: &str, set: &crate::checks::CheckSet) {
        let mut buf = [0u8; 4096];
        let n = set.render(&mut buf);
        println!("== {name} ==\n{}", core::str::from_utf8(&buf[..n]).unwrap());
    }

    #[test]
    fn dump_red_sets() {
        dump("ebase", &crate::stareco::ebase::run_ebase_checks());
        dump("upstream", &crate::stareco::upstream::run_upstream_checks());
        dump("difftable", &crate::stareco::difftable::run_difftable_checks());
        dump("iconpack", &crate::stareco::iconpack::run_iconpack_checks());
        dump("themeshare", &crate::stareco::themeshare::run_themeshare_checks());
        dump("devportal", &crate::stareco::devportal::run_devportal_checks());
        dump("examples", &crate::stareco::examples::run_examples_checks());
        dump("apistab", &crate::stareco::apistab::run_apistab_checks());
        dump("releasecal", &crate::stareco::releasecal::run_releasecal_checks());
        dump("feedbackloop", &crate::stareco::feedbackloop::run_feedbackloop_checks());
        dump("l10nopen", &crate::stareco::l10nopen::run_l10nopen_checks());
        dump("a11yopen", &crate::stareco::a11yopen::run_a11yopen_checks());
        dump("secdisclose", &crate::stareco::secdisclose::run_secdisclose_checks());
        dump("brandkit", &crate::stareco::brandkit::run_brandkit_checks());
        dump("craftbadge", &crate::stareco::craftbadge::run_craftbadge_checks());
        dump("eduportfolio", &crate::stareco::eduportfolio::run_eduportfolio_checks());
        dump("starmapprov", &crate::stareco::starmapprov::run_starmapprov_checks());
        dump("syncroam", &crate::stareco::syncroam::run_syncroam_checks());
        dump("governance", &crate::stareco::governance::run_governance_checks());
        dump("ecoreport", &crate::stareco::ecoreport::run_ecoreport_checks());
        dump("ecogate", &crate::stareco::ecogate::run_ecogate_checks());
        dump("f133d", &crate::stareco::deep::f133d::run_f133_deep_checks());
        dump("f136d", &crate::stareco::deep::f136d::run_f136_deep_checks());
        dump("f137d", &crate::stareco::deep::f137d::run_f137_deep_checks());
        dump("f138d", &crate::stareco::deep::f138d::run_f138_deep_checks());
        dump("f146d", &crate::stareco::deep::f146d::run_f146_deep_checks());
        dump("f148d", &crate::stareco::deep::f148d::run_f148_deep_checks());
    }

    /// 域聚合等价断言：舱内逐模块红绿拼出的聚合与真实 mod.rs 聚合
    /// 口径一致（22 blocks 全绿才收工——基础 21 + 深化 1）。
    #[test]
    fn domain_aggregate_all_green() {
        let mut red = alloc::vec::Vec::new();
        let base: [(&str, crate::checks::CheckSet); 21] = [
            ("ebase", crate::stareco::ebase::run_ebase_checks()),
            ("F131", crate::stareco::upstream::run_upstream_checks()),
            ("F132", crate::stareco::difftable::run_difftable_checks()),
            ("F133", crate::stareco::iconpack::run_iconpack_checks()),
            ("F134", crate::stareco::themeshare::run_themeshare_checks()),
            ("F135", crate::stareco::devportal::run_devportal_checks()),
            ("F136", crate::stareco::examples::run_examples_checks()),
            ("F137", crate::stareco::apistab::run_apistab_checks()),
            ("F138", crate::stareco::releasecal::run_releasecal_checks()),
            ("F139", crate::stareco::feedbackloop::run_feedbackloop_checks()),
            ("F140", crate::stareco::l10nopen::run_l10nopen_checks()),
            ("F141", crate::stareco::a11yopen::run_a11yopen_checks()),
            ("F142", crate::stareco::secdisclose::run_secdisclose_checks()),
            ("F143", crate::stareco::brandkit::run_brandkit_checks()),
            ("F144", crate::stareco::craftbadge::run_craftbadge_checks()),
            ("F145", crate::stareco::eduportfolio::run_eduportfolio_checks()),
            ("F146", crate::stareco::starmapprov::run_starmapprov_checks()),
            ("F147", crate::stareco::syncroam::run_syncroam_checks()),
            ("F148", crate::stareco::governance::run_governance_checks()),
            ("F149", crate::stareco::ecoreport::run_ecoreport_checks()),
            ("F150", crate::stareco::ecogate::run_ecogate_checks()),
        ];
        for (tag, sub) in base {
            if !(sub.all_passed() && !sub.truncated()) {
                red.push(tag);
            }
        }
        let deep = crate::stareco::deep::f131d::run_f131_deep_checks();
        let _ = deep;
        // 深化层 20 模块逐个红绿
        let deeps: [(&str, crate::checks::CheckSet); 20] = [
            ("F131d", crate::stareco::deep::f131d::run_f131_deep_checks()),
            ("F132d", crate::stareco::deep::f132d::run_f132_deep_checks()),
            ("F133d", crate::stareco::deep::f133d::run_f133_deep_checks()),
            ("F134d", crate::stareco::deep::f134d::run_f134_deep_checks()),
            ("F135d", crate::stareco::deep::f135d::run_f135_deep_checks()),
            ("F136d", crate::stareco::deep::f136d::run_f136_deep_checks()),
            ("F137d", crate::stareco::deep::f137d::run_f137_deep_checks()),
            ("F138d", crate::stareco::deep::f138d::run_f138_deep_checks()),
            ("F139d", crate::stareco::deep::f139d::run_f139_deep_checks()),
            ("F140d", crate::stareco::deep::f140d::run_f140_deep_checks()),
            ("F141d", crate::stareco::deep::f141d::run_f141_deep_checks()),
            ("F142d", crate::stareco::deep::f142d::run_f142_deep_checks()),
            ("F143d", crate::stareco::deep::f143d::run_f143_deep_checks()),
            ("F144d", crate::stareco::deep::f144d::run_f144_deep_checks()),
            ("F145d", crate::stareco::deep::f145d::run_f145_deep_checks()),
            ("F146d", crate::stareco::deep::f146d::run_f146_deep_checks()),
            ("F147d", crate::stareco::deep::f147d::run_f147_deep_checks()),
            ("F148d", crate::stareco::deep::f148d::run_f148_deep_checks()),
            ("F149d", crate::stareco::deep::f149d::run_f149_deep_checks()),
            ("F150d", crate::stareco::deep::f150d::run_f150_deep_checks()),
        ];
        for (tag, sub) in deeps {
            if !(sub.all_passed() && !sub.truncated()) {
                red.push(tag);
            }
        }
        assert!(red.is_empty(), "STARECO-V2 域自检存在红项：{:?}", red);
    }
}
