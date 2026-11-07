//! VX9 尾批 W2 探针：卡 12732~12735 四模块本地编译/自检入口。
//! 红线豁免区（探针 main.rs 允许 #[allow]，正式模块严禁）。
#![allow(dead_code)]

extern crate alloc;

#[path = "../../../../varix/src/unxstar/unx12732_sync_family.rs"]
mod unx12732;
#[path = "../../../../varix/src/unxstar/unx12733_vmem_threestate.rs"]
mod unx12733;
#[path = "../../../../varix/src/unxstar/unx12734_nativefile_disp.rs"]
mod unx12734;
#[path = "../../../../varix/src/unxstar/unx12735_registry_family.rs"]
mod unx12735;

mod checks;

use checks::CheckSet;

fn main() {
    let mut total = 0usize;
    let mut red = 0usize;
    let mut red_lines: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();

    let sets: [(&'static str, CheckSet); 4] = [
        ("UNX-F12732", unx12732::run_unx12732_checks()),
        ("UNX-F12733", unx12733::run_unx12733_checks()),
        ("UNX-F12734", unx12734::run_unx12734_checks()),
        ("UNX-F12735", unx12735::run_unx12735_checks()),
    ];

    let mut i = 0usize;
    while i < sets.len() {
        if let Some((label, s)) = sets.get(i) {
            let (p, f) = s.tally();
            total += p + f;
            red += f;
            println!("{} PASS {}/{}", label, p, p + f);
            let mut k = 0usize;
            while k < s.len() {
                if let Some(c) = s.get(k) {
                    if !c.passed {
                        let d = s.detail_for_check(k);
                        let text = if d.is_empty() { c.detail } else { d };
                        red_lines.push(alloc::format!("  - {}: {}", c.name, text));
                    }
                }
                k += 1;
            }
        }
        i += 1;
    }

    if red == 0 {
        println!("TOTAL={} RED=0 PANIC=0", total);
    } else {
        println!("TOTAL={} RED={} PANIC=0", total, red);
        let mut j = 0usize;
        while j < red_lines.len() {
            if let Some(line) = red_lines.get(j) {
                println!("{}", line);
            }
            j += 1;
        }
        std::process::exit(1);
    }
}
