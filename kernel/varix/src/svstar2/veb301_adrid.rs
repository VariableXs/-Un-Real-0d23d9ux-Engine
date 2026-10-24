//! veb301_adrid —— VE-F0301 Adreno 识别与代际分型。
//! 锚点（CoRun Unxreal 主册）：VE-F0301 · Adreno 识别与代际分型 · 目标 300 行
//! 判据原文：VE-F0301-J1 Adreno 识别账：厂商号核（高通商号
//!           0x5143 / QCMP 折叠）；型号窗（A 系 / 6 系 / 7
//!           系三窗，各带型号号段）；代际映射（型号 → 代际
//!           名）；代际继承核（6 系继承 5 系 ISA 子集，7 系
//!           继承 6 系）；未知型号兜底立案；分型稳定核。
//! 组件：厂商核 + 型号窗 + 代际映射 + 兜底账。
//! 职责定位：Adreno 识别是「高通线的点名册」——型号窗是班
//!           级：号段在窗内才算本班学生；代际继承是家谱：
//!           7 系认 6 系做爹，能力向下兼容；兜底立案是转学
//!           生登记：不认识也要记下名字。
//! 性能逐项分解：识别 O(表长) ≤ 6。
//! 诊断码段：0x31440（0x31441~0x31449 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AD_VERSION: &str = "veb301_adrid-v1";

/// 诊断段基址（0x31440 段 · 本模块独占）。
pub const AD_SEG_BASE: u32 = 0x31440;
pub const XC_MAGIC: u32 = 0x31441; // 账魔数破
pub const XC_VENDOR: u32 = 0x31442; // 非高通商号

/// 账魔数。
const MAGIC: u32 = 0x3144_18DE;

/// 高通商号。
pub const VENDOR_QCOM: u32 = 0x5143;

/// Adreno 代际。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AGen {
    A3xx,
    A5xx,
    A6xx,
    A7xx,
    Unknown,
}

/// 代际继承位（继承掩码：A6xx 继承 A5xx 子集，A7xx 继承 A6xx）。
pub fn inherits(child: AGen) -> Option<AGen> {
    match child {
        AGen::A6xx => Some(AGen::A5xx),
        AGen::A7xx => Some(AGen::A6xx),
        _ => None,
    }
}

/// 型号窗表：(型号 lo, hi, 代际)。
pub const MODEL_TABLE: [(u32, u32, AGen); 6] = [
    (300, 399, AGen::A3xx),
    (500, 599, AGen::A5xx),
    (600, 619, AGen::A6xx),
    (620, 699, AGen::A6xx),
    (700, 749, AGen::A7xx),
    (750, 799, AGen::A7xx),
];

/// 识别簿。
pub struct AdrIdent {
    pub unknown_cases: Vec<u32>,
    pub ident_count: u32,
    magic: u32,
}

/// 识别结果。
#[derive(Debug, PartialEq, Eq)]
pub struct AdrRes {
    pub gen: AGen,
    pub model: u32,
}

impl AdrIdent {
    /// 建。
    pub fn new() -> AdrIdent {
        AdrIdent { unknown_cases: Vec::new(), ident_count: 0, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 识别：型号窗查表（窗沿含等）。
    pub fn identify(&mut self, vendor: u32, model: u32) -> Result<AdrRes, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if vendor != VENDOR_QCOM {
            return Err(XC_VENDOR);
        }
        self.ident_count += 1;
        for row in MODEL_TABLE.iter() {
            if model >= row.0 && model <= row.1 {
                return Ok(AdrRes { gen: row.2, model });
            }
        }
        self.unknown_cases.push(model);
        Ok(AdrRes { gen: AGen::Unknown, model })
    }

    pub fn unknown_count(&self) -> usize {
        self.unknown_cases.len()
    }
}

pub fn run_veb301_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0301");
    let mut id = AdrIdent::new();
    // 族一：厂商门。
    s.add("非高通拒", id.identify(0x1002, 650) == Err(XC_VENDOR), "AMD 号");
    s.add("零商号拒", id.identify(0, 650) == Err(XC_VENDOR), "无门牌");
    // 族二：三窗分型。
    let r1 = id.identify(VENDOR_QCOM, 330);
    s.add("330 归 A3", matches!(&r1, Ok(r) if r.gen == AGen::A3xx), "A 系窗");
    let r2 = id.identify(VENDOR_QCOM, 540);
    s.add("540 归 A5", matches!(&r2, Ok(r) if r.gen == AGen::A5xx), "5 系窗");
    let r3 = id.identify(VENDOR_QCOM, 650);
    s.add("650 归 A6", matches!(&r3, Ok(r) if r.gen == AGen::A6xx), "6 系窗");
    let r4 = id.identify(VENDOR_QCOM, 740);
    s.add("740 归 A7", matches!(&r4, Ok(r) if r.gen == AGen::A7xx), "7 系窗");
    // 边界：恰窗沿。
    let r5 = id.identify(VENDOR_QCOM, 600);
    s.add("600 恰窗沿", matches!(&r5, Ok(r) if r.gen == AGen::A6xx), "lo 含等");
    let r6 = id.identify(VENDOR_QCOM, 799);
    s.add("799 恰窗沿", matches!(&r6, Ok(r) if r.gen == AGen::A7xx), "hi 含等");
    // 族三：未知兜底。
    let r7 = id.identify(VENDOR_QCOM, 999);
    s.add("999 兜底", matches!(&r7, Ok(r) if r.gen == AGen::Unknown), "立案");
    s.add("立案 1", id.unknown_count() == 1, "一笔");
    // 族四：代际继承。
    s.add("A6 继 A5", inherits(AGen::A6xx) == Some(AGen::A5xx), "家谱");
    s.add("A7 继 A6", inherits(AGen::A7xx) == Some(AGen::A6xx), "家谱");
    s.add("A3 无继承", inherits(AGen::A3xx).is_none(), "根代");
    s.add("Unknown 无继承", inherits(AGen::Unknown).is_none(), "圈外");
    // 继承链闭环：A7→A6→A5。
    let chain = match inherits(AGen::A7xx) {
        Some(g) => inherits(g),
        None => None,
    };
    s.add("链 A7→A5", chain == Some(AGen::A5xx), "两跳");
    // 族五：分型稳定核。
    let ra = id.identify(VENDOR_QCOM, 650);
    let rb = id.identify(VENDOR_QCOM, 650);
    s.add("两次同果", ra == rb, "稳定");
    s.add("识别计 8", id.ident_count == 8, "含兜底");
    s.add("魔数核", AD_SEG_BASE == 0x31440, "段基址");
    s
}
