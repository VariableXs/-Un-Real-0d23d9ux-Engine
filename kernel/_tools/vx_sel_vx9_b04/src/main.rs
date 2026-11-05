// probe harness for AI-VX9-1105 batch B04 — not kernel code.
#![allow(unused)]
extern crate alloc;
#[path = "../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../varix/src/unxstar/unx12648_osver_tier.rs"]
pub mod unx12648_osver_tier;
#[path = "../../../varix/src/unxstar/unx12649_sysinfo_probe.rs"]
pub mod unx12649_sysinfo_probe;
#[path = "../../../varix/src/unxstar/unx12650_objtype_enum.rs"]
pub mod unx12650_objtype_enum;
#[path = "../../../varix/src/unxstar/unx12651_cosign_slot.rs"]
pub mod unx12651_cosign_slot;
#[path = "../../../varix/src/unxstar/unx12652_wine_ref.rs"]
pub mod unx12652_wine_ref;
#[path = "../../../varix/src/unxstar/unx12653_probe_mut20.rs"]
pub mod unx12653_probe_mut20;
#[path = "../../../varix/src/unxstar/unx12654_probe_cmptable.rs"]
pub mod unx12654_probe_cmptable;
#[path = "../../../varix/src/unxstar/unx12655_probe_p95.rs"]
pub mod unx12655_probe_p95;
#[path = "../../../varix/src/unxstar/unx12656_probe_edge.rs"]
pub mod unx12656_probe_edge;
#[path = "../../../varix/src/unxstar/unx12657_antihallu.rs"]
pub mod unx12657_antihallu;
#[path = "../../../varix/src/unxstar/unx12658_ktest_agg.rs"]
pub mod unx12658_ktest_agg;
#[path = "../../../varix/src/unxstar/unx12659_b34_theme.rs"]
pub mod unx12659_b34_theme;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12648", unx12648_osver_tier::run_unx12648_checks),
    ("unx12649", unx12649_sysinfo_probe::run_unx12649_checks),
    ("unx12650", unx12650_objtype_enum::run_unx12650_checks),
    ("unx12651", unx12651_cosign_slot::run_unx12651_checks),
    ("unx12652", unx12652_wine_ref::run_unx12652_checks),
    ("unx12653", unx12653_probe_mut20::run_unx12653_checks),
    ("unx12654", unx12654_probe_cmptable::run_unx12654_checks),
    ("unx12655", unx12655_probe_p95::run_unx12655_checks),
    ("unx12656", unx12656_probe_edge::run_unx12656_checks),
    ("unx12657", unx12657_antihallu::run_unx12657_checks),
    ("unx12658", unx12658_ktest_agg::run_unx12658_checks),
    ("unx12659", unx12659_b34_theme::run_unx12659_checks),
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
