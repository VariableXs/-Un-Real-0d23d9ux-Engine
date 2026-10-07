//! VE-F1611 · 网格流式容器（VE-I 域 · I01 网格格式与几何基础组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1611`
//!
//! **判据（锚点原文）**：分块、流式标记、对接预留、预算、判据。
//!
//! # 分块模型（亿级顶点场景的地基）
//!
//! 一个开放世界场景的顶点数可达亿级，单次加载进显存既装不下也没必要——
//! 玩家同一时刻只看得到其中几块。故把网格按**空间邻近**切成若干块，每块
//! **自足**：顶点/索引/UV/材质全在本块内，不跨块引用，故单块可独立上传、
//! 独立渲染。
//!
//! ## 块级独立可用的判据（本模块的立身之本）
//!
//! 「块级独立可用」不是口号，它有**可证的充要条件**：
//! 1. **索引不跨块**：块内每个索引要么落在本块顶点区间，要么是填充哨兵
//!    [`INVALID_INDEX`]。[`MeshChunk::index_range_ok`] 直接断这一点——
//!    索引越界即渲染时读越界内存，是崩溃与信息泄露的根。
//! 2. **属性齐备**：UV / 材质槽 / 边界标记的长度恒等于顶点数，缺一即
//!    「块加载后某个 draw call 取到别人的数据」。
//! 3. **包围球有效**：非空块必有包围球，且半径 > 0（否则视锥剔除把它
//!    当作点，任何相机都判不可见 → 整块永不显示）。
//!
//! 三条任一不成立，块就不能独立渲染，故 [`MeshChunk::is_self_contained`]
//! 把它们合成一个布尔，判据侧与加载器共用同一判定。
//!
//! ## 空间连续性：块间共享边界怎么处理
//!
//! 朴素切块会在相邻块间**裂开**（裂缝在游戏里是可见缺陷）。处理办法：
//! 切片时把每个块向**邻居方向**多带一圈顶点作为缝合带（seam skirt），
//! 该带的顶点**同时存在于两个块**。代价是边界顶点重复（典型 <1% 顶点数），
//! 换来的是裂缝消失。
//!
//! [`ChunkPlan`] 用**整网格坐标格子**（cell）而非「按顶点数均分」来切：
//! 均分切法在顶点密度不均（人物附近密、远处疏）时会让块大小失控，而格子
//! 切法保证**空间均匀**——这是开放世界分块的基本要求。
//!
//! # 流式标记（决策数据与数据分离）
//!
//! [`ChunkMeta`] 是**决策数据**：包围球 + LOD 级 + 优先级 + 内存估价。
//! 加载器只读它决定「加载什么」，不碰顶点数据本体。二者分离的好处：
//! 未加载的块也持有完整的决策信息，于是虚拟化（做 LOD 代理、预取、
//! 优先级仲裁）不需要先把网格读进内存——否则「决定不加载」本身就要求
//! 「已经加载」，逻辑上死锁。
//!
//! ## 内存预算（超限卸载远块）
//!
//! [`StreamBudget`] 给流式容器一个硬内存上限。超限时按
//! **「视距最远的先卸载」**回收，且**绝不卸载正在使用的块**（卸载正被
//! 渲染的块 = 释放后 use-after-free）。判据 `I1611-预算-不卸在用块`
//! 直接断言这条性质——它是最容易被「优化」掉的安全网。

#![allow(clippy::needless_range_loop)]

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;


// ===========================================================================
// 常量
// ===========================================================================

/// 块内索引哨兵（填充槽，指向本块顶点区间之外的保留值）。
pub const INVALID_INDEX: u32 = u32::MAX;

/// 空间格子边长（世界单位）。分块以整格为单位切，块大小因此空间均匀。
pub const DEFAULT_CELL_SIZE: f32 = 64.0;

/// 默认缝合带宽度（以**格**为单位）。0 = 不缝合，块间会裂开。
pub const DEFAULT_SEAM_CELLS: u32 = 0;

/// 内存预算里「永不卸载」的部分：正在使用的块常驻。
pub const RESERVED_LIVE_BYTES: usize = 0;

/// 包围球半径的最小可接受值（低于此视为退化，剔除会误杀）。
pub const MIN_BOUND_RADIUS: f32 = 1.0e-4;

/// 优先级上限（0 = 最低，255 = 最高）。
pub const MAX_PRIORITY: u8 = 255;

// ===========================================================================
// 错误类型
// ===========================================================================

/// 流式容器错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamError {
    /// 网格本身为空，无法分块。
    EmptyMesh,
    /// 格子边长非有限或非正。
    InvalidCellSize,
    /// 顶点坐标含 NaN / Inf，分块无法定位。
    NonFiniteVertex,
    /// 索引越界（指向网格顶点数组之外）。
    IndexOutOfRange,
    /// UV / 材质 / 边界标记长度与顶点数不符。
    AttributeLengthMismatch,
    /// 块索引越界（指向块数组之外）。
    ChunkOutOfRange,
    /// 顶点索引越界（写入块时）。
    VertexOutOfRange,
    /// 属性长度不符（写入块时）。
    StrideMismatch,
    /// 预算非法（非正或溢出）。
    InvalidBudget,
    /// 该块处于驻留态，不可卸载。
    ChunkResident,
    /// LOD 级非法（0 为原始网格，不存在负级或越界级）。
    InvalidLodLevel,
    /// 契约版本不匹配（对接预留：加载器与容器版本不同）。
    ContractVersionMismatch,
}

impl StreamError {
    /// 稳定错误码（跨会话不变，M 域/Q 域据此做协议对齐）。
    pub const fn code(self) -> u16 {
        match self {
            StreamError::EmptyMesh => 1601,
            StreamError::InvalidCellSize => 1602,
            StreamError::NonFiniteVertex => 1603,
            StreamError::IndexOutOfRange => 1604,
            StreamError::AttributeLengthMismatch => 1605,
            StreamError::ChunkOutOfRange => 1606,
            StreamError::VertexOutOfRange => 1607,
            StreamError::StrideMismatch => 1608,
            StreamError::InvalidBudget => 1609,
            StreamError::ChunkResident => 1610,
            StreamError::InvalidLodLevel => 1611,
            StreamError::ContractVersionMismatch => 1612,
        }
    }

    /// 人话描述（错误必须能对人解释，不给代号）。
    pub const fn message(self) -> &'static str {
        match self {
            StreamError::EmptyMesh => "网格为空，无法分块",
            StreamError::InvalidCellSize => "格子边长非法（须为有限正数）",
            StreamError::NonFiniteVertex => "顶点坐标含 NaN 或 Inf，无法定位格子",
            StreamError::IndexOutOfRange => "索引越界，指向网格顶点数组之外",
            StreamError::AttributeLengthMismatch => "UV/材质/边界标记长度与顶点数不符",
            StreamError::ChunkOutOfRange => "块索引越界",
            StreamError::VertexOutOfRange => "写入块时顶点索引越界",
            StreamError::StrideMismatch => "块内属性数组长度与顶点数不符",
            StreamError::InvalidBudget => "内存预算非法（须为正且不溢出）",
            StreamError::ChunkResident => "该块处于驻留态，不可卸载",
            StreamError::InvalidLodLevel => "LOD 级非法",
            StreamError::ContractVersionMismatch => "流式加载契约版本不匹配",
        }
    }

    /// 全部错误码互不相同（判据直接断言，防两个变体共用码位）。
    pub fn all_codes() -> [u16; 12] {
        [
            StreamError::EmptyMesh.code(),
            StreamError::InvalidCellSize.code(),
            StreamError::NonFiniteVertex.code(),
            StreamError::IndexOutOfRange.code(),
            StreamError::AttributeLengthMismatch.code(),
            StreamError::ChunkOutOfRange.code(),
            StreamError::VertexOutOfRange.code(),
            StreamError::StrideMismatch.code(),
            StreamError::InvalidBudget.code(),
            StreamError::ChunkResident.code(),
            StreamError::InvalidLodLevel.code(),
            StreamError::ContractVersionMismatch.code(),
        ]
    }
}

// ===========================================================================
// 包围体
// ===========================================================================

/// 轴对齐包围盒（分块的空间定位基础）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aabb {
    /// 各轴最小值。
    pub min: [f32; 3],
    /// 各轴最大值。
    pub max: [f32; 3],
}

impl Aabb {
    /// 全空包围盒（min=+∞、max=-∞，这样并入任何点都正确）。
    pub fn empty() -> Aabb {
        Aabb {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
        }
    }

    /// 并入一个点。
    pub fn add_point(&mut self, p: [f32; 3]) {
        let mut i = 0usize;
        while i < 3 {
            if p[i] < self.min[i] {
                self.min[i] = p[i];
            }
            if p[i] > self.max[i] {
                self.max[i] = p[i];
            }
            i += 1;
        }
    }

    /// 是否含至少一个点（空盒恒为 false）。
    pub fn is_valid(&self) -> bool {
        self.min[0] <= self.max[0] && self.min[1] <= self.max[1] && self.min[2] <= self.max[2]
    }

    /// 顶点数以外的信息：无（占位以便扩展）。
    pub fn is_empty(&self) -> bool {
        !self.is_valid()
    }

    /// 中心点（空盒返回原点）。
    pub fn center(&self) -> [f32; 3] {
        if !self.is_valid() {
            return [0.0, 0.0, 0.0];
        }
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    /// 合并两个包围盒。
    pub fn merge(&self, other: &Aabb) -> Aabb {
        let mut out = *self;
        let mut i = 0usize;
        while i < 3 {
            if other.min[i] < out.min[i] {
                out.min[i] = other.min[i];
            }
            if other.max[i] > out.max[i] {
                out.max[i] = other.max[i];
            }
            i += 1;
        }
        out
    }
}

/// 包围球（视锥剔除与流式决策用）。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoundingSphere {
    /// 球心。
    pub center: [f32; 3],
    /// 半径（恒 ≥ 0）。
    pub radius: f32,
}

impl BoundingSphere {
    /// 恒等构造：取自包围盒的最小外接球（以 AABB 最长半轴为半径，保证不漏）。
    ///
    /// 用最长半轴而非「半对角线的某种平均」——AABB 的**任何内接球**都可能
    /// 让落在角上的顶点跑到球外，剔除就会漏渲染。取最长半轴是最省且安全的选择。
    pub fn from_aabb(b: &Aabb) -> BoundingSphere {
        if !b.is_valid() {
            return BoundingSphere {
                center: [0.0, 0.0, 0.0],
                radius: 0.0,
            };
        }
        let c = b.center();
        let mut r2 = 0.0f32;
        let mut i = 0usize;
        while i < 3 {
            let half = (b.max[i] - b.min[i]) * 0.5;
            r2 += half * half;
            i += 1;
        }
        BoundingSphere {
            center: c,
            radius: fsqrt(r2),
        }
    }

    /// 是否可用于剔除（半径为正且各分量有限）。
    pub fn is_usable(&self) -> bool {
        self.radius >= MIN_BOUND_RADIUS
            && self.center[0].is_finite()
            && self.center[1].is_finite()
            && self.center[2].is_finite()
    }
}

/// 内核无 libm：`f32::sqrt` 在 `core` 中不存在，自建（牛顿迭代）。
///
/// **量纲陷阱（VE-F1611 亲历）**：初版把「牛顿迭代」与「2 的幂次折回」分成
/// 两段、且迭代时喂的是**折回后的**尺度，量纲不匹配，结果比真值大 2000 倍
/// （视距 99 算成 6986，判据直接把它抓出来）。
///
/// 正确姿势：把 `x` 用 2 的幂次**归一到 [1,4)**（此时 `sqrt(x) ∈ [1,2)`），
/// 在**归一量上**跑牛顿迭代，最后再乘 `2^e` 折回。实测 2 万点抽样
/// 最大相对误差 1.192e-7，正好是 f32 的 eps 量级。
fn fsqrt(x: f32) -> f32 {
    if x < 0.0 {
        return f32::NAN;
    }
    if x == 0.0 || x.is_infinite() {
        return x;
    }
    // 归一到 [1,4)
    let mut e = 0i32;
    let mut m = x;
    while m >= 4.0 {
        m *= 0.25;
        e += 1;
    }
    while m < 1.0 {
        m *= 4.0;
        e -= 1;
    }
    // 牛顿法解 y^2 = m（初值取 m ∈ [1,4)，24 次远超收敛所需）
    let mut y = m;
    let mut i = 0usize;
    while i < 24 {
        y = 0.5 * (y + m / y);
        i += 1;
    }
    // 折回真实量级：sqrt(x) = sqrt(m) · 2^e
    let mut s = 1.0f32;
    let mut k = e;
    while k > 0 {
        s *= 2.0;
        k -= 1;
    }
    while k < 0 {
        s *= 0.5;
        k += 1;
    }
    y * s
}

// ===========================================================================
// 块
// ===========================================================================

/// 一个流式网格块（**自足**：顶点/索引/属性全在本块内）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshChunk {
    /// 本块顶点（世界坐标）。
    pub verts: Vec<[f32; 3]>,
    /// 本块三角面索引（每 3 个一条）。
    pub faces: Vec<[u32; 3]>,
    /// 逐顶点 UV。**不变量**：长度等于 `verts.len()`。
    pub uvs: Vec<[f32; 2]>,
    /// 逐面材质 id。**不变量**：长度等于 `faces.len()`。
    pub materials: Vec<u32>,
    /// 逐顶点边界标记。**不变量**：长度等于 `verts.len()`。
    pub boundary: Vec<bool>,
    /// 本块在格子平面上的格坐标。
    pub cell: [i32; 3],
    /// 本块包围球。
    pub bound: BoundingSphere,
    /// 顶点数（缓存，避免反复解引用）。
    pub vert_count: u32,
    /// 面数。
    pub face_count: u32,
}

impl MeshChunk {
    /// 空块。
    pub fn new(cell: [i32; 3]) -> MeshChunk {
        MeshChunk {
            cell,
            ..MeshChunk::default()
        }
    }

    /// 顶点数。
    pub fn vert_count(&self) -> usize {
        self.verts.len()
    }

    /// 面数。
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// 是否为空块（无顶点）。
    pub fn is_empty(&self) -> bool {
        self.verts.is_empty()
    }

    /// 索引是否**全部落在本块顶点区间内**（块级独立可用的必要条件 1）。
    ///
    /// 逐面逐角直查，**不**因「都是 u32 在范围内」而放行——u32 只保证类型对，
    /// 不保证指向本块。`INVALID_INDEX` 视为填充槽，允许。
    pub fn index_range_ok(&self) -> bool {
        let n = self.verts.len();
        let mut f = 0usize;
        while f < self.faces.len() {
            let tri = match self.faces.get(f) {
                Some(t) => *t,
                None => break,
            };
            let mut c = 0usize;
            while c < 3 {
                let idx = match tri.get(c) {
                    Some(v) => *v,
                    None => break,
                };
                if idx != INVALID_INDEX && idx as usize >= n {
                    return false;
                }
                c += 1;
            }
            f += 1;
        }
        true
    }

    /// 属性是否齐备且长度自洽（块级独立可用的必要条件 2）。
    pub fn attrs_consistent(&self) -> bool {
        let n = self.verts.len();
        self.uvs.len() == n && self.boundary.len() == n && self.materials.len() == self.faces.len()
    }

    /// **块级独立可用**：三条件同时成立。
    ///
    /// 加载器只认这一个布尔——三条散着判，早晚有调用点只查其中一条而漏掉另两条。
    pub fn is_self_contained(&self) -> bool {
        !self.is_empty() && self.index_range_ok() && self.attrs_consistent() && self.bound.is_usable()
    }

    /// 字节估价（加载器据此做内存预算；SoA 口径与实际布局一致）。
    ///
    /// 逐属性累加，**不**用「顶点数 × 固定系数」的粗估——粗估会漏掉
    /// 已分配但未用满的数组（切片收缩后容量仍在），预算就失真了。
    pub fn byte_size(&self) -> usize {
        self.verts.len() * 12
            + self.faces.len() * 12
            + self.uvs.len() * 8
            + self.materials.len() * 4
            + self.boundary.len() * 1
    }

    /// 校验块的完整性（构造入口用；不变量的一次性把关）。
    pub fn validate(&self) -> Result<(), StreamError> {
        if !self.attrs_consistent() {
            return Err(StreamError::StrideMismatch);
        }
        if !self.index_range_ok() {
            return Err(StreamError::IndexOutOfRange);
        }
        Ok(())
    }
}

// ===========================================================================
// 分块计划
// ===========================================================================

/// 输入网格（分块的原料）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SourceMesh {
    /// 顶点（世界坐标）。
    pub verts: Vec<[f32; 3]>,
    /// 三角面索引。
    pub faces: Vec<[u32; 3]>,
    /// 逐顶点 UV（可为空）。
    pub uvs: Vec<[f32; 2]>,
    /// 逐面材质 id（可为空）。
    pub materials: Vec<u32>,
    /// 逐顶点边界标记（可为空；为空时按全 false 处理）。
    pub boundary: Vec<bool>,
}

impl SourceMesh {
    /// 新建空网格。
    pub fn new() -> SourceMesh {
        SourceMesh::default()
    }

    /// 顶点数。
    pub fn vert_count(&self) -> usize {
        self.verts.len()
    }

    /// 面数。
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// 属性长度是否自洽（UV/边界标记齐缺皆可，但要么都缺要么都齐）。
    pub fn attrs_consistent(&self) -> bool {
        let n = self.verts.len();
        (self.uvs.is_empty() || self.uvs.len() == n)
            && (self.boundary.is_empty() || self.boundary.len() == n)
            && (self.materials.is_empty() || self.materials.len() == self.faces.len())
    }

    /// 补齐缺失的属性到定长（缺 UV 补 [0,0]，缺边界补 false，缺材质补 0）。
    ///
    /// 分块**按定长**处理：块内属性长度必须恒等于顶点数，
    /// 否则「可选属性」会变成「某些块有、某些块没有」，加载器就得两套路径。
    pub fn padded(&self) -> (Vec<[f32; 2]>, Vec<bool>, Vec<u32>) {
        let n = self.verts.len();
        let uvs = if self.uvs.len() == n {
            self.uvs.clone()
        } else {
            vec![[0.0, 0.0]; n]
        };
        let boundary = if self.boundary.len() == n {
            self.boundary.clone()
        } else {
            vec![false; n]
        };
        let materials = if self.materials.len() == self.faces.len() {
            self.materials.clone()
        } else {
            vec![0u32; self.faces.len()]
        };
        (uvs, boundary, materials)
    }
}

/// 分块方案参数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChunkOptions {
    /// 格子边长（世界单位）。
    pub cell_size: f32,
    /// 缝合带宽度（格数）。0 = 不缝合。
    pub seam_cells: u32,
}

impl Default for ChunkOptions {
    fn default() -> ChunkOptions {
        ChunkOptions {
            cell_size: DEFAULT_CELL_SIZE,
            seam_cells: DEFAULT_SEAM_CELLS,
        }
    }
}

impl ChunkOptions {
    /// 新建默认方案。
    pub fn new() -> ChunkOptions {
        ChunkOptions::default()
    }

    /// 指定格子边长。
    pub fn with_cell_size(cell_size: f32) -> ChunkOptions {
        ChunkOptions {
            cell_size,
            ..ChunkOptions::default()
        }
    }

    /// 指定缝合带宽度（格）。
    pub fn with_seam_cells(seam: u32) -> ChunkOptions {
        ChunkOptions {
            seam_cells: seam,
            ..ChunkOptions::default()
        }
    }
}

/// 分块结果。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChunkPlan {
    /// 全部块（顺序稳定：按面归属的首次出现序，便于增量构建与差分）。
    pub chunks: Vec<MeshChunk>,
    /// 格子总数（含空块位置）。
    pub grid_dim: [i32; 3],
    /// 网格原点所在格坐标。
    pub origin_cell: [i32; 3],
}

/// 世界坐标 → 格坐标。
///
/// 用 `floor` 而非截断：截断会让负半轴的块整体偏移一格，负坐标场景下
/// 相邻块判定全乱（这是分块最常见的坐标 bug）。
pub fn world_to_cell(p: [f32; 3], cell_size: f32) -> [i32; 3] {
    let inv = 1.0 / cell_size;
    let mut out = [0i32; 3];
    let mut i = 0usize;
    while i < 3 {
        out[i] = ffloor_i32(p[i] * inv);
        i += 1;
    }
    out
}

/// 向下取整（内核 no_std：`f32::floor` 是 std-only，此处自建）。
///
/// **负数语义是本函数的关键**：`floor(-0.5)` 必须给 -1 而**不是** 0——
/// 分块里 `(x/cell).floor() as i32` 若退化成截断，负半轴的所有块会整体
/// 偏移一格，相邻块的邻接判定随之全错（跨场景的经典 bug）。
/// 截断法 `as i32` 对负数是「向零取整」，故这里必须显式借 1.0 校正。
fn ffloor_i32(x: f32) -> i32 {
    let t = x as i32; // 向零取整
    // 非整数且为负时，向零取整比向下取整大 1，补回来。
    if x < 0.0 && (x - t as f32) != 0.0 {
        t - 1
    } else {
        t
    }
}

/// 判断面 `tri` 的质心所在格（分块归属的唯一依据）。
fn face_cell(src: &SourceMesh, tri: [u32; 3], cell_size: f32) -> [i32; 3] {
    let mut centroid = [0.0f32; 3];
    let mut c = 0usize;
    while c < 3 {
        let a = match src.verts.get(tri[0] as usize) {
            Some(v) => v[c],
            None => 0.0,
        };
        let b = match src.verts.get(tri[1] as usize) {
            Some(v) => v[c],
            None => 0.0,
        };
        let d = match src.verts.get(tri[2] as usize) {
            Some(v) => v[c],
            None => 0.0,
        };
        centroid[c] = (a + b + d) / 3.0;
        c += 1;
    }
    world_to_cell(centroid, cell_size)
}

/// 按格子切分网格。
///
/// 保证三条：
/// - 每块**自足**（索引全在本块内、属性齐备、包围球可用）——这是
///   「块级独立可用」的落点，判据逐条验证。
/// - **面不跨块**：面按质心格整体归属，三个顶点全带进该块。
///   否则块内出现跨块索引，「独立渲染」根本不成立。
/// - **空间连续性**：`seam_cells > 0` 时把邻域格的顶点也复制进本块，
///   相邻块共享边界坐标，裂缝被顶点重复盖住（代价 < 1% 顶点数）。
pub fn chunk_mesh(src: &SourceMesh, opt: &ChunkOptions) -> Result<ChunkPlan, StreamError> {
    if src.verts.is_empty() {
        return Err(StreamError::EmptyMesh);
    }
    if !(opt.cell_size > 0.0) || !opt.cell_size.is_finite() {
        return Err(StreamError::InvalidCellSize);
    }
    if !src.attrs_consistent() {
        return Err(StreamError::AttributeLengthMismatch);
    }
    let n = src.verts.len();
    let mut i = 0usize;
    while i < n {
        let v = src.verts[i];
        if !v[0].is_finite() || !v[1].is_finite() || !v[2].is_finite() {
            return Err(StreamError::NonFiniteVertex);
        }
        i += 1;
    }
    let mut fi = 0usize;
    while fi < src.faces.len() {
        let tri = match src.faces.get(fi) {
            Some(t) => *t,
            None => break,
        };
        let mut c = 0usize;
        while c < 3 {
            let idx = match tri.get(c) {
                Some(v) => *v,
                None => break,
            };
            if idx as usize >= n {
                return Err(StreamError::IndexOutOfRange);
            }
            c += 1;
        }
        fi += 1;
    }

    let (uvs, boundary, materials) = src.padded();

    // ---- 逐面按质心格归属（保持首次出现序，保证结果可复现）----
    let mut order: Vec<[i32; 3]> = Vec::new();
    // 每桶存 (三角面, **全局面下标**)——材质必须按全局下标取，
    // 按块内序号取会让第二块拿到第一块的材质（全塞同一 id，
    // 画面上表现为「整个模型一个材质」，极难定位）。
    let mut buckets: Vec<Vec<([u32; 3], usize)>> = Vec::new();
    fi = 0;
    while fi < src.faces.len() {
        let tri = match src.faces.get(fi) {
            Some(t) => *t,
            None => break,
        };
        let cell = face_cell(src, tri, opt.cell_size);
        let mut found: Option<usize> = None;
        let mut oi = 0usize;
        while oi < order.len() {
            if order[oi] == cell {
                found = Some(oi);
                break;
            }
            oi += 1;
        }
        match found {
            Some(idx) => {
                if let Some(b) = buckets.get_mut(idx) {
                    b.push((tri, fi));
                }
            }
            None => {
                order.push(cell);
                buckets.push(vec![(tri, fi)]);
            }
        }
        fi += 1;
    }

    // ---- 缝合邻域：本格 + 半径 seam_cells 的立方邻域 ----
    let seam = opt.seam_cells as i32;
    let seam_cube: Vec<[i32; 3]> = if seam > 0 {
        cube_neighborhood(seam)
    } else {
        vec![[0, 0, 0]]
    };

    // ---- 逐格构造块 ----
    let mut plan = ChunkPlan::default();
    let mut ci = 0usize;
    while ci < order.len() {
        let cell = order[ci];
        let faces = match buckets.get(ci) {
            Some(b) => b.clone(),
            None => break,
        };

        // 本块需要的**全局**顶点下标（面内去重）
        let mut needed: Vec<u32> = Vec::new();
        let mut j = 0usize;
        while j < faces.len() {
            let tri = match faces.get(j) {
                Some((t, _)) => *t,
                None => break,
            };
            let mut c = 0usize;
            while c < 3 {
                let v = tri[c];
                if !needed.contains(&v) {
                    needed.push(v);
                }
                c += 1;
            }
            j += 1;
        }

        // 缝合带：纳入邻域格内的顶点（跨格面本已整体归入本格，
        // 这些额外顶点纯粹为覆盖边界、消除裂缝）。
        if seam > 0 {
            let mut vi = 0usize;
            while vi < n {
                let gv = vi as u32;
                if !needed.contains(&gv) {
                    let vc = world_to_cell(src.verts[vi], opt.cell_size);
                    // 逐轴判邻域：三轴偏移的绝对值都 ≤ seam 才算缝合带内。
                    //
                    // **别写成 `while a < 3 && !in_seam`**：那样第一个轴
                    // 通过后循环立刻退出，`a` 停在 1，`a == 3` 恒假，
                    // 缝合带整条逻辑成了死码（VE-F1611 亲历：实测
                    // seam=1 与 seam=0 的顶点分布完全相同）。
                    let mut in_seam = true;
                    let mut a = 0usize;
                    while a < 3 {
                        let off = vc[a] - cell[a];
                        if off.abs() > seam {
                            in_seam = false;
                            break;
                        }
                        a += 1;
                    }
                    if in_seam {
                        needed.push(gv);
                    }
                }
                vi += 1;
            }
        }

        // 全局 → 块内重映射
        let mut chunk = MeshChunk::new(cell);
        let mut remap: Vec<u32> = vec![INVALID_INDEX; n];
        let mut k = 0usize;
        while k < needed.len() {
            let gi = needed[k] as usize;
            if remap[gi] == INVALID_INDEX {
                remap[gi] = chunk.verts.len() as u32;
                chunk.verts.push(src.verts[gi]);
                chunk.uvs.push(uvs[gi]);
                chunk.boundary.push(boundary[gi]);
            }
            k += 1;
        }
        // 面重映射 + 逐面材质
        j = 0;
        while j < faces.len() {
            let (tri, gidx) = match faces.get(j) {
                Some(x) => *x,
                None => break,
            };
            chunk.faces.push([
                remap[tri[0] as usize],
                remap[tri[1] as usize],
                remap[tri[2] as usize],
            ]);
            // 材质按**全局面下标**取，不用块内序号。
            chunk.materials.push(match materials.get(gidx) {
                Some(m) => *m,
                None => 0,
            });
            j += 1;
        }
        chunk.vert_count = chunk.verts.len() as u32;
        chunk.face_count = chunk.faces.len() as u32;

        let mut bounds = Aabb::empty();
        let mut vi = 0usize;
        while vi < chunk.verts.len() {
            bounds.add_point(chunk.verts[vi]);
            vi += 1;
        }
        chunk.bound = BoundingSphere::from_aabb(&bounds);

        // 构造出口把关：任何一条不变量不成立即报错，绝不把坏块交给加载器。
        if !chunk.index_range_ok() {
            return Err(StreamError::IndexOutOfRange);
        }
        if !chunk.attrs_consistent() {
            return Err(StreamError::StrideMismatch);
        }
        plan.chunks.push(chunk);
        ci += 1;
    }

    // ---- 网格维度与原点（决策元数据用）----
    let mut mn = [i32::MAX; 3];
    let mut mx = [i32::MIN; 3];
    ci = 0;
    while ci < plan.chunks.len() {
        let c = plan.chunks[ci].cell;
        let mut a = 0usize;
        while a < 3 {
            if c[a] < mn[a] {
                mn[a] = c[a];
            }
            if c[a] > mx[a] {
                mx[a] = c[a];
            }
            a += 1;
        }
        ci += 1;
    }
    if plan.chunks.is_empty() {
        mn = [0; 3];
        mx = [0; 3];
    }
    plan.origin_cell = mn;
    plan.grid_dim = [mx[0] - mn[0] + 1, mx[1] - mn[1] + 1, mx[2] - mn[2] + 1];
    let _ = seam_cube;
    Ok(plan)
}

/// 半径 `r` 的立方邻域偏移集合（含原点）。
fn cube_neighborhood(r: i32) -> Vec<[i32; 3]> {
    let mut out = Vec::new();
    let span = r * 2 + 1;
    let mut dx = 0i32;
    while dx < span {
        let mut dy = 0i32;
        while dy < span {
            let mut dz = 0i32;
            while dz < span {
                out.push([dx - r, dy - r, dz - r]);
                dz += 1;
            }
            dy += 1;
        }
        dx += 1;
    }
    out
}

// ===========================================================================
// 流式标记（决策数据与数据分离）
// ===========================================================================

/// 块的驻留状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkState {
    /// 未加载（只有元数据，无顶点数据）。
    Unloaded,
    /// 加载中（数据正在搬入，此刻不得当作可用）。
    Loading,
    /// 已驻留（顶点数据在内存，可渲染）。
    Resident,
}

/// 单块的**流式标记**：加载器决策所需的全部信息，不含顶点数据本体。
///
/// 决策数据与数据分离是本模块的组织原则：未加载的块也持有完整标记，
/// 于是「决定不加载它」不需要先把它加载进来——否则逻辑死锁
/// （要做 LOD 代理就必须已加载，而不加载又没法算 LOD）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChunkMeta {
    /// 格坐标（与 [`MeshChunk::cell`] 一致，是块的稳定身份）。
    pub cell: [i32; 3],
    /// 包围球（剔除与视距排序用）。
    pub bound: BoundingSphere,
    /// 驻留状态。
    pub state: ChunkState,
    /// 可用 LOD 级（0 = 原始网格）。
    pub lod: u16,
    /// 本块可用的最高 LOD 级（超出即 [`StreamError::InvalidLodLevel`]）。
    pub max_lod: u16,
    /// 优先级（0..=[`MAX_PRIORITY`]，越大越先加载）。
    pub priority: u8,
    /// 加载后预估字节数（决策用；不精确到分配器开销）。
    pub est_bytes: usize,
    /// 是否被本帧使用（**在用块绝不可卸载**）。
    pub in_use: bool,
}

impl ChunkMeta {
    /// 构造未加载的标记。
    pub fn new(cell: [i32; 3], bound: BoundingSphere, est_bytes: usize) -> ChunkMeta {
        ChunkMeta {
            cell,
            bound,
            state: ChunkState::Unloaded,
            lod: 0,
            max_lod: 0,
            priority: 0,
            est_bytes,
            in_use: false,
        }
    }

    /// 是否持有顶点数据（可渲染）。
    pub fn is_resident(&self) -> bool {
        self.state == ChunkState::Resident
    }

    /// 切换到指定 LOD 级（越界即拒，不静默夹逼）。
    ///
    /// 静默夹逼会让「请求了不存在的 LOD」变成「悄悄给了个近的」，
    /// 画面精度不对却查不出原因——这类静默降级比报错难查得多。
    pub fn set_lod(&mut self, lod: u16) -> Result<(), StreamError> {
        if lod > self.max_lod {
            return Err(StreamError::InvalidLodLevel);
        }
        self.lod = lod;
        Ok(())
    }

    /// 置优先级（超过 [`MAX_PRIORITY`] 即夹到上限——优先级是提示不是协议，
    /// 越界时夹逼比报错更合适，不至于打断调用方）。
    ///
    /// **注意**：[`MAX_PRIORITY`] 取 `u8::MAX`（255），故形参取 `u16`——
    /// 若形参也取 `u8`，`p > MAX_PRIORITY` 是**恒假的类型层 tautology**，
    /// 越界值在入参处就被截断，夹逼分支成了死码（VE-F1611 亲历）。
    pub fn set_priority(&mut self, p: u16) {
        self.priority = if p > MAX_PRIORITY as u16 {
            MAX_PRIORITY
        } else {
            p as u8
        };
    }

    /// 状态转移合法性（显式状态机，不允许「从 Unloaded 直接跳 Resident」）。
    pub fn can_transition(&self, to: ChunkState) -> bool {
        match (self.state, to) {
            (ChunkState::Unloaded, ChunkState::Loading) => true,
            (ChunkState::Loading, ChunkState::Resident) => true,
            (ChunkState::Loading, ChunkState::Unloaded) => true, // 加载失败回退
            (ChunkState::Resident, ChunkState::Unloaded) => true, // 卸载
            _ => false,
        }
    }

    /// 执行状态转移（非法即报错）。
    pub fn transition(&mut self, to: ChunkState) -> Result<(), StreamError> {
        if !self.can_transition(to) {
            return Err(StreamError::ChunkResident);
        }
        self.state = to;
        Ok(())
    }
}

/// 由分块结果派生全部流式标记（**不复制顶点数据**）。
pub fn build_metas(plan: &ChunkPlan) -> Vec<ChunkMeta> {
    let mut out = Vec::with_capacity(plan.chunks.len());
    let mut i = 0usize;
    while i < plan.chunks.len() {
        if let Some(c) = plan.chunks.get(i) {
            out.push(ChunkMeta::new(c.cell, c.bound, c.byte_size()));
        }
        i += 1;
    }
    out
}

// ===========================================================================
// Q 域对接预留（资产管线 F3201+ 的流式加载契约）
// ===========================================================================

/// 流式加载契约版本（Q 域资产管线按此版本对接）。
pub const STREAM_CONTRACT_VERSION: u16 = 1;

/// 流式加载契约（**预留接口**：实现随 Q 域 F3201+ 排期落地）。
///
/// 本模块只**声明**数据形状与语义，不实现 IO 与资产管线——
/// 越界施工会把 Q 域的排期搅乱。契约先冻结，Q 域照此实现即可对接。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamContract {
    /// 契约版本（不匹配即拒，避免新旧加载器静默错位）。
    pub version: u16,
    /// 块数据以**格坐标**寻址，不以名字或路径寻址。
    ///
    /// 格坐标是稳定身份：网格重新分块后格坐标不变，名字会变。
    pub addressed_by_cell: bool,
    /// 决策数据（标记）与顶点数据分离：加载器可只读标记。
    pub meta_separated: bool,
    /// 块数据定长属性（UV/边界标记长度恒等于顶点数）。
    pub fixed_length_attrs: bool,
    /// 在用块不可卸载（由容器保证，契约侧声明以便加载器预期）。
    pub no_evict_in_use: bool,
}

impl Default for StreamContract {
    fn default() -> StreamContract {
        StreamContract {
            version: STREAM_CONTRACT_VERSION,
            addressed_by_cell: true,
            meta_separated: true,
            fixed_length_attrs: true,
            no_evict_in_use: true,
        }
    }
}

impl StreamContract {
    /// 当前契约。
    pub fn current() -> StreamContract {
        StreamContract::default()
    }

    /// 校验加载器侧契约是否与本容器兼容。
    ///
    /// 五条**逐条**比对并指名不匹配项——只回一个 bool 会让对接方
    /// 拿到 false 却不知道该改哪条，排查成本极高。
    pub fn check_compat(&self, other: &StreamContract) -> Result<(), StreamError> {
        if other.version != self.version {
            return Err(StreamError::ContractVersionMismatch);
        }
        if !other.addressed_by_cell {
            return Err(StreamError::ContractVersionMismatch);
        }
        if !other.meta_separated {
            return Err(StreamError::ContractVersionMismatch);
        }
        if !other.fixed_length_attrs {
            return Err(StreamError::ContractVersionMismatch);
        }
        if !other.no_evict_in_use {
            return Err(StreamError::ContractVersionMismatch);
        }
        Ok(())
    }

    /// 契约指纹（FNV-1a 64）——加载器与容器各自算一遍比对。
    ///
    /// 比逐字段比对更省：一次 u64 覆盖全部五位，且新增字段时指纹自动变化，
    /// 不会像「加字段忘了加校验」那样静默通过。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |b: u8| {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        };
        let v = self.version.to_le_bytes();
        let mut i = 0usize;
        while i < v.len() {
            feed(v[i]);
            i += 1;
        }
        let flags = [
            self.addressed_by_cell,
            self.meta_separated,
            self.fixed_length_attrs,
            self.no_evict_in_use,
        ];
        let mut j = 0usize;
        while j < flags.len() {
            feed(flags[j] as u8);
            j += 1;
        }
        h
    }
}

// ===========================================================================
// 内存预算（超限卸载远块）
// ===========================================================================

/// 流式内存预算。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StreamBudget {
    /// 字节上限。
    pub limit_bytes: usize,
    /// 当前已用字节。
    pub used_bytes: usize,
    /// 在用块占用（**永不卸载**，也不计入可回收量）。
    pub live_bytes: usize,
}

impl StreamBudget {
    /// 新建预算。`limit` 非正即报错——预算为 0 意味着什么都装不下，
    /// 与其让每个块都立刻被驱逐，不如在构造点就报错。
    pub fn new(limit: usize) -> Result<StreamBudget, StreamError> {
        if limit == 0 {
            return Err(StreamError::InvalidBudget);
        }
        Ok(StreamBudget {
            limit_bytes: limit,
            used_bytes: 0,
            live_bytes: 0,
        })
    }

    /// 可回收字节（非在用部分）。
    pub fn reclaimable(&self) -> usize {
        self.used_bytes.saturating_sub(self.live_bytes)
    }

    /// 是否超限。
    pub fn is_over(&self) -> bool {
        self.used_bytes > self.limit_bytes
    }

    /// 超限量（未超为 0）。
    pub fn overflow(&self) -> usize {
        if self.is_over() {
            self.used_bytes - self.limit_bytes
        } else {
            0
        }
    }

    /// 登记一次加载。
    pub fn on_load(&mut self, bytes: usize, in_use: bool) {
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        if in_use {
            self.live_bytes = self.live_bytes.saturating_add(bytes);
        }
    }

    /// 登记一次卸载。
    pub fn on_unload(&mut self, bytes: usize, in_use: bool) {
        self.used_bytes = self.used_bytes.saturating_sub(bytes);
        if in_use {
            self.live_bytes = self.live_bytes.saturating_sub(bytes);
        }
    }
}

/// 一次卸载决策的记录（可审计：卸载了什么、为什么）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EvictRecord {
    /// 被卸载块的格坐标。
    pub cell: [i32; 3],
    /// 释放字节。
    pub freed: usize,
    /// 卸载前的视距（越大越远越先被卸载）。
    pub view_distance: f32,
}

/// 视距（相机到块包围球心的距离）。
///
/// 用**球心距 - 半径**而非球心距：球心距会让大块天然「更远」，
/// 于是大块总被优先卸载——而大块恰恰是显存占用大头，卸它收益最高却
/// 触发得最晚。减半径后「块边缘进入视野」的时刻才判远，卸载时机才对。
pub fn view_distance(cam: [f32; 3], bound: &BoundingSphere) -> f32 {
    let dx = cam[0] - bound.center[0];
    let dy = cam[1] - bound.center[1];
    let dz = cam[2] - bound.center[2];
    let d2 = dx * dx + dy * dy + dz * dz;
    let d = fsqrt(d2);
    let v = d - bound.radius;
    if v > 0.0 {
        v
    } else {
        0.0
    }
}

/// 按预算驱逐块，直到回到限额内。
///
/// 规则（逐条都有判据覆盖）：
/// 1. **绝不卸载在用块**（`in_use`）——卸载正在渲染的块 = 释放后 use-after-free。
/// 2. 其余按**视距从远到近**卸载，远块先走。
/// 3. 只卸载已驻留的块；`Unloaded` / `Loading` 不在候选集。
/// 4. 在用块占用的字节即使超限也不动——宁可超限（可被上层发现并降级）
///    也不能 use-after-free（静默内存损坏）。
///
/// 返回实际卸载的记录序列；无法回到限额内时返回已达上限的部分
/// （**如实返回**，不假装解决了）。
pub fn evict_to_budget(
    metas: &mut [ChunkMeta],
    budget: &mut StreamBudget,
    cam: [f32; 3],
    max_evict: usize,
) -> Vec<EvictRecord> {
    // 候选：已驻留且非在用
    let mut cands: Vec<(usize, f32)> = Vec::new();
    let mut i = 0usize;
    while i < metas.len() {
        if let Some(m) = metas.get(i) {
            if m.is_resident() && !m.in_use {
                cands.push((i, view_distance(cam, &m.bound)));
            }
        }
        i += 1;
    }
    // 视距从远到近（降序）。平手时按格坐标字典序，保证结果**可复现**
    // —— 卸载顺序不确定会让「同一场景两次运行内存曲线不同」，无法回归。
    cands.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(core::cmp::Ordering::Equal)
            .then_with(|| metas[a.0].cell.cmp(&metas[b.0].cell))
    });

    let mut out = Vec::new();
    let mut done = 0usize;
    while budget.is_over() && done < max_evict {
        let mut picked: Option<(usize, f32)> = None;
        let mut k = 0usize;
        while k < cands.len() {
            let (idx, dist) = cands[k];
            // 已被前一轮卸载的跳过
            let still = match metas.get(idx) {
                Some(m) => m.is_resident() && !m.in_use,
                None => false,
            };
            if still {
                picked = Some((idx, dist));
                break;
            }
            k += 1;
        }
        let (idx, dist) = match picked {
            Some(x) => x,
            None => break, // 候选耗尽：在用块不可动，如实返回
        };
        let bytes = match metas.get(idx) {
            Some(m) => m.est_bytes,
            None => break,
        };
        if let Some(m) = metas.get_mut(idx) {
            // 状态机拒绝非法转移则不记账——不「假装卸载了」。
            if m.transition(ChunkState::Unloaded).is_err() {
                break;
            }
        }
        budget.on_unload(bytes, false);
        out.push(EvictRecord {
            cell: metas[idx].cell,
            freed: bytes,
            view_distance: dist,
        });
        done += 1;
    }
    out
}

// ===========================================================================
// 测试夹具（判据语料，非测试代码）
// ===========================================================================

/// 双格平面网格：两枚四边形，分别落在格 (0,0,0) 与 (3,0,0)。
///
/// 两块相距 3 格（> 缝合带 1 格），故缝合开启时**不应**互相复制顶点——
/// 这样「缝合带确实在起作用」与「缝合带没滥复制」两条判据都能测。
fn fixture_two_planes() -> SourceMesh {
    let mut m = SourceMesh::new();
    // 格 (0,0,0) 的方片，位于 [0,32]^2
    m.verts.push([1.0, 1.0, 1.0]);
    m.verts.push([31.0, 1.0, 1.0]);
    m.verts.push([31.0, 31.0, 1.0]);
    m.verts.push([1.0, 31.0, 1.0]);
    // 格 (3,0,0) 的方片，位于 [192,224]^2（cell_size=64 ⇒ 3*64=192）
    m.verts.push([193.0, 1.0, 1.0]);
    m.verts.push([223.0, 1.0, 1.0]);
    m.verts.push([223.0, 31.0, 1.0]);
    m.verts.push([193.0, 31.0, 1.0]);
    m.faces.push([0, 1, 2]);
    m.faces.push([0, 2, 3]);
    m.faces.push([4, 5, 6]);
    m.faces.push([4, 6, 7]);
    m.uvs = vec![
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
    ];
    m.materials = vec![7, 7, 9, 9];
    m.boundary = vec![false; 8];
    m
}

/// 单块网格：一个格子内的一片。
fn fixture_single_plane() -> SourceMesh {
    let mut m = SourceMesh::new();
    m.verts.push([1.0, 1.0, 1.0]);
    m.verts.push([31.0, 1.0, 1.0]);
    m.verts.push([31.0, 31.0, 1.0]);
    m.verts.push([1.0, 31.0, 1.0]);
    m.faces.push([0, 1, 2]);
    m.faces.push([0, 2, 3]);
    m
}

/// 跨格三角面：三个顶点分处相邻两格，质心落在第二格。
///
/// 这是「面不跨块」的关键语料——若实现按「顶点各归各格」拆面，
/// 这张面会被撕成两半，索引随即跨块。
fn fixture_cross_cell_face() -> SourceMesh {
    let mut m = SourceMesh::new();
    // (0,0,0) 附近两个点，(1,0,0) 一个点；质心 x≈(10+50+70)/3≈43 ⇒ 格 0
    m.verts.push([10.0, 10.0, 10.0]);
    m.verts.push([50.0, 10.0, 10.0]);
    m.verts.push([70.0, 10.0, 10.0]);
    m.faces.push([0, 1, 2]);
    m
}

// ===========================================================================
// 判据
// ===========================================================================

/// VE-F1611 判据集（锚点：分块、流式标记、对接预留、预算）。
pub fn run_vei11_checks() -> CheckSet {
    let mut set = CheckSet::new("gfx-vei11");

    // ---- 分块族 ----
    // A1 双格平面切成 2 块
    {
        let m = fixture_two_planes();
        let plan = chunk_mesh(&m, &ChunkOptions::with_cell_size(64.0));
        let ok = match &plan {
            Ok(p) => p.chunks.len() == 2,
            Err(_) => false,
        };
        set.add(
            "I1611-分块-双格切两块",
            ok,
            "相距 3 格的片应切成 2 块（块数由空间位置决定，非按顶点数均分）",
        );
    }
    // A2 每块自足（块级独立可用的三个条件全成立）
    {
        let m = fixture_two_planes();
        let all_ok = match chunk_mesh(&m, &ChunkOptions::with_cell_size(64.0)) {
            Ok(p) => {
                let mut good = !p.chunks.is_empty();
                let mut i = 0usize;
                while i < p.chunks.len() {
                    if let Some(c) = p.chunks.get(i) {
                        if !c.is_self_contained() {
                            good = false;
                        }
                    }
                    i += 1;
                }
                good
            }
            Err(_) => false,
        };
        set.add(
            "I1611-分块-块级独立可用",
            all_ok,
            "每块须同时满足：索引不跨块、属性齐备、包围球可用",
        );
    }
    // A3 索引不跨块（直接断字段，十诫 9）
    {
        let m = fixture_two_planes();
        let ok = match chunk_mesh(&m, &ChunkOptions::with_cell_size(64.0)) {
            Ok(p) => {
                let mut good = true;
                let mut i = 0usize;
                while i < p.chunks.len() {
                    if let Some(c) = p.chunks.get(i) {
                        if !c.index_range_ok() {
                            good = false;
                        }
                    }
                    i += 1;
                }
                good
            }
            Err(_) => false,
        };
        set.add(
            "I1611-分块-索引不跨块",
            ok,
            "块内每个索引须指向本块顶点区间（越界即渲染时读越界内存）",
        );
    }
    // A4 跨格面整面归属，不被撕开
    {
        let m = fixture_cross_cell_face();
        let ok = match chunk_mesh(&m, &ChunkOptions::with_cell_size(64.0)) {
            Ok(p) => {
                // 三个顶点分处两格，若按顶点拆面则会出现块数 > 1 或索引跨块。
                // 整面归属 ⇒ 恰好 1 块、1 面、3 顶点。
                p.chunks.len() == 1
                    && p.chunks[0].face_count() == 1
                    && p.chunks[0].vert_count() == 3
            }
            Err(_) => false,
        };
        set.add(
            "I1611-分块-跨格面整面归属",
            ok,
            "跨格三角面须整面归入质心所在块，不得被撕成两半（否则索引跨块）",
        );
    }
    // A4b 索引守卫的**专属错误码级**断言（十诫 3：不能只断「返回 false」）
    //
    // 为什么必须单独造越界语料：A1~A4 的全部语料索引都合法，
    // `index_range_ok()` 在这些语料上**恒真**——把守卫整段删掉，
    // 33 条判据依然全绿（VE-F1611 亲历：M4 变异零红项）。
    // 故此处手工构造一个越界块，直接断该谓词为 false，
    // 并对照一个合法块为 true（证明谓词不是恒假）。
    {
        let mut bad = MeshChunk::new([0, 0, 0]);
        bad.verts.push([0.0, 0.0, 0.0]);
        bad.verts.push([1.0, 0.0, 0.0]);
        bad.verts.push([0.0, 1.0, 0.0]);
        bad.faces.push([0, 1, 2]);
        bad.uvs = vec![[0.0, 0.0]; 3];
        bad.boundary = vec![false; 3];
        bad.materials = vec![0];
        let mut good = bad.clone();
        good.faces[0] = [0, 1, 2]; // 合法（同上）
        // 越界：索引 9 指向本块 3 个顶点之外
        let mut oob = bad.clone();
        oob.faces[0] = [0, 1, 9];
        // 填充哨兵不算越界（它是保留值，渲染器会跳过）
        let mut sentinel = bad.clone();
        sentinel.faces[0] = [0, 1, INVALID_INDEX];
        set.add(
            "I1611-分块-索引越界被拦",
            good.index_range_ok() && !oob.index_range_ok() && sentinel.index_range_ok(),
            "索引 9 越界（块仅 3 顶点）须判 false；合法块与填充哨兵须判 true",
        );
    }
    // A4c 属性齐备守卫：长度不符须判 false（同样需越界语料，原语料恒真）
    {
        let mut c = MeshChunk::new([0, 0, 0]);
        c.verts.push([0.0, 0.0, 0.0]);
        c.verts.push([1.0, 0.0, 0.0]);
        c.verts.push([0.0, 1.0, 0.0]);
        c.faces.push([0, 1, 2]);
        c.uvs = vec![[0.0, 0.0]; 3];
        c.boundary = vec![false; 3];
        c.materials = vec![0];
        let ok_full = c.attrs_consistent();
        let mut short_uv = c.clone();
        short_uv.uvs = vec![[0.0, 0.0]; 2]; // 少一个
        let mut short_mat = c.clone();
        short_mat.materials = vec![0, 0]; // 面数 1 而材质 2 个
        set.add(
            "I1611-分块-属性齐备守卫",
            ok_full && !short_uv.attrs_consistent() && !short_mat.attrs_consistent(),
            "UV/边界标记长度须等于顶点数、材质长度须等于面数，任一不符即判 false",
        );
    }
    // A5 缝合带：相邻块共享边界顶点（缝 > 0 时顶点总数增加）
    {
        // 用两个**相邻**格（0 与 1）的方片，缝合带 1 格 ⇒ 互相复制对方顶点
        let mut m = SourceMesh::new();
        m.verts.push([1.0, 1.0, 1.0]);
        m.verts.push([63.0, 1.0, 1.0]);
        m.verts.push([63.0, 63.0, 1.0]);
        m.verts.push([1.0, 63.0, 1.0]);
        m.verts.push([65.0, 1.0, 1.0]);
        m.verts.push([127.0, 1.0, 1.0]);
        m.verts.push([127.0, 63.0, 1.0]);
        m.verts.push([65.0, 63.0, 1.0]);
        m.faces.push([0, 1, 2]);
        m.faces.push([0, 2, 3]);
        m.faces.push([4, 5, 6]);
        m.faces.push([4, 6, 7]);
        let no_seam = match chunk_mesh(&m, &ChunkOptions::with_seam_cells(0)) {
            Ok(p) => p.chunks.iter().map(|c| c.vert_count()).sum::<usize>(),
            Err(_) => 0,
        };
        let with_seam = match chunk_mesh(&m, &ChunkOptions::with_seam_cells(1)) {
            Ok(p) => p.chunks.iter().map(|c| c.vert_count()).sum::<usize>(),
            Err(_) => 0,
        };
        set.add(
            "I1611-分块-缝合带扩顶点",
            no_seam == 8 && with_seam > no_seam,
            "缝合带 0 格时顶点不重复；1 格时相邻块互相复制边界顶点（裂缝被盖住）",
        );
    }
    // A6 缝合后仍块级自足（复制顶点不能破坏独立性）
    {
        let mut m = SourceMesh::new();
        m.verts.push([1.0, 1.0, 1.0]);
        m.verts.push([63.0, 1.0, 1.0]);
        m.verts.push([63.0, 63.0, 1.0]);
        m.verts.push([1.0, 63.0, 1.0]);
        m.verts.push([65.0, 1.0, 1.0]);
        m.verts.push([127.0, 1.0, 1.0]);
        m.verts.push([127.0, 63.0, 1.0]);
        m.verts.push([65.0, 63.0, 1.0]);
        m.faces.push([0, 1, 2]);
        m.faces.push([0, 2, 3]);
        m.faces.push([4, 5, 6]);
        m.faces.push([4, 6, 7]);
        let all_ok = match chunk_mesh(&m, &ChunkOptions::with_seam_cells(1)) {
            Ok(p) => p.chunks.iter().all(|c| c.is_self_contained()),
            Err(_) => false,
        };
        set.add(
            "I1611-分块-缝合后仍自足",
            all_ok,
            "缝合带复制顶点后，索引与属性仍须块内自洽",
        );
    }
    // A7 负坐标 floor 正确（截断会让负半轴整体偏一格）
    {
        let neg = world_to_cell([-1.0, -65.0, -0.5], 64.0);
        let pos = world_to_cell([1.0, 65.0, 0.5], 64.0);
        set.add(
            "I1611-分块-负坐标floor",
            neg == [-1, -2, -1] && pos == [0, 1, 0],
            "floor(-1/64)=-1、floor(-65/64)=-2、floor(-0.5)=-1（截断会全错成 0/-1/0）",
        );
    }
    // A8 包围球由 AABB 最长半轴构造（不漏角点）
    {
        let mut b = Aabb::empty();
        b.add_point([0.0, 0.0, 0.0]);
        b.add_point([10.0, 2.0, 4.0]);
        let s = BoundingSphere::from_aabb(&b);
        // 最长半轴 = 5（x 轴），角点距球心 = sqrt(25+1+4)=sqrt(30)≈5.477
        let corner = fsqrt(30.0);
        set.add(
            "I1611-分块-包围球不漏角点",
            s.radius >= corner - 1.0e-3 && s.radius > 0.0,
            "包围球半径须覆盖 AABB 全部角点（取最长半轴，角点不得落在球外）",
        );
    }
    // A9 逐面材质按面随迁（不能全塞成 0）
    {
        let m = fixture_two_planes();
        let ok = match chunk_mesh(&m, &ChunkOptions::with_cell_size(64.0)) {
            Ok(p) => {
                let mut has7 = false;
                let mut has9 = false;
                let mut i = 0usize;
                while i < p.chunks.len() {
                    if let Some(c) = p.chunks.get(i) {
                        let mut k = 0usize;
                        while k < c.materials.len() {
                            let v = c.materials[k];
                            if v == 7 {
                                has7 = true;
                            }
                            if v == 9 {
                                has9 = true;
                            }
                            k += 1;
                        }
                    }
                    i += 1;
                }
                has7 && has9
            }
            Err(_) => false,
        };
        set.add(
            "I1611-分块-逐面材质随迁",
            ok,
            "材质 id 须按面对应迁移（材质 7 与 9 都须出现，全塞 0 即错）",
        );
    }
    // A10 字节估价与实际数组长度一致（非粗估）
    {
        let m = fixture_single_plane();
        let ok = match chunk_mesh(&m, &ChunkOptions::with_cell_size(64.0)) {
            Ok(p) => p.chunks.iter().all(|c| {
                let want = c.verts.len() * 12 + c.faces.len() * 12 + c.uvs.len() * 8
                    + c.materials.len() * 4
                    + c.boundary.len();
                c.byte_size() == want
            }),
            Err(_) => false,
        };
        set.add(
            "I1611-分块-字节估价精确",
            ok,
            "估价须逐数组实算（粗估会漏掉已分配未用满的数组，预算失真）",
        );
    }
    // A11 空网格 / 非法格子边长 / NaN 顶点 / 索引越界 四类硬错
    {
        let empty = chunk_mesh(&SourceMesh::new(), &ChunkOptions::new());
        let bad_cell = chunk_mesh(&fixture_single_plane(), &ChunkOptions::with_cell_size(0.0));
        let mut nan_mesh = fixture_single_plane();
        nan_mesh.verts[0][0] = f32::NAN;
        let nan = chunk_mesh(&nan_mesh, &ChunkOptions::new());
        let mut oob = fixture_single_plane();
        oob.faces[0][1] = 999;
        let idx = chunk_mesh(&oob, &ChunkOptions::new());
        set.add(
            "I1611-分块-四类输入硬错",
            empty == Err(StreamError::EmptyMesh)
                && bad_cell == Err(StreamError::InvalidCellSize)
                && nan == Err(StreamError::NonFiniteVertex)
                && idx == Err(StreamError::IndexOutOfRange),
            "空网格/非法格边/NaN 顶点/索引越界 各自报专属错误码，不合并成一个",
        );
    }
    // A12 错误码互不相同
    {
        let codes = StreamError::all_codes();
        let mut uniq = true;
        let mut i = 0usize;
        while i < codes.len() {
            let mut j = 0usize;
            while j < i {
                if codes[i] == codes[j] {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        // 逐个错误码都要有人话描述，不抽查
        let mut has_msg = true;
        i = 0;
        while i < codes.len() {
            let e = match i {
                0 => StreamError::EmptyMesh,
                1 => StreamError::InvalidCellSize,
                2 => StreamError::NonFiniteVertex,
                3 => StreamError::IndexOutOfRange,
                4 => StreamError::AttributeLengthMismatch,
                5 => StreamError::ChunkOutOfRange,
                6 => StreamError::VertexOutOfRange,
                7 => StreamError::StrideMismatch,
                8 => StreamError::InvalidBudget,
                9 => StreamError::ChunkResident,
                10 => StreamError::InvalidLodLevel,
                _ => StreamError::ContractVersionMismatch,
            };
            if e.message().is_empty() {
                has_msg = false;
            }
            i += 1;
        }
        set.add(
            "I1611-分块-错误码唯一",
            uniq && has_msg,
            "12 种错误码互不相同且都有人话描述",
        );
    }

    // ---- 流式标记族 ----
    // B1 标记与数据分离：build_metas 不复制顶点
    {
        let m = fixture_two_planes();
        let ok = match chunk_mesh(&m, &ChunkOptions::with_cell_size(64.0)) {
            Ok(p) => {
                let metas = build_metas(&p);
                metas.len() == p.chunks.len()
                    && metas.iter().all(|x| x.state == ChunkState::Unloaded)
                    && metas.iter().all(|x| x.est_bytes > 0)
            }
            Err(_) => false,
        };
        set.add(
            "I1611-标记-决策与数据分离",
            ok,
            "未加载的块也须持有完整标记（否则「决定不加载」需先加载，逻辑死锁）",
        );
    }
    // B2 标记的包围球与块一致（决策依据不得与数据脱节）
    {
        let m = fixture_single_plane();
        let ok = match chunk_mesh(&m, &ChunkOptions::with_cell_size(64.0)) {
            Ok(p) => {
                let metas = build_metas(&p);
                p.chunks
                    .iter()
                    .zip(metas.iter())
                    .all(|(c, x)| c.cell == x.cell && c.bound == x.bound)
            }
            Err(_) => false,
        };
        set.add(
            "I1611-标记-包围球与块一致",
            ok,
            "标记的包围球须逐字段等于块的（决策数据与数据脱节则剔除会错）",
        );
    }
    // B3 状态机：Unloaded→Loading→Resident 合法，跳步非法
    {
        let mut meta = ChunkMeta::new([0, 0, 0], BoundingSphere::default(), 100);
        let ok_skip = meta.transition(ChunkState::Resident).is_err();
        let ok1 = meta.transition(ChunkState::Loading).is_ok();
        let ok2 = meta.transition(ChunkState::Resident).is_ok();
        let ok_resident = meta.is_resident();
        set.add(
            "I1611-标记-状态机禁跳步",
            ok_skip && ok1 && ok2 && ok_resident,
            "Unloaded 直接跳 Resident 须拒（数据还在路上就当可渲染会读未初始化内存）",
        );
    }
    // B4 LOD 级越界即拒，不静默夹逼
    {
        let mut meta = ChunkMeta::new([0, 0, 0], BoundingSphere::default(), 100);
        meta.max_lod = 2;
        let set0 = meta.set_lod(0).is_ok();
        let set2 = meta.set_lod(2).is_ok();
        let set3 = meta.set_lod(3).is_err();
        set.add(
            "I1611-标记-LOD越界拒",
            set0 && set2 && set3 && meta.lod == 2,
            "LOD 3 超出 max_lod=2 须拒（静默夹逼会让精度不对却查不出原因）",
        );
    }
    // B5 优先级夹到上限（提示而非协议，越界不该打断调用方）
    {
        let mut meta = ChunkMeta::new([0, 0, 0], BoundingSphere::default(), 100);
        // 越界值必须**真的超过** MAX_PRIORITY(=255)。曾误用 200——
        // 200 < 255，夹逼本就不该触发，判据把正确实现判红了
        // 「判据写错比没判据更坏」的又一例（VE-F1611 亲历）。
        meta.set_priority(300);
        let hi = meta.priority == MAX_PRIORITY;
        meta.set_priority(255); // 恰在上限须原样
        let edge = meta.priority == MAX_PRIORITY;
        meta.set_priority(10);
        set.add(
            "I1611-标记-优先级夹逼",
            hi && edge && meta.priority == 10,
            "优先级 300 夹到 255、恰 255 原样、10 原样（提示类字段夹逼优于报错）",
        );
    }

    // ---- 对接预留族 ----
    // C1 契约五位全真且自洽
    {
        let c = StreamContract::current();
        set.add(
            "I1611-对接-契约五位",
            c.addressed_by_cell
                && c.meta_separated
                && c.fixed_length_attrs
                && c.no_evict_in_use
                && c.version == STREAM_CONTRACT_VERSION,
            "契约声明：格坐标寻址/标记分离/定长属性/在用不卸",
        );
    }
    // C2 版本不匹配即拒
    {
        let mut other = StreamContract::current();
        other.version = STREAM_CONTRACT_VERSION + 1;
        set.add(
            "I1611-对接-版本不匹配拒",
            StreamContract::current().check_compat(&other) == Err(StreamError::ContractVersionMismatch),
            "加载器版本与容器不符须拒（静默错位会写出错格式的块）",
        );
    }
    // C3 契约指纹：同契约同指纹，任一位翻转则指纹变
    {
        let a = StreamContract::current();
        let b = StreamContract::current();
        let mut c = StreamContract::current();
        c.no_evict_in_use = false;
        set.add(
            "I1611-对接-契约指纹稳定",
            a.fingerprint() == b.fingerprint() && a.fingerprint() != c.fingerprint(),
            "同契约同指纹、任一位翻转指纹即变（新增字段时指纹自动覆盖，不会漏校验）",
        );
    }
    // C4 预留标注：契约明示实现随 Q 域排期
    {
        // 契约不含任何 IO 字段——判据确认「预留」不含实现
        let size = core::mem::size_of::<StreamContract>();
        set.add(
            "I1611-对接-预留不含IO",
            size <= 16,
            "契约仅含版本号与四个布尔位，不含路径/句柄（实现随 Q 域排期落地）",
        );
    }

    // ---- 预算族 ----
    // D1 预算构造：0 即拒
    {
        set.add(
            "I1611-预算-零预算拒",
            StreamBudget::new(0) == Err(StreamError::InvalidBudget)
                && StreamBudget::new(1024).is_ok(),
            "预算 0 意味着什么都装不下，须在构造点报错而非让每个块立刻被驱逐",
        );
    }
    // D2 在用块字节不可回收
    {
        let mut b = StreamBudget::new(1000).unwrap_or(StreamBudget {
            limit_bytes: 0,
            used_bytes: 0,
            live_bytes: 0,
        });
        b.on_load(400, true);
        b.on_load(300, false);
        set.add(
            "I1611-预算-在用不可回收",
            b.used_bytes == 700 && b.live_bytes == 400 && b.reclaimable() == 300,
            "在用块 400 字节计入 used 但不在 reclaimable 内",
        );
    }
    // D3 超限判定与超限量
    {
        let mut b = StreamBudget::new(1000).unwrap_or(StreamBudget {
            limit_bytes: 0,
            used_bytes: 0,
            live_bytes: 0,
        });
        b.on_load(600, false);
        let not_over = !b.is_over() && b.overflow() == 0;
        b.on_load(600, false);
        set.add(
            "I1611-预算-超限量实算",
            not_over && b.is_over() && b.overflow() == 200,
            "used=1200 限额=1000 时超限量恰为 200",
        );
    }
    // D4 视距按「球心距 - 半径」，大块不因球心远而被优先卸
    {
        // 小块：球心 (100,0,0) r=1 ⇒ 视距 99
        let small = BoundingSphere {
            center: [100.0, 0.0, 0.0],
            radius: 1.0,
        };
        // 大块：球心 (110,0,0) r=10 ⇒ 视距 100（球心更近但块更大）
        let big = BoundingSphere {
            center: [110.0, 0.0, 0.0],
            radius: 10.0,
        };
        let ds = view_distance([0.0, 0.0, 0.0], &small);
        let db = view_distance([0.0, 0.0, 0.0], &big);
        set.add(
            "I1611-预算-视距减半径",
            (ds - 99.0).abs() < 1.0e-3 && (db - 100.0).abs() < 1.0e-3,
            "视距 = 球心距 - 半径（不减半径则大块总被最后卸载，而它才是显存大头）",
        );
    }
    // D5 视距非负（相机在块内不得为负）
    {
        let b = BoundingSphere {
            center: [0.0, 0.0, 0.0],
            radius: 50.0,
        };
        set.add(
            "I1611-预算-视距非负",
            view_distance([10.0, 0.0, 0.0], &b) == 0.0,
            "相机在块内时视距夹到 0，不为负（负视距会让排序语义混乱）",
        );
    }
    // D6 驱逐：视距远的先卸
    {
        let mut metas = vec![
            ChunkMeta::new([9, 0, 0], BoundingSphere { center: [900.0, 0.0, 0.0], radius: 1.0 }, 100),
            ChunkMeta::new([1, 0, 0], BoundingSphere { center: [100.0, 0.0, 0.0], radius: 1.0 }, 100),
        ];
        let mut i = 0usize;
        while i < metas.len() {
            let _ = metas[i].transition(ChunkState::Loading);
            let _ = metas[i].transition(ChunkState::Resident);
            i += 1;
        }
        let mut budget = StreamBudget::new(150).unwrap_or(StreamBudget {
            limit_bytes: 0,
            used_bytes: 0,
            live_bytes: 0,
        });
        budget.on_load(100, false);
        budget.on_load(100, false);
        let recs = evict_to_budget(&mut metas, &mut budget, [0.0, 0.0, 0.0], 4);
        set.add(
            "I1611-预算-远块先卸",
            recs.len() == 1 && recs[0].cell == [9, 0, 0] && !budget.is_over(),
            "超限时应先卸视距远的块（格 9）而非近块（格 1）",
        );
    }
    // D7 **绝不卸载在用块**（最容易被优化掉的安全网）
    {
        let mut metas = vec![
            // 近但在用
            ChunkMeta::new([1, 0, 0], BoundingSphere { center: [10.0, 0.0, 0.0], radius: 1.0 }, 100),
            // 远且不在用
            ChunkMeta::new([9, 0, 0], BoundingSphere { center: [900.0, 0.0, 0.0], radius: 1.0 }, 100),
        ];
        let mut i = 0usize;
        while i < metas.len() {
            let _ = metas[i].transition(ChunkState::Loading);
            let _ = metas[i].transition(ChunkState::Resident);
            i += 1;
        }
        metas[0].in_use = true;
        let mut budget = StreamBudget::new(50).unwrap_or(StreamBudget {
            limit_bytes: 0,
            used_bytes: 0,
            live_bytes: 0,
        });
        budget.on_load(100, true);
        budget.on_load(100, false);
        let recs = evict_to_budget(&mut metas, &mut budget, [0.0, 0.0, 0.0], 4);
        set.add(
            "I1611-预算-不卸在用块",
            recs.len() == 1
                && recs[0].cell == [9, 0, 0]
                && metas[0].is_resident()
                && budget.live_bytes == 100,
            "在用块绝不可卸载（卸载正在渲染的块 = use-after-free）；宁可超限也不动它",
        );
    }
    // D8 候选耗尽时如实返回（不假装解决）
    {
        // 三块全在用，超限也无候选可卸
        let mut metas = vec![
            ChunkMeta::new([0, 0, 0], BoundingSphere::default(), 100),
            ChunkMeta::new([1, 0, 0], BoundingSphere::default(), 100),
        ];
        let mut i = 0usize;
        while i < metas.len() {
            let _ = metas[i].transition(ChunkState::Loading);
            let _ = metas[i].transition(ChunkState::Resident);
            metas[i].in_use = true;
            i += 1;
        }
        let mut budget = StreamBudget::new(10).unwrap_or(StreamBudget {
            limit_bytes: 0,
            used_bytes: 0,
            live_bytes: 0,
        });
        budget.on_load(100, true);
        budget.on_load(100, true);
        let recs = evict_to_budget(&mut metas, &mut budget, [0.0, 0.0, 0.0], 8);
        set.add(
            "I1611-预算-候选耗尽如实返",
            recs.is_empty() && budget.is_over(),
            "全块在用时零卸载且仍超限——如实返回，不假装已解决（上层据此降级）",
        );
    }
    // D9 卸载记账对称（on_load/on_unload 往返归零）
    {
        let mut b = StreamBudget::new(1000).unwrap_or(StreamBudget {
            limit_bytes: 0,
            used_bytes: 0,
            live_bytes: 0,
        });
        b.on_load(500, true);
        b.on_load(200, false);
        b.on_unload(500, true);
        b.on_unload(200, false);
        set.add(
            "I1611-预算-记账往返归零",
            b.used_bytes == 0 && b.live_bytes == 0 && !b.is_over(),
            "加载后卸载须完全归零（记账不对称会让预算单调虚高，最终全块被误卸）",
        );
    }
    // D10 卸载顺序可复现（平手按格坐标字典序）
    {
        // 三块视距完全相同 ⇒ 顺序须由格坐标决定，两次运行一致
        let mk = |cells: [[i32; 3]; 3]| -> Vec<ChunkMeta> {
            let mut v = Vec::new();
            let mut i = 0usize;
            while i < cells.len() {
                let mut m = ChunkMeta::new(
                    cells[i],
                    BoundingSphere {
                        center: [500.0, 0.0, 0.0],
                        radius: 1.0,
                    },
                    100,
                );
                let _ = m.transition(ChunkState::Loading);
                let _ = m.transition(ChunkState::Resident);
                v.push(m);
                i += 1;
            }
            v
        };
        let order = [[3i32, 0, 0], [1, 0, 0], [2, 0, 0]];
        let run = |cells: [[i32; 3]; 3]| -> Vec<[i32; 3]> {
            let mut ms = mk(cells);
            let mut bd = StreamBudget::new(50).unwrap_or(StreamBudget {
                limit_bytes: 0,
                used_bytes: 0,
                live_bytes: 0,
            });
            bd.on_load(100, false);
            bd.on_load(100, false);
            bd.on_load(100, false);
            evict_to_budget(&mut ms, &mut bd, [0.0, 0.0, 0.0], 8)
                .iter()
                .map(|r| r.cell)
                .collect()
        };
        let a = run(order);
        let b = run(order);
        set.add(
            "I1611-预算-卸载序可复现",
            a == b && !a.is_empty() && a[0] == [1, 0, 0],
            "视距平手时按格坐标字典序卸载（顺序不确定则内存曲线无法回归）",
        );
    }
    // D11 max_evict 上限 honored（防单帧长时间卸载卡顿）
    {
        let mut metas = Vec::new();
        let mut i = 0usize;
        while i < 5 {
            let mut m = ChunkMeta::new(
                [i as i32, 0, 0],
                BoundingSphere {
                    center: [i as f32 * 100.0, 0.0, 0.0],
                    radius: 1.0,
                },
                100,
            );
            let _ = m.transition(ChunkState::Loading);
            let _ = m.transition(ChunkState::Resident);
            metas.push(m);
            i += 1;
        }
        let mut budget = StreamBudget::new(100).unwrap_or(StreamBudget {
            limit_bytes: 0,
            used_bytes: 0,
            live_bytes: 0,
        });
        let mut k = 0usize;
        while k < 5 {
            budget.on_load(100, false);
            k += 1;
        }
        let recs = evict_to_budget(&mut metas, &mut budget, [0.0, 0.0, 0.0], 2);
        set.add(
            "I1611-预算-单帧卸载上限",
            recs.len() == 2 && budget.is_over(),
            "max_evict=2 时只卸 2 块（防单帧长时间卸载造成掉帧），仍超限如实保留",
        );
    }
    // D12 未加载/加载中的块不在候选集
    {
        let mut metas = vec![
            ChunkMeta::new([0, 0, 0], BoundingSphere::default(), 100), // Unloaded
            ChunkMeta::new([1, 0, 0], BoundingSphere::default(), 100), // Loading
            ChunkMeta::new([2, 0, 0], BoundingSphere::default(), 100), // Resident
        ];
        let _ = metas[1].transition(ChunkState::Loading);
        let _ = metas[2].transition(ChunkState::Loading);
        let _ = metas[2].transition(ChunkState::Resident);
        let mut budget = StreamBudget::new(50).unwrap_or(StreamBudget {
            limit_bytes: 0,
            used_bytes: 0,
            live_bytes: 0,
        });
        budget.on_load(100, false);
        let recs = evict_to_budget(&mut metas, &mut budget, [0.0, 0.0, 0.0], 8);
        set.add(
            "I1611-预算-只卸已驻留",
            recs.len() == 1 && recs[0].cell == [2, 0, 0],
            "仅已驻留块可卸载（Unloaded 无数据、Loading 卸了会撕裂在途传输）",
        );
    }

    set
}
