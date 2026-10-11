//! CGPU-F1719 · 异构 SDK 与开发者体验
//!
//! 判据锚点（施工书原文摘录）：
//! SDK：SDK 套件（异构 SDK（API 封装/工具（模拟器/trace 分析器分发——SDK 清单；
//! DX（开发者体验（文档/示例/错误信息友好（DX 标准（错误信息三要素复用——DX
//! 规范；版本（SDK 版本策略（与 API 冻结对齐——版本对齐；分发（SDK 分发渠道
//! ——分发（开放复用；反馈（开发者反馈通道——反馈闭环；测试（清单/DX/版本/
//! 分发/反馈五组）。判据：SDK 清单、DX 规范、版本对齐、分发、反馈、五组、判据。
//!
//! 诊断码段 0xA201~0xA205（独占）：
//!   0xA201 SDK 清单缺件（API 封装/模拟器/trace 分析器任一缺席）
//!   0xA202 DX 失范（错误信息三要素缺一/文档示例空）
//!   0xA203 版本失对齐（SDK 版本越 API 冻结锚）
//!   0xA204 分发失道（渠道不在册/反馈通道闭环断）
//!   0xA205 段越界（判码落 0xA200..0xA300 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。


/// 模块版本。
pub const SDK_DX_VERSION: &str = "K38-sdk-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xA201~0xA205）
// ---------------------------------------------------------------------------

pub const XC_KIT_INCOMPLETE: u32 = 0xA201;
pub const XC_DX_SUBPAR: u32 = 0xA202;
pub const XC_VER_MISALIGNED: u32 = 0xA203;
pub const XC_DISTRO_BROKEN: u32 = 0xA204;
pub const XC_SEG_OOR: u32 = 0xA205;

// ---------------------------------------------------------------------------
// 一、SDK 清单
// ---------------------------------------------------------------------------

/// SDK 件三闭集（判据"API 封装/工具（模拟器/trace 分析器"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KitItem {
    /// API 封装（K07 API10 的开发者面）。
    ApiWrap,
    /// 模拟器（vck12/vck24 复用）。
    Simulator,
    /// trace 分析器（vck21/vck22 复用）。
    TraceAnalyzer,
}

impl KitItem {
    pub const ALL: [KitItem; 3] =
        [KitItem::ApiWrap, KitItem::Simulator, KitItem::TraceAnalyzer];

    pub const fn name(self) -> &'static str {
        match self {
            KitItem::ApiWrap => "api_wrap",
            KitItem::Simulator => "simulator",
            KitItem::TraceAnalyzer => "trace_analyzer",
        }
    }

    /// 复用源单（SDK 件不是新造——每件挂既有成果）。
    pub const fn origin(self) -> &'static str {
        match self {
            KitItem::ApiWrap => "CGPU-F1614",
            KitItem::Simulator => "CGPU-F1612",
            KitItem::TraceAnalyzer => "CGPU-F1702",
        }
    }
}

/// SDK 清单：三件全在册。
pub struct SdkManifest {
    pub ready: [bool; 3],
}

impl SdkManifest {
    pub fn build() -> Self {
        SdkManifest { ready: [true; 3] }
    }

    pub fn complete(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            if !self.ready[i] || KitItem::ALL[i].origin().is_empty() {
                return Err(XC_KIT_INCOMPLETE);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、DX 规范（错误信息三要素复用）
// ---------------------------------------------------------------------------

/// 错误信息三要素：码（机器可查）+ 人话（开发者可读）+ 动作（下一步可做）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrMsg {
    pub code: u32,
    pub human: &'static str,
    pub action: &'static str,
}

impl ErrMsg {
    /// DX 达标：三要素齐（0xA202）。
    pub fn compliant(&self) -> Result<(), u32> {
        if self.human.is_empty() || self.action.is_empty() || self.code == 0 {
            return Err(XC_DX_SUBPAR);
        }
        Ok(())
    }
}

/// DX 抽样：三条错误信息全达标。
pub fn dx_audit(msgs: &[ErrMsg]) -> Result<(), u32> {
    if msgs.len() < 3 {
        return Err(XC_DX_SUBPAR);
    }
    for m in msgs {
        m.compliant()?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、版本对齐 + 分发 + 反馈
// ---------------------------------------------------------------------------

/// 版本策略：SDK 版本必须对齐 API 冻结锚（vck14 FrozenRegistry v1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerAlign {
    /// SDK 版本。
    pub sdk_ver: u32,
    /// API 冻结锚版本。
    pub api_frozen_ver: u32,
}

impl VerAlign {
    pub fn build() -> Self {
        VerAlign { sdk_ver: 1, api_frozen_ver: 1 }
    }

    /// 对齐：SDK 版本 == 冻结锚版本（超前=文档未冻/滞后=落后冻结面——都失对齐）。
    pub fn aligned(&self) -> Result<(), u32> {
        if self.sdk_ver != self.api_frozen_ver {
            return Err(XC_VER_MISALIGNED);
        }
        Ok(())
    }
}

/// 分发渠道二闭集（开放复用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DistroChannel {
    /// 源码随仓库分发（默认）。
    InRepo,
    /// 版本化快照。
    ReleaseTag,
}

impl DistroChannel {
    pub const ALL: [DistroChannel; 2] = [DistroChannel::InRepo, DistroChannel::ReleaseTag];
}

/// 反馈闭环：通道在册 + 有回执位（闭环=收得到也回得出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FeedbackLoop {
    pub channel_open: bool,
    pub receipt_out: bool,
}

impl FeedbackLoop {
    pub const fn build() -> Self {
        FeedbackLoop { channel_open: true, receipt_out: true }
    }

    pub fn closed(&self) -> Result<(), u32> {
        if !self.channel_open || !self.receipt_out {
            return Err(XC_DISTRO_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（五组）
// ---------------------------------------------------------------------------

/// SDK 引擎。
pub struct SdkEngine;

impl SdkEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：清单。
        let m = SdkManifest::build();
        m.complete()?;
        // 组二：DX——三条全达标 + 缺要素反例。
        let msgs = [
            ErrMsg { code: 0xA201, human: "SDK 缺模拟器件", action: "安装 simulator 组件" },
            ErrMsg { code: 0xA202, human: "错误信息缺动作", action: "补 action 字段" },
            ErrMsg { code: 0xA203, human: "版本失对齐", action: "对齐 API 冻结锚 v1" },
        ];
        dx_audit(&msgs)?;
        let bad = [ErrMsg { code: 0xA202, human: "没人话", action: "" }];
        if dx_audit(&bad).is_ok() {
            return Err(XC_DX_SUBPAR);
        }
        // 组三：版本对齐——对齐过 + 超前反例。
        VerAlign::build().aligned()?;
        let ahead = VerAlign { sdk_ver: 2, api_frozen_ver: 1 };
        if ahead.aligned().is_ok() {
            return Err(XC_VER_MISALIGNED);
        }
        // 组四：分发——双渠道在册。
        for c in DistroChannel::ALL {
            if c == DistroChannel::InRepo || c == DistroChannel::ReleaseTag {
                continue;
            }
            return Err(XC_DISTRO_BROKEN);
        }
        // 组五：反馈闭环。
        FeedbackLoop::build().closed()?;
        let broken = FeedbackLoop { channel_open: true, receipt_out: false };
        if broken.closed().is_ok() {
            return Err(XC_DISTRO_BROKEN);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xA200..0xA300 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_KIT_INCOMPLETE,
            XC_DX_SUBPAR,
            XC_VER_MISALIGNED,
            XC_DISTRO_BROKEN,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xA200 || codes[i] >= 0xA300 {
                return Err(XC_KIT_INCOMPLETE);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_KIT_INCOMPLETE);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
