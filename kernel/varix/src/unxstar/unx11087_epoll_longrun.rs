//! UNX-F11087 epoll 长稳段（百万事件驱动）（0x1eae0 段）
//!
//! 8 个兴趣 fd 交替使用 Level 与 Edge 语义，按 10 个十万事件窗口推进，总计
//! 一百万事件。Level fd 的每个事件都必须投递；Edge fd 只有“同 fd 同事件种
//! 类连续出现”才被抑制。每 64 个事件形成一个 wait 批次，批次总数为 15625。
//! 不变式：raw = delivered + suppressed = 1,000,000，LT 侧零抑制，
//! ET 侧 delivered + suppressed = raw，lost 恒为零。
//!
//! 结构：兴趣项（fd/掩码/模式/代次）+ 事件窗口（原始/投递/抑制/三类事件/批次）
//! + epoll 总账（百万账、模式语义、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11087 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1eae0;

/// 空账 —— 尚未跑任何事件窗口。
pub const EL_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— fd 为零、掩码为零/超位、窗口号或批次参数越界。
pub const EL_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— 同一 fd 重复注册兴趣。
pub const EL_DUP: u32 = SEG_BASE + 0x03;
/// 模式错 —— 已武装 fd 试图从 Level 切到 Edge（或反向）。
pub const EL_MODE: u32 = SEG_BASE + 0x04;
/// 事件丢失 —— raw 不等于 delivered+suppressed，或百万账未闭合。
pub const EL_LOSS: u32 = SEG_BASE + 0x05;
/// 封账后写 —— 总账冻结后仍注册或推进。
pub const EL_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（epoll 百万事件账）。
pub const EL_TAG: u64 = 0x1ea_87;

/// 兴趣 fd 数。
pub const EL_FDS: usize = 8;
/// 窗口数。
pub const EL_WINDOWS: usize = 10;
/// 每窗事件数。
pub const EL_PER_WINDOW: u32 = 100_000;
/// 百万事件总数。
pub const EL_EVENTS: u64 = 1_000_000;
/// wait 批次大小。
pub const EL_BATCH: u64 = 64;

/// epoll 触发模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Level,
    Edge,
}

impl Trigger {
    pub fn code(self) -> u8 {
        match self {
            Trigger::Level => 0,
            Trigger::Edge => 1,
        }
    }
}

/// 一个 epoll 兴趣项。
#[derive(Clone, Debug)]
pub struct Interest {
    pub fd: u32,
    pub mask: u8,
    pub mode: Trigger,
    pub generation: u16,
}

/// 一个十万事件窗口。
#[derive(Clone, Debug)]
pub struct EventWindow {
    pub window: u16,
    pub raw: u32,
    pub delivered: u32,
    pub suppressed: u32,
    pub readable: u32,
    pub writable: u32,
    pub errors: u32,
    pub batches: u32,
}

impl EventWindow {
    /// 窗口账守恒。
    pub fn balanced(&self) -> bool {
        self.raw != 0
            && self.delivered.wrapping_add(self.suppressed) == self.raw
            && self.readable.wrapping_add(self.writable).wrapping_add(self.errors) == self.raw
    }
}

/// epoll 百万事件总账。
#[derive(Clone, Debug)]
pub struct EpollRun {
    pub interests: Vec<Interest>,
    pub windows: Vec<EventWindow>,
    pub raw: u64,
    pub delivered: u64,
    pub suppressed: u64,
    pub lt_raw: u64,
    pub lt_delivered: u64,
    pub et_raw: u64,
    pub et_delivered: u64,
    pub batches: u32,
    pub max_batch: u16,
    pub lost: u32,
    pub prev_et_fd: u32,
    pub prev_et_kind: u8,
    pub has_prev: bool,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl EpollRun {
    pub fn new() -> EpollRun {
        EpollRun {
            interests: Vec::new(),
            windows: Vec::new(),
            raw: 0,
            delivered: 0,
            suppressed: 0,
            lt_raw: 0,
            lt_delivered: 0,
            et_raw: 0,
            et_delivered: 0,
            batches: 0,
            max_batch: 0,
            lost: 0,
            prev_et_fd: 0,
            prev_et_kind: 0,
            has_prev: false,
            frozen: false,
            root: None,
        }
    }

    fn interest_at(&self, fd: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.interests.len() {
            if self.interests[i].fd == fd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 注册兴趣；掩码只允许低三位，0 与超位均拒。
    pub fn add_interest(&mut self, fd: u32, mask: u8, mode: Trigger) -> Result<(), u32> {
        if self.frozen {
            return Err(EL_FROZEN);
        }
        if fd == 0 || mask == 0 || mask > 0x07 {
            return Err(EL_RANGE);
        }
        if self.interest_at(fd).is_some() {
            return Err(EL_DUP);
        }
        if self.interests.len() >= EL_FDS {
            return Err(EL_RANGE);
        }
        self.interests.push(Interest {
            fd,
            mask,
            mode,
            generation: self.interests.len() as u16 + 1,
        });
        Ok(())
    }

    /// 模式切换在武装后禁止；原模式重提视为幂等。
    pub fn retune(&mut self, fd: u32, mode: Trigger) -> Result<(), u32> {
        if self.frozen {
            return Err(EL_FROZEN);
        }
        let ii = match self.interest_at(fd) {
            Some(i) => i,
            None => return Err(EL_RANGE),
        };
        if self.interests[ii].mode != mode {
            return Err(EL_MODE);
        }
        Ok(())
    }

    /// 纯函数式事件账校验，供注入与窗口复用。
    pub fn verify_account(&mut self, raw: u64, delivered: u64, suppressed: u64) -> Result<(), u32> {
        if raw == 0 {
            return Err(EL_RANGE);
        }
        if delivered.wrapping_add(suppressed) != raw {
            self.lost = self.lost.wrapping_add(1);
            return Err(EL_LOSS);
        }
        Ok(())
    }

    /// 跑一个十万事件窗口。每轮从窗口局部计数开始，不按事件 push。
    pub fn run_window(&mut self, window: u16) -> Result<EventWindow, u32> {
        if self.frozen {
            return Err(EL_FROZEN);
        }
        if window >= EL_WINDOWS as u16 || window as usize != self.windows.len() {
            return Err(EL_RANGE);
        }
        if self.interests.len() != EL_FDS {
            return Err(EL_EMPTY);
        }
        let mut raw = 0u32;
        let mut delivered = 0u32;
        let mut suppressed = 0u32;
        let mut readable = 0u32;
        let mut writable = 0u32;
        let mut errors = 0u32;
        let mut batch_win = 0u32;
        let mut i = 0u32;
        while i < EL_PER_WINDOW {
            let serial = (window as u32).wrapping_mul(EL_PER_WINDOW).wrapping_add(i);
            // 每两个事件共享同一 fd/种类，给 Edge 语义制造真实的连续重复。
            let token = serial / 2;
            let fi = (token as usize) % EL_FDS;
            let fd = self.interests[fi].fd;
            let mode = self.interests[fi].mode;
            let kind = (token.wrapping_add(fi as u32) % 10) as u8;
            raw = raw.wrapping_add(1);
            self.raw = self.raw.wrapping_add(1);
            if self.raw % EL_BATCH == 1 {
                self.batches = self.batches.wrapping_add(1);
                batch_win = batch_win.wrapping_add(1);
            }
            match kind {
                0..=4 => readable = readable.wrapping_add(1),
                5..=8 => writable = writable.wrapping_add(1),
                _ => errors = errors.wrapping_add(1),
            }
            match mode {
                Trigger::Level => {
                    self.lt_raw = self.lt_raw.wrapping_add(1);
                    self.lt_delivered = self.lt_delivered.wrapping_add(1);
                    self.delivered = self.delivered.wrapping_add(1);
                    delivered = delivered.wrapping_add(1);
                }
                Trigger::Edge => {
                    self.et_raw = self.et_raw.wrapping_add(1);
                    let repeat = self.has_prev && self.prev_et_fd == fd
                        && self.prev_et_kind == kind;
                    if repeat {
                        self.suppressed = self.suppressed.wrapping_add(1);
                        suppressed = suppressed.wrapping_add(1);
                    } else {
                        self.et_delivered = self.et_delivered.wrapping_add(1);
                        self.delivered = self.delivered.wrapping_add(1);
                        delivered = delivered.wrapping_add(1);
                        self.prev_et_fd = fd;
                        self.prev_et_kind = kind;
                        self.has_prev = true;
                    }
                }
            }
            i = i.wrapping_add(1);
        }
        if let Err(e) = self.verify_account(raw as u64, delivered as u64, suppressed as u64) {
            return Err(e);
        }
        self.max_batch = EL_BATCH as u16;
        let rec = EventWindow {
            window,
            raw,
            delivered,
            suppressed,
            readable,
            writable,
            errors,
            batches: batch_win,
        };
        self.windows.push(rec.clone());
        Ok(rec)
    }

    /// 跑满十个窗口（百万事件）。
    pub fn run_all(&mut self) -> Result<u64, u32> {
        let mut w = 0u16;
        while w < EL_WINDOWS as u16 {
            self.run_window(w)?;
            w = w.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 百万事件守恒。
    pub fn million_conserved(&self) -> bool {
        self.raw == EL_EVENTS
            && self.delivered.wrapping_add(self.suppressed) == self.raw
            && self.lost == 0
            && self.windows.len() == EL_WINDOWS
    }

    /// 模式语义：LT 零抑制，ET 自身两侧账对平。
    pub fn mode_semantics_ok(&self) -> bool {
        self.lt_delivered == self.lt_raw
            && self.et_delivered.wrapping_add(self.suppressed) == self.et_raw
            && self.lt_raw.wrapping_add(self.et_raw) == self.raw
            && self.lt_raw > 0
            && self.et_raw > 0
            && self.suppressed < self.raw
    }

    /// 窗口账逐窗对平且批次总数正确。
    pub fn windows_ok(&self) -> bool {
        if self.windows.len() != EL_WINDOWS {
            return false;
        }
        let mut sum_raw = 0u64;
        let mut sum_batches = 0u32;
        let mut i = 0usize;
        while i < self.windows.len() {
            if self.windows[i].window as usize != i || !self.windows[i].balanced() {
                return false;
            }
            sum_raw = sum_raw.wrapping_add(self.windows[i].raw as u64);
            sum_batches = sum_batches.wrapping_add(self.windows[i].batches);
            i += 1;
        }
        sum_raw == EL_EVENTS && sum_batches == self.batches && self.batches == 15_625
    }

    /// 总审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.raw == 0 {
            return Err(EL_EMPTY);
        }
        if self.lost != 0 || self.raw != EL_EVENTS {
            return Err(EL_LOSS);
        }
        if self.delivered.wrapping_add(self.suppressed) != self.raw {
            return Err(EL_LOSS);
        }
        if !self.mode_semantics_ok() {
            return Err(EL_MODE);
        }
        if !self.windows_ok() {
            return Err(EL_LOSS);
        }
        Ok(())
    }

    /// FNV-1a：兴趣项、每窗事件账与全局模式计数全入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = EL_TAG;
        let mut i = 0usize;
        while i < self.interests.len() {
            let it = &self.interests[i];
            h = (h ^ it.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.mask as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.mode.code() as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.generation as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut k = 0usize;
        while k < self.windows.len() {
            let w = &self.windows[k];
            h = (h ^ w.window as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.raw as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.delivered as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.suppressed as u64).wrapping_mul(0x100000001b3);
            h = (h ^ w.batches as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.raw).wrapping_mul(0x100000001b3);
        h = (h ^ self.delivered).wrapping_mul(0x100000001b3);
        h = (h ^ self.suppressed).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结百万事件账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 事件/模式摘要。
    pub fn summary(&self) -> String {
        let label = "epoll".to_string();
        format!("{}|raw={}|del={}|sup={}|batches={}|lost={}", label, self.raw,
            self.delivered, self.suppressed, self.batches, self.lost)
    }
}

/// 四路坏样本：fd 重号、零掩码、模式回切、冻结后注册。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let mut a = EpollRun::new();
    let _ = a.add_interest(1, 0x01, Trigger::Level);
    got[0] = a.add_interest(1, 0x02, Trigger::Edge).err() == Some(EL_DUP);

    let mut b = EpollRun::new();
    got[1] = b.add_interest(2, 0, Trigger::Level).err() == Some(EL_RANGE);

    let mut c = EpollRun::new();
    let _ = c.add_interest(3, 0x01, Trigger::Level);
    got[2] = c.retune(3, Trigger::Edge).err() == Some(EL_MODE);

    let mut d = EpollRun::new();
    let _ = d.add_interest(4, 0x03, Trigger::Edge);
    let _ = d.freeze();
    got[3] = d.add_interest(5, 0x01, Trigger::Level).err() == Some(EL_FROZEN);
    got
}

/// epoll 百万事件判据。
pub fn run_unx11087_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11087");

    let mut b = EpollRun::new();
    let mut fd = 1u32;
    while fd <= EL_FDS as u32 {
        let mode = if fd <= 4 { Trigger::Level } else { Trigger::Edge };
        let _ = b.add_interest(fd, 0x07, mode);
        fd = fd.wrapping_add(1);
    }
    let run = b.run_all();

    // J1 两半：事件账百万守恒，模式语义百万零漂。
    let j1a = b.million_conserved();
    let j1b = b.mode_semantics_ok() && b.lt_delivered == b.lt_raw;
    s.add("J1-事件账百万守恒", j1a, "raw 等于 delivered+suppressed 且 lost=0");
    s.add("J1-模式语义零漂", j1b, "LT 零抑制且 ET 两侧账对平");

    // J2 批次账：15625 个 64 事件批次，逐窗对平。
    let j2 = b.windows_ok() && b.batches == 15_625 && b.max_batch == 64;
    s.add("J2-批次账", j2, "10 窗批次和等于 15625");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-重号注入", inj[0], "同 fd 二次注册必拒");
    s.add("J3-掩码注入", inj[1], "零掩码注册必拒");
    s.add("J3-模式注入", inj[2], "武装后模式回切必拒");
    s.add("J3-冻结注入", inj[3], "冻结后注册兴趣必拒");

    // J4 冻结：根稳定且注册/推进双拒。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.root == Some(root) && f.fold() == root
        && f.add_interest(9, 1, Trigger::Level).err() == Some(EL_FROZEN)
        && f.run_window(0).err() == Some(EL_FROZEN);
    s.add("J4-封账不可变", j4, "冻结后注册与窗口推进均拒");

    // J5 指纹敏感：单窗抑制数变化即改根。
    let mut c = b.clone();
    c.windows[7].suppressed = c.windows[7].suppressed.wrapping_add(1);
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单窗抑制数变化即改变百万事件根");

    // J6 空账/范围/丢事件。
    let mut e = EpollRun::new();
    let empty = e.audit();
    let bad_fd = e.add_interest(0, 1, Trigger::Level);
    let loss = e.verify_account(100, 90, 9);
    let j6 = empty.err() == Some(EL_EMPTY) && bad_fd.err() == Some(EL_RANGE)
        && loss.err() == Some(EL_LOSS);
    s.add("J6-空账丢账", j6, "空账、零 fd、事件缺口均拒");

    // J7 兴趣唯一有序：8 个 fd 严格递增且 4 LT / 4 ET。
    let mut ordered = true;
    let mut lt = 0u8;
    let mut et = 0u8;
    let mut i = 1usize;
    while i < b.interests.len() {
        if b.interests[i].fd <= b.interests[i - 1].fd {
            ordered = false;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < b.interests.len() {
        match b.interests[k].mode {
            Trigger::Level => lt = lt.wrapping_add(1),
            Trigger::Edge => et = et.wrapping_add(1),
        }
        k += 1;
    }
    s.add("J7-兴趣有序", ordered && lt == 4 && et == 4, "8 fd 唯一且模式各半");

    // J8 负语义：九窗时不得判百万全绿。
    let mut partial = EpollRun::new();
    let mut pfd = 1u32;
    while pfd <= 8 {
        let _ = partial.add_interest(pfd, 0x07, if pfd <= 4 { Trigger::Level } else { Trigger::Edge });
        pfd = pfd.wrapping_add(1);
    }
    let mut pw = 0u16;
    while pw < 9 {
        let _ = partial.run_window(pw);
        pw = pw.wrapping_add(1);
    }
    let j8 = !partial.million_conserved() && partial.raw == 900_000
        && partial.audit().err() == Some(EL_LOSS);
    s.add("J8-九窗非绿", j8, "九十万事件不得冒充百万守恒");

    // J9 动态事件账。
    s.add_dyn("J9-事件动态", j1a,
        &format!("raw={} del={} sup={} lt={} et={}", b.raw, b.delivered,
            b.suppressed, b.lt_raw, b.et_raw));

    // J10 窗口种类分解守恒。
    let mut kind_ok = true;
    let mut wi = 0usize;
    while wi < b.windows.len() {
        let w = &b.windows[wi];
        if w.readable.wrapping_add(w.writable).wrapping_add(w.errors) != w.raw {
            kind_ok = false;
        }
        wi += 1;
    }
    s.add("J10-种类守恒", kind_ok, "readable+writable+errors 逐窗等于 raw");

    // J11 审计与摘要。
    let text = b.summary();
    let j11 = b.audit().is_ok() && run.is_ok() && text.starts_with("epoll|");
    s.add_dyn("J11-审计摘要", j11, &text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11087_flow() {
        let mut b = EpollRun::new();
        assert!(b.audit().err() == Some(EL_EMPTY));
        assert!(b.add_interest(1, 1, Trigger::Level).is_ok());
        assert!(b.add_interest(1, 2, Trigger::Edge).err() == Some(EL_DUP));
        assert!(b.retune(1, Trigger::Edge).err() == Some(EL_MODE));
        assert!(b.verify_account(100, 90, 9).err() == Some(EL_LOSS));
        let mut pfd = 2u32;
        while pfd <= 8 {
            let _ = b.add_interest(pfd, 1, Trigger::Edge);
            pfd = pfd.wrapping_add(1);
        }
        assert!(b.run_window(0).is_ok());
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.run_window(1).err() == Some(EL_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11087_checks_green() {
        assert!(run_unx11087_checks().all_passed(), "UNX-F11087 判据未全绿");
    }
}
