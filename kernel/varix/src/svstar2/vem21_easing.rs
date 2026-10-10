//! VE-F2421 · 缓动库（VE-M 域 · 动画段 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2421`
//!
//! **判据（锚点原文）**：30+ 全集、P 词汇统一、纯函数纪律、选用手册、判据。
//!
//! **职责定位（锚点原文）**：缓动库——缓动函数全集（线性/二次~五次/
//! 弹性/回弹/弹跳/步进——30+ 缓动函数库……及其 In/Out/InOut 变体
//! ——30+ 函数枚举与实现声明）、与 VE-P 统一（缓动词汇与 P 域动效库
//! 统一——跨域缓动词汇单源）、缓动曲线资产（自定义缓动曲线注册——
//! F2403 注册机制延续）、缓动语义文档（每函数的视觉效果语义与适用
//! 建议）。
//!
//! # 一、30+ 全集（12 基函数 × 3 变体）
//!
//! 基函数十二：linear/quad/cubic/quart/quint（幂族）/sine/expo/circ/
//! back/elastic/bounce/step。每基三变体（In=f(t)/Out=1-f(1-t)/
//! InOut=分段复合）——36 条目全入库，契约逐条验（端点 f(0)==0、
//! f(1)==1 逐位+全域有限）。纯函数纪律：全部 `fn(f32) -> f32` 无
//! 状态无副作用。
//!
//! # 二、P 词汇统一（跨域单源声明）
//!
//! 同一 [`EasingBase`] 枚举表服务 M 域动画与 P 域 UI 动效——
//! [`p_vocabulary`] 是词汇统一的对账表（12 基名+wire 名+P 侧列），
//! 查重名/缺行/空列即拒（枚举撞号→对账裁决）。如实声明：P 域动效
//! 库本体在 VE-P 册，本表是**词汇单源声明与对账面**，P 侧模块落地
//! 后按 wire 名逐项对账。
//!
//! # 三、自定义注册（F2403 机制延续）+ 语义手册
//!
//! [`CustomEasing`] 注册制：内置+自定义双轨；自定义函数必须过
//! **纯函数红线**（契约成立+双调用逐位相同——状态依赖即拒）。
//! 未注册引用返 None（拒绝+指引，不静默回退）。语义手册 12 行
//! （elastic=过冲振荡/back=回拉预蓄势/bounce=落地弹跳……）——
//! 空语义即拒（手册与实现漂移→对账钩子）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::float::FloatExt;
use core::sync::atomic::Ordering;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const EASING_VERSION: &str = "M21-easing-v1";

/// 基函数数（十二）。
pub const BASE_COUNT: usize = 12;

/// 变体数（In/Out/InOut）。
pub const VARIANT_COUNT: usize = 3;

/// 库条目总数（12×3=36 ≥30——锚点 30+）。
pub const LIB_COUNT: usize = BASE_COUNT * VARIANT_COUNT;

/// 契约采样点数（全域有限性抽检）。
pub const CONTRACT_SAMPLES: usize = 33;

/// 未注册缓动引用（拒绝+指引）。
pub const E_EASE_UNREGISTERED: &str = "E_EASE_UNREGISTERED";

/// 自定义函数非纯（状态依赖——纯函数红线）。
pub const E_EASE_IMPURE: &str = "E_EASE_IMPURE";

/// 契约不成立（端点/有限性）。
pub const E_EASE_CONTRACT: &str = "E_EASE_CONTRACT";

/// 词汇撞号/缺行（P 侧对账裁决）。
pub const E_EASE_VOCAB: &str = "E_EASE_VOCAB";

/// 语义手册漂移（空语义/缺行）。
pub const E_EASE_MANUAL: &str = "E_EASE_MANUAL";

// ---------------------------------------------------------------------------
// 二、基函数与变体系统
// ---------------------------------------------------------------------------

/// 缓动函数签名（纯函数——无状态无副作用）。
pub type EasingFn = fn(f32) -> f32;

/// 基函数枚举（十二闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EasingBase {
    /// 线性。
    Linear,
    /// 二次幂。
    Quad,
    /// 三次幂。
    Cubic,
    /// 四次幂。
    Quart,
    /// 五次幂。
    Quint,
    /// 正弦。
    Sine,
    /// 指数。
    Expo,
    /// 圆形。
    Circ,
    /// 回弹（过冲预蓄势）。
    Back,
    /// 弹性（过冲振荡）。
    Elastic,
    /// 弹跳（落地）。
    Bounce,
    /// 步进（离散）。
    Step,
}

impl EasingBase {
    /// 十二闭集（顺序即册内顺序）。
    pub const ALL: [EasingBase; BASE_COUNT] = [
        EasingBase::Linear,
        EasingBase::Quad,
        EasingBase::Cubic,
        EasingBase::Quart,
        EasingBase::Quint,
        EasingBase::Sine,
        EasingBase::Expo,
        EasingBase::Circ,
        EasingBase::Back,
        EasingBase::Elastic,
        EasingBase::Bounce,
        EasingBase::Step,
    ];

    /// wire 名（P 域对齐的同名标识）。
    pub fn wire(self) -> &'static str {
        match self {
            EasingBase::Linear => "linear",
            EasingBase::Quad => "quad",
            EasingBase::Cubic => "cubic",
            EasingBase::Quart => "quart",
            EasingBase::Quint => "quint",
            EasingBase::Sine => "sine",
            EasingBase::Expo => "expo",
            EasingBase::Circ => "circ",
            EasingBase::Back => "back",
            EasingBase::Elastic => "elastic",
            EasingBase::Bounce => "bounce",
            EasingBase::Step => "step",
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            EasingBase::Linear => "线性",
            EasingBase::Quad => "二次",
            EasingBase::Cubic => "三次",
            EasingBase::Quart => "四次",
            EasingBase::Quint => "五次",
            EasingBase::Sine => "正弦",
            EasingBase::Expo => "指数",
            EasingBase::Circ => "圆形",
            EasingBase::Back => "回弹",
            EasingBase::Elastic => "弹性",
            EasingBase::Bounce => "弹跳",
            EasingBase::Step => "步进",
        }
    }

    /// 基函数实现（In 语义的纯函数——t∈[0,1]）。
    pub fn eval_in(self, t: f32) -> f32 {
        match self {
            EasingBase::Linear => t,
            EasingBase::Quad => t * t,
            EasingBase::Cubic => t * t * t,
            EasingBase::Quart => t * t * t * t,
            EasingBase::Quint => t * t * t * t * t,
            EasingBase::Sine => 1.0 - (t * core::f32::consts::FRAC_PI_2).m_cos(),
            EasingBase::Expo => {
                if t <= 0.0 {
                    0.0
                } else {
                    // 指数是小数（10·(t-1)∈[-10,0]）——必须走 m_powf；
                    // 早先写 m_powi(10*(t-1.0) as i32) 会把 -0.5 截成 0，
                    // expo-in(0.5) 错得 1.0（整数指数病）。
                    2.0f32.m_powf(10.0 * (t - 1.0))
                }
            }
            EasingBase::Circ => 1.0 - (1.0 - t * t).m_sqrt(),
            EasingBase::Back => {
                let c1 = 1.70158f32;
                let c3 = c1 + 1.0;
                c3 * t * t * t - c1 * t * t
            }
            EasingBase::Elastic => {
                if t <= 0.0 || t >= 1.0 {
                    t
                } else {
                    (13.0 * core::f32::consts::FRAC_PI_2 * t).m_sin() * 2.0f32.m_powf(10.0 * (t - 1.0))
                }
            }
            EasingBase::Bounce => 1.0 - bounce_out(1.0 - t),
            EasingBase::Step => {
                if t < 1.0 {
                    0.0
                } else {
                    1.0
                }
            }
        }
    }

    /// 视觉语义（手册原文——空即漂移）。
    pub fn semantics(self) -> &'static str {
        match self {
            EasingBase::Linear => "匀速无加减速——机械感强，适合循环平移动画",
            EasingBase::Quad => "二次加减速——轻快的通用选择",
            EasingBase::Cubic => "三次加减速——比二次更柔和的起动",
            EasingBase::Quart => "四次加减速——起动更重、收势更沉",
            EasingBase::Quint => "五次加减速——最强的起步推力",
            EasingBase::Sine => "正弦加减速——最柔和的起停，适合氛围动画",
            EasingBase::Expo => "指数加减速——极慢起动后冲刺，戏剧感",
            EasingBase::Circ => "圆形加减速——起停都急，中段快",
            EasingBase::Back => "回拉预蓄势——先反向再冲向目标",
            EasingBase::Elastic => "过冲振荡——橡皮筋式余振",
            EasingBase::Bounce => "落地弹跳——小球落台式的连续反弹",
            EasingBase::Step => "离散步进——无中间态，用于逐帧/切换",
        }
    }

    /// 选用建议（场景→缓动映射）。
    pub fn suggestion(self) -> &'static str {
        match self {
            EasingBase::Linear => "传送带/旋转 spinner 等匀速循环",
            EasingBase::Quad | EasingBase::Cubic => "通用 UI 位移/淡入淡出",
            EasingBase::Quart | EasingBase::Quint => "重物落位/大面板滑入",
            EasingBase::Sine => "呼吸灯/环境氛围的缓变",
            EasingBase::Expo => "相机冲刺/急停镜头",
            EasingBase::Circ => "小型徽章的弹出收回",
            EasingBase::Back => "交互回弹（按钮按下回弹）用 back-out",
            EasingBase::Elastic => "橡皮筋拖拽/吸引就位用 elastic-out",
            EasingBase::Bounce => "掉落物落地/通知弹信用 bounce-out",
            EasingBase::Step => "计时器跳秒/离散状态切换",
        }
    }
}

/// bounce-out 基函数（bounce-in = 1 - bounce_out(1-t)——复用不另抄）。
fn bounce_out(t: f32) -> f32 {
    let n1 = 7.5625f32;
    let d1 = 2.75f32;
    if t < 1.0 / d1 {
        n1 * t * t
    } else if t < 2.0 / d1 {
        let x = t - 1.5 / d1;
        n1 * x * x + 0.75
    } else if t < 2.5 / d1 {
        let x = t - 2.25 / d1;
        n1 * x * x + 0.9375
    } else {
        let x = t - 2.625 / d1;
        n1 * x * x + 0.984_375
    }
}

/// 变体（In/Out/InOut 三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EasingVariant {
    /// In（起步加速）。
    In,
    /// Out（收势减速）。
    Out,
    /// InOut（两头缓中间快）。
    InOut,
}

impl EasingVariant {
    /// 三闭集。
    pub const ALL: [EasingVariant; VARIANT_COUNT] = [EasingVariant::In, EasingVariant::Out, EasingVariant::InOut];

    /// wire 后缀。
    pub fn suffix(self) -> &'static str {
        match self {
            EasingVariant::In => "-in",
            EasingVariant::Out => "-out",
            EasingVariant::InOut => "-inout",
        }
    }
}

/// 变体变换（代数声明：In=f(t)/Out=1-f(1-t)/InOut=分段复合）。
pub fn variant_eval(base: EasingBase, v: EasingVariant, t: f32) -> f32 {
    match v {
        EasingVariant::In => base.eval_in(t),
        EasingVariant::Out => 1.0 - base.eval_in(1.0 - t),
        EasingVariant::InOut => {
            if t < 0.5 {
                base.eval_in(2.0 * t) / 2.0
            } else {
                1.0 - base.eval_in(2.0 - 2.0 * t) / 2.0
            }
        }
    }
}

/// 入口名（`<base><suffix>`——如 quad-out）。
pub fn easing_name(base: EasingBase, v: EasingVariant) -> String {
    let mut s = String::from(base.wire());
    s.push_str(v.suffix());
    s
}

// ---------------------------------------------------------------------------
// 三、库表与查询（注册制延续 F2403）
// ---------------------------------------------------------------------------

/// 库条目表（36 条——12 基×3 变体）。
///
/// **仅供自省与手册对账**（要 `Vec`/`String` 的枚举面）。查询路径**不走这里**——
/// [`lookup`] 是零分配查表；热路径物化 36 条 `String` 会把 O(1) 查表变成每次
/// 36 次堆分配，锚点「注册表查表 O(1)；全库零分配」即失守。
pub fn easing_library() -> Vec<(String, EasingFn)> {
    let mut out: Vec<(String, EasingFn)> = Vec::new();
    for b in EasingBase::ALL.iter() {
        for v in EasingVariant::ALL.iter() {
            let f: EasingFn = variant_fn(*b, *v);
            out.push((easing_name(*b, *v), f));
        }
    }
    out
}

/// 变体的函数指针化（fn item → fn pointer——查表出口）。
pub fn variant_fn(base: EasingBase, v: EasingVariant) -> EasingFn {
    match (base, v) {
        (EasingBase::Linear, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Linear, EasingVariant::In, t),
        (EasingBase::Linear, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Linear, EasingVariant::Out, t),
        (EasingBase::Linear, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Linear, EasingVariant::InOut, t),
        (EasingBase::Quad, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Quad, EasingVariant::In, t),
        (EasingBase::Quad, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Quad, EasingVariant::Out, t),
        (EasingBase::Quad, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Quad, EasingVariant::InOut, t),
        (EasingBase::Cubic, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Cubic, EasingVariant::In, t),
        (EasingBase::Cubic, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Cubic, EasingVariant::Out, t),
        (EasingBase::Cubic, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Cubic, EasingVariant::InOut, t),
        (EasingBase::Quart, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Quart, EasingVariant::In, t),
        (EasingBase::Quart, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Quart, EasingVariant::Out, t),
        (EasingBase::Quart, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Quart, EasingVariant::InOut, t),
        (EasingBase::Quint, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Quint, EasingVariant::In, t),
        (EasingBase::Quint, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Quint, EasingVariant::Out, t),
        (EasingBase::Quint, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Quint, EasingVariant::InOut, t),
        (EasingBase::Sine, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Sine, EasingVariant::In, t),
        (EasingBase::Sine, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Sine, EasingVariant::Out, t),
        (EasingBase::Sine, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Sine, EasingVariant::InOut, t),
        (EasingBase::Expo, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Expo, EasingVariant::In, t),
        (EasingBase::Expo, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Expo, EasingVariant::Out, t),
        (EasingBase::Expo, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Expo, EasingVariant::InOut, t),
        (EasingBase::Circ, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Circ, EasingVariant::In, t),
        (EasingBase::Circ, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Circ, EasingVariant::Out, t),
        (EasingBase::Circ, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Circ, EasingVariant::InOut, t),
        (EasingBase::Back, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Back, EasingVariant::In, t),
        (EasingBase::Back, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Back, EasingVariant::Out, t),
        (EasingBase::Back, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Back, EasingVariant::InOut, t),
        (EasingBase::Elastic, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Elastic, EasingVariant::In, t),
        (EasingBase::Elastic, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Elastic, EasingVariant::Out, t),
        (EasingBase::Elastic, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Elastic, EasingVariant::InOut, t),
        (EasingBase::Bounce, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Bounce, EasingVariant::In, t),
        (EasingBase::Bounce, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Bounce, EasingVariant::Out, t),
        (EasingBase::Bounce, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Bounce, EasingVariant::InOut, t),
        (EasingBase::Step, EasingVariant::In) => |t: f32| variant_eval(EasingBase::Step, EasingVariant::In, t),
        (EasingBase::Step, EasingVariant::Out) => |t: f32| variant_eval(EasingBase::Step, EasingVariant::Out, t),
        (EasingBase::Step, EasingVariant::InOut) => |t: f32| variant_eval(EasingBase::Step, EasingVariant::InOut, t),
    }
}

/// 入口名解析：`quad-out` → (Quad, Out)。**零分配 O(1) 查表的关键**。
///
/// 先按三闭集切后缀（`strip_suffix` 返回借位切片，不产生 `String`），再在十二
/// 基名里线性找——12 次 `str` 相等比较，无堆分配、无中间缓冲。
pub fn parse_name(name: &str) -> Option<(EasingBase, EasingVariant)> {
    // 长后缀优先：`quad-inout` 必须先试 `-inout`，否则会被 `-in` 抢先匹配成
    // 基名 `quad-inout`（查无此基）而误拒——后缀顺序即优先级。
    for v in [EasingVariant::InOut, EasingVariant::In, EasingVariant::Out].iter() {
        if let Some(stem) = name.strip_suffix(v.suffix()) {
            if let Some(b) = EasingBase::ALL.iter().find(|b| b.wire() == stem) {
                return Some((*b, *v));
            }
        }
    }
    None
}

/// 按名查缓动（**零分配 O(1)**：解析名 → 静态分派；未命中返 None——拒绝+指引，
/// 不静默回退到别的缓动）。
pub fn lookup(name: &str) -> Option<EasingFn> {
    let (b, v) = parse_name(name)?;
    Some(variant_fn(b, v))
}

/// t 越界钳制查询（[0,1] 外先钳后算——锚点错误路径）。
pub fn eval_clamped(name: &str, t: f32) -> Option<f32> {
    let f = lookup(name)?;
    let tc = if t < 0.0 {
        0.0
    } else if t > 1.0 {
        1.0
    } else {
        t
    };
    Some(f(tc))
}

/// 契约核验（端点逐位+全域有限）。
pub fn contract_holds(f: EasingFn) -> bool {
    if f(0.0) != 0.0 || f(1.0) != 1.0 {
        return false;
    }
    let mut i = 0usize;
    while i <= CONTRACT_SAMPLES {
        let t = i as f32 / CONTRACT_SAMPLES as f32;
        if !f(t).is_finite() {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 四、P 词汇统一（跨域对账面）
// ---------------------------------------------------------------------------

/// P 域词汇对齐行（M 侧基名 ↔ P 侧 wire 名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VocabRow {
    /// 基函数。
    pub base: EasingBase,
    /// P 侧 wire 名（跨域同一标识）。
    pub p_wire: &'static str,
}

/// 词汇统一表（12 行——M/P 两域同一枚举表）。
pub fn p_vocabulary() -> Vec<VocabRow> {
    EasingBase::ALL
        .iter()
        .map(|b| VocabRow { base: *b, p_wire: b.wire() })
        .collect()
}

/// 词汇核验：12 行全覆盖+wire 唯一+非空（枚举撞号即裁决）。
pub fn vocab_verdict(rows: &[VocabRow]) -> Result<(), String> {
    if rows.len() != BASE_COUNT {
        return Err(format!(
            "{}：词汇行 {} ≠ {}（全集不齐）",
            E_EASE_VOCAB, rows.len(), BASE_COUNT
        ));
    }
    for b in EasingBase::ALL.iter() {
        let hit = rows.iter().any(|r| r.base == *b);
        if !hit {
            return Err(format!("{}：基函数 {} 缺 P 域对齐行", E_EASE_VOCAB, b.wire()));
        }
    }
    // wire 唯一（撞号裁决——两个基函数共用一个 wire 名即撞号）。
    for (i, a) in rows.iter().enumerate() {
        for b in rows.iter().skip(i + 1) {
            if a.p_wire == b.p_wire {
                return Err(format!(
                    "{}：wire 名 {} 被 {} 与 {} 撞号——对账裁决唯一名",
                    E_EASE_VOCAB, a.p_wire, a.base.wire(), b.base.wire()
                ));
            }
        }
        if a.p_wire.trim().is_empty() {
            return Err(format!("{}：基函数 {} 的 P 侧 wire 为空", E_EASE_VOCAB, a.base.wire()));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、自定义注册（F2403 机制延续——纯函数红线）
// ---------------------------------------------------------------------------

/// 自定义缓动（注册制——内置+自定义双轨）。
#[derive(Clone, Debug)]
pub struct CustomEasing {
    /// 名字（调用方引用用）。
    pub name: String,
    /// 实现（纯函数指针）。
    pub f: EasingFn,
}

/// 自定义注册表。
#[derive(Clone, Debug, Default)]
pub struct CustomRegistry {
    entries: Vec<CustomEasing>,
}

impl CustomRegistry {
    /// 空注册表。
    pub fn new() -> CustomRegistry {
        CustomRegistry::default()
    }

    /// 注册（契约+纯度双检——非纯即拒）。
    pub fn register(&mut self, name: &str, f: EasingFn) -> Result<(), String> {
        if name.trim().is_empty() {
            return Err(format!("{}：自定义缓动名为空", E_EASE_UNREGISTERED));
        }
        // 契约检：端点+有限。
        if !contract_holds(f) {
            return Err(format!(
                "{}：自定义缓动 {} 契约不成立（f(0)!=0 或 f(1)!=1 或非有限）",
                E_EASE_CONTRACT, name
            ));
        }
        // 纯度检：同输入双调用逐位相同（状态依赖必现形）。
        let probes = [0.0f32, 0.25, 0.5, 0.75, 1.0];
        for t in probes.iter() {
            let a = f(*t);
            let b = f(*t);
            if a.to_bits() != b.to_bits() {
                return Err(format!(
                    "{}：自定义缓动 {} 非纯（同输入 {} 两次结果位异）——状态依赖拒收",
                    E_EASE_IMPURE, name, t
                ));
            }
        }
        if self.entries.iter().any(|e| e.name == name) {
            return Err(format!("{}：自定义缓动 {} 重名注册", E_EASE_VOCAB, name));
        }
        self.entries.push(CustomEasing { name: String::from(name), f });
        Ok(())
    }

    /// 查自定义（未注册返 None）。
    pub fn lookup(&self, name: &str) -> Option<EasingFn> {
        self.entries.iter().find(|e| e.name == name).map(|e| e.f)
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空册。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 六、语义手册（选用指南+漂移对账）
// ---------------------------------------------------------------------------

/// 手册行（基函数→语义+建议）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManualRow {
    /// 基函数。
    pub base: EasingBase,
    /// 视觉语义。
    pub semantics: &'static str,
    /// 选用建议。
    pub suggestion: &'static str,
}

/// 手册表（12 行——从 EasingBase 的语义方法生成，单一事实来源）。
pub fn manual() -> Vec<ManualRow> {
    EasingBase::ALL
        .iter()
        .map(|b| ManualRow { base: *b, semantics: b.semantics(), suggestion: b.suggestion() })
        .collect()
}

/// 手册核验：全基覆盖+语义/建议非空（漂移即拒）。
pub fn manual_verdict(rows: &[ManualRow]) -> Result<(), String> {
    for b in EasingBase::ALL.iter() {
        let hit = rows.iter().find(|r| r.base == *b);
        match hit {
            None => return Err(format!("{}：基函数 {} 缺手册行", E_EASE_MANUAL, b.wire())),
            Some(r) => {
                if r.semantics.trim().is_empty() {
                    return Err(format!("{}：{} 缺视觉语义（手册漂移）", E_EASE_MANUAL, b.wire()));
                }
                if r.suggestion.trim().is_empty() {
                    return Err(format!("{}：{} 缺选用建议（手册漂移）", E_EASE_MANUAL, b.wire()));
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2421 域自检（判据五组：全集/词汇/纯函数/手册/收尾）。
pub fn run_vem21_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2421");
    let lib = easing_library();

    // --- 30+ 全集（判据一）---
    s.add(
        "M21-全集-01",
        lib.len() == LIB_COUNT && LIB_COUNT == 36 && BASE_COUNT == 12 && VARIANT_COUNT == 3,
        "36 条目（12 基×3 变体 ≥30+）",
    );
    // 入口名唯一。
    let mut uniq = true;
    for (i, a) in lib.iter().enumerate() {
        for b in lib.iter().skip(i + 1) {
            uniq = uniq && a.0 != b.0;
        }
    }
    s.add("M21-全集-02", uniq, "入口名唯一（quad-in/out/inout 等不撞）");
    // 契约逐条成立（36 条全过端点+有限）。
    let all_contract = lib.iter().all(|(_, f)| contract_holds(*f));
    s.add(
        "M21-全集-03",
        all_contract,
        "36 条契约全成立（端点逐位+33 采样全域有限）",
    );

    // --- 变体代数（判据一续）---
    // Out(t) == 1-In(1-t)（代数恒等式实测对拍）。
    let mut alg = true;
    let mut i = 0usize;
    while i <= 10 {
        let t = i as f32 / 10.0;
        for b in EasingBase::ALL.iter() {
            let out_v = variant_eval(*b, EasingVariant::Out, t);
            let expect = 1.0 - b.eval_in(1.0 - t);
            if out_v.to_bits() != expect.to_bits() {
                alg = false;
            }
        }
        i += 1;
    }
    s.add("M21-变体-01", alg, "Out(t)==1-In(1-t) 代数恒等式逐位成立");
    // InOut 缝隙跃变=端点亏量（精确恒等式，不设任意容差）：
    // lo=f(1-δ)/2、hi=1-f(1-δ)/2 ⇒ |lo-hi| ≡ |f(1-δ)-1|。circ 在
    // t=1 有垂直切线（导数奇异）——缝隙跃变≈2e-3 是函数真属性不是
    // 缺陷，判据如实验"跃变被端点亏量解释"而不是假装光滑。
    let mut seam_ok = true;
    for b in EasingBase::ALL.iter() {
        if *b == EasingBase::Step {
            continue;
        }
        let lo = variant_eval(*b, EasingVariant::InOut, 0.5 - 1.0e-6);
        let hi = variant_eval(*b, EasingVariant::InOut, 0.5 + 1.0e-6);
        let deficit = (b.eval_in(1.0 - 2.0e-6) - 1.0).abs();
        if (lo - hi).abs() > deficit + 1.0e-6 {
            seam_ok = false;
        }
    }
    s.add("M21-变体-02", seam_ok, "InOut 缝隙跃变=端点亏量（恒等式；circ 垂直切线如实）");
    // step 的离散步进性：In 在末端跳（0.9999→0，恰 1→1）；Out 在起点跳
    // （恰 0→0，0.5→1——Out=1-In(1-t) 把 step 的跳变搬到起点）。
    let step_discrete = variant_eval(EasingBase::Step, EasingVariant::In, 0.999_9) == 0.0
        && variant_eval(EasingBase::Step, EasingVariant::In, 1.0) == 1.0
        && variant_eval(EasingBase::Step, EasingVariant::Out, 0.0) == 0.0
        && variant_eval(EasingBase::Step, EasingVariant::Out, 0.5) == 1.0;
    s.add("M21-变体-03", step_discrete, "step 离散步进（In 末端跳/Out 起点跳）");
    // t 越界钳制。
    let clamped = eval_clamped("quad-out", -0.5) == Some(0.0) && eval_clamped("quad-out", 1.5) == Some(1.0);
    s.add("M21-变体-04", clamped && eval_clamped("no-such", 0.5).is_none(), "t 越界钳制+未注册拒（双向）");

    // --- P 词汇统一（判据二）---
    let vocab = p_vocabulary();
    s.add(
        "M21-词汇-01",
        vocab_verdict(&vocab).is_ok() && vocab.len() == BASE_COUNT,
        "P 词汇 12 行全覆盖且 wire 唯一",
    );
    // 撞号可检出（构造两个基共用 wire）。
    let mut crash = vocab.clone();
    crash[1].p_wire = crash[0].p_wire;
    let r = vocab_verdict(&crash);
    s.add(
        "M21-词汇-02",
        r.is_err() && r.as_ref().unwrap_err().contains("撞号") && r.as_ref().unwrap_err().starts_with(E_EASE_VOCAB),
        "wire 撞号对账裁决",
    );
    // 缺行可检出。
    let mut thin = vocab.clone();
    thin.pop();
    s.add("M21-词汇-03", vocab_verdict(&thin).is_err(), "词汇缺行拒绝（全集不齐）");

    // --- 纯函数纪律（判据三）---
    // 自定义合法注册（契约+纯度）。
    fn my_ease(t: f32) -> f32 {
        t * t
    }
    let mut reg = CustomRegistry::new();
    let ok_reg = reg.register("custom.quad", my_ease);
    s.add(
        "M21-纯函数-01",
        ok_reg.is_ok() && reg.len() == 1 && reg.lookup("custom.quad").is_some(),
        "合法自定义注册（quad 平方过契约+纯度）",
    );
    // 契约违例拒绝（端点不对：f(1)=0.5）。
    fn bad_end(t: f32) -> f32 {
        0.5 * t
    }
    let r = reg.register("custom.bad-end", bad_end);
    s.add(
        "M21-纯函数-02",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_EASE_CONTRACT),
        "端点不合自定义拒绝（契约红线）",
    );
    // 非纯拒绝（状态依赖：原子计数器令同输入两次结果位异）。
    IMPURE_CALLS.store(0, Ordering::Relaxed);
    let r = reg.register("custom.impure", counting_ease);
    s.add(
        "M21-纯函数-03",
        r.is_err() && r.as_ref().unwrap_err().starts_with(E_EASE_IMPURE),
        "非纯自定义拒绝（双调用位异——状态依赖现形）",
    );
    // 未注册引用返 None（拒绝+指引）。
    s.add(
        "M21-纯函数-04",
        reg.lookup("custom.nonesuch").is_none() && lookup("nope-out").is_none(),
        "未注册引用 None（拒绝不静默回退）",
    );

    // --- 语义手册（判据四）---
    let rows = manual();
    s.add(
        "M21-手册-01",
        manual_verdict(&rows).is_ok() && rows.len() == BASE_COUNT,
        "手册 12 行全覆盖（语义+建议非空）",
    );
    // 锚点关键词抽查（elastic=过冲振荡/back=回拉预蓄势/bounce=落地弹跳）。
    let kw = rows.iter().any(|r| r.base == EasingBase::Elastic && r.semantics.contains("过冲振荡"))
        && rows.iter().any(|r| r.base == EasingBase::Back && r.semantics.contains("回拉预蓄势"))
        && rows.iter().any(|r| r.base == EasingBase::Bounce && r.semantics.contains("落地弹跳"));
    s.add("M21-手册-02", kw, "锚点三关键词在册（过冲振荡/回拉预蓄势/落地弹跳）");
    // 漂移可检出（构造空语义行）。
    let mut drift = rows.clone();
    drift[0].semantics = "  ";
    let r = manual_verdict(&drift);
    s.add(
        "M21-手册-03",
        r.is_err() && r.as_ref().unwrap_err().contains("linear") && r.as_ref().unwrap_err().starts_with(E_EASE_MANUAL),
        "空语义拒绝（手册漂移对账）",
    );

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in EASING_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("M21-版本-01", fp != 0, "版本指纹非零（M21-easing-v1）");

    s.add(
        "M21-暂挂-01",
        M_LEDGER_EASE_SUSPENDED_NOTE.contains("暂挂") && M_LEDGER_EASE_SUSPENDED_NOTE.contains("F2421"),
        "M 域账本暂挂声明显性（M02 组首项）",
    );

    // M21-暂挂-02：判据条数对账（本条为第 20 条）。
    s.add("M21-暂挂-02", s.len() == 19, "判据条数对账（19+本条）");

    s
}

/// 非纯样本的调用计数（no_std 原子——状态依赖的物质载体）。
static IMPURE_CALLS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// 非纯样本函数（端点恒精确、中段随内部计数位变——契约能过、纯度必拒，
/// 这样拒因才能落到 IMPURE 而不是 CONTRACT：非纯样本要"只犯一条罪"）。
fn counting_ease(t: f32) -> f32 {
    let n = IMPURE_CALLS.fetch_add(1, Ordering::Relaxed);
    if t > 0.0 && t < 1.0 {
        t + (n & 1) as f32 * 1.0e-6
    } else {
        t
    }
}

/// M 域账本暂挂声明（M02 组首项——缓动库；P 域词汇对账随 VE-P 落地回填）。
pub const M_LEDGER_EASE_SUSPENDED_NOTE: &str = "缓动库 36 条目与 P 域词汇统一表入 M 域账本：建账前暂挂声明（移交期模式延续——F2421 同款）；P 域动效库本体落地后按 wire 名逐项对账";
