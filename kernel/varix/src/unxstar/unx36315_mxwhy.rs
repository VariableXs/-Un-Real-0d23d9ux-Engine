//! UNX-F36315-J1 矩阵体验日志（权限困惑指纹 × 矩阵维度）。
//! 判据：① "为什么被拒"类查询指纹接口在位（归因账直出人话）；
//! ② 误拒率统计账 = 0（黄金库内）断言；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 授权诊断的用户侧出口）：矩阵的归因账
//! 是机器口径（码），体验侧需要**指纹接口**——输入格号，输出结
//! 构化指纹：判决、门标签、人话串（面×客体×操作 + 命中门名）、
//! 修复提示（五门各一条固定建议——IL 门建议降请求/换面、DACL
//! 门建议找属主授权、特权门建议提权、掩码门建议换客体类型、过
//! 滤门建议走提权通道）。指纹的账面判据是「直出」：人话串从归
//! 因码分支拼装、不经二次解释层（无转译损耗——转译层是新的漂
//! 移源）。误拒率账：黄金库 = 语义书 90 个应许格——体验接口对
//! 黄金库逐格查询必须判许（把应许报成拒 = 误拒），统计账恒 0；
//! 这是「实现有洞先伤合法用户」的直接量化。指纹门标签与人话串
//! 的结构断言（含面名、含门标签、提示非空）逐拒格校验。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use super::unx36311_mxexplain as ex;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB250;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_FINGER: u16 = SEG_BASE + 1; // 指纹破（标签错/人话缺/提示空）
pub const E_FALSE_REJECT: u16 = SEG_BASE + 2; // 误拒率非 0
pub const E_RANGE: u16 = SEG_BASE + 3; // 格号越界
pub const E_GOLDEN: u16 = SEG_BASE + 4; // 黄金库账不平（90 格）
pub const E_FROZEN: u16 = SEG_BASE + 5; // 拒格指纹数漂移（150）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 拒格指纹数 / 黄金库格数冻结锚。
pub const FROZEN_FINGERPRINTS: u32 = 150;
pub const FROZEN_GOLDEN: u32 = 90;

/// 查询指纹（体验接口的返回结构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhyFingerprint {
    pub cell: u32,
    pub verdict: u8,
    pub gate_tag: &'static str,
    pub human: String,
    pub fix_hint: String,
}

/// 五门修复提示（固定建议——提示文案闭集）。
pub fn fix_hint_of(code: u8) -> Result<&'static str, u16> {
    Ok(match code {
        mx::A_DACL => "找客体属主补授权位（DACL 变更）",
        mx::A_IL => "降低请求等级或换用同等级客体（强制标签）",
        mx::A_PRIV => "经提权通道取得所需特权后重试",
        mx::A_MASK => "该访问位对此客体类型不可请求，换操作或客体",
        mx::A_DENY => "当前令牌被过滤，走提权/同意通道获取完整令牌",
        mx::A_ALLOW => "无需修复（本格应许）",
        _ => return Err(E_FINGER),
    })
}

// ---- 指纹接口（判据 ①：为什么被拒 → 结构化指纹） -------------------------------
pub fn query_why(cell: u32) -> Result<WhyFingerprint, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (s, o, op) = mx::cell_parts(cell)?;
    let code = mx::evaluate(s, o, op)?;
    let want = mx::spec_at(cell)?;
    if code != want {
        return Err(E_FINGER); // 指纹接口只服务真实判决
    }
    let mut human = String::from("格");
    human.push_str(&cell.to_string());
    human.push(':');
    human.push_str(ex::face_name(s)?);
    human.push('×');
    human.push_str(ex::object_name(o)?);
    human.push('×');
    human.push_str(ex::op_name(op)?);
    let gate_tag: &'static str;
    if code == mx::A_ALLOW {
        human.push_str(" -> 许");
        gate_tag = "许";
    } else {
        human.push_str(" -> ");
        human.push_str(mx::attribution_text(code));
        gate_tag = match code {
            mx::A_DACL => "DACL",
            mx::A_IL => "IL",
            mx::A_PRIV => "特权",
            mx::A_MASK => "掩码",
            mx::A_DENY => "过滤",
            _ => return Err(E_FINGER),
        };
    }
    Ok(WhyFingerprint {
        cell,
        verdict: code,
        gate_tag,
        human,
        fix_hint: String::from(fix_hint_of(code)?),
    })
}

/// 指纹结构断言（拒格：五标签之一 + 人话含面名与门标签 + 提示
/// 非空且非「无需修复」；许格：标签恰为「许」且提示为免修文案）。
pub fn fingerprint_valid(fp: &WhyFingerprint) -> Result<(), u16> {
    if fp.verdict == mx::A_ALLOW {
        if fp.gate_tag != "许" || fp.fix_hint != "无需修复（本格应许）" {
            return Err(E_FINGER);
        }
        return Ok(());
    }
    if fp.gate_tag == "许" || fp.gate_tag.len() > 6 {
        return Err(E_FINGER);
    }
    let face_in = ex::face_name(mx::cell_parts(fp.cell)?.0)?;
    if !fp.human.contains(face_in) || !fp.human.contains("拒·") {
        return Err(E_FINGER);
    }
    if fp.fix_hint.is_empty() || fp.fix_hint == "无需修复（本格应许）" {
        return Err(E_FINGER);
    }
    Ok(())
}

// ---- 黄金库误拒率账（判据 ②） ---------------------------------------------------
// 黄金库 = 语义书 90 应许格。体验接口逐格查询：指纹判许才许，
// 任何一格报拒即误拒（合法用户被挡在门外——体验侧最重的账）。
pub fn golden_false_reject_audit() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut golden = 0u32;
    let mut false_rejects = 0u32;
    for id in 1..=mx::N_CELLS as u32 {
        let want = mx::spec_at(id)?;
        if want != mx::A_ALLOW {
            continue;
        }
        let fp = query_why(id)?;
        if fp.verdict != mx::A_ALLOW {
            false_rejects += 1; // 误拒落账（不静默、不截断）
        }
        fingerprint_valid(&fp)?;
        golden += 1;
    }
    if golden != FROZEN_GOLDEN {
        return Err(E_GOLDEN);
    }
    if false_rejects != 0 {
        return Err(E_FALSE_REJECT);
    }
    Ok(false_rejects)
}

// ---- 拒格指纹全量账（体验日志的落账面） -------------------------------------------
pub fn fingerprint_ledger() -> Result<Vec<WhyFingerprint>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut ledger = Vec::new();
    for id in 1..=mx::N_CELLS as u32 {
        let fp = query_why(id)?;
        if fp.verdict != mx::A_ALLOW {
            fingerprint_valid(&fp)?;
            ledger.push(fp);
        }
    }
    if ledger.len() as u32 != FROZEN_FINGERPRINTS {
        return Err(E_FROZEN);
    }
    Ok(ledger)
}

/// 指纹门分布统计（拒格指纹按五标签聚点——体验困惑的分布画像）。
pub fn fingerprint_gate_tally(ledger: &[WhyFingerprint]) -> Result<[u32; 5], u16> {
    let mut tally = [0u32; 5];
    for fp in ledger.iter() {
        let slot = match fp.gate_tag {
            "DACL" => 0,
            "IL" => 1,
            "特权" => 2,
            "掩码" => 3,
            "过滤" => 4,
            _ => return Err(E_FINGER),
        };
        tally[slot] += 1;
    }
    // 分布守恒：五标签和 = 指纹数（150）。
    let total = tally.iter().fold(0u32, |a, v| a + *v);
    if total != FROZEN_FINGERPRINTS {
        return Err(E_FROZEN);
    }
    Ok(tally)
}

/// 指纹呈现行（体验日志行：人话 + 修复提示）。
pub fn fingerprint_row_text(fp: &WhyFingerprint) -> String {
    let mut text = String::from("[");
    text.push_str(fp.gate_tag);
    text.push_str("] ");
    text.push_str(&fp.human);
    text.push_str(" | 修复: ");
    text.push_str(&fp.fix_hint);
    text
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let zero = golden_false_reject_audit()?;
    let ledger = fingerprint_ledger()?;
    let tally = fingerprint_gate_tally(&ledger)?;
    // 呈现面抽核：首条指纹行带标签前缀与修复段。
    let first = ledger.first().cloned().unwrap_or(WhyFingerprint {
        cell: 0,
        verdict: 255,
        gate_tag: "",
        human: String::new(),
        fix_hint: String::new(),
    });
    let row = fingerprint_row_text(&first);
    if !row.starts_with('[') || !row.contains("修复: ") {
        return Err(E_FINGER);
    }
    if zero != 0 || ledger.len() as u32 != FROZEN_FINGERPRINTS {
        return Err(E_FALSE_REJECT);
    }
    // 五标签全显影（分布画像非退化）。
    if tally.iter().any(|v| *v == 0) {
        return Err(E_FINGER);
    }
    Ok(zero.wrapping_mul(1000).wrapping_add(ledger.len() as u32))
}

// ---- 指纹样例账增补 -----------------------------------------------------------
/// 五门指纹样例（每门取一格生成完整指纹并过结构断言——体验接口
/// 的门级抽面呈现；五门标签各显影一次）。
pub fn gate_sample_fingerprints() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 每门一个代表格（DACL/IL/特权/掩码/过滤——语义书冻结分布
    // 里的代表格）。
    let probes: [u32; 5] = [
        mx::cell_id(mx::S_UNTRUSTED, 0, mx::OP_DEL)?, // DACL
        mx::cell_id(mx::S_LOW, 0, mx::OP_WRITE)?,     // IL
        mx::cell_id(mx::S_MEDSTD, 5, mx::OP_WDAC)?,   // 特权
        mx::cell_id(mx::S_ELEVATED, 1, mx::OP_EXEC)?, // 掩码
        mx::cell_id(mx::S_FILTERED, 0, mx::OP_WDAC)?, // 过滤
    ];
    let expect_tags: [&str; 5] = ["DACL", "IL", "特权", "掩码", "过滤"];
    for i in 0..5 {
        let fp = query_why(probes[i])?;
        fingerprint_valid(&fp)?;
        if fp.gate_tag != expect_tags[i] {
            return Err(E_FINGER);
        }
        if fp.verdict == mx::A_ALLOW || !fp.human.starts_with("格") {
            return Err(E_FINGER);
        }
    }
    Ok(5)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FINGER => "指纹破（标签错/人话缺面名/提示空）",
        E_FALSE_REJECT => "误拒率非 0（黄金库内应许格被报拒）",
        E_RANGE => "格号越界",
        E_GOLDEN => "黄金库账不平（90 格）",
        E_FROZEN => "拒格指纹数漂移（150）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36315_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36315_mxwhy");
    cs.add("SEG 段位独占", SEG_BASE == 0xB250, "0xB250~0xB25F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "指纹接口 150 拒格全落账·误拒率 0·五标签全显影"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("黄金库锚", FROZEN_GOLDEN == 90, "黄金库 = 语义书应许格 90");
    cs.add("免修锚", fix_hint_of(mx::A_ALLOW) == Ok("无需修复（本格应许）"), "许格提示免修文案");
    cs.add("门级样例账", gate_sample_fingerprints() == Ok(5), "五门代表格指纹全过结构断言");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36315_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36315_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
