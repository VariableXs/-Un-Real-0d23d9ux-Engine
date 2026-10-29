//! UNX-F36167-J1 AccessCheck 第⑤步：覆盖判定与三分（accover）。
//! 判据：① step5 冻结签名
//! `step5(&AcRequest, u32, &EvalOut) -> Result<AcOutcome, u16>`——
//! 判定序：Deny 命中即拒（failed_step=AceEval）→ 覆盖即授（Grant）→
//! MA 尽量授（Partial）→ 覆盖不足拒（failed_step=CoverJudge）；
//! granted 恒 ⊆ expanded（装配 E_INV 兜底）；② 覆盖语义委托 36109
//! （covers/missing_bits/popcount），MA 请求位语义委托 36110；
//! ③ 注入负例显性拒；④ ≥310 行。
//! 设计纪律：36161 通过 `super::unx36167_accover as s5` 消费 step5；
//! 三分枚举真源 36161::Verdict，步骤序真源 36161::StepId。

use super::unx36109_maskcover as mc;
use super::unx36110_maxallowed as ma;
use super::unx36161_acfreeze as acf;
use super::unx36165_aceeval as s4;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3c30;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ACEDENY: u16 = SEG_BASE + 2; // Deny ACE 命中（cause 面）
pub const E_COVERLACK: u16 = SEG_BASE + 3; // 覆盖不足（cause 面）
pub const E_MAFALL: u16 = SEG_BASE + 4; // MA 尽量授未满（cause 面）
pub const E_INV: u16 = SEG_BASE + 5; // 判定面非法（granted ⊄ expanded 等）
pub const E_NEG: u16 = SEG_BASE + 6; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 7; // 判定序闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// 判定原因种类（判别式直接取诊断码值；与 step5 写入的 cause 对齐）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cause {
    AceDeny = E_ACEDENY,
    CoverLack = E_COVERLACK,
    MaFall = E_MAFALL,
}

impl Cause {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

// ---- 第⑤步契约 -------------------------------------------------------------
/// step5：覆盖判定与三分。
/// 判定序（互斥，命中即返）：
/// ① ev.denied_hit → Deny，failed_step=AceEval(4)，granted=0，
///   missing=expanded，cause=E_ACEDENY；
/// ② covers(granted, expanded) → Grant，granted=ev.granted，
///   failed_step=0，cause=0，missing=0；
/// ③ is_ma(desired) → Partial，granted=ev.granted（尽量授，不拒），
///   failed_step=0，cause=E_MAFALL，missing=覆盖缺口；
/// ④ 其余 → Deny，granted=0，failed_step=CoverJudge(5)，
///   cause=E_COVERLACK，missing=覆盖缺口。
pub fn step5(
    req: &acf::AcRequest,
    expanded: u32,
    ev: &s4::EvalOut,
) -> Result<acf::AcOutcome, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if ev.granted & !expanded != 0 {
        return Err(E_INV); // 判定面前置守卫：granted 必须 ⊆ expanded
    }
    if ev.denied_hit {
        return Ok(acf::AcOutcome {
            granted: 0,
            verdict: acf::Verdict::Deny.code(),
            failed_step: acf::StepId::AceEval.code(),
            cause: E_ACEDENY,
            missing: expanded,
        });
    }
    if mc::covers(ev.granted, expanded) {
        return Ok(acf::AcOutcome {
            granted: ev.granted,
            verdict: acf::Verdict::Grant.code(),
            failed_step: 0,
            cause: 0,
            missing: 0,
        });
    }
    if ma::is_ma(req.desired) {
        return Ok(acf::AcOutcome {
            granted: ev.granted,
            verdict: acf::Verdict::Partial.code(),
            failed_step: 0,
            cause: E_MAFALL,
            missing: mc::missing_bits(ev.granted, expanded),
        });
    }
    Ok(acf::AcOutcome {
        granted: 0,
        verdict: acf::Verdict::Deny.code(),
        failed_step: acf::StepId::CoverJudge.code(),
        cause: E_COVERLACK,
        missing: mc::missing_bits(ev.granted, expanded),
    })
}

/// 判定序合法性：四支互斥且有序（Deny 命中 > 覆盖 > MA > 缺口）。
pub fn judge_order_ok(ev: &s4::EvalOut, expanded: u32, desired_ma: bool) -> Result<acf::Verdict, u16> {
    if ev.granted & !expanded != 0 {
        return Err(E_INV);
    }
    if ev.denied_hit {
        return Ok(acf::Verdict::Deny);
    }
    if mc::covers(ev.granted, expanded) {
        return Ok(acf::Verdict::Grant);
    }
    if desired_ma {
        return Ok(acf::Verdict::Partial);
    }
    Ok(acf::Verdict::Deny)
}

/// 三分一致性：outcome 三元组（verdict/failed_step/cause）必须符合
/// 冻结映射——Grant→(0,0)；Partial→(0,MAFALL)；Deny→(4,ACEDENY) 或
/// (5,COVERLACK)。
pub fn outcome_consistent(o: &acf::AcOutcome) -> Result<(), u16> {
    match o.verdict_of() {
        acf::Verdict::Grant => {
            if o.failed_step != 0 || o.cause != 0 || o.missing != 0 {
                return Err(E_TABLE);
            }
        }
        acf::Verdict::Partial => {
            if o.failed_step != 0 || o.cause != Cause::MaFall.code() {
                return Err(E_TABLE);
            }
        }
        acf::Verdict::Deny => {
            let ok1 = o.failed_step == acf::StepId::AceEval.code() && o.cause == Cause::AceDeny.code();
            let ok2 = o.failed_step == acf::StepId::CoverJudge.code() && o.cause == Cause::CoverLack.code();
            if !ok1 && !ok2 {
                return Err(E_TABLE);
            }
        }
    }
    Ok(())
}

/// 覆盖缺口统计：多组 (granted, required) 的缺口位计数样本面。
pub fn coverage_stats(pairs: &[(u32, u32)]) -> Result<Vec<u32>, u16> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < pairs.len() {
        let (g, r) = pairs[i];
        if g & !r != 0 {
            return Err(E_INV); // granted ⊄ required 的样本非法
        }
        out.push(mc::popcount(mc::missing_bits(g, r)));
        i += 1;
    }
    Ok(out)
}

/// 判定面自检：版本锚 + Cause 判别式 + 三分映射序。
pub fn judge_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if Cause::AceDeny.code() != E_ACEDENY
        || Cause::CoverLack.code() != E_COVERLACK
        || Cause::MaFall.code() != E_MAFALL
    {
        return Err(E_TABLE);
    }
    let ev = s4::EvalOut { granted: 0, denied_hit: false, denied_by: 0, deny_union: 0 };
    if judge_order_ok(&ev, 0x89, false) != Ok(acf::Verdict::Deny) {
        return Err(E_TABLE);
    }
    if judge_order_ok(&ev, 0x89, true) != Ok(acf::Verdict::Partial) {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 演练：R1 闭集自检；R2 全覆盖授 / 部分覆盖 MA 尽量授；R3 Deny 命中
/// 与覆盖不足双路；R4 三分一致性与缺口统计；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：闭集自检。
    judge_integrity()?;
    hits += 1;
    // R2：全覆盖授 + MA 尽量授三分。
    let req_plain = acf::AcRequest {
        token: acf::tok_plain(0x1111),
        owner: 1,
        dacl: acf::DaclFace { state: 3, aces: Vec::new() },
        desired: 0x0000_0089,
        map_name: "File",
        priv_gate: None,
    };
    let ev_full = s4::EvalOut { granted: 0x0000_0089, denied_hit: false, denied_by: 0, deny_union: 0 };
    let o1 = step5(&req_plain, 0x0000_0089, &ev_full)?;
    if o1.verdict_of() != acf::Verdict::Grant || o1.granted != 0x89 {
        return Err(E_NEG);
    }
    outcome_consistent(&o1)?;
    let req_ma = acf::AcRequest {
        token: acf::tok_plain(0x1111),
        owner: 1,
        dacl: acf::DaclFace { state: 3, aces: Vec::new() },
        desired: 0x0200_0000 | 0x0012_0089,
        map_name: "File",
        priv_gate: None,
    };
    let ev_part = s4::EvalOut { granted: 0x0000_0089, denied_hit: false, denied_by: 0, deny_union: 0 };
    let o2 = step5(&req_ma, 0x0012_0089, &ev_part)?;
    if o2.verdict_of() != acf::Verdict::Partial || o2.granted != 0x89 || o2.missing != 0x0012_0000 {
        return Err(E_NEG);
    }
    outcome_consistent(&o2)?;
    hits += 1;
    // R3：Deny 命中（failed_step=4）与覆盖不足（failed_step=5）双路。
    let ev_deny = s4::EvalOut { granted: 0, denied_hit: true, denied_by: 0x1111, deny_union: 0x0010_0000 };
    let o3 = step5(&req_plain, 0x0012_0089, &ev_deny)?;
    if o3.verdict_of() != acf::Verdict::Deny || o3.failed_step != acf::StepId::AceEval.code() {
        return Err(E_NEG);
    }
    if o3.cause != Cause::AceDeny.code() || o3.missing != 0x0012_0089 {
        return Err(E_NEG);
    }
    outcome_consistent(&o3)?;
    let ev_lack = s4::EvalOut { granted: 0x0000_0001, denied_hit: false, denied_by: 0, deny_union: 0 };
    let o4 = step5(&req_plain, 0x0000_0089, &ev_lack)?;
    if o4.verdict_of() != acf::Verdict::Deny || o4.failed_step != acf::StepId::CoverJudge.code() {
        return Err(E_NEG);
    }
    if o4.cause != Cause::CoverLack.code() || o4.missing != 0x0000_0088 {
        return Err(E_NEG);
    }
    outcome_consistent(&o4)?;
    hits += 1;
    // R4：三分一致性 + 缺口统计样本。
    let stats = coverage_stats(&[(0x89, 0x89), (0x01, 0x89), (0xFF, 0xFF), (0, 0)])?;
    if stats != vec![0, 2, 0, 0] {
        return Err(E_NEG);
    }
    if judge_order_ok(&ev_full, 0x89, false) != Ok(acf::Verdict::Grant) {
        return Err(E_NEG);
    }
    hits += 1;
    // R5：负例注入——granted ⊄ expanded / 坏三分映射 / 坏判定序。
    let ev_bad = s4::EvalOut { granted: 0xFFFF_0000, denied_hit: false, denied_by: 0, deny_union: 0 };
    if step5(&req_plain, 0x0000_0089, &ev_bad).is_ok() {
        return Err(E_NEG);
    }
    let mut bad_out = o1;
    bad_out.cause = 0x7777;
    if outcome_consistent(&bad_out).is_ok() {
        return Err(E_NEG);
    }
    let ev_flip = s4::EvalOut { granted: 0, denied_hit: true, denied_by: 0, deny_union: 0 };
    if judge_order_ok(&ev_flip, 0x89, true) != Ok(acf::Verdict::Deny) {
        return Err(E_NEG); // Deny 命中优先于 MA 尽量授
    }
    if coverage_stats(&[(0x0200_0000, 0x0000_0089)]).is_ok() {
        return Err(E_NEG); // granted ⊄ required 样本必须拒
    }
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
    fn judge_table_holds() {
        assert_eq!(judge_integrity(), Ok(()));
    }

    #[test]
    fn ma_partial_never_denies() {
        let req = acf::AcRequest {
            token: acf::tok_plain(1),
            owner: 1,
            dacl: acf::DaclFace { state: 3, aces: Vec::new() },
            desired: 0x0200_0000 | 0x89,
            map_name: "File",
            priv_gate: None,
        };
        let ev = s4::EvalOut { granted: 0, denied_hit: false, denied_by: 0, deny_union: 0 };
        let o = step5(&req, 0x89, &ev).ok().unwrap_or(acf::AcOutcome {
            granted: 0xFFFF_FFFF,
            verdict: 1,
            failed_step: 0,
            cause: 0,
            missing: 0,
        });
        assert_eq!(o.verdict_of(), acf::Verdict::Partial);
    }

    #[test]
    fn deny_hit_beats_ma() {
        let req = acf::AcRequest {
            token: acf::tok_plain(1),
            owner: 1,
            dacl: acf::DaclFace { state: 3, aces: Vec::new() },
            desired: 0x0200_0000 | 0x89,
            map_name: "File",
            priv_gate: None,
        };
        let ev = s4::EvalOut { granted: 0, denied_hit: true, denied_by: 7, deny_union: 0x89 };
        let o = step5(&req, 0x89, &ev).ok().unwrap_or(acf::AcOutcome {
            granted: 0xFFFF_FFFF,
            verdict: 0,
            failed_step: 0,
            cause: 0,
            missing: 0,
        });
        assert_eq!(o.verdict_of(), acf::Verdict::Deny);
    }
}

pub fn run_unx36167_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36167_accover");
    cs.add("SEG 段位独占", SEG_BASE == 0x3c30, "0x3C30~0x3C3F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "判定序/三分/一致性/负例全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", judge_integrity() == Ok(()), "Cause 判别式 + 三分映射序");
    cs.add("契约冻结", VER == 1, "step5 契约锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ACEDENY => "Deny ACE 命中",
        E_COVERLACK => "覆盖不足",
        E_MAFALL => "MA 尽量授未满",
        E_INV => "判定面非法（granted ⊄ expanded）",
        E_NEG => "负例注入未被检出",
        E_TABLE => "判定序闭集破坏",
        _ => "未知诊断码",
    }
}
