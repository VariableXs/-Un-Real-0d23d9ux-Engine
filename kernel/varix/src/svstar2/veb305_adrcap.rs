//! veb305_adrcap —— VE-F0305 Adreno 能力位。
//! 锚点（CoRun Unxreal 主册）：VE-F0305 · Adreno 能力位 · 目标 300 行
//! 判据原文：VE-F0305-J1 能力位账：十位固定（LRZ / Binning /
//!           UBWC / ASTC / FlexVAR / 保守捕获 / 异步 UCHE /
//!           双源混合 / 多视图 / 光追探针）；代际位集（A5 /
//!           A6 / A7 三代各位集递增）；位单调核（7 系位集 ⊇
//!           6 系 ⊇ 5 系）；需求核（需求位 ⊆ 位集）；特有位
//!           （A7 独有光追探针位）；位描述查询（位号 → 名）。
//! 组件：代际位集 + 单调核 + 需求核 + 位名表。
//! 职责定位：能力位是「Adreno 的徽章墙」——十位是十枚徽章：
//!           代越高徽章越多且不回收（单调）；光追探针是 7 系
//!           的独门徽章：别系挂不上；需求核是查岗：缺哪枚徽
//!           章就进不了那道门。
//! 性能逐项分解：查位 O(1)。
//! 诊断码段：0x31540（0x31541~0x31549 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const AC_VERSION: &str = "veb305_adrcap-v1";

/// 诊断段基址（0x31540 段 · 本模块独占）。
pub const AC_SEG_BASE: u32 = 0x31540;
pub const XC_MAGIC: u32 = 0x31541; // 账魔数破
pub const XC_BIT: u32 = 0x31542; // 位号越界

/// 账魔数。
const MAGIC: u32 = 0x3154_1B0F;

/// 位号定义。
pub const BIT_LRZ: u32 = 0;
pub const BIT_BINNING: u32 = 1;
pub const BIT_UBWC: u32 = 2;
pub const BIT_ASTC: u32 = 3;
pub const BIT_FLEXVAR: u32 = 4;
pub const BIT_CCP: u32 = 5;
pub const BIT_UCHE_ASYNC: u32 = 6;
pub const BIT_DUAL_SRC: u32 = 7;
pub const BIT_MULTIVIEW: u32 = 8;
pub const BIT_RT_PROBE: u32 = 9;
pub const NBITS: u32 = 10;

/// A5 位集。
pub const CAPS_A5: u32 = (1 << BIT_LRZ) | (1 << BIT_BINNING) | (1 << BIT_ASTC);
/// A6 位集（含 A5 全部 + 6 系新增）。
pub const CAPS_A6: u32 = CAPS_A5
    | (1 << BIT_UBWC)
    | (1 << BIT_FLEXVAR)
    | (1 << BIT_CCP)
    | (1 << BIT_UCHE_ASYNC)
    | (1 << BIT_MULTIVIEW);
/// A7 位集（含 A6 全部 + 7 系新增）。
pub const CAPS_A7: u32 = CAPS_A6 | (1 << BIT_DUAL_SRC) | (1 << BIT_RT_PROBE);

/// 位名表。
pub const BIT_NAMES: [&str; 10] = [
    "lrz", "binning", "ubwc", "astc", "flexvar", "ccp", "uche_async", "dual_src", "multiview",
    "rt_probe",
];

/// 代际。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AGen {
    A5,
    A6,
    A7,
}

/// 代际位集查询。
pub fn gen_caps(g: AGen) -> u32 {
    match g {
        AGen::A5 => CAPS_A5,
        AGen::A6 => CAPS_A6,
        AGen::A7 => CAPS_A7,
    }
}

/// 能力簿。
pub struct CapBook {
    pub gen: AGen,
    magic: u32,
}

impl CapBook {
    /// 建。
    pub fn new(gen: AGen) -> CapBook {
        CapBook { gen, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 位存在？
    pub fn has(&self, bit: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if bit >= NBITS {
            return Err(XC_BIT);
        }
        Ok((gen_caps(self.gen) >> bit) & 1 == 1)
    }

    /// 位名查询。
    pub fn name(bit: u32) -> Result<&'static str, u32> {
        if bit >= NBITS {
            return Err(XC_BIT);
        }
        Ok(BIT_NAMES[bit as usize])
    }

    /// 需求核。
    pub fn require(&self, need: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if need & !((1u32 << NBITS) - 1) != 0 {
            return Err(XC_BIT);
        }
        if need & !gen_caps(self.gen) != 0 {
            return Err(XC_BIT);
        }
        Ok(())
    }
}

/// 单调核：A7 ⊇ A6 ⊇ A5。
pub fn monotonic() -> bool {
    let c5 = gen_caps(AGen::A5);
    let c6 = gen_caps(AGen::A6);
    let c7 = gen_caps(AGen::A7);
    (c6 & c5) == c5 && (c7 & c6) == c6
}

pub fn run_veb305_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0305");
    // 族一：代际位集。
    let b5 = CapBook::new(AGen::A5);
    let b6 = CapBook::new(AGen::A6);
    let b7 = CapBook::new(AGen::A7);
    s.add("A5 有 LRZ", b5.has(BIT_LRZ) == Ok(true), "基础");
    s.add("A5 无 UBWC", b5.has(BIT_UBWC) == Ok(false), "6 系才有");
    s.add("A6 有 UBWC", b6.has(BIT_UBWC) == Ok(true), "新增");
    s.add("A6 无双源", b6.has(BIT_DUAL_SRC) == Ok(false), "7 系才有");
    s.add("A7 有双源", b7.has(BIT_DUAL_SRC) == Ok(true), "新增");
    s.add("A7 独光追", b7.has(BIT_RT_PROBE) == Ok(true) && b6.has(BIT_RT_PROBE) == Ok(false), "特有位");
    // 位号越界。
    s.add("位 10 拒", b7.has(10) == Err(XC_BIT), "越界");
    s.add("位名越界拒", CapBook::name(10) == Err(XC_BIT), "越界");
    s.add("位名 LRZ", CapBook::name(0) == Ok("lrz"), "名表");
    s.add("位名光追", CapBook::name(9) == Ok("rt_probe"), "名表");
    // 族二：单调核。
    s.add("位集单调", monotonic(), "7⊇6⊇5");
    s.add("A5 子 A6", gen_caps(AGen::A5) & gen_caps(AGen::A6) == gen_caps(AGen::A5), "含等");
    s.add("A6 子 A7", gen_caps(AGen::A6) & gen_caps(AGen::A7) == gen_caps(AGen::A6), "含等");
    // 位数递增。
    s.add("位计数 3", gen_caps(AGen::A5).count_ones() == 3, "A5");
    s.add("位计数 8", gen_caps(AGen::A6).count_ones() == 8, "A6");
    s.add("位计数 10", gen_caps(AGen::A7).count_ones() == 10, "A7 满");
    // 族三：需求核。
    s.add("A5 满足 LRZ", b5.require(1 << BIT_LRZ).is_ok(), "单需求");
    s.add("A5 缺 UBWC", b5.require(1 << BIT_UBWC) == Err(XC_BIT), "缺位");
    s.add("A6 满足 UBWC", b6.require(1 << BIT_UBWC).is_ok(), "有位");
    s.add("A6 缺光追", b6.require(1 << BIT_RT_PROBE) == Err(XC_BIT), "特有位挡");
    s.add("A7 满足光追", b7.require(1 << BIT_RT_PROBE).is_ok(), "全绿");
    s.add("组合需求 A6", b6.require((1 << BIT_LRZ) | (1 << BIT_MULTIVIEW)).is_ok(), "组合");
    s.add("组合缺位 A5", b5.require((1 << BIT_LRZ) | (1 << BIT_MULTIVIEW)) == Err(XC_BIT), "部分缺");
    s.add("越位需求拒", b7.require(1 << 10) == Err(XC_BIT), "位外");
    s.add("零需求过", b5.require(0).is_ok(), "空需");
    // 族四：位名互异。
    let mut uniq = true;
    for i in 0..NBITS {
        for j in (i + 1)..NBITS {
            if CapBook::name(i) == CapBook::name(j) {
                uniq = false;
            }
        }
    }
    s.add("名互异", uniq, "十名十样");
    s.add("魔数核", AC_SEG_BASE == 0x31540, "段基址");
    s
}
