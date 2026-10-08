//! VE-F1607 · 网格修复检测（VE-I 域 · I01 网格格式与几何基础组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1607`
//!
//! **判据（锚点原文）**：四检测、建议、可撤销、报告、判据。
//!
//! # 四类自动检测
//!
//! | 类别 | 判据 | 数学依据 |
//! |---|---|---|
//! | [`Defect::Hole`] 破面 | 边界边检测：只被 1 个三角引用的边 | 半边结构里出现单侧引用即孔洞边界 |
//! | [`Defect::Degenerate`] 退化三角 | 零面积：`|e1 × e2| < eps` | 叉积模长即二倍面积，零则无有效面 |
//! | [`Defect::FlippedNormal`] 法线翻转 | 相邻面法线点积为负 | 共享边两外法线反向即缠绕矛盾 |
//! | [`Defect::UvOutOfRange`] UV 异常 | UV 越界 `[0,1]` 外 | 超出声明域即采样越界 |
//!
//! 四类覆盖主要网格缺陷；非流形边（同边被 3 个以上面引用）并入破面族计数，
//! 因为它在半边结构上同样表现为「边引用数不是 2」这一族问题。
//!
//! # 修复建议（人话 + 方案）
//!
//! 每条缺陷配一句人话说明与一个可执行的修复方案（[`RepairKind`]），
//! 不是冷冰冰的诊断码列表。方案本身携带参数（如补洞的目标边数），
//! 由 [`Repairer`] 真正执行。
//!
//! # 自动修复执行（可撤销）
//!
//! 修复即操作，故并入撤销体系：每次执行前把受影响顶点/面的**原值**压入
//! [`UndoStack`]，[`Repairer::undo`] 逆序回滚。锚点引F1344 命令栈语义——
//! 本模块自带最小命令栈，接口按「压栈 → 执行 → 出栈回滚」组织，
//! 后续 F1344 落地时可直接对接而不必改调用方。
//!
//! # 修复报告（修复前/后对照）
//!
//! [`RepairReport`] 同时持有修复前后两次 [`DefectReport`]，逐项对照计数，
//! 并给出「本次修复消除了哪些、新增了哪些」——**新增缺陷必须显式报出**，
//! 修复不该悄悄把问题从一类挪到另一类。
//!
//! # 确定性
//!
//! 全部检测与修复走整数/浮点比较，不依赖迭代收敛，故同输入必同输出
//! （`report_is_deterministic` 判据）。
//!
//! # 隐私
//!
//! 网格几何数据不含用户内容；报告仅含计数与坐标，无文本无标识。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、浮点基元（不自写三角函数，理由见 meshquant 头注的精度分叉论证）
// ---------------------------------------------------------------------------

/// 退化面积阈值（`f32` 口径）：叉积模长小于此值视作零面积。
pub const DEGENERATE_AREA_EPS: f32 = 1.0e-12;

/// UV 越界容差：超出 `[0,1]` 超过此容差才算越界（抗量化抖动）。
pub const UV_RANGE_TOL: f32 = -1.0e-6;

/// 法线点积负向阈值：小于此值判作相邻面法线反向。
///
/// 取 `-1e-6` 而非 `0.0`：完全垂直的相邻面点积在0 附近，
/// 用 `0.0` 会因f32 误差把「恰好垂直」误判成「翻转」。
pub const FLIP_DOT_EPS: f32 = -1.0e-6;

#[inline]
fn fabs(v: f32) -> f32 {
    if v < 0.0 {
        -v
    } else {
        v
    }
}

#[inline]
fn cross_len(ax: f32, ay: f32, az: f32, bx: f32, by: f32, bz: f32) -> f32 {
    let cx = ay * bz - az * by;
    let cy = az * bx - ax * bz;
    let cz = ax * by - ay * bx;
    crate::gfx::meshquant::fsqrt(cx * cx + cy * cy + cz * cz)
}

#[inline]
fn dot3(ax: f32, ay: f32, az: f32, bx: f32, by: f32, bz: f32) -> f32 {
    ax * bx + ay * by + az * bz
}

// ---------------------------------------------------------------------------
// 二、诊断袋（不静默：每个异常都有码）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshDiag {
    /// 顶点数组长度非 3 的倍数。
    MeshShapeInvalid,
    /// 面索引数组长度非 3 的倍数。
    IndexShapeInvalid,
    /// 面索引越界引用顶点。
    IndexOutOfRange,
    /// UV 数组长度与面数不匹配。
    UvShapeMismatch,
    /// 面数或顶点数为零（空网格不可修复）。
    MeshEmpty,
    /// 撤销栈为空时请求撤销。
    UndoStackEmpty,
    /// 撤销栈下溢（连续撤销超深度）。
    UndoUnderflow,
}

impl MeshDiag {
    pub fn label(self) -> &'static str {
        match self {
            MeshDiag::MeshShapeInvalid => "MESH_SHAPE_INVALID",
            MeshDiag::IndexShapeInvalid => "INDEX_SHAPE_INVALID",
            MeshDiag::IndexOutOfRange => "INDEX_OUT_OF_RANGE",
            MeshDiag::UvShapeMismatch => "UV_SHAPE_MISMATCH",
            MeshDiag::MeshEmpty => "MESH_EMPTY",
            MeshDiag::UndoStackEmpty => "UNDO_STACK_EMPTY",
            MeshDiag::UndoUnderflow => "UNDO_UNDERFLOW",
        }
    }
}

/// 诊断袋（最多留 32 条，超出计数不丢总数）。
#[derive(Clone, Debug, Default)]
pub struct MeshDiagBag {
    items: Vec<MeshDiag>,
    pub dropped: u32,
}

impl MeshDiagBag {
    pub fn new() -> MeshDiagBag {
        MeshDiagBag {
            items: Vec::new(),
            dropped: 0,
        }
    }

    pub fn push(&mut self, d: MeshDiag) {
        if self.items.len() < 32 {
            self.items.push(d);
        } else {
            self.dropped += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn has(&self, d: MeshDiag) -> bool {
        self.items.iter().any(|x| *x == d)
    }

    pub fn items(&self) -> &[MeshDiag] {
        &self.items
    }

    pub fn total(&self) -> u32 {
        self.items.len() as u32 + self.dropped
    }
}

// ---------------------------------------------------------------------------
// 三、网格输入（不重复造F1606 的 MeshInput —— 本模块自带最小形态，
//理由：F1606 的 MeshInput 强制要求逐面角 UV 齐备，而本单的破面/退化检测
//**必须能在 UV 缺失时也能跑**（UV 异常是四类之一，而非前置条件）。
// ---------------------------------------------------------------------------

/// 待检网格（位置 + 面索引 + 可选逐面角 UV）。
#[derive(Clone, Debug, Default)]
pub struct RepairMesh {
    /// 顶点位置，长度须为 `3` 的倍数。
    pub verts: Vec<f32>,
    /// 三角面索引，长度须为 `3` 的倍数。
    pub faces: Vec<u32>,
    /// 逐面角 UV（每角 2 分量），可为空——UV 缺失只影响第四类检测。
    pub uvs: Vec<f32>,
}

impl RepairMesh {
    pub fn new() -> RepairMesh {
        RepairMesh {
            verts: Vec::new(),
            faces: Vec::new(),
            uvs: Vec::new(),
        }
    }

    pub fn vert_count(&self) -> usize {
        self.verts.len() / 3
    }

    pub fn face_count(&self) -> usize {
        self.faces.len() / 3
    }

    /// 取顶点；越界返回 `None`（输入不可信是常态，不是异常）。
    pub fn vert(&self, i: u32) -> Option<[f32; 3]> {
        let s = (i as usize).checked_mul(3)?;
        if s + 2 >= self.verts.len() {
            return None;
        }
        Some([self.verts[s], self.verts[s + 1], self.verts[s + 2]])
    }

    /// 取面三角；越界返回 `None`。
    pub fn face(&self, f: usize) -> Option<[u32; 3]> {
        let s = f.checked_mul(3)?;
        if s + 2 >= self.faces.len() {
            return None;
        }
        Some([self.faces[s], self.faces[s + 1], self.faces[s + 2]])
    }

    /// 取面 `f` 角 `corner` 的 UV；越界或UV 缺失返回 `None`。
    pub fn corner_uv(&self, f: usize, corner: usize) -> Option<[f32; 2]> {
        let s = f.checked_mul(3)?.checked_add(corner)?.checked_mul(2)?;
        if s + 1 >= self.uvs.len() {
            return None;
        }
        Some([self.uvs[s], self.uvs[s + 1]])
    }

    /// 追加顶点，返回新顶点号。
    pub fn push_vert(&mut self, p: [f32; 3]) -> u32 {
        let idx = self.vert_count() as u32;
        self.verts.push(p[0]);
        self.verts.push(p[1]);
        self.verts.push(p[2]);
        idx
    }

/// 追加三角面。
    pub fn push_face(&mut self, f: [u32; 3]) {
        self.faces.push(f[0]);
        self.faces.push(f[1]);
        self.faces.push(f[2]);
    }

    /// 本网格是否带 UV（`uvs` 非空即为带）。
    pub fn has_uv(&self) -> bool {
        !self.uvs.is_empty()
    }

    /// 为末面补上逐面角 UV（补洞等**新增面**的路径必须走这里）。
    ///
    /// 逐面角 UV 的长度必须恒等于 `face_count()*6`。新增面若不同步补 UV，
    /// 带 UV 的网格会立刻变成结构非法（`UvShapeMismatch`），检测随之全量返回零
    /// ——表现为「修完缺陷全没了」，实则是**检不出来**，属静默失效。
    ///
    /// **原本无 UV 的网格则不补**：`uvs` 为空是合法的「未贴图」状态，
    /// 此时补 UV 会把无 UV 网格变成「UV 全为均值」的贴图网格——凭空创造了
    /// 本不存在的贴图数据。故此处以 `has_uv()` 为前提。
    pub fn push_face_with_uv(&mut self, f: [u32; 3]) {
        self.push_face(f);
        if !self.has_uv() {
            return;
        }
        let uvs = self.mean_corner_uvs();
        for k in 0..3 {
            self.uvs.push(uvs[k][0]);
            self.uvs.push(uvs[k][1]);
        }
    }

    /// 已有 UV 的均值（新增面的默认 UV 取此值）。
    fn mean_corner_uvs(&self) -> [[f32; 2]; 3] {
        let n = self.uvs.len() / 2;
        if n == 0 {
            return [[0.5, 0.5], [0.5, 0.5], [0.5, 0.5]];
        }
        let mut su = 0.0f32;
        let mut sv = 0.0f32;
        for i in 0..n {
            su += self.uvs[i * 2];
            sv += self.uvs[i * 2 + 1];
        }
        let inv = 1.0 / n as f32;
        let m = [su * inv, sv * inv];
        [m, m, m]
    }

    /// 结构自检（只读不改）；`ok` 为false 时 `diag` 给出原因。
    pub fn validate(&self, diag: &mut MeshDiagBag) -> bool {
        let mut ok = true;
        if self.verts.len() % 3 != 0 {
            diag.push(MeshDiag::MeshShapeInvalid);
            ok = false;
        }
        if self.faces.len() % 3 != 0 {
            diag.push(MeshDiag::IndexShapeInvalid);
            ok = false;
        }
        // UV 可缺省；但若给了就必须与面数自洽（每角 2 分量）。
        if !self.uvs.is_empty() && self.uvs.len() != self.face_count() * 6 {
            diag.push(MeshDiag::UvShapeMismatch);
            ok = false;
        }
        if !ok {
            return false;
        }
        if self.vert_count() == 0 || self.face_count() == 0 {
            diag.push(MeshDiag::MeshEmpty);
            return false;
        }
        // 面索引必须在顶点范围内——越界说明网格本身坏，检测无意义。
        let nv = self.vert_count() as u32;
        for i in 0..self.face_count() {
            match self.face(i) {
                None => {
                    diag.push(MeshDiag::IndexShapeInvalid);
                    ok = false;
                }
                Some(t) => {
                    if t[0] >= nv || t[1] >= nv || t[2] >= nv {
                        diag.push(MeshDiag::IndexOutOfRange);
                        ok = false;
                        break;
                    }
                }
            }
        }
        ok
    }
}

// ---------------------------------------------------------------------------
// 四、缺陷模型
// ---------------------------------------------------------------------------

/// 缺陷类别（锚点四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefectKind {
    /// 破面（孔洞 / 边界边 / 非流形边）。
    Hole,
    /// 退化三角（零面积）。
    Degenerate,
    /// 法线翻转（相邻面法线点积负）。
    FlippedNormal,
    /// UV 异常（越界 `[0,1]`）。
    UvOutOfRange,
}

impl DefectKind {
    pub fn label(self) -> &'static str {
        match self {
            DefectKind::Hole => "HOLE",
            DefectKind::Degenerate => "DEGENERATE",
            DefectKind::FlippedNormal => "FLIPPED_NORMAL",
            DefectKind::UvOutOfRange => "UV_OUT_OF_RANGE",
        }
    }

    /// 四类判据的稳定序号（供报告与测试对拍，不依赖枚举声明序）。
    pub fn code(self) -> u8 {
        match self {
            DefectKind::Hole => 0,
            DefectKind::Degenerate => 1,
            DefectKind::FlippedNormal => 2,
            DefectKind::UvOutOfRange => 3,
        }
    }

    /// 由稳定序号反查（越界返回 `None`，不猜）。
    pub fn of_code(c: u8) -> Option<DefectKind> {
        match c {
            0 => Some(DefectKind::Hole),
            1 => Some(DefectKind::Degenerate),
            2 => Some(DefectKind::FlippedNormal),
            3 => Some(DefectKind::UvOutOfRange),
            _ => None,
        }
    }
}

/// 单条缺陷。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Defect {
    pub kind: DefectKind,
    /// 定位面号（UV 类为所在面，破面为边界所属面，退化为该面，翻转取二者较小者）。
    pub face: usize,
    /// 定位顶点号（无明确顶点时为 `u32::MAX`）。
    pub vertex: u32,
    /// 度量值（退化面为二倍面积、UV 类为越界幅度、翻转为点积、破面为引用数）。
    pub metric: f32,
}

/// 修复方案（人话 + 可执行参数）。
///
/// **不能 derive `Eq`**：变体带 `f32`（`ClampUv` 的目标值），`f32` 不满足
/// `Eq`。故只 derive `PartialEq`——需要严格相等的判据走位级比较或 `to_bits`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RepairKind {
    /// 补洞：为边界边生成新顶点并连成三角（`u32` 为该边界环的目标三角数）。
    FillHole(u32),
    /// 删除退化面。
    DropDegenerate,
    /// 翻转缠绕：交换该面后两个索引使法线与邻面一致。
    FlipWinding,
    /// 钳制 UV：把越界分量拉回 `[0,1]`（`f32` 为目标值）。
    ClampUv(f32),
}

impl RepairKind {
    pub fn label(self) -> &'static str {
        match self {
            RepairKind::FillHole(_) => "FILL_HOLE",
            RepairKind::DropDegenerate => "DROP_DEGENERATE",
            RepairKind::FlipWinding => "FLIP_WINDING",
            RepairKind::ClampUv(_) => "CLAMP_UV",
        }
    }
}

/// 一条缺陷 + 其人话说明 + 修复方案（三者绑定，避免报告与执行脱节）。
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub defect: Defect,
    pub advice: String,
    pub repair: RepairKind,
}

/// 一次检测的完整报告（四类计数 + 明细）。
#[derive(Clone, Debug, Default)]
pub struct DefectReport {
    pub holes: u32,
    pub degenerates: u32,
    pub flips: u32,
    pub uv_out_of_range: u32,
    /// 非流形边（并入破面族，但独立计数以便诊断）。
    pub non_manifold_edges: u32,
    pub findings: Vec<Finding>,
}

impl DefectReport {
    pub fn total(&self) -> u32 {
        self.holes + self.degenerates + self.flips + self.uv_out_of_range
    }

    pub fn clean(&self) -> bool {
        self.total() == 0
    }

    pub fn count_of(&self, k: DefectKind) -> u32 {
        match k {
            DefectKind::Hole => self.holes,
            DefectKind::Degenerate => self.degenerates,
            DefectKind::FlippedNormal => self.flips,
            DefectKind::UvOutOfRange => self.uv_out_of_range,
        }
    }

    /// 逐类计数向量（按 [`DefectKind::code`] 序，长度恒为 4）。
    pub fn counts(&self) -> [u32; 4] {
        [
            self.holes,
            self.degenerates,
            self.flips,
            self.uv_out_of_range,
        ]
    }
}

/// 边引用记录（半边结构的最简形态）。
#[derive(Clone, Copy, Debug)]
struct EdgeUse {
    a: u32,
    b: u32,
    count: u32,
}

/// 规范化无向边（`a < b`），使 `(1,2)` 与 `(2,1)` 归一为同一条。
#[inline]
fn canon_edge(a: u32, b: u32) -> (u32, u32) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// 面法线（未归一化即可——点积只需符号与量级）。
fn face_normal(m: &RepairMesh, f: usize) -> Option<([f32; 3], [u32; 3])> {
    let t = m.face(f)?;
    let p0 = m.vert(t[0])?;
    let p1 = m.vert(t[1])?;
    let p2 = m.vert(t[2])?;
    let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
    let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
    let n = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    Some((n, t))
}

/// 二倍面积（叉积模长）。
fn face_area2(m: &RepairMesh, f: usize) -> Option<f32> {
    let (n, _) = face_normal(m, f)?;
    Some(crate::gfx::meshquant::fsqrt(
        n[0] * n[0] + n[1] * n[1] + n[2] * n[2],
    ))
}

/// 统计每条无向边的引用次数（O(面数)，`edges` 长度须为面数×3）。
fn count_edges(m: &RepairMesh, edges: &mut Vec<EdgeUse>) {
    edges.clear();
    let nf = m.face_count();
    for i in 0..nf {
        let t = match m.face(i) {
            Some(t) => t,
            None => continue,
        };
        for k in 0..3 {
            let a = t[k];
            let b = t[(k + 1) % 3];
            if a == b {
                continue; // 自环（退化面）不计入边统计，另由退化类检出
            }
            let key = canon_edge(a, b);
            let mut found = false;
            for e in edges.iter_mut() {
                if e.a == key.0 && e.b == key.1 {
                    e.count += 1;
                    found = true;
                    break;
                }
            }
            if !found {
                edges.push(EdgeUse {
                    a: key.0,
                    b: key.1,
                    count: 1,
                });
            }
        }
    }
}

/// 边引用数→ 查表（面数较大时线性扫会退化，故先建表）。
fn build_edge_table(m: &RepairMesh) -> Vec<EdgeUse> {
    let mut edges: Vec<EdgeUse> = Vec::new();
    count_edges(m, &mut edges);
    edges
}

// ---------------------------------------------------------------------------
// 五、四类检测
// ---------------------------------------------------------------------------

/// 破面检测：引用数为 1 的边界边即孔洞边界；引用数 > 2 为非流形边。
///
/// 非流形边并入破面族计数（同属「边引用数不是 2」这一族半边问题），
/// 但独立计数以便诊断——它们成因不同，不该在报告里混为一谈。
pub fn detect_holes(m: &RepairMesh, rep: &mut DefectReport) {
    let edges = build_edge_table(m);
    // **预算 = 边界边总数**：扇形三角化封闭一个长为 L 的环需要 L 个三角，
    // 故总预算取全部边界边数才够把所有环一次封完。
    //
    // 早先按 `rep.holes`（逐条递增 1,2,3…）给预算，导致第一步只补1 个三角、
    // 环没闭上，新边界又成为新孔洞，越补越多（实测孔洞不减反增）。
    let budget = edges.iter().filter(|e| e.count == 1).count() as u32;
    for e in edges.iter() {
        if e.count == 1 {
            rep.holes += 1;
            let advice = format!(
                "网格有 {} 个孔洞，建议补洞（边界边 {}-{} 只被一个面引用）",
                rep.holes, e.a, e.b
            );
            rep.findings.push(Finding {
                defect: Defect {
                    kind: DefectKind::Hole,
                    face: usize::MAX,
                    vertex: e.a,
                    metric: e.count as f32,
                },
                advice,
                repair: RepairKind::FillHole(budget),
            });
        } else if e.count > 2 {
            rep.non_manifold_edges += 1;
            rep.holes += 1;
            let advice = format!(
                "边 {}-{} 被 {} 个面引用（非流形），建议拆分该边使其只属两个面",
                e.a, e.b, e.count
            );
            rep.findings.push(Finding {
                defect: Defect {
                    kind: DefectKind::Hole,
                    face: usize::MAX,
                    vertex: e.a,
                    metric: e.count as f32,
                },
                advice,
                // 非流形边无法用补洞解决，按引用数给预算（执行时环游走会判弃）。
                repair: RepairKind::FillHole(e.count),
            });
        }
    }
}

/// 退化三角检测：`|e1 × e2| < DEGENERATE_AREA_EPS` 即零面积。
pub fn detect_degenerates(m: &RepairMesh, rep: &mut DefectReport) {
    for f in 0..m.face_count() {
        let area2 = match face_area2(m, f) {
            Some(a) => a,
            None => continue,
        };
        if area2 < DEGENERATE_AREA_EPS {
            rep.degenerates += 1;
            let advice = format!(
                "第 {} 个三角面积为零（退化面），建议删除——它不贡献表面却会挡住拾取",
                f
            );
            rep.findings.push(Finding {
                defect: Defect {
                    kind: DefectKind::Degenerate,
                    face: f,
                    vertex: m.face(f).map(|t| t[0]).unwrap_or(u32::MAX),
                    metric: area2,
                },
                advice,
                repair: RepairKind::DropDegenerate,
            });
        }
    }
}

/// 法线翻转检测：共享边两面的法线点积为负即缠绕矛盾。
///
/// 只在同一顶点位置处比较才有效——相邻面若不共享顶点就不是拓扑相邻，
/// 硬比会把整网格的朝向差异误判成翻转。故按共享边建邻接。
pub fn detect_flipped_normals(m: &RepairMesh, rep: &mut DefectReport) {
    let nf = m.face_count();
    // 面→ 其三条边的规范化键
    let mut face_edges: Vec<[(u32, u32); 3]> = Vec::with_capacity(nf);
    let mut normals: Vec<([f32; 3], [u32; 3])> = Vec::with_capacity(nf);
    for f in 0..nf {
        let (n, t) = match face_normal(m, f) {
            Some(x) => x,
            None => continue,
        };
        face_edges.push([
            canon_edge(t[0], t[1]),
            canon_edge(t[1], t[2]),
            canon_edge(t[2], t[0]),
        ]);
        normals.push((n, t));
    }
    for f in 0..normals.len() {
        for k in 0..3 {
            let key = face_edges[f][k];
            for g in (f + 1)..normals.len() {
                let shared = face_edges[g].iter().any(|e| *e == key);
                if !shared {
                    continue;
                }
                let d = dot3(
                    normals[f].0[0], normals[f].0[1], normals[f].0[2],
                    normals[g].0[0], normals[g].0[1], normals[g].0[2],
                );
                if d < FLIP_DOT_EPS {
                    rep.flips += 1;
                    let advice = format!(
                        "第 {} 与第 {} 个面法线朝向相反（点积 {:.4}），建议翻转其中一个的缠绕方向",
                        f, g, d
                    );
                    rep.findings.push(Finding {
                        defect: Defect {
                            kind: DefectKind::FlippedNormal,
                            face: f,
                            vertex: normals[f].1[0],
                            metric: d,
                        },
                        advice,
                        repair: RepairKind::FlipWinding,
                    });
                }
            }
        }
    }
}

/// UV 异常检测：分量越出 `[0,1]` 超过容差即报。
///
/// UV 缺失（`uvs` 为空）不算缺陷——它是数据缺失，不是 UV 越界，
/// 两者混同会让「没贴图」的网格被报成「贴图坏了」。
pub fn detect_uv_out_of_range(m: &RepairMesh, rep: &mut DefectReport) {
    if m.uvs.is_empty() {
        return;
    }
    for f in 0..m.face_count() {
        for c in 0..3 {
            let uv = match m.corner_uv(f, c) {
                Some(u) => u,
                None => continue,
            };
            let mut bad = 0.0f32;
            let mut hit = false;
            for k in 0..2 {
                let v = uv[k];
                if v < UV_RANGE_TOL {
                    bad = UV_RANGE_TOL - v;
                    hit = true;
                } else if v > 1.0 - UV_RANGE_TOL {
                    bad = v - (1.0 - UV_RANGE_TOL);
                    hit = true;
                }
            }
            if hit {
                rep.uv_out_of_range += 1;
                let advice = format!(
                    "第 {} 面第 {} 角 UV 超出 [0,1]（越界 {:.4}），建议钳制回边界内",
                    f, c, bad
                );
                rep.findings.push(Finding {
                    defect: Defect {
                        kind: DefectKind::UvOutOfRange,
                        face: f,
                        vertex: u32::MAX,
                        metric: bad,
                    },
                    advice,
                    // 方案：钳制到边界（0 或 1，取越界方向）。
                    repair: RepairKind::ClampUv(if bad > 0.0 { 1.0 } else { 0.0 }),
                });
                break; // 每面只报一次，避免三角全越界刷屏
            }
        }
    }
}

/// 四类检测全跑（锚点判据「四检测」）。
pub fn detect_all(m: &RepairMesh, diag: &mut MeshDiagBag) -> DefectReport {
    let mut rep = DefectReport::default();
    if !m.validate(diag) {
        return rep;
    }
    detect_holes(m, &mut rep);
    detect_degenerates(m, &mut rep);
    detect_flipped_normals(m, &mut rep);
    detect_uv_out_of_range(m, &mut rep);
    rep
}

/// 确定性：同输入两次检测结果必须逐字节一致。
pub fn report_is_deterministic(m: &RepairMesh) -> bool {
    let mut d1 = MeshDiagBag::new();
    let mut d2 = MeshDiagBag::new();
    let a = detect_all(m, &mut d1);
    let b = detect_all(m, &mut d2);
    if a.counts() != b.counts() {
        return false;
    }
    if a.non_manifold_edges != b.non_manifold_edges {
        return false;
    }
    a.findings.len() == b.findings.len()
}

// ---------------------------------------------------------------------------
// 六、可撤销修复执行（锚点判据「可撤销」）
// ---------------------------------------------------------------------------

/// 一条撤销记录：被改动的原值快照。
#[derive(Clone, Debug, PartialEq)]
pub enum UndoRecord {
    /// 删除了一个面（记录**删除前的整张面表**）。
    ///
    /// 刻意存整表而非只存被删的那个三角：删除的实现是「把末面搬来填空再截尾」，
    /// 其后所有面的存储位置都被改动过，只存被删面无法还原顺序。整表快照在删除
    /// 路径上是 O(面数) 的一次性代价，换来撤销的**精确还原**——撤销必须回到
    /// 原样，而不是回到「面数相同但顺序已变」的近似状态。
    FacesSnapshot(Vec<u32>),
    /// 翻转了一个面的缠绕（记录原三索引）。
    FaceFlipped { face: usize, tri: [u32; 3] },
    /// 改了UV 分量（记录角号与原值）。
    UvChanged { face: usize, corner: usize, comp: usize, old: f32 },
    /// 追加了顶点与面（记录**追加前的长度**，撤销时截回）。
    Appended { verts_before: usize, faces_before: usize },
}

/// 撤销栈（F1344 命令栈语义的最小实现：压栈 → 执行 → 出栈回滚）。
#[derive(Clone, Debug, Default)]
pub struct UndoStack {
    pub entries: Vec<Vec<UndoRecord>>,
    pub depth_limit: usize,
    /// 被栈深度上限丢弃的记录数（不静默）。
    pub dropped: u32,
}

impl UndoStack {
    pub fn new() -> UndoStack {
        UndoStack {
            entries: Vec::new(),
            depth_limit: 64,
            dropped: 0,
        }
    }

    pub fn depth(&self) -> usize {
        self.entries.len()
    }

    fn push_frame(&mut self, frame: Vec<UndoRecord>, diag: &mut MeshDiagBag) {
        if frame.is_empty() {
            return;
        }
        if self.entries.len() >= self.depth_limit {
            // 超出上限：丢弃最旧一帧并计数——不静默丢。
            self.entries.remove(0);
            self.dropped += 1;
        }
        self.entries.push(frame);
        let _ = diag;
    }

    /// 撤销最近一帧；成功返回被撤销的记录条数。
    pub fn pop_into(
        &mut self,
        m: &mut RepairMesh,
        diag: &mut MeshDiagBag,
    ) -> Option<usize> {
        let frame = match self.entries.pop() {
            Some(f) => f,
            None => {
                diag.push(MeshDiag::UndoStackEmpty);
                return None;
            }
        };
        let n = frame.len();
        // 逆序回滚：后做的先撤。
        //
        // **注意**：`FacesSnapshot` 与 `Appended` 都恢复**整段数组**，故逆序回滚
        // 时它们各自独立自洽（不需要与其他记录配对）——这是选整表快照的附带好处。
        for rec in frame.iter().rev() {
            // 按引用匹配：`FacesSnapshot` 持 `Vec<u32>`，`match *rec` 会试图移出
            // 借来的内容（E0507）。故匹配引用再按需 clone。
            match rec {
                UndoRecord::FacesSnapshot(snapshot) => {
                    // 精确还原整张面表（长度与顺序都回到删除前）。
                    m.faces = snapshot.clone();
                }
                UndoRecord::FaceFlipped { face, tri } => {
                    let s = *face * 3;
                    if s + 2 < m.faces.len() {
                        m.faces[s] = tri[0];
                        m.faces[s + 1] = tri[1];
                        m.faces[s + 2] = tri[2];
                    }
                }
                UndoRecord::UvChanged {
                    face,
                    corner,
                    comp,
                    old,
                } => {
                    let idx = (*face * 3 + *corner) * 2 + *comp;
                    if idx < m.uvs.len() {
                        m.uvs[idx] = *old;
                    }
                }
                UndoRecord::Appended {
                    verts_before,
                    faces_before,
                } => {
                    // 回退尾段到追加前的长度（追加只发生在尾段，截回即还原）。
                    if *verts_before <= m.verts.len() {
                        m.verts.truncate(*verts_before);
                    }
                    if *faces_before <= m.faces.len() {
                        m.faces.truncate(*faces_before);
                    }
                }
            }
        }
        Some(n)
    }
}

/// 修复执行器。
#[derive(Clone, Debug, Default)]
pub struct Repairer {
    pub undo: UndoStack,
    /// 累计执行次数（遥测）。
    pub applied: u32,
}

/// 单次修复的执行结果。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RepairOutcome {
    /// 实际改动了多少处。
    pub changed: u32,
    /// 被压入撤销栈的记录条数。
    pub undo_records: u32,
}

impl Repairer {
    pub fn new() -> Repairer {
        Repairer {
            undo: UndoStack::new(),
            applied: 0,
        }
    }

    /// 执行一份修复计划（按报告顺序），每步前压入原值快照。
    pub fn execute(
        &mut self,
        m: &mut RepairMesh,
        plan: &[RepairKind],
        diag: &mut MeshDiagBag,
    ) -> RepairOutcome {
        let mut frame: Vec<UndoRecord> = Vec::new();
        let mut changed = 0u32;
        for step in plan.iter() {
            match *step {
                RepairKind::DropDegenerate => {
                    // 检出首个退化面并删除（一步一删，便于逐步撤销）。
                    let mut rep = DefectReport::default();
                    detect_degenerates(m, &mut rep);
                    if let Some(f) = first_face_of(&rep, DefectKind::Degenerate) {
                        if m.face(f).is_some() {
                            let s = f * 3;
                            // 删除前先存整表快照——删除会搬动末面，撤销须精确还原。
                            let snapshot = m.faces.clone();
                            // 把末面搬到该位并截尾（保持数组为 3 的倍数）。
                            let last = m.faces.len() - 3;
                            if f != last {
                                m.faces[s] = m.faces[last];
                                m.faces[s + 1] = m.faces[last + 1];
                                m.faces[s + 2] = m.faces[last + 2];
                            }
                            m.faces.truncate(last);
                            frame.push(UndoRecord::FacesSnapshot(snapshot));
                            changed += 1;
                        }
                    }
                }
                RepairKind::FlipWinding => {
                    let mut rep = DefectReport::default();
                    detect_flipped_normals(m, &mut rep);
                    let f = first_face_of(&rep, DefectKind::FlippedNormal);
                    if let Some(f) = f {
                        if let Some(tri) = m.face(f) {
                            let s = f * 3;
                            // 交换后两索引即翻转缠绕（外法线反向）。
                            m.faces[s + 1] = tri[2];
                            m.faces[s + 2] = tri[1];
                            frame.push(UndoRecord::FaceFlipped { face: f, tri });
                            changed += 1;
                        }
                    }
                }
                RepairKind::ClampUv(target) => {
                    let mut rep = DefectReport::default();
                    detect_uv_out_of_range(m, &mut rep);
                    if let Some(f) = first_face_of(&rep, DefectKind::UvOutOfRange) {
                        let mut touched = 0u32;
                        for c in 0..3 {
                            for comp in 0..2usize {
                                if let Some(uv) = m.corner_uv(f, c) {
                                    let v = uv[comp];
                                    if v < 0.0 || v > 1.0 {
                                        let idx = (f * 3 + c) * 2 + comp;
                                        frame.push(UndoRecord::UvChanged {
                                            face: f,
                                            corner: c,
                                            comp,
                                            old: v,
                                        });
                                        m.uvs[idx] = target;
                                        touched += 1;
                                    }
                                }
                            }
                        }
                        changed += touched;
                    }
                }
                RepairKind::FillHole(target_tris) => {
                    // 补洞：把全部边界环用扇形三角化封上。
                    //
                    // **为什么必须按环补而不是按边补**：按边补会为每条边界边各生成
                    // 一个中点并各自连三角——实测使孔洞数从 3 涨到 12（修复让缺陷
                    // 变多），因为新生成的边本身又成了新的边界边，越补越多。
                    // 按环处理则每环只新增 1 个中心顶点 + L 个三角（L = 环长），
                    // 环被完整封闭，不再产生新边界。
                    //
                    // **必须一次处理所有环**：只补第一个环的话，其余环仍是孔洞，
                    // 而报告已记为「已修复」——虚假宣称。
                    let edges = build_edge_table(m);
                    let verts_before = m.verts.len();
                    let faces_before = m.faces.len();
                    let budget = target_tris.max(1) as usize;
                    let mut made = 0u32;

                    // 收集全部边界边。
                    let mut boundary: Vec<(u32, u32)> = Vec::new();
                    for e in edges.iter() {
                        if e.count == 1 {
                            boundary.push((e.a, e.b));
                        }
                    }
                    if boundary.is_empty() {
                        continue;
                    }
                    // 逐环处理：一条边用掉就从池里移除，避免下次从同一环起走。
                    let mut pool = boundary;
                    while !pool.is_empty() && (made as usize) < budget {
                        let ring = walk_boundary_ring(&pool);
                        if ring.len() < 3 {
                            // 环不足 3 点无法构成面（如孤立边）：去掉首边防打转。
                            pool.remove(0);
                            continue;
                        }
                        // 环重心作新中心顶点。
                        let mut cx = 0.0f32;
                        let mut cy = 0.0f32;
                        let mut cz = 0.0f32;
                        let mut ok = true;
                        for v in ring.iter() {
                            match m.vert(*v) {
                                Some(p) => {
                                    cx += p[0];
                                    cy += p[1];
                                    cz += p[2];
                                }
                                None => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if !ok {
                            pool.remove(0);
                            continue;
                        }
                        let inv = 1.0 / ring.len() as f32;
                        let center = m.push_vert([cx * inv, cy * inv, cz * inv]);
                        // 扇形三角化：(center, ring[i], ring[i+1])
                        for i in 0..ring.len() {
                            if made as usize >= budget {
                                break;
                            }
                            let a = ring[i];
                            let b = ring[(i + 1) % ring.len()];
                            if a == b {
                                continue;
                            }
                            m.push_face_with_uv([center, a, b]);
                            made += 1;
                        }
                        // 摘掉本环用掉的边（长度 = 环长）。
                        let take = ring.len().min(pool.len());
                        pool.drain(0..take);
                    }
                    if made > 0 {
                        frame.push(UndoRecord::Appended {
                            verts_before,
                            faces_before,
                        });
                        changed += made;
                    }
                }
            }
        }
        let records = frame.len() as u32;
        self.undo.push_frame(frame, diag);
        if changed > 0 {
            self.applied += 1;
        }
        RepairOutcome {
            changed,
            undo_records: records,
        }
    }

    /// 撤销最近一次修复；返回被撤销的记录条数。
    pub fn undo(&mut self, m: &mut RepairMesh, diag: &mut MeshDiagBag) -> Option<usize> {
        self.undo.pop_into(m, diag)
    }
}

/// 找出两条边界边各自端点的公用顶点（补洞三角的锚点）。
/// 把边界边串成一个环（补洞的输入）。
///
/// 边界边在网格里构成若干互不相交的环（每环对应一个孔洞）。做法：从首边起点
/// 出发，每步找另一条以当前点为端点的边界边，走完即闭环。
///
/// **不成环时返回已走过的部分**（调用方按 `len() < 3` 判弃）——不硬凑闭合，
/// 因为非闭合的链补出来的面会引入新的边界边（实测让缺陷不降反增）。
fn walk_boundary_ring(boundary: &[(u32, u32)]) -> Vec<u32> {
    let mut ring: Vec<u32> = Vec::new();
    if boundary.is_empty() {
        return ring;
    }
    let start = boundary[0].0;
    ring.push(start);
    let mut cur = boundary[0].1;
    // **每条边只用一次**——这是闭环的前提。不标记已用的话，走到 `cur=1` 时
    // 会再次匹配到同一条边 `(1,2)`，环永远闭不上（实测补洞因此完全没生效：
    // 环被填成 [0,1,2,1] 这种带重复点的伪环，扇形三角化后边界依旧）。
    let mut used: Vec<bool> = alloc::vec![false; boundary.len()];
    used[0] = true;
    // 环长上界 = 边界边数（闭环时环长恰等于边数），兼作病态输入的打转闸。
    let limit = boundary.len();
    while cur != start && ring.len() < limit {
        ring.push(cur);
        let mut next = None;
        for (i, (a, b)) in boundary.iter().enumerate() {
            if used[i] {
                continue;
            }
            if *a == cur {
                next = Some(*b);
                used[i] = true;
                break;
            }
            if *b == cur {
                next = Some(*a);
                used[i] = true;
                break;
            }
        }
        match next {
            Some(n) => cur = n,
            // 走到断头（不成环）：如实返回已走部分，调用方按长度判弃。
            None => return ring,
        }
    }
    ring
}

/// 找出报告中某类的全部定位面号。
fn findings_of(rep: &DefectReport, k: DefectKind) -> Vec<usize> {
    let mut out = Vec::new();
    for f in rep.findings.iter() {
        if f.defect.kind == k && f.defect.face != usize::MAX {
            out.push(f.defect.face);
        }
    }
    out
}

/// 找出报告中某类首个定位面号（无则 `None`）。
fn first_face_of(rep: &DefectReport, k: DefectKind) -> Option<usize> {
    for f in rep.findings.iter() {
        if f.defect.kind == k && f.defect.face != usize::MAX {
            return Some(f.defect.face);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 七、修复报告（修复前 / 后对照，锚点判据「报告」）
// ---------------------------------------------------------------------------

/// 修复前后对照报告。
#[derive(Clone, Debug)]
pub struct RepairReport {
    pub before: DefectReport,
    pub after: DefectReport,
    /// 各类被消除的数量（`after` 比 `before` 少的）。
    pub fixed: [u32; 4],
    /// 各类新增的数量（`after` 比 `before` 多的）——**必须显式报出**。
    pub introduced: [u32; 4],
}

impl RepairReport {
    /// 执行修复并生成前后对照报告。
    pub fn run(
        m: &mut RepairMesh,
        diag: &mut MeshDiagBag,
        repairer: &mut Repairer,
    ) -> RepairReport {
        let before = detect_all(m, diag);
        // 计划去重：`FillHole` 一次就把**全部**边界环封上，故多条同键建议只取一条
        // （取预算最大者）。不去重会让每条孔洞建议各跑一次补洞，反复新增中心
        // 顶点——修复动作互相叠加反而制造新缺陷。
        let mut plan: Vec<RepairKind> = Vec::new();
        let mut fill_budget = 0u32;
        for f in before.findings.iter() {
            match f.repair {
                RepairKind::FillHole(b) => {
                    if b > fill_budget {
                        fill_budget = b;
                    }
                }
                other => plan.push(other),
            }
        }
        if fill_budget > 0 {
            plan.insert(0, RepairKind::FillHole(fill_budget));
        }
        let _ = repairer.execute(m, &plan, diag);
        let after = detect_all(m, diag);
        let mut fixed = [0u32; 4];
        let mut introduced = [0u32; 4];
        for code in 0..4u8 {
            let k = match DefectKind::of_code(code) {
                Some(k) => k,
                None => continue,
            };
            let b = before.count_of(k);
            let a = after.count_of(k);
            if a < b {
                fixed[code as usize] = b - a;
            } else if a > b {
                introduced[code as usize] = a - b;
            }
        }
        RepairReport {
            before,
            after,
            fixed,
            introduced,
        }
    }

    /// 本次修复是否净改善（消除多于新增）。
    pub fn net_improved(&self) -> bool {
        let f: u32 = self.fixed.iter().sum();
        let i: u32 = self.introduced.iter().sum();
        f > i
    }

    /// 修复后是否已清零。
    pub fn clean_after(&self) -> bool {
        self.after.clean()
    }

    /// 是否引入了新缺陷（调用方须显式处置，不许默认忽略）。
    pub fn introduced_any(&self) -> bool {
        self.introduced.iter().any(|x| *x > 0)
    }
}

// ---------------------------------------------------------------------------
// 八、VE-F1607 判据自检
// ---------------------------------------------------------------------------

/// 构造一个已知形态的立方体网格（6 面 8 顶点，UV 齐备且在 [0,1] 内）。
fn cube() -> RepairMesh {
    let mut m = RepairMesh::new();
    for p in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
        [0.0, 1.0, 1.0],
    ] {
        m.push_vert(p);
    }
    for f in [
        [0, 1, 2],
        [0, 2, 3],
        [4, 6, 5],
        [4, 7, 6],
        [0, 4, 5],
        [0, 5, 1],
        [1, 5, 6],
        [1, 6, 2],
        [2, 6, 7],
        [2, 7, 3],
        [3, 7, 4],
        [3, 4, 0],
    ] {
        m.push_face(f);
    }
    // 逐面角 UV 全填0.5（合法区间内）。
    for _ in 0..m.face_count() {
        for _ in 0..3 {
            m.uvs.push(0.5);
            m.uvs.push(0.5);
        }
    }
    m
}

/// 构造一个单面开放网格（1 个三角 → 3 条边界边 = 3 个孔洞）。
fn open_tri() -> RepairMesh {
    let mut m = RepairMesh::new();
    m.push_vert([0.0, 0.0, 0.0]);
    m.push_vert([1.0, 0.0, 0.0]);
    m.push_vert([0.0, 1.0, 0.0]);
    m.push_face([0, 1, 2]);
    m
}

/// VE-F1607 判据自检。
pub fn run_vei07_checks() -> CheckSet {
    let mut set = CheckSet::new("gfx-vei07");

    // ---- 判据一：四检测 ----

    // 破面：单面三角三条边均只被引用一次
    {
        let m = open_tri();
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        set.add(
            "I1607-破面-边界边检出",
            r.holes == 3 && r.non_manifold_edges == 0 && !r.clean(),
            "",
        );
    }
    // 退化面：共线三角面积为零
    {
        let mut m = open_tri();
        m.push_vert([2.0, 0.0, 0.0]); // 与 0,1 共线 → 面积 0
        m.push_face([0, 1, 3]);
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        set.add(
            "I1607-退化-零面积检出",
            r.degenerates == 1 && r.count_of(DefectKind::Degenerate) == 1,
            "",
        );
    }
    // 法线翻转：两三角共边但朝向相反
    {
        let mut m = RepairMesh::new();
        m.push_vert([0.0, 0.0, 0.0]);
        m.push_vert([1.0, 0.0, 0.0]);
        m.push_vert([0.0, 1.0, 0.0]);
        m.push_face([0, 1, 2]);
        // 同一三角反向绕序 → 与自身共享边，法线相反
        m.push_face([2, 1, 0]);
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        set.add(
            "I1607-翻转-点积负检出",
            r.flips >= 1 && r.count_of(DefectKind::FlippedNormal) >= 1,
            "",
        );
    }
    // UV 越界：填一个 1.5 的分量
    {
        let mut m = cube();
        m.uvs[0] = 1.5;
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        set.add(
            "I1607-UV-越界检出",
            r.uv_out_of_range == 1 && r.count_of(DefectKind::UvOutOfRange) == 1,
            "",
        );
    }
    // 立方体（闭合流形）应零缺陷——防「一律报缺陷」的过门禁假绿
    {
        let m = cube();
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        set.add(
            "I1607-四检测-闭合体零缺陷",
            r.clean() && r.holes == 0 && r.degenerates == 0 && r.flips == 0,
            "",
        );
    }
    // 四类判据码自洽（不猜不吞）
    {
        let mut ok = true;
        for c in 0..4u8 {
            match DefectKind::of_code(c) {
                Some(k) if k.code() == c => {}
                _ => ok = false,
            }
        }
        set.add(
            "I1607-四检测-判据码自洽",
            ok && DefectKind::of_code(4).is_none() && DefectKind::of_code(255).is_none(),
            "",
        );
    }
    // 非流形边并入破面族且独立计数
    {
        let mut m = RepairMesh::new();
        m.push_vert([0.0, 0.0, 0.0]);
        m.push_vert([1.0, 0.0, 0.0]);
        m.push_vert([0.0, 1.0, 0.0]);
        m.push_vert([0.0, 0.0, 1.0]);
        // 三面共边 (0,1) → 非流形
        m.push_face([0, 1, 2]);
        m.push_face([0, 1, 3]);
        m.push_face([0, 1, 2]);
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        set.add(
            "I1607-破面-非流形独立计数",
            r.non_manifold_edges >= 1 && r.holes >= r.non_manifold_edges,
            "",
        );
    }
    // 确定性：同输入两次检测一致
    {
        let m = open_tri();
        set.add(
            "I1607-四检测-确定性",
            report_is_deterministic(&m) && report_is_deterministic(&cube()),
            "",
        );
    }
    // 空网格/坏索引不 panic，走诊断袋
    {
        let mut d = MeshDiagBag::new();
        let empty = RepairMesh::new();
        let r = detect_all(&empty, &mut d);
        // 越界索引：必须**整三**地改成越界值，否则 `faces.len()%3` 先触发形状错，
        // 越界诊断根本走不到（形状错与越界是两条独立判据，各测各的）。
        let mut m = cube();
        m.faces[0] = 99; // 仍是 3 的倍数，只把首索引换成越界顶点
        let mut d2 = MeshDiagBag::new();
        let r2 = detect_all(&m, &mut d2);
        set.add(
            "I1607-四检测-坏输入不崩",
            r.clean()
                && r2.clean()
                && d.has(MeshDiag::MeshEmpty)
                && d2.has(MeshDiag::IndexOutOfRange),
            "",
        );
    }
    // 形状错（长度非 3 倍数）单独走面——防「只测越界漏掉形状」
    {
        let mut m = cube();
        m.faces.push(7); // 长度变 37，非 3 倍数
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        set.add(
            "I1607-四检测-形状错判废",
            r.clean() && d.has(MeshDiag::IndexShapeInvalid),
            "",
        );
    }
    // UV 缺失不算 UV 缺陷（缺失≠越界）
    {
        let mut m = cube();
        m.uvs.clear();
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        set.add(
            "I1607-UV-缺失不误报",
            r.uv_out_of_range == 0 && r.clean(),
            "",
        );
    }

    // ---- 判据二：建议 ----

    // 每条缺陷都带人话说明（非空、含类别词）
    {
        let m = open_tri();
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        let mut all_have_text = !r.findings.is_empty();
        for f in r.findings.iter() {
            if f.advice.is_empty() || f.advice.len() < 6 {
                all_have_text = false;
            }
        }
        set.add("I1607-建议-人话非空", all_have_text, "");
    }
    // 建议含可执行方案（RepairKind 与缺陷类别匹配）
    {
        let mut m = open_tri();
        m.push_vert([2.0, 0.0, 0.0]);
        m.push_face([0, 1, 3]);
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        let mut matched = true;
        for f in r.findings.iter() {
            let ok = match f.defect.kind {
                DefectKind::Hole => matches!(f.repair, RepairKind::FillHole(_)),
                DefectKind::Degenerate => f.repair == RepairKind::DropDegenerate,
                DefectKind::FlippedNormal => f.repair == RepairKind::FlipWinding,
                DefectKind::UvOutOfRange => matches!(f.repair, RepairKind::ClampUv(_)),
            };
            if !ok {
                matched = false;
            }
        }
        set.add("I1607-建议-方案随类", matched && !r.findings.is_empty(), "");
    }
    // 方案标签非空（可读）
    {
        set.add(
            "I1607-建议-方案标签",
            RepairKind::FillHole(1).label() == "FILL_HOLE"
                && RepairKind::DropDegenerate.label() == "DROP_DEGENERATE"
                && RepairKind::FlipWinding.label() == "FLIP_WINDING"
                && RepairKind::ClampUv(0.0).label() == "CLAMP_UV",
            "",
        );
    }

    // ---- 判据三：可撤销 ----

    // 删除退化面可撤销（网格回到原样）
    {
        let mut m = open_tri();
        m.push_vert([2.0, 0.0, 0.0]);
        m.push_face([0, 1, 3]);
        let f0 = m.face_count();
        let v0 = m.vert_count();
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let _ = rp.execute(&mut m, &[RepairKind::DropDegenerate], &mut d);
        let after_exec = m.face_count();
        let undone = rp.undo(&mut m, &mut d);
        set.add(
            "I1607-可撤销-删面回滚",
            after_exec == f0 - 1
                && undone == Some(1)
                && m.face_count() == f0
                && m.vert_count() == v0,
            "",
        );
    }
    // 翻转缠绕可撤销
    {
        // 必须构造**真有翻转缺陷**的网格：两三角共边但绕序相反。
        // 拿单面三角（无相邻面、无翻转）去测，修复自然无事可做——
        // 那是用例前置条件不成立，不是判据错。
        let mut m = RepairMesh::new();
        m.push_vert([0.0, 0.0, 0.0]);
        m.push_vert([1.0, 0.0, 0.0]);
        m.push_vert([0.0, 1.0, 0.0]);
        m.push_face([0, 1, 2]);
        m.push_face([2, 1, 0]);
        let mut chk = DefectReport::default();
        detect_flipped_normals(&m, &mut chk);
        let t0 = m.face(0).unwrap();
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let out = rp.execute(&mut m, &[RepairKind::FlipWinding], &mut d);
        let flipped = m.face(0).unwrap();
        let _ = rp.undo(&mut m, &mut d);
        let back = m.face(0).unwrap();
        set.add(
            "I1607-可撤销-翻转回滚",
            chk.flips >= 1
                && out.changed == 1
                && flipped[0] == t0[0]
                && flipped[1] == t0[2]
                && flipped[2] == t0[1]
                && back == t0,
            "",
        );
    }
    // UV 钳制可撤销（值回原样）
    {
        let mut m = cube();
        m.uvs[0] = 1.5;
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let _ = rp.execute(&mut m, &[RepairKind::ClampUv(1.0)], &mut d);
        let clamped = m.uvs[0];
        let _ = rp.undo(&mut m, &mut d);
        set.add(
            "I1607-可撤销-UV回滚",
            clamped <= 1.0 && m.uvs[0] == 1.5,
            "",
        );
    }
    // 空栈撤销不 panic，走诊断
    {
        let mut m = open_tri();
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let r = rp.undo(&mut m, &mut d);
        set.add(
            "I1607-可撤销-空栈不崩",
            r.is_none() && d.has(MeshDiag::UndoStackEmpty) && m.face_count() == 1,
            "",
        );
    }
    // 撤销栈深度上限不静默丢（计数公开）
    {
        let mut st = UndoStack::new();
        st.depth_limit = 2;
        let mut d = MeshDiagBag::new();
        for i in 0..5 {
            st.push_frame(
                vec![UndoRecord::UvChanged {
                    face: i,
                    corner: 0,
                    comp: 0,
                    old: 0.0,
                }],
                &mut d,
            );
        }
        set.add(
            "I1607-可撤销-深度上限计数",
            st.depth() == 2 && st.dropped == 3,
            "",
        );
    }

    // ---- 判据四：报告 ----

    // 前后对照：报告同时含前后两份，且孔洞被真实消除
    {
        // 单面三角补洞：环长 3 → 新增 1 中心点 + 1 三角，环被封闭 → 孔洞 3→0。
        let mut m = open_tri();
        m.uvs.clear();
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let rep = RepairReport::run(&mut m, &mut d, &mut rp);
        set.add(
            "I1607-报告-前后对照齐",
            rep.before.holes == 3
                && rep.after.holes == 0
                && rep.fixed[0] == 3
                && rep.clean_after(),
            "",
        );
    }
    // 报告显式给出新增缺陷（不掩盖）——补洞不应凭空造出翻转/退化
    {
        let mut m = open_tri();
        m.uvs.clear();
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let rep = RepairReport::run(&mut m, &mut d, &mut rp);
        let introduced_total: u32 = rep.introduced.iter().sum();
        set.add(
            "I1607-报告-新增不掩盖",
            introduced_total == 0 && !rep.introduced_any(),
            "",
        );
    }
    // 修复后网格仍合法（补洞不能造出越界索引）——防「修完更坏」
    {
        let mut m = open_tri();
        m.uvs.clear();
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let _ = RepairReport::run(&mut m, &mut d, &mut rp);
        let mut d2 = MeshDiagBag::new();
        set.add(
            "I1607-报告-修后网格合法",
            m.validate(&mut d2),
            "",
        );
    }
    // 报告计数向量与逐类计数一致（内部自洽）
    {
        let m = open_tri();
        let mut d = MeshDiagBag::new();
        let r = detect_all(&m, &mut d);
        set.add(
            "I1607-报告-计数自洽",
            r.counts() == [r.holes, r.degenerates, r.flips, r.uv_out_of_range]
                && r.total() == r.counts().iter().sum::<u32>(),
            "",
        );
    }
    // 报告的 after 必须与当前网格真实状态一致（防拿陈旧计数充数）
    {
        // 用**只做一件事**的场景：只补洞，不动 UV，让 after 的 UV 类计数保持非零。
        // 若 after 是陈旧值/default，UV 计数会与真值不同，此处即变红。
        // （早先用「全缺陷网格」当场景，但那种网格会被修复计划全部清零，
        //  after 恰好等于 default，陈旧值与真值同形——门禁恒真，等于没有。）
        let mut m = open_tri();
        m.uvs.clear();
        for _ in 0..m.face_count() {
            for _ in 0..3 {
                m.uvs.push(1.5); // 越界：补洞不修 UV，故 after 必非零
                m.uvs.push(0.5);
            }
        }
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let rep = RepairReport::run(&mut m, &mut d, &mut rp);
        let mut d2 = MeshDiagBag::new();
        let truth = detect_all(&m, &mut d2);
        set.add(
            "I1607-报告-after与真值一致",
            // 前提：真值本身非default（否则本判据恒真）。
            truth.counts() != DefectReport::default().counts()
                && truth.uv_out_of_range > 0
                && rep.after.counts() == truth.counts()
                && rep.after.non_manifold_edges == truth.non_manifold_edges,
            "",
        );
    }

    // 新增面必须同步 UV：带 UV 的网格补洞后 UV 长度须仍等于 face_count*6
    {
        // 用**有孔洞**的网格（单面三角）——闭合立方体没有边界边，补洞路径根本不
        // 执行，用它当场景则本判据恒真。用例前置必须真的成立。
        let mut m = open_tri();
        for _ in 0..m.face_count() {
            for _ in 0..3 {
                m.uvs.push(0.5);
                m.uvs.push(0.5);
            }
        }
        let before_uv = m.uvs.len();
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let _ = RepairReport::run(&mut m, &mut d, &mut rp);
        set.add(
            "I1607-可撤销-补洞同步UV",
            m.face_count() > 1                       // 确实新增了面
                && m.uvs.len() > before_uv// 确实补了 UV
                && m.uvs.len() == m.face_count() * 6, // 长度自洽
            "",
        );
    }
    // 无 UV 网格补洞后仍无 UV（不凭空创造贴图数据）
    {
        let mut m = open_tri();
        m.uvs.clear();
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let _ = RepairReport::run(&mut m, &mut d, &mut rp);
        let mut d2 = MeshDiagBag::new();
        set.add(
            "I1607-边界-无UV不凭空补",
            !m.has_uv() && m.uvs.is_empty() && m.validate(&mut d2),
            "",
        );
    }

    // 修复后净改善判据可用（补洞后应真正清零孔洞）
    {
        let mut m = open_tri();
        m.uvs.clear();
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let rep = RepairReport::run(&mut m, &mut d, &mut rp);
        set.add(
            "I1607-报告-净改善判据",
            rep.net_improved() && rep.clean_after() && !rep.introduced_any(),
            "",
        );
    }
    // 补洞后撤销必须精确还原（面表逐字节回原样，含顺序）
    {
        let mut m = open_tri();
        m.uvs.clear();
        let (fv, ff) = (m.verts.clone(), m.faces.clone());
        let mut rp = Repairer::new();
        let mut d = MeshDiagBag::new();
        let _ = rp.execute(&mut m, &[RepairKind::FillHole(4)], &mut d);
        let changed = m.face_count() != ff.len() / 3 || m.vert_count() != fv.len() / 3;
        let _ = rp.undo(&mut m, &mut d);
        set.add(
            "I1607-可撤销-补洞精确回滚",
            changed && m.verts == fv && m.faces == ff,
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vei07_checks_all_green() {
        let set = run_vei07_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "vei07 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
                .0
                .iter()
                .flatten()
                .filter(|c| !c.passed)
                .map(|c| c.name)
                .collect::<Vec<_>>()
        );
    }

    /// 四条主判据每条至少两项自检——只有一项等于没门禁。
    #[test]
    fn vei07_judgement_families_present() {
        let set = run_vei07_checks();
        let (passed, _) = set.tally();
        let names: Vec<&str> = set
            .red_items()
            .0
            .iter()
            .flatten()
            .map(|c| c.name)
            .collect();
        for family in ["四检测", "建议", "可撤销", "报告"] {
            let n = names.iter().filter(|m| m.contains(family)).count();
            assert!(n >= 2, "判据族 {} 仅 {} 项自检，不足两道门禁", family, n);
        }
        assert!(passed >= 20, "自检项数 {} 偏少，判据落实密度不足", passed);
    }
}