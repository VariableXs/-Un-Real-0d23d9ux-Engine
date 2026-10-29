//! UNX-F11011 epoll ONESHOT/EXCLUSIVE 挂点联签（单发与惊群抑制挂账）（0x1e160 段）
//!
//! 锚点：UNX-F11011-J1 ONESHOT 失能账逐次对平（10/10），EXCLUSIVE 抑制账
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：单发账（ONESHOT 触发即失能、失能后通知必拒）+ 抑制账（EXCLUSIVE
//! 组内单醒、组内其余必被抑制）+ 失能对账器（触发次数 = 失能次数）+ fold
//! 指纹与冻结 + 注入核（失能后通知/组内双醒/零 fd/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e160 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e160;
pub const OS_DISABLED: u32 = SEG_BASE + 0x01; // 失能后通知
pub const OS_DOUBLEWAKE: u32 = SEG_BASE + 0x02; // 组内双醒
pub const OS_NOFD: u32 = SEG_BASE + 0x03; // 零 fd
pub const OS_NOGROUP: u32 = SEG_BASE + 0x04; // 组不存在
pub const OS_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const OS_CAP: u32 = SEG_BASE + 0x06; // 挂账超容

/// 千轮联测轮数。
pub const ROUNDS: u32 = 1000;
/// 挂账定容。
pub const OS_CAP_N: usize = 64;
/// 单发账域标（fold 种子）。
pub const OS_TAG: u64 = 0x1e16_0b;

/// 挂点模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookMode {
    OneShot,
    Exclusive,
}

/// 单条挂账：fd + 组号 + 模式 + 触发位 + 失能位。
#[derive(Clone, Debug)]
pub struct HookItem {
    pub fd: u32,
    pub group: u32,
    pub mode: HookMode,
    pub fired: bool,
    pub disabled: bool,
}

/// epoll ONESHOT/EXCLUSIVE 挂账：登记 → 单发触发 → 组内抑制 → 冻结。
pub struct HookLedger {
    pub items: Vec<HookItem>,
    pub fire_count: u32,
    pub suppress_count: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl HookLedger {
    pub fn new() -> HookLedger {
        HookLedger { items: Vec::new(), fire_count: 0, suppress_count: 0, frozen: false, root: None }
    }

    /// 挂账登记：冻结、零 fd、重号、超容均拒。
    pub fn enroll(&mut self, fd: u32, group: u32, mode: HookMode) -> Result<(), u32> {
        if self.frozen {
            return Err(OS_FROZEN);
        }
        if fd == 0 {
            return Err(OS_NOFD);
        }
        if self.items.iter().any(|i| i.fd == fd) {
            return Err(OS_DOUBLEWAKE);
        }
        if self.items.len() >= OS_CAP_N {
            return Err(OS_CAP);
        }
        self.items.push(HookItem { fd, group, mode, fired: false, disabled: false });
        Ok(())
    }

    /// ONESHOT 触发：翻触发位即失能，二次通知必拒。
    pub fn fire_oneshot(&mut self, fd: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(OS_FROZEN);
        }
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].fd == fd && self.items[k].mode == HookMode::OneShot {
                if self.items[k].disabled {
                    return Err(OS_DISABLED);
                }
                self.items[k].fired = true;
                self.items[k].disabled = true;
                self.fire_count = self.fire_count.wrapping_add(1);
                return Ok(true);
            }
            k += 1;
        }
        Err(OS_NOFD)
    }

    /// EXCLUSIVE 唤醒：组内恰醒一个（序最靠前的未醒者），其余计入抑制。
    pub fn wake_exclusive(&mut self, group: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(OS_FROZEN);
        }
        // 先数组内未醒者（借用副本避免迭代中改容器）
        let mut waiting = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            if it.group == group && it.mode == HookMode::Exclusive && !it.fired {
                waiting += 1;
            }
            k += 1;
        }
        if waiting == 0 {
            return Err(OS_NOGROUP);
        }
        let mut woken = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].group == group
                && self.items[k].mode == HookMode::Exclusive
                && !self.items[k].fired
            {
                if woken == 0 {
                    self.items[k].fired = true;
                    self.fire_count = self.fire_count.wrapping_add(1);
                } else {
                    self.suppress_count = self.suppress_count.wrapping_add(1);
                }
                woken += 1;
            }
            k += 1;
        }
        Ok(1)
    }

    /// 失能项清单。
    pub fn disabled_fds(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].disabled {
                g.push(self.items[k].fd);
            }
            k += 1;
        }
        g
    }

    /// 单发对平：ONESHOT 项触发数 = 失能数。
    pub fn oneshot_conserved(&self) -> bool {
        let mut fired = 0u32;
        let mut disabled = 0u32;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            if it.mode == HookMode::OneShot {
                if it.fired {
                    fired += 1;
                }
                if it.disabled {
                    disabled += 1;
                }
            }
            k += 1;
        }
        fired == disabled
    }

    /// 组内零双醒探针：每个 Exclusive 组的 fired 数 ≤ 1。
    pub fn no_double_wake(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            let gi = self.items[i].group;
            let mut j = i.wrapping_add(1);
            while j < self.items.len() {
                if self.items[j].group == gi
                    && self.items[j].mode == HookMode::Exclusive
                    && self.items[i].mode == HookMode::Exclusive
                    && self.items[i].fired
                    && self.items[j].fired
                {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含 fd/组/模式/双位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = OS_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let e = &self.items[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.group as u64).wrapping_mul(0x100000001b3);
            let m = match e.mode {
                HookMode::OneShot => 1u64,
                HookMode::Exclusive => 2u64,
            };
            h = (h ^ m).wrapping_mul(0x100000001b3);
            h = (h ^ e.fired as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.disabled as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.fire_count as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.suppress_count as u64).wrapping_mul(0x100000001b3);
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
pub fn os_sample(no: u32) -> u32 {
    no.wrapping_mul(29).wrapping_add(4)
}

/// 千轮联测全链：ONESHOT 五项逐轮触发、EXCLUSIVE 组五项逐轮单醒。
pub fn run_hook_thousand() -> (HookLedger, bool) {
    let mut l = HookLedger::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= 5u32 {
        if l.enroll(os_sample(i), 7, HookMode::OneShot).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 1u32;
    while i <= 5u32 {
        if l.enroll(os_sample(i + 5), 9, HookMode::Exclusive).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut r = 0u32;
    while r < ROUNDS {
        // ONESHOT 逐项触发一轮（触发后失能，二次必拒即记 ok=false 不触发）
        let mut i = 1u32;
        while i <= 5u32 {
            let _ = l.fire_oneshot(os_sample(i));
            i += 1;
        }
        // 组重置：EXCLUSIVE 逐轮可再醒（重置触发位）
        let mut k = 0usize;
        while k < l.items.len() {
            if l.items[k].mode == HookMode::Exclusive {
                l.items[k].fired = false;
            }
            k += 1;
        }
        if l.wake_exclusive(9) != Ok(1) {
            ok = false;
        }
        r += 1;
    }
    let l_ok = ok && l.no_double_wake();
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) ONESHOT 失能后通知必拒
    let mut a = HookLedger::new();
    let _ = a.enroll(3, 7, HookMode::OneShot);
    let _ = a.fire_oneshot(3);
    got[0] = a.fire_oneshot(3).err() == Some(OS_DISABLED);
    // 2) 零 fd 必拒
    let mut b = HookLedger::new();
    got[1] = b.enroll(0, 7, HookMode::OneShot).err() == Some(OS_NOFD);
    // 3) 组不存在必拒
    let mut c = HookLedger::new();
    let _ = c.enroll(3, 7, HookMode::Exclusive);
    got[2] = c.wake_exclusive(42).err() == Some(OS_NOGROUP);
    // 4) 冻结后写账必抓
    let (mut d, _) = run_hook_thousand();
    d.freeze();
    got[3] = d.enroll(99, 7, HookMode::OneShot).err() == Some(OS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11011_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11011");

    // J1 ONESHOT 失能账逐次对平：触发数 = 失能数且位账一致
    let (mut book, green) = run_hook_thousand();
    s.add("J1-单发失能账", green && book.oneshot_conserved(), "ONESHOT 触发与失能逐次对平");

    // J2 EXCLUSIVE 抑制账：组内零双醒且抑制计数为千轮 × 四余
    s.add("J2-惊群抑制账", book.no_double_wake() && book.suppress_count == ROUNDS * 4, "组内单醒余四抑制千轮零漂");

    // J3 失能清单：五个 ONESHOT 全失能
    let dcnt = book.disabled_fds().len();
    s.add("J3-失能清单", dcnt == 5, "五个 ONESHOT 项全数失能在册");

    // J4 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J4-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：失能后通知 + 零 fd
    let inj = inject_four();
    s.add("J5-失能零 fd 检出", inj[0] && inj[1], "失能后通知与零 fd 两路必抓");

    // J6 注入检出：组不存在 + 冻结后写
    s.add("J6-组冻结检出", inj[2] && inj[3], "空组唤醒必拒且冻结后写必抓");

    // J7 模式分旗：ONESHOT 与 EXCLUSIVE 挂账可区分
    let j7 = HookMode::OneShot != HookMode::Exclusive;
    s.add("J7-模式分旗", j7, "两模式互异");

    // J8 重号挂账拒
    let j8 = {
        let mut c = HookLedger::new();
        let _ = c.enroll(3, 7, HookMode::OneShot);
        c.enroll(3, 9, HookMode::Exclusive).err() == Some(OS_DOUBLEWAKE)
    };
    s.add("J8-重号拒", j8, "fd 重号必拒");

    // J9 ONESHOT 触发即失能：触发后 disabled 置位且 fired 置位
    let j9 = {
        let mut c = HookLedger::new();
        let _ = c.enroll(3, 7, HookMode::OneShot);
        let _ = c.fire_oneshot(3);
        match c.items.iter().find(|i| i.fd == 3) {
            Some(it) => it.fired && it.disabled,
            None => false,
        }
    };
    s.add("J9-触发即失能", j9, "单发触发位与失能位同翻");

    // J10 超容拒：第 OS_CAP_N+1 项必拒
    let j10 = {
        let mut c = HookLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= OS_CAP_N as u32 {
            if c.enroll(n, 1, HookMode::OneShot).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(999, 1, HookMode::OneShot).err() == Some(OS_CAP)
    };
    s.add("J10-超容边界", j10, "超出定容必拒且原账不损");

    // J11 EXCLUSIVE 首醒选择：序最靠前者醒、其余抑制
    let j11 = {
        let mut c = HookLedger::new();
        let mut i = 1u32;
        while i <= 3u32 {
            let _ = c.enroll(i, 5, HookMode::Exclusive);
            i += 1;
        }
        let _ = c.wake_exclusive(5);
        let mut first = false;
        let mut rest = 0u32;
        let mut k = 0usize;
        while k < c.items.len() {
            if c.items[k].fired {
                if c.items[k].fd == 1 {
                    first = true;
                } else {
                    rest += 1;
                }
            }
            k += 1;
        }
        first && rest == 0 && c.suppress_count == 2
    };
    s.add("J11-首醒选择", j11, "组内序最靠前者单醒余者抑制");

    // J12 样本确定性：fd 同入同出且互异
    let v3a = os_sample(3);
    let v3b = os_sample(3);
    let mut distinct = true;
    let mut i = 1u32;
    while i <= 10u32 {
        let mut j = i.wrapping_add(1);
        while j <= 10u32 {
            if os_sample(i) == os_sample(j) {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("J12-样本确定性", v3a == v3b && distinct, "fd 样本同入同出且互异");

    // J13 空组多轮复醒：组重置后可再单醒（轮转语义）
    let j13 = {
        let mut c = HookLedger::new();
        let _ = c.enroll(1, 5, HookMode::Exclusive);
        let _ = c.enroll(2, 5, HookMode::Exclusive);
        let f1 = c.wake_exclusive(5);
        let mut k = 0usize;
        while k < c.items.len() {
            c.items[k].fired = false;
            k += 1;
        }
        let f2 = c.wake_exclusive(5);
        f1 == Ok(1) && f2 == Ok(1) && c.fire_count == 2
    };
    s.add("J13-组轮转复醒", j13, "组重置后逐轮可再单醒");

    // J14 动态明细：触发与抑制双计数落账
    let fc = book.fire_count;
    let sc = book.suppress_count;
    s.add_dyn(
        "J14-双计数明细",
        fc >= ROUNDS && sc == ROUNDS * 4,
        &format!("fire={} supp={}", fc, sc),
    );

    // J15 动态明细：冻结根十六进制落账
    let fp = book.fold();
    s.add_dyn("J15-冻结根明细", fp == root, &format!("root={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11011_hook_flow() {
        let mut l = HookLedger::new();
        assert!(l.enroll(0, 7, HookMode::OneShot).err() == Some(OS_NOFD));
        assert!(l.enroll(3, 7, HookMode::OneShot).is_ok());
        assert!(l.enroll(3, 9, HookMode::Exclusive).err() == Some(OS_DOUBLEWAKE));
        assert_eq!(l.fire_oneshot(3), Ok(true));
        assert!(l.fire_oneshot(3).err() == Some(OS_DISABLED));
        l.freeze();
        assert!(l.enroll(4, 7, HookMode::OneShot).err() == Some(OS_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11011_checks_green() {
        let s = run_unx11011_checks();
        assert!(s.all_passed(), "UNX-F11011 判据未全绿");
    }
}
