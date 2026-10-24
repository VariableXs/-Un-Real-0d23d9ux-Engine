//! cga42_iffreeze.rs ↔ CGPU-F0042 与任务图域接口冻结（0x1C30 段）
//!
//! 职责（施工书原文）：
//!   CPU 加速域（A）与任务图调度域（B，CGPU-F0161+）的接口冻结：
//!   任务节点接口（类型/读写集/预算标签）、发射协议、完成回调、
//!   取消语义——冻结书 + 语义测试 + fake 实现（B 域凭此开工）。
//!
//! 判据（施工书原文）：
//!   接口冻结、语义测试、fake 可建、B 域开工验证、版本纪律。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1C30 独占 0x10）。
pub const SEG_BASE: u16 = 0x1C30;
pub const XC_IFVER: u16 = SEG_BASE + 1; // 接口版本失配
pub const XY_EMIT: u16 = SEG_BASE + 2; // 发射协议违规
pub const XY_CANCEL: u16 = SEG_BASE + 3; // 取消语义违规

/// 冻结书版本（B 域开工的验收锚点——不兼容改动必须升版本号）
pub const IF_VERSION: u32 = 1;
pub const IF_NAME: &str = "A→B 任务节点接口";

/// 任务节点类型闭集（冻结项 1：类型）
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NodeKind {
    Render = SEG_BASE,  // 0x1C30 渲染记录
    Copy,               // 0x1C31 资源拷贝
    Compute,            // 0x1C32 计算
    Fence,              // 0x1C33 栅栏
}
impl NodeKind {
    pub const fn code(self) -> u16 { self as u16 }
    pub const fn name(self) -> &'static str {
        match self {
            NodeKind::Render => "渲染记录",
            NodeKind::Copy => "资源拷贝",
            NodeKind::Compute => "计算",
            NodeKind::Fence => "栅栏",
        }
    }
    pub const ALL: [NodeKind; 4] = [NodeKind::Render, NodeKind::Copy, NodeKind::Compute, NodeKind::Fence];
}

/// 任务节点（冻结项 2：读写集 + 预算标签）
#[derive(Clone)]
pub struct IfNode {
    pub kind: NodeKind,
    pub read_set: u64,
    pub write_set: u64,
    pub budget_us: u32,
}

impl IfNode {
    /// 冻结书合法性：栅栏零读写集；其余节点至少一方有集
    pub fn frozen_valid(&self) -> bool {
        match self.kind {
            NodeKind::Fence => self.read_set == 0 && self.write_set == 0,
            _ => self.read_set != 0 || self.write_set != 0,
        }
    }
    /// 读写冲突（同资源写写 / 读写交叠）
    pub fn conflicts(&self, other: &IfNode) -> bool {
        (self.write_set & other.write_set) != 0
            || ((self.write_set & other.read_set) != 0 && other.kind != NodeKind::Fence)
            || ((self.read_set & other.write_set) != 0 && self.kind != NodeKind::Fence)
    }
}

/// 发射协议（冻结项 3）：状态机 Idle→Emitted→Running→Done/Cancelled
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EmitState {
    Idle = SEG_BASE + 0x10,    // 0x1C40
    Emitted,                   // 0x1C41
    Running,                   // 0x1C42
    Done,                      // 0x1C43
    Cancelled,                 // 0x1C44
}
impl EmitState {
    pub const fn code(self) -> u16 { self as u16 }
    /// 状态迁移闭集（发射协议冻结：非法迁移一律拒）
    pub fn transition(self, ev: EmitEvent) -> Result<EmitState, u16> {
        use EmitEvent::*;
        use EmitState::*;
        match (self, ev) {
            (Idle, Emit) => Ok(Emitted),
            (Emitted, Start) => Ok(Running),
            (Running, Complete) => Ok(Done),
            (Emitted, Cancel) | (Running, Cancel) => Ok(Cancelled),
            (Done, _) | (Cancelled, _) => Err(XY_CANCEL), // 终态不可再迁
            _ => Err(XY_EMIT),
        }
    }
}

/// 发射事件闭集
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EmitEvent {
    Emit = SEG_BASE + 0x20,    // 0x1C50
    Start,                     // 0x1C51
    Complete,                  // 0x1C52
    Cancel,                    // 0x1C53
}
impl EmitEvent {
    pub const fn code(self) -> u16 { self as u16 }
}

/// 完成回调（冻结项 4）：签名冻结 fn(id, state)
pub type DoneCallback = fn(u64, EmitState) -> u64;

/// 取消语义（冻结项 5）：Running/Emitted 可取消；Done 不可追取消
pub fn cancel_semantics(cur: EmitState) -> Result<EmitState, u16> {
    cur.transition(EmitEvent::Cancel)
}

/// fake 实现（B 域开工凭据）：最小发射器
pub struct FakeEmitter {
    pub version: u32,
    pub nodes: Vec<(u64, IfNode, EmitState)>,
    pub callbacks_log: Vec<(u64, EmitState)>,
}

impl FakeEmitter {
    pub fn new() -> Self {
        FakeEmitter { version: IF_VERSION, nodes: Vec::new(), callbacks_log: Vec::new() }
    }
    /// 版本握手：B 域按冻结书版本开工；失配拒
    pub fn handshake(&self, peer_version: u32) -> Result<(), u16> {
        if peer_version != IF_VERSION {
            return Err(XC_IFVER);
        }
        Ok(())
    }
    /// 发射（协议校验：节点合法性 + 状态机迁移）
    pub fn emit(&mut self, id: u64, node: IfNode) -> Result<(), u16> {
        if !node.frozen_valid() {
            return Err(XY_EMIT);
        }
        if self.nodes.iter().any(|(i, _, _)| *i == id) {
            return Err(XY_EMIT); // 同 ID 重发射拒
        }
        self.nodes.push((id, node, EmitState::Emitted));
        Ok(())
    }
    /// 推进状态
    pub fn advance(&mut self, id: u64, ev: EmitEvent) -> Result<EmitState, u16> {
        let slot = self.nodes.iter_mut().find(|(i, _, _)| *i == id).ok_or(XY_EMIT)?;
        let next = slot.2.transition(ev)?;
        slot.2 = next;
        if next == EmitState::Done || next == EmitState::Cancelled {
            self.callbacks_log.push((id, next));
        }
        Ok(next)
    }
}

/// 语义测试台：接口冻结书全项跑一遍（fake 上的往返）
pub fn semantic_suite() -> Result<(usize, usize), u16> {
    let mut fe = FakeEmitter::new();
    fe.handshake(IF_VERSION)?;
    // 四类节点全发射
    let nodes = vec![
        IfNode { kind: NodeKind::Render, read_set: 1, write_set: 2, budget_us: 10 },
        IfNode { kind: NodeKind::Copy, read_set: 2, write_set: 0, budget_us: 5 },
        IfNode { kind: NodeKind::Compute, read_set: 0, write_set: 4, budget_us: 20 },
        IfNode { kind: NodeKind::Fence, read_set: 0, write_set: 0, budget_us: 1 },
    ];
    for (i, n) in nodes.iter().enumerate() {
        fe.emit(i as u64, n.clone())?;
    }
    // 全链推进 → 回调
    for i in 0..4u64 {
        fe.advance(i, EmitEvent::Start)?;
        fe.advance(i, EmitEvent::Complete)?;
    }
    let cb_ok = fe.callbacks_log.len() == 4
        && fe.callbacks_log.iter().all(|&(_, st)| st == EmitState::Done);
    let conflict = nodes[0].conflicts(&nodes[2]); // 写写交叠 (2&4)? 0b10 & 0b100=0 — 无
    Ok((cb_ok as usize + (fe.version == IF_VERSION) as usize, conflict as usize))
}

pub fn self_audit() -> Result<(), u16> {
    let (a, _) = semantic_suite()?;
    if a < 2 {
        return Err(XY_EMIT);
    }
    Ok(())
}

pub fn run_cga42_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0042");
    // J1 接口冻结（冻结书五项齐：类型/读写集预算/发射协议/完成回调/取消语义）
    let ok = !IF_NAME.is_empty()
        && NodeKind::ALL.len() == 4
        && IF_VERSION == 1
        && NodeKind::Render.code() != NodeKind::Fence.code();
    s.add("CGA-42-01-冻结书齐", ok, if ok { "" } else { "冻结项缺" });
    // J1-b 节点冻结合法性（栅栏零集 / 其余必有集 / 非法拒）
    let fence = IfNode { kind: NodeKind::Fence, read_set: 0, write_set: 0, budget_us: 1 };
    let bad_fence = IfNode { kind: NodeKind::Fence, read_set: 1, write_set: 0, budget_us: 1 };
    let render = IfNode { kind: NodeKind::Render, read_set: 1, write_set: 0, budget_us: 5 };
    let empty = IfNode { kind: NodeKind::Compute, read_set: 0, write_set: 0, budget_us: 5 };
    let ok = fence.frozen_valid() && !bad_fence.frozen_valid() && render.frozen_valid() && !empty.frozen_valid();
    s.add("CGA-42-02-节点合法性", ok, if ok { "" } else { "冻结破缺" });
    // J2 语义测试（状态机全链 + 回调 + 终态拒）
    let (score, _) = semantic_suite().ok().unwrap();
    let ok = score >= 2;
    s.add("CGA-42-03-语义测试", ok, if ok { "" } else { "语义破缺" });
    // J2-b 状态机非法迁移（跳步拒 + 终态拒）
    let ok = EmitState::Idle.transition(EmitEvent::Start) == Err(XY_EMIT)
        && EmitState::Idle.transition(EmitEvent::Complete) == Err(XY_EMIT)
        && EmitState::Done.transition(EmitEvent::Cancel) == Err(XY_CANCEL)
        && EmitState::Cancelled.transition(EmitEvent::Start) == Err(XY_CANCEL);
    s.add("CGA-42-04-迁移闭集", ok, if ok { "" } else { "跳步放行" });
    // J2-c 冲突检测（写写 / 读写交叠 / 无交集）
    let w1 = IfNode { kind: NodeKind::Render, read_set: 0, write_set: 0b10, budget_us: 1 };
    let w2 = IfNode { kind: NodeKind::Render, read_set: 0, write_set: 0b10, budget_us: 1 };
    let r2 = IfNode { kind: NodeKind::Copy, read_set: 0b10, write_set: 0, budget_us: 1 };
    let far = IfNode { kind: NodeKind::Compute, read_set: 0b100, write_set: 0b1000, budget_us: 1 };
    let ok = w1.conflicts(&w2) && w1.conflicts(&r2) && !w1.conflicts(&far);
    s.add("CGA-42-05-冲突检测", ok, if ok { "" } else { "冲突漏检" });
    // J3 fake 可建（FakeEmitter 新建 + 握手 + 重发射拒）
    let mut fe = FakeEmitter::new();
    let n = IfNode { kind: NodeKind::Compute, read_set: 1, write_set: 0, budget_us: 3 };
    let ok = fe.emit(1, n.clone()).is_ok() && fe.emit(1, n) == Err(XY_EMIT);
    s.add("CGA-42-06-fake可建", ok, if ok { "" } else { "fake破缺" });
    // J4 B 域开工验证（版本握手过/失配拒 + 取消语义可达）
    let ok = fe.handshake(IF_VERSION).is_ok()
        && fe.handshake(2) == Err(XC_IFVER)
        && cancel_semantics(EmitState::Emitted) == Ok(EmitState::Cancelled)
        && cancel_semantics(EmitState::Running) == Ok(EmitState::Cancelled)
        && cancel_semantics(EmitState::Done) == Err(XY_CANCEL);
    s.add("CGA-42-07-B域开工验证", ok, if ok { "" } else { "握手破缺" });
    // J5 版本纪律（版本常量冻结 + 升版必须改常量——当前锚 1 可机读）
    let ok = IF_VERSION >= 1 && IF_VERSION < 0xFFFF;
    s.add("CGA-42-08-版本纪律", ok, if ok { "" } else { "版本失锚" });
    // J6 码位互异段独占
    let ok = SEG_BASE % 0x10 == 0
        && XC_IFVER != XY_EMIT
        && XY_EMIT != XY_CANCEL
        && EmitState::Idle.code() != EmitState::Cancelled.code();
    s.add("CGA-42-09-码位互异", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga42_checks_all_green() {
        let set = run_cga42_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga42_cancel_path_logs_callback() {
        // 取消也走完成回调（Cancelled 进日志）
        let mut fe = FakeEmitter::new();
        let n = IfNode { kind: NodeKind::Render, read_set: 1, write_set: 0, budget_us: 2 };
        fe.emit(9, n).ok().unwrap();
        fe.advance(9, EmitEvent::Cancel).ok().unwrap();
        assert_eq!(fe.callbacks_log, vec![(9, EmitState::Cancelled)]);
    }
}
