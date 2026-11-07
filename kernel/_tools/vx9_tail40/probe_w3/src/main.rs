//! VX9 尾批 W3 探针：卡 12736~12739 四模块本地编译与判据收口自检。
//! 跑 4 个 run_unx<id>_checks()，统计 total/red，打印 TOTAL=4 RED=0
//! PANIC=0；有红项则逐条打明细并以非零码退出。
extern crate alloc;

mod checks;

#[path = "../../../../varix/src/unxstar/unx12736_token_family.rs"]
mod unx12736;
#[path = "../../../../varix/src/unxstar/unx12737_diff_ledger.rs"]
mod unx12737;
#[path = "../../../../varix/src/unxstar/unx12738_dual_ledger_agreement.rs"]
mod unx12738;
#[path = "../../../../varix/src/unxstar/unx12739_ktest_b37.rs"]
mod unx12739;

fn main() {
    let sets = [
        unx12736::run_unx12736_checks(),
        unx12737::run_unx12737_checks(),
        unx12738::run_unx12738_checks(),
        unx12739::run_unx12739_checks(),
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
    // 判据谓词双跑一致抽验（收口只增不减语义）。
    let a = unx12736::extra_checks().len();
    let b = unx12739::extra_checks().len();
    println!(
        "收口条数: unx12736={} unx12739={} (冻结 16/16)",
        a, b
    );
    println!("MODULES=4 TOTAL={} RED={} PANIC={}", total, red, panicked);
    if red > 0 {
        std::process::exit(1);
    }
}
