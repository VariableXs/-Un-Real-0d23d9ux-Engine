//! UNX-F10914 注册上限/ENOMEM 矩阵（0x1d1c0 段）
//!
//! 锚点：UNX-F10914-J1 正面：兴趣表满→ENOMEM 显式（10/10）；反面：
//! 余量内零误报（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·注册上限面）。
//!
//! 结构：兴趣表（定容 + 内存页账双上限）+ 裁断器（表满→ENOMEM、页
//! 不足→ENOMEM、余量内全通）+ 注入核（页耗尽/表满后摘除复用/重号/
//! 冻结改表四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d1c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d1c0;
pub const NM_ENOMEM: u32 = SEG_BASE + 0x01; // 上限耗尽
pub const NM_EBADF: u32 = SEG_BASE + 0x02; // fd 未登记
pub const NM_DUP: u32 = SEG_BASE + 0x03; // 重号
pub const NM_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改表
pub const NM_NOPAGE: u32 = SEG_BASE + 0x05; // 页账耗尽
pub const NM_EINVAL: u32 = SEG_BASE + 0x06; // 事件掩码零

/// 兴趣表条数上限。
pub const TABLE_CAP: usize = 24;
/// 内存页账初始余量。
pub const PAGE_BUDGET: u32 = 96;
/// 每条目页耗。
pub const PAGE_PER_ENTRY: u32 = 4;
/// NM 域标（fold 种子）。
pub const NM_TAG: u64 = 0x1d1c20e;

/// 兴趣表条目。
#[derive(Clone, Debug)]
pub struct InterestSlot {
    pub fd: i32,
    pub events: u32,
}

impl InterestSlot {
    pub fn armed(&self) -> bool {
        self.events != 0
    }
}

/// 注册上限账。
pub struct LimitTable {
    pub slots: Vec<InterestSlot>,
    pub pages: u32,
    pub frozen: bool,
}

impl LimitTable {
    pub fn new() -> LimitTable {
        LimitTable { slots: Vec::new(), pages: PAGE_BUDGET, frozen: false }
    }

    /// 注册：冻结 → 重号 → 掩码零 → 表满 ENOMEM → 页耗 ENOMEM。
    pub fn add(&mut self, fd: i32, events: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(NM_FROZEN);
        }
        if self.slots.iter().any(|e| e.fd == fd) {
            return Err(NM_DUP);
        }
        if events == 0 {
            return Err(NM_EINVAL);
        }
        if self.slots.len() >= TABLE_CAP {
            return Err(NM_ENOMEM);
        }
        if self.pages < PAGE_PER_ENTRY {
            return Err(NM_NOPAGE);
        }
        self.pages -= PAGE_PER_ENTRY;
        self.slots.push(InterestSlot { fd, events });
        Ok(())
    }

    /// 摘除：释放表位与页账。
    pub fn del(&mut self, fd: i32) -> Result<(), u32> {
        if self.frozen {
            return Err(NM_FROZEN);
        }
        let mut idx = self.slots.len();
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                idx = k;
            }
            k += 1;
        }
        if idx == self.slots.len() {
            return Err(NM_EBADF);
        }
        self.slots.remove(idx);
        self.pages = self.pages.wrapping_add(PAGE_PER_ENTRY);
        Ok(())
    }

    /// 余量：表余位与页余量取小。
    pub fn headroom(&self) -> u32 {
        let slot_room = (TABLE_CAP - self.slots.len()) as u32;
        let page_room = self.pages / PAGE_PER_ENTRY;
        if slot_room < page_room {
            slot_room
        } else {
            page_room
        }
    }

    /// armed 条目数。
    pub fn armed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].armed() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 链式 fold 指纹（序敏感，含页账）。
    pub fn fold(&self) -> u64 {
        let mut h = NM_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.events as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.pages as u64).wrapping_mul(0x100000001b3);
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
    // 1) 表满后注册必报 ENOMEM
    let mut t = LimitTable::new();
    let mut f = 1i32;
    while f <= TABLE_CAP as i32 {
        let _ = t.add(f, 0x1);
        f += 1;
    }
    got[0] = t.add(9999, 0x1).err() == Some(NM_ENOMEM);
    // 2) 页账耗尽必报 ENOMEM（表未满）
    let mut p = LimitTable::new();
    p.pages = PAGE_PER_ENTRY - 1;
    got[1] = p.add(1, 0x1).err() == Some(NM_NOPAGE);
    // 3) 表满后摘除一 dispon 复用余量
    let _ = t.del(1);
    got[2] = t.add(8888, 0x1).is_ok();
    // 4) 冻结后注册必抓
    let mut fz = LimitTable::new();
    fz.freeze();
    got[3] = fz.add(1, 0x1).err() == Some(NM_FROZEN);
    got
}

/// 事件掩码确定性样本。
pub fn nm_sample_events(r: u32) -> u32 {
    let m = r.wrapping_mul(2654435761) % 3;
    (1u32).wrapping_shl(m + 1)
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10914_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10914");

    // J1 正面：兴趣表满→ENOMEM 显式十连
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut t = LimitTable::new();
        let mut f = 1i32;
        while f <= TABLE_CAP as i32 {
            let _ = t.add(f, 0x1);
            f += 1;
        }
        if t.add(5000 + r as i32, 0x1).err() == Some(NM_ENOMEM) {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-表满ENOMEM十连", h1 == 10, "表满注册十轮全数显式 ENOMEM");

    // J2 页账耗尽→ENOMEM 十连
    // 单条页耗 PAGE_PER_ENTRY=4：余量须落在 [0,4) 才算不足，取 r % PAGE_PER_ENTRY。
    let mut h2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut t = LimitTable::new();
        t.pages = r2 % PAGE_PER_ENTRY;
        if t.add(1, 0x1).err() == Some(NM_NOPAGE) {
            h2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-页耗ENOMEM十连", h2 == 10, "页不足十轮全数 ENOMEM");

    // J3 反面：余量内零误报（千次小批注册）
    let mut ok = 0u32;
    let mut r3 = 0u32;
    while r3 < 1000 {
        let mut t = LimitTable::new();
        let m = ((r3 % 3) + 1) as i32;
        let mut f = 1i32;
        let mut pass = true;
        while f <= m {
            if t.add(f, nm_sample_events(r3 as u32 + f as u32)).is_err() {
                pass = false;
            }
            f += 1;
        }
        if pass {
            ok += 1;
        }
        r3 += 1;
    }
    s.add("J3-余量内千次零误报", ok == 1000, "小批注册千轮零误报");

    // J4 摘除释放页账：注册-摘除-再注册页账复原
    let j4 = {
        let mut t = LimitTable::new();
        let _ = t.add(1, 0x1);
        let p1 = t.pages;
        let _ = t.del(1);
        t.pages == PAGE_BUDGET && p1 == PAGE_BUDGET - PAGE_PER_ENTRY
    };
    s.add("J4-摘除释放页账", j4, "摘除后页账复原");

    // J5 余量账与手算一致
    let j5 = {
        let mut t = LimitTable::new();
        let mut f = 1i32;
        while f <= 5 {
            let _ = t.add(f, 0x1);
            f += 1;
        }
        t.headroom() == (TABLE_CAP as u32 - 5).min((PAGE_BUDGET - 5 * PAGE_PER_ENTRY) / PAGE_PER_ENTRY)
    };
    s.add("J5-余量对账", j5, "headroom 与双上限手算一致");

    // J6 注入四路全抓
    let inj = inject_four();
    s.add("J6-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "表满/页耗/复用/冻结全数命中");

    // J7 重号拒收
    let j7 = {
        let mut t = LimitTable::new();
        let _ = t.add(7, 0x1);
        t.add(7, 0x2).err() == Some(NM_DUP)
    };
    s.add("J7-重号拒收", j7, "同 fd 重复注册必拒");

    // J8 掩码零拒收
    let j8 = LimitTable::new().add(8, 0).err() == Some(NM_EINVAL);
    s.add("J8-掩码零拒", j8, "零掩码注册必拒");

    // J9 摘除不存在的 fd 拒
    let j9 = LimitTable::new().del(9).err() == Some(NM_EBADF);
    s.add("J9-摘除不存在拒", j9, "摘除未注册 fd 必拒");

    // J10 冻结后注册/摘除双拒
    let j10 = {
        let mut t = LimitTable::new();
        t.freeze();
        t.add(10, 0x1).err() == Some(NM_FROZEN) && t.del(10).err() == Some(NM_FROZEN)
    };
    s.add("J10-冻结双拒", j10, "冻结后注册与摘除均必拒");

    // J11 armed 计数随掩码变化
    let j11 = {
        let mut t = LimitTable::new();
        let _ = t.add(11, 0x1);
        let a1 = t.armed_count();
        let _ = t.add(12, 0x2);
        let a2 = t.armed_count();
        a1 == 1 && a2 == 2
    };
    s.add("J11-armed计数", j11, "armed 计数逐条累积");

    // J12 fold 稳定：注册/摘除改变指纹、同态一致
    let j12 = {
        let mut t = LimitTable::new();
        let _ = t.add(13, 0x1);
        let f0 = t.fold();
        let _ = t.del(13);
        let f1 = t.fold();
        let f2 = t.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J12-指纹稳定", j12, "同态指纹稳定且异态指纹必变");

    // J13 动态明细：余量内千次计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J14 动态明细：双十连计数落账
    s.add_dyn("J14-十连明细", h1 == 10 && h2 == 10, &format!("table={} page={}", h1, h2));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10914_limittable_flow() {
        let mut t = LimitTable::new();
        assert!(t.add(1, 0x1).is_ok());
        assert!(t.add(1, 0x1).err() == Some(NM_DUP));
        assert!(t.add(2, 0).err() == Some(NM_EINVAL));
        assert_eq!(t.pages, PAGE_BUDGET - PAGE_PER_ENTRY);
        assert!(t.del(1).is_ok());
        assert_eq!(t.pages, PAGE_BUDGET);
        assert!(t.del(1).err() == Some(NM_EBADF));
        let mut f = 1i32;
        while f <= TABLE_CAP as i32 {
            let _ = t.add(f, 0x1);
            f += 1;
        }
        assert!(t.add(999, 0x1).err() == Some(NM_ENOMEM));
        t.freeze();
        assert!(t.add(1000, 0x1).err() == Some(NM_FROZEN));
    }

    #[test]
    fn unx10914_checks_green() {
        let s = run_unx10914_checks();
        assert!(s.all_passed(), "UNX-F10914 判据未全绿");
    }
}
