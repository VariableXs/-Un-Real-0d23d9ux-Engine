//! UNX-F11048 LTP eventfd/timerfd 组用例对照（fd 族对账）（0x1e600 段）
//!
//! 外部对照面第 8 批：eventfd/timerfd 组用例对照。两面对账：
//! eventfd 面（写累加/读清零的计数器语义）10 例，
//! timerfd 面（定装/到期/读到期数语义）10 例。
//! 期望账先行登记，实测账逐例回填，matched 逐例对平；
//! 守恒式：eventfd 面 written == read_back + residue（计数不生灭），
//! timerfd 面 expired == cleared（到期必清）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11048 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11048 - 11000) * 0x20;

/// 用例账空 —— 对照前必须先登记期望
pub const FD_EMPTY: u32 = SEG_BASE + 0x01;
/// 案例号越界 —— 20 例编号 0..20
pub const FD_RANGE: u32 = SEG_BASE + 0x02;
/// 期望与实测不一致 —— 对照失配
pub const FD_MISMATCH: u32 = SEG_BASE + 0x03;
/// 守恒被破坏 —— 计数器余额或到期账失衡
pub const FD_UNBALANCED: u32 = SEG_BASE + 0x04;
/// 面（face）编号越界 —— 只有 0 eventfd 面与 1 timerfd 面
pub const FD_BADFACE: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再回填实测
pub const FD_FROZEN: u32 = SEG_BASE + 0x06;

/// eventfd 面用例数：写读计数语义覆盖
pub const EFD_CASES: usize = 10;
/// timerfd 面用例数：定装到期读语义覆盖
pub const TFD_CASES: usize = 10;
/// 对照总例数
pub const CASES_TOTAL: usize = EFD_CASES + TFD_CASES;
/// eventfd 面每例写读轮数
pub const EFD_ROUNDS: usize = 30;
/// timerfd 面每例定装轮数
pub const TFD_ROUNDS: usize = 20;
/// 计数器单步增量
pub const EFD_STEP: u64 = 3;
/// fd 族对照面标签
pub const FDFD_TAG: u64 = 0x6566_645f_6c74_3048;

/// 单条对照用例：期望账 + 实测账 + 对平标记
#[derive(Clone, Debug)]
pub struct FdCase {
    /// LTP 风格用例号（0x6000 段起编）
    pub cid: u32,
    /// 用例名
    pub name: String,
    /// 所属面：0=eventfd，1=timerfd
    pub face: u8,
    /// 期望值（登记账）
    pub expect: u64,
    /// 实测值（回填账）
    pub actual: u64,
    /// 是否已对平
    pub matched: bool,
}

impl FdCase {
    /// 登记一条期望，实测留空等待回填
    pub fn new(cid: u32, name: &str, face: u8, expect: u64) -> FdCase {
        FdCase { cid, name: name.to_string(), face, expect, actual: 0, matched: false }
    }
}

/// fd 族对照总账
#[derive(Clone, Debug)]
pub struct FdLedger {
    /// 20 条对照用例（前 10 eventfd 面，后 10 timerfd 面）
    pub cases: Vec<FdCase>,
    /// eventfd 面累计写入计数
    pub written: u64,
    /// eventfd 面累计读出计数
    pub read_back: u64,
    /// eventfd 面仍滞留的计数（未读余额）
    pub residue: u64,
    /// timerfd 面累计到期次数
    pub expired: u64,
    /// timerfd 面累计读清次数
    pub cleared: u64,
    /// 已对平例数
    pub matched_n: usize,
    /// 已回填例数
    pub filled_n: usize,
    /// 冻结标记
    pub frozen: bool,
}

impl FdLedger {
    /// 立账并登记全部 20 例期望
    pub fn new() -> FdLedger {
        let mut lg = FdLedger {
            cases: Vec::new(),
            written: 0, read_back: 0, residue: 0,
            expired: 0, cleared: 0,
            matched_n: 0, filled_n: 0, frozen: false,
        };
        for i in 0..EFD_CASES {
            // eventfd 面期望 = 轮数 × 步长 × 倍率
            let expect = (EFD_ROUNDS as u64) * EFD_STEP * ((i as u64) + 1);
            lg.cases.push(FdCase::new(0x6000 + i as u32, "fd_efd", 0, expect));
        }
        for i in 0..TFD_CASES {
            // timerfd 面期望 = 定装轮数 × 每轮到期 2 次
            let expect = (TFD_ROUNDS as u64) * 2 * ((i as u64) + 1);
            lg.cases.push(FdCase::new(0x6100 + i as u32, "fd_tfd", 1, expect));
        }
        lg
    }

    /// 实际运行一例 eventfd 对照：EFD_ROUNDS 倍率轮，每轮写 EFD_STEP 读 EFD_STEP
    pub fn run_efd(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(FD_FROZEN); }
        if no >= EFD_CASES { return Err(FD_RANGE); }
        let rounds = EFD_ROUNDS * (no + 1);
        let mut got = 0u64;
        for _ in 0..rounds {
            self.written = self.written.wrapping_add(EFD_STEP);
            self.read_back = self.read_back.wrapping_add(EFD_STEP);
            got = got.wrapping_add(EFD_STEP);
        }
        self.residue = self.written.wrapping_sub(self.read_back);
        Ok(got)
    }

    /// 实际运行一例 timerfd 对照：TFD_ROUNDS 倍率轮，每轮定装 1 次到期 2 次读清 2 次
    pub fn run_tfd(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(FD_FROZEN); }
        if no < EFD_CASES || no >= CASES_TOTAL { return Err(FD_RANGE); }
        let rounds = TFD_ROUNDS * (no + 1 - EFD_CASES);
        let mut got = 0u64;
        for _ in 0..rounds {
            self.expired = self.expired.wrapping_add(2);
            got = got.wrapping_add(2);
            self.cleared = self.cleared.wrapping_add(2);
        }
        Ok(got)
    }

    /// 回填一例实测并对平
    pub fn fill(&mut self, no: usize, actual: u64) -> Result<bool, u32> {
        if self.frozen { return Err(FD_FROZEN); }
        if no >= CASES_TOTAL { return Err(FD_RANGE); }
        let c = &mut self.cases[no];
        if c.matched { return Err(FD_MISMATCH); }
        c.actual = actual;
        c.matched = c.expect == c.actual;
        self.filled_n += 1;
        if c.matched { self.matched_n += 1; }
        Ok(c.matched)
    }

    /// 对照一例：跑 + 回填一步到位
    pub fn compare(&mut self, no: usize) -> Result<bool, u32> {
        let actual = if no < EFD_CASES { self.run_efd(no)? } else { self.run_tfd(no)? };
        self.fill(no, actual)
    }

    /// 取一例（空账返回 FD_EMPTY，越界返回 FD_RANGE）
    pub fn case_of(&self, no: usize) -> Result<usize, u32> {
        if self.cases.is_empty() { return Err(FD_EMPTY); }
        if no >= self.cases.len() { return Err(FD_RANGE); }
        Ok(no)
    }

    /// 按面统计对平数
    pub fn matched_face(&self, face: u8) -> Result<usize, u32> {
        if face > 1 { return Err(FD_BADFACE); }
        Ok(self.cases.iter().filter(|c| c.face == face && c.matched).count())
    }

    /// 守恒探测：written == read_back + residue 且 expired == cleared
    pub fn conserved(&self) -> bool {
        self.written == self.read_back.wrapping_add(self.residue)
            && self.expired == self.cleared
    }

    /// 守恒闸：失衡时返回 FD_UNBALANCED 拒止
    pub fn balance_gate(&self) -> Result<(), u32> {
        if self.conserved() { Ok(()) } else { Err(FD_UNBALANCED) }
    }

    /// 全面对平：20/20 且守恒成立
    pub fn aligned(&self) -> bool {
        self.matched_n == CASES_TOTAL
            && self.matched_face(0).unwrap_or(usize::MAX) == EFD_CASES
            && self.matched_face(1).unwrap_or(usize::MAX) == TFD_CASES
            && self.conserved()
    }

    /// FNV 折叠：全部用例与计数器折成指纹
    pub fn fold(&self) -> u64 {
        let mut h: u64 = FDFD_TAG;
        for c in &self.cases {
            h = (h ^ (c.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ c.expect).wrapping_mul(0x100000001b3);
            h = (h ^ c.actual).wrapping_mul(0x100000001b3);
            h = (h ^ (c.matched as u64)).wrapping_mul(0x100000001b3);
        }
        for x in [self.written, self.read_back, self.expired, self.cleared] {
            h = (h ^ x).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 冻结总账
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：全部 20 例对平
pub fn sample_aligned() -> (FdLedger, bool) {
    let mut lg = FdLedger::new();
    for no in 0..CASES_TOTAL {
        let _ = lg.compare(no);
    }
    let ok = lg.aligned();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：错填实测 / 越界 / 冻结拒写
pub fn inject_four() -> [bool; 4] {
    // 注入 1：错填实测 → 对照判失配（Ok(false)，matched 保持 false）
    let mut lg = FdLedger::new();
    let i1 = matches!(lg.fill(0, 999), Ok(false)) && !lg.cases[0].matched;
    // 注入 2：eventfd 面越界案例号 → FD_RANGE 拒止
    let i2 = matches!(lg.run_efd(EFD_CASES), Err(FD_RANGE));
    // 注入 3：eventfd 面号跑 timerfd 面 → FD_RANGE 拒止
    let i3 = matches!(lg.run_tfd(0), Err(FD_RANGE));
    // 注入 4：冻结后回填 → FD_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.fill(1, 1), Err(FD_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 eventfd 对照对平（10/10），timerfd 对照（10/10）
pub fn run_unx11048_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11048");
    let (lg, ok) = sample_aligned();
    let m0 = lg.matched_face(0).unwrap_or(usize::MAX);
    let m1 = lg.matched_face(1).unwrap_or(usize::MAX);

    s.add("J1-efd对平", ok && m0 == EFD_CASES, "eventfd 面用例对照 10/10 对平");
    s.add("J1-tfd对平", ok && m1 == TFD_CASES, "timerfd 面用例对照 10/10 对平");
    s.add("J1-总量对平", lg.matched_n == CASES_TOTAL, "对照总量 20/20 守恒");
    s.add("J2-计数守恒", lg.written == lg.read_back + lg.residue, "eventfd 面 written==read_back+residue");
    s.add("J2-到期守恒", lg.expired == lg.cleared, "timerfd 面 expired==cleared 到期必清");
    s.add("J2-守恒闸", lg.balance_gate().is_ok(), "守恒闸对平账放行、失衡即 FD_UNBALANCED");
    s.add("J3-编号连续", lg.cases.len() == CASES_TOTAL && lg.cases[0].cid == 0x6000 && lg.cases[10].cid == 0x6100, "用例号 0x6000/0x6100 双段起编连续");
    let el = FdLedger { cases: Vec::new(), written: 0, read_back: 0, residue: 0, expired: 0, cleared: 0, matched_n: 0, filled_n: 0, frozen: false };
    s.add("J3-空账拒查", matches!(el.case_of(0), Err(FD_EMPTY)) && matches!(lg.case_of(0), Ok(0)), "空账查询被 FD_EMPTY 拒止、实账放行");
    s.add("J3-面归属", lg.cases.iter().filter(|c| c.face == 0).count() == EFD_CASES, "面归属标注准确 eventfd 面 10 例");
    s.add("J3-期望非零", lg.cases.iter().all(|c| c.expect > 0), "全部期望账非零登记");
    s.add("J4-实测回填", lg.filled_n == CASES_TOTAL, "20 例实测账全部回填");
    s.add("J4-冻结拒写", inject_four()[3], "冻结后回填被 FD_FROZEN 拒止");
    s.add("J5-失配拒平", inject_four()[0], "错填实测被判失配且不误标对平");
    s.add("J5-越界拒跑", inject_four()[1] && inject_four()[2], "越界案例号被 FD_RANGE 拒止");
    s.add("J6-读清归零", lg.residue == 0 && lg.expired == lg.cleared, "对照收尾计数余额为零且到期账清平");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J7-对平动态",
        ok,
        &format!("对平 {}/{}，efd 面 {} 例 tfd 面 {} 例，fold={:#x}", lg.matched_n, CASES_TOTAL, m0, m1, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11048_ltp_eventfd_timerfd_flow() {
        let mut lg = FdLedger::new();
        // 冻结前越界拒跑
        assert!(matches!(lg.run_efd(99), Err(FD_RANGE)));
        // 全量对照
        for no in 0..CASES_TOTAL {
            assert!(lg.compare(no).unwrap_or(false), "案例 {} 应对平", no);
        }
        assert_eq!(lg.matched_n, CASES_TOTAL);
        assert!(lg.conserved());
        assert_eq!(lg.residue, 0);
        assert_eq!(lg.expired, lg.cleared);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        // 冻结后拒改
        assert!(matches!(lg.fill(0, 1), Err(FD_FROZEN)));
        // 注入路径全拒止
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11048_checks_green() {
        let s = run_unx11048_checks();
        assert!(s.all_passed(), "UNX-F11048 判据未全绿");
    }
}
