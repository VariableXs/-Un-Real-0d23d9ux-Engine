//! CGPU-F1717 · AI PC 场景体系
//!
//! 判据锚点（施工书原文摘录）：
//! AI PC：AI PC 定义（NPU 常驻 AI 能力（本地推理服务——本地 AI 声明；能力目录
//! （AI PC 能力目录（超分/转写/摘要/生图——目录（能力注册（应用发现 API——发现
//! 接口；隐私（本地 AI=数据不出机——隐私卖点与红线（红线复用；资源治理（AI 服务
//! 资源配额（不挤占前台——配额复用；调度（AI 任务后台调度（空闲优先——空闲复用；
//! 测试（目录/发现/隐私/配额/调度五组）。判据：本地声明、能力目录、隐私红线、
//! 配额、空闲、五组、判据。
//!
//! 诊断码段 0xA001~0xA005（独占）：
//!   0xA001 本地声明失格（AI 服务未声明本地/常驻态缺失）
//!   0xA002 能力目录越表（能力落四闭集外/发现接口未注册）
//!   0xA003 隐私红线破（AI 数据出机）
//!   0xA004 配额失守（AI 服务挤占前台配额）
//!   0xA005 段越界（判码落 0xA000..0xA100 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。


/// 模块版本。
pub const AI_PC_VERSION: &str = "K36-aipc-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0xA001~0xA005）
// ---------------------------------------------------------------------------

pub const XC_LOCAL_DECL_MISS: u32 = 0xA001;
pub const XC_CAPDIR_OOB: u32 = 0xA002;
pub const XC_DATA_EXFIL: u32 = 0xA003;
pub const XC_QUOTA_BREACH: u32 = 0xA004;
pub const XC_SEG_OOR: u32 = 0xA005;

// ---------------------------------------------------------------------------
// 一、本地 AI 声明
// ---------------------------------------------------------------------------

/// 本地 AI 声明：NPU 常驻推理服务（本地=数据不出机前置条件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocalAiDecl {
    /// 服务已声明。
    pub declared: bool,
    /// 常驻态（常驻=随系统在岗）。
    pub resident: bool,
    /// 推理引擎（2=NPU）。
    pub engine: u8,
}

impl LocalAiDecl {
    pub const fn build() -> Self {
        LocalAiDecl { declared: true, resident: true, engine: 2 }
    }

    pub fn verified(&self) -> Result<(), u32> {
        if !self.declared || !self.resident || self.engine != 2 {
            return Err(XC_LOCAL_DECL_MISS);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、能力目录与应用发现
// ---------------------------------------------------------------------------

/// AI PC 能力四闭集（判据"超分/转写/摘要/生图"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiPcCap {
    Upscale,
    Transcribe,
    Summarize,
    GenImage,
}

impl AiPcCap {
    pub const ALL: [AiPcCap; 4] =
        [AiPcCap::Upscale, AiPcCap::Transcribe, AiPcCap::Summarize, AiPcCap::GenImage];

    pub const fn name(self) -> &'static str {
        match self {
            AiPcCap::Upscale => "upscale",
            AiPcCap::Transcribe => "transcribe",
            AiPcCap::Summarize => "summarize",
            AiPcCap::GenImage => "gen_image",
        }
    }
}

/// 能力目录：四能力注册 + 应用发现接口。
pub struct CapCatalog {
    /// 注册位。
    pub registered: [bool; 4],
    /// 发现接口在册。
    pub discover_api: bool,
}

impl CapCatalog {
    pub fn build() -> Self {
        CapCatalog { registered: [true; 4], discover_api: true }
    }

    /// 目录闭合：四能力全注册 + 发现接口在册（0xA002）。
    pub fn closed(&self) -> Result<(), u32> {
        for r in &self.registered {
            if !r {
                return Err(XC_CAPDIR_OOB);
            }
        }
        if !self.discover_api {
            return Err(XC_CAPDIR_OOB);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、隐私红线（本地 AI=数据不出机）
// ---------------------------------------------------------------------------

/// 出机闸：AI 数据流必须全本地（任何出机通道=0xA003）。
pub fn local_only_gate(exfil_channels: &[&str]) -> Result<(), u32> {
    if !exfil_channels.is_empty() {
        return Err(XC_DATA_EXFIL);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、资源配额（不挤占前台）
// ---------------------------------------------------------------------------

/// 配额账：AI 服务后台配额上限（前台保底）。
pub struct AiQuota {
    /// AI 服务可占百万分比（350_000 = 35%）。
    pub ai_share_ppm: u32,
    /// 前台保底百万分比（450_000 = 45%）。
    pub fg_reserve_ppm: u32,
}

impl AiQuota {
    pub const fn build() -> Self {
        AiQuota { ai_share_ppm: 350_000, fg_reserve_ppm: 450_000 }
    }

    /// 配额守恒：AI 占比 + 前台保底 ≤ 1_000_000 且前台保底不被挤占（0xA004）。
    pub fn guarded(&self) -> Result<(), u32> {
        if self.ai_share_ppm as u64 + self.fg_reserve_ppm as u64 > 1_000_000 {
            return Err(XC_QUOTA_BREACH);
        }
        if self.fg_reserve_ppm < 450_000 {
            return Err(XC_QUOTA_BREACH); // 前台保底线 45%。
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、空闲优先调度
// ---------------------------------------------------------------------------

/// 调度档三闭集（前台活跃→AI 让空 / 空闲窗口→AI 全速 / 低负载→AI 限额跑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleTier {
    FgActive,
    IdleWindow,
    LowLoad,
}

/// 调度器：按系统态选档（空闲优先复用）。
pub fn schedule(fg_active: bool, idle_window: bool) -> IdleTier {
    if fg_active {
        IdleTier::FgActive
    } else if idle_window {
        IdleTier::IdleWindow
    } else {
        IdleTier::LowLoad
    }
}

/// 档速率万分比（FgActive=0 停 / IdleWindow=10000 全速 / LowLoad=3000 限额）。
pub const fn tier_rate_bp(t: IdleTier) -> u32 {
    match t {
        IdleTier::FgActive => 0,
        IdleTier::IdleWindow => 10_000,
        IdleTier::LowLoad => 3_000,
    }
}

// ---------------------------------------------------------------------------
// 自检（五组）
// ---------------------------------------------------------------------------

/// AI PC 引擎。
pub struct AiPcEngine;

impl AiPcEngine {
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        // 组一：本地声明。
        LocalAiDecl::build().verified()?;
        // 组二：能力目录。
        let cat = CapCatalog::build();
        cat.closed()?;
        if AiPcCap::ALL.len() != 4 {
            return Err(XC_CAPDIR_OOB);
        }
        // 组三：隐私红线——零出机过 + 出机拒。
        local_only_gate(&[])?;
        if local_only_gate(&["cloud-upload"]).is_ok() {
            return Err(XC_DATA_EXFIL);
        }
        // 组四：配额——默认守恒 + 挤占反例。
        AiQuota::build().guarded()?;
        let greedy = AiQuota { ai_share_ppm: 600_000, fg_reserve_ppm: 400_000 };
        if greedy.guarded().is_ok() {
            return Err(XC_QUOTA_BREACH);
        }
        // 组五：空闲调度——三态速率对拍。
        if tier_rate_bp(schedule(true, true)) != 0 {
            return Err(XC_QUOTA_BREACH);
        }
        if tier_rate_bp(schedule(false, true)) != 10_000 {
            return Err(XC_QUOTA_BREACH);
        }
        if tier_rate_bp(schedule(false, false)) != 3_000 {
            return Err(XC_QUOTA_BREACH);
        }
        Ok(())
    }

    /// 段对账：五码全落 0xA000..0xA100 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_LOCAL_DECL_MISS,
            XC_CAPDIR_OOB,
            XC_DATA_EXFIL,
            XC_QUOTA_BREACH,
            XC_SEG_OOR,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0xA000 || codes[i] >= 0xA100 {
                return Err(XC_LOCAL_DECL_MISS);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_LOCAL_DECL_MISS);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
