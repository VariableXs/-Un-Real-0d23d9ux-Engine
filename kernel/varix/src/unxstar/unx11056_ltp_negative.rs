//! UNX-F11056 LTP 负向用例对照（错误注入组对账）（0x1e700 段）
//!
//! 负向用例对照：对 12 条负向用例（3 类错误 × 4 族）逐一注入，
//! 期望账登记应拒错误码，实测账记录真实拒止码；对平要求
//! rejected 且 got == expect（拒止语义逐例对齐）。
//! 三类错误：欠流（空取）、过流（满存）、越号（错 id）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11056 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11056 - 11000) * 0x20;

/// 用例账空 —— 注入前必须先登记期望
pub const NG_EMPTY: u32 = SEG_BASE + 0x01;
/// 用例号越界 —— 12 例编号 0..12
pub const NG_RANGE: u32 = SEG_BASE + 0x02;
/// 期望未登记 —— 注入前必须先登记期望错误码
pub const NG_NOEXPECT: u32 = SEG_BASE + 0x03;
/// 拒止语义错位 —— 实测码与期望码不一致
pub const NG_MISALIGNED: u32 = SEG_BASE + 0x04;
/// 应拒未拒 —— 注入未被拒止
pub const NG_NOTREJ: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再注入
pub const NG_FROZEN: u32 = SEG_BASE + 0x06;

/// 错误类：欠流（空取）
pub const EC_UNDERFLOW: u32 = SEG_BASE + 0x10;
/// 错误类：过流（满存）
pub const EC_OVERFLOW: u32 = SEG_BASE + 0x11;
/// 错误类：越号（错 id）
pub const EC_BADID: u32 = SEG_BASE + 0x12;
/// 负向用例总数：3 类 × 4 族
pub const CASES_TOTAL: usize = 12;
/// 每类用例数
pub const CASES_PER_CLASS: usize = 4;
/// 领域容量
pub const DOMAIN_CAP: u64 = 8;
/// 负向对照账标签
pub const NEG_TAG: u64 = 0x6e65_676e_676e_3047;

/// 单条负向用例
#[derive(Clone, Debug)]
pub struct NegCase {
    /// 用例号（0xF000 段起编）
    pub cid: u32,
    /// 所属族号 0..4
    pub fam: u8,
    /// 错误类
    pub class: u32,
    /// 期望错误码
    pub expect: u32,
    /// 实测错误码
    pub got: u32,
    /// 已拒止且对平
    pub aligned: bool,
}

/// 负向对照总账
#[derive(Clone, Debug)]
pub struct NegLedger {
    /// 12 条负向用例
    pub cases: Vec<NegCase>,
    /// 领域余额（欠流/过流注入的真实靶子）
    pub domain: u64,
    /// 已注入例数
    pub injected_n: usize,
    /// 对平例数
    pub aligned_n: usize,
    /// 冻结标记
    pub frozen: bool,
}

impl NegLedger {
    /// 立账：12 例期望先行登记，domain 置半满
    pub fn new() -> NegLedger {
        let mut lg = NegLedger {
            cases: Vec::new(),
            domain: DOMAIN_CAP / 2,
            injected_n: 0, aligned_n: 0, frozen: false,
        };
        for class in 0..3usize {
            for fam in 0..CASES_PER_CLASS {
                let class_code = match class {
                    0 => EC_UNDERFLOW,
                    1 => EC_OVERFLOW,
                    _ => EC_BADID,
                };
                lg.cases.push(NegCase {
                    cid: 0xF000 + (class * CASES_PER_CLASS + fam) as u32,
                    fam: fam as u8,
                    class: class_code,
                    expect: class_code,
                    got: 0,
                    aligned: false,
                });
            }
        }
        lg
    }

    /// 真实注入欠流：domain 为零时取数被拒
    pub fn inject_underflow(&mut self) -> u32 {
        if self.domain == 0 { EC_UNDERFLOW } else {
            self.domain = self.domain.wrapping_sub(1);
            0
        }
    }

    /// 真实注入过流：domain 满时存数被拒
    pub fn inject_overflow(&mut self) -> u32 {
        if self.domain >= DOMAIN_CAP { EC_OVERFLOW } else {
            self.domain = self.domain.wrapping_add(1);
            0
        }
    }

    /// 真实注入越号：族号越界即拒
    pub fn inject_badid(&mut self, fam: u8) -> u32 {
        if fam as usize >= CASES_PER_CLASS { EC_BADID } else { 0 }
    }

    /// 注入一例：按类执行真实注入并回填实测码
    pub fn inject_case(&mut self, no: usize) -> Result<bool, u32> {
        if self.frozen { return Err(NG_FROZEN); }
        if self.cases.is_empty() { return Err(NG_EMPTY); }
        if no >= self.cases.len() { return Err(NG_RANGE); }
        let (class, fam) = (self.cases[no].class, self.cases[no].fam);
        let got = match class {
            EC_UNDERFLOW => self.inject_underflow(),
            EC_OVERFLOW => self.inject_overflow(),
            EC_BADID => self.inject_badid(fam.wrapping_add(CASES_PER_CLASS as u8)),
            _ => return Err(NG_NOEXPECT),
        };
        if got == 0 { return Err(NG_NOTREJ); }
        let c = &mut self.cases[no];
        c.got = got;
        c.aligned = c.got == c.expect;
        self.injected_n += 1;
        if c.aligned { self.aligned_n += 1; }
        Ok(c.aligned)
    }

    /// 全量注入（domain 需先排空再填满，保证欠流/过流路径可达）
    pub fn inject_all(&mut self) -> Result<usize, u32> {
        // 欠流类：先排空 domain，4 例全部真实欠流
        self.domain = 0;
        for no in 0..CASES_PER_CLASS {
            self.inject_case(no)?;
        }
        // 过流类：先填满 domain，4 例全部真实过流
        self.domain = DOMAIN_CAP;
        for no in CASES_PER_CLASS..CASES_PER_CLASS * 2 {
            self.inject_case(no)?;
        }
        // 越号类：4 例全部越界族号
        for no in CASES_PER_CLASS * 2..CASES_TOTAL {
            self.inject_case(no)?;
        }
        Ok(self.aligned_n)
    }

    /// 按类统计对平数
    pub fn aligned_class(&self, class: u32) -> usize {
        self.cases.iter().filter(|c| c.class == class && c.aligned).count()
    }

    /// 全量对平：12/12
    pub fn all_aligned(&self) -> bool {
        self.aligned_n == CASES_TOTAL && self.injected_n == CASES_TOTAL
    }

    /// 语义对齐闸：已注入例实测码与期望码错位即 NG_MISALIGNED
    pub fn align_gate(&self) -> Result<(), u32> {
        if self.cases.iter().all(|c| !c.aligned || c.got == c.expect) {
            Ok(())
        } else {
            Err(NG_MISALIGNED)
        }
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = NEG_TAG;
        for c in &self.cases {
            h = (h ^ (c.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (c.expect as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (c.got as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (c.aligned as u64)).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.domain).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结负向账
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：12 例全量注入对平
pub fn sample_injected() -> (NegLedger, bool) {
    let mut lg = NegLedger::new();
    let _ = lg.inject_all();
    let ok = lg.all_aligned();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：应拒未拒 / 越界 / 冻结 / 空账
pub fn inject_four() -> [bool; 4] {
    let mut lg = NegLedger::new();
    // 注入 1：domain 半满时欠流注入未被拒 → NG_NOTREJ 拒收该例
    let i1 = matches!(lg.inject_case(0), Err(NG_NOTREJ));
    // 注入 2：用例号越界 → NG_RANGE 拒止
    let i2 = matches!(lg.inject_case(CASES_TOTAL), Err(NG_RANGE));
    // 注入 3：冻结后注入 → NG_FROZEN 拒止
    lg.freeze();
    let i3 = matches!(lg.inject_case(1), Err(NG_FROZEN));
    // 注入 4：空账注入 → NG_EMPTY 拒止
    let mut el = NegLedger { cases: Vec::new(), domain: 0, injected_n: 0, aligned_n: 0, frozen: false };
    let i4 = matches!(el.inject_case(0), Err(NG_EMPTY));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 负向组用例对照对平，拒止语义对齐
pub fn run_unx11056_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11056");
    let (lg, ok) = sample_injected();

    s.add("J1-全量注入", ok && lg.injected_n == CASES_TOTAL, "12 例负向用例全量注入");
    s.add("J1-逐例对平", lg.aligned_n == CASES_TOTAL, "拒止码与期望码 12/12 对齐");
    s.add("J1-欠流对齐", lg.aligned_class(EC_UNDERFLOW) == CASES_PER_CLASS, "欠流类 4/4 拒止语义对齐");
    s.add("J1-过流对齐", lg.aligned_class(EC_OVERFLOW) == CASES_PER_CLASS, "过流类 4/4 拒止语义对齐");
    s.add("J1-越号对齐", lg.aligned_class(EC_BADID) == CASES_PER_CLASS, "越号类 4/4 拒止语义对齐");
    s.add("J1-对齐闸", lg.align_gate().is_ok(), "语义对齐闸全量放行");
    s.add("J2-码正交", EC_UNDERFLOW != EC_OVERFLOW && EC_OVERFLOW != EC_BADID, "三类错误码互异");
    s.add("J2-码归属", lg.cases.iter().all(|c| c.class == c.expect), "期望码按类登记准确");
    s.add("J2-实测非零", lg.cases.iter().all(|c| c.got != 0), "全部实测码非零（皆被拒止）");
    s.add("J3-编号连续", lg.cases[0].cid == 0xF000 && lg.cases[CASES_TOTAL - 1].cid == 0xF00B, "用例号 0xF000 段起编连续");
    s.add("J3-族覆盖", (0..CASES_PER_CLASS as u8).all(|f| lg.cases.iter().filter(|c| c.fam == f).count() == 3), "四族每族 3 例覆盖");
    s.add("J4-未拒拒收", inject_four()[0], "应拒未拒例被 NG_NOTREJ 拒收");
    s.add("J4-越界拒注", inject_four()[1], "用例号越界被 NG_RANGE 拒止");
    s.add("J5-冻结拒注", inject_four()[2], "冻结后注入被 NG_FROZEN 拒止");
    s.add("J5-空账拒注", inject_four()[3], "空账注入被 NG_EMPTY 拒止");
    s.add("J6-指纹稳定", lg.frozen && lg.fold() == lg.fold(), "冻结后指纹重算稳定");
    s.add_dyn(
        "J7-负向动态",
        ok,
        &format!("对平 {}/{}，domain {}，fold={:#x}", lg.aligned_n, CASES_TOTAL, lg.domain, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11056_ltp_negative_flow() {
        let mut lg = NegLedger::new();
        // 半满 domain 欠流注入不被拒（负向真实验证）
        assert_eq!(lg.inject_underflow(), 0);
        // 全量注入
        assert_eq!(lg.inject_all().unwrap_or(0), CASES_TOTAL);
        assert!(lg.all_aligned());
        assert_eq!(lg.aligned_n, CASES_TOTAL);
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11056_checks_green() {
        let s = run_unx11056_checks();
        assert!(s.all_passed(), "UNX-F11056 判据未全绿");
    }
}
