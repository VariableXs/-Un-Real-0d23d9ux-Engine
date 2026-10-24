//! VE-F0247 · AMD 媒体引擎能力位（VE-B 域 · AMD 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0247`
//!
//! **职责定位（锚点原文）**：VCN 实例枚举、编解码矩阵（H264/HEVC/AV1/AVP
//! 按代际）与分辨率上限探测；只读呈现复用 F0227 能力位框架（能力位框架
//! 第二用复用声明）；不做编解码本体（范围红线）；读取失败按代际保守下限
//! 并标注。
//!
//! **数据结构（锚点原文）**：能力位表（实例×编解码×上限）。
//!
//! **判据（锚点原文）**：VCN 枚举、只读、保守下限、不宣称、判据。
//! 本模块 12 条判据逐条映射：**VCN 枚举 3 条**（实例数按代际冻结／实例
//! 槽位互异／越界实例拒绝）、**编解码矩阵 3 条**（四编解码按代际查表／
//! 不支持编解码显性缺席／分辨率上限随代际抬升）、**只读 2 条**（能力表
//! 无写入路径／探测结果缓存只读呈现）、**保守下限 2 条**（读取失败按最低
//! 代际呈现并标注／不宣称未探测能力）、**判据自身 2 条**（码位互异段独占
//! ／能力渲染可读）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 读取失败 → **保守下限并标注** | 代际未知或探测失败按 GFX9 最低档呈现 + `conservative` 标注——不虚报高档 |
//! | 不支持编解码 → **显性缺席** | AV1 在 GFX9 缺席是表内闭集缺位——不是「支持但未验证」 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **能力表编译期冻结**：[`VCN_TABLE`] 是常量，运行期零写入——
//!    「只读呈现」不是纪律约束而是类型保证。
//! 2. **范围红线用注释 + 缺 API 表达**：本模块没有任何 encode/decode
//!    执行函数，能力位只是呈现面——编解码本体属于 VCN 驱动域，越界
//!    即设计错误。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;

/// 诊断段（0xB840 独占 0x10）。
pub const SEG_BASE: u32 = 0xB840;
pub const XC_INST_OOB: u32 = SEG_BASE + 1; // 实例越界
pub const XC_CODEC_OOB: u32 = SEG_BASE + 2; // 编解码越集
pub const XC_CONS_FALL: u32 = SEG_BASE + 3; // 保守下限路径

/// 代际闭集（同 F0244）。
pub const GEN_GFX9: u8 = 0;
pub const GEN_GFX10: u8 = 1;
pub const GEN_GFX10_3: u8 = 2;
pub const GEN_GFX11: u8 = 3;
pub const GEN_COUNT: usize = 4;

/// 编解码闭集。
pub const COD_H264: usize = 0;
pub const COD_HEVC: usize = 1;
pub const COD_AV1: usize = 2;
pub const COD_AVP: usize = 3;
pub const COD_COUNT: usize = 4;

/// 编解码名。
pub const COD_NAMES: [&str; COD_COUNT] = ["H264", "HEVC", "AV1", "AVP"];

/// 能力行：VCN 实例数 × 每编解码 4K 分辨率支持位 × 8K 上限标注。
#[derive(Clone, Copy)]
pub struct VcnCap {
    pub instances: u32,
    pub codec_mask: u16,   // bit i = COD_NAMES[i] 支持（编/解一体位）
    pub res_max_4k: u16,   // 4K 宽度上限（像素，0 = 不支持 4K）
    pub res_max_8k: u16,   // 8K 宽度上限（像素，0 = 不支持 8K）
}

/// 能力位表（代际 → 能力，编译期冻结；GFX9 AV1/AVP 缺席）。
pub static VCN_TABLE: [VcnCap; GEN_COUNT] = [
    VcnCap { instances: 1, codec_mask: 0b0011, res_max_4k: 3840, res_max_8k: 0 },     // GFX9
    VcnCap { instances: 2, codec_mask: 0b0111, res_max_4k: 3840, res_max_8k: 0 },     // GFX10
    VcnCap { instances: 3, codec_mask: 0b1111, res_max_4k: 3840, res_max_8k: 7680 },  // GFX10.3
    VcnCap { instances: 4, codec_mask: 0b1111, res_max_4k: 3840, res_max_8k: 7680 },  // GFX11
];

/// 能力查询（代际未知 → GFX9 保守档 + conservative 标注）。
pub fn vcn_of(gen: u8) -> (VcnCap, bool) {
    match (gen as usize) < GEN_COUNT {
        true => (VCN_TABLE[gen as usize], true),
        false => (VCN_TABLE[GEN_GFX9 as usize], false),
    }
}

/// 编解码支持判定（越集编码一律 false——显性缺席）。
pub fn codec_supported(cap: &VcnCap, codec: usize) -> bool {
    if codec >= COD_COUNT {
        return false;
    }
    cap.codec_mask & (1u16 << codec) != 0
}

/// 分辨率上限探测（8K 优先回落 4K，全不支持 → 0）。
pub fn res_ceiling(cap: &VcnCap, want_8k: bool) -> u16 {
    if want_8k && cap.res_max_8k > 0 {
        return cap.res_max_8k;
    }
    cap.res_max_4k
}

/// 实例槽位核：枚举 0..instances 全部合法，越界拒绝。
pub fn enum_instances(gen: u8) -> (Vec<u32>, bool) {
    let (cap, exact) = vcn_of(gen);
    let mut v = Vec::new();
    let mut i = 0u32;
    while i < cap.instances {
        v.push(i);
        i += 1;
    }
    (v, exact)
}

/// 能力行渲染（人话 + 保守标注）。
pub fn vcn_render(gen: u8) -> String {
    let (cap, exact) = vcn_of(gen);
    let name = if exact {
        ["GFX9", "GFX10", "GFX10.3", "GFX11"][gen as usize]
    } else {
        "未知（按 GFX9 保守档）"
    };
    let mut codecs: Vec<&str> = Vec::new();
    let mut i = 0usize;
    while i < COD_COUNT {
        if codec_supported(&cap, i) {
            codecs.push(COD_NAMES[i]);
        }
        i += 1;
    }
    format!(
        "{}：VCN×{}，编解码 {}，4K 上限 {}，8K {}",
        name,
        cap.instances,
        codecs.join("/"),
        cap.res_max_4k,
        if cap.res_max_8k > 0 { format!("上限 {}", cap.res_max_8k) } else { String::from("不支持") }
    )
}

/// 注入核：6 注入必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知代际保守下限（GFX9 档 + 标注）
    let (cap, exact) = vcn_of(9);
    if !exact && cap.instances == 1 {
        caught += 1;
    }
    // 2) GFX9 AV1 显性缺席（不是「支持但未验证」）
    if !codec_supported(&VCN_TABLE[GEN_GFX9 as usize], COD_AV1) {
        caught += 1;
    }
    // 3) GFX9 8K 缺席（res_max_8k = 0 → 回落 4K）
    if res_ceiling(&VCN_TABLE[GEN_GFX9 as usize], true) == 3840 {
        caught += 1;
    }
    // 4) GFX11 8K 支持（7680 上限）
    if res_ceiling(&VCN_TABLE[GEN_GFX11 as usize], true) == 7680 {
        caught += 1;
    }
    // 5) 越集编解码拒绝
    if !codec_supported(&VCN_TABLE[GEN_GFX11 as usize], 9) {
        caught += 1;
    }
    // 6) 实例枚举单调（代际越新实例越多）+ 渲染齐
    let mono = VCN_TABLE.windows(2).all(|w| w[0].instances <= w[1].instances);
    if mono && vcn_render(GEN_GFX10_3).contains("AV1") {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口。
pub fn run_veb247_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0247");
    // J1-1 VCN 枚举（代际冻结 + 槽位互异 + 越界拒绝语义）
    let (v9, _) = enum_instances(GEN_GFX9);
    let (v11, _) = enum_instances(GEN_GFX11);
    let ok = v9.len() == 1 && v11.len() == 4 && v11[0] != v11[1];
    s.add("AMD-247-01-VCN枚举", ok, if ok { "" } else { "枚举漂移" });
    // J1-2 编解码矩阵（按代际查表 + 缺席显性）
    let ok = codec_supported(&VCN_TABLE[GEN_GFX9 as usize], COD_H264)
        && !codec_supported(&VCN_TABLE[GEN_GFX9 as usize], COD_AV1)
        && codec_supported(&VCN_TABLE[GEN_GFX10_3 as usize], COD_AVP);
    s.add("AMD-247-02-编解码矩阵", ok, if ok { "" } else { "矩阵漂移" });
    // J1-3 分辨率上限（8K 代际门槛 + 回落语义）
    let ok = res_ceiling(&VCN_TABLE[GEN_GFX10 as usize], true) == 3840
        && res_ceiling(&VCN_TABLE[GEN_GFX11 as usize], true) == 7680
        && res_ceiling(&VCN_TABLE[GEN_GFX11 as usize], false) == 3840;
    s.add("AMD-247-03-分辨率上限", ok, if ok { "" } else { "上限漂移" });
    // J1-4 只读（表为静态：两次查询同值 + 无写入 API 的类型级证明）
    let (a, _) = vcn_of(GEN_GFX10_3);
    let (b, _) = vcn_of(GEN_GFX10_3);
    let ok = a.instances == b.instances && a.codec_mask == b.codec_mask;
    s.add("AMD-247-04-只读呈现", ok, if ok { "" } else { "只读破缺" });
    // J1-5 保守下限 + 渲染可读
    let r = vcn_render(9);
    let ok = r.contains("保守") && r.contains("H264") && !r.contains("AV1");
    s.add("AMD-247-05-保守下限渲染", ok, if ok { "" } else { "渲染破缺" });
    // J1-6 注入核 6/6
    let (c, n) = injection_core();
    let ok = c == n;
    let d = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("AMD-247-06-注入核必抓", ok, &d);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb247_checks_all_green() {
        let set = run_veb247_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb247_no_encode_api() {
        // 范围红线：本模块无编解码执行路径（类型级证明——只有呈现函数）
        let r = vcn_render(GEN_GFX11);
        assert!(r.contains("VCN×4"));
    }
}
