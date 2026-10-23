//! CGPU-F1928 · 显示输出安全（CGPU-M 域 · 显示组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1928`
//!
//! **判据（锚点原文）**：HDCP 矩阵、捕获复用、防护复用、三组、判据。
//!
//! **职责定位（锚点原文摘录）**：安全：显示安全（内容保护（HDCP（支持矩阵——HDCP 矩阵（诚实（能力显性——诚实复用；
//! 捕获控制（屏幕捕获权限（复用——捕获复用；黑屏攻击（恶意模式防护——防护复用 F1925——防护复用；测试（HDCP/捕获/防护
//! 三组）。判据：HDCP 矩阵、捕获复用、防护复用、三组、判据。
//!
//! ## 零、总览：一条输出路径，五道闸门，一本总账
//!
//! 显示输出是攻击者最"看得见"的攻击面：往帧缓冲里写什么、安全提示会不会
//! 被盖掉、谁能开屏幕捕获、显示器链路有没有内容保护——任何一环失守，
//! 用户看到的就是被伪造的世界。本模块把显示输出安全收敛为**一条被管的
//! 输出路径**，路径上依次设五道闸门，路径旁挂一本定长审计总账：
//!
//! 1. **HDCP 支持矩阵**（判据一，[`HdcpMatrix`]）：每个连接器的内容保护
//!    能力来自一张**写死的诚实能力表**——`None` 必须如实上报，协商请求
//!    超出能力时显性报 [`OutSecError::HdcpUnavailable`]，**绝不静默降级**。
//!    "诚实复用"在本模块落地为：协商函数永远复用同一张能力表，不存在
//!    第二条旁路上报路径。
//! 2. **捕获控制**（判据二，[`CaptureControl`]）：屏幕捕获是特权操作，
//!    普通档写者一律拒绝；存在活动安全覆盖层时，低于安全档的捕获只能
//!    得到"挖黑版"（[`CaptureOutcome::Blanked`]）——安全层内容绝不进入
//!    捕获结果。捕获复用判据落地为：捕获权限检查**复用**写者账的档位，
//!    不另立权限源。
//! 3. **黑屏攻击防护**（判据三，黑屏守卫）：连续多帧全黑整帧写是典型
//!    恶意模式（防护复用 F1925 的"恶意模式防护"语义）。守卫按写者记
//!    连续暗帧数，达到阈值即拉黑该写者并显性拦截
//!    （[`OutSecError::BlackscreenBlocked`]），全入账。
//! 4. **帧缓冲写权限账**（输出防篡改，[`WriterLedger`]）：写者注册制——
//!    未注册者写不进；注册时绑定档位与可写区域位图；每次写按**权限矩阵**
//!    （档位 × 区域安全级，[`permission_matrix`]）裁决；写后立即对该区域
//!    计算 FNV-1a 完整性校验和存入期望表，按固定步距抽查回读
//!    （[`OutSecCore::sample_integrity`]），篡改即报
//!    [`OutSecError::TamperDetected`]。
//! 5. **安全覆盖层栈**（[`OverlayStack`]）：安全提示/锁屏/捕获阻断三类
//!    安全覆盖层压在普通内容之上；**普通内容不可覆盖安全层**——任何与
//!    活动安全覆盖层矩形相交的写入，低于安全档一律拒绝
//!    （[`OutSecError::SecureLayerViolation`]）。覆盖栈严格按优先级递增
//!    入栈、只能从栈顶弹出（[`OutSecError::OverlayNotTop`]），栈序即合成序。
//!
//! 路径旁的**输出审计账**（[`AuditRing`]）：注册/写入/拒绝/覆盖/权限变更/
//! 捕获/HDCP/篡改告警/状态跃迁全部入账，定长环形（256 条），被覆盖的旧
//! 事件显性计数（`overflow`），绝不静默丢账。**篡改检测器**
//! （[`OutSecCore::run_tamper_scan`]）周期性做三路一致性对账：区域校验和
//! 回读、覆盖栈序与注册表对账、写者账与提交账对账——任何一路不一致即
//! 告警入账并把状态机推入告警态。
//!
//! 状态机（[`OutSecState`]）五态：`Offline → Init → Armed ⇄ Alert →
//! LockedDown`，跃迁表写死、非法跃迁显性拒绝并入账。`health()` 体检给
//! 三档结论；`fingerprint()` 用 FNV-1a 64 对全量状态做确定性指纹；
//! `run_dual_comparison()` 用同一份脚本在两个独立实例上双跑对拍指纹——
//! 分歧即确定性破缺。
//!
//! ## 纪律（本文件自守）
//!
//! * 完全自包含：只 `use core::…` / `use alloc::{vec::Vec, format, string…}`，
//!   禁 `use crate::…`；no_std + alloc。
//! * 内核路径零 `unwrap`/`expect`/`panic!`/运行期 `assert!`；可失败一律
//!   `Result<T, OutSecError>`，16 个变体恰好占满 0x1360~0x136F 诊断段，
//!   每变体 `detail()` 给中文可操作诊断。
//! * 溢出显性：计数全走 `saturating_add`，伪随机走 `wrapping`，地址合成
//!   走 `checked_add`，绝不静默回绕。
//! * `const fn` 自守：无 `slice::get`、无 `Vec` 索引、无 `Ord::min/max`、
//!   无派生 `PartialEq` 的 `==`、循环一律 `while`+索引。
//! * >64KB 结构体不上栈：帧缓冲 8KiB 常驻堆；测试载荷按需堆分配。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ===================================================================
// 常量与定容（全部写死并注明来源）
// ===================================================================

/// 诊断码段基址：0x1360。
/// （来源：CGPU-F1928 施工分配的独占诊断段；开工前已 `grep 0x136`
/// 扫描 `kernel/varix/src/cgpu/` 确认空闲。）
pub const DIAG_BASE: u32 = 0x1360;

/// 诊断码段上限：0x136F。16 个诊断码与本模块错误变体一一对应。
pub const DIAG_LIMIT: u32 = 0x136F;

/// 帧缓冲宽（像素）。
/// （来源：F1928 定容——安全子系统采用 128×64 单字节亮度仿真面作为
/// 完整性校验的真实载体；8 KiB 常驻堆，远低于 64 KiB 上栈红线。）
pub const FB_W: usize = 128;

/// 帧缓冲高（像素）。（来源：同 [`FB_W`]。）
pub const FB_H: usize = 64;

/// 帧缓冲总字节数 = 128 × 64 = 8192。
pub const FB_BYTES: usize = FB_W * FB_H;

/// 输出区域数：整帧 / 状态栏 / 内容区 / 安全面板。
/// （来源：F1928 判据"权限分级"——区域安全级三分需要至少四个典型区域
/// 才能同时覆盖公共、特权、安全三档。）
pub const REGION_COUNT: usize = 4;

/// 写者账容量上限：16。
/// （来源：F1928 写者注册制定容——显示输出路径的合法写者只有合成器、
/// 状态栏服务、安全提示服务、锁屏服务等个位数进程，16 留足裕量。）
pub const MAX_WRITERS: usize = 16;

/// 覆盖栈深度上限：8。
/// （来源：F1928 安全覆盖层定容——安全提示/锁屏/捕获阻断三类安全层
/// 加普通内容层，8 层已超出真实显示栈的合成深度。）
pub const OVERLAY_STACK_MAX: usize = 8;

/// 审计环形账容量：256 条。
/// （来源：F1928"审计"判据定容——定长环形保证内存上界确定，
/// 被覆盖旧事件显性计数，不静默丢账。）
pub const AUDIT_RING_CAP: usize = 256;

/// 写提交账容量：64 条。
/// （来源：F1928"输出防篡改"——提交账只服务近期写者×区域对账，
/// 64 条覆盖 16 写者 × 4 区域的一轮完整提交。）
pub const COMMIT_RING_CAP: usize = 64;

/// HDCP 连接器数：4。
/// （来源：F1928"HDCP 矩阵"——桌面级典型配置 DP×2 + HDMI×2。）
pub const CONNECTOR_COUNT: usize = 4;

/// 完整性抽查步距：每 7 次提交强制回读一次全部脏区。
/// （来源：F1928"输出防篡改"抽查频率定容；取素数 7，避免与任何
/// 帧周期/提交批量形成共振漏检。）
pub const INTEGRITY_SAMPLE_INTERVAL: u32 = 7;

/// 黑屏判定亮度上限（0~255 亮度标尺）。
/// （来源：F1928"黑屏攻击防护"——整帧平均亮度 ≤8 视为"有效全黑"，
/// 留 8 级给扫描噪声与真黑场校准帧。）
pub const BLACKSCREEN_LUMA_MAX: u16 = 8;

/// 黑屏攻击判定的连续暗帧阈值：3。
/// （来源：防护复用 F1925"恶意模式防护"语义——单帧全黑可能是合法
/// 熄屏指令，连续 3 帧才定性为攻击模式。）
pub const BLACKSCREEN_DARK_LIMIT: u8 = 3;

// 编译期铁律自证（const assert 允许且必须）。
const _: () = assert!(DIAG_LIMIT - DIAG_BASE == 15);
const _: () = assert!(FB_BYTES == FB_W * FB_H);
const _: () = assert!(FB_BYTES <= 64 * 1024);
const _: () = assert!(AUDIT_RING_CAP.is_power_of_two());
const _: () = assert!(COMMIT_RING_CAP.is_power_of_two());
const _: () = assert!(MAX_WRITERS <= 64);
const _: () = assert!(OVERLAY_STACK_MAX >= 2);
const _: () = assert!(BLACKSCREEN_DARK_LIMIT >= 2);
const _: () = assert!(INTEGRITY_SAMPLE_INTERVAL >= 2);

// ===================================================================
// 错误闭集：16 个变体恰好占满 0x1360~0x136F
// ===================================================================

/// 显示输出安全错误闭集。
///
/// 每个变体绑定一个独占诊断码（[`OutSecError::code`]，0x1360~0x136F）
/// 与一条中文可操作诊断（[`OutSecError::detail`]）。闭集设计：调用方
/// `match` 全枚举即可穷尽处理，新增变体即编译期破坏所有穷尽 match——
/// 这是刻意的 API 冻结策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutSecError {
    /// 子系统未完成初始化（状态机仍在 Offline）。
    NotInitialized,
    /// 当前状态机状态或账本状态不允许该操作（含非法跃迁、账满、回放不变量破坏）。
    InvalidState,
    /// 写者未注册——显示输出路径只认注册制写者。
    UnknownWriter,
    /// 写者名重复注册——同一输出主体只允许一个账目。
    DuplicateWriter,
    /// 写入被权限账拒绝（档位不满足区域安全级 / 区域未授权 / 写者已被拉黑）。
    WriteDenied,
    /// 写载荷尺寸与目标区域像素数不符。
    PayloadSizeMismatch,
    /// 普通内容试图覆盖活动安全覆盖层——安全层不可覆盖。
    SecureLayerViolation,
    /// 黑屏攻击防护拦截（连续暗帧达阈值，写者已拉黑）。
    BlackscreenBlocked,
    /// 覆盖栈中找不到指定覆盖层。
    OverlayNotFound,
    /// 覆盖栈已满（超过 [`OVERLAY_STACK_MAX`] 层）。
    OverlayStackFull,
    /// 只允许从栈顶弹出——指定覆盖层不在栈顶。
    OverlayNotTop,
    /// 高权限操作被拒（非安全档执行安全档专属操作 / 职责分离违规）。
    PermissionDenied,
    /// 升权被拒——档位只能降权即时生效，升权必须走审批路径。
    ApprovalRequired,
    /// HDCP 能力不可用（连接器不存在 / 请求等级超出诚实能力表，绝不静默降级）。
    HdcpUnavailable,
    /// 篡改检测命中（完整性校验和回读不符 / 双跑指纹分歧）。
    TamperDetected,
    /// 屏幕捕获被策略或档位拒绝。
    CaptureDenied,
}

impl OutSecError {
    /// 独占诊断码：`DIAG_BASE + 变体序号`，全程 0x1360~0x136F。
    pub const fn code(self) -> u32 {
        match self {
            OutSecError::NotInitialized => DIAG_BASE,
            OutSecError::InvalidState => DIAG_BASE + 1,
            OutSecError::UnknownWriter => DIAG_BASE + 2,
            OutSecError::DuplicateWriter => DIAG_BASE + 3,
            OutSecError::WriteDenied => DIAG_BASE + 4,
            OutSecError::PayloadSizeMismatch => DIAG_BASE + 5,
            OutSecError::SecureLayerViolation => DIAG_BASE + 6,
            OutSecError::BlackscreenBlocked => DIAG_BASE + 7,
            OutSecError::OverlayNotFound => DIAG_BASE + 8,
            OutSecError::OverlayStackFull => DIAG_BASE + 9,
            OutSecError::OverlayNotTop => DIAG_BASE + 10,
            OutSecError::PermissionDenied => DIAG_BASE + 11,
            OutSecError::ApprovalRequired => DIAG_BASE + 12,
            OutSecError::HdcpUnavailable => DIAG_BASE + 13,
            OutSecError::TamperDetected => DIAG_BASE + 14,
            OutSecError::CaptureDenied => DIAG_BASE + 15,
        }
    }

    /// 中文可操作诊断（闭集静态串；诊断码见 [`OutSecError::code`]）。
    pub const fn detail(self) -> &'static str {
        match self {
            OutSecError::NotInitialized => "子系统未初始化：先 request_state(Init) 完成初始化，再 request_state(Arm) 进入武装态后重试该操作。",
            OutSecError::InvalidState => "当前状态或账本状态不允许该操作：检查状态机状态（health() 可见），确认跃迁符合 Offline→Init→Armed⇄Alert→LockedDown 跃迁表，确认账本未满、回放序列未破坏。",
            OutSecError::UnknownWriter => "写者未注册：显示输出实行注册制，先 register_writer() 取得写者 ID，再携带该 ID 重试；不要复用已注销写者的 ID。",
            OutSecError::DuplicateWriter => "写者名重复：同一输出主体只允许一个账目，改用唯一名（建议：进程名+实例号）重新注册。",
            OutSecError::WriteDenied => "写入被拒：核对三件事——写者档位是否满足区域安全级（permission_matrix）、注册时 allow_mask 是否包含目标区域、写者是否已被黑屏防护拉黑（health().notes 可见）。",
            OutSecError::PayloadSizeMismatch => "载荷尺寸不符：载荷字节数必须精确等于目标区域像素数（REGIONS[region].rect.area()），请按区域表裁剪或补齐后重试。",
            OutSecError::SecureLayerViolation => "普通内容不可覆盖安全层：目标矩形与活动安全覆盖层相交且写者档位低于安全档；请缩小写入矩形避开安全层，或走审批升级为安全档。",
            OutSecError::BlackscreenBlocked => "黑屏攻击拦截：该写者连续整帧写入有效全黑内容达阈值，已被拉黑；合法熄屏请走显示功耗域 DPMS 路径，不要用全零帧实现。",
            OutSecError::OverlayNotFound => "覆盖层不存在：核对 push_overlay 返回的覆盖层 ID；已被弹出或从未注册的 ID 不可再操作。",
            OutSecError::OverlayStackFull => "覆盖栈已满：先 pop_overlay 释放栈顶再入栈；正常显示栈不应超过 8 层，超限说明合成结构异常，优先排查泄漏。",
            OutSecError::OverlayNotTop => "只许从栈顶弹出：覆盖栈按优先级严格递增管理，非栈顶覆盖层必须先等其上层全部弹出；如需跨层撤销，请整体重建覆盖栈。",
            OutSecError::PermissionDenied => "权限不足：该操作为安全档专属（或违反职责分离——审批人不得审批自己的升权请求）；请由安全档主体发起，或走 request_promotion 审批路径。",
            OutSecError::ApprovalRequired => "升权必须走审批：档位降权即时生效，升权不允许直改；先 request_promotion() 立请求，再由另一安全档主体 approve_promotion()。",
            OutSecError::HdcpUnavailable => "HDCP 能力不可用：请求的内容保护等级超出该连接器诚实能力表（或连接器不存在）；请按 honest_report() 选择该连接器实际支持的最高等级，本模块绝不静默降级。",
            OutSecError::TamperDetected => "篡改命中：区域完整性校验和回读与期望不符（或双跑指纹分歧）。先 run_tamper_scan() 定位不一致区域，排查越权 DMA 写入后重写该区域以清除告警。",
            OutSecError::CaptureDenied => "屏幕捕获被拒：捕获为特权操作，且全局策略可为 DenyAll；请确认捕获策略与写者档位，安全覆盖层活动期间低档捕获只能得到挖黑版。",
        }
    }
}

impl core::fmt::Display for OutSecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.detail())
    }
}

// ===================================================================
// 基础类型：档位、区域安全级、矩形、区域表、权限矩阵
// ===================================================================

/// 输出主体安全档位：普通 / 特权 / 安全 三档。
///
/// 判据"权限分级"的三档落地。档位只决定"能做什么"，不决定"是谁"——
/// 身份由写者账的注册名承担。档位序为 `Normal < Privileged < Security`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SafetyTier {
    /// 普通档：只能写公共级区域（内容区）。
    Normal,
    /// 特权档：可写特权级及以下（整帧、状态栏、内容区）。
    Privileged,
    /// 安全档：全区域可写，且是安全覆盖层/审批/注入面的唯一合法执行者。
    Security,
}

impl SafetyTier {
    /// 档位秩（1/2/3）。const fn 自守：match 取秩，不用派生序比较。
    pub const fn rank(self) -> u8 {
        match self {
            SafetyTier::Normal => 1,
            SafetyTier::Privileged => 2,
            SafetyTier::Security => 3,
        }
    }

    /// 状态指纹用稳定标签（0/1/2），永不随枚举增删变化。
    pub const fn tag(self) -> u8 {
        match self {
            SafetyTier::Normal => 0,
            SafetyTier::Privileged => 1,
            SafetyTier::Security => 2,
        }
    }
}

/// 区域安全级：公共 / 特权 / 安全 三级。
///
/// 与档位共用同一套秩比较：档位秩 ≥ 区域级秩 才放行
/// （[`matrix_allows`]）。等级来源：整帧与状态栏承载系统输出（特权），
/// 内容区是应用画布（公共），安全面板是安全提示落点（安全）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegionClass {
    /// 公共级：任何注册写者按授权位图可写。
    Public,
    /// 特权级：需要特权档及以上。
    Privileged,
    /// 安全级：仅安全档可写，且写入还要过覆盖层相交检查。
    Secure,
}

impl RegionClass {
    /// 区域级秩（1/2/3）。
    pub const fn rank(self) -> u8 {
        match self {
            RegionClass::Public => 1,
            RegionClass::Privileged => 2,
            RegionClass::Secure => 3,
        }
    }
}

/// 输出面矩形（像素坐标，左上原点，y 向下）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// 左上角 x。
    pub x: u16,
    /// 左上角 y。
    pub y: u16,
    /// 宽。
    pub w: u16,
    /// 高。
    pub h: u16,
}

impl Rect {
    /// 构造（const fn：纯算术，无派生相等比较）。
    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    /// 像素面积（const fn；u32 乘法无溢出风险：两维上限 128×64）。
    pub const fn area(&self) -> u32 {
        self.w as u32 * self.h as u32
    }

    /// 是否与另一矩形相交（运行期；坐标用 saturating 加法防回绕）。
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x.saturating_add(o.w)
            && o.x < self.x.saturating_add(self.w)
            && self.y < o.y.saturating_add(o.h)
            && o.y < self.y.saturating_add(self.h)
    }

    /// 是否完整落在帧缓冲界内（运行期；saturating 防回绕）。
    pub fn within_fb(&self) -> bool {
        self.w > 0
            && self.h > 0
            && (self.x as usize).saturating_add(self.w as usize) <= FB_W
            && (self.y as usize).saturating_add(self.h as usize) <= FB_H
    }
}

/// 命名输出区域（区域表条目）。id 即区域号，永不变更。
#[derive(Debug, Clone, Copy)]
pub struct NamedRegion {
    /// 区域号（0~3，与 [`REGIONS`] 下标一致）。
    pub id: usize,
    /// 区域名（审计账与人读报表使用）。
    pub name: &'static str,
    /// 区域矩形。
    pub rect: Rect,
    /// 区域安全级。
    pub class: RegionClass,
}

/// 输出区域常量表（写死，来源：F1928 权限分级判据的区域划分设计）：
///
/// | id | 名 | 矩形 | 安全级 | 说明 |
/// |----|------|------|--------|------|
/// | 0 | 整帧 | 0,0,128,64 | 特权 | 整帧重写是输出路径变更，特权档起 |
/// | 1 | 状态栏 | 0,0,128,8 | 特权 | 系统状态输出 |
/// | 2 | 内容区 | 0,8,128,48 | 公共 | 应用画布 |
/// | 3 | 安全面板 | 0,56,128,8 | 安全 | 安全提示/锁屏锚定条 |
pub const REGIONS: [NamedRegion; REGION_COUNT] = [
    NamedRegion {
        id: 0,
        name: "整帧",
        rect: Rect::new(0, 0, FB_W as u16, FB_H as u16),
        class: RegionClass::Privileged,
    },
    NamedRegion {
        id: 1,
        name: "状态栏",
        rect: Rect::new(0, 0, FB_W as u16, 8),
        class: RegionClass::Privileged,
    },
    NamedRegion {
        id: 2,
        name: "内容区",
        rect: Rect::new(0, 8, FB_W as u16, 48),
        class: RegionClass::Public,
    },
    NamedRegion {
        id: 3,
        name: "安全面板",
        rect: Rect::new(0, 56, FB_W as u16, 8),
        class: RegionClass::Secure,
    },
];

/// 权限矩阵：`[档位秩-1][区域级秩-1] -> 是否放行`。
///
/// const fn 手工展开（while+索引自守），编译期即可全表求值。
/// 与 [`matrix_allows`] 单点判定互为对拍——矩阵是表，判定是律，
/// 测试里逐格对拍保证两者永不分叉。
pub const fn permission_matrix() -> [[bool; 3]; 3] {
    let mut m = [[false; 3]; 3];
    let mut t = 0usize;
    while t < 3 {
        let mut c = 0usize;
        while c < 3 {
            m[t][c] = (t as u8 + 1) >= (c as u8 + 1);
            c += 1;
        }
        t += 1;
    }
    m
}

/// 单点权限判定律：档位秩 ≥ 区域级秩 即放行。
pub const fn matrix_allows(tier: SafetyTier, class: RegionClass) -> bool {
    tier.rank() >= class.rank()
}

/// 区域像素数（区域号非法返回 None——运行期显性失败，不静默钳制）。
pub fn region_pixel_count(region: usize) -> Option<usize> {
    if region >= REGION_COUNT {
        return None;
    }
    let r = &REGIONS[region];
    Some(r.rect.area() as usize)
}

/// 区域在帧缓冲中的 (偏移, 长度)（区域号非法或常量表越界返回 None）。
fn region_span(region: usize) -> Option<(usize, usize)> {
    if region >= REGION_COUNT {
        return None;
    }
    let r = &REGIONS[region];
    let off = r.rect.y as usize * FB_W + r.rect.x as usize;
    let len = r.rect.w as usize * r.rect.h as usize;
    if off.saturating_add(len) > FB_BYTES {
        return None;
    }
    Some((off, len))
}

/// FNV-1a 64 位散列。
///
/// 常量来源：Landon Curt Noll 公开的 FNV-1a 64 位参数——偏移基
/// `0xcbf29ce484222325`，质数 `0x100000001b3`。用于区域完整性校验和、
/// 审计摘要与全状态指纹。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ===================================================================
// 子系统一：HDCP 支持矩阵（判据一 · 诚实能力显性）
// ===================================================================

/// HDCP 内容保护等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HdcpLevel {
    /// 无内容保护（必须如实上报，不得伪装）。
    None,
    /// HDCP 1.0（DP/HDMI 早期版本）。
    V10,
    /// HDCP 2.0。
    V20,
    /// HDCP 2.2（4K 流媒体合规线）。
    V22,
}

impl HdcpLevel {
    /// 等级秩：None=0 < V10=1 < V20=2 < V22=3。
    pub const fn rank(self) -> u8 {
        match self {
            HdcpLevel::None => 0,
            HdcpLevel::V10 => 1,
            HdcpLevel::V20 => 2,
            HdcpLevel::V22 => 3,
        }
    }

    /// 人读标签（诚实报表用）。
    pub const fn label(self) -> &'static str {
        match self {
            HdcpLevel::None => "无保护",
            HdcpLevel::V10 => "HDCP 1.0",
            HdcpLevel::V20 => "HDCP 2.0",
            HdcpLevel::V22 => "HDCP 2.2",
        }
    }
}

/// HDCP 支持矩阵：每个连接器一条诚实能力记录。
///
/// "诚实复用"判据的落地形态：全模块只有这一张能力表（写死），协商、
/// 上报、审计引用全部经由它——不存在第二条能力旁路，因此不可能出现
/// "报表说无、协商却放行"的分裂。能力值来源：硬件盘点表（DP0/DP1
/// 支持 HDCP 2.2 与 2.0，HDMI0 仅 1.0，HDMI1 无保护——`None` 必须
/// 如实上报，这正是"诚实"判据的反向用例）。
pub struct HdcpMatrix {
    /// 连接器能力表，下标即连接器号。
    caps: [HdcpLevel; CONNECTOR_COUNT],
}

impl HdcpMatrix {
    /// 诚实能力表（const fn 写死，来源注释见类型文档）。
    pub const fn honest_table() -> HdcpMatrix {
        HdcpMatrix {
            caps: [HdcpLevel::V22, HdcpLevel::V22, HdcpLevel::V10, HdcpLevel::None],
        }
    }

    /// 连接器能力（const fn：数组直接下标，不用 slice::get）。
    /// 连接器号越界返回 None——显性失败，不静默钳制。
    pub fn capability(&self, connector: usize) -> Option<HdcpLevel> {
        if connector >= CONNECTOR_COUNT {
            return None;
        }
        Some(self.caps[connector])
    }

    /// 内容保护协商：请求等级 ≤ 连接器实际能力才放行。
    ///
    /// 诚实铁律：请求超出能力时返回 [`OutSecError::HdcpUnavailable`]，
    /// **绝不静默降级到低等级**——降级必须由调用方显式改参数重发。
    pub fn negotiate(
        &self,
        connector: usize,
        requested: HdcpLevel,
    ) -> Result<HdcpLevel, OutSecError> {
        let cap = match self.capability(connector) {
            Some(c) => c,
            None => return Err(OutSecError::HdcpUnavailable),
        };
        if requested.rank() > cap.rank() {
            return Err(OutSecError::HdcpUnavailable);
        }
        Ok(requested)
    }

    /// 诚实能力报表（人读；`None` 显性呈现为"无保护"）。
    pub fn honest_report(&self) -> String {
        let mut s = String::from("HDCP 诚实能力表:");
        let mut i = 0usize;
        while i < CONNECTOR_COUNT {
            if i > 0 {
                s.push(';');
            }
            s.push_str(&format!(" conn{}={}", i, self.caps[i].label()));
            i += 1;
        }
        s
    }
}

// ===================================================================
// 子系统二：捕获控制（判据二 · 捕获权限复用写者账档位）
// ===================================================================

/// 全局屏幕捕获策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturePolicy {
    /// 允许（仍需写者档位 ≥ 特权档；安全层活动时低档捕获得挖黑版）。
    Allowed,
    /// 全局禁止（连安全档也不放行——合规锁定期使用）。
    DenyAll,
}

/// 单次捕获裁决结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureOutcome {
    /// 放行：完整帧可捕获。
    Allowed,
    /// 挖黑放行：捕获可得，但安全覆盖层区域被挖黑。
    Blanked,
    /// 拒绝：本次捕获未发生。
    Denied,
}

/// 捕获控制账。
///
/// 判据"捕获复用"：本账不另立权限源，档位检查直接复用写者账的
/// [`SafetyTier`]——写者账是唯一权限事实源。
#[derive(Debug, Clone)]
pub struct CaptureControl {
    /// 当前全局策略。
    pub policy: CapturePolicy,
    /// 捕获尝试总数（saturating）。
    pub attempts: u32,
    /// 完整放行次数。
    pub granted: u32,
    /// 挖黑放行次数。
    pub blanked: u32,
    /// 拒绝次数。
    pub denied: u32,
}

impl CaptureControl {
    /// 新账：默认策略 Allowed（来源：F1928 判据——捕获默认受档位管，
    /// 全局禁捕是合规锁定期才启用的紧缩态）。
    pub const fn new() -> CaptureControl {
        CaptureControl {
            policy: CapturePolicy::Allowed,
            attempts: 0,
            granted: 0,
            blanked: 0,
            denied: 0,
        }
    }
}

// ===================================================================
// 子系统三：写者账（输出防篡改 · 写者注册制）
// ===================================================================

/// 写者账记录：一个输出主体的全部安全事实。
#[derive(Debug, Clone)]
pub struct WriterRecord {
    /// 写者 ID（从 1 起单调分配，0 保留给系统）。
    pub id: u32,
    /// 注册名（唯一；审计账与报表使用）。
    pub name: String,
    /// 当前安全档位。
    pub tier: SafetyTier,
    /// 可写区域位图：bit i = 区域 i 授权（i < REGION_COUNT 之外位忽略）。
    pub allow_mask: u8,
    /// 是否在册活动（注销后保留记录但拒绝操作）。
    pub active: bool,
    /// 是否已被黑屏防护拉黑（拉黑后所有写请求一律拒绝）。
    pub blacklisted: bool,
    /// 累计成功提交次数（saturating）。
    pub committed: u32,
    /// 累计被拒次数（saturating）。
    pub denied: u32,
    /// 连续暗帧计数（黑屏守卫状态，见 [`BLACKSCREEN_DARK_LIMIT`]）。
    pub dark_streak: u8,
    /// 注册时系统节拍（审计溯源）。
    pub registered_tick: u64,
}

impl WriterRecord {
    /// 区域授权位检查（运行期；区域号越界一律不放行）。
    pub fn region_allowed(&self, region: usize) -> bool {
        if region >= REGION_COUNT {
            return false;
        }
        (self.allow_mask >> region) & 1 == 1
    }
}

/// 写者账：注册制 + 唯一名 + 档位事实源。
pub struct WriterLedger {
    /// 记录数组（含已注销者——账只增不删，保证 ID 溯源链完整）。
    pub(crate) writers: Vec<WriterRecord>,
    /// 下一个写者 ID。
    next_id: u32,
}

impl WriterLedger {
    /// 空账。
    pub fn new() -> WriterLedger {
        WriterLedger {
            writers: Vec::new(),
            next_id: 1,
        }
    }

    /// 按 ID 查下标。
    pub fn index_of(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.writers.len() {
            if self.writers[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 按 ID 查记录（只读克隆——调用方快照检查用，避免借用冲突）。
    pub fn snapshot(&self, id: u32) -> Option<WriterRecord> {
        self.index_of(id).map(|i| self.writers[i].clone())
    }

    /// 注册名唯一性检查。
    pub fn name_exists(&self, name: &str) -> bool {
        let mut i = 0usize;
        while i < self.writers.len() {
            if self.writers[i].name.as_str() == name {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 注册新写者。账满或重名显性失败。
    pub fn register(
        &mut self,
        name: &str,
        tier: SafetyTier,
        allow_mask: u8,
        tick: u64,
    ) -> Result<u32, OutSecError> {
        if self.writers.len() >= MAX_WRITERS {
            return Err(OutSecError::InvalidState);
        }
        if self.name_exists(name) {
            return Err(OutSecError::DuplicateWriter);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.writers.push(WriterRecord {
            id,
            name: String::from(name),
            tier,
            allow_mask,
            active: true,
            blacklisted: false,
            committed: 0,
            denied: 0,
            dark_streak: 0,
            registered_tick: tick,
        });
        Ok(id)
    }

    /// 活动写者计数（篡改检测器对账用）。
    pub fn active_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.writers.len() {
            if self.writers[i].active {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 账内记录总数（容量对账用）。
    pub fn total_count(&self) -> usize {
        self.writers.len()
    }
}

// ===================================================================
// 子系统四：安全覆盖层栈（安全层不可覆盖）
// ===================================================================

/// 覆盖层种类。前三类是安全覆盖层（[`OverlayKind::is_security`]），
/// 普通内容层永远不得与其相交写入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    /// 安全提示（告警横幅、合规提示）。
    SecurityNotice,
    /// 锁屏（全屏接管）。
    LockScreen,
    /// 捕获阻断条（标记"该区域不得进入捕获结果"）。
    CaptureBlock,
    /// 普通内容层（应用窗口、悬浮层）。
    Content,
}

impl OverlayKind {
    /// 是否安全覆盖层（const fn：match 闭式判定，不用派生相等）。
    pub const fn is_security(self) -> bool {
        match self {
            OverlayKind::SecurityNotice => true,
            OverlayKind::LockScreen => true,
            OverlayKind::CaptureBlock => true,
            OverlayKind::Content => false,
        }
    }

    /// 人读标签。
    pub const fn label(self) -> &'static str {
        match self {
            OverlayKind::SecurityNotice => "安全提示",
            OverlayKind::LockScreen => "锁屏",
            OverlayKind::CaptureBlock => "捕获阻断",
            OverlayKind::Content => "普通内容",
        }
    }

    /// 稳定标签（状态指纹编码用，永不复号）。
    pub const fn tag(self) -> u8 {
        match self {
            OverlayKind::SecurityNotice => 0,
            OverlayKind::LockScreen => 1,
            OverlayKind::CaptureBlock => 2,
            OverlayKind::Content => 3,
        }
    }
}

/// 覆盖层记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overlay {
    /// 覆盖层 ID（从 1 起单调分配）。
    pub id: u32,
    /// 种类。
    pub kind: OverlayKind,
    /// 覆盖矩形。
    pub rect: Rect,
    /// 优先级（栈内自底向上严格递增；栈序即合成序）。
    pub priority: u16,
    /// 属主写者 ID（弹出权限校验用）。
    pub owner: u32,
    /// 是否活动（入栈即活动；扫描对账要求栈内记录全部活动）。
    pub active: bool,
}

/// 覆盖层栈：Vec 尾端即栈顶，严格按优先级递增管理。
///
/// 栈序管理三律（判据"安全覆盖层"的栈序语义）：
/// 1. 入栈优先级必须严格大于现栈顶（否则整栈序被破坏，显性拒绝）；
/// 2. 只能从栈顶弹出，指定 ID 非栈顶即 [`OutSecError::OverlayNotTop`]；
/// 3. 安全覆盖层入栈需安全档属主，普通内容层入栈若与任何活动安全层
///    矩形相交且属主低于安全档，即 [`OutSecError::SecureLayerViolation`]。
pub struct OverlayStack {
    /// 栈体（下标 0 是栈底，len-1 是栈顶）。
    pub(crate) stack: Vec<Overlay>,
    next_id: u32,
}

impl OverlayStack {
    /// 空栈。
    pub fn new() -> OverlayStack {
        OverlayStack {
            stack: Vec::new(),
            next_id: 1,
        }
    }

    /// 栈深。
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// 是否空栈。
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// 栈顶（只读克隆；空栈返回 None）。
    pub fn top(&self) -> Option<Overlay> {
        if self.stack.is_empty() {
            return None;
        }
        Some(self.stack[self.stack.len() - 1])
    }

    /// 活动安全覆盖层矩形列表（写路径相交检查与捕获挖黑共用）。
    pub fn active_security_rects(&self) -> Vec<Rect> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.stack.len() {
            let o = &self.stack[i];
            if o.active && o.kind.is_security() {
                out.push(o.rect);
            }
            i += 1;
        }
        out
    }

    /// 指定矩形是否与任何活动安全覆盖层相交。
    pub fn hits_security(&self, rect: &Rect) -> bool {
        let mut i = 0usize;
        while i < self.stack.len() {
            let o = &self.stack[i];
            if o.active && o.kind.is_security() && o.rect.intersects(rect) {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ===================================================================
// 子系统五：写提交账（环形）与审计账（环形）
// ===================================================================

/// 一次成功写提交的完整记录（完整性对账的事实源）。
#[derive(Debug, Clone, Copy)]
pub struct WriteCommit {
    /// 提交序号（从 1 起单调）。
    pub seq: u64,
    /// 写者 ID。
    pub writer: u32,
    /// 区域号。
    pub region: usize,
    /// 提交时区域 FNV-1a 校验和（期望值）。
    pub checksum: u64,
    /// 提交字节数。
    pub len: u32,
    /// 提交时系统节拍。
    pub tick: u64,
}

/// 写提交环形账：定长 [`COMMIT_RING_CAP`]，溢出显性计数。
pub struct CommitRing {
    buf: Vec<WriteCommit>,
    head: usize,
    filled: usize,
    /// 被覆盖的旧提交数（显性溢出账）。
    pub evicted: u64,
    next_seq: u64,
}

impl CommitRing {
    /// 空环（预分配 CAP 条占位）。
    pub fn new() -> CommitRing {
        CommitRing {
            buf: Vec::new(),
            head: 0,
            filled: 0,
            evicted: 0,
            next_seq: 1,
        }
    }

    /// 入环（满则覆盖最旧并计数）。返回分配的提交序号。
    pub fn push(&mut self, item: WriteCommit) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        if self.filled < COMMIT_RING_CAP {
            self.buf.push(item);
            self.filled = self.filled.saturating_add(1);
        } else {
            self.buf[self.head] = item;
            self.head = (self.head + 1) % COMMIT_RING_CAP;
            self.evicted = self.evicted.saturating_add(1);
        }
        seq
    }

    /// 自旧到新遍历（提交序号递增序）。
    pub fn iter_oldest_first(&self) -> Vec<WriteCommit> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.filled {
            let idx = if self.filled < COMMIT_RING_CAP {
                i
            } else {
                (self.head + i) % COMMIT_RING_CAP
            };
            out.push(self.buf[idx]);
            i += 1;
        }
        out
    }

    /// 某写者的在环提交数（仅无淘汰时可用于对账）。
    pub fn count_for_writer(&self, writer: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.filled {
            let idx = if self.filled < COMMIT_RING_CAP {
                i
            } else {
                (self.head + i) % COMMIT_RING_CAP
            };
            if self.buf[idx].writer == writer {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 在环条数。
    pub fn len(&self) -> usize {
        self.filled
    }
}

/// 审计事件种类（输出路径变更/覆盖操作/权限变更/安全事件的闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditKind {
    /// 写者注册。
    WriterRegistered,
    /// 写者注销。
    WriterRemoved,
    /// 写请求被拒（码字段携带具体拒绝诊断码）。
    WriterDenied,
    /// 写提交成功。
    WriteCommitted,
    /// 覆盖层入栈。
    OverlayPushed,
    /// 覆盖层出栈。
    OverlayPopped,
    /// 覆盖层入栈被拒。
    OverlayPushDenied,
    /// 档位降权（即时生效）。
    TierDemoted,
    /// 升权请求建立。
    PromotionRequested,
    /// 升权审批通过。
    PromotionApproved,
    /// 升权审批驳回。
    PromotionRejected,
    /// 捕获请求入账。
    CaptureRequested,
    /// 捕获被拒入账。
    CaptureDenied,
    /// 捕获挖黑放行入账。
    CaptureGrantedProxy,
    /// HDCP 协商成功。
    HdcpNegotiated,
    /// HDCP 协商被拒（诚实失败，绝不静默降级）。
    HdcpRefused,
    /// 黑屏攻击定性入账（写者拉黑）。
    BlackscreenFlagged,
    /// 篡改告警。
    TamperAlert,
    /// 篡改扫描完成。
    ScanCompleted,
    /// 注入面操作入账（越权写注入/篡改注入均留痕）。
    InjectionWrite,
    /// 状态机跃迁（aux 编码 from<<8|to 的状态标签）。
    StateChanged,
    /// 非法状态跃迁被拒。
    StateChangeRejected,
}

impl AuditKind {
    /// 稳定标签（指纹/摘要编码用，永不复号）。
    pub const fn tag(self) -> u8 {
        match self {
            AuditKind::WriterRegistered => 0,
            AuditKind::WriterRemoved => 1,
            AuditKind::WriterDenied => 2,
            AuditKind::WriteCommitted => 3,
            AuditKind::OverlayPushed => 4,
            AuditKind::OverlayPopped => 5,
            AuditKind::OverlayPushDenied => 6,
            AuditKind::TierDemoted => 7,
            AuditKind::PromotionRequested => 8,
            AuditKind::PromotionApproved => 9,
            AuditKind::PromotionRejected => 10,
            AuditKind::CaptureRequested => 11,
            AuditKind::CaptureDenied => 12,
            AuditKind::HdcpNegotiated => 13,
            AuditKind::HdcpRefused => 14,
            AuditKind::BlackscreenFlagged => 15,
            AuditKind::TamperAlert => 16,
            AuditKind::ScanCompleted => 17,
            AuditKind::InjectionWrite => 18,
            AuditKind::StateChanged => 19,
            AuditKind::StateChangeRejected => 20,
            AuditKind::CaptureGrantedProxy => 21,
        }
    }
}

/// 一条审计事件。
#[derive(Debug, Clone)]
pub struct AuditEvent {
    /// 事件序号（从 1 起单调；环形窗口内必须严格递增——回放不变量）。
    pub seq: u64,
    /// 事件时系统节拍。
    pub tick: u64,
    /// 事件种类。
    pub kind: AuditKind,
    /// 主体写者 ID（0 = 系统）。
    pub actor: u32,
    /// 关联诊断码（拒绝类事件携带 OutSecError::code()；通过类为 0）。
    pub code: u32,
    /// 事件种类专属辅助字段（StateChanged 编码 from<<8|to 状态标签）。
    pub aux: u32,
    /// 人读附注。
    pub note: String,
}

/// 审计环形账：定长 [`AUDIT_RING_CAP`]，旧事件被覆盖时 `overflow`
/// 显性计数——绝不静默丢账（判据"审计"）。
pub struct AuditRing {
    buf: Vec<AuditEvent>,
    head: usize,
    /// 在环条数。
    pub filled: usize,
    /// 被覆盖的旧事件数（显性溢出账）。
    pub overflow: u64,
    next_seq: u64,
}

impl AuditRing {
    /// 空账。
    pub fn new() -> AuditRing {
        AuditRing {
            buf: Vec::new(),
            head: 0,
            filled: 0,
            overflow: 0,
            next_seq: 1,
        }
    }

    /// 入账（满则覆盖最旧并显性计数）。返回分配的事件序号。
    pub fn push(&mut self, event: AuditEvent) -> u64 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        let mut e = event;
        e.seq = seq;
        if self.filled < AUDIT_RING_CAP {
            self.buf.push(e);
            self.filled = self.filled.saturating_add(1);
        } else {
            self.buf[self.head] = e;
            self.head = (self.head + 1) % AUDIT_RING_CAP;
            self.overflow = self.overflow.saturating_add(1);
        }
        seq
    }

    /// 自旧到新遍历（序号递增序）。
    pub fn iter_oldest_first(&self) -> Vec<AuditEvent> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.filled {
            let idx = if self.filled < AUDIT_RING_CAP {
                i
            } else {
                (self.head + i) % AUDIT_RING_CAP
            };
            out.push(self.buf[idx].clone());
            i += 1;
        }
        out
    }

    /// 指定种类的事件计数（测试与体检用）。
    pub fn count_kind(&self, kind: AuditKind) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.filled {
            let idx = if self.filled < AUDIT_RING_CAP {
                i
            } else {
                (self.head + i) % AUDIT_RING_CAP
            };
            if self.buf[idx].kind == kind {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 审计摘要：对 (环状态 + 全部事件字段) 做 FNV-1a 64。
    ///
    /// 双跑对拍与回放对拍的比较基准；任何字段（含附注字节）漂移即
    /// 摘要分叉。
    pub fn digest(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(self.filled * 48 + 32);
        b.extend_from_slice(&(self.filled as u64).to_be_bytes());
        b.extend_from_slice(&self.overflow.to_be_bytes());
        let events = self.iter_oldest_first();
        let mut i = 0usize;
        while i < events.len() {
            let e = &events[i];
            b.extend_from_slice(&e.seq.to_be_bytes());
            b.extend_from_slice(&e.tick.to_be_bytes());
            b.push(e.kind.tag());
            b.extend_from_slice(&e.actor.to_be_bytes());
            b.extend_from_slice(&e.code.to_be_bytes());
            b.extend_from_slice(&e.aux.to_be_bytes());
            b.extend_from_slice(e.note.as_bytes());
            b.push(0xFF);
            i += 1;
        }
        fnv1a64(&b)
    }
}

// ===================================================================
// 子系统六：升权审批账（降权即时 · 升权走审批）
// ===================================================================

/// 一条升权请求。
#[derive(Debug, Clone, Copy)]
pub struct PromotionRequest {
    /// 请求 ID（从 1 起单调）。
    pub req_id: u32,
    /// 目标写者 ID。
    pub target: u32,
    /// 原档位。
    pub from: SafetyTier,
    /// 申请目标档位。
    pub to: SafetyTier,
    /// 请求时系统节拍。
    pub tick: u64,
    /// 是否待决（批准/驳回后关闭；关闭的请求不可再审）。
    pub open: bool,
}

/// 升权审批账。
///
/// 判据"权限分级"的审批路径落地：降权即时生效、全程入账；升权必须
/// 先立请求，再由**另一名**安全档主体批准（职责分离：审批人 ≠ 申请人），
/// 审批通过/驳回同样留痕。
pub struct ApprovalLedger {
    /// 请求账（只增不删；关闭的请求保留作审计证据）。
    pub reqs: Vec<PromotionRequest>,
    next_id: u32,
    /// 累计批准数。
    pub approved: u32,
    /// 累计驳回数。
    pub rejected: u32,
}

impl ApprovalLedger {
    /// 空账。
    pub fn new() -> ApprovalLedger {
        ApprovalLedger {
            reqs: Vec::new(),
            next_id: 1,
            approved: 0,
            rejected: 0,
        }
    }

    /// 立请求（同目标已有待决请求则显性失败）。
    pub fn open_request(
        &mut self,
        target: u32,
        from: SafetyTier,
        to: SafetyTier,
        tick: u64,
    ) -> Result<u32, OutSecError> {
        let mut i = 0usize;
        while i < self.reqs.len() {
            let r = &self.reqs[i];
            if r.open && r.target == target {
                return Err(OutSecError::InvalidState);
            }
            i += 1;
        }
        let req_id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.reqs.push(PromotionRequest {
            req_id,
            target,
            from,
            to,
            tick,
            open: true,
        });
        Ok(req_id)
    }

    /// 查待决请求（只读克隆）。
    pub fn open_request_of(&self, req_id: u32) -> Option<PromotionRequest> {
        let mut i = 0usize;
        while i < self.reqs.len() {
            if self.reqs[i].req_id == req_id && self.reqs[i].open {
                return Some(self.reqs[i]);
            }
            i += 1;
        }
        None
    }

    /// 关闭请求（审批终点共用）。
    pub fn close(&mut self, req_id: u32) {
        let mut i = 0usize;
        while i < self.reqs.len() {
            if self.reqs[i].req_id == req_id {
                self.reqs[i].open = false;
                return;
            }
            i += 1;
        }
    }
}

// ===================================================================
// 子系统七：输出安全状态机
// ===================================================================

/// 输出安全状态机五态。
///
/// 跃迁表（写死，[`OutSecState::transition_legal`] 为唯一裁决）：
///
/// ```text
/// Offline --Init--> Init --Arm--> Armed --AlertRaised--> Alert
/// Alert --ScanClean--> Armed        （篡改扫描全绿后回到武装态）
/// Armed/Alert --Lockdown--> LockedDown --Reset--> Offline（重启复位）
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutSecState {
    /// 下电/未初始化：只允许初始化跃迁。
    Offline,
    /// 初始化：允许配置（注册写者、HDCP 协商），不允许内容写。
    Init,
    /// 武装：输出路径全功能放行（写、覆盖、捕获、审批）。
    Armed,
    /// 告警：篡改检测命中；内容写仍放行但体检必须非绿，等待扫描洗白或锁定。
    Alert,
    /// 锁定：一切内容操作冻结，仅允许 Reset 回 Offline（等效重启）。
    LockedDown,
}

impl OutSecState {
    /// 稳定标签（指纹/审计 aux 编码用）。
    pub const fn tag(self) -> u8 {
        match self {
            OutSecState::Offline => 0,
            OutSecState::Init => 1,
            OutSecState::Armed => 2,
            OutSecState::Alert => 3,
            OutSecState::LockedDown => 4,
        }
    }

    /// 由标签还原状态（未知标签回退 Offline——审计数据可能来自旧版本）。
    pub const fn from_tag(t: u8) -> OutSecState {
        match t {
            1 => OutSecState::Init,
            2 => OutSecState::Armed,
            3 => OutSecState::Alert,
            4 => OutSecState::LockedDown,
            _ => OutSecState::Offline,
        }
    }

    /// 跃迁合法性（const fn：match 闭式裁决，不用派生相等比较）。
    pub const fn transition_legal(from: OutSecState, to: OutSecState) -> bool {
        match from {
            OutSecState::Offline => matches!(to, OutSecState::Init),
            OutSecState::Init => matches!(to, OutSecState::Armed),
            OutSecState::Armed => matches!(to, OutSecState::Alert | OutSecState::LockedDown),
            OutSecState::Alert => matches!(to, OutSecState::Armed | OutSecState::LockedDown),
            OutSecState::LockedDown => matches!(to, OutSecState::Offline),
        }
    }

    /// 配置类操作（注册/HDCP/状态跃迁）允许的状态。
    pub const fn provision_ok(self) -> bool {
        matches!(self, OutSecState::Init | OutSecState::Armed)
    }

    /// 内容类操作（写/覆盖/捕获/审批）允许的状态。
    ///
    /// 告警态仍允许内容写：显示输出在告警期间保持可用（用户仍要看
    /// 安全提示），且重写失配区域正是洗白告警的正道。
    pub const fn content_ok(self) -> bool {
        matches!(self, OutSecState::Armed | OutSecState::Alert)
    }

    /// 扫描允许的状态（武装态例行扫、告警态洗白扫）。
    pub const fn scan_ok(self) -> bool {
        matches!(self, OutSecState::Armed | OutSecState::Alert)
    }
}

/// 状态跃迁事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateEvent {
    /// 初始化。
    Init,
    /// 武装。
    Arm,
    /// 篡改告警拉响。
    AlertRaised,
    /// 扫描全绿洗白。
    ScanClean,
    /// 安全锁定。
    Lockdown,
    /// 复位（LockedDown 专用，等效重启）。
    Reset,
}

impl StateEvent {
    /// 由当前状态求目标状态（const fn；无合法目标返回 None）。
    pub const fn target(self, from: OutSecState) -> Option<OutSecState> {
        match from {
            OutSecState::Offline => match self {
                StateEvent::Init => Some(OutSecState::Init),
                _ => None,
            },
            OutSecState::Init => match self {
                StateEvent::Arm => Some(OutSecState::Armed),
                _ => None,
            },
            OutSecState::Armed => match self {
                StateEvent::AlertRaised => Some(OutSecState::Alert),
                StateEvent::Lockdown => Some(OutSecState::LockedDown),
                _ => None,
            },
            OutSecState::Alert => match self {
                StateEvent::ScanClean => Some(OutSecState::Armed),
                StateEvent::Lockdown => Some(OutSecState::LockedDown),
                _ => None,
            },
            OutSecState::LockedDown => match self {
                StateEvent::Reset => Some(OutSecState::Offline),
                _ => None,
            },
        }
    }
}

// ===================================================================
// 核心门面：OutSecCore
// ===================================================================

/// 写提交回执。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteReceipt {
    /// 提交序号。
    pub seq: u64,
    /// 区域号。
    pub region: usize,
    /// 写入字节数。
    pub len: u32,
    /// 区域 FNV-1a 完整性校验和（期望值，写入期望表）。
    pub checksum: u64,
    /// 提交时系统节拍。
    pub tick: u64,
}

/// 完整性回读裁决。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrityVerdict {
    /// 回读校验和与期望一致。
    Intact,
    /// 不一致——篡改命中。
    Tampered,
}

/// 篡改扫描报告。
#[derive(Debug, Clone)]
pub struct TamperScanReport {
    /// 三路对账是否全部一致。
    pub consistent: bool,
    /// 校验和回读核对的区域数。
    pub regions_checked: usize,
    /// 校验和不符的区域号列表。
    pub mismatched_regions: Vec<usize>,
    /// 对账的写者数。
    pub writers_checked: usize,
    /// 对账的覆盖层数。
    pub overlays_checked: usize,
    /// 本次扫描新增告警数。
    pub alerts_added: u32,
    /// 扫描后状态机状态。
    pub state_after: OutSecState,
}

/// 体检结论三档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthVerdict {
    /// 全绿：无篡改告警、未锁定、审计无溢出、无拒绝记录。
    Green,
    /// 黄：存在拒绝记录/审计溢出/处于告警态——可运行但需关注。
    Amber,
    /// 红：有篡改告警或已锁定——输出路径不可信。
    Red,
}

/// 体检报告（`health()`）。
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// 当前状态。
    pub state: OutSecState,
    /// 结论。
    pub verdict: HealthVerdict,
    /// 活动写者数。
    pub writers_active: u32,
    /// 覆盖栈深。
    pub overlays_active: usize,
    /// 审计在环条数。
    pub audit_len: usize,
    /// 审计显性溢出计数。
    pub audit_overflow: u64,
    /// 累计篡改告警数。
    pub tamper_alerts: u32,
    /// 累计完整性与抽查次数。
    pub integrity_samples: u32,
    /// 累计成功提交数。
    pub writes_committed: u32,
    /// 累计被拒数。
    pub writes_denied: u32,
    /// 人读体检附注。
    pub notes: Vec<String>,
}

/// 审计回放报告。
#[derive(Debug, Clone)]
pub struct ReplayReport {
    /// 回放事件数。
    pub events: u32,
    /// 其中状态跃迁事件数。
    pub state_events: u32,
    /// 回放终态。
    pub replay_state: OutSecState,
    /// 审计摘要（与 [`OutSecCore::audit_digest`] 一致性对拍用）。
    pub digest: u64,
}

/// 双跑对拍报告。
#[derive(Debug, Clone)]
pub struct DualRunReport {
    /// 脚本步数。
    pub steps: usize,
    /// 是否全程一致。
    pub matched: bool,
    /// 首个分歧步（None = 无分歧）。
    pub diverged_step: Option<usize>,
    /// A 实例终态指纹。
    pub fingerprint_a: u64,
    /// B 实例终态指纹。
    pub fingerprint_b: u64,
    /// A 实例审计摘要。
    pub audit_digest_a: u64,
    /// B 实例审计摘要。
    pub audit_digest_b: u64,
}

/// 显示输出安全核心：一条被管的输出路径 + 五道闸门 + 一本总账。
///
/// 所有可变状态集中于此；对外只暴露语义化方法，字段全部私有，
/// 保证任何状态变更都必然经过权限裁决与审计入账。
pub struct OutSecCore {
    /// 状态机当前状态。
    state: OutSecState,
    /// 系统节拍（调用方推进；确定性双跑的对钟源）。
    tick: u64,
    /// 帧缓冲（8 KiB，常驻堆；单字节亮度面，完整性校验的真实载体）。
    fb: Vec<u8>,
    /// 写者账。
    writers: WriterLedger,
    /// 覆盖层栈。
    overlays: OverlayStack,
    /// 写提交账。
    commits: CommitRing,
    /// 审计总账。
    audit: AuditRing,
    /// 升权审批账。
    approvals: ApprovalLedger,
    /// HDCP 诚实能力矩阵。
    hdcp: HdcpMatrix,
    /// 捕获控制账。
    capture: CaptureControl,
    /// 各区域期望校验和（写后完整性基线）。
    region_expected: [u64; REGION_COUNT],
    /// 各区域脏标（写后未回读过 = true；扫描/回读核过后清位）。
    region_dirty: [bool; REGION_COUNT],
    /// 累计篡改告警数。
    tamper_alerts: u32,
    /// 累计扫描次数。
    scans: u32,
    /// 累计成功提交数。
    writes_committed: u32,
    /// 累计被拒写数。
    writes_denied: u32,
    /// 累计完整性抽查次数。
    integrity_samples: u32,
}

impl OutSecCore {
    /// 新核心：Offline 态、零帧缓冲（全黑）、全空账本、诚实 HDCP 表。
    pub fn new() -> OutSecCore {
        OutSecCore {
            state: OutSecState::Offline,
            tick: 0,
            fb: vec![0u8; FB_BYTES],
            writers: WriterLedger::new(),
            overlays: OverlayStack::new(),
            commits: CommitRing::new(),
            audit: AuditRing::new(),
            approvals: ApprovalLedger::new(),
            hdcp: HdcpMatrix::honest_table(),
            capture: CaptureControl::new(),
            region_expected: [0u64; REGION_COUNT],
            region_dirty: [false; REGION_COUNT],
            tamper_alerts: 0,
            scans: 0,
            writes_committed: 0,
            writes_denied: 0,
            integrity_samples: 0,
        }
    }

    // ---------- 时钟与状态机 ----------

    /// 推进系统节拍（调用方是唯一时钟源——确定性双跑的根基）。
    pub fn advance_tick(&mut self, delta: u64) {
        self.tick = self.tick.wrapping_add(delta);
    }

    /// 当前状态。
    pub const fn state(&self) -> OutSecState {
        self.state
    }

    /// 当前节拍。
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// 发起状态跃迁：跃迁表裁决，非法跃迁显性拒绝并入账。
    pub fn request_state(&mut self, ev: StateEvent) -> Result<OutSecState, OutSecError> {
        match ev.target(self.state) {
            Some(to) => {
                let aux = ((self.state.tag() as u32) << 8) | to.tag() as u32;
                self.state = to;
                self.audit_push(AuditKind::StateChanged, 0, 0, aux, "状态跃迁");
                Ok(self.state)
            }
            None => {
                self.audit_push(
                    AuditKind::StateChangeRejected,
                    0,
                    OutSecError::InvalidState.code(),
                    0,
                    "非法状态跃迁被拒",
                );
                Err(OutSecError::InvalidState)
            }
        }
    }

    // ---------- 审计内部件 ----------

    fn audit_push(&mut self, kind: AuditKind, actor: u32, code: u32, aux: u32, note: &str) {
        self.audit.push(AuditEvent {
            seq: 0,
            tick: self.tick,
            kind,
            actor,
            code,
            aux,
            note: String::from(note),
        });
    }

    /// 审计快照（自旧到新克隆）。
    pub fn audit_events(&self) -> Vec<AuditEvent> {
        self.audit.iter_oldest_first()
    }

    /// 指定种类事件计数。
    pub fn audit_count_kind(&self, kind: AuditKind) -> u32 {
        self.audit.count_kind(kind)
    }

    /// 审计摘要。
    pub fn audit_digest(&self) -> u64 {
        self.audit.digest()
    }

    /// 审计回放校验：环内序号必须严格递增；状态跃迁链必须能从
    /// Offline 按跃迁表逐步合法重演到回放终态。
    pub fn audit_verify_replay(&self) -> Result<ReplayReport, OutSecError> {
        let events = self.audit.iter_oldest_first();
        let mut last_seq: u64 = 0;
        let mut replay_state = OutSecState::Offline;
        let mut state_events = 0u32;
        let mut i = 0usize;
        while i < events.len() {
            let e = &events[i];
            if e.seq <= last_seq {
                return Err(OutSecError::InvalidState);
            }
            last_seq = e.seq;
            if e.kind == AuditKind::StateChanged {
                let from = OutSecState::from_tag(((e.aux >> 8) & 0xFF) as u8);
                let to = OutSecState::from_tag((e.aux & 0xFF) as u8);
                if from != replay_state || !OutSecState::transition_legal(from, to) {
                    return Err(OutSecError::InvalidState);
                }
                replay_state = to;
                state_events = state_events.saturating_add(1);
            }
            i += 1;
        }
        Ok(ReplayReport {
            events: events.len() as u32,
            state_events,
            replay_state,
            digest: self.audit.digest(),
        })
    }

    // ---------- 写者注册与档位 ----------

    /// 注册写者（配置类操作：Init/Armed 态可用）。
    pub fn register_writer(
        &mut self,
        name: &str,
        tier: SafetyTier,
        allow_mask: u8,
    ) -> Result<u32, OutSecError> {
        if !self.state.provision_ok() {
            return Err(OutSecError::NotInitialized);
        }
        let id = self.writers.register(name, tier, allow_mask, self.tick)?;
        self.audit_push(
            AuditKind::WriterRegistered,
            id,
            0,
            tier.tag() as u32,
            "写者注册",
        );
        Ok(id)
    }

    /// 注销写者（安全档专属；记录保留，ID 溯源链不断）。
    pub fn remove_writer(&mut self, actor: u32, target: u32) -> Result<(), OutSecError> {
        if !self.state.provision_ok() {
            return Err(OutSecError::NotInitialized);
        }
        let ai = match self.writers.index_of(actor) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        if self.writers.writers[ai].tier != SafetyTier::Security {
            self.audit_push(
                AuditKind::WriterDenied,
                actor,
                OutSecError::PermissionDenied.code(),
                0,
                "非安全档尝试注销写者",
            );
            return Err(OutSecError::PermissionDenied);
        }
        let ti = match self.writers.index_of(target) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        self.writers.writers[ti].active = false;
        self.audit_push(AuditKind::WriterRemoved, actor, 0, target, "写者注销");
        Ok(())
    }

    /// 写者快照（外部对账用）。
    pub fn writer_snapshot(&self, id: u32) -> Option<WriterRecord> {
        self.writers.snapshot(id)
    }

    /// 档位变更：降权即时生效；升权显性走审批路径
    /// （返回 [`OutSecError::ApprovalRequired`] 并在诊断中指路）。
    pub fn set_tier(
        &mut self,
        actor: u32,
        target: u32,
        to: SafetyTier,
    ) -> Result<SafetyTier, OutSecError> {
        if !self.state.provision_ok() {
            return Err(OutSecError::NotInitialized);
        }
        let ai = match self.writers.index_of(actor) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        if self.writers.writers[ai].tier != SafetyTier::Security {
            self.audit_push(
                AuditKind::WriterDenied,
                actor,
                OutSecError::PermissionDenied.code(),
                0,
                "非安全档尝试变更档位",
            );
            return Err(OutSecError::PermissionDenied);
        }
        let ti = match self.writers.index_of(target) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        let from = self.writers.writers[ti].tier;
        if to == from {
            return Err(OutSecError::InvalidState);
        }
        if to.rank() < from.rank() {
            // 降权：即时生效，入账。
            self.writers.writers[ti].tier = to;
            self.audit_push(
                AuditKind::TierDemoted,
                actor,
                0,
                ((from.tag() as u32) << 8) | to.tag() as u32,
                "档位降权即时生效",
            );
            Ok(to)
        } else {
            // 升权：绝不直改，指路审批。
            self.audit_push(
                AuditKind::WriterDenied,
                actor,
                OutSecError::ApprovalRequired.code(),
                0,
                "升权尝试被拒：必须走审批路径",
            );
            Err(OutSecError::ApprovalRequired)
        }
    }

    /// 立升权请求（申请人自请；同主体有待决请求时显性失败）。
    pub fn request_promotion(
        &mut self,
        target: u32,
        to: SafetyTier,
    ) -> Result<u32, OutSecError> {
        if !self.state.content_ok() {
            return Err(OutSecError::NotInitialized);
        }
        let ti = match self.writers.index_of(target) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        if !self.writers.writers[ti].active {
            return Err(OutSecError::UnknownWriter);
        }
        let from = self.writers.writers[ti].tier;
        if to.rank() <= from.rank() {
            return Err(OutSecError::InvalidState);
        }
        let req_id = self.approvals.open_request(target, from, to, self.tick)?;
        self.audit_push(
            AuditKind::PromotionRequested,
            target,
            0,
            req_id,
            "升权请求建立（待安全档审批）",
        );
        Ok(req_id)
    }

    /// 审批升权：审批人必须是安全档，且不得审批自己的请求（职责分离）。
    pub fn approve_promotion(
        &mut self,
        req_id: u32,
        approver: u32,
    ) -> Result<SafetyTier, OutSecError> {
        if !self.state.content_ok() {
            return Err(OutSecError::NotInitialized);
        }
        let ai = match self.writers.index_of(approver) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        if self.writers.writers[ai].tier != SafetyTier::Security {
            self.audit_push(
                AuditKind::PromotionRejected,
                approver,
                OutSecError::PermissionDenied.code(),
                req_id,
                "非安全档尝试审批升权",
            );
            return Err(OutSecError::PermissionDenied);
        }
        let req = match self.approvals.open_request_of(req_id) {
            Some(r) => r,
            None => return Err(OutSecError::InvalidState),
        };
        if req.target == approver {
            self.audit_push(
                AuditKind::PromotionRejected,
                approver,
                OutSecError::PermissionDenied.code(),
                req_id,
                "职责分离违规：审批人不得审批自己的升权请求",
            );
            return Err(OutSecError::PermissionDenied);
        }
        let ti = match self.writers.index_of(req.target) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        self.writers.writers[ti].tier = req.to;
        self.approvals.close(req_id);
        self.approvals.approved = self.approvals.approved.saturating_add(1);
        self.audit_push(
            AuditKind::PromotionApproved,
            approver,
            0,
            req_id,
            "升权审批通过",
        );
        Ok(req.to)
    }

    /// 驳回升权请求（同样留痕）。
    pub fn reject_promotion(&mut self, req_id: u32, approver: u32) -> Result<(), OutSecError> {
        if !self.state.content_ok() {
            return Err(OutSecError::NotInitialized);
        }
        let ai = match self.writers.index_of(approver) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        if self.writers.writers[ai].tier != SafetyTier::Security {
            return Err(OutSecError::PermissionDenied);
        }
        match self.approvals.open_request_of(req_id) {
            Some(_) => {
                self.approvals.close(req_id);
                self.approvals.rejected = self.approvals.rejected.saturating_add(1);
                self.audit_push(
                    AuditKind::PromotionRejected,
                    approver,
                    0,
                    req_id,
                    "升权请求驳回",
                );
                Ok(())
            }
            None => Err(OutSecError::InvalidState),
        }
    }

    /// 待决请求查询。
    pub fn open_promotion(&self, req_id: u32) -> Option<PromotionRequest> {
        self.approvals.open_request_of(req_id)
    }

    // ---------- 写路径：权限裁决 → 黑屏守卫 → 落盘 → 完整性基线 ----------

    /// 写请求拒绝内务：写者账计数 + 总账计数 + 审计入账。
    fn deny_write(&mut self, writer: u32, wi: Option<usize>, err: OutSecError, note: &str) {
        if let Some(i) = wi {
            self.writers.writers[i].denied = self.writers.writers[i].denied.saturating_add(1);
        }
        self.writes_denied = self.writes_denied.saturating_add(1);
        self.audit_push(AuditKind::WriterDenied, writer, err.code(), 0, note);
    }

    /// 提交一次区域写（显示输出主路径）。
    ///
    /// 五道裁决依序执行，任何一道拒绝即短路并全量入账：
    /// 1. 状态门（Armed 才可写）；2. 身份门（注册制）；3. 授权门
    /// （区域位图）；4. 档位门（权限矩阵）；5. 安全覆盖门（相交即拒）。
    /// 通过后进入黑屏守卫，再落盘、记完整性基线、入提交账，并按
    /// [`INTEGRITY_SAMPLE_INTERVAL`] 步距强制回读抽查。
    pub fn submit_write(
        &mut self,
        writer: u32,
        region: usize,
        payload: &[u8],
    ) -> Result<WriteReceipt, OutSecError> {
        if !self.state.content_ok() {
            return Err(OutSecError::NotInitialized);
        }
        // 身份门。
        let wi = match self.writers.index_of(writer) {
            Some(i) => i,
            None => {
                self.deny_write(writer, None, OutSecError::UnknownWriter, "未注册写者尝试写入");
                return Err(OutSecError::UnknownWriter);
            }
        };
        if !self.writers.writers[wi].active {
            self.deny_write(writer, Some(wi), OutSecError::UnknownWriter, "已注销写者尝试写入");
            return Err(OutSecError::UnknownWriter);
        }
        if self.writers.writers[wi].blacklisted {
            self.deny_write(writer, Some(wi), OutSecError::WriteDenied, "已拉黑写者尝试写入");
            return Err(OutSecError::WriteDenied);
        }
        // 授权门。
        if !self.writers.writers[wi].region_allowed(region) {
            self.deny_write(
                writer,
                Some(wi),
                OutSecError::WriteDenied,
                "区域未授权：allow_mask 不含目标区域",
            );
            return Err(OutSecError::WriteDenied);
        }
        // 档位门（权限矩阵）。
        let class = REGIONS[region].class;
        if !matrix_allows(self.writers.writers[wi].tier, class) {
            self.deny_write(
                writer,
                Some(wi),
                OutSecError::WriteDenied,
                "档位不满足区域安全级（权限矩阵拒绝）",
            );
            return Err(OutSecError::WriteDenied);
        }
        // 安全覆盖门：目标区域与活动安全覆盖层相交即拒（普通内容不可覆盖安全层）。
        let region_rect = REGIONS[region].rect;
        if self.writers.writers[wi].tier != SafetyTier::Security && self.overlays.hits_security(&region_rect)
        {
            self.deny_write(
                writer,
                Some(wi),
                OutSecError::SecureLayerViolation,
                "写矩形与活动安全覆盖层相交且档位低于安全档",
            );
            return Err(OutSecError::SecureLayerViolation);
        }
        // 载荷尺寸门。
        let expect_len = match region_pixel_count(region) {
            Some(n) => n,
            None => {
                self.deny_write(writer, Some(wi), OutSecError::WriteDenied, "区域号非法");
                return Err(OutSecError::WriteDenied);
            }
        };
        if payload.len() != expect_len {
            self.deny_write(
                writer,
                Some(wi),
                OutSecError::PayloadSizeMismatch,
                "载荷字节数与区域像素数不符",
            );
            return Err(OutSecError::PayloadSizeMismatch);
        }
        // 黑屏守卫：整帧 + 有效全黑 → 连续计数；达阈值拉黑并拦截。
        let is_full_frame = region == 0;
        if is_full_frame {
            let luma = luma_average_x255(payload);
            if luma < BLACKSCREEN_LUMA_MAX {
                self.writers.writers[wi].dark_streak =
                    self.writers.writers[wi].dark_streak.saturating_add(1);
                if self.writers.writers[wi].dark_streak >= BLACKSCREEN_DARK_LIMIT {
                    self.writers.writers[wi].blacklisted = true;
                    self.audit_push(
                        AuditKind::BlackscreenFlagged,
                        writer,
                        OutSecError::BlackscreenBlocked.code(),
                        self.writers.writers[wi].dark_streak as u32,
                        "黑屏攻击定性：连续有效全黑整帧达阈值，写者拉黑",
                    );
                    self.deny_write(
                        writer,
                        Some(wi),
                        OutSecError::BlackscreenBlocked,
                        "黑屏攻击拦截：写者已拉黑",
                    );
                    return Err(OutSecError::BlackscreenBlocked);
                }
            } else {
                self.writers.writers[wi].dark_streak = 0;
            }
        }
        // 落盘。
        let (off, len) = match region_span(region) {
            Some(s) => s,
            None => {
                self.deny_write(writer, Some(wi), OutSecError::WriteDenied, "区域表越界");
                return Err(OutSecError::WriteDenied);
            }
        };
        self.fb[off..off + len].copy_from_slice(payload);
        let checksum = fnv1a64(&self.fb[off..off + len]);
        self.region_expected[region] = checksum;
        self.region_dirty[region] = true;
        // 整帧写（区域 0）合法覆盖全部子区域字节：子区域完整性基线必须
        // 随之刷新（基线永远描述"最后一次合法输出后的真实帧面"），否则
        // 篡改扫描会把合法整帧写误判为子区域篡改。
        if region == 0 {
            let mut r = 1usize;
            while r < REGION_COUNT {
                if let Some((soff, slen)) = region_span(r) {
                    self.region_expected[r] = fnv1a64(&self.fb[soff..soff + slen]);
                    self.region_dirty[r] = true;
                }
                r += 1;
            }
        }
        let seq = self.commits.push(WriteCommit {
            seq: 0,
            writer,
            region,
            checksum,
            len: len as u32,
            tick: self.tick,
        });
        self.writers.writers[wi].committed = self.writers.writers[wi].committed.saturating_add(1);
        self.writes_committed = self.writes_committed.saturating_add(1);
        self.audit_push(
            AuditKind::WriteCommitted,
            writer,
            0,
            region as u32,
            "写提交成功",
        );
        // 抽查：每 INTEGRITY_SAMPLE_INTERVAL 次提交强制回读全部脏区。
        if self.writes_committed % INTEGRITY_SAMPLE_INTERVAL == 0 {
            self.integrity_samples = self.integrity_samples.saturating_add(1);
            let mut r = 0usize;
            while r < REGION_COUNT {
                if self.region_dirty[r] && self.verify_region_pure(r) == IntegrityVerdict::Tampered {
                    self.raise_tamper_alert(r, "写后抽查回读不符");
                    return Err(OutSecError::TamperDetected);
                }
                r += 1;
            }
        }
        Ok(WriteReceipt {
            seq,
            region,
            len: len as u32,
            checksum,
            tick: self.tick,
        })
    }

    /// 纯读回读：重算区域校验和与期望比对（不改账、不改状态）。
    pub fn verify_region_pure(&self, region: usize) -> IntegrityVerdict {
        match region_span(region) {
            Some((off, len)) => {
                let actual = fnv1a64(&self.fb[off..off + len]);
                if actual == self.region_expected[region] {
                    IntegrityVerdict::Intact
                } else {
                    IntegrityVerdict::Tampered
                }
            }
            None => IntegrityVerdict::Tampered,
        }
    }

    /// 显式回读指定区域（返回重算校验和；不符即篡改错误）。
    pub fn verify_region(&self, region: usize) -> Result<u64, OutSecError> {
        match region_span(region) {
            Some((off, len)) => {
                let actual = fnv1a64(&self.fb[off..off + len]);
                if actual != self.region_expected[region] {
                    Err(OutSecError::TamperDetected)
                } else {
                    Ok(actual)
                }
            }
            None => Err(OutSecError::InvalidState),
        }
    }

    /// 篡改告警内务：告警计数 + 审计入账 + 状态机拉响（Armed → Alert）。
    fn raise_tamper_alert(&mut self, region: usize, note: &str) {
        self.tamper_alerts = self.tamper_alerts.saturating_add(1);
        self.audit_push(
            AuditKind::TamperAlert,
            0,
            OutSecError::TamperDetected.code(),
            region as u32,
            note,
        );
        if self.state == OutSecState::Armed {
            let _ = self.request_state(StateEvent::AlertRaised);
        }
    }

    /// 注入面：裸写一个字节，绕过权限账与完整性基线（建模越权 DMA 写）。
    ///
    /// 仅安全档写者可调用（注入演练也是受控操作），全程入账
    /// [`AuditKind::InjectionWrite`]。注入后区域校验和必然失配——
    /// 这是篡改检测器与抽查路径的真实靶标。
    pub fn inject_raw_write(
        &mut self,
        actor: u32,
        region: usize,
        offset_in_region: usize,
        value: u8,
    ) -> Result<(), OutSecError> {
        if !self.state.content_ok() {
            return Err(OutSecError::NotInitialized);
        }
        let ai = match self.writers.index_of(actor) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        if self.writers.writers[ai].tier != SafetyTier::Security {
            self.audit_push(
                AuditKind::WriterDenied,
                actor,
                OutSecError::PermissionDenied.code(),
                0,
                "非安全档尝试使用注入面",
            );
            return Err(OutSecError::PermissionDenied);
        }
        let (off, len) = match region_span(region) {
            Some(s) => s,
            None => return Err(OutSecError::InvalidState),
        };
        if offset_in_region >= len {
            return Err(OutSecError::InvalidState);
        }
        let idx = off.saturating_add(offset_in_region);
        if idx >= self.fb.len() {
            return Err(OutSecError::InvalidState);
        }
        self.fb[idx] = value;
        self.audit_push(
            AuditKind::InjectionWrite,
            actor,
            0,
            ((region as u32) << 16) | offset_in_region as u32,
            "注入面裸写（越权 DMA 建模，已留痕）",
        );
        Ok(())
    }

    // ---------- 安全覆盖层栈操作 ----------

    /// 入栈覆盖层。
    ///
    /// 规则：安全层需安全档属主；栈序要求优先级严格递增；普通内容层
    /// 与活动安全层相交且属主低于安全档即拒（安全层不可覆盖）。
    /// 返回分配的覆盖层 ID。
    pub fn push_overlay(
        &mut self,
        actor: u32,
        kind: OverlayKind,
        rect: Rect,
        priority: u16,
    ) -> Result<u32, OutSecError> {
        if !self.state.content_ok() {
            return Err(OutSecError::NotInitialized);
        }
        let ai = match self.writers.index_of(actor) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        let tier = self.writers.writers[ai].tier;
        let deny_push = |core: &mut OutSecCore, err: OutSecError, note: &str| {
            core.audit_push(
                AuditKind::OverlayPushDenied,
                actor,
                err.code(),
                0,
                note,
            );
        };
        if !rect.within_fb() {
            deny_push(self, OutSecError::InvalidState, "覆盖矩形越出帧缓冲界");
            return Err(OutSecError::InvalidState);
        }
        if self.overlays.stack.len() >= OVERLAY_STACK_MAX {
            deny_push(self, OutSecError::OverlayStackFull, "覆盖栈已满");
            return Err(OutSecError::OverlayStackFull);
        }
        if let Some(top) = self.overlays.top() {
            if priority <= top.priority {
                deny_push(
                    self,
                    OutSecError::InvalidState,
                    "栈序违规：优先级必须严格大于栈顶",
                );
                return Err(OutSecError::InvalidState);
            }
        }
        if kind.is_security() && tier != SafetyTier::Security {
            deny_push(
                self,
                OutSecError::PermissionDenied,
                "安全覆盖层仅安全档可入栈",
            );
            return Err(OutSecError::PermissionDenied);
        }
        if !kind.is_security() && tier != SafetyTier::Security && self.overlays.hits_security(&rect)
        {
            deny_push(
                self,
                OutSecError::SecureLayerViolation,
                "普通内容层与活动安全层相交且档位低于安全档",
            );
            return Err(OutSecError::SecureLayerViolation);
        }
        let id = self.overlays.next_id;
        self.overlays.next_id = self.overlays.next_id.saturating_add(1);
        self.overlays.stack.push(Overlay {
            id,
            kind,
            rect,
            priority,
            owner: actor,
            active: true,
        });
        self.audit_push(AuditKind::OverlayPushed, actor, 0, id, "覆盖层入栈");
        Ok(id)
    }

    /// 出栈：只允许弹栈顶（指定 ID 非栈顶即错）。
    ///
    /// 弹出权限：属主本人或安全档；安全层弹出无论属主一律要安全档。
    pub fn pop_overlay(&mut self, actor: u32, overlay_id: u32) -> Result<Overlay, OutSecError> {
        if !self.state.content_ok() {
            return Err(OutSecError::NotInitialized);
        }
        let ai = match self.writers.index_of(actor) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        let tier = self.writers.writers[ai].tier;
        let top = match self.overlays.top() {
            Some(t) => t,
            None => return Err(OutSecError::OverlayNotFound),
        };
        if top.id != overlay_id {
            return Err(OutSecError::OverlayNotTop);
        }
        let is_owner = top.owner == actor;
        let may_pop = tier == SafetyTier::Security || (is_owner && !top.kind.is_security());
        if !may_pop {
            self.audit_push(
                AuditKind::OverlayPushDenied,
                actor,
                OutSecError::PermissionDenied.code(),
                overlay_id,
                "覆盖层弹出权限不足",
            );
            return Err(OutSecError::PermissionDenied);
        }
        let popped = self.overlays.stack.remove(self.overlays.stack.len() - 1);
        self.audit_push(AuditKind::OverlayPopped, actor, 0, overlay_id, "覆盖层出栈");
        Ok(popped)
    }

    /// 栈顶快照。
    pub fn overlay_top(&self) -> Option<Overlay> {
        self.overlays.top()
    }

    /// 栈深。
    pub fn overlay_depth(&self) -> usize {
        self.overlays.len()
    }

    // ---------- 捕获控制与 HDCP 协商 ----------

    /// 请求一次屏幕捕获。
    ///
    /// 裁决序：全局策略 → 写者档位（复用写者账，判据"捕获复用"）→
    /// 安全层活动时低档捕获得挖黑版。全程入账。
    pub fn request_capture(&mut self, writer: u32) -> Result<CaptureOutcome, OutSecError> {
        if !self.state.content_ok() {
            return Err(OutSecError::NotInitialized);
        }
        self.capture.attempts = self.capture.attempts.saturating_add(1);
        self.audit_push(AuditKind::CaptureRequested, writer, 0, 0, "屏幕捕获请求");
        let wi = match self.writers.index_of(writer) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        let tier = self.writers.writers[wi].tier;
        if self.capture.policy == CapturePolicy::DenyAll || tier == SafetyTier::Normal {
            self.capture.denied = self.capture.denied.saturating_add(1);
            self.audit_push(
                AuditKind::CaptureDenied,
                writer,
                OutSecError::CaptureDenied.code(),
                0,
                "捕获被拒（全局禁捕或普通档）",
            );
            return Ok(CaptureOutcome::Denied);
        }
        let secure_active = !self.overlays.active_security_rects().is_empty();
        if secure_active && tier != SafetyTier::Security {
            self.capture.blanked = self.capture.blanked.saturating_add(1);
            self.audit_push(
                AuditKind::CaptureGrantedProxy,
                writer,
                0,
                0,
                "捕获挖黑放行（安全层活动，安全区挖黑）",
            );
            return Ok(CaptureOutcome::Blanked);
        }
        self.capture.granted = self.capture.granted.saturating_add(1);
        Ok(CaptureOutcome::Allowed)
    }

    /// 设置全局捕获策略（安全档专属）。
    pub fn set_capture_policy(&mut self, actor: u32, policy: CapturePolicy) -> Result<(), OutSecError> {
        let ai = match self.writers.index_of(actor) {
            Some(i) => i,
            None => return Err(OutSecError::UnknownWriter),
        };
        if self.writers.writers[ai].tier != SafetyTier::Security {
            return Err(OutSecError::PermissionDenied);
        }
        self.capture.policy = policy;
        Ok(())
    }

    /// 捕获账快照。
    pub fn capture_snapshot(&self) -> CaptureControl {
        CaptureControl {
            policy: self.capture.policy,
            attempts: self.capture.attempts,
            granted: self.capture.granted,
            blanked: self.capture.blanked,
            denied: self.capture.denied,
        }
    }

    /// HDCP 协商（复用诚实能力表；成败均入账）。
    pub fn hdcp_negotiate(
        &mut self,
        connector: usize,
        requested: HdcpLevel,
    ) -> Result<HdcpLevel, OutSecError> {
        match self.hdcp.negotiate(connector, requested) {
            Ok(level) => {
                self.audit_push(
                    AuditKind::HdcpNegotiated,
                    0,
                    0,
                    ((connector as u32) << 8) | requested.rank() as u32,
                    "HDCP 协商成功",
                );
                Ok(level)
            }
            Err(e) => {
                self.audit_push(
                    AuditKind::HdcpRefused,
                    0,
                    e.code(),
                    connector as u32,
                    "HDCP 协商被拒：诚实能力不足，绝不静默降级",
                );
                Err(e)
            }
        }
    }

    /// HDCP 诚实报表。
    pub fn hdcp_report(&self) -> String {
        self.hdcp.honest_report()
    }

    // ---------- 篡改检测器 ----------

    /// 周期篡改扫描：三路一致性对账。
    ///
    /// 1. 区域校验和回读（脏区逐一核对期望基线）；
    /// 2. 覆盖栈对账（栈内优先级严格递增、ID 唯一、全部活动）；
    /// 3. 写者账对账（账容不超限；提交账无淘汰时逐写者计数一致）。
    /// 任一路不一致 → 告警入账 + 状态机拉响；全绿且处于告警态 → 洗白回武装。
    pub fn run_tamper_scan(&mut self) -> Result<TamperScanReport, OutSecError> {
        if !self.state.scan_ok() {
            return Err(OutSecError::NotInitialized);
        }
        self.scans = self.scans.saturating_add(1);
        let mut mismatched: Vec<usize> = Vec::new();
        let mut regions_checked = 0usize;
        let mut r = 0usize;
        while r < REGION_COUNT {
            if self.region_dirty[r] {
                regions_checked = regions_checked.saturating_add(1);
                if self.verify_region_pure(r) == IntegrityVerdict::Tampered {
                    mismatched.push(r);
                } else {
                    self.region_dirty[r] = false;
                }
            }
            r += 1;
        }
        // 覆盖栈对账。
        let overlays_checked = self.overlays.stack.len();
        let mut overlay_ok = true;
        let mut i = 0usize;
        while i < self.overlays.stack.len() {
            let o = &self.overlays.stack[i];
            if !o.active {
                overlay_ok = false;
            }
            let mut j = i + 1;
            while j < self.overlays.stack.len() {
                if self.overlays.stack[j].id == o.id {
                    overlay_ok = false;
                }
                if j > i && self.overlays.stack[j].priority <= self.overlays.stack[i].priority {
                    overlay_ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        // 写者账对账。
        let writers_checked = self.writers.total_count();
        let mut writer_ok = true;
        if writers_checked > MAX_WRITERS {
            writer_ok = false;
        }
        if self.commits.evicted == 0 {
            let mut w = 0usize;
            while w < self.writers.writers.len() {
                let rec = &self.writers.writers[w];
                if rec.active
                    && rec.committed as usize > self.commits.count_for_writer(rec.id) as usize
                {
                    writer_ok = false;
                }
                w += 1;
            }
        }
        let consistent = mismatched.is_empty() && overlay_ok && writer_ok;
        let mut alerts_added = 0u32;
        if !consistent {
            let mut m = 0usize;
            while m < mismatched.len() {
                let reg = mismatched[m];
                self.raise_tamper_alert(reg, "篡改扫描：区域校验和回读不符");
                alerts_added = alerts_added.saturating_add(1);
                m += 1;
            }
            if !overlay_ok {
                self.tamper_alerts = self.tamper_alerts.saturating_add(1);
                alerts_added = alerts_added.saturating_add(1);
                self.audit_push(
                    AuditKind::TamperAlert,
                    0,
                    OutSecError::TamperDetected.code(),
                    0,
                    "篡改扫描：覆盖栈序/注册一致性破坏",
                );
            }
            if !writer_ok {
                self.tamper_alerts = self.tamper_alerts.saturating_add(1);
                alerts_added = alerts_added.saturating_add(1);
                self.audit_push(
                    AuditKind::TamperAlert,
                    0,
                    OutSecError::TamperDetected.code(),
                    0,
                    "篡改扫描：写者账与提交账不一致",
                );
            }
        } else if self.state == OutSecState::Alert {
            let _ = self.request_state(StateEvent::ScanClean);
        }
        let state_after = self.state;
        self.audit_push(
            AuditKind::ScanCompleted,
            0,
            0,
            if consistent { 1 } else { 0 },
            "篡改扫描完成",
        );
        Ok(TamperScanReport {
            consistent,
            regions_checked,
            mismatched_regions: mismatched,
            writers_checked,
            overlays_checked,
            alerts_added,
            state_after,
        })
    }

    /// 累计扫描次数。
    pub const fn scan_count(&self) -> u32 {
        self.scans
    }

    /// 累计篡改告警数。
    pub const fn tamper_alert_count(&self) -> u32 {
        self.tamper_alerts
    }

    /// 累计完整性抽查次数。
    pub const fn integrity_sample_count(&self) -> u32 {
        self.integrity_samples
    }

    /// 审计显性溢出计数。
    pub const fn audit_overflow_count(&self) -> u64 {
        self.audit.overflow
    }

    /// 指定写者在提交账中的在环提交数。
    pub fn commit_count_for(&self, writer: u32) -> u32 {
        self.commits.count_for_writer(writer)
    }

    // ---------- 体检 / 指纹 / 双跑 ----------

    /// 体检：三档结论 + 人读附注。
    ///
    /// 红：有篡改告警或已锁定；黄：有拒绝记录/审计溢出/处于告警态；
    /// 绿：其余。
    pub fn health(&self) -> HealthReport {
        let mut notes: Vec<String> = Vec::new();
        let mut verdict = HealthVerdict::Green;
        if self.tamper_alerts > 0 {
            verdict = HealthVerdict::Red;
            notes.push(format!("存在 {} 次篡改告警，输出路径完整性存疑", self.tamper_alerts));
        }
        if self.state == OutSecState::LockedDown {
            verdict = HealthVerdict::Red;
            notes.push(String::from("输出路径已安全锁定，等待复位"));
        }
        if verdict == HealthVerdict::Green && self.audit.overflow > 0 {
            verdict = HealthVerdict::Amber;
            notes.push(format!("审计环形账已溢出 {} 条旧事件（显性计数）", self.audit.overflow));
        }
        if verdict == HealthVerdict::Green && self.writes_denied > 0 {
            verdict = HealthVerdict::Amber;
            notes.push(format!("存在 {} 次被拒写，请核查权限账", self.writes_denied));
        }
        if self.state == OutSecState::Alert && verdict == HealthVerdict::Green {
            verdict = HealthVerdict::Amber;
            notes.push(String::from("处于告警态，等待篡改扫描洗白"));
        }
        if self.state == OutSecState::Offline && verdict == HealthVerdict::Green {
            verdict = HealthVerdict::Amber;
            notes.push(String::from("未武装：状态机仍在 Offline，输出路径尚未受管"));
        }
        if notes.is_empty() {
            notes.push(String::from("输出路径五道闸门全绿"));
        }
        HealthReport {
            state: self.state,
            verdict,
            writers_active: self.writers.active_count(),
            overlays_active: self.overlays.len(),
            audit_len: self.audit.filled,
            audit_overflow: self.audit.overflow,
            tamper_alerts: self.tamper_alerts,
            integrity_samples: self.integrity_samples,
            writes_committed: self.writes_committed,
            writes_denied: self.writes_denied,
            notes,
        }
    }

    /// 全状态确定性指纹：所有可变状态按定序定宽编码后做 FNV-1a 64。
    ///
    /// 双跑对拍的比较基准；编码顺序即本函数书写顺序，永不重排。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(4096);
        b.push(self.state.tag());
        b.extend_from_slice(&self.tick.to_be_bytes());
        let mut i = 0usize;
        while i < self.writers.writers.len() {
            let w = &self.writers.writers[i];
            b.extend_from_slice(&w.id.to_be_bytes());
            b.push(w.tier.tag());
            b.push(w.allow_mask);
            b.push(w.active as u8);
            b.push(w.blacklisted as u8);
            b.extend_from_slice(&w.committed.to_be_bytes());
            b.extend_from_slice(&w.denied.to_be_bytes());
            b.push(w.dark_streak);
            b.extend_from_slice(&w.registered_tick.to_be_bytes());
            b.extend_from_slice(w.name.as_bytes());
            b.push(0xFF);
            i += 1;
        }
        b.push(0xFE);
        let mut i = 0usize;
        while i < self.overlays.stack.len() {
            let o = &self.overlays.stack[i];
            b.extend_from_slice(&o.id.to_be_bytes());
            b.push(o.kind.tag());
            b.extend_from_slice(&o.rect.x.to_be_bytes());
            b.extend_from_slice(&o.rect.y.to_be_bytes());
            b.extend_from_slice(&o.rect.w.to_be_bytes());
            b.extend_from_slice(&o.rect.h.to_be_bytes());
            b.extend_from_slice(&o.priority.to_be_bytes());
            b.extend_from_slice(&o.owner.to_be_bytes());
            b.push(o.active as u8);
            i += 1;
        }
        b.push(0xFE);
        let commits = self.commits.iter_oldest_first();
        let mut i = 0usize;
        while i < commits.len() {
            let c = &commits[i];
            b.extend_from_slice(&c.seq.to_be_bytes());
            b.extend_from_slice(&c.writer.to_be_bytes());
            b.push(c.region as u8);
            b.extend_from_slice(&c.checksum.to_be_bytes());
            b.extend_from_slice(&c.len.to_be_bytes());
            b.extend_from_slice(&c.tick.to_be_bytes());
            i += 1;
        }
        b.push(0xFE);
        let events = self.audit.iter_oldest_first();
        let mut i = 0usize;
        while i < events.len() {
            let e = &events[i];
            b.extend_from_slice(&e.seq.to_be_bytes());
            b.extend_from_slice(&e.tick.to_be_bytes());
            b.push(e.kind.tag());
            b.extend_from_slice(&e.actor.to_be_bytes());
            b.extend_from_slice(&e.code.to_be_bytes());
            b.extend_from_slice(&e.aux.to_be_bytes());
            b.extend_from_slice(e.note.as_bytes());
            b.push(0xFF);
            i += 1;
        }
        b.push(0xFE);
        b.extend_from_slice(&self.audit.overflow.to_be_bytes());
        b.extend_from_slice(&self.commits.evicted.to_be_bytes());
        b.extend_from_slice(&self.tamper_alerts.to_be_bytes());
        b.extend_from_slice(&self.scans.to_be_bytes());
        b.extend_from_slice(&self.writes_committed.to_be_bytes());
        b.extend_from_slice(&self.writes_denied.to_be_bytes());
        b.extend_from_slice(&self.integrity_samples.to_be_bytes());
        b.extend_from_slice(&self.capture.attempts.to_be_bytes());
        b.extend_from_slice(&self.capture.granted.to_be_bytes());
        b.extend_from_slice(&self.capture.blanked.to_be_bytes());
        b.extend_from_slice(&self.capture.denied.to_be_bytes());
        b.extend_from_slice(&self.approvals.approved.to_be_bytes());
        b.extend_from_slice(&self.approvals.rejected.to_be_bytes());
        let mut i = 0usize;
        while i < REGION_COUNT {
            b.extend_from_slice(&self.region_expected[i].to_be_bytes());
            b.push(self.region_dirty[i] as u8);
            i += 1;
        }
        fnv1a64(&b)
    }

}

// ===================================================================
// 工具函数：亮度均值 / 确定性脚本载荷
// ===================================================================

/// 载荷平均亮度（0~255 标尺，saturating 累加；空载荷返回 0）。
///
/// 黑屏守卫的判定量：整帧平均亮度低于 [`BLACKSCREEN_LUMA_MAX`] 记暗帧。
pub fn luma_average_x255(payload: &[u8]) -> u16 {
    if payload.is_empty() {
        return 0;
    }
    let mut sum: u32 = 0;
    let mut i = 0usize;
    while i < payload.len() {
        sum = sum.saturating_add(payload[i] as u32);
        i += 1;
    }
    (sum / payload.len() as u32) as u16
}

/// 确定性脚本载荷生成器：64 位 LCG（显性 wrapping）逐字节填充。
///
/// 双跑对拍的载荷源——同一 seed 永远产出同一字节序列，无时钟、无
/// 环境熵参与。
pub fn script_payload(seed: u16, len: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E3779B97F4A7C15 ^ (seed as u64).wrapping_mul(0xD1B54A32D192ED03);
    let mut out: Vec<u8> = Vec::with_capacity(len);
    let mut i = 0usize;
    while i < len {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // 取高 33~40 位——LCG 低位的短周期模式不进入载荷。
        out.push((x >> 33) as u8);
        i += 1;
    }
    out
}

/// 生成"有效全黑"整帧载荷（黑屏守卫测试靶标）。
pub fn dark_full_payload() -> Vec<u8> {
    vec![0u8; region_pixel_count(0).unwrap_or(FB_BYTES)]
}

// ===================================================================
// 双跑确定性对拍：脚本 + 执行器 + 比对器
// ===================================================================

/// 双跑脚本操作（Copy 闭集；slot 映射写者 ID = slot+1，注册序即分配序）。
#[derive(Debug, Clone, Copy)]
pub enum ScriptOp {
    /// 状态跃迁。
    State(StateEvent),
    /// 注册写者。
    RegisterWriter { slot: u8, tier: SafetyTier, mask: u8 },
    /// 区域写（载荷由 seed 确定性生成）。
    Write { slot: u8, region: usize, seed: u16 },
    /// 整帧全黑写（黑屏守卫靶标）。
    DarkWrite { slot: u8 },
    /// 入栈覆盖层。
    PushOverlay { slot: u8, kind: OverlayKind, priority: u16 },
    /// 出栈覆盖层。
    PopOverlay { slot: u8, overlay_id: u32 },
    /// 降权（即时）。
    Demote { actor_slot: u8, target_slot: u8 },
    /// 立升权请求。
    RequestPromotion { slot: u8, to: SafetyTier },
    /// 审批升权。
    ApprovePromotion { actor_slot: u8, req_id: u32 },
    /// 篡改扫描。
    Scan,
}

/// 脚本写者名（确定性；slot→名 闭式映射）。
pub const fn script_writer_name(slot: u8) -> &'static str {
    match slot {
        0 => "sec-ctl",
        1 => "priv-app",
        2 => "norm-app",
        _ => "ext-app",
    }
}

/// 双跑确定性脚本：覆盖注册、四档写、越权写、安全层相交写、覆盖栈
/// 全序操作、降权/审批升权、扫描、锁定复位——每步两侧结果必然同构。
///
/// 预期拒绝步（两侧同样拒绝并入账）同样是确定性对拍的一部分：
/// 越权写注入（步 8/13）与覆盖越权注入（步 11/18）都在脚本里。
pub const SCRIPT: [ScriptOp; 26] = [
    ScriptOp::State(StateEvent::Init),
    ScriptOp::RegisterWriter { slot: 0, tier: SafetyTier::Security, mask: 0b1111 },
    ScriptOp::RegisterWriter { slot: 1, tier: SafetyTier::Privileged, mask: 0b1111 },
    ScriptOp::RegisterWriter { slot: 2, tier: SafetyTier::Normal, mask: 0b0100 },
    ScriptOp::State(StateEvent::Arm),
    ScriptOp::Write { slot: 1, region: 0, seed: 0x1111 },
    ScriptOp::Write { slot: 2, region: 2, seed: 0x2222 },
    // 越权写注入：普通档写整帧（特权级）→ 权限矩阵拒绝。
    ScriptOp::Write { slot: 2, region: 0, seed: 0x3333 },
    ScriptOp::PushOverlay { slot: 1, kind: OverlayKind::Content, priority: 1 },
    ScriptOp::PushOverlay { slot: 0, kind: OverlayKind::SecurityNotice, priority: 2 },
    // 覆盖越权注入：特权档整帧写与安全提示层相交 → SecureLayerViolation。
    ScriptOp::Write { slot: 1, region: 0, seed: 0x4444 },
    // 栈序注入：内容层优先级不高于栈顶 → 拒绝。
    ScriptOp::PushOverlay { slot: 1, kind: OverlayKind::Content, priority: 1 },
    // 栈顶弹出注入：弹非栈顶 ID → OverlayNotTop。
    ScriptOp::PopOverlay { slot: 1, overlay_id: 1 },
    ScriptOp::PopOverlay { slot: 0, overlay_id: 2 },
    ScriptOp::PopOverlay { slot: 0, overlay_id: 1 },
    ScriptOp::Write { slot: 0, region: 3, seed: 0x5555 },
    ScriptOp::Demote { actor_slot: 0, target_slot: 1 },
    // 降权后越权写注入：原特权档写整帧 → 权限矩阵拒绝。
    ScriptOp::Write { slot: 1, region: 0, seed: 0x6666 },
    ScriptOp::RequestPromotion { slot: 1, to: SafetyTier::Privileged },
    ScriptOp::ApprovePromotion { actor_slot: 0, req_id: 1 },
    ScriptOp::Write { slot: 1, region: 0, seed: 0x7777 },
    ScriptOp::DarkWrite { slot: 1 },
    ScriptOp::Scan,
    ScriptOp::Scan,
    ScriptOp::State(StateEvent::Lockdown),
    ScriptOp::State(StateEvent::Reset),
];

/// 执行单步脚本（拒绝步静默吞错——拒绝本身入审计账，两侧必然同构）。
pub fn apply_script_op(core: &mut OutSecCore, op: ScriptOp) {
    match op {
        ScriptOp::State(ev) => {
            let _ = core.request_state(ev);
        }
        ScriptOp::RegisterWriter { slot, tier, mask } => {
            let _ = core.register_writer(script_writer_name(slot), tier, mask);
        }
        ScriptOp::Write { slot, region, seed } => {
            if let Some(len) = region_pixel_count(region) {
                let payload = script_payload(seed, len);
                let _ = core.submit_write(slot as u32 + 1, region, &payload);
            }
        }
        ScriptOp::DarkWrite { slot } => {
            let payload = dark_full_payload();
            let _ = core.submit_write(slot as u32 + 1, 0, &payload);
        }
        ScriptOp::PushOverlay { slot, kind, priority } => {
            // 覆盖矩形确定性：奇数优先级铺安全面板条，偶数铺内容区。
            let rect = if kind.is_security() {
                REGIONS[3].rect
            } else {
                REGIONS[2].rect
            };
            let _ = core.push_overlay(slot as u32 + 1, kind, rect, priority);
        }
        ScriptOp::PopOverlay { slot, overlay_id } => {
            let _ = core.pop_overlay(slot as u32 + 1, overlay_id);
        }
        ScriptOp::Demote { actor_slot, target_slot } => {
            let _ = core.set_tier(
                actor_slot as u32 + 1,
                target_slot as u32 + 1,
                SafetyTier::Normal,
            );
        }
        ScriptOp::RequestPromotion { slot, to } => {
            let _ = core.request_promotion(slot as u32 + 1, to);
        }
        ScriptOp::ApprovePromotion { actor_slot, req_id } => {
            let _ = core.approve_promotion(req_id, actor_slot as u32 + 1);
        }
        ScriptOp::Scan => {
            let _ = core.run_tamper_scan();
        }
    }
}

/// 双跑确定性对拍：同一脚本在两个独立实例上逐步执行，每步比对
/// 全状态指纹；终态再比对审计摘要并各自做审计回放校验。
///
/// 分歧即确定性破缺——输出安全路径的任何非确定行为（未初始化读、
/// 指针序、哈希种子漂移）都会在这里现形。
pub fn run_dual_comparison() -> Result<DualRunReport, OutSecError> {
    let mut a = OutSecCore::new();
    let mut b = OutSecCore::new();
    let mut diverged: Option<usize> = None;
    let mut i = 0usize;
    while i < SCRIPT.len() {
        a.advance_tick(1);
        b.advance_tick(1);
        apply_script_op(&mut a, SCRIPT[i]);
        apply_script_op(&mut b, SCRIPT[i]);
        if diverged.is_none() && a.fingerprint() != b.fingerprint() {
            diverged = Some(i);
        }
        i += 1;
    }
    let fa = a.fingerprint();
    let fbp = b.fingerprint();
    let da = a.audit_digest();
    let db = b.audit_digest();
    // 双方审计回放都必须合法（回放不变量属于确定性契约的一部分）。
    let ra = a.audit_verify_replay()?;
    let rb = b.audit_verify_replay()?;
    Ok(DualRunReport {
        steps: SCRIPT.len(),
        matched: diverged.is_none() && fa == fbp && da == db && ra.digest == rb.digest,
        diverged_step: diverged,
        fingerprint_a: fa,
        fingerprint_b: fbp,
        audit_digest_a: da,
        audit_digest_b: db,
    })
}

// ===================================================================
// 测试：三组（HDCP/捕获/防护）+ 深度子系统冒烟
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 统一引导：初始化 → 注册三个标准写者 → 武装。
    /// 返回 (核心, 安全档ID, 特权档ID, 普通档ID)。
    fn boot() -> (OutSecCore, u32, u32, u32) {
        let mut c = OutSecCore::new();
        c.request_state(StateEvent::Init).unwrap();
        let sec = c.register_writer("sec-ctl", SafetyTier::Security, 0b1111).unwrap();
        let privw = c.register_writer("priv-app", SafetyTier::Privileged, 0b1111).unwrap();
        let norm = c.register_writer("norm-app", SafetyTier::Normal, 0b0100).unwrap();
        c.request_state(StateEvent::Arm).unwrap();
        (c, sec, privw, norm)
    }

    // ---------- 诊断段与基础组 ----------

    #[test]
    fn diag_codes_exactly_fill_segment() {
        // 16 个变体的码必须恰好占满 0x1360~0x136F 且互不重复。
        let all = [
            OutSecError::NotInitialized,
            OutSecError::InvalidState,
            OutSecError::UnknownWriter,
            OutSecError::DuplicateWriter,
            OutSecError::WriteDenied,
            OutSecError::PayloadSizeMismatch,
            OutSecError::SecureLayerViolation,
            OutSecError::BlackscreenBlocked,
            OutSecError::OverlayNotFound,
            OutSecError::OverlayStackFull,
            OutSecError::OverlayNotTop,
            OutSecError::PermissionDenied,
            OutSecError::ApprovalRequired,
            OutSecError::HdcpUnavailable,
            OutSecError::TamperDetected,
            OutSecError::CaptureDenied,
        ];
        assert_eq!(all.len(), 16);
        let mut i = 0usize;
        while i < all.len() {
            let code = all[i].code();
            assert!(code >= DIAG_BASE && code <= DIAG_LIMIT, "码越段: {:#x}", code);
            assert!(!all[i].detail().is_empty());
            let mut j = i + 1;
            while j < all.len() {
                assert_ne!(all[i].code(), all[j].code(), "诊断码重复");
                j += 1;
            }
            i += 1;
        }
        assert_eq!(OutSecError::NotInitialized.code(), DIAG_BASE);
        assert_eq!(OutSecError::CaptureDenied.code(), DIAG_LIMIT);
        // Display 落到 detail。
        assert_eq!(
            format!("{}", OutSecError::TamperDetected),
            OutSecError::TamperDetected.detail()
        );
    }

    #[test]
    fn fnv_group_known_vector() {
        // FNV-1a 64 已知向量：空 = 偏移基；"a" = 0xaf63dc4c8601ec8c；
        // "foobar" = 0x85944171f73967e8（与参考实现逐位对拍）。
        assert_eq!(fnv1a64(&[]), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn permission_matrix_group() {
        // 常量矩阵与单点判定律逐格对拍：表与律永不分叉。
        let m = permission_matrix();
        let tiers = [SafetyTier::Normal, SafetyTier::Privileged, SafetyTier::Security];
        let classes = [RegionClass::Public, RegionClass::Privileged, RegionClass::Secure];
        let mut t = 0usize;
        while t < 3 {
            let mut c = 0usize;
            while c < 3 {
                assert_eq!(
                    m[t][c],
                    matrix_allows(tiers[t], classes[c]),
                    "矩阵[{t}][{c}]与判定律分叉"
                );
                c += 1;
            }
            t += 1;
        }
        // 抽查语义：普通档只进公共级；安全档全通；特权档进不了安全级。
        assert!(matrix_allows(SafetyTier::Normal, RegionClass::Public));
        assert!(!matrix_allows(SafetyTier::Normal, RegionClass::Privileged));
        assert!(!matrix_allows(SafetyTier::Privileged, RegionClass::Secure));
        assert!(matrix_allows(SafetyTier::Security, RegionClass::Secure));
        // 区域表自洽：像素数与帧缓冲界。
        let mut r = 0usize;
        while r < REGION_COUNT {
            assert_eq!(REGIONS[r].id, r);
            assert!(REGIONS[r].rect.within_fb(), "区域 {r} 越界");
            assert_eq!(region_pixel_count(r).unwrap(), REGIONS[r].rect.area() as usize);
            r += 1;
        }
        assert!(region_pixel_count(9).is_none());
    }

    // ---------- 写者注册组 ----------

    #[test]
    fn registration_group() {
        let mut c = OutSecCore::new();
        c.request_state(StateEvent::Init).unwrap();
        let w1 = c.register_writer("app", SafetyTier::Normal, 0b0100).unwrap();
        assert_eq!(w1, 1);
        // 重名 → DuplicateWriter。
        assert_eq!(
            c.register_writer("app", SafetyTier::Normal, 0b0100),
            Err(OutSecError::DuplicateWriter)
        );
        // Offline 态不可注册（NotInitialized）。
        let mut d = OutSecCore::new();
        assert_eq!(
            d.register_writer("x", SafetyTier::Normal, 0),
            Err(OutSecError::NotInitialized)
        );
        // 账满 → InvalidState（容量 16）。
        let mut e = OutSecCore::new();
        e.request_state(StateEvent::Init).unwrap();
        let mut i = 0u32;
        while i < MAX_WRITERS as u32 {
            let name = format!("w{}", i);
            e.register_writer(&name, SafetyTier::Normal, 0).unwrap();
            i += 1;
        }
        assert_eq!(
            e.register_writer("overflow", SafetyTier::Normal, 0),
            Err(OutSecError::InvalidState)
        );
        let _ = w1;
    }

    // ---------- 越权写拦截组（冒烟判据一） ----------

    #[test]
    fn unauthorized_write_interception_group() {
        let (mut c, _sec, _privw, norm) = boot();
        let content_len = region_pixel_count(2).unwrap();
        // 1. 未注册写者 → UnknownWriter。
        assert_eq!(
            c.submit_write(99, 2, &script_payload(1, content_len)),
            Err(OutSecError::UnknownWriter)
        );
        // 2. 普通档写内容区（公共级、已授权）→ 放行。
        assert!(c.submit_write(norm, 2, &script_payload(2, content_len)).is_ok());
        // 3. 普通档写整帧（特权级）→ 权限矩阵拒绝。
        let full_len = region_pixel_count(0).unwrap();
        assert_eq!(
            c.submit_write(norm, 0, &script_payload(3, full_len)),
            Err(OutSecError::WriteDenied)
        );
        // 4. 区域未授权（普通档 mask 只有区域 2）。
        assert_eq!(
            c.submit_write(norm, 1, &script_payload(4, region_pixel_count(1).unwrap())),
            Err(OutSecError::WriteDenied)
        );
        // 5. 载荷尺寸不符。
        assert_eq!(
            c.submit_write(norm, 2, &script_payload(5, content_len - 1)),
            Err(OutSecError::PayloadSizeMismatch)
        );
        // 6. 计数与审计对账：被拒 4 次（3/4/5 各 1 + 未注册 1）。
        let rec = c.writer_snapshot(norm).unwrap();
        assert_eq!(rec.denied, 3);
        assert_eq!(rec.committed, 1);
        assert_eq!(c.audit_count_kind(AuditKind::WriterDenied), 4);
        assert_eq!(c.writes_denied, 4);
        // 7. 成功写后完整性基线可回读。
        assert!(c.verify_region(2).is_ok());
    }

    // ---------- 安全覆盖层组（冒烟判据二） ----------

    #[test]
    fn secure_layer_not_coverable_group() {
        let (mut c, sec, privw, norm) = boot();
        // 安全覆盖层：安全档属主入栈，压在安全面板条上。
        let oid = c.push_overlay(sec, OverlayKind::SecurityNotice, REGIONS[3].rect, 1).unwrap();
        assert_eq!(oid, 1);
        assert_eq!(c.overlay_depth(), 1);
        // 非安全档推安全层 → PermissionDenied。
        assert_eq!(
            c.push_overlay(privw, OverlayKind::LockScreen, REGIONS[3].rect, 2),
            Err(OutSecError::PermissionDenied)
        );
        // 栈序违规：优先级不高于栈顶 → InvalidState。
        assert_eq!(
            c.push_overlay(sec, OverlayKind::CaptureBlock, REGIONS[3].rect, 1),
            Err(OutSecError::InvalidState)
        );
        // 覆盖越权注入：特权档整帧写与安全层相交 → SecureLayerViolation。
        let full_len = region_pixel_count(0).unwrap();
        assert_eq!(
            c.submit_write(privw, 0, &script_payload(1, full_len)),
            Err(OutSecError::SecureLayerViolation)
        );
        // 普通档推普通内容层但矩形压上安全层 → SecureLayerViolation。
        assert_eq!(
            c.push_overlay(norm, OverlayKind::Content, REGIONS[3].rect, 2),
            Err(OutSecError::SecureLayerViolation)
        );
        // 不相交的普通写照常放行（内容区与安全面板条不相交）。
        let content_len = region_pixel_count(2).unwrap();
        assert!(c.submit_write(norm, 2, &script_payload(2, content_len)).is_ok());
        // 栈顶弹出纪律：弹非栈顶 → OverlayNotTop；弹栈顶 → 成功。
        let oid2 = c.push_overlay(sec, OverlayKind::LockScreen, REGIONS[3].rect, 3).unwrap();
        assert_eq!(
            c.pop_overlay(sec, oid),
            Err(OutSecError::OverlayNotTop)
        );
        assert_eq!(c.pop_overlay(sec, oid2).unwrap().id, oid2);
        // 安全档可代弹他人普通层；属主本人弹自己的普通层也合法。
        assert_eq!(c.pop_overlay(privw, oid), Err(OutSecError::PermissionDenied));
        assert_eq!(c.pop_overlay(sec, oid).unwrap().id, oid);
        assert_eq!(c.overlay_depth(), 0);
    }

    // ---------- 黑屏攻击防护组（冒烟判据三） ----------

    #[test]
    fn blackscreen_guard_group() {
        let (mut c, _sec, privw, _norm) = boot();
        let dark = dark_full_payload();
        assert_eq!(luma_average_x255(&dark), 0);
        // 前两帧暗帧放行（可能是合法熄屏），计数递增。
        assert!(c.submit_write(privw, 0, &dark).is_ok());
        assert!(c.submit_write(privw, 0, &dark).is_ok());
        // 第三帧达阈值 → 拦截 + 拉黑。
        assert_eq!(
            c.submit_write(privw, 0, &dark),
            Err(OutSecError::BlackscreenBlocked)
        );
        let rec = c.writer_snapshot(privw).unwrap();
        assert!(rec.blacklisted);
        assert_eq!(rec.dark_streak, BLACKSCREEN_DARK_LIMIT);
        // 拉黑后任何写一律拒绝。
        let lit = script_payload(7, region_pixel_count(0).unwrap());
        assert_eq!(c.submit_write(privw, 0, &lit), Err(OutSecError::WriteDenied));
        // 拦截全程入账：1 条定性 + 1 条拦截拒绝。
        assert_eq!(c.audit_count_kind(AuditKind::BlackscreenFlagged), 1);
        // 非整帧暗写不算（守卫只盯整帧）。
        let (mut d, _s, p2, _n) = boot();
        let dark_status = vec![0u8; region_pixel_count(1).unwrap()];
        assert!(d.submit_write(p2, 1, &dark_status).is_ok());
        assert!(d.writer_snapshot(p2).unwrap().dark_streak == 0);
    }

    // ---------- HDCP 诚实组（判据一组） ----------

    #[test]
    fn hdcp_honest_group() {
        let (mut c, _s, _p, _n) = boot();
        // 诚实表：conn0/conn1 = 2.2，conn2 = 1.0，conn3 = 无保护。
        assert_eq!(c.hdcp_negotiate(0, HdcpLevel::V22), Ok(HdcpLevel::V22));
        assert_eq!(c.hdcp_negotiate(1, HdcpLevel::V20), Ok(HdcpLevel::V20));
        assert_eq!(c.hdcp_negotiate(2, HdcpLevel::V10), Ok(HdcpLevel::V10));
        // conn3 无保护：请求 None 也如实放行（不上假装有保护）。
        assert_eq!(c.hdcp_negotiate(3, HdcpLevel::None), Ok(HdcpLevel::None));
        // 超能力请求 → 显性拒绝，绝不静默降级。
        assert_eq!(
            c.hdcp_negotiate(2, HdcpLevel::V20),
            Err(OutSecError::HdcpUnavailable)
        );
        assert_eq!(
            c.hdcp_negotiate(3, HdcpLevel::V10),
            Err(OutSecError::HdcpUnavailable)
        );
        // 连接器越界 → 同码拒绝。
        assert_eq!(
            c.hdcp_negotiate(4, HdcpLevel::V10),
            Err(OutSecError::HdcpUnavailable)
        );
        // 成败均入账。
        assert_eq!(c.audit_count_kind(AuditKind::HdcpNegotiated), 4);
        assert_eq!(c.audit_count_kind(AuditKind::HdcpRefused), 3);
        // 诚实报表显性呈现"无保护"。
        let rep = c.hdcp_report();
        assert!(rep.contains("无保护"));
        assert!(rep.contains("HDCP 2.2"));
    }

    // ---------- 捕获控制组（判据二组） ----------

    #[test]
    fn capture_control_group() {
        let (mut c, sec, privw, norm) = boot();
        // 普通档 → 拒绝。
        assert_eq!(c.request_capture(norm), Ok(CaptureOutcome::Denied));
        // 无安全层时特权档 → 完整放行。
        assert_eq!(c.request_capture(privw), Ok(CaptureOutcome::Allowed));
        // 安全覆盖层活动：特权档 → 挖黑放行；安全档 → 完整放行。
        let oid = c.push_overlay(sec, OverlayKind::SecurityNotice, REGIONS[3].rect, 1).unwrap();
        assert_eq!(c.request_capture(privw), Ok(CaptureOutcome::Blanked));
        assert_eq!(c.request_capture(sec), Ok(CaptureOutcome::Allowed));
        // 全局禁捕：连安全档也拒。
        c.set_capture_policy(sec, CapturePolicy::DenyAll).unwrap();
        assert_eq!(c.request_capture(sec), Ok(CaptureOutcome::Denied));
        // 非安全档改策略 → PermissionDenied。
        assert_eq!(
            c.set_capture_policy(privw, CapturePolicy::Allowed),
            Err(OutSecError::PermissionDenied)
        );
        // 账对账：尝试 5 = 拒 2（普通档 + 全局禁捕）+ 完整放行 2 + 挖黑 1。
        let cap = c.capture_snapshot();
        assert_eq!(cap.attempts, 5);
        assert_eq!(cap.granted, 2);
        assert_eq!(cap.blanked, 1);
        assert_eq!(cap.denied, 2);
        assert_eq!(c.audit_count_kind(AuditKind::CaptureRequested), 5);
        assert_eq!(c.audit_count_kind(AuditKind::CaptureDenied), 2);
        let _ = oid;
    }

    // ---------- 权限分级与审批组（判据四） ----------

    #[test]
    fn tier_promotion_approval_group() {
        let (mut c, sec, privw, norm) = boot();
        // 升权直改 → ApprovalRequired（指路审批）。
        assert_eq!(
            c.set_tier(sec, norm, SafetyTier::Privileged),
            Err(OutSecError::ApprovalRequired)
        );
        // 降权即时生效。
        assert_eq!(
            c.set_tier(sec, privw, SafetyTier::Normal),
            Ok(SafetyTier::Normal)
        );
        assert_eq!(c.writer_snapshot(privw).unwrap().tier, SafetyTier::Normal);
        // 降权入账。
        assert_eq!(c.audit_count_kind(AuditKind::TierDemoted), 1);
        // 非安全档发档位指令 → PermissionDenied。
        assert_eq!(
            c.set_tier(norm, privw, SafetyTier::Privileged),
            Err(OutSecError::PermissionDenied)
        );
        // 升权走审批：立请求 → 他人安全档批准 → 生效。
        let req = c.request_promotion(privw, SafetyTier::Privileged).unwrap();
        assert!(c.open_promotion(req).unwrap().open);
        // 自批（审批人 = 申请人）→ 职责分离拒绝；此处 privw 非安全档，
        // 先命中档位门，同样以 PermissionDenied 落账。
        assert_eq!(
            c.approve_promotion(req, privw),
            Err(OutSecError::PermissionDenied)
        );
        assert_eq!(
            c.approve_promotion(req, sec),
            Ok(SafetyTier::Privileged)
        );
        assert_eq!(c.writer_snapshot(privw).unwrap().tier, SafetyTier::Privileged);
        assert!(c.open_promotion(req).is_none());
        // 重复审批已关闭请求 → InvalidState。
        assert_eq!(
            c.approve_promotion(req, sec),
            Err(OutSecError::InvalidState)
        );
        // 审批留痕：1 立 1 批 1 驳（重复审批已关闭请求不重复入账）。
        assert_eq!(c.audit_count_kind(AuditKind::PromotionRequested), 1);
        assert_eq!(c.audit_count_kind(AuditKind::PromotionApproved), 1);
        assert_eq!(c.audit_count_kind(AuditKind::PromotionRejected), 1);
        // 批准后原被拒的整帧写应当放行。
        let full_len = region_pixel_count(0).unwrap();
        assert!(c.submit_write(privw, 0, &script_payload(9, full_len)).is_ok());
        // 驳回路径同样留痕。
        let req2 = c.request_promotion(norm, SafetyTier::Privileged).unwrap();
        assert!(c.reject_promotion(req2, sec).is_ok());
        assert_eq!(c.audit_count_kind(AuditKind::PromotionRejected), 2);
    }

    // ---------- 篡改检测组（冒烟判据五） ----------

    #[test]
    fn tamper_detection_group() {
        let (mut c, sec, _p, _n) = boot();
        // 正常写后回读一致。
        let content_len = region_pixel_count(2).unwrap();
        c.submit_write(_n, 2, &script_payload(1, content_len)).unwrap();
        assert_eq!(c.verify_region_pure(2), IntegrityVerdict::Intact);
        // 非安全档用注入面 → PermissionDenied。
        assert_eq!(
            c.inject_raw_write(_n, 2, 0, 0xAB),
            Err(OutSecError::PermissionDenied)
        );
        // 安全档注入裸写（越权 DMA 建模）→ 回读立即失配。
        c.inject_raw_write(sec, 2, 3, 0xAB).unwrap();
        assert_eq!(c.verify_region_pure(2), IntegrityVerdict::Tampered);
        assert!(c.verify_region(2).is_err());
        // 扫描：命中 → 告警入账 + 状态机拉响。
        let rep = c.run_tamper_scan().unwrap();
        assert!(!rep.consistent);
        assert_eq!(rep.mismatched_regions, vec![2usize]);
        assert_eq!(rep.alerts_added, 1);
        assert_eq!(rep.state_after, OutSecState::Alert);
        assert_eq!(c.state(), OutSecState::Alert);
        assert_eq!(c.tamper_alert_count(), 1);
        assert_eq!(c.audit_count_kind(AuditKind::TamperAlert), 1);
        // 重写区域清除基线漂移 → 扫描全绿洗白回武装态。
        c.submit_write(_n, 2, &script_payload(2, content_len)).unwrap();
        let rep2 = c.run_tamper_scan().unwrap();
        assert!(rep2.consistent);
        assert_eq!(rep2.state_after, OutSecState::Armed);
        assert_eq!(c.state(), OutSecState::Armed);
        // 注入留痕。
        assert_eq!(c.audit_count_kind(AuditKind::InjectionWrite), 1);
    }

    // ---------- 审计账与回放组（冒烟判据六） ----------

    #[test]
    fn audit_ring_and_replay_group() {
        let (mut c, _s, p, _n) = boot();
        // 溢出显性计数：连续写状态栏 300 次 → 事件数超过 256。
        let len = region_pixel_count(1).unwrap();
        let mut i = 0u32;
        while i < 300 {
            c.submit_write(p, 1, &script_payload((i as u16) | 1, len)).unwrap();
            i += 1;
        }
        assert_eq!(c.audit_events().len(), AUDIT_RING_CAP);
        assert!(c.audit_overflow_count() > 0);
        // 回放校验合法：序号严格递增 + 状态跃迁链可重演。
        let rep = c.audit_verify_replay().unwrap();
        assert!(rep.events >= AUDIT_RING_CAP as u32);
        assert_eq!(rep.digest, c.audit_digest());
        // 摘要确定性：同核重复计算必须恒等。
        assert_eq!(c.audit_digest(), c.audit_digest());
        // 溢出计数在体检里可见（黄档）。
        let h = c.health();
        assert!(h.audit_overflow > 0);
    }

    // ---------- 状态机组 ----------

    #[test]
    fn state_machine_group() {
        let mut c = OutSecCore::new();
        // Offline 直接武装 → 非法跃迁。
        assert_eq!(
            c.request_state(StateEvent::Arm),
            Err(OutSecError::InvalidState)
        );
        assert_eq!(c.audit_count_kind(AuditKind::StateChangeRejected), 1);
        // Offline 态不可写。
        let len = region_pixel_count(2).unwrap();
        assert_eq!(
            c.submit_write(1, 2, &script_payload(1, len)),
            Err(OutSecError::NotInitialized)
        );
        // 合法链：Init → Arm → Lockdown → Reset。
        c.request_state(StateEvent::Init).unwrap();
        let w = c.register_writer("w", SafetyTier::Normal, 0b0100).unwrap();
        c.request_state(StateEvent::Arm).unwrap();
        // Init 态不可写但可注册（上面已证）。
        c.request_state(StateEvent::Lockdown).unwrap();
        assert_eq!(
            c.submit_write(w, 2, &script_payload(2, len)),
            Err(OutSecError::NotInitialized)
        );
        c.request_state(StateEvent::Reset).unwrap();
        assert_eq!(c.state(), OutSecState::Offline);
        // LockedDown 只能 Reset。
        c.request_state(StateEvent::Init).unwrap();
        c.request_state(StateEvent::Arm).unwrap();
        c.request_state(StateEvent::Lockdown).unwrap();
        assert_eq!(c.request_state(StateEvent::Arm), Err(OutSecError::InvalidState));
        // 跃迁链回放终态 = 当前状态。
        let rep = c.audit_verify_replay().unwrap();
        assert_eq!(rep.replay_state, OutSecState::LockedDown);
        assert_eq!(rep.state_events, 7);
    }

    // ---------- 体检组 ----------

    #[test]
    fn health_group() {
        // 新核：未武装 → 黄。
        let fresh = OutSecCore::new();
        let h0 = fresh.health();
        assert_eq!(h0.verdict, HealthVerdict::Amber);
        assert!(h0.writers_active == 0);
        // 全流程后：绿。
        let (mut c, _s, _p, _n) = boot();
        let len = region_pixel_count(2).unwrap();
        c.submit_write(_n, 2, &script_payload(1, len)).unwrap();
        let h1 = c.health();
        assert_eq!(h1.verdict, HealthVerdict::Green);
        assert_eq!(h1.writers_active, 3);
        assert_eq!(h1.writes_committed, 1);
        // 篡改后：红。
        c.inject_raw_write(_s, 2, 0, 0x5A).unwrap();
        c.run_tamper_scan().unwrap();
        let h2 = c.health();
        assert_eq!(h2.verdict, HealthVerdict::Red);
        assert!(h2.tamper_alerts >= 1);
        // 附注非空。
        assert!(!h2.notes.is_empty());
    }

    // ---------- 完整性抽查组 ----------

    #[test]
    fn integrity_sampling_group() {
        let (mut c, _s, _p, n) = boot();
        let len = region_pixel_count(2).unwrap();
        // 第 7 次提交触发一次全脏区抽查。
        let mut i = 0u32;
        while i < 7 {
            c.submit_write(n, 2, &script_payload((i as u16) | 1, len)).unwrap();
            i += 1;
        }
        assert_eq!(c.integrity_sample_count(), 1);
        // 抽查在基线一致时静默通过，不产生告警。
        assert_eq!(c.tamper_alert_count(), 0);
        // 提交账逐写者对账（无淘汰）。
        assert_eq!(c.commit_count_for(n), 7);
        assert_eq!(c.audit_count_kind(AuditKind::WriteCommitted), 7);
    }

    // ---------- 双跑确定性对拍组（冒烟判据七） ----------

    #[test]
    fn dual_run_determinism_group() {
        let rep = run_dual_comparison().unwrap();
        assert_eq!(rep.steps, SCRIPT.len());
        assert!(rep.matched, "双跑分叉于步 {:?}", rep.diverged_step);
        assert!(rep.diverged_step.is_none());
        assert_eq!(rep.fingerprint_a, rep.fingerprint_b);
        assert_eq!(rep.audit_digest_a, rep.audit_digest_b);
        // 指纹敏感性：两个独立实例执行同一操作序列后指纹一致；
        // 任一实例多执行一步真实差异操作，指纹必分叉。
        let (mut a, _sa, _pa, na) = boot();
        let (mut b, _sb, _pb, nb) = boot();
        let len = region_pixel_count(2).unwrap();
        a.submit_write(na, 2, &script_payload(0xABCD, len)).unwrap();
        b.submit_write(nb, 2, &script_payload(0xABCD, len)).unwrap();
        a.advance_tick(1);
        b.advance_tick(1);
        assert_eq!(a.fingerprint(), b.fingerprint());
        // A 再多写一次（不同 seed）→ 分叉。
        a.submit_write(na, 2, &script_payload(0x4321, len)).unwrap();
        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    // ---------- 脚本回归组：脚本本身必须走通预期路径 ----------

    #[test]
    fn script_regression_group() {
        let mut c = OutSecCore::new();
        let mut i = 0usize;
        while i < SCRIPT.len() {
            c.advance_tick(1);
            apply_script_op(&mut c, SCRIPT[i]);
            i += 1;
        }
        // 越权写注入恰好 3 步被拦（矩阵×2 + 安全层相交×1）。
        assert_eq!(c.writes_denied, 3);
        assert_eq!(c.audit_count_kind(AuditKind::WriterDenied), 3);
        // 覆盖越权注入恰好 1 步被拦（栈序违规）。
        assert_eq!(c.audit_count_kind(AuditKind::OverlayPushDenied), 1);
        // 双扫描全绿、黑屏靶标单帧不计（未达阈值）。
        assert_eq!(c.scan_count(), 2);
        assert_eq!(c.tamper_alert_count(), 0);
        assert_eq!(c.audit_count_kind(AuditKind::BlackscreenFlagged), 0);
        // 升权审批走通一次。
        assert_eq!(c.audit_count_kind(AuditKind::PromotionApproved), 1);
        // 脚本终态：锁定后复位回 Offline；回放链合法。
        assert_eq!(c.state(), OutSecState::Offline);
        assert!(c.audit_verify_replay().is_ok());
        // 体检黄档（离线未武装）。
        assert_eq!(c.health().verdict, HealthVerdict::Amber);
    }
}
