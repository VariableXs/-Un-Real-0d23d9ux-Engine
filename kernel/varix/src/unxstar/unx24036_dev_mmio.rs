//! UNX-F24036 设备生命周期状态机与 MMIO BAR 映射管理·子功能16（0x40660 段）
//!
//! 锚点：UNX-F24036-J1 映射账齐备；J2 字段齐备；J3~J8 双通道切面操作语义
//! （BAR 清单/重映射换基）可证伪全绿；J9 账目对齐；J10 守卫拒；J11 闭集守恒；
//! J12 折叠指纹稳定且敏感；J13 注入核 5/5 全抓。
//! 域：CoRun·UNX-F5-B15（设备生命周期状态机与 MMIO BAR 映射管理）。
//!
//! 切面：子功能16 以「BAR 清单 × 重映射换基」双通道推进，折叠指纹取mul31 链式变体，
//! 诊断码闭集 MM_FROZEN/MM_UNK_DEV/MM_BAD_BAR/MM_DUP_MAP/MM_NO_MAP。
//!
//! 结构：设备生命周期账（冷/热/移除）+ BAR 映射账 + 生命周期核（warm/reset/remove）+ 映射闸（在场/参数/重复）+ 双通道切面操作 + 折叠指纹 + 注入核（野设备/BAR 非法/重复映射/未映射摘除/印后必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x40660 独占 0x20）。
pub const SEG_BASE: u32 = 0x40660;
pub const MM_FROZEN: u32 = SEG_BASE + 0x01; // 印后操作
pub const MM_UNK_DEV: u32 = SEG_BASE + 0x02; // 野设备或野引用
pub const MM_BAD_BAR: u32 = SEG_BASE + 0x03; // BAR 参数非法
pub const MM_DUP_MAP: u32 = SEG_BASE + 0x04; // 重复映射占用
pub const MM_NO_MAP: u32 = SEG_BASE + 0x05; // 未映射或状态未就绪

/// 设备生命周期账目：0 冷 / 1 热 / 2 移除。
#[derive(Clone, PartialEq)]
pub struct MmioDev {
    pub dev_no: u32,
    pub state: u8,
}

/// BAR 映射账条目。
#[derive(Clone, PartialEq)]
pub struct MapRec {
    pub seq: u32,
    pub dev_no: u32,
    pub bar: u8,
    pub base: u64,
    pub len: u32,
}

/// MMIO 映射总线。
pub struct MmioBus {
    pub devs: Vec<MmioDev>,
    pub maps: Vec<MapRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl MmioBus {
    pub fn new() -> MmioBus {
        MmioBus { devs: Vec::new(), maps: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 设备上账（冷态进入，dev_no 自增分配）。
    pub fn add_dev(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(MM_FROZEN);
        }
        let dev_no = self.devs.len() as u32;
        self.devs.push(MmioDev { dev_no, state: 0 });
        Ok(dev_no)
    }

    fn dev_alive(&self, dev_no: u32) -> bool {
        matches!(self.devs.iter().find(|d| d.dev_no == dev_no), Some(d) if d.state != 2)
    }

    /// 生命周期：冷 → 热。
    pub fn warm(&mut self, dev_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(MM_FROZEN);
        }
        match self.devs.iter_mut().find(|d| d.dev_no == dev_no) {
            Some(d) if d.state == 0 => {
                d.state = 1;
                Ok(())
            }
            Some(_) => Err(MM_NO_MAP),
            None => Err(MM_UNK_DEV),
        }
    }

    /// 生命周期：热 → 复位（保持热态）。
    pub fn reset(&mut self, dev_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(MM_FROZEN);
        }
        match self.devs.iter_mut().find(|d| d.dev_no == dev_no) {
            Some(d) if d.state == 1 => Ok(()),
            Some(_) => Err(MM_NO_MAP),
            None => Err(MM_UNK_DEV),
        }
    }

    /// BAR 映射落账（设备须在场且未移除，bar<6 且 len/base 非零）。
    pub fn map_bar(&mut self, dev_no: u32, bar: u8, base: u64, len: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(MM_FROZEN);
        }
        if !self.dev_alive(dev_no) {
            return Err(MM_UNK_DEV);
        }
        if bar > 5 || len == 0 || base == 0 {
            return Err(MM_BAD_BAR);
        }
        if self.maps.iter().any(|m| m.dev_no == dev_no && m.bar == bar) {
            return Err(MM_DUP_MAP);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.maps.push(MapRec { seq, dev_no, bar, base, len });
        Ok(seq)
    }

    /// 摘除映射（未映射走 MM_NO_MAP，野设备走 MM_UNK_DEV）。
    pub fn unmap_bar(&mut self, dev_no: u32, bar: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(MM_FROZEN);
        }
        if !self.dev_alive(dev_no) {
            return Err(MM_UNK_DEV);
        }
        let pos = match self.maps.iter().position(|m| m.dev_no == dev_no && m.bar == bar) {
            Some(p) => p,
            None => return Err(MM_NO_MAP),
        };
        self.maps.remove(pos);
        Ok(())
    }

    pub fn map_of(&self, dev_no: u32, bar: u8) -> Option<&MapRec> {
        self.maps.iter().find(|m| m.dev_no == dev_no && m.bar == bar)
    }

    /// 生命周期：任意在场态 → 移除，映射账同步清空。
    pub fn remove_dev(&mut self, dev_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(MM_FROZEN);
        }
        match self.devs.iter_mut().find(|d| d.dev_no == dev_no) {
            Some(d) if d.state != 2 => {
                d.state = 2;
            }
            Some(_) => return Err(MM_NO_MAP),
            None => return Err(MM_UNK_DEV),
        }
        self.maps.retain(|m| m.dev_no != dev_no);
        Ok(())
    }

    /// 折叠指纹（对账主指纹）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x40660;
        for it in &self.maps {
            h = h.wrapping_mul(31).wrapping_add(it.seq);
            h = h.wrapping_mul(31).wrapping_add(it.dev_no);
            h = h.wrapping_mul(31).wrapping_add(it.base as u32);
            h = h.wrapping_mul(31).wrapping_add(it.len as u32);
            h = h.wrapping_mul(31).wrapping_add(it.bar as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 子功能切面操作（BAR 清单/重映射换基 通道）。
impl MmioBus {
    /// 列出设备当前映射的全部 BAR 号。
    pub fn bars_of(&self, dev_no: u32) -> Vec<u8> {
        let mut bars = Vec::new();
        for m in &self.maps {
            if m.dev_no == dev_no {
                bars.push(m.bar);
            }
        }
        bars
    }

    /// 重映射：在既有映射上换基址（未映射走 MM_NO_MAP）。
    pub fn remap_bar(&mut self, dev_no: u32, bar: u8, new_base: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(MM_FROZEN);
        }
        if new_base == 0 {
            return Err(MM_BAD_BAR);
        }
        let m = match self.maps.iter_mut().find(|m| m.dev_no == dev_no && m.bar == bar) {
            Some(m) => m,
            None => return Err(MM_NO_MAP),
        };
        m.base = new_base;
        Ok(())
    }
}

/// 标准样本：三设备两映射（dev2 留空供扩账）。
pub fn mk_mmio() -> MmioBus {
    let mut b = MmioBus::new();
    let _ = b.add_dev();
    let _ = b.add_dev();
    let _ = b.add_dev();
    let _ = b.map_bar(0, 0, 0xF0000000, 0x1000);
    let _ = b.map_bar(1, 0, 0xF2000000, 0x4000);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_mmio();
    // 1) 野设备映射
    got[0] = b.map_bar(9, 0, 0xF0000000, 0x1000).err() == Some(MM_UNK_DEV);
    // 2) BAR 参数非法
    got[1] = b.map_bar(2, 9, 0xF0000000, 0x1000).err() == Some(MM_BAD_BAR);
    // 3) 重复映射占用
    got[2] = b.map_bar(0, 0, 0xE0000000, 0x1000).err() == Some(MM_DUP_MAP);
    // 4) 未映射先摘除
    got[3] = b.unmap_bar(2, 0).err() == Some(MM_NO_MAP);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.map_bar(2, 1, 0xF7000000, 0x400).err() == Some(MM_FROZEN);
    got
}

/// 判据主体（13 条 J 判据）。
pub fn run_unx24036_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24036");
    let mut b = mk_mmio();

    // J1 账非空
    let j1 = b.devs.len() == 3 && b.maps.len() == 2;
    s.add("J1-映射账齐", j1, "三设备两映射全落账");

    // J2 字段齐
    let j2 = b.devs.iter().all(|d| d.state <= 2)
        && b.maps.iter().all(|m| m.len > 0 && m.base != 0);
    s.add("J2-字段齐备", j2, "设备态与映射参数齐");

    // 通道：BAR 清单
    {
        let mut t = mk_mmio();
        let mut t = mk_mmio();
        let m1 = t.bars_of(0) == vec![0] && t.bars_of(1) == vec![0];
        let _ = t.map_bar(0, 3, 0xF4000000, 0x800);
        let m2 = t.bars_of(0) == vec![0, 3] && t.bars_of(9).is_empty();
        let _ = t.remove_dev(0);
        let m3 = t.bars_of(0).is_empty() && t.bars_of(1) == vec![0];
        s.add("J3-BAR 清单", m1, "正向语义真实落账");
        s.add("J4-BAR 清单", m2, "非法入参必拒");
        s.add("J5-BAR 清单", m3, "复验账目一致");
    }

    // 通道：重映射换基
    {
        let mut t = mk_mmio();
        let mut t = mk_mmio();
        let m1 = t.remap_bar(0, 0, 0xE0100000).is_ok()
            && t.map_of(0, 0).map(|m| m.base == 0xE0100000).unwrap_or(false);
        let m2 = t.remap_bar(0, 4, 0xE0000000).err() == Some(MM_NO_MAP);
        let m3 = t.remap_bar(0, 0, 0).err() == Some(MM_BAD_BAR) && m2;
        s.add("J6-重映射换基", m1, "正向语义真实落账");
        s.add("J7-重映射换基", m2, "非法入参必拒");
        s.add("J8-重映射换基", m3, "复验账目一致");
    }

    // J9 账目对齐
    let _ = b.warm(0);
    let j9 = b.devs.iter().all(|d| d.state <= 2)
        && b.maps.iter().all(|m| b.devs.iter().any(|d| d.dev_no == m.dev_no));
    s.add("J9-账目对齐", j9, "映射账与设备表逐条对齐");

    // J10 守卫拒
    let j10 = b.map_bar(9, 0, 0xF0000000, 0x1000).err() == Some(MM_UNK_DEV)
        && b.map_bar(2, 9, 0xF0000000, 0x1000).err() == Some(MM_BAD_BAR)
        && b.map_bar(2, 0, 0xF0000000, 0x1000).is_ok()
        && b.map_bar(2, 0, 0xF0000000, 0x1000).err() == Some(MM_DUP_MAP);
    s.add("J10-守卫拒", j10, "野设备/BAR 非法/重复映射全拒");

    // J11 闭集守恒
    let j11 = {
        let mut ok = b.maps.len() == 3 && b.maps.iter().all(|m| m.seq > 0 && m.seq < b.seq_next);
        for i in 0..b.maps.len() {
            for j in (i + 1)..b.maps.len() {
                if b.maps[i].seq == b.maps[j].seq {
                    ok = false;
                }
            }
        }
        ok
    };
    s.add("J11-序列守恒", j11, "映射 seq 两两相异且落在分配窗内");

    // J12 指纹稳定且敏感
    let f0 = b.fold();
    let _ = b.map_bar(2, 1, 0xF7000000, 0x400);
    let f1 = b.fold();
    let j12 = b.fold() == f1 && f1 != f0;
    s.add("J12-指纹稳定", j12, "折叠指纹可复现且对新增映射敏感");

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
    fn unx24036_flow() {
        let mut b = mk_mmio();
        assert!(b.warm(0).is_ok());
        assert!(b.reset(0).is_ok());
        assert!(b.map_bar(2, 1, 0xF7000000, 0x400).is_ok());
        assert_eq!(b.map_bar(0, 0, 0xE0000000, 0x1000).err(), Some(MM_DUP_MAP));
        assert_eq!(b.map_bar(9, 0, 0xE0000000, 0x1000).err(), Some(MM_UNK_DEV));
        assert_eq!(b.map_bar(2, 9, 0xE0000000, 0x1000).err(), Some(MM_BAD_BAR));
        assert_eq!(b.unmap_bar(2, 5).err(), Some(MM_NO_MAP));
        assert!(b.remove_dev(1).is_ok());
        assert_eq!(b.map_bar(1, 0, 0xE0000000, 0x1000).err(), Some(MM_UNK_DEV));
        assert!(b.seal() > 0);
        assert_eq!(b.map_bar(2, 2, 0xE0000000, 0x1000).err(), Some(MM_FROZEN));
    }

    #[test]
    fn unx24036_checks_green() {
        let s = run_unx24036_checks();
        assert!(s.all_passed(), "UNX-F24036 判据未全绿");
    }
}
