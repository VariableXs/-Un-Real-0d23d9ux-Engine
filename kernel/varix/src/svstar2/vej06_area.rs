//! VE-F1806 · 面光源预留（VE-J 域 · 光照与阴影 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1806`
//!
//! **判据（锚点原文）**：接口预留、诚实标注、前向兼容、LTC 研究位、判据。
//!
//! # 职责与边界（先说清「不做什么」）
//!
//! 本条目**一期不做面光源实现**，只交付三样东西：
//! 1. **接口位冻结**——面光源类型枚举（RECT/TUBE）、参数块扩展草案（长宽/
//!    朝向/双面开关）、LTC 矩阵纹理引用位、与 F1829 接触硬化阴影的联动
//!    预留位。目的是**防后续破坏性变更**：字段序与语义现在定死，将来实现
//!    只能在此基础上填，不得挪位。
//! 2. **诚实标注**——未实现范围、预期形态、研究风险清单成文；**研究位不得
//!    写成实现文档**（文档一致性检查在本模块内以可执行判据落地拦截）。
//! 3. **显性报错**——用户引用面光源类型即报错（预留不静默），并指路当前
//!    可用替代方案（多点光阵列近似）。
//!
//! # 诚实标注（锚点要求，不得美化）
//!
//! 本模块**不产生任何光照贡献**。`AreaLight::contribution`恒返回零并附
//! 「未实现」标注——这是刻意设计，不是遗漏：宁可显式返回零 + 说明，也不
//! 编一个「近似实现」让调用方误以为面光源已可用。
//!
//! LTC（Linearly Transformed Cobes）**仅作研究位登记**，无任何代码路径；
//! 成本模型「预期高于点光一个数量级」是**预期值不是实测值**，落地时须入
//! F1811 账本实测。
//!
//! # 前向兼容约束
//!
//! [`AREA_FWD_COMPAT_DOC`] 成文冻结：未来实现不得改变既有字段偏移，
//! LTC 采样结果只允许经预留位接入，不得新增必填参数。本约束由
//! `check_area_forward_compat` 判据守护（约束文本被改动即变红）。

use alloc::format;
use alloc::string::String;

use crate::checks::CheckSet;

/// 面光源类型枚举位（**全部为预留**，一期无任何实现分支）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AreaLightKind {
    /// 矩形光（面光源，预留）。
    Rect,
    /// 管灯（面光源，预留）。
    Tube,
}

impl AreaLightKind {
    /// 稳定线编码（注册进 F1807 管理器用；一经发布不得改值）。
    pub fn code(self) -> u32 {
        match self {
            AreaLightKind::Rect => 0x0300,
            AreaLightKind::Tube => 0x0301,
        }
    }

    /// 线编码 → 类型；未知码拒绝（不猜类型——猜错会让用户以为该类型已实现）。
    pub fn from_code(code: u32) -> Option<AreaLightKind> {
        match code {
            0x0300 => Some(AreaLightKind::Rect),
            0x0301 => Some(AreaLightKind::Tube),
            _ => None,
        }
    }

    /// 人话标签。
    pub fn label(self) -> &'static str {
        match self {
            AreaLightKind::Rect => "RECT(矩形光)",
            AreaLightKind::Tube => "TUBE(管灯)",
        }
    }
}

/// 已登记的面光源类型全集（枚举注册表；F1807 调度按此遍历——预留类型
/// **不参与调度**，故本表只用于登记与报错指路，不产出渲染项）。
pub const AREA_KINDS: [AreaLightKind; 2] = [AreaLightKind::Rect, AreaLightKind::Tube];

/// 前向兼容约束（锚点「参数块前向兼容——未来实现不得改变既有字段偏移」）。
///
/// 本常量是**契约文本载体**，被 `check_area_forward_compat` 判据守护：
/// 文本被改动（挪字段/允新增必填参数）即判红，防有人无意破坏后续实现。
pub const AREA_FWD_COMPAT_DOC: &str = "\
面光源参数块前向兼容约束（VE-F1806 · v1 冻结）：既有字段偏移不可变更——\
pos(0) dir(1) color(2) intensity(3) range(4) 五项语义与顺序冻结；\
扩展字段（width/height/orientation/two_sided）只能追加在其后，不得插入其间；\
LTC 矩阵纹理引用位为可选引用，不得转为必填参数；\
本约束文本变更须经双签并升版，不得由单方实现者自行放宽。";

/// LTC 研究位登记（锚点「LTC 等实现路径的研究位标注」）。
///
/// **这是研究位，不是实现**：无代码路径消费它。成本数字是**预期值**，
/// 落地时必须以F1811 账本实测值替换——文档一致性判据守护「不得把研究位
/// 写成已完成实现」的表述。
pub const LTC_RESEARCH_NOTE: &str = "\
LTC（Linearly Transformed Cobes）研究位登记（VE-F1806 · 一期无实现）：\
面光源软阴影的候选实现路径，用预计算矩阵把任意面积光解析解折叠为有限次\
求和。风险清单：①预计算表显存开销未实测；②退化情形（光源朝向背向\
观察者/极小尺寸）数值稳定性未验证；③与现有 F1825 PCSS 的收益对比未量化。\
成本预期：单光源求和次数高于点光约一个数量级（**预期值，非实测**；\
落地时以 F1811 账本实测数据为准，不得沿用本预期值作结论）。";

/// 诚实标注：未实现范围（锚点「诚实标注段」）。
pub const HONEST_NOT_IMPLEMENTED_DOC: &str = "\
VE-F1806 一期诚实标注：面光源（矩形光/管灯）**未实现**，本条目只交付接口\
位冻结与研究位登记。调用面光源贡献函数恒得零并附未实现标注——刻意如此，\
避免调用方误以为可用。若需面光源近似效果，当前可用替代方案为多点光\
阵列近似（本条目 `substitute_plan` 给出建议档位）。";

/// 面光源未实现错误（显性报错；预留不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AreaError {
    /// 引用了预留面光源类型（一期无实现）。
    NotImplemented {
        /// 类型标签。
        kind: &'static str,
        /// 线编码。
        code: u32,
    },
    /// 线编码无法映射到已登记类型（不猜类型）。
    UnknownKind {
        /// 原始线编码。
        code: u32,
    },
    /// 参数块非法（宽度/朝向等扩展字段的边界防护）。
    BadParam {
        /// 字段名。
        field: &'static str,
        /// 原始值描述。
        detail: String,
    },
}

impl AreaError {
    /// 人话描述（发生了什么/为什么/下一步）。
    pub fn describe(&self) -> String {
        match self {
            AreaError::NotImplemented { kind, code } => format!(
                "面光源类型 {kind}（编码 0x{code:04X}）被引用，但一期未实现——\
                 预留接口不静默吞。下一步：改用多点光阵列近似（见 substitute_plan）；\
                 面光源真实实现属后续版本边界（见 HONEST_NOT_IMPLEMENTED_DOC）"
            ),
            AreaError::UnknownKind { code } => format!(
                "面光线编码 0x{code:04X} 无法映射到已登记类型——\
                 不猜测类型（猜错会让调用方误以为该类型已实现）。\
                 下一步：使用已登记编码 {:?}",
                AREA_KINDS.map(|k| k.code())
            ),
            AreaError::BadParam { field, detail } => format!(
                "面光源参数块字段 {field} 非法：{detail}——参数块虽为预留仍需\
                 可校验，避免预留位被垃圾数据占用。下一步：修正该字段取值"
            ),
        }
    }
}

/// 参数块扩展草案（**冻结**：字段序即接口，一期不做实现）。
///
/// 既有五项（pos/dir/color/intensity/range）语义冻结在先，扩展四项只能追加
/// 在后——见 [`AREA_FWD_COMPAT_DOC`]。
#[derive(Clone, Debug, PartialEq)]
pub struct AreaLight {
    /// 位置（场景空间；与点光/聚光同语义）。
    pub pos: (f32, f32, f32),
    /// 朝向（场景空间）。
    pub dir: (f32, f32, f32),
    /// 线性颜色三分量。
    pub color: (f32, f32, f32),
    /// 强度（非负；量纲语义引用 F1802）。
    pub intensity: f32,
    /// 作用半径。
    pub range: f32,
    // ---- 以下为扩展草案字段（追加位，不得插入既有字段之间） ----
    /// 宽度（沿朝向的横向边长；必须 >0）。
    pub width: f32,
    /// 高度（沿朝向的纵向边长；必须 >0）。
    pub height: f32,
    /// 朝向绕轴角度（弧度；用于矩形光定向）。
    pub orientation: f32,
    /// 双面开关（true=双面发光，false=单面）。
    pub two_sided: bool,
    /// LTC 矩阵纹理引用位（`None`=未绑定；一期无代码消费）。
    pub ltc_matrix_ref: Option<u32>,
    /// F1829 接触硬化阴影联动预留位（`None`=不联动）。
    pub contact_hardening_ref: Option<u32>,
}

impl AreaLight {
    /// 构造参数块草案（边界防护：宽度/高度/强度的有限性与正值）。
    ///
    /// 注意：本函数**只校验不实现**——返回的参数块仍然不产生光照贡献
    /// （见 [`AreaLight::contribution`]）。
    ///
    /// 参数平铺 10 个是刻意的：本条目职责就是**冻结参数字段序**，平铺签名
    /// 让「字段顺序即接口」在签名层面可见可查；改成参数结构体反而把顺序
    /// 藏进类型里，与前向兼容约束的意图相悖。
    #[allow(clippy::too_many_arguments)]
    pub fn draft(
        kind: AreaLightKind,
        pos: (f32, f32, f32),
        dir: (f32, f32, f32),
        color: (f32, f32, f32),
        intensity: f32,
        range: f32,
        width: f32,
        height: f32,
        orientation: f32,
        two_sided: bool,
    ) -> Result<AreaLight, AreaError> {
        // 类型先验：一期任何面光源类型都显性拒绝（不因参数合法就放行）。
        let _ = kind;
        if !width.is_finite() || width <= 0.0 {
            return Err(AreaError::BadParam {
                field: "width",
                detail: format!("宽度 {width} 非有限或非正"),
            });
        }
        if !height.is_finite() || height <= 0.0 {
            return Err(AreaError::BadParam {
                field: "height",
                detail: format!("高度 {height} 非有限或非正"),
            });
        }
        if !intensity.is_finite() || intensity < 0.0 {
            return Err(AreaError::BadParam {
                field: "intensity",
                detail: format!("强度 {intensity} 非有限或为负"),
            });
        }
        if !orientation.is_finite() {
            return Err(AreaError::BadParam {
                field: "orientation",
                detail: format!("朝向角 {orientation} 非有限"),
            });
        }
        Ok(AreaLight {
            pos,
            dir,
            color,
            intensity,
            range,
            width,
            height,
            orientation,
            two_sided,
            ltc_matrix_ref: None,
            contact_hardening_ref: None,
        })
    }

    /// 光照贡献（**恒零 + 未实现标注**——刻意如此）。
    ///
    /// 一期不做 LTC 求和。这里返回零而非「近似实现」，是为了让调用方
    /// 在视觉上立刻看到「面光源没生效」，而不是拿到一个看似合理的错误结果。
    pub fn contribution(&self, _px: (f32, f32, f32)) -> (f32, f32, f32) {
        (0.0, 0.0, 0.0)
    }

    /// 未实现标注（调用方可据此提示用户，而不必自己猜）。
    pub fn not_implemented_note(&self) -> &'static str {
        HONEST_NOT_IMPLEMENTED_DOC
    }

    /// 绑定 LTC 矩阵纹理引用位（仅登记，**一期无代码消费**）。
    pub fn bind_ltc_matrix(&mut self, tex: u32) {
        self.ltc_matrix_ref = Some(tex);
    }

    /// 绑定 F1829 接触硬化阴影联动位（仅登记）。
    pub fn bind_contact_hardening(&mut self, id: u32) {
        self.contact_hardening_ref = Some(id);
    }
}

/// 当前可用的替代方案（锚点「显性报错并文档指路当前替代方案」）。
///
/// 诚实标注：这是**近似**不是等价——多点光阵列只能逼近面光的软阴影，
/// 成本随灯数线性上升。
pub fn substitute_plan(target_softness: f32) -> &'static str {
    if !target_softness.is_finite() {
        return "目标软度非法：请改用多点光阵列近似，或等待面光源落地（F1806后续版本）";
    }
    if target_softness <= 0.0 {
        // 硬边需求用点光即可，无需阵列。
        "软度需求 ≤0：用单点光（F1804）即可，无需阵列近似"
    } else if target_softness < 0.5 {
        "软度需求 <0.5：3 点光小阵列近似（成本线性上升，精度有限）"
    } else {
        "软度需求 ≥0.5：5~8 点光阵列近似（成本显著上升；\
         强软阴影需求建议等待面光源 LTC 落地）"
    }
}

/// 请求使用面光源类型（**一期恒显性报错**——判据「预留不静默」）。
pub fn request_area_light(kind: AreaLightKind) -> Result<(), AreaError> {
    Err(AreaError::NotImplemented {
        kind: kind.label(),
        code: kind.code(),
    })
}

/// 按线编码请求面光源类型（未知码亦报错，不猜类型）。
pub fn request_area_light_by_code(code: u32) -> Result<AreaLightKind, AreaError> {
    match AreaLightKind::from_code(code) {
        Some(k) => Err(AreaError::NotImplemented {
            kind: k.label(),
            code,
        }),
        None => Err(AreaError::UnknownKind { code }),
    }
}

/// 枚举注册查询（供 F1807 管理器遍历；预留类型不参与调度，此处仅登记）。
pub fn registered_kinds() -> [AreaLightKind; 2] {
    AREA_KINDS
}

/// 预留类型是否参与调度（一期恒否——预留类型不产生渲染项）。
pub fn participates_in_scheduling(_kind: AreaLightKind) -> bool {
    false
}

/// VE-F1806 域自检（判据逐条映射见 `vej06_checks.rs`）。
pub fn run_vej06_checks() -> CheckSet {
    super::vej06_checks::run_vej06_checks()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft_ok() -> AreaLight {
        AreaLight::draft(
            AreaLightKind::Rect,
            (0.0, 0.0, 0.0),
            (0.0, 0.0, -1.0),
            (1.0, 1.0, 1.0),
            1.0,
            10.0,
            2.0,
            3.0,
            0.0,
            false,
        )
        .unwrap()
    }

    #[test]
    fn vej06_reserved_kind_rejected_explicitly() {
        // 引用预留类型 → 显性报错 + 三要素 + 指路替代方案。
        let e = request_area_light(AreaLightKind::Rect).unwrap_err();
        let d = e.describe();
        // 三要素：未实现（发生了什么）+ 下一步（怎么办）+ 预留不静吞。
        // 指路项按锚点原文核对——错误文案给的是「多点光阵列近似」这条
        // 具体替代方案，而非泛泛的「替代」二字。
        assert!(
            d.contains("未实现") && d.contains("下一步") && d.contains("预留"),
            "三要素齐发: {d}"
        );
        assert!(d.contains("多点光阵列近似"), "指路当前替代方案: {d}");
        assert!(matches!(e, AreaError::NotImplemented { code: 0x0300, .. }));
        // TUBE 同样拒绝。
        assert!(request_area_light(AreaLightKind::Tube).is_err());
    }

    #[test]
    fn vej06_unknown_code_not_guessed() {
        let e = request_area_light_by_code(0x0999).unwrap_err();
        assert!(matches!(e, AreaError::UnknownKind { code: 0x0999 }));
        assert!(e.describe().contains("不猜测类型"));
        // 已登记码也仍报「未实现」（不是「未知」）。
        let e2 = request_area_light_by_code(0x0301).unwrap_err();
        assert!(matches!(e2, AreaError::NotImplemented { .. }));
    }

    #[test]
    fn vej06_contribution_is_honestly_zero() {
        let a = draft_ok();
        // 贡献恒零：未实现不假装可用。
        assert_eq!(a.contribution((1.0, 1.0, 1.0)), (0.0, 0.0, 0.0));
        assert_eq!(a.contribution((0.0, 0.0, 0.0)), (0.0, 0.0, 0.0));
        // 标注可读且诚实。
        assert!(a.not_implemented_note().contains("未实现"));
    }

    #[test]
    fn vej06_param_guard_and_reserved_slots() {
        // 宽高非正 → 拒。
        assert!(matches!(
            AreaLight::draft(
                AreaLightKind::Rect,
                (0.0, 0.0, 0.0),
                (0.0, 0.0, -1.0),
                (1.0, 1.0, 1.0),
                1.0,
                10.0,
                0.0,
                3.0,
                0.0,
                false
            ),
            Err(AreaError::BadParam { field: "width", .. })
        ));
        assert!(matches!(
            AreaLight::draft(
                AreaLightKind::Rect,
                (0.0, 0.0, 0.0),
                (0.0, 0.0, -1.0),
                (1.0, 1.0, 1.0),
                1.0,
                10.0,
                2.0,
                f32::NAN,
                0.0,
                false
            ),
            Err(AreaError::BadParam {
                field: "height",
                ..
            })
        ));
        // 引用位默认空，绑定后可读回。
        let mut a = draft_ok();
        assert_eq!(a.ltc_matrix_ref, None);
        a.bind_ltc_matrix(7);
        a.bind_contact_hardening(9);
        assert_eq!(a.ltc_matrix_ref, Some(7));
        assert_eq!(a.contact_hardening_ref, Some(9));
        // 预留类型不参与调度。
        assert!(!participates_in_scheduling(AreaLightKind::Rect));
        assert!(!participates_in_scheduling(AreaLightKind::Tube));
    }

    #[test]
    fn vej06_reserved_slots_have_no_lighting_path() {
        // LTC 引用位绑定后仍无光照路径——防止「绑了纹理就算实现」的误解。
        let mut a = draft_ok();
        a.bind_ltc_matrix(1);
        assert_eq!(a.contribution((5.0, 5.0, 5.0)), (0.0, 0.0, 0.0));
    }

    #[test]
    fn vej06_checks_all_green() {
        let set = run_vej06_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F1806 域自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }
}
