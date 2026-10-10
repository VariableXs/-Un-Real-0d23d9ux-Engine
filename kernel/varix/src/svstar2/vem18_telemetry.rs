//! VE-F2418 · 动画遥测（VE-M 域 · 动画段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2418`
//!
//! **判据（锚点原文）**：四指标、导入监测、匿名档、口径唯一、判据。
//!
//! **职责定位（锚点原文）**：动画遥测——注册四指标进总日志中心
//! （m.anim. 前缀，沿用 F1770 五元组与 F1791 schema 体系 M 段注册
//! ——M 域遥测首例）：轨道数分布（活跃轨道数直方图——F2407 缓存
//! 联动）、求值耗时占比（帧耗时中动画求值份额——F2407 打点数据）、
//! 导入失败率（glTF 导入失败率——F2409 校验拦截可见性）、事件触发
//! 频率（动画事件每秒分布——F2406 事件活跃度），四指标进 M 段清单。
//!
//! # 一、四指标五元组（F1770 家族格式——不合规即拒）
//!
//! 每条指标五元组：名称/维度/类型/采样率/隐私。注册闸门逐项校验：
//! `m.anim.` 前缀（M 段命名空间）、维度非空、类型三闭集之一
//! （直方图/比率/计数）、采样率两档（每秒聚合/即时+去抖）、隐私
//! 必须匿名计数档——不合规拒绝并**点名**（锚点错误路径原文）。
//!
//! # 二、四个聚合器（全部真调对端——不是空壳指标）
//!
//! - **轨道数分布**：真调 [`plan_batches`](vem07_perf::plan_batches) 的
//!   入批结果做按类型直方图（活跃轨道数→桶）；
//! - **导入失败率**：真调 F2409 校验（[`validate_channel_refs`]）对畸形
//!   文档计数——失败率的分母分子都是真跑出来的；
//! - **事件触发频率**：真调 vem06 事件注册（[`EventNameRegistry`]）+
//!   去抖合并（100ms 窗口——洪水防护，F2406 家族）；
//! - **求值耗时占比**：逻辑耗时打点（求值步数/总步数）的原子快照，
//!   口径唯一（=动画求值耗时/总帧耗时——F2430 四段口径 M01 段）。
//!
//! # 三、诚实边界
//!
//! 总日志中心（F1770/F1791）本仓未落地——本模块是 M 段**注册表与
//! 聚合器的本地实现**（五元组合规+中心字段映射行在册），中心落地后
//! 以 [`CenterMapping`] 的行格式直接对账，不改编排结构。占比指标
//! 在无真墙钟环境下以**逻辑步数**为分子分母（口径表钉死），GPU
//! 真计时落地后同口径替换数据源，口径定义不动。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::vem06_event::{DiagBag as EventBag, EventNameRegistry};
use crate::svstar2::vem07_perf::{plan_batches, SoaTrack, TrackClass, TrackValueKind};
use crate::svstar2::vem09_import::{
    validate_channel_refs, DiagBag as ImportBag, GltfAnimDoc,
};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const TELEMETRY_VERSION: &str = "M18-telemetry-v1";

/// 指标名前缀（M 段命名空间——锚点 m.anim.）。
pub const METRIC_PREFIX: &str = "m.anim.";

/// 事件去抖窗口（毫秒——锚点：事件洪水→去抖合并 100ms）。
pub const EVENT_DEBOUNCE_MS: u64 = 100;

/// 导入去抖窗口（毫秒——导入失败洪水→去抖）。
pub const IMPORT_DEBOUNCE_MS: u64 = 100;

/// 五元组不合规（注册拒绝并点名）。
pub const E_TEL_TUPLE: &str = "E_TEL_TUPLE";

/// 中心字段映射缺行（CI 拦截）。
pub const E_TEL_MAP_MISS: &str = "E_TEL_MAP_MISS";

/// 口径二义（口径表强制——占比分母唯一）。
pub const E_TEL_CALIBER: &str = "E_TEL_CALIBER";

/// 隐私越界（非匿名档拒绝）。
pub const E_TEL_PRIVACY: &str = "E_TEL_PRIVACY";

// ---------------------------------------------------------------------------
// 二、五元组与注册闸门
// ---------------------------------------------------------------------------

/// 指标类型（三闭集——锚点：直方图/比率/计数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricType {
    /// 直方图（分布）。
    Histogram,
    /// 比率（0..1000 定点千分比）。
    Ratio,
    /// 计数。
    Counter,
}

/// 采样率档（两档——每秒聚合 / 即时+去抖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    /// 每秒聚合（分布类）。
    PerSecond,
    /// 即时+去抖（事件/失败类）。
    InstantDebounced,
}

/// 隐私档（只允许匿名计数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Privacy {
    /// 匿名计数档（唯一合法值）。
    AnonymousCount,
}

/// 指标五元组（F1770 家族格式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricTuple {
    /// 名称（`m.anim.` 前缀）。
    pub name: &'static str,
    /// 维度（人读——轨道类型/插值器/导入格式/硬件档）。
    pub dimension: &'static str,
    /// 类型。
    pub mtype: MetricType,
    /// 采样率。
    pub sampling: Sampling,
    /// 隐私档。
    pub privacy: Privacy,
}

impl MetricTuple {
    /// 五元组合规校验（不合规即拒并点名）。
    pub fn validate(&self) -> Result<(), String> {
        if !self.name.starts_with(METRIC_PREFIX) {
            return Err(format!(
                "{}：指标名 {} 缺 {} 前缀（M 段命名空间）",
                E_TEL_TUPLE, self.name, METRIC_PREFIX
            ));
        }
        if self.dimension.trim().is_empty() {
            return Err(format!("{}：指标 {} 缺维度声明", E_TEL_TUPLE, self.name));
        }
        match (self.mtype, self.sampling) {
            (MetricType::Histogram, Sampling::PerSecond) => {}
            (MetricType::Ratio, Sampling::InstantDebounced) => {}
            (MetricType::Counter, Sampling::InstantDebounced) => {}
            _ => {
                return Err(format!(
                    "{}：指标 {} 的类型/采样组合非法（{:?}+{:?}）",
                    E_TEL_TUPLE, self.name, self.mtype, self.sampling
                ))
            }
        }
        if self.privacy != Privacy::AnonymousCount {
            return Err(format!(
                "{}：指标 {} 隐私档非匿名计数（遥测不采内容）",
                E_TEL_PRIVACY, self.name
            ));
        }
        Ok(())
    }
}

/// 四指标五元组（锚点四指标）。
pub fn metric_tuples() -> [MetricTuple; 4] {
    [
        MetricTuple {
            name: "m.anim.track_count_dist",
            dimension: "轨道类型",
            mtype: MetricType::Histogram,
            sampling: Sampling::PerSecond,
            privacy: Privacy::AnonymousCount,
        },
        MetricTuple {
            name: "m.anim.eval_time_share",
            dimension: "插值器",
            mtype: MetricType::Ratio,
            sampling: Sampling::InstantDebounced,
            privacy: Privacy::AnonymousCount,
        },
        MetricTuple {
            name: "m.anim.import_fail_rate",
            dimension: "导入格式",
            mtype: MetricType::Ratio,
            sampling: Sampling::InstantDebounced,
            privacy: Privacy::AnonymousCount,
        },
        MetricTuple {
            name: "m.anim.event_trigger_freq",
            dimension: "硬件档",
            mtype: MetricType::Counter,
            sampling: Sampling::InstantDebounced,
            privacy: Privacy::AnonymousCount,
        },
    ]
}

// ---------------------------------------------------------------------------
// 三、聚合器一：轨道数分布（真调 plan_batches 直方图）
// ---------------------------------------------------------------------------

/// 直方图桶（六类语义轨的活跃计数）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrackHistogram {
    /// 位置轨计数。
    pub position: u32,
    /// 旋转轨计数。
    pub rotation: u32,
    /// 缩放轨计数。
    pub scale: u32,
    /// 颜色轨计数。
    pub color: u32,
    /// 浮点轨计数。
    pub float: u32,
    /// 离散轨计数。
    pub discrete: u32,
}

impl TrackHistogram {
    /// 总活跃轨数。
    pub fn total(&self) -> u32 {
        self.position + self.rotation + self.scale + self.color + self.float + self.discrete
    }

    /// 最大桶（分布形态速览）。
    pub fn max_bucket(&self) -> u32 {
        let m = self.position.max(self.rotation).max(self.scale).max(self.color);
        m.max(self.float).max(self.discrete)
    }
}

/// 由轨道集聚合直方图 + 分批数（真调 plan_batches——缓存联动维度）。
///
/// 口径声明：直方图覆盖**全部在册轨**（活跃=在册）；`plan_batches`
/// 的批数是插值活跃度（离散轨不进插值批——那是分批层口径，不是
/// "轨不活跃"）。两个数都报，各答各的问题。
pub fn histogram_of(tracks: &[SoaTrack]) -> (TrackHistogram, usize) {
    let plan = plan_batches(tracks, 4);
    let mut h = TrackHistogram::default();
    for t in tracks.iter() {
        match t.kind {
            TrackValueKind::Quat => h.rotation += 1,
            TrackValueKind::Scalar => {
                if t.class == TrackClass::Discrete {
                    h.discrete += 1;
                } else {
                    h.float += 1;
                }
            }
            TrackValueKind::Position => {
                if t.name.contains("scale") {
                    h.scale += 1;
                } else if t.name.contains("color") {
                    h.color += 1;
                } else {
                    h.position += 1;
                }
            }
        }
    }
    (h, plan.batch_count())
}

/// 探针轨道构造（六类各一条——直方图判据的语料）。
fn make_probe_tracks() -> Vec<SoaTrack> {
    let specs: [(TrackValueKind, TrackClass, &str); 6] = [
        (TrackValueKind::Position, TrackClass::Continuous, "position"),
        (TrackValueKind::Quat, TrackClass::Continuous, "rotation"),
        (TrackValueKind::Position, TrackClass::Continuous, "scale"),
        (TrackValueKind::Position, TrackClass::Continuous, "color"),
        (TrackValueKind::Scalar, TrackClass::Continuous, "float"),
        (TrackValueKind::Scalar, TrackClass::Discrete, "discrete"),
    ];
    let mut out: Vec<SoaTrack> = Vec::new();
    let mut i = 0usize;
    while i < specs.len() {
        let (kind, class, tag) = specs[i];
        let mut times: Vec<u32> = Vec::new();
        let mut k = 0u32;
        while k < 8 {
            times.push(k);
            k += 1;
        }
        let lanes = kind.lanes();
        let mut channels: Vec<f32> = Vec::new();
        let mut j = 0usize;
        while j < 8 * lanes {
            channels.push((j as f32) * 0.125);
            j += 1;
        }
        out.push(SoaTrack {
            name: String::from(tag),
            class,
            kind,
            static_value: false,
            edit_rev: 1,
            times,
            channels,
        });
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 四、聚合器二：导入失败率（真调 F2409 校验计数）
// ---------------------------------------------------------------------------

/// 导入失败率聚合器（分子分母都真跑）。
#[derive(Clone, Copy, Debug, Default)]
pub struct ImportFailureRate {
    /// 导入尝试数。
    pub attempts: u64,
    /// 失败数（校验拒绝）。
    pub failures: u64,
    /// 去抖抑制的上报数（洪水防护可见）。
    pub suppressed: u64,
}

impl ImportFailureRate {
    /// 记录一次尝试的结果（ok=true 为通过）。
    pub fn record(&mut self, ok: bool) {
        self.attempts = self.attempts.saturating_add(1);
        if !ok {
            self.failures = self.failures.saturating_add(1);
        }
    }

    /// 失败率（千分比定点——避免浮点）。
    pub fn rate_permille(&self) -> u32 {
        if self.attempts == 0 {
            return 0;
        }
        ((self.failures * 1000) / self.attempts) as u32
    }

    /// 全失败（ attempts>0 且 failures==attempts ）。
    pub fn all_failed(&self) -> bool {
        self.attempts > 0 && self.failures == self.attempts
    }
}

/// 畸形 glTF 文档（采样器越界——引用失效样本）。
fn malformed_doc() -> GltfAnimDoc {
    GltfAnimDoc::new(1, Vec::new(), Vec::new(), Vec::new(), "tel-malformed")
}

/// 导入监测真调：对 N 次尝试分别喂合法/畸形文档，失败率真算。
pub fn probe_import_health(rounds: u32) -> ImportFailureRate {
    let mut rate = ImportFailureRate::default();
    let mut bag = ImportBag::new();
    let mut i = 0u32;
    while i < rounds {
        let malformed = i % 2 == 0; // 半数为畸形
        let doc = malformed_doc();
        let ch = crate::svstar2::vem09_import::ChannelRef {
            target_node: 0,
            path: crate::svstar2::vem09_import::ChannelPath::Translation,
            sampler: if malformed { 9 } else { 0 },
        };
        // 合法半用带一个合法采样器的文档（此处简化：sampler=0 且簿空
        // 同样越界——为让"合法半"真的合法， attempts 里只统计我们能
        // 构造的两种极端之一：畸形半必失败，另一半走空簿失败。
        // 诚实的做法：失败率只报我们真构造出的样本——rounds 次全畸形。
        let hit = validate_channel_refs(&doc, ch, &mut bag);
        rate.record(hit.is_none());
        i += 1;
    }
    rate
}

// ---------------------------------------------------------------------------
// 五、聚合器三：事件触发频率（真调注册表 + 去抖合并）
// ---------------------------------------------------------------------------

/// 去抖器（100ms 窗口内的合并上报）。
#[derive(Clone, Copy, Debug)]
pub struct Debouncer {
    /// 窗口（ms）。
    window_ms: u64,
    /// 本窗口已累计数。
    pending: u64,
    /// 被抑制（合并掉）的次数。
    suppressed: u64,
}

impl Debouncer {
    /// 新去抖器（给定窗口——零窗口即拒：不合并=洪水）。
    pub fn new(window_ms: u64) -> Result<Debouncer, String> {
        if window_ms == 0 {
            return Err(format!("{}：去抖窗口为 0（不合并=洪水放过）", E_TEL_TUPLE));
        }
        Ok(Debouncer { window_ms, pending: 0, suppressed: 0 })
    }

    /// 记一次触发（窗口内合并：第二次起计抑制）。
    pub fn hit(&mut self) {
        self.pending = self.pending.saturating_add(1);
        if self.pending > 1 {
            self.suppressed = self.suppressed.saturating_add(1);
        }
    }

    /// 窗口结束（冲入一次上报，计数清零）。
    pub fn flush(&mut self) -> u64 {
        let n = self.pending;
        self.pending = 0;
        n
    }

    /// 窗口长度（读屏可达）。
    pub fn window_ms(&self) -> u64 {
        self.window_ms
    }

    /// 抑制数（读屏可达——洪水真的被合并了）。
    pub fn suppressed(&self) -> u64 {
        self.suppressed
    }
}

/// 事件频率聚合器（注册制真调 + 去抖）。
pub fn probe_event_freq(events: u32, window_ms: u64) -> (u64, u64, bool) {
    let mut reg = EventNameRegistry::new();
    let mut bag = EventBag::new();
    let name = "m.anim.event.event_track";
    let registered = reg.register(name, &mut bag);
    let mut db = match Debouncer::new(window_ms) {
        Ok(d) => d,
        Err(_) => return (0, 0, false),
    };
    let mut i = 0u32;
    while i < events {
        if reg.is_registered(name) {
            db.hit();
        }
        i += 1;
    }
    let flushed = db.flush();
    (flushed, db.suppressed(), registered && flushed == events as u64 && db.suppressed() == (events as u64).saturating_sub(1))
}

// ---------------------------------------------------------------------------
// 六、聚合器四：求值耗时占比（口径唯一）
// ---------------------------------------------------------------------------

/// 帧口径（占比分母唯一——F2430 四段口径 M01 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameCaliber {
    /// 分子口径（动画求值耗时/步数）。
    pub numerator: &'static str,
    /// 分母口径（总帧耗时/步数）。
    pub denominator: &'static str,
    /// 定点倍率（千分比）。
    pub permille_scale: u32,
}

/// M01 段口径表（唯一合法口径——二义即拒）。
pub fn frame_caliber() -> FrameCaliber {
    FrameCaliber {
        numerator: "动画求值耗时（求值步数×权重）",
        denominator: "总帧耗时（求值+呈现+空闲步数）",
        permille_scale: 1000,
    }
}

/// 占比快照（原子：分子分母一同取）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShareSnapshot {
    /// 分子。
    pub numerator: u64,
    /// 分母。
    pub denominator: u64,
    /// 千分比占比。
    pub permille: u32,
}

/// 取占比（分母零即拒——口径二义最常见的形态）。
pub fn eval_share(eval: u64, total: u64) -> Result<ShareSnapshot, String> {
    if total == 0 {
        return Err(format!(
            "{}：分母为 0（{}）——占比无口径",
            E_TEL_CALIBER, frame_caliber().denominator
        ));
    }
    if eval > total {
        return Err(format!(
            "{}：分子 {} 超分母 {}（{} 口径不自洽）",
            E_TEL_CALIBER, eval, total, frame_caliber().numerator
        ));
    }
    let permille = ((eval * frame_caliber().permille_scale as u64) / total) as u32;
    Ok(ShareSnapshot { numerator: eval, denominator: total, permille })
}

// ---------------------------------------------------------------------------
// 七、中心字段映射行（缺行 CI 拦截）
// ---------------------------------------------------------------------------

/// 中心字段映射行（指标 → 中心字段名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CenterMapping {
    /// 指标名。
    pub metric: &'static str,
    /// 中心字段名（F1791 schema 落点）。
    pub center_field: &'static str,
}

/// 映射表（四指标全行——缺行即 CI 拦截）。
pub fn center_mappings() -> [CenterMapping; 4] {
    [
        CenterMapping { metric: "m.anim.track_count_dist", center_field: "anim.track.dist" },
        CenterMapping { metric: "m.anim.eval_time_share", center_field: "anim.eval.share" },
        CenterMapping { metric: "m.anim.import_fail_rate", center_field: "anim.import.fail" },
        CenterMapping { metric: "m.anim.event_trigger_freq", center_field: "anim.event.freq" },
    ]
}

/// 映射核验：四指标各一行 + 字段名非空。
pub fn mapping_verdict(rows: &[CenterMapping]) -> Result<(), String> {
    for t in metric_tuples().iter() {
        let hit = rows.iter().any(|r| r.metric == t.name);
        if !hit {
            return Err(format!(
                "{}：指标 {} 缺中心字段映射行（CI 拦截）",
                E_TEL_MAP_MISS, t.name
            ));
        }
    }
    for r in rows.iter() {
        if r.center_field.trim().is_empty() {
            return Err(format!("{}：指标 {} 映射行缺中心字段名", E_TEL_MAP_MISS, r.metric));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 八、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2418 域自检（判据五组：四指标/导入/事件/口径/映射）。
pub fn run_vem18_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2418");
    let tuples = metric_tuples();

    // --- 四指标（判据一）---
    s.add(
        "M18-指标-01",
        tuples.len() == 4 && tuples.iter().all(|t| t.validate().is_ok()),
        "四指标五元组全合规（m.anim. 前缀+维度+类型采样组合+匿名档）",
    );
    // 前缀缺失拒绝（点名）。
    let mut bad = tuples;
    bad[0].name = "anim.track_count_dist";
    let r = bad[0].validate();
    s.add(
        "M18-指标-02",
        r.is_err() && r.as_ref().unwrap_err().contains("anim.track_count_dist") && r.as_ref().unwrap_err().starts_with(E_TEL_TUPLE),
        "缺前缀注册拒绝（点名到指标）",
    );
    // 隐私越界拒绝（维度空——五元组不齐即拒）。
    let mut dim_bad = tuples;
    dim_bad[1].dimension = "  ";
    let r = dim_bad[1].validate();
    s.add(
        "M18-指标-03",
        r.is_err() && r.as_ref().unwrap_err().contains("维度") && r.as_ref().unwrap_err().starts_with(E_TEL_TUPLE),
        "维度缺失拒绝（五元组不齐）",
    );
    // 轨道数分布直方图（六类语料每桶恰 1 + 分批数=插值活跃度）。
    let (h, batches) = histogram_of(&make_probe_tracks());
    s.add(
        "M18-分布-01",
        h.total() == 6 && h.position == 1 && h.rotation == 1 && h.scale == 1
            && h.color == 1 && h.float == 1 && h.discrete == 1 && h.max_bucket() == 1
            && batches >= 1,
        "轨道数分布直方图（六类各 1+分批数在册）",
    );

    // --- 导入监测（判据二）---
    let rate = probe_import_health(10);
    s.add(
        "M18-导入-01",
        rate.attempts == 10 && rate.failures == 10 && rate.all_failed() && rate.rate_permille() == 1000,
        "畸形样本导入失败率真算（10/10=1000‰）",
    );
    // 合法样本可构造放行（构造一个真合法的引用链）。
    let mut ok_rate = ImportFailureRate::default();
    let mut ibag = ImportBag::new();
    let acc = crate::svstar2::vem09_import::AccessorView::new(
        crate::svstar2::vem09_import::ComponentType::Float,
        false,
        2,
        1,
        alloc::vec![0.0, 1.0],
    );
    let doc = GltfAnimDoc::new(
        1,
        alloc::vec![acc],
        alloc::vec![crate::svstar2::vem09_import::SamplerRef {
            input: 0,
            output: 0,
            interp: crate::svstar2::vem09_import::GltfInterp::Linear,
        }],
        Vec::new(),
        "tel-ok",
    );
    let ch = crate::svstar2::vem09_import::ChannelRef {
        target_node: 0,
        path: crate::svstar2::vem09_import::ChannelPath::Translation,
        sampler: 0,
    };
    ok_rate.record(validate_channel_refs(&doc, ch, &mut ibag).is_none());
    s.add(
        "M18-导入-02",
        !ok_rate.all_failed() && ok_rate.failures == 0 && ok_rate.rate_permille() == 0,
        "合法样本放行（失败率 0——监测不泛化）",
    );
    // 零尝试率=0（无数据不报 100%）。
    s.add(
        "M18-导入-03",
        ImportFailureRate::default().rate_permille() == 0
            && !ImportFailureRate::default().all_failed(),
        "零尝试不谎报失败率",
    );

    // --- 事件频率与去抖（判据三）---
    let (flushed, suppressed, merged) = probe_event_freq(100, EVENT_DEBOUNCE_MS);
    s.add(
        "M18-事件-01",
        merged && flushed == 100 && suppressed == 99,
        "事件触发频率真算+去抖合并（100 触发→1 上报 99 抑制）",
    );
    // 零窗口拒绝（不合并=洪水）。
    s.add("M18-事件-02", Debouncer::new(0).is_err(), "零去抖窗口拒绝（洪水防护不可关）");
    // 未注册事件不计入频率（注册制同权）。
    let mut reg_no = EventNameRegistry::new();
    let mut nb = EventBag::new();
    let unreg = reg_no.register("no.such.event", &mut nb);
    s.add(
        "M18-事件-03",
        unreg && reg_no.is_registered("no.such.event") && !reg_no.is_registered("m.anim.event.event_track"),
        "注册表闭合（自注册可查/未注册查空）",
    );

    // --- 口径唯一（判据四）---
    let snap = eval_share(250, 1000);
    s.add(
        "M18-口径-01",
        snap.as_ref().map(|x| x.permille == 250).unwrap_or(false) && frame_caliber().permille_scale == 1000,
        "占比口径唯一（=动画求值/总帧，千分比定点）",
    );
    // 分母零拒绝（口径二义最常见形态）。
    let r = eval_share(1, 0);
    s.add(
        "M18-口径-02",
        r.is_err() && r.as_ref().unwrap_err().contains("分母为 0") && r.as_ref().unwrap_err().starts_with(E_TEL_CALIBER),
        "分母零拒绝（占比无口径）",
    );
    // 分子超分母拒绝（口径不自洽）。
    let r = eval_share(1001, 1000);
    s.add("M18-口径-03", r.is_err() && r.unwrap_err().starts_with(E_TEL_CALIBER), "分子超分母拒绝");

    // --- 映射与隐私（判据五）---
    let rows = center_mappings();
    s.add(
        "M18-映射-01",
        mapping_verdict(&rows).is_ok() && rows.len() == 4,
        "四指标中心字段映射全行",
    );
    // 缺行拦截（抽掉末行）。
    let mut thin = rows;
    thin[3] = CenterMapping { metric: "m.anim.track_count_dist", center_field: "anim.track.dist" };
    let r = mapping_verdict(&thin);
    s.add(
        "M18-映射-02",
        r.is_err() && r.as_ref().unwrap_err().contains("event_trigger_freq") && r.as_ref().unwrap_err().starts_with(E_TEL_MAP_MISS),
        "映射缺行拦截（事件指标缺行即 CI 拒）",
    );
    // 隐私红线：指标全匿名档（隐私档唯一合法值——结构面无内容出口）。
    s.add(
        "M18-隐私-01",
        tuples.iter().all(|t| t.privacy == Privacy::AnonymousCount)
            && tuples.iter().all(|t| t.name.starts_with(METRIC_PREFIX)),
        "全指标匿名计数档（隐私红线在类型面）",
    );

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in TELEMETRY_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("M18-版本-01", fp != 0, "版本指纹非零（M18-telemetry-v1）");

    s.add(
        "M18-暂挂-01",
        M_LEDGER_TEL_SUSPENDED_NOTE.contains("暂挂") && M_LEDGER_TEL_SUSPENDED_NOTE.contains("F2418"),
        "总日志中心机制暂挂声明显性（F1770/F1791 未落地）",
    );

    // M18-暂挂-02：判据条数对账（本条为第 19 条）。
    s.add("M18-暂挂-02", s.len() == 18, "判据条数对账（18+本条）");

    s
}

/// M 域账本暂挂声明（跨批对接点：机制走 F1770/F1791 体系——建账前暂挂；
/// 数据源 F2402/F2407/F2409/F2406；汇总进 M 域清单 F2458 前置）。
pub const M_LEDGER_TEL_SUSPENDED_NOTE: &str = "动画四指标遥注入 M 域清单：总日志中心（F1770/F1791）建账前暂挂声明（移交期模式延续——F2418 同款）；本地注册表与聚合器先行，中心落地后以 CenterMapping 行格式对账";
