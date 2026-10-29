//! unx4413_clistat — UNX-F4413 CLOEXEC 统计账（诊断段 0xB760）。
//!
//! 判据原文：J1 三源统计闭集：O_CLOEXEC（open 时置位）/ F_SETFD
//!           （事后置位）/ dup3(…, O_CLOEXEC)（复制时置位）三源
//!           分账，cloexec_total = Σ三源，任何位变更必经其一；
//!           J2 存量账：cloexec_set = Σ置位 − Σ清除，与 fd 表实际
//!           置位数逐 fd 对账一致；
//!           J3 exec 消费账：exec_flush 记录「关闭数 + 存活数」，
//!           两者之和 = exec 时刻置位存量——一把清账；
//!           J4 双账守恒：events = set + clr + flush，且 bytes 类
//!           计数不掺入（本账纯位账）。
//! 设计纪律：
//!   1. Linux 语义：FD_CLOEXEC 是 fd 私有位，exec 时关闭置位 fd；
//!      三源写入口径不同但位账同源。
//!   2. 事件账：每笔 (来源, fd, 方向 set/clr) 入流水，统计与流水
//!      双向可核对。
//!   3. 快照：对账用（fd 表位向量 vs 统计推演）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0xB760 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB760;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_FD_BAD: u16 = SEG_BASE + 1;   // fd 越表
const E_BAL: u16 = SEG_BASE + 2;      // 统计账不守恒
const E_SRC_BAD: u16 = SEG_BASE + 3;  // 来源越闭集
const E_SNAP_DIFF: u16 = SEG_BASE + 4;// 快照对账失守
const E_FLUSH_TWICE: u16 = SEG_BASE + 5; // exec 双清账

/// CLOEXEC 三源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CxeSrc {
    OpenFlag = 1,  // O_CLOEXEC
    SetFd = 2,     // F_SETFD
    Dup3 = 3,      // dup3 复制时置位
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Set = 1,
    Clr = 2,
}

/// 流水事件。
#[derive(Clone, Copy)]
pub struct CxeEvent {
    pub src: u8,
    pub fd: u16,
    pub dir: u8, // 1=set 2=clr
    pub flushed: bool,
}

/// CLOEXEC 统计账枢纽。
pub struct CloStat {
    bits: Vec<bool>, // fd 表位向量（快照对账用）
    events: Vec<CxeEvent>,
    pub set_open: u32,
    pub set_setfd: u32,
    pub set_dup3: u32,
    pub clr_count: u32,
    pub flush_closed: u32,
    pub flush_survived: u32,
    flushed: bool,
}

impl CloStat {
    pub fn new(nfds: usize) -> CloStat {
        CloStat {
            bits: vec![false; nfds],
            events: Vec::new(),
            set_open: 0,
            set_setfd: 0,
            set_dup3: 0,
            clr_count: 0,
            flush_closed: 0,
            flush_survived: 0,
            flushed: false,
        }
    }

    /// 置位入口（判据 J1 三源分账）。
    pub fn mark(&mut self, src: CxeSrc, fd: usize) -> Result<(), u16> {
        if fd >= self.bits.len() {
            return Err(E_FD_BAD);
        }
        let s = match src {
            CxeSrc::OpenFlag => 1u8,
            CxeSrc::SetFd => 2,
            CxeSrc::Dup3 => 3,
        };
        if s == 0 {
            return Err(E_SRC_BAD);
        }
        // 同 fd 重复置位幂等：位不重复计，事件仍流水
        let already = self.bits[fd];
        self.bits[fd] = true;
        match src {
            CxeSrc::OpenFlag => {
                if !already {
                    self.set_open += 1;
                }
            }
            CxeSrc::SetFd => {
                if !already {
                    self.set_setfd += 1;
                }
            }
            CxeSrc::Dup3 => {
                if !already {
                    self.set_dup3 += 1;
                }
            }
        }
        self.events.push(CxeEvent {
            src: s,
            fd: fd as u16,
            dir: Direction::Set as u8,
            flushed: false,
        });
        Ok(())
    }

    /// 清除入口。
    pub fn unmark(&mut self, fd: usize) -> Result<(), u16> {
        if fd >= self.bits.len() {
            return Err(E_FD_BAD);
        }
        let was = self.bits[fd];
        self.bits[fd] = false;
        if was {
            self.clr_count += 1;
        }
        self.events.push(CxeEvent {
            src: 2, // clr 只经 F_SETFD 口径
            fd: fd as u16,
            dir: Direction::Clr as u8,
            flushed: false,
        });
        Ok(())
    }

    /// 置位存量（判据 J2 存量账推演值）。
    pub fn set_total(&self) -> u32 {
        self.set_open + self.set_setfd + self.set_dup3
    }

    /// fd 表实际置位数。
    pub fn bits_count(&self) -> u32 {
        let mut n = 0;
        for b in self.bits.iter() {
            if *b {
                n += 1;
            }
        }
        n
    }

    /// 存量对账（判据 J2）：推演值 − 清除数 == 实际置位数。
    pub fn stock_balanced(&self) -> bool {
        self.set_total() >= self.clr_count
            && self.set_total() - self.clr_count == self.bits_count()
    }

    /// exec 清账（判据 J3）：一把清置位 fd，记关闭/存活分账。
    pub fn exec_flush(&mut self) -> Result<(u32, u32), u16> {
        if self.flushed {
            return Err(E_FLUSH_TWICE);
        }
        let mut closed = 0u32;
        for (i, b) in self.bits.iter_mut().enumerate() {
            if *b {
                *b = false;
                // 流水补记 flush 事件
                self.events.push(CxeEvent {
                    src: 0, // flush 源
                    fd: i as u16,
                    dir: Direction::Clr as u8,
                    flushed: true,
                });
                closed += 1;
            }
        }
        self.flush_closed = closed;
        self.flush_survived = self.bits.len() as u32 - closed;
        self.flushed = true;
        Ok((closed, self.flush_survived))
    }

    /// 双账守恒（判据 J4）：events 数 = set 总数 + clr 数（flush 计入 clr 侧）。
    pub fn event_balanced(&self) -> bool {
        let mut sets = 0u32;
        let mut clrs = 0u32;
        for e in self.events.iter() {
            if e.dir == Direction::Set as u8 {
                sets += 1;
            } else {
                clrs += 1;
            }
        }
        sets == self.set_total() && clrs == self.clr_count + self.flush_closed
    }

    /// 流水长度。
    pub fn events_len(&self) -> usize {
        self.events.len()
    }
}

/// 自例程：R1 三源分账各一笔；R2 重复置位幂等；R3 清除与存量对账；
/// R4 exec 一把清账；R5 双账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三源各置一 fd（判据 J1）。
    let mut s = CloStat::new(8);
    s.mark(CxeSrc::OpenFlag, 0)?;
    s.mark(CxeSrc::SetFd, 1)?;
    s.mark(CxeSrc::Dup3, 2)?;
    if s.set_open != 1 || s.set_setfd != 1 || s.set_dup3 != 1 || s.set_total() != 3 {
        return Err(E_BAL);
    }
    if s.bits_count() != 3 || !s.stock_balanced() {
        return Err(E_SNAP_DIFF);
    }
    hits += 1;

    // R2：重复置位幂等——同源同 fd 再 mark，统计不涨。
    s.mark(CxeSrc::SetFd, 1)?;
    if s.set_setfd != 1 || s.bits_count() != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：清除一笔 + 存量对账（判据 J2）。
    s.unmark(1)?;
    if s.clr_count != 1 || s.bits_count() != 2 {
        return Err(E_BAL);
    }
    if !s.stock_balanced() {
        return Err(E_SNAP_DIFF);
    }
    hits += 1;

    // R4：exec 一把清（判据 J3）——关闭 2，存活 6；二次 flush 拒收。
    let (closed, survived) = s.exec_flush()?;
    if closed != 2 || survived != 6 {
        return Err(E_BAL);
    }
    if s.bits_count() != 0 {
        return Err(E_BAL);
    }
    if s.exec_flush() != Err(E_FLUSH_TWICE) {
        return Err(E_FLUSH_TWICE);
    }
    hits += 1;

    // R5：双账守恒终验（判据 J4）。
    if !s.event_balanced() {
        return Err(E_BAL);
    }
    // 流水闭合：3 set + 1 dup幂等set + 1 clr + 2 flush = 7 事件
    if s.events_len() != 7 {
        return Err(E_BAL);
    }
    // 越表拦截
    if s.mark(CxeSrc::OpenFlag, 99) != Err(E_FD_BAD) {
        return Err(E_FD_BAD);
    }
    if s.unmark(99) != Err(E_FD_BAD) {
        return Err(E_FD_BAD);
    }
    hits += 1;

    Ok(hits)
}
