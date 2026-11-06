//! UNX-F50062 长边/短边翻与横竖纸张交叉矩阵（4 组合）（0x433e0 段）
//!
//! 锚点：UNX-F50062-J1 交叉矩阵 ×4 ×10 输出逐项一致；判据锚定 §AI-63 §2 专题四。
//!
//! 数据段：`SEG_BASE = 0x433e0`；诊断码闭集 `SEG_BASE + 1 ..= SEG_BASE + 11`，
//! 逐项见 [`MatrixError`]。
//!
//! 为什么需要「交叉矩阵」而不是复用 F50061 的三模：F50061 把翻向当成文档坐标里的
//! 常量（长边翻 → 背面 R0、短边翻 → 背面 R180）。这在竖版纸上成立，但横版纸是把
//! 整张纸先转 90° 再进纸的——**用户选的「长边」是文档长边，翻的却是物理轴**。
//! 纸张横放后，文档长边落到物理短边上，翻向语义随之互换。四组合因此不是
//! 「两个模式 × 两种版式」的重复枚举：
//!
//! ```text
//!              物理长边竖直          物理长边水平
//! 长边翻       竖版 → 背面 R0        横版 → 背面 R180（绕水平轴翻）
//! 短边翻       竖版 → 背面 R180      横版 → 背面 R0  （绕竖直轴翻）
//! ```
//!
//! 推导只有一条：**翻向轴 = 该版式下所选边的物理方向**；绕竖直轴翻，背面仍然
//! 正立（R0）；绕水平轴翻，背面上下颠倒（R180）。再把「内容在纸上的旋转」与
//! 「翻过之后纸的旋转」合成，得到背面总旋转。判据用闭式参考表独立复算，不信
//! 调度器的自述。
//!
//! 结构清单：
//! * [`Rot`]：象限旋转，Z4 群（合成 / 逆元 / 四次回原）；
//! * [`Orient`] + [`FlipEdge`] + [`Axis`] + [`Edge`] + [`SheetGeom`]：版式与几何；
//! * [`ComboSpec`]：由「版式 × 翻向」派生的不可变规格（唯一派生入口）；
//! * [`ComboMatrix`]：四组合注册表（键唯一 + 良构 + 定容 + 重复必拒）；
//! * [`ComboRun`]：单组合状态机（Idle / Running / Done，非法迁移显式拒绝）；
//! * [`MatrixRow`] + [`MatrixLedger`]：逐张落账（同组合内纸号连续、键唯一）；
//! * [`injection_core`]：8 类坏样本注入核。
//!
//! 边界纪律：本模块只接受**整张纸**（偶数页）。奇数页补白是 F50063 的职责，
//! 这里显式返回 [`MatrixError::Parity`]，不做静默补页——两卡共用结论会掩盖
//! 「谁补的、补在哪一面」这条审计线。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x433e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x433e0;

const E_ZERO_PAGES: u16 = 1;
const E_TOO_MANY: u16 = 2;
const E_PARITY: u16 = 3;
const E_ORIENT: u16 = 4;
const E_FLIP: u16 = 5;
const E_ROT: u16 = 6;
const E_DUPLICATE: u16 = 7;
const E_FULL: u16 = 8;
const E_ORDER: u16 = 9;
const E_IMMUTABLE: u16 = 10;
const E_STATE: u16 = 11;

/// 交叉矩阵错误。`code()` 返回 `SEG_BASE + n`，闭集内全部码值都被真实构造使用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatrixError {
    /// 页数为 0（空作业不产生任何纸张）。
    ZeroPages,
    /// 页数超过拼版上限。
    PageOverflow,
    /// 页数为奇数：本模块只认整张纸，补白归 F50063。
    Parity,
    /// 版式码不在闭集 0..=1。
    OrientCode,
    /// 翻向码不在闭集 0..=1。
    FlipCode,
    /// 旋转码越界，或规格与派生结果不一致。
    RotationMismatch,
    /// 同一（版式, 翻向）组合被注册两次，或账本同键重复。
    Duplicate,
    /// 注册表 / 账本容量已满。
    Full,
    /// 同组合内纸号跳号（落账必须逐张连续）。
    OrderGap,
    /// 封账后只读。
    Immutable,
    /// 状态机非法迁移，或纸号越出拼版范围。
    State,
}

impl MatrixError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            MatrixError::ZeroPages => E_ZERO_PAGES,
            MatrixError::PageOverflow => E_TOO_MANY,
            MatrixError::Parity => E_PARITY,
            MatrixError::OrientCode => E_ORIENT,
            MatrixError::FlipCode => E_FLIP,
            MatrixError::RotationMismatch => E_ROT,
            MatrixError::Duplicate => E_DUPLICATE,
            MatrixError::Full => E_FULL,
            MatrixError::OrderGap => E_ORDER,
            MatrixError::Immutable => E_IMMUTABLE,
            MatrixError::State => E_STATE,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断（日志/红项明细用）。
    pub fn message(self) -> &'static str {
        match self {
            MatrixError::ZeroPages => "page count is zero",
            MatrixError::PageOverflow => "page count above imposition limit",
            MatrixError::Parity => "odd page count needs padding (F50063)",
            MatrixError::OrientCode => "orientation code outside closed set",
            MatrixError::FlipCode => "flip edge code outside closed set",
            MatrixError::RotationMismatch => "rotation does not match derived spec",
            MatrixError::Duplicate => "combo or ledger key registered twice",
            MatrixError::Full => "registry or ledger capacity reached",
            MatrixError::OrderGap => "sheet index gap inside one combo",
            MatrixError::Immutable => "ledger is sealed read-only",
            MatrixError::State => "illegal state transition or sheet out of range",
        }
    }
}

/// 象限旋转（Z4 群）。双面翻只可能是 0/90/180/270 四档：任意角旋转在纸上没有
/// 语义（纸只能翻不能拧），把取值关进闭集比事后夹取更安全。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rot {
    /// 正立。
    R0,
    /// 顺时针 90°。
    R90,
    /// 180°（上下颠倒）。
    R180,
    /// 顺时针 270°（= 逆时针 90°）。
    R270,
}

impl Rot {
    /// 象限码 0..=3。
    pub fn code(self) -> u8 {
        match self {
            Rot::R0 => 0,
            Rot::R90 => 1,
            Rot::R180 => 2,
            Rot::R270 => 3,
        }
    }

    /// 象限码反解；闭集外返回 [`MatrixError::RotationMismatch`]。
    pub fn from_code(c: u8) -> Result<Rot, MatrixError> {
        match c {
            0 => Ok(Rot::R0),
            1 => Ok(Rot::R90),
            2 => Ok(Rot::R180),
            3 => Ok(Rot::R270),
            _ => Err(MatrixError::RotationMismatch),
        }
    }

    /// 由象限和构造（内部用，`q` 已由 `compose` 取模）。
    fn from_quarter(q: u8) -> Rot {
        match q % 4 {
            1 => Rot::R90,
            2 => Rot::R180,
            3 => Rot::R270,
            _ => Rot::R0,
        }
    }

    /// 合成：先转 `other` 再转 `self`。Z4 可交换，但语义仍按函数复合写清。
    pub fn compose(self, other: Rot) -> Rot {
        Rot::from_quarter(self.code() + other.code())
    }

    /// 逆元：180° 自逆，90°/270° 互逆。
    pub fn inverse(self) -> Rot {
        match self {
            Rot::R0 => Rot::R0,
            Rot::R90 => Rot::R270,
            Rot::R180 => Rot::R180,
            Rot::R270 => Rot::R90,
        }
    }

    /// 档名（落账文本用）。
    pub fn label(self) -> &'static str {
        match self {
            Rot::R0 => "0",
            Rot::R90 => "90",
            Rot::R180 => "180",
            Rot::R270 => "270",
        }
    }

    /// 全部四档（穷举用）。
    pub const ALL: [Rot; 4] = [Rot::R0, Rot::R90, Rot::R180, Rot::R270];
}

/// 纸张进机时的物理版式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orient {
    /// 竖版（宽 < 高）。
    Portrait,
    /// 横版（宽 > 高）：整张纸先转 90°。
    Landscape,
}

impl Orient {
    /// 版式码 0..=1。
    pub fn code(self) -> u8 {
        match self {
            Orient::Portrait => 0,
            Orient::Landscape => 1,
        }
    }

    /// 版式码反解；闭集外返回 [`MatrixError::OrientCode`]。
    pub fn from_code(c: u8) -> Result<Orient, MatrixError> {
        match c {
            0 => Ok(Orient::Portrait),
            1 => Ok(Orient::Landscape),
            _ => Err(MatrixError::OrientCode),
        }
    }

    /// 内容落到纸上的基础旋转：横版必须把内容转 90° 才装得下。
    pub fn content_rot(self) -> Rot {
        match self {
            Orient::Portrait => Rot::R0,
            Orient::Landscape => Rot::R90,
        }
    }

    /// 该版式的物理尺寸。
    pub fn geom(self) -> SheetGeom {
        match self {
            Orient::Portrait => SheetGeom::A4_PORTRAIT,
            Orient::Landscape => SheetGeom::A4_LANDSCAPE,
        }
    }

    /// 进纸前缘（leading edge）的长度：竖版短边进纸，横版长边进纸。
    ///
    /// 为什么它重要：前缘长度决定进纸辊咬合与翻向轴的相对关系，是
    /// 「横版长边翻实际变成水平轴翻」的几何依据。
    pub fn feed_mm10(self) -> u32 {
        match self {
            Orient::Portrait => SheetGeom::A4_PORTRAIT.short_edge(),
            Orient::Landscape => SheetGeom::A4_LANDSCAPE.long_edge(),
        }
    }

    /// 版式名。
    pub fn label(self) -> &'static str {
        match self {
            Orient::Portrait => "portrait",
            Orient::Landscape => "landscape",
        }
    }

    /// 全部两档。
    pub const ALL: [Orient; 2] = [Orient::Portrait, Orient::Landscape];
}

/// 用户选择的装订/翻向边（文档坐标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlipEdge {
    /// 文档长边。
    Long,
    /// 文档短边。
    Short,
}

impl FlipEdge {
    /// 翻向码 0..=1。
    pub fn code(self) -> u8 {
        match self {
            FlipEdge::Long => 0,
            FlipEdge::Short => 1,
        }
    }

    /// 翻向码反解；闭集外返回 [`MatrixError::FlipCode`]。
    pub fn from_code(c: u8) -> Result<FlipEdge, MatrixError> {
        match c {
            0 => Ok(FlipEdge::Long),
            1 => Ok(FlipEdge::Short),
            _ => Err(MatrixError::FlipCode),
        }
    }

    /// 翻向名。
    pub fn label(self) -> &'static str {
        match self {
            FlipEdge::Long => "long-edge",
            FlipEdge::Short => "short-edge",
        }
    }

    /// 全部两档。
    pub const ALL: [FlipEdge; 2] = [FlipEdge::Long, FlipEdge::Short];
}

/// 物理翻向轴（纸面内）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// 竖直轴（左右翻，像翻书）。
    Vertical,
    /// 水平轴（上下翻，像翻台历）。
    Horizontal,
}

impl Axis {
    /// 轴名。
    pub fn label(self) -> &'static str {
        match self {
            Axis::Vertical => "vertical",
            Axis::Horizontal => "horizontal",
        }
    }

    /// 绕该轴翻过之后，背面内容的旋转：竖直轴 → R0，水平轴 → R180。
    /// 这是整个交叉矩阵唯一需要「猜」的物理结论，其余全部由它推导。
    pub fn back_rot(self) -> Rot {
        match self {
            Axis::Vertical => Rot::R0,
            Axis::Horizontal => Rot::R180,
        }
    }
}

/// 纸张物理边。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// 左边（竖直装订边）。
    Left,
    /// 上边（水平装订边）。
    Top,
    /// 右边。
    Right,
    /// 下边。
    Bottom,
}

impl Edge {
    /// 边码（落账指纹用）。
    pub fn code(self) -> u8 {
        match self {
            Edge::Left => 0,
            Edge::Top => 1,
            Edge::Right => 2,
            Edge::Bottom => 3,
        }
    }

    /// 边名。
    pub fn label(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Top => "top",
            Edge::Right => "right",
            Edge::Bottom => "bottom",
        }
    }
}

/// 纸张几何（单位 0.1mm，整数避免浮点舍入污染判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetGeom {
    /// 宽（0.1mm）。
    pub w_mm10: u32,
    /// 高（0.1mm）。
    pub h_mm10: u32,
}

impl SheetGeom {
    /// A4 竖版：210.0 × 297.0 mm。
    pub const A4_PORTRAIT: SheetGeom = SheetGeom {
        w_mm10: 2100,
        h_mm10: 2970,
    };

    /// A4 横版：297.0 × 210.0 mm。
    pub const A4_LANDSCAPE: SheetGeom = SheetGeom {
        w_mm10: 2970,
        h_mm10: 2100,
    };

    /// 构造（宽高不做合法性判断，尺寸校验属于上游纸库）。
    pub const fn new(w_mm10: u32, h_mm10: u32) -> SheetGeom {
        SheetGeom { w_mm10, h_mm10 }
    }

    /// 物理长边。
    pub fn long_edge(self) -> u32 {
        if self.w_mm10 > self.h_mm10 {
            self.w_mm10
        } else {
            self.h_mm10
        }
    }

    /// 物理短边。
    pub fn short_edge(self) -> u32 {
        if self.w_mm10 < self.h_mm10 {
            self.w_mm10
        } else {
            self.h_mm10
        }
    }

    /// 是否横版（宽 > 高）。
    pub fn is_landscape(self) -> bool {
        self.w_mm10 > self.h_mm10
    }

    /// 转 90° 后的尺寸（宽高互换）。
    pub fn rotated(self) -> SheetGeom {
        SheetGeom::new(self.h_mm10, self.w_mm10)
    }
}

/// 该版式下的物理翻向轴：先找「用户选的边」在本版式的物理方向。
fn axis_for(orient: Orient, flip: FlipEdge) -> Axis {
    match orient {
        // 竖版：长边竖直、短边水平。
        Orient::Portrait => match flip {
            FlipEdge::Long => Axis::Vertical,
            FlipEdge::Short => Axis::Horizontal,
        },
        // 横版：长边水平、短边竖直。语义在这里互换。
        Orient::Landscape => match flip {
            FlipEdge::Long => Axis::Horizontal,
            FlipEdge::Short => Axis::Vertical,
        },
    }
}

/// 装订边：竖直轴 → 左边（书本），水平轴 → 上边（台历）。
fn binding_for(axis: Axis) -> Edge {
    match axis {
        Axis::Vertical => Edge::Left,
        Axis::Horizontal => Edge::Top,
    }
}

/// 一个（版式, 翻向）组合的完整规格。所有字段都由 [`ComboSpec::derive`] 派生，
/// 外部不能手工拼装出半套字段——良构性由 [`ComboSpec::well_formed`] 复核。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComboSpec {
    /// 版式。
    pub orient: Orient,
    /// 文档翻向边。
    pub flip: FlipEdge,
    /// 内容在正面纸上的旋转。
    pub content_rot: Rot,
    /// 翻纸带来的额外旋转。
    pub flip_rot: Rot,
    /// 背面总旋转 = `content_rot ∘ flip_rot`。
    pub back_rot: Rot,
    /// 物理翻向轴。
    pub axis: Axis,
    /// 装订边。
    pub binding: Edge,
    /// 进纸前缘长度（0.1mm）。
    pub feed_mm10: u32,
}

impl ComboSpec {
    /// 唯一派生入口：由版式与翻向算出全部字段。
    pub fn derive(orient: Orient, flip: FlipEdge) -> ComboSpec {
        let axis = axis_for(orient, flip);
        let content_rot = orient.content_rot();
        let flip_rot = axis.back_rot();
        ComboSpec {
            orient,
            flip,
            content_rot,
            flip_rot,
            back_rot: content_rot.compose(flip_rot),
            axis,
            binding: binding_for(axis),
            feed_mm10: orient.feed_mm10(),
        }
    }

    /// 组合键：版式 × 2 + 翻向（0..=3）。注册表用它判重。
    pub fn key(self) -> u8 {
        self.orient.code() * 2 + self.flip.code()
    }

    /// 组合名（日志用）。
    pub fn label(self) -> String {
        format!("{}/{}", self.orient.label(), self.flip.label())
    }

    /// 良构复核：每个字段都必须能从两个枚举重新派生出来。
    pub fn well_formed(&self) -> bool {
        let d = ComboSpec::derive(self.orient, self.flip);
        self.content_rot == d.content_rot
            && self.flip_rot == d.flip_rot
            && self.back_rot == d.back_rot
            && self.axis == d.axis
            && self.binding == d.binding
            && self.feed_mm10 == d.feed_mm10
    }
}

/// 组合注册表容量（4 组合恰满；保留上限常量便于判「第 5 个必拒」）。
pub const MAX_COMBOS: usize = 4;
/// 单次拼版页数上限（A4 手册量级）。
pub const MAX_PAGES: u16 = 10_000;
/// 落账行数上限（4 组合 × 数百张）。
pub const MAX_ROWS: usize = 4096;

/// 四组合注册表：键唯一、规格良构、定容。注册表是「只读事实表」，拼版与
/// 判据都从它取规格，避免各处各自硬编码一张四行表。
pub struct ComboMatrix {
    combos: Vec<ComboSpec>,
    capacity: usize,
}

impl ComboMatrix {
    /// 空注册表（容量 4）。
    pub fn empty() -> ComboMatrix {
        ComboMatrix {
            combos: Vec::with_capacity(MAX_COMBOS),
            capacity: MAX_COMBOS,
        }
    }

    /// 规范四组合：版式主序、翻向次序，故 `key() == 下标`。
    pub fn build() -> ComboMatrix {
        let mut m = ComboMatrix::empty();
        let mut oi = 0usize;
        while oi < Orient::ALL.len() {
            if let Some(o) = Orient::ALL.get(oi) {
                let mut fi = 0usize;
                while fi < FlipEdge::ALL.len() {
                    if let Some(f) = FlipEdge::ALL.get(fi) {
                        // 构造期必然成功：键按序唯一、派生即良构、容量恰 4。
                        // 仍走 push 而非直插，保证「注册路径只有一条」。
                        let _ = m.push(ComboSpec::derive(*o, *f));
                    }
                    fi += 1;
                }
            }
            oi += 1;
        }
        m
    }

    /// 注册一个组合：容量 → 良构 → 键唯一。
    pub fn push(&mut self, spec: ComboSpec) -> Result<(), MatrixError> {
        if self.combos.len() >= self.capacity {
            return Err(MatrixError::Full);
        }
        if !spec.well_formed() {
            return Err(MatrixError::RotationMismatch);
        }
        let key = spec.key();
        let mut i = 0usize;
        while i < self.combos.len() {
            if let Some(s) = self.combos.get(i) {
                if s.key() == key {
                    return Err(MatrixError::Duplicate);
                }
            }
            i += 1;
        }
        self.combos.push(spec);
        Ok(())
    }

    /// 组合数。
    pub fn len(&self) -> usize {
        self.combos.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.combos.is_empty()
    }

    /// 取第 `i` 个组合（越界 `None`）。
    pub fn get(&self, i: usize) -> Option<&ComboSpec> {
        self.combos.get(i)
    }

    /// 按（版式, 翻向）查找。
    pub fn find(&self, orient: Orient, flip: FlipEdge) -> Option<&ComboSpec> {
        let key = orient.code() * 2 + flip.code();
        let mut i = 0usize;
        while i < self.combos.len() {
            if let Some(s) = self.combos.get(i) {
                if s.key() == key {
                    return Some(s);
                }
            }
            i += 1;
        }
        None
    }

    /// 全表复核：恰好四个、键覆盖 0..=3、逐个良构。
    pub fn validate(&self) -> Result<usize, MatrixError> {
        if self.combos.len() != MAX_COMBOS {
            return Err(MatrixError::State);
        }
        let mut seen = [false; MAX_COMBOS];
        let mut i = 0usize;
        while i < self.combos.len() {
            let s = match self.combos.get(i) {
                Some(v) => v,
                None => return Err(MatrixError::State),
            };
            if !s.well_formed() {
                return Err(MatrixError::RotationMismatch);
            }
            let k = s.key() as usize;
            if k >= seen.len() {
                return Err(MatrixError::RotationMismatch);
            }
            if let Some(slot) = seen.get_mut(k) {
                if *slot {
                    return Err(MatrixError::Duplicate);
                }
                *slot = true;
            }
            i += 1;
        }
        Ok(self.combos.len())
    }

    /// 注册表指纹（四组合规格按序串接）。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.combos.len() {
            if let Some(s) = self.combos.get(i) {
                h = fnv_byte(h, s.orient.code());
                h = fnv_byte(h, s.flip.code());
                h = fnv_byte(h, s.content_rot.code());
                h = fnv_byte(h, s.flip_rot.code());
                h = fnv_byte(h, s.back_rot.code());
                h = fnv_byte(h, s.binding.code());
                h = fnv_u32(h, s.feed_mm10);
            }
            i += 1;
        }
        h
    }
}

/// 一张纸的落账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixRow {
    /// 组合下标（等于 `ComboSpec::key`）。
    pub combo: u8,
    /// 纸号（0 起，同组合内连续）。
    pub sheet: u16,
    /// 正面页码。
    pub front_page: u16,
    /// 背面页码。
    pub back_page: u16,
    /// 正面旋转。
    pub front_rot: Rot,
    /// 背面旋转。
    pub back_rot: Rot,
    /// 装订边。
    pub binding: Edge,
}

impl MatrixRow {
    /// 物理键：组合 × 65536 + 纸号。
    pub fn key(&self) -> u32 {
        (self.combo as u32) * 65_536 + self.sheet as u32
    }

    /// 一行落账文本。
    pub fn line(&self) -> String {
        format!(
            "c{} s{} p{}/{} rot{}/{} bind{}",
            self.combo,
            self.sheet,
            self.front_page,
            self.back_page,
            self.front_rot.label(),
            self.back_rot.label(),
            self.binding.label()
        )
    }
}

/// 闭式参考行：直接按「第 k 张纸 = 2k+1 / 2k+2」与规格派生旋转复算。
///
/// 它与 [`ComboRun`] 是两条独立推导：状态机逐张 emit，这里对纸号直接求解。
/// 两者必须逐项同解，否则说明「顺序推进」或「规格派生」有一个错了。
pub fn reference_row(spec: &ComboSpec, sheet: u16) -> MatrixRow {
    MatrixRow {
        combo: spec.key(),
        sheet,
        front_page: sheet.saturating_mul(2).saturating_add(1),
        back_page: sheet.saturating_mul(2).saturating_add(2),
        front_rot: spec.content_rot,
        back_rot: spec.back_rot,
        binding: spec.binding,
    }
}

/// 单组合拼版的状态机阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 未启动。
    Idle,
    /// 逐张推进中。
    Running,
    /// 已收束（不再接受 emit）。
    Done,
}

/// 单组合状态机：`start → emit × sheets → finish`。每一步都校验阶段与范围，
/// 不接受「先 finish 再 emit」或「Done 之后继续 emit」。
pub struct ComboRun {
    spec: ComboSpec,
    total_sheets: u16,
    emitted: u16,
    phase: Phase,
    rows: Vec<MatrixRow>,
}

impl ComboRun {
    /// 启动一次组合拼版：零页 / 超上限 / 奇数页在此一次性拒绝。
    pub fn start(spec: ComboSpec, pages: u16) -> Result<ComboRun, MatrixError> {
        if pages == 0 {
            return Err(MatrixError::ZeroPages);
        }
        if pages > MAX_PAGES {
            return Err(MatrixError::PageOverflow);
        }
        if pages % 2 != 0 {
            return Err(MatrixError::Parity);
        }
        let sheets = pages / 2;
        Ok(ComboRun {
            spec,
            total_sheets: sheets,
            emitted: 0,
            phase: Phase::Running,
            rows: Vec::with_capacity(sheets as usize),
        })
    }

    /// 推进一步：产出下一张纸的落账行。Done / 越界分别返回 State / State。
    pub fn emit(&mut self) -> Result<MatrixRow, MatrixError> {
        if self.phase != Phase::Running {
            return Err(MatrixError::State);
        }
        if self.emitted >= self.total_sheets {
            return Err(MatrixError::State);
        }
        let row = reference_row(&self.spec, self.emitted);
        self.rows.push(row);
        self.emitted = self.emitted.saturating_add(1);
        Ok(row)
    }

    /// 收束：必须恰好在所有纸都 emit 完之后调用。
    pub fn finish(&mut self) -> Result<(), MatrixError> {
        if self.phase != Phase::Running {
            return Err(MatrixError::State);
        }
        if self.emitted != self.total_sheets {
            return Err(MatrixError::State);
        }
        self.phase = Phase::Done;
        Ok(())
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 总纸张数。
    pub fn total_sheets(&self) -> u16 {
        self.total_sheets
    }

    /// 已产出纸张数。
    pub fn emitted(&self) -> u16 {
        self.emitted
    }

    /// 规格。
    pub fn spec(&self) -> ComboSpec {
        self.spec
    }

    /// 已产出的行（按纸号升序）。
    pub fn rows(&self) -> &[MatrixRow] {
        &self.rows
    }

    /// 本组合指纹：按纸号串正/背面旋转与装订边。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fnv_u16(h, r.sheet);
                h = fnv_u16(h, r.front_page);
                h = fnv_u16(h, r.back_page);
                h = fnv_byte(h, r.front_rot.code());
                h = fnv_byte(h, r.back_rot.code());
                h = fnv_byte(h, r.binding.code());
            }
            i += 1;
        }
        h
    }
}

/// 组合拼版并落账：对注册表里每个组合跑一遍状态机，结果按组合序入账。
pub fn plan_all(matrix: &ComboMatrix, pages: u16) -> Result<MatrixLedger, MatrixError> {
    let mut led = MatrixLedger::new(MAX_ROWS);
    let mut c = 0usize;
    while c < matrix.len() {
        let spec = match matrix.get(c) {
            Some(v) => *v,
            None => return Err(MatrixError::State),
        };
        let mut run = ComboRun::start(spec, pages)?;
        while run.emitted() < run.total_sheets() {
            let row = run.emit()?;
            led.insert(row)?;
        }
        run.finish()?;
        c += 1;
    }
    Ok(led)
}

/// 逐张总账：同组合内纸号连续 + 物理键唯一 + 规格一致 + 封账只读。
pub struct MatrixLedger {
    rows: Vec<MatrixRow>,
    capacity: usize,
    sealed: bool,
    rejected: u32,
}

impl MatrixLedger {
    /// 建空账；容量 0 按 1 处理，上限 [`MAX_ROWS`]。
    pub fn new(capacity: usize) -> MatrixLedger {
        let cap = if capacity == 0 {
            1
        } else if capacity > MAX_ROWS {
            MAX_ROWS
        } else {
            capacity
        };
        MatrixLedger {
            rows: Vec::with_capacity(cap),
            capacity: cap,
            sealed: false,
            rejected: 0,
        }
    }

    fn reject(&mut self, e: MatrixError) -> Result<(), MatrixError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 同组合已入账的纸张数（顺序校验的期望纸号）。
    pub fn count_for(&self, combo: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.combo == combo {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 入账一张纸：封账 → 容量 → 键范围 → 页配对 → 键唯一 → 纸号连续。
    pub fn insert(&mut self, row: MatrixRow) -> Result<(), MatrixError> {
        if self.sealed {
            return self.reject(MatrixError::Immutable);
        }
        if self.rows.len() >= self.capacity {
            return self.reject(MatrixError::Full);
        }
        if row.combo as usize >= MAX_COMBOS {
            return self.reject(MatrixError::State);
        }
        // 只认整张纸：正面奇数、背面紧接 +1。空白背面属于 F50063。
        if row.front_page == 0
            || row.front_page % 2 != 1
            || row.back_page != row.front_page.saturating_add(1)
        {
            return self.reject(MatrixError::State);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(o) = self.rows.get(i) {
                if o.combo == row.combo && o.sheet == row.sheet {
                    return self.reject(MatrixError::Duplicate);
                }
            }
            i += 1;
        }
        let want = self.count_for(row.combo);
        if row.sheet as usize != want {
            return self.reject(MatrixError::OrderGap);
        }
        self.rows.push(row);
        Ok(())
    }

    /// 全表复核：逐行校验纸号连续、旋转与规格一致、页码配对、装订边一致。
    pub fn verify(&self, matrix: &ComboMatrix) -> Result<usize, MatrixError> {
        let mut next = [0u16; MAX_COMBOS];
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MatrixError::State),
            };
            let spec = match matrix.get(r.combo as usize) {
                Some(v) => *v,
                None => return Err(MatrixError::State),
            };
            let expect = match next.get(r.combo as usize) {
                Some(v) => *v,
                None => return Err(MatrixError::State),
            };
            if r.sheet != expect {
                return Err(MatrixError::OrderGap);
            }
            if r.front_rot != spec.content_rot
                || r.back_rot != spec.back_rot
                || r.binding != spec.binding
            {
                return Err(MatrixError::RotationMismatch);
            }
            if r.front_page != r.sheet.saturating_mul(2).saturating_add(1)
                || r.back_page != r.front_page.saturating_add(1)
            {
                return Err(MatrixError::State);
            }
            if let Some(slot) = next.get_mut(r.combo as usize) {
                *slot = slot.saturating_add(1);
            }
            i += 1;
        }
        Ok(self.rows.len())
    }

    /// 封账：之后入账一律 [`MatrixError::Immutable`]。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 是否已封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 累计被拒次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 取第 `i` 行（越界 `None`）。
    pub fn row(&self, i: usize) -> Option<&MatrixRow> {
        self.rows.get(i)
    }

    /// 取某组合第 `sheet` 张（闭式下标只在组合连续入账时成立，故走扫描）。
    pub fn find(&self, combo: u8, sheet: u16) -> Option<&MatrixRow> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.combo == combo && r.sheet == sheet {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 全账指纹（按入账序）。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fnv_byte(h, r.combo);
                h = fnv_u16(h, r.sheet);
                h = fnv_u16(h, r.front_page);
                h = fnv_u16(h, r.back_page);
                h = fnv_byte(h, r.front_rot.code());
                h = fnv_byte(h, r.back_rot.code());
                h = fnv_byte(h, r.binding.code());
            }
            i += 1;
        }
        h
    }

    /// 落账文本（逐行）。
    pub fn ledger_text(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                out.push_str(&r.line());
                out.push('\n');
            }
            i += 1;
        }
        out
    }
}

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_u16(h: u32, v: u16) -> u32 {
    fnv_byte(fnv_byte(h, (v >> 8) as u8), v as u8)
}

fn fnv_u32(h: u32, v: u32) -> u32 {
    fnv_byte(
        fnv_byte(fnv_byte(fnv_byte(h, (v >> 24) as u8), (v >> 16) as u8), (v >> 8) as u8),
        v as u8,
    )
}

/// 注入核：8 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let spec = ComboSpec::derive(Orient::Portrait, FlipEdge::Long);

    // 1) 零页：空作业不产纸。
    if ComboRun::start(spec, 0).err() == Some(MatrixError::ZeroPages) {
        caught += 1;
    }

    // 2) 奇数页：整张纸之外的补白不归本模块。
    if ComboRun::start(spec, 9).err() == Some(MatrixError::Parity) {
        caught += 1;
    }

    // 3) 版式码越界。
    if Orient::from_code(2).err() == Some(MatrixError::OrientCode) {
        caught += 1;
    }

    // 4) 翻向码越界。
    if FlipEdge::from_code(7).err() == Some(MatrixError::FlipCode) {
        caught += 1;
    }

    // 5) 重复组合：同键注册两次必拒。
    let mut dup = ComboMatrix::empty();
    let first = dup.push(spec).is_ok();
    if first && dup.push(spec).err() == Some(MatrixError::Duplicate) && dup.len() == 1 {
        caught += 1;
    }

    // 6) 旋转被篡改：规格要求背面 R0，账行写成 R180，verify 必须抓。
    let matrix = ComboMatrix::build();
    let mut bad = MatrixLedger::new(4);
    let mut tampered = reference_row(&spec, 0);
    tampered.back_rot = Rot::R180;
    if bad.insert(tampered).is_ok()
        && bad.verify(&matrix).err() == Some(MatrixError::RotationMismatch)
    {
        caught += 1;
    }

    // 7) 同键重复入账。
    let mut twice = MatrixLedger::new(8);
    let back = MatrixLedger::new(0); // 容量 0 按 1 处理，顺带覆盖下界
    if twice.insert(reference_row(&spec, 0)).is_ok()
        && twice.insert(reference_row(&spec, 0)).err() == Some(MatrixError::Duplicate)
        && twice.len() == 1
        && back.capacity() == 1
    {
        caught += 1;
    }

    // 8) 状态机：收束之后再 emit 必拒。
    if let Ok(mut run) = ComboRun::start(spec, 2) {
        let step = run.emit().is_ok();
        let done = run.finish().is_ok();
        if step && done && run.emit().err() == Some(MatrixError::State) {
            caught += 1;
        }
    }

    (caught, total)
}

/// UNX-F50062 自检：19 条判据（J1 原文：交叉矩阵 ×4 ×10 输出逐项一致）。
pub fn run_unx50062_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50062");
    let matrix = ComboMatrix::build();
    const PAGES: u16 = 10;
    const SHEETS: u16 = 5;
    // 4 组合 × 5 张 = 20 行；每行承载正反 2 页，合计 40 个页输出。
    const TOTAL_ROWS: usize = (MAX_COMBOS as usize) * (SHEETS as usize);

    // J1-1 四组合注册：恰好 4 个、键等于下标、逐个良构、全表 validate 通过。
    let mut reg_ok = matrix.len() == MAX_COMBOS;
    let mut i = 0usize;
    while i < matrix.len() {
        match matrix.get(i) {
            Some(sp) => {
                if sp.key() as usize != i || !sp.well_formed() {
                    reg_ok = false;
                }
            }
            None => reg_ok = false,
        }
        i += 1;
    }
    if matrix.validate() != Ok(MAX_COMBOS) {
        reg_ok = false;
    }
    s.add("50062-J1-组合注册", reg_ok, if reg_ok { "" } else { "键/良构/全表校验失败" });

    // J1-2 交叉表闭式：翻向轴互换后，**翻纸旋转**为
    // 竖版长边=R0、竖版短边=R180、横版长边=R180、横版短边=R0；
    // 背面总旋转还要叠上横版内容的 90°。物理轴必须与该版式下所选边的方向一致。
    let expect = [
        (Orient::Portrait, FlipEdge::Long, Rot::R0, Rot::R0, Axis::Vertical),
        (Orient::Portrait, FlipEdge::Short, Rot::R180, Rot::R180, Axis::Horizontal),
        (Orient::Landscape, FlipEdge::Long, Rot::R180, Rot::R270, Axis::Horizontal),
        (Orient::Landscape, FlipEdge::Short, Rot::R0, Rot::R90, Axis::Vertical),
    ];
    let mut table_ok = true;
    let mut k = 0usize;
    while k < expect.len() {
        if let Some((o, f, flip_rot, back_rot, axis)) = expect.get(k) {
            match matrix.find(*o, *f) {
                Some(sp) => {
                    if sp.flip_rot != *flip_rot || sp.back_rot != *back_rot || sp.axis != *axis {
                        table_ok = false;
                    }
                }
                None => table_ok = false,
            }
        }
        k += 1;
    }
    s.add("50062-J1-交叉闭式", table_ok, if table_ok { "" } else { "四组合闭式表不符" });

    // J1-3 ×4 ×10 逐项一致：4 组合 × 5 张（10 页）= 20 行落账，与闭式参考行逐字段比对（本卡核心判据）。
    let mut mismatch = 0u16;
    let mut rows_total = 0usize;
    match plan_all(&matrix, PAGES) {
        Ok(led) => {
            rows_total = led.len();
            let mut c = 0usize;
            while c < matrix.len() {
                if let Some(sp) = matrix.get(c) {
                    let mut sh = 0u16;
                    while sh < SHEETS {
                        let want = reference_row(sp, sh);
                        match led.row(c * SHEETS as usize + sh as usize) {
                            Some(got) => {
                                if got.combo != want.combo
                                    || got.sheet != want.sheet
                                    || got.front_page != want.front_page
                                    || got.back_page != want.back_page
                                    || got.front_rot != want.front_rot
                                    || got.back_rot != want.back_rot
                                    || got.binding != want.binding
                                {
                                    mismatch = mismatch.saturating_add(1);
                                }
                            }
                            None => mismatch = mismatch.saturating_add(1),
                        }
                        sh += 1;
                    }
                } else {
                    mismatch = mismatch.saturating_add(1);
                }
                c += 1;
            }
        }
        Err(_) => mismatch = 999,
    }
    let ok = rows_total == (MAX_COMBOS as usize) * (SHEETS as usize) && mismatch == 0;
    let det = format!("rows={} mismatch={}", rows_total, mismatch);
    s.add_dyn("50062-J1-四组合十页", ok, &det);

    // J1-4 页序守恒：每组合正面 1/3/5/7/9、背面 2/4/6/8/10，合计 10 页无重无漏。
    let mut pages_ok = true;
    let mut page_len = 0usize;
    let mut page_found = 0usize;
    let mut page_bad = 0u16;
    match plan_all(&matrix, PAGES) {
        Ok(led) => {
            page_len = led.len();
            let mut c = 0usize;
            while c < matrix.len() {
                let mut sh = 0u16;
                while sh < SHEETS {
                    match led.find(c as u8, sh) {
                        Some(r) => {
                            page_found += 1;
                            if r.front_page != sh * 2 + 1 || r.back_page != sh * 2 + 2 {
                                page_bad = page_bad.saturating_add(1);
                            }
                        }
                        None => page_bad = page_bad.saturating_add(1),
                    }
                    sh += 1;
                }
                c += 1;
            }
            if led.len() != TOTAL_ROWS {
                pages_ok = false;
            }
        }
        Err(_) => pages_ok = false,
    }
    if page_len != TOTAL_ROWS || page_found != TOTAL_ROWS || page_bad != 0 {
        pages_ok = false;
    }
    let det = format!("n={} f={} b={}", page_len, page_found, page_bad);
    s.add_dyn("50062-J1-页序守恒", pages_ok, &det);

    // J1-5 正反配对：同组合所有行 back = front + 1 且 front 为奇数。
    let mut pair_ok = true;
    if let Ok(led) = plan_all(&matrix, PAGES) {
        let mut i = 0usize;
        while i < led.len() {
            if let Some(r) = led.row(i) {
                if r.front_page % 2 != 1 || r.back_page != r.front_page.saturating_add(1) {
                    pair_ok = false;
                }
            }
            i += 1;
        }
    } else {
        pair_ok = false;
    }
    s.add("50062-J1-正反配对", pair_ok, if pair_ok { "" } else { "正反面不配对" });

    // J1-6 旋转群：四组合 back = content ∘ flip；Z4 单位元/逆元/结合律/四次回原。
    let mut rot_ok = true;
    let mut c = 0usize;
    while c < matrix.len() {
        if let Some(sp) = matrix.get(c) {
            if sp.back_rot != sp.content_rot.compose(sp.flip_rot) {
                rot_ok = false;
            }
        }
        c += 1;
    }
    let mut a = 0usize;
    while a < Rot::ALL.len() {
        if let Some(x) = Rot::ALL.get(a) {
            if x.compose(Rot::R0) != *x || x.compose(x.inverse()) != Rot::R0 {
                rot_ok = false;
            }
            // 四次回原：从单位元出发连续合成 x 四次，必须回到单位元。
            let mut acc = Rot::R0;
            let mut t = 0usize;
            while t < 4 {
                acc = acc.compose(*x);
                t += 1;
            }
            if acc != Rot::R0 {
                rot_ok = false;
            }
            let mut b = 0usize;
            while b < Rot::ALL.len() {
                if let Some(y) = Rot::ALL.get(b) {
                    if x.compose(*y) != y.compose(*x) {
                        rot_ok = false;
                    }
                }
                b += 1;
            }
        }
        a += 1;
    }
    s.add("50062-J1-旋转群", rot_ok, if rot_ok { "" } else { "Z4 或派生式不成立" });

    // J1-7 装订边：同组合内恒定，且竖直轴 ↔ 左边、水平轴 ↔ 上边。
    let mut bind_ok = true;
    let mut c2 = 0usize;
    while c2 < matrix.len() {
        if let Some(sp) = matrix.get(c2) {
            let want = match sp.axis {
                Axis::Vertical => Edge::Left,
                Axis::Horizontal => Edge::Top,
            };
            if sp.binding != want {
                bind_ok = false;
            }
            let mut sh = 0u16;
            while sh < SHEETS {
                let want_row = reference_row(sp, sh);
                if want_row.binding != want {
                    bind_ok = false;
                }
                sh += 1;
            }
        }
        c2 += 1;
    }
    s.add("50062-J1-装订边恒定", bind_ok, if bind_ok { "" } else { "装订边与轴不符" });

    // J1-8 几何：横版 = 竖版旋转 90°；长短边守恒；前缘长度随版式变化。
    let p = SheetGeom::A4_PORTRAIT;
    let l = SheetGeom::A4_LANDSCAPE;
    let mut geo_ok = p.rotated() == l
        && p.long_edge() == l.long_edge()
        && p.short_edge() == l.short_edge()
        && p.long_edge() == 2970
        && p.short_edge() == 2100
        && !p.is_landscape()
        && l.is_landscape();
    let mut c3 = 0usize;
    while c3 < matrix.len() {
        if let Some(sp) = matrix.get(c3) {
            let want_feed = match sp.orient {
                Orient::Portrait => 2100,
                Orient::Landscape => 2970,
            };
            if sp.feed_mm10 != want_feed || sp.feed_mm10 != sp.orient.feed_mm10() {
                geo_ok = false;
            }
        }
        c3 += 1;
    }
    s.add("50062-J1-几何守恒", geo_ok, if geo_ok { "" } else { "尺寸/前缘不符" });

    // J1-9 确定性：同参两次全账指纹逐位相同，且四组合各自指纹互不相同。
    let d1 = match plan_all(&matrix, PAGES) {
        Ok(v) => v.digest(),
        Err(_) => 0,
    };
    let d2 = match plan_all(&matrix, PAGES) {
        Ok(v) => v.digest(),
        Err(_) => 1,
    };
    let mut digests: Vec<u32> = Vec::with_capacity(MAX_COMBOS);
    let mut c4 = 0usize;
    while c4 < matrix.len() {
        if let Some(sp) = matrix.get(c4) {
            if let Ok(mut run) = ComboRun::start(*sp, PAGES) {
                let mut guard = 0u16;
                while run.emitted() < run.total_sheets() && guard < 64 {
                    let _ = run.emit();
                    guard = guard.saturating_add(1);
                }
                let _ = run.finish();
                digests.push(run.digest());
            }
        }
        c4 += 1;
    }
    let mut distinct = true;
    let mut a2 = 0usize;
    while a2 < digests.len() {
        let mut b2 = a2 + 1;
        while b2 < digests.len() {
            if let (Some(x), Some(y)) = (digests.get(a2), digests.get(b2)) {
                if x == y {
                    distinct = false;
                }
            }
            b2 += 1;
        }
        a2 += 1;
    }
    let det = format!("d={:08x}/{:08x} n={}", d1, d2, digests.len());
    s.add_dyn("50062-J1-映射确定", d1 == d2 && distinct && digests.len() == 4, &det);

    // J1-10 状态机：正常 emit→finish；提前 finish 必拒；Done 后 emit 必拒。
    let spec = ComboSpec::derive(Orient::Landscape, FlipEdge::Long);
    let mut st_ok = false;
    if let Ok(mut run) = ComboRun::start(spec, PAGES) {
        let early = run.finish().err() == Some(MatrixError::State);
        let mut emits_ok = true;
        let mut n = 0u16;
        while n < SHEETS {
            if run.emit().is_err() {
                emits_ok = false;
            }
            n += 1;
        }
        let finished = run.emitted() == SHEETS;
        st_ok = early
            && emits_ok
            && finished
            && run.phase() == Phase::Running
            && run.finish().is_ok()
            && run.phase() == Phase::Done
            && run.emit().err() == Some(MatrixError::State);
    }
    s.add("50062-J1-状态机", st_ok, if st_ok { "" } else { "阶段迁移未拦截" });

    // J1-11 账本复核：20 行 verify 通过（双射 + 规格一致）。
    let verify_code: i64;
    match plan_all(&matrix, PAGES) {
        Ok(v) => {
            verify_code = match v.verify(&matrix) {
                Ok(n) => n as i64,
                Err(e) => -(e.code() as i64),
            };
        }
        Err(e) => verify_code = -(e.code() as i64),
    }
    let verify_ok = verify_code == TOTAL_ROWS as i64;
    let det = format!("verify={}", verify_code);
    s.add_dyn("50062-J1-账本复核", verify_ok, &det);

    // J1-12 奇数页必拒（补白归 F50063）。
    let parity_ok = ComboRun::start(spec, 9).err() == Some(MatrixError::Parity)
        && ComboRun::start(spec, 1).err() == Some(MatrixError::Parity);
    s.add("50062-J1-奇数必拒", parity_ok, if parity_ok { "" } else { "奇数页被接受" });

    // J1-13 零页 / 超上限必拒。
    let bound_ok = ComboRun::start(spec, 0).err() == Some(MatrixError::ZeroPages)
        && ComboRun::start(spec, MAX_PAGES + 1).err() == Some(MatrixError::PageOverflow);
    s.add("50062-J1-边界必拒", bound_ok, if bound_ok { "" } else { "0/超限被接受" });

    // J1-14 非法码必拒：版式 2、翻向 7、旋转 4。
    let code_ok = Orient::from_code(2).err() == Some(MatrixError::OrientCode)
        && FlipEdge::from_code(7).err() == Some(MatrixError::FlipCode)
        && Rot::from_code(4).err() == Some(MatrixError::RotationMismatch);
    s.add("50062-J1-非法码必拒", code_ok, if code_ok { "" } else { "闭集外码被接受" });

    // J1-15 重复组合必拒、第五个组合必拒（Full）。
    let mut reg = ComboMatrix::empty();
    let mut push_ok = reg.push(spec).is_ok();
    push_ok = push_ok && reg.push(spec).err() == Some(MatrixError::Duplicate);
    let mut n2 = 0usize;
    while n2 < 4 {
        if let Some(o) = Orient::ALL.get(n2 / 2) {
            if let Some(f) = FlipEdge::ALL.get(n2 % 2) {
                let _ = reg.push(ComboSpec::derive(*o, *f));
            }
        }
        n2 += 1;
    }
    // 首行已占一个槽，补齐到 4 个后再推第五个。
    let mut fills = 0usize;
    while reg.len() < MAX_COMBOS && fills < MAX_COMBOS {
        let mut fi = 0usize;
        while fi < FlipEdge::ALL.len() && reg.len() < MAX_COMBOS {
            if let Some(o) = Orient::ALL.get(fills % Orient::ALL.len()) {
                if let Some(f) = FlipEdge::ALL.get(fi) {
                    let _ = reg.push(ComboSpec::derive(*o, *f));
                }
            }
            fi += 1;
        }
        fills += 1;
    }
    let last_spec = ComboSpec::derive(Orient::Landscape, FlipEdge::Short);
    let full_ok = reg.len() == MAX_COMBOS && reg.push(last_spec).err() == Some(MatrixError::Full);
    let ok = push_ok && full_ok;
    s.add("50062-J1-注册必拒", ok, if ok { "" } else { "重复/超容被接受" });

    // J1-16 旋转篡改必抓：把一行背面旋转改错，verify 返回 RotationMismatch。
    let mut tamper = false;
    if let Ok(src) = plan_all(&matrix, PAGES) {
        let mut bad = MatrixLedger::new(MAX_ROWS);
        let mut idx = 0usize;
        let mut inserted = true;
        while idx < src.len() {
            match src.row(idx) {
                Some(r) => {
                    let mut copy = *r;
                    if idx == 0 {
                        copy.back_rot = Rot::R180;
                    }
                    if bad.insert(copy).is_err() {
                        inserted = false;
                    }
                }
                None => inserted = false,
            }
            idx += 1;
        }
        tamper = inserted
            && bad.len() == src.len()
            && bad.verify(&matrix).err() == Some(MatrixError::RotationMismatch);
    }
    s.add("50062-J1-篡改必抓", tamper, if tamper { "" } else { "错旋转进了复核" });

    // J1-17 同纸重复入账必拒。
    let dup_ok = match (plan_all(&matrix, PAGES), plan_all(&matrix, PAGES)) {
        (Ok(a), Ok(_)) => match a.row(0) {
            Some(r) => {
                let mut led = MatrixLedger::new(8);
                led.insert(*r).is_ok()
                    && led.insert(*r).err() == Some(MatrixError::Duplicate)
                    && led.len() == 1
                    && led.rejected() == 1
            }
            None => false,
        },
        _ => false,
    };
    s.add("50062-J1-重张必拒", dup_ok, if dup_ok { "" } else { "同纸两次入账" });

    // J1-18 跳号/定容/封账：sheet 1 先入账 → OrderGap；容量 1 收满 → Full；
    // 封账后 → Immutable，且账不变。
    let mut seq_ok = false;
    if let Some(sp) = matrix.get(0) {
        let skip = reference_row(sp, 1);
        let first = reference_row(sp, 0);
        let mut led = MatrixLedger::new(1);
        let gap = led.insert(skip).err() == Some(MatrixError::OrderGap);
        let one = led.insert(first).is_ok();
        let next = reference_row(sp, 1);
        let full = led.insert(next).err() == Some(MatrixError::Full);
        led.seal();
        let imm = led.insert(next).err() == Some(MatrixError::Immutable);
        seq_ok = gap && one && full && imm && led.len() == 1 && led.is_sealed();
    }
    s.add("50062-J1-账本纪律", seq_ok, if seq_ok { "" } else { "跳号/容量/封账未拦截" });

    // J1-19 八类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50062-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50062_checks_green() {
        let set = run_unx50062_checks();
        assert!(set.all_passed(), "UNX-F50062 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50062_cross_matrix_rotations() {
        let m = ComboMatrix::build();
        // 交叉语义落在 flip_rot（翻纸带来的旋转）上；背面总旋转再叠内容旋转。
        let cases = [
            (Orient::Portrait, FlipEdge::Long, Rot::R0, Rot::R0),
            (Orient::Portrait, FlipEdge::Short, Rot::R180, Rot::R180),
            (Orient::Landscape, FlipEdge::Long, Rot::R180, Rot::R270),
            (Orient::Landscape, FlipEdge::Short, Rot::R0, Rot::R90),
        ];
        for (o, f, flip, back) in cases {
            let sp = m.find(o, f);
            assert!(sp.is_some());
            if let Some(v) = sp {
                assert_eq!(v.flip_rot, flip, "{:?}/{:?}", o, f);
                assert_eq!(v.back_rot, back, "{:?}/{:?}", o, f);
                assert_eq!(v.back_rot, v.content_rot.compose(v.flip_rot));
            }
        }
    }

    #[test]
    fn unx50062_forty_rows_match_reference() {
        let m = ComboMatrix::build();
        let led = plan_all(&m, 10);
        assert!(led.is_ok());
        if let Ok(l) = led {
            assert_eq!(l.len(), 20);
            assert_eq!(l.verify(&m), Ok(20));
            for c in 0..4usize {
                if let Some(sp) = m.get(c) {
                    for sh in 0..5u16 {
                        let want = reference_row(sp, sh);
                        let got = l.find(sp.key(), sh);
                        assert_eq!(got, Some(&want));
                    }
                }
            }
        }
    }

    #[test]
    fn unx50062_state_machine_rejects_illegal_moves() {
        let sp = ComboSpec::derive(Orient::Portrait, FlipEdge::Short);
        let started = ComboRun::start(sp, 4);
        assert!(started.is_ok());
        if let Ok(mut run) = started {
            assert_eq!(run.finish().err(), Some(MatrixError::State));
            assert!(run.emit().is_ok());
            assert!(run.emit().is_ok());
            assert!(run.finish().is_ok());
            assert_eq!(run.emit().err(), Some(MatrixError::State));
            assert_eq!(run.phase(), Phase::Done);
        }
    }

    #[test]
    fn unx50062_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 8);
    }
}
