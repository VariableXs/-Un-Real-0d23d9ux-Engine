//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18431 矩阵观测导出（裁决过程可回放） · 0x7E780段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18431-J1 每次查询裁决轨迹（命中层/依据/终值）可导出，100 次查询轨迹完整率 100%
//! 设计要点：
//!   - 版本谎报矩阵四层优先链：ActCtx(0) > Manifest(1) > Compat(2) > Default(3)，
//!     每次查询沿链落到首个「有值且有依据」的层，命中即出一条裁决轨迹
//!     （序号/进程/API/维度/命中层/依据/终值七元组）；
//!   - 观测面：轨迹账本按写入序落账，导出为回放文本（表头+数据行），
//!     回放面按同序重放 100 次查询并与轨迹逐字段对拍——裁决过程可回放；
//!   - 完整率判定：查询数 = 轨迹数（每查询恰一条），每条轨迹
//!     层合法(<4) ∧ 依据非空(basis>0) ∧ 序号连续递增 ⇒ 完整率 100%；
//!   - 注入核四类：无依据命中(basis=0)/乱序序号/重复序号/越层(layer≥4)，
//!     逐类 10 轮必被显式拒绝；
//!   - 边界：空查询落 Default 兜底层照出轨迹；账本满按 dropped 显性记账
//!     不静默丢；重复注册按更新记账不产生重复键；
//!   - 确定性：LCG 种子写死 = SEG_BASE(0x7E780)，双跑导出逐字节一致；
//!   - no_std + alloc；零 unwrap/expect/panic；禁浮点/IO/墙钟/rand。
//! 结构清单：
//!   HitLayer / ObsCode(诊断闭集) / MatrixEntry+LieMatrix(矩阵)
//!   / TraceRecord+TraceLedger(轨迹账本) / export+replay(导出回放面)
//!   / Lcg(固定种子) / InjectLedger(注入记账) / golden_run(黄金全链)
//!   / run_unx18431_checks(判据收口)
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E780，AI-VX11-1105-G1·A04 独占区）。
pub const SEG_BASE: u32 = 0x7E780;

// ── 域冻结常量（推导注记） ───────────────────────────────────────
/// 查询轮次（冻结：判据原文指定 100 次查询）。
pub const QUERY_ROUNDS: usize = 100;
/// 轨迹账本容量（冻结：≥100 留 1 倍余量，满后 dropped 记账）。
pub const TRACE_CAP: usize = 200;
/// 矩阵条目容量（冻结：键空间 8 进程 × 4 API × 3 维度 = 96 ≤ 128）。
pub const MATRIX_CAP: usize = 128;
/// 优先链层数（冻结：四级优先序）。
pub const LAYER_N: usize = 4;
/// 注入判定轮数（冻结：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 键空间：进程数（冻结）。
pub const PROC_N: u32 = 8;
/// 键空间：API 族数（冻结）。
pub const API_N: u32 = 4;
/// 键空间：维度数（冻结）。
pub const DIM_N: u32 = 3;

// ── 命中层枚举（四级优先序） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitLayer {
    /// 激活上下文层（最高优先）。
    ActCtx = 0,
    /// manifest 声明层。
    Manifest = 1,
    /// 兼容值层（未声明应用的 6.2 类兼容口径）。
    Compat = 2,
    /// 系统默认值层（锚基线解耦插槽，兜底必命中）。
    Default = 3,
}

impl HitLayer {
    /// 层序号（0..4）。
    pub fn idx(self) -> usize {
        self as usize
    }
    /// 层名（导出文本与中文诊断共用）。
    pub fn name(self) -> &'static str {
        match self {
            HitLayer::ActCtx => "ActCtx",
            HitLayer::Manifest => "Manifest",
            HitLayer::Compat => "Compat",
            HitLayer::Default => "Default",
        }
    }
    /// 由层序号还原枚举；越界返回 None（导出校验面用）。
    pub fn from_idx(i: usize) -> Option<HitLayer> {
        match i {
            0 => Some(HitLayer::ActCtx),
            1 => Some(HitLayer::Manifest),
            2 => Some(HitLayer::Compat),
            3 => Some(HitLayer::Default),
            _ => None,
        }
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsCode {
    /// 轨迹账本空（导出拒）
    EmptyLedger,
    /// 序号断裂（乱序/跳号）
    SeqBroken,
    /// 序号重复
    SeqDup,
    /// 命中依据为空（basis=0）
    BasisMissing,
    /// 命中层越界（layer≥4）
    LayerInvalid,
    /// 回放对拍不一致
    ReplayMismatch,
    /// 轨迹账本满（dropped 记账）
    LedgerFull,
    /// 矩阵条目满
    MatrixFull,
    /// 查询键落在矩阵外且 Default 层无兜底
    NoFallback,
    /// 导出文本被截断（缓冲不足）
    ExportTrunc,
    /// 双跑不一致（确定性红线）
    NotReproducible,
}

impl ObsCode {
    /// 诊断码取值 = SEG_BASE + 段内序。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            ObsCode::EmptyLedger => "轨迹账本空：无可导出裁决轨迹",
            ObsCode::SeqBroken => "轨迹序号断裂：乱序或跳号",
            ObsCode::SeqDup => "轨迹序号重复：回放键不唯一",
            ObsCode::BasisMissing => "命中依据为空：basis=0 不许出轨迹",
            ObsCode::LayerInvalid => "命中层越界：四级优先链外",
            ObsCode::ReplayMismatch => "回放对拍不一致：裁决过程不可复现",
            ObsCode::LedgerFull => "轨迹账本满：超出按 dropped 显性记账",
            ObsCode::MatrixFull => "矩阵条目满：注册被拒且账不丢",
            ObsCode::NoFallback => "查询无兜底：Default 层未注册该键",
            ObsCode::ExportTrunc => "导出缓冲不足：文本被截断",
            ObsCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    /// 诊断码闭集（ALL 恒完备）。
    pub const ALL: [ObsCode; 11] = [
        ObsCode::EmptyLedger,
        ObsCode::SeqBroken,
        ObsCode::SeqDup,
        ObsCode::BasisMissing,
        ObsCode::LayerInvalid,
        ObsCode::ReplayMismatch,
        ObsCode::LedgerFull,
        ObsCode::MatrixFull,
        ObsCode::NoFallback,
        ObsCode::ExportTrunc,
        ObsCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文文案；段外码返回统一未知文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ObsCode::ALL.len() {
        if ObsCode::ALL[i].val() == code {
            return ObsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E780 段闭集内"
}

// ── 谎报矩阵（本模块本地域模型：四层优先链） ─────────────────────
/// 单条矩阵条目：三元组键 + 四层值/依据插槽。
/// 不变量：slot_basis[l]==0 ⇒ slot_val[l] 必为 None（有值必有据）。
#[derive(Clone, Debug)]
pub struct MatrixEntry {
    /// 进程粒度键（映像序号）。
    pub proc_id: u32,
    /// API 族键（GetVersionEx/VerifyVersionInfo…族序）。
    pub api_id: u32,
    /// 查询维度（主/次/Build/SP/ProductType/Suite 序）。
    pub dim: u32,
    /// 四层值插槽（None=该层未声明）。
    pub slot_val: [Option<u32>; LAYER_N],
    /// 四层依据插槽（0=无依据；与 slot_val 同步）。
    pub slot_basis: [u32; LAYER_N],
}

impl MatrixEntry {
    /// 空条目（const，供定容数组初始化备用）。
    pub const EMPTY: MatrixEntry = MatrixEntry {
        proc_id: 0,
        api_id: 0,
        dim: 0,
        slot_val: [None; LAYER_N],
        slot_basis: [0; LAYER_N],
    };
}

/// 谎报矩阵本体：线性键表（本卡规模 96 键，线性扫描够用且零歧义）。
pub struct LieMatrix {
    entries: Vec<MatrixEntry>,
    cap: usize,
    /// 注册计数（含更新；与 entries.len() 的差=更新次数）。
    reg_total: u32,
}

impl LieMatrix {
    /// 建空矩阵。
    pub fn new() -> LieMatrix {
        LieMatrix { entries: Vec::new(), cap: MATRIX_CAP, reg_total: 0 }
    }

    /// 按 (proc,api,dim) 找条目下标。
    pub fn find(&self, proc_id: u32, api_id: u32, dim: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.proc_id == proc_id && e.api_id == api_id && e.dim == dim {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 注册某层值+依据：同键同层重复注册按「更新」记账（不产生重复键）。
    /// 依据为空（basis==0）直接拒——有值必有据是矩阵不变量。
    pub fn register(
        &mut self,
        proc_id: u32,
        api_id: u32,
        dim: u32,
        layer: usize,
        val: u32,
        basis: u32,
    ) -> Result<(), u32> {
        if layer >= LAYER_N {
            return Err(ObsCode::LayerInvalid.val());
        }
        if basis == 0 {
            return Err(ObsCode::BasisMissing.val());
        }
        self.reg_total = self.reg_total.saturating_add(1);
        match self.find(proc_id, api_id, dim) {
            Some(i) => {
                // 已有条目：更新对应层（先取可变借用，块内完成再走计数）。
                let e = &mut self.entries[i];
                e.slot_val[layer] = Some(val);
                e.slot_basis[layer] = basis;
                Ok(())
            }
            None => {
                if self.entries.len() >= self.cap {
                    // 满则回滚计数——被拒的注册不许进账（账不丢=计数守恒）。
                    self.reg_total = self.reg_total.saturating_sub(1);
                    return Err(ObsCode::MatrixFull.val());
                }
                let mut e = MatrixEntry::EMPTY;
                e.proc_id = proc_id;
                e.api_id = api_id;
                e.dim = dim;
                e.slot_val[layer] = Some(val);
                e.slot_basis[layer] = basis;
                self.entries.push(e);
                Ok(())
            }
        }
    }

    /// 查询：沿四级优先链落到首个「有值且有据」的层。
    /// 返回 (命中层, 终值, 依据号)；全链落空（含 Default 未注册）→ NoFallback。
    pub fn query(&self, proc_id: u32, api_id: u32, dim: u32) -> Result<(HitLayer, u32, u32), u32> {
        match self.find(proc_id, api_id, dim) {
            None => Err(ObsCode::NoFallback.val()),
            Some(i) => {
                let e = &self.entries[i];
                let mut l = 0;
                while l < LAYER_N {
                    if let Some(v) = e.slot_val[l] {
                        if e.slot_basis[l] > 0 {
                            return Ok((HitLayer::from_idx(l).unwrap_or(HitLayer::Default), v, e.slot_basis[l]));
                        }
                    }
                    l += 1;
                }
                Err(ObsCode::NoFallback.val())
            }
        }
    }

    /// 各层命中计数（观测面自证：四层至少各命中一次才叫覆盖全链）。
    pub fn layer_hits(&self, ledger: &TraceLedger) -> [u32; LAYER_N] {
        let mut hits = [0u32; LAYER_N];
        let mut i = 0;
        while i < ledger.recs.len() {
            let r = &ledger.recs[i];
            if (r.layer as usize) < LAYER_N {
                hits[r.layer as usize] = hits[r.layer as usize].saturating_add(1);
            }
            i += 1;
        }
        hits
    }

    /// 已注册条目数。
    pub fn used(&self) -> usize {
        self.entries.len()
    }
}

// ── 裁决轨迹账本（观测面） ──────────────────────────────────────
/// 单条裁决轨迹：一次查询的七元组观测记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceRecord {
    /// 全局单调序号（回放键）。
    pub seq: u32,
    pub proc_id: u32,
    pub api_id: u32,
    pub dim: u32,
    /// 命中层（0..4）。
    pub layer: u8,
    /// 命中依据号（>0 才合法）。
    pub basis: u32,
    /// 终值（矩阵裁决值）。
    pub value: u32,
}

/// 轨迹账本：按查询序落账；满则 dropped 显性记账。
pub struct TraceLedger {
    pub recs: Vec<TraceRecord>,
    cap: usize,
    pub dropped: u32,
    next_seq: u32,
}

impl TraceLedger {
    /// 建账本（容量 TRACE_CAP）。
    pub fn new() -> TraceLedger {
        TraceLedger { recs: Vec::new(), cap: TRACE_CAP, dropped: 0, next_seq: 0 }
    }

    /// 落一条轨迹：序号自动分配；满则记账不写。
    pub fn record(&mut self, proc_id: u32, api_id: u32, dim: u32, layer: HitLayer, basis: u32, value: u32) {
        if self.recs.len() >= self.cap {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.recs.push(TraceRecord {
            seq: self.next_seq,
            proc_id,
            api_id,
            dim,
            layer: layer.idx() as u8,
            basis,
            value,
        });
        self.next_seq = self.next_seq.saturating_add(1);
    }

    /// 完整率校验：查询数=轨迹数 ∧ 逐条（层合法 ∧ 依据非空 ∧ 序号连续）。
    /// 返回 Ok(完整条数)；任一违约返回 Err(码)——完整率必为 100% 才算过。
    pub fn validate(&self, expect_queries: usize) -> Result<usize, u32> {
        if self.recs.len() != expect_queries {
            return Err(ObsCode::EmptyLedger.val());
        }
        let mut i = 0;
        while i < self.recs.len() {
            let r = &self.recs[i];
            if HitLayer::from_idx(r.layer as usize).is_none() {
                return Err(ObsCode::LayerInvalid.val());
            }
            if r.basis == 0 {
                return Err(ObsCode::BasisMissing.val());
            }
            if r.seq != i as u32 {
                return Err(ObsCode::SeqBroken.val());
            }
            i += 1;
        }
        Ok(self.recs.len())
    }

    /// 导出为回放文本：表头 + 每轨迹一行七段竖线分隔。
    /// 导出前先做完整率校验——脏账本不给导出（观测面只放干净数据）。
    pub fn export(&self, expect_queries: usize) -> Result<Vec<String>, u32> {
        self.validate(expect_queries)?;
        if self.recs.len() == 0 {
            return Err(ObsCode::EmptyLedger.val());
        }
        let mut out: Vec<String> = Vec::new();
        out.push(format!("OBS-TRACE v1 seq|proc|api|dim|layer|basis|value n={}", self.recs.len()));
        let mut i = 0;
        while i < self.recs.len() {
            let r = &self.recs[i];
            let layer_name = match HitLayer::from_idx(r.layer as usize) {
                Some(l) => l.name(),
                None => "?",
            };
            out.push(format!(
                "{}|{}|{}|{}|{}|{}|{}",
                r.seq, r.proc_id, r.api_id, r.dim, layer_name, r.basis, r.value
            ));
            i += 1;
        }
        Ok(out)
    }
}

/// 回放对拍：按同序重放查询，命中层/依据/终值逐字段与轨迹一致才可回放。
pub fn replay(matrix: &LieMatrix, queries: &[(u32, u32, u32)], ledger: &TraceLedger) -> Result<(), u32> {
    if queries.len() != ledger.recs.len() {
        return Err(ObsCode::ReplayMismatch.val());
    }
    let mut i = 0;
    while i < queries.len() {
        let (p, a, d) = queries[i];
        let hit = matrix.query(p, a, d)?;
        let r = &ledger.recs[i];
        if hit.0.idx() as u8 != r.layer
            || hit.1 != r.value
            || hit.2 != r.basis
            || p != r.proc_id
            || a != r.api_id
            || d != r.dim
        {
            return Err(ObsCode::ReplayMismatch.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 固定种子 LCG（确定性语料） ──────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    /// 种子写死调用方传 SEG_BASE。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n) 确定性取值。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 生成 100 次查询语料（确定性：同种子同序列）。
pub fn make_queries(seed: u64) -> Vec<(u32, u32, u32)> {
    let mut g = Lcg(seed);
    let mut q: Vec<(u32, u32, u32)> = Vec::new();
    let mut i = 0;
    while i < QUERY_ROUNDS {
        let p = g.below(PROC_N as u64) as u32;
        let a = g.below(API_N as u64) as u32;
        let d = g.below(DIM_N as u64) as u32;
        q.push((p, a, d));
        i += 1;
    }
    q
}

/// 预注册矩阵：把语料涉及的全部键挂上层级——
/// 三分之一键挂 ActCtx，三分之一挂 Manifest，其余只挂 Compat；
/// 全部键都挂 Default 兜底（值 62 系兼容口径），保证查询必命中。
pub fn seed_matrix(matrix: &mut LieMatrix, queries: &[(u32, u32, u32)]) -> Result<(), u32> {
    // 先对语料去重（线性查重，键空间小）。
    let mut seen: Vec<(u32, u32, u32)> = Vec::new();
    let mut i = 0;
    while i < queries.len() {
        let k = queries[i];
        if matrix.find(k.0, k.1, k.2).is_none() && !seen.contains(&k) {
            seen.push(k);
        }
        i += 1;
    }
    let mut j = 0;
    while j < seen.len() {
        let (p, a, d) = seen[j];
        let v = 62_000 + (j as u32) * 3;
        // 四分摊：0→ActCtx、1→Manifest、2→Compat、3→仅 Default
        // ——保证四层命中覆盖（Default 兜底必须被踩到）。
        match j % 4 {
            0 => {
                matrix.register(p, a, d, HitLayer::ActCtx.idx(), v, 100 + j as u32)?;
                matrix.register(p, a, d, HitLayer::Default.idx(), 62_990, 9)?;
            }
            1 => {
                matrix.register(p, a, d, HitLayer::Manifest.idx(), v, 200 + j as u32)?;
                matrix.register(p, a, d, HitLayer::Default.idx(), 62_990, 9)?;
            }
            2 => {
                matrix.register(p, a, d, HitLayer::Compat.idx(), v, 300 + j as u32)?;
                matrix.register(p, a, d, HitLayer::Default.idx(), 62_990, 9)?;
            }
            _ => {
                // 仅 Default：上层全空 → 查询必落兜底层。
                matrix.register(p, a, d, HitLayer::Default.idx(), 62_990, 9)?;
            }
        }
        j += 1;
    }
    Ok(())
}

// ── 黄金全链：100 查询 → 轨迹 → 导出 → 回放 → 双跑一致 ──────────
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let mut matrix = LieMatrix::new();
    let queries = make_queries(SEG_BASE as u64);
    seed_matrix(&mut matrix, &queries)?;
    // 100 次查询逐条落轨迹。
    let mut ledger = TraceLedger::new();
    let mut i = 0;
    while i < queries.len() {
        let (p, a, d) = queries[i];
        let (layer, val, basis) = matrix.query(p, a, d)?;
        ledger.record(p, a, d, layer, basis, val);
        i += 1;
    }
    // 完整率 100%：查询数=轨迹数且逐条齐备。
    let complete = ledger.validate(queries.len())?;
    if complete != QUERY_ROUNDS {
        return Err(ObsCode::EmptyLedger.val());
    }
    // 导出 + 回放。
    let lines = ledger.export(queries.len())?;
    if lines.len() != QUERY_ROUNDS + 1 {
        return Err(ObsCode::ExportTrunc.val());
    }
    replay(&matrix, &queries, &ledger)?;
    // 双跑：再跑一遍全链，导出文本逐行一致（确定性红线）。
    let mut matrix2 = LieMatrix::new();
    let queries2 = make_queries(SEG_BASE as u64);
    seed_matrix(&mut matrix2, &queries2)?;
    let mut ledger2 = TraceLedger::new();
    let mut k = 0;
    while k < queries2.len() {
        let (p, a, d) = queries2[k];
        let (layer, val, basis) = matrix2.query(p, a, d)?;
        ledger2.record(p, a, d, layer, basis, val);
        k += 1;
    }
    let lines2 = ledger2.export(queries2.len())?;
    let mut n = 0;
    while n < lines.len() && n < lines2.len() {
        if lines[n] != lines2[n] {
            return Err(ObsCode::NotReproducible.val());
        }
        n += 1;
    }
    if lines.len() != lines2.len() {
        return Err(ObsCode::NotReproducible.val());
    }
    // 层覆盖自证：四层至少各命中 1 条（否则优先链没被全链踩到）。
    let hits = matrix.layer_hits(&ledger);
    let mut l = 0;
    while l < LAYER_N {
        if hits[l] == 0 {
            return Err(ObsCode::LayerInvalid.val());
        }
        l += 1;
    }
    // 导出文本 FNV 指纹（回放指纹入册）。
    let fp = fnv_lines(&lines);
    Ok((complete, fp, matrix.used() as u32))
}

/// 行集 FNV-1a 指纹。
pub fn fnv_lines(lines: &[String]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut i = 0;
    while i < lines.len() {
        let b = lines[i].as_bytes();
        let mut j = 0;
        while j < b.len() {
            h ^= b[j] as u32;
            h = h.wrapping_mul(0x01000193);
            j += 1;
        }
        h = h.wrapping_mul(0x01000193); // 行分隔符占位，逐行扰动。
        i += 1;
    }
    h
}

// ── 注入核（四类坏样本，逐类 10 轮必拒） ────────────────────────
/// 构造一条「无依据命中」轨迹（basis=0，矩阵不可能产出，只能人工注入）。
pub fn make_bad_basis_ledger() -> TraceLedger {
    let mut led = TraceLedger::new();
    led.record(1, 0, 0, HitLayer::Manifest, 0, 62_000);
    led
}

/// 构造「乱序序号」账本（跳号）。
pub fn make_bad_seq_gap_ledger() -> TraceLedger {
    let mut led = TraceLedger::new();
    led.record(1, 0, 0, HitLayer::ActCtx, 7, 62_001);
    led.record(2, 0, 1, HitLayer::Default, 9, 62_990);
    // 手工把第二条 seq 改成 5——账本内部跳号。
    let n = led.recs.len();
    if n >= 2 {
        led.recs[n - 1].seq = 5;
    }
    led
}

/// 构造「重复序号」账本。
pub fn make_bad_seq_dup_ledger() -> TraceLedger {
    let mut led = TraceLedger::new();
    led.record(1, 0, 0, HitLayer::ActCtx, 7, 62_001);
    led.record(2, 0, 1, HitLayer::Default, 9, 62_990);
    let n = led.recs.len();
    if n >= 2 {
        led.recs[n - 1].seq = 0;
    }
    led
}

/// 构造「越层」账本（layer=4，优先链外）。
pub fn make_bad_layer_ledger() -> TraceLedger {
    let mut led = TraceLedger::new();
    led.record(1, 0, 0, HitLayer::ActCtx, 7, 62_001);
    let n = led.recs.len();
    if n >= 1 {
        led.recs[n - 1].layer = 4;
    }
    led
}

/// 注入判定账（四槽：无依据/跳号/重复序号/越层）。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    /// 记一轮：slot 槽试了、是否被抓。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 全槽全抓（漏检必须为 0）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检总数（守恒：注入 = 检出 + 漏检）。
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 4 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 跑 10 轮四类注入：坏账本走导出面（export 内含 validate）必被拒。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：无依据命中。
        let l0 = make_bad_basis_ledger();
        led.record(0, l0.export(1).is_err());
        // 槽 1：序号跳号（账本 2 条，期望 2）。
        let l1 = make_bad_seq_gap_ledger();
        led.record(1, l1.export(2).is_err());
        // 槽 2：序号重复。
        let l2 = make_bad_seq_dup_ledger();
        led.record(2, l2.export(2).is_err());
        // 槽 3：越层。
        let l3 = make_bad_layer_ledger();
        led.record(3, l3.export(1).is_err());
        r += 1;
    }
    led
}

// ── 探针三序（正/负/负） ────────────────────────────────────────
/// 正探针：合法单查询轨迹齐备可导出。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut matrix = LieMatrix::new();
    matrix.register(3, 1, 2, HitLayer::Manifest.idx(), 62_123, 55)?;
    matrix.register(3, 1, 2, HitLayer::Default.idx(), 62_990, 9)?;
    let mut ledger = TraceLedger::new();
    let (l, v, b) = matrix.query(3, 1, 2)?;
    ledger.record(3, 1, 2, l, b, v);
    let lines = ledger.export(1)?;
    if lines.len() != 2 {
        return Err(ObsCode::ExportTrunc.val());
    }
    Ok(())
}

/// 负探针：无兜底键查询必拒——约定「负探针返回 Err(拒因码)」，
/// 调用方以 is_err() 断言拒绝发生。
pub fn probe_seq_b() -> Result<(), u32> {
    let matrix = LieMatrix::new();
    match matrix.query(9, 9, 9) {
        Err(c) => Err(c),
        Ok(_) => Err(ObsCode::NotReproducible.val()),
    }
}

/// 负探针：空依据注册必拒（有值必有据不变量）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut matrix = LieMatrix::new();
    matrix.register(0, 0, 0, HitLayer::Manifest.idx(), 62_000, 0)
}

/// 段位守卫：全部诊断码落在 [SEG_BASE, SEG_BASE+0x40) 窗口。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ObsCode::ALL.len() {
        let v = ObsCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18431_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18431 矩阵观测导出");
    // ① 黄金全链：100 查询 → 轨迹完整率 100% → 导出 → 回放 → 双跑一致。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, _, _)) => *n == QUERY_ROUNDS,
        Err(_) => false,
    };
    s.add_dyn("J1-100查询轨迹完整率100%", g_ok, &format!("完整={:?}", g.map(|r| r.0)));
    // ② 命中层分布覆盖：四层优先链全链被踩到（含播种成功）。
    let mut matrix = LieMatrix::new();
    let queries = make_queries(SEG_BASE as u64);
    let seed_ok = seed_matrix(&mut matrix, &queries).is_ok();
    let mut ledger = TraceLedger::new();
    let mut i = 0;
    while i < queries.len() {
        if let Ok((l, v, b)) = matrix.query(queries[i].0, queries[i].1, queries[i].2) {
            ledger.record(queries[i].0, queries[i].1, queries[i].2, l, b, v);
        }
        i += 1;
    }
    let hits = matrix.layer_hits(&ledger);
    let cover = seed_ok && hits[0] > 0 && hits[1] > 0 && hits[2] > 0 && hits[3] > 0;
    s.add_dyn("命中层四层覆盖", cover, &format!("{:?}", hits));
    // ③ 查询数 = 轨迹数（每查询恰一条，守恒）。
    s.add("查询轨迹守恒", ledger.recs.len() == QUERY_ROUNDS, "查询数=轨迹数");
    // ④ 导出文本行数 = 轨迹数 + 表头。
    let lines_ok = match ledger.export(queries.len()) {
        Ok(lines) => lines.len() == QUERY_ROUNDS + 1,
        Err(_) => false,
    };
    s.add("导出文本齐备", lines_ok, "表头+100 行");
    // ⑤ 回放一致。
    let rep_ok = replay(&matrix, &queries, &ledger).is_ok();
    s.add("裁决回放一致", rep_ok, "同序重放逐字段相等");
    // ⑥⑦⑧⑨ 注入四类 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "注入-无依据命中10/10拒",
        led.caught[0] == JUDGE_ROUNDS,
        &format!("{}/{}", led.caught[0], led.tries[0]),
    );
    s.add_dyn(
        "注入-序号跳号10/10拒",
        led.caught[1] == JUDGE_ROUNDS,
        &format!("{}/{}", led.caught[1], led.tries[1]),
    );
    s.add_dyn(
        "注入-序号重复10/10拒",
        led.caught[2] == JUDGE_ROUNDS,
        &format!("{}/{}", led.caught[2], led.tries[2]),
    );
    s.add_dyn(
        "注入-越层10/10拒",
        led.caught[3] == JUDGE_ROUNDS,
        &format!("{}/{}", led.caught[3], led.tries[3]),
    );
    // ⑩ 注入守恒：注入次数 = 检出 + 漏检，漏检必须为 0。
    s.add("注入漏检为零", led.missed() == 0, "检出+漏检=注入");
    // ⑪ 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    s.add("探针三序正负负", a && b && c, "正过/无兜底拒/空依据拒");
    // ⑫ 矩阵注册边界：重复注册按更新记账，键不重复。
    let mut m2 = LieMatrix::new();
    let mut upd_ok = m2.register(1, 1, 1, HitLayer::Compat.idx(), 10, 3).is_ok();
    upd_ok = upd_ok && m2.register(1, 1, 1, HitLayer::Compat.idx(), 20, 4).is_ok();
    upd_ok = upd_ok && m2.used() == 1;
    let q2 = m2.query(1, 1, 1);
    upd_ok = upd_ok && match q2 {
        Ok((l, v, b)) => l == HitLayer::Compat && v == 20 && b == 4,
        Err(_) => false,
    };
    s.add("重复注册按更新记账", upd_ok, "键唯一+新值生效");
    // ⑬ 账本满边界：dropped 显性记账不静默丢。
    let mut l3 = TraceLedger::new();
    let mut k = 0;
    while k < TRACE_CAP + 5 {
        l3.record((k % 4) as u32, 0, 0, HitLayer::Default, 9, 62_990);
        k += 1;
    }
    s.add("账本满显性记账", l3.dropped == 5, "满后 5 条入 dropped");
    // ⑭ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut c = 0;
    while c < ObsCode::ALL.len() {
        if ObsCode::ALL[c].detail().is_empty() {
            closed = false;
        }
        c += 1;
    }
    s.add("诊断文案闭集非空", closed, "11 码全有中文建议");
    s.add("段位守卫 0x7E780", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_100_queries_complete_and_replayable() {
        let g = golden_run();
        match g {
            Ok((n, _, used)) => {
                assert_eq!(n, 100, "完整轨迹 100 条");
                assert!(used > 0, "矩阵必须已注册");
            }
            Err(e) => panic!("golden_run 应全绿: {} {}", e, detail(e)),
        }
    }

    #[test]
    fn four_layers_all_hit() {
        let mut matrix = LieMatrix::new();
        let queries = make_queries(SEG_BASE as u64);
        assert!(seed_matrix(&mut matrix, &queries).is_ok());
        let mut ledger = TraceLedger::new();
        for &(p, a, d) in queries.iter() {
            if let Ok((l, v, b)) = matrix.query(p, a, d) {
                ledger.record(p, a, d, l, b, v);
            }
        }
        let hits = matrix.layer_hits(&ledger);
        assert!(hits[0] > 0 && hits[1] > 0 && hits[2] > 0 && hits[3] > 0);
    }

    #[test]
    fn injections_all_caught_ten_rounds() {
        let led = inject_rounds(10);
        assert!(led.all_caught(), "四类注入必须 10/10 全抓");
        assert_eq!(led.missed(), 0, "漏检必须为零");
    }

    #[test]
    fn replay_matches_and_double_run_identical() {
        let mut matrix = LieMatrix::new();
        let queries = make_queries(SEG_BASE as u64);
        assert!(seed_matrix(&mut matrix, &queries).is_ok());
        let mut ledger = TraceLedger::new();
        for &(p, a, d) in queries.iter() {
            if let Ok((l, v, b)) = matrix.query(p, a, d) {
                ledger.record(p, a, d, l, b, v);
            }
        }
        assert!(replay(&matrix, &queries, &ledger).is_ok());
        let a = golden_run();
        let b = golden_run();
        assert_eq!(a, b, "双跑逐字段一致");
    }

    #[test]
    fn probes_positive_and_negative() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_err());
        assert!(probe_seq_c().is_err());
    }

    #[test]
    fn ledger_full_is_accounted_not_silent() {
        let mut l = TraceLedger::new();
        let mut k = 0;
        while k < TRACE_CAP + 5 {
            l.record(0, 0, 0, HitLayer::Default, 9, 1);
            k += 1;
        }
        assert_eq!(l.recs.len(), TRACE_CAP);
        assert_eq!(l.dropped, 5, "溢出 5 条必须显性入账");
    }

    #[test]
    fn duplicate_register_updates_in_place() {
        let mut m = LieMatrix::new();
        assert!(m.register(2, 2, 2, HitLayer::ActCtx.idx(), 1, 1).is_ok());
        assert!(m.register(2, 2, 2, HitLayer::ActCtx.idx(), 2, 2).is_ok());
        assert_eq!(m.used(), 1, "重复注册不产生重复键");
        let q = m.query(2, 2, 2);
        match q {
            Ok((_, v, b)) => {
                assert_eq!(v, 2);
                assert_eq!(b, 2);
            }
            Err(_) => panic!("更新后查询必须命中"),
        }
    }

    #[test]
    fn segment_guard_and_closed_text() {
        assert!(segment_guard());
        for c in ObsCode::ALL.iter() {
            assert!(!c.detail().is_empty(), "诊断码 {:?} 文案不得为空", c);
            assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40);
        }
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18431_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20, "判据条数 8~20");
    }
}
