//! VE-F0252 · AMD 多卡拓扑（VE-B 域 · AMD 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0252`
//!
//! **职责定位（锚点原文）**：多卡拓扑呈现（链路类型×带宽）、显式选择目
//! 标卡（明确声明不做隐式负载分摊），跨卡资源句柄隔离；单卡路径为默认
//! 且性能不劣化；卡移除时相关资源回收并通知。
//!
//! **数据结构（锚点原文）**：拓扑表（卡×链路×带宽）；选择记录；隔离句
//! 柄表。
//!
//! **判据（锚点原文）**：显式选择、隐式禁用、拓扑呈现、隔离。
//! 本模块 13 条判据逐条映射：**显式选择 4 条**（选择必须显式调用三要素
//! 齐全／选择记录入账可查／重复显式选择更新记录／未选择时提交拒绝）、
//! **隐式禁用 2 条**（无隐式负载分摊 API／跨卡自动分发路径不存在）、
//! **拓扑呈现 3 条**（拓扑表卡×链路×带宽三元组入表／链路类型命名渲染
//! ／拓扑读取失败按独立卡降级）、**隔离 3 条**（跨卡句柄访问拒绝／卡移
//! 除资源回收计数／回收后通知事件入账）、**判据自身 1 条**（码位互异
//! 段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 拓扑读取失败 → **按独立卡处理** | [`MultiGpu::probe_fail`]：拓扑表清空后每卡独立路径，显式选择仍可用 |
//! | 选择越界 → **拒绝三要素** | 目标卡 + 链路 + 带宽三者任一越界即拒——不是静默钳位 |
//! | 卡移除 → **相关资源回收加通知** | [`MultiGpu::remove_card`] 回收句柄表 + 通知账——不是静默失效 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **隐式禁用靠「没有」保证**：本模块不提供任何「把工作自动摊到多
//!    卡」的入口——隐式分摊在 API 面上不存在，比「提供但默认关」诚实。
//! 2. **句柄带卡号前缀**：隔离句柄表把句柄编码为 (卡号， 本地号) 二元
//!    组，跨卡访问在查表层就拒绝——类型即边界。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xBB40 独占 0x10）。
pub const SEG_BASE: u32 = 0xBB40;
pub const XC_NO_TARGET: u32 = SEG_BASE + 1; // 未显式选择目标卡
pub const XC_OOB: u32 = SEG_BASE + 2; // 选择越界
pub const XC_XCARD: u32 = SEG_BASE + 3; // 跨卡句柄访问

/// 链路类型。
pub const L_PCIE: u8 = 0;
pub const L_XGMI: u8 = 1; // AMD 专有高速互联
pub const L_NONE: u8 = 2; // 无直连（经主机中转）

/// 链路类型名。
pub fn link_name(l: u8) -> &'static str {
    match l {
        L_PCIE => "PCIe",
        L_XGMI => "xGMI",
        L_NONE => "无直连",
        _ => "?",
    }
}

/// 拓扑表条目：卡 × 链路 × 带宽（GB/s）。
#[derive(Clone)]
pub struct TopoEntry {
    pub card: u8,
    pub peer: u8,
    pub link: u8,
    pub gbps: u32,
}

/// 隔离句柄：(卡号， 本地号) 编码为 u64。
pub fn encode_handle(card: u8, local: u32) -> u64 {
    ((card as u64) << 32) | local as u64
}
pub fn handle_card(h: u64) -> u8 {
    (h >> 32) as u8
}

/// 通知事件。
#[derive(Clone)]
pub struct Notice {
    pub card: u8,
    pub text: String,
}

/// 多卡拓扑器。
pub struct MultiGpu {
    pub cards: Vec<u8>,            // 在卡列表
    pub topo: Vec<TopoEntry>,      // 拓扑表
    pub selected: Option<u8>,      // 显式选择记录
    pub handles: Vec<(u64, u8)>,   // 隔离句柄表（句柄→属卡）
    pub notices: Vec<Notice>,      // 通知账
    pub reclaimed: u32,            // 回收计数
}

impl MultiGpu {
    pub fn new() -> Self {
        MultiGpu {
            cards: Vec::new(),
            topo: Vec::new(),
            selected: None,
            handles: Vec::new(),
            notices: Vec::new(),
            reclaimed: 0,
        }
    }

    /// 拓扑探测失败 → 按独立卡处理（拓扑表清空，显式选择仍可用）。
    pub fn probe_fail(&mut self) {
        self.topo.clear();
        self.notices.push(Notice {
            card: 0xff,
            text: "拓扑读取失败，按独立卡处理".into(),
        });
    }

    /// 显式选择目标卡：目标卡 + 链路 + 带宽三要素齐全才落账。
    pub fn select(&mut self, card: u8, link: u8, gbps: u32) -> Result<(), u32> {
        if !self.cards.contains(&card) {
            return Err(XC_OOB);
        }
        if link > L_NONE || gbps == 0 {
            return Err(XC_OOB);
        }
        self.selected = Some(card);
        Ok(())
    }

    /// 提交核：未显式选择目标卡 → 拒绝（不做隐式分摊兜底）。
    pub fn submit(&self, payload: u64) -> Result<u64, u32> {
        match self.selected {
            Some(c) => Ok(encode_handle(c, payload as u32)),
            None => Err(XC_NO_TARGET),
        }
    }

    /// 跨卡句柄校验：句柄属卡 != 当前选择 → 拒绝（隔离边界）。
    pub fn validate_handle(&self, h: u64) -> Result<(), u32> {
        let hc = handle_card(h);
        match self.selected {
            Some(c) if c == hc => Ok(()),
            _ => Err(XC_XCARD),
        }
    }

    /// 卡移除：回收属卡句柄 + 通知入账。
    pub fn remove_card(&mut self, card: u8) -> u32 {
        let before = self.handles.len();
        self.handles.retain(|(h, _)| handle_card(*h) != card);
        self.reclaimed += (before - self.handles.len()) as u32;
        if self.selected == Some(card) {
            self.selected = None;
        }
        self.cards.retain(|c| *c != card);
        self.topo.retain(|t| t.card != card && t.peer != card);
        self.notices.push(Notice {
            card,
            text: format!("卡移除，回收资源 {}", self.reclaimed),
        });
        self.reclaimed
    }

    /// 拓扑渲染。
    pub fn render_topo(&self) -> String {
        let mut out = format!(
            "多卡拓扑 卡数={} 直连对={} 选择={}\n",
            self.cards.len(),
            self.topo.len(),
            self.selected.map(|c| c.to_string()).unwrap_or_else(|| "未选".into())
        );
        for t in &self.topo {
            let line = format!(
                "  卡{}↔卡{} 链路={} 带宽={}GB/s\n",
                t.card, t.peer, link_name(t.link), t.gbps
            );
            out.push_str(&line);
        }
        out
    }
}

impl Default for MultiGpu {
    fn default() -> Self {
        Self::new()
    }
}

/// 双卡 xGMI 场景核。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut g = MultiGpu::new();
    g.cards = vec![0, 1];
    g.topo.push(TopoEntry { card: 0, peer: 1, link: L_XGMI, gbps: 92 });
    // 1) 未选择提交拒绝
    if g.submit(1) == Err(XC_NO_TARGET) {
        caught += 1;
    }
    // 2) 显式选择 + 提交编码属卡
    if g.select(0, L_XGMI, 92).is_ok() && g.submit(7).map(encode_card_of).ok() == Some(0) {
        caught += 1;
    }
    // 3) 越界选择拒
    if g.select(9, L_XGMI, 92) == Err(XC_OOB) {
        caught += 1;
    }
    // 4) 跨卡句柄拒
    let foreign = encode_handle(1, 7);
    if g.validate_handle(foreign) == Err(XC_XCARD) {
        caught += 1;
    }
    // 5) 拓扑渲染含链路名与带宽
    if g.render_topo().contains("xGMI") && g.render_topo().contains("92GB/s") {
        caught += 1;
    }
    (caught, 5)
}

fn encode_card_of(h: u64) -> u8 {
    handle_card(h)
}

/// 判据入口。
pub fn run_veb252_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0252");
    let mut g = MultiGpu::new();
    g.cards = vec![0, 1, 2];
    g.topo.push(TopoEntry { card: 0, peer: 1, link: L_XGMI, gbps: 92 });
    g.topo.push(TopoEntry { card: 1, peer: 2, link: L_PCIE, gbps: 32 });
    // J1-1 未显式选择提交拒绝（无隐式兜底）
    let ok = g.submit(1) == Err(XC_NO_TARGET);
    s.add("AMD-252-01-未选拒绝", ok, if ok { "" } else { "隐式兜底渗入" });
    // J1-2 显式选择三要素（卡+链路+带宽齐全）
    let ok = g.select(0, L_XGMI, 92).is_ok() && g.selected == Some(0);
    s.add("AMD-252-02-显式选择", ok, if ok { "" } else { "选择破缺" });
    // J1-3 选择越界拒绝三要素
    let ok = g.select(9, L_XGMI, 92) == Err(XC_OOB)
        && g.select(0, 9, 92) == Err(XC_OOB)
        && g.select(0, L_XGMI, 0) == Err(XC_OOB);
    s.add("AMD-252-03-越界拒绝", ok, if ok { "" } else { "越界放水" });
    // J1-4 提交编码属卡（隔离句柄表前缀）
    let h = g.submit(7).unwrap_or(0);
    let ok = handle_card(h) == 0;
    s.add("AMD-252-04-句柄带属卡", ok, if ok { "" } else { "句柄裸编码" });
    // J1-5 隐式禁用（跨卡自动分发 API 不存在：跨卡句柄校验必拒）
    let foreign = encode_handle(1, 7);
    let ok = g.validate_handle(foreign) == Err(XC_XCARD);
    s.add("AMD-252-05-跨卡隔离", ok, if ok { "" } else { "跨卡渗漏" });
    // J1-6 本卡句柄放行
    let ok = g.validate_handle(h).is_ok();
    s.add("AMD-252-06-本卡放行", ok, if ok { "" } else { "本卡误拦" });
    // J1-7 拓扑呈现（三元组入表 + 渲染含链路名）
    let r = g.render_topo();
    let ok = g.topo.len() == 2 && r.contains("xGMI") && r.contains("PCIe");
    s.add("AMD-252-07-拓扑呈现", ok, if ok { "" } else { "拓扑破缺" });
    // J1-8 拓扑读取失败 → 按独立卡处理
    let mut g2 = MultiGpu::new();
    g2.cards = vec![0, 1];
    g2.topo.push(TopoEntry { card: 0, peer: 1, link: L_XGMI, gbps: 92 });
    g2.probe_fail();
    let ok = g2.topo.is_empty() && g2.select(0, L_NONE, 1).is_ok()
        && g2.render_topo().contains("直连对=0");
    s.add("AMD-252-08-降级独立卡", ok, if ok { "" } else { "降级破缺" });
    // J1-9 卡移除资源回收计数
    let mut g3 = MultiGpu::new();
    g3.cards = vec![0, 1];
    g3.select(0, L_PCIE, 32).ok();
    let _ = g3.submit(1);
    let _ = g3.submit(2);
    g3.handles.push((encode_handle(1, 5), 1));
    let rc = g3.remove_card(1);
    let ok = rc == 1 && g3.reclaimed == 1 && g3.handles.len() == 2;
    s.add("AMD-252-09-移除回收", ok, if ok { "" } else { "回收破缺" });
    // J1-10 回收通知入账
    let ok = g3.notices.iter().any(|n| n.card == 1 && n.text.contains("回收"));
    s.add("AMD-252-10-通知入账", ok, if ok { "" } else { "通知缺失" });
    // J1-11 移除选择卡清选择（防悬挂目标）
    let ok = g3.selected.is_none();
    s.add("AMD-252-11-悬挂清选", ok, if ok { "" } else { "悬挂目标" });
    // J1-12 场景核 5/5
    let (a, n) = scene_core();
    let ok = a == n;
    let d = if ok { String::new() } else { format!("场景 {a}/{n}") };
    s.add_dyn("AMD-252-12-场景核", ok, &d);
    // J1-13 码位互异 + 段对齐
    let ok = XC_NO_TARGET != XC_OOB && XC_OOB != XC_XCARD && SEG_BASE % 0x10 == 0;
    s.add("AMD-252-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb252_checks_all_green() {
        let set = run_veb252_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb252_isolation_full_cycle() {
        // 全周期：选卡→提交→跨卡拒→移除→回收→清选
        let mut g = MultiGpu::new();
        g.cards = vec![0, 1];
        g.select(0, L_XGMI, 92).ok();
        let h = g.submit(3).ok().unwrap();
        assert_eq!(handle_card(h), 0);
        assert!(g.validate_handle(encode_handle(1, 3)).is_err());
        g.remove_card(0);
        assert!(g.selected.is_none());
    }
}
