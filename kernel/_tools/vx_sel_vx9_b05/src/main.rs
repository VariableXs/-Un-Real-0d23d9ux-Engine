// probe harness for AI-VX9-1105 batch B05 — not kernel code.
#![allow(unused)]
extern crate alloc;
#[path = "../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../varix/src/unxstar/unx12660_b34_regdoor.rs"]
pub mod unx12660_b34_regdoor;
#[path = "../../../varix/src/unxstar/unx12661_hiveread_edge.rs"]
pub mod unx12661_hiveread_edge;
#[path = "../../../varix/src/unxstar/unx12662_currentver_probe.rs"]
pub mod unx12662_currentver_probe;
#[path = "../../../varix/src/unxstar/unx12663_handle_table.rs"]
pub mod unx12663_handle_table;
#[path = "../../../varix/src/unxstar/unx12664_qobj_consume.rs"]
pub mod unx12664_qobj_consume;
#[path = "../../../varix/src/unxstar/unx12665_fileobj_dosdev.rs"]
pub mod unx12665_fileobj_dosdev;
#[path = "../../../varix/src/unxstar/unx12666_pipe_mailslot.rs"]
pub mod unx12666_pipe_mailslot;
#[path = "../../../varix/src/unxstar/unx12667_section_probe.rs"]
pub mod unx12667_section_probe;
#[path = "../../../varix/src/unxstar/unx12668_token_probe.rs"]
pub mod unx12668_token_probe;
#[path = "../../../varix/src/unxstar/unx12669_sync_probe.rs"]
pub mod unx12669_sync_probe;
#[path = "../../../varix/src/unxstar/unx12670_proc_enum.rs"]
pub mod unx12670_proc_enum;
#[path = "../../../varix/src/unxstar/unx12671_snap_protocol.rs"]
pub mod unx12671_snap_protocol;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12660", unx12660_b34_regdoor::run_unx12660_checks),
    ("unx12661", unx12661_hiveread_edge::run_unx12661_checks),
    ("unx12662", unx12662_currentver_probe::run_unx12662_checks),
    ("unx12663", unx12663_handle_table::run_unx12663_checks),
    ("unx12664", unx12664_qobj_consume::run_unx12664_checks),
    ("unx12665", unx12665_fileobj_dosdev::run_unx12665_checks),
    ("unx12666", unx12666_pipe_mailslot::run_unx12666_checks),
    ("unx12667", unx12667_section_probe::run_unx12667_checks),
    ("unx12668", unx12668_token_probe::run_unx12668_checks),
    ("unx12669", unx12669_sync_probe::run_unx12669_checks),
    ("unx12670", unx12670_proc_enum::run_unx12670_checks),
    ("unx12671", unx12671_snap_protocol::run_unx12671_checks),
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
