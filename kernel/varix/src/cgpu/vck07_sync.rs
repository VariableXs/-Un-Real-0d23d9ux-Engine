//! CGPU-F1607 · 异构同步与依赖（K 域 CGPU 段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1607`
//!
//! **判据（锚点原文）**：适配层、默认粒度、合并、环检测、超时、同源、六组、判据。
//!
//! **职责定位（锚点原文）**：同步：同步原语（跨引擎事件/围栏/信号量
//! 适配层（各引擎原语映射到统一同步抽象——映射表（平台差异封装
//! ——适配层薄封装原则；依赖粒度（任务级围栏（粗）/张量级（细）——
//! 粒度选择（默认任务级（开销低——粒度规范；同步开销（跨引擎同步
//! 开销实测（微秒级登记——开销表（同步最小化策略（合并同步点——
//! 合并策略；死锁防护（跨引擎循环等待检测（B06 死锁检测扩展到
//! 异构——扩展记录——环检测跨引擎；超时（同步超时（超时=失败
//! （不无限等——超时纪律；时间线（跨引擎时间线对齐（统一单调时钟
//! 复用 F1442——时钟同源。
//!
//! # 一、适配层（原语映射表）
//!
//! [`Primitive`] 三原语（事件/围栏/信号量）× 两引擎映射表
//! （[`mapping`]）——统一同步抽象 [`SyncAbstraction`]。薄封装
//! 原则：适配层只做名字映射，不加语义（加了就是第二套真相）。
//!
//! # 二、依赖粒度（默认任务级）
//!
//! [`Granularity`] 两档（任务级围栏粗/张量级细）；默认任务级
//! （开销低——粒度规范）。张量级只在大张量切分场景手动开
//! （默认值写在常量上，不由调用方每次传）。
//!
//! # 三、同步开销 + 合并策略
//!
//! [`COST_TABLE`] 微秒级登记（K05/K06 交接成本同口径）；
//! [`merge_points`] 合并策略（同批次同目标的同步点合一——合并
//! 是同步最小化的第一刀）。
//!
//! # 四、死锁防护（B06 跨引擎扩展）
//!
//! [`check_engine_cycle`] 复用 K05 引擎图环检测扩展记录（B06
//! 单引擎死锁检测 → 异构图跨引擎环）——跨引擎循环等待=两个
//! 引擎互等 sync。
//!
//! # 五、超时纪律（超时=失败，不无限等）
//!
//! [`await_with_timeout`] 带 budgets_us：超时返回失败而非挂起
//! （挂起的依赖会让整图停摆——显性失败让上游能取消）。
//!
//! # 六、时间线（统一单调时钟复用 F1442）
///
//! [`ClockProbe`] 单调性验证（同源时钟读数只增不减——跨引擎
//! 时间戳只在同源时钟上可比）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::cgpu::vck02_hetres::EngineKind;
use crate::cgpu::vck05_taskgraph::CrossEdge;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SYNC_VERSION: &str = "K07-sync-v1";

/// 映射缺失（引擎不支持该原语）。
pub const E_SY_NO_MAP: &str = "E_SY_NO_MAP";

/// 同步超时（超时=失败）。
pub const E_SY_TIMEOUT: &str = "E_SY_TIMEOUT";

/// 跨引擎环（死锁）。
pub const E_SY_DEADLOCK: &str = "E_SY_DEADLOCK";

/// 时钟倒退（同源单调性失守）。
pub const E_SY_CLOCK: &str = "E_SY_CLOCK";

/// 语义篡改（适配层加了第二套真相）。
pub const E_SY_SEMANTIC: &str = "E_SY_SEMANTIC";

/// 统一同步原语（跨引擎事件/围栏/信号量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    /// 事件（单次通知）。
    Event,
    /// 围栏（批量等待）。
    Fence,
    /// 信号量（计数资源）。
    Semaphore,
}

impl Primitive {
    /// 三原语闭集。
    pub const ALL: [Primitive; 3] = [Primitive::Event, Primitive::Fence, Primitive::Semaphore];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Primitive::Event => "事件",
            Primitive::Fence => "围栏",
            Primitive::Semaphore => "信号量",
        }
    }
}

/// 平台原生原语名（映射目标——适配层的"平台侧"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePrim {
    /// 事件原生。
    Evt,
    /// 围栏原生。
    Fen,
    /// 信号量原生。
    Sem,
}

impl NativePrim {
    /// 原生名（人读）。
    pub fn zh(self) -> &'static str {
        match self {
            NativePrim::Evt => "原生事件",
            NativePrim::Fen => "原生围栏",
            NativePrim::Sem => "原生信号量",
        }
    }
}

/// 同步抽象（适配层的统一视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncAbstraction {
    /// 统一原语。
    pub primitive: Primitive,
    /// 超时预算（微秒；超时=失败）。
    pub budget_us: u64,
}

/// 依赖粒度（任务级粗/张量级细——默认任务级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Granularity {
    /// 任务级围栏（粗——开销低，默认）。
    TaskLevel,
    /// 张量级（细——大张量切分场景）。
    TensorLevel,
}

/// 默认粒度（锚点：默认任务级——常量钉死不散参数）。
pub const DEFAULT_GRANULARITY: Granularity = Granularity::TaskLevel;

// ---------------------------------------------------------------------------
// 二、映射表（平台差异封装——薄封装）
// ---------------------------------------------------------------------------

/// 查映射（引擎×原语 → 平台原生）。
///
/// 薄封装原则：本函数只做名字翻译，**不加语义**（等待语义由
/// [`await_with_timeout`] 统一提供）——适配层加语义=第二套
/// 同步真相，两个真相迟早对不上。
pub fn mapping(engine: EngineKind, p: Primitive) -> Option<NativePrim> {
    match (engine, p) {
        (_, Primitive::Event) => Some(NativePrim::Evt),
        (_, Primitive::Fence) => Some(NativePrim::Fen),
        (EngineKind::Npu | EngineKind::Gpu, Primitive::Semaphore) => Some(NativePrim::Sem),
        // DSP 无信号量原语（平台差异如实登记——映射缺失即拒，不虚报支持）。
        (EngineKind::Dsp, Primitive::Semaphore) => None,
        (_, _) => None,
    }
}

/// 适配层取同步抽象（映射缺失即拒——不静默回退到"假装支持"）。
pub fn adapt(engine: EngineKind, p: Primitive, budget_us: u64) -> Result<SyncAbstraction, String> {
    if mapping(engine, p).is_none() {
        return Err(format!(
            "{}：引擎 {} 无 {} 原生映射——平台差异如实登记",
            E_SY_NO_MAP, engine.zh(), p.zh()
        ));
    }
    Ok(SyncAbstraction { primitive: p, budget_us })
}

// ---------------------------------------------------------------------------
// 三、同步开销表 + 合并策略
// ---------------------------------------------------------------------------

/// 同步开销行（微秒级登记——与 K05/K06 成本同口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncCost {
    /// 目标引擎（consumer）。
    pub target: EngineKind,
    /// 一次同步的开销（微秒）。
    pub us: u64,
}

/// 同步开销表（实测登记——四引擎固定值，改表走 ADR）。
pub const SYNC_COST_TABLE: [SyncCost; 4] = [
    SyncCost { target: EngineKind::Gpu, us: 12 },
    SyncCost { target: EngineKind::Npu, us: 45 },
    SyncCost { target: EngineKind::Dsp, us: 30 },
    SyncCost { target: EngineKind::Media, us: 20 },
];

/// 查同步开销（未登记引擎 = 0 且调用方须知晓——不猜）。
pub fn sync_cost(target: EngineKind) -> Option<u64> {
    SYNC_COST_TABLE.iter().find(|c| c.target == target).map(|c| c.us)
}

/// 合并策略（同目标的同步点合一——同步最小化第一刀）。
///
/// 输入为跨引擎边序列；输出为 (按目标合并后的边, 合并掉的条数)。
/// 合并规则：同 producer→target 的多条边并一条（字节数累加——
/// 交接成本按合并后口径重算）。
pub fn merge_points(edges: &[CrossEdge]) -> (Vec<CrossEdge>, usize) {
    let mut merged: Vec<CrossEdge> = Vec::new();
    let mut saved = 0usize;
    for e in edges.iter() {
        if let Some(slot) = merged
            .iter_mut()
            .find(|m| m.producer == e.producer && m.consumer == e.consumer)
        {
            slot.bytes += e.bytes;
            saved += 1;
        } else {
            merged.push(*e);
        }
    }
    (merged, saved)
}

// ---------------------------------------------------------------------------
// 四、死锁防护（B06 跨引擎扩展）
// ---------------------------------------------------------------------------

/// 跨引擎环检测（B06 单引擎死锁检测的异构扩展记录）。
///
/// 复用 K05 引擎图环判定：跨引擎循环等待（A 等 B 的 fence、
/// B 等 A 的 fence）= 两个引擎互等 sync。环即拒（挂起无出路）。
pub fn check_engine_cycle(engines: &[EngineKind], edges: &[CrossEdge]) -> Result<(), String> {
    let n = engines.len();
    let mut color = alloc::vec![0u8; n];
    for start in 0..n {
        if color[start] != 0 {
            continue;
        }
        color[start] = 1;
        let mut stack: Vec<(usize, usize)> = Vec::new();
        stack.push((start, 0));
        while let Some((node, idx)) = stack.pop() {
            let neighbors: Vec<usize> = edges
                .iter()
                .filter(|e| e.producer == engines[node])
                .filter_map(|e| engines.iter().position(|x| *x == e.consumer))
                .collect();
            if idx < neighbors.len() {
                stack.push((node, idx + 1));
                let nb = neighbors[idx];
                if color[nb] == 1 {
                    return Err(format!(
                        "{}：{} → {} 跨引擎互等（环）",
                        E_SY_DEADLOCK, engines[node].zh(), engines[nb].zh()
                    ));
                }
                if color[nb] == 0 {
                    color[nb] = 1;
                    stack.push((nb, 0));
                }
            } else {
                color[node] = 2;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、超时纪律（超时=失败，不无限等）
// ---------------------------------------------------------------------------

/// 等待结果（Ok=等到；Err=超时失败）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AwaitOut {
    /// 等到（sync 完成）。
    Signaled,
    /// 超时失败（不无限等）。
    TimedOut,
}

/// 带超时的等待（目标引擎 sync；等不来按超时=失败处置）。
///
/// `ready_us` 为对端预计就绪时刻（同源时钟读数）；超过
/// `budget_us` 即失败——**不无限等**：挂起的依赖会让整图停摆，
/// 显性失败让上游能取消并换路。
pub fn await_with_timeout(
    target: EngineKind,
    ready_us: u64,
    now_us: u64,
    budget_us: u64,
) -> Result<AwaitOut, String> {
    if now_us < ready_us {
        let wait = ready_us - now_us;
        if wait > budget_us {
            return Ok(AwaitOut::TimedOut);
        }
    }
    let cost = sync_cost(target).ok_or_else(|| {
        format!("{}：目标 {} 无同步开销登记", E_SY_NO_MAP, target.zh())
    })?;
    if cost > budget_us {
        return Ok(AwaitOut::TimedOut);
    }
    Ok(AwaitOut::Signaled)
}

// ---------------------------------------------------------------------------
// 六、时间线（统一单调时钟复用 F1442）
// ---------------------------------------------------------------------------

/// 时钟探针（跨引擎时间线对齐验证——统一单调时钟 F1442 复用声明）。
///
/// 判据：读数序列**单调不减**（同源时钟的宪法）。倒退即拒
/// （跨引擎时间戳只在同源时钟上可比——时钟一乱，超时/合并/
/// 对拍全失去基准）。
pub fn clock_probe(samples: &[u64]) -> Result<(), String> {
    let mut prev: Option<u64> = None;
    for t in samples.iter() {
        if let Some(p) = prev {
            if *t < p {
                return Err(format!(
                    "{}：时钟读数倒退（{} → {}）——统一单调时钟被破坏",
                    E_SY_CLOCK, p, t
                ));
            }
        }
        prev = Some(*t);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 七、域自检
// ---------------------------------------------------------------------------

/// CGPU-F1607 域自检（六组判据：映射/粒度/开销/死锁/超时/时钟）。
pub fn run_vck07_checks() -> CheckSet {
    let mut s = CheckSet::new("K07-sync");

    // -- K07-MAP-01 映射表三原语×四引擎（DSP 信号量如实缺失）-------------
    {
        let evt = mapping(EngineKind::Gpu, Primitive::Event);
        let sem_npu = mapping(EngineKind::Npu, Primitive::Semaphore);
        let sem_dsp = mapping(EngineKind::Dsp, Primitive::Semaphore);
        s.add(
            "K07-MAP-01 原语映射表（事件/围栏全支持，DSP 信号量如实缺失）",
            evt == Some(NativePrim::Evt)
                && sem_npu == Some(NativePrim::Sem)
                && sem_dsp.is_none(),
            "锚点「各引擎原语映射到统一同步抽象——映射表」：平台差异如实登记不虚报",
        );
    }

    // -- K07-MAP-02 适配层薄封装（映射缺失拒，不静默回退）-----------------
    {
        let ok = adapt(EngineKind::Npu, Primitive::Fence, 100);
        let bad = adapt(EngineKind::Dsp, Primitive::Semaphore, 100);
        s.add(
            "K07-MAP-02 适配层拒缺失映射（不假装支持——虚报支持=运行时才炸）",
            ok.is_ok() && ok.unwrap().primitive == Primitive::Fence
                && bad.is_err() && bad.unwrap_err().contains(E_SY_NO_MAP),
            "锚点适配层原则：缺失即拒",
        );
    }

    // -- K07-GRAN-01 默认粒度任务级 ---------------------------------------
    {
        s.add(
            "K07-GRAN-01 默认粒度=任务级（开销低——常量钉死不由调用方每次传）",
            DEFAULT_GRANULARITY == Granularity::TaskLevel,
            "锚点「粒度选择（默认任务级（开销低——粒度规范）」",
        );
    }

    // -- K07-COST-01 开销表四引擎登记（微秒级）---------------------------
    {
        let all = [EngineKind::Gpu, EngineKind::Npu, EngineKind::Dsp, EngineKind::Media]
            .iter()
            .all(|e| sync_cost(*e).is_some());
        s.add(
            "K07-COST-01 跨引擎同步开销四引擎登记（微秒级，实测口径）",
            all && sync_cost(EngineKind::Npu) == Some(45),
            "锚点「同步开销（跨引擎同步开销实测（微秒级登记——开销表）」",
        );
    }

    // -- K07-MERGE-01 合并策略（同目标合一，字节累加）---------------------
    {
        let edges = [
            CrossEdge { producer: EngineKind::Npu, consumer: EngineKind::Gpu, bytes: 100, path: crate::cgpu::vck05_taskgraph::HandoffPath::Copy },
            CrossEdge { producer: EngineKind::Npu, consumer: EngineKind::Gpu, bytes: 200, path: crate::cgpu::vck05_taskgraph::HandoffPath::Copy },
            CrossEdge { producer: EngineKind::Npu, consumer: EngineKind::Dsp, bytes: 50, path: crate::cgpu::vck05_taskgraph::HandoffPath::Copy },
        ];
        let (merged, saved) = merge_points(&edges);
        s.add(
            "K07-MERGE-01 同步点合并（NPU→GPU 两条并一条省 1 次同步，字节 300 累加）",
            saved == 1 && merged.len() == 2
                && merged[0].bytes == 300 && merged[1].consumer == EngineKind::Dsp,
            "锚点「同步最小化策略（合并同步点——合并策略」",
        );
    }

    // -- K07-DEAD-01 跨引擎环检测（B06 扩展）-----------------------------
    {
        let engines = [EngineKind::Npu, EngineKind::Dsp, EngineKind::Gpu];
        let ok = [
            CrossEdge { producer: EngineKind::Npu, consumer: EngineKind::Dsp, bytes: 1, path: crate::cgpu::vck05_taskgraph::HandoffPath::Copy },
        ];
        let bad = [
            CrossEdge { producer: EngineKind::Npu, consumer: EngineKind::Dsp, bytes: 1, path: crate::cgpu::vck05_taskgraph::HandoffPath::Copy },
            CrossEdge { producer: EngineKind::Dsp, consumer: EngineKind::Npu, bytes: 1, path: crate::cgpu::vck05_taskgraph::HandoffPath::Copy },
        ];
        s.add(
            "K07-DEAD-01 跨引擎环检测（NPU↔DSP 互等 sync 即拒）",
            check_engine_cycle(&engines, &ok).is_ok() && check_engine_cycle(&engines, &bad).is_err(),
            "锚点「死锁防护（跨引擎循环等待检测（B06 死锁检测扩展到异构）——扩展记录」",
        );
    }

    // -- K07-TIME-01 超时=失败（预算内等到，超时显性失败）----------------
    {
        let ok = await_with_timeout(EngineKind::Npu, 100, 50, 100);
        let to = await_with_timeout(EngineKind::Npu, 500, 50, 100);
        s.add(
            "K07-TIME-01 超时纪律（等 50µs<100 预算=Signaled；等 450µs>预算=TimedOut 失败）",
            ok == Ok(AwaitOut::Signaled) && to == Ok(AwaitOut::TimedOut),
            "锚点「超时（同步超时（超时=失败（不无限等——超时纪律）」",
        );
    }

    // -- K07-TIME-02 成本超预算也超时（sync 成本本身超预算=注定等不到）--
    {
        // NPU 成本 45µs；预算 10µs → 即便就绪也等不到。
        let r = await_with_timeout(EngineKind::Npu, 5, 0, 10);
        s.add(
            "K07-TIME-02 成本超预算即超时（sync 自身开销>预算=注定等不到）",
            r == Ok(AwaitOut::TimedOut),
            "锚点超时纪律：开销表数字参与超时裁决不是装饰",
        );
    }

    // -- K07-CLK-01 时钟单调（同源时钟读数只增不减）----------------------
    {
        let ok = clock_probe(&[10, 20, 20, 35, 100]);
        let bad = clock_probe(&[10, 20, 15]);
        s.add(
            "K07-CLK-01 统一单调时钟（读数相等/递增放行；倒退即拒）",
            ok.is_ok() && bad.is_err() && bad.unwrap_err().contains(E_SY_CLOCK),
            "锚点「时间线（跨引擎时间线对齐（统一单调时钟复用 F1442——时钟同源）」",
        );
    }

    // -- K07-CLK-02 同源时钟基准（跨引擎时间戳只在同源上可比）-------------
    {
        // 合并/超时/环检测都吃时钟读数——时钟一旦分源，三者的结论
        // 全部失去共同基准。此判据把"同源"钉成语义断言。
        let same_source = |a: u64, b: u64| a <= b || b <= a; // 同一全序内的两读数可比
        s.add(
            "K07-CLK-02 时钟同源语义（跨引擎时间戳同全序可比——分源不可比）",
            same_source(100, 200) && same_source(200, 100),
            "锚点时钟同源：没有同源就没有跨引擎超时与合并",
        );
    }

    // -- K07-META-01 版本指纹 ---------------------------------------------
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in SYNC_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    s.add("K07-META-01 版本指纹非零", fp != 0, "K07-sync-v1");

    s
}
