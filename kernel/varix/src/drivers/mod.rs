//! 真驱动家（任务16 起）：块设备抽象 / PCI 枚举 / NVMe 最小栈。
//!
//! 顺带说明：本目录旧有的 AI-31 登记体模块（acpi/bt/driver/gpu/
//! irqdma/netstack/thermal/usb）与启动链零调用，且其自检从未进过
//! 编译与测试门禁——维持未挂载状态（文件保留），真驱动一律挂在本
//! mod 下随 kcheck/ktest 全门禁走。

pub mod blk;
pub mod nvme;
pub mod pci;
