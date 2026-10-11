//! VE-F5216 · 特效性能（VE-Z 域 · 性能面 · 锚点目标 300 行 · 深化版）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5216`
//!
//! **判据（锚点原文）**：逐特效价签、语义降级、双层预算、回灌、判据。
//!
//! **职责定位（锚点原文）**：特效性能三件——逐特效性能画像（每个特效的
//! 开销标价——特效也有价签）/特效降级阶梯（帧紧张时特效按语义权重降级
//! ——环境先减战斗后减）/特效性能预算（子系统预算+组合预算双层；三档
//! 预算同 U07 口径）。
//!
//! **数据结构（锚点原文）**：画像器；降级阶梯；双层预算。
//!
//! **错误路径与降级矩阵（锚点原文）**：画像失真→采样校准；降级无序→
//! 语义权重断言；预算失准→实测回灌。
//!
//! **性能逐项分解（锚点原文）**：画像 O(特效数)；降载 O(1)（预排序游标
//! 单步推进）；回灌 O(样本)。
//!
//! **跨批对接点（锚点原文）**：F5137 分级口径参照上游；F5208 组合消费；
//! F5219 联调。
//!
//! **无障碍与隐私（锚点原文）**：价签可见即诚实承诺（域本色核心）；降
//! 级读屏可达（降级单带可读名单——读屏播报"环境粒子已降级"不是黑箱
//! 掉帧）。
//!
//! # 深化说明：特效没有免费午餐——价签可见是诚实承诺
//!
//! 「逐特效性能画像」——每个特效一笔开销账（[`PerfProfiler`] 逐特效累计
//! 采样→均価价签 µs/帧），价签**可见**（查询口返回明细而非总和无明细
//! ——总账会藏胖子）：画像失真→**采样校准**（样本不足三枚即低置信
//! [`Confidence`] 三态——低置信价签显性标注不冒充确定值，校准=继续采样
//! 至置信门槛）；「特效降级阶梯」——帧紧张时按**语义权重**降级（
//! [`SemWeight`] 三档：环境权重最低先减、转场居中、战斗最高最后减——
//! 环境少一朵花玩家不在意，战斗少一记刀光玩家当场举报），降载 O(1)（
//! 阶梯预排序游标单步推进[`DegradeLadder`]），**降级无序→语义权重断言**
//! （降级单权重序单调不减——序乱即断言违例显性立案而非静默乱减）；「特
//! 效性能预算」——**双层**（子系统预算逐特效类+组合预算总量，任一层超
//! 支即超支——子系统内省余量不等于组合安全，两本账都要记），三档
//! [`BudgetVerdict`] Ok/Tight/Over 同 U07 口径（余量 >10% Ok / 0<余量≤10%
//! Tight / 余量≤0 Over）；「预算失准→实测回灌」——基线与实测均值偏差
//! 超阈（20%）即回灌校准（[`Feedback`] 基线=实测均值，O(样本) 一遍求和
//! ——基线是假设实测是事实，事实压过假设）。
//!
//! ## 深化一：降级单是读屏可达的——减了什么要能说出来
//!
//! 降级单（[`DegradePlan`]）带**可读名单**（逐项特效名+语义档），读屏播
//! 报"环境粒子已降级"而非黑箱掉帧——域本色核心「价签可见即诚实承诺」
//! 的降级侧延伸：预算紧了诚实说紧了、减了什么诚实说减了什么。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const PERF_VERSION: &str = "Z16-perf-v1";

/// 采样校准门槛（样本 ≥3 枚价签才到置信档——判据"画像失真→采样校准"）。
pub const CONFIDENCE_SAMPLES: u32 = 3;

/// 回灌触发阈（基线与实测偏差 ppm >200000 即 20% 触发校准）。
pub const FEEDBACK_DRIFT_PPM: u64 = 200_000;

/// 语义权重三档（判据"环境先减战斗后减"——数值小先降级）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum SemWeight {
    /// 环境（花草尘埃——最先减）。
    Ambient,
    /// 转场（过场光效——居中）。
    Transition,
    /// 战斗（刀光弹焰——最后减）。
    Combat,
}

impl SemWeight {
    pub const ALL: [SemWeight; 3] = [SemWeight::Ambient, SemWeight::Transition, SemWeight::Combat];

    pub const fn ordinal(self) -> usize {
        match self {
            SemWeight::Ambient => 0,
            SemWeight::Transition => 1,
            SemWeight::Combat => 2,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            SemWeight::Ambient => "环境",
            SemWeight::Transition => "转场",
            SemWeight::Combat => "战斗",
        }
    }
}

/// 置信三态（判据"画像失真→采样校准"——低置信显性标注）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confidence {
    /// 零样本（无价签）。
    Zero,
    /// 低置信（样本 <3——价签待校准）。
    Low,
    /// 置信（样本 ≥3——价签可用）。
    Confident,
}

/// 诊断码（Z 域字符串码五码互异——vez 系列惯例）。
pub const E_PF_NOFX: &str = "E_PF_NOFX";
pub const E_PF_UNORDERED: &str = "E_PF_UNORDERED";
pub const E_PF_OVER: &str = "E_PF_OVER";
pub const E_PF_NOCAL: &str = "E_PF_NOCAL";
pub const E_PF_BADSAMPLE: &str = "E_PF_BADSAMPLE";

/// 诊断详情（五码闭集逐码中文说明）。
pub fn reason_of(code: &str) -> &'static str {
    match code {
        E_PF_NOFX => "价签查询无此特效：画像账零样本，价签为零不是缺省估计",
        E_PF_UNORDERED => "降级单语义权重序违例：降级必须环境先战斗后，序乱即立案",
        E_PF_OVER => "预算超支：双层任一层余量耗尽，显性超支不静默",
        E_PF_NOCAL => "实测样本非法（零样本或含零开销行）：回灌拒绝",
        E_PF_BADSAMPLE => "画像采样非法：单帧开销为 0 的样本不可入账",
        _ => "未知性能诊断码",
    }
}

/// 单特效价签（判据"逐特效价签"——开销标价+置信度）。
#[derive(Clone, Copy, Debug)]
pub struct PriceTag {
    /// 特效名。
    pub name: &'static str,
    /// 语义权重（降级阶梯排序键）。
    pub weight: SemWeight,
    /// 价签：平均开销 µs/帧（零样本=0）。
    pub avg_us: u64,
    /// 样本数。
    pub samples: u32,
}

impl PriceTag {
    /// 价签置信（采样校准口径）。
    pub const fn confidence(&self) -> Confidence {
        if self.samples == 0 {
            Confidence::Zero
        } else if self.samples < CONFIDENCE_SAMPLES {
            Confidence::Low
        } else {
            Confidence::Confident
        }
    }
}

/// 画像器（判据"逐特效性能画像"——O(特效数) 全量导出）。
pub struct PerfProfiler {
    /// 逐特效累计（名, 总开销 µs, 样本数, 权重）。
    books: Vec<(&'static str, u64, u32, SemWeight)>,
}

impl PerfProfiler {
    pub fn new() -> Self {
        PerfProfiler { books: Vec::new() }
    }

    /// 采样入账（O(1) 摊销；单帧开销 0 拒——E_PF_BADSAMPLE）。
    pub fn sample(&mut self, name: &'static str, weight: SemWeight, cost_us: u64) -> Result<(), &'static str> {
        if cost_us == 0 {
            return Err(E_PF_BADSAMPLE);
        }
        for b in self.books.iter_mut() {
            if b.0 == name {
                b.1 += cost_us;
                b.2 += 1;
                return Ok(());
            }
        }
        self.books.push((name, cost_us, 1, weight));
        Ok(())
    }

    /// 价签查询（O(1) 定位需扫描——账按特效数 O(n)，n 特效闭集小）。
    pub fn tag(&self, name: &str) -> PriceTag {
        for b in &self.books {
            if b.0 == name {
                return PriceTag { name: b.0, weight: b.3, avg_us: b.1 / b.2 as u64, samples: b.2 };
            }
        }
        PriceTag { name: "", weight: SemWeight::Ambient, avg_us: 0, samples: 0 }
    }

    /// 全量价签（画像 O(特效数)——降级阶梯的输入面）。
    pub fn all_tags(&self) -> Vec<PriceTag> {
        self.books
            .iter()
            .map(|b| PriceTag { name: b.0, weight: b.3, avg_us: b.1 / b.2 as u64, samples: b.2 })
            .collect()
    }

    /// 账面特效数。
    pub fn len(&self) -> usize {
        self.books.len()
    }

    pub fn is_empty(&self) -> bool {
        self.books.is_empty()
    }
}

/// 阶梯候选行（预排序输入——判据"降载 O(1) 游标"的排序前提）。
#[derive(Clone, Copy, Debug)]
pub struct LadderRow {
    pub tag: PriceTag,
}

/// 降级单（判据"语义降级"输出——读屏可达的可读名单）。
#[derive(Clone, Debug)]
pub struct DegradePlan {
    /// 被降级特效（按降级顺序）。
    pub dropped: Vec<PriceTag>,
    /// 降级后总开销 µs/帧。
    pub remaining_us: u64,
}

impl DegradePlan {
    /// 语义权重断言（判据"降级无序→语义权重断言"）：降级序权重单调
    /// 不减——环境先战斗后；违例即 E_PF_UNORDERED。
    pub fn assert_ordered(&self) -> Result<(), &'static str> {
        let mut last = 0usize;
        for t in &self.dropped {
            let o = t.weight.ordinal();
            if o < last {
                return Err(E_PF_UNORDERED);
            }
            last = o;
        }
        Ok(())
    }

    /// 读屏可读名单（判据"降级读屏可达"——减了什么能说出来）。
    pub fn spoken(&self) -> String {
        let mut s = String::from("已降级：");
        for (i, t) in self.dropped.iter().enumerate() {
            if i > 0 {
                s.push_str("、");
            }
            s.push_str(t.weight.zh());
            s.push_str(t.name);
        }
        s
    }
}

/// 降级阶梯（判据"特效降级阶梯"本体）。
pub struct DegradeLadder;

impl DegradeLadder {
    /// 构造预排序阶梯（权重升序——环境先战斗后 + 同权重价签降序——先减
    /// 收益最大的）。排序本身 O(n log n) 一次性；此后单步降载 O(1) 游标
    /// 推进（锚点口径）。
    pub fn build(tags: &[PriceTag]) -> Vec<LadderRow> {
        let mut rows: Vec<LadderRow> = tags.iter().map(|t| LadderRow { tag: *t }).collect();
        rows.sort_by(|a, b| {
            a.tag.weight.ordinal().cmp(&b.tag.weight.ordinal())
                .then(b.tag.avg_us.cmp(&a.tag.avg_us))
        });
        rows
    }

    /// 降级决策（锚点"帧紧张时特效按语义权重降级"）：阶梯序逐个降，
    /// 直到总开销 ≤ 预算或阶梯耗尽；全保留为 cur ≤ budget 的平凡返回。
    pub fn degrade(rows: &[LadderRow], cur_us: u64, budget_us: u64) -> DegradePlan {
        let mut dropped = Vec::new();
        let mut remaining = cur_us;
        for r in rows {
            if remaining <= budget_us {
                break;
            }
            remaining = remaining.saturating_sub(r.tag.avg_us);
            dropped.push(r.tag);
        }
        DegradePlan { dropped, remaining_us: remaining }
    }
}

/// 预算三档（判据"三档预算同 U07 口径"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetVerdict {
    /// 余量 >10%。
    Ok,
    /// 0 < 余量 ≤10%。
    Tight,
    /// 余量 ≤0（超支）。
    Over,
}

/// 双层预算（判据"子系统预算+组合预算双层"）。
pub struct DualBudget {
    /// 子系统预算（类名, 预算 µs, 已用 µs）。
    subs: Vec<(&'static str, u64, u64)>,
    /// 组合预算 µs / 已用 µs。
    combo_budget_us: u64,
    combo_used_us: u64,
}

impl DualBudget {
    /// 构造：子系统闭集 + 组合总预算（组合预算独立声明，不等于子系统和
    /// ——双层是两本账不是一本账的两列）。
    pub fn new(subs: Vec<(&'static str, u64)>, combo_budget_us: u64) -> Self {
        DualBudget {
            subs: subs.into_iter().map(|(n, b)| (n, b, 0)).collect(),
            combo_budget_us,
            combo_used_us: 0,
        }
    }

    /// 记账（子层+组层同时记——判据"组合消费"对拍 F5208 语义）。
    pub fn commit(&mut self, sub: &str, cost_us: u64) -> Result<(), &'static str> {
        let mut found = false;
        for b in self.subs.iter_mut() {
            if b.0 == sub {
                b.2 += cost_us;
                found = true;
            }
        }
        if !found {
            return Err(E_PF_NOFX);
        }
        self.combo_used_us += cost_us;
        Ok(())
    }

    /// 单层三档判定（used ≥ budget 即 Over 短路——margin 不做负数减法；
    /// budget=0 恒 Over）。
    pub fn verdict_of(budget_us: u64, used_us: u64) -> BudgetVerdict {
        if budget_us == 0 || used_us >= budget_us {
            return BudgetVerdict::Over;
        }
        let margin_ppm = (budget_us - used_us) * 1_000_000 / budget_us;
        if margin_ppm > 100_000 {
            BudgetVerdict::Ok
        } else {
            BudgetVerdict::Tight
        }
    }

    /// 双层联合判定（判据"任一层超支即超支"——最差档胜出）。
    pub fn verdict(&self, sub: &str) -> BudgetVerdict {
        let mut worst = self.combo_verdict();
        for b in &self.subs {
            if b.0 == sub {
                let v = Self::verdict_of(b.1, b.2);
                if (v as usize) > (worst as usize) {
                    worst = v;
                }
            }
        }
        worst
    }

    /// 组合层判定。
    pub fn combo_verdict(&self) -> BudgetVerdict {
        Self::verdict_of(self.combo_budget_us, self.combo_used_us)
    }

    /// 组合已用（F5208 组合消费对拍口）。
    pub fn combo_used_us(&self) -> u64 {
        self.combo_used_us
    }
}

/// 实测回灌（判据"预算失准→实测回灌"——O(样本) 一遍求和）。
pub struct Feedback;

impl Feedback {
    /// 回灌判定：基线与实测均值偏差 ppm > 阈即校准（新基线=实测均值）。
    /// 返回（是否回灌, 新基线 µs）。零样本/含零行拒绝（E_PF_NOCAL）。
    pub fn calibrate(baseline_us: u64, observed: &[u64]) -> Result<(bool, u64), &'static str> {
        if observed.is_empty() {
            return Err(E_PF_NOCAL);
        }
        let mut sum = 0u64;
        for &o in observed {
            if o == 0 {
                return Err(E_PF_NOCAL);
            }
            sum += o;
        }
        let avg = sum / observed.len() as u64;
        let drift_ppm = if avg >= baseline_us {
            (avg - baseline_us) * 1_000_000 / baseline_us.max(1)
        } else {
            (baseline_us - avg) * 1_000_000 / avg.max(1)
        };
        if drift_ppm > FEEDBACK_DRIFT_PPM {
            Ok((true, avg))
        } else {
            Ok((false, baseline_us))
        }
    }
}

/// 判据主入口（注册于 svstar2/mod.rs blocks 数组，tag VE-F5216）。
pub fn run_vez16_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5216");

    // --- 价签（判据一）---
    // 价-01：逐特效价签手算——sparks 战斗三样本 [200,300,400] avg=300 置信；
    // bloom 环境两样本 [100,100] avg=100 低置信（<3 采样校准门槛）；
    // 零样本特效价签为 0 且 Confidence::Zero（不冒充估计）。
    let mut pf = PerfProfiler::new();
    let _ = pf.sample("sparks", SemWeight::Combat, 200);
    let _ = pf.sample("sparks", SemWeight::Combat, 300);
    let _ = pf.sample("sparks", SemWeight::Combat, 400);
    let _ = pf.sample("bloom", SemWeight::Ambient, 100);
    let _ = pf.sample("bloom", SemWeight::Ambient, 100);
    let t1 = pf.tag("sparks");
    let t2 = pf.tag("bloom");
    let t3 = pf.tag("ghost");
    let price_ok = t1.avg_us == 300
        && t1.samples == 3
        && t1.confidence() == Confidence::Confident
        && t1.weight == SemWeight::Combat
        && t2.avg_us == 100
        && t2.confidence() == Confidence::Low
        && t3.samples == 0
        && t3.avg_us == 0
        && t3.confidence() == Confidence::Zero;
    if price_ok {
        s.ok("EZ16-价-01-逐特效价签与置信");
    } else {
        s.fail("EZ16-价-01-逐特效价签与置信", "价签失效");
    }

    // --- 价签（判据二）---
    // 价-02：采样校准防线——单帧开销 0 拒 E_PF_BADSAMPLE（零开销样本是
    // 测量事故不是快特效）；画像 O(特效数) 全量导出账面 2 特效。
    let r0 = pf.sample("badfx", SemWeight::Ambient, 0);
    let zero_rejected = matches!(r0, Err(E_PF_BADSAMPLE));
    let all = pf.all_tags();
    let export_ok = all.len() == 2 && pf.len() == 2 && !pf.is_empty();
    if zero_rejected && export_ok {
        s.ok("EZ16-价-02-采样防线与全量导出");
    } else {
        s.fail("EZ16-价-02-采样防线与全量导出", "画像防线失效");
    }

    // --- 语义降级（判据三）---
    // 级-01：语义权重闭集三档 ordinal 0..=2 连续互异、中文名互异
    // （环境/转场/战斗——降级序语义锚）。
    let w = SemWeight::ALL;
    let ord_ok = w[0].ordinal() == 0 && w[1].ordinal() == 1 && w[2].ordinal() == 2;
    let names_ok = w[0].zh() != w[1].zh() && w[0].zh() != w[2].zh() && w[1].zh() != w[2].zh();
    if ord_ok && names_ok && w.len() == 3 {
        s.ok("EZ16-级-01-语义权重三档闭集");
    } else {
        s.fail("EZ16-级-01-语义权重三档闭集", "权重闭集失效");
    }

    // --- 语义降级（判据四）---
    // 级-02：降级阶梯手算——候选 bloom(环境,100)/dust(环境,80)/
    // flash(转场,60)/sparks(战斗,300)，cur=540 budget=400 超 140：
    // 阶梯序 bloom→dust→flash→sparks；降 bloom 余 440>400 继续、
    // 降 dust 余 360≤400 停——降级单 [bloom,dust] 环境先减战斗保留。
    let tags = alloc::vec![
        PriceTag { name: "sparks", weight: SemWeight::Combat, avg_us: 300, samples: 3 },
        PriceTag { name: "bloom", weight: SemWeight::Ambient, avg_us: 100, samples: 3 },
        PriceTag { name: "flash", weight: SemWeight::Transition, avg_us: 60, samples: 3 },
        PriceTag { name: "dust", weight: SemWeight::Ambient, avg_us: 80, samples: 3 },
    ];
    let ladder = DegradeLadder::build(&tags);
    let ladder_ok = ladder[0].tag.name == "bloom"
        && ladder[1].tag.name == "dust"
        && ladder[2].tag.name == "flash"
        && ladder[3].tag.name == "sparks";
    let plan = DegradeLadder::degrade(&ladder, 540, 400);
    let plan_ok = plan.dropped.len() == 2
        && plan.dropped[0].name == "bloom"
        && plan.dropped[1].name == "dust"
        && plan.remaining_us == 360
        && plan.assert_ordered().is_ok();
    if ladder_ok && plan_ok {
        s.ok("EZ16-级-02-环境先减战斗保留");
    } else {
        s.fail("EZ16-级-02-环境先减战斗保留", "降级阶梯失效");
    }

    // --- 语义降级（判据五）---
    // 级-03：极端紧张全降级 + 序断言——budget=0 时四级全降（含战斗——
    // 预算归零语义），降级序权重单调不减不违例；读屏名单含全部四名。
    let plan0 = DegradeLadder::degrade(&ladder, 540, 0);
    let all_dropped = plan0.dropped.len() == 4
        && plan0.remaining_us == 0
        && plan0.assert_ordered().is_ok();
    let spoken = plan0.spoken();
    let spoken_ok = spoken.contains("环境bloom")
        && spoken.contains("环境dust")
        && spoken.contains("转场flash")
        && spoken.contains("战斗sparks");
    if all_dropped && spoken_ok {
        s.ok("EZ16-级-03-极端全降与读屏名单");
    } else {
        s.fail("EZ16-级-03-极端全降与读屏名单", "全降语义失效");
    }

    // --- 双层预算（判据六）---
    // 预-01：三档 U07 口径手算——budget=300：used=250 余量 16.7%>10% Ok；
    // used=280 余量 6.7%∈(0,10%] Tight；used=300 余量 0 Over；
    // used=310 超 3.3% Over；budget=0 恒 Over。
    let v_ok = DualBudget::verdict_of(300, 250) == BudgetVerdict::Ok;
    let v_tight = DualBudget::verdict_of(300, 280) == BudgetVerdict::Tight;
    let v_over0 = DualBudget::verdict_of(300, 300) == BudgetVerdict::Over;
    let v_over1 = DualBudget::verdict_of(300, 310) == BudgetVerdict::Over;
    let v_zero = DualBudget::verdict_of(0, 0) == BudgetVerdict::Over;
    if v_ok && v_tight && v_over0 && v_over1 && v_zero {
        s.ok("EZ16-预-01-三档U07口径手算");
    } else {
        s.fail("EZ16-预-01-三档U07口径手算", "三档判定失效");
    }

    // --- 双层预算（判据七）---
    // 预-02：双层记账——子系统粒子 250/300（Ok）+后处理 190/200（Tight），
    // 组合 440/500（余量 12% Ok）；联合判定取最差档：粒子视点 Ok、
    // 后处理视点 Tight、组合 Tight；未知子系统拒绝 E_PF_NOFX。
    let mut db = DualBudget::new(alloc::vec![("particles", 300), ("postfx", 200)], 500);
    let c1 = db.commit("particles", 250);
    let c2 = db.commit("postfx", 190);
    let commits_ok = c1.is_ok() && c2.is_ok() && db.combo_used_us() == 440;
    let dual_ok = db.verdict("particles") == BudgetVerdict::Ok
        && db.verdict("postfx") == BudgetVerdict::Tight
        && db.combo_verdict() == BudgetVerdict::Ok;
    let unknown = matches!(db.commit("ghost", 1), Err(E_PF_NOFX));
    if commits_ok && dual_ok && unknown {
        s.ok("EZ16-预-02-双层记账取最差档");
    } else {
        s.fail("EZ16-预-02-双层记账取最差档", "双层预算失效");
    }

    // --- 双层预算（判据八）---
    // 预-03：组合超支显性——再 commit 后组合 560/500 超 12% →
    // combo Over；联合判定取最差档（任一层超支即超支——组合超支传染
    // 两个子系统的联合视点），子层账面独立未动（postfx 仍 190）。
    let c3 = db.commit("particles", 120);
    let over_ok = c3.is_ok()
        && db.combo_used_us() == 560
        && db.combo_verdict() == BudgetVerdict::Over
        && db.verdict("postfx") == BudgetVerdict::Over
        && db.verdict("particles") == BudgetVerdict::Over;
    if over_ok {
        s.ok("EZ16-预-03-组合超支不静默");
    } else {
        s.fail("EZ16-预-03-组合超支不静默", "超支显性失效");
    }

    // --- 回灌（判据九）---
    // 灌-01：实测回灌手算——基线 100 实测 [150,150,150] avg=150 偏差
    // 50%>20% 触发校准新基线 150；偏差 10%（[110]）不回灌基线保持；
    // 偏差恰 20%（[120]）不触发（> 语义）；零样本/含零行拒 E_PF_NOCAL。
    let fb1 = Feedback::calibrate(100, &[150, 150, 150]);
    let fb2 = Feedback::calibrate(100, &[110]);
    let fb3 = Feedback::calibrate(100, &[120]);
    let fb4 = Feedback::calibrate(100, &[]);
    let fb5 = Feedback::calibrate(100, &[100, 0]);
    let fb_ok = matches!(fb1, Ok((true, 150)))
        && matches!(fb2, Ok((false, 100)))
        && matches!(fb3, Ok((false, 100)))
        && matches!(fb4, Err(E_PF_NOCAL))
        && matches!(fb5, Err(E_PF_NOCAL));
    if fb_ok {
        s.ok("EZ16-灌-01-偏差回灌校准手算");
    } else {
        s.fail("EZ16-灌-01-偏差回灌校准手算", "回灌失效");
    }

    // --- 回灌（判据十）---
    // 灌-02：低方向偏差回灌对拍——基线 200 实测 [100,100] avg=100 偏差
    // 50%（低方向也失准）触发校准新基线 100（双向漂移都校——事实压假设）。
    let fb6 = Feedback::calibrate(200, &[100, 100]);
    let fb6_ok = matches!(fb6, Ok((true, 100)));
    if fb6_ok {
        s.ok("EZ16-灌-02-双向漂移都校准");
    } else {
        s.fail("EZ16-灌-02-双向漂移都校准", "双向回灌失效");
    }

    // --- 判据元（判据十一）---
    // 元-01：五码互异 + 文案非空互异 + 版本串锚定。
    let codes = [E_PF_NOFX, E_PF_UNORDERED, E_PF_OVER, E_PF_NOCAL, E_PF_BADSAMPLE];
    let mut uniq = true;
    for i in 0..5 {
        for j in (i + 1)..5 {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let mut det_uniq = true;
    let mut det_nonempty = true;
    for i in 0..5 {
        let d = reason_of(codes[i]);
        if d.is_empty() || reason_of(codes[i]) == reason_of("未知") {
            det_nonempty = false;
        }
        for j in (i + 1)..5 {
            if reason_of(codes[i]) == reason_of(codes[j]) {
                det_uniq = false;
            }
        }
    }
    let unknown_reason = reason_of("E_UNKNOWN") != reason_of(E_PF_NOFX);
    let meta_ok = uniq && det_uniq && det_nonempty && unknown_reason && PERF_VERSION == "Z16-perf-v1";
    if meta_ok {
        s.ok("EZ16-元-01-五码闭集与版本");
    } else {
        s.fail("EZ16-元-01-五码闭集与版本", "判据元失效");
    }

    // --- 判据元（判据十二）---
    // 元-02：零 panic 面——除法链除数均为非零路径（samples>0 才入账、
    // avg.max(1)、budget=0 短路 Over）；行为对拍：空画像器全量导出空、
    // 空阶梯降级平凡返回零降级。
    let empty_pf = PerfProfiler::new();
    let empty_ladder = DegradeLadder::build(&[]);
    let empty_plan = DegradeLadder::degrade(&empty_ladder, 500, 100);
    let no_panic = empty_pf.all_tags().is_empty()
        && empty_pf.is_empty()
        && empty_plan.dropped.is_empty()
        && empty_plan.remaining_us == 500
        && empty_plan.assert_ordered().is_ok();
    if no_panic {
        s.ok("EZ16-元-02-零panic空态平凡");
    } else {
        s.fail("EZ16-元-02-零panic空态平凡", "panic 面失效");
    }

    // --- 编排（判据十三）---
    // 编-01：画像→阶梯→预算全链路——画像四特效价签→阶梯预排序→
    // cur=540 降级至 360→预算 tight 场景对拍（360 vs budget 400 组合
    // 余量 10% 恰 Tight——(400-360)×1e6/400=100000 恰等于阈不大于）。
    let mut pf2 = PerfProfiler::new();
    let _ = pf2.sample("sparks", SemWeight::Combat, 300);
    let _ = pf2.sample("bloom", SemWeight::Ambient, 100);
    let _ = pf2.sample("flash", SemWeight::Transition, 60);
    let _ = pf2.sample("dust", SemWeight::Ambient, 80);
    let tags2 = pf2.all_tags();
    let ladder2 = DegradeLadder::build(&tags2);
    let plan2 = DegradeLadder::degrade(&ladder2, 540, 400);
    let chain_ok = plan2.dropped.len() == 2
        && plan2.remaining_us == 360
        && DualBudget::verdict_of(400, 360) == BudgetVerdict::Tight;
    let sum_spoken = plan2.spoken();
    let chain_spoken = sum_spoken.contains("环境bloom") && sum_spoken.contains("环境dust");
    if chain_ok && chain_spoken {
        s.ok("EZ16-编-01-画像阶梯预算全链");
    } else {
        s.fail("EZ16-编-01-画像阶梯预算全链", "全链路失效");
    }

    s
}
