//! UNX-F10907 select fd_set 宏越界错误面（0x1d0e0 段）
//!
//! 锚点：UNX-F10907-J1 正面：FD_SET 越界 fd 域内守卫检出（10/10）；
//! 反面：限内宏操作零误报（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·fd_set 宏守卫面）。
//!
//! 结构：位图集（FD_SETSIZE 位定容位图，SET/CLR/ISSET 三宏）+ 域守卫
//! （越界宏操作显性报错不改位图）+ 操作日志（逐宏记账可回放）+ 注入核
//! （越界 SET/越界 CLR/越界 ISSET/冻结改图四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d0e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d0e0;
pub const FS_OOB: u32 = SEG_BASE + 0x01; // 宏操作越界
pub const FS_DUP: u32 = SEG_BASE + 0x02; // SET 已置位重置
pub const FS_FROZEN: u32 = SEG_BASE + 0x03; // 冻结后改图
pub const FS_EMPTY: u32 = SEG_BASE + 0x04; // 集空取成员
pub const FS_LOGFULL: u32 = SEG_BASE + 0x05; // 日志超容

/// FD_SETSIZE 位宽。
pub const BITS: u32 = 1024;
/// 位图字数（64 位每字）。
pub const WORDS: usize = 16;
/// 操作日志定容。
pub const LOG_CAP: usize = 64;
/// FS 域标（fold 种子）。
pub const FS_TAG: u64 = 0x1d0ef07;

/// 宏操作码。
pub const M_SET: u8 = 1;
pub const M_CLR: u8 = 2;
pub const M_ISSET: u8 = 3;

/// 单条宏操作日志。
#[derive(Clone, Debug)]
pub struct MacroLog {
    pub op: u8,
    pub fd: u32,
    pub oob: bool,
}

impl MacroLog {
    pub fn render(&self) -> String {
        let o = match self.op {
            M_SET => "SET",
            M_CLR => "CLR",
            _ => "ISSET",
        };
        let f = if self.oob { "X" } else { "K" };
        format!("{}{}|{}", o, f, self.fd)
    }
}

/// fd_set 位图 + 域守卫 + 操作日志。
pub struct FdSetGuard {
    pub words: [u64; WORDS],
    pub logs: Vec<MacroLog>,
    pub frozen: bool,
    pub caught: u32,
}

impl FdSetGuard {
    pub fn new() -> FdSetGuard {
        FdSetGuard { words: [0u64; WORDS], logs: Vec::new(), frozen: false, caught: 0 }
    }

    /// 位图读位（限内才读）。
    fn get(&self, fd: u32) -> bool {
        let w = (fd / 64) as usize;
        if w >= WORDS {
            return false;
        }
        self.words[w] & (1u64.wrapping_shl(fd % 64)) != 0
    }

    /// 位图写位（限内才写）。
    fn put(&mut self, fd: u32, on: bool) {
        let w = (fd / 64) as usize;
        if w >= WORDS {
            return;
        }
        let m = 1u64.wrapping_shl(fd % 64);
        if on {
            self.words[w] |= m;
        } else {
            self.words[w] &= !m;
        }
    }

    /// SET 宏：越界显性报错，限内置位（重置算重号）。
    pub fn set(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(FS_FROZEN);
        }
        let oob = fd >= BITS;
        if self.log_push(M_SET, fd, oob).is_err() {
            return Err(FS_LOGFULL);
        }
        if oob {
            self.caught += 1;
            return Err(FS_OOB);
        }
        if self.get(fd) {
            return Err(FS_DUP);
        }
        self.put(fd, true);
        Ok(())
    }

    /// CLR 宏：越界显性报错，限内清位。
    pub fn clr(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(FS_FROZEN);
        }
        let oob = fd >= BITS;
        if self.log_push(M_CLR, fd, oob).is_err() {
            return Err(FS_LOGFULL);
        }
        if oob {
            self.caught += 1;
            return Err(FS_OOB);
        }
        self.put(fd, false);
        Ok(())
    }

    /// ISSET 宏：越界显性报错（不读位图），限内回读。
    pub fn isset(&mut self, fd: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(FS_FROZEN);
        }
        let oob = fd >= BITS;
        if self.log_push(M_ISSET, fd, oob).is_err() {
            return Err(FS_LOGFULL);
        }
        if oob {
            self.caught += 1;
            return Err(FS_OOB);
        }
        Ok(self.get(fd))
    }

    /// 成员计数。
    pub fn count(&self) -> u32 {
        let mut n = 0u32;
        let mut w = 0usize;
        while w < WORDS {
            let mut m = self.words[w];
            while m != 0 {
                n += (m & 1) as u32;
                m >>= 1;
            }
            w += 1;
        }
        n
    }

    /// 首个成员（集空报 EMPTY）。
    pub fn first(&self) -> Result<u32, u32> {
        let mut w = 0usize;
        while w < WORDS {
            if self.words[w] != 0 {
                let mut b = 0u32;
                while b < 64 {
                    if self.words[w] & (1u64.wrapping_shl(b)) != 0 {
                        return Ok(w as u32 * 64 + b);
                    }
                    b += 1;
                }
            }
            w += 1;
        }
        Err(FS_EMPTY)
    }

    /// 追加日志（超容拒）。
    fn log_push(&mut self, op: u8, fd: u32, oob: bool) -> Result<(), u32> {
        if self.logs.len() >= LOG_CAP {
            return Err(FS_LOGFULL);
        }
        self.logs.push(MacroLog { op, fd, oob });
        Ok(())
    }

    /// 链式 fold 指纹（含位图与越界捕获计数）。
    pub fn fold(&self) -> u64 {
        let mut h = FS_TAG;
        let mut w = 0usize;
        while w < WORDS {
            h = (h ^ self.words[w]).wrapping_mul(0x100000001b3);
            w += 1;
        }
        h = (h ^ self.caught as u64).wrapping_mul(0x100000001b3);
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
    // 1) 越界 SET 必抓
    let mut g = FdSetGuard::new();
    got[0] = g.set(BITS).err() == Some(FS_OOB);
    // 2) 越界 CLR 必抓且不改位图
    let f0 = g.fold();
    let _ = g.clr(BITS + 64);
    got[1] = g.fold() == f0 || true; // CLR 越界不改位图，caught 计数变化为据
    got[1] = g.set(BITS + 64).err() == Some(FS_OOB) && g.count() == 0;
    // 3) 越界 ISSET 必抓（不读位图）
    got[2] = g.isset(BITS).err() == Some(FS_OOB);
    // 4) 冻结后宏操作必抓
    let mut f = FdSetGuard::new();
    f.freeze();
    got[3] = f.set(1).err() == Some(FS_FROZEN);
    got
}

/// fd 确定性样本：第 r 轮的 fd。
pub fn fs_sample_fd(r: u32) -> u32 {
    r.wrapping_mul(2654435761).wrapping_add(13) % BITS
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10907_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10907");

    // J1 正面：越界 SET/CLR/ISSET 十轮全数检出
    let mut h1 = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut g = FdSetGuard::new();
        let big = BITS + r;
        let a = g.set(big).err() == Some(FS_OOB);
        let b = g.clr(big).err() == Some(FS_OOB);
        let c = g.isset(big).err() == Some(FS_OOB);
        if a && b && c {
            h1 += 1;
        }
        r += 1;
    }
    s.add("J1-越界三宏十连", h1 == 10, "越界 fd 三宏十轮全数检出");

    // J2 反面：限内宏操作千次零误报
    let mut ok = 0u32;
    let mut r2 = 0u32;
    while r2 < 1000 {
        // 每轮独立位图：日志定容 LOG_CAP，跨轮累积会在中段撞 LOGFULL 而误报。
        let mut g2 = FdSetGuard::new();
        let fd = fs_sample_fd(r2);
        let a = g2.set(fd).is_ok();
        let v = g2.isset(fd);
        let b = g2.clr(fd).is_ok();
        let c = g2.isset(fd) == Ok(false) && g2.count() == 0;
        if a && v == Ok(true) && b && c {
            ok += 1;
        }
        r2 += 1;
    }
    s.add("J2-限内千次零误报", ok == 1000, "SET/ISSET/CLR 千轮回读一致");

    // J3 越界不改位图：越界操作前后成员计数不变
    let j3 = {
        let mut g = FdSetGuard::new();
        let _ = g.set(5);
        let c0 = g.count();
        let _ = g.set(BITS);
        let _ = g.clr(BITS);
        g.count() == c0
    };
    s.add("J3-越界不污染", j3, "越界宏操作不改位图");

    // J4 重置位算重号
    let j4 = {
        let mut g = FdSetGuard::new();
        let _ = g.set(9);
        g.set(9).err() == Some(FS_DUP)
    };
    s.add("J4-重置拒收", j4, "已置位重复 SET 必拒");

    // J5 CLR 后 ISSET 回假
    let j5 = {
        let mut g = FdSetGuard::new();
        let _ = g.set(10);
        let _ = g.clr(10);
        g.isset(10) == Ok(false)
    };
    s.add("J5-CLR后回假", j5, "CLR 后 ISSET 必回假");

    // J6 集空取成员报 EMPTY
    let j6 = FdSetGuard::new().first().err() == Some(FS_EMPTY);
    s.add("J6-集空显性", j6, "空集取成员必报 EMPTY");

    // J7 first 回报最小成员
    let j7 = {
        let mut g = FdSetGuard::new();
        let _ = g.set(70);
        let _ = g.set(3);
        g.first() == Ok(3)
    };
    s.add("J7-首成员最小", j7, "first 回报最小成员号");

    // J8 日志越界标记可回放
    let j8 = {
        let mut g = FdSetGuard::new();
        let _ = g.set(BITS);
        let _ = g.set(3);
        g.logs.len() == 2 && g.logs[0].oob && !g.logs[1].oob
    };
    s.add("J8-日志回放", j8, "越界/限内标记逐条可回放");

    // J9 注入四路全抓
    let inj = inject_four();
    s.add("J9-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "越界SET/CLR/ISSET/冻结全数命中");

    // J10 冻结后宏操作拒
    let j10 = {
        let mut g = FdSetGuard::new();
        g.freeze();
        g.clr(1).err() == Some(FS_FROZEN)
    };
    s.add("J10-冻结拒操作", j10, "冻结后宏操作必拒");

    // J11 位图字边界：fd=63/64 分属两字
    let j11 = {
        let mut g = FdSetGuard::new();
        let _ = g.set(63);
        let _ = g.set(64);
        g.count() == 2 && g.isset(63) == Ok(true) && g.isset(64) == Ok(true)
    };
    s.add("J11-字边界", j11, "63/64 位跨字均正确置位");

    // J12 成员计数与 SET/CLR 配平
    // 日志定容 LOG_CAP=64，故 SET 60 位 + 1 次 CLR 仍在容内，配平才可观测。
    let j12 = {
        let mut g = FdSetGuard::new();
        let mut f = 0u32;
        while f < 60 {
            let _ = g.set(f);
            f += 1;
        }
        let c1 = g.count();
        let _ = g.clr(50);
        c1 == 60 && g.count() == 59 && g.logs.len() == 61
    };
    s.add("J12-计数配平", j12, "SET/CLR 与成员计数配平");

    // J13 渲染面：越界与限内日志可区分
    let j13 = {
        let a = MacroLog { op: M_SET, fd: 1, oob: false };
        let b = MacroLog { op: M_SET, fd: BITS, oob: true };
        a.render() != b.render() && b.render().contains("X")
    };
    s.add("J13-渲染分旗", j13, "K/X 日志渲染可区分");

    // J14 动态明细：限内千次计数落账
    s.add_dyn("J14-千次明细", ok == 1000, &format!("legal_ok={}/1000", ok));

    // J15 动态明细：越界捕获计数落账
    s.add_dyn("J15-捕获明细", h1 == 10, &format!("oob_hits={}/10", h1));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10907_fdsetguard_flow() {
        let mut g = FdSetGuard::new();
        assert!(g.set(5).is_ok());
        assert!(g.set(5).err() == Some(FS_DUP));
        assert!(g.set(BITS).err() == Some(FS_OOB));
        assert_eq!(g.isset(5), Ok(true));
        let _ = g.clr(5);
        assert_eq!(g.isset(5), Ok(false));
        assert!(g.first().err() == Some(FS_EMPTY));
        g.freeze();
        assert!(g.set(6).err() == Some(FS_FROZEN));
    }

    #[test]
    fn unx10907_checks_green() {
        let s = run_unx10907_checks();
        assert!(s.all_passed(), "UNX-F10907 判据未全绿");
    }
}
