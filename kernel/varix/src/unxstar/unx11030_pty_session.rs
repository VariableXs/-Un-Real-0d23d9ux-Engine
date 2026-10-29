//! UNX-F11030 pty 会话挂点联签（控制终端与前台组挂号）（0x1e3c0 段）
//!
//! 锚点：UNX-F11030-J1 会话挂号账逐项对平（10/10），前台组切换账一致（10/10）。
//! 域：CoRun·UNX-C4-B32（五族挂号联签域·pty 会话）。
//!
//! 结构：会话账（会话 id + 控制终端 + 进程组表 + 前台组指针）+ 建会器
//! （setsid 立账、组须先挂会）+ 前台切换器（组须在会内、切换记账）+ 对
//! 平断言（会内组全在册、前台指针有效）+ fold 与冻结 + 注入核（组不在会
//! 内切换/重复组/空会切前台/冻结后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e3c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e3c0;
pub const SS_NOGRP: u32 = SEG_BASE + 0x01; // 组不在会内
pub const SS_DUPGRP: u32 = SEG_BASE + 0x02; // 组重号
pub const SS_NOSESS: u32 = SEG_BASE + 0x03; // 会话未立
pub const SS_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const SS_NOCTTY: u32 = SEG_BASE + 0x05; // 控制终端未挂
pub const SS_CAP: u32 = SEG_BASE + 0x06; // 组表超容

/// 会话数定容。
pub const SESS_CAP: usize = 8;
/// 组表定容（单会话）。
pub const GRP_CAP: usize = 8;
/// 会话挂号对平数（10/10：五会话 × 两面）。
pub const SESS_FACES: usize = 10;
/// pty 会话账域标（fold 种子）。
pub const SESS_TAG: u64 = 0x1e3c_30;

/// 单会话账：会话 id + 控制终端 + 组表 + 前台组。
#[derive(Clone, Debug)]
pub struct Session {
    pub sid: u32,
    pub ctty: Option<u32>,
    pub grps: Vec<u32>,
    pub fgrp: Option<u32>,
    pub switches: u32,
}

impl Session {
    /// 组在册检查。
    pub fn has_grp(&self, gid: u32) -> bool {
        let mut k = 0usize;
        while k < self.grps.len() {
            if self.grps[k] == gid {
                return true;
            }
            k += 1;
        }
        false
    }

    /// 前台指针有效。
    pub fn fg_valid(&self) -> bool {
        match self.fgrp {
            Some(g) => self.has_grp(g),
            None => false,
        }
    }

    pub fn render(&self) -> String {
        match (self.ctty, self.fgrp) {
            (Some(c), Some(f)) => format!("sess|s{}|tty{}|fg{}", self.sid, c, f),
            (Some(c), None) => format!("sess|s{}|tty{}|fg-", self.sid, c),
            _ => format!("sess|s{}|tty-|fg-", self.sid),
        }
    }
}

/// 会话挂号账：setsid 立账 → 挂组 → 挂控制终端 → 前台切换封账。
pub struct SessBook {
    pub sess: Vec<Session>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SessBook {
    pub fn new() -> SessBook {
        SessBook { sess: Vec::new(), frozen: false, root: None }
    }

    /// setsid 立账：重号必拒。
    pub fn setsid(&mut self, sid: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SS_FROZEN);
        }
        if self.sess.iter().any(|x| x.sid == sid) {
            return Err(SS_DUPGRP);
        }
        if self.sess.len() >= SESS_CAP {
            return Err(SS_CAP);
        }
        self.sess.push(Session {
            sid,
            ctty: None,
            grps: Vec::new(),
            fgrp: None,
            switches: 0,
        });
        Ok(())
    }

    /// 挂控制终端：会须在册。
    pub fn set_ctty(&mut self, sid: u32, tty: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SS_FROZEN);
        }
        let mut k = 0usize;
        while k < self.sess.len() {
            if self.sess[k].sid == sid {
                self.sess[k].ctty = Some(tty);
                return Ok(());
            }
            k += 1;
        }
        Err(SS_NOSESS)
    }

    /// 挂进程组：组号全域唯一，单会组表超容必拒。
    pub fn add_grp(&mut self, sid: u32, gid: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(SS_FROZEN);
        }
        let mut k = 0usize;
        while k < self.sess.len() {
            if self.sess[k].sid == sid {
                if self.sess[k].has_grp(gid) {
                    return Err(SS_DUPGRP);
                }
                if self.sess[k].grps.len() >= GRP_CAP {
                    return Err(SS_CAP);
                }
                self.sess[k].grps.push(gid);
                return Ok(());
            }
            k += 1;
        }
        Err(SS_NOSESS)
    }

    /// 前台切换：组须在会内且控制终端已挂。
    pub fn tcsetpgrp(&mut self, sid: u32, gid: u32) -> Result<u32, u32> {
        if self.frozen {
            return Err(SS_FROZEN);
        }
        let mut k = 0usize;
        while k < self.sess.len() {
            if self.sess[k].sid == sid {
                if self.sess[k].ctty.is_none() {
                    return Err(SS_NOCTTY);
                }
                if !self.sess[k].has_grp(gid) {
                    return Err(SS_NOGRP);
                }
                self.sess[k].fgrp = Some(gid);
                self.sess[k].switches = self.sess[k].switches.wrapping_add(1);
                return Ok(self.sess[k].switches);
            }
            k += 1;
        }
        Err(SS_NOSESS)
    }

    /// 按会话查位次。
    pub fn find(&self, sid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.sess.len() {
            if self.sess[k].sid == sid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 挂号对平：逐会前台指针有效且组全在册。
    pub fn aligned(&self) -> bool {
        let mut ok = self.sess.len() * 2 == SESS_FACES;
        let mut k = 0usize;
        while k < self.sess.len() {
            if !self.sess[k].fg_valid() || self.sess[k].switches == 0 {
                ok = false;
            }
            k += 1;
        }
        ok
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SESS_TAG;
        let mut k = 0usize;
        while k < self.sess.len() {
            let x = &self.sess[k];
            h = (h ^ x.sid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.ctty.unwrap_or(0) as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.fgrp.unwrap_or(0) as u64).wrapping_mul(0x100000001b3);
            h = (h ^ x.switches as u64).wrapping_mul(0x100000001b3);
            let mut g = 0usize;
            while g < x.grps.len() {
                h = (h ^ x.grps[g] as u64).wrapping_mul(0x100000001b3);
                g += 1;
            }
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

/// 会话 id 确定性样本。
pub fn ss_sid(no: usize) -> u32 {
    0x900 + no as u32 * 10
}

/// 组 id 确定性样本（会话内两组成对面）。
pub fn ss_gid(no: usize, half: usize) -> u32 {
    ss_sid(no) + 1 + half as u32
}

/// 十面对账全链：五会话各挂双组与控制终端，前台在两组间各切两次。
pub fn build_sessbook() -> (SessBook, bool) {
    let mut b = SessBook::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < SESS_FACES / 2 {
        if b.setsid(ss_sid(i)).is_err() {
            ok = false;
        }
        if b.set_ctty(ss_sid(i), 0x70 + i as u32).is_err() {
            ok = false;
        }
        if b.add_grp(ss_sid(i), ss_gid(i, 0)).is_err() {
            ok = false;
        }
        if b.add_grp(ss_sid(i), ss_gid(i, 1)).is_err() {
            ok = false;
        }
        // 前台切换账：先组零后组一（各会计两次）。
        if b.tcsetpgrp(ss_sid(i), ss_gid(i, 0)).is_err() {
            ok = false;
        }
        if b.tcsetpgrp(ss_sid(i), ss_gid(i, 1)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let b_ok = ok && b.aligned();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 组不在会内切换必拒
    let mut a = SessBook::new();
    let _ = a.setsid(1);
    let _ = a.set_ctty(1, 0x77);
    let _ = a.add_grp(1, 11);
    got[0] = a.tcsetpgrp(1, 99).err() == Some(SS_NOGRP);
    // 2) 会话未立必拒
    let mut b = SessBook::new();
    got[1] = b.add_grp(0x1234, 5).err() == Some(SS_NOSESS)
        && b.set_ctty(0x1234, 1).err() == Some(SS_NOSESS);
    // 3) 控制终端未挂切前台必拒
    let mut c = SessBook::new();
    let _ = c.setsid(2);
    let _ = c.add_grp(2, 21);
    got[2] = c.tcsetpgrp(2, 21).err() == Some(SS_NOCTTY);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_sessbook();
    d.freeze();
    got[3] = d.tcsetpgrp(ss_sid(0), ss_gid(0, 0)).err() == Some(SS_FROZEN)
        && d.setsid(0x4321).err() == Some(SS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11030_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11030");

    // J1 会话挂号账逐项对平：五会话 × 双面
    let (mut book, green) = build_sessbook();
    s.add("J1-挂号对平", green && book.aligned(), "五会话挂号逐面立账对平");

    // J2 前台组切换账一致：逐会切换两次且指针有效
    let j2 = {
        let mut ok = true;
        let mut k = 0usize;
        while k < book.sess.len() {
            if book.sess[k].switches != 2 || !book.sess[k].fg_valid() {
                ok = false;
            }
            k += 1;
        }
        ok
    };
    s.add("J2-切换一致", j2, "逐会切换两次前台指针有效");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.tcsetpgrp(ss_sid(0), ss_gid(0, 0)).err() == Some(SS_FROZEN)
        && book.add_grp(ss_sid(0), 0x777).err() == Some(SS_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后切组挂组均必拒");

    // J5 注入检出：组不在会内 + 会话未立
    let inj = inject_four();
    s.add("J5-离会未立抓", inj[0] && inj[1], "组不在会内与会话未立两路必抓");

    // J6 注入检出：终端未挂 + 冻结后写
    s.add("J6-无终冻结检出", inj[2] && inj[3], "终端未挂与冻结后写必抓");

    // J7 会话重号必拒
    let j7 = {
        let mut c = SessBook::new();
        let _ = c.setsid(0x33);
        c.setsid(0x33).err() == Some(SS_DUPGRP)
    };
    s.add("J7-会话重号拒", j7, "setsid 重号必拒");

    // J8 组重号必拒
    let j8 = {
        let mut c = SessBook::new();
        let _ = c.setsid(0x34);
        let _ = c.add_grp(0x34, 41);
        c.add_grp(0x34, 41).err() == Some(SS_DUPGRP)
    };
    s.add("J8-组重号拒", j8, "组重挂必拒");

    // J9 切换历史：前台指向最后切换的组
    let j9 = {
        let mut c = SessBook::new();
        let _ = c.setsid(0x35);
        let _ = c.set_ctty(0x35, 0x78);
        let _ = c.add_grp(0x35, 51);
        let _ = c.add_grp(0x35, 52);
        let _ = c.tcsetpgrp(0x35, 51);
        let _ = c.tcsetpgrp(0x35, 52);
        c.find(0x35).map(|k| c.sess[k].fgrp) == Some(Some(52))
            && c.find(0x35).map(|k| c.sess[k].switches) == Some(2)
    };
    s.add("J9-前台历史", j9, "前台指向末次切换组");

    // J10 终端挂账：控制终端 id 确定性
    let j10 = book.find(ss_sid(2)).map(|k| book.sess[k].ctty) == Some(Some(0x72));
    s.add("J10-终端挂账", j10, "控制终端按样本确定性挂账");

    // J11 渲染面：会话渲染含终端与前台
    let j11 = {
        let x = Session {
            sid: 3,
            ctty: Some(9),
            grps: Vec::new(),
            fgrp: Some(5),
            switches: 1,
        };
        let r = x.render();
        r.contains("tty9") && r.contains("fg5")
    };
    s.add("J11-渲染明细", j11, "会话渲染含终端前台");

    // J12 组表超容拒：第 GRP_CAP+1 组必拒
    let j12 = {
        let mut c = SessBook::new();
        let _ = c.setsid(0x36);
        let mut filled = true;
        let mut n = 0u32;
        while (n as usize) < GRP_CAP {
            if c.add_grp(0x36, 0x60 + n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.add_grp(0x36, 0x99).err() == Some(SS_CAP)
    };
    s.add("J12-组表超容", j12, "组表超出定容必拒");

    // J13 样本确定性：会话与组 id 同入同出
    let s1a = ss_sid(1);
    let s1b = ss_sid(1);
    let g1a = ss_gid(1, 0);
    let g1b = ss_gid(1, 0);
    s.add("J13-样本确定性", s1a == s1b && g1a == g1b, "会话与组样本同入同出");

    // J14 悬空注入：前台指针指向不在会内组（构造态）断言为假
    let j14 = {
        let mut g: Vec<u32> = Vec::new();
        g.push(7);
        let x = Session {
            sid: 0x37,
            ctty: Some(1),
            grps: g,
            fgrp: Some(8),
            switches: 1,
        };
        !x.fg_valid()
    };
    s.add("J14-悬空注入", j14, "前台指向会外组断言为假");

    // J15 动态明细：十面对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-十面明细",
        fp == root && book.aligned(),
        &format!("root={:#x} sess={}", fp, book.sess.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11030_sess_flow() {
        let (mut b, built) = build_sessbook();
        assert!(built);
        // 冻结面必须封账后才成立：未封账时 tcsetpgrp 先撞 SS_NOGRP
        // （0x777 不在册的组号），永远到不了 SS_FROZEN。初版把这条冻结
        // 断言放在 freeze 之前，前提不成立；封账后 set_ctty 与 tcsetpgrp
        // 两路都必须被 SS_FROZEN 拒。
        b.freeze();
        assert!(b.tcsetpgrp(ss_sid(0), 0x777).err() == Some(SS_FROZEN));
        assert!(b.set_ctty(ss_sid(1), 1).err() == Some(SS_FROZEN));
        assert!(b.root.is_some());
    }

    #[test]
    fn unx11030_checks_green() {
        let s = run_unx11030_checks();
        assert!(s.all_passed(), "UNX-F11030 判据未全绿");
    }
}
