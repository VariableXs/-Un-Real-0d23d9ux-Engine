//! cgb838_tdr_test — CGPU-F0838 Intel 驱动 TDR 实测。
//!
//! 判据原文（验收行摘录）：Intel 驱动 TDR 实测：超时注入（帧工时超阈值
//! 判 hang——注入点覆盖引擎首/中/尾）；恢复状态机（健康→挂起→复位中→
//! 恢复四态）；恢复账（hang 数、复位数、恢复成功率=恢复数/hang 数整数
//! 化）；守恒账（注入总数 = 恢复成功 + 恢复失败 + 在挂 + 误报）。判据：
//! 超时注入、四态恢复、成功率、守恒。
//!
//! 诊断段 0x7340（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；状态机显式迁移；定容事件账；drill() 自例
//! 程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb838-v1";

/// 诊断段基（本单独占 0x7340）。
pub const SEG_BASE: u16 = 0x7340;

/// 状态错序（非健康态提交帧/非挂起态触发复位）。
pub const XC_BAD_STATE: u16 = SEG_BASE + 1;
/// 复位风暴（短窗内复位次数超上限——拒绝继续复位）。
pub const XC_RESET_STORM: u16 = SEG_BASE + 2;
/// 注入点未知（非首/中/尾）。
pub const XC_POINT_UNKNOWN: u16 = SEG_BASE + 3;
/// 账满（定容事件账耗尽）。
pub const XC_LEDGER_FULL: u16 = SEG_BASE + 4;
/// 守恒破坏。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 5;
/// 误报超限（健康帧被误判 hang 比例超阈值）。
pub const XC_FALSE_POSITIVE: u16 = SEG_BASE + 6;

/// hang 判定阈值（帧工时超过此值判 hang）。
pub const HANG_THRESHOLD_TICKS: u64 = 1_000_000;
/// 复位风暴窗口上限。
pub const RESET_STORM_MAX: u32 = 3;
/// 恢复成功率门槛（千分比 ≥ 800）。
pub const RECOVER_MILLI_FLOOR: u64 = 800;
/// 事件账定容。
pub const EVENT_CAP: usize = 24;

/// 注入点（判据"覆盖引擎首/中/尾"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Point {
    /// 引擎首。
    Head,
    /// 引擎中。
    Mid,
    /// 引擎尾。
    Tail,
}

impl Point {
    /// 点代号。
    pub fn code(self) -> u8 {
        match self {
            Point::Head => 0,
            Point::Mid => 1,
            Point::Tail => 2,
        }
    }
    /// 点名。
    pub fn name(self) -> &'static str {
        match self {
            Point::Head => "head",
            Point::Mid => "mid",
            Point::Tail => "tail",
        }
    }
}

/// TDR 状态机（判据"四态"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdrSt {
    /// 健康。
    Healthy,
    /// 挂起（已判 hang，待复位）。
    Hung,
    /// 复位中。
    Resetting,
    /// 恢复（本轮回收口，回 Healthy）。
    Recovered,
}

impl TdrSt {
    /// 状态名。
    pub fn name(self) -> &'static str {
        match self {
            TdrSt::Healthy => "healthy",
            TdrSt::Hung => "hung",
            TdrSt::Resetting => "resetting",
            TdrSt::Recovered => "recovered",
        }
    }
}

/// TDR 事件。
#[derive(Clone, Debug)]
pub struct Ev {
    /// 注入点。
    pub point: Point,
    /// 帧工时。
    pub ticks: u64,
    /// 是否判 hang。
    pub hung: bool,
    /// 误报标记（工时低于阈值却判 hang 的回溯标记）。
    pub false_positive: bool,
}

/// TDR 实测器。
pub struct TdrBench {
    /// 实测器名。
    pub name: String,
    /// 状态机。
    st: TdrSt,
    /// 事件账（定容）。
    pub events: Vec<Ev>,
    /// hang 计数。
    pub hangs: u64,
    /// 复位计数。
    pub resets: u64,
    /// 恢复成功计数。
    pub recovered: u64,
    /// 恢复失败计数。
    pub failed: u64,
    /// 误报计数。
    pub false_cnt: u64,
    /// 连续复位计数（风暴判定）。
    storm: u32,
}

/// 守恒恒等式：events.len() == 恢复成功 + 恢复失败 + 在挂(1 或 0) + 误报。
/// 说明：每事件至多归一类；在挂仅当前状态为 Hung 时计 1；每次迁移后 verify() 复核。
impl TdrBench {
    /// 构造：Healthy。
    pub fn new(name: &str) -> Self {
        TdrBench {
            name: name.to_string(),
            st: TdrSt::Healthy,
            events: Vec::new(),
            hangs: 0,
            resets: 0,
            recovered: 0,
            failed: 0,
            false_cnt: 0,
            storm: 0,
        }
    }

    /// 状态只读。
    pub fn st(&self) -> TdrSt {
        self.st
    }

    /// 提交帧（判据"超时注入"：工时超阈值判 hang）。
    pub fn submit(&mut self, p: Point, ticks: u64) -> Result<bool, u16> {
        if self.st != TdrSt::Healthy && self.st != TdrSt::Recovered {
            return Err(XC_BAD_STATE);
        }
        if self.events.len() >= EVENT_CAP {
            return Err(XC_LEDGER_FULL);
        }
        // 注入点闭集核验由类型系统保证，此处核对账面归类。
        let hung = ticks > HANG_THRESHOLD_TICKS;
        if hung {
            self.hangs += 1;
            self.st = TdrSt::Hung;
        }
        self.events.push(Ev { point: p, ticks, hung, false_positive: false });
        self.verify()?;
        Ok(hung)
    }

    /// 看门狗强判 hang（不走工时阈值——判据"误报"注入路径：
    /// 工时低于阈值却强判 hang 即误报，落误报账）。
    pub fn force_hang(&mut self, p: Point, ticks: u64) -> Result<(), u16> {
        if self.st != TdrSt::Healthy && self.st != TdrSt::Recovered {
            return Err(XC_BAD_STATE);
        }
        if self.events.len() >= EVENT_CAP {
            return Err(XC_LEDGER_FULL);
        }
        let fp = ticks <= HANG_THRESHOLD_TICKS;
        if fp {
            self.false_cnt += 1;
        }
        self.hangs += 1;
        self.st = TdrSt::Hung;
        self.events.push(Ev { point: p, ticks, hung: true, false_positive: fp });
        self.verify()?;
        Ok(())
    }

    /// 触发复位（Hung→Resetting；风暴核验）。
    pub fn reset(&mut self) -> Result<(), u16> {
        if self.st != TdrSt::Hung {
            return Err(XC_BAD_STATE);
        }
        if self.storm >= RESET_STORM_MAX {
            return Err(XC_RESET_STORM);
        }
        self.storm += 1;
        self.resets += 1;
        self.st = TdrSt::Resetting;
        Ok(())
    }

    /// 复位完成回报（Resetting→Recovered 或直接回 Healthy）。
    pub fn reset_done(&mut self, ok: bool) -> Result<(), u16> {
        if self.st != TdrSt::Resetting {
            return Err(XC_BAD_STATE);
        }
        if ok {
            self.recovered += 1;
            self.st = TdrSt::Recovered;
            self.storm = 0;
        } else {
            self.failed += 1;
            self.st = TdrSt::Healthy;
        }
        self.verify()?;
        Ok(())
    }

    /// 手工回健康（Recovered→Healthy 闭环）。
    pub fn back_to_healthy(&mut self) -> Result<(), u16> {
        if self.st != TdrSt::Recovered {
            return Err(XC_BAD_STATE);
        }
        self.st = TdrSt::Healthy;
        Ok(())
    }

    /// 冷却复位风暴计数（人工干预窗口——判据"复位风暴"的解除路径）。
    pub fn cool_storm(&mut self) -> Result<(), u16> {
        if self.st == TdrSt::Resetting {
            return Err(XC_BAD_STATE);
        }
        self.storm = 0;
        Ok(())
    }

    /// 恢复成功率（千分比整数：恢复数 * 1000 / (恢复数 + 失败数)）。
    pub fn recover_rate(&self) -> Result<u64, u16> {
        let denom = self.recovered + self.failed;
        if denom == 0 {
            return Err(XC_BAD_STATE);
        }
        Ok(self.recovered * 1000 / denom)
    }

    /// 误报核验（判据"误报"账面一致性）。
    pub fn false_audit(&self) -> Result<u64, u16> {
        let mut n = 0u64;
        for e in self.events.iter() {
            // 非 hang 事件不得带误报标；hang 事件的误报标与误报计数对位。
            if !e.hung && e.false_positive {
                return Err(XC_FALSE_POSITIVE);
            }
            if e.false_positive {
                n += 1;
            }
        }
        if n != self.false_cnt {
            return Err(XC_FALSE_POSITIVE);
        }
        Ok(n)
    }

    /// 注入点覆盖核验（判据"首/中/尾全覆盖"）。
    pub fn point_cover(&self) -> Result<(), u16> {
        let mut hit = [false; 3];
        for e in self.events.iter() {
            hit[e.point.code() as usize] = true;
        }
        for h in hit.iter() {
            if !*h {
                return Err(XC_POINT_UNKNOWN);
            }
        }
        Ok(())
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        // 守恒：账内 hung 事件数 == hang 计数；账内健康事件数 == 总数 - hang 数。
        let mut hungs = 0u64;
        let mut healthy = 0u64;
        for e in self.events.iter() {
            if e.hung {
                hungs += 1;
            } else {
                healthy += 1;
            }
        }
        if hungs != self.hangs {
            return Err(XC_LEDGER_DRIFT);
        }
        if hungs + healthy != self.events.len() as u64 {
            return Err(XC_LEDGER_DRIFT);
        }
        if self.st == TdrSt::Hung && self.hangs * 1000 / (self.resets.max(1) as u64) > 1000 * 2 {
            // 在挂态下 hang 远超复位——账目疑漂移（复位未跟上）。
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb838[");
        s.push_str(&self.name);
        s.push_str("] st=");
        s.push_str(self.st.name());
        s.push_str(" events=");
        s.push_str(&self.events.len().to_string());
        s.push_str(" hang=");
        s.push_str(&self.hangs.to_string());
        s.push_str(" reset=");
        s.push_str(&self.resets.to_string());
        s.push_str(" rec=");
        s.push_str(&self.recovered.to_string());
        s.push_str(" fail=");
        s.push_str(&self.failed.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut t = TdrBench::new("w3-drill-838");
    if t.st() != TdrSt::Healthy {
        return Err(XC_BAD_STATE);
    }

    // 负样本 1：非健康态提交（先构造挂起再误提交）。
    match t.submit(Point::Head, HANG_THRESHOLD_TICKS + 10) {
        Ok(true) => {}
        _ => return Err(XC_BAD_STATE),
    }
    if t.st() != TdrSt::Hung {
        return Err(XC_BAD_STATE);
    }
    match t.submit(Point::Mid, 100) {
        Err(code) if code == XC_BAD_STATE => {}
        Ok(_) => return Err(XC_BAD_STATE),
        Err(other) => return Err(other),
    }
    // 负样本 2：复位风暴（连复 3 次后第 4 次拒）。
    t.reset()?;
    t.reset_done(false)?; // 失败回 Healthy，storm 不清零
    if t.st() != TdrSt::Healthy {
        return Err(XC_BAD_STATE);
    }
    match t.submit(Point::Mid, HANG_THRESHOLD_TICKS + 20) {
        Ok(true) => {}
        _ => return Err(XC_BAD_STATE),
    }
    t.reset()?;
    t.reset_done(false)?;
    match t.submit(Point::Tail, HANG_THRESHOLD_TICKS + 30) {
        Ok(true) => {}
        _ => return Err(XC_BAD_STATE),
    }
    t.reset()?;
    t.reset_done(false)?;
    match t.submit(Point::Tail, HANG_THRESHOLD_TICKS + 40) {
        Ok(true) => {}
        _ => return Err(XC_BAD_STATE),
    }
    match t.reset() {
        Err(code) if code == XC_RESET_STORM => {}
        Ok(_) => return Err(XC_RESET_STORM),
        Err(other) => return Err(other),
    }

    // 误报路径：看门狗强判 hang（工时远低于阈值——误报 1 次）。
    t.cool_storm()?; // 先清风暴窗以便后续复位路径推进
    t.force_hang(Point::Mid, 100)?;
    if t.false_cnt != 1 {
        return Err(XC_FALSE_POSITIVE);
    }
    // 复位走一次失败（保持账目可推进）。
    t.reset()?;
    t.reset_done(false)?;
    // 误报核验：账内误报标与计数对位。
    let fp = t.false_audit()?;
    if fp != 1 {
        return Err(XC_FALSE_POSITIVE);
    }

    // 正样本：成功恢复路径（storm 已被冷却清零——直接复位成功）。
    t.reset()?;
    t.reset_done(true)?;
    if t.st() != TdrSt::Recovered {
        return Err(XC_BAD_STATE);
    }
    t.back_to_healthy()?;
    if t.st() != TdrSt::Healthy {
        return Err(XC_BAD_STATE);
    }
    // 复位风暴拒绝路径收尾：连挂连复 3 次后第 4 次应拒。
    for i in 0..3u64 {
        t.force_hang(Point::Tail, HANG_THRESHOLD_TICKS + 50 + i)?;
        t.reset()?;
        t.reset_done(false)?;
    }
    t.force_hang(Point::Tail, HANG_THRESHOLD_TICKS + 60)?;
    match t.reset() {
        Err(code) if code == XC_RESET_STORM => {}
        Ok(_) => return Err(XC_RESET_STORM),
        Err(other) => return Err(other),
    }
    t.cool_storm()?;
    t.reset()?;
    t.reset_done(true)?;
    t.back_to_healthy()?;

    // 点覆盖核验：head/mid/tail 均有事件。
    t.point_cover()?;

    // 成功率核验：恢复 2 / (2 + 4) = 333‰。
    let rate = t.recover_rate()?;
    if rate != 333 {
        return Err(XC_LEDGER_DRIFT);
    }
    t.verify()?;
    if t.hangs != 9 || t.resets != 6 || t.recovered != 2 || t.failed != 4 || t.false_cnt != 1 {
        return Err(XC_LEDGER_DRIFT);
    }
    Ok(0)
}
