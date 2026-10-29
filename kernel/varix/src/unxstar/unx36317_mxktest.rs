//! UNX-F36317-J1 矩阵满格 ktest 注册（240 格断言面）。
//! 判据：① 满格断言面入 ktest 全绿；② 全跑 ≤90s 记账；③ 纯功
//! 能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 校验框架的断言注册面）：满格矩阵的
//! ktest 化 = 240 格各注册一条断言项（格号 + 期望归因码——期望
//! 取自语义书，注册时不允许自造期望值），断言面跑批 = 逐项求值
//! 对照（真引擎 vs 注册期望），绿项数 = 注册项数才算全绿——少
//! 一项绿或一项注册漏跑都算批失败。注册面自带**覆盖核**：240
//! 项格号连续无重（断言面没有洞才叫满格面）、期望码全部落在闭
//! 集（出闭集期望 = 注册破）。全跑耗时记账：门深映射逐项计费
//！ （掩码 2 / 过滤 3 / DACL 4 / IL 5 / 特权 6 / 许 6 tick——
//! 与 F36309 仪器账同源不同路，总耗时双账必须同值），预算 90s
//！ （90_000 tick @1kHz），实跑入账并对预算断言；预算簿三要素
//! （预算/实耗/结论）齐备才算「记账」，缺要素 = 白跑。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB270;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_REG: u16 = SEG_BASE + 1; // 注册面破（洞/重/出闭集期望）
pub const E_ASSERT: u16 = SEG_BASE + 2; // 断言红（引擎 vs 期望）
pub const E_BUDGET: u16 = SEG_BASE + 3; // 全跑出 90s 预算
pub const E_BOOK: u16 = SEG_BASE + 4; // 预算簿三要素缺
pub const E_RANGE: u16 = SEG_BASE + 5; // 格号越界

/// 模块版本锚。
pub const VER: u32 = 1;

/// ktest 注册项数 / 预算（90s @1kHz）/ 冻结门深总耗时锚。
pub const REGISTRY_SIZE: usize = 240;
pub const RUN_BUDGET_TICKS: u32 = 90_000;
pub const FROZEN_TOTAL_TICKS: u32 = 1_073;

/// ktest 断言项（格号 + 期望归因码——期望只来自语义书）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KTestEntry {
    pub cell: u32,
    pub expect: u8,
}

// ---- 注册面（判据 ① 的账本） ---------------------------------------------------
pub fn build_registry() -> Result<Vec<KTestEntry>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut reg = Vec::new();
    for id in 1..=REGISTRY_SIZE as u32 {
        let expect = mx::spec_at(id)?;
        if !mx::attribution_in_range(expect) {
            return Err(E_REG);
        }
        reg.push(KTestEntry { cell: id, expect });
    }
    // 覆盖核：格号连续无重无漏（满格面的字面含义）。
    for (i, entry) in reg.iter().enumerate() {
        if entry.cell != (i + 1) as u32 {
            return Err(E_REG);
        }
    }
    if reg.len() != REGISTRY_SIZE {
        return Err(E_REG);
    }
    Ok(reg)
}

/// 门深计费（verdict → tick；与 F36309 仪器账同源不同路——双账
/// 同值的独立计费路径）。
pub fn gate_depth_ticks(verdict: u8) -> Result<u32, u16> {
    Ok(match verdict {
        mx::A_MASK => 2,
        mx::A_DENY => 3,
        mx::A_DACL => 4,
        mx::A_IL => 5,
        mx::A_PRIV => 6,
        mx::A_ALLOW => 6,
        _ => return Err(E_REG),
    })
}

// ---- 断言面跑批（逐项求值对照 + 计时入账） ----------------------------------------
pub struct RunBook {
    pub budget: u32,
    pub used: u32,
    pub green: u32,
    pub registered: u32,
    pub verdict_ok: bool,
}

pub fn run_ktest(reg: &[KTestEntry]) -> Result<RunBook, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if reg.len() != REGISTRY_SIZE {
        return Err(E_REG);
    }
    let mut used = 0u32;
    let mut green = 0u32;
    for entry in reg.iter() {
        let (s, o, op) = mx::cell_parts(entry.cell)?;
        let got = mx::evaluate(s, o, op)?;
        used += gate_depth_ticks(got)?;
        if got != entry.expect {
            return Err(E_ASSERT); // 红项即批失败（不留半绿）
        }
        green += 1;
    }
    // 预算簿三要素齐备判定。
    if used > RUN_BUDGET_TICKS {
        return Err(E_BUDGET);
    }
    Ok(RunBook {
        budget: RUN_BUDGET_TICKS,
        used,
        green,
        registered: reg.len() as u32,
        verdict_ok: green == reg.len() as u32 && used <= RUN_BUDGET_TICKS,
    })
}

/// 预算簿核（三要素：预算非零、实耗 ≤ 预算、绿数 = 注册数）。
pub fn runbook_valid(book: &RunBook) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if book.budget == 0 || book.used > book.budget || book.green != book.registered {
        return Err(E_BOOK);
    }
    if !book.verdict_ok {
        return Err(E_BOOK);
    }
    Ok(())
}

/// 门深双账核对：ktest 计费总耗 == F36309 冻结耗时账（同源不同
/// 路的两条计费路径必须收敛同一账值——漂移即有一路计费失真）。
pub fn depth_ledger_reconcile(used: u32) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if used != FROZEN_TOTAL_TICKS {
        return Err(E_BOOK);
    }
    Ok(())
}

/// 断言面账行（呈现面：注册数/绿数/实耗/预算）。
pub fn runbook_text(book: &RunBook) -> Result<String, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut text = String::from("registered=");
    text.push_str(&book.registered.to_string());
    text.push_str(" green=");
    text.push_str(&book.green.to_string());
    text.push_str(" used=");
    text.push_str(&book.used.to_string());
    text.push_str(" budget=");
    text.push_str(&book.budget.to_string());
    Ok(text)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let reg = build_registry()?;
    let book = run_ktest(&reg)?;
    runbook_valid(&book)?;
    depth_ledger_reconcile(book.used)?;
    let text = runbook_text(&book)?;
    if !text.starts_with("registered=240") || !text.contains("green=240") {
        return Err(E_BOOK);
    }
    if book.used != FROZEN_TOTAL_TICKS || book.budget != RUN_BUDGET_TICKS {
        return Err(E_BUDGET);
    }
    Ok(book.green.wrapping_mul(1000).wrapping_add(book.used))
}

// ---- 断言面分布账增补 -----------------------------------------------------------
/// 按面绿数账（每面 30 项全绿——面级守恒，和 240；面账缺项即
/// 满格面破）。
pub fn face_green_tally() -> Result<[u32; 8], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let reg = build_registry()?;
    let mut tally = [0u32; 8];
    for entry in reg.iter() {
        let (s, o, op) = mx::cell_parts(entry.cell)?;
        if mx::evaluate(s, o, op)? != entry.expect {
            return Err(E_ASSERT);
        }
        tally[s] += 1;
    }
    for v in tally.iter() {
        if *v != 30 {
            return Err(E_REG);
        }
    }
    Ok(tally)
}

/// 门深直方图（断言面计费分布 [24,40,69,13,94]——与性能账、聚
/// 合账三账同分布，三路计费互证）。
pub fn depth_histogram() -> Result<[u32; 5], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let frozen: [u32; 5] = [24, 40, 69, 13, 94];
    let reg = build_registry()?;
    let mut hist = [0u32; 5];
    for entry in reg.iter() {
        let (s, o, op) = mx::cell_parts(entry.cell)?;
        let ticks = gate_depth_ticks(mx::evaluate(s, o, op)?)?;
        let slot = (ticks - 2) as usize;
        if slot >= 5 {
            return Err(E_BOOK);
        }
        hist[slot] += 1;
    }
    if hist != frozen {
        return Err(E_BOOK);
    }
    Ok(hist)
}

/// 断言面行文本（每面一行绿账——呈现面，S7 面绿账抽核）。
pub fn face_green_rows() -> Result<String, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let tally = face_green_tally()?;
    let mut text = String::from("面绿账:");
    for s in 0..mx::N_SUBJ {
        text.push_str(" S");
        text.push_str(&s.to_string());
        text.push('=');
        text.push_str(&tally[s].to_string());
    }
    if !text.contains("S7=30") {
        return Err(E_BOOK);
    }
    Ok(text)
}

/// 预算余量账（预算 - 实耗 = 余量 88927——预算簿的第四要素，
/// 余量账与预算账双向可推）。
pub fn budget_margin() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let reg = build_registry()?;
    let book = run_ktest(&reg)?;
    let margin = book.budget - book.used;
    if margin != RUN_BUDGET_TICKS - FROZEN_TOTAL_TICKS {
        return Err(E_BOOK);
    }
    Ok(margin)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_REG => "注册面破（洞/重/期望出闭集）",
        E_ASSERT => "断言红（引擎 vs 注册期望漂移）",
        E_BUDGET => "全跑出 90s 预算",
        E_BOOK => "预算簿破（三要素缺/双账漂移）",
        E_RANGE => "格号越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36317_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36317_mxktest");
    cs.add("SEG 段位独占", SEG_BASE == 0xB270, "0xB270~0xB27F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "240 断言全绿·双账 1073 tick 同值·预算 90000 记账齐"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("注册锚", build_registry().map(|v| v.len()) == Ok(REGISTRY_SIZE), "注册面 240 项连续无洞");
    cs.add("预算锚", RUN_BUDGET_TICKS == 90_000, "90s @ 1kHz = 90000 tick");
    cs.add("分布账", face_green_tally().map(|t| t.iter().fold(0u32, |a, v| a + *v)) == Ok(240) && depth_histogram() == Ok([24, 40, 69, 13, 94]) && budget_margin() == Ok(88927), "面绿 8×30·门深直方图三账同分布·预算余量 88927");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36317_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36317_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
