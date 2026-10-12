//! UNX-F1221 Draining 态 · 并行窗口串行化
//! 判据原文（CoRun Unxreal 主册）：UNX-F1221-J1 双模并行（轮询泵+中断通道同时活跃）事件零丢失（双源恒等断言 10^5 事件），并行窗口双源事件串行化正确率 100%（同源序不乱），排空循环退出条件全验证
//! · 组件：合并序列器（双源事件按序号单列化）+ 序号单调 + 顺序守恒
//! · 判据落地：并行窗口双源事件串行化正确率 100%（同源序不乱）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const DLZ_VERSION: &str = "unx21-v1";

/// 诊断段基址。
pub const DLZ_SEG_BASE: u16 = 0x6370;

// --------------------------------------------------------------------------
// 诊断码（dlz 段 · 8 枚互斥 + segment_guard 双写对拍）
// --------------------------------------------------------------------------
/// 串行化诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum dlzCode {
    /// 序号倒退。
    SeqBackwards,
    /// 序号重复。
    SeqDup,
    /// 跨源序号交错乱。
    SrcCross,
    /// 合流序号断档。
    MergeGap,
    /// 同源序被打乱。
    InterleaveBad,
    /// 串行化偏向单源。
    SerialBias,
    /// 正确率未满即宣告。
    FinalNotGreen,
    /// 导出不符。
    ExportBad,
}

impl dlzCode {
    /// 全量闭集。
    pub const ALL: [dlzCode; 8] = [
        dlzCode::SeqBackwards,
        dlzCode::SeqDup,
        dlzCode::SrcCross,
        dlzCode::MergeGap,
        dlzCode::InterleaveBad,
        dlzCode::SerialBias,
        dlzCode::FinalNotGreen,
        dlzCode::ExportBad,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            dlzCode::SeqBackwards => 0x6370 + 0,
            dlzCode::SeqDup => 0x6370 + 1,
            dlzCode::SrcCross => 0x6370 + 2,
            dlzCode::MergeGap => 0x6370 + 3,
            dlzCode::InterleaveBad => 0x6370 + 4,
            dlzCode::SerialBias => 0x6370 + 5,
            dlzCode::FinalNotGreen => 0x6370 + 6,
            dlzCode::ExportBad => 0x6370 + 7,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            dlzCode::SeqBackwards => "序号倒退",
            dlzCode::SeqDup => "序号重复",
            dlzCode::SrcCross => "跨源序号交错乱",
            dlzCode::MergeGap => "合流序号断档",
            dlzCode::InterleaveBad => "同源序被打乱",
            dlzCode::SerialBias => "串行化偏向单源",
            dlzCode::FinalNotGreen => "正确率未满即宣告",
            dlzCode::ExportBad => "导出不符",
        }
    }

    /// 可操作修复建议（每条诊断都带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            dlzCode::SeqBackwards => "拒收并记账",
            dlzCode::SeqDup => "去重",
            dlzCode::SrcCross => "按源分段",
            dlzCode::MergeGap => "补号",
            dlzCode::InterleaveBad => "恢复同源序",
            dlzCode::SerialBias => "按序号轮转",
            dlzCode::FinalNotGreen => "达标后才宣告",
            dlzCode::ExportBad => "按 schema 校正",
        }
    }
}


// ---------------------------------------------------------------------------
// 序号串行化器（双源事件按全局序号单列化；同源序必须单调）
// ---------------------------------------------------------------------------

/// 一个待串行化事件。
#[derive(Clone, Copy, Debug)]
pub struct SeqEvent {
    /// 源标识（0 = 轮询，1 = 中断）。
    pub src: u8,
    /// 源内序号。
    pub seq: u64,
}

/// 串行化器（每源独立单调检查）。
pub struct Serializer {
    /// 源 0 的下一个期望序号。
    pub next0: u64,
    /// 源 1 的下一个期望序号。
    pub next1: u64,
    /// 接受数。
    pub accepted: u64,
    /// 拒绝数。
    pub rejected: u64,
}

impl Serializer {
    /// 初态（期望序号 0）。
    pub const fn new() -> Self {
        Self { next0: 0, next1: 0, accepted: 0, rejected: 0 }
    }

    /// 施加一个事件（true = 接受；序号必须等于该源期望值）。
    pub fn push(&mut self, e: SeqEvent) -> bool {
        let want = if e.src == 0 { self.next0 } else { self.next1 };
        if e.seq != want {
            self.rejected += 1;
            return false;
        }
        if e.src == 0 {
            self.next0 += 1;
        } else {
            self.next1 += 1;
        }
        self.accepted += 1;
        true
    }

    /// 串行化正确率（万分比）。
    pub fn rate(&self) -> u64 {
        let total = self.accepted + self.rejected;
        if total == 0 {
            return 0;
        }
        ((self.accepted as u128 * 10000 / total as u128) as u64)
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（诊断码必须落在本模块段内；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码 ∈ [0x6370, 0x637f]。
pub const fn dlz_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < dlzCode::ALL.len() {
        let c = dlzCode::ALL[i].code();
        if c < 0x6370 || c > 0x637f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：逐检查带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 同源序不乱（双源交错合法）。
fn c1_order() -> bool {
    let mut s = Serializer::new();
    s.push(SeqEvent { src: 0, seq: 0 })
        && s.push(SeqEvent { src: 1, seq: 0 })
        && s.push(SeqEvent { src: 1, seq: 1 })
        && s.push(SeqEvent { src: 0, seq: 1 })
        && s.accepted == 4 && s.rejected == 0
}

/// 同源乱序被拒。
///
/// 期望值按 `rate = accepted×10000/(accepted+rejected)` 独立重算：
/// 三推一拒 ⇒ 2 接受 / 1 拒绝 ⇒ 2×10000/3 = 6666（截断取整）。
/// （原断言写 7500 是拿三条全过当分子的误算——orphan 期无人编译，
/// 注册进来才炸。判据值必须从公式独立复算，不能从情景印象倒推。）
fn c2_reorder() -> bool {
    let mut s = Serializer::new();
    s.push(SeqEvent { src: 0, seq: 0 })
        && !s.push(SeqEvent { src: 0, seq: 2 })
        && s.rejected == 1
        && s.push(SeqEvent { src: 0, seq: 1 })
        && s.rate() == 6666
}

/// 段位守卫：全部诊断码落在本模块段内。
fn dlz_guard_seg() -> bool {
    dlz_segment_guard()
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_dlz_checks() -> CheckSet {
    let mut set = CheckSet::new("dlz");
    set.add(
        "F1221-ser-c1",
        c1_order(),
        if c1_order() { "" } else { "same-source order broken" },
    );
    set.add(
        "F1221-ser-c2",
        c2_reorder(),
        if c2_reorder() { "" } else { "reordered same-source event accepted" },
    );
    set.add(
        "dlz-seg-guard",
        dlz_guard_seg(),
        if dlz_guard_seg() { "" } else { "diagnostic code outside module segment" },
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
    fn dlz_all_checks_green() {
        let set = run_dlz_checks();
        assert!(set.all_passed(), "DLZ domain checks must be green");
    }

    #[test]
    fn dlz_segment_guard_holds() {
        assert!(dlz_segment_guard());
    }
}
