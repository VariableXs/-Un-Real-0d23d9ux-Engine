//! Varix kernel — AI-01 boot & platform domain (F001~F025).
//!
//! The lib is `no_std` on the kernel target and builds against std on the
//! host so every module's unit tests run natively (`cargo ktest`).
#![cfg_attr(not(test), no_std)]

pub mod acpi;
pub mod banner;
pub mod bootopt;
pub mod cmdline;
pub mod console;
pub mod fb;
pub mod font;
pub mod integrity;
pub mod kaslr;
pub mod limine;
pub mod logger;
pub mod logo;
pub mod memmap;
pub mod once;
pub mod platform;
pub mod progress;
pub mod selftest;
pub mod serial;
pub mod smbios;
pub mod timeline;
