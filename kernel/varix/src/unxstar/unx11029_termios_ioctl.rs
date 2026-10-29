//! UNX-F11029 termios ioctl 挂点联签（挂号与行规程账对账）（0x1e3a0 段）
//!
//! 锚点：UNX-F11029-J1 ioctl 挂点千次对平（10/10），标志账零漂移（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·termios ioctl）。
//!
//! 结构：终端行规程账（fd + 四标志组 iflag/oflag/cflag/lflag + ioctl 计
//! 数）+ set/get 挂点（非法标志必拒、set 记账）+ 基线快照（set 定值后快
//! 照冻结）+ 零漂移断言（重放 set 后与基线逐组比对）+ fold 与冻结 + 注
//! 入核（非法标志/未注册 fd/重号 fd/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e3a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e3a0;
pub const TI_BADFLAG: u32 = SEG_BASE + 0x01; // 非法标志
pub const TI_NOFD: u32 = SEG_BASE + 0x02; // fd 未注册
pub const TI_DUPFD: u32 = SEG_BASE + 0x03; // fd 重号
pub const TI_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const TI_CAP: u32 = SEG_BASE + 0x05; // 终端表超容
pub const TI_ZSET: u32 = SEG_BASE + 0x06; // 零参 set

/// 终端表定容。
pub const TT_CAP: usize = 12;
/// 千次 ioctl 轮数。
pub const TI_ROUNDS: u32 = 1000;
/// termios 账域标（fold 种子）。
pub const TTY_TAG: u64 = 0x1e3a_29;
/// 终端对平面规模（10/10）。
pub const TTS_NEEDED: usize = 10;
/// 标志组合法定域（四组各八位低位样例）。
pub const TI_LEGAL: u32 = 0x0000_0fff;

/// 单终端行规程项：fd + 四标志组 + ioctl 计数。
#[derive(Clone, Debug)]
pub struct TtyItem {
    pub fd: u32,
    pub iflag: u32,
    pub oflag: u32,
    pub cflag: u32,
    pub lflag: u32,
    pub sets: u32,
    pub gets: u32,
}

impl TtyItem {
    /// 四组标志逐组渲染。
    pub fn render(&self) -> String {
        format!(
            "termios|fd{}|i{:x}|o{:x}|c{:x}|l{:x}",
            self.fd, self.iflag, self.oflag, self.cflag, self.lflag
        )
    }

    /// 与基线逐组比对（零漂移面）。
    pub fn drift_free(&self, base: &TtyItem) -> bool {
        self.iflag == base.iflag
            && self.oflag == base.oflag
            && self.cflag == base.cflag
            && self.lflag == base.lflag
    }
}

/// termios 挂号账：注册 → ioctl set/get → 基线快照 → 零漂移封账。
pub struct TermBook {
    pub ttys: Vec<TtyItem>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl TermBook {
    pub fn new() -> TermBook {
        TermBook { ttys: Vec::new(), frozen: false, root: None }
    }

    /// 注册终端 fd：冻结、重号、超容必拒。
    pub fn enroll(&mut self, fd: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TI_FROZEN);
        }
        if self.ttys.iter().any(|x| x.fd == fd) {
            return Err(TI_DUPFD);
        }
        if self.ttys.len() >= TT_CAP {
            return Err(TI_CAP);
        }
        self.ttys.push(TtyItem {
            fd,
            iflag: 0,
            oflag: 0,
            cflag: 0,
            lflag: 0,
            sets: 0,
            gets: 0,
        });
        Ok(())
    }

    /// ioctl set 挂点：非法标志与零参必拒；写入并计数。
    pub fn tcset(&mut self, fd: u32, which: usize, val: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(TI_FROZEN);
        }
        if val == 0 {
            return Err(TI_ZSET);
        }
        if val & !TI_LEGAL != 0 {
            return Err(TI_BADFLAG);
        }
        let mut k = 0usize;
        while k < self.ttys.len() {
            if self.ttys[k].fd == fd {
                match which {
                    0 => self.ttys[k].iflag = val,
                    1 => self.ttys[k].oflag = val,
                    2 => self.ttys[k].cflag = val,
                    3 => self.ttys[k].lflag = val,
                    _ => return Err(TI_BADFLAG),
                }
                self.ttys[k].sets = self.ttys[k].sets.wrapping_add(1);
                return Ok(());
            }
            k += 1;
        }
        Err(TI_NOFD)
    }

    /// ioctl get 挂点：读回指定标志组。
    pub fn tcget(&mut self, fd: u32, which: usize) -> Result<u32, u32> {
        if self.frozen {
            return Err(TI_FROZEN);
        }
        let mut k = 0usize;
        while k < self.ttys.len() {
            if self.ttys[k].fd == fd {
                self.ttys[k].gets = self.ttys[k].gets.wrapping_add(1);
                return Ok(match which {
                    0 => self.ttys[k].iflag,
                    1 => self.ttys[k].oflag,
                    2 => self.ttys[k].cflag,
                    _ => self.ttys[k].lflag,
                });
            }
            k += 1;
        }
        Err(TI_NOFD)
    }

    /// 按 fd 查位次。
    pub fn find(&self, fd: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.ttys.len() {
            if self.ttys[k].fd == fd {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = TTY_TAG;
        let mut k = 0usize;
        while k < self.ttys.len() {
            let x = &self.ttys[k];
            h = (h ^ x.fd as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.iflag as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.oflag as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.cflag as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.lflag as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.sets as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.gets as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
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
pub fn ti_fd(no: usize) -> u32 {
    500 + no as u32
}

/// 标志值确定性样本（各轮循环取四组）。
pub fn ti_flag(r: u32) -> u32 {
    0x001 << (r % 8)
}

/// 千次对账全链：十终端注册；千轮在轮转终端上 set 四组 + get 回读；
/// 终值与最后 set 值逐组比对（零漂移）。
pub fn build_termbook() -> (TermBook, Vec<TtyItem>, bool) {
    let mut b = TermBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < TTS_NEEDED {
        if b.enroll(ti_fd(i)).is_err() {
            ok = false;
        }
        i += 1;
    }
    // 每终端最后写入的标志组基线（镜像账）。
    let mut baselines: Vec<TtyItem> = Vec::new();
    let mut i = 0usize;
    while i < TTS_NEEDED {
        baselines.push(TtyItem {
            fd: ti_fd(i),
            iflag: 0,
            oflag: 0,
            cflag: 0,
            lflag: 0,
            sets: 0,
            gets: 0,
        });
        i += 1;
    }
    let mut r = 0u32;
    while r < TI_ROUNDS {
        let k = (r % TTS_NEEDED as u32) as usize;
        let fd = ti_fd(k);
        let v = ti_flag(r);
        let which = (r % 4) as usize;
        if b.tcset(fd, which, v).is_err() {
            ok = false;
        }
        match which {
            0 => baselines[k].iflag = v,
            1 => baselines[k].oflag = v,
            2 => baselines[k].cflag = v,
            _ => baselines[k].lflag = v,
        }
        if b.tcget(fd, which).is_err() {
            ok = false;
        }
        r += 1;
    }
    // 零漂移断言：账面与镜像基线逐组比对。
    let mut i = 0usize;
    while i < TTS_NEEDED {
        let pos = b.find(ti_fd(i));
        match pos {
            Some(k) => {
                if !b.ttys[k].drift_free(&baselines[i]) {
                    ok = false;
                }
            }
            None => ok = false,
        }
        i += 1;
    }
    (b, baselines, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 非法标志必拒
    let mut a = TermBook::new();
    let _ = a.enroll(1);
    got[0] = a.tcset(1, 0, 0x8000).err() == Some(TI_BADFLAG);
    // 2) 未注册 fd 必拒
    let mut b = TermBook::new();
    got[1] = b.tcset(0x999, 0, 1).err() == Some(TI_NOFD)
        && b.tcget(0x999, 0).err() == Some(TI_NOFD);
    // 3) 重号与零参必拒
    let mut c = TermBook::new();
    let _ = c.enroll(7);
    got[2] = c.enroll(7).err() == Some(TI_DUPFD) && c.tcset(7, 0, 0).err() == Some(TI_ZSET);
    // 4) 冻结后写账必抓
    let (mut d, _, _) = build_termbook();
    d.freeze();
    got[3] = d.tcset(ti_fd(0), 0, 1).err() == Some(TI_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11029_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11029");

    // J1 ioctl 挂点千次对平：十终端千轮 set+get
    let (mut book, bases, green) = build_termbook();
    s.add("J1-千次对平", green && book.ttys.len() == 10, "十终端千轮 set+get 对平");

    // J2 标志账零漂移：账面与镜像基线逐组一致
    let mut drift_ok = true;
    let mut i = 0usize;
    while i < book.ttys.len() && i < bases.len() {
        if !book.ttys[i].drift_free(&bases[i]) {
            drift_ok = false;
        }
        i += 1;
    }
    s.add("J2-标志零漂移", drift_ok, "四标志组逐组与基线一致");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.tcset(ti_fd(0), 0, 1).err() == Some(TI_FROZEN)
        && book.tcget(ti_fd(0), 0).err() == Some(TI_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后 set 与 get 均必拒");

    // J5 注入检出：非法标志 + 未注册 fd
    let inj = inject_four();
    s.add("J5-非法未注抓", inj[0] && inj[1], "非法标志与未注册 fd 两路必抓");

    // J6 注入检出：重号零参 + 冻结后写
    s.add("J6-重号零参检出", inj[2] && inj[3], "重号与零参及冻结后写必抓");

    // J7 get 回读语义：set 后 get 即回同值
    let j7 = {
        let mut c = TermBook::new();
        let _ = c.enroll(0x21);
        let _ = c.tcset(0x21, 2, 0x040);
        c.tcget(0x21, 2) == Ok(0x040)
    };
    s.add("J7-get 回读", j7, "set 后 get 回同值");

    // J8 组隔离：改一组不损他组
    let j8 = {
        let mut c = TermBook::new();
        let _ = c.enroll(0x22);
        let _ = c.tcset(0x22, 0, 0x001);
        let _ = c.tcset(0x22, 3, 0x008);
        let i0 = c.tcget(0x22, 0);
        let l3 = c.tcget(0x22, 3);
        i0 == Ok(0x001) && l3 == Ok(0x008)
    };
    s.add("J8-组隔离", j8, "四组标志互不干扰");

    // J9 法定域掩码：TI_LEGAL 内值可写入
    let j9 = {
        let mut c = TermBook::new();
        let _ = c.enroll(0x23);
        let mut all_ok = true;
        let mut v = 1u32;
        while v <= 0x800 {
            if c.tcset(0x23, 1, v).is_err() {
                all_ok = false;
            }
            v = v.wrapping_mul(2);
        }
        all_ok
    };
    s.add("J9-法定域写入", j9, "法定域内逐位可写");

    // J10 计数均衡：各终端 set 计数均衡
    let j10 = {
        let per = TI_ROUNDS / TTS_NEEDED as u32;
        let mut even = true;
        let mut k = 0usize;
        while k < book.ttys.len() {
            if book.ttys[k].sets != per || book.ttys[k].gets != per {
                even = false;
            }
            k += 1;
        }
        even && per == 100
    };
    s.add("J10-计数均衡", j10, "十终端各百次 set+get");

    // J11 超容拒：第 TT_CAP+1 终端必拒
    let j11 = {
        let mut c = TermBook::new();
        let mut filled = true;
        let mut n = 0u32;
        while (n as usize) < TT_CAP {
            if c.enroll(0x1000 + n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(0x2000).err() == Some(TI_CAP)
    };
    s.add("J11-超容边界", j11, "超出定容必拒且原账不损");

    // J12 渲染面：终端渲染含 fd 与标志
    let j12 = {
        let x = TtyItem { fd: 0x2a, iflag: 0x42, oflag: 0, cflag: 0, lflag: 0, sets: 3, gets: 1 };
        let r = x.render();
        r.contains("fd42") && r.contains("i42")
    };
    s.add("J12-渲染明细", j12, "终端渲染含 fd 与标志组");

    // J13 样本确定性：fd 与标志同入同出
    let f2a = ti_fd(2);
    let f2b = ti_fd(2);
    let g5a = ti_flag(5);
    let g5b = ti_flag(5);
    s.add("J13-样本确定性", f2a == f2b && g5a == g5b, "fd 与标志样本同入同出");

    // J14 漂移注入：篡改镜像基线则漂移断言为假
    let j14 = {
        let mut bad = bases[0].clone();
        bad.iflag = bad.iflag.wrapping_add(0x100);
        !book.ttys[0].drift_free(&bad)
    };
    s.add("J14-漂移注入", j14, "篡改基线漂移断言为假");

    // J15 动态明细：千次对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-千次明细",
        fp == root && book.ttys.len() == 10,
        &format!("root={:#x} ttys=10", fp),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11029_termios_flow() {
        let (mut b, _, built) = build_termbook();
        assert!(built);
        assert!(b.tcset(ti_fd(0), 0, 0x8000).err() == Some(TI_BADFLAG));
        b.freeze();
        assert!(b.tcset(ti_fd(1), 1, 1).err() == Some(TI_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11029_checks_green() {
        let s = run_unx11029_checks();
        assert!(s.all_passed(), "UNX-F11029 判据未全绿");
    }
}
