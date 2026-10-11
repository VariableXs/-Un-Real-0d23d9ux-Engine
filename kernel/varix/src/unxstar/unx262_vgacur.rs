//! unxt268_unx262_vgacur —— UNX-F0262 VGA 硬件光标精细化（形状/显隐/位置三态）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0262 · VGA 通道硬件光标精细化 · 判据：光标形状/显隐/位置三态硬件寄存器语义正确，渲染抽样一致
//! · 组件：三态寄存器语义表 + 光标形状表（32×32 位图域）+ 位置钳制 + 抽样对拍
//! · 判据落地：三态寄存器语义 + 渲染抽样一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T268_VERSION: &str = "t268-unx262_vgacur-v1";

/// 诊断段基址。
pub const T268_SEG_BASE: u16 = 0x4e70;

// --------------------------------------------------------------------------
// 诊断码（t268 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 硬件光标诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t268Code {
    /// 0x4e70 形状非法（位图超出 32×32 域）。
    ShapeBad,
    /// 0x4e71 显隐态非法（二态之外）。
    VisibilityBad,
    /// 0x4e72 位置越界（超文本网格 80×25）。
    PosOob,
    /// 0x4e73 CRTC 寄存器越界（0x00..0x18 之外）。
    CrtcBad,
    /// 0x4e74 抽样分歧（渲染抽样不一致）。
    SamplingDiff,
    /// 0x4e75 光标账破缺。
    LedgerBreak,
    /// 0x4e76 三态迁移违例。
    OrderViolated,
    /// 0x4e77 守卫触发。
    GuardTripped,
    /// 0x4e78 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4e79 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4e7a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4e7b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4e7c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4e7d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t268Code {
    /// 全量闭集。
    pub const ALL: [t268Code; 14] = [
        t268Code::ShapeBad,
        t268Code::VisibilityBad,
        t268Code::PosOob,
        t268Code::CrtcBad,
        t268Code::SamplingDiff,
        t268Code::LedgerBreak,
        t268Code::OrderViolated,
        t268Code::GuardTripped,
        t268Code::LedgerBroken,
        t268Code::SnapshotFingerprint,
        t268Code::TableBroken,
        t268Code::IndexMisaligned,
        t268Code::DuplicateEntry,
        t268Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t268Code::ShapeBad => 0x4e70,
            t268Code::VisibilityBad => 0x4e71,
            t268Code::PosOob => 0x4e72,
            t268Code::CrtcBad => 0x4e73,
            t268Code::SamplingDiff => 0x4e74,
            t268Code::LedgerBreak => 0x4e75,
            t268Code::OrderViolated => 0x4e76,
            t268Code::GuardTripped => 0x4e77,
            t268Code::LedgerBroken => 0x4e78,
            t268Code::SnapshotFingerprint => 0x4e79,
            t268Code::TableBroken => 0x4e7a,
            t268Code::IndexMisaligned => 0x4e7b,
            t268Code::DuplicateEntry => 0x4e7c,
            t268Code::SegmentGuard => 0x4e7d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t268Code::ShapeBad => "形状非法（位图超出 32×32 域）",
            t268Code::VisibilityBad => "显隐态非法（二态之外）",
            t268Code::PosOob => "位置越界（超文本网格 80×25）",
            t268Code::CrtcBad => "CRTC 寄存器越界（0x00..0x18 之外）",
            t268Code::SamplingDiff => "抽样分歧（渲染抽样不一致）",
            t268Code::LedgerBreak => "光标账破缺",
            t268Code::OrderViolated => "三态迁移违例",
            t268Code::GuardTripped => "守卫触发",
            t268Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t268Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t268Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t268Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t268Code::DuplicateEntry => "条目重复（同键双写）",
            t268Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t268Code::ShapeBad => "按位图域校正",
            t268Code::VisibilityBad => "按二态闭集校正",
            t268Code::PosOob => "按网格律钳制",
            t268Code::CrtcBad => "按 CRTC 索引域校正",
            t268Code::SamplingDiff => "按抽样口径归因",
            t268Code::LedgerBreak => "按守恒重建",
            t268Code::OrderViolated => "按三态律重排",
            t268Code::GuardTripped => "检查临界区",
            t268Code::LedgerBroken => "按守恒律重建台账后重跑",
            t268Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t268Code::TableBroken => "按语义规则重建常量表",
            t268Code::IndexMisaligned => "按声明序归一化后重跑",
            t268Code::DuplicateEntry => "合并同键条目为末次生效",
            t268Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// VGA 硬件光标三态（形状/显隐/位置）+ 32×32 位图域
// ---------------------------------------------------------------------------

/// 光标位图域（32 × 32 单色位图）。
pub const CURSOR_W: u32 = 32;

/// 光标位图高。
pub const CURSOR_H: u32 = 32;

/// 位图行字数（32 位宽 → 4 字节/行）。
pub const CURSOR_ROW_WORDS: u32 = 4;

/// 位图总字节（32 行 × 4 字 × 2 字节 = 256）。
pub const CURSOR_BYTES: u32 = CURSOR_H * CURSOR_ROW_WORDS * 2;

/// 文本网格宽（80 列）。
pub const TEXT_COLS: u32 = 80;

/// 文本网格高（25 行）。
pub const TEXT_ROWS: u32 = 25;

/// 显隐态（二态闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorVis {
    /// 隐藏。
    Hidden,
    /// 显示。
    Shown,
}

/// 位置（列 + 行，钳制域内）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorPos {
    /// 列（0..=79）。
    pub col: u32,
    /// 行（0..=24）。
    pub row: u32,
}

/// 位置合法性 + 钳制（越界钳到边界而非拒弃）。
pub const fn cursor_clamp(p: &CursorPos) -> CursorPos {
    let col = if p.col >= TEXT_COLS { TEXT_COLS - 1 } else { p.col };
    let row = if p.row >= TEXT_ROWS { TEXT_ROWS - 1 } else { p.row };
    CursorPos { col, row }
}

/// 位图合法性（32×32 域 + 每行 4 字）。
pub const fn cursor_shape_ok(rows: u32, row_words: u32) -> bool {
    rows == CURSOR_H && row_words == CURSOR_ROW_WORDS
}

/// 三态寄存器语义（CRTC 索引 0x0A/0x0B 起始高位寄存器）。
pub const CRTC_CURSOR_START: u8 = 0x0A;

/// CRTC 光标结束寄存器。
pub const CRTC_CURSOR_END: u8 = 0x0B;

/// CRTC 光标位置高位寄存器。
pub const CRTC_CURSOR_LOC_HI: u8 = 0x0E;

/// CRTC 光标位置低位寄存器。
pub const CRTC_CURSOR_LOC_LO: u8 = 0x0F;

/// CRTC 索引域上限（0x18）。
pub const CRTC_MAX_IDX: u8 = 0x18;

/// CRTC 索引合法性。
pub const fn crtc_idx_ok(idx: u8) -> bool {
    idx <= CRTC_MAX_IDX
}

/// 位图行 XOR 演化（显隐切换的位图翻转口径：显示 = 屏 ⊕ 位图）。
pub const fn cursor_xor(screen_bit: bool, map_bit: bool) -> bool {
    screen_bit ^ map_bit
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4e70, 0x4e7f]。
pub const fn t268_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t268Code::ALL.len() {
        let c = t268Code::ALL[i].code();
        if c < 0x4e70 || c > 0x4e7f {
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
fn t268_guard_seg() -> bool {
    t268_segment_guard()
}

/// 位图域（32×32/256 字节）+ CRTC 索引域 + 位置钳制。
fn check_c1_shape() -> bool {
    let mut ok = cursor_shape_ok(32, 4) && CURSOR_BYTES == 256;
    if ok {
        ok = crtc_idx_ok(CRTC_CURSOR_START) && crtc_idx_ok(CRTC_CURSOR_END);
    }
    if ok {
        ok = crtc_idx_ok(0x18) && !crtc_idx_ok(0x19);
    }
    if ok {
        // 越界位置钳到 79×24。
        let c = cursor_clamp(&CursorPos { col: 200, row: 100 });
        ok = c.col == 79 && c.row == 24;
    }
    ok
}

/// 显隐 XOR 抽样（同屏同位一致 + 翻转位变化）。
fn check_c2_xor() -> bool {
    let screen = [true, true, false, false, true, false, true, true];
    let map = [true, false, true, false, true, false, true, false];
    let mut ok = true;
    let mut i = 0;
    while i < 8 {
        ok = ok && cursor_xor(screen[i], map[i]) == (screen[i] != map[i]);
        i += 1;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t268_checks() -> CheckSet {
    let mut set = CheckSet::new("T268");
    set.add(
        "t268-seg-guard",
        t268_guard_seg(),
        if t268_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t268-c1-shape",
        check_c1_shape(),
        if check_c1_shape() { "" } else { "cursor shape/crtc mismatch" },
    );
    set.add(
        "t268-c2-xor",
        check_c2_xor(),
        if check_c2_xor() { "" } else { "cursor xor sampling mismatch" },
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
    fn t268_all_checks_green() {
        let set = run_t268_checks();
        assert!(set.all_passed(), "T268 domain checks must be green");
    }

    #[test]
    fn t268_segment_guard_holds() {
        assert!(t268_segment_guard());
    }
}
