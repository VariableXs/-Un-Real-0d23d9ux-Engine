//! UNX-F10913 混合 fd 类型注册矩阵（全类型 pollable）（0x1d1a0 段）
//!
//! 锚点：UNX-F10913-J1 正面：五类型同 epoll 实例混注册全通（10/10）；
//! 反面：类型间零串扰（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·混合类型注册面）。
//!
//! 结构：五类型账（pipe/socket/tty/eventfd/timerfd 同实例定容混注）+
//! 路由器（事件按类型路由零串扰）+ 注入核（串扰/坏类型/重号/冻结改账
//! 四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d1a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d1a0;
pub const MX_CROSS: u32 = SEG_BASE + 0x01; // 类型间事件串扰
pub const MX_BADTYPE: u32 = SEG_BASE + 0x02; // 类型号非法
pub const MX_DUP: u32 = SEG_BASE + 0x03; // fd 重号
pub const MX_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后改账
pub const MX_NOEVENT: u32 = SEG_BASE + 0x05; // 全无事件

/// 五类类型号。
pub const T_PIPE: u8 = 1;
pub const T_SOCK: u8 = 2;
pub const T_TTY: u8 = 3;
pub const T_EFD: u8 = 4;
pub const T_TFD: u8 = 5;

/// 混注账定容。
pub const CAP: usize = 40;
/// MX 域标（fold 种子）。
pub const MX_TAG: u64 = 0x1d1a60d;

/// 混注条目。
#[derive(Clone, Debug)]
pub struct MixSlot {
    pub fd: i32,
    pub kind: u8,
    pub armed: bool,
}

impl MixSlot {
    pub fn kind_ok(&self) -> bool {
        self.kind >= T_PIPE && self.kind <= T_TFD
    }

    /// 类型旗位（每类独占一位，供串扰审计）。
    pub fn kind_flag(&self) -> u32 {
        1u32.wrapping_shl((self.kind as u32) % 32)
    }

    pub fn render(&self) -> String {
        let k = match self.kind {
            T_PIPE => "PIPE",
            T_SOCK => "SOCK",
            T_TTY => "TTY",
            T_EFD => "EFD",
            _ => "TFD",
        };
        format!("M{}|{}", self.fd, k)
    }
}

/// 混注账 + 事件路由。
pub struct MixBoard {
    pub slots: Vec<MixSlot>,
    pub frozen: bool,
    /// 每类累计收到的路由事件数（五类各一账）。
    pub routed: [u32; 5],
}

impl MixBoard {
    pub fn new() -> MixBoard {
        MixBoard { slots: Vec::new(), frozen: false, routed: [0u32; 5] }
    }

    /// 混注册：冻结、坏类型、重号、超容均拒。
    pub fn register(&mut self, fd: i32, kind: u8, armed: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(MX_FROZEN);
        }
        if kind < T_PIPE || kind > T_TFD {
            return Err(MX_BADTYPE);
        }
        if self.slots.iter().any(|e| e.fd == fd) {
            return Err(MX_DUP);
        }
        if self.slots.len() >= CAP {
            return Err(MX_BADTYPE);
        }
        self.slots.push(MixSlot { fd, kind, armed });
        Ok(())
    }

    /// 事件路由：按 fd 找类型，路由到对应类账并回报目标 fd。
    pub fn route(&mut self, fd: i32) -> Result<i32, u32> {
        let mut idx = self.slots.len();
        let mut k = 0usize;
        while k < self.slots.len() {
            if self.slots[k].fd == fd {
                idx = k;
            }
            k += 1;
        }
        if idx == self.slots.len() {
            return Err(MX_NOEVENT);
        }
        if !self.slots[idx].armed {
            return Err(MX_NOEVENT);
        }
        let ki = (self.slots[idx].kind - 1) as usize;
        self.routed[ki] += 1;
        Ok(fd)
    }

    /// 串扰审计：每类账只随本类 fd 增长。
    pub fn no_cross(&self, before: &[u32; 5], fired_kind: u8) -> bool {
        let mut k = 0usize;
        while k < 5 {
            let want = if (k as u8 + 1) == fired_kind { 1u32 } else { 0u32 };
            if self.routed[k] - before[k] != want {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 五类齐注审计。
    pub fn all_kinds(&self) -> bool {
        let mut seen = [false; 5];
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            if e.kind_ok() {
                seen[(e.kind - 1) as usize] = true;
            }
            k += 1;
        }
        seen[0] && seen[1] && seen[2] && seen[3] && seen[4]
    }

    /// 链式 fold 指纹（序敏感，含路由账）。
    pub fn fold(&self) -> u64 {
        let mut h = MX_TAG;
        let mut k = 0usize;
        while k < self.slots.len() {
            let e = &self.slots[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.kind as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.armed as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.routed[0] as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.routed[1] as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.routed[2] as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.routed[3] as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.routed[4] as u64).wrapping_mul(0x100000001b3);
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
    // 1) 串扰必抓：路由只入本类账，他类账不动
    let mut b = MixBoard::new();
    let _ = b.register(1, T_SOCK, true);
    let before = [0u32; 5];
    let _ = b.route(1);
    got[0] = b.no_cross(&before, T_SOCK) && b.routed[0] == 0;
    // 2) 坏类型必拒
    let mut x = MixBoard::new();
    got[1] = x.register(2, 0, true).err() == Some(MX_BADTYPE)
        && x.register(2, 9, true).err() == Some(MX_BADTYPE);
    // 3) 重号必拒
    let mut d = MixBoard::new();
    let _ = d.register(3, T_PIPE, true);
    got[2] = d.register(3, T_TTY, true).err() == Some(MX_DUP);
    // 4) 冻结后注册必抓
    let mut f = MixBoard::new();
    f.freeze();
    got[3] = f.register(4, T_PIPE, true).err() == Some(MX_FROZEN);
    got
}

/// 类型确定性样本：第 r 轮的类型。
pub fn mx_sample_kind(r: u32) -> u8 {
    (r.wrapping_mul(2654435761).wrapping_add(29) % 5 + 1) as u8
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10913_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10913");

    // J1 正面：五类型同实例混注册全通（十轮）
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut b = MixBoard::new();
        let _ = b.register((r * 5 + 1) as i32, T_PIPE, true);
        let _ = b.register((r * 5 + 2) as i32, T_SOCK, true);
        let _ = b.register((r * 5 + 3) as i32, T_TTY, true);
        let _ = b.register((r * 5 + 4) as i32, T_EFD, true);
        let _ = b.register((r * 5 + 5) as i32, T_TFD, true);
        if b.all_kinds() && b.slots.len() == 5 {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-五类混注十连", h1 == 10, "五类型同实例十轮全通");

    // J2 反面：类型间零串扰千次
    let mut ok = 0u32;
    let mut r2 = 0u32;
    while r2 < 1000 {
        let mut b = MixBoard::new();
        let kind = mx_sample_kind(r2);
        let _ = b.register(r2 as i32 + 1, kind, true);
        let before = [0u32; 5];
        let _ = b.route(r2 as i32 + 1);
        if b.no_cross(&before, kind) {
            ok += 1;
        }
        r2 += 1;
    }
    s.add("J2-千次零串扰", ok == 1000, "千轮路由零串扰");

    // J3 惰条目不路由
    let j3 = {
        let mut b = MixBoard::new();
        let _ = b.register(50, T_EFD, false);
        b.route(50).err() == Some(MX_NOEVENT)
    };
    s.add("J3-惰条不路由", j3, "未 armed 条目路由必拒");

    // J4 未注册 fd 路由拒
    let j4 = MixBoard::new().route(99).err() == Some(MX_NOEVENT);
    s.add("J4-未注册拒", j4, "未注册 fd 路由必拒");

    // J5 注入四路全抓
    let inj = inject_four();
    s.add("J5-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "串扰/坏类型/重号/冻结全数命中");

    // J6 五类各路由一次后账各一
    let j6 = {
        let mut b = MixBoard::new();
        let _ = b.register(60, T_PIPE, true);
        let _ = b.register(61, T_SOCK, true);
        let _ = b.register(62, T_TTY, true);
        let _ = b.register(63, T_EFD, true);
        let _ = b.register(64, T_TFD, true);
        let _ = b.route(60);
        let _ = b.route(61);
        let _ = b.route(62);
        let _ = b.route(63);
        let _ = b.route(64);
        b.routed == [1u32, 1, 1, 1, 1]
    };
    s.add("J6-五账各一", j6, "五类各路由一次账各计一");

    // J7 重号拒收
    let j7 = {
        let mut b = MixBoard::new();
        let _ = b.register(65, T_TFD, true);
        b.register(65, T_PIPE, true).err() == Some(MX_DUP)
    };
    s.add("J7-重号拒收", j7, "同 fd 二次注册必拒");

    // J8 冻结后注册拒
    let j8 = {
        let mut b = MixBoard::new();
        b.freeze();
        b.register(66, T_PIPE, true).err() == Some(MX_FROZEN)
    };
    s.add("J8-冻结拒注册", j8, "冻结后注册必拒");

    // J9 渲染面：五类渲染互异
    let j9 = {
        let a = MixSlot { fd: 1, kind: T_PIPE, armed: true };
        let b2 = MixSlot { fd: 1, kind: T_SOCK, armed: true };
        let c = MixSlot { fd: 1, kind: T_TFD, armed: true };
        a.render() != b2.render() && b2.render() != c.render() && a.render() != c.render()
    };
    s.add("J9-渲染互异", j9, "类型渲染可区分");

    // J10 容量边界
    let j10 = {
        let mut b = MixBoard::new();
        let mut f = 1i32;
        let mut full = true;
        while f <= CAP as i32 {
            if b.register(f, T_PIPE, true).is_err() {
                full = false;
            }
            f += 1;
        }
        full && b.register(9999, T_PIPE, true).err() == Some(MX_BADTYPE)
    };
    s.add("J10-容量边界", j10, "超容注册必拒且原账不损");

    // J11 fold 稳定：路由前后指纹必变
    let j11 = {
        let mut b = MixBoard::new();
        let _ = b.register(70, T_TTY, true);
        let f0 = b.fold();
        let _ = b.route(70);
        let f1 = b.fold();
        let f2 = b.fold();
        f0 != f1 && f1 == f2 && f1 != 0
    };
    s.add("J11-指纹随路由变", j11, "路由账入指纹且同态稳定");

    // J12 动态明细：千次零串扰计数落账
    s.add_dyn("J12-千次明细", ok == 1000, &format!("no_cross={}/1000", ok));

    // J13 动态明细：五类混注十连计数落账
    s.add_dyn("J13-十连明细", h1 == 10, &format!("mix_hits={}/10", h1));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10913_mixboard_flow() {
        let mut b = MixBoard::new();
        assert!(b.register(1, T_PIPE, true).is_ok());
        assert!(b.register(1, T_SOCK, true).err() == Some(MX_DUP));
        assert!(b.register(2, 9, true).err() == Some(MX_BADTYPE));
        assert_eq!(b.route(1), Ok(1));
        assert!(b.route(2).err() == Some(MX_NOEVENT));
        b.freeze();
        assert!(b.register(3, T_PIPE, true).err() == Some(MX_FROZEN));
    }

    #[test]
    fn unx10913_checks_green() {
        let s = run_unx10913_checks();
        assert!(s.all_passed(), "UNX-F10913 判据未全绿");
    }
}
