//! 真驱动家（任务16 起）：块设备抽象 / PCI 枚举 / NVMe 最小栈。
//!
//! 顺带说明：本目录旧有的 AI-31 登记体模块（acpi/bt/driver/gpu/
//! irqdma/netstack/thermal/usb）与启动链零调用，且其自检从未进过
//! 编译与测试门禁——维持未挂载状态（文件保留），真驱动一律挂在本
//! mod 下随 kcheck/ktest 全门禁走。

pub mod blk;
/// S4 批（AI-5）：AHCI 最小栈——SATA 块设备（单端口单槽 LBA48，纯轮询）。
pub mod ahci;
/// S4.2-A（AI-5）：USB 大容量存储 BOT+SCSI 传输层（协议层，管道泛型）。
pub mod msc;
pub mod nvme;
pub mod pci;
/// S4.1（AI-5）：xHCI 最小栈——真机 USB 键鼠（HID boot 协议，纯轮询）。
pub mod xhci;
