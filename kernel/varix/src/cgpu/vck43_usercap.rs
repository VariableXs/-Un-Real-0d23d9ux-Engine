//! CGPU-F1724 · 异构用户可见能力
//!
//! 判据锚点（施工书原文摘录）：
//! 用户可见：UE 集成（用户可见（AI 能力开关/状态/解释（N 域 UE 集成——集成声明
//! （设置界面（能力面板——面板规范；解释（能力解释（"NPU 正在加速视频通话"——
//! 解释文案（三要素复用——文案规范；克制（能力提示克制（不炫耀——克制复用；
//! 无障碍（能力面板无障碍——无障碍复用；测试（集成/面板/解释/克制/无障碍五组）。
//! 判据：集成声明、面板、文案、克制、无障碍、五组、判据。
//!
//! 诊断码段 0xA701~0xA705（独占）：
//!   0xA701 集成缺面（UE 三接面开关/状态/解释任一缺）
//!   0xA702 面板失范（能力面板项缺/开关不可关）
//!   0xA703 文案缺素（解释三要素缺一）
//!   0xA704 无障碍缺查（读屏标签/对比度/键盘可达缺一）
//!   0xA705 段越界（判码落 0xA700..0xA800 区间外；克制违例并入文案组判 0xA703）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。

/// 模块版本。
pub const USER_CAP_VERSION: &str = "K43-usercap-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xA701~0xA705）
// ---------------------------------------------------------------------------

pub const XC_UC_INTEGRATION_MISSING: u32 = 0xA701;
pub const XC_UC_PANEL_SUBPAR: u32 = 0xA702;
pub const XC_UC_MSG_MISSING: u32 = 0xA703;
pub const XC_UC_A11Y_MISSING: u32 = 0xA704;
pub const XC_UC_SEG_OOR: u32 = 0xA705;

// ---------------------------------------------------------------------------
// 一、UE 集成声明（开关/状态/解释 三接面）
// ---------------------------------------------------------------------------

/// 三接面闭集（N 域 UE 集成锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UeSurface {
    /// 开关：用户可开可关（不可强制）。
    Toggle,
    /// 状态：实时可查（在加速/空闲/降级）。
    Status,
    /// 解释：人话文案可读。
    Explain,
}

impl UeSurface {
    pub const ALL: [UeSurface; 3] =
        [UeSurface::Toggle, UeSurface::Status, UeSurface::Explain];

    pub const fn name(self) -> &'static str {
        match self {
            UeSurface::Toggle => "toggle",
            UeSurface::Status => "status",
            UeSurface::Explain => "explain",
        }
    }
}

/// 集成声明：三接面全接通。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UeIntegration {
    pub wired: [bool; 3],
}

impl UeIntegration {
    pub fn build() -> Self {
        UeIntegration { wired: [true; 3] }
    }

    pub fn complete(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            if !self.wired[i] {
                return Err(XC_UC_INTEGRATION_MISSING);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、能力面板规范（设置界面）
// ---------------------------------------------------------------------------

/// 面板项闭集（每能力一项：开关 + 状态位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanelItem {
    pub cap: &'static str,
    /// 开关可关（不可关即失范——强制能力不进面板）。
    pub toggleable: bool,
    /// 状态可查。
    pub status_visible: bool,
}

impl PanelItem {
    pub fn sound(&self) -> Result<(), u32> {
        if self.cap.is_empty() || !self.toggleable || !self.status_visible {
            return Err(XC_UC_PANEL_SUBPAR);
        }
        Ok(())
    }
}

/// 面板：四能力项在册全规范。
pub struct CapPanel {
    pub items: [PanelItem; 4],
}

impl CapPanel {
    pub fn build() -> Self {
        CapPanel {
            items: [
                PanelItem { cap: "sr_upscale", toggleable: true, status_visible: true },
                PanelItem { cap: "asr_transcribe", toggleable: true, status_visible: true },
                PanelItem { cap: "doc_summarize", toggleable: true, status_visible: true },
                PanelItem { cap: "video_call_boost", toggleable: true, status_visible: true },
            ],
        }
    }

    pub fn compliant(&self) -> Result<(), u32> {
        for it in self.items.iter() {
            it.sound()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、解释文案规范（三要素）+ 克制审计
// ---------------------------------------------------------------------------

/// 能力解释三要素：谁在加速 / 在干什么 / 用户收益。
/// 模板："NPU 正在加速视频通话"——引擎+动作+对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapExplain {
    pub who: &'static str,
    pub doing: &'static str,
    pub benefit: &'static str,
}

impl CapExplain {
    pub fn compliant(&self) -> Result<(), u32> {
        if self.who.is_empty() || self.doing.is_empty() || self.benefit.is_empty() {
            return Err(XC_UC_MSG_MISSING);
        }
        Ok(())
    }

    /// 合成完整文案（三要素拼接——"NPU 正在加速视频通话"式）。
    pub fn sentence(&self) -> Result<&'static str, u32> {
        // 静态文案表：who+doing 组合查表，不在册即缺素。
        const LINES: [(&str, &str, &str); 4] = [
            ("NPU", "正在加速视频通话", "画面更流畅、更省电"),
            ("iGPU", "正在超分画面", "更清晰、帧率更稳"),
            ("DSP", "正在处理空间音频", "声音更沉浸"),
            ("NPU", "正在转写会议记录", "记录自动成稿"),
        ];
        let mut i = 0;
        while i < LINES.len() {
            if LINES[i].0 == self.who && LINES[i].1 == self.doing {
                self.compliant()?;
                return Ok(LINES[i].2);
            }
            i += 1;
        }
        Err(XC_UC_MSG_MISSING)
    }
}

/// 克制审计：文案不得含炫耀词（闭集四词——复用克制惯例）。
pub fn restraint_audit(text: &str) -> Result<(), u32> {
    const BOAST: [&str; 4] = ["最强", "第一", "无敌", "碾压"];
    let mut i = 0;
    while i < BOAST.len() {
        if contains(text, BOAST[i]) {
            return Err(XC_UC_MSG_MISSING);
        }
        i += 1;
    }
    Ok(())
}

/// no_std 子串包含（朴素逐字节——无 core 依赖）。
fn contains(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return false;
    }
    let mut i = 0;
    while i + n.len() <= h.len() {
        let mut j = 0;
        while j < n.len() && h[i + j] == n[j] {
            j += 1;
        }
        if j == n.len() {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 四、无障碍（读屏标签/对比度/键盘可达）
// ---------------------------------------------------------------------------

/// 无障碍三查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A11ySpec {
    /// 读屏标签非空。
    pub screen_reader_label: &'static str,
    /// 对比度比千分值（4.5:1 = 4500；低于拒）。
    pub contrast_permille: u32,
    /// 键盘可达（全功能不依赖指针）。
    pub keyboard_reachable: bool,
}

impl A11ySpec {
    pub fn compliant(&self) -> Result<(), u32> {
        if self.screen_reader_label.is_empty() {
            return Err(XC_UC_A11Y_MISSING);
        }
        if self.contrast_permille < 4500 {
            return Err(XC_UC_A11Y_MISSING);
        }
        if !self.keyboard_reachable {
            return Err(XC_UC_A11Y_MISSING);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（五组：集成/面板/文案/克制/无障碍）
// ---------------------------------------------------------------------------

/// 用户可见能力引擎。
pub struct UserCapEngine;

impl UserCapEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：集成三接面。
        UeIntegration::build().complete()?;
        let mut torn = UeIntegration::build();
        torn.wired[2] = false;
        if torn.complete().is_ok() {
            return Err(XC_UC_INTEGRATION_MISSING);
        }
        // 组二：面板四项全规范 + 反例（不可关拒）。
        CapPanel::build().compliant()?;
        let mut bad_panel = CapPanel::build();
        bad_panel.items[0].toggleable = false;
        if bad_panel.compliant().is_ok() {
            return Err(XC_UC_PANEL_SUBPAR);
        }
        // 组三：文案三要素 + 句式合成在册。
        let ex = CapExplain { who: "NPU", doing: "正在加速视频通话", benefit: "更省电" };
        ex.compliant()?;
        ex.sentence()?;
        if (CapExplain { who: "NPU", doing: "", benefit: "x" }).compliant().is_ok() {
            return Err(XC_UC_MSG_MISSING);
        }
        if (CapExplain { who: "GPU99", doing: "正在乱来", benefit: "x" }).sentence().is_ok() {
            return Err(XC_UC_MSG_MISSING);
        }
        // 组四：克制审计——炫耀词拦 + 朴素文案过。
        if restraint_audit("NPU 最强加速").is_ok() {
            return Err(XC_UC_MSG_MISSING);
        }
        restraint_audit("NPU 正在加速视频通话")?;
        // 组五：无障碍三查 + 反例（对比度不足拒）。
        A11ySpec { screen_reader_label: "AI 能力开关", contrast_permille: 4500, keyboard_reachable: true }
            .compliant()?;
        if (A11ySpec { screen_reader_label: "AI 能力开关", contrast_permille: 3000, keyboard_reachable: true })
            .compliant()
            .is_ok()
        {
            return Err(XC_UC_A11Y_MISSING);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xA700..0xA800 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_UC_INTEGRATION_MISSING,
            XC_UC_PANEL_SUBPAR,
            XC_UC_MSG_MISSING,
            XC_UC_A11Y_MISSING,
            XC_UC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xA700 || codes[i] >= 0xA800 {
                return Err(XC_UC_INTEGRATION_MISSING);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_UC_INTEGRATION_MISSING);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
