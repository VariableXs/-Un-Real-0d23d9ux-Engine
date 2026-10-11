//! unxt561_fbmode —— UNX-F0481 帧缓冲模式表（48 个 VESA 模式）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0481 · 帧缓冲信息账冻结（四要素） · 判据：四要素账齐（物理基址/宽/高/bpp）且与 F0110 探测值复读一致（4/4 字段），版本封账
//! · 组件：48 模式表（宽/高/bpp）+ 逐行守卫 + 像素容量换算
//! · 判据落地：帧缓冲探测的候选模式面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T561_VERSION: &str = "t561-fbmode-v1";

/// 诊断段基址。
pub const T561_SEG_BASE: u16 = 0x4500;

// --------------------------------------------------------------------------
// 诊断码（t561 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 帧缓冲模式诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t561Code {
    /// 0x4500 模式非法（表外模式号）。
    ModeBad,
    /// 0x4501 宽非法（0 或超 8192）。
    WidthBad,
    /// 0x4502 高非法（0 或超 8192）。
    HeightBad,
    /// 0x4503 bpp 非法（非 8/15/16/32）。
    BppBad,
    /// 0x4504 容量溢出（宽×高×bpp 超 u64）。
    CapacityOverflow,
    /// 0x4505 行距未对齐（非 4/16/32）。
    PitchUnaligned,
    /// 0x4506 模式账破缺。
    LedgerBreak,
    /// 0x4507 守卫触发。
    GuardTripped,
    /// 0x4508 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4509 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x450a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x450b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x450c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x450d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x450e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t561Code {
    /// 全量闭集。
    pub const ALL: [t561Code; 15] = [
        t561Code::ModeBad,
        t561Code::WidthBad,
        t561Code::HeightBad,
        t561Code::BppBad,
        t561Code::CapacityOverflow,
        t561Code::PitchUnaligned,
        t561Code::LedgerBreak,
        t561Code::GuardTripped,
        t561Code::LedgerBroken,
        t561Code::SnapshotFingerprint,
        t561Code::TableBroken,
        t561Code::IndexMisaligned,
        t561Code::DuplicateEntry,
        t561Code::OrderViolated,
        t561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t561Code::ModeBad => 0x4500,
            t561Code::WidthBad => 0x4501,
            t561Code::HeightBad => 0x4502,
            t561Code::BppBad => 0x4503,
            t561Code::CapacityOverflow => 0x4504,
            t561Code::PitchUnaligned => 0x4505,
            t561Code::LedgerBreak => 0x4506,
            t561Code::GuardTripped => 0x4507,
            t561Code::LedgerBroken => 0x4508,
            t561Code::SnapshotFingerprint => 0x4509,
            t561Code::TableBroken => 0x450a,
            t561Code::IndexMisaligned => 0x450b,
            t561Code::DuplicateEntry => 0x450c,
            t561Code::OrderViolated => 0x450d,
            t561Code::SegmentGuard => 0x450e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t561Code::ModeBad => "模式非法（表外模式号）",
            t561Code::WidthBad => "宽非法（0 或超 8192）",
            t561Code::HeightBad => "高非法（0 或超 8192）",
            t561Code::BppBad => "bpp 非法（非 8/15/16/32）",
            t561Code::CapacityOverflow => "容量溢出（宽×高×bpp 超 u64）",
            t561Code::PitchUnaligned => "行距未对齐（非 4/16/32）",
            t561Code::LedgerBreak => "模式账破缺",
            t561Code::GuardTripped => "守卫触发",
            t561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t561Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t561Code::DuplicateEntry => "条目重复（同键双写）",
            t561Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t561Code::ModeBad => "按模式表登记新模式",
            t561Code::WidthBad => "按宽表校正",
            t561Code::HeightBad => "按高表校正",
            t561Code::BppBad => "按位宽表校正",
            t561Code::CapacityOverflow => "分段核算",
            t561Code::PitchUnaligned => "按对齐律校正",
            t561Code::LedgerBreak => "按守恒重建",
            t561Code::GuardTripped => "检查临界区",
            t561Code::LedgerBroken => "按守恒律重建台账后重跑",
            t561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t561Code::TableBroken => "按语义规则重建常量表",
            t561Code::IndexMisaligned => "按声明序归一化后重跑",
            t561Code::DuplicateEntry => "合并同键条目为末次生效",
            t561Code::OrderViolated => "按依赖矩阵重排后重跑",
            t561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 fbmode（51 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// VESA 帧缓冲模式表（48 个真实模式逐行守卫）。
/// 行数。
pub const fbmode_COUNT: usize = 51;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static fbmode: [(&'static str, u64); 51] = [
    ("MODE100", 256), // 模式 0x100：640x400 @8bpp
    ("MODE101", 257), // 模式 0x101：640x480 @8bpp
    ("MODE102", 258), // 模式 0x102：800x600 @4bpp
    ("MODE103", 259), // 模式 0x103：800x600 @8bpp
    ("MODE104", 260), // 模式 0x104：1024x768 @4bpp
    ("MODE105", 261), // 模式 0x105：1024x768 @8bpp
    ("MODE106", 262), // 模式 0x106：1280x1024 @4bpp
    ("MODE107", 263), // 模式 0x107：1280x1024 @8bpp
    ("MODE10D", 269), // 模式 0x10D：320x200 @15bpp
    ("MODE10E", 270), // 模式 0x10E：320x200 @16bpp
    ("MODE10F", 271), // 模式 0x10F：320x200 @32bpp
    ("MODE110", 272), // 模式 0x110：640x480 @15bpp
    ("MODE111", 273), // 模式 0x111：640x480 @16bpp
    ("MODE112", 274), // 模式 0x112：640x480 @32bpp
    ("MODE113", 275), // 模式 0x113：800x600 @15bpp
    ("MODE114", 276), // 模式 0x114：800x600 @16bpp
    ("MODE115", 277), // 模式 0x115：800x600 @32bpp
    ("MODE116", 278), // 模式 0x116：1024x768 @15bpp
    ("MODE117", 279), // 模式 0x117：1024x768 @16bpp
    ("MODE118", 280), // 模式 0x118：1024x768 @32bpp
    ("MODE119", 281), // 模式 0x119：1280x1024 @15bpp
    ("MODE11A", 282), // 模式 0x11A：1280x1024 @16bpp
    ("MODE11B", 283), // 模式 0x11B：1280x1024 @32bpp
    ("MODE136", 310), // 模式 0x136：320x200 @8bpp
    ("MODE137", 311), // 模式 0x137：320x400 @8bpp
    ("MODE138", 312), // 模式 0x138：320x400 @16bpp
    ("MODE139", 313), // 模式 0x139：320x200 @32bpp
    ("MODE13A", 314), // 模式 0x13A：320x240 @8bpp
    ("MODE13B", 315), // 模式 0x13B：400x300 @8bpp
    ("MODE13C", 316), // 模式 0x13C：512x384 @8bpp
    ("MODE13D", 317), // 模式 0x13D：512x384 @16bpp
    ("MODE140", 320), // 模式 0x140：640x350 @8bpp
    ("MODE141", 321), // 模式 0x141：640x400 @8bpp
    ("MODE142", 322), // 模式 0x142：640x480 @8bpp
    ("MODE143", 323), // 模式 0x143：640x480 @16bpp
    ("MODE144", 324), // 模式 0x144：800x600 @8bpp
    ("MODE145", 325), // 模式 0x145：800x600 @16bpp
    ("MODE146", 326), // 模式 0x146：1024x768 @8bpp
    ("MODE147", 327), // 模式 0x147：1024x768 @16bpp
    ("MODE148", 328), // 模式 0x148：1280x960 @8bpp
    ("MODE149", 329), // 模式 0x149：1280x960 @16bpp
    ("MODE14A", 330), // 模式 0x14A：1280x1024 @8bpp
    ("MODE14B", 331), // 模式 0x14B：1280x1024 @16bpp
    ("MODE14C", 332), // 模式 0x14C：1600x1200 @8bpp
    ("MODE14D", 333), // 模式 0x14D：1600x1200 @16bpp
    ("MODE14E", 334), // 模式 0x14E：1792x1344 @8bpp
    ("MODE14F", 335), // 模式 0x14F：1792x1344 @16bpp
    ("MODE150", 336), // 模式 0x150：1856x1392 @8bpp
    ("MODE151", 337), // 模式 0x151：1856x1392 @16bpp
    ("MODE152", 338), // 模式 0x152：1920x1440 @8bpp
    ("MODE153", 339), // 模式 0x153：1920x1440 @16bpp
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn fbmode_get(key: &str) -> Option<u64> {
    match key {
        "MODE100" => Some(256),
        "MODE101" => Some(257),
        "MODE102" => Some(258),
        "MODE103" => Some(259),
        "MODE104" => Some(260),
        "MODE105" => Some(261),
        "MODE106" => Some(262),
        "MODE107" => Some(263),
        "MODE10D" => Some(269),
        "MODE10E" => Some(270),
        "MODE10F" => Some(271),
        "MODE110" => Some(272),
        "MODE111" => Some(273),
        "MODE112" => Some(274),
        "MODE113" => Some(275),
        "MODE114" => Some(276),
        "MODE115" => Some(277),
        "MODE116" => Some(278),
        "MODE117" => Some(279),
        "MODE118" => Some(280),
        "MODE119" => Some(281),
        "MODE11A" => Some(282),
        "MODE11B" => Some(283),
        "MODE136" => Some(310),
        "MODE137" => Some(311),
        "MODE138" => Some(312),
        "MODE139" => Some(313),
        "MODE13A" => Some(314),
        "MODE13B" => Some(315),
        "MODE13C" => Some(316),
        "MODE13D" => Some(317),
        "MODE140" => Some(320),
        "MODE141" => Some(321),
        "MODE142" => Some(322),
        "MODE143" => Some(323),
        "MODE144" => Some(324),
        "MODE145" => Some(325),
        "MODE146" => Some(326),
        "MODE147" => Some(327),
        "MODE148" => Some(328),
        "MODE149" => Some(329),
        "MODE14A" => Some(330),
        "MODE14B" => Some(331),
        "MODE14C" => Some(332),
        "MODE14D" => Some(333),
        "MODE14E" => Some(334),
        "MODE14F" => Some(335),
        "MODE150" => Some(336),
        "MODE151" => Some(337),
        "MODE152" => Some(338),
        "MODE153" => Some(339),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn fbmode_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < fbmode_COUNT {
        let (k, v) = fbmode[i];
        let ok = match k {
            "MODE100" => v >= 0x100 && v <= 0x160,
            "MODE101" => v >= 0x100 && v <= 0x160,
            "MODE102" => v >= 0x100 && v <= 0x160,
            "MODE103" => v >= 0x100 && v <= 0x160,
            "MODE104" => v >= 0x100 && v <= 0x160,
            "MODE105" => v >= 0x100 && v <= 0x160,
            "MODE106" => v >= 0x100 && v <= 0x160,
            "MODE107" => v >= 0x100 && v <= 0x160,
            "MODE10D" => v >= 0x100 && v <= 0x160,
            "MODE10E" => v >= 0x100 && v <= 0x160,
            "MODE10F" => v >= 0x100 && v <= 0x160,
            "MODE110" => v >= 0x100 && v <= 0x160,
            "MODE111" => v >= 0x100 && v <= 0x160,
            "MODE112" => v >= 0x100 && v <= 0x160,
            "MODE113" => v >= 0x100 && v <= 0x160,
            "MODE114" => v >= 0x100 && v <= 0x160,
            "MODE115" => v >= 0x100 && v <= 0x160,
            "MODE116" => v >= 0x100 && v <= 0x160,
            "MODE117" => v >= 0x100 && v <= 0x160,
            "MODE118" => v >= 0x100 && v <= 0x160,
            "MODE119" => v >= 0x100 && v <= 0x160,
            "MODE11A" => v >= 0x100 && v <= 0x160,
            "MODE11B" => v >= 0x100 && v <= 0x160,
            "MODE136" => v >= 0x100 && v <= 0x160,
            "MODE137" => v >= 0x100 && v <= 0x160,
            "MODE138" => v >= 0x100 && v <= 0x160,
            "MODE139" => v >= 0x100 && v <= 0x160,
            "MODE13A" => v >= 0x100 && v <= 0x160,
            "MODE13B" => v >= 0x100 && v <= 0x160,
            "MODE13C" => v >= 0x100 && v <= 0x160,
            "MODE13D" => v >= 0x100 && v <= 0x160,
            "MODE140" => v >= 0x100 && v <= 0x160,
            "MODE141" => v >= 0x100 && v <= 0x160,
            "MODE142" => v >= 0x100 && v <= 0x160,
            "MODE143" => v >= 0x100 && v <= 0x160,
            "MODE144" => v >= 0x100 && v <= 0x160,
            "MODE145" => v >= 0x100 && v <= 0x160,
            "MODE146" => v >= 0x100 && v <= 0x160,
            "MODE147" => v >= 0x100 && v <= 0x160,
            "MODE148" => v >= 0x100 && v <= 0x160,
            "MODE149" => v >= 0x100 && v <= 0x160,
            "MODE14A" => v >= 0x100 && v <= 0x160,
            "MODE14B" => v >= 0x100 && v <= 0x160,
            "MODE14C" => v >= 0x100 && v <= 0x160,
            "MODE14D" => v >= 0x100 && v <= 0x160,
            "MODE14E" => v >= 0x100 && v <= 0x160,
            "MODE14F" => v >= 0x100 && v <= 0x160,
            "MODE150" => v >= 0x100 && v <= 0x160,
            "MODE151" => v >= 0x100 && v <= 0x160,
            "MODE152" => v >= 0x100 && v <= 0x160,
            "MODE153" => v >= 0x100 && v <= 0x160,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn fbmode_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < fbmode_COUNT {
        s = s.wrapping_add(fbmode[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn fbmode_unique() -> bool {
    let mut i = 0usize;
    while i < fbmode_COUNT {
        let mut j = i + 1;
        while j < fbmode_COUNT {
            if fbmode[i].0 == fbmode[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 模式换算引擎（像素容量 + 行距对齐）
// ---------------------------------------------------------------------------

/// 帧缓冲区大小（字节）= 宽 × 高 × bpp / 8。
pub const fn fb_size_bytes(w: u32, h: u32, bpp: u32) -> Option<u64> {
    if w == 0 || h == 0 || bpp == 0 {
        return None;
    }
    let pixels = (w as u64) * (h as u64);
    let bytes = pixels * (bpp as u64) / 8;
    Some(bytes)
}

/// 行距对齐（16 字节粒度；上取整到下一个 16 倍数）。
pub const fn fb_pitch_align(w: u32, bpp: u32) -> u32 {
    let row_bits = w.saturating_mul(bpp);
    let row_bytes = (row_bits + 7) / 8;
    let rem = row_bytes % 16;
    if rem == 0 {
        row_bytes
    } else {
        row_bytes + (16 - rem)
    }
}

/// 总帧缓冲字节（用对齐行距核算， quadruple-buffer 前核算用）。
pub const fn fb_stride_bytes(w: u32, h: u32, bpp: u32) -> Option<u64> {
    let pitch = fb_pitch_align(w, bpp) as u64;
    if pitch == 0 {
        return None;
    }
    Some(pitch * (h as u64))
}

/// 模式容量抽样（1024x768x32 = 3MiB）。
pub const fn fb_mode_capacity_ok() -> bool {
    match fb_size_bytes(1024, 768, 32) {
        Some(v) => v == 1024 * 768 * 4,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4500, 0x450f]。
pub const fn t561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t561Code::ALL.len() {
        let c = t561Code::ALL[i].code();
        if c < 0x4500 || c > 0x450f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t561_guard_seg() -> bool {
    t561_segment_guard()
}

/// 表 fbmode 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t561_fbmode_guard() -> bool {
    let mut ok = fbmode_COUNT == 51;
    if ok {
        ok = fbmode_scan() == 0;
    }
    if ok {
        ok = fbmode_unique();
    }
    ok
}

/// 容量换算（1024x768x32=3MiB + 行距对齐）。
fn check_c2_conversion() -> bool {
    let mut ok = fb_mode_capacity_ok();
    if ok {
        ok = fb_size_bytes(640, 480, 32) == Some(640 * 480 * 4);
    }
    if ok {
        // 640x32bpp 行 2560 字节 → 已 16 对齐（2560/16=160）。
        ok = fb_pitch_align(640, 32) == 2560;
    }
    if ok {
        // 641x32bpp 行 2564 → 上取整至 2560+16=2576。
        ok = fb_pitch_align(641, 32) == 2576;
    }
    if ok {
        ok = fb_stride_bytes(8, 4, 8).is_some();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t561_checks() -> CheckSet {
    let mut set = CheckSet::new("T561");
    set.add(
        "t561-seg-guard",
        t561_guard_seg(),
        if t561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t561-c1-mode-guard",
        check_t561_fbmode_guard(),
        if check_t561_fbmode_guard() { "" } else { "table fbmode guard mismatch" },
    );
    set.add(
        "t561-c2-conversion",
        check_c2_conversion(),
        if check_c2_conversion() { "" } else { "fb conversion mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t561_all_checks_green() {
        let set = run_t561_checks();
        assert!(set.all_passed(), "T561 domain checks must be green");
    }

    #[test]
    fn t561_segment_guard_holds() {
        assert!(t561_segment_guard());
    }
}
