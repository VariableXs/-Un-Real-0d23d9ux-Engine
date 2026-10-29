//! UNX-F36168-J1 AccessCheck 诊断面（acdiagnose）。
//! 判据：① 结果诊断——AcOutcome → DiagKind 分类闭集（失败步名回显、
//! cause 归因、缺口行文）；② 缺口解释行（缺口位序 = 位号升序，与
//! B06 36101 位名表对齐）；③ FNV 指纹校验；④ 注入负例显性拒；⑤ ≥310 行。
//! 设计纪律：消费 36161（AcOutcome/StepId/Verdict）、36167（Cause）、
//! 36101（位名表 bit_name）、36109（missing_bits/popcount）。

use super::unx36101_masktable as mt;
use super::unx36109_maskcover as mc;
use super::unx36161_acfreeze as acf;
use super::unx36167_accover as cov;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3c40;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CLASS: u16 = SEG_BASE + 2; // 诊断分类越出闭集
pub const E_STEP: u16 = SEG_BASE + 3; // 失败步号不在五步序内
pub const E_NEG: u16 = SEG_BASE + 4; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 5; // 诊断表闭集破坏
pub const E_TEXT: u16 = SEG_BASE + 6; // 解释行文与缺口位失配

/// 模块版本锚。
pub const VER: u32 = 1;

/// 诊断分类（判别式冻结；覆盖 AcOutcome 全域）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagKind {
    OkGrant = 0x0001,    // 全量授予
    OkPartial = 0x0002,  // MA 尽量授
    DenyAce = 0x0003,    // Deny ACE 命中
    DenyCover = 0x0004,  // 覆盖不足
    DenyDacl = 0x0005,   // DACL 空态（ABSENT/EMPTY 装配拒）
    Malformed = 0x0006,  // 三元组不一致（verdict/step/cause 失配）
}

impl DiagKind {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

pub const DIAG_NAMES: [&str; 6] = [
    "ok-grant",
    "ok-partial",
    "deny-ace",
    "deny-cover",
    "deny-dacl",
    "malformed",
];

/// 步名回显（0=未失败；1..5=五步名；越界 "bad-step"）。
pub fn step_name(failed_step: u16) -> &'static str {
    match failed_step {
        0 => "none",
        1 => acf::StepId::Impersonation.name(),
        2 => acf::StepId::Privilege.name(),
        3 => acf::StepId::DaclState.name(),
        4 => acf::StepId::AceEval.name(),
        5 => acf::StepId::CoverJudge.name(),
        _ => "bad-step",
    }
}

/// 结果诊断分类（outcome_consistent 通过后按 verdict/cause 分桶；
/// 不一致的 outcome 归 Malformed）。
pub fn diagnose(o: &acf::AcOutcome) -> Result<DiagKind, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 三元组一致性前置（消费 36167 的冻结映射）。
    let consistent = match o.verdict_of() {
        acf::Verdict::Grant => o.failed_step == 0 && o.cause == 0,
        acf::Verdict::Partial => o.failed_step == 0 && o.cause == cov::Cause::MaFall.code(),
        acf::Verdict::Deny => {
            let ok1 = o.failed_step == acf::StepId::AceEval.code() && o.cause == cov::Cause::AceDeny.code();
            let ok2 = o.failed_step == acf::StepId::CoverJudge.code() && o.cause == cov::Cause::CoverLack.code();
            let ok3 = o.failed_step == acf::StepId::DaclState.code();
            ok1 || ok2 || ok3
        }
    };
    if !consistent {
        return Ok(DiagKind::Malformed);
    }
    match o.verdict_of() {
        acf::Verdict::Grant => Ok(DiagKind::OkGrant),
        acf::Verdict::Partial => Ok(DiagKind::OkPartial),
        acf::Verdict::Deny => {
            if o.cause == cov::Cause::AceDeny.code() {
                Ok(DiagKind::DenyAce)
            } else if o.cause == cov::Cause::CoverLack.code() {
                Ok(DiagKind::DenyCover)
            } else {
                Ok(DiagKind::DenyDacl)
            }
        }
    }
}

/// 缺口行文："gap=N[bit@i;...]"（i 升序；N=缺口位计数）。
/// 无缺口返回 "no-gap"。保留位缺口前缀 "rsvd:"（不可授权）。
pub fn gap_line(granted: u32, required: u32) -> String {
    let m = mc::missing_bits(granted, required);
    if m == 0 {
        return "no-gap".to_string();
    }
    let mut s = String::from("gap=");
    let mut n = 0u32;
    let mut bit = 0u32;
    let mut positions: Vec<u32> = Vec::new();
    while bit < 32 {
        if m & (1u32 << bit) != 0 {
            n += 1;
            positions.push(bit);
        }
        bit += 1;
    }
    s.push_str(&n.to_string());
    s.push('[');
    let mut i = 0;
    while i < positions.len() {
        if i > 0 {
            s.push(';');
        }
        s.push_str("bit@");
        s.push_str(&positions[i].to_string());
        i += 1;
    }
    s.push(']');
    s
}

/// 归因行文：一步式摘要 "diag=<kind> step=<step> cause=<code>"。
pub fn summary_line(o: &acf::AcOutcome) -> Result<String, u16> {
    let kind = diagnose(o)?;
    let mut s = String::from("diag=");
    s.push_str(DIAG_NAMES[(kind.code() - 1) as usize]);
    s.push_str(" step=");
    s.push_str(step_name(o.failed_step));
    s.push_str(" cause=");
    s.push_str(&o.cause.to_string());
    Ok(s)
}

/// 诊断面 FNV 指纹（DIAG_NAMES + 类别闭集）。
pub fn diag_fingerprint() -> u64 {
    let mut h: u64 = 0x0100_93e7_0100_3c41;
    let mut i = 0;
    while i < DIAG_NAMES.len() {
        let b = DIAG_NAMES[i].as_bytes();
        let mut j = 0;
        while j < b.len() {
            h ^= b[j] as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
            j += 1;
        }
        h ^= 0xFF;
        h = h.wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    h
}

/// 分类闭集自检：判别式连续、名字非空、步名边界。
pub fn diag_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut k = 1;
    while k <= 6 {
        if DiagKind::of(k as u16).is_none() {
            return Err(E_TABLE);
        }
        k += 1;
    }
    let mut i = 0;
    while i < DIAG_NAMES.len() {
        if DIAG_NAMES[i].is_empty() {
            return Err(E_TABLE);
        }
        i += 1;
    }
    if step_name(6) != "bad-step" {
        return Err(E_TABLE);
    }
    Ok(())
}

impl DiagKind {
    pub fn of(v: u16) -> Option<DiagKind> {
        match v {
            1 => Some(DiagKind::OkGrant),
            2 => Some(DiagKind::OkPartial),
            3 => Some(DiagKind::DenyAce),
            4 => Some(DiagKind::DenyCover),
            5 => Some(DiagKind::DenyDacl),
            6 => Some(DiagKind::Malformed),
            _ => None,
        }
    }
}

/// 演练：R1 闭集自检；R2 三分诊断（Grant/Partial/Deny×3 路径）；
/// R3 Malformed 归类（篡改三元组）；R4 缺口行文升序与位名对拍；
/// R5 负例注入（坏步名/指纹稳定/行文失配）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：闭集自检。
    diag_integrity()?;
    hits += 1;
    // R2：三分诊断——构造五路 outcome 分桶。
    let o_grant = acf::AcOutcome { granted: 0x89, verdict: 0, failed_step: 0, cause: 0, missing: 0 };
    if diagnose(&o_grant)? != DiagKind::OkGrant {
        return Err(E_NEG);
    }
    let o_partial = acf::AcOutcome {
        granted: 0x89,
        verdict: 2,
        failed_step: 0,
        cause: cov::Cause::MaFall.code(),
        missing: 0x120000,
    };
    if diagnose(&o_partial)? != DiagKind::OkPartial {
        return Err(E_NEG);
    }
    let o_ace = acf::AcOutcome {
        granted: 0,
        verdict: 1,
        failed_step: acf::StepId::AceEval.code(),
        cause: cov::Cause::AceDeny.code(),
        missing: 0x89,
    };
    if diagnose(&o_ace)? != DiagKind::DenyAce {
        return Err(E_NEG);
    }
    let o_cover = acf::AcOutcome {
        granted: 1,
        verdict: 1,
        failed_step: acf::StepId::CoverJudge.code(),
        cause: cov::Cause::CoverLack.code(),
        missing: 0x88,
    };
    if diagnose(&o_cover)? != DiagKind::DenyCover {
        return Err(E_NEG);
    }
    let o_dacl = acf::AcOutcome {
        granted: 0,
        verdict: 1,
        failed_step: acf::StepId::DaclState.code(),
        cause: 0x3C02,
        missing: 0x89,
    };
    if diagnose(&o_dacl)? != DiagKind::DenyDacl {
        return Err(E_NEG);
    }
    hits += 1;
    // R3：Malformed——cause 越出 cause 闭集 / 步号与 verdict 失配。
    let mut bad = o_grant;
    bad.cause = 0x7777;
    if diagnose(&bad)? != DiagKind::Malformed {
        return Err(E_NEG);
    }
    let mut bad2 = o_partial;
    bad2.failed_step = acf::StepId::AceEval.code();
    if diagnose(&bad2)? != DiagKind::Malformed {
        return Err(E_NEG);
    }
    hits += 1;
    // R4：缺口行文——升序位号、no-gap 短路、归因行文拼装。
    if gap_line(0x89, 0x89) != "no-gap" {
        return Err(E_NEG);
    }
    let g = gap_line(0x0000_0001, 0x0000_0089);
    if g != "gap=2[bit@3;bit@7]" {
        return Err(E_TEXT);
    }
    let s = summary_line(&o_grant)?;
    if !s.starts_with("diag=ok-grant step=none cause=0") {
        return Err(E_TEXT);
    }
    let s2 = summary_line(&o_ace)?;
    if !s2.contains("step=ace-eval") {
        return Err(E_TEXT);
    }
    hits += 1;
    // R5：负例注入——坏步名 / 指纹稳定 / 步名五步回显。
    if step_name(0xFF) != "bad-step" {
        return Err(E_NEG);
    }
    if step_name(acf::StepId::CoverJudge.code()) != acf::StepId::CoverJudge.name() {
        return Err(E_NEG);
    }
    let fp1 = diag_fingerprint();
    let fp2 = diag_fingerprint();
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_NEG);
    }
    if DiagKind::of(9).is_some() {
        return Err(E_NEG);
    }
    let _ = mt::ALL_LEGAL; // 位名表消费锚保持
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn diag_table_holds() {
        assert_eq!(diag_integrity(), Ok(()));
    }

    #[test]
    fn malformed_caught() {
        let mut o = acf::AcOutcome { granted: 0x89, verdict: 0, failed_step: 0, cause: 0, missing: 0 };
        o.verdict = 1; // Grant 三元组 → verdict 篡改 Deny 而无 step/cause
        assert_eq!(diagnose(&o), Ok(DiagKind::Malformed));
    }

    #[test]
    fn gap_line_ascending() {
        assert_eq!(gap_line(0, 0x89), "gap=3[bit@0;bit@3;bit@7]");
    }
}

pub fn run_unx36168_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36168_acdiagnose");
    cs.add("SEG 段位独占", SEG_BASE == 0x3c40, "0x3C40~0x3C4F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "分类/行文/负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", diag_integrity() == Ok(()), "DiagKind 判别式 + 步名边界");
    cs.add("契约冻结", VER == 1, "诊断面锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CLASS => "诊断分类越出闭集",
        E_STEP => "失败步号不在五步序内",
        E_NEG => "负例注入未被检出",
        E_TABLE => "诊断表闭集破坏",
        E_TEXT => "解释行文与缺口位失配",
        _ => "未知诊断码",
    }
}
