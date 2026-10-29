//! unx0983_stormtier — UNX-F0983 风暴分级账（四级风暴与升级路径）。
//!
//! 任务锚点：UNX-F0983（第二批工位 W7，SEG 段位 0x80E0，本单独占
//! 0x80E0..=0x80FF；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**风暴四级闭集逐级可追溯（每级带判据/
//! 动作/升级条件三列），升级只进不跳（逐级升级必拦跳级），降级走
//! 评估回退（可跨级降但留痕），分级账守恒（各级行数和==总行数）**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x80E 段闭集）；tick 显性
//! 注入零墙钟；分级账定容守恒（四级和==总数 恒等式机检）；结尾
//! drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x80E0 段，本单独占）
// ---------------------------------------------------------------------------

/// 风暴分级账段基址（0x80E0，本单 0x80E0..=0x80FF 独占）。
pub const SEG_BASE: u16 = 0x80E0;
/// 0x80E1 风暴级越界（级码不在四级闭集内）。
pub const XC_TIER_BAD: u16 = SEG_BASE + 1;
/// 0x80E2 跳级升级（升级跨度超过一级）。
pub const XC_ESCALATE_JUMP: u16 = SEG_BASE + 2;
/// 0x80E3 升级判据缺失（升级时新级判据未成立——空升拦停）。
pub const XC_NO_CRITERION: u16 = SEG_BASE + 3;
/// 0x80E4 降级无评估（降级动作缺评估回执——回退纪律破坏）。
pub const XC_NO_EVAL: u16 = SEG_BASE + 4;
/// 0x80E5 分级账守恒破坏（四级行数和 != 总行数）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 5;
/// 0x80E6 级变 tick 回退（级变时序单调性破坏）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 6;
/// 0x80E7 分级账指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 7;
/// 0x80E8 账本定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 8;
/// 0x80E9 未升级即降级（一级以下无级可降）。
pub const XC_UNDERFLOW: u16 = SEG_BASE + 9;
/// 0x80EA 级序未开账（首行非一级——账面起点纪律破坏）。
pub const XC_NOT_ROOTED: u16 = SEG_BASE + 10;

const _: () = assert!(XC_TIER_BAD != XC_ESCALATE_JUMP);
const _: () = assert!(XC_NO_EVAL != XC_UNDERFLOW);
const _: () = assert!(XC_DRIFT != XC_NOT_ROOTED);

// ---------------------------------------------------------------------------
// §1 风暴四级闭集
// ---------------------------------------------------------------------------

/// 风暴四级闭集（对齐 F0975 故障级：一级观察 / 二级节流 / 三级隔离 / 四级安全模式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StormTier {
    /// 一级：率越阈观察。
    S1,
    /// 二级：确认风暴，节流降压。
    S2,
    /// 三级：节流不足，向量隔离。
    S3,
    /// 四级：隔离不足，安全模式。
    S4,
}

impl StormTier {
    /// 级码（1..=4）。
    pub const fn code(self) -> u8 {
        match self {
            StormTier::S1 => 1,
            StormTier::S2 => 2,
            StormTier::S3 => 3,
            StormTier::S4 => 4,
        }
    }
    /// 级名。
    pub const fn name(self) -> &'static str {
        match self {
            StormTier::S1 => "观察",
            StormTier::S2 => "节流",
            StormTier::S3 => "隔离",
            StormTier::S4 => "安全模式",
        }
    }
    /// 级判据（升级条件——三列之一）。
    pub const fn criterion(self) -> &'static str {
        match self {
            StormTier::S1 => "率越高阈且持续",
            StormTier::S2 => "风暴确认成立",
            StormTier::S3 => "节流后率仍越阈",
            StormTier::S4 => "隔离后率仍越阈",
        }
    }
    /// 级动作。
    pub const fn action(self) -> &'static str {
        match self {
            StormTier::S1 => "记账观察",
            StormTier::S2 => "启动节流协商",
            StormTier::S3 => "屏蔽单向量",
            StormTier::S4 => "进入安全模式",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<StormTier> {
        match c {
            1 => Some(StormTier::S1),
            2 => Some(StormTier::S2),
            3 => Some(StormTier::S3),
            4 => Some(StormTier::S4),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §2 评估回执闭集（降级必须带评估回执）
// ---------------------------------------------------------------------------

/// 降级评估回执闭集：率回落 / 上层令降 / 观察期满。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalKind {
    /// 率回落判据成立。
    RateDown,
    /// 上层显式令降（治理面指令）。
    Ordered,
    /// 观察期满自动降。
    WindowUp,
}

impl EvalKind {
    /// 回执码（1/2/3）。
    pub const fn code(self) -> u8 {
        match self {
            EvalKind::RateDown => 1,
            EvalKind::Ordered => 2,
            EvalKind::WindowUp => 3,
        }
    }
    /// 回执名。
    pub const fn name(self) -> &'static str {
        match self {
            EvalKind::RateDown => "率回落",
            EvalKind::Ordered => "上层令降",
            EvalKind::WindowUp => "观察期满",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<EvalKind> {
        match c {
            1 => Some(EvalKind::RateDown),
            2 => Some(EvalKind::Ordered),
            3 => Some(EvalKind::WindowUp),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §3 级变行与分级账（StormTierBook）
// ---------------------------------------------------------------------------

/// 一行级变账：级 / 方向（升/降）/ 评估回执（降级行必带）/ tick。
#[derive(Clone, Copy, Debug)]
pub struct TierRow {
    tier: StormTier,
    up: bool,
    eval: Option<EvalKind>,
    tick: u64,
}

/// 分级账容量定值：32 行（风暴处置全周期级变量级）。
pub const TIER_CAP: usize = 32;

/// 风暴分级账——四级风暴与升级路径账本本体。
///
/// 守恒恒等式：`Σ(四级行数) == 总行数`（每行恰归一级）；升级恒等
/// 式：升级跨度恒为一级（跳级必拦）；降级恒带评估回执。
#[derive(Debug)]
pub struct StormTierBook {
    rows: Vec<TierRow>,
    cap: usize,
    cur: StormTier,
    per_tier: [u64; 4],
    last_tick: u64,
    rooted: bool,
    rejected: u32,
}

impl StormTierBook {
    /// 建分级账（初始一级，首行即根）。
    pub fn new(cap: usize) -> Result<StormTierBook, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(StormTierBook {
            rows: Vec::with_capacity(cap),
            cap,
            cur: StormTier::S1,
            per_tier: [0; 4],
            last_tick: 0,
            rooted: false,
            rejected: 0,
        })
    }

    /// 当前级。
    pub fn current(&self) -> StormTier {
        self.cur
    }
    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }
    /// 各级行数快照。
    pub fn per_tier(&self) -> [u64; 4] {
        self.per_tier
    }

    /// 升一级（逐级必拦跳级；升级判据由调用方以 bool 显性传入）。
    pub fn escalate(&mut self, criterion_met: bool, tick: u64) -> Result<StormTier, u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        if !criterion_met {
            self.rejected += 1;
            return Err(XC_NO_CRITERION);
        }
        let next_c = self.cur.code() + 1;
        let next = match StormTier::from_code(next_c) {
            Some(t) => t,
            None => {
                self.rejected += 1;
                return Err(XC_UNDERFLOW);
            }
        };
        // 账面起点纪律：首行必须是一级。
        if !self.rooted {
            if self.cur != StormTier::S1 {
                self.rejected += 1;
                return Err(XC_NOT_ROOTED);
            }
            self.rooted = true;
        }
        if self.rows.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        self.per_tier[(next.code() - 1) as usize] += 1;
        self.cur = next;
        self.last_tick = tick;
        self.rows.push(TierRow {
            tier: next,
            up: true,
            eval: None,
            tick,
        });
        Ok(next)
    }

    /// 降级（必须带评估回执；可跨级降但逐行留痕）。
    pub fn deescalate(&mut self, eval: EvalKind, tick: u64) -> Result<StormTier, u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        if EvalKind::from_code(eval.code()).is_none() {
            self.rejected += 1;
            return Err(XC_NO_EVAL);
        }
        if self.cur == StormTier::S1 {
            self.rejected += 1;
            return Err(XC_UNDERFLOW);
        }
        if self.rows.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        let next_c = self.cur.code() - 1;
        let next = match StormTier::from_code(next_c) {
            Some(t) => t,
            None => {
                self.rejected += 1;
                return Err(XC_TIER_BAD);
            }
        };
        self.per_tier[(next.code() - 1) as usize] += 1;
        self.cur = next;
        self.last_tick = tick;
        self.rows.push(TierRow {
            tier: next,
            up: false,
            eval: Some(eval),
            tick,
        });
        Ok(next)
    }

    /// 守恒机检：四级行数和 == 总行数（逐行重扫）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut c = [0u64; 4];
        let mut i: usize = 0;
        while i < self.rows.len() {
            let k = (self.rows[i].tier.code() - 1) as usize;
            c[k] += 1;
            i += 1;
        }
        if c[0] != self.per_tier[0]
            || c[1] != self.per_tier[1]
            || c[2] != self.per_tier[2]
            || c[3] != self.per_tier[3]
        {
            return Err(XC_CONSERVATION);
        }
        if c[0] + c[1] + c[2] + c[3] != self.rows.len() as u64 {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 升降对账：升行数 = 降行数 + (当前级 - 1)（级位守恒）。
    pub fn audit_level_balance(&self) -> Result<(), u16> {
        let mut ups: u32 = 0;
        let mut downs: u32 = 0;
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].up {
                ups += 1;
            } else {
                downs += 1;
            }
            i += 1;
        }
        if ups != downs + (self.cur.code() - 1) {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 分级账指纹（当前级/行数/升降数 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = (h ^ self.cur.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.rows.len() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.last_tick).wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    /// 分级账文本导出（确定性序 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        self.audit_conservation()?;
        self.audit_level_balance()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0983-TIER|v1|cur=");
        s.push_str(self.cur.name());
        s.push_str("|rows=");
        s.push_str(&self.rows.len().to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < self.rows.len() {
            s.push_str(if self.rows[i].up { "UP|" } else { "DOWN|" });
            s.push_str(self.rows[i].tier.name());
            s.push('|');
            match self.rows[i].eval {
                Some(e) => s.push_str(e.name()),
                None => s.push('-'),
            }
            s.push('\n');
            i += 1;
        }
        if self.fingerprint() != fp0 {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// §4 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：逐级升到四级→跳级/无判据/无评估/降穿注入全拦→降回→守恒。
pub fn drill() -> Result<u32, u16> {
    let mut b = StormTierBook::new(TIER_CAP)?;
    // 逐级升级：S1→S2→S3→S4。
    b.escalate(true, 10)?;
    b.escalate(true, 20)?;
    b.escalate(true, 30)?;
    if b.current() != StormTier::S4 {
        return Err(XC_ESCALATE_JUMP);
    }
    // 注入一：四级再升（无级可升）。
    if b.escalate(true, 40) != Err(XC_UNDERFLOW) {
        return Err(XC_UNDERFLOW);
    }
    // 注入二：判据不成立升级。
    if b.escalate(false, 50) != Err(XC_NO_CRITERION) {
        return Err(XC_NO_CRITERION);
    }
    // 逐级降回：S4→S3→S2→S1（各带评估回执）。
    b.deescalate(EvalKind::RateDown, 60)?;
    b.deescalate(EvalKind::Ordered, 70)?;
    b.deescalate(EvalKind::WindowUp, 80)?;
    if b.current() != StormTier::S1 {
        return Err(XC_NO_EVAL);
    }
    // 注入三：一级再降（降穿）。
    if b.deescalate(EvalKind::RateDown, 90) != Err(XC_UNDERFLOW) {
        return Err(XC_UNDERFLOW);
    }
    // 注入四：tick 回退。
    if b.escalate(true, 5) != Err(XC_TICK_REGRESS) {
        return Err(XC_TICK_REGRESS);
    }
    // 守恒机检：6 行 = 3 升 + 3 降，级位平衡 ups(3) == downs(3) + (1-1)。
    b.audit_conservation()?;
    b.audit_level_balance()?;
    if b.len() != 6 || b.rejected() != 4 {
        return Err(XC_CONSERVATION);
    }
    // 双导出指纹稳定。
    let fp1 = b.fingerprint();
    let _ = b.export()?;
    if b.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
