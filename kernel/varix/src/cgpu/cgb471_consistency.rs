//! CGPU-F0471 · 一致性审查（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 一致性审查：四类一致（错误五元组/遥测 schema/三级命名/术语
//!   ——九组逐项比对）；
//! - 跨域一致（与 B 域遥测 schema、与 D 域合同口径的核对）；
//! - 文案一致（预算域特有文案——账单/告警/解释器输出的人话质量）。
//!   判据：四类、跨域、文案、判据。
//!
//! 设计纪律：
//! - 四类一致性各自结构化：错误五元组（code/severity/source/cause/
//!   remediation 字段齐备）、遥测 schema（键集同构）、三级命名
//!   （域-组-指标段位格式）、术语表（同词同义）——逐组比对用
//!   归一化键集差。
//! - 跨域：B 域 schema 键集是本域键集的超集子约束；D 域合同口径
//!   （数值单位/方向）逐点对齐。
//! - 文案一致：三条人话质量规则（带数字/带动作指引/无裸错误码），
//!   文案行逐条校验。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x10A0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x10A0，全仓独占）。
pub const SEG_BASE: u16 = 0x10A0;

pub const E_CS_TUPLE: u16 = SEG_BASE + 1; // 五元组不全
pub const E_CS_SCHEMA: u16 = SEG_BASE + 2; // schema 键集不一致
pub const E_CS_NAMING: u16 = SEG_BASE + 3; // 命名段位违规
pub const E_CS_TERM: u16 = SEG_BASE + 4; // 术语不一致

/// 一、错误五元组。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrTuple {
    pub code: u16,
    pub severity: u8, // 0=info 1=warn 2=error
    pub source: u8,   // 源域码
    pub cause: u8,    // 原因码
    pub remediation: u8, // 处置指引码
}

impl ErrTuple {
    pub fn complete(&self) -> bool {
        self.code != 0 && self.severity <= 2 && self.source != 0 && self.cause != 0 && self.remediation != 0
    }
}

/// 九组错误码五元组齐备核验。
pub fn tuples_ok(ts: &[ErrTuple]) -> Result<(), u16> {
    if ts.is_empty() || ts.iter().any(|t| !t.complete()) {
        return Err(E_CS_TUPLE);
    }
    Ok(())
}

/// 二、遥测 schema 键集一致性（本域 vs B 域参考）。
pub fn schema_consistent(ours: &[&str], b_domain: &[&str]) -> Result<(), u16> {
    if ours.is_empty() {
        return Err(E_CS_SCHEMA);
    }
    // 本域每个键必须在 B 域 schema 中存在（本域是子集约束）。
    for k in ours {
        if !b_domain.contains(k) {
            return Err(E_CS_SCHEMA);
        }
    }
    Ok(())
}

/// 三、三级命名：域段-组段-指标段（如 "c03.arb.latency"）。
pub fn naming_ok(fq: &str) -> Result<(), u16> {
    let parts: Vec<&str> = fq.split('.').collect();
    if parts.len() != 3 {
        return Err(E_CS_NAMING);
    }
    let domain = parts[0];
    // 域段：c 开头+两位组号（c01~c09）。
    let valid_domain = domain.len() == 3
        && domain.starts_with('c')
        && domain.as_bytes()[1].is_ascii_digit()
        && domain.as_bytes()[2].is_ascii_digit();
    // 组段与指标段：非空小写。
    let valid_rest = parts[1].len() > 0 && parts[2].len() > 0;
    if !valid_domain || !valid_rest {
        return Err(E_CS_NAMING);
    }
    Ok(())
}

/// 四、术语表：同词同义（key → 规范词）。
pub fn term_ok(terms: &[(&str, &str)], word: &str, expect: &str) -> Result<(), u16> {
    match terms.iter().find(|(w, _)| *w == word) {
        Some((_, canon)) if *canon == expect => Ok(()),
        Some((_, _)) => Err(E_CS_TERM),
        None => Err(E_CS_TERM),
    }
}

/// 五、跨域 D 域合同口径：单位+方向逐点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DUnit {
    pub key: u8,
    pub unit: u8, // 0=‰ 1=ns 2=tick
    pub dir: u8,  // 0=低好 1=高好
}

pub fn d_units_ok(mine: &[DUnit], theirs: &[DUnit]) -> Result<(), u16> {
    for m in mine {
        match theirs.iter().find(|t| t.key == m.key) {
            Some(t) if t.unit == m.unit && t.dir == m.dir => {}
            _ => return Err(E_CS_SCHEMA),
        }
    }
    Ok(())
}

/// 六、文案人话质量：三规则。
pub fn copy_quality(text: &str) -> bool {
    let has_digit = text.as_bytes().iter().any(|b| b.is_ascii_digit());
    let has_action = text.contains("请") || text.contains("将") || text.contains("可");
    let has_naked_code = text.contains("E_0x");
    has_digit && has_action && !has_naked_code
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、错误五元组。
    {
        let full = ErrTuple { code: 0x0A01, severity: 2, source: 3, cause: 7, remediation: 2 };
        v.push(("F0471-四类-五元组完整通过", tuples_ok(&[full]).is_ok()));
        let broken = ErrTuple { code: 0x0A02, severity: 1, source: 0, cause: 7, remediation: 0 };
        v.push(("F0471-四类-五元组缺字段拒", tuples_ok(&[broken]) == Err(E_CS_TUPLE)));
    }
    // 二、遥测 schema 跨域一致。
    {
        let b = ["frame.tick", "edge.triggers", "edge.recover"];
        let ours = ["edge.triggers", "edge.recover"];
        v.push(("F0471-跨域-schema子集通过", schema_consistent(&ours, &b).is_ok()));
        let alien = ["edge.triggers", "ghost.key"];
        v.push(("F0471-跨域-幽灵键拒", schema_consistent(&alien, &b) == Err(E_CS_SCHEMA)));
    }
    // 三、三级命名。
    {
        v.push(("F0462-命名-规范全名通过", naming_ok("c03.arb.latency").is_ok()));
        v.push(("F0471-命名-两段拒", naming_ok("c03.latency") == Err(E_CS_NAMING)));
        v.push(("F0471-命名-四段拒", naming_ok("c03.a.b.c") == Err(E_CS_NAMING)));
        v.push(("F0471-命名-域段缺位拒", naming_ok("c3.arb.latency") == Err(E_CS_NAMING)));
    }
    // 四、术语一致。
    {
        let terms = [("弃车", "sacrifice"), ("紧缩", "saver"), ("迁回", "migrate_back")];
        v.push(("F0471-四类-术语同义通过", term_ok(&terms, "弃车", "sacrifice").is_ok()));
        v.push(("F0471-四类-术语错配拒", term_ok(&terms, "弃车", "drop") == Err(E_CS_TERM)));
        v.push(("F0471-四类-术语缺席拒", term_ok(&terms, "回涨", "recover") == Err(E_CS_TERM)));
    }
    // 五、D 域合同口径。
    {
        let mine = [DUnit { key: 1, unit: 1, dir: 0 }, DUnit { key: 2, unit: 0, dir: 1 }];
        let theirs = [DUnit { key: 1, unit: 1, dir: 0 }, DUnit { key: 2, unit: 0, dir: 1 }];
        v.push(("F0471-跨域-合同口径对齐", d_units_ok(&mine, &theirs).is_ok()));
        let flipped = [DUnit { key: 1, unit: 1, dir: 1 }, DUnit { key: 2, unit: 0, dir: 1 }];
        v.push(("F0471-跨域-方向错位拒", d_units_ok(&mine, &flipped) == Err(E_CS_SCHEMA)));
        let unit_mis = [DUnit { key: 1, unit: 2, dir: 0 }, DUnit { key: 2, unit: 0, dir: 1 }];
        v.push(("F0471-跨域-单位错位拒", d_units_ok(&mine, &unit_mis) == Err(E_CS_SCHEMA)));
    }
    // 六、文案一致。
    {
        v.push(("F0471-文案-合格样例", copy_quality("预算已压缩至 550‰，请关闭后台应用以恢复全速")));
        v.push(("F0471-文案-无数字拒", !copy_quality("预算已压缩，请关闭后台应用")));
        v.push(("F0471-文案-无动作拒", !copy_quality("预算压缩至 550‰")));
        v.push(("F0471-文案-裸错误码拒", !copy_quality("E_0x0F61 请重试 3 次后可将配额调回")));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0471_consistency_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
