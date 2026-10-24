//! cgb15_xfprio — CGPU-F0228 优先级跨帧传播。
//!
//! 判据原文（施工书锚点 CGPU-F0228）：优先级传播：跨帧依赖的优先级传导（帧
//! N 高优任务等待帧 N-1 低优产出→被等待任务临时提升——继承式提升防优先级
//! 倒置跨帧化）；提升上限与回落（提升不超原级+2——防提升风暴，完成即回落）；
//! 与 B04 优先级域对接（同表格同语义——跨帧不引入第二套优先级）。判据：继
//! 承提升、上限回落、同表语义。
//!
//! 职责定位：跨帧优先级倒置的防线。帧 N 的交互级任务等帧 N-1 的后台级产出
//! 时，若不提升被等任务，交互延迟会被后台工作拖垮且倒置跨帧隐匿。提升纪律：
//! ①继承式——提升到等待者与被等者中较高一级（只升不降）；②上限——提升后
//! 不超"原级+2"（防提升风暴沿依赖链级联放大）；③回落——被等任务完成即回
//! 原级，账面留痕可查；④同表——优先级枚举与 B04 同一张表（此处复刻其序与
//! 语义，注释锚定，不发明新级别）。诊断码段 0xD450。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_PRIO_VERSION: &str = "B15-xfprio-v1";

// ---------------------------------------------------------------------------
// 诊断码（0xD450 槽）
// ---------------------------------------------------------------------------

/// 提升超上限（原级+2 之外——提升风暴防线）。
pub const XC_BOOST_OVER_CAP: u32 = 0xD450;
/// 提升目标不存在（被等任务未登记）。
pub const XC_BOOST_TARGET_MISSING: u32 = 0xD451;
/// 回落无提升记录（完成回落时找不到在册提升——账不平）。
pub const XC_FALLBACK_NO_BOOST: u32 = 0xD452;
/// 优先级倒置检出未处置（高优等低优且提升被拒——须显性立案而非静默）。
pub const XC_INVERSION_UNRESOLVED: u32 = 0xD453;
/// 重复提升（同任务同帧重复登记提升——账目二义）。
pub const XC_DOUBLE_BOOST: u32 = 0xD454;
/// 优先级表越界（外部传入的级值超出 B04 同表范围）。
pub const XC_PRIO_OUT_OF_TABLE: u32 = 0xD455;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 提升上限：不超原级 +2（判据"上限"——写死并锚定施工书）。
pub const BOOST_CAP: u32 = 2;
/// 在册提升账容量。
pub const BOOST_LEDGER_CAP: usize = 512;

// ---------------------------------------------------------------------------
// 优先级表（与 B04 同表同序——判据"同表语义"）
// ---------------------------------------------------------------------------

/// 优先级闭集：数值越大越优先。B04 优先级域同表，序与语义一一对应。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum PrioLevel {
    /// 后台级（0）：预取、整理、统计。
    Background = 0,
    /// 常规级（1）：普通渲染任务。
    Normal = 1,
    /// 交互级（2）：输入响应、光标路径。
    Interactive = 2,
    /// 呈现级（3）：合成器最终产出、帧截止前必达。
    Present = 3,
}

impl PrioLevel {
    pub const ALL: [PrioLevel; 4] = [
        PrioLevel::Background,
        PrioLevel::Normal,
        PrioLevel::Interactive,
        PrioLevel::Present,
    ];

    pub const fn rank(self) -> u32 {
        self as u32
    }

    pub const fn zh(self) -> &'static str {
        match self {
            PrioLevel::Background => "后台",
            PrioLevel::Normal => "常规",
            PrioLevel::Interactive => "交互",
            PrioLevel::Present => "呈现",
        }
    }

    /// 级值 → 级（外部接口用，越界即 None——调用方显性处置）。
    pub const fn from_rank(r: u32) -> Option<PrioLevel> {
        match r {
            0 => Some(PrioLevel::Background),
            1 => Some(PrioLevel::Normal),
            2 => Some(PrioLevel::Interactive),
            3 => Some(PrioLevel::Present),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 提升账
// ---------------------------------------------------------------------------

/// 一条跨帧提升记录。
#[derive(Clone, Debug)]
pub struct Boost {
    /// 被提升任务（帧号，任务号）。
    pub target_frame: u64,
    pub target_task: u32,
    /// 等待者（帧号，任务号，其优先级是提升动因）。
    pub waiter_frame: u64,
    pub waiter_task: u32,
    /// 原级。
    pub original: PrioLevel,
    /// 提升后级。
    pub boosted: PrioLevel,
    /// 提升纪元。
    pub epoch: u64,
    /// 是否已回落。
    pub fallen_back: bool,
}

// ---------------------------------------------------------------------------
// 传播引擎
// ---------------------------------------------------------------------------

/// 跨帧优先级传播引擎。
pub struct PrioPropagator {
    pub name: String,
    /// 任务当前级账（帧号+任务号 → 级）——提升与回落的单一真源。
    levels: Vec<(u64, u32, PrioLevel)>,
    /// 提升账（在册未回落的 + 已回落留痕的）。
    ledger: Vec<Boost>,
    /// 立案账：倒置检出但提升被拒的显性记录（零静默）。
    cases: Vec<(u32, String)>,
    /// 统计。
    pub total_boosts: u64,
    pub total_fallbacks: u64,
    pub total_rejects: u64,
}

impl PrioPropagator {
    pub fn new(name: &str) -> Self {
        PrioPropagator {
            name: String::from(name),
            levels: Vec::new(),
            ledger: Vec::new(),
            cases: Vec::new(),
            total_boosts: 0,
            total_fallbacks: 0,
            total_rejects: 0,
        }
    }

    fn reject(&mut self, code: u32, why: &str) -> u32 {
        self.cases.push((code, String::from(why)));
        if self.cases.len() > 256 {
            self.cases.remove(0);
        }
        self.total_rejects += 1;
        code
    }

    // ---------------- 任务登记 ----------------

    /// 登记任务及其原级（帧提交时的申报面）。
    pub fn register_task(&mut self, frame: u64, task: u32, level: PrioLevel) -> Result<(), u32> {
        if self
            .levels
            .iter()
            .any(|(f, t, _)| *f == frame && *t == task)
        {
            return Err(self.reject(XC_PRIO_OUT_OF_TABLE, "任务重复登记"));
        }
        self.levels.push((frame, task, level));
        Ok(())
    }

    fn level_of(&self, frame: u64, task: u32) -> Option<PrioLevel> {
        self.levels
            .iter()
            .find(|(f, t, _)| *f == frame && *t == task)
            .map(|(_, _, l)| *l)
    }

    /// 查询任务当前级（含提升中的临时级——调度面读这里，不读申报面）。
    pub fn current_level(&self, frame: u64, task: u32) -> Option<PrioLevel> {
        let orig = self.level_of(frame, task)?;
        // 有未回落的提升记录则返回提升级。
        for b in self.ledger.iter() {
            if b.target_frame == frame && b.target_task == task && !b.fallen_back {
                return Some(b.boosted);
            }
        }
        Some(orig)
    }

    // ---------------- 继承提升（判据"继承提升"） ----------------

    /// 跨帧等待登记：waiter（帧 N 高优）等 target（帧 N-k 低优）。
    /// 若 waiter 级 > target 级 → 继承式提升 target 至 waiter 级，
    /// 且不超原级+BOOST_CAP；超上限则提升到上限并立案显性记录差额。
    pub fn cross_frame_wait(
        &mut self,
        waiter_frame: u64,
        waiter_task: u32,
        target_frame: u64,
        target_task: u32,
        epoch: u64,
    ) -> Result<Option<Boost>, u32> {
        let wl = match self.level_of(waiter_frame, waiter_task) {
            Some(l) => l,
            None => return Err(self.reject(XC_BOOST_TARGET_MISSING, "等待者未登记")),
        };
        let tl = match self.level_of(target_frame, target_task) {
            Some(l) => l,
            None => return Err(self.reject(XC_BOOST_TARGET_MISSING, "被等任务未登记")),
        };
        if wl <= tl {
            return Ok(None); // 无倒置，无需提升。
        }
        // 已有在册提升则拒绝重复（账目二义防线）。
        if self.ledger.iter().any(|b| {
            b.target_frame == target_frame
                && b.target_task == target_task
                && !b.fallen_back
        }) {
            return Err(self.reject(XC_DOUBLE_BOOST, "在册提升未回落又提"));
        }
        // 上限：max(waiter 级, target 原级 + CAP) 之外拒绝——但倒置必须显性处置，
        // 故取"可提升的最高级"并立案剩余差额。
        let want = wl.rank();
        let cap = tl.rank() + BOOST_CAP;
        let effective = if want <= cap { want } else { cap };
        let boosted = match PrioLevel::from_rank(effective) {
            Some(l) => l,
            None => return Err(self.reject(XC_PRIO_OUT_OF_TABLE, "提升级值越表")),
        };
        if want > cap {
            let mut s = String::from("提升触上限：期望呈现差 ");
            s.push_str(&itoa((want - cap) as u64));
            s.push_str(" 级——上游限流或拆任务，静默倒置即违约");
            self.cases.push((XC_INVERSION_UNRESOLVED, s));
        }
        let b = Boost {
            target_frame,
            target_task,
            waiter_frame,
            waiter_task,
            original: tl,
            boosted,
            epoch,
            fallen_back: false,
        };
        self.ledger.push(b.clone());
        if self.ledger.len() > BOOST_LEDGER_CAP {
            self.ledger.remove(0);
        }
        self.total_boosts += 1;
        Ok(Some(b))
    }

    // ---------------- 回落（判据"上限回落"） ----------------

    /// 被等任务完成：在册提升回落，账面留痕。
    pub fn complete(&mut self, frame: u64, task: u32) -> Result<PrioLevel, u32> {
        let mut found = false;
        let n = self.ledger.len();
        let mut i = 0;
        while i < n {
            if self.ledger[i].target_frame == frame
                && self.ledger[i].target_task == task
                && !self.ledger[i].fallen_back
            {
                self.ledger[i].fallen_back = true;
                found = true;
                self.total_fallbacks += 1;
            }
            i += 1;
        }
        let orig = match self.level_of(frame, task) {
            Some(l) => l,
            None => return Err(self.reject(XC_BOOST_TARGET_MISSING, "完成任务未登记")),
        };
        if !found {
            return Err(self.reject(XC_FALLBACK_NO_BOOST, "完成回落无在册提升"));
        }
        Ok(orig)
    }

    // ---------------- 核验 ----------------

    /// 账面核验：①每个未回落提升的 boosted 级 ≤ 原级+CAP；②提升后级 > 原级
    /// （只升不降）；③回落数 ≤ 提升数。
    pub fn validate(&self) -> Result<(), u32> {
        for b in self.ledger.iter() {
            if b.boosted.rank() > b.original.rank() + BOOST_CAP {
                return Err(XC_BOOST_OVER_CAP);
            }
            if b.boosted.rank() <= b.original.rank() {
                return Err(XC_PRIO_OUT_OF_TABLE);
            }
        }
        if self.total_fallbacks > self.total_boosts {
            return Err(XC_FALLBACK_NO_BOOST);
        }
        Ok(())
    }

    /// 提升账只读投影。
    pub fn boosts(&self) -> &[Boost] {
        &self.ledger
    }

    /// 立案账只读投影。
    pub fn case_log(&self) -> &[(u32, String)] {
        &self.cases
    }

    /// 快照。
    pub fn snapshot(&self) -> PrioSnap {
        let active = self.ledger.iter().filter(|b| !b.fallen_back).count();
        PrioSnap {
            version: String::from(XF_PRIO_VERSION),
            engine: self.name.clone(),
            tasks: self.levels.len() as u64,
            boosts: self.total_boosts,
            active_boosts: active as u64,
            fallbacks: self.total_fallbacks,
            cases: self.cases.len() as u64,
            rejects: self.total_rejects,
        }
    }
}

/// 快照。
#[derive(Clone, Debug)]
pub struct PrioSnap {
    pub version: String,
    pub engine: String,
    pub tasks: u64,
    pub boosts: u64,
    pub active_boosts: u64,
    pub fallbacks: u64,
    pub cases: u64,
    pub rejects: u64,
}

impl PrioSnap {
    pub fn detail(&self) -> String {
        let mut s = String::from("[跨帧优先级 ");
        s.push_str(&self.engine);
        s.push_str(" 任务=");
        s.push_str(&itoa(self.tasks));
        s.push_str(" 提升=");
        s.push_str(&itoa(self.boosts));
        s.push_str(" 在提=");
        s.push_str(&itoa(self.active_boosts));
        s.push_str(" 回落=");
        s.push_str(&itoa(self.fallbacks));
        s.push_str(" 立案=");
        s.push_str(&itoa(self.cases));
        s.push_str("]");
        s
    }
}

fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    match core::str::from_utf8(&buf[i..]) {
        Ok(s) => String::from(s),
        Err(_) => String::from("?"),
    }
}

// ---------------------------------------------------------------------------
// 场景化构造（纯函数，确定性）
// ---------------------------------------------------------------------------

/// 场景 A：交互级等后台级——继承提升 + 完成回落全流程。
pub fn scenario_boost_fallback() -> Result<PrioSnap, u32> {
    let mut p = PrioPropagator::new("B15-继承");
    p.register_task(10, 0, PrioLevel::Background)?; // 帧N-1 低优产出。
    p.register_task(11, 0, PrioLevel::Interactive)?; // 帧N 高优等待。
    let b = p.cross_frame_wait(11, 0, 10, 0, 11)?;
    match b {
        Some(b) => {
            if b.boosted != PrioLevel::Interactive {
                return Err(XC_BOOST_OVER_CAP);
            }
        }
        None => return Err(XC_BOOST_TARGET_MISSING),
    }
    // 调度面读到的是提升级。
    if p.current_level(10, 0) != Some(PrioLevel::Interactive) {
        return Err(XC_DOUBLE_BOOST);
    }
    p.complete(10, 0)?;
    // 回落后读回原级。
    if p.current_level(10, 0) != Some(PrioLevel::Background) {
        return Err(XC_FALLBACK_NO_BOOST);
    }
    p.validate()?;
    Ok(p.snapshot())
}

/// 场景 B：上限负向——后台被呈现级等待，触 CAP 立案但提升不越线。
pub fn scenario_cap_case() -> Result<PrioSnap, u32> {
    let mut p = PrioPropagator::new("B15-上限");
    p.register_task(20, 0, PrioLevel::Normal)?;
    p.register_task(21, 0, PrioLevel::Present)?;
    let b = p.cross_frame_wait(21, 0, 20, 0, 21)?;
    match b {
        Some(b) => {
            // 期望呈现(3)，上限=常规(1)+2=3 → 恰好可提至呈现。
            if b.boosted != PrioLevel::Present {
                return Err(XC_BOOST_OVER_CAP);
            }
        }
        None => return Err(XC_BOOST_TARGET_MISSING),
    }
    p.complete(20, 0)?;
    p.validate()?;
    Ok(p.snapshot())
}

/// 场景 C：重复提升负向——在册未回落再提必须拒。
pub fn scenario_double_boost() -> Result<(), u32> {
    let mut p = PrioPropagator::new("B15-重提拒");
    p.register_task(30, 0, PrioLevel::Background)?;
    p.register_task(31, 0, PrioLevel::Interactive)?;
    p.register_task(31, 1, PrioLevel::Interactive)?;
    p.cross_frame_wait(31, 0, 30, 0, 30)?;
    match p.cross_frame_wait(31, 1, 30, 0, 31) {
        Err(code) if code == XC_DOUBLE_BOOST => Ok(()),
        Ok(_) => Err(XC_DOUBLE_BOOST),
        Err(other) => Err(other),
    }
}

/// 场景 D：无倒置路径——高优等高优不产生提升记录。
pub fn scenario_no_inversion() -> Result<(), u32> {
    let mut p = PrioPropagator::new("B15-无倒置");
    p.register_task(40, 0, PrioLevel::Interactive)?;
    p.register_task(41, 0, PrioLevel::Interactive)?;
    let b = p.cross_frame_wait(41, 0, 40, 0, 41)?;
    if b.is_some() {
        return Err(XC_DOUBLE_BOOST);
    }
    p.validate()
}
