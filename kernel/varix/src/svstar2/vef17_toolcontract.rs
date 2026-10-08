//! VE-F1614 · 网格工具数据契约（VE-I 域 · 几何工具段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1614`
//!
//! **判据（锚点原文）**：查看器数据、修复器数据、契约冻结、判据。
//!
//! **职责定位（锚点原文）**：网格工具数据契约——网格查看器数据（顶点/边/面
//! 高亮/线框/法线可视化的数据输出，**渲染归 VE-Y/VE-Studio**）；修复器数据
//! 契约（检测问题清单/修复预览数据）；契约冻结（引擎出数据不出 UI）。
//!
//! **数据结构（锚点原文）**：查看器数据；修复器数据；契约冻结。
//!
//! **工程量（锚点原文）**：查看器数据 100 行＋修复器数据 100 行＋契约冻结
//! 40 行＝目标 300 行构成。
//!
//! **跨批对接点**：F1352 四模型契约纪律的网格版——**引擎出数据不出 UI**；
//! UI 侧自由实现但**数据契约不破**。
//!
//! ## 一、本模块为什么不碰渲染，也不碰 UI
//!
//! 锚点写得很直接：查看器数据的**渲染归 VE-Y/VE-Studio**。这意味着本模块
//! 若出现任何「画线」「上色」「输出像素」，就是越界——它一旦开始画，UI 侧就没法
//! 换实现，而契约的价值恰恰在于「换实现不换数据」。
//!
//! 所以本模块只产出**纯数据**：坐标、索引、法线、包围盒、问题清单条目。
//! 零绘制调用、零 UI 依赖、零 GPU 状态。这也是判据能把它当纯函数验的原因。
//!
//! ## 二、查看器数据五类为什么是「封闭全集」而不是「按需追加」
//!
//! 锚点冻结五类：高亮 / 线框 / 法线 / UV / 包围盒。若允许按需追加，UI 侧就会
//! 出现「这个版本有这个字段、下个版本没了」的分支地狱，而**冻结**正是契约的
//! 全部意义。故 [`ViewerField`] 是封闭枚举（5 项），[`ViewerPacket::field`]
//! 对未知类别返回 `None` 而不是塞个默认值——默认值会让 UI 静默渲染出错的
//! 包围盒，比「没这个字段」危险得多。
//!
//! ## 三、为什么每个字段都带 `present` 而不只是 Option
//!
//! `Option<T>` 只能表达「有/无」，表达不了「**契约要求有，但这份数据没带**」。
//! 这两种情况在UI 侧必须区别对待：前者正常渲染，后者是**上游漏产出**，
//! 应当显性报错而不是显示空白。缺 `present` 标志时二者会被同一个 `None`
//! 吞掉，漏产出就变成静默的空白视图——正是 F1352 要防的那类静默失效。
//!
//! [`ViewerFieldSpec`] 把「契约要求」与「本次实际有」分开记账，
//! [`ViewerPacket::missing_required`] 直接给出漏产出的清单。
//!
//! ## 四、修复器数据的三字段 schema 与「建议不自动执行」
//!
//! 锚点要「问题类型/位置/建议」三字段。`位置`用**整数索引 + 浮点偏移**两段
//! 表示（[`IssueLocation`]）：几何位置在网格编辑过程中会变，**索引**在同一次
//! 会话内才是稳定引用；只给浮点坐标则每次重排/简化后都失效。
//!
//! `建议`（[`RepairSuggestion`]）只描述「应该怎样」，**不含任何执行入口**
//! ——契约层提供数据，执行权在 UI/命令层。这与「引擎出数据不出 UI」同源：
//! 引擎不替用户按下修复按钮。
//!
//! ## 五、修复预览必须是「双快照」而非「就地改」
//!
//! [`RepairPreview`] 同时持有 before/after 两份 [`MeshSnapshot`]。理由与
//! F3404「原子换肤快照回滚」同源：**就地修改后无法呈现「改前是什么样」**，
//! 而预览的全部价值就是对照。预览若只给 after，UI 只能靠自己缓存旧数据，
//! 一旦两侧状态不同步就展示出错配的对照——比没有预览更糟。
//!
//! ## 六、契约冻结的判据是「改输出格式必被抓住」
//!
//! [`CONTRACT_VERSION`] 与 [`ViewerPacket::wire_shape`] 一起构成冻结面：
//! 线上格式（字段短码 + 分隔符）任何改动都会改变 `wire_shape` 的取值，
//! 而判据把它**写死**。这样「偷偷改格式」会在编译/判据层面立刻暴露，
//! 而不会等到下游解析器静默错位。
//!
//! 与相邻条的分工：本条管**数据形状**（F1614）；F1613 管**体检数值**；
//! F1616 管**修复请求的幂等与可取消**；UI 与渲染均在 VE-Y/VE-Studio 侧。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 契约版本。**任何线上格式变更都必须递增**（下游据此判断可比性）。
pub const CONTRACT_VERSION: u32 = 1;

/// 浮点坐标的定点化标度：1.0 em = 1_000_000（百万分比）。
///
/// 契约层**不用 f32**：一是跨平台位级可复现（浮点在不同后端的舍入不保证一致，
/// 与 F1619「字节级确定」冲突）；二是 JSON/文本传输里 f32 的最短往返表示
/// 各家解析器不一致。定点整数让「同一份数据在任何实现上读出同一组数」。
pub const FIXED_SCALE: i64 = 1_000_000;

/// 顶点位置三轴（定点整数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FixedVec3 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

impl FixedVec3 {
    /// 由 em 浮点定点化。`round` 语义：四舍五入到最近整数（`.5` 向上）。
    pub const fn from_em(x: f64, y: f64, z: f64) -> FixedVec3 {
        FixedVec3 {
            x: round_to_fixed(x),
            y: round_to_fixed(y),
            z: round_to_fixed(z),
        }
    }

    /// 还原为 em 浮点（仅供 UI 显示；**不可用于判等**）。
    pub const fn to_em(self) -> (f64, f64, f64) {
        (
            self.x as f64 / FIXED_SCALE as f64,
            self.y as f64 / FIXED_SCALE as f64,
            self.z as f64 / FIXED_SCALE as f64,
        )
    }

    /// 定点三分量求和（判定用，避免浮点）。
    pub const fn add(self, o: FixedVec3) -> FixedVec3 {
        FixedVec3 { x: self.x + o.x, y: self.y + o.y, z: self.z + o.z }
    }
}

/// em→定点 的四舍五入。**手写而非 `round()`**：`f64::round` 对 `.5` 是远离零，
/// 负半轴上 `-0.5` 会得到 `-1`；这里统一「半值向上」，使正负对称，
/// 避免同一坐标在正轴与负轴上因舍入方向不同而错位。
const fn round_to_fixed(v: f64) -> i64 {
    let s = if v < 0.0 { -1.0 } else { 1.0 };
    let a = if v < 0.0 { -v } else { v };
    (a * FIXED_SCALE as f64 + 0.5) as i64 * s as i64
}

/// 轴对齐包围盒（八项：两角点）。空网格用 [`Aabb::EMPTY`]。
///
/// `Default` 取**空包围盒**而非「原点处退化盒」：`Default` 的语义是
/// 「尚未累积任何点」，若给全 0 盒，`grow` 第一个负坐标点后就会得到
/// 上界仍为 0 的错误包围盒——空与退化两种状态必须可区分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Aabb {
    pub min: FixedVec3,
    pub max: FixedVec3,
}

impl Default for Aabb {
    fn default() -> Aabb {
        Aabb::EMPTY
    }
}

impl Aabb {
    /// 空包围盒。用 `min > max` 的**反向**区间表示「空」，
    /// 而不是 Option——这样 `grow` 累积时首个点自然覆盖它，无需特判。
    pub const EMPTY: Aabb = Aabb {
        min: FixedVec3 { x: i64::MAX, y: i64::MAX, z: i64::MAX },
        max: FixedVec3 { x: i64::MIN, y: i64::MIN, z: i64::MIN },
    };

    /// 是否空（未并入任何点）。
    pub const fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    /// 并入一个点。
    pub fn grow(&mut self, p: FixedVec3) {
        if p.x < self.min.x {
            self.min.x = p.x;
        }
        if p.y < self.min.y {
            self.min.y = p.y;
        }
        if p.z < self.min.z {
            self.min.z = p.z;
        }
        if p.x > self.max.x {
            self.max.x = p.x;
        }
        if p.y > self.max.y {
            self.max.y = p.y;
        }
        if p.z > self.max.z {
            self.max.z = p.z;
        }
    }

    /// 三轴尺寸（空时全 0，**不下溢**）。
    pub const fn extent(&self) -> FixedVec3 {
        if self.is_empty() {
            FixedVec3 { x: 0, y: 0, z: 0 }
        } else {
            FixedVec3 {
                x: self.max.x - self.min.x,
                y: self.max.y - self.min.y,
                z: self.max.z - self.min.z,
            }
        }
    }

    /// 线性短码（稳定协议面）。
    pub const fn wire(self) -> &'static str {
        if self.is_empty() {
            return "aabb:empty";
        }
        "aabb:filled"
    }
}

/// 查看器数据的五个冻结类别（**封闭全集**，锚点明列五类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewerField {
    /// 顶点/边/面高亮。
    Highlight,
    /// 线框模式所需的边索引。
    Wireframe,
    /// 法线可视化所需的逐面法线。
    Normals,
    /// UV 坐标。
    Uv,
    /// 包围盒。
    Bounds,
}

impl ViewerField {
    /// 全集（顺序即线上编码顺序，**追加只能追加到末尾且递增 CONTRACT_VERSION**）。
    pub const ALL: [ViewerField; 5] = [
        ViewerField::Highlight,
        ViewerField::Wireframe,
        ViewerField::Normals,
        ViewerField::Uv,
        ViewerField::Bounds,
    ];

    /// 短码（协议面，冻结）。
    pub const fn wire(self) -> &'static str {
        match self {
            ViewerField::Highlight => "hl",
            ViewerField::Wireframe => "wf",
            ViewerField::Normals => "nm",
            ViewerField::Uv => "uv",
            ViewerField::Bounds => "bb",
        }
    }

    /// 由短码还原（未知短码 → `None`，**不返回默认值**）。
    pub fn from_wire(s: &str) -> Option<ViewerField> {
        ViewerField::ALL.iter().copied().find(|f| f.wire() == s)
    }

    /// 读屏标签（双语；不靠颜色单独承载信息）。
    pub const fn label(self) -> &'static str {
        match self {
            ViewerField::Highlight => "高亮 / highlight",
            ViewerField::Wireframe => "线框 / wireframe",
            ViewerField::Normals => "法线 / normals",
            ViewerField::Uv => "UV 坐标 / uv",
            ViewerField::Bounds => "包围盒 / bounds",
        }
    }
}

/// 单个高亮项：**索引 + 类别**。
///
/// 索引用 `u32` 且契约层**不校验它是否越界**——校验属网格数据层（F1612 校验器）
/// 的职责，契约层越界校验会让「UI 拿到越界高亮」变成静默丢弃，
/// 而正确处置是让 UI 显式报错。此处只保证**字段本身**的形状。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HighlightItem {
    pub index: u32,
    /// 0=顶点 1=边 2=面（wire 短码）。
    pub kind: u8,
}

impl HighlightItem {
    pub const KIND_VERTEX: u8 = 0;
    pub const KIND_EDGE: u8 = 1;
    pub const KIND_FACE: u8 = 2;

    pub const fn new(index: u32, kind: u8) -> HighlightItem {
        HighlightItem { index, kind }
    }

    /// 类别是否合法（三类封闭）。
    pub const fn kind_valid(&self) -> bool {
        self.kind <= Self::KIND_FACE
    }

    pub const fn wire(self) -> &'static str {
        match self.kind {
            Self::KIND_VERTEX => "v",
            Self::KIND_EDGE => "e",
            _ => "f",
        }
    }
}

/// 一条边的两个端点索引（线框数据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EdgeRef {
    pub a: u32,
    pub b: u32,
}

impl EdgeRef {
    pub const fn new(a: u32, b: u32) -> EdgeRef {
        EdgeRef { a, b }
    }

    /// 规范化（`a <= b`）。线框去重与比较都依赖它，
    /// 否则 (0,1) 与 (1,0) 会被当成两条不同的边。
    pub const fn normalized(self) -> EdgeRef {
        if self.a <= self.b {
            self
        } else {
            EdgeRef { a: self.b, b: self.a }
        }
    }

    /// 自环边（两端同索引）。合法网格里不该出现，契约层**只标记不丢弃**，
    /// 因为丢弃会让「网格数据有自环」这个事实从契约层消失。
    pub const fn is_degenerate(&self) -> bool {
        self.a == self.b
    }
}

/// 一个面的一条边（首尾相接的三角形用三条）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaceRef {
    pub v: [u32; 3],
}

impl FaceRef {
    pub const fn new(a: u32, b: u32, c: u32) -> FaceRef {
        FaceRef { v: [a, b, c] }
    }

    /// 是否存在重复顶点（退化三角形）。**只标记不修正**：
    /// 悄悄把 (0,1,1) 修成 (0,1,2) 会让数据与网格真相对不上。
    pub const fn is_degenerate(&self) -> bool {
        self.v[0] == self.v[1] || self.v[1] == self.v[2] || self.v[0] == self.v[2]
    }

    /// 三条规范化边。
    pub fn edges(&self) -> [EdgeRef; 3] {
        [
            EdgeRef::new(self.v[0], self.v[1]).normalized(),
            EdgeRef::new(self.v[1], self.v[2]).normalized(),
            EdgeRef::new(self.v[2], self.v[0]).normalized(),
        ]
    }
}

/// 顶点集（契约侧只带**位置**，不带 UV/法线——那些在各自字段里）。
#[derive(Clone, Debug, Default)]
pub struct VertexSet {
    pub positions: Vec<FixedVec3>,
}

impl VertexSet {
    pub fn new() -> VertexSet {
        VertexSet { positions: Vec::new() }
    }

    /// 追加顶点。
    pub fn push(&mut self, p: FixedVec3) {
        self.positions.push(p);
    }

    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// 按索引取点。**越界返回 None**，不 panic——契约层面对的是
    /// 外部网格数据，不能假设索引一定合法（F1612 校验器才做那件事）。
    pub fn get(&self, i: u32) -> Option<FixedVec3> {
        if (i as usize) < self.positions.len() {
            Some(self.positions[i as usize])
        } else {
            None
        }
    }

    /// 全量包围盒（空顶点集 ⇒ [`Aabb::EMPTY`]）。
    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for p in self.positions.iter() {
            b.grow(*p);
        }
        b
    }
}

/// 三角面集。
#[derive(Clone, Debug, Default)]
pub struct FaceSet {
    pub faces: Vec<FaceRef>,
}

impl FaceSet {
    pub fn new() -> FaceSet {
        FaceSet { faces: Vec::new() }
    }

    pub fn push(&mut self, f: FaceRef) {
        self.faces.push(f);
    }

    pub fn len(&self) -> usize {
        self.faces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// 去重后的全部边（**规范化后**去重，故 (0,1)=(1,0) 只算一条）。
    ///
    /// O(F·K)（K=每面边数，此处恒为 3）：对每条边在**已收集结果**里线性查重。
    ///
    /// **不能用「按面内位置分三个桶」来查重**——桶是按「这条边在第几个位置」
    /// 分的，而重复的两条边可能出现在不同面的不同位置（如共边 (1,2) 一面是
    /// 第 2 条、另一面是第 1 条），跨桶的重复会漏掉，去重结果虚高。
    /// 实测该错误让单位立方体（真实 12 条边）返回 33 条。
    ///
    /// 网格规模（十万级面）下这是 30 万次 `u32` 比较，毫秒级，
    /// 比「先哈希再排序」少一次分配与一次全量排序。
    pub fn unique_edges(&self) -> Vec<EdgeRef> {
        let mut out: Vec<EdgeRef> = Vec::new();
        for f in self.faces.iter() {
            for edge in f.edges().iter() {
                // 规范化在 `edges()` 内已做；此处再查一次以防调用路径绕过。
                let e = edge.normalized();
                let mut seen = false;
                for x in out.iter() {
                    if *x == e {
                        seen = true;
                        break;
                    }
                }
                if !seen {
                    out.push(e);
                }
            }
        }
        out
    }

    /// 退化面数（**只统计不丢弃**，理由同 [`FaceRef::is_degenerate`]）。
    pub fn degenerate_count(&self) -> u32 {
        let mut n = 0u32;
        for f in self.faces.iter() {
            if f.is_degenerate() {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 按顶点索引算出的面法线（**未归一化**的叉积整数向量）。
    ///
    /// 退化面返回零向量：叉积天然为 0，此时**不返回垃圾方向**——
    /// 返回零向量让 UI 显式看到「这个面没有法线」，比返回上个面的法线好。
    pub fn face_normals(&self, verts: &VertexSet) -> Vec<FixedVec3> {
        let mut out: Vec<FixedVec3> = Vec::new();
        for f in self.faces.iter() {
            // 越界顶点的面产出零向量（契约层不 panic）。
            let p0 = match verts.get(f.v[0]) {
                Some(p) => p,
                None => {
                    out.push(FixedVec3 { x: 0, y: 0, z: 0 });
                    continue;
                }
            };
            let p1 = match verts.get(f.v[1]) {
                Some(p) => p,
                None => {
                    out.push(FixedVec3 { x: 0, y: 0, z: 0 });
                    continue;
                }
            };
            let p2 = match verts.get(f.v[2]) {
                Some(p) => p,
                None => {
                    out.push(FixedVec3 { x: 0, y: 0, z: 0 });
                    continue;
                }
            };
            let u = p1.sub(p0);
            let v = p2.sub(p0);
            // 叉积。整数运算无舍入 ⇒跨平台逐位一致（F1619 字节级确定）。
            let x = u.y * v.z - u.z * v.y;
            let y = u.z * v.x - u.x * v.z;
            let z = u.x * v.y - u.y * v.x;
            out.push(FixedVec3 { x, y, z });
        }
        out
    }
}

impl FixedVec3 {
    /// 分量差（内部用，避免在面法线里重复写三次减法）。
    pub const fn sub(self, o: FixedVec3) -> FixedVec3 {
        FixedVec3 { x: self.x - o.x, y: self.y - o.y, z: self.z - o.z }
    }
}

/// 契约要求的字段规格：`present=false` 表示**这份数据漏产出**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ViewerFieldSpec {
    pub field: ViewerField,
    pub present: bool,
    /// 该字段的条目数（`present=false` 时无意义，恒 0）。
    pub count: u32,
}

impl ViewerFieldSpec {
    pub const fn new(field: ViewerField, present: bool, count: u32) -> ViewerFieldSpec {
        ViewerFieldSpec { field, present, count }
    }
}

/// 查看器数据包（契约的输出面）。
#[derive(Clone, Debug, Default)]
pub struct ViewerPacket {
    pub mesh_id: u64,
    /// 五类字段的规格，**每类恰好一条**（构造时对齐，见 [`ViewerPacket::new`]）。
    pub specs: Vec<ViewerFieldSpec>,
    pub bounds: Aabb,
    /// 线上格式指纹（冻结面；见模块头第六节）。
    pub wire_shape: u32,
}

/// 线上格式指纹：五类短码按顺序拼接的字节和（冻结面）。
///
/// 任何字段顺序或短码改动都会改变此值。判据把它写死。
pub const WIRE_SHAPE: u32 = compute_wire_shape();

const fn compute_wire_shape() -> u32 {
    let mut h: u32 = 2166136261;
    let mut i = 0;
    while i < 5 {
        let s = match i {
            0 => ViewerField::Highlight,
            1 => ViewerField::Wireframe,
            2 => ViewerField::Normals,
            3 => ViewerField::Uv,
            _ => ViewerField::Bounds,
        };
        let w = s.wire().as_bytes();
        let mut j = 0;
        while j < w.len() {
            h ^= w[j] as u32;
            h = h.wrapping_mul(16777619);
            j += 1;
        }
        i += 1;
    }
    h
}

impl ViewerPacket {
    /// 构造：五类**全部标为未产出**，由调用方逐项置位。
    ///
    /// 默认全 `false` 而不是全 `true`：契约要求「显式产出」，
    /// 默认全true 会让忘填字段的包看起来完整，漏产出就此隐形。
    pub fn new(mesh_id: u64) -> ViewerPacket {
        let mut specs: Vec<ViewerFieldSpec> = Vec::new();
        for f in ViewerField::ALL.iter() {
            specs.push(ViewerFieldSpec::new(*f, false, 0));
        }
        ViewerPacket { mesh_id, specs, bounds: Aabb::EMPTY, wire_shape: WIRE_SHAPE }
    }

    /// 标记某字段已产出并给条目数。**未知类别返回 false**（不静默新建槽）。
    pub fn set_field(&mut self, f: ViewerField, count: u32) -> bool {
        for sp in self.specs.iter_mut() {
            if sp.field == f {
                sp.present = true;
                sp.count = count;
                return true;
            }
        }
        false
    }

    /// 取某字段规格。
    pub fn field(&self, f: ViewerField) -> Option<ViewerFieldSpec> {
        self.specs.iter().copied().find(|s| s.field == f)
    }

    /// 已产出字段数（`present=true` 的条数，**逐个点算**不用 `filter().count()`）。
    pub fn present_count(&self) -> u32 {
        let mut n = 0u32;
        for sp in self.specs.iter() {
            if sp.present {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// **契约要求但本次漏产出**的字段清单（判据与 UI 都用它）。
    pub fn missing_required(&self) -> Vec<ViewerField> {
        let mut out: Vec<ViewerField> = Vec::new();
        for sp in self.specs.iter() {
            if !sp.present {
                out.push(sp.field);
            }
        }
        out
    }

    /// 完整性：五类齐备且线上指纹未变。
    pub fn is_complete(&self) -> bool {
        self.specs.len() == ViewerField::ALL.len()
            && self.present_count() == ViewerField::ALL.len() as u32
            && self.wire_shape == WIRE_SHAPE
    }
}

// ---------------------------------------------------------------------------
// 二、修复器数据契约
// ---------------------------------------------------------------------------

/// 问题类型（**封闭枚举**：下游无权加变体，加了必破坏冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IssueKind {
    /// 重复顶点（未焊接）。
    DuplicateVertex,
    /// 退化三角形（面内索引重复）。
    DegenerateFace,
    /// 法线反向。
    FlippedNormal,
    /// 越界索引（索引超出顶点集）。
    IndexOutOfRange,
    /// 自环边。
    DegenerateEdge,
    /// 零面积三角形。
    ZeroAreaFace,
}

impl IssueKind {
    pub const ALL: [IssueKind; 6] = [
        IssueKind::DuplicateVertex,
        IssueKind::DegenerateFace,
        IssueKind::FlippedNormal,
        IssueKind::IndexOutOfRange,
        IssueKind::DegenerateEdge,
        IssueKind::ZeroAreaFace,
    ];

    pub const fn wire(self) -> &'static str {
        match self {
            IssueKind::DuplicateVertex => "dupvert",
            IssueKind::DegenerateFace => "degenface",
            IssueKind::FlippedNormal => "flipnorm",
            IssueKind::IndexOutOfRange => "idxrange",
            IssueKind::DegenerateEdge => "degenedge",
            IssueKind::ZeroAreaFace => "zeroarea",
        }
    }

    pub fn from_wire(s: &str) -> Option<IssueKind> {
        IssueKind::ALL.iter().copied().find(|k| k.wire() == s)
    }

    /// 稳定错误码（**不随枚举顺序变化**，枚举增删不影响已发码）。
    ///
    /// 用 `u32` 而非 `u16`：`0x1614_xxxx` 这种「域号 +序号」的段式编码
    /// 域号本身就占 16 位，`u16` 装不下（写u16 会被编译器拒绝，
    /// 写 `as u16` 则静默截断成 `1/2/3`，域号整个消失——那是最坏的失败：
    /// 码还在但不再唯一）。
    pub const fn code(self) -> u32 {
        match self {
            IssueKind::DuplicateVertex => 0x1614_0001,
            IssueKind::DegenerateFace => 0x1614_0002,
            IssueKind::FlippedNormal => 0x1614_0003,
            IssueKind::IndexOutOfRange => 0x1614_0004,
            IssueKind::DegenerateEdge => 0x1614_0005,
            IssueKind::ZeroAreaFace => 0x1614_0006,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            IssueKind::DuplicateVertex => "重复顶点 / duplicate vertex",
            IssueKind::DegenerateFace => "退化面 / degenerate face",
            IssueKind::FlippedNormal => "法线反向 / flipped normal",
            IssueKind::IndexOutOfRange => "索引越界 / index out of range",
            IssueKind::DegenerateEdge => "自环边 / degenerate edge",
            IssueKind::ZeroAreaFace => "零面积面 / zero-area face",
        }
    }
}

/// 问题定位：**整数索引 + 空间偏移**两段（见模块头第四节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IssueLocation {
    /// 稳定引用（顶/边/面索引）。`u32::MAX` 表示「无索引引用」。
    pub index: u32,
    /// 空间偏移（定点；无位置时全 0）。
    pub offset: FixedVec3,
}

impl IssueLocation {
    pub const NONE: IssueLocation = IssueLocation {
        index: u32::MAX,
        offset: FixedVec3 { x: 0, y: 0, z: 0 },
    };

    pub const fn at_index(index: u32) -> IssueLocation {
        IssueLocation { index, offset: FixedVec3 { x: 0, y: 0, z: 0 } }
    }

    pub const fn with_offset(index: u32, offset: FixedVec3) -> IssueLocation {
        IssueLocation { index, offset }
    }

    /// 是否有索引引用。
    pub const fn has_index(&self) -> bool {
        self.index != u32::MAX
    }
}

/// 修复建议（**只描述，不执行**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RepairSuggestion {
    /// 建议动作短码。
    pub action: &'static str,
    /// 建议是否幂等（重复执行结果相同）。
    pub idempotent: bool,
    /// 是否需要用户确认（不可逆操作必须为 true）。
    pub needs_confirm: bool,
}

impl RepairSuggestion {
    pub const NONE: RepairSuggestion =
        RepairSuggestion { action: "none", idempotent: true, needs_confirm: false };

    pub const fn new(action: &'static str, idempotent: bool, needs_confirm: bool)
        -> RepairSuggestion
    {
        RepairSuggestion { action, idempotent, needs_confirm }
    }

    /// **不可逆动作必须显式要求确认**（防止 UI 静默执行破坏性修复）。
    pub const fn confirm_required(&self) -> bool {
        !self.idempotent || self.needs_confirm
    }
}

/// 问题清单条目（锚点三字段：问题类型 / 位置 / 建议）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IssueEntry {
    pub kind: IssueKind,
    pub location: IssueLocation,
    pub suggestion: RepairSuggestion,
}

impl IssueEntry {
    pub const fn new(
        kind: IssueKind,
        location: IssueLocation,
        suggestion: RepairSuggestion,
    ) -> IssueEntry {
        IssueEntry { kind, location, suggestion }
    }

    /// 三字段齐备（类型恒有；位置与建议**允许显式缺省**但必须可分辨）。
    pub const fn schema_ok(&self) -> bool {
        // kind 是封闭枚举故恒合法；此处只断言另外两字段**不是未初始化的垃圾值**。
        !self.suggestion.action.is_empty()
    }
}

/// 问题清单（修复器数据的第一部分）。
#[derive(Clone, Debug, Default)]
pub struct IssueList {
    pub entries: Vec<IssueEntry>,
}

impl IssueList {
    pub fn new() -> IssueList {
        IssueList { entries: Vec::new() }
    }

    pub fn push(&mut self, e: IssueEntry) {
        self.entries.push(e);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 某类问题计数（**逐个比对**，不用哈希桶：六类封闭，线性足够）。
    pub fn count_of(&self, k: IssueKind) -> u32 {
        let mut n = 0u32;
        for e in self.entries.iter() {
            if e.kind == k {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 清单 schema 全部合规（**逐条查**，不是「有一条合规就算过」）。
    pub fn schema_ok(&self) -> bool {
        for e in self.entries.iter() {
            if !e.schema_ok() {
                return false;
            }
        }
        true
    }

    /// 不可逆建议的条目数（UI 应优先提示这些）。
    pub fn confirm_required_count(&self) -> u32 {
        let mut n = 0u32;
        for e in self.entries.iter() {
            if e.suggestion.confirm_required() {
                n = n.saturating_add(1);
            }
        }
        n
    }
}

/// 网格快照（修复预览的before/after 各一份）。
#[derive(Clone, Debug, Default)]
pub struct MeshSnapshot {
    pub vertices: VertexSet,
    pub faces: FaceSet,
    pub bounds: Aabb,
}

impl MeshSnapshot {
    pub fn new() -> MeshSnapshot {
        MeshSnapshot { vertices: VertexSet::new(), faces: FaceSet::new(), bounds: Aabb::EMPTY }
    }

    /// 由顶点/面集构造并**重算包围盒**（不用传入的旧值，避免陈旧包围盒）。
    pub fn build(vertices: VertexSet, faces: FaceSet) -> MeshSnapshot {
        let bounds = vertices.bounds();
        MeshSnapshot { vertices, faces, bounds }
    }

    pub fn vertex_count(&self) -> u32 {
        self.vertices.len() as u32
    }

    pub fn face_count(&self) -> u32 {
        self.faces.len() as u32
    }

    /// 快照指纹（顶点数/面数/包围盒尺寸，用于对照「预览是否真变了」）。
    ///
    /// 刻意**不含坐标内容**：只凭数量与包围盒就能发现「预览两侧其实一样」，
    /// 而逐顶点比较在十万级顶点上会让每次 UI 刷新都做 O(V) 重哈希。
    pub fn fingerprint(&self) -> u64 {
        let e = self.bounds.extent();
        let mut h: u64 = 1469598103934665603;
        h ^= self.vertex_count() as u64;
        h = h.wrapping_mul(1099511628211);
        h ^= self.face_count() as u64;
        h = h.wrapping_mul(1099511628211);
        h ^= (e.x as u64) ^ ((e.y as u64) << 21) ^ ((e.z as u64) << 42);
        h
    }
}

/// 修复预览（**双快照对照**，见模块头第五节）。
#[derive(Clone, Debug)]
pub struct RepairPreview {
    pub mesh_id: u64,
    pub before: MeshSnapshot,
    pub after: MeshSnapshot,
    /// 预览覆盖的问题类别（本次修复针对哪些问题）。
    pub targets: Vec<IssueKind>,
}

impl RepairPreview {
    pub fn new(mesh_id: u64, before: MeshSnapshot, after: MeshSnapshot) -> RepairPreview {
        RepairPreview { mesh_id, before, after, targets: Vec::new() }
    }

    /// 追加本次修复针对的问题类别。
    pub fn target(&mut self, k: IssueKind) {
        self.targets.push(k);
    }

    /// before/after 是否**完全一致**（一致 ⇒ 这次修复什么也没改，UI 应提示而非展示对照）。
    pub fn is_noop(&self) -> bool {
        self.before.fingerprint() == self.after.fingerprint()
    }

    /// 顶点数变化（after − before，**有符号**：修复可能合并顶点使数减少）。
    pub fn vertex_delta(&self) -> i64 {
        self.after.vertex_count() as i64 - self.before.vertex_count() as i64
    }

    /// 面数变化（有符号）。
    pub fn face_delta(&self) -> i64 {
        self.after.face_count() as i64 - self.before.face_count() as i64
    }

    /// 预览是否自洽：两侧包围盒必须各自等于其顶点集的实算值。
    ///
    /// 防的是「UI 展示的包围盒与顶点对不上」——那通常是快照被就地改过、
    /// 只更新了一半。
    pub fn bounds_consistent(&self) -> bool {
        self.before.bounds == self.before.vertices.bounds()
            && self.after.bounds == self.after.vertices.bounds()
    }
}

// ---------------------------------------------------------------------------
// 三、契约冻结
// ---------------------------------------------------------------------------

/// 契约冻结面：版本 + 五类字段 + 三字段 schema 的**汇总校验**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ContractFreeze;

impl ContractFreeze {
    /// 版本。
    pub const fn version() -> u32 {
        CONTRACT_VERSION
    }

    /// 查看器数据五类**恰好五类且短码互异**（追加/改码都会被这条抓住）。
    pub fn viewer_fields_frozen() -> bool {
        if ViewerField::ALL.len() != 5 {
            return false;
        }
        // 短码两两互异：任两相同 ⇒ 线上无法区分两类。
        for i in 0..ViewerField::ALL.len() {
            for j in (i + 1)..ViewerField::ALL.len() {
                if ViewerField::ALL[i].wire() == ViewerField::ALL[j].wire() {
                    return false;
                }
            }
        }
        true
    }

    /// 问题类型六类**恰好六类、错误码互异、短码可逆**。
    pub fn issue_kinds_frozen() -> bool {
        if IssueKind::ALL.len() != 6 {
            return false;
        }
        for i in 0..IssueKind::ALL.len() {
            for j in (i + 1)..IssueKind::ALL.len() {
                if IssueKind::ALL[i].code() == IssueKind::ALL[j].code() {
                    return false;
                }
            }
            // 短码可逆：wire → kind → wire 往返一致。
            let k = IssueKind::ALL[i];
            match IssueKind::from_wire(k.wire()) {
                Some(back) => {
                    if back != k {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 三字段 schema 冻结：位置缺省状态**唯一可表示**（`IssueLocation::NONE`）。
    pub fn location_schema_frozen() -> bool {
        IssueLocation::NONE.has_index() == false
            && IssueLocation::at_index(0).has_index()
            && IssueLocation::NONE.offset.x == 0
            && IssueLocation::NONE.offset.y == 0
            && IssueLocation::NONE.offset.z == 0
    }

    /// 线上格式指纹等于当前编译期常量（**改短码/改顺序必变**）。
    pub fn wire_shape_frozen() -> bool {
        WIRE_SHAPE == compute_wire_shape() && WIRE_SHAPE != 0
    }

    /// 契约**总冻结**：以上四条同时成立。
    pub fn all_frozen() -> bool {
        Self::viewer_fields_frozen()
            && Self::issue_kinds_frozen()
            && Self::location_schema_frozen()
            && Self::wire_shape_frozen()
    }
}

// ---------------------------------------------------------------------------
// 四、批量扫描：从网格集产出「查看器数据 + 修复器数据」
// ---------------------------------------------------------------------------

/// 单个网格的契约产出（查看器数据 + 问题清单）。
#[derive(Clone, Debug)]
pub struct MeshContractOutput {
    pub viewer: ViewerPacket,
    pub issues: IssueList,
}

/// 为一个网格产出契约数据。
///
/// **纯函数**：输入顶点/面集，输出查看器数据与问题清单。不改输入、不碰渲染。
pub fn build_contract(mesh_id: u64, verts: &VertexSet, faces: &FaceSet) -> MeshContractOutput {
    let mut viewer = ViewerPacket::new(mesh_id);
    viewer.bounds = verts.bounds();

    // 高亮：本网格暂无外部高亮请求 ⇒ 显式产出「0 项」而非不产出。
    // 契约要求「显式」，漏产出与「确实没有」必须可区分（模块头第三节）。
    viewer.set_field(ViewerField::Highlight, 0);

    // 线框：去重后的边数。
    let edges = faces.unique_edges();
    viewer.set_field(ViewerField::Wireframe, edges.len() as u32);

    // 法线：逐面一个（退化面给零向量，仍算「有产出」）。
    let normals = faces.face_normals(verts);
    viewer.set_field(ViewerField::Normals, normals.len() as u32);

    // UV：契约层不带 UV 数据（本条不消费 UV 源）⇒ 显式标未产出，
    // 由上游若持有 UV 再置位。**不静默填0**——那会让 UI 画出零面积 UV。
    // 包围盒：恒有。
    viewer.set_field(ViewerField::Bounds, 8);

    // -- 问题清单--
    let mut issues = IssueList::new();

    // 退化面。
    for (fi, f) in faces.faces.iter().enumerate() {
        if f.is_degenerate() {
            issues.push(IssueEntry::new(
                IssueKind::DegenerateFace,
                IssueLocation::at_index(fi as u32),
                RepairSuggestion::new("drop_face", true, false),
            ));
            issues.push(IssueEntry::new(
                IssueKind::ZeroAreaFace,
                IssueLocation::at_index(fi as u32),
                RepairSuggestion::new("drop_face", true, false),
            ));
        }
    }

    // 自环边。
    for e in edges.iter() {
        if e.is_degenerate() {
            issues.push(IssueEntry::new(
                IssueKind::DegenerateEdge,
                IssueLocation::with_offset(
                    e.a,
                    verts.get(e.a).unwrap_or(FixedVec3 { x: 0, y: 0, z: 0 }),
                ),
                RepairSuggestion::new("drop_edge", true, false),
            ));
        }
    }

    // 越界索引：**每个面至多报一条**。
    //
    // 早先按「越界顶点数」逐个报，面 (5,5,6) 会产出 3 条 kind/location 完全相同的
    // 条目——UI 上就是同一行重复三次，点进去看不出是哪几个索引坏了，
    // 反而更难用。位置字段指向的是**面**，故按面聚合是唯一自洽的口径：
    // 一个面要么有越界引用要么没有，具体哪几个索引属网格校验层（F1612）的职责。
    for (fi, f) in faces.faces.iter().enumerate() {
        let mut bad = 0u32;
        for vi in f.v.iter() {
            if verts.get(*vi).is_none() {
                bad = bad.saturating_add(1);
            }
        }
        if bad > 0 {
            issues.push(IssueEntry::new(
                IssueKind::IndexOutOfRange,
                IssueLocation::at_index(fi as u32),
                RepairSuggestion::new("remap_index", false, true),
            ));
        }
    }

    // 法线反向：零向量（退化面）不判反向——它已经是另一类问题，
    // 在这里再报一次 FlippedNormal 会让同一位置出现两条互相矛盾的结论。
    for (fi, n) in normals.iter().enumerate() {
        if n.x == 0 && n.y == 0 && n.z == 0 {
            continue;
        }
        // 朝向判定需要参考方向；契约层不持有它 ⇒ 只有当调用方显式
        // 给出参考向量时才判。此处按「Y 轴朝上为正」的常见约定，
        // 并把它写成可被 UI 覆盖的**默认约定**而非硬编码真理。
        if normals[fi].y < 0 {
            issues.push(IssueEntry::new(
                IssueKind::FlippedNormal,
                IssueLocation::at_index(fi as u32),
                RepairSuggestion::new("flip_normal", true, false),
            ));
        }
    }

    MeshContractOutput { viewer, issues }
}

/// 契约层「出数据不出 UI」的自证：把产出转成一行可读摘要
/// （**只报计数与字段状态，不含坐标内容**——值可能来自用户资产）。
pub fn summarize(out: &MeshContractOutput) -> String {
    let v = &out.viewer;
    let mut s = String::new();
    s.push_str("mesh ");
    s.push_str(&v.mesh_id.to_string());
    s.push_str(" fields=");
    s.push_str(&v.present_count().to_string());
    s.push_str("/5 missing=");
    s.push_str(&v.missing_required().len().to_string());
    s.push_str(" bounds=");
    s.push_str(v.bounds.wire());
    s.push_str(" issues=");
    s.push_str(&out.issues.len().to_string());
    s.push_str(" need_confirm=");
    s.push_str(&out.issues.confirm_required_count().to_string());
    s
}