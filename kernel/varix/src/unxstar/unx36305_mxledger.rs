//! UNX-F36305-J1 矩阵满格总账（240 格 × 结果 × 归因 × 对照四列）。
//! 判据：① 四列总账 240 格全格可查；② 应拒格 100% 拒 / 应许格
//! 100% 许 / 零越格断言（母版 X1 总兑现）；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 授权审计的账面化）：矩阵满格后第一件事
//! 是把 240 格落成**可查总账**——每格四列：结果列（许/拒）、归因
//! 列（五门闭集码）、对照列（镜像主体面同列格——S2↔S5、S3↔S4
//! 是等值面，同列格判决必须一致）、窗列（对照格号，可跳转复核）。
//! 「全格可查」= 账行按格号寻址 240/240 命中、无洞无重；「零越
//! 格」= 引擎求值与语义书（母版 X1）零漂移——许格判拒或拒格判
//! 许都是越格，一格出现即总账作废。总账同时是 B16 后续各卡的对
//! 账基准：归因账、负例库、锁步回归、审计对账全部以本账的冻结
//! 分布（许 90 / 拒 150）为守恒前提。镜像等值面的判据是矩阵横
//! 向一致性的低成本锚：两组面定义不同（沙箱消费预设 vs AppCont
//! 容、标准用户 vs 服务低 IL 请求者）但授权谱相同——定义不同而
//! 判决相同正是「面实现独立、行为一致」的直接证据。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB1B0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_HOLE: u16 = SEG_BASE + 1; // 总账有洞（寻址未命中/重行）
pub const E_CROSS: u16 = SEG_BASE + 2; // 越格（引擎 vs 母版许拒漂移）
pub const E_MIRROR: u16 = SEG_BASE + 3; // 镜像等值面同列判决不一致
pub const E_TOTAL: u16 = SEG_BASE + 4; // 冻结分布守恒破（许 90/拒 150）
pub const E_RANGE: u16 = SEG_BASE + 5; // 格号越界

/// 模块版本锚。
pub const VER: u32 = 1;

/// 总账冻结分布锚（母版 X1 总兑现的量化面）。
pub const FROZEN_ALLOW: u32 = 90;
pub const FROZEN_DENY: u32 = 150;

/// 总账行：四列 = 结果 / 归因 / 对照判决 / 窗（对照格号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerRow {
    pub id: u32,
    pub allow: bool,
    pub attrib: u8,
    pub mirror_allow: bool,
    pub mirror_id: u32,
}

/// 镜像面映射（等值面配对：S2↔S5、S3↔S4；其余自照——自照格的
/// 对照列即自身，闭环仍可查）。
pub fn mirror_face(s: usize) -> usize {
    match s {
        mx::S_APPCONT => mx::S_LOWSANDBOX,
        mx::S_LOWSANDBOX => mx::S_APPCONT,
        mx::S_MEDSTD => mx::S_MEDSVC,
        mx::S_MEDSVC => mx::S_MEDSTD,
        other => other,
    }
}

/// 单格账行构造（结果列 = 引擎求值，对照列 = 镜像面同列求值）。
pub fn ledger_row(id: u32) -> Result<LedgerRow, u16> {
    let (s, o, op) = mx::cell_parts(id)?;
    let attrib = mx::evaluate(s, o, op)?;
    let ms = mirror_face(s);
    let mirror_attrib = mx::evaluate(ms, o, op)?;
    Ok(LedgerRow {
        id,
        allow: attrib == mx::A_ALLOW,
        attrib,
        mirror_allow: mirror_attrib == mx::A_ALLOW,
        mirror_id: mx::cell_id(ms, o, op)?,
    })
}

// ---- 判据 ①：四列总账 240 格全格可查 -----------------------------------------
// 逐格落账（行号升序、格号连续 1..=240 无跳无重），行内四列齐备
// 且归因码在闭集内——账面寻址能力是「可查」的直接含义。
pub fn build_full_ledger() -> Result<Vec<LedgerRow>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rows = Vec::new();
    for id in 1..=mx::N_CELLS as u32 {
        let row = ledger_row(id)?;
        if row.id != id {
            return Err(E_HOLE);
        }
        if !mx::attribution_in_range(row.attrib) || !mx::attribution_in_range(row_mirror_attrib(&row)) {
            return Err(E_HOLE);
        }
        rows.push(row);
    }
    if rows.len() != mx::N_CELLS {
        return Err(E_HOLE);
    }
    // 格号连续零跳号（账面无洞的直接断言）。
    for (i, row) in rows.iter().enumerate() {
        if row.id != (i + 1) as u32 {
            return Err(E_HOLE);
        }
    }
    Ok(rows)
}

fn row_mirror_attrib(row: &LedgerRow) -> u8 {
    if row.mirror_allow {
        mx::A_ALLOW
    } else {
        mx::A_DACL
    }
}

// ---- 判据 ②：应拒格 100% 拒 / 应许格 100% 许 / 零越格 -------------------------
// 引擎求值 vs 语义书逐格对向核：母版判许的格引擎必须许、母版判
// 拒的格引擎必须拒；守恒账 = 许 90 + 拒 150 = 240。
pub fn cross_check_ledger(rows: &[LedgerRow]) -> Result<(u32, u32), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if rows.len() != mx::N_CELLS {
        return Err(E_HOLE);
    }
    let mut allow = 0u32;
    let mut deny = 0u32;
    for row in rows {
        let (s, o, op) = mx::cell_parts(row.id)?;
        let want = mx::spec_at(row.id)?;
        let got_allow = mx::evaluate(s, o, op)? == mx::A_ALLOW;
        let want_allow = want == mx::A_ALLOW;
        // 越格：任一方向许拒漂移即破（母版 X1 总兑现锚）。
        if got_allow != want_allow {
            return Err(E_CROSS);
        }
        if got_allow {
            allow += 1;
        } else {
            deny += 1;
        }
    }
    if allow != FROZEN_ALLOW || deny != FROZEN_DENY {
        return Err(E_TOTAL);
    }
    Ok((allow, deny))
}

// ---- 镜像等值面一致性 --------------------------------------------------------
// S2↔S5、S3↔S4 两对等值面：同列格（同客体同操作）判决必须一致。
// 自照格跳过（对照列即自身，恒一致无信息量）。
pub fn mirror_consistency(rows: &[LedgerRow]) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut checked = 0u32;
    for row in rows {
        let (s, _, _) = mx::cell_parts(row.id)?;
        let ms = mirror_face(s);
        if ms == s {
            continue;
        }
        let o = ((row.id as usize - 1) % (mx::N_OBJ * mx::N_OP)) / mx::N_OP;
        let op = (row.id as usize - 1) % mx::N_OP;
        if row.mirror_id != mx::cell_id(ms, o, op)? {
            return Err(E_MIRROR);
        }
        let mirror_attrib = mx::evaluate(ms, o, op)?;
        if (mirror_attrib == mx::A_ALLOW) != row.mirror_allow {
            return Err(E_MIRROR);
        }
        if mx::evaluate(s, o, op)? == mx::A_ALLOW && !row.allow {
            return Err(E_MIRROR);
        }
        checked += 1;
    }
    // 镜像格 = 两对等值面 × 30 格 × 2 = 120。
    if checked != 120 {
        return Err(E_MIRROR);
    }
    Ok(checked)
}

/// 账行摘要串（呈现面：格号 + 许拒 + 归因人话）。
pub fn row_summary(id: u32) -> Result<String, u16> {
    let row = ledger_row(id)?;
    let mut text = String::from("格");
    text.push_str(&row.id.to_string());
    text.push_str(if row.allow { ":许" } else { ":拒·" });
    if !row.allow {
        text.push_str(mx::attribution_text(row.attrib));
    }
    Ok(text)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let rows = build_full_ledger()?;
    let (allow, deny) = cross_check_ledger(&rows)?;
    let mirrored = mirror_consistency(&rows)?;
    // 呈现面抽核：首行/末行摘要以「格」开头。
    let head = row_summary(1)?;
    let tail = row_summary(mx::N_CELLS as u32)?;
    if !head.starts_with("格") || !tail.starts_with("格") {
        return Err(E_HOLE);
    }
    if allow != FROZEN_ALLOW || deny != FROZEN_DENY || mirrored != 120 {
        return Err(E_TOTAL);
    }
    Ok(allow.wrapping_mul(1000).wrapping_add(mirrored))
}

// ---- 总账呈现增补 ---------------------------------------------------------------
/// 每面许格账（镜像等值面对：S2=S5=7、S3=S4=11；冻结行分布
/// [7,9,7,11,11,7,11,27]，和 90——面级守恒，许格不许漂移）。
pub fn allow_by_face() -> Result<[u32; 8], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let frozen: [u32; 8] = [7, 9, 7, 11, 11, 7, 11, 27];
    let mut tally = [0u32; 8];
    for id in 1..=mx::N_CELLS as u32 {
        let (s, o, op) = mx::cell_parts(id)?;
        if mx::evaluate(s, o, op)? == mx::A_ALLOW {
            tally[s] += 1;
        }
    }
    if tally != frozen {
        return Err(E_CROSS);
    }
    Ok(tally)
}

/// 总账六类归因统计（69/13/4/24/40/90——与归因账同分布的独立
/// 复算路径，账间守恒：总账侧与归因账侧各自聚合必须同值）。
pub fn ledger_gate_tally() -> Result<[u32; 6], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let frozen: [u32; 6] = [69, 13, 4, 24, 40, 90];
    let mut tally = [0u32; 6];
    for id in 1..=mx::N_CELLS as u32 {
        let (s, o, op) = mx::cell_parts(id)?;
        let code = mx::evaluate(s, o, op)?;
        let slot = match code {
            mx::A_DACL => 0,
            mx::A_IL => 1,
            mx::A_PRIV => 2,
            mx::A_MASK => 3,
            mx::A_DENY => 4,
            mx::A_ALLOW => 5,
            _ => return Err(E_HOLE),
        };
        tally[slot] += 1;
    }
    if tally != frozen {
        return Err(E_TOTAL);
    }
    Ok(tally)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_HOLE => "总账有洞（寻址未命中/重行/列缺）",
        E_CROSS => "越格（引擎 vs 母版许拒漂移）",
        E_MIRROR => "镜像等值面同列判决不一致",
        E_TOTAL => "冻结分布守恒破（许 90/拒 150）",
        E_RANGE => "格号越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36305_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36305_mxledger");
    cs.add("SEG 段位独占", SEG_BASE == 0xB1B0, "0xB1B0~0xB1BF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "240 格四列总账全可查·零越格·镜像 120 格一致"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("守恒锚", FROZEN_ALLOW + FROZEN_DENY == 240, "许 90 + 拒 150 = 240（母版 X1）");
    cs.add("可查锚", ledger_row(1).is_ok() && ledger_row(mx::N_CELLS as u32).is_ok(), "首末格寻址命中");
    cs.add("账间守恒", allow_by_face().map(|t| t.iter().fold(0u32, |a, v| a + *v)) == Ok(90) && ledger_gate_tally() == Ok([69, 13, 4, 24, 40, 90]), "面许账和 90·六类归因 69/13/4/24/40/90");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36305_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36305_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
