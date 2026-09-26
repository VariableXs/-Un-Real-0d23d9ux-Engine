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
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f131e.rs"]
        pub mod f131e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f131g.rs"]
        pub mod f131g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f131h.rs"]
        pub mod f131h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f132e.rs"]
        pub mod f132e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f132g.rs"]
        pub mod f132g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f132h.rs"]
        pub mod f132h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f134e.rs"]
        pub mod f134e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f134g.rs"]
        pub mod f134g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f134h.rs"]
        pub mod f134h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f135e.rs"]
        pub mod f135e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f135g.rs"]
        pub mod f135g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f135h.rs"]
        pub mod f135h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f136e.rs"]
        pub mod f136e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f136g.rs"]
        pub mod f136g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f136h.rs"]
        pub mod f136h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f137e.rs"]
        pub mod f137e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f137g.rs"]
        pub mod f137g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f137h.rs"]
        pub mod f137h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f138e.rs"]
        pub mod f138e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f138g.rs"]
        pub mod f138g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f138h.rs"]
        pub mod f138h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f139e.rs"]
        pub mod f139e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f139g.rs"]
        pub mod f139g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f139h.rs"]
        pub mod f139h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f140e.rs"]
        pub mod f140e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f140g.rs"]
        pub mod f140g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f140h.rs"]
        pub mod f140h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f141e.rs"]
        pub mod f141e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f141g.rs"]
        pub mod f141g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f141h.rs"]
        pub mod f141h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f142e.rs"]
        pub mod f142e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f142g.rs"]
        pub mod f142g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f142h.rs"]
        pub mod f142h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f143e.rs"]
        pub mod f143e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f143g.rs"]
        pub mod f143g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f143h.rs"]
        pub mod f143h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f133e.rs"]
        pub mod f133e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f133g.rs"]
        pub mod f133g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f133h.rs"]
        pub mod f133h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f144e.rs"]
        pub mod f144e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f144g.rs"]
        pub mod f144g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f144h.rs"]
        pub mod f144h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f145e.rs"]
        pub mod f145e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f145g.rs"]
        pub mod f145g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f145h.rs"]
        pub mod f145h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f146e.rs"]
        pub mod f146e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f146g.rs"]
        pub mod f146g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f146h.rs"]
        pub mod f146h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f147e.rs"]
        pub mod f147e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f147g.rs"]
        pub mod f147g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f147h.rs"]
        pub mod f147h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f148e.rs"]
        pub mod f148e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f148g.rs"]
        pub mod f148g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f148h.rs"]
        pub mod f148h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f149e.rs"]
        pub mod f149e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f149g.rs"]
        pub mod f149g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f149h.rs"]
        pub mod f149h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f150e.rs"]
        pub mod f150e;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f150g.rs"]
        pub mod f150g;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f150h.rs"]
        pub mod f150h;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f150i.rs"]
        pub mod f150i;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f150j.rs"]
        pub mod f150j;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f150k.rs"]
        pub mod f150k;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f150l.rs"]
        pub mod f150l;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f131f.rs"]
        pub mod f131f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f132f.rs"]
        pub mod f132f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f133f.rs"]
        pub mod f133f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f134f.rs"]
        pub mod f134f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f135f.rs"]
        pub mod f135f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f136f.rs"]
        pub mod f136f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f137f.rs"]
        pub mod f137f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f138f.rs"]
        pub mod f138f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f139f.rs"]
        pub mod f139f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f140f.rs"]
        pub mod f140f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f141f.rs"]
        pub mod f141f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f142f.rs"]
        pub mod f142f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f143f.rs"]
        pub mod f143f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f144f.rs"]
        pub mod f144f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f145f.rs"]
        pub mod f145f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f146f.rs"]
        pub mod f146f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f147f.rs"]
        pub mod f147f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f148f.rs"]
        pub mod f148f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f149f.rs"]
        pub mod f149f;
        #[path = "../../../../../../kernel/varix/src/stareco/deep/f150f.rs"]
        pub mod f150f;
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

    /// 批次三（f 系列）聚合全绿断言：20 blocks 逐个红绿，红项指名。
    #[test]
    fn deep3_aggregate_all_green() {
        let deeps: [(&str, crate::checks::CheckSet); 20] = [
            ("F131f", crate::stareco::deep::f131f::run_f131_deep3_checks()),
            ("F132f", crate::stareco::deep::f132f::run_f132_deep3_checks()),
            ("F133f", crate::stareco::deep::f133f::run_f133_deep3_checks()),
            ("F134f", crate::stareco::deep::f134f::run_f134_deep3_checks()),
            ("F135f", crate::stareco::deep::f135f::run_f135_deep3_checks()),
            ("F136f", crate::stareco::deep::f136f::run_f136_deep3_checks()),
            ("F137f", crate::stareco::deep::f137f::run_f137_deep3_checks()),
            ("F138f", crate::stareco::deep::f138f::run_f138_deep3_checks()),
            ("F139f", crate::stareco::deep::f139f::run_f139_deep3_checks()),
            ("F140f", crate::stareco::deep::f140f::run_f140_deep3_checks()),
            ("F141f", crate::stareco::deep::f141f::run_f141_deep3_checks()),
            ("F142f", crate::stareco::deep::f142f::run_f142_deep3_checks()),
            ("F143f", crate::stareco::deep::f143f::run_f143_deep3_checks()),
            ("F144f", crate::stareco::deep::f144f::run_f144_deep3_checks()),
            ("F145f", crate::stareco::deep::f145f::run_f145_deep3_checks()),
            ("F146f", crate::stareco::deep::f146f::run_f146_deep3_checks()),
            ("F147f", crate::stareco::deep::f147f::run_f147_deep3_checks()),
            ("F148f", crate::stareco::deep::f148f::run_f148_deep3_checks()),
            ("F149f", crate::stareco::deep::f149f::run_f149_deep3_checks()),
            ("F150f", crate::stareco::deep::f150f::run_f150_deep3_checks()),
        ];
        let mut red = alloc::vec::Vec::new();
        let mut green = 0usize;
        for (tag, sub) in deeps {
            if sub.all_passed() && !sub.truncated() {
                green += 1;
            } else {
                red.push(tag);
            }
        }
        assert!(
            red.is_empty(),
            "STARECO-V2 深化批次三存在红项：{:?}（绿 {}/20）",
            red,
            green
        );
    }

    /// 批次四（g 系列）聚合全绿断言：随批扩容（当前 6 块，批次四完工 20 块）。
    #[test]
    fn deep4_aggregate_all_green() {
        let deeps: [(&str, crate::checks::CheckSet); 20] = [
            ("F131g", crate::stareco::deep::f131g::run_f131_deep4_checks()),
            ("F132g", crate::stareco::deep::f132g::run_f132_deep4_checks()),
            ("F133g", crate::stareco::deep::f133g::run_f133_deep4_checks()),
            ("F134g", crate::stareco::deep::f134g::run_f134_deep4_checks()),
            ("F135g", crate::stareco::deep::f135g::run_f135_deep4_checks()),
            ("F136g", crate::stareco::deep::f136g::run_f136_deep4_checks()),
            ("F137g", crate::stareco::deep::f137g::run_f137_deep4_checks()),
            ("F138g", crate::stareco::deep::f138g::run_f138_deep4_checks()),
            ("F139g", crate::stareco::deep::f139g::run_f139_deep4_checks()),
            ("F140g", crate::stareco::deep::f140g::run_f140_deep4_checks()),
            ("F141g", crate::stareco::deep::f141g::run_f141_deep4_checks()),
            ("F142g", crate::stareco::deep::f142g::run_f142_deep4_checks()),
            ("F143g", crate::stareco::deep::f143g::run_f143_deep4_checks()),
            ("F144g", crate::stareco::deep::f144g::run_f144_deep4_checks()),
            ("F145g", crate::stareco::deep::f145g::run_f145_deep4_checks()),
            ("F146g", crate::stareco::deep::f146g::run_f146_deep4_checks()),
            ("F147g", crate::stareco::deep::f147g::run_f147_deep4_checks()),
            ("F148g", crate::stareco::deep::f148g::run_f148_deep4_checks()),
            ("F149g", crate::stareco::deep::f149g::run_f149_deep4_checks()),
            ("F150g", crate::stareco::deep::f150g::run_f150_deep4_checks()),
        ];
        let mut red = alloc::vec::Vec::new();
        for (tag, sub) in deeps {
            if !(sub.all_passed() && !sub.truncated()) {
                red.push(tag);
            }
        }
        assert!(red.is_empty(), "STARECO-V2 深化批次四存在红项：{:?}", red);
    }

    /// 批次五（h 系列）聚合全绿断言：20 blocks。
    #[test]
    fn deep5_aggregate_all_green() {
        let deeps: [(&str, crate::checks::CheckSet); 20] = [
            ("F131h", crate::stareco::deep::f131h::run_f131_deep5_checks()),
            ("F132h", crate::stareco::deep::f132h::run_f132_deep5_checks()),
            ("F133h", crate::stareco::deep::f133h::run_f133_deep5_checks()),
            ("F134h", crate::stareco::deep::f134h::run_f134_deep5_checks()),
            ("F135h", crate::stareco::deep::f135h::run_f135_deep5_checks()),
            ("F136h", crate::stareco::deep::f136h::run_f136_deep5_checks()),
            ("F137h", crate::stareco::deep::f137h::run_f137_deep5_checks()),
            ("F138h", crate::stareco::deep::f138h::run_f138_deep5_checks()),
            ("F139h", crate::stareco::deep::f139h::run_f139_deep5_checks()),
            ("F140h", crate::stareco::deep::f140h::run_f140_deep5_checks()),
            ("F141h", crate::stareco::deep::f141h::run_f141_deep5_checks()),
            ("F142h", crate::stareco::deep::f142h::run_f142_deep5_checks()),
            ("F143h", crate::stareco::deep::f143h::run_f143_deep5_checks()),
            ("F144h", crate::stareco::deep::f144h::run_f144_deep5_checks()),
            ("F145h", crate::stareco::deep::f145h::run_f145_deep5_checks()),
            ("F146h", crate::stareco::deep::f146h::run_f146_deep5_checks()),
            ("F147h", crate::stareco::deep::f147h::run_f147_deep5_checks()),
            ("F148h", crate::stareco::deep::f148h::run_f148_deep5_checks()),
            ("F149h", crate::stareco::deep::f149h::run_f149_deep5_checks()),
            ("F150h", crate::stareco::deep::f150h::run_f150_deep5_checks()),
        ];
        let mut red = alloc::vec::Vec::new();
        for (tag, sub) in deeps {
            if !(sub.all_passed() && !sub.truncated()) {
                red.push(tag);
            }
        }
        assert!(red.is_empty(), "STARECO-V2 深化批次五存在红项：{:?}", red);
    }

    /// 收尾件 f150i（域快照与闸门）全绿断言。
    #[test]
    fn deep5b_aggregate_all_green() {
        let set = crate::stareco::deep::f150i::run_f150_deep5b_checks();
        assert!(
            set.all_passed() && !set.truncated(),
            "STARECO-V2 f150i 收尾件存在红项"
        );
    }

    /// 收尾件 f150j（批次台账总装）全绿断言。
    #[test]
    fn deep5c_aggregate_all_green() {
        let set = crate::stareco::deep::f150j::run_f150_deep5c_checks();
        assert!(
            set.all_passed() && !set.truncated(),
            "STARECO-V2 f150j 台账总装存在红项"
        );
    }

    /// 批次六收口件（f150k 总闸 + f150l 交接面）全绿断言。
    #[test]
    fn deep6_aggregate_all_green() {
        let sets = [
            crate::stareco::deep::f150k::run_f150_deep6_checks(),
            crate::stareco::deep::f150l::run_f150_deep6b_checks(),
        ];
        for set in sets {
            assert!(
                set.all_passed() && !set.truncated(),
                "STARECO-V2 批次六收口件存在红项"
            );
        }
    }

    /// 批次三红项明细 dump（每块逐行渲染，定位具体断言）。
    #[test]
    fn dump_red_sets3() {
        let mods: [(&str, fn() -> crate::checks::CheckSet); 20] = [
            ("f131f", crate::stareco::deep::f131f::run_f131_deep3_checks),
            ("f132f", crate::stareco::deep::f132f::run_f132_deep3_checks),
            ("f133f", crate::stareco::deep::f133f::run_f133_deep3_checks),
            ("f134f", crate::stareco::deep::f134f::run_f134_deep3_checks),
            ("f135f", crate::stareco::deep::f135f::run_f135_deep3_checks),
            ("f136f", crate::stareco::deep::f136f::run_f136_deep3_checks),
            ("f137f", crate::stareco::deep::f137f::run_f137_deep3_checks),
            ("f138f", crate::stareco::deep::f138f::run_f138_deep3_checks),
            ("f139f", crate::stareco::deep::f139f::run_f139_deep3_checks),
            ("f140f", crate::stareco::deep::f140f::run_f140_deep3_checks),
            ("f141f", crate::stareco::deep::f141f::run_f141_deep3_checks),
            ("f142f", crate::stareco::deep::f142f::run_f142_deep3_checks),
            ("f143f", crate::stareco::deep::f143f::run_f143_deep3_checks),
            ("f144f", crate::stareco::deep::f144f::run_f144_deep3_checks),
            ("f145f", crate::stareco::deep::f145f::run_f145_deep3_checks),
            ("f146f", crate::stareco::deep::f146f::run_f146_deep3_checks),
            ("f147f", crate::stareco::deep::f147f::run_f147_deep3_checks),
            ("f148f", crate::stareco::deep::f148f::run_f148_deep3_checks),
            ("f149f", crate::stareco::deep::f149f::run_f149_deep3_checks),
            ("f150f", crate::stareco::deep::f150f::run_f150_deep3_checks),
        ];
        for (name, f) in mods {
            let set = f();
            let mut buf = [0u8; 4096];
            let n = set.render(&mut buf);
            println!("== {name} ==\n{}", core::str::from_utf8(&buf[..n]).unwrap_or("?"));
        }
    }
}

#[cfg(test)]
mod probe4 {
    #[test]
    fn dump_g4() {
        let sets: [(&str, crate::checks::CheckSet); 40] = [
            ("f131g", crate::stareco::deep::f131g::run_f131_deep4_checks()),
            ("f132g", crate::stareco::deep::f132g::run_f132_deep4_checks()),
            ("f133g", crate::stareco::deep::f133g::run_f133_deep4_checks()),
            ("f134g", crate::stareco::deep::f134g::run_f134_deep4_checks()),
            ("f135g", crate::stareco::deep::f135g::run_f135_deep4_checks()),
            ("f136g", crate::stareco::deep::f136g::run_f136_deep4_checks()),
            ("f137g", crate::stareco::deep::f137g::run_f137_deep4_checks()),
            ("f138g", crate::stareco::deep::f138g::run_f138_deep4_checks()),
            ("f139g", crate::stareco::deep::f139g::run_f139_deep4_checks()),
            ("f140g", crate::stareco::deep::f140g::run_f140_deep4_checks()),
            ("f141g", crate::stareco::deep::f141g::run_f141_deep4_checks()),
            ("f142g", crate::stareco::deep::f142g::run_f142_deep4_checks()),
            ("f143g", crate::stareco::deep::f143g::run_f143_deep4_checks()),
            ("f144g", crate::stareco::deep::f144g::run_f144_deep4_checks()),
            ("f145g", crate::stareco::deep::f145g::run_f145_deep4_checks()),
            ("f146g", crate::stareco::deep::f146g::run_f146_deep4_checks()),
            ("f147g", crate::stareco::deep::f147g::run_f147_deep4_checks()),
            ("f148g", crate::stareco::deep::f148g::run_f148_deep4_checks()),
            ("f149g", crate::stareco::deep::f149g::run_f149_deep4_checks()),
            ("f150g", crate::stareco::deep::f150g::run_f150_deep4_checks()),
               ("f131h", crate::stareco::deep::f131h::run_f131_deep5_checks()),
            ("f132h", crate::stareco::deep::f132h::run_f132_deep5_checks()),
            ("f133h", crate::stareco::deep::f133h::run_f133_deep5_checks()),
            ("f134h", crate::stareco::deep::f134h::run_f134_deep5_checks()),
            ("f135h", crate::stareco::deep::f135h::run_f135_deep5_checks()),
            ("f136h", crate::stareco::deep::f136h::run_f136_deep5_checks()),
            ("f137h", crate::stareco::deep::f137h::run_f137_deep5_checks()),
            ("f138h", crate::stareco::deep::f138h::run_f138_deep5_checks()),
            ("f139h", crate::stareco::deep::f139h::run_f139_deep5_checks()),
            ("f140h", crate::stareco::deep::f140h::run_f140_deep5_checks()),
            ("f141h", crate::stareco::deep::f141h::run_f141_deep5_checks()),
            ("f142h", crate::stareco::deep::f142h::run_f142_deep5_checks()),
            ("f143h", crate::stareco::deep::f143h::run_f143_deep5_checks()),
            ("f144h", crate::stareco::deep::f144h::run_f144_deep5_checks()),
            ("f145h", crate::stareco::deep::f145h::run_f145_deep5_checks()),
            ("f146h", crate::stareco::deep::f146h::run_f146_deep5_checks()),
            ("f147h", crate::stareco::deep::f147h::run_f147_deep5_checks()),
            ("f148h", crate::stareco::deep::f148h::run_f148_deep5_checks()),
            ("f149h", crate::stareco::deep::f149h::run_f149_deep5_checks()),
            ("f150h", crate::stareco::deep::f150h::run_f150_deep5_checks()),
        ];
        for (name, set) in sets {
            let mut buf = [0u8; 4096];
            let n = set.render(&mut buf);
            println!("== {name} ==\n{}", core::str::from_utf8(&buf[..n]).unwrap_or("?"));
        }
    }
}

