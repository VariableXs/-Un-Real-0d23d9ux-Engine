// probe harness for AI-VX9-1105 batch B07 — not kernel code.
#![allow(unused)]
extern crate alloc;
#[path = "../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../varix/src/unxstar/unx12684_oa_consumer_reg.rs"]
pub mod unx12684_oa_consumer_reg;
#[path = "../../../varix/src/unxstar/unx12685_loaderlock_cosign.rs"]
pub mod unx12685_loaderlock_cosign;
#[path = "../../../varix/src/unxstar/unx12686_tls_callback_order.rs"]
pub mod unx12686_tls_callback_order;
#[path = "../../../varix/src/unxstar/unx12687_handle_reuse_reg.rs"]
pub mod unx12687_handle_reuse_reg;
#[path = "../../../varix/src/unxstar/unx12688_rootpath_protocol.rs"]
pub mod unx12688_rootpath_protocol;
#[path = "../../../varix/src/unxstar/unx12689_deletepending_reg.rs"]
pub mod unx12689_deletepending_reg;
#[path = "../../../varix/src/unxstar/unx12690_fourway_recon.rs"]
pub mod unx12690_fourway_recon;
#[path = "../../../varix/src/unxstar/unx12691_edge_registry.rs"]
pub mod unx12691_edge_registry;
#[path = "../../../varix/src/unxstar/unx12692_twostate_ledger.rs"]
pub mod unx12692_twostate_ledger;
#[path = "../../../varix/src/unxstar/unx12693_dualhost_summary.rs"]
pub mod unx12693_dualhost_summary;
#[path = "../../../varix/src/unxstar/unx12694_p95_budget_reg.rs"]
pub mod unx12694_p95_budget_reg;
#[path = "../../../varix/src/unxstar/unx12695_risk_ledger.rs"]
pub mod unx12695_risk_ledger;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12684", unx12684_oa_consumer_reg::run_unx12684_checks),
    ("unx12685", unx12685_loaderlock_cosign::run_unx12685_checks),
    ("unx12686", unx12686_tls_callback_order::run_unx12686_checks),
    ("unx12687", unx12687_handle_reuse_reg::run_unx12687_checks),
    ("unx12688", unx12688_rootpath_protocol::run_unx12688_checks),
    ("unx12689", unx12689_deletepending_reg::run_unx12689_checks),
    ("unx12690", unx12690_fourway_recon::run_unx12690_checks),
    ("unx12691", unx12691_edge_registry::run_unx12691_checks),
    ("unx12692", unx12692_twostate_ledger::run_unx12692_checks),
    ("unx12693", unx12693_dualhost_summary::run_unx12693_checks),
    ("unx12694", unx12694_p95_budget_reg::run_unx12694_checks),
    ("unx12695", unx12695_risk_ledger::run_unx12695_checks),
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
