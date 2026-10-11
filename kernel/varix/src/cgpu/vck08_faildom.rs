//! vck08_faildom — CGPU-F1608 异构失败域与降级（NPU 失败不拖垮 GPU）。
//!
//! 判据原文（施工书锚点 CGPU-F1608）：失败降级：失败域（引擎级失败域（NPU
//! 失败不拖垮 GPU——失败域隔离——隔离原则（引擎崩溃=该引擎任务失败（图其他
//! 部分可选重路由——重路由策略（失败任务→GPU 软件回退（回退路径表（NPU 推
//! 理→GPU 推理回退/DSP→CPU——逐引擎回退表；部分失败语义（图部分失败——部
//! 分结果可用性标注（消费方决策——部分失败语义；引擎重置（引擎挂→引擎重置
//! 流程（重置安全（不碰其他引擎——重置纪律（重置后重新探测能力——复探；降
//! 级透明（回退执行→结果带"回退"标注（性能差异显性——回退标注；演练（引
//! 擎 kill 注入演练——注入测试；测试（隔离/回退表/部分语义/重置/标注/演练
//! 六组）。判据：失败域、回退表、部分语义、重置纪律、标注、演练、六组、判据。
//!
//! 职责定位：引擎级失败域四域闭集（NPU/GPU/DSP/CPU 各自筑墙，崩溃=该域任务
//! 失败，隔离墙拦截一切跨域传播企图并记账）+ 重路由策略（失败任务沿逐引擎
//! 回退表走：NPU 推理→GPU 推理回退、DSP→CPU，多级链 NPU→GPU→CPU 三跳封
//! 顶，链断裂显性 0x6F02 不静默吞）+ 部分失败语义（图执行逐节点标注可用性
//! 三态 Full/Degraded/None，消费方按标注决策，图级 Outcome 三态 Complete/
//! Partial/Failed——部分结果可用性标注缺失即 0x6F03）+ 重置纪律（重置流程
//! 只碰本域，越界触及其他域即 0x6F04 记账；重置后强制能力复探对接 vck04
//! 探测语义，复探不过降 Retired 不回 Healthy）+ 降级透明（回退执行的结果必
//! 带 Origin=Fallback{path} 标注与性能差异百分比，标注丢失即 0x6F05——消
//! 费方永远知道自己拿的是不是原生结果）+ 演练（引擎 kill 注入：注入→隔离
//! 验证→回退验证→重置验证→复探验证→标注验证五验证闭环，演练报告逐项留
//! 痕）。与 vck04_hetprobe 分工：彼建能力表（探测），此在引擎垮塌时按表降
//! 级（失败域）。诊断码段 0x6F01~0x6F05。

use alloc::vec::Vec;

/// 模块版本。
pub const FAILDOM_VERSION: &str = "K08-faildom-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x6F 段，本会话独占；0x67~0x6E 已被 C04/D06/F05/O01/T03/
// T04/T05/U04 占用——先入库者占段纪律，本文件取 0x6F）
// ---------------------------------------------------------------------------

/// 0x6F01 隔离墙穿越（跨域故障传播企图被墙拦下记账）。
pub const XC_WALL_BREACH: u32 = 0x6F01;
/// 0x6F02 回退链断裂（无可用回退路径——显性拒不静默吞）。
pub const XC_CHAIN_BROKEN: u32 = 0x6F02;
/// 0x6F03 部分结果可用性标注缺失（消费方无从决策）。
pub const XC_ANNOT_MISSING: u32 = 0x6F03;
/// 0x6F04 重置越界（重置流程触及其他引擎域）。
pub const XC_RESET_OVERREACH: u32 = 0x6F04;
/// 0x6F05 回退标注丢失（降级不透明——消费方被蒙在鼓里）。
pub const XC_FALLBACK_UNMARKED: u32 = 0x6F05;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 引擎域数（四域闭集）。
pub const DOMAIN_COUNT: usize = 4;
/// 回退链最大跳数（NPU→GPU→CPU 三跳封顶）。
pub const MAX_FALLBACK_HOPS: usize = 3;
/// 演练案例上限。
pub const MAX_DRILL_CASES: usize = 8;
/// 部分失败图中节点数上限（演练图规模）。
pub const GRAPH_NODES_MAX: usize = 16;
/// 原生执行基线毫秒（性能差异百分比分母，非零保证）。
pub const NATIVE_BASELINE_MS: u32 = 100;

// ---------------------------------------------------------------------------
// 一、引擎与状态（失败域四域闭集）
// ---------------------------------------------------------------------------

/// 引擎四域（判据"引擎级失败域"——每域一堵墙）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineId {
    Npu,
    Gpu,
    Dsp,
    Cpu,
}

impl EngineId {
    /// 四域闭集。
    pub const ALL: [EngineId; DOMAIN_COUNT] =
        [EngineId::Npu, EngineId::Gpu, EngineId::Dsp, EngineId::Cpu];

    /// 中文名（判据行锚点）。
    pub const fn zh(self) -> &'static str {
        match self {
            EngineId::Npu => "NPU",
            EngineId::Gpu => "GPU",
            EngineId::Dsp => "DSP",
            EngineId::Cpu => "CPU",
        }
    }

    /// 闭集内序号（账本索引）。
    pub const fn idx(self) -> usize {
        match self {
            EngineId::Npu => 0,
            EngineId::Gpu => 1,
            EngineId::Dsp => 2,
            EngineId::Cpu => 3,
        }
    }

    /// 从序号还原（越界给 Cpu 兜底——账本闭合）。
    pub const fn from_idx(i: usize) -> EngineId {
        match i {
            0 => EngineId::Npu,
            1 => EngineId::Gpu,
            2 => EngineId::Dsp,
            _ => EngineId::Cpu,
        }
    }
}

/// 引擎五态（Healthy 稳态 / Degraded 带病运行 / Crashed 崩塌 / Resetting
/// 重置中 / Retired 复探不过退役——五态闭集不造第六态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineState {
    Healthy,
    Degraded,
    Crashed,
    Resetting,
    Retired,
}

impl EngineState {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            EngineState::Healthy => "健康",
            EngineState::Degraded => "带病",
            EngineState::Crashed => "崩塌",
            EngineState::Resetting => "重置中",
            EngineState::Retired => "退役",
        }
    }

    /// 该态是否可承接新任务（Retired/Crashed/Resetting 三态拒收）。
    pub const fn schedulable(self) -> bool {
        matches!(self, EngineState::Healthy | EngineState::Degraded)
    }
}

/// 单个失败域（一域一账：状态+崩溃数+重置数+复探标记）。
#[derive(Clone, Copy, Debug)]
pub struct Domain {
    /// 域引擎。
    pub id: EngineId,
    /// 当前状态。
    pub state: EngineState,
    /// 崩塌次数。
    pub crash_count: u32,
    /// 重置次数。
    pub reset_count: u32,
    /// 最近一次重置后是否已复探。
    pub reprobe_done: bool,
    /// 最近事件备注（'C'崩/'R'重/'P'复探/'D'降级，0=无）。
    pub last_note: u8,
}

impl Domain {
    /// 新域出厂即健康。
    pub const fn new(id: EngineId) -> Domain {
        Domain {
            id,
            state: EngineState::Healthy,
            crash_count: 0,
            reset_count: 0,
            reprobe_done: true,
            last_note: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 二、隔离墙（DomainRegistry：崩溃不跨域）
// ---------------------------------------------------------------------------

/// 失败域注册表：四域账本+隔离墙。
///
/// 隔离原则（判据原文）：引擎崩溃=该引擎任务失败——本表唯一的写入口
/// `crash` 只动被点名的域；`propagate_attempt` 显式模拟跨域传播并拦下
/// 记账 0x6F01——墙不是"碰巧没塌"而是"塌了也过不来"。
#[derive(Debug)]
pub struct DomainRegistry {
    /// 四域账本（按 EngineId::ALL 序）。
    pub domains: [Domain; DOMAIN_COUNT],
    /// 隔离墙拦截的跨域传播次数。
    pub wall_intercepts: u32,
    /// 崩塌事件总账。
    pub crash_events: u32,
}

impl DomainRegistry {
    /// 出厂：四域全健康，墙零拦截。
    pub const fn new() -> DomainRegistry {
        DomainRegistry {
            domains: [
                Domain::new(EngineId::Npu),
                Domain::new(EngineId::Gpu),
                Domain::new(EngineId::Dsp),
                Domain::new(EngineId::Cpu),
            ],
            wall_intercepts: 0,
            crash_events: 0,
        }
    }

    /// 域查询（快照拷贝——外部不持有内部引用）。
    pub fn domain(&self, id: EngineId) -> Domain {
        self.domains[id.idx()]
    }

    /// 引擎崩塌：只标记本域，崩溃计数+1（隔离写入口）。
    pub fn crash(&mut self, id: EngineId) -> Result<(), u32> {
        let i = id.idx();
        self.domains[i].state = EngineState::Crashed;
        self.domains[i].crash_count += 1;
        self.domains[i].last_note = b'C';
        self.crash_events += 1;
        Ok(())
    }

    /// 跨域传播企图：NPU 崩了想让 GPU 陪葬——墙拦下记 0x6F01。
    ///
    /// 返回 Err(0x6F01) 表传播被拒；from 域状态不受影响（墙外的事墙外了）。
    pub fn propagate_attempt(&mut self, from: EngineId, to: EngineId) -> Result<(), u32> {
        if from == to {
            return Ok(()); // 同域不叫传播
        }
        self.wall_intercepts += 1;
        Err(XC_WALL_BREACH)
    }

    /// 隔离验证：from 崩塌后，to 域必须仍然可调度（NPU 失败不拖垮 GPU）。
    pub fn isolation_holds(&self, crashed: EngineId, bystander: EngineId) -> bool {
        if crashed == bystander {
            return true;
        }
        self.domain(bystander).state.schedulable()
    }

    /// 全域崩塌扫描（演练用）。
    pub fn crashed_domains(&self) -> Vec<EngineId> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < DOMAIN_COUNT {
            if self.domains[i].state == EngineState::Crashed {
                out.push(EngineId::from_idx(i));
            }
            i += 1;
        }
        out
    }

    /// 状态快照（重置安全对拍用——只拍五态，不带域账）。
    pub const fn state_snapshot(&self) -> [EngineState; DOMAIN_COUNT] {
        [
            self.domains[0].state,
            self.domains[1].state,
            self.domains[2].state,
            self.domains[3].state,
        ]
    }
}

// ---------------------------------------------------------------------------
// 三、回退路径表（逐引擎回退表 + 多级链 + 重路由策略）
// ---------------------------------------------------------------------------

/// 逐引擎回退表（判据原文"回退路径表（NPU 推理→GPU 推理回退/DSP→CPU"）。
///
/// 表语义：`first_hop(id)` 给该引擎崩塌后的第一回退目标；沿表续走构成多级
/// 链（NPU→GPU→CPU）。表是静态闭集——未登记回退的引擎（CPU 是最终兜底）
/// 链到此为止，链断裂走 0x6F02 显性报。
#[derive(Debug)]
pub struct FallbackTable {
    /// 链解析走过的最大跳数（审计用）。
    pub max_hops_used: usize,
    /// 链断裂次数账。
    pub broken_chains: u32,
    /// 重路由任务数账。
    pub rerouted_tasks: u32,
}

impl FallbackTable {
    /// 新表。
    pub const fn new() -> FallbackTable {
        FallbackTable {
            max_hops_used: 0,
            broken_chains: 0,
            rerouted_tasks: 0,
        }
    }

    /// 第一跳回退（逐引擎回退表本体）。
    pub const fn first_hop(id: EngineId) -> Option<EngineId> {
        match id {
            EngineId::Npu => Some(EngineId::Gpu), // NPU 推理→GPU 推理回退
            EngineId::Dsp => Some(EngineId::Cpu), // DSP→CPU
            EngineId::Gpu => Some(EngineId::Cpu), // GPU→CPU 软件回退
            EngineId::Cpu => None,                // CPU 是最终兜底，无更深处可退
        }
    }

    /// 多级回退链解析（上限 MAX_FALLBACK_HOPS 封顶防环）。
    pub fn chain_of(&mut self, id: EngineId) -> Vec<EngineId> {
        let mut chain = Vec::new();
        let mut cur = id;
        let mut hops = 0;
        while hops < MAX_FALLBACK_HOPS {
            match Self::first_hop(cur) {
                Some(next) => {
                    chain.push(next);
                    cur = next;
                    hops += 1;
                }
                None => break,
            }
        }
        if hops > self.max_hops_used {
            self.max_hops_used = hops;
        }
        chain
    }

    /// 重路由策略：失败任务沿链找第一个可调度引擎。
    ///
    /// registry 用于逐跳健康检查（跳过崩塌/重置中/退役的候选）。
    /// 无路可走→Err(0x6F02) 链断裂显性报。
    pub fn reroute(
        &mut self,
        failed: EngineId,
        registry: &DomainRegistry,
    ) -> Result<EngineId, u32> {
        let chain = self.chain_of(failed);
        let mut i = 0;
        while i < chain.len() {
            let cand = chain[i];
            if registry.domain(cand).state.schedulable() {
                self.rerouted_tasks += 1;
                return Ok(cand);
            }
            i += 1;
        }
        self.broken_chains += 1;
        Err(XC_CHAIN_BROKEN)
    }

    /// 表完备性对账：四域逐域验表（CPU 兜底无回退=合法；其余三域必须有跳）。
    pub fn table_complete(&self) -> bool {
        let mut i = 0;
        while i < DOMAIN_COUNT {
            let id = EngineId::from_idx(i);
            if id != EngineId::Cpu && Self::first_hop(id).is_none() {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 四、部分失败语义（图部分失败——部分结果可用性标注——消费方决策）
// ---------------------------------------------------------------------------

/// 节点可用性三态（判据"部分结果可用性标注"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Availability {
    /// 原生完成，全量可用。
    Full,
    /// 回退完成，降级可用（消费方决策是否接受）。
    Degraded,
    /// 未能产出（连回退也无路）。
    None,
}

impl Availability {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Availability::Full => "全量可用",
            Availability::Degraded => "降级可用",
            Availability::None => "不可用",
        }
    }
}

/// 图节点执行结果（逐节点标注——部分失败语义的原子单位）。
#[derive(Clone, Copy, Debug)]
pub struct NodeResult {
    /// 节点号。
    pub node: usize,
    /// 计划执行域。
    pub planned: EngineId,
    /// 实际执行域（回退时与 planned 不同）。
    pub executed: EngineId,
    /// 可用性标注。
    pub avail: Availability,
}

impl NodeResult {
    /// 标注一致性：说不可用（None）就必须没换域执行（没跑成谈不上回退落
    /// 地）；换了域却报 None=标注矛盾。
    pub fn annotation_consistent(&self) -> bool {
        if self.avail == Availability::None {
            self.executed == self.planned
        } else {
            true
        }
    }
}

/// 图级三态（Complete 全绿 / Partial 部分可用 / Failed 全灭）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphOutcome {
    Complete,
    Partial,
    Failed,
}

/// 部分失败语义引擎：逐节点落标注、图级汇总、消费方决策。
#[derive(Debug)]
pub struct PartialSemantics {
    /// 节点结果账。
    pub nodes: Vec<NodeResult>,
    /// 标注缺失次数账（0x6F03）。
    pub missing_annotations: u32,
}

impl PartialSemantics {
    /// 新语义账。
    pub const fn new() -> PartialSemantics {
        PartialSemantics {
            nodes: Vec::new(),
            missing_annotations: 0,
        }
    }

    /// 记一个节点结果（标注缺失即 0x6F03 记账拒收——没有标注的结果不进账）。
    pub fn record(&mut self, r: NodeResult) -> Result<(), u32> {
        // None 可用性必须配 executed 停留在计划域之外或明确失败——这里统一
        // 约束：avail=None 时 executed 必须等于 planned（没跑成，谈不上换域）。
        if r.avail == Availability::None && r.executed != r.planned {
            self.missing_annotations += 1;
            return Err(XC_ANNOT_MISSING);
        }
        self.nodes.push(r);
        Ok(())
    }

    /// 图级汇总：有 None→至少有 Full/Degraded 存活则 Partial，全 None 则
    /// Failed，全无 None 则 Complete。
    pub fn outcome(&self) -> GraphOutcome {
        if self.nodes.is_empty() {
            return GraphOutcome::Failed;
        }
        let mut dead = 0usize;
        let mut alive = 0usize;
        let mut i = 0;
        while i < self.nodes.len() {
            match self.nodes[i].avail {
                Availability::None => dead += 1,
                _ => alive += 1,
            }
            i += 1;
        }
        if dead == 0 {
            GraphOutcome::Complete
        } else if alive > 0 {
            GraphOutcome::Partial
        } else {
            GraphOutcome::Failed
        }
    }

    /// 消费方决策（判据"消费方决策"）：给节点号，回消费方视角的可用裁决。
    ///
    /// Full=可消费；Degraded=可消费但须带降级提示（消费方自行权衡）；
    /// None=不可消费。越界节点号=不可消费。
    pub fn consume_decision(&self, node: usize) -> Result<Availability, u32> {
        let mut i = 0;
        while i < self.nodes.len() {
            if self.nodes[i].node == node {
                return Ok(self.nodes[i].avail);
            }
            i += 1;
        }
        Err(XC_ANNOT_MISSING)
    }

    /// 存活节点清点（Partial 态下消费方最关心的一行数）。
    pub fn alive_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.nodes.len() {
            if self.nodes[i].avail != Availability::None {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 五、重置纪律（重置安全——不碰其他引擎——重置后复探）
// ---------------------------------------------------------------------------

/// 重置协议：软重置本域+隔离审计+复探对接。
#[derive(Debug)]
pub struct ResetProtocol {
    /// 重置账：每域重置次数（对外快照用 domains 里也有，此处留独立账防串）。
    pub resets: [u32; DOMAIN_COUNT],
    /// 越界账：重置流程触及其他域的次数（演练注入验证这个必须为 0）。
    pub overreach_events: u32,
    /// 复探账：复探通过次数。
    pub reprobe_passes: u32,
    /// 复探失败退役次数。
    pub retirements: u32,
}

impl ResetProtocol {
    /// 新协议。
    pub const fn new() -> ResetProtocol {
        ResetProtocol {
            resets: [0; DOMAIN_COUNT],
            overreach_events: 0,
            reprobe_passes: 0,
            retirements: 0,
        }
    }

    /// 引擎重置（判据"重置安全（不碰其他引擎）"）。
    ///
    /// 流程：目标域置 Resetting→复探→按复探结果定 Healthy 或 Retired。
    /// `touch_scope` 是重置实际触碰的域集合审计口：流程只应触碰单元素
    /// [id]，越界即 0x6F04 记账。
    pub fn reset(
        &mut self,
        registry: &mut DomainRegistry,
        id: EngineId,
        touch_scope: &[EngineId],
        reprobe_ok: bool,
    ) -> Result<(), u32> {
        // 越界审计：触碰集必须恰好是 {id}。
        let mut i = 0;
        while i < touch_scope.len() {
            if touch_scope[i] != id {
                self.overreach_events += 1;
                return Err(XC_RESET_OVERREACH);
            }
            i += 1;
        }
        if touch_scope.is_empty() {
            self.overreach_events += 1;
            return Err(XC_RESET_OVERREACH);
        }
        let idx = id.idx();
        registry.domains[idx].state = EngineState::Resetting;
        registry.domains[idx].reset_count += 1;
        registry.domains[idx].last_note = b'R';
        self.resets[idx] += 1;
        // 复探（判据"重置后重新探测能力——复探"）：对接 vck04 探测语义的
        // 布尔门。复探不过=Retired，绝不回 Healthy。
        if reprobe_ok {
            registry.domains[idx].state = EngineState::Healthy;
            registry.domains[idx].reprobe_done = true;
            registry.domains[idx].last_note = b'P';
            self.reprobe_passes += 1;
        } else {
            registry.domains[idx].state = EngineState::Retired;
            registry.domains[idx].reprobe_done = false;
            registry.domains[idx].last_note = b'D';
            self.retirements += 1;
        }
        Ok(())
    }

    /// 重置安全不变量：全域扫描——除重置目标外其余三域状态必须未被重置流
    /// 程改动（调用方在重置前后各拍一次快照对拍）。
    pub fn safety_holds(
        &self,
        before: &[EngineState; DOMAIN_COUNT],
        after: &[EngineState; DOMAIN_COUNT],
        target: EngineId,
    ) -> bool {
        let mut i = 0;
        while i < DOMAIN_COUNT {
            if i != target.idx() && before[i] != after[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 六、降级透明（回退标注 + 性能差异显性）
// ---------------------------------------------------------------------------

/// 结果来源标注（判据"结果带'回退'标注（性能差异显性）"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Origin {
    /// 原生执行（计划域跑成的）。
    Native,
    /// 回退执行（换域跑成的——必须带路径与性能差）。
    Fallback {
        /// 回退链（自失败引擎起，至实际执行域止）。
        hops: u8,
        /// 实际执行域序号。
        landed: usize,
    },
}

/// 带标注的结果（降级透明的交付物）。
#[derive(Clone, Copy, Debug)]
pub struct AnnotatedResult {
    /// 来源标注。
    pub origin: Origin,
    /// 原生基线耗时（毫秒）。
    pub native_ms: u32,
    /// 实际耗时（毫秒）。
    pub actual_ms: u32,
}

impl AnnotatedResult {
    /// 原生结果。
    pub const fn native(actual_ms: u32) -> AnnotatedResult {
        AnnotatedResult {
            origin: Origin::Native,
            native_ms: NATIVE_BASELINE_MS,
            actual_ms,
        }
    }

    /// 回退结果（强制带链信息——没有路径信息的回退结果构造不出来）。
    pub const fn fallback(hops: u8, landed: usize, actual_ms: u32) -> AnnotatedResult {
        AnnotatedResult {
            origin: Origin::Fallback { hops, landed },
            native_ms: NATIVE_BASELINE_MS,
            actual_ms,
        }
    }

    /// 性能差异千分比（实际相对基线慢多少‰；快了返回 0 不给负数——
    /// no_std 无浮点惯例，千分整数足够显性）。
    pub fn perf_penalty_permille(&self) -> u32 {
        if self.actual_ms <= self.native_ms || self.native_ms == 0 {
            return 0;
        }
        let delta = (self.actual_ms - self.native_ms) as u64 * 1000;
        let base = self.native_ms as u64;
        let v = delta / base;
        if v > u32::MAX as u64 {
            u32::MAX
        } else {
            v as u32
        }
    }

    /// 透明性校验：回退来源必须有正数链信息且性能差可计算。
    pub fn transparent(&self) -> bool {
        match self.origin {
            Origin::Native => true,
            Origin::Fallback { hops, landed } => hops > 0 && landed < DOMAIN_COUNT,
        }
    }
}

/// 标注账：交付过的结果逐条留痕，标注丢失（不透明结果出库）即 0x6F05。
#[derive(Debug)]
pub struct FallbackAnnotation {
    /// 交付账。
    pub ledger: Vec<AnnotatedResult>,
    /// 不透明出库次数账（必须恒 0——出库即账）。
    pub opaque_deliveries: u32,
}

impl FallbackAnnotation {
    /// 新账。
    pub const fn new() -> FallbackAnnotation {
        FallbackAnnotation {
            ledger: Vec::new(),
            opaque_deliveries: 0,
        }
    }

    /// 交付：不透明结果拒收记账 0x6F05。
    pub fn deliver(&mut self, r: AnnotatedResult) -> Result<(), u32> {
        if !r.transparent() {
            self.opaque_deliveries += 1;
            return Err(XC_FALLBACK_UNMARKED);
        }
        self.ledger.push(r);
        Ok(())
    }

    /// 回退交付占比（万分比）——降级透明度的量化公示。
    pub fn fallback_ratio_bp(&self) -> u32 {
        if self.ledger.is_empty() {
            return 0;
        }
        let mut fb = 0usize;
        let mut i = 0;
        while i < self.ledger.len() {
            if matches!(self.ledger[i].origin, Origin::Fallback { .. }) {
                fb += 1;
            }
            i += 1;
        }
        let total = self.ledger.len() as u64;
        let v = fb as u64 * 10_000 / total;
        if v > u32::MAX as u64 {
            u32::MAX
        } else {
            v as u32
        }
    }
}

// ---------------------------------------------------------------------------
// 七、演练（引擎 kill 注入——五验证闭环）
// ---------------------------------------------------------------------------

/// 演练案例（判据"注入测试"）。
#[derive(Clone, Copy, Debug)]
pub struct DrillCase {
    /// 注入 kill 的引擎。
    pub victim: EngineId,
    /// 预期重路由落点（None=预期链断裂）。
    pub expect_land: Option<EngineId>,
    /// 复探是否通过（演练可注入复探失败考察退役路径）。
    pub reprobe_ok: bool,
}

/// 演练报告（五验证逐项留痕）。
#[derive(Clone, Copy, Debug)]
pub struct DrillReport {
    /// 注入引擎。
    pub victim: EngineId,
    /// 验证一：隔离（旁观域仍可调度）。
    pub v_isolation: bool,
    /// 验证二：回退（重路由落点符合预期）。
    pub v_fallback: bool,
    /// 验证三：重置（重置安全无越界）。
    pub v_reset: bool,
    /// 验证四：复探（复探后状态正确）。
    pub v_reprobe: bool,
    /// 验证五：标注（回退结果带透明标注）。
    pub v_annotation: bool,
}

impl DrillReport {
    /// 五验证是否全绿。
    pub const fn all_green(&self) -> bool {
        self.v_isolation && self.v_fallback && self.v_reset && self.v_reprobe && self.v_annotation
    }
}

/// 失败域总引擎：注册表+回退表+部分语义+重置协议+标注账+演练簿六件合体。
#[derive(Debug)]
pub struct FailureEngine {
    /// 域注册表（含隔离墙）。
    pub registry: DomainRegistry,
    /// 回退路径表。
    pub table: FallbackTable,
    /// 部分失败语义。
    pub partial: PartialSemantics,
    /// 重置协议。
    pub reset_proto: ResetProtocol,
    /// 标注账。
    pub annotation: FallbackAnnotation,
    /// 演练案例簿。
    pub drill_cases: Vec<DrillCase>,
    /// 演练报告簿。
    pub drill_reports: Vec<DrillReport>,
}

impl FailureEngine {
    /// 出厂。
    pub const fn new() -> FailureEngine {
        FailureEngine {
            registry: DomainRegistry::new(),
            table: FallbackTable::new(),
            partial: PartialSemantics::new(),
            reset_proto: ResetProtocol::new(),
            annotation: FallbackAnnotation::new(),
            drill_cases: Vec::new(),
            drill_reports: Vec::new(),
        }
    }

    /// 单案例演练：kill 注入→五验证→报告入簿。
    ///
    /// 流程（判据"引擎 kill 注入演练"）：
    /// 1. kill 注入（crash victim）；
    /// 2. 隔离验证：victim 外任取旁观域仍可调度；
    /// 3. 回退验证：重路由落点与预期一致（预期断裂则验 Err(0x6F02)）；
    /// 4. 重置验证：对 victim 走重置流程（触碰集恰为 {victim}）后快照
    ///    对拍——其他三域状态不得被改；
    /// 5. 复探验证：复探通过→Healthy / 复探失败→Retired；
    /// 6. 标注验证：回退成功时交付结果必须透明（Fallback 标注+性能差）。
    pub fn kill_drill(&mut self, case: DrillCase) -> Result<DrillReport, u32> {
        // 验证一前置：拍旁观域名册（victim 之外的第一旁观域）。
        let mut bystander = EngineId::Cpu;
        let mut i = 0;
        while i < DOMAIN_COUNT {
            let cand = EngineId::from_idx(i);
            if cand != case.victim {
                bystander = cand;
                break;
            }
            i += 1;
        }
        // 1. kill 注入。
        self.registry.crash(case.victim)?;
        // 2. 隔离验证：旁观域仍可调度（NPU 失败不拖垮 GPU）。
        let v_isolation = self.registry.isolation_holds(case.victim, bystander);
        // 3. 回退验证。
        let v_fallback;
        let mut landed: Option<EngineId> = None;
        match self.table.reroute(case.victim, &self.registry) {
            Ok(target) => {
                landed = Some(target);
                v_fallback = case.expect_land == Some(target);
            }
            Err(code) => {
                if code == XC_CHAIN_BROKEN && case.expect_land.is_none() {
                    v_fallback = true;
                } else {
                    v_fallback = false;
                }
            }
        }
        // 4/5. 重置+复探验证：快照前后对拍。
        let before = self.registry.state_snapshot();
        // 演练走标准触碰集：恰 {victim}。
        let scope = [case.victim];
        let (v_reset, v_reprobe) = match self
            .reset_proto
            .reset(&mut self.registry, case.victim, &scope, case.reprobe_ok)
        {
            Ok(()) => {
                let after = self.registry.state_snapshot();
                let vr = self.reset_proto.safety_holds(&before, &after, case.victim);
                let st = self.registry.domain(case.victim).state;
                let vp = if case.reprobe_ok {
                    st == EngineState::Healthy
                } else {
                    st == EngineState::Retired
                };
                (vr, vp)
            }
            Err(_) => (false, false),
        };
        // 6. 标注验证：回退成功时交付结果必须透明。
        let v_annotation = match landed {
            Some(target) => {
                // 回退耗时按跳数放大（每跳 +40ms 基线）——性能差异显性。
                let hops = self.table.chain_of(case.victim).len().min(u8::MAX as usize) as u8;
                let actual = NATIVE_BASELINE_MS + 40 * hops as u32;
                let r = AnnotatedResult::fallback(hops.max(1), target.idx(), actual);
                match self.annotation.deliver(r) {
                    Ok(()) => r.transparent(),
                    Err(_) => false,
                }
            }
            None => self.annotation.opaque_deliveries == 0,
        };
        let report = DrillReport {
            victim: case.victim,
            v_isolation,
            v_fallback,
            v_reset,
            v_reprobe,
            v_annotation,
        };
        self.drill_reports.push(report);
        if report.all_green() {
            Ok(report)
        } else {
            Err(XC_WALL_BREACH)
        }
    }

    /// 图级部分失败演练：victim 崩→部分节点回退→部分语义落账。
    ///
    /// nodes 计划表：每节点一个计划域；崩塌域上的节点走重路由，落成的记
    /// Degraded，重路由断裂记 None，计划域不崩的记 Full。
    pub fn graph_partial_drill(&mut self, plans: &[(usize, EngineId)], victim: EngineId) -> Result<GraphOutcome, u32> {
        self.registry.crash(victim)?;
        let mut i = 0;
        while i < plans.len() {
            let (node, planned) = plans[i];
            if planned == victim {
                match self.table.reroute(victim, &self.registry) {
                    Ok(target) => {
                        self.partial.record(NodeResult {
                            node,
                            planned,
                            executed: target,
                            avail: Availability::Degraded,
                        })?;
                    }
                    Err(code) => {
                        if code != XC_CHAIN_BROKEN {
                            return Err(code);
                        }
                        self.partial.record(NodeResult {
                            node,
                            planned,
                            executed: planned,
                            avail: Availability::None,
                        })?;
                    }
                }
            } else {
                self.partial.record(NodeResult {
                    node,
                    planned,
                    executed: planned,
                    avail: Availability::Full,
                })?;
            }
            i += 1;
        }
        Ok(self.partial.outcome())
    }

    /// 演练案例登记（上限 MAX_DRILL_CASES）。
    pub fn enroll(&mut self, case: DrillCase) -> Result<(), u32> {
        if self.drill_cases.len() >= MAX_DRILL_CASES {
            return Err(XC_CHAIN_BROKEN);
        }
        self.drill_cases.push(case);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 八、自检（六组：隔离/回退表/部分语义/重置/标注/演练 + 段对账）
// ---------------------------------------------------------------------------

impl FailureEngine {
    /// 组一：隔离——NPU 崩不拖垮 GPU；墙拦跨域传播；账本闭合。
    pub fn selfcheck_isolation(&mut self) -> Result<(), u32> {
        let mut e = FailureEngine::new();
        e.registry.crash(EngineId::Npu)?;
        if e.registry.domain(EngineId::Npu).state != EngineState::Crashed {
            return Err(XC_WALL_BREACH);
        }
        if !e.registry.isolation_holds(EngineId::Npu, EngineId::Gpu) {
            return Err(XC_WALL_BREACH); // GPU 被拖垮=隔离失败
        }
        // 跨域传播企图必被拦（NPU 想拖 GPU 陪葬）。
        if e.registry.propagate_attempt(EngineId::Npu, EngineId::Gpu).is_ok() {
            return Err(XC_WALL_BREACH);
        }
        if e.registry.wall_intercepts != 1 {
            return Err(XC_WALL_BREACH);
        }
        // 同域不叫传播（合法直通）。
        if e.registry.propagate_attempt(EngineId::Npu, EngineId::Npu).is_err() {
            return Err(XC_WALL_BREACH);
        }
        // 崩塌账闭合。
        if e.registry.crash_events != 1 || e.registry.domain(EngineId::Npu).crash_count != 1 {
            return Err(XC_WALL_BREACH);
        }
        // 崩塌扫描闭合。
        let dead = e.registry.crashed_domains();
        if dead.len() != 1 || dead[0] != EngineId::Npu {
            return Err(XC_WALL_BREACH);
        }
        Ok(())
    }

    /// 组二：回退表——四域逐域验表、链解析封顶、重路由落点、断裂显性。
    pub fn selfcheck_fallback_table(&mut self) -> Result<(), u32> {
        let mut e = FailureEngine::new();
        if !e.table.table_complete() {
            return Err(XC_CHAIN_BROKEN);
        }
        // 逐引擎回退表对册：NPU→GPU、DSP→CPU、GPU→CPU、CPU→None。
        if FallbackTable::first_hop(EngineId::Npu) != Some(EngineId::Gpu) {
            return Err(XC_CHAIN_BROKEN);
        }
        if FallbackTable::first_hop(EngineId::Dsp) != Some(EngineId::Cpu) {
            return Err(XC_CHAIN_BROKEN);
        }
        if FallbackTable::first_hop(EngineId::Gpu) != Some(EngineId::Cpu) {
            return Err(XC_CHAIN_BROKEN);
        }
        if FallbackTable::first_hop(EngineId::Cpu).is_some() {
            return Err(XC_CHAIN_BROKEN);
        }
        // 多级链：NPU→GPU→CPU 两跳封顶。
        let chain = e.table.chain_of(EngineId::Npu);
        if chain.len() != 2 || chain[0] != EngineId::Gpu || chain[1] != EngineId::Cpu {
            return Err(XC_CHAIN_BROKEN);
        }
        // NPU 崩→重路由落 GPU（第一跳健康即停）。
        e.registry.crash(EngineId::Npu)?;
        if e.table.reroute(EngineId::Npu, &e.registry)? != EngineId::Gpu {
            return Err(XC_CHAIN_BROKEN);
        }
        // GPU 也崩→落 CPU（跳过崩塌候选）。
        e.registry.crash(EngineId::Gpu)?;
        if e.table.reroute(EngineId::Npu, &e.registry)? != EngineId::Cpu {
            return Err(XC_CHAIN_BROKEN);
        }
        // CPU 兜底也崩→链断裂显性 0x6F02，不静默吞。
        e.registry.crash(EngineId::Cpu)?;
        match e.table.reroute(EngineId::Npu, &e.registry) {
            Err(code) if code == XC_CHAIN_BROKEN => {}
            _ => return Err(XC_CHAIN_BROKEN),
        }
        if e.table.broken_chains != 1 {
            return Err(XC_CHAIN_BROKEN);
        }
        Ok(())
    }

    /// 组三：部分语义——逐节点标注、图级三态、消费方决策、标注缺失拦截。
    pub fn selfcheck_partial(&mut self) -> Result<(), u32> {
        let mut e = FailureEngine::new();
        // 全绿图：三节点全 Full→Complete。
        let mut n = 0;
        while n < 3 {
            e.partial.record(NodeResult {
                node: n,
                planned: EngineId::Gpu,
                executed: EngineId::Gpu,
                avail: Availability::Full,
            })?;
            n += 1;
        }
        if e.partial.outcome() != GraphOutcome::Complete {
            return Err(XC_ANNOT_MISSING);
        }
        // 混合图：一节点 Degraded 一节点 None→Partial；消费方决策逐节点可查。
        let mut e2 = FailureEngine::new();
        e2.partial.record(NodeResult {
            node: 0,
            planned: EngineId::Npu,
            executed: EngineId::Gpu,
            avail: Availability::Degraded,
        })?;
        e2.partial.record(NodeResult {
            node: 1,
            planned: EngineId::Npu,
            executed: EngineId::Npu,
            avail: Availability::None,
        })?;
        if e2.partial.outcome() != GraphOutcome::Partial {
            return Err(XC_ANNOT_MISSING);
        }
        if e2.partial.consume_decision(0)? != Availability::Degraded {
            return Err(XC_ANNOT_MISSING);
        }
        if e2.partial.consume_decision(1)? != Availability::None {
            return Err(XC_ANNOT_MISSING);
        }
        // 越界节点=不可消费（Err 而非编造 Full）。
        if e2.partial.consume_decision(99).is_ok() {
            return Err(XC_ANNOT_MISSING);
        }
        // None 标注配换域执行=标注矛盾（执行了却说不可用）→0x6F03 拒收。
        let bad = NodeResult {
            node: 7,
            planned: EngineId::Npu,
            executed: EngineId::Gpu,
            avail: Availability::None,
        };
        if e2.partial.record(bad) != Err(XC_ANNOT_MISSING) {
            return Err(XC_ANNOT_MISSING);
        }
        if e2.partial.missing_annotations != 1 {
            return Err(XC_ANNOT_MISSING);
        }
        // 标注一致性接口：矛盾案例 false；正常 None（没换域）与 Degraded 均 true。
        if bad.annotation_consistent() {
            return Err(XC_ANNOT_MISSING);
        }
        if !e2.partial.nodes[0].annotation_consistent()
            || !e2.partial.nodes[1].annotation_consistent()
        {
            return Err(XC_ANNOT_MISSING);
        }
        // 全灭图：单节点 None→Failed。
        let mut e3 = FailureEngine::new();
        e3.partial.record(NodeResult {
            node: 0,
            planned: EngineId::Dsp,
            executed: EngineId::Dsp,
            avail: Availability::None,
        })?;
        if e3.partial.outcome() != GraphOutcome::Failed {
            return Err(XC_ANNOT_MISSING);
        }
        Ok(())
    }

    /// 组四：重置——越界拦截、复探过则健康、复探败则退役、快照对拍安全。
    pub fn selfcheck_reset(&mut self) -> Result<(), u32> {
        let mut e = FailureEngine::new();
        // 越界：触碰集含他域→0x6F04 拒收。
        e.registry.crash(EngineId::Npu)?;
        let scope_bad = [EngineId::Npu, EngineId::Gpu];
        if e.reset_proto.reset(&mut e.registry, EngineId::Npu, &scope_bad, true)
            != Err(XC_RESET_OVERREACH)
        {
            return Err(XC_RESET_OVERREACH);
        }
        // 空触碰集=同样越界。
        let scope_empty: [EngineId; 0] = [];
        if e.reset_proto.reset(&mut e.registry, EngineId::Npu, &scope_empty, true)
            != Err(XC_RESET_OVERREACH)
        {
            return Err(XC_RESET_OVERREACH);
        }
        if e.reset_proto.overreach_events != 2 {
            return Err(XC_RESET_OVERREACH);
        }
        // 正路：恰 {NPU} 触碰+复探过→Healthy。
        let scope_ok = [EngineId::Npu];
        let before = e.registry.state_snapshot();
        e.reset_proto.reset(&mut e.registry, EngineId::Npu, &scope_ok, true)?;
        let after = e.registry.state_snapshot();
        if !e.reset_proto.safety_holds(&before, &after, EngineId::Npu) {
            return Err(XC_RESET_OVERREACH); // 他域被动=重置越界实锤
        }
        if e.registry.domain(EngineId::Npu).state != EngineState::Healthy {
            return Err(XC_RESET_OVERREACH);
        }
        // 复探败→Retired，绝不回 Healthy。
        e.registry.crash(EngineId::Dsp)?;
        e.reset_proto.reset(&mut e.registry, EngineId::Dsp, &[EngineId::Dsp], false)?;
        if e.registry.domain(EngineId::Dsp).state != EngineState::Retired {
            return Err(XC_RESET_OVERREACH);
        }
        if e.reset_proto.retirements != 1 {
            return Err(XC_RESET_OVERREACH);
        }
        // Retired 域不可调度（退役不接活）。
        if EngineState::Retired.schedulable() {
            return Err(XC_RESET_OVERREACH);
        }
        Ok(())
    }

    /// 组五：标注——回退结果强制带路径与性能差、不透明拒收、占比可算。
    pub fn selfcheck_annotation(&mut self) -> Result<(), u32> {
        let mut e = FailureEngine::new();
        // 原生结果透明。
        let nat = AnnotatedResult::native(100);
        if !nat.transparent() || nat.perf_penalty_permille() != 0 {
            return Err(XC_FALLBACK_UNMARKED);
        }
        // 回退结果：两跳落 GPU/CPU，实际 180ms→慢 800‰。
        let fb = AnnotatedResult::fallback(2, EngineId::Cpu.idx(), 180);
        if !fb.transparent() {
            return Err(XC_FALLBACK_UNMARKED);
        }
        if fb.perf_penalty_permille() != 800 {
            return Err(XC_FALLBACK_UNMARKED);
        }
        e.annotation.deliver(fb)?;
        // 回退但零跳信息=不透明→0x6F05 拒收。
        let opaque = AnnotatedResult::fallback(0, EngineId::Cpu.idx(), 200);
        if e.annotation.deliver(opaque) != Err(XC_FALLBACK_UNMARKED) {
            return Err(XC_FALLBACK_UNMARKED);
        }
        if e.annotation.opaque_deliveries != 1 {
            return Err(XC_FALLBACK_UNMARKED);
        }
        // 越界落点=不透明。
        let oob = AnnotatedResult::fallback(1, 9, 150);
        if e.annotation.deliver(oob) != Err(XC_FALLBACK_UNMARKED) {
            return Err(XC_FALLBACK_UNMARKED);
        }
        // 原生交付后占比：1 原生+1 回退=5000bp。
        e.annotation.deliver(nat)?;
        if e.annotation.fallback_ratio_bp() != 5000 {
            return Err(XC_FALLBACK_UNMARKED);
        }
        Ok(())
    }

    /// 组六：演练——kill 注入五验证全绿；链断裂案例；复探失败退役案例；
    /// 图级部分失败演练落账。
    pub fn selfcheck_drill(&mut self) -> Result<(), u32> {
        // 案例一：kill NPU→回退 GPU→复探过→五验证全绿。
        let mut e = FailureEngine::new();
        let r = e.kill_drill(DrillCase {
            victim: EngineId::Npu,
            expect_land: Some(EngineId::Gpu),
            reprobe_ok: true,
        })?;
        if !r.all_green() {
            return Err(XC_WALL_BREACH);
        }
        // 案例二：kill DSP→回退 CPU→复探败→退役路径。
        let mut e2 = FailureEngine::new();
        let r2 = e2.kill_drill(DrillCase {
            victim: EngineId::Dsp,
            expect_land: Some(EngineId::Cpu),
            reprobe_ok: false,
        })?;
        if !r2.all_green() {
            return Err(XC_WALL_BREACH);
        }
        if e2.registry.domain(EngineId::Dsp).state != EngineState::Retired {
            return Err(XC_WALL_BREACH);
        }
        // 案例三：kill CPU→链断裂显性（expect_land=None 走 0x6F02 分支）。
        let mut e3 = FailureEngine::new();
        let r3 = e3.kill_drill(DrillCase {
            victim: EngineId::Cpu,
            expect_land: None,
            reprobe_ok: true,
        })?;
        if !r3.all_green() {
            return Err(XC_WALL_BREACH);
        }
        // 案例登记上限。
        let mut e4 = FailureEngine::new();
        let mut k = 0;
        while k < MAX_DRILL_CASES {
            e4.enroll(DrillCase {
                victim: EngineId::Npu,
                expect_land: Some(EngineId::Gpu),
                reprobe_ok: true,
            })?;
            k += 1;
        }
        if e4.enroll(DrillCase {
            victim: EngineId::Npu,
            expect_land: Some(EngineId::Gpu),
            reprobe_ok: true,
        }).is_ok() {
            return Err(XC_WALL_BREACH); // 超上限必须拒
        }
        // 图级部分失败：NPU 崩，NPU 节点降级可用、GPU 节点全量可用→Partial。
        let mut e5 = FailureEngine::new();
        let plans = [
            (0usize, EngineId::Npu),
            (1usize, EngineId::Gpu),
            (2usize, EngineId::Npu),
        ];
        let outcome = e5.graph_partial_drill(&plans, EngineId::Npu)?;
        if outcome != GraphOutcome::Partial {
            return Err(XC_ANNOT_MISSING);
        }
        if e5.partial.alive_count() != 3 {
            return Err(XC_ANNOT_MISSING); // 降级可用也算活口
        }
        if e5.partial.consume_decision(0)? != Availability::Degraded {
            return Err(XC_ANNOT_MISSING);
        }
        if e5.partial.consume_decision(1)? != Availability::Full {
            return Err(XC_ANNOT_MISSING);
        }
        // 回退落点标注链可审计（重路由账=2 次 NPU 节点回退）。
        if e5.table.rerouted_tasks != 2 {
            return Err(XC_ANNOT_MISSING);
        }
        Ok(())
    }

    /// 全量自检入口（六组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_isolation()?;
        self.selfcheck_fallback_table()?;
        self.selfcheck_partial()?;
        self.selfcheck_reset()?;
        self.selfcheck_annotation()?;
        self.selfcheck_drill()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x6F00..0x7000 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_WALL_BREACH,
            XC_CHAIN_BROKEN,
            XC_ANNOT_MISSING,
            XC_RESET_OVERREACH,
            XC_FALLBACK_UNMARKED,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x6F00 || codes[i] >= 0x7000 {
                return Err(XC_WALL_BREACH);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_WALL_BREACH);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
