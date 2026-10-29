//! UNX-F11050 失配归因账（不可对齐用例豁免登记）（0x1e640 段）
//!
//! 外部对照面第 10 批：失配逐例归因留痕，不可对齐用例豁免登记。
//! 20 例对照账中逐例判定：对平 / 失配归因（根因码留痕）/ 豁免登记（理由码留痕）。
//! 守恒式：aligned + mismatched + exempted == 总例数（三态互斥完备）；
//! 豁免率闸：exempted 不超过总例数的四分之一，超闸即拒收。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11050 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1e000 + (11050 - 11000) * 0x20;

/// 账空 —— 归因前必须先登记对照例
pub const MM_EMPTY: u32 = SEG_BASE + 0x01;
/// 例号越界 —— 20 例编号 0..20
pub const MM_RANGE: u32 = SEG_BASE + 0x02;
/// 归因缺失 —— 失配例必须带根因码
pub const MM_NOROOT: u32 = SEG_BASE + 0x03;
/// 豁免超闸 —— 豁免率超过 1/4 拒收
pub const MM_OVERGATE: u32 = SEG_BASE + 0x04;
/// 状态非法 —— 只允许对平/失配/豁免三态
pub const MM_BADSTATE: u32 = SEG_BASE + 0x05;
/// 账已冻结 —— freeze 后禁止再归因
pub const MM_FROZEN: u32 = SEG_BASE + 0x06;

/// 三态：对平
pub const ST_ALIGNED: u8 = 0;
/// 三态：失配
pub const ST_MISMATCH: u8 = 1;
/// 三态：豁免
pub const ST_EXEMPT: u8 = 2;
/// 对照总例数
pub const CASES_TOTAL: usize = 20;
/// 豁免闸：最多豁免 5 例（1/4）
pub const EXEMPT_GATE: usize = 5;
/// 失配归因账标签
pub const MISMTAG: u64 = 0x6d69_736d_6d74_3041;

/// 根因码：域语义差
pub const ROOT_SEMANTIC: u32 = 0x1001;
/// 根因码：环境差
pub const ROOT_ENV: u32 = 0x1002;
/// 根因码：精度差
pub const ROOT_PRECISION: u32 = 0x1003;
/// 豁免码：平台不可复现
pub const EXEMPT_NOENV: u32 = 0x2001;
/// 豁免码：依赖外部时钟
pub const EXEMPT_CLOCK: u32 = 0x2002;

/// 单条对照例：状态 + 归因码
#[derive(Clone, Debug)]
pub struct MmCase {
    /// 对照例号
    pub cid: u32,
    /// 用例名
    pub name: String,
    /// 三态状态
    pub state: u8,
    /// 期望值
    pub expect: u64,
    /// 实测值
    pub actual: u64,
    /// 根因码或豁免码（对平例为 0）
    pub code: u32,
}

/// 失配归因总账
#[derive(Clone, Debug)]
pub struct MismatchLedger {
    /// 20 条对照例
    pub cases: Vec<MmCase>,
    /// 对平例数
    pub aligned_n: usize,
    /// 失配例数
    pub mismatch_n: usize,
    /// 豁免例数
    pub exempt_n: usize,
    /// 已归因例数（失配+豁免合计）
    pub attributed_n: usize,
    /// 冻结标记
    pub frozen: bool,
}

impl MismatchLedger {
    /// 立账：20 例期望先行登记，实测留空
    pub fn new() -> MismatchLedger {
        let mut lg = MismatchLedger {
            cases: Vec::new(),
            aligned_n: 0, mismatch_n: 0, exempt_n: 0,
            attributed_n: 0, frozen: false,
        };
        for i in 0..CASES_TOTAL {
            lg.cases.push(MmCase {
                cid: 0x9000 + i as u32,
                name: "mm_case".to_string(),
                state: ST_ALIGNED,
                expect: ((i as u64) + 1) * 11,
                actual: 0,
                code: 0,
            });
        }
        lg
    }

    /// 回填实测并三态判定：相等对平，不等进入待归因（失配态）
    pub fn fill(&mut self, no: usize, actual: u64) -> Result<bool, u32> {
        if self.frozen { return Err(MM_FROZEN); }
        if self.cases.is_empty() { return Err(MM_EMPTY); }
        if no >= self.cases.len() { return Err(MM_RANGE); }
        let c = &mut self.cases[no];
        c.actual = actual;
        let hit = c.expect == actual;
        c.code = 0;
        c.state = if hit { ST_ALIGNED } else { ST_MISMATCH };
        self.retally();
        Ok(hit)
    }

    /// 依三态重排计数：计数恒由各例状态导出，空例/重填/转态均不漂移。
    fn retally(&mut self) {
        let mut a = 0usize;
        let mut m = 0usize;
        let mut e = 0usize;
        let mut att = 0usize;
        let mut k = 0usize;
        while k < self.cases.len() {
            match self.cases[k].state {
                ST_ALIGNED => a += 1,
                ST_MISMATCH => {
                    m += 1;
                    if self.cases[k].code != 0 { att += 1; }
                }
                _ => {
                    e += 1;
                    if self.cases[k].code != 0 { att += 1; }
                }
            }
            k += 1;
        }
        self.aligned_n = a;
        self.mismatch_n = m;
        self.exempt_n = e;
        self.attributed_n = att;
    }

    /// 失配例归因：必须带根因码
    pub fn attribute(&mut self, no: usize, root: u32) -> Result<(), u32> {
        if self.frozen { return Err(MM_FROZEN); }
        if no >= self.cases.len() { return Err(MM_RANGE); }
        if self.cases[no].state != ST_MISMATCH { return Err(MM_BADSTATE); }
        if root == 0 { return Err(MM_NOROOT); }
        self.cases[no].code = root;
        self.retally();
        Ok(())
    }

    /// 豁免登记：仅对平/失配态可转豁免，且不得超闸
    pub fn exempt(&mut self, no: usize, reason: u32) -> Result<(), u32> {
        if self.frozen { return Err(MM_FROZEN); }
        if no >= self.cases.len() { return Err(MM_RANGE); }
        if reason == 0 { return Err(MM_NOROOT); }
        if self.exempt_n >= EXEMPT_GATE { return Err(MM_OVERGATE); }
        let c = &mut self.cases[no];
        if c.state == ST_EXEMPT { return Err(MM_BADSTATE); }
        c.state = ST_EXEMPT;
        c.code = reason;
        self.retally();
        Ok(())
    }

    /// 三态守恒：三态计数之和等于总例数
    pub fn conserved(&self) -> bool {
        self.aligned_n + self.mismatch_n + self.exempt_n == self.cases.len()
    }

    /// 豁免率闸内：豁免例数不超过闸值
    pub fn in_gate(&self) -> bool {
        self.exempt_n <= EXEMPT_GATE
    }

    /// 归因完备：所有失配与豁免例均带非零码
    pub fn attributed_all(&self) -> bool {
        self.cases.iter().all(|c| match c.state {
            ST_ALIGNED => c.code == 0,
            _ => c.code != 0,
        })
    }

    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = MISMTAG;
        for c in &self.cases {
            h = (h ^ (c.cid as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ c.expect).wrapping_mul(0x100000001b3);
            h = (h ^ c.actual).wrapping_mul(0x100000001b3);
            h = (h ^ (c.state as u64)).wrapping_mul(0x100000001b3);
            h = (h ^ (c.code as u64)).wrapping_mul(0x100000001b3);
        }
        h = (h ^ (self.attributed_n as u64)).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结归因账
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        self.fold()
    }
}

/// 确定性样本：14 对平 + 3 失配归因 + 3 豁免
pub fn sample_attributed() -> (MismatchLedger, bool) {
    let mut lg = MismatchLedger::new();
    // 例 0..14 对平
    for no in 0..14 {
        let expect = lg.cases[no].expect;
        let _ = lg.fill(no, expect);
    }
    // 例 14..17 失配并归因
    for no in 14..17 {
        let _ = lg.fill(no, lg.cases[no].expect + 1);
        let root = if no == 14 { ROOT_SEMANTIC } else if no == 15 { ROOT_ENV } else { ROOT_PRECISION };
        let _ = lg.attribute(no, root);
    }
    // 例 17..20 豁免登记
    for no in 17..20 {
        let reason = if no == 17 { EXEMPT_NOENV } else { EXEMPT_CLOCK };
        let _ = lg.exempt(no, reason);
    }
    let ok = lg.conserved() && lg.in_gate() && lg.attributed_all();
    lg.freeze();
    (lg, ok)
}

/// 注入路径：无根因归因 / 非失配归因 / 豁免超闸 / 冻结拒写
pub fn inject_four() -> [bool; 4] {
    let mut lg = MismatchLedger::new();
    // 注入 1：例 0 尚未回填（对平态）→ 归因被 MM_BADSTATE 拒止
    let i1 = matches!(lg.attribute(0, ROOT_SEMANTIC), Err(MM_BADSTATE));
    // 注入 2：回填失配后用零根因归因 → MM_NOROOT 拒止
    let _ = lg.fill(1, 0);
    let i2 = matches!(lg.attribute(1, 0), Err(MM_NOROOT));
    // 注入 3：连豁免 6 例 → 第 6 次被 MM_OVERGATE 拒止
    let mut i3 = false;
    for no in 2..8 {
        let r = lg.exempt(no, EXEMPT_NOENV);
        if no == 7 && matches!(r, Err(MM_OVERGATE)) { i3 = true; }
    }
    // 注入 4：冻结后回填 → MM_FROZEN 拒止
    lg.freeze();
    let i4 = matches!(lg.fill(9, 1), Err(MM_FROZEN));
    [i1, i2, i3, i4]
}

/// 判据检查：J1 失配逐例归因留痕，豁免率在闸内
pub fn run_unx11050_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11050");
    let (lg, ok) = sample_attributed();

    s.add("J1-三态完备", lg.conserved() && lg.cases.len() == CASES_TOTAL, "对平/失配/豁免三态互斥完备守恒");
    s.add("J1-失配留痕", lg.mismatch_n == 0 || lg.cases.iter().filter(|c| c.state == ST_MISMATCH).all(|c| c.code != 0), "失配例全部带根因码留痕");
    s.add("J1-豁免登记", lg.exempt_n == 3, "不可对齐用例 3 例豁免登记");
    s.add("J2-豁免闸内", lg.in_gate(), "豁免率不超过 1/4 闸值");
    s.add("J2-归因完备", lg.attributed_all(), "对平例零码、失配与豁免例非零码");
    s.add("J2-归因计数", lg.attributed_n == lg.mismatch_n + lg.exempt_n, "归因计数与失配加豁免合计一致");
    s.add("J3-对平计数", lg.aligned_n == 14, "对平 14 例计数准确");
    s.add("J3-根因分布", lg.cases.iter().filter(|c| c.code == ROOT_SEMANTIC).count() == 1, "根因码逐例可区分");
    s.add("J3-豁免码合法", lg.cases.iter().filter(|c| c.state == ST_EXEMPT).all(|c| c.code == EXEMPT_NOENV || c.code == EXEMPT_CLOCK), "豁免码取自登记表");
    s.add("J4-编号连续", lg.cases[0].cid == 0x9000 && lg.cases[CASES_TOTAL - 1].cid == 0x9013, "对照例号 0x9000 段起编连续");
    let mut el = MismatchLedger { cases: Vec::new(), aligned_n: 0, mismatch_n: 0, exempt_n: 0, attributed_n: 0, frozen: false };
    let mut lg2 = MismatchLedger::new();
    let r2 = lg2.fill(0, lg2.cases[0].expect);
    s.add("J4-空账拒填", matches!(el.fill(0, 1), Err(MM_EMPTY)) && matches!(r2, Ok(true)), "空账回填被 MM_EMPTY 拒止、实账放行");
    s.add("J4-期望非零", lg.cases.iter().all(|c| c.expect > 0), "全部期望账非零登记");
    s.add("J5-状态拒判", inject_four()[0], "非失配态归因被 MM_BADSTATE 拒止");
    s.add("J5-零码拒因", inject_four()[1], "零根因码被 MM_NOROOT 拒止");
    s.add("J5-超闸拒免", inject_four()[2], "豁免超闸被 MM_OVERGATE 拒止");
    s.add("J6-冻结拒写", inject_four()[3], "冻结后回填被 MM_FROZEN 拒止");
    s.add_dyn(
        "J7-归因动态",
        ok,
        &format!("对平 {} 失配 {} 豁免 {}/{}，归因 {}，fold={:#x}", lg.aligned_n, lg.mismatch_n, lg.exempt_n, EXEMPT_GATE, lg.attributed_n, lg.fold()),
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11050_mismatch_ledger_flow() {
        let mut lg = MismatchLedger::new();
        // 越界拒回填
        assert!(matches!(lg.fill(99, 0), Err(MM_RANGE)));
        // 样本复放
        for no in 0..14 {
            let e = lg.cases[no].expect;
            assert!(lg.fill(no, e).unwrap_or(false));
        }
        for no in 14..17 {
            let e = lg.cases[no].expect;
            assert!(!lg.fill(no, e + 1).unwrap_or(true));
            let root = if no == 14 { ROOT_SEMANTIC } else if no == 15 { ROOT_ENV } else { ROOT_PRECISION };
            assert!(lg.attribute(no, root).is_ok());
        }
        for no in 17..20 {
            let reason = if no == 17 { EXEMPT_NOENV } else { EXEMPT_CLOCK };
            assert!(lg.exempt(no, reason).is_ok());
        }
        assert!(lg.conserved());
        assert_eq!(lg.aligned_n, 14);
        assert_eq!(lg.mismatch_n, 3);
        assert_eq!(lg.exempt_n, 3);
        assert!(lg.in_gate() && lg.attributed_all());
        let root = lg.freeze();
        assert!(lg.frozen);
        assert_eq!(root, lg.fold());
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11050_checks_green() {
        let s = run_unx11050_checks();
        assert!(s.all_passed(), "UNX-F11050 判据未全绿");
    }
}
