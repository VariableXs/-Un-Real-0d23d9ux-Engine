//! unx0939_bootseq — UNX-F0939 启动期核间序列固化（BSP-AP 协同收口）。
//!
//! ═══════════════════════════════════════════════════════════════════
//! 锚点头（施工书锚点 UNX-F0939 · CoRun Varix STAR II · Unxreal）
//! ═══════════════════════════════════════════════════════════════════
//! 定位原文（docs/Varix/CoRun Varix STAR II · Unxreal/
//! CoRun Varix STAR II · Unxreal.md:2606，UNX-A2-B07 批·B07 中断面，
//! 全文照抄）：
//!
//! | UNX-F0939 | 启动期核间序列固化（BSP-AP 协同收口） | 320 | 已深化 |
//! | UNX-F0939-J1 启动序列账完整（BSP→AP 各步账回放），序列回归断言
//! | （现存 SMP 启动全绿零改动语义——账面挂接不碰时序），收口断言
//! | （全部 AP 在线确认 10/10 次） |
//!
//! 判据原文（J 判据全文照抄）：
//! **UNX-F0939-J1 启动序列账完整（BSP→AP 各步账回放），序列回归断言
//! （现存 SMP 启动全绿零改动语义——账面挂接不碰时序），收口断言
//! （全部 AP 在线确认 10/10 次）**
//!
//! 判据拆解（三成分逐条落面）：
//! ① 启动序列账完整——BSP→AP 七步全序（BSP 自检→INIT-SIPI→AP 等待
//!    哨兵→AP 自检→AP 特征采集→AP 装载→AP 在线确认），每步一笔账
//!    （时戳/核号/结论），账面回放（chain_replay）按账重演整序列并
//!    逐步对拍实账，回放一致 100%；
//! ② 序列回归断言——账面挂接模式：启动序列账只观测不改时序（挂接
//!    声明+零改动语义自证：观测器全部 API &self 或写独立账，挂接
//!    前后驱动 transcript 逐位全等机检）；"现存 SMP 启动全绿"由
//!    双跑确定性对拍+健康启动全链审计固守；
//! ③ 收口断言——全部 AP 在线确认：在线计数==AP 总数判据+逐 AP 确认
//!    行+10/10 次演练计数（不同种子 10 组健康启动全部收口闭合）。
//!
//! ═══════════════════════════════════════════════════════════════════
//! 职责定位（为什么这单必须立）
//! ═══════════════════════════════════════════════════════════════════
//! F0900~F0940 的 IPI 面全部立在"核已在线"的地基上；这片地基自己的
//! 账——**核是怎么一个个从睡到醒、从醒到在线的**——没有固化的账面。
//! x86 SMP 启动的铁序是死的：BSP 自检 → 写 INIT IPI → 写 SIPI（两次，
//! 间隔 10ms/200µs 量级）→ AP 在哨兵页自旋应答 → AP 各自跑自检/特征
//! 采集/镜像装载 → 逐 AP 在线确认。任何一步错序（先 SIPI 后 INIT、
//! 先装载后自检）都是不可恢复的死机；任何 AP 未应答被静默吞掉都是
//! "核数莫名少一"的幽灵病。Windows 深在哪：KiInitProcessor 的启动序
//! 列散在 HAL 与内核两处，无全序账，AP 失败时只留一个 BugCheck 码，
//! 无逐步账可回放。Varix：**序列账单源化**——七步全序 const 表+每步
//! 前置条件 const 判定+每步一笔账（时戳/核号/结论三件套），账面回放
//! 100% 对拍机检（判据①的本体）。
//!
//! 本单交付九面：
//! 1. `BootStep` 启动步闭集——七步全序 const 表+每步前置条件（const
//!    纯函数判定，错序在编译期就有位掩码判据可依）；
//! 2. `BootSequenceLedger` 启动序列账——每步：时戳/核号/结论，账面
//!    回放（chain_replay：按账重演整序列，逐步对拍实账，回放一致
//!    100%）；
//! 3. 序列回归断言——账面挂接模式（`BootObserver`）：只观测不改时
//!    序；零改动语义自证（挂接前后驱动 transcript 逐位全等）；
//! 4. 收口断言（`ClosureGate`）——在线计数==AP 总数+逐 AP 确认行+
//!    10/10 次演练计数；
//! 5. 超时与降级——每步超时预算（写死常量表，来源注释）+AP 未在线
//!    降级账（隔离不重试——与 F0347 语义对齐声明，API 层面禁重试）；
//! 6. 事件账（`EventLedger`）+`health()` 健康面（绿/黄/红+千分比
//!    通过率）；
//! 7. FNV-1a 指纹+双跑确定性对拍（同计划同种子两跑指纹全等）；
//! 8. 注入面三类各 10/10 检出——跳步注入（链审计检出步缺失）/AP 缺
//!    席注入（收口计数失配检出）/回放篡改注入（驱动重演逐字段对拍
//!    检出）；
//! 9. 全链自证聚合入口 `run_all_checks()`（ktest B07 桥接点）。
//!
//! 边界声明：本单是**启动序列的抽象状态机建模**（确定性仿真驱动），
//! 不碰真实 SIPIClient/INIT-SIPI 端口写——真实硬件路径属 SMP 启动域
//! 其他单；本单只固化"序"与"账"，给真实路径做回归挂接面。序列账只
//! 观测不改时序：观测器无副作用面（全部 API &self 或写独立账），
//! 挂接不碰时序由 transcript 全等机检背书。
//! 上游：F0347 降级语义（隔离不重试，本单对齐）、F0900 IPI 面总账
//! （本单是其地基账）；下游：F0940 ktest B07 断言集（本单聚合入口
//! 挂接）、SMP 启动真实路径（回归挂接面）。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect/panic!/运行期 assert!；
//! 零 IO 零墙钟（tick 显性注入：`BootTicker` trait，确定性 LCG 抖动，
//! Numerical Recipes 参数）；错误全走 `Result<T, BootSeqError>`，每
//! 变体带 `detail()` 中文可操作诊断与诊断码；溢出全显性
//! wrapping/saturating；>64KB 结构体（账/事件账/计划）全部堆载体，
//! 栈上只过句柄（size_of 编译期断言背书）；const fn 全部守纪（无
//! slice::get、无 Vec 索引、无 Ord::min/max、无 for 循环——while+
//! 索引，无 derive PartialEq 的 ==）。诊断码段 0x19B0~0x19BF 本单
//! 独占（全仓 grep 0x19B 确认空闲）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const BOOT_SEQ_VERSION: &str = "B07-bootseq-v1";

// ---------------------------------------------------------------------------
// §0 诊断码（0x19B0~0x19BF 段，本单独占——全仓 grep 0x19B 确认空闲）
// ---------------------------------------------------------------------------

/// 0x19B0 步序违例（实账步号倒退——链审计红线）。
pub const XC_STEP_OUT_OF_ORDER: u32 = 0x19B0;
/// 0x19B1 步缺失（账面跳步——跳步注入检出面）。
pub const XC_STEP_MISSING: u32 = 0x19B1;
/// 0x19B2 步超时（步耗时超出预算表——降级入口）。
pub const XC_STEP_BUDGET_TIMEOUT: u32 = 0x19B2;
/// 0x19B3 AP 未在线已隔离（降级账立案——不静默吞核）。
pub const XC_AP_OFFLINE_ISOLATED: u32 = 0x19B3;
/// 0x19B4 重试违例（隔离不重试红线——降级账出现非零重试计数）。
pub const XC_RETRY_FORBIDDEN: u32 = 0x19B4;
/// 0x19B5 收口未闭合（在线确认数 < AP 总数——收口断言红线）。
pub const XC_CLOSURE_INCOMPLETE: u32 = 0x19B5;
/// 0x19B6 回放对拍失配（驱动重演与实账逐字段不等——判据①红线）。
pub const XC_REPLAY_MISMATCH: u32 = 0x19B6;
/// 0x19B7 账面篡改检出（回放对拍抓到实账被改——注入面）。
pub const XC_TAMPER_DETECTED: u32 = 0x19B7;
/// 0x19B8 启动计划非法（AP 总数越界/缺席核越界/重复缺席）。
pub const XC_PLAN_INVALID: u32 = 0x19B8;
/// 0x19B9 空账（无记录可审计——审计前置红线）。
pub const XC_LEDGER_EMPTY: u32 = 0x19B9;
/// 0x19BA 注入未检出（演练红账：注入发生而账面无感——判据红线）。
pub const XC_INJECTION_UNDETECTED: u32 = 0x19BA;
/// 0x19BB 双跑指纹漂移（同计划同种子两跑指纹不等——确定性破约）。
pub const XC_FINGERPRINT_DRIFT: u32 = 0x19BB;
/// 0x19BC transcript 漂移（挂接前后驱动 transcript 不等——零改动语义破约）。
pub const XC_TRANSCRIPT_DRIFT: u32 = 0x19BC;
/// 0x19BD 健康红账（链违例等红线成立——health() 判红）。
pub const XC_HEALTH_RED: u32 = 0x19BD;
/// 0x19BA~0x19BF 段内：0x19BE AP 核号越界（缺席核不在 1..=AP 总数）。
pub const XC_AP_OUT_OF_RANGE: u32 = 0x19BE;
/// 0x19BF 账容量越限（事件账防御上限触发——立案不静默）。
pub const XC_LEDGER_OVERFLOW: u32 = 0x19BF;

/// 本单诊断码全表（段内独占+两两互异，编译期断言背书）。
pub const BOOT_SEQ_CODE_TABLE: [u32; 16] = [
    XC_STEP_OUT_OF_ORDER,
    XC_STEP_MISSING,
    XC_STEP_BUDGET_TIMEOUT,
    XC_AP_OFFLINE_ISOLATED,
    XC_RETRY_FORBIDDEN,
    XC_CLOSURE_INCOMPLETE,
    XC_REPLAY_MISMATCH,
    XC_TAMPER_DETECTED,
    XC_PLAN_INVALID,
    XC_LEDGER_EMPTY,
    XC_INJECTION_UNDETECTED,
    XC_FINGERPRINT_DRIFT,
    XC_TRANSCRIPT_DRIFT,
    XC_HEALTH_RED,
    XC_AP_OUT_OF_RANGE,
    XC_LEDGER_OVERFLOW,
];

const fn code_in_band(c: u32) -> bool {
    c >= 0x19B0 && c <= 0x19BF
}

/// 编译期断言：16 个诊断码全部落在 0x19B0~0x19BF 段内。
const _: () = assert!(
    code_in_band(XC_STEP_OUT_OF_ORDER)
        && code_in_band(XC_STEP_MISSING)
        && code_in_band(XC_STEP_BUDGET_TIMEOUT)
        && code_in_band(XC_AP_OFFLINE_ISOLATED)
        && code_in_band(XC_RETRY_FORBIDDEN)
        && code_in_band(XC_CLOSURE_INCOMPLETE)
        && code_in_band(XC_REPLAY_MISMATCH)
        && code_in_band(XC_TAMPER_DETECTED)
        && code_in_band(XC_PLAN_INVALID)
        && code_in_band(XC_LEDGER_EMPTY)
        && code_in_band(XC_INJECTION_UNDETECTED)
        && code_in_band(XC_FINGERPRINT_DRIFT)
        && code_in_band(XC_TRANSCRIPT_DRIFT)
        && code_in_band(XC_HEALTH_RED)
        && code_in_band(XC_AP_OUT_OF_RANGE)
        && code_in_band(XC_LEDGER_OVERFLOW),
);

/// 编译期断言：诊断码两两互异（while+索引，守 const fn 纪律）。
const fn codes_unique(t: [u32; 16]) -> bool {
    let mut i = 0usize;
    while i < 16 {
        let mut j = i + 1;
        while j < 16 {
            if t[i] == t[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

const _: () = assert!(codes_unique(BOOT_SEQ_CODE_TABLE));

// ---------------------------------------------------------------------------
// §1 FNV-1a 64 位指纹（账面指纹/双跑对拍/回放对拍共用）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基（FNV 参考实现标准常量，写死）。
pub const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数（FNV 参考实现标准常量，写死）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 逐字节搅拌（确定性指纹的最小原语）。
pub fn fnv_mix(mut acc: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        acc ^= bytes[i] as u64;
        acc = acc.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    acc
}

/// 常用标量搅拌糖：u64 按小端 8 字节入 FNV。
fn fnv_u64(acc: u64, v: u64) -> u64 {
    fnv_mix(acc, &v.to_le_bytes())
}

// ---------------------------------------------------------------------------
// §2 BootStep 启动步闭集（七步全序 const 表+每步前置条件）
// ---------------------------------------------------------------------------

/// 启动步总数（闭集，编译期锁定）。
pub const BOOT_STEP_COUNT: usize = 7;
/// AP 核总数上限（x86 常见平台量级上限，写死；来源：SMP 启动域
/// 哨兵页容量约定，本单仿真模型沿用）。
pub const MAX_AP: usize = 256;

/// 启动步闭集（BSP→AP 七步全序）。
///
/// 全序铁律（x86 多核启动协议的抽象化，来源：Intel SDM Vol.3 §8.4
/// "Multiple-Processor (MP) Initialization"——INIT-SIPI 排序时序，
/// 量级折算为 tick 预算见 [`STEP_BUDGET_TICKS`]）：
/// 步 0 BSP 自检 → 步 1 INIT IPI → 步 2 AP 哨兵等待 → 步 3 AP 自检
/// → 步 4 AP 特征采集 → 步 5 AP 装载 → 步 6 AP 在线确认。
/// 任何一步错序即死机不可恢复——本闭集把"序"做成编译期事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BootStep {
    /// 步 0：BSP 自检（引导核自量——时钟/中断控制器/哨兵页就绪）。
    BspSelfCheck = 0,
    /// 步 1：INIT IPI 下发（唤醒所有 AP 到实模式起点）。
    InitSipi = 1,
    /// 步 2：AP 等待哨兵（BSP 侧等 AP 在哨兵页应答计数齐）。
    ApSentinelWait = 2,
    /// 步 3：AP 自检（每个 AP 各自跑——页表/栈/GDT 就位自证）。
    ApSelfCheck = 3,
    /// 步 4：AP 特征采集（每 AP CPUID/拓扑/NUMA 采集入账）。
    ApFeatureProbe = 4,
    /// 步 5：AP 装载（每 AP 装载启动栈+任务入口）。
    ApImageLoad = 5,
    /// 步 6：AP 在线确认（每 AP 正式入在线集——收口断言的逐行本体）。
    ApOnlineConfirm = 6,
}

/// 七步全序 const 表（步号升序，闭集全量——判据①"全序"的编译期本体）。
pub const BOOT_STEP_ALL: [BootStep; BOOT_STEP_COUNT] = [
    BootStep::BspSelfCheck,
    BootStep::InitSipi,
    BootStep::ApSentinelWait,
    BootStep::ApSelfCheck,
    BootStep::ApFeatureProbe,
    BootStep::ApImageLoad,
    BootStep::ApOnlineConfirm,
];

impl BootStep {
    /// 步号（const 纯函数，discriminant 直取）。
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// 步名（const 纯函数，账面/诊断共用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            BootStep::BspSelfCheck => "BSP自检",
            BootStep::InitSipi => "INIT-SIPI",
            BootStep::ApSentinelWait => "AP等待哨兵",
            BootStep::ApSelfCheck => "AP自检",
            BootStep::ApFeatureProbe => "AP特征采集",
            BootStep::ApImageLoad => "AP装载",
            BootStep::ApOnlineConfirm => "AP在线确认",
        }
    }

    /// 执行角色（const 纯函数）：步 0~2 BSP 独跑，步 3~6 每 AP 各跑。
    pub const fn exec_role(self) -> BootRole {
        match self {
            BootStep::BspSelfCheck | BootStep::InitSipi | BootStep::ApSentinelWait => BootRole::Bsp,
            BootStep::ApSelfCheck
            | BootStep::ApFeatureProbe
            | BootStep::ApImageLoad
            | BootStep::ApOnlineConfirm => BootRole::Ap,
        }
    }

    /// 前置步号（const 纯函数）：本步开跑前必须已完成的步（-1 表示无）。
    /// 七步线性链：步 i 的前置恰为步 i-1（i>0）——错序即死机的铁律。
    pub const fn predecessor_index(self) -> i16 {
        match self {
            BootStep::BspSelfCheck => -1,
            BootStep::InitSipi => 0,
            BootStep::ApSentinelWait => 1,
            BootStep::ApSelfCheck => 2,
            BootStep::ApFeatureProbe => 3,
            BootStep::ApImageLoad => 4,
            BootStep::ApOnlineConfirm => 5,
        }
    }

    /// 每步前置条件判定（const 纯函数）：
    /// `completed` 为已完成步号位掩码（bit i = 步 i 已完成）。
    /// 判据：前置步已置位。步 0 无前置恒真。
    pub const fn precondition_satisfied(completed: u16, step: BootStep) -> bool {
        let p = step.predecessor_index();
        if p < 0 {
            return true;
        }
        let pi = p as u16;
        let bit = 1u16 << pi;
        (completed & bit) == bit
    }

    /// 步基础耗时（tick，const 纯函数）——仿真模型常量（量级对齐
    /// SDM §8.4.4 INIT-SIPI 排序经验值折算，非硬件实测，仅建模）。
    pub const fn base_ticks(self) -> u64 {
        STEP_BASE_TICKS[self.index() as usize]
    }

    /// 步超时预算（tick，const 纯函数）——超预算即降级（见 §7）。
    pub const fn budget_ticks(self) -> u64 {
        STEP_BUDGET_TICKS[self.index() as usize]
    }
}

/// 步基础耗时表（tick，量级来源：Intel SDM Vol.3 §8.4.4 INIT-SIPI
/// 排序时序（10ms/200µs 量级）折算为 tick 的建模常量，写死；非硬件
/// 实测值——本单是抽象状态机，量级只保序不保真）。
pub const STEP_BASE_TICKS: [u64; BOOT_STEP_COUNT] = [12, 60, 180, 24, 36, 90, 150];

/// 步超时预算表（tick，写死；预算>基础耗时×2 留抖动余量；来源同上
/// 建模折算——超预算即走降级账，隔离不重试）。
pub const STEP_BUDGET_TICKS: [u64; BOOT_STEP_COUNT] = [40, 200, 600, 80, 120, 300, 500];

/// 编译期断言：七步全序表长度锁定、步号闭集连续、前置链成立。
const _: () = assert!(BOOT_STEP_ALL.len() == BOOT_STEP_COUNT);
const _: () = assert!(STEP_BASE_TICKS.len() == BOOT_STEP_COUNT);
const _: () = assert!(STEP_BUDGET_TICKS.len() == BOOT_STEP_COUNT);
const _: () = assert!(
    BootStep::BspSelfCheck.index() == 0
        && BootStep::InitSipi.index() == 1
        && BootStep::ApSentinelWait.index() == 2
        && BootStep::ApSelfCheck.index() == 3
        && BootStep::ApFeatureProbe.index() == 4
        && BootStep::ApImageLoad.index() == 5
        && BootStep::ApOnlineConfirm.index() == 6,
);
/// 编译期断言：前置链线性（步 i 前置恰为 i-1）。
const _: () = assert!(
    BootStep::InitSipi.predecessor_index() == 0
        && BootStep::ApSentinelWait.predecessor_index() == 1
        && BootStep::ApSelfCheck.predecessor_index() == 2
        && BootStep::ApFeatureProbe.predecessor_index() == 3
        && BootStep::ApImageLoad.predecessor_index() == 4
        && BootStep::ApOnlineConfirm.predecessor_index() == 5,
);
/// 编译期断言：步结构体尺寸收口（栈上只过句柄——>64KB 结构体禁上栈
/// 的纪律背书：账/事件账/计划全是堆载体，这里锁的是栈过路单元）。
const _: () = assert!(core::mem::size_of::<BootStep>() <= 2);
const _: () = assert!(core::mem::size_of::<BootRole>() <= 1);
const _: () = assert!(core::mem::size_of::<StepOutcome>() <= 1);

// ---------------------------------------------------------------------------
// §3 角色/核描述子/结论（账面最小词表）
// ---------------------------------------------------------------------------

/// 启动角色（BSP 单核 vs AP 逐核）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootRole {
    /// 引导核（核号 0）。
    Bsp,
    /// 应用核（核号 1..=AP 总数）。
    Ap,
}

impl BootRole {
    /// 角色名（账面/诊断共用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            BootRole::Bsp => "BSP",
            BootRole::Ap => "AP",
        }
    }
}

/// 步结论（账面三件套之一）：过 / 超时降级 / 隔离离场。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome {
    /// 步过（预算内完成）。
    Pass,
    /// 步超时（超预算——走降级账）。
    Timeout,
    /// 核被隔离离场（AP 未在线——不再出现在后续步）。
    Isolated,
}

impl StepOutcome {
    /// 结论名（账面/诊断共用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            StepOutcome::Pass => "过",
            StepOutcome::Timeout => "超时",
            StepOutcome::Isolated => "隔离",
        }
    }

    /// 红账判定（const 纯函数）：非过即红（超时/隔离都进健康账）。
    pub const fn is_red(self) -> bool {
        match self {
            StepOutcome::Pass => false,
            StepOutcome::Timeout | StepOutcome::Isolated => true,
        }
    }
}

/// AP 核描述子（启动前由计划给出的预期 AP 集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApDescriptor {
    /// 核号（1..=AP 总数；0 保留给 BSP）。
    pub core: u16,
    /// APIC ID（建模：核号×16+1——确定性映射，仅仿真用）。
    pub apic_id: u32,
    /// 预期特征位（建模：bit0=x2APIC，bit1=x2APIC 禁用位——特征采集
    /// 步的账面预期值）。
    pub expect_features: u32,
}

impl ApDescriptor {
    /// 按核号构造描述子（确定性映射，建模约定）。
    pub const fn of(core: u16) -> Self {
        ApDescriptor {
            core,
            apic_id: (core as u32) * 16 + 1,
            expect_features: 0x3,
        }
    }
}

/// 单笔步账（时戳/核号/结论三件套+步号+诊断码+备注）。
///
/// 尺寸收口：heap 载体（Vec 元素），编译期断言 ≤128B——>64KB 结构体
/// 禁上栈的纪律在账面同样成立。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepRecord {
    /// 账内序号（单增，入账时由账分配）。
    pub seq: u32,
    /// 步号（0..=6，七步闭集）。
    pub step_idx: u8,
    /// 执行角色。
    pub role: BootRole,
    /// 核号（BSP=0；AP=1..=AP 总数）。
    pub core: u16,
    /// 步起始时戳（tick）。
    pub tick_start: u64,
    /// 步结束时戳（tick）。
    pub tick_end: u64,
    /// 结论。
    pub outcome: StepOutcome,
    /// 诊断码（Pass 为 0；Timeout 为 XC_STEP_BUDGET_TIMEOUT）。
    pub diag: u32,
    /// 备注（中文可读，per-AP 语义）。
    pub note: String,
}

impl StepRecord {
    /// 构造（tick 差用 wrapping 兜底——时戳回绕显性不 panic）。
    pub fn new(step: BootStep, role: BootRole, core: u16, tick_start: u64, tick_end: u64, outcome: StepOutcome, diag: u32, note: String) -> Self {
        StepRecord {
            seq: 0,
            step_idx: step.index(),
            role,
            core,
            tick_start,
            tick_end,
            outcome,
            diag,
            note,
        }
    }

    /// 步耗时（wrapping 兜底）。
    pub fn elapsed(&self) -> u64 {
        self.tick_end.wrapping_sub(self.tick_start)
    }

    /// 一行可读账（账面回放/对拍证据共用）。
    pub fn line(&self) -> String {
        format!(
            "[{:03}] 步{}({}) {}核{} tick={}..={} diag={:#06x} {}",
            self.seq,
            self.step_idx,
            step_name(self.step_idx),
            self.role.as_str(),
            self.core,
            self.tick_start,
            self.tick_end,
            self.diag,
            self.note,
        )
    }
}

/// 步号→步名（账面回放/证据行共用——账里只存步号，展示时还原名）。
pub const fn step_name(idx: u8) -> &'static str {
    match idx {
        0 => BootStep::BspSelfCheck.as_str(),
        1 => BootStep::InitSipi.as_str(),
        2 => BootStep::ApSentinelWait.as_str(),
        3 => BootStep::ApSelfCheck.as_str(),
        4 => BootStep::ApFeatureProbe.as_str(),
        5 => BootStep::ApImageLoad.as_str(),
        6 => BootStep::ApOnlineConfirm.as_str(),
        _ => "未知步",
    }
}

impl StepRecord {
}

// ---------------------------------------------------------------------------
// §4 确定性时钟与抖动（零 IO 零墙钟——tick 显性注入）
// ---------------------------------------------------------------------------

/// 时钟端口（tick 显性注入——本单不碰墙钟不碰 IO）。
pub trait BootTicker {
    /// 当前 tick。
    fn now(&self) -> u64;
    /// 推进 tick（显性：只有驱动显式推进，时序才动）。
    fn advance(&mut self, delta: u64);
}

/// 确定性 LCG 时钟（Numerical Recipes 参数——同种子同序列，
/// 双跑对拍的确定性底座）。
#[derive(Debug, Clone)]
pub struct LcgClock {
    state: u64,
    ticks: u64,
}

impl LcgClock {
    /// 构造（状态与 tick 分离：state 供抖动，ticks 供时序）。
    pub const fn new(seed: u64) -> Self {
        LcgClock {
            state: seed ^ 0x9E37_79B9_7F4A_7C15, // 黄金比例常数（Knuth），写死
            ticks: 0,
        }
    }

    /// 下一随机数（wrapping——LCG 溢出即语义）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364_1362_2384_6793_005) // NR LCG 乘子，写死
            .wrapping_add(1442_6950_4088_8896_3407); // NR LCG 增量，写死
        self.state
    }

    /// 有界抖动（0..bound；bound=0 恒 0）。
    pub fn jitter(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            0
        } else {
            self.next_u64() % bound
        }
    }
}

impl BootTicker for LcgClock {
    fn now(&self) -> u64 {
        self.ticks
    }
    fn advance(&mut self, delta: u64) {
        self.ticks = self.ticks.wrapping_add(delta);
    }
}

// ---------------------------------------------------------------------------
// §5 BootSequenceLedger 启动序列账（每步：时戳/核号/结论三件套）
// ---------------------------------------------------------------------------

/// 事件种类闭集（事件账词表——观测器/审计/演练共用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// 步观测（观测器挂接写入——独立账，不碰实账）。
    StepObserved,
    /// AP 在线确认行（收口断言逐行本体）。
    ApOnline,
    /// AP 隔离立案（未在线/超时——降级账联动）。
    ApIsolated,
    /// 收口评估结论。
    ClosureAssessed,
    /// 注入检出（演练红/绿账）。
    InjectionFound,
    /// 回放完成（chain_replay / driver replay）。
    ReplayDone,
    /// 零改动语义自证完成。
    InertnessProven,
    /// 双跑确定性对拍完成。
    DualRunProven,
    /// 链审计完成。
    AuditDone,
}

impl EventKind {
    /// 事件名（账面/诊断共用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            EventKind::StepObserved => "步观测",
            EventKind::ApOnline => "AP在线",
            EventKind::ApIsolated => "AP隔离",
            EventKind::ClosureAssessed => "收口评估",
            EventKind::InjectionFound => "注入检出",
            EventKind::ReplayDone => "回放完成",
            EventKind::InertnessProven => "零改动自证",
            EventKind::DualRunProven => "双跑对拍",
            EventKind::AuditDone => "链审计",
        }
    }
}

/// 单笔事件账（事件账最小单元）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRecord {
    /// 账内序号（单增，入账时由账分配）。
    pub seq: u32,
    /// 时戳（tick）。
    pub tick: u64,
    /// 事件种类。
    pub kind: EventKind,
    /// 诊断码（0=无异常）。
    pub diag: u32,
    /// 摘要（中文可读）。
    pub summary: String,
}

impl EventRecord {
    /// 构造。
    pub fn new(tick: u64, kind: EventKind, diag: u32, summary: String) -> Self {
        EventRecord {
            seq: 0,
            tick,
            kind,
            diag,
            summary,
        }
    }
}

/// 事件账（观测器独立账/实账联动事件共用载体）。
///
/// 容量防御：超上限不静默丢弃——overflow 计数+报错（`XC_LEDGER_OVERFLOW`）。
#[derive(Debug, Clone)]
pub struct EventLedger {
    events: Vec<EventRecord>,
    overflow: usize,
}

/// 事件账容量上限（防御常量，写死；正常全链 <300 笔，10 倍余量）。
pub const EVENT_LEDGER_CAP: usize = 4096;

impl EventLedger {
    /// 空账构造。
    pub fn new() -> Self {
        EventLedger {
            events: Vec::new(),
            overflow: 0,
        }
    }

    /// 入账（容量防御：越限立案不静默）。
    pub fn push(&mut self, mut rec: EventRecord) -> Result<(), BootSeqError> {
        if self.events.len() >= EVENT_LEDGER_CAP {
            self.overflow = self.overflow.saturating_add(1);
            return Err(BootSeqError::LedgerOverflow {
                code: XC_LEDGER_OVERFLOW,
            });
        }
        rec.seq = self.events.len() as u32;
        self.events.push(rec);
        Ok(())
    }

    /// 账面只读视图。
    pub fn events(&self) -> &[EventRecord] {
        &self.events
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 溢出计数（越限立案的留痕）。
    pub fn overflow(&self) -> usize {
        self.overflow
    }

    /// 指纹（FNV-1a——双跑/回放对拍共用）。
    pub fn fingerprint(&self) -> u64 {
        let mut acc = FNV_OFFSET_BASIS;
        for e in &self.events {
            acc = fnv_u64(acc, e.seq as u64);
            acc = fnv_u64(acc, e.tick);
            acc = fnv_u64(acc, e.kind as u64);
            acc = fnv_u64(acc, e.diag as u64);
            acc = fnv_mix(acc, e.summary.as_bytes());
        }
        acc = fnv_u64(acc, self.overflow as u64);
        acc
    }
}

/// 降级账单笔（隔离不重试的逐案留痕）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DegradeRecord {
    /// 核号。
    pub core: u16,
    /// 立案时戳（tick）。
    pub tick: u64,
    /// 立案原因（`'static` 中文短语——常量见 [`DEGRADE_REASON_SENTINEL`] 等）。
    pub reason: &'static str,
    /// 重试计数（**恒 0**——隔离不重试红线；非零入账直接被拒）。
    pub retry_attempt: u32,
    /// 诊断码。
    pub diag: u32,
}

/// 降级原因：INIT-SIPI 后哨兵未应答（对齐 F0347：隔离不重试）。
pub const DEGRADE_REASON_SENTINEL: &str = "INIT-SIPI后哨兵未应答——隔离不重试（对齐F0347）";
/// 降级原因：步超时预算超标（对齐 F0347：隔离不重试）。
pub const DEGRADE_REASON_TIMEOUT: &str = "步超时预算超标——隔离不重试（对齐F0347）";

/// F0347 语义对齐声明（冻结文本——判据"隔离不重试——与 F0347 语义
/// 对齐声明"的本体）。
pub const F0347_ALIGNMENT_DECL: &str = "本单 AP 未在线降级语义与 UNX-F0347 对齐：一律隔离离场（isolated），\
重试计数恒 0，API 层面无重试入口（push_degrade 拒绝非零 retry_attempt），\
降级逐案留痕（核号/时戳/原因/重试计数四件套），不静默吞核。";

/// 启动序列账本体（判据①"启动序列账完整"的载体）。
///
/// 三层账：步账（records：每步 时戳/核号/结论）+ 降级账（degrades：
/// 隔离逐案）+ 事件账（events：观测/审计/演练联动）。>64KB 结构体
/// 禁上栈：本体只有堆句柄（三个 Vec+一个 String），编译期断言背书。
#[derive(Debug, Clone)]
pub struct BootSequenceLedger {
    /// 计划标签（ap_total/seed/注入摘要——账的溯源头）。
    plan_tag: String,
    /// AP 总数（预期收口基线）。
    ap_total: u16,
    /// 步账（时序严格——入账即校验步号单调）。
    records: Vec<StepRecord>,
    /// 降级账（隔离逐案，重试计数恒 0）。
    degrades: Vec<DegradeRecord>,
    /// 事件账（驱动侧联动事件）。
    events: EventLedger,
}

impl BootSequenceLedger {
    /// 空账构造（校验 AP 总数基线）。
    pub fn new(ap_total: u16, plan_tag: &str) -> Result<Self, BootSeqError> {
        if ap_total == 0 || ap_total as usize > MAX_AP {
            return Err(BootSeqError::PlanInvalid {
                code: XC_PLAN_INVALID,
                reason: format!(
                    "AP 总数 {} 越界（合法 1..={}）：修正启动计划再入账",
                    ap_total, MAX_AP
                ),
            });
        }
        Ok(BootSequenceLedger {
            plan_tag: plan_tag.to_string(),
            ap_total,
            records: Vec::new(),
            degrades: Vec::new(),
            events: EventLedger::new(),
        })
    }

    /// 计划标签。
    pub fn plan_tag(&self) -> &str {
        &self.plan_tag
    }

    /// AP 总数（收口基线）。
    pub fn ap_total(&self) -> u16 {
        self.ap_total
    }

    /// 步账只读视图。
    pub fn records(&self) -> &[StepRecord] {
        &self.records
    }

    /// 降级账只读视图。
    pub fn degrades(&self) -> &[DegradeRecord] {
        &self.degrades
    }

    /// 事件账只读视图。
    pub fn events(&self) -> &EventLedger {
        &self.events
    }

    /// 步账长。
    pub fn records_len(&self) -> usize {
        self.records.len()
    }

    /// 步账可变视图——**仅供回放篡改注入演练**（生产路径禁用：
    /// 篡改必被驱动重演逐字段对拍抓出，见 §11）。
    pub fn records_mut_tamper_surface(&mut self) -> &mut [StepRecord] {
        &mut self.records
    }

    /// 入一笔步账（序号由账分配；步号单调校验——驱动侧防御账）。
    pub fn push_record(&mut self, mut rec: StepRecord) -> Result<(), BootSeqError> {
        if rec.step_idx as usize >= BOOT_STEP_COUNT {
            return Err(BootSeqError::OrderViolation {
                code: XC_STEP_OUT_OF_ORDER,
                reason: format!(
                    "步号 {} 越出七步闭集（合法 0..={}）：检查驱动步表",
                    rec.step_idx,
                    BOOT_STEP_COUNT - 1
                ),
            });
        }
        let expect_seq = self.records.len() as u32;
        if let Some(last) = self.records.last() {
            if rec.step_idx < last.step_idx {
                self.events.push(EventRecord::new(
                    rec.tick_end,
                    EventKind::AuditDone,
                    XC_STEP_OUT_OF_ORDER,
                    format!(
                        "步序倒退：步号 {} 小于已入账步号 {}（核 {}）",
                        rec.step_idx, last.step_idx, rec.core
                    ),
                ))?;
                return Err(BootSeqError::OrderViolation {
                    code: XC_STEP_OUT_OF_ORDER,
                    reason: format!(
                        "步号 {} 倒退（已入账至 {}）：七步全序被破坏，检查驱动步表",
                        rec.step_idx, last.step_idx
                    ),
                });
            }
        }
        rec.seq = expect_seq;
        self.records.push(rec);
        Ok(())
    }

    /// 入一笔降级账（**隔离不重试红线**：非零重试计数直接拒收——
    /// F0347 语义对齐的 API 级执法）。
    pub fn push_degrade(&mut self, rec: DegradeRecord) -> Result<(), BootSeqError> {
        if rec.retry_attempt != 0 {
            return Err(BootSeqError::RetryViolation {
                code: XC_RETRY_FORBIDDEN,
                reason: format!(
                    "核 {} 降级账出现非零重试计数 {}：隔离不重试（F0347），禁止重试入口",
                    rec.core, rec.retry_attempt
                ),
            });
        }
        self.events.push(EventRecord::new(
            rec.tick,
            EventKind::ApIsolated,
            rec.diag,
            format!("核 {} 隔离立案：{}", rec.core, rec.reason),
        ))?;
        self.degrades.push(rec);
        Ok(())
    }

    /// 入一笔事件账。
    pub fn push_event(&mut self, rec: EventRecord) -> Result<(), BootSeqError> {
        self.events.push(rec)
    }

    /// 账指纹（FNV-1a：步账+降级账+事件账+计划头全覆盖——双跑/
    /// 回放对拍共用）。
    pub fn fingerprint(&self) -> u64 {
        let mut acc = FNV_OFFSET_BASIS;
        acc = fnv_mix(acc, self.plan_tag.as_bytes());
        acc = fnv_u64(acc, self.ap_total as u64);
        for r in &self.records {
            acc = fnv_u64(acc, r.seq as u64);
            acc = fnv_u64(acc, r.step_idx as u64);
            acc = fnv_u64(acc, r.role as u64);
            acc = fnv_u64(acc, r.core as u64);
            acc = fnv_u64(acc, r.tick_start);
            acc = fnv_u64(acc, r.tick_end);
            acc = fnv_u64(acc, r.outcome as u64);
            acc = fnv_u64(acc, r.diag as u64);
            acc = fnv_mix(acc, r.note.as_bytes());
        }
        for d in &self.degrades {
            acc = fnv_u64(acc, d.core as u64);
            acc = fnv_u64(acc, d.tick);
            acc = fnv_mix(acc, d.reason.as_bytes());
            acc = fnv_u64(acc, d.retry_attempt as u64);
            acc = fnv_u64(acc, d.diag as u64);
        }
        acc = fnv_u64(acc, self.events.fingerprint());
        acc
    }

    /// 降级账审计（隔离不重试红线机检：全部重试计数==0）。
    pub fn degrade_retry_audit(&self) -> Result<usize, BootSeqError> {
        for d in &self.degrades {
            if d.retry_attempt != 0 {
                return Err(BootSeqError::RetryViolation {
                    code: XC_RETRY_FORBIDDEN,
                    reason: format!(
                        "核 {} 降级案重试计数 {} 非零：红线违例，清查降级写入路径",
                        d.core, d.retry_attempt
                    ),
                });
            }
        }
        Ok(self.degrades.len())
    }
}

// ---------------------------------------------------------------------------
// §6 链审计（序列回归断言的账面半边：步序单调+七步闭集完整）
// ---------------------------------------------------------------------------

/// 链审计报告（步序违例/步缺失/红账行计数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainAuditReport {
    /// 审计步账条数。
    pub records_total: usize,
    /// 已见步号位掩码（bit i = 步 i 有账）。
    pub steps_seen_mask: u16,
    /// 违例行（步序倒退/步缺失，中文证据）。
    pub violations: Vec<String>,
    /// 红账行数（结论非"过"的步账）。
    pub red_count: usize,
    /// 七步全序完整（七步皆有账且零违例）。
    pub complete_chain: bool,
}

impl ChainAuditReport {
    /// 是否零违例。
    pub fn clean(&self) -> bool {
        self.violations.is_empty()
    }
}

impl BootSequenceLedger {
    /// 链审计（判据②"序列回归断言"的账面半边）：
    /// 逐条扫步账——步号必须单调不减；跨步必须恰好+1（跳步即步缺失
    /// 立案）；末步必须达步 6（七步闭集完整）；红账行逐条计数。
    pub fn chain_audit(&self) -> ChainAuditReport {
        let mut mask: u16 = 0;
        let mut cur: i16 = -1;
        let mut violations: Vec<String> = Vec::new();
        let mut red_count: usize = 0;
        let mut i = 0usize;
        while i < self.records.len() {
            let r = &self.records[i];
            if r.outcome.is_red() {
                red_count = red_count.saturating_add(1);
            }
            let s = r.step_idx as i16;
            if s == cur {
                // 同步多行（per-AP 逐核账）——合法。
            } else if s == cur + 1 {
                cur = s;
                mask |= 1u16 << (s as u16);
            } else if s > cur + 1 {
                violations.push(format!(
                    "步缺失：账序 {} 处步号 {} 直接跳过步号 {}（跳步注入检出面，diag={:#06x}）",
                    r.seq, s, cur + 1, XC_STEP_MISSING
                ));
                cur = s;
                mask |= 1u16 << (s as u16);
            } else {
                violations.push(format!(
                    "步序倒退：账序 {} 步号 {} 小于已推进步号 {}（diag={:#06x}）",
                    r.seq, s, cur, XC_STEP_OUT_OF_ORDER
                ));
            }
            i += 1;
        }
        let complete_chain = violations.is_empty()
            && cur == (BOOT_STEP_COUNT as i16 - 1)
            && self.records_len() > 0;
        ChainAuditReport {
            records_total: self.records.len(),
            steps_seen_mask: mask,
            violations,
            red_count,
            complete_chain,
        }
    }
}

// ---------------------------------------------------------------------------
// §7 超时与降级（步超时预算+AP 未在线降级账——隔离不重试）
// ---------------------------------------------------------------------------

/// 超时判定（建模核心）：步耗时（wrapping 差）超预算表即超时。
/// 预算表见 [`STEP_BUDGET_TICKS`]（来源注释在表处）。
pub const fn step_timeout(elapsed: u64, step: BootStep) -> bool {
    elapsed > step.budget_ticks()
}

// ---------------------------------------------------------------------------
// §8 BootDriver 启动驱动（确定性仿真——抽象状态机，不碰真实 SIPIClient）
// ---------------------------------------------------------------------------

/// 启动计划（驱动的全部输入——双跑/回放/演练的确定性底座）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootPlan {
    /// AP 总数（1..=256）。
    pub ap_total: u16,
    /// 确定性种子（LCG 抖动源——同种子同时序）。
    pub seed: u64,
    /// 缺席核集（AP 缺席注入面：这些核全程不应答）。
    pub absent_cores: Vec<u16>,
    /// 跳步注入面（该步不产生账——链审计应检出步缺失）。
    pub skip_step: Option<BootStep>,
    /// 超时注入面（该步追加耗时——预算应判超时）。
    pub slow_step: Option<BootStep>,
    /// 超时注入追加耗时（tick）。
    pub slow_by: u64,
}

impl BootPlan {
    /// 健康计划（无注入——收口演练/回归基线用）。
    pub fn healthy(ap_total: u16, seed: u64) -> Self {
        BootPlan {
            ap_total,
            seed,
            absent_cores: Vec::new(),
            skip_step: None,
            slow_step: None,
            slow_by: 0,
        }
    }

    /// 计划标签（账面溯源头）。
    pub fn tag(&self) -> String {
        format!(
            "bootseq ap={} seed={:#x} absent={:?} skip={:?} slow={:?}+{}",
            self.ap_total, self.seed, self.absent_cores, self.skip_step, self.slow_step, self.slow_by
        )
    }

    /// 校验（AP 总数/缺席核范围/重复缺席）。
    pub fn validate(&self) -> Result<(), BootSeqError> {
        if self.ap_total == 0 || self.ap_total as usize > MAX_AP {
            return Err(BootSeqError::PlanInvalid {
                code: XC_PLAN_INVALID,
                reason: format!(
                    "AP 总数 {} 越界（合法 1..={}）：修正启动计划",
                    self.ap_total, MAX_AP
                ),
            });
        }
        let mut i = 0usize;
        while i < self.absent_cores.len() {
            let c = self.absent_cores[i];
            if c == 0 || c > self.ap_total {
                return Err(BootSeqError::ApOutOfRange {
                    code: XC_AP_OUT_OF_RANGE,
                    reason: format!(
                        "缺席核 {} 越界（合法 1..={}）：核 0 是 BSP，缺席只对 AP 有意义",
                        c, self.ap_total
                    ),
                });
            }
            let mut j = i + 1;
            while j < self.absent_cores.len() {
                if self.absent_cores[j] == c {
                    return Err(BootSeqError::PlanInvalid {
                        code: XC_PLAN_INVALID,
                        reason: format!("缺席核 {} 重复登记：去重后重提计划", c),
                    });
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 是否缺席核（查表——线性扫，缺席集极小）。
    pub fn is_absent(&self, core: u16) -> bool {
        let mut i = 0usize;
        while i < self.absent_cores.len() {
            if self.absent_cores[i] == core {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 驱动 transcript（驱动可见发射序列——零改动语义自证的对拍本体）。
///
/// 每条打包：bit24..31=步号；bit8..23=核号；bit0..7=结论；
/// 全 1 表示"发射被抑制"（跳步/隔离离场的显性标记）。
/// 挂接观测器前后 transcript 逐位全等 = 挂接不碰时序的机检证据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverTranscript {
    entries: Vec<u32>,
}

/// transcript 抑制标记（跳步/隔离离场）。
pub const TRANSCRIPT_SUPPRESSED: u32 = 0xFFFF_FFFF;

impl DriverTranscript {
    /// 空 transcript。
    pub fn new() -> Self {
        DriverTranscript {
            entries: Vec::new(),
        }
    }

    /// 入一条（打包语义见类型注释）。
    pub fn push(&mut self, step_idx: u8, core: u16, outcome: StepOutcome) {
        let packed = ((step_idx as u32) << 24) | ((core as u32) << 8) | (outcome as u32);
        self.entries.push(packed);
    }

    /// 入一条抑制标记。
    pub fn push_suppressed(&mut self) {
        self.entries.push(TRANSCRIPT_SUPPRESSED);
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 只读视图。
    pub fn entries(&self) -> &[u32] {
        &self.entries
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut acc = FNV_OFFSET_BASIS;
        for &e in &self.entries {
            acc = fnv_u64(acc, e as u64);
        }
        acc
    }
}

/// 观测分发型辅助（可变借用而非移动 `observer`——驱动循环内多处
/// 观测点共用一个 `Option<(&BootObserver, &mut EventLedger)>`；
/// 方法调用/参数传递沿 &mut 链自动降引用）。
fn obs_step(observer: &mut Option<(&BootObserver, &mut EventLedger)>, rec: &StepRecord) {
    if let Some((o, s)) = observer {
        o.observe_step(rec, s);
    }
}

/// 观测分发型辅助（隔离立案）。
fn obs_isolated(observer: &mut Option<(&BootObserver, &mut EventLedger)>, core: u16, tick: u64) {
    if let Some((o, s)) = observer {
        o.observe_isolated(core, tick, s);
    }
}

/// 启动驱动（确定性仿真状态机）。
///
/// 职责：按七步全序跑一遍抽象启动序列，产（实账， transcript）。
/// 不碰真实 SIPIClient/INIT-SIPI 端口——时序全由注入的 [`BootTicker`]
/// 推进，抖动由计划种子的 LCG 决定（同计划同种子⇒同账同 transcript，
/// 双跑对拍的确定性底座）。
#[derive(Debug, Clone)]
pub struct BootDriver {
    plan: BootPlan,
}

impl BootDriver {
    /// 构造（计划先过校验）。
    pub fn new(plan: BootPlan) -> Result<Self, BootSeqError> {
        plan.validate()?;
        Ok(BootDriver { plan })
    }

    /// 计划只读视图。
    pub fn plan(&self) -> &BootPlan {
        &self.plan
    }

    /// 跑一遍启动序列（无观测器挂接——等价 `run_observed(.., None)`）。
    pub fn run<T: BootTicker>(
        &self,
        ticker: &mut T,
    ) -> Result<(BootSequenceLedger, DriverTranscript), BootSeqError> {
        self.run_observed(ticker, None)
    }

    /// 跑一遍启动序列（可选挂接观测器）。
    ///
    /// 挂接纪律（零改动语义）：观测器只读步账+写独立事件账，驱动
    /// 主流程（计划/时钟/步序/结论判定）一个字节不动——transcript
    /// 全等机检见 §9 `observer_inertness_proof`。
    pub fn run_observed<T: BootTicker>(
        &self,
        ticker: &mut T,
        mut observer: Option<(&BootObserver, &mut EventLedger)>,
    ) -> Result<(BootSequenceLedger, DriverTranscript), BootSeqError> {
        let mut rng = LcgClock::new(self.plan.seed);
        let mut ledger = BootSequenceLedger::new(self.plan.ap_total, &self.plan.tag())?;
        let mut transcript = DriverTranscript::new();
        // 隔离位图（栈上 256B 句柄级小物——核一旦隔离离场不再入后续步）。
        let mut isolated = [false; MAX_AP];

        // 缺席核：INIT-SIPI 后哨兵不应答——步 2 立案隔离（不重试）。
        // 先置位，步 2 入账时逐核立案。
        let mut ai = 0usize;
        while ai < self.plan.absent_cores.len() {
            let c = self.plan.absent_cores[ai] as usize;
            if c < MAX_AP {
                isolated[c] = true;
            }
            ai += 1;
        }

        let mut si = 0usize;
        while si < BOOT_STEP_COUNT {
            let step = BOOT_STEP_ALL[si];

            // 跳步注入：该步不产生任何账（链审计应检出步缺失）。
            if self.plan.skip_step == Some(step) {
                transcript.push_suppressed();
                ledger.push_event(EventRecord::new(
                    ticker.now(),
                    EventKind::InjectionFound,
                    XC_STEP_MISSING,
                    format!("步 {}（{}）被注入跳过——账面无该步（链审计检出面）", step.index(), step.as_str()),
                ))?;
                si += 1;
                continue;
            }

            // 超时注入追加耗时。
            let mut extra: u64 = 0;
            if self.plan.slow_step == Some(step) {
                extra = self.plan.slow_by;
            }

            match step.exec_role() {
                BootRole::Bsp => {
                    // BSP 三步：单核单账。
                    let t0 = ticker.now();
                    let cost = step
                        .base_ticks()
                        .wrapping_add(rng.jitter(8))
                        .wrapping_add(extra);
                    ticker.advance(cost);
                    let t1 = ticker.now();
                    let elapsed = t1.wrapping_sub(t0);
                    let (outcome, diag) = if step_timeout(elapsed, step) {
                        (StepOutcome::Timeout, XC_STEP_BUDGET_TIMEOUT)
                    } else {
                        (StepOutcome::Pass, 0)
                    };
                    let rec = StepRecord::new(
                        step,
                        BootRole::Bsp,
                        0,
                        t0,
                        t1,
                        outcome,
                        diag,
                        format!("BSP 侧 {} 一笔账", step.as_str()),
                    );
                    ledger.push_record(rec.clone())?;
                    transcript.push(step.index(), 0, outcome);
                    obs_step(&mut observer, &rec);
                    // 步 2（AP 等待哨兵）：缺席核在此立案隔离。
                    if step == BootStep::ApSentinelWait {
                        let mut ai = 0usize;
                        while ai < self.plan.absent_cores.len() {
                            let c = self.plan.absent_cores[ai];
                            let drec = DegradeRecord {
                                core: c,
                                tick: t1,
                                reason: DEGRADE_REASON_SENTINEL,
                                retry_attempt: 0,
                                diag: XC_AP_OFFLINE_ISOLATED,
                            };
                            ledger.push_degrade(drec)?;
                            transcript.push_suppressed();
                            obs_isolated(&mut observer, c, t1);
                            ai += 1;
                        }
                    }
                }
                BootRole::Ap => {
                    // AP 四步：逐核单账（核号升序——确定性遍历序）。
                    let mut core = 1u16;
                    while core <= self.plan.ap_total {
                        if isolated[core as usize] {
                            transcript.push_suppressed();
                            core = core.saturating_add(1);
                            continue;
                        }
                        let t0 = ticker.now();
                        let cost = step
                            .base_ticks()
                            .wrapping_add(rng.jitter(8))
                            .wrapping_add(extra);
                        ticker.advance(cost);
                        let t1 = ticker.now();
                        let elapsed = t1.wrapping_sub(t0);
                        let (outcome, diag) = if step_timeout(elapsed, step) {
                            (StepOutcome::Timeout, XC_STEP_BUDGET_TIMEOUT)
                        } else {
                            (StepOutcome::Pass, 0)
                        };
                        let rec = StepRecord::new(
                            step,
                            BootRole::Ap,
                            core,
                            t0,
                            t1,
                            outcome,
                            diag,
                            format!(
                                "AP 核 {} {}（apic={}）",
                                core,
                                step.as_str(),
                                ApDescriptor::of(core).apic_id
                            ),
                        );
                        ledger.push_record(rec.clone())?;
                        transcript.push(step.index(), core, outcome);
                        obs_step(&mut observer, &rec);
                        if outcome == StepOutcome::Timeout {
                            // 超时降级：隔离离场，不重试（F0347 对齐）。
                            isolated[core as usize] = true;
                            ledger.push_degrade(DegradeRecord {
                                core,
                                tick: t1,
                                reason: DEGRADE_REASON_TIMEOUT,
                                retry_attempt: 0,
                                diag: XC_STEP_BUDGET_TIMEOUT,
                            })?;
                            transcript.push_suppressed();
                            obs_isolated(&mut observer, core, t1);
                        }
                        if outcome == StepOutcome::Pass && step == BootStep::ApOnlineConfirm {
                            ledger.push_event(EventRecord::new(
                                t1,
                                EventKind::ApOnline,
                                0,
                                format!("核 {} 在线确认（收口逐行）", core),
                            ))?;
                        }
                        core = core.saturating_add(1);
                    }
                }
            }
            si += 1;
        }
        Ok((ledger, transcript))
    }
}

// ---------------------------------------------------------------------------
// §9 账面挂接（BootObserver——序列回归断言：只观测不改时序）
// ---------------------------------------------------------------------------

/// 观测器零改动语义契约（冻结文本——判据②"账面挂接不碰时序"的
/// 声明本体）。
pub const OBSERVER_ZERO_SIDE_EFFECT_DECL: &str = "BootObserver 全部 API 以 &self 收口，无任何可变状态；\
观测输出只进调用方提供的独立 EventLedger（独立账），不改实账、不改时钟、不改步序、不改结论——\
挂接前后驱动 transcript 逐位全等（observer_inertness_proof 机检背书）。";

/// 账面挂接观测器（序列回归断言的挂接半边）。
///
/// 只观测：步账逐笔入独立事件账、隔离逐案入独立事件账、收口结论
/// 入独立事件账。全部 API `&self`——观测器自身无副作用面。
#[derive(Debug, Clone, Copy)]
pub struct BootObserver {
    name: &'static str,
}

impl BootObserver {
    /// 构造（const——挂接点可静态固化为 &'static 常量）。
    pub const fn new(name: &'static str) -> Self {
        BootObserver { name }
    }

    /// 观测器名。
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// 观测一笔步账（写独立账——不碰实账不碰驱动）。
    pub fn observe_step(&self, rec: &StepRecord, sink: &mut EventLedger) {
        let _ = sink.push(EventRecord::new(
            rec.tick_end,
            EventKind::StepObserved,
            rec.diag,
            format!(
                "{} 观测：步{} {}核{} {}",
                self.name,
                rec.step_idx,
                rec.role.as_str(),
                rec.core,
                rec.outcome.as_str()
            ),
        ));
    }

    /// 观测一次隔离立案（写独立账）。
    pub fn observe_isolated(&self, core: u16, tick: u64, sink: &mut EventLedger) {
        let _ = sink.push(EventRecord::new(
            tick,
            EventKind::ApIsolated,
            XC_AP_OFFLINE_ISOLATED,
            format!("{} 观测：核 {} 隔离离场（不重试）", self.name, core),
        ));
    }

    /// 观测收口结论（写独立账）。
    pub fn observe_closure(&self, rep: &ClosureReport, sink: &mut EventLedger) {
        let _ = sink.push(EventRecord::new(
            0,
            EventKind::ClosureAssessed,
            if rep.closed { 0 } else { XC_CLOSURE_INCOMPLETE },
            format!(
                "{} 观测：收口 {}/{} {}",
                self.name,
                rep.online_count,
                rep.ap_total,
                if rep.closed { "闭合" } else { "未闭合" }
            ),
        ));
    }
}

/// 观测器惰性证明（零改动语义自证——判据②的机检本体）：
/// 同计划同种子两跑——一次裸跑，一次挂接观测器跑；
/// 断言 transcript 逐位全等+实账指纹全等。
/// 任何不等 ⇒ `XC_TRANSCRIPT_DRIFT`（零改动语义破约红线）。
pub fn observer_inertness_proof(plan: &BootPlan) -> Result<InertnessProof, BootSeqError> {
    let driver = BootDriver::new(plan.clone())?;

    // 裸跑。
    let mut clock0 = LcgClock::new(plan.seed);
    let (ledger0, transcript0) = driver.run(&mut clock0)?;

    // 挂接跑：观测器全程在环，输出只进独立账。
    let obs = BootObserver::new("回归挂接");
    let mut sink = EventLedger::new();
    let mut clock1 = LcgClock::new(plan.seed);
    let (ledger1, transcript1) = {
        // 借用拆分：观测器与独立账分开借。
        let (o, s) = (obs, &mut sink);
        driver.run_observed(&mut clock1, Some((&o, s)))?
    };

    let transcript_equal = transcript0 == transcript1;
    let ledger_equal = ledger0.fingerprint() == ledger1.fingerprint();
    let observed_events = sink.len();
    let proved = transcript_equal && ledger_equal && observed_events > 0;
    if !proved {
        return Err(BootSeqError::TranscriptDrift {
            code: XC_TRANSCRIPT_DRIFT,
            reason: format!(
                "挂接前后 transcript 全等={} 实账指纹全等={} 观测事件={}——\
                 零改动语义破约，清查观测器是否触碰驱动状态",
                transcript_equal, ledger_equal, observed_events
            ),
        });
    }
    Ok(InertnessProof {
        transcript_fp_before: transcript0.fingerprint(),
        transcript_fp_after: transcript1.fingerprint(),
        observed_events,
        proved,
    })
}

/// 惰性证明报告（挂接前后对拍证据）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InertnessProof {
    /// 裸跑 transcript 指纹。
    pub transcript_fp_before: u64,
    /// 挂接跑 transcript 指纹。
    pub transcript_fp_after: u64,
    /// 观测器写入独立账的事件数（>0 证明观测器真的在环）。
    pub observed_events: usize,
    /// 证明成立（transcript 全等+实账指纹全等+观测非空）。
    pub proved: bool,
}

// ---------------------------------------------------------------------------
// §10 账面回放（chain_replay——判据①"BSP→AP 各步账回放"的本体）
// ---------------------------------------------------------------------------

/// 账面回放报告（按账重演整序列，逐步对拍实账）。
///
/// 回放器只认账面，独立推进 canonical 序列模型：
/// ① 步 0~2 预期 BSP 核 0 各一笔；② AP 参集 S = 账内步 3（AP 自检）
/// 出现的核集（升序去重）；③ 步 3~6 每步预期恰好覆盖 S 各一笔；
/// ④ 逐条对拍（步号/角色/核号三元组逐位相等）；⑤ 时戳单调性复验。
/// 回放一致 100% = matches == 回放行数 == 实账行数且零失配。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainReplayReport {
    /// 实账行数。
    pub actual_rows: usize,
    /// 回放重演行数。
    pub replayed_rows: usize,
    /// 对拍通过行数。
    pub matches: usize,
    /// 失配证据行（中文，逐行定位）。
    pub mismatches: Vec<String>,
    /// 回放一致 100%（判据①红线）。
    pub full_match: bool,
    /// 回放 AP 参集（步 3 提取，升序）。
    pub replay_ap_set: Vec<u16>,
}

impl BootSequenceLedger {
    /// 账面回放（不跑驱动、不重算时序——只按账重演序列模型对拍）。
    pub fn chain_replay(&self) -> ChainReplayReport {
        // ① 提取 AP 参集（步 3 账面核集，升序去重）。
        let mut ap_set: Vec<u16> = Vec::new();
        for r in &self.records {
            if r.step_idx == BootStep::ApSelfCheck.index() && r.role == BootRole::Ap {
                let mut seen = false;
                for &c in &ap_set {
                    if c == r.core {
                        seen = true;
                        break;
                    }
                }
                if !seen {
                    ap_set.push(r.core);
                }
            }
        }
        ap_set.sort_unstable();

        // ② 重演预期序列：步 0~2 BSP 核 0；步 3~6 逐核覆盖 S。
        let mut expected: Vec<(u8, BootRole, u16)> = Vec::new();
        let mut s = 0u8;
        while s < BOOT_STEP_COUNT as u8 {
            let step = BOOT_STEP_ALL[s as usize];
            if step.exec_role() == BootRole::Bsp {
                expected.push((s, BootRole::Bsp, 0));
            } else {
                let mut i = 0usize;
                while i < ap_set.len() {
                    expected.push((s, BootRole::Ap, ap_set[i]));
                    i += 1;
                }
            }
            s += 1;
        }

        // ③ 逐条对拍。
        let mut matches = 0usize;
        let mut mismatches: Vec<String> = Vec::new();
        let mut i = 0usize;
        let common = if expected.len() < self.records.len() {
            expected.len()
        } else {
            self.records.len()
        };
        while i < common {
            let (es, er, ec) = expected[i];
            let r = &self.records[i];
            let row_ok = es == r.step_idx && er == r.role && ec == r.core;
            if row_ok {
                matches = matches.saturating_add(1);
            } else {
                mismatches.push(format!(
                    "回放失配 @{}：预期（步{} {}核{}）实账（步{} {}核{}）",
                    i, es, er.as_str(), ec, r.step_idx, r.role.as_str(), r.core
                ));
            }
            i += 1;
        }
        if expected.len() != self.records.len() {
            mismatches.push(format!(
                "行数失配：回放重演 {} 行 vs 实账 {} 行",
                expected.len(),
                self.records.len()
            ));
        }

        // ④ 时戳单调性复验（重演一致性的一部分：tick_end 不回退）。
        let mut i = 1usize;
        while i < self.records.len() {
            if self.records[i].tick_end < self.records[i - 1].tick_end {
                mismatches.push(format!(
                    "时戳回退 @{}：tick_end {} < 前行 {}（时序账被污染）",
                    i, self.records[i].tick_end, self.records[i - 1].tick_end
                ));
            }
            i += 1;
        }

        let full_match = mismatches.is_empty()
            && expected.len() == self.records.len()
            && matches == expected.len()
            && !self.records.is_empty();
        ChainReplayReport {
            actual_rows: self.records.len(),
            replayed_rows: expected.len(),
            matches,
            mismatches,
            full_match,
            replay_ap_set: ap_set,
        }
    }
}

// ---------------------------------------------------------------------------
// §11 驱动重演（driver replay——全量重演+逐字段对拍：篡改检出面）
// ---------------------------------------------------------------------------

/// 驱动重演报告（全量：同计划同种子重跑驱动，产回放账，与实账
/// 逐字段对拍——步号/角色/核号/时戳/结论/诊断码/备注全查）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverReplayReport {
    /// 对拍字段数（行数×字段数）。
    pub compared_fields: usize,
    /// 失配字段证据（中文逐条）。
    pub mismatches: Vec<String>,
    /// 回放一致（零失配且行数相等）。
    pub consistent: bool,
    /// 实账行数。
    pub len_actual: usize,
    /// 回放账行数。
    pub len_replay: usize,
}

/// 驱动重演（判据①的加强面）：同计划同种子重跑驱动 ⇒ 回放账与实账
/// 逐字段对拍。健康账 100% 一致；实账任何字段被篡改（含时戳）即被
/// 抓出——回放篡改注入的检出面。
pub fn replay_against_driver(
    actual: &BootSequenceLedger,
    plan: &BootPlan,
) -> Result<DriverReplayReport, BootSeqError> {
    let driver = BootDriver::new(plan.clone())?;
    let mut clock = LcgClock::new(plan.seed);
    let (replay_ledger, _t) = driver.run(&mut clock)?;

    let ra = actual.records();
    let rr = replay_ledger.records();
    let mut mismatches: Vec<String> = Vec::new();
    let mut compared_fields: usize = 0;
    let common = if ra.len() < rr.len() { ra.len() } else { rr.len() };
    let mut i = 0usize;
    while i < common {
        let a = &ra[i];
        let b = &rr[i];
        // 七字段逐一对拍（顺序稳定——证据行可复现）。
        if a.step_idx != b.step_idx {
            mismatches.push(format!(
                "步号失配 @{}：实账 {} 回放 {}",
                i, a.step_idx, b.step_idx
            ));
        }
        compared_fields = compared_fields.saturating_add(1);
        if a.role != b.role {
            mismatches.push(format!(
                "角色失配 @{}：实账 {} 回放 {}",
                i,
                a.role.as_str(),
                b.role.as_str()
            ));
        }
        compared_fields = compared_fields.saturating_add(1);
        if a.core != b.core {
            mismatches.push(format!(
                "核号失配 @{}：实账 {} 回放 {}",
                i, a.core, b.core
            ));
        }
        compared_fields = compared_fields.saturating_add(1);
        if a.tick_start != b.tick_start {
            mismatches.push(format!(
                "起始时戳失配 @{}：实账 {} 回放 {}（实账疑被篡改）",
                i, a.tick_start, b.tick_start
            ));
        }
        compared_fields = compared_fields.saturating_add(1);
        if a.tick_end != b.tick_end {
            mismatches.push(format!(
                "结束时戳失配 @{}：实账 {} 回放 {}（实账疑被篡改）",
                i, a.tick_end, b.tick_end
            ));
        }
        compared_fields = compared_fields.saturating_add(1);
        if a.outcome != b.outcome {
            mismatches.push(format!(
                "结论失配 @{}：实账 {} 回放 {}",
                i,
                a.outcome.as_str(),
                b.outcome.as_str()
            ));
        }
        compared_fields = compared_fields.saturating_add(1);
        if a.diag != b.diag {
            mismatches.push(format!(
                "诊断码失配 @{}：实账 {:#06x} 回放 {:#06x}",
                i, a.diag, b.diag
            ));
        }
        compared_fields = compared_fields.saturating_add(1);
        if a.note != b.note {
            mismatches.push(format!("备注失配 @{}：实账「{}」回放「{}」", i, a.note, b.note));
        }
        compared_fields = compared_fields.saturating_add(1);
        i += 1;
    }
    if ra.len() != rr.len() {
        mismatches.push(format!(
            "行数失配：实账 {} 行 vs 回放 {} 行",
            ra.len(),
            rr.len()
        ));
    }
    let consistent = mismatches.is_empty() && ra.len() == rr.len() && !ra.is_empty();
    Ok(DriverReplayReport {
        compared_fields,
        mismatches,
        consistent,
        len_actual: ra.len(),
        len_replay: rr.len(),
    })
}

// ---------------------------------------------------------------------------
// §12 收口断言（ClosureGate——全部 AP 在线确认：计数判据+逐行+10/10）
// ---------------------------------------------------------------------------

/// 逐 AP 在线确认行（收口断言的逐行本体）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnlineRow {
    /// 核号。
    pub core: u16,
    /// APIC ID（描述子确定性映射）。
    pub apic_id: u32,
    /// 在线确认时戳（tick_end）。
    pub confirmed_tick: u64,
}

/// 收口评估报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosureReport {
    /// 逐 AP 在线确认行（核号升序）。
    pub rows: Vec<OnlineRow>,
    /// 在线确认计数。
    pub online_count: usize,
    /// AP 总数（预期基线）。
    pub ap_total: u16,
    /// 收口闭合（在线计数==AP 总数且缺席清单为空）。
    pub closed: bool,
    /// 缺席清单（预期在线而未确认的核，升序）。
    pub missing: Vec<u16>,
}

impl BootSequenceLedger {
    /// 收口评估（判据③）：扫步 6 账——结论"过"的逐核成行；
    /// 在线计数==AP 总数 ⇒ 闭合；差额核逐个入缺席清单。
    pub fn closure_assess(&self) -> ClosureReport {
        let mut rows: Vec<OnlineRow> = Vec::new();
        for r in &self.records {
            if r.step_idx == BootStep::ApOnlineConfirm.index()
                && r.role == BootRole::Ap
                && r.outcome == StepOutcome::Pass
            {
                rows.push(OnlineRow {
                    core: r.core,
                    apic_id: ApDescriptor::of(r.core).apic_id,
                    confirmed_tick: r.tick_end,
                });
            }
        }
        rows.sort_unstable_by_key(|x| x.core);
        let online_count = rows.len();
        // 缺席清单：预期核 1..=AP 总数 不在确认行内。
        let mut missing: Vec<u16> = Vec::new();
        let mut c = 1u16;
        while c <= self.ap_total {
            let mut found = false;
            for r in &rows {
                if r.core == c {
                    found = true;
                    break;
                }
            }
            if !found {
                missing.push(c);
            }
            c = c.saturating_add(1);
        }
        let closed = online_count == self.ap_total as usize && missing.is_empty();
        ClosureReport {
            rows,
            online_count,
            ap_total: self.ap_total,
            closed,
            missing,
        }
    }
}

/// 收口演练种子表（10 组——判据③"10/10 次演练计数"，写死；
/// 取自 F0939 单号的确定性种子族）。
pub const DRILL_SEEDS: [u64; 10] = [
    0x0939_0001,
    0x0939_0002,
    0x0939_0003,
    0x0939_0004,
    0x0939_0005,
    0x0939_0006,
    0x0939_0007,
    0x0939_0008,
    0x0939_0009,
    0x0939_000A,
];

/// 收口演练报告（10/10 计数器）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrillReport {
    /// 演练组数。
    pub drills: usize,
    /// 收口闭合组数。
    pub closed: usize,
    /// 全闭合（10/10——判据③红线）。
    pub all_closed: bool,
    /// 逐组（种子，闭合，在线计数）。
    pub per_drill: Vec<(u64, bool, usize)>,
}

/// 收口闸（判据③的执行体）。
pub struct ClosureGate;

impl ClosureGate {
    /// 闸名。
    pub const GATE_NAME: &'static str = "B07·启动收口闸";

    /// 评估一笔账的收口（委托账面评估——单一事实源）。
    pub fn assess(ledger: &BootSequenceLedger) -> ClosureReport {
        ledger.closure_assess()
    }

    /// 收口演练：对每组种子跑健康启动（AP 总数固定），
    /// 数收口闭合组数。全闭合 ⇒ 10/10。
    pub fn drill(ap_total: u16, seeds: &[u64]) -> Result<DrillReport, BootSeqError> {
        let mut per: Vec<(u64, bool, usize)> = Vec::new();
        let mut closed = 0usize;
        let mut i = 0usize;
        while i < seeds.len() {
            let plan = BootPlan::healthy(ap_total, seeds[i]);
            let driver = BootDriver::new(plan)?;
            let mut clock = LcgClock::new(seeds[i]);
            let (ledger, _t) = driver.run(&mut clock)?;
            let rep = ledger.closure_assess();
            if rep.closed {
                closed = closed.saturating_add(1);
            }
            per.push((seeds[i], rep.closed, rep.online_count));
            i += 1;
        }
        Ok(DrillReport {
            drills: seeds.len(),
            closed,
            all_closed: closed == seeds.len() && seeds.len() > 0,
            per_drill: per,
        })
    }
}

// ---------------------------------------------------------------------------
// §13 注入面（三类各 10/10 检出：跳步/AP 缺席/回放篡改）
// ---------------------------------------------------------------------------

/// 注入种类闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionKind {
    /// 跳步注入（该步不产生账——链审计应检出步缺失）。
    SkipStep,
    /// AP 缺席注入（核全程不应答——收口计数失配应检出）。
    ApAbsent,
    /// 回放篡改注入（实账字段被改——驱动重演逐字段对拍应检出）。
    LedgerTamper,
}

impl InjectionKind {
    /// 注入名（账面/诊断共用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            InjectionKind::SkipStep => "跳步注入",
            InjectionKind::ApAbsent => "AP缺席注入",
            InjectionKind::LedgerTamper => "回放篡改注入",
        }
    }
}

/// 单次注入发现。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InjectionFinding {
    /// 注入种类。
    pub kind: InjectionKind,
    /// 是否检出。
    pub detected: bool,
    /// 证据（中文）。
    pub evidence: String,
}

/// 注入演练汇总（10/10 计数器）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrillSummary {
    /// 注入种类。
    pub kind: InjectionKind,
    /// 演练组数。
    pub drills: usize,
    /// 检出组数。
    pub detected: usize,
    /// 全检出（10/10）。
    pub all_detected: bool,
    /// 首条证据。
    pub first_evidence: String,
}

/// 汇总闭包：全部检出才过；任何未检出 ⇒ `XC_INJECTION_UNDETECTED` 红线。
fn summarize(findings: Vec<InjectionFinding>) -> Result<DrillSummary, BootSeqError> {
    let kind = match findings.first() {
        Some(f) => f.kind,
        None => {
            return Err(BootSeqError::InjectionUndetected {
                code: XC_INJECTION_UNDETECTED,
                reason: "注入演练零样本：种子表为空，修演练参数".to_string(),
            })
        }
    };
    let mut detected = 0usize;
    let mut first_evidence = String::new();
    for f in &findings {
        if f.detected {
            detected = detected.saturating_add(1);
        }
        if first_evidence.is_empty() {
            first_evidence = f.evidence.clone();
        }
    }
    let all = detected == findings.len() && !findings.is_empty();
    if !all {
        // 找出未检出组作证据。
        let mut miss = String::new();
        for f in &findings {
            if !f.detected {
                miss = f.evidence.clone();
                break;
            }
        }
        return Err(BootSeqError::InjectionUndetected {
            code: XC_INJECTION_UNDETECTED,
            reason: format!(
                "{} 演练 {}/{} 检出（红线要求全检出）；未检出证据：{}",
                kind.as_str(),
                detected,
                findings.len(),
                miss
            ),
        });
    }
    Ok(DrillSummary {
        kind,
        drills: findings.len(),
        detected,
        all_detected: all,
        first_evidence,
    })
}

/// 演练一：跳步注入 ×10——每步轮换跳过，链审计必须逐次检出步缺失。
pub fn drill_skip_injection() -> Result<DrillSummary, BootSeqError> {
    let mut findings: Vec<InjectionFinding> = Vec::new();
    let mut i = 0usize;
    while i < BOOT_STEP_COUNT {
        let plan = BootPlan {
            ap_total: 4,
            seed: DRILL_SEEDS[i],
            absent_cores: Vec::new(),
            skip_step: Some(BOOT_STEP_ALL[i]),
            slow_step: None,
            slow_by: 0,
        };
        let driver = BootDriver::new(plan)?;
        let mut clock = LcgClock::new(DRILL_SEEDS[i]);
        let (ledger, _t) = driver.run(&mut clock)?;
        let audit = ledger.chain_audit();
        // 检出面双查：中段跳步→违例行；尾步跳步→链不完整（七步闭集
        // 未走满）。两查任一命中即检出。
        let detected = !audit.violations.is_empty() || !audit.complete_chain;
        findings.push(InjectionFinding {
            kind: InjectionKind::SkipStep,
            detected,
            evidence: format!(
                "跳过步{}（{}）：链审计违例 {} 行+链完整={}，首行「{}」",
                i,
                BOOT_STEP_ALL[i].as_str(),
                audit.violations.len(),
                audit.complete_chain,
                audit
                    .violations
                    .first()
                    .map(|s| s.as_str())
                    .unwrap_or(if audit.complete_chain { "（无）" } else { "链不完整（七步未走满）" })
            ),
        });
        i += 1;
    }
    summarize(findings)
}

/// 演练二：AP 缺席注入 ×10——轮换缺席核，收口评估必须逐次检出
/// （在线计数 < AP 总数+缺席清单点名）。
pub fn drill_absent_injection() -> Result<DrillSummary, BootSeqError> {
    let mut findings: Vec<InjectionFinding> = Vec::new();
    let mut i = 0usize;
    while i < DRILL_SEEDS.len() {
        let victim = (i % 4) as u16 + 1; // 1..=4 轮换
        let plan = BootPlan {
            ap_total: 4,
            seed: DRILL_SEEDS[i],
            absent_cores: vec![victim],
            skip_step: None,
            slow_step: None,
            slow_by: 0,
        };
        let driver = BootDriver::new(plan)?;
        let mut clock = LcgClock::new(DRILL_SEEDS[i]);
        let (ledger, _t) = driver.run(&mut clock)?;
        let rep = ledger.closure_assess();
        let audit = ledger.degrade_retry_audit()?;
        let detected = !rep.closed && rep.missing.contains(&victim) && audit == 1;
        findings.push(InjectionFinding {
            kind: InjectionKind::ApAbsent,
            detected,
            evidence: format!(
                "缺席核 {}：收口 {}/{} 未闭合，缺席清单 {:?}，降级案 {} 笔（重试计数全 0）",
                victim, rep.online_count, rep.ap_total, rep.missing, audit
            ),
        });
        i += 1;
    }
    summarize(findings)
}

/// 演练三：回放篡改注入 ×10——对健康账逐组改一个字段
/// （时戳/核号/结论轮换），驱动重演逐字段对拍必须逐次抓出。
pub fn drill_tamper_injection() -> Result<DrillSummary, BootSeqError> {
    let mut findings: Vec<InjectionFinding> = Vec::new();
    let mut i = 0usize;
    while i < DRILL_SEEDS.len() {
        let plan = BootPlan::healthy(4, DRILL_SEEDS[i]);
        let driver = BootDriver::new(plan.clone())?;
        let mut clock = LcgClock::new(DRILL_SEEDS[i]);
        let (healthy, _t) = driver.run(&mut clock)?;
        let mut tampered = healthy.clone();
        let n = tampered.records_len();
        if n == 0 {
            return Err(BootSeqError::LedgerEmpty {
                code: XC_LEDGER_EMPTY,
                reason: "健康账空：驱动产出异常，先查驱动".to_string(),
            });
        }
        // 轮换篡改点：时戳/核号/结论。
        let idx = (i * 3 + 1) % n;
        match i % 3 {
            0 => {
                let recs = tampered.records_mut_tamper_surface();
                recs[idx].tick_end = recs[idx].tick_end.wrapping_add(1);
            }
            1 => {
                let recs = tampered.records_mut_tamper_surface();
                recs[idx].core = recs[idx].core.wrapping_add(100);
            }
            _ => {
                let recs = tampered.records_mut_tamper_surface();
                recs[idx].outcome = StepOutcome::Isolated;
            }
        }
        let rep = replay_against_driver(&tampered, &plan)?;
        let detected = !rep.consistent && !rep.mismatches.is_empty();
        findings.push(InjectionFinding {
            kind: InjectionKind::LedgerTamper,
            detected,
            evidence: format!(
                "篡改账序 {}（第 {} 类字段）：对拍 {} 字段失配 {} 行，首行「{}」",
                idx,
                i % 3,
                rep.compared_fields,
                rep.mismatches.len(),
                rep.mismatches.first().map(|s| s.as_str()).unwrap_or("（无）")
            ),
        });
        i += 1;
    }
    summarize(findings)
}

// ---------------------------------------------------------------------------
// §14 双跑确定性对拍 + 健康面（health()）
// ---------------------------------------------------------------------------

/// 双跑确定性对拍：同计划同种子跑两遍，账指纹+transcript 指纹必须
/// 全等（LCG 确定性的端到端背书——判据②"现存 SMP 启动全绿"的
/// 确定性半边）。
pub fn dual_run_determinism(plan: &BootPlan) -> Result<DualRunProof, BootSeqError> {
    let driver = BootDriver::new(plan.clone())?;
    let mut c0 = LcgClock::new(plan.seed);
    let (l0, t0) = driver.run(&mut c0)?;
    let mut c1 = LcgClock::new(plan.seed);
    let (l1, t1) = driver.run(&mut c1)?;
    let fp0 = l0.fingerprint();
    let fp1 = l1.fingerprint();
    let equal = fp0 == fp1 && t0 == t1;
    if !equal {
        return Err(BootSeqError::FingerprintDrift {
            code: XC_FINGERPRINT_DRIFT,
            reason: format!(
                "双跑指纹 {:#018x} vs {:#018x} 不等（transcript 全等={}）：\
                 确定性破约，清查时钟/抖动源是否混入外部状态",
                fp0, fp1, t0 == t1
            ),
        });
    }
    Ok(DualRunProof {
        fingerprint_a: fp0,
        fingerprint_b: fp1,
        transcript_equal: true,
        records: l0.records_len(),
    })
}

/// 双跑对拍证据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DualRunProof {
    /// 第一跑账指纹。
    pub fingerprint_a: u64,
    /// 第二跑账指纹。
    pub fingerprint_b: u64,
    /// transcript 全等。
    pub transcript_equal: bool,
    /// 账行数。
    pub records: usize,
}

/// 健康状态三档（绿/黄/红）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// 绿：零违例零降级且收口闭合。
    Green,
    /// 黄：无违例但有降级（隔离离场）或收口未闭合（降级语义内）。
    Amber,
    /// 红：链违例成立（步序/步缺失）——回归红线。
    Red,
}

impl HealthStatus {
    /// 状态名（账面/诊断共用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            HealthStatus::Green => "绿",
            HealthStatus::Amber => "黄",
            HealthStatus::Red => "红",
        }
    }
}

/// 健康面报告（判据②"现存 SMP 启动全绿"的量化面）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// 健康状态。
    pub status: HealthStatus,
    /// 步账行数。
    pub records: usize,
    /// 事件账笔数。
    pub events: usize,
    /// 降级案数。
    pub degraded: usize,
    /// 链违例行数。
    pub violations: usize,
    /// 千分比通过率（Pass 行 / 总行 ×1000，saturating）。
    pub pass_rate_milli: u32,
    /// 收口闭合。
    pub closed: bool,
    /// 账指纹。
    pub fingerprint: u64,
}

impl BootSequenceLedger {
    /// 健康面：链审计+收口+降级三账合一判色。
    ///
    /// 判色规则（冻结）：链违例>0 ⇒ 红；否则降级>0 或收口未闭合
    /// ⇒ 黄；否则绿。通过率只统计 Pass 行占比（千分比，saturating）。
    pub fn health(&self) -> HealthReport {
        let audit = self.chain_audit();
        let closure = self.closure_assess();
        let violations = audit.violations.len();
        let degraded = self.degrades.len();
        let status = if violations > 0 || (!audit.complete_chain && self.records_len() > 0) {
            HealthStatus::Red
        } else if degraded > 0 || !closure.closed || audit.red_count > 0 {
            // 红/隔离结论行（超时降级/隔离离场）也进黄档——降级语义
            // 内，非链违例。
            HealthStatus::Amber
        } else {
            HealthStatus::Green
        };
        let total = self.records.len();
        let mut pass = 0usize;
        for r in &self.records {
            if r.outcome == StepOutcome::Pass {
                pass = pass.saturating_add(1);
            }
        }
        let rate = if total == 0 {
            0
        } else {
            ((pass as u64) * 1000 / total as u64) as u32
        };
        HealthReport {
            status,
            records: total,
            events: self.events().len(),
            degraded,
            violations,
            pass_rate_milli: rate,
            closed: closure.closed,
            fingerprint: self.fingerprint(),
        }
    }
}

// ---------------------------------------------------------------------------
// §15 错误闭集（BootSeqError——每变体 detail() 中文可操作诊断+诊断码）
// ---------------------------------------------------------------------------

/// 本单错误闭集（全可失败路径的唯一出口）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootSeqError {
    /// 步序违例（账面步号倒退/越出闭集）。
    OrderViolation { code: u32, reason: String },
    /// 步缺失（跳步注入检出后转红线的报错面）。
    StepMissing { code: u32, reason: String },
    /// 步超时（预算超标）。
    BudgetTimeout { code: u32, reason: String },
    /// 重试违例（隔离不重试红线）。
    RetryViolation { code: u32, reason: String },
    /// 收口未闭合。
    ClosureIncomplete { code: u32, reason: String },
    /// 回放对拍失配。
    ReplayMismatch { code: u32, reason: String },
    /// 注入未检出（演练红线）。
    InjectionUndetected { code: u32, reason: String },
    /// 计划非法。
    PlanInvalid { code: u32, reason: String },
    /// 空账。
    LedgerEmpty { code: u32, reason: String },
    /// 账容量越限。
    LedgerOverflow { code: u32 },
    /// 双跑指纹漂移。
    FingerprintDrift { code: u32, reason: String },
    /// transcript 漂移（零改动语义破约）。
    TranscriptDrift { code: u32, reason: String },
    /// AP 核号越界。
    ApOutOfRange { code: u32, reason: String },
    /// 健康红账。
    HealthRed { code: u32, reason: String },
}

impl BootSeqError {
    /// 诊断码。
    pub fn code(&self) -> u32 {
        match self {
            BootSeqError::OrderViolation { code, .. }
            | BootSeqError::StepMissing { code, .. }
            | BootSeqError::BudgetTimeout { code, .. }
            | BootSeqError::RetryViolation { code, .. }
            | BootSeqError::ClosureIncomplete { code, .. }
            | BootSeqError::ReplayMismatch { code, .. }
            | BootSeqError::InjectionUndetected { code, .. }
            | BootSeqError::PlanInvalid { code, .. }
            | BootSeqError::LedgerEmpty { code, .. }
            | BootSeqError::LedgerOverflow { code }
            | BootSeqError::FingerprintDrift { code, .. }
            | BootSeqError::TranscriptDrift { code, .. }
            | BootSeqError::ApOutOfRange { code, .. }
            | BootSeqError::HealthRed { code, .. } => *code,
        }
    }

    /// 中文可操作诊断（每变体一条——修什么、去哪查）。
    pub fn detail(&self) -> &'static str {
        match self {
            BootSeqError::OrderViolation { .. } => {
                "步序违例：七步全序被破坏（步号倒退或越出闭集）。修法：检查驱动步表/挂接点是否乱序入账，\
                 先跑 chain_audit 定位失配账序。"
            }
            BootSeqError::StepMissing { .. } => {
                "步缺失：账面出现跳步（步号跨越大于 1）。修法：核对被跳过的步是否被注入面抑制，\
                 生产路径必须补齐该步账。"
            }
            BootSeqError::BudgetTimeout { .. } => {
                "步超时：步耗时超出 STEP_BUDGET_TICKS 预算。修法：确认时钟注入是否过快/过慢，\
                 超时核走降级账（隔离不重试），勿静默重试。"
            }
            BootSeqError::RetryViolation { .. } => {
                "重试违例：降级账出现非零重试计数——隔离不重试（F0347）红线。修法：删除重试路径，\
                 未在线 AP 一律隔离离场并留案。"
            }
            BootSeqError::ClosureIncomplete { .. } => {
                "收口未闭合：在线确认数 < AP 总数。修法：对照缺席清单逐核查哨兵应答与降级账，\
                 确认隔离语义后才可放行后续域。"
            }
            BootSeqError::ReplayMismatch { .. } => {
                "回放对拍失配：驱动重演与实账逐字段不等。修法：优先怀疑实账被篡改或计划/种子不一致，\
                 用 mismatches 逐行定位。"
            }
            BootSeqError::InjectionUndetected { .. } => {
                "注入未检出：演练中注入发生而账面无感。修法：检查对应检出面（链审计/收口评估/重演对拍）\
                 是否被旁路。"
            }
            BootSeqError::PlanInvalid { .. } => {
                "启动计划非法：AP 总数越界/缺席核越界/重复缺席。修法：按 reason 修正计划字段后重试。"
            }
            BootSeqError::LedgerEmpty { .. } => {
                "空账：无步账可审计。修法：先跑驱动产出实账，再入审计/回放/收口。"
            }
            BootSeqError::LedgerOverflow { .. } => {
                "账容量越限：事件账超防御上限。修法：收敛观测频率或扩容 EVENT_LEDGER_CAP（需评审）。"
            }
            BootSeqError::FingerprintDrift { .. } => {
                "双跑指纹漂移：同计划同种子两跑指纹不等。修法：清查时钟/抖动源是否混入外部状态，\
                 保证 LCG 单源。"
            }
            BootSeqError::TranscriptDrift { .. } => {
                "transcript 漂移：挂接观测器前后驱动发射序列不等——零改动语义破约。修法：\
                 检查观测器是否触碰驱动状态（必须只写独立账）。"
            }
            BootSeqError::ApOutOfRange { .. } => {
                "AP 核号越界：缺席核不在 1..=AP 总数（核 0 是 BSP）。修法：修正缺席核清单。"
            }
            BootSeqError::HealthRed { .. } => {
                "健康红账：链违例红线成立。修法：先清 chain_audit 违例行，再放行后续域。"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// §16 全链自证（聚合入口——ktest B07 桥接点）
// ---------------------------------------------------------------------------

/// 七步全序 const 表自检（表序/前置链/角色/预算四查——编译期断言
/// 之外再给运行期一份可入账的证据）。
fn const_table_selfcheck() -> Result<String, BootSeqError> {
    let mut i = 0usize;
    while i < BOOT_STEP_COUNT {
        let step = BOOT_STEP_ALL[i];
        if step.index() as usize != i {
            return Err(BootSeqError::OrderViolation {
                code: XC_STEP_OUT_OF_ORDER,
                reason: format!("全序表位 {} 存的步号是 {}：表序错乱", i, step.index()),
            });
        }
        if i > 0 {
            if step.predecessor_index() != i as i16 - 1 {
                return Err(BootSeqError::OrderViolation {
                    code: XC_STEP_OUT_OF_ORDER,
                    reason: format!(
                        "步 {} 前置步号 {} ≠ {}：前置链断裂",
                        i,
                        step.predecessor_index(),
                        i - 1
                    ),
                });
            }
            if !BootStep::precondition_satisfied(1u16 << (i - 1), step) {
                return Err(BootSeqError::OrderViolation {
                    code: XC_STEP_OUT_OF_ORDER,
                    reason: format!("步 {} 前置位掩码判定失败：前置条件表错", i),
                });
            }
        }
        if step.base_ticks() >= step.budget_ticks() {
            return Err(BootSeqError::BudgetTimeout {
                code: XC_STEP_BUDGET_TIMEOUT,
                reason: format!(
                    "步 {} 基础耗时 {} ≥ 预算 {}：预算表失配",
                    i,
                    step.base_ticks(),
                    step.budget_ticks()
                ),
            });
        }
        i += 1;
    }
    Ok(format!(
        "七步全序表自检过：{} 步闭集、前置链线性、角色分布 BSP3/AP4、预算表 {} 项全盖",
        BOOT_STEP_COUNT, STEP_BUDGET_TICKS.len()
    ))
}

/// 健康启动全链（判据②"现存 SMP 启动全绿"的正面样本）：
/// 4 AP 健康计划 → 跑 → 链审计完整零违例 → 零降级 → 收口闭合 4/4
/// → 健康绿。
fn healthy_boot_full_chain() -> Result<String, BootSeqError> {
    let plan = BootPlan::healthy(4, 0x0939_BEEF);
    let driver = BootDriver::new(plan)?;
    let mut clock = LcgClock::new(0x0939_BEEF);
    let (ledger, _t) = driver.run(&mut clock)?;
    let audit = ledger.chain_audit();
    if !audit.complete_chain {
        return Err(BootSeqError::OrderViolation {
            code: XC_STEP_OUT_OF_ORDER,
            reason: format!(
                "健康启动链不完整：{} 行掩码 {:#06x} 违例 {:?}",
                audit.records_total, audit.steps_seen_mask, audit.violations
            ),
        });
    }
    let mask_expect: u16 = 0x7F;
    if audit.steps_seen_mask != mask_expect {
        return Err(BootSeqError::StepMissing {
            code: XC_STEP_MISSING,
            reason: format!(
                "健康启动步掩码 {:#06x} ≠ {:#06x}：七步有缺",
                audit.steps_seen_mask, mask_expect
            ),
        });
    }
    let degrades = ledger.degrade_retry_audit()?;
    if degrades != 0 {
        return Err(BootSeqError::RetryViolation {
            code: XC_RETRY_FORBIDDEN,
            reason: format!("健康启动竟有 {} 笔降级案：驱动状态污染", degrades),
        });
    }
    let closure = ledger.closure_assess();
    if !closure.closed || closure.online_count != 4 {
        return Err(BootSeqError::ClosureIncomplete {
            code: XC_CLOSURE_INCOMPLETE,
            reason: format!(
                "健康启动收口 {}/{} 未闭合缺席 {:?}",
                closure.online_count, closure.ap_total, closure.missing
            ),
        });
    }
    let h = ledger.health();
    if h.status != HealthStatus::Green {
        return Err(BootSeqError::HealthRed {
            code: XC_HEALTH_RED,
            reason: format!(
                "健康启动判色 {:?}（违例 {} 降级 {}）",
                h.status, h.violations, h.degraded
            ),
        });
    }
    Ok(format!(
        "健康启动全链过：{} 行步账（BSP3+AP16）、零违例零降级、收口 {}/{} 闭合、健康绿、通过率 {}‰",
        audit.records_total, closure.online_count, closure.ap_total, h.pass_rate_milli
    ))
}

/// 账面回放一致 100%（判据①"各步账回放"正面样本）。
fn chain_replay_consistency() -> Result<String, BootSeqError> {
    let plan = BootPlan::healthy(4, 0x0939_CAFE);
    let driver = BootDriver::new(plan)?;
    let mut clock = LcgClock::new(0x0939_CAFE);
    let (ledger, _t) = driver.run(&mut clock)?;
    let rep = ledger.chain_replay();
    if !rep.full_match {
        return Err(BootSeqError::ReplayMismatch {
            code: XC_REPLAY_MISMATCH,
            reason: format!(
                "账面回放不一致：重演 {} 行 vs 实账 {} 行，对拍 {} 行失配 {} 行，首行「{}」",
                rep.replayed_rows,
                rep.actual_rows,
                rep.matches,
                rep.mismatches.len(),
                rep.mismatches.first().map(|s| s.as_str()).unwrap_or("（无）")
            ),
        });
    }
    Ok(format!(
        "账面回放一致 100%：实账 {} 行 == 重演 {} 行，AP 参集 {:?} 升序提取",
        rep.actual_rows, rep.replayed_rows, rep.replay_ap_set
    ))
}

/// 驱动重演一致（全量逐字段对拍正面样本）+ 篡改检出（注入面联动）。
fn driver_replay_and_tamper() -> Result<String, BootSeqError> {
    let plan = BootPlan::healthy(4, 0x0939_DEAD);
    let driver = BootDriver::new(plan.clone())?;
    let mut clock = LcgClock::new(0x0939_DEAD);
    let (healthy, _t) = driver.run(&mut clock)?;
    let rep_ok = replay_against_driver(&healthy, &plan)?;
    if !rep_ok.consistent {
        return Err(BootSeqError::ReplayMismatch {
            code: XC_REPLAY_MISMATCH,
            reason: format!(
                "健康账重演竟失配 {} 行：{}",
                rep_ok.mismatches.len(),
                rep_ok.mismatches.first().map(|s| s.as_str()).unwrap_or("（无）")
            ),
        });
    }
    // 篡改一处时戳 → 重演必须抓出。
    let mut tampered = healthy.clone();
    {
        let recs = tampered.records_mut_tamper_surface();
        let mid = recs.len() / 2;
        recs[mid].tick_end = recs[mid].tick_end.wrapping_add(7);
    }
    let rep_bad = replay_against_driver(&tampered, &plan)?;
    if rep_bad.consistent || rep_bad.mismatches.is_empty() {
        return Err(BootSeqError::InjectionUndetected {
            code: XC_INJECTION_UNDETECTED,
            reason: "篡改时戳后重演仍判一致：回放篡改检出面失灵".to_string(),
        });
    }
    Ok(format!(
        "驱动重演过：健康账 {} 行逐字段对拍零失配（{} 字段）；篡改时戳后检出 {} 行失配（{} 字段）",
        rep_ok.len_actual, rep_ok.compared_fields, rep_bad.mismatches.len(), rep_bad.compared_fields
    ))
}

/// 收口演练 10/10（判据③）。
fn closure_drill_10() -> Result<String, BootSeqError> {
    let rep = ClosureGate::drill(4, &DRILL_SEEDS)?;
    if !rep.all_closed {
        return Err(BootSeqError::ClosureIncomplete {
            code: XC_CLOSURE_INCOMPLETE,
            reason: format!("收口演练 {}/{} 闭合（要求 10/10）", rep.closed, rep.drills),
        });
    }
    Ok(format!(
        "收口演练 10/10：{} 组种子全部闭合（在线 4/4，逐行确认齐）",
        rep.drills
    ))
}

/// 三类注入各 10/10 检出（判据红线：注入必抓）。
fn injections_all_detected() -> Result<String, BootSeqError> {
    let skip = drill_skip_injection()?;
    let absent = drill_absent_injection()?;
    let tamper = drill_tamper_injection()?;
    Ok(format!(
        "三注入全检出：{}（{}/{}）；{}（{}/{}）；{}（{}/{}）",
        skip.kind.as_str(), skip.detected, skip.drills,
        absent.kind.as_str(), absent.detected, absent.drills,
        tamper.kind.as_str(), tamper.detected, tamper.drills,
    ))
}

/// 双跑确定性 + 挂接零改动自证（判据②）。
fn determinism_and_inertness() -> Result<String, BootSeqError> {
    let plan = BootPlan::healthy(4, 0x0939_F00D);
    let dual = dual_run_determinism(&plan)?;
    let inert = observer_inertness_proof(&plan)?;
    if !dual.transcript_equal || dual.fingerprint_a != dual.fingerprint_b {
        return Err(BootSeqError::FingerprintDrift {
            code: XC_FINGERPRINT_DRIFT,
            reason: "双跑对拍破约".to_string(),
        });
    }
    if !inert.proved {
        return Err(BootSeqError::TranscriptDrift {
            code: XC_TRANSCRIPT_DRIFT,
            reason: "挂接零改动自证破约".to_string(),
        });
    }
    Ok(format!(
        "双跑确定性过：两跑指纹 {:#018x} 全等（{} 行）；挂接零改动过：transcript 前后 {:#018x} 全等，观测 {} 笔入独立账",
        dual.fingerprint_a, dual.records, inert.transcript_fp_before, inert.observed_events
    ))
}

/// 降级账对齐 F0347（隔离不重试语义的负面+正面双查）：
/// 负面——非零重试计数入账被拒；正面——缺席核降级案全零重试。
fn degrade_f0347_alignment() -> Result<String, BootSeqError> {
    // 正面：缺席注入 ⇒ 降级案 1 笔（重试计数 0）且收口未闭合。
    let plan = BootPlan {
        ap_total: 4,
        seed: 0x0939_1BAD,
        absent_cores: vec![2],
        skip_step: None,
        slow_step: None,
        slow_by: 0,
    };
    let driver = BootDriver::new(plan)?;
    let mut clock = LcgClock::new(0x0939_1BAD);
    let (ledger, _t) = driver.run(&mut clock)?;
    let n = ledger.degrade_retry_audit()?;
    if n != 1 {
        return Err(BootSeqError::RetryViolation {
            code: XC_RETRY_FORBIDDEN,
            reason: format!("缺席核 2 应立案 1 笔降级，实得 {} 笔：降级账失准", n),
        });
    }
    let closure = ledger.closure_assess();
    if closure.closed || closure.online_count != 3 {
        return Err(BootSeqError::ClosureIncomplete {
            code: XC_CLOSURE_INCOMPLETE,
            reason: format!(
                "缺席注入后收口应为 3/4 未闭合，实得 {}/{} closed={}",
                closure.online_count, closure.ap_total, closure.closed
            ),
        });
    }
    let h = ledger.health();
    if h.status != HealthStatus::Amber {
        return Err(BootSeqError::HealthRed {
            code: XC_HEALTH_RED,
            reason: format!("缺席注入应判黄（降级语义内），实判 {:?}", h.status),
        });
    }
    // 负面：非零重试计数必须被拒收（红线执法）。
    let mut guard = BootSequenceLedger::new(1, "红线执法")?;
    let reject = guard.push_degrade(DegradeRecord {
        core: 1,
        tick: 1,
        reason: DEGRADE_REASON_TIMEOUT,
        retry_attempt: 1,
        diag: XC_RETRY_FORBIDDEN,
    });
    match reject {
        Err(e) if e.code() == XC_RETRY_FORBIDDEN => {}
        _ => {
            return Err(BootSeqError::RetryViolation {
                code: XC_RETRY_FORBIDDEN,
                reason: "非零重试计数入账未被拒：红线执法失灵".to_string(),
            })
        }
    }
    Ok(format!(
        "降级对齐 F0347 过：缺席核隔离立案 1 笔（重试计数 0）、收口 3/4 判黄；非零重试入账被拒（红线执法在位）",
    ))
}

/// 超时降级链路（判据⑤）：INIT-SIPI 注入超时 → 步账超时结论 →
/// 预算判据复核 → 健康黄（无链违例）。
fn timeout_degrade_chain() -> Result<String, BootSeqError> {
    let plan = BootPlan {
        ap_total: 2,
        seed: 0x0939_5107,
        absent_cores: Vec::new(),
        skip_step: None,
        slow_step: Some(BootStep::InitSipi),
        slow_by: 1000,
    };
    let driver = BootDriver::new(plan)?;
    let mut clock = LcgClock::new(0x0939_5107);
    let (ledger, _t) = driver.run(&mut clock)?;
    // 步 1 账必为超时结论+预算判据复核。
    let mut found_timeout = false;
    for r in ledger.records() {
        if r.step_idx == BootStep::InitSipi.index() && r.outcome == StepOutcome::Timeout {
            let elapsed = r.elapsed();
            if !step_timeout(elapsed, BootStep::InitSipi) {
                return Err(BootSeqError::BudgetTimeout {
                    code: XC_STEP_BUDGET_TIMEOUT,
                    reason: format!("步 1 耗时 {} 未超预算却记超时：预算判据失准", elapsed),
                });
            }
            found_timeout = true;
        }
    }
    if !found_timeout {
        return Err(BootSeqError::BudgetTimeout {
            code: XC_STEP_BUDGET_TIMEOUT,
            reason: "INIT-SIPI 注入超时未产生超时账：超时链路失灵".to_string(),
        });
    }
    let audit = ledger.chain_audit();
    if !audit.clean() {
        return Err(BootSeqError::HealthRed {
            code: XC_HEALTH_RED,
            reason: "超时注入不应产生链违例（超时是降级不是错序）".to_string(),
        });
    }
    let h = ledger.health();
    if h.status != HealthStatus::Amber {
        return Err(BootSeqError::HealthRed {
            code: XC_HEALTH_RED,
            reason: format!("超时降级应判黄，实判 {:?}", h.status),
        });
    }
    Ok(format!(
        "超时降级链路过：INIT-SIPI 超预算记超时（预算判据复核过）、零链违例、健康黄（通过率 {}‰）",
        h.pass_rate_milli
    ))
}

/// 全链自证聚合（ktest B07 桥接点）：九面一锤定音。
pub fn run_all_checks() -> Result<String, (u32, String)> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(
        const_table_selfcheck()
            .map_err(|e| (e.code(), e.detail().to_string()))?,
    );
    parts.push(
        healthy_boot_full_chain()
            .map_err(|e| (e.code(), format!("{}：{}", e.detail(), err_reason(&e))))?,
    );
    parts.push(
        chain_replay_consistency()
            .map_err(|e| (e.code(), format!("{}：{}", e.detail(), err_reason(&e))))?,
    );
    parts.push(
        driver_replay_and_tamper()
            .map_err(|e| (e.code(), format!("{}：{}", e.detail(), err_reason(&e))))?,
    );
    parts.push(
        closure_drill_10()
            .map_err(|e| (e.code(), format!("{}：{}", e.detail(), err_reason(&e))))?,
    );
    parts.push(
        injections_all_detected()
            .map_err(|e| (e.code(), format!("{}：{}", e.detail(), err_reason(&e))))?,
    );
    parts.push(
        determinism_and_inertness()
            .map_err(|e| (e.code(), format!("{}：{}", e.detail(), err_reason(&e))))?,
    );
    parts.push(
        degrade_f0347_alignment()
            .map_err(|e| (e.code(), format!("{}：{}", e.detail(), err_reason(&e))))?,
    );
    parts.push(
        timeout_degrade_chain()
            .map_err(|e| (e.code(), format!("{}：{}", e.detail(), err_reason(&e))))?,
    );
    // 契约文本完整性（冻结声明在场——挂接/对齐两声明不可缺失）。
    if OBSERVER_ZERO_SIDE_EFFECT_DECL.is_empty() || F0347_ALIGNMENT_DECL.is_empty() {
        return Err((XC_TRANSCRIPT_DRIFT, "冻结契约文本缺失".to_string()));
    }
    parts.push(format!(
        "契约声明在场：零改动语义（{}字）+F0347 对齐（{}字）",
        OBSERVER_ZERO_SIDE_EFFECT_DECL.len(),
        F0347_ALIGNMENT_DECL.len()
    ));
    // 指纹稳定性：同账两次指纹必同（FNV 纯函数性）。
    let plan = BootPlan::healthy(2, 0x0939_5A9F);
    let driver = match BootDriver::new(plan) {
        Ok(d) => d,
        Err(e) => return Err((e.code(), e.detail().to_string())),
    };
    let mut clock = LcgClock::new(0x0939_5A9F);
    let (ledger, _t) = match driver.run(&mut clock) {
        Ok(x) => x,
        Err(e) => return Err((e.code(), e.detail().to_string())),
    };
    if ledger.fingerprint() != ledger.fingerprint() {
        return Err((XC_FINGERPRINT_DRIFT, "同账两次指纹不等：FNV 不纯".to_string()));
    }
    parts.push(format!(
        "指纹稳定：账指纹 {:#018x} 两次全等；{} 项检查全绿",
        ledger.fingerprint(),
        parts.len() + 1
    ));
    Ok(parts.join("；"))
}

/// 提取错误原因串（聚合入口的格式化糖——detail 是静态面，reason 是
/// 动态面，两层都要给）。
fn err_reason(e: &BootSeqError) -> String {
    match e {
        BootSeqError::OrderViolation { reason, .. }
        | BootSeqError::StepMissing { reason, .. }
        | BootSeqError::BudgetTimeout { reason, .. }
        | BootSeqError::RetryViolation { reason, .. }
        | BootSeqError::ClosureIncomplete { reason, .. }
        | BootSeqError::ReplayMismatch { reason, .. }
        | BootSeqError::InjectionUndetected { reason, .. }
        | BootSeqError::PlanInvalid { reason, .. }
        | BootSeqError::LedgerEmpty { reason, .. }
        | BootSeqError::FingerprintDrift { reason, .. }
        | BootSeqError::TranscriptDrift { reason, .. }
        | BootSeqError::ApOutOfRange { reason, .. }
        | BootSeqError::HealthRed { reason, .. } => reason.clone(),
        BootSeqError::LedgerOverflow { .. } => "事件账越限".to_string(),
    }
}

// ---------------------------------------------------------------------------
// §17 冒烟测试（探针 cargo test——七步全序/回放一致/收口 10/10/
//      三注入检出/双跑一致/零改动/红线执法）
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// §17 冒烟测试（探针 cargo test——七步全序/回放一致/收口 10/10/
//      三注入检出/双跑一致/零改动/红线执法）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试专用解包糖：内核纪律零 unwrap/expect/panic——测试也守同一条
    /// 线（失败一律走 assert! 报错面，带诊断码与中文诊断）。
    fn must_ok<T>(r: Result<T, BootSeqError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => {
                assert!(
                    false,
                    "期望成功，实得 {:#06x}：{}（{}）",
                    e.code(),
                    e.detail(),
                    err_reason(&e)
                );
                loop {}
            }
        }
    }

    #[test]
    fn smoke_seven_step_full_order() {
        // 七步全序：健康启动七步皆有账、掩码 0x7F、零违例。
        let plan = BootPlan::healthy(4, 0x5EED_0939);
        let driver = must_ok(BootDriver::new(plan));
        let mut clock = LcgClock::new(0x5EED_0939);
        let (ledger, transcript) = must_ok(driver.run(&mut clock));
        let audit = ledger.chain_audit();
        assert!(audit.complete_chain, "七步全序完整");
        assert_eq!(audit.steps_seen_mask, 0x7F, "步掩码须全 1");
        assert!(audit.violations.is_empty(), "零违例");
        // 账行数：BSP 3 + AP 4核×4步 = 19。
        assert_eq!(ledger.records_len(), 3 + 4 * 4, "账行数 19");
        assert_eq!(transcript.len(), 19, "transcript 发射 19 条");
        // 步名还原（step_name 全覆盖 0..=6）。
        let mut i = 0u8;
        while i < 7 {
            assert_ne!(step_name(i), "未知步", "步名表覆盖步 {}", i);
            i += 1;
        }
    }

    #[test]
    fn smoke_replay_consistency() {
        // 账面回放 100% + 驱动重演逐字段一致。
        let plan = BootPlan::healthy(4, 0x5EED_0002);
        let driver = must_ok(BootDriver::new(plan.clone()));
        let mut clock = LcgClock::new(0x5EED_0002);
        let (ledger, _t) = must_ok(driver.run(&mut clock));
        let chain = ledger.chain_replay();
        assert!(
            chain.full_match,
            "账面回放一致 100%：{:?}",
            chain.mismatches
        );
        assert_eq!(chain.actual_rows, chain.replayed_rows, "行数对齐");
        assert_eq!(chain.replay_ap_set, vec![1, 2, 3, 4], "AP 参集提取");
        let drv = must_ok(replay_against_driver(&ledger, &plan));
        assert!(drv.consistent, "驱动重演一致：{:?}", drv.mismatches);
        assert!(drv.compared_fields >= drv.len_actual * 8, "八字段全对拍");
    }

    #[test]
    fn smoke_closure_10_of_10() {
        // 收口断言：10/10 组演练全部闭合，逐组在线 4/4。
        let rep = must_ok(ClosureGate::drill(4, &DRILL_SEEDS));
        assert_eq!(rep.drills, 10);
        assert_eq!(rep.closed, 10, "10/10 闭合");
        assert!(rep.all_closed);
        for entry in &rep.per_drill {
            let (seed, closed, online) = *entry;
            assert!(closed, "种子 {:#x} 闭合", seed);
            assert_eq!(online, 4, "种子 {:#x} 在线 4/4", seed);
        }
        // 收口闸评估与账面评估一致（单一事实源）。
        let plan = BootPlan::healthy(4, DRILL_SEEDS[0]);
        let driver = must_ok(BootDriver::new(plan));
        let mut clock = LcgClock::new(DRILL_SEEDS[0]);
        let (ledger, _t) = must_ok(driver.run(&mut clock));
        let a = ledger.closure_assess();
        let b = ClosureGate::assess(&ledger);
        assert_eq!(a, b, "收口闸==账面评估");
        assert!(b.closed);
    }

    #[test]
    fn smoke_three_injections_detected() {
        // 三类注入各全检出（跳步为七步轮换 7 组——每步都试）。
        let skip = must_ok(drill_skip_injection());
        assert_eq!(skip.drills, 7, "七步轮换");
        assert!(skip.all_detected);
        let absent = must_ok(drill_absent_injection());
        assert_eq!(absent.drills, 10);
        assert!(absent.all_detected);
        let tamper = must_ok(drill_tamper_injection());
        assert_eq!(tamper.drills, 10);
        assert!(tamper.all_detected);
    }

    #[test]
    fn smoke_dual_run_deterministic() {
        // 双跑：同计划同种子两跑指纹+transcript 全等。
        let plan = BootPlan::healthy(4, 0x5EED_0005);
        let proof = must_ok(dual_run_determinism(&plan));
        assert_eq!(proof.fingerprint_a, proof.fingerprint_b);
        assert!(proof.transcript_equal);
    }

    #[test]
    fn smoke_observer_inert() {
        // 零改动语义：挂接前后 transcript 逐位全等，观测非空。
        let plan = BootPlan::healthy(4, 0x5EED_0006);
        let proof = must_ok(observer_inertness_proof(&plan));
        assert!(proof.proved);
        assert_eq!(proof.transcript_fp_before, proof.transcript_fp_after);
        assert!(proof.observed_events >= 19, "观测覆盖全部步账");
    }

    #[test]
    fn smoke_retry_redline() {
        // 隔离不重试红线：非零重试计数入账被拒；审计零重试。
        let mut g = must_ok(BootSequenceLedger::new(1, "t"));
        let bad = g.push_degrade(DegradeRecord {
            core: 1,
            tick: 0,
            reason: DEGRADE_REASON_TIMEOUT,
            retry_attempt: 3,
            diag: XC_RETRY_FORBIDDEN,
        });
        match bad {
            Err(e) => assert_eq!(e.code(), XC_RETRY_FORBIDDEN, "拒收走红线码"),
            Ok(()) => assert!(false, "非零重试必须被拒"),
        }
        let ok = g.push_degrade(DegradeRecord {
            core: 1,
            tick: 0,
            reason: DEGRADE_REASON_SENTINEL,
            retry_attempt: 0,
            diag: XC_AP_OFFLINE_ISOLATED,
        });
        assert!(ok.is_ok(), "零重试可入账");
        assert_eq!(must_ok(g.degrade_retry_audit()), 1);
        // 错误诊断面：detail 非空+码在段内。
        let e = BootSeqError::RetryViolation {
            code: XC_RETRY_FORBIDDEN,
            reason: "测试".to_string(),
        };
        assert!(!e.detail().is_empty());
        assert!(e.code() >= 0x19B0 && e.code() <= 0x19BF);
    }

    #[test]
    fn smoke_health_faces() {
        // 健康三态：绿（健康）/黄（缺席降级）/红（跳步违例）。
        let d = must_ok(BootDriver::new(BootPlan::healthy(2, 0x5EED_0008)));
        let mut clock = LcgClock::new(0x5EED_0008);
        let (green, _) = must_ok(d.run(&mut clock));
        assert_eq!(green.health().status, HealthStatus::Green);

        let plan_absent = BootPlan {
            ap_total: 2,
            seed: 0x5EED_0009,
            absent_cores: vec![1],
            skip_step: None,
            slow_step: None,
            slow_by: 0,
        };
        let d2 = must_ok(BootDriver::new(plan_absent));
        let mut clock2 = LcgClock::new(0x5EED_0009);
        let (amber, _) = must_ok(d2.run(&mut clock2));
        assert_eq!(amber.health().status, HealthStatus::Amber);

        let plan_skip = BootPlan {
            ap_total: 2,
            seed: 0x5EED_000A,
            absent_cores: Vec::new(),
            skip_step: Some(BootStep::ApFeatureProbe),
            slow_step: None,
            slow_by: 0,
        };
        let d3 = must_ok(BootDriver::new(plan_skip));
        let mut clock3 = LcgClock::new(0x5EED_000A);
        let (red, _) = must_ok(d3.run(&mut clock3));
        assert_eq!(red.health().status, HealthStatus::Red);
    }

    #[test]
    fn smoke_run_all_checks() {
        // 全链聚合九面一锤定音。
        match run_all_checks() {
            Ok(summary) => assert!(!summary.is_empty(), "聚合摘要非空"),
            Err((code, msg)) => assert!(
                false,
                "全链自证红账 {:#06x}: {}",
                code, msg
            ),
        }
    }
}
