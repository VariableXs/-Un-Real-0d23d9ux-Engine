//! cgb861_amdconsist —— CGPU-F0861 AMD 一致性。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0861 AMD 一致性  <SEG 0x7620>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0861；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 一致性矩阵，整数账面）：
//!   1. 一致性维度闭集八维（写死）：行为一致 / 数值一致 / 时序一
//!      致 / 内存一致 / 同步一致 / 错误码一致 / 指纹一致 / 文档一
//!      致；维号 0..=7，越界红码（XC_BAD_DIM）；
//!   2. 每维检查状态机：Idle → Running → Pass / Fail（终态双轨）；
//!      Fail 可重跑（回 Running，重跑计数入账）；
//!   3. 矩阵账（8 维 × 定容 8 轮环）守恒：检查数 = 通过数 + 失败
//!      数 + 在跑数；漂移红码（XC_MATRIX_DRIFT）；
//!   4. 一致性判据：八维 Pass 且每维轮环 ≥ 3 轮，才出一致章；未
//!      齐出章红码（XC_CONSIST_INCOMPLETE）；
//!   5. 轮次账：每维轮号单调递增，回退红码（XC_ROUND_BACKWARD）；
//!   6. 差异登记账（定容 32）：登记差异 = 关闭差异 + 挂起差异；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7620 段为本模块独占（SEG_BASE=0x7620），判别式
//!   SEG_BASE+1..+N；与 0x7600/0x7640 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const CS_VERSION: &str = "cgb861_amdconsist-v1";

/// 诊断段基址（0x7620 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7620;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7621 维号越界（0..=7 之外）。
pub const XC_BAD_DIM: u16 = SEG_BASE + 1;
/// 0x7622 检查状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 2;
/// 0x7623 矩阵账漂移（检查 ≠ 通过 + 失败 + 在跑）。
pub const XC_MATRIX_DRIFT: u16 = SEG_BASE + 3;
/// 0x7624 轮号回退。
pub const XC_ROUND_BACKWARD: u16 = SEG_BASE + 4;
/// 0x7625 一致性未齐即出章。
pub const XC_CONSIST_INCOMPLETE: u16 = SEG_BASE + 5;
/// 0x7626 轮环账漂移。
pub const XC_ROUND_DRIFT: u16 = SEG_BASE + 6;
/// 0x7627 差异账漂移（登记 ≠ 关闭 + 挂起）。
pub const XC_DIFF_DRIFT: u16 = SEG_BASE + 7;
/// 0x7628 差异账满（32）。
pub const XC_DIFF_FULL: u16 = SEG_BASE + 8;
/// 0x7629 重复差异签名。
pub const XC_DUP_DIFF: u16 = SEG_BASE + 9;

/// 维度数（写死：8）。
pub const DIM_CAP: usize = 8;
/// 轮环容量（写死：8）。
pub const ROUND_CAP: usize = 8;
/// 出章最低轮数（写死：3）。
pub const ROUND_MIN: usize = 3;
/// 差异账容量（写死：32）。
pub const DIFF_CAP: usize = 32;

/// 维度闭集。
pub const D_BEHAVIOR: usize = 0;
pub const D_NUMERIC: usize = 1;
pub const D_TIMING: usize = 2;
pub const D_MEMORY: usize = 3;
pub const D_SYNC: usize = 4;
pub const D_ERRCODE: usize = 5;
pub const D_FINGERPRINT: usize = 6;
pub const D_DOC: usize = 7;

/// 维名（闭集原文）。
pub fn dim_name(d: usize) -> &'static str {
    match d {
        D_BEHAVIOR => "behavior",
        D_NUMERIC => "numeric",
        D_TIMING => "timing",
        D_MEMORY => "memory",
        D_SYNC => "sync",
        D_ERRCODE => "errcode",
        D_FINGERPRINT => "fingerprint",
        _ => "doc",
    }
}

/// 检查状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CState {
    /// 空闲。
    Idle,
    /// 在跑。
    Running,
    /// 通过（终态，可重开）。
    Pass,
    /// 失败。
    Fail,
}

/// 单维轮记录。
#[derive(Clone, Copy, Debug)]
pub struct Round {
    /// 轮号。
    pub no: u32,
    /// 结果（true = pass）。
    pub ok: bool,
}

/// 单维账。
pub struct DimLedger {
    /// 状态。
    pub state: CState,
    /// 轮环（定容 8）。
    pub rounds: Vec<Round>,
    /// 环头。
    pub head: usize,
    /// 入账数。
    pub pushed: u64,
    /// 逐出数。
    pub evicted: u64,
    /// 末轮号。
    pub last_no: u32,
    /// 重跑数（Fail→Running）。
    pub reruns: u64,
}

impl DimLedger {
    fn new() -> Self {
        DimLedger {
            state: CState::Idle,
            rounds: Vec::new(),
            head: 0,
            pushed: 0,
            evicted: 0,
            last_no: 0,
            reruns: 0,
        }
    }

    /// 轮账守恒。
    pub fn balanced(&self) -> bool {
        self.pushed == self.rounds.len() as u64 + self.evicted
    }

    /// 连过判定：末 ROUND_MIN 轮全过。
    pub fn streak_pass(&self) -> bool {
        if self.rounds.len() < ROUND_MIN {
            return false;
        }
        let mut k = 0;
        while k < ROUND_MIN {
            let idx = (self.head + self.rounds.len() - 1 - k) % self.rounds.len();
            if !self.rounds[idx].ok {
                return false;
            }
            k += 1;
        }
        true
    }
}

/// AMD 一致性矩阵。
pub struct ConsistMatrix {
    /// 八维账。
    pub dims: [DimLedger; DIM_CAP],
    /// 检查数（开跑计一次）。
    pub checked: u64,
    /// 通过数。
    pub passed: u64,
    /// 失败数。
    pub failed: u64,
    /// 在跑数。
    pub inflight: u64,
    /// 差异账（签名 + 挂起标记）。
    pub diffs: Vec<(String, bool)>,
    /// 登记差异数。
    pub diffs_filed: u64,
    /// 关闭差异数。
    pub diffs_closed: u64,
    /// 挂起差异数。
    pub diffs_open: u64,
    /// 一致章标志。
    pub sealed: bool,
    /// 累计红码。
    pub red_count: u64,
}

impl ConsistMatrix {
    /// 空矩阵。
    pub fn new() -> Self {
        ConsistMatrix {
            dims: [
                DimLedger::new(),
                DimLedger::new(),
                DimLedger::new(),
                DimLedger::new(),
                DimLedger::new(),
                DimLedger::new(),
                DimLedger::new(),
                DimLedger::new(),
            ],
            checked: 0,
            passed: 0,
            failed: 0,
            inflight: 0,
            diffs: Vec::new(),
            diffs_filed: 0,
            diffs_closed: 0,
            diffs_open: 0,
            sealed: false,
            red_count: 0,
        }
    }

    /// 开跑（Idle/Fail → Running）。
    pub fn start(&mut self, dim: usize) -> Result<(), u16> {
        if dim >= DIM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_DIM);
        }
        let cur = self.dims[dim].state;
        if cur != CState::Idle && cur != CState::Fail {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if cur == CState::Fail {
            self.dims[dim].reruns += 1;
            self.failed -= 1;
        } else {
            self.checked += 1;
        }
        self.inflight += 1;
        self.dims[dim].state = CState::Running;
        Ok(())
    }

    /// 结跑（Running → Pass/Fail；轮号单调；落轮环）。
    pub fn finish(&mut self, dim: usize, round: u32, ok: bool) -> Result<(), u16> {
        if dim >= DIM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_DIM);
        }
        if self.dims[dim].state != CState::Running {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        if round <= self.dims[dim].last_no {
            self.red_count += 1;
            return Err(XC_ROUND_BACKWARD);
        }
        let d = &mut self.dims[dim];
        let r = Round { no: round, ok };
        if d.rounds.len() == ROUND_CAP {
            d.rounds[d.head] = r;
            d.head = (d.head + 1) % ROUND_CAP;
            d.evicted += 1;
            d.pushed += 1;
        } else {
            d.rounds.push(r);
            d.pushed += 1;
        }
        d.last_no = round;
        self.inflight -= 1;
        if ok {
            self.passed += 1;
            d.state = CState::Pass;
        } else {
            self.failed += 1;
            d.state = CState::Fail;
        }
        Ok(())
    }

    /// 登记差异（签名去重；账满红）。
    pub fn file_diff(&mut self, sig: &str) -> Result<(), u16> {
        if self.has_diff(sig) {
            self.red_count += 1;
            return Err(XC_DUP_DIFF);
        }
        if self.diffs.len() == DIFF_CAP {
            self.red_count += 1;
            return Err(XC_DIFF_FULL);
        }
        self.diffs.push((String::from(sig), true));
        self.diffs_filed += 1;
        self.diffs_open += 1;
        Ok(())
    }

    /// 签名查重。
    pub fn has_diff(&self, sig: &str) -> bool {
        let mut i = 0;
        while i < self.diffs.len() {
            if self.diffs[i].0 == sig {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 关闭差异。
    pub fn close_diff(&mut self, sig: &str) -> Result<(), u16> {
        let mut i = 0;
        while i < self.diffs.len() {
            if self.diffs[i].0 == sig {
                if !self.diffs[i].1 {
                    self.red_count += 1;
                    return Err(XC_DIFF_DRIFT);
                }
                self.diffs[i].1 = false;
                self.diffs_open -= 1;
                self.diffs_closed += 1;
                return Ok(());
            }
            i += 1;
        }
        Err(XC_DIFF_DRIFT)
    }

    /// 差异账守恒。
    pub fn diff_balanced(&self) -> bool {
        self.diffs_filed == self.diffs_closed + self.diffs_open
    }

    /// 矩阵账守恒。
    pub fn matrix_balanced(&self) -> bool {
        self.checked == self.passed + self.failed + self.inflight
    }

    /// 一致章（八维 Pass 且 streak ≥ 3 且差异清零）。
    pub fn seal(&mut self) -> Result<u32, u16> {
        if self.sealed {
            self.red_count += 1;
            return Err(XC_CONSIST_INCOMPLETE);
        }
        if !self.matrix_balanced() {
            self.red_count += 1;
            return Err(XC_MATRIX_DRIFT);
        }
        let mut d = 0;
        while d < DIM_CAP {
            if self.dims[d].state != CState::Pass {
                return Err(XC_CONSIST_INCOMPLETE);
            }
            if !self.dims[d].streak_pass() {
                return Err(XC_CONSIST_INCOMPLETE);
            }
            if !self.dims[d].balanced() {
                return Err(XC_ROUND_DRIFT);
            }
            d += 1;
        }
        if self.diffs_open != 0 {
            return Err(XC_CONSIST_INCOMPLETE);
        }
        if !self.diff_balanced() {
            return Err(XC_DIFF_DRIFT);
        }
        self.sealed = true;
        Ok(DIM_CAP as u32)
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amdconsist pass=");
        s += &self.passed.to_string();
        s += " fail=";
        s += &self.failed.to_string();
        s += " diffs=";
        s += &self.diffs_open.to_string();
        s += " sealed=";
        s += if self.sealed { "yes" } else { "no" };
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.matrix_balanced() {
            return Err(XC_MATRIX_DRIFT);
        }
        if !self.diff_balanced() {
            return Err(XC_DIFF_DRIFT);
        }
        let mut d = 0;
        while d < DIM_CAP {
            if !self.dims[d].balanced() {
                return Err(XC_ROUND_DRIFT);
            }
            d += 1;
        }
        Ok(())
    }
}

/// 自例程：八维跑满 → 差异登记关闭 → 出章 + 负例五连。
pub fn drill() -> Result<u32, u16> {
    let mut m = ConsistMatrix::new();
    // 负例：维越界。
    if m.start(DIM_CAP) != Err(XC_BAD_DIM) {
        return Err(XC_BAD_DIM);
    }
    // 八维各 3 轮全过。
    let mut d = 0;
    let mut round: u32 = 0;
    while d < DIM_CAP {
        m.start(d)?;
        round += 1;
        m.finish(d, round, true)?;
        m.start(d)?;
        round += 1;
        m.finish(d, round, true)?;
        m.start(d)?;
        round += 1;
        m.finish(d, round, true)?;
        d += 1;
    }
    m.audit()?;
    // 差异路径：Fail 一轮再重跑通过。
    let mut m2 = ConsistMatrix::new();
    m2.start(D_MEMORY)?;
    m2.finish(D_MEMORY, 1, false)?;
    m2.file_diff("mem-race-a")?;
    // 负例：重复签名。
    if m2.file_diff("mem-race-a") != Err(XC_DUP_DIFF) {
        return Err(XC_DUP_DIFF);
    }
    // 负例：未 Pass 出章。
    if m2.seal() != Err(XC_CONSIST_INCOMPLETE) {
        return Err(XC_CONSIST_INCOMPLETE);
    }
    m2.start(D_MEMORY)?;
    m2.finish(D_MEMORY, 2, true)?;
    m2.start(D_MEMORY)?;
    m2.finish(D_MEMORY, 3, true)?;
    // 负例：轮号回退。
    m2.start(D_SYNC)?;
    if m2.finish(D_SYNC, 1, true) != Ok(()) {
        return Err(XC_ROUND_BACKWARD);
    }
    m2.start(D_SYNC)?;
    if m2.finish(D_SYNC, 1, true) != Err(XC_ROUND_BACKWARD) {
        return Err(XC_ROUND_BACKWARD);
    }
    // 补足其余维。
    let mut d2 = 0;
    let mut r2: u32 = 0;
    while d2 < DIM_CAP {
        if d2 != D_MEMORY && d2 != D_SYNC {
            let mut k = 0;
            while k < ROUND_MIN {
                m2.start(d2)?;
                r2 += 1;
                m2.finish(d2, r2, true)?;
                k += 1;
            }
        }
        d2 += 1;
    }
    // D_MEMORY 只 2 轮 → 不足 3 出章红。
    if m2.seal() != Err(XC_CONSIST_INCOMPLETE) {
        return Err(XC_CONSIST_INCOMPLETE);
    }
    m2.start(D_MEMORY)?;
    m2.finish(D_MEMORY, 4, true)?;
    m2.start(D_SYNC)?;
    r2 += 1;
    m2.finish(D_SYNC, r2 + 10, true)?;
    m2.close_diff("mem-race-a")?;
    // 负例：差异未清路径已清，改验关闭再关红。
    if m2.close_diff("mem-race-a") != Err(XC_DIFF_DRIFT) {
        return Err(XC_DIFF_DRIFT);
    }
    m2.audit()?;
    let n = m2.seal()?;
    if n != DIM_CAP as u32 {
        return Err(XC_CONSIST_INCOMPLETE);
    }
    // 负例：已出章再出。
    if m2.seal() != Err(XC_CONSIST_INCOMPLETE) {
        return Err(XC_CONSIST_INCOMPLETE);
    }
    if m.summary().len() == 0 {
        return Err(XC_MATRIX_DRIFT);
    }
    Ok(0)
}
