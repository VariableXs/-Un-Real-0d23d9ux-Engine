//! VX9 尾批 W9 探针：卡 12785/12786/12787/12788 四模块本地编译与判据收口自检。
//! 跑 4 个 run_unx<id>_checks()，统计 total/red，打印 TOTAL=4 RED=0
//! PANIC=0；有红项则逐条打明细并以非零码退出。
// 探针 crate 特许：checks.rs 共享管件的聚合面（KernelCheckup 等）在探针
// 场景不全部可达，死代码告警在本探针内显性豁免（仅探针，不进生产模块）。
#![allow(dead_code)]
extern crate alloc;

mod checks;

#[path = "../../../../varix/src/unxstar/unx12785_seal_ledger.rs"]
mod unx12785;
#[path = "../../../../varix/src/unxstar/unx12786_close_bar.rs"]
mod unx12786;
#[path = "../../../../varix/src/unxstar/unx12787_criteria_unique.rs"]
mod unx12787;
#[path = "../../../../varix/src/unxstar/unx12788_line_ledger.rs"]
mod unx12788;

fn main() {
    // 四模块检查面逐个收口；panic 用 catch_unwind 圈住，逐模块记 PANIC。
    let entries: [(&str, fn() -> checks::CheckSet); 4] = [
        ("unx12785", unx12785::run_unx12785_checks),
        ("unx12786", unx12786::run_unx12786_checks),
        ("unx12787", unx12787::run_unx12787_checks),
        ("unx12788", unx12788::run_unx12788_checks),
    ];
    let mut total = 0usize;
    let mut red = 0usize;
    let mut panicked = 0usize;
    for (name, f) in entries.iter() {
        let set = std::panic::catch_unwind(f);
        match set {
            Ok(s) => {
                let (passed, failed) = s.tally();
                total += passed + failed;
                red += failed;
                print!("{}: {}/{} pass", name, passed, passed + failed);
                if failed > 0 {
                    println!("  红项: {}", s.red_items_text());
                } else {
                    println!();
                }
            }
            Err(_) => {
                panicked += 1;
                println!("{}: PANIC（检查面抛panic，计入红）", name);
                red += 1;
            }
        }
    }
    // 判据谓词双跑一致抽验（对账一致语义：extra_checks 双跑条数相等且非零）。
    let pairs: [(&str, usize, usize); 4] = [
        ("unx12785", unx12785::extra_checks().len(), unx12785::extra_checks().len()),
        ("unx12786", unx12786::extra_checks().len(), unx12786::extra_checks().len()),
        ("unx12787", unx12787::extra_checks().len(), unx12787::extra_checks().len()),
        ("unx12788", unx12788::extra_checks().len(), unx12788::extra_checks().len()),
    ];
    for (name, a, b) in pairs.iter() {
        if a != b || *a == 0 {
            println!("RED: {} 双跑条数不一致 {} vs {}", name, a, b);
            red += 1;
        }
    }
    // 段位守卫复述（探针侧独立口径）。
    if !(unx12785::SEG_BASE == 0x82E40
        && unx12786::SEG_BASE == 0x82E80
        && unx12787::SEG_BASE == 0x82EC0
        && unx12788::SEG_BASE == 0x82F00)
    {
        println!("RED: SEG_BASE 段位不复述");
        red += 1;
    }
    println!("TOTAL={} RED={} PANIC={}", total, red, panicked);
    if red > 0 || panicked > 0 {
        std::process::exit(1);
    }
}
