//! ves455_s25engine — VE-F4221 · 契约引擎总架构
//! U02 定位：契约引擎组（DSL/验证器/版本管理/冲突检测/影响分析——契约全生命周期执行引擎）；
//! 架构声明：注册中心为事实源、引擎为执行层——执行不复制状态（单源红线）；五段流水线（解析→验证→版本→检测→分析）；
//! 段间失配→对拍；状态复制检出→重构+立案；解析失败→显性报错定位；
//! 引擎含无障碍判据执行专段（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1600;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnCode {
    Ok = 0,
    StageMismatch = SEG_BASE + 0x01, // 0x1601
    StateCopied = SEG_BASE + 0x02,   // 0x1602 单源红线
    ParseFail = SEG_BASE + 0x03,     // 0x1603
}

/// 五段流水线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage { Parse = 0, Validate = 1, Version = 2, Detect = 3, Analyze = 4 }

impl Stage {
    pub const ALL: [Stage; 5] = [Stage::Parse, Stage::Validate, Stage::Version, Stage::Detect, Stage::Analyze];
}

/// 引擎架构册。
pub struct EngineBook {
    pub stages: Vec<Stage>,
    pub reg_rev: u64,     // 注册中心版本（事实源）
    pub local_state: Vec<(u32, u32)>, // 本地缓存（不得复制状态——只存指针级引用）
    pub a11y_stage: bool, // 无障碍判据执行专段（域本色）
    pub parse_err_at: Option<usize>,
}

impl EngineBook {
    pub fn new() -> Self { EngineBook { stages: Vec::new(), reg_rev: 0, local_state: Vec::new(), a11y_stage: false, parse_err_at: None } }
    /// 顺序走段（失配→对拍码）。
    pub fn advance(&mut self, s: Stage) -> Result<(), EnCode> {
        if s as usize != self.stages.len() { return Err(EnCode::StageMismatch); }
        self.stages.push(s);
        Ok(())
    }
    /// 单源红线核验：本地状态只允许存「引用 id」不允许存值副本（此处以 (id, 0) 代引用）。
    pub fn single_source_check(&self) -> Result<(), EnCode> {
        for (_, v) in &self.local_state {
            if *v != 0 { return Err(EnCode::StateCopied); }
        }
        Ok(())
    }
    pub fn cache_ref(&mut self, id: u32) { if !self.local_state.iter().any(|(i, _)| *i == id) { self.local_state.push((id, 0)); } }
    /// 解析失败→显性定位（段内偏移）。
    pub fn parse(&mut self, valid: bool, at: usize) -> Result<(), EnCode> {
        if !valid { self.parse_err_at = Some(at); return Err(EnCode::ParseFail); }
        self.parse_err_at = None;
        Ok(())
    }
    /// 全链就绪：五段齐+单源+a11y 专段（域本色）。
    pub fn ready(&self) -> bool {
        self.stages.len() == 5 && self.single_source_check().is_ok() && self.a11y_stage
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves455_checks() -> CheckSet {
    let mut set = CheckSet::new("ves455");
    let mut bk = EngineBook::new();

    // C455-01 越段→失配码。
    chk(&mut set, "C455-01", bk.advance(Stage::Validate) == Err(EnCode::StageMismatch), "越段失配");

    // C455-02 顺序走段（先解析成功）。
    bk.parse(true, 0).unwrap();
    for s in Stage::ALL { bk.advance(s).unwrap(); }
    chk(&mut set, "C455-02", bk.stages.len() == 5, "五段贯通");

    // C455-03 单源红线：本地只存引用→过。
    bk.cache_ref(7001);
    bk.cache_ref(7002);
    chk(&mut set, "C455-03", bk.single_source_check().is_ok(), "单源过");

    // C455-04 状态复制检出→红线码+立案路径。
    bk.local_state[0].1 = 42; // 模拟复制了值
    chk(&mut set, "C455-04", bk.single_source_check() == Err(EnCode::StateCopied), "复制检出");

    // C455-05 重构（清副本）→复检过。
    bk.local_state[0].1 = 0;
    chk(&mut set, "C455-05", bk.single_source_check().is_ok(), "重构复检");

    // C455-06 解析失败→显性定位。
    chk(&mut set, "C455-06", bk.parse(false, 37) == Err(EnCode::ParseFail)
        && bk.parse_err_at == Some(37), "解析定位");

    // C455-07 修好→定位清空。
    bk.parse(true, 0).unwrap();
    chk(&mut set, "C455-07", bk.parse_err_at.is_none(), "定位清空");

    // C455-08 全链就绪：a11y 专段缺→未就绪（域本色）。
    chk(&mut set, "C455-08", !bk.ready(), "a11y 专段缺");

    // C455-09 a11y 专段置位→就绪。
    bk.a11y_stage = true;
    chk(&mut set, "C455-09", bk.ready(), "全链就绪");

    // C455-10 五段单源断言。
    chk(&mut set, "C455-10", Stage::ALL.len() == 5
        && Stage::ALL[0] == Stage::Parse && Stage::ALL[4] == Stage::Analyze, "五段单源");

    // C455-11 缓存引用幂等。
    bk.cache_ref(7001);
    chk(&mut set, "C455-11", bk.local_state.len() == 2, "引用幂等");

    // C455-12 错误码段位单源。
    chk(&mut set, "C455-12", EnCode::StateCopied as u16 == SEG_BASE + 2, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves455_smoke() {
        let set = run_ves455_checks();
        assert!(set.failed_count() == 0, "ves455 red: {}", set.red_items_text());
    }
}
