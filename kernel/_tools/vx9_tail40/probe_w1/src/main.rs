//! VX9 尾批 W1 探针：卡 12728~12731 四模块本地编译与判据收口自检。
//! 跑 4 个 run_unx<id>_checks()，统计 total/red，打印 TOTAL=4 RED=0
//! PANIC=0；有红项则逐条打明细并以非零码退出。
extern crate alloc;

mod checks;

#[path = "../../../../varix/src/unxstar/unx12728_unwindinfo_family.rs"]
mod unx12728;
#[path = "../../../../varix/src/unxstar/unx12729_context_frame.rs"]
mod unx12729;
#[path = "../../../../varix/src/unxstar/unx12730_critsec_bits.rs"]
mod unx12730;
#[path = "../../../../varix/src/unxstar/unx12731_oa_fields.rs"]
mod unx12731;

fn main() {
    let sets = [
        unx12728::run_unx12728_checks(),
        unx12729::run_unx12729_checks(),
        unx12730::run_unx12730_checks(),
        unx12731::run_unx12731_checks(),
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
    // 判据谓词双跑一致抽验（对账一致语义）。
    let a = unx12728::extra_checks().len();
    let b = unx12728::extra_checks().len();
    if a != b {
        red += 1;
        println!("红项: unx12728 extra_checks 双跑条数不一致 {} vs {}", a, b);
    }
    println!("MODULES=4 TOTAL={} RED={} PANIC={}", total, red, panicked);
    if red > 0 {
        std::process::exit(1);
    }
}
