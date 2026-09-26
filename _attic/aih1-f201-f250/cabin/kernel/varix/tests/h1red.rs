//! 隔离舱诊断件（AI-H1 v2 批）：逐项打印每个 CheckSet 的红项名。
//! 仅用于舱内定位，不属于交付物；主工作区无此文件。

use varix::h1star::*;

fn dump(name: &str, set: &varix::checks::CheckSet) {
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
    dump("F201-textsel", &textsel::run_textsel_checks());
    dump("F201v2-textsel", &textsel::run_textsel_v2_checks());
    dump("F202-undoframe", &undoframe::run_undoframe_checks());
    dump("F202v2-undoframe", &undoframe::run_undoframe_v2_checks());
    dump("F203-rubbersel", &rubbersel::run_rubbersel_checks());
    dump("F203v2-rubbersel", &rubbersel::run_rubbersel_v2_checks());
    dump("F204-scrolluni", &scrolluni::run_scrolluni_checks());
    dump("F204v2-scrolluni", &scrolluni::run_scrolluni_v2_checks());
    dump("F205-tipsys", &tipsys::run_tipsys_checks());
    dump("F205v2-tipsys", &tipsys::run_tipsys_v2_checks());
    dump("F206-focusnav", &focusnav::run_focusnav_checks());
    dump("F206v2-focusnav", &focusnav::run_focusnav_v2_checks());
    dump("F207-dialksem", &dialksem::run_dialksem_checks());
    dump("F207v2-dialksem", &dialksem::run_dialksem_v2_checks());
    dump("F208-progfeed", &progfeed::run_progfeed_checks());
    dump("F208v2-progfeed", &progfeed::run_progfeed_v2_checks());
    dump("F209-errthree", &errthree::run_errthree_checks());
    dump("F209v2-errthree", &errthree::run_errthree_v2_checks());
    dump("F210-emptystate", &emptystate::run_emptystate_checks());
    dump("F210v2-emptystate", &emptystate::run_emptystate_v2_checks());
    dump("F211-edkeys", &edkeys::run_edkeys_checks());
    dump("F211v2-edkeys", &edkeys::run_edkeys_v2_checks());
    dump("F212-drophl", &drophl::run_drophl_checks());
    dump("F212v2-drophl", &drophl::run_drophl_v2_checks());
    dump("F213-titlebar", &titlebar::run_titlebar_checks());
    dump("F213v2-titlebar", &titlebar::run_titlebar_v2_checks());
    dump("F214-winsize", &winsize::run_winsize_checks());
    dump("F214v2-winsize", &winsize::run_winsize_v2_checks());
    dump("F215-menulev", &menulev::run_menulev_checks());
    dump("F215v2-menulev", &menulev::run_menulev_v2_checks());
    dump("F216-triwidget", &triwidget::run_triwidget_checks());
    dump("F216v2-triwidget", &triwidget::run_triwidget_v2_checks());
    dump("F217-numspin", &numspin::run_numspin_checks());
    dump("F217v2-numspin", &numspin::run_numspin_v2_checks());
    dump("F218-listsel", &listsel::run_listsel_checks());
    dump("F218v2-listsel", &listsel::run_listsel_v2_checks());
    dump("F219-viewmem", &viewmem::run_viewmem_checks());
    dump("F219v2-viewmem", &viewmem::run_viewmem_v2_checks());
    dump("F220-pasteplain", &pasteplain::run_pasteplain_checks());
    dump("F220v2-pasteplain", &pasteplain::run_pasteplain_v2_checks());
    dump("F221-findbar", &findbar::run_findbar_checks());
    dump("F221v2-findbar", &findbar::run_findbar_v2_checks());
    dump("F222-fontrnd", &fontrnd::run_fontrnd_checks());
    dump("F222v2-fontrnd", &fontrnd::run_fontrnd_v2_checks());
    dump("F223-caret", &caret::run_caret_checks());
    dump("F223v2-caret", &caret::run_caret_v2_checks());
    dump("F224-dpiscale", &dpiscale::run_dpiscale_checks());
    dump("F224v2-dpiscale", &dpiscale::run_dpiscale_v2_checks());
    dump("F225-themeswap", &themeswap::run_themeswap_checks());
    dump("F225v2-themeswap", &themeswap::run_themeswap_v2_checks());
    dump("F226-winlayer", &winlayer::run_winlayer_checks());
    dump("F226v2-winlayer", &winlayer::run_winlayer_v2_checks());
    dump("F227-winanim", &winanim::run_winanim_checks());
    dump("F227v2-winanim", &winanim::run_winanim_v2_checks());
    dump("F228-vlist", &vlist::run_vlist_checks());
    dump("F228v2-vlist", &vlist::run_vlist_v2_checks());
    dump("F229-fieldui", &fieldui::run_fieldui_checks());
    dump("F229v2-fieldui", &fieldui::run_fieldui_v2_checks());
    dump("F230-pwdeye", &pwdeye::run_pwdeye_checks());
    dump("F230v2-pwdeye", &pwdeye::run_pwdeye_v2_checks());
    dump("F231-formval", &formval::run_formval_checks());
    dump("F231v2-formval", &formval::run_formval_v2_checks());
    dump("F232-datepick", &datepick::run_datepick_checks());
    dump("F232v2-datepick", &datepick::run_datepick_v2_checks());
    dump("F233-filedlg", &filedlg::run_filedlg_checks());
    dump("F233v2-filedlg", &filedlg::run_filedlg_v2_checks());
    dump("F234-colorpick", &colorpick::run_colorpick_checks());
    dump("F234v2-colorpick", &colorpick::run_colorpick_v2_checks());
    dump("F235-vdesk", &vdesk::run_vdesk_checks());
    dump("F235v2-vdesk", &vdesk::run_vdesk_v2_checks());
    dump("F236-winsnap", &winsnap::run_winsnap_checks());
    dump("F236v2-winsnap", &winsnap::run_winsnap_v2_checks());
    dump("F237-winmem", &winmem::run_winmem_checks());
    dump("F237v2-winmem", &winmem::run_winmem_v2_checks());
    dump("F238-lockui", &lockui::run_lockui_checks());
    dump("F238v2-lockui", &lockui::run_lockui_v2_checks());
    dump("F239-brightosd", &brightosd::run_brightosd_checks());
    dump("F239v2-brightosd", &brightosd::run_brightosd_v2_checks());
    dump("F240-volosd", &volosd::run_volosd_checks());
    dump("F240v2-volosd", &volosd::run_volosd_v2_checks());
    dump("F241-audioroute", &audioroute::run_audioroute_checks());
    dump("F241v2-audioroute", &audioroute::run_audioroute_v2_checks());
    dump("F242-netstate", &netstate::run_netstate_checks());
    dump("F242v2-netstate", &netstate::run_netstate_v2_checks());
    dump("F243-hidfiles", &hidfiles::run_hidfiles_checks());
    dump("F243v2-hidfiles", &hidfiles::run_hidfiles_v2_checks());
    dump("F244-hotkeyreg", &hotkeyreg::run_hotkeyreg_checks());
    dump("F244v2-hotkeyreg", &hotkeyreg::run_hotkeyreg_v2_checks());
    dump("F245-lessmotion", &lessmotion::run_lessmotion_checks());
    dump("F245v2-lessmotion", &lessmotion::run_lessmotion_v2_checks());
    dump("F246-textscale", &textscale::run_textscale_checks());
    dump("F246v2-textscale", &textscale::run_textscale_v2_checks());
    dump("F247-texttrunc", &texttrunc::run_texttrunc_checks());
    dump("F247v2-texttrunc", &texttrunc::run_texttrunc_v2_checks());
    dump("F248-winpin", &winpin::run_winpin_checks());
    dump("F248v2-winpin", &winpin::run_winpin_v2_checks());
    dump("F249-hotcorner", &hotcorner::run_hotcorner_checks());
    dump("F249v2-hotcorner", &hotcorner::run_hotcorner_v2_checks());
    dump("F250-pointerprec", &pointerprec::run_pointerprec_checks());
    dump("F250v2-pointerprec", &pointerprec::run_pointerprec_v2_checks());
}
