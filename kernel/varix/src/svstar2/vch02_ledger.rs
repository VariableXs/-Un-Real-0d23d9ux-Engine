//! CGPU-F1122 · 显存记账（CGPU-H 域 · VH02 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1122`
//!
//! **判据（锚点原文）**：全量埋点、开销、查询、账单联动、判据。
//!
//! # 一、全分配点埋点：显存分配/释放全量记账
//!
//! 复用 B05 记账理念 GPU 化：每一个显存分配/释放都产生一条
//! [`LedgerOp`]（单调 seq 全序），分配点以编译期注册的 site 标签
//! （[`SITE_REGISTRY`]）埋点——记账不是事后推测而是事件驱动。
//! **分配点栈回溯采样**：每 [`SAMPLE_EVERY_N`] 次分配采样一次调用
//! 栈（上限 [`STACK_DEPTH`] 层），采样命中记入 site 报告——栈回溯
//! 全量做太贵，采样即够定位（诚实开销口径）。
//!
//! # 二、记账开销 <1%
//!
//! 每条记账 = 定长字段写入 + 余额/峰值 O(1) 更新（[`OP_COST_STEPS`]
//! 步确定性成本模型）；开销账面（[`OverheadEntry`]）入册，实测由真机
//! 回填，未回填不虚报达标。
//!
//! # 三、账本查询：任意池任意时刻显存可查（12 章透明）
//!
//! [`VramLedger::query_at`]：给定池与 seq 上限，重放账本到该时刻给出
//! 余额——任意时刻可查；实时余额 [`VramLedger::live`] 与 O(1) 维护的
//! 峰值 [`VramLedger::peak`] 并行提供。重放与实时维护双源一致是判据
//! （H02-查询-双源一致）。
//!
//! # 四、账本与 C 域账单联动（F0419 显存维度数据源）
//!
//! [`export_bill`] 导出 [`BillFeedV1`]（版本冻结）：池/累计分配/活跃
//! 字节/峰值/操作数五字段——C 域账单（F0419）按此结构消费显存维度；
//! 字段与版本变动必须走 ADR（与 80 帧合同同纪律）。
//!
//! 布线：记账层包住 vch01 [`PoolSet`] 真实池操作（先池成功后记账），
//! [`VramLedger::reconcile`] 对账双面（记账余额 vs 池自记 used）。

use crate::checks::CheckSet;
use crate::svstar2::vch01_budgetpool::{PoolKind, PoolSet};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码：H02 续编（vch01 占 0x5001-0x5006）。
// ---------------------------------------------------------------------------

/// 账本参数非法（site 未注册 / 池越界）。
pub const ERR_SITE_UNKNOWN: u16 = 0x5007;
/// 释放记账与池释放结果不一致（配对断裂）。
pub const ERR_FREE_UNPAIRED: u16 = 0x5008;
/// 对账失败（记账余额 vs 池自记 used）。
pub const ERR_RECONCILE: u16 = 0x5009;
/// 账单导出参数非法。
pub const ERR_BILL_ARG: u16 = 0x500A;

// ---------------------------------------------------------------------------
// 分配点注册表与栈回溯采样
// ---------------------------------------------------------------------------

/// 栈回溯采样周期（每 N 次分配采一次）。
pub const SAMPLE_EVERY_N: u32 = 64;

/// 单次回溯最大深度。
pub const STACK_DEPTH: usize = 8;

/// 单条记账确定性成本（步）；对照分配主路径成本模型估开销。
pub const OP_COST_STEPS: u32 = 3;

/// 单次分配主路径确定性成本（步；含页表/桥接/校验建模）。
pub const ALLOC_COST_STEPS: u32 = 512;

/// 分配点注册表（编译期闭集；埋点必须引用在册 site）。
pub const SITE_REGISTRY: [&str; 8] = [
    "tex-upload",   // 纹理上传（F0010 带宽调度侧）
    "tex-decode",   // 纹理解码输出
    "render-target",// 渲染目标
    "swap-chain",   // 交换链
    "mesh-vb",      // 网格顶点缓冲
    "mesh-ib",      // 网格索引缓冲
    "video-frame",  // 视频帧（F0017 路径）
    "misc-pool",    // 杂项池
];

/// site 名 → id（未注册返回 None；埋点侧拒绝未知分配点）。
pub fn site_id(name: &str) -> Option<u8> {
    SITE_REGISTRY.iter().position(|s| *s == name).map(|i| i as u8)
}

/// 账本操作种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    /// 分配。
    Alloc,
    /// 释放。
    Free,
}

/// 采样到的调用栈快照（站点相对；深度上限 [`STACK_DEPTH`]）。
#[derive(Clone, Debug)]
pub struct StackSample {
    /// 触发采样的分配 seq。
    pub at_seq: u32,
    /// site id。
    pub site: u8,
    /// 回溯帧（调用点标签序列，建模为 site 链）。
    pub frames: Vec<u8>,
}

/// 单条账本操作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerOp {
    /// 全序号（单调递增，从 1 起）。
    pub seq: u32,
    /// 操作种类。
    pub kind: OpKind,
    /// 池。
    pub pool: PoolKind,
    /// 所有者（进程/任务 id）。
    pub owner: u32,
    /// 字节数。
    pub bytes: u64,
    /// 分配点 site id（Free 记录沿用分配时的 site）。
    pub site: u8,
}

// ---------------------------------------------------------------------------
// 账本本体
// ---------------------------------------------------------------------------

/// site 聚合报告。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SiteReport {
    /// 该 site 累计分配字节数。
    pub total_alloc_bytes: u64,
    /// 该 site 当前活跃字节数。
    pub live_bytes: u64,
    /// 该 site 分配次数。
    pub alloc_count: u32,
    /// 该 site 被采样到的栈次数。
    pub sample_count: u32,
}

/// 显存账本：全量记账 + O(1) 实时余额 + 任意时刻重放查询。
#[derive(Debug)]
pub struct VramLedger {
    /// 全量操作账（单调 seq）。
    ops: Vec<LedgerOp>,
    /// 各池实时余额（下标 = PoolKind as usize）。
    balance: [u64; 3],
    /// 各池峰值（不随回退抹除，与 vch01 纪律一致）。
    peak: [u64; 3],
    /// 各 site 聚合。
    sites: [SiteReport; 8],
    /// 分配计数（采样节拍用）。
    alloc_tick: u32,
    /// 采样栈快照。
    samples: Vec<StackSample>,
}

impl VramLedger {
    /// 新账本。
    pub fn new() -> Self {
        VramLedger {
            ops: Vec::new(),
            balance: [0; 3],
            peak: [0; 3],
            sites: [SiteReport::default(); 8],
            alloc_tick: 0,
            samples: Vec::new(),
        }
    }

    /// 已记账操作数。
    pub fn op_count(&self) -> u32 {
        self.ops.len() as u32
    }

    /// 全量账只读视图（12 章透明：账本本身可审计）。
    pub fn ops(&self) -> &[LedgerOp] {
        &self.ops
    }

    /// 分配记账：池操作成功后调用；site 未注册拒绝（埋点纪律）。
    /// 返回 seq。栈回溯按 [`SAMPLE_EVERY_N`] 节拍采样。
    pub fn record_alloc(
        &mut self,
        pool: PoolKind,
        owner: u32,
        bytes: u64,
        site: u8,
        stack: Option<&[u8]>,
    ) -> Result<u32, u16> {
        if site as usize >= SITE_REGISTRY.len() {
            return Err(ERR_SITE_UNKNOWN);
        }
        let seq = self.ops.len() as u32 + 1;
        self.ops.push(LedgerOp { seq, kind: OpKind::Alloc, pool, owner, bytes, site });
        let p = pool as usize;
        self.balance[p] += bytes;
        if self.balance[p] > self.peak[p] {
            self.peak[p] = self.balance[p];
        }
        let sr = &mut self.sites[site as usize];
        sr.total_alloc_bytes += bytes;
        sr.live_bytes += bytes;
        sr.alloc_count += 1;
        // 栈回溯采样节拍。
        self.alloc_tick += 1;
        if self.alloc_tick % SAMPLE_EVERY_N == 0 {
            let frames: Vec<u8> = stack.unwrap_or(&[]).iter().take(STACK_DEPTH).copied().collect();
            self.samples.push(StackSample { at_seq: seq, site, frames });
            self.sites[site as usize].sample_count += 1;
        }
        Ok(seq)
    }

    /// 释放记账：`bytes` 必须与该池该 owner 的活跃分配配对（由调用方
    /// 从池 handle 查得）；未配对（余额不足扣减）拒绝。
    pub fn record_free(
        &mut self,
        pool: PoolKind,
        owner: u32,
        bytes: u64,
        site: u8,
    ) -> Result<u32, u16> {
        if site as usize >= SITE_REGISTRY.len() {
            return Err(ERR_SITE_UNKNOWN);
        }
        let p = pool as usize;
        if self.balance[p] < bytes {
            return Err(ERR_FREE_UNPAIRED);
        }
        // site 级活跃也必须够扣（防跨 site 错配）。
        if self.sites[site as usize].live_bytes < bytes {
            return Err(ERR_FREE_UNPAIRED);
        }
        let seq = self.ops.len() as u32 + 1;
        self.ops.push(LedgerOp { seq, kind: OpKind::Free, pool, owner, bytes, site });
        self.balance[p] -= bytes;
        self.sites[site as usize].live_bytes -= bytes;
        Ok(seq)
    }

    /// 实时余额（任意池任意时刻的「现在」）。
    pub fn live(&self, pool: PoolKind) -> u64 {
        self.balance[pool as usize]
    }

    /// 峰值（不随回退抹除）。
    pub fn peak(&self, pool: PoolKind) -> u64 {
        self.peak[pool as usize]
    }

    /// 任意时刻查询：重放账本到 `upto_seq`（含）给出池余额。
    /// 双源一致判据的查询面（12 章透明）。
    pub fn query_at(&self, pool: PoolKind, upto_seq: u32) -> Result<u64, u16> {
        if upto_seq == 0 || upto_seq > self.ops.len() as u32 {
            return Err(ERR_BILL_ARG);
        }
        let mut bal = 0u64;
        for op in &self.ops {
            if op.seq > upto_seq {
                break;
            }
            if op.pool != pool {
                continue;
            }
            match op.kind {
                OpKind::Alloc => bal += op.bytes,
                OpKind::Free => bal -= op.bytes,
            }
        }
        Ok(bal)
    }

    /// site 报告（site 越界返回 None）。
    pub fn site_report(&self, site: u8) -> Option<SiteReport> {
        self.sites.get(site as usize).copied()
    }

    /// 采样栈列表（只读）。
    pub fn stack_samples(&self) -> &[StackSample] {
        &self.samples
    }

    /// 对账：记账余额 vs 池自记 used（双面一致）。
    pub fn reconcile(&self, set: &PoolSet) -> Result<(), u16> {
        for kind in [PoolKind::Process, PoolKind::System, PoolKind::Reserved] {
            if self.live(kind) != set.pool(kind).used() {
                return Err(ERR_RECONCILE);
            }
        }
        Ok(())
    }

    /// 记账开销（万分比）：确定性成本模型 OP/(OP+主路径)。
    /// 3/(3+512) ≈ 58bp < 100bp（1%）。
    pub fn overhead_bp(&self) -> u32 {
        OP_COST_STEPS * 10_000 / (OP_COST_STEPS + ALLOC_COST_STEPS)
    }
}

impl Default for VramLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// C 域账单联动（F0419 显存维度数据源；版本冻结）
// ---------------------------------------------------------------------------

/// 账单结构版本（变动走 ADR）。
pub const BILL_VERSION: u32 = 1;

/// C 域账单显存维度数据源（F0419 消费面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BillFeedV1 {
    /// 结构版本（恒 1）。
    pub version: u32,
    /// 池。
    pub pool: PoolKind,
    /// 累计分配字节数（全量埋点口径）。
    pub allocated_total: u64,
    /// 当前活跃字节数。
    pub live_bytes: u64,
    /// 峰值字节数。
    pub peak_bytes: u64,
    /// 账本操作数。
    pub ops_count: u32,
}

/// 导出某池账单（数据源一致性由判据保证：live/peak 直取账本）。
pub fn export_bill(ledger: &VramLedger, pool: PoolKind) -> BillFeedV1 {
    let mut allocated_total = 0u64;
    let mut ops_count = 0u32;
    for op in &ledger.ops {
        if op.pool == pool {
            ops_count += 1;
            if op.kind == OpKind::Alloc {
                allocated_total += op.bytes;
            }
        }
    }
    BillFeedV1 {
        version: BILL_VERSION,
        pool,
        allocated_total,
        live_bytes: ledger.live(pool),
        peak_bytes: ledger.peak(pool),
        ops_count,
    }
}

// ---------------------------------------------------------------------------
// 记账开销账面（入 CGPU-Bench）
// ---------------------------------------------------------------------------

/// 开销账面条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OverheadEntry {
    /// 条目名。
    pub name: &'static str,
    /// 目标（万分比；<1% = 100bp）。
    pub target_bp: u32,
    /// 实测万分比（None = 真机未回填，不虚报）。
    pub measured_bp: Option<u32>,
}

impl OverheadEntry {
    /// 目标是否达成（未回填 = false）。
    pub fn target_met(&self) -> bool {
        match self.measured_bp {
            Some(v) => v <= self.target_bp,
            None => false,
        }
    }
}

/// 本件开销账面。
pub const OVERHEAD_LEDGER: [OverheadEntry; 1] =
    [OverheadEntry { name: "vram-ledger-overhead", target_bp: 100, measured_bp: None }];

/// 本件判据聚合（由 checks 文件实现）。
pub fn run_vch02_checks() -> CheckSet {
    crate::svstar2::vch02_ledger_checks::run_vch02_checks()
}
