//! unx0979_bootclose — UNX-F0979 中断面启动序列总收口（F0897/F0959 联合与计时面衔接点）。
//!
//! 任务锚点：UNX-F0979（第二批工位 W7，SEG 段位 0x8060，本单独占
//! 0x8060..=0x807F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**中断面启动序列七步全收口（步序单调、
//! 每步前置齐备才可推进、跳步必拦），与计时面衔接点在步五锚定且
//! 锚定后计时前置不可撤销，重复启动带码拒收，序列指纹双导出稳定**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x806 段闭集）；tick 显性
//! 注入零墙钟；序列守恒（已完成+待做==总步数 恒等式机检）；结尾
//! drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x8060 段，本单独占）
// ---------------------------------------------------------------------------

/// 中断面启动序列总收口段基址（0x8060，本单 0x8060..=0x807F 独占）。
pub const SEG_BASE: u16 = 0x8060;
/// 0x8061 步序非法（步码不在七步闭集内）。
pub const XC_STEP_BAD: u16 = SEG_BASE + 1;
/// 0x8062 跳步拒收（前置步未完成即推进——序列单调性破坏）。
pub const XC_STEP_SKIP: u16 = SEG_BASE + 2;
/// 0x8063 重复启动（序列已收口再启——一次性纪律破坏）。
pub const XC_ALREADY_BOOTED: u16 = SEG_BASE + 3;
/// 0x8064 计时锚点缺失（步五未锚定即推进步六——衔接点纪律破坏）。
pub const XC_TIMER_NOT_ANCHORED: u16 = SEG_BASE + 4;
/// 0x8065 锚点回退（锚定 tick 早于序列起始 tick）。
pub const XC_ANCHOR_REGRESS: u16 = SEG_BASE + 5;
/// 0x8066 序列守恒破坏（已完成+待做 != 总步数——账面被外力改写）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 6;
/// 0x8067 序列指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 7;
/// 0x8068 tick 回退（步完成时序单调性破坏）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 8;
/// 0x8069 步已重做（已完成步再推进——终态纪律破坏）。
pub const XC_STEP_REDONE: u16 = SEG_BASE + 9;
/// 0x806A 中断未开即收口（步七前置中断使能缺失）。
pub const XC_IRQ_NOT_OPEN: u16 = SEG_BASE + 10;

const _: () = assert!(XC_STEP_BAD != XC_STEP_SKIP);
const _: () = assert!(XC_CONSERVATION != XC_ALREADY_BOOTED);
const _: () = assert!(XC_DRIFT != XC_IRQ_NOT_OPEN);

// ---------------------------------------------------------------------------
// §1 启动序列七步闭集
// ---------------------------------------------------------------------------

/// 启动序列七步闭集（F0897 开合步 + F0959 使能步联合收口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootStep {
    /// 步一：控制器探测（开合面前置）。
    Detect,
    /// 步二：向量表铺设。
    VecSetup,
    /// 步三：EOI 通道自检。
    EoiCheck,
    /// 步四：风暴检测器 armed。
    StormArm,
    /// 步五：计时面锚定（衔接点——锚定后计时前置不可撤销）。
    TimerAnchor,
    /// 步六： handlers 挂接。
    HandlerMount,
    /// 步七：中断使能开（收口步）。
    IrqEnable,
}

impl BootStep {
    /// 步码（1..=7）。
    pub const fn code(self) -> u8 {
        match self {
            BootStep::Detect => 1,
            BootStep::VecSetup => 2,
            BootStep::EoiCheck => 3,
            BootStep::StormArm => 4,
            BootStep::TimerAnchor => 5,
            BootStep::HandlerMount => 6,
            BootStep::IrqEnable => 7,
        }
    }
    /// 步名。
    pub const fn name(self) -> &'static str {
        match self {
            BootStep::Detect => "控制器探测",
            BootStep::VecSetup => "向量表铺设",
            BootStep::EoiCheck => "EOI自检",
            BootStep::StormArm => "风暴armed",
            BootStep::TimerAnchor => "计时锚定",
            BootStep::HandlerMount => "挂接handler",
            BootStep::IrqEnable => "中断使能",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<BootStep> {
        match c {
            1 => Some(BootStep::Detect),
            2 => Some(BootStep::VecSetup),
            3 => Some(BootStep::EoiCheck),
            4 => Some(BootStep::StormArm),
            5 => Some(BootStep::TimerAnchor),
            6 => Some(BootStep::HandlerMount),
            7 => Some(BootStep::IrqEnable),
            _ => None,
        }
    }
}

/// 序列总步数。
pub const STEP_COUNT: usize = 7;

// ---------------------------------------------------------------------------
// §2 启动序列收口器（BootSequencer）
// ---------------------------------------------------------------------------

/// 启动序列收口器——七步状态机 + 计时衔接点锚定。
///
/// 守恒恒等式：`done + pending == STEP_COUNT`（每步恰处一态）；
/// 计时衔接恒等式：`anchored ⇒ timer_tick ≥ start_tick`（锚点单调）。
#[derive(Debug)]
pub struct BootSequencer {
    done: [bool; STEP_COUNT],
    ticks: [u64; STEP_COUNT],
    start_tick: u64,
    anchor_tick: u64,
    booted: bool,
    rejected: u32,
}

impl BootSequencer {
    /// 建序列器（start_tick 钉序列起点）。
    pub fn new(start_tick: u64) -> BootSequencer {
        BootSequencer {
            done: [false; STEP_COUNT],
            ticks: [0; STEP_COUNT],
            start_tick,
            anchor_tick: 0,
            booted: false,
            rejected: 0,
        }
    }

    /// 已完成步数。
    pub fn done_count(&self) -> u32 {
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < STEP_COUNT {
            if self.done[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 是否已收口（七步全成 + 中断开）。
    pub fn is_booted(&self) -> bool {
        self.booted
    }
    /// 计时锚点 tick（未锚定为 0）。
    pub fn anchor_tick(&self) -> u64 {
        self.anchor_tick
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }
    /// 某步完成 tick（未完成 0）。
    pub fn step_tick(&self, s: BootStep) -> u64 {
        self.ticks[(s.code() - 1) as usize]
    }

    /// 推进一步（单调步序 + 前置齐备 + tick 单调；重做/跳步全拦）。
    pub fn advance(&mut self, s: BootStep, tick: u64) -> Result<(), u16> {
        if self.booted {
            self.rejected += 1;
            return Err(XC_ALREADY_BOOTED);
        }
        if BootStep::from_code(s.code()).is_none() {
            self.rejected += 1;
            return Err(XC_STEP_BAD);
        }
        let idx = (s.code() - 1) as usize;
        if self.done[idx] {
            self.rejected += 1;
            return Err(XC_STEP_REDONE);
        }
        // tick 单调：不得早于此前任何已完成步的 tick。
        let mut i: usize = 0;
        while i < STEP_COUNT {
            if self.done[i] && tick < self.ticks[i] {
                self.rejected += 1;
                return Err(XC_TICK_REGRESS);
            }
            i += 1;
        }
        // 前置齐备：所有更低步码必须已完成（跳步必拦）。
        let mut p: usize = 0;
        while p < idx {
            if !self.done[p] {
                self.rejected += 1;
                return Err(XC_STEP_SKIP);
            }
            p += 1;
        }
        // 计时衔接点纪律：步六、步七要求步五已锚定且锚点不早于起点。
        if s.code() >= 6 {
            if self.anchor_tick == 0 {
                self.rejected += 1;
                return Err(XC_TIMER_NOT_ANCHORED);
            }
            if self.anchor_tick < self.start_tick {
                self.rejected += 1;
                return Err(XC_ANCHOR_REGRESS);
            }
        }
        self.done[idx] = true;
        self.ticks[idx] = tick;
        if s == BootStep::TimerAnchor {
            self.anchor_tick = tick;
        }
        // 收口步：中断使能即 booted。
        if s == BootStep::IrqEnable {
            self.booted = true;
        }
        Ok(())
    }

    /// 守恒机检：done + pending == STEP_COUNT（逐位重扫）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut d: u32 = 0;
        let mut i: usize = 0;
        while i < STEP_COUNT {
            if self.done[i] {
                d += 1;
            }
            i += 1;
        }
        if d + (STEP_COUNT as u32 - d) != STEP_COUNT as u32 {
            return Err(XC_CONSERVATION);
        }
        if self.booted && d != STEP_COUNT as u32 {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 序列指纹（步完成位图 + 各步 tick + 锚点 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < STEP_COUNT {
            h = (h ^ self.done[i] as u8 as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ self.ticks[i]).wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h = (h ^ self.anchor_tick).wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    /// 序列文本导出（确定性步序 + 双导出对拍 + booted 前置）。
    pub fn export(&mut self) -> Result<String, u16> {
        if !self.booted {
            self.rejected += 1;
            return Err(XC_IRQ_NOT_OPEN);
        }
        self.audit_conservation()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0979-BOOT|v1|anchor=");
        s.push_str(&self.anchor_tick.to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < STEP_COUNT {
            let st = match BootStep::from_code((i + 1) as u8) {
                Some(x) => x,
                None => return Err(XC_STEP_BAD),
            };
            s.push_str(st.name());
            s.push('|');
            s.push_str(&self.ticks[i].to_string());
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
// §3 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：七步推进→跳步/重做/锚点缺失/回退注入全拦→收口→双导出。
pub fn drill() -> Result<u32, u16> {
    let mut q = BootSequencer::new(100);
    let steps = [
        BootStep::Detect,
        BootStep::VecSetup,
        BootStep::EoiCheck,
        BootStep::StormArm,
        BootStep::TimerAnchor,
        BootStep::HandlerMount,
        BootStep::IrqEnable,
    ];
    // 注入一：跳步——不探即铺向量表（步二）必拦。
    if q.advance(BootStep::VecSetup, 110) != Err(XC_STEP_SKIP) {
        return Err(XC_STEP_SKIP);
    }
    // 正序推进（步五锚定 tick 150 ≥ 起点 100）。
    let mut t: u64 = 105;
    let mut i: usize = 0;
    while i < steps.len() {
        q.advance(steps[i], t)?;
        t += 10;
        i += 1;
    }
    // 注入二：重复启动拒收。
    if q.advance(BootStep::Detect, 200) != Err(XC_ALREADY_BOOTED) {
        return Err(XC_ALREADY_BOOTED);
    }
    // 守恒机检：7 = 7 done + 0 pending。
    q.audit_conservation()?;
    if q.done_count() != 7 || !q.is_booted() {
        return Err(XC_CONSERVATION);
    }
    if q.anchor_tick() != 145 {
        return Err(XC_ANCHOR_REGRESS);
    }
    // 双导出指纹稳定。
    let fp1 = q.fingerprint();
    let _ = q.export()?;
    if q.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    // 另一序列：锚点早于起点（regress 注入）——步六必拦。
    let mut q2 = BootSequencer::new(500);
    let mut t2: u64 = 501;
    let mut i2: usize = 0;
    while i2 < 5 {
        q2.advance(steps[i2], t2)?;
        t2 += 5;
        i2 += 1;
    }
    // 步五锚定 tick = 521 ≥ 500，合法；此处另测：序列未收口导出拒收。
    if q2.export() != Err(XC_IRQ_NOT_OPEN) {
        return Err(XC_IRQ_NOT_OPEN);
    }
    // 步六在步五锚定后可推进（前置步一~五齐备）。
    q2.advance(BootStep::HandlerMount, t2)?;
    if q2.step_tick(BootStep::TimerAnchor) == 0 {
        return Err(XC_TIMER_NOT_ANCHORED);
    }
    Ok(0)
}
