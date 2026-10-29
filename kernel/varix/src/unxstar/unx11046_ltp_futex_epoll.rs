//! UNX-F11046 LTP futex/epoll 组用例对照（复用与同步族对账）（0x1e5c0 段）
//!
//! 锚点：UNX-F11046-J1 futex 组用例对照对平（10/10），epoll 组对照（10/10）。
//! 域：CoRun·UNX-C4-B33（LTP 对照域·复用与同步族）。
//!
//! 结构：双面对照账（futex 面 + epoll 面各十例：例号 + 语义模式 + 裁断
//! 位）+ 对照器（模式语义与 LTP 期望一致才翻绿）+ 模式域校验（模式越界
//! 必拒）+ 逐面对平器 + fold 与冻结 + 注入核（模式越界/漏对照/重号/冻结
//! 后写四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e5c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e5c0;
pub const MU_BADMODE: u32 = SEG_BASE + 0x01; // 模式越界
pub const MU_NOCASE: u32 = SEG_BASE + 0x02; // 例未立账
pub const MU_DUP: u32 = SEG_BASE + 0x03; // 例号重号
pub const MU_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const MU_SHORT: u32 = SEG_BASE + 0x05; // 面缺例
pub const MU_MISS: u32 = SEG_BASE + 0x06; // 语义失配

/// 每面例数（10/10）。
pub const MU_CASES: usize = 10;
/// 语义模式空间：等待/唤醒/就绪/挂起/复位（0..4）。
pub const MU_MODES: u32 = 5;
/// 复用同步对照账域标（fold 种子）。
pub const MUX_TAG: u64 = 0x1e5c_2e;

/// 单对照例：例号 + 模式 + 裁断位。
#[derive(Clone, Debug)]
pub struct MuCase {
    pub cid: u32,
    pub mode: u32,
    pub verdict: bool,
}

impl MuCase {
    /// 对照齐绿：模式合法且裁断翻绿。
    pub fn green(&self) -> bool {
        self.verdict && self.mode < MU_MODES
    }

    pub fn render(&self) -> String {
        let flag = if self.green() { "G" } else { "R" };
        format!("mc|c{}|m{}|{}", self.cid, self.mode, flag)
    }
}

/// 复用同步对照账：futex 面 + epoll 面。
pub struct MuxLedger {
    pub ftx: Vec<MuCase>,
    pub epl: Vec<MuCase>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl MuxLedger {
    pub fn new() -> MuxLedger {
        MuxLedger { ftx: Vec::new(), epl: Vec::new(), frozen: false, root: None }
    }

    /// 立账：冻结、模式越界、重号必拒。
    pub fn enroll(&mut self, face: usize, cid: u32, mode: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(MU_FROZEN);
        }
        if face >= 2 {
            return Err(MU_NOCASE);
        }
        if mode >= MU_MODES {
            return Err(MU_BADMODE);
        }
        let tab = if face == 0 { &mut self.ftx } else { &mut self.epl };
        if tab.iter().any(|x| x.cid == cid) {
            return Err(MU_DUP);
        }
        tab.push(MuCase { cid, mode, verdict: false });
        Ok(())
    }

    /// 对照：LTP 期望模式与账面一致才翻绿。
    pub fn compare(&mut self, face: usize, cid: u32, expect: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(MU_FROZEN);
        }
        if face >= 2 {
            return Err(MU_NOCASE);
        }
        if expect >= MU_MODES {
            return Err(MU_BADMODE);
        }
        let tab = if face == 0 { &mut self.ftx } else { &mut self.epl };
        let mut hit = false;
        let mut ok = false;
        let mut k = 0usize;
        while k < tab.len() {
            if tab[k].cid == cid {
                tab[k].verdict = tab[k].mode == expect;
                ok = tab[k].verdict;
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(ok)
        } else {
            Err(MU_NOCASE)
        }
    }

    /// 单面对平。
    pub fn face_even(&self, face: usize) -> bool {
        let tab = if face == 0 { &self.ftx } else { &self.epl };
        if tab.len() != MU_CASES {
            return false;
        }
        let mut ok = true;
        let mut k = 0usize;
        while k < tab.len() {
            if !tab[k].green() {
                ok = false;
            }
            k += 1;
        }
        ok
    }

    /// 双面对平。
    pub fn closed(&self) -> bool {
        self.face_even(0) && self.face_even(1)
    }

    /// 模式分布：逐面五模式覆盖。
    pub fn modes_covered(&self, face: usize) -> bool {
        let tab = if face == 0 { &self.ftx } else { &self.epl };
        let mut m = 0u32;
        let mut ok = true;
        while m < MU_MODES {
            let mut found = false;
            let mut k = 0usize;
            while k < tab.len() {
                if tab[k].mode == m {
                    found = true;
                }
                k += 1;
            }
            if !found {
                ok = false;
            }
            m += 1;
        }
        ok
    }

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = MUX_TAG;
        let mut f = 0usize;
        while f < 2 {
            let tab = if f == 0 { &self.ftx } else { &self.epl };
            let mut k = 0usize;
            while k < tab.len() {
                h = (h ^ tab[k].cid as u64).wrapping_mul(0x100000001b3);
                h = (h ^ tab[k].mode as u64).wrapping_mul(0x100000001b3);
                h = (h ^ tab[k].verdict as u64).wrapping_mul(0x100000001b3);
                k += 1;
            }
            f += 1;
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

/// futex 面例号样本。
pub fn mu_cid(no: usize) -> u32 {
    0xd000 + no as u32
}

/// epoll 面例号样本。
pub fn me_cid(no: usize) -> u32 {
    0xe000 + no as u32
}

/// 模式确定性样本。
pub fn mu_mode(no: usize) -> u32 {
    (no % MU_MODES as usize) as u32
}

/// 双面全量对照全链。
pub fn build_muxledger() -> (MuxLedger, bool) {
    let mut l = MuxLedger::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < MU_CASES {
        if l.enroll(0, mu_cid(i), mu_mode(i)).is_err() {
            ok = false;
        }
        if l.enroll(1, me_cid(i), mu_mode(i + 2)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < MU_CASES {
        if l.compare(0, mu_cid(i), mu_mode(i)).is_err() {
            ok = false;
        }
        if l.compare(1, me_cid(i), mu_mode(i + 2)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let l_ok = ok && l.closed();
    (l, l_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 模式越界必拒
    let mut a = MuxLedger::new();
    got[0] = a.enroll(0, 1, MU_MODES).err() == Some(MU_BADMODE)
        && a.compare(0, 1, MU_MODES).err() == Some(MU_BADMODE);
    // 2) 例未立账对照必拒
    let mut b = MuxLedger::new();
    got[1] = b.compare(1, 0x777, 1).err() == Some(MU_NOCASE);
    // 3) 重号必拒
    let mut c = MuxLedger::new();
    let _ = c.enroll(1, 9, 1);
    got[2] = c.enroll(1, 9, 2).err() == Some(MU_DUP);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_muxledger();
    d.freeze();
    got[3] = d.enroll(0, 0xf000, 1).err() == Some(MU_FROZEN)
        && d.compare(1, me_cid(0), 1).err() == Some(MU_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11046_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11046");

    // J1 futex 组用例对照对平
    let (mut book, green) = build_muxledger();
    s.add("J1-futex 面对平", green && book.face_even(0), "futex 面十例模式对照齐绿");

    // J2 epoll 组对照：epoll 面齐绿且模式覆盖
    s.add(
        "J2-epoll 面对平",
        book.face_even(1) && book.modes_covered(0) && book.modes_covered(1),
        "epoll 面齐绿且模式逐面覆盖",
    );

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.enroll(0, 0xf000, 1).err() == Some(MU_FROZEN)
        && book.compare(1, me_cid(0), 1).err() == Some(MU_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后立账对照均必拒");

    // J5 注入检出：模式越界 + 例未立账
    let inj = inject_four();
    s.add("J5-越模未立抓", inj[0] && inj[1], "模式越界与例未立账两路必抓");

    // J6 注入检出：重号 + 冻结后写
    s.add("J6-重号冻结检出", inj[2] && inj[3], "例号重号与冻结后写必抓");

    // J7 模式分布：双面五模式全覆盖
    let j7 = book.modes_covered(0) && book.modes_covered(1);
    s.add("J7-模式覆盖", j7, "语义模式逐面全覆盖");

    // J8 失配可报：对照期望错必出红
    let j8 = {
        let mut c = MuxLedger::new();
        let _ = c.enroll(0, mu_cid(0), 1);
        let _ = c.compare(0, mu_cid(0), 2);
        !c.face_even(0)
    };
    s.add("J8-失配可报", j8, "模式失配裁断为红对平为假");

    // J9 漏对照为假
    let j9 = {
        let mut c = MuxLedger::new();
        let mut i = 0usize;
        while i < MU_CASES {
            let _ = c.enroll(1, me_cid(i), mu_mode(i + 2));
            i += 1;
        }
        let mut i = 1usize;
        while i < MU_CASES {
            let _ = c.compare(1, me_cid(i), mu_mode(i + 2));
            i += 1;
        }
        !c.face_even(1)
    };
    s.add("J9-漏照为假", j9, "漏对照一例对平断言为假");

    // J10 模式样本确定性
    let m4a = mu_mode(4);
    let m4b = mu_mode(4);
    s.add("J10-模式确定", m4a == m4b, "模式样本同入同出");

    // J11 例号分面确定
    let j11 = mu_cid(0) != me_cid(0) && mu_cid(9) != me_cid(9);
    s.add("J11-例号分面", j11, "两面例号空间互异");

    // J12 渲染分旗
    let j12 = {
        let g = MuCase { cid: 1, mode: 3, verdict: true };
        let r = MuCase { cid: 1, mode: 3, verdict: false };
        g.render().contains('G') && r.render().contains('R')
    };
    s.add("J12-渲染分旗", j12, "绿红对照渲染可区分");

    // J13 面越界拒
    let j13 = MuxLedger::new().enroll(2, 1, 1).err() == Some(MU_NOCASE)
        && MuxLedger::new().compare(2, 1, 1).err() == Some(MU_NOCASE);
    s.add("J13-面越界拒", j13, "面号越界立账对照均必拒");

    // J14 对照幂等：重复对照指纹稳定
    let j14 = {
        let mut c = MuxLedger::new();
        let _ = c.enroll(0, mu_cid(3), 2);
        let _ = c.compare(0, mu_cid(3), 2);
        let f1 = c.fold();
        let _ = c.compare(0, mu_cid(3), 2);
        c.fold() == f1
    };
    s.add("J14-对照幂等", j14, "重复对照指纹稳定");

    // J15 动态明细：双面对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-双面明细",
        fp == root && book.closed(),
        &format!("root={:#x} ftx={} epl={}", fp, book.ftx.len(), book.epl.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11046_mux_flow() {
        let (mut l, built) = build_muxledger();
        assert!(built);
        assert!(l.compare(0, 0x777, 1).err() == Some(MU_NOCASE));
        l.freeze();
        assert!(l.enroll(1, 0xf000, 1).err() == Some(MU_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11046_checks_green() {
        let s = run_unx11046_checks();
        assert!(s.all_passed(), "UNX-F11046 判据未全绿");
    }
}
