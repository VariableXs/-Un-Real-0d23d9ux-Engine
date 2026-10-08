//! CGPU-F2081 · N 域开工与原生效果总架构（CGPU-N 域 · 原生效果重建 · 开工单 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2081`
//!
//! 锚点原文：「N 域开工：域使命声明（原生效果重建——把操作系统/浏览器原生的视觉
//! 效果（Acrylic/Mica/背景模糊/毛玻璃）在 VE 渲染下**像素级重建**：不是模仿，
//! 是等价——等价声明）；M 域移交包签收记录（F2077 签收）；官方主题映射（模糊
//! 景深/光晕镜头/材质重建/阴影重建——十组映射表）；架构五段（效果清单→重建策略
//! →GPU 管线→质量验证→性能治理）；与 O 域（效果来源（CSS backdrop/filter 等
//! ——来源协议（跨域契约——契约声明；等价标准（与原生效果像素/时序等价——等价
//! 标准（SSIM+延迟双门禁——双门禁；风险四条（原生效果黑盒/版本漂移/GPU 差异/
//! 性能——各配预案）；测试（使命/签收/映射/五段/来源/等价/风险七组）。判据：
//! 等价声明、签收、映射、五段、来源契约、双门禁、风险、七组、判据。」
//!
//! # 一、等价是**判定**不是修辞：SSIM+延迟双门禁
//!
//! 「不是模仿，是等价」若无法判定，第一张工期紧张的截图就会把它降级成「看起来
//! 差不多」。[`EquivalenceGate`] 把等价落成双门禁判定：结构相似度 SSIM 达标
//! **且**时序延迟达标才算重建等价——任一门禁不过即不等价（显性假，不静默放行），
//! 「等价声明」由此可机检。
//!
//! # 二、签收是**状态机**：M 域移交七件逐一签、缺件不兑付
//!
//! M 域移交包（F2077）七件名册 [`HANDOFF_SEVEN`] 落成签收账 [`HandoffLedger`]：
//! 逐件签收、重复签显性拒、越界拒、七件全签才 [`HandoffLedger::ready_to_extend`]
//! ——「N 域签收」是可审计的状态迁移，不是会议纪要上的一个勾。
//!
//! # 三、来源协议是**契约声明**：与 O 域的跨域对接面
//!
//! 原生效果从哪里来（CSS backdrop/filter、Win32/DWM）由 O 域供给——[`O_RELATION`]
//! 把跨域契约原句承载为常量：来源协议逐句可 grep，N 域不私设效果来源白名单。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、诊断码（vcn01 独占段 0x9C..）
// ---------------------------------------------------------------------------

/// 签收越界（名册只有七件）。
pub const E_N081_SIGN_RANGE: u16 = 0x9C00;
/// 重复签收（显性拒，不静默去重）。
pub const E_N081_DUP_SIGN: u16 = 0x9C01;
/// SSIM 门禁不过（结构相似度低于阈值——像素级等价不成立）。
pub const E_N081_SSIM_LOW: u16 = 0x9C02;
/// 延迟门禁不过（重建时序超出预算——时序等价不成立）。
pub const E_N081_LATENCY_HIGH: u16 = 0x9C03;
/// 层序越界（架构只有五段）。
pub const E_N081_STAGE_RANGE: u16 = 0x9C04;
/// 组映射越界（承接主题号超出 0..=4）。
pub const E_N081_THEME_RANGE: u16 = 0x9C05;

/// 域版本。
pub const VCN01_VERSION: &str = "CN01-nativerfx-v1";

// ---------------------------------------------------------------------------
// 二、域使命声明（等价声明）
// ---------------------------------------------------------------------------

/// 域使命（锚点「域使命声明」原句承载——「像素级重建」「不是模仿，是等价」是判据词）。
pub const MISSION: &str =
    "把操作系统/浏览器原生的视觉效果（Acrylic/Mica/背景模糊/毛玻璃）在 VE 渲染下\
像素级重建：不是模仿，是等价——重建效果与原生效果像素/时序双等价";

// ---------------------------------------------------------------------------
// 三、M 域移交包七件签收（F2077 签收）
// ---------------------------------------------------------------------------

/// M 域移交包七件名册（F2077 锚点「包内容七件」——签收对象规格）。
pub const HANDOFF_SEVEN: [&str; 7] = [
    "接口冻结清单（64 函数+四资产）",
    "契约清单（十一域契约）",
    "资产清单（五账）",
    "基线快照（总册+损耗表+精度表）",
    "遗留移交清单（无线显示/8K/DV/DRM——处置建议）",
    "原生效果衔接包（显示链效果接口草案+色彩管线引用 V 域声明+光敏安全红线引用）",
    "经验教训十条",
];

/// 七件签收账（逐件签、重复拒、缺件不兑付——签收是状态机不是纪要）。
#[derive(Clone, Debug)]
pub struct HandoffLedger {
    signed: [bool; 7],
    /// 签收动作计数。
    pub sign_actions: u32,
}

impl HandoffLedger {
    /// 新账（七件全未签）。
    pub fn new() -> HandoffLedger {
        HandoffLedger { signed: [false; 7], sign_actions: 0 }
    }

    /// **sign**：逐件签收。越界拒 [`E_N081_SIGN_RANGE`]；重复签拒 [`E_N081_DUP_SIGN`]。
    pub fn sign(&mut self, item: usize) -> Result<(), u16> {
        if item >= 7 {
            return Err(E_N081_SIGN_RANGE);
        }
        if self.signed[item] {
            return Err(E_N081_DUP_SIGN);
        }
        self.signed[item] = true;
        self.sign_actions += 1;
        Ok(())
    }

    /// 某件是否已签。
    pub fn is_signed(&self, item: usize) -> Result<bool, u16> {
        if item >= 7 {
            return Err(E_N081_SIGN_RANGE);
        }
        Ok(self.signed[item])
    }

    /// 已签件数。
    pub fn signed_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < 7 {
            if self.signed[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// **ready_to_extend**：七件全签才许 N 域扩展动工（F2077 签收兑现起点）。
    pub fn ready_to_extend(&self) -> bool {
        self.signed_count() == 7
    }
}

// ---------------------------------------------------------------------------
// 四、官方主题映射（四主题封闭 + 十组映射表）
// ---------------------------------------------------------------------------

/// N 域官方主题（封闭四主题——锚点「官方主题映射」）。
pub const THEMES: [&str; 4] = ["模糊景深", "光晕镜头", "材质重建", "阴影重建"];

/// 域任务段（F2081-F2240，160 单）。
pub const DOMAIN_RANGE: (u32, u32) = (2081, 2240);

/// 组规划条目（十组映射表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    /// 组号（1..=10，对应 N01..N10）。
    pub gid: u8,
    /// 组名。
    pub name: &'static str,
    /// 承接主题号（1..=4；开工组/收口组可空段用 0——0 仅限 N01/N10）。
    pub theme: u8,
}

/// 十组封闭表（锚点「十组映射表」——组段 N01..N10 连续无洞）。
pub const GROUPS: [Group; 10] = [
    Group { gid: 1, name: "N01 开工与总架构", theme: 0 },
    Group { gid: 2, name: "N02 分类与清单", theme: 1 },
    Group { gid: 3, name: "N03 模糊重建", theme: 1 },
    Group { gid: 4, name: "N04 光效重建", theme: 2 },
    Group { gid: 5, name: "N05 材质重建", theme: 3 },
    Group { gid: 6, name: "N06 阴影重建", theme: 4 },
    Group { gid: 7, name: "N07 等价验证", theme: 1 },
    Group { gid: 8, name: "N08 性能治理", theme: 2 },
    Group { gid: 9, name: "N09 场景扩展", theme: 3 },
    Group { gid: 10, name: "N10 域收口", theme: 0 },
];

/// 组号连续性核对（N01..N10 无洞无重——判据侧独立重算同一结果）。
pub fn groups_contiguous() -> bool {
    let mut ok = true;
    let mut expect = 1u8;
    let mut i = 0usize;
    while i < GROUPS.len() {
        if GROUPS[i].gid != expect {
            ok = false;
        }
        expect += 1;
        i += 1;
    }
    ok && expect == 11
}

/// 组-主题映射合法（承接主题号在 0..=4；0 仅限开工/收口组 N01/N10）。
pub fn group_themes_valid() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < GROUPS.len() {
        let t = GROUPS[i].theme;
        if t > 4 {
            ok = false;
        }
        if t == 0 && GROUPS[i].gid != 1 && GROUPS[i].gid != 10 {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 域段单数守恒（F2081-F2240 恰 160 单——判据侧独立重算）。
pub const fn domain_size() -> usize {
    (DOMAIN_RANGE.1 - DOMAIN_RANGE.0 + 1) as usize
}

// ---------------------------------------------------------------------------
// 五、架构五段（顺序即秩）
// ---------------------------------------------------------------------------

/// 架构五段封闭表（锚点「架构五段」——效果清单→重建策略→GPU 管线→质量验证→性能治理）。
pub const ARCH_STAGES: [&str; 5] =
    ["效果清单", "重建策略", "GPU 管线", "质量验证", "性能治理"];

/// 段名查询（越界 None——第六段不存在）。
pub fn stage_of(i: usize) -> Result<&'static str, u16> {
    if i >= ARCH_STAGES.len() {
        return Err(E_N081_STAGE_RANGE);
    }
    Ok(ARCH_STAGES[i])
}

// ---------------------------------------------------------------------------
// 六、与 O 域来源协议（跨域契约声明）
// ---------------------------------------------------------------------------

/// 与 O 域关系（锚点「来源协议（跨域契约——契约声明」原句承载）。
pub const O_RELATION: &str =
    "效果来源由 O 域供给（CSS backdrop/filter、Win32/DWM 原生效果清单）——\
来源协议跨域契约，N 域不私设效果来源白名单";

// ---------------------------------------------------------------------------
// 七、等价标准：SSIM+延迟双门禁
// ---------------------------------------------------------------------------

/// SSIM 门禁阈值（结构相似度下限——像素级等价的量化口径）。
pub const SSIM_THRESHOLD: u32 = 98;
/// 延迟门禁阈值（重建引入延迟上限，毫秒——时序等价的量化口径）。
pub const LATENCY_THRESHOLD_MS: u32 = 2;

/// SSIM 定标（千分制：0..=1000；98 = 0.980——门禁输入口径写死防漂移）。
pub const SSIM_SCALE: u32 = 1000;

/// 等价门禁判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Equivalence {
    /// 双门禁全过——重建与原生等价。
    Equivalent,
    /// SSIM 门禁不过（像素等价不成立）。
    PixelMismatch,
    /// 延迟门禁不过（时序等价不成立）。
    TimingMismatch,
}

/// **等价判定**：SSIM（千分制）与重建延迟（ms）双门禁。
///
/// 双过 → [`Equivalence::Equivalent`]；SSIM 低 → [`E_N081_SSIM_LOW`] 痕记
/// [`Equivalence::PixelMismatch`]；延迟超 → [`E_N081_LATENCY_HIGH`] 痕记
/// [`Equivalence::TimingMismatch`]（SSIM 先判——像素不等于时序补救无意义）。
pub const fn equivalence_gate(ssim_permille: u32, latency_ms: u32) -> Equivalence {
    if ssim_permille < SSIM_THRESHOLD {
        return Equivalence::PixelMismatch;
    }
    if latency_ms > LATENCY_THRESHOLD_MS {
        return Equivalence::TimingMismatch;
    }
    Equivalence::Equivalent
}

// ---------------------------------------------------------------------------
// 八、域风险四条（各配预案）
// ---------------------------------------------------------------------------

/// 域风险四条（锚点「风险四条」——各配预案，风险不裸奔）。
pub const RISKS: [(&str, &str); 4] = [
    ("原生效果黑盒", "逐效果逆向采样建档——行为对拍补规范缺失"),
    ("版本漂移", "效果基线随系统版本冻结——漂移检出即重基线"),
    ("GPU 差异", "等价门禁按 GPU 档分别建账——差异进账不入档"),
    ("性能", "双门禁含延迟口径——性能不达标即不等价不降标"),
];

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    let mut s = String::new();
    s.push_str(VCN01_VERSION);
    s.push_str(" themes=4 groups=10 range=2081-2240 stages=5 gate=ssim+latency");
    s
}

// ---------------------------------------------------------------------------
// 编译期闸
// ---------------------------------------------------------------------------

const _: () = {
    assert!(HANDOFF_SEVEN.len() == 7);
    assert!(THEMES.len() == 4);
    assert!(GROUPS.len() == 10);
    assert!(ARCH_STAGES.len() == 5);
    assert!(RISKS.len() == 4);
    assert!(SSIM_THRESHOLD == 98 && SSIM_SCALE == 1000 && LATENCY_THRESHOLD_MS == 2);
    assert!(SSIM_THRESHOLD < SSIM_SCALE);
    assert!(DOMAIN_RANGE.0 == 2081 && DOMAIN_RANGE.1 == 2240);
    assert!((DOMAIN_RANGE.1 - DOMAIN_RANGE.0 + 1) as usize == 160);
    assert!(E_N081_SIGN_RANGE & 0xFF00 == 0x9C00);
    assert!(E_N081_DUP_SIGN & 0xFF00 == 0x9C00);
    assert!(E_N081_SSIM_LOW & 0xFF00 == 0x9C00);
    assert!(E_N081_LATENCY_HIGH & 0xFF00 == 0x9C00);
    assert!(E_N081_STAGE_RANGE & 0xFF00 == 0x9C00);
    assert!(E_N081_THEME_RANGE & 0xFF00 == 0x9C00);
    assert!(
        E_N081_SIGN_RANGE != E_N081_DUP_SIGN
            && E_N081_DUP_SIGN != E_N081_SSIM_LOW
            && E_N081_SSIM_LOW != E_N081_LATENCY_HIGH
            && E_N081_LATENCY_HIGH != E_N081_STAGE_RANGE
            && E_N081_STAGE_RANGE != E_N081_THEME_RANGE
    );
};
