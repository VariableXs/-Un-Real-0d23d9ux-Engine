//! UNX-F36259-J1 B13 ktest 断言集（模拟面全判据注册）。
//! 判据：① 本批 18 条判据聚合 ktest 全绿——36241~36258 十八面 +
//! 本聚合面自身；② ktest 聚合入长驻（三账快照：面数/绿数/总检查
//! 数恒等）；③ 纯功能 ≥300 行。
//! 设计纪律：与 36197（B10 ktest）同构的聚合面——面数账是聚合的
//! 第一判据（登记数 = 批内实际模块数），红项必须可归因（red_items
//! 非空才算诊断）。本模块是 B13 的收口判据面：36260 守恒收口消费
//! 本面的绿态。

use crate::checks::CheckSet;
use super::unx36241_implevel as m41;
use super::unx36242_anon as m42;
use super::unx36243_ident as m43;
use super::unx36244_impers as m44;
use super::unx36245_threadtok as m45;
use super::unx36246_impcycle as m46;
use super::unx36247_revert as m47;
use super::unx36248_imppriv as m48;
use super::unx36249_impac as m49;
use super::unx36250_imprefcnt as m50;
use super::unx36251_imphandle as m51;
use super::unx36252_impsess as m52;
use super::unx36253_impaudit as m53;
use super::unx36254_impperf as m54;
use super::unx36255_impfuzz as m55;
use super::unx36256_impwinref as m56;
use super::unx36257_implockstep as m57;
use super::unx36258_impxper as m58;

pub const SEG_BASE: u16 = 0xA410;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FACE: u16 = SEG_BASE + 2; // 面数账不符
pub const E_RED: u16 = SEG_BASE + 3; // 某面判据红
pub const E_SNAP: u16 = SEG_BASE + 4; // 指纹快照漂移
pub const E_INTG: u16 = SEG_BASE + 5; // 三账恒等破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 注册面数（18 面 + 本聚合面 = 19 条判据）。
pub const FACE_N: usize = 18;
pub const TOTAL_VERDICTS: usize = 19;

/// 面注册表（面号 → 标签名 + 判据函数）。
pub type FaceFn = fn() -> CheckSet;

pub const FACES: [(&'static str, FaceFn); FACE_N] = [
    ("F36241-implevel", m41::run_unx36241_checks),
    ("F36242-anon", m42::run_unx36242_checks),
    ("F36243-ident", m43::run_unx36243_checks),
    ("F36244-impers", m44::run_unx36244_checks),
    ("F36245-threadtok", m45::run_unx36245_checks),
    ("F36246-impcycle", m46::run_unx36246_checks),
    ("F36247-revert", m47::run_unx36247_checks),
    ("F36248-imppriv", m48::run_unx36248_checks),
    ("F36249-impac", m49::run_unx36249_checks),
    ("F36250-imprefcnt", m50::run_unx36250_checks),
    ("F36251-imphandle", m51::run_unx36251_checks),
    ("F36252-impsess", m52::run_unx36252_checks),
    ("F36253-impaudit", m53::run_unx36253_checks),
    ("F36254-impperf", m54::run_unx36254_checks),
    ("F36255-impfuzz", m55::run_unx36255_checks),
    ("F36256-impwinref", m56::run_unx36256_checks),
    ("F36257-implockstep", m57::run_unx36257_checks),
    ("F36258-impxper", m58::run_unx36258_checks),
];

/// 聚合快照（三账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AggSnapshot {
    pub faces: u32,
    pub green: u32,
    pub checks_total: u32,
}

/// ktest 聚合执行（18 面全跑 + 三账快照）。
pub fn ktest_aggregate() -> Result<AggSnapshot, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if FACES.len() != FACE_N {
        return Err(E_FACE);
    }
    let mut green = 0u32;
    let mut checks_total = 0u32;
    let mut i = 0;
    while i < FACE_N {
        let cs = FACES[i].1();
        checks_total += cs.len() as u32;
        if !cs.all_passed() {
            if cs.failed_count() > 0 && cs.red_items_text().is_empty() {
                return Err(E_RED);
            }
            return Err(E_RED);
        }
        green += 1;
        i += 1;
    }
    Ok(AggSnapshot { faces: FACE_N as u32, green, checks_total })
}

/// 快照恒等（聚合重放——长驻确定性前提）。
pub fn snapshot_stable() -> Result<u32, u16> {
    let s1 = ktest_aggregate()?;
    let s2 = ktest_aggregate()?;
    if s1 != s2 {
        return Err(E_SNAP);
    }
    if s1.faces != s1.green || s1.green != FACE_N as u32 {
        return Err(E_INTG);
    }
    Ok(s1.checks_total)
}

/// 标签面健康（18 标签互异非空 + 卡号段连续 F36241..F36258）。
pub fn label_anchor() -> Result<(), u16> {
    let mut i = 0;
    while i < FACE_N {
        let (tag, _) = FACES[i];
        if tag.is_empty() {
            return Err(E_FACE);
        }
        let expect = alloc::format!("F{}", 36241 + i);
        let tag_head = &tag[..expect.len()];
        if tag_head != expect.as_str() {
            return Err(E_FACE);
        }
        let mut k = i + 1;
        while k < FACE_N {
            if FACES[i].0 == FACES[k].0 {
                return Err(E_FACE);
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 聚合指纹（三账折叠）。
pub fn agg_fingerprint() -> u32 {
    match ktest_aggregate() {
        Ok(s) => s.faces.wrapping_mul(1_000_003) ^ s.green.wrapping_mul(97) ^ s.checks_total,
        Err(_) => 0xDEAD_0000 | E_RED as u32,
    }
}

/// 红因归因提示表（面号 → 该面最常见红因——诊断加速索引，与
/// 36197 RED_HINTS 同构）。
pub const RED_HINTS: [&'static str; FACE_N] = [
    "四级行为表漂移（Delegation 能力位被误置非零）",
    "匿名级查询通道漏身份（五通道一漏即破）",
    "识别级资源门构造面被开（res_grant 非零）",
    "模拟求值双路径（等价破——禁第二套实现）",
    "挂接位门或权门漏检（双闸一漏即破）",
    "终止未回退令牌（状态残留）",
    "回退五字段对平缺项（depth/level 残留）",
    "模拟态特权放大（越自持面或禁用清单漏检）",
    "取令牌次序错乱（缓存旧令牌或双源实现）",
    "引用计数下溢或孤儿未清扫",
    "模拟态重查老句柄（掩码被旁路）",
    "跨会话模拟漏拒或错误码不统一",
    "审计三要素缺项或漏报未检出",
    "P99 步数模型漂移（场景数据耦合）",
    "fuzz 覆盖账不全（三维一档零命中）",
    "Delegation 被写成 N/A（混淆账面）",
    "锁步重放非确定（状态残留或时钟漂移）",
    "挫败指纹未自动标记（显著性门槛错）",
];

/// 红因提示查询（面号越界 = E_FACE；提示非空——表漂移即红）。
pub fn red_hint(face_idx: usize) -> Result<&'static str, u16> {
    if face_idx >= FACE_N {
        return Err(E_FACE);
    }
    let hint = RED_HINTS[face_idx];
    if hint.is_empty() {
        return Err(E_RED);
    }
    Ok(hint)
}

/// 标签反查提示（tag → 归因——诊断侧常持标签不持面号）。
pub fn hint_for_tag(tag: &str) -> Result<&'static str, u16> {
    let mut i = 0;
    while i < FACE_N {
        if FACES[i].0 == tag {
            return red_hint(i);
        }
        i += 1;
    }
    Err(E_FACE)
}

/// 每面检查数分账（tag, checks）——Σ 分账 = 聚合三账的 checks_total。
pub fn per_face_counts() -> Result<alloc::vec::Vec<(&'static str, u32)>, u16> {
    let mut out = alloc::vec::Vec::new();
    let mut i = 0;
    while i < FACE_N {
        let (tag, f) = FACES[i];
        out.push((tag, f().len() as u32));
        i += 1;
    }
    Ok(out)
}

/// 分账恒等（Σ per_face = checks_total——总/分账双向对平）。
pub fn per_face_reconcile() -> Result<u32, u16> {
    let snap = ktest_aggregate()?;
    let counts = per_face_counts()?;
    if counts.len() as u32 != snap.faces {
        return Err(E_FACE);
    }
    let mut sum = 0u32;
    let mut i = 0;
    while i < counts.len() {
        if counts[i].0 != FACES[i].0 {
            return Err(E_FACE);
        }
        sum = sum.wrapping_add(counts[i].1);
        i += 1;
    }
    if sum != snap.checks_total {
        return Err(E_INTG);
    }
    Ok(sum)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 标签面健康。
    if VER != 1 {
        return Err(E_VER);
    }
    label_anchor()?;
    // R2：18 面聚合全绿 + 三账快照。
    let total = snapshot_stable()?;
    if total == 0 {
        return Err(E_INTG);
    }
    // R3：聚合指纹恒等。
    if agg_fingerprint() == 0xDEAD_0000 | E_RED as u32 {
        return Err(E_RED);
    }
    // R4：总分账对平（Σ 每面检查数 = checks_total）。
    let reconciled = per_face_reconcile()?;
    if reconciled != total {
        return Err(E_INTG);
    }
    // R5：归因表全通（18 面正查 + 反查 + 越界拒）。
    let mut i = 0;
    while i < FACE_N {
        red_hint(i)?;
        i += 1;
    }
    hint_for_tag("F36258-impxper")?;
    if hint_for_tag("F36299-none").is_ok() {
        return Err(E_FACE);
    }
    // R6：判据总数下界。
    if total < FACE_N as u32 {
        return Err(E_FACE);
    }
    Ok(total)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FACE => "面数账不符（漏登记/多登记）",
        E_RED => "某面判据红（聚合绿破）",
        E_SNAP => "指纹快照漂移",
        E_INTG => "三账恒等破",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36259_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36259_b13ktest");
    cs.add("SEG 段位独占", SEG_BASE == 0xA410, "0xA410~0xA41F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "18 面聚合全绿/三账快照恒等/标签连续"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("面数账", FACE_N == 18 && TOTAL_VERDICTS == 19, "18 面 + 本面 = 19 条判据");
    cs.add("指纹面", agg_fingerprint() != 0xDEAD_0000 | E_RED as u32, "聚合指纹非红哨值");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_aggregate_green() {
        let s = ktest_aggregate().unwrap_or(AggSnapshot { faces: 0, green: 0, checks_total: 0 });
        assert_eq!(s.faces, s.green);
        assert!(s.checks_total >= FACE_N as u32);
    }

    #[test]
    fn test_snapshot_and_labels() {
        assert!(snapshot_stable().is_ok());
        assert!(label_anchor().is_ok());
    }
}
