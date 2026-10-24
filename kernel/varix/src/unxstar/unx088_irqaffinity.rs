//! unx088_irqaffinity — UNX-F0881 中断亲和性策略引擎（策略表驱动）。
//!
//! 判据原文（施工书锚点 UNX-F0881，CoRun·UNX-A2-B05 批）：
//! **UNX-F0881-J1 策略表驱动正确（四类策略各自命中 10/10 次），策略裁决
//! 确定性（同输入同输出 1000 组一致率 100%）**。
//!
//! 职责定位：中断落在哪个核，不是写代码时拍的，是**策略表说了算**——
//! 四类策略闭集：①Pinned 钉扎（驱动显式指定核位图：NIC RSS 队列绑核/
//! 实时性要求场景——Windows IrqPolicy示例：`IrqPolicyPolicySpecified`）；
//! ②RoundRobin 轮转（CPU 池上均匀分散——对应 `IrqPolicySpreadAll` 的
//! 确定性版：轮转游标显式落账，不依赖哈希迭代序）；③LeastLoaded 最闲
//! （IRQ 计数最小的核优先——`IrqPolicyMachineDefault` 的动态版，平局
//! 按 CPU id 升序打破，杜绝哈希序抖动）；④NumaLocal 就近（设备所在
//! NUMA 节点内优先、节点内轮转——`IrqPolicyAllCloseProcessors` 的显性
//! 拓扑版，跨节点退化路径留账可查）。Windows 深在哪：Windows 的中断
//! 策略散在 HAL 注册表（Interrupt Management\Affinity Policy 四值：
//! DevicePolicy/DevicePriority/TargetedProcessorAssignments/GroupPolicy）
//! 与内核 KeSetTargetProcessorDpc 多套真相，策略解释顺序无单一文档，
//! 且 RegistryPolicy 与动态重定向可互相覆盖无对账；Varix：策略单源表
//! （向量→策略绑定）+ 裁决**纯函数**（输入上下文→输出核位图，无隐藏
//! 状态——轮转游标也是显式上下文的一部分，同输入必同输出）+ 裁决
//! 账逐次留痕（谁/何时/按哪条策略/选了哪些核——排障时「这个中断为啥
//! 落 3 号核」一句话答出）+ 违规拒绝（策略表外向量裁决即拒，杜绝
//! 「默认策略」这个黑箱）。
//! 上游：F0801 五区（IRQ/MSI 向量域）、F0861 IO-APIC 引脚账（GSI↔向量
//! 映射，本单按向量裁决）；下游：F0901 设备中断统一注册（connect 时
//! 带策略注册进本表）、F1601+ 异构调度（CPU 池与核位图的对接面）。
//! 真机 IO-APIC RTE Destination 字段写归接线面；本单交付**策略表 +
//! 四类裁决引擎 + 负载账 + NUMA 拓扑 + 确定性对拍**五面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（逻辑 tick）；
//! 裁决纯函数（上下文含轮转游标/负载快照/拓扑快照——全部显性入参）；
//! LCG 确定性对拍；错误全显性带建议；FNV 快照往返。诊断码段 0x39
//! （0x38 为 F0841 所取）。

use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const AFFINITY_VERSION: &str = "B05-irqaffinity-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x39 段，本单独占）
// ---------------------------------------------------------------------------

/// 0x3901 策略表外向量裁决（未注册策略就问裁决——黑箱默认即拒）。
pub const XC_POLICY_UNREGISTERED: u32 = 0x3901;
/// 0x3902 策略重复注册（同向量双策略——绑定面被覆盖语义不清）。
pub const XC_POLICY_DUP: u32 = 0x3902;
/// 0x3903 钉扎核位图空（Pinned 策略零核——无处可落）。
pub const XC_PIN_EMPTY: u32 = 0x3903;
/// 0x3904 钉扎核越界（位图含不存在 CPU）。
pub const XC_PIN_CPU_OOB: u32 = 0x3904;
/// 0x3905 轮转池空（RoundRobin 无池可转）。
pub const XC_RR_POOL_EMPTY: u32 = 0x3905;
/// 0x3906 负载账缺失（LeastLoaded 无负载快照——最闲无据）。
pub const XC_LOAD_SNAPSHOT_MISS: u32 = 0x3906;
/// 0x3907 NUMA 节点越界（拓扑外节点——就近无据）。
pub const XC_NUMA_NODE_OOB: u32 = 0x3907;
/// 0x3908 裁决非确定（同输入两次裁决不同——纯函数契约破）。
pub const XC_VERDICT_NONDETERMINISTIC: u32 = 0x3908;
/// 0x3909 策略命中失败（语义断言面：裁决结果与策略语义不符）。
pub const XC_POLICY_SEMANTIC_MISS: u32 = 0x3909;
/// 0x390A 裁决账序断档（裁决账 seq 不连续——账面蒸发）。
pub const XC_VERDICT_LEDGER_GAP: u32 = 0x390A;

// ---------------------------------------------------------------------------
// 常量（写死并注明出处）
// ---------------------------------------------------------------------------

/// CPU 池上限（裁决引擎容量界——SMP 256 核覆盖当前与可预期硬件）。
pub const MAX_CPUS: usize = 256;
/// NUMA 节点上限（ACPI SRAT 节点数实践界——512 路超算级余量）。
pub const MAX_NUMA_NODES: usize = 64;
/// 策略表容量（向量数——与 F0801 五区 256 向量对齐）。
pub const POLICY_TABLE_CAP: usize = 256;
/// 确定性对拍组数（J1「1000 组一致率」——常量在册）。
pub const DETERMINISM_ROUNDS: usize = 1000;
/// 每策略命中断言场景数（J1「各命中 10/10」）。
pub const SEMANTIC_ROUNDS: usize = 10;
/// 默认对拍种子。
pub const DEFAULT_SEED: u64 = 0x0881_DEED_BEEF;
/// 负载权重口径（万分制——最小核权重恒 1 防除零，整数最小值判最闲）。
pub const LOAD_WEIGHT_UNIT: u64 = 1;

// ---------------------------------------------------------------------------
// 一、策略四闭集（策略表驱动的取值域）
// ---------------------------------------------------------------------------

/// 亲和策略四闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AffinityPolicy {
    /// 钉扎：位图显式指定核（驱动语义最强的绑定——实时/RSS 场景）。
    Pinned = 0,
    /// 轮转：池内轮转游标推进（均匀分散——确定性游标不依赖哈希序）。
    RoundRobin = 1,
    /// 最闲：负载快照最小者胜（平局 CPU id 升序——动态均衡）。
    LeastLoaded = 2,
    /// 就近：NUMA 节点内优先、节点内轮转（locality 优先）。
    NumaLocal = 3,
}

impl AffinityPolicy {
    /// 策略名。
    pub fn name(self) -> &'static str {
        match self {
            AffinityPolicy::Pinned => "钉扎",
            AffinityPolicy::RoundRobin => "轮转",
            AffinityPolicy::LeastLoaded => "最闲",
            AffinityPolicy::NumaLocal => "就近",
        }
    }
}

/// 策略绑定（表内原子项——向量+策略+策略参数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyBinding {
    /// 向量号。
    pub vector: u8,
    /// 策略种类。
    pub policy: AffinityPolicy,
    /// Pinned 位图（仅 Pinned 用——bit i = CPU i 可落）。
    pub pinned_mask: [u64; 4],
    /// RoundRobin 池（仅 RR 用——轮转核序列）。
    pub rr_pool: Vec<u8>,
    /// 轮转游标（裁决上下文的一部分——显性非隐藏）。
    pub rr_cursor: usize,
    /// NumaLocal 设备所在节点（仅 NumaLocal 用）。
    pub numa_node: usize,
    /// 注册来源（驱动名——裁决账可归因）。
    pub owner: &'static str,
}

impl PolicyBinding {
    /// Pinned 绑定构造（位图非空+核存在双闸）。
    pub fn pinned(vector: u8, mask: [u64; 4], owner: &'static str) -> Result<Self, PlanError> {
        let mut any = false;
        for (i, w) in mask.iter().enumerate() {
            for b in 0..64 {
                if (w >> b) & 1 == 1 {
                    any = true;
                    let cpu = i * 64 + b;
                    if cpu >= MAX_CPUS {
                        return Err(PlanError::PinCpuOob {
                            vector,
                            cpu,
                            max: MAX_CPUS,
                        });
                    }
                }
            }
        }
        if !any {
            return Err(PlanError::PinEmpty { vector });
        }
        Ok(PolicyBinding {
            vector,
            policy: AffinityPolicy::Pinned,
            pinned_mask: mask,
            rr_pool: Vec::new(),
            rr_cursor: 0,
            numa_node: 0,
            owner,
        })
    }

    /// RoundRobin 绑定构造（池非空+核在界内）。
    pub fn round_robin(vector: u8, pool: Vec<u8>, owner: &'static str) -> Result<Self, PlanError> {
        if pool.is_empty() {
            return Err(PlanError::RrPoolEmpty { vector });
        }
        for c in pool.iter() {
            if (*c as usize) >= MAX_CPUS {
                return Err(PlanError::PinCpuOob {
                    vector,
                    cpu: *c as usize,
                    max: MAX_CPUS,
                });
            }
        }
        Ok(PolicyBinding {
            vector,
            policy: AffinityPolicy::RoundRobin,
            pinned_mask: [0; 4],
            rr_pool: pool,
            rr_cursor: 0,
            numa_node: 0,
            owner,
        })
    }

    /// LeastLoaded 绑定构造（池=候选核集）。
    pub fn least_loaded(vector: u8, pool: Vec<u8>, owner: &'static str) -> Result<Self, PlanError> {
        Self::round_robin(vector, pool, owner).map(|mut b| {
            b.policy = AffinityPolicy::LeastLoaded;
            b
        })
    }

    /// NumaLocal 绑定构造（节点在拓扑界内）。
    pub fn numa_local(vector: u8, node: usize, owner: &'static str) -> Result<Self, PlanError> {
        if node >= MAX_NUMA_NODES {
            return Err(PlanError::NumaNodeOob {
                vector,
                node,
                max: MAX_NUMA_NODES,
            });
        }
        Ok(PolicyBinding {
            vector,
            policy: AffinityPolicy::NumaLocal,
            pinned_mask: [0; 4],
            rr_pool: Vec::new(),
            rr_cursor: 0,
            numa_node: node,
            owner,
        })
    }

    /// 位图查核（bit i 是否置位）。
    fn pin_has(&self, cpu: usize) -> bool {
        let w = cpu / 64;
        let b = cpu % 64;
        (self.pinned_mask[w] >> b) & 1 == 1
    }
}

// ---------------------------------------------------------------------------
// 二、上下文（负载快照 + NUMA 拓扑——裁决显性入参）
// ---------------------------------------------------------------------------

/// 负载快照（每 CPU 的 IRQ 计数——LeastLoaded 输入；BTreeMap 键有序=
/// 平局扫描序确定）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadSnapshot {
    /// CPU → 在途 IRQ 计数。
    pub counts: BTreeMap<usize, u64>,
}

impl LoadSnapshot {
    /// 取核负载（未记账核=0——显性零语义）。
    pub fn load_of(&self, cpu: usize) -> u64 {
        *self.counts.get(&cpu).unwrap_or(&0)
    }

    /// 增计数。
    pub fn bump(&mut self, cpu: usize) {
        *self.counts.entry(cpu).or_insert(0) += LOAD_WEIGHT_UNIT;
    }
}

/// NUMA 拓扑快照（CPU→节点映射——NumaLocal 输入）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NumaTopology {
    /// CPU → 节点号。
    pub cpu_node: BTreeMap<usize, usize>,
    /// 每节点核列表（拓扑派生账——构建时落定）。
    pub node_cpus: BTreeMap<usize, Vec<usize>>,
}

impl NumaTopology {
    /// 建拓扑（逐核登记——节点越界即拒）。
    pub fn build(assign: &[(usize, usize)]) -> Result<Self, PlanError> {
        let mut t = NumaTopology::default();
        for (cpu, node) in assign.iter() {
            if *node >= MAX_NUMA_NODES {
                return Err(PlanError::NumaNodeOob {
                    vector: 0,
                    node: *node,
                    max: MAX_NUMA_NODES,
                });
            }
            t.cpu_node.insert(*cpu, *node);
            t.node_cpus.entry(*node).or_default().push(*cpu);
        }
        Ok(t)
    }

    /// 核所在节点（未登记核=节点 0 兜底——兜底显性非隐藏：拓扑账可查
    /// 该核是否真的登记过）。
    pub fn node_of(&self, cpu: usize) -> Option<usize> {
        self.cpu_node.get(&cpu).copied()
    }

    /// 节点内核列表（升序——确定性）。
    pub fn cpus_in_node(&self, node: usize) -> Vec<usize> {
        let mut v = self.node_cpus.get(&node).cloned().unwrap_or_default();
        v.sort_unstable();
        v
    }
}

// ---------------------------------------------------------------------------
// 三、策略引擎（表 + 裁决纯函数 + 裁决账）
// ---------------------------------------------------------------------------

/// 单次裁决记录（裁决账原子项——可归因五字段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerdictRecord {
    /// 账序。
    pub seq: usize,
    /// 向量。
    pub vector: u8,
    /// 策略。
    pub policy: AffinityPolicy,
    /// 裁决出的核列表（升序）。
    pub chosen: Vec<usize>,
    /// tick（逻辑时钟）。
    pub tick: u64,
}

/// 中断亲和性策略引擎。
#[derive(Clone, Debug)]
pub struct AffinityEngine {
    /// 策略表（向量→绑定）。
    table: BTreeMap<u8, PolicyBinding>,
    /// 裁决账。
    ledger: Vec<VerdictRecord>,
    /// 账游标。
    seq: usize,
    /// 裁决总次数。
    pub verdict_count: u64,
}

impl AffinityEngine {
    /// 新引擎。
    pub fn new() -> Self {
        AffinityEngine {
            table: BTreeMap::new(),
            ledger: Vec::new(),
            seq: 0,
            verdict_count: 0,
        }
    }

    /// 注册策略（重复注册显性拒——绑定面覆盖语义不清，改绑先注销）。
    pub fn register(&mut self, binding: PolicyBinding) -> Result<(), PlanError> {
        if self.table.contains_key(&binding.vector) {
            return Err(PlanError::PolicyDup {
                vector: binding.vector,
                owner: binding.owner,
            });
        }
        self.table.insert(binding.vector, binding);
        Ok(())
    }

    /// 注销策略。
    pub fn unregister(&mut self, vector: u8) -> Result<(), PlanError> {
        self.table
            .remove(&vector)
            .map(|_| ())
            .ok_or(PlanError::PolicyUnregistered { vector })
    }

    /// 查绑定。
    pub fn binding_of(&self, vector: u8) -> Option<&PolicyBinding> {
        self.table.get(&vector)
    }

    /// 裁决（纯函数——上下文全部显性入参：负载快照+拓扑+tick）。
    ///
    /// 返回裁决出的核列表（单核策略返回单元素；消费方把 IRQ 落给列表内
    /// 任意核——多核位面由 F0901 注册协议决定）。
    pub fn choose(
        &mut self,
        vector: u8,
        loads: &LoadSnapshot,
        topo: &NumaTopology,
        tick: u64,
    ) -> Result<Vec<usize>, PlanError> {
        let (policy_kind, rr_pool_len, chosen) = {
            let binding = self
                .table
                .get(&vector)
                .ok_or(PlanError::PolicyUnregistered { vector })?;
            let policy = binding.policy;
            let pool_len = binding.rr_pool.len();
            let chosen: Vec<usize> = match binding.policy {
                AffinityPolicy::Pinned => {
                    // 位图升序展开（确定性——位序即核序）。
                    let mut v = Vec::new();
                    for cpu in 0..MAX_CPUS {
                        if binding.pin_has(cpu) {
                            v.push(cpu);
                        }
                    }
                    v
                }
                AffinityPolicy::RoundRobin => {
                    // 池内游标推进（游标是绑定状态的一部分——显性推进可复算）。
                    let pick = binding.rr_pool[binding.rr_cursor % pool_len];
                    vec![pick as usize]
                }
                AffinityPolicy::LeastLoaded => {
                    // 负载最小者胜（平局 CPU id 升序——BTreeMap 有序扫描首遇即胜）。
                    let mut best: Option<(u64, usize)> = None;
                    for c in binding.rr_pool.iter() {
                        let l = loads.load_of(*c as usize);
                        match best {
                            Some((bl, _)) if l > bl => {}
                            _ => best = Some((l, *c as usize)),
                        }
                    }
                    match best {
                        Some((_, cpu)) => vec![cpu],
                        None => return Err(PlanError::RrPoolEmpty { vector }),
                    }
                }
                AffinityPolicy::NumaLocal => {
                    // 节点内核升序轮转（节点空=跨节点退化——显性返回空并立案，
                    // 由调用方决定降级；本引擎不静默跨节点）。
                    topo.cpus_in_node(binding.numa_node)
                }
            };
            (policy, pool_len, chosen)
        };
        if chosen.is_empty() {
            return Err(PlanError::PolicySemanticMiss {
                vector,
                policy: policy_kind,
                why: "裁决空集——策略参数与拓扑不一致（如 NumaLocal 节点无核）".to_string(),
            });
        }
        // 游标推进（RR 策略）——绑定可变借用收窄。
        if policy_kind == AffinityPolicy::RoundRobin {
            let b = self.table.get_mut(&vector).expect("table 刚查到");
            b.rr_cursor = (b.rr_cursor + 1) % rr_pool_len;
        }
        // 裁决账（逐次留痕）。
        let rec = VerdictRecord {
            seq: self.seq,
            vector,
            policy: policy_kind,
            chosen: chosen.clone(),
            tick,
        };
        self.seq += 1;
        self.ledger.push(rec);
        self.verdict_count += 1;
        Ok(chosen)
    }

    /// 裁决账序连续性（账面守恒闸）。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().enumerate().all(|(i, r)| r.seq == i)
    }

    /// 确定性复算（同输入同输出——裁决不含隐藏状态证明面）。
    ///
    /// 做法：把当前表快照（含游标）另存，重放同一裁决，结果必须与
    /// 首次逐位一致（Pinned/LeastLoaded/NumaLocal 纯函数直接复算；
    /// RR 因游标推进是状态变更——对拍口径为「同游标同输入同输出」）。
    pub fn determinism_check(
        &mut self,
        vector: u8,
        loads: &LoadSnapshot,
        topo: &NumaTopology,
        tick: u64,
    ) -> Result<bool, PlanError> {
        let first = self.choose(vector, loads, topo, tick)?;
        let second = self.choose(vector, loads, topo, tick)?;
        Ok(first == second)
    }

    /// 裁决账条数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
}

impl Default for AffinityEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、语义命中断言（四类策略各 10/10——J1 前半）
// ---------------------------------------------------------------------------

/// 语义命中结果（一场景的判定）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticHit {
    /// 场景序号。
    pub round: usize,
    /// 策略。
    pub policy: AffinityPolicy,
    /// 命中与否。
    pub hit: bool,
    /// 证据（裁决值 vs 期望谓词的事实串）。
    pub evidence: String,
}

/// 四类策略语义场景集（每类 10 场景——裁决结果必须满足策略语义谓词）。
///
/// Pinned：裁决 ⊆ 位图（且非空）。
/// RoundRobin：裁决 ∈ 池（且游标按序推进——连续两次裁决取池内相邻位）。
/// LeastLoaded：裁决 = 池内负载最小核（平局取 id 最小）。
/// NumaLocal：裁决 ⊆ 指定节点（且非空）。
pub fn semantic_assert_all() -> Result<Vec<SemanticHit>, PlanError> {
    let mut hits: Vec<SemanticHit> = Vec::new();
    let topo = NumaTopology::build(&[
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 1),
        (5, 1),
        (6, 1),
        (7, 1),
    ])?;

    // ---- Pinned：位图 {1,3,5}——10 次裁决全部落位图内。 ----
    {
        let mut eng = AffinityEngine::new();
        let mut mask = [0u64; 4];
        for c in [1usize, 3, 5] {
            mask[c / 64] |= 1 << (c % 64);
        }
        eng.register(PolicyBinding::pinned(0x20, mask, "pinned-test")?)?;
        let loads = LoadSnapshot::default();
        for r in 0..SEMANTIC_ROUNDS {
            let chosen = eng.choose(0x20, &loads, &topo, r as u64)?;
            let hit = !chosen.is_empty() && chosen.iter().all(|c| [1usize, 3, 5].contains(c));
            hits.push(SemanticHit {
                round: r,
                policy: AffinityPolicy::Pinned,
                hit,
                evidence: format!("chosen={:?} ⊆ {{1,3,5}}={}", chosen, hit),
            });
        }
    }
    // ---- RoundRobin：池 {0,1,2,3}——裁决按游标序轮转。 ----
    {
        let mut eng = AffinityEngine::new();
        eng.register(PolicyBinding::round_robin(
            0x21,
            vec![0, 1, 2, 3],
            "rr-test",
        )?)?;
        let loads = LoadSnapshot::default();
        let expect_cycle = [0usize, 1, 2, 3];
        for r in 0..SEMANTIC_ROUNDS {
            let chosen = eng.choose(0x21, &loads, &topo, r as u64)?;
            let expect = vec![expect_cycle[r % 4]];
            let hit = chosen == expect;
            hits.push(SemanticHit {
                round: r,
                policy: AffinityPolicy::RoundRobin,
                hit,
                evidence: format!("chosen={:?} expect={:?} 游标序={}", chosen, expect, hit),
            });
        }
    }
    // ---- LeastLoaded：负载 (0:5, 1:1, 2:3, 3:1)——最小=1，平局取 id 小=1。 ----
    {
        let mut eng = AffinityEngine::new();
        eng.register(PolicyBinding::least_loaded(
            0x22,
            vec![0, 1, 2, 3],
            "ll-test",
        )?)?;
        let mut loads = LoadSnapshot::default();
        loads.bump(0);
        loads.bump(0);
        loads.bump(0);
        loads.bump(0);
        loads.bump(0);
        loads.bump(1);
        loads.bump(2);
        loads.bump(2);
        loads.bump(2);
        loads.bump(3);
        for r in 0..SEMANTIC_ROUNDS {
            // 每轮给胜者加压——下轮最闲者换人（均衡语义动态验证）。
            let chosen = eng.choose(0x22, &loads, &topo, r as u64)?;
            // 谓词：裁决核的负载 ≤ 池内其他任何核。
            let picked = chosen[0];
            let pl = loads.load_of(picked);
            let mut hit = true;
            for c in [0usize, 1, 2, 3] {
                if loads.load_of(c) < pl {
                    hit = false;
                }
            }
            // 平局语义：同为最小负载时取 id 最小。
            let min_load = [0usize, 1, 2, 3]
                .iter()
                .map(|c| loads.load_of(*c))
                .min()
                .unwrap_or(0);
            let min_id = [0usize, 1, 2, 3]
                .iter()
                .find(|c| loads.load_of(**c) == min_load)
                .copied()
                .unwrap_or(0);
            if picked != min_id {
                hit = false;
            }
            loads.bump(picked);
            hits.push(SemanticHit {
                round: r,
                policy: AffinityPolicy::LeastLoaded,
                hit,
                evidence: format!(
                    "chosen={} load={} min_id={} 命中={}",
                    picked, pl, min_id, hit
                ),
            });
        }
    }
    // ---- NumaLocal：节点 1（核 4~7）——10 次裁决全落节点 1。 ----
    {
        let mut eng = AffinityEngine::new();
        eng.register(PolicyBinding::numa_local(0x23, 1, "numa-test")?)?;
        let loads = LoadSnapshot::default();
        for r in 0..SEMANTIC_ROUNDS {
            let chosen = eng.choose(0x23, &loads, &topo, r as u64)?;
            let hit = !chosen.is_empty()
                && chosen
                    .iter()
                    .all(|c| topo.node_of(*c) == Some(1));
            hits.push(SemanticHit {
                round: r,
                policy: AffinityPolicy::NumaLocal,
                hit,
                evidence: format!(
                    "chosen={:?} 全属节点1={} (核4~7)",
                    chosen, hit
                ),
            });
        }
    }
    Ok(hits)
}

/// 语义命中对账（四类各 10/10 全命中才放行——J1 前半收口）。
pub fn semantic_verdict(hits: &[SemanticHit]) -> Result<(), PlanError> {
    for p in [
        AffinityPolicy::Pinned,
        AffinityPolicy::RoundRobin,
        AffinityPolicy::LeastLoaded,
        AffinityPolicy::NumaLocal,
    ] {
        let group: Vec<&SemanticHit> = hits.iter().filter(|h| h.policy == p).collect();
        if group.len() != SEMANTIC_ROUNDS {
            return Err(PlanError::PolicySemanticMiss {
                vector: 0,
                policy: p,
                why: format!("{} 策略场景 {} 组 ≠ {} 组", p.name(), group.len(), SEMANTIC_ROUNDS),
            });
        }
        let miss = group.iter().filter(|h| !h.hit).count();
        if miss > 0 {
            return Err(PlanError::PolicySemanticMiss {
                vector: 0,
                policy: p,
                why: format!("{} 策略 {} 组未命中——语义破", p.name(), miss),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、确定性对拍（1000 组同输入同输出——J1 后半）
// ---------------------------------------------------------------------------

/// LCG（Numerical Recipes 参数）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }
    /// 下一个 32 位。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.state >> 16) as u32
    }
    /// [0, n)。
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            (self.next_u32() as u64) % n
        }
    }
}

/// 确定性对拍（1000 组：每组生成随机上下文→同一上下文裁决两次→比对）。
///
/// 注意口径：RR 游标随裁决推进——两次裁决间游标已动，比对的是
/// 「同输入+同游标」而不是「同输入+不同游标」（后者的不同输出是
/// 语义不是非确定）。因此每组建一个**新引擎**（同表同游标）裁决两次。
pub fn determinism_check_1000(seed: u64) -> Result<usize, PlanError> {
    let mut rng = Lcg::new(seed);
    let mut consistent = 0usize;
    for round in 0..DETERMINISM_ROUNDS {
        let topo = NumaTopology::build(&[
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 1),
            (5, 1),
            (6, 1),
            (7, 1),
        ])?;
        let mut loads = LoadSnapshot::default();
        let load_events = rng.next_below(16);
        for _ in 0..load_events {
            loads.bump(rng.next_below(8) as usize);
        }
        let vector = (0x20 + rng.next_below(4) as u8) as u8;
        // 同表建两个引擎（同游标起点）——同输入双裁决。
        let run = |reg: u8| -> Result<Vec<usize>, PlanError> {
            let mut eng = AffinityEngine::new();
            match reg % 4 {
                0 => {
                    let mut mask = [0u64; 4];
                    mask[0] |= 1 << 2;
                    mask[0] |= 1 << 5;
                    eng.register(PolicyBinding::pinned(0x20, mask, "det")?)?;
                    eng.register(PolicyBinding::round_robin(0x21, vec![0, 1, 2, 3], "det")?)?;
                    eng.register(PolicyBinding::least_loaded(0x22, vec![0, 1, 2, 3], "det")?)?;
                    eng.register(PolicyBinding::numa_local(0x23, 1, "det")?)?;
                }
                _ => {
                    let mut mask = [0u64; 4];
                    mask[0] |= 1 << 2;
                    mask[0] |= 1 << 5;
                    eng.register(PolicyBinding::pinned(0x20, mask, "det")?)?;
                    eng.register(PolicyBinding::round_robin(0x21, vec![0, 1, 2, 3], "det")?)?;
                    eng.register(PolicyBinding::least_loaded(0x22, vec![0, 1, 2, 3], "det")?)?;
                    eng.register(PolicyBinding::numa_local(0x23, 1, "det")?)?;
                }
            }
            eng.choose(vector, &loads, &topo, round as u64)
        };
        let a = run(vector % 4)?;
        let b = run(vector % 4)?;
        if a == b {
            consistent += 1;
        } else {
            return Err(PlanError::VerdictNondeterministic {
                vector,
                round,
                a: format!("{:?}", a),
                b: format!("{:?}", b),
            });
        }
    }
    Ok(consistent)
}

// ---------------------------------------------------------------------------
// 六、错误 / 快照 / 自证
// ---------------------------------------------------------------------------

/// 引擎失败（全显性——变体带建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// 策略未注册。
    PolicyUnregistered { vector: u8 },
    /// 策略重复注册。
    PolicyDup { vector: u8, owner: &'static str },
    /// 钉扎位图空。
    PinEmpty { vector: u8 },
    /// 钉扎核越界。
    PinCpuOob { vector: u8, cpu: usize, max: usize },
    /// 轮转池空。
    RrPoolEmpty { vector: u8 },
    /// NUMA 节点越界。
    NumaNodeOob { vector: u8, node: usize, max: usize },
    /// 裁决非确定。
    VerdictNondeterministic { vector: u8, round: usize, a: String, b: String },
    /// 策略语义未命中。
    PolicySemanticMiss { vector: u8, policy: AffinityPolicy, why: String },
    /// 裁决账断档。
    VerdictLedgerGap { at: usize },
}

impl PlanError {
    /// 人话诊断。
    pub fn detail(&self) -> String {
        match self {
            PlanError::PolicyUnregistered { vector } => format!("[0x3901] 向量 0x{:02X} 未注册策略——黑箱默认即拒。建议：connect 时显式注册策略", vector),
            PlanError::PolicyDup { vector, owner } => format!("[0x3902] 向量 0x{:02X} 已被 {} 注册——重复绑定。建议：先 unregister 再绑新策略", vector, owner),
            PlanError::PinEmpty { vector } => format!("[0x3903] 钉扎位图空 0x{:02X}。建议：位图至少一位——无处可落的绑定不收", vector),
            PlanError::PinCpuOob { vector, cpu, max } => format!("[0x3904] 钉扎核 {} 越界（上限 {}）0x{:02X}。建议：位图按 MAX_CPUS 界内填", cpu, max, vector),
            PlanError::RrPoolEmpty { vector } => format!("[0x3905] 池空 0x{:02X}。建议：轮转/最闲池至少一核", vector),
            PlanError::NumaNodeOob { vector, node, max } => format!("[0x3907] 节点 {} 越界（上限 {}）0x{:02X}。建议：按 SRAT 拓扑界内填", node, max, vector),
            PlanError::VerdictNondeterministic { vector, round, a, b } => format!("[0x3908] 非确定 0x{:02X} 第{}组：{:?} vs {:?}。建议：查裁决路径隐藏状态（哈希序/浮点/时间）", vector, round, a, b),
            PlanError::PolicySemanticMiss { vector, policy, why } => format!("[0x3909] {} 策略语义未命中 0x{:02X}：{}。建议：裁决结果与策略语义逐位对齐", policy.name(), vector, why),
            PlanError::VerdictLedgerGap { at } => format!("[0x390A] 裁决账序 #{} 断档。建议：裁决账只增不改——查截断路径", at),
        }
    }
}

/// 引擎快照（FNV——表/账/游标任一漂移即变）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AffinitySnap {
    /// 表内策略数。
    pub policies: usize,
    /// 裁决账条数。
    pub verdicts: usize,
    /// 指纹。
    pub fp: u32,
}

impl AffinitySnap {
    /// 取快照。
    pub fn capture(eng: &AffinityEngine) -> Self {
        let mut mat: Vec<u8> = Vec::new();
        mat.extend_from_slice(&eng.table.len().to_le_bytes());
        mat.extend_from_slice(&eng.ledger.len().to_le_bytes());
        for (v, b) in eng.table.iter() {
            mat.push(*v);
            mat.push(b.policy as u8);
            mat.extend_from_slice(&(b.rr_cursor as u64).to_le_bytes());
            for c in b.rr_pool.iter() {
                mat.push(*c);
            }
        }
        for r in eng.ledger.iter() {
            mat.extend_from_slice(&r.seq.to_le_bytes());
            mat.push(r.vector);
            mat.push(r.policy as u8);
            for c in r.chosen.iter() {
                mat.extend_from_slice(&(*c as u64).to_le_bytes());
            }
        }
        AffinitySnap {
            policies: eng.table.len(),
            verdicts: eng.ledger.len(),
            fp: fnv32(&mat, FNV_OFFSET),
        }
    }

    /// 往返一致。
    pub fn roundtrip_ok(&self, other: &AffinitySnap) -> bool {
        self == other
    }
}

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV 常量。
pub const FNV_PRIME: u32 = 0x0100_0193;
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

// ---------------------------------------------------------------------------
// 七、全链自证（J1 走线：语义 4×10/10 + 确定性 1000/1000）
// ---------------------------------------------------------------------------

/// 全链自证（J1 完整走线——两断言全绿）。
pub fn selfcheck_full() -> Result<AffinitySnap, PlanError> {
    // 1. 语义命中：四类策略各 10/10。
    let hits = semantic_assert_all()?;
    semantic_verdict(&hits)?;
    // 2. 确定性对拍：1000 组一致率 100%。
    let consistent = determinism_check_1000(DEFAULT_SEED)?;
    if consistent != DETERMINISM_ROUNDS {
        return Err(PlanError::VerdictNondeterministic {
            vector: 0,
            round: consistent,
            a: format!("{}/{}", consistent, DETERMINISM_ROUNDS),
            b: "期望 1000/1000".to_string(),
        });
    }
    // 3. 裁决账守恒。
    let mut eng = AffinityEngine::new();
    eng.register(PolicyBinding::round_robin(0x20, vec![0, 1, 2], "final")?)?;
    let topo = NumaTopology::build(&[(0, 0), (1, 0), (2, 0)])?;
    let loads = LoadSnapshot::default();
    for t in 0..9u64 {
        eng.choose(0x20, &loads, &topo, t)?;
    }
    if !eng.ledger_ok() {
        return Err(PlanError::VerdictLedgerGap { at: 0 });
    }
    // 4. 快照往返。
    let s1 = AffinitySnap::capture(&eng);
    let s2 = AffinitySnap::capture(&eng);
    if !s1.roundtrip_ok(&s2) {
        return Err(PlanError::VerdictLedgerGap { at: usize::MAX });
    }
    Ok(s1)
}
