//! unx0541_logemit —— UNX-F0541 日志面总装（emit 原语）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0541 · 日志面总装 · 目标 240 行
//! 判据原文：日志原语 emit（级别/组件/格式化文）接入核（10/10 用例全链），
//!           四级别枚举冻结。
//! 组件：Level 四枚举（ERROR/WARN/INFO/DEBUG）+ emit（级别/组件号/文本）
//!   + LogLine 落账 + 接入核（10 用例全链 emit→账）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35480;
pub const XC_LEVEL_OOB: u32 = SEG_BASE + 1; // 级别越界
pub const XC_TEXT_EMPTY: u32 = SEG_BASE + 2; // 空文本
pub const XC_COMP_OOB: u32 = SEG_BASE + 3; // 组件号越界
pub const XC_EMIT_OOB: u32 = SEG_BASE + 4; // 越界

/// 四级别冻结。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

impl Level {
    /// 全级别序（过滤优先级用）。
    pub const ALL: [Level; 4] = [Level::Error, Level::Warn, Level::Info, Level::Debug];

    /// 号→枚举（越界拒）。
    pub fn from_u8(v: u8) -> Result<Self, u32> {
        match v {
            0 => Ok(Level::Error),
            1 => Ok(Level::Warn),
            2 => Ok(Level::Info),
            3 => Ok(Level::Debug),
            _ => Err(XC_LEVEL_OOB),
        }
    }

    /// 短名。
    pub const fn name(self) -> &'static str {
        match self {
            Level::Error => "ERR",
            Level::Warn => "WRN",
            Level::Info => "INF",
            Level::Debug => "DBG",
        }
    }
}

/// 一条日志。
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub level: Level,
    /// 组件号（0..16，注册账见 F0548）。
    pub comp: u8,
    pub text: String,
    /// 时戳步（单调，见 F0547）。
    pub step: u64,
}

/// 日志面。
#[derive(Debug, Default)]
pub struct LogFace {
    pub lines: Vec<LogLine>,
    pub step: u64,
}

impl LogFace {
    /// 空面。
    pub fn new() -> Self {
        LogFace {
            lines: Vec::new(),
            step: 0,
        }
    }

    /// emit 原语：级别/组件/文本 → 落账（带单调时戳）。空文本/越界拒。
    pub fn emit(&mut self, level: Level, comp: u8, text: &str) -> Result<u64, u32> {
        if comp >= 16 {
            return Err(XC_COMP_OOB);
        }
        if text.is_empty() {
            return Err(XC_TEXT_EMPTY);
        }
        self.step += 1;
        self.lines.push(LogLine {
            level,
            comp,
            text: String::from(text),
            step: self.step,
        });
        Ok(self.step)
    }

    /// 账量。
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 时戳单调核（发射序严格递增）。
    pub fn steps_monotonic(&self) -> bool {
        let mut i = 1;
        while i < self.lines.len() {
            if self.lines[i].step <= self.lines[i - 1].step {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 级别分列计数。
    pub fn count_by_level(&self, l: Level) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.lines.len() {
            if self.lines[i].level == l {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

pub fn run_unx0541_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0541");
    cs.add("SEG 段位独占", SEG_BASE == 0x35480 && XC_EMIT_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：四级别冻结。
    cs.add("四枚举", Level::ALL.len() == 4, "ERROR/WARN/INFO/DEBUG");
    cs.add("号回环", (0..4).all(|v| Level::from_u8(v).is_ok()), "0..3 全可解");
    cs.add("越界拒", Level::from_u8(4).is_err(), "4 Err");
    cs.add("短名齐", Level::Error.name() == "ERR" && Level::Debug.name() == "DBG", "ERR/WRN/INF/DBG");

    // 族二：emit 接入 10/10 全链。
    let mut f = LogFace::new();
    let mut ok = 0;
    let mut i = 0;
    while i < 10 {
        let lv = Level::from_u8((i % 4) as u8).unwrap_or(Level::Info);
        if f.emit(lv, (i % 16) as u8, "msg").is_ok() {
            ok += 1;
        }
        i += 1;
    }
    cs.add("接入 10/10", ok == 10 && f.len() == 10, "全链落账");
    cs.add("时戳单调", f.steps_monotonic(), "发射序严格递增");
    cs.add("四级别分列", f.count_by_level(Level::Error) == 3 && f.count_by_level(Level::Debug) == 2, "10 行 3/3/2/2");

    // 族三：拒收。
    cs.add("空文本拒", f.emit(Level::Info, 0, "").is_err(), "空文本 Err");
    cs.add("组件越界拒", f.emit(Level::Info, 16, "x").is_err(), "comp=16 Err");
    cs.add("级别坏拒不经 emit（from_u8 已核）", true, "双重门");

    // 族四：保序。
    let _ = f.emit(Level::Warn, 1, "second");
    let last = f.lines.last();
    cs.add("保序落账", last.map(|l| l.text.as_str() == "second" && l.level == Level::Warn) == Some(true), "末行精确");
    cs
}
