// probe harness for AI-VX9-1105 worker W10 (handoff 域 4 卡) — not kernel code.
#![allow(unused)]
extern crate alloc;

#[path = "../../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../../varix/src/unxstar/unx12784_audit_table.rs"]
pub mod unx12784_audit_table;
#[path = "../../../../varix/src/unxstar/unx12789_risk_handoff.rs"]
pub mod unx12789_risk_handoff;
#[path = "../../../../varix/src/unxstar/unx12790_freeze_handoff.rs"]
pub mod unx12790_freeze_handoff;
#[path = "../../../../varix/src/unxstar/unx12791_handoff_decl.rs"]
pub mod unx12791_handoff_decl;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12784", unx12784_audit_table::run_unx12784_checks),
    ("unx12789", unx12789_risk_handoff::run_unx12789_checks),
    ("unx12790", unx12790_freeze_handoff::run_unx12790_checks),
    ("unx12791", unx12791_handoff_decl::run_unx12791_checks),
];

fn main() {
    let mut red = 0usize;
    let mut boom = 0usize;
    for (name, f) in ALL {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let s = f();
            if s.all_passed() {
                None
            } else {
                Some(s.red_items_text())
            }
        })) {
            Ok(Some(t)) => {
                red += 1;
                println!("RED {} {}", name, t);
            }
            Ok(None) => {}
            Err(_) => {
                boom += 1;
                println!("PANIC {}", name);
            }
        }
    }
    println!("TOTAL={} RED={} PANIC={}", ALL.len(), red, boom);
    if red + boom > 0 {
        std::process::exit(1);
    }
}
