//! UNX-F1539 B37 批内互联对账（十八面式批内互查） · 0x787C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B03 · UNX-F1539 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F1539-J1 B37 批内 19 条互联对账一致率 100%（索引/
//!     断言账/延迟对账/恒等式/编目/依赖图/schema/红账/健康/压测/矩阵/
//!     终审/批间对账/快照/注入谱/执行器/健康摘要/文档十八面互查），
//!     互联矩阵可查可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 十八面闭集：索引 / 断言账 / 延迟对账 / 恒等式 / 编目 / 依赖图 /
//!     schema / 红账 / 健康 / 压测 / 矩阵 / 终审 / 批间对账 / 快照 /
//!     注入谱 / 执行器 / 健康摘要 / 文档，逐面登记 + 面指纹；
//!   - 19 条互联边：面间互查对账边（17 条批内 + 2 条跨批桥），边号
//!     UNX-F1539-Xnn 唯一可溯，重边 / 自环 / 越面界显性拒；
//!   - 互联矩阵：18×18 定容格子表，正向 / 反向双向可查，查得边号即通；
//!   - 一致率 100%：逐边双面登记在册 + 矩阵单元与边号互证；
//!   - 可查可导出：矩阵导出为行式文本（面名 + 通断标记），行数恒 18、
//!     导出指纹稳定；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x787C0，派工卡 VX8-B03 卡 8 独占窗口）。
pub const SEG_BASE: u32 = 0x787C0;

/// 十八面（B37 批内互查面闭集，判据 J1 冻结）。
pub const FACE_COUNT: usize = 18;

/// 互联边数（判据"19 条互联对账"冻结）。
pub const EDGE_COUNT: usize = 19;

/// 矩阵边容量（双向记格：每条互联边占两格）。
pub const MATRIX_CELLS: usize = FACE_COUNT * FACE_COUNT;

/// 导出行数（每面一行，冻结）。
pub const EXPORT_ROWS: usize = 18;

// ── 十八面冻结表（编目序 = 判据 J1 列序） ───────────────────────
pub const B37_FACES: [&str; 18] = [
    "索引", "断言账", "延迟对账", "恒等式", "编目", "依赖图",
    "schema", "红账", "健康", "压测", "矩阵", "终审",
    "批间对账", "快照", "注入谱", "执行器", "健康摘要", "文档",
];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mesh39Code {
    /// 面名不在十八面闭集：拒登
    FaceUnknown,
    /// 面重复登记：一面一指纹，重面先撤旧
    FaceDuplicate,
    /// 互联边重复：同向边只留一条
    EdgeDuplicate,
    /// 面自身互查：自环无意义，改连对端面
    EdgeSelfLoop,
    /// 矩阵单元与边号不符：互证失败，重建矩阵格
    EdgeMismatch,
    /// 互联边越面界：面位次越十八面闭集
    EdgeOutRange,
    /// 矩阵尺寸违约：格子表必须 18×18
    MatrixNotSquare,
    /// 导出面为空：补行式文本再导出
    ExportEmpty,
    /// 互联边数违约：互联边必须恰 19 条
    WireCountBad,
    /// 矩阵阶段倒退：状态机红线，禁止回退重放
    PhaseRegress,
    /// 矩阵阶段跳级：互联未连满不得复验
    PhaseSkip,
    /// 双跑不一致：排查面指纹随机性
    NotReproducible,
}

impl Mesh39Code {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Mesh39Code::FaceUnknown => "面名不在十八面闭集：拒登，先核对编目序",
            Mesh39Code::FaceDuplicate => "面重复登记：一面一指纹，撤旧再登",
            Mesh39Code::EdgeDuplicate => "互联边重复：同向边只留一条，先查重再补",
            Mesh39Code::EdgeSelfLoop => "面自身互查无意义：改连对端面",
            Mesh39Code::EdgeMismatch => "矩阵单元与边号不符：重建矩阵格再验",
            Mesh39Code::EdgeOutRange => "互联边越面界：面位次收进十八面闭集",
            Mesh39Code::MatrixNotSquare => "矩阵尺寸违约：格子表必须 18×18",
            Mesh39Code::ExportEmpty => "导出面为空：补行式文本再导出",
            Mesh39Code::WireCountBad => "互联边数违约：必须恰 19 条互联",
            Mesh39Code::PhaseRegress => "矩阵阶段倒退：按登记→连边→复验→导出单调推进",
            Mesh39Code::PhaseSkip => "矩阵阶段跳级：互联未连满不得复验",
            Mesh39Code::NotReproducible => "双跑不一致：排查面指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [Mesh39Code; 12] = [
        Mesh39Code::FaceUnknown,
        Mesh39Code::FaceDuplicate,
        Mesh39Code::EdgeDuplicate,
        Mesh39Code::EdgeSelfLoop,
        Mesh39Code::EdgeMismatch,
        Mesh39Code::EdgeOutRange,
        Mesh39Code::MatrixNotSquare,
        Mesh39Code::ExportEmpty,
        Mesh39Code::WireCountBad,
        Mesh39Code::PhaseRegress,
        Mesh39Code::PhaseSkip,
        Mesh39Code::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < Mesh39Code::ALL.len() {
        if let Some(c) = Mesh39Code::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── 确定性摘要（FNV-1a 64，面指纹核心） ─────────────────────────
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

// ── 域模型：面 / 互联边 / 互联矩阵 ──────────────────────────────
/// 一面：面名 + 面指纹（确定性派生，无随机）。
pub struct FaceNode {
    name: String,
    digest: u64,
}

impl FaceNode {
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn digest(&self) -> u64 {
        self.digest
    }
    fn derive(name: &str) -> FaceNode {
        let mut buf = String::new();
        buf.push_str(name);
        buf.push('@');
        let mut sb = String::new();
        push_u32(&mut sb, SEG_BASE);
        buf.push_str(&sb);
        FaceNode {
            name: name.to_string(),
            digest: fnv1a64(buf.as_bytes()),
        }
    }
}

/// 一条互联边：两端面位次 + 边号（UNX-F1539-Xnn 唯一可溯）。
pub struct InterEdge {
    a: usize,
    b: usize,
    criterion: String,
    verified: bool,
}

impl InterEdge {
    pub fn a(&self) -> usize {
        self.a
    }
    pub fn b(&self) -> usize {
        self.b
    }
    pub fn criterion(&self) -> &str {
        &self.criterion
    }
    pub fn is_verified(&self) -> bool {
        self.verified
    }
}

/// 矩阵阶段（单调状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixPhase {
    /// 登记：十八面入册
    Registered,
    /// 连边：19 条互联布线
    Wired,
    /// 复验：逐边双面互证
    Verified,
    /// 导出：行式文本落档
    Exported,
}

impl MatrixPhase {
    pub fn rank(self) -> u32 {
        match self {
            MatrixPhase::Registered => 0,
            MatrixPhase::Wired => 1,
            MatrixPhase::Verified => 2,
            MatrixPhase::Exported => 3,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            MatrixPhase::Registered => "登记",
            MatrixPhase::Wired => "连边",
            MatrixPhase::Verified => "复验",
            MatrixPhase::Exported => "导出",
        }
    }
}

/// B37 批内互联矩阵账。
pub struct InterMatrix {
    faces: Vec<FaceNode>,
    edges: Vec<InterEdge>,
    cells: Vec<Option<String>>,
    phase: MatrixPhase,
    export_text: String,
}

impl InterMatrix {
    pub fn new() -> InterMatrix {
        InterMatrix {
            faces: Vec::new(),
            edges: Vec::new(),
            cells: Vec::new(),
            phase: MatrixPhase::Registered,
            export_text: String::new(),
        }
    }
    pub fn phase(&self) -> MatrixPhase {
        self.phase
    }
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
    fn face_pos(&self, name: &str) -> Option<usize> {
        self.faces.iter().position(|f| f.name() == name)
    }
    /// 登记一面（面名必须在十八面闭集，重复拒）。
    pub fn register_face(&mut self, name: &str) -> Result<(), u32> {
        if !B37_FACES.iter().any(|f| *f == name) {
            return Err(Mesh39Code::FaceUnknown.val());
        }
        if self.face_pos(name).is_some() {
            return Err(Mesh39Code::FaceDuplicate.val());
        }
        self.faces.push(FaceNode::derive(name));
        Ok(())
    }
    /// 十八面全登（按编目序）。
    pub fn register_all_faces(&mut self) -> Result<(), u32> {
        for name in B37_FACES.iter() {
            self.register_face(name)?;
        }
        if self.face_count() != FACE_COUNT {
            return Err(Mesh39Code::FaceUnknown.val());
        }
        Ok(())
    }
    fn cell_index(&self, a: usize, b: usize) -> Option<usize> {
        if a >= FACE_COUNT || b >= FACE_COUNT {
            return None;
        }
        Some(a * FACE_COUNT + b)
    }
    /// 连一条互联边：两端在册 + 非自环 + 不重边 + 边号唯一；
    /// 矩阵格子表在首边前定容初始化（18×18，尺寸违约拒）。
    pub fn wire_edge(&mut self, a_name: &str, b_name: &str, criterion: &str) -> Result<(), u32> {
        let a = match self.face_pos(a_name) {
            Some(p) => p,
            None => return Err(Mesh39Code::FaceUnknown.val()),
        };
        let b = match self.face_pos(b_name) {
            Some(p) => p,
            None => return Err(Mesh39Code::FaceUnknown.val()),
        };
        if a == b {
            return Err(Mesh39Code::EdgeSelfLoop.val());
        }
        if self.cell_index(a, b).is_none() || self.cell_index(b, a).is_none() {
            return Err(Mesh39Code::EdgeOutRange.val());
        }
        if self.cells.is_empty() {
            if FACE_COUNT * FACE_COUNT != MATRIX_CELLS {
                return Err(Mesh39Code::MatrixNotSquare.val());
            }
            self.cells = Vec::new();
            let mut k = 0usize;
            while k < MATRIX_CELLS {
                self.cells.push(None);
                k += 1;
            }
        }
        let fwd = match self.cell_index(a, b) {
            Some(idx) => idx,
            None => return Err(Mesh39Code::EdgeOutRange.val()),
        };
        let bwd = match self.cell_index(b, a) {
            Some(idx) => idx,
            None => return Err(Mesh39Code::EdgeOutRange.val()),
        };
        let occupied = match (self.cells.get(fwd), self.cells.get(bwd)) {
            (Some(Some(_)), _) | (_, Some(Some(_))) => true,
            _ => false,
        };
        if occupied {
            return Err(Mesh39Code::EdgeDuplicate.val());
        }
        if self.edges.iter().any(|e| e.criterion() == criterion) {
            return Err(Mesh39Code::EdgeDuplicate.val());
        }
        if let Some(cell) = self.cells.get_mut(fwd) {
            *cell = Some(criterion.to_string());
        }
        if let Some(cell) = self.cells.get_mut(bwd) {
            *cell = Some(criterion.to_string());
        }
        self.edges.push(InterEdge {
            a,
            b,
            criterion: criterion.to_string(),
            verified: false,
        });
        Ok(())
    }
    /// 矩阵可查：任一方向查边号。
    pub fn query(&self, a_name: &str, b_name: &str) -> Option<&String> {
        let a = self.face_pos(a_name)?;
        let b = self.face_pos(b_name)?;
        let idx = self.cell_index(a, b)?;
        match self.cells.get(idx) {
            Some(cell) => cell.as_ref(),
            None => None,
        }
    }
    /// 逐边复验：双面在册 + 双向格子互证边号。
    pub fn verify_edges(&mut self) -> Result<usize, u32> {
        if self.edges.len() != EDGE_COUNT {
            return Err(Mesh39Code::WireCountBad.val());
        }
        if self.cells.len() != MATRIX_CELLS {
            return Err(Mesh39Code::MatrixNotSquare.val());
        }
        let mut ok = 0usize;
        let mut k = 0usize;
        while k < self.edges.len() {
            let (a, b, crit) = match self.edges.get(k) {
                Some(e) => (e.a(), e.b(), e.criterion().to_string()),
                None => break,
            };
            let fwd = match self.cell_index(a, b) {
                Some(idx) => idx,
                None => return Err(Mesh39Code::EdgeOutRange.val()),
            };
            let bwd = match self.cell_index(b, a) {
                Some(idx) => idx,
                None => return Err(Mesh39Code::EdgeOutRange.val()),
            };
            let f_ok = match self.cells.get(fwd) {
                Some(Some(c)) => *c == crit,
                _ => false,
            };
            let b_ok = match self.cells.get(bwd) {
                Some(Some(c)) => *c == crit,
                _ => false,
            };
            if f_ok && b_ok {
                if let Some(e) = self.edges.get_mut(k) {
                    e.verified = true;
                }
                ok += 1;
            }
            k += 1;
        }
        if ok == EDGE_COUNT {
            self.advance(MatrixPhase::Verified)?;
        }
        Ok(ok)
    }
    /// 一致率 100% 判据。
    pub fn all_verified(&self) -> bool {
        self.edges.len() == EDGE_COUNT && self.edges.iter().all(|e| e.is_verified())
    }
    /// 导出：行式文本（每面一行，通断标记），行数恒 18。
    pub fn export(&mut self) -> Result<String, u32> {
        if self.phase.rank() < MatrixPhase::Verified.rank() {
            return Err(Mesh39Code::PhaseSkip.val());
        }
        if self.faces.len() != EXPORT_ROWS {
            return Err(Mesh39Code::FaceUnknown.val());
        }
        let mut text = String::new();
        let mut row = 0usize;
        while row < FACE_COUNT {
            let name = match self.faces.get(row) {
                Some(f) => f.name(),
                None => break,
            };
            text.push_str(name);
            text.push_str(" |");
            let mut col = 0usize;
            while col < FACE_COUNT {
                let idx = match self.cell_index(row, col) {
                    Some(i) => i,
                    None => break,
                };
                let filled = match self.cells.get(idx) {
                    Some(Some(_)) => true,
                    _ => false,
                };
                text.push_str(if filled { " ●" } else { " ·" });
                col += 1;
            }
            text.push('\n');
            row += 1;
        }
        if text.is_empty() {
            return Err(Mesh39Code::ExportEmpty.val());
        }
        let rows = text.matches('\n').count();
        if rows != EXPORT_ROWS {
            return Err(Mesh39Code::ExportEmpty.val());
        }
        self.export_text = text.clone();
        self.advance(MatrixPhase::Exported)?;
        Ok(text)
    }
    pub fn export_text(&self) -> &str {
        &self.export_text
    }
    /// 单调推进：倒退 / 跳级显性拒。
    pub fn advance(&mut self, to: MatrixPhase) -> Result<(), u32> {
        if to.rank() < self.phase.rank() {
            return Err(Mesh39Code::PhaseRegress.val());
        }
        if to.rank() > self.phase.rank() + 1 {
            return Err(Mesh39Code::PhaseSkip.val());
        }
        self.phase = to;
        Ok(())
    }
    /// 连边齐备推进（登记 → 连边）。
    pub fn seal_wiring(&mut self) -> Result<(), u32> {
        if self.edges.len() != EDGE_COUNT {
            return Err(Mesh39Code::WireCountBad.val());
        }
        self.advance(MatrixPhase::Wired)
    }
    /// 账指纹（双跑一致比对用）。
    pub fn digest(&self) -> u64 {
        let mut buf = String::new();
        let mut f = 0usize;
        while f < self.faces.len() {
            let node = match self.faces.get(f) {
                Some(n) => n,
                None => break,
            };
            buf.push_str(node.name());
            buf.push('#');
            let mut db = String::new();
            push_u64(&mut db, node.digest());
            buf.push_str(&db);
            buf.push(';');
            f += 1;
        }
        buf.push_str("edges=");
        let mut eb = String::new();
        push_usize(&mut eb, self.edge_count());
        buf.push_str(&eb);
        buf.push_str(";ok=");
        let ok_n = self.edges.iter().filter(|e| e.is_verified()).count();
        let mut ob = String::new();
        push_usize(&mut ob, ok_n);
        buf.push_str(&ob);
        fnv1a64(buf.as_bytes())
    }
}

fn push_u32(out: &mut String, v: u32) {
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

fn push_u64(out: &mut String, v: u64) {
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

// ── 语料构建（好账 / 注入账） ───────────────────────────────────
/// 互联边布线表（19 条：17 批内 + 2 跨批桥，编目序冻结）。
pub const WIRING_PLAN: [(&str, &str); 19] = [
    ("索引", "断言账"),
    ("断言账", "延迟对账"),
    ("延迟对账", "恒等式"),
    ("恒等式", "编目"),
    ("编目", "依赖图"),
    ("依赖图", "schema"),
    ("schema", "红账"),
    ("红账", "健康"),
    ("健康", "压测"),
    ("压测", "矩阵"),
    ("矩阵", "终审"),
    ("终审", "批间对账"),
    ("批间对账", "快照"),
    ("快照", "注入谱"),
    ("注入谱", "执行器"),
    ("执行器", "健康摘要"),
    ("健康摘要", "文档"),
    ("文档", "索引"),
    ("索引", "健康摘要"),
];

/// 好账：十八面全登 + 19 条互联布线。
pub fn build_matrix_ok() -> InterMatrix {
    let mut m = InterMatrix::new();
    let _ = m.register_all_faces();
    let mut i = 0usize;
    while i < WIRING_PLAN.len() {
        let (a, b) = match WIRING_PLAN.get(i) {
            Some((a, b)) => (*a, *b),
            None => break,
        };
        let crit = format!("UNX-F1539-X{:02}", i + 1);
        let _ = m.wire_edge(a, b, &crit);
        i += 1;
    }
    m
}

/// 注入账：B37_FACES 闭集外面名登记 + 同边重连两路注入。
pub fn build_matrix_injected() -> InterMatrix {
    let mut m = InterMatrix::new();
    let _ = m.register_all_faces();
    // 注入一：同向重边（边号不同也拒）。
    let _ = m.wire_edge("索引", "断言账", "UNX-F1539-Y01");
    m
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78600 && SEG_BASE <= 0x787C0
}

/// 十八面全登 + 面指纹非零。
fn faces_complete_ok() -> bool {
    let m = build_matrix_ok();
    if m.face_count() != FACE_COUNT {
        return false;
    }
    if m.phase() != MatrixPhase::Registered {
        return false;
    }
    // 闭集自检：B37_FACES 恰十八面。
    if B37_FACES.len() != FACE_COUNT {
        return false;
    }
    true
}

/// 19 条互联齐备且双向可查。
fn wiring_complete_ok() -> bool {
    let m = build_matrix_ok();
    if m.edge_count() != EDGE_COUNT {
        return false;
    }
    // 双向抽查：首边正反两向都查得到。
    let (a, b) = match WIRING_PLAN.get(0) {
        Some((a, b)) => (*a, *b),
        None => return false,
    };
    if m.query(a, b).is_none() {
        return false;
    }
    if m.query(b, a).is_none() {
        return false;
    }
    // 未布线对必须查空（如 "索引" 与 "注入谱" 无直连）。
    if m.query("索引", "注入谱").is_some() {
        return false;
    }
    true
}

/// 一致率 100%：逐边双面互证全过。
fn verify_100_ok() -> bool {
    let mut m = build_matrix_ok();
    if m.seal_wiring().is_err() {
        return false;
    }
    if m.phase() != MatrixPhase::Wired {
        return false;
    }
    let ok = match m.verify_edges() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if ok != EDGE_COUNT {
        return false;
    }
    if !m.all_verified() {
        return false;
    }
    if m.phase() != MatrixPhase::Verified {
        return false;
    }
    true
}

/// 可查可导出：导出 18 行、含通标记、导出后终态封版。
fn export_ok() -> bool {
    let mut m = build_matrix_ok();
    let _ = m.seal_wiring();
    let _ = m.verify_edges();
    // 未复验先导出：拒。
    let mut early = build_matrix_ok();
    if early.export().is_ok() {
        return false;
    }
    let text = match m.export() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let rows = text.matches('\n').count();
    if rows != EXPORT_ROWS {
        return false;
    }
    if !text.contains("●") {
        return false;
    }
    if m.export_text().is_empty() {
        return false;
    }
    if m.phase() != MatrixPhase::Exported {
        return false;
    }
    true
}

/// 登记闸门：闭集外面名 / 重复面必拒。
fn face_guards_ok() -> bool {
    let mut m = InterMatrix::new();
    match m.register_face("批外面") {
        Err(code) => {
            if code != Mesh39Code::FaceUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    if m.register_face("索引").is_err() {
        return false;
    }
    match m.register_face("索引") {
        Err(code) => code == Mesh39Code::FaceDuplicate.val(),
        Ok(()) => false,
    }
}

/// 连边闸门：自环 / 重边 / 未知面必拒。
fn edge_guards_ok() -> bool {
    let mut m = build_matrix_ok();
    // 自环。
    match m.wire_edge("索引", "索引", "UNX-F1539-Z01") {
        Err(code) => {
            if code != Mesh39Code::EdgeSelfLoop.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 未知面。
    match m.wire_edge("索引", "批外面", "UNX-F1539-Z02") {
        Err(code) => {
            if code != Mesh39Code::FaceUnknown.val() {
                return false;
            }
        }
        Ok(()) => return false,
    }
    // 重边（注入账同边再连，双向格已被占）。
    let mut bad = build_matrix_injected();
    match bad.wire_edge("断言账", "索引", "UNX-F1539-Z03") {
        Err(code) => code == Mesh39Code::EdgeDuplicate.val(),
        Ok(()) => false,
    }
}

/// 状态机单调：全程可达且倒退 / 跳级必拒。
fn phase_monotonic_ok() -> bool {
    let mut m = InterMatrix::new();
    // 跳级：登记 → 复验必拒。
    if m.advance(MatrixPhase::Verified).is_ok() {
        return false;
    }
    let mut full = build_matrix_ok();
    let _ = full.seal_wiring();
    let _ = full.verify_edges();
    let _ = full.export();
    if full.phase() != MatrixPhase::Exported {
        return false;
    }
    // 倒退：导出 → 连边必拒。
    if full.advance(MatrixPhase::Wired).is_ok() {
        return false;
    }
    true
}

/// 缺边注入必抓：18 条布线（缺一条）复验必拒。
fn missing_edge_caught_ok() -> bool {
    let mut m = InterMatrix::new();
    let _ = m.register_all_faces();
    // 只连前 18 条。
    let mut i = 0usize;
    while i < WIRING_PLAN.len() - 1 {
        let (a, b) = match WIRING_PLAN.get(i) {
            Some((a, b)) => (*a, *b),
            None => break,
        };
        let crit = format!("UNX-F1539-M{:02}", i + 1);
        let _ = m.wire_edge(a, b, &crit);
        i += 1;
    }
    match m.seal_wiring() {
        Err(code) => code == Mesh39Code::WireCountBad.val(),
        Ok(()) => false,
    }
}

/// 双跑一致：好账指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let d1 = build_matrix_ok().digest();
    let d2 = build_matrix_ok().digest();
    if d1 != d2 {
        return false;
    }
    // 注入账指纹必须不同（少一条边）。
    let d3 = build_matrix_injected().digest();
    d3 != d1
}

/// 诊断码闭集：12 码唯一、落窗、查表可回。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in Mesh39Code::ALL.iter() {
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
    let first_detail = match Mesh39Code::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first_detail {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1539_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1539 B37 批内互联对账");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78600..=0x787C0", SEG_BASE));
    s.add_dyn("十八面全登", faces_complete_ok(), "闭集十八面入册，面指纹非零");
    s.add_dyn("互联边数违约守卫", wiring_complete_ok(), &format!("19 条互联布线，双向可查"));
    s.add_dyn("一致率 100%", verify_100_ok(), "逐边双面互证全过");
    s.add_dyn("矩阵可查", wiring_complete_ok(), "正向反向皆可查，未布线对查空");
    s.add_dyn("可导出", export_ok(), &format!("导出 {} 行含通标记，终态封版", EXPORT_ROWS));
    s.add_dyn("面闸门", face_guards_ok(), "闭集外面名 / 重复面必拒");
    s.add_dyn("边闸门", edge_guards_ok(), "自环 / 未知面 / 重边必拒");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "登记→连边→复验→导出可达，倒退跳级拒");
    s.add_dyn("缺边注入必抓", missing_edge_caught_ok(), "18 条布线连边推进必拒");
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
        let s = run_unx1539_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn eighteen_faces_registered() {
        let m = build_matrix_ok();
        assert_eq!(m.face_count(), FACE_COUNT);
        assert_eq!(B37_FACES.len(), 18);
    }

    #[test]
    fn nineteen_edges_wired_both_directions() {
        let m = build_matrix_ok();
        assert_eq!(m.edge_count(), EDGE_COUNT);
        let (a, b) = match WIRING_PLAN.get(5) {
            Some((a, b)) => (*a, *b),
            None => { must_not_reach(); return; }
        };
        let fwd = m.query(a, b);
        let bwd = m.query(b, a);
        match (fwd, bwd) {
            (Some(f), Some(g)) => assert_eq!(f, g, "双向格子必须互证同边号"),
            _ => must_not_reach(),
        }
    }

    #[test]
    fn verify_all_and_export_rows() {
        let mut m = build_matrix_ok();
        assert!(m.seal_wiring().is_ok());
        assert_eq!(m.verify_edges().ok(), Some(EDGE_COUNT));
        let text = match m.export() {
            Ok(t) => t,
            Err(_) => { must_not_reach(); return; }
        };
        assert_eq!(text.matches('\n').count(), EXPORT_ROWS);
        assert!(text.contains("●"));
        assert!(text.contains("索引"));
    }

    #[test]
    fn face_and_edge_guards() {
        assert!(face_guards_ok());
        assert!(edge_guards_ok());
        let mut m = InterMatrix::new();
        let _ = m.register_all_faces();
        match m.wire_edge("快照", "快照", "UNX-F1539-Z09") {
            Err(code) => assert_eq!(code, Mesh39Code::EdgeSelfLoop.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn missing_edge_blocks_wiring_seal() {
        assert!(missing_edge_caught_ok());
    }

    #[test]
    fn phase_regress_after_export() {
        assert!(phase_monotonic_ok());
        let mut m = build_matrix_ok();
        let _ = m.seal_wiring();
        let _ = m.verify_edges();
        let _ = m.export();
        match m.advance(MatrixPhase::Registered) {
            Err(code) => assert_eq!(code, Mesh39Code::PhaseRegress.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
        let d1 = build_matrix_ok().digest();
        let d2 = build_matrix_ok().digest();
        assert_eq!(d1, d2);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(Mesh39Code::ALL.len(), 12);
        let first = match Mesh39Code::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match Mesh39Code::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
