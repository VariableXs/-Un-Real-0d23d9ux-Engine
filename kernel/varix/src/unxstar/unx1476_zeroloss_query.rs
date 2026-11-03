//! UNX-F1476 零丢查询接口（断点/修复/日报三维查询冻结面） · 0x75840段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，AI-VX7-1104 批3 H 组）：
//!   三维查询 100%（按向量/按时间窗/按归因标签），查询与账本对拍
//!   一致率 100%。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 查询面 v1 冻结三维度：按归因标签（label）/按时间窗（tick 窗）/
//!     按向量（vector 桶），维度闭集入 `QueryKind`，面版本冻结 v1，
//!     冻结后维度表改写显性拒（PostFreezeWrite）；
//!   ② 越界查询显性 Empty 不造假：未知标签/空窗/空向量一律返回
//!     `Answer{ empty: true, hits: [] }`——不返回伪造命中，不静默
//!     填充；参数本身非法（标签越码界/窗倒挂/向量越桶界）才走 Err；
//!   ③ 对拍口径：查询命中集与账本直查（stride 抽样）逐条对拍，
//!     一致率按千分比整数记账，判据要求 1000‰（=100%）；
//!   ④ 命中集按 seq 升序返回（写入序即 seq 序，查询不复排不丢序）；
//!   ⑤ 语料冻结 24 条记录（seq 0..24，tick/标签/向量按冻结算子推导），
//!     每维期望命中数手工冻结为第三方锚，向量桶 7 冻结为空桶；
//!   ⑥ 注入路：未知标签造假路（改词法让 Empty 返回伪命中）/ 窗倒挂
//!     路 / 向量越界路 / 冻结后改维路 / 版本倒退路，每路必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75840，批3 H 组独占窗口）。
pub const SEG_BASE: u32 = 0x75840;

/// 查询面版本（冻结 v1）。
pub const FACE_VERSION: u32 = 1;
/// 查询维度数（冻结 3：按标签/按时间窗/按向量）。
pub const QUERY_DIMS: usize = 3;
/// 归因标签码界（冻结 32：0..=31）。
pub const LABEL_CAP: u16 = 32;
/// 向量桶界（冻结 8：0..=7）。
pub const VECTOR_CAP: u8 = 8;
/// 时间窗上界（冻结 10000 步：tick 语义域）。
pub const WINDOW_MAX_TICK: u64 = 10_000;
/// 对拍抽样步长（冻结 4：每 4 条抽 1 条）。
pub const SAMPLE_STRIDE: usize = 4;
/// 账本容量上限（冻结 1024）。
pub const MAX_RECORDS: usize = 1024;
/// 判据轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 诊断码闭集：每码带可操作中文建议（detail 文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueryCode {
    /// 标签越码界：label ≥ 32
    LabelOver,
    /// 向量越桶界：vector ≥ 8
    VectorOver,
    /// 时间窗非法：lo > hi 或 hi 越上界
    BadWindow,
    /// 维度未知：不在三维闭集内
    DimUnknown,
    /// 冻结后改维：面已冻结维度表只读
    PostFreezeWrite,
    /// 版本倒退：面版本低于 v1
    FaceRegress,
    /// 对拍失配：抽样对拍一致率 < 1000‰
    CrossMismatch,
    /// 空账查询面：账本零记录（对拍无意义显性拒）
    EmptyLedger,
    /// 记录越界：超过 MAX_RECORDS
    RecordOver,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl QueryCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            QueryCode::LabelOver => "标签越码界：label 须在 0..=31，请核对归因标签编码表",
            QueryCode::VectorOver => "向量越桶界：vector 须在 0..=7，请核对向量桶登记表",
            QueryCode::BadWindow => "时间窗非法：lo ≤ hi 且 hi ≤ 10000，请核对窗参数",
            QueryCode::DimUnknown => "维度未知：查询维度仅 按标签/按时间窗/按向量 三维，请按闭集调用",
            QueryCode::PostFreezeWrite => "冻结后改维拒：查询面 v1 已冻结，如需改维度请升版 v2 重走冻结",
            QueryCode::FaceRegress => "版本倒退拒：面版本只能升不许降，请核对版本账",
            QueryCode::CrossMismatch => "对拍失配：抽样对拍一致率不足 1000‰，请查查询谓词与账本口径",
            QueryCode::EmptyLedger => "空账查询面：账本零记录时对拍无意义，请先灌记录",
            QueryCode::RecordOver => "记录越界：超过 MAX_RECORDS=1024，请拆册",
            QueryCode::NotReproducible => "双跑不一致：复现红线，请检查是否有墙钟/随机参与取值",
        }
    }
    pub const ALL: [QueryCode; 10] = [
        QueryCode::LabelOver,
        QueryCode::VectorOver,
        QueryCode::BadWindow,
        QueryCode::DimUnknown,
        QueryCode::PostFreezeWrite,
        QueryCode::FaceRegress,
        QueryCode::CrossMismatch,
        QueryCode::EmptyLedger,
        QueryCode::RecordOver,
        QueryCode::NotReproducible,
    ];
}

/// 段内查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < QueryCode::ALL.len() {
        if let Some(c) = QueryCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75840 段闭集内"
}

// ── 域模型：账本与记录 ──────────────────────────────────────────
/// 单条零丢事件记录（查询面的账本行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventRecord {
    /// 序号（写入序，命中集按此升序）
    pub seq: u32,
    /// 落账步（时间窗维度键）
    pub at_tick: u64,
    /// 归因标签（0..=31）
    pub label: u16,
    /// 向量桶（0..=7）
    pub vector: u8,
    /// 量（断点/修复/日报语义下的计数，查询面只透传）
    pub amount: u64,
}

/// 账本（append-only，冻结容量）。
#[derive(Clone, Debug, Default)]
pub struct QueryLedger {
    pub records: Vec<EventRecord>,
}

impl QueryLedger {
    pub fn new() -> QueryLedger {
        QueryLedger::default()
    }
    /// 灌记录（seq 自动按写入序；越容显性拒）。
    pub fn append(&mut self, at_tick: u64, label: u16, vector: u8, amount: u64) -> Result<u32, u32> {
        if self.records.len() >= MAX_RECORDS {
            return Err(QueryCode::RecordOver.val());
        }
        let seq = self.records.len() as u32;
        self.records.push(EventRecord { seq, at_tick, label, vector, amount });
        Ok(seq)
    }
    /// 按 seq 直读（对拍面的账本侧入口）。
    pub fn record_at(&self, seq: u32) -> Option<EventRecord> {
        self.records.get(seq as usize).copied()
    }
}

/// 查询维度闭集（冻结 v1：三维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryKind {
    /// 按归因标签
    ByLabel,
    /// 按时间窗
    ByWindow,
    /// 按向量
    ByVector,
}

/// 查询答案（显性 Empty 语义：empty=true 时 hits 恒空，不造假）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryAnswer {
    pub kind: QueryKind,
    /// 命中 seq 升序集
    pub hits: Vec<u32>,
    /// 显性空（未知标签/空窗/空向量）
    pub empty: bool,
}

impl QueryAnswer {
    /// 造假守卫：empty 态携带命中即违例（语义断言用）。
    pub fn honest(&self) -> bool {
        if self.empty {
            self.hits.is_empty()
        } else {
            !self.hits.is_empty()
        }
    }
}

/// 查询面 v1（三维冻结表 + 冻结态）。
#[derive(Clone, Debug)]
pub struct QueryFaceV1 {
    pub version: u32,
    /// 三维启用表（闭集序：ByLabel/ByWindow/ByVector）
    pub dims: [bool; QUERY_DIMS],
    pub frozen: bool,
}

impl QueryFaceV1 {
    pub fn new() -> QueryFaceV1 {
        QueryFaceV1 { version: FACE_VERSION, dims: [true; QUERY_DIMS], frozen: false }
    }

    /// 冻结面（单向：Frozen 后维度表只读，版本只升不降）。
    pub fn freeze(&mut self) -> Result<(), u32> {
        self.frozen = true;
        Ok(())
    }

    /// 改维（冻结后显性拒；越维闭集拒）。
    pub fn set_dim(&mut self, dim: usize, on: bool) -> Result<(), u32> {
        if dim >= QUERY_DIMS {
            return Err(QueryCode::DimUnknown.val());
        }
        if self.frozen {
            return Err(QueryCode::PostFreezeWrite.val());
        }
        self.dims[dim] = on;
        Ok(())
    }

    /// 版本推进（只升不降）。
    pub fn bump_version(&mut self, v: u32) -> Result<(), u32> {
        if v < self.version {
            return Err(QueryCode::FaceRegress.val());
        }
        self.version = v;
        Ok(())
    }

    fn dim_enabled(&self, kind: QueryKind) -> bool {
        match kind {
            QueryKind::ByLabel => self.dims.get(0).copied().unwrap_or(false),
            QueryKind::ByWindow => self.dims.get(1).copied().unwrap_or(false),
            QueryKind::ByVector => self.dims.get(2).copied().unwrap_or(false),
        }
    }
}

// ── 三维查询（冻结 v1 语义） ────────────────────────────────────
/// 按归因标签查询：越码界 Err；无命中显性 Empty；命中按 seq 升序。
pub fn query_by_label(face: &QueryFaceV1, ledger: &QueryLedger, label: u16) -> Result<QueryAnswer, u32> {
    if !face.dim_enabled(QueryKind::ByLabel) {
        return Err(QueryCode::DimUnknown.val());
    }
    if label >= LABEL_CAP {
        return Err(QueryCode::LabelOver.val());
    }
    let mut hits = Vec::new();
    let mut i = 0usize;
    while i < ledger.records.len() {
        if let Some(r) = ledger.records.get(i) {
            if r.label == label {
                hits.push(r.seq);
            }
        }
        i += 1;
    }
    let empty = hits.is_empty();
    Ok(QueryAnswer { kind: QueryKind::ByLabel, hits, empty })
}

/// 按时间窗查询 [lo, hi]：倒挂/越上界 Err；空窗显性 Empty。
pub fn query_by_window(face: &QueryFaceV1, ledger: &QueryLedger, lo: u64, hi: u64) -> Result<QueryAnswer, u32> {
    if !face.dim_enabled(QueryKind::ByWindow) {
        return Err(QueryCode::DimUnknown.val());
    }
    if lo > hi || hi > WINDOW_MAX_TICK {
        return Err(QueryCode::BadWindow.val());
    }
    let mut hits = Vec::new();
    let mut i = 0usize;
    while i < ledger.records.len() {
        if let Some(r) = ledger.records.get(i) {
            if r.at_tick >= lo && r.at_tick <= hi {
                hits.push(r.seq);
            }
        }
        i += 1;
    }
    let empty = hits.is_empty();
    Ok(QueryAnswer { kind: QueryKind::ByWindow, hits, empty })
}

/// 按向量桶查询：越桶界 Err；空向量显性 Empty。
pub fn query_by_vector(face: &QueryFaceV1, ledger: &QueryLedger, vector: u8) -> Result<QueryAnswer, u32> {
    if !face.dim_enabled(QueryKind::ByVector) {
        return Err(QueryCode::DimUnknown.val());
    }
    if vector >= VECTOR_CAP {
        return Err(QueryCode::VectorOver.val());
    }
    let mut hits = Vec::new();
    let mut i = 0usize;
    while i < ledger.records.len() {
        if let Some(r) = ledger.records.get(i) {
            if r.vector == vector {
                hits.push(r.seq);
            }
        }
        i += 1;
    }
    let empty = hits.is_empty();
    Ok(QueryAnswer { kind: QueryKind::ByVector, hits, empty })
}

/// 命中集对拍（单维）：命中集内每条必须逐条满足该维谓词，
/// 且账本直查的满足集与命中集逐条等值（防漏防多）。
fn hits_match_predicate<F>(hits: &[u32], ledger: &QueryLedger, pred: F) -> bool
where
    F: Fn(&EventRecord) -> bool,
{
    // 防多：命中集每条真满足谓词。
    let mut i = 0usize;
    while i < hits.len() {
        let seq = match hits.get(i) {
            Some(s) => *s,
            None => return false,
        };
        let r = match ledger.record_at(seq) {
            Some(r) => r,
            None => return false,
        };
        if !pred(&r) {
            return false;
        }
        i += 1;
    }
    // 防漏：账本直查满足谓词的条数与命中集条数相等。
    let mut total = 0u32;
    let mut j = 0usize;
    while j < ledger.records.len() {
        if let Some(r) = ledger.records.get(j) {
            if pred(r) {
                total += 1;
            }
        }
        j += 1;
    }
    total == hits.len() as u32
}

/// 三维对拍一致率（千分比整数）：抽样（stride）逐维逐条对拍。
pub fn crosscheck_permille(face: &QueryFaceV1, ledger: &QueryLedger) -> Result<u32, u32> {
    if ledger.records.is_empty() {
        return Err(QueryCode::EmptyLedger.val());
    }
    let mut matched = 0u32;
    let mut sampled = 0u32;
    let mut i = 0usize;
    while i < ledger.records.len() {
        if i % SAMPLE_STRIDE != 0 {
            i += 1;
            continue;
        }
        let r = match ledger.records.get(i) {
            Some(r) => *r,
            None => break,
        };
        sampled += 1;
        // 维 1：按标签。
        let a1 = query_by_label(face, ledger, r.label)?;
        let m1 = a1.honest() && hits_match_predicate(&a1.hits, ledger, |x| x.label == r.label);
        // 维 2：按时间窗（以该记录 tick 为中心的最小合法窗）。
        let wlo = if r.at_tick > 0 { r.at_tick - 1 } else { 0 };
        let whi = if r.at_tick < WINDOW_MAX_TICK { r.at_tick + 1 } else { WINDOW_MAX_TICK };
        let a2 = query_by_window(face, ledger, wlo, whi)?;
        let m2 = a2.honest() && hits_match_predicate(&a2.hits, ledger, |x| x.at_tick >= wlo && x.at_tick <= whi);
        // 维 3：按向量。
        let a3 = query_by_vector(face, ledger, r.vector)?;
        let m3 = a3.honest() && hits_match_predicate(&a3.hits, ledger, |x| x.vector == r.vector);
        if m1 && m2 && m3 {
            matched += 1;
        }
        i += 1;
    }
    if sampled == 0 {
        return Err(QueryCode::EmptyLedger.val());
    }
    Ok((matched * 1000) / sampled)
}

// ── 语料（冻结：24 条，tick/标签/向量按冻结算子推导） ────────────
/// 语料算子：tick = (seq*37) % 950，label = seq % 8，vector = seq % 7，
/// amount = (seq*11) % 9 + 1。全确定性，无随机无墙钟。
/// 分布冻结：标签 0..7 各 3 命中、标签 8..31 全空；向量桶 0..6 各有
/// 命中（桶 0 有 4 个）、桶 7 全空——空桶供"显性 Empty"语义断言。
pub fn corpus_record(seq: u32) -> EventRecord {
    EventRecord {
        seq,
        at_tick: ((seq as u64) * 37) % 950,
        label: (seq % 8) as u16,
        vector: (seq % 7) as u8,
        amount: ((seq * 11) % 9 + 1) as u64,
    }
}

/// 按语料算子灌满账本（24 条冻结）。
pub fn build_ledger() -> Result<QueryLedger, u32> {
    let mut l = QueryLedger::new();
    let mut seq = 0u32;
    while seq < 24 {
        let r = corpus_record(seq);
        l.append(r.at_tick, r.label, r.vector, r.amount)?;
        seq += 1;
    }
    Ok(l)
}

/// 语料期望：标签 7 的命中数（手工冻结锚：seq ∈ {7,15,23}）。
pub fn corpus_expect_label7() -> u32 {
    3
}

/// 语料期望：向量 3 的命中数（手工冻结锚：seq ∈ {3,10,17}）。
pub fn corpus_expect_vector3() -> u32 {
    3
}

/// 语料期望：向量 0 的命中数（手工冻结锚：seq ∈ {0,7,14,21}）。
pub fn corpus_expect_vector0() -> u32 {
    4
}

// ── LCG（种子固定可用） ────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（五路） ─────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 5],
    pub caught: [u32; 5],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 5], caught: [0; 5] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 5 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 黄金全链：灌账→三维查询→锚对拍→越界 Empty→对拍一致率→冻结 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let face = QueryFaceV1::new();
    let ledger = build_ledger()?;
    // 三维查询各一跑（标签 7 / 窗 [0,100] / 向量 3）。
    let a1 = query_by_label(&face, &ledger, 7)?;
    let a2 = query_by_window(&face, &ledger, 0, 100)?;
    let a3 = query_by_vector(&face, &ledger, 3)?;
    // 锚对拍：期望命中数逐维等值。
    let anchor_ok = (a1.hits.len() as u32 == corpus_expect_label7()
        && a3.hits.len() as u32 == corpus_expect_vector3()
        && !a2.hits.is_empty()) as u32;
    // 越界 Empty 不造假：未知标签 31 无命中（语料算子不产生 31）。
    let a4 = query_by_label(&face, &ledger, 31)?;
    let empty_honest = (a4.empty && a4.hits.is_empty() && a4.honest()) as u32;
    // 对拍一致率 1000‰。
    let permille = crosscheck_permille(&face, &ledger)?;
    let cross_ok = (permille == 1000) as u32;
    // 冻结面 + 冻结后改维拒。
    let mut f2 = QueryFaceV1::new();
    f2.freeze()?;
    let frozen_write_rejected = matches!(f2.set_dim(0, false), Err(e) if e == QueryCode::PostFreezeWrite.val()) as u32;
    Ok((
        anchor_ok + empty_honest + cross_ok + frozen_write_rejected + 1,
        frozen_write_rejected + 1,
        permille,
    ))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链精确值（绿位 5 / 拦 2 / 一致率 1000‰）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, 2, 1000)) => Ok(()),
        _ => Err(QueryCode::NotReproducible.val()),
    }
}

/// 序 B：越界查询三态显性断言（未知标签 Empty / 窗倒挂 Err / 向量越界 Err）。
pub fn probe_seq_b() -> Result<(), u32> {
    let face = QueryFaceV1::new();
    let ledger = build_ledger()?;
    // 未知标签：显性 Empty（语料算子 seq*7%32 不产生 31）。
    let empty_label = query_by_label(&face, &ledger, 31)?;
    if !(empty_label.empty && empty_label.hits.is_empty()) {
        return Err(QueryCode::CrossMismatch.val());
    }
    // 窗倒挂：显性 Err。
    if !matches!(query_by_window(&face, &ledger, 100, 50), Err(e) if e == QueryCode::BadWindow.val()) {
        return Err(QueryCode::CrossMismatch.val());
    }
    // 窗越上界：显性 Err。
    if !matches!(query_by_window(&face, &ledger, 0, WINDOW_MAX_TICK + 1), Err(e) if e == QueryCode::BadWindow.val()) {
        return Err(QueryCode::CrossMismatch.val());
    }
    // 向量越桶界：显性 Err。
    if !matches!(query_by_vector(&face, &ledger, VECTOR_CAP), Err(e) if e == QueryCode::VectorOver.val()) {
        return Err(QueryCode::CrossMismatch.val());
    }
    // 标签越码界：显性 Err。
    if !matches!(query_by_label(&face, &ledger, LABEL_CAP), Err(e) if e == QueryCode::LabelOver.val()) {
        return Err(QueryCode::CrossMismatch.val());
    }
    Ok(())
}

/// 序 C：冻结面（版本 v1 / 三维闭集 / 语料锚 / 抽样步长）。
pub fn probe_seq_c() -> Result<(), u32> {
    let consts_ok = FACE_VERSION == 1 && QUERY_DIMS == 3 && LABEL_CAP == 32 && VECTOR_CAP == 8 && SAMPLE_STRIDE == 4;
    // 语料锚复算：标签 7 与向量 3 的命中数。
    let ledger = build_ledger()?;
    let face = QueryFaceV1::new();
    let a1 = query_by_label(&face, &ledger, 7)?;
    let a3 = query_by_vector(&face, &ledger, 3)?;
    let anchor_ok = a1.hits.len() as u32 == corpus_expect_label7() && a3.hits.len() as u32 == corpus_expect_vector3();
    match (consts_ok, anchor_ok) {
        (true, true) => Ok(()),
        _ => Err(QueryCode::FaceRegress.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 5 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        let _ = rng.next();
        let face = QueryFaceV1::new();
        let ledger = build_ledger()?;
        // 路 1：未知标签 Empty 造假路——Empty 态命中集必须恒空，
        // 任何伪命中都被 honest() 守卫抓出（这里构造伪命中验证守卫）。
        let fake = QueryAnswer { kind: QueryKind::ByLabel, hits: vec![0], empty: true };
        led.record(0, !fake.honest() && query_by_label(&face, &ledger, 31)?.honest());
        // 路 2：窗倒挂路。
        led.record(1, matches!(query_by_window(&face, &ledger, 90, 40), Err(e) if e == QueryCode::BadWindow.val()));
        // 路 3：向量越界路。
        let vover = rng.below(VECTOR_CAP as u64) as u8 + VECTOR_CAP;
        led.record(2, matches!(query_by_vector(&face, &ledger, vover), Err(e) if e == QueryCode::VectorOver.val()));
        // 路 4：冻结后改维路。
        let mut f2 = QueryFaceV1::new();
        let _ = f2.freeze();
        led.record(3, matches!(f2.set_dim(1, false), Err(e) if e == QueryCode::PostFreezeWrite.val()));
        // 路 5：版本倒退路。
        let mut f3 = QueryFaceV1::new();
        let _ = f3.freeze();
        led.record(4, matches!(f3.bump_version(0), Err(e) if e == QueryCode::FaceRegress.val()));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(QueryCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0usize;
    while i < QueryCode::ALL.len() {
        if let Some(c) = QueryCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ok, caught, pm)) => *ok == 5 && *caught == 2 && *pm == 1000,
            Err(_) => false,
        },
        match &g {
            Ok((ok, caught, pm)) => format!("绿位={} 拦={} 一致率={}‰", ok, caught, pm),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 2. 三维查询 100%（三维各一跑全成功且 honest）。
    let face = QueryFaceV1::new();
    let ledger = match build_ledger() {
        Ok(l) => l,
        Err(e) => {
            v.push(("J1-三维查询100%", false, format!("灌账 err=0x{:x}", e)));
            return v;
        }
    };
    let a1 = query_by_label(&face, &ledger, 7);
    let a2 = query_by_window(&face, &ledger, 0, 200);
    let a3 = query_by_vector(&face, &ledger, 3);
    let dims_ok = match (a1, a2, a3) {
        (Ok(x), Ok(y), Ok(z)) => x.honest() && y.honest() && z.honest() && !x.empty && !z.empty && !y.empty,
        _ => false,
    };
    v.push((
        "J1-三维查询100%",
        dims_ok,
        String::from("标签7/窗[0,200]/向量3 三维各一跑全命中且诚实"),
    ));

    // 3. 语料锚对拍（期望命中数手工冻结）。
    let a1b = query_by_label(&face, &ledger, 7);
    let a3b = query_by_vector(&face, &ledger, 3);
    let anchor_ok = match (a1b, a3b) {
        (Ok(x), Ok(z)) => x.hits.len() as u32 == corpus_expect_label7() && z.hits.len() as u32 == corpus_expect_vector3(),
        _ => false,
    };
    v.push((
        "J1-语料锚对拍",
        anchor_ok,
        format!("标签7 期望 {} 命中，向量3 期望 {} 命中", corpus_expect_label7(), corpus_expect_vector3()),
    ));

    // 4. 越界 Empty 不造假（三态语义断言）。
    let b = probe_seq_b();
    let b_ok = b.is_ok();
    let b_err = match b {
        Ok(()) => String::from("Empty 态零伪命中 / 倒挂窗、越界参显性 Err"),
        Err(e) => format!("err=0x{:x} {}", e, detail(e)),
    };
    v.push(("J1-越界查询语义断言", b_ok, b_err));

    // 5. 对拍一致率 1000‰。
    let pm = crosscheck_permille(&face, &ledger);
    v.push((
        "J1-对拍一致率1000‰",
        matches!(pm, Ok(p) if p == 1000),
        match pm {
            Ok(p) => format!("抽样 stride={} 一致率 {}‰", SAMPLE_STRIDE, p),
            Err(e) => format!("err=0x{:x} {}", e, detail(e)),
        },
    ));

    // 6. 空窗显性 Empty（合法窗 [950, 10000] 语料不落点）。
    let a_empty = query_by_window(&face, &ledger, 950, 10_000);
    let empty_ok = matches!(&a_empty, Ok(a) if a.empty && a.hits.is_empty() && a.honest());
    v.push((
        "J1-空窗显性Empty",
        empty_ok,
        match a_empty {
            Ok(a) => format!("合法窗无落点 empty={} hits={}", a.empty, a.hits.len()),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 7. 冻结面：冻结后改维拒 + 版本倒退拒。
    let mut f2 = QueryFaceV1::new();
    let _ = f2.freeze();
    let frozen_rej = matches!(f2.set_dim(2, false), Err(e) if e == QueryCode::PostFreezeWrite.val());
    let ver_rej = matches!(f2.bump_version(0), Err(e) if e == QueryCode::FaceRegress.val());
    v.push((
        "J1-冻结面改写拒",
        frozen_rej && ver_rej,
        format!("冻结后改维拒={} 版本倒退拒={}", frozen_rej, ver_rej),
    ));

    // 8. 冻结前改维放行（关闭标签维后按标签查询显性拒）。
    let mut f3 = QueryFaceV1::new();
    let dim_off_ok = f3.set_dim(0, false).is_ok();
    let label_disabled = matches!(query_by_label(&f3, &ledger, 0), Err(e) if e == QueryCode::DimUnknown.val());
    v.push((
        "J1-维度开关语义",
        dim_off_ok && label_disabled,
        format!("关维后查询显性拒（DimUnknown）"),
    ));

    // 9. 空账对拍显性拒。
    let empty_ledger = QueryLedger::new();
    v.push((
        "J1-空账对拍拒",
        matches!(crosscheck_permille(&face, &empty_ledger), Err(e) if e == QueryCode::EmptyLedger.val()),
        String::from("零记录对拍无意义，显性拒"),
    ));

    // 10. 记录越界拒。
    let mut big = QueryLedger::new();
    let mut i = 0usize;
    let mut filled_ok = true;
    while i < MAX_RECORDS {
        if big.append((i % 100) as u64, (i % 32) as u16, (i % 8) as u8, 1).is_err() {
            filled_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-记录越界拒",
        filled_ok && big.append(0, 0, 0, 1).is_err() && big.records.len() == MAX_RECORDS,
        format!("灌满 {} 条后追加显性拒", MAX_RECORDS),
    ));

    // 11. 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // 12. 探针三序。
    let a = probe_seq_a().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b_ok && c,
        format!("a=黄金({}) b=越界断言({}) c=冻结锚({})", a, b_ok, c),
    ));

    // 13. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));

    // 14. 段位守卫 + 闭集查表。
    let guard_ok = segment_guard();
    let mut table_ok = true;
    let mut i = 0usize;
    while i < QueryCode::ALL.len() {
        if let Some(code) = QueryCode::ALL.get(i) {
            if detail(code.val()).is_empty() {
                table_ok = false;
            }
        }
        i += 1;
    }
    v.push((
        "J3-段位守卫+闭集",
        guard_ok && table_ok,
        format!("0x75840 窗口守卫={} 闭集查表={}", guard_ok, table_ok),
    ));

    v
}

pub fn run_unx1476_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1476 零丢查询接口（三维查询冻结面）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（允许 assert!/assert_eq!；取值仍走 .get()） ──────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_all_green() {
        assert_eq!(golden_run(), Ok((5, 2, 1000)));
    }

    #[test]
    fn corpus_operator_deterministic() {
        let r0 = corpus_record(0);
        let r1 = corpus_record(1);
        assert_eq!(r0.at_tick, 0);
        assert_eq!(r0.label, 0);
        assert_eq!(r0.vector, 0);
        assert_eq!(r1.at_tick, 37);
        assert_eq!(r1.label, 1, "label = seq % 8");
        assert_eq!(r1.vector, 1, "vector = seq % 7");
        // 双跑逐条等值（冻结算子）。
        let mut i = 0u32;
        while i < 24 {
            assert_eq!(corpus_record(i), corpus_record(i), "seq {} 语料算子必须确定性", i);
            i += 1;
        }
    }

    #[test]
    fn three_dims_query_hits_expected() {
        let face = QueryFaceV1::new();
        let ledger = build_ledger().unwrap_or_else(|_| QueryLedger::new());
        let a1 = query_by_label(&face, &ledger, 7).unwrap_or(QueryAnswer { kind: QueryKind::ByLabel, hits: Vec::new(), empty: true });
        assert_eq!(a1.hits.len() as u32, corpus_expect_label7());
        let a3 = query_by_vector(&face, &ledger, 3).unwrap_or(QueryAnswer { kind: QueryKind::ByVector, hits: Vec::new(), empty: true });
        assert_eq!(a3.hits.len() as u32, corpus_expect_vector3());
        let a0 = query_by_vector(&face, &ledger, 0).unwrap_or(QueryAnswer { kind: QueryKind::ByVector, hits: Vec::new(), empty: true });
        assert_eq!(a0.hits.len() as u32, corpus_expect_vector0());
        // 命中集升序（写入序即 seq 序）。
        let mut prev = 0u32;
        let mut first = true;
        let mut i = 0usize;
        while i < a1.hits.len() {
            if let Some(s) = a1.hits.get(i) {
                assert!(first || *s > prev, "命中集必须按 seq 升序");
                prev = *s;
                first = false;
            }
            i += 1;
        }
    }

    #[test]
    fn out_of_bounds_queries_explicit() {
        let face = QueryFaceV1::new();
        let ledger = build_ledger().unwrap_or_else(|_| QueryLedger::new());
        // 未知标签 → Empty 且零伪命中。
        let a = query_by_label(&face, &ledger, 31).unwrap_or(QueryAnswer { kind: QueryKind::ByLabel, hits: vec![1], empty: false });
        assert!(a.empty && a.hits.is_empty() && a.honest(), "未知标签必须显性 Empty");
        // 窗倒挂 / 越上界 → Err。
        assert_eq!(query_by_window(&face, &ledger, 100, 50).err(), Some(QueryCode::BadWindow.val()));
        assert_eq!(query_by_window(&face, &ledger, 0, WINDOW_MAX_TICK + 1).err(), Some(QueryCode::BadWindow.val()));
        // 向量越界 / 标签越界 → Err。
        assert_eq!(query_by_vector(&face, &ledger, VECTOR_CAP).err(), Some(QueryCode::VectorOver.val()));
        assert_eq!(query_by_label(&face, &ledger, LABEL_CAP).err(), Some(QueryCode::LabelOver.val()));
    }

    #[test]
    fn empty_window_and_vector_explicit_not_fabricated() {
        let face = QueryFaceV1::new();
        let ledger = build_ledger().unwrap_or_else(|_| QueryLedger::new());
        let a = query_by_window(&face, &ledger, 950, 10_000).unwrap_or(QueryAnswer { kind: QueryKind::ByWindow, hits: vec![2], empty: false });
        assert!(a.empty && a.hits.is_empty(), "合法空窗必须显性 Empty 不造假");
        // 空向量桶（桶 7 冻结为空）：显性 Empty 零伪命中。
        let v = query_by_vector(&face, &ledger, 7).unwrap_or(QueryAnswer { kind: QueryKind::ByVector, hits: vec![3], empty: false });
        assert!(v.empty && v.hits.is_empty() && v.honest(), "空向量桶必须显性 Empty 不造假");
    }

    #[test]
    fn frozen_face_write_rejected() {
        let mut f = QueryFaceV1::new();
        assert!(f.set_dim(0, false).is_ok(), "冻结前改维放行");
        let _ = f.freeze();
        assert_eq!(f.set_dim(0, true).err(), Some(QueryCode::PostFreezeWrite.val()));
        assert_eq!(f.bump_version(0).err(), Some(QueryCode::FaceRegress.val()));
        assert!(f.bump_version(FACE_VERSION + 1).is_ok(), "升版放行");
        // 关维后该维查询显性拒。
        let mut f2 = QueryFaceV1::new();
        let _ = f2.set_dim(2, false);
        assert_eq!(query_by_vector(&f2, &QueryLedger::new(), 0).err(), Some(QueryCode::DimUnknown.val()));
    }

    #[test]
    fn crosscheck_full_consistency() {
        let face = QueryFaceV1::new();
        let ledger = build_ledger().unwrap_or_else(|_| QueryLedger::new());
        assert_eq!(crosscheck_permille(&face, &ledger).unwrap_or(0), 1000, "抽样对拍一致率必须 1000‰");
        // 空账对拍显性拒。
        assert_eq!(
            crosscheck_permille(&face, &QueryLedger::new()).err(),
            Some(QueryCode::EmptyLedger.val())
        );
    }

    #[test]
    fn hits_match_predicate_catches_tamper() {
        let face = QueryFaceV1::new();
        let ledger = build_ledger().unwrap_or_else(|_| QueryLedger::new());
        let a = query_by_label(&face, &ledger, 7).unwrap_or(QueryAnswer { kind: QueryKind::ByLabel, hits: Vec::new(), empty: true });
        // 伪造多命中（seq 0 不属标签 7）→ 防多守卫必抓。
        let mut bad_hits = a.hits.clone();
        let _ = bad_hits.push(0);
        assert!(!hits_match_predicate(&bad_hits, &ledger, |x| x.label == 7), "伪命中必须被抓");
        // 漏命中（去掉一条真命中）→ 防漏守卫必抓。
        let mut few_hits = a.hits.clone();
        let _ = few_hits.pop();
        assert!(!hits_match_predicate(&few_hits, &ledger, |x| x.label == 7), "漏命中必须被抓");
    }

    #[test]
    fn record_over_rejected() {
        let mut big = QueryLedger::new();
        let mut i = 0usize;
        while i < MAX_RECORDS {
            assert!(big.append((i % 100) as u64, (i % 32) as u16, (i % 8) as u8, 1).is_ok());
            i += 1;
        }
        assert_eq!(big.append(0, 0, 0, 1).err(), Some(QueryCode::RecordOver.val()));
        assert_eq!(big.records.len(), MAX_RECORDS);
    }

    #[test]
    fn inject_rounds_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "五路注入必须全抓：{:?}", led.caught);
    }

    #[test]
    fn probes_repro_and_guard() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn diagnostic_table_closed_set() {
        let mut i = 0usize;
        while i < QueryCode::ALL.len() {
            let c = QueryCode::ALL.get(i).copied().unwrap_or(QueryCode::NotReproducible);
            let v = c.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40, "诊断码 0x{:x} 越窗", v);
            assert!(!detail(v).is_empty());
            i += 1;
        }
        assert!(detail(0x777).contains("未知诊断码"));
    }

    #[test]
    fn checkset_counts() {
        let s = run_unx1476_checks();
        assert!(s.len() >= 12, "检查条目须 ≥12，实际 {}", s.len());
        assert!(s.all_passed(), "全链须绿：{}", s.red_items_text());
    }
}
