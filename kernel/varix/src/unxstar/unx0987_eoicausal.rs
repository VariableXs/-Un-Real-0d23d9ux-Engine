//! unx0987_eoicausal — UNX-F0987 风暴-EOI 因果闭环收口（F0849 联动验证与根因修复回路）。
//!
//! 任务锚点：UNX-F0987（第二批工位 W7，SEG 段位 0x8160，本单独占
//! 0x8160..=0x817F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**风暴与 EOI 缺失因果逐案闭环（风暴案
//! 必有 EOI 缺失相关性判定，相关案必有根因修复记录，修复后复测必
//! 过），回路四步（发现-归因-修复-复测）逐案齐备，闭环账守恒**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x816 段闭集）；tick 显性
//! 注入零墙钟；闭环账定容守恒（已闭+未闭==总案 恒等式机检）；结尾
//! drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x8160 段，本单独占）
// ---------------------------------------------------------------------------

/// 风暴-EOI 因果闭环段基址（0x8160，本单 0x8160..=0x817F 独占）。
pub const SEG_BASE: u16 = 0x8160;
/// 0x8161 案号非法（零号/重复——闭环案身份不唯一）。
pub const XC_CASE_BAD: u16 = SEG_BASE + 1;
/// 0x8162 无相关强归因（EOI 缺失率低于相关阈却判因果成立）。
pub const XC_NO_CORRELATION: u16 = SEG_BASE + 2;
/// 0x8163 跳步归因（未发现即归因——回路步序破坏）。
pub const XC_STEP_SKIP: u16 = SEG_BASE + 3;
/// 0x8164 修复记录缺失（归因成立案缺修复动作行）。
pub const XC_FIX_MISSING: u16 = SEG_BASE + 4;
/// 0x8165 复测未过（修复后风暴重现——修复无效）。
pub const XC_RETEST_FAIL: u16 = SEG_BASE + 5;
/// 0x8166 闭环账守恒破坏（已闭+未闭 != 总案）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 6;
/// 0x8167 回路 tick 回退（回路时序单调性破坏）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 7;
/// 0x8168 闭环面指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 8;
/// 0x8169 闭环案定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 9;
/// 0x816A 相关性阈值非法（相关阈为零或超万分域）。
pub const XC_CORR_THRESH: u16 = SEG_BASE + 10;

const _: () = assert!(XC_CASE_BAD != XC_NO_CORRELATION);
const _: () = assert!(XC_FIX_MISSING != XC_RETEST_FAIL);
const _: () = assert!(XC_DRIFT != XC_CORR_THRESH);

// ---------------------------------------------------------------------------
// §1 回路四步闭集（发现 → 归因 → 修复 → 复测）
// ---------------------------------------------------------------------------

/// 回路步闭集：发现 / 归因 / 修复 / 复测（步序单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopStep {
    /// 发现：风暴案立案。
    Found,
    /// 归因：EOI 缺失相关性判定。
    Attributed,
    /// 修复：根因修复动作落账。
    Fixed,
    /// 复测：修复后风暴重现判定。
    Retested,
}

impl LoopStep {
    /// 步码（1..=4）。
    pub const fn code(self) -> u8 {
        match self {
            LoopStep::Found => 1,
            LoopStep::Attributed => 2,
            LoopStep::Fixed => 3,
            LoopStep::Retested => 4,
        }
    }
    /// 步名。
    pub const fn name(self) -> &'static str {
        match self {
            LoopStep::Found => "发现",
            LoopStep::Attributed => "归因",
            LoopStep::Fixed => "修复",
            LoopStep::Retested => "复测",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<LoopStep> {
        match c {
            1 => Some(LoopStep::Found),
            2 => Some(LoopStep::Attributed),
            3 => Some(LoopStep::Fixed),
            4 => Some(LoopStep::Retested),
            _ => None,
        }
    }
}

/// EOI 缺失相关阈（万分基点——缺失率 ≥ 3000 判与风暴相关）。
pub const CORR_THRESHOLD_BP: u32 = 3_000;

// ---------------------------------------------------------------------------
// §2 闭环案（CausalCase）
// ---------------------------------------------------------------------------

/// 一宗闭环案：案号 / 四步完成位 / EOI 缺失率 / 修复注记 / 复测结论。
#[derive(Clone, Debug)]
pub struct CausalCase {
    case_id: u32,
    found: bool,
    attributed: bool,
    fixed: bool,
    retested: bool,
    eoi_miss_bp: u32,
    fix_note: String,
    retest_storm_free: bool,
    closed: bool,
}

impl CausalCase {
    /// 步进度（已完成步数 0..=4）。
    pub fn progress(&self) -> u8 {
        let mut n: u8 = 0;
        if self.found {
            n += 1;
        }
        if self.attributed {
            n += 1;
        }
        if self.fixed {
            n += 1;
        }
        if self.retested {
            n += 1;
        }
        n
    }
    /// 是否已闭环。
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

// ---------------------------------------------------------------------------
// §3 因果闭环账（CausalLedger）
// ---------------------------------------------------------------------------

/// 闭环账容量定值：16 案（风暴-EOI 因果案全周期量级）。
pub const CASE_CAP: usize = 16;

/// 因果闭环账——F0849 联动验证与根因修复回路的本体。
///
/// 守恒恒等式：`已闭环数 + 未闭环数 == 总案数`（每案恰处一态）；
/// 步序恒等式：每案四步单调推进（跳步必拦）；相关恒等式：归因成立
/// 案的 EOI 缺失率必 ≥ 相关阈。
#[derive(Debug)]
pub struct CausalLedger {
    cases: Vec<CausalCase>,
    cap: usize,
    closed_cnt: u32,
    last_tick: u64,
    rejected: u32,
}

impl CausalLedger {
    /// 建闭环账。
    pub fn new(cap: usize) -> Result<CausalLedger, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        if CORR_THRESHOLD_BP == 0 || CORR_THRESHOLD_BP > 10_000 {
            return Err(XC_CORR_THRESH);
        }
        Ok(CausalLedger {
            cases: Vec::with_capacity(cap),
            cap,
            closed_cnt: 0,
            last_tick: 0,
            rejected: 0,
        })
    }

    /// 案数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    fn find(&self, case_id: u32) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.cases.len() {
            if self.cases[i].case_id == case_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 立案（发现步——案号/重复/tick 全检）。
    pub fn open_case(&mut self, case_id: u32, eoi_miss_bp: u32, tick: u64) -> Result<(), u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        if case_id == 0 || eoi_miss_bp > 10_000 {
            self.rejected += 1;
            return Err(XC_CASE_BAD);
        }
        if self.find(case_id).is_some() {
            self.rejected += 1;
            return Err(XC_CASE_BAD);
        }
        if self.cases.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        self.cases.push(CausalCase {
            case_id,
            found: true,
            attributed: false,
            fixed: false,
            retested: false,
            eoi_miss_bp,
            fix_note: String::new(),
            retest_storm_free: false,
            closed: false,
        });
        self.last_tick = tick;
        Ok(())
    }

    /// 归因（前置：已发现；相关性判定——缺失率低于阈强归因拦停）。
    pub fn attribute(&mut self, case_id: u32, tick: u64) -> Result<(), u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        let idx = match self.find(case_id) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_CASE_BAD);
            }
        };
        if !self.cases[idx].found {
            self.rejected += 1;
            return Err(XC_STEP_SKIP);
        }
        if self.cases[idx].eoi_miss_bp < CORR_THRESHOLD_BP {
            self.rejected += 1;
            return Err(XC_NO_CORRELATION);
        }
        self.cases[idx].attributed = true;
        self.last_tick = tick;
        Ok(())
    }

    /// 修复（前置：已归因；修复注记非空）。
    pub fn fix(&mut self, case_id: u32, note: &str, tick: u64) -> Result<(), u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        let idx = match self.find(case_id) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_CASE_BAD);
            }
        };
        if !self.cases[idx].attributed {
            self.rejected += 1;
            return Err(XC_STEP_SKIP);
        }
        if note.is_empty() {
            self.rejected += 1;
            return Err(XC_FIX_MISSING);
        }
        self.cases[idx].fix_note = String::from(note);
        self.cases[idx].fixed = true;
        self.last_tick = tick;
        Ok(())
    }

    /// 复测（前置：已修复；风暴重现则修复无效不得闭环）。
    pub fn retest(&mut self, case_id: u32, storm_free: bool, tick: u64) -> Result<(), u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        let idx = match self.find(case_id) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_CASE_BAD);
            }
        };
        let c = &mut self.cases[idx];
        if !c.fixed {
            self.rejected += 1;
            return Err(XC_STEP_SKIP);
        }
        if !storm_free {
            self.rejected += 1;
            return Err(XC_RETEST_FAIL);
        }
        c.retested = true;
        c.retest_storm_free = true;
        c.closed = true;
        self.closed_cnt += 1;
        self.last_tick = tick;
        Ok(())
    }

    /// 守恒机检：已闭 + 未闭 == 总案（逐案重扫）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut c: u32 = 0;
        let mut i: usize = 0;
        while i < self.cases.len() {
            if self.cases[i].closed {
                c += 1;
            }
            i += 1;
        }
        if c != self.closed_cnt {
            return Err(XC_CONSERVATION);
        }
        if c + (self.cases.len() as u32 - c) != self.cases.len() as u32 {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 步序机检：逐案四步单调（found→attributed→fixed→retested）。
    pub fn audit_step_order(&self) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < self.cases.len() {
            let c = &self.cases[i];
            if (c.attributed && !c.found) || (c.fixed && !c.attributed) || (c.retested && !c.fixed)
            {
                return Err(XC_STEP_SKIP);
            }
            // 已闭环案必四步全成。
            if c.closed && c.progress() != 4 {
                return Err(XC_STEP_SKIP);
            }
            i += 1;
        }
        Ok(())
    }

    /// 闭环账指纹（案进度/闭态逐案 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.cases.len() {
            h = (h ^ self.cases[i].case_id as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ self.cases[i].progress() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ self.cases[i].closed as u8 as u64).wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 闭环账文本导出（确定性序 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        self.audit_conservation()?;
        self.audit_step_order()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0987-CAUSAL|v1|cases=");
        s.push_str(&self.cases.len().to_string());
        s.push_str("|closed=");
        s.push_str(&self.closed_cnt.to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < self.cases.len() {
            s.push_str(&self.cases[i].case_id.to_string());
            s.push('|');
            s.push_str(&self.cases[i].progress().to_string());
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

/// 自例程：立案→归因→修复→复测闭环→低相关/跳步/复测失败注入全拦→守恒。
pub fn drill() -> Result<u32, u16> {
    let mut l = CausalLedger::new(CASE_CAP)?;
    // 案一：全闭环（相关性 5000 ≥ 3000）。
    l.open_case(1, 5_000, 10)?;
    l.attribute(1, 20)?;
    l.fix(1, "补发缺失 EOI 并校正 EOI 计数器", 30)?;
    l.retest(1, true, 40)?;
    // 案二：归因拦停（相关性 1000 < 3000——非 EOI 因果案）。
    l.open_case(2, 1_000, 50)?;
    if l.attribute(2, 60) != Err(XC_NO_CORRELATION) {
        return Err(XC_NO_CORRELATION);
    }
    // 案三：修复后复测失败（风暴重现——不得闭环）。
    l.open_case(3, 8_000, 70)?;
    l.attribute(3, 80)?;
    l.fix(3, "调低该向量屏蔽粒度", 90)?;
    if l.retest(3, false, 100) != Err(XC_RETEST_FAIL) {
        return Err(XC_RETEST_FAIL);
    }
    // 注入面：零号案、重复案、跳步归因、空修复注记、tick 回退全拦。
    if l.open_case(0, 5_000, 110) != Err(XC_CASE_BAD) {
        return Err(XC_CASE_BAD);
    }
    if l.open_case(1, 5_000, 110) != Err(XC_CASE_BAD) {
        return Err(XC_CASE_BAD);
    }
    if l.fix(2, "未归因先修复", 110) != Err(XC_STEP_SKIP) {
        return Err(XC_STEP_SKIP);
    }
    if l.attribute(9, 110) != Err(XC_CASE_BAD) {
        return Err(XC_CASE_BAD);
    }
    if l.open_case(4, 5_000, 5) != Err(XC_TICK_REGRESS) {
        return Err(XC_TICK_REGRESS);
    }
    // 守恒机检：3 案 = 1 已闭 + 2 未闭；注入面 7 次拒收全拦。
    l.audit_conservation()?;
    l.audit_step_order()?;
    if l.closed_cnt != 1 || l.len() != 3 || l.rejected() != 7 {
        return Err(XC_CONSERVATION);
    }
    // 双导出指纹稳定。
    let fp1 = l.fingerprint();
    let _ = l.export()?;
    if l.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
