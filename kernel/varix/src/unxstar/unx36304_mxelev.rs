//! UNX-F36304-J1 矩阵第 211–240 格：提权/系统主体面与空态对照。
//! 判据：① 提权/系统主体 30 格全对；② 三态 ACL 对照格（null/空/
//! 无）嵌入全矩阵零混；③ 纯功能代码 ≥300 行。
//!
//! 设计纪律（对标 Windows 特权令牌与 DACL 三态语义）：提权/系统
//! 主体面 S7 = rank 4 + 管理员组 + 四特权全持——它是矩阵的「上界
//! 主体」，除掩码门（客体不支持的访问位）外全部放行：27 许 + 3
//! 掩码拒（注册表键与桌面堆上的 EXECUTE、桌面堆上的 DELETE），
//! 该 3 格是「提权也不越掩码」的对照锚——特权不能创造客体不存
//! 在的访问位，与 Windows 的 DesiredAccess & 对象 rights 掩码语义
//! 对齐。DACL 三态（Windows 命名对象安全描述符的三种特殊态）：
//!   null DACL —— SecurityDescriptor 里 DACL 指针为 null：显式
//!                「人人全控」，求值跳过 ACL 检查（不是缺省！）；
//!   空 DACL   —— DACL 存在但 ACE 数为 0：显式「全拒」；
//!   无 DACL   —— 缺省继承：按父对象/令牌缺省组 ACE 授权求值。
//! 三态对照格**不入 240 格主空间**（对照空间独立编址），对照格
//! 与主矩阵零混 = 主空间 240 格不受三态求值污染、对照结果只挂
//! 在对照键下。三态间的强序：null 不比常规更严、空不比常规更松，
//! 逐例断言。

use crate::checks::CheckSet;
use super::unx36302_mxloband as mx;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB1A0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 0; // 版本锚不符
pub const E_FACE: u16 = SEG_BASE + 1; // 提权面 30 格账不平
pub const E_TRISTATE: u16 = SEG_BASE + 2; // 三态对照例破
pub const E_MIX: u16 = SEG_BASE + 3; // 对照格混入主空间
pub const E_RANGE: u16 = SEG_BASE + 4; // 格号/态值越界
pub const E_SPEC: u16 = SEG_BASE + 5; // 与语义书漂移

/// 模块版本锚。
pub const VER: u32 = 1;

/// 提权面主体（S7）与面格区间（格 211–240）。
pub const FACE: usize = mx::S_ELEVATED;
pub const FACE_LO: u32 = 211;
pub const FACE_HI: u32 = 240;

/// 对照空间键（独立编址 1001..1030，与主空间 1..240 零交）。
pub const VARIANT_LO: u32 = 1001;
pub const VARIANT_HI: u32 = 1030;

// ---- 判据 ①：提权/系统主体 30 格全对 -----------------------------------------
// S7 面逐格：引擎 vs 语义书全对平；面账冻结分布 27 许 + 3 掩码
// 拒；特权面断言：S7 四特权全持 → 面内特权门 0 拒（特权门的
// 「不持权」侧由其他面显影，本面是「持权放行」侧对照）。
pub fn elevated_face_sweep() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let subj = mx::subject_at(FACE)?;
    if subj.rank != 4 || subj.group != 1 {
        return Err(E_FACE); // 提权面定义漂移
    }
    if subj.privs & (mx::P_BACKUP | mx::P_TAKEOWN | mx::P_SDCONV | mx::P_RESTORE)
        != (mx::P_BACKUP | mx::P_TAKEOWN | mx::P_SDCONV | mx::P_RESTORE)
    {
        return Err(E_FACE); // 四特权缺一 = 面定义漂移
    }
    let mut allowed = 0u32;
    let mut mask_blocked = 0u32;
    for o in 0..mx::N_OBJ {
        for op in 0..mx::N_OP {
            let got = mx::evaluate(FACE, o, op)?;
            let want = mx::spec_at(mx::cell_id(FACE, o, op)?)?;
            if got != want {
                return Err(E_SPEC);
            }
            match got {
                mx::A_ALLOW => allowed += 1,
                mx::A_MASK => mask_blocked += 1,
                _ => return Err(E_FACE), // 提权面不允许 DACL/IL/特权/过滤拒
            }
        }
    }
    if allowed != 27 || mask_blocked != 3 {
        return Err(E_FACE);
    }
    Ok(allowed * 100 + mask_blocked)
}

// ---- 判据 ②：三态 ACL 对照格（null/空/无）× 2 客体 × 5 操作 = 30 例 ----------
// 对照例键 = 1001 + 序号（O0 五操作三态 15 例 + O5 五操作三态
// 15 例）。逐例断言三态强序与零混：
//   null  ⊇ 常规（null 求值跳过 ACL，永不比常规更严）；
//   空    ⊆ 常规（空 DACL 全拒，永不比常规更松）；
//   无    ≡ 常规（缺省继承在本矩阵按同组 ACE 授权，与常规同谱）。
pub fn tristate_acl_matrix() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut cases = 0u32;
    let mut key = VARIANT_LO;
    for obj in [0usize, 5usize] {
        for op in 0..mx::N_OP {
            let normal = mx::evaluate(FACE, obj, op)?;
            let null_state = mx::evaluate_with_acl_state(FACE, obj, op, 1)?;
            let empty_state = mx::evaluate_with_acl_state(FACE, obj, op, 2)?;
            let absent_state = mx::evaluate_with_acl_state(FACE, obj, op, 3)?;
            // null：只受掩码门约束（提权面无过滤/IL/特权拒）。
            let null_floor = if op == mx::OP_EXEC && (obj == 1 || obj == 5) {
                mx::A_MASK
            } else if op == mx::OP_DEL && obj == 5 {
                mx::A_MASK
            } else {
                mx::A_ALLOW
            };
            if null_state != null_floor {
                return Err(E_TRISTATE);
            }
            // 强序：null 不更严（拒值单调）、空全拒、无 ≡ 常规。
            if null_state != mx::A_ALLOW && null_state != normal && normal == mx::A_ALLOW {
                return Err(E_TRISTATE);
            }
            // 空 DACL：ACL 侧全拒；掩码格（O5 EXEC/DEL）被更前的
            // 掩码门先拦——门序决定归因，拒的结论不变。
            let empty_expect = if (op == mx::OP_EXEC || op == mx::OP_DEL) && obj == 5 {
                mx::A_MASK
            } else {
                mx::A_DACL
            };
            if empty_state != empty_expect {
                return Err(E_TRISTATE);
            }
            if absent_state != normal {
                return Err(E_TRISTATE);
            }
            // 零混：对照键不落主空间；主空间同位置结果未变。
            if key < VARIANT_LO || key > VARIANT_HI || key <= mx::N_CELLS as u32 {
                return Err(E_MIX);
            }
            if mx::spec_at(mx::cell_id(FACE, obj, op)?)? != normal {
                return Err(E_MIX);
            }
            cases += 3; // 10 组 × 3 态 = 30 例（null/空/无各计一例）
            key += 1;
        }
    }
    if cases != 30 {
        return Err(E_TRISTATE);
    }
    Ok(cases)
}

/// 三态求值的态值域核（态值 > 3 必须显性拒——封闭态域）。
pub fn tristate_domain_check() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if mx::evaluate_with_acl_state(FACE, 0, mx::OP_READ, 4).err() != Some(mx::E_RANGE) {
        return Err(E_TRISTATE);
    }
    Ok(())
}

/// 对照账行（键 → 三态码组——对照空间的呈现面）。
pub fn variant_row(key: u32) -> Result<(u8, u8, u8, u8), u16> {
    if key < VARIANT_LO || key > VARIANT_HI {
        return Err(E_RANGE);
    }
    let idx = (key - VARIANT_LO) as usize;
    let obj = if idx < 15 { 0 } else { 5 };
    let op = idx % 5;
    let state = (idx / 5) % 3; // 0 null / 1 empty / 2 absent
    let null_state = mx::evaluate_with_acl_state(FACE, obj, op, 1)?;
    let empty_state = mx::evaluate_with_acl_state(FACE, obj, op, 2)?;
    let absent_state = mx::evaluate_with_acl_state(FACE, obj, op, 3)?;
    let _ = state;
    Ok((state as u8, null_state, empty_state, absent_state))
}

/// 全面 30 键对照账（逐键取行，键连续零跳号）。
pub fn variant_ledger() -> Result<Vec<(u32, u8, u8, u8, u8)>, u16> {
    let mut rows = Vec::new();
    for key in VARIANT_LO..=VARIANT_HI {
        let (st, n, e, a) = variant_row(key)?;
        rows.push((key, st, n, e, a));
    }
    if rows.len() != 30 {
        return Err(E_TRISTATE);
    }
    Ok(rows)
}

// ---- 确定性演练 -------------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let face = elevated_face_sweep()?;
    let tri = tristate_acl_matrix()?;
    tristate_domain_check()?;
    let ledger = variant_ledger()?;
    // 对照账抽核：末键存在、态值合法且空态恒拒（O5 WDAC 为掩码
    // 外格，空态 DACL 全拒口径成立）。
    let last = ledger[29];
    if last.1 > 2 || last.3 != mx::A_DACL {
        return Err(E_TRISTATE);
    }
    if face / 100 != 27 || tri != 30 {
        return Err(E_FACE);
    }
    Ok(face.wrapping_mul(1000).wrapping_add(tri))
}

// ---- 三态呈现账增补 -----------------------------------------------------------
/// null 态放行清单账（提权面 × 2 客体：O0 全 5 操作 + O5 读/写/
/// 改权——null 跳过 ACL 后仅受掩码门约束，放行 8 例）。
pub fn null_allow_rows() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut allowed = 0u32;
    for obj in [0usize, 5usize] {
        for op in 0..mx::N_OP {
            let null_state = mx::evaluate_with_acl_state(FACE, obj, op, 1)?;
            if null_state == mx::A_ALLOW {
                allowed += 1;
            }
        }
    }
    if allowed != 8 {
        return Err(E_TRISTATE);
    }
    Ok(allowed)
}

/// 三态分布账（10 组对照例的三态结果守恒：null 放行 8 + 掩码先
/// 拦 2 = 10；空态 ACL 拒 8 + 掩码先拦 2 = 10；无态 ≡ 常规 10/10
/// ——分布冻结，三态任何一处漂移即对照例破）。
pub fn variant_tally() -> Result<(u32, u32, u32, u32), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut null_allow = 0u32;
    let mut null_mask = 0u32;
    let mut empty_dacl = 0u32;
    let mut empty_mask = 0u32;
    let mut absent_eq = 0u32;
    for obj in [0usize, 5usize] {
        for op in 0..mx::N_OP {
            let normal = mx::evaluate(FACE, obj, op)?;
            match mx::evaluate_with_acl_state(FACE, obj, op, 1)? {
                mx::A_ALLOW => null_allow += 1,
                mx::A_MASK => null_mask += 1,
                _ => return Err(E_TRISTATE),
            }
            match mx::evaluate_with_acl_state(FACE, obj, op, 2)? {
                mx::A_DACL => empty_dacl += 1,
                mx::A_MASK => empty_mask += 1,
                _ => return Err(E_TRISTATE),
            }
            if mx::evaluate_with_acl_state(FACE, obj, op, 3)? == normal {
                absent_eq += 1;
            }
        }
    }
    if null_allow != 8 || null_mask != 2 || empty_dacl != 8 || empty_mask != 2 || absent_eq != 10 {
        return Err(E_TRISTATE);
    }
    Ok((null_allow, empty_dacl, empty_mask, absent_eq))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FACE => "提权面 30 格账不平（27 许 3 掩码拒漂移）",
        E_TRISTATE => "三态 ACL 对照例破（null/空/无强序或结果漂移）",
        E_MIX => "对照格混入主空间（240 格被三态求值污染）",
        E_RANGE => "格号/态值越界",
        E_SPEC => "与语义书漂移",
        _ => "未知诊断码",
    }
}

pub fn run_unx36304_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36304_mxelev");
    cs.add("SEG 段位独占", SEG_BASE == 0xB1A0, "0xB1A0~0xB1AF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "提权面 27 许 3 掩码拒·三态 30 例强序成立·零混"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("提权面锚", elevated_face_sweep() == Ok(2703), "S7 全对：许 27 / 掩码拒 3");
    cs.add("三态锚", tristate_acl_matrix() == Ok(30), "null/空/无 × 2 客体 × 5 操作 30 例");
    cs.add("零混锚", VARIANT_LO > mx::N_CELLS as u32, "对照空间 1001..1030 与主空间 1..240 零交");
    cs.add("三态分布账", variant_tally() == Ok((8, 8, 2, 10)) && null_allow_rows() == Ok(8), "null 许 8/掩码 2·空拒 8/掩码 2·无≡常规 10");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face() {
        let cs = run_unx36304_checks();
        assert!(cs.len() > 0);
    }

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36304_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
        assert!(cs.len() >= 5);
    }
}
