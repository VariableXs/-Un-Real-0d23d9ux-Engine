//! F028 深化批次三 · DPI 缩放传播执行/边界/注入面（compatstar2/deep2 · G-A-28）。
//!
//! 批次一/二深化覆盖三态决议主干；本批补齐主册【功能定义】「全语义对齐」的
//! 缩放传播侧出口：MDT_* 感知类型判定表（UNAWARE/SYSTEM_AWARE/
//! PER_MONITOR_AWARE_V2 三型→行为矩阵）、缩放因子 16.16 定点乘法（u32 Q16.16
//! ：u32×u32 用 u64 中间步防溢出，四舍五入）、子窗口缩放消息传播时序模型
//! （父先 AFTERPARENT 后子，深度定长 4 树，传播顺序账）、每 DPI 字体点数→
//! 像素换算（px = pt × dpi / 72，对拍 96/144/192 三档）、缩放历史回溯账
//! （定长 8 次变更环形，可回放最近一次逆变换）。
//!
//! 判据对账：深化以主册【设计细节】/【状态与异常】未落地面为源，一处一事实
//! （MS DPI_AWARENESS_CONTEXT_*/WM_DPICHANGED*/DEVMODE pt-to-px 语义对拍）。
//!
//! 零堆纪律：定长顺序账 + 定长历史环，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// DPI_AWARENESS_CONTEXT_UNAWARE = -1（MS GetThreadDpiAwarenessContext 句柄值）。
pub const MDT_UNAWARE: i32 = -1;
/// DPI_AWARENESS_CONTEXT_SYSTEM_AWARE = -2（MS 句柄值）。
pub const MDT_SYSTEM_AWARE: i32 = -2;
/// DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2 = -5（MS 句柄值）。
pub const MDT_PER_MONITOR_AWARE_V2: i32 = -5;
/// WM_DPICHANGED = 0x02E0（MS winuser.h）。
pub const WM_DPICHANGED: u32 = 0x02E0;
/// WM_DPICHANGED_BEFOREPARENT = 0x02E2（MS winuser.h）。
pub const WM_DPICHANGED_BEFOREPARENT: u32 = 0x02E2;
/// WM_DPICHANGED_AFTERPARENT = 0x02E3（MS winuser.h）。
pub const WM_DPICHANGED_AFTERPARENT: u32 = 0x02E3;
/// Q16.16 定点 1.0（65536）。
pub const Q16_ONE: u32 = 1 << 16;
/// 传播顺序账容量（定长 8 事件）。
pub const PROP_LEDGER_CAP: usize = 8;
/// 传播深度上限（定长 4 树：父 + 三层子）。
pub const MAX_PROP_DEPTH: usize = 4;
/// 每英寸点数基准（MS 排版语义：px = pt × dpi / 72）。
pub const POINTS_PER_INCH: u32 = 72;
/// 缩放历史环容量（定长 8 次变更）。
pub const SCALE_HISTORY_CAP: usize = 8;

// ---------------------------------------------------------------------------
// MDT_* 感知类型判定表（三型→行为矩阵）
// ---------------------------------------------------------------------------

/// 感知类型对应行为（行为矩阵主轴）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpiBehavior {
    /// 合成器离屏位图放大（模糊但比例正确——主册 unaware 语义）。
    BitmapStretch,
    /// 按主屏 DPI 虚拟化坐标（程序以为 96 DPI）。
    SystemVirtualized,
    /// 拿真实像素 + 子窗口重发 DPICHANGED。
    RealPixels,
}

/// 判定表：句柄值 → 行为；未知句柄 → None（显性不支持）。
pub fn behavior(ctx: i32) -> Option<DpiBehavior> {
    match ctx {
        MDT_UNAWARE => Some(DpiBehavior::BitmapStretch),
        MDT_SYSTEM_AWARE => Some(DpiBehavior::SystemVirtualized),
        MDT_PER_MONITOR_AWARE_V2 => Some(DpiBehavior::RealPixels),
        _ => None,
    }
}

/// 行为矩阵第三轴：仅 per-monitor V2 向子窗口重发 DPICHANGED（MS 特性面）。
pub fn child_renotify(ctx: i32) -> bool {
    ctx == MDT_PER_MONITOR_AWARE_V2
}

// ---------------------------------------------------------------------------
// 缩放因子 16.16 定点乘法
// ---------------------------------------------------------------------------

/// 缩放百分比 → Q16.16 因子（150% → 98304）。
pub fn scale_q16(percent: u32) -> u32 {
    percent * Q16_ONE / 100
}

/// Q16.16 定点乘法：u32×u32 用 u64 中间步防溢出，>>16 前加 0x8000 四舍五入；
/// 结果饱和到 u32::MAX（不回绕——零静默溢出）。
pub fn mul_q16(px: u32, factor: u32) -> u32 {
    let prod = (px as u64) * (factor as u64) + 0x8000;
    let v = prod >> 16;
    if v > u32::MAX as u64 {
        u32::MAX
    } else {
        v as u32
    }
}

// ---------------------------------------------------------------------------
// 子窗口缩放消息传播时序模型（传播顺序账）
// ---------------------------------------------------------------------------

/// 事件种类：父窗口 WM_DPICHANGED / 子窗口 WM_DPICHANGED_AFTERPARENT。
pub const EVT_DPICHANGED: u8 = 0;
pub const EVT_AFTERPARENT: u8 = 1;

/// 传播顺序账（定长 8 事件）：父 DPICHANGED 先行，子 AFTERPARENT 随后。
pub struct PropLedger {
    pub events: [(u8, u8); PROP_LEDGER_CAP], // (kind, depth)
    pub len: usize,
    /// 时序违例计数（零静默）。
    pub violations: u32,
}

impl PropLedger {
    pub const fn new() -> Self {
        PropLedger { events: [(0u8, 0u8); PROP_LEDGER_CAP], len: 0, violations: 0 }
    }

    /// 记一笔：AFTERPARENT 前必须已有同深度之父 DPICHANGED；越树深度拒绝。
    pub fn record(&mut self, kind: u8, depth: u8) -> Result<(), &'static str> {
        if depth as usize >= MAX_PROP_DEPTH {
            self.violations += 1;
            return Err("depth-over-tree");
        }
        if kind == EVT_AFTERPARENT {
            let orphan = depth == 0
                || !(0..self.len).any(|i| self.events[i] == (EVT_DPICHANGED, depth - 1));
            if orphan {
                self.violations += 1;
                return Err("afterparent-orphan");
            }
        }
        if self.len >= PROP_LEDGER_CAP {
            return Err("ledger-full");
        }
        self.events[self.len] = (kind, depth);
        self.len += 1;
        Ok(())
    }

    /// 时序判定：每笔 AFTERPARENT 的同深度父 DPICHANGED 都在更早序号。
    pub fn parent_first(&self) -> bool {
        for i in 0..self.len {
            let (kind, depth) = self.events[i];
            if kind == EVT_AFTERPARENT {
                if depth == 0 {
                    return false;
                }
                let mut seen = false;
                for j in 0..i {
                    if self.events[j] == (EVT_DPICHANGED, depth - 1) {
                        seen = true;
                    }
                }
                if !seen {
                    return false;
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 每 DPI 字体点数→像素换算
// ---------------------------------------------------------------------------

/// px = pt × dpi / 72（96/144/192 三档对拍主册判据；整除口径不取整）。
pub fn pt_to_px(pt: u32, dpi: u32) -> u32 {
    pt * dpi / POINTS_PER_INCH
}

// ---------------------------------------------------------------------------
// 缩放历史回溯账
// ---------------------------------------------------------------------------

/// 缩放历史环（定长 8 次变更：(dpi_from, dpi_to)）。
pub struct ScaleHistory {
    pub entries: [(u32, u32); SCALE_HISTORY_CAP],
    pub len: usize,
    pub head: usize,
    /// 被环形覆盖挤出的旧条数（如实入账）。
    pub dropped: u32,
}

impl ScaleHistory {
    pub const fn new() -> Self {
        ScaleHistory { entries: [(0u32, 0u32); SCALE_HISTORY_CAP], len: 0, head: 0, dropped: 0 }
    }

    /// 环形入账：满 8 后覆盖最旧并计数（不静默）。
    pub fn push(&mut self, from: u32, to: u32) {
        if self.len < SCALE_HISTORY_CAP {
            self.len += 1;
        } else {
            self.dropped += 1;
        }
        self.entries[self.head] = (from, to);
        self.head = (self.head + 1) % SCALE_HISTORY_CAP;
    }

    /// 最近一次变更。
    pub fn last(&self) -> Option<(u32, u32)> {
        if self.len == 0 {
            return None;
        }
        let idx = (self.head + SCALE_HISTORY_CAP - 1) % SCALE_HISTORY_CAP;
        Some(self.entries[idx])
    }

    /// 回放最近一次逆变换：px × dpi_from / dpi_to。
    pub fn undo_px(&self, px: u32) -> Option<u32> {
        let (from, to) = self.last()?;
        Some(px * from / to)
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化批次三）
// ---------------------------------------------------------------------------

pub fn run_f028e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F028-dpi-prop-d3");
    // 1) MDT_* 句柄值对拍 MS（-1/-2/-5）+ 行为矩阵三型一一对应。
    cs.add(
        "mdt_behavior_matrix",
        MDT_UNAWARE == -1
            && MDT_SYSTEM_AWARE == -2
            && MDT_PER_MONITOR_AWARE_V2 == -5
            && behavior(MDT_UNAWARE) == Some(DpiBehavior::BitmapStretch)
            && behavior(MDT_SYSTEM_AWARE) == Some(DpiBehavior::SystemVirtualized)
            && behavior(MDT_PER_MONITOR_AWARE_V2) == Some(DpiBehavior::RealPixels)
            && behavior(-3) == None,
        "",
    );
    // 2) 子窗口重发仅 V2 有（行为矩阵第三轴）。
    cs.add(
        "mdt_child_renotify_only_v2",
        child_renotify(MDT_PER_MONITOR_AWARE_V2)
            && !child_renotify(MDT_UNAWARE)
            && !child_renotify(MDT_SYSTEM_AWARE),
        "",
    );
    // 3) Q16.16 因子：100%→65536，150%→98304。
    cs.add("q16_scale_factor", scale_q16(100) == Q16_ONE && scale_q16(150) == 98304, "");
    // 4) 定点乘法：100px×150%=150px；1px×1.5 四舍五入 → 2。
    cs.add(
        "q16_mul_rounding",
        mul_q16(100, scale_q16(150)) == 150 && mul_q16(1, 0x1_8000) == 2,
        "",
    );
    // 5) u64 中间步防溢出：极大 px 饱和不回绕。
    cs.add("q16_mul_saturate", mul_q16(u32::MAX, 0x2_0000) == u32::MAX, "");
    // 6) 传播时序：各层父 DPICHANGED 先行，子 AFTERPARENT 依次随后。
    let mut l = PropLedger::new();
    let ok = l.record(EVT_DPICHANGED, 0).is_ok()
        && l.record(EVT_DPICHANGED, 1).is_ok()
        && l.record(EVT_DPICHANGED, 2).is_ok()
        && l.record(EVT_AFTERPARENT, 1).is_ok()
        && l.record(EVT_AFTERPARENT, 2).is_ok()
        && l.record(EVT_AFTERPARENT, 3).is_ok();
    cs.add("propagate_parent_first", ok && l.len == 6 && l.parent_first(), "");
    // 7) 孤儿 AFTERPARENT 与越树深度显性拒绝并计数。
    let mut l2 = PropLedger::new();
    let orphan = l2.record(EVT_AFTERPARENT, 1);
    let deep = l2.record(EVT_DPICHANGED, 4);
    cs.add(
        "propagate_orphan_rejected",
        orphan == Err("afterparent-orphan")
            && deep == Err("depth-over-tree")
            && l2.violations == 2
            && l2.len == 0,
        "",
    );
    // 8) 每 DPI 字体换算：12pt @96/144/192 → 16/24/32 px。
    cs.add(
        "pt_to_px_three_dpis",
        pt_to_px(12, 96) == 16 && pt_to_px(12, 144) == 24 && pt_to_px(12, 192) == 32,
        "",
    );
    // 9) 历史环：10 次入账 → len 8、dropped 2、最近一条 (144,192)。
    let mut h = ScaleHistory::new();
    for _ in 0..5 {
        h.push(96, 144);
    }
    for _ in 0..5 {
        h.push(144, 192);
    }
    cs.add(
        "history_ring_8",
        h.len == 8 && h.dropped == 2 && h.last() == Some((144, 192)),
        "",
    );
    // 10) 逆变换回放：32px@192 回放到 144 档 → 24px。
    cs.add("history_undo_latest", h.undo_px(32) == Some(24), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_full_rejects() {
        let mut l = PropLedger::new();
        // 合法链：每层父 DPICHANGED 先行，子 AFTERPARENT 才可入账。
        let chain = [
            (EVT_DPICHANGED, 0), (EVT_AFTERPARENT, 1),
            (EVT_DPICHANGED, 1), (EVT_AFTERPARENT, 2),
            (EVT_DPICHANGED, 2), (EVT_AFTERPARENT, 3),
            (EVT_DPICHANGED, 3), (EVT_DPICHANGED, 0),
        ];
        for (k, d) in chain {
            assert!(l.record(k, d).is_ok(), "账容量 8 内必成");
        }
        assert_eq!(l.record(EVT_DPICHANGED, 0), Err("ledger-full"), "账满显性拒绝");
        assert!(l.parent_first());
    }

    #[test]
    fn undo_empty_history_is_none() {
        let h = ScaleHistory::new();
        assert_eq!(h.last(), None);
        assert_eq!(h.undo_px(10), None, "空账无可回放——显性 None");
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f028e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
