//! AI-U1 检查项对账机数器（对齐 C2/J2 tally 先例）。
//!
//! 用法：放入隔离验证宿主 `src/bin/tally.rs` 后 `cargo run --bin tally`。
//! 逐块打印主层 CheckSet 检查数与宿主单测数；TOTAL 行 = 对账基准。
//! 检查数从 CheckSet 内部计数读取（红绿皆计——对账只看数量与截断态）。

extern crate alloc;

#[path = "../../../../../kernel/varix/src/checks.rs"]
mod checks;

#[path = "../../../../../kernel/varix/src/uni1/mod.rs"]
mod uni1;

use checks::CheckSet;

fn block_report(tag: &str, set: &CheckSet) -> (usize, bool) {
    let n = set.len();
    let truncated = set.truncated();
    println!("{tag} checks={n} truncated={truncated}");
    (n, truncated)
}

fn main() {
    let blocks: [(&str, CheckSet); 51] = [
        ("ubase", uni1::ubase::run_ubase_checks()),
        ("F401", uni1::autoarrange::run_autoarrange_checks()),
        ("F402", uni1::taskmhot::run_taskmhot_checks()),
        ("F403", uni1::lockhot::run_lockhot_checks()),
        ("F404", uni1::explorehot::run_explorehot_checks()),
        ("F405", uni1::altf4::run_altf4_checks()),
        ("F406", uni1::secscr::run_secscr_checks()),
        ("F407", uni1::sethot::run_sethot_checks()),
        ("F408", uni1::winxmenu::run_winxmenu_checks()),
        ("F409", uni1::ctxhelp::run_ctxhelp_checks()),
        ("F410", uni1::enterkey::run_enterkey_checks()),
        ("F411", uni1::backnav::run_backnav_checks()),
        ("F412", uni1::altrprop::run_altrprop_checks()),
        ("F413", uni1::prtsrc::run_prtsrc_checks()),
        ("F414", uni1::dragtrash::run_dragtrash_checks()),
        ("F415", uni1::trashicon::run_trashicon_checks()),
        ("F416", uni1::winkey::run_winkey_checks()),
        ("F417", uni1::tilegrid::run_tilegrid_checks()),
        ("F418", uni1::pinbar::run_pinbar_checks()),
        ("F419", uni1::barctx::run_barctx_checks()),
        ("F420", uni1::clockctx::run_clockctx_checks()),
        ("F421", uni1::imeind::run_imeind_checks()),
        ("F422", uni1::volfly::run_volfly_checks()),
        ("F423", uni1::batfly::run_batfly_checks()),
        ("F424", uni1::escstack::run_escstack_checks()),
        ("F425", uni1::shake::run_shake_checks()),
        ("F426", uni1::docskeys::run_docskeys_checks()),
        ("F427", uni1::clipkeys::run_clipkeys_checks()),
        ("F428", uni1::printkey::run_printkey_checks()),
        ("F429", uni1::fullscreen::run_fullscreen_checks()),
        ("F430", uni1::zoomwheel::run_zoomwheel_checks()),
        ("F431", uni1::newfolder::run_newfolder_checks()),
        ("F432", uni1::listnav::run_listnav_checks()),
        ("F433", uni1::kbdmenu::run_kbdmenu_checks()),
        ("F434", uni1::dlgkeys::run_dlgkeys_checks()),
        ("F435", uni1::dropdown::run_dropdown_checks()),
        ("F436", uni1::sliderkeys::run_sliderkeys_checks()),
        ("F437", uni1::fmtdisk::run_fmtdisk_checks()),
        ("F438", uni1::diskmnt::run_diskmnt_checks()),
        ("F439", uni1::drvcrypt::run_drvcrypt_checks()),
        ("F440", uni1::isomount::run_isomount_checks()),
        ("F441", uni1::schedtask::run_schedtask_checks()),
        ("F442", uni1::restpoint::run_restpoint_checks()),
        ("F443", uni1::btpair::run_btpair_checks()),
        ("F444", uni1::ptrsetup::run_ptrsetup_checks()),
        ("F445", uni1::disparrange::run_disparrange_checks()),
        ("F446", uni1::ressel::run_ressel_checks()),
        ("F447", uni1::sndaudition::run_sndaudition_checks()),
        ("F448", uni1::mictest::run_mictest_checks()),
        ("F449", uni1::camtest::run_camtest_checks()),
        ("F450", uni1::stickkeys::run_stickkeys_checks()),
    ];
    let mut total = 0usize;
    let mut red = 0usize;
    for (tag, set) in &blocks {
        let (n, _trunc) = block_report(tag, set);
        total += n;
        if !set.all_passed() {
            red += 1;
            println!("  !! {tag} HAS RED CHECKS");
        }
    }
    println!("TOTAL blocks={} checks={total} red_blocks={red} capacity=64 truncation=none(if blocks<=64)", blocks.len());
}
