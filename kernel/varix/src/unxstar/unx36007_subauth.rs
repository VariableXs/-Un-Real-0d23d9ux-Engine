//! unx36007_subauth — UNX-F36007 SID 组成子身份（SubAuthority 前缀族）
//! 提取原语。
//!
//! 判据原文：
//!   J1 从 6 段 SID 提取域前缀 100 例全对；短 SID（无子权限）提取注入
//!      10 例返回空而非崩溃。
//! 设计纪律：
//!   1. 域前缀语义：域前缀 = 子权限数组去掉末项 RID 的前缀切片
//!      （S-1-5-21-a-b-c-rid → [21, a, b, c]）。仅含 RID 的短 SID
//!      前缀为空切片——返回空而非崩溃（显性 Empty 态）。
//!   2. 族键提取：取前 depth 项子权限作族键（depth ≤ 计数-1 守卫），
//!      族键指纹 FNV-1a 64 确定性可复算。
//!   3. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x7330;

// ── 诊断码闭集（0x7330~0x734F，独占）─────────────────────────
const E_DEPTH: u16 = SEG_BASE + 1; // 族键深度越界（≥ 计数，会吞 RID）
const E_ANCHOR: u16 = SEG_BASE + 2; // 提取失配（前缀与构造期望漂移）
const E_EMPTY_OK: u16 = SEG_BASE + 3; // 短 SID 应返回空却返回了非空
const E_FP: u16 = SEG_BASE + 4; // 族键指纹复算失配
const E_BAL: u16 = SEG_BASE + 5; // 判例账守恒破（100/10 案例数漂移）

use crate::checks::CheckSet;
use super::unx36001_sidcore::Sid;

/// 域前缀提取：去掉末项 RID 的子权限切片（可为空——短 SID）。
pub fn domain_prefix(s: &Sid) -> &[u32] {
    let n = s.sub_count as usize;
    if n <= 1 {
        // 短 SID：仅 RID，无域前缀——空切片显性返回，不崩溃。
        return &[];
    }
    &s.sub_authorities[..n - 1]
}

/// 族键提取：前 depth 项子权限（depth 0 合法=空键；depth ≥ 计数拒绝）。
pub fn family_key(s: &Sid, depth: usize) -> Result<&[u32], u16> {
    let n = s.sub_count as usize;
    if depth >= n {
        return Err(E_DEPTH);
    }
    Ok(&s.sub_authorities[..depth])
}

/// 族键指纹：FNV-1a 64（逐项小端四字节铺开，确定性）。
pub fn family_fingerprint(subs: &[u32]) -> u64 {
    let mut h: u64 = 0x811C_9DC5;
    let mut i = 0;
    while i < subs.len() {
        let b = subs[i].to_le_bytes();
        let mut j = 0;
        while j < 4 {
            h = (h ^ b[j] as u64).wrapping_mul(0x0100_0193);
            j += 1;
        }
        i += 1;
    }
    h
}

/// 「6 段 SID」锚：S-1-5-21-a-b（字段数 6：S/1/5/21/a/b）→ 域前缀
/// 为 [21, a]（去掉末项 b=RID）。语义锚函数（判据 J1 的 6 段样本族）。
pub fn six_field_prefix(s: &Sid) -> Result<&[u32], u16> {
    // 6 段 = S + rev + IA + 2 子权限 ⇒ sub_count == 2。
    if s.sub_count != 2 {
        return Err(E_ANCHOR);
    }
    Ok(&s.sub_authorities[..1])
}

/// 族册行：族键指纹 + 成员计数 + 代表成员 RID。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FamilyRow {
    pub key_fp: u64,   // 族键指纹（前 depth 项子权限）
    pub depth: usize,  // 族键深度
    pub members: u32,  // 成员计数
    pub first_rid: u32, // 首个成员 RID（代表面）
}

/// 族册：按族键归并 SID 群（前缀族组织面，ACL 枚举底座）。
/// 定容 16 族 × 每 16 成员；溢出显性拒（不静默丢族）。
pub struct FamilyBook {
    keys: [Option<(u64, usize)>; 16],
    counts: [u32; 16],
    first_rid: [u32; 16],
    pub enrolled: u32,
    pub rejected: u32,
    pub req_total: u32,
}

impl FamilyBook {
    pub fn new() -> FamilyBook {
        FamilyBook {
            keys: [None; 16],
            counts: [0; 16],
            first_rid: [0; 16],
            enrolled: 0,
            rejected: 0,
            req_total: 0,
        }
    }

    /// 入族：按前 depth 项子权限归并（depth 由调用方声明，≤计数-1）。
    pub fn enroll(&mut self, s: &Sid, depth: usize) -> Result<u32, u16> {
        self.req_total += 1;
        let key = match family_key(s, depth) {
            Ok(k) => k,
            Err(_) => {
                self.rejected += 1; // 越界请求入拒绝账（不灭失）
                return Err(E_DEPTH);
            }
        };
        let fp = family_fingerprint(key);
        let mut i = 0;
        while i < 16 {
            match self.keys[i] {
                Some((f, d)) if f == fp && d == depth => {
                    self.counts[i] += 1;
                    self.enrolled += 1;
                    return Ok(self.counts[i]);
                }
                None => {
                    self.keys[i] = Some((fp, depth));
                    self.counts[i] = 1;
                    self.first_rid[i] = s.rid();
                    self.enrolled += 1;
                    return Ok(1);
                }
                _ => {}
            }
            i += 1;
        }
        self.rejected += 1;
        Err(E_DEPTH)
    }

    /// 族计数查询（指纹）。
    pub fn count_of(&self, fp: u64) -> Option<u32> {
        let mut i = 0;
        while i < 16 {
            if let Some((f, _)) = self.keys[i] {
                if f == fp {
                    return Some(self.counts[i]);
                }
            }
            i += 1;
        }
        None
    }

    /// 活族数。
    pub fn live_families(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < 16 {
            if self.keys[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 行导出（收口面）。
    pub fn row(&self, i: usize) -> Option<FamilyRow> {
        if i >= 16 {
            return None;
        }
        let (f, d) = self.keys[i]?;
        Some(FamilyRow {
            key_fp: f,
            depth: d,
            members: self.counts[i],
            first_rid: self.first_rid[i],
        })
    }

    /// 守恒：入族 + 拒绝 == 总请求。
    pub fn conserve(&self) -> bool {
        self.enrolled + self.rejected == self.req_total
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_DEPTH => "族键深度越界（depth ≥ 计数）",
        E_ANCHOR => "提取失配（前缀与构造期望漂移）",
        E_EMPTY_OK => "短 SID 应返回空却返回非空",
        E_FP => "族键指纹复算失配",
        E_BAL => "判例账守恒破（100/10 案例数漂移）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：100 例域前缀提取全对 + 10 例短 SID 空返回（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut seed: u32 = 0x3607_0001;
    let mut cases = 0u32;

    // R1：100 例域前缀提取——计数 2..=15 循环铺，前缀必须逐项等于
    // 构造期望（subs[..cnt-1]）。
    let mut n = 0;
    while n < 100 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let cnt = (n % 14) + 2; // 2..=15
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ia_val = ((seed & 0xFFFF) as u64) << 8 | 0x21;
        let mut subs = [0u32; 15];
        let mut i = 0;
        while i < cnt {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            subs[i] = seed | 1;
            i += 1;
        }
        let s = Sid::new(1, Sid::ia_from(ia_val), &subs[..cnt])?;
        let p = domain_prefix(&s);
        if p.len() != cnt - 1 {
            return Err(E_ANCHOR);
        }
        let mut i = 0;
        while i < cnt - 1 {
            if p[i] != subs[i] {
                return Err(E_ANCHOR);
            }
            i += 1;
        }
        cases += 1;
        n += 1;
    }
    if cases != 100 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：短 SID 注入 10 例——仅 RID（计数 1），域前缀必须为空切片。
    let mut n = 0;
    while n < 10 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let s = Sid::new(1, Sid::ia_from(5), &[seed | 1])?;
        let p = domain_prefix(&s);
        if !p.is_empty() {
            return Err(E_EMPTY_OK);
        }
        n += 1;
    }
    hits += 1;

    // R3：族键提取——depth 0..=cnt-1 全档合法、cnt 起全拒；指纹复算
    // 逐项一致。
    let s = Sid::new(1, Sid::ia_from(5), &[21, 100, 1000, 5000])?;
    let mut d = 0;
    while d < 4 {
        let k = family_key(&s, d)?;
        if k.len() != d {
            return Err(E_DEPTH);
        }
        let mut i = 0;
        while i < d {
            if k[i] != [21u32, 100, 1000, 5000][i] {
                return Err(E_ANCHOR);
            }
            i += 1;
        }
        let fp1 = family_fingerprint(k);
        let fp2 = family_fingerprint(&s.sub_authorities[..d]);
        if fp1 != fp2 || fp1 == 0 {
            return Err(E_FP);
        }
        d += 1;
    }
    if family_key(&s, 4).is_ok() {
        return Err(E_DEPTH);
    }
    if family_key(&s, 5).is_ok() {
        return Err(E_DEPTH);
    }
    hits += 1;

    // R4：6 段锚——S-1-5-21-a（计数 2）前缀 [21]；非 6 段显性拒。
    let six = Sid::new(1, Sid::ia_from(5), &[21, 7777])?;
    let p = six_field_prefix(&six)?;
    if p != &[21u32] {
        return Err(E_ANCHOR);
    }
    let not6 = Sid::new(1, Sid::ia_from(5), &[21, 100, 1000])?;
    if six_field_prefix(&not6).is_ok() {
        return Err(E_ANCHOR);
    }
    // 短 SID（计数 1）对 6 段锚同样显性拒。
    let one = Sid::new(1, Sid::ia_from(5), &[1000])?;
    if six_field_prefix(&one).is_ok() {
        return Err(E_ANCHOR);
    }
    hits += 1;

    // R5：族册面——同族键（深度 2）多成员归并计数、异族分桶、
    // depth 越界显性拒、守恒对平。
    let mut fb = FamilyBook::new();
    // 族 A：键 [21, 100]，成员 RID 1000..1004（5 成员）。
    let mut m = 0u32;
    while m < 5 {
        let s = Sid::new(1, Sid::ia_from(5), &[21, 100, 1000 + m])?;
        if fb.enroll(&s, 2)? != m + 1 {
            return Err(E_ANCHOR);
        }
        m += 1;
    }
    // 族 B：键 [21, 200]，成员 2 个。
    let mut m = 0u32;
    while m < 2 {
        let s = Sid::new(1, Sid::ia_from(5), &[21, 200, 2000 + m])?;
        fb.enroll(&s, 2)?;
        m += 1;
    }
    // 族键指纹查询：族 A 计 5、族 B 计 2、未知族 None。
    let fp_a = family_fingerprint(&[21, 100]);
    let fp_b = family_fingerprint(&[21, 200]);
    if fb.count_of(fp_a) != Some(5) || fb.count_of(fp_b) != Some(2) {
        return Err(E_ANCHOR);
    }
    if fb.count_of(family_fingerprint(&[21, 999])).is_some() {
        return Err(E_ANCHOR);
    }
    if fb.live_families() != 2 {
        return Err(E_BAL);
    }
    // depth 越界（= 计数）显性拒不入账。
    let s_bad = Sid::new(1, Sid::ia_from(5), &[21, 300, 3000])?;
    if fb.enroll(&s_bad, 3).is_ok() {
        return Err(E_DEPTH);
    }
    // 行导出对账：族 A 首成员 RID = 1000。
    let ra = fb.row(0).ok_or(E_BAL)?;
    if ra.members != 5 || ra.depth != 2 || ra.first_rid != 1000 {
        return Err(E_BAL);
    }
    if !fb.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36007_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36007_subauth");
    cs.add("SEG 段位独占", SEG_BASE == 0x7330, "0x7330~0x734F 本模块独占");
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
        assert_eq!(drill(), Ok(5));
    }
}
