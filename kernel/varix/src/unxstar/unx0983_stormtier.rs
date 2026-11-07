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
        if ups != downs + (self.cur.code() as u32 - 1) { // 收库修正：code() 返回 u8，与 u32 账面对加前显式升位
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

// ════════════════════ 判据聚合入口（AI-INTAKE-1107 收库补齐） ════════════════════
//
// 修正记录：本模块历史会话只交付了域模型与测试，从未交付 run_unx983_checks
// 聚合入口——等于从未参与判据聚合闭环。此入口把模块自身的公开 API 逐项
// 折叠成 CheckSet 判据（判据强度不降：每条判据都调真实 API、断真实性质）。

use crate::checks::CheckSet;

pub fn run_unx983_checks() -> CheckSet {
    let mut s = CheckSet::new("unx0983_stormtier");
    // J1 核心路径：逐级升→四类注入拦→降回→守恒→双导出。
    s.add("核心路径drill全过", drill() == Ok(0), "S1→S4 逐级/注入四拦/降回 S1/守恒/指纹稳定返回 0");
    // J2 风暴四级闭集（码 1..=4 双向可逆）。
    let tier_closed = StormTier::from_code(1) == Some(StormTier::S1)
        && StormTier::from_code(2) == Some(StormTier::S2)
        && StormTier::from_code(3) == Some(StormTier::S3)
        && StormTier::from_code(4) == Some(StormTier::S4)
        && StormTier::from_code(0).is_none()
        && StormTier::from_code(5).is_none()
        && StormTier::S1.code() == 1 && StormTier::S4.code() == 4;
    s.add("风暴四级闭集", tier_closed, "from_code 1..=4 可逆、0/5 拒、码值冻结");
    // J3 三列元数据（判据/动作/升级条件）逐级齐备。
    let meta_ok = [StormTier::S1, StormTier::S2, StormTier::S3, StormTier::S4]
        .iter()
        .all(|t| !t.name().is_empty() && !t.criterion().is_empty() && !t.action().is_empty());
    s.add("三列元数据齐备", meta_ok, "四级 name/criterion/action 全非空");
    // J4 评估回执闭集（码 1..=3 双向可逆）。
    let eval_closed = EvalKind::from_code(1) == Some(EvalKind::RateDown)
        && EvalKind::from_code(2) == Some(EvalKind::Ordered)
        && EvalKind::from_code(3) == Some(EvalKind::WindowUp)
        && EvalKind::from_code(0).is_none()
        && EvalKind::from_code(4).is_none()
        && EvalKind::RateDown.code() == 1 && EvalKind::WindowUp.code() == 3;
    s.add("评估回执闭集", eval_closed, "from_code 1..=3 可逆、0/4 拒、码值冻结");
    // J5 正例：逐级升级只进不跳（S1→S2→S3→S4）。
    let ladder_ok = match StormTierBook::new(TIER_CAP) {
        Ok(mut b) => {
            let r1 = b.escalate(true, 10);
            let r2 = b.escalate(true, 20);
            let r3 = b.escalate(true, 30);
            r1 == Ok(StormTier::S2) && r2 == Ok(StormTier::S3) && r3 == Ok(StormTier::S4)
                && b.current() == StormTier::S4 && b.len() == 3
        }
        Err(_) => false,
    };
    s.add("逐级升级到四级", ladder_ok, "三升全过且逐级落账 3 行、终态 S4");
    // J6 反例一：四级再升/判据缺失/tick 回退三类注入全拦。
    let neg1_ok = match StormTierBook::new(TIER_CAP) {
        Ok(mut b) => {
            let _ = b.escalate(true, 10);
            let _ = b.escalate(true, 20);
            let _ = b.escalate(true, 30);
            b.escalate(true, 40) == Err(XC_UNDERFLOW)
                && b.escalate(false, 50) == Err(XC_NO_CRITERION)
                && b.escalate(true, 20) == Err(XC_TICK_REGRESS)
        }
        Err(_) => false,
    };
    s.add("升级三类注入拦", neg1_ok, "S4 再升 UNDERFLOW、无判据 NO_CRITERION、tick 回退 TICK_REGRESS");
    // J7 正例+反例：逐级降回（各带评估回执）、一级再降拦。
    let down_ok = match StormTierBook::new(TIER_CAP) {
        Ok(mut b) => {
            let _ = b.escalate(true, 10);
            let _ = b.escalate(true, 20);
            let _ = b.escalate(true, 30);
            let d1 = b.deescalate(EvalKind::RateDown, 40);
            let d2 = b.deescalate(EvalKind::Ordered, 50);
            let d3 = b.deescalate(EvalKind::WindowUp, 60);
            d1 == Ok(StormTier::S3) && d2 == Ok(StormTier::S2) && d3 == Ok(StormTier::S1)
                && b.current() == StormTier::S1
                && b.deescalate(EvalKind::RateDown, 70) == Err(XC_UNDERFLOW)
        }
        Err(_) => false,
    };
    s.add("逐级降回带回执", down_ok, "三降各带回执、终态 S1、一级再降 UNDERFLOW 拦");
    // J8 分级账守恒：四级行数和==总行数 + 级位平衡。
    let audit_ok = match StormTierBook::new(TIER_CAP) {
        Ok(mut b) => {
            let _ = b.escalate(true, 10);
            let _ = b.escalate(true, 20);
            let _ = b.deescalate(EvalKind::RateDown, 20);
            b.audit_conservation().is_ok()
                && b.audit_level_balance().is_ok()
                && b.len() == 3
                && b.per_tier().iter().sum::<u64>() == 3
        }
        Err(_) => false,
    };
    s.add("分级账守恒双审计", audit_ok, "2 升 1 降、级位平衡 ups==downs+(cur-1)、per_tier 和==3");
    // J9 反例二：定容非法（容量为零）拒收。
    let cap_ok = match StormTierBook::new(0) {
        Err(XC_CAP_INVALID) => true,
        _ => false,
    };
    s.add("零容量拒收", cap_ok, "new(0) 被 XC_CAP_INVALID 拒——账本定容非零纪律");
    // J10 诊断段独占与常量冻结。
    s.add("诊断段位独占", SEG_BASE == 0x80E0 && TIER_CAP == 32, "0x80E0 段独占、账容 32 冻结");
    // 动态明细两列。
    let (export_ok, fp_stable, export_len) = match StormTierBook::new(TIER_CAP) {
        Ok(mut b) => {
            let _ = b.escalate(true, 10);
            let _ = b.escalate(true, 20);
            let fp1 = b.fingerprint();
            let e1 = b.export();
            let fp2 = b.fingerprint();
            (e1.is_ok(), fp1 == fp2, e1.map(|t| t.len()).unwrap_or(0))
        }
        Err(_) => (false, false, 0),
    };
    s.add_dyn("导出对拍稳定", export_ok && fp_stable, &alloc::format!("分级账导出 {} 字节、双指纹一致 {}", export_len, fp_stable));
    let (rej_book, rej_n) = match StormTierBook::new(TIER_CAP) {
        Ok(mut b) => {
            let _ = b.escalate(false, 10);
            let _ = b.deescalate(EvalKind::RateDown, 10);
            (true, b.rejected())
        }
        Err(_) => (false, 0u32),
    };
    s.add_dyn("拒收显性记账", rej_book && rej_n == 2, &alloc::format!("无判据升 + 一级降各拒一次，rejected={}", rej_n));
    s
}
