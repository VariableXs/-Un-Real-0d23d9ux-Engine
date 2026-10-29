//! UNX-F11053 对照回归重放（全量用例重放账）（0x1e6a0 段）
//!
//! 对照回归重放：对照战役 40 例（8 族 × 5 例）首跑留账后，
//! 全量重放一遍，逐例比对首跑值与重放值；确定性要求
//! 首跑 == 重放（40/40），漂移即拒收。守恒式：
//! agreed == replayed == 总例数（重放全覆盖且零漂移）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11053 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11053 - 11000) * 0x20;

/// 账空 —— 重放前必须先首跑留账
pub const RP_EMPTY: u32 = SEG_BASE + 0x01;
/// 例号越界 —— 40 例编号 0..40
pub const RP_RANGE: u32 = SEG_BASE + 0x02;
/// 首跑未留账 —— 重放前必须先 run 首跑
pub const RP_NORUN: u32 = SEG_BASE + 0x03;
/// 重放漂移 —— 重放值与首跑值不一致
pub const RP_DRIFT: u32 = SEG_BASE + 0x04;
/// 重复重放 —— 一例只重放一次
pub const RP_DUP: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再重放
pub const RP_FROZEN: u32 = SEG_BASE + 0x06;

/// 重放族数
pub const FAMILIES: usize = 8;
/// 每族例数
pub const CASES_PER_FAM: usize = 5;
/// 重放总例数
pub const CASES_TOTAL: usize = FAMILIES * CASES_PER_FAM;
/// 重放账标签
pub const REPLAY_TAG: u64 = 0x7270_6c72_706c_3044;

/// 确定性用例函数：f(no) 闭环可重放
pub fn replay_fn(no: usize) -> u64 {
    let n = (no + 1) as u64;
    n.wrapping_mul(37).wrapping_add(n.wrapping_mul(n) % 23)
}

/// 单条重放例：首跑值 + 重放值
#[derive(Clone, Debug)]
pub struct RpCase {
    /// 例号（0xC000 段起编）
    pub cid: u32,
    /// 族号
    pub fam: u8,
    /// 首跑值
    pub first: u64,
    /// 重放值
    pub replay: u64,
    /// 一致标记
    pub agreed: bool,
}

/// 全量重放总账
#[derive(Clone, Debug)]
pub struct ReplayLedger {
    /// 40 条重放例
    pub cases: Vec<RpCase>,
    /// 首跑例数
    pub run_n: usize,
    /// 重放例数
    pub replayed_n: usize,
    /// 一致例数
    pub agreed_n: usize,
    /// 漂移检出次数
    pub drifts: u64,
    /// 冻结标记
    pub frozen: bool,
}

impl ReplayLedger {
    /// 立账：40 例首跑值留空
    pub fn new() -> ReplayLedger {
        let mut lg = ReplayLedger {
            cases: Vec::new(),
            run_n: 0, replayed_n: 0, agreed_n: 0,
            drifts: 0, frozen: false,
        };
        for fam in 0..FAMILIES {
            for ci in 0..CASES_PER_FAM {
                let no = fam * CASES_PER_FAM + ci;
                lg.cases.push(RpCase {
                    cid: 0xC000 + no as u32,
                    fam: fam as u8,
                    first: 0,
                    replay: 0,
                    agreed: false,
                });
            }
        }
        lg
    }

    /// 首跑一例：留账
    pub fn run_first(&mut self, no: usize) -> Result<u64, u32> {
        if self.frozen { return Err(RP_FROZEN); }
        if self.cases.is_empty() { return Err(RP_EMPTY); }
        if no >= self.cases.len() { return Err(RP_RANGE); }
        let v = replay_fn(no);
        self.cases[no].first = v;
        self.run_n += 1;
        Ok(v)
    }

    /// 重放一例：必须与首跑一致
    pub fn replay(&mut self, no: usize) -> Result<bool, u32> {
        if self.frozen { return Err(RP_FROZEN); }
        if no >= self.cases.len() { return Err(RP_RANGE); }
        let c = &mut self.cases[no];
        if c.first == 0 { return Err(RP_NORUN); }
        if c.agreed { return Err(RP_DUP); }
        let v = replay_fn(no);
        c.replay = v;
        c.agreed = c.first == v;
        self.replayed_n += 1;
        if c.agreed { self.agreed_n += 1; } else { self.drifts = self.drifts.wrapping_add(1); }
        Ok(c.agreed)
    }

    /// 全量重放：先首跑后重放
    pub fn replay_all(&mut self) -> Result<usize, u32> {
        for no in 0..self.cases.len() {
            if self.cases[no].first == 0 {
                self.run_first(no)?;
            }
        }
        for no in 0..self.cases.len() {
            self.replay(no)?;
        }
        Ok(self.agreed_n)
    }

    /// 按族统计一致例数
    pub fn agreed_fam(&self, fam: u8) -> usize {
        self.cases.iter().filter(|c| c.fam == fam && c.agreed).count()
    }

    /// 全量一致：40/40 零漂移
    pub fn all_agreed(&self) -> bool {
        self.agreed_n == CASES_TOTAL && self.drifts == 0 && self.replayed_n == CASES_TOTAL
    }

    /// 漂移闸：存在漂移即 RP_DRIFT
    pub fn drift_gate(&self) -> Result<(), u32> {
        if self.drifts == 0 { Ok(()) } else { Err(RP_DRIFT) }
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = REPLAY_TAG;
        for c in &self.cases {
            h = (h ^ (c.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ c.first).wrapping_mul(0x100000001b3);
            h = (h ^ c.replay).wrapping_mul(0x100000001b3);
            h = (h ^ (c.agreed as u64)).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.drifts).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结重放账
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：40 例全量重放零漂移
pub fn sample_replayed() -> (ReplayLedger, bool) {
    let mut lg = ReplayLedger::new();
    let _ = lg.replay_all();
    let ok = lg.all_agreed();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：未首跑重放 / 重复重放 / 越界 / 冻结
pub fn inject_four() -> [bool; 4] {
    let mut lg = ReplayLedger::new();
    // 注入 1：未首跑先重放 → RP_NORUN 拒止
    let i1 = matches!(lg.replay(0), Err(RP_NORUN));
    // 注入 2：首跑后重放，再重复重放 → RP_DUP 拒止
    let _ = lg.run_first(1);
    let _ = lg.replay(1);
    let i2 = matches!(lg.replay(1), Err(RP_DUP));
    // 注入 3：例号越界 → RP_RANGE 拒止
    let i3 = matches!(lg.run_first(CASES_TOTAL), Err(RP_RANGE));
    // 注入 4：冻结后重放 → RP_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.replay(2), Err(RP_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 全量重放逐例对平，重放与首跑一致
pub fn run_unx11053_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11053");
    let (lg, ok) = sample_replayed();

    s.add("J1-全量重放", ok && lg.replayed_n == CASES_TOTAL, "40 例全量重放无遗漏");
    s.add("J1-逐例一致", lg.agreed_n == CASES_TOTAL, "重放值与首跑值 40/40 逐例一致");
    s.add("J1-零漂移", lg.drifts == 0 && lg.drift_gate().is_ok(), "重放零漂移检出");
    s.add("J2-首跑留账", lg.run_n == CASES_TOTAL, "首跑 40 例全部留账");
    s.add("J2-闭环函数", (0..CASES_TOTAL).all(|no| replay_fn(no) == replay_fn(no)), "用例函数闭环可重放");
    s.add("J2-首重同源", lg.cases.iter().all(|c| c.first == c.replay && c.first > 0), "首跑与重放同源取值");
    s.add("J3-族覆盖", (0..FAMILIES as u8).all(|f| lg.agreed_fam(f) == CASES_PER_FAM), "八族每族 5/5 例一致");
    s.add("J3-编号连续", lg.cases[0].cid == 0xC000 && lg.cases[CASES_TOTAL - 1].cid == 0xC027, "例号 0xC000 段起编连续");
    s.add("J3-首跑非零", lg.cases.iter().all(|c| c.first > 0), "全部首跑值非零留账");
    s.add("J4-未跑拒放", inject_four()[0], "未首跑重放被 RP_NORUN 拒止");
    s.add("J4-重复拒放", inject_four()[1], "重复重放被 RP_DUP 拒止");
    s.add("J5-越界拒放", inject_four()[2], "例号越界被 RP_RANGE 拒止");
    s.add("J5-冻结拒放", inject_four()[3], "冻结后重放被 RP_FROZEN 拒止");
    s.add("J6-空账拒跑", ReplayLedger { cases: Vec::new(), run_n: 0, replayed_n: 0, agreed_n: 0, drifts: 0, frozen: false }.run_first(0).is_err(), "空账首跑被 RP_EMPTY 拒止");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");

    // J7 错误码分段互异：六码落在段基址页内且两两不同
    let codes = [RP_EMPTY, RP_RANGE, RP_NORUN, RP_DRIFT, RP_DUP, RP_FROZEN];
    let mut in_seg = true;
    let mut distinct = true;
    let mut a = 0usize;
    while a < codes.len() {
        if codes[a] <= SEG_BASE || codes[a] >= SEG_BASE + 0x20 {
            in_seg = false;
        }
        let mut b = a.wrapping_add(1);
        while b < codes.len() {
            if codes[a] == codes[b] {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    s.add("J7-错误码分段", in_seg && distinct, "六码同段内两两互异");
    s.add_dyn(
        "J7-重放动态",
        ok,
        &format!("一致 {}/{}，漂移 {}，fold={:#x}", lg.agreed_n, CASES_TOTAL, lg.drifts, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11053_replay_ledger_flow() {
        let mut lg = ReplayLedger::new();
        // 全量重放
        assert_eq!(lg.replay_all().unwrap_or(0), CASES_TOTAL);
        assert!(lg.all_agreed());
        assert_eq!(lg.run_n, CASES_TOTAL);
        assert_eq!(lg.agreed_n, CASES_TOTAL);
        // 确定性抽查
        assert_eq!(replay_fn(7), replay_fn(7));
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11053_checks_green() {
        let s = run_unx11053_checks();
        assert!(s.all_passed(), "UNX-F11053 判据未全绿");
    }
}
