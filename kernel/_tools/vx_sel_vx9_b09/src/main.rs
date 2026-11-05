// probe harness for AI-VX9-1105 batch B09 — not kernel code.
#![allow(unused)]
extern crate alloc;
#[path = "../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../varix/src/unxstar/unx12708_cosign_replay_proto.rs"]
pub mod unx12708_cosign_replay_proto;
#[path = "../../../varix/src/unxstar/unx12709_slot_sla_reg.rs"]
pub mod unx12709_slot_sla_reg;
#[path = "../../../varix/src/unxstar/unx12710_frozen_iface_reg.rs"]
pub mod unx12710_frozen_iface_reg;
#[path = "../../../varix/src/unxstar/unx12711_xref_zero_dangle.rs"]
pub mod unx12711_xref_zero_dangle;
#[path = "../../../varix/src/unxstar/unx12712_asset_sha_version.rs"]
pub mod unx12712_asset_sha_version;
#[path = "../../../varix/src/unxstar/unx12713_twostate_final_ledger.rs"]
pub mod unx12713_twostate_final_ledger;
#[path = "../../../varix/src/unxstar/unx12714_risk_ledger_final.rs"]
pub mod unx12714_risk_ledger_final;
#[path = "../../../varix/src/unxstar/unx12715_docalign_final.rs"]
pub mod unx12715_docalign_final;
#[path = "../../../varix/src/unxstar/unx12716_ktest_agg_six.rs"]
pub mod unx12716_ktest_agg_six;
#[path = "../../../varix/src/unxstar/unx12717_type1_final_ledger.rs"]
pub mod unx12717_type1_final_ledger;
#[path = "../../../varix/src/unxstar/unx12718_ctype_reg_framework.rs"]
pub mod unx12718_ctype_reg_framework;
#[path = "../../../varix/src/unxstar/unx12719_b37_b40_preview.rs"]
pub mod unx12719_b37_b40_preview;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12708", unx12708_cosign_replay_proto::run_unx12708_checks),
    ("unx12709", unx12709_slot_sla_reg::run_unx12709_checks),
    ("unx12710", unx12710_frozen_iface_reg::run_unx12710_checks),
    ("unx12711", unx12711_xref_zero_dangle::run_unx12711_checks),
    ("unx12712", unx12712_asset_sha_version::run_unx12712_checks),
    ("unx12713", unx12713_twostate_final_ledger::run_unx12713_checks),
    ("unx12714", unx12714_risk_ledger_final::run_unx12714_checks),
    ("unx12715", unx12715_docalign_final::run_unx12715_checks),
    ("unx12716", unx12716_ktest_agg_six::run_unx12716_checks),
    ("unx12717", unx12717_type1_final_ledger::run_unx12717_checks),
    ("unx12718", unx12718_ctype_reg_framework::run_unx12718_checks),
    ("unx12719", unx12719_b37_b40_preview::run_unx12719_checks),
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
