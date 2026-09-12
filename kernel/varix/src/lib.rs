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

// --- VARIX-M500 AI-16~AI-20（F376~F500，成熟化系列）--------------------------
pub mod deskwis; //   AI-16 桌面智慧与空间管理
pub mod dataflow; //  AI-17 数据流动与互操作
pub mod selfheal; //  AI-18 自愈与可靠性深化
pub mod observ; //    AI-19 性能艺术与观测（先行域）
pub mod i18n; //      AI-20 全球化与作品集交付

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

// --- VARIX-M500 AI-11~AI-15（F251~F375，内核成熟化与生态深化）---------------
// 能源 v2（envpower）、主题艺术（theme）、硬件兼容广度（hwcompat）、
// 应用生态 SDK（appmgr）、自动化引擎（automation）。
// M500 的 F 编号与 VARIX-500 历史编号空间重叠，符号全部落在新模块避免冲突。
pub mod envpower;
pub mod theme;
pub mod hwcompat;
pub mod appmgr;
pub mod automation;

// --- VARIX-M500 AI-01~AI-05（F001~F125，M1 底座 + M3 生态门口）--------------
// 启动体验（m5boot）、算力编排（m5sched）、内存智能（m5mem）、
// 服务编排 IPC（m5srv）、文件系统与数据（m5fs）。
// M500 的 F 编号与 VARIX-500 历史编号空间重叠，符号全部落在新模块避免冲突。
pub mod m5boot;
pub mod m5sched;
pub mod m5mem;
pub mod m5srv;
pub mod m5fs;

// --- VARIX-M400 AI-09~AI-16（F201~F400，成品体验与看不见的质量）-------------
// 桌面 shell 完备化（m4shell）、兼容层扩展（m4compat）、性能工程（m4perf）、
// 安全与隐私（m4privsec）、发布工程（m4release）、质量与测试（m4quality）、
// 文档与生态（m4docseco）、艺术与体验（m4arts）。
// M400 的 F 编号与历史编号空间重叠，符号全部落在新模块避免冲突。
pub mod m4shell;
pub mod m4compat;
pub mod m4perf;
pub mod m4privsec;
pub mod m4release;
pub mod m4quality;
pub mod m4docseco;
pub mod m4arts;

// VARIX-M400 W1/W2 domains (AI-01 ~ AI-08).
pub mod m400boot;
pub mod m400mem;
pub mod m400sched;
pub mod m400syssec;
pub mod m400store;
pub mod m400input;
pub mod m400net;
pub mod m400gfx;

// --- VARIX-M600 AI-21~AI-24（F501~F600，作品交付波次）------------------------
// 数据同步（m6sync）、智能助手（m6assist）、无障碍与全球化（m6a11y）、
// 作品交付与文档（m6deliver）。符号全部落在新模块避免编号冲突。
pub mod m6sync;
pub mod m6assist;
pub mod m6a11y;
pub mod m6deliver;

// --- VARIX-M600 AI-01（F001~F025，启动与秒开域）与 VARIX-M700 AI-01
// --- （F001~F025，进程与线程域）。符号全部落在新模块避免编号冲突。
pub mod m600boot;
pub mod m700proc;

// --- VARIX-M700 AI-21~AI-28（F501~F700，内核成熟化收口波次）-------------------
// 调度器（m7sched）、容器隔离（m7cgroup）、时间日志（m7timelog）、
// 引导固件（m7bootfw）、可测试性（m7testing）、基准度量（m7bench）、
// 兼容移植（m7compat）、文档发布（m7docrel）。符号全落新模块避免编号冲突。
pub mod m7sched;
pub mod m7cgroup;
pub mod m7timelog;
pub mod m7bootfw;
pub mod m7testing;
pub mod m7bench;
pub mod m7compat;
pub mod m7docrel;

// --- VARIX-M600 AI-11~AI-20（F251~F500，感官/生态/信任波次）-------------------
// 图像媒体（m600media）、动效空间（m600motion）、Shell 精工（m600shell）、
// 文件数据（m600files）、应用 SDK（m600sdk）、自动化（m600auto）、
// 网络互联（m600net）、服务进程（m600svc）、隐私信任（m600priv）、
// 硬件广度（m600hw）。符号全部落在新模块避免编号冲突。
pub mod m600media;
pub mod m600motion;
pub mod m600shell;
pub mod m600files;
pub mod m600sdk;
pub mod m600auto;
pub mod m600net;
pub mod m600svc;
pub mod m600priv;
pub mod m600hw;

// --- VARIX-M700 AI-11~AI-20（F251~F500，存储/图形网络波次）--------------------
// 缓存回写（m700cache）、设备模型（m700dev）、驱动框架（m700drv）、
// 输入内核（m700input）、GPU 驱动（m700gpu）、显示合成（m700disp）、
// 协议栈（m700net）、无线链路（m700rf）、电源时钟（m700pwr）、多核 SMP（m700smp）。
pub mod m700cache;
pub mod m700dev;
pub mod m700drv;
pub mod m700input;
pub mod m700gpu;
pub mod m700disp;
pub mod m700net;
pub mod m700rf;
pub mod m700pwr;
pub mod m700smp;
