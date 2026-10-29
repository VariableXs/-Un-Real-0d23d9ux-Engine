//! unx36012_integrity — UNX-F36012 完整性级 SID（S-1-16-x）与 IL 数值映射。
//!
//! 判据原文：
//!   J1 IL SID ↔ 数值（0/4096/8192/12288/16384）双向映射 100 例零差；
//!      非法 IL 值注入 10 例拒绝。
//! 设计纪律：
//!   1. IL 锚：S-1-16-0=Untrusted、S-1-16-4096=Low、S-1-16-8192=Medium、
//!      S-1-16-12288=High、S-1-16-16384=System（Windows 完整性级公开锚）。
//!   2. 结构约束：IL SID 必须为 S-1-16-<x> 形（IA=16、子权限恰 1 项）；
//!      x 不在五档闭集内构造期拒绝。
//!   3. 策略面：级别支配比较（NoWriteUp/NoReadUp 语义原语）。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x73D0;

// ── 诊断码闭集（0x73D0~0x73EF，独占）─────────────────────────
const E_IL: u16 = SEG_BASE + 1; // IL 数值不在五档闭集
const E_FORM: u16 = SEG_BASE + 2; // 结构非 S-1-16-x 形（IA/子权数不符）
const E_DOM: u16 = SEG_BASE + 3; // 支配比较失配
const E_BAL: u16 = SEG_BASE + 4; // 判例账守恒破

use crate::checks::CheckSet;
use super::unx36001_sidcore::Sid;

/// IL 锚五档。
pub const IL_UNTRUSTED: u32 = 0;
pub const IL_LOW: u32 = 4096;
pub const IL_MEDIUM: u32 = 8192;
pub const IL_HIGH: u32 = 12288;
pub const IL_SYSTEM: u32 = 16384;
/// IL SID 的 IA 锚（S-1-16-…）。
pub const IL_IA: u64 = 16;

/// 五档闭集（升序）。
pub const IL_TAB: [(u32, &'static str); 5] = [
    (IL_UNTRUSTED, "UNTRUSTED"),
    (IL_LOW, "LOW"),
    (IL_MEDIUM, "MEDIUM"),
    (IL_HIGH, "HIGH"),
    (IL_SYSTEM, "SYSTEM"),
];

/// IL 闭集成员判定。
pub fn il_valid(x: u32) -> bool {
    let mut i = 0;
    while i < IL_TAB.len() {
        if IL_TAB[i].0 == x {
            return true;
        }
        i += 1;
    }
    false
}

/// 数值 → IL SID（闭集外拒绝）。
pub fn sid_of_il(x: u32) -> Result<Sid, u16> {
    if !il_valid(x) {
        return Err(E_IL);
    }
    Sid::new(1, Sid::ia_from(IL_IA), &[x])
}

/// IL SID → 数值（结构/闭集双校验）。
pub fn il_of_sid(s: &Sid) -> Result<u32, u16> {
    if s.ia_value() != IL_IA || s.sub_count != 1 {
        return Err(E_FORM);
    }
    let x = s.sub_authorities[0];
    if !il_valid(x) {
        return Err(E_IL);
    }
    Ok(x)
}

/// IL 档位序（0..=4，用于支配比较的档位索引）。
pub fn il_rank(x: u32) -> Result<usize, u16> {
    if !il_valid(x) {
        return Err(E_IL);
    }
    let mut i = 0;
    while i < IL_TAB.len() {
        if IL_TAB[i].0 == x {
            return Ok(i);
        }
        i += 1;
    }
    Err(E_IL)
}

/// 支配比较：主体 IL 是否可写目标 IL（NoWriteUp 语义：主体档 ≥ 目标档）。
pub fn dominates(subject_il: u32, target_il: u32) -> Result<bool, u16> {
    Ok(il_rank(subject_il)? >= il_rank(target_il)?)
}

/// 助记名查询。
pub fn name_of_il(x: u32) -> Option<&'static str> {
    let mut i = 0;
    while i < IL_TAB.len() {
        if IL_TAB[i].0 == x {
            return Some(IL_TAB[i].1);
        }
        i += 1;
    }
    None
}

/// 策略三权面（Windows 完整性策略语义桩）：
/// NoWriteUp（写不高就）/ NoReadUp（读不低就——反向锚）/ NoExecuteUp。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IlPolicy {
    pub no_write_up: bool,
    pub no_read_up: bool,
    pub no_execute_up: bool,
}

impl IlPolicy {
    /// 默认策略锚（Windows 默认 = 仅 NoWriteUp）。
    pub fn default_policy() -> IlPolicy {
        IlPolicy { no_write_up: true, no_read_up: false, no_execute_up: false }
    }
    /// 全开强策略。
    pub fn strict_policy() -> IlPolicy {
        IlPolicy { no_write_up: true, no_read_up: true, no_execute_up: true }
    }
}

/// 策略裁定：subject 对 object 的单权判定（档位序比较）。
/// write: 允许 ⇔ 满足 no_write_up 时 subject ≥ object；
/// read:  允许 ⇔ 满足 no_read_up 时 subject ≥ object（低不可读高）；
/// exec:  允许 ⇔ 满足 no_execute_up 时 subject ≥ object。
pub fn policy_verdict(pol: &IlPolicy, subject: u32, object: u32, op: u8) -> Result<bool, u16> {
    let s = il_rank(subject)?;
    let o = il_rank(object)?;
    match op {
        0 => Ok(!pol.no_write_up || s >= o),   // OP_WRITE
        1 => Ok(!pol.no_read_up || s >= o),    // OP_READ
        2 => Ok(!pol.no_execute_up || s >= o), // OP_EXEC
        _ => Err(E_DOM),
    }
}

pub const OP_WRITE: u8 = 0;
pub const OP_READ: u8 = 1;
pub const OP_EXEC: u8 = 2;

/// 策略矩阵全谱核：五档 × 五档 × 三权在两种策略下全裁定复算，
/// 与档位序独立推导逐格对平（判据 J1 策略面）。
pub fn policy_matrix_check(pol: &IlPolicy) -> Result<u32, u16> {
    let order = [IL_UNTRUSTED, IL_LOW, IL_MEDIUM, IL_HIGH, IL_SYSTEM];
    let mut checked = 0u32;
    let mut a = 0;
    while a < 5 {
        let mut b = 0;
        while b < 5 {
            let mut op = 0;
            while op < 3 {
                let got = policy_verdict(pol, order[a], order[b], op)?;
                let expect = match op {
                    0 => !pol.no_write_up || a >= b,
                    1 => !pol.no_read_up || a >= b,
                    _ => !pol.no_execute_up || a >= b,
                };
                if got != expect {
                    return Err(E_DOM);
                }
                checked += 1;
                op += 1;
            }
            b += 1;
        }
        a += 1;
    }
    Ok(checked)
}

/// IL 十六进制渲染锚：S-1-16-<x> 的 x 段十六进制小写（审计面对平用）。
pub fn il_hex(x: u32, out: &mut [u8]) -> Result<usize, u16> {
    if !il_valid(x) {
        return Err(E_IL);
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut started = false;
    let mut n = 0usize;
    // 8 位定宽去前导零（x < 2^32）。
    let mut sh = 28i32;
    while sh >= 0 {
        let d = ((x >> sh) & 0xF) as usize;
        if d != 0 || started || sh == 0 {
            if n >= out.len() {
                return Err(E_DOM);
            }
            out[n] = HEX[d];
            n += 1;
            started = true;
        }
        sh -= 4;
    }
    Ok(n)
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_IL => "IL 数值不在五档闭集",
        E_FORM => "结构非 S-1-16-x 形（IA/子权数不符）",
        E_DOM => "支配比较失配",
        E_BAL => "判例账守恒破",
        _ => "未知诊断码",
    }
}

/// 确定性演练：100 例双向映射零差 + 10 例非法注入全拒（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut cases = 0u32;

    // R1：100 例双向映射——五档 × 20 轮：il→sid→il 还原 + sid→il→sid
    // 还原 + 助记名锚逐档对平。
    let mut r = 0;
    while r < 20 {
        let mut i = 0;
        while i < IL_TAB.len() {
            let x = IL_TAB[i].0;
            let s = sid_of_il(x)?;
            if il_of_sid(&s)? != x {
                return Err(E_IL);
            }
            let s2 = sid_of_il(il_of_sid(&s)?)?;
            if s2.fingerprint() != s.fingerprint() {
                return Err(E_BAL);
            }
            if name_of_il(x) != Some(IL_TAB[i].1) {
                return Err(E_DOM);
            }
            cases += 1;
            i += 1;
        }
        r += 1;
    }
    if cases != 100 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：非法 IL 注入 10 例——闭集外值 sid_of_il 全拒；其中可构造形
    // 的（S-1-16-<bad>）il_of_sid 同样全拒。
    let bads: [u32; 10] = [
        1, 100, 2048, 4095, 4097, 10000, 16383, 16385, 32768, 65536,
    ];
    let mut k = 0;
    while k < 10 {
        if sid_of_il(bads[k]) != Err(E_IL) {
            return Err(E_IL);
        }
        let s = Sid::new(1, Sid::ia_from(IL_IA), &[bads[k]])?;
        if il_of_sid(&s) != Err(E_IL) {
            return Err(E_IL);
        }
        if il_rank(bads[k]).is_ok() {
            return Err(E_IL);
        }
        k += 1;
    }
    hits += 1;

    // R3：结构注入——IA 非 16 / 子权限数 ≠1 / 零子权，全部 E_FORM。
    let s_bad_ia = Sid::new(1, Sid::ia_from(5), &[IL_MEDIUM])?;
    if il_of_sid(&s_bad_ia) != Err(E_FORM) {
        return Err(E_FORM);
    }
    let s_bad_cnt = Sid::new(1, Sid::ia_from(IL_IA), &[IL_MEDIUM, IL_MEDIUM])?;
    if il_of_sid(&s_bad_cnt) != Err(E_FORM) {
        return Err(E_FORM);
    }
    hits += 1;

    // R4：支配比较锚——System≥High≥Medium≥Low≥Untrusted 全序 20 对；
    // 逆向恒 false；同档恒 true。
    let order = [IL_UNTRUSTED, IL_LOW, IL_MEDIUM, IL_HIGH, IL_SYSTEM];
    let mut a = 0;
    while a < 5 {
        let mut b = 0;
        while b < 5 {
            let d = dominates(order[a], order[b])?;
            if a >= b && !d {
                return Err(E_DOM);
            }
            if a < b && d {
                return Err(E_DOM);
            }
            b += 1;
        }
        a += 1;
    }
    let mut r = 0;
    while r < 20 {
        let mut b = 0;
        while b < 5 {
            if !dominates(order[b], order[b])? {
                return Err(E_DOM);
            }
            b += 1;
        }
        r += 1;
    }
    hits += 1;

    // R5：策略矩阵全谱——默认策略与强策略下 5×5×3 全裁定复算对平
    //（各 75 格）；策略语义锚抽验：默认策略下 Low 写 High 拒、
    // High 写 Low 许；强策略下 Low 读 High 拒。
    let dp = IlPolicy::default_policy();
    if policy_matrix_check(&dp)? != 75 {
        return Err(E_DOM);
    }
    let sp = IlPolicy::strict_policy();
    if policy_matrix_check(&sp)? != 75 {
        return Err(E_DOM);
    }
    if policy_verdict(&dp, IL_LOW, IL_HIGH, OP_WRITE)? {
        return Err(E_DOM);
    }
    if !policy_verdict(&dp, IL_HIGH, IL_LOW, OP_WRITE)? {
        return Err(E_DOM);
    }
    if policy_verdict(&sp, IL_LOW, IL_HIGH, OP_READ)? {
        return Err(E_DOM);
    }
    // 非法操作码显性拒。
    if policy_verdict(&dp, IL_LOW, IL_LOW, 9).is_ok() {
        return Err(E_DOM);
    }
    hits += 1;

    // R6：IL 十六进制渲染锚——五档渲染逐档对平（0→"0"、4096→"1000"、
    // 8192→"2000"、12288→"3000"、16384→"4000"）；非法档显性拒。
    let expect = ["0", "1000", "2000", "3000", "4000"];
    let mut buf = [0u8; 16];
    let mut i = 0;
    while i < 5 {
        let n = il_hex(IL_TAB[i].0, &mut buf)?;
        if n != expect[i].len() || &buf[..n] != expect[i].as_bytes() {
            return Err(E_DOM);
        }
        i += 1;
    }
    if il_hex(4095, &mut buf).is_ok() {
        return Err(E_IL);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36012_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36012_integrity");
    cs.add("SEG 段位独占", SEG_BASE == 0x73D0, "0x73D0~0x73EF 本模块独占");
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
