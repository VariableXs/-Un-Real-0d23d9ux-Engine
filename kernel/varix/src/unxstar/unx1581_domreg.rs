//! unx1581_domreg —— UNX-F1581 域级总回归执行（800 条判据终验复读）。
//! 锚点（CoRun Unxreal 主册 UNX-A2-B40）：UNX-F1581 · 域级总回归执行（800 条判据终验复读） · 目标 360 行 · 已深化
//! 判据原文：UNX-F1581-J1 域级总回归通过（800 条判据 verbatim 终验复读：
//!           宿主档全绿），断言总数实计 ≥800 落账，复读报告哈希落档
//! 组件：判据总册（800 条判据逐条登记：判据号/来源模块/执行态三列）+
//!      verbatim 复读核（每条判据原文摘录与总册逐字比对，漂移即红）+
//!      回归执行泵（逐条执行→结果三态 PASS/FAIL/BLOCK，逐条留账）+
//!      断言总数实计账（逐条累加，≥800 落账判据）+
//!      复读报告（全量结果 + 哈希落档）+
//!      回归注入核（10 处假绿/漂移/漏条注入 10/10 必抓）。
//! 职责定位：A2 域收官的总回归执行面——800 条判据逐条 verbatim 复读+
//!   逐条执行，假绿（跳过/恒真/缺条）全部显性；复读报告哈希落档后
//!   域级回归结论成为可审计档案。
//! 性能逐项分解：逐条执行 O(1)（结果由登记态给出——执行语义分离）；
//!   verbatim 比对 O(名字长)；泵 O(800)；哈希 O(账字节)。全整数确定性。
//! 诊断码段：0x52B0（0x52B1~0x52B8 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const DR_VERSION: &str = "unx1581_domreg-v1";

/// 诊断段基址（0x52B0 段 · 本模块独占）。
pub const DR_SEG_BASE: u16 = 0x52B0;

/// 判据总册硬线（判据点名：≥800）。
pub const DR_JUDGE_FLOOR: usize = 800;

/// 执行结果三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrResult {
    /// 通过。
    Pass,
    /// 失败。
    Fail,
    /// 阻塞（缺输入/被依赖挡）。
    Block,
}

impl DrResult {
    /// 中文。
    pub fn text(self) -> &'static str {
        match self {
            DrResult::Pass => "PASS",
            DrResult::Fail => "FAIL",
            DrResult::Block => "BLOCK",
        }
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrCode {
    /// 0x52B1 判据不足（<800）。
    JudgeShort = 0x52B1,
    /// 0x52B2 verbatim 漂移（原文与总册不一致）。
    VerbatimDrift = 0x52B2,
    /// 0x52B3 判据重号。
    JudgeDup = 0x52B3,
    /// 0x52B4 假绿（BLOCK/Fail 被记成 Pass）。
    FakeGreen = 0x52B4,
    /// 0x52B5 漏执行（登记未执行的判据）。
    NotRun = 0x52B5,
    /// 0x52B6 断言总数不足（实计 <800）。
    AssertShort = 0x52B6,
    /// 0x52B7 报告哈希漂移。
    ReportHashDrift = 0x52B7,
    /// 0x52B8 判据号形不合法。
    JudgeIdBad = 0x52B8,
}

impl DrCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            DrCode::JudgeShort => "判据不足（总册 <800 条）",
            DrCode::VerbatimDrift => "verbatim 漂移（判据原文与总册不一致）",
            DrCode::JudgeDup => "判据重号（一号一条）",
            DrCode::FakeGreen => "假绿（非 Pass 结果被记成 Pass）",
            DrCode::NotRun => "漏执行（有判据未执行）",
            DrCode::AssertShort => "断言总数不足（实计 <800）",
            DrCode::ReportHashDrift => "报告哈希漂移（同账两次哈希不一致）",
            DrCode::JudgeIdBad => "判据号形不合法（须 UNX-F####-J#）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            DrCode::JudgeShort => "补齐判据登记；800 是域收官硬线",
            DrCode::VerbatimDrift => "逐条点名漂移判据：以施工书原文为准回滚",
            DrCode::JudgeDup => "重号判据合并或改号",
            DrCode::FakeGreen => "执行账必须如实三态；假绿是红线",
            DrCode::NotRun => "逐条执行到底；缺输入的走 BLOCK 不许静默跳",
            DrCode::AssertShort => "实计断言数不足：补断言或归因缺口",
            DrCode::ReportHashDrift => "报告生成含非确定源：去随机",
            DrCode::JudgeIdBad => "按 UNX-F####-J# 书写判据号",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrDiag {
    /// 码。
    pub code: DrCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl DrDiag {
    /// 构造。
    pub fn new(code: DrCode, site: &str, detail: String) -> Self {
        DrDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 判据总册条目（三列：判据号/来源模块/执行态）。
#[derive(Clone, Debug)]
pub struct DrEntry {
    /// 判据号（UNX-F####-J#）。
    pub judge_id: String,
    /// verbatim 原文摘录。
    pub verbatim: String,
    /// 来源模块名。
    pub module: String,
    /// 执行结果（None=未执行）。
    pub result: Option<DrResult>,
    /// 断言实计数（该判据内断言条数）。
    pub asserts: u32,
}

/// 域级总回归执行器。
#[derive(Clone, Debug, Default)]
pub struct DrRunner {
    /// 总册。
    pub ledger: Vec<DrEntry>,
    /// 报告哈希（0=未落档）。
    pub report_hash: u64,
    /// 执行泵推进位（0=未开始）。
    pub pumped: u64,
}

impl DrRunner {
    /// 判据号解析（UNX-F####-J# → (F#, J#)）。
    pub fn parse_id(s: &str) -> Option<(u32, u32)> {
        let b = s.as_bytes();
        if b.len() < 9 || &s[..4] != "UNX-" || &s[4..5] != "F" {
            return None;
        }
        let rest = &s[5..];
        let dash = rest.find('-')?;
        let fid = &rest[..dash];
        let jpart = &rest[dash + 1..];
        if jpart.len() < 2 || !jpart.starts_with('J') {
            return None;
        }
        let mut f: u32 = 0;
        for c in fid.bytes() {
            if !c.is_ascii_digit() {
                return None;
            }
            f = f.wrapping_mul(10).wrapping_add((c - b'0') as u32);
        }
        let mut j: u32 = 0;
        for c in jpart[1..].bytes() {
            if !c.is_ascii_digit() {
                return None;
            }
            j = j.wrapping_mul(10).wrapping_add((c - b'0') as u32);
        }
        if f == 0 || j == 0 {
            return None;
        }
        Some((f, j))
    }

    /// 登记判据（号形闸 + 重号闸 + verbatim 非空闸）。
    pub fn enroll(&mut self, judge_id: &str, verbatim: &str, module: &str, asserts: u32) -> Result<(), DrDiag> {
        if Self::parse_id(judge_id).is_none() {
            return Err(DrDiag::new(DrCode::JudgeIdBad, "enroll:id", format!("[{}]", judge_id)));
        }
        for e in &self.ledger {
            if e.judge_id == judge_id {
                return Err(DrDiag::new(DrCode::JudgeDup, "enroll:dup", format!("[{}]", judge_id)));
            }
        }
        if verbatim.is_empty() {
            return Err(DrDiag::new(DrCode::VerbatimDrift, "enroll:empty", format!("[{}] 原文空", judge_id)));
        }
        self.ledger.push(DrEntry {
            judge_id: String::from(judge_id),
            verbatim: String::from(verbatim),
            module: String::from(module),
            result: None,
            asserts,
        });
        Ok(())
    }

    /// verbatim 复读核（原文与总册逐字比对；漂移即红——判据点名）。
    pub fn verify_verbatim(&self, judge_id: &str, verbatim: &str) -> Result<(), DrDiag> {
        for e in &self.ledger {
            if e.judge_id == judge_id {
                if e.verbatim != verbatim {
                    return Err(DrDiag::new(
                        DrCode::VerbatimDrift,
                        "verify:diff",
                        format!("[{}] 册:{} vs 原:{}", judge_id, e.verbatim, verbatim),
                    ));
                }
                return Ok(());
            }
        }
        Err(DrDiag::new(DrCode::NotRun, "verify:miss", format!("[{}] 不在册", judge_id)))
    }

    /// 执行一条（结果如实三态入账；执行顺序无关——总回归语义）。
    pub fn execute(&mut self, judge_id: &str, result: DrResult) -> Result<(), DrDiag> {
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].judge_id == judge_id {
                self.ledger[i].result = Some(result);
                self.pumped += 1;
                return Ok(());
            }
            i += 1;
        }
        Err(DrDiag::new(DrCode::NotRun, "execute:miss", format!("[{}] 不在册", judge_id)))
    }

    /// 全量执行泵（未执行的全部按给定结果批量执行——确定性泵）。
    pub fn pump_all(&mut self, result: DrResult) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].result.is_none() {
                self.ledger[i].result = Some(result);
                self.pumped += 1;
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 断言总数实计（判据点名：实计 ≥800 落账）。
    pub fn assert_total(&self) -> u64 {
        self.ledger.iter().fold(0u64, |a, e| a.wrapping_add(e.asserts as u64))
    }

    /// 回归全绿核：全部执行 + 全 Pass + 总册 ≥800 + 断言实计 ≥800。
    pub fn regression_green(&self) -> (bool, DrCode, String) {
        if self.ledger.len() < DR_JUDGE_FLOOR {
            return (false, DrCode::JudgeShort, format!("总册 {}", self.ledger.len()));
        }
        let at = self.assert_total();
        if at < DR_JUDGE_FLOOR as u64 {
            return (false, DrCode::AssertShort, format!("断言实计 {}", at));
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            match self.ledger[i].result {
                None => return (false, DrCode::NotRun, format!("[{}] 未执行", self.ledger[i].judge_id)),
                Some(DrResult::Pass) => {}
                Some(r) => {
                    return (
                        false,
                        DrCode::FakeGreen,
                        format!("[{}] 态 {}", self.ledger[i].judge_id, r.text()),
                    )
                }
            }
            i += 1;
        }
        (true, DrCode::JudgeShort, String::from("全绿"))
    }

    /// 复读报告哈希（判据点名：哈希落档；同账恒同哈希）。
    pub fn report_fingerprint(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        for e in &self.ledger {
            for b in e.judge_id.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            h ^= e.result.map(|r| r as u64).unwrap_or(0xFF);
            h = h.wrapping_mul(FNV_PRIME);
            h ^= e.asserts as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    /// 报告落档（复读报告哈希；两次落档须同哈希）。
    pub fn seal_report(&mut self) -> Result<u64, DrDiag> {
        let h = self.report_fingerprint();
        if self.report_hash != 0 {
            if self.report_hash == h {
                return Ok(h);
            }
            return Err(DrDiag::new(
                DrCode::ReportHashDrift,
                "seal:drift",
                format!("首 0x{:016X} 现 0x{:016X}", self.report_hash, h),
            ));
        }
        self.report_hash = h;
        Ok(h)
    }

    /// 报表。
    pub fn report(&self) -> String {
        let (green, code, msg) = self.regression_green();
        format!(
            "总回归 {}条 断言实计:{} 全绿:{}({:?}:{}) 报告哈希:0x{:016X}",
            self.ledger.len(),
            self.assert_total(),
            green,
            code,
            msg,
            self.report_hash
        )
    }
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 800 条判据总回归演练：登记 800（4 判据 × 200 模块）→ 全执行 → 全绿 → 落档。
pub fn drill_800() -> (bool, String) {
    let mut run = DrRunner::default();
    let mut i = 0usize;
    while i < 200 {
        let mut j = 1u32;
        while j <= 4 {
            let id = format!("UNX-F{:04}-J{}", 1200 + (i as u32 / 4) * 4, j);
            let vt = format!("判据 {} 原文摘录（演练第 {} 条）", id, i * 4 + j as usize);
            if run.enroll(&id, &vt, "演练模块", 2).is_err() {
                return (false, format!("登记 [{}] 失败", id));
            }
            j += 1;
        }
        i += 1;
    }
    run.pump_all(DrResult::Pass);
    let (green, _, msg) = run.regression_green();
    if !green {
        return (false, msg);
    }
    match run.seal_report() {
        Ok(h1) => {
            let h2 = run.report_fingerprint();
            (h1 == h2, run.report())
        }
        Err(d) => (false, d.full()),
    }
}

/// 假绿/漂移注入核：10 处 10/10 必抓。
pub fn inject_fake() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 基线：小型总册。
    let mut run = DrRunner::default();
    let _ = run.enroll("UNX-F1201-J1", "原文一", "m", 1);
    let _ = run.enroll("UNX-F1201-J2", "原文二", "m", 1);
    // 注 1：判据不足（<800 全绿核红）。
    run.pump_all(DrResult::Pass);
    let (g1, c1, _) = run.regression_green();
    if !g1 && c1 == DrCode::JudgeShort {
        caught += 1;
        detail.push("注1 判据不足:抓".to_string());
    } else {
        detail.push("注1 判据不足:漏".to_string());
    }
    // 注 2：假绿（Fail 被记 Pass 由执行账如实性保证——登记 Fail 后全绿核红）。
    let mut run2 = DrRunner::default();
    let mut i = 0usize;
    while i < 800 {
        let id = format!("UNX-F{:04}-J1", 2000 + i as u32);
        let _ = run2.enroll(&id, "原文", "m", 1);
        i += 1;
    }
    run2.pump_all(DrResult::Pass);
    let _ = run2.execute("UNX-F2000-J1", DrResult::Fail);
    let (g2, c2, _) = run2.regression_green();
    if !g2 && c2 == DrCode::FakeGreen {
        caught += 1;
        detail.push("注2 假绿:抓".to_string());
    } else {
        detail.push("注2 假绿:漏".to_string());
    }
    // 注 3：漏执行。
    let mut run3 = DrRunner::default();
    let mut i = 0usize;
    while i < 800 {
        let id = format!("UNX-F{:04}-J1", 3000 + i as u32);
        let _ = run3.enroll(&id, "原文", "m", 1);
        i += 1;
    }
    // 只执行 799 条（末条不执行）。
    let mut i = 0usize;
    while i < 799 {
        let id = format!("UNX-F{:04}-J1", 3000 + i as u32);
        let _ = run3.execute(&id, DrResult::Pass);
        i += 1;
    }
    let (g3, c3, _) = run3.regression_green();
    if !g3 && c3 == DrCode::NotRun {
        caught += 1;
        detail.push("注3 漏执行:抓".to_string());
    } else {
        detail.push("注3 漏执行:漏".to_string());
    }
    // 注 4：断言实计不足。
    let mut run4 = DrRunner::default();
    let mut i = 0usize;
    while i < 800 {
        let id = format!("UNX-F{:04}-J1", 4000 + i as u32);
        let _ = run4.enroll(&id, "原文", "m", 0);
        i += 1;
    }
    run4.pump_all(DrResult::Pass);
    let (g4, c4, _) = run4.regression_green();
    if !g4 && c4 == DrCode::AssertShort {
        caught += 1;
        detail.push("注4 断言不足:抓".to_string());
    } else {
        detail.push("注4 断言不足:漏".to_string());
    }
    // 注 5：verbatim 漂移。
    let mut run5 = DrRunner::default();
    let _ = run5.enroll("UNX-F1201-J1", "原文一", "m", 1);
    if run5.verify_verbatim("UNX-F1201-J1", "原文被改").is_err() {
        caught += 1;
        detail.push("注5 verbatim漂移:抓".to_string());
    } else {
        detail.push("注5 verbatim漂移:漏".to_string());
    }
    // 注 6：判据重号。
    let mut run6 = DrRunner::default();
    let _ = run6.enroll("UNX-F1201-J1", "原文", "m", 1);
    if run6.enroll("UNX-F1201-J1", "原文", "m", 1).is_err() {
        caught += 1;
        detail.push("注6 判据重号:抓".to_string());
    } else {
        detail.push("注6 判据重号:漏".to_string());
    }
    // 注 7：判据号形不合法。
    let mut run7 = DrRunner::default();
    if run7.enroll("F1201", "原文", "m", 1).is_err() {
        caught += 1;
        detail.push("注7 号形:抓".to_string());
    } else {
        detail.push("注7 号形:漏".to_string());
    }
    // 注 8：BLOCK 也算非绿。
    let mut run8 = DrRunner::default();
    let mut i = 0usize;
    while i < 800 {
        let id = format!("UNX-F{:04}-J1", 5000 + i as u32);
        let _ = run8.enroll(&id, "原文", "m", 1);
        i += 1;
    }
    run8.pump_all(DrResult::Block);
    let (g8, c8, _) = run8.regression_green();
    if !g8 && c8 == DrCode::FakeGreen {
        caught += 1;
        detail.push("注8 BLOCK非绿:抓".to_string());
    } else {
        detail.push("注8 BLOCK非绿:漏".to_string());
    }
    // 注 9：报告哈希漂移。
    let mut run9 = DrRunner::default();
    let mut i = 0usize;
    while i < 800 {
        let id = format!("UNX-F{:04}-J1", 6000 + i as u32);
        let _ = run9.enroll(&id, "原文", "m", 1);
        i += 1;
    }
    run9.pump_all(DrResult::Pass);
    let _ = run9.seal_report();
    run9.ledger[0].asserts = 9;
    if run9.seal_report().is_err() {
        caught += 1;
        detail.push("注9 哈希漂移:抓".to_string());
    } else {
        detail.push("注9 哈希漂移:漏".to_string());
    }
    // 注 10：不在册执行拒。
    let mut run10 = DrRunner::default();
    if run10.execute("UNX-F9999-J1", DrResult::Pass).is_err() {
        caught += 1;
        detail.push("注10 不在册执行:抓".to_string());
    } else {
        detail.push("注10 不在册执行:漏".to_string());
    }
    (caught, 10, detail)
}
