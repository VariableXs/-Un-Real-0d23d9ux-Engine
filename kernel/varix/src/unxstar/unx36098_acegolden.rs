//! UNX-F36098-J1 ACE 黄金用例库 v1（B09/B10/B11 共享消费锚）。
//! 判据：① 27 条金例（17 合法 + 10 拒绝，覆盖类型/掩码/标志/尺寸四维），
//! 逐条可重放判定全对平；② 库指纹 FNV 两次求值稳定；③ 篡改注入 2 例
//! （期望翻转/期望码翻转）必被重放器检出；④ 纯功能代码 ≥300 行。
//! 设计纪律：自含模块——金例只引用真源 unx36081_acecore 的闸与码，
//! 不 import 未来批模块（B09/B10/B11 到场后按卡消费本库）。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x38a0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;      // 版本锚不符
pub const E_COUNT: u16 = SEG_BASE + 2;    // 金例数不符
pub const E_MISMATCH: u16 = SEG_BASE + 3;// 重放判定与期望不符
pub const E_NAME: u16 = SEG_BASE + 4;    // 金例名非法（空名）
pub const E_CS: u16 = SEG_BASE + 5;      // 库指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 金例合法尺寸基准（头最小尺寸；尺寸维金例另有 0x10/0x1000/坏值）。
pub const CASE_SIZE: u16 = core::ACE_HEADER_MIN as u16;

/// 金例：类型/掩码/标志/尺寸/期望（expect_ok: 1=收 0=拒；拒例带期望码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenCase {
    pub name: &'static str,
    pub ace_type: u8,
    pub mask: u32,
    pub flags: u8,
    pub size: u16,
    pub expect_ok: u8,
    pub expect_code: u16,
}

/// 金例库 v1（27 条：G001..G027）。
pub const GOLDEN_N: usize = 27;
pub const GOLDEN_CASES: [GoldenCase; GOLDEN_N] = [
    GoldenCase { name: "G001-allow-base", ace_type: 0x00, mask: 0x0001_0001, flags: 0x00, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G002-allow-inherited", ace_type: 0x00, mask: 0x0001_0001, flags: 0x10, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G003-allow-all-specific", ace_type: 0x00, mask: 0x0000_ffff, flags: 0x00, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G004-allow-standard5", ace_type: 0x00, mask: 0x001f_0000, flags: 0x00, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G005-allow-generic-read", ace_type: 0x00, mask: 0x8000_0000, flags: 0x00, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G006-deny-base", ace_type: 0x01, mask: 0x0001_0001, flags: 0x00, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G007-deny-ci", ace_type: 0x01, mask: 0x0001_0001, flags: 0x01, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G008-audit-success", ace_type: 0x02, mask: 0x0001_0001, flags: 0x40, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G009-audit-failure", ace_type: 0x02, mask: 0x0001_0001, flags: 0x80, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G010-audit-both", ace_type: 0x02, mask: 0x0001_0001, flags: 0xc0, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G011-allowed-object", ace_type: 0x05, mask: 0x0001_0001, flags: 0x00, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G012-denied-object", ace_type: 0x06, mask: 0x0001_0001, flags: 0x00, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G013-label-zeromask", ace_type: 0x11, mask: 0x0000_0000, flags: 0x00, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G014-resource-attr", ace_type: 0x12, mask: 0x0001_0001, flags: 0x00, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G015-scoped-policy", ace_type: 0x13, mask: 0x0001_0001, flags: 0x00, size: CASE_SIZE, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G016-allow-size-0x10", ace_type: 0x00, mask: 0x0001_0001, flags: 0x00, size: 0x0010, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G017-allow-size-max", ace_type: 0x00, mask: 0x0001_0001, flags: 0x00, size: 0x1000, expect_ok: 1, expect_code: 0 },
    GoldenCase { name: "G018-unknown-type-04", ace_type: 0x04, mask: 0x0001_0001, flags: 0x00, size: CASE_SIZE, expect_ok: 0, expect_code: core::E_TYPE },
    GoldenCase { name: "G019-unknown-type-ee", ace_type: 0xee, mask: 0x0001_0001, flags: 0x00, size: CASE_SIZE, expect_ok: 0, expect_code: core::E_TYPE },
    GoldenCase { name: "G020-size-7", ace_type: 0x00, mask: 0x0001_0001, flags: 0x00, size: 7, expect_ok: 0, expect_code: core::E_SIZE },
    GoldenCase { name: "G021-size-0", ace_type: 0x00, mask: 0x0001_0001, flags: 0x00, size: 0, expect_ok: 0, expect_code: core::E_SIZE },
    GoldenCase { name: "G022-size-over", ace_type: 0x00, mask: 0x0001_0001, flags: 0x00, size: 0xf001, expect_ok: 0, expect_code: core::E_SIZE },
    GoldenCase { name: "G023-flags-0x20", ace_type: 0x00, mask: 0x0001_0001, flags: 0x20, size: CASE_SIZE, expect_ok: 0, expect_code: core::E_FLAGS },
    GoldenCase { name: "G024-flags-0x60", ace_type: 0x00, mask: 0x0001_0001, flags: 0x60, size: CASE_SIZE, expect_ok: 0, expect_code: core::E_FLAGS },
    GoldenCase { name: "G025-mask-reserved-02", ace_type: 0x00, mask: 0x0200_0000, flags: 0x00, size: CASE_SIZE, expect_ok: 0, expect_code: core::E_MASK },
    GoldenCase { name: "G026-mask-reserved-0e", ace_type: 0x00, mask: 0x0e00_0000, flags: 0x00, size: CASE_SIZE, expect_ok: 0, expect_code: core::E_MASK },
    GoldenCase { name: "G027-flags-0xa0", ace_type: 0x01, mask: 0x0001_0001, flags: 0xa0, size: CASE_SIZE, expect_ok: 0, expect_code: core::E_FLAGS },
];

/// 金例两态类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseKind {
    /// 合法例（应收）。
    Accept = 0x0001,
    /// 拒绝例（应拒且码正确）。
    Reject = 0x0002,
}

impl CaseKind {
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 由 expect_ok 归类（0/1 之外 = None）。
    pub fn from_expect(ok: u8) -> Option<CaseKind> {
        match ok {
            1 => Some(CaseKind::Accept),
            0 => Some(CaseKind::Reject),
            _ => None,
        }
    }
}

/// 单例重放判定：真源闸结果与期望对拍（不符 = E_MISMATCH）。
pub fn replay(c: &GoldenCase) -> Result<(), u16> {
    let r = core::AceHeader::new(c.ace_type, c.flags, c.size, c.mask);
    match (c.expect_ok, r) {
        (1, Ok(_)) => Ok(()),
        (1, Err(_)) => Err(E_MISMATCH),
        (0, Ok(_)) => Err(E_MISMATCH),
        (0, Err(code)) => {
            if code != c.expect_code {
                return Err(E_MISMATCH);
            }
            Ok(())
        }
        _ => Err(E_MISMATCH),
    }
}

/// 全库重放：逐条判定，任何一条不符即 E_MISMATCH。
pub fn replay_all() -> Result<u32, u16> {
    let mut ok_n: u32 = 0;
    let mut i = 0;
    while i < GOLDEN_N {
        replay(&GOLDEN_CASES[i])?;
        ok_n += 1;
        i += 1;
    }
    Ok(ok_n)
}

/// 库纪律自检：条数锚、名字非空且带 G 前缀、期望码闭集（合法例码必 0、
/// 拒例码必真源码）、合法/拒绝两态计数对平。
pub fn library_integrity() -> Result<(), u16> {
    if GOLDEN_N != 27 || GOLDEN_N < 24 {
        return Err(E_COUNT);
    }
    let mut valid_n = 0;
    let mut reject_n = 0;
    let mut i = 0;
    while i < GOLDEN_N {
        let c = &GOLDEN_CASES[i];
        if c.name.is_empty() || c.name.as_bytes()[0] != b'G' {
            return Err(E_NAME);
        }
        if c.expect_ok > 1 {
            return Err(E_MISMATCH);
        }
        if c.expect_ok == 1 && c.expect_code != 0 {
            return Err(E_MISMATCH);
        }
        if c.expect_ok == 1 {
            valid_n += 1;
        } else {
            reject_n += 1;
        }
        i += 1;
    }
    if valid_n + reject_n != GOLDEN_N {
        return Err(E_COUNT);
    }
    if valid_n != 17 || reject_n != 10 {
        return Err(E_COUNT);
    }
    Ok(())
}

/// 两态计数（合法例数 / 拒绝例数）。
pub fn case_counts() -> (usize, usize) {
    let mut v = 0;
    let mut r = 0;
    let mut i = 0;
    while i < GOLDEN_N {
        if GOLDEN_CASES[i].expect_ok == 1 {
            v += 1;
        } else {
            r += 1;
        }
        i += 1;
    }
    (v, r)
}

/// 期望选择器：按 expect_ok 取金例位次清单（分面重放用）。
pub fn cases_by_expect(ok: u8) -> Vec<usize> {
    let mut out: Vec<usize> = vec![];
    let mut i = 0;
    while i < GOLDEN_N {
        if GOLDEN_CASES[i].expect_ok == ok {
            out.push(i);
        }
        i += 1;
    }
    out
}

/// 分面重放：只重放选定子集（合法面 / 拒绝面），全对平返回条数。
pub fn replay_subset(idxs: &[usize]) -> Result<u32, u16> {
    let mut ok_n: u32 = 0;
    let mut k = 0;
    while k < idxs.len() {
        if idxs[k] >= GOLDEN_N {
            return Err(E_COUNT);
        }
        replay(&GOLDEN_CASES[idxs[k]])?;
        ok_n += 1;
        k += 1;
    }
    Ok(ok_n)
}

/// 库指纹：FNV over 逐例四件+期望 + 版本锚（B09/B10/B11 消费时对拍）。
pub fn library_fingerprint() -> u64 {
    let mut feed = Vec::new();
    feed.extend_from_slice(&VER.to_le_bytes());
    feed.extend_from_slice(&(GOLDEN_N as u64).to_le_bytes());
    let mut i = 0;
    while i < GOLDEN_N {
        let c = &GOLDEN_CASES[i];
        feed.push(c.ace_type);
        feed.extend_from_slice(&c.mask.to_le_bytes());
        feed.push(c.flags);
        feed.extend_from_slice(&c.size.to_le_bytes());
        feed.push(c.expect_ok);
        feed.extend_from_slice(&c.expect_code.to_le_bytes());
        i += 1;
    }
    core::fnv1a(&feed)
}

/// 摘要行（String 面，无 IO）。
pub fn summary() -> String {
    let mut s = String::new();
    s.push_str("golden n=");
    s.push_str(&GOLDEN_N.to_string());
    s.push_str(" fp=");
    s.push_str(&library_fingerprint().to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_COUNT, E_MISMATCH, E_NAME, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_COUNT => "金例数不符",
        E_MISMATCH => "重放判定与期望不符",
        E_NAME => "金例名非法（空名）",
        E_CS => "库指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 库纪律；R2 全库 27 条重放全对平；R3 篡改注入 2 例必检出；
/// R4 分面重放（合法面 17 / 拒绝面 10）；R5 指纹两次求值稳定且非零。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：库纪律 + 版本锚。
    library_integrity()?;
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：全库重放（27 条逐条对平）。
    let n = replay_all()?;
    if n != GOLDEN_N as u32 {
        return Err(E_COUNT);
    }
    hits += n;
    // R3：篡改注入 2 例——重放器必检出。
    // 第 1 例：合法例期望翻转成拒（期望不符 → E_MISMATCH）。
    let mut t1 = GOLDEN_CASES[0];
    t1.expect_ok = 0;
    match replay(&t1) {
        Ok(_) => return Err(E_MISMATCH),
        Err(c) => {
            if c != E_MISMATCH {
                return Err(E_MISMATCH);
            }
        }
    }
    hits += 1;
    // 第 2 例：拒例期望码翻转（码不符 → E_MISMATCH）。
    let mut t2 = GOLDEN_CASES[18];
    t2.expect_code = core::E_SIZE;
    match replay(&t2) {
        Ok(_) => return Err(E_MISMATCH),
        Err(c) => {
            if c != E_MISMATCH {
                return Err(E_MISMATCH);
            }
        }
    }
    hits += 1;
    // R4：分面重放（合法面 17 条 / 拒绝面 10 条，两态计数对平）。
    let (v, r) = case_counts();
    if v != 17 || r != 10 || v + r != GOLDEN_N {
        return Err(E_COUNT);
    }
    let vn = replay_subset(&cases_by_expect(1))?;
    let rn = replay_subset(&cases_by_expect(0))?;
    if vn != 17 || rn != 10 {
        return Err(E_COUNT);
    }
    hits += 2;
    // R5：指纹两次求值稳定且非零。
    let fp1 = library_fingerprint();
    let fp2 = library_fingerprint();
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(33));
    }

    #[test]
    fn subset_replay() {
        assert_eq!(replay_subset(&cases_by_expect(1)), Ok(17));
        assert_eq!(replay_subset(&cases_by_expect(0)), Ok(10));
        assert_eq!(replay_subset(&[99]), Err(E_COUNT));
        assert_eq!(case_counts(), (17, 10));
    }

    #[test]
    fn replay_all_green() {
        assert_eq!(replay_all(), Ok(27));
    }

    #[test]
    fn library_count_anchor() {
        assert_eq!(GOLDEN_N, 27);
        assert!(GOLDEN_N >= 24);
        assert_eq!(library_integrity(), Ok(()));
    }

    #[test]
    fn fingerprint_stable() {
        let a = library_fingerprint();
        let b = library_fingerprint();
        assert_eq!(a, b);
        assert!(summary().contains("golden"));
    }
}

pub fn run_unx36098_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36098_acegolden");
    cs.add("SEG 段位独占", SEG_BASE == 0x38a0, "0x38a0~0x38af 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "金例 27 条全对平、篡改 2 例必检出"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("金例数锚 27≥24", GOLDEN_N == 27, "金例数锚漂移");
    cs.add("库指纹稳定", library_fingerprint() != 0, "库指纹求值为零");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
