use compatstar2_verify::compatstar2 as c2;
type CS = compatstar2_verify::checks::CheckSet;
fn main() {
    let domains: [(&str, fn() -> CS, fn() -> CS, fn() -> CS, fn() -> CS, fn() -> CS); 20] = [
        ("F021", c2::memalign::run_memalign_checks, c2::memalign::run_memalign_deep, c2::deep::f021d::run_f021d_checks, c2::deep2::f021e::run_f021e_checks, c2::deep3::f021f::run_f021f_checks),
        ("F022", c2::timefam::run_timefam_checks, c2::timefam::run_timefam_deep, c2::deep::f022d::run_f022d_checks, c2::deep2::f022e::run_f022e_checks, c2::deep3::f022f::run_f022f_checks),
        ("F023", c2::winsock::run_winsock_checks, c2::winsock::run_winsock_deep, c2::deep::f023d::run_f023d_checks, c2::deep2::f023e::run_f023e_checks, c2::deep3::f023f::run_f023f_checks),
        ("F024", c2::tlsstore::run_tlsstore_checks, c2::tlsstore::run_tlsstore_deep, c2::deep::f024d::run_f024d_checks, c2::deep2::f024e::run_f024e_checks, c2::deep3::f024f::run_f024f_checks),
        ("F025", c2::printpdf::run_printpdf_checks, c2::printpdf::run_printpdf_deep, c2::deep::f025d::run_f025d_checks, c2::deep2::f025e::run_f025e_checks, c2::deep3::f025f::run_f025f_checks),
        ("F026", c2::winmm::run_winmm_checks, c2::winmm::run_winmm_deep, c2::deep::f026d::run_f026d_checks, c2::deep2::f026e::run_f026e_checks, c2::deep3::f026f::run_f026f_checks),
        ("F027", c2::imm32::run_imm32_checks, c2::imm32::run_imm32_deep, c2::deep::f027d::run_f027d_checks, c2::deep2::f027e::run_f027e_checks, c2::deep3::f027f::run_f027f_checks),
        ("F028", c2::dpistate::run_dpistate_checks, c2::dpistate::run_dpistate_deep, c2::deep::f028d::run_f028d_checks, c2::deep2::f028e::run_f028e_checks, c2::deep3::f028f::run_f028f_checks),
        ("F029", c2::moneum::run_moneum_checks, c2::moneum::run_moneum_deep, c2::deep::f029d::run_f029d_checks, c2::deep2::f029e::run_f029e_checks, c2::deep3::f029f::run_f029f_checks),
        ("F030", c2::installr::run_installr_checks, c2::installr::run_installr_deep, c2::deep::f030d::run_f030d_checks, c2::deep2::f030e::run_f030e_checks, c2::deep3::f030f::run_f030f_checks),
        ("F031", c2::uninstall::run_uninstall_checks, c2::uninstall::run_uninstall_deep, c2::deep::f031d::run_f031d_checks, c2::deep2::f031e::run_f031e_checks, c2::deep3::f031f::run_f031f_checks),
        ("F032", c2::runtimes::run_runtimes_checks, c2::runtimes::run_runtimes_deep, c2::deep::f032d::run_f032d_checks, c2::deep2::f032e::run_f032e_checks, c2::deep3::f032f::run_f032f_checks),
        ("F033", c2::buildchain::run_buildchain_checks, c2::buildchain::run_buildchain_deep, c2::deep::f033d::run_f033d_checks, c2::deep2::f033e::run_f033e_checks, c2::deep3::f033f::run_f033f_checks),
        ("F034", c2::codepage::run_codepage_checks, c2::codepage::run_codepage_deep, c2::deep::f034d::run_f034d_checks, c2::deep2::f034e::run_f034e_checks, c2::deep3::f034f::run_f034f_checks),
        ("F035", c2::compatwiz::run_compatwiz_checks, c2::compatwiz::run_compatwiz_deep, c2::deep::f035d::run_f035d_checks, c2::deep2::f035e::run_f035e_checks, c2::deep3::f035f::run_f035f_checks),
        ("F036", c2::stardraft::run_stardraft_checks, c2::stardraft::run_stardraft_deep, c2::deep::f036d::run_f036d_checks, c2::deep2::f036e::run_f036e_checks, c2::deep3::f036f::run_f036f_checks),
        ("F037", c2::peblockui::run_peblockui_checks, c2::peblockui::run_peblockui_deep, c2::deep::f037d::run_f037d_checks, c2::deep2::f037e::run_f037e_checks, c2::deep3::f037f::run_f037f_checks),
        ("F038", c2::isolevel::run_isolevel_checks, c2::isolevel::run_isolevel_deep, c2::deep::f038d::run_f038d_checks, c2::deep2::f038e::run_f038e_checks, c2::deep3::f038f::run_f038f_checks),
        ("F039", c2::gamefront::run_gamefront_checks, c2::gamefront::run_gamefront_deep, c2::deep::f039d::run_f039d_checks, c2::deep2::f039e::run_f039e_checks, c2::deep3::f039f::run_f039f_checks),
        ("F040", c2::compatledger::run_compatledger_checks, c2::compatledger::run_compatledger_deep, c2::deep::f040d::run_f040d_checks, c2::deep2::f040e::run_f040e_checks, c2::deep3::f040f::run_f040f_checks),
    ];
    let (mut m, mut d1, mut d2, mut d3, mut d4, mut red) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    for (tag, f, g, h, k, l) in domains {
        let (a, b, c, d, e) = (f(), g(), h(), k(), l());
        let (ap, _) = a.tally(); let (bp, _) = b.tally(); let (cp, _) = c.tally(); let (dp, _) = d.tally(); let (ep, _) = e.tally();
        for s in [&a, &b, &c, &d, &e] {
            if !s.all_passed() || s.truncated() { red += 1; println!("{} RED", tag); }
        }
        m += a.len(); d1 += b.len(); d2 += c.len(); d3 += d.len(); d4 += e.len();
        println!("{} main {}/{} d1 {}/{} d2 {}/{} d3 {}/{} d4 {}/{}", tag, ap, a.len(), bp, b.len(), cp, c.len(), dp, d.len(), ep, e.len());
    }
    println!("TOTAL main={} deep1={} deep2={} deep3={} deep4={} sum={} red={}", m, d1, d2, d3, d4, m + d1 + d2 + d3 + d4, red);
}
