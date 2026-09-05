//! L3 shell — 桌面环境系统集成（M5/M6/M7/M8）：
//! - tray.rs     OS 托盘图标 + 菜单（打开桌面/软件/系统窗口/退出）
//! - hardware.rs 蓝牙/Wi-Fi/音频/摄像头/麦克风（Windows API，只读 + 音量控制）
//! - explorer.rs 文件管理器（全盘浏览 + 受控写操作 + 删除入回收站）
//! - recycle.rs  全局回收站（数据库软删除 + 工作区 .trash + fs recycle 聚合）
//! - launcher.rs 第三方软件启动器 + 便携性三级分级（🟢/🟡/🔴）
//! - usb.rs      U 盘完全便携：打包/SHA-256 校验/拔出保护
//! - netconsent.rs 联网确认策略存储（默认零联网；任何联网前必须用户明确授权）
//! - xflow.rs    跨软件数据流：跨窗口拖拽光标跟踪（批次 C 规格 5.7）

pub mod appman;
pub mod embed;
pub mod explorer;
pub mod imwatch;
pub mod hardware;
pub mod kbdhook;
pub mod launcher;
pub mod netconsent;
pub mod privacy;
pub mod recycle;
pub mod sysinfo;
pub mod tray;
pub mod usb;
pub mod wallpaper;
pub mod winman;
pub mod xflow;
