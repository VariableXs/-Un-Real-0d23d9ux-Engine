//! Varix kernel — AI-01 boot & platform domain (F001~F025),
//! AI-02 CPU & interrupt domain (F026~F050),
//! AI-03 memory-management domain (F051~F075),
//! AI-04 scheduler domain (F076~F100),
//! AI-05 process & user-space domain (F101~F125).
//!
//! The lib is `no_std` on the kernel target and builds against std on the
//! host so every module's unit tests run natively (`cargo ktest`).
#![cfg_attr(not(test), no_std)]

pub mod acpi;
pub mod audio;
pub mod banner;
pub mod bootopt;
pub mod checks;
pub mod cmdline;
pub mod deploy;
pub mod deveco;
pub mod driver;
pub mod console;
pub mod cpu;
pub mod fb;
pub mod font;
pub mod integrity;
pub mod kaslr;
pub mod limine;
pub mod logger;
pub mod logo;
pub mod mem;
pub mod memmap;
pub mod once;
pub mod platform;
pub mod power;
pub mod proc;
pub mod progress;
pub mod robust;
pub mod sched;
pub mod selftest;
pub mod serial;
pub mod service;
pub mod smbios;
pub mod timeline;
pub mod ui;
pub mod virt;
pub mod vsem;
