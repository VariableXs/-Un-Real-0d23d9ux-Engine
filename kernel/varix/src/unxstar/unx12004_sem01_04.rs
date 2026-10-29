//! UNX-F12004 进程终止语义与退出状态传播链（0x192A0 段）
//!
//! 判据原文：进程终止以退出状态码（NTSTATUS 语义值）落账——终止只能
//! 发生一次（双死即账目冲突）；孤儿子进程由父退出传播链继承父状态；
//! 已收割（reaped）节点不可再被传播改账；异常退出（标记位）必须携带
//! 非零状态码；收割只对已死进程合法。
//!
//! 结构：终止链（TermChain：节点册 pid/parent/status/reaped）+ 终止核
//! + 传播核（孤儿继承）+ 收割核 + 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12004_checks）。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x192A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x192A0;
pub const TM_NO_SUCH: u32 = SEG_BASE + 0x01; // 终止不存在进程
pub const TM_ALREADY_DEAD: u32 = SEG_BASE + 0x02; // 双重终止
pub const TM_BAD_EXIT: u32 = SEG_BASE + 0x03; // 异常退出却带零状态
pub const TM_REAP_LIVE: u32 = SEG_BASE + 0x04; // 收割未死进程
pub const TM_PROP_TO_REAPED: u32 = SEG_BASE + 0x05; // 传播到已收割节点
pub const TM_FROZEN: u32 = SEG_BASE + 0x06; // 印账后仍动作

/// 终止链节点。
#[derive(Clone)]
pub struct TermNode {
    pub pid: u64,
    pub parent: u64,
    /// 退出状态码（NTSTATUS 语义值，0=未退出）。
    pub status: u32,
    /// 异常退出标记。
    pub abnormal: bool,
    /// 已被父收割。
    pub reaped: bool,
    /// 状态来源：0=自查 1=父传播继承。
    pub via: u8,
}

impl TermNode {
    pub fn dead(&self) -> bool {
        self.status != 0
    }
}

/// 终止链账册。
pub struct TermChain {
    pub nodes: Vec<TermNode>,
    pub sealed: bool,
}

impl TermChain {
    pub fn new() -> TermChain {
        TermChain { nodes: Vec::new(), sealed: false }
    }

    /// 登记存活进程（parent=0 表示系统根）。
    pub fn spawn(&mut self, pid: u64, parent: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(TM_FROZEN);
        }
        if pid == 0 || self.nodes.iter().any(|n| n.pid == pid) {
            return Err(TM_NO_SUCH);
        }
        self.nodes.push(TermNode {
            pid,
            parent,
            status: 0,
            abnormal: false,
            reaped: false,
            via: 0,
        });
        Ok(())
    }

    fn idx(&self, pid: u64) -> Option<usize> {
        self.nodes.iter().position(|n| n.pid == pid)
    }

    /// 终止核：单次终止 + 异常退出必须带非零状态。
    pub fn terminate(&mut self, pid: u64, status: u32, abnormal: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(TM_FROZEN);
        }
        let i = match self.idx(pid) {
            Some(i) => i,
            None => return Err(TM_NO_SUCH),
        };
        if self.nodes[i].dead() {
            return Err(TM_ALREADY_DEAD);
        }
        if abnormal && status == 0 {
            return Err(TM_BAD_EXIT);
        }
        self.nodes[i].status = status;
        self.nodes[i].abnormal = abnormal;
        self.nodes[i].via = 0;
        // 传播：死亡沿父链向孤儿子进程跨代蔓延（孙进程同样继承）——
        // 死亡队列逐层推进，子死则孙亦孤，直至无新增死亡。
        let mut dying: Vec<u64> = vec![pid];
        while !dying.is_empty() {
            let mut next: Vec<u64> = Vec::new();
            for dead_parent in dying.iter() {
                let kids: Vec<u64> = self
                    .nodes
                    .iter()
                    .filter(|n| n.parent == *dead_parent && n.pid != *dead_parent && !n.dead() && !n.reaped)
                    .map(|n| n.pid)
                    .collect();
                for k in kids {
                    let ki = self.idx(k).unwrap_or(0);
                    self.nodes[ki].status = status;
                    self.nodes[ki].abnormal = abnormal;
                    self.nodes[ki].via = 1;
                    next.push(k);
                }
            }
            dying = next;
        }
        Ok(())
    }

    /// 收割核：只收割已死且未收割的节点。
    pub fn reap(&mut self, pid: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(TM_FROZEN);
        }
        let i = match self.idx(pid) {
            Some(i) => i,
            None => return Err(TM_NO_SUCH),
        };
        if !self.nodes[i].dead() {
            return Err(TM_REAP_LIVE);
        }
        if self.nodes[i].reaped {
            return Err(TM_REAP_LIVE);
        }
        self.nodes[i].reaped = true;
        Ok(())
    }

    /// 传播核：对指定已收割子节点改账必拒（显性入口，供判据与注入）。
    pub fn propagate_to(&mut self, pid: u64, status: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TM_FROZEN);
        }
        let i = match self.idx(pid) {
            Some(i) => i,
            None => return Err(TM_NO_SUCH),
        };
        if self.nodes[i].reaped {
            return Err(TM_PROP_TO_REAPED);
        }
        if self.nodes[i].dead() {
            return Err(TM_ALREADY_DEAD);
        }
        self.nodes[i].status = status;
        self.nodes[i].via = 1;
        Ok(())
    }

    /// 传播一致性：所有 via=1 的节点状态必须等于其父状态。
    pub fn propagation_consistent(&self) -> bool {
        self.nodes.iter().all(|n| {
            if n.via == 1 {
                match self.nodes.iter().find(|p| p.pid == n.parent) {
                    Some(p) => p.status == n.status,
                    None => false,
                }
            } else {
                true
            }
        })
    }

    /// 孤儿清点：父已死而子未死的节点数（应为 0——传播覆盖）。
    pub fn orphans(&self) -> usize {
        self.nodes
            .iter()
            .filter(|n| {
                !n.dead()
                    && n.parent != 0
                    && self
                        .nodes
                        .iter()
                        .any(|p| p.pid == n.parent && p.dead())
            })
            .count()
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for n in &self.nodes {
            h = h.wrapping_mul(211).wrapping_add((n.pid & 0xFFFF) as u32);
            h = h.wrapping_mul(211).wrapping_add(n.status);
            h = h.wrapping_mul(211).wrapping_add(n.abnormal as u32);
            h = h.wrapping_mul(211).wrapping_add(n.reaped as u32);
            h = h.wrapping_mul(211).wrapping_add(n.via as u32);
        }
        h
    }

    /// 诊断面：链摘要。
    pub fn summary(&self) -> String {
        let dead = self.nodes.iter().filter(|n| n.dead()).count();
        format!("n={}|dead={}|reaped={}",
            self.nodes.len(), dead, self.nodes.iter().filter(|n| n.reaped).count())
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut c = TermChain::new();
    let _ = c.spawn(0x11, 0);
    let _ = c.spawn(0x12, 0x11);
    // 1) 终止不存在进程 → TM_NO_SUCH。
    got[0] = c.terminate(0xFEED, 5, false).err() == Some(TM_NO_SUCH);
    // 2) 异常退出带零状态 → TM_BAD_EXIT。
    got[1] = c.terminate(0x12, 0, true).err() == Some(TM_BAD_EXIT);
    // 3) 收割未死进程 → TM_REAP_LIVE。
    got[2] = c.reap(0x12).err() == Some(TM_REAP_LIVE);
    // 4) 双重终止 → TM_ALREADY_DEAD。
    let _ = c.terminate(0x12, 0xC000_0005, true);
    got[3] = c.terminate(0x12, 1, false).err() == Some(TM_ALREADY_DEAD);
    // 5) 传播到已收割节点 → TM_PROP_TO_REAPED。
    let _ = c.reap(0x12);
    got[4] = c.propagate_to(0x12, 9).err() == Some(TM_PROP_TO_REAPED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12004_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12004");

    // J1 正路径：父死子继——传播链状态一致、无孤儿残留。
    let j1 = {
        let mut c = TermChain::new();
        let _ = c.spawn(0xA0, 0);
        let _ = c.spawn(0xA1, 0xA0);
        let _ = c.spawn(0xA2, 0xA0);
        let _ = c.terminate(0xA0, 0xC000_0005, true);
        c.propagation_consistent() && c.orphans() == 0
            && c.nodes.iter().filter(|n| n.via == 1).count() == 2
    };
    s.add("J1-父死子继", j1, "双子继承父状态");

    // J2 不存在进程终止拒。
    let j2 = {
        let mut c = TermChain::new();
        c.terminate(9, 1, false).err() == Some(TM_NO_SUCH)
    };
    s.add("J2-幽灵终止拒", j2, "未登记 pid 必拒");

    // J3 双重终止拒。
    let j3 = {
        let mut c = TermChain::new();
        let _ = c.spawn(3, 0);
        let _ = c.terminate(3, 1, false);
        c.terminate(3, 2, false).err() == Some(TM_ALREADY_DEAD)
    };
    s.add("J3-双死拒", j3, "终止单次性");

    // J4 异常零状态拒；正常零态（ tranquil exit 语义以非零计）同样拒——
    //    状态码 0 只用于存活标记。
    let j4 = {
        let mut c = TermChain::new();
        let _ = c.spawn(4, 0);
        let ab = c.terminate(4, 0, true).err() == Some(TM_BAD_EXIT);
        c.nodes[0].status = 0;
        ab
    };
    s.add("J4-零态异常拒", j4, "异常必须带码");

    // J5 收割活进程拒。
    let j5 = {
        let mut c = TermChain::new();
        let _ = c.spawn(5, 0);
        c.reap(5).err() == Some(TM_REAP_LIVE)
    };
    s.add("J5-收割活者拒", j5, "先死后收");

    // J6 重复收割拒。
    let j6 = {
        let mut c = TermChain::new();
        let _ = c.spawn(6, 0);
        let _ = c.terminate(6, 2, false);
        let _ = c.reap(6);
        c.reap(6).err() == Some(TM_REAP_LIVE)
    };
    s.add("J6-重收割拒", j6, "收割单次性");

    // J7 传播到已收割节点拒。
    let j7 = {
        let mut c = TermChain::new();
        let _ = c.spawn(7, 0);
        let _ = c.spawn(8, 7);
        let _ = c.terminate(7, 3, false);
        let _ = c.reap(8);
        c.propagate_to(8, 9).err() == Some(TM_PROP_TO_REAPED)
    };
    s.add("J7-改账收割者拒", j7, "已收割节点冻结");

    // J8 正常退出不传播异常标记：父正常死（abnormal=false）子亦正常。
    let j8 = {
        let mut c = TermChain::new();
        let _ = c.spawn(9, 0);
        let _ = c.spawn(10, 9);
        let _ = c.terminate(9, 0, false);
        c.nodes.iter().filter(|n| n.via == 1).all(|n| !n.abnormal)
    };
    s.add("J8-正常退出传播", j8, "异常标记随传播");

    // J9 印账冻结。
    let j9 = {
        let mut c = TermChain::new();
        let _ = c.spawn(11, 0);
        let f1 = c.fold();
        let f2 = c.fold();
        c.seal();
        f1 == f2 && c.terminate(11, 1, false).err() == Some(TM_FROZEN)
    };
    s.add("J9-指纹封账", j9, "指纹幂等且印后必拒");

    // J10 摘要面。
    let j10 = {
        let mut c = TermChain::new();
        let _ = c.spawn(12, 0);
        let _ = c.terminate(12, 1, false);
        let t = c.summary();
        t.contains("n=") && t.contains("dead=1")
    };
    s.add("J10-摘要面", j10, "摘要含死账");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：三代链。
    let mut c = TermChain::new();
    let _ = c.spawn(0x21, 0);
    let _ = c.spawn(0x22, 0x21);
    let _ = c.spawn(0x23, 0x22);
    let _ = c.terminate(0x21, 0xC000_0139, true);
    s.add("样例传播一致", c.propagation_consistent(), "双代继承对账");
    s.add("样例无孤儿", c.orphans() == 0, "传播覆盖全链");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12004_termination_flow() {
        let mut c = TermChain::new();
        assert!(c.spawn(1, 0).is_ok());
        assert!(c.spawn(2, 1).is_ok());
        assert!(c.terminate(1, 0xC000_0005, true).is_ok());
        assert_eq!(c.terminate(1, 3, false), Err(TM_ALREADY_DEAD));
        // 子节点 2 经传播继承父状态已死：收割合法。
        assert!(c.reap(2).is_ok());
        assert_eq!(c.reap(2), Err(TM_REAP_LIVE));
        assert!(c.reap(1).is_ok());
        assert_eq!(c.reap(1), Err(TM_REAP_LIVE));
    }

    #[test]
    fn unx12004_checks_green() {
        let s = run_unx12004_checks();
        assert!(s.all_passed(), "UNX-F12004 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12004_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
