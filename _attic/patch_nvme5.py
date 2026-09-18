# -*- coding: utf-8 -*-
"""修复：mod.rs 只挂真驱动三模块 / BAR0 64 位判定 / 测试断言笔误 / mut 警告。"""
from pathlib import Path

# ---- 1) drivers/mod.rs：只挂载 blk/nvme/pci（AI 登记体 8 模块维持未编译） ----
m = Path("kernel/varix/src/drivers/mod.rs")
new_mod = '''//! 真驱动家（任务16 起）：块设备抽象 / PCI 枚举 / NVMe 最小栈。
//!
//! 顺带说明：本目录旧有的 AI-31 登记体模块（acpi/bt/driver/gpu/
//! irqdma/netstack/thermal/usb）与启动链零调用，且其自检从未进过
//! 编译与测试门禁——维持未挂载状态（文件保留），真驱动一律挂在本
//! mod 下随 kcheck/ktest 全门禁走。

pub mod blk;
pub mod nvme;
pub mod pci;
'''
m.write_text(new_mod, encoding="utf-8")
assert "pub mod nvme;" in m.read_text(encoding="utf-8")

# ---- 2) pci.rs：BAR0 64 位判定修正 + 测试断言 ----
p = Path("kernel/varix/src/drivers/pci.rs")
s = p.read_text(encoding="utf-8")
old = """pub fn parse_bar0_mmio(ecam: &mut dyn EcamAccess, addr: u64) -> Option<u64> {
    let low = ecam.read32(addr + 0x10);
    let kind = low & 0b1111;
    let base = match kind {
        0b0000 => low as u64,
        0b0010 => {
            let high = ecam.read32(addr + 0x14);
            ((high as u64) << 32) | (low as u64 & 0xFFFF_FFF0)
        }
        _ => return None,
    };
    if base == 0 {
        return None;
    }
    Some(base & !0xF)
}"""
new = """pub fn parse_bar0_mmio(ecam: &mut dyn EcamAccess, addr: u64) -> Option<u64> {
    let low = ecam.read32(addr + 0x10);
    if low & 1 != 0 {
        return None; // IO BAR——NVMe/AHCI 必须 MMIO，如实拒绝。
    }
    // bit2:1 = 0b10 表示 64 位 MMIO（BAR1 给高 32 位）。
    let base = if (low >> 1) & 0b11 == 0b10 {
        let high = ecam.read32(addr + 0x14);
        ((high as u64) << 32) | (low as u64 & 0xFFFF_FFF0)
    } else {
        low as u64 & 0xFFFF_FFF0
    };
    if base == 0 {
        return None;
    }
    Some(base & !0xF)
}"""
assert old in s, "pci bar0"
s = s.replace(old, new)
# 测试注释同步
s = s.replace("// 64 位 BAR：BAR0 低 nibble=0b0100（0b10 64 位 + bit2=0）。\n        assert_eq!(hit.bar0, 0xC000_0000);",
              "// 64 位 BAR：BAR0 bit2:1=0b10，基址取高位清零后的 0xC000_0000。\n        assert_eq!(hit.bar0, 0xC000_0000);")
p.write_text(s, encoding="utf-8")

# ---- 3) nvme.rs：测试断言笔误（PRP 高 32 位）+ mut 清理 ----
p = Path("kernel/varix/src/drivers/nvme.rs")
s = p.read_text(encoding="utf-8")
old = """        assert_eq!(s.0[2], 0xABCD_E000);
        assert_eq!(s.0[3], 1);
        assert_eq!(s.0[10], 2, "SLBA low");"""
new = """        assert_eq!(s.0[2], 0xABCD_E000);
        assert_eq!(s.0[3], 0, "PRP1 高 32 位（0xABCD_E000 < 2^32）");
        assert_eq!(s.0[10], 2, "SLBA low");"""
assert old in s, "nvme prp assert"
s = s.replace(old, new)
# 6 处 mut 警告：let mut dev = SharedDev::new(); → dev 从未被可变使用
s = s.replace("        let mut dev = SharedDev::new();", "        let dev = SharedDev::new();")
p.write_text(s, encoding="utf-8")

print("all fixed")
