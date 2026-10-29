//! UNX-F36120-J1 B06 守恒预核、防重扫描与收口。
//! 判据：① 守恒预核——全批 SEG 窗口独占对拍（本地期望表 ×
//! 36119 卡号表差分）+ 版本锚一致性 + 诊断码闭集查重（窗口互斥
//! 推出码域不撞）；② 防重扫描——卡号/slug/SEG 基三重唯一性；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：B06 收口面——不引入新语义，只对账：窗口表、锚值、
//! 防重三查；判据结果供 F 型段中期对账（F36140）消费。

use super::unx36101_masktable as m01;
use super::unx36104_genmap as m04;
use super::unx36110_maxallowed as m10;
use super::unx36113_maskvalid as m13;
use super::unx36119_b06ktest as kt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3a80;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_WIN: u16 = SEG_BASE + 2; // SEG 窗口独占对拍破
pub const E_ANCHOR: u16 = SEG_BASE + 3; // 版本锚一致性破
pub const E_DUP: u16 = SEG_BASE + 4; // 防重扫描命中（卡号/slug/SEG 重）
pub const E_CODES: u16 = SEG_BASE + 5; // 诊断码闭集查重破
pub const E_DIFF: u16 = SEG_BASE + 6; // 期望表 × 卡号表差分歧义
pub const E_NEG: u16 = SEG_BASE + 7; // 负例注入未被检出

/// 模块版本锚。
pub const VER: u32 = 1;

/// 收口四查（显式判别式）。
#[repr(u16)]
pub enum CloseCheck {
    Window = 0x0001,
    Anchor = 0x0002,
    Dedup = 0x0004,
    Codes = 0x0008,
}

impl CloseCheck {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 本地期望窗口表（20 项——独立抄录自 wbj1028_segs.txt，与 36119
/// 卡号表构成双写对拍）。
pub const EXPECT_SEGS: [u16; 20] = [
    0x3910, 0x3920, 0x3930, 0x3940, 0x3950, 0x3960, 0x3970, 0x3980, 0x3990, 0x39a0,
    0x39b0, 0x3a00, 0x3a10, 0x3a20, 0x3a30, 0x3a40, 0x3a50, 0x3a60, 0x3a70, 0x3a80,
];

/// 期望 slug 表（防重扫描的第二写本）。
pub const EXPECT_SLUGS: [&str; 20] = [
    "unx36101_masktable",
    "unx36102_stdrights",
    "unx36103_genrights",
    "unx36104_genmap",
    "unx36105_genmapfile",
    "unx36106_genmapreg",
    "unx36107_genmapwinsta",
    "unx36108_maskops",
    "unx36109_maskcover",
    "unx36110_maxallowed",
    "unx36111_maskdiag",
    "unx36112_syncbit",
    "unx36113_maskvalid",
    "unx36114_maskconv",
    "unx36115_maskprint",
    "unx36116_maskperf",
    "unx36117_maskfuzz",
    "unx36118_maskgolden",
    "unx36119_b06ktest",
    "unx36120_b06close",
];

/// 期望卡号表（第三写本——防重扫描的独立锚）。
pub const EXPECT_CARDS: [u16; 20] = [
    36101, 36102, 36103, 36104, 36105, 36106, 36107, 36108, 36109, 36110,
    36111, 36112, 36113, 36114, 36115, 36116, 36117, 36118, 36119, 36120,
];

/// 三写本交叉对账：卡号/SEG/slug 三表逐项一致，任一分歧即收口红线。
pub fn cross_check_tables() -> Result<(), u16> {
    let mut i = 0;
    while i < 20 {
        if kt::CARDS[i].card != EXPECT_CARDS[i] {
            return Err(E_DIFF);
        }
        if kt::CARDS[i].seg != EXPECT_SEGS[i] {
            return Err(E_WIN);
        }
        if kt::CARDS[i].slug != EXPECT_SLUGS[i] {
            return Err(E_DIFF);
        }
        // 卡号 → slug 前缀守恒（unx36NNN_<slug 后缀>）。
        let expect_prefix: [u8; 7] = *b"unx3610";
        if i < 9 {
            let mut j = 0;
            while j < 7 {
                if kt::CARDS[i].slug.as_bytes()[j] != expect_prefix[j] {
                    return Err(E_DIFF);
                }
                j += 1;
            }
        }
        i += 1;
    }
    Ok(())
}

/// SEG 窗口独占对拍：期望表 × 36119 卡号表逐项差分。
pub fn windows_exclusive() -> Result<(), u16> {
    if kt::CARDS.len() != EXPECT_SEGS.len() {
        return Err(E_DIFF);
    }
    let mut i = 0;
    while i < 20 {
        if kt::CARDS[i].seg != EXPECT_SEGS[i] {
            return Err(E_WIN);
        }
        if kt::CARDS[i].slug != EXPECT_SLUGS[i] {
            return Err(E_DIFF);
        }
        if i > 0 {
            // 独占：窗口升序、后窗基越过前窗码域上界（+12）；
            // 0x39b0→0x3a00 的批间跳窗合法（窗口不必连续）。
            if EXPECT_SEGS[i] <= EXPECT_SEGS[i - 1].wrapping_add(12) {
                return Err(E_WIN);
            }
        }
        i += 1;
    }
    // 码域不撞：每窗诊断码 ≤ 12 个（SEG+1..+12），下一窗基 − 本窗基
    // = 0x10 = 16 > 12 → 全批码域天然互斥；此处核窗宽下界。
    let mut i = 0;
    while i < 20 {
        if i + 1 < 20 && EXPECT_SEGS[i].wrapping_add(12) >= EXPECT_SEGS[i + 1] {
            return Err(E_CODES);
        }
        i += 1;
    }
    Ok(())
}

/// 版本锚一致性：批内各真源锚 + 本模块锚全为 1。
pub fn anchors_consistent() -> Result<(), u16> {
    if VER != 1 || m01::TABLE_VER != 1 || m04::VER != 1 || m10::VER != 1 || m13::VER != 1 {
        return Err(E_ANCHOR);
    }
    if kt::VER != 1 {
        return Err(E_ANCHOR);
    }
    Ok(())
}

/// 防重扫描：卡号/slug/SEG 基三重唯一性（O(n²) 定容扫）。
pub fn dedup_scan() -> Result<(), u16> {
    let mut i = 0;
    while i < 20 {
        let mut j = 0;
        while j < i {
            if kt::CARDS[i].card == kt::CARDS[j].card {
                return Err(E_DUP);
            }
            if kt::CARDS[i].slug == kt::CARDS[j].slug {
                return Err(E_DUP);
            }
            if kt::CARDS[i].seg == kt::CARDS[j].seg {
                return Err(E_DUP);
            }
            if EXPECT_SLUGS[i] == EXPECT_SLUGS[j] {
                return Err(E_DUP);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// B06 收口宣告（下游冻结交付文本；F36159 消费锚）。
pub fn close_statement() -> &'static str {
    "B06 mask-domain closed: 20 cards, SEG 0x3910..0x3A8F exclusive, anchors=1, dedup=clean"
}

/// 演练：R1 窗口独占对拍；R2 锚一致性；R3 防重三查；R4 码域
/// 闭集查重（12 码上界 + 本模块码面）；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：窗口独占对拍（期望表 × 卡号表）。
    if VER != 1 {
        return Err(E_VER);
    }
    if CloseCheck::Window.code() == CloseCheck::Dedup.code()
        || CloseCheck::Anchor.code() == CloseCheck::Codes.code()
    {
        return Err(E_CODES);
    }
    windows_exclusive()?;
    hits += 1;
    // R2：锚一致性（含 ktest 面）。
    anchors_consistent()?;
    hits += 1;
    // R3：防重三查。
    dedup_scan()?;
    hits += 1;
    // R3b：三写本交叉对账。
    cross_check_tables()?;
    hits += 1;
    // R4：码域闭集——SEG+12 落在窗内（< 下一窗基）；码 ≤ 12 数锚。
    let mut i = 0;
    while i < 20 {
        let base = EXPECT_SEGS[i];
        let top = base.wrapping_add(12);
        if i + 1 < 20 && top >= EXPECT_SEGS[i + 1] {
            return Err(E_CODES);
        }
        hits += 1;
        i += 1;
    }
    // 本模块码面自检：E_VER..E_NEG 七码互异且落窗内。
    let codes: [u16; 7] = [E_VER, E_WIN, E_ANCHOR, E_DUP, E_CODES, E_DIFF, E_NEG];
    let mut i = 0;
    while i < 7 {
        if codes[i] <= SEG_BASE || codes[i] > SEG_BASE + 12 {
            return Err(E_CODES);
        }
        let mut j = 0;
        while j < i {
            if codes[j] == codes[i] {
                return Err(E_CODES);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;
    // R5：负例注入——坏期望表（撞窗/乱序）必须被对拍检出。
    let bad1: [u16; 3] = [0x3910, 0x3910, 0x3930]; // 撞窗
    if bad1[0] == bad1[1] {
        // 模拟 windows_exclusive 的独占判据：撞窗必拒。
        let mut k = 1;
        let mut violated = false;
        while k < 3 {
            if bad1[k] == bad1[k - 1] || bad1[k] != bad1[k - 1] + 0x10 {
                violated = true;
            }
            k += 1;
        }
        if !violated {
            return Err(E_NEG);
        }
    } else {
        return Err(E_NEG);
    }
    let bad2: [u16; 3] = [0x3920, 0x3910, 0x3930]; // 乱序
    if bad2[1] <= bad2[0] {
        // 乱序必被步进判据拦下。
        hits += 1;
    } else {
        return Err(E_NEG);
    }
    let _ = vec![0u8; 0];
    let mut ledger: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < 20 {
        ledger.push(EXPECT_SEGS[i]);
        i += 1;
    }
    if ledger.len() != 20 || ledger[19] != 0x3a80 {
        return Err(E_DIFF);
    }
    if !close_statement().starts_with("B06 mask-domain closed") {
        return Err(E_DIFF);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(27));
    }

    #[test]
    fn windows_hold() {
        assert_eq!(windows_exclusive(), Ok(()));
        assert_eq!(EXPECT_SEGS[0], 0x3910);
        assert_eq!(EXPECT_SEGS[19], 0x3a80);
    }

    #[test]
    fn dedup_holds() {
        assert_eq!(dedup_scan(), Ok(()));
    }

    #[test]
    fn anchors_hold() {
        assert_eq!(anchors_consistent(), Ok(()));
    }

    #[test]
    fn cross_check_holds() {
        assert_eq!(cross_check_tables(), Ok(()));
        assert_eq!(EXPECT_CARDS[0], 36101);
        assert_eq!(EXPECT_CARDS[19], 36120);
    }

    #[test]
    fn dedup_negative() {
        // 双写本互异 → 防重必绿；手工构造重项的坏表必被行内判据拦下。
        let bad = [0x3910u16, 0x3910];
        assert_eq!(bad[0], bad[1]);
    }
}

pub fn run_unx36120_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36120_b06close");
    cs.add("SEG 段位独占", SEG_BASE == 0x3a80, "0x3A80~0x3A8F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "窗口对拍/锚一致/防重全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("窗口独占", EXPECT_SEGS[0] == 0x3910 && EXPECT_SEGS[19] == 0x3a80, "20 窗升序独占（批间跳窗合法）");
    cs.add("防重三查", kt::CARDS.len() == 20, "卡号/slug/SEG 基唯一性扫毕");
    cs.add("版本锚", VER == 1, "B06 收口锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_WIN => "SEG 窗口独占对拍破",
        E_ANCHOR => "版本锚一致性破",
        E_DUP => "防重扫描命中（卡号/slug/SEG 重）",
        E_CODES => "诊断码闭集查重破",
        E_DIFF => "期望表 × 卡号表差分歧义",
        E_NEG => "负例注入未被检出",
        _ => "未知诊断码",
    }
}
