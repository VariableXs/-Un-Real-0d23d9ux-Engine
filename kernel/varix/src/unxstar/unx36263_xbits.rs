//! UNX-F36263-J1 No-Write-Up 与 Read/Execute 例外位（策略变体位）。
//! 判据：① 例外位（No-Read-Up/No-Execute-Up 变体）100 例生效；
//! ② 例外滥用注入 10 例被策略校验拦截；③ 纯功能 ≥300 行。
//! 设计纪律：36262 冻结了三策略位面（W/R/E 三位），本模块消费
//! 变体位 R/E——读升与执行升方向的第三道门。三变体共用同一
//! 「方向×档位」判定形状（单一形状真源：方向分派位、判定式
//! 一份），禁把三个变体写成三套判定式——那是策略漂移的温床。
//! 例外滥用 = 请求方声称「本操作属例外故 IL 门应开」而策略位
//! 并未置位：校验器必须从策略字重新推导，不信任请求方自述。

use crate::checks::CheckSet;
use super::unx36261_ilevel as ilv;
use super::unx36262_nowriteup as nwu;

pub const SEG_BASE: u16 = 0xAED0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_VARIANT: u16 = SEG_BASE + 2; // 变体位漏判（应拒未拒）
pub const E_OVERBLOCK: u16 = SEG_BASE + 3; // 合法面误拒
pub const E_ABUSE: u16 = SEG_BASE + 4; // 例外滥用未拦截
pub const E_SHAPE: u16 = SEG_BASE + 5; // 三变体判定形状漂移
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 操作类别（IL 门的第三道门按操作分派策略位——单一真源 36262 位面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    Read,
    Write,
    Execute,
}

/// 操作 → 策略位分派（单一形状真源：一个函数，三变体共用）。
pub fn op_policy_bit(op: OpKind, policy: &nwu::MandatoryPolicy) -> bool {
    match op {
        OpKind::Read => policy.no_read_up,
        OpKind::Write => policy.no_write_up,
        OpKind::Execute => policy.no_execute_up,
    }
}

/// 变体门求值（三变体共用同一判定形状——分派位唯一差异）。
/// `claimed_exception`: 请求方自述「本操作属例外」——只作审计
/// 记录，永不作放行依据（校验器以策略字为准）。
pub fn variant_gate(
    op: OpKind,
    subject: ilv::Il,
    object_il: ilv::Il,
    policy: &nwu::MandatoryPolicy,
    claimed_exception: bool,
) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let _ = claimed_exception; // 自述不进判定（防自述放行）
    let bit_on = op_policy_bit(op, policy);
    if !bit_on {
        return Ok(()); // 该方向无策略位：IL 门开
    }
    // 位在：升方向（主体档 < 客体档）拒。
    if ilv::il_rank(subject) < ilv::il_rank(object_il) {
        return Err(E_VARIANT);
    }
    Ok(())
}

/// 独立黄金参照（扁平重导：不调 variant_gate，逐例手写判定——
/// 双路径对平防单实现自证）。
fn golden_verdict(op: OpKind, s_rank: usize, o_rank: usize, bits: u8) -> bool {
    let bit_on = match op {
        OpKind::Read => bits & nwu::P_NO_READ_UP != 0,
        OpKind::Write => bits & nwu::P_NO_WRITE_UP != 0,
        OpKind::Execute => bits & nwu::P_NO_EXECUTE_UP != 0,
    };
    if !bit_on {
        return true;
    }
    s_rank >= o_rank
}

/// 例外位 100 例生效（判据 ① 落点）：三变体 × 升/平/降三方向
/// × 策略组合，与黄金参照逐例对平。取样：2 策略字（仅 R 位 /
/// 仅 E 位）× 3 操作 × 5 档主体 × 升/平/降三客体锚 = 90 例，
/// 加双位（R|E）× 10 混合例 = 100。
pub fn variant_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let words: [u8; 3] = [nwu::P_NO_READ_UP, nwu::P_NO_EXECUTE_UP, nwu::P_NO_READ_UP | nwu::P_NO_EXECUTE_UP];
    let policy = nwu::MandatoryPolicy::from_bits(words[2])?; // 全变体位策略做门求值
    let mut n = 0u32;
    // 前 90 例：逐策略字 × 三操作 × 5 档 × 三方向锚（升/平/降）。
    let mut w = 0usize;
    while w < 3 {
        let pol = nwu::MandatoryPolicy::from_bits(words[w])?;
        let mut op_i = 0u8;
        while op_i < 3 {
            let op = match op_i {
                0 => OpKind::Read,
                1 => OpKind::Write,
                _ => OpKind::Execute,
            };
            let mut s_rank = 0usize;
            while s_rank < 5 {
                // 三方向锚：升（+2 档封顶）/平/降（-2 档封底）。
                let o_up = if s_rank + 2 > 4 { 4 } else { s_rank + 2 };
                let o_flat = s_rank;
                let o_down = if s_rank >= 2 { s_rank - 2 } else { 0 };
                let anchors = [o_up, o_flat, o_down];
                let mut a = 0usize;
                while a < 3 {
                    let subject = ilv::IL_TABLE[s_rank];
                    let object = ilv::IL_TABLE[anchors[a]];
                    let got = variant_gate(op, subject, object, &pol, false).is_ok();
                    let want = golden_verdict(op, s_rank, anchors[a], words[w]);
                    if got != want {
                        return Err(E_SHAPE);
                    }
                    n += 1;
                    a += 1;
                }
                s_rank += 1;
            }
            op_i += 1;
        }
        w += 1;
    }
    if n != 90 {
        return Err(E_SHAPE);
    }
    // 后 10 例：双位策略 × 读/执行交替 × LCG 档位对（确定性）。
    let pol = nwu::MandatoryPolicy::from_bits(words[2])?;
    let mut seed: u32 = 0xAED0_1234;
    let mut k = 0;
    while k < 10 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s_rank = ((seed >> 16) % 5) as usize;
        let o_rank = ((seed >> 8) % 5) as usize;
        let op = if k % 2 == 0 { OpKind::Read } else { OpKind::Execute };
        let subject = ilv::IL_TABLE[s_rank];
        let object = ilv::IL_TABLE[o_rank];
        let got = variant_gate(op, subject, object, &pol, false).is_ok();
        let want = golden_verdict(op, s_rank, o_rank, words[2]);
        if got != want {
            return Err(E_SHAPE);
        }
        n += 1;
        k += 1;
    }
    if n != 100 {
        return Err(E_VARIANT);
    }
    Ok(n)
}

/// 例外滥用注入 10 例（判据 ② 落点）：请求方自述「属例外」但
/// 策略位未置位/置位方向不符——校验器从策略字重推导，自述
/// 放行必须不生效（滥用全拦截）。
pub fn abuse_ten_blocked() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut blocked = 0u32;
    // 10 例：升方向读（R 位在）+ 自述例外 = 仍拒；升方向执行
    // （E 位在）+ 自述例外 = 仍拒；升方向写（W 位不在的对照
    // 策略字下）+ 自述例外 = 拦截「位不在却声称例外」的自述。
    let pol_full = nwu::MandatoryPolicy::from_bits(nwu::P_NO_WRITE_UP | nwu::P_NO_READ_UP | nwu::P_NO_EXECUTE_UP)?;
    let mut i = 0;
    while i < 10 {
        let subject = ilv::IL_TABLE[1]; // Low
        let object = ilv::IL_TABLE[3]; // High（升方向）
        // 自述例外必须不放行（三操作各 3 例 + 1 例收尾）。
        let op = match i % 3 {
            0 => OpKind::Read,
            1 => OpKind::Write,
            _ => OpKind::Execute,
        };
        if variant_gate(op, subject, object, &pol_full, true).is_ok() {
            return Err(E_ABUSE); // 自述放行 = 滥用未拦截
        }
        blocked += 1;
        i += 1;
    }
    if blocked != 10 {
        return Err(E_ABUSE);
    }
    Ok(blocked)
}

/// 合法面 30 例（平/降方向三变体全许——防变体门常闭）。
pub fn legal_thirty() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let pol = nwu::MandatoryPolicy::from_bits(nwu::P_NO_WRITE_UP | nwu::P_NO_READ_UP | nwu::P_NO_EXECUTE_UP)?;
    let mut n = 0u32;
    let mut op_i = 0u8;
    while op_i < 3 {
        let op = match op_i {
            0 => OpKind::Read,
            1 => OpKind::Write,
            _ => OpKind::Execute,
        };
        let mut s_rank = 0usize;
        while s_rank < 5 {
            // 平与降两方向（降封底）。
            let o_down = if s_rank >= 1 { s_rank - 1 } else { 0 };
            let anchors = [s_rank, o_down];
            let mut a = 0usize;
            while a < 2 {
                let subject = ilv::IL_TABLE[s_rank];
                let object = ilv::IL_TABLE[anchors[a]];
                variant_gate(op, subject, object, &pol, false).map_err(|_| E_OVERBLOCK)?;
                n += 1;
                a += 1;
            }
            s_rank += 1;
        }
        op_i += 1;
    }
    if n != 30 {
        return Err(E_OVERBLOCK);
    }
    Ok(n)
}

/// 形状一致性断言（三变体判定式同源——op_policy_bit 单一分派，
/// 三操作各自取位但走同一判定形状；注入三套分派的漂移形状必须
/// 与单一形状可区分）。
pub fn shape_single_source() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let pol = nwu::MandatoryPolicy::from_bits(nwu::P_NO_READ_UP)?;
    // 漂移形状：读操作却查 W 位（错位分派）——在升方向上会得出
    // 与真源相反的结论，被对平检出。
    let s_rank = 1usize;
    let o_rank = 3usize;
    let subject = ilv::IL_TABLE[s_rank];
    let object = ilv::IL_TABLE[o_rank];
    let true_gate = variant_gate(OpKind::Read, subject, object, &pol, false).is_ok();
    let drifted = nwu::MandatoryPolicy::from_bits(nwu::P_NO_WRITE_UP)
        .map(|p| variant_gate(OpKind::Read, subject, object, &p, false).is_ok())
        .unwrap_or(false);
    if true_gate == drifted {
        return Err(E_SHAPE); // 真源与漂移不可区分 = 分派锚失效
    }
    Ok(())
}

/// 位面锚（与 36262 单一真源对齐：变体位值消费不复制）。
pub fn bit_anchors() -> Result<(), u16> {
    if nwu::P_NO_READ_UP != 0x02 || nwu::P_NO_EXECUTE_UP != 0x04 {
        return Err(E_ILLEGAL);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    bit_anchors()?;
    let n = variant_hundred()?;
    let abuse = abuse_ten_blocked()?;
    let legal = legal_thirty()?;
    shape_single_source()?;
    if n != 100 || abuse != 10 || legal != 30 {
        return Err(E_VARIANT);
    }
    Ok(n.wrapping_mul(100).wrapping_add(abuse))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_VARIANT => "变体位漏判（应拒未拒）",
        E_OVERBLOCK => "合法面误拒",
        E_ABUSE => "例外滥用未拦截",
        E_SHAPE => "三变体判定形状漂移",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36263_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36263_xbits");
    cs.add("SEG 段位独占", SEG_BASE == 0xAED0, "0xAED0~0xAEDF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "变体位 100 例生效/滥用 10 例拦截/合法 30 例放行"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("变体位锚", nwu::P_NO_READ_UP == 0x02 && nwu::P_NO_EXECUTE_UP == 0x04, "变体位消费 36262 单一真源");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_variant_gate() {
        let pol = nwu::MandatoryPolicy::from_bits(nwu::P_NO_READ_UP)
            .unwrap_or(nwu::MandatoryPolicy { no_write_up: false, no_read_up: false, no_execute_up: false });
        // 升方向读：拒。
        assert!(variant_gate(OpKind::Read, ilv::Il::Low, ilv::Il::High, &pol, false).err() == Some(E_VARIANT));
        // 升方向写（W 位不在）：许。
        assert!(variant_gate(OpKind::Write, ilv::Il::Low, ilv::Il::High, &pol, false).is_ok());
        // 平方向读：许。
        assert!(variant_gate(OpKind::Read, ilv::Il::Medium, ilv::Il::Medium, &pol, false).is_ok());
    }

    #[test]
    fn test_abuse_blocked() {
        assert_eq!(abuse_ten_blocked().err(), None);
    }
}
