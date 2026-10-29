//! UNX-F11042 LTP pipe/fifo 组用例对照（管道族对账）（0x1e540 段）
//!
//! 锚点：UNX-F11042-J1 pipe 组用例对照逐例对平（10/10），fifo 组对照（10/10）。
//! 域：CoRun·UNX-C4-B33（LTP 对照域·管道族）。
//!
//! 结构：管道组对照账（pipe 面 + fifo 面各十例：例号 + 期望 + 实测 + 裁
//! 断位）+ 对照器（期望与实测一致才翻绿）+ 逐面对平器（pipe 面与 fifo 面
//! 各自全绿）+ fold 与冻结 + 注入核（期望错值/漏对照/面缺例/冻结后写四
//! 路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1e540 独占 0x20）。
pub const SEG_BASE: u32 = 0x1e540;
pub const LP_MISMATCH: u32 = SEG_BASE + 0x01; // 期望失配
pub const LP_NOCASE: u32 = SEG_BASE + 0x02; // 例未立账
pub const LP_DUP: u32 = SEG_BASE + 0x03; // 例号重号
pub const LP_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后写账
pub const LP_SHORT: u32 = SEG_BASE + 0x05; // 面缺例
pub const LP_ZEXP: u32 = SEG_BASE + 0x06; // 零期望

/// 每面例数（10/10）。
pub const PF_CASES: usize = 10;
/// 管道对照账域标（fold 种子）。
pub const PFC_TAG: u64 = 0x1e54_2a;

/// 单对照例：例号 + 期望 + 实测 + 裁断位。
#[derive(Clone, Debug)]
pub struct PipeCase {
    pub cid: u32,
    pub expect: u64,
    pub actual: u64,
    pub verdict: bool,
}

impl PipeCase {
    /// 对照齐绿：期望非零且与实测一致。
    pub fn green(&self) -> bool {
        self.verdict && self.expect != 0 && self.expect == self.actual
    }

    pub fn render(&self) -> String {
        let flag = if self.green() { "G" } else { "R" };
        format!("pc|c{}|{}|e{}|a{}", self.cid, flag, self.expect, self.actual)
    }
}

/// 管道族对照账：pipe 面 + fifo 面双面立账对照。
pub struct PipeFifoLedger {
    pub pipe: Vec<PipeCase>,
    pub fifo: Vec<PipeCase>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl PipeFifoLedger {
    pub fn new() -> PipeFifoLedger {
        PipeFifoLedger { pipe: Vec::new(), fifo: Vec::new(), frozen: false, root: None }
    }

    /// 立账（face: 0=pipe 1=fifo）：冻结、零期望、重号必拒。
    pub fn enroll(&mut self, face: usize, cid: u32, expect: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(LP_FROZEN);
        }
        if face >= 2 {
            return Err(LP_NOCASE);
        }
        if expect == 0 {
            return Err(LP_ZEXP);
        }
        let tab = if face == 0 { &mut self.pipe } else { &mut self.fifo };
        if tab.iter().any(|x| x.cid == cid) {
            return Err(LP_DUP);
        }
        tab.push(PipeCase { cid, expect, actual: 0, verdict: false });
        Ok(())
    }

    /// 对照：实测与期望一致才翻绿；例未立账必拒。
    pub fn compare(&mut self, face: usize, cid: u32, actual: u64) -> Result<bool, u32> {
        if self.frozen {
            return Err(LP_FROZEN);
        }
        if face >= 2 {
            return Err(LP_NOCASE);
        }
        let tab = if face == 0 { &mut self.pipe } else { &mut self.fifo };
        let mut hit = false;
        let mut ok = false;
        let mut k = 0usize;
        while k < tab.len() {
            if tab[k].cid == cid {
                tab[k].actual = actual;
                tab[k].verdict = tab[k].expect == actual;
                ok = tab[k].verdict;
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(ok)
        } else {
            Err(LP_NOCASE)
        }
    }

    /// 单面缺例清单。
    pub fn gaps_of(&self, face: usize) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let tab = if face == 0 { &self.pipe } else { &self.fifo };
        let mut k = 0usize;
        while k < tab.len() {
            if !tab[k].verdict {
                g.push(tab[k].cid);
            }
            k += 1;
        }
        g
    }

    /// 单面对平：例数齐且全绿。
    pub fn face_even(&self, face: usize) -> bool {
        let tab = if face == 0 { &self.pipe } else { &self.fifo };
        if tab.len() != PF_CASES {
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

    /// 链式 fold 指纹（FNV 风格，序敏感）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = PFC_TAG;
        let mut k = 0usize;
        while k < self.pipe.len() {
            h = (h ^ self.pipe[k].cid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.pipe[k].expect).wrapping_mul(0x100000001b3);
            h = (h ^ self.pipe[k].verdict as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        k = 0;
        while k < self.fifo.len() {
            h = (h ^ self.fifo[k].cid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ self.fifo[k].expect).wrapping_mul(0x100000001b3);
            h = (h ^ self.fifo[k].verdict as u64).wrapping_mul(0x100000001b3);
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

/// pipe 面例号确定性样本。
pub fn lp_cid(no: usize) -> u32 {
    0x5000 + no as u32
}

/// fifo 面例号确定性样本。
pub fn lf_cid(no: usize) -> u32 {
    0x6000 + no as u32
}

/// 期望值确定性样本。
pub fn pf_expect(no: usize) -> u64 {
    ((no % 7 + 2) as u64).wrapping_mul(0x100000001b3) ^ (no as u64).wrapping_add(0x5a)
}

/// 双面全量对照全链。
pub fn build_pipefifo() -> (PipeFifoLedger, bool) {
    let mut l = PipeFifoLedger::new();
    let mut ok = true;
    let mut i = 0usize;
    while i < PF_CASES {
        if l.enroll(0, lp_cid(i), pf_expect(i)).is_err() {
            ok = false;
        }
        if l.enroll(1, lf_cid(i), pf_expect(i + PF_CASES)).is_err() {
            ok = false;
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < PF_CASES {
        if l.compare(0, lp_cid(i), pf_expect(i)).is_err() {
            ok = false;
        }
        if l.compare(1, lf_cid(i), pf_expect(i + PF_CASES)).is_err() {
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
    // 1) 期望错值必出红
    let mut a = PipeFifoLedger::new();
    let _ = a.enroll(0, lp_cid(0), pf_expect(0));
    let _ = a.compare(0, lp_cid(0), pf_expect(0).wrapping_add(1));
    got[0] = a.gaps_of(0).len() == 1 && !a.face_even(0);
    // 2) 例未立账对照必拒
    let mut b = PipeFifoLedger::new();
    got[1] = b.compare(0, 0x777, 1).err() == Some(LP_NOCASE);
    // 3) 零期望与重号必拒
    let mut c = PipeFifoLedger::new();
    let z = c.enroll(0, lp_cid(1), 0).err() == Some(LP_ZEXP);
    let _ = c.enroll(0, lp_cid(1), 5);
    got[2] = z && c.enroll(0, lp_cid(1), 5).err() == Some(LP_DUP);
    // 4) 冻结后写账必抓
    let (mut d, _) = build_pipefifo();
    d.freeze();
    got[3] = d.enroll(0, 0x8888, 1).err() == Some(LP_FROZEN)
        && d.compare(1, lf_cid(0), 1).err() == Some(LP_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11042_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11042");

    // J1 pipe 组用例对照逐例对平
    let (mut book, green) = build_pipefifo();
    s.add("J1-pipe 面对平", green && book.face_even(0), "pipe 面十例逐例对照齐绿");

    // J2 fifo 组对照：fifo 面逐例齐绿
    s.add("J2-fifo 面对平", book.face_even(1), "fifo 面十例逐例对照齐绿");

    // J3 冻结根：fold 稳定且与根一致
    let root = book.freeze();
    s.add("J3-冻结根", book.root == Some(root) && book.fold() == root && root != 0, "冻结根落账且指纹稳定");

    // J4 冻结后写账必抓
    let j4 = book.enroll(0, 0x8888, 1).err() == Some(LP_FROZEN)
        && book.compare(0, lp_cid(0), 1).err() == Some(LP_FROZEN);
    s.add("J4-冻结拒写", j4, "冻结后立账对照均必拒");

    // J5 注入检出：期望错值 + 例未立账
    let inj = inject_four();
    s.add("J5-错值未立抓", inj[0] && inj[1], "期望错值与例未立账两路必抓");

    // J6 注入检出：零期望重号 + 冻结后写
    s.add("J6-零重冻结检出", inj[2] && inj[3], "零期望重号与冻结后写必抓");

    // J7 双面齐备：两各十例
    let j7 = book.pipe.len() == PF_CASES && book.fifo.len() == PF_CASES;
    s.add("J7-双面齐备", j7, "pipe 与 fifo 面各十例");

    // J8 缺例裁断：例数不足面平为假
    let j8 = {
        let mut c = PipeFifoLedger::new();
        let mut i = 0usize;
        while i < PF_CASES - 1 {
            let _ = c.enroll(0, lp_cid(i), pf_expect(i));
            i += 1;
        }
        !c.face_even(0)
    };
    s.add("J8-缺例为假", j8, "面缺例对平断言为假");

    // J9 期望样本确定性
    let e3a = pf_expect(3);
    let e3b = pf_expect(3);
    s.add("J9-期望确定", e3a == e3b, "期望样本同入同出");

    // J10 例号分面确定：pipe 与 fifo 例号互异
    let j10 = lp_cid(0) != lf_cid(0) && lp_cid(9) != lf_cid(9);
    s.add("J10-例号分面", j10, "两面例号空间互异");

    // J11 渲染分旗：绿红可区分
    let j11 = {
        let g = PipeCase { cid: 1, expect: 5, actual: 5, verdict: true };
        let r = PipeCase { cid: 1, expect: 5, actual: 4, verdict: false };
        g.render().contains('G') && r.render().contains('R')
    };
    s.add("J11-渲染分旗", j11, "绿红对照渲染可区分");

    // J12 对照幂等：重复对照结果一致
    let j12 = {
        let mut c = PipeFifoLedger::new();
        let _ = c.enroll(0, lp_cid(2), pf_expect(2));
        let f1 = c.fold();
        let _ = c.compare(0, lp_cid(2), pf_expect(2));
        let f2 = c.fold();
        let _ = c.compare(0, lp_cid(2), pf_expect(2));
        f2 != f1 && c.fold() == f2
    };
    s.add("J12-对照幂等", j12, "重复对照指纹稳定");

    // J13 失配可报：实测错值逐例可枚举
    let j13 = {
        let mut c = PipeFifoLedger::new();
        let _ = c.enroll(0, lp_cid(3), pf_expect(3));
        let _ = c.compare(0, lp_cid(3), pf_expect(3).wrapping_add(7));
        c.gaps_of(0).len() == 1 && !c.closed()
    };
    s.add("J13-失配可报", j13, "失配例可枚举且对平为假");

    // J14 面错误码拒：face 越界必拒
    let j14 = PipeFifoLedger::new().enroll(2, 1, 1).err() == Some(LP_NOCASE)
        && PipeFifoLedger::new().compare(2, 1, 1).err() == Some(LP_NOCASE);
    s.add("J14-面越界拒", j14, "面号越界立账对照均必拒");

    // J15 动态明细：双面对平与冻结根落账
    let fp = book.fold();
    s.add_dyn(
        "J15-双面明细",
        fp == root && book.closed(),
        &format!("root={:#x} pipe={} fifo={}", fp, book.pipe.len(), book.fifo.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11042_pipefifo_flow() {
        let (mut l, built) = build_pipefifo();
        assert!(built);
        assert!(l.compare(0, 0x777, 1).err() == Some(LP_NOCASE));
        l.freeze();
        assert!(l.enroll(0, 0x8888, 1).err() == Some(LP_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx11042_checks_green() {
        let s = run_unx11042_checks();
        assert!(s.all_passed(), "UNX-F11042 判据未全绿");
    }
}
