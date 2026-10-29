//! UNX-F10926 msgctl 错误矩阵（RMID/STAT/SET 面）（0x1d340 段）
//!
//! 锚点：UNX-F10926-J1 正面：EINVAL/EPERM/EACCES 三错误各 10/10；
//! 反面：合法命令零误报（千次 10/10）。
//! 域：CoRun·UNX-B27（SysV 错误矩阵域·msgctl 面）。
//!
//! 结构：控制账（RMID/STAT/SET 三命令 + 属主位 + 读权限位）+ 裁断器
//! （坏命令/坏 qid→EINVAL、非属主 RMID/SET→EPERM、无读权 STAT→
//! EACCES）+ 注入核（坏命令/越主/越权/冻结改账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d340 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d340;
pub const CT_EINVAL: u32 = SEG_BASE + 0x01; // 坏命令/坏 qid
pub const CT_EPERM: u32 = SEG_BASE + 0x02; // 非属主
pub const CT_EACCES: u32 = SEG_BASE + 0x03; // 无读权
pub const CT_EIDRM: u32 = SEG_BASE + 0x04; // 已删
pub const CT_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后改账
pub const CT_ENOENT: u32 = SEG_BASE + 0x06; // 队列缺失

/// 控制账 fold 域标（种子）。
pub const CT_TAG: u64 = 0x1d34_26;

/// 命令码。
pub const CMD_RMID: u8 = 1;
pub const CMD_STAT: u8 = 2;
pub const CMD_SET: u8 = 3;

/// 控制账条目。
#[derive(Clone, Debug)]
pub struct CtlQueue {
    pub qid: u32,
    pub owner: u32,
    pub readable: bool,
    pub alive: bool,
    pub mode_val: u32,
}

impl CtlQueue {
    pub fn render(&self) -> String {
        let a = if self.alive { "A" } else { "D" };
        format!("C{}|{}|{}", self.qid, a, self.mode_val)
    }
}

/// msgctl 裁断账。
pub struct CtlBoard {
    pub queues: Vec<CtlQueue>,
    pub frozen: bool,
}

impl CtlBoard {
    pub fn new() -> CtlBoard {
        CtlBoard { queues: Vec::new(), frozen: false }
    }

    /// 建队列（属主 + 读权位）。
    pub fn create(&mut self, qid: u32, owner: u32, readable: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(CT_FROZEN);
        }
        if self.queues.iter().any(|q| q.qid == qid) {
            return Err(CT_EINVAL);
        }
        if self.queues.len() >= 24 {
            return Err(CT_EINVAL);
        }
        self.queues.push(CtlQueue { qid, owner, readable, alive: true, mode_val: 0o644 });
        Ok(())
    }

    /// 查下标。
    fn find(&self, qid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.queues.len() {
            if self.queues[k].qid == qid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// msgctl 主裁断。
    pub fn ctl(&mut self, qid: u32, cmd: u8, caller: u32, new_val: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(CT_FROZEN);
        }
        if cmd != CMD_RMID && cmd != CMD_STAT && cmd != CMD_SET {
            return Err(CT_EINVAL);
        }
        let idx = match self.find(qid) {
            Some(i) => i,
            None => return Err(CT_EINVAL),
        };
        if !self.queues[idx].alive {
            return Err(CT_EIDRM);
        }
        if cmd == CMD_STAT {
            if !self.queues[idx].readable {
                return Err(CT_EACCES);
            }
            return Ok(self.queues[idx].mode_val);
        }
        // RMID/SET 需属主。
        if self.queues[idx].owner != caller {
            return Err(CT_EPERM);
        }
        if cmd == CMD_RMID {
            self.queues[idx].alive = false;
            Ok(0)
        } else {
            if new_val == 0 {
                return Err(CT_EINVAL);
            }
            self.queues[idx].mode_val = new_val;
            Ok(new_val)
        }
    }

    /// 存活计数。
    pub fn alive_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.queues.len() {
            if self.queues[k].alive {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 链式 fold 指纹（序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h = CT_TAG;
        let mut k = 0usize;
        while k < self.queues.len() {
            let q = &self.queues[k];
            h = (h ^ q.qid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.owner as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.mode_val as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.alive as u64).wrapping_mul(0x100000001b3);
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
    // 1) 坏命令必报 EINVAL
    let mut b = CtlBoard::new();
    let _ = b.create(1, 100, true);
    got[0] = b.ctl(1, 9, 100, 0).err() == Some(CT_EINVAL);
    // 2) 非属主 RMID 必报 EPERM
    let mut p = CtlBoard::new();
    let _ = p.create(2, 100, true);
    got[1] = p.ctl(2, CMD_RMID, 200, 0).err() == Some(CT_EPERM);
    // 3) 无读权 STAT 必报 EACCES
    let mut a = CtlBoard::new();
    let _ = a.create(3, 100, false);
    got[2] = a.ctl(3, CMD_STAT, 100, 0).err() == Some(CT_EACCES);
    // 4) 冻结后命令必抓
    let mut f = CtlBoard::new();
    f.freeze();
    got[3] = f.ctl(1, CMD_STAT, 100, 0).err() == Some(CT_FROZEN);
    got
}

/// 调用者确定性样本。
pub fn ct_sample_caller(r: u32) -> u32 {
    r.wrapping_mul(2654435761).wrapping_add(59) % 500 + 1
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10926_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10926");

    // J1 正面·EINVAL 十连（坏命令与坏 qid 混合）
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut b = CtlBoard::new();
        let _ = b.create(r + 1, ct_sample_caller(r), true);
        let bad = if r % 2 == 0 {
            b.ctl(r + 1, 9, ct_sample_caller(r), 0).err() == Some(CT_EINVAL)
        } else {
            b.ctl(9999, CMD_STAT, ct_sample_caller(r), 0).err() == Some(CT_EINVAL)
        };
        if bad {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-EINVAL十连", h1 == 10, "坏命令/坏 qid 十轮全数 EINVAL");

    // J2 正面·EPERM 十连
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut b = CtlBoard::new();
        let _ = b.create(r2 + 20, 1, true);
        if b.ctl(r2 + 20, CMD_RMID, 2, 0).err() == Some(CT_EPERM) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-EPERM十连", h2 == 10, "非属主 RMID 十轮全数 EPERM");

    // J3 正面·EACCES 十连
    let mut h3 = 0u32;
    let mut r3 = 0u32;
    while r3 < 10 {
        let mut b = CtlBoard::new();
        let _ = b.create(r3 + 40, 1, false);
        if b.ctl(r3 + 40, CMD_STAT, 1, 0).err() == Some(CT_EACCES) {
            h3 += 1;
        }
        r3 += 1;
    }
    s.add("J3-EACCES十连", h3 == 10, "无读权 STAT 十轮全数 EACCES");

    // J4 反面：合法命令千次零误报
    let mut ok = 0u32;
    let mut r4 = 0u32;
    while r4 < 1000 {
        let mut b = CtlBoard::new();
        let qid = (r4 % 5) as u32 + 1;
        let _ = b.create(qid, 1, true);
        let v = b.ctl(qid, CMD_SET, 1, ct_sample_caller(r4));
        let g = b.ctl(qid, CMD_STAT, 1, 0);
        if v.is_ok() && g.is_ok() {
            ok += 1;
        }
        r4 += 1;
    }
    s.add("J4-合法千次零误报", ok == 1000, "SET/STAT 千轮零误报");

    // J5 RMID 后 STAT 报 EIDRM
    let j5 = {
        let mut b = CtlBoard::new();
        let _ = b.create(50, 1, true);
        let _ = b.ctl(50, CMD_RMID, 1, 0);
        b.ctl(50, CMD_STAT, 1, 0).err() == Some(CT_EIDRM)
    };
    s.add("J5-RMID后EIDRM", j5, "删后 STAT 报 EIDRM");

    // J6 SET 回读一致
    let j6 = {
        let mut b = CtlBoard::new();
        let _ = b.create(51, 1, true);
        let _ = b.ctl(51, CMD_SET, 1, 0o600);
        b.ctl(51, CMD_STAT, 1, 0) == Ok(0o600)
    };
    s.add("J6-SET回读一致", j6, "SET 后 STAT 回读一致");

    // J7 SET 零值拒
    let j7 = {
        let mut b = CtlBoard::new();
        let _ = b.create(52, 1, true);
        b.ctl(52, CMD_SET, 1, 0).err() == Some(CT_EINVAL)
    };
    s.add("J7-SET零值拒", j7, "SET 零值必拒");

    // J8 注入四路全抓
    let inj = inject_four();
    s.add("J8-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "坏命令/越主/越权/冻结全数命中");

    // J9 存活计数随 RMID 递减
    let j9 = {
        let mut b = CtlBoard::new();
        let _ = b.create(53, 1, true);
        let _ = b.create(54, 1, true);
        let a1 = b.alive_count();
        let _ = b.ctl(53, CMD_RMID, 1, 0);
        a1 == 2 && b.alive_count() == 1
    };
    s.add("J9-存活计数", j9, "RMID 后存活计数减一");

    // J10 冻结后三命令全拒
    let j10 = {
        let mut b = CtlBoard::new();
        b.freeze();
        b.ctl(1, CMD_RMID, 1, 0).err() == Some(CT_FROZEN)
            && b.ctl(1, CMD_STAT, 1, 0).err() == Some(CT_FROZEN)
            && b.ctl(1, CMD_SET, 1, 1).err() == Some(CT_FROZEN)
    };
    s.add("J10-冻结三拒", j10, "冻结后三命令全必拒");

    // J11 渲染面：活/死渲染可区分
    let j11 = {
        let a = CtlQueue { qid: 1, owner: 1, readable: true, alive: true, mode_val: 0o644 };
        let mut b = a.clone();
        b.alive = false;
        a.render() != b.render()
    };
    s.add("J11-渲染分旗", j11, "活/死渲染可区分");

    // J12 fold 稳定：RMID 改变指纹、同态一致
    let j12 = {
        let mut b = CtlBoard::new();
        let _ = b.create(55, 1, true);
        let f0 = b.fold();
        let _ = b.ctl(55, CMD_RMID, 1, 0);
        let f1 = b.fold();
        let f2 = b.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J12-指纹稳定", j12, "删除入指纹且同态稳定");

    // J13 动态明细：千次零误报计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J14 动态明细：三十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10 && h3 == 10, &format!("einval={} eperm={} eacces={}", h1, h2, h3));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10926_ctlboard_flow() {
        let mut b = CtlBoard::new();
        assert!(b.create(1, 100, true).is_ok());
        assert!(b.create(1, 100, true).err() == Some(CT_EINVAL));
        assert!(b.ctl(1, 9, 100, 0).err() == Some(CT_EINVAL));
        assert!(b.ctl(99, CMD_STAT, 100, 0).err() == Some(CT_EINVAL));
        assert_eq!(b.ctl(1, CMD_SET, 100, 0o600), Ok(0o600));
        assert_eq!(b.ctl(1, CMD_STAT, 100, 0), Ok(0o600));
        assert!(b.ctl(1, CMD_SET, 200, 0o600).err() == Some(CT_EPERM));
        assert_eq!(b.ctl(1, CMD_RMID, 100, 0), Ok(0));
        assert!(b.ctl(1, CMD_STAT, 100, 0).err() == Some(CT_EIDRM));
        b.freeze();
        assert!(b.ctl(1, CMD_STAT, 100, 0).err() == Some(CT_FROZEN));
    }

    #[test]
    fn unx10926_checks_green() {
        let s = run_unx10926_checks();
        assert!(s.all_passed(), "UNX-F10926 判据未全绿");
    }
}
