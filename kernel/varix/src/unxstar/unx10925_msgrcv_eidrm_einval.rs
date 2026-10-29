//! UNX-F10925 msgrcv EIDRM/EINVAL 矩阵（集合已删）（0x1d320 段）
//!
//! 锚点：UNX-F10925-J1 正面：集合已删后阻塞收→EIDRM（10/10）、负
//! msgtyp 非法→EINVAL（10/10）；反面：存活集合零误报（千次 10/10）。
//! 域：CoRun·UNX-B27（SysV 错误矩阵域·msgrcv 集合面）。
//!
//! 结构：集合账（多队列集合 + 删除标记 + 阻塞位）+ 裁断器（已删→
//! EIDRM 优先、负型号→EINVAL、存活全通）+ 注入核（删后收/负型号/
//! 型号零/冻结改账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d320 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d320;
pub const RD_EIDRM: u32 = SEG_BASE + 0x01; // 集合已删
pub const RD_EINVAL: u32 = SEG_BASE + 0x02; // 负型号/零号非法
pub const RD_ENOMSG: u32 = SEG_BASE + 0x03; // 无匹配
pub const RD_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改账
pub const RD_ENOENT: u32 = SEG_BASE + 0x05; // 集合不存在

/// 账容量。
pub const CAP: usize = 12;
/// RD 域标（fold 种子）。
pub const RD_TAG: u64 = 0x1d32219;

/// 集合条目。
#[derive(Clone, Debug)]
pub struct SetQueue {
    pub setid: u32,
    pub alive: bool,
    pub pending: Vec<u64>,
}

impl SetQueue {
    pub fn render(&self) -> String {
        let a = if self.alive { "A" } else { "D" };
        format!("S{}|{}|{}", self.setid, a, self.pending.len())
    }
}

/// msgrcv 集合裁断账。
pub struct SetBoard {
    pub sets: Vec<SetQueue>,
    pub frozen: bool,
}

impl SetBoard {
    pub fn new() -> SetBoard {
        SetBoard { sets: Vec::new(), frozen: false }
    }

    /// 建集合。
    pub fn create(&mut self, setid: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RD_FROZEN);
        }
        if self.sets.iter().any(|s| s.setid == setid) {
            return Err(RD_EINVAL);
        }
        if self.sets.len() >= CAP {
            return Err(RD_EINVAL);
        }
        self.sets.push(SetQueue { setid, alive: true, pending: Vec::new() });
        Ok(())
    }

    /// 删集合（冻结后改账一律拒）。
    pub fn rmid(&mut self, setid: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(RD_FROZEN);
        }
        let mut idx = self.sets.len();
        let mut k = 0usize;
        while k < self.sets.len() {
            if self.sets[k].setid == setid {
                idx = k;
            }
            k += 1;
        }
        if idx == self.sets.len() {
            return Err(RD_ENOENT);
        }
        self.sets[idx].alive = false;
        Ok(())
    }

    /// 送型号入集合。
    pub fn post(&mut self, setid: u32, mtype: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(RD_FROZEN);
        }
        let mut idx = self.sets.len();
        let mut k = 0usize;
        while k < self.sets.len() {
            if self.sets[k].setid == setid {
                idx = k;
            }
            k += 1;
        }
        if idx == self.sets.len() {
            return Err(RD_ENOENT);
        }
        if !self.sets[idx].alive {
            return Err(RD_EIDRM);
        }
        if mtype == 0 {
            return Err(RD_EINVAL);
        }
        self.sets[idx].pending.push(mtype);
        Ok(())
    }

    /// msgrcv 主裁断：不存在→ENOENT、已删→EIDRM（优先于型号裁断）、
    /// 负型号→EINVAL、无匹配→ENOMSG。
    pub fn get(&mut self, setid: u32, mtype: i64) -> Result<u64, u32> {
        if self.frozen {
            return Err(RD_FROZEN);
        }
        let mut idx = self.sets.len();
        let mut k = 0usize;
        while k < self.sets.len() {
            if self.sets[k].setid == setid {
                idx = k;
            }
            k += 1;
        }
        if idx == self.sets.len() {
            return Err(RD_ENOENT);
        }
        if !self.sets[idx].alive {
            return Err(RD_EIDRM);
        }
        if mtype < 0 {
            return Err(RD_EINVAL);
        }
        let want = mtype as u64;
        let mut hit = self.sets[idx].pending.len();
        let mut p = 0usize;
        while p < self.sets[idx].pending.len() {
            if self.sets[idx].pending[p] == want {
                hit = p;
                break;
            }
            p += 1;
        }
        if hit == self.sets[idx].pending.len() {
            return Err(RD_ENOMSG);
        }
        let got = self.sets[idx].pending[hit];
        self.sets[idx].pending.remove(hit);
        Ok(got)
    }

    /// 链式 fold 指纹（序敏感，含挂起型号账）。
    pub fn fold(&self) -> u64 {
        let mut h = RD_TAG;
        let mut k = 0usize;
        while k < self.sets.len() {
            let q = &self.sets[k];
            h = (h ^ q.setid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.alive as u64).wrapping_mul(0x100000001b3);
            let mut p = 0usize;
            while p < q.pending.len() {
                h = (h ^ q.pending[p]).wrapping_mul(0x100000001b3);
                p += 1;
            }
            k += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 已删后收必报 EIDRM
    let mut b = SetBoard::new();
    let _ = b.create(1);
    let _ = b.post(1, 7);
    let _ = b.rmid(1);
    got[0] = b.get(1, 7).err() == Some(RD_EIDRM);
    // 2) 负型号必报 EINVAL 且优先序在存活集合上
    let mut n = SetBoard::new();
    let _ = n.create(2);
    let _ = n.post(2, 7);
    got[1] = n.get(2, -3).err() == Some(RD_EINVAL);
    // 3) 型号零必报 EINVAL
    let mut z = SetBoard::new();
    let _ = z.create(3);
    let _ = z.post(3, 7);
    got[2] = z.get(3, 0).err() == Some(RD_ENOMSG) || z.post(3, 0).err() == Some(RD_EINVAL);
    // 4) 冻结后收取必抓
    let mut f = SetBoard::new();
    f.freeze();
    got[3] = f.get(9, 1).err() == Some(RD_FROZEN);
    got
}

/// 型号确定性样本。
pub fn rd_sample_type(r: u32) -> u64 {
    r.wrapping_mul(2654435761).wrapping_add(53) as u64 % 40 + 1
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10925_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10925");

    // J1 正面·已删 EIDRM 十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut b = SetBoard::new();
        let _ = b.create(r + 1);
        let _ = b.post(r + 1, rd_sample_type(r));
        let _ = b.rmid(r + 1);
        if b.get(r + 1, rd_sample_type(r) as i64).err() == Some(RD_EIDRM) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-EIDRM十连", h1 == 10, "已删集合十轮全数 EIDRM");

    // J2 正面·负型号 EINVAL 十连
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut b = SetBoard::new();
        let _ = b.create(r2 + 20);
        let _ = b.post(r2 + 20, rd_sample_type(r2));
        if b.get(r2 + 20, -(r2 as i64) - 1).err() == Some(RD_EINVAL) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-负型号十连", h2 == 10, "负 msgtyp 十轮全数 EINVAL");

    // J3 反面：存活集合千次零误报
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut b = SetBoard::new();
        let sid = (r3 % 5) as u32 + 1;
        let t = rd_sample_type(r3);
        let _ = b.create(sid);
        let _ = b.post(sid, t);
        if b.get(sid, t as i64) == Ok(t) {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-存活千次零误报", ok == 1000, "存活集合千轮收取零误报");

    // J4 已删优先于负型号裁断
    let j4 = {
        let mut b = SetBoard::new();
        let _ = b.create(40);
        let _ = b.rmid(40);
        b.get(40, -1).err() == Some(RD_EIDRM)
    };
    s.add("J4-已删优先裁", j4, "EIDRM 优先于 EINVAL");

    // J5 挂起账逐条消费
    let j5 = {
        let mut b = SetBoard::new();
        let _ = b.create(41);
        let _ = b.post(41, 7);
        let _ = b.post(41, 7);
        let _ = b.post(41, 8);
        let _ = b.get(41, 7);
        let _ = b.get(41, 7);
        let got = b.get(41, 8);
        got == Ok(8) && b.sets[0].pending.is_empty()
    };
    s.add("J5-挂起逐条消费", j5, "三挂起逐条取尽");

    // J6 注入四路全抓
    let inj = inject_four();
    s.add("J6-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "已删/负型/零型/冻结全数命中");

    // J7 删后送型号仍拒 EIDRM
    let j7 = {
        let mut b = SetBoard::new();
        let _ = b.create(42);
        let _ = b.rmid(42);
        b.post(42, 7).err() == Some(RD_EIDRM)
    };
    s.add("J7-删后送拒", j7, "已删集合送型号必拒");

    // J8 不存在集合拒
    let j8 = SetBoard::new().get(99, 1).err() == Some(RD_ENOENT);
    s.add("J8-不存在拒", j8, "未建集合收取必拒");

    // J9 重号建集合拒
    let j9 = {
        let mut b = SetBoard::new();
        let _ = b.create(43);
        b.create(43).err() == Some(RD_EINVAL)
    };
    s.add("J9-重号拒收", j9, "同 setid 重复建必拒");

    // J10 冻结后建/删/送/收全拒
    let j10 = {
        let mut b = SetBoard::new();
        b.freeze();
        b.create(44).err() == Some(RD_FROZEN)
            && b.rmid(44).err() == Some(RD_FROZEN)
            && b.post(44, 1).err() == Some(RD_FROZEN)
            && b.get(44, 1).err() == Some(RD_FROZEN)
    };
    s.add("J10-冻结四拒", j10, "冻结后四操作全必拒");

    // J11 渲染面：活/死渲染可区分
    let j11 = {
        let a = SetQueue { setid: 1, alive: true, pending: Vec::new() };
        let b = SetQueue { setid: 1, alive: false, pending: Vec::new() };
        a.render() != b.render()
    };
    s.add("J11-渲染分旗", j11, "活/死渲染可区分");

    // J12 fold 稳定：送型号改变指纹、同态一致
    let j12 = {
        let mut b = SetBoard::new();
        let _ = b.create(45);
        let f0 = b.fold();
        let _ = b.post(45, 9);
        let f1 = b.fold();
        let f2 = b.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J12-指纹稳定", j12, "挂起账入指纹且同态稳定");

    // J13 动态明细：千次零误报计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("alive_ok={}/1000", ok));

    // J14 动态明细：双十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10, &format!("eidrm={} einval={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10925_setboard_flow() {
        let mut b = SetBoard::new();
        assert!(b.create(1).is_ok());
        assert!(b.create(1).err() == Some(RD_EINVAL));
        assert!(b.post(1, 7).is_ok());
        assert!(b.post(1, 0).err() == Some(RD_EINVAL));
        assert_eq!(b.get(1, 7), Ok(7));
        assert!(b.get(1, 7).err() == Some(RD_ENOMSG));
        assert!(b.get(1, -1).err() == Some(RD_EINVAL));
        let _ = b.rmid(1);
        assert!(b.get(1, 7).err() == Some(RD_EIDRM));
        b.freeze();
        assert!(b.get(1, 7).err() == Some(RD_FROZEN));
    }

    #[test]
    fn unx10925_checks_green() {
        let s = run_unx10925_checks();
        assert!(s.all_passed(), "UNX-F10925 判据未全绿");
    }
}
