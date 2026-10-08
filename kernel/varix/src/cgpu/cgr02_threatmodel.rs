//! CGPU-F2722 · 威胁模型与分类（CGPU-R 域 · 安全渲染 · 批次 R01 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2722`
//!
//! **判据（锚点原文）**：五类、分类表、清单、两组、判据。
//!
//! **职责定位（锚点原文）**：威胁模型（恶意着色器/资源炸弹/越界/
//! 外泄/死循环——五类分类——分类表）；攻击面（渲染输入面清单——
//! 清单实现）；测试（分类/攻击面两组）。
//!
//! # 一、为什么威胁必须先分类而不是先拦
//!
//! 拦截是策略，分类是认知：不分类的拦截是黑盒武断（拦错了说不清
//! 为什么），分类后的拦截是规则执行（每类威胁有名字、有机制、有
//! 可机检的判定阈值）。五类**封闭**——恶意着色器/资源炸弹/越界/
//! 外泄/死循环——封闭集表外不立类：第墌六类是新任务不是本表的
//! 事，本表硬塞第六类就是越权改规格。
//!
//! # 二、分类表为什么每类绑定一个整数阈值
//!
//! 判定要可机检：**可机检 = 整数比较**。每类威胁绑定一个判定
//! 阈值（非白名单内在函数数/申请 KiB/迭代上界）——浮点比较在
//! no_std 内核面不可靠（舍入歧义），字符串判定不可判定。阈值
//! 字面量冻结在分类表里，判据侧独立对拍——表改了判据必红。
//!
//! # 三、攻击面清单为什么五面封闭
//!
//! 渲染输入面只有五个口子：着色器代码/资源申请/几何索引/输出
//! 目标/控制流——五面对五类威胁**一一承载**（每面至少拦一类，
//! 每类至少有一面拦）。面清单封闭=攻击面有界：清单外的输入不存在
//! 进管线的方式（不存在=不需要检查），清单内每面有显性检查点。
//!
//! # 四、特征为什么不含内容
//!
//! 分类只看元数据（计数/阈值/布尔位），不看像素与数据内容——
//! 与 F4407 采样口径同源：防线读的是行为的形状，不是内容的
//! 心事。内容审查是数据验证域（R02）的职责，边界外扩即越权
//! （cgr01 定位条款）。
//!
//! # 五、对接
//!
//! 上游 cgr01 四段架构（本单是「验证」段的分类依据表）；下游
//! F2723 信任分级（分类结果决定验证强度）、F2725 故障隔离
//! （拦截即隔离触发）、F2729 攻击模拟器（模拟本表五类场景）。
//! 诊断码接续 cgr01 的 0x57xx 段（0x5708 起，互不重叠）。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、五类威胁闭集（判据一：五类）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const THREAT_MODEL_VERSION: &str = "R02-threat-v1";

/// 五类威胁**封闭**分类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreatKind {
    /// 恶意着色器：调用表外内在函数或非常规指令模式。
    MaliciousShader,
    /// 资源炸弹：单次申请超限资源拖垮显存/带宽。
    ResourceBomb,
    /// 越界：索引/偏移声明域外读写。
    OutOfBounds,
    /// 外泄：渲染数据流向非本地输出面（网络/外设）。
    Exfiltration,
    /// 死循环：控制流无迭代上界或超上界。
    InfiniteLoop,
}

/// 五类威胁的中文名与一句话机制（分类表行内容，判据独立对拍）。
pub const THREAT_KINDS: [(&str, &str); 5] = [
    ("恶意着色器", "调用表外内在函数或非常规指令模式"),
    ("资源炸弹", "单次申请超限资源拖垮显存与带宽"),
    ("越界", "索引或偏移声明域外读写"),
    ("外泄", "渲染数据流向非本地输出面"),
    ("死循环", "控制流无迭代上界或超上界"),
];

/// 类数守恒（封闭集口径：枚举变体数 = 表行数 = 5）。
pub const THREAT_COUNT: usize = 5;

/// 威胁诊断码（R 域 0x57xx 段，接续 cgr01 的 0x5706）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreatCode(pub u16);

impl ThreatCode {
    /// 恶意着色器（0x5708——表外内在函数）。
    pub const MALICIOUS_SHADER: ThreatCode = ThreatCode(0x5708);
    /// 资源炸弹（0x5709——申请超限）。
    pub const RESOURCE_BOMB: ThreatCode = ThreatCode(0x5709);
    /// 越界（0x570A——声明域外索引）。
    pub const OUT_OF_BOUNDS: ThreatCode = ThreatCode(0x570A);
    /// 外泄（0x570B——非本地输出面）。
    pub const EXFILTRATION: ThreatCode = ThreatCode(0x570B);
    /// 死循环（0x570C——无迭代上界）。
    pub const INFINITE_LOOP: ThreatCode = ThreatCode(0x570C);
    /// 特征签名非法（0x570D——表外特征面拒绝）。
    pub const FEATURES_INVALID: ThreatCode = ThreatCode(0x570D);

    /// 人话（读屏可播报）。
    pub fn say(self) -> String {
        match self {
            ThreatCode::MALICIOUS_SHADER => "威胁拦截：恶意着色器（表外内在函数）".into(),
            ThreatCode::RESOURCE_BOMB => "威胁拦截：资源炸弹（申请超限）".into(),
            ThreatCode::OUT_OF_BOUNDS => "威胁拦截：越界（声明域外索引）".into(),
            ThreatCode::EXFILTRATION => "威胁拦截：外泄（非本地输出面）".into(),
            ThreatCode::INFINITE_LOOP => "威胁拦截：死循环（无迭代上界）".into(),
            ThreatCode::FEATURES_INVALID => "威胁分类拒绝：特征签名非法".into(),
            ThreatCode(_) => "未知威胁诊断码（R02 表外）".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、输入特征（分类的输入——只含元数据不含内容）
// ---------------------------------------------------------------------------

/// 死循环迭代上界红线（控制流上界超此即死循环类）。
pub const MAX_ITER_BOUND: u32 = 65_536;
/// 资源申请红线（KiB，单次申请超此即资源炸弹类）。
pub const MAX_RESOURCE_KIB: u64 = 262_144;

/// 输入特征签名（**只含检测所需元数据，不含像素与数据内容**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputFeatures {
    /// 着色器白名单之外的内在函数调用数。
    pub unknown_intrinsics: u32,
    /// 单次资源申请量（KiB）。
    pub resource_kib: u64,
    /// 几何索引是否声明域外。
    pub index_out_of_domain: bool,
    /// 是否存在非本地输出访问（网络/外设位）。
    pub external_access: bool,
    /// 控制流迭代上界（None = 上界缺失）。
    pub iter_bound: Option<u32>,
}

/// 特征合法性（伪造签名拦截：超红线千倍的申请量物理不可能——
/// 炸弹拦截在红线处，见到千倍值即特征面被篡改，拒绝分类）。
pub fn features_legal(f: &InputFeatures) -> bool {
    f.resource_kib <= MAX_RESOURCE_KIB.saturating_mul(1024)
}

// ---------------------------------------------------------------------------
// 三、分类表（判据二：分类表——每类一条可机检规则）
// ---------------------------------------------------------------------------

/// 分类规则（分类表行：一类一规则一阈值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreatRule {
    /// 威胁类别序（0..5，对齐 THREAT_KINDS）。
    pub kind_slot: usize,
    /// 特征名（人话）。
    pub feature: &'static str,
    /// 判定阈值（整数，字面量冻结）。
    pub threshold: u64,
    /// 检测面（攻击面清单对齐）。
    pub surface_slot: usize,
}

/// 分类表：五类五规则，阈值字面量冻结（判据侧独立对拍）。
pub const THREAT_RULES: [ThreatRule; 5] = [
    ThreatRule { kind_slot: 0, feature: "表外内在函数调用数", threshold: 0, surface_slot: 0 },
    ThreatRule { kind_slot: 1, feature: "单次资源申请 KiB", threshold: MAX_RESOURCE_KIB, surface_slot: 1 },
    ThreatRule { kind_slot: 2, feature: "索引声明域外", threshold: 1, surface_slot: 2 },
    ThreatRule { kind_slot: 3, feature: "非本地输出访问位", threshold: 1, surface_slot: 3 },
    ThreatRule { kind_slot: 4, feature: "迭代上界缺失或超限", threshold: MAX_ITER_BOUND as u64, surface_slot: 4 },
];

/// 分类（按规则表顺序逐条判定；无威胁 None——放行不是默认是判定）。
pub fn classify(f: &InputFeatures) -> Option<ThreatKind> {
    if !features_legal(f) {
        return None;
    }
    if f.unknown_intrinsics as u64 > THREAT_RULES[0].threshold {
        return Some(ThreatKind::MaliciousShader);
    }
    if f.resource_kib > THREAT_RULES[1].threshold {
        return Some(ThreatKind::ResourceBomb);
    }
    if f.index_out_of_domain {
        return Some(ThreatKind::OutOfBounds);
    }
    if f.external_access {
        return Some(ThreatKind::Exfiltration);
    }
    match f.iter_bound {
        None => Some(ThreatKind::InfiniteLoop),
        Some(b) if b as u64 > THREAT_RULES[4].threshold => Some(ThreatKind::InfiniteLoop),
        Some(_) => None,
    }
}

/// 分类结果的诊断码（拦截面用）。
pub fn code_of(k: ThreatKind) -> ThreatCode {
    match k {
        ThreatKind::MaliciousShader => ThreatCode::MALICIOUS_SHADER,
        ThreatKind::ResourceBomb => ThreatCode::RESOURCE_BOMB,
        ThreatKind::OutOfBounds => ThreatCode::OUT_OF_BOUNDS,
        ThreatKind::Exfiltration => ThreatCode::EXFILTRATION,
        ThreatKind::InfiniteLoop => ThreatCode::INFINITE_LOOP,
    }
}

// ---------------------------------------------------------------------------
// 四、攻击面清单（判据三：清单——五面封闭，面类承载）
// ---------------------------------------------------------------------------

/// 渲染输入面（五面封闭）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// 着色器代码（承载：恶意着色器）。
    ShaderCode,
    /// 资源申请（承载：资源炸弹）。
    ResourceClaim,
    /// 几何索引（承载：越界）。
    GeometryIndex,
    /// 输出目标（承载：外泄）。
    OutputTarget,
    /// 控制流（承载：死循环）。
    ControlFlow,
}

/// 攻击面清单：五面五承载（面→威胁类别序，一一对应）。
pub const SURFACE_LIST: [(&str, usize); 5] = [
    ("着色器代码", 0),
    ("资源申请", 1),
    ("几何索引", 2),
    ("输出目标", 3),
    ("控制流", 4),
];

/// 单面裁决。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceVerdict {
    /// 放行（本面特征在域内）。
    Pass,
    /// 拦截（带威胁码——进隔离，F2725 触发）。
    Block(ThreatCode),
}

/// 逐面扫描（清单封闭：恰好五条裁决，不多不少）。
pub fn surface_scan(f: &InputFeatures) -> Result<Vec<(Surface, SurfaceVerdict)>, ThreatCode> {
    if !features_legal(f) {
        return Err(ThreatCode::FEATURES_INVALID);
    }
    let mut out = Vec::new();
    out.push((Surface::ShaderCode, if f.unknown_intrinsics as u64 > THREAT_RULES[0].threshold {
        SurfaceVerdict::Block(ThreatCode::MALICIOUS_SHADER)
    } else {
        SurfaceVerdict::Pass
    }));
    out.push((Surface::ResourceClaim, if f.resource_kib > THREAT_RULES[1].threshold {
        SurfaceVerdict::Block(ThreatCode::RESOURCE_BOMB)
    } else {
        SurfaceVerdict::Pass
    }));
    out.push((Surface::GeometryIndex, if f.index_out_of_domain {
        SurfaceVerdict::Block(ThreatCode::OUT_OF_BOUNDS)
    } else {
        SurfaceVerdict::Pass
    }));
    out.push((Surface::OutputTarget, if f.external_access {
        SurfaceVerdict::Block(ThreatCode::EXFILTRATION)
    } else {
        SurfaceVerdict::Pass
    }));
    out.push((Surface::ControlFlow, match f.iter_bound {
        None => SurfaceVerdict::Block(ThreatCode::INFINITE_LOOP),
        Some(b) if b as u64 > THREAT_RULES[4].threshold => {
            SurfaceVerdict::Block(ThreatCode::INFINITE_LOOP)
        }
        Some(_) => SurfaceVerdict::Pass,
    }));
    Ok(out)
}

/// 拦截面计数（扫描汇总：O(面数)）。
pub fn blocked_count(v: &[(Surface, SurfaceVerdict)]) -> u32 {
    v.iter()
        .filter(|(_, d)| matches!(d, SurfaceVerdict::Block(_)))
        .count() as u32
}

// ---------------------------------------------------------------------------
// 五、读屏（人话摘要）
// ---------------------------------------------------------------------------

/// 威胁面人话单行（读屏可播报——面名+裁决）。
pub fn surface_line(s: Surface, d: &SurfaceVerdict) -> String {
    let name = match s {
        Surface::ShaderCode => "着色器代码",
        Surface::ResourceClaim => "资源申请",
        Surface::GeometryIndex => "几何索引",
        Surface::OutputTarget => "输出目标",
        Surface::ControlFlow => "控制流",
    };
    match d {
        SurfaceVerdict::Pass => alloc::format!("输入面 {}：放行（域内）", name),
        SurfaceVerdict::Block(c) => alloc::format!("输入面 {}：{}（{}）", name, c.say(), THREAT_MODEL_VERSION),
    }
}

// ---------------------------------------------------------------------------
// 六、单元测试（锚点两组：分类组 / 攻击面组）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn clean() -> InputFeatures {
        InputFeatures {
            unknown_intrinsics: 0,
            resource_kib: 1024,
            index_out_of_domain: false,
            external_access: false,
            iter_bound: Some(4096),
        }
    }

    // -- 分类组 --
    #[test]
    fn 分类组_五类逐条命中() {
        let mut f = clean();
        f.unknown_intrinsics = 1;
        assert_eq!(classify(&f), Some(ThreatKind::MaliciousShader));
        let mut f = clean();
        f.resource_kib = MAX_RESOURCE_KIB + 1;
        assert_eq!(classify(&f), Some(ThreatKind::ResourceBomb));
        let mut f = clean();
        f.index_out_of_domain = true;
        assert_eq!(classify(&f), Some(ThreatKind::OutOfBounds));
        let mut f = clean();
        f.external_access = true;
        assert_eq!(classify(&f), Some(ThreatKind::Exfiltration));
        let mut f = clean();
        f.iter_bound = None;
        assert_eq!(classify(&f), Some(ThreatKind::InfiniteLoop));
    }

    #[test]
    fn 分类组_无威胁放行_恰阈不拦() {
        assert_eq!(classify(&clean()), None);
        let mut f = clean();
        f.resource_kib = MAX_RESOURCE_KIB; // 恰阈不拦（> 语义）
        assert_eq!(classify(&f), None);
    }

    // -- 攻击面组 --
    #[test]
    fn 攻击面组_恰五面_拦截数对账() {
        let v = surface_scan(&clean()).expect("clean 特征合法");
        assert_eq!(v.len(), SURFACE_LIST.len());
        assert_eq!(blocked_count(&v), 0);
        let mut f = clean();
        f.unknown_intrinsics = 2;
        f.external_access = true;
        let v = surface_scan(&f).expect("合法");
        assert_eq!(blocked_count(&v), 2);
    }

    #[test]
    fn 攻击面组_伪造签名拒绝() {
        let mut f = clean();
        f.resource_kib = MAX_RESOURCE_KIB * 4096; // 超红线千倍=物理不可能
        assert!(!features_legal(&f));
        assert_eq!(surface_scan(&f), Err(ThreatCode::FEATURES_INVALID));
    }

    #[test]
    fn 攻击面组_全拦与码对账() {
        let f = InputFeatures {
            unknown_intrinsics: 9,
            resource_kib: MAX_RESOURCE_KIB * 4,
            index_out_of_domain: true,
            external_access: true,
            iter_bound: None,
        };
        let v = surface_scan(&f).expect("合法");
        assert_eq!(blocked_count(&v), 5);
        assert!(v.iter().all(|(_, d)| matches!(d, SurfaceVerdict::Block(_))));
    }
}
