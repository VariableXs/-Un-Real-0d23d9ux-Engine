use compatstar2_verify::compatstar2 as c2;
fn main() {
    let domains: [(&str, fn() -> compatstar2_verify::checks::CheckSet, fn() -> compatstar2_verify::checks::CheckSet); 20] = [
        ("F021", c2::memalign::run_memalign_checks, c2::memalign::run_memalign_deep),
        ("F022", c2::timefam::run_timefam_checks, c2::timefam::run_timefam_deep),
        ("F023", c2::winsock::run_winsock_checks, c2::winsock::run_winsock_deep),
        ("F024", c2::tlsstore::run_tlsstore_checks, c2::tlsstore::run_tlsstore_deep),
        ("F025", c2::printpdf::run_printpdf_checks, c2::printpdf::run_printpdf_deep),
        ("F026", c2::winmm::run_winmm_checks, c2::winmm::run_winmm_deep),
        ("F027", c2::imm32::run_imm32_checks, c2::imm32::run_imm32_deep),
        ("F028", c2::dpistate::run_dpistate_checks, c2::dpistate::run_dpistate_deep),
        ("F029", c2::moneum::run_moneum_checks, c2::moneum::run_moneum_deep),
        ("F030", c2::installr::run_installr_checks, c2::installr::run_installr_deep),
        ("F031", c2::uninstall::run_uninstall_checks, c2::uninstall::run_uninstall_deep),
        ("F032", c2::runtimes::run_runtimes_checks, c2::runtimes::run_runtimes_deep),
        ("F033", c2::buildchain::run_buildchain_checks, c2::buildchain::run_buildchain_deep),
        ("F034", c2::codepage::run_codepage_checks, c2::codepage::run_codepage_deep),
        ("F035", c2::compatwiz::run_compatwiz_checks, c2::compatwiz::run_compatwiz_deep),
        ("F036", c2::stardraft::run_stardraft_checks, c2::stardraft::run_stardraft_deep),
        ("F037", c2::peblockui::run_peblockui_checks, c2::peblockui::run_peblockui_deep),
        ("F038", c2::isolevel::run_isolevel_checks, c2::isolevel::run_isolevel_deep),
        ("F039", c2::gamefront::run_gamefront_checks, c2::gamefront::run_gamefront_deep),
        ("F040", c2::compatledger::run_compatledger_checks, c2::compatledger::run_compatledger_deep),
    ];
    let (mut m, mut d, mut red) = (0usize, 0usize, 0usize);
    for (tag, f, g) in domains {
        let (a, b) = (f(), g());
        let (ap, _) = a.tally();
        let (bp, _) = b.tally();
        if !a.all_passed() || a.truncated() { red += 1; println!("{} main RED {} ({})", tag, ap, a.len()); }
        if !b.all_passed() || b.truncated() { red += 1; println!("{} deep RED {} ({})", tag, bp, b.len()); }
        m += a.len(); d += b.len();
        println!("{} main {}/{} deep {}/{}", tag, ap, a.len(), bp, b.len());
    }
    println!("TOTAL main={} deep={} sum={} red={}", m, d, m + d, red);
}
