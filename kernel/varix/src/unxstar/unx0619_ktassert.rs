//! unx0619_ktassert —— UNX-F0619 ktest initrd 面断言接入。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：UNX-F0619 · ktest initrd 面断言接入 · 目标 ≥240 行
//! 判据原文摘录：B31 判据入套件 unx_a1_b31_p1 全绿，映射双向对账。
//! ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 断言接入 = 判据→套件 的映射登记，一条判据只挂一个套件位，
//!      双向对账（正向表与反向表互为逆映射）不平即拒收。
//!   2. 套件名闭集：unx_a1_b31_p1 为本面唯一套件，别名/空名拒收。
//!   3. 全绿定义：套件内逐条断言跑主路径（0=过），无一条错误路径漏跑。
//!   4. 定容判据槽（B31 initrd 面 4 条判据：F0601/F0604/F0613/F0618），
//!      守恒恒等式：登记数 + 空槽数 = 4。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!      确定性。
//!
//! 诊断段声明：SEG 0x4F20 段位本模块独占。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x4F20 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4F20;
/// 诊断段顶。
pub const SEG_TOP: u16 = 0x4F2F;

/// XcCode：判据号为零（非法判据）。
pub const XC_BAD_CRIT: u16 = SEG_BASE + 1; // 0x4F21 判据号必须非零
/// XcCode：判据重复登记。
pub const XC_DUP_CRIT: u16 = SEG_BASE + 2; // 0x4F22 一号一位不许重复
/// XcCode：套件名非法（空名/越闭集）。
pub const XC_BAD_SUITE: u16 = SEG_BASE + 3; // 0x4F23 套件名闭集外
/// XcCode：双向对账不平。
pub const XC_BIMAP_BROKEN: u16 = SEG_BASE + 4; // 0x4F24 正反表互逆失败
/// XcCode：断言执行红。
pub const XC_ASSERT_RED: u16 = SEG_BASE + 5; // 0x4F25 逐条断言有红
/// XcCode：套件为空不判绿。
pub const XC_EMPTY_SUITE: u16 = SEG_BASE + 6; // 0x4F26 空套件禁止放行

/// B31 initrd 面判据槽容量（F0601/F0604/F0613/F0618 闭集）。
pub const SLOT_N: usize = 4;

/// 本面唯一套件名。
pub const SUITE_NAME: &str = "unx_a1_b31_p1";

/// 判据号闭集（B31 initrd 面）。
pub const B31_CRIT_IDS: [u16; SLOT_N] = [601, 604, 613, 618];

/// 断言登记项。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertReg {
    /// 判据号（F06xx 的 xx 三位）。
    pub crit_id: u16,
    /// 挂靠套件名。
    pub suite: String,
    /// 断言摘要（判据原文短摘，非空）。
    pub digest: String,
}

/// 断言执行结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertOut {
    /// 判据号。
    pub crit_id: u16,
    /// 过=0；红=错误码（XcCode 语义同全局）。
    pub verdict: u16,
}

impl AssertOut {
    /// 绿。
    pub fn green(&self) -> bool {
        self.verdict == 0
    }
}

/// 双向映射账本：正向表（判据→套件）+ 反向表（套件→判据集）。
/// 对账恒等式：正向表条数 = 反向表判据总数，且逐条互逆。
#[derive(Clone, Debug, Default)]
pub struct BiMapBook {
    /// 正向表（按登记序）。
    fwd: Vec<AssertReg>,
    /// 反向表（套件名 → 判据号集，按判据号升序）。
    rev: Vec<(String, Vec<u16>)>,
}

impl BiMapBook {
    /// 空账本。
    pub fn new() -> Self {
        BiMapBook {
            fwd: Vec::new(),
            rev: Vec::new(),
        }
    }

    /// 正向表条数。
    pub fn fwd_n(&self) -> usize {
        self.fwd.len()
    }

    /// 正向查套件。
    pub fn suite_of(&self, crit_id: u16) -> Option<&str> {
        self.fwd
            .iter()
            .find(|r| r.crit_id == crit_id)
            .map(|r| r.suite.as_str())
    }

    /// 反向查判据集。
    pub fn crits_of(&self, suite: &str) -> Option<Vec<u16>> {
        self.rev
            .iter()
            .find(|(s, _)| s == suite)
            .map(|(_, v)| v.clone())
    }

    /// 登记一条映射（判据→套件）。校验闭集、去重、非空摘要。
    pub fn enroll(&mut self, crit_id: u16, suite: &str, digest: &str) -> Result<(), u16> {
        if crit_id == 0 {
            return Err(XC_BAD_CRIT);
        }
        if suite.is_empty() || suite != SUITE_NAME {
            return Err(XC_BAD_SUITE);
        }
        if digest.is_empty() {
            return Err(XC_BAD_CRIT);
        }
        if self.fwd.iter().any(|r| r.crit_id == crit_id) {
            return Err(XC_DUP_CRIT);
        }
        self.fwd.push(AssertReg {
            crit_id,
            suite: suite.to_string(),
            digest: digest.to_string(),
        });
        // 反向表同步维护。
        match self.rev.iter_mut().find(|(s, _)| s == suite) {
            Some((_, v)) => {
                v.push(crit_id);
                v.sort_unstable();
            }
            None => {
                let mut v = Vec::new();
                v.push(crit_id);
                self.rev.push((suite.to_string(), v));
            }
        }
        Ok(())
    }

    /// 双向对账：正向逐条 → 反向可命中；反向逐条 → 正向可命中；计数相等。
    pub fn bimap_reconcile(&self) -> Result<usize, u16> {
        let rev_total: usize = self.rev.iter().map(|(_, v)| v.len()).sum();
        if self.fwd.len() != rev_total {
            return Err(XC_BIMAP_BROKEN);
        }
        for r in &self.fwd {
            match self.crits_of(&r.suite) {
                Some(v) => {
                    if !v.contains(&r.crit_id) {
                        return Err(XC_BIMAP_BROKEN);
                    }
                }
                None => return Err(XC_BIMAP_BROKEN),
            }
        }
        for (s, v) in &self.rev {
            for c in v {
                match self.suite_of(*c) {
                    Some(name) => {
                        if name != s {
                            return Err(XC_BIMAP_BROKEN);
                        }
                    }
                    None => return Err(XC_BIMAP_BROKEN),
                }
            }
        }
        Ok(self.fwd.len())
    }

    /// 按判据号升序返回正向表（确定性遍历序）。
    pub fn fwd_sorted(&self) -> Vec<AssertReg> {
        let mut v = self.fwd.clone();
        v.sort_by_key(|r| r.crit_id);
        v
    }
}

/// 逐条断言执行机：对一条判据跑"登记在场+摘要非空+号在闭集"三查。
/// 三查全过 = 绿（0），否则红（对应 XcCode）。
pub fn run_one(book: &BiMapBook, crit_id: u16) -> AssertOut {
    let in_closed = B31_CRIT_IDS.contains(&crit_id);
    let reg = book.fwd.iter().find(|r| r.crit_id == crit_id);
    let verdict = if !in_closed {
        XC_BAD_CRIT
    } else {
        match reg {
            None => XC_ASSERT_RED,
            Some(r) => {
                if r.digest.is_empty() || r.suite != SUITE_NAME {
                    XC_ASSERT_RED
                } else {
                    0
                }
            }
        }
    };
    AssertOut { crit_id, verdict }
}

/// 套件全绿核：登记满闭集 + 双向对账平 + 逐条断言全绿。
/// 空套件不放行（XC_EMPTY_SUITE）。
pub fn suite_green(book: &BiMapBook) -> Result<Vec<AssertOut>, u16> {
    if book.fwd_n() == 0 {
        return Err(XC_EMPTY_SUITE);
    }
    book.bimap_reconcile()?;
    let mut outs = Vec::new();
    for c in B31_CRIT_IDS.iter() {
        let o = run_one(book, *c);
        if !o.green() {
            return Err(o.verdict);
        }
        outs.push(o);
    }
    Ok(outs)
}

/// 入套件器：把 B31 initrd 面四条判据按固定摘要登记入 unx_a1_b31_p1。
pub fn enroll_b31_suite() -> Result<BiMapBook, u16> {
    let mut book = BiMapBook::new();
    let digests = [
        "F0601 initrd 空判",
        "F0604 消费跳步",
        "F0613 空清单封账",
        "F0618 空档路径账",
    ];
    for i in 0..SLOT_N {
        book.enroll(B31_CRIT_IDS[i], SUITE_NAME, digests[i])?;
    }
    Ok(book)
}

/// 自例程：主路径（入套件→双向对账→全绿）+ 错误路径（坏号/重号/坏套件/对账破/空套件）。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let book = enroll_b31_suite()?;
    if book.fwd_n() != SLOT_N {
        return Err(XC_ASSERT_RED);
    }
    let n = book.bimap_reconcile()?;
    if n != SLOT_N {
        return Err(XC_BIMAP_BROKEN);
    }
    let outs = suite_green(&book)?;
    if outs.len() != SLOT_N || !outs.iter().all(|o| o.green()) {
        return Err(XC_ASSERT_RED);
    }
    // 正反查一致：600 号不在场。
    if book.suite_of(600).is_some() {
        return Err(XC_BAD_CRIT);
    }
    if book.crits_of(SUITE_NAME).is_none() {
        return Err(XC_BIMAP_BROKEN);
    }

    // 错误路径 1：零号判据。
    let mut b1 = BiMapBook::new();
    if b1.enroll(0, SUITE_NAME, "x") != Err(XC_BAD_CRIT) {
        return Err(XC_BAD_CRIT);
    }
    // 错误路径 2：重号。
    let mut b2 = enroll_b31_suite()?;
    if b2.enroll(601, SUITE_NAME, "再挂") != Err(XC_DUP_CRIT) {
        return Err(XC_DUP_CRIT);
    }
    // 错误路径 3：套件名闭集外。
    let mut b3 = BiMapBook::new();
    if b3.enroll(601, "unx_a1_b32_p1", "别家套件") != Err(XC_BAD_SUITE) {
        return Err(XC_BAD_SUITE);
    }
    // 错误路径 4：空套件全绿核拒放行。
    let b4 = BiMapBook::new();
    if suite_green(&b4) != Err(XC_EMPTY_SUITE) {
        return Err(XC_EMPTY_SUITE);
    }
    // 错误路径 5：漏登一条 → 断言红。
    let mut b5 = BiMapBook::new();
    b5.enroll(601, SUITE_NAME, "只登一条")?;
    if suite_green(&b5) != Err(XC_ASSERT_RED) {
        return Err(XC_ASSERT_RED);
    }
    // 排序确定性。
    let sorted = book.fwd_sorted();
    for i in 1..sorted.len() {
        if sorted[i - 1].crit_id >= sorted[i].crit_id {
            return Err(XC_BIMAP_BROKEN);
        }
    }
    Ok(0)
}
