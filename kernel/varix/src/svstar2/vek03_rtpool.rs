//! VE-F2003 · 中间渲染目标池（Rust 迁移版 · VE-K 域）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2003`
//!
//! **迁移声明（塔主 2026-10-07 TypeScript 全面迁移指令）**：本模块是
//! `src/system/ve/kDomainPost/f2003-intermediate-rt-pool.ts`（1236 行 TS，
//! 板面 VE-F2003 已完成单）的 **Rust 重实现**——按锚点原文逐面移植：
//! 桶化复用（空闲链表按尺寸×格式分桶，桶键严格匹配绝不跨规格复用）、
//! 区间别名（编译期标注存活闭区间 birth/death，严格不重叠者共用物理资源，
//! 贪心着色 O(RT 数)，正确性优先于最优性——误判别名=读写竞争=画面闪）、
//! 配额双因子水位（张数/字节，95% 拒绝水位给三要素）、引用计数与重复归还/
//! 未知句柄/提前回收防护、帧边界回收（按引用计数而非帧号——别名机制下同
//! 一物理资源可能跨帧存活，按帧号回收会误伤）、泄漏账本（连续 N 帧未回收
//! 疑似泄漏）、与 F2002 编译计划的对接核对。迁移完成后该 TS 文件被本实现
//! 替代（全仓除自身外无其他引用，可删——本任务收尾时删除）。
//!
//! **与 VE-F2016 的关系**：本模块是 F2016 三段 fuzz 第三段（RT 池压力 fuzz）
//! 的被测对象——超配额申请风暴/别名区间重叠注入/万次申请不回收三路对抗
//! 必须全部被防线拦住（防线绕过=P0）。
//!
//! 与 F1528 是同构实现（桶分+复用）但不是同一份代码——跨域同构声明见锚点，
//! 复用其理念而非依赖其实现（域间不交叉依赖）。与 F2001/F2002 的 RtSpec：
//! TS 版独立声明同构结构以免跨条 import 副作用，Rust 版保持同构独立
//! （`vek02_dagexec::RtSpec` 带槽名，本池规格无槽名——分桶键不含槽）。
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;

use super::vek02_dagexec;

use alloc::format;
use alloc::vec::Vec;

// ===========================================================================
// 一、诊断与结果类型
// ===========================================================================

/// 池专属诊断码（独立枚举，不与 F2002 执行器共用——处置方向不同）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolDiagCode {
    /// 池超配额（F1776 中间池水位）→ 拒绝分配。
    QuotaExceeded,
    /// 桶键不匹配（尺寸或格式不同）→ 拒绝跨桶复用。
    BucketKeyMismatch,
    /// 别名冲突：区间标注错误导致读写竞争。
    AliasConflict,
    /// 别名区间的端点非法（死亡早于出生等）。
    AliasIntervalInvalid,
    /// 重复归还。
    DoubleRelease,
    /// 归还未分配的句柄。
    UnknownHandle,
    /// 引用计数未归零即被要求回收。
    EarlyRecollect,
    /// 泄漏超阈（连续 N 帧未回收）。
    LeakSuspected,
    /// 配额查询参数非法。
    QuotaParamInvalid,
    /// 分配计划与池状态不一致。
    PlanPoolMismatch,
}

/// 一条池诊断（三要素）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PoolDiagnostic {
    /// 诊断码。
    pub code: PoolDiagCode,
    /// 描述。
    pub message: String,
    /// 处置建议。
    pub hint: String,
}

/// 诊断聚合器。
#[derive(Clone, Default, Debug)]
pub struct PoolDiagBag {
    items: Vec<PoolDiagnostic>,
}

impl PoolDiagBag {
    /// 入一条诊断。
    pub fn push(&mut self, code: PoolDiagCode, message: &str, hint: &str) {
        self.items.push(PoolDiagnostic {
            code,
            message: if message.is_empty() { "（未提供描述）".to_string() } else { message.to_string() },
            hint: if hint.is_empty() { "（未提供处置建议）".to_string() } else { hint.to_string() },
        });
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 全部诊断。
    pub fn all(&self) -> &[PoolDiagnostic] {
        &self.items
    }

    /// 是否含某码。
    pub fn has(&self, code: PoolDiagCode) -> bool {
        self.items.iter().any(|d| d.code == code)
    }
}

// ===========================================================================
// 二、RT 规格与桶键（判据一：池化复用）
// ===========================================================================

/// RT 规格（与 F2001 的 RtSpec 同构，独立声明以免运行时跨条依赖）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RtSpec {
    /// 宽。
    pub width: u32,
    /// 高。
    pub height: u32,
    /// 格式。
    pub format: String,
}

/// 桶键：尺寸 × 格式的严格组合（绝不跨规格复用）。
pub fn bucket_key(spec: &RtSpec) -> String {
    format!("{}x{}@{}", spec.width, spec.height, spec.format)
}

/// 规格是否合法（宽高为正、格式非空）。
pub fn is_valid_spec(spec: &RtSpec) -> bool {
    spec.width > 0 && spec.height > 0 && !spec.format.is_empty()
}

/// 两规格是否严格相等（桶键相等）。
pub fn same_spec(a: &RtSpec, b: &RtSpec) -> bool {
    a.width == b.width && a.height == b.height && a.format == b.format
}

/// 池中一个 RT 条目（含生命周期状态）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PoolEntry {
    /// 句柄（池与后端之间的唯一契约）。
    pub handle: u64,
    /// 当前绑定的逻辑槽名（别名复用时会变）。
    pub slot: String,
    /// 规格。
    pub spec: RtSpec,
    /// 在用标记。
    pub in_use: bool,
    /// 引用计数（别名后仍逐句柄计数）。
    pub ref_count: u32,
    /// 别名组 id：共用同一物理资源的逻辑 RT 共享该 id；非别名为自身句柄。
    pub alias_group: u64,
}

/// 桶：一个（尺寸×格式）桶的空闲句柄集合。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Bucket {
    /// 桶键。
    pub key: String,
    /// 规格。
    pub spec: RtSpec,
    /// 空闲句柄（已归还且引用计数归零）。
    pub free_handles: Vec<u64>,
}

// ===========================================================================
// 三、区间别名分析（判据二：区间别名，编译期 O(RT 数)）
// ===========================================================================

/// RT 存活区间（**闭区间**：birth=首次被写的执行序下标，death=最后一次被读
/// 的下标。第 N 步内既写又读的 RT death 仍是 N，与「第 N 步才出生」的另一个
/// RT 在 N 处相接，此时**不可**别名——闭区间语义正是防这个）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LiveInterval {
    /// 逻辑槽名。
    pub slot: String,
    /// 首写序号。
    pub birth: u32,
    /// 末读序号。
    pub death: u32,
}

/// 区间是否合法（birth ≤ death）。
pub fn is_valid_interval(iv: &LiveInterval) -> bool {
    iv.death >= iv.birth
}

/// 两个区间是否严格不重叠（可别名）。判据：`a.death < b.birth || b.death <
/// a.birth`（严格小于——闭区间语义，相接不算不重叠）。
pub fn intervals_disjoint(a: &LiveInterval, b: &LiveInterval) -> bool {
    a.death < b.birth || b.death < a.birth
}

/// 一个别名组：共用同一物理资源的逻辑槽。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AliasGroup {
    /// 组 id。
    pub group_id: usize,
    /// 成员槽名。
    pub members: Vec<String>,
}

/// 别名分析结果。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AliasAnalysis {
    /// 逐槽的别名组 id。
    pub group_of_slot: Vec<(String, usize)>,
    /// 别名组列表（成员数 ≥2 才有复用价值，单成员组也列出便于调试）。
    pub groups: Vec<AliasGroup>,
    /// 物理资源数（别名后的实际 RT 数）。
    pub physical_count: usize,
    /// 逻辑 RT 数（别名前）。
    pub logical_count: usize,
    /// 节省的 RT 数。
    pub saved_count: usize,
    /// 非法区间（诊断用）。
    pub invalid_intervals: Vec<String>,
}

impl AliasAnalysis {
    /// 查槽的组 id。
    pub fn group_of(&self, slot: &str) -> Option<usize> {
        self.group_of_slot.iter().find(|(s, _)| s == slot).map(|(_, g)| *g)
    }
}

/**
 * 区间图着色求别名（F2002 编译期调用，O(RT 数)）。
 *
 * 算法（贪心，编译期一次性，不是热路径）：按 birth 升序扫描每个区间；若与
 * 已有组内**所有**成员都不重叠，则并入该组；否则开新组。组内成员两两不重叠
 * 是组的不变式。贪心不保证最少组数，但保证**正确**（绝不误判可别名）——
 * 误判会导致读写竞争（画面闪），正确性优先于最优性。
 * 同 birth 按 slot 名定序，保证分组结果确定性。
 */
pub fn analyze_aliases(intervals: &[LiveInterval]) -> AliasAnalysis {
    let mut valid: Vec<LiveInterval> = Vec::new();
    let mut invalid: Vec<String> = Vec::new();
    for iv in intervals {
        if is_valid_interval(iv) {
            valid.push(iv.clone());
        } else {
            invalid.push(iv.slot.clone());
        }
    }
    valid.sort_by(|a, b| {
        if a.birth != b.birth {
            a.birth.cmp(&b.birth)
        } else {
            a.slot.cmp(&b.slot)
        }
    });
    let mut groups: Vec<AliasGroup> = Vec::new();
    let mut group_of_slot: Vec<(String, usize)> = Vec::new();
    for iv in &valid {
        let mut placed = false;
        for g in groups.iter_mut() {
            // 组内全部成员都与新区间不重叠，才可并入（不变式：组内两两不重叠）
            let disjoint_all = g.members.iter().all(|m| {
                valid
                    .iter()
                    .find(|x| x.slot == *m)
                    .map(|mv| intervals_disjoint(iv, mv))
                    .unwrap_or(false)
            });
            if disjoint_all {
                g.members.push(iv.slot.clone());
                group_of_slot.push((iv.slot.clone(), g.group_id));
                placed = true;
                break;
            }
        }
        if !placed {
            let gid = groups.len();
            groups.push(AliasGroup { group_id: gid, members: vec![iv.slot.clone()] });
            group_of_slot.push((iv.slot.clone(), gid));
        }
    }
    let physical = groups.len();
    let logical = valid.len();
    AliasAnalysis {
        group_of_slot,
        groups,
        physical_count: physical,
        logical_count: logical,
        saved_count: logical.saturating_sub(physical),
        invalid_intervals: invalid,
    }
}

/**
 * 别名正确性校验（调试模式断言，判据五的边界防护）。
 * 逐对检查同组成员是否真的两两不重叠；有重叠即记 ALIAS_CONFLICT（开发期拦截）。
 */
pub fn verify_alias_groups(
    analysis: &AliasAnalysis,
    intervals: &[LiveInterval],
) -> Vec<PoolDiagnostic> {
    let mut out: Vec<PoolDiagnostic> = Vec::new();
    let by_slot = |slot: &str| intervals.iter().find(|iv| iv.slot == slot);
    for g in &analysis.groups {
        for i in 0..g.members.len() {
            for j in (i + 1)..g.members.len() {
                let a = by_slot(&g.members[i]);
                let b = by_slot(&g.members[j]);
                if let (Some(a), Some(b)) = (a, b) {
                    if !intervals_disjoint(a, b) {
                        out.push(PoolDiagnostic {
                            code: PoolDiagCode::AliasConflict,
                            message: format!(
                                "别名组 {} 内 {}[{},{}] 与 {}[{},{}] 区间重叠。",
                                g.group_id, a.slot, a.birth, a.death, b.slot, b.birth, b.death
                            ),
                            hint: "区间标注有误：重叠区间共用同一物理资源会造成读写竞争（画面闪烁）。请核对 DAG 编译期标注的 birth/death。".to_string(),
                        });
                    }
                }
            }
        }
    }
    out
}

// ===========================================================================
// 四、泄漏账本（判据四：泄漏防护）
// ===========================================================================

/// 泄漏记录：按句柄 + 首见帧记账。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LeakRecord {
    /// 句柄。
    pub handle: u64,
    /// 槽名。
    pub slot: String,
    /// 首次发现未回收的帧号。
    pub first_seen_frame: u64,
    /// 最近一次仍未回收的帧号。
    pub last_seen_frame: u64,
    /// 连续未回收帧数。
    pub frames: u64,
}

/// 泄漏账本。
#[derive(Clone, Debug)]
pub struct LeakLedger {
    records: Vec<LeakRecord>,
    threshold: u64,
}

impl LeakLedger {
    /// 构造（默认阈值 3 帧）。
    pub fn new(threshold: u64) -> LeakLedger {
        LeakLedger { records: Vec::new(), threshold }
    }

    /// 登记一次未回收（帧边界调用）。帧号连续则累加；不连续（中间回收过又
    /// 漏了）则重新起算。
    pub fn observe(&mut self, handle: u64, slot: &str, frame: u64) {
        if let Some(prev) = self.records.iter_mut().find(|r| r.handle == handle) {
            let contiguous = frame == prev.last_seen_frame + 1;
            if contiguous {
                prev.frames += 1;
            } else {
                prev.first_seen_frame = frame;
                prev.frames = 1;
            }
            prev.last_seen_frame = frame;
            prev.slot = slot.to_string();
        } else {
            self.records.push(LeakRecord {
                handle,
                slot: slot.to_string(),
                first_seen_frame: frame,
                last_seen_frame: frame,
                frames: 1,
            });
        }
    }

    /// 本帧已回收的句柄应清账。
    pub fn clear(&mut self, handle: u64) {
        self.records.retain(|r| r.handle != handle);
    }

    /// 超阈的泄漏（连续 N 帧未回收）。
    pub fn suspected(&self) -> Vec<LeakRecord> {
        self.records.iter().filter(|r| r.frames >= self.threshold).cloned().collect()
    }

    /// 全部在账记录（按连续帧数降序）。
    pub fn all(&self) -> Vec<LeakRecord> {
        let mut out = self.records.clone();
        out.sort_by(|a, b| b.frames.cmp(&a.frames));
        out
    }

    /// 阈值。
    pub fn limit(&self) -> u64 {
        self.threshold
    }

    /// 在账条数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ===========================================================================
// 五、中间渲染目标池（判据一 + 三 + 五）
// ===========================================================================

/// 配额（F1776 中间池段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quota {
    /// 池容量（同时在用的 RT 数上限）。
    pub max_entries: u32,
    /// 显存字节上限。
    pub max_bytes: u64,
    /// 触发拒绝的水位比例（千分数；锚点：中间池 950‰=95%）。
    /// TS 版用 f32，Rust 迁移改千分数定点——同一份配额在不同优化级别下
    /// 算出不同浮点水位会让「配额对账」变成假对账（F2014 同款教训）。
    pub reject_ratio_permille: u32,
}

/// 池统计（遥测与调优双用，进 F2059）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PoolStats {
    /// 复用命中数。
    pub hits: u64,
    /// 未命中（新建）数。
    pub misses: u64,
    /// 当前在用数。
    pub live_count: usize,
    /// 条目总数。
    pub total_entries: usize,
    /// 峰值在用数。
    pub peak_live: usize,
    /// 在用字节数。
    pub bytes_in_use: u64,
    /// 桶数。
    pub bucket_count: usize,
    /// 当前是否已过拒绝水位。
    pub at_reject_watermark: bool,
}

/// 分配结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AllocResult {
    /// 句柄。
    pub handle: u64,
    /// 是否命中复用（同桶空闲 RT）。
    pub hit: bool,
}

/// 池状态快照（不可变）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RtPoolState {
    /// 条目。
    pub entries: Vec<PoolEntry>,
    /// 桶。
    pub buckets: Vec<Bucket>,
    /// 帧号。
    pub frame: u64,
}

/// 帧边界报告。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct FrameReport {
    /// 帧号。
    pub frame: u64,
    /// 本帧回收（移出池）的条目数。
    pub reaped: usize,
    /// 疑似泄漏。
    pub leaks: Vec<LeakRecord>,
    /// 当前在用数。
    pub live: usize,
}

/**
 * 中间渲染目标池。
 *
 * TS 版是不可变风格（每操作返回新状态）；Rust 版用 `&mut self` 就地变更——
 * 语义等价（每次调用是一个原子步骤），省掉全量克隆：万次申请不回收的
 * 压测语料在不可变风格下是 O(n²) 克隆，压测自己会淹死自己。
 */
#[derive(Debug)]
pub struct RtTargetPool {
    entries: Vec<PoolEntry>,
    buckets: Vec<Bucket>,
    quota: Quota,
    diag_bag: PoolDiagBag,
    leaks: LeakLedger,
    hits: u64,
    misses: u64,
    peak_live: usize,
    frame: u64,
    next_group_id: u64,
}

impl RtTargetPool {
    /// 构造。
    pub fn new(quota: Quota, leak_threshold: u64) -> RtTargetPool {
        RtTargetPool {
            entries: Vec::new(),
            buckets: Vec::new(),
            quota,
            diag_bag: PoolDiagBag::default(),
            leaks: LeakLedger::new(leak_threshold),
            hits: 0,
            misses: 0,
            peak_live: 0,
            frame: 0,
            next_group_id: 1,
        }
    }

    /// 当前状态快照。
    pub fn state(&self) -> RtPoolState {
        RtPoolState {
            entries: self.entries.clone(),
            buckets: self.buckets.clone(),
            frame: self.frame,
        }
    }

    /// 统计。
    pub fn stats(&self) -> PoolStats {
        let live = self.entries.iter().filter(|e| e.in_use).count();
        let bytes = self.bytes_used();
        PoolStats {
            hits: self.hits,
            misses: self.misses,
            live_count: live,
            total_entries: self.entries.len(),
            peak_live: self.peak_live,
            bytes_in_use: bytes,
            bucket_count: self.buckets.len(),
            at_reject_watermark: self.reached_reject_watermark(),
        }
    }

    /// 泄漏账本。
    pub fn leak_ledger(&self) -> &LeakLedger {
        &self.leaks
    }

    /// 累计诊断。
    pub fn diagnostics(&self) -> &[PoolDiagnostic] {
        self.diag_bag.all()
    }

    fn bytes_used(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.in_use)
            .map(|e| rt_bytes(&e.spec))
            .sum()
    }

    /// 是否已达拒绝水位（判据五；双因子按**或**——任一超即拒）。
    fn reached_reject_watermark(&self) -> bool {
        let live = self.entries.iter().filter(|e| e.in_use).count() as u64;
        let by_count = live >= (self.quota.max_entries as u64) * (self.quota.reject_ratio_permille as u64) / 1000;
        let by_bytes = self.bytes_used() >= self.quota.max_bytes * (self.quota.reject_ratio_permille as u64) / 1000;
        by_count || by_bytes
    }

    fn track_peak(&mut self) {
        let live = self.entries.iter().filter(|e| e.in_use).count();
        if live > self.peak_live {
            self.peak_live = live;
        }
    }

    /**
     * 分配一张 RT。
     *
     * 流程：桶内空闲复用 → 配额水位检查 → 新建。
     * 配额拒绝给三要素（当前用量/上限/建议降档），不是裸失败。
     */
    pub fn allocate(&mut self, slot: &str, spec: &RtSpec) -> Result<AllocResult, PoolDiagnostic> {
        if !is_valid_spec(spec) {
            let d = PoolDiagnostic {
                code: PoolDiagCode::QuotaParamInvalid,
                message: format!("RT 规格非法：{}。", bucket_key(spec)),
                hint: "宽高须为正、格式非空；非法规格会落入错误的桶导致跨规格复用。".to_string(),
            };
            self.diag_bag.push(d.code, &d.message, &d.hint);
            return Err(d);
        }
        // 步骤 1：同桶空闲复用（严格键匹配）
        let key = bucket_key(spec);
        if let Some(bi) = self.buckets.iter().position(|b| b.key == key) {
            let free = self.buckets[bi]
                .free_handles
                .iter()
                .find(|h| {
                    self.entries
                        .iter()
                        .any(|e| e.handle == **h && !e.in_use && e.ref_count == 0)
                })
                .copied();
            if let Some(free_handle) = free {
                let entry = self.entries.iter().find(|e| e.handle == free_handle);
                if let Some(entry) = entry {
                    if !same_spec(&entry.spec, spec) {
                        // 关键：桶键相同才复用，否则拒绝跨桶（显性登记而非静默）
                        self.diag_bag.push(
                            PoolDiagCode::BucketKeyMismatch,
                            &format!(
                                "桶 {} 内句柄 {} 规格为 {}，与请求不符，拒绝复用。",
                                key,
                                free_handle,
                                bucket_key(&entry.spec)
                            ),
                            "桶键与实际规格不符说明分桶逻辑有 bug；宁可不复用也不能跨规格。",
                        );
                    } else {
                        if let Some(e) = self.entries.iter_mut().find(|e| e.handle == free_handle) {
                            e.slot = slot.to_string();
                            e.in_use = true;
                            e.ref_count = 1;
                        }
                        self.buckets[bi].free_handles.retain(|h| *h != free_handle);
                        self.hits += 1;
                        self.track_peak();
                        return Ok(AllocResult { handle: free_handle, hit: true });
                    }
                }
            }
        }
        // 步骤 2：配额水位检查（判据五）
        let live = self.entries.iter().filter(|e| e.in_use).count();
        if self.reached_reject_watermark() {
            let bytes = self.bytes_used();
            let msg = format!(
                "中间 RT 池达拒绝水位（在用 {}/{} 张，{}/{} 字节）。",
                live, self.quota.max_entries, bytes, self.quota.max_bytes
            );
            let hint = format!(
                "当前用量 {} 张 {} 字节；上限 {} 张 {} 字节；建议降质量档（F2015）或缩短后处理链。",
                live, bytes, self.quota.max_entries, self.quota.max_bytes
            );
            self.diag_bag.push(PoolDiagCode::QuotaExceeded, &msg, &hint);
            return Err(PoolDiagnostic {
                code: PoolDiagCode::QuotaExceeded,
                message: msg,
                hint,
            });
        }
        if live as u32 >= self.quota.max_entries {
            let msg = format!("中间 RT 池已满：在用达上限 {} 张。", self.quota.max_entries);
            let hint = format!(
                "当前用量 {} 张；上限 {} 张；建议合并相邻效果或降档。",
                live, self.quota.max_entries
            );
            self.diag_bag.push(PoolDiagCode::QuotaExceeded, &msg, &hint);
            return Err(PoolDiagnostic { code: PoolDiagCode::QuotaExceeded, message: msg, hint });
        }
        // 步骤 3：新建条目
        let next_handle = self.entries.iter().map(|e| e.handle + 1).max().unwrap_or(0);
        let entry = PoolEntry {
            handle: next_handle,
            slot: slot.to_string(),
            spec: spec.clone(),
            in_use: true,
            ref_count: 1,
            alias_group: self.next_group_id,
        };
        self.next_group_id += 1;
        self.entries.push(entry);
        if let Some(bi) = self.buckets.iter().position(|b| b.key == key) {
            if !self.buckets[bi].free_handles.contains(&next_handle) {
                // 桶已存在（此前仅拒绝复用），句柄直接在用不进空闲表
            }
        } else {
            self.buckets.push(Bucket {
                key,
                spec: spec.clone(),
                free_handles: Vec::new(),
            });
        }
        self.misses += 1;
        self.track_peak();
        Ok(AllocResult { handle: next_handle, hit: false })
    }

    /// 引用计数 +1（同一物理资源被多逻辑 RT 持有）。
    pub fn retain(&mut self, handle: u64) -> Result<(), PoolDiagnostic> {
        match self.entries.iter_mut().find(|e| e.handle == handle) {
            None => {
                let d = PoolDiagnostic {
                    code: PoolDiagCode::UnknownHandle,
                    message: format!("引用计数失败：句柄 {} 不存在。", handle),
                    hint: "核对句柄来源。".to_string(),
                };
                self.diag_bag.push(d.code, &d.message, &d.hint);
                Err(d)
            }
            Some(e) => {
                e.ref_count += 1;
                Ok(())
            }
        }
    }

    /**
     * 归还 RT（引用计数 -1）。
     * 引用计数未归零 → 拒绝回收并登记 EARLY_RECOLLECT（判据三：防提前回收）。
     */
    pub fn release(&mut self, handle: u64) -> Result<bool, PoolDiagnostic> {
        let idx = match self.entries.iter().position(|e| e.handle == handle) {
            Some(i) => i,
            None => {
                let d = PoolDiagnostic {
                    code: PoolDiagCode::UnknownHandle,
                    message: format!("归还失败：句柄 {} 不存在。", handle),
                    hint: "核对句柄来源；未分配的句柄会掩盖真正的分配缺陷。".to_string(),
                };
                self.diag_bag.push(d.code, &d.message, &d.hint);
                return Err(d);
            }
        };
        let e = &self.entries[idx];
        if !e.in_use {
            let d = PoolDiagnostic {
                code: PoolDiagCode::DoubleRelease,
                message: format!("重复归还句柄 {}。", handle),
                hint: "句柄只能归还一次；重复归还会让在用计数虚低，最终表现为池耗尽。".to_string(),
            };
            self.diag_bag.push(d.code, &d.message, &d.hint);
            return Err(d);
        }
        if e.ref_count > 1 {
            self.entries[idx].ref_count -= 1;
            return Ok(false);
        }
        // 引用计数归零：真正进入空闲桶
        let key = bucket_key(&e.spec);
        self.entries[idx].in_use = false;
        self.entries[idx].ref_count = 0;
        let spec = self.entries[idx].spec.clone();
        if let Some(b) = self.buckets.iter_mut().find(|b| b.key == key) {
            b.free_handles.push(handle);
        } else {
            self.buckets.push(Bucket { key, spec, free_handles: vec![handle] });
        }
        Ok(true)
    }

    /**
     * 帧边界回收（判据三）：推进帧号、清理泄漏账本、回收「本帧已死且无人
     * 持有」的 RT。回收依据是**引用计数**而非「上一帧用过」——别名机制下
     * 同一物理资源可能跨帧存活，仅按帧号回收会误伤。
     */
    pub fn begin_frame(&mut self, dead_handles: &[u64]) -> FrameReport {
        self.frame += 1;
        let mut bag = PoolDiagBag::default();
        // 泄漏观测：帧边界仍 in_use 的句柄视为未回收
        let live_snapshot: Vec<(u64, String)> = self
            .entries
            .iter()
            .filter(|e| e.in_use)
            .map(|e| (e.handle, e.slot.clone()))
            .collect();
        for (h, slot) in &live_snapshot {
            self.leaks.observe(*h, slot, self.frame);
        }
        for e in &self.entries {
            if !e.in_use {
                self.leaks.clear(e.handle);
            }
        }
        let suspected = self.leaks.suspected();
        for r in &suspected {
            bag.push(
                PoolDiagCode::LeakSuspected,
                &format!(
                    "句柄 {}（槽 {}）已连续 {} 帧未回收（首见帧 {}）。",
                    r.handle, r.slot, r.frames, r.first_seen_frame
                ),
                "核对 F2002 执行列表的 release 调用是否配对，或检查是否有效果抛异常跳过了归还。",
            );
            self.diag_bag
                .push(PoolDiagCode::LeakSuspected, &bag.all()[bag.len() - 1].message, &bag.all()[bag.len() - 1].hint);
        }
        // 回收：引用计数归零的 dead 句柄直接释放出池（减少常驻条目）
        let mut reaped = 0usize;
        if !dead_handles.is_empty() {
            let before = self.entries.len();
            self.entries.retain(|e| {
                let dead = dead_handles.contains(&e.handle);
                if dead && !e.in_use && e.ref_count == 0 {
                    reaped += 1;
                    return false;
                }
                true
            });
            // 同步清理桶中的悬空句柄
            for b in self.buckets.iter_mut() {
                b.free_handles
                    .retain(|h| self.entries.iter().any(|e| e.handle == *h));
            }
            let _ = before;
        }
        FrameReport {
            frame: self.frame,
            reaped,
            leaks: suspected,
            live: self.entries.iter().filter(|e| e.in_use).count(),
        }
    }
}

/// 按格式位深估算单张 RT 字节数。
pub fn rt_bytes(spec: &RtSpec) -> u64 {
    let bpp = bytes_per_pixel(&spec.format) as u64;
    spec.width as u64 * spec.height as u64 * bpp
}

/// 每像素字节数（未知格式按 8 字节保守估计——宁可高估占用触发降档，
/// 也不要低估后 OOM）。
pub fn bytes_per_pixel(format: &str) -> u32 {
    match format {
        "RGBA16F" => 8,
        "RGBA8" | "R11G11B10F" => 4,
        _ => 8,
    }
}

// ===========================================================================
// 六、与 F2002 编译产物对接
// ===========================================================================

/// 由 F2002 编译产物推导存活区间。
///
/// 无执行序信息时退化为按分配序推断：birth=下标，death=最后一次被读的下标。
/// `reads_of(slot)` 给出该槽的读者槽序列。
pub fn intervals_from_plan(
    plan: &vek02_dagexec::RtAllocationPlan,
    reads_of: &dyn Fn(&str) -> Vec<String>,
) -> Vec<LiveInterval> {
    let mut last_read: Vec<(String, usize)> = Vec::new();
    let slots: Vec<String> = plan.allocations.iter().map(|(_, s)| s.slot.clone()).collect();
    for (i, slot) in slots.iter().enumerate() {
        for r in reads_of(slot) {
            if let Some(at) = slots.iter().position(|s| s == &r) {
                if at > i {
                    if let Some(lr) = last_read.iter_mut().find(|(s, _)| s == slot) {
                        if at > lr.1 {
                            lr.1 = at;
                        }
                    } else {
                        last_read.push((slot.clone(), at));
                    }
                }
            }
        }
    }
    plan.allocations
        .iter()
        .enumerate()
        .map(|(i, (_, s))| {
            let death = last_read
                .iter()
                .find(|(sl, _)| sl == &s.slot)
                .map(|(_, d)| *d as u32)
                .unwrap_or(i as u32);
            LiveInterval { slot: s.slot.clone(), birth: i as u32, death }
        })
        .collect()
}

/// 池与编译计划的一致性核对（计划要求的每个槽都必须在池中有在用条目）。
pub fn verify_plan_against_pool(
    plan: &vek02_dagexec::RtAllocationPlan,
    pool: &RtPoolState,
) -> Vec<PoolDiagnostic> {
    let mut out: Vec<PoolDiagnostic> = Vec::new();
    for (effect_id, spec) in &plan.allocations {
        let live = pool
            .entries
            .iter()
            .any(|e| e.in_use && e.slot == spec.slot);
        if !live {
            out.push(PoolDiagnostic {
                code: PoolDiagCode::PlanPoolMismatch,
                message: format!("分配计划要求槽 {}（效果 {}），但池中无在用条目。", spec.slot, effect_id),
                hint: "F2002 执行器须先向池申请再执行；未申请直接执行会读到未初始化 RT。".to_string(),
            });
        }
    }
    out
}

// ===========================================================================
// 七、域自检（TS 版 runSelfCheck 判据组的 Rust 移植）
// ===========================================================================

/// VE-F2003 迁移件自检（TS 版 selfCheckPooling/selfCheckReclaim/
/// selfCheckLeak/selfCheckQuota/selfCheckAlias/selfCheckPlanBridge 判据组的
/// 移植；随模块常驻编译）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    fn spec_a() -> RtSpec {
        RtSpec { width: 1920, height: 1080, format: "RGBA16F".to_string() }
    }
    fn spec_b() -> RtSpec {
        RtSpec { width: 1920, height: 1080, format: "RGBA8".to_string() }
    }
    fn spec_c() -> RtSpec {
        RtSpec { width: 1280, height: 720, format: "RGBA16F".to_string() }
    }
    fn mk_quota(max_entries: u32, max_bytes: u64) -> Quota {
        Quota { max_entries, max_bytes, reject_ratio_permille: 950 }
    }

    /// 跑全部判据。
    pub fn run_vek03_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vek03");

        // -- P03-POOL-01 桶化复用：同规格命中、跨规格绝不跨桶 -----------------
        {
            let mut p = RtTargetPool::new(mk_quota(8, 1 << 30), 3);
            let a1 = p.allocate("s1", &spec_a());
            let a2 = p.allocate("s2", &spec_a());
            let b1 = p.allocate("b1", &spec_b());
            let c1 = p.allocate("c1", &spec_c());
            let base_ok = matches!((&a1, &a2, &b1, &c1), (Ok(x), Ok(y), Ok(z), Ok(w))
                if !x.hit && !y.hit && !z.hit && !w.hit);
            // 归还 a 后同规格再申请必命中；不同规格永不命中他桶
            let released = a1.as_ref().map(|_| true).unwrap_or(false);
            let h = a1.as_ref().map(|r| r.handle).unwrap_or(0);
            let _ = released;
            let rel = p.release(h);
            let a3 = p.allocate("s3", &spec_a());
            let reuse_ok = matches!((&rel, &a3), (Ok(_), Ok(r)) if r.hit);
            s.add(
                "P03-POOL-01 桶化复用：同规格归还后命中、异规格各开新桶（桶键=尺寸×格式严格匹配）",
                base_ok && reuse_ok,
                "TS 版 selfCheckPooling 判据一移植；跨规格复用=读到错误格式的像素",
            );
        }

        // -- P03-RECLAIM-01 重复归还/未知句柄/引用计数三防 --------------------
        {
            let mut p = RtTargetPool::new(mk_quota(8, 1 << 30), 3);
            let a = p.allocate("s1", &spec_a());
            let h = a.as_ref().map(|r| r.handle).unwrap_or(0);
            let _ = a;
            // retain 后第一次 release 只减计数不进空闲桶
            let _ = p.retain(h);
            let r1 = p.release(h);
            let partial = matches!(&r1, Ok(true) if false) || matches!(&r1, Ok(false));
            let r2 = p.release(h);
            let final_ok = matches!(&r2, Ok(true));
            let r3 = p.release(h);
            let dup = matches!(&r3, Err(d) if d.code == PoolDiagCode::DoubleRelease);
            let r4 = p.release(9999);
            let unknown = matches!(&r4, Err(d) if d.code == PoolDiagCode::UnknownHandle);
            s.add(
                "P03-RECLAIM-01 引用计数两段归还、重复归还与未知句柄各自专属码拒绝",
                partial && final_ok && dup && unknown,
                "TS 版 selfCheckReclaim 判据三移植；重复归还让在用计数虚低→池耗尽",
            );
        }

        // -- P03-LEAK-01 泄漏账本：连续帧累计、回收清账、阈值判定 -------------
        {
            let mut p = RtTargetPool::new(mk_quota(8, 1 << 30), 3);
            let a = p.allocate("leaky", &spec_a());
            let h = a.as_ref().map(|r| r.handle).unwrap_or(0);
            let _ = a;
            let mut seen = 0u64;
            for _ in 0..3 {
                let rep = p.begin_frame(&[]);
                seen = seen.max(rep.leaks.len() as u64);
            }
            let flagged = seen >= 1 && p.leak_ledger().all().iter().any(|r| r.handle == h && r.frames >= 3);
            // 归还后清账
            let _ = p.release(h);
            let rep2 = p.begin_frame(&[]);
            let cleared = p.leak_ledger().all().iter().all(|r| r.handle != h);
            let _ = rep2;
            s.add(
                "P03-LEAK-01 泄漏账本：连续 3 帧未回收达阈、归还后清账（泄漏不静默）",
                flagged && cleared,
                "TS 版 selfCheckLeak 判据四移植；账本漏记=泄漏静默→池耗尽",
            );
        }

        // -- P03-QUOTA-01 配额双因子：张数水位 + 字节水位各自触发 -------------
        {
            // 张数：max 4，950‰ 水位=floor(3.8)=3 → 第 4 张拒
            let mut p = RtTargetPool::new(mk_quota(4, 1 << 30), 3);
            let mut count_rejects = 0u64;
            for i in 0..6 {
                let spec = RtSpec { width: 64 + i as u32, height: 64, format: "RGBA8".to_string() };
                if p.allocate(&format!("s{}", i), &spec).is_err() {
                    count_rejects += 1;
                }
            }
            let count_ok = count_rejects == 3;
            // 字节：max_bytes 只够 1 张的一半 → 第 2 张拒（张数远未满）
            let mut pb = RtTargetPool::new(mk_quota(100, 1920 * 1080 * 8), 3);
            let big = spec_a();
            let _ = pb.allocate("t0", &big);
            let by_bytes = matches!(pb.allocate("t1", &big), Err(d) if d.code == PoolDiagCode::QuotaExceeded);
            // 非法规格
            let mut ps = RtTargetPool::new(mk_quota(4, 1 << 30), 3);
            let bad = RtSpec { width: 0, height: 100, format: "RGBA8".to_string() };
            let invalid = matches!(ps.allocate("bad", &bad), Err(d) if d.code == PoolDiagCode::QuotaParamInvalid);
            s.add(
                "P03-QUOTA-01 配额双因子各自触发（张数/字节）+ 非法规格专属码拒绝",
                count_ok && by_bytes && invalid,
                "TS 版 selfCheckQuota 判据五移植；只试一条水位的防线是半条防线",
            );
        }

        // -- P03-ALIAS-01 区间别名：线性复用省资源、相接不复用（闭区间）-------
        {
            // 线性链 s0[0,0] s1[1,1] s2[2,2]：两两不重叠 → 全并入一组
            let linear = vec![
                LiveInterval { slot: "s0".to_string(), birth: 0, death: 0 },
                LiveInterval { slot: "s1".to_string(), birth: 1, death: 1 },
                LiveInterval { slot: "s2".to_string(), birth: 2, death: 2 },
            ];
            let la = analyze_aliases(&linear);
            // 相接：[0,1] 与 [1,2] 闭区间在 1 处相接——不可别名
            let touching = vec![
                LiveInterval { slot: "t0".to_string(), birth: 0, death: 1 },
                LiveInterval { slot: "t1".to_string(), birth: 1, death: 2 },
            ];
            let ta = analyze_aliases(&touching);
            let saved_ok = la.saved_count == 2 && ta.physical_count == 2;
            // 非法区间入 invalid 清单
            let bad = vec![LiveInterval { slot: "x".to_string(), birth: 5, death: 3 }];
            let ba = analyze_aliases(&bad);
            let invalid_ok = ba.invalid_intervals.len() == 1;
            s.add(
                "P03-ALIAS-01 区间别名：不相交复用省 2、闭区间相接不复用、非法区间显性入清单",
                saved_ok && invalid_ok,
                "TS 版 selfCheckAlias 判据二移植；相接误别名=第 N 步既写又读的竞争",
            );
        }

        // -- P03-BRIDGE-01 计划对接：池缺槽报 PlanPoolMismatch ----------------
        {
            use super::vek02_dagexec::RtAllocationPlan;
            use super::vek02_dagexec::RtSpec as DagRtSpec;
            let plan = RtAllocationPlan {
                allocations: vec![(
                    "eff".to_string(),
                    DagRtSpec {
                        slot: "sX".to_string(),
                        width: 64,
                        height: 64,
                        format: "RGBA8".to_string(),
                    },
                )],
                required_slots: 1,
            };
            let mut p = RtTargetPool::new(mk_quota(4, 1 << 30), 3);
            let other = spec_a();
            let _ = p.allocate("sA", &other);
            let missing = verify_plan_against_pool(&plan, &p.state());
            let hit_missing = missing.iter().any(|d| d.code == PoolDiagCode::PlanPoolMismatch);
            // 申请到位后核对通过
            let want = RtSpec { width: 64, height: 64, format: "RGBA8".to_string() };
            let _ = p.allocate("sX", &want);
            let now_ok = verify_plan_against_pool(&plan, &p.state()).is_empty();
            s.add(
                "P03-BRIDGE-01 计划与池一致性核对：缺槽显性报、补申请后通过",
                hit_missing && now_ok,
                "TS 版 selfCheckPlanBridge 判据移植；未申请直接执行=读到未初始化 RT",
            );
        }

        // -- P03-REAP-01 帧边界回收：dead+空闲+零计数才出池，悬空句柄清桶 -----
        {
            let mut p = RtTargetPool::new(mk_quota(8, 1 << 30), 3);
            let a = p.allocate("s1", &spec_a());
            let h = a.as_ref().map(|r| r.handle).unwrap_or(0);
            let _ = a;
            let _ = p.release(h);
            let rep = p.begin_frame(&[h]);
            let reaped = rep.reaped == 1;
            let gone = !p.state().entries.iter().any(|e| e.handle == h);
            let bucket_clean = p.state().buckets.iter().all(|b| !b.free_handles.contains(&h));
            s.add(
                "P03-REAP-01 帧边界回收：空闲零计数句柄出池且桶内悬空句柄同步清理",
                reaped && gone && bucket_clean,
                "回收依据是引用计数而非帧号——别名机制下跨帧存活的物理资源不能按帧误伤",
            );
        }

        s
    }
}
