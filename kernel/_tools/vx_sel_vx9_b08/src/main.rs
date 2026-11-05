// probe harness for AI-VX9-1105 batch B08 — not kernel code.
#![allow(unused)]
extern crate alloc;
#[path = "../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../varix/src/unxstar/unx12696_antihallu.rs"]
pub mod unx12696_antihallu;
#[path = "../../../varix/src/unxstar/unx12697_docalign_master.rs"]
pub mod unx12697_docalign_master;
#[path = "../../../varix/src/unxstar/unx12698_ktest_agg_b35.rs"]
pub mod unx12698_ktest_agg_b35;
#[path = "../../../varix/src/unxstar/unx12699_b36_gateway.rs"]
pub mod unx12699_b36_gateway;
#[path = "../../../varix/src/unxstar/unx12700_midrecon_anchor.rs"]
pub mod unx12700_midrecon_anchor;
#[path = "../../../varix/src/unxstar/unx12701_cosign_protocol.rs"]
pub mod unx12701_cosign_protocol;
#[path = "../../../varix/src/unxstar/unx12702_fourdomain_matrix.rs"]
pub mod unx12702_fourdomain_matrix;
#[path = "../../../varix/src/unxstar/unx12703_frozen_cover.rs"]
pub mod unx12703_frozen_cover;
#[path = "../../../varix/src/unxstar/unx12704_judge_unique160.rs"]
pub mod unx12704_judge_unique160;
#[path = "../../../varix/src/unxstar/unx12705_inject_ledger.rs"]
pub mod unx12705_inject_ledger;
#[path = "../../../varix/src/unxstar/unx12706_dualhost_rate.rs"]
pub mod unx12706_dualhost_rate;
#[path = "../../../varix/src/unxstar/unx12707_perf_budget_o1.rs"]
pub mod unx12707_perf_budget_o1;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12696", unx12696_antihallu::run_unx12696_checks),
    ("unx12697", unx12697_docalign_master::run_unx12697_checks),
    ("unx12698", unx12698_ktest_agg_b35::run_unx12698_checks),
    ("unx12699", unx12699_b36_gateway::run_unx12699_checks),
    ("unx12700", unx12700_midrecon_anchor::run_unx12700_checks),
    ("unx12701", unx12701_cosign_protocol::run_unx12701_checks),
    ("unx12702", unx12702_fourdomain_matrix::run_unx12702_checks),
    ("unx12703", unx12703_frozen_cover::run_unx12703_checks),
    ("unx12704", unx12704_judge_unique160::run_unx12704_checks),
    ("unx12705", unx12705_inject_ledger::run_unx12705_checks),
    ("unx12706", unx12706_dualhost_rate::run_unx12706_checks),
    ("unx12707", unx12707_perf_budget_o1::run_unx12707_checks),
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
