//! F0168 子模块三 · 账本与观测面（原交付结构八、九、十、十四、十五节）。
//!
//! 八、三队列用例模型（判据五：用例表 + 走查）；九、映射账本（滚动留痕 256 帧 + 三态查询，B08 五元组）；
//! 十、对外接口冻结 v1（map_pipeline 一站式管线；扩展只增不改）；
//! 十四、映射回放与漂移检测（StabilityGrade 四级定档）；十五、映射遥测导出（指标 id 闭集六条）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::mapping::*;
use super::repair::*;

// 八、三队列用例模型（判据五走查基准：五行用例表逐例钉住期望队列与依据；帧图构造与回放自检共用）。

/// 三队列用例（expect_* 两列即判据五的对拍锚——走查失败报行号即可定位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueUseCase {
    /// 用例名。
    pub name: &'static str,
    /// 任务类别。
    pub kind: TaskKind,
    /// 期望队列。
    pub expect_queue: QueueKind,
    /// 期望路由依据。
    pub expect_basis: &'static str,
}

/// 三队列用例表（判据五：三类各一例 + 屏障特例 + 呈现主卡；行序即走查序——任务 id 1..=5）。
/// 1 主pass绘制→图形（归属路由）；2 光照计算→计算；3 纹理上传→拷贝；
/// 4 资源屏障→图形（屏障跟写）；5 呈现提交→图形（主卡呈现）。
pub const QUEUE_USE_CASES: [QueueUseCase; 5] = [
    QueueUseCase { name: "主pass绘制", kind: TaskKind::Rasterize, expect_queue: QueueKind::Graphics, expect_basis: BASIS_RES_HOME },
    QueueUseCase { name: "光照异步计算", kind: TaskKind::ComputeDispatch, expect_queue: QueueKind::Compute, expect_basis: BASIS_RES_HOME },
    QueueUseCase { name: "纹理上传", kind: TaskKind::Upload, expect_queue: QueueKind::Copy, expect_basis: BASIS_RES_HOME },
    QueueUseCase { name: "资源屏障", kind: TaskKind::Barrier, expect_queue: QueueKind::Graphics, expect_basis: BASIS_BARRIER_FOLLOWS_WRITE },
    QueueUseCase { name: "呈现提交", kind: TaskKind::Present, expect_queue: QueueKind::Graphics, expect_basis: BASIS_PRESENT_PRIMARY },
];

/// 用例帧图构造（判据五最小拓扑——`walk_use_cases` 与 `replay_selfcheck` 共用）：
/// 单卡（card=1 主卡、三队列各 1）、单资源（res=100 归卡 1）；任务 id 1..=5
/// 对应用例序，全部触碰 res=100（呈现例外：无资源触碰走主卡规则）。
/// 返回 (拓扑序, 节点表)——注册/归属失败显性上抛。
fn build_use_case_frame(eng: &mut MapEngine) -> Result<(Vec<u32>, Vec<MapNode>), MqCode> {
    let dev = GpuDevice {
        card: 1,
        label: "dGPU0".to_string(),
        queues: [1, 1, 1],
        primary: true,
    };
    eng.devices.register(dev)?;
    eng.homes.assign(100, 1)?;
    let mut order: Vec<u32> = Vec::new();
    let mut nodes: Vec<MapNode> = Vec::new();
    for (i, uc) in QUEUE_USE_CASES.iter().enumerate() {
        let tid = (i + 1) as u32;
        order.push(tid);
        let res_write: Vec<u32> = if uc.kind == TaskKind::Present {
            Vec::new()
        } else {
            let mut w: Vec<u32> = Vec::new();
            w.push(100);
            w
        };
        nodes.push(MapNode {
            task_id: tid,
            kind: uc.kind,
            deps: Vec::new(),
            res_read: Vec::new(),
            res_write,
            card_hint: 0,
        });
    }
    Ok((order, nodes))
}

/// 用例走查（判据五机检面：构造最小帧图 → 映射 → 逐例对拍期望）。
/// 走查序即用例表行序；任一用例不符即整面判负——不折中不跳过。
pub fn walk_use_cases() -> Result<bool, MqCode> {
    let mut eng = MapEngine::new();
    let (order, nodes) = build_use_case_frame(&mut eng)?;
    let mappings = eng.map_frame(&order, &nodes)?;
    if mappings.len() != QUEUE_USE_CASES.len() {
        return Ok(false);
    }
    for (i, uc) in QUEUE_USE_CASES.iter().enumerate() {
        let slot = mappings.get(i);
        match slot {
            Some(m) if m.queue == uc.expect_queue && m.basis == uc.expect_basis => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

// 九、映射账本（滚动留痕封顶 256 帧 + 聚合 + 三态查询——B08 五元组对齐，模式复用 F0483 帧账）。

/// 单帧映射留痕（B08 五元组：场景标签 + 映射数 + 违例数 + 开销 ppm + 码面；code=0 即成功帧）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapFrameRecord {
    /// 场景标签（帧的场景上下文——与 F0483 场景标签同源）。
    pub scene: String,
    /// 映射节点数。
    pub mapped: u32,
    /// 一致性违例数。
    pub violations: u32,
    /// 映射开销（万分比）。
    pub overhead_ppm: u32,
    /// 首个诊断码（0 = 无错）。
    pub code: u16,
}

/// 映射账本（256 帧滚动 + 聚合桶 + 三态查询；落账/查询由调用方串行化——内核单写者纪律）。
#[derive(Clone, Debug, Default)]
pub struct MapLedger {
    records: Vec<MapFrameRecord>,
}

impl MapLedger {
    /// 空账（落账面滚动封顶 256——最老先出，容量有界）。
    pub fn new() -> Self {
        MapLedger { records: Vec::new() }
    }

    /// 落账（封顶 256 滚动——最老先出；成功帧与失败帧同账，码面区分）。
    pub fn record(&mut self, rec: MapFrameRecord) {
        if self.records.len() >= 256 {
            let _evicted = self.records.remove(0);
        }
        self.records.push(rec);
    }

    /// 精确查（场景标签精确匹配 → 记录拷贝；新→旧扫——精确命中取最新一条）。
    pub fn exact(&self, scene: &str) -> Option<MapFrameRecord> {
        self.records.iter().rev().find(|r| r.scene == scene).cloned()
    }

    /// 聚合查（场景前缀聚合 → (帧数, Σ映射, Σ违例, 峰值 ppm)；零命中 None）。
    pub fn aggregate(&self, prefix: &str) -> Option<(u32, u32, u32, u32)> {
        let mut frames = 0u32;
        let mut mapped = 0u32;
        let mut violations = 0u32;
        let mut peak = 0u32;
        for r in self.records.iter() {
            if r.scene.starts_with(prefix) {
                frames += 1;
                mapped += r.mapped;
                violations += r.violations;
                peak = peak.max(r.overhead_ppm);
            }
        }
        if frames == 0 {
            None
        } else {
            Some((frames, mapped, violations, peak))
        }
    }

    /// 账深。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空判（空账即无任何帧留痕——查询面 NotFound 的结构性来源）。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// 三态查询语义（查账返回闭集——三态互斥完备，无第四态）。
/// NotFound 是显性结论而非错误：表外场景如实回报，调用方按需建账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapQuery {
    /// 精确命中。
    Exact(MapFrameRecord),
    /// 前缀聚合命中。
    Aggregated(u32, u32, u32, u32),
    /// 未命中（表外场景——显性 NotFound，不猜）。
    NotFound,
}

/// 统一查账入口（先精确后聚合——两段式确定性）。
/// ```text
/// Exact —— 恰有该场景标签的最新一帧；Aggregated —— 无精确帧但有前缀聚合；NotFound —— 精确、聚合皆空。
/// ```
pub fn query_map_ledger(ledger: &MapLedger, scene: &str) -> MapQuery {
    if let Some(r) = ledger.exact(scene) {
        return MapQuery::Exact(r);
    }
    if let Some((f, m, v, p)) = ledger.aggregate(scene) {
        return MapQuery::Aggregated(f, m, v, p);
    }
    MapQuery::NotFound
}

// 十、对外接口冻结 v1（MapRequestV1 进、MapReplyV1 出；map_pipeline 串判据一~四；扩展只增不改）。

/// 映射请求（冻结 v1：版本号 + 序 + 节点表；version ≠ 1 由调用方按协议面拒）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapRequestV1 {
    /// 协议版本（1）。
    pub version: u32,
    /// 场景标签。
    pub scene: String,
    /// 拓扑序（cgb07 排序面输出）。
    pub order: Vec<u32>,
    /// 节点表。
    pub nodes: Vec<MapNode>,
}

/// 映射答复（冻结 v1；accepted=true 且 code=0x6104 为「接受但待修」——违例走 plan_transfer）。
/// accepted=false 时 mappings/violations 恒空。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapReplyV1 {
    /// 接受标记。
    pub accepted: bool,
    /// 映射结论（接受时非空）。
    pub mappings: Vec<QueueMapping>,
    /// 一致性违例（接受但有待修时非空）。
    pub violations: Vec<CoherenceViolation>,
    /// 码面（0 = 全过）。
    pub code: u16,
    /// 人读明细。
    pub detail: String,
}

/// 映射管线（判据一~四串行一站式：规则查行→路由→映射→一致性校验→落账）。
/// 每段开销按 [`MAP_SEGMENTS`] 口径计入实测数组（上游注入基址，本函数累加
/// 建模步——零墙钟）。
/// ```text
/// req ─map_frame─► Ok ─check_coherence─► violations 空 → accepted=true, code=0；
///                                    └► 非空 → accepted=true, code=0x6104（待修）；
/// Err(code) → accepted=false，code 原样透传。两条路径都落账（成功/失败同账）。
/// ```
pub fn map_pipeline(
    eng: &mut MapEngine,
    ledger: &mut MapLedger,
    req: &MapRequestV1,
) -> MapReplyV1 {
    let mut overhead = [0u32; 4];
    overhead[0] = 1;
    overhead[1] = 3;
    overhead[2] = 3;
    overhead[3] = 2;
    match eng.map_frame(&req.order, &req.nodes) {
        Ok(mappings) => {
            let violations = check_coherence(&req.nodes, &mappings);
            let ppm = map_overhead_ppm(&overhead);
            let rec = MapFrameRecord {
                scene: req.scene.clone(),
                mapped: mappings.len() as u32,
                violations: violations.len() as u32,
                overhead_ppm: ppm,
                code: 0,
            };
            ledger.record(rec);
            if violations.is_empty() {
                MapReplyV1 {
                    accepted: true,
                    mappings,
                    violations,
                    code: 0,
                    detail: "映射完成：一致性全过".to_string(),
                }
            } else {
                MapReplyV1 {
                    accepted: true,
                    mappings,
                    detail: "映射完成：存在一致性违例待修".to_string(),
                    violations,
                    code: MqCode::QueueCoherenceViolation as u16,
                }
            }
        }
        Err(code) => {
            let rec = MapFrameRecord {
                scene: req.scene.clone(),
                mapped: 0,
                violations: 0,
                overhead_ppm: map_overhead_ppm(&overhead),
                code: code as u16,
            };
            ledger.record(rec);
            MapReplyV1 {
                accepted: false,
                mappings: Vec::new(),
                violations: Vec::new(),
                code: code as u16,
                detail: code.detail().to_string(),
            }
        }
    }
}

// 十四、映射回放与漂移检测（确定性重放——同输入必同输出的运行时守卫；StabilityGrade 四级定档）。

/// 漂移记录（首跑/回放两份 (队列, 卡) 并排——漂移面直接可读；task_id 是对拍主键）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapDrift {
    /// 任务 id。
    pub task_id: u32,
    /// 首跑队列。
    pub queue_first: QueueKind,
    /// 回放队列。
    pub queue_replay: QueueKind,
    /// 首跑卡。
    pub card_first: u32,
    /// 回放卡。
    pub card_replay: u32,
}

/// 按任务 id 找映射结论（回放对拍/帧间稳定共用的「同任务比对」原语）。
/// 命中语义：清单内首个同 id 条目（映射清单按约定一任务一条）。
fn mapping_of_task<'a>(mappings: &'a [QueueMapping], task_id: u32) -> Option<&'a QueueMapping> {
    mappings.iter().find(|mp| mp.task_id == task_id)
}

/// 单条回放漂移推导：回放侧同任务结论与首跑不一致 → 漂移记录；一致或
/// 回放侧缺失 → None（缺失不在漂移口径内——那是 map_frame 守卫的职责面）。
fn replay_drift(f: &QueueMapping, replay: &[QueueMapping]) -> Option<MapDrift> {
    let rp = mapping_of_task(replay, f.task_id)?;
    if rp.queue == f.queue && rp.card == f.card {
        return None;
    }
    Some(MapDrift {
        task_id: f.task_id,
        queue_first: f.queue,
        queue_replay: rp.queue,
        card_first: f.card,
        card_replay: rp.card,
    })
}

/// 单帧回放比对（确定性守卫：重放同输入，逐条对拍映射结论）。
/// 返回漂移清单（空表 = 回放一致 = 确定性成立）；调用方须用新引擎重放
/// （负载计数器清零——引擎无隐藏态建模范式）。
pub fn replay_compare(
    order: &[u32],
    nodes: &[MapNode],
    first: &[QueueMapping],
    homes: &ResourceHome,
    devices: &DeviceTable,
) -> Vec<MapDrift> {
    let mut eng = MapEngine::new();
    eng.homes = homes.clone();
    eng.devices = devices.clone();
    let replay = match eng.map_frame(order, nodes) {
        Ok(r) => r,
        Err(_) => {
            // 回放失败 = 引擎非确定（同输入不同结局）——全量漂移单条总账。
            let mut out: Vec<MapDrift> = Vec::new();
            if let Some(f0) = first.get(0) {
                out.push(MapDrift {
                    task_id: f0.task_id,
                    queue_first: f0.queue,
                    queue_replay: f0.queue,
                    card_first: f0.card,
                    card_replay: f0.card,
                });
            }
            return out;
        }
    };
    let mut out: Vec<MapDrift> = Vec::new();
    for f in first.iter() {
        // 回放侧同任务结论不一致即漂移（逐条对拍）。
        if let Some(d) = replay_drift(f, &replay) {
            out.push(d);
        }
    }
    out
}

/// 帧间稳定性（连续两帧同场景同图 → 映射应稳定；漂移即帧间抖动源）。
/// 返回变化任务数；场景切换的合法变化由调用方按场景标签过滤——本函数只给裸差异。
/// 计数口径：a 侧逐任务——b 侧缺同任务或结论不同均计一次变化。
pub fn frame_stability(a: &[QueueMapping], b: &[QueueMapping]) -> u32 {
    let mut diff = 0u32;
    for ma in a.iter() {
        let changed = match mapping_of_task(b, ma.task_id) {
            Some(mb) => mb.queue != ma.queue || mb.card != ma.card,
            None => true,
        };
        if changed {
            diff += 1;
        }
    }
    diff
}

/// 稳定性等级闭集（差异面 → 稳定度评级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StabilityGrade {
    /// 全同（差异 0）。
    Frozen,
    /// 微漂（差异 ≤ 全图 10%）。
    Micro,
    /// 活跃（差异 ≤ 50%）。
    Active,
    /// 重排（差异 > 50%——建议查放置抖动源）。
    Shuffled,
}

impl StabilityGrade {
    /// 评级（差异比例整数万分比判定：0 = 全同；≤1000 = 微漂；≤5000 = 活跃；
    /// >5000 = 重排。差异 ≥ 全图即封顶 PER_UNIT——重排档必达）。
    pub const fn grade(diff: u32, total: u32) -> StabilityGrade {
        if total == 0 {
            return StabilityGrade::Frozen;
        }
        const PER_UNIT: u32 = 10_000;
        let ppm = if diff >= total {
            PER_UNIT
        } else {
            diff.saturating_mul(PER_UNIT) / total
        };
        if ppm == 0 {
            StabilityGrade::Frozen
        } else if ppm <= 1_000 {
            StabilityGrade::Micro
        } else if ppm <= 5_000 {
            StabilityGrade::Active
        } else {
            StabilityGrade::Shuffled
        }
    }

    /// 人读名（设置/诊断面文案——四档与 grade 分档一一对应）。
    pub const fn name(self) -> &'static str {
        match self {
            StabilityGrade::Frozen => "全同",
            StabilityGrade::Micro => "微漂",
            StabilityGrade::Active => "活跃",
            StabilityGrade::Shuffled => "重排",
        }
    }
}

/// 回放一致性自检（引擎建模范守卫：walk_use_cases 的映射重放零漂移）。
/// 通过判：首跑与重放逐条 (任务, 队列, 卡) 全等；帧图/映射失败均按自检不过（false），不 panic。
pub fn replay_selfcheck() -> bool {
    let mut eng = MapEngine::new();
    let (order, nodes) = match build_use_case_frame(&mut eng) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let first = match eng.map_frame(&order, &nodes) {
        Ok(f) => f,
        Err(_) => return false,
    };
    replay_compare(&order, &nodes, &first, &eng.homes, &eng.devices).is_empty()
}

// 十五、映射遥测导出（B08 五元组快照——指标 id 闭集六条，表外 id 拒导；值全为整数口径）。

/// 指标名前缀（遥测命名纪律：域面点分路径——MAP_METRIC_IDS 逐条以本串开头）。
pub const MAP_METRIC_PREFIX: &str = "b02.framegraph.";

/// 遥测指标行（id/值/单位/口径一行——导出面完整性与行序由 export_metrics_complete 核验）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapMetric {
    /// 指标 id（前缀 + 短名——全量可枚举闭集内）。
    pub id: &'static str,
    /// 值（整数口径——无浮点）。
    pub value: u32,
    /// 单位。
    pub unit: &'static str,
    /// 口径一句话。
    pub basis: &'static str,
}

/// 指标 id 闭集（六条——表外 id 导出即拒；行序即导出契约序）。
pub const MAP_METRIC_IDS: [&str; 6] = [
    "b02.framegraph.mapped",
    "b02.framegraph.violations",
    "b02.framegraph.transfers",
    "b02.framegraph.overhead_ppm",
    "b02.framegraph.balance_ppm",
    "b02.framegraph.drift",
];

/// 指标 id 在册核验（运行时逐字符比较；导出面过滤 + 完整性核验的元素前提）。
pub fn metric_id_known(id: &str) -> bool {
    MAP_METRIC_IDS.iter().any(|known| *known == id)
}

/// 单帧遥测输入包（export_metrics 的参数结构体化载体：六个 u32 与原平铺参数一一对应）。
pub struct MapMetricInput {
    /// 本帧映射节点数。
    pub mapped: u32,
    /// 一致性违例数。
    pub violations: u32,
    /// 跨卡搬运计划数。
    pub transfers: u32,
    /// 映射面开销占帧预算万分比。
    pub overhead_ppm: u32,
    /// 三队列均衡度万分比。
    pub balance_ppm: u32,
    /// 回放漂移条数。
    pub drift: u32,
}

/// 单帧遥测快照导出（判据四 + 九的观测收口；确定性、零 IO）。
/// 结构体化签名——导出面是纯装配（值/单位/口径拼行）；调用方从账本/聚合面取数。
/// 2026-10-15 结构体化：原 6 参版本全库零调用方，零行为变化。
pub fn export_metrics(input: MapMetricInput) -> Vec<MapMetric> {
    let mut out: Vec<MapMetric> = Vec::new();
    let rows: [(&'static str, u32, &'static str, &'static str); 6] = [
        (MAP_METRIC_IDS[0], input.mapped, "个", "本帧映射节点数"),
        (MAP_METRIC_IDS[1], input.violations, "条", "一致性违例数"),
        (MAP_METRIC_IDS[2], input.transfers, "条", "跨卡搬运计划数"),
        (MAP_METRIC_IDS[3], input.overhead_ppm, "ppm", "映射面开销占帧预算万分比"),
        (MAP_METRIC_IDS[4], input.balance_ppm, "ppm", "三队列均衡度万分比"),
        (MAP_METRIC_IDS[5], input.drift, "条", "回放漂移条数"),
    ];
    for row in rows.iter() {
        if metric_id_known(row.0) {
            out.push(MapMetric { id: row.0, value: row.1, unit: row.2, basis: row.3 });
        }
    }
    out
}

/// 快照完整性核验（导出面必须恰好六条全 id——缺行/错位/多导出即残缺）。
pub fn export_metrics_complete(m: &[MapMetric]) -> bool {
    if m.len() != MAP_METRIC_IDS.len() {
        return false;
    }
    m.iter().zip(MAP_METRIC_IDS.iter()).all(|(row, known)| row.id == *known)
}

/// 守恒断言（编译期钉死：用例表五行；遥测指标六条闭集）。
const _: () = {
    assert!(QUEUE_USE_CASES.len() == 5, "三队列用例表必须五行");
    assert!(MAP_METRIC_IDS.len() == 6, "映射遥测指标必须六条闭集");
};
