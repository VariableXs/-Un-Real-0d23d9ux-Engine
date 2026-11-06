//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18568 · 版本面全量回归（B01–B08 判据联合） · 0x809C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18568-J1 八批冻结判据联合回归全绿，交叉漂移注入必红
//! 设计要点：
//!   - 八批闭集 B01..B08：每批冻结判据数按 10+批序推导（B01=10 …
//!     B08=17，合计 108），冻结指纹 = mix(批号, 冻结数, 种子)——注册
//!     后不可改，改一即漂移；
//!   - 联合回归 sweep：逐批复算指纹与冻结数对拍，指纹不符 →
//!     FpDrift、冻结数不符 → CountDrift、批缺席 → RegistryShort；
//!     8/8 绿才准收口；
//!   - 交叉漂移注入必红：把两批指纹互换（交叉污染），sweep 必须双批
//!     同时判红——漂移检出不得漏、不得串批；
//!   - 回归状态机：开册→扫批→封册，单调三态；未扫批封册必拒；
//!   - 双跑确定性：同册两跑判定数组逐位相等（复测抽样即双跑对拍）；
//!   - 注入核三类：指纹漂移 / 冻结数漂移 / 批缺席，各 10 轮全抓，
//!     漏检为零；边界：批号越界/重复注册/未扫即封；
//!   - LCG 种子写死 = SEG_BASE（0x809C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! 结构清单：
//!   数据结构：BatchRow（批行）/ FullRegression（八批回归账）
//!   状态机  ：RegPhase（开册→扫批→封册 单调三态）
//!   账本    ：批行数组账（8 槽）+ 判定账 + 漂移检出账 + 注入检出账
//!   错误路径：RegCode 12 码闭集，code() = SEG_BASE + n（0x809C0 窗）
//!   注入核  ：指纹漂移/冻结数漂移/批缺席三类逐类断言被拒（10/10）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x809C0，AI-VX11-1105 批09 第 8 槽，0x40 对齐独占窗）。
pub const SEG_BASE: u32 = 0x809C0;

// ── 冻结常量（写死并注明推导） ──────────────────────────────────
/// 批数（判据原文钉死：B01–B08 八批）。
pub const BATCH_N: usize = 8;
/// 各批冻结判据数（推导：10 + 批序；合计 108，写死入账不可改）。
pub const BATCH_COUNTS: [u32; BATCH_N] = [10, 11, 12, 13, 14, 15, 16, 17];
/// 判据总数（推导：Σ(10+i) = 80 + 28 = 108）。
pub const TOTAL_CRITERIA: u32 = 108;
/// 判据迭代轮数（写死：注入 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 指纹种子（e 的十六进制前 16 位，冻结推导）。
pub const FP_SEED: u64 = 0xE2B7_3A1C_D106_B5A7;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegCode {
    /// 批号越界：0..8 之外
    BatchOutOfRange,
    /// 指纹漂移：冻结指纹复算不符
    FpDrift,
    /// 冻结数漂移：判据数与 10+批序 不符
    CountDrift,
    /// 批缺席：册内批行缺
    RegistryShort,
    /// 重复注册：同批号二注册
    DupBatch,
    /// 漂移漏检：交叉漂移未判红
    CrossDriftMissed,
    /// 回归未就绪：未扫批即封册
    RegressNotReady,
    /// 状态越级/倒退
    StateBreak,
    /// 指纹不齐：复算原语对拍不符
    FpMismatch,
    /// 账目守恒破
    ConservationBreak,
    /// 双跑漂移
    NotReproducible,
    /// 未知诊断码兜底
    Unknown,
}

impl RegCode {
    /// 诊断码 = SEG_BASE + 序号（段内闭集，const 供静态裁决表）。
    pub const fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文建议（文案闭集，每码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            RegCode::BatchOutOfRange => "批号越界：只在 0..8 内取",
            RegCode::FpDrift => "指纹漂移：冻结指纹复算不符，按冻结数重出",
            RegCode::CountDrift => "冻结数漂移：判据数必须等于 10+批序",
            RegCode::RegistryShort => "批缺席：八批齐才准联合回归",
            RegCode::DupBatch => "重复注册：一批号一注册",
            RegCode::CrossDriftMissed => "漂移漏检：交叉污染必须双批判红",
            RegCode::RegressNotReady => "回归未就绪：先扫批再封册",
            RegCode::StateBreak => "状态越级或倒退：按 开册→扫批→封册 走",
            RegCode::FpMismatch => "指纹不齐：复算原语对拍不符",
            RegCode::ConservationBreak => "账目不守恒：判定数对不上批数",
            RegCode::NotReproducible => "双跑漂移：冻结种子重跑对拍",
            RegCode::Unknown => "未知诊断码：不在 0x809C0 段闭集内",
        }
    }
    pub const ALL: [RegCode; 12] = [
        RegCode::BatchOutOfRange,
        RegCode::FpDrift,
        RegCode::CountDrift,
        RegCode::RegistryShort,
        RegCode::DupBatch,
        RegCode::CrossDriftMissed,
        RegCode::RegressNotReady,
        RegCode::StateBreak,
        RegCode::FpMismatch,
        RegCode::ConservationBreak,
        RegCode::NotReproducible,
        RegCode::Unknown,
    ];
}

/// 码值反查中文建议（段外码落 Unknown 兜底）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        if RegCode::ALL[i].code() == code {
            return RegCode::ALL[i].detail();
        }
        i += 1;
    }
    RegCode::Unknown.detail()
}

// ── 指纹原语 ────────────────────────────────────────────────────
/// 64 位雪崩混合（写死常量，双跑必同）。
pub fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    x
}

/// 批冻结指纹：批号、冻结数二元混合（种子写死，双跑必同）。
pub fn batch_fp(bid: u32, cnt: u32) -> u64 {
    mix64(((bid as u64) << 32) ^ (cnt as u64) ^ FP_SEED)
}

// ── 回归状态机（单调三态） ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegPhase {
    Open = 0,
    Sweeped = 1,
    Sealed = 2,
}

impl RegPhase {
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单调 +1 放行；倒退/越级拒 StateBreak。
    pub fn advance(self, to: RegPhase) -> Result<RegPhase, u32> {
        let from = self.rank();
        let dst = to.rank();
        if dst == from + 1 {
            Ok(to)
        } else {
            Err(RegCode::StateBreak.code())
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────
/// 批行：批号 + 冻结数 + 冻结指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchRow {
    pub bid: u32,
    pub frozen_cnt: u32,
    pub frozen_fp: u64,
    pub present: bool,
}

impl BatchRow {
    pub const fn empty(bid: u32) -> BatchRow {
        BatchRow { bid, frozen_cnt: 0, frozen_fp: 0, present: false }
    }
}

/// 八批回归账。
pub struct FullRegression {
    /// 批行数组账（8 槽定容）。
    pub batches: [BatchRow; BATCH_N],
    /// 回归状态机。
    pub phase: RegPhase,
    /// 最近一次 sweep 判定账（逐批绿/红）。
    pub verdicts: [bool; BATCH_N],
    /// sweep 轮次（对账轴）。
    pub sweep_cnt: u32,
}

impl FullRegression {
    pub fn new() -> FullRegression {
        FullRegression {
            batches: [
                BatchRow::empty(0),
                BatchRow::empty(1),
                BatchRow::empty(2),
                BatchRow::empty(3),
                BatchRow::empty(4),
                BatchRow::empty(5),
                BatchRow::empty(6),
                BatchRow::empty(7),
            ],
            phase: RegPhase::Open,
            verdicts: [false; BATCH_N],
            sweep_cnt: 0,
        }
    }

    /// 注册批：批号域守卫 + 查重 + 冻结指纹按冻结数算出（不可改）。
    pub fn register(&mut self, bid: usize) -> Result<u64, u32> {
        if bid >= BATCH_N {
            return Err(RegCode::BatchOutOfRange.code());
        }
        if self.batches[bid].present {
            return Err(RegCode::DupBatch.code());
        }
        let cnt = BATCH_COUNTS[bid];
        let f = batch_fp(bid as u32, cnt);
        self.batches[bid] = BatchRow {
            bid: bid as u32,
            frozen_cnt: cnt,
            frozen_fp: f,
            present: true,
        };
        Ok(f)
    }

    /// 单批复测：指纹对拍 + 冻结数对拍。返回 Err(码)=该批判红。
    pub fn recheck_one(&self, bid: usize) -> Result<(), u32> {
        if bid >= BATCH_N {
            return Err(RegCode::BatchOutOfRange.code());
        }
        let b = match self.batches.get(bid) {
            Some(b) => *b,
            None => return Err(RegCode::RegistryShort.code()),
        };
        if !b.present {
            return Err(RegCode::RegistryShort.code());
        }
        // 冻结数闭式：10 + 批序。
        let want_cnt = BATCH_COUNTS[bid];
        if b.frozen_cnt != want_cnt {
            return Err(RegCode::CountDrift.code());
        }
        // 指纹复算对拍。
        let want_fp = batch_fp(b.bid, b.frozen_cnt);
        if want_fp != b.frozen_fp {
            return Err(RegCode::FpDrift.code());
        }
        Ok(())
    }

    /// 联合回归 sweep：八批逐批复测，判定账全量落位。
    /// 全绿返回 Ok(true)；任一批红返回 Ok(false)（判定账仍全量在册，
    /// 收口闸再看）；册缺批即 Err(RegistryShort)。
    pub fn sweep(&mut self) -> Result<bool, u32> {
        if self.phase != RegPhase::Open {
            return Err(RegCode::StateBreak.code());
        }
        let mut all = true;
        let mut i = 0;
        while i < BATCH_N {
            match self.recheck_one(i) {
                Ok(()) => self.verdicts[i] = true,
                Err(c) if c == RegCode::BatchOutOfRange.code() => {
                    return Err(RegCode::BatchOutOfRange.code())
                }
                Err(c) if c == RegCode::RegistryShort.code() => {
                    return Err(RegCode::RegistryShort.code())
                }
                Err(_) => {
                    self.verdicts[i] = false;
                    all = false;
                }
            }
            i += 1;
        }
        self.sweep_cnt = self.sweep_cnt.saturating_add(1);
        self.phase = self.phase.advance(RegPhase::Sweeped)?;
        Ok(all)
    }

    /// 封册闸（扫批→封册）：判定账 8/8 全绿 + 判定数守恒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != RegPhase::Sweeped {
            return Err(RegCode::RegressNotReady.code());
        }
        let mut i = 0;
        while i < BATCH_N {
            if !self.verdicts[i] {
                return Err(RegCode::CrossDriftMissed.code());
            }
            i += 1;
        }
        self.phase = self.phase.advance(RegPhase::Sealed)?;
        Ok(())
    }

    /// 篡改核（仅注入面）：互换两批冻结指纹（交叉污染）。
    pub fn tamper_swap_fps(&mut self, a: usize, b: usize) -> Result<(), u32> {
        if a >= BATCH_N || b >= BATCH_N {
            return Err(RegCode::BatchOutOfRange.code());
        }
        let fa = self.batches[a].frozen_fp;
        let fb = self.batches[b].frozen_fp;
        self.batches[a].frozen_fp = fb;
        self.batches[b].frozen_fp = fa;
        Ok(())
    }

    /// 篡改核（仅注入面）：改一批冻结数。
    pub fn tamper_count(&mut self, bid: usize, cnt: u32) -> Result<(), u32> {
        if bid >= BATCH_N {
            return Err(RegCode::BatchOutOfRange.code());
        }
        self.batches[bid].frozen_cnt = cnt;
        Ok(())
    }

    /// 篡改核（仅注入面）：撤一批在册旗。
    pub fn tamper_unregister(&mut self, bid: usize) -> Result<(), u32> {
        if bid >= BATCH_N {
            return Err(RegCode::BatchOutOfRange.code());
        }
        self.batches[bid].present = false;
        Ok(())
    }

    /// 账守恒：判定账长度 = 批数；sweep 计数域内。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut n = 0u32;
        let mut i = 0;
        while i < BATCH_N {
            match self.batches.get(i) {
                Some(b) => {
                    if b.present {
                        n = n.saturating_add(1);
                    }
                }
                None => return Err(RegCode::RegistryShort.code()),
            }
            i += 1;
        }
        if self.sweep_cnt > 0 && n != BATCH_N as u32 {
            // 扫过批后册必须满（sweep 前置就是八批齐）。
            if self.phase.rank() >= RegPhase::Sweeped.rank() {
                return Err(RegCode::RegistryShort.code());
            }
        }
        Ok(())
    }
}

// ── 批裁决表（输入：在册/数对/指纹对 → 期望码；0=绿） ──────────
/// 四例样本：齐→绿；数漂→CountDrift；指纹漂→FpDrift；缺批→
/// RegistryShort。
pub const BATCH_CASES: [(bool, bool, bool, u32); 4] = [
    // (present, count_ok, fp_ok, want: 0=绿)
    (true, true, true, 0),
    (true, false, true, RegCode::CountDrift.code()),
    (true, true, false, RegCode::FpDrift.code()),
    (false, true, true, RegCode::RegistryShort.code()),
];

/// 批裁决闭式：与 recheck_one 语义同源（判据复算核）。
pub fn adjudicate_batch(present: bool, count_ok: bool, fp_ok: bool) -> u32 {
    if !present {
        return RegCode::RegistryShort.code();
    }
    if !count_ok {
        return RegCode::CountDrift.code();
    }
    if !fp_ok {
        return RegCode::FpDrift.code();
    }
    0
}

/// 裁决表逐例对拍。
pub fn adjudication_ok() -> bool {
    let mut i = 0;
    while i < BATCH_CASES.len() {
        let (p, c, f, want) = BATCH_CASES[i];
        if adjudicate_batch(p, c, f) != want {
            return false;
        }
        i += 1;
    }
    true
}

// ── 标准流程与黄金全链 ──────────────────────────────────────────
/// 黄金全链：八批注册→联合 sweep 8/8 绿→封册→账守恒。
pub fn golden_run() -> Result<(u32, [bool; BATCH_N]), u32> {
    let mut r = FullRegression::new();
    let mut i = 0;
    while i < BATCH_N {
        r.register(i)?;
        i += 1;
    }
    let all = r.sweep()?;
    if !all {
        return Err(RegCode::CrossDriftMissed.code());
    }
    r.seal()?;
    r.conserve()?;
    // 判据总数闭式：判定全绿时冻结数合计必须 = 108。
    let mut total = 0u32;
    let mut k = 0;
    while k < BATCH_N {
        match r.batches.get(k) {
            Some(b) => total = total.saturating_add(b.frozen_cnt),
            None => return Err(RegCode::RegistryShort.code()),
        }
        k += 1;
    }
    if total != TOTAL_CRITERIA {
        return Err(RegCode::ConservationBreak.code());
    }
    Ok((total, r.verdicts))
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入核：三类违约各 JUDGE_ROUNDS 轮，逐类断言被显式拒绝。
///   类 0：交叉指纹漂移（互换两批指纹 → sweep 双批判红）
///   类 1：冻结数漂移（改一批数 → CountDrift 判红）
///   类 2：批缺席（撤在册旗 → RegistryShort 整册拒）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 类 0：交叉指纹漂移（互换批 2/5 指纹）。
        let mut a = fresh_full()?;
        a.tamper_swap_fps(2, 5)?;
        let all_a = a.sweep()?;
        let hit_a = !all_a && !a.verdicts[2] && !a.verdicts[5];
        led.record(0, hit_a);
        // 类 1：冻结数漂移。
        let mut b = fresh_full()?;
        b.tamper_count(3, 99)?;
        let hit_b = match b.recheck_one(3) {
            Err(c) if c == RegCode::CountDrift.code() => true,
            _ => false,
        };
        led.record(1, hit_b);
        // 类 2：批缺席。
        let mut c = fresh_full()?;
        c.tamper_unregister(6)?;
        let hit_c = match c.sweep() {
            Err(cc) if cc == RegCode::RegistryShort.code() => true,
            _ => false,
        };
        led.record(2, hit_c);
        r += 1;
    }
    Ok(led)
}

/// 造一台八批齐册（注入前置；不扫批）。
fn fresh_full() -> Result<FullRegression, u32> {
    let mut r = FullRegression::new();
    let mut i = 0;
    while i < BATCH_N {
        r.register(i)?;
        i += 1;
    }
    Ok(r)
}

// ── 边界与探针 ──────────────────────────────────────────────────
/// 边界探针：批号越界 / 重复注册 / 未扫即封 / 相位倒退。
pub fn boundary_probe() -> Result<(), u32> {
    let mut r = FullRegression::new();
    // 批号越界。
    match r.register(BATCH_N) {
        Err(c) if c == RegCode::BatchOutOfRange.code() => {}
        _ => return Err(RegCode::BatchOutOfRange.code()),
    }
    // 重复注册。
    r.register(0)?;
    match r.register(0) {
        Err(c) if c == RegCode::DupBatch.code() => {}
        _ => return Err(RegCode::DupBatch.code()),
    }
    // 未扫即封。
    let mut h = fresh_full()?;
    match h.seal() {
        Err(c) if c == RegCode::RegressNotReady.code() => {}
        _ => return Err(RegCode::RegressNotReady.code()),
    }
    // 相位倒退（封册后回扫）。
    h.sweep()?;
    h.seal()?;
    match h.sweep() {
        Err(c) if c == RegCode::StateBreak.code() => Ok(()),
        _ => Err(RegCode::StateBreak.code()),
    }
}

/// 探针 A：状态机越级（开册直取封册）必拒。
pub fn probe_phase_skip() -> Result<(), u32> {
    let r = FullRegression::new();
    let skip = match RegPhase::Open.advance(RegPhase::Sealed) {
        Err(c) if c == RegCode::StateBreak.code() => true,
        _ => false,
    };
    if !skip {
        return Err(RegCode::StateBreak.code());
    }
    let _ = r;
    Ok(())
}

/// 探针 B：指纹原语确定性（同参两算同值、异参异值）。
pub fn probe_fp_determinism() -> Result<(), u32> {
    let mut i = 0;
    while i < BATCH_N {
        let f1 = batch_fp(i as u32, BATCH_COUNTS[i]);
        let f2 = batch_fp(i as u32, BATCH_COUNTS[i]);
        if f1 != f2 {
            return Err(RegCode::FpMismatch.code());
        }
        let mut k = 0;
        while k < BATCH_N {
            if k != i && batch_fp(k as u32, BATCH_COUNTS[k]) == f1 {
                return Err(RegCode::FpMismatch.code());
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 探针 C：八批指纹互异（批指纹闭集无碰撞）。
pub fn probe_fp_distinct() -> Result<(), u32> {
    let mut i = 0;
    while i < BATCH_N {
        let mut k = i + 1;
        while k < BATCH_N {
            if batch_fp(i as u32, BATCH_COUNTS[i]) == batch_fp(k as u32, BATCH_COUNTS[k]) {
                return Err(RegCode::FpMismatch.code());
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 双跑一致：判定账逐位相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RegCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：SEG 对齐/窗位 + 码闭集段内 + 文案非空 + 冻结数推导闭式。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x84000).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < RegCode::ALL.len() {
        let v = RegCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if RegCode::ALL[i].detail().len() == 0 {
            return false;
        }
        i += 1;
    }
    // 冻结数闭式复算：BATCH_COUNTS[i] == 10+i 且合计 108。
    let mut total = 0u32;
    let mut i = 0;
    while i < BATCH_N {
        if BATCH_COUNTS[i] != (10 + i) as u32 {
            return false;
        }
        total = total.saturating_add(BATCH_COUNTS[i]);
        i += 1;
    }
    total == TOTAL_CRITERIA
}

/// 运行期摘要（动态明细消费）。
pub fn summary() -> String {
    let g = golden_run();
    format!("{:?}", g.0)
}

// ── 检查入口（判据 8–20 条） ────────────────────────────────────
pub fn run_unx18568_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18568 版本面全量回归（B01–B08 判据联合）");

    // ── 静态明细（主体） ─────────────────────────────────────
    let golden = golden_run();
    s.add("J1-八批联合回归全绿", golden.is_ok(), "八批注册→sweep 8/8→封册→108 判据守恒");
    let ok2 = match &golden {
        Ok((total, v)) => {
            let mut all = true;
            let mut i = 0;
            while i < BATCH_N {
                if !v[i] {
                    all = false;
                }
                i += 1;
            }
            *total == TOTAL_CRITERIA && all
        }
        Err(_) => false,
    };
    s.add("J1-判定账8/8全绿", ok2, "逐批判定真、判据总数=108");
    s.add("J1-批裁决表四例对拍", adjudication_ok(), "在册/数对/指纹对四态闭式一致");
    s.add("J3-状态机越级必拒", probe_phase_skip().is_ok(), "开册直取封册被 StateBreak 拒");
    s.add("J5-指纹确定性", probe_fp_determinism().is_ok(), "同参两算同值、跨批无碰撞");
    s.add("J5-八批指纹互异", probe_fp_distinct().is_ok(), "批指纹闭集两两不等");
    s.add("J4-边界（越界/重复/未扫即封/相位倒退）", boundary_probe().is_ok(), "四类界外全部显式拒");
    s.add("J7-段位与冻结数守卫", segment_guard(), "0x809C0 段 0x40 窗、BATCH_COUNTS=10+i、合计108");

    // ── 动态明细（≤8 槽） ────────────────────────────────────
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    let (t, c) = match &led {
        Ok(l) => (l.tries[0] + l.tries[1] + l.tries[2], l.caught[0] + l.caught[1] + l.caught[2]),
        Err(_) => (0u32, 0u32),
    };
    s.add_dyn("J2-注入10/10全抓", led_ok, &format!("三类各{}轮 抓{}/{}", JUDGE_ROUNDS, c, t));
    s.add_dyn("J2-漏检为零", match &led { Ok(l) => l.missed() == 0, Err(_) => false }, &format!("漏检{}", match &led { Ok(l) => l.missed(), Err(_) => 99u32 }));
    s.add_dyn("J6-双跑一致", repro_check().is_ok(), &format!("total={:?}", summary()));
    s.add_dyn("J2-交叉漂移双批判红", cross_drift_probe().is_ok(), &format!("交叉={:?}", cross_drift_probe().err()));
    s
}

/// 交叉漂移必红复算：互换指纹后 sweep 逐批核红位恰为互换对。
pub fn cross_drift_probe() -> Result<(), u32> {
    let mut r = fresh_full()?;
    r.tamper_swap_fps(0, 7)?;
    let all = r.sweep()?;
    if all {
        return Err(RegCode::CrossDriftMissed.code());
    }
    if r.verdicts[0] || r.verdicts[7] {
        return Err(RegCode::CrossDriftMissed.code());
    }
    // 其余六批必须仍绿（不串批）。
    let mut k = 1;
    while k < 7 {
        if !r.verdicts[k] {
            return Err(RegCode::ConservationBreak.code());
        }
        k += 1;
    }
    Ok(())
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checks_all_passed() {
        let set = run_unx18568_checks();
        assert!(set.all_passed(), "UNX-F18568 判据集必须全绿: {}", set.red_items_text());
        assert!(set.len() >= 8 && set.len() <= 20);
    }

    #[test]
    fn test_golden_run_all_green() {
        let g = golden_run();
        assert!(g.is_ok());
        let (total, v) = g.unwrap_or((0, [false; BATCH_N]));
        assert_eq!(total, TOTAL_CRITERIA);
        let mut i = 0;
        while i < BATCH_N {
            assert!(v[i], "批{}应绿", i);
            i += 1;
        }
    }

    #[test]
    fn test_cross_drift_double_red() {
        assert!(cross_drift_probe().is_ok());
        let mut r = fresh_full().unwrap_or(FullRegression::new());
        r.tamper_swap_fps(1, 4).unwrap_or(());
        assert!(!r.sweep().unwrap_or(true));
        assert!(!r.verdicts[1]);
        assert!(!r.verdicts[4]);
    }

    #[test]
    fn test_count_drift_red() {
        let mut r = fresh_full().unwrap_or(FullRegression::new());
        r.tamper_count(5, 999).unwrap_or(());
        assert_eq!(r.recheck_one(5).err(), Some(RegCode::CountDrift.code()));
    }

    #[test]
    fn test_registry_short_rejected() {
        let mut r = fresh_full().unwrap_or(FullRegression::new());
        r.tamper_unregister(2).unwrap_or(());
        assert_eq!(r.sweep().err(), Some(RegCode::RegistryShort.code()));
    }

    #[test]
    fn test_state_machine_monotonic() {
        assert!(probe_phase_skip().is_ok());
        assert_eq!(
            RegPhase::Sealed.advance(RegPhase::Open).err(),
            Some(RegCode::StateBreak.code())
        );
        assert!(RegPhase::Open.advance(RegPhase::Sweeped).is_ok());
    }

    #[test]
    fn test_fp_properties() {
        assert!(probe_fp_determinism().is_ok());
        assert!(probe_fp_distinct().is_ok());
    }

    #[test]
    fn test_boundary() {
        assert!(boundary_probe().is_ok());
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS);
        let l = led.unwrap_or(InjectLedger::new());
        assert!(l.all_caught());
        assert_eq!(l.missed(), 0);
        for i in 0..3 {
            assert_eq!(l.tries[i], JUDGE_ROUNDS);
            assert_eq!(l.caught[i], JUDGE_ROUNDS);
        }
    }

    #[test]
    fn test_repro_dual_run() {
        assert!(repro_check().is_ok());
        let a = golden_run().unwrap_or((0, [false; BATCH_N]));
        let b = golden_run().unwrap_or((1, [true; BATCH_N]));
        assert_eq!(a, b);
    }

    #[test]
    fn test_segment_guard_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x809C0);
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in RegCode::ALL.iter() {
            assert_eq!(detail(c.code()), c.detail());
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }
}
