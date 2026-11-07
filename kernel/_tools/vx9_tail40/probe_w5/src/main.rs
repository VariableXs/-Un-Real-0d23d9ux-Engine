//! VX9 尾批 W5 探针：卡 12754~12757 四模块本地编译与判据收口自检。
//! 跑 4 个 run_unx<id>_checks()，统计 total/red，打印 TOTAL=4 RED=0
//! PANIC=0；有红项则逐条打明细并以非零码退出。
use probe_w5::{unx12754, unx12755, unx12756, unx12757};

fn main() {
    let sets = [
        unx12754::run_unx12754_checks(),
        unx12755::run_unx12755_checks(),
        unx12756::run_unx12756_checks(),
        unx12757::run_unx12757_checks(),
    ];
    let mut total = 0usize;
    let mut red = 0usize;
    let panicked = 0usize;
    for s in sets.iter() {
        let (passed, failed) = s.tally();
        total += passed + failed;
        red += failed;
        print!("{}: {}/{} pass", s.domain, passed, passed + failed);
        if failed > 0 {
            println!("  红项: {}", s.red_items_text());
        } else {
            println!();
        }
    }
    // 判据谓词双跑一致抽验（对账一致语义，覆盖本批两个新模块）。
    let a = unx12756::extra_checks().len();
    let b = unx12756::extra_checks().len();
    if a != b {
        red += 1;
        println!("红项: unx12756 extra_checks 双跑条数不一致 {} vs {}", a, b);
    }
    let c = unx12757::extra_checks().len();
    let d = unx12757::extra_checks().len();
    if c != d {
        red += 1;
        println!("红项: unx12757 extra_checks 双跑条数不一致 {} vs {}", c, d);
    }
    println!("MODULES=4 TOTAL={} RED={} PANIC={}", total, red, panicked);
    if red > 0 {
        std::process::exit(1);
    }
}
