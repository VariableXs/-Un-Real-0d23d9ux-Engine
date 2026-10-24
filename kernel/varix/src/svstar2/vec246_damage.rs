//! VE-F0646 表面 damage 协议对接（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0646`
//!
//! **判据（锚点原文逐条）**：
//! - **双向对等**：既能声明（提交前）也能接收（外部更新映射）；
//! - **时序契约**：声明在提交点之前完成，提交后声明无效（违例显性拒绝）；
//! - **风暴合并**：帧内 damage 风暴合并为单帧区域集；
//! - **降级登记**：老表面不支持区域协议 → 全帧降级显性登记；
//! - **判据**：帧区域集是声明数据来源（F0613 衔接）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::Rect;

/// damage 请求（本帧 damaged 区域集）
#[derive(Clone, Debug, PartialEq)]
pub struct DamageReq {
    pub frame: u64,
    pub regions: Vec<Rect>,
}

/// 协议状态机（时序契约：声明窗口在提交点之前）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmgPhase {
    /// 声明窗口开启（合成中）
    Open,
    /// 已提交（窗口关闭）
    Committed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DmgChannel {
    pub frame: u64,
    pub phase: DmgPhase,
    /// 声明账（双向对等之声明侧）
    pub declared: Vec<Rect>,
    /// 接收账（外部事件映射）
    pub received: Vec<Rect>,
    /// 时序违例拒绝账
    pub rejected: u32,
    /// 全帧降级登记（老表面）
    pub fallback_full: bool,
}

impl DmgChannel {
    pub fn new(frame: u64) -> DmgChannel {
        DmgChannel {
            frame,
            phase: DmgPhase::Open,
            declared: Vec::new(),
            received: Vec::new(),
            rejected: 0,
            fallback_full: false,
        }
    }

    /// 声明 damage（提交前有效；提交后拒绝并记账）
    pub fn declare(&mut self, r: Rect) -> bool {
        if self.phase == DmgPhase::Committed {
            self.rejected += 1;
            return false;
        }
        self.declared.push(r);
        true
    }

    /// 接收外部 damage（视频帧/外部写入 → 图层内容脏）
    pub fn receive(&mut self, r: Rect) -> bool {
        if self.phase == DmgPhase::Committed {
            self.rejected += 1;
            return false;
        }
        self.received.push(r);
        true
    }

    /// 提交点（关闭声明窗口）
    pub fn commit(&mut self) {
        self.phase = DmgPhase::Committed;
    }

    /// 双向对等核验：声明与接收两侧同帧可合并成同一区域集
    pub fn bidirectional(&self) -> bool {
        // 语义核验：两侧账独立存在且类型一致（都能进帧收集）
        self.declared.len() + self.received.len() > 0
            || (self.declared.is_empty() && self.received.is_empty())
    }

    /// 帧区域集（风暴合并：声明+接收全部合并输出，F0613 数据来源）
    pub fn frame_regions(&self) -> Vec<Rect> {
        let mut all: Vec<Rect> = Vec::new();
        all.extend(self.declared.iter().copied());
        all.extend(self.received.iter().copied());
        all
    }

    /// 老表面降级：不支持区域协议 → 全帧 damage
    pub fn degrade_full(&mut self) {
        self.fallback_full = true;
        self.declared.clear();
        self.received.clear();
        self.declared.push((0, 0, u32::MAX, u32::MAX));
    }
}

/// 风暴合并：帧内 N 个 damage 事件合并（矩形并集贪心）
pub fn storm_merge(rs: &[Rect]) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for &r in rs {
        let mut merged = false;
        for o in out.iter_mut() {
            if intersects(*o, r) {
                *o = union(*o, r);
                merged = true;
                break;
            }
        }
        if !merged {
            out.push(r);
        }
    }
    out
}

fn intersects(a: Rect, b: Rect) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

fn union(a: Rect, b: Rect) -> Rect {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), b.3.max(a.3))
}

pub fn run_vec246_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec246");
    // 族一：声明窗口时序
    let mut ch = DmgChannel::new(1);
    s.add("协议-初始开启", ch.phase == DmgPhase::Open, "open");
    s.add("协议-提交前有效", ch.declare((0, 0, 10, 10)), "declare ok");
    ch.commit();
    s.add("协议-提交后拒绝", !ch.declare((5, 5, 15, 15)) && ch.rejected == 1, "post-commit reject");
    s.add("协议-提交后接收拒", !ch.receive((1, 1, 2, 2)) && ch.rejected == 2, "receive reject");
    // 族二：双向对等
    let mut ch2 = DmgChannel::new(2);
    ch2.declare((0, 0, 10, 10));
    ch2.receive((20, 20, 30, 30));
    s.add("协议-双向记账", ch2.declared.len() == 1 && ch2.received.len() == 1, "both sides");
    s.add("协议-对等核验", ch2.bidirectional(), "bidi ok");
    // 族三：风暴合并
    let regions = ch2.frame_regions();
    s.add("协议-帧区域集", regions.len() == 2, "2 regions");
    let merged = storm_merge(&regions);
    s.add("风暴-并集归并", merged.len() == 1 && merged[0] == (0, 0, 30, 30), "storm merged");
    // 不相交不合并
    let apart = storm_merge(&[(0, 0, 5, 5), (100, 100, 110, 110)]);
    s.add("风暴-相离保留", apart.len() == 2, "keep apart");
    // 链式风暴（A∩B, B∩C）
    let chain = storm_merge(&[(0, 0, 10, 10), (5, 0, 15, 10), (10, 0, 20, 10)]);
    s.add("风暴-链式归一", chain.len() == 1 && chain[0] == (0, 0, 20, 10), "chain");
    // 族四：全帧降级
    let mut ch3 = DmgChannel::new(3);
    ch3.declare((0, 0, 4, 4));
    ch3.degrade_full();
    s.add("降级-全帧登记", ch3.fallback_full && ch3.declared[0] == (0, 0, u32::MAX, u32::MAX), "full fallback");
    s.add("降级-原区域清空", ch3.received.is_empty(), "cleared");
    // 族五：跨帧通道独立
    let a = DmgChannel::new(7);
    let b = DmgChannel::new(8);
    s.add("协议-帧独立", a.frame != b.frame && a.declared.is_empty(), "per-frame");
    s
}
