//! UNX-F36109-J1 掩码覆盖判定（所需 ⊆ 已授）原语。
//! 判据：① covers/missing/gap 三面一致性守恒（covers ⇔ missing==0
//! ⇔ gap_n==0）全样本通过；② 单调性与缺口提取逐位回查 + 负例；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：授权判定消费方——covers 是 36110 授权计算器与未来
//! AccessCheck 第⑤步的判定核；位段名诊断借 36101 真源。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3990;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CONS: u16 = SEG_BASE + 2; // 三面一致性守恒破
pub const E_MONO: u16 = SEG_BASE + 3; // 单调性破（扩权后覆盖反而丢）
pub const E_GAP: u16 = SEG_BASE + 4; // 缺口提取与 missing 掩码不符
pub const E_NEG: u16 = SEG_BASE + 5; // 负例注入未被检出
pub const E_EMPTY: u16 = SEG_BASE + 6; // 空需求判定语义错（空需求恒覆盖）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 覆盖判定三分（授/部分/拒——AccessCheck 第⑤步结果消费核）。
#[repr(u16)]
pub enum CoverVerdict {
    Full = 0x0001,
    Partial = 0x0002,
    Denied = 0x0004,
}

impl CoverVerdict {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 判定：全过 Full；有授有缺 Partial；全缺 Denied。
    pub fn judge(granted: u32, required: u32) -> CoverVerdict {
        let missing = required & !granted;
        if missing == 0 {
            CoverVerdict::Full
        } else if required & granted != 0 {
            CoverVerdict::Partial
        } else {
            CoverVerdict::Denied
        }
    }
}

/// 覆盖判定：所需 ⊆ 已授。
pub fn covers(granted: u32, required: u32) -> bool {
    required & !granted == 0
}

/// 缺失位掩码：所需有而已授无的位。
pub fn missing_bits(granted: u32, required: u32) -> u32 {
    required & !granted
}

/// 覆盖账：判定 + 缺失掩码 + 缺口数三面合一。
pub struct CoverReport {
    pub ok: bool,
    pub missing: u32,
    pub gap_n: u32,
}

/// 出账（三面同源，drill 守恒对拍）。
pub fn report(granted: u32, required: u32) -> CoverReport {
    let m = missing_bits(granted, required);
    CoverReport { ok: m == 0, missing: m, gap_n: popcount(m) }
}

/// 32 位整数 popcount（无 std 依赖）。
pub fn popcount(m: u32) -> u32 {
    let mut n = 0;
    let mut x = m;
    while x != 0 {
        n += x & 1;
        x >>= 1;
    }
    n
}

/// 缺口提取：缺失位号升序表（含段归属标注）。
pub fn gap_list(granted: u32, required: u32) -> Result<Vec<(u8, u16)>, u16> {
    let m = missing_bits(granted, required);
    let mut out: Vec<(u8, u16)> = Vec::new();
    let mut b = 0;
    while b < 32 {
        if m & (1u32 << b) != 0 {
            let seg = match mt::seg_of(b)? {
                mt::SegKind::Specific => 1u16,
                mt::SegKind::Standard => 2u16,
                mt::SegKind::Generic => 3u16,
                _ => 4u16,
            };
            out.push((b as u8, seg));
        }
        b += 1;
    }
    Ok(out)
}

/// 空需求哨兵：空需求恒覆盖（任何授权都满足“什么都不缺”）。
pub fn empty_required_ok(granted: u32) -> bool {
    covers(granted, 0)
}

/// 分段覆盖账：需求按段拆分（特定/标准/GENERIC）逐段覆盖判定，
/// 返回三段 (需求位, 已满足位) 快照——AccessCheck 第⑤步结果
/// 三分（授/拒/部分）的消费核。
pub struct SegCover {
    pub req_spec: u32,
    pub ok_spec: bool,
    pub req_std: u32,
    pub ok_std: bool,
    pub req_gen: u32,
    pub ok_gen: bool,
    /// 三段全过 = 全量覆盖；全不过 = 全拒；否则 = 部分。
    pub partial: bool,
}

/// 出分段账（需求含保留位 → 账面未定义，显性拒）。
pub fn seg_report(granted: u32, required: u32) -> Result<SegCover, u16> {
    if required & mt::ALL_RSVD != 0 {
        return Err(E_CONS);
    }
    let rs = required & mt::SPEC_MASK;
    let rt = required & mt::STD_USED;
    let rg = required & mt::GEN_USED;
    let missing = required & !granted;
    let got_some = required & granted != 0;
    let partial = missing != 0 && got_some;
    Ok(SegCover {
        req_spec: rs,
        ok_spec: rs & !granted == 0,
        req_std: rt,
        ok_std: rt & !granted == 0,
        req_gen: rg,
        ok_gen: rg & !granted == 0,
        partial,
    })
}

/// 覆盖账与分段账的守恒对拍（合法域定律）：全量 covers ⇔ 三段
/// ok 全真 ⇔ 非部分。需求含保留位时账面未定义——显性拒。
fn seg_consistent(g: u32, r: u32) -> Result<(), u16> {
    if r & mt::ALL_RSVD != 0 {
        return Err(E_CONS);
    }
    let full = covers(g, r);
    let s = seg_report(g, r)?;
    if s.ok_spec && s.ok_std && s.ok_gen && !full {
        return Err(E_CONS);
    }
    if full && !(s.ok_spec && s.ok_std && s.ok_gen) {
        return Err(E_CONS);
    }
    if s.partial && full {
        return Err(E_CONS);
    }
    if !full && r != 0 && !s.partial && (r & g) != 0 {
        return Err(E_CONS); // 有授有缺却未标部分 = 分段账失灵
    }
    Ok(())
}

/// 演练：R1 基本判定正负例；R2 三面守恒全样本；R3 单调性；
/// R4 缺口提取逐位回查；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    if VER != 1 {
        return Err(E_VER);
    }
    // R1：基本正负例（含空需求、空授权、全并）。
    if !covers(0x0012_0089, 0x0012_0089) {
        return Err(E_CONS);
    }
    if covers(0x0002_0000, 0x0012_0089) {
        return Err(E_CONS);
    }
    if !covers(0, 0) || !empty_required_ok(0) || !empty_required_ok(mt::ALL_LEGAL) {
        return Err(E_EMPTY);
    }
    if covers(0, mt::STD_USED) || !covers(mt::ALL_LEGAL, mt::STD_USED) {
        return Err(E_CONS);
    }
    hits += 1;
    // R2：三面守恒——covers ⇔ missing==0 ⇔ gap_n==0，样本两两配对。
    let s = sample_set();
    let n = s.len();
    let mut pairs = 0usize;
    let mut i = 0;
    while i < n {
        let mut j = 0;
        while j < n {
            let r = report(s[i], s[j]);
            if r.ok != covers(s[i], s[j]) || r.ok != (r.missing == 0) || r.ok != (r.gap_n == 0) {
                return Err(E_CONS);
            }
            if r.gap_n != popcount(r.missing) {
                return Err(E_CONS);
            }
            pairs += 1;
            j += 1;
        }
        i += 1;
    }
    if pairs != n * n {
        return Err(E_NEG);
    }
    hits += 1;
    // R2b：分段账守恒——合法域样本两两对拍 seg_consistent。
    let mut i = 0;
    while i < n {
        let mut j = 0;
        while j < n {
            let g = s[i];
            let r = s[j] & mt::ALL_LEGAL;
            seg_consistent(g, r)?;
            // 三分面守恒：Full ⇔ covers；Partial ⇐ 非全过且有授。
            let vc = CoverVerdict::judge(g, r).code();
            if vc == CoverVerdict::Full.code() && !covers(g, r) {
                return Err(E_CONS);
            }
            if covers(g, r) && vc != CoverVerdict::Full.code() {
                return Err(E_CONS);
            }
            j += 1;
        }
        i += 1;
    }
    // 分段账负例：需求含保留位 → 显性拒。
    if seg_report(0, mt::STD_RSVD).is_ok() {
        return Err(E_CONS);
    }
    hits += 1;
    // R3：单调性——已授扩权（或等权）后覆盖不减；需求扩权后覆盖不增。
    let mut i = 0;
    while i < n {
        let mut j = 0;
        while j < n {
            let g1 = s[i] & mt::ALL_LEGAL;
            let g2 = g1 | (s[j] & mt::ALL_LEGAL);
            let req = s[(i + j) % n] & mt::ALL_LEGAL;
            if covers(g1, req) && !covers(g2, req) {
                return Err(E_MONO);
            }
            let r1 = req & mt::ALL_LEGAL;
            let r2 = r1 | (s[(i * 3 + j) % n] & mt::ALL_LEGAL);
            if covers(g1, r2) && !covers(g1, r1) {
                return Err(E_MONO);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;
    // R4：缺口提取——逐位回查：缺口表 ∪ = missing；表内位确实缺失。
    let mut i = 0;
    while i < 24 {
        let g = s[i % n];
        let r = s[(i * 7 + 3) % n];
        let m = missing_bits(g, r);
        let gaps = gap_list(g, r)?;
        let mut acc: u32 = 0;
        let mut k = 0;
        while k < gaps.len() {
            let (bit, _seg) = gaps[k];
            let bm = 1u32 << bit;
            if bm & m == 0 {
                return Err(E_GAP); // 表内位不在缺失掩码中
            }
            if bm & g != 0 {
                return Err(E_GAP); // 已授位不该出现
            }
            acc |= bm;
            k += 1;
        }
        if acc != m {
            return Err(E_GAP);
        }
        if gaps.len() as u32 != popcount(m) {
            return Err(E_GAP);
        }
        i += 1;
    }
    hits += 1;
    // R5：负例注入——伪 covers（恒真实现）必须被守恒检出。
    fn fake_covers(_g: u32, _r: u32) -> bool {
        true
    }
    let mut i = 0;
    let mut caught = 0;
    while i < n {
        if fake_covers(s[i], s[(i + 5) % n]) != covers(s[i], s[(i + 5) % n]) {
            caught += 1;
        }
        i += 1;
    }
    if caught == 0 {
        return Err(E_NEG);
    }
    if missing_bits(mt::ALL_LEGAL, mt::ALL_LEGAL) != 0 {
        return Err(E_CONS);
    }
    let _ = vec![0u8; 0];
    hits += 1;
    Ok(hits)
}

/// 确定性样本集（16 例：段边界 + LCG 派生）。
pub fn sample_set() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    v.push(0);
    v.push(mt::SPEC_MASK);
    v.push(mt::STD_USED);
    v.push(mt::GEN_USED);
    v.push(mt::ALL_LEGAL);
    v.push(0x0012_0089);
    v.push(0x0002_0019);
    v.push(0x0000_00FF);
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut i = 0;
    while i < 8 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        v.push((state >> 33) as u32);
        i += 1;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(6));
    }

    #[test]
    fn basic_covers() {
        assert!(covers(0x1F, 0x0F));
        assert!(!covers(0x0F, 0x1F));
        assert_eq!(missing_bits(0x0F, 0x1F), 0x10);
    }

    #[test]
    fn empty_required_always_ok() {
        assert!(covers(0, 0));
        assert!(covers(mt::ALL_LEGAL, 0));
    }

    #[test]
    fn report_consistent() {
        let r = report(0x0000_0089, 0x0000_008B);
        assert!(!r.ok);
        assert_eq!(r.missing, 0x2);
        assert_eq!(r.gap_n, 1);
    }

    #[test]
    fn seg_partial_semantics() {
        let s = seg_report(0x0000_0089, 0x0000_00AB).ok().unwrap_or(SegCover {
            req_spec: 0,
            ok_spec: true,
            req_std: 0,
            ok_std: true,
            req_gen: 0,
            ok_gen: true,
            partial: false,
        });
        assert!(!s.ok_spec);
        assert!(s.partial);
        assert_eq!(s.req_std, 0);
    }
}

pub fn run_unx36109_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36109_maskcover");
    cs.add("SEG 段位独占", SEG_BASE == 0x3990, "0x3990~0x399F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三面守恒/单调/负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("样本量锚", sample_set().len() == 16, "8 手picked + 8 LCG");
    cs.add("空需求哨兵", covers(0, 0), "空需求恒覆盖");
    cs.add("真源锚", mt::ALL_LEGAL == 0xF01F_FFFF, "合法域 = 36101::ALL_LEGAL");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CONS => "三面一致性守恒破",
        E_MONO => "单调性破（扩权后覆盖反而丢）",
        E_GAP => "缺口提取与 missing 掩码不符",
        E_NEG => "负例注入未被检出",
        E_EMPTY => "空需求判定语义错（空需求恒覆盖）",
        _ => "未知诊断码",
    }
}
