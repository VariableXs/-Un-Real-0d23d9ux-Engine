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
        fn diag_tag(t: &str) { let _ = t; }
        let mut blocks: alloc::vec::Vec<(&'static str, crate::checks::CheckSet)> = alloc::vec::Vec::new();
        blocks.push(("ubase", crate::uni1::ubase::run_ubase_checks()));
        blocks.push(("F401", crate::uni1::autoarrange::run_autoarrange_checks()));
        blocks.push(("F402", crate::uni1::taskmhot::run_taskmhot_checks()));
        blocks.push(("F403", crate::uni1::lockhot::run_lockhot_checks()));
        blocks.push(("F404", crate::uni1::explorehot::run_explorehot_checks()));
        blocks.push(("F405", crate::uni1::altf4::run_altf4_checks()));
        blocks.push(("F406", crate::uni1::secscr::run_secscr_checks()));
        blocks.push(("F407", crate::uni1::sethot::run_sethot_checks()));
        blocks.push(("F408", crate::uni1::winxmenu::run_winxmenu_checks()));
        blocks.push(("F409", crate::uni1::ctxhelp::run_ctxhelp_checks()));
        blocks.push(("F410", crate::uni1::enterkey::run_enterkey_checks()));
        blocks.push(("F411", crate::uni1::backnav::run_backnav_checks()));
        blocks.push(("F412", crate::uni1::altrprop::run_altrprop_checks()));
        blocks.push(("F413", crate::uni1::prtsrc::run_prtsrc_checks()));
        blocks.push(("F414", crate::uni1::dragtrash::run_dragtrash_checks()));
        blocks.push(("F415", crate::uni1::trashicon::run_trashicon_checks()));
        blocks.push(("F416", crate::uni1::winkey::run_winkey_checks()));
        blocks.push(("F417", crate::uni1::tilegrid::run_tilegrid_checks()));
        blocks.push(("F418", crate::uni1::pinbar::run_pinbar_checks()));
        blocks.push(("F419", crate::uni1::barctx::run_barctx_checks()));
        blocks.push(("F420", crate::uni1::clockctx::run_clockctx_checks()));
        blocks.push(("F421", crate::uni1::imeind::run_imeind_checks()));
        blocks.push(("F422", crate::uni1::volfly::run_volfly_checks()));
        blocks.push(("F423", crate::uni1::batfly::run_batfly_checks()));
        blocks.push(("F424", crate::uni1::escstack::run_escstack_checks()));
        blocks.push(("F425", crate::uni1::shake::run_shake_checks()));
        blocks.push(("F426", crate::uni1::docskeys::run_docskeys_checks()));
        blocks.push(("F427", crate::uni1::clipkeys::run_clipkeys_checks()));
        blocks.push(("F428", crate::uni1::printkey::run_printkey_checks()));
        blocks.push(("F429", crate::uni1::fullscreen::run_fullscreen_checks()));
        blocks.push(("F430", crate::uni1::zoomwheel::run_zoomwheel_checks()));
        blocks.push(("F431", crate::uni1::newfolder::run_newfolder_checks()));
        blocks.push(("F432", crate::uni1::listnav::run_listnav_checks()));
        blocks.push(("F433", crate::uni1::kbdmenu::run_kbdmenu_checks()));
        blocks.push(("F434", crate::uni1::dlgkeys::run_dlgkeys_checks()));
        blocks.push(("F435", crate::uni1::dropdown::run_dropdown_checks()));
        blocks.push(("F436", crate::uni1::sliderkeys::run_sliderkeys_checks()));
        blocks.push(("F437", crate::uni1::fmtdisk::run_fmtdisk_checks()));
        blocks.push(("F438", crate::uni1::diskmnt::run_diskmnt_checks()));
        blocks.push(("F439", crate::uni1::drvcrypt::run_drvcrypt_checks()));
        blocks.push(("F440", crate::uni1::isomount::run_isomount_checks()));
        blocks.push(("F441", crate::uni1::schedtask::run_schedtask_checks()));
        blocks.push(("F442", crate::uni1::restpoint::run_restpoint_checks()));
        blocks.push(("F443", crate::uni1::btpair::run_btpair_checks()));
        blocks.push(("F444", crate::uni1::ptrsetup::run_ptrsetup_checks()));
        blocks.push(("F445", crate::uni1::disparrange::run_disparrange_checks()));
        blocks.push(("F446", crate::uni1::ressel::run_ressel_checks()));
        blocks.push(("F447", crate::uni1::sndaudition::run_sndaudition_checks()));
        blocks.push(("F448", crate::uni1::mictest::run_mictest_checks()));
        blocks.push(("F449", crate::uni1::camtest::run_camtest_checks()));
        blocks.push(("F450", crate::uni1::stickkeys::run_stickkeys_checks()));
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
            let mut blocks: alloc::vec::Vec<(&'static str, crate::checks::CheckSet)> = alloc::vec::Vec::new();
        blocks.push(("ubase", crate::uni1::ubase::run_ubase_checks()));
        blocks.push(("F401", crate::uni1::autoarrange::run_autoarrange_checks()));
        blocks.push(("F402", crate::uni1::taskmhot::run_taskmhot_checks()));
        blocks.push(("F403", crate::uni1::lockhot::run_lockhot_checks()));
        blocks.push(("F404", crate::uni1::explorehot::run_explorehot_checks()));
        blocks.push(("F405", crate::uni1::altf4::run_altf4_checks()));
        blocks.push(("F406", crate::uni1::secscr::run_secscr_checks()));
        blocks.push(("F407", crate::uni1::sethot::run_sethot_checks()));
        blocks.push(("F408", crate::uni1::winxmenu::run_winxmenu_checks()));
        blocks.push(("F409", crate::uni1::ctxhelp::run_ctxhelp_checks()));
        blocks.push(("F410", crate::uni1::enterkey::run_enterkey_checks()));
        blocks.push(("F411", crate::uni1::backnav::run_backnav_checks()));
        blocks.push(("F412", crate::uni1::altrprop::run_altrprop_checks()));
        blocks.push(("F413", crate::uni1::prtsrc::run_prtsrc_checks()));
        blocks.push(("F414", crate::uni1::dragtrash::run_dragtrash_checks()));
        blocks.push(("F415", crate::uni1::trashicon::run_trashicon_checks()));
        blocks.push(("F416", crate::uni1::winkey::run_winkey_checks()));
        blocks.push(("F417", crate::uni1::tilegrid::run_tilegrid_checks()));
        blocks.push(("F418", crate::uni1::pinbar::run_pinbar_checks()));
        blocks.push(("F419", crate::uni1::barctx::run_barctx_checks()));
        blocks.push(("F420", crate::uni1::clockctx::run_clockctx_checks()));
        blocks.push(("F421", crate::uni1::imeind::run_imeind_checks()));
        blocks.push(("F422", crate::uni1::volfly::run_volfly_checks()));
        blocks.push(("F423", crate::uni1::batfly::run_batfly_checks()));
        blocks.push(("F424", crate::uni1::escstack::run_escstack_checks()));
        blocks.push(("F425", crate::uni1::shake::run_shake_checks()));
        blocks.push(("F426", crate::uni1::docskeys::run_docskeys_checks()));
        blocks.push(("F427", crate::uni1::clipkeys::run_clipkeys_checks()));
        blocks.push(("F428", crate::uni1::printkey::run_printkey_checks()));
        blocks.push(("F429", crate::uni1::fullscreen::run_fullscreen_checks()));
        blocks.push(("F430", crate::uni1::zoomwheel::run_zoomwheel_checks()));
        blocks.push(("F431", crate::uni1::newfolder::run_newfolder_checks()));
        blocks.push(("F432", crate::uni1::listnav::run_listnav_checks()));
        blocks.push(("F433", crate::uni1::kbdmenu::run_kbdmenu_checks()));
        blocks.push(("F434", crate::uni1::dlgkeys::run_dlgkeys_checks()));
        blocks.push(("F435", crate::uni1::dropdown::run_dropdown_checks()));
        blocks.push(("F436", crate::uni1::sliderkeys::run_sliderkeys_checks()));
        blocks.push(("F437", crate::uni1::fmtdisk::run_fmtdisk_checks()));
        blocks.push(("F438", crate::uni1::diskmnt::run_diskmnt_checks()));
        blocks.push(("F439", crate::uni1::drvcrypt::run_drvcrypt_checks()));
        blocks.push(("F440", crate::uni1::isomount::run_isomount_checks()));
        blocks.push(("F441", crate::uni1::schedtask::run_schedtask_checks()));
        blocks.push(("F442", crate::uni1::restpoint::run_restpoint_checks()));
        blocks.push(("F443", crate::uni1::btpair::run_btpair_checks()));
        blocks.push(("F444", crate::uni1::ptrsetup::run_ptrsetup_checks()));
        blocks.push(("F445", crate::uni1::disparrange::run_disparrange_checks()));
        blocks.push(("F446", crate::uni1::ressel::run_ressel_checks()));
        blocks.push(("F447", crate::uni1::sndaudition::run_sndaudition_checks()));
        blocks.push(("F448", crate::uni1::mictest::run_mictest_checks()));
        blocks.push(("F449", crate::uni1::camtest::run_camtest_checks()));
        blocks.push(("F450", crate::uni1::stickkeys::run_stickkeys_checks()));
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
#[cfg(test)]
mod diag_seq {
    #[test]
    fn seq() {
        core::hint::black_box(println!("ENTER ubase"));
        let s = crate::uni1::ubase::run_ubase_checks();
        core::hint::black_box(assert!(s.all_passed(), "ubase red"));
        core::hint::black_box(println!("ENTER F401"));
        let s = crate::uni1::autoarrange::run_autoarrange_checks();
        core::hint::black_box(assert!(s.all_passed(), "F401 red"));
        core::hint::black_box(println!("ENTER F402"));
        let s = crate::uni1::taskmhot::run_taskmhot_checks();
        core::hint::black_box(assert!(s.all_passed(), "F402 red"));
        core::hint::black_box(println!("ENTER F403"));
        let s = crate::uni1::lockhot::run_lockhot_checks();
        core::hint::black_box(assert!(s.all_passed(), "F403 red"));
        core::hint::black_box(println!("ENTER F404"));
        let s = crate::uni1::explorehot::run_explorehot_checks();
        core::hint::black_box(assert!(s.all_passed(), "F404 red"));
        core::hint::black_box(println!("ENTER F405"));
        let s = crate::uni1::altf4::run_altf4_checks();
        core::hint::black_box(assert!(s.all_passed(), "F405 red"));
        core::hint::black_box(println!("ENTER F406"));
        let s = crate::uni1::secscr::run_secscr_checks();
        core::hint::black_box(assert!(s.all_passed(), "F406 red"));
        core::hint::black_box(println!("ENTER F407"));
        let s = crate::uni1::sethot::run_sethot_checks();
        core::hint::black_box(assert!(s.all_passed(), "F407 red"));
        core::hint::black_box(println!("ENTER F408"));
        let s = crate::uni1::winxmenu::run_winxmenu_checks();
        core::hint::black_box(assert!(s.all_passed(), "F408 red"));
        core::hint::black_box(println!("ENTER F409"));
        let s = crate::uni1::ctxhelp::run_ctxhelp_checks();
        core::hint::black_box(assert!(s.all_passed(), "F409 red"));
        core::hint::black_box(println!("ENTER F410"));
        let s = crate::uni1::enterkey::run_enterkey_checks();
        core::hint::black_box(assert!(s.all_passed(), "F410 red"));
        core::hint::black_box(println!("ENTER F411"));
        let s = crate::uni1::backnav::run_backnav_checks();
        core::hint::black_box(assert!(s.all_passed(), "F411 red"));
        core::hint::black_box(println!("ENTER F412"));
        let s = crate::uni1::altrprop::run_altrprop_checks();
        core::hint::black_box(assert!(s.all_passed(), "F412 red"));
        core::hint::black_box(println!("ENTER F413"));
        let s = crate::uni1::prtsrc::run_prtsrc_checks();
        core::hint::black_box(assert!(s.all_passed(), "F413 red"));
        core::hint::black_box(println!("ENTER F414"));
        let s = crate::uni1::dragtrash::run_dragtrash_checks();
        core::hint::black_box(assert!(s.all_passed(), "F414 red"));
        core::hint::black_box(println!("ENTER F415"));
        let s = crate::uni1::trashicon::run_trashicon_checks();
        core::hint::black_box(assert!(s.all_passed(), "F415 red"));
        core::hint::black_box(println!("ENTER F416"));
        let s = crate::uni1::winkey::run_winkey_checks();
        core::hint::black_box(assert!(s.all_passed(), "F416 red"));
        core::hint::black_box(println!("ENTER F417"));
        let s = crate::uni1::tilegrid::run_tilegrid_checks();
        core::hint::black_box(assert!(s.all_passed(), "F417 red"));
        core::hint::black_box(println!("ENTER F418"));
        let s = crate::uni1::pinbar::run_pinbar_checks();
        core::hint::black_box(assert!(s.all_passed(), "F418 red"));
        core::hint::black_box(println!("ENTER F419"));
        let s = crate::uni1::barctx::run_barctx_checks();
        core::hint::black_box(assert!(s.all_passed(), "F419 red"));
        core::hint::black_box(println!("ENTER F420"));
        let s = crate::uni1::clockctx::run_clockctx_checks();
        core::hint::black_box(assert!(s.all_passed(), "F420 red"));
        core::hint::black_box(println!("ENTER F421"));
        let s = crate::uni1::imeind::run_imeind_checks();
        core::hint::black_box(assert!(s.all_passed(), "F421 red"));
        core::hint::black_box(println!("ENTER F422"));
        let s = crate::uni1::volfly::run_volfly_checks();
        core::hint::black_box(assert!(s.all_passed(), "F422 red"));
        core::hint::black_box(println!("ENTER F423"));
        let s = crate::uni1::batfly::run_batfly_checks();
        core::hint::black_box(assert!(s.all_passed(), "F423 red"));
        core::hint::black_box(println!("ENTER F424"));
        let s = crate::uni1::escstack::run_escstack_checks();
        core::hint::black_box(assert!(s.all_passed(), "F424 red"));
        core::hint::black_box(println!("ENTER F425"));
        let s = crate::uni1::shake::run_shake_checks();
        core::hint::black_box(assert!(s.all_passed(), "F425 red"));
        core::hint::black_box(println!("ENTER F426"));
        let s = crate::uni1::docskeys::run_docskeys_checks();
        core::hint::black_box(assert!(s.all_passed(), "F426 red"));
        core::hint::black_box(println!("ENTER F427"));
        let s = crate::uni1::clipkeys::run_clipkeys_checks();
        core::hint::black_box(assert!(s.all_passed(), "F427 red"));
        core::hint::black_box(println!("ENTER F428"));
        let s = crate::uni1::printkey::run_printkey_checks();
        core::hint::black_box(assert!(s.all_passed(), "F428 red"));
        core::hint::black_box(println!("ENTER F429"));
        let s = crate::uni1::fullscreen::run_fullscreen_checks();
        core::hint::black_box(assert!(s.all_passed(), "F429 red"));
        core::hint::black_box(println!("ENTER F430"));
        let s = crate::uni1::zoomwheel::run_zoomwheel_checks();
        core::hint::black_box(assert!(s.all_passed(), "F430 red"));
        core::hint::black_box(println!("ENTER F431"));
        let s = crate::uni1::newfolder::run_newfolder_checks();
        core::hint::black_box(assert!(s.all_passed(), "F431 red"));
        core::hint::black_box(println!("ENTER F432"));
        let s = crate::uni1::listnav::run_listnav_checks();
        core::hint::black_box(assert!(s.all_passed(), "F432 red"));
        core::hint::black_box(println!("ENTER F433"));
        let s = crate::uni1::kbdmenu::run_kbdmenu_checks();
        core::hint::black_box(assert!(s.all_passed(), "F433 red"));
        core::hint::black_box(println!("ENTER F434"));
        let s = crate::uni1::dlgkeys::run_dlgkeys_checks();
        core::hint::black_box(assert!(s.all_passed(), "F434 red"));
        core::hint::black_box(println!("ENTER F435"));
        let s = crate::uni1::dropdown::run_dropdown_checks();
        core::hint::black_box(assert!(s.all_passed(), "F435 red"));
        core::hint::black_box(println!("ENTER F436"));
        let s = crate::uni1::sliderkeys::run_sliderkeys_checks();
        core::hint::black_box(assert!(s.all_passed(), "F436 red"));
        core::hint::black_box(println!("ENTER F437"));
        let s = crate::uni1::fmtdisk::run_fmtdisk_checks();
        core::hint::black_box(assert!(s.all_passed(), "F437 red"));
        core::hint::black_box(println!("ENTER F438"));
        let s = crate::uni1::diskmnt::run_diskmnt_checks();
        core::hint::black_box(assert!(s.all_passed(), "F438 red"));
        core::hint::black_box(println!("ENTER F439"));
        let s = crate::uni1::drvcrypt::run_drvcrypt_checks();
        core::hint::black_box(assert!(s.all_passed(), "F439 red"));
        core::hint::black_box(println!("ENTER F440"));
        let s = crate::uni1::isomount::run_isomount_checks();
        core::hint::black_box(assert!(s.all_passed(), "F440 red"));
        core::hint::black_box(println!("ENTER F441"));
        let s = crate::uni1::schedtask::run_schedtask_checks();
        core::hint::black_box(assert!(s.all_passed(), "F441 red"));
        core::hint::black_box(println!("ENTER F442"));
        let s = crate::uni1::restpoint::run_restpoint_checks();
        core::hint::black_box(assert!(s.all_passed(), "F442 red"));
        core::hint::black_box(println!("ENTER F443"));
        let s = crate::uni1::btpair::run_btpair_checks();
        core::hint::black_box(assert!(s.all_passed(), "F443 red"));
        core::hint::black_box(println!("ENTER F444"));
        let s = crate::uni1::ptrsetup::run_ptrsetup_checks();
        core::hint::black_box(assert!(s.all_passed(), "F444 red"));
        core::hint::black_box(println!("ENTER F445"));
        let s = crate::uni1::disparrange::run_disparrange_checks();
        core::hint::black_box(assert!(s.all_passed(), "F445 red"));
        core::hint::black_box(println!("ENTER F446"));
        let s = crate::uni1::ressel::run_ressel_checks();
        core::hint::black_box(assert!(s.all_passed(), "F446 red"));
        core::hint::black_box(println!("ENTER F447"));
        let s = crate::uni1::sndaudition::run_sndaudition_checks();
        core::hint::black_box(assert!(s.all_passed(), "F447 red"));
        core::hint::black_box(println!("ENTER F448"));
        let s = crate::uni1::mictest::run_mictest_checks();
        core::hint::black_box(assert!(s.all_passed(), "F448 red"));
        core::hint::black_box(println!("ENTER F449"));
        let s = crate::uni1::camtest::run_camtest_checks();
        core::hint::black_box(assert!(s.all_passed(), "F449 red"));
        core::hint::black_box(println!("ENTER F450"));
        let s = crate::uni1::stickkeys::run_stickkeys_checks();
        core::hint::black_box(assert!(s.all_passed(), "F450 red"));
        core::hint::black_box(println!("ENTER ubase"));
        let s = crate::uni1::ubase::run_ubase_checks();
        core::hint::black_box(assert!(s.all_passed(), "ubase red"));
        core::hint::black_box(println!("ENTER F401"));
        let s = crate::uni1::autoarrange::run_autoarrange_checks();
        core::hint::black_box(assert!(s.all_passed(), "F401 red"));
        core::hint::black_box(println!("ENTER F402"));
        let s = crate::uni1::taskmhot::run_taskmhot_checks();
        core::hint::black_box(assert!(s.all_passed(), "F402 red"));
        core::hint::black_box(println!("ENTER F403"));
        let s = crate::uni1::lockhot::run_lockhot_checks();
        core::hint::black_box(assert!(s.all_passed(), "F403 red"));
        core::hint::black_box(println!("ENTER F404"));
        let s = crate::uni1::explorehot::run_explorehot_checks();
        core::hint::black_box(assert!(s.all_passed(), "F404 red"));
        core::hint::black_box(println!("ENTER F405"));
        let s = crate::uni1::altf4::run_altf4_checks();
        core::hint::black_box(assert!(s.all_passed(), "F405 red"));
        core::hint::black_box(println!("ENTER F406"));
        let s = crate::uni1::secscr::run_secscr_checks();
        core::hint::black_box(assert!(s.all_passed(), "F406 red"));
        core::hint::black_box(println!("ENTER F407"));
        let s = crate::uni1::sethot::run_sethot_checks();
        core::hint::black_box(assert!(s.all_passed(), "F407 red"));
        core::hint::black_box(println!("ENTER F408"));
        let s = crate::uni1::winxmenu::run_winxmenu_checks();
        core::hint::black_box(assert!(s.all_passed(), "F408 red"));
        core::hint::black_box(println!("ENTER F409"));
        let s = crate::uni1::ctxhelp::run_ctxhelp_checks();
        core::hint::black_box(assert!(s.all_passed(), "F409 red"));
        core::hint::black_box(println!("ENTER F410"));
        let s = crate::uni1::enterkey::run_enterkey_checks();
        core::hint::black_box(assert!(s.all_passed(), "F410 red"));
        core::hint::black_box(println!("ENTER F411"));
        let s = crate::uni1::backnav::run_backnav_checks();
        core::hint::black_box(assert!(s.all_passed(), "F411 red"));
        core::hint::black_box(println!("ENTER F412"));
        let s = crate::uni1::altrprop::run_altrprop_checks();
        core::hint::black_box(assert!(s.all_passed(), "F412 red"));
        core::hint::black_box(println!("ENTER F413"));
        let s = crate::uni1::prtsrc::run_prtsrc_checks();
        core::hint::black_box(assert!(s.all_passed(), "F413 red"));
        core::hint::black_box(println!("ENTER F414"));
        let s = crate::uni1::dragtrash::run_dragtrash_checks();
        core::hint::black_box(assert!(s.all_passed(), "F414 red"));
        core::hint::black_box(println!("ENTER F415"));
        let s = crate::uni1::trashicon::run_trashicon_checks();
        core::hint::black_box(assert!(s.all_passed(), "F415 red"));
        core::hint::black_box(println!("ENTER F416"));
        let s = crate::uni1::winkey::run_winkey_checks();
        core::hint::black_box(assert!(s.all_passed(), "F416 red"));
        core::hint::black_box(println!("ENTER F417"));
        let s = crate::uni1::tilegrid::run_tilegrid_checks();
        core::hint::black_box(assert!(s.all_passed(), "F417 red"));
        core::hint::black_box(println!("ENTER F418"));
        let s = crate::uni1::pinbar::run_pinbar_checks();
        core::hint::black_box(assert!(s.all_passed(), "F418 red"));
        core::hint::black_box(println!("ENTER F419"));
        let s = crate::uni1::barctx::run_barctx_checks();
        core::hint::black_box(assert!(s.all_passed(), "F419 red"));
        core::hint::black_box(println!("ENTER F420"));
        let s = crate::uni1::clockctx::run_clockctx_checks();
        core::hint::black_box(assert!(s.all_passed(), "F420 red"));
        core::hint::black_box(println!("ENTER F421"));
        let s = crate::uni1::imeind::run_imeind_checks();
        core::hint::black_box(assert!(s.all_passed(), "F421 red"));
        core::hint::black_box(println!("ENTER F422"));
        let s = crate::uni1::volfly::run_volfly_checks();
        core::hint::black_box(assert!(s.all_passed(), "F422 red"));
        core::hint::black_box(println!("ENTER F423"));
        let s = crate::uni1::batfly::run_batfly_checks();
        core::hint::black_box(assert!(s.all_passed(), "F423 red"));
        core::hint::black_box(println!("ENTER F424"));
        let s = crate::uni1::escstack::run_escstack_checks();
        core::hint::black_box(assert!(s.all_passed(), "F424 red"));
        core::hint::black_box(println!("ENTER F425"));
        let s = crate::uni1::shake::run_shake_checks();
        core::hint::black_box(assert!(s.all_passed(), "F425 red"));
        core::hint::black_box(println!("ENTER F426"));
        let s = crate::uni1::docskeys::run_docskeys_checks();
        core::hint::black_box(assert!(s.all_passed(), "F426 red"));
        core::hint::black_box(println!("ENTER F427"));
        let s = crate::uni1::clipkeys::run_clipkeys_checks();
        core::hint::black_box(assert!(s.all_passed(), "F427 red"));
        core::hint::black_box(println!("ENTER F428"));
        let s = crate::uni1::printkey::run_printkey_checks();
        core::hint::black_box(assert!(s.all_passed(), "F428 red"));
        core::hint::black_box(println!("ENTER F429"));
        let s = crate::uni1::fullscreen::run_fullscreen_checks();
        core::hint::black_box(assert!(s.all_passed(), "F429 red"));
        core::hint::black_box(println!("ENTER F430"));
        let s = crate::uni1::zoomwheel::run_zoomwheel_checks();
        core::hint::black_box(assert!(s.all_passed(), "F430 red"));
        core::hint::black_box(println!("ENTER F431"));
        let s = crate::uni1::newfolder::run_newfolder_checks();
        core::hint::black_box(assert!(s.all_passed(), "F431 red"));
        core::hint::black_box(println!("ENTER F432"));
        let s = crate::uni1::listnav::run_listnav_checks();
        core::hint::black_box(assert!(s.all_passed(), "F432 red"));
        core::hint::black_box(println!("ENTER F433"));
        let s = crate::uni1::kbdmenu::run_kbdmenu_checks();
        core::hint::black_box(assert!(s.all_passed(), "F433 red"));
        core::hint::black_box(println!("ENTER F434"));
        let s = crate::uni1::dlgkeys::run_dlgkeys_checks();
        core::hint::black_box(assert!(s.all_passed(), "F434 red"));
        core::hint::black_box(println!("ENTER F435"));
        let s = crate::uni1::dropdown::run_dropdown_checks();
        core::hint::black_box(assert!(s.all_passed(), "F435 red"));
        core::hint::black_box(println!("ENTER F436"));
        let s = crate::uni1::sliderkeys::run_sliderkeys_checks();
        core::hint::black_box(assert!(s.all_passed(), "F436 red"));
        core::hint::black_box(println!("ENTER F437"));
        let s = crate::uni1::fmtdisk::run_fmtdisk_checks();
        core::hint::black_box(assert!(s.all_passed(), "F437 red"));
        core::hint::black_box(println!("ENTER F438"));
        let s = crate::uni1::diskmnt::run_diskmnt_checks();
        core::hint::black_box(assert!(s.all_passed(), "F438 red"));
        core::hint::black_box(println!("ENTER F439"));
        let s = crate::uni1::drvcrypt::run_drvcrypt_checks();
        core::hint::black_box(assert!(s.all_passed(), "F439 red"));
        core::hint::black_box(println!("ENTER F440"));
        let s = crate::uni1::isomount::run_isomount_checks();
        core::hint::black_box(assert!(s.all_passed(), "F440 red"));
        core::hint::black_box(println!("ENTER F441"));
        let s = crate::uni1::schedtask::run_schedtask_checks();
        core::hint::black_box(assert!(s.all_passed(), "F441 red"));
        core::hint::black_box(println!("ENTER F442"));
        let s = crate::uni1::restpoint::run_restpoint_checks();
        core::hint::black_box(assert!(s.all_passed(), "F442 red"));
        core::hint::black_box(println!("ENTER F443"));
        let s = crate::uni1::btpair::run_btpair_checks();
        core::hint::black_box(assert!(s.all_passed(), "F443 red"));
        core::hint::black_box(println!("ENTER F444"));
        let s = crate::uni1::ptrsetup::run_ptrsetup_checks();
        core::hint::black_box(assert!(s.all_passed(), "F444 red"));
        core::hint::black_box(println!("ENTER F445"));
        let s = crate::uni1::disparrange::run_disparrange_checks();
        core::hint::black_box(assert!(s.all_passed(), "F445 red"));
        core::hint::black_box(println!("ENTER F446"));
        let s = crate::uni1::ressel::run_ressel_checks();
        core::hint::black_box(assert!(s.all_passed(), "F446 red"));
        core::hint::black_box(println!("ENTER F447"));
        let s = crate::uni1::sndaudition::run_sndaudition_checks();
        core::hint::black_box(assert!(s.all_passed(), "F447 red"));
        core::hint::black_box(println!("ENTER F448"));
        let s = crate::uni1::mictest::run_mictest_checks();
        core::hint::black_box(assert!(s.all_passed(), "F448 red"));
        core::hint::black_box(println!("ENTER F449"));
        let s = crate::uni1::camtest::run_camtest_checks();
        core::hint::black_box(assert!(s.all_passed(), "F449 red"));
        core::hint::black_box(println!("ENTER F450"));
        let s = crate::uni1::stickkeys::run_stickkeys_checks();
        core::hint::black_box(assert!(s.all_passed(), "F450 red"));
    }
}
