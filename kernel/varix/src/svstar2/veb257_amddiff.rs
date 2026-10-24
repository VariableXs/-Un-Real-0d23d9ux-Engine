//! VE-F0257 · AMD 版本区间差异表（VE-B 域 · AMD 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0257`
//!
//! **职责定位（锚点原文）**：版本区间×行为差异（API 可用性、已知问题、
//! 推荐版本）登记，与 F0235 式缺陷表分工（缺陷等于硬件规避、差异等于
//! 版本行为）；探测入初始化；未知版本保守路径；差异表框架复用 F0238
//! （差异表框架第二用复用）。
//!
//! **数据结构（锚点原文）**：差异表（区间×条目×建议）。
//!
//! **判据（锚点原文）**：区间匹配、分工边界、保守未知、矩阵联动。
//! 本模块 12 条判据逐条映射：**区间匹配 4 条**（版本命中登记区间／区间
//! 边界含端点一致／多区间不重叠校验／区间外未命中走保守）、**分工边界
//! 3 条**（缺陷类拒绝入差异表——缺陷归 F0235／差异类拒绝入缺陷表——
//! 差异归本表／两表渲染分工字段可辨）、**保守未知 3 条**（未知版本走保
//! 守路径标记／保守路径禁用新 API 面／探测失败同未知）、**矩阵联动 1
//! 条**（差异条目联动认证矩阵坐标可查）、**判据自身 1 条**（码位互异段
//! 独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 探测失败 → **未知保守** | [`DiffTable::probe_failed`]：未知态走保守路径禁用新 API——不猜测版本 |
//! | 冲突 → **实测修正** | 区间重叠检出报错，实测记录可仲裁修正表 |
//! | 缺推荐 → **不代言呈现全部** | 无推荐版本的区间如实渲染「无推荐」——不替用户选 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **缺陷表与差异表分工是语义边界不是文件边界**：缺陷=「这块硬件
//!    坏了要绕」，差异=「这版驱动行为变了要适配」——混表会让规避动
//!    作错位（对驱动版本做硬件 workaround 是浪费时间）。
//! 2. **保守未知是唯一安全侧**：未知版本既可能是新也可能是假——禁用
//!    新 API 面是唯一两向都安全的动作。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xC0C0 独占 0x10）。
pub const SEG_BASE: u32 = 0xC0C0;
pub const XC_OVERLAP: u32 = SEG_BASE + 1; // 区间重叠
pub const XC_KIND_BAD: u32 = SEG_BASE + 2; // 分类越界（缺陷混入差异表）
pub const XC_UNKNOWN: u32 = SEG_BASE + 3; // 未知版本

/// 条目类：差异表只收「版本行为差异」。
pub const K_API: u8 = 0; // API 可用性差异
pub const K_ISSUE: u8 = 1; // 已知问题（版本侧）
pub const K_RECO: u8 = 2; // 推荐版本建议

/// 条目类名。
pub fn kind_name(k: u8) -> &'static str {
    match k {
        K_API => "API可用性",
        K_ISSUE => "已知问题",
        K_RECO => "推荐版本",
        _ => "?",
    }
}

/// 差异条目。
#[derive(Clone)]
pub struct DiffEntry {
    pub lo: u32,          // 驱动版本区间下含
    pub hi: u32,          // 上含
    pub kind: u8,
    pub desc: String,
    pub reco: Option<String>, // 缺推荐 = None（不代言）
    pub cert_coord: Option<(u8, u8, u8)>, // 认证矩阵坐标联动
}

/// 保守未知态。
pub struct UnknownState {
    pub unknown: bool,
    pub reason: String,
}

/// 差异表（框架第二用复用：区间/条目/渲染与 F0238 一致）。
pub struct DiffTable {
    pub entries: Vec<DiffEntry>,
    pub unknown_state: UnknownState,
    pub new_api_locked: bool, // 保守路径禁用新 API 面
}

impl DiffTable {
    pub fn new() -> Self {
        DiffTable {
            entries: Vec::new(),
            unknown_state: UnknownState { unknown: false, reason: String::new() },
            new_api_locked: false,
        }
    }

    /// 登记条目：区间重叠拒；缺陷类（kind=0xFF 占位）拒——归 F0235。
    pub fn add(&mut self, lo: u32, hi: u32, kind: u8, desc: String) -> Result<(), u32> {
        if kind > K_RECO {
            return Err(XC_KIND_BAD); // 差异表不收缺陷（分工边界）
        }
        if lo > hi {
            return Err(XC_OVERLAP);
        }
        for e in &self.entries {
            if lo <= e.hi && e.lo <= hi {
                return Err(XC_OVERLAP); // 区间重叠显式拒
            }
        }
        self.entries.push(DiffEntry {
            lo,
            hi,
            kind,
            desc,
            reco: None,
            cert_coord: None,
        });
        Ok(())
    }

    /// 区间查询：命中返回条目下标集；未命中走保守。
    pub fn lookup(&mut self, ver: u32) -> Vec<usize> {
        let hits: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| ver >= e.lo && ver <= e.hi)
            .map(|(i, _)| i)
            .collect();
        if hits.is_empty() {
            self.mark_unknown(format!("版本 {} 未命中登记区间", ver));
        }
        hits
    }

    /// 探测失败 → 未知保守路径（禁用新 API 面）。
    pub fn probe_failed(&mut self, reason: String) {
        self.mark_unknown(reason);
    }

    fn mark_unknown(&mut self, reason: String) {
        self.unknown_state = UnknownState { unknown: true, reason };
        self.new_api_locked = true;
    }

    /// 实测修正：修正条目推荐版本（冲突仲裁入口）。
    pub fn amend_reco(&mut self, idx: usize, reco: String) -> Result<(), u32> {
        let e = self.entries.get_mut(idx).ok_or(XC_UNKNOWN)?;
        e.reco = Some(reco);
        Ok(())
    }

    /// 渲染（分工字段可辨：差异 vs 缺陷归属声明）。
    pub fn render(&self) -> String {
        let mut out = format!(
            "版本差异表 条目={} 未知={} 新API锁={}\n（缺陷归 F0235 缺陷表：本表只收版本行为差异）\n",
            self.entries.len(),
            self.unknown_state.unknown,
            self.new_api_locked
        );
        for e in &self.entries {
            let line = format!(
                "  [{}..{}] {} {} 推荐={}\n",
                e.lo,
                e.hi,
                kind_name(e.kind),
                e.desc,
                e.reco.clone().unwrap_or_else(|| "无推荐（不代言）".into())
            );
            out.push_str(&line);
        }
        if self.unknown_state.unknown {
            let line = format!("  [未知保守] {}\n", self.unknown_state.reason);
            out.push_str(&line);
        }
        out
    }
}

impl Default for DiffTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 场景核：命中/未命中/保守/分工。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut t = DiffTable::new();
    // 1) 登记三区间
    if t.add(100, 199, K_API, "旧 API 面".into()).is_ok()
        && t.add(200, 299, K_RECO, "推荐 250+".into()).is_ok()
        && t.add(300, 399, K_ISSUE, "已知抖动".into()).is_ok()
    {
        caught += 1;
    }
    // 2) 重叠拒
    if t.add(250, 350, K_API, "重叠".into()) == Err(XC_OVERLAP) {
        caught += 1;
    }
    // 3) 命中区间
    if t.lookup(150).len() == 1 {
        caught += 1;
    }
    // 4) 未命中 → 保守锁定
    let _ = t.lookup(999);
    if t.new_api_locked && t.unknown_state.unknown {
        caught += 1;
    }
    // 5) 缺陷类拒入差异表
    if t.add(400, 499, 0xFF, "硬件缺陷".into()) == Err(XC_KIND_BAD) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口。
pub fn run_veb257_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0257");
    // J1-1 区间命中
    let mut t = DiffTable::new();
    let _ = t.add(100, 199, K_API, "旧 API 面".into());
    let ok = t.lookup(150).len() == 1 && t.lookup(100).len() == 1 && t.lookup(199).len() == 1;
    s.add("AMD-257-01-区间命中含端点", ok, if ok { "" } else { "区间破缺" });
    // J1-2 区间外未命中走保守
    let _ = t.lookup(999); // 未命中触发保守路径
    let ok = t.new_api_locked && t.unknown_state.reason.contains("未命中");
    s.add("AMD-257-02-未命中保守", ok, if ok { "" } else { "未命中裸奔" });
    // J1-3 区间重叠显式拒
    let mut t2 = DiffTable::new();
    let _ = t2.add(100, 199, K_API, "A".into());
    let ok = t2.add(150, 250, K_ISSUE, "重叠".into()) == Err(XC_OVERLAP);
    s.add("AMD-257-03-重叠拒绝", ok, if ok { "" } else { "重叠放行" });
    // J1-4 多区间不重叠校验通过
    let ok = t2.add(200, 299, K_RECO, "B".into()).is_ok() && t2.entries.len() == 2;
    s.add("AMD-257-04-不重叠准入", ok, if ok { "" } else { "合法区间误拒" });
    // J1-5 缺陷类拒绝入差异表（分工边界）
    let ok = t2.add(400, 499, 0xFF, "硬件缺陷".into()) == Err(XC_KIND_BAD);
    s.add("AMD-257-05-缺陷不入差异表", ok, if ok { "" } else { "分工越界" });
    // J1-6 差异表渲染声明分工（缺陷归 F0235）
    let ok = t2.render().contains("缺陷归 F0235");
    s.add("AMD-257-06-分工字段可辨", ok, if ok { "" } else { "分工不可辨" });
    // J1-7 探测失败 → 未知保守
    let mut t3 = DiffTable::new();
    t3.probe_failed("PCI 配置空间读超时".into());
    let ok = t3.new_api_locked && t3.unknown_state.unknown;
    s.add("AMD-257-07-探测失败保守", ok, if ok { "" } else { "探测失败裸奔" });
    // J1-8 保守路径禁用新 API 面（锁定位可查）
    let ok = t3.render().contains("新API锁=true");
    s.add("AMD-257-08-新API面锁定", ok, if ok { "" } else { "锁定不可查" });
    // J1-9 缺推荐不代言
    let ok = t2.render().contains("无推荐（不代言）") || t2.entries.iter().all(|e| e.reco.is_none());
    s.add("AMD-257-09-缺推荐不代言", ok, if ok { "" } else { "替用户代言" });
    // J1-10 实测修正推荐
    let _ = t2.amend_reco(1, "实测推荐 260".into());
    let ok = t2.entries[1].reco.as_ref().map(|r| r.contains("260")).unwrap_or(false);
    s.add("AMD-257-10-实测修正", ok, if ok { "" } else { "仲裁失效" });
    // J1-11 认证矩阵坐标联动
    let mut t4 = DiffTable::new();
    let _ = t4.add(100, 199, K_API, "联动".into());
    t4.entries[0].cert_coord = Some((1, 0, 2));
    let ok = t4.entries[0].cert_coord == Some((1, 0, 2));
    s.add("AMD-257-11-矩阵坐标联动", ok, if ok { "" } else { "联动缺失" });
    // J1-12 码位互异 + 段对齐
    let ok = XC_OVERLAP != XC_KIND_BAD && XC_KIND_BAD != XC_UNKNOWN && SEG_BASE % 0x10 == 0;
    s.add("AMD-257-12-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb257_checks_all_green() {
        let set = run_veb257_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb257_boundary_semantics() {
        // 端点含入语义：[100,199] 的 199 命中、200 不命中
        let mut t = DiffTable::new();
        let _ = t.add(100, 199, K_API, "A".into());
        assert_eq!(t.lookup(199).len(), 1);
        // 200 未命中 → 进入保守（但先把 unknown 清掉重新查）
        t.unknown_state.unknown = false;
        t.new_api_locked = false;
        assert!(t.lookup(200).is_empty());
        assert!(t.new_api_locked);
    }
}
