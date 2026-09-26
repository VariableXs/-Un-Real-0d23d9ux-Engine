
// ===========================================================================
// 隔离舱诊断件（临时，不回主工作区）：逐项打印 CheckSet 红项名。
// ===========================================================================
#[cfg(test)]
mod reddiag {
    fn dump(name: &str, set: &crate::checks::CheckSet) {
        let (p, f) = set.tally();
        if f == 0 && !set.truncated() {
            println!("GREEN {} ({} items)", name, set.len());
            return;
        }
        println!("RED   {} ({}/{} green, truncated={})", name, p, p + f, set.truncated());
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    println!("      RED-ITEM: {}", c.name);
                }
            }
        }
    }

    #[test]
    fn dump_h1_all_checksets() {
        dump("F201", &super::textsel::run_textsel_checks());
        dump("F201v2", &super::textsel::run_textsel_v2_checks());
        dump("F202", &super::undoframe::run_undoframe_checks());
        dump("F202v2", &super::undoframe::run_undoframe_v2_checks());
        dump("F203", &super::rubbersel::run_rubbersel_checks());
        dump("F203v2", &super::rubbersel::run_rubbersel_v2_checks());
        dump("F204", &super::scrolluni::run_scrolluni_checks());
        dump("F204v2", &super::scrolluni::run_scrolluni_v2_checks());
        dump("F205", &super::tipsys::run_tipsys_checks());
        dump("F205v2", &super::tipsys::run_tipsys_v2_checks());
        dump("F206", &super::focusnav::run_focusnav_checks());
        dump("F206v2", &super::focusnav::run_focusnav_v2_checks());
        dump("F207", &super::dialksem::run_dialksem_checks());
        dump("F207v2", &super::dialksem::run_dialksem_v2_checks());
        dump("F208", &super::progfeed::run_progfeed_checks());
        dump("F208v2", &super::progfeed::run_progfeed_v2_checks());
        dump("F209", &super::errthree::run_errthree_checks());
        dump("F209v2", &super::errthree::run_errthree_v2_checks());
        dump("F210", &super::emptystate::run_emptystate_checks());
        dump("F210v2", &super::emptystate::run_emptystate_v2_checks());
        dump("F211", &super::edkeys::run_edkeys_checks());
        dump("F211v2", &super::edkeys::run_edkeys_v2_checks());
        dump("F212", &super::drophl::run_drophl_checks());
        dump("F212v2", &super::drophl::run_drophl_v2_checks());
        dump("F213", &super::titlebar::run_titlebar_checks());
        dump("F213v2", &super::titlebar::run_titlebar_v2_checks());
        dump("F214", &super::winsize::run_winsize_checks());
        dump("F214v2", &super::winsize::run_winsize_v2_checks());
        dump("F215", &super::menulev::run_menulev_checks());
        dump("F215v2", &super::menulev::run_menulev_v2_checks());
        dump("F216", &super::triwidget::run_triwidget_checks());
        dump("F216v2", &super::triwidget::run_triwidget_v2_checks());
        dump("F217", &super::numspin::run_numspin_checks());
        dump("F217v2", &super::numspin::run_numspin_v2_checks());
        dump("F218", &super::listsel::run_listsel_checks());
        dump("F218v2", &super::listsel::run_listsel_v2_checks());
        dump("F219", &super::viewmem::run_viewmem_checks());
        dump("F219v2", &super::viewmem::run_viewmem_v2_checks());
        dump("F220", &super::pasteplain::run_pasteplain_checks());
        dump("F220v2", &super::pasteplain::run_pasteplain_v2_checks());
        dump("F221", &super::findbar::run_findbar_checks());
        dump("F221v2", &super::findbar::run_findbar_v2_checks());
        dump("F222", &super::fontrnd::run_fontrnd_checks());
        dump("F222v2", &super::fontrnd::run_fontrnd_v2_checks());
        dump("F223", &super::caret::run_caret_checks());
        dump("F223v2", &super::caret::run_caret_v2_checks());
        dump("F224", &super::dpiscale::run_dpiscale_checks());
        dump("F224v2", &super::dpiscale::run_dpiscale_v2_checks());
        dump("F225", &super::themeswap::run_themeswap_checks());
        dump("F225v2", &super::themeswap::run_themeswap_v2_checks());
        dump("F226", &super::winlayer::run_winlayer_checks());
        dump("F226v2", &super::winlayer::run_winlayer_v2_checks());
        dump("F227", &super::winanim::run_winanim_checks());
        dump("F227v2", &super::winanim::run_winanim_v2_checks());
        dump("F228", &super::vlist::run_vlist_checks());
        dump("F228v2", &super::vlist::run_vlist_v2_checks());
        dump("F229", &super::fieldui::run_fieldui_checks());
        dump("F229v2", &super::fieldui::run_fieldui_v2_checks());
        dump("F230", &super::pwdeye::run_pwdeye_checks());
        dump("F230v2", &super::pwdeye::run_pwdeye_v2_checks());
        dump("F231", &super::formval::run_formval_checks());
        dump("F231v2", &super::formval::run_formval_v2_checks());
        dump("F232", &super::datepick::run_datepick_checks());
        dump("F232v2", &super::datepick::run_datepick_v2_checks());
        dump("F233", &super::filedlg::run_filedlg_checks());
        dump("F233v2", &super::filedlg::run_filedlg_v2_checks());
        dump("F234", &super::colorpick::run_colorpick_checks());
        dump("F234v2", &super::colorpick::run_colorpick_v2_checks());
        dump("F235", &super::vdesk::run_vdesk_checks());
        dump("F235v2", &super::vdesk::run_vdesk_v2_checks());
        dump("F236", &super::winsnap::run_winsnap_checks());
        dump("F236v2", &super::winsnap::run_winsnap_v2_checks());
        dump("F237", &super::winmem::run_winmem_checks());
        dump("F237v2", &super::winmem::run_winmem_v2_checks());
        dump("F238", &super::lockui::run_lockui_checks());
        dump("F238v2", &super::lockui::run_lockui_v2_checks());
        dump("F239", &super::brightosd::run_brightosd_checks());
        dump("F239v2", &super::brightosd::run_brightosd_v2_checks());
        dump("F240", &super::volosd::run_volosd_checks());
        dump("F240v2", &super::volosd::run_volosd_v2_checks());
        dump("F241", &super::audioroute::run_audioroute_checks());
        dump("F241v2", &super::audioroute::run_audioroute_v2_checks());
        dump("F242", &super::netstate::run_netstate_checks());
        dump("F242v2", &super::netstate::run_netstate_v2_checks());
        dump("F243", &super::hidfiles::run_hidfiles_checks());
        dump("F243v2", &super::hidfiles::run_hidfiles_v2_checks());
        dump("F244", &super::hotkeyreg::run_hotkeyreg_checks());
        dump("F244v2", &super::hotkeyreg::run_hotkeyreg_v2_checks());
        dump("F245", &super::lessmotion::run_lessmotion_checks());
        dump("F245v2", &super::lessmotion::run_lessmotion_v2_checks());
        dump("F246", &super::textscale::run_textscale_checks());
        dump("F246v2", &super::textscale::run_textscale_v2_checks());
        dump("F247", &super::texttrunc::run_texttrunc_checks());
        dump("F247v2", &super::texttrunc::run_texttrunc_v2_checks());
        dump("F248", &super::winpin::run_winpin_checks());
        dump("F248v2", &super::winpin::run_winpin_v2_checks());
        dump("F249", &super::hotcorner::run_hotcorner_checks());
        dump("F249v2", &super::hotcorner::run_hotcorner_v2_checks());
        dump("F250", &super::pointerprec::run_pointerprec_checks());
        dump("F250v2", &super::pointerprec::run_pointerprec_v2_checks());
    }
}
