//! L3 shell — 桌面环境系统集成（M5/M6/M7/M8）：
//! - tray.rs     OS 托盘图标 + 菜单（打开桌面/软件/系统窗口/退出）
//! - hardware.rs 蓝牙/Wi-Fi/音频/摄像头/麦克风（Windows API，只读 + 音量控制）
//! - explorer.rs 文件管理器（全盘浏览 + 受控写操作 + 删除入回收站）
//! - recycle.rs  全局回收站（数据库软删除 + 工作区 .trash + fs recycle 聚合）
//! - launcher.rs 第三方软件启动器 + 便携性三级分级（🟢/🟡/🔴）
//! - usb.rs      U 盘完全便携：打包/SHA-256 校验/拔出保护
//! - netconsent.rs 联网确认策略存储（默认零联网；任何联网前必须用户明确授权）
//! - xflow.rs    跨软件数据流：跨窗口拖拽光标跟踪（批次 C 规格 5.7）

pub mod ai;
pub mod appman;
pub mod browsers;
pub mod code;
pub mod embed;
pub mod envs;
pub mod ecosystem;
pub mod diagnostic;
pub mod explorer;
pub mod ext_plugin;
pub mod extensions;
pub mod audioime;
pub mod fsindex;
pub mod sysmaint;
pub mod imwatch;
pub mod installer;
pub mod isolation;
pub mod hardware;
pub mod kbdhook;
pub mod mousefeel;
pub mod git_panel;
pub mod launcher;
pub mod netconsent;
pub mod network;
pub mod privacy;
pub mod privacy_shield;
pub use privacy_shield::privacy_shield_log;
pub mod print;
pub mod recovery;
pub mod recycle;
pub mod search;
pub mod security;
pub mod shellmode;
pub mod shell_watch;
pub mod single_instance;
pub mod sysenv;
pub mod taskman;
pub mod tools;
pub mod sysinfo;
pub mod terminal;
pub mod toolchains;
pub mod compat;
pub mod compat_probe;
pub mod container;
pub mod capture;
pub mod directshell;
pub mod tray;
pub mod usb;
pub mod wallpaper;
pub mod winman;
pub mod xflow;
// AI-09 文件操作组（M-21/Z-29..Z-35/M-19..M-27）
pub mod fileops;
// AI-10 文件管理与数据安全组（U-16/U-25…U-36/N-31/V-31）
pub mod versions;
pub mod tags;
pub mod transfer;
pub mod archive;
pub mod lineage;
pub mod panic;
pub mod trust;
pub mod incognito;
pub mod insights;
// AI-07 效率中枢组（N-15 剪贴板历史后端 / N-18 宏引擎护栏与触发器）
pub mod cliphist;
pub mod macros;
// AI-13 性能与长跑组（U-20 内存守护 / U-22 IO 治理 / M-46 日志轮转 / M-47 迁移预检 /
// M-48 DB 紧凑 / N-35 分身 / N-36 接力 / M-53 崩溃转储 / M-54 CPU 配额 / U-19 boot 阶段）
pub mod perf;
// AI-14 开放接口组（U-37/38/39、Z-51/52/55、N-28/30）
pub mod openhub;
// 系统探测与电源（lib.rs 已注册命令；模块文件曾被并发回滚，此处补声明）
pub mod sysprobe;
pub mod winpower;
// AI-15 开放工具组（M-57/59/63、V-89/90 出站桥与安全扫描；V-81 winget；V-82 环境变量；
// V-83/V-86 计划任务与启动延迟；V-84/85 关联快照与卸载善后；V-87 服务依赖图）
pub mod opentools;
pub mod winget;
pub mod envedit;
pub mod workshop;
pub mod assocguard;
pub mod svcgraph;
// AI-19 M-73/M-74 与 AI-16 soundnotify：接线暂存于 lib.rs/mod.rs 历史，模块文件由各组交付时恢复声明
// AI-16 启动与声音通知组（Z-43 音量记忆 / Z-45 方案校验 / Z-46 通信设备 /
// Z-47 通知存档 / Z-48 麦克风指示 / Z-49 提醒中心）
