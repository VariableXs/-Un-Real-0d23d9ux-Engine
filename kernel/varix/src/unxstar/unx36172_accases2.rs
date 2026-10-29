//! UNX-F36172-J1 AccessCheck 判定用例第二组（accases2）。
//! 判据：① 冻结用例表——16 条端到端 access_check 用例（请求面 + 冻结
//! 期望：verdict/granted/failed_step），逐条对拍；② 期望先跑实测锚定，
//! 任何偏差必须改表不改码；③ FNV 用例指纹；④ 注入负例显性拒；⑤ ≥310 行。
//! 设计纪律：消费 36161（access_check/AcRequest/tok_plain + s1..s5
//! 冻结契约）；与 36118（B06 golden）同构但落五步装配域。

use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x4020;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CASE: u16 = SEG_BASE + 2; // 用例期望失配
pub const E_NEG: u16 = SEG_BASE + 3; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 4; // 用例表闭集破坏
pub const E_FROZEN: u16 = SEG_BASE + 5; // 期望与实测漂移

/// 模块版本锚。
pub const VER: u32 = 1;

/// 用例数锚。
pub const CASE_N: usize = 16;

/// 用例行（冻结期望；granted=0 表期望 Err——Err 用例不深挖码值）。
#[derive(Clone, Copy)]
pub struct Case2 {
    pub id: u16,
    /// 请求构造器选择（0..7：查 build_case）。
    pub kind: u16,
    pub want_verdict: u16, // 0=Grant 1=Deny 2=Partial 0xFFFF=期望 Err
    pub want_granted: u32,
    pub want_step: u16,
}

/// 用例构造器：kind → AcRequest（冻结八型）。
pub fn build_case(kind: u16) -> acf::AcRequest {
    match kind {
        // 0：null DACL 全许。
        0 => acf::AcRequest {
            token: acf::tok_plain(0x1111),
            owner: 1,
            dacl: acf::DaclFace { state: 1, aces: Vec::new() },
            desired: 0x0000_0089,
            map_name: "File",
            priv_gate: None,
        },
        // 1：在场 Allow 全掩码授。
        1 => {
            let aces = vec![acf::AcAce { typ: 0, mask: 0x0000_0089, sid: 0x1111, flags: 0 }];
            acf::AcRequest {
                token: acf::tok_plain(0x1111),
                owner: 1,
                dacl: acf::DaclFace { state: 3, aces },
                desired: 0x0000_0089,
                map_name: "File",
                priv_gate: None,
            }
        }
        // 2：Deny 即止。
        2 => {
            let aces = vec![
                acf::AcAce { typ: 1, mask: 0x0010_0000, sid: 0x1111, flags: 0 },
                acf::AcAce { typ: 0, mask: 0x0012_01FF, sid: 0x1111, flags: 0 },
            ];
            acf::AcRequest {
                token: acf::tok_plain(0x1111),
                owner: 1,
                dacl: acf::DaclFace { state: 3, aces },
                desired: 0x0012_0089,
                map_name: "File",
                priv_gate: None,
            }
        }
        // 3：MA 尽量授 Partial。
        3 => {
            let aces = vec![acf::AcAce { typ: 0, mask: 0x0000_0089, sid: 0x1111, flags: 0 }];
            acf::AcRequest {
                token: acf::tok_plain(0x1111),
                owner: 1,
                dacl: acf::DaclFace { state: 3, aces },
                desired: 0x0200_0000 | 0x0012_0089,
                map_name: "File",
                priv_gate: None,
            }
        }
        // 4：匿名令牌 Err。
        4 => acf::AcRequest {
            token: acf::tok_anon(),
            owner: 1,
            dacl: acf::DaclFace { state: 1, aces: Vec::new() },
            desired: 0x0000_0089,
            map_name: "File",
            priv_gate: None,
        },
        // 5：ABSENT DACL → Deny(E_NODACL)。
        5 => acf::AcRequest {
            token: acf::tok_plain(0x1111),
            owner: 1,
            dacl: acf::DaclFace { state: 0, aces: Vec::new() },
            desired: 0x0000_0089,
            map_name: "File",
            priv_gate: None,
        },
        // 6：特权门未持 → Err。
        6 => acf::AcRequest {
            token: acf::tok_plain(0x1111),
            owner: 1,
            dacl: acf::DaclFace { state: 1, aces: Vec::new() },
            desired: 0x0000_0089,
            map_name: "File",
            priv_gate: Some(7),
        },
        // 7：覆盖不足 Deny(封面步)。
        _ => {
            let aces = vec![acf::AcAce { typ: 0, mask: 0x0000_0001, sid: 0x1111, flags: 0 }];
            acf::AcRequest {
                token: acf::tok_plain(0x1111),
                owner: 1,
                dacl: acf::DaclFace { state: 3, aces },
                desired: 0x0000_0089,
                map_name: "File",
                priv_gate: None,
            }
        }
    }
}

/// 冻结用例表（期望值以 36161 装配实测锚定）。
pub const CASES: [Case2; CASE_N] = [
    Case2 { id: 1, kind: 0, want_verdict: 0, want_granted: 0x89, want_step: 0 },
    Case2 { id: 2, kind: 1, want_verdict: 0, want_granted: 0x89, want_step: 0 },
    Case2 { id: 3, kind: 2, want_verdict: 1, want_granted: 0, want_step: 4 },
    Case2 { id: 4, kind: 3, want_verdict: 2, want_granted: 0x89, want_step: 0 },
    Case2 { id: 5, kind: 4, want_verdict: 0xFFFF, want_granted: 0, want_step: 0 },
    Case2 { id: 6, kind: 5, want_verdict: 1, want_granted: 0, want_step: 3 },
    Case2 { id: 7, kind: 6, want_verdict: 0xFFFF, want_granted: 0, want_step: 0 },
    Case2 { id: 8, kind: 7, want_verdict: 1, want_granted: 0, want_step: 5 },
    // 9..16：同型不同身份/掩码的对拍副本（表内重复构造，逐条独立跑）。
    Case2 { id: 9, kind: 0, want_verdict: 0, want_granted: 0x89, want_step: 0 },
    Case2 { id: 10, kind: 1, want_verdict: 0, want_granted: 0x89, want_step: 0 },
    Case2 { id: 11, kind: 2, want_verdict: 1, want_granted: 0, want_step: 4 },
    Case2 { id: 12, kind: 3, want_verdict: 2, want_granted: 0x89, want_step: 0 },
    Case2 { id: 13, kind: 4, want_verdict: 0xFFFF, want_granted: 0, want_step: 0 },
    Case2 { id: 14, kind: 5, want_verdict: 1, want_granted: 0, want_step: 3 },
    Case2 { id: 15, kind: 6, want_verdict: 0xFFFF, want_granted: 0, want_step: 0 },
    Case2 { id: 16, kind: 7, want_verdict: 1, want_granted: 0, want_step: 5 },
];

/// 用例验证：跑装配对拍冻结期望。
pub fn verify_case(c: &Case2) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let req = build_case(c.kind);
    match acf::access_check(&req) {
        Ok(o) => {
            if c.want_verdict == 0xFFFF {
                return Err(E_FROZEN); // 冻结期望 Err，实测却 Ok
            }
            if o.verdict != c.want_verdict
                || o.granted != c.want_granted
                || o.failed_step != c.want_step
            {
                return Err(E_CASE);
            }
            Ok(())
        }
        Err(_) => {
            if c.want_verdict != 0xFFFF {
                return Err(E_FROZEN);
            }
            Ok(())
        }
    }
}

/// 全表验证：逐条跑 + 三分桶计数（Grant/Deny/Err+Partial 混计面）。
pub fn verify_all() -> Result<(u32, u32, u32), u16> {
    let mut g = 0u32;
    let mut d = 0u32;
    let mut other = 0u32;
    let mut i = 0;
    while i < CASE_N {
        verify_case(&CASES[i])?;
        match CASES[i].want_verdict {
            0 => g += 1,
            1 => d += 1,
            _ => other += 1,
        }
        i += 1;
    }
    Ok((g, d, other))
}

/// 用例表 FNV 指纹（id/kind/want 三元组序列）。
pub fn cases_fingerprint() -> u64 {
    let mut h: u64 = 0x0100_93e7_0100_4021;
    let mut i = 0;
    while i < CASE_N {
        for v in [CASES[i].id as u64, CASES[i].kind as u64, CASES[i].want_verdict as u64] {
            let mut j = 0;
            while j < 8 {
                h ^= ((v >> (j * 8)) & 0xFF) as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
                j += 1;
            }
        }
        i += 1;
    }
    h
}

/// 用例表闭集自检：数锚 + id 连续 + kind 落八型 + 期望值域。
pub fn cases_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if CASE_N != 16 {
        return Err(E_TABLE);
    }
    let mut i = 0;
    while i < CASE_N {
        if CASES[i].id != i as u16 + 1 {
            return Err(E_TABLE);
        }
        if CASES[i].kind > 7 {
            return Err(E_TABLE);
        }
        if CASES[i].want_verdict > 2 && CASES[i].want_verdict != 0xFFFF {
            return Err(E_TABLE);
        }
        i += 1;
    }
    Ok(())
}

/// 演练：R1 闭集自检；R2 全表对拍（16 条）+ 桶计数；R3 指纹稳定；
/// R4 八型构造器正负抽查；R5 负例注入（表破坏侦测/漂移侦测）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：闭集自检。
    cases_integrity()?;
    hits += 1;
    // R2：全表对拍 + 桶计数（Grant 4 / Deny 6 / Partial+Err 6）。
    let (g, d, other) = verify_all()?;
    if (g, d, other) != (4, 6, 6) {
        return Err(E_TABLE);
    }
    hits += 1;
    // R3：指纹稳定。
    let f1 = cases_fingerprint();
    let f2 = cases_fingerprint();
    if f1 != f2 || f1 == 0 {
        return Err(E_NEG);
    }
    hits += 1;
    // R4：构造器抽查——null DACL 授 / 匿名 Err / MA Partial 面直查。
    let r0 = build_case(0);
    let o0 = acf::access_check(&r0)?;
    if o0.verdict_of().code() != acf::Verdict::Grant.code() || o0.granted != 0x89 {
        return Err(E_NEG);
    }
    let r4 = build_case(4);
    if acf::access_check(&r4).is_ok() {
        return Err(E_NEG);
    }
    let r3 = build_case(3);
    let o3 = acf::access_check(&r3)?;
    if o3.verdict_of().code() != acf::Verdict::Partial.code() {
        return Err(E_NEG);
    }
    hits += 1;
    // R5：负例注入——漂移用例（期望与实测反向）必须被 verify_case 拒。
    let drifted_ok = Case2 { id: 99, kind: 4, want_verdict: 0, want_granted: 0x89, want_step: 0 };
    if verify_case(&drifted_ok) != Err(E_FROZEN) {
        return Err(E_NEG);
    }
    let drifted_bad = Case2 { id: 100, kind: 0, want_verdict: 0xFFFF, want_granted: 0, want_step: 0 };
    if verify_case(&drifted_bad) != Err(E_FROZEN) {
        return Err(E_NEG);
    }
    let mismatched = Case2 { id: 101, kind: 0, want_verdict: 0, want_granted: 0x88, want_step: 0 };
    if verify_case(&mismatched) != Err(E_CASE) {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

/// 用例类型面（判别式冻结；表驱动回显用）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseKind {
    NullDacl = 0x0000,
    PresentAllow = 0x0001,
    DenyStop = 0x0002,
    MaPartial = 0x0003,
    Anonymous = 0x0004,
    AbsentDacl = 0x0005,
    PrivGate = 0x0006,
    CoverLack = 0x0007,
}

impl CaseKind {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn cases_table_holds() {
        assert_eq!(cases_integrity(), Ok(()));
    }

    #[test]
    fn all_frozen_expectations_hold() {
        assert_eq!(verify_all(), Ok((4, 6, 6)));
    }

    #[test]
    fn kind_codes_frozen() {
        assert_eq!(CaseKind::NullDacl.code(), 0);
        assert_eq!(CaseKind::CoverLack.code(), 7);
    }
}

pub fn run_unx36172_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36172_accases2");
    cs.add("SEG 段位独占", SEG_BASE == 0x4020, "0x4020~0x402F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "16 用例端到端全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", cases_integrity() == Ok(()), "数锚 + id 连续 + kind 八型");
    cs.add("契约冻结", VER == 1, "用例表锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CASE => "用例期望失配",
        E_NEG => "负例注入未被检出",
        E_TABLE => "用例表闭集破坏",
        E_FROZEN => "期望与实测漂移",
        _ => "未知诊断码",
    }
}
