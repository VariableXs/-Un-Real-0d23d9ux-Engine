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

// --- GALAXY-1800 AI-16~AI-25 (G901~G1500) ----------------------------------
pub mod galaxy;

// --- VARIABLE-200 内核并入 Variable 系统（F001~F200）------------------------
// AI-01 用户态进程域：落点 proc.rs（扩展）+ proc/uspace.rs + mem/addrspace.rs。
// AI-02 ELF 加载与 ABI 域：落点 exec.rs（VXELF 格式与加载器）。
// AI-03 系统调用域：落点 syscall/。
#[path = "exec.rs"]
pub mod exec;
#[path = "syscall/mod.rs"]
pub mod syscall;

// --- GALAXY-1800 AI-01~AI-07（G001~G420，W1/W2）-----------------------------
pub mod gtoolchain;
pub mod gconcur;
pub mod gmem;
pub mod gstore;
pub mod gbus;
pub mod gperiph;
pub mod gnet;

// --- GALAXY-1800 AI-08~AI-16 (G421~G960) ------------------------------------
pub mod gdist;
pub mod gcons;
pub mod gcont;
pub mod guni;
pub mod gpm;
pub mod gperf;
pub mod gobs;
pub mod gsec;
pub mod grtc;
// --- TRINITY-500 AI-20 (F476~F500) 工程质量与门禁收官（模块注册由 AI-20 统一收口）
pub mod quality;

// --- AURORA-1000 AI-16~AI-30 (A376~A750) ------------------------------------
pub mod workspace;
pub mod designsys;
pub mod fileman;
pub mod settings;
pub mod apps;
pub mod terminal;
pub mod editor;
pub mod imageview;
pub mod player;
pub mod netweb;
pub mod notify;
pub mod search;
pub mod sysmon;
pub mod pkgstore;
pub mod printing;
pub mod w3gate;
// （aurora:: 命名空间由 AI-01~AI-15 收口段统一注册，见文件底部）

// --- AURORA-1000 AI-31~AI-40 (A751~A1000，W4/W5) -----------------------------
#[path = "a11y/a11y.rs"]
pub mod a11y;
#[path = "power/aurora.rs"]
pub mod apower;
#[path = "perf/perf.rs"]
pub mod perf;
#[path = "stability/stability.rs"]
pub mod stability;
#[path = "security/aurora.rs"]
pub mod asecurity;
#[path = "testing/testing.rs"]
pub mod testing;
#[path = "help/help.rs"]
pub mod help;
#[path = "acceptance/acceptance.rs"]
pub mod acceptance;
#[path = "release/release.rs"]
pub mod release;
#[path = "finalize/finalize.rs"]
pub mod finalize;

// --- AURORA-1000 AI-01~AI-15 (A001~A375, W1/W2) -----------------------------
// 界面栈十五域统一收口在 aurora:: 命名空间（顶层 display/input/audio 已被
// VARIX 既有模块占用，依赖收口：不覆盖、只新增）。
pub mod aurora;

// --- AURORA-1000 步骤 0028 · 全局可观测计数器 --------------------------------
pub mod metrics;

// --- VARIABLE-200 AI-04~AI-08（F076~F200，内核并入 Variable 系统）-----------
pub mod srv;
pub mod gfxsrv;
pub mod hidsrv;
pub mod vport;
pub mod bootchain;
