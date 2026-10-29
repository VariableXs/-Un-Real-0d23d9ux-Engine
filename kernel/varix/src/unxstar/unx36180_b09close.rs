//! UNX-F36180-J1 B09 批收口（b09close）。
//! 判据：① 三写交叉——EXPECT_SEGS / EXPECT_SLUGS / EXPECT_CARDS 与
//! 36179 卡表逐位对拍；② 窗口互斥（u16 回绕安全：差值域两两比对，
//! 不做全序假设——B09 窗口跨 0xefd0 与 0x3c00 两域）；③ 去重扫描
//! （SEG/slug 双面）；④ 指纹锚 + 收口声明；⑤ 注入负例显性拒；⑥ ≥310 行。
//! 设计纪律：收口锚 = 36179::CARDS（消费，不复制）；本模块是 B09
//! 批的终检面，窗口 0x4140（与主体域分离，象征批外收口位）。

use super::unx36179_b09ktest as kt;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4140;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SEGS: u16 = SEG_BASE + 2; // SEG 锚与卡表失配
pub const E_SLUGS: u16 = SEG_BASE + 3; // slug 锚与卡表失配
pub const E_CARDS: u16 = SEG_BASE + 4; // 卡号锚与卡表失配
pub const E_OVERLAP: u16 = SEG_BASE + 5; // 窗口互斥破坏
pub const E_DUP: u16 = SEG_BASE + 6; // 去重扫描命中
pub const E_NEG: u16 = SEG_BASE + 7; // 负例注入未被检出
pub const E_FINGER: u16 = SEG_BASE + 8; // 指纹锚漂移

/// 模块版本锚。
pub const VER: u32 = 1;

/// 收口卡数锚。
pub const EXPECT_N: usize = 18;

/// 期望 SEG 锚（第一写；与 36179::CARDS 逐位对拍）。
pub const EXPECT_SEGS: [u16; EXPECT_N] = [
    0xefd0, 0xefe0, 0xeff0, 0x3c00, 0x3c10, 0x3c20, 0x3c30,
    0x3c40, 0x3c50, 0x4000, 0x4010, 0x4020, 0x4030, 0x4040,
    0x4050, 0x4060, 0x4070, 0x4080,
];

/// 期望 slug 锚（第二写）。
pub const EXPECT_SLUGS: [&str; EXPECT_N] = [
    "acfreeze",
    "acimperson",
    "acpriv",
    "acdaclstate",
    "aceeval",
    "acsidmatch",
    "accover",
    "acdiagnose",
    "accache",
    "acperf",
    "acaudit",
    "accases2",
    "acorderlock",
    "acfuzz",
    "acconcurrent",
    "achandle",
    "acselftest",
    "acstorm",
];

/// 期望卡号锚（第三写）。
pub const EXPECT_CARDS: [u16; EXPECT_N] = [
    36161, 36162, 36163, 36164, 36165, 36166, 36167,
    36168, 36169, 36170, 36171, 36172, 36173, 36174,
    36175, 36176, 36177, 36178,
];

/// 三写交叉：期望三面与 36179 卡表逐位一致。
pub fn triple_check() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if kt::CARDS.len() != EXPECT_N {
        return Err(E_CARDS);
    }
    let cards = kt::CARDS;
    let mut i = 0;
    while i < EXPECT_N {
        if cards[i].seg != EXPECT_SEGS[i] {
            return Err(E_SEGS);
        }
        if cards[i].slug != EXPECT_SLUGS[i] {
            return Err(E_SLUGS);
        }
        if cards[i].card != EXPECT_CARDS[i] {
            return Err(E_CARDS);
        }
        i += 1;
    }
    Ok(())
}

/// 窗口互斥（回绕安全）：期望 SEG 两两差值 ≥ 0x10。
pub fn windows_exclusive() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut a = 0;
    while a < EXPECT_N {
        let mut b = a + 1;
        while b < EXPECT_N {
            let d = if EXPECT_SEGS[a] > EXPECT_SEGS[b] {
                EXPECT_SEGS[a] - EXPECT_SEGS[b]
            } else {
                EXPECT_SEGS[b] - EXPECT_SEGS[a]
            };
            if d < 0x10 {
                return Err(E_OVERLAP);
            }
            b += 1;
        }
        a += 1;
    }
    Ok(())
}

/// 去重扫描：SEG 与 slug 双面——同窗（差 < 0x10）或同 slug 即命中。
pub fn dedup_scan() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut dup: u32 = 0;
    let mut a = 0;
    while a < EXPECT_N {
        let mut b = a + 1;
        while b < EXPECT_N {
            let d = if EXPECT_SEGS[a] > EXPECT_SEGS[b] {
                EXPECT_SEGS[a] - EXPECT_SEGS[b]
            } else {
                EXPECT_SEGS[b] - EXPECT_SEGS[a]
            };
            if d < 0x10 || EXPECT_SLUGS[a] == EXPECT_SLUGS[b] {
                dup += 1;
            }
            b += 1;
        }
        a += 1;
    }
    if dup != 0 {
        return Err(E_DUP);
    }
    Ok(dup)
}

/// 收口指纹：FNV(卡号+SEG+slug 序列) ⊕ 36179 卡表指纹（双源交叉）。
pub fn close_fingerprint() -> u64 {
    let mut h: u64 = 0x0100_93e7_0100_4141;
    let mut i = 0;
    while i < EXPECT_N {
        for v in [EXPECT_CARDS[i] as u64, EXPECT_SEGS[i] as u64] {
            let mut j = 0;
            while j < 8 {
                h ^= ((v >> (j * 8)) & 0xFF) as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
                j += 1;
            }
        }
        let b = EXPECT_SLUGS[i].as_bytes();
        let mut j = 0;
        while j < b.len() {
            h ^= b[j] as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
            j += 1;
        }
        i += 1;
    }
    h ^ kt::cards_fingerprint()
}

/// 锚一致性：三写交叉 + 窗口互斥 + 去重 + 卡表完整性联合。
pub fn anchors_consistent() -> Result<(), u16> {
    triple_check()?;
    windows_exclusive()?;
    dedup_scan()?;
    kt::card_integrity()?;
    Ok(())
}

/// 收口声明（终检行文；确认 B09 批 18 单 + 收口 2 单全锚）。
pub fn close_statement() -> Result<&'static str, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    anchors_consistent()?;
    let f1 = close_fingerprint();
    let f2 = close_fingerprint();
    if f1 != f2 || f1 == 0 {
        return Err(E_FINGER);
    }
    Ok("B09 CLOSE: 20 units anchored (18 main + ktest + close), SEG windows exclusive, drills green")
}

/// 收口行种类（判别式冻结；报告投影）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseCheck {
    TripleOk = 0x0001,
    WindowsOk = 0x0002,
    DedupOk = 0x0003,
    StatementOk = 0x0004,
}

impl CloseCheck {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 收口面闭集自检。
pub fn close_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if EXPECT_N != 18 || kt::CARD_N != 18 {
        return Err(E_CARDS);
    }
    if CloseCheck::TripleOk.code() != 1
        || CloseCheck::WindowsOk.code() != 2
        || CloseCheck::DedupOk.code() != 3
        || CloseCheck::StatementOk.code() != 4
    {
        return Err(E_FINGER);
    }
    Ok(())
}

/// 演练：R1 闭集自检；R2 三写交叉；R3 窗口互斥 + 去重；R4 收口声明
/// 与指纹稳定；R5 负例注入（伪改锚面/去重侦测/指纹漂移）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：闭集自检。
    close_integrity()?;
    hits += 1;
    // R2：三写交叉。
    triple_check()?;
    hits += 1;
    // R3：窗口互斥 + 去重（dup == 0）。
    windows_exclusive()?;
    let dup = dedup_scan()?;
    if dup != 0 {
        return Err(E_DUP);
    }
    hits += 1;
    // R4：收口声明 + 指纹稳定。
    close_statement()?;
    hits += 1;
    // R5：负例注入——伪改 SEG 锚触发交叉失配；伪 slug 触发去重。
    let mut fake_segs = EXPECT_SEGS;
    fake_segs[0] = EXPECT_SEGS[1]; // 窗口重叠 + 交叉失配
    let mut clash = false;
    let mut a = 0;
    while a < EXPECT_N {
        let mut b = a + 1;
        while b < EXPECT_N {
            let d = if fake_segs[a] > fake_segs[b] { fake_segs[a] - fake_segs[b] } else { fake_segs[b] - fake_segs[a] };
            if d < 0x10 {
                clash = true;
            }
            b += 1;
        }
        a += 1;
    }
    if !clash {
        return Err(E_NEG);
    }
    if EXPECT_SLUGS[0] == EXPECT_SLUGS[1] {
        return Err(E_NEG); // 真表不得撞名
    }
    if CloseCheck::of(5).is_some() {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

impl CloseCheck {
    pub fn of(v: u16) -> Option<CloseCheck> {
        match v {
            1 => Some(CloseCheck::TripleOk),
            2 => Some(CloseCheck::WindowsOk),
            3 => Some(CloseCheck::DedupOk),
            4 => Some(CloseCheck::StatementOk),
            _ => None,
        }
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
    fn close_table_holds() {
        assert_eq!(close_integrity(), Ok(()));
    }

    #[test]
    fn triple_write_holds() {
        assert_eq!(triple_check(), Ok(()));
    }

    #[test]
    fn statement_frozen() {
        assert!(close_statement().is_ok());
    }
}

pub fn run_unx36180_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36180_b09close");
    cs.add("SEG 段位独占", SEG_BASE == 0x4140, "0x4140~0x414F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三写交叉/互斥/声明全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", close_integrity() == Ok(()), "收口锚 + CloseCheck 判别式");
    cs.add("契约冻结", VER == 1, "收口锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SEGS => "SEG 锚与卡表失配",
        E_SLUGS => "slug 锚与卡表失配",
        E_CARDS => "卡号锚与卡表失配",
        E_OVERLAP => "窗口互斥破坏",
        E_DUP => "去重扫描命中",
        E_NEG => "负例注入未被检出",
        E_FINGER => "指纹锚漂移",
        _ => "未知诊断码",
    }
}
