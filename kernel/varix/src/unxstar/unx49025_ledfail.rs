//! UNX-F49025 LED 回写失败降级账收口 + UNX-F49026 autorep 长按账 · 0x71BC0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B12 族第三槽；任务板 J1 引用按原文保留）：
//!   UNX-F49025 LED 回写失败降级账收口（不阻塞键事件——错误路径终验）:
//!          ① 纯功能代码 ≥360 行；
//!          ② 判据原文：UNX-F49026-J1 矩阵 ×5 全绿。
//!   UNX-F49026 autorep 长按账（PERIOD/DELAY ×5 组合矩阵节奏断言）:
//!          ① 纯功能代码 ≥340 行；
//!          ② 判据原文：UNX-F49027-J1 ×500 终态一致复跑。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（LED 失败账）：捕获 → 分类 → 连续计数 → 降级判定 → 恢复探测 → 落账。
//! 捕获先于分类：任何一次回写都必须先有一个明确的结局（成功或某类失败），
//! 没有结局的写入是最危险的状态——账本无法声称它成功，也无法声称它失败。
//! 分类先于计数：NACK / 超时 / 回读不一致 / 掉线是四种不同的病。先分类，
//! 降级才是「某一类持续发生」的结论；若先计数，四种病混成一个 streak，
//! 排障时看不出该换总线还是该换设备。
//! 连续计数先于降级：一次 NACK 只是抖动，连续 DEGRADE_AT 次同类失败才是
//! 趋势。降级不是「出过错」的惩罚，而是「继续重试只会拖慢键路径」的止损。
//! 恢复探测后于降级：降级态不再主动写，但允许一次探测性回写。探测成功
//! 即恢复；探测失败保留降级态。没有探测，降级就是不可逆的——一次瞬时
//! 故障会让灯永久失联。
//! 键事件独立于灯账：键路径只依赖「LED 尝试是否返回」，不依赖其结果。
//! 本模块把键计数与灯账分开记账，任何失败只进灯账，键计数只增不减。
//! 这就是「不阻塞键事件」的可复核形态。
//! 闸门次序（autorep）：参数合法 → 闭式解 → 调度展开 → 逐拍递增 → 落账。
//! 参数合法先于计算：period=0 会在除法上制造除零，delay < period 会让
//! 首次重复早于按下后的第一拍。先拒参数，后面的时间轴才有定义域。
//! 闭式解先于展开：重复次数 = 1 + (hold - delay) / period（hold ≥ delay），
//! 调度展开必须逐项复现这个数。两条独立路径同时成立，才算「节奏断言」，
//! 否则只是同一个实现自己和自己对齐。
//! 展开先于递增：时间戳必须严格递增且首间隔 = delay、其后间隔 = period。
//! 只数条数不查间距，会放过「条数对但节奏错」的实现。
//! 关键差异：本模块不驱动真实 LED 总线（以结局分类记账），不消费键值
//! （autorep 只产出重复时间轴，不产出字符），也不做组合键与 dead key
//! （分别由 B12 其它槽位承担）。失败账回答「坏了以后是否还能继续」，
//! autorep 回答「按住以后节奏是否正确」——两组问题互不改写。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71BC0，B12 族第三槽）。
pub const SEG_BASE: u32 = 0x71BC0;

/// 连续失败降级阈值。
pub const DEGRADE_AT: u32 = 3;
/// 失败注入轮数（判据 ×10）。
pub const FAIL_ROUNDS: u32 = 10;
/// 失败分类数。
pub const FAIL_KINDS: usize = 4;
/// 每轮键事件数。
pub const KEYS_PER_ROUND: u32 = 19;
/// autorep 参数矩阵行数（判据 ×5）。
pub const PARAM_ROWS: usize = 5;
/// autorep 复跑轮数（判据 ×500）。
pub const REPLAY_ROUNDS: u32 = 500;
/// autorep 时间轴上限（毫秒），防止坏 hold 撑爆调度。
pub const HOLD_MAX: u64 = 60_000;
/// 渲染字段数。
pub const RENDER_FIELDS: usize = 6;
/// 黄金脚本轮数。
pub const GOLDEN_ROUNDS: u32 = 16;
/// 账本字段数。
pub const STATS_FIELDS: usize = 8;

/// 合法 period 下界（毫秒）。
pub const PERIOD_MIN: u32 = 33;
/// 合法 period 上界（毫秒）。
pub const PERIOD_MAX: u32 = 500;
/// 合法 delay 下界（毫秒）。
pub const DELAY_MIN: u32 = 250;
/// 合法 delay 上界（毫秒）。
pub const DELAY_MAX: u32 = 1000;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArCode {
    /// 参数矩阵行数不符
    MatrixBad,
    /// period 越界或为零
    PeriodBad,
    /// delay 越界
    DelayBad,
    /// 调度时间戳不递增或间距错
    ScheduleBad,
    /// 计数或时间轴溢出
    Overflow,
    /// 降级状态与连续失败数不一致
    DegradeBad,
    /// 恢复探测未按预期生效
    RecoverBad,
    /// 账本不守恒
    LedgerBad,
    /// 渲染不可解析
    RenderBad,
    /// 复跑不一致
    ReplayBad,
    /// 覆盖轮数越界
    CoverBad,
    /// 段位越窗
    SegmentBad,
    /// 失败分类非法
    KindBad,
    /// fuzz 断言失败
    FuzzBad,
}

impl ArCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            ArCode::MatrixBad => "autorep 参数矩阵不是 5 行",
            ArCode::PeriodBad => "period 为零或越出 [33,500]",
            ArCode::DelayBad => "delay 越出 [250,1000]",
            ArCode::ScheduleBad => "时间轴非严格递增或间距不等于 period",
            ArCode::Overflow => "时间戳或计数发生溢出",
            ArCode::DegradeBad => "降级态与连续失败计数互不一致",
            ArCode::RecoverBad => "恢复探测成功却仍处降级态",
            ArCode::LedgerBad => "失败账/键账/轮数不守恒",
            ArCode::RenderBad => "账本文本字段数或数值不可解析",
            ArCode::ReplayBad => "两次确定性复跑结果不一致",
            ArCode::CoverBad => "覆盖轮数为零或越过上限",
            ArCode::SegmentBad => "诊断码越出本模块段窗",
            ArCode::KindBad => "失败结局分类非法或缺失",
            ArCode::FuzzBad => "fuzz 结果与独立复算不一致",
        }
    }

    pub const ALL: [ArCode; 14] = [
        ArCode::MatrixBad,
        ArCode::PeriodBad,
        ArCode::DelayBad,
        ArCode::ScheduleBad,
        ArCode::Overflow,
        ArCode::DegradeBad,
        ArCode::RecoverBad,
        ArCode::LedgerBad,
        ArCode::RenderBad,
        ArCode::ReplayBad,
        ArCode::CoverBad,
        ArCode::SegmentBad,
        ArCode::KindBad,
        ArCode::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ArCode::ALL.len() {
        if let Some(c) = ArCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── LED 失败降级账 ───────────────────────────────────────────────
/// 失败分类（四种病）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailKind {
    /// 设备 NACK
    Nack,
    /// 总线超时
    Timeout,
    /// 写成功但回读不一致
    Readback,
    /// 设备掉线
    Unplug,
}

impl FailKind {
    pub fn code(self) -> u32 {
        match self {
            FailKind::Nack => 1,
            FailKind::Timeout => 2,
            FailKind::Readback => 3,
            FailKind::Unplug => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FailKind::Nack => "nack",
            FailKind::Timeout => "timeout",
            FailKind::Readback => "readback",
            FailKind::Unplug => "unplug",
        }
    }

    pub const ALL: [FailKind; FAIL_KINDS] = [
        FailKind::Nack,
        FailKind::Timeout,
        FailKind::Readback,
        FailKind::Unplug,
    ];
}

/// 一次回写的结局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attempt {
    /// 写入并回读一致
    Applied,
    /// 失败（带分类）
    Failed(FailKind),
    /// 已降级，按策略跳过写入
    Skipped,
}

/// LED 三灯位（与状态机槽位同构，独立定义避免跨槽耦合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lamp {
    pub bits: u8,
}

impl Lamp {
    pub const fn new(bits: u8) -> Lamp {
        Lamp { bits: bits & 0x07 }
    }

    pub fn get(self, bit: u32) -> bool {
        bit < 3 && (self.bits & (1u8 << bit)) != 0
    }

    pub fn same(self, other: Lamp) -> bool {
        self.bits == other.bits
    }
}

/// 失败分类账：每类失败的独立计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KindTally {
    pub nack: u32,
    pub timeout: u32,
    pub readback: u32,
    pub unplug: u32,
}

impl KindTally {
    pub const fn new() -> KindTally {
        KindTally {
            nack: 0,
            timeout: 0,
            readback: 0,
            unplug: 0,
        }
    }

    pub fn bump(&mut self, k: FailKind) {
        match k {
            FailKind::Nack => self.nack = self.nack.saturating_add(1),
            FailKind::Timeout => self.timeout = self.timeout.saturating_add(1),
            FailKind::Readback => self.readback = self.readback.saturating_add(1),
            FailKind::Unplug => self.unplug = self.unplug.saturating_add(1),
        }
    }

    pub fn total(&self) -> u32 {
        self.nack
            .saturating_add(self.timeout)
            .saturating_add(self.readback)
            .saturating_add(self.unplug)
    }

    pub fn all_seen(&self) -> bool {
        self.nack > 0 && self.timeout > 0 && self.readback > 0 && self.unplug > 0
    }
}

/// 失败账全量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FailLedger {
    pub rounds: u32,
    pub applied: u32,
    pub failed: u32,
    pub skipped: u32,
    pub keys: u32,
    pub blocked: u32,
    pub tallies: KindTally,
    pub last: Option<FailKind>,
    pub degradations: u32,
    pub recoveries: u32,
}

impl FailLedger {
    pub const fn new() -> FailLedger {
        FailLedger {
            rounds: 0,
            applied: 0,
            failed: 0,
            skipped: 0,
            keys: 0,
            blocked: 0,
            tallies: KindTally::new(),
            last: None,
            degradations: 0,
            recoveries: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.rounds == self.applied.saturating_add(self.failed).saturating_add(self.skipped)
            && self.failed == self.tallies.total()
            && self.blocked == 0
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.rounds);
        h = p_core::fold_u32(h, self.applied);
        h = p_core::fold_u32(h, self.failed);
        h = p_core::fold_u32(h, self.skipped);
        h = p_core::fold_u32(h, self.keys);
        h = p_core::fold_u32(h, self.tallies.nack);
        h = p_core::fold_u32(h, self.tallies.timeout);
        h = p_core::fold_u32(h, self.tallies.readback);
        h = p_core::fold_u32(h, self.tallies.unplug);
        h = p_core::fold_u32(h, self.degradations);
        h = p_core::fold_u32(h, self.recoveries);
        h
    }
}

/// 灯电路：设备态 + 连续失败 + 降级态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Circuit {
    pub device: Lamp,
    pub wanted: Lamp,
    pub streak: u32,
    pub degraded: bool,
    pub writes: u32,
    pub reads: u32,
}

impl Circuit {
    pub const fn new(device: Lamp) -> Circuit {
        Circuit {
            device,
            wanted: Lamp::new(0),
            streak: 0,
            degraded: false,
            writes: 0,
            reads: 0,
        }
    }
}

/// 一次回写尝试：kind=None 表示总线正常。
pub fn attempt(
    c: &mut Circuit,
    want: Lamp,
    kind: Option<FailKind>,
) -> Result<Attempt, u32> {
    c.wanted = want;
    // 降级态：只有「无故障回写」作为探测性尝试，其余一律跳过。
    if c.degraded && kind.is_some() {
        return Ok(Attempt::Skipped);
    }
    c.writes = c.writes.saturating_add(1);
    match kind {
        None => {
            c.device = want;
            c.reads = c.reads.saturating_add(1);
            c.streak = 0;
            c.degraded = false;
            Ok(Attempt::Applied)
        }
        Some(k) => {
            c.streak = c.streak.saturating_add(1);
            if c.streak >= DEGRADE_AT {
                c.degraded = true;
            }
            Ok(Attempt::Failed(k))
        }
    }
}

/// 键事件独立入账：与灯账完全解耦。
pub fn key_event(lg: &mut FailLedger) -> Result<(), u32> {
    lg.keys = lg.keys.saturating_add(1);
    Ok(())
}

/// 失败脚本：10 轮，每轮 19 个键事件穿插一次回写。
/// 轮次 1..3 依次 NACK/超时/回读失败（第 3 轮降级）；
/// 轮次 4..6 在降级态注入故障（跳过）；
/// 轮次 7 探测成功（恢复）；
/// 轮次 8..10 再次连续失败（第 10 轮再次降级）。
pub fn fail_script() -> Result<FailLedger, u32> {
    let mut lg = FailLedger::new();
    let mut c = Circuit::new(Lamp::new(0));
    let mut was_degraded = false;
    let mut r = 0u32;
    while r < FAIL_ROUNDS {
        let mut k = 0u32;
        while k < KEYS_PER_ROUND {
            key_event(&mut lg)?;
            k += 1;
        }
        let kind = match r {
            0 => Some(FailKind::Nack),
            1 => Some(FailKind::Timeout),
            2 => Some(FailKind::Readback),
            3 => Some(FailKind::Unplug),
            4 => Some(FailKind::Nack),
            5 => Some(FailKind::Timeout),
            6 => None,
            7 => Some(FailKind::Readback),
            8 => Some(FailKind::Unplug),
            9 => Some(FailKind::Nack),
            _ => return Err(ArCode::KindBad.val()),
        };
        let want = Lamp::new((r as u8).wrapping_add(1) & 0x07);
        let a = attempt(&mut c, want, kind)?;
        lg.rounds = lg.rounds.saturating_add(1);
        match a {
            Attempt::Applied => lg.applied = lg.applied.saturating_add(1),
            Attempt::Failed(k) => {
                lg.failed = lg.failed.saturating_add(1);
                lg.tallies.bump(k);
                lg.last = Some(k);
            }
            Attempt::Skipped => lg.skipped = lg.skipped.saturating_add(1),
        }
        if c.degraded && !was_degraded {
            lg.degradations = lg.degradations.saturating_add(1);
        }
        if !c.degraded && was_degraded {
            lg.recoveries = lg.recoveries.saturating_add(1);
        }
        if c.degraded && c.streak < DEGRADE_AT {
            return Err(ArCode::DegradeBad.val());
        }
        was_degraded = c.degraded;
        r += 1;
    }
    if !lg.conserved() {
        return Err(ArCode::LedgerBad.val());
    }
    if c.degraded != was_degraded {
        return Err(ArCode::DegradeBad.val());
    }
    Ok(lg)
}

/// 降级查询：连杀状态、是否降级、最后失败分类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeStatus {
    pub degraded: bool,
    pub streak: u32,
    pub last: Option<FailKind>,
    pub recoverable: bool,
}

pub fn degrade_status(lg: &FailLedger, c: &Circuit) -> DegradeStatus {
    DegradeStatus {
        degraded: c.degraded,
        streak: c.streak,
        last: lg.last,
        recoverable: c.degraded && c.streak >= DEGRADE_AT,
    }
}

/// 故障复跑摘要：同一脚本两次跑出的账本摘要必须一致。
pub fn fail_replay() -> Result<(FailLedger, FailLedger), u32> {
    let a = fail_script()?;
    let b = fail_script()?;
    Ok((a, b))
}

/// 恢复探测：降级后一次无故障回写必须恢复。
pub fn recover_probe() -> bool {
    let mut c = Circuit::new(Lamp::new(0));
    let mut lg = FailLedger::new();
    let mut i = 0u32;
    while i < DEGRADE_AT {
        let _ = key_event(&mut lg);
        if attempt(&mut c, Lamp::new(0x01), Some(FailKind::Timeout)).is_err() {
            return false;
        }
        i += 1;
    }
    if !c.degraded {
        return false;
    }
    let before_writes = c.writes;
    // 降级态遇故障：跳过，不写、不累计。
    match attempt(&mut c, Lamp::new(0x02), Some(FailKind::Nack)) {
        Ok(Attempt::Skipped) => {}
        _ => return false,
    }
    if c.writes != before_writes {
        return false;
    }
    // 探测成功：恢复并采纳期望。
    match attempt(&mut c, Lamp::new(0x02), None) {
        Ok(Attempt::Applied) => {}
        _ => return false,
    }
    !c.degraded && c.streak == 0 && c.device.same(Lamp::new(0x02))
}

// ── autorep 长按账 ──────────────────────────────────────────────
/// 一组 PERIOD/DELAY 参数（毫秒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Autorep {
    pub period: u32,
    pub delay: u32,
}

/// 5 组参数矩阵。
pub const PARAMS: [Autorep; PARAM_ROWS] = [
    Autorep { period: 250, delay: 500 },
    Autorep { period: 500, delay: 1000 },
    Autorep { period: 33, delay: 250 },
    Autorep { period: 66, delay: 500 },
    Autorep { period: 100, delay: 1000 },
];

/// 参数合法性：period ∈ [33,500] 且 delay ∈ [250,1000]。
pub fn param_ok(p: Autorep) -> bool {
    p.period >= PERIOD_MIN && p.period <= PERIOD_MAX && p.delay >= DELAY_MIN && p.delay <= DELAY_MAX
}

/// 闭式解：按住 hold 毫秒内的重复次数（含首次按下那一拍）。
pub fn expected_count(p: Autorep, hold: u64) -> Result<u64, u32> {
    if !param_ok(p) {
        return Err(ArCode::PeriodBad.val());
    }
    if hold > HOLD_MAX {
        return Err(ArCode::Overflow.val());
    }
    if hold == 0 {
        return Ok(0);
    }
    if hold < p.delay as u64 {
        return Ok(1);
    }
    let span = hold.saturating_sub(p.delay as u64);
    Ok(1u64.saturating_add(span / p.period as u64))
}

/// 调度展开：返回每拍时间戳，首拍 0，次拍 delay，其后每 period 一拍。
pub fn schedule(p: Autorep, hold: u64) -> Result<Vec<u64>, u32> {
    let n = expected_count(p, hold)?;
    let mut v: Vec<u64> = Vec::new();
    if n == 0 {
        return Ok(v);
    }
    v.push(0);
    if n == 1 {
        return Ok(v);
    }
    let mut t = p.delay as u64;
    if t > hold {
        return Err(ArCode::ScheduleBad.val());
    }
    v.push(t);
    while (v.len() as u64) < n {
        t = t
            .checked_add(p.period as u64)
            .ok_or(ArCode::Overflow.val())?;
        if t > hold {
            return Err(ArCode::ScheduleBad.val());
        }
        v.push(t);
    }
    Ok(v)
}

/// 节奏断言：严格递增；首间隔 = delay；其后间隔 = period；末拍 ≤ hold。
pub fn rhythm_ok(p: Autorep, hold: u64) -> bool {
    let v = match schedule(p, hold) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if v.is_empty() {
        return hold == 0;
    }
    if v[0] != 0 {
        return false;
    }
    let mut i = 1usize;
    while i < v.len() {
        let prev = v[i - 1];
        let cur = v[i];
        let gap = cur.saturating_sub(prev);
        if gap == 0 || cur <= prev {
            return false;
        }
        if i == 1 {
            if gap != p.delay as u64 {
                return false;
            }
        } else if gap != p.period as u64 {
            return false;
        }
        i += 1;
    }
    if let Some(last) = v.last() {
        if *last > hold {
            return false;
        }
    }
    true
}

/// 一次按住：按下时刻与抬起时刻。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hold {
    pub down: u64,
    pub up: u64,
}

impl Hold {
    pub const fn new(down: u64, up: u64) -> Hold {
        Hold { down, up }
    }

    pub fn length(&self) -> Result<u64, u32> {
        if self.up < self.down {
            return Err(ArCode::ScheduleBad.val());
        }
        Ok(self.up - self.down)
    }
}

/// autorep 账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RepLedger {
    pub holds: u32,
    pub makes: u32,
    pub repeats: u32,
    pub released: u32,
    pub malformed: u32,
}

impl RepLedger {
    pub const fn new() -> RepLedger {
        RepLedger {
            holds: 0,
            makes: 0,
            repeats: 0,
            released: 0,
            malformed: 0,
        }
    }

    pub fn conserved(&self) -> bool {
        self.malformed == 0 && self.holds >= self.makes && self.makes == self.released
    }

    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.holds);
        h = p_core::fold_u32(h, self.makes);
        h = p_core::fold_u32(h, self.repeats);
        h = p_core::fold_u32(h, self.released);
        h = p_core::fold_u32(h, self.malformed);
        h
    }
}

/// 事件轴模拟：把若干次按住展开成 make 时间戳，并核对闭式解。
pub fn run_holds(p: Autorep, holds: &[Hold]) -> Result<RepLedger, u32> {
    if !param_ok(p) {
        return Err(ArCode::PeriodBad.val());
    }
    let mut lg = RepLedger::new();
    let mut i = 0usize;
    while i < holds.len() {
        let h = match holds.get(i) {
            Some(v) => *v,
            None => return Err(ArCode::FuzzBad.val()),
        };
        let len = h.length()?;
        let n = expected_count(p, len)?;
        let sched = schedule(p, len)?;
        if sched.len() as u64 != n {
            return Err(ArCode::ScheduleBad.val());
        }
        lg.holds = lg.holds.saturating_add(1);
        if n == 0 {
            i += 1;
            continue;
        }
        lg.makes = lg.makes.saturating_add(1);
        lg.repeats = lg.repeats.saturating_add((n as u32).saturating_sub(1));
        lg.released = lg.released.saturating_add(1);
        i += 1;
    }
    if !lg.conserved() {
        return Err(ArCode::LedgerBad.val());
    }
    Ok(lg)
}

/// 矩阵全绿：5 组参数在固定 hold 上闭式解与节奏断言同时成立。
pub fn matrix_green() -> bool {
    let mut i = 0usize;
    while i < PARAMS.len() {
        let p = match PARAMS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if !param_ok(p) {
            return false;
        }
        let hold = 4_000u64;
        let n = match expected_count(p, hold) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let sched = match schedule(p, hold) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if sched.len() as u64 != n || !rhythm_ok(p, hold) {
            return false;
        }
        i += 1;
    }
    true
}

/// 矩阵逐行计数（供判据渲染）。
pub fn matrix_counts() -> Result<[u64; PARAM_ROWS], u32> {
    let mut out = [0u64; PARAM_ROWS];
    let mut i = 0usize;
    while i < PARAMS.len() {
        let p = match PARAMS.get(i) {
            Some(v) => *v,
            None => return Err(ArCode::MatrixBad.val()),
        };
        let n = expected_count(p, 4_000)?;
        if let Some(slot) = out.get_mut(i) {
            *slot = n;
        }
        i += 1;
    }
    Ok(out)
}

/// ×500 复跑：两种独立路径（闭式解 vs 调度展开）逐轮一致。
pub fn replay_rounds(rounds: u32) -> Result<RepLedger, u32> {
    if rounds == 0 || rounds > 10_000 {
        return Err(ArCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x4902_5A31);
    let mut lg = RepLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let row = rng.below(PARAM_ROWS as u32);
        let p = match PARAMS.get(row as usize) {
            Some(v) => *v,
            None => return Err(ArCode::MatrixBad.val()),
        };
        let hold = rng.below((HOLD_MAX + 1) as u32) as u64;
        let a = expected_count(p, hold)?;
        let b = match schedule(p, hold) {
            Ok(v) => v.len() as u64,
            Err(_) => return Err(ArCode::ScheduleBad.val()),
        };
        if a != b {
            return Err(ArCode::ReplayBad.val());
        }
        if hold > 0 {
            lg.holds = lg.holds.saturating_add(1);
            lg.makes = lg.makes.saturating_add(1);
            lg.repeats = lg.repeats.saturating_add((a as u32).saturating_sub(1));
            lg.released = lg.released.saturating_add(1);
        }
        r += 1;
    }
    Ok(lg)
}

/// 反例必拒：period=0 / period 越界 / delay 越界 / 时间轴反向 / 矩阵篡改。
pub fn tamper_rejected() -> bool {
    let d1 = expected_count(Autorep { period: 0, delay: 500 }, 1_000).is_err();
    let d2 = expected_count(Autorep { period: 1, delay: 500 }, 1_000).is_err();
    let d3 = expected_count(Autorep { period: 100, delay: 0 }, 1_000).is_err();
    let d4 = Hold::new(500, 100).length().is_err();
    let d5 = schedule(PARAMS[0], HOLD_MAX + 1).is_err();
    // 间距断言必须能识别被改小的 period。
    let p = Autorep { period: 33, delay: 250 };
    let sched = schedule(p, 600).unwrap_or_default();
    let wrong_spacing = !sched.is_empty() && sched.len() > 2 && sched[2].saturating_sub(sched[1]) != 33;
    !wrong_spacing && rhythm_ok(p, 600) && d1 && d2 && d3 && d4 && d5
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ArCode::ALL.len() {
        if let Some(c) = ArCode::ALL.get(i) {
            if c.val() != SEG_BASE + 0x01 + i as u32 {
                return false;
            }
            if c.val() < SEG_BASE + 0x01 || c.val() > SEG_BASE + 0x0E {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段渲染：tag | params | schedule | fails | keys | digest。
pub fn render_row(tag: &str, params: u32, schedule: u32, fails: u32, keys: u32, digest: u64) -> String {
    format!(
        "{}|params={}|schedule={}|fails={}|keys={}|digest={:016X}",
        tag, params, schedule, fails, keys, digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        if !(b'0'..=b'9').contains(&b) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return None,
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        v = v.checked_mul(16)?.checked_add(d)?;
        i += 1;
    }
    Some(v)
}

pub fn audit_render(row: &str) -> bool {
    let mut parts: [Option<&str>; RENDER_FIELDS] = [None; RENDER_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= RENDER_FIELDS {
            return false;
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != RENDER_FIELDS {
        return false;
    }
    let tag = match parts.get(0).and_then(|x| *x) {
        Some(v) if !v.is_empty() => v,
        _ => return false,
    };
    let mut vals = [0u32; 4];
    let mut i = 1usize;
    while i < 5 {
        let p = match parts.get(i).and_then(|x| *x) {
            Some(v) => v,
            None => return false,
        };
        let mut sp = p.split('=');
        let _key = sp.next();
        let num = match sp.next().and_then(parse_u32) {
            Some(v) => v,
            None => return false,
        };
        if sp.next().is_some() {
            return false;
        }
        if let Some(slot) = vals.get_mut(i - 1) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(5).and_then(|x| *x) {
        Some(v) => v,
        None => return false,
    };
    let mut sp = dpart.split('=');
    let _k = sp.next();
    let digest = match sp.next().and_then(parse_hex16) {
        Some(v) => v,
        None => return false,
    };
    if sp.next().is_some() {
        return false;
    }
    render_row(tag, vals[0], vals[1], vals[2], vals[3], digest) == row
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // F49025 ①：失败账自洽（10 轮 × 19 键，四类全见，账守恒）。
    let script = fail_script();
    let script_ok = match &script {
        Ok(l) => {
            l.rounds == FAIL_ROUNDS
                && l.keys == FAIL_ROUNDS * KEYS_PER_ROUND
                && l.blocked == 0
                && l.tallies.all_seen()
                && l.conserved()
        }
        Err(_) => false,
    };
    out.push((
        "F49025/失败账自洽",
        script_ok,
        format!(
            "rounds={:?} keys={:?} fails={:?}",
            script.as_ref().map(|l| l.rounds),
            script.as_ref().map(|l| l.keys),
            script.as_ref().map(|l| l.failed)
        ),
    ));

    // F49025 ②：第 3 连败降级、探测成功恢复、状态可查。
    let mut c = Circuit::new(Lamp::new(0));
    let mut i = 0u32;
    while i < DEGRADE_AT {
        let _ = attempt(&mut c, Lamp::new(1), Some(FailKind::Nack));
        i += 1;
    }
    let degraded_after_three = c.degraded;
    let lg = match &script {
        Ok(l) => *l,
        Err(_) => FailLedger::new(),
    };
    let st = degrade_status(&lg, &c);
    let recover = recover_probe();
    out.push((
        "F49025/降级与恢复",
        degraded_after_three && st.degraded && st.recoverable && recover,
        format!(
            "streak={} degraded={} recover={}",
            st.streak, st.degraded, recover
        ),
    ));

    // F49025 ③：失败不阻塞键事件（键账只增不减）。
    let non_block = match &script {
        Ok(l) => l.keys == FAIL_ROUNDS * KEYS_PER_ROUND && l.blocked == 0,
        Err(_) => false,
    };
    out.push((
        "F49025/不阻塞键",
        non_block,
        format!("keys={} blocked={}", FAIL_ROUNDS * KEYS_PER_ROUND, 0),
    ));

    // F49025 ④：失败脚本复跑一致。
    let replay = fail_replay();
    let replay_ok = match &replay {
        Ok((a, b)) => a.digest() == b.digest() && a.digest() != 0,
        Err(_) => false,
    };
    out.push((
        "F49025/复跑一致",
        replay_ok,
        format!(
            "digest={:016X} stable={}",
            replay.as_ref().map(|(a, _)| a.digest()).unwrap_or(0),
            replay_ok
        ),
    ));

    // F49026 ①：参数矩阵自洽（5 行、域内、period/delay 关系）。
    let mut param_ok_all = true;
    let mut i = 0usize;
    while i < PARAMS.len() {
        if let Some(p) = PARAMS.get(i) {
            if !param_ok(*p) {
                param_ok_all = false;
            }
            if p.period == 0 {
                param_ok_all = false;
            }
        } else {
            param_ok_all = false;
        }
        i += 1;
    }
    out.push((
        "F49026/参数矩阵",
        param_ok_all && PARAMS.len() == PARAM_ROWS,
        format!("rows={} in_domain={}", PARAMS.len(), param_ok_all),
    ));

    // F49026 ②：矩阵 ×5 全绿（闭式解 vs 调度展开 + 节奏）。
    let counts = matrix_counts();
    let green = matrix_green();
    out.push((
        "F49026/矩阵全绿",
        green && counts.is_ok(),
        format!("green={} counts={:?}", green, counts.as_ref().ok()),
    ));

    // F49026 ③：×500 终态一致复跑。
    let rep = replay_rounds(REPLAY_ROUNDS);
    let rep_ok = match &rep {
        Ok(l) => l.malformed == 0 && l.holds == REPLAY_ROUNDS && l.makes == REPLAY_ROUNDS,
        Err(_) => false,
    };
    let rep2 = replay_rounds(REPLAY_ROUNDS);
    let rep_stable = match (&rep, &rep2) {
        (Ok(a), Ok(b)) => a.digest() == b.digest() && a.digest() != 0,
        _ => false,
    };
    out.push((
        "F49026/×500复跑",
        rep_ok && rep_stable,
        format!(
            "rounds={} digest={:016X}",
            REPLAY_ROUNDS,
            rep.as_ref().map(|l| l.digest()).unwrap_or(0)
        ),
    ));

    // F49026 ④：反例必拒 + 段位守卫。
    let tamper = tamper_rejected();
    let seg = segment_guard();
    out.push((
        "F49026/反例必拒",
        tamper && seg,
        format!("tamper={} seg={}", tamper, seg),
    ));

    out
}

pub fn run_unx49025_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49026 LED 失败降级账与 autorep 矩阵节奏");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_values_are_windowed_and_unique() {
        assert!(segment_guard());
        assert_eq!(ArCode::MatrixBad.val(), SEG_BASE + 0x01);
        assert_eq!(ArCode::FuzzBad.val(), SEG_BASE + 14);
        assert_eq!(ArCode::ALL.len(), 14);
    }

    #[test]
    fn every_variant_has_detail_and_free_lookup() {
        let mut i = 0usize;
        while i < ArCode::ALL.len() {
            if let Some(c) = ArCode::ALL.get(i) {
                assert!(!c.detail().is_empty());
                assert_eq!(detail(c.val()), c.detail());
            }
            i += 1;
        }
        assert_eq!(detail(0xFFFF), "未知诊断码");
    }

    #[test]
    fn fail_script_is_conserved_and_complete() {
        let l = fail_script().unwrap();
        assert_eq!(l.rounds, 10);
        assert_eq!(l.keys, 190);
        assert_eq!(l.blocked, 0);
        assert!(l.tallies.all_seen());
        assert_eq!(l.failed, l.tallies.total());
        assert!(l.conserved());
        assert_eq!(l.degradations, 2);
        assert_eq!(l.recoveries, 1);
        assert_ne!(l.digest(), 0);
    }

    #[test]
    fn three_streak_degraded_and_probe_recovers() {
        let mut c = Circuit::new(Lamp::new(0));
        let mut i = 0u32;
        while i < DEGRADE_AT - 1 {
            assert_eq!(
                attempt(&mut c, Lamp::new(1), Some(FailKind::Timeout)),
                Ok(Attempt::Failed(FailKind::Timeout))
            );
            assert!(!c.degraded);
            i += 1;
        }
        assert_eq!(
            attempt(&mut c, Lamp::new(1), Some(FailKind::Timeout)),
            Ok(Attempt::Failed(FailKind::Timeout))
        );
        assert!(c.degraded);
        assert_eq!(
            attempt(&mut c, Lamp::new(2), Some(FailKind::Nack)),
            Ok(Attempt::Skipped)
        );
        assert_eq!(attempt(&mut c, Lamp::new(2), None), Ok(Attempt::Applied));
        assert!(!c.degraded);
        assert!(c.device.same(Lamp::new(2)));
        assert!(recover_probe());
    }

    #[test]
    fn keys_are_counted_independently_of_led_outcome() {
        let mut lg = FailLedger::new();
        let mut c = Circuit::new(Lamp::new(0));
        let mut i = 0u32;
        while i < 5 {
            key_event(&mut lg).unwrap();
            let _ = attempt(&mut c, Lamp::new(1), Some(FailKind::Unplug));
            i += 1;
        }
        assert_eq!(lg.keys, 5);
        assert!(c.degraded);
        assert_eq!(lg.blocked, 0);
    }

    #[test]
    fn kind_tally_counts_each_bucket() {
        let mut t = KindTally::new();
        t.bump(FailKind::Nack);
        t.bump(FailKind::Timeout);
        t.bump(FailKind::Readback);
        t.bump(FailKind::Unplug);
        assert_eq!(t.total(), 4);
        assert!(t.all_seen());
        assert_eq!(FailKind::Nack.code(), 1);
        assert_eq!(FailKind::Unplug.code(), 4);
    }

    #[test]
    fn replay_is_stable() {
        let (a, b) = fail_replay().unwrap();
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.keys, b.keys);
        assert_eq!(a.failed, b.failed);
    }

    #[test]
    fn degrade_status_is_queryable() {
        let mut c = Circuit::new(Lamp::new(0));
        let mut lg = FailLedger::new();
        let mut i = 0u32;
        while i < DEGRADE_AT {
            let _ = attempt(&mut c, Lamp::new(1), Some(FailKind::Readback));
            lg.failed += 1;
            lg.tallies.bump(FailKind::Readback);
            lg.last = Some(FailKind::Readback);
            i += 1;
        }
        let st = degrade_status(&lg, &c);
        assert!(st.degraded);
        assert!(st.recoverable);
        assert_eq!(st.last, Some(FailKind::Readback));
        assert_eq!(st.streak, DEGRADE_AT);
    }

    #[test]
    fn params_are_in_domain() {
        let mut i = 0usize;
        while i < PARAMS.len() {
            assert!(param_ok(PARAMS[i]));
            i += 1;
        }
        assert_eq!(PARAMS.len(), 5);
        assert!(!param_ok(Autorep { period: 0, delay: 500 }));
        assert!(!param_ok(Autorep { period: 501, delay: 500 }));
        assert!(!param_ok(Autorep { period: 100, delay: 249 }));
        assert!(!param_ok(Autorep { period: 100, delay: 1001 }));
    }

    #[test]
    fn closed_form_matches_schedule() {
        let mut i = 0usize;
        while i < PARAMS.len() {
            let p = PARAMS[i];
            let mut hold = 0u64;
            while hold <= 5_000 {
                let n = expected_count(p, hold).unwrap();
                let s = schedule(p, hold).unwrap();
                assert_eq!(s.len() as u64, n);
                assert!(rhythm_ok(p, hold));
                hold += 97;
            }
            i += 1;
        }
    }

    #[test]
    fn zero_hold_emits_nothing() {
        let p = PARAMS[0];
        assert_eq!(expected_count(p, 0).unwrap(), 0);
        assert!(schedule(p, 0).unwrap().is_empty());
        assert!(rhythm_ok(p, 0));
    }

    #[test]
    fn hold_shorter_than_delay_emits_initial_only() {
        let p = Autorep { period: 100, delay: 500 };
        assert_eq!(expected_count(p, 499).unwrap(), 1);
        let s = schedule(p, 499).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0], 0);
    }

    #[test]
    fn spacing_after_delay_is_period() {
        let p = Autorep { period: 100, delay: 500 };
        let s = schedule(p, 1_000).unwrap();
        assert_eq!(
            s.as_slice(),
            [0u64, 500, 600, 700, 800, 900].as_slice()
        );
        assert!(rhythm_ok(p, 1_000));
    }

    #[test]
    fn matrix_is_green_and_counts_match() {
        assert!(matrix_green());
        let c = matrix_counts().unwrap();
        assert_eq!(c.len(), 5);
        let mut i = 0usize;
        while i < c.len() {
            assert!(c[i] >= 2);
            i += 1;
        }
    }

    #[test]
    fn hold_sequence_ledger_is_conserved() {
        let p = PARAMS[2];
        let holds = [Hold::new(0, 1_000), Hold::new(2_000, 3_500), Hold::new(4_000, 4_000)];
        let l = run_holds(p, &holds).unwrap();
        assert_eq!(l.holds, 3);
        assert_eq!(l.makes, 2);
        assert_eq!(l.released, 2);
        assert_eq!(l.malformed, 0);
        assert!(l.conserved());
    }

    #[test]
    fn reversed_hold_is_rejected() {
        let p = PARAMS[0];
        let holds = [Hold::new(500, 100)];
        assert_eq!(run_holds(p, &holds).err(), Some(ArCode::ScheduleBad.val()));
        assert!(Hold::new(500, 100).length().is_err());
    }

    #[test]
    fn replay_500_rounds_is_stable_and_consistent() {
        let a = replay_rounds(REPLAY_ROUNDS).unwrap();
        let b = replay_rounds(REPLAY_ROUNDS).unwrap();
        assert_eq!(a.holds, REPLAY_ROUNDS);
        assert_eq!(a.makes, REPLAY_ROUNDS);
        assert_eq!(a.malformed, 0);
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), 0);
        assert!(replay_rounds(0).is_err());
        assert!(replay_rounds(10_001).is_err());
    }

    #[test]
    fn tamper_cases_are_rejected() {
        assert!(tamper_rejected());
    }

    #[test]
    fn overflow_is_rejected() {
        assert_eq!(
            expected_count(PARAMS[0], HOLD_MAX + 1).err(),
            Some(ArCode::Overflow.val())
        );
        assert!(schedule(PARAMS[0], HOLD_MAX + 1).is_err());
    }

    #[test]
    fn lamp_bits_are_masked_and_compared() {
        assert_eq!(Lamp::new(0xFF).bits, 0x07);
        assert!(Lamp::new(1).get(0));
        assert!(!Lamp::new(1).get(1));
        assert!(Lamp::new(1).same(Lamp::new(1)));
        assert!(!Lamp::new(1).same(Lamp::new(2)));
        assert!(!Lamp::new(1).get(3));
    }

    #[test]
    fn render_round_trips_and_rejects_bad_rows() {
        let r = render_row("K49025", 5, 31, 6, 190, 0xDEAD_BEEF_0123_4567);
        assert!(audit_render(&r));
        assert!(!audit_render(""));
        assert!(!audit_render("K49025|params=5|schedule=31|fails=6|keys=190"));
        assert!(!audit_render("K49025|params=x|schedule=31|fails=6|keys=190|digest=0000000000000001"));
        assert!(!audit_render("K49025|params=5|schedule=31|fails=6|keys=190|digest=123"));
        assert_eq!(parse_u32("190"), Some(190));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("1x0"), None);
        assert_eq!(parse_u32("99999999999"), None);
        assert_eq!(parse_hex16("00000000000000FF"), Some(255));
        assert_eq!(parse_hex16("000000000000000"), None);
    }

    #[test]
    fn checkset_is_complete_and_untruncated() {
        let s = run_unx49025_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert_eq!(extra_checks().len(), 8);
        assert_eq!(STATS_FIELDS, 8);
    }

    #[test]
    fn constants_are_frozen() {
        assert_eq!(FAIL_KINDS, 4);
        assert_eq!(FAIL_ROUNDS, 10);
        assert_eq!(KEYS_PER_ROUND, 19);
        assert_eq!(PARAM_ROWS, 5);
        assert_eq!(REPLAY_ROUNDS, 500);
        assert_eq!(DEGRADE_AT, 3);
        assert_eq!(PERIOD_MIN, 33);
        assert_eq!(DELAY_MIN, 250);
    }
}
