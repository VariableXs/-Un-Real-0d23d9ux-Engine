// probe harness for AI-VX9-1105 batch B06 — not kernel code.
#![allow(unused)]
extern crate alloc;
#[path = "../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../varix/src/unxstar/unx12672_probeinj2.rs"]
pub mod unx12672_probeinj2;
#[path = "../../../varix/src/unxstar/unx12673_dualcmp20.rs"]
pub mod unx12673_dualcmp20;
#[path = "../../../varix/src/unxstar/unx12674_cmptab2.rs"]
pub mod unx12674_cmptab2;
#[path = "../../../varix/src/unxstar/unx12675_p95budget2.rs"]
pub mod unx12675_p95budget2;
#[path = "../../../varix/src/unxstar/unx12676_edgesamp2.rs"]
pub mod unx12676_edgesamp2;
#[path = "../../../varix/src/unxstar/unx12677_e1receipt.rs"]
pub mod unx12677_e1receipt;
#[path = "../../../varix/src/unxstar/unx12678_docalign2.rs"]
pub mod unx12678_docalign2;
#[path = "../../../varix/src/unxstar/unx12679_ktestagg2.rs"]
pub mod unx12679_ktestagg2;
#[path = "../../../varix/src/unxstar/unx12680_b35gate.rs"]
pub mod unx12680_b35gate;
#[path = "../../../varix/src/unxstar/unx12681_d2retest.rs"]
pub mod unx12681_d2retest;
#[path = "../../../varix/src/unxstar/unx12682_d3retest.rs"]
pub mod unx12682_d3retest;
#[path = "../../../varix/src/unxstar/unx12683_loadcode.rs"]
pub mod unx12683_loadcode;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12672", unx12672_probeinj2::run_unx12672_checks),
    ("unx12673", unx12673_dualcmp20::run_unx12673_checks),
    ("unx12674", unx12674_cmptab2::run_unx12674_checks),
    ("unx12675", unx12675_p95budget2::run_unx12675_checks),
    ("unx12676", unx12676_edgesamp2::run_unx12676_checks),
    ("unx12677", unx12677_e1receipt::run_unx12677_checks),
    ("unx12678", unx12678_docalign2::run_unx12678_checks),
    ("unx12679", unx12679_ktestagg2::run_unx12679_checks),
    ("unx12680", unx12680_b35gate::run_unx12680_checks),
    ("unx12681", unx12681_d2retest::run_unx12681_checks),
    ("unx12682", unx12682_d3retest::run_unx12682_checks),
    ("unx12683", unx12683_loadcode::run_unx12683_checks),
];

fn main() {
    let mut red = 0usize;
    let mut boom = 0usize;
    for (name, f) in ALL {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let s = f();
            if s.all_passed() { None } else { Some(s.red_items_text()) }
        })) {
            Ok(Some(t)) => { red += 1; println!("RED {} {}", name, t); }
            Ok(None) => {}
            Err(_) => { boom += 1; println!("PANIC {}", name); }
        }
    }
    println!("TOTAL={} RED={} PANIC={}", ALL.len(), red, boom);
}
