//! Varix kernel — AI-01 boot & platform domain (F001~F025),
//! AI-02 CPU & interrupt domain (F026~F050),
//! AI-03 memory-management domain (F051~F075),
//! AI-04 scheduler domain (F076~F100),
//! AI-05 process & user-space domain (F101~F125),
//! AI-06 storage & filesystem domain (F126~F150),
//! AI-07 input & HID domain (F151~F175),
//! AI-08 graphics & display domain (F176~F200),
//! AI-09 network stack domain (F201~F225),
//! AI-10 security & isolation domain (F226~F250).
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
pub mod display;
pub mod driver;
pub mod console;
pub mod cpu;
pub mod fb;
pub mod font;
pub mod fs;
pub mod gfx;
pub mod share;
pub mod shell;
pub mod switcher;
pub mod input;
pub mod integrity;
pub mod kaslr;
pub mod limine;
pub mod logger;
pub mod logo;
pub mod mem;
pub mod memmap;
pub mod net;
pub mod once;
pub mod platform;
pub mod power;
pub mod proc;
pub mod progress;
pub mod robust;
pub mod sched;
pub mod security;
pub mod selftest;
pub mod serial;
pub mod service;
pub mod smbios;
pub mod storage;
pub mod timeline;
pub mod ui;
pub mod vwm;
pub mod virt;
pub mod vsem;

// --- TRINITY-500 AI-11~AI-19 (F251~F475) -----------------------------------
#[path = "shell/taskbar.rs"]
pub mod taskbar;
#[path = "shell/startmenu.rs"]
pub mod startmenu;
#[path = "proc/entry.rs"]
pub mod entry;
#[path = "proc/ipc.rs"]
pub mod ipc;
#[path = "app/write.rs"]
pub mod app_write;
#[path = "app/mind.rs"]
pub mod app_mind;
#[path = "app/code.rs"]
pub mod app_code;
#[path = "app/fate.rs"]
pub mod app_fate;
pub mod sync;
pub mod sec;
pub mod verify;
