// probe harness for AI-VX9-1105 batch B01 — not kernel code.
#![allow(unused)]
extern crate alloc;
#[path = "../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../varix/src/unxstar/unx12612_alpc_ctxport.rs"]
pub mod unx12612_alpc_ctxport;
#[path = "../../../varix/src/unxstar/unx12613_handle_pass.rs"]
pub mod unx12613_handle_pass;
#[path = "../../../varix/src/unxstar/unx12614_port_close.rs"]
pub mod unx12614_port_close;
#[path = "../../../varix/src/unxstar/unx12615_cosign_preform.rs"]
pub mod unx12615_cosign_preform;
#[path = "../../../varix/src/unxstar/unx12616_msg_inject30.rs"]
pub mod unx12616_msg_inject30;
#[path = "../../../varix/src/unxstar/unx12617_msg_ledger.rs"]
pub mod unx12617_msg_ledger;
#[path = "../../../varix/src/unxstar/unx12618_p95_budget.rs"]
pub mod unx12618_p95_budget;
#[path = "../../../varix/src/unxstar/unx12619_ktest_agg.rs"]
pub mod unx12619_ktest_agg;
#[path = "../../../varix/src/unxstar/unx12620_b32_gate.rs"]
pub mod unx12620_b32_gate;
#[path = "../../../varix/src/unxstar/unx12621_apc_alertwait.rs"]
pub mod unx12621_apc_alertwait;
#[path = "../../../varix/src/unxstar/unx12622_user_apc.rs"]
pub mod unx12622_user_apc;
#[path = "../../../varix/src/unxstar/unx12623_queue_apc.rs"]
pub mod unx12623_queue_apc;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12612", unx12612_alpc_ctxport::run_unx12612_checks),
    ("unx12613", unx12613_handle_pass::run_unx12613_checks),
    ("unx12614", unx12614_port_close::run_unx12614_checks),
    ("unx12615", unx12615_cosign_preform::run_unx12615_checks),
    ("unx12616", unx12616_msg_inject30::run_unx12616_checks),
    ("unx12617", unx12617_msg_ledger::run_unx12617_checks),
    ("unx12618", unx12618_p95_budget::run_unx12618_checks),
    ("unx12619", unx12619_ktest_agg::run_unx12619_checks),
    ("unx12620", unx12620_b32_gate::run_unx12620_checks),
    ("unx12621", unx12621_apc_alertwait::run_unx12621_checks),
    ("unx12622", unx12622_user_apc::run_unx12622_checks),
    ("unx12623", unx12623_queue_apc::run_unx12623_checks),
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
