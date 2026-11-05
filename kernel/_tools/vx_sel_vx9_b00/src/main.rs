// probe harness for AI-VX9-1105 batch B00 — not kernel code.
#![allow(unused)]
extern crate alloc;
#[path = "../../../varix/src/checks.rs"]
pub mod checks;

#[path = "../../../varix/src/unxstar/unx12600_milestone600.rs"]
pub mod unx12600_milestone600;
#[path = "../../../varix/src/unxstar/unx12601_d1d4_tricol.rs"]
pub mod unx12601_d1d4_tricol;
#[path = "../../../varix/src/unxstar/unx12602_alpc_connect.rs"]
pub mod unx12602_alpc_connect;
#[path = "../../../varix/src/unxstar/unx12603_msg_boundary.rs"]
pub mod unx12603_msg_boundary;
#[path = "../../../varix/src/unxstar/unx12604_sqos_tier.rs"]
pub mod unx12604_sqos_tier;
#[path = "../../../varix/src/unxstar/unx12605_sid_guard.rs"]
pub mod unx12605_sid_guard;
#[path = "../../../varix/src/unxstar/unx12606_listen_accept.rs"]
pub mod unx12606_listen_accept;
#[path = "../../../varix/src/unxstar/unx12607_section_view.rs"]
pub mod unx12607_section_view;
#[path = "../../../varix/src/unxstar/unx12608_readwrite.rs"]
pub mod unx12608_readwrite;
#[path = "../../../varix/src/unxstar/unx12609_reply_pair.rs"]
pub mod unx12609_reply_pair;
#[path = "../../../varix/src/unxstar/unx12610_msg_type.rs"]
pub mod unx12610_msg_type;
#[path = "../../../varix/src/unxstar/unx12611_msg_attr.rs"]
pub mod unx12611_msg_attr;

const ALL: &[(&str, fn() -> checks::CheckSet)] = &[
    ("unx12600", unx12600_milestone600::run_unx12600_checks),
    ("unx12601", unx12601_d1d4_tricol::run_unx12601_checks),
    ("unx12602", unx12602_alpc_connect::run_unx12602_checks),
    ("unx12603", unx12603_msg_boundary::run_unx12603_checks),
    ("unx12604", unx12604_sqos_tier::run_unx12604_checks),
    ("unx12605", unx12605_sid_guard::run_unx12605_checks),
    ("unx12606", unx12606_listen_accept::run_unx12606_checks),
    ("unx12607", unx12607_section_view::run_unx12607_checks),
    ("unx12608", unx12608_readwrite::run_unx12608_checks),
    ("unx12609", unx12609_reply_pair::run_unx12609_checks),
    ("unx12610", unx12610_msg_type::run_unx12610_checks),
    ("unx12611", unx12611_msg_attr::run_unx12611_checks),
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
