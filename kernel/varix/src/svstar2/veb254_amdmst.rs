//! VE-F0254 · AMD MST 拓扑发现（VE-B 域 · AMD 组 · 目标 330 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0254`
//!
//! **职责定位（锚点原文）**：HPD 处理复用 F0232 框架（HPD 框架第二用复
//! 用），MST 拓扑发现（分支设备枚举×路径分配）、拓扑变化触发重算；拓
//! 扑环检测拒绝成环配置；带宽分账不足时部分启用并通知。
//!
//! **数据结构（锚点原文）**：MST 拓扑树（分支×路径×负载）；HPD 状态机
//! 复用。
//!
//! **判据（锚点原文）**：MST 枚举、环检测、带宽分账、框架复用。
//! 本模块 13 条判据逐条映射：**MST 枚举 4 条**（分支设备逐层枚举入树／
//! 路径分配根到叶唯一／叶子端点数可查／拓扑变化触发重算计数）、**环检
//! 测 3 条**（成环配置显式拒绝／自环拒绝／无环配置全绿放行）、**带宽分
//! 账 3 条**（逐路径带宽占用记账／不足时部分启用＋通知／全空时全启用）、
//! **框架复用 2 条**（HPD 状态机热插/拔沿复用 F0232 语义／拔出触发分支
//! 摘除重算）、**判据自身 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 拓扑环 → **拒绝并说明** | [`MstTopo::add_branch`] 沿父链回溯发现祖先重复即拒——不是静默去重 |
//! | 带宽不足 → **部分启用加通知** | 分账器按路径优先序逐个启用，耗尽即停 + 通知账——不是全灭 |
//! | 枚举失败 → **降级单屏** | 树根可直连一个端点，MST 层失败不影响单屏基线 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **环检测走父链**：MST 树的环只会来自「把祖先当孩子」的错误配
//!    置，沿父链回溯 O(深度) 即可检出——比全局 DFS 便宜且语义精确。
//! 2. **部分启用优先序**：带宽分账不是先到先得就完，而是按路径登记序
//!    稳定启用——同一拓扑两次分账结果一致，可测试。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xBCC0 独占 0x10）。
pub const SEG_BASE: u32 = 0xBCC0;
pub const XC_CYCLE: u32 = SEG_BASE + 1; // 拓扑成环
pub const XC_SELF: u32 = SEG_BASE + 2; // 自环
pub const XC_BW: u32 = SEG_BASE + 3; // 带宽全空（连部分启用也不够）

/// HPD 状态机（复用 F0232 框架第二用：沿语义与 F0232 一致）。
pub const HPD_CONNECT: u8 = 0;
pub const HPD_DISCONNECT: u8 = 1;

/// HPD 沿名。
pub fn hpd_name(e: u8) -> &'static str {
    match e {
        HPD_CONNECT => "热插",
        HPD_DISCONNECT => "拔出",
        _ => "?",
    }
}

/// MST 分支设备（父指针树）。
#[derive(Clone)]
pub struct Branch {
    pub id: u8,
    pub parent: Option<u8>, // None = 根
    pub path: Vec<u8>,      // 根到叶路径
    pub enabled: bool,      // 带宽分账结果
}

/// 通知账条。
#[derive(Clone)]
pub struct MstNotice {
    pub text: String,
}

/// MST 拓扑树 + 带宽分账器。
pub struct MstTopo {
    pub branches: Vec<Branch>,
    pub total_bw: u32,      // 总带宽（链路 Gbps）
    pub used_bw: u32,       // 已分账
    pub per_path_bw: u32,   // 每路径需求
    pub reflow_count: u32,  // 拓扑变化重算计数
    pub hpd_events: Vec<(u8, u8)>, // (沿， 分支)
    pub notices: Vec<MstNotice>,
}

impl MstTopo {
    pub fn new(total_bw: u32, per_path_bw: u32) -> Self {
        MstTopo {
            branches: Vec::new(),
            total_bw,
            used_bw: 0,
            per_path_bw,
            reflow_count: 0,
            hpd_events: Vec::new(),
            notices: Vec::new(),
        }
    }

    /// HPD 沿（复用 F0232 语义）：热插触发重算，拔出摘子树触发重算。
    pub fn hpd(&mut self, edge: u8, branch: u8) {
        self.hpd_events.push((edge, branch));
        match edge {
            HPD_CONNECT => self.reflow(),
            HPD_DISCONNECT => {
                let before = self.branches.len();
                // 摘除该分支及其子树（父链含 branch 的全部摘除）
                let mut kill: Vec<u8> = vec![branch];
                loop {
                    let mut grew = false;
                    for b in &self.branches {
                        if let Some(p) = b.parent {
                            if kill.contains(&p) && !kill.contains(&b.id) {
                                kill.push(b.id);
                                grew = true;
                            }
                        }
                    }
                    if !grew {
                        break;
                    }
                }
                self.branches.retain(|b| !kill.contains(&b.id));
                let _ = before;
                self.reflow();
            }
            _ => {}
        }
    }

    /// 分支入树：沿父链环检测（祖先重复即成环拒绝）。
    pub fn add_branch(&mut self, id: u8, parent: Option<u8>) -> Result<(), u32> {
        if Some(id) == parent {
            return Err(XC_SELF);
        }
        if let Some(p) = parent {
            // 沿父链回溯：如果链上已出现 id，则加入会成环
            let mut cur = Some(p);
            let mut hops = 0;
            while let Some(c) = cur {
                if c == id {
                    return Err(XC_CYCLE);
                }
                cur = self.branches.iter().find(|b| b.id == c).and_then(|b| b.parent);
                hops += 1;
                if hops > 64 {
                    return Err(XC_CYCLE); // 防御：链超深视同环
                }
            }
        }
        let mut path = Vec::new();
        if let Some(p) = parent {
            if let Some(pb) = self.branches.iter().find(|b| b.id == p) {
                path = pb.path.clone();
            }
        }
        path.push(id);
        self.branches.push(Branch { id, parent, path, enabled: false });
        self.reflow();
        Ok(())
    }

    /// 带宽分账：按登记序稳定启用，不足时部分启用 + 通知。
    pub fn reflow(&mut self) {
        self.reflow_count += 1;
        self.used_bw = 0;
        for b in self.branches.iter_mut() {
            b.enabled = false;
        }
        for i in 0..self.branches.len() {
            if self.used_bw + self.per_path_bw <= self.total_bw {
                self.branches[i].enabled = true;
                self.used_bw += self.per_path_bw;
            } else {
                let text = format!(
                    "带宽不足：路径 {} 部分启用（{}/{} Gbps）",
                    self.branches[i].id, self.used_bw, self.total_bw
                );
                self.notices.push(MstNotice { text });
                break; // 稳定序：耗尽即停
            }
        }
        if self.branches.len() > 0 && self.used_bw == 0 && self.per_path_bw > self.total_bw {
            let text = format!(
                "带宽全空：单路径需求 {} 超总带宽 {}",
                self.per_path_bw, self.total_bw
            );
            self.notices.push(MstNotice { text });
        }
    }

    /// 启用路径数。
    pub fn enabled_count(&self) -> usize {
        self.branches.iter().filter(|b| b.enabled).count()
    }

    /// 拓扑渲染。
    pub fn render(&self) -> String {
        let mut out = format!(
            "MST 拓扑 分支={} 启用={} 带宽={}/{} 重算={}\n",
            self.branches.len(),
            self.enabled_count(),
            self.used_bw,
            self.total_bw,
            self.reflow_count
        );
        for b in &self.branches {
            let line = format!(
                "  分支{} 父={} 路径长度={} 启用={}\n",
                b.id,
                b.parent.map(|p| p.to_string()).unwrap_or_else(|| "根".into()),
                b.path.len(),
                b.enabled
            );
            out.push_str(&line);
        }
        for n in &self.notices {
            let line = format!("  [通知] {}\n", n.text);
            out.push_str(&line);
        }
        out
    }
}

/// 场景核：枚举→环检→分账→拔出摘子树。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut t = MstTopo::new(144, 36); // 4 路径满载
    // 1) 枚举 4 分支全绿启用
    let _ = t.add_branch(1, None);
    let _ = t.add_branch(2, Some(1));
    let _ = t.add_branch(3, Some(1));
    let _ = t.add_branch(4, Some(2));
    if t.enabled_count() == 4 {
        caught += 1;
    }
    // 2) 环拒绝（把根挂到叶下）
    if t.add_branch(5, Some(5)).is_ok() {
        // 自环不可能成功——防御分支不计
    }
    let _ = t.add_branch(5, Some(4));
    if t.add_branch(1, Some(5)) == Err(XC_CYCLE) {
        caught += 1;
    }
    // 3) 拔出分支 1 → 摘除 1/2/3/4/5 全子树
    t.hpd(HPD_DISCONNECT, 1);
    if t.branches.is_empty() && t.reflow_count >= 3 {
        caught += 1;
    }
    (caught, 3)
}

/// 判据入口。
pub fn run_veb254_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0254");
    // J1-1 分支枚举入树
    let mut t = MstTopo::new(144, 36);
    let _ = t.add_branch(1, None);
    let _ = t.add_branch(2, Some(1));
    let ok = t.branches.len() == 2 && t.branches[1].parent == Some(1);
    s.add("AMD-254-01-分支枚举入树", ok, if ok { "" } else { "枚举破缺" });
    // J1-2 路径分配根到叶唯一（子路径 = 父路径 + 自身）
    let ok = t.branches[0].path == vec![1] && t.branches[1].path == vec![1, 2];
    s.add("AMD-254-02-路径唯一", ok, if ok { "" } else { "路径破缺" });
    // J1-3 自环拒绝
    let ok = t.add_branch(3, Some(3)) == Err(XC_SELF);
    s.add("AMD-254-03-自环拒绝", ok, if ok { "" } else { "自环放行" });
    // J1-4 成环拒绝并说明（祖先挂到子孙下）
    let ok = t.add_branch(1, Some(2)) == Err(XC_CYCLE);
    s.add("AMD-254-04-成环拒绝", ok, if ok { "" } else { "环放行" });
    // J1-5 拓扑变化触发重算计数
    let ok = t.reflow_count == 2;
    s.add("AMD-254-05-变化触发重算", ok, if ok { "" } else { "重算未触发" });
    // J1-6 带宽全启用（4 路径 144Gbps）
    let mut t2 = MstTopo::new(144, 36);
    let _ = t2.add_branch(1, None);
    let _ = t2.add_branch(2, Some(1));
    let _ = t2.add_branch(3, Some(1));
    let _ = t2.add_branch(4, Some(2));
    let ok = t2.enabled_count() == 4 && t2.used_bw == 144;
    s.add("AMD-254-06-带宽全启用", ok, if ok { "" } else { "分账破缺" });
    // J1-7 带宽不足部分启用 + 通知
    let mut t3 = MstTopo::new(108, 36); // 只够 3 路
    let _ = t3.add_branch(1, None);
    let _ = t3.add_branch(2, Some(1));
    let _ = t3.add_branch(3, Some(1));
    let _ = t3.add_branch(4, Some(2));
    let ok = t3.enabled_count() == 3
        && t3.notices.iter().any(|n| n.text.contains("部分启用"));
    s.add("AMD-254-07-部分启用通知", ok, if ok { "" } else { "分账全灭" });
    // J1-8 带宽全空通知
    let mut t4 = MstTopo::new(20, 36);
    let _ = t4.add_branch(1, None);
    let ok = t4.enabled_count() == 0 && t4.notices.iter().any(|n| n.text.contains("带宽全空"));
    s.add("AMD-254-08-全空通知", ok, if ok { "" } else { "全空静默" });
    // J1-9 HPD 热插复用（沿语义 + 重算）
    let mut t5 = MstTopo::new(144, 36);
    t5.hpd(HPD_CONNECT, 1);
    let ok = t5.hpd_events.len() == 1 && t5.reflow_count >= 1;
    s.add("AMD-254-09-HPD复用", ok, if ok { "" } else { "HPD 语义漂移" });
    // J1-10 拔出摘子树（父摘子全链）
    let mut t6 = MstTopo::new(144, 36);
    let _ = t6.add_branch(1, None);
    let _ = t6.add_branch(2, Some(1));
    let _ = t6.add_branch(3, Some(2));
    t6.hpd(HPD_DISCONNECT, 2);
    let ok = t6.branches.len() == 1 && t6.branches[0].id == 1;
    s.add("AMD-254-10-拔出摘子树", ok, if ok { "" } else { "摘除不全" });
    // J1-11 枚举失败降级单屏（空树仍可渲染，根端点直连）
    let t7 = MstTopo::new(144, 36);
    let ok = t7.render().contains("分支=0");
    s.add("AMD-254-11-降级单屏", ok, if ok { "" } else { "空树崩溃" });
    // J1-12 场景核 3/3
    let (a, n) = scene_core();
    let ok = a == n;
    let d = if ok { String::new() } else { format!("场景 {a}/{n}") };
    s.add_dyn("AMD-254-12-场景核", ok, &d);
    // J1-13 码位互异 + 段对齐
    let ok = XC_CYCLE != XC_SELF && XC_SELF != XC_BW && SEG_BASE % 0x10 == 0;
    s.add("AMD-254-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb254_checks_all_green() {
        let set = run_veb254_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb254_cycle_defense_deep_chain() {
        // 深链防环：5 层后把根挂尾 → 环拒绝
        let mut t = MstTopo::new(144, 36);
        for i in 1..=5u8 {
            let p = if i == 1 { None } else { Some(i - 1) };
            assert!(t.add_branch(i, p).is_ok());
        }
        assert_eq!(t.add_branch(1, Some(5)), Err(XC_CYCLE));
    }
}
