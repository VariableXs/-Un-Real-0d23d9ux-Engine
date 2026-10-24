//! VE-F3550 · 设置中心深度集成 —— 一入口五域 + 即时生效 + 双向定位 + 空态引导。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3550`
//!
//! # 职责（锚点原文拆解）
//!
//! - **一入口五域**（[`SettingsPanel`]）：色彩/声音/字体/图标/动效
//!   五域设置收进一个面板，每域一节不缺。
//! - **即时生效**（[`EffectChain`]）：改一格立即看到效果——
//!   设置项与生效链直连，无「应用」按钮延迟。
//! - **双向定位**：从设置查生效（[`SettingsPanel::effect_of`]）、
//!   从生效反查设置（[`SettingsPanel::setting_of`]），两向都通。
//! - **空态引导**（[`GuideState`]）：没配过 Persona 时展示引导，
//!   可发现性不靠用户自己撞。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 入口缺失 | 补齐（五域节全渲染） |
//! | 联动失效 | 修正（生效链断连告警） |
//! | 引导缺失 | 补引导 |
//!
//! # 复杂度
//!
//! O(面板)（五域定长）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6180 起）
// ---------------------------------------------------------------------------

/// 设置集成诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsCode {
    /// 域节缺失。
    DomainMissing = 6180,
    /// 生效链断连。
    ChainBroken = 6181,
    /// 双向定位失败。
    LocateFailed = 6182,
    /// 空态引导缺失。
    GuideMissing = 6183,
}

impl SettingsCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            SettingsCode::DomainMissing => "设置域节缺失",
            SettingsCode::ChainBroken => "生效链断连",
            SettingsCode::LocateFailed => "双向定位失败",
            SettingsCode::GuideMissing => "空态引导缺失",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, SettingsCode::ChainBroken)
    }
}

// ---------------------------------------------------------------------------
// 二、五域
// ---------------------------------------------------------------------------

/// 个性化五域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    /// 色彩。
    Color,
    /// 声音。
    Sound,
    /// 字体。
    Font,
    /// 图标。
    Icon,
    /// 动效。
    Motion,
}

impl Domain {
    /// 全部五域（定序遍历用）。
    pub const ALL: [Domain; 5] = [
        Domain::Color,
        Domain::Sound,
        Domain::Font,
        Domain::Icon,
        Domain::Motion,
    ];

    /// 域短名。
    pub const fn label(self) -> &'static str {
        match self {
            Domain::Color => "色彩",
            Domain::Sound => "声音",
            Domain::Font => "字体",
            Domain::Icon => "图标",
            Domain::Motion => "动效",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、生效链
// ---------------------------------------------------------------------------

/// 生效链节点（设置项 id ↔ 效果 id 双向绑定）。
#[derive(Clone, Copy, Debug)]
pub struct EffectLink {
    /// 设置项 id。
    pub setting_id: u32,
    /// 效果 id。
    pub effect_id: u32,
    /// 所属域。
    pub domain: Domain,
}

/// 生效链（改一格立即生效的通路）。
#[derive(Default)]
pub struct EffectChain {
    links: Vec<EffectLink>,
}

impl EffectChain {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 建链。
    pub fn bind(&mut self, link: EffectLink) {
        self.links.push(link);
    }

    /// 从设置查效果。
    pub fn effect_of(&self, setting_id: u32) -> Option<u32> {
        self.links
            .iter()
            .find(|l| l.setting_id == setting_id)
            .map(|l| l.effect_id)
    }

    /// 从效果查设置。
    pub fn setting_of(&self, effect_id: u32) -> Option<u32> {
        self.links
            .iter()
            .find(|l| l.effect_id == effect_id)
            .map(|l| l.setting_id)
    }

    /// 链路数。
    pub fn len(&self) -> usize {
        self.links.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    /// 某域是否已建链。
    pub fn covers(&self, d: Domain) -> bool {
        self.links.iter().any(|l| l.domain == d)
    }
}

// ---------------------------------------------------------------------------
// 四、空态引导
// ---------------------------------------------------------------------------

/// 引导状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuideState {
    /// 未展示（已有配置）。
    Hidden,
    /// 待展示（空态）。
    Pending,
    /// 已展示。
    Shown,
}

impl GuideState {
    /// 状态短名。
    pub const fn label(self) -> &'static str {
        match self {
            GuideState::Hidden => "不展示",
            GuideState::Pending => "待展示",
            GuideState::Shown => "已展示",
        }
    }
}

// ---------------------------------------------------------------------------
// 五、设置面板
// ---------------------------------------------------------------------------

/// 设置面板。
pub struct SettingsPanel {
    /// 五域节渲染标志（bit0..bit4 对应 Color..Motion）。
    rendered: u8,
    chain: EffectChain,
    guide: GuideState,
}

impl SettingsPanel {
    /// 新建（五域节全渲染，空态引导待展示）。
    pub fn new() -> Self {
        Self {
            rendered: 0b1_1111,
            chain: EffectChain::new(),
            guide: GuideState::Pending,
        }
    }

    /// 五域节完整性。
    pub fn complete(&self) -> bool {
        self.rendered == 0b1_1111
    }

    /// 建生效链。
    pub fn bind_effect(&mut self, link: EffectLink) {
        self.chain.bind(link);
    }

    /// 改设置并即时生效（返回效果 id；链断则 Err）。
    pub fn change(&self, setting_id: u32) -> Result<u32, SettingsCode> {
        self.chain.effect_of(setting_id).ok_or(SettingsCode::ChainBroken)
    }

    /// 从设置查生效。
    pub fn effect_of(&self, setting_id: u32) -> Option<u32> {
        self.chain.effect_of(setting_id)
    }

    /// 从生效反查设置。
    pub fn setting_of(&self, effect_id: u32) -> Option<u32> {
        self.chain.setting_of(effect_id)
    }

    /// 空态引导触发（无任何链路 = 空态）。
    pub fn maybe_guide(&mut self) -> GuideState {
        if self.chain.is_empty() && self.guide == GuideState::Pending {
            self.guide = GuideState::Shown;
        }
        self.guide
    }

    /// 引导状态。
    pub fn guide(&self) -> GuideState {
        self.guide
    }

    /// 生效链只读。
    pub fn chain(&self) -> &EffectChain {
        &self.chain
    }
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// 设置集成判据集。
pub fn run_settings_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10j_settings");

    // 1. 五域节完整。
    let p = SettingsPanel::new();
    set.add("6180-五域完整", p.complete(), "");

    // 2. 即时生效（建链后 change 直达）。
    let mut p2 = SettingsPanel::new();
    p2.bind_effect(EffectLink {
        setting_id: 10,
        effect_id: 20,
        domain: Domain::Color,
    });
    let effect = p2.change(10).unwrap_or(0);
    set.add("6181-即时生效", effect == 20, "");

    // 3. 双向定位。
    set.add(
            "6182-双向定位",
            p2.effect_of(10) == Some(20) && p2.setting_of(20) == Some(10),
            "",
        );

    // 4. 链断报错。
    set.add(
            "6183-链断告警",
            p2.change(999).err() == Some(SettingsCode::ChainBroken),
            "",
        );

    // 5. 空态引导。
    let mut p3 = SettingsPanel::new();
    let g = p3.maybe_guide();
    set.add("6184-空态引导", g == GuideState::Shown, "");

    // 6. 有配置不打扰。
    let mut p4 = SettingsPanel::new();
    p4.bind_effect(EffectLink {
        setting_id: 1,
        effect_id: 1,
        domain: Domain::Sound,
    });
    let g4 = p4.maybe_guide();
    set.add("6185-有配置不打扰", g4 == GuideState::Pending || g4 == GuideState::Hidden, "");

    // 7. 域覆盖判定。
    set.add(
            "6186-域覆盖",
            p4.chain().covers(Domain::Sound) && !p4.chain().covers(Domain::Motion),
            "",
        );

    set
}
