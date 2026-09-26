//! K1 深化批次 CheckSet 全量验证（宿主同构 f475 口径）。
use k1_deep_verify::perfstar;

fn main() {
    // 大栈线程：多域 run_*_checks 的定长大数组在 debug 下叠加爆默认栈
    // （K1 报告 §3.2 既有教训——原 crate 以 274 域循环注册同构规避）。
    let child = std::thread::Builder::new().stack_size(64 << 20).spawn(run_all).unwrap();
    child.join().unwrap();
}

fn run_all() {
    let domains: [(&str, fn() -> k1_deep_verify::checks::CheckSet); 17] = [
        ("F041", perfstar::frameledger::run_frameledger_checks),
        ("F042", perfstar::frameattr::run_frameattr_checks),
        ("F043", perfstar::startprof::run_startprof_checks),
        ("F044", perfstar::prefetch2::run_prefetch2_checks),
        ("F045", perfstar::pagewater::run_pagewater_checks),
        ("F046", perfstar::wcoalesce::run_wcoalesce_checks),
        ("F047", perfstar::latbudget::run_latbudget_checks),
        ("F048", perfstar::cpufreq::run_cpufreq_checks),
        ("F049", perfstar::idlezero::run_idlezero_checks),
        ("F050", perfstar::intrcoal::run_intrcoal_checks),
        ("F051", perfstar::bigpage::run_bigpage_checks),
        ("F052", perfstar::heapfrag::run_heapfrag_checks),
        ("F053", perfstar::bootpar::run_bootpar_checks),
        ("F054", perfstar::imgsimd::run_imgsimd_checks),
        ("F055", perfstar::glyphcache::run_glyphcache_checks),
        ("F056", perfstar::dirtyrect::run_dirtyrect_checks),
        ("F057", perfstar::iotier::run_iotier_checks),
    ];
    let mut total = 0usize;
    let mut failed = 0usize;
    for (tag, f) in domains {
        let cs = f();
        let (p, fl) = cs.tally();
        total += p + fl;
        failed += fl;
        println!("{}: {} checks, failed={}", tag, p + fl, fl);
        for i in 0..cs.len() {
            if let Some(c) = cs.get(i) {
                if !c.passed {
                    println!("  FAIL: {} {}", c.name, c.detail);
                }
            }
        }
    }
    println!("TOTAL: {} checks, {} failed", total, failed);
    assert!(failed == 0, "CheckSet 红线");
}
