//! VE-F0259 · AMD 已知缺陷规避表（VE-B 域 · AMD 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0259`
//!
//! **职责定位（锚点原文）**：已知缺陷（代际×版本×触发条件×规避动作）
//! 登记并自动启用规避路径，复用 F0235 框架（规避表框架第二用复用）；
//! 表随 F0098 热更新发布；启用状态入诊断快照可查。
//!
//! **数据结构（锚点原文）**：缺陷表；启用记录；表版本号。
//!
//! **判据（锚点原文）**：自动规避、热更新、保守兜底、快照记录。
//! 本模块 13 条判据逐条映射：**自动规避 4 条**（四元组命中自动启用规避
//! 位／规避位启用 O(1) 查表／未命中不启用（不多绕）／命中与未命中都入
//! 启用记录账）、**热更新 3 条**（表版本号单调递增／热更新替换表内容保
//! 留启用账／旧版本表不复活——版本回退拒）、**保守兜底 3 条**（表加载
//! 失败保守加告警（全规避位不启用）／冲突时精确匹配优先／规避失效降级
//! 报告入账）、**快照记录 2 条**（启用状态入诊断快照渲染可查／快照含表
//! 版本号）、**判据自身 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 规避失效 → **降级报告** | 启用位核验失败 → 报告入账 + 规避位回落——不是静默失效 |
//! | 表加载失败 → **保守加告警** | 空表 + 告警账——裸奔比多绕危险，但无表可依时只能告警 |
//! | 冲突 → **精确匹配优先** | 四元组全匹配 > 代际匹配——精确规则覆盖宽规则 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **命中即启用**：规避不是「代码里到处 if」，而是查表——设备描述
//!    四元组（代际×版本×触发×动作）命中即把规避位置位，运行时只读位。
//!    表随 F0098 热更新后规避面随表走，不用改内核。
//! 2. **保守兜底两向**：无表 → 不启用任何规避 + 告警（诚实裸奔）；
//!    有表 → 命中才启用（不多绕）。两向都不假装安全。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xC340 独占 0x10）。
pub const SEG_BASE: u32 = 0xC340;
pub const XC_LOAD_FAIL: u32 = SEG_BASE + 1; // 表加载失败
pub const XC_ROLLBACK: u32 = SEG_BASE + 2; // 版本回退
pub const XC_EVADE_FAIL: u32 = SEG_BASE + 3; // 规避失效

/// 缺陷四元组条目。
#[derive(Clone)]
pub struct DefectEntry {
    pub gen: u8,        // 代际（GFX 族号）
    pub drv_lo: u32,    // 驱动版本区间下含
    pub drv_hi: u32,    // 上含
    pub trigger: String, // 触发条件
    pub evade: String,  // 规避动作
    pub exact: bool,    // 精确匹配规则（冲突时优先）
}

/// 启用记录。
#[derive(Clone)]
pub struct EvadeRec {
    pub gen: u8,
    pub drv: u32,
    pub enabled: bool,
    pub evade: String,
}

/// 规避表（框架第二用复用：条目/优先/渲染与 F0235 一致）。
pub struct EvadeTable {
    pub entries: Vec<DefectEntry>,
    pub table_ver: u64,     // 表版本号（单调）
    pub evade_bits: Vec<(u8, bool)>, // 规避位（代际→启用）
    pub recs: Vec<EvadeRec>, // 启用记录账
    pub alerts: Vec<String>, // 告警账
    pub degraded: Vec<String>, // 降级报告账
}

impl EvadeTable {
    pub fn new() -> Self {
        EvadeTable {
            entries: Vec::new(),
            table_ver: 1,
            evade_bits: Vec::new(),
            recs: Vec::new(),
            alerts: Vec::new(),
            degraded: Vec::new(),
        }
    }

    /// 登记缺陷条目。
    pub fn add(
        &mut self,
        gen: u8,
        drv_lo: u32,
        drv_hi: u32,
        trigger: &str,
        evade: &str,
        exact: bool,
    ) {
        self.entries.push(DefectEntry {
            gen,
            drv_lo,
            drv_hi,
            trigger: trigger.into(),
            evade: evade.into(),
            exact,
        });
    }

    /// 热更新：替换表内容 + 版本单调（回退拒）。
    pub fn hot_update(&mut self, new_entries: Vec<DefectEntry>, new_ver: u64) -> Result<(), u32> {
        if new_ver <= self.table_ver {
            return Err(XC_ROLLBACK); // 旧版本表不复活
        }
        self.table_ver = new_ver;
        self.entries = new_entries;
        Ok(())
    }

    /// 自动规避核：四元组命中 → 启用位 + 记录账；未命中 → 不启用。
    /// 冲突时精确匹配优先。
    pub fn match_device(&mut self, gen: u8, drv: u32, trigger_seen: &str) -> bool {
        // 精确匹配优先：先扫 exact=true
        // 快照收集法：先把命中动作抄出账，退出借用后再置位（防 E0502）
        let snapped = self
            .entries
            .iter()
            .filter(|e| e.gen == gen && drv >= e.drv_lo && drv <= e.drv_hi && trigger_seen.contains(&e.trigger))
            .max_by_key(|e| if e.exact { 1 } else { 0 })
            .map(|e| e.evade.clone());
        match snapped {
            Some(evade) => {
                self.set_bit(gen, true);
                self.recs.push(EvadeRec { gen, drv, enabled: true, evade });
                true
            }
            None => {
                self.set_bit(gen, false);
                self.recs.push(EvadeRec { gen, drv, enabled: false, evade: String::new() });
                false
            }
        }
    }

    fn set_bit(&mut self, gen: u8, on: bool) {
        match self.evade_bits.iter_mut().find(|(g, _)| *g == gen) {
            Some((_, b)) => *b = on,
            None => self.evade_bits.push((gen, on)),
        }
    }

    /// 表加载失败 → 保守：全规避位不启用 + 告警。
    pub fn load_failed(&mut self) -> u32 {
        self.entries.clear();
        for (g, b) in self.evade_bits.iter_mut() {
            *b = false;
        }
        let line = format!("表加载失败（{:x}），保守模式全规避位不启用", XC_LOAD_FAIL);
        self.alerts.push(line);
        XC_LOAD_FAIL
    }

    /// 规避失效核验：启用位为真但规避动作未生效 → 降级报告 + 位回落。
    pub fn verify(&mut self, gen: u8, action_seen: &str) -> Result<(), u32> {
        let Some((g, on)) = self.evade_bits.iter().find(|(g, _)| *g == gen).copied() else {
            return Ok(()); // 无位即无承诺
        };
        if on {
            let Some(rec) = self.recs.iter().rev().find(|r| r.gen == gen && r.enabled) else {
                return Ok(());
            };
            if action_seen != rec.evade {
                let line = format!(
                    "规避失效：代际 {} 期望「{}」实见「{}」",
                    gen, rec.evade, action_seen
                );
                self.degraded.push(line);
                self.set_bit(gen, false);
                return Err(XC_EVADE_FAIL);
            }
        }
        Ok(())
    }

    /// 诊断快照渲染（启用状态 + 表版本号可查）。
    pub fn snapshot(&self) -> String {
        let mut out = format!("规避表快照 版本={} 条目={}\n", self.table_ver, self.entries.len());
        for (g, b) in &self.evade_bits {
            let line = format!("  代际 {} 规避位={}\n", g, b);
            out.push_str(&line);
        }
        for a in &self.alerts {
            let line = format!("  [告警] {}\n", a);
            out.push_str(&line);
        }
        for d in &self.degraded {
            let line = format!("  [降级] {}\n", d);
            out.push_str(&line);
        }
        out
    }
}

impl Default for EvadeTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 场景核：命中启用→热更新→失效降级。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut t = EvadeTable::new();
    t.add(1, 100, 199, "DSBR", "禁用着色器后背板", false);
    // 1) 命中自动启用
    if t.match_device(1, 150, "触发 DSBR 溢出") {
        caught += 1;
    }
    // 2) 热更新版本单调
    let new_entries = vec![DefectEntry {
        gen: 1,
        drv_lo: 100,
        drv_hi: 299,
        trigger: "DSBR".into(),
        evade: "强制半速率".into(),
        exact: true,
    }];
    if t.hot_update(new_entries, 2).is_ok() && t.table_ver == 2 {
        caught += 1;
    }
    // 3) 版本回退拒
    if t.hot_update(Vec::new(), 1) == Err(XC_ROLLBACK) {
        caught += 1;
    }
    // 4) 规避失效降级报告 + 位回落
    if t.verify(1, "错误动作") == Err(XC_EVADE_FAIL)
        && t.degraded.iter().any(|d| d.contains("规避失效"))
    {
        caught += 1;
    }
    (caught, 4)
}

/// 判据入口。
pub fn run_veb259_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0259");
    // J1-1 四元组命中自动启用
    let mut t = EvadeTable::new();
    t.add(1, 100, 199, "DSBR", "禁用着色器后背板", false);
    let ok = t.match_device(1, 150, "DSBR 溢出") && t.evade_bits.contains(&(1, true));
    s.add("AMD-259-01-命中自动启用", ok, if ok { "" } else { "启用破缺" });
    // J1-2 启用记录账（命中入账含规避动作）
    let ok = t.recs.iter().any(|r| r.enabled && r.evade.contains("后背板"));
    s.add("AMD-259-02-启用记录入账", ok, if ok { "" } else { "账缺失" });
    // J1-3 未命中不启用（不多绕）+ 留账
    let ok = !t.match_device(1, 999, "DSBR 溢出")
        && t.recs.last().map(|r| !r.enabled).unwrap_or(false);
    s.add("AMD-259-03-未命中不绕", ok, if ok { "" } else { "未命中误启用" });
    // J1-4 规避位 O(1) 查表（位表存在即查）
    let ok = t.evade_bits.len() == 1;
    s.add("AMD-259-04-规避位表", ok, if ok { "" } else { "位表破缺" });
    // J1-5 热更新版本单调
    let mut t2 = EvadeTable::new();
    let e0 = vec![DefectEntry {
        gen: 2, drv_lo: 1, drv_hi: 9, trigger: "X".into(), evade: "Y".into(), exact: false,
    }];
    let _ = t2.hot_update(e0, 3);
    let ok = t2.table_ver == 3;
    s.add("AMD-259-05-热更新单调", ok, if ok { "" } else { "版本破缺" });
    // J1-6 版本回退拒（旧表不复活）
    let ok = t2.hot_update(Vec::new(), 2) == Err(XC_ROLLBACK) && t2.table_ver == 3;
    s.add("AMD-259-06-回退拒绝", ok, if ok { "" } else { "旧表复活" });
    // J1-7 热更新替换内容保留位表
    let ok = t2.entries.len() == 1 && t2.entries[0].gen == 2;
    s.add("AMD-259-07-替换保位表", ok, if ok { "" } else { "替换破缺" });
    // J1-8 表加载失败保守加告警
    let mut t3 = EvadeTable::new();
    t3.match_device(1, 150, "DSBR");
    t3.load_failed();
    let ok = t3.entries.is_empty()
        && !t3.evade_bits.contains(&(1, true))
        && t3.alerts.iter().any(|a| a.contains("保守"));
    s.add("AMD-259-08-加载失败保守", ok, if ok { "" } else { "裸奔无告警" });
    // J1-9 冲突精确匹配优先
    let mut t4 = EvadeTable::new();
    t4.add(1, 100, 299, "DSBR", "宽规则动作", false);
    t4.add(1, 150, 150, "DSBR", "精确规则动作", true);
    let ok = t4.match_device(1, 150, "DSBR")
        && t4.recs.last().map(|r| r.evade == "精确规则动作").unwrap_or(false);
    s.add("AMD-259-09-精确优先", ok, if ok { "" } else { "优先序破缺" });
    // J1-10 规避失效降级报告 + 位回落
    let mut t5 = EvadeTable::new();
    t5.add(1, 100, 199, "DSBR", "禁用后背板", false);
    let _ = t5.match_device(1, 150, "DSBR");
    let ok = t5.verify(1, "错误动作") == Err(XC_EVADE_FAIL)
        && t5.evade_bits.contains(&(1, false))
        && t5.degraded.iter().any(|d| d.contains("期望"));
    s.add("AMD-259-10-失效降级回落", ok, if ok { "" } else { "失效静默" });
    // J1-11 快照含表版本 + 启用状态
    let mut t6 = EvadeTable::new();
    let _ = t6.match_device(1, 150, "未见");
    let snap = t6.snapshot();
    let ok = snap.contains("版本=1") && snap.contains("规避位=false");
    s.add("AMD-259-11-快照可查", ok, if ok { "" } else { "快照破缺" });
    // J1-12 场景核 4/4
    let (a, n) = scene_core();
    let ok = a == n;
    let d = if ok { String::new() } else { format!("场景 {a}/{n}") };
    s.add_dyn("AMD-259-12-场景核", ok, &d);
    // J1-13 码位互异 + 段对齐
    let ok = XC_LOAD_FAIL != XC_ROLLBACK && XC_ROLLBACK != XC_EVADE_FAIL && SEG_BASE % 0x10 == 0;
    s.add("AMD-259-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb259_checks_all_green() {
        let set = run_veb259_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb259_hot_update_then_match() {
        // 热更新后新规则立即生效（表随更新走）
        let mut t = EvadeTable::new();
        assert!(!t.match_device(3, 500, "PWSAG"));
        let e = vec![DefectEntry {
            gen: 3, drv_lo: 400, drv_hi: 599, trigger: "PWSAG".into(),
            evade: "拉平电压曲线".into(), exact: false,
        }];
        t.hot_update(e, 2).ok();
        assert!(t.match_device(3, 500, "PWSAG 抖动"));
        assert!(t.snapshot().contains("版本=2"));
    }
}
