//! VE-F6009 · 物理遥测（vead09_phystel）
//!
//! 物理遥测四类聚合——**步长耗时 / 物理体数量 / 约束数量 / 回放使用**
//! （物理参数可分析、**场景内容不记录**——遥测分析的是数值曲线不是
//! 世界内容）；**物理体数量时间线**——规模随时间变化曲线洪峰一眼可
//! 见（峰值超基线阈值即标注 BodiesSpike）；**时间线开放导出**（行式
//! 确定性导出两遍逐字一致——对账口径可复算）；**时间线含异常事件标
//! 注**（四类闭集）→**标注含事件来源分类**（四源闭集）→**分类含统计
//! 汇总**（逐源计数恰值对账——标注不是散点是有账的台账）。
//!
//! 数据结构（锚点原文）：四类聚合器。
//!
//! 错误路径与降级矩阵：**失败→缓存**（采样失败上帧缓存续用+stale
//! 告警——账不缺行只是略旧）；**含内容→阻断**（内容红线是类型面闸：
//! 事件结构零字符串字段+禁录名单注入尝试即拒 E_PT_CONTENT_BLOCKED
//! ——分析面拿不到场景名实体名材质名才是红线不是承诺）；**超限→滚
//! 动**（时间线有界环满丢最旧计数不静默 E_PT_ROLL_OVERFLOW）。性能：
//! 聚合 O(样本)（单遍累加四元画像）。跨批对接点：U 域总日志中心
//! （vead08 回放使用喂数/vez10 特效遥测四元画像同口径/vee09 六字段
//! 零内容同纪律）。无障碍：口径公开——四类指标口径三元组（名/单位/
//! 周期）声明制+FNV 指纹对拍漂移立案 E_PT_CALIBER_DRIFT（口径是契
//! 约不是注释）+caliber_spoken 读屏可达。隐私面：零内容类型面保证。
//!
//! 诊断码 0xB3 段五码互异：`E_PT_CONTENT_BLOCKED` /
//! `E_PT_CACHE_STALE` / `E_PT_ROLL_OVERFLOW` / `E_PT_BAD_SOURCE` /
//! `E_PT_CALIBER_DRIFT`。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// 物理遥测版本。
pub const PHYSTEL_VERSION: &str = "D09-phystel-v1";
/// 时间线环容量（超限滚动丢最旧）。
pub const RING_CAP: usize = 64;
/// 洪峰标注阈值（峰值 ≥ 基线均值 × 倍数即标注）。
pub const PEAK_SPIKE_RATIO: u64 = 2;
/// 基线窗口（洪峰检测取最近 N 点均值作基线）。
pub const BASELINE_WIN: usize = 3;
/// 步长耗时预算（µs——超即 StepOverrun 标注）。
pub const STEP_BUDGET_US: u64 = 5000;
/// 采样失败允许连续缓存续用次数（超此值缓存过期不再续）。
pub const STALE_LIMIT: u32 = 3;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0xB3 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 内容阻断：遥测入口收到场景内容（禁录名单命中）。
pub const E_PT_CONTENT_BLOCKED: u16 = 0xB301;
/// 缓存续用：采样失败上帧缓存顶上（stale 告警——账不缺但略旧）。
pub const E_PT_CACHE_STALE: u16 = 0xB302;
/// 滚动溢出：时间线环满丢最旧（计数不静默）。
pub const E_PT_ROLL_OVERFLOW: u16 = 0xB303;
/// 来源闭集外：异常标注来源未声明。
pub const E_PT_BAD_SOURCE: u16 = 0xB304;
/// 口径漂移：指标口径三元组与冻结表不符。
pub const E_PT_CALIBER_DRIFT: u16 = 0xB305;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_PT_CONTENT_BLOCKED => "内容阻断：遥测只收数值不收场景内容",
        E_PT_CACHE_STALE => "缓存续用：采样失败上帧缓存顶上并告警",
        E_PT_ROLL_OVERFLOW => "滚动溢出：时间线环满丢最旧，计数留痕",
        E_PT_BAD_SOURCE => "来源闭集外：异常标注来源未声明",
        E_PT_CALIBER_DRIFT => "口径漂移：指标口径三元组与冻结表不符",
        _ => "未知诊断码",
    }
}

// ---------------------------------------------------------------------------
// 立案簿（0xB3 段事件账）
// ---------------------------------------------------------------------------

/// 诊断立案单条。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

/// 诊断立案簿（顺序追加，容量有界防膨胀）。
#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    /// 读屏摘要。
    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 四元画像（vez10 同口径——count/total/max/min 单遍累加 O(样本)）
// ---------------------------------------------------------------------------

/// 四元画像聚合器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuadAgg {
    pub count: u64,
    pub total: u64,
    pub max: u64,
    pub min: u64,
}

impl QuadAgg {
    pub fn new() -> QuadAgg {
        QuadAgg { count: 0, total: 0, max: 0, min: u64::MAX }
    }

    /// 单样本累加（O(1)——min 初值 u64::MAX 空态不可误报）。
    pub fn add(&mut self, v: u64) {
        self.count += 1;
        self.total += v;
        if v > self.max {
            self.max = v;
        }
        if v < self.min {
            self.min = v;
        }
    }

    /// 均值（整数除法——count==0 返回 0 不除零）。
    pub fn mean(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.total / self.count
        }
    }

    /// 空态（无样本）。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 读屏摘要。
    pub fn spoken(&self) -> String {
        if self.is_empty() {
            return "画像空（无样本）".to_string();
        }
        format!(
            "样本{}合计{}均值{}峰值{}谷值{}",
            self.count, self.total, self.mean(), self.max, self.min
        )
    }
}

// ---------------------------------------------------------------------------
// 内容红线（类型面闸——不是文档承诺）
// ---------------------------------------------------------------------------

/// 禁录名单（场景内容键——遥测入口命中即阻断）。
pub const CONTENT_KEYS: [&str; 6] =
    ["scene_name", "entity_name", "material_name", "mesh_id", "texture_id", "world_seed"];

/// 内容审计：键是否命中禁录名单（O(名单) 定长扫描）。
pub fn is_content_key(key: &str) -> bool {
    CONTENT_KEYS.iter().any(|k| *k == key)
}

/// 遥测键白名单（四类聚合允许的键——白名单外一律拒）。
pub const TELEMETRY_KEYS: [&str; 8] = [
    "step_cost_us", "body_count", "constraint_count", "replay_record",
    "replay_play", "replay_seek", "solver_iter", "island_count",
];

/// 键是否为合法遥测键。
pub fn is_telemetry_key(key: &str) -> bool {
    TELEMETRY_KEYS.iter().any(|k| *k == key)
}

// ---------------------------------------------------------------------------
// 异常标注（四类闭集 × 四源闭集——标注有账不是散点）
// ---------------------------------------------------------------------------

/// 异常事件类别（四类闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnomalyKind {
    /// 物理体数量洪峰（峰值超基线阈值）。
    BodiesSpike,
    /// 约束数量洪峰。
    ConstraintSpike,
    /// 步长超预算（单步耗时 > STEP_BUDGET_US）。
    StepOverrun,
    /// 回放异常（对拍失配/耗尽等——vead08 喂入）。
    ReplayFail,
}

impl AnomalyKind {
    pub const COUNT: usize = 4;

    pub fn all() -> [AnomalyKind; AnomalyKind::COUNT] {
        [
            AnomalyKind::BodiesSpike,
            AnomalyKind::ConstraintSpike,
            AnomalyKind::StepOverrun,
            AnomalyKind::ReplayFail,
        ]
    }

    pub fn zh(self) -> &'static str {
        match self {
            AnomalyKind::BodiesSpike => "体数洪峰",
            AnomalyKind::ConstraintSpike => "约束洪峰",
            AnomalyKind::StepOverrun => "步长超预算",
            AnomalyKind::ReplayFail => "回放异常",
        }
    }
}

/// 异常来源分类（四源闭集——标注必须归源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnomalySource {
    /// 世界步进主循环。
    WorldStep,
    /// 约束求解器。
    ConstraintSolve,
    /// 回放引擎（vead08）。
    ReplayEngine,
    /// 外部注入（调试/工具）。
    External,
}

impl AnomalySource {
    pub const COUNT: usize = 4;

    pub fn all() -> [AnomalySource; AnomalySource::COUNT] {
        [
            AnomalySource::WorldStep,
            AnomalySource::ConstraintSolve,
            AnomalySource::ReplayEngine,
            AnomalySource::External,
        ]
    }

    pub fn zh(self) -> &'static str {
        match self {
            AnomalySource::WorldStep => "世界步进",
            AnomalySource::ConstraintSolve => "约束求解",
            AnomalySource::ReplayEngine => "回放引擎",
            AnomalySource::External => "外部注入",
        }
    }
}

/// 异常标注单条（帧号+类别+来源——零内容：只有数值与枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnomalyMark {
    pub frame: u64,
    pub kind: AnomalyKind,
    pub source: AnomalySource,
    /// 触发值（洪峰值/耗时值——数值不是内容）。
    pub value: u64,
}

// ---------------------------------------------------------------------------
// 时间线（物理体数量曲线——洪峰可见/标注有账/导出确定/超限滚动）
// ---------------------------------------------------------------------------

/// 时间线采样点（帧号+体数+约束数——零内容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountPoint {
    pub frame: u64,
    pub bodies: u64,
    pub constraints: u64,
}

/// 物理体数量时间线（有界环+洪峰标注+来源账+确定导出）。
#[derive(Debug)]
pub struct Timeline {
    points: Vec<CountPoint>,
    marks: Vec<AnomalyMark>,
    pub dropped_oldest: usize,
    /// 逐源统计汇总（标注不是散点——分类有账）。
    src_tally: [u64; AnomalySource::COUNT],
    /// 逐类统计汇总。
    kind_tally: [u64; AnomalyKind::COUNT],
}

impl Timeline {
    pub fn new() -> Timeline {
        Timeline {
            points: Vec::new(),
            marks: Vec::new(),
            dropped_oldest: 0,
            src_tally: [0; AnomalySource::COUNT],
            kind_tally: [0; AnomalyKind::COUNT],
        }
    }

    /// 采样推进（O(1) 追加，满环丢最旧计数不静默）。
    pub fn push(&mut self, p: CountPoint, bag: &mut DiagBag) {
        if self.points.len() >= RING_CAP {
            self.points.remove(0);
            self.dropped_oldest += 1;
            bag.push(E_PT_ROLL_OVERFLOW, "时间线滚动", &format!("丢最旧帧 {}", p.frame));
        }
        self.points.push(p);
    }

    /// 洪峰检出（O(基线窗)：最近点超基线均值×阈值即标注——体数与
    /// 约束各自独立基线，不互借）。
    /// 返回是否标注。
    pub fn detect_spike(&mut self, frame: u64) -> bool {
        let n = self.points.len();
        if n < BASELINE_WIN + 1 {
            return false;
        }
        let cur = self.points[n - 1];
        let b_base = self.points[n - 1 - BASELINE_WIN..n - 1]
            .iter()
            .map(|p| p.bodies)
            .sum::<u64>()
            / BASELINE_WIN as u64;
        let c_base = self.points[n - 1 - BASELINE_WIN..n - 1]
            .iter()
            .map(|p| p.constraints)
            .sum::<u64>()
            / BASELINE_WIN as u64;
        let mut spiked = false;
        if b_base > 0 && cur.bodies >= b_base * PEAK_SPIKE_RATIO {
            self.mark(AnomalyMark {
                frame,
                kind: AnomalyKind::BodiesSpike,
                source: AnomalySource::WorldStep,
                value: cur.bodies,
            });
            spiked = true;
        }
        if c_base > 0 && cur.constraints >= c_base * PEAK_SPIKE_RATIO {
            self.mark(AnomalyMark {
                frame,
                kind: AnomalyKind::ConstraintSpike,
                source: AnomalySource::ConstraintSolve,
                value: cur.constraints,
            });
            spiked = true;
        }
        spiked
    }

    /// 步长超预算标注（步进循环喂数——value 即耗时）。
    pub fn mark_step_overrun(&mut self, frame: u64, cost_us: u64) {
        if cost_us > STEP_BUDGET_US {
            self.mark(AnomalyMark {
                frame,
                kind: AnomalyKind::StepOverrun,
                source: AnomalySource::WorldStep,
                value: cost_us,
            });
        }
    }

    /// 回放异常标注（vead08 喂入——来源回放引擎）。
    pub fn mark_replay_fail(&mut self, frame: u64, value: u64) {
        self.mark(AnomalyMark {
            frame,
            kind: AnomalyKind::ReplayFail,
            source: AnomalySource::ReplayEngine,
            value,
        });
    }

    /// 标注入账（台账不是立案——marks 与双 tally 足账）。
    fn mark(&mut self, m: AnomalyMark) {
        self.marks.push(m);
        self.src_tally[m.source as usize] += 1;
        self.kind_tally[m.kind as usize] += 1;
    }

    /// 逐源统计（分类统计汇总——判据恰值对账）。
    pub fn source_tally(&self) -> [u64; AnomalySource::COUNT] {
        self.src_tally
    }

    /// 逐类统计。
    pub fn kind_tally(&self) -> [u64; AnomalyKind::COUNT] {
        self.kind_tally
    }

    /// 峰值体数（时间线洪峰一眼可见——O(点数) 单遍）。
    pub fn peak_bodies(&self) -> u64 {
        self.points.iter().map(|p| p.bodies).max().unwrap_or(0)
    }

    /// 峰值约束数。
    pub fn peak_constraints(&self) -> u64 {
        self.points.iter().map(|p| p.constraints).max().unwrap_or(0)
    }

    /// 点数。
    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// 标注数。
    pub fn mark_count(&self) -> usize {
        self.marks.len()
    }

    /// 开放导出（行式确定性——两遍逐字一致；含标注列）。
    pub fn export_rows(&self) -> String {
        let mut out = String::from("frame,bodies,constraints,marks\n");
        for p in self.points.iter() {
            let m = self
                .marks
                .iter()
                .filter(|mk| mk.frame == p.frame)
                .map(|mk| mk.kind.zh())
                .collect::<Vec<_>>()
                .join("+");
            out.push_str(&format!("{},{},{},{}\n", p.frame, p.bodies, p.constraints, m));
        }
        out
    }

    /// 时间线读屏摘要（洪峰+标注账）。
    pub fn spoken(&self) -> String {
        format!(
            "时间线{}点峰值体{}峰约束{}标注{}滚动{}",
            self.points.len(),
            self.peak_bodies(),
            self.peak_constraints(),
            self.marks.len(),
            self.dropped_oldest
        )
    }
}

// ---------------------------------------------------------------------------
// 四类聚合器总装（步长耗时/物理体数量/约束数量/回放使用）
// ---------------------------------------------------------------------------

/// 回放使用聚合（vead08 喂入——record/play/seek 三计数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayUseAgg {
    pub record_count: u64,
    pub play_count: u64,
    pub seek_count: u64,
    pub fail_count: u64,
}

impl ReplayUseAgg {
    pub fn new() -> ReplayUseAgg {
        ReplayUseAgg { record_count: 0, play_count: 0, seek_count: 0, fail_count: 0 }
    }

    pub fn on_record(&mut self) {
        self.record_count += 1;
    }

    pub fn on_play(&mut self) {
        self.play_count += 1;
    }

    pub fn on_seek(&mut self) {
        self.seek_count += 1;
    }

    pub fn on_fail(&mut self, frame: u64, tl: &mut Timeline) {
        self.fail_count += 1;
        tl.mark_replay_fail(frame, self.fail_count);
    }

    pub fn spoken(&self) -> String {
        format!(
            "回放使用 录{}放{}跳{}败{}",
            self.record_count, self.play_count, self.seek_count, self.fail_count
        )
    }
}

/// 失败→缓存架（采样失败上帧缓存续用——账不缺只是略旧）。
#[derive(Debug)]
pub struct CacheShelf {
    last: Option<CountPoint>,
    stale_run: u32,
    pub stale_hits: u32,
    pub fresh_writes: u32,
}

impl CacheShelf {
    pub fn new() -> CacheShelf {
        CacheShelf { last: None, stale_run: 0, stale_hits: 0, fresh_writes: 0 }
    }

    /// 采样成功：缓存刷新。
    pub fn store(&mut self, p: CountPoint) {
        self.last = Some(p);
        self.stale_run = 0;
        self.fresh_writes += 1;
    }

    /// 采样失败：上帧缓存续用（连续失败超 STALE_LIMIT 缓存过期不续）。
    /// 返回 Some(续用点) 或 None（无缓存/过期）。
    pub fn fallback(&mut self, bag: &mut DiagBag) -> Option<CountPoint> {
        if self.stale_run >= STALE_LIMIT {
            return None; // 缓存过期——不再续（略旧也不谎报新鲜）。
        }
        self.stale_run += 1;
        match self.last {
            Some(p) => {
                self.stale_hits += 1;
                bag.push(
                    E_PT_CACHE_STALE,
                    "采样失败缓存续用",
                    &format!("续用帧{} 第{}次", p.frame, self.stale_run),
                );
                Some(p)
            }
            None => None,
        }
    }

    pub fn has_cache(&self) -> bool {
        self.last.is_some()
    }
}

// ---------------------------------------------------------------------------
// 口径公开（三元组声明制 + FNV 指纹对拍漂移立案）
// ---------------------------------------------------------------------------

/// 指标口径三元组（名/单位/更新周期——unx0913 schema 同构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caliber {
    pub name: &'static str,
    pub unit: &'static str,
    pub period: &'static str,
}

/// 四类指标口径冻结表（口径是契约——改动即漂移立案）。
pub const CALIBERS: [Caliber; 4] = [
    Caliber { name: "step_cost", unit: "us", period: "per-step" },
    Caliber { name: "body_count", unit: "count", period: "per-frame" },
    Caliber { name: "constraint_count", unit: "count", period: "per-frame" },
    Caliber { name: "replay_use", unit: "count", period: "per-session" },
];

/// FNV-1a 单步。
pub fn fnv_step(hash: u32, byte: u8) -> u32 {
    (hash ^ byte as u32).wrapping_mul(0x01000193)
}

/// 口径表指纹（FNV 逐三元组推进——漂移对拍锚）。
pub fn caliber_fingerprint(cals: &[Caliber]) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for c in cals {
        for b in c.name.bytes() {
            h = fnv_step(h, b);
        }
        h = fnv_step(h, b'|');
        for b in c.unit.bytes() {
            h = fnv_step(h, b);
        }
        h = fnv_step(h, b'|');
        for b in c.period.bytes() {
            h = fnv_step(h, b);
        }
        h = fnv_step(h, b'\n');
    }
    h
}

/// 口径漂移对拍（表与冻结指纹不符→E_PT_CALIBER_DRIFT）。
pub fn audit_caliber(cals: &[Caliber], frozen: u32, bag: &mut DiagBag) -> bool {
    let fp = caliber_fingerprint(cals);
    if fp != frozen {
        bag.push(E_PT_CALIBER_DRIFT, "口径漂移", &format!("指纹 {:08x}≠{:08x}", fp, frozen));
        false
    } else {
        true
    }
}

/// 口径读屏可达（四类指标三元组逐条念出）。
pub fn caliber_spoken() -> String {
    let mut out = String::from("口径公开：");
    for c in CALIBERS.iter() {
        out.push_str(&format!("[{}/{} {}]", c.name, c.unit, c.period));
    }
    out
}

// ---------------------------------------------------------------------------
// 总装（四类聚合器 + 时间线 + 缓存架 + 键闸——单入口）
// ---------------------------------------------------------------------------

/// 物理遥测总闸。
#[derive(Debug)]
pub struct Phystel {
    /// 步长耗时画像。
    pub step_cost: QuadAgg,
    /// 约束数量画像。
    pub constraints: QuadAgg,
    /// 回放使用。
    pub replay: ReplayUseAgg,
    /// 物理体数量时间线。
    pub tl: Timeline,
    /// 采样失败缓存架。
    pub shelf: CacheShelf,
    /// 立案簿。
    pub bag: DiagBag,
    /// 当前体数（时间线由 step 推进）。
    pub bodies: u64,
}

impl Phystel {
    pub fn new() -> Phystel {
        Phystel {
            step_cost: QuadAgg::new(),
            constraints: QuadAgg::new(),
            replay: ReplayUseAgg::new(),
            tl: Timeline::new(),
            shelf: CacheShelf::new(),
            bag: DiagBag::new(),
            bodies: 0,
        }
    }

    /// 遥测键闸（含内容→阻断；白名单外→拒）——返回是否放行。
    pub fn key_gate(&mut self, key: &str) -> bool {
        if is_content_key(key) {
            self.bag.push(
                E_PT_CONTENT_BLOCKED,
                key,
                "场景内容禁录——遥测只收数值",
            );
            return false;
        }
        if !is_telemetry_key(key) {
            self.bag.push(
                E_PT_CONTENT_BLOCKED,
                key,
                "白名单外键拒收——闭集面",
            );
            return false;
        }
        true
    }

    /// 帧采样（体数+约束数入时间线+洪峰检出；ok=false 走缓存续用）。
    pub fn sample_frame(&mut self, frame: u64, bodies: u64, cons: u64, ok: bool) {
        if !ok {
            // 失败→缓存续用：时间线补上帧点（账不缺只是略旧）。
            if let Some(p) = self.shelf.fallback(&mut self.bag) {
                self.tl.push(p, &mut self.bag);
            }
            return;
        }
        self.bodies = bodies;
        let p = CountPoint { frame, bodies, constraints: cons };
        self.shelf.store(p);
        self.tl.push(p, &mut self.bag);
        self.tl.detect_spike(frame);
    }

    /// 步长耗时喂数（超预算标注）。
    pub fn feed_step_cost(&mut self, frame: u64, cost_us: u64) {
        self.step_cost.add(cost_us);
        self.tl.mark_step_overrun(frame, cost_us);
    }

    /// 约束数量喂数（画像）。
    pub fn feed_constraints(&mut self, n: u64) {
        self.constraints.add(n);
    }

    /// 全量读屏摘要。
    pub fn spoken(&self) -> String {
        format!(
            "步长[{}]；约束[{}]；{}；{}；{}",
            self.step_cost.spoken(),
            self.constraints.spoken(),
            self.replay.spoken(),
            self.tl.spoken(),
            caliber_spoken()
        )
    }
}

// ---------------------------------------------------------------------------
// 判据（十项——四类聚合/内容不记/时间线洪峰/标注来源/统计汇总/
// 开放导出/失败缓存/超限滚动/口径公开/口径漂移/全链编排）
// ---------------------------------------------------------------------------

/// F6009 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vead09_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F6009");

    // 聚-01 四类聚合：步长画像手算锚 [1000,2000,3000]→count3/total6000/
    // max3000/min1000/mean2000；约束 [40,60]→count2 mean50；回放使用三计数。
    {
        let mut pt = Phystel::new();
        pt.feed_step_cost(1, 1000);
        pt.feed_step_cost(2, 2000);
        pt.feed_step_cost(3, 3000);
        let st = pt.step_cost;
        pt.feed_constraints(40);
        pt.feed_constraints(60);
        let ct = pt.constraints;
        pt.replay.on_record();
        pt.replay.on_play();
        pt.replay.on_play();
        pt.replay.on_seek();
        let agg_ok = st.count == 3
            && st.total == 6000
            && st.max == 3000
            && st.min == 1000
            && st.mean() == 2000
            && ct.count == 2
            && ct.mean() == 50
            && pt.replay.record_count == 1
            && pt.replay.play_count == 2
            && pt.replay.seek_count == 1;
        if agg_ok {
            s.ok("F6009-聚-01-四类聚合画像恰值");
        } else {
            s.fail("F6009-聚-01-四类聚合画像恰值", "画像或计数与手算不符");
        }
    }

    // 聚-02 内容不记：禁录键阻断+白名单外拒+白名单内放行。
    {
        let mut pt = Phystel::new();
        let r1 = pt.key_gate("scene_name"); // 禁录名单
        let r2 = pt.key_gate("entity_name");
        let r3 = pt.key_gate("unknown_thing"); // 白名单外
        let r4 = pt.key_gate("step_cost_us"); // 合法键
        let content_ok = !r1 && !r2 && !r3 && r4
            && pt.bag.count_of(E_PT_CONTENT_BLOCKED) == 3
            && is_content_key("material_name")
            && is_telemetry_key("replay_play");
        if content_ok {
            s.ok("F6009-聚-02-内容红线类型面闸");
        } else {
            s.fail("F6009-聚-02-内容红线类型面闸", "键闸判定或立案不符");
        }
    }

    // 线-01 时间线洪峰：基线 [10,12,11] 均值 11 阈 22，峰值 24 标注。
    {
        let mut pt = Phystel::new();
        pt.sample_frame(1, 10, 4, true);
        pt.sample_frame(2, 12, 4, true);
        pt.sample_frame(3, 11, 4, true);
        let no_mark = pt.tl.mark_count();
        pt.sample_frame(4, 24, 5, true); // 24 ≥ 2×11=22 → BodiesSpike
        let tl_ok = no_mark == 0
            && pt.tl.mark_count() == 1
            && pt.tl.peak_bodies() == 24
            && pt.tl.len() == 4
            && pt.bodies == 24;
        if tl_ok {
            s.ok("F6009-线-01-体数时间线洪峰标注");
        } else {
            s.fail("F6009-线-01-体数时间线洪峰标注", "洪峰检出或峰值不符");
        }
    }

    // 线-02 标注来源分类：四类标注全进账+逐源逐类恰值汇总。
    {
        let mut pt = Phystel::new();
        pt.sample_frame(1, 10, 4, true);
        pt.sample_frame(2, 12, 4, true);
        pt.sample_frame(3, 11, 4, true);
        pt.sample_frame(4, 30, 30, true); // 体+约束双洪峰（30 ≥ 22）
        pt.feed_step_cost(5, 8000); // 步长超预算 8000 > 5000
        pt.replay.on_fail(6, &mut pt.tl);
        let src = pt.tl.source_tally();
        let kind = pt.tl.kind_tally();
        // 手算：体峰(4)+约束峰(4)+步长超支(5)+回放失败(6)=4 标注。
        let mark_ok = pt.tl.mark_count() == 4
            && src[AnomalySource::WorldStep as usize] == 2 // 体峰+步长
            && src[AnomalySource::ConstraintSolve as usize] == 1
            && src[AnomalySource::ReplayEngine as usize] == 1
            && src[AnomalySource::External as usize] == 0
            && kind[AnomalyKind::BodiesSpike as usize] == 1
            && kind[AnomalyKind::ConstraintSpike as usize] == 1
            && kind[AnomalyKind::StepOverrun as usize] == 1
            && kind[AnomalyKind::ReplayFail as usize] == 1;
        if mark_ok {
            s.ok("F6009-线-02-标注来源分类统计汇总");
        } else {
            s.fail("F6009-线-02-标注来源分类统计汇总", "逐源逐类账不平");
        }
    }

    // 线-03 开放导出：行式确定性（两遍逐字一致）+标注列在导出中可见。
    {
        let mut pt = Phystel::new();
        pt.sample_frame(1, 10, 4, true);
        pt.sample_frame(2, 12, 4, true);
        pt.sample_frame(3, 11, 4, true);
        pt.sample_frame(4, 24, 5, true); // 洪峰标注
        let e1 = pt.tl.export_rows();
        let e2 = pt.tl.export_rows();
        let exp_ok = e1 == e2
            && e1.starts_with("frame,bodies,constraints,marks\n")
            && e1.contains("4,24,5,体数洪峰")
            && e1.contains("1,10,4,")
            && pt.tl.mark_count() == 1;
        if exp_ok {
            s.ok("F6009-线-03-时间线开放导出确定性");
        } else {
            s.fail("F6009-线-03-时间线开放导出确定性", "导出不确定或标注列缺");
        }
    }

    // 降-01 失败→缓存：采样失败上帧缓存续用+stale 告警；连败 3 次过期。
    {
        let mut pt = Phystel::new();
        pt.sample_frame(1, 10, 4, true);
        pt.sample_frame(2, 0, 0, false); // 失败——续用帧1点
        pt.sample_frame(3, 0, 0, false); // 失败——续用帧1点
        pt.sample_frame(4, 0, 0, false); // 失败——续用帧1点
        pt.sample_frame(5, 0, 0, false); // 过期——不续（STALE_LIMIT=3 已用完）
        let cache_ok = pt.shelf.stale_hits == 3
            && pt.shelf.fresh_writes == 1
            && pt.bag.count_of(E_PT_CACHE_STALE) == 3
            && pt.tl.len() == 4 // 帧1原点+3次续用（帧5过期不补）
            && pt.tl.peak_bodies() == 10
            && pt.shelf.has_cache(); // 缓存仍在（过期不续不清除）
        if cache_ok {
            s.ok("F6009-降-01-采样失败缓存续用过期");
        } else {
            s.fail("F6009-降-01-采样失败缓存续用过期", "续用次数或账面不符");
        }
    }

    // 降-02 超限→滚动：环满丢最旧计数不静默（RING_CAP=64）。
    {
        let mut pt = Phystel::new();
        for f in 1..=(RING_CAP as u64 + 4) {
            pt.sample_frame(f, 10, 2, true); // 68 点进 64 容量环
        }
        let roll_ok = pt.tl.len() == RING_CAP
            && pt.tl.dropped_oldest == 4
            && pt.bag.count_of(E_PT_ROLL_OVERFLOW) == 4;
        if roll_ok {
            s.ok("F6009-降-02-时间线超限滚动留痕");
        } else {
            s.fail("F6009-降-02-时间线超限滚动留痕", "滚动计数或环容量不符");
        }
    }

    // 口-01 口径公开：四类三元组齐+指纹稳定（两次计算同值）+读屏可达。
    {
        let fp1 = caliber_fingerprint(&CALIBERS);
        let fp2 = caliber_fingerprint(&CALIBERS);
        let sp = caliber_spoken();
        let cal_ok = CALIBERS.len() == 4
            && fp1 == fp2
            && fp1 != 0
            && CALIBERS.iter().all(|c| !c.name.is_empty() && !c.unit.is_empty() && !c.period.is_empty())
            && sp.contains("step_cost/us")
            && sp.contains("body_count/count")
            && sp.contains("constraint_count/count")
            && sp.contains("replay_use/count");
        if cal_ok {
            s.ok("F6009-口-01-四类口径三元组公开可达");
        } else {
            s.fail("F6009-口-01-四类口径三元组公开可达", "三元组或指纹不符");
        }
    }

    // 口-02 口径漂移：冻结指纹后改任一三元组→E_PT_CALIBER_DRIFT 立案。
    {
        let mut bag = DiagBag::new();
        let frozen = caliber_fingerprint(&CALIBERS);
        let ok1 = audit_caliber(&CALIBERS, frozen, &mut bag); // 一致——过
        let drifted = [
            Caliber { name: "step_cost", unit: "ms", period: "per-step" }, // 单位被改
            CALIBERS[1],
            CALIBERS[2],
            CALIBERS[3],
        ];
        let ok2 = audit_caliber(&drifted, frozen, &mut bag); // 漂移——立案
        let drift_ok = ok1 && !ok2 && bag.count_of(E_PT_CALIBER_DRIFT) == 1;
        if drift_ok {
            s.ok("F6009-口-02-口径漂移指纹对拍立案");
        } else {
            s.fail("F6009-口-02-口径漂移指纹对拍立案", "漂移未检出或误报");
        }
    }

    // 编-01 全链编排：采样→洪峰→步长超支→回放失败→缓存续用→导出对账
    // →口径指纹全绿。
    {
        let mut pt = Phystel::new();
        pt.sample_frame(1, 10, 4, true);
        pt.sample_frame(2, 12, 4, true);
        pt.sample_frame(3, 11, 4, true);
        pt.sample_frame(4, 26, 6, true); // 体洪峰 26≥22
        pt.feed_step_cost(4, 6000); // 步长超支 6000>5000
        pt.feed_constraints(6);
        pt.replay.on_record();
        pt.replay.on_fail(5, &mut pt.tl);
        pt.sample_frame(5, 0, 0, false); // 采样失败——缓存续用（不产生标注）
        let marks = pt.tl.mark_count();
        let export = pt.tl.export_rows();
        let frozen = caliber_fingerprint(&CALIBERS);
        let cal_ok = audit_caliber(&CALIBERS, frozen, &mut pt.bag);
        // 手算标注：体峰(4)+步长超支(4,6000)+回放失败(5)=3。
        let chain_ok = marks == 3
            && cal_ok
            && export.contains("4,26,6,体数洪峰")
            && pt.replay.fail_count == 1
            && pt.step_cost.max == 6000
            && pt.shelf.stale_hits == 1
            && pt.bag.has(E_PT_CACHE_STALE)
            && pt.tl.peak_bodies() == 26;
        if chain_ok {
            s.ok("F6009-编-01-采样标注导出口径全链O样本对账");
        } else {
            s.fail("F6009-编-01-采样标注导出口径全链O样本对账", "编排对账不符");
        }
    }

    s
}
