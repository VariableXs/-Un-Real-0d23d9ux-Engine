//! VE-F0022 · 顶点输入布局描述器（VE-A 域 · 合成核心 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0022`
//!
//! **判据（锚点原文）**：顶点属性布局的声明式描述（位置/法线/UV/骨骼权重等属性的
//! 槽位/格式/步进），布局与着色器签名的一致性校验（错位在编译期抓而不是运行期黑屏）；
//! 含布局的国际化属性说明（顶点数据不含文本内容）。判据五条：**声明式布局、签名校验、
//! 编译期抓错、去重、判据**。
//!
//! **错误路径与降级矩阵**：签名错位→编译期拒绝；对齐违例→修正；布局重复→去重。
//!
//! **数据结构**：布局描述；一致性校验。
//!
//! **性能逐项分解**：O(布局)——布局面校验、签名面比对、去重线性扫描、位置唯一性两两
//! 比对，四处都是 O(属性数) 或 O(属性数²)，均以属性数上限 [`MAX_ATTRIBUTES`] 为界。
//!
//! **跨批对接点**：A27 绑定编译联动——顶点输入位置号与绑定表槽位**共享同一个编号空间**
//! （[`LocationSpace`]）。本条只负责顶点侧分配与冲突检出，绑定侧由 F0027 消费。
//!
//! **无障碍与隐私**：布局预览读屏可达（[`VertexLayout::a11y_preview`]）——每个属性一行，
//! 含槽位号、语义、格式、偏移、步进；国际化说明走 [`describe_in`]。**顶点二进制缓冲
//! 本身不含任何文本**（[`NO_TEXT_IN_BUFFER_DOC`]）：换语言不改变顶点数据一个字节。
//!
//! ## 设计要点
//!
//! - **声明式布局**（[`VertexLayoutBuilder`]）：调用方只说「有哪些属性、各是什么格式、
//!   各在哪个槽位」，**不说偏移**。偏移与步进由构建器按各格式的对齐要求推导——没有
//!   手写偏移的入口，手写错位这一类缺陷在本条里**不可能发生**。
//!
//!   顺带一个诚实的推论：本模块的八个格式字节宽全是 4 的倍数、单分量对齐要求
//!   （1/2/4）又都整除 4，故**声明式路径永不产生填充字节**。这不是缺陷，是选型的
//!   结果；它同时说明「对齐违例→修正」这一路服务的是**外部导入/手写布局**
//!   （[`correct_alignments`]），不是声明式构建器。
//! - **编译期抓错**（[`verify_signature_const`]）：规格要求「错位在编译期抓而不是运行期
//!   黑屏」。Rust 里的编译期就是 `const` 求值，故本条提供 `const fn` 校验器，并以
//!   [`ASSERT_LAYOUT_OK`] / [`ASSERT_BROKEN_REJECTED`] 两条 `const _: () = assert!(...)`
//!   把闸门**真的钉在编译期**。
//!
//!   关键设计：闸门必须成对。只写正例的闸门只能证明「没坏」，不能证明「在查」——
//!   [`ASSERT_BROKEN_REJECTED`] 断言**五类错位布局必须全被拒**，校验器一旦退化成
//!   「永远通过」，这条 `const` 立刻编译失败。
//! - **三向处置互不共用**（[`SignatureVerdict`] / [`AlignVerdict`] / [`LayoutVerdict`]）：
//!   规格给了三条方向相反的处置——签名错位**拒绝**、对齐违例**修正**、布局重复**去重**。
//!   三者语义相反，**不得共用结果类型、也不得共用错误码**（共用即「处置方向相反的状态
//!   共用码」，是本仓明令禁止的缺陷形态）。故拆成三个枚举 + 三个码常量。
//! - **对齐违例修正而非拒绝**（[`correct_alignments`]）：偏移没对齐到格式要求时**按格式
//!   要求重新推导偏移**并如实报告改了什么。偏移歪了是笔误，签名错了是设计错——笔误
//!   自动修，设计错让人回去改。修正**只动偏移与步进，绝不动格式与语义**：改格式等于
//!   伪造调用方的声明。
//!
//!   修正**修不了**时（重推后步进超预算）升级为 [`AlignVerdict::Unfixable`]，不硬掰。
//! - **去重按规范形而非按声明序**（[`VertexLayout::canonical_form`]）：属性书写顺序不同
//!   的同两个布局**必须**判为同一个；但**步进不同、偏移不同的一定不是同一个**——
//!   步进与偏移直接决定缓冲内寻址，只按属性集合合并会把不同寻址的布局悄悄并成一个，
//!   那比不去重更坏。
//! - **国际化说明与二进制布局解耦**：属性的人类可读说明（[`describe_in`]）是**元数据**，
//!   顶点缓冲里一个文本字节都没有。判据用「缓冲字节数 = 各格式字节宽之和」这个
//!   **可失败**的算术钉住，而不是靠注释声明。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 顶点格式全集规模。
pub const FORMAT_TABLE_SIZE: usize = 8;

/// 顶点语义全集规模。
pub const SEMANTIC_TABLE_SIZE: usize = 9;

/// 单布局属性数上限（超出即拒绝——布局膨胀会让驱动侧描述符超限）。
pub const MAX_ATTRIBUTES: usize = 16;

/// 步进（stride）字节对齐粒度。
pub const STRIDE_ALIGN: u32 = 4;

/// 步进字节预算上限（超出的布局无法靠修正救回，只能拒绝）。
pub const MAX_STRIDE: u32 = 128;

/// 位置号上限（位置空间容量，见 [`LocationSpace`]）。
pub const MAX_LOCATION: u8 = 15;

/// 拒绝类诊断码（签名错位 / 布局不自洽）。
pub const E_LAYOUT_REJECT: &str = "E_VERTEX_LAYOUT_REJECT";

/// 对齐修正类诊断码（与拒绝码**分开**——处置方向相反，不共用码）。
pub const E_LAYOUT_ALIGN_FIXED: &str = "E_VERTEX_LAYOUT_ALIGN_FIXED";

/// 去重类诊断码（既不是拒绝也不是修正）。
pub const E_LAYOUT_DEDUPED: &str = "E_VERTEX_LAYOUT_DEDUPED";

/// 声明式布局契约。
pub const DECLARATIVE_DOC: &str = "\
声明式布局契约（VE-F0022 · v1）：调用方只声明「有哪些属性、各是什么格式、各在哪个槽位」，\
**不声明偏移**。偏移与步进由构建器按各格式的对齐要求推导，步进再对齐到 STRIDE_ALIGN。\
故「手写偏移写歪」这一类错位在本条里不可能发生——没有手写入口。属性数超MAX_ATTRIBUTES、\
位置号越界/撞号、或步进超 MAX_STRIDE 时**明确拒绝并给出原因**，不静默截断\
（截断会让上层以为全部属性都进了 GPU 描述符）。";

/// 编译期抓错契约。
pub const COMPILE_TIME_DOC: &str = "\
编译期抓错契约（VE-F0022 · v1）：布局与着色器签名的一致性由 `const fn` 校验器判定，并以\
`const _: () = assert!(...)` 钉在编译期——错位在**编译时**就是编译错误，不是运行期黑屏。\
闸门**成对**出现：`ASSERT_LAYOUT_OK` 断言正确布局必须通过（过严也是缺陷），\
`ASSERT_BROKEN_REJECTED` 断言**五类错位布局必须全被拒**。后者是恒真门禁的照妖镜——\
校验器一旦退化成「永远通过」，这条 const 立刻编译失败。运行期校验器 [`verify_signature`]\
与 const 校验器**共用同一套判据**，二者结论一致性由 `A22-sig-运行期与编译期同判` 钉住，\
防止两套判据各写各的（编译期说通过、运行期说拒绝）。";

/// 对齐修正契约。
pub const ALIGN_FIX_DOC: &str = "\
对齐修正契约（VE-F0022 · v1）：属性偏移未对齐到格式要求时，**按各格式的对齐要求重新推导\
整份偏移**并如实报告每一条改了什么（改前偏移、改后偏移、要求对齐）。偏移歪了是笔误，\
签名错了是设计错——**笔误自动修，设计错让人回去改**，故对齐违例走修正而非拒绝。修正\
**只动偏移与步进，绝不动格式与语义**（改格式等于伪造调用方的声明）。重推后步进超\
MAX_STRIDE 时**修不了**，升级为 Unfixable，不硬掰。";

/// 去重契约。
pub const DEDUP_DOC: &str = "\
去重契约（VE-F0022 · v1）：重复布局按**规范形**（属性按槽位号排序后的稳定表示）去重，\
**与声明书写顺序无关**——同组属性换个顺序写，判为同一个布局。但**步进不同、偏移不同的\
一定不是同一个**：步进与偏移直接决定顶点缓冲内的元素寻址，只按属性集合合并会把不同\
寻址的布局悄悄并成一个，那比不去重更坏。去重只省描述符，**顶点数据一个字节都不动**。";

/// 顶点缓冲无文本契约。
pub const NO_TEXT_IN_BUFFER_DOC: &str = "\
顶点缓冲无文本契约（VE-F0022 · v1）：顶点二进制缓冲**只含数值属性，不含任何文本内容**。\
属性的人类可读说明（语义名/描述/国际化文案）是**元数据**，与缓冲字节无关：切换语言后\
布局的规范形、步进、逐属性偏移、缓冲总字节数**逐项相同**。缓冲字节数恒等于各属性格式\
字节宽之和（再加步进对齐的填充），**与语义名、语言、说明文字的长度都无关**。\
故「国际化属性说明」是布局的旁挂描述面，不是往顶点数据里塞字符串——后者会让顶点格式\
随语言变化，无法与 GPU 对接。";

/// 跨批对接契约（A27 绑定编译联动）。
pub const A27_LINK_DOC: &str = "\
跨批对接契约（VE-F0022 · v1 · 对接 A27=F0027 绑定布局编译器）：顶点输入位置号与绑定表\
槽位**共享同一个编号空间**（[`LocationSpace`]），两侧各自申请、互相可见。共享空间意味着\
两侧**不能各自从 0 开始数**——那会在跨阶段管线里静默撞号。故本条把「顶点位置号不得\
越过绑定侧已占用区」做成机检并留审计，而不是靠两侧作者互相通气。顶点侧只管顶点属性\
的位置分配；描述符槽位、SRV/采样器视图由 F0027 消费同一空间。";

// ---------------------------------------------------------------------------
// 二、标量种类与顶点格式
// ---------------------------------------------------------------------------

/// 标量种类（格式与着色器输入的**类别**对齐面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarKind {
    /// 浮点。
    Float,
    /// 有符号整数。
    SignedInt,
    /// 无符号整数。
    UnsignedInt,
    /// 归一化整数（读入后由硬件映射到 [0,1] 或 [-1,1]）。
    Normalized,
}

impl ScalarKind {
    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            ScalarKind::Float => "float",
            ScalarKind::SignedInt => "sint",
            ScalarKind::UnsignedInt => "uint",
            ScalarKind::Normalized => "norm",
        }
    }

    /// 文字描述（读屏可达）。
    pub const fn describe(self) -> &'static str {
        match self {
            ScalarKind::Float => "浮点数：由硬件做插值",
            ScalarKind::SignedInt => "有符号整数：不插值，按位传",
            ScalarKind::UnsignedInt => "无符号整数：不插值，按位传",
            ScalarKind::Normalized => "归一化整数：读入时映射到零到一或负一到一",
        }
    }

    /// 是否参与插值（顶点属性里整数类不插值，这是布局与着色器必须说清的事）。
    pub const fn interpolated(self) -> bool {
        matches!(self, ScalarKind::Float | ScalarKind::Normalized)
    }

    /// 全集。
    pub fn all() -> [ScalarKind; 4] {
        [
            ScalarKind::Float,
            ScalarKind::SignedInt,
            ScalarKind::UnsignedInt,
            ScalarKind::Normalized,
        ]
    }
}

/// `const` 可用的标量种类相等判定。
///
/// **不能写 `a == b`**：`PartialEq` 派生的 `eq` 不是 `const fn`，在 `const` 上下文里
/// 调用非 const 函数是硬错误。改用 `matches!`逐变体列出相等组合——这也让「哪几种算
/// 同类」变成一份显式清单，而不是靠派生宏隐含。
pub const fn kind_eq(a: ScalarKind, b: ScalarKind) -> bool {
    matches!(
        (a, b),
        (ScalarKind::Float, ScalarKind::Float)
            | (ScalarKind::SignedInt, ScalarKind::SignedInt)
            | (ScalarKind::UnsignedInt, ScalarKind::UnsignedInt)
            | (ScalarKind::Normalized, ScalarKind::Normalized)
    )
}

/// 顶点属性格式（顶点数据在缓冲里的存法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexFormat {
    /// 单分量浮点。
    Float32,
    /// 双分量浮点。
    Float32x2,
    /// 三分量浮点（位置/法线/切线的常规选型）。
    Float32x3,
    /// 四分量浮点。
    Float32x4,
    /// 四分量 8 位无符号归一化（顶点色/骨骼权重）。
    Unorm8x4,
    /// 双分量 16 位有符号归一化（压缩切线）。
    Snorm16x2,
    /// 双分量 16 位无符号整数（UV 的 16 位原始值）。
    Uint16x2,
    /// 四分量 8 位无符号整数（骨骼索引的常规选型）。
    Uint8x4,
}

impl VertexFormat {
    /// 标量种类。
    pub const fn kind(self) -> ScalarKind {
        match self {
            VertexFormat::Float32
            | VertexFormat::Float32x2
            | VertexFormat::Float32x3
            | VertexFormat::Float32x4 => ScalarKind::Float,
            VertexFormat::Unorm8x4 | VertexFormat::Snorm16x2 => ScalarKind::Normalized,
            VertexFormat::Uint16x2 | VertexFormat::Uint8x4 => ScalarKind::UnsignedInt,
        }
    }

    /// 分量数。
    pub const fn components(self) -> u8 {
        match self {
            VertexFormat::Float32 => 1,
            VertexFormat::Float32x2 => 2,
            VertexFormat::Float32x3 => 3,
            VertexFormat::Float32x4 => 4,
            VertexFormat::Unorm8x4 => 4,
            VertexFormat::Snorm16x2 => 2,
            VertexFormat::Uint16x2 => 2,
            VertexFormat::Uint8x4 => 4,
        }
    }

    /// 单分量字节宽度。
    pub const fn component_bytes(self) -> u32 {
        match self {
            VertexFormat::Float32
            | VertexFormat::Float32x2
            | VertexFormat::Float32x3
            | VertexFormat::Float32x4 => 4,
            VertexFormat::Unorm8x4 | VertexFormat::Uint8x4 => 1,
            VertexFormat::Snorm16x2 | VertexFormat::Uint16x2 => 2,
        }
    }

    /// 字节宽度（该属性占用的缓冲字节数）。
    pub const fn byte_size(self) -> u32 {
        self.component_bytes() * self.components() as u32
    }

    /// 该格式要求的**偏移对齐**（属性起点必须落在它的整数倍上）。
    ///
    /// 取单分量字节宽度而非固定 4：16 位格式放在 2 的倍数上即可，8 位格式放在任意字节上
    /// 即可。用固定 4 会把合法的紧凑布局判成违例——那是过严，不是对齐。
    pub const fn required_align(self) -> u32 {
        self.component_bytes()
    }

    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            VertexFormat::Float32 => "r32f",
            VertexFormat::Float32x2 => "r32g2f",
            VertexFormat::Float32x3 => "r32g3f",
            VertexFormat::Float32x4 => "r32g4f",
            VertexFormat::Unorm8x4 => "r8g4b8a8unorm",
            VertexFormat::Snorm16x2 => "r16g16snorm",
            VertexFormat::Uint16x2 => "r16g16uint",
            VertexFormat::Uint8x4 => "r8g8b8a8uint",
        }
    }

    /// 文字描述（读屏可达）。
    pub const fn describe(self) -> &'static str {
        match self {
            VertexFormat::Float32 => "单分量 32 位浮点",
            VertexFormat::Float32x2 => "双分量 32 位浮点",
            VertexFormat::Float32x3 => "三分量 32 位浮点",
            VertexFormat::Float32x4 => "四分量 32 位浮点",
            VertexFormat::Unorm8x4 => "四分量 8 位无符号归一化",
            VertexFormat::Snorm16x2 => "双分量 16 位有符号归一化",
            VertexFormat::Uint16x2 => "双分量 16 位无符号整数",
            VertexFormat::Uint8x4 => "四分量 8 位无符号整数",
        }
    }

    /// **线上编码值**（写进顶点布局描述符的字节码）。
    ///
    /// 与枚举判别值**刻意不相等**——判别值是编译期产物，线上码是 ABI 契约；用
    /// `enum as u8` 造二进制头会让「重排枚举成员」变成静默的线上不兼容。映射写死在此，
    /// 由 `A22-decl-线上码与判别值解耦` 钉住。
    pub const fn wire(self) -> u8 {
        match self {
            VertexFormat::Float32 => 0x01,
            VertexFormat::Float32x2 => 0x02,
            VertexFormat::Float32x3 => 0x03,
            VertexFormat::Float32x4 => 0x04,
            VertexFormat::Unorm8x4 => 0x05,
            VertexFormat::Snorm16x2 => 0x06,
            VertexFormat::Uint16x2 => 0x07,
            VertexFormat::Uint8x4 => 0x08,
        }
    }

    /// 依语义给出的**默认格式**（声明式构建器缺格式时的选型）。
    ///
    /// 骨骼权重用四分量归一化（4 根骨影响一个顶点），骨骼索引用四分量 8 位整数
    /// （每字节一根骨，硬件直接当索引用）——这两个与语义**强绑定**，用错（比如给骨骼
    /// 权重发整数格式）在签名校验里会被标量种类判据抓住。
    pub const fn default_for(semantic: VertexSemantic) -> VertexFormat {
        match semantic {
            VertexSemantic::Position
            | VertexSemantic::Normal
            | VertexSemantic::Tangent
            | VertexSemantic::InstanceId => VertexFormat::Float32x4,
            VertexSemantic::Uv0 | VertexSemantic::Uv1 => VertexFormat::Float32x2,
            VertexSemantic::Color | VertexSemantic::BoneWeight => VertexFormat::Unorm8x4,
            VertexSemantic::BoneIndex => VertexFormat::Uint8x4,
        }
    }

    /// 全集。
    pub fn all() -> [VertexFormat; FORMAT_TABLE_SIZE] {
        [
            VertexFormat::Float32,
            VertexFormat::Float32x2,
            VertexFormat::Float32x3,
            VertexFormat::Float32x4,
            VertexFormat::Unorm8x4,
            VertexFormat::Snorm16x2,
            VertexFormat::Uint16x2,
            VertexFormat::Uint8x4,
        ]
    }
}

// ---------------------------------------------------------------------------
// 三、顶点语义
// ---------------------------------------------------------------------------

/// 顶点语义（这个属性是什么）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexSemantic {
    /// 顶点位置。
    Position,
    /// 法线。
    Normal,
    /// 切线（法线贴图用）。
    Tangent,
    /// 第一套纹理坐标。
    Uv0,
    /// 第二套纹理坐标。
    Uv1,
    /// 顶点色。
    Color,
    /// 骨骼索引。
    BoneIndex,
    /// 骨骼权重。
    BoneWeight,
    /// 逐实例标识。
    InstanceId,
}

impl VertexSemantic {
    /// 稳定短名（跨语言日志/描述符比对用）。
    pub const fn tag(self) -> &'static str {
        match self {
            VertexSemantic::Position => "POSITION",
            VertexSemantic::Normal => "NORMAL",
            VertexSemantic::Tangent => "TANGENT",
            VertexSemantic::Uv0 => "TEXCOORD0",
            VertexSemantic::Uv1 => "TEXCOORD1",
            VertexSemantic::Color => "COLOR",
            VertexSemantic::BoneIndex => "BONEINDEX",
            VertexSemantic::BoneWeight => "BONEWEIGHT",
            VertexSemantic::InstanceId => "INSTANCEID",
        }
    }

    /// 语义类别（决定读屏措辞与默认格式的归口）。
    pub const fn class_tag(self) -> &'static str {
        match self {
            VertexSemantic::Position => "position",
            VertexSemantic::Normal | VertexSemantic::Tangent => "direction",
            VertexSemantic::Uv0 | VertexSemantic::Uv1 => "texcoord",
            VertexSemantic::Color => "color",
            VertexSemantic::BoneIndex => "skin-index",
            VertexSemantic::BoneWeight => "skin-weight",
            VertexSemantic::InstanceId => "instance",
        }
    }

    /// 语义**约定下标**（同语义多份时的第几份，如第二套 UV 为 1）。
    pub const fn default_index(self) -> u8 {
        match self {
            VertexSemantic::Uv1 => 1,
            _ => 0,
        }
    }

    /// 该语义**必须**是整数类（骨骼索引给了浮点格式是设计错，签名校验会拒）。
    pub const fn requires_integer(self) -> bool {
        matches!(self, VertexSemantic::BoneIndex)
    }

    /// 全集。
    pub fn all() -> [VertexSemantic; SEMANTIC_TABLE_SIZE] {
        [
            VertexSemantic::Position,
            VertexSemantic::Normal,
            VertexSemantic::Tangent,
            VertexSemantic::Uv0,
            VertexSemantic::Uv1,
            VertexSemantic::Color,
            VertexSemantic::BoneIndex,
            VertexSemantic::BoneWeight,
            VertexSemantic::InstanceId,
        ]
    }
}

// ---------------------------------------------------------------------------
// 四、输入速率与编译期可用的属性描述
// ---------------------------------------------------------------------------

/// 属性步进（每个顶点给一次，还是每个实例给一次）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputRate {
    /// 逐顶点。
    PerVertex,
    /// 逐实例。
    PerInstance,
}

impl InputRate {
    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            InputRate::PerVertex => "vertex",
            InputRate::PerInstance => "instance",
        }
    }

    /// 文字描述（读屏可达）。
    pub const fn describe(self) -> &'static str {
        match self {
            InputRate::PerVertex => "逐顶点：每个顶点都取一次",
            InputRate::PerInstance => "逐实例：同一批顶点共用一份，取一次",
        }
    }

    /// 全集。
    pub fn all() -> [InputRate; 2] {
        [InputRate::PerVertex, InputRate::PerInstance]
    }
}

/// 编译期可用的顶点属性描述（无堆分配、无字符串——故能进 `const` 上下文）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstAttr {
    /// 语义。
    pub semantic: VertexSemantic,
    /// 同语义下标（第二套 UV 为 1）。
    pub semantic_index: u8,
    /// 格式。
    pub format: VertexFormat,
    /// 位置号（着色器输入位置）。
    pub location: u8,
    /// 字节偏移（由构建器推导，不手写）。
    pub offset: u32,
    /// 步进。
    pub rate: InputRate,
}

/// 编译期可用的着色器输入签名（着色器反射产出的那一面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstInput {
    /// 位置号。
    pub location: u8,
    /// 标量种类。
    pub kind: ScalarKind,
    /// 分量数。
    pub components: u8,
}

/// 编译期判定结论。
///
/// **只表达拒绝方向**——对齐修正与去重不是编译期闸门的事，它们各有各的时机
/// （修正服务外部导入布局，去重服务描述符复用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstVerdict {
    /// 通过。
    Ok,
    /// 有问题（附故障）。
    LayoutFault(LayoutFault),
}

/// 布局故障（每类一个码，**互不共用**——成因与处置方向都不同）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutFault {
    /// 步进未对齐到 [`STRIDE_ALIGN`]。
    StrideMisaligned {
        /// 实际步进。
        stride: u32,
    },
    /// 属性偏移未对齐到格式要求。
    OffsetMisaligned {
        /// 位置号。
        location: u8,
        /// 实际偏移。
        offset: u32,
        /// 要求对齐。
        required: u32,
    },
    /// 属性尾部越出步进。
    OutOfStride {
        /// 位置号。
        location: u8,
        /// 起始偏移。
        offset: u32,
        /// 属性字节数。
        size: u32,
        /// 布局步进。
        stride: u32,
    },
    /// 两个属性占用同一位置号。
    LocationConflict {
        /// 冲突的位置号。
        location: u8,
    },
    /// 着色器要这个位置，布局没给。
    MissingInput {
        /// 缺失的位置号。
        location: u8,
    },
    /// 标量种类不符（浮点给了整数、整数给了浮点）。
    KindMismatch {
        /// 位置号。
        location: u8,
        /// 布局侧种类。
        layout: ScalarKind,
        /// 着色器侧种类。
        shader: ScalarKind,
    },
    /// 分量数不符（着色器要 float3、布局给 float2 这类）。
    WidthMismatch {
        /// 位置号。
        location: u8,
        /// 布局侧分量数。
        layout: u8,
        /// 着色器侧分量数。
        shader: u8,
    },
}

impl LayoutFault {
    /// 诊断码。
    pub const fn code(self) -> &'static str {
        match self {
            LayoutFault::StrideMisaligned { .. } => "E_VL_STRIDE_MISALIGNED",
            LayoutFault::OffsetMisaligned { .. } => "E_VL_OFFSET_MISALIGNED",
            LayoutFault::OutOfStride { .. } => "E_VL_OUT_OF_STRIDE",
            LayoutFault::LocationConflict { .. } => "E_VL_LOCATION_CONFLICT",
            LayoutFault::MissingInput { .. } => "E_VL_SIGNATURE_MISSING_INPUT",
            LayoutFault::KindMismatch { .. } => "E_VL_SIGNATURE_KIND_MISMATCH",
            LayoutFault::WidthMismatch { .. } => "E_VL_SIGNATURE_WIDTH_MISMATCH",
        }
    }

    /// 人类可读描述（读屏与诊断包共用；要说实话，不许空话）。
    pub fn describe(self) -> String {
        match self {
            LayoutFault::StrideMisaligned { stride } => {
                format!("步进 {} 字节未对齐到 {} 字节", stride, STRIDE_ALIGN)
            }
            LayoutFault::OffsetMisaligned { location, offset, required } => format!(
                "位置 {}：偏移 {} 不是 {} 的整数倍（应按该格式的 {} 字节对齐）",
                location, offset, required, required
            ),
            LayoutFault::OutOfStride { location, offset, size, stride } => format!(
                "位置 {}：属性占用 [{}, {}字节) 越出步进 {}",
                location,
                offset,
                offset + size,
                stride
            ),
            LayoutFault::LocationConflict { location } => {
                format!("位置号 {} 被多个属性同时占用", location)
            }
            LayoutFault::MissingInput { location } => {
                format!("位置 {}：着色器声明了该输入，布局未提供", location)
            }
            LayoutFault::KindMismatch { location, layout, shader } => format!(
                "位置 {}：标量种类不符——布局 {} 对着色器 {}",
                location,
                layout.tag(),
                shader.tag()
            ),
            LayoutFault::WidthMismatch { location, layout, shader } => format!(
                "位置 {}：分量数不符——布局 {} 分量对着色器 {} 分量",
                location, layout, shader
            ),
        }
    }
}

impl ConstVerdict {
    /// 是否通过。
    pub const fn is_ok(self) -> bool {
        matches!(self, ConstVerdict::Ok)
    }

    /// 故障码（通过时为空串）。
    pub fn fault_code(self) -> &'static str {
        match self {
            ConstVerdict::Ok => "",
            ConstVerdict::LayoutFault(f) => f.code(),
        }
    }

    /// 故障描述（通过时为空串）。
    pub fn fault_text(self) -> String {
        match self {
            ConstVerdict::Ok => String::new(),
            ConstVerdict::LayoutFault(f) => f.describe(),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、编译期校验器（判据「签名校验」「编译期抓错」的落点）
// ---------------------------------------------------------------------------

/// 向上取整到 `align` 的整数倍（`align` 为 1 时原样返回）。
pub const fn align_up(v: u32, align: u32) -> u32 {
    if align <= 1 {
        return v;
    }
    let rem = v % align;
    if rem == 0 {
        v
    } else {
        v + (align - rem)
    }
}

/// **编译期**布局自洽性校验（不涉及着色器签名的那半）。
///
/// `const fn`：可放进 `const _: () = assert!(...)`，错位即编译错误。
pub const fn verify_layout_const(attrs: &[ConstAttr], stride: u32) -> ConstVerdict {
    if stride % STRIDE_ALIGN != 0 {
        return ConstVerdict::LayoutFault(LayoutFault::StrideMisaligned { stride });
    }
    let n = attrs.len();
    let mut i = 0;
    while i < n {
        let a = attrs[i];
        let required = a.format.required_align();
        if a.offset % required != 0 {
            return ConstVerdict::LayoutFault(LayoutFault::OffsetMisaligned {
                location: a.location,
                offset: a.offset,
                required,
            });
        }
        let size = a.format.byte_size();
        if a.offset + size > stride {
            return ConstVerdict::LayoutFault(LayoutFault::OutOfStride {
                location: a.location,
                offset: a.offset,
                size,
                stride,
            });
        }
        // 位置号唯一性：与已扫过的属性两两比对（O(n²)，n ≤ MAX_ATTRIBUTES）。
        let mut j = 0;
        while j < i {
            if attrs[j].location == a.location {
                return ConstVerdict::LayoutFault(LayoutFault::LocationConflict {
                    location: a.location,
                });
            }
            j += 1;
        }
        i += 1;
    }
    ConstVerdict::Ok
}

/// **编译期**布局 ↔ 着色器签名一致性校验（判据「签名校验」的落点）。
///
/// 判据三条：① 着色器声明的每个输入，布局必须给得出且位置号对齐；
/// ② 标量种类必须一致（浮点对浮点、整数对整数、归一化对归一化）；
/// ③ 分量数必须一致。
///
/// 顺序有意为之：**先把布局自身的自洽性查完，再查签名面**。布局都不自洽时报签名
/// 错位是误导——真正该改的是布局。
pub const fn verify_signature_const(
    attrs: &[ConstAttr],
    inputs: &[ConstInput],
    stride: u32,
) -> ConstVerdict {
    let layout_verdict = verify_layout_const(attrs, stride);
    if !layout_verdict.is_ok() {
        return layout_verdict;
    }
    let m = inputs.len();
    let n = attrs.len();
    let mut k = 0;
    while k < m {
        let inp = inputs[k];
        let mut found = false;
        let mut i = 0;
        while i < n {
            let a = attrs[i];
            if a.location == inp.location {
                found = true;
                if !kind_eq(a.format.kind(), inp.kind) {
                    return ConstVerdict::LayoutFault(LayoutFault::KindMismatch {
                        location: inp.location,
                        layout: a.format.kind(),
                        shader: inp.kind,
                    });
                }
                if a.format.components() != inp.components {
                    return ConstVerdict::LayoutFault(LayoutFault::WidthMismatch {
                        location: inp.location,
                        layout: a.format.components(),
                        shader: inp.components,
                    });
                }
            }
            i += 1;
        }
        if !found {
            return ConstVerdict::LayoutFault(LayoutFault::MissingInput {
                location: inp.location,
            });
        }
        k += 1;
    }
    ConstVerdict::Ok
}

// ---------------------------------------------------------------------------
// 六、编译期基准语料（闸门就钉在这几组数据上）
// ---------------------------------------------------------------------------

/// 基准正确布局：位置 + 法线 + UV（32 字节步进，偏移全对齐）。
pub const BASELINE_ATTRS: [ConstAttr; 3] = [
    ConstAttr {
        semantic: VertexSemantic::Position,
        semantic_index: 0,
        format: VertexFormat::Float32x3,
        location: 0,
        offset: 0,
        rate: InputRate::PerVertex,
    },
    ConstAttr {
        semantic: VertexSemantic::Normal,
        semantic_index: 0,
        format: VertexFormat::Float32x3,
        location: 1,
        offset: 12,
        rate: InputRate::PerVertex,
    },
    ConstAttr {
        semantic: VertexSemantic::Uv0,
        semantic_index: 0,
        format: VertexFormat::Float32x2,
        location: 2,
        offset: 24,
        rate: InputRate::PerVertex,
    },
];

/// 基准布局步进。
pub const BASELINE_STRIDE: u32 = 32;

/// 基准着色器签名（与 [`BASELINE_ATTRS`] 一致）。
pub const BASELINE_INPUTS: [ConstInput; 3] = [
    ConstInput { location: 0, kind: ScalarKind::Float, components: 3 },
    ConstInput { location: 1, kind: ScalarKind::Float, components: 3 },
    ConstInput { location: 2, kind: ScalarKind::Float, components: 2 },
];

/// **故意错位**的布局：UV 被放到偏移 26（4 字节要求下未对齐）。
///
/// 这不是随手编的数——26 是「有人手动插了个 2 字节填充、忘了删」的真实产物形态。
/// 这类错位在运行期的表现是**画面里 UV 采样逐渐偏移**（不崩、不报错，只是慢慢不对），
/// 恰恰是最该在编译期抓住的一类。步进取 36（已对齐），好让**偏移错位**成为首个故障——
/// 若步进也用 34，报出来的会是步进错位，把真正的问题盖住了。
pub const BROKEN_ATTRS: [ConstAttr; 3] = [
    ConstAttr {
        semantic: VertexSemantic::Position,
        semantic_index: 0,
        format: VertexFormat::Float32x3,
        location: 0,
        offset: 0,
        rate: InputRate::PerVertex,
    },
    ConstAttr {
        semantic: VertexSemantic::Normal,
        semantic_index: 0,
        format: VertexFormat::Float32x3,
        location: 1,
        offset: 12,
        rate: InputRate::PerVertex,
    },
    ConstAttr {
        semantic: VertexSemantic::Uv0,
        semantic_index: 0,
        format: VertexFormat::Float32x2,
        location: 2,
        offset: 26,
        rate: InputRate::PerVertex,
    },
];

/// 故意错位布局的步进（**已对齐**，好让偏移错位成为首个报出的故障）。
pub const BROKEN_STRIDE: u32 = 36;

/// 步进未对齐的语料（34 不是 4 的倍数）。
pub const STRIDE_BAD_STRIDE: u32 = 34;

/// 签名缺输入的语料：着色器多要一个切线输入，布局没给。
pub const OVERREACH_INPUTS: [ConstInput; 4] = [
    ConstInput { location: 0, kind: ScalarKind::Float, components: 3 },
    ConstInput { location: 1, kind: ScalarKind::Float, components: 3 },
    ConstInput { location: 2, kind: ScalarKind::Float, components: 2 },
    ConstInput { location: 3, kind: ScalarKind::Float, components: 3 },
];

/// 标量种类错位的语料：位置 1（法线）着色器按无符号整数收，布局给浮点。
pub const KIND_MISMATCH_INPUTS: [ConstInput; 3] = [
    ConstInput { location: 0, kind: ScalarKind::Float, components: 3 },
    ConstInput { location: 1, kind: ScalarKind::UnsignedInt, components: 3 },
    ConstInput { location: 2, kind: ScalarKind::Float, components: 2 },
];

/// 分量数错位的语料：位置 2 着色器要 3 分量，布局只给 2 分量。
pub const WIDTH_MISMATCH_INPUTS: [ConstInput; 3] = [
    ConstInput { location: 0, kind: ScalarKind::Float, components: 3 },
    ConstInput { location: 1, kind: ScalarKind::Float, components: 3 },
    ConstInput { location: 2, kind: ScalarKind::Float, components: 3 },
];

/// 撞号语料：两个属性抢位置 1。第三个属性用 4 字节格式，尾部不越步进——
/// 好让**撞号**而不是越界成为首个报出的故障。
pub const CONFLICT_ATTRS: [ConstAttr; 3] = [
    ConstAttr {
        semantic: VertexSemantic::Position,
        semantic_index: 0,
        format: VertexFormat::Float32x3,
        location: 0,
        offset: 0,
        rate: InputRate::PerVertex,
    },
    ConstAttr {
        semantic: VertexSemantic::Normal,
        semantic_index: 0,
        format: VertexFormat::Float32x3,
        location: 1,
        offset: 12,
        rate: InputRate::PerVertex,
    },
    ConstAttr {
        semantic: VertexSemantic::Color,
        semantic_index: 0,
        format: VertexFormat::Unorm8x4,
        location: 1,
        offset: 24,
        rate: InputRate::PerVertex,
    },
];

/// 越界语料：属性尾部越出步进（偏移 28 + 12 字节 = 40 > 32）。
pub const OVERRUN_ATTRS: [ConstAttr; 2] = [
    ConstAttr {
        semantic: VertexSemantic::Position,
        semantic_index: 0,
        format: VertexFormat::Float32x3,
        location: 0,
        offset: 0,
        rate: InputRate::PerVertex,
    },
    ConstAttr {
        semantic: VertexSemantic::Normal,
        semantic_index: 0,
        format: VertexFormat::Float32x3,
        location: 1,
        offset: 28,
        rate: InputRate::PerVertex,
    },
];

/// **编译期闸门 · 正例**：正确布局 + 一致签名必须通过。
///
/// 编译不过就说明校验器把对的判成错的（过严也是缺陷）。
pub const ASSERT_LAYOUT_OK: () = {
    assert!(verify_signature_const(&BASELINE_ATTRS, &BASELINE_INPUTS, BASELINE_STRIDE).is_ok());
    assert!(verify_layout_const(&BASELINE_ATTRS, BASELINE_STRIDE).is_ok());
};

/// **编译期闸门 · 反例（恒真门禁的照妖镜）**：六类错位**必须**全被拒。
///
/// 校验器一旦退化成「永远通过」，这条 `const` 立刻编译失败。
/// 只写正例的闸门等于没有闸门——它只能证明「没坏」，不能证明「在查」。
pub const ASSERT_BROKEN_REJECTED: () = {
    // 偏移未对齐
    assert!(!verify_signature_const(&BROKEN_ATTRS, &BASELINE_INPUTS, BROKEN_STRIDE).is_ok());
    // 步进未对齐
    assert!(!verify_layout_const(&BASELINE_ATTRS, STRIDE_BAD_STRIDE).is_ok());
    // 签名缺输入
    assert!(!verify_signature_const(&BASELINE_ATTRS, &OVERREACH_INPUTS, BASELINE_STRIDE).is_ok());
    // 标量种类错位
    assert!(!verify_signature_const(&BASELINE_ATTRS, &KIND_MISMATCH_INPUTS, BASELINE_STRIDE).is_ok());
    // 分量数错位
    assert!(!verify_signature_const(&BASELINE_ATTRS, &WIDTH_MISMATCH_INPUTS, BASELINE_STRIDE).is_ok());
    // 位置撞号 / 尾部越界
    assert!(!verify_layout_const(&CONFLICT_ATTRS, BASELINE_STRIDE).is_ok());
    assert!(!verify_layout_const(&OVERRUN_ATTRS, BASELINE_STRIDE).is_ok());
};

// ---------------------------------------------------------------------------
// 七、声明式构建器（判据「声明式布局」的落点）
// ---------------------------------------------------------------------------

/// 声明式构建结果（**建成**方向）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuildVerdict {
    /// 布局成立。
    Built {
        /// 布局。
        layout: VertexLayout,
    },
    /// 明确拒绝（附原因）。
    Rejected {
        /// 原因。
        reason: String,
    },
}

impl BuildVerdict {
    /// 是否建成。
    pub fn built(&self) -> bool {
        matches!(self, BuildVerdict::Built { .. })
    }

    /// 取布局（未建成时给 `None`，供不想 match 的调用方使用）。
    pub fn layout(&self) -> Option<&VertexLayout> {
        match self {
            BuildVerdict::Built { layout } => Some(layout),
            BuildVerdict::Rejected { .. } => None,
        }
    }
}

/// 声明式顶点布局构建器。
///
/// 只收「有哪些属性」，不收偏移——偏移由本构建器推导（见 [`DECLARATIVE_DOC`]）。
pub struct VertexLayoutBuilder {
    /// 声明的属性（按声明序）。
    attrs: Vec<ConstAttr>,
}

impl VertexLayoutBuilder {
    /// 新建空构建器。
    pub fn new() -> Self {
        VertexLayoutBuilder { attrs: Vec::new() }
    }

    /// 声明一个属性（不给偏移；偏移随后续属性自动推导）。
    ///
    /// 返回 `false` 表示属性数已超上限——**不静默丢弃**：超限的布局若被静默截断，
    /// 上层会以为全部属性都进了 GPU 描述符。
    pub fn push(&mut self, semantic: VertexSemantic, format: VertexFormat, location: u8) -> bool {
        if self.attrs.len() >= MAX_ATTRIBUTES {
            return false;
        }
        self.attrs.push(ConstAttr {
            semantic,
            semantic_index: semantic.default_index(),
            format,
            location,
            // 占位：build() 里的偏移推导会就地改写。
            offset: 0,
            rate: InputRate::PerVertex,
        });
        true
    }

    /// 声明一个逐实例属性。
    pub fn push_instanced(
        &mut self,
        semantic: VertexSemantic,
        format: VertexFormat,
        location: u8,
    ) -> bool {
        if !self.push(semantic, format, location) {
            return false;
        }
        let last = self.attrs.len() - 1;
        self.attrs[last].rate = InputRate::PerInstance;
        true
    }

    /// 已声明属性数。
    pub fn len(&self) -> usize {
        self.attrs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    /// 推导偏移并建成布局。
    pub fn build(self) -> BuildVerdict {
        if self.attrs.is_empty() {
            return BuildVerdict::Rejected {
                reason: String::from("空布局：没有任何属性，无法与着色器签名对齐"),
            };
        }
        let mut attrs = self.attrs;
        let n = attrs.len();
        // ---- 位置号：越界与撞号先行检出 ----
        // 撞号必须先于偏移推导：撞号时推导出来的寻址建立在错误前提上。
        let mut i = 0;
        while i < n {
            if attrs[i].location > MAX_LOCATION {
                return BuildVerdict::Rejected {
                    reason: format!(
                        "位置号 {} 越界（上限 {}，该编号空间与绑定侧共享）",
                        attrs[i].location, MAX_LOCATION
                    ),
                };
            }
            let mut j = 0;
            while j < i {
                if attrs[j].location == attrs[i].location {
                    return BuildVerdict::Rejected {
                        reason: format!("位置号 {} 被重复占用", attrs[i].location),
                    };
                }
                j += 1;
            }
            i += 1;
        }
        // ---- 语义下标必须与约定相符 ----
        i = 0;
        while i < n {
            if attrs[i].semantic_index != attrs[i].semantic.default_index() {
                return BuildVerdict::Rejected {
                    reason: format!(
                        "语义 {} 的下标 {} 与约定 {} 不符",
                        attrs[i].semantic.tag(),
                        attrs[i].semantic_index,
                        attrs[i].semantic.default_index()
                    ),
                };
            }
            i += 1;
        }
        // ---- 偏移推导（声明式的核心：偏移不是手写的） ----
        //
        // **按位置号排序后再推导**，不按声明书写序。这一点决定成败：若按书写序推导，
        // 同一组属性换个顺序声明就会得到不同的偏移与规范形——声明式布局将退化成
        // 「书写序的函数」，去重（[`DEDUP_DOC`] 要求按规范形去重）随之失效，
        // 而且这种失效很隐蔽：两份语义完全相同的声明被判成两个不同布局。
        //
        // 按位置号推导后，**声明顺序不影响结果**（`A22-dedup-声明序无关` 钉住这条）。
        sort_attrs_by_location(&mut attrs);
        let total = derive_offsets(&mut attrs);
        let stride = align_up(total, STRIDE_ALIGN);
        if stride > MAX_STRIDE {
            return BuildVerdict::Rejected {
                reason: format!(
                    "推导步进 {} 字节超预算 {}（属性太多或格式太宽）",
                    stride, MAX_STRIDE
                ),
            };
        }
        let verdict = verify_layout_const(&attrs, stride);
        if !verdict.is_ok() {
            return BuildVerdict::Rejected {
                reason: format!("推导后仍不自洽：{}", verdict.fault_text()),
            };
        }
        BuildVerdict::Built { layout: VertexLayout { attrs, stride } }
    }
}

impl Default for VertexLayoutBuilder {
    fn default() -> Self {
        VertexLayoutBuilder::new()
    }
}

/// 就地推导偏移：逐属性按格式的对齐要求落位，返回末属性尾部（未做步进对齐）。
fn derive_offsets(attrs: &mut [ConstAttr]) -> u32 {
    let mut cursor: u32 = 0;
    let mut i = 0;
    while i < attrs.len() {
        let aligned = align_up(cursor, attrs[i].format.required_align());
        attrs[i].offset = aligned;
        cursor = aligned + attrs[i].format.byte_size();
        i += 1;
    }
    cursor
}

// ---------------------------------------------------------------------------
// 八、顶点布局
// ---------------------------------------------------------------------------

/// 顶点输入布局（描述式；不含量化后的顶点数据本身）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VertexLayout {
    /// 属性（按声明序；规范形另算）。
    pub attrs: Vec<ConstAttr>,
    /// 步进（每元素字节数）。
    pub stride: u32,
}

impl VertexLayout {
    /// 属性数。
    pub fn len(&self) -> usize {
        self.attrs.len()
    }

    /// 是否无属性。
    pub fn is_empty(&self) -> bool {
        self.attrs.is_empty()
    }

    /// 按位置号取属性。
    pub fn attr_at(&self, location: u8) -> Option<&ConstAttr> {
        self.attrs.iter().find(|a| a.location == location)
    }

    /// 逐实例属性数。
    pub fn instance_rate_count(&self) -> usize {
        self.attrs
            .iter()
            .filter(|a| a.rate == InputRate::PerInstance)
            .count()
    }

    /// 各属性格式字节宽之和（**顶点数据的实际占用**）。
    ///
    /// 与步进之差即步进对齐填充。与语义名、语言、说明文字**全都无关**——
    /// 这正是「顶点数据不含文本」的可失败算术（见 [`NO_TEXT_IN_BUFFER_DOC`]）。
    pub fn payload_bytes(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < self.attrs.len() {
            sum += self.attrs[i].format.byte_size();
            i += 1;
        }
        sum
    }

    /// **规范形**：属性按位置号升序排序后的稳定文本表示（去重的唯一依据）。
    ///
    /// 含步进与逐属性偏移——见 [`DEDUP_DOC`]：**步进参与规范形**，因为步进决定缓冲内寻址。
    pub fn canonical_form(&self) -> String {
        let mut sorted: Vec<ConstAttr> = self.attrs.clone();
        sort_attrs_by_location(&mut sorted);
        let mut s = String::new();
        s.push_str("stride=");
        s.push_str(&self.stride.to_string());
        s.push(';');
        let mut i = 0;
        while i < sorted.len() {
            let a = sorted[i];
            s.push_str(&format!(
                "loc{}:{}#{}.{}.{}@{};",
                a.location,
                a.semantic.tag(),
                a.semantic_index,
                a.format.wire(),
                a.rate.tag(),
                a.offset
            ));
            i += 1;
        }
        s
    }

    /// 顶点数据总字节数（一个元素的步进；**不含任何文本**——见 [`NO_TEXT_IN_BUFFER_DOC`]）。
    pub const fn vertex_bytes(&self) -> u32 {
        self.stride
    }

    /// 属性表读屏文本（无障碍判据的落点）。
    pub fn a11y_preview(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "顶点输入布局：{} 个属性，步进 {} 字节",
            self.len(),
            self.stride
        ));
        let mut sorted: Vec<ConstAttr> = self.attrs.clone();
        sort_attrs_by_location(&mut sorted);
        let mut i = 0;
        while i < sorted.len() {
            let a = sorted[i];
            out.push(format!(
                "槽位 {}：{}（{}），格式 {}（{}），偏移 {} 字节，{}",
                a.location,
                a.semantic.tag(),
                semantic_zh(a.semantic),
                a.format.tag(),
                a.format.describe(),
                a.offset,
                a.rate.describe()
            ));
            i += 1;
        }
        out
    }
}

/// 属性按位置号升序排序（插入排序；属性数上限 [`MAX_ATTRIBUTES`]，无需更激进的算法）。
fn sort_attrs_by_location(v: &mut [ConstAttr]) {
    let n = v.len();
    let mut i = 1;
    while i < n {
        let key = v[i];
        let mut j = i;
        while j > 0 && key.location < v[j - 1].location {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
        i += 1;
    }
}

/// 支持的说明语言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    /// 简体中文。
    ZhCn,
    /// 英文。
    EnUs,
}

impl Locale {
    /// 稳定短名。
    pub const fn tag(self) -> &'static str {
        match self {
            Locale::ZhCn => "zh-CN",
            Locale::EnUs => "en-US",
        }
    }

    /// 全集。
    pub fn all() -> [Locale; 2] {
        [Locale::ZhCn, Locale::EnUs]
    }
}

/// 语义的简体中文说明（读屏用；元数据，不进顶点缓冲）。
pub fn semantic_zh(s: VertexSemantic) -> &'static str {
    match s {
        VertexSemantic::Position => "顶点位置",
        VertexSemantic::Normal => "表面法线",
        VertexSemantic::Tangent => "表面切线",
        VertexSemantic::Uv0 => "第一套纹理坐标",
        VertexSemantic::Uv1 => "第二套纹理坐标",
        VertexSemantic::Color => "顶点颜色",
        VertexSemantic::BoneIndex => "骨骼索引",
        VertexSemantic::BoneWeight => "骨骼权重",
        VertexSemantic::InstanceId => "逐实例标识",
    }
}

/// 语义的英文说明。
pub fn semantic_en(s: VertexSemantic) -> &'static str {
    match s {
        VertexSemantic::Position => "vertex position",
        VertexSemantic::Normal => "surface normal",
        VertexSemantic::Tangent => "surface tangent",
        VertexSemantic::Uv0 => "primary texture coordinates",
        VertexSemantic::Uv1 => "secondary texture coordinates",
        VertexSemantic::Color => "vertex colour",
        VertexSemantic::BoneIndex => "skeleton bone indices",
        VertexSemantic::BoneWeight => "skeleton bone weights",
        VertexSemantic::InstanceId => "per-instance identifier",
    }
}

/// **国际化属性说明**（元数据；顶点缓冲里没有文本，见 [`NO_TEXT_IN_BUFFER_DOC`]）。
///
/// 与 [`VertexLayout::a11y_preview`] 的分工：那里是**读屏行**（固定中文措辞），
/// 这里按语言产出说明，并可断言「换语言不改变二进制布局」。
pub fn describe_in(layout: &VertexLayout, locale: Locale) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "顶点输入布局（{}）：{} 个属性，步进 {} 字节",
        locale.tag(),
        layout.len(),
        layout.stride
    ));
    let mut sorted: Vec<ConstAttr> = layout.attrs.clone();
    sort_attrs_by_location(&mut sorted);
    let mut i = 0;
    while i < sorted.len() {
        let a = sorted[i];
        let semantic_text = match locale {
            Locale::ZhCn => semantic_zh(a.semantic),
            Locale::EnUs => semantic_en(a.semantic),
        };
        let rate_text = match locale {
            Locale::ZhCn => a.rate.describe(),
            Locale::EnUs => match a.rate {
                InputRate::PerVertex => "per vertex: one value per vertex",
                InputRate::PerInstance => "per instance: one value shared by the batch",
            },
        };
        out.push(format!(
            "槽位 {}：{} = {}，格式 {}（{} 字节），偏移 {}，{}",
            a.location,
            a.semantic.tag(),
            semantic_text,
            a.format.tag(),
            a.format.byte_size(),
            a.offset,
            rate_text
        ));
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 九、运行期校验（与编译期同判据）
// ---------------------------------------------------------------------------

/// 运行期一致性结论（**拒绝**方向）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignatureVerdict {
    /// 一致。
    Consistent,
    /// 不一致，逐条列出。
    Rejected {
        /// 故障清单（至少一条）。
        faults: Vec<LayoutFault>,
    },
}

impl SignatureVerdict {
    /// 是否一致。
    pub fn consistent(&self) -> bool {
        matches!(self, SignatureVerdict::Consistent)
    }

    /// 是否被拒。
    pub fn rejected(&self) -> bool {
        matches!(self, SignatureVerdict::Rejected { .. })
    }

    /// 故障条数。
    pub fn fault_count(&self) -> usize {
        match self {
            SignatureVerdict::Consistent => 0,
            SignatureVerdict::Rejected { faults } => faults.len(),
        }
    }

    /// 诊断汇总（零静默：每条故障都带码与说法）。
    pub fn diagnostics(&self) -> Vec<String> {
        match self {
            SignatureVerdict::Consistent => vec![String::from("布局与着色器签名一致")],
            SignatureVerdict::Rejected { faults } => faults
                .iter()
                .map(|f| format!("[{}] {}", f.code(), f.describe()))
                .collect(),
        }
    }
}

/// **运行期**布局 ↔ 签名一致性校验。
///
/// 与 [`verify_signature_const`] **同一套判据**，差别只在表达：const 版首个故障即返回
/// （编译期只需知道「有问题」）；运行期版**收齐全部故障**再报（改一次比改十次好）。
/// 二者结论一致性由 `A22-sig-运行期与编译期同判` 钉住——防止两套判据各写各的。
pub fn verify_signature(layout: &VertexLayout, inputs: &[ConstInput]) -> SignatureVerdict {
    let mut faults: Vec<LayoutFault> = Vec::new();
    // ---- 布局面 ----
    if layout.stride % STRIDE_ALIGN != 0 {
        faults.push(LayoutFault::StrideMisaligned { stride: layout.stride });
    }
    let n = layout.attrs.len();
    let mut i = 0;
    while i < n {
        let a = layout.attrs[i];
        let required = a.format.required_align();
        if a.offset % required != 0 {
            faults.push(LayoutFault::OffsetMisaligned {
                location: a.location,
                offset: a.offset,
                required,
            });
        }
        let size = a.format.byte_size();
        if a.offset + size > layout.stride {
            faults.push(LayoutFault::OutOfStride {
                location: a.location,
                offset: a.offset,
                size,
                stride: layout.stride,
            });
        }
        let mut j = 0;
        while j < i {
            if layout.attrs[j].location == a.location {
                faults.push(LayoutFault::LocationConflict { location: a.location });
            }
            j += 1;
        }
        i += 1;
    }
    // ---- 签名面 ----
    let m = inputs.len();
    let mut k = 0;
    while k < m {
        let inp = inputs[k];
        let mut found = false;
        let mut p = 0;
        while p < n {
            let a = layout.attrs[p];
            if a.location == inp.location {
                found = true;
                if a.format.kind() != inp.kind {
                    faults.push(LayoutFault::KindMismatch {
                        location: inp.location,
                        layout: a.format.kind(),
                        shader: inp.kind,
                    });
                }
                if a.format.components() != inp.components {
                    faults.push(LayoutFault::WidthMismatch {
                        location: inp.location,
                        layout: a.format.components(),
                        shader: inp.components,
                    });
                }
            }
            p += 1;
        }
        if !found {
            faults.push(LayoutFault::MissingInput { location: inp.location });
        }
        k += 1;
    }
    if faults.is_empty() {
        SignatureVerdict::Consistent
    } else {
        SignatureVerdict::Rejected { faults }
    }
}

// ---------------------------------------------------------------------------
// 十、对齐修正（处置方向：修正，与拒绝/去重分开）
// ---------------------------------------------------------------------------

/// 一条对齐修正记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlignFix {
    /// 位置号。
    pub location: u8,
    /// 修正前偏移。
    pub from: u32,
    /// 修正后偏移。
    pub to: u32,
    /// 要求对齐。
    pub required: u32,
}

impl AlignFix {
    /// 短名（诊断包与读屏）。
    pub fn tag(&self) -> String {
        format!(
            "loc{}: {}->{} (align {})",
            self.location, self.from, self.to, self.required
        )
    }
}

/// 对齐修正结果（**修正**方向）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AlignVerdict {
    /// 无需修正（偏移已全部对齐、步进已合规）。
    Aligned,
    /// 已修正，逐条列出改了什么。
    Corrected {
        /// 修正后的布局。
        layout: VertexLayout,
        /// 修正清单（至少一条）。
        fixes: Vec<AlignFix>,
    },
    /// 修不了（重推后步进超预算）——这一路才升级为**拒绝**。
    Unfixable {
        /// 原因。
        reason: String,
    },
}

impl AlignVerdict {
    /// 是否发生了修正。
    pub fn corrected(&self) -> bool {
        matches!(self, AlignVerdict::Corrected { .. })
    }

    /// 修不了（调用方据此走拒绝路径，不必 match 三态）。
    pub fn unfixable(&self) -> bool {
        matches!(self, AlignVerdict::Unfixable { .. })
    }
}

/// 对齐违例**修正**（[`ALIGN_FIX_DOC`]）。
///
/// 逐属性按格式的对齐要求重新推导偏移，步进再对齐到 [`STRIDE_ALIGN`]。
/// **只动偏移与步进，绝不动格式与语义**——改格式等于伪造调用方的声明。
/// 重推后步进超 [`MAX_STRIDE`] 则返回 [`AlignVerdict::Unfixable`]（不硬掰）。
///
/// 服务对象是**外部导入/手写布局**：声明式构建器（[`VertexLayoutBuilder`]）产出的布局
/// 天生对齐，走到这里只会得到 [`AlignVerdict::Aligned`]。
pub fn correct_alignments(layout: &VertexLayout) -> AlignVerdict {
    let mut attrs = layout.attrs.clone();
    let mut fixes: Vec<AlignFix> = Vec::new();
    let mut i = 0;
    while i < attrs.len() {
        let required = attrs[i].format.required_align();
        let old = attrs[i].offset;
        let aligned = align_up(old, required);
        if aligned != old {
            fixes.push(AlignFix {
                location: attrs[i].location,
                from: old,
                to: aligned,
                required,
            });
            attrs[i].offset = aligned;
        }
        i += 1;
    }
    let mut cursor: u32 = 0;
    i = 0;
    while i < attrs.len() {
        let tail = attrs[i].offset + attrs[i].format.byte_size();
        if tail > cursor {
            cursor = tail;
        }
        i += 1;
    }
    let stride = align_up(cursor, STRIDE_ALIGN);
    if fixes.is_empty() && stride == layout.stride {
        return AlignVerdict::Aligned;
    }
    if stride > MAX_STRIDE {
        return AlignVerdict::Unfixable {
            reason: format!(
                "按对齐修正后步进需 {} 字节，超预算 {}；不动格式就修不回来",
                stride, MAX_STRIDE
            ),
        };
    }
    let corrected = VertexLayout { attrs, stride };
    if !verify_layout_const(&corrected.attrs, corrected.stride).is_ok() {
        return AlignVerdict::Unfixable {
            reason: String::from("按对齐修正后仍越出步进，不动格式就修不回来"),
        };
    }
    AlignVerdict::Corrected {
        layout: corrected,
        fixes,
    }
}

// ---------------------------------------------------------------------------
// 十一、布局去重（处置方向：去重，与拒绝/修正分开）
// ---------------------------------------------------------------------------

/// 去重结果（**去重**方向）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutVerdict {
    /// 首次出现，已登记。
    Registered {
        /// 槽位号（去重表下标）。
        slot: usize,
    },
    /// 与已有槽位重复，复用之。
    Deduplicated {
        /// 复用到的槽位号。
        slot: usize,
        /// 该槽位累计申报次数。
        seen: usize,
    },
}

impl LayoutVerdict {
    /// 是否命中去重。
    pub fn deduplicated(&self) -> bool {
        matches!(self, LayoutVerdict::Deduplicated { .. })
    }

    /// 槽位号（两种形态都有）。
    pub fn slot(&self) -> usize {
        match self {
            LayoutVerdict::Registered { slot } => *slot,
            LayoutVerdict::Deduplicated { slot, .. } => *slot,
        }
    }
}

/// 布局去重表。
///
/// 按 [`VertexLayout::canonical_form`] 去重（见 [`DEDUP_DOC`]）。去重**只省描述符**，
/// 顶点数据一个字节都不动。
pub struct LayoutRegistry {
    /// 已登记的规范形。
    forms: Vec<String>,
    /// 每个规范形的申报次数。
    seen: Vec<usize>,
    /// 申报总次数。
    submissions: usize,
}

impl LayoutRegistry {
    /// 新建空表。
    pub fn new() -> Self {
        LayoutRegistry {
            forms: Vec::new(),
            seen: Vec::new(),
            submissions: 0,
        }
    }

    /// 申报一个布局。
    pub fn submit(&mut self, layout: &VertexLayout) -> LayoutVerdict {
        self.submissions += 1;
        let form = layout.canonical_form();
        let mut i = 0;
        while i < self.forms.len() {
            if self.forms[i] == form {
                self.seen[i] += 1;
                return LayoutVerdict::Deduplicated {
                    slot: i,
                    seen: self.seen[i],
                };
            }
            i += 1;
        }
        self.forms.push(form);
        self.seen.push(1);
        LayoutVerdict::Registered {
            slot: self.forms.len() - 1,
        }
    }

    /// 唯一布局数。
    pub fn unique(&self) -> usize {
        self.forms.len()
    }

    /// 申报总次数。
    pub fn submissions(&self) -> usize {
        self.submissions
    }

    /// 省掉的描述符数（申报数 − 唯一数）。
    pub fn saved(&self) -> usize {
        self.submissions - self.forms.len()
    }

    /// 某槽位的申报次数。
    pub fn seen_at(&self, slot: usize) -> usize {
        match self.seen.get(slot) {
            Some(v) => *v,
            None => 0,
        }
    }
}

impl Default for LayoutRegistry {
    fn default() -> Self {
        LayoutRegistry::new()
    }
}

// ---------------------------------------------------------------------------
// 十二、位置空间（跨批对接 A27）
// ---------------------------------------------------------------------------

/// 顶点输入 / 绑定表**共享**的位置编号空间（见 [`A27_LINK_DOC`]）。
pub struct LocationSpace {
    /// 已被顶点侧占用的位置号。
    vertex_taken: Vec<u8>,
    /// 已被绑定侧（F0027）占用的位置号。
    binding_taken: Vec<u8>,
    /// 越界与撞号审计。
    audits: Vec<String>,
}

impl LocationSpace {
    /// 新建空空间。
    pub fn new() -> Self {
        LocationSpace {
            vertex_taken: Vec::new(),
            binding_taken: Vec::new(),
            audits: Vec::new(),
        }
    }

    /// 顶点侧申请位置号。
    pub fn alloc_vertex(&mut self, location: u8) -> bool {
        if location > MAX_LOCATION {
            self.audits.push(format!(
                "顶点侧申请位置 {} 越界（上限 {}）——拒绝",
                location, MAX_LOCATION
            ));
            return false;
        }
        if self.binding_taken.contains(&location) {
            self.audits.push(format!(
                "顶点侧申请位置 {} 与绑定侧占用冲突——拒绝（两侧共享编号空间）",
                location
            ));
            return false;
        }
        if self.vertex_taken.contains(&location) {
            self.audits.push(format!("顶点侧重复申请位置 {} ——拒绝", location));
            return false;
        }
        self.vertex_taken.push(location);
        true
    }

    /// 绑定侧占用位置号（F0027 消费面）。
    pub fn reserve_binding(&mut self, location: u8) -> bool {
        if location > MAX_LOCATION {
            self.audits.push(format!(
                "绑定侧预留位置 {} 越界（上限 {}）——拒绝",
                location, MAX_LOCATION
            ));
            return false;
        }
        if self.vertex_taken.contains(&location) || self.binding_taken.contains(&location) {
            self.audits.push(format!(
                "绑定侧预留位置 {} 已被占用——拒绝（两侧共享编号空间）",
                location
            ));
            return false;
        }
        self.binding_taken.push(location);
        true
    }

    /// 顶点侧已占位置数。
    pub fn vertex_count(&self) -> usize {
        self.vertex_taken.len()
    }

    /// 绑定侧已占位置数。
    pub fn binding_count(&self) -> usize {
        self.binding_taken.len()
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }
}

impl Default for LocationSpace {
    fn default() -> Self {
        LocationSpace::new()
    }
}

// ---------------------------------------------------------------------------
// 十三、判据自检（CheckSet）
// ---------------------------------------------------------------------------

/// 取构建结果里的布局；未建成时给 `None`（自检内部用，避免 unwrap）。
fn built_layout(v: &BuildVerdict) -> Option<VertexLayout> {
    match v.layout() {
        Some(l) => Some(l.clone()),
        None => None,
    }
}

/// VE-F0022 · 顶点输入布局描述器 —— 判据自检。
///
/// 判据五条（锚点）：声明式布局、签名校验、编译期抓错、去重、判据。
/// 覆盖八个判据族：`decl-*`（声明式布局）、`sig-*`（签名校验）、`ct-*`（编译期抓错）、
/// `align-*`（对齐修正）、`dedup-*`（去重）、`i18n-*`（国际化说明）、
/// `a27-*`（跨批位置空间）、`judge-*`（契约条款在场）。
pub fn run_vea22_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0022");

    // ---- 声明式布局 ----

    {
        // 格式全集：字节宽 = 单分量宽 × 分量数；线上码两两不同。
        let all = VertexFormat::all();
        let sized_ok = all
            .iter()
            .all(|f| f.byte_size() == f.component_bytes() * f.components() as u32);
        let mut wires: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < all.len() {
            wires.push(all[k].wire());
            k += 1;
        }
        let mut uniq = true;
        let mut a = 0;
        while a < wires.len() {
            let mut b = a + 1;
            while b < wires.len() {
                if wires[a] == wires[b] {
                    uniq = false;
                }
                b += 1;
            }
            a += 1;
        }
        set.add(
            "A22-decl-格式表规模与字节宽自洽",
            all.len() == FORMAT_TABLE_SIZE && sized_ok && uniq,
            "",
        );
    }

    {
        // **线上码与判别值解耦**：线上码是显式映射，不是 `enum as u8`。
        // 若有人把 wire() 改成判别值，本判据必须变红（判别值恰为 0..7，线上码是 1..8）。
        let f0 = VertexFormat::Float32;
        let discriminant_like = f0 as u8;
        set.add(
            "A22-decl-线上码与判别值解耦",
            f0.wire() == 0x01 && discriminant_like == 0 && f0.wire() != discriminant_like,
            "",
        );
    }

    {
        // 语义全集规模、类别标签齐备、默认选型有分量。
        let sems = VertexSemantic::all();
        let named = sems.iter().all(|s| !s.tag().is_empty() && !s.class_tag().is_empty());
        let defaults_ok = sems
            .iter()
            .all(|s| s.default_index() <= 1 && VertexFormat::default_for(*s).components() > 0);
        let idx_ok = sems
            .iter()
            .all(|s| s.default_index() == if *s == VertexSemantic::Uv1 { 1 } else { 0 });
        set.add(
            "A22-decl-语义表规模与默认选型齐备",
            sems.len() == SEMANTIC_TABLE_SIZE && named && defaults_ok && idx_ok,
            "",
        );
    }

    {
        // 骨骼索引「必须整数类」这条强绑定：默认格式确实是整数，
        // 且判据本身只对骨骼索引为真（对位置为假——否则又是恒真弱门禁）。
        let bi = VertexSemantic::BoneIndex;
        let pos = VertexSemantic::Position;
        set.add(
            "A22-decl-骨骼索引强绑定整数",
            bi.requires_integer()
                && !pos.requires_integer()
                && VertexFormat::default_for(bi).kind() == ScalarKind::UnsignedInt
                && VertexFormat::default_for(bi).components() == 4,
            "",
        );
    }

    {
        // 标量种类：只有浮点与归一化插值，整数类不插值。
        let interpolated = ScalarKind::Float.interpolated()
            && ScalarKind::Normalized.interpolated()
            && !ScalarKind::SignedInt.interpolated()
            && !ScalarKind::UnsignedInt.interpolated();
        let described = ScalarKind::all()
            .iter()
            .all(|k| !k.tag().is_empty() && !k.describe().is_empty());
        set.add(
            "A22-decl-插值性质与描述齐备",
            interpolated && described && ScalarKind::all().len() == 4,
            "",
        );
    }

    {
        // 声明式推导：位置/法线/UV 的偏移应为 0/12/24，步进 32。
        let mut b = VertexLayoutBuilder::new();
        b.push(VertexSemantic::Position, VertexFormat::Float32x3, 0);
        b.push(VertexSemantic::Normal, VertexFormat::Float32x3, 1);
        b.push(VertexSemantic::Uv0, VertexFormat::Float32x2, 2);
        let l = built_layout(&b.build());
        let ok = match &l {
            Some(x) => {
                let offs: Vec<u32> = x.attrs.iter().map(|a| a.offset).collect();
                offs == vec![0u32, 12u32, 24u32]
                    && x.stride == 32
                    && x.vertex_bytes() == 32
                    && offs == BASELINE_ATTRS.iter().map(|a| a.offset).collect::<Vec<u32>>()
            }
            None => false,
        };
        set.add(
            "A22-decl-偏移由声明推导且合基准",
            ok && DECLARATIVE_DOC.contains("不声明偏移"),
            "",
        );
    }

    {
        // 步进不留半格尾巴：UV（4 字节，对齐 2）+ 顶点色（4 字节，对齐 1）→ 步进 8。
        let mut b = VertexLayoutBuilder::new();
        b.push(VertexSemantic::Uv0, VertexFormat::Uint16x2, 0);
        b.push(VertexSemantic::Color, VertexFormat::Unorm8x4, 1);
        let l = built_layout(&b.build());
        let ok = match &l {
            Some(x) => {
                let offs: Vec<u32> = x.attrs.iter().map(|a| a.offset).collect();
                offs == vec![0u32, 4u32] && x.stride == 8 && x.stride % STRIDE_ALIGN == 0
            }
            None => false,
        };
        set.add("A22-decl-步进补齐不留半格", ok, "");
    }

    {
        // **声明式路径不产生填充字节**（八个格式字节宽全是 4 的倍数、对齐要求整除 4）。
        // 这条把「填充恒为 0」写成可失败断言：日后新增一个 3 字节宽的格式，此处立刻变红，
        // 提醒同步更新 [`NO_TEXT_IN_BUFFER_DOC`] 里「缓冲字节数 = 各格式字节宽之和」的表述。
        let all = VertexFormat::all();
        let mut no_pad = true;
        let mut k = 0;
        while k < all.len() {
            if all[k].byte_size() % STRIDE_ALIGN != 0 {
                no_pad = false;
            }
            k += 1;
        }
        set.add(
            "A22-decl-声明式路径无填充字节",
            no_pad && MAX_ATTRIBUTES == 16,
            "",
        );
    }

    {
        // 空布局**拒绝**（不静默建成空描述符——空描述符会让签名校验永远「通过」）。
        let b = VertexLayoutBuilder::new();
        let r = b.build();
        let reason = match &r {
            BuildVerdict::Rejected { reason } => reason.clone(),
            BuildVerdict::Built { .. } => String::new(),
        };
        set.add(
            "A22-decl-空布局拒绝",
            !r.built() && reason.contains("空布局"),
            "",
        );
    }

    {
        // 位置号越界**拒绝**并带原因（不静默钳制——钳制会让两个属性悄悄撞号）。
        let mut b = VertexLayoutBuilder::new();
        b.push(VertexSemantic::Position, VertexFormat::Float32x3, 0);
        b.push(VertexSemantic::Normal, VertexFormat::Float32x3, 200);
        let r = b.build();
        let reason = match &r {
            BuildVerdict::Rejected { reason } => reason.clone(),
            BuildVerdict::Built { .. } => String::new(),
        };
        set.add(
            "A22-decl-位置越界拒绝并说明",
            !r.built() && reason.contains("越界"),
            "",
        );
    }

    {
        // 重复位置号**拒绝**（撞号必须先于偏移推导被拦下）。
        let mut b = VertexLayoutBuilder::new();
        b.push(VertexSemantic::Position, VertexFormat::Float32x3, 0);
        b.push(VertexSemantic::Normal, VertexFormat::Float32x3, 0);
        let r = b.build();
        let reason = match &r {
            BuildVerdict::Rejected { reason } => reason.clone(),
            BuildVerdict::Built { .. } => String::new(),
        };
        set.add(
            "A22-decl-重复位置拒绝",
            !r.built() && reason.contains("重复占用"),
            "",
        );
    }

    {
        // 逐实例属性：步进标记正确，且**只动那一个属性**的 rate。
        let mut b = VertexLayoutBuilder::new();
        b.push(VertexSemantic::Position, VertexFormat::Float32x3, 0);
        b.push(VertexSemantic::Normal, VertexFormat::Float32x3, 1);
        b.push_instanced(VertexSemantic::InstanceId, VertexFormat::Float32x4, 2);
        let l = built_layout(&b.build());
        let ok = match &l {
            Some(x) => {
                x.instance_rate_count() == 1
                    && x.attr_at(2).map(|a| a.rate) == Some(InputRate::PerInstance)
                    && x.attr_at(0).map(|a| a.rate) == Some(InputRate::PerVertex)
                    && x.attr_at(1).map(|a| a.rate) == Some(InputRate::PerVertex)
            }
            None => false,
        };
        set.add("A22-decl-逐实例步进标记只动一个", ok, "");
    }

    {
        // 语义下标与约定不符 → 拒绝（第二套 UV 的下标写错会让去重误判）。
        // 语料形态：把 Uv0 的下标改成 1（那是 Uv1 的下标）。
        let attrs = vec![ConstAttr {
            semantic: VertexSemantic::Uv0,
            semantic_index: 1,
            format: VertexFormat::Float32x2,
            location: 0,
            offset: 0,
            rate: InputRate::PerVertex,
        }];
        let stride = 8;
        // 走运行期校验路径：下标错位不会被 const 校验器抓到（它不看下标），
        // 故这里直接验构建器的拒绝理由——用一条独立的显式声明入口模拟。
        let r = check_semantic_index(&attrs);
        set.add(
            "A22-decl-语义下标与约定不符拒绝",
            !r.is_empty() && r.iter().any(|s| s.contains("下标")) && stride == 8,
            "",
        );
    }

    {
        // 属性数超上限：**拒绝新属性**且如实返回 false（不静默丢弃）。
        let mut b = VertexLayoutBuilder::new();
        let mut i = 0;
        let mut all_ok = true;
        while i < MAX_ATTRIBUTES {
            if !b.push(VertexSemantic::Color, VertexFormat::Unorm8x4, i as u8) {
                all_ok = false;
            }
            i += 1;
        }
        let overflow_rejected = !b.push(VertexSemantic::Color, VertexFormat::Unorm8x4, 0);
        set.add(
            "A22-decl-属性数超限不静默丢弃",
            all_ok && overflow_rejected && b.len() == MAX_ATTRIBUTES,
            "",
        );
    }

    {
        // 步进超预算**拒绝**（不是截断）：16 个 Float32x4 = 256 字节 > MAX_STRIDE。
        let mut b = VertexLayoutBuilder::new();
        let mut i = 0;
        while i < MAX_ATTRIBUTES {
            b.push(VertexSemantic::Color, VertexFormat::Float32x4, i as u8);
            i += 1;
        }
        let r = b.build();
        let reason = match &r {
            BuildVerdict::Rejected { reason } => reason.clone(),
            BuildVerdict::Built { .. } => String::new(),
        };
        set.add(
            "A22-decl-步进超预算拒绝",
            !r.built() && reason.contains("超预算"),
            "",
        );
    }

    // ---- 签名校验 ----

    {
        // 基准布局 + 一致签名 → 运行期判定为一致。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let v = verify_signature(&l, &BASELINE_INPUTS);
        set.add("A22-sig-基准一致", v.consistent() && v.fault_count() == 0, "");
    }

    {
        // 布局多给一个属性、着色器不要 → **仍算一致**（布局可超集，这是合法的）。
        // 判据：只有「着色器要而布局没有」才是错；反向不是。
        let mut b = VertexLayoutBuilder::new();
        b.push(VertexSemantic::Position, VertexFormat::Float32x3, 0);
        b.push(VertexSemantic::Normal, VertexFormat::Float32x3, 1);
        b.push(VertexSemantic::Uv0, VertexFormat::Float32x2, 2);
        b.push(VertexSemantic::Tangent, VertexFormat::Float32x3, 3);
        let l = built_layout(&b.build());
        let ok = match &l {
            Some(x) => {
                // 确认确实多了一个属性（否则这条判据恒真）。
                x.len() == 4 && verify_signature(x, &BASELINE_INPUTS).consistent()
            }
            None => false,
        };
        set.add("A22-sig-布局超集不算错", ok, "");
    }

    {
        // 着色器多要一个输入 → **拒绝**并指名缺失位置号。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let v = verify_signature(&l, &OVERREACH_INPUTS);
        let has_missing = match &v {
            SignatureVerdict::Rejected { faults } => faults
                .iter()
                .any(|f| matches!(f, LayoutFault::MissingInput { location: 3 })),
            SignatureVerdict::Consistent => false,
        };
        set.add(
            "A22-sig-缺输入拒绝并指名",
            v.rejected() && has_missing && v.fault_count() == 1,
            "",
        );
    }

    {
        // 标量种类错位 → 拒绝，且故障码与缺输入**不同**（不共用码）。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let v = verify_signature(&l, &KIND_MISMATCH_INPUTS);
        let code = first_code(&v);
        set.add(
            "A22-sig-标量种类错位拒绝",
            v.rejected() && code == "E_VL_SIGNATURE_KIND_MISMATCH",
            "",
        );
    }

    {
        // 分量数错位 → 拒绝。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let v = verify_signature(&l, &WIDTH_MISMATCH_INPUTS);
        set.add(
            "A22-sig-分量数错位拒绝",
            v.rejected() && first_code(&v) == "E_VL_SIGNATURE_WIDTH_MISMATCH",
            "",
        );
    }

    {
        // 故障码两两不同：成因与处置不同的状态不得共用码。
        let faults = [
            LayoutFault::StrideMisaligned { stride: 34 },
            LayoutFault::OffsetMisaligned { location: 0, offset: 1, required: 4 },
            LayoutFault::OutOfStride { location: 0, offset: 60, size: 12, stride: 32 },
            LayoutFault::LocationConflict { location: 0 },
            LayoutFault::MissingInput { location: 0 },
            LayoutFault::KindMismatch {
                location: 0,
                layout: ScalarKind::Float,
                shader: ScalarKind::UnsignedInt,
            },
            LayoutFault::WidthMismatch { location: 0, layout: 3, shader: 4 },
        ];
        let mut uniq = true;
        let mut i = 0;
        while i < faults.len() {
            let mut j = i + 1;
            while j < faults.len() {
                if faults[i].code() == faults[j].code() {
                    uniq = false;
                }
                j += 1;
            }
            i += 1;
        }
        set.add("A22-sig-故障码两两不同", uniq && faults.len() == 7, "");
    }

    {
        // **零静默**：每条故障都带码与说法（诊断文案要说实话，不许空串）。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let v = verify_signature(&l, &OVERREACH_INPUTS);
        let diags = v.diagnostics();
        let all_speak = diags
            .iter()
            .all(|d| d.contains("E_VL_") && d.len() > 8);
        let described = LayoutFault::MissingInput { location: 7 }
            .describe()
            .contains('7');
        set.add(
            "A22-sig-诊断零静默",
            diags.len() == 1 && all_speak && described,
            "",
        );
    }

    {
        // 运行期**收齐全部故障**，不是首个即返回（改一次比改十次好）。
        // 语料：位置 0 偏移未对齐 + 位置 2 分量数不符 + 位置 1 缺输入 → 三条。
        let bad = VertexLayout {
            attrs: vec![
                ConstAttr {
                    semantic: VertexSemantic::Position,
                    semantic_index: 0,
                    format: VertexFormat::Float32x3,
                    location: 0,
                    // 2 不是 4 的倍数
                    offset: 2,
                    rate: InputRate::PerVertex,
                },
                ConstAttr {
                    semantic: VertexSemantic::Uv0,
                    semantic_index: 0,
                    format: VertexFormat::Float32x2,
                    location: 2,
                    offset: 16,
                    rate: InputRate::PerVertex,
                },
            ],
            stride: 32,
        };
        let v = verify_signature(&bad, &WIDTH_MISMATCH_INPUTS);
        // 三条：OffsetMisaligned(loc0) + MissingInput(loc1) + WidthMismatch(loc2)
        set.add("A22-sig-运行期收齐多故障", v.fault_count() == 3, "");
    }

    {
        // 位置冲突在**运行期**也须检出（与编译期同判据，不只编译期抓）。
        let clash = VertexLayout {
            attrs: CONFLICT_ATTRS.to_vec(),
            stride: BASELINE_STRIDE,
        };
        let v = verify_signature(&clash, &BASELINE_INPUTS);
        let has_conflict = match &v {
            SignatureVerdict::Rejected { faults } => faults
                .iter()
                .any(|f| matches!(f, LayoutFault::LocationConflict { location: 1 })),
            SignatureVerdict::Consistent => false,
        };
        set.add("A22-sig-撞号运行期亦检出", v.rejected() && has_conflict, "");
    }

    // ---- 编译期抓错 ----

    {
        // 编译期闸门本身在位：两条 const 已被求值（若为假，编译期就失败了）。
        set.add(
            "A22-ct-编译期闸门在位",
            ASSERT_LAYOUT_OK == () && ASSERT_BROKEN_REJECTED == (),
            "",
        );
    }

    {
        // **反假**：七类错位必须真的判红。
        //
        // 用例形态说明：若实现退化为「永远 Ok」，这里 7 个 `!is_ok()` 全 false → 判据红。
        // 这是 `ASSERT_BROKEN_REJECTED` 的运行期同构检查——编译期那条只保证编译得过，
        // 这里保证它**不是恒真**。
        let all_rejected = [
            verify_signature_const(&BROKEN_ATTRS, &BASELINE_INPUTS, BROKEN_STRIDE),
            verify_layout_const(&BASELINE_ATTRS, STRIDE_BAD_STRIDE),
            verify_signature_const(&BASELINE_ATTRS, &OVERREACH_INPUTS, BASELINE_STRIDE),
            verify_signature_const(&BASELINE_ATTRS, &KIND_MISMATCH_INPUTS, BASELINE_STRIDE),
            verify_signature_const(&BASELINE_ATTRS, &WIDTH_MISMATCH_INPUTS, BASELINE_STRIDE),
            verify_layout_const(&CONFLICT_ATTRS, BASELINE_STRIDE),
            verify_layout_const(&OVERRUN_ATTRS, BASELINE_STRIDE),
        ]
        .iter()
        .all(|v| !v.is_ok());
        set.add("A22-ct-七类错位全部编译期拒绝", all_rejected, "");
    }

    {
        // 每类错位报出**正确的码**（不能一律报「布局有问题」）。
        // 语料刻意让「步进未对齐」与「偏移未对齐」分开——两者都在布局面，
        // 若顺序写反，BROKEN 那组会先报步进码，把真问题盖住。
        let codes = [
            verify_signature_const(&BROKEN_ATTRS, &BASELINE_INPUTS, BROKEN_STRIDE).fault_code(),
            verify_layout_const(&BASELINE_ATTRS, STRIDE_BAD_STRIDE).fault_code(),
            verify_signature_const(&BASELINE_ATTRS, &OVERREACH_INPUTS, BASELINE_STRIDE).fault_code(),
            verify_signature_const(&BASELINE_ATTRS, &KIND_MISMATCH_INPUTS, BASELINE_STRIDE).fault_code(),
            verify_signature_const(&BASELINE_ATTRS, &WIDTH_MISMATCH_INPUTS, BASELINE_STRIDE).fault_code(),
            verify_layout_const(&CONFLICT_ATTRS, BASELINE_STRIDE).fault_code(),
            verify_layout_const(&OVERRUN_ATTRS, BASELINE_STRIDE).fault_code(),
        ];
        set.add(
            "A22-ct-错位报出对应诊断码",
            codes[0] == "E_VL_OFFSET_MISALIGNED"
                && codes[1] == "E_VL_STRIDE_MISALIGNED"
                && codes[2] == "E_VL_SIGNATURE_MISSING_INPUT"
                && codes[3] == "E_VL_SIGNATURE_KIND_MISMATCH"
                && codes[4] == "E_VL_SIGNATURE_WIDTH_MISMATCH"
                && codes[5] == "E_VL_LOCATION_CONFLICT"
                && codes[6] == "E_VL_OUT_OF_STRIDE",
            "",
        );
    }

    {
        // **运行期与编译期同判**：两套表达不得各说各话。
        // 对每份语料，const 判红 ⟺ 运行期判红。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let inputs_sets: [&[ConstInput]; 3] = [
            &BASELINE_INPUTS,
            &OVERREACH_INPUTS,
            &KIND_MISMATCH_INPUTS,
        ];
        let mut agree = true;
        let mut i = 0;
        while i < inputs_sets.len() {
            let c = verify_signature_const(&l.attrs, inputs_sets[i], l.stride);
            let r = verify_signature(&l, inputs_sets[i]);
            if c.is_ok() != r.consistent() {
                agree = false;
            }
            i += 1;
        }
        set.add("A22-sig-运行期与编译期同判", agree, "");
    }

    {
        // 错位布局必须**既**被编译期拒、**又**被运行期拒（闸门无死角）。
        let broken = VertexLayout {
            attrs: BROKEN_ATTRS.to_vec(),
            stride: BROKEN_STRIDE,
        };
        let c = verify_signature_const(&broken.attrs, &BASELINE_INPUTS, broken.stride);
        let r = verify_signature(&broken, &BASELINE_INPUTS);
        set.add(
            "A22-ct-错位布局两闸同拒",
            !c.is_ok() && r.rejected() && !r.diagnostics().is_empty(),
            "",
        );
    }

    {
        // const 校验器只表达拒绝方向：通过时**不产故障**（零静默的反面——没问题就不该有声音）。
        let ok = verify_signature_const(&BASELINE_ATTRS, &BASELINE_INPUTS, BASELINE_STRIDE);
        set.add(
            "A22-ct-通过时不产故障",
            ok.is_ok() && ok.fault_code().is_empty() && ok.fault_text().is_empty(),
            "",
        );
    }

    {
        // 布局面故障**优先于**签名面故障（布局不自洽时报签名错位是误导）。
        // 语料：布局既撞号又缺输入 → 必须先报撞号（布局面）。
        let both = VertexLayout {
            attrs: CONFLICT_ATTRS.to_vec(),
            stride: BASELINE_STRIDE,
        };
        let v = verify_signature_const(&both.attrs, &OVERREACH_INPUTS, both.stride);
        set.add(
            "A22-ct-布局面故障优先报出",
            v.fault_code() == "E_VL_LOCATION_CONFLICT",
            "",
        );
    }

    // ---- 对齐修正 ----

    {
        // 错位布局经**修正**后自洽（而不是被拒绝）——「对齐违例→修正」的落点。
        let broken = VertexLayout {
            attrs: BROKEN_ATTRS.to_vec(),
            stride: BROKEN_STRIDE,
        };
        let r = correct_alignments(&broken);
        let now_ok = match &r {
            AlignVerdict::Corrected { layout, .. } => {
                verify_layout_const(&layout.attrs, layout.stride).is_ok()
            }
            _ => false,
        };
        set.add(
            "A22-align-错位修正为修正非拒绝",
            r.corrected() && now_ok && ALIGN_FIX_DOC.contains("修正"),
            "",
        );
    }

    {
        // 修正记录**如实报告**改了什么：UV 从 26 修正到 28（4 字节对齐）。
        // 用**真实的表外形态**：26 不是随手编的数，是「插了 2 字节填充忘了删」的产物。
        let broken = VertexLayout {
            attrs: BROKEN_ATTRS.to_vec(),
            stride: BROKEN_STRIDE,
        };
        let r = correct_alignments(&broken);
        let fix_uv = match &r {
            AlignVerdict::Corrected { fixes, .. } => {
                fixes.iter().find(|f| f.location == 2).copied()
            }
            _ => None,
        };
        let reported = match fix_uv {
            Some(f) => f.from == 26 && f.to == 28 && f.required == 4 && f.tag().contains("26->28"),
            None => false,
        };
        // 步进同步收窄：36 → 32（修正后末属性尾部 28+8=36，越出 36?不越，但填充归零）
        let stride_ok = match &r {
            AlignVerdict::Corrected { layout, .. } => layout.stride % STRIDE_ALIGN == 0,
            _ => false,
        };
        set.add("A22-align-修正如实报告且步进合规", reported && stride_ok, "");
    }

    {
        // **修正只动偏移，绝不动格式与语义**（改格式等于伪造调用方的声明）。
        let broken = VertexLayout {
            attrs: BROKEN_ATTRS.to_vec(),
            stride: BROKEN_STRIDE,
        };
        let r = correct_alignments(&broken);
        let untouched = match &r {
            AlignVerdict::Corrected { layout, .. } => {
                let mut same = layout.len() == broken.len();
                let mut i = 0;
                while i < layout.len() && same {
                    let x = layout.attrs[i];
                    let y = broken.attrs[i];
                    if x.semantic != y.semantic
                        || x.format != y.format
                        || x.location != y.location
                        || x.rate != y.rate
                        || x.semantic_index != y.semantic_index
                    {
                        same = false;
                    }
                    i += 1;
                }
                same
            }
            _ => false,
        };
        set.add("A22-align-修正不动格式语义", untouched, "");
    }

    {
        // 已对齐的布局：修正**幂等**（返回 Aligned，且布局逐位不变）。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let r = correct_alignments(&l);
        let idempotent = matches!(r, AlignVerdict::Aligned) && !r.corrected();
        // 再钉一层：对齐布局的规范形在修正前后必须逐字节相同。
        let canonical_same = match &r {
            AlignVerdict::Corrected { layout, .. } => layout.canonical_form() == l.canonical_form(),
            _ => true,
        };
        set.add("A22-align-已对齐修正幂等", idempotent && canonical_same, "");
    }

    {
        // 修正**修不了**时升级为拒绝，不硬掰。
        // 语料形态：10 个 Float32x4（每 16 字节）铺开，把最后一个属性挪到偏移 143
        // （未对齐）——按对齐修正后它落到 144，尾部 160 > MAX_STRIDE(128)，
        // 不动格式就修不回来，故必须 Unfixable 而非硬掰出一个越界布局。
        let mut attrs: Vec<ConstAttr> = Vec::new();
        let mut i: u32 = 0;
        while i < 10 {
            attrs.push(ConstAttr {
                semantic: VertexSemantic::Color,
                semantic_index: 0,
                format: VertexFormat::Float32x4,
                location: i as u8,
                offset: if i == 9 { 143 } else { i * 16 },
                rate: InputRate::PerVertex,
            });
            i += 1;
        }
        let bad = VertexLayout { attrs, stride: 160 };
        let r = correct_alignments(&bad);
        let said_budget = match &r {
            AlignVerdict::Unfixable { reason } => reason.contains("预算"),
            _ => false,
        };
        set.add("A22-align-修不了则升级为拒绝", r.unfixable() && said_budget, "");
    }

    {
        // 对齐要求按**格式**而非固定 4：8 位任意字节、16 位 2 的倍数、32 位 4 的倍数。
        // 这条同时是「8 位格式放在奇数偏移合法」的反证：见下一条。
        let ok8 = VertexFormat::Uint8x4.required_align() == 1;
        let ok16 = VertexFormat::Uint16x2.required_align() == 2;
        let ok32 = VertexFormat::Float32x3.required_align() == 4;
        let not_fixed = VertexFormat::Uint8x4.required_align() != 4;
        set.add(
            "A22-align-对齐要求按格式非固定四",
            ok8 && ok16 && ok32 && not_fixed,
            "",
        );
    }

    {
        // 8 位格式放在**奇数偏移合法**（若对齐要求被误写成固定 4，此处必红）。
        let odd = VertexLayout {
            attrs: vec![ConstAttr {
                semantic: VertexSemantic::Color,
                semantic_index: 0,
                format: VertexFormat::Uint8x4,
                location: 0,
                // 3：奇数，对齐要求 1 → 合法
                offset: 3,
                rate: InputRate::PerVertex,
            }],
            stride: 8,
        };
        set.add(
            "A22-align-八位格式奇数偏移合法",
            verify_layout_const(&odd.attrs, odd.stride).is_ok(),
            "",
        );
    }

    // ---- 去重 ----

    {
        // 同布局重复申报 → 去重命中，且省下一个描述符。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let mut reg = LayoutRegistry::new();
        let first = reg.submit(&l);
        let second = reg.submit(&l);
        set.add(
            "A22-dedup-重复布局去重",
            matches!(first, LayoutVerdict::Registered { slot: 0 })
                && second.deduplicated()
                && reg.unique() == 1
                && reg.saved() == 1
                && DEDUP_DOC.contains("规范形"),
            "",
        );
    }

    {
        // **声明顺序不同但同组属性 → 必须判为同一个**（去重按规范形，不按书写序）。
        //
        // 反假设计：光断言「两个布局去重命中」是**弱门禁**——拿同一个布局提交两次也恒绿。
        // 故此处先钉住「两次的**声明书写序确实不同**」，再钉「建出来的布局规范形相同、
        // 去重命中」。若有人把偏移改回按书写序推导，第二条立刻红。
        //
        // 注意：重排必须把 (语义, 格式, 位置) **三元组整体**倒过来，不能只倒语义而让格式
        // 按下标另配——那会造出「UV 配三分量浮点」这种**本就不同的布局**，判据红得毫无道理。
        let decls: [(VertexSemantic, VertexFormat, u8); 3] = [
            (VertexSemantic::Position, VertexFormat::Float32x3, 0),
            (VertexSemantic::Normal, VertexFormat::Float32x3, 1),
            (VertexSemantic::Uv0, VertexFormat::Float32x2, 2),
        ];
        let rev: [(VertexSemantic, VertexFormat, u8); 3] =
            [decls[2], decls[1], decls[0]];
        // 两次声明的书写序确实不同（否则整条判据恒真）。
        let decl_differs = decls[0].0 != rev[0].0 && decls[2].0 != rev[2].0;
        let mut b1 = VertexLayoutBuilder::new();
        let mut i = 0;
        while i < 3 {
            b1.push(decls[i].0, decls[i].1, decls[i].2);
            i += 1;
        }
        let mut b2 = VertexLayoutBuilder::new();
        let mut i = 0;
        while i < 3 {
            b2.push(rev[i].0, rev[i].1, rev[i].2);
            i += 1;
        }
        let l1 = built_layout(&b1.build());
        let l2 = built_layout(&b2.build());
        let ok = match (&l1, &l2) {
            (Some(x), Some(y)) => {
                let mut reg = LayoutRegistry::new();
                reg.submit(x);
                let r = reg.submit(y);
                decl_differs
                    && x.canonical_form() == y.canonical_form()
                    && r.deduplicated()
                    && reg.unique() == 1
            }
            _ => false,
        };
        set.add("A22-dedup-声明序无关", ok, "");
    }

    {
        // **步进不同的一定不是同一个** —— 本条最容易写错的地方。
        // 语料形态：属性集合完全相同，只差步进（32 vs 64）。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let fat = VertexLayout {
            attrs: l.attrs.clone(),
            stride: 64,
        };
        let mut reg = LayoutRegistry::new();
        let a = reg.submit(&l);
        let b = reg.submit(&fat);
        set.add(
            "A22-dedup-步进不同不合并",
            matches!(a, LayoutVerdict::Registered { slot: 0 })
                && matches!(b, LayoutVerdict::Registered { slot: 1 })
                && reg.unique() == 2
                && reg.saved() == 0,
            "",
        );
    }

    {
        // **偏移不同也不合并**：属性集合与步进都相同、只差一个属性偏移。
        // 若去重只看「语义集合」而不看偏移，两个不同寻址的布局会被悄悄并成一个。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let mut shifted = l.attrs.clone();
        shifted[1].offset = 16;
        let moved = VertexLayout { attrs: shifted, stride: 32 };
        let mut reg = LayoutRegistry::new();
        reg.submit(&l);
        let r = reg.submit(&moved);
        set.add("A22-dedup-偏移不同不合并", !r.deduplicated() && reg.unique() == 2, "");
    }

    {
        // 格式不同不合并（语义相同、格式不同是两种布局）。
        let mut b1 = VertexLayoutBuilder::new();
        b1.push(VertexSemantic::Color, VertexFormat::Unorm8x4, 0);
        let mut b2 = VertexLayoutBuilder::new();
        b2.push(VertexSemantic::Color, VertexFormat::Float32x4, 0);
        let l1 = built_layout(&b1.build());
        let l2 = built_layout(&b2.build());
        let ok = match (&l1, &l2) {
            (Some(x), Some(y)) => {
                let mut reg = LayoutRegistry::new();
                reg.submit(x);
                let r = reg.submit(y);
                x.canonical_form() != y.canonical_form() && !r.deduplicated() && reg.unique() == 2
            }
            _ => false,
        };
        set.add("A22-dedup-格式不同不合并", ok, "");
    }

    {
        // 步进不同也不合并（逐实例布局：步进语义不同，合并即串味）。
        let mut b1 = VertexLayoutBuilder::new();
        b1.push(VertexSemantic::Position, VertexFormat::Float32x3, 0);
        let mut b2 = VertexLayoutBuilder::new();
        b2.push_instanced(VertexSemantic::InstanceId, VertexFormat::Float32x4, 0);
        let l1 = built_layout(&b1.build());
        let l2 = built_layout(&b2.build());
        let ok = match (&l1, &l2) {
            (Some(x), Some(y)) => {
                let mut reg = LayoutRegistry::new();
                reg.submit(x);
                let r = reg.submit(y);
                !r.deduplicated() && reg.unique() == 2
            }
            _ => false,
        };
        set.add("A22-dedup-步进类别不同不合并", ok, "");
    }

    {
        // 申报次数如实累计（去重省了多少可查）。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let mut reg = LayoutRegistry::new();
        reg.submit(&l);
        reg.submit(&l);
        let r = reg.submit(&l);
        set.add(
            "A22-dedup-申报次数可查",
            reg.submissions() == 3
                && reg.unique() == 1
                && reg.saved() == 2
                && matches!(r, LayoutVerdict::Deduplicated { seen: 3, .. })
                && reg.seen_at(0) == 3
                && reg.seen_at(9) == 0,
            "",
        );
    }

    // ---- 国际化说明（顶点数据不含文本） ----

    {
        // **换语言不改变二进制布局**：说明行数相同，且两种语言产出的**布局侧数据**
        // （规范形 / 步进 / 缓冲字节）完全一致。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let zh = describe_in(&l, Locale::ZhCn);
        let en = describe_in(&l, Locale::EnUs);
        // 12+12+8 = 32，恰好等于步进（无填充）
        let bytes = l.payload_bytes();
        set.add(
            "A22-i18n-换语言不动二进制",
            zh.len() == en.len()
                && zh.len() == l.len() + 1
                && bytes == 32
                && l.vertex_bytes() == 32
                && l.canonical_form().contains("stride=32"),
            "",
        );
    }

    {
        // 两种语言的说明**确实不同**（否则上一条是恒真的——两份一模一样的文案也算「相同」）。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let zh = describe_in(&l, Locale::ZhCn);
        let en = describe_in(&l, Locale::EnUs);
        let mut differs = false;
        let mut i = 0;
        while i < zh.len() {
            if zh[i] != en[i] {
                differs = true;
            }
            i += 1;
        }
        set.add("A22-i18n-两种语言文案确有差异", differs, "");
    }

    {
        // **顶点数据不含文本**：缓冲字节数恒等于各格式字节宽之和，**与语义无关**。
        //
        // 用**表外真实形态**钉死：换一个带骨骼权重/索引的布局（5 个属性），
        // 若说明文字被塞进了属性，字节数会随语义/语言变化。
        let mut b = VertexLayoutBuilder::new();
        b.push(VertexSemantic::Position, VertexFormat::Float32x3, 0);
        b.push(VertexSemantic::Normal, VertexFormat::Float32x3, 1);
        b.push(VertexSemantic::BoneIndex, VertexFormat::Uint8x4, 2);
        b.push(VertexSemantic::BoneWeight, VertexFormat::Unorm8x4, 3);
        b.push(VertexSemantic::Color, VertexFormat::Unorm8x4, 4);
        let l = built_layout(&b.build());
        let ok = match &l {
            Some(x) => {
                // 12+12+4+4+4 = 36
                let expect = 36u32;
                x.payload_bytes() == expect
                    && x.vertex_bytes() == expect
                    && x.stride == expect
                    && NO_TEXT_IN_BUFFER_DOC.contains("不含任何文本")
            }
            None => false,
        };
        set.add("A22-i18n-顶点数据不含文本", ok, "");
    }

    {
        // 每种步进的格式字节宽各异时，缓冲字节数仍逐项等于格式之和（填充为 0）。
        // 这条把「无填充」钉在**多格式混排**上：Uint16x2(4) + Unorm8x4(4) + Float32x3(12) = 20。
        let mut b = VertexLayoutBuilder::new();
        b.push(VertexSemantic::Uv0, VertexFormat::Uint16x2, 0);
        b.push(VertexSemantic::Color, VertexFormat::Unorm8x4, 1);
        b.push(VertexSemantic::Normal, VertexFormat::Float32x3, 2);
        let l = built_layout(&b.build());
        let ok = match &l {
            Some(x) => {
                let offs: Vec<u32> = x.attrs.iter().map(|a| a.offset).collect();
                x.payload_bytes() == 20 && x.stride == 20 && offs == vec![0u32, 4u32, 8u32]
            }
            None => false,
        };
        set.add("A22-i18n-混排格式字节数逐项对齐", ok, "");
    }

    {
        // 每个语义在两种语言下都有说明（无障碍：布局预览读屏可达）。
        let sems = VertexSemantic::all();
        let mut covered = true;
        let mut distinct = true;
        let mut i = 0;
        while i < sems.len() {
            if semantic_zh(sems[i]).is_empty() || semantic_en(sems[i]).is_empty() {
                covered = false;
            }
            // 中英文说明不得相同（否则「双语」是假的）
            if semantic_zh(sems[i]) == semantic_en(sems[i]) {
                distinct = false;
            }
            i += 1;
        }
        set.add("A22-i18n-全语义双语覆盖且确为双语", covered && distinct, "");
    }

    {
        // 读屏预览：1 行表头 + 每属性一行；每行能找到该属性的槽位号与偏移。
        // 用 find 而非按下标取——若排序变了，按下标会取错属性，判据就成了假绿。
        let l = match built_layout(&skin_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let p = l.a11y_preview();
        let mut rows_ok = p.len() == l.len() + 1 && p.len() == 6;
        let mut i = 0;
        while i < l.len() && rows_ok {
            let a = l.attrs[i];
            let loc = format!("槽位 {}", a.location);
            let off = format!("偏移 {} 字节", a.offset);
            let found = p
                .iter()
                .skip(1)
                .any(|row| row.contains(&loc) && row.contains(&off));
            if !found {
                rows_ok = false;
            }
            i += 1;
        }
        // 表头须报出属性数与步进（12+4+4+4+8 = 32）
        let header_ok = p
            .first()
            .map(|h| h.contains("5 个属性") && h.contains("32 字节"))
            .unwrap_or(false);
        set.add("A22-i18n-读屏预览逐属性成行", rows_ok && header_ok, "");
    }

    // ---- 跨批位置空间（A27） ----

    {
        // 顶点侧与绑定侧**共享编号空间**：绑定侧占掉的位置，顶点侧申请必须失败。
        let mut sp = LocationSpace::new();
        let b_ok = sp.reserve_binding(4);
        let v_ok = sp.alloc_vertex(4);
        let audited = sp.audits().iter().any(|a| a.contains("绑定侧占用冲突"));
        set.add(
            "A22-a27-两侧共享编号空间",
            b_ok && !v_ok && sp.binding_count() == 1 && sp.vertex_count() == 0 && audited,
            "",
        );
    }

    {
        // 反向同理：顶点侧占掉的，绑定侧也拿不到。
        let mut sp = LocationSpace::new();
        sp.alloc_vertex(2);
        let audited = sp.audits().len();
        set.add(
            "A22-a27-反向亦不互抢",
            !sp.reserve_binding(2) && sp.audits().len() > audited,
            "",
        );
    }

    {
        // 越界申请**拒绝**并留审计（不静默钳制——钳制会让两侧以为拿到了合法位置）。
        let mut sp = LocationSpace::new();
        let ok = sp.alloc_vertex(200);
        let audited = sp.audits().iter().any(|a| a.contains("越界"));
        set.add("A22-a27-越界申请拒绝留痕", !ok && audited, "", );
    }

    {
        // 撞号**拒绝**并留审计（共享空间里最危险的是两侧各自从 0 数）。
        let mut sp = LocationSpace::new();
        sp.alloc_vertex(1);
        let dup = sp.alloc_vertex(1);
        let audited = sp.audits().iter().any(|a| a.contains("重复申请"));
        set.add("A22-a27-撞号拒绝留痕", !dup && audited, "");
    }

    {
        // 不撞号时正常分配，两侧计数各自独立（顶点 2 个 + 绑定 2 个）。
        let mut sp = LocationSpace::new();
        let ok = sp.alloc_vertex(0)
            && sp.alloc_vertex(1)
            && sp.reserve_binding(8)
            && sp.reserve_binding(9);
        set.add(
            "A22-a27-正常分配两侧独立",
            ok
                && sp.vertex_count() == 2
                && sp.binding_count() == 2
                && sp.audits().is_empty()
                && A27_LINK_DOC.contains("共享同一个编号空间"),
            "",
        );
    }

    {
        // 顶点布局的位置号**必须**都能在共享空间里申请到——
        // 否则「布局成立但位置号在跨批空间里已被绑定侧占掉」，是本条最容易漏的一路。
        let l = match built_layout(&baseline_builder().build()) {
            Some(x) => x,
            None => VertexLayout { attrs: Vec::new(), stride: 0 },
        };
        let mut sp = LocationSpace::new();
        // 绑定侧先占掉位置 2（基准布局的 UV 槽位）
        sp.reserve_binding(2);
        let mut i = 0;
        let mut blocked = 0;
        while i < l.len() {
            if !sp.alloc_vertex(l.attrs[i].location) {
                blocked += 1;
            }
            i += 1;
        }
        set.add(
            "A22-a27-布局槽位与绑定侧冲突可检出",
            blocked == 1 && sp.vertex_count() == 2 && sp.binding_count() == 1,
            "",
        );
    }

    // ---- 契约条款在场 ----

    {
        let docs_ok = DECLARATIVE_DOC.contains("不声明偏移")
            && COMPILE_TIME_DOC.contains("编译期")
            && ALIGN_FIX_DOC.contains("修正")
            && DEDUP_DOC.contains("规范形")
            && NO_TEXT_IN_BUFFER_DOC.contains("不含任何文本")
            && A27_LINK_DOC.contains("A27");
        set.add("A22-judge-六契约条款在场", docs_ok, "");
    }

    {
        // **三向处置互不共用**：拒绝、修正、去重是三个不同的结果类型，
        // 且三条顶层诊断码互不相同——共用即「处置方向相反的状态共用码」。
        let reject_diags = SignatureVerdict::Rejected {
            faults: vec![LayoutFault::MissingInput { location: 0 }],
        }
        .diagnostics()
        .len();
        let fix = AlignFix { location: 0, from: 1, to: 4, required: 4 };
        let codes_differ = E_LAYOUT_REJECT != E_LAYOUT_ALIGN_FIXED
            && E_LAYOUT_ALIGN_FIXED != E_LAYOUT_DEDUPED
            && E_LAYOUT_REJECT != E_LAYOUT_DEDUPED;
        // 三态枚举确实是三个不同类型（编译器保证）——这里钉的是它们的**码**分离。
        set.add(
            "A22-judge-三向处置码分离",
            reject_diags == 1
                && fix.from == 1
                && fix.to == 4
                && codes_differ
                && E_LAYOUT_REJECT.starts_with("E_")
                && E_LAYOUT_ALIGN_FIXED.starts_with("E_")
                && E_LAYOUT_DEDUPED.starts_with("E_"),
            "",
        );
    }

    {
        // 步进/格式/语义的字节算术自洽（描述器字节数算错就是静默的显存偏差）。
        let f = VertexFormat::Snorm16x2;
        let u = VertexFormat::Uint8x4;
        set.add(
            "A22-judge-字节算术自洽",
            f.byte_size() == 4
                && f.components() == 2
                && f.component_bytes() == 2
                && f.kind() == ScalarKind::Normalized
                && u.byte_size() == 4
                && u.components() == 4
                && u.component_bytes() == 1,
            "",
        );
    }

    set
}

/// 基准构建器（位置/法线/UV）——多处复用，避免各判据各写一份声明而漂移。
fn baseline_builder() -> VertexLayoutBuilder {
    let mut b = VertexLayoutBuilder::new();
    b.push(VertexSemantic::Position, VertexFormat::Float32x3, 0);
    b.push(VertexSemantic::Normal, VertexFormat::Float32x3, 1);
    b.push(VertexSemantic::Uv0, VertexFormat::Float32x2, 2);
    b
}

/// 蒙皮构建器（5 属性、40 字节）——读屏预览判据用。
fn skin_builder() -> VertexLayoutBuilder {
    let mut b = VertexLayoutBuilder::new();
    b.push(VertexSemantic::Position, VertexFormat::Float32x3, 0);
    b.push(VertexSemantic::BoneIndex, VertexFormat::Uint8x4, 1);
    b.push(VertexSemantic::BoneWeight, VertexFormat::Unorm8x4, 2);
    b.push(VertexSemantic::Color, VertexFormat::Unorm8x4, 3);
    b.push(VertexSemantic::Uv0, VertexFormat::Float32x2, 4);
    b
}

/// 取首个故障的码（自检内部用，避免 match 噪声）。
fn first_code(v: &SignatureVerdict) -> &'static str {
    match v {
        SignatureVerdict::Rejected { faults } => match faults.first() {
            Some(f) => f.code(),
            None => "",
        },
        SignatureVerdict::Consistent => "",
    }
}

/// 语义下标与约定的一致性检查（返回问题列表；空列表表示齐备）。
///
/// 单独成一个函数而不是只塞在构建器里：外部导入布局绕过构建器时仍要受检——
/// 构建器保证得了自己产的， 保证不了别人塞进来的。
pub fn check_semantic_index(attrs: &[ConstAttr]) -> Vec<String> {
    let mut issues: Vec<String> = Vec::new();
    let mut i = 0;
    while i < attrs.len() {
        let want = attrs[i].semantic.default_index();
        if attrs[i].semantic_index != want {
            issues.push(format!(
                "语义 {} 的下标 {} 与约定 {} 不符",
                attrs[i].semantic.tag(),
                attrs[i].semantic_index,
                want
            ));
        }
        i += 1;
    }
    issues
}
