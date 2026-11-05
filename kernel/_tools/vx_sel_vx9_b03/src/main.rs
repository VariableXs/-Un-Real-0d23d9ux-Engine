// probe harness for AI-VX9-1105 batch B03 — not kernel code.
#![allow(unused)]
extern crate alloc;
#[path = "../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../varix/src/unxstar/unx12636_msgface_samples.rs"]
pub mod unx12636_msgface_samples;
#[path = "../../../varix/src/unxstar/unx12637_two_state_ledger.rs"]
pub mod unx12637_two_state_ledger;
#[path = "../../../varix/src/unxstar/unx12638_doc_align_msd.rs"]
pub mod unx12638_doc_align_msd;
#[path = "../../../varix/src/unxstar/unx12639_ktest_agg.rs"]
pub mod unx12639_ktest_agg;
#[path = "../../../varix/src/unxstar/unx12640_b33_gate.rs"]
pub mod unx12640_b33_gate;
#[path = "../../../varix/src/unxstar/unx12641_consumedge.rs"]
pub mod unx12641_consumedge;
#[path = "../../../varix/src/unxstar/unx12642_tricol_charter.rs"]
pub mod unx12642_tricol_charter;
#[path = "../../../varix/src/unxstar/unx12643_peb_probe.rs"]
pub mod unx12643_peb_probe;
#[path = "../../../varix/src/unxstar/unx12644_teb_probe.rs"]
pub mod unx12644_teb_probe;
#[path = "../../../varix/src/unxstar/unx12645_ntstatus_probe.rs"]
pub mod unx12645_ntstatus_probe;
#[path = "../../../varix/src/unxstar/unx12646_qproc_probe.rs"]
pub mod unx12646_qproc_probe;
#[path = "../../../varix/src/unxstar/unx12647_qthread_probe.rs"]
pub mod unx12647_qthread_probe;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12636", unx12636_msgface_samples::run_unx12636_checks),
    ("unx12637", unx12637_two_state_ledger::run_unx12637_checks),
    ("unx12638", unx12638_doc_align_msd::run_unx12638_checks),
    ("unx12639", unx12639_ktest_agg::run_unx12639_checks),
    ("unx12640", unx12640_b33_gate::run_unx12640_checks),
    ("unx12641", unx12641_consumedge::run_unx12641_checks),
    ("unx12642", unx12642_tricol_charter::run_unx12642_checks),
    ("unx12643", unx12643_peb_probe::run_unx12643_checks),
    ("unx12644", unx12644_teb_probe::run_unx12644_checks),
    ("unx12645", unx12645_ntstatus_probe::run_unx12645_checks),
    ("unx12646", unx12646_qproc_probe::run_unx12646_checks),
    ("unx12647", unx12647_qthread_probe::run_unx12647_checks),
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
