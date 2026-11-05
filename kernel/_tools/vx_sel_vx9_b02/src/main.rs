// probe harness for AI-VX9-1105 batch B02 — not kernel code.
#![allow(unused)]
extern crate alloc;
#[path = "../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../varix/src/unxstar/unx12624_kapc_guard.rs"]
pub mod unx12624_kapc_guard;
#[path = "../../../varix/src/unxstar/unx12625_alert_delay.rs"]
pub mod unx12625_alert_delay;
#[path = "../../../varix/src/unxstar/unx12626_event_pair.rs"]
pub mod unx12626_event_pair;
#[path = "../../../varix/src/unxstar/unx12627_wait_single.rs"]
pub mod unx12627_wait_single;
#[path = "../../../varix/src/unxstar/unx12628_io_port.rs"]
pub mod unx12628_io_port;
#[path = "../../../varix/src/unxstar/unx12629_io_complete.rs"]
pub mod unx12629_io_complete;
#[path = "../../../varix/src/unxstar/unx12630_pkt_fields.rs"]
pub mod unx12630_pkt_fields;
#[path = "../../../varix/src/unxstar/unx12631_async_inject.rs"]
pub mod unx12631_async_inject;
#[path = "../../../varix/src/unxstar/unx12632_dualhost_table.rs"]
pub mod unx12632_dualhost_table;
#[path = "../../../varix/src/unxstar/unx12633_msgface_ledger.rs"]
pub mod unx12633_msgface_ledger;
#[path = "../../../varix/src/unxstar/unx12634_d4_cosign_receipt.rs"]
pub mod unx12634_d4_cosign_receipt;
#[path = "../../../varix/src/unxstar/unx12635_pump_longrun.rs"]
pub mod unx12635_pump_longrun;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12624", unx12624_kapc_guard::run_unx12624_checks),
    ("unx12625", unx12625_alert_delay::run_unx12625_checks),
    ("unx12626", unx12626_event_pair::run_unx12626_checks),
    ("unx12627", unx12627_wait_single::run_unx12627_checks),
    ("unx12628", unx12628_io_port::run_unx12628_checks),
    ("unx12629", unx12629_io_complete::run_unx12629_checks),
    ("unx12630", unx12630_pkt_fields::run_unx12630_checks),
    ("unx12631", unx12631_async_inject::run_unx12631_checks),
    ("unx12632", unx12632_dualhost_table::run_unx12632_checks),
    ("unx12633", unx12633_msgface_ledger::run_unx12633_checks),
    ("unx12634", unx12634_d4_cosign_receipt::run_unx12634_checks),
    ("unx12635", unx12635_pump_longrun::run_unx12635_checks),
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
