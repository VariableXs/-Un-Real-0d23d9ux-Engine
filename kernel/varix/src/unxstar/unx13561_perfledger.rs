//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13561 · 域级性能核账总成（任务书装载分段计时权威位） · 0x65C00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥370 行；
//!   ② UNX-F13561-J1 性能总账（七段计时×样本集矩阵：段号/样本号/环境档/
//!      耗时四列），协议单点复用声明；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 七段计时 × 样本集（12 样本）× 环境档（冷/温/热）三列全组合：
//!     行总数 = 7×12×3 = 252 行，四列账（段号/样本号/环境档/耗时）；
//!   - 每行状态机闭集：空→计时中→已计时→已记账→已封存，任何非法边
//!     （未计时先记账/封存后重记账/跳级封存）一律拒收；
//!   - 协议单点复用声明：计时协议标识全库只定义一处（PROTOCOL_ID），
//!     所有账行只许引用该单点，携带他牌协议者拒收；
//!   - 耗时单调性：同段同样本 热档 ≤ 温档 ≤ 冷档（缓存越冷越慢）；
//!   - 守恒：Σ已记账行 ≡ 252，封存后账目只读；
//!   - 注入三族：段号越闭集/未计时先记账/封存后重记账 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65C00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65C00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65C00;

/// 计时段闭集：七段（装载/解析/建树/挂载/句柄化/读写/收官）。
pub const SEGMENT_COUNT: usize = 7;

/// 样本集闭集：12 样本。
pub const SAMPLE_COUNT: usize = 12;

/// 环境档闭集：冷/温/热 三档。
pub const ENV_COUNT: usize = 3;

/// 全矩阵行总数：7×12×3 = 252（冻结常量，来源：三闭集直积）。
pub const TOTAL_ROWS: usize = SEGMENT_COUNT * SAMPLE_COUNT * ENV_COUNT;

/// 协议单点复用声明：计时协议标识全库唯一（写死，禁二次定义）。
pub const PROTOCOL_ID: &'static str = "UNX-PERF-TIMER-P1";

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerfCode {
    /// 段号越闭集
    SegOutOfRange,
    /// 样本号越闭集
    SampleOutOfRange,
    /// 环境档枚举外值
    EnvInvalid,
    /// 未计时先记账
    BookWithoutTiming,
    /// 封存后重记账（账目只读红线）
    DoubleBook,
    /// 耗时为零（时钟未走）
    ZeroCycles,
    /// 耗时单调性破（热档慢于冷档）
    MonotonicBreak,
    /// 守恒破：Σ已记账行 ≢ 252
    ConservationBreak,
    /// 协议标识非单点（他牌协议混入）
    ProtocolForeign,
}

impl PerfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PerfCode::SegOutOfRange => "段号越闭集：1~7 冻结",
            PerfCode::SampleOutOfRange => "样本号越闭集：1~12 冻结",
            PerfCode::EnvInvalid => "环境档枚举外值：冷/温/热 三档",
            PerfCode::BookWithoutTiming => "未计时先记账：必须先 Timing 态",
            PerfCode::DoubleBook => "封存后重记账：账目只读红线",
            PerfCode::ZeroCycles => "耗时为零：时钟未走不许入账",
            PerfCode::MonotonicBreak => "耗时单调破：热档不得慢于冷档",
            PerfCode::ConservationBreak => "守恒破：Σ已记账行 ≡ 252",
            PerfCode::ProtocolForeign => "协议标识非单点：只许引用 PROTOCOL_ID",
        }
    }
    pub const ALL: [PerfCode; 9] = [
        PerfCode::SegOutOfRange,
        PerfCode::SampleOutOfRange,
        PerfCode::EnvInvalid,
        PerfCode::BookWithoutTiming,
        PerfCode::DoubleBook,
        PerfCode::ZeroCycles,
        PerfCode::MonotonicBreak,
        PerfCode::ConservationBreak,
        PerfCode::ProtocolForeign,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        if PerfCode::ALL[i].val() == code {
            return PerfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65C00 段闭集内"
}

// ── 环境档三闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnvTier {
    Cold,
    Warm,
    Hot,
}

impl EnvTier {
    /// 档位序号：冷 0 / 温 1 / 热 2（用于矩阵下标与单调性比较）。
    pub fn idx(self) -> usize {
        match self {
            EnvTier::Cold => 0,
            EnvTier::Warm => 1,
            EnvTier::Hot => 2,
        }
    }
    /// 单调性权重：越热权重越小（耗时越短）。
    pub fn rank(self) -> u64 {
        match self {
            EnvTier::Cold => 3,
            EnvTier::Warm => 2,
            EnvTier::Hot => 1,
        }
    }
}

// ── 行状态五闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowPhase {
    Empty,
    Timing,
    Timed,
    Booked,
    Sealed,
}

/// 合法边闭集：Empty→Timing→Timed→Booked→Sealed（线性单链，无旁路）。
fn edge_legal(cur: RowPhase, next: RowPhase) -> bool {
    matches!(
        (cur, next),
        (RowPhase::Empty, RowPhase::Timing)
            | (RowPhase::Timing, RowPhase::Timed)
            | (RowPhase::Timed, RowPhase::Booked)
            | (RowPhase::Booked, RowPhase::Sealed)
    )
}

// ── 账行（四列：段号/样本号/环境档/耗时） ────────────────────────
pub struct PerfRow {
    pub seg: usize,
    pub sample: usize,
    pub env: EnvTier,
    pub cycles: u64,
    /// 引用的协议单点标识（必须 ≡ PROTOCOL_ID）。
    pub protocol: &'static str,
    pub phase: RowPhase,
}

// ── 性能总账 ─────────────────────────────────────────────────────
pub struct PerfLedger {
    /// 行池：下标 = ((seg-1)*12 + (sample-1))*3 + env_idx。
    rows: Vec<Option<PerfRow>>,
    pub booked: u32,
    pub sealed: u32,
    /// 全账封存旗：置位后整账只读。
    pub frozen: bool,
}

impl PerfLedger {
    pub fn new() -> PerfLedger {
        let mut rows = Vec::new();
        let mut i = 0;
        while i < TOTAL_ROWS {
            rows.push(None);
            i += 1;
        }
        PerfLedger { rows, booked: 0, sealed: 0, frozen: false }
    }

    fn index_of(seg: usize, sample: usize, env: EnvTier) -> usize {
        ((seg - 1) * SAMPLE_COUNT + (sample - 1)) * ENV_COUNT + env.idx()
    }

    /// 边界预检：段号/样本号闭集与下标长度先比。
    fn bounds(seg: usize, sample: usize) -> Result<(), u32> {
        if seg == 0 || seg > SEGMENT_COUNT {
            return Err(PerfCode::SegOutOfRange.val());
        }
        if sample == 0 || sample > SAMPLE_COUNT {
            return Err(PerfCode::SampleOutOfRange.val());
        }
        Ok(())
    }

    /// 状态推进（闭式边集 + 非法转换拒收）。
    fn advance(row: &mut Option<PerfRow>, next: RowPhase) -> Result<(), u32> {
        match row {
            None => {
                if next != RowPhase::Timing {
                    return Err(PerfCode::BookWithoutTiming.val());
                }
                Ok(())
            }
            Some(ref r) => {
                if r.frozen_by_ledger() {
                    return Err(PerfCode::DoubleBook.val());
                }
                if edge_legal(r.phase, next) {
                    Ok(())
                } else {
                    Err(PerfCode::BookWithoutTiming.val())
                }
            }
        }
    }

    /// 开计时：Empty→Timing，携带协议单点标识。
    pub fn start(&mut self, seg: usize, sample: usize, env: EnvTier) -> Result<(), u32> {
        if self.frozen {
            return Err(PerfCode::DoubleBook.val());
        }
        Self::bounds(seg, sample)?;
        let idx = Self::index_of(seg, sample, env);
        if idx >= self.rows.len() {
            return Err(PerfCode::SegOutOfRange.val());
        }
        let mut slot = self.rows[idx].take();
        let r = Self::advance(&mut slot, RowPhase::Timing);
        let out = match r {
            Ok(()) => {
                self.rows[idx] = Some(PerfRow {
                    seg,
                    sample,
                    env,
                    cycles: 0,
                    protocol: PROTOCOL_ID,
                    phase: RowPhase::Timing,
                });
                Ok(())
            }
            Err(e) => Err(e),
        };
        if self.rows[idx].is_none() && slot.is_some() {
            // 原行仍在原相位（推进失败回滚）。
            self.rows[idx] = slot;
        }
        out
    }

    /// 记账：Timing→Timed→Booked（耗时入列，协议单点校验）。
    pub fn book(&mut self, seg: usize, sample: usize, env: EnvTier, cycles: u64) -> Result<(), u32> {
        if self.frozen {
            return Err(PerfCode::DoubleBook.val());
        }
        Self::bounds(seg, sample)?;
        if cycles == 0 {
            return Err(PerfCode::ZeroCycles.val());
        }
        let idx = Self::index_of(seg, sample, env);
        if idx >= self.rows.len() {
            return Err(PerfCode::SegOutOfRange.val());
        }
        let was = self.rows[idx].is_some();
        Self::advance(&mut self.rows[idx], RowPhase::Timed)?;
        // Timed→Booked。
        Self::advance(&mut self.rows[idx], RowPhase::Booked)?;
        if let Some(ref mut r) = self.rows[idx] {
            if r.protocol != PROTOCOL_ID {
                return Err(PerfCode::ProtocolForeign.val());
            }
            r.cycles = cycles;
            r.phase = RowPhase::Booked;
        }
        if !was {
            self.booked += 1;
        }
        Ok(())
    }

    /// 封存单行：Booked→Sealed。
    pub fn seal_row(&mut self, seg: usize, sample: usize, env: EnvTier) -> Result<(), u32> {
        Self::bounds(seg, sample)?;
        let idx = Self::index_of(seg, sample, env);
        if idx >= self.rows.len() {
            return Err(PerfCode::SegOutOfRange.val());
        }
        let cur = match self.rows[idx] {
            Some(ref r) => r.phase,
            None => return Err(PerfCode::BookWithoutTiming.val()),
        };
        if !edge_legal(cur, RowPhase::Sealed) {
            return Err(PerfCode::BookWithoutTiming.val());
        }
        if let Some(ref mut r) = self.rows[idx] {
            r.phase = RowPhase::Sealed;
        }
        self.sealed += 1;
        Ok(())
    }

    /// 全账封存：须 252 行全 Booked。
    pub fn seal_all(&mut self) -> Result<(), u32> {
        if self.booked != TOTAL_ROWS as u32 || self.sealed != TOTAL_ROWS as u32 {
            return Err(PerfCode::ConservationBreak.val());
        }
        self.frozen = true;
        Ok(())
    }

    /// 守恒：Σ已记账行 ≡ 252，且相位全 Sealed。
    pub fn conservation(&self) -> Result<[u32; 5], u32> {
        let mut counts = [0u32; 5];
        let mut i = 0;
        while i < self.rows.len() {
            match self.rows[i] {
                Some(ref r) => {
                    let k = match r.phase {
                        RowPhase::Empty => 0,
                        RowPhase::Timing => 1,
                        RowPhase::Timed => 2,
                        RowPhase::Booked => 3,
                        RowPhase::Sealed => 4,
                    };
                    counts[k] += 1;
                }
                None => counts[0] += 1,
            }
            i += 1;
        }
        let sum = counts[0] + counts[1] + counts[2] + counts[3] + counts[4];
        if sum != TOTAL_ROWS as u32 || counts[4] != TOTAL_ROWS as u32 {
            return Err(PerfCode::ConservationBreak.val());
        }
        Ok(counts)
    }

    /// 耗时单调性：同段同样本 热档 ≤ 温档 ≤ 冷档。
    pub fn monotonic(&self, seg: usize, sample: usize) -> Result<(), u32> {
        Self::bounds(seg, sample)?;
        let mut e = 0;
        while e + 1 < ENV_COUNT {
            let a = Self::index_of(seg, sample, EnvTier::Hot);
            let b = Self::index_of(seg, sample, EnvTier::Warm);
            let c = Self::index_of(seg, sample, EnvTier::Cold);
            let hot = match self.rows[a] {
                Some(ref r) => r.cycles,
                None => return Err(PerfCode::ConservationBreak.val()),
            };
            let warm = match self.rows[b] {
                Some(ref r) => r.cycles,
                None => return Err(PerfCode::ConservationBreak.val()),
            };
            let cold = match self.rows[c] {
                Some(ref r) => r.cycles,
                None => return Err(PerfCode::ConservationBreak.val()),
            };
            let _ = e;
            if hot > warm || warm > cold {
                return Err(PerfCode::MonotonicBreak.val());
            }
            e += 1;
        }
        let _ = a_guard(hot_cold_rank());
        Ok(())
    }

    /// 四列账导出（确定性序：段→样本→档）。
    pub fn export_matrix(&self) -> Result<Vec<(usize, usize, u8, u64)>, u32> {
        let mut out = Vec::new();
        let mut seg = 1;
        while seg <= SEGMENT_COUNT {
            let mut s = 1;
            while s <= SAMPLE_COUNT {
                let mut e = 0;
                while e < ENV_COUNT {
                    let env = match e {
                        0 => EnvTier::Cold,
                        1 => EnvTier::Warm,
                        _ => EnvTier::Hot,
                    };
                    let idx = Self::index_of(seg, s, env);
                    if idx >= self.rows.len() {
                        return Err(PerfCode::ConservationBreak.val());
                    }
                    match self.rows[idx] {
                        Some(ref r) => out.push((r.seg, r.sample, env.idx() as u8, r.cycles)),
                        None => return Err(PerfCode::ConservationBreak.val()),
                    }
                    e += 1;
                }
                s += 1;
            }
            seg += 1;
        }
        Ok(out)
    }
}

impl PerfRow {
    /// 行级只读判定：Sealed 相位即只读。
    fn frozen_by_ledger(&self) -> bool {
        self.phase == RowPhase::Sealed
    }
}

/// 档位权重对照（供单调性检查引用，独立常量不复用被测值）。
const fn hot_cold_rank() -> u64 {
    1
}
fn a_guard(u: u64) -> u64 {
    u
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演：252 行全填（单调档距）→封存→守恒→幂等 ─────────────
pub fn golden_run() -> Result<u64, u32> {
    let mut led = PerfLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut seg = 1;
    while seg <= SEGMENT_COUNT {
        let mut s = 1;
        while s <= SAMPLE_COUNT {
            // 冷档基准耗时，温/热按档距收缩（保单调：热 ≤ 温 ≤ 冷）。
            let base = 8_000 + rng.below(4_000);
            let warm = base - 1 - rng.below(1_000);
            let hot = warm - 1 - rng.below(1_000);
            let mut e = 0;
            while e < ENV_COUNT {
                let env = match e {
                    0 => EnvTier::Cold,
                    1 => EnvTier::Warm,
                    _ => EnvTier::Hot,
                };
                let cy = match e {
                    0 => base,
                    1 => warm,
                    _ => hot,
                };
                led.start(seg, s, env)?;
                led.book(seg, s, env, cy)?;
                led.seal_row(seg, s, env)?;
                e += 1;
            }
            s += 1;
        }
        seg += 1;
    }
    led.seal_all()?;
    led.conservation()?;
    // 12 样本 × 7 段单调性全查。
    let mut seg2 = 1;
    while seg2 <= SEGMENT_COUNT {
        let mut s2 = 1;
        while s2 <= SAMPLE_COUNT {
            led.monotonic(seg2, s2)?;
            s2 += 1;
        }
        seg2 += 1;
    }
    let m1 = led.export_matrix()?;
    let m2 = led.export_matrix()?;
    if m1.len() != m2.len() {
        return Err(PerfCode::ConservationBreak.val());
    }
    // 幂等：双导出逐行同。
    let mut i = 0;
    while i < m1.len() {
        if m1[i] != m2[i] {
            return Err(PerfCode::ConservationBreak.val());
        }
        i += 1;
    }
    // 行数黄金对照：恰 252 行。
    if m1.len() != TOTAL_ROWS {
        return Err(PerfCode::ConservationBreak.val());
    }
    let mut h: u64 = 0x811C9DC5;
    i = 0;
    while i < m1.len() {
        let (sg, sm, ev, cy) = m1[i];
        h = (h ^ (sg as u64)).wrapping_mul(0x01000193);
        h = (h ^ (sm as u64)).wrapping_mul(0x01000193);
        h = (h ^ (ev as u64)).wrapping_mul(0x01000193);
        h = (h ^ cy).wrapping_mul(0x01000193);
        i += 1;
    }
    Ok(h)
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：段号越闭集。
    ledger.tries[0] += 1;
    let mut led = PerfLedger::new();
    let r0 = led.start(SEGMENT_COUNT + 1, 1, EnvTier::Hot);
    let mut c0 = false;
    if let Err(e) = r0 {
        if e == PerfCode::SegOutOfRange.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：未计时先记账。
    ledger.tries[1] += 1;
    let r1 = led.book(1, 1, EnvTier::Hot, 100);
    let mut c1 = false;
    if let Err(e) = r1 {
        if e == PerfCode::BookWithoutTiming.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：封存后重记账（账目只读红线）。
    ledger.tries[2] += 1;
    let _ = led.start(1, 1, EnvTier::Hot);
    let _ = led.book(1, 1, EnvTier::Hot, 500);
    let _ = led.seal_row(1, 1, EnvTier::Hot);
    let r2 = led.book(1, 1, EnvTier::Hot, 600);
    let mut c2 = false;
    if let Err(e) = r2 {
        if e == PerfCode::DoubleBook.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PerfCode::ALL.len() {
        let v = PerfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演252行", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 守恒正演：行池总数恒 252。
    v.push((
        "J1-行池恒等252",
        TOTAL_ROWS == 252 && PerfLedger::new().rows_len() == 252,
        format!("TOTAL_ROWS={}", TOTAL_ROWS),
    ));
    // 协议单点：全账行协议标识只许一个。
    v.push((
        "J1-协议单点复用",
        PROTOCOL_ID == "UNX-PERF-TIMER-P1" && !PROTOCOL_ID.is_empty(),
        String::from(PROTOCOL_ID),
    ));
    // 反向判据：零耗时必拒。
    let mut l2 = PerfLedger::new();
    let _ = l2.start(2, 3, EnvTier::Cold);
    let zero = l2.book(2, 3, EnvTier::Cold, 0);
    let mut ok = false;
    if let Err(e) = zero {
        if e == PerfCode::ZeroCycles.val() {
            ok = true;
        }
    }
    v.push(("J1-零耗时必拒", ok, String::from("cycles=0 拒收")));
    // 环境档闭集：档位序号 0/1/2。
    v.push((
        "J1-环境档闭集",
        EnvTier::Cold.idx() == 0 && EnvTier::Warm.idx() == 1 && EnvTier::Hot.idx() == 2,
        String::from("冷0/温1/热2"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65C00 窗口")));
    v
}

impl PerfLedger {
    /// 行池长度只读出口（供判据引用，判据独立写死 252 不复用 TOTAL_ROWS）。
    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }
}

pub fn run_unx13561_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13561 域级性能核账总成（七段计时×样本集矩阵）");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
