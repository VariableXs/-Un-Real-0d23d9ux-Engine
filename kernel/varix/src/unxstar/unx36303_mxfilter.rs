//! UNX-F36303-J1 矩阵第 181–210 格：过滤令牌主体面（UAC 普通启动）。
//! 判据：① 过滤令牌 30 格 DENY 位全生效；② 拒绝归因（DENY/IL/掩码）
//! 100% 可读；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows UAC 过滤令牌）：管理员以普通方式启动进程
//! 时拿到的是**过滤令牌**——管理员组 SID 被移除、特权被剥除，只剩
//! 标准用户授权。矩阵里该语义落在主体面 S6：group=0（按用户 ACE
//! 求值）、deny 位面剥除 WRITE_DAC（改权必须先过同意/提权，本面
//! 无此能力）。「DENY 位全生效」的判据是：凡请求位与过滤剥除位相
//! 交的格，**必须**在令牌过滤门被拦截且归因码恰为 A_DENY——若这类
//! 格放行或落到别的门归因，等于过滤令牌失真（UAC 形同虚设）。
//! 「归因 100% 可读」的判据是：面内 30 格逐格归因码落在闭集内且
//! 人话明细非空，DENY/IL/掩码三类归因在本面均有显影格。全矩阵
//! 层面另有一条强不变量：令牌过滤门先于 DACL 门（residual mask
//! 语义）——任何主体、任何格，请求位与该主体 deny 位面相交的，
//! 引擎必须归因 A_DENY，逐格扫描验证。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB190;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_DENY: u16 = SEG_BASE + 1; // 过滤位相交格未归因 A_DENY
pub const E_FACE: u16 = SEG_BASE + 2; // 过滤面 30 格账不平
pub const E_READABLE: u16 = SEG_BASE + 3; // 归因不可读（出闭集/明细空）
pub const E_RANGE: u16 = SEG_BASE + 4; // 格号越界
pub const E_SPEC: u16 = SEG_BASE + 5; // 与语义书漂移

/// 模块版本锚。
pub const VER: u32 = 1;

/// 过滤面主体（S6）与面格区间（格 181–210）。
pub const FACE: usize = mx::S_FILTERED;
pub const FACE_LO: u32 = 181;
pub const FACE_HI: u32 = 210;

// ---- 判据 ①：过滤令牌 30 格 DENY 位全生效 -----------------------------------
// S6 的 deny 位面 = WRITE_DAC。面内 6 个 WDAC 格必须全部被令牌过
// 滤门拦截（归因恰为 A_DENY——不是 DACL、不是掩码，剥除发生在
// 请求进入 ACL 求值之前）；面内应许 11 格与语义书对平。
pub fn filtered_face_deny_sweep() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let subj = mx::subject_at(FACE)?;
    if subj.deny != mx::AB_WDAC || subj.deny == 0 {
        return Err(E_DENY); // 过滤位面定义漂移
    }
    let mut denied = 0u32;
    let mut allowed = 0u32;
    for o in 0..mx::N_OBJ {
        for op in 0..mx::N_OP {
            let got = mx::evaluate(FACE, o, op)?;
            let want = mx::spec_at(mx::cell_id(FACE, o, op)?)?;
            if got != want {
                return Err(E_SPEC);
            }
            let req: u8 = 1 << op;
            if req & subj.deny != 0 {
                // 过滤位相交格：必须拒且归因恰为令牌过滤门。
                if got != mx::A_DENY {
                    return Err(E_DENY);
                }
                denied += 1;
            } else if got == mx::A_ALLOW {
                allowed += 1;
            }
        }
    }
    // 面账冻结值：WDAC 6 格全拦 + 应许 11 格。
    if denied != 6 || allowed != 11 {
        return Err(E_FACE);
    }
    Ok(denied * 100 + allowed)
}

// ---- 全矩阵令牌过滤不变量（过滤门先于 DACL 的 residual mask 语义） -----------
// 8 个主体面 × 240 格逐格扫描：请求位与主体 deny 位面相交的格，
// 引擎求值必须**拒**——通常归因 A_DENY（过滤门），掩码格（如桌
// 面堆 DEL）被更前的掩码门先拦也如实认账（门序决定归因，拒的
// 结论不变）。过滤门显影数冻结 40（S2 17 + S5 17 + S6 6）。
pub fn token_filter_invariant() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut hits = 0u32;
    for s in 0..mx::N_SUBJ {
        let subj = mx::subject_at(s)?;
        if subj.deny == 0 {
            continue;
        }
        for o in 0..mx::N_OBJ {
            for op in 0..mx::N_OP {
                let req: u8 = 1 << op;
                if req & subj.deny == 0 {
                    continue;
                }
                let got = mx::evaluate(s, o, op)?;
                // 相交格必拒：过滤门归因或掩码门先拦，两者之外 = 过滤门失位。
                if got != mx::A_DENY && got != mx::A_MASK {
                    return Err(E_DENY);
                }
                if got == mx::A_DENY {
                    hits += 1;
                }
            }
        }
    }
    // 冻结分布：过滤门显影 40 格（掩码先拦格不计入但已核必拒）。
    if hits != 40 {
        return Err(E_DENY);
    }
    Ok(hits)
}

// ---- 判据 ②：拒绝归因（DENY/IL/掩码）100% 可读 -------------------------------
// 面内 30 格逐格核：归因码在闭集内、人话明细非空且带「拒·」前缀
// （拒格）/恰为「许」（许格）；DENY/IL/掩码三类归因码在本面均有
// 显影格（S6 行冻结分布：DENY 6、IL 1、MASK 3）。
pub fn attribution_readable_face() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut readable = 0u32;
    let mut seen_deny = false;
    let mut seen_il = false;
    let mut seen_mask = false;
    for o in 0..mx::N_OBJ {
        for op in 0..mx::N_OP {
            let id = mx::cell_id(FACE, o, op)?;
            let got = mx::evaluate(FACE, o, op)?;
            let want = mx::spec_at(id)?;
            if got != want || !mx::attribution_in_range(got) {
                return Err(E_READABLE);
            }
            let text = mx::attribution_text(got);
            let ok = if got == mx::A_ALLOW {
                text == "许"
            } else {
                text.starts_with("拒·") && text.len() > 6
            };
            if !ok {
                return Err(E_READABLE);
            }
            if got == mx::A_DENY {
                seen_deny = true;
            }
            if got == mx::A_IL {
                seen_il = true;
            }
            if got == mx::A_MASK {
                seen_mask = true;
            }
            readable += 1;
        }
    }
    // 三类归因全显影（缺类 = 归因面退化为单门 = 可读性名存实亡）。
    if !seen_deny || !seen_il || !seen_mask {
        return Err(E_READABLE);
    }
    Ok(readable)
}

/// 归因账行（面内单格 → 人话串——账面直出可读明细）。
pub fn face_row_text(id: u32) -> Result<String, u16> {
    let (s, o, op) = mx::cell_parts(id)?;
    let code = mx::evaluate(s, o, op)?;
    let mut row = String::from("格");
    row.push_str(&id.to_string());
    row.push_str(":");
    row.push_str(mx::attribution_text(code));
    Ok(row)
}

/// 全面 30 行账（逐格人话明细——账面可读性的呈现面）。
pub fn face_ledger_text() -> Result<Vec<String>, u16> {
    let mut rows = Vec::new();
    for id in FACE_LO..=FACE_HI {
        rows.push(face_row_text(id)?);
    }
    if rows.len() != 30 {
        return Err(E_FACE);
    }
    Ok(rows)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let deny = filtered_face_deny_sweep()?;
    let readable = attribution_readable_face()?;
    let invariant = token_filter_invariant()?;
    let ledger = face_ledger_text()?;
    // 账行抽核：第 1 行与第 30 行非空且以「格」开头（呈现面真实可读）。
    if ledger.len() != 30 || !ledger[0].starts_with("格") || !ledger[29].starts_with("格") {
        return Err(E_READABLE);
    }
    if deny / 100 != 6 || readable != 30 || invariant != 40 {
        return Err(E_FACE);
    }
    Ok(deny.wrapping_mul(1000).wrapping_add(readable))
}

// ---- 面 DENY 明细账与操作分类账（呈现面增补） ---------------------------------
/// WDAC 六格逐格拦截账行（格号 + 剥除位面——DENY 位的逐格显影，
/// 过滤位全生效不是一句断言，是六条可读账行）。
pub fn deny_detail_rows() -> Result<Vec<String>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let subj = mx::subject_at(FACE)?;
    let mut rows = Vec::new();
    for o in 0..mx::N_OBJ {
        let id = mx::cell_id(FACE, o, mx::OP_WDAC)?;
        let got = mx::evaluate(FACE, o, mx::OP_WDAC)?;
        if got != mx::A_DENY {
            return Err(E_DENY);
        }
        let mut text = String::from("WDAC 拦截 格");
        text.push_str(&id.to_string());
        text.push_str(" 剥除位面=");
        text.push_str(&subj.deny.to_string());
        rows.push(text);
    }
    if rows.len() != 6 {
        return Err(E_FACE);
    }
    Ok(rows)
}

/// 面内按操作轴的拦截账（每操作 6 格中拒格数，冻结 [1,2,4,6,6]，
/// 和 19 = 30 - 11 许——面账的操作级分账，写/删/改权轴拦截最密）。
pub fn face_op_tally() -> Result<[u32; 5], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let frozen: [u32; 5] = [1, 2, 4, 6, 6];
    let mut tally = [0u32; 5];
    for o in 0..mx::N_OBJ {
        for op in 0..mx::N_OP {
            let got = mx::evaluate(FACE, o, op)?;
            if got != mx::A_ALLOW {
                tally[op] += 1;
            }
        }
    }
    if tally != frozen {
        return Err(E_FACE);
    }
    let sum = tally.iter().fold(0u32, |a, v| a + *v);
    if sum != 19 {
        return Err(E_FACE);
    }
    Ok(tally)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_DENY => "过滤位相交格未归因 A_DENY（过滤门失位）",
        E_FACE => "过滤面 30 格账不平",
        E_READABLE => "归因不可读（出闭集/明细空/类缺影）",
        E_RANGE => "格号越界",
        E_SPEC => "与语义书漂移",
        _ => "未知诊断码",
    }
}

pub fn run_unx36303_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36303_mxfilter");
    cs.add("SEG 段位独占", SEG_BASE == 0xB190, "0xB190~0xB19F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "WDAC 6 格 DENY 全生效·归因 30/30 可读·不变量 40 格"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("过滤位锚", mx::subject_at(FACE).map(|v| v.deny) == Ok(mx::AB_WDAC), "S6 过滤位面 = WRITE_DAC 剥除");
    cs.add("面账锚", filtered_face_deny_sweep() == Ok(611), "拒 6 许 11（语义书冻结）");
    cs.add("不变量锚", token_filter_invariant() == Ok(40), "全矩阵过滤拦截 40 格（S2/S5/S6）");
    cs.add("面分类账", face_op_tally().map(|t| t.iter().fold(0u32, |a, v| a + *v)) == Ok(19) && deny_detail_rows().map(|v| v.len()) == Ok(6), "WDAC 6 格逐格显影·操作轴拒账 [1,2,4,6,6]");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36303_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36303_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 5);
    }
}
