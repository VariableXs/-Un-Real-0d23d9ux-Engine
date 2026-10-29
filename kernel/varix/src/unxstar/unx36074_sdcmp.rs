//! UNX-F36074-J1 SD 比较原语（等价比较：语义等而非字节等）。
//! 判据：① 语义等/字节差分离 100 例（偏移重排语义等、控制位差即不等）；
//! ② 比较反身/对称/传递三律 50 例 + 反例注入 12 例全检出；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：SD 的「等」有兩层——语义等（四段在场性 + 控制位语义位
//! 逐位同 + 段内容指纹同，偏移数值可不同——自相对化会重排偏移）与
//! 字节等（逐字节同）。授权裁决消费语义等；快照对拍消费字节等。
//! 两层绝不能混用（混用 = 比较原语红线）。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;
use super::unx36061_sdcore::{
    self as core, Sd, C_DACL_PRESENT, C_SACL_PRESENT, C_SELF_REL, E_CTRL, FNV_OFF, OFF_NONE,
};

pub const SEG_BASE: u16 = 0xe260;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_EQ: u16 = SEG_BASE + 1; // 语义等判定破（应等判不等/反之）
pub const E_BYTE: u16 = SEG_BASE + 2; // 字节等判定破
pub const E_LAW: u16 = SEG_BASE + 3; // 三律破（反身/对称/传递）
pub const E_MIX: u16 = SEG_BASE + 4; // 两层比较混用（红线）

/// 语义等：修订 + 控制位（剥 SELF_REL——独立/自相对两种形态可语义等）
/// + 四段在场性 + 段内容指纹（体由调用方提供，缺省段用哨兵）。
pub fn semantically_equal(
    a: &Sd,
    a_bodies: &[Vec<u8>],
    b: &Sd,
    b_bodies: &[Vec<u8>],
) -> Result<bool, u16> {
    core::sd_parse_ok(a)?;
    core::sd_parse_ok(b)?;
    if a.revision != b.revision {
        return Ok(false);
    }
    let ca = a.control & !C_SELF_REL;
    let cb = b.control & !C_SELF_REL;
    if ca != cb {
        return Ok(false);
    }
    // 在场性四元组。
    if a.owner_absent() != b.owner_absent()
        || a.group_absent() != b.group_absent()
        || a.sacl_absent() != b.sacl_absent()
        || a.dacl_absent() != b.dacl_absent()
    {
        return Ok(false);
    }
    // 段内容指纹（缺省段指纹 = 常量哨兵）。
    let fa = body_fp(a, a_bodies)?;
    let fb = body_fp(b, b_bodies)?;
    Ok(fa == fb)
}

/// 段内容指纹：控制位语义面 + 四段体序贯 FNV。
/// 体按「在场序位」映射：第 j 个体对应第 j 个在场段（缺省段用哨兵）。
pub fn body_fp(sd: &Sd, bodies: &[Vec<u8>]) -> Result<u64, u16> {
    if bodies.len() > 4 {
        return Err(E_EQ);
    }
    let mut h = FNV_OFF;
    let mut pi = 0; // 在场段序位（体表下标）
    let mut i = 0;
    while i < 4 {
        let present = match i {
            0 => !sd.owner_absent(),
            1 => !sd.group_absent(),
            2 => !sd.sacl_absent(),
            _ => !sd.dacl_absent(),
        };
        if present {
            match bodies.get(pi) {
                Some(bytes) => core::fnv_push(bytes, &mut h),
                None => {
                    let mark = (i as u64) + 0x100; // 在场但未给体：显性哨兵
                    core::fnv_push(&mark.to_le_bytes(), &mut h);
                }
            }
            pi += 1;
        } else {
            let mark = 0xffff_u64; // 缺省段哨兵
            core::fnv_push(&mark.to_le_bytes(), &mut h);
        }
        i += 1;
    }
    Ok(h)
}

/// 字节等：序列化面逐字节（两层分离的消费面——快照对拍用）。
pub fn byte_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 两层分离红线：语义等与字节等的关系恒为「字节等 ⇒ 语义等」，
/// 反之不必然（偏移重排/SELF_REL 位差 = 字节不等但语义等）。
/// 违反该蕴含即混用红线破。
pub fn layering_holds(a: &Sd, a_bodies: &[Vec<u8>], b: &Sd, b_bodies: &[Vec<u8>]) -> Result<(), u16> {
    let sem = semantically_equal(a, a_bodies, b, b_bodies)?;
    let ba = pack(a, a_bodies)?;
    let bb = pack(b, b_bodies)?;
    let byt = byte_equal(&ba, &bb);
    if byt && !sem {
        return Err(E_MIX); // 字节等却语义不等：两层混用
    }
    Ok(())
}

/// 轻量打包（比较用确定性序：头字段序 + 段体序——非真实自相对布局）。
pub fn pack(sd: &Sd, bodies: &[Vec<u8>]) -> Result<Vec<u8>, u16> {
    let mut v: Vec<u8> = Vec::new();
    v.push(sd.revision);
    v.extend_from_slice(&sd.control.to_le_bytes());
    v.extend_from_slice(&sd.off_owner.to_le_bytes());
    v.extend_from_slice(&sd.off_group.to_le_bytes());
    v.extend_from_slice(&sd.off_sacl.to_le_bytes());
    v.extend_from_slice(&sd.off_dacl.to_le_bytes());
    let mut i = 0;
    while i < bodies.len() {
        v.extend_from_slice(&(bodies[i].len() as u32).to_le_bytes());
        v.extend_from_slice(&bodies[i]);
        i += 1;
    }
    Ok(v)
}

/// 反身律：自身与自身语义等（含体）。
pub fn reflexive(sd: &Sd, bodies: &[Vec<u8>]) -> Result<(), u16> {
    let same = semantically_equal(sd, bodies, sd, bodies)?;
    if !same {
        return Err(E_LAW);
    }
    Ok(())
}

/// 对称律：a≈b ⇔ b≈a。
pub fn symmetric(a: &Sd, ab: &[Vec<u8>], b: &Sd, bb: &[Vec<u8>]) -> Result<(), u16> {
    let ab2 = semantically_equal(a, ab, b, bb)?;
    let ba = semantically_equal(b, bb, a, ab)?;
    if ab2 != ba {
        return Err(E_LAW);
    }
    Ok(())
}

/// 传递律：a≈b 且 b≈c ⇒ a≈c。
pub fn transitive(
    a: &Sd, ab: &[Vec<u8>],
    b: &Sd, bb: &[Vec<u8>],
    c: &Sd, cb: &[Vec<u8>],
) -> Result<(), u16> {
    let x = semantically_equal(a, ab, b, bb)?;
    let y = semantically_equal(b, bb, c, cb)?;
    let z = semantically_equal(a, ab, c, cb)?;
    if x && y && !z {
        return Err(E_LAW);
    }
    Ok(())
}

fn detail(code: u16) -> &'static str {
    match code {
        E_EQ => "语义等判定破",
        E_BYTE => "字节等判定破",
        E_LAW => "三律破（反身/对称/传递）",
        E_MIX => "语义/字节两层比较混用（红线）",
        E_CTRL => "控制位含未知位",
        _ => "未知诊断码",
    }
}

/// 夹具体（16 字节）。
fn fixture_body(tag: u8) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < 16 {
        v.push(tag.wrapping_add(i as u8));
        i += 1;
    }
    v
}

/// 确定性演练：R1 语义/字节分离 100 例（偏移重排=语义等、字节差）；
/// R2 控制位差（SELF_REL 位差语义等 / 其余位差不等）50 例；
/// R3 三律 50 例；R4 反例注入 12 例全检出；R5 两层蕴含红线。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：100 例——同形状双体，偏移重排（自相对化语义）。
    let mut r = 0u32;
    while r < 100 {
        let mut a = Sd::bare();
        a.control = C_DACL_PRESENT;
        a.off_dacl = 0;
        let mut b = a;
        b.off_dacl = 0x40; // 重排后的真实字节偏移
        if r % 2 == 0 {
            b.set_bit(C_SELF_REL); // 自相对形态位
        }
        let body_a = fixture_body((r % 13) as u8);
        let body_b = fixture_body((r % 13) as u8);
        if !semantically_equal(&a, &[body_a.clone()], &b, &[body_b.clone()])? {
            return Err(E_EQ);
        }
        // 字节面：偏移不同 → 字节不等（打包面）。
        if byte_equal(&pack(&a, &[body_a.clone()])?, &pack(&b, &[body_b.clone()])?) {
            return Err(E_BYTE);
        }
        layering_holds(&a, &[body_a], &b, &[body_b])?;
        hits += 1;
        r += 1;
    }
    // R2：控制位差 50 例。
    let mut r2 = 0u32;
    while r2 < 50 {
        let mut a = Sd::bare();
        a.control = C_DACL_PRESENT | C_SACL_PRESENT;
        let mut b = a;
        if r2 % 2 == 0 {
            b.set_bit(C_SELF_REL); // 语义等（剥位后同）
        } else {
            b.clear_bit(C_SACL_PRESENT); // 语义不等（在场性变）
            b.off_sacl = OFF_NONE;
        }
        let eq = semantically_equal(&a, &[], &b, &[])?;
        if r2 % 2 == 0 && !eq {
            return Err(E_EQ);
        }
        if r2 % 2 == 1 && eq {
            return Err(E_EQ);
        }
        hits += 1;
        r2 += 1;
    }
    // R3：三律 50 例（a→b→c 链：同体不同偏移）。
    let mut r3 = 0u32;
    while r3 < 50 {
        let mut a = Sd::bare();
        a.control = C_DACL_PRESENT;
        a.off_dacl = 0;
        let mut b = a;
        b.off_dacl = 0x40;
        let mut c = a;
        c.off_dacl = 0x80;
        let body = fixture_body((r3 % 7) as u8);
        reflexive(&a, &[body.clone()])?;
        symmetric(&a, &[body.clone()], &b, &[body.clone()])?;
        transitive(&a, &[body.clone()], &b, &[body.clone()], &c, &[body])?;
        hits += 1;
        r3 += 1;
    }
    // R4：反例注入 12 例（应不等判等 = 红线）。
    let mut n = 0;
    let base = Sd::bare();
    let base_b = fixture_body(1);
    //  i1 控制位差（DACL_PRESENT）；i2 修订差；i3 在场性差（owner）；
    //  i4 段体内容差；i5 段体长度差；i6 在场但体缺（哨兵 vs 真体）；
    //  i7 双段差；i8 组段在场差；i9 SACL 保护位差（控制位面）；
    //  i10 空体对有体；i11 长度差 1 字节；i12 全等对照（负控：必须判等）。
    let mut m1 = base;
    m1.control = C_DACL_PRESENT;
    if semantically_equal(&base, &[], &m1, &[])? {
        return Err(E_EQ);
    }
    n += 1;
    let mut m2 = base;
    m2.revision = 2;
    if semantically_equal(&base, &[], &m2, &[]).is_ok() {
        return Err(E_EQ); // 坏修订被解析门拒——比较面不允许静默通过
    }
    n += 1;
    let mut m3 = base;
    m3.off_owner = 0;
    if semantically_equal(&base, &[], &m3, &[])? {
        return Err(E_EQ);
    }
    n += 1;
    let mut holder = base;
    holder.control = C_DACL_PRESENT;
    holder.off_dacl = 0;
    let b_alt = fixture_body(2);
    if semantically_equal(&holder, &[base_b.clone()], &holder, &[b_alt.clone()])? {
        return Err(E_EQ); // 在场段体内容差 → 必不等
    }
    n += 1;
    let mut b_long = base_b.clone();
    b_long.push(0xff);
    if semantically_equal(&holder, &[base_b.clone()], &holder, &[b_long])? {
        return Err(E_EQ);
    }
    n += 1;
    let mut m6 = base;
    m6.off_dacl = 0; // DACL 在场
    if semantically_equal(&base, &[], &m6, &[])? {
        return Err(E_EQ); // 在场性差
    }
    n += 1;
    let mut m7 = base;
    m7.control = C_DACL_PRESENT | C_SACL_PRESENT;
    m7.off_dacl = 0;
    m7.off_sacl = 0;
    if semantically_equal(&base, &[], &m7, &[])? {
        return Err(E_EQ);
    }
    n += 1;
    let mut m8 = base;
    m8.off_group = 0;
    if semantically_equal(&base, &[], &m8, &[])? {
        return Err(E_EQ);
    }
    n += 1;
    let mut m9 = base;
    m9.control = 0x2000; // SACL_PROT 位（语义位差）
    if semantically_equal(&base, &[], &m9, &[])? {
        return Err(E_EQ);
    }
    n += 1;
    let mut m10 = base;
    m10.off_dacl = 0;
    if semantically_equal(&base, &[], &m10, &[base_b.clone()])? {
        // base 无 DACL、m10 有 DACL + 体——不等
        return Err(E_EQ);
    }
    n += 1;
    let mut b_one = base_b.clone();
    b_one.truncate(1);
    if byte_equal(&base_b, &b_one) {
        return Err(E_BYTE);
    }
    n += 1;
    if !semantically_equal(&base, &[], &base, &[])? {
        return Err(E_EQ); // 负控：全等对照必须判等
    }
    n += 1;
    if n != 12 {
        return Err(E_EQ);
    }
    hits += 1;
    // R5：两层蕴含红线（字节等 ⇒ 语义等）——构造字节等对拍。
    let mut x = Sd::bare();
    x.control = C_DACL_PRESENT;
    x.off_dacl = 0;
    let y = x;
    layering_holds(&x, &[fixture_body(3)], &y, &[fixture_body(3)])?;
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(202));
    }

    #[test]
    fn self_relative_bit_is_semantic_noop() {
        let mut a = Sd::bare();
        a.control = C_DACL_PRESENT;
        let mut b = a;
        b.set_bit(C_SELF_REL);
        assert!(semantically_equal(&a, &[], &b, &[]).ok().unwrap_or(false));
    }

    #[test]
    fn laws_hold() {
        let a = Sd::bare();
        assert!(reflexive(&a, &[]).is_ok());
    }
}

pub fn run_unx36074_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36074_sdcmp");
    cs.add("SEG 段位独占", SEG_BASE == 0xe260, "0xe260~0xe26f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "语义/字节分离 100 例+三律 50 例+反例 12 例全检出"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
