//! UNX-F10901 epoll_wait 错误矩阵（EBADF/EFAULT/ENOMEM）（0x1d020 段）
//!
//! 锚点：UNX-F10901-J1 正面：三错误各 10/10；反面：合法调用零误报
//! （千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·epoll_wait 面）。
//!
//! 结构：兴趣门（fd 存活表 + 用户缓冲有效位 + 内存余量账）+ 三错误
//! 裁断器（坏 fd→EBADF、坏缓冲→EFAULT、内存不足→ENOMEM，序先 EBADF）
//! + 注入核（坏 fd/坏缓冲/内存枯竭/零事件四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d020 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d020;
pub const EW_EBADF: u32 = SEG_BASE + 0x01; // fd 不在兴趣表或已死
pub const EW_EFAULT: u32 = SEG_BASE + 0x02; // 用户缓冲区无效
pub const EW_ENOMEM: u32 = SEG_BASE + 0x03; // 内核内存不足
pub const EW_EINVAL: u32 = SEG_BASE + 0x04; // maxevents 非正
pub const EW_NOEVENT: u32 = SEG_BASE + 0x05; // 就绪事件数为零
pub const EW_CLOSED: u32 = SEG_BASE + 0x06; // 门已封闭

/// 兴趣门定容。
pub const CAP: usize = 16;
/// 单次就绪预算（确定性就绪数）。
pub const READY_BUDGET: usize = 4;
/// B26 epoll_wait 域标（fold 种子）。
pub const EW_TAG: u64 = 0x1d02e01;

/// 单条 fd 兴趣项。
#[derive(Clone, Debug)]
pub struct FdInterest {
    pub fd: i32,
    pub live: bool,
    pub armed: bool,
}

impl FdInterest {
    /// 就绪位：armed 且存活的条目视为就绪。
    pub fn ready(&self) -> bool {
        self.live && self.armed
    }

    /// 渲染：存活/armed 双旗。
    pub fn render(&self) -> String {
        let lv = if self.live { "L" } else { "D" };
        let ar = if self.armed { "A" } else { "I" };
        format!("F{}|{}{}", self.fd, lv, ar)
    }
}

/// epoll_wait 错误门：查 fd→查缓冲→查内存→数就绪。
pub struct WaitGate {
    pub interest: Vec<FdInterest>,
    pub buf_live: bool,
    pub mem_pages: u32,
    pub closed: bool,
}

impl WaitGate {
    pub fn new() -> WaitGate {
        WaitGate { interest: Vec::new(), buf_live: true, mem_pages: 64, closed: false }
    }

    /// 登记兴趣 fd：封门、重号、超容均拒。
    pub fn arm(&mut self, fd: i32, armed: bool) -> Result<(), u32> {
        if self.closed {
            return Err(EW_CLOSED);
        }
        if self.interest.iter().any(|e| e.fd == fd) {
            return Err(EW_EBADF);
        }
        if self.interest.len() >= CAP {
            return Err(EW_ENOMEM);
        }
        self.interest.push(FdInterest { fd, live: true, armed });
        Ok(())
    }

    /// 杀 fd：标死不删（后续 wait 命中 EBADF）。
    pub fn kill(&mut self, fd: i32) -> Result<(), u32> {
        let mut hit = false;
        let mut k = 0usize;
        while k < self.interest.len() {
            if self.interest[k].fd == fd {
                self.interest[k].live = false;
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(())
        } else {
            Err(EW_EBADF)
        }
    }

    /// 就绪计数（不消耗，供反面对照）。
    pub fn ready_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.interest.len() {
            if self.interest[k].ready() {
                n += 1;
            }
            k += 1;
        }
        n.min(READY_BUDGET)
    }

    /// wait 主裁断：错误序 EBADF → EFAULT → ENOMEM → 零事件。
    pub fn wait(&self, fd: i32, maxevents: u32, buf_pages: u32) -> Result<usize, u32> {
        let mut known = false;
        let mut alive = false;
        let mut k = 0usize;
        while k < self.interest.len() {
            if self.interest[k].fd == fd {
                known = true;
                alive = self.interest[k].live;
            }
            k += 1;
        }
        if !known || !alive {
            return Err(EW_EBADF);
        }
        if !self.buf_live {
            return Err(EW_EFAULT);
        }
        if maxevents == 0 {
            return Err(EW_EINVAL);
        }
        let need = (maxevents as u32 / 2).max(1);
        if self.mem_pages < need {
            return Err(EW_ENOMEM);
        }
        let n = self.ready_count();
        if n == 0 {
            return Err(EW_NOEVENT);
        }
        Ok(n.min(maxevents as usize))
    }

    /// 链式 fold 指纹（序敏感，含存活/armed 位与内存账）。
    pub fn fold(&self) -> u64 {
        let mut h = EW_TAG;
        let mut k = 0usize;
        while k < self.interest.len() {
            let e = &self.interest[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.live as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.armed as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h = (h ^ self.buf_live as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.mem_pages as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 封门：落指纹根，封后不得再改表。
    pub fn freeze(&mut self) -> u64 {
        self.closed = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 坏 fd（未登记）必报 EBADF
    let g = WaitGate::new();
    got[0] = g.wait(999, 8, 4).err() == Some(EW_EBADF);
    // 2) 坏缓冲必报 EFAULT（且 EBADF 优先序不乱）
    let mut b = WaitGate::new();
    let _ = b.arm(3, true);
    b.buf_live = false;
    got[1] = b.wait(3, 8, 4).err() == Some(EW_EFAULT);
    // 3) 内存枯竭必报 ENOMEM
    let mut m = WaitGate::new();
    let _ = m.arm(4, true);
    m.mem_pages = 0;
    got[2] = m.wait(4, 8, 4).err() == Some(EW_ENOMEM);
    // 4) 零就绪必报 NOEVENT，不误报成功
    let z = WaitGate::new();
    let mut q = WaitGate::new();
    let _ = q.arm(5, false);
    got[3] = z.wait(5, 8, 4).err() == Some(EW_EBADF) && q.wait(5, 8, 4).err() == Some(EW_NOEVENT);
    got
}

/// 就绪面确定性样本：第 r 轮的 armed 位。
pub fn ew_sample_armed(fd: i32, round: u32) -> bool {
    (fd as u32).wrapping_add(round).wrapping_mul(2654435761) % 7 != 0
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10901_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10901");

    // J1 正面·EBADF 10/10：杀 fd 与未登记 fd 全数命中
    let mut hits = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut g = WaitGate::new();
        let _ = g.arm(r as i32 + 10, true);
        let _ = g.kill(r as i32 + 10);
        if g.wait(r as i32 + 10, 8, 4).err() == Some(EW_EBADF) {
            hits += 1;
        }
        r += 1;
    }
    s.add("J1-EBADF十连", hits == 10, "坏 fd 十轮全数报 EBADF");

    // J2 正面·EFAULT 10/10
    let mut hits2 = 0u32;
    let mut r2 = 0u32;
    while r2 < 10 {
        let mut g = WaitGate::new();
        let _ = g.arm(r2 as i32 + 20, true);
        g.buf_live = false;
        if g.wait(r2 as i32 + 20, 8, 4).err() == Some(EW_EFAULT) {
            hits2 += 1;
        }
        r2 += 1;
    }
    s.add("J2-EFAULT十连", hits2 == 10, "坏缓冲十轮全数报 EFAULT");

    // J3 正面·ENOMEM 10/10
    // 需求页数 = maxevents/2 = 4；轮内余量必须落在 [0, need) 才算“不足”，
    // 故取 r % need 而非 r（r≥4 时余量已够用，会落到零就绪分支）。
    let mut hits3 = 0u32;
    let mut r3 = 0u32;
    while r3 < 10 {
        let mut g = WaitGate::new();
        let _ = g.arm(r3 as i32 + 30, true);
        g.mem_pages = r3 % ((8 / 2).max(1));
        if g.wait(r3 as i32 + 30, 8, 4).err() == Some(EW_ENOMEM) {
            hits3 += 1;
        }
        r3 += 1;
    }
    s.add("J3-ENOMEM十连", hits3 == 10, "内存不足十轮全数报 ENOMEM");

    // J4 反面：合法调用千次零误报
    // 合法性由样本决定武装位：热轮必得就绪数，冷轮必得 NOEVENT（零就绪不是误报），
    // 千轮逐轮判类正确即零误报。
    let mut ok = 0u32;
    let mut r4 = 0u32;
    while r4 < 1000 {
        let mut g = WaitGate::new();
        let fd = (r4 % 9) as i32 + 1;
        let hot = ew_sample_armed(fd, r4);
        let _ = g.arm(fd, hot);
        let res = g.wait(fd, 8, 4);
        let good = if hot {
            res.is_ok()
        } else {
            res.err() == Some(EW_NOEVENT)
        };
        if good {
            ok += 1;
        }
        r4 += 1;
    }
    s.add("J4-合法千次零误报", ok == 1000, "合法 wait 千轮零误报");

    // J5 错误优先序：EBADF 先于 EFAULT
    let j5 = {
        let mut g = WaitGate::new();
        g.buf_live = false;
        g.wait(7, 8, 4).err() == Some(EW_EBADF)
    };
    s.add("J5-优先序EBADF先", j5, "坏 fd 优先于坏缓冲裁断");

    // J6 错误优先序：EFAULT 先于 ENOMEM
    let j6 = {
        let mut g = WaitGate::new();
        let _ = g.arm(8, true);
        g.buf_live = false;
        g.mem_pages = 0;
        g.wait(8, 8, 4).err() == Some(EW_EFAULT)
    };
    s.add("J6-优先序EFAULT先", j6, "坏缓冲优先于内存枯竭裁断");

    // J7 maxevents 零拒收
    let j7 = {
        let mut g = WaitGate::new();
        let _ = g.arm(9, true);
        g.wait(9, 0, 4).err() == Some(EW_EINVAL)
    };
    s.add("J7-零事件数拒收", j7, "maxevents 为零必报 EINVAL");

    // J8 就绪数上限：READY_BUDGET 封顶
    let j8 = {
        let mut g = WaitGate::new();
        let mut f = 1i32;
        while f <= 10 {
            let _ = g.arm(f, true);
            f += 1;
        }
        g.wait(1, 8, 4) == Ok(READY_BUDGET)
    };
    s.add("J8-就绪预算封顶", j8, "就绪数不超过 READY_BUDGET");

    // J9 注入四路全抓
    let inj = inject_four();
    s.add("J9-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "坏fd/坏缓冲/枯竭/零就绪全数命中");

    // J10 杀后登记重号拒：死 fd 不许复活同号
    let j10 = {
        let mut g = WaitGate::new();
        let _ = g.arm(11, true);
        let _ = g.kill(11);
        g.arm(11, false).err() == Some(EW_EBADF)
    };
    s.add("J10-死号不复活", j10, "已杀 fd 同号登记必拒");

    // J11 封门拒改表
    let j11 = {
        let mut g = WaitGate::new();
        g.freeze();
        g.arm(12, true).err() == Some(EW_CLOSED)
    };
    s.add("J11-封门拒改表", j11, "冻结后登记必拒");

    // J11b 渲染面：存活/死、armed/惰四态可区分
    let j11b = {
        let a = FdInterest { fd: 1, live: true, armed: true };
        let d = FdInterest { fd: 1, live: false, armed: true };
        a.render() != d.render() && a.render().contains("LA") && d.render().contains("D")
    };
    s.add("J11b-渲染分旗", j11b, "四态渲染可区分");

    // J12 fold 稳定：同状态两次指纹一致且随状态变化
    let j12 = {
        let mut g = WaitGate::new();
        let _ = g.arm(13, true);
        let h1 = g.fold();
        let h2 = g.fold();
        let _ = g.kill(13);
        h1 == h2 && h1 != g.fold() && h1 != 0
    };
    s.add("J12-指纹稳定", j12, "同态指纹稳定且异态指纹必变");

    // J13 动态明细：合法千次计数落账
    s.add_dyn("J13-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J14 动态明细：就绪预算计数落账
    let rc = {
        let mut g = WaitGate::new();
        let mut f = 1i32;
        while f <= 10 {
            let _ = g.arm(f, true);
            f += 1;
        }
        g.ready_count()
    };
    s.add_dyn("J14-就绪明细", rc == READY_BUDGET, &format!("ready={}", rc));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10901_waitgate_flow() {
        let mut g = WaitGate::new();
        assert!(g.arm(3, true).is_ok());
        assert!(g.arm(3, true).err() == Some(EW_EBADF));
        assert_eq!(g.wait(3, 8, 4), Ok(1.min(READY_BUDGET)));
        let _ = g.kill(3);
        assert!(g.wait(3, 8, 4).err() == Some(EW_EBADF));
        g.buf_live = false;
        let _ = g.arm(4, true);
        assert!(g.wait(4, 8, 4).err() == Some(EW_EFAULT));
        g.mem_pages = 0;
        assert!(g.wait(4, 8, 4).err() == Some(EW_EFAULT));
        g.buf_live = true;
        assert!(g.wait(4, 8, 4).err() == Some(EW_ENOMEM));
        g.freeze();
        assert!(g.arm(5, true).err() == Some(EW_CLOSED));
    }

    #[test]
    fn unx10901_checks_green() {
        let s = run_unx10901_checks();
        assert!(s.all_passed(), "UNX-F10901 判据未全绿");
    }
}
