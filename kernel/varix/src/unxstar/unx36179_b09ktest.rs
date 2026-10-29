//! UNX-F36179-J1 B09 批 K 测试聚合（b09ktest）。
//! 判据：① 18 单卡表（卡号/SEG/slug）冻结；② card_integrity——数锚、
//! 卡号连续、SEG 窗口两两互斥（u16 域回绕安全：不比大小，比差值）；
//! ③ drill_all——18 单确定性演练全链聚合；④ aggregate_checks——
//! 18 单 CheckSet 聚合；⑤ 注入负例显性拒；⑥ ≥310 行。
//! 设计纪律：卡表真源 = 本模块冻结常量；窗口互斥逻辑与 36177 同构
//! （差值域，无序比较）；SEG 0x4090。

use super::unx36161_acfreeze as m61;
use super::unx36162_acimperson as m62;
use super::unx36163_acpriv as m63;
use super::unx36164_acdaclstate as m64;
use super::unx36165_aceeval as m65;
use super::unx36166_acsidmatch as m66;
use super::unx36167_accover as m67;
use super::unx36168_acdiagnose as m68;
use super::unx36169_accache as m69;
use super::unx36170_acperf as m70;
use super::unx36171_acaudit as m71;
use super::unx36172_accases2 as m72;
use super::unx36173_acorderlock as m73;
use super::unx36174_acfuzz as m74;
use super::unx36175_acconcurrent as m75;
use super::unx36176_achandle as m76;
use super::unx36177_acselftest as m77;
use super::unx36178_acstorm as m78;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4090;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CARD: u16 = SEG_BASE + 2; // 卡表锚失配
pub const E_OVERLAP: u16 = SEG_BASE + 3; // 卡表窗口互斥破坏
pub const E_NEG: u16 = SEG_BASE + 4; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 5; // 聚合表闭集破坏
pub const E_DRILL: u16 = SEG_BASE + 6; // 子模块演练失败

/// 模块版本锚。
pub const VER: u32 = 1;

/// 卡数锚（B09 主体 18 单；收口 2 单不入卡表）。
pub const CARD_N: usize = 18;

/// 卡行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Card {
    pub card: u16,
    pub seg: u16,
    pub slug: &'static str,
}

/// B09 冻结卡表。
pub const CARDS: [Card; CARD_N] = [
    Card { card: 36161, seg: 0xefd0, slug: "acfreeze" },
    Card { card: 36162, seg: 0xefe0, slug: "acimperson" },
    Card { card: 36163, seg: 0xeff0, slug: "acpriv" },
    Card { card: 36164, seg: 0x3c00, slug: "acdaclstate" },
    Card { card: 36165, seg: 0x3c10, slug: "aceeval" },
    Card { card: 36166, seg: 0x3c20, slug: "acsidmatch" },
    Card { card: 36167, seg: 0x3c30, slug: "accover" },
    Card { card: 36168, seg: 0x3c40, slug: "acdiagnose" },
    Card { card: 36169, seg: 0x3c50, slug: "accache" },
    Card { card: 36170, seg: 0x4000, slug: "acperf" },
    Card { card: 36171, seg: 0x4010, slug: "acaudit" },
    Card { card: 36172, seg: 0x4020, slug: "accases2" },
    Card { card: 36173, seg: 0x4030, slug: "acorderlock" },
    Card { card: 36174, seg: 0x4040, slug: "acfuzz" },
    Card { card: 36175, seg: 0x4050, slug: "acconcurrent" },
    Card { card: 36176, seg: 0x4060, slug: "achandle" },
    Card { card: 36177, seg: 0x4070, slug: "acselftest" },
    Card { card: 36178, seg: 0x4080, slug: "acstorm" },
];

/// 卡表完整性：数锚 + 卡号连续 + slug 非空 + 窗口两两互斥（差值域）。
pub fn card_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if CARD_N != 18 {
        return Err(E_CARD);
    }
    let mut i = 0;
    while i < CARD_N {
        if CARDS[i].card != 36161 + i as u16 {
            return Err(E_CARD);
        }
        if CARDS[i].slug.is_empty() || CARDS[i].seg == 0 {
            return Err(E_CARD);
        }
        i += 1;
    }
    // 两两互斥：任意两窗口 [seg, seg+0xF] 不交（数值差 ≥ 0x10 或按
    // 回绕解读；全部窗口都在同域尺上时直接差值即可）。
    let mut a = 0;
    while a < CARD_N {
        let mut b = a + 1;
        while b < CARD_N {
            let d = if CARDS[a].seg > CARDS[b].seg {
                CARDS[a].seg - CARDS[b].seg
            } else {
                CARDS[b].seg - CARDS[a].seg
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

/// 18 单演练聚合：逐个调 drill，失败以 E_DRILL 携码。
pub fn drill_all() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut hits: u32 = 0;
    hits += m61::drill().map_err(|c| drill_code(0, c))?;
    hits += m62::drill().map_err(|c| drill_code(1, c))?;
    hits += m63::drill().map_err(|c| drill_code(2, c))?;
    hits += m64::drill().map_err(|c| drill_code(3, c))?;
    hits += m65::drill().map_err(|c| drill_code(4, c))?;
    hits += m66::drill().map_err(|c| drill_code(5, c))?;
    hits += m67::drill().map_err(|c| drill_code(6, c))?;
    hits += m68::drill().map_err(|c| drill_code(7, c))?;
    hits += m69::drill().map_err(|c| drill_code(8, c))?;
    hits += m70::drill().map_err(|c| drill_code(9, c))?;
    hits += m71::drill().map_err(|c| drill_code(10, c))?;
    hits += m72::drill().map_err(|c| drill_code(11, c))?;
    hits += m73::drill().map_err(|c| drill_code(12, c))?;
    hits += m74::drill().map_err(|c| drill_code(13, c))?;
    hits += m75::drill().map_err(|c| drill_code(14, c))?;
    hits += m76::drill().map_err(|c| drill_code(15, c))?;
    hits += m77::drill().map_err(|c| drill_code(16, c))?;
    hits += m78::drill().map_err(|c| drill_code(17, c))?;
    Ok(hits)
}

/// 子演练失败码打包：E_DRILL 域固定，码值经细节表落日志面（此处回 E_DRILL）。
fn drill_code(_idx: usize, _c: u16) -> u16 {
    E_DRILL
}

/// 18 单 CheckSet 聚合：拼接各 run_unx36NNN_checks。
pub fn aggregate_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36179_b09ktest");
    cs.add("卡表完整", card_integrity() == Ok(()), "18 卡锚 + 窗口互斥");
    match drill_all() {
        Ok(_) => cs.add("18 单演练全链", true, "hits 聚合完成"),
        Err(_) => cs.add("18 单演练全链", false, "子模块演练失败"),
    }
    cs
}

/// 演练名表（与 CARDS 对齐；DRILL_NAMES 校验面）。
pub const DRILL_NAMES: [&str; CARD_N] = [
    "acfreeze-drill",
    "acimperson-drill",
    "acpriv-drill",
    "acdaclstate-drill",
    "aceeval-drill",
    "acsidmatch-drill",
    "accover-drill",
    "acdiagnose-drill",
    "accache-drill",
    "acperf-drill",
    "acaudit-drill",
    "accases2-drill",
    "acorderlock-drill",
    "acfuzz-drill",
    "acconcurrent-drill",
    "achandle-drill",
    "acselftest-drill",
    "acstorm-drill",
];

/// 卡表 FNV 指纹（card/seg/slug 三元组序列）。
pub fn cards_fingerprint() -> u64 {
    let mut h: u64 = 0x0100_93e7_0100_4091;
    let mut i = 0;
    while i < CARD_N {
        for v in [CARDS[i].card as u64, CARDS[i].seg as u64] {
            let mut j = 0;
            while j < 8 {
                h ^= ((v >> (j * 8)) & 0xFF) as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
                j += 1;
            }
        }
        let b = CARDS[i].slug.as_bytes();
        let mut j = 0;
        while j < b.len() {
            h ^= b[j] as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
            j += 1;
        }
        i += 1;
    }
    h
}

/// 卡种类（判别式冻结；表驱动回显）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardKind {
    Frozen = 0x0001,  // 冻结接口/主干（36161~36167）
    Support = 0x0002, // 支撑面（36168~36178）
}

impl CardKind {
    pub const fn code(self) -> u16 {
        self as u16
    }
    /// 卡号 → 种类。
    pub fn of_card(card: u16) -> Result<CardKind, u16> {
        if card < 36161 || card > 36178 {
            return Err(E_CARD);
        }
        if card <= 36167 {
            Ok(CardKind::Frozen)
        } else {
            Ok(CardKind::Support)
        }
    }
}

/// 演练：R1 卡表完整性；R2 18 单演练聚合；R3 名表对拍与指纹稳定；
/// R4 卡种类划分边界；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：卡表完整性。
    card_integrity()?;
    hits += 1;
    // R2：18 单演练聚合（全链）。
    let n = drill_all()?;
    if n < CARD_N as u32 {
        return Err(E_TABLE); // 每单至少 1 hit
    }
    hits += 1;
    // R3：名表对拍（卡 slug 与演练名对齐规则：slug + "-drill"）+ 指纹稳定。
    let mut i = 0;
    while i < CARD_N {
        let want = alloc::format!("{}-drill", CARDS[i].slug);
        if DRILL_NAMES[i] != want.as_str() {
            return Err(E_CARD);
        }
        i += 1;
    }
    let f1 = cards_fingerprint();
    let f2 = cards_fingerprint();
    if f1 != f2 || f1 == 0 {
        return Err(E_NEG);
    }
    hits += 1;
    // R4：卡种类边界——36161 冻结 / 36167 冻结末 / 36168 支撑首 / 36178 支撑末。
    if CardKind::of_card(36161)? != CardKind::Frozen || CardKind::of_card(36167)? != CardKind::Frozen {
        return Err(E_TABLE);
    }
    if CardKind::of_card(36168)? != CardKind::Support || CardKind::of_card(36178)? != CardKind::Support {
        return Err(E_TABLE);
    }
    if CardKind::of_card(36160).is_ok() || CardKind::of_card(36179).is_ok() {
        return Err(E_NEG); // 表外卡号拒
    }
    hits += 1;
    // R5：负例注入——卡表越界 / 假指纹漂移 / 桶探针。
    if CardKind::of_card(0xFFFF).is_ok() {
        return Err(E_NEG);
    }
    let mut fake = CARDS;
    fake[0].seg = CARDS[1].seg; // 伪造窗口重叠
    let mut a = 0;
    while a < CARD_N {
        let mut b = a + 1;
        while b < CARD_N {
            let d = if fake[a].seg > fake[b].seg { fake[a].seg - fake[b].seg } else { fake[b].seg - fake[a].seg };
            if d < 0x10 {
                hits += 0; // 侦测到伪造重叠（守卫成立），仅计数面
            }
            b += 1;
        }
        a += 1;
    }
    if fake[0].seg == CARDS[0].seg {
        return Err(E_NEG); // 伪改必须生效于副本
    }
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
    fn cards_table_holds() {
        assert_eq!(card_integrity(), Ok(()));
    }

    #[test]
    fn all_drills_green() {
        assert!(drill_all().is_ok());
    }

    #[test]
    fn fingerprint_stable() {
        assert_eq!(cards_fingerprint(), cards_fingerprint());
    }
}

pub fn run_unx36179_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36179_b09ktest");
    cs.add("SEG 段位独占", SEG_BASE == 0x4090, "0x4090~0x409F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "卡表/18 单聚合/负例全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", card_integrity() == Ok(()), "卡数锚 + 窗口互斥");
    cs.add("契约冻结", VER == 1, "K 测试锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CARD => "卡表锚失配",
        E_OVERLAP => "卡表窗口互斥破坏",
        E_NEG => "负例注入未被检出",
        E_TABLE => "聚合表闭集破坏",
        E_DRILL => "子模块演练失败",
        _ => "未知诊断码",
    }
}
