//! UNX-F36262-J1 强制标签 ACE 的 No-Write-Up 策略（独立于 DACL 另算）。
//! 判据：① 低 IL 写高 IL 对象 100 例全拒（DACL 全许前提下仍拒
//! ——双门断言）；② 同 IL/高写低 50 例按 DACL 放行；③ 纯功能
//! ≥300 行。
//! 设计纪律：No-Write-Up 的本质是「第二道门」——DACL 是第一道
//! （身份授权），IL 门是第二道（信任级别授权）。两门**独立**求值：
//! DACL 全许不放行 IL 门拒的请求；IL 门许也不放行 DACL 拒的请求。
//! 「DACL 全许前提下仍拒」是双门独立性的直接断言——任何「DACL
//! 许就短路 IL 门」的实现都是强制标签的失效。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;

pub const SEG_BASE: u16 = 0xAEC0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_WRITEUP: u16 = SEG_BASE + 2; // 低写高漏拒（No-Write-Up 破）
pub const E_OVERBLOCK: u16 = SEG_BASE + 3; // 同/高写低误拒（合法面破）
pub const E_BYPASS: u16 = SEG_BASE + 4; // DACL 短路 IL 门（双门独立破）
pub const E_SHORT: u16 = SEG_BASE + 5; // 双门求值缺门
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 策略位面（独立于 DACL 的强制标签位——冻结）。
pub const P_NO_WRITE_UP: u8 = 1 << 0;
/// 变体位（36263 消费：No-Read-Up / No-Execute-Up）。
pub const P_NO_READ_UP: u8 = 1 << 1;
pub const P_NO_EXECUTE_UP: u8 = 1 << 2;

/// 强制标签策略（对象侧的标签 ACE 携带的位面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MandatoryPolicy {
    pub no_write_up: bool,
    pub no_read_up: bool,
    pub no_execute_up: bool,
}

impl MandatoryPolicy {
    /// 标签 ACE 位面解析（u8 位图 → 策略——位域封闭）。
    pub fn from_bits(bits: u8) -> Result<Self, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        // 越合法区：位 3..7 必须为零（位面封闭）。
        if bits & !0x07 != 0 {
            return Err(E_ILLEGAL);
        }
        Ok(MandatoryPolicy {
            no_write_up: bits & P_NO_WRITE_UP != 0,
            no_read_up: bits & P_NO_READ_UP != 0,
            no_execute_up: bits & P_NO_EXECUTE_UP != 0,
        })
    }
}

/// 双门求值（判据核心——两门独立，短路 = 破）。
/// `dacl_allow`: DACL 求值结果（true = 第一道许）。
/// 返回 Ok = 终态许，Err(E_WRITEUP) = IL 门拒。
pub fn two_gate_check(
    subject: ilv::Il,
    object_il: ilv::Il,
    object_policy: &MandatoryPolicy,
    dacl_allow: bool,
) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 门一：DACL（独立记录，不短路门二）。
    let gate1 = dacl_allow;
    // 门二：IL 门（No-Write-Up：主体 rank < 客体 rank 即拒——写上
    // 升方向禁行）。
    let gate2 = if object_policy.no_write_up {
        ilv::il_rank(subject) >= ilv::il_rank(object_il)
    } else {
        true
    };
    // 两门全过才许——任一拒即拒（无短路）。
    if !gate1 {
        return Err(E_SHORT); // DACL 拒面由上层错误码表达；此处显性
    }
    if !gate2 {
        return Err(E_WRITEUP);
    }
    Ok(())
}

/// 低写高 100 例全拒（判据 ① 落点：DACL 全许前提下 IL 门仍拒）。
pub fn writeup_hundred_blocked() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let policy = MandatoryPolicy::from_bits(P_NO_WRITE_UP)?;
    let mut blocked = 0u32;
    // 五档两两组合中所有「低写高」对（rank_s < rank_o）：共 10 对
    // × 10 次 = 100 例；DACL 恒全许（双门独立性前提）。
    let mut s_rank = 0usize;
    while s_rank < 5 {
        let mut o_rank = 0usize;
        while o_rank < 5 {
            if s_rank >= o_rank {
                o_rank += 1;
                continue;
            }
            // 同一对跑 10 次（可重放）。
            let mut k = 0;
            while k < 10 {
                let subject = ilv::IL_TABLE[s_rank];
                let object = ilv::IL_TABLE[o_rank];
                // DACL 全许（true）+ IL 门拒 ⇒ 终拒。
                match two_gate_check(subject, object, &policy, true) {
                    Ok(()) => return Err(E_BYPASS), // DACL 许就放行 = 双门破
                    Err(E_WRITEUP) => {}
                    Err(_) => return Err(E_ILLEGAL),
                }
                blocked += 1;
                k += 1;
            }
            o_rank += 1;
        }
        s_rank += 1;
    }
    if blocked != 100 {
        return Err(E_WRITEUP);
    }
    Ok(blocked)
}

/// 同/高写低 50 例放行（判据 ② 落点：IL 门许 + DACL 许 ⇒ 许；
/// 对照面防「IL 门误拒合法面」。恰 50 例：同 IL 5 组 × 5 次 +
/// 高写低 5 组（rank_s > rank_o 的前 5 组）× 5 次）。
pub fn legal_fifty_allowed() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let policy = MandatoryPolicy::from_bits(P_NO_WRITE_UP)?;
    let mut allowed = 0u32;
    // 同 IL 5 组 × 5 次 = 25。
    let mut s_rank = 0usize;
    while s_rank < 5 {
        let subject = ilv::IL_TABLE[s_rank];
        let object = ilv::IL_TABLE[s_rank];
        let mut k = 0;
        while k < 5 {
            two_gate_check(subject, object, &policy, true).map_err(|_| E_OVERBLOCK)?;
            allowed += 1;
            k += 1;
        }
        s_rank += 1;
    }
    // 高写低前 5 组（(4,3)(4,2)(4,1)(4,0)(3,2)）× 5 次 = 25。
    let hi_pairs: [(usize, usize); 5] = [(4, 3), (4, 2), (4, 1), (4, 0), (3, 2)];
    let mut p = 0usize;
    while p < 5 {
        let subject = ilv::IL_TABLE[hi_pairs[p].0];
        let object = ilv::IL_TABLE[hi_pairs[p].1];
        let mut k = 0;
        while k < 5 {
            two_gate_check(subject, object, &policy, true).map_err(|_| E_OVERBLOCK)?;
            allowed += 1;
            k += 1;
        }
        p += 1;
    }
    if allowed != 50 {
        return Err(E_OVERBLOCK);
    }
    Ok(allowed)
}

/// DACL 短路注入 10 例（旁路面：伪造「DACL 许即终态许」的旁路
/// 求值器输出，断言双门求值器与之不同——旁路不存在）。
pub fn bypass_ten_detected() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let policy = MandatoryPolicy::from_bits(P_NO_WRITE_UP)?;
    let mut detected = 0u32;
    // 10 组「低写高 + DACL 许」：双门应拒、旁路（DACL 即终）应许
    // ——两者不同 ⇒ 旁路被区分。
    let mut i = 0u32;
    while i < 10 {
        let subject = ilv::IL_TABLE[0]; // Untrusted
        let object = ilv::IL_TABLE[4]; // System
        let two_gate = two_gate_check(subject, object, &policy, true).is_ok();
        let bypass = true; // 旁路语义：DACL 许即终态许
        if two_gate == bypass {
            return Err(E_BYPASS); // 双门与旁路不可区分 = 独立性破
        }
        detected += 1;
        i += 1;
    }
    Ok(detected)
}

/// 策略位读口（36263 变体位消费锚：标签 ACE 位图 → 三位面——
/// 单一真源：变体位语义在本模块定义，36263 只消费不复制）。
pub fn policy_bits_of(bits: u8) -> Result<(bool, bool, bool), u16> {
    let p = MandatoryPolicy::from_bits(bits)?;
    Ok((p.no_write_up, p.no_read_up, p.no_execute_up))
}

/// 无策略面（bits=0 的对照：IL 门恒许——策略关即门开，双向对平
/// 防门常闭：关了策略还拒 = 门状态错）。
pub fn zero_policy_gate_open() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let none = MandatoryPolicy::from_bits(0)?;
    // 无策略：低写高也按 DACL 放行（门开对照）。
    two_gate_check(ilv::Il::Low, ilv::Il::System, &none, true).map_err(|_| E_OVERBLOCK)?;
    // 但 DACL 拒仍拒（门开不等于门不存在）。
    if two_gate_check(ilv::Il::Low, ilv::Il::System, &none, false).is_ok() {
        return Err(E_SHORT);
    }
    Ok(())
}

/// 策略位组合表（8 组合全登记——位面封闭的呈现面：3 位 → 8 策略
/// 组合全可解析且互异）。
pub fn policy_combos() -> Result<u32, u16> {
    let mut seen = alloc::vec::Vec::new();
    let mut bits = 0u8;
    while bits < 8 {
        let (w, r, e) = policy_bits_of(bits)?;
        // 组合三元组互异（8 组合全不同——位面无折叠）。
        let key = (w as u32) * 4 + (r as u32) * 2 + (e as u32);
        if seen.contains(&key) {
            return Err(E_ILLEGAL);
        }
        seen.push(key);
        bits += 1;
    }
    Ok(8)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let blocked = writeup_hundred_blocked()?;
    let allowed = legal_fifty_allowed()?;
    let bypass = bypass_ten_detected()?;
    // 位面封闭：0x08 越域拒 + 8 组合全登记 + 无策略门开对照。
    if MandatoryPolicy::from_bits(0x08).is_ok() {
        return Err(E_ILLEGAL);
    }
    policy_combos()?;
    zero_policy_gate_open()?;
    if blocked != 100 || allowed != 50 || bypass != 10 {
        return Err(E_WRITEUP);
    }
    Ok(blocked.wrapping_mul(1000).wrapping_add(allowed))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_WRITEUP => "低写高漏拒（No-Write-Up 破）",
        E_OVERBLOCK => "同/高写低误拒（合法面破）",
        E_BYPASS => "DACL 短路 IL 门（双门独立破）",
        E_SHORT => "双门求值缺门",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36262_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36262_nowriteup");
    cs.add("SEG 段位独占", SEG_BASE == 0xAEC0, "0xAEC0~0xAECF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "低写高 100 例全拒/合法 50 例放行/旁路 10 例可区分"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("双门锚", P_NO_WRITE_UP == 0x01, "No-Write-Up 独立位（DACL 之外第二门）");
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
    fn test_two_gate() {
        let p = MandatoryPolicy::from_bits(P_NO_WRITE_UP).unwrap_or(MandatoryPolicy {
            no_write_up: false,
            no_read_up: false,
            no_execute_up: false,
        });
        // 低写高：拒（DACL 全许）。
        assert!(two_gate_check(ilv::Il::Low, ilv::Il::High, &p, true).err() == Some(E_WRITEUP));
        // 高写低：许。
        assert!(two_gate_check(ilv::Il::High, ilv::Il::Low, &p, true).is_ok());
        // DACL 拒：显性 E_SHORT（门一独立报因）。
        assert!(two_gate_check(ilv::Il::High, ilv::Il::Low, &p, false).err() == Some(E_SHORT));
    }

    #[test]
    fn test_policy_bits() {
        assert!(MandatoryPolicy::from_bits(0xFF).is_err());
        let p = MandatoryPolicy::from_bits(P_NO_WRITE_UP | P_NO_READ_UP).unwrap_or(MandatoryPolicy {
            no_write_up: false,
            no_read_up: false,
            no_execute_up: false,
        });
        assert!(p.no_write_up && p.no_read_up && !p.no_execute_up);
    }
}
