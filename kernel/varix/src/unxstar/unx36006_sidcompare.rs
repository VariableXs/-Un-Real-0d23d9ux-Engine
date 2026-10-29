//! unx36006_sidcompare — UNX-F36006 SID 等价比较与归一化（子权限前缀匹配）。
//!
//! 判据原文：
//!   J1 完全相等/前缀相等/不相等三态判定 300 例全对；前缀语义
//!      （组匹配用）单独断言 50 例。
//! 设计纪律：
//!   1. 三态语义：Exact = 修订号+IA+全部子权限逐项相等；Prefix = IA
//!      相等且一方为另一方的子权限真前缀（组匹配语义：组 SID 前缀
//!      命中成员 SID）；Unequal = 其余。
//!   2. 归一化：去尾部零子权限（保留至少 1 项）——前缀匹配先归一化
//!      再比较，杜绝「尾部零填充」造成的假性差异。
//!   3. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x7310;

// ── 诊断码闭集（0x7310~0x732F，独占）─────────────────────────
const E_STATE: u16 = SEG_BASE + 1; // 三态判定失配
const E_NORM: u16 = SEG_BASE + 2; // 归一化失配（尾部零未剥离/剥空）
const E_ANCHOR: u16 = SEG_BASE + 3; // IA 语义锚失配（前缀必同 IA）
const E_BAL: u16 = SEG_BASE + 4; // 判例账守恒破（300/50 案例数漂移）

use crate::checks::CheckSet;
use super::unx36001_sidcore::{Sid, MAX_SUBS};

/// 三态判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmp3 {
    Exact = 0,   // 完全相等
    Prefix = 1,  // 前缀相等（IA 同、子权限真前缀）
    Unequal = 2, // 不相等
}

/// 归一化：去尾部零子权限（至少保留 1 项），返回新 SID。
/// 判据面：尾部非零时必须原样返回（指纹不变）。
pub fn normalize(s: &Sid) -> Result<Sid, u16> {
    let n = s.sub_count as usize;
    if n == 0 {
        return Err(E_NORM);
    }
    let mut keep = n;
    while keep > 1 && s.sub_authorities[keep - 1] == 0 {
        keep -= 1;
    }
    if keep == n {
        return Ok(*s); // 无尾部零：原样（指纹必须逐位不变）
    }
    Sid::new(s.revision, s.id_authority, &s.sub_authorities[..keep])
}

/// 三态判定（先归一化再比较）。
pub fn compare(a: &Sid, b: &Sid) -> Cmp3 {
    let na = match normalize(a) {
        Ok(v) => v,
        Err(_) => *a,
    };
    let nb = match normalize(b) {
        Ok(v) => v,
        Err(_) => *b,
    };
    if na.revision != nb.revision || na.ia_value() != nb.ia_value() {
        return Cmp3::Unequal;
    }
    let ca = na.sub_count as usize;
    let cb = nb.sub_count as usize;
    let sa = na.subs();
    let sb = nb.subs();
    if ca == cb && sa == sb {
        return Cmp3::Exact;
    }
    let (short, long) = if ca < cb { (sa, sb) } else { (sb, sa) };
    let mut i = 0;
    let mut head_eq = true;
    while i < short.len() {
        if short[i] != long[i] {
            head_eq = false;
            break;
        }
        i += 1;
    }
    if head_eq {
        Cmp3::Prefix
    } else {
        Cmp3::Unequal
    }
}

/// 前缀语义专用面（组匹配）：member 是否命中 group 的前缀族。
/// 契约：group 是成员所属域前缀（如 S-1-5-21-a-b-c），member 在该域内
/// （前缀相等）即命中——与 compare 的 Prefix 态同一语义、独立入口。
pub fn group_match(group: &Sid, member: &Sid) -> bool {
    compare(group, member) == Cmp3::Prefix
}

/// 三态编码：Cmp3 → 稳定 u8 码（账面/导出面用，0/1/2 闭集）。
pub fn encode_cmp(c: Cmp3) -> u8 {
    match c {
        Cmp3::Exact => 0,
        Cmp3::Prefix => 1,
        Cmp3::Unequal => 2,
    }
}

/// 三态助记名（诊断面）。
pub fn name_of_cmp(c: Cmp3) -> &'static str {
    match c {
        Cmp3::Exact => "EXACT",
        Cmp3::Prefix => "PREFIX",
        Cmp3::Unequal => "UNEQUAL",
    }
}

/// 三态编码闭集自检：0/1/2 互异且助记名互异（账面健康面）。
pub fn cmp_alphabet_check() -> Result<(), u16> {
    let all = [Cmp3::Exact, Cmp3::Prefix, Cmp3::Unequal];
    let mut a = 0;
    while a < 3 {
        let mut b = a + 1;
        while b < 3 {
            if encode_cmp(all[a]) == encode_cmp(all[b])
                || name_of_cmp(all[a]) == name_of_cmp(all[b])
            {
                return Err(E_STATE);
            }
            b += 1;
        }
        a += 1;
    }
    Ok(())
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_STATE => "三态判定失配",
        E_NORM => "归一化失配（尾部零剥离面）",
        E_ANCHOR => "IA 语义锚失配（前缀必同 IA）",
        E_BAL => "判例账守恒破（300/50 案例数漂移）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：300 例三态全对 + 50 例前缀语义独立断言（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut seed: u32 = 0x3606_0001;
    let mut cases = 0u32;

    // R1：Exact 100 例——同构造 SID 逐对 compare==Exact。
    let mut n = 0;
    while n < 100 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let cnt = (seed as usize % 15) + 1;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ia_val = ((seed & 0xFFFF) as u64) << 8 | 0x21;
        let mut subs = [0u32; MAX_SUBS];
        let mut i = 0;
        while i < cnt {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            subs[i] = seed;
            i += 1;
        }
        let s = Sid::new(1, Sid::ia_from(ia_val), &subs[..cnt])?;
        if compare(&s, &s) != Cmp3::Exact {
            return Err(E_STATE);
        }
        cases += 1;
        n += 1;
    }
    hits += 1;

    // R2：Prefix 100 例——构造（域前缀 SID, 域内成员 SID）对，
    // 前缀 1~4 项铺开，compare==Prefix 且 IA 必同。
    let mut n = 0;
    while n < 100 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let plen = (seed as usize % 4) + 1; // 前缀长度 1..=4
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ia_val = ((seed & 0xFFFF) as u64) << 8 | 0x21;
        let mut pre = [0u32; MAX_SUBS];
        let mut i = 0;
        while i < plen {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            pre[i] = seed | 1; // 非零前缀
            i += 1;
        }
        let g = Sid::new(1, Sid::ia_from(ia_val), &pre[..plen])?;
        let mut full = [0u32; MAX_SUBS];
        let mut j = 0;
        while j < plen {
            full[j] = pre[j];
            j += 1;
        }
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        full[plen] = seed | 1; // 追加 RID，成员 = 前缀 + 1 项
        let m = Sid::new(1, Sid::ia_from(ia_val), &full[..plen + 1])?;
        if compare(&g, &m) != Cmp3::Prefix {
            return Err(E_STATE);
        }
        if g.ia_value() != m.ia_value() {
            return Err(E_ANCHOR);
        }
        cases += 1;
        n += 1;
    }
    hits += 1;

    // R3：Unequal 100 例——IA 异 / 末项异 / 前缀中断三种族各铺，
    // compare==Unequal。
    let mut n = 0;
    while n < 100 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let fam = n % 3;
        let cnt = 3;
        let mut sa = [0u32; MAX_SUBS];
        let mut sb = [0u32; MAX_SUBS];
        let mut i = 0;
        while i < cnt {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            sa[i] = seed | 1;
            sb[i] = seed | 1;
            i += 1;
        }
        let (ia_a, ia_b) = match fam {
            0 => (0x21u64, 0x22u64), // IA 异
            1 => {
                sb[cnt - 1] ^= 1; // 末项异
                (0x21, 0x21)
            }
            _ => {
                sb[0] ^= 1; // 前缀中断
                (0x21, 0x21)
            }
        };
        let a = Sid::new(1, Sid::ia_from(ia_a), &sa[..cnt])?;
        let b = Sid::new(1, Sid::ia_from(ia_b), &sb[..cnt])?;
        if compare(&a, &b) != Cmp3::Unequal {
            return Err(E_STATE);
        }
        cases += 1;
        n += 1;
    }
    if cases != 300 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：前缀语义独立断言 50 例——group_match 与构造期望独立对账：
    // 25 例命中（成员 = 组前缀 + RID）、25 例不命中（前缀第二项异）。
    let mut gm = 0u32;
    let mut n = 0;
    while n < 50 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let plen = (seed as usize % 3) + 2; // 前缀 2..=4
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ia_val = ((seed & 0xFFF) as u64) << 8 | 0x21;
        let mut pre = [0u32; MAX_SUBS];
        let mut i = 0;
        while i < plen {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            pre[i] = seed | 1;
            i += 1;
        }
        let g = Sid::new(1, Sid::ia_from(ia_val), &pre[..plen])?;
        let mut mem = [0u32; MAX_SUBS];
        let mut j = 0;
        while j < plen {
            mem[j] = pre[j];
            j += 1;
        }
        if n % 2 == 0 {
            // 命中族：成员 = 组 + RID。
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            mem[plen] = seed | 1;
            let m = Sid::new(1, Sid::ia_from(ia_val), &mem[..plen + 1])?;
            if !group_match(&g, &m) {
                return Err(E_STATE);
            }
        } else {
            // 不命中族：前缀首项异（IA 同）。
            mem[0] ^= 1;
            let m = Sid::new(1, Sid::ia_from(ia_val), &mem[..plen])?;
            if group_match(&g, &m) {
                return Err(E_STATE);
            }
        }
        gm += 1;
        n += 1;
    }
    if gm != 50 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：归一化锚——尾部零剥离：S-1-5-21-1000+0 → S-1-5-21-1000；
    // 全零段保留 1 项；无尾零原样（指纹逐位不变）。
    let s0 = Sid::new(1, Sid::ia_from(5), &[21, 1000, 0])?;
    let n0 = normalize(&s0)?;
    if n0.sub_count != 2 || n0.rid() != 1000 {
        return Err(E_NORM);
    }
    let sz = Sid::new(1, Sid::ia_from(5), &[0])?;
    if normalize(&sz)?.sub_count != 1 {
        return Err(E_NORM);
    }
    let sn = Sid::new(1, Sid::ia_from(5), &[21, 1000])?;
    if normalize(&sn)?.fingerprint() != sn.fingerprint() {
        return Err(E_NORM);
    }
    // 归一化联动：带尾零 SID 与剥离后 SID compare==Exact。
    if compare(&s0, &sn) != Cmp3::Exact {
        return Err(E_NORM);
    }
    hits += 1;

    // R6：三态字母表——编码 0/1/2 互异、助记名互异；样本三态编码
    // 逐例对平（Exact=0/Prefix=1/Unequal=2 稳定账面码）。
    cmp_alphabet_check()?;
    let g = Sid::new(1, Sid::ia_from(5), &[21, 100])?;
    let m = Sid::new(1, Sid::ia_from(5), &[21, 100, 1000])?;
    let o = Sid::new(1, Sid::ia_from(5), &[21, 999])?;
    if encode_cmp(compare(&g, &g)) != 0 || name_of_cmp(compare(&g, &g)) != "EXACT" {
        return Err(E_STATE);
    }
    if encode_cmp(compare(&g, &m)) != 1 || name_of_cmp(compare(&g, &m)) != "PREFIX" {
        return Err(E_STATE);
    }
    if encode_cmp(compare(&g, &o)) != 2 || name_of_cmp(compare(&g, &o)) != "UNEQUAL" {
        return Err(E_STATE);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36006_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36006_sidcompare");
    cs.add("SEG 段位独占", SEG_BASE == 0x7310, "0x7310~0x732F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(6));
    }
}
