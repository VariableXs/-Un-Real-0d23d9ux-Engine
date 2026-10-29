//! UNX-F11008 epoll ADD 挂号账联签（兴趣列表注册项对账）（0x1e100 段）
//!
//! 锚点：UNX-F11008-J1 ADD 注册千次账对平（10/10），兴趣列表一致性
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：兴趣列表（epoll ADD 逐项挂号：fd + 事件掩码）+ 千次注册账（每笔
//! 落账并计数）+ 一致性对账器（在册项 = 计数 = 可枚举项，掩码非零）+ fold
//! 指纹与冻结 + 注入核（零掩码/重号/越界 fd/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e100 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e100;
pub const EA_ZEROMASK: u32 = SEG_BASE + 0x01; // 事件掩码为零
pub const EA_DUPFD: u32 = SEG_BASE + 0x02; // fd 重号
pub const EA_NOFD: u32 = SEG_BASE + 0x03; // 越界 fd
pub const EA_COUNT: u32 = SEG_BASE + 0x04; // 计数账漂移
pub const EA_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const EA_CAP: u32 = SEG_BASE + 0x06; // 兴趣表超容

/// 千次 ADD 注册轮数。
pub const ROUNDS: u32 = 1000;
/// 兴趣表定容。
pub const LIST_CAP: usize = 64;
/// 事件掩码合法位集（IN/OUT/PRI/ERR/HUP/RDHUP）。
pub const MASK_LEGAL: u32 = 0x0000_003f;
/// 兴趣表域标（fold 种子）。
pub const EA_TAG: u64 = 0x1e10_08;

/// 单条兴趣项：fd + 事件掩码 + 落账序。
#[derive(Clone, Debug)]
pub struct InterestItem {
    pub fd: u32,
    pub mask: u32,
    pub seq: u32,
}

impl InterestItem {
    /// 掩码必须落在合法位集内。
    pub fn mask_ok(&self) -> bool {
        self.mask != 0 && (self.mask & !MASK_LEGAL) == 0
    }

    pub fn render(&self) -> String {
        format!("fd{}|m{:#x}|s{}", self.fd, self.mask, self.seq)
    }
}

/// epoll ADD 兴趣列表：逐项挂号 → 计数对账 → 冻结。
pub struct InterestList {
    pub items: Vec<InterestItem>,
    pub add_count: u32,
    /// 本表起算计数（重开后带过去的累计基），用于在册数对账。
    pub table_base: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl InterestList {
    pub fn new() -> InterestList {
        InterestList { items: Vec::new(), add_count: 0, table_base: 0, frozen: false, root: None }
    }

    /// ADD 挂号：冻结、零掩码、非法掩码位、fd 重号、超容均拒。
    pub fn add(&mut self, fd: u32, mask: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(EA_FROZEN);
        }
        if mask == 0 || (mask & !MASK_LEGAL) != 0 {
            return Err(EA_ZEROMASK);
        }
        if fd == 0 {
            return Err(EA_NOFD);
        }
        if self.items.iter().any(|i| i.fd == fd) {
            return Err(EA_DUPFD);
        }
        if self.items.len() >= LIST_CAP {
            return Err(EA_CAP);
        }
        self.add_count = self.add_count.wrapping_add(1);
        self.items.push(InterestItem { fd, mask, seq: self.add_count });
        Ok(())
    }

    /// 表满重开：清空在册项，累计计数与序号带过去作新表起算基。
    pub fn reopen(&mut self) {
        self.items.clear();
        self.table_base = self.add_count;
    }

    /// fd 是否在册。
    pub fn contains(&self, fd: u32) -> bool {
        self.items.iter().any(|i| i.fd == fd)
    }

    /// 按 fd 查项副本。
    pub fn get(&self, fd: u32) -> Option<InterestItem> {
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].fd == fd {
                return Some(self.items[k].clone());
            }
            k += 1;
        }
        None
    }

    /// 掩码位违反项清单（一致性探针）。
    pub fn bad_mask(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].mask_ok() {
                g.push(self.items[k].fd);
            }
            k += 1;
        }
        g
    }

    /// 落账序严格递增（千次账序对账）。
    pub fn seq_monotonic(&self) -> bool {
        let mut k = 1usize;
        while k < self.items.len() {
            if self.items[k].seq <= self.items[k - 1].seq {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 一致性裁断：在册数 = 本表计数增量、掩码全合法、序号严格递增。
    pub fn consistent(&self) -> bool {
        self.items.len() == self.add_count.wrapping_sub(self.table_base) as usize
            && self.bad_mask().is_empty()
            && self.seq_monotonic()
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含 fd/掩码/序）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = EA_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let e = &self.items[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.mask as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.seq as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.add_count as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// fd 确定性样本。
pub fn fd_sample(no: u32) -> u32 {
    no.wrapping_mul(3).wrapping_add(3)
}

/// 掩码确定性样本（轮转合法位）。
pub fn mask_sample(no: u32) -> u32 {
    // 单位掩码循环：bit(no % 6)
    let sh = no % 6;
    1u32.wrapping_shl(sh)
}

/// 千次注册全链：64 fd 循环重登？——fd 重号必拒，改为千次计数账：
/// 兴趣表满 64 后重开新表，计数跨表累计。
pub fn run_add_thousand() -> (InterestList, bool) {
    let mut l = InterestList::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= ROUNDS {
        let fd = fd_sample((i.wrapping_sub(1)) % LIST_CAP as u32 + 1);
        let m = mask_sample(i);
        if l.add(fd, m).is_err() {
            // 表满重开：清在册项，计数带过去作新表起算基
            l.reopen();
            if l.add(fd, m).is_err() {
                ok = false;
            }
        }
        i += 1;
    }
    let fin = ok && l.add_count == ROUNDS;
    (l, fin)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 零掩码必拒
    let mut a = InterestList::new();
    got[0] = a.add(3, 0).err() == Some(EA_ZEROMASK);
    // 2) fd 重号必拒
    let mut b = InterestList::new();
    let _ = b.add(3, 1);
    got[1] = b.add(3, 2).err() == Some(EA_DUPFD);
    // 3) 非法掩码位必拒
    let mut c = InterestList::new();
    got[2] = c.add(3, 0x40).err() == Some(EA_ZEROMASK);
    // 4) 冻结后写账必抓
    let mut d = InterestList::new();
    let _ = d.add(3, 1);
    d.freeze();
    got[3] = d.add(4, 1).err() == Some(EA_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11008_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11008");

    // J1 ADD 注册千次账对平：计数恰为千
    let (l, green) = run_add_thousand();
    s.add("J1-千次账对平", green && l.add_count == ROUNDS, "千次 ADD 计数账零漂移");

    // J2 兴趣列表一致性：在册项掩码全合法且序号递增
    s.add("J2-兴趣列表一致性", l.consistent() && l.bad_mask().is_empty(), "在册项掩码合法且落账序严格递增");

    // J3 fd 重开零残留：末表在册项数恰为千次对定容的余数
    let tail = (ROUNDS % LIST_CAP as u32) as usize;
    s.add("J3-重开零残留", l.items.len() == tail, "末表在册项数恰为千次对定容余数");

    // J4 冻结根：fold 稳定且与根一致
    let mut l2 = l;
    let root = l2.freeze();
    s.add("J4-冻结根", l2.root == Some(root) && l2.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：零掩码 + fd 重号
    let inj = inject_four();
    s.add("J5-掩码重号检出", inj[0] && inj[1], "零掩码与 fd 重号两路必抓");

    // J6 注入检出：非法掩码位 + 冻结后写
    s.add("J6-非法冻结检出", inj[2] && inj[3], "越权掩码位必拒且冻结后写必抓");

    // J7 按 fd 查项：首末命中且回读掩码一致
    let j7 = {
        let mut c = InterestList::new();
        let mut i = 1u32;
        while i <= 8u32 {
            let _ = c.add(fd_sample(i), mask_sample(i));
            i += 1;
        }
        let g0 = c.get(fd_sample(1));
        let g7 = c.get(fd_sample(8));
        match (g0, g7) {
            (Some(a), Some(b)) => a.mask == mask_sample(1) && b.mask == mask_sample(8),
            _ => false,
        }
    };
    s.add("J7-按 fd 查项", j7, "首末项命中且掩码回读一致");

    // J8 越界 fd 拒（fd 零）
    let j8 = InterestList::new().add(0, 1).err() == Some(EA_NOFD);
    s.add("J8-零 fd 拒", j8, "fd 零必拒");

    // J9 单表超容拒：第 LIST_CAP+1 项必拒
    let j9 = {
        let mut c = InterestList::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= LIST_CAP as u32 {
            if c.add(n.wrapping_add(1), 1).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.add(999, 1).err() == Some(EA_CAP)
    };
    s.add("J9-超容边界", j9, "超出定容必拒且原账不损");

    // J10 落账序严格递增：千次计数账序无回退
    s.add("J10-落账序递增", l2.seq_monotonic(), "末表落账序号严格递增");

    // J11 掩码轮转覆盖全部合法位：六位均出现过
    let j11 = {
        let mut c = InterestList::new();
        let mut i = 1u32;
        while i <= 6u32 {
            let _ = c.add(i, mask_sample(i));
            i += 1;
        }
        let mut bits = 0u32;
        let mut k = 0usize;
        while k < c.items.len() {
            bits |= c.items[k].mask;
            k += 1;
        }
        bits == MASK_LEGAL
    };
    s.add("J11-掩码全位覆盖", j11, "六轮单掩码轮转覆盖全部合法位");

    // J12 渲染面：含 fd 与掩码十六进制
    let j12 = {
        let it = InterestItem { fd: 7, mask: 0x8, seq: 3 };
        let r = it.render();
        r.contains("fd7") && r.contains("m0x8")
    };
    s.add("J12-渲染明细", j12, "兴趣项渲染含 fd 与掩码十六进制");

    // J13 样本确定性：fd 与掩码同入同出且 fd 互异
    let f3a = fd_sample(3);
    let f3b = fd_sample(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= LIST_CAP as u32 {
        let mut j = i.wrapping_add(1);
        while j <= LIST_CAP as u32 {
            if fd_sample(i) == fd_sample(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J13-样本确定性", f3a == f3b && distinct, "fd 样本同入同出且互异");

    // J14 动态明细：千次计数落账
    let ac = l2.add_count;
    s.add_dyn("J14-计数明细", ac == ROUNDS, &format!("add_count={}", ac));

    // J15 动态明细：冻结根十六进制落账
    let fp = l2.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11008_interest_flow() {
        let mut l = InterestList::new();
        assert!(l.add(0, 1).err() == Some(EA_NOFD));
        assert!(l.add(3, 0).err() == Some(EA_ZEROMASK));
        assert!(l.add(3, 1).is_ok());
        assert!(l.add(3, 2).err() == Some(EA_DUPFD));
        assert!(l.contains(3) && !l.contains(4));
        l.freeze();
        assert!(l.add(4, 1).err() == Some(EA_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11008_checks_green() {
        let s = run_unx11008_checks();
        assert!(s.all_passed(), "UNX-F11008 判据未全绿");
    }
}
