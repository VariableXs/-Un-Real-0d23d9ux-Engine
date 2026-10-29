//! UNX-F10902 epoll_ctl EPERM 矩阵（非 pollable fd）（0x1d040 段）
//!
//! 锚点：UNX-F10902-J1 正面：普通文件 fd 注册→EPERM（10/10）；反面：
//! 可 poll 类型零误拒（千次 10/10）。
//! 域：CoRun·UNX-B26（复用器错误矩阵域·epoll_ctl EPERM 面）。
//!
//! 结构：类型册（五类 pollable + 一类非 pollable 定容登记）+ 裁断器
//! （ADD/MOD/DEL 三操作对普通文件一律 EPERM，pollable 全通）+ 注入核
//! （文件挂监听/未登记操作/封册后改表/重号四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1d040 独占 0x20）。
pub const SEG_BASE: u32 = 0x1d040;
pub const CE_EPERM: u32 = SEG_BASE + 0x01; // 非 pollable 类型拒注册
pub const CE_EBADF: u32 = SEG_BASE + 0x02; // fd 未登记
pub const CE_DUP: u32 = SEG_BASE + 0x03; // fd 重号
pub const CE_FROZEN: u32 = SEG_BASE + 0x04; // 封册后改表
pub const CE_EINVAL: u32 = SEG_BASE + 0x05; // 事件掩码零
pub const CE_NOCLASS: u32 = SEG_BASE + 0x06; // 类型缺失

/// pollable 类型（1=pipe 2=socket 3=tty 4=eventfd 5=signalfd）。
pub const POLLABLE_PIPE: u8 = 1;
pub const POLLABLE_SOCK: u8 = 2;
pub const POLLABLE_TTY: u8 = 3;
pub const POLLABLE_EFD: u8 = 4;
pub const POLLABLE_SFD: u8 = 5;
/// 非 pollable：普通文件。
pub const PLAIN_FILE: u8 = 6;

/// 类型册定容。
pub const CAP: usize = 32;
/// CE 域标（fold 种子）。
pub const CE_TAG: u64 = 0x1d04c02;

/// epoll_ctl 操作码。
pub const OP_ADD: u8 = 1;
pub const OP_MOD: u8 = 2;
pub const OP_DEL: u8 = 3;

/// 单条类型登记项。
#[derive(Clone, Debug)]
pub struct ClassEntry {
    pub fd: i32,
    pub class: u8,
}

impl ClassEntry {
    /// pollable 判定：普通文件一律不可 poll。
    pub fn pollable(&self) -> bool {
        self.class >= POLLABLE_PIPE && self.class <= POLLABLE_SFD
    }

    pub fn render(&self) -> String {
        let flag = if self.pollable() { "P" } else { "F" };
        format!("C{}|{}|{}", self.fd, flag, self.class)
    }
}

/// epoll_ctl 类型裁断册：登记→按类裁断三操作。
pub struct CtlLedger {
    pub entries: Vec<ClassEntry>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CtlLedger {
    pub fn new() -> CtlLedger {
        CtlLedger { entries: Vec::new(), frozen: false, root: None }
    }

    /// 登记类型：封册、重号、超容、零类均拒。
    pub fn enroll(&mut self, fd: i32, class: u8) -> Result<(), u32> {
        if self.frozen {
            return Err(CE_FROZEN);
        }
        if class == 0 || class > PLAIN_FILE {
            return Err(CE_NOCLASS);
        }
        if self.entries.iter().any(|e| e.fd == fd) {
            return Err(CE_DUP);
        }
        if self.entries.len() >= CAP {
            return Err(CE_EBADF);
        }
        self.entries.push(ClassEntry { fd, class });
        Ok(())
    }

    /// 类查询。
    pub fn class_of(&self, fd: i32) -> Result<u8, u32> {
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].fd == fd {
                return Ok(self.entries[k].class);
            }
            k += 1;
        }
        Err(CE_EBADF)
    }

    /// ctl 主裁断：未登记 EBADF → 事件零 EINVAL → 非 pollable EPERM。
    pub fn ctl(&self, fd: i32, op: u8, events: u32) -> Result<(), u32> {
        let cls = self.class_of(fd)?;
        if op != OP_ADD && op != OP_MOD && op != OP_DEL {
            return Err(CE_EINVAL);
        }
        if op != OP_DEL && events == 0 {
            return Err(CE_EINVAL);
        }
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].fd == fd {
                if !self.entries[k].pollable() {
                    return Err(CE_EPERM);
                }
            }
            k += 1;
        }
        Ok(())
    }

    /// 链式 fold 指纹（序敏感，含类号）。
    pub fn fold(&self) -> u64 {
        let mut h = CE_TAG;
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            h = (h ^ e.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.class as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 封册：落指纹根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 普通文件挂监听必报 EPERM
    let mut l = CtlLedger::new();
    let _ = l.enroll(1, PLAIN_FILE);
    got[0] = l.ctl(1, OP_ADD, 0x1).err() == Some(CE_EPERM);
    // 2) 未登记 fd 操作必报 EBADF
    let g = CtlLedger::new();
    got[1] = g.ctl(99, OP_ADD, 0x1).err() == Some(CE_EBADF);
    // 3) 非 pollable 的 MOD/DEL 同样必拒（三操作全矩阵）
    let mut m = CtlLedger::new();
    let _ = m.enroll(2, PLAIN_FILE);
    got[2] = m.ctl(2, OP_MOD, 0x1).err() == Some(CE_EPERM) && m.ctl(2, OP_DEL, 0).err() == Some(CE_EPERM);
    // 4) 封册后登记必抓
    let mut d = CtlLedger::new();
    d.freeze();
    got[3] = d.enroll(3, POLLABLE_PIPE).err() == Some(CE_FROZEN);
    got
}

/// 类型确定性样本：奇 fd 为普通文件，偶 fd 为 pollable（按类轮转）。
pub fn ce_sample_class(fd: i32) -> u8 {
    if fd % 2 != 0 {
        PLAIN_FILE
    } else {
        let m = ((fd / 2) as u32).wrapping_mul(2654435761).wrapping_add(7) % 5;
        (m + 1) as u8
    }
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10902_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10902");

    // J1 正面：普通文件注册→EPERM 10/10
    let mut hits = 0u32;
    let mut r = 0u32;
    while r < 10 {
        let mut l = CtlLedger::new();
        let _ = l.enroll(r as i32 + 1, PLAIN_FILE);
        if l.ctl(r as i32 + 1, OP_ADD, 0x1).err() == Some(CE_EPERM) {
            hits += 1;
        }
        r += 1;
    }
    s.add("J1-文件EPERM十连", hits == 10, "普通文件注册十轮全数 EPERM");

    // J2 反面：五类 pollable 千次零误拒
    let mut ok = 0u32;
    let mut pollable_rounds = 0u32;
    let mut r2 = 0u32;
    while r2 < 1000 {
        let mut l = CtlLedger::new();
        // 抽样只取偶号 fd（样本约定偶号即 pollable），千轮全部落在 pollable 面；
        // 普通文件面由 J1/J5 覆盖。
        let fd = (r2 % 5) as i32 * 2 + 10;
        let cls = ce_sample_class(fd);
        let _ = l.enroll(fd, cls);
        if cls != PLAIN_FILE {
            pollable_rounds += 1;
            if l.ctl(fd, OP_ADD, 0x1).is_ok() {
                ok += 1;
            }
        }
        r2 += 1;
    }
    s.add("J2-pollable千次零误拒", ok == pollable_rounds && pollable_rounds >= 900, "pollable 类型千轮零误拒");

    // J3 五类 pollable 全通
    let j3 = {
        let mut l = CtlLedger::new();
        let mut f = 1i32;
        let mut pass = true;
        while f <= 5 {
            let cls = f as u8;
            if l.enroll(f, cls).is_err() || l.ctl(f, OP_ADD, 0x1).is_err() {
                pass = false;
            }
            f += 1;
        }
        pass
    };
    s.add("J3-五类全通", j3, "pipe/socket/tty/eventfd/signalfd 全数可注册");

    // J4 三操作矩阵：pollable ADD/MOD/DEL 全通
    let j4 = {
        let mut l = CtlLedger::new();
        let _ = l.enroll(20, POLLABLE_SOCK);
        l.ctl(20, OP_ADD, 0x1).is_ok() && l.ctl(20, OP_MOD, 0x3).is_ok() && l.ctl(20, OP_DEL, 0).is_ok()
    };
    s.add("J4-三操作全通", j4, "pollable 三操作全矩阵零误拒");

    // J5 注入四路全抓
    let inj = inject_four();
    s.add("J5-注入四路全抓", inj[0] && inj[1] && inj[2] && inj[3], "EPERM/EBADF/全矩阵/封册全数命中");

    // J6 未登记 fd 必报 EBADF 且优先于 EPERM 路径
    let j6 = CtlLedger::new().ctl(999, OP_ADD, 0x1).err() == Some(CE_EBADF);
    s.add("J6-未登记EBADF", j6, "未登记 fd 操作必报 EBADF");

    // J7 事件掩码零拒收（DEL 豁免）
    let j7 = {
        let mut l = CtlLedger::new();
        let _ = l.enroll(21, POLLABLE_PIPE);
        l.ctl(21, OP_ADD, 0).err() == Some(CE_EINVAL) && l.ctl(21, OP_DEL, 0).is_ok()
    };
    s.add("J7-零掩码拒收", j7, "ADD/MOD 零掩码必拒且 DEL 豁免");

    // J8 类号非法拒收
    let j8 = {
        let mut l = CtlLedger::new();
        l.enroll(22, 0).err() == Some(CE_NOCLASS) && l.enroll(22, 9).err() == Some(CE_NOCLASS)
    };
    s.add("J8-类号非法", j8, "类号零与越界必拒");

    // J9 重号拒收
    let j9 = {
        let mut l = CtlLedger::new();
        let _ = l.enroll(23, POLLABLE_TTY);
        l.enroll(23, POLLABLE_PIPE).err() == Some(CE_DUP)
    };
    s.add("J9-重号拒收", j9, "同 fd 二次登记必拒");

    // J10 封册后改表拒
    let j10 = {
        let mut l = CtlLedger::new();
        l.freeze();
        l.enroll(24, POLLABLE_EFD).err() == Some(CE_FROZEN)
    };
    s.add("J10-封册拒改", j10, "冻结后登记必拒");

    // J11 渲染面：pollable 与普通文件标记可区分
    let j11 = {
        let p = ClassEntry { fd: 1, class: POLLABLE_PIPE };
        let f = ClassEntry { fd: 2, class: PLAIN_FILE };
        p.render() != f.render() && p.render().contains("P") && f.render().contains("F")
    };
    s.add("J11-渲染分旗", j11, "P/F 类型渲染可区分");

    // J12 fold 稳定：同态一致、改类必变
    let j12 = {
        let mut l = CtlLedger::new();
        let _ = l.enroll(25, POLLABLE_SFD);
        let h1 = l.fold();
        let h2 = l.fold();
        let _ = l.enroll(26, POLLABLE_PIPE);
        h1 == h2 && h1 != l.fold() && h1 != 0
    };
    s.add("J12-指纹稳定", j12, "同态指纹稳定且异态指纹必变");

    // J13 动态明细：千次零误拒计数落账
    s.add_dyn("J13-千次明细", ok == pollable_rounds, &format!("legal_ok={}/{}", ok, pollable_rounds));

    // J14 动态明细：EPERM 十连计数落账
    s.add_dyn("J14-十连明细", hits == 10, &format!("eperm_hits={}/10", hits));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10902_ctlledger_flow() {
        let mut l = CtlLedger::new();
        assert!(l.enroll(1, PLAIN_FILE).is_ok());
        assert!(l.enroll(1, POLLABLE_PIPE).err() == Some(CE_DUP));
        assert!(l.enroll(2, POLLABLE_PIPE).is_ok());
        assert!(l.ctl(1, OP_ADD, 0x1).err() == Some(CE_EPERM));
        assert!(l.ctl(2, OP_ADD, 0x1).is_ok());
        assert!(l.ctl(2, OP_MOD, 0x1).is_ok());
        assert!(l.ctl(2, OP_DEL, 0).is_ok());
        assert!(l.ctl(99, OP_ADD, 0x1).err() == Some(CE_EBADF));
        l.freeze();
        assert!(l.enroll(3, POLLABLE_SOCK).err() == Some(CE_FROZEN));
    }

    #[test]
    fn unx10902_checks_green() {
        let s = run_unx10902_checks();
        assert!(s.all_passed(), "UNX-F10902 判据未全绿");
    }
}
