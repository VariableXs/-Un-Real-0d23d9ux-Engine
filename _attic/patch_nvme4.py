# -*- coding: utf-8 -*-
"""expect_err → match 解构。"""
from pathlib import Path

p = Path("kernel/varix/src/drivers/nvme.rs")
s = p.read_text(encoding="utf-8")
old = """        let e = NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 1_000)
            .expect_err("持续失败必须报错");
        assert_eq!(e, BlockError::DeviceReset, "重试耗尽=DeviceReset 口径");"""
new = """        let e = match NvmeCtrl::init_with_recovery(dev.clone(), dev.clone(), fake_now, 1_000) {
            Ok(_) => panic!("持续失败必须报错"),
            Err(e) => e,
        };
        assert_eq!(e, BlockError::DeviceReset, "重试耗尽=DeviceReset 口径");"""
assert old in s
s = s.replace(old, new)
p.write_text(s, encoding="utf-8")
print("expect_err replaced")
