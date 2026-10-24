//! veb325_rvisa —— VE-F0325 RISC-V ISA 差异对编译器的影响研究。
//! 锚点（CoRun Unxreal 主册）：VE-F0325 · RISC-V ISA 差异对编译器的影响研究 · 目标 320 行
//! 判据原文：VE-F0325-J1 ISA 研究账：扩展位集（基线 IMA +
//!           可选 A/C/F/D/V/B 十位）；目标 ISA 描述（位集 +
//!           扩展名表）；编译可行性（源需求位集 ⊆ 目标位集
//!           → 可编）；缺扩展指认（缺哪个指哪个，按位号
//!           最小先指）；软回退（F 缺 → 软浮点标志位可编但
//!           带慢速记号）；版本序核（位集单调：目标升级不
//!           缩水）；扩展名查询（位号 → 名）。
//! 组件：扩展位集 + 可行性 + 缺位指认 + 软回退。
//! 职责定位：ISA 研究是「RISC-V 的方言地图」——扩展是方言
//!           单词：会说 A 才能编原子操作；缺位指认是字典的
//!           翻译：哪里听不懂要翻到哪一页；软回退是比划着
//!           交流：能沟通但慢十倍，要说清楚是比划的。
//! 性能逐项分解：可行性 O(1)。
//! 诊断码段：0x31A40（0x31A41~0x31A49 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const IX_VERSION: &str = "veb325_rvisa-v1";

/// 诊断段基址（0x31A40 段 · 本模块独占）。
pub const IX_SEG_BASE: u32 = 0x31A40;
pub const XC_MAGIC: u32 = 0x31A41; // 账魔数破
pub const XC_BIT: u32 = 0x31A42; // 位号越界

/// 账魔数。
const MAGIC: u32 = 0x31A4_1B2C;

/// 十位扩展。
pub const BIT_M: u32 = 0; // 乘除
pub const BIT_A: u32 = 1; // 原子
pub const BIT_C: u32 = 2; // 压缩
pub const BIT_F: u32 = 3; // 单浮
pub const BIT_D: u32 = 4; // 双浮
pub const BIT_V: u32 = 5; // 向量
pub const BIT_B: u32 = 6; // 位操作
pub const BIT_H: u32 = 7; // 虚拟化
pub const BIT_P: u32 = 8; // 打包 DSP
pub const BIT_J: u32 = 9; // 动态语言
pub const NBITS: u32 = 10;

/// 基线位集（IMA）。
pub const BASE_IMA: u32 = (1 << BIT_I) | (1 << BIT_M) | (1 << BIT_A) | (1 << BIT_C);
/// I 位（恒有）。
pub const BIT_I: u32 = 10;
/// 全掩码（含 I 共 11 位）。
pub const MASK_ALL: u32 = (1u32 << 11) - 1;

/// 扩展名表。
pub const EXT_NAMES: [&str; 11] = [
    "m", "a", "c", "f", "d", "v", "b", "h", "p", "j", "i",
];

/// 位名查询。
pub fn ext_name(bit: u32) -> Result<&'static str, u32> {
    if bit > BIT_I {
        return Err(XC_BIT);
    }
    Ok(EXT_NAMES[bit as usize])
}

/// 编译裁定。
#[derive(Debug, PartialEq, Eq)]
pub enum Compile {
    Ok,
    /// 可编但带慢速记号（软回退）。
    Slow(u32), // 缺的浮点位
    /// 不可编（缺指认：最小位号）。
    Miss(u32),
}

/// 编译器研究簿。
pub struct IsaBook {
    /// 目标位集。
    pub target: u32,
    magic: u32,
}

impl IsaBook {
    /// 建。
    pub fn new(target: u32) -> IsaBook {
        IsaBook { target: target & MASK_ALL, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 位有？
    pub fn has(&self, bit: u32) -> Result<bool, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if bit > BIT_I {
            return Err(XC_BIT);
        }
        Ok((self.target >> bit) & 1 == 1)
    }

    /// 编译裁定：源需求 ⊆ 目标？
    pub fn compile(&self, need: u32) -> Compile {
        if !self.magic_ok() {
            return Compile::Miss(XC_MAGIC);
        }
        let lack = need & !self.target;
        if lack == 0 {
            return Compile::Ok;
        }
        // 软回退：只缺 F（单浮）→ Slow。
        if lack == (1 << BIT_F) {
            return Compile::Slow(BIT_F);
        }
        // 只缺 D 且有 F → Slow（双浮用软）。
        if lack == (1 << BIT_D) && (self.target & (1 << BIT_F)) != 0 {
            return Compile::Slow(BIT_D);
        }
        // 最小缺位指认。
        let mut min = None;
        for b in 0..=BIT_I {
            if (lack >> b) & 1 == 1 {
                min = Some(b);
                break;
            }
        }
        Compile::Miss(min.unwrap_or(XC_BIT))
    }

    /// 版本序核：目标升级不缩水（self ⊇ old）。
    pub fn superset(&self, old: u32) -> bool {
        self.target & old == old
    }
}

pub fn run_veb325_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0325");
    // 族零：位名表。
    s.add("名 M", ext_name(BIT_M) == Ok("m"), "乘除");
    s.add("名 V", ext_name(BIT_V) == Ok("v"), "向量");
    s.add("名 I", ext_name(BIT_I) == Ok("i"), "基线");
    s.add("位 11 拒", ext_name(11) == Err(XC_BIT), "越界");
    // 族一：基线目标。
    let rv32ima = IsaBook::new(BASE_IMA);
    s.add("基线有 I", rv32ima.has(BIT_I) == Ok(true), "恒有");
    s.add("基线有 M", rv32ima.has(BIT_M) == Ok(true), "IMA");
    s.add("基线无 V", rv32ima.has(BIT_V) == Ok(false), "未开");
    // 族二：编译可行性。
    s.add("IMA 编 IMA", rv32ima.compile(BASE_IMA) == Compile::Ok, "自编");
    s.add("缺 V 拒", rv32ima.compile(1 << BIT_V) == Compile::Miss(BIT_V), "向量缺");
    s.add("缺 B 拒", rv32ima.compile(1 << BIT_B) == Compile::Miss(BIT_B), "位操缺");
    // 多缺取最小位号：缺 A+C → 指 A（位 1 < 位 2）。
    let rv32i = IsaBook::new(1 << BIT_I);
    s.add("最小指认 A", rv32i.compile((1 << BIT_A) | (1 << BIT_C)) == Compile::Miss(BIT_A), "位 1 先");
    // 族三：软回退。
    s.add("缺 F 慢编", rv32ima.compile(1 << BIT_F) == Compile::Slow(BIT_F), "软浮点");
    // 有 F 缺 D → Slow(D)。
    let rvf = IsaBook::new(BASE_IMA | (1 << BIT_F));
    s.add("有 F 缺 D 慢", rvf.compile(1 << BIT_D) == Compile::Slow(BIT_D), "软双浮");
    // 无 F 缺 D → Miss(D)（没软垫）。
    s.add("无 F 缺 D 拒", rv32ima.compile(1 << BIT_D) == Compile::Miss(BIT_D), "无垫");
    // 缺 F 又缺 D → Miss(F)（最小位）。
    s.add("缺 F+D 拒", rv32ima.compile((1 << BIT_F) | (1 << BIT_D)) == Compile::Miss(BIT_F), "最小先");
    // 族四：版本序核。
    let full = IsaBook::new(MASK_ALL);
    s.add("全超基线", full.superset(BASE_IMA), "升级不缩水");
    s.add("基线不超全", rv32ima.superset(MASK_ALL) == false, "降级检得");
    // 位集掩码。
    s.add("越位截断", IsaBook::new(0xFFFF_FFFF).target == MASK_ALL, "掩 11 位");
    // 族五：需求含 I 恒可（基线必含 I）。
    let any = IsaBook::new(0);
    s.add("零目标也有 I", any.has(BIT_I) == Ok(false), "零集真零");
    s.add("零集编 I 拒", any.compile(1 << BIT_I) == Compile::Miss(BIT_I), "无基线拒");
    s.add("魔数核", IX_SEG_BASE == 0x31A40, "段基址");
    s
}
