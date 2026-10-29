//! UNX-F11018 挂号压力联测（futex 万次 WAKE/WAIT 与 epoll 千次事件）（0x1e240 段）
//!
//! 锚点：UNX-F11018-J1 futex 万次账守恒（10/10），epoll 千次账守恒
//! （10/10）。
//! 域：CoRun·UNX-C4-B31（futex/epoll 挂号联签域）。
//!
//! 结构：压力账（futex 键队列深度账 + epoll fd 事件账）+ 万次 WAIT/WAKE
//! 循环（逐键入眠/唤醒、队列深度归零守恒）+ 千次事件循环（逐 fd 投递/
//! 处理、事件账守恒）+ fold 指纹与冻结 + 注入核（唤醒无人/处理无事件/
//! 键深度负走/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e240 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e240;
pub const SP_WAKE0: u32 = SEG_BASE + 0x01; // 唤醒无人
pub const SP_HANDLE0: u32 = SEG_BASE + 0x02; // 处理无事件
pub const SP_NEGDEPTH: u32 = SEG_BASE + 0x03; // 键深度负走
pub const SP_NOKEY: u32 = SEG_BASE + 0x04; // 键/fd 不存在
pub const SP_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const SP_CAP: u32 = SEG_BASE + 0x06; // 压力账超容

/// futex 万次与 epoll 千次。
pub const FT_ROUNDS: u32 = 10_000;
pub const EP_ROUNDS: u32 = 1_000;
/// 压力账定容。
pub const SP_CAP_N: usize = 64;
/// 压力账域标（fold 种子）。
pub const SP_TAG: u64 = 0x1e24_12;

/// futex 键账项：键 + 队列深度。
#[derive(Clone, Debug)]
pub struct KeySlot {
    pub key: u32,
    pub depth: u32,
    pub wake_total: u32,
}

/// epoll fd 账项：fd + 事件余额。
#[derive(Clone, Debug)]
pub struct FdSlot {
    pub fd: u32,
    pub pending: u32,
    pub handled_total: u32,
}

/// 压力账：万次 futex 与千次 epoll 双账守恒。
pub struct StressLedger {
    pub keys: Vec<KeySlot>,
    pub fds: Vec<FdSlot>,
    pub wait_total: u32,
    pub wake_total: u32,
    pub event_total: u32,
    pub handle_total: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl StressLedger {
    pub fn new() -> StressLedger {
        StressLedger {
            keys: Vec::new(),
            fds: Vec::new(),
            wait_total: 0,
            wake_total: 0,
            event_total: 0,
            handle_total: 0,
            frozen: false,
            root: None,
        }
    }

    /// 挂 futex 键。
    pub fn add_key(&mut self, key: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        if key == 0 {
            return Err(SP_NOKEY);
        }
        if self.keys.iter().any(|k| k.key == key) {
            return Err(SP_CAP);
        }
        if self.keys.len() >= SP_CAP_N {
            return Err(SP_CAP);
        }
        self.keys.push(KeySlot { key, depth: 0, wake_total: 0 });
        Ok(())
    }

    /// 挂 epoll fd。
    pub fn add_fd(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        if fd == 0 {
            return Err(SP_NOKEY);
        }
        if self.fds.iter().any(|f| f.fd == fd) {
            return Err(SP_CAP);
        }
        if self.fds.len() >= SP_CAP_N {
            return Err(SP_CAP);
        }
        self.fds.push(FdSlot { fd, pending: 0, handled_total: 0 });
        Ok(())
    }

    /// WAIT 入眠：键深度加一。
    pub fn wait_on(&mut self, key: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        let mut k = 0usize;
        while k < self.keys.len() {
            if self.keys[k].key == key {
                self.keys[k].depth = self.keys[k].depth.wrapping_add(1);
                self.wait_total = self.wait_total.wrapping_add(1);
                return Ok(());
            }
            k += 1;
        }
        Err(SP_NOKEY)
    }

    /// WAKE 唤醒：键深度为零必拒（唤醒无人），否则深度减一。
    pub fn wake_on(&mut self, key: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        let mut k = 0usize;
        while k < self.keys.len() {
            if self.keys[k].key == key {
                if self.keys[k].depth == 0 {
                    return Err(SP_WAKE0);
                }
                self.keys[k].depth -= 1;
                self.keys[k].wake_total = self.keys[k].wake_total.wrapping_add(1);
                self.wake_total = self.wake_total.wrapping_add(1);
                return Ok(());
            }
            k += 1;
        }
        Err(SP_NOKEY)
    }

    /// 事件投递：fd 余额加一。
    pub fn event_on(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        let mut k = 0usize;
        while k < self.fds.len() {
            if self.fds[k].fd == fd {
                self.fds[k].pending = self.fds[k].pending.wrapping_add(1);
                self.event_total = self.event_total.wrapping_add(1);
                return Ok(());
            }
            k += 1;
        }
        Err(SP_NOKEY)
    }

    /// 事件处理：余额为零必拒（处理无事件），否则减一。
    pub fn handle_on(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_FROZEN);
        }
        let mut k = 0usize;
        while k < self.fds.len() {
            if self.fds[k].fd == fd {
                if self.fds[k].pending == 0 {
                    return Err(SP_HANDLE0);
                }
                self.fds[k].pending -= 1;
                self.fds[k].handled_total = self.fds[k].handled_total.wrapping_add(1);
                self.handle_total = self.handle_total.wrapping_add(1);
                return Ok(());
            }
            k += 1;
        }
        Err(SP_NOKEY)
    }

    /// futex 万次守恒：WAIT 总数 = WAKE 总数且逐键深度归零。
    pub fn ft_conserved(&self, rounds: u32) -> bool {
        if self.wait_total != rounds || self.wake_total != rounds {
            return false;
        }
        let mut k = 0usize;
        while k < self.keys.len() {
            if self.keys[k].depth != 0 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// epoll 千次守恒：事件总数 = 处理总数且逐 fd 余额归零。
    pub fn ep_conserved(&self, rounds: u32) -> bool {
        if self.event_total != rounds || self.handle_total != rounds {
            return false;
        }
        let mut k = 0usize;
        while k < self.fds.len() {
            if self.fds[k].pending != 0 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含双账与四计数）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SP_TAG;
        let mut k = 0usize;
        while k < self.keys.len() {
            h = (h ^ self.keys[k].key as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.keys[k].depth as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.keys[k].wake_total as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        let mut k = 0usize;
        while k < self.fds.len() {
            h = (h ^ self.fds[k].fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.fds[k].pending as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.fds[k].handled_total as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.wait_total as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.wake_total as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.event_total as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.handle_total as u64).wrapping_mul(0x100000001b3);
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

/// 键与 fd 确定性样本。
pub fn sp_key(no: u32) -> u32 {
    no.wrapping_mul(9).wrapping_add(1)
}

/// 万次 + 千次压测全链。
pub fn run_stress() -> (StressLedger, bool) {
    let mut l = StressLedger::new();
    let mut ok = true;
    // 双族各十号
    let mut i = 1u32;
    while i <= 10u32 {
        if l.add_key(sp_key(i)).is_err() {
            ok = false;
        }
        if l.add_fd(sp_key(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    // futex 万次：每键千次 WAIT/WAKE 成对
    let mut n = 0u32;
    while n < FT_ROUNDS {
        let key = sp_key(n % 10 + 1);
        if l.wait_on(key).is_err() {
            ok = false;
        }
        if l.wake_on(key).is_err() {
            ok = false;
        }
        n += 1;
    }
    // epoll 千次：每 fd 百次投递/处理成对
    let mut n = 0u32;
    while n < EP_ROUNDS {
        let fd = sp_key(n % 10 + 1);
        if l.event_on(fd).is_err() {
            ok = false;
        }
        if l.handle_on(fd).is_err() {
            ok = false;
        }
        n += 1;
    }
    let l_ok = ok && l.ft_conserved(FT_ROUNDS) && l.ep_conserved(EP_ROUNDS);
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 唤醒无人必拒
    let mut a = StressLedger::new();
    let _ = a.add_key(5);
    got[0] = a.wake_on(5).err() == Some(SP_WAKE0);
    // 2) 处理无事件必拒
    let mut b = StressLedger::new();
    let _ = b.add_fd(5);
    got[1] = b.handle_on(5).err() == Some(SP_HANDLE0);
    // 3) 键/fd 不存在必拒
    let mut c = StressLedger::new();
    let _ = c.add_key(5);
    got[2] = c.wait_on(9).err() == Some(SP_NOKEY) && c.event_on(9).err() == Some(SP_NOKEY);
    // 4) 冻结后写账必抓
    let (mut d, _) = run_stress();
    d.freeze();
    got[3] = d.wait_on(sp_key(1)).err() == Some(SP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11018_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11018");

    // J1 futex 万次账守恒
    let (mut book, green) = run_stress();
    s.add("J1-futex 万次守恒", green && book.ft_conserved(FT_ROUNDS), "万次 WAIT/WAKE 成对守恒深度归零");

    // J2 epoll 千次账守恒
    s.add("J2-epoll 千次守恒", book.ep_conserved(EP_ROUNDS), "千次投递/处理成对守恒余额归零");

    // J3 双账独立：futex 计数与 epoll 计数互不扰动
    s.add(
        "J3-双账独立",
        book.wait_total == FT_ROUNDS && book.event_total == EP_ROUNDS && book.wait_total != book.event_total,
        "双账计数各自守恒互不扰动",
    );

    // J4 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J4-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J5 注入检出：唤醒无人 + 处理无事件
    let inj = inject_four();
    s.add("J5-空唤空处理检出", inj[0] && inj[1], "唤醒无人与处理无事件两路必抓");

    // J6 注入检出：键不存在 + 冻结后写
    s.add("J6-无键冻结检出", inj[2] && inj[3], "键/fd 不存在必拒且冻结后写必抓");

    // J7 深度瞬态可探：入眠不醒深度留一
    let j7 = {
        let mut c = StressLedger::new();
        let _ = c.add_key(5);
        let _ = c.wait_on(5);
        c.keys[0].depth == 1 && !c.ft_conserved(1)
    };
    s.add("J7-深度瞬态可探", j7, "入眠未醒时守恒立即为假");

    // J8 余额瞬态可探：投递不处理余额留一
    let j8 = {
        let mut c = StressLedger::new();
        let _ = c.add_fd(5);
        let _ = c.event_on(5);
        c.fds[0].pending == 1 && !c.ep_conserved(1)
    };
    s.add("J8-余额瞬态可探", j8, "投递未处理时守恒立即为假");

    // J9 零号键/fd 拒
    let j9 = StressLedger::new().add_key(0).err() == Some(SP_NOKEY)
        && StressLedger::new().add_fd(0).err() == Some(SP_NOKEY);
    s.add("J9-零号拒", j9, "零键与零 fd 均必拒");

    // J10 重号键/fd 拒
    let j10 = {
        let mut c = StressLedger::new();
        let _ = c.add_key(5);
        let _ = c.add_fd(5);
        c.add_key(5).err() == Some(SP_CAP) && c.add_fd(5).err() == Some(SP_CAP)
    };
    s.add("J10-重号拒", j10, "键与 fd 重挂必拒");

    // J11 超容拒：第 SP_CAP_N+1 号必拒
    let j11 = {
        let mut c = StressLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= SP_CAP_N as u32 {
            if c.add_key(n.wrapping_add(1)).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.add_key(99999).err() == Some(SP_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 逐键唤醒留痕：每键 wake_total 恰千
    let j12 = {
        let mut all = true;
        let mut k = 0usize;
        while k < book.keys.len() {
            if book.keys[k].wake_total != FT_ROUNDS / 10 {
                all = false;
            }
            k += 1;
        }
        all && book.keys.len() == 10
    };
    s.add("J12-逐键留痕", j12, "十键各千次唤醒留痕齐平");

    // J13 逐 fd 处理留痕：每 fd handled_total 恰百
    let j13 = {
        let mut all = true;
        let mut k = 0usize;
        while k < book.fds.len() {
            if book.fds[k].handled_total != EP_ROUNDS / 10 {
                all = false;
            }
            k += 1;
        }
        all && book.fds.len() == 10
    };
    s.add("J13-逐 fd 留痕", j13, "十 fd 各百次处理留痕齐平");

    // J14 动态明细：四计数落账
    s.add_dyn(
        "J14-四计数明细",
        book.wait_total == FT_ROUNDS && book.handle_total == EP_ROUNDS,
        &format!("wait={} wake={} ev={} hd={}", book.wait_total, book.wake_total, book.event_total, book.handle_total),
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
    fn unx11018_stress_flow() {
        let mut l = StressLedger::new();
        assert!(l.add_key(5).is_ok());
        assert!(l.add_key(5).err() == Some(SP_CAP));
        assert!(l.wake_on(5).err() == Some(SP_WAKE0));
        assert!(l.wait_on(5).is_ok());
        assert!(l.wake_on(5).is_ok());
        assert!(l.add_fd(5).is_ok());
        assert!(l.handle_on(5).err() == Some(SP_HANDLE0));
        assert!(l.event_on(5).is_ok());
        assert!(l.handle_on(5).is_ok());
        assert!(l.ft_conserved(1) && l.ep_conserved(1));
        l.freeze();
        assert!(l.wait_on(5).err() == Some(SP_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11018_checks_green() {
        let s = run_unx11018_checks();
        assert!(s.all_passed(), "UNX-F11018 判据未全绿");
    }
}
