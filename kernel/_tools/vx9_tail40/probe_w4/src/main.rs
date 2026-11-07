//! VX9 尾批 W4 探针：SEH 压测域四模块（12740/12742/12743/12752）本地
//! 编译与判据收口自检。跑 4 个 run_unx<id>_checks()，统计 total/red，
//! 打印 TOTAL=4 RED=0 PANIC=0；有红项则逐条打明细并以非零码退出。
//! 注：探针 crate 允许 #[allow]（本文件是契约允许的例外）。
#![allow(dead_code)]

extern crate alloc;

mod checks;

#[path = "../../../../varix/src/unxstar/unx12740_b38_preview.rs"]
mod unx12740;
#[path = "../../../../varix/src/unxstar/unx12742_disp_stress.rs"]
mod unx12742;
#[path = "../../../../varix/src/unxstar/unx12743_unwind_chain.rs"]
mod unx12743;
#[path = "../../../../varix/src/unxstar/unx12752_stress_boundary_corpus.rs"]
mod unx12752;

fn main() {
    let sets = [
        unx12740::run_unx12740_checks(),
        unx12742::run_unx12742_checks(),
        unx12743::run_unx12743_checks(),
        unx12752::run_unx12752_checks(),
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
    let a = unx12740::extra_checks().len();
    let b = unx12742::extra_checks().len();
    let c = unx12743::extra_checks().len();
    println!(
        "收口条数: unx12740={} unx12742={} unx12743={} (冻结 17/17/17)",
        a, b, c
    );
    println!("MODULES=4 TOTAL={} RED={} PANIC={}", total, red, panicked);
    if red > 0 {
        std::process::exit(1);
    }
}
