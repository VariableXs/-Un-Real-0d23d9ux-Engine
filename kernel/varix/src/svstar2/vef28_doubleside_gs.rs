//! VE-F1625 · 双侧渲染与几何着色探测（VE-I 域 · I02 顶点流水线组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1625`
//!
//! 双侧渲染与几何着色探测极致深化：双面渲染语义（背面剔除开关/双面光
//! 照语义——跨后端一致）；几何着色能力探测（GS 支持度探测——能力表驱
//! 动降级，Metal 不支持传统 GS）；诚实标注（后端差异如实呈现——能力
//! 矩阵驱动而非假设）。
//!
//! ## 要点一：双面语义 = 剔除开关 + 法线翻转的成对语义
//!
//! 剔除开关三态（none/back/front）：cull none 时背面也被光栅化——双面
//! 渲染；此时光照公式必须配合**法线翻转语义**（背面片元法线取反），
//! 否则背面光照错向。两者是成对语义：开双面而不翻法线 = 伪双面。
//!
//! ## 要点二：跨后端一致的锚是我方统一绕序语义
//!
//! 本域统一声明 **CCW = 正面**；各后端原生默认不同（D3D12 默认 CW=
//! 正面，Vulkan/Metal 默认 CCW=正面）——差异封装在翻译表里（后端差
//! 异封装，F1626 约定承接深度范围/NDC 类差异的同一纪律），上层只见统
//! 一语义，不见后端暗差异。
//!
//! ## 要点三：GS 探测走能力表，不走假设
//!
//! 几何着色（GS）支持度来自**静态能力表**（探测纪律 F1208 范式：探
//! 测→能力表→降级→诚实标注）；Metal 不支持传统 GS——表驱动降级给
//! 出替代路径声明（实例化展开），不支持的域**管线降级或替代路径**，
//! 绝不假装支持。
//!
//! ## 要点四：诚实标注与能力矩阵同源
//!
//! 每条后端差异标注**从能力矩阵/翻译表派生生成**（同源），标注与矩
//! 阵矛盾即诊断码拒绝——「能力矩阵驱动而非假设」落成可判据的机制，
//! 不靠口头诚实。
//!
//! ## 要点五：零 panic 面
//!
//! 无 unwrap/expect/裸下标越界；wire 域外、零向量法线、标注矛盾一律
//! 专属码拒绝。
//!
//! ## 要点六：诊断码独占 0x4Cxx 段
//!
//! 全仓 grep 零占用后选定；与 vef23~vef27（0x40~0x44）/vea47~vea51
//! （0x47~0x4B）/vch01+（0x50+）互不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、双面渲染语义（剔除开关 + 法线翻转 + 跨后端翻译）
// ---------------------------------------------------------------------------

/// 背面剔除开关（三态：双面渲染语义的第一半）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CullMode {
    /// 不剔除（双面渲染——背面也光栅化）。
    None,
    /// 剔除背面（正面渲染——默认单面）。
    Back,
    /// 剔除正面（如洞穴内壁/阴影体渲染的用法）。
    Front,
}

impl CullMode {
    /// wire 码（判据侧独立对拍）。
    pub const fn wire(self) -> u8 {
        match self {
            CullMode::None => 0,
            CullMode::Back => 1,
            CullMode::Front => 2,
        }
    }

    /// wire 解码（域外拒——DEGENERATE 防线不收脏值）。
    pub fn from_wire(w: u8) -> Result<CullMode, DgCode> {
        match w {
            0 => Ok(CullMode::None),
            1 => Ok(CullMode::Back),
            2 => Ok(CullMode::Front),
            _ => Err(DgCode::CULL_WIRE_INVALID),
        }
    }

    /// 是否双面渲染（none ⇒ 双面）。
    pub const fn is_two_sided(self) -> bool {
        matches!(self, CullMode::None)
    }
}

/// 绕序（本域统一语义：**CCW = 正面**——跨后端一致的锚）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Winding {
    /// 逆时针（本域统一语义下的正面）。
    Ccw,
    /// 顺时针。
    Cw,
}

/// 渲染后端（三后端封闭——能力矩阵与翻译表的行域）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Direct3D 12（原生默认 CW=正面——与我方语义相反）。
    D3d12,
    /// Vulkan（原生默认 CCW=正面——与我方语义一致）。
    Vulkan,
    /// Metal（原生默认 CCW=正面；不支持传统 GS）。
    Metal,
}

/// 双面光照的作用面（法线翻转语义的自变量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacetSide {
    /// 正面（法线原样）。
    Front,
    /// 背面（法线取反——双面光照语义的核心）。
    Back,
}

/// 双面光照法线翻转：背面片元法线取反，正面原样。
///
/// 成对语义红线：cull none（双面）时调用方必须走本语义翻背面法线，
/// 否则背面光照错向（判据族 TWOSIDE 对拍）。零向量不可翻（拒绝）。
pub fn facing_normal_flip(
    normal: [f32; 3],
    side: FacetSide,
) -> Result<[f32; 3], DgCode> {
    let degenerate = normal[0] == 0.0 && normal[1] == 0.0 && normal[2] == 0.0;
    if degenerate {
        return Err(DgCode::DEGENERATE_NORMAL);
    }
    Ok(match side {
        FacetSide::Front => normal,
        FacetSide::Back => [-normal[0], -normal[1], -normal[2]],
    })
}

/// 后端原生剔除语义（翻译产物——差异封装在这里，不外泄）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeCull {
    /// 后端原生剔除态。
    pub cull: CullMode,
    /// 后端是否需翻转绕序定义才能等效我方「CCW=正面」。
    pub winding_flip: bool,
}

/// 统一语义 → 后端原生语义翻译表（3 后端 × 3 模式封闭九格）。
///
/// 差异封装纪律（F1626 约定的同一范式）：D3D12 原生默认 CW=正面，
/// 要等效我方 CCW=正面 必须翻转绕序定义（winding_flip=true）；
/// Vulkan/Metal 原生与我方一致（false）。剔除态本身逐后端直通。
pub const TRANSLATION: [[NativeCull; 3]; 3] = [
    // D3d12 行：winding_flip = true（原生 CW=正面）
    [
        NativeCull { cull: CullMode::None, winding_flip: true },
        NativeCull { cull: CullMode::Back, winding_flip: true },
        NativeCull { cull: CullMode::Front, winding_flip: true },
    ],
    // Vulkan 行：与我方语义一致
    [
        NativeCull { cull: CullMode::None, winding_flip: false },
        NativeCull { cull: CullMode::Back, winding_flip: false },
        NativeCull { cull: CullMode::Front, winding_flip: false },
    ],
    // Metal 行：与我方语义一致
    [
        NativeCull { cull: CullMode::None, winding_flip: false },
        NativeCull { cull: CullMode::Back, winding_flip: false },
        NativeCull { cull: CullMode::Front, winding_flip: false },
    ],
];

/// 后端在翻译表中的行号（封闭域内查表）。
pub const fn backend_row(b: Backend) -> usize {
    match b {
        Backend::D3d12 => 0,
        Backend::Vulkan => 1,
        Backend::Metal => 2,
    }
}

/// 剔除模式在翻译表中的列号（封闭域内查表）。
pub const fn cull_col(m: CullMode) -> usize {
    match m {
        CullMode::None => 0,
        CullMode::Back => 1,
        CullMode::Front => 2,
    }
}

/// 查翻译：统一语义 + 后端 ⇒ 原生语义（封闭九格全命中，无暗格）。
pub const fn translate(b: Backend, m: CullMode) -> NativeCull {
    TRANSLATION[backend_row(b)][cull_col(m)]
}

// ---------------------------------------------------------------------------
// 二、GS 能力探测（能力表驱动降级——F1208 探测纪律范式）
// ---------------------------------------------------------------------------

/// 几何着色支持度（能力表条目——探测产物，非假设）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GsSupport {
    /// 支持传统 GS（可原生走几何着色管线）。
    Full,
    /// 不支持传统 GS（Metal——锚点原文语义）。
    None,
}

/// GS 能力矩阵（静态 const 表——探测纪律 F1208：能力表驱动，不假设）。
pub const GS_CAPS: [(Backend, GsSupport); 3] = [
    (Backend::D3d12, GsSupport::Full),
    (Backend::Vulkan, GsSupport::Full),
    (Backend::Metal, GsSupport::None),
];

/// 查 GS 能力（线性扫封闭表——三行全在，越界域不存在）。
pub const fn gs_caps_of(b: Backend) -> GsSupport {
    let row = backend_row(b);
    GS_CAPS[row].1
}

/// GS 管线计划（能力表驱动的降级裁决——支持走原生，不支持走替代）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GsPlan {
    /// 是否原生走 GS。
    pub native: bool,
    /// 替代路径声明（不支持时非空——诚实降级；支持时 None）。
    pub alt_path: Option<&'static str>,
}

/// GS 降级替代路径（锚点语义：实例化展开替代传统 GS 的展开语义）。
pub const GS_ALT_PATH: &str = "实例化展开替代路径：以实例化重绘展开几何，替代传统 GS 的展开语义（能力表驱动降级，F1208 范式）";

/// GS 管线计划裁决：能力表查得支持度后分流。
pub fn gs_plan(b: Backend) -> Result<GsPlan, DgCode> {
    match gs_caps_of(b) {
        GsSupport::Full => Ok(GsPlan { native: true, alt_path: None }),
        GsSupport::None => Ok(GsPlan { native: false, alt_path: Some(GS_ALT_PATH) }),
    }
}

// ---------------------------------------------------------------------------
// 三、诚实标注（能力矩阵驱动而非假设——标注与矩阵同源可判据）
// ---------------------------------------------------------------------------

/// 标注主题（差异声明的分账维度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnoTopic {
    /// 双面/剔除语义差异（翻译表来源）。
    TwoSided,
    /// GS 支持度差异（能力矩阵来源）。
    GsSupport,
}

/// 一条后端差异标注（后端 + 主题 + 声明文本）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub backend: Backend,
    pub topic: AnnoTopic,
    pub statement: String,
}

/// 从翻译表派生某后端的双面语义标注（同源：文本含矩阵事实）。
pub fn annotate_twosided(b: Backend) -> Annotation {
    let row = backend_row(b);
    let flip = TRANSLATION[row][0].winding_flip;
    let flip_word = if flip { "需翻转绕序定义（原生默认 CW=正面）" } else { "绕序语义与我方一致（原生默认 CCW=正面）" };
    Annotation {
        backend: b,
        topic: AnnoTopic::TwoSided,
        statement: alloc::format!(
            "双面语义差异封装：{} 的剔除语义{}；统一语义锚为 CCW=正面（F1626 约定承接）",
            backend_name(b),
            flip_word
        ),
    }
}

/// 从 GS 能力矩阵派生某后端的 GS 标注（同源：文本含矩阵事实）。
pub fn annotate_gs(b: Backend) -> Annotation {
    let sup = gs_caps_of(b);
    let statement = match sup {
        GsSupport::Full => alloc::format!(
            "GS 支持度：{} 支持传统几何着色（能力表 Full，管线原生路径）",
            backend_name(b)
        ),
        GsSupport::None => alloc::format!(
            "GS 支持度：{} 不支持传统几何着色（能力表 None）——能力表驱动降级：{}",
            backend_name(b),
            GS_ALT_PATH
        ),
    };
    Annotation {
        backend: b,
        topic: AnnoTopic::GsSupport,
        statement,
    }
}

/// 全量诚实标注（6 条 = 3 后端 × 2 主题；逐条由矩阵派生——同源机制）。
pub fn annotations() -> Vec<Annotation> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < GS_CAPS.len() {
        let b = GS_CAPS[i].0;
        out.push(annotate_twosided(b));
        out.push(annotate_gs(b));
        i += 1;
    }
    out
}

/// 标注一致性裁决：标注与能力矩阵/翻译表逐点核对，矛盾即拒。
///
/// 「能力矩阵驱动而非假设」的机制化：任何手写标注入库前过本裁决，
/// 与矩阵矛盾的字条进不了账（判据族 HONESTY 反向语料对拍）。
pub fn verify_annotation(a: &Annotation) -> Result<(), DgCode> {
    if a.statement.is_empty() {
        return Err(DgCode::ANNOTATION_CONFLICT);
    }
    match a.topic {
        AnnoTopic::TwoSided => {
            let expect = translate(a.backend, CullMode::None).winding_flip;
            let says_flip = a.statement.contains("需翻转绕序定义");
            let says_same = a.statement.contains("绕序语义与我方一致");
            if (expect && says_flip && !says_same) || (!expect && says_same && !says_flip) {
                Ok(())
            } else {
                Err(DgCode::ANNOTATION_CONFLICT)
            }
        }
        AnnoTopic::GsSupport => {
            // 子串包含陷阱防护："不支持传统几何着色" 本身包含
            // "支持传统几何着色"——先判否定短语，再在否定缺席时判肯定。
            let says_none = a.statement.contains("不支持传统几何着色");
            let says_full = !says_none && a.statement.contains("支持传统几何着色");
            let expect_none = matches!(gs_caps_of(a.backend), GsSupport::None);
            if (expect_none && says_none && !says_full) || (!expect_none && says_full && !says_none) {
                Ok(())
            } else {
                Err(DgCode::ANNOTATION_CONFLICT)
            }
        }
    }
}

/// 后端人话名（标注文本与判据共用——单一出处）。
pub const fn backend_name(b: Backend) -> &'static str {
    match b {
        Backend::D3d12 => "D3D12",
        Backend::Vulkan => "Vulkan",
        Backend::Metal => "Metal",
    }
}

// ---------------------------------------------------------------------------
// 四、错误契约（独占 0x4Cxx 段）
// ---------------------------------------------------------------------------

/// vef28 诊断码。独占 `0x4Cxx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DgCode(pub u16);

impl DgCode {
    /// 剔除开关 wire 码域外。
    pub const CULL_WIRE_INVALID: DgCode = DgCode(0x4C01);
    /// 后端 wire 码域外。
    pub const BACKEND_WIRE_INVALID: DgCode = DgCode(0x4C02);
    /// GS 不可用且替代路径缺失（表封闭下的防御位）。
    pub const GS_NO_FALLBACK: DgCode = DgCode(0x4C03);
    /// 标注与能力矩阵矛盾（诚实标注机制化拒绝）。
    pub const ANNOTATION_CONFLICT: DgCode = DgCode(0x4C04);
    /// 零向量法线不可翻转。
    pub const DEGENERATE_NORMAL: DgCode = DgCode(0x4C05);
    /// 绕序 wire 码域外。
    pub const WINDING_WIRE_INVALID: DgCode = DgCode(0x4C06);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            DgCode::CULL_WIRE_INVALID => "剔除开关 wire 码域外：合法域 {0,1,2}".into(),
            DgCode::BACKEND_WIRE_INVALID => "后端 wire 码域外：合法域 {0,1,2}".into(),
            DgCode::GS_NO_FALLBACK => "GS 不可用且替代路径缺失：能力表封闭下不可达的防御位".into(),
            DgCode::ANNOTATION_CONFLICT => "标注与能力矩阵矛盾：诚实标注机制化拒绝入库".into(),
            DgCode::DEGENERATE_NORMAL => "零向量法线不可翻转：双面光照语义无定义".into(),
            DgCode::WINDING_WIRE_INVALID => "绕序 wire 码域外：合法域 {0,1}".into(),
            DgCode(_) => "未知 vef28 双面/GS 探测域诊断码".into(),
        }
    }
}

/// 后端 wire 解码（域外拒）。
pub fn backend_from_wire(w: u8) -> Result<Backend, DgCode> {
    match w {
        0 => Ok(Backend::D3d12),
        1 => Ok(Backend::Vulkan),
        2 => Ok(Backend::Metal),
        _ => Err(DgCode::BACKEND_WIRE_INVALID),
    }
}

/// 绕序 wire 解码（域外拒）。
pub fn winding_from_wire(w: u8) -> Result<Winding, DgCode> {
    match w {
        0 => Ok(Winding::Ccw),
        1 => Ok(Winding::Cw),
        _ => Err(DgCode::WINDING_WIRE_INVALID),
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑（双面语义 / GS 探测 / 诚实标注）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 剔除开关wire往返与域外拒() {
        for m in [CullMode::None, CullMode::Back, CullMode::Front] {
            assert_eq!(CullMode::from_wire(m.wire()), Ok(m));
        }
        assert_eq!(CullMode::from_wire(3), Err(DgCode::CULL_WIRE_INVALID));
    }

    #[test]
    fn 翻译表九格D3D翻转其余直通() {
        assert!(translate(Backend::D3d12, CullMode::None).winding_flip);
        assert!(!translate(Backend::Vulkan, CullMode::None).winding_flip);
        assert!(!translate(Backend::Metal, CullMode::Back).winding_flip);
    }

    #[test]
    fn 法线翻转背面取反正面原样() {
        let n = [0.0, 0.0, 1.0];
        assert_eq!(facing_normal_flip(n, FacetSide::Front), Ok(n));
        assert_eq!(facing_normal_flip(n, FacetSide::Back), Ok([0.0, 0.0, -1.0]));
        assert_eq!(facing_normal_flip([0.0; 3], FacetSide::Back), Err(DgCode::DEGENERATE_NORMAL));
    }

    #[test]
    fn metal不支持gs降级给替代路径() {
        assert_eq!(gs_caps_of(Backend::Metal), GsSupport::None);
        let p = gs_plan(Backend::Metal).unwrap();
        assert!(!p.native);
        assert!(p.alt_path.is_some());
        let p2 = gs_plan(Backend::Vulkan).unwrap();
        assert!(p2.native && p2.alt_path.is_none());
    }

    #[test]
    fn 标注与矩阵矛盾被拒() {
        let mut a = annotate_gs(Backend::Metal);
        assert_eq!(verify_annotation(&a), Ok(()));
        // 反向语料：把 Metal 的 GS 标注篡改成「支持」——与矩阵矛盾
        a.statement = "GS 支持度：Metal 支持传统几何着色（能力表 Full，管线原生路径）".into();
        assert_eq!(verify_annotation(&a), Err(DgCode::ANNOTATION_CONFLICT));
    }
}
