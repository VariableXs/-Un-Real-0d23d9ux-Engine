//! UNX-F0845 · spurious 中断五态处置账 · 诊断段 0x6680（独占 0x6680~0x668F）
//!
//! **判据原文摘录**：五态判定正确率 100%（注入各态 10/10 次），处置动作与
//! 态匹配零错位。
//!
//! **设计纪律**：
//! 1. 五态闭集（SDM §10.9 spurious 向量 0xFF + 本域扩展）：
//!    真·中断 / 使能期伪中断（SVR bit0=1 且向量非 0xFF）/ 未使能期伪中断
//!    （SVR bit0=0 期到达）/ 风暴期伪中断（率窗密度越限）/ 观察态（带内
//!    带外边界样本）。
//! 2. 判定真源 = const 规则表：`(svr_enabled, vec, storm_flag) → 态` 纯
//!    函数走查；注入面各态 10 次共 50 次对拍，期望态 vs 实判态逐条相等，
//!    50/50 才过（100%）。
//! 3. 处置动作与态一一对应（查表分发）：真中断→正常 EOI 路径；使能期伪→
//!    计数+EOI（SDM：伪中断仍须 EOI）；未使能期伪→丢弃+计数（不 EOI）；
//!    风暴期伪→限流+红账；观察→仅立案计数。双层错位检测：处置计数增量
//!    指纹必须与态对应表期望增量相等，错位数恒为 0。
//! 4. 守恒恒等式：`arrive_total = real + spur_enabled + spur_disabled +
//!    spur_storm + observed`，五分桶和恒等于到达总数。
//! 5. 零 unwrap/expect/panic/浮点/std；事件 tick 内置自增，无墙钟。
//!
//! **诊断段声明**：0x6680~0x668F 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x6680~0x668F 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x6680;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x668F;

/// spurious 向量必须为 0xFF（SDM 固定槽位，其他向量不得走伪中断路径）。
pub const X_S5_VEC_NOT_FF: u16 = 0x6681;
/// SVR 使能态写入非法（bit0 之外的保留位被置位）。
pub const X_S5_SVR_BITS_BAD: u16 = 0x6682;
/// 五态枚举越界（内部不变量破坏）。
pub const X_S5_STATE_RANGE: u16 = 0x6683;
/// 处置错位检出（执行动作与态对应表不符）。
pub const X_S5_DISPO_MISMATCH: u16 = 0x6684;
/// 态误判检出（实判态 vs 期望规则表不符）。
pub const X_S5_MISJUDGE: u16 = 0x6685;
/// 五分桶守恒破缺（arrive ≠ 五桶之和）。
pub const X_S5_BUCKET_UNBALANCED: u16 = 0x6686;
/// 判定演练未达 50/50（正确率 <100%）。
pub const X_S5_MATRIX_NOT_FULL: u16 = 0x6687;
/// 风暴限流配置非法（限流阈为 0 或超窗容）。
pub const X_S5_STORM_CFG: u16 = 0x6688;
/// 处置账满（定容拒绝，降级计数）。
pub const X_S5_LEDGER_FULL: u16 = 0x6689;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_S5_VEC_NOT_FF => "伪中断向量非0xFF",
        X_S5_SVR_BITS_BAD => "SVR保留位置位",
        X_S5_STATE_RANGE => "五态枚举越界",
        X_S5_DISPO_MISMATCH => "处置错位检出",
        X_S5_MISJUDGE => "态误判检出",
        X_S5_BUCKET_UNBALANCED => "五桶守恒破缺",
        X_S5_MATRIX_NOT_FULL => "判定矩阵未满",
        X_S5_STORM_CFG => "风暴限流配置非法",
        X_S5_LEDGER_FULL => "处置账满",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、五态闭集与处置动作表
// ===========================================================================

/// 五态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpurState {
    /// 真·中断（非 0xFF 向量正常到达）。
    RealIrq,
    /// 使能期伪中断（SVR 使能位=1，0xFF 到达）。
    SpurEnabled,
    /// 未使能期伪中断（SVR 使能位=0，0xFF 到达）。
    SpurDisabled,
    /// 风暴期伪中断（伪中断率窗密度越限）。
    SpurStorm,
    /// 观察态（边界样本：恰在阈值上的伪中断）。
    Observed,
}

/// 五处置动作闭集（与态一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dispo {
    /// 正常 EOI 路径（真中断）。
    NormalEoi,
    /// 计数 + EOI（使能期伪中断，SDM 要求仍写 EOI）。
    CountAndEoi,
    /// 丢弃 + 计数，不 EOI（未使能期伪中断，中断线未被确认）。
    DropCount,
    /// 限流 + 红账（风暴期）。
    ThrottleRed,
    /// 仅立案计数（观察态）。
    CaseOnly,
}

impl SpurState {
    /// 态→处置（唯一真源查表）。
    pub const fn dispo(self) -> Dispo {
        match self {
            SpurState::RealIrq => Dispo::NormalEoi,
            SpurState::SpurEnabled => Dispo::CountAndEoi,
            SpurState::SpurDisabled => Dispo::DropCount,
            SpurState::SpurStorm => Dispo::ThrottleRed,
            SpurState::Observed => Dispo::CaseOnly,
        }
    }
    /// 态→处置计数期望增量指纹（双层错位检测基准；与执行层十进位
    /// 位权严格对应：RealIrq=1 / SpurEnabled=1+10 / Disabled=100 /
    /// Storm=1000 / Observed=10000，五值互异）。
    pub const fn dispo_fingerprint(self) -> u64 {
        match self {
            SpurState::RealIrq => 1,
            SpurState::SpurEnabled => 11,
            SpurState::SpurDisabled => 100,
            SpurState::SpurStorm => 1000,
            SpurState::Observed => 10000,
        }
    }
    /// 态序数（桶索引）。
    pub const fn bucket(self) -> usize {
        match self {
            SpurState::RealIrq => 0,
            SpurState::SpurEnabled => 1,
            SpurState::SpurDisabled => 2,
            SpurState::SpurStorm => 3,
            SpurState::Observed => 4,
        }
    }
}

/// 五分桶守恒账：sum(桶) 恒等于 arrive_total。
#[derive(Clone, Copy)]
pub struct Buckets {
    pub arrive_total: u64,
    pub b: [u64; 5],
}

impl Buckets {
    pub const fn new() -> Self {
        Self {
            arrive_total: 0,
            b: [0; 5],
        }
    }
    pub fn add(&mut self, s: SpurState) {
        self.arrive_total += 1;
        self.b[s.bucket()] += 1;
    }
    pub const fn balanced(&self) -> bool {
        self.b[0] + self.b[1] + self.b[2] + self.b[3] + self.b[4] == self.arrive_total
    }
}

// ===========================================================================
// 三、判定规则表（纯函数真源）
// ===========================================================================

/// 判定输入：SVR 使能位 + 到达向量 + 风暴标志。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct JudgeInput {
    /// SVR bit0：伪中断使能。
    pub svr_enabled: bool,
    /// 到达向量号。
    pub vector: u8,
    /// 风暴期标志（率窗密度越限置位）。
    pub storm: bool,
    /// 边界观察标记（率窗恰达阈值的样本，由率窗层传入）。
    pub boundary: bool,
}

/// 判定规则表（const 纯函数，唯一真源）：
/// · 向量 ≠ 0xFF → 真中断（无论 SVR/风暴态）；
/// · 风暴期 0xFF → 风暴态；非风暴期 0xFF 且未使能 → 未使能态；
/// · 非风暴 0xFF 使能且 boundary → 观察态；否则 → 使能态。
pub const fn judge(inp: JudgeInput) -> SpurState {
    if inp.vector != 0xFF {
        return SpurState::RealIrq;
    }
    if inp.storm {
        return SpurState::SpurStorm;
    }
    if !inp.svr_enabled {
        return SpurState::SpurDisabled;
    }
    if inp.boundary {
        return SpurState::Observed;
    }
    SpurState::SpurEnabled
}

// ===========================================================================
// 四、处置账引擎
// ===========================================================================

/// 处置计数器组（双层错位检测的执行层指纹源）。
#[derive(Clone, Copy, Default)]
pub struct DispoCounters {
    pub eoi: u64,
    pub spur_en: u64,
    pub drop: u64,
    pub red: u64,
    pub case: u64,
}

impl DispoCounters {
    /// 执行层指纹：十进位素数位权线性组合（每计数器占独立十进位，
    /// 增量可唯一反解——错位检测的基准函数）。
    pub const fn fingerprint(&self) -> u64 {
        self.eoi + self.spur_en * 10 + self.drop * 100 + self.red * 1000 + self.case * 10000
    }
}

/// 处置引擎：判态→查表处置→执行→双层错位检测。
pub struct DispoEngine {
    pub svr_enabled: bool,
    pub storm: bool,
    pub buckets: Buckets,
    pub counters: DispoCounters,
    /// 错位检出数（判据要求恒 0）。
    pub mismatches: u64,
    /// 处置账（定容环形）。
    ledger: Vec<(u64, SpurState, Dispo)>,
    cap: usize,
    head: usize,
    /// 事件 tick（内置自增）。
    tick: u64,
}

impl DispoEngine {
    pub fn new(cap: usize) -> Self {
        Self {
            svr_enabled: true,
            storm: false,
            buckets: Buckets::new(),
            counters: DispoCounters::default(),
            mismatches: 0,
            ledger: Vec::with_capacity(cap),
            cap,
            head: 0,
            tick: 0,
        }
    }

    /// 一次到达：判定→分桶→查表处置→执行→双层检测。返回实判态。
    pub fn arrive(&mut self, inp: JudgeInput) -> Result<SpurState, u16> {
        self.tick += 1;
        if inp.vector != 0xFF && inp.vector < 32 {
            return Err(X_S5_STATE_RANGE);
        }
        // 期望态（规则表纯走查）。
        let expect = judge(inp);
        // 实判态（引擎判定路径——与规则表同逻辑但独立实现，双保险）。
        let actual = self.decide(&inp);
        if actual != expect {
            self.mismatches += 1;
        }
        self.buckets.add(actual);
        // 查表层处置。
        let want = actual.dispo();
        // 执行层：按处置实际动作计数。
        let before = self.counters.fingerprint();
        self.execute(actual)?;
        let after = self.counters.fingerprint();
        // 双层错位检测：执行层指纹增量必须与态对应表期望增量相等。
        if after - before != actual.dispo_fingerprint() {
            self.mismatches += 1;
            return Err(X_S5_DISPO_MISMATCH);
        }
        // 账目环形入账。
        if self.ledger.len() < self.cap {
            self.ledger.push((self.tick, actual, want));
        } else if self.cap > 0 {
            self.ledger[self.head] = (self.tick, actual, want);
            self.head = (self.head + 1) % self.cap;
        } else {
            return Err(X_S5_LEDGER_FULL);
        }
        // 五桶守恒。
        if !self.buckets.balanced() {
            return Err(X_S5_BUCKET_UNBALANCED);
        }
        Ok(actual)
    }

    /// 引擎独立判定路径（与 const 规则表逻辑等价、实现分离）。
    fn decide(&self, inp: &JudgeInput) -> SpurState {
        if inp.vector != 0xFF {
            SpurState::RealIrq
        } else if self.storm {
            SpurState::SpurStorm
        } else if !self.svr_enabled {
            SpurState::SpurDisabled
        } else if inp.boundary {
            SpurState::Observed
        } else {
            SpurState::SpurEnabled
        }
    }

    /// 执行层：按态实施处置动作（计数增量即执行层证据）。
    fn execute(&mut self, s: SpurState) -> Result<(), u16> {
        match s {
            SpurState::RealIrq => {
                self.counters.eoi += 1;
            }
            SpurState::SpurEnabled => {
                self.counters.eoi += 1;
                self.counters.spur_en += 1;
            }
            SpurState::SpurDisabled => {
                self.counters.drop += 1;
            }
            SpurState::SpurStorm => {
                self.counters.red += 1;
            }
            SpurState::Observed => {
                self.counters.case += 1;
            }
        }
        Ok(())
    }
}

// ===========================================================================
// 五、判定矩阵演练（各态注入 10 次 = 50 次对拍）
// ===========================================================================

/// 矩阵演练结论。
pub struct JudgeMatrix {
    pub cases: u64,
    pub correct: u64,
    pub mismatches: u64,
}

impl JudgeMatrix {
    /// 正确率万分比（10000 = 100%）。
    pub const fn agree_ppm(&self) -> u64 {
        if self.cases == 0 {
            return 0;
        }
        self.correct * 10000 / self.cases
    }
}

/// 注入五态各 10 次：真中断 10 次（向量 40）、使能伪 10、未使能伪 10、
/// 风暴伪 10、观察 10；引擎判态与规则表期望逐条对拍。
pub fn run_judge_matrix() -> Result<JudgeMatrix, u16> {
    let mut eng = DispoEngine::new(64);
    let mut cases = 0u64;
    let mut correct = 0u64;
    // —— 真·中断 10 次。
    let mut i = 0;
    while i < 10 {
        let inp = JudgeInput {
            svr_enabled: true,
            vector: 40 + i as u8,
            storm: false,
            boundary: false,
        };
        eng.svr_enabled = true;
        eng.storm = false;
        let got = eng.arrive(inp)?;
        if got == judge(inp) {
            correct += 1;
        }
        cases += 1;
        i += 1;
    }
    // —— 使能期伪 10 次。
    let mut i = 0;
    while i < 10 {
        let inp = JudgeInput {
            svr_enabled: true,
            vector: 0xFF,
            storm: false,
            boundary: false,
        };
        eng.svr_enabled = true;
        eng.storm = false;
        let got = eng.arrive(inp)?;
        if got == judge(inp) {
            correct += 1;
        }
        cases += 1;
        i += 1;
    }
    // —— 未使能期伪 10 次。
    let mut i = 0;
    while i < 10 {
        let inp = JudgeInput {
            svr_enabled: false,
            vector: 0xFF,
            storm: false,
            boundary: false,
        };
        eng.svr_enabled = false;
        eng.storm = false;
        let got = eng.arrive(inp)?;
        if got == judge(inp) {
            correct += 1;
        }
        cases += 1;
        i += 1;
    }
    // —— 风暴期伪 10 次。
    let mut i = 0;
    while i < 10 {
        let inp = JudgeInput {
            svr_enabled: true,
            vector: 0xFF,
            storm: true,
            boundary: false,
        };
        eng.svr_enabled = true;
        eng.storm = true;
        let got = eng.arrive(inp)?;
        if got == judge(inp) {
            correct += 1;
        }
        cases += 1;
        i += 1;
    }
    // —— 观察态 10 次。
    let mut i = 0;
    while i < 10 {
        let inp = JudgeInput {
            svr_enabled: true,
            vector: 0xFF,
            storm: false,
            boundary: true,
        };
        eng.svr_enabled = true;
        eng.storm = false;
        let got = eng.arrive(inp)?;
        if got == judge(inp) {
            correct += 1;
        }
        cases += 1;
        i += 1;
    }
    Ok(JudgeMatrix {
        cases,
        correct,
        mismatches: eng.mismatches,
    })
}

// ===========================================================================
// 六、drill 自例程
// ===========================================================================

/// 自例程：50/50 判定矩阵 + 处置零错位 + 五桶守恒 + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) 非 0xFF 伪中断路径向量越界闸门（<32 拒绝）。
    let mut eng = DispoEngine::new(8);
    if eng
        .arrive(JudgeInput {
            svr_enabled: true,
            vector: 7,
            storm: false,
            boundary: false,
        })
        .is_err()
    {
        // 向量 7 < 32 且非 0xFF：闸门拒绝为正确行为，不视为错误。
    } else {
        return Ok(1);
    }
    // 2) 主路径：判定矩阵 50/50。
    let m = run_judge_matrix()?;
    if m.cases != 50 || m.correct != 50 {
        return Err(X_S5_MATRIX_NOT_FULL);
    }
    if m.mismatches != 0 {
        return Err(X_S5_MISJUDGE);
    }
    if m.agree_ppm() != 10_000 {
        return Ok(2);
    }
    // 3) 处置与态零错位：engine 独立单跑五类各 1 次。
    let mut eng2 = DispoEngine::new(8);
    let s1 = eng2.arrive(JudgeInput { svr_enabled: true, vector: 60, storm: false, boundary: false })?;
    let s2 = eng2.arrive(JudgeInput { svr_enabled: true, vector: 0xFF, storm: false, boundary: false })?;
    let s3 = eng2.arrive(JudgeInput { svr_enabled: false, vector: 0xFF, storm: false, boundary: false })?;
    let s4 = eng2.arrive(JudgeInput { svr_enabled: true, vector: 0xFF, storm: true, boundary: false })?;
    let s5 = eng2.arrive(JudgeInput { svr_enabled: true, vector: 0xFF, storm: false, boundary: true })?;
    if s1.dispo() != Dispo::NormalEoi
        || s2.dispo() != Dispo::CountAndEoi
        || s3.dispo() != Dispo::DropCount
        || s4.dispo() != Dispo::ThrottleRed
        || s5.dispo() != Dispo::CaseOnly
    {
        return Err(X_S5_DISPO_MISMATCH);
    }
    if eng2.mismatches != 0 {
        return Err(X_S5_DISPO_MISMATCH);
    }
    // 4) 五桶守恒：arrive 5 次 ⇒ 桶和 = 5。
    if !eng2.buckets.balanced() || eng2.buckets.arrive_total != 5 {
        return Err(X_S5_BUCKET_UNBALANCED);
    }
    // 5) 态处置查表完备性：五态处置与指纹闭集。
    let all = [
        SpurState::RealIrq,
        SpurState::SpurEnabled,
        SpurState::SpurDisabled,
        SpurState::SpurStorm,
        SpurState::Observed,
    ];
    let fps = [1u64, 11, 100, 1000, 10000];
    let mut k = 0;
    while k < 5 {
        if all[k].dispo_fingerprint() != fps[k] {
            return Ok(5);
        }
        k += 1;
    }
    // 6) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_S5_VEC_NOT_FF, X_S5_SVR_BITS_BAD, X_S5_STATE_RANGE, X_S5_DISPO_MISMATCH,
        X_S5_MISJUDGE, X_S5_BUCKET_UNBALANCED, X_S5_MATRIX_NOT_FULL, X_S5_STORM_CFG, X_S5_LEDGER_FULL,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(6);
        }
    }
    Ok(0)
}
