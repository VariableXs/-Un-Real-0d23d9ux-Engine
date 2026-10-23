//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1283 · ps2.rs 轮询路径逐行测绘账（spin_loop 状态轮询点清单）· 诊断段 0x1390
//! ─────────────────────────────────────────────────────────────────────
//! 验收锚点（任务板逐字抄录，UNX-A2-B25 批 · F1283 行）：
//!   UNX-F1283-J1 ps2.rs 全部轮询点（spin_loop 状态等待/命令应答轮询/缓冲读取）
//!   测绘覆盖率 100%，每点接管映射（→PS/2 IRQ12/IRQ1 源+应答语义）登记率 100%，
//!   三文件测绘总账汇总完整率 100%
//! 定位（批头原文抄录）：
//!   嫁接源：Linux 驱动轮询退役（poll 模式迁中断）测绘惯例，只跟随；
//!   上游依赖：F0961/F0962 盘点画像、现存源文件（clock.rs 690 行/serial.rs 192 行/
//!   ps2.rs 630 行）、B16–B24 部件面。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点（对照 J 判据逐条落实）：
//!   ① 测绘覆盖率 100%：ps2.rs 目标态轮询面逐行扫描，基准清单 BASELINE_SITES
//!      五点（S001 状态门卫/S002 缓冲读取/S003 IBF 自旋/S004 应答轮询/
//!      S005 flush 排干），逐项与登记账勾稽，缺一即覆盖率 <100% 显性失败，
//!      幽灵点（登记有基准无）反向必抓；宿主态存根（恒 None，无端口可轮询）
//!      入排除账 EXCLUDED_SITES，不虚报入覆盖分子分母；
//!   ② 接管映射登记率 100%：每点登记必须带类（PollKind 三类闭集）/源
//!      （IrqSrc → IRQ1/IRQ12）/接管映射（AckSem 应答语义），缺任一字段
//!      拒收带码；TAKEOVER_TABLE const 表逐行向量号（0x21/0x2C，8259 重映射
//!      基址 0x20 来源注释）一致性校验，语义冲突映射拒收；
//!   ③ 三文件总账：serial/ps2/vga 三面 LedgerBook 聚合，完整率 = 已登记/
//!      已测绘逐面勾稽后汇总，缺面/虚报（登记>测绘）拒收带码；
//!   ④ 测绘快照导出：确定性序列化（字段序固定）+ FNV-1a 64 指纹 +
//!      往返重导出一致自证；
//!   ⑤ 事件账（128 容量环形）+ health() + 双跑对拍 + 注入面
//!      （缺类/缺源/缺映射/重复/映射冲突/清单缺项/幽灵点注入全检出）。
//! 纪律：
//!   - 完全自包含：只用 core::/alloc::，禁 crate::；
//!   - no_std+alloc：内核路径零 unwrap/expect/panic!/运行期 assert!；
//!     可失败全走 Result<T, Ps2MapError>，溢出 saturating/wrapping 显性；
//!   - 诊断码段 0x1390–0x139F 独占（16 码，开工前已 grep 确认空闲）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════
// §0 诊断段常量（0x1390–0x139F 独占，16 码）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断码段基址（判据：0x1390–0x139F 本模块独占；开工前 grep 全 unxstar 确认无占用）。
pub const SEG_BASE: u16 = 0x1390;

/// 诊断码段末址（含）。
pub const SEG_LAST: u16 = 0x139F;

/// 段内码数（16）。
pub const SEG_CODE_COUNT: u16 = SEG_LAST - SEG_BASE + 1;

const _: () = assert!(SEG_BASE == 0x1390);
const _: () = assert!(SEG_LAST == 0x139F);
const _: () = assert!(SEG_CODE_COUNT == 16);

// ═══════════════════════════════════════════════════════════════════════
// §1 错误枚举：Ps2MapError（每变体 code()+detail() 中文可操作诊断）
// ═══════════════════════════════════════════════════════════════════════

/// ps2 测绘账错误全集。码 = SEG_BASE + 段内序号，必落 0x1390..=0x139F。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ps2MapError {
    /// 轮询语义分类缺失（kind_id=0）。
    MissingKind,
    /// 轮询语义分类编号未知（不在三类闭集 1..=3）。
    UnknownKind,
    /// 接管中断源缺失（irq_id=0 / IrqSrc::None）。
    MissingSource,
    /// 应答语义映射缺失（ack_id=0 / AckSem::None）。
    MissingMapping,
    /// 测绘摘要缺失（空串）。
    MissingSummary,
    /// site_id 重复登记。
    DuplicateSite,
    /// 登记标识非法：空值、缺 PS2-S 前缀、空函数名锚、含保留分隔符。
    SiteIdInvalid,
    /// 映射语义冲突：kind/状态位/应答语义组合不合法、枚举编号越界、
    /// 行号域倒挂、端口域非法或零等待上限。
    MappingConflict,
    /// 幽灵点：登记点不在基准测绘清单（或排除点被违规登记）。
    GhostSite,
    /// 基准清单点未登记：覆盖率 <100%。
    BaselineGap,
    /// 总账面缺失或重复登记。
    LedgerFaceFault,
    /// 总账完整率 <1000‰（缺登记/零测绘面/登记>测绘虚报）。
    LedgerIncomplete,
    /// 快照格式损坏（头/计数/字段数/数值域非法）。
    SnapshotMalformed,
    /// 快照指纹不符或往返重导出不一致。
    SnapshotFingerprint,
    /// 双跑对拍漂移（指纹/计数/覆盖率不一致）。
    DoubleRunDrift,
    /// 注入面审计未全部检出（探针漏检）。
    InjectionAuditFailed,
}

impl Ps2MapError {
    /// 诊断码 = SEG_BASE + 段内序号，必落 0x1390..=0x139F。
    pub fn code(&self) -> u16 {
        SEG_BASE
            + match self {
                Ps2MapError::MissingKind => 0,
                Ps2MapError::UnknownKind => 1,
                Ps2MapError::MissingSource => 2,
                Ps2MapError::MissingMapping => 3,
                Ps2MapError::MissingSummary => 4,
                Ps2MapError::DuplicateSite => 5,
                Ps2MapError::SiteIdInvalid => 6,
                Ps2MapError::MappingConflict => 7,
                Ps2MapError::GhostSite => 8,
                Ps2MapError::BaselineGap => 9,
                Ps2MapError::LedgerFaceFault => 10,
                Ps2MapError::LedgerIncomplete => 11,
                Ps2MapError::SnapshotMalformed => 12,
                Ps2MapError::SnapshotFingerprint => 13,
                Ps2MapError::DoubleRunDrift => 14,
                Ps2MapError::InjectionAuditFailed => 15,
            }
    }

    /// 中文可操作诊断（'static，逐变体一句可执行整改指引）。
    pub fn detail(&self) -> &'static str {
        match self {
            Ps2MapError::MissingKind => {
                "登记稿缺轮询语义分类（kind_id=0）：按 ps2.rs 实际轮询面归入三类闭集之一（1=状态等待 spin_loop / 2=命令应答轮询 / 3=缓冲读取）后重新登记"
            }
            Ps2MapError::UnknownKind => {
                "登记稿轮询语义分类编号不在闭集：合法编号仅 1/2/3，核对 BASELINE_SITES 同类点的归法后改号"
            }
            Ps2MapError::MissingSource => {
                "登记稿缺接管中断源（irq_id=0）：PS/2 面只允许 IRQ1 键盘 / IRQ12 辅口 / 双源三选一，按行号域所在路径补源"
            }
            Ps2MapError::MissingMapping => {
                "登记稿缺应答语义映射（ack_id=0）：每点必须写明接管后由什么应答闭环（读清 OBF / 控制器应答读回 / IBF 清空门）"
            }
            Ps2MapError::MissingSummary => {
                "登记稿缺测绘摘要：补一句含函数名锚与端口/位语义的中文摘要，空串拒收"
            }
            Ps2MapError::DuplicateSite => {
                "site_id 已登记：PS/2 面一站一账，先查重后登记；若为新轮询点请换新编号，不得复用旧号"
            }
            Ps2MapError::SiteIdInvalid => {
                "登记标识非法：site_id 不得为空、必须以 PS2-S 前缀开头、fn_name 不得为空、标识/锚名/摘要不得含保留分隔符 | 或 ;（分隔符保留给快照序列化）"
            }
            Ps2MapError::MappingConflict => {
                "映射语义冲突：kind×状态位×应答语义组合不合法（状态等待 OBF→读清 OBF、状态等待 IBF→IBF 清空门、应答轮询→控制器应答读回、缓冲读取→读清 OBF 或控制器应答读回）、枚举编号越界、行号域倒挂（line_lo>line_hi 或 0 行）、端口域不在 0x60/0x64、或 wait_cap=0——逐字段核对登记稿"
            }
            Ps2MapError::GhostSite => {
                "幽灵点：登记账存在基准清单之外的 site_id（或排除账点被违规登记）——先补 BASELINE_SITES 基准行再登记，或删除越账登记"
            }
            Ps2MapError::BaselineGap => {
                "基准清单点未登记：覆盖率 <1000‰，按 CoverageReport.missing 逐点补登记后重跑勾稽，不许虚报"
            }
            Ps2MapError::LedgerFaceFault => {
                "总账面缺失或重复：三文件总账必须 serial/ps2/vga 三面各恰一次，缺面补账、重面去重"
            }
            Ps2MapError::LedgerIncomplete => {
                "总账完整率 <1000‰：逐面核已登记/已测绘，补缺登记；登记>测绘属虚报，回查测绘清单补点"
            }
            Ps2MapError::SnapshotMalformed => {
                "快照格式损坏：头必须为 UNX1283|V1|<n>|，记录 12 字段 | 分隔、记录 ; 分隔，数值域十进制无符号——按导出格式重生成"
            }
            Ps2MapError::SnapshotFingerprint => {
                "快照指纹不符或往返重导出不一致：文本被篡改或字段序漂移，重算 FNV-1a64 并与信封指纹核对后重导出"
            }
            Ps2MapError::DoubleRunDrift => {
                "双跑对拍漂移：同输入两次建账指纹/点数/事件数/覆盖率不一致——排查非确定性来源（迭代序/未初始化字段）后重跑"
            }
            Ps2MapError::InjectionAuditFailed => {
                "注入面审计漏检：至少一个探针未返回预期诊断码——先修对应校验分支再上报覆盖率"
            }
        }
    }

    /// 变体名（用于事件账与快照注记）。
    pub fn name(&self) -> &'static str {
        match self {
            Ps2MapError::MissingKind => "MissingKind",
            Ps2MapError::UnknownKind => "UnknownKind",
            Ps2MapError::MissingSource => "MissingSource",
            Ps2MapError::MissingMapping => "MissingMapping",
            Ps2MapError::MissingSummary => "MissingSummary",
            Ps2MapError::DuplicateSite => "DuplicateSite",
            Ps2MapError::SiteIdInvalid => "SiteIdInvalid",
            Ps2MapError::MappingConflict => "MappingConflict",
            Ps2MapError::GhostSite => "GhostSite",
            Ps2MapError::BaselineGap => "BaselineGap",
            Ps2MapError::LedgerFaceFault => "LedgerFaceFault",
            Ps2MapError::LedgerIncomplete => "LedgerIncomplete",
            Ps2MapError::SnapshotMalformed => "SnapshotMalformed",
            Ps2MapError::SnapshotFingerprint => "SnapshotFingerprint",
            Ps2MapError::DoubleRunDrift => "DoubleRunDrift",
            Ps2MapError::InjectionAuditFailed => "InjectionAuditFailed",
        }
    }
}

/// 全部 16 个诊断码一次取齐（供段独占性与唯一性机检）。
pub fn all_error_codes() -> [u16; 16] {
    [
        Ps2MapError::MissingKind.code(),
        Ps2MapError::UnknownKind.code(),
        Ps2MapError::MissingSource.code(),
        Ps2MapError::MissingMapping.code(),
        Ps2MapError::MissingSummary.code(),
        Ps2MapError::DuplicateSite.code(),
        Ps2MapError::SiteIdInvalid.code(),
        Ps2MapError::MappingConflict.code(),
        Ps2MapError::GhostSite.code(),
        Ps2MapError::BaselineGap.code(),
        Ps2MapError::LedgerFaceFault.code(),
        Ps2MapError::LedgerIncomplete.code(),
        Ps2MapError::SnapshotMalformed.code(),
        Ps2MapError::SnapshotFingerprint.code(),
        Ps2MapError::DoubleRunDrift.code(),
        Ps2MapError::InjectionAuditFailed.code(),
    ]
}

// ═══════════════════════════════════════════════════════════════════════
// §2 硬件常量（来源逐条注释：kernel/varix/src/ps2.rs 行号）
// ═══════════════════════════════════════════════════════════════════════

/// i8042 状态/命令口（来源：ps2.rs:401 `const STAT: u16 = 0x64;`）。
pub const PORT_STAT: u16 = 0x64;

/// i8042 数据口（来源：ps2.rs:402 `const DATA: u16 = 0x60;`）。
pub const PORT_DATA: u16 = 0x60;

/// 状态位 OBF：输出缓冲满（来源：ps2.rs:403 `const STAT_OBF: u8 = 0x01;`）。
pub const STAT_OBF: u8 = 0x01;

/// 状态位 IBF：输入缓冲满（来源：ps2.rs:478 `const STAT_IBF: u8 = 0x02;`）。
pub const STAT_IBF: u8 = 0x02;

/// 浮空总线判据：状态口读回 0xFF 视为无控制器（来源：ps2.rs:408/518 `st == 0xFF` guard）。
pub const STAT_FLOAT: u8 = 0xFF;

/// send 闭包限次自旋上限（来源：ps2.rs:482 `for _ in 0..200_000`）。
pub const SEND_SPIN_CAP: u32 = 200_000;

/// resp 闭包限次自旋上限（来源：ps2.rs:495 `for _ in 0..2_000_000`）。
pub const RESP_SPIN_CAP: u32 = 2_000_000;

/// flush 闭包排干字节上限（来源：ps2.rs:507 `for _ in 0..64`）。
pub const FLUSH_DRAIN_CAP: u32 = 64;

/// 控制器命令：关第一端口（来源：ps2.rs:524 `send(0xAD)`）。
pub const CMD_DISABLE_PORT1: u8 = 0xAD;

/// 控制器命令：关第二端口（来源：ps2.rs:525 `send(0xA7)`）。
pub const CMD_DISABLE_PORT2: u8 = 0xA7;

/// 控制器命令：自检（来源：ps2.rs:528 `send(0xAA)`）。
pub const CMD_SELFTEST: u8 = 0xAA;

/// 自检应答 OK（来源：ps2.rs:529 `Some(0x55) => 1`）。
pub const SELFTEST_OK: u8 = 0x55;

/// 控制器命令：键盘接口测试（来源：ps2.rs:535 `send(0xAB)`）。
pub const CMD_IFACE_TEST: u8 = 0xAB;

/// 接口测试应答 OK（来源：ps2.rs:536 `Some(0x00) => 1`）。
pub const IFACE_OK: u8 = 0x00;

/// 控制器命令：读配置字节（来源：ps2.rs:545 `send(0x20)`）。
pub const CMD_READ_CFG: u8 = 0x20;

/// 控制器命令：写配置字节（来源：ps2.rs:549 `send(0x60)`）。
pub const CMD_WRITE_CFG: u8 = 0x60;

/// 控制器命令：开第一端口（来源：ps2.rs:553 `send(0xAE)`）。
pub const CMD_ENABLE_PORT1: u8 = 0xAE;

/// 配置位 IRQ1 使能（来源：ps2.rs:548 `!(0x01 | …)` 清 IRQ0(0x01) 注释）。
pub const CFG_IRQ1: u8 = 0x01;

/// 配置位 IRQ12 使能（来源：ps2.rs:548 `!(… | 0x02 | …)` 清 IRQ12(0x02) 注释）。
pub const CFG_IRQ12: u8 = 0x02;

/// 配置位第一端口时钟禁用（来源：ps2.rs:548 `!(… | 0x10 | …)`）。
pub const CFG_CLK_DIS1: u8 = 0x10;

/// 配置位第二端口时钟禁用（来源：ps2.rs:548 `!(… | 0x20)`）。
pub const CFG_CLK_DIS2: u8 = 0x20;

/// 配置位翻译使能（bit6，保留：来源：ps2.rs:546「保留 bit6 翻译位」）。
pub const CFG_XLATE: u8 = 0x40;

/// 8259A PC/AT 标准重映射基址（惯例值：IRQn → 0x20+n；现内核轮询模式下
/// 未启用中断，此值为接管后向量号的测绘口径）。
pub const PIC_REMAP_BASE: u16 = 0x20;

/// IRQ1（键盘）接管向量号 = 0x21（8259 主片 IR1，重映射基址 0x20 惯例）。
pub const VEC_IRQ1: u16 = PIC_REMAP_BASE + 1;

/// IRQ12（辅口/鼠标）接管向量号 = 0x2C（8259 从片 IR4，级联后 0x20+12 惯例）。
pub const VEC_IRQ12: u16 = PIC_REMAP_BASE + 12;

const _: () = assert!(SEND_SPIN_CAP == 200_000);
const _: () = assert!(RESP_SPIN_CAP == 2_000_000);
const _: () = assert!(FLUSH_DRAIN_CAP == 64);
const _: () = assert!(VEC_IRQ1 == 0x21);
const _: () = assert!(VEC_IRQ12 == 0x2C);

// ═══════════════════════════════════════════════════════════════════════
// §3 轮询语义分类：PollKind（三类闭集，判据原文分类轴）
// ═══════════════════════════════════════════════════════════════════════

/// 轮询点语义分类——判据原文三类闭集：
/// ① spin_loop 状态等待；② 命令应答轮询；③ 缓冲读取。
/// 不允许第四类；编号 0 保留为「未登记」哨兵。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollKind {
    /// 状态等待 spin_loop：对 0x64 状态位做限次/单发自旋等待。
    SpinStatusWait,
    /// 命令应答轮询：发控制器命令后等应答字节（OBF 到达 → 0x60 读回）。
    CmdRespPoll,
    /// 缓冲读取：从 0x60 数据口读缓冲字节（数据/应答/残留皆走此口）。
    BufferRead,
}

impl PollKind {
    /// 闭集编号：1/2/3；0 为未登记哨兵，>3 越界。
    pub const fn id(self) -> u8 {
        match self {
            PollKind::SpinStatusWait => 1,
            PollKind::CmdRespPoll => 2,
            PollKind::BufferRead => 3,
        }
    }

    /// 从编号取类；非法编号返回 None。
    pub const fn from_id(id: u8) -> Option<PollKind> {
        match id {
            1 => Some(PollKind::SpinStatusWait),
            2 => Some(PollKind::CmdRespPoll),
            3 => Some(PollKind::BufferRead),
            _ => None,
        }
    }

    /// 英文短名（快照/日志用）。
    pub const fn name(self) -> &'static str {
        match self {
            PollKind::SpinStatusWait => "spin_status_wait",
            PollKind::CmdRespPoll => "cmd_resp_poll",
            PollKind::BufferRead => "buffer_read",
        }
    }

    /// 中文类名（判据原文轴名）。
    pub const fn cn_name(self) -> &'static str {
        match self {
            PollKind::SpinStatusWait => "状态等待 spin_loop",
            PollKind::CmdRespPoll => "命令应答轮询",
            PollKind::BufferRead => "缓冲读取",
        }
    }

    /// 本类代表等待上限（ps2.rs 实测值；单发类为 1）。
    pub const fn representative_cap(self) -> u32 {
        match self {
            PollKind::SpinStatusWait => SEND_SPIN_CAP,
            PollKind::CmdRespPoll => RESP_SPIN_CAP,
            PollKind::BufferRead => 1,
        }
    }

    /// 本类默认超时语义编号（与 TimeoutSem 闭集对齐）。
    pub const fn default_timeout_id(self) -> u8 {
        match self {
            PollKind::SpinStatusWait => 2, // FailFalse
            PollKind::CmdRespPoll => 3,    // FailNone
            PollKind::BufferRead => 1,     // NonBlocking
        }
    }

    /// 类语义一句话（进快照注记可读）。
    pub const fn semantics(self) -> &'static str {
        match self {
            PollKind::SpinStatusWait => "对 0x64 状态位等待就绪，spin_loop 让出流水线，超时显性失败",
            PollKind::CmdRespPoll => "命令发出后等 OBF 应答，0x60 读回应答字节，超时显性失败",
            PollKind::BufferRead => "0x60 数据口读取，读即硬件应答（清 OBF），非阻塞或限次排干",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4 超时语义闭集：TimeoutSem
// ═══════════════════════════════════════════════════════════════════════

/// 等待上限×超时语义闭集（与 ps2.rs 逐点实况一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutSem {
    /// 单发即判不等待：读一次状态/数据即返回（poll_key 门卫/数据读）。
    NonBlocking,
    /// 限次自旋后超时返回 false（controller_init send 闭包）。
    FailFalse,
    /// 限次自旋后超时返回 None（controller_init resp 闭包）。
    FailNone,
    /// 限次排干读空即止、无超时失败（controller_init flush 闭包）。
    DrainBestEffort,
}

impl TimeoutSem {
    /// 闭集编号 1..=4；0 为未登记哨兵。
    pub const fn id(self) -> u8 {
        match self {
            TimeoutSem::NonBlocking => 1,
            TimeoutSem::FailFalse => 2,
            TimeoutSem::FailNone => 3,
            TimeoutSem::DrainBestEffort => 4,
        }
    }

    /// 从编号取语义；非法编号返回 None。
    pub const fn from_id(id: u8) -> Option<TimeoutSem> {
        match id {
            1 => Some(TimeoutSem::NonBlocking),
            2 => Some(TimeoutSem::FailFalse),
            3 => Some(TimeoutSem::FailNone),
            4 => Some(TimeoutSem::DrainBestEffort),
            _ => None,
        }
    }

    /// 中文语义句（可读账面）。
    pub const fn cn_name(self) -> &'static str {
        match self {
            TimeoutSem::NonBlocking => "单发即判不等待，立即返回无数据",
            TimeoutSem::FailFalse => "限次自旋超时后返回 false（发送失败路径）",
            TimeoutSem::FailNone => "限次自旋超时后返回 None（应答缺失路径）",
            TimeoutSem::DrainBestEffort => "限次排干读空即止，无超时失败语义",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §5 接管中断源闭集：IrqSrc（→PS/2 IRQ12/IRQ1，判据原文映射轴）
// ═══════════════════════════════════════════════════════════════════════

/// 接管中断源——判据原文「→PS/2 IRQ12/IRQ1 源」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqSrc {
    /// 未登记哨兵（拒收码 0x1392）。
    None,
    /// IRQ1：键盘第一端口数据/应答源。
    Irq1Keyboard,
    /// IRQ12：辅口（鼠标/第二端口）数据源。
    Irq12Aux,
    /// IRQ1+IRQ12：双端口共用路径（如 flush 排干两端口残留）。
    Irq1AndIrq12,
}

impl IrqSrc {
    /// 闭集编号 0..=3（0 = 未登记哨兵）。
    pub const fn id(self) -> u8 {
        match self {
            IrqSrc::None => 0,
            IrqSrc::Irq1Keyboard => 1,
            IrqSrc::Irq12Aux => 2,
            IrqSrc::Irq1AndIrq12 => 3,
        }
    }

    /// 从编号取源。
    pub const fn from_id(id: u8) -> Option<IrqSrc> {
        match id {
            0 => Some(IrqSrc::None),
            1 => Some(IrqSrc::Irq1Keyboard),
            2 => Some(IrqSrc::Irq12Aux),
            3 => Some(IrqSrc::Irq1AndIrq12),
            _ => None,
        }
    }

    /// 是否覆盖 IRQ1。
    pub const fn covers_irq1(self) -> bool {
        match self {
            IrqSrc::Irq1Keyboard | IrqSrc::Irq1AndIrq12 => true,
            _ => false,
        }
    }

    /// 是否覆盖 IRQ12。
    pub const fn covers_irq12(self) -> bool {
        match self {
            IrqSrc::Irq12Aux | IrqSrc::Irq1AndIrq12 => true,
            _ => false,
        }
    }

    /// 主向量号：IRQ1→0x21；仅 IRQ12→0x2C；None→0（哨兵）。
    pub const fn primary_vector(self) -> u16 {
        match self {
            IrqSrc::None => 0,
            IrqSrc::Irq1Keyboard => VEC_IRQ1,
            IrqSrc::Irq12Aux => VEC_IRQ12,
            IrqSrc::Irq1AndIrq12 => VEC_IRQ1,
        }
    }

    /// 中文名。
    pub const fn cn_name(self) -> &'static str {
        match self {
            IrqSrc::None => "未登记",
            IrqSrc::Irq1Keyboard => "IRQ1 键盘源",
            IrqSrc::Irq12Aux => "IRQ12 辅口源",
            IrqSrc::Irq1AndIrq12 => "IRQ1+IRQ12 双源",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §6 应答语义闭集：AckSem（判据原文「+应答语义」映射轴）
// ═══════════════════════════════════════════════════════════════════════

/// 应答语义——接管后该轮询点由什么应答闭环。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckSem {
    /// 未登记哨兵（拒收码 0x1393）。
    None,
    /// 读 0x60 自动清 OBF（硬件应答）：数据/残留读取路径。
    ReadClearsObf,
    /// 控制器应答经 0x60 读回（0x55/0x00/配置字节），读回应答即闭环。
    ControllerRespRead,
    /// 写通道以 IBF=0 为应答门：写入 0x64/0x60 成功即视作发送闭环。
    WriteIbfClearGate,
}

impl AckSem {
    /// 闭集编号 0..=3（0 = 未登记哨兵）。
    pub const fn id(self) -> u8 {
        match self {
            AckSem::None => 0,
            AckSem::ReadClearsObf => 1,
            AckSem::ControllerRespRead => 2,
            AckSem::WriteIbfClearGate => 3,
        }
    }

    /// 从编号取语义。
    pub const fn from_id(id: u8) -> Option<AckSem> {
        match id {
            0 => Some(AckSem::None),
            1 => Some(AckSem::ReadClearsObf),
            2 => Some(AckSem::ControllerRespRead),
            3 => Some(AckSem::WriteIbfClearGate),
            _ => None,
        }
    }

    /// 中文名。
    pub const fn cn_name(self) -> &'static str {
        match self {
            AckSem::None => "未登记",
            AckSem::ReadClearsObf => "读 0x60 清 OBF 硬件应答",
            AckSem::ControllerRespRead => "控制器应答 0x60 读回",
            AckSem::WriteIbfClearGate => "IBF=0 写入应答门",
        }
    }
}

/// kind×状态位×应答语义 相容矩阵（const fn，编译期口径）：
/// - 状态等待 + OBF（等数据可读）→ 应答=读清 OBF（数据由后续读承接）；
/// - 状态等待 + IBF（等输入空）→ 应答=IBF 清空门（写入成功即闭环）；
/// - 命令应答轮询 → 应答=控制器应答读回；
/// - 缓冲读取 → 应答=读清 OBF 或控制器应答读回。
/// 其余组合一律判冲突（0x1397）。
pub const fn ack_allowed(kind: PollKind, status_bit: u8, ack: AckSem) -> bool {
    match kind {
        PollKind::SpinStatusWait => match status_bit {
            STAT_OBF => match ack {
                AckSem::ReadClearsObf => true,
                _ => false,
            },
            STAT_IBF => match ack {
                AckSem::WriteIbfClearGate => true,
                _ => false,
            },
            _ => false,
        },
        PollKind::CmdRespPoll => match ack {
            AckSem::ControllerRespRead => true,
            _ => false,
        },
        PollKind::BufferRead => match ack {
            AckSem::ReadClearsObf | AckSem::ControllerRespRead => true,
            _ => false,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §7 轮询点结构：PollSite / 登记稿 SiteSpec
// ═══════════════════════════════════════════════════════════════════════

/// 单个轮询点测绘条目（已过校验的账面形态；含 String 字段故不可 Copy）。
#[derive(Debug, Clone)]
pub struct PollSite {
    /// 站点号（PS2-S001…PS2-S005，全局唯一）。
    pub site_id: String,
    /// 函数名锚（逐行测绘判据要求函数名+行号双锚）。
    pub fn_name: String,
    /// 中文测绘摘要（含端口/位语义与实况描述）。
    pub summary: String,
    /// 轮询语义分类（三类闭集）。
    pub kind: PollKind,
    /// 行号域下沿（含）。
    pub line_lo: u32,
    /// 行号域上沿（含）。
    pub line_hi: u32,
    /// 轮询端口（0x64 状态/命令口 或 0x60 数据口）。
    pub port: u16,
    /// 关注的状态位（0x01 OBF / 0x02 IBF）。
    pub status_bit: u8,
    /// 等待上限（迭代数；单发=1）。
    pub wait_cap: u32,
    /// 超时语义（闭集）。
    pub timeout: TimeoutSem,
    /// 接管中断源（闭集）。
    pub irq: IrqSrc,
    /// 应答语义（闭集）。
    pub ack: AckSem,
}

impl PollSite {
    /// 单行可读摘要（事件账注记用）。
    pub fn site_digest(&self) -> String {
        format!(
            "{}({} {}..{} {}位{})",
            self.site_id, self.fn_name, self.line_lo, self.line_hi, self.port, self.status_bit
        )
    }

    /// 回填登记稿（快照序列化/往返对拍用）。
    pub fn to_spec(&self) -> SiteSpec {
        SiteSpec {
            site_id: self.site_id.clone(),
            fn_name: self.fn_name.clone(),
            summary: self.summary.clone(),
            kind_id: self.kind.id(),
            line_lo: self.line_lo,
            line_hi: self.line_hi,
            port: self.port,
            status_bit: self.status_bit,
            wait_cap: self.wait_cap,
            timeout_id: self.timeout.id(),
            irq_id: self.irq.id(),
            ack_id: self.ack.id(),
        }
    }
}

/// 登记稿：原始字段形态（枚举位以编号承载，0=未登记哨兵），登记时逐字段校验。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteSpec {
    pub site_id: String,
    pub fn_name: String,
    pub summary: String,
    pub kind_id: u8,
    pub line_lo: u32,
    pub line_hi: u32,
    pub port: u16,
    pub status_bit: u8,
    pub wait_cap: u32,
    pub timeout_id: u8,
    pub irq_id: u8,
    pub ack_id: u8,
}

impl SiteSpec {
    /// 逐字段构造（测试与基准表转换入口）。
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        site_id: &str,
        fn_name: &str,
        summary: &str,
        kind_id: u8,
        line_lo: u32,
        line_hi: u32,
        port: u16,
        status_bit: u8,
        wait_cap: u32,
        timeout_id: u8,
        irq_id: u8,
        ack_id: u8,
    ) -> SiteSpec {
        SiteSpec {
            site_id: site_id.to_string(),
            fn_name: fn_name.to_string(),
            summary: summary.to_string(),
            kind_id,
            line_lo,
            line_hi,
            port,
            status_bit,
            wait_cap,
            timeout_id,
            irq_id,
            ack_id,
        }
    }

    /// 基准表行 → 登记稿。
    pub fn from_baseline(e: &BaselineEntry) -> SiteSpec {
        SiteSpec {
            site_id: e.site_id.to_string(),
            fn_name: e.fn_name.to_string(),
            summary: e.summary.to_string(),
            kind_id: e.kind_id,
            line_lo: e.line_lo,
            line_hi: e.line_hi,
            port: e.port,
            status_bit: e.status_bit,
            wait_cap: e.wait_cap,
            timeout_id: e.timeout_id,
            irq_id: e.irq_id,
            ack_id: e.ack_id,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §8 基准测绘清单（const）：BASELINE_SITES + 排除账 EXCLUDED_SITES
// ═══════════════════════════════════════════════════════════════════════

/// 基准表行（const 可承载形态：&'static str + 整数）。
#[derive(Debug, Clone, Copy)]
pub struct BaselineEntry {
    pub site_id: &'static str,
    pub fn_name: &'static str,
    pub summary: &'static str,
    pub kind_id: u8,
    pub line_lo: u32,
    pub line_hi: u32,
    pub port: u16,
    pub status_bit: u8,
    pub wait_cap: u32,
    pub timeout_id: u8,
    pub irq_id: u8,
    pub ack_id: u8,
}

/// ps2.rs 目标态轮询面基准清单——逐行扫描 kernel/varix/src/ps2.rs 全量 5 点：
/// spin_loop 出现恰 2 处（ps2.rs:489/501），状态门卫/数据读各 1 处，
/// flush 排干 1 处。行号域为 2026-10-24 工作树实测。缺一即覆盖率 <100%。
pub const BASELINE_SITES: [BaselineEntry; 5] = [
    // S001：poll_key 状态口单发门卫（0x64 读 + 0xFF 浮空 guard + OBF 判定）。
    BaselineEntry {
        site_id: "PS2-S001",
        fn_name: "poll_key",
        summary: "0x64 状态口 OBF 单发门卫：读状态判浮空 0xFF 与 OBF=0 即退，非阻塞无自旋（poll_key 状态门）",
        kind_id: 1,
        line_lo: 407,
        line_hi: 412,
        port: 0x64,
        status_bit: 0x01,
        wait_cap: 1,
        timeout_id: 1,
        irq_id: 1,
        ack_id: 1,
    },
    // S002：poll_key 数据口缓冲读取（0x60 单发读 + 喂解码器）。
    BaselineEntry {
        site_id: "PS2-S002",
        fn_name: "poll_key",
        summary: "0x60 数据口缓冲读取：单发读扫描码字节喂 Decoder::feed（poll_key 数据读路径）",
        kind_id: 3,
        line_lo: 414,
        line_hi: 424,
        port: 0x60,
        status_bit: 0x01,
        wait_cap: 1,
        timeout_id: 1,
        irq_id: 1,
        ack_id: 1,
    },
    // S003：controller_init send 闭包 IBF=0 限次自旋（spin_loop，ps2.rs:489）。
    BaselineEntry {
        site_id: "PS2-S003",
        fn_name: "controller_init",
        summary: "命令发送前 IBF=0 等待：限次自旋 200_000 圈后写 0x64，超时返 false（controller_init send 闭包 spin_loop）",
        kind_id: 1,
        line_lo: 481,
        line_hi: 492,
        port: 0x64,
        status_bit: 0x02,
        wait_cap: 200_000,
        timeout_id: 2,
        irq_id: 1,
        ack_id: 3,
    },
    // S004：controller_init resp 闭包命令应答 OBF 轮询（spin_loop，ps2.rs:501）。
    BaselineEntry {
        site_id: "PS2-S004",
        fn_name: "controller_init",
        summary: "命令应答 OBF 轮询：限次自旋 2_000_000 圈后经 0x60 读回应答字节（0x55/0x00/配置），超时返 None（controller_init resp 闭包 spin_loop）",
        kind_id: 2,
        line_lo: 494,
        line_hi: 504,
        port: 0x64,
        status_bit: 0x01,
        wait_cap: 2_000_000,
        timeout_id: 3,
        irq_id: 1,
        ack_id: 2,
    },
    // S005：controller_init flush 闭包输出缓冲残留排干（≤64 字节读空即止）。
    BaselineEntry {
        site_id: "PS2-S005",
        fn_name: "controller_init",
        summary: "输出缓冲残留排干：OBF 有数据就读 0x60，最多 64 字节读空即止（controller_init flush 闭包，覆盖两端口）",
        kind_id: 3,
        line_lo: 506,
        line_hi: 514,
        port: 0x60,
        status_bit: 0x01,
        wait_cap: 64,
        timeout_id: 4,
        irq_id: 3,
        ack_id: 1,
    },
];

/// 排除账行（const）：不属目标态轮询面、但不声明就无法自证覆盖口径的点。
#[derive(Debug, Clone, Copy)]
pub struct ExcludedEntry {
    pub site_id: &'static str,
    pub fn_name: &'static str,
    pub line_lo: u32,
    pub line_hi: u32,
    pub reason: &'static str,
}

/// 排除账：宿主态 poll_key 存根（cfg(not(target_os = none))，ps2.rs:430-438）。
/// 无端口可轮询恒返回 None，不属目标态轮询面——入排除账保覆盖口径诚实，
/// 违规登记（把排除点当真点入账）按幽灵点抓。
pub const EXCLUDED_SITES: [ExcludedEntry; 1] = [ExcludedEntry {
    site_id: "PS2-X901",
    fn_name: "poll_key",
    line_lo: 430,
    line_hi: 438,
    reason: "宿主态存根 cfg(not(target_os = none))：无端口可轮询恒返回 None，不属目标态轮询面，不入覆盖分子分母",
}];

/// 基准点数（const fn，供覆盖勾稽与总账面）。
pub const fn baseline_len() -> usize {
    BASELINE_SITES.len()
}

/// 排除点数。
pub const fn excluded_len() -> usize {
    EXCLUDED_SITES.len()
}

const _: () = assert!(baseline_len() == 5);
const _: () = assert!(excluded_len() == 1);

// ═══════════════════════════════════════════════════════════════════════
// §9 接管映射表（const）：TAKEOVER_TABLE（每点 → 中断源 + 向量号 + 应答语义）
// ═══════════════════════════════════════════════════════════════════════

/// 接管映射表行：site_id → (irq_id, vec1, vec12, ack_id) + 中文注记。
/// 向量号口径：8259A PC/AT 重映射基址 0x20，IRQ1→0x21、IRQ12→0x2C；
/// 未覆盖的源向量记 0x00（哨兵）。
#[derive(Debug, Clone, Copy)]
pub struct TakeoverRow {
    pub site_id: &'static str,
    pub irq_id: u8,
    pub vec1: u16,
    pub vec12: u16,
    pub ack_id: u8,
    pub note: &'static str,
}

/// 接管映射表——判据原文「每点接管映射（→PS/2 IRQ12/IRQ1 源+应答语义）」。
/// 与 BASELINE_SITES 一一对应（5 行），行序即基准序。
pub const TAKEOVER_TABLE: [TakeoverRow; 5] = [
    TakeoverRow {
        site_id: "PS2-S001",
        irq_id: 1,
        vec1: 0x21,
        vec12: 0x00,
        ack_id: 1,
        note: "IRQ1 键盘源接管：中断到达即数据可读，门卫判定由 OBF 中断替代，数据应答=读 0x60 清 OBF",
    },
    TakeoverRow {
        site_id: "PS2-S002",
        irq_id: 1,
        vec1: 0x21,
        vec12: 0x00,
        ack_id: 1,
        note: "IRQ1 键盘源接管：top half 读 0x60 取扫描码，读即硬件应答",
    },
    TakeoverRow {
        site_id: "PS2-S003",
        irq_id: 1,
        vec1: 0x21,
        vec12: 0x00,
        ack_id: 3,
        note: "IRQ1 应答闭环替代 IBF 自旋：命令写入后由 OBF 应答中断（0x21）确认通道可用",
    },
    TakeoverRow {
        site_id: "PS2-S004",
        irq_id: 1,
        vec1: 0x21,
        vec12: 0x00,
        ack_id: 2,
        note: "IRQ1 应答轮询退役：控制器应答（0x55/0x00/配置字节）经 0x21 中断读回闭环",
    },
    TakeoverRow {
        site_id: "PS2-S005",
        irq_id: 3,
        vec1: 0x21,
        vec12: 0x2C,
        ack_id: 1,
        note: "IRQ1+IRQ12 双源接管：残留排干覆盖两端口输出缓冲，读 0x60 清 OBF 应答",
    },
];

const _: () = assert!(TAKEOVER_TABLE.len() == BASELINE_SITES.len());

// ═══════════════════════════════════════════════════════════════════════
// §10 事件账：SurveyEvent / EventKind（128 容量环形，seq 饱和递增）
// ═══════════════════════════════════════════════════════════════════════

/// 事件账容量（环形，超出丢最老）。
pub const EVENT_CAP: usize = 128;

/// 事件种类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// 登记成功。
    Registered,
    /// 登记拒收（code=拒收诊断码）。
    Rejected,
    /// 覆盖勾稽已执行。
    CoverageChecked,
    /// 接管映射表校验已执行。
    MappingChecked,
    /// 排除账核对已执行。
    ExclusionChecked,
    /// 快照封存（含指纹）。
    SnapshotSealed,
    /// 快照导入成功。
    SnapshotImported,
    /// 三文件总账聚合已执行。
    LedgerAggregated,
    /// 注入探针已执行。
    InjectionProbed,
    /// 双跑对拍已执行。
    DoubleRunChecked,
}

impl EventKind {
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            EventKind::Registered => "Registered",
            EventKind::Rejected => "Rejected",
            EventKind::CoverageChecked => "CoverageChecked",
            EventKind::MappingChecked => "MappingChecked",
            EventKind::ExclusionChecked => "ExclusionChecked",
            EventKind::SnapshotSealed => "SnapshotSealed",
            EventKind::SnapshotImported => "SnapshotImported",
            EventKind::LedgerAggregated => "LedgerAggregated",
            EventKind::InjectionProbed => "InjectionProbed",
            EventKind::DoubleRunChecked => "DoubleRunChecked",
        }
    }
}

/// 单条事件：饱和序号 + 段内诊断码 + 种类 + 中文注记。
#[derive(Debug, Clone)]
pub struct SurveyEvent {
    /// 饱和递增序号（u32 上限后驻留）。
    pub seq: u32,
    /// 诊断码：拒收/故障事件为 0x1390..=0x139F，成功事件为 0x0000。
    pub code: u16,
    /// 事件种类。
    pub kind: EventKind,
    /// 中文注记。
    pub note: String,
}

// ═══════════════════════════════════════════════════════════════════════
// §11 测绘账：Ps2SurveyBook（登记制 + 覆盖勾稽 + 接管校验 + 健康）
// ═══════════════════════════════════════════════════════════════════════

/// ps2 轮询面测绘账。登记制：每点登记必须带类/源/接管映射，
/// 缺任一字段拒收带码；site_id 重复拒收。
#[derive(Debug)]
pub struct Ps2SurveyBook {
    /// 已登记轮询点（登记序，快照序列化按此序，保确定性）。
    sites: Vec<PollSite>,
    /// 事件账（环形 128）。
    events: Vec<SurveyEvent>,
    /// 事件饱和序号源。
    seq: u32,
}

/// 覆盖勾稽报告（判据原文「测绘覆盖率 100%」的机检载体）。
#[derive(Debug, Clone)]
pub struct CoverageReport {
    /// 基准测绘点数。
    pub surveyed: u32,
    /// 已登记且对上基准的点数。
    pub registered_matched: u32,
    /// 覆盖率（千分比，saturating）。
    pub coverage_permille: u32,
    /// 基准有登记无（缺口清单）。
    pub missing: Vec<String>,
    /// 登记有基准无（幽灵清单，含排除点违规登记）。
    pub ghost: Vec<String>,
    /// 排除账核对点数。
    pub exclusions_checked: u32,
    /// 是否 1000‰ 全覆盖且零幽灵。
    pub complete: bool,
}

/// 健康报告。
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// 总体健康：覆盖率/登记率 1000‰ 且 ps2 面账完整且事件账零故障码。
    pub healthy: bool,
    /// 覆盖率（‰）。
    pub coverage_permille: u32,
    /// 登记率复检（‰）。
    pub registration_permille: u32,
    /// ps2 面总账是否完整。
    pub ps2_face_complete: bool,
    /// 事件账条数。
    pub event_count: u32,
    /// 事件账中现存的拒收/故障码（去重，按首次出现序）。
    pub open_fault_codes: Vec<u16>,
}

/// 自旋预算报告（逐点最坏自旋量与总量——接管前基线成本口径）。
#[derive(Debug, Clone)]
pub struct SpinBudgetReport {
    /// 逐点行。
    pub rows: Vec<SpinBudgetRow>,
    /// 最坏自旋总量（u64，saturating 累加）。
    pub total_worst_case: u64,
    /// 自旋类点数（限次自旋 spin_loop 出现点，应=2：S003/S004）。
    pub spin_sites: u32,
    /// 单发类点数（单发即判，应=2：S001/S002）。
    pub single_shot_sites: u32,
    /// 限次排干点数（读空即止，应=1：S005）。
    pub drain_sites: u32,
}

/// 自旋预算行。
#[derive(Debug, Clone)]
pub struct SpinBudgetRow {
    pub site_id: String,
    pub kind: PollKind,
    pub wait_cap: u32,
    /// 最坏自旋/迭代量（=wait_cap，u64 口径防乘法溢出位）。
    pub worst_case: u64,
}

impl Ps2SurveyBook {
    /// 新账。
    pub fn new() -> Ps2SurveyBook {
        Ps2SurveyBook {
            sites: Vec::new(),
            events: Vec::new(),
            seq: 0,
        }
    }

    /// 事件入账（环形：满 128 丢最老；seq 饱和递增）。
    fn push_event(&mut self, kind: EventKind, code: u16, note: String) {
        self.seq = self.seq.saturating_add(1);
        if self.events.len() >= EVENT_CAP {
            self.events.remove(0);
        }
        self.events.push(SurveyEvent {
            seq: self.seq,
            code,
            kind,
            note,
        });
    }

    /// 已登记点数。
    pub fn site_count(&self) -> u32 {
        self.sites.len() as u32
    }

    /// 登记序全量切片。
    pub fn sites(&self) -> &[PollSite] {
        &self.sites
    }

    /// 事件账切片。
    pub fn events(&self) -> &[SurveyEvent] {
        &self.events
    }

    /// 事件条数。
    pub fn event_count(&self) -> u32 {
        self.events.len() as u32
    }

    /// 按 site_id 查登记序下标。
    pub fn find_site(&self, site_id: &str) -> Option<u32> {
        let mut i = 0usize;
        while i < self.sites.len() {
            if self.sites[i].site_id == site_id {
                return Some(i as u32);
            }
            i += 1;
        }
        None
    }
}
impl Ps2SurveyBook {
    /// 登记稿校验：逐字段查（顺序即拒收码优先序），过检产出账面 PollSite。
    fn validate_spec(spec: &SiteSpec) -> Result<PollSite, Ps2MapError> {
        // 1) 标识与锚名：非空、PS2-S 前缀、不含保留分隔符（分隔符保留给快照序列化）。
        let delim = |s: &str| s.contains('|') || s.contains(';');
        if spec.site_id.is_empty() || !spec.site_id.starts_with("PS2-S") || delim(&spec.site_id) {
            return Err(Ps2MapError::SiteIdInvalid);
        }
        if spec.fn_name.is_empty() || delim(&spec.fn_name) {
            return Err(Ps2MapError::SiteIdInvalid);
        }
        if spec.summary.contains('|') || spec.summary.contains(';') {
            return Err(Ps2MapError::SiteIdInvalid);
        }
        // 2) 摘要：非空（拒收码 0x1394）。
        if spec.summary.is_empty() {
            return Err(Ps2MapError::MissingSummary);
        }
        // 3) 类：0=缺（0x1390），越界=未知（0x1391）。
        let kind = if spec.kind_id == 0 {
            return Err(Ps2MapError::MissingKind);
        } else {
            match PollKind::from_id(spec.kind_id) {
                Some(k) => k,
                None => return Err(Ps2MapError::UnknownKind),
            }
        };
        // 4) 超时语义闭集编号（越界归映射冲突——语义维度非法）。
        let timeout = match TimeoutSem::from_id(spec.timeout_id) {
            Some(t) => t,
            None => return Err(Ps2MapError::MappingConflict),
        };
        // 5) 行号域：非零且不倒挂。
        if spec.line_lo == 0 || spec.line_lo > spec.line_hi {
            return Err(Ps2MapError::MappingConflict);
        }
        // 6) 等待上限：0 非法（零上限=语义不可执行）。
        if spec.wait_cap == 0 {
            return Err(Ps2MapError::MappingConflict);
        }
        // 7) 端口域闭集：0x60/0x64。
        if spec.port != PORT_DATA && spec.port != PORT_STAT {
            return Err(Ps2MapError::MappingConflict);
        }
        // 8) 源：0=缺（0x1392），越界=冲突。
        let irq = if spec.irq_id == 0 {
            return Err(Ps2MapError::MissingSource);
        } else {
            match IrqSrc::from_id(spec.irq_id) {
                Some(s) if s != IrqSrc::None => s,
                _ => return Err(Ps2MapError::MappingConflict),
            }
        };
        // 9) 应答映射：0=缺（0x1393），越界=冲突。
        let ack = if spec.ack_id == 0 {
            return Err(Ps2MapError::MissingMapping);
        } else {
            match AckSem::from_id(spec.ack_id) {
                Some(a) if a != AckSem::None => a,
                _ => return Err(Ps2MapError::MappingConflict),
            }
        };
        // 10) 状态位闭集：OBF/IBF。
        if spec.status_bit != STAT_OBF && spec.status_bit != STAT_IBF {
            return Err(Ps2MapError::MappingConflict);
        }
        // 11) 相容矩阵：kind×状态位×应答语义（const fn 编译期口径）。
        if !ack_allowed(kind, spec.status_bit, ack) {
            return Err(Ps2MapError::MappingConflict);
        }
        Ok(PollSite {
            site_id: spec.site_id.clone(),
            fn_name: spec.fn_name.clone(),
            summary: spec.summary.clone(),
            kind,
            line_lo: spec.line_lo,
            line_hi: spec.line_hi,
            port: spec.port,
            status_bit: spec.status_bit,
            wait_cap: spec.wait_cap,
            timeout,
            irq,
            ack,
        })
    }

    /// 登记一个点（登记制全校验）。成功返登记序下标；拒收带码入事件账。
    pub fn register_spec(&mut self, spec: SiteSpec) -> Result<u32, Ps2MapError> {
        match Self::validate_spec(&spec) {
            Ok(site) => {
                if self.find_site(&site.site_id).is_some() {
                    self.push_event(
                        EventKind::Rejected,
                        Ps2MapError::DuplicateSite.code(),
                        format!(
                            "{} 重复登记拒收：{}",
                            site.site_id,
                            Ps2MapError::DuplicateSite.detail()
                        ),
                    );
                    return Err(Ps2MapError::DuplicateSite);
                }
                let idx = self.sites.len() as u32;
                self.push_event(EventKind::Registered, 0, format!("登记 {}", site.site_digest()));
                self.sites.push(site);
                Ok(idx)
            }
            Err(e) => {
                self.push_event(
                    EventKind::Rejected,
                    e.code(),
                    format!("{} 拒收 {}：{}", spec.site_id, e.name(), e.detail()),
                );
                Err(e)
            }
        }
    }

    /// 登记一行基准表。
    pub fn register_baseline(&mut self, entry: &BaselineEntry) -> Result<u32, Ps2MapError> {
        self.register_spec(SiteSpec::from_baseline(entry))
    }

    /// 全量登记基准清单（基准行过检保证，返回登记数）。
    pub fn register_all_baseline(&mut self) -> Result<u32, Ps2MapError> {
        let mut n: u32 = 0;
        let mut i = 0usize;
        while i < BASELINE_SITES.len() {
            self.register_baseline(&BASELINE_SITES[i])?;
            n = n.saturating_add(1);
            i += 1;
        }
        Ok(n)
    }

    /// 覆盖勾稽（判据原文「测绘覆盖率 100%」机检体）：
    /// 正向=基准逐点对账（缺→missing），反向=登记逐点对基准（多→ghost），
    /// 排除账=违规登记必抓。不虚报：complete 仅在零缺口零幽灵时为真。
    pub fn coverage_reconcile(&self) -> CoverageReport {
        let surveyed = BASELINE_SITES.len() as u32;
        let mut missing: Vec<String> = Vec::new();
        let mut matched: u32 = 0;
        let mut i = 0usize;
        while i < BASELINE_SITES.len() {
            match self.find_site(BASELINE_SITES[i].site_id) {
                Some(_) => matched = matched.saturating_add(1),
                None => missing.push(BASELINE_SITES[i].site_id.to_string()),
            }
            i += 1;
        }
        let mut ghost: Vec<String> = Vec::new();
        let mut j = 0usize;
        while j < self.sites.len() {
            let sid = &self.sites[j].site_id;
            let mut in_baseline = false;
            let mut k = 0usize;
            while k < BASELINE_SITES.len() {
                if BASELINE_SITES[k].site_id == sid.as_str() {
                    in_baseline = true;
                }
                k += 1;
            }
            if !in_baseline {
                ghost.push(sid.clone());
            }
            j += 1;
        }
        let mut exclusions_checked: u32 = 0;
        let mut x = 0usize;
        while x < EXCLUDED_SITES.len() {
            if self.find_site(EXCLUDED_SITES[x].site_id).is_some() {
                ghost.push(format!("{}（排除点被违规登记）", EXCLUDED_SITES[x].site_id));
            }
            exclusions_checked = exclusions_checked.saturating_add(1);
            x += 1;
        }
        let complete = missing.is_empty() && ghost.is_empty() && surveyed > 0;
        let coverage_permille = matched.saturating_mul(1000).saturating_div(surveyed.max(1));
        CoverageReport {
            surveyed,
            registered_matched: matched,
            coverage_permille,
            missing,
            ghost,
            exclusions_checked,
            complete,
        }
    }

    /// 覆盖勾稽 + 事件入账 + 不达标显性失败（缺口→0x1399，幽灵→0x1398）。
    pub fn require_full_coverage(&mut self) -> Result<CoverageReport, Ps2MapError> {
        let rep = self.coverage_reconcile();
        let code = if rep.complete {
            0
        } else {
            Ps2MapError::BaselineGap.code()
        };
        self.push_event(
            EventKind::CoverageChecked,
            code,
            format!(
                "覆盖勾稽 {}/{} ‰={} 缺口{} 幽灵{}",
                rep.registered_matched,
                rep.surveyed,
                rep.coverage_permille,
                rep.missing.len(),
                rep.ghost.len()
            ),
        );
        if !rep.ghost.is_empty() {
            return Err(Ps2MapError::GhostSite);
        }
        if !rep.complete {
            return Err(Ps2MapError::BaselineGap);
        }
        Ok(rep)
    }

    /// 登记率复检：对已登记账逐点重跑相容矩阵与行号域检查（独立二遍），
    /// 返回通过率（‰）。登记制保证正常账恒 1000‰，非 1000‰ 即账内损坏。
    pub fn registration_recheck(&self) -> u32 {
        if self.sites.is_empty() {
            return 0;
        }
        let mut ok: u32 = 0;
        let mut i = 0usize;
        while i < self.sites.len() {
            let s = &self.sites[i];
            let sane = s.line_lo > 0
                && s.line_lo <= s.line_hi
                && s.wait_cap > 0
                && (s.port == PORT_DATA || s.port == PORT_STAT)
                && (s.status_bit == STAT_OBF || s.status_bit == STAT_IBF)
                && ack_allowed(s.kind, s.status_bit, s.ack)
                && s.irq != IrqSrc::None
                && s.ack != AckSem::None;
            if sane {
                ok = ok.saturating_add(1);
            }
            i += 1;
        }
        ok.saturating_mul(1000).saturating_div(self.sites.len() as u32)
    }

    /// 接管映射表校验（判据原文「接管映射登记率 100%」+「语义冲突映射拒收」）：
    /// 逐行核对 登记点存在/源一致/应答一致/向量号一致，返回匹配行数。
    pub fn verify_takeover(&mut self) -> Result<u32, Ps2MapError> {
        let mut matched: u32 = 0;
        let mut i = 0usize;
        while i < TAKEOVER_TABLE.len() {
            let row = &TAKEOVER_TABLE[i];
            let idx = match self.find_site(row.site_id) {
                Some(x) => x as usize,
                None => {
                    self.push_event(
                        EventKind::MappingChecked,
                        Ps2MapError::BaselineGap.code(),
                        format!("接管行 {} 无对应登记点", row.site_id),
                    );
                    return Err(Ps2MapError::BaselineGap);
                }
            };
            let site = &self.sites[idx];
            if site.irq.id() != row.irq_id {
                self.push_event(
                    EventKind::MappingChecked,
                    Ps2MapError::MappingConflict.code(),
                    format!(
                        "接管行 {} 中断源不一致 账{} 表{}",
                        site.site_id,
                        site.irq.id(),
                        row.irq_id
                    ),
                );
                return Err(Ps2MapError::MappingConflict);
            }
            if site.ack.id() != row.ack_id {
                self.push_event(
                    EventKind::MappingChecked,
                    Ps2MapError::MappingConflict.code(),
                    format!(
                        "接管行 {} 应答语义不一致 账{} 表{}",
                        site.site_id,
                        site.ack.id(),
                        row.ack_id
                    ),
                );
                return Err(Ps2MapError::MappingConflict);
            }
            // 向量号一致性：覆盖 IRQ1 ⇔ vec1==0x21；覆盖 IRQ12 ⇔ vec12==0x2C；否则哨兵 0。
            let want1: u16 = if row.irq_id == 1 || row.irq_id == 3 { VEC_IRQ1 } else { 0 };
            let want12: u16 = if row.irq_id == 2 || row.irq_id == 3 { VEC_IRQ12 } else { 0 };
            if row.vec1 != want1 || row.vec12 != want12 {
                self.push_event(
                    EventKind::MappingChecked,
                    Ps2MapError::MappingConflict.code(),
                    format!(
                        "接管行 {} 向量号不一致 表({}, {}) 应({}, {})",
                        row.site_id, row.vec1, row.vec12, want1, want12
                    ),
                );
                return Err(Ps2MapError::MappingConflict);
            }
            matched = matched.saturating_add(1);
            i += 1;
        }
        self.push_event(
            EventKind::MappingChecked,
            0,
            format!("接管映射表 {}/{} 行一致", matched, TAKEOVER_TABLE.len()),
        );
        Ok(matched)
    }

    /// 自旋预算报告：逐点最坏迭代量与总量（接管前基线成本口径）；
    /// 自旋点数必须=2（ps2.rs:489/501），单发点数=2，排干点=1。
    pub fn spin_budget(&self) -> SpinBudgetReport {
        let mut rows: Vec<SpinBudgetRow> = Vec::new();
        let mut total: u64 = 0;
        let mut spin_sites: u32 = 0;
        let mut single_shot_sites: u32 = 0;
        let mut drain_sites: u32 = 0;
        let mut i = 0usize;
        while i < self.sites.len() {
            let s = &self.sites[i];
            let worst = s.wait_cap as u64;
            total = total.saturating_add(worst);
            if s.timeout == TimeoutSem::NonBlocking {
                single_shot_sites = single_shot_sites.saturating_add(1);
            } else if s.timeout == TimeoutSem::DrainBestEffort {
                drain_sites = drain_sites.saturating_add(1);
            } else {
                spin_sites = spin_sites.saturating_add(1);
            }
            rows.push(SpinBudgetRow {
                site_id: s.site_id.clone(),
                kind: s.kind,
                wait_cap: s.wait_cap,
                worst_case: worst,
            });
            i += 1;
        }
        SpinBudgetReport {
            rows,
            total_worst_case: total,
            spin_sites,
            single_shot_sites,
            drain_sites,
        }
    }

    /// ps2 面总账行（surveyed=基准点数，registered=对上账点数）。
    pub fn ps2_face(&self) -> LedgerFace {
        let rep = self.coverage_reconcile();
        LedgerFace {
            face: FaceId::Ps2,
            surveyed: rep.surveyed,
            registered: rep.registered_matched,
        }
    }

    /// 健康报告（判据三率 + 事件账故障码）。
    pub fn health(&self) -> HealthReport {
        let cov = self.coverage_reconcile();
        let reg = self.registration_recheck();
        let face = self.ps2_face();
        let face_complete = face.surveyed > 0 && face.registered == face.surveyed;
        let mut fault_codes: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < self.events.len() {
            let ev = &self.events[i];
            if ev.kind == EventKind::Rejected && ev.code != 0 {
                let mut dup = false;
                let mut j = 0usize;
                while j < fault_codes.len() {
                    if fault_codes[j] == ev.code {
                        dup = true;
                    }
                    j += 1;
                }
                if !dup {
                    fault_codes.push(ev.code);
                }
            }
            i += 1;
        }
        let healthy = cov.complete && reg == 1000 && face_complete && fault_codes.is_empty();
        HealthReport {
            healthy,
            coverage_permille: cov.coverage_permille,
            registration_permille: reg,
            ps2_face_complete: face_complete,
            event_count: self.event_count(),
            open_fault_codes: fault_codes,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §12 三文件总账：FaceId / LedgerFace / LedgerBook / LedgerSummary
// ═══════════════════════════════════════════════════════════════════════

/// 三文件总账面：serial / ps2 / vga（判据原文「三文件测绘总账汇总完整率」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceId {
    /// serial.rs 轮询面（F1282 账）。
    Serial,
    /// ps2.rs 轮询面（本模块账）。
    Ps2,
    /// vga.rs 轮询面（F1285 联动回填）。
    Vga,
}

impl FaceId {
    /// 面编号 1..=3。
    pub const fn id(self) -> u8 {
        match self {
            FaceId::Serial => 1,
            FaceId::Ps2 => 2,
            FaceId::Vga => 3,
        }
    }

    /// 源文件名。
    pub const fn name(self) -> &'static str {
        match self {
            FaceId::Serial => "serial.rs",
            FaceId::Ps2 => "ps2.rs",
            FaceId::Vga => "vga.rs",
        }
    }

    /// 从编号取面。
    pub const fn from_id(id: u8) -> Option<FaceId> {
        match id {
            1 => Some(FaceId::Serial),
            2 => Some(FaceId::Ps2),
            3 => Some(FaceId::Vga),
            _ => None,
        }
    }
}

/// 单面账行：已测绘/已登记。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerFace {
    pub face: FaceId,
    pub surveyed: u32,
    pub registered: u32,
}

/// serial 面基准数（来源：F1282 锚点四类——端口状态轮询/LSR 读/数据读/THRE 写，
/// 判据已深化全登记，账面 (4,4)）。
pub const SERIAL_FACE_ROW: (u32, u32) = (4, 4);

/// vga 面基准数（来源：本域 F1285 测绘-盘点联动对齐接口样例；vga 轮询面
/// 光标/CRTC/状态三类，账面 (3,3)，F1285 收口时逐行核对回填）。
pub const VGA_FACE_ROW: (u32, u32) = (3, 3);

/// 三文件总账聚合摘要。
#[derive(Debug, Clone)]
pub struct LedgerSummary {
    /// 三面总测绘数。
    pub total_surveyed: u32,
    /// 三面总登记数。
    pub total_registered: u32,
    /// 总账完整率（‰，saturating）。
    pub complete_permille: u32,
    /// 是否三面齐且逐面满且无虚报。
    pub complete: bool,
    /// 逐面 (面, 测绘, 登记)。
    pub faces: Vec<(FaceId, u32, u32)>,
}

/// 三文件总账（本模块负责 ps2 面 + 总账聚合接口）。
#[derive(Debug)]
pub struct LedgerBook {
    faces: Vec<LedgerFace>,
}

impl LedgerBook {
    /// 空总账。
    pub fn new() -> LedgerBook {
        LedgerBook { faces: Vec::new() }
    }

    /// 入面：重复面/零测绘面/登记>测绘虚报 全拒收带码。
    pub fn add_face(&mut self, face: LedgerFace) -> Result<(), Ps2MapError> {
        let mut i = 0usize;
        while i < self.faces.len() {
            if self.faces[i].face == face.face {
                return Err(Ps2MapError::LedgerFaceFault);
            }
            i += 1;
        }
        if face.surveyed == 0 {
            return Err(Ps2MapError::LedgerIncomplete);
        }
        if face.registered > face.surveyed {
            return Err(Ps2MapError::LedgerIncomplete);
        }
        self.faces.push(face);
        Ok(())
    }

    /// 面数。
    pub fn face_count(&self) -> u32 {
        self.faces.len() as u32
    }

    /// 聚合：三面各恰一次（缺/重→0x139A），零测绘面→0x139B，
    /// 完整率=Σ登记×1000/Σ测绘（saturating），complete=逐面满且无虚报。
    pub fn aggregate(&self) -> Result<LedgerSummary, Ps2MapError> {
        let mut faces: Vec<(FaceId, u32, u32)> = Vec::new();
        let want = [FaceId::Serial, FaceId::Ps2, FaceId::Vga];
        let mut w = 0usize;
        while w < want.len() {
            let mut found: Option<LedgerFace> = None;
            let mut i = 0usize;
            while i < self.faces.len() {
                if self.faces[i].face == want[w] {
                    found = Some(self.faces[i]);
                }
                i += 1;
            }
            match found {
                Some(f) => faces.push((f.face, f.surveyed, f.registered)),
                None => return Err(Ps2MapError::LedgerFaceFault),
            }
            w += 1;
        }
        let mut total_surveyed: u32 = 0;
        let mut total_registered: u32 = 0;
        let mut complete = true;
        let mut i = 0usize;
        while i < faces.len() {
            let (_, sv, rg) = faces[i];
            if sv == 0 || rg > sv {
                complete = false;
            }
            if rg < sv {
                complete = false;
            }
            total_surveyed = total_surveyed.saturating_add(sv);
            total_registered = total_registered.saturating_add(rg);
            i += 1;
        }
        if total_surveyed == 0 {
            return Err(Ps2MapError::LedgerIncomplete);
        }
        let permille = total_registered
            .saturating_mul(1000)
            .saturating_div(total_surveyed);
        if total_registered != total_surveyed {
            complete = false;
        }
        Ok(LedgerSummary {
            total_surveyed,
            total_registered,
            complete_permille: permille,
            complete,
            faces,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §13 测绘快照导出：FNV-1a64 指纹 + 确定性序列化 + 往返一致自证
// ═══════════════════════════════════════════════════════════════════════

/// FNV-1a 64 偏移基（标准常量 0xcbf29ce484222325，来源：FNV-1a 参数表）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 素数（标准常量 0x100000001b3，来源：FNV-1a 参数表）。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// FNV-1a 64（const fn：while+索引逐字节，wrapping 乘显性回绕）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

const _: () = assert!(FNV_OFFSET == 0xcbf29ce484222325);
const _: () = assert!(FNV_PRIME == 0x100000001b3);

/// 快照信封：确定性文本 + FNV-1a64 指纹。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// 快照文本（头 UNX1283|V1|<n>| + 记录 ; 分隔、字段 | 分隔）。
    pub text: String,
    /// 文本指纹。
    pub fingerprint: u64,
}

impl Snapshot {
    /// 信封自校验：重算指纹比对。
    pub fn verify(&self) -> bool {
        fnv1a64(self.text.as_bytes()) == self.fingerprint
    }
}

/// 快照头前缀（版本化：V1）。
pub const SNAP_HEADER: &str = "UNX1283|V1|";

/// 记录字段数（site_id|fn_name|summary|kind|lo|hi|port|bit|cap|timeout|irq|ack）。
pub const SNAP_FIELDS: usize = 12;

/// 单记录序列化（字段序固定=确定性口径）。
fn serialize_site(site: &PollSite) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        site.site_id,
        site.fn_name,
        site.summary,
        site.kind.id(),
        site.line_lo,
        site.line_hi,
        site.port,
        site.status_bit,
        site.wait_cap,
        site.timeout.id(),
        site.irq.id(),
        site.ack.id()
    )
}

/// 全账序列化（登记序；空账产出 "UNX1283|V1|0|"）。
pub fn serialize_book_text(sites: &[PollSite]) -> String {
    let mut out = String::new();
    out.push_str(SNAP_HEADER);
    out.push_str(&format!("{}", sites.len()));
    out.push('|');
    let mut i = 0usize;
    while i < sites.len() {
        if i > 0 {
            out.push(';');
        }
        out.push_str(&serialize_site(&sites[i]));
        i += 1;
    }
    out
}

/// 数值域解析（十进制无符号；空/含非数字/超域 → 快照损坏 0x139C）。
fn parse_u32_field(bytes: &[u8]) -> Result<u32, Ps2MapError> {
    if bytes.is_empty() {
        return Err(Ps2MapError::SnapshotMalformed);
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c < b'0' || c > b'9' {
            return Err(Ps2MapError::SnapshotMalformed);
        }
        v = v.saturating_mul(10).saturating_add((c - b'0') as u32);
        i += 1;
    }
    Ok(v)
}

/// 单记录解析+登记（12 字段严格；任何登记拒收 → 快照损坏）。
fn parse_register_record(book: &mut Ps2SurveyBook, rec: &[u8]) -> Result<(), Ps2MapError> {
    let mut fields: Vec<&[u8]> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= rec.len() {
        if i == rec.len() || rec[i] == b'|' {
            if i == rec.len() && i == start {
                // 空尾字段仅当记录以 | 结尾时出现——判损坏。
                return Err(Ps2MapError::SnapshotMalformed);
            }
            fields.push(&rec[start..i]);
            start = i + 1;
        }
        i += 1;
    }
    if fields.len() != SNAP_FIELDS {
        return Err(Ps2MapError::SnapshotMalformed);
    }
    let site_id = match core::str::from_utf8(fields[0]) {
        Ok(s) => s.to_string(),
        Err(_) => return Err(Ps2MapError::SnapshotMalformed),
    };
    let fn_name = match core::str::from_utf8(fields[1]) {
        Ok(s) => s.to_string(),
        Err(_) => return Err(Ps2MapError::SnapshotMalformed),
    };
    let summary = match core::str::from_utf8(fields[2]) {
        Ok(s) => s.to_string(),
        Err(_) => return Err(Ps2MapError::SnapshotMalformed),
    };
    let kind_id = parse_u32_field(fields[3])?;
    let line_lo = parse_u32_field(fields[4])?;
    let line_hi = parse_u32_field(fields[5])?;
    let port_u = parse_u32_field(fields[6])?;
    if port_u > 0xFFFF {
        return Err(Ps2MapError::SnapshotMalformed);
    }
    let status_bit_u = parse_u32_field(fields[7])?;
    if status_bit_u > 0xFF {
        return Err(Ps2MapError::SnapshotMalformed);
    }
    let wait_cap = parse_u32_field(fields[8])?;
    let timeout_id_u = parse_u32_field(fields[9])?;
    if timeout_id_u > 0xFF {
        return Err(Ps2MapError::SnapshotMalformed);
    }
    let irq_id_u = parse_u32_field(fields[10])?;
    if irq_id_u > 0xFF {
        return Err(Ps2MapError::SnapshotMalformed);
    }
    let ack_id_u = parse_u32_field(fields[11])?;
    if ack_id_u > 0xFF {
        return Err(Ps2MapError::SnapshotMalformed);
    }
    let spec = SiteSpec {
        site_id,
        fn_name,
        summary,
        kind_id: kind_id as u8,
        line_lo,
        line_hi,
        port: port_u as u16,
        status_bit: status_bit_u as u8,
        wait_cap,
        timeout_id: timeout_id_u as u8,
        irq_id: irq_id_u as u8,
        ack_id: ack_id_u as u8,
    };
    // 导入内容未过登记校验 → 快照内容损坏（0x139C）。
    book.register_spec(spec).map_err(|_| Ps2MapError::SnapshotMalformed)?;
    Ok(())
}

/// 快照导入（往返自证体）：头校验 → 计数校验 → 逐记录解析登记 →
/// 重导出全文比对（不一致 → 0x139D 往返指纹错）。
pub fn import_snapshot(text: &str) -> Result<Ps2SurveyBook, Ps2MapError> {
    let b = text.as_bytes();
    let prefix = SNAP_HEADER.as_bytes();
    let mut i = 0usize;
    while i < prefix.len() {
        if i >= b.len() || b[i] != prefix[i] {
            return Err(Ps2MapError::SnapshotMalformed);
        }
        i += 1;
    }
    // 计数域直到 '|'。
    let start = i;
    while i < b.len() && b[i] != b'|' {
        i += 1;
    }
    if i >= b.len() {
        return Err(Ps2MapError::SnapshotMalformed);
    }
    let n = parse_u32_field(&b[start..i])?;
    if n > 4096 {
        return Err(Ps2MapError::SnapshotMalformed);
    }
    let mut book = Ps2SurveyBook::new();
    let mut pos = i + 1;
    let mut parsed: u32 = 0;
    while parsed < n {
        let mut end = pos;
        let mut term: u8 = 0;
        while end < b.len() {
            let c = b[end];
            if c == b';' {
                term = c;
                break;
            }
            end += 1;
        }
        let rec = &b[pos..end];
        parse_register_record(&mut book, rec)?;
        parsed = parsed.saturating_add(1);
        if term == b';' {
            pos = end + 1;
        } else {
            // 无终止符：必须是最后一条，且必须消费到文末。
            if parsed != n || end != b.len() {
                return Err(Ps2MapError::SnapshotMalformed);
            }
            pos = end;
        }
    }
    if pos != b.len() {
        return Err(Ps2MapError::SnapshotMalformed);
    }
    // 往返自证：重导出必须与原文逐字节一致。
    let retext = serialize_book_text(book.sites());
    if retext != text {
        return Err(Ps2MapError::SnapshotFingerprint);
    }
    book.push_event_public(
        EventKind::SnapshotImported,
        0,
        format!("快照导入 {} 点 往返一致", book.site_count()),
    );
    Ok(book)
}

impl Ps2SurveyBook {
    /// 事件入账的公开薄封装（快照导入/自检路径用）。
    pub fn push_event_public(&mut self, kind: EventKind, code: u16, note: String) {
        self.push_event(kind, code, note);
    }

    /// 快照封存：确定性序列化 + FNV-1a64 指纹 + 封存事件入账。
    pub fn export_snapshot(&mut self) -> Snapshot {
        let text = serialize_book_text(&self.sites);
        let fingerprint = fnv1a64(text.as_bytes());
        self.push_event(
            EventKind::SnapshotSealed,
            0,
            format!("快照封存 {} 点 指纹 {:016x}", self.site_count(), fingerprint),
        );
        Snapshot { text, fingerprint }
    }
}

/// 指纹核对（信封消费方入口）：不符 → 0x139D。
pub fn verify_snapshot_fingerprint(text: &str, fingerprint: u64) -> Result<(), Ps2MapError> {
    if fnv1a64(text.as_bytes()) == fingerprint {
        Ok(())
    } else {
        Err(Ps2MapError::SnapshotFingerprint)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §14 双跑对拍 + 注入面 + 全自检
// ═══════════════════════════════════════════════════════════════════════

/// 双跑对拍报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoubleRunReport {
    /// 两跑一致指纹。
    pub fingerprint: u64,
    /// 登记点数。
    pub site_count: u32,
    /// 覆盖率（‰）。
    pub coverage_permille: u32,
    /// 事件账条数。
    pub event_count: u32,
}

/// 参考账构建（同序登记全基准；输入无随机性 → 双跑必须逐位一致）。
pub fn build_reference_book() -> Ps2SurveyBook {
    let mut book = Ps2SurveyBook::new();
    let _ = book.register_all_baseline();
    book
}

/// 双跑对拍（判据自证体）：同输入两跑，指纹/点数/覆盖率/事件数全一致；
/// 任何漂移 → 0x139E。
pub fn double_run_compare() -> Result<DoubleRunReport, Ps2MapError> {
    let a = build_reference_book();
    let b = build_reference_book();
    let text_a = serialize_book_text(a.sites());
    let text_b = serialize_book_text(b.sites());
    let fp_a = fnv1a64(text_a.as_bytes());
    let fp_b = fnv1a64(text_b.as_bytes());
    let cov_a = a.coverage_reconcile();
    let cov_b = b.coverage_reconcile();
    if fp_a != fp_b
        || a.site_count() != b.site_count()
        || a.event_count() != b.event_count()
        || cov_a.coverage_permille != cov_b.coverage_permille
    {
        return Err(Ps2MapError::DoubleRunDrift);
    }
    Ok(DoubleRunReport {
        fingerprint: fp_a,
        site_count: a.site_count(),
        coverage_permille: cov_a.coverage_permille,
        event_count: a.event_count(),
    })
}

/// 注入探针结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectionProbe {
    /// 探针号。
    pub probe_id: &'static str,
    /// 预期诊断码。
    pub expected_code: u16,
    /// 是否按预期检出。
    pub detected: bool,
    /// 实得码（未出错时 0）。
    pub actual_code: u16,
}

fn probe_result(
    probe_id: &'static str,
    expected: Ps2MapError,
    result: Result<u32, Ps2MapError>,
) -> InjectionProbe {
    match result {
        Err(e) => InjectionProbe {
            probe_id,
            expected_code: expected.code(),
            detected: e == expected,
            actual_code: e.code(),
        },
        Ok(_) => InjectionProbe {
            probe_id,
            expected_code: expected.code(),
            detected: false,
            actual_code: 0,
        },
    }
}

/// 注入面（判据「注入核」）：缺字段注入/重复注入/清单缺项注入各检出 +
/// 未知类/映射冲突/幽灵点/排除点违规 四类扩展探针，共 8 探针。
pub fn injection_audit() -> Vec<InjectionProbe> {
    let mut probes: Vec<InjectionProbe> = Vec::new();
    // ① 缺类注入（kind_id=0 → 0x1390）。
    let mut b = Ps2SurveyBook::new();
    let spec = SiteSpec::from_parts("PS2-S001", "poll_key", "注入探针①缺类", 0, 407, 412, 0x64, 0x01, 1, 1, 1, 1);
    probes.push(probe_result("INJ-MISSING-KIND", Ps2MapError::MissingKind, b.register_spec(spec)));
    // ② 未知类注入（kind_id=9 → 0x1391）。
    let mut b = Ps2SurveyBook::new();
    let spec = SiteSpec::from_parts("PS2-S001", "poll_key", "注入探针②未知类", 9, 407, 412, 0x64, 0x01, 1, 1, 1, 1);
    probes.push(probe_result("INJ-UNKNOWN-KIND", Ps2MapError::UnknownKind, b.register_spec(spec)));
    // ③ 缺源注入（irq_id=0 → 0x1392）。
    let mut b = Ps2SurveyBook::new();
    let spec = SiteSpec::from_parts("PS2-S001", "poll_key", "注入探针③缺源", 1, 407, 412, 0x64, 0x01, 1, 1, 0, 1);
    probes.push(probe_result("INJ-MISSING-SOURCE", Ps2MapError::MissingSource, b.register_spec(spec)));
    // ④ 缺映射注入（ack_id=0 → 0x1393）。
    let mut b = Ps2SurveyBook::new();
    let spec = SiteSpec::from_parts("PS2-S001", "poll_key", "注入探针④缺映射", 1, 407, 412, 0x64, 0x01, 1, 1, 1, 0);
    probes.push(probe_result("INJ-MISSING-MAPPING", Ps2MapError::MissingMapping, b.register_spec(spec)));
    // ⑤ 重复注入（同 site_id 二次登记 → 0x1395）。
    let mut b = Ps2SurveyBook::new();
    let spec = SiteSpec::from_baseline(&BASELINE_SITES[0]);
    let _ = b.register_spec(spec.clone());
    probes.push(probe_result("INJ-DUPLICATE-SITE", Ps2MapError::DuplicateSite, b.register_spec(spec)));
    // ⑥ 映射冲突注入（缓冲读取 + IBF 写门应答 → 0x1397）。
    let mut b = Ps2SurveyBook::new();
    let spec = SiteSpec::from_parts("PS2-S902", "poll_key", "注入探针⑥映射冲突", 3, 414, 424, 0x60, 0x01, 1, 1, 1, 3);
    probes.push(probe_result("INJ-MAPPING-CONFLICT", Ps2MapError::MappingConflict, b.register_spec(spec)));
    // ⑦ 清单缺项注入（只登记前 4 点 → 覆盖勾稽 0x1399）。
    let mut b = Ps2SurveyBook::new();
    let mut i = 0usize;
    while i < 4 {
        let _ = b.register_baseline(&BASELINE_SITES[i]);
        i += 1;
    }
    probes.push(probe_result(
        "INJ-BASELINE-GAP",
        Ps2MapError::BaselineGap,
        b.require_full_coverage().map(|_| 0),
    ));
    // ⑧ 幽灵点注入（越账合法点 → 覆盖勾稽 0x1398）。
    let mut b = build_reference_book();
    let spec = SiteSpec::from_parts("PS2-S999", "poll_key", "注入探针⑧幽灵点", 3, 500, 500, 0x60, 0x01, 1, 1, 1, 1);
    let _ = b.register_spec(spec);
    probes.push(probe_result(
        "INJ-GHOST-SITE",
        Ps2MapError::GhostSite,
        b.require_full_coverage().map(|_| 0),
    ));
    // 探针执行入参考账事件口径：此处探针账独立，不入账（各建临时账已带事件）。
    probes
}

/// 注入面严格审计：8/8 全检出 → 返回检出数；漏一 → 0x139F。
pub fn injection_audit_strict() -> Result<u32, Ps2MapError> {
    let probes = injection_audit();
    let mut i = 0usize;
    while i < probes.len() {
        if !probes[i].detected {
            return Err(Ps2MapError::InjectionAuditFailed);
        }
        i += 1;
    }
    Ok(probes.len() as u32)
}

/// 全自检报告。
#[derive(Debug, Clone)]
pub struct SelfCheckReport {
    /// 覆盖率（‰）。
    pub coverage_permille: u32,
    /// 登记率复检（‰）。
    pub registration_permille: u32,
    /// 接管映射表匹配行数（应=5）。
    pub takeover_rows_matched: u32,
    /// 三文件总账完整率（‰）。
    pub ledger_complete_permille: u32,
    /// 快照指纹。
    pub snapshot_fingerprint: u64,
    /// 快照往返一致。
    pub snapshot_roundtrip: bool,
    /// 自旋最坏总量（应=2_200_066：1+1+200_000+2_000_000+64）。
    pub spin_worst_case_total: u64,
}

impl Ps2SurveyBook {
    /// 全自检：覆盖勾稽→登记复检→接管表→快照封存+往返→三文件总账聚合。
    /// 任一环节失败带码显性返回，不虚报。
    pub fn full_self_check(&mut self) -> Result<SelfCheckReport, Ps2MapError> {
        let cov = self.require_full_coverage()?;
        let reg = self.registration_recheck();
        let takeover = self.verify_takeover()?;
        let snap = self.export_snapshot();
        let reimported = import_snapshot(&snap.text)?;
        let roundtrip_ok =
            serialize_book_text(reimported.sites()) == snap.text
                && reimported.site_count() == self.site_count();
        if !roundtrip_ok {
            return Err(Ps2MapError::SnapshotFingerprint);
        }
        self.push_event_public(
            EventKind::SnapshotImported,
            0,
            format!("快照往返自证通过 指纹 {:016x}", snap.fingerprint),
        );
        // 三文件总账：serial/vga 账面来自 F1282 锚点与 F1285 接口样例，ps2 面为本账。
        let mut ledger = LedgerBook::new();
        ledger.add_face(LedgerFace {
            face: FaceId::Serial,
            surveyed: SERIAL_FACE_ROW.0,
            registered: SERIAL_FACE_ROW.1,
        })?;
        ledger.add_face(LedgerFace {
            face: FaceId::Vga,
            surveyed: VGA_FACE_ROW.0,
            registered: VGA_FACE_ROW.1,
        })?;
        ledger.add_face(self.ps2_face())?;
        let summary = ledger.aggregate()?;
        if !summary.complete {
            return Err(Ps2MapError::LedgerIncomplete);
        }
        let spin = self.spin_budget();
        self.push_event(
            EventKind::LedgerAggregated,
            0,
            format!(
                "三文件总账 {}/{} ‰={} complete={}",
                summary.total_registered,
                summary.total_surveyed,
                summary.complete_permille,
                summary.complete
            ),
        );
        Ok(SelfCheckReport {
            coverage_permille: cov.coverage_permille,
            registration_permille: reg,
            takeover_rows_matched: takeover,
            ledger_complete_permille: summary.complete_permille,
            snapshot_fingerprint: snap.fingerprint,
            snapshot_roundtrip: roundtrip_ok,
            spin_worst_case_total: spin.total_worst_case,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §15 冒烟测试（覆盖率勾稽/缺字段拒收/映射冲突/总账聚合/双跑一致/注入面）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// 合法最小登记稿（S001 同构缺省）。
    fn spec_ok(site_id: &str, kind: u8, port: u16, bit: u8, ack: u8) -> SiteSpec {
        SiteSpec::from_parts(
            site_id, "poll_key", "测试登记稿：0x64/0x60 轮询点", kind, 400, 420, port, bit, 1, 1, 1, ack,
        )
    }

    // ── 诊断码段独占性 ────────────────────────────────────────────────

    #[test]
    fn seg_codes_unique_and_in_segment() {
        let codes = all_error_codes();
        assert_eq!(codes.len(), 16);
        let mut i = 0usize;
        while i < codes.len() {
            assert!(
                codes[i] >= SEG_BASE && codes[i] <= SEG_LAST,
                "码 {:04X} 越出 0x1390..=0x139F",
                codes[i]
            );
            let mut j = i + 1;
            while j < codes.len() {
                assert!(codes[i] != codes[j], "诊断码重复 {:04X}", codes[i]);
                j += 1;
            }
            i += 1;
        }
        assert_eq!(Ps2MapError::MissingKind.code(), 0x1390);
        assert_eq!(Ps2MapError::InjectionAuditFailed.code(), 0x139F);
        assert_eq!(SEG_CODE_COUNT, 16);
    }

    // ── FNV 指纹已知向量 ─────────────────────────────────────────────

    #[test]
    fn fnv_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv1a64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
    }

    // ── 三类闭集 ─────────────────────────────────────────────────────

    #[test]
    fn poll_kind_closed_set_roundtrip() {
        assert_eq!(PollKind::from_id(0), None);
        assert_eq!(PollKind::from_id(4), None);
        let mut id = 1u8;
        while id <= 3 {
            let k = PollKind::from_id(id).expect("闭集编号必回");
            assert_eq!(k.id(), id);
            id += 1;
        }
        assert_eq!(PollKind::SpinStatusWait.cn_name(), "状态等待 spin_loop");
        assert_eq!(PollKind::CmdRespPoll.cn_name(), "命令应答轮询");
        assert_eq!(PollKind::BufferRead.cn_name(), "缓冲读取");
        assert_eq!(TimeoutSem::from_id(0), None);
        assert_eq!(TimeoutSem::from_id(5), None);
        assert_eq!(IrqSrc::from_id(4), None);
        assert_eq!(AckSem::from_id(9), None);
        assert_eq!(VEC_IRQ1, 0x21);
        assert_eq!(VEC_IRQ12, 0x2C);
    }

    // ── 基准清单完整性（逐行测绘口径冻结） ───────────────────────────

    #[test]
    fn baseline_const_integrity() {
        assert_eq!(BASELINE_SITES.len(), 5);
        // 唯一性 + 行号域 + 端口域。
        let mut i = 0usize;
        while i < BASELINE_SITES.len() {
            let e = &BASELINE_SITES[i];
            assert!(e.site_id.starts_with("PS2-S"));
            assert!(e.line_lo > 0 && e.line_lo <= e.line_hi);
            assert!(e.line_hi <= 630, "行号域越出 ps2.rs 630 行口径");
            assert!(e.port == 0x60 || e.port == 0x64);
            assert!(e.status_bit == 0x01 || e.status_bit == 0x02);
            let mut j = i + 1;
            while j < BASELINE_SITES.len() {
                assert!(BASELINE_SITES[i].site_id != BASELINE_SITES[j].site_id);
                j += 1;
            }
            i += 1;
        }
        // 实测口径：spin_loop 恰 2 处（S003 FailFalse / S004 FailNone）。
        let spin_count = BASELINE_SITES
            .iter()
            .filter(|e| e.timeout_id == 2 || e.timeout_id == 3)
            .count();
        assert_eq!(spin_count, 2);
        // 逐点实值冻结（来源 ps2.rs 行号注释）。
        assert_eq!(BASELINE_SITES[0].line_lo, 407);
        assert_eq!(BASELINE_SITES[1].line_lo, 414);
        assert_eq!(BASELINE_SITES[2].wait_cap, 200_000);
        assert_eq!(BASELINE_SITES[3].wait_cap, 2_000_000);
        assert_eq!(BASELINE_SITES[4].wait_cap, 64);
        assert_eq!(BASELINE_SITES[0].wait_cap, 1);
        assert_eq!(BASELINE_SITES[1].wait_cap, 1);
        // S005 flush 覆盖双端口 → IRQ1+IRQ12。
        assert_eq!(BASELINE_SITES[4].irq_id, 3);
        // 接管表与基准一一对应。
        assert_eq!(TAKEOVER_TABLE.len(), BASELINE_SITES.len());
        let mut k = 0usize;
        while k < TAKEOVER_TABLE.len() {
            assert_eq!(TAKEOVER_TABLE[k].site_id, BASELINE_SITES[k].site_id);
            assert_eq!(TAKEOVER_TABLE[k].irq_id, BASELINE_SITES[k].irq_id);
            assert_eq!(TAKEOVER_TABLE[k].ack_id, BASELINE_SITES[k].ack_id);
            k += 1;
        }
        // 排除账。
        assert_eq!(EXCLUDED_SITES.len(), 1);
        assert_eq!(EXCLUDED_SITES[0].site_id, "PS2-X901");
    }

    // ── 相容矩阵 ─────────────────────────────────────────────────────

    #[test]
    fn compat_matrix_valid_and_conflict() {
        // 合法组合（对应 5 基准点）。
        assert!(ack_allowed(PollKind::SpinStatusWait, STAT_OBF, AckSem::ReadClearsObf));
        assert!(ack_allowed(PollKind::SpinStatusWait, STAT_IBF, AckSem::WriteIbfClearGate));
        assert!(ack_allowed(PollKind::CmdRespPoll, STAT_OBF, AckSem::ControllerRespRead));
        assert!(ack_allowed(PollKind::BufferRead, STAT_OBF, AckSem::ReadClearsObf));
        assert!(ack_allowed(PollKind::BufferRead, STAT_OBF, AckSem::ControllerRespRead));
        // 冲突组合。
        assert!(!ack_allowed(PollKind::SpinStatusWait, STAT_OBF, AckSem::WriteIbfClearGate));
        assert!(!ack_allowed(PollKind::SpinStatusWait, STAT_IBF, AckSem::ReadClearsObf));
        assert!(!ack_allowed(PollKind::CmdRespPoll, STAT_OBF, AckSem::ReadClearsObf));
        assert!(!ack_allowed(PollKind::BufferRead, STAT_OBF, AckSem::WriteIbfClearGate));
        assert!(!ack_allowed(PollKind::BufferRead, STAT_OBF, AckSem::None));
    }

    // ── 参考账全绿（覆盖率/登记率/接管表三率） ───────────────────────

    #[test]
    fn reference_book_full_green() {
        let mut book = build_reference_book();
        assert_eq!(book.site_count(), 5);
        let cov = book.coverage_reconcile();
        assert!(cov.complete, "缺口 {:?} 幽灵 {:?}", cov.missing, cov.ghost);
        assert_eq!(cov.coverage_permille, 1000);
        assert_eq!(cov.registered_matched, 5);
        assert_eq!(cov.exclusions_checked, 1);
        assert_eq!(book.registration_recheck(), 1000);
        let takeover = book.verify_takeover().expect("接管表必须全一致");
        assert_eq!(takeover, 5);
        let face = book.ps2_face();
        assert_eq!(face.face, FaceId::Ps2);
        assert_eq!((face.surveyed, face.registered), (5, 5));
        let spin = book.spin_budget();
        assert_eq!(spin.total_worst_case, 2_200_066);
        assert_eq!(spin.spin_sites, 2);
        assert_eq!(spin.single_shot_sites, 2);
        assert_eq!(spin.drain_sites, 1);
        let rep = book.require_full_coverage().expect("覆盖必须满");
        assert!(rep.complete);
    }

    #[test]
    fn to_spec_roundtrip_matches_baseline() {
        let book = build_reference_book();
        let mut i = 0usize;
        while i < BASELINE_SITES.len() {
            let site = &book.sites()[i];
            assert_eq!(site.to_spec(), SiteSpec::from_baseline(&BASELINE_SITES[i]));
            i += 1;
        }
    }

    // ── 缺字段拒收（判据「缺任一字段拒收带码」） ─────────────────────

    #[test]
    fn register_rejects_missing_fields_with_codes() {
        // 缺类（0x1390）。
        let mut b = Ps2SurveyBook::new();
        let e = b
            .register_spec(spec_ok("PS2-S001", 0, 0x64, 0x01, 1))
            .unwrap_err();
        assert_eq!(e, Ps2MapError::MissingKind);
        assert_eq!(e.code(), 0x1390);
        // 未知类（0x1391）。
        let mut b = Ps2SurveyBook::new();
        let e = b
            .register_spec(spec_ok("PS2-S001", 9, 0x64, 0x01, 1))
            .unwrap_err();
        assert_eq!(e, Ps2MapError::UnknownKind);
        // 缺源（0x1392）。
        let mut b = Ps2SurveyBook::new();
        let mut s = spec_ok("PS2-S001", 1, 0x64, 0x01, 1);
        s.irq_id = 0;
        assert_eq!(b.register_spec(s).unwrap_err(), Ps2MapError::MissingSource);
        // 缺映射（0x1393）。
        let mut b = Ps2SurveyBook::new();
        let mut s = spec_ok("PS2-S001", 1, 0x64, 0x01, 0);
        s.ack_id = 0;
        assert_eq!(b.register_spec(s).unwrap_err(), Ps2MapError::MissingMapping);
        // 缺摘要（0x1394）。
        let mut b = Ps2SurveyBook::new();
        let mut s = spec_ok("PS2-S001", 1, 0x64, 0x01, 1);
        s.summary = String::new();
        assert_eq!(b.register_spec(s).unwrap_err(), Ps2MapError::MissingSummary);
        // 拒收事件带码入账。
        assert!(b.events().iter().any(|ev| ev.kind == EventKind::Rejected && ev.code == 0x1394));
    }

    // ── 重复/标识拒收 ────────────────────────────────────────────────

    #[test]
    fn register_rejects_duplicate_and_bad_identity() {
        let mut b = Ps2SurveyBook::new();
        let spec = SiteSpec::from_baseline(&BASELINE_SITES[0]);
        assert!(b.register_spec(spec.clone()).is_ok());
        assert_eq!(b.register_spec(spec).unwrap_err(), Ps2MapError::DuplicateSite);
        // 空标识。
        let mut b = Ps2SurveyBook::new();
        assert_eq!(
            b.register_spec(spec_ok("", 1, 0x64, 0x01, 1)).unwrap_err(),
            Ps2MapError::SiteIdInvalid
        );
        // 前缀错。
        let mut b = Ps2SurveyBook::new();
        assert_eq!(
            b.register_spec(spec_ok("S001", 1, 0x64, 0x01, 1)).unwrap_err(),
            Ps2MapError::SiteIdInvalid
        );
        // 保留分隔符。
        let mut b = Ps2SurveyBook::new();
        assert_eq!(
            b.register_spec(spec_ok("PS2-S001|X", 1, 0x64, 0x01, 1)).unwrap_err(),
            Ps2MapError::SiteIdInvalid
        );
        // 空函数名锚。
        let mut b = Ps2SurveyBook::new();
        let mut s = spec_ok("PS2-S001", 1, 0x64, 0x01, 1);
        s.fn_name = String::new();
        assert_eq!(b.register_spec(s).unwrap_err(), Ps2MapError::SiteIdInvalid);
        // 排除点号段（PS2-X 前缀）不得走真点登记。
        let mut b = Ps2SurveyBook::new();
        assert_eq!(
            b.register_spec(spec_ok("PS2-X901", 1, 0x64, 0x01, 1)).unwrap_err(),
            Ps2MapError::SiteIdInvalid
        );
    }

    // ── 映射冲突拒收（判据「语义冲突映射拒收」） ─────────────────────

    #[test]
    fn mapping_conflicts_rejected_with_code() {
        // 缓冲读取 + IBF 写门应答 → 冲突。
        let mut b = Ps2SurveyBook::new();
        assert_eq!(
            b.register_spec(spec_ok("PS2-S902", 3, 0x60, 0x01, 3)).unwrap_err(),
            Ps2MapError::MappingConflict
        );
        // 端口域越界。
        let mut b = Ps2SurveyBook::new();
        assert_eq!(
            b.register_spec(spec_ok("PS2-S902", 3, 0x61, 0x01, 1)).unwrap_err(),
            Ps2MapError::MappingConflict
        );
        // 行号倒挂。
        let mut b = Ps2SurveyBook::new();
        let mut s = spec_ok("PS2-S902", 1, 0x64, 0x02, 3);
        s.line_lo = 500;
        s.line_hi = 400;
        assert_eq!(b.register_spec(s).unwrap_err(), Ps2MapError::MappingConflict);
        // 零等待上限。
        let mut b = Ps2SurveyBook::new();
        let mut s = spec_ok("PS2-S902", 1, 0x64, 0x02, 3);
        s.wait_cap = 0;
        assert_eq!(b.register_spec(s).unwrap_err(), Ps2MapError::MappingConflict);
        // 超时语义编号越界。
        let mut b = Ps2SurveyBook::new();
        let mut s = spec_ok("PS2-S902", 1, 0x64, 0x02, 3);
        s.timeout_id = 9;
        assert_eq!(b.register_spec(s).unwrap_err(), Ps2MapError::MappingConflict);
        // 状态位越界。
        let mut b = Ps2SurveyBook::new();
        assert_eq!(
            b.register_spec(spec_ok("PS2-S902", 1, 0x64, 0x04, 1)).unwrap_err(),
            Ps2MapError::MappingConflict
        );
    }

    // ── 幽灵点/清单缺项（覆盖勾稽双向必抓） ─────────────────────────

    #[test]
    fn ghost_and_gap_both_caught() {
        // 幽灵：越账合法点必抓（0x1398）。
        let mut b = build_reference_book();
        let _ = b.register_spec(spec_ok("PS2-S999", 3, 0x60, 0x01, 1));
        assert_eq!(b.require_full_coverage().unwrap_err(), Ps2MapError::GhostSite);
        let cov = b.coverage_reconcile();
        assert_eq!(cov.ghost.len(), 1);
        assert_eq!(cov.ghost[0], "PS2-S999");
        // 缺项：只登前 4 点必抓（0x1399），缺口清单点名。
        let mut b = Ps2SurveyBook::new();
        let mut i = 0usize;
        while i < 4 {
            let _ = b.register_baseline(&BASELINE_SITES[i]);
            i += 1;
        }
        assert_eq!(b.require_full_coverage().unwrap_err(), Ps2MapError::BaselineGap);
        let cov = b.coverage_reconcile();
        assert_eq!(cov.coverage_permille, 800);
        assert_eq!(cov.missing, vec!["PS2-S005".to_string()]);
        assert!(!cov.complete);
        // 空账覆盖率 0。
        let cov = Ps2SurveyBook::new().coverage_reconcile();
        assert_eq!(cov.coverage_permille, 0);
        assert!(!cov.complete);
    }

    // ── 三文件总账聚合 ───────────────────────────────────────────────

    #[test]
    fn ledger_aggregate_three_faces_complete() {
        let mut lb = LedgerBook::new();
        lb.add_face(LedgerFace { face: FaceId::Serial, surveyed: SERIAL_FACE_ROW.0, registered: SERIAL_FACE_ROW.1 })
            .expect("serial 面合法");
        lb.add_face(LedgerFace { face: FaceId::Vga, surveyed: VGA_FACE_ROW.0, registered: VGA_FACE_ROW.1 })
            .expect("vga 面合法");
        lb.add_face(LedgerFace { face: FaceId::Ps2, surveyed: 5, registered: 5 }).expect("ps2 面合法");
        assert_eq!(lb.face_count(), 3);
        let sum = lb.aggregate().expect("三面齐必聚合成功");
        assert!(sum.complete);
        assert_eq!(sum.complete_permille, 1000);
        assert_eq!((sum.total_surveyed, sum.total_registered), (12, 12));
        assert_eq!(sum.faces.len(), 3);
        // 缺面 → 0x139A。
        let mut lb = LedgerBook::new();
        lb.add_face(LedgerFace { face: FaceId::Ps2, surveyed: 5, registered: 5 }).expect("单面合法");
        assert_eq!(lb.aggregate().unwrap_err(), Ps2MapError::LedgerFaceFault);
        // 重复面 → 0x139A。
        let mut lb = LedgerBook::new();
        lb.add_face(LedgerFace { face: FaceId::Ps2, surveyed: 5, registered: 5 }).expect("首入合法");
        assert_eq!(
            lb.add_face(LedgerFace { face: FaceId::Ps2, surveyed: 5, registered: 5 }).unwrap_err(),
            Ps2MapError::LedgerFaceFault
        );
        // 零测绘面 → 0x139B。
        let mut lb = LedgerBook::new();
        assert_eq!(
            lb.add_face(LedgerFace { face: FaceId::Vga, surveyed: 0, registered: 0 }).unwrap_err(),
            Ps2MapError::LedgerIncomplete
        );
        // 虚报（登记>测绘）→ 0x139B。
        let mut lb = LedgerBook::new();
        assert_eq!(
            lb.add_face(LedgerFace { face: FaceId::Vga, surveyed: 3, registered: 5 }).unwrap_err(),
            Ps2MapError::LedgerIncomplete
        );
        // 缺登记面拉低完整率且 complete=false 不虚报。
        let mut lb = LedgerBook::new();
        lb.add_face(LedgerFace { face: FaceId::Serial, surveyed: 4, registered: 4 }).expect("serial 合法");
        lb.add_face(LedgerFace { face: FaceId::Vga, surveyed: 3, registered: 3 }).expect("vga 合法");
        lb.add_face(LedgerFace { face: FaceId::Ps2, surveyed: 5, registered: 4 }).expect("ps2 半账合法");
        let sum = lb.aggregate().expect("聚合可执行");
        assert!(!sum.complete);
        assert_eq!(sum.complete_permille, 11_000 / 12);
    }

    // ── 快照往返 + 指纹 + 篡改/损坏检出 ─────────────────────────────

    #[test]
    fn snapshot_roundtrip_and_fingerprint() {
        let mut book = build_reference_book();
        let snap = book.export_snapshot();
        assert!(snap.verify());
        // 确定性：同账重导出文本与指纹不变（指纹只含站点文本）。
        let snap2 = book.export_snapshot();
        assert_eq!(snap.text, snap2.text);
        assert_eq!(snap.fingerprint, snap2.fingerprint);
        // 往返导入：逐点逐字段一致。
        let back = import_snapshot(&snap.text).expect("往返必须成功");
        assert_eq!(back.site_count(), 5);
        assert_eq!(serialize_book_text(back.sites()), snap.text);
        let mut i = 0usize;
        while i < 5 {
            assert_eq!(back.sites()[i].to_spec(), SiteSpec::from_baseline(&BASELINE_SITES[i]));
            i += 1;
        }
        // 指纹核对。
        assert!(verify_snapshot_fingerprint(&snap.text, snap.fingerprint).is_ok());
        assert_eq!(
            verify_snapshot_fingerprint(&snap.text, snap.fingerprint.wrapping_add(1)).unwrap_err(),
            Ps2MapError::SnapshotFingerprint
        );
        // 空账快照往返。
        let empty_text = serialize_book_text(&[]);
        assert_eq!(empty_text, "UNX1283|V1|0|");
        let empty_book = import_snapshot(&empty_text).expect("空账快照合法");
        assert_eq!(empty_book.site_count(), 0);
    }

    #[test]
    fn snapshot_tamper_and_malformations_detected() {
        let text = serialize_book_text(build_reference_book().sites());
        // 篡改指纹必抓。
        let fp = fnv1a64(text.as_bytes());
        let tampered_kind = text.replace("|3|414|424|96|", "|7|414|424|96|"); // kind 3→7
        assert!(tampered_kind != text);
        assert!(verify_snapshot_fingerprint(&tampered_kind, fp).is_err());
        // 未知类导入 → 内容损坏（0x139C）。
        assert_eq!(
            import_snapshot(&tampered_kind).unwrap_err(),
            Ps2MapError::SnapshotMalformed
        );
        // 非规范数字（047）→ 往返重导出不一致（0x139D）。
        let noncanon = text.replace("|407|412|", "|047|412|");
        assert_eq!(
            import_snapshot(&noncanon).unwrap_err(),
            Ps2MapError::SnapshotFingerprint
        );
        // 头损坏。
        assert_eq!(
            import_snapshot("UNX1284|V1|0|").unwrap_err(),
            Ps2MapError::SnapshotMalformed
        );
        // 截断。
        let cut = &text[..text.len() - 6];
        assert_eq!(import_snapshot(cut).unwrap_err(), Ps2MapError::SnapshotMalformed);
        // 字段数不足。
        let short = format!("UNX1283|V1|1|{}|{}|1|1", "PS2-S001", "poll_key");
        assert_eq!(import_snapshot(&short).unwrap_err(), Ps2MapError::SnapshotMalformed);
        // 计数与实量不符（计数 4 < 实量 5，尾账未消费）。
        let over = format!("{}4|{}", "UNX1283|V1|", &text["UNX1283|V1|".len() + 2..]);
        assert_eq!(import_snapshot(&over).unwrap_err(), Ps2MapError::SnapshotMalformed);
    }

    // ── 双跑对拍 ─────────────────────────────────────────────────────

    #[test]
    fn double_run_deterministic() {
        let rep = double_run_compare().expect("双跑必须一致");
        assert_eq!(rep.site_count, 5);
        assert_eq!(rep.coverage_permille, 1000);
        assert_eq!(rep.event_count, 5); // 5 条 Registered
        let book = build_reference_book();
        let text = serialize_book_text(book.sites());
        assert_eq!(rep.fingerprint, fnv1a64(text.as_bytes()));
    }

    // ── 注入面 8 探针全检出 ──────────────────────────────────────────

    #[test]
    fn injection_audit_all_detected() {
        let probes = injection_audit();
        assert_eq!(probes.len(), 8);
        let expected = [
            ("INJ-MISSING-KIND", 0x1390u16),
            ("INJ-UNKNOWN-KIND", 0x1391),
            ("INJ-MISSING-SOURCE", 0x1392),
            ("INJ-MISSING-MAPPING", 0x1393),
            ("INJ-DUPLICATE-SITE", 0x1395),
            ("INJ-MAPPING-CONFLICT", 0x1397),
            ("INJ-BASELINE-GAP", 0x1399),
            ("INJ-GHOST-SITE", 0x1398),
        ];
        let mut i = 0usize;
        while i < probes.len() {
            assert_eq!(probes[i].probe_id, expected[i].0);
            assert_eq!(probes[i].expected_code, expected[i].1);
            assert!(probes[i].detected, "探针 {} 漏检", probes[i].probe_id);
            assert_eq!(probes[i].actual_code, expected[i].1);
            i += 1;
        }
        assert_eq!(injection_audit_strict().expect("审计必须全绿"), 8);
    }

    // ── 全自检 + 健康 ────────────────────────────────────────────────

    #[test]
    fn full_self_check_and_health_green() {
        let mut book = build_reference_book();
        let rep = book.full_self_check().expect("全自检必须绿");
        assert_eq!(rep.coverage_permille, 1000);
        assert_eq!(rep.registration_permille, 1000);
        assert_eq!(rep.takeover_rows_matched, 5);
        assert_eq!(rep.ledger_complete_permille, 1000);
        assert!(rep.snapshot_roundtrip);
        assert_eq!(rep.spin_worst_case_total, 2_200_066);
        let health = book.health();
        assert!(health.healthy);
        assert_eq!(health.coverage_permille, 1000);
        assert_eq!(health.registration_permille, 1000);
        assert!(health.ps2_face_complete);
        assert!(health.open_fault_codes.is_empty());
        assert!(health.event_count >= 5);
        // 事件账含勾稽/映射/封存/聚合各一。
        let kinds: Vec<EventKind> = book.events().iter().map(|e| e.kind).collect();
        assert!(kinds.contains(&EventKind::CoverageChecked));
        assert!(kinds.contains(&EventKind::MappingChecked));
        assert!(kinds.contains(&EventKind::SnapshotSealed));
        assert!(kinds.contains(&EventKind::LedgerAggregated));
        assert!(kinds.contains(&EventKind::SnapshotImported));
    }

    #[test]
    fn health_unhealthy_on_fault_events() {
        let mut b = Ps2SurveyBook::new();
        let _ = b.register_spec(spec_ok("PS2-S001", 1, 0x64, 0x01, 1));
        let mut s = spec_ok("PS2-S001", 1, 0x64, 0x01, 1);
        s.summary = String::new(); // 触发 0x1394 拒收
        let _ = b.register_spec(s);
        let health = b.health();
        assert!(!health.healthy);
        assert!(health.open_fault_codes.contains(&0x1394));
        // 缺登记 → 覆盖率非 1000，面不完整。
        assert_eq!(health.coverage_permille, 200);
        assert!(!health.ps2_face_complete);
    }

    // ── 事件账环形与饱和序号 ─────────────────────────────────────────

    #[test]
    fn event_ring_cap_and_seq() {
        let mut b = Ps2SurveyBook::new();
        let spec = SiteSpec::from_baseline(&BASELINE_SITES[0]);
        let _ = b.register_spec(spec.clone());
        let mut k = 0u32;
        while k < 300 {
            let _ = b.register_spec(spec.clone());
            k += 1;
        }
        assert_eq!(b.event_count(), EVENT_CAP as u32);
        assert_eq!(b.events().len(), 128);
        // 首条 Registered 已被挤出：最老一条应为 Rejected。
        assert_eq!(b.events()[0].kind, EventKind::Rejected);
        // 饱和序号连续：1 Registered + 300 Rejected = 301。
        assert_eq!(b.events().last().expect("非空").seq, 301);
    }
}
