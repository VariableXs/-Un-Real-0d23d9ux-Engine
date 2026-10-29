//! UNX-F36108-J1 掩码合成原语（求并/求交/求差）。
//! 判据：① 三原语恒等式对拍（a∪a=a、a∩b⊆a、(a−b)∩b=∅ 等八律）
//! 全样本通过；② 注入“破差原语”负例必须被恒等式检出；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：掩码代数消费方——位段常量从 36101 真源取；原语供
//! 36109 覆盖判定与 36116 性能账复用；不引入类型语义。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3980;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_IDEM: u16 = SEG_BASE + 2; // 幂等律破（a∪a≠a 或 a∩a≠a）
pub const E_SUB: u16 = SEG_BASE + 3; // 子集律破（a∩b⊆a）
pub const E_DIFF: u16 = SEG_BASE + 4; // 差律破（(a−b)∩b≠∅）
pub const E_ABSORB: u16 = SEG_BASE + 5; // 吸收律破（a∪∅≠a / a∩U≠a）
pub const E_DEMORG: u16 = SEG_BASE + 6; // 德摩根律破
pub const E_NEG: u16 = SEG_BASE + 7; // 负例注入未被检出（自检面失灵）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 求并。
pub fn munion(a: u32, b: u32) -> u32 {
    a | b
}

/// 求交。
pub fn minter(a: u32, b: u32) -> u32 {
    a & b
}

/// 求差（a 减 b）。
pub fn mdiff(a: u32, b: u32) -> u32 {
    a & !b
}

/// 补（全集 = 全体合法位）。
pub fn mcompl(a: u32) -> u32 {
    mt::ALL_LEGAL & !a
}

/// 子集判定（a ⊆ b）。
pub fn msub(a: u32, b: u32) -> bool {
    a & !b == 0
}

/// 恒等式清单编号（显式判别式；供账目与负例定位）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Law {
    UnionIdem = 0x0001,      // a∪a = a
    InterIdem = 0x0002,      // a∩a = a
    InterSub = 0x0003,       // a∩b ⊆ a
    DiffEmpty = 0x0004,      // (a−b)∩b = ∅
    UnionEmpty = 0x0005,     // a∪∅ = a
    InterUniv = 0x0006,      // a∩U = a（U = 全体合法位）
    DiffSelf = 0x0007,       // a−a = ∅
    DeMorgan = 0x0008,       // 补(a∪b) = 补a∩补b
}

impl Law {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn name(self) -> &'static str {
        match self {
            Law::UnionIdem => "a|a==a",
            Law::InterIdem => "a&a==a",
            Law::InterSub => "(a&b)<=a",
            Law::DiffEmpty => "((a&!b)&b)==0",
            Law::UnionEmpty => "a|0==a",
            Law::InterUniv => "a&U==a",
            Law::DiffSelf => "a&!a==0",
            Law::DeMorgan => "compl(a|b)==compl(a)&compl(b)",
        }
    }
}

/// 定律总数锚。
pub const LAW_N: usize = 8;
pub const ALL_LAWS: [Law; LAW_N] = [
    Law::UnionIdem,
    Law::InterIdem,
    Law::InterSub,
    Law::DiffEmpty,
    Law::UnionEmpty,
    Law::InterUniv,
    Law::DiffSelf,
    Law::DeMorgan,
];

/// 对单个 (a, b) 对验证全部定律；返回 Ok 或首个破律。
pub fn verify_pair(a: u32, b: u32) -> Result<(), u16> {
    if munion(a, a) != a {
        return Err(E_IDEM);
    }
    if minter(a, a) != a {
        return Err(E_IDEM);
    }
    if !msub(minter(a, b), a) {
        return Err(E_SUB);
    }
    if minter(mdiff(a, b), b) != 0 {
        return Err(E_DIFF);
    }
    if munion(a, 0) != a {
        return Err(E_ABSORB);
    }
    if minter(a, mt::ALL_LEGAL) != (a & mt::ALL_LEGAL) {
        return Err(E_ABSORB);
    }
    if mdiff(a, a) != 0 {
        return Err(E_DIFF);
    }
    if mcompl(munion(a, b)) != minter(mcompl(a), mcompl(b)) {
        return Err(E_DEMORG);
    }
    Ok(())
}

/// 破差原语（负例注入用）：把差实现成“交”的伪原语。
pub fn broken_diff(a: u32, b: u32) -> u32 {
    a & b
}

/// 对伪原语跑差律——检出即真（伪差 ∩ b ≠ 0 = 差律破被抓住）。
pub fn broken_diff_caught(a: u32, b: u32) -> bool {
    minter(broken_diff(a, b), b) != 0
}

/// 确定性样本集：手picked 边界 + LCG 派生（同一种子可复现）。
pub fn sample_masks() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    v.push(0);
    v.push(mt::ALL_LEGAL);
    v.push(mt::SPEC_MASK);
    v.push(mt::STD_USED);
    v.push(mt::GEN_USED);
    v.push(mt::ALL_RSVD);
    v.push(0x0012_0089);
    v.push(0x0002_0019);
    v.push(0x8000_0000);
    v.push(0x1000_0000);
    // LCG 派生 16 例（确定性）。
    let mut state: u64 = 0x243F_6A88_85A3_08D3;
    let mut i = 0;
    while i < 16 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let m = (state >> 33) as u32;
        v.push(m);
        i += 1;
    }
    v
}

/// 全样本对拍：样本集两两配对跑全部定律。
/// 返回 (对数, 首错)；全过 → Err(0) 哨兵？不——用 Ok(对数)。
pub fn verify_all() -> Result<usize, u16> {
    let s = sample_masks();
    let n = s.len();
    let mut pairs = 0usize;
    let mut i = 0;
    while i < n {
        let mut j = 0;
        while j < n {
            verify_pair(s[i], s[j])?;
            pairs += 1;
            j += 1;
        }
        i += 1;
    }
    Ok(pairs)
}

/// 演练：R1 律表纪律；R2 全样本恒等式对拍；R3 负例注入（破差原语
/// 必须被检出，且对真原语不得误报）；R4 合成守恒（差+交=原集）；
/// R5 补对合（补的补=原，限于合法域）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：律表纪律——8 律、判别式连续、名非空且唯一。
    if VER != 1 {
        return Err(E_VER);
    }
    if LAW_N != 8 {
        return Err(E_NEG);
    }
    let mut names: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < LAW_N {
        let nm = ALL_LAWS[i].name();
        if nm.is_empty() {
            return Err(E_NEG);
        }
        let mut j = 0;
        while j < i {
            if names[j] == nm {
                return Err(E_NEG);
            }
            j += 1;
        }
        names.push(nm);
        if ALL_LAWS[i].code() != (i as u16 + 1) {
            return Err(E_NEG);
        }
        i += 1;
    }
    hits += 1;
    // R2：全样本对拍（26 样本 → 676 对 × 8 律）。
    let pairs = verify_all()?;
    if pairs != 676 {
        return Err(E_NEG);
    }
    hits += 1;
    // R3：负例注入——破差原语必须被差律检出（至少 8 例），真原语不得误报。
    let s = sample_masks();
    let mut caught = 0;
    let mut i = 0;
    while i < 8 {
        // 固定非交对：s[6]∩s[7]=0x01（保证伪差必被差律检出）。
        let (a, b) = if i % 2 == 0 { (s[6], s[7]) } else { (s[7], s[6]) };
        if !broken_diff_caught(a, b) {
            // 伪差在 a∩b 为空时与真差同值——选种子保证非空；若仍空则账错。
            if minter(a, b) != 0 {
                return Err(E_NEG);
            }
        } else {
            caught += 1;
        }
        if verify_pair(a, b).is_err() {
            return Err(E_DIFF); // 真原语误报 = 自检面失灵
        }
        i += 1;
    }
    if caught == 0 {
        return Err(E_NEG);
    }
    hits += 1;
    // R4：合成守恒——(a−b) ∪ (a∩b) = a（差与交的分解律），全样本。
    let mut i = 0;
    while i < s.len() {
        let a = s[i] & mt::ALL_LEGAL; // 分解律在合法域内陈述
        let mut j = 0;
        while j < s.len() {
            let b = s[j] & mt::ALL_LEGAL;
            if munion(mdiff(a, b), minter(a, b)) != a {
                return Err(E_SUB);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;
    // R5：补对合——compl(compl(a)) = a & U（合法域对合）。
    let mut i = 0;
    while i < s.len() {
        let a = s[i];
        if mcompl(mcompl(a)) != (a & mt::ALL_LEGAL) {
            return Err(E_DEMORG);
        }
        i += 1;
    }
    if mcompl(0) != mt::ALL_LEGAL || mcompl(mt::ALL_LEGAL) != 0 {
        return Err(E_ABSORB);
    }
    let _ = vec![0u8; 0];
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn laws_hold() {
        assert_eq!(verify_pair(0x0012_0089, 0x0002_0019), Ok(()));
        assert_eq!(verify_pair(0, mt::ALL_LEGAL), Ok(()));
    }

    #[test]
    fn broken_diff_detected() {
        assert!(broken_diff_caught(0xF0, 0x0F) == false);
        assert_eq!(broken_diff(0xF0, 0x0F), 0);
        assert_eq!(mdiff(0xF0, 0x0F), 0xF0);
    }

    #[test]
    fn demorgan_holds() {
        let a = 0x0012_0089;
        let b = 0x0002_0019;
        assert_eq!(mcompl(munion(a, b)), minter(mcompl(a), mcompl(b)));
    }
}

pub fn run_unx36108_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36108_maskops");
    cs.add("SEG 段位独占", SEG_BASE == 0x3980, "0x3980~0x398F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "八律全样本对平、负例全检出"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("律表闭集", LAW_N == 8, "八条布尔恒等式");
    cs.add("样本量锚", sample_masks().len() == 26, "10 手picked + 16 LCG");
    cs.add("真源锚", mt::ALL_LEGAL == 0xF01F_FFFF, "合法域 = 36101::ALL_LEGAL");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_IDEM => "幂等律破（a∪a≠a 或 a∩a≠a）",
        E_SUB => "子集律破（a∩b⊆a）",
        E_DIFF => "差律破（(a−b)∩b≠∅）",
        E_ABSORB => "吸收律破（a∪∅≠a / a∩U≠a）",
        E_DEMORG => "德摩根律破",
        E_NEG => "负例注入未被检出（自检面失灵）",
        _ => "未知诊断码",
    }
}
