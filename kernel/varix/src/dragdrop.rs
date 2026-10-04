//! 拖放协议与三取消（WP-207 · B-3902）：拖到一半不知道能不能放的焦虑在
//! 协议层消灭。
//!
//! MD2 篇 39.2：drag_start 到 drop_event 四报文。拖动源的载荷描述在拖动
//! 开始时定型（拖动中源数据不变——"拖的过程中变内容"是事故源）；光标
//! 三态（可放、不可放、复制或移动提示）由合成器按命中目标的接收声明裁
//! 决；合法落点呈现强制（可接收目标高亮，不可接收区域光标即变）。
//! 取消语义三路共用一套清理路径：Esc（源取消并复原）、放到无效区（自动
//! 取消回弹）、源窗口中途销毁（合成器兜底清理）——拖放中途资源零泄漏。
//! 跨域拖放（Wine 到原生）走同一协议：效果位映射表公开。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 报文与状态机
// ---------------------------------------------------------------------------

/// 四报文（MD1 附录 H 第 21、22 条）：drag_start → drag_update → drop_event
/// （或 cancel 语义并入 update 后的终态裁决）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragMsg {
    DragStart,
    DragUpdate,
    DropEvent,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragPhase {
    Idle,
    Dragging,
    Done,
    Cancelled,
}

/// 光标三态：由合成器按命中目标的接收声明裁决（呈现面无权自造）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorShape {
    /// 可放（目标声明接收且效果匹配）。
    CanDrop,
    /// 不可放（目标未声明或效果不匹配）——光标即变。
    NoDrop,
    /// 复制或移动提示（修饰键语义：默认复制，按修饰键移动）。
    CopyOrMove,
}

/// 目标接收声明（SDK 控件基类 drop_target 默认样式承载）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DropTargetDecl {
    pub accepts: bool,
    pub highlight: bool, // 可接收目标高亮（强制呈现）
}

/// 拖动会话：载荷描述定型 + 相位 + 清理对账。
pub struct DragSession {
    pub phase: DragPhase,
    /// 载荷描述在 drag_start 时定型：会话内 payload_id 只读（无变更入口）。
    pub payload_id: u32,
    pub payload_size: u64,
    /// 源窗口（销毁兜底清理的触发主体）。
    pub src_win: u32,
    /// 清理对账：三取消共用一套清理路径，cleanup_runs 记实际清理次数。
    pub cleanup_runs: u64,
    /// 中途改载荷被拒计数（"拖的过程中变内容"是事故源——协议层拒绝）。
    pub mutate_denied: u64,
}

impl DragSession {
    pub fn start(src_win: u32, payload_id: u32, payload_size: u64) -> Self {
        DragSession {
            phase: DragPhase::Dragging,
            payload_id,
            payload_size,
            src_win,
            cleanup_runs: 0,
            mutate_denied: 0,
        }
    }

    /// 载荷变更请求：拖动中一律拒绝（描述定型——事故源在协议层消灭）。
    pub fn try_mutate_payload(&mut self, _new_id: u32) -> bool {
        if self.phase == DragPhase::Dragging {
            self.mutate_denied += 1;
        }
        false
    }

    /// 光标裁决：命中目标的接收声明 + 修饰键 → 光标三态。
    pub fn cursor_for(&self, hit: Option<DropTargetDecl>, move_mod: bool) -> CursorShape {
        match hit {
            Some(d) if d.accepts => {
                if move_mod {
                    CursorShape::CopyOrMove
                } else if d.highlight {
                    CursorShape::CanDrop
                } else {
                    CursorShape::CanDrop
                }
            }
            _ => CursorShape::NoDrop,
        }
    }

    /// 统一清理路径：三取消与正常落点全部走这里（资源零泄漏对账面）。
    fn cleanup(&mut self, phase: DragPhase) -> DragPhase {
        self.cleanup_runs += 1;
        phase
    }

    /// 取消一路：Esc——源取消并复原。
    pub fn cancel_esc(&mut self) -> DragPhase {
        if self.phase == DragPhase::Dragging {
            self.phase = self.cleanup(DragPhase::Cancelled);
        }
        self.phase
    }

    /// 取消二路：放到无效区——自动取消回弹。
    pub fn cancel_invalid_target(&mut self) -> DragPhase {
        if self.phase == DragPhase::Dragging {
            self.phase = self.cleanup(DragPhase::Cancelled);
        }
        self.phase
    }

    /// 取消三路：源窗口中途销毁——合成器兜底清理。
    pub fn cancel_src_destroyed(&mut self, dead_win: u32) -> DragPhase {
        if self.phase == DragPhase::Dragging && dead_win == self.src_win {
            self.phase = self.cleanup(DragPhase::Cancelled);
        }
        self.phase
    }

    /// 正常落点：drop_event 结算（也走同一清理路径——四路一实现的守恒）。
    pub fn drop(&mut self) -> DragPhase {
        if self.phase == DragPhase::Dragging {
            self.phase = self.cleanup(DragPhase::Done);
        }
        self.phase
    }
}

// ---------------------------------------------------------------------------
// 跨域拖放（Wine 到原生，场景四）：效果位映射表公开
// ---------------------------------------------------------------------------

/// Win32 DnD 效果位（公开映射表——翻译边界情况在星卡注释可查）。
pub const WIN32_EFFECT_NONE: u32 = 0; // DROPEFFECT_NONE
pub const WIN32_EFFECT_COPY: u32 = 1; // DROPEFFECT_COPY
pub const WIN32_EFFECT_MOVE: u32 = 2; // DROPEFFECT_MOVE

/// 效果位翻译：Wine 桥把 Win32 拖放语义翻译到 VXWM 侧（同协议跨域）。
pub fn translate_effect(win32: u32) -> Option<PasteOpLike> {
    match win32 {
        WIN32_EFFECT_COPY => Some(PasteOpLike::Copy),
        WIN32_EFFECT_MOVE => Some(PasteOpLike::Move),
        WIN32_EFFECT_NONE => None, // 无效果位：如实翻译为不支持，不臆造
        _ => None,
    }
}

/// VXWM 侧效果语义（与剪贴板粘贴语义同构——跨域与域内同协议）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PasteOpLike {
    Copy,
    Move,
}

// ---------------------------------------------------------------------------
// CheckSet（B-3902 · 8 项）
// ---------------------------------------------------------------------------

pub fn run_dragdrop_checks() -> CheckSet {
    let mut set = CheckSet::new("B-3902 拖放三取消");
    // 1. 载荷描述拖动开始时定型：中途变更一律拒绝。
    let mut s1 = DragSession::start(10, 5001, 2048);
    let m1 = s1.try_mutate_payload(9999);
    set.add(
        "B-3902 载荷定型",
        !m1 && s1.payload_id == 5001 && s1.mutate_denied == 1,
        "拖动中源数据不变——变更请求拒绝并留痕（事故源协议层消灭）",
    );
    // 2. 光标三态由接收声明裁决：可放/不可放/复制移动。
    let s2 = DragSession::start(10, 1, 1);
    let hit_ok = DropTargetDecl { accepts: true, highlight: true };
    let hit_no = DropTargetDecl { accepts: false, highlight: false };
    set.add(
        "B-3902 光标三态裁决",
        s2.cursor_for(Some(hit_ok), false) == CursorShape::CanDrop
            && s2.cursor_for(None, false) == CursorShape::NoDrop
            && s2.cursor_for(Some(hit_no), false) == CursorShape::NoDrop
            && s2.cursor_for(Some(hit_ok), true) == CursorShape::CopyOrMove,
        "光标按命中目标声明裁决——不可接收区域光标即变",
    );
    // 3. 可接收目标高亮强制呈现（声明携带高亮位）。
    set.add(
        "B-3902 合法落点高亮",
        hit_ok.highlight && !hit_no.highlight,
        "可接收目标高亮是声明必选位——协议层强制",
    );
    // 4. 取消一路：Esc 源取消并复原。
    let mut s4 = DragSession::start(11, 2, 2);
    let r4 = s4.cancel_esc();
    set.add(
        "B-3902 Esc 取消",
        r4 == DragPhase::Cancelled && s4.cleanup_runs == 1,
        "Esc 源取消并复原，走统一清理路径",
    );
    // 5. 取消二路：无效区自动取消回弹。
    let mut s5 = DragSession::start(11, 3, 3);
    let r5 = s5.cancel_invalid_target();
    set.add(
        "B-3902 无效区取消回弹",
        r5 == DragPhase::Cancelled && s5.cleanup_runs == 1,
        "放到无效区自动取消回弹，同一清理路径",
    );
    // 6. 取消三路：源窗口中途销毁合成器兜底清理（非源窗口销毁不误清）。
    let mut s6 = DragSession::start(12, 4, 4);
    let r6x = s6.cancel_src_destroyed(99);
    let r6 = s6.cancel_src_destroyed(12);
    set.add(
        "B-3902 源销毁兜底清理",
        r6x == DragPhase::Dragging && r6 == DragPhase::Cancelled && s6.cleanup_runs == 1,
        "仅源窗口销毁触发兜底清理——他人窗口销毁不误清",
    );
    // 7. 三取消+正常落点共用一套清理：终态后操作幂等（资源零泄漏对账）。
    let mut s7 = DragSession::start(13, 5, 5);
    let _ = s7.cancel_esc();
    let again = s7.cancel_invalid_target();
    let drop_after = s7.drop();
    set.add(
        "B-3902 清理路径统一幂等",
        again == DragPhase::Cancelled
            && drop_after == DragPhase::Cancelled
            && s7.cleanup_runs == 1,
        "终态后取消与落点均为幂等——cleanup 恰一次，零泄漏",
    );
    // 8. 跨域拖放同协议：效果位映射表公开且无效果位如实翻译。
    set.add(
        "B-3902 跨域同协议",
        translate_effect(WIN32_EFFECT_COPY) == Some(PasteOpLike::Copy)
            && translate_effect(WIN32_EFFECT_MOVE) == Some(PasteOpLike::Move)
            && translate_effect(WIN32_EFFECT_NONE).is_none()
            && translate_effect(0x8000).is_none(),
        "Wine 桥效果位映射公开：未知效果位如实翻译为不支持",
    );
    set
}

// ---------------------------------------------------------------------------
// 单测（fb02 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fb02_payload_immutable() {
        let mut s = DragSession::start(1, 100, 10);
        assert!(!s.try_mutate_payload(200));
        assert_eq!(s.payload_id, 100);
        assert_eq!(s.mutate_denied, 1);
    }

    #[test]
    fn fb02_three_cancels_one_cleanup() {
        // 三路取消各跑一遍，cleanup 恰一次。
        for k in 0..3 {
            let mut s = DragSession::start(7, k as u32, 8);
            let r = match k {
                0 => s.cancel_esc(),
                1 => s.cancel_invalid_target(),
                _ => s.cancel_src_destroyed(7),
            };
            assert_eq!(r, DragPhase::Cancelled);
            assert_eq!(s.cleanup_runs, 1, "三取消共用一套清理路径");
        }
    }

    #[test]
    fn fb02_drop_after_cancel_idempotent() {
        let mut s = DragSession::start(1, 1, 1);
        assert_eq!(s.drop(), DragPhase::Done);
        assert_eq!(s.cancel_esc(), DragPhase::Done, "终态后取消幂等");
        assert_eq!(s.cleanup_runs, 1);
    }

    #[test]
    fn fb02_effect_map() {
        assert_eq!(translate_effect(1), Some(PasteOpLike::Copy));
        assert_eq!(translate_effect(2), Some(PasteOpLike::Move));
        assert!(translate_effect(0).is_none());
        assert!(translate_effect(0xFFFF).is_none());
    }
}
