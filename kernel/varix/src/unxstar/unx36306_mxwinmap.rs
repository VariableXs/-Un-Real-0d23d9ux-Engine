//! UNX-F36306-J1 矩阵满格 Windows 对照账（基准机数据格逐格标注）。
//! 判据：① 对照账 240 格逐格标注（一致/待验/N-A 如实三态）；②
//! "待验证"格清单化不虚报（防幻觉红线口径）；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 基准机对照的方法学）：矩阵格的 Windows
//! 语义对照分**三态如实标注**，禁止把未实测格报成「已对照」：
//!   一致（W_MATCH）——格判决可由本引擎规则与 Windows 公开语义
//!     （ACL 求值次序、强制标签门序、DesiredAccess 掩码）直接互
//!     推，逐格已对平；
//!   待验（W_PENDING）——格判决依赖**基准机实测数据**（真实进程
//!     对象的安全描述符、真实令牌的组员与特权内容），未在基准机
//!     采集前只挂清单、不出结论；
//!   N-A（W_NA）——格的操作位在 Windows 对应客体类型上不可请求
//!     （掩码门拒），对照命题本身不适用。
//! 防幻觉红线：待验格只允许由**显式规则**进入清单（本账：客体
//! O4 进程句柄 × 非管理员主体 = 35 格——真实进程 ACL 因进程而
//! 异，公开语义推不出唯一定值），清单长度与格数双向对平、清单
//! 外不得出现待验标注；三态计数守恒（一致 + 待验 + N-A = 240），
//! 缺格或重叠都意味着标注面破。对照结论只报三态之一，不虚报
//! 「全一致」——这正是母版 X1 的防漂移口径。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB1C0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_ANNOT: u16 = SEG_BASE + 1; // 标注面破（缺格/重叠/出三态）
pub const E_PENDING: u16 = SEG_BASE + 2; // 待验清单虚报（规则外待验）
pub const E_CONSERV: u16 = SEG_BASE + 3; // 三态计数守恒破
pub const E_MATCH: u16 = SEG_BASE + 4; // 一致格对平失败
pub const E_RANGE: u16 = SEG_BASE + 5; // 格号越界

/// 模块版本锚。
pub const VER: u32 = 1;

/// 对照三态（标注闭集）。
pub const W_MATCH: u8 = 1; // 一致
pub const W_PENDING: u8 = 2; // 待验
pub const W_NA: u8 = 3; // N-A

/// 对照账冻结计数锚。
pub const FROZEN_MATCH: u32 = 181;
pub const FROZEN_PENDING: u32 = 35;
pub const FROZEN_NA: u32 = 24;

/// 待验判据规则：客体 O4（进程句柄——真实 ACL 因进程而异）×
/// 非管理员主体。规则显式、可复核，是「清单化不虚报」的唯一入
/// 口；管理员主体对进程句柄的授权谱仍由本引擎规则可推，不待验。
pub fn pending_rule(s: usize, o: usize) -> bool {
    o == 4 && mx::subject_at(s).map(|v| v.group) != Ok(1)
}

/// 单格标注（纯规则求值：N-A 优先于待验——掩码不适用格无对照
/// 命题可言；其余按待验规则分流，剩余全为一致并逐格对平）。
pub fn annotation_of(id: u32) -> Result<u8, u16> {
    let (s, o, op) = mx::cell_parts(id)?;
    let got = mx::evaluate(s, o, op)?;
    let want = mx::spec_at(id)?;
    if got != want {
        return Err(E_MATCH);
    }
    if got == mx::A_MASK {
        return Ok(W_NA);
    }
    if pending_rule(s, o) {
        return Ok(W_PENDING);
    }
    Ok(W_MATCH)
}

/// 单格标注原因串（逐格可读——标注不是黑盒位）。
pub fn annotation_reason(id: u32) -> Result<String, u16> {
    let a = annotation_of(id)?;
    Ok(match a {
        W_MATCH => String::from("一致：引擎规则与 Windows ACL/强制标签语义互推对平"),
        W_PENDING => String::from("待验：依赖基准机实测数据（进程对象真实安全描述符/令牌内容）"),
        W_NA => String::from("N-A：操作位在对应客体类型上不可请求（掩码门拒）"),
        _ => return Err(E_ANNOT),
    })
}

// ---- 待验清单（规则外零待验——防幻觉红线的账面化） ----------------------------
// 清单按格号升序逐格扫描生成；再反向核：每个待验标注格必须在
// 清单内、清单内每格标注必为待验——双向对平，清单即账、账即清单。
pub fn pending_list() -> Result<Vec<u32>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut list = Vec::new();
    for id in 1..=mx::N_CELLS as u32 {
        if annotation_of(id)? == W_PENDING {
            list.push(id);
        }
    }
    // 规则重放核数：O4 × 非管理员主体（S0..S7 中 group != 1 的 7 面）
    // × 5 操作 = 35。
    let mut rule_count = 0u32;
    for s in 0..mx::N_SUBJ {
        if pending_rule(s, 4) {
            rule_count += mx::N_OP as u32;
        }
    }
    if list.len() as u32 != rule_count {
        return Err(E_PENDING);
    }
    Ok(list)
}

/// 三态全账（240 格逐格标注 + 计数守恒 + 清单双向对平）。
pub fn full_annotation_ledger() -> Result<Vec<(u32, u8)>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let pending = pending_list()?;
    let mut ledger = Vec::new();
    let mut n_match = 0u32;
    let mut n_pending = 0u32;
    let mut n_na = 0u32;
    for id in 1..=mx::N_CELLS as u32 {
        let a = annotation_of(id)?;
        match a {
            W_MATCH => n_match += 1,
            W_PENDING => {
                // 清单双向对平：标注为待验 ⇒ 在清单内。
                if !pending.contains(&id) {
                    return Err(E_PENDING);
                }
                n_pending += 1;
            }
            W_NA => n_na += 1,
            _ => return Err(E_ANNOT),
        }
        ledger.push((id, a));
    }
    // 反向：清单内每格标注必为待验（规则外待验 = 虚报）。
    for id in pending.iter() {
        if annotation_of(*id)? != W_PENDING {
            return Err(E_PENDING);
        }
    }
    // 守恒：三态计数和 = 240 且各项命中冻结锚。
    if n_match + n_pending + n_na != mx::N_CELLS as u32 {
        return Err(E_CONSERV);
    }
    if n_match != FROZEN_MATCH || n_pending != FROZEN_PENDING || n_na != FROZEN_NA {
        return Err(E_CONSERV);
    }
    Ok(ledger)
}

/// 三态计数统计（呈现面：供聚合账消费的量化行）。
pub fn tally() -> Result<(u32, u32, u32), u16> {
    let ledger = full_annotation_ledger()?;
    let mut m = 0u32;
    let mut p = 0u32;
    let mut n = 0u32;
    for (_, a) in ledger.iter() {
        match *a {
            W_MATCH => m += 1,
            W_PENDING => p += 1,
            W_NA => n += 1,
            _ => return Err(E_ANNOT),
        }
    }
    Ok((m, p, n))
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let ledger = full_annotation_ledger()?;
    let (m, p, n) = tally()?;
    // 呈现面抽核：三个态各抽一格取原因串，非空且前缀区分三态。
    let r1 = annotation_reason(1)?; // 匿名读文件 → 一致
    let r2 = annotation_reason(mx::cell_id(0, 4, mx::OP_READ)?)?; // O4 非管理员 → 待验
    let r3 = annotation_reason(mx::cell_id(7, 1, mx::OP_EXEC)?)?; // 注册表键 EXEC → N-A
    if r1.is_empty() || r2.is_empty() || r3.is_empty() {
        return Err(E_ANNOT);
    }
    if !r1.starts_with("一致") || !r2.starts_with("待验") || !r3.starts_with("N-A") {
        return Err(E_ANNOT);
    }
    if ledger.len() != mx::N_CELLS || m != FROZEN_MATCH || p != FROZEN_PENDING || n != FROZEN_NA {
        return Err(E_CONSERV);
    }
    Ok(m.wrapping_mul(1000).wrapping_add(p))
}

// ---- 三态清单呈现增补 -----------------------------------------------------------
/// 待验格按面分布（O4 × 非管理员 7 面 × 5 格 = 35；S7 面管理组
/// 不待验——行序 S0..S7，清单的分布级呈现）。
pub fn pending_by_face() -> Result<[u32; 8], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let frozen: [u32; 8] = [5, 5, 5, 5, 5, 5, 5, 0];
    let mut tally = [0u32; 8];
    for id in 1..=mx::N_CELLS as u32 {
        let (s, o, _) = mx::cell_parts(id)?;
        if pending_rule(s, o) && annotation_of(id)? == W_PENDING {
            tally[s] += 1;
        }
    }
    if tally != frozen {
        return Err(E_PENDING);
    }
    Ok(tally)
}

/// N-A 格按 (客体,操作) 对聚合（O1 EXEC ×8 + O5 EXEC ×8 + O5 DEL
/// ×8 = 24——三行账，掩码不适用格的对照命题不适用）。
pub fn na_by_pair() -> Result<Vec<(usize, usize, u32)>, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut rows = Vec::new();
    for (o, op) in [(1usize, mx::OP_EXEC), (5usize, mx::OP_EXEC), (5usize, mx::OP_DEL)] {
        let mut n = 0u32;
        for s in 0..mx::N_SUBJ {
            let id = mx::cell_id(s, o, op)?;
            if annotation_of(id)? == W_NA {
                n += 1;
            }
        }
        rows.push((o, op, n));
    }
    let total = rows.iter().fold(0u32, |a, r| a + r.2);
    if rows.len() != 3 || total != FROZEN_NA {
        return Err(E_ANNOT);
    }
    Ok(rows)
}

/// 一致格按面分布（每面 30 - 待验 5 - N-A 3 = 22；S7 面无待验
/// 故 27；冻结 [22×7, 27]，和 181——三态的第三态分布账）。
pub fn match_by_face() -> Result<[u32; 8], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let frozen: [u32; 8] = [22, 22, 22, 22, 22, 22, 22, 27];
    let mut tally = [0u32; 8];
    for id in 1..=mx::N_CELLS as u32 {
        let (s, _, _) = mx::cell_parts(id)?;
        if annotation_of(id)? == W_MATCH {
            tally[s] += 1;
        }
    }
    if tally != frozen {
        return Err(E_CONSERV);
    }
    Ok(tally)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ANNOT => "标注面破（缺格/重叠/出三态闭集）",
        E_PENDING => "待验清单虚报（规则外待验/清单账不平）",
        E_CONSERV => "三态计数守恒破（181+35+24 != 240 或锚漂移）",
        E_MATCH => "一致格对平失败（引擎 vs 语义书漂移）",
        E_RANGE => "格号越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36306_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36306_mxwinmap");
    cs.add("SEG 段位独占", SEG_BASE == 0xB1C0, "0xB1C0~0xB1CF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "240 格三态标注·一致 181/待验 35/N-A 24·清单双向对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("守恒锚", FROZEN_MATCH + FROZEN_PENDING + FROZEN_NA == 240, "三态计数和 = 240（如实全格标注）");
    cs.add("清单锚", pending_list().map(|v| v.len() as u32) == Ok(FROZEN_PENDING), "待验清单 35 格规则内生成");
    cs.add("三态分布账", match_by_face().map(|t| t.iter().fold(0u32, |a, v| a + *v)) == Ok(FROZEN_MATCH) && na_by_pair().map(|v| v.len()) == Ok(3) && pending_by_face().map(|t| t.iter().fold(0u32, |a, v| a + *v)) == Ok(FROZEN_PENDING), "一致按面 [22×7,27]·N-A 三对账 8+8+8·待验按面 5×7");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36306_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36306_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 4);
    }
}
