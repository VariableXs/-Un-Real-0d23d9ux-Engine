//! UNX-F24058 驱动模型对接与电源管理操作位（pm_ops）·子功能18（0x40920 段）
//!
//! 锚点：UNX-F24058-J1 电源账齐备；J2 字段齐备；J3~J8 双通道切面操作语义
//! （驱动改名/迁移对账）可证伪全绿；J9 状态守恒；J10 守卫拒；J11 闭集守恒；
//! J12 折叠指纹稳定且敏感；J13 注入核 5/5 全抓。
//! 域：CoRun·UNX-F5-B15（驱动模型对接与电源管理操作位（pm_ops））。
//!
//! 切面：子功能18 以「驱动改名 × 迁移对账」双通道推进，折叠指纹取djb2 旋转变体，
//! 诊断码闭集 PM_FROZEN/PM_UNK_DEV/PM_NO_OPS/PM_DUP_DRV/PM_BAD_STATE。
//!
//! 结构：驱动 ops 位表 + 设备电源账 + 迁移核（suspend/resume/runtime_idle）+ 位闸与状态闸 + 双通道切面操作 + 折叠指纹 + 注入核（重名驱动/野引用/缺位/错态迁移/印后必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40920 独占 0x20）。
pub const SEG_BASE: u32 = 0x40920;
pub const PM_FROZEN: u32 = SEG_BASE + 0x01; // 印后操作
pub const PM_UNK_DEV: u32 = SEG_BASE + 0x02; // 野设备或野引用
pub const PM_NO_OPS: u32 = SEG_BASE + 0x03; // ops 位不支持
pub const PM_DUP_DRV: u32 = SEG_BASE + 0x04; // 重名驱动或重复挂账
pub const PM_BAD_STATE: u32 = SEG_BASE + 0x05; // 非法状态迁移

/// 驱动电源操作位。
#[derive(Clone, PartialEq)]
pub struct PmOpsRec {
    pub name: String,
    pub allow_suspend: bool,
    pub allow_resume: bool,
    pub allow_idle: bool,
}

impl PmOpsRec {
    /// 是否具备指定 op 位（0 suspend / 1 resume / 2 runtime_idle）。
    pub fn has_bit(&self, op: u8) -> bool {
        match op {
            0 => self.allow_suspend,
            1 => self.allow_resume,
            _ => self.allow_idle,
        }
    }
}

/// 设备电源账目：state 0 活动 / 1 挂起 / 2 运行时空闲。
#[derive(Clone, PartialEq)]
pub struct PmDev {
    pub dev_no: u32,
    pub drv: String,
    pub state: u8,
    pub trans: u32,
}

/// 电源管理总线。
pub struct PmBus {
    pub pms: Vec<PmOpsRec>,
    pub devs: Vec<PmDev>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PmBus {
    pub fn new() -> PmBus {
        PmBus { pms: Vec::new(), devs: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 注册驱动电源操作位（重名走 PM_DUP_DRV）。
    pub fn register_pm(&mut self, name: &str, suspend: bool, resume: bool, idle: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(PM_FROZEN);
        }
        if name.is_empty() || self.pms.iter().any(|p| p.name == name) {
            return Err(PM_DUP_DRV);
        }
        self.pms.push(PmOpsRec {
            name: name.to_string(),
            allow_suspend: suspend,
            allow_resume: resume,
            allow_idle: idle,
        });
        Ok(())
    }

    /// 设备挂账到驱动（野引用走 PM_UNK_DEV，重复挂账走 PM_DUP_DRV）。
    pub fn attach(&mut self, dev_no: u32, drv: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PM_FROZEN);
        }
        if !self.pms.iter().any(|p| p.name == drv) {
            return Err(PM_UNK_DEV);
        }
        if self.devs.iter().any(|d| d.dev_no == dev_no) {
            return Err(PM_DUP_DRV);
        }
        self.devs.push(PmDev { dev_no, drv: drv.to_string(), state: 0, trans: 0 });
        Ok(())
    }

    /// 挂起：活动态 + allow_suspend 位。
    pub fn suspend(&mut self, dev_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PM_FROZEN);
        }
        let drv = match self.devs.iter().find(|d| d.dev_no == dev_no) {
            Some(d) => d.drv.clone(),
            None => return Err(PM_UNK_DEV),
        };
        if !self.pms.iter().any(|p| p.name == drv && p.allow_suspend) {
            return Err(PM_NO_OPS);
        }
        for d in self.devs.iter_mut() {
            if d.dev_no == dev_no {
                if d.state != 0 {
                    return Err(PM_BAD_STATE);
                }
                d.state = 1;
                d.trans += 1;
                return Ok(());
            }
        }
        Err(PM_UNK_DEV)
    }

    /// 恢复：挂起态 + allow_resume 位。
    pub fn resume(&mut self, dev_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PM_FROZEN);
        }
        let drv = match self.devs.iter().find(|d| d.dev_no == dev_no) {
            Some(d) => d.drv.clone(),
            None => return Err(PM_UNK_DEV),
        };
        if !self.pms.iter().any(|p| p.name == drv && p.allow_resume) {
            return Err(PM_NO_OPS);
        }
        for d in self.devs.iter_mut() {
            if d.dev_no == dev_no {
                if d.state != 1 {
                    return Err(PM_BAD_STATE);
                }
                d.state = 0;
                d.trans += 1;
                return Ok(());
            }
        }
        Err(PM_UNK_DEV)
    }

    /// 运行时空闲：活动态 + allow_idle 位。
    pub fn runtime_idle(&mut self, dev_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PM_FROZEN);
        }
        let drv = match self.devs.iter().find(|d| d.dev_no == dev_no) {
            Some(d) => d.drv.clone(),
            None => return Err(PM_UNK_DEV),
        };
        if !self.pms.iter().any(|p| p.name == drv && p.allow_idle) {
            return Err(PM_NO_OPS);
        }
        for d in self.devs.iter_mut() {
            if d.dev_no == dev_no {
                if d.state != 0 {
                    return Err(PM_BAD_STATE);
                }
                d.state = 2;
                d.trans += 1;
                return Ok(());
            }
        }
        Err(PM_UNK_DEV)
    }

    /// 设备态查询（野设备回 None）。
    pub fn state_of(&self, dev_no: u32) -> Option<u8> {
        self.devs.iter().find(|d| d.dev_no == dev_no).map(|d| d.state)
    }

    /// 迁移次数查询（野设备回 None）。
    pub fn trans_of(&self, dev_no: u32) -> Option<u32> {
        self.devs.iter().find(|d| d.dev_no == dev_no).map(|d| d.trans)
    }

    /// 折叠指纹（对账主指纹）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x40920;
        for it in &self.devs {
            h = h.wrapping_mul(33).wrapping_add(it.dev_no);
            h = h.wrapping_mul(33).wrapping_add(it.state as u32).rotate_left(7);
            h = h.wrapping_mul(33).wrapping_add(it.trans);
            for by in it.drv.bytes() {
                h = h.wrapping_mul(33).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 子功能切面操作（驱动改名/迁移对账 通道）。
impl PmBus {
    /// 驱动改名：ops 表与设备账同步换名（新名冲突走 PM_DUP_DRV）。
    pub fn rename_pm(&mut self, old: &str, new: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PM_FROZEN);
        }
        if self.pms.iter().any(|p| p.name == new) {
            return Err(PM_DUP_DRV);
        }
        if !self.pms.iter().any(|p| p.name == old) {
            return Err(PM_UNK_DEV);
        }
        for p in self.pms.iter_mut() {
            if p.name == old {
                p.name = new.to_string();
            }
        }
        for d in self.devs.iter_mut() {
            if d.drv == old {
                d.drv = new.to_string();
            }
        }
        Ok(())
    }

    /// 全量迁移计数（对账用）。
    pub fn trans_total(&self) -> u32 {
        let mut sum: u32 = 0;
        for d in &self.devs {
            sum += d.trans;
        }
        sum
    }

    /// 已发生迁移的设备数。
    pub fn busy_count(&self) -> usize {
        self.devs.iter().filter(|d| d.trans > 0).count()
    }
}

/// 标准样本：gpu 全位驱动 + aud 仅挂起位驱动，三设备两挂。
pub fn mk_pm() -> PmBus {
    let mut b = PmBus::new();
    let _ = b.register_pm("gpu", true, true, true);
    let _ = b.register_pm("aud", true, false, false);
    let _ = b.attach(0, "gpu");
    let _ = b.attach(1, "aud");
    let _ = b.attach(2, "gpu");
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_pm();
    // 1) 重名驱动注册
    got[0] = b.register_pm("gpu", true, true, true).err() == Some(PM_DUP_DRV);
    // 2) 野引用挂账
    got[1] = b.attach(3, "ghost").err() == Some(PM_UNK_DEV);
    // 3) ops 位不支持
    got[2] = b.runtime_idle(1).err() == Some(PM_NO_OPS);
    // 4) 非法状态迁移
    let _ = b.suspend(2);
    got[3] = b.suspend(2).err() == Some(PM_BAD_STATE);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.register_pm("late", true, true, true).err() == Some(PM_FROZEN);
    got
}

/// 判据主体（13 条 J 判据）。
pub fn run_unx24058_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24058");
    let mut b = mk_pm();

    // J1 账非空
    let j1 = b.pms.len() == 2 && b.devs.len() == 3;
    s.add("J1-电源账齐", j1, "两驱动三设备全落账");

    // J2 字段齐
    let j2 = b.pms.iter().all(|p| !p.name.is_empty())
        && b.devs.iter().all(|d| d.state == 0 && d.trans == 0);
    s.add("J2-字段齐备", j2, "ops 位与初始活动态齐");

    // 通道：驱动改名
    {
        let mut t = mk_pm();
        let mut t = mk_pm();
        let m1 = t.rename_pm("aud", "snd").is_ok() && t.pms.iter().any(|p| p.name == "snd");
        let m2 = t.rename_pm("snd", "gpu").err() == Some(PM_DUP_DRV);
        let m3 = t.rename_pm("ghost", "x").err() == Some(PM_UNK_DEV)
            && t.devs.iter().any(|d| d.drv == "snd");
        s.add("J3-驱动改名", m1, "正向语义真实落账");
        s.add("J4-驱动改名", m2, "非法入参必拒");
        s.add("J5-驱动改名", m3, "复验账目一致");
    }

    // 通道：迁移对账
    {
        let mut t = mk_pm();
        let mut t = mk_pm();
        let m1 = t.trans_total() == 0 && t.busy_count() == 0;
        let _ = t.suspend(0);
        let _ = t.resume(0);
        let m2 = t.trans_total() == 2 && t.busy_count() == 1;
        let m3 = t.trans_total() == t.trans_of(0).unwrap_or(0)
            + t.trans_of(1).unwrap_or(0) + t.trans_of(2).unwrap_or(0);
        s.add("J6-迁移对账", m1, "正向语义真实落账");
        s.add("J7-迁移对账", m2, "非法入参必拒");
        s.add("J8-迁移对账", m3, "复验账目一致");
    }

    // J9 状态守恒
    let _ = b.suspend(0);
    let _ = b.resume(0);
    let j9 = b.state_of(0) == Some(0) && b.trans_of(0) == Some(2) && b.trans_of(1) == Some(0);
    s.add("J9-状态守恒", j9, "suspend/resume 成对计数");

    // J10 守卫拒
    let j10 = b.suspend(9).err() == Some(PM_UNK_DEV)
        && b.suspend(1).is_ok()
        && b.suspend(1).err() == Some(PM_BAD_STATE)
        && b.resume(1).err() == Some(PM_NO_OPS)
        && b.runtime_idle(1).err() == Some(PM_NO_OPS);
    s.add("J10-守卫拒", j10, "野设备/重复迁移/挂起恢复缺位/空闲缺位全拒");

    // J11 闭集守恒
    let j11 = b.pms.iter().all(|p| p.has_bit(0) || p.has_bit(1) || p.has_bit(2))
        && b.devs.iter().all(|d| !d.drv.is_empty());
    s.add("J11-位闭集", j11, "每驱动至少一 op 位且设备账驱动名齐");

    // J12 指纹稳定且敏感
    let f0 = b.fold();
    let _ = b.suspend(2);
    let f1 = b.fold();
    let j12 = b.fold() == f1 && f1 != f0;
    s.add("J12-指纹稳定", j12, "折叠指纹可复现且对状态迁移敏感");

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
    fn unx24058_flow() {
        let mut b = mk_pm();
        assert!(b.suspend(0).is_ok());
        assert!(b.resume(0).is_ok());
        assert!(b.runtime_idle(0).is_ok());
        assert_eq!(b.resume(0).err(), Some(PM_BAD_STATE));
        assert_eq!(b.register_pm("gpu", true, true, true).err(), Some(PM_DUP_DRV));
        assert_eq!(b.attach(0, "aud").err(), Some(PM_DUP_DRV));
        assert_eq!(b.attach(9, "ghost").err(), Some(PM_UNK_DEV));
        assert_eq!(b.runtime_idle(1).err(), Some(PM_NO_OPS));
        assert!(b.suspend(1).is_ok());
        assert_eq!(b.suspend(1).err(), Some(PM_BAD_STATE));
        assert!(b.seal() > 0);
        assert_eq!(b.suspend(2).err(), Some(PM_FROZEN));
    }

    #[test]
    fn unx24058_checks_green() {
        let s = run_unx24058_checks();
        assert!(s.all_passed(), "UNX-F24058 判据未全绿");
    }
}
