//! UNX-F10927 msgget 表满 ENOSPC 矩阵（0x1d360 段）
//!
//! 锚点：UNX-F10927-J1 正面：系统级队列数满→ENOSPC（10/10）；反面：
//! 余量内零误报（千次 10/10）。
//! 域：CoRun·UNX-B27（SysV 错误矩阵域·msgget 表满面）。
//!
//! 结构：系统级队列表（MSGMNI 定容全局账）+ 裁断器（表满→ENOSPC、
//! 删后腾位可复用、余量内全通）+ 注入核（满表/满表删一复用/重号/
//! 冻结改表四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d360 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d360;
pub const SP_ENOSPC: u32 = SEG_BASE + 0x01; // 系统级表满
pub const SP_EINVAL: u32 = SEG_BASE + 0x02; // key/flag 非法
pub const SP_EEXIST: u32 = SEG_BASE + 0x03; // 重号
pub const SP_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改表
pub const SP_ENOENT: u32 = SEG_BASE + 0x05; // 键缺失
pub const SP_DRAIN: u32 = SEG_BASE + 0x06; // 腾位失败

/// MSGMNI 上限。
pub const MSGMNI: usize = 12;
/// SP 域标（fold 种子）。
pub const SP_TAG: u64 = 0x1d36220;

/// 系统队列表条目。
#[derive(Clone, Debug)]
pub struct SysQueue {
    pub key: u32,
    pub qid: u32,
    pub alive: bool,
}

impl SysQueue {
    pub fn render(&self) -> String {
        let a = if self.alive { "A" } else { "D" };
        format!("SQ{}|{}|{}", self.key, a, self.qid)
    }
}

/// msgget 表满裁断账。
pub struct SysTable {
    pub slots: Vec<SysQueue>,
    pub frozen: bool,
    pub next_qid: u32,
}

impl SysTable {
    pub fn new() -> SysTable {
        SysTable { slots: Vec::new(), frozen: false, next_qid: 1 }
    }

    /// 存活队列数。
    pub fn alive_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].alive {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 建队列（表满→ENOSPC）。
    pub fn create(&mut self, key: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        if key == 0 {
            return Err(SP_EINVAL);
        }
        if self.slots.iter().any(|q| q.key == key && q.alive) {
            return Err(SP_EEXIST);
        }
        if self.alive_count() >= MSGMNI {
            return Err(SP_ENOSPC);
        }
        // 死槽复用：先找同 key 死槽，否则尾插。
        let mut reuse = self.slots.len();
        let mut k = 0usize;
        while k < self.slots.len() {
            if !self.slots[k].alive && self.slots[k].key == key {
                reuse = k;
                break;
            }
            k += 1;
        }
        let qid = self.next_qid;
        self.next_qid += 1;
        if reuse < self.slots.len() {
            self.slots[reuse].qid = qid;
            self.slots[reuse].alive = true;
        } else {
            self.slots.push(SysQueue { key, qid, alive: true });
        }
        Ok(qid)
    }

    /// 查（无 IPC_CREAT）。
    pub fn open(&self, key: u32) -> Result<u32, u32> {
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].key == key && self.slots[k].alive {
                return Ok(self.slots[k].qid);
            }
            k += 1;
        }
        Err(SP_ENOENT)
    }

    /// 删（腾位；冻结后改表一律拒）。
    pub fn rmid(&mut self, qid: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        let mut idx = self.slots.len();
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].qid == qid && self.slots[k].alive {
                idx = k;
            }
            k += 1;
        }
        if idx == self.slots.len() {
            return Err(SP_DRAIN);
        }
        self.slots[idx].alive = false;
        Ok(())
    }

    /// 链式 fold 指纹（序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h = SP_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let q = &self.slots[k];
            h = (h ^ q.key as u64).wrapping_mul(0x100000001b3);
            h = (h ^ q.qid as u64).wrapping_mul(0x100000001b3);
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
    // 1) 表满必报 ENOSPC
    let mut t = SysTable::new();
    let mut k = 1u32;
    while k <= MSGMNI as u32 {
        let _ = t.create(k * 10);
        k += 1;
    }
    got[0] = t.create(9999).err() == Some(SP_ENOSPC);
    // 2) 满表删一后可复用
    let _ = t.rmid(1);
    got[1] = t.create(9998).is_ok();
    // 3) 零键必报 EINVAL
    let mut z = SysTable::new();
    got[2] = z.create(0).err() == Some(SP_EINVAL);
    // 4) 冻结后建队必抓
    let mut f = SysTable::new();
    f.freeze();
    got[3] = f.create(5).err() == Some(SP_FROZEN);
    got
}

/// key 确定性样本。
pub fn sp_sample_key(r: u32) -> u32 {
    r.wrapping_mul(2654435761).wrapping_add(61) % 500 + 1
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10927_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10927");

    // J1 正面：表满→ENOSPC 十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut t = SysTable::new();
        let mut k = 1u32;
        while k <= MSGMNI as u32 {
            let _ = t.create(k * 10 + r);
            k += 1;
        }
        if t.create(5000 + r).err() == Some(SP_ENOSPC) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-表满ENOSPC十连", h1 == 10, "MSGMNI 满十轮全数 ENOSPC");

    // J2 满表删一复用
    let mut ok = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut t = SysTable::new();
        let mut k = 1u32;
        while k <= MSGMNI as u32 {
            let _ = t.create(k * 10);
            k += 1;
        }
        let _ = t.rmid(r2 + 1);
        if t.create(6000 + r2).is_ok() && t.alive_count() == MSGMNI {
            ok += 1;
        }
        r2 += 1;
    }
    s.add("J2-删一复用十连", ok == 10, "删一腾位后建队复活且账满");

    // J3 反面：余量内千次零误报
    let mut ok3 = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut t = SysTable::new();
        let m = (r3 % (MSGMNI as u32 - 1)) + 1;
        let mut k = 0u32;
        let mut pass = true;
        while k < m {
            if t.create(sp_sample_key(r3 + k)).is_err() {
                pass = false;
            }
            k += 1;
        }
        if pass {
            ok3 += 1;
        }
        r3 += 1;
    }
    s.add("J3-余量内千次零误报", ok3 == 1000, "余量内千轮建队零误报");

    // J4 查回读：建后即开同 qid
    let j4 = {
        let mut t = SysTable::new();
        let q = t.create(100);
        t.open(100) == q
    };
    s.add("J4-建开同号", j4, "建后开同 qid");

    // J5 注入四路全抓
    let inj = inject_four();
    s.add("J5-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "满表/复用/零键/冻结全数命中");

    // J6 重号建拒
    let j6 = {
        let mut t = SysTable::new();
        let _ = t.create(200);
        t.create(200).err() == Some(SP_EEXIST)
    };
    s.add("J6-重号拒收", j6, "同 key 重复建必拒");

    // J7 死键复活（rmid 后同 key 再建）
    let j7 = {
        let mut t = SysTable::new();
        let _ = t.create(300);
        let _ = t.rmid(t.open(300).unwrap_or(0));
        t.create(300).is_ok()
    };
    s.add("J7-死键复活", j7, "删后同键可重建");

    // J8 删不存在的 qid 报 DRAIN
    let j8 = SysTable::new().rmid(999).err() == Some(SP_DRAIN);
    s.add("J8-删虚拒", j8, "删不存在 qid 必拒");

    // J9 冻结后建/删双拒
    let j9 = {
        let mut t = SysTable::new();
        t.freeze();
        t.create(400).err() == Some(SP_FROZEN) && t.rmid(1).err() == Some(SP_FROZEN)
    };
    s.add("J9-冻结双拒", j9, "冻结后建与删均必拒");

    // J10 qid 单调分配
    let j10 = {
        let mut t = SysTable::new();
        let a = t.create(500);
        let b = t.create(501);
        a == Ok(1) && b == Ok(2)
    };
    s.add("J10-qid单调", j10, "qid 按建序单调");

    // J11 渲染面：活/死渲染可区分
    let j11 = {
        let a = SysQueue { key: 1, qid: 1, alive: true };
        let mut b = a.clone();
        b.alive = false;
        a.render() != b.render()
    };
    s.add("J11-渲染分旗", j11, "活/死渲染可区分");

    // J12 fold 稳定：建队改变指纹、同态一致
    let j12 = {
        let mut t = SysTable::new();
        let f0 = t.fold();
        let _ = t.create(600);
        let f1 = t.fold();
        let f2 = t.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J12-指纹稳定", j12, "建队入指纹且同态稳定");

    // J13 动态明细：余量千次计数落账
    s.add_dyn("J13-千次明细", ok3 == 1000, &format!("legal_ok={}/1000", ok3));

    // J14 动态明细：ENOSPC 十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && ok == 10, &format!("enospc={} reuse={}", h1, ok));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10927_systable_flow() {
        let mut t = SysTable::new();
        assert_eq!(t.create(10), Ok(1));
        assert!(t.create(10).err() == Some(SP_EEXIST));
        assert_eq!(t.open(10), Ok(1));
        assert!(t.open(11).err() == Some(SP_ENOENT));
        assert!(t.create(0).err() == Some(SP_EINVAL));
        let mut k = 2u32;
        while k <= MSGMNI as u32 {
            let _ = t.create(k * 10);
            k += 1;
        }
        assert!(t.create(999).err() == Some(SP_ENOSPC));
        let _ = t.rmid(1);
        assert!(t.create(999).is_ok());
        t.freeze();
        assert!(t.create(998).err() == Some(SP_FROZEN));
    }

    #[test]
    fn unx10927_checks_green() {
        let s = run_unx10927_checks();
        assert!(s.all_passed(), "UNX-F10927 判据未全绿");
    }
}
