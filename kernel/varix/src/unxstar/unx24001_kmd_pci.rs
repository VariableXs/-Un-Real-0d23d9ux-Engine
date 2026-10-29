//! UNX-F24001 KMD 驱动注册与 PCI 设备探测框架·子功能01（0x40200 段）
//!
//! 锚点：UNX-F24001-J1 注册账（驱动注册表与 PCI 设备表逐条落账）；
//! J2 探测语义（vendor/device 匹配闸 + 幂等 bind 拒 + 封印闸三性
//! 质）；J3 探测指纹（绑定账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（KMD 驱动注册与 PCI 探测面）。
//!
//! 结构：PCI 设备表 + 驱动注册表 + id_match 探测核 + 绑定账本 + 折
//! 叠指纹 + 注入核（重名驱动/野设备/不匹配/重复 bind/印后注册必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40200 独占 0x20）。
pub const SEG_BASE: u32 = 0x40200;
pub const KP_DUP_DRV: u32 = SEG_BASE + 0x01; // 重名驱动
pub const KP_UNK_DEV: u32 = SEG_BASE + 0x02; // 野设备
pub const KP_NO_MATCH: u32 = SEG_BASE + 0x03; // vendor/device 不匹配
pub const KP_DUP_BIND: u32 = SEG_BASE + 0x04; // 重复 bind
pub const KP_FROZEN: u32 = SEG_BASE + 0x05; // 印后注册

/// PCI 设备条目。
#[derive(Clone, PartialEq)]
pub struct PciDev {
    pub dev_no: u32,
    pub bus: u8,
    pub slot: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
}

impl PciDev {
    pub fn key(&self) -> u32 {
        ((self.bus as u32) << 16) | ((self.slot as u32) << 8) | self.dev_no as u32
    }
}

/// 驱动注册条目。
#[derive(Clone)]
pub struct DriverRec {
    pub name: String,
    pub vendor_id: u16,
    pub device_ids: Vec<u16>,
    pub bind_count: u32,
}

impl DriverRec {
    /// 是否支持该 (vendor, device)。
    pub fn matches(&self, vendor_id: u16, device_id: u16) -> bool {
        self.vendor_id == vendor_id && self.device_ids.contains(&device_id)
    }
}

/// 绑定账。
#[derive(Clone, PartialEq)]
pub struct BindRec {
    pub seq: u32,
    pub dev_key: u32,
    pub drv_name: String,
}

/// PCI 探测总线。
pub struct PciBus {
    pub devs: Vec<PciDev>,
    pub drivers: Vec<DriverRec>,
    pub binds: Vec<BindRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PciBus {
    pub fn new() -> PciBus {
        PciBus { devs: Vec::new(), drivers: Vec::new(), binds: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn add_dev(&mut self, bus: u8, slot: u8, vendor_id: u16, device_id: u16, class_code: u8) -> Result<u32, u32> {
        if self.sealed {
            return Err(KP_FROZEN);
        }
        let dev_no = self.devs.len() as u32;
        let d = PciDev { dev_no, bus, slot, vendor_id, device_id, class_code };
        if self.devs.iter().any(|x| x.key() == d.key()) {
            return Err(KP_UNK_DEV);
        }
        self.devs.push(d);
        Ok(dev_no)
    }

    pub fn register_driver(&mut self, name: &str, vendor_id: u16, device_ids: &[u16]) -> Result<(), u32> {
        if self.sealed {
            return Err(KP_FROZEN);
        }
        if name.is_empty() || self.drivers.iter().any(|d| d.name == name) {
            return Err(KP_DUP_DRV);
        }
        if device_ids.is_empty() {
            return Err(KP_NO_MATCH);
        }
        self.drivers.push(DriverRec {
            name: name.to_string(),
            vendor_id,
            device_ids: device_ids.to_vec(),
            bind_count: 0,
        });
        Ok(())
    }

    /// 探测：为设备找首个匹配驱动并落绑定账。
    pub fn probe(&mut self, dev_no: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(KP_FROZEN);
        }
        let d = match self.devs.iter().find(|x| x.dev_no == dev_no) {
            Some(d) => d.clone(),
            None => return Err(KP_UNK_DEV),
        };
        let drv = match self.drivers.iter().find(|r| r.matches(d.vendor_id, d.device_id)) {
            Some(r) => r.name.clone(),
            None => return Err(KP_NO_MATCH),
        };
        if self.binds.iter().any(|b| b.dev_key == d.key() && b.drv_name == drv) {
            return Err(KP_DUP_BIND);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.binds.push(BindRec { seq, dev_key: d.key(), drv_name: drv.clone() });
        if let Some(r) = self.drivers.iter_mut().find(|r| r.name == drv) {
            r.bind_count += 1;
        }
        Ok(seq)
    }

    pub fn bind_of(&self, dev_no: u32) -> Option<&BindRec> {
        let d = self.devs.iter().find(|x| x.dev_no == dev_no)?;
        self.binds.iter().find(|b| b.dev_key == d.key())
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.binds.len() {
            for j in (i + 1)..self.binds.len() {
                if self.binds[i].seq == self.binds[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x40200;
        for b in &self.binds {
            h = h.wrapping_mul(31).wrapping_add(b.seq);
            h = h.wrapping_mul(31).wrapping_add(b.dev_key);
            for by in b.drv_name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两驱动三设备。
pub fn mk_bus() -> PciBus {
    let mut b = PciBus::new();
    let _ = b.register_driver("vkdrv", 0x10DE, &[0x2544, 0x2504]);
    let _ = b.register_driver("ixdrv", 0x8086, &[0x4680, 0x4692]);
    let _ = b.add_dev(0, 0x08, 0x10DE, 0x2544, 0x03);
    let _ = b.add_dev(0, 0x10, 0x8086, 0x4680, 0x03);
    let _ = b.add_dev(1, 0x04, 0x10DE, 0x9999, 0x06); // 无驱动匹配
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_bus();
    // 1) 重名驱动
    got[0] = b.register_driver("vkdrv", 0x10DE, &[0x1111]).err() == Some(KP_DUP_DRV);
    // 2) 野设备探测
    got[1] = b.probe(99).err() == Some(KP_UNK_DEV);
    // 3) 无驱动匹配
    got[2] = b.probe(2).err() == Some(KP_NO_MATCH);
    // 4) 重复 bind
    let _ = b.probe(0);
    got[3] = b.probe(0).err() == Some(KP_DUP_BIND);
    // 5) 印后注册必抓
    b.seal();
    got[4] = b.register_driver("laterv", 0x1234, &[0x1]).err() == Some(KP_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24001");
    let mut b = mk_bus();

    // J1 注册账非空
    let j1 = b.drivers.len() == 2 && b.devs.len() == 3;
    s.add("J1-注册账齐", j1, "两驱动三设备全落表");

    // J2 驱动 id 表齐
    let j2 = b.drivers.iter().all(|d| !d.device_ids.is_empty());
    s.add("J2-id表齐备", j2, "驱动 device_ids 非空");

    // J3 匹配闸正例
    let j3 = b.drivers[0].matches(0x10DE, 0x2544) && b.drivers[1].matches(0x8086, 0x4680);
    s.add("J3-匹配正例", j3, "vendor/device 双匹配");

    // J4 匹配闸反例
    let j4 = !b.drivers[0].matches(0x8086, 0x2544) && !b.drivers[0].matches(0x10DE, 0x4680);
    s.add("J4-匹配反例", j4, "单侧不符即不匹配");

    // J5 探测成功落账
    let j5 = b.probe(0).is_ok() && b.bind_of(0).map(|x| x.drv_name == "vkdrv").unwrap_or(false);
    s.add("J5-探测落账", j5, "dev0 绑 vkdrv");

    // J6 第二设备独立绑定
    let j6 = b.probe(1).is_ok() && b.bind_of(1).map(|x| x.drv_name == "ixdrv").unwrap_or(false);
    s.add("J6-独立绑定", j6, "dev1 绑 ixdrv");

    // J7 无匹配拒
    let j7 = b.probe(2).err() == Some(KP_NO_MATCH);
    s.add("J7-无匹配拒", j7, "无驱动匹配设备必拒");

    // J8 重复 bind 拒
    let j8 = b.probe(0).err() == Some(KP_DUP_BIND);
    s.add("J8-重复bind拒", j8, "同设备同驱动二次探测必拒");

    // J9 绑定计数守恒
    let j9 = b.drivers[0].bind_count == 1 && b.drivers[1].bind_count == 1;
    s.add("J9-计数守恒", j9, "bind_count 与账一致");

    // J10 野设备拒
    let j10 = b.probe(99).err() == Some(KP_UNK_DEV);
    s.add("J10-野设备拒", j10, "越表设备号必拒");

    // J11 重名驱动拒
    let j11 = b.register_driver("vkdrv", 0x10DE, &[0x2]).err() == Some(KP_DUP_DRV);
    s.add("J11-重名驱动拒", j11, "驱动重名必拒");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24001_probe_flow() {
        let mut b = mk_bus();
        assert!(b.probe(0).is_ok());
        assert!(b.probe(1).is_ok());
        assert_eq!(b.probe(2), Err(KP_NO_MATCH));
        assert_eq!(b.probe(0), Err(KP_DUP_BIND));
        assert_eq!(b.probe(99), Err(KP_UNK_DEV));
        assert_eq!(b.register_driver("vkdrv", 0x10DE, &[0x1]), Err(KP_DUP_DRV));
        assert!(b.seal() > 0);
        assert_eq!(b.register_driver("x", 0x1, &[0x1]), Err(KP_FROZEN));
    }

    #[test]
    fn unx24001_checks_green() {
        let s = run_unx24001_checks();
        assert!(s.all_passed(), "UNX-F24001 判据未全绿");
    }
}
