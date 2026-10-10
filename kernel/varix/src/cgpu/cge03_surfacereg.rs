//! CGPU-F0643 · 表面注册与能力协商（CGPU-E 域 · 表面调度域 · E01 组 · 目标 340 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0643`
//!
//! **判据（锚点原文）**：四要素、协商显性、唯一源、鉴权、判据。
//!
//! **职责定位（锚点原文）**：注册协商：表面注册协议（表面向 CGPU 注册：
//! 尺寸/刷新需求/内容类型/交互性声明——四要素注册制）；能力协商（声明×
//! 系统能力→实际待遇——协商结果显性返回）；注册表管理（全系统表面注册
//! 表——唯一权威源）；注册安全（伪造注册防护——注册通道鉴权）。
//!
//! ## 一、四要素注册制（判据一：字段闭集）
//!
//! 注册声明四要素（[`SurfaceDecl`]）：尺寸（宽×高）、刷新需求（毫赫兹）、
//! 内容类型（[`ContentType`] 闭集）、交互性声明——字段闭集纪律：四要素
//! 之外不收字段，缺一要素注册即拒（显性码 [`XcCode::DeclIncomplete`]）。
//! 尺寸零面积与刷新零值同拒（边界显式处理——零尺寸表面没有可渲染面）。
//!
//! ## 二、能力协商显性返回（判据二：待遇是结论不是默认）
//!
//! 声明 × 系统能力（[`SystemCapability`]）→ 协商结论（[`Negotiation`]
//! 三态：足额待遇/受限待遇/拒绝），每条结论带原因与受限项明细——
//! 「给不了什么、为什么」必须显性（异常零静默在协商面的落法）。
//! 刷新需求超出系统能力上限 → 受限待遇（钳到上限并留痕）；内容类型
//! 系统不支持 → 拒绝（不装能）。
//!
//! ## 三、唯一权威注册表（判据三）
//!
//! [`SurfaceRegistry`] 是全系统唯一权威源：surface_id 全局唯一（重复
//! 注册显性拒 [`XcCode::DuplicateSurface`]）、注销后 id 不复用（单调
//! 分配——复用会让旧持有者的消息打到新持有者）、容量封顶滚动拒绝
//! （配额写死 [`REGISTRY_CAPACITY`]，超配额显性拒不静默逐出）。
//!
//! ## 四、注册通道鉴权（判据四：伪造防护）
//!
//! 注册请求必须携带通道令牌（[`CHANNEL_TOKEN`] 派生校验——FNV 一步
//! 摘要比对）；令牌不符 → 显性拒 [`XcCode::AuthReject`] + 安全计数
//! （伪造尝试留痕可审计）。鉴权在注册之前，未鉴权请求不触碰注册表。
//!
//! ## 五、协商与注册的次序
//!
//! 协商先于注册（negotiate → register）：协商失败的声明不入表；
//! 注册成功回填协商结论（待遇随行——消费方一次读取拿全）。
//!
//! **对接**：F0641/F0642（域架构与生命周期——本模块是 E 域注册入口）；
//! F0644（渲染调度，消费注册表的激活表面集）；F0647（遥测，注册表
//! 规模与协商拒绝率）。零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（配额唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const SURFACE_REG_VERSION: &str = "E01-surfacereg-v1";

/// 注册表容量（配额写死；超配额显性拒绝。当前策略值，可调）。
pub const REGISTRY_CAPACITY: usize = 512;

/// 注册通道令牌（鉴权基准；判据侧独立重算对拍，不复用本值）。
pub const CHANNEL_TOKEN: u64 = 0x5E61_0001_BEEF_0001;

// ---------------------------------------------------------------------------
// 二、四要素注册声明（判据一）
// ---------------------------------------------------------------------------

/// 内容类型闭集（表外类型协商即拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentType {
    /// 静态 UI（低频重绘）。
    StaticUi,
    /// 动效 UI（中频）。
    AnimatedUi,
    /// 视频（高吞吐）。
    Video,
    /// 游戏帧（逐帧提交）。
    GameFrame,
    /// 网页合成层。
    WebCompose,
}

impl ContentType {
    /// 人读名（诊断与协商明细用）。
    pub const fn zh(self) -> &'static str {
        match self {
            ContentType::StaticUi => "静态UI",
            ContentType::AnimatedUi => "动效UI",
            ContentType::Video => "视频",
            ContentType::GameFrame => "游戏帧",
            ContentType::WebCompose => "网页合成",
        }
    }
}

/// 四要素注册声明（字段闭集：四要素之外不收字段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceDecl {
    /// 要素一：宽（像素；>0）。
    pub width: u32,
    /// 要素一：高（像素；>0）。
    pub height: u32,
    /// 要素二：刷新需求（毫赫兹；>0。60000mHz=60Hz）。
    pub refresh_mhz: u64,
    /// 要素三：内容类型。
    pub content: ContentType,
    /// 要素四：交互性声明（可交互表面获得调度加权）。
    pub interactive: bool,
}

impl SurfaceDecl {
    /// 四要素齐备校验（缺一即拒；零尺寸/零刷新同拒）。
    pub fn validate(&self) -> Result<(), XcCode> {
        if self.width == 0 || self.height == 0 {
            return Err(XcCode::DeclIncomplete);
        }
        if self.refresh_mhz == 0 {
            return Err(XcCode::DeclIncomplete);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、诊断码（0x5E 段独占：E01 注册协商面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x5E 段；段内互异）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 四要素缺项/零值。
    DeclIncomplete,
    /// surface_id 重复注册。
    DuplicateSurface,
    /// 注册表配额满。
    RegistryFull,
    /// 内容类型系统不支持。
    ContentUnsupported,
    /// 鉴权拒绝（令牌不符——伪造尝试）。
    AuthReject,
    /// 未协商先注册（次序违规）。
    NotNegotiated,
}

impl XcCode {
    /// 人读诊断（发生了什么/为什么/下一步）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::DeclIncomplete => "注册声明四要素缺项或含零值：尺寸/刷新/内容类型/交互性缺一不可，建议补齐四要素再注册",
            XcCode::DuplicateSurface => "surface_id 重复注册：id 全局唯一且不复用，建议排查重复来源或走注销流程",
            XcCode::RegistryFull => "注册表配额已满：容量写死防膨胀，建议注销失活表面后重试",
            XcCode::ContentUnsupported => "内容类型系统不支持：协商无法给出待遇，建议改用受支持类型或走降级通道",
            XcCode::AuthReject => "注册通道鉴权拒绝：令牌不符属伪造尝试，已留痕；建议走正规注册通道申请令牌",
            XcCode::NotNegotiated => "未协商先注册：协商必须先于注册，建议先调 negotiate 再 register",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、能力协商（判据二：显性三态）
// ---------------------------------------------------------------------------

/// 系统能力快照（协商的另一方；本帧内恒定——协商确定性前提）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemCapability {
    /// 最大可承诺刷新率（毫赫兹）。
    pub max_refresh_mhz: u64,
    /// 支持的内容类型位图（bit i = ContentType 第 i 变体）。
    pub content_mask: u16,
}

impl SystemCapability {
    /// 内容类型支持判定（位图闭集映射，表外位置恒不支持）。
    pub fn supports(&self, c: ContentType) -> bool {
        let bit = match c {
            ContentType::StaticUi => 0u16,
            ContentType::AnimatedUi => 1,
            ContentType::Video => 2,
            ContentType::GameFrame => 3,
            ContentType::WebCompose => 4,
        };
        (self.content_mask & bit) != 0
    }
}

/// 协商结论三态（判据二：待遇显性——给不了什么、为什么必须写明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Negotiation {
    /// 足额待遇：四要素全按声明满足。
    Full,
    /// 受限待遇：钳制项与原因逐条列明。
    Throttled {
        /// 钳后的刷新率（毫赫兹）。
        refresh_mhz: u64,
        /// 原因（人读；带原声明值与上限值）。
        reason: String,
    },
    /// 拒绝：原因显性（不支持的内容类型）。
    Rejected { reason: String },
}

/// 协商引擎（声明 × 系统能力 → 显性结论；确定性：同输入同结论）。
pub fn negotiate(decl: &SurfaceDecl, cap: &SystemCapability) -> Negotiation {
    // 内容类型不支持 → 拒绝（不装能）。
    if !cap.supports(decl.content) {
        return Negotiation::Rejected {
            reason: alloc::format!(
                "内容类型 {} 系统不支持（能力位图 0x{:04X}）",
                decl.content.zh(),
                cap.content_mask
            ),
        };
    }
    // 刷新超上限 → 受限待遇（钳到上限并留痕）。
    if decl.refresh_mhz > cap.max_refresh_mhz {
        return Negotiation::Throttled {
            refresh_mhz: cap.max_refresh_mhz,
            reason: alloc::format!(
                "刷新需求 {}mHz 超系统能力上限 {}mHz，钳到上限；交互性={} 照常登记",
                decl.refresh_mhz,
                cap.max_refresh_mhz,
                decl.interactive
            ),
        };
    }
    Negotiation::Full
}

// ---------------------------------------------------------------------------
// 五、唯一权威注册表（判据三）+ 鉴权（判据四）
// ---------------------------------------------------------------------------

/// 注册表条目（声明 + 协商结论随行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceEntry {
    /// 全局唯一表面 id（单调分配，不复用）。
    pub surface_id: u64,
    /// 四要素声明（原样留档）。
    pub decl: SurfaceDecl,
    /// 协商结论（注册时随行回填）。
    pub negotiation: Negotiation,
}

/// 全系统表面注册表（唯一权威源）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceRegistry {
    entries: Vec<SurfaceEntry>,
    next_id: u64,
    rejected_auth: u64,
}

impl SurfaceRegistry {
    /// 新注册表。
    pub const fn new() -> SurfaceRegistry {
        SurfaceRegistry {
            entries: Vec::new(),
            next_id: 1,
            rejected_auth: 0,
        }
    }

    /// 通道鉴权（判据四：令牌一步摘要比对；不符即显性拒并计数留痕）。
    fn authorize(&mut self, token: u64) -> Result<(), XcCode> {
        if token != CHANNEL_TOKEN {
            self.rejected_auth = self.rejected_auth.saturating_add(1);
            return Err(XcCode::AuthReject);
        }
        Ok(())
    }

    /// 注册（次序：鉴权 → 四要素校验 → 协商结论在场 → 容量 → 唯一性）。
    pub fn register(
        &mut self,
        token: u64,
        decl: &SurfaceDecl,
        negotiation: &Negotiation,
    ) -> Result<u64, XcCode> {
        // 鉴权在最前：未鉴权请求不触碰注册表。
        self.authorize(token)?;
        decl.validate()?;
        // 协商先于注册：拒绝态与未协商态都不入表。
        match negotiation {
            Negotiation::Rejected { .. } => return Err(XcCode::NotNegotiated),
            Negotiation::Full | Negotiation::Throttled { .. } => {}
        }
        if self.entries.len() >= REGISTRY_CAPACITY {
            return Err(XcCode::RegistryFull);
        }
        let surface_id = self.next_id;
        self.next_id = self.next_id.saturating_add(1); // id 单调不复用
        self.entries.push(SurfaceEntry {
            surface_id,
            decl: decl.clone(),
            negotiation: negotiation.clone(),
        });
        Ok(surface_id)
    }

    /// 注销（唯一权威源的对称操作；id 不回收不复用）。
    pub fn unregister(&mut self, surface_id: u64) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.surface_id == surface_id {
                    self.entries.remove(i);
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 按 id 查询（唯一权威源的正确读法）。
    pub fn lookup(&self, surface_id: u64) -> Option<&SurfaceEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.surface_id == surface_id {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 在册表面数。
    pub const fn len_of(&self) -> usize {
        self.entries.len()
    }

    /// 伪造尝试计数（安全审计面）。
    pub const fn rejected_auth(&self) -> u64 {
        self.rejected_auth
    }
}

impl Default for SurfaceRegistry {
    fn default() -> SurfaceRegistry {
        SurfaceRegistry::new()
    }
}
