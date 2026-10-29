//! ves476_s26static — VE-F4242 · 静态一致性验证
//! 静态扫描验证三线：API 用法一致性+令牌字面量检出（硬编码色值/间距=发现项）+契约引用静态闭合；
//! CI 集成（合入前必过）；硬编码检出→定位+修复单；引用断→补挂+立案；CI 误拦→白名单评审；
//! 含无障碍属性静态检（aria/对比度声明——域本色线）；扫描不读用户数据。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1A00;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StCode {
    Ok = 0,
    Hardcoded = SEG_BASE + 0x01, // 0x1A01
    RefBroken = SEG_BASE + 0x02, // 0x1A2
    CiFalseBlock = SEG_BASE + 0x03, // 0x1A03
}

/// 三线+a11y 属性检（第四线——域本色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StaticLine { ApiUsage = 0, TokenLiteral = 1, RefClosure = 2, A11yAttr = 3 }

impl StaticLine {
    pub const ALL: [StaticLine; 4] = [
        StaticLine::ApiUsage, StaticLine::TokenLiteral, StaticLine::RefClosure, StaticLine::A11yAttr];
}

/// 扫描目标（文件级合成）。
pub struct ScanTarget { pub name: &'static str, pub api_ok: bool, pub literals: u32, pub refs: Vec<u32>, pub a11y_attr: bool }

/// 静态验证册。
pub struct StaticBook {
    pub targets: Vec<ScanTarget>,
    pub known_refs: Vec<u32>,  // 已注册契约引用
    pub whitelist: Vec<&'static str>,
    pub ci_gate: bool,
    pub filed: u32,
    pub fix_orders: Vec<&'static str>,
}

impl StaticBook {
    pub fn new() -> Self {
        StaticBook { targets: Vec::new(), known_refs: Vec::new(), whitelist: Vec::new(), ci_gate: false, filed: 0, fix_orders: Vec::new() }
    }
    pub fn put(&mut self, t: ScanTarget) {
        self.targets.retain(|x| x.name != t.name);
        self.targets.push(t);
    }
    /// 四线扫描：字面量检出/引用闭合/a11y 属性（API 用法线同查）。
    pub fn scan(&mut self) -> Result<(), StCode> {
        for t in &self.targets {
            if t.literals > 0 {
                if !self.fix_orders.contains(&t.name) { self.fix_orders.push(t.name); }
                return Err(StCode::Hardcoded);
            }
            for r in &t.refs {
                if !self.known_refs.contains(r) { self.filed += 1; return Err(StCode::RefBroken); }
            }
            if !t.a11y_attr { return Err(StCode::Hardcoded); } // a11y 属性缺=域本色线发现
            if !t.api_ok { return Err(StCode::Hardcoded); }
        }
        Ok(())
    }
    /// CI 闸：扫描绿→放行；误拦→白名单评审通道。
    pub fn ci_gate(&mut self, target: &'static str) -> Result<(), StCode> {
        if self.whitelist.contains(&target) { return Ok(()); }
        if self.ci_gate { Ok(()) } else { Err(StCode::CiFalseBlock) }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves476_checks() -> CheckSet {
    let mut set = CheckSet::new("ves476");
    let mut bk = StaticBook::new();
    bk.known_refs = vec![7001, 7002];
    bk.targets.push(ScanTarget { name: "ui/main.rs", api_ok: true, literals: 0, refs: vec![7001], a11y_attr: true });

    // C476-01 四线全绿扫描。
    chk(&mut set, "C476-01", bk.scan().is_ok(), "四线全绿");

    // C476-02 硬编码字面量检出→定位+修复单。
    bk.targets[0].literals = 3;
    chk(&mut set, "C476-02", bk.scan() == Err(StCode::Hardcoded)
        && bk.fix_orders == vec!["ui/main.rs"], "字面量检出");

    // C476-03 修复（归零令牌化）→复扫过。
    bk.targets[0].literals = 0;
    chk(&mut set, "C476-03", bk.scan().is_ok(), "令牌化复扫");

    // C476-04 引用断→补挂+立案。
    bk.targets[0].refs.push(7999);
    chk(&mut set, "C476-04", bk.scan() == Err(StCode::RefBroken) && bk.filed == 1, "引用断立案");

    // C476-05 补挂（注册契约）→复扫过。
    bk.known_refs.push(7999);
    chk(&mut set, "C476-05", bk.scan().is_ok(), "补挂复扫");

    // C476-06 a11y 属性检：aria/对比度声明缺→发现（域本色线）。
    bk.targets[0].a11y_attr = false;
    chk(&mut set, "C476-06", bk.scan() == Err(StCode::Hardcoded), "a11y 属性缺");

    // C476-07 补声明复扫过。
    bk.targets[0].a11y_attr = true;
    chk(&mut set, "C476-07", bk.scan().is_ok(), "补声明过");

    // C476-08 CI 闸：绿态放行。
    bk.ci_gate = true;
    chk(&mut set, "C476-08", bk.ci_gate("ui/main.rs").is_ok(), "CI 放行");

    // C476-09 CI 误拦→白名单评审通道。
    bk.ci_gate = false;
    bk.whitelist.push("generated/tmp.rs");
    chk(&mut set, "C476-09", bk.ci_gate("generated/tmp.rs").is_ok()
        && bk.ci_gate("ui/main.rs") == Err(StCode::CiFalseBlock), "白名单通道");

    // C476-10 四线名单单源。
    chk(&mut set, "C476-10", StaticLine::ALL.len() == 4
        && StaticLine::ALL[3] == StaticLine::A11yAttr, "四线单源");

    // C476-11 修复单幂等。
    bk.targets[0].literals = 2;
    let _ = bk.scan();
    let _ = bk.scan();
    chk(&mut set, "C476-11", bk.fix_orders.len() == 1, "修复单幂等");

    // C476-12 扫描不读用户数据断言（目标均为代码路径）。
    chk(&mut set, "C476-12", bk.targets.iter().all(|t| t.name.ends_with(".rs")), "不读用户数据");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves476_smoke() {
        let set = run_ves476_checks();
        assert!(set.failed_count() == 0, "ves476 red: {}", set.red_items_text());
    }
}
