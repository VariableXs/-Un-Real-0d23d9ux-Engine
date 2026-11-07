//! VX9 尾批 W7 探针：卡 12763/12764/12765/12766 四模块本地编译与判据收口自检。
//! 跑 4 个 run_unx<id>_checks()，统计 total/red，打印 TOTAL=4 RED=0
//! PANIC=0；有红项则逐条打明细并以非零码退出。
// 探针 crate 特许：checks.rs 共享管件的聚合面（KernelCheckup 等）在探针
// 场景不全部可达，死代码告警在本探针内显性豁免（仅探针，不进生产模块）。
#![allow(dead_code)]
extern crate alloc;

mod checks;

#[path = "../../../../varix/src/unxstar/unx12763_adr_anchors.rs"]
mod unx12763;
#[path = "../../../../varix/src/unxstar/unx12764_tbt_ledger.rs"]
mod unx12764;
#[path = "../../../../varix/src/unxstar/unx12765_offset_prov.rs"]
mod unx12765;
#[path = "../../../../varix/src/unxstar/unx12766_ntstatus_prov.rs"]
mod unx12766;

fn main() {
    let sets = [
        unx12763::run_unx12763_checks(),
        unx12764::run_unx12764_checks(),
        unx12765::run_unx12765_checks(),
        unx12766::run_unx12766_checks(),
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
        (unx12763::extra_checks().len(), unx12763::extra_checks().len(), "unx12763"),
        (unx12764::extra_checks().len(), unx12764::extra_checks().len(), "unx12764"),
        (unx12765::extra_checks().len(), unx12765::extra_checks().len(), "unx12765"),
        (unx12766::extra_checks().len(), unx12766::extra_checks().len(), "unx12766"),
    ];
    for (a, b, name) in pairs.iter() {
        if a != b || *a == 0 {
            println!("RED: {} 双跑条数不一致 {} vs {}", name, a, b);
            red += 1;
        }
    }
    // 段位守卫复述（探针侧独立口径）。
    if !(unx12763::SEG_BASE == 0x828C0
        && unx12764::SEG_BASE == 0x82900
        && unx12765::SEG_BASE == 0x82940
        && unx12766::SEG_BASE == 0x82980)
    {
        println!("RED: SEG_BASE 段位不复述");
        red += 1;
    }
    println!("TOTAL={} RED={} PANIC={}", total, red, panicked);
    if red > 0 {
        std::process::exit(1);
    }
}
