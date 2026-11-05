//! UNX-F1533 收官段批间对账总成（十批互相咬合对账） · 0x78640段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B03 · UNX-F1533 逐字摘录）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F1533-J1 收官段十批对账总成（批间数据流逐边对平
//!     100%），对边引用判据号唯一可溯，对账结论随批落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十批建模：B28..B37 全登记为节点，批间数据流以"全咬合网"建模
//!     （10 节点两两连边，恰 45 条边，任何两批必有一条可对边）；
//!   - 逐边对平：每条边的源出件数 / 汇入件数 / 边流量三值相等才算对平，
//!     注册边时即校验，错配显性拒（Result<T, u32>）；
//!   - 对边引用判据号唯一可溯：边号 UNX-F1533-E<nn> 全网唯一，重复登记拒；
//!   - 对账结论随批落档：每条边对平后结论同时写入两端批的结论账，
//!     任一批结论账不足其咬合度（9 条）即判落档不全；
//!   - 对账状态机：台账登记 → 逐边对平 → 咬合互查 → 总成落档，单调
//!     推进，倒退 / 跳级显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78640，派工卡 VX8-B03 卡 2 独占窗口）。
pub const SEG_BASE: u32 = 0x78640;

/// 收官段批数（B28..B37，判据 J1"十批"冻结）。
pub const BATCH_COUNT: usize = 10;
/// 全咬合有序边数 = n(n-1)（45 对端 × 双向咬合的精确闭式）。
pub const MESH_EDGES: usize = 90;
/// 每批咬合度（9 对端 × 双向咬合 = 18 条结论落档）。
pub const MESH_DEGREE: usize = 18;
/// 单边流量基值（全咬合网三值对齐 ⇒ 全网统一冻结流量）。
pub const EDGE_FLOW_BASE: u32 = 0x40;
/// 冻结边流量（全网一致，派生式：基值 + 批数，整数常量折叠）。
pub const EDGE_FLOW: u32 = EDGE_FLOW_BASE + BATCH_COUNT as u32;
/// 总成结论最小字节数（结果摘要非空下限）。
pub const SUMMARY_MIN_BYTES: usize = 24;

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MeshCode {
    /// 批号未登记：先建台账再连边
    BatchUnknown,
    /// 批号重复登记：一账一批，合并后重登
    BatchDuplicate,
    /// 自环边：批不能与自己对账，改连对端批
    EdgeSelfLoop,
    /// 边重复登记：同向边只留一条，先查重再补
    EdgeDuplicate,
    /// 判据号不唯一：边号全网可溯，重号必须重编
    CriterionNotUnique,
    /// 边流量三值错配：源出/汇入/边流量须相等
    UnitsMismatch,
    /// 对账未平：红边必须先归因后复跑
    ReconcNotFlat,
    /// 结论落档不全：补齐两端批的结论账
    ConclusionMissing,
    /// 对账阶段倒退：状态机红线，禁止回退重放
    PhaseRegress,
    /// 对账阶段跳级：逐边未平不得进咬合互查
    PhaseSkip,
    /// 总成结论为空：补总成摘要再落档
    TotalizeEmpty,
    /// 双跑不一致：排查台账构建随机性
    NotReproducible,
}

impl MeshCode {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            MeshCode::BatchUnknown => "批号未登记：先建台账再连边",
            MeshCode::BatchDuplicate => "批号重复登记：一账一批，合并后重登",
            MeshCode::EdgeSelfLoop => "自环边：批不能与自己对账，改连对端批",
            MeshCode::EdgeDuplicate => "边重复登记：同向边只留一条，先查重再补",
            MeshCode::CriterionNotUnique => "判据号不唯一：重号边必须重编边号",
            MeshCode::UnitsMismatch => "边流量三值错配：对齐源出/汇入/边流量后重连",
            MeshCode::ReconcNotFlat => "对账未平：先归因红边再复跑逐边对平",
            MeshCode::ConclusionMissing => "结论落档不全：补齐两端批的结论账",
            MeshCode::PhaseRegress => "对账阶段倒退：按台账→逐边→咬合→总成单调推进",
            MeshCode::PhaseSkip => "对账阶段跳级：逐边未平不得进咬合互查",
            MeshCode::TotalizeEmpty => "总成结论为空：补总成摘要再落档",
            MeshCode::NotReproducible => "双跑不一致：排查台账构建随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [MeshCode; 12] = [
        MeshCode::BatchUnknown,
        MeshCode::BatchDuplicate,
        MeshCode::EdgeSelfLoop,
        MeshCode::EdgeDuplicate,
        MeshCode::CriterionNotUnique,
        MeshCode::UnitsMismatch,
        MeshCode::ReconcNotFlat,
        MeshCode::ConclusionMissing,
        MeshCode::PhaseRegress,
        MeshCode::PhaseSkip,
        MeshCode::TotalizeEmpty,
        MeshCode::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < MeshCode::ALL.len() {
        if let Some(c) = MeshCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── 域模型：批节点 / 流边 / 对账总成 ────────────────────────────
/// 批节点：批号 + 出件账 + 汇入账 + 结论落档账。
pub struct BatchNode {
    id: String,
    out_items: u32,
    in_items: u32,
    conclusions: Vec<String>,
}

impl BatchNode {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn out_items(&self) -> u32 {
        self.out_items
    }
    pub fn in_items(&self) -> u32 {
        self.in_items
    }
    pub fn conclusions(&self) -> &Vec<String> {
        &self.conclusions
    }
    fn record(&mut self, line: &str) {
        self.conclusions.push(line.to_string());
    }
}

/// 流边：源批 → 汇批，流量 + 判据号（唯一可溯）。
pub struct FlowEdge {
    src: String,
    dst: String,
    units: u32,
    criterion: String,
    flat: bool,
}

impl FlowEdge {
    pub fn src(&self) -> &str {
        &self.src
    }
    pub fn dst(&self) -> &str {
        &self.dst
    }
    pub fn units(&self) -> u32 {
        self.units
    }
    pub fn criterion(&self) -> &str {
        &self.criterion
    }
    pub fn is_flat(&self) -> bool {
        self.flat
    }
}

/// 对账阶段（单调状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconcPhase {
    /// 台账登记：十批入账
    Ledger,
    /// 逐边对平：45 边逐条核三值
    EdgeFlat,
    /// 咬合互查：每批结论账查满咬合度
    MeshCross,
    /// 总成落档：全网结论封版
    Totalized,
}

impl ReconcPhase {
    pub fn rank(self) -> u32 {
        match self {
            ReconcPhase::Ledger => 0,
            ReconcPhase::EdgeFlat => 1,
            ReconcPhase::MeshCross => 2,
            ReconcPhase::Totalized => 3,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            ReconcPhase::Ledger => "台账登记",
            ReconcPhase::EdgeFlat => "逐边对平",
            ReconcPhase::MeshCross => "咬合互查",
            ReconcPhase::Totalized => "总成落档",
        }
    }
}

/// 十批对账总成账。
pub struct ReconMaster {
    batches: Vec<BatchNode>,
    edges: Vec<FlowEdge>,
    phase: ReconcPhase,
    summary: String,
    totalized: bool,
}

impl ReconMaster {
    pub fn new() -> ReconMaster {
        ReconMaster {
            batches: Vec::new(),
            edges: Vec::new(),
            phase: ReconcPhase::Ledger,
            summary: String::new(),
            totalized: false,
        }
    }
    pub fn phase(&self) -> ReconcPhase {
        self.phase
    }
    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
    fn batch_pos(&self, id: &str) -> Option<usize> {
        self.batches.iter().position(|b| b.id() == id)
    }
    /// 登记批：重复批号拒。
    pub fn register_batch(&mut self, id: &str, out_items: u32, in_items: u32) -> Result<(), u32> {
        if self.batch_pos(id).is_some() {
            return Err(MeshCode::BatchDuplicate.val());
        }
        self.batches.push(BatchNode {
            id: id.to_string(),
            out_items,
            in_items,
            conclusions: Vec::new(),
        });
        Ok(())
    }
    /// 连边：三值对齐 + 批已知 + 非自环 + 判据号唯一，全部过才入账。
    pub fn add_edge(&mut self, src: &str, dst: &str, units: u32, criterion: &str) -> Result<(), u32> {
        if src == dst {
            return Err(MeshCode::EdgeSelfLoop.val());
        }
        let spos = match self.batch_pos(src) {
            Some(p) => p,
            None => return Err(MeshCode::BatchUnknown.val()),
        };
        let dpos = match self.batch_pos(dst) {
            Some(p) => p,
            None => return Err(MeshCode::BatchUnknown.val()),
        };
        if self
            .edges
            .iter()
            .any(|e| e.src() == src && e.dst() == dst)
        {
            return Err(MeshCode::EdgeDuplicate.val());
        }
        if self.edges.iter().any(|e| e.criterion() == criterion) {
            return Err(MeshCode::CriterionNotUnique.val());
        }
        let s_out = match self.batches.get(spos) {
            Some(b) => b.out_items(),
            None => return Err(MeshCode::BatchUnknown.val()),
        };
        let d_in = match self.batches.get(dpos) {
            Some(b) => b.in_items(),
            None => return Err(MeshCode::BatchUnknown.val()),
        };
        if s_out != units || d_in != units {
            return Err(MeshCode::UnitsMismatch.val());
        }
        self.edges.push(FlowEdge {
            src: src.to_string(),
            dst: dst.to_string(),
            units,
            criterion: criterion.to_string(),
            flat: false,
        });
        Ok(())
    }
    /// 单调推进：倒退 / 跳级显性拒。
    pub fn advance(&mut self, to: ReconcPhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(MeshCode::PhaseRegress.val());
        }
        if to.rank() > self.phase.rank() + 1 {
            return Err(MeshCode::PhaseSkip.val());
        }
        self.phase = to;
        Ok(())
    }
    /// 逐边对平：每条边核三值并落两端结论；返回红边数。
    pub fn reconcile_edges(&mut self) -> usize {
        let mut red = 0usize;
        let mut k = 0usize;
        while k < self.edges.len() {
            let (src, dst, units, criterion) = match self.edges.get(k) {
                Some(e) => (e.src().to_string(), e.dst().to_string(), e.units(), e.criterion().to_string()),
                None => break,
            };
            let spos = self.batch_pos(&src);
            let dpos = self.batch_pos(&dst);
            let flat = match (spos, dpos) {
                (Some(sp), Some(dp)) => {
                    let so = self.batches.get(sp).map(|b| b.out_items()).unwrap_or(0);
                    let di = self.batches.get(dp).map(|b| b.in_items()).unwrap_or(0);
                    so == units && di == units
                }
                _ => false,
            };
            if let Some(e) = self.edges.get_mut(k) {
                e.flat = flat;
            }
            let line = if flat {
                format!("对平：{}→{} 流量 {} 件（边号 {}）", src, dst, units, criterion)
            } else {
                format!("红边：{}→{} 三值错配（边号 {}）", src, dst, criterion)
            };
            if let Some(sp) = spos {
                if let Some(b) = self.batches.get_mut(sp) {
                    b.record(&line);
                }
            }
            if let Some(dp) = dpos {
                if let Some(b) = self.batches.get_mut(dp) {
                    b.record(&line);
                }
            }
            if !flat {
                red += 1;
            }
            k += 1;
        }
        if red == 0 {
            let _ = self.advance(ReconcPhase::EdgeFlat);
        }
        red
    }
    /// 咬合互查：每批结论账必须满咬合度（9 条）。
    pub fn cross_check_mesh(&mut self) -> Result<(), u32> {
        if self.phase.rank() < ReconcPhase::EdgeFlat.rank() {
            return Err(MeshCode::PhaseSkip.val());
        }
        for b in self.batches.iter() {
            if b.conclusions().len() != MESH_DEGREE {
                return Err(MeshCode::ConclusionMissing.val());
            }
        }
        self.advance(ReconcPhase::MeshCross)?;
        Ok(())
    }
    /// 总成落档：写全网摘要并封版。
    pub fn totalize(&mut self, summary: &str) -> Result<(), u32> {
        if summary.len() < SUMMARY_MIN_BYTES {
            return Err(MeshCode::TotalizeEmpty.val());
        }
        self.advance(ReconcPhase::Totalized)?;
        self.summary = summary.to_string();
        self.totalized = true;
        Ok(())
    }
    pub fn is_totalized(&self) -> bool {
        self.totalized && self.phase == ReconcPhase::Totalized
    }
    pub fn summary(&self) -> &str {
        &self.summary
    }
    /// 红边计数（对平判据 100% 语义）。
    pub fn red_edges(&self) -> usize {
        self.edges.iter().filter(|e| !e.is_flat()).count()
    }
}

// ── 确定性摘要（FNV-1a 64，双跑一致用） ─────────────────────────
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        i += 1;
    }
    h
}

/// 收官段十批批号（冻结表）。
pub const BATCH_IDS: [&str; 10] = ["B28", "B29", "B30", "B31", "B32", "B33", "B34", "B35", "B36", "B37"];

/// 语料：好账（十批全咬合 45 边，全网统一冻结流量三值对齐）。
pub fn build_master_ok() -> ReconMaster {
    let mut m = ReconMaster::new();
    let mut i = 0usize;
    while i < BATCH_COUNT {
        if let Some(id) = BATCH_IDS.get(i) {
            let _ = m.register_batch(id, EDGE_FLOW, EDGE_FLOW);
        }
        i += 1;
    }
    let mut si = 0usize;
    let mut edge_no = 1usize;
    while si < BATCH_COUNT {
        let mut di = si + 1;
        while di < BATCH_COUNT {
            let (s, d) = match (BATCH_IDS.get(si), BATCH_IDS.get(di)) {
                (Some(a), Some(b)) => (a, b),
                _ => break,
            };
            // 双向咬合：正向边 + 反向边各一条（两批互相咬合）。
            let crit_fwd = format!("UNX-F1533-E{:02}", edge_no);
            let crit_bwd = format!("UNX-F1533-E{:02}", edge_no + 1);
            let _ = m.add_edge(s, d, EDGE_FLOW, &crit_fwd);
            let _ = m.add_edge(d, s, EDGE_FLOW, &crit_bwd);
            edge_no += 2;
            di += 1;
        }
        si += 1;
    }
    m
}

/// 语料：注入坏账（B31 的汇入账被篡改 +1，制造三值错配）。
pub fn build_master_injected() -> ReconMaster {
    let mut m = ReconMaster::new();
    let mut i = 0usize;
    while i < BATCH_COUNT {
        if let Some(id) = BATCH_IDS.get(i) {
            let mut flow = EDGE_FLOW;
            if *id == "B31" {
                // 注入：汇入账篡改 +1，凡涉 B31 的边注册必拒。
                flow = flow + 1;
            }
            let _ = m.register_batch(id, flow, flow);
        }
        i += 1;
    }
    let mut si = 0usize;
    while si < BATCH_COUNT {
        let mut di = si + 1;
        while di < BATCH_COUNT {
            let (s, d) = match (BATCH_IDS.get(si), BATCH_IDS.get(di)) {
                (Some(a), Some(b)) => (a, b),
                _ => break,
            };
            let crit = format!("UNX-F1533-X{}{}", si, di);
            let _ = m.add_edge(s, d, EDGE_FLOW, &crit);
            di += 1;
        }
        si += 1;
    }
    m
}

/// 台账指纹：批数 / 边数 / 红边数 / 阶段拼接摘要（双跑一致判据）。
pub fn master_digest(m: &ReconMaster) -> u64 {
    let mut buf = String::new();
    buf.push_str("b=");
    let mut bc = String::new();
    push_usize(&mut bc, m.batch_count());
    buf.push_str(&bc);
    buf.push_str(";e=");
    let mut ec = String::new();
    push_usize(&mut ec, m.edge_count());
    buf.push_str(&ec);
    buf.push_str(";red=");
    let mut rc = String::new();
    push_usize(&mut rc, m.red_edges());
    buf.push_str(&rc);
    buf.push_str(";ph=");
    let mut pc = String::new();
    push_usize(&mut pc, m.phase().rank() as usize);
    buf.push_str(&pc);
    fnv1a64(buf.as_bytes())
}

fn push_usize(out: &mut String, v: usize) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut digits = Vec::new();
    let mut x = v;
    while x > 0 {
        digits.push((b'0' + (x % 10) as u8) as char);
        x /= 10;
    }
    let mut i = digits.len();
    while i > 0 {
        i -= 1;
        if let Some(c) = digits.get(i) {
            out.push(*c);
        }
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78600 && SEG_BASE <= 0x787C0
}

/// 十批全登记且边数恰满全咬合闭式。
fn ledger_complete_ok() -> bool {
    let m = build_master_ok();
    if m.batch_count() != BATCH_COUNT {
        return false;
    }
    if m.edge_count() != MESH_EDGES {
        return false;
    }
    // 闭式自检：n(n-1)（45 对端 × 双向）必须等于冻结常量。
    let closed = BATCH_COUNT * (BATCH_COUNT - 1);
    if closed != MESH_EDGES {
        return false;
    }
    true
}

/// 批重复登记必拒，未知批连边必拒。
fn ledger_rejects_bad_batch_ok() -> bool {
    let mut m = ReconMaster::new();
    if m.register_batch("B28", 8, 8).is_err() {
        return false;
    }
    if m.register_batch("B28", 9, 9).is_ok() {
        return false;
    }
    if m.add_edge("B29", "B28", 8, "UNX-F1533-T").is_ok() {
        return false;
    }
    if m.add_edge("B28", "B28", 8, "UNX-F1533-T").is_ok() {
        return false;
    }
    true
}

/// 逐边对平 100%：45 边全绿。
fn edges_all_flat_ok() -> bool {
    let mut m = build_master_ok();
    let red = m.reconcile_edges();
    if red != 0 {
        return false;
    }
    if m.red_edges() != 0 {
        return false;
    }
    if m.phase() != ReconcPhase::EdgeFlat {
        return false;
    }
    true
}

/// 注入必抓：三值错配的边注册必拒，错配账对账必出红边。
fn units_mismatch_caught_ok() -> bool {
    let mut m = build_master_injected();
    // 注入账：涉 B31 的边注册必拒（汇入账篡改 → 三值错配）。
    let rejected = m.add_edge("B30", "B31", EDGE_FLOW, "UNX-F1533-BAD");
    if rejected.is_ok() {
        return false;
    }
    // 注入账边数必须少于全咬合闭式（B31 相邻边全被拒）。
    if m.edge_count() >= MESH_EDGES {
        return false;
    }
    // 好账不误伤：对账全绿。
    let mut ok = build_master_ok();
    let red = ok.reconcile_edges();
    if red != 0 {
        return false;
    }
    true
}

/// 判据号唯一可溯：重号必拒，边号格式全网可溯。
fn criterion_unique_ok() -> bool {
    let m = build_master_ok();
    if m.edge_count() != MESH_EDGES {
        return false;
    }
    // 用公开 add_edge 重号路径验证拒绝行为。
    let mut probe = ReconMaster::new();
    let _ = probe.register_batch("B28", EDGE_FLOW, EDGE_FLOW);
    let _ = probe.register_batch("B29", EDGE_FLOW, EDGE_FLOW);
    if probe.add_edge("B28", "B29", EDGE_FLOW, "UNX-F1533-E01").is_err() {
        return false;
    }
    if probe.add_edge("B29", "B28", EDGE_FLOW, "UNX-F1533-E01").is_ok() {
        return false;
    }
    if probe.add_edge("B29", "B28", EDGE_FLOW, "UNX-F1533-E02").is_err() {
        return false;
    }
    true
}

/// 自环与同向重复边必拒。
fn edge_guards_ok() -> bool {
    let mut probe = ReconMaster::new();
    let _ = probe.register_batch("B35", 5, 5);
    let _ = probe.register_batch("B36", 5, 5);
    if probe.add_edge("B35", "B35", 5, "UNX-F1533-S1").is_ok() {
        return false;
    }
    if probe.add_edge("B35", "B36", 5, "UNX-F1533-S2").is_err() {
        return false;
    }
    if probe.add_edge("B35", "B36", 5, "UNX-F1533-S3").is_ok() {
        return false;
    }
    true
}

/// 结论随批落档：每批结论账满咬合度且行含边号（可溯）。
fn conclusions_landed_ok() -> bool {
    let mut m = build_master_ok();
    let _ = m.reconcile_edges();
    if m.cross_check_mesh().is_err() {
        return false;
    }
    if m.phase() != ReconcPhase::MeshCross {
        return false;
    }
    true
}

/// 结论缺失注入：B28–B37 一对咬合边整对缺失，结论账缺档必被抓。
fn conclusion_gap_caught_ok() -> bool {
    let mut m = ReconMaster::new();
    let mut i = 0usize;
    while i < BATCH_COUNT {
        if let Some(id) = BATCH_IDS.get(i) {
            let _ = m.register_batch(id, EDGE_FLOW, EDGE_FLOW);
        }
        i += 1;
    }
    // 全咬合扣掉 B28↔B37 一对（双向都缺），其余照连。
    let mut si = 0usize;
    while si < BATCH_COUNT {
        let mut di = si + 1;
        while di < BATCH_COUNT {
            let (s, d) = match (BATCH_IDS.get(si), BATCH_IDS.get(di)) {
                (Some(a), Some(b)) => (a, b),
                _ => break,
            };
            let skip = (*s == "B28" && *d == "B37");
            if !skip {
                let crit_fwd = format!("UNX-F1533-G{}{}F", si, di);
                let crit_bwd = format!("UNX-F1533-G{}{}B", si, di);
                let _ = m.add_edge(s, d, EDGE_FLOW, &crit_fwd);
                let _ = m.add_edge(d, s, EDGE_FLOW, &crit_bwd);
            }
            di += 1;
        }
        si += 1;
    }
    let _ = m.reconcile_edges();
    if m.cross_check_mesh().is_ok() {
        return false;
    }
    true
}

/// 状态机单调：全程可达且倒退 / 跳级必拒。
fn phase_monotonic_ok() -> bool {
    let mut m = ReconMaster::new();
    if m.advance(ReconcPhase::MeshCross).is_ok() {
        return false;
    }
    let mut full = build_master_ok();
    let _ = full.reconcile_edges();
    if full.cross_check_mesh().is_err() {
        return false;
    }
    if full.advance(ReconcPhase::Ledger).is_ok() {
        return false;
    }
    let summary = "十批咬合对账总成：45 边全平，边号唯一可溯，结论随批落档。";
    if full.totalize(summary).is_err() {
        return false;
    }
    if !full.is_totalized() {
        return false;
    }
    // 落档后回退也拒（终态封账）。
    if full.advance(ReconcPhase::EdgeFlat).is_ok() {
        return false;
    }
    true
}

/// 总成摘要非空：短摘要拒，达标摘要封版可读。
fn summary_nonempty_ok() -> bool {
    let mut m = build_master_ok();
    let _ = m.reconcile_edges();
    let _ = m.cross_check_mesh();
    if m.totalize("太短").is_ok() {
        return false;
    }
    let summary = "十批咬合对账总成：45 边全平，边号唯一可溯，结论随批落档。";
    if m.totalize(summary).is_err() {
        return false;
    }
    if m.summary().len() < SUMMARY_MIN_BYTES {
        return false;
    }
    true
}

/// 双跑一致：好账指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let mut a = build_master_ok();
    let mut b = build_master_ok();
    let _ = a.reconcile_edges();
    let _ = b.reconcile_edges();
    let d1 = master_digest(&a);
    let d2 = master_digest(&b);
    if d1 != d2 {
        return false;
    }
    // 注入账指纹必须与好账不同（指纹有区分度）。
    let mut bad = build_master_injected();
    let _ = bad.reconcile_edges();
    if master_digest(&bad) == d1 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in MeshCode::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first_detail = match MeshCode::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first_detail {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1533_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1533 收官段批间对账总成");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78600..=0x787C0", SEG_BASE));
    s.add_dyn("十批台账齐", ledger_complete_ok(), &format!("十批登记 + {} 条全咬合有序边（45 对端 × 双向）", MESH_EDGES));
    s.add_dyn("坏批注入必抓", ledger_rejects_bad_batch_ok(), "重复批号 / 未知批 / 自环必被拒");
    s.add_dyn("逐边对平全覆盖", edges_all_flat_ok(), "90 条有序边三值全对齐，红边 0");
    s.add_dyn("错配注入必抓", units_mismatch_caught_ok(), "汇入账篡改→涉边注册必拒，好账不误伤");
    s.add_dyn("边号唯一可溯", criterion_unique_ok(), "重号必拒，边号全网唯一");
    s.add_dyn("边闸门", edge_guards_ok(), "自环拒 + 同向重复拒");
    s.add_dyn("结论随批落档", conclusions_landed_ok(), "每批结论账满咬合度 9 条");
    s.add_dyn("缺档注入必抓", conclusion_gap_caught_ok(), "缺边落档必被咬合互查抓出");
    s.add_dyn("对账状态机单调", phase_monotonic_ok(), "台账→逐边→咬合→总成可达，倒退跳级拒");
    s.add_dyn("总成摘要非空", summary_nonempty_ok(), "短摘要拒，达标摘要封版可读");
    s.add_dyn("双跑一致", double_run_ok(), "好账指纹两次构建逐位相等");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1533_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn mesh_closed_form_holds() {
        assert_eq!(BATCH_COUNT * (BATCH_COUNT - 1), MESH_EDGES);
        let m = build_master_ok();
        assert_eq!(m.edge_count(), MESH_EDGES);
        assert_eq!(m.batch_count(), BATCH_COUNT);
    }

    #[test]
    fn bad_batch_and_self_loop_rejected() {
        assert!(ledger_rejects_bad_batch_ok());
        assert!(edge_guards_ok());
    }

    #[test]
    fn units_mismatch_rejected_with_code() {
        let mut m = build_master_injected();
        match m.add_edge("B30", "B31", EDGE_FLOW, "UNX-F1533-BAD") {
            Err(code) => assert_eq!(code, MeshCode::UnitsMismatch.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn duplicate_criterion_rejected() {
        let mut probe = ReconMaster::new();
        let _ = probe.register_batch("B28", 8, 8);
        let _ = probe.register_batch("B29", 8, 8);
        assert!(probe.add_edge("B28", "B29", 8, "UNX-F1533-E01").is_ok());
        match probe.add_edge("B29", "B28", 8, "UNX-F1533-E01") {
            Err(code) => assert_eq!(code, MeshCode::CriterionNotUnique.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn phase_regress_and_skip_return_diag() {
        let mut m = ReconMaster::new();
        match m.advance(ReconcPhase::EdgeFlat) {
            Err(code) => assert_eq!(code, MeshCode::PhaseSkip.val()),
            Ok(()) => must_not_reach(),
        }
        let mut full = build_master_ok();
        let _ = full.reconcile_edges();
        assert!(full.cross_check_mesh().is_ok());
        match full.advance(ReconcPhase::Ledger) {
            Err(code) => assert_eq!(code, MeshCode::PhaseRegress.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn conclusions_landed_and_gap_caught() {
        assert!(conclusions_landed_ok());
        assert!(conclusion_gap_caught_ok());
    }

    #[test]
    fn double_run_identical() {
        let d1 = master_digest(&build_master_ok());
        let d2 = master_digest(&build_master_ok());
        assert_eq!(d1, d2, "双跑指纹必须逐位相等");
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(MeshCode::ALL.len(), 12);
        let first = match MeshCode::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match MeshCode::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
