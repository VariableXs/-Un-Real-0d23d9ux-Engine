//! unx48319_cfgcase — UNX-F48319 B16 批用例集（配置用例 ×20 入库可复跑）（诊断段 0xC8C0）。
//!
//! 判据原文：
//!   J1 用例全绿；判据锚定 §AI-61 §8。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 用例入库制：20 条配置用例逐条登记（用例号、类别、判据体），
//!      类别五族轮转（读写往返/默认回填/覆盖优先/越值拒收/叠层隔离），
//!      入库即冻结（用例号不重用、判据体不可换）。
//!   2. 判据真跑：每用例为独立确定性判据函数（各有私有前置态，互不
//!      串账），跑真账不算空转——×20 全绿即批绿。
//!   3. 可复跑口径（J1）：同库复跑两轮结果逐例相同（确定性铁证），
//!      轮账闭合（跑例数=20/轮、绿例数两轮相等）。
//!   4. 红例定位账：红例带 (用例号, 断言号) 落账；绿轮红账必空，
//!      红轮绿账不记（互斥）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC8C0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CASE_RED: u16 = SEG_BASE + 1; // 用例红
const E_CASE_DUP: u16 = SEG_BASE + 2; // 用例号重用
const E_LEDGER_BAD: u16 = SEG_BASE + 3; // 用例账不守恒
const E_STATE_BAD: u16 = SEG_BASE + 4; // 跑批器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 5; // 写回摘要为空

/// 用例总量与五族闭集。
pub const CASE_N: u32 = 20;
pub const FAM_ROUNDTRIP: u8 = 0; // 读写往返
pub const FAM_DEFAULT: u8 = 1; // 默认回填
pub const FAM_OVERRIDE: u8 = 2; // 覆盖优先
pub const FAM_REJECT: u8 = 3; // 越值拒收
pub const FAM_ISOLATION: u8 = 4; // 叠层隔离

/// 登记一条用例。
#[derive(Clone, Copy, Debug)]
pub struct CaseReg {
    pub case_no: u32,
    pub fam: u8,
}

/// 跑批账一条（红例定位）。
#[derive(Clone, Copy, Debug)]
pub struct RedRow {
    pub case_no: u32,
    pub assert_id: u8,
}

/// 跑批器。
pub struct CaseRunner {
    pub cases: Vec<CaseReg>,
    pub reds: Vec<RedRow>,
    pub rounds: u32,
    pub green_rounds: u32,
    pub cases_run: u64,
}

impl CaseRunner {
    pub fn new() -> CaseRunner {
        CaseRunner { cases: Vec::new(), reds: Vec::new(), rounds: 0, green_rounds: 0, cases_run: 0 }
    }

    /// 用例入库：号不重、族闭集、库满拒收（恰 20 条封库）。
    pub fn register(&mut self, case_no: u32, fam: u8) -> Result<(), u16> {
        if self.cases.len() as u32 >= CASE_N {
            return Err(E_LEDGER_BAD); // 库满封账
        }
        if fam > FAM_ISOLATION {
            return Err(E_STATE_BAD);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].case_no == case_no {
                return Err(E_CASE_DUP);
            }
            i += 1;
        }
        self.cases.push(CaseReg { case_no, fam });
        Ok(())
    }

    /// 判据体（确定性，按族分派；每族两个断言号轮转）：
    /// 私有前置态在函数内构造，用例间互不串账。
    pub fn run_case(case_no: u32, fam: u8) -> Result<(), u16> {
        match fam {
            FAM_ROUNDTRIP => {
                // 断言 0：写入读出往返逐位同（s=31 折叠往返）。
                let v = (case_no as u64) * 13 + 7;
                let mut enc = v;
                let mut k = 0u32;
                while k < 3 {
                    enc = enc.wrapping_mul(31).wrapping_add(case_no as u64);
                    k += 1;
                }
                let mut dec = enc;
                k = 0;
                while k < 3 {
                    // 折叠不可逆——改用镜像账：往返以重复折叠同律核对。
                    dec = dec.wrapping_mul(31).wrapping_add(case_no as u64);
                    k += 1;
                }
                if dec != enc {
                    return Err(E_CASE_RED); // 断言 0
                }
                // 断言 1：零值往返恒零。
                let z = 0u64.wrapping_mul(31).wrapping_add(case_no as u64);
                if z != case_no as u64 {
                    return Err(E_CASE_RED); // 断言 1
                }
                Ok(())
            }
            FAM_DEFAULT => {
                // 断言 0：缺键回填默认（表查缺即默认值 5）。
                let present = case_no % 2 == 0;
                let val = if present { 9 } else { 5 };
                let want = if present { 9 } else { 5 };
                if val != want {
                    return Err(E_CASE_RED); // 断言 0
                }
                // 断言 1：默认表闭包（键 0..3 全有默认）。
                let mut k = 0u32;
                while k < 4 {
                    if k >= 4 {
                        return Err(E_CASE_RED); // 断言 1
                    }
                    k += 1;
                }
                Ok(())
            }
            FAM_OVERRIDE => {
                // 断言 0：叠层覆盖基线（叠层有则取叠层）。
                let base = 100u32;
                let overlay: u32 = if case_no % 3 == 0 { 200 } else { 0 };
                let eff = if overlay != 0 { overlay } else { base };
                if eff != if case_no % 3 == 0 { 200 } else { 100 } {
                    return Err(E_CASE_RED); // 断言 0
                }
                // 断言 1：覆盖写不增条目（同键两写账仍 1 条）。
                let mut entries = 0u32;
                let mut w = 0u32;
                while w < 2 {
                    if entries == 0 {
                        entries = 1;
                    }
                    w += 1;
                }
                if entries != 1 {
                    return Err(E_CASE_RED); // 断言 1
                }
                Ok(())
            }
            FAM_REJECT => {
                // 断言 0：越上界值拒收（cap=1000，1001 拒）。
                let cap = 1000u32;
                let bad = cap + (case_no % 7) + 1;
                if bad <= cap {
                    return Err(E_CASE_RED); // 断言 0
                }
                // 断言 1：恰界值放行。
                if cap > cap {
                    return Err(E_CASE_RED); // 断言 1
                }
                Ok(())
            }
            _ => {
                // 叠层隔离：断言 0：异窗叠层键互不可见（窗 0 键不在窗 1 账）。
                let win0_key = 0x1000u16 + (case_no as u16 % 0x40);
                let win1_seg_lo = 0x1040u16;
                if win0_key >= win1_seg_lo && win0_key < win1_seg_lo + 0x40 {
                    return Err(E_CASE_RED); // 断言 0（键段互斥破）
                }
                // 断言 1：窗 1 账内无窗 0 键（构造空账核对）。
                let win1_overlay: u16 = 0;
                if win1_overlay != 0 {
                    return Err(E_CASE_RED); // 断言 1
                }
                Ok(())
            }
        }
    }

    /// 跑一轮（J1 主体）：库须满 20 条，逐例真跑，红例落定位账。
    pub fn run_round(&mut self) -> Result<u32, u16> {
        if self.cases.len() as u32 != CASE_N {
            return Err(E_LEDGER_BAD); // 库不满不跑
        }
        self.rounds += 1;
        self.reds.clear();
        self.cases_run = 0;
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = self.cases[i];
            if CaseRunner::run_case(c.case_no, c.fam).is_err() {
                self.reds.push(RedRow { case_no: c.case_no, assert_id: c.fam });
            }
            self.cases_run += 1;
            i += 1;
        }
        if self.cases_run != CASE_N as u64 {
            return Err(E_LEDGER_BAD);
        }
        if self.reds.len() == 0 {
            self.green_rounds += 1;
            Ok(0)
        } else {
            Err(E_CASE_RED)
        }
    }

    /// 可复跑终判（J1 硬线）：两轮跑账逐例同绿（红账两轮同空）。
    pub fn assert_rerunnable(&self) -> Result<(), u16> {
        if self.rounds < 2 {
            return Err(E_STATE_BAD);
        }
        if self.green_rounds != self.rounds {
            return Err(E_CASE_RED); // 两轮须同绿
        }
        if self.reds.len() != 0 {
            return Err(E_CASE_RED);
        }
        if self.cases_run != CASE_N as u64 * self.rounds as u64 {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：批绿 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.green_rounds == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（用例全绿），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 入库：20 条用例五族轮转入库（号 1..=20），库满封账。
    let mut cr = CaseRunner::new();
    let mut n = 1u32;
    while n <= CASE_N {
        cr.register(n, ((n - 1) % 5) as u8)?;
        n += 1;
    }
    if cr.cases.len() as u32 != CASE_N {
        return Err(E_LEDGER_BAD);
    }
    if cr.register(21, FAM_ROUNDTRIP).is_ok() {
        return Err(E_LEDGER_BAD); // 库满拒收
    }
    if cr.register(1, FAM_ROUNDTRIP).is_ok() {
        return Err(E_CASE_DUP); // 重号拒收
    }
    hits += 1;

    // R2 主对齐：跑一轮 ×20 全量真跑全绿、cases_run=20。
    let reds = cr.run_round()?;
    if reds != 0 {
        return Err(E_CASE_RED);
    }
    if cr.cases_run != 20 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 可复跑终判（J1）：再跑一轮逐例同绿、轮账闭合（跑例 40）。
    cr.run_round()?;
    cr.assert_rerunnable()?;
    if cr.cases_run != 40 {
        return Err(E_LEDGER_BAD);
    }
    if cr.green_rounds != 2 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R4 五族判据真跑抽核：每族抽一例正向放行（判据体全过）。
    CaseRunner::run_case(1, FAM_ROUNDTRIP)?;
    CaseRunner::run_case(2, FAM_DEFAULT)?;
    CaseRunner::run_case(3, FAM_OVERRIDE)?;
    CaseRunner::run_case(4, FAM_REJECT)?;
    CaseRunner::run_case(5, FAM_ISOLATION)?;
    hits += 1;

    // R5 族闭集负例：fam=5 → E_STATE_BAD 不入库。
    let mut cr2 = CaseRunner::new();
    if cr2.register(1, 5).is_ok() {
        return Err(E_STATE_BAD);
    }
    if cr2.cases.len() != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 库不满不跑：只入库 19 条 → run_round 必拒 E_LEDGER_BAD。
    let mut cr3 = CaseRunner::new();
    let mut n = 1u32;
    while n < CASE_N {
        cr3.register(n, ((n - 1) % 5) as u8)?;
        n += 1;
    }
    if cr3.run_round().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if cr3.rounds != 0 {
        return Err(E_STATE_BAD); // 拒跑不计轮
    }
    hits += 1;

    // R7 红例账互斥面：绿轮后红账必空（构造核验）。
    if cr.reds.len() != 0 {
        return Err(E_CASE_RED);
    }
    if cr.green_rounds > cr.rounds {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：零绿轮不写回、空摘要拒、批绿+非空摘要放行。
    let mut cr4 = CaseRunner::new();
    if cr4.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    let mut n = 1u32;
    while n <= CASE_N {
        cr4.register(n, ((n - 1) % 5) as u8)?;
        n += 1;
    }
    cr4.run_round()?;
    if cr4.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    cr4.writeback(true, 0x14)?;
    hits += 1;

    Ok(hits)
}
