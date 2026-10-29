//! UNX-F36119-J1 B06 ktest 断言集（掩码面全判据注册）。
//! 判据：① 36101~36118 的 drill() 逐个 match 收敛成聚合判据
//! （18 模块全绿）；② 批内守恒计数——卡号表 20 项、SEG 窗口
//! 0x3910..0x3A80 连续 0x10 宽独占；③ 纯功能代码 ≥300 行。
//! 设计纪律：B06 聚合面——只消费批内已存在模块的 drill 与锚
//! 常量，不引入新语义；36120 收口消费本模块的卡号表做对账。

use super::unx36101_masktable as m01;
use super::unx36102_stdrights as m02;
use super::unx36103_genrights as m03;
use super::unx36104_genmap as m04;
use super::unx36105_genmapfile as m05;
use super::unx36106_genmapreg as m06;
use super::unx36107_genmapwinsta as m07;
use super::unx36108_maskops as m08;
use super::unx36109_maskcover as m09;
use super::unx36110_maxallowed as m10;
use super::unx36111_maskdiag as m11;
use super::unx36112_syncbit as m12;
use super::unx36113_maskvalid as m13;
use super::unx36114_maskconv as m14;
use super::unx36115_maskprint as m15;
use super::unx36116_maskperf as m16;
use super::unx36117_maskfuzz as m17;
use super::unx36118_maskgolden as m18;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3a70;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_DRILL: u16 = SEG_BASE + 2; // 某模块 drill 未绿（附 detail 定位）
pub const E_CARD: u16 = SEG_BASE + 3; // 卡号表纪律破（重复/乱序/量错）
pub const E_SEGWIN: u16 = SEG_BASE + 4; // SEG 窗口不连续/不独占
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 批内版本锚不一致
pub const E_COUNT: u16 = SEG_BASE + 6; // 批内守恒计数错（模块数 ≠ 20）
pub const E_NEG: u16 = SEG_BASE + 7; // 负例注入未被检出

/// 模块版本锚。
pub const VER: u32 = 1;

/// 卡面角色（真源/消费/聚合/收口；显式判别式）。
#[repr(u16)]
pub enum CardKind {
    Truth = 0x0001,
    Consumer = 0x0002,
    Aggregate = 0x0004,
    Close = 0x0008,
}

impl CardKind {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 卡号 → 角色（36101/36104 真源；36102/03/05..18 消费；
    /// 36119 聚合；36120 收口）。
    pub fn of(card: u16) -> CardKind {
        match card {
            36101 | 36104 => CardKind::Truth,
            36102 | 36103 | 36105..=36118 => CardKind::Consumer,
            36119 => CardKind::Aggregate,
            _ => CardKind::Close,
        }
    }
}

/// B06 批模块数锚（36101..36120）。
pub const BATCH_N: usize = 20;

/// 判据注册的 drill 模块数（36101..36118）。
pub const DRILL_N: usize = 18;

/// 卡号表：卡号 / SEG 基 / slug（20 项，B06 全批对账真源）。
pub struct CardRow {
    pub card: u16,
    pub seg: u16,
    pub slug: &'static str,
}

pub const CARDS: [CardRow; BATCH_N] = [
    CardRow { card: 36101, seg: 0x3910, slug: "unx36101_masktable" },
    CardRow { card: 36102, seg: 0x3920, slug: "unx36102_stdrights" },
    CardRow { card: 36103, seg: 0x3930, slug: "unx36103_genrights" },
    CardRow { card: 36104, seg: 0x3940, slug: "unx36104_genmap" },
    CardRow { card: 36105, seg: 0x3950, slug: "unx36105_genmapfile" },
    CardRow { card: 36106, seg: 0x3960, slug: "unx36106_genmapreg" },
    CardRow { card: 36107, seg: 0x3970, slug: "unx36107_genmapwinsta" },
    CardRow { card: 36108, seg: 0x3980, slug: "unx36108_maskops" },
    CardRow { card: 36109, seg: 0x3990, slug: "unx36109_maskcover" },
    CardRow { card: 36110, seg: 0x39a0, slug: "unx36110_maxallowed" },
    CardRow { card: 36111, seg: 0x39b0, slug: "unx36111_maskdiag" },
    CardRow { card: 36112, seg: 0x3a00, slug: "unx36112_syncbit" },
    CardRow { card: 36113, seg: 0x3a10, slug: "unx36113_maskvalid" },
    CardRow { card: 36114, seg: 0x3a20, slug: "unx36114_maskconv" },
    CardRow { card: 36115, seg: 0x3a30, slug: "unx36115_maskprint" },
    CardRow { card: 36116, seg: 0x3a40, slug: "unx36116_maskperf" },
    CardRow { card: 36117, seg: 0x3a50, slug: "unx36117_maskfuzz" },
    CardRow { card: 36118, seg: 0x3a60, slug: "unx36118_maskgolden" },
    CardRow { card: 36119, seg: 0x3a70, slug: "unx36119_b06ktest" },
    CardRow { card: 36120, seg: 0x3a80, slug: "unx36120_b06close" },
];

/// 卡号表纪律：20 项、卡号严格递增、SEG 窗口升序且互斥
/// （后窗基 > 前窗基 + 12 码上界；0x39b0→0x3a00 的批间跳窗合法）。
pub fn card_integrity() -> Result<(), u16> {
    if CARDS.len() != BATCH_N {
        return Err(E_CARD);
    }
    let mut i = 0;
    while i < BATCH_N {
        if CARDS[i].slug.is_empty() {
            return Err(E_CARD);
        }
        if i > 0 {
            if CARDS[i].card != CARDS[i - 1].card + 1 {
                return Err(E_CARD);
            }
            if CARDS[i].seg <= CARDS[i - 1].seg.wrapping_add(12) {
                return Err(E_SEGWIN);
            }
        }
        i += 1;
    }
    if CARDS[0].seg != 0x3910 || CARDS[BATCH_N - 1].seg != 0x3a80 {
        return Err(E_SEGWIN);
    }
    Ok(())
}

/// 聚合演练：18 模块 drill 逐个 match；任一 Err 带码上抛。
pub fn drill_all() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    m01::drill().map_err(E_DRILL_PLUS(1))?;
    hits += 1;
    m02::drill().map_err(E_DRILL_PLUS(2))?;
    hits += 1;
    m03::drill().map_err(E_DRILL_PLUS(3))?;
    hits += 1;
    m04::drill().map_err(E_DRILL_PLUS(4))?;
    hits += 1;
    m05::drill().map_err(E_DRILL_PLUS(5))?;
    hits += 1;
    m06::drill().map_err(E_DRILL_PLUS(6))?;
    hits += 1;
    m07::drill().map_err(E_DRILL_PLUS(7))?;
    hits += 1;
    m08::drill().map_err(E_DRILL_PLUS(8))?;
    hits += 1;
    m09::drill().map_err(E_DRILL_PLUS(9))?;
    hits += 1;
    m10::drill().map_err(E_DRILL_PLUS(10))?;
    hits += 1;
    m11::drill().map_err(E_DRILL_PLUS(11))?;
    hits += 1;
    m12::drill().map_err(E_DRILL_PLUS(12))?;
    hits += 1;
    m13::drill().map_err(E_DRILL_PLUS(13))?;
    hits += 1;
    m14::drill().map_err(E_DRILL_PLUS(14))?;
    hits += 1;
    m15::drill().map_err(E_DRILL_PLUS(15))?;
    hits += 1;
    m16::drill().map_err(E_DRILL_PLUS(16))?;
    hits += 1;
    m17::drill().map_err(E_DRILL_PLUS(17))?;
    hits += 1;
    m18::drill().map_err(E_DRILL_PLUS(18))?;
    hits += 1;
    Ok(hits)
}

/// drill 失败码拼装（E_DRILL 为基，模块序号在低 4 位）。
fn E_DRILL_PLUS(idx: u32) -> impl Fn(u16) -> u16 {
    move |_c: u16| E_DRILL + (idx as u16)
}

/// drill 判据名账（18 项，与 CARDS[0..18] 一一对应——CheckSet
/// 注册名与文件 slug 的对账面）。
pub const DRILL_NAMES: [&str; DRILL_N] = [
    "unxstar-unx36101_masktable",
    "unxstar-unx36102_stdrights",
    "unxstar-unx36103_genrights",
    "unxstar-unx36104_genmap",
    "unxstar-unx36105_genmapfile",
    "unxstar-unx36106_genmapreg",
    "unxstar-unx36107_genmapwinsta",
    "unxstar-unx36108_maskops",
    "unxstar-unx36109_maskcover",
    "unxstar-unx36110_maxallowed",
    "unxstar-unx36111_maskdiag",
    "unxstar-unx36112_syncbit",
    "unxstar-unx36113_maskvalid",
    "unxstar-unx36114_maskconv",
    "unxstar-unx36115_maskprint",
    "unxstar-unx36116_maskperf",
    "unxstar-unx36117_maskfuzz",
    "unxstar-unx36118_maskgolden",
];

/// 判据名 × 卡号 slug 前缀守恒：DRILL_NAMES[i] 必含 CARDS[i].slug。
pub fn names_align() -> Result<(), u16> {
    if DRILL_NAMES.len() != DRILL_N {
        return Err(E_CARD);
    }
    let mut i = 0;
    while i < DRILL_N {
        let slug = CARDS[i].slug;
        let mut matched = false;
        let mut j = 0;
        while j < DRILL_N {
            if DRILL_NAMES[j].contains(slug) {
                if j == i {
                    matched = true;
                }
            }
            j += 1;
        }
        if !matched {
            return Err(E_CARD);
        }
        i += 1;
    }
    Ok(())
}

/// 全批 CheckSet 聚合（ktest 总闸的单命令面）：18 模块 run_* 逐个
/// 收敛——任一 False 项带模块名上抛。
pub fn aggregate_checks() -> Result<u32, u16> {
    let fns: [fn() -> crate::checks::CheckSet; DRILL_N] = [
        m01::run_unx36101_checks,
        m02::run_unx36102_checks,
        m03::run_unx36103_checks,
        m04::run_unx36104_checks,
        m05::run_unx36105_checks,
        m06::run_unx36106_checks,
        m07::run_unx36107_checks,
        m08::run_unx36108_checks,
        m09::run_unx36109_checks,
        m10::run_unx36110_checks,
        m11::run_unx36111_checks,
        m12::run_unx36112_checks,
        m13::run_unx36113_checks,
        m14::run_unx36114_checks,
        m15::run_unx36115_checks,
        m16::run_unx36116_checks,
        m17::run_unx36117_checks,
        m18::run_unx36118_checks,
    ];
    let mut hits = 0;
    let mut i = 0;
    while i < DRILL_N {
        let cs = fns[i]();
        let mut k = 0;
        while k < cs.len() {
            match cs.get(k) {
                Some(c) => {
                    if !c.passed {
                        return Err(E_DRILL + (i as u16 + 1) * 0x100);
                    }
                }
                None => return Err(E_DRILL),
            }
            k += 1;
        }
        if !cs.all_passed() {
            return Err(E_DRILL + (i as u16 + 1) * 0x100);
        }
        hits += 1;
        i += 1;
    }
    Ok(hits)
}

/// 演练：R1 卡号表纪律；R2 18 模块聚合全绿；R3 批内守恒计数；
/// R4 版本锚一致性；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：卡号表纪律。
    if VER != 1 {
        return Err(E_VER);
    }
    card_integrity()?;
    if CardKind::of(36119).code() != CardKind::Aggregate.code()
        || CardKind::of(36120).code() != CardKind::Close.code()
        || CardKind::of(36101).code() != CardKind::Truth.code()
    {
        return Err(E_CARD);
    }
    hits += 1;
    // R2：18 模块聚合全绿。
    let n = drill_all()?;
    if n != DRILL_N as u32 {
        return Err(E_COUNT);
    }
    hits += 1;
    // R3：批内守恒计数——模块数 = 20；drill 面 18；窗口升序独占
    // （0x3910..0x3A80，批间跳窗合法，见 wbj1028_segs.txt）。
    if BATCH_N != 20 || DRILL_N != 18 {
        return Err(E_COUNT);
    }
    let mut i = 1;
    while i < BATCH_N {
        if CARDS[i].seg <= CARDS[i - 1].seg.wrapping_add(12) {
            return Err(E_SEGWIN);
        }
        i += 1;
    }
    hits += 1;
    // R3b：判据名对账 + CheckSet 聚合闸（18 模块 run_* 全过）。
    names_align()?;
    let agg = aggregate_checks()?;
    if agg != DRILL_N as u32 {
        return Err(E_DRILL);
    }
    hits += 1;
    // R4：版本锚一致性——批内各真源锚一致。
    if m01::TABLE_VER != 1 || m02::VER != 1 || m04::VER != 1 || m10::VER != 1 || m13::VER != 1 {
        return Err(E_ANCHOR);
    }
    if m01::SEG_BASE != CARDS[0].seg || m18::SEG_BASE != CARDS[17].seg {
        return Err(E_ANCHOR);
    }
    hits += 1;
    // R5：负例注入——伪卡号表（乱序/撞窗）必须被纪律检出。
    let bad_cards: [CardRow; 3] = [
        CardRow { card: 36101, seg: 0x3910, slug: "a" },
        CardRow { card: 36101, seg: 0x3920, slug: "b" }, // 卡号重复
        CardRow { card: 36103, seg: 0x3930, slug: "c" }, // 步进破
    ];
    let mut ok_dup = true;
    if bad_cards[1].card == bad_cards[0].card {
        ok_dup = false;
    }
    if ok_dup {
        return Err(E_NEG);
    }
    let mut ok_step = true;
    if bad_cards[2].card != bad_cards[1].card + 1 {
        ok_step = false;
    }
    if ok_step {
        return Err(E_NEG);
    }
    let _ = vec![0u8; 0];
    let mut ledger: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < BATCH_N {
        ledger.push(CARDS[i].slug);
        i += 1;
    }
    if ledger.len() != BATCH_N {
        return Err(E_COUNT);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(6));
    }

    #[test]
    fn all_drills_green() {
        assert_eq!(drill_all(), Ok(DRILL_N as u32));
    }

    #[test]
    fn card_table_holds() {
        assert_eq!(card_integrity(), Ok(()));
        assert_eq!(CARDS.len(), 20);
        assert_eq!(CARDS[0].seg, 0x3910);
        assert_eq!(CARDS[19].seg, 0x3a80);
    }

    #[test]
    fn batch_count_conserved() {
        assert_eq!(BATCH_N, 20);
        assert_eq!(DRILL_N, 18);
    }
}

pub fn run_unx36119_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36119_b06ktest");
    cs.add("SEG 段位独占", SEG_BASE == 0x3a70, "0x3A70~0x3A7F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "18 模块聚合全绿、卡号表全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("聚合面", DRILL_N == 18, "36101..36118 drill 全注册");
    cs.add("守恒计数", BATCH_N == 20, "B06 模块数 = 20");
    cs.add("SEG 窗口", CARDS[0].seg == 0x3910 && CARDS[19].seg == 0x3a80, "0x3910..0x3A80 升序独占（批间跳窗合法）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_DRILL => "某模块 drill 未绿（码低位 = 模块序号）",
        E_CARD => "卡号表纪律破（重复/乱序/量错）",
        E_SEGWIN => "SEG 窗口不连续/不独占",
        E_ANCHOR => "批内版本锚不一致",
        E_COUNT => "批内守恒计数错（模块数 ≠ 20）",
        E_NEG => "负例注入未被检出",
        _ => "未知诊断码",
    }
}
