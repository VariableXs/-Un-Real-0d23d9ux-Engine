//! AI-U1 隔离舱：#[path] 直挂 kernel/varix/src/uni1 真实文件 + 真实
//! checks.rs，宿主侧独立验证。用途：多 AI 并行施工期，主 crate 可能被
//! 其他分队的在建模块挡住编译——隔离舱让本队判据验证不排队、不被卡。
//!
//! 挂载的都是**真实生产文件**（不是副本）：这里绿 = 仓库里的代码绿。

#![cfg_attr(not(test), no_std)]

extern crate alloc;

// 真实自检底座（与主 crate 同一份 checks.rs）。
#[path = "../../../kernel/varix/src/checks.rs"]
pub mod checks;

// AI-U1 uni1 全域（mod.rs 相对路径加载同目录九个真实模块）。
#[path = "../../../kernel/varix/src/uni1/mod.rs"]
pub mod uni1;

#[cfg(test)]
mod tests {
    #[test]
    fn cabin_full_domain_green() {
        let blocks: [(&str, crate::checks::CheckSet); 51] = [
            ("ubase", crate::uni1::ubase::run_ubase_checks()),
            ("F401", crate::uni1::autoarrange::run_autoarrange_checks()),
            ("F402", crate::uni1::taskmhot::run_taskmhot_checks()),
            ("F403", crate::uni1::lockhot::run_lockhot_checks()),
            ("F404", crate::uni1::explorehot::run_explorehot_checks()),
            ("F405", crate::uni1::altf4::run_altf4_checks()),
            ("F406", crate::uni1::secscr::run_secscr_checks()),
            ("F407", crate::uni1::sethot::run_sethot_checks()),
            ("F408", crate::uni1::winxmenu::run_winxmenu_checks()),
            ("F409", crate::uni1::ctxhelp::run_ctxhelp_checks()),
            ("F410", crate::uni1::enterkey::run_enterkey_checks()),
            ("F411", crate::uni1::backnav::run_backnav_checks()),
            ("F412", crate::uni1::altrprop::run_altrprop_checks()),
            ("F413", crate::uni1::prtsrc::run_prtsrc_checks()),
            ("F414", crate::uni1::dragtrash::run_dragtrash_checks()),
            ("F415", crate::uni1::trashicon::run_trashicon_checks()),
            ("F416", crate::uni1::winkey::run_winkey_checks()),
            ("F417", crate::uni1::tilegrid::run_tilegrid_checks()),
            ("F418", crate::uni1::pinbar::run_pinbar_checks()),
            ("F419", crate::uni1::barctx::run_barctx_checks()),
            ("F420", crate::uni1::clockctx::run_clockctx_checks()),
            ("F421", crate::uni1::imeind::run_imeind_checks()),
            ("F422", crate::uni1::volfly::run_volfly_checks()),
            ("F423", crate::uni1::batfly::run_batfly_checks()),
            ("F424", crate::uni1::escstack::run_escstack_checks()),
            ("F425", crate::uni1::shake::run_shake_checks()),
            ("F426", crate::uni1::docskeys::run_docskeys_checks()),
            ("F427", crate::uni1::clipkeys::run_clipkeys_checks()),
            ("F428", crate::uni1::printkey::run_printkey_checks()),
            ("F429", crate::uni1::fullscreen::run_fullscreen_checks()),
            ("F430", crate::uni1::zoomwheel::run_zoomwheel_checks()),
            ("F431", crate::uni1::newfolder::run_newfolder_checks()),
            ("F432", crate::uni1::listnav::run_listnav_checks()),
            ("F433", crate::uni1::kbdmenu::run_kbdmenu_checks()),
            ("F434", crate::uni1::dlgkeys::run_dlgkeys_checks()),
            ("F435", crate::uni1::dropdown::run_dropdown_checks()),
            ("F436", crate::uni1::sliderkeys::run_sliderkeys_checks()),
            ("F437", crate::uni1::fmtdisk::run_fmtdisk_checks()),
            ("F438", crate::uni1::diskmnt::run_diskmnt_checks()),
            ("F439", crate::uni1::drvcrypt::run_drvcrypt_checks()),
            ("F440", crate::uni1::isomount::run_isomount_checks()),
            ("F441", crate::uni1::schedtask::run_schedtask_checks()),
            ("F442", crate::uni1::restpoint::run_restpoint_checks()),
            ("F443", crate::uni1::btpair::run_btpair_checks()),
            ("F444", crate::uni1::ptrsetup::run_ptrsetup_checks()),
            ("F445", crate::uni1::disparrange::run_disparrange_checks()),
            ("F446", crate::uni1::ressel::run_ressel_checks()),
            ("F447", crate::uni1::sndaudition::run_sndaudition_checks()),
            ("F448", crate::uni1::mictest::run_mictest_checks()),
            ("F449", crate::uni1::camtest::run_camtest_checks()),
            ("F450", crate::uni1::stickkeys::run_stickkeys_checks()),
        ];
        for (tag, set) in blocks {
            let mut bad = alloc::vec::Vec::new();
            for i in 0..set.len() {
                if let Some(c) = set.get(i) {
                    if !c.passed {
                        bad.push(c.name);
                    }
                }
            }
            assert!(set.all_passed() && !set.truncated(), "[{tag}] 红项：{:?}", bad);
        }
    }
}

#[cfg(test)]
mod diag {
    #[test]
    fn print_all_red_checks() {
        let blocks: [(&str, crate::checks::CheckSet); 51] = [
            ("ubase", crate::uni1::ubase::run_ubase_checks()),
            ("F401", crate::uni1::autoarrange::run_autoarrange_checks()),
            ("F402", crate::uni1::taskmhot::run_taskmhot_checks()),
            ("F403", crate::uni1::lockhot::run_lockhot_checks()),
            ("F404", crate::uni1::explorehot::run_explorehot_checks()),
            ("F405", crate::uni1::altf4::run_altf4_checks()),
            ("F406", crate::uni1::secscr::run_secscr_checks()),
            ("F407", crate::uni1::sethot::run_sethot_checks()),
            ("F408", crate::uni1::winxmenu::run_winxmenu_checks()),
            ("F409", crate::uni1::ctxhelp::run_ctxhelp_checks()),
            ("F410", crate::uni1::enterkey::run_enterkey_checks()),
            ("F411", crate::uni1::backnav::run_backnav_checks()),
            ("F412", crate::uni1::altrprop::run_altrprop_checks()),
            ("F413", crate::uni1::prtsrc::run_prtsrc_checks()),
            ("F414", crate::uni1::dragtrash::run_dragtrash_checks()),
            ("F415", crate::uni1::trashicon::run_trashicon_checks()),
            ("F416", crate::uni1::winkey::run_winkey_checks()),
            ("F417", crate::uni1::tilegrid::run_tilegrid_checks()),
            ("F418", crate::uni1::pinbar::run_pinbar_checks()),
            ("F419", crate::uni1::barctx::run_barctx_checks()),
            ("F420", crate::uni1::clockctx::run_clockctx_checks()),
            ("F421", crate::uni1::imeind::run_imeind_checks()),
            ("F422", crate::uni1::volfly::run_volfly_checks()),
            ("F423", crate::uni1::batfly::run_batfly_checks()),
            ("F424", crate::uni1::escstack::run_escstack_checks()),
            ("F425", crate::uni1::shake::run_shake_checks()),
            ("F426", crate::uni1::docskeys::run_docskeys_checks()),
            ("F427", crate::uni1::clipkeys::run_clipkeys_checks()),
            ("F428", crate::uni1::printkey::run_printkey_checks()),
            ("F429", crate::uni1::fullscreen::run_fullscreen_checks()),
            ("F430", crate::uni1::zoomwheel::run_zoomwheel_checks()),
            ("F431", crate::uni1::newfolder::run_newfolder_checks()),
            ("F432", crate::uni1::listnav::run_listnav_checks()),
            ("F433", crate::uni1::kbdmenu::run_kbdmenu_checks()),
            ("F434", crate::uni1::dlgkeys::run_dlgkeys_checks()),
            ("F435", crate::uni1::dropdown::run_dropdown_checks()),
            ("F436", crate::uni1::sliderkeys::run_sliderkeys_checks()),
            ("F437", crate::uni1::fmtdisk::run_fmtdisk_checks()),
            ("F438", crate::uni1::diskmnt::run_diskmnt_checks()),
            ("F439", crate::uni1::drvcrypt::run_drvcrypt_checks()),
            ("F440", crate::uni1::isomount::run_isomount_checks()),
            ("F441", crate::uni1::schedtask::run_schedtask_checks()),
            ("F442", crate::uni1::restpoint::run_restpoint_checks()),
            ("F443", crate::uni1::btpair::run_btpair_checks()),
            ("F444", crate::uni1::ptrsetup::run_ptrsetup_checks()),
            ("F445", crate::uni1::disparrange::run_disparrange_checks()),
            ("F446", crate::uni1::ressel::run_ressel_checks()),
            ("F447", crate::uni1::sndaudition::run_sndaudition_checks()),
            ("F448", crate::uni1::mictest::run_mictest_checks()),
            ("F449", crate::uni1::camtest::run_camtest_checks()),
            ("F450", crate::uni1::stickkeys::run_stickkeys_checks()),
        ];
        for (tag, set) in blocks {
            for i in 0..set.len() {
                if let Some(c) = set.get(i) {
                    if !c.passed {
                        println!("RED [{}] {} — {}", tag, c.name, c.detail);
                    }
                }
            }
            if set.truncated() {
                println!("TRUNC [{}]", tag);
            }
        }
    }
}
