//! VE-F0719 · 栅格化组收口（D06 组 20 项收口）
//!
//! 职责：D06 组 20 项收口（组级范式）——自检入树、质量对拍报告
//!（F0708 Skia 级对拍结果）、性能基准报告、组双签、台账、
//! 向 D07 贝塞尔组移交（栅格化就绪——贝塞尔数学组的成果直接喂栅格化，接口已冻结）。
//! 判据：20 项入树、对拍报告、双签、移交、断点。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 组收口条目。
#[derive(Clone, Debug, PartialEq)]
pub struct CloseItem {
    pub title: String,
    pub in_tree: bool,
    pub judged: bool,
}

/// 质量对拍报告：对拍形状数 + 平均质量分 + 差距待修数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QualityReport {
    pub shapes: u32,
    pub avg_score_permille: u32,
    pub pending_gaps: u32,
}

impl QualityReport {
    /// 报告合格：对拍形状 >= 6（F0708 六类）且平均分 >= 900 且待修差距 == 0。
    pub fn acceptable(&self) -> bool {
        self.shapes >= 6 && self.avg_score_permille >= 900 && self.pending_gaps == 0
    }
}

/// 性能基准报告：基准条目数 + 全绿标志。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerfReport {
    pub benches: u32,
    pub all_green: bool,
}

impl PerfReport {
    pub fn acceptable(&self) -> bool {
        self.benches >= 3 && self.all_green
    }
}

/// 组收口总核验：20 项 + 双报告 + 双签 → 通过；缺项逐一点名。
pub fn group_close(
    items: &[CloseItem],
    quality: &QualityReport,
    perf: &PerfReport,
    dual_sign: bool,
) -> Result<(), String> {
    if items.len() != 20 {
        return Err(format!("收口条目 {} != 20", items.len()));
    }
    let red: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| !i.in_tree || !i.judged)
        .map(|(n, _)| n + 1)
        .collect();
    if !red.is_empty() {
        return Err(format!("未入树或判据未绿：第 {} 项", red.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(",")));
    }
    if !quality.acceptable() {
        return Err("质量对拍报告不合格".to_string());
    }
    if !perf.acceptable() {
        return Err("性能基准报告不合格".to_string());
    }
    if !dual_sign {
        return Err("组双签缺失：收口阻断".to_string());
    }
    Ok(())
}

/// D07 移交：栅格化就绪声明——贝塞尔成果直接喂栅格化（接口冻结）。
#[derive(Clone, Debug, PartialEq)]
pub struct Handover {
    pub raster_ready: bool,
    /// 接口冻结：贝塞尔→栅格化的喂入接口已冻结（F0707 统一接口复用）。
    pub iface_frozen: bool,
    /// 交接用例数。
    pub cases: u32,
}

impl Handover {
    pub fn validate(&self) -> Result<(), String> {
        if !self.raster_ready {
            return Err("栅格化未就绪：移交阻断".to_string());
        }
        if !self.iface_frozen {
            return Err("贝塞尔喂入接口未冻结：移交阻断".to_string());
        }
        if self.cases < 8 {
            return Err(format!("交接用例 {} < 8", self.cases));
        }
        Ok(())
    }
}

/// 断点登记：D06→D07 里程碑锚。
#[derive(Clone, Debug, PartialEq)]
pub struct Breakpoint {
    pub tag: String,
    pub from_group: u32,
    pub to_group: u32,
}

pub fn run_vec319_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec319");
    // 族一：20 项收口
    let items: Vec<CloseItem> = (0..20)
        .map(|i| CloseItem { title: format!("D06-{}", i + 1), in_tree: true, judged: true })
        .collect();
    let q = QualityReport { shapes: 6, avg_score_permille: 930, pending_gaps: 0 };
    let p = PerfReport { benches: 3, all_green: true };
    s.add("全绿收口通过", group_close(&items, &q, &p, true).is_ok(), "close-ok");
    let mut one_red = items.clone();
    one_red[7].in_tree = false;
    match group_close(&one_red, &q, &p, true) {
        Err(m) => s.add("未入树点名", m.contains("第 8 项"), "red-item"),
        Ok(()) => s.add("未入树点名", false, "expected err"),
    }
    let nineteen: Vec<CloseItem> = items[..19].to_vec();
    s.add("条目数不对拒收", group_close(&nineteen, &q, &p, true).is_err(), "count");
    // 族二：双报告
    let q_bad = QualityReport { shapes: 4, avg_score_permille: 930, pending_gaps: 0 };
    s.add("形状不足报告不合格", !q_bad.acceptable(), "few-shapes");
    let q_low = QualityReport { shapes: 6, avg_score_permille: 880, pending_gaps: 0 };
    s.add("均分过低不合格", !q_low.acceptable(), "low-score");
    let q_gap = QualityReport { shapes: 6, avg_score_permille: 930, pending_gaps: 2 };
    s.add("差距未清不合格", !q_gap.acceptable(), "pending-gap");
    let p_bad = PerfReport { benches: 2, all_green: true };
    s.add("基准不足不合格", !p_bad.acceptable(), "few-bench");
    let p_red = PerfReport { benches: 3, all_green: false };
    s.add("基准有红不合格", !p_red.acceptable(), "bench-red");
    s.add("报告不合格阻断收口", group_close(&items, &q_bad, &p, true).is_err(), "block");
    // 族三：双签
    s.add("双签缺失阻断", group_close(&items, &q, &p, false).is_err(), "no-sign");
    // 族四：D07 移交
    let h = Handover { raster_ready: true, iface_frozen: true, cases: 12 };
    s.add("就绪+冻结+用例足移交通过", h.validate().is_ok(), "handover-ok");
    let h1 = Handover { raster_ready: false, iface_frozen: true, cases: 12 };
    s.add("未就绪阻断", h1.validate().is_err(), "not-ready");
    let h2 = Handover { raster_ready: true, iface_frozen: false, cases: 12 };
    s.add("接口未冻结阻断", h2.validate().is_err(), "unfrozen");
    let h3 = Handover { raster_ready: true, iface_frozen: true, cases: 4 };
    s.add("用例不足阻断", h3.validate().is_err(), "thin");
    // 族五：断点与台账
    let bp = Breakpoint { tag: "D06-CLOSE".to_string(), from_group: 6, to_group: 7 };
    s.add("断点登记", bp.from_group == 6 && bp.to_group == 7 && bp.tag.contains("D06"), "bp");
    s.add("台账=20项+报告+双签+移交", {
        group_close(&items, &q, &p, true).is_ok() && h.validate().is_ok()
    }, "ledger");
    s
}
