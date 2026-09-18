# -*- coding: utf-8 -*-
"""修复：match const pattern（命名常量）+ probe 模块路径 super::super。"""
from pathlib import Path

p = Path("kernel/varix/src/drivers/nvme.rs")
s = p.read_text(encoding="utf-8")

# 1) 常量补齐（REG_*_HI）
anchor = 'pub const DB_ADMIN_CQ: u16 = 0x1004;'
assert anchor in s
s = s.replace(
    anchor,
    anchor
    + "\npub const REG_CAP_HI: u16 = REG_CAP + 4; // DSTRD 在 CAP[35:32]\npub const REG_ASQ_HI: u16 = REG_ASQ + 4;\npub const REG_ACQ_HI: u16 = REG_ACQ + 4;",
    1,
)

# 2) match pattern 换常量名
for old, new in [
    ("REG_CAP + 4 => 0,      // DSTRD=0", "REG_CAP_HI => 0, // DSTRD=0"),
    ("REG_ASQ + 4 => self.asq = (self.asq & 0xFFFF_FFFF) | ((val as u64) << 32),",
     "REG_ASQ_HI => self.asq = (self.asq & 0xFFFF_FFFF) | ((val as u64) << 32),"),
    ("REG_ACQ + 4 => self.acq = (self.acq & 0xFFFF_FFFF) | ((val as u64) << 32),",
     "REG_ACQ_HI => self.acq = (self.acq & 0xFFFF_FFFF) | ((val as u64) << 32),"),
]:
    assert old in s, old
    s = s.replace(old, new)

# 3) 主干代码里 read32(REG_CAP + 4) 改常量
s = s.replace("let cap_hi = self.bar.read32(REG_CAP + 4);", "let cap_hi = self.bar.read32(REG_CAP_HI);")

# 4) probe 模块路径：target mod 的 super=nvme，需 super::super 到 drivers
s = s.replace("super::pci::target::", "super::super::pci::target::")
s = s.replace("super::pci::scan_nvme", "super::super::pci::scan_nvme")
s = s.replace("super::blk::loopback_probe", "super::super::blk::loopback_probe")

p.write_text(s, encoding="utf-8")
s2 = p.read_text(encoding="utf-8")
assert "REG_CAP_HI => 0" in s2
assert "super::super::pci::scan_nvme" in s2
print("fixed: const patterns + module paths")
