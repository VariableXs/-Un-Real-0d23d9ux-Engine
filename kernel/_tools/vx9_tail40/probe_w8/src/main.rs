//! VX9 尾批 W8 探针：卡 12767/12780/12782/12783 四模块本地编译与判据收口自检。
//! 跑 4 个 run_unx<id>_checks()，统计 total/red，打印 TOTAL=4 RED=0
//! PANIC=0；有红项则逐条打明细并以非零码退出。
// 探针 crate 特许：checks.rs 共享管件的聚合面（KernelCheckup 等）在探针
// 场景不全部可达，死代码告警在本探针内显性豁免（仅探针，不进生产模块）。
#![allow(dead_code)]
extern crate alloc;

mod checks;

#[path = "../../../../varix/src/unxstar/unx12767_layout_prov.rs"]
mod unx12767;
#[path = "../../../../varix/src/unxstar/unx12780_doc_closure.rs"]
mod unx12780;
#[path = "../../../../varix/src/unxstar/unx12782_plane_freeze.rs"]
mod unx12782;
#[path = "../../../../varix/src/unxstar/unx12783_takeover.rs"]
mod unx12783;

fn main() {
    let sets = [
        unx12767::run_unx12767_checks(),
        unx12780::run_unx12780_checks(),
        unx12782::run_unx12782_checks(),
        unx12783::run_unx12783_checks(),
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
    // 判据谓词双跑一致抽验（对账一致语义：extra_checks 双跑条数相等且非零）。
    let pairs = [
        (unx12767::extra_checks().len(), unx12767::extra_checks().len(), "unx12767"),
        (unx12780::extra_checks().len(), unx12780::extra_checks().len(), "unx12780"),
        (unx12782::extra_checks().len(), unx12782::extra_checks().len(), "unx12782"),
        (unx12783::extra_checks().len(), unx12783::extra_checks().len(), "unx12783"),
    ];
    for (a, b, name) in pairs.iter() {
        if a != b || *a == 0 {
            println!("RED: {} 双跑条数不一致 {} vs {}", name, a, b);
            red += 1;
        }
    }
    // 段位守卫复述（探针侧独立口径）。
    if !(unx12767::SEG_BASE == 0x829C0
        && unx12780::SEG_BASE == 0x82D00
        && unx12782::SEG_BASE == 0x82D80
        && unx12783::SEG_BASE == 0x82DC0)
    {
        println!("RED: SEG_BASE 段位不复述");
        red += 1;
    }
    println!("TOTAL={} RED={} PANIC={}", total, red, panicked);
    if red > 0 {
        std::process::exit(1);
    }
}
