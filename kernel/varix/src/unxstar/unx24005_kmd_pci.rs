//! UNX-F24005 KMD 驱动注册与 PCI 设备探测框架·子功能05（0x40280 段）
//!
//! 锚点：UNX-F24005-J1 注册账齐备；J2 字段齐备；J3~J8 双通道切面操作语义
//! （驱动改名/重绑换账）可证伪全绿；J9 对账守恒；J10 守卫拒；J11 闭集守恒；
//! J12 折叠指纹稳定且敏感；J13 注入核 5/5 全抓。
//! 域：CoRun·UNX-F5-B15（KMD 驱动注册与 PCI 设备探测框架）。
//!
//! 切面：子功能05 以「驱动改名 × 重绑换账」双通道推进，折叠指纹取FNV-1a变体，
//! 诊断码闭集 KP_DUP_DRV/KP_UNK_DEV/KP_NO_MATCH/KP_DUP_BIND/KP_FROZEN。
//!
//! 结构：PCI 设备表 + 驱动注册表 + 绑定账本 + 探测核（匹配闸/幂等 bind 拒/封印闸）+ 双通道切面操作 + 折叠指纹 + 注入核（重名驱动/野设备/不匹配/重复 bind/印后注册必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40280 独占 0x20）。
pub const SEG_BASE: u32 = 0x40280;
pub const KP_DUP_DRV: u32 = SEG_BASE + 0x01; // 重名驱动或驱动表冲突
pub const KP_UNK_DEV: u32 = SEG_BASE + 0x02; // 野设备或野引用
pub const KP_NO_MATCH: u32 = SEG_BASE + 0x03; // vendor/device 不匹配
pub const KP_DUP_BIND: u32 = SEG_BASE + 0x04; // 重复 bind
pub const KP_FROZEN: u32 = SEG_BASE + 0x05; // 印后操作

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

    /// 解绑：摘除绑定账并回退驱动计数（无账可摘走 KP_UNK_DEV）。
    pub fn unbind(&mut self, dev_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(KP_FROZEN);
        }
        let d = match self.devs.iter().find(|x| x.dev_no == dev_no) {
            Some(d) => d.key(),
            None => return Err(KP_UNK_DEV),
        };
        let pos = match self.binds.iter().position(|b| b.dev_key == d) {
            Some(p) => p,
            None => return Err(KP_UNK_DEV),
        };
        let drv = self.binds[pos].drv_name.clone();
        self.binds.remove(pos);
        if let Some(r) = self.drivers.iter_mut().find(|r| r.name == drv) {
            r.bind_count -= 1;
        }
        Ok(())
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

    /// 折叠指纹（对账主指纹）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        for it in &self.binds {
            h = (h ^ it.seq).wrapping_mul(0x01000193);
            for by in it.drv_name.bytes() {
                h = (h ^ by as u32).wrapping_mul(0x01000193);
            }
            h = (h ^ it.dev_key).wrapping_mul(0x01000193);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 子功能切面操作（驱动改名/重绑换账 通道）。
impl PciBus {
    /// 驱动改名：注册表与绑定账同步换名（新名冲突走 KP_DUP_DRV）。
    pub fn rename_driver(&mut self, old: &str, new: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(KP_FROZEN);
        }
        if self.drivers.iter().any(|r| r.name == new) {
            return Err(KP_DUP_DRV);
        }
        if !self.drivers.iter().any(|r| r.name == old) {
            return Err(KP_UNK_DEV);
        }
        for r in self.drivers.iter_mut() {
            if r.name == old {
                r.name = new.to_string();
            }
        }
        for b in self.binds.iter_mut() {
            if b.drv_name == old {
                b.drv_name = new.to_string();
            }
        }
        Ok(())
    }

    /// 重绑：摘旧账再探新账，回新 seq（无旧账走 KP_DUP_BIND）。
    pub fn rebind(&mut self, dev_no: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(KP_FROZEN);
        }
        let old = match self.bind_of(dev_no) {
            Some(b) => b.seq,
            None => return Err(KP_DUP_BIND),
        };
        let key = match self.devs.iter().find(|x| x.dev_no == dev_no) {
            Some(d) => d.key(),
            None => return Err(KP_UNK_DEV),
        };
        let pos = match self.binds.iter().position(|b| b.dev_key == key) {
            Some(p) => p,
            None => return Err(KP_DUP_BIND),
        };
        let drv = self.binds[pos].drv_name.clone();
        self.binds.remove(pos);
        if let Some(r) = self.drivers.iter_mut().find(|r| r.name == drv) {
            r.bind_count -= 1;
        }
        match self.probe(dev_no) {
            Ok(seq) if seq != old => Ok(seq),
            Ok(_) => Err(KP_DUP_BIND),
            Err(e) => Err(e),
        }
    }
}

/// 标准样本：两驱动三设备（dev2 无驱动匹配）。
pub fn mk_bus() -> PciBus {
    let mut b = PciBus::new();
    let _ = b.register_driver("vkdrv", 0x10DE, &[0x2544, 0x2504]);
    let _ = b.register_driver("ixdrv", 0x8086, &[0x4680, 0x4692]);
    let _ = b.add_dev(0, 0x08, 0x10DE, 0x2544, 0x03);
    let _ = b.add_dev(0, 0x10, 0x8086, 0x4680, 0x03);
    let _ = b.add_dev(1, 0x04, 0x10DE, 0x9999, 0x06);
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

/// 判据主体（13 条 J 判据）。
pub fn run_unx24005_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24005");
    let mut b = mk_bus();

    // J1 账非空
    let j1 = b.drivers.len() == 2 && b.devs.len() == 3;
    s.add("J1-注册账齐", j1, "两驱动三设备全落表");

    // J2 字段齐
    let j2 = b.drivers.iter().all(|d| !d.device_ids.is_empty())
        && b.devs.iter().all(|d| d.vendor_id != 0);
    s.add("J2-字段齐备", j2, "驱动 id 表与设备厂商号齐");

    // 通道：驱动改名
    {
        let mut t = mk_bus();
        let mut t = mk_bus();
        let _ = t.probe(0);
        let m1 = t.rename_driver("vkdrv", "nvk").is_ok()
            && t.bind_of(0).map(|b| b.drv_name == "nvk").unwrap_or(false);
        let m2 = t.rename_driver("nvk", "ixdrv").err() == Some(KP_DUP_DRV);
        let m3 = t.rename_driver("ghost", "x").err() == Some(KP_UNK_DEV)
            && t.drivers.iter().any(|r| r.name == "nvk");
        s.add("J3-驱动改名", m1, "正向语义真实落账");
        s.add("J4-驱动改名", m2, "非法入参必拒");
        s.add("J5-驱动改名", m3, "复验账目一致");
    }

    // 通道：重绑换账
    {
        let mut t = mk_bus();
        let mut t = mk_bus();
        let first = match t.probe(0) { Ok(s) => s, Err(_) => 0 };
        let m1 = t.rebind(0).is_ok() && t.bind_of(0).is_some();
        let m2 = t.rebind(2).err() == Some(KP_DUP_BIND) && t.rebind(9).err() == Some(KP_DUP_BIND);
        let m3 = match t.bind_of(0) { Some(b) => b.seq > first, None => false };
        s.add("J6-重绑换账", m1, "正向语义真实落账");
        s.add("J7-重绑换账", m2, "非法入参必拒");
        s.add("J8-重绑换账", m3, "复验账目一致");
    }

    // J9 对账守恒
    let _ = b.probe(0);
    let _ = b.probe(1);
    let j9 = b.drivers.iter().map(|d| d.bind_count as usize).sum::<usize>() == b.binds.len();
    s.add("J9-计数守恒", j9, "绑定计数总和与绑定账长度一致");

    // J10 守卫拒
    let j10 = b.probe(99).err() == Some(KP_UNK_DEV)
        && b.register_driver("xx", 0x1, &[]).err() == Some(KP_NO_MATCH)
        && b.probe(2).err() == Some(KP_NO_MATCH);
    s.add("J10-守卫拒", j10, "野设备/空 id 表/无匹配三类守卫全拒");

    // J11 闭集守恒
    let j11 = b.seq_unique() && b.binds.len() == 2 && b.binds.iter().all(|x| x.seq > 0);
    s.add("J11-序列守恒", j11, "seq 两两相异且逐条正序落账");

    // J12 指纹稳定且敏感
    let f0 = b.fold();
    let _ = b.unbind(0);
    let f1 = b.fold();
    let j12 = b.fold() == f1 && f1 != f0;
    s.add("J12-指纹稳定", j12, "折叠指纹可复现且对账本增删敏感");

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
    fn unx24005_flow() {
        let mut b = mk_bus();
        assert!(b.probe(0).is_ok());
        assert!(b.probe(1).is_ok());
        assert_eq!(b.probe(2).err(), Some(KP_NO_MATCH));
        assert_eq!(b.probe(0).err(), Some(KP_DUP_BIND));
        assert_eq!(b.probe(99).err(), Some(KP_UNK_DEV));
        assert_eq!(b.register_driver("vkdrv", 0x10DE, &[0x1]).err(), Some(KP_DUP_DRV));
        assert!(b.seal() > 0);
        assert_eq!(b.register_driver("x", 0x1, &[0x1]).err(), Some(KP_FROZEN));
    }

    #[test]
    fn unx24005_checks_green() {
        let s = run_unx24005_checks();
        assert!(s.all_passed(), "UNX-F24005 判据未全绿");
    }
}
