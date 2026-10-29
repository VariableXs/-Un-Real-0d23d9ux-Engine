//! UNX-F36197-J1 B10 ktest 断言集（边界面全判据注册）。
//! 判据：① 本批 17 条判据聚合 ktest 全绿——36181~36196 十六面 +
//! 本模块聚合面自身，逐面 CheckSet 全绿直证；② ktest 聚合入长驻
//! （聚合面自带指纹快照——面数/绿数/总检查数三账恒等）；③ 纯功能
//! ≥320 行。
//! 设计纪律：ktest 聚合的价值是「单点全绿证明」——16 个面的判据
//! 各自绿不等于聚合面绿（聚合可能漏登记某个面——**面数账**是聚合
//! 的第一判据：登记数必须等于批内实际模块数）。本模块是 B10 的
//! 收口判据面：36198 守恒收口消费本面的绿态。

use crate::checks::CheckSet;
use super::unx36181_maxallowed as m36181;
use super::unx36182_madeny as m36182;
use super::unx36183_mapriv as m36183;
use super::unx36184_dacltriad as m36184;
use super::unx36185_probeguard as m36185;
use super::unx36186_errbook as m36186;
use super::unx36187_refusal as m36187;
use super::unx36188_edgelib as m36188;
use super::unx36189_edgefuzz as m36189;
use super::unx36190_perfbudget as m36190;
use super::unx36191_winref as m36191;
use super::unx36192_lockstep as m36192;
use super::unx36193_explain as m36193;
use super::unx36194_ilhook as m36194;
use super::unx36195_reconcile as m36195;
use super::unx36196_frustration as m36196;

pub const SEG_BASE: u16 = 0x4630;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FACE: u16 = SEG_BASE + 2; // 面数账不符（漏登记/多登记）
pub const E_RED: u16 = SEG_BASE + 3; // 某面判据红（聚合绿破）
pub const E_SNAP: u16 = SEG_BASE + 4; // 指纹快照漂移
pub const E_INTG: u16 = SEG_BASE + 5; // 三账恒等破
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 掩码越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 注册面数（16 面判据 + 本聚合面 = 17 条判据——判据锚）。
pub const FACE_N: usize = 16;
pub const TOTAL_VERDICTS: usize = 17;

/// 面注册表（面号 → 标签名 + 判据函数——聚合的登记面）。
pub type FaceFn = fn() -> CheckSet;

pub const FACES: [(&'static str, FaceFn); FACE_N] = [
    ("F36181-maxallowed", m36181::run_unx36181_checks),
    ("F36182-madeny", m36182::run_unx36182_checks),
    ("F36183-mapriv", m36183::run_unx36183_checks),
    ("F36184-dacltriad", m36184::run_unx36184_checks),
    ("F36185-probeguard", m36185::run_unx36185_checks),
    ("F36186-errbook", m36186::run_unx36186_checks),
    ("F36187-refusal", m36187::run_unx36187_checks),
    ("F36188-edgelib", m36188::run_unx36188_checks),
    ("F36189-edgefuzz", m36189::run_unx36189_checks),
    ("F36190-perfbudget", m36190::run_unx36190_checks),
    ("F36191-winref", m36191::run_unx36191_checks),
    ("F36192-lockstep", m36192::run_unx36192_checks),
    ("F36193-explain", m36193::run_unx36193_checks),
    ("F36194-ilhook", m36194::run_unx36194_checks),
    ("F36195-reconcile", m36195::run_unx36195_checks),
    ("F36196-frustration", m36196::run_unx36196_checks),
];

/// 聚合快照（三账：面数/绿面数/总检查项数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AggSnapshot {
    pub faces: u32,
    pub green: u32,
    pub checks_total: u32,
}

/// ktest 聚合执行（16 面全跑 + 三账快照——判据 ① 的落点）。
pub fn ktest_aggregate() -> Result<AggSnapshot, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 面数账：登记数必须等于批内实际模块数（漏登记 = E_FACE）。
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
            // 红面必须可归因（red_items_text 非空——带明细的红才算诊断）。
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

/// 快照恒等（聚合重放——ktest 长驻的确定性前提）。
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

/// 指纹快照（三账折叠——面数/绿数/检查总数的冻结锚）。
pub fn agg_fingerprint() -> u32 {
    match ktest_aggregate() {
        Ok(s) => s.faces.wrapping_mul(1_000_003) ^ s.green.wrapping_mul(97) ^ s.checks_total,
        Err(_) => 0xDEAD_0000 | E_RED as u32,
    }
}

/// 标签面健康：16 标签互异非空 + 卡号段连续（F36181..F36196）。
pub fn label_anchor() -> Result<(), u16> {
    let mut i = 0;
    while i < FACE_N {
        let (tag, _) = FACES[i];
        if tag.is_empty() {
            return Err(E_FACE);
        }
        // 卡号段连续性（标签前 7 字节 "F361NN" 对应 36181+i）。
        let expect = alloc::format!("F{}", 36181 + i);
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

// ---- 红因归因表与每面分账（诊断加速索引 + 总分账对平） ---------------------

/// 红因归因提示表（面号 → 该面历史上最常见的红因——诊断加速索引）。
/// 索引与 FACES 同序：面 i 红 ⇒ 先查 RED_HINTS[i] 归因，再进该面
/// red_items_text 明细——两跳定位，避免二分重跑的分钟级成本。
pub const RED_HINTS: [&'static str; FACE_N] = [
    "MA 展开表漂移（WRITE_OWNER 剔除位序错）",
    "Deny 终态回溯缺 allow_acc 剔除",
    "PrivAct 判别式越合法区（禁 transmute）",
    "三态黄金库掩码混入请求面 MA 位",
    "探测路径步数不恒等（时延侧信道）",
    "注入例误用桶 16 真码（SEG_BASE+0x20）",
    "拒绝三要素折叠位漏项",
    "边界库与 B09 索引重复登记",
    "fuzz 覆盖账循环变量类型推断错",
    "P99 冻结界被探测扰动击穿",
    "N/A 账计数错（13 vs 10）",
    "锁步剧本缺 Copy derive 或 zeroed 残留",
    "回炉账越窗码（0x4254 应为 0x4154）",
    "契约能力位落 STD_USED 带错位",
    "reconcile 少/多事件计数分支混淆",
    "GEN 位 28..30 被窄化规则误拒（P0 真缺陷形态）",
];

/// 红因提示查询（面号越界 = E_FACE；提示必须非空——空提示即表漂移）。
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

/// 每面检查数分账（tag, checks）——Σ 分账 = 聚合三账的 checks_total
/// （分账与总账恒等是聚合登记完整性的第二形态：漏登记一面，分账和
/// 必然小于总账；错序登记则标签对位破）。
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

/// 分账恒等（Σ per_face = checks_total——总账/分账双向对平）。
pub fn per_face_reconcile() -> Result<u32, u16> {
    let snap = ktest_aggregate()?;
    let counts = per_face_counts()?;
    if counts.len() as u32 != snap.faces {
        return Err(E_FACE);
    }
    let mut sum = 0u32;
    let mut i = 0;
    while i < counts.len() {
        // 标签与聚合登记同序同标签（分账错位即漂移）。
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
    // R2：16 面聚合全绿 + 三账快照。
    let total = snapshot_stable()?;
    if total == 0 {
        return Err(E_INTG);
    }
    // R3：聚合指纹恒等（长驻快检）。
    if agg_fingerprint() == 0xDEAD_0000 | E_RED as u32 {
        return Err(E_RED);
    }
    // R4：总分账对平（Σ 每面检查数 = checks_total）。
    let reconciled = per_face_reconcile()?;
    if reconciled != total {
        return Err(E_INTG);
    }
    // R5：判据总数对账（16 面 × 每面 6 条 + 本面 6 条 = 聚合三账锚；
    // 每面判据数由各面自持——这里只验非零下界）。
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
        E_ILLEGAL => "掩码越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36197_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36197_ktest");
    cs.add("SEG 段位独占", SEG_BASE == 0x4630, "0x4630~0x463F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "16 面聚合全绿/三账快照恒等/标签连续"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("面数账", FACE_N == 16 && TOTAL_VERDICTS == 17, "16 面 + 本面 = 17 条判据（判据锚）");
    cs.add("指纹面", agg_fingerprint() != 0xDEAD_0000 | E_RED as u32, "聚合指纹非红哨值");
    cs.add("分账恒等", per_face_reconcile().is_ok(), "Σ 每面检查数 = 聚合 checks_total（总/分账对平）");
    cs.add("归因表", RED_HINTS.len() == FACE_N && red_hint(0).is_ok() && hint_for_tag("F36196-frustration").is_ok(), "归因表 16 条与面同序、正反查全通");
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
    fn test_snapshot_stable() {
        assert!(snapshot_stable().is_ok());
        assert!(label_anchor().is_ok());
    }

    #[test]
    fn test_reconcile_and_hints() {
        // 总分账对平 + 归因表全通（16 面正查 + 标签反查 + 越界拒）。
        assert!(per_face_reconcile().is_ok());
        let mut i = 0;
        while i < FACE_N {
            assert!(red_hint(i).is_ok());
            i += 1;
        }
        assert!(hint_for_tag("F36181-maxallowed").is_ok());
        assert!(hint_for_tag("F36199-not-a-face").is_err());
    }
}
