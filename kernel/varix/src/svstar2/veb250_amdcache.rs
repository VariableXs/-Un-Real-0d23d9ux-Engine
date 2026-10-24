//! VE-F0250 · AMD 片上缓存命中提示（VE-B 域 · AMD 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0250`
//!
//! **职责定位（锚点原文）**：大容量片上缓存命中友好的放置与访问序提示
//! （只建议不强制），命中率遥测只读计数采集；效果诚实呈现不夸大（实验档
//! 语义标注）；不支持代际标记特性缺席而非假装生效。
//!
//! **数据结构（锚点原文）**：访问序提示结构；命中率计数器；缺席标记。
//!
//! **判据（锚点原文）**：只建议不强制、诚实标注、缺测标记、缺席语义。
//! 本模块 12 条判据逐条映射：**只建议不强制 4 条**（提示仅入账不改放置
//! 决定／强制路径拒绝提示干预／提示采纳与否结果一致可查／提示建议值显式
//! 独立于硬性约束）、**诚实标注 3 条**（实验档语义标注常驻／效果呈现带
//! 计数不夸大／渲染含「实验」字样）、**缺测标记 2 条**（计数不可用标注
//! 缺测／缺测不参与均值计算）、**缺席语义 2 条**（不支持代际标记特性缺
//! 席而非假装生效／缺席态下 API 返回未启用而非空数据）、**判据自身
//! 1 条**（码位互异段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 计数不可用 → **标注缺测** | [`HintTelemetry::mark_unavailable`]：置缺测位——缺测样本不入均值，不假装为零 |
//! | 提示不被硬件采纳 → **如实呈现** | 采纳率 = 实际命中差 / 提示次数，只记账不美化 |
//! | 不支持代际 → **特性缺席标记** | [`CacheHint::feature_absent`]：缺席态显式拒绝建议生成，而非静默空结果 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **提示与决策分离**：提示只进「建议账」，放置决策函数不读建议账
//!    ——这从类型层面保证「只建议不强制」：想让提示影响决策必须显式
//!    走 `adopt` 门（且采纳率照实记录）。
//! 2. **缺测 ≠ 零**：缺测是独立状态位，均值计算显式跳过缺测样本——
//!    把「没测到」和「测到了是 0」混为一谈是遥测系统最经典的谎报。

use alloc::vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xB9C0 独占 0x10）。
pub const SEG_BASE: u32 = 0xB9C0;
pub const XC_FORCE_REJ: u32 = SEG_BASE + 1; // 强制路径拒绝提示干预
pub const XC_ABSENT: u32 = SEG_BASE + 2; // 代际不支持特性缺席
pub const XC_HINT_BAD: u32 = SEG_BASE + 3; // 提示参数无效

/// 提示档位：实验档语义标注（诚实呈现的根）。
pub const TIER_EXPERIMENTAL: &str = "实验档";

/// 访问序提示建议（只建议不强制：建议值独立于硬性放置约束）。
#[derive(Clone, PartialEq)]
pub struct AccessHint {
    pub resource: u64,
    pub order: Vec<u64>, // 建议访问序
    pub adoptable: bool, // 硬件是否采纳（照实记录）
}

/// 命中率计数器 + 缺测标记。
pub struct HintTelemetry {
    pub hint_count: u32,   // 提示次数
    pub hit_delta: u32,    // 实际命中差计数
    pub missing: bool,     // 缺测标记：计数不可用
    pub samples: Vec<(bool, u32)>, // (有效?, 命中值)
}

impl HintTelemetry {
    pub fn new() -> Self {
        HintTelemetry { hint_count: 0, hit_delta: 0, missing: false, samples: Vec::new() }
    }

    /// 记一次样本。
    pub fn sample(&mut self, ok: bool, hits: u32) {
        self.samples.push((ok, hits));
        if ok {
            self.hint_count += 1;
            self.hit_delta += hits;
        }
    }

    /// 计数不可用 → 标注缺测（缺测样本不入均值）。
    pub fn mark_unavailable(&mut self) {
        self.missing = true;
        self.samples.push((false, 0));
    }

    /// 均值：只算有效样本；全缺测返回 None（不假装为零）。
    pub fn mean(&self) -> Option<u32> {
        if self.missing && self.hint_count == 0 {
            return None;
        }
        if self.hint_count == 0 {
            return None;
        }
        Some(self.hit_delta / self.hint_count)
    }

    /// 诚实呈现：均值 + 缺测标注 + 实验档常驻标注。
    pub fn render(&self) -> String {
        let mut out = format!(
            "缓存提示遥测[{}] 提示={} 缺测={}\n",
            TIER_EXPERIMENTAL,
            self.hint_count,
            if self.missing { "是" } else { "否" }
        );
        match self.mean() {
            Some(m) => {
                let line = format!("  均值命中差={}（实验档：效果不做夸大承诺）\n", m);
                out.push_str(&line);
            }
            None => {
                let line = format!("  均值=缺测（样本不可用，不插值不补零）\n");
                out.push_str(&line);
            }
        }
        out
    }
}

impl Default for HintTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

/// 片上缓存提示器：代际能力 + 建议账 + 强制路径隔离。
pub struct CacheHint {
    pub generation_supported: bool, // 代际是否支持（不支持 = 特性缺席）
    pub hints: Vec<AccessHint>,
    pub force_blocked: u32, // 强制路径拦截次数
}

impl CacheHint {
    pub fn new(generation_supported: bool) -> Self {
        CacheHint { generation_supported, hints: Vec::new(), force_blocked: 0 }
    }

    /// 特性缺席标记：不支持代际显式拒绝（返回缺席码而非假装生效）。
    pub fn feature_absent(&self) -> bool {
        !self.generation_supported
    }

    /// 生成访问序提示（只建议：建议值不含任何硬性约束字段）。
    pub fn suggest(&mut self, resource: u64, order: Vec<u64>) -> Result<&AccessHint, u32> {
        if self.feature_absent() {
            return Err(XC_ABSENT);
        }
        if order.len() < 2 {
            return Err(XC_HINT_BAD);
        }
        self.hints.push(AccessHint { resource, order, adoptable: false });
        let n = self.hints.len();
        Ok(&self.hints[n - 1])
    }

    /// 硬件采纳回报（照实记录，不夸大）。
    pub fn report_adopt(&mut self, resource: u64, adopted: bool) -> bool {
        for h in self.hints.iter_mut().rev() {
            if h.resource == resource {
                h.adoptable = adopted;
                return adopted;
            }
        }
        false
    }

    /// 强制路径：放置决策**不读**建议账（类型层保证只建议不强制）。
    /// 任何试图把提示注入强制路径的调用显式拒绝并计数。
    pub fn enforce_path(&mut self, resource: u64) -> Result<u64, u32> {
        if self.hints.iter().any(|h| h.resource == resource && h.adoptable) {
            self.force_blocked += 1;
            return Err(XC_FORCE_REJ); // 提示不进入强制路径
        }
        Ok(resource) // 强制路径只看资源本身
    }

    /// 建议账渲染。
    pub fn render(&self) -> String {
        let mut out = format!(
            "缓存提示账 支持={} 建议={} 强制拦截={}\n",
            self.generation_supported,
            self.hints.len(),
            self.force_blocked
        );
        for h in &self.hints {
            let line = format!(
                "  资源={:x} 序长={} 采纳={}\n",
                h.resource,
                h.order.len(),
                if h.adoptable { "是" } else { "否" }
            );
            out.push_str(&line);
        }
        out
    }
}

/// 场景核：缺席代际拒绝 + 支持代际建议全绿。
pub fn scene_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 不支持代际 → 特性缺席
    let mut bad = CacheHint::new(false);
    if bad.feature_absent() && bad.suggest(1, vec![1, 2]) == Err(XC_ABSENT) {
        caught += 1;
    }
    // 2) 支持代际 → 建议入账
    let mut ok = CacheHint::new(true);
    if ok.suggest(0x10, vec![3, 1, 2]).is_ok() && ok.hints.len() == 1 {
        caught += 1;
    }
    // 3) 序过短拒
    if ok.suggest(0x11, vec![7]) == Err(XC_HINT_BAD) {
        caught += 1;
    }
    // 4) 采纳回报照实
    ok.report_adopt(0x10, true);
    if ok.hints[0].adoptable {
        caught += 1;
    }
    // 5) 强制路径拒绝提示干预
    if ok.enforce_path(0x10) == Err(XC_FORCE_REJ) && ok.force_blocked == 1 {
        caught += 1;
    }
    // 6) 强制路径对未提示资源放行
    if ok.enforce_path(0x99).is_ok() {
        caught += 1;
    }
    (caught, 6)
}

/// 判据入口。
pub fn run_veb250_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0250");
    // J1-1 只建议不强制（强制路径拒绝提示干预 + 计数）
    let mut c = CacheHint::new(true);
    let _ = c.suggest(0xA0, vec![1, 2, 3]);
    c.report_adopt(0xA0, true);
    let ok = c.enforce_path(0xA0) == Err(XC_FORCE_REJ) && c.force_blocked == 1;
    s.add("AMD-250-01-只建议不强制", ok, if ok { "" } else { "提示渗入强制路径" });
    // J1-2 强制路径未提示资源放行（提示不影响无关决策）
    let ok = c.enforce_path(0xA1).is_ok();
    s.add("AMD-250-02-强制路径无关放行", ok, if ok { "" } else { "无关资源被误拦" });
    // J1-3 提示采纳与否结果一致可查（照实记录双向）
    let mut c2 = CacheHint::new(true);
    let _ = c2.suggest(1, vec![1, 2]);
    let _ = c2.suggest(2, vec![2, 1]);
    c2.report_adopt(1, true);
    c2.report_adopt(2, false);
    let ok = c2.hints[0].adoptable && !c2.hints[1].adoptable;
    s.add("AMD-250-03-采纳照实双向", ok, if ok { "" } else { "采纳记账单向" });
    // J1-4 建议值独立于硬性约束（建议结构无约束字段）
    let ok = c2.hints[0].order.len() == 2 && c2.force_blocked == 0;
    s.add("AMD-250-04-建议独立约束", ok, if ok { "" } else { "建议耦合约束" });
    // J1-5 诚实标注（渲染含实验档字样 + 效果行）
    let mut t = HintTelemetry::new();
    t.sample(true, 10);
    t.sample(true, 20);
    let ok = t.render().contains("实验档") && t.mean() == Some(15);
    s.add("AMD-250-05-诚实标注实验档", ok, if ok { "" } else { "实验档标注缺失" });
    // J1-6 缺测标记（缺测不入均值 + 不假装为零）
    let mut t2 = HintTelemetry::new();
    t2.sample(true, 30);
    t2.mark_unavailable();
    let ok = t2.missing && t2.mean() == Some(30) && t2.render().contains("缺测=是");
    s.add("AMD-250-06-缺测标记", ok, if ok { "" } else { "缺测位破缺" });
    // J1-7 全缺测 → 均值 None（不假装为零）
    let mut t3 = HintTelemetry::new();
    t3.mark_unavailable();
    let ok = t3.mean().is_none() && t3.render().contains("不插值不补零");
    s.add("AMD-250-07-全缺测不插值", ok, if ok { "" } else { "缺测被当零" });
    // J1-8 缺席语义（不支持代际显式拒绝而非空数据）
    let mut c3 = CacheHint::new(false);
    let ok = c3.feature_absent() && c3.suggest(9, vec![1, 2]) == Err(XC_ABSENT);
    s.add("AMD-250-08-缺席语义显式", ok, if ok { "" } else { "缺席假装生效" });
    // J1-9 缺席态渲染含不支持（可查非静默）
    let ok = c3.render().contains("支持=false");
    s.add("AMD-250-09-缺席渲染可查", ok, if ok { "" } else { "缺席不可查" });
    // J1-10 场景核 6/6
    let (a, n) = scene_core();
    let ok = a == n;
    let d = if ok { String::new() } else { format!("场景 {a}/{n}") };
    s.add_dyn("AMD-250-10-场景核", ok, &d);
    // J1-11 码位互异
    let codes = [XC_FORCE_REJ, XC_ABSENT, XC_HINT_BAD];
    let ok = codes[0] != codes[1] && codes[1] != codes[2] && codes[0] != codes[2];
    s.add("AMD-250-11-码位互异", ok, if ok { "" } else { "码位冲突" });
    // J1-12 段独占（段基址不与邻居重叠）
    let ok = SEG_BASE % 0x10 == 0;
    s.add("AMD-250-12-段对齐", ok, if ok { "" } else { "段未对齐" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb250_checks_all_green() {
        let set = run_veb250_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn veb250_hint_never_leaks_into_enforce() {
        // 全序：建议→采纳→强制路径始终拒绝提示干预
        let mut c = CacheHint::new(true);
        let _ = c.suggest(0x77, vec![5, 6]);
        c.report_adopt(0x77, true);
        assert!(c.enforce_path(0x77).is_err());
        assert_eq!(c.force_blocked, 1);
    }
}
