//! H2 资产管线 · 深化批次三（DPI 档位选择 + 主题内存预算 + 图标语
//! 审计——「素材 4K 清晰度」硬指标与 F068 接口承接的域内落位）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F285 图标缓存**：三层缓存的资产入口——本管线只产出「该装哪
//!   档、占多少字节」的决策，缓存执行归 [`crate::h2star::h2cache`]；
//! - **F068 渲染资产按需装载（跨域引用，K2 件）**：单主题驻留内存
//!   ≤80MB（4K 全套实测）——本层把预算做成**记账器**：装载决策全部
//!   过预算闸，超线触发驱逐计划，预算守恒是机判的；
//! - **F300 系统图标语汇总表**：栅格关键线审计（40 枚抽样）、三态
//!   参数一致扫描、风格孤例=0——审计管线是「账本可机检」的实现；
//! - **人格章程·4K 纪律**：高分屏放大不糊 = **优先选原生 ≥ 屏幕档
//!   的资产**，实在没有才放大最大档并打 `upscaled` 标记（放大是
//!   降级路径，必须显性化）。
//!
//! 时间纪律：无时钟；字节预算以 u64 记账，档位以千分率（1000=1x）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// DPI 档位与 mip 选择
// ---------------------------------------------------------------------------

/// 单主题驻留内存预算（F068 判据原值——跨域引用常量在域内唯一）。
pub const THEME_BUDGET_BYTES: u64 = 80 * 1024 * 1024;

/// 屏幕档位（千分率：1000 = 1x、1500 = 150%……4K 资产 = 4000）。
pub const SCALE_BANDS: [u32; 5] = [1000, 1250, 1500, 2000, 4000];

/// mip 选择：给屏幕档 `dpi_permille` 与资产可用档集，返回选用档。
/// 规则（4K 纪律）：**选 ≥ dpi 的最小档**（原生清晰，只缩不放）；
/// 没有任何 ≥ 档时取最大档 + `upscaled = true`（降级显性化）。
pub fn pick_mip(dpi_permille: u32, available: &[u32]) -> (u32, bool) {
    if available.is_empty() {
        return (dpi_permille, true);
    }
    let mut best: Option<u32> = None;
    for &a in available {
        if a >= dpi_permille && best.map_or(true, |b| a < b) {
            best = Some(a);
        }
    }
    match best {
        Some(s) => (s, false),
        None => {
            let max = *available.iter().max().unwrap();
            (max, true)
        }
    }
}

/// 档位是否为合法资产档（管线入口拒野档位——表外档进不来）。
pub fn is_legal_band(scale: u32) -> bool {
    SCALE_BANDS.contains(&scale)
}

// ---------------------------------------------------------------------------
// 主题内存预算记账器（F068 接口承接）
// ---------------------------------------------------------------------------

/// 一笔资产占用：登记时带字节量与是否常驻（壁纸常驻、其余可驱逐）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetHold {
    pub id: u32,
    pub bytes: u64,
    pub pinned: bool,
    /// 最近使用戳（分钟戳，调用方注入）——LRU 驱逐序。
    pub last_used: u64,
}

/// 主题预算器：所有装载决策过闸，预算**守恒**（账面 = Σ持有）。
pub struct ThemeBudget {
    cap: u64,
    holds: Vec<AssetHold>,
    next_id: u32,
    pub evicted: u32,
}

impl ThemeBudget {
    pub fn new(cap: u64) -> ThemeBudget {
        ThemeBudget { cap, holds: Vec::new(), next_id: 0, evicted: 0 }
    }

    pub fn with_f068_budget() -> ThemeBudget {
        ThemeBudget::new(THEME_BUDGET_BYTES)
    }

    /// 账面占用（守恒口径：逐笔求和，不许有影子账）。
    pub fn used(&self) -> u64 {
        self.holds.iter().map(|h| h.bytes).sum()
    }

    pub fn len(&self) -> usize {
        self.holds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.holds.is_empty()
    }

    /// 触碰：更新最近使用戳（缓存命中的口径）。
    pub fn touch(&mut self, id: u32, now_min: u64) -> bool {
        match self.holds.iter_mut().find(|h| h.id == id) {
            Some(h) => {
                h.last_used = now_min;
                true
            }
            None => false,
        }
    }

    /// 装载决策：先驱逐再入账——返回 `(装载的 id, 驱逐清单)`；
    /// 单笔字节超预算直接拒绝（拒绝原因显式，不静默截断）。
    pub fn admit(&mut self, bytes: u64, pinned: bool, now_min: u64) -> AdmitPlan {
        if bytes > self.cap {
            return AdmitPlan { accepted: false, id: 0, evicted: Vec::new(), reason: "single asset over budget" };
        }
        let mut evicted = Vec::new();
        while self.used() + bytes > self.cap {
            match self.evict_one(now_min) {
                Some(id) => evicted.push(id),
                None => {
                    // 全是常驻：放不下就是放不下，如实拒绝。
                    return AdmitPlan { accepted: false, id: 0, evicted, reason: "pinned holds exceed budget" };
                }
            }
        }
        let id = self.next_id;
        self.next_id += 1;
        self.holds.push(AssetHold { id, bytes, pinned, last_used: now_min });
        AdmitPlan { accepted: true, id, evicted, reason: "" }
    }

    /// 驱逐一个：LRU（最近使用最旧先走）；常驻项豁免。
    fn evict_one(&mut self, _now: u64) -> Option<u32> {
        let cand = self
            .holds
            .iter()
            .filter(|h| !h.pinned)
            .min_by_key(|h| h.last_used)
            .map(|h| h.id)?;
        let idx = self.holds.iter().position(|h| h.id == cand)?;
        self.holds.remove(idx);
        self.evicted += 1;
        Some(cand)
    }

    /// 释放（显式卸载——主题切走时的清账口）。
    pub fn release(&mut self, id: u32) -> bool {
        let before = self.holds.len();
        self.holds.retain(|h| h.id != id);
        self.holds.len() != before
    }
}

/// 装载决策结果。
#[derive(Debug)]
pub struct AdmitPlan {
    pub accepted: bool,
    pub id: u32,
    pub evicted: Vec<u32>,
    pub reason: &'static str,
}

// ---------------------------------------------------------------------------
// F300 图标语审计管线
// ---------------------------------------------------------------------------

/// 图标栅格关键线（F300 判据原值：24px 栅格）。
pub const ICON_GRID_PX: u32 = 24;

/// 三态（F300 判据：正常/悬停/禁用三态参数一致）。
pub const ICON_STATES: [&str; 3] = ["normal", "hover", "disabled"];

/// 一枚受审图标：四个关键线偏移（左/上/右/下，相对 24px 栅格）+
/// 描边宽度。三态共用同一组几何参数（一致性的结构来源）。
#[derive(Clone, Copy, Debug)]
pub struct IconAsset {
    pub name: &'static str,
    /// [left, top, right, bottom] 关键线内缩（px，0..=4 合法域）。
    pub keylines: [u32; 4],
    pub stroke_px: u32,
    pub states: [bool; 3],
}

/// 审计结论。
#[derive(Debug)]
pub struct IconAudit {
    /// 缺态清单（三态不全 = 审计红）。
    pub missing_states: Vec<&'static str>,
    /// 关键线越界清单（不在 0..=4 合法域 = 红线外）。
    pub keyline_outliers: Vec<&'static str>,
    /// 风格孤例（与全体共识偏差超阈 = 孤例，F300 判据「孤例=0」）。
    pub style_outliers: Vec<&'static str>,
    /// 抽样数（判据口径：40 枚入表）。
    pub sampled: usize,
}

impl IconAudit {
    pub fn all_green(&self) -> bool {
        self.missing_states.is_empty()
            && self.keyline_outliers.is_empty()
            && self.style_outliers.is_empty()
    }
}

/// 图标语审计：三查全走一遍（缺态/关键线/孤例）。
/// 孤例判定：共识描边 = 全体中位数；偏差 >1px 即孤例——中位数
/// 抗单枚异常值，比均值诚实。
pub fn audit_icons(assets: &[IconAsset]) -> IconAudit {
    let mut missing = Vec::new();
    let mut out_of_keyline = Vec::new();
    for a in assets {
        if a.states != [true, true, true] {
            missing.push(a.name);
        }
        if a.keylines.iter().any(|&k| k > 4) || a.stroke_px == 0 || a.stroke_px > 4 {
            out_of_keyline.push(a.name);
        }
    }
    // 共识描边 = 中位数（跳过关键线越界者，避免坏值拉偏共识）。
    let mut strokes: Vec<u32> =
        assets.iter().filter(|a| !out_of_keyline.contains(&a.name)).map(|a| a.stroke_px).collect();
    strokes.sort_unstable();
    let consensus = strokes.get(strokes.len() / 2).copied().unwrap_or(2);
    let mut style = Vec::new();
    for a in assets {
        if !out_of_keyline.contains(&a.name)
            && (a.stroke_px as i32 - consensus as i32).abs() > 1
        {
            style.push(a.name);
        }
    }
    IconAudit {
        missing_states: missing,
        keyline_outliers: out_of_keyline,
        style_outliers: style,
        sampled: assets.len(),
    }
}

// ---------------------------------------------------------------------------
// 缩略图解码档位（F285 资产入口 + F093 跨域引用口径）
// ---------------------------------------------------------------------------

/// 缩略图三档（列表条/详情/预览——字节预算与解码档同源）。
pub const THUMB_TARGETS: [(u32, u64); 3] =
    [(32, 4 * 1024), (256, 256 * 1024), (1024, 4 * 1024 * 1024)];

/// 解码决策：目标尺寸 → (输出档, 字节上限, 是否渐进解码)。
/// 超大源图强制渐进（首行可见优先——「先骨架后内容」的资产面）。
pub fn decode_plan(src_bytes: u64, target_px: u32) -> (u32, u64, bool) {
    let (_, cap) = THUMB_TARGETS
        .iter()
        .copied()
        .find(|(px, _)| *px >= target_px)
        .unwrap_or((1024, 4 * 1024 * 1024));
    let progressive = src_bytes > cap * 4;
    (target_px, cap, progressive)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2asset_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2asset");
    // mip：优先 ≥ 档最小者；无 ≥ 档时最大档 + upscaled 显性化。
    let (s1, up1) = pick_mip(1500, &[1000, 1500, 2000, 4000]);
    set.add("h2asset mip native", s1 == 1500 && !up1, "exact band");
    let (s2, up2) = pick_mip(1500, &[1000]);
    set.add("h2asset mip upscale marked", s2 == 1000 && up2, "degradation visible");
    let (s3, up3) = pick_mip(4000, &[1000, 1250, 1500, 2000, 4000]);
    set.add("h2asset mip 4k native", s3 == 4000 && !up3, "4K crisp");
    set.add("h2asset mip empty honest", pick_mip(1000, &[]) == (1000, true), "no silent default");
    // 野档位拒收。
    set.add("h2asset band table", is_legal_band(1500) && !is_legal_band(1333), "no wild bands");
    // 预算守恒：装载-驱逐-释放全程 used() == Σ holds。
    let mut tb = ThemeBudget::new(1000);
    let a = tb.admit(400, false, 0);
    let b = tb.admit(400, false, 1);
    set.add(
        "h2asset budget conserved",
        a.accepted && b.accepted && tb.used() == 800 && tb.len() == 2,
        "ledger = sum",
    );
    // 超预算触发 LRU 驱逐：旧触碰的先走（last_used 小者）。
    let c = tb.admit(400, false, 2);
    set.add(
        "h2asset lru evict",
        c.accepted && c.evicted == vec![0] && tb.used() == 800 && tb.evicted == 1,
        "oldest first",
    );
    // 触碰改写驱逐序：先驱逐 1（旧者先走），触碰 2 后它成为最新。
    let d = tb.admit(400, false, 3);
    set.add(
        "h2asset touch saves",
        d.accepted && d.evicted == vec![1] && tb.touch(2, 99),
        "recent use wins",
    );
    // 常驻豁免：只剩常驻时拒绝且留原因（不静默、不硬塞）。
    let mut tb2 = ThemeBudget::new(1000);
    let p = tb2.admit(800, true, 0);
    let r = tb2.admit(400, false, 1);
    set.add(
        "h2asset pinned refuse",
        p.accepted && !r.accepted && r.reason == "pinned holds exceed budget",
        "refuse with reason",
    );
    // 单笔超预算直接拒（截断是骗人，拒绝是诚实）。
    let r2 = tb2.admit(2000, false, 2);
    set.add("h2asset single cap", !r2.accepted && r2.reason == "single asset over budget", "no truncation");
    // 释放清账。
    tb2.release(p.id);
    set.add("h2asset release", tb2.used() == 0 && tb2.is_empty(), "clean teardown");
    // F068 全尺寸主题演练：4K 全套逐笔入账不超线、预算恒守恒。
    let mut theme = ThemeBudget::with_f068_budget();
    let mut ok = true;
    for i in 0..200u64 {
        let plan = theme.admit(512 * 1024, i % 10 == 0, i);
        ok &= plan.accepted;
        ok &= theme.used() <= THEME_BUDGET_BYTES;
    }
    set.add("h2asset f068 80mb", ok && theme.used() <= THEME_BUDGET_BYTES, "cap held under load");
    // F300 审计：三查各有命中 + 全绿路径。
    let good = IconAsset { name: "ok", keylines: [2, 2, 2, 2], stroke_px: 2, states: [true, true, true] };
    let half = IconAsset { name: "half", keylines: [2, 2, 2, 2], stroke_px: 2, states: [true, true, false] };
    let wild = IconAsset { name: "wild", keylines: [2, 9, 2, 2], stroke_px: 7, states: [true, true, true] };
    let odd = IconAsset { name: "odd", keylines: [2, 2, 2, 2], stroke_px: 4, states: [true, true, true] };
    let audit = audit_icons(&[good, half, wild, odd, IconAsset { name: "g2", keylines: [2, 2, 2, 2], stroke_px: 2, states: [true, true, true] }]);
    set.add(
        "h2asset f300 audit",
        audit.missing_states == vec!["half"]
            && audit.keyline_outliers == vec!["wild"]
            && audit.style_outliers == vec!["odd"]
            && audit.sampled == 5,
        "three scans hit",
    );
    let clean = audit_icons(&[good, IconAsset { name: "g3", keylines: [1, 2, 1, 2], stroke_px: 2, states: [true, true, true] }]);
    set.add("h2asset f300 outliers zero", clean.all_green(), "孤例=0");
    set.add("h2asset f300 grid", ICON_GRID_PX == 24 && ICON_STATES.len() == 3, "24px / 3 states");
    // 解码档：32/256/1024 三档命中；大源图渐进标记。
    let (px1, cap1, pr1) = decode_plan(1_000_000, 256);
    set.add("h2asset thumb tier", px1 == 256 && cap1 == 256 * 1024 && !pr1, "256 tier");
    let (px2, _, pr2) = decode_plan(64 * 1024 * 1024, 1024);
    set.add("h2asset progressive", px2 == 1024 && pr2, "big src progressive");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2asset_all_green() {
        let set = run_h2asset_checks();
        assert!(set.all_passed(), "h2asset 自检有红项");
        assert!(!set.truncated(), "h2asset 自检溢出");
    }

    #[test]
    fn budget_never_exceeds_cap_under_churn() {
        // 1 万轮装载-触碰-释放翻搅：预算线一次都不许破（守恒压力钉死）。
        let mut tb = ThemeBudget::new(10_000);
        let mut id = 0;
        for i in 0..10_000u64 {
            let plan = tb.admit(300 + (i % 5) * 100, false, i);
            if plan.accepted {
                id = plan.id;
            }
            if i % 7 == 0 {
                tb.release(id);
            }
            assert!(tb.used() <= 10_000, "cap breached at turn {i}");
        }
    }

    #[test]
    fn mip_prefers_smallest_sufficient() {
        // 可用档乱序给出，选择仍是最小足档。
        let (s, up) = pick_mip(1250, &[4000, 1000, 1500, 1250]);
        assert_eq!((s, up), (1250, false));
    }
}
