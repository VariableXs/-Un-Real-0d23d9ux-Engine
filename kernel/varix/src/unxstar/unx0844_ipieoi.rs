//! unx0844_ipieoi — UNX-F0844 IPI 的 EOI 语义与自 IPI 回环（LAPIC IPI 域）。
//!
//! 判据原文（施工书锚点 UNX-F0844，docs/Varix/CoRun Varix STAR II · Unxreal/
//! CoRun Varix STAR II · Unxreal.md 行 2483，全文抄录）：
//! **UNX-F0844 | IPI 的 EOI 语义与自 IPI 回环 | 260 | 已深化 | UNX-F0844-J1
//! IPI EOI 10 万次零残留（ISR 位清零断言），自 IPI 回环时延 µs 级落账**。
//!
//! 批定位原文（同文件 UNX-A2-B03 批引言，抄录定位句）：
//! 「AI-02 承办｜域账累计：本批 5,160 / 240,000｜嫁接源：Intel SDM Vol.3
//! §10.8/§10.9，只跟随｜上游：F0823 单点封装、F0805 spurious 入口、F0021
//! eoi_pending 账本｜防重：对 F0021（EOI 纲领）与 F0823（单点封装与五类分账）
//! 为分层深化——本批把五类向量的 EOI 行为逐类铺开、复挂/丢失/重投递等事故
//! 语义独立条目化」。
//!
//! 职责定位：IPI（核间中断）是唯一由软件亲手发射的中断——发射语义错一步，
//! 后果不是「中断丢失」就是「核假死」。本单把 IPI 的完整因果链做成**寄存器
//! 语义抽象建模 + 注入面**（不写真实端口 IO，真机 MMIO/MSR 归接线面）：
//!
//! ① ICR 双 32 位写序纪律：xAPIC 架构下 ICR 是两个 32 位寄存器——高字
//!    0x310（目标域：目的 APIC ID）、低字 0x300（触发域：向量/模式/级别/
//!    触发/简写域）。Intel SDM Vol.3 §10.6.1 明文：**对低字的写是投递的
//!    触发点**，软件必须先写高字、后写低字。施工书行文「先低字后高字」按
//!    SDM 语义归一为「触发域（低字）最后写」——先低字后高字即写序违例，
//!    拦截器当场立案（WriteOrderViolation），违例写不进入投递账。
//! ② IPI 全生命周期账（IpiLifecycle）：发送（ICR 写序通过）→投递
//!    （Delivery Status 空闲回落）→服务（ISR 置位）→EOI（ISR 清零）四阶段
//!    状态机，每阶段带抽象时戳，单向推进、越序拒绝。
//! ③ ICR 交付状态（Delivery Status 位，ICR 低字 bit12）：Sending 期间
//!    重写 ICR 是未定义行为源（SDM：交付状态为 Sending 时不得修改 ICR）——
//!    忙重写拦截（IcrBusyRewrite）+ 违例账。
//! ④ ISR 残留检测器：每次 EOI 后该向量 ISR 位**必须**清零（J1 判据的
//!    逐次断言面）；10 万次 EOI 全量断言零残留；任何残留 latch 红案且不可
//!    洗白（red_flags 单调）。
//! ⑤ 自 IPI 回环（SelfIpiLoop）：自发自收回环时延账（抽象时钟注入测量，
//!    µs 级落账）+ 回环不打断当前服务态语义（在服务优先级类 ≥ 回环向量类
//!    时挂 IRR 等待、不抢占——TPR/ISR 双闸）+ 回环向量合法域校验
//!    （0x20..=0xFE 且 ≠0xFF spurious；<0x20 为异常保留域）。
//! ⑥ 模式×EOI 需求真值表（const）：Fixed/Lowest/ExtINT 走 EOI；SMI/NMI/
//!    INIT/Startup 不置 ISR、不产生 EOI 要求（SDM §10.9）——模式分账
//!    逐模式独立计数，对非 EOI 模式写 EOI = 语义错位，拒绝并立案。
//! ⑦ 注入面三件套：写序违例注入 / EOI 残留注入 / ICR 忙重写注入——各
//!    10/10 次必检出，漏检 = InjectionMissed 红案（检测器敏感度自证）。
//! ⑧ 事件账 + health() + FNV 指纹 + 双跑确定性对拍：同种子两次全量跑
//!    （各 10 万次 EOI）指纹必须逐位一致——确定性是「零残留是结构保证而非
//!    碰运气」的证明面。
//!
//! 上游：F0821 LAPIC 基址/映射（寄存器偏移取其闭集）、F0838 xAPIC MMIO
//! 访问原语（真实写归原语）、F0841 deassert-EOI 顺序协议（电平面）；下游：
//! F0845 spurious 五态、F0847 EOI 丢失注入检测器（本单残留检测是其子面）、
//! F0861 IO-APIC（多核目的地扩展）。
//!
//! 设计要点：no_std+alloc；内核路径零 unwrap/expect/panic!/运行期 assert!
//! （编译期 const 断言可）；可失败全走 Result<T, IpiEoiError>，错误变体
//! 全带中文可操作 detail()；溢出显性 saturating/wrapping；>64KB 结构体不
//! 上栈（ISR/IRR 位图各 32B、服务簿 256×8B，全部小块）；LCG 确定性随机
//! （Numerical Recipes 常数，同种子同序列可 CI 回放）；FNV-1a 64 位指纹
//! 往返稳定。诊断码段 **0x1910~0x191F**（0x1900 被 vea39_msaa 占用，
//! 0x1920/0x1940/0x1960/0x1980 被 svstar2 占用，本段经全仓 grep 确认独占）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const IPI_EOI_VERSION: &str = "B03-ipieoi-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x1910 段，本单独占；0x1900 段为 vea39_msaa 所取）
// ---------------------------------------------------------------------------

/// 0x1910 ICR 写序违例（低字先于高字——低字写即触发投递，目标域未备位）。
pub const XC_ICR_WRITE_ORDER: u32 = 0x1910;
/// 0x1911 ICR 忙重写（Delivery Status=Sending 期写 ICR——SDM 明令禁止）。
pub const XC_ICR_BUSY_REWRITE: u32 = 0x1911;
/// 0x1912 EOI 残留（EOI 后 ISR 位未清零——J1 判据的红案信号）。
pub const XC_EOI_RESIDUE: u32 = 0x1912;
/// 0x1913 向量非法域（<0x20 异常保留 / 0xFF spurious / 域外）。
pub const XC_VECTOR_ILLEGAL: u32 = 0x1913;
/// 0x1914 模式 EOI 错位（对 SMI/NMI/INIT/Startup 写 EOI——模式分账错位）。
pub const XC_MODE_EOI_MISALIGN: u32 = 0x1914;
/// 0x1915 双重 EOI（向量已退出服务态再写 EOI——一次投递一次 EOI 纪律破）。
pub const XC_DOUBLE_EOI: u32 = 0x1915;
/// 0x1916 回环时延越界（超出 µs 级窗口上界——回环账 J1 拒收）。
pub const XC_LOOPBACK_LATENCY: u32 = 0x1916;
/// 0x1917 时钟回退（抽象时钟 tick 非单调——时戳账失真）。
pub const XC_CLOCK_REGRESSION: u32 = 0x1917;
/// 0x1918 账本失衡（发送/投递/服务/EOI 守恒式破——账面蒸发）。
pub const XC_LEDGER_UNBALANCED: u32 = 0x1918;
/// 0x1919 注入漏检（注入违规未被检测器捕获——敏感度不足）。
pub const XC_INJECTION_MISSED: u32 = 0x1919;
/// 0x191A 简写域非法组合（如 Startup×SelfOnly——INIT/SIPI 打自身无意义）。
pub const XC_SHORTHAND_ILLEGAL: u32 = 0x191A;
/// 0x191B 指纹失配（双跑确定性对拍不一致——确定性破）。
pub const XC_FINGERPRINT_MISMATCH: u32 = 0x191B;
/// 0x191C 容量耗尽（账本/挂起队列超出上限——内存纪律闸）。
pub const XC_CAPACITY_EXHAUSTED: u32 = 0x191C;
/// 0x191D 向量不在服务态（EOI 打在 ISR 位为空的向量上）。
pub const XC_NOT_IN_SERVICE: u32 = 0x191D;
/// 0x191E 红案已 latch（残留/风暴类红案未清——健康面必红）。
pub const XC_RED_FLAG_LATCHED: u32 = 0x191E;
/// 0x191F 无在途发送（对空 ICR 的投递/服务操作——生命周期越序）。
pub const XC_NO_OUTSTANDING: u32 = 0x191F;

// ---------------------------------------------------------------------------
// 常量（写死并注明出处）
// ---------------------------------------------------------------------------

/// xAPIC ICR 低字寄存器偏移（SDM Vol.3 Table 10-1：ICR offset 300H）。
pub const ICR_LOW_OFFSET: u32 = 0x300;
/// xAPIC ICR 高字寄存器偏移（SDM Vol.3 Table 10-1：ICR offset 310H）。
pub const ICR_HIGH_OFFSET: u32 = 0x310;
/// xAPIC EOI 寄存器偏移（SDM Vol.3 Table 10-1：EOI offset 0B0H）。
pub const EOI_REG_OFFSET: u32 = 0x0B0;
/// xAPIC ISR 基偏移（SDM Table 10-1：ISR 100H..170H，8×32 位、16 字节步距）。
pub const ISR_BASE_OFFSET: u32 = 0x100;
/// xAPIC IRR 基偏移（SDM Table 10-1：IRR 200H..270H，同 ISR 布局）。
pub const IRR_BASE_OFFSET: u32 = 0x200;
/// ISR/IRR 寄存器步距（SDM Table 10-1：连续寄存器间 16 字节对齐步距）。
pub const ISR_IRR_STRIDE: u32 = 0x10;
/// ICR 低字 Delivery Status 位号（SDM Table 10-3/10-4：bit 12）。
pub const ICR_DELIVERY_STATUS_BIT: u32 = 12;
/// ICR 低字投递模式域位号（bit 10:8，SDM Table 10-4）。
pub const ICR_MODE_SHIFT: u32 = 8;
/// ICR 低字级别域位号（bit 14：1=assert / 0=deassert，SDM Table 10-4）。
pub const ICR_LEVEL_BIT: u32 = 14;
/// ICR 低字触发模式域位号（bit 15：0=edge / 1=level，SDM Table 10-4）。
pub const ICR_TRIGGER_BIT: u32 = 15;
/// ICR 低字简写域位号（bit 19:18，SDM Table 10-4）。
pub const ICR_SHORTHAND_SHIFT: u32 = 18;
/// spurious 向量（SDM §10.9：0xFF 为 spurious 向量惯例位，不参与 EOI）。
pub const SPURIOUS_VECTOR: u8 = 0xFF;
/// 向量合法域下界（SDM §6.15 Table 6-4：向量 0..31 为异常/Intel 保留）。
pub const VECTOR_LEGAL_LO: u8 = 0x20;
/// 向量合法域上界（0xFF 为 spurious；Fixed 回环合法域 0x20..=0xFE）。
pub const VECTOR_LEGAL_HI: u8 = 0xFE;
/// 合法域跨度（0xFE-0x20+1 = 223——LCG 均匀取样的模数）。
pub const VECTOR_SPAN: u32 = (VECTOR_LEGAL_HI as u32) - (VECTOR_LEGAL_LO as u32) + 1;
/// 抽象时钟标定：1 µs = 100 tick（LAPIC 总线参考档 100 MHz；SDM §10.5.4
/// LAPIC 计时源为处理器总线时钟，抽象档取整 100 tick/µs 便于 µs 落账）。
pub const CLOCK_TICKS_PER_US: u64 = 100;
/// 自 IPI 回环时延 µs 级窗口上界（J1「µs 级」量化：>50 µs 即越出回环
/// 语义档——真实 LAPIC 回环为总线往返 + 处理器取指量级，QEMU/真机均在
/// 个位数 µs 内；50 为留 10 倍余量的判级线）。
pub const LOOPBACK_LATENCY_MAX_US: u64 = 50;
/// J1 判据 campaign 次数（10 万次 EOI 零残留——判据原文数值写死）。
pub const CAMPAIGN_ITERATIONS: u64 = 100_000;
/// 账本容量上限（条目级内存纪律闸：10 万 campaign + 回环 + 模式分账
/// 余量后取 2^20；超限拒绝而非无界增长——no_std 内存纪律）。
pub const LEDGER_CAP: usize = 1_048_576;
/// 挂起（IRR 等待）队列上限（优先级挂起语义的有界实现；回环压测远低于此）。
pub const HELD_CAP: usize = 4096;
/// 注入账容量上限（注入面板有界；10/10 注入 ×3 类远低于此）。
pub const INJECTION_CAP: usize = 4096;
/// 回环账容量上限（回环压测 1 万次级，留 10 倍余量）。
pub const LOOPBACK_CAP: usize = 131_072;
/// LCG 乘数（Numerical Recipes：a=1664525，32 位 LCG 标准常数）。
pub const LCG_A: u32 = 1_664_525;
/// LCG 增量（Numerical Recipes：c=1013904223，与 a 配对全周期）。
pub const LCG_C: u32 = 1_013_904_223;
/// FNV-1a 64 位偏移基（FNV 公开参考值）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数（FNV 公开参考值）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// 指纹 schema 标识（本单账本 schema 版本编码，进指纹首拍防跨版误对拍）。
pub const FINGERPRINT_SCHEMA: u64 = 0x554E_5838_3434_3401;

// ---------------------------------------------------------------------------
// 投递模式 × EOI 需求真值表（const）
// ---------------------------------------------------------------------------

/// LAPIC ICR 投递模式（SDM Vol.3 Table 10-4 Delivery Mode 域 3 位码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeliveryMode {
    /// 000 Fixed——固定向量投递，需要 EOI。
    Fixed,
    /// 001 Lowest Priority——最低优先级投递，需要 EOI。
    LowestPriority,
    /// 010 SMI——系统管理中断，不置 ISR、不走 EOI。
    Smi,
    /// 100 NMI——不可屏蔽中断，不置 ISR、不走 EOI。
    Nmi,
    /// 101 INIT——处理器初始化 IPI，不走 EOI。
    Init,
    /// 110 Startup——SIPI 启动 IPI（向量为启动页号），不走 EOI。
    Startup,
    /// 111 ExtINT——外部（PIC 级联）中断，经 LAPIC 投递时走 EOI。
    ExtInt,
}

impl DeliveryMode {
    /// 3 位码（写进 ICR 低字 bit 10:8）。
    pub const fn code(self) -> u8 {
        match self {
            DeliveryMode::Fixed => 0b000,
            DeliveryMode::LowestPriority => 0b001,
            DeliveryMode::Smi => 0b010,
            DeliveryMode::Nmi => 0b100,
            DeliveryMode::Init => 0b101,
            DeliveryMode::Startup => 0b110,
            DeliveryMode::ExtInt => 0b111,
        }
    }

    /// 3 位码反解（0b011 保留码返回 None——架构未定义）。
    pub const fn from_code(code: u8) -> Option<DeliveryMode> {
        match code & 0b111 {
            0b000 => Some(DeliveryMode::Fixed),
            0b001 => Some(DeliveryMode::LowestPriority),
            0b010 => Some(DeliveryMode::Smi),
            0b100 => Some(DeliveryMode::Nmi),
            0b101 => Some(DeliveryMode::Init),
            0b110 => Some(DeliveryMode::Startup),
            0b111 => Some(DeliveryMode::ExtInt),
            _ => None,
        }
    }

    /// 本模式是否要求 EOI（查 const 真值表）。
    pub const fn eoi_required(self) -> bool {
        EOI_REQUIRED_BY_MODE[self.code() as usize]
    }

    /// 中文模式名（分账与导出面）。
    pub const fn name(self) -> &'static str {
        match self {
            DeliveryMode::Fixed => "Fixed",
            DeliveryMode::LowestPriority => "LowestPriority",
            DeliveryMode::Smi => "SMI",
            DeliveryMode::Nmi => "NMI",
            DeliveryMode::Init => "INIT",
            DeliveryMode::Startup => "Startup",
            DeliveryMode::ExtInt => "ExtINT",
        }
    }
}

/// 模式×EOI 需求真值表（下标 = 投递模式 3 位码）。
///
/// 来源：Intel SDM Vol.3 §10.9「EOI」：Fixed 与 Lowest-Priority 类中断
/// handler 必须写 EOI；NMI/SMI/INIT/Startup 不更新 ISR/IRR、不产生 EOI
/// 要求；ExtINT 经 LAPIC 投递时 handler 写 EOI 会触发对 I/O APIC 的级联
/// EOI 广播（故为 true）。0b011 保留码按 false（不可投递即无 EOI）。
pub const EOI_REQUIRED_BY_MODE: [bool; 8] = [
    true,  // 000 Fixed
    true,  // 001 Lowest Priority
    false, // 010 SMI
    false, // 011 保留（架构未定义投递模式）
    false, // 100 NMI
    false, // 101 INIT
    false, // 110 Startup
    true,  // 111 ExtINT
];

/// ICR 低字简写域（SDM Table 10-4 Destination Shorthand bit 19:18）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shorthand {
    /// 00 目的地域指定（bit 31:24 为物理/逻辑目的 APIC ID）。
    DestField,
    /// 01 仅自身（自 IPI 回环的合法简写）。
    SelfOnly,
    /// 10 全体含自身。
    AllIncludingSelf,
    /// 11 全体不含自身。
    AllExcludingSelf,
}

impl Shorthand {
    /// 2 位码。
    pub const fn code(self) -> u8 {
        match self {
            Shorthand::DestField => 0b00,
            Shorthand::SelfOnly => 0b01,
            Shorthand::AllIncludingSelf => 0b10,
            Shorthand::AllExcludingSelf => 0b11,
        }
    }

    /// 2 位码反解。
    pub const fn from_code(code: u8) -> Shorthand {
        match code & 0b11 {
            0b01 => Shorthand::SelfOnly,
            0b10 => Shorthand::AllIncludingSelf,
            0b11 => Shorthand::AllExcludingSelf,
            _ => Shorthand::DestField,
        }
    }
}

// ---------------------------------------------------------------------------
// ICR 低字组包 / 解包（bit 域与 SDM Table 10-4 逐位对齐）
// ---------------------------------------------------------------------------

/// ICR 低字解包结果（bit 域语义视图）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IcrLowFields {
    /// bit 7:0 投递向量（Startup 模式下为启动页号）。
    pub vector: u8,
    /// bit 10:8 投递模式。
    pub mode: DeliveryMode,
    /// bit 14 级别域（true=assert / false=deassert）。
    pub level_assert: bool,
    /// bit 15 触发模式（true=level / false=edge）。
    pub trigger_level: bool,
    /// bit 19:18 简写域。
    pub shorthand: Shorthand,
}

/// 组包 ICR 低字（与 SDM Table 10-4 位域一一对应）。
pub fn compose_icr_low(
    vector: u8,
    mode: DeliveryMode,
    level_assert: bool,
    trigger_level: bool,
    shorthand: Shorthand,
) -> u32 {
    let mut w = vector as u32;
    w |= (mode.code() as u32) << ICR_MODE_SHIFT;
    if level_assert {
        w |= 1u32 << ICR_LEVEL_BIT;
    }
    if trigger_level {
        w |= 1u32 << ICR_TRIGGER_BIT;
    }
    w |= (shorthand.code() as u32) << ICR_SHORTHAND_SHIFT;
    w
}

/// 解包 ICR 低字（保留码模式回退 Fixed 并由调用方账面记录）。
pub fn parse_icr_low(word: u32) -> IcrLowFields {
    let mode_code = ((word >> ICR_MODE_SHIFT) & 0b111) as u8;
    let mode = match DeliveryMode::from_code(mode_code) {
        Some(m) => m,
        None => DeliveryMode::Fixed,
    };
    IcrLowFields {
        vector: (word & 0xFF) as u8,
        mode,
        level_assert: (word >> ICR_LEVEL_BIT) & 1 == 1,
        trigger_level: (word >> ICR_TRIGGER_BIT) & 1 == 1,
        shorthand: Shorthand::from_code(((word >> ICR_SHORTHAND_SHIFT) & 0b11) as u8),
    }
}

// ---------------------------------------------------------------------------
// 错误类型（每变体带中文可操作诊断 detail()）
// ---------------------------------------------------------------------------

/// IPI EOI 域错误全集。每个变体对应一个诊断码（code()），detail() 给出
/// 中文可操作处置建议。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiEoiError {
    /// ICR 写序违例：低字先于高字（低字写即触发投递，目标域未备位）。
    WriteOrderViolation,
    /// ICR 忙重写：Delivery Status=Sending 期间写 ICR。
    IcrBusyRewrite,
    /// EOI 残留：EOI 后该向量 ISR 位未清零（J1 红案信号）。
    EoiResidue { vector: u8 },
    /// 向量非法域：<0x20 异常保留、0xFF spurious 或域外。
    InvalidVector { vector: u8 },
    /// 模式 EOI 错位：对不走 EOI 的模式（SMI/NMI/INIT/Startup）写 EOI。
    ModeEoiNotApplicable { mode_code: u8 },
    /// 双重 EOI：向量已退出服务态再写 EOI。
    DoubleEoi { vector: u8 },
    /// 回环时延越界：超出 µs 级窗口上界。
    LoopbackLatencyOut { latency_us: u64, bound_us: u64 },
    /// 时钟回退：抽象时钟 tick 非单调。
    ClockRegression { prev: u64, next: u64 },
    /// 账本失衡：发送/投递/服务/EOI 守恒式破。
    LedgerUnbalanced { sent: u64, retired: u64 },
    /// 注入漏检：注入违规未被检测器捕获。
    InjectionMissed { kind_code: u8 },
    /// 简写域非法组合（如 Startup×SelfOnly）。
    ShorthandIllegal { shorthand_code: u8, mode_code: u8 },
    /// 指纹失配：双跑确定性对拍不一致。
    FingerprintMismatch { left: u64, right: u64 },
    /// 容量耗尽：账本/队列超上限。
    CapacityExhausted,
    /// 向量不在服务态：EOI 打在 ISR 位为空且从未服务过的向量上。
    VectorNotInService { vector: u8 },
    /// 红案已 latch：残留类红案在案，健康面必红。
    RedFlagLatched { code: u32 },
    /// 无在途发送：对空 ICR 的投递/服务操作。
    NoOutstandingSend,
}

impl IpiEoiError {
    /// 中文可操作诊断（每变体必给处置建议）。
    pub fn detail(&self) -> &'static str {
        match self {
            IpiEoiError::WriteOrderViolation => "ICR 写序违例：xAPIC 下必须先写高字（0x310 目标域）后写低字（0x300 触发域）——低字写即触发投递（SDM §10.6.1）。本次违例写已被拦截未入账；请检查发送路径是否把低字写排到了高字之前。",
            IpiEoiError::IcrBusyRewrite => "ICR 忙重写：Delivery Status 仍为 Sending（ICR 低字 bit12）时重写 ICR 是 SDM 明令禁止的操作。请先等待上一次投递落账（settle_delivery）使状态回 Idle，再发起新事务。",
            IpiEoiError::EoiResidue { .. } => "EOI 残留：EOI 写之后该向量 ISR 位仍为 1——EOI 丢失或被吞。残留会阻断同级及更低优先级中断（PPR 卡死），请核对 EOI 写路径与寄存器偏移（0x0B0），并触发 ISR 残留扫描定位向量。",
            IpiEoiError::InvalidVector { .. } => "向量非法域：IPI 向量必须在 0x20..=0xFE（0x00..0x1F 为 CPU 异常/Intel 保留域，0xFF 为 spurious）。请改用合法域向量后重发。",
            IpiEoiError::ModeEoiNotApplicable { .. } => "模式 EOI 错位：SMI/NMI/INIT/Startup 投递不更新 ISR/IRR、不产生 EOI 要求（SDM §10.9）。对这类向量写 EOI 属语义错位，请按模式分账改走无 EOI 完成路径。",
            IpiEoiError::DoubleEoi { .. } => "双重 EOI：该向量 ISR 位已清零（此前已 EOI）。一次投递一次 EOI——重复 EOI 会误触发对其他向量的重投递裁决。请核对 handler 是否重复退出路径。",
            IpiEoiError::LoopbackLatencyOut { .. } => "回环时延越界：自 IPI 回环往返超出 µs 级窗口（50µs）。请检查抽象时钟标定（CLOCK_TICKS_PER_US=100）与注入的 tick 序列是否把非回环等待计入往返。",
            IpiEoiError::ClockRegression { .. } => "时钟回退：注入的 tick 小于上一时戳——时戳账失真、时延不可算。请保证时钟单调递增后再重放。",
            IpiEoiError::LedgerUnbalanced { .. } => "账本失衡：发送/投递/服务/EOI 守恒式不成立（在途+挂起+已退休 ≠ 已发送）。请先跑 ledger_summary() 对各分账，定位蒸发段后重放。",
            IpiEoiError::InjectionMissed { .. } => "注入漏检：注入的违规操作未被检测器捕获——检测面存在盲区。请核对该注入路径是否绕过了单点封装（tracker/检测器直连面）。",
            IpiEoiError::ShorthandIllegal { .. } => "简写域非法组合：INIT/Startup 打自身无架构意义（INIT-SIPI 序只打 AP）。请改用 DestField 或广播简写。",
            IpiEoiError::FingerprintMismatch { .. } => "指纹失配：同种子双跑 FNV 指纹不一致——确定性破坏。请核对两跑之间是否有外部状态（时钟/TOR 种子）差异，再逐段对账。",
            IpiEoiError::CapacityExhausted => "容量耗尽：账本/挂起队列达到上限。no_std 下不允许无界增长——请分批落账或提高上限常量后重跑。",
            IpiEoiError::VectorNotInService { .. } => "向量不在服务态：EOI 打在 ISR 位为空且从未服务过的向量上。请先确认投递与服务两阶段已落账（settle_delivery + enter_service）。",
            IpiEoiError::RedFlagLatched { .. } => "红案已 latch：残留/漏检类红案在案不可洗白。健康面必红属设计——请先处置红案根因，再以新机重放。",
            IpiEoiError::NoOutstandingSend => "无在途发送：对空 ICR 的投递/服务操作。请先 send_ipi 产生在途事务，再按 发送→投递→服务→EOI 次序推进。",
        }
    }

    /// 对应诊断码（0x1910 段；段内 16 码与本枚举一一对应）。
    pub const fn code(&self) -> u32 {
        match self {
            IpiEoiError::WriteOrderViolation => XC_ICR_WRITE_ORDER,
            IpiEoiError::IcrBusyRewrite => XC_ICR_BUSY_REWRITE,
            IpiEoiError::EoiResidue { .. } => XC_EOI_RESIDUE,
            IpiEoiError::InvalidVector { .. } => XC_VECTOR_ILLEGAL,
            IpiEoiError::ModeEoiNotApplicable { .. } => XC_MODE_EOI_MISALIGN,
            IpiEoiError::DoubleEoi { .. } => XC_DOUBLE_EOI,
            IpiEoiError::LoopbackLatencyOut { .. } => XC_LOOPBACK_LATENCY,
            IpiEoiError::ClockRegression { .. } => XC_CLOCK_REGRESSION,
            IpiEoiError::LedgerUnbalanced { .. } => XC_LEDGER_UNBALANCED,
            IpiEoiError::InjectionMissed { .. } => XC_INJECTION_MISSED,
            IpiEoiError::ShorthandIllegal { .. } => XC_SHORTHAND_ILLEGAL,
            IpiEoiError::FingerprintMismatch { .. } => XC_FINGERPRINT_MISMATCH,
            IpiEoiError::CapacityExhausted => XC_CAPACITY_EXHAUSTED,
            IpiEoiError::VectorNotInService { .. } => XC_NOT_IN_SERVICE,
            IpiEoiError::RedFlagLatched { .. } => XC_RED_FLAG_LATCHED,
            IpiEoiError::NoOutstandingSend => XC_NO_OUTSTANDING,
        }
    }
}

// ---------------------------------------------------------------------------
// 向量位图（ISR/IRR 寄存器文件建模：8×32 位、向量 v → 字 v>>5 位 v&31）
// ---------------------------------------------------------------------------

/// 256 向量位图——ISR/IRR 寄存器文件的抽象（SDM Table 10-1 布局：8 个
/// 32 位寄存器，位序与向量号线性对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VectorBitmap {
    words: [u32; 8],
}

impl VectorBitmap {
    /// 全零位图（const 可构造）。
    pub const fn new() -> VectorBitmap {
        VectorBitmap { words: [0u32; 8] }
    }

    /// 置位（置 ISR/IRR 对应向量位）。
    pub fn set(&mut self, vector: u8) {
        self.words[(vector >> 5) as usize] |= 1u32 << (vector & 31);
    }

    /// 清位（EOI 语义：清对应向量位）。
    pub fn clear(&mut self, vector: u8) {
        self.words[(vector >> 5) as usize] &= !(1u32 << (vector & 31));
    }

    /// 测位。
    pub fn test(&self, vector: u8) -> bool {
        (self.words[(vector >> 5) as usize] >> (vector & 31)) & 1 == 1
    }

    /// 最高置位向量（None=全空）——对应「当前最高在服务向量」的硬件裁决序。
    pub fn highest(&self) -> Option<u8> {
        let mut w = 7usize;
        while w > 0 && self.words[w] == 0 {
            w -= 1;
        }
        if self.words[w] == 0 {
            return None;
        }
        let mut bit = 31usize;
        while bit > 0 && (self.words[w] >> bit) & 1 == 0 {
            bit -= 1;
        }
        Some(((w << 5) | bit) as u8)
    }

    /// 置位总数（守恒式与健康面用）。
    pub fn count(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < 8 {
            n = n.saturating_add(self.words[i].count_ones() as u64);
            i += 1;
        }
        n
    }

    /// 全量残留清单（按向量号升序）——残留扫描（F0850 的本单子面）。
    pub fn residue_list(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut w = 0usize;
        while w < 8 {
            let mut b = 0u32;
            while b < 32 {
                if (self.words[w] >> b) & 1 == 1 {
                    out.push(((w << 5) | b as usize) as u8);
                }
                b += 1;
            }
            w += 1;
        }
        out
    }

    /// 寄存器文件快照（8×32 位原样——指纹与对拍面）。
    pub fn words(&self) -> [u32; 8] {
        self.words
    }
}

// ---------------------------------------------------------------------------
// ICR 写序纪律与交付状态（发送面的单点封装）
// ---------------------------------------------------------------------------

/// ICR 写序违例计数的内部视图（供机器账面读取）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IcrStats {
    /// 高字（0x310）写次数。
    pub high_writes: u64,
    /// 低字（0x300）提交次数（= 触发投递次数）。
    pub low_commits: u64,
    /// 写序违例拦截次数。
    pub order_violations: u64,
    /// 忙重写拦截次数。
    pub busy_rewrites: u64,
    /// Delivery Status 当前是否 Sending。
    pub sending: bool,
}

/// ICR 双 32 位写序纪律 + Delivery Status 建模。
///
/// 事务纪律（SDM §10.6.1）：每次发送 = 高字写（目标域备位）→ 低字写
/// （触发投递）。低字先于高字 = WriteOrderViolation；Delivery Status=
/// Sending 期任何 ICR 写 = IcrBusyRewrite。两类违例都**不进入投递账**。
#[derive(Clone, Debug)]
pub struct IcrWriteTracker {
    txn_open: bool,
    sending: bool,
    high_writes: u64,
    low_commits: u64,
    order_violations: u64,
    busy_rewrites: u64,
}

impl IcrWriteTracker {
    /// 新建：Idle、无事务。
    pub const fn new() -> IcrWriteTracker {
        IcrWriteTracker {
            txn_open: false,
            sending: false,
            high_writes: 0,
            low_commits: 0,
            order_violations: 0,
            busy_rewrites: 0,
        }
    }

    /// 写 ICR 高字（0x310 目标域）。Sending 期拒绝（忙重写）。
    pub fn write_high(&mut self, _dest_word: u32) -> Result<(), IpiEoiError> {
        if self.sending {
            self.busy_rewrites = self.busy_rewrites.saturating_add(1);
            return Err(IpiEoiError::IcrBusyRewrite);
        }
        self.txn_open = true;
        self.high_writes = self.high_writes.saturating_add(1);
        Ok(())
    }

    /// 写 ICR 低字（0x300 触发域）——**触发投递的一笔**。要求高字已在本
    /// 事务中先写；否则写序违例拦截。
    pub fn write_low(&mut self, word: u32) -> Result<IcrLowFields, IpiEoiError> {
        if self.sending {
            self.busy_rewrites = self.busy_rewrites.saturating_add(1);
            return Err(IpiEoiError::IcrBusyRewrite);
        }
        if !self.txn_open {
            self.order_violations = self.order_violations.saturating_add(1);
            return Err(IpiEoiError::WriteOrderViolation);
        }
        let fields = parse_icr_low(word);
        self.txn_open = false;
        self.sending = true;
        self.low_commits = self.low_commits.saturating_add(1);
        Ok(fields)
    }

    /// 交付落定：Delivery Status Sending→Idle（下一次 ICR 写的合法前提）。
    pub fn settle(&mut self) {
        self.sending = false;
    }

    /// 注入面：强制置 Sending（模拟在途投递悬挂）。
    pub fn force_busy(&mut self) {
        self.sending = true;
    }

    /// 错误恢复：强制回 Idle（生命周期越序后的兜底，只复位状态不洗账）。
    pub fn recover_idle(&mut self) {
        self.sending = false;
        self.txn_open = false;
    }

    /// 统计视图。
    pub fn stats(&self) -> IcrStats {
        IcrStats {
            high_writes: self.high_writes,
            low_commits: self.low_commits,
            order_violations: self.order_violations,
            busy_rewrites: self.busy_rewrites,
            sending: self.sending,
        }
    }
}

// ---------------------------------------------------------------------------
// ISR 残留检测器（J1「ISR 位清零断言」的逐次 + 全量两面）
// ---------------------------------------------------------------------------

/// 单条残留红案。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResidueLatch {
    /// 发生残留时的生命周期序号。
    pub seq: u64,
    /// 残留向量。
    pub vector: u8,
    /// 抽象时钟时戳。
    pub tick: u64,
}

/// ISR 残留检测器。
///
/// 两面：①逐次面——每次 EOI 后对该向量 ISR 位断言（100k campaign 的
/// 逐次断言即此）；②全量面——8×32 位寄存器文件全扫（F0850 子面）。
/// 任何残留：计数、latch 红案（red=true 不可洗白）、可查清单。
#[derive(Clone, Debug)]
pub struct ResidueDetector {
    checks: u64,
    residues: u64,
    red: bool,
    latched: Vec<ResidueLatch>,
}

impl ResidueDetector {
    /// 新建检测器。
    pub const fn new() -> ResidueDetector {
        ResidueDetector {
            checks: 0,
            residues: 0,
            red: false,
            latched: Vec::new(),
        }
    }

    /// 逐次面：EOI 后单向量 ISR 位清零断言。返回 Err(EoiResidue) 即红案。
    pub fn check_after_eoi(
        &mut self,
        words: &[u32; 8],
        vector: u8,
        seq: u64,
        tick: u64,
    ) -> Result<(), IpiEoiError> {
        self.checks = self.checks.saturating_add(1);
        let bit_set = (words[(vector >> 5) as usize] >> (vector & 31)) & 1 == 1;
        if bit_set {
            self.residues = self.residues.saturating_add(1);
            self.red = true;
            self.latched.push(ResidueLatch { seq, vector, tick });
            return Err(IpiEoiError::EoiResidue { vector });
        }
        Ok(())
    }

    /// 全量面：扫 256 向量，返回本次扫出的残留数（并逐条 latch）。
    pub fn scan_all(&mut self, words: &[u32; 8], tick: u64) -> u64 {
        let mut found = 0u64;
        let mut w = 0usize;
        while w < 8 {
            let mut b = 0u32;
            while b < 32 {
                if (words[w] >> b) & 1 == 1 {
                    found = found.saturating_add(1);
                    self.residues = self.residues.saturating_add(1);
                    self.red = true;
                    self.latched.push(ResidueLatch {
                        seq: 0,
                        vector: ((w << 5) | b as usize) as u8,
                        tick,
                    });
                }
                b += 1;
            }
            w += 1;
        }
        found
    }

    /// 已执行的清零断言次数（J1 覆盖率=10 万的证据面）。
    pub fn check_count(&self) -> u64 {
        self.checks
    }

    /// 累计残留次数。
    pub fn residue_count(&self) -> u64 {
        self.residues
    }

    /// 红案是否在案（一旦 latch 不可洗白）。
    pub fn is_red(&self) -> bool {
        self.red
    }

    /// 红案清单快照。
    pub fn latches(&self) -> Vec<ResidueLatch> {
        self.latched.clone()
    }
}

// ---------------------------------------------------------------------------
// IPI 全生命周期账（四阶段状态机 + 每阶段时戳 + 模式分账）
// ---------------------------------------------------------------------------

/// 生命周期阶段（单向：Sent→Delivered→Serviced→EoiCompleted；
/// HeldPending 为优先级挂起的旁路态，被调度后回到 Serviced）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IpiPhase {
    /// 已发送（ICR 低字提交，Delivery Status=Sending）。
    Sent,
    /// 已投递（Delivery Status 回 Idle，ICR 事务闭合）。
    Delivered,
    /// 服务中（ISR 位置位）或无 EOI 模式已直接完成。
    Serviced,
    /// 已挂起（优先级/TPR 闸：IRR 等待，不抢占当前服务态）。
    HeldPending,
    /// 已终结（ISR 位清零 / 无 EOI 模式语义完成）。
    EoiCompleted,
}

/// 单条 IPI 生命周期记录（四阶段时戳齐备即账目完整）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LifecycleRecord {
    /// 生命周期序号（= 账本条目下标，单调）。
    pub seq: u64,
    /// 投递向量。
    pub vector: u8,
    /// 投递模式 3 位码。
    pub mode_code: u8,
    /// 简写域 2 位码。
    pub shorthand_code: u8,
    /// 是否自 IPI 回环。
    pub self_loop: bool,
    /// 是否无 EOI 语义完成（SMI/NMI/INIT/Startup）。
    pub no_eoi: bool,
    /// 当前阶段。
    pub phase: IpiPhase,
    /// 发送时戳。
    pub sent_tick: u64,
    /// 投递时戳。
    pub delivered_tick: u64,
    /// 服务时戳。
    pub serviced_tick: u64,
    /// EOI（或无 EOI 完成）时戳。
    pub eoi_tick: u64,
}

/// 生命周期账本：记录 + 四类总量 + 模式分账（模式×阶段矩阵的投影）。
#[derive(Clone, Debug)]
pub struct IpiLedger {
    records: Vec<LifecycleRecord>,
    sent_total: u64,
    delivered_total: u64,
    serviced_total: u64,
    eoi_total: u64,
    no_eoi_completions: u64,
    held_total: u64,
    dispatched_total: u64,
    spurious_rejected: u64,
    invalid_rejected: u64,
    sent_by_mode: [u64; 8],
    eoi_by_mode: [u64; 8],
    no_eoi_by_mode: [u64; 8],
}

impl IpiLedger {
    /// 空账本。
    pub const fn new() -> IpiLedger {
        IpiLedger {
            records: Vec::new(),
            sent_total: 0,
            delivered_total: 0,
            serviced_total: 0,
            eoi_total: 0,
            no_eoi_completions: 0,
            held_total: 0,
            dispatched_total: 0,
            spurious_rejected: 0,
            invalid_rejected: 0,
            sent_by_mode: [0; 8],
            eoi_by_mode: [0; 8],
            no_eoi_by_mode: [0; 8],
        }
    }

    /// 推入新记录（Sent 态），返回序号。容量闸超限拒绝。
    pub fn push_record(
        &mut self,
        vector: u8,
        mode_code: u8,
        shorthand_code: u8,
        sent_tick: u64,
        self_loop: bool,
    ) -> Result<u64, IpiEoiError> {
        if self.records.len() >= LEDGER_CAP {
            return Err(IpiEoiError::CapacityExhausted);
        }
        let seq = self.records.len() as u64;
        self.records.push(LifecycleRecord {
            seq,
            vector,
            mode_code,
            shorthand_code,
            self_loop,
            no_eoi: false,
            phase: IpiPhase::Sent,
            sent_tick,
            delivered_tick: 0,
            serviced_tick: 0,
            eoi_tick: 0,
        });
        self.sent_total = self.sent_total.saturating_add(1);
        self.sent_by_mode[(mode_code & 0b111) as usize] =
            self.sent_by_mode[(mode_code & 0b111) as usize].saturating_add(1);
        Ok(seq)
    }

    /// 记录阶段推进：投递。
    pub fn rec_set_delivered(&mut self, seq: u64, tick: u64) -> Result<(), IpiEoiError> {
        match self.records.get_mut(seq as usize) {
            Some(r) => {
                r.phase = IpiPhase::Delivered;
                r.delivered_tick = tick;
                self.delivered_total = self.delivered_total.saturating_add(1);
                Ok(())
            }
            None => Err(IpiEoiError::LedgerUnbalanced {
                sent: seq,
                retired: 0,
            }),
        }
    }

    /// 记录阶段推进：服务（ISR 置位）。
    pub fn rec_set_serviced(&mut self, seq: u64, tick: u64) -> Result<(), IpiEoiError> {
        match self.records.get_mut(seq as usize) {
            Some(r) => {
                r.phase = IpiPhase::Serviced;
                r.serviced_tick = tick;
                self.serviced_total = self.serviced_total.saturating_add(1);
                Ok(())
            }
            None => Err(IpiEoiError::LedgerUnbalanced {
                sent: seq,
                retired: 0,
            }),
        }
    }

    /// 记录阶段推进：挂起（IRR 等待）。
    pub fn rec_set_held(&mut self, seq: u64) -> Result<(), IpiEoiError> {
        match self.records.get_mut(seq as usize) {
            Some(r) => {
                r.phase = IpiPhase::HeldPending;
                self.held_total = self.held_total.saturating_add(1);
                Ok(())
            }
            None => Err(IpiEoiError::LedgerUnbalanced {
                sent: seq,
                retired: 0,
            }),
        }
    }

    /// 记录阶段推进：终结（EOI 清零或无 EOI 语义完成）。
    pub fn rec_set_completed(&mut self, seq: u64, tick: u64, no_eoi: bool) -> Result<(), IpiEoiError> {
        match self.records.get_mut(seq as usize) {
            Some(r) => {
                r.phase = IpiPhase::EoiCompleted;
                r.eoi_tick = tick;
                r.no_eoi = no_eoi;
                if no_eoi {
                    self.no_eoi_completions = self.no_eoi_completions.saturating_add(1);
                    self.no_eoi_by_mode[(r.mode_code & 0b111) as usize] =
                        self.no_eoi_by_mode[(r.mode_code & 0b111) as usize].saturating_add(1);
                } else {
                    self.eoi_total = self.eoi_total.saturating_add(1);
                    self.eoi_by_mode[(r.mode_code & 0b111) as usize] =
                        self.eoi_by_mode[(r.mode_code & 0b111) as usize].saturating_add(1);
                }
                Ok(())
            }
            None => Err(IpiEoiError::LedgerUnbalanced {
                sent: seq,
                retired: 0,
            }),
        }
    }

    /// 挂起被调度回服务态（HeldPending→Serviced，held 总量只增不减作累计）。
    pub fn rec_rehold_to_serviced(&mut self, seq: u64, tick: u64) -> Result<(), IpiEoiError> {
        match self.records.get_mut(seq as usize) {
            Some(r) => {
                r.phase = IpiPhase::Serviced;
                r.serviced_tick = tick;
                self.serviced_total = self.serviced_total.saturating_add(1);
                self.dispatched_total = self.dispatched_total.saturating_add(1);
                Ok(())
            }
            None => Err(IpiEoiError::LedgerUnbalanced {
                sent: seq,
                retired: 0,
            }),
        }
    }

    /// 账目快照引用。
    pub fn records(&self) -> &[LifecycleRecord] {
        &self.records
    }

    /// 发送总量。
    pub fn sent_total(&self) -> u64 {
        self.sent_total
    }

    /// 投递总量。
    pub fn delivered_total(&self) -> u64 {
        self.delivered_total
    }

    /// 服务总量（含挂起后被调度的）。
    pub fn serviced_total(&self) -> u64 {
        self.serviced_total
    }

    /// EOI 总量。
    pub fn eoi_total(&self) -> u64 {
        self.eoi_total
    }

    /// 无 EOI 完成总量（SMI/NMI/INIT/Startup 分账）。
    pub fn no_eoi_completions(&self) -> u64 {
        self.no_eoi_completions
    }

    /// 挂起累计。
    pub fn held_total(&self) -> u64 {
        self.held_total
    }

    /// 挂起被调度累计。
    pub fn dispatched_total(&self) -> u64 {
        self.dispatched_total
    }

    /// spurious EOI 拒绝计数。
    pub fn spurious_rejected(&self) -> u64 {
        self.spurious_rejected
    }

    /// 非法向量拒绝计数。
    pub fn invalid_rejected(&self) -> u64 {
        self.invalid_rejected
    }

    /// 模式分账：按模式发送。
    pub fn sent_by_mode(&self) -> [u64; 8] {
        self.sent_by_mode
    }

    /// 模式分账：按模式 EOI。
    pub fn eoi_by_mode(&self) -> [u64; 8] {
        self.eoi_by_mode
    }

    /// 模式分账：按模式无 EOI 完成。
    pub fn no_eoi_by_mode(&self) -> [u64; 8] {
        self.no_eoi_by_mode
    }

    /// 守恒式：在服务 + 挂起 + 已退休 + 在途 = 已发送，且账面在服务数与
    /// ISR 位图实测数互证一致。
    pub fn balance_ok(&self, in_service: u64, held_now: u64, open_outstanding: u64) -> bool {
        let retired = self.eoi_total.saturating_add(self.no_eoi_completions);
        let in_isr = self.serviced_total.saturating_sub(self.eoi_total);
        let lhs = in_isr
            .saturating_add(held_now)
            .saturating_add(retired)
            .saturating_add(open_outstanding);
        lhs == self.sent_total && in_isr == in_service
    }
}

// ---------------------------------------------------------------------------
// 自 IPI 回环账（µs 级时延落账 + 直方图 + p99）
// ---------------------------------------------------------------------------

/// 回环单步结局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoopStep {
    /// 回环闭合，往返时延（µs，向下取整）。
    Completed { latency_us: u64 },
    /// 回环被当前服务态/TPR 闸挂起（不打断语义生效），等待调度。
    HeldPending,
}

/// 回环服务结局（enter_service 投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ServiceOutcome {
    /// 已进入服务态（ISR 置位）。
    Serviced,
    /// 被优先级/TPR 闸挂起（IRR 等待）。
    HeldPending,
    /// 无 EOI 模式语义完成（SMI/NMI/INIT/Startup）。
    CompletedNoEoi,
}

/// 单条回环记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LoopRecord {
    /// 关联生命周期序号。
    pub seq: u64,
    /// 回环向量。
    pub vector: u8,
    /// 发送时戳。
    pub sent_tick: u64,
    /// 闭合时戳（0=未闭合）。
    pub done_tick: u64,
    /// 往返时延（µs；未闭合为 0）。
    pub latency_us: u64,
    /// 是否经历挂起（回环不打断语义触发）。
    pub held: bool,
    /// 是否闭合。
    pub completed: bool,
}

/// 回环时延直方图（µs 桶：≤1/≤2/≤5/≤10/≤20/≤50/≤100/超界）。
///
/// 桶界来源：J1「µs 级」量化 + LOOPBACK_LATENCY_MAX_US=50 判级线；
/// 100 µs 桶留作越界前最后一档观察位。
#[derive(Clone, Copy, Debug)]
pub struct LatencyHistogram {
    buckets: [u64; 8],
}

impl LatencyHistogram {
    /// 桶界（µs）：样本 s 落入首个满足 s <= edge 的桶；全不满足入越界桶。
    pub const EDGES_US: [u64; 7] = [1, 2, 5, 10, 20, 50, 100];

    /// 空直方图。
    pub const fn new() -> LatencyHistogram {
        LatencyHistogram { buckets: [0; 8] }
    }

    /// 落账一样本。
    pub fn record(&mut self, latency_us: u64) {
        let mut idx = 7usize;
        let mut i = 0usize;
        while i < 7 {
            if latency_us <= LatencyHistogram::EDGES_US[i] {
                idx = i;
                break;
            }
            i += 1;
        }
        self.buckets[idx] = self.buckets[idx].saturating_add(1);
    }

    /// 各桶计数。
    pub fn buckets(&self) -> [u64; 8] {
        self.buckets
    }

    /// 总样本。
    pub fn total(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < 8 {
            n = n.saturating_add(self.buckets[i]);
            i += 1;
        }
        n
    }
}

/// 自 IPI 回环账。
#[derive(Clone, Debug)]
pub struct SelfIpiLoop {
    records: Vec<LoopRecord>,
    opened: u64,
    completed: u64,
    held_total: u64,
    sum_latency_us: u64,
    max_latency_us: u64,
    hist: LatencyHistogram,
}

impl SelfIpiLoop {
    /// 空回环账。
    pub const fn new() -> SelfIpiLoop {
        SelfIpiLoop {
            records: Vec::new(),
            opened: 0,
            completed: 0,
            held_total: 0,
            sum_latency_us: 0,
            max_latency_us: 0,
            hist: LatencyHistogram::new(),
        }
    }

    /// 开一条回环记录，返回记录下标。
    pub fn open(&mut self, seq: u64, vector: u8, sent_tick: u64) -> Result<usize, IpiEoiError> {
        if self.records.len() >= LOOPBACK_CAP {
            return Err(IpiEoiError::CapacityExhausted);
        }
        let idx = self.records.len();
        self.records.push(LoopRecord {
            seq,
            vector,
            sent_tick,
            done_tick: 0,
            latency_us: 0,
            held: false,
            completed: false,
        });
        self.opened = self.opened.saturating_add(1);
        Ok(idx)
    }

    /// 标记该回环经历挂起。
    pub fn mark_held(&mut self, idx: usize) -> Result<(), IpiEoiError> {
        match self.records.get_mut(idx) {
            Some(r) => {
                r.held = true;
                self.held_total = self.held_total.saturating_add(1);
                Ok(())
            }
            None => Err(IpiEoiError::LedgerUnbalanced {
                sent: idx as u64,
                retired: 0,
            }),
        }
    }

    /// 闭合回环：算 µs 时延、落直方图、校验 µs 级窗口。
    pub fn complete(&mut self, idx: usize, done_tick: u64) -> Result<u64, IpiEoiError> {
        let latency_us = match self.records.get(idx) {
            Some(r) => {
                if r.completed {
                    return Err(IpiEoiError::LedgerUnbalanced {
                        sent: r.seq,
                        retired: 0,
                    });
                }
                done_tick.saturating_sub(r.sent_tick) / CLOCK_TICKS_PER_US
            }
            None => {
                return Err(IpiEoiError::LedgerUnbalanced {
                    sent: idx as u64,
                    retired: 0,
                })
            }
        };
        if latency_us > LOOPBACK_LATENCY_MAX_US {
            return Err(IpiEoiError::LoopbackLatencyOut {
                latency_us,
                bound_us: LOOPBACK_LATENCY_MAX_US,
            });
        }
        match self.records.get_mut(idx) {
            Some(r) => {
                r.done_tick = done_tick;
                r.latency_us = latency_us;
                r.completed = true;
            }
            None => {
                return Err(IpiEoiError::LedgerUnbalanced {
                    sent: idx as u64,
                    retired: 0,
                })
            }
        }
        self.completed = self.completed.saturating_add(1);
        self.sum_latency_us = self.sum_latency_us.saturating_add(latency_us);
        if latency_us > self.max_latency_us {
            self.max_latency_us = latency_us;
        }
        self.hist.record(latency_us);
        Ok(latency_us)
    }

    /// 已闭合数。
    pub fn completed(&self) -> u64 {
        self.completed
    }

    /// 开账数。
    pub fn opened(&self) -> u64 {
        self.opened
    }

    /// 经历挂起的回环数（回环不打断语义的触发面统计）。
    pub fn held_total(&self) -> u64 {
        self.held_total
    }

    /// 时延总和（µs；指纹面用）。
    pub fn sum_latency_us_total(&self) -> u64 {
        self.sum_latency_us
    }

    /// 时延均值（µs；零样本返回 0）。
    pub fn avg_latency_us(&self) -> u64 {
        if self.completed == 0 {
            0
        } else {
            self.sum_latency_us / self.completed
        }
    }

    /// 时延峰值（µs）。
    pub fn max_latency_us(&self) -> u64 {
        self.max_latency_us
    }

    /// 直方图视图。
    pub fn histogram(&self) -> LatencyHistogram {
        self.hist
    }

    /// 已闭合样本时延清单（p99 等分位计算面）。
    pub fn latency_samples(&self) -> Vec<u64> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.records.len() {
            if self.records[i].completed {
                out.push(self.records[i].latency_us);
            }
            i += 1;
        }
        out
    }

    /// p99 时延（µs；不足 100 样本取最大——小样本保守）。
    pub fn p99_latency_us(&self) -> u64 {
        let mut s = self.latency_samples();
        if s.is_empty() {
            return 0;
        }
        s.sort_unstable();
        let idx = ((s.len() as u64).saturating_mul(99) / 100) as usize;
        let idx = if idx >= s.len() { s.len() - 1 } else { idx };
        s[idx]
    }

    /// 记录快照。
    pub fn records(&self) -> &[LoopRecord] {
        &self.records
    }
}

// ---------------------------------------------------------------------------
// 注入面板（写序违例 / EOI 残留 / ICR 忙重写 三类，检出必账、漏检必红）
// ---------------------------------------------------------------------------

/// 注入类别码：1=写序违例 2=EOI 残留 3=ICR 忙重写。
pub const INJECTION_KIND_ORDER: u8 = 1;
/// 注入类别码：EOI 残留。
pub const INJECTION_KIND_RESIDUE: u8 = 2;
/// 注入类别码：ICR 忙重写。
pub const INJECTION_KIND_BUSY: u8 = 3;

/// 单条注入记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectionRecord {
    /// 类别码。
    pub kind_code: u8,
    /// 注入时戳。
    pub tick: u64,
    /// 是否被检测器捕获。
    pub detected: bool,
}

/// 注入面板：每次注入必落账；漏检 = InjectionMissed 红案（敏感度自证面）。
#[derive(Clone, Debug)]
pub struct InjectionPanel {
    records: Vec<InjectionRecord>,
    detected: u64,
    missed: u64,
}

impl InjectionPanel {
    /// 空面板。
    pub const fn new() -> InjectionPanel {
        InjectionPanel {
            records: Vec::new(),
            detected: 0,
            missed: 0,
        }
    }

    /// 落一笔注入。未检出 → Err(InjectionMissed)（调用方升级为红案）。
    pub fn record(
        &mut self,
        kind_code: u8,
        tick: u64,
        detected: bool,
    ) -> Result<(), IpiEoiError> {
        if self.records.len() >= INJECTION_CAP {
            return Err(IpiEoiError::CapacityExhausted);
        }
        self.records.push(InjectionRecord {
            kind_code,
            tick,
            detected,
        });
        if detected {
            self.detected = self.detected.saturating_add(1);
            Ok(())
        } else {
            self.missed = self.missed.saturating_add(1);
            Err(IpiEoiError::InjectionMissed { kind_code })
        }
    }

    /// 检出总数。
    pub fn detected(&self) -> u64 {
        self.detected
    }

    /// 漏检总数（非零即健康红）。
    pub fn missed(&self) -> u64 {
        self.missed
    }

    /// 记录清单。
    pub fn records(&self) -> &[InjectionRecord] {
        &self.records
    }

    /// 按类别检出数（10/10 断言面）。
    pub fn detected_by_kind(&self, kind_code: u8) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.records.len() {
            if self.records[i].kind_code == kind_code && self.records[i].detected {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 服务簿（每向量：在服务模式/无 EOI 完成/服务次数——EOI 语义裁决依据）
// ---------------------------------------------------------------------------

/// 每向量服务簿条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct ServiceBookEntry {
    /// 最近一次服务的模式码（0=从未服务）。
    mode_code: u8,
    /// 当前是否在服务态（ISR 位语义镜像）。
    in_service: bool,
    /// 是否以无 EOI 模式完成过最近一次服务。
    no_eoi_done: bool,
    /// 服务次数（saturating）。
    service_count: u8,
    /// 最近一次服务关联的生命周期序号。
    seq: u64,
}

impl ServiceBookEntry {
    const EMPTY: ServiceBookEntry = ServiceBookEntry {
        mode_code: 0,
        in_service: false,
        no_eoi_done: false,
        service_count: 0,
        seq: 0,
    };
}

/// 挂起队列条目（IRR 等待面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HeldEntry {
    /// 生命周期序号。
    pub seq: u64,
    /// 挂起向量。
    pub vector: u8,
    /// 原始发送时戳（时延含挂起时间）。
    pub sent_tick: u64,
    /// 关联回环记录下标（非回环为 None）。
    pub loop_idx: Option<usize>,
}

/// 在途发送（ICR 提交后、投递落定前的事务载荷）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutstandingSend {
    /// 生命周期序号。
    pub seq: u64,
    /// 投递向量。
    pub vector: u8,
    /// 投递模式。
    pub mode: DeliveryMode,
    /// 简写域。
    pub shorthand: Shorthand,
    /// 目的域原值（简写非 DestField 时忽略——SDM 语义）。
    pub dest: u32,
    /// 发送时戳。
    pub sent_tick: u64,
    /// 是否已投递落定。
    pub delivered: bool,
}

// ---------------------------------------------------------------------------
// 主机：IPI EOI 语义机（四阶段状态机 + 残留检测 + 回环 + 注入面）
// ---------------------------------------------------------------------------

/// 向量域校验（按模式分域：Fixed 类走 0x20..=0xFE；NMI 允许异常域向量
/// 0x00..=0xFE（典型 NMI 向量 2）；SMI/INIT 忽略向量；Startup 向量为启动
/// 页号 0x00..=0xFF；ExtINT 走 0x20..=0xFE）。来源：SDM §6.15 / §10.6.2。
pub fn vector_domain_ok(mode: DeliveryMode, vector: u8) -> bool {
    match mode {
        DeliveryMode::Fixed | DeliveryMode::LowestPriority | DeliveryMode::ExtInt => {
            vector >= VECTOR_LEGAL_LO && vector <= VECTOR_LEGAL_HI
        }
        DeliveryMode::Nmi => vector <= 0xFE,
        DeliveryMode::Smi | DeliveryMode::Init => true,
        DeliveryMode::Startup => true,
    }
}

/// 回环向量合法域（J1 明确校验面：0x20..=0xFE 且 ≠0xFF spurious）。
pub const fn loopback_vector_ok(vector: u8) -> bool {
    vector >= VECTOR_LEGAL_LO && vector <= VECTOR_LEGAL_HI
}

/// FNV-1a 64 位单拍（u64 按 8 字节小端序逐字节混合）。
pub fn fnv_u64(state: u64, v: u64) -> u64 {
    let mut h = state;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV-1a 64 位单拍（u32 按 4 字节混合）。
pub fn fnv_u32(state: u64, v: u32) -> u64 {
    let mut h = state;
    let mut i = 0u32;
    while i < 4 {
        h ^= ((v >> (i * 8)) & 0xFF) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// IPI 健康态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealthStatus {
    /// 全绿：无红案、账目守恒、无漏检。
    Green,
    /// 黄：结构健康但有挂起/在途未收口。
    Amber,
    /// 红：红案在案（残留/漏检/失衡/越界）。
    Red,
}

/// 健康报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IpiHealth {
    /// 总体健康态。
    pub status: HealthStatus,
    /// 红案计数（单调，不可洗白）。
    pub red_flags: u64,
    /// 累计残留数。
    pub residues: u64,
    /// 守恒式是否成立。
    pub balance_ok: bool,
    /// 当前挂起数。
    pub held_pending: usize,
    /// 当前在服务数。
    pub in_service: u64,
    /// 注入检出数。
    pub injections_detected: u64,
    /// 注入漏检数。
    pub injections_missed: u64,
}

/// J1 campaign 报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CampaignReport {
    /// 执行次数（EOI 次数）。
    pub iterations: u64,
    /// 本次 campaign 段新增残留（J1 必须为 0）。
    pub residues: u64,
    /// 本次段 EOI 总数。
    pub eoi_total: u64,
    /// 本次段发送总数。
    pub sent_total: u64,
    /// 本次段消耗 tick。
    pub elapsed_ticks: u64,
    /// 守恒式是否成立。
    pub balance_ok: bool,
    /// 段末指纹。
    pub fingerprint: u64,
}

/// 模式分账 campaign 报告（模式×EOI 需求真值表的行为面投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModeCampaignReport {
    /// 执行次数。
    pub iterations: u64,
    /// 按模式发送分账。
    pub sent_by_mode: [u64; 8],
    /// 按模式 EOI 分账。
    pub eoi_by_mode: [u64; 8],
    /// 按模式无 EOI 完成分账。
    pub no_eoi_by_mode: [u64; 8],
    /// 本次段新增残留。
    pub residues: u64,
    /// 守恒式是否成立。
    pub balance_ok: bool,
}

/// IPI EOI 语义机：本单全部子系统的宿主。
///
/// 状态机铁律：发送（ICR 写序）→投递（Delivery Status 回 Idle）→服务
/// （ISR 置位 / 挂起裁决）→EOI（ISR 清零 + 残留断言）。任何越序操作返回
/// Err 且不推进账目；红案 latch 后不可洗白。
#[derive(Clone, Debug)]
pub struct IpiEoiMachine {
    tick_now: u64,
    icr: IcrWriteTracker,
    isr: VectorBitmap,
    irr: VectorBitmap,
    book: [ServiceBookEntry; 256],
    tpr_class: u8,
    detector: ResidueDetector,
    ledger: IpiLedger,
    loops: SelfIpiLoop,
    injections: InjectionPanel,
    outstanding: Option<OutstandingSend>,
    pending_loop: Option<usize>,
    held: Vec<HeldEntry>,
    red_flags: u64,
    dest_default: u32,
}

impl IpiEoiMachine {
    /// 新建语义机（Idle、全账归零）。
    pub const fn new() -> IpiEoiMachine {
        IpiEoiMachine {
            tick_now: 0,
            icr: IcrWriteTracker::new(),
            isr: VectorBitmap::new(),
            irr: VectorBitmap::new(),
            book: [ServiceBookEntry::EMPTY; 256],
            tpr_class: 0,
            detector: ResidueDetector::new(),
            ledger: IpiLedger::new(),
            loops: SelfIpiLoop::new(),
            injections: InjectionPanel::new(),
            outstanding: None,
            pending_loop: None,
            held: Vec::new(),
            red_flags: 0,
            dest_default: 0x0200_0000,
        }
    }

    // ---------------- 时钟与配置 ----------------

    /// 抽象时钟推进（强制单调；回退即 ClockRegression 红案）。
    pub fn advance_clock(&mut self, tick: u64) -> Result<(), IpiEoiError> {
        if tick < self.tick_now {
            self.red_flags = self.red_flags.saturating_add(1);
            return Err(IpiEoiError::ClockRegression {
                prev: self.tick_now,
                next: tick,
            });
        }
        self.tick_now = tick;
        Ok(())
    }

    /// 当前时钟。
    pub fn tick(&self) -> u64 {
        self.tick_now
    }

    /// 设置默认目的域（物理 APIC ID 位于 bit 31:24——xAPIC DestField 语义）。
    pub fn set_dest_default(&mut self, dest_word: u32) {
        self.dest_default = dest_word;
    }

    /// 设置 TPR 优先级类（0..=15，TPR bit 7:4——SDM §10.9.1 任务优先级）。
    pub fn set_tpr_class(&mut self, class: u8) {
        self.tpr_class = class & 0x0F;
    }

    /// 当前 TPR 优先级类。
    pub fn tpr_class(&self) -> u8 {
        self.tpr_class
    }

    /// 错误恢复：Delivery Status 强制回 Idle（只复位状态不洗账）。
    pub fn icr_recover_idle(&mut self) {
        self.icr.recover_idle();
    }

    // ---------------- 只读观察面 ----------------

    /// ISR 位图快照。
    pub fn isr_words(&self) -> [u32; 8] {
        self.isr.words()
    }

    /// IRR 位图快照。
    pub fn irr_words(&self) -> [u32; 8] {
        self.irr.words()
    }

    /// 在服务数。
    pub fn in_service_count(&self) -> u64 {
        self.isr.count()
    }

    /// 挂起数。
    pub fn held_pending(&self) -> usize {
        self.held.len()
    }

    /// 挂起清单。
    pub fn held_entries(&self) -> Vec<HeldEntry> {
        self.held.clone()
    }

    /// 在途事务是否存在。
    pub fn has_outstanding(&self) -> bool {
        self.outstanding.is_some()
    }

    /// 红案计数。
    pub fn red_flags(&self) -> u64 {
        self.red_flags
    }

    /// 残留检测器视图。
    pub fn detector(&self) -> &ResidueDetector {
        &self.detector
    }

    /// 生命周期账视图。
    pub fn ledger(&self) -> &IpiLedger {
        &self.ledger
    }

    /// 回环账视图。
    pub fn loops(&self) -> &SelfIpiLoop {
        &self.loops
    }

    /// 注入面板视图。
    pub fn injections(&self) -> &InjectionPanel {
        &self.injections
    }

    /// ICR 统计视图。
    pub fn icr_stats(&self) -> IcrStats {
        self.icr.stats()
    }

    /// 当前服务态阻断类（TPR 类与最高 ISR 类取大——投递裁决的 PPR 近似）。
    fn block_class(&self) -> u8 {
        let mut c = self.tpr_class;
        if let Some(hv) = self.isr.highest() {
            let k = hv >> 4;
            if k > c {
                c = k;
            }
        }
        c
    }

    // ---------------- 四阶段状态机 ----------------

    /// 阶段一：发送。走 ICR 双字写序纪律（高字→低字），违例不进投递账。
    pub fn send_ipi(
        &mut self,
        tick: u64,
        vector: u8,
        mode: DeliveryMode,
        shorthand: Shorthand,
        dest: u32,
    ) -> Result<u64, IpiEoiError> {
        self.send_common(tick, vector, mode, shorthand, dest, false)
    }

    /// 发送公共体（self_loop 标记回环生命周期记录）。
    fn send_common(
        &mut self,
        tick: u64,
        vector: u8,
        mode: DeliveryMode,
        shorthand: Shorthand,
        dest: u32,
        self_loop: bool,
    ) -> Result<u64, IpiEoiError> {
        self.advance_clock(tick)?;
        if !vector_domain_ok(mode, vector) {
            self.ledger.invalid_rejected = self.ledger.invalid_rejected.saturating_add(1);
            return Err(IpiEoiError::InvalidVector { vector });
        }
        // 简写域×模式组合纪律：INIT/SIPI 打自身无架构意义（INIT-SIPI 序只打 AP）。
        if (mode == DeliveryMode::Init || mode == DeliveryMode::Startup)
            && shorthand == Shorthand::SelfOnly
        {
            return Err(IpiEoiError::ShorthandIllegal {
                shorthand_code: shorthand.code(),
                mode_code: mode.code(),
            });
        }
        // 简写非 DestField 时目的域被硬件忽略（SDM Table 10-4）。
        let dest_word = if shorthand == Shorthand::DestField {
            dest
        } else {
            0
        };
        // ICR 双字写序：高字（0x310 目标域）先写、低字（0x300 触发域）后写。
        self.icr.write_high(dest_word)?;
        let low = compose_icr_low(vector, mode, true, false, shorthand);
        let _fields = match self.icr.write_low(low) {
            Ok(f) => f,
            Err(e) => {
                // 真实发送路径的违例 = 纪律破，立案（注入面绕过本路径不在此计）。
                self.red_flags = self.red_flags.saturating_add(1);
                return Err(e);
            }
        };
        let seq = self.ledger.push_record(vector, mode.code(), shorthand.code(), tick, self_loop)?;
        self.outstanding = Some(OutstandingSend {
            seq,
            vector,
            mode,
            shorthand,
            dest: dest_word,
            sent_tick: tick,
            delivered: false,
        });
        Ok(seq)
    }

    /// 阶段二：投递落定。Delivery Status Sending→Idle，在途事务标记已投递
    /// （事务载荷由服务步消费——投递步只落标记不销账）。返回投递向量。
    pub fn settle_delivery(&mut self, tick: u64) -> Result<u8, IpiEoiError> {
        self.advance_clock(tick)?;
        if self.outstanding.is_none() {
            return Err(IpiEoiError::NoOutstandingSend);
        }
        self.icr.settle();
        let (seq, vector, already) = match self.outstanding.as_mut() {
            Some(o) => {
                let already = o.delivered;
                o.delivered = true;
                (o.seq, o.vector, already)
            }
            None => return Err(IpiEoiError::NoOutstandingSend),
        };
        if already {
            // 双重投递落定——生命周期越序，拒绝（账目未二次推进）。
            return Err(IpiEoiError::NoOutstandingSend);
        }
        self.ledger.rec_set_delivered(seq, tick)?;
        Ok(vector)
    }

    /// 阶段三：服务。EOI 必需模式置 ISR 位（受优先级/TPR 闸裁决，可挂起）；
    /// 非 EOI 模式（SMI/NMI/INIT/Startup）不置 ISR/IRR，直接语义完成分账
    /// （SDM §10.9：这类投递不更新 ISR/IRR、不产生 EOI 要求）。
    pub fn enter_service(&mut self, tick: u64) -> Result<ServiceOutcome, IpiEoiError> {
        self.advance_clock(tick)?;
        let out = match self.outstanding.take() {
            Some(o) => o,
            None => return Err(IpiEoiError::NoOutstandingSend),
        };
        if !out.delivered {
            self.red_flags = self.red_flags.saturating_add(1);
            return Err(IpiEoiError::LedgerUnbalanced {
                sent: out.seq,
                retired: 0,
            });
        }
        let vector = out.vector;
        let mode = out.mode;
        if !mode.eoi_required() {
            // 无 EOI 模式：语义完成，不置 ISR/IRR。
            self.ledger.rec_set_completed(out.seq, tick, true)?;
            let slot = &mut self.book[vector as usize];
            slot.mode_code = mode.code();
            slot.in_service = false;
            slot.no_eoi_done = true;
            slot.service_count = slot.service_count.saturating_add(1);
            slot.seq = out.seq;
            return Ok(ServiceOutcome::CompletedNoEoi);
        }
        // 优先级裁决：向量类 <= 阻断类（最高 ISR 类 / TPR 类）→ 挂起 IRR。
        if (vector >> 4) <= self.block_class() {
            if self.held.len() >= HELD_CAP {
                return Err(IpiEoiError::CapacityExhausted);
            }
            self.irr.set(vector);
            let loop_idx = self.pending_loop.take();
            if let Some(li) = loop_idx {
                self.loops.mark_held(li)?;
            }
            self.held.push(HeldEntry {
                seq: out.seq,
                vector,
                sent_tick: out.sent_tick,
                loop_idx,
            });
            self.ledger.rec_set_held(out.seq)?;
            return Ok(ServiceOutcome::HeldPending);
        }
        // 进入服务态：ISR 置位 + 服务簿登记。
        self.isr.set(vector);
        let slot = &mut self.book[vector as usize];
        slot.mode_code = mode.code();
        slot.in_service = true;
        slot.no_eoi_done = false;
        slot.service_count = slot.service_count.saturating_add(1);
        slot.seq = out.seq;
        self.ledger.rec_set_serviced(out.seq, tick)?;
        Ok(ServiceOutcome::Serviced)
    }

    /// 阶段四：EOI。清 ISR 位 + J1 残留断言 + 模式门禁 + 守恒分账。
    ///
    /// 语义门禁序：spurious → 模式 EOI 错位 → 双重 EOI/不在服务 → 清位 →
    /// 残留断言 → 挂起自动调度（真实 LAPIC 在 EOI 后重裁 IRR）。
    pub fn issue_eoi(&mut self, tick: u64, vector: u8) -> Result<(), IpiEoiError> {
        self.advance_clock(tick)?;
        if vector == SPURIOUS_VECTOR {
            self.ledger.spurious_rejected = self.ledger.spurious_rejected.saturating_add(1);
            return Err(IpiEoiError::InvalidVector { vector });
        }
        let entry = self.book[vector as usize];
        if !entry.in_service {
            if entry.no_eoi_done {
                return Err(IpiEoiError::ModeEoiNotApplicable {
                    mode_code: entry.mode_code,
                });
            }
            if entry.service_count > 0 {
                return Err(IpiEoiError::DoubleEoi { vector });
            }
            return Err(IpiEoiError::VectorNotInService { vector });
        }
        self.isr.clear(vector);
        self.book[vector as usize].in_service = false;
        let mode = match DeliveryMode::from_code(entry.mode_code) {
            Some(m) => m,
            None => {
                self.red_flags = self.red_flags.saturating_add(1);
                return Err(IpiEoiError::LedgerUnbalanced {
                    sent: entry.seq,
                    retired: 0,
                });
            }
        };
        if !mode.eoi_required() {
            // 服务簿与真值表互证失败——簿面损坏，红案。
            self.red_flags = self.red_flags.saturating_add(1);
            return Err(IpiEoiError::ModeEoiNotApplicable {
                mode_code: entry.mode_code,
            });
        }
        self.ledger.rec_set_completed(entry.seq, tick, false)?;
        // J1 逐次断言：EOI 后该向量 ISR 位必须已清零。
        let words = self.isr.words();
        self.detector.check_after_eoi(&words, vector, entry.seq, tick)?;
        // EOI 后重裁挂起队列（真实 LAPIC 语义）。
        self.dispatch_held(tick)?;
        Ok(())
    }

    /// 挂起调度：EOI 清位后重裁 IRR——取队首类高于当前阻断类的挂起项移入
    /// 服务态（FIFO + 优先级，LAPIC 重裁语义的抽象）。
    pub fn dispatch_held(&mut self, tick: u64) -> Result<Option<u8>, IpiEoiError> {
        self.advance_clock(tick)?;
        let mut pick: Option<usize> = None;
        let mut i = 0usize;
        while i < self.held.len() {
            // 挂起项可调度 = 其优先级类高于当前阻断类（TPR 类与最高 ISR 类取大）——
            // TPR 未降档时低类挂起项继续等待（真实 LAPIC 重裁语义）。
            if (self.held[i].vector >> 4) > self.block_class() {
                pick = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match pick {
            Some(i) => i,
            None => return Ok(None),
        };
        let e = self.held.remove(idx);
        self.irr.clear(e.vector);
        self.isr.set(e.vector);
        let slot = &mut self.book[e.vector as usize];
        slot.in_service = true;
        slot.no_eoi_done = false;
        self.ledger.rec_rehold_to_serviced(e.seq, tick)?;
        if let Some(li) = e.loop_idx {
            match self.loops.complete(li, tick) {
                Ok(_latency) => {
                    self.pending_loop = None;
                }
                Err(err) => {
                    self.red_flags = self.red_flags.saturating_add(1);
                    self.pending_loop = None;
                    return Err(err);
                }
            }
        }
        Ok(Some(e.vector))
    }

    // ---------------- 自 IPI 回环 ----------------

    /// 自 IPI 回环：发送一发自 IPI（Fixed×SelfOnly，向量合法域校验）。
    /// 返回生命周期序号。
    pub fn self_ipi_loopback(&mut self, tick: u64, vector: u8) -> Result<u64, IpiEoiError> {
        if !loopback_vector_ok(vector) {
            self.ledger.invalid_rejected = self.ledger.invalid_rejected.saturating_add(1);
            return Err(IpiEoiError::InvalidVector { vector });
        }
        // 回环事务前账：开启回环记录（发送时戳 = 本 tick）。
        self.advance_clock(tick)?;
        let seq = self.send_common(tick, vector, DeliveryMode::Fixed, Shorthand::SelfOnly, 0, true)?;
        let li = self.loops.open(seq, vector, tick)?;
        self.pending_loop = Some(li);
        Ok(seq)
    }

    /// 回环闭合步：投递→服务→（未挂起则）EOI→时延落账。
    /// 挂起（回环不打断当前服务态语义）时返回 HeldPending，待 EOI 触发
    /// 重裁后由 dispatch_held 闭合。
    pub fn self_ipi_loopback_settle(&mut self, tick: u64) -> Result<LoopStep, IpiEoiError> {
        let vector = self.settle_delivery(tick)?;
        let outcome = self.enter_service(tick)?;
        match outcome {
            ServiceOutcome::Serviced => {
                self.issue_eoi(tick, vector)?;
                let li = match self.pending_loop.take() {
                    Some(li) => li,
                    None => {
                        return Err(IpiEoiError::LedgerUnbalanced {
                            sent: 0,
                            retired: 0,
                        })
                    }
                };
                match self.loops.complete(li, tick) {
                    Ok(latency) => Ok(LoopStep::Completed { latency_us: latency }),
                    Err(e) => {
                        self.red_flags = self.red_flags.saturating_add(1);
                        Err(e)
                    }
                }
            }
            ServiceOutcome::HeldPending => Ok(LoopStep::HeldPending),
            ServiceOutcome::CompletedNoEoi => Err(IpiEoiError::ModeEoiNotApplicable {
                mode_code: DeliveryMode::Fixed.code(),
            }),
        }
    }

    // ---------------- 注入面（三类违规各 10/10 必检出） ----------------

    /// 注入：写序违例（低字先于高字）。返回是否被拦截（应恒 true）。
    pub fn inject_write_order_violation(&mut self, tick: u64) -> Result<bool, IpiEoiError> {
        self.advance_clock(tick)?;
        let low = compose_icr_low(0x40, DeliveryMode::Fixed, true, false, Shorthand::SelfOnly);
        let detected = self.icr.write_low(low).is_err();
        self.record_injection(INJECTION_KIND_ORDER, tick, detected)?;
        Ok(detected)
    }

    /// 注入：ICR 忙重写（Delivery Status=Sending 期写 ICR）。
    pub fn inject_icr_busy_rewrite(&mut self, tick: u64) -> Result<bool, IpiEoiError> {
        self.advance_clock(tick)?;
        self.icr.force_busy();
        let detected = self.icr.write_high(0x0000_0001).is_err();
        self.icr.recover_idle();
        self.record_injection(INJECTION_KIND_BUSY, tick, detected)?;
        Ok(detected)
    }

    /// 注入：EOI 残留（模拟 EOI 丢失——ISR 位置位后无人清零），残留扫描
    /// 必须 latch；注入后清位恢复现场（红案照 latch，不洗白）。
    pub fn inject_eoi_residue(&mut self, tick: u64, vector: u8) -> Result<bool, IpiEoiError> {
        self.advance_clock(tick)?;
        if !loopback_vector_ok(vector) {
            return Err(IpiEoiError::InvalidVector { vector });
        }
        if self.isr.test(vector) || self.irr.test(vector) {
            return Err(IpiEoiError::DoubleEoi { vector });
        }
        self.isr.set(vector);
        let words = self.isr.words();
        let found = self.detector.scan_all(&words, tick);
        self.isr.clear(vector);
        let detected = found > 0;
        self.record_injection(INJECTION_KIND_RESIDUE, tick, detected)?;
        Ok(detected)
    }

    /// 注入落账公共体：漏检升级红案。
    fn record_injection(
        &mut self,
        kind_code: u8,
        tick: u64,
        detected: bool,
    ) -> Result<(), IpiEoiError> {
        match self.injections.record(kind_code, tick, detected) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.red_flags = self.red_flags.saturating_add(1);
                Err(e)
            }
        }
    }

    // ---------------- J1 campaign：10 万次 EOI 零残留 ----------------

    /// J1 主判据 campaign：iterations 次「发送→投递→服务→EOI」全周期，
    /// LCG 确定性选向量（0x20..=0xFE 均匀取样），逐次 EOI 后 ISR 位清零
    /// 断言。返回报告；任何残留/违例即时 Err 上抛。
    pub fn run_campaign(&mut self, seed: u32, iterations: u64) -> Result<CampaignReport, IpiEoiError> {
        let residues_before = self.detector.residue_count();
        let sent_before = self.ledger.sent_total();
        let eoi_before = self.ledger.eoi_total();
        let tick_before = self.tick_now;
        let mut state: u32 = seed;
        let mut i: u64 = 0;
        while i < iterations {
            state = state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
            let vector = VECTOR_LEGAL_LO + (state % VECTOR_SPAN) as u8;
            let send_tick = self.tick_now.saturating_add(1 + ((state >> 9) % 4) as u64);
            self.send_ipi(
                send_tick,
                vector,
                DeliveryMode::Fixed,
                Shorthand::DestField,
                self.dest_default,
            )?;
            self.settle_delivery(send_tick.saturating_add(1))?;
            self.enter_service(send_tick.saturating_add(2))?;
            self.issue_eoi(send_tick.saturating_add(3), vector)?;
            i += 1;
        }
        let residues = self.detector.residue_count().saturating_sub(residues_before);
        let report = CampaignReport {
            iterations,
            residues,
            eoi_total: self.ledger.eoi_total().saturating_sub(eoi_before),
            sent_total: self.ledger.sent_total().saturating_sub(sent_before),
            elapsed_ticks: self.tick_now.saturating_sub(tick_before),
            balance_ok: self.balance_ok(),
            fingerprint: self.fnv_fingerprint(),
        };
        let _ = eoi_before;
        if residues != 0 {
            return Err(IpiEoiError::EoiResidue { vector: 0 });
        }
        Ok(report)
    }

    /// 模式分账 campaign：7 类投递模式轮转（LCG 选模式），EOI 必需模式走
    /// 全周期，无 EOI 模式走语义完成——真值表的账面投影。
    pub fn run_mode_campaign(
        &mut self,
        seed: u32,
        iterations: u64,
    ) -> Result<ModeCampaignReport, IpiEoiError> {
        let residues_before = self.detector.residue_count();
        let sent_before = self.ledger.sent_by_mode();
        let eoi_before = self.ledger.eoi_by_mode();
        let no_eoi_before = self.ledger.no_eoi_by_mode();
        let mut state: u32 = seed;
        let mut i: u64 = 0;
        while i < iterations {
            state = state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
            // 7 类模式轮转（跳过保留码 0b011）。
            const MODE_CODES: [u8; 7] = [0b000, 0b001, 0b010, 0b100, 0b101, 0b110, 0b111];
            let pick = ((state >> 8) % 7) as usize;
            let mode = match DeliveryMode::from_code(MODE_CODES[pick]) {
                Some(m) => m,
                None => {
                    self.ledger.invalid_rejected = self.ledger.invalid_rejected.saturating_add(1);
                    i += 1;
                    continue;
                }
            };
            let vector = match mode {
                DeliveryMode::Fixed | DeliveryMode::LowestPriority | DeliveryMode::ExtInt => {
                    VECTOR_LEGAL_LO + (state % VECTOR_SPAN) as u8
                }
                DeliveryMode::Nmi => 0x02,
                DeliveryMode::Smi | DeliveryMode::Init => 0x00,
                DeliveryMode::Startup => 0xA0 + ((state >> 16) % 0x20) as u8,
            };
            let send_tick = self.tick_now.saturating_add(1 + ((state >> 9) % 3) as u64);
            self.send_ipi(send_tick, vector, mode, Shorthand::DestField, self.dest_default)?;
            self.settle_delivery(send_tick.saturating_add(1))?;
            self.enter_service(send_tick.saturating_add(2))?;
            if mode.eoi_required() {
                self.issue_eoi(send_tick.saturating_add(3), vector)?;
            }
            i += 1;
        }
        let mut sent_by_mode = [0u64; 8];
        let mut eoi_by_mode = [0u64; 8];
        let mut no_eoi_by_mode = [0u64; 8];
        let mut k = 0usize;
        while k < 8 {
            sent_by_mode[k] = self.ledger.sent_by_mode()[k].saturating_sub(sent_before[k]);
            eoi_by_mode[k] = self.ledger.eoi_by_mode()[k].saturating_sub(eoi_before[k]);
            no_eoi_by_mode[k] = self.ledger.no_eoi_by_mode()[k].saturating_sub(no_eoi_before[k]);
            k += 1;
        }
        Ok(ModeCampaignReport {
            iterations,
            sent_by_mode,
            eoi_by_mode,
            no_eoi_by_mode,
            residues: self.detector.residue_count().saturating_sub(residues_before),
            balance_ok: self.balance_ok(),
        })
    }

    // ---------------- 守恒 + 健康 + 指纹 + 双跑 ----------------

    /// 守恒式：在服务 + 挂起 + 已退休 + 在途 = 已发送。
    pub fn balance_ok(&self) -> bool {
        let open = if self.outstanding.is_some() { 1u64 } else { 0 };
        self.ledger
            .balance_ok(self.isr.count(), self.held.len() as u64, open)
    }

    /// 健康报告。
    pub fn health(&self) -> IpiHealth {
        let balance = self.balance_ok();
        let residues = self.detector.residue_count();
        let missed = self.injections.missed();
        let red = self.red_flags > 0
            || self.detector.is_red()
            || missed > 0
            || !balance;
        let status = if red {
            HealthStatus::Red
        } else if self.held.len() > 0 || self.outstanding.is_some() {
            HealthStatus::Amber
        } else {
            HealthStatus::Green
        };
        IpiHealth {
            status,
            red_flags: self.red_flags,
            residues,
            balance_ok: balance,
            held_pending: self.held.len(),
            in_service: self.isr.count(),
            injections_detected: self.injections.detected(),
            injections_missed: missed,
        }
    }

    /// FNV-1a 64 位全账指纹（双跑对拍面：schema + 全计数 + 全记录）。
    pub fn fnv_fingerprint(&self) -> u64 {
        let mut h = fnv_u64(FNV_OFFSET, FINGERPRINT_SCHEMA);
        h = fnv_u64(h, IPI_EOI_VERSION.len() as u64);
        h = fnv_u64(h, self.tick_now);
        let st = self.icr.stats();
        h = fnv_u64(h, st.high_writes);
        h = fnv_u64(h, st.low_commits);
        h = fnv_u64(h, st.order_violations);
        h = fnv_u64(h, st.busy_rewrites);
        h = fnv_u64(h, if st.sending { 1 } else { 0 });
        h = fnv_u64(h, self.tpr_class as u64);
        let mut w = 0usize;
        while w < 8 {
            h = fnv_u32(h, self.isr.words[w]);
            h = fnv_u32(h, self.irr.words[w]);
            w += 1;
        }
        let mut v = 0usize;
        while v < 256 {
            let e = self.book[v];
            let packed = (e.mode_code as u64)
                | ((e.in_service as u64) << 8)
                | ((e.no_eoi_done as u64) << 9)
                | ((e.service_count as u64) << 10)
                | ((e.seq & 0x0000_FFFF_FFFF_FFFF) << 16);
            h = fnv_u64(h, packed);
            v += 1;
        }
        h = fnv_u64(h, self.detector.check_count());
        h = fnv_u64(h, self.detector.residue_count());
        h = fnv_u64(h, if self.detector.is_red() { 1 } else { 0 });
        h = fnv_u64(h, self.detector.latched.len() as u64);
        let lg = &self.ledger;
        h = fnv_u64(h, lg.sent_total());
        h = fnv_u64(h, lg.delivered_total());
        h = fnv_u64(h, lg.serviced_total());
        h = fnv_u64(h, lg.eoi_total());
        h = fnv_u64(h, lg.no_eoi_completions());
        h = fnv_u64(h, lg.held_total());
        h = fnv_u64(h, lg.dispatched_total());
        h = fnv_u64(h, lg.spurious_rejected());
        h = fnv_u64(h, lg.invalid_rejected());
        let mut m = 0usize;
        while m < 8 {
            h = fnv_u64(h, lg.sent_by_mode()[m]);
            h = fnv_u64(h, lg.eoi_by_mode()[m]);
            h = fnv_u64(h, lg.no_eoi_by_mode()[m]);
            m += 1;
        }
        h = fnv_u64(h, lg.records().len() as u64);
        let mut r = 0usize;
        while r < lg.records().len() {
            let rec = &lg.records()[r];
            h = fnv_u64(h, rec.seq);
            h = fnv_u64(h, (rec.vector as u64) | ((rec.mode_code as u64) << 8));
            h = fnv_u64(h, (rec.shorthand_code as u64) | ((if rec.self_loop { 1u64 } else { 0 }) << 8));
            h = fnv_u64(h, rec.sent_tick);
            h = fnv_u64(h, rec.delivered_tick);
            h = fnv_u64(h, rec.serviced_tick);
            h = fnv_u64(h, rec.eoi_tick);
            h = fnv_u64(h, (rec.phase as u64).wrapping_add(if rec.no_eoi { 16 } else { 0 }));
            r += 1;
        }
        h = fnv_u64(h, self.loops.opened());
        h = fnv_u64(h, self.loops.completed());
        h = fnv_u64(h, self.loops.held_total());
        h = fnv_u64(h, self.loops.sum_latency_us_total());
        h = fnv_u64(h, self.loops.max_latency_us());
        let lp = &self.loops;
        h = fnv_u64(h, lp.records().len() as u64);
        let mut li = 0usize;
        while li < lp.records().len() {
            let lr = &lp.records()[li];
            let packed = lr.seq
                | ((lr.vector as u64) << 32)
                | ((if lr.completed { 1u64 } else { 0 }) << 40)
                | ((if lr.held { 1u64 } else { 0 }) << 41);
            h = fnv_u64(h, packed);
            h = fnv_u64(h, lr.sent_tick);
            h = fnv_u64(h, lr.done_tick);
            h = fnv_u64(h, lr.latency_us);
            li += 1;
        }
        h = fnv_u64(h, self.injections.detected());
        h = fnv_u64(h, self.injections.missed());
        h = fnv_u64(h, self.injections.records().len() as u64);
        let mut ii = 0usize;
        while ii < self.injections.records().len() {
            let ir = &self.injections.records()[ii];
            let packed = (ir.kind_code as u64)
                | ((ir.tick) << 8)
                | ((if ir.detected { 1u64 } else { 0 }) << 63);
            h = fnv_u64(h, packed);
            ii += 1;
        }
        h = fnv_u64(h, self.held.len() as u64);
        let mut hi = 0usize;
        while hi < self.held.len() {
            let he = &self.held[hi];
            let packed = he.seq | ((he.vector as u64) << 32) | (he.sent_tick << 40);
            h = fnv_u64(h, packed);
            hi += 1;
        }
        h = fnv_u64(h, self.red_flags);
        h
    }

    /// 双跑确定性对拍（J1 的结构证明面）：同种子两台新机各跑全量 campaign，
    /// 指纹与报告逐项必须一致。返回 (左指纹, 右指纹)。
    pub fn dual_run_determinism(seed: u32) -> Result<(u64, u64), IpiEoiError> {
        let mut left = IpiEoiMachine::new();
        let lrep = left.run_campaign(seed, CAMPAIGN_ITERATIONS)?;
        let mut right = IpiEoiMachine::new();
        let rrep = right.run_campaign(seed, CAMPAIGN_ITERATIONS)?;
        let lf = left.fnv_fingerprint();
        let rf = right.fnv_fingerprint();
        if lf != rf || lrep != rrep {
            return Err(IpiEoiError::FingerprintMismatch {
                left: lf,
                right: rf,
            });
        }
        Ok((lf, rf))
    }

    // ---------------- 导出面 ----------------

    /// 账本摘要（多行文本，模式分账矩阵 + 守恒式 + 健康态）。
    pub fn ledger_summary(&self) -> String {
        let lg = &self.ledger;
        let mut s = String::new();
        s.push_str("== UNX-F0844 IPI EOI 账本摘要 ==\n");
        s.push_str(&format!(
            "发送/投递/服务/EOI: {}/{}\n",
            lg.sent_total(),
            lg.delivered_total()
        ));
        s.push_str(&format!(
            "服务 {}/EOI {}/无EOI完成 {}\n",
            lg.serviced_total(),
            lg.eoi_total(),
            lg.no_eoi_completions()
        ));
        s.push_str(&format!(
            "挂起累计 {} / 被调度 {} / 拒绝(spurious {} 非法 {})\n",
            lg.held_total(),
            lg.dispatched_total(),
            lg.spurious_rejected(),
            lg.invalid_rejected()
        ));
        const MODES: [DeliveryMode; 7] = [
            DeliveryMode::Fixed,
            DeliveryMode::LowestPriority,
            DeliveryMode::Smi,
            DeliveryMode::Nmi,
            DeliveryMode::Init,
            DeliveryMode::Startup,
            DeliveryMode::ExtInt,
        ];
        let mut mi = 0usize;
        while mi < 7 {
            let m = MODES[mi];
            let c = m.code() as usize;
            s.push_str(&format!(
                "  {:<14} 发送 {} EOI {} 无EOI完成 {} 需EOI={}\n",
                m.name(),
                lg.sent_by_mode()[c],
                lg.eoi_by_mode()[c],
                lg.no_eoi_by_mode()[c],
                m.eoi_required()
            ));
            mi += 1;
        }
        s.push_str(&format!(
            "回环: 开 {} 闭 {} 挂 {} 均值 {}µs 峰值 {}µs p99 {}µs\n",
            self.loops.opened(),
            self.loops.completed(),
            self.loops.held_total(),
            self.loops.avg_latency_us(),
            self.loops.max_latency_us(),
            self.loops.p99_latency_us()
        ));
        s.push_str(&format!(
            "注入: 检出 {} 漏检 {}\n",
            self.injections.detected(),
            self.injections.missed()
        ));
        s.push_str(&format!(
            "守恒: {} 残留: {} 红案: {}\n",
            self.balance_ok(),
            self.detector.residue_count(),
            self.red_flags
        ));
        s
    }
}

// ---------------------------------------------------------------------------
// 编译期自检（const 断言：诊断码段独占 + 真值表 + 位域常数 + 判据常数）
// ---------------------------------------------------------------------------

/// 诊断码全集（段内独占性 + 界内性的编译期证据）。
const XC_SEGMENT_ALL: [u32; 16] = [
    XC_ICR_WRITE_ORDER,
    XC_ICR_BUSY_REWRITE,
    XC_EOI_RESIDUE,
    XC_VECTOR_ILLEGAL,
    XC_MODE_EOI_MISALIGN,
    XC_DOUBLE_EOI,
    XC_LOOPBACK_LATENCY,
    XC_CLOCK_REGRESSION,
    XC_LEDGER_UNBALANCED,
    XC_INJECTION_MISSED,
    XC_SHORTHAND_ILLEGAL,
    XC_FINGERPRINT_MISMATCH,
    XC_CAPACITY_EXHAUSTED,
    XC_NOT_IN_SERVICE,
    XC_RED_FLAG_LATCHED,
    XC_NO_OUTSTANDING,
];

/// const 自检：16 码全部落在 0x1910..=0x191F 且两两互异（段独占证据）。
const fn xc_codes_ok() -> bool {
    let mut i = 0usize;
    while i < 16 {
        let c = XC_SEGMENT_ALL[i];
        if c < 0x1910 || c > 0x191F {
            return false;
        }
        let mut j = i + 1;
        while j < 16 {
            if XC_SEGMENT_ALL[j] == c {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
const _: () = assert!(xc_codes_ok());

/// 真值表自检：EOI 必需模式 = {Fixed, LowestPriority, ExtINT}。
const _: () = assert!(DeliveryMode::Fixed.eoi_required());
const _: () = assert!(DeliveryMode::LowestPriority.eoi_required());
const _: () = assert!(DeliveryMode::ExtInt.eoi_required());
/// 真值表自检：无 EOI 模式 = {SMI, NMI, INIT, Startup}。
const _: () = assert!(!DeliveryMode::Smi.eoi_required());
const _: () = assert!(!DeliveryMode::Nmi.eoi_required());
const _: () = assert!(!DeliveryMode::Init.eoi_required());
const _: () = assert!(!DeliveryMode::Startup.eoi_required());

/// 回环合法域自检：0x20 入域、0x1F（异常域）出域、0xFF（spurious）出域。
const _: () = assert!(loopback_vector_ok(VECTOR_LEGAL_LO));
const _: () = assert!(!loopback_vector_ok(0x1F));
const _: () = assert!(!loopback_vector_ok(SPURIOUS_VECTOR));

/// 判据/标定常数自检（写死值与注释来源一致）。
const _: () = assert!(VECTOR_SPAN == 223);
const _: () = assert!(CLOCK_TICKS_PER_US == 100);
const _: () = assert!(LOOPBACK_LATENCY_MAX_US == 50);
const _: () = assert!(CAMPAIGN_ITERATIONS == 100_000);
const _: () = assert!(LEDGER_CAP as u64 > CAMPAIGN_ITERATIONS);
const _: () = assert!(ICR_LOW_OFFSET == 0x300 && ICR_HIGH_OFFSET == 0x310);
const _: () = assert!(EOI_REG_OFFSET == 0x0B0 && ISR_BASE_OFFSET == 0x100 && IRR_BASE_OFFSET == 0x200);
const _: () = assert!(ICR_DELIVERY_STATUS_BIT == 12 && ICR_MODE_SHIFT == 8);
const _: () = assert!(ICR_LEVEL_BIT == 14 && ICR_TRIGGER_BIT == 15 && ICR_SHORTHAND_SHIFT == 18);
const _: () = assert!(FNV_PRIME == 0x0000_0100_0000_01b3);
const _: () = assert!(FNV_OFFSET == 0xcbf2_9ce4_8422_2325);
const _: () = assert!(LCG_A == 1_664_525 && LCG_C == 1_013_904_223);

// ---------------------------------------------------------------------------
// 测试（宿主档冒烟：J1 全判据 + 注入 10/10 + 模式分账 + 双跑一致）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// J1-1：IPI EOI 10 万次零残留（ISR 位清零断言逐次覆盖 10 万次）。
    #[test]
    fn campaign_100k_zero_residue() {
        let mut m = IpiEoiMachine::new();
        let rep = m
            .run_campaign(0xF084_4001, CAMPAIGN_ITERATIONS)
            .expect("J1: 10 万次 EOI 必须零残留");
        assert_eq!(rep.iterations, CAMPAIGN_ITERATIONS);
        assert_eq!(rep.residues, 0);
        assert_eq!(rep.sent_total, CAMPAIGN_ITERATIONS);
        assert_eq!(rep.eoi_total, CAMPAIGN_ITERATIONS);
        assert!(rep.balance_ok);
        // 逐次断言覆盖面：10 万次 EOI = 10 万次清零断言。
        assert_eq!(m.detector().check_count(), CAMPAIGN_ITERATIONS);
        assert!(!m.detector().is_red());
        assert_eq!(m.in_service_count(), 0);
        assert_eq!(m.health().status, HealthStatus::Green);
        assert_eq!(m.icr_stats().order_violations, 0);
        assert_eq!(m.icr_stats().busy_rewrites, 0);
    }

    /// J1-2：自 IPI 回环时延 µs 级落账（1000 回环全闭合、直方图、p99）。
    #[test]
    fn loopback_latency_ledger_us_scale() {
        let mut m = IpiEoiMachine::new();
        let mut t: u64 = 1000;
        let mut i: u64 = 0;
        while i < 1000 {
            let vector = 0x40u8 + (i % 0x30) as u8;
            let _seq = m.self_ipi_loopback(t, vector).expect("回环发送");
            match m.self_ipi_loopback_settle(t + 260).expect("回环闭合") {
                LoopStep::Completed { latency_us } => {
                    assert!(latency_us <= 3, "回环时延越 µs 级窗: {}µs", latency_us);
                }
                LoopStep::HeldPending => panic!("空 ISR 下回环不应挂起"),
            }
            t += 400;
            i += 1;
        }
        let l = m.loops();
        assert_eq!(l.opened(), 1000);
        assert_eq!(l.completed(), 1000);
        assert_eq!(l.held_total(), 0);
        assert!(l.max_latency_us() <= LOOPBACK_LATENCY_MAX_US);
        assert!(l.p99_latency_us() <= 3);
        assert_eq!(l.histogram().total(), 1000);
        assert_eq!(l.histogram().buckets()[1], 1000); // 260 tick = 2µs → ≤2µs 桶
        assert_eq!(m.health().status, HealthStatus::Green);
        assert!(m.balance_ok());
    }

    /// 注入面 1/3：写序违例 10/10 检出，违例写不进投递账。
    #[test]
    fn write_order_injection_10_of_10() {
        let mut m = IpiEoiMachine::new();
        let mut t = 10u64;
        let mut i = 0;
        while i < 10 {
            let ok = m
                .inject_write_order_violation(t)
                .expect("写序违例必须被拦截");
            assert!(ok);
            t += 10;
            i += 1;
        }
        assert_eq!(m.injections().detected_by_kind(INJECTION_KIND_ORDER), 10);
        assert_eq!(m.icr_stats().order_violations, 10);
        assert_eq!(m.icr_stats().low_commits, 0);
        assert!(!m.has_outstanding());
        assert_eq!(m.health().status, HealthStatus::Green);
    }

    /// 注入面 2/3：ICR 忙重写 10/10 检出（Sending 期写 ICR 全拦截）。
    #[test]
    fn busy_rewrite_injection_10_of_10() {
        let mut m = IpiEoiMachine::new();
        let mut t = 10u64;
        let mut i = 0;
        while i < 10 {
            let ok = m
                .inject_icr_busy_rewrite(t)
                .expect("忙重写必须被拦截");
            assert!(ok);
            t += 10;
            i += 1;
        }
        assert_eq!(m.injections().detected_by_kind(INJECTION_KIND_BUSY), 10);
        assert_eq!(m.icr_stats().busy_rewrites, 10);
        assert!(!m.icr_stats().sending);
        assert_eq!(m.health().status, HealthStatus::Green);
    }

    /// 注入面 3/3：EOI 残留 10/10 被 latch（红案不可洗白，健康转红）。
    #[test]
    fn residue_injection_10_of_10_latches_red() {
        let mut m = IpiEoiMachine::new();
        let mut i: u64 = 0;
        while i < 10 {
            let v = 0x30u8 + i as u8;
            let ok = m
                .inject_eoi_residue(100 + i, v)
                .expect("残留注入必须被检测");
            assert!(ok);
            i += 1;
        }
        assert_eq!(m.injections().detected_by_kind(INJECTION_KIND_RESIDUE), 10);
        assert!(m.detector().is_red());
        assert_eq!(m.detector().residue_count(), 10);
        assert_eq!(m.detector().latches().len(), 10);
        assert_eq!(m.health().status, HealthStatus::Red);
        // 注入后现场已恢复：ISR 空（红案在案但账面无残留位）。
        assert_eq!(m.in_service_count(), 0);
    }

    /// J1-3：模式×EOI 需求真值表分账——NMI 不置 ISR、无 EOI 完成、对它
    /// 写 EOI = 语义错位拒绝；Fixed 全周期 EOI 落账。
    #[test]
    fn mode_eoi_accounting() {
        let mut m = IpiEoiMachine::new();
        // NMI：语义完成（无 EOI）。
        m.send_ipi(10, 0x02, DeliveryMode::Nmi, Shorthand::DestField, 0)
            .expect("NMI 发送");
        m.settle_delivery(11).expect("NMI 投递");
        let oc = m.enter_service(12).expect("NMI 服务");
        assert_eq!(oc, ServiceOutcome::CompletedNoEoi);
        assert_eq!(m.in_service_count(), 0, "NMI 不置 ISR（SDM §10.9）");
        match m.issue_eoi(13, 0x02) {
            Err(IpiEoiError::ModeEoiNotApplicable { mode_code }) => {
                assert_eq!(mode_code, DeliveryMode::Nmi.code());
            }
            other => panic!("NMI EOI 必须被拒: {:?}", other),
        }
        // INIT/Startup/SMI 同法各一笔（跨模式分账）。
        m.send_ipi(20, 0x00, DeliveryMode::Init, Shorthand::DestField, 0)
            .expect("INIT 发送");
        m.settle_delivery(21).unwrap();
        assert_eq!(m.enter_service(22).unwrap(), ServiceOutcome::CompletedNoEoi);
        m.send_ipi(30, 0xA0, DeliveryMode::Startup, Shorthand::DestField, 0)
            .expect("SIPI 发送");
        m.settle_delivery(31).unwrap();
        assert_eq!(m.enter_service(32).unwrap(), ServiceOutcome::CompletedNoEoi);
        m.send_ipi(40, 0x00, DeliveryMode::Smi, Shorthand::DestField, 0)
            .expect("SMI 发送");
        m.settle_delivery(41).unwrap();
        assert_eq!(m.enter_service(42).unwrap(), ServiceOutcome::CompletedNoEoi);
        // Fixed：全周期 EOI。
        m.send_ipi(50, 0x50, DeliveryMode::Fixed, Shorthand::DestField, 0)
            .expect("Fixed 发送");
        m.settle_delivery(51).unwrap();
        assert_eq!(m.enter_service(52).unwrap(), ServiceOutcome::Serviced);
        m.issue_eoi(53, 0x50).expect("Fixed EOI");
        // 分账核账。
        assert_eq!(m.ledger().no_eoi_by_mode()[DeliveryMode::Nmi.code() as usize], 1);
        assert_eq!(m.ledger().no_eoi_by_mode()[DeliveryMode::Init.code() as usize], 1);
        assert_eq!(m.ledger().no_eoi_by_mode()[DeliveryMode::Startup.code() as usize], 1);
        assert_eq!(m.ledger().no_eoi_by_mode()[DeliveryMode::Smi.code() as usize], 1);
        assert_eq!(m.ledger().eoi_by_mode()[DeliveryMode::Fixed.code() as usize], 1);
        assert_eq!(m.ledger().eoi_total(), 1);
        assert_eq!(m.ledger().no_eoi_completions(), 4);
        assert!(m.balance_ok());
        assert_eq!(m.health().status, HealthStatus::Green);
    }

    /// 模式分账 campaign：7 类模式轮转，分账矩阵投影与真值表逐格一致。
    #[test]
    fn mode_campaign_matrix() {
        let mut m = IpiEoiMachine::new();
        let rep = m
            .run_mode_campaign(0x0844_0003, 700)
            .expect("模式 campaign 应全绿");
        assert_eq!(rep.iterations, 700);
        assert_eq!(rep.residues, 0);
        assert!(rep.balance_ok);
        let mut sent_sum = 0u64;
        let mut eoi_sum = 0u64;
        let mut no_eoi_sum = 0u64;
        let mut k = 0usize;
        while k < 8 {
            sent_sum += rep.sent_by_mode[k];
            eoi_sum += rep.eoi_by_mode[k];
            no_eoi_sum += rep.no_eoi_by_mode[k];
            k += 1;
        }
        assert_eq!(sent_sum, 700);
        assert_eq!(eoi_sum + no_eoi_sum, 700);
        // EOI 必需模式不产生无 EOI 完成；无 EOI 模式不产生 EOI。
        for m_mode in [
            DeliveryMode::Fixed,
            DeliveryMode::LowestPriority,
            DeliveryMode::ExtInt,
        ] {
            assert_eq!(rep.no_eoi_by_mode[m_mode.code() as usize], 0);
        }
        for m_mode in [
            DeliveryMode::Smi,
            DeliveryMode::Nmi,
            DeliveryMode::Init,
            DeliveryMode::Startup,
        ] {
            assert_eq!(rep.eoi_by_mode[m_mode.code() as usize], 0);
            assert!(rep.no_eoi_by_mode[m_mode.code() as usize] > 0);
        }
    }

    /// J1-4：回环不打断当前服务态——在服务 0xF0（类 0xF）时回环 0x50
    /// 挂 IRR 等待；EOI 后重裁调度、回环账含挂起经历闭合。
    #[test]
    fn hold_semantics_loopback_not_interrupting() {
        let mut m = IpiEoiMachine::new();
        m.send_ipi(100, 0xF0, DeliveryMode::Fixed, Shorthand::DestField, 0)
            .expect("阻塞向量发送");
        m.settle_delivery(101).unwrap();
        m.enter_service(102).unwrap();
        assert_eq!(m.in_service_count(), 1);
        // 回环 0x50（类 0x5）——低于在服务类，必须挂起。
        let _seq = m.self_ipi_loopback(103, 0x50).expect("回环发送");
        match m.self_ipi_loopback_settle(105).expect("回环投递+服务") {
            LoopStep::HeldPending => {}
            LoopStep::Completed { .. } => panic!("低类回环不得打断高类服务态"),
        }
        assert_eq!(m.held_pending(), 1);
        assert_eq!(m.in_service_count(), 1, "回环不打断当前服务态");
        let w = m.isr_words();
        assert_eq!((w[(0xF0 >> 5) as usize] >> (0xF0 & 31)) & 1, 1, "0xF0 仍在服务");
        assert_eq!((w[(0x50 >> 5) as usize] >> (0x50 & 31)) & 1, 0, "0x50 未抢占");
        let iw = m.irr_words();
        assert_eq!((iw[(0x50 >> 5) as usize] >> (0x50 & 31)) & 1, 1, "0x50 挂 IRR");
        // EOI 阻塞向量 → 重裁调度挂起项 → 回环闭合（含挂起经历）。
        m.issue_eoi(110, 0xF0).expect("阻塞向量 EOI");
        assert_eq!(m.held_pending(), 0);
        assert_eq!(m.in_service_count(), 1, "挂起项已被调度进服务态");
        assert_eq!(m.loops().completed(), 1);
        assert_eq!(m.loops().held_total(), 1);
        assert!(m.loops().records()[0].held);
        assert!(m.loops().records()[0].completed);
        m.issue_eoi(120, 0x50).expect("被调度向量 EOI");
        assert_eq!(m.in_service_count(), 0);
        assert!(m.balance_ok());
        assert_eq!(m.health().status, HealthStatus::Green);
    }

    /// J1-5：TPR 闸——TPR 类 0xF 时空 ISR 下回环仍挂起；TPR 降档后可调度。
    #[test]
    fn tpr_gate_holds_and_release() {
        let mut m = IpiEoiMachine::new();
        m.set_tpr_class(0xF);
        let _seq = m.self_ipi_loopback(200, 0x50).expect("回环发送");
        match m.self_ipi_loopback_settle(201).expect("回环投递+服务") {
            LoopStep::HeldPending => {}
            LoopStep::Completed { .. } => panic!("TPR 类 0xF 必须闸住类 0x5 回环"),
        }
        assert_eq!(m.held_pending(), 1);
        // TPR 未降档时手动调度也不得放出（阻断类仍 0xF）。
        assert_eq!(m.dispatch_held(202).expect("调度"), None);
        assert_eq!(m.held_pending(), 1);
        m.set_tpr_class(0x0);
        assert_eq!(m.dispatch_held(203).expect("调度"), Some(0x50));
        assert_eq!(m.loops().completed(), 1);
        m.issue_eoi(204, 0x50).expect("EOI");
        assert_eq!(m.health().status, HealthStatus::Green);
    }

    /// J1-6：双跑确定性对拍——同种子两台新机各 10 万次 EOI，指纹与报告
    /// 逐位一致（零残留是结构保证而非碰运气的证明面）。
    #[test]
    fn dual_run_determinism_100k() {
        let (a, b) = IpiEoiMachine::dual_run_determinism(0x0844_0007)
            .expect("同种子双跑指纹必须一致");
        assert_eq!(a, b);
        assert_ne!(a, 0);
    }

    /// 指纹敏感性：不同种子（不同向量序列）指纹必不同。
    #[test]
    fn fingerprint_seed_sensitivity() {
        let mut a = IpiEoiMachine::new();
        let mut b = IpiEoiMachine::new();
        let _ = a.run_campaign(0x1111_1111, 1000).unwrap();
        let _ = b.run_campaign(0x2222_2222, 1000).unwrap();
        assert_ne!(a.fnv_fingerprint(), b.fnv_fingerprint());
        // 同机重放同序列：再跑同种子一段后仍与镜像机对齐（确定性贯穿多段）。
        let mut c = IpiEoiMachine::new();
        let _ = c.run_campaign(0x1111_1111, 1000).unwrap();
        assert_eq!(a.fnv_fingerprint(), c.fnv_fingerprint());
    }

    /// 回环向量合法域：<0x20 异常域与 0xFF spurious 均拒绝。
    #[test]
    fn loopback_vector_domain_rejected() {
        let mut m = IpiEoiMachine::new();
        match m.self_ipi_loopback(1, 0x1F) {
            Err(IpiEoiError::InvalidVector { vector }) => assert_eq!(vector, 0x1F),
            other => panic!("异常域向量必须拒: {:?}", other),
        }
        match m.self_ipi_loopback(2, SPURIOUS_VECTOR) {
            Err(IpiEoiError::InvalidVector { vector }) => assert_eq!(vector, SPURIOUS_VECTOR),
            other => panic!("spurious 必须拒: {:?}", other),
        }
        assert_eq!(m.ledger().invalid_rejected(), 2);
        assert_eq!(m.loops().opened(), 0);
    }

    /// 简写域组合纪律：INIT/Startup×SelfOnly 拒绝。
    #[test]
    fn shorthand_illegal_rejected() {
        let mut m = IpiEoiMachine::new();
        match m.send_ipi(1, 0x00, DeliveryMode::Init, Shorthand::SelfOnly, 0) {
            Err(IpiEoiError::ShorthandIllegal {
                shorthand_code,
                mode_code,
            }) => {
                assert_eq!(shorthand_code, Shorthand::SelfOnly.code());
                assert_eq!(mode_code, DeliveryMode::Init.code());
            }
            other => panic!("INIT×Self 必须拒: {:?}", other),
        }
        match m.send_ipi(2, 0xA0, DeliveryMode::Startup, Shorthand::SelfOnly, 0) {
            Err(IpiEoiError::ShorthandIllegal { .. }) => {}
            other => panic!("SIPI×Self 必须拒: {:?}", other),
        }
        // 合法组合放行。
        m.send_ipi(3, 0x00, DeliveryMode::Init, Shorthand::DestField, 0)
            .expect("INIT×DestField 合法");
    }

    /// spurious 向量 EOI 拒绝（SDM §10.9：spurious 不需要也不应 EOI）。
    #[test]
    fn spurious_eoi_rejected() {
        let mut m = IpiEoiMachine::new();
        match m.issue_eoi(1, SPURIOUS_VECTOR) {
            Err(IpiEoiError::InvalidVector { vector }) => assert_eq!(vector, SPURIOUS_VECTOR),
            other => panic!("spurious EOI 必须拒: {:?}", other),
        }
        assert_eq!(m.ledger().spurious_rejected(), 1);
    }

    /// 双重 EOI 与不在服务 EOI 两种拒绝路径分立。
    #[test]
    fn double_eoi_and_not_in_service_rejected() {
        let mut m = IpiEoiMachine::new();
        // 从未服务：VectorNotInService。
        match m.issue_eoi(1, 0x40) {
            Err(IpiEoiError::VectorNotInService { vector }) => assert_eq!(vector, 0x40),
            other => panic!("未服务 EOI 必须拒: {:?}", other),
        }
        // 服务一次并 EOI 后再 EOI：DoubleEoi。
        m.send_ipi(2, 0x40, DeliveryMode::Fixed, Shorthand::DestField, 0)
            .unwrap();
        m.settle_delivery(3).unwrap();
        m.enter_service(4).unwrap();
        m.issue_eoi(5, 0x40).unwrap();
        match m.issue_eoi(6, 0x40) {
            Err(IpiEoiError::DoubleEoi { vector }) => assert_eq!(vector, 0x40),
            other => panic!("双重 EOI 必须拒: {:?}", other),
        }
        assert!(m.balance_ok());
    }

    /// 写序纪律正路径：高字→低字合法序通过；低字先写（真机 bug 模式）拦截。
    #[test]
    fn icr_write_order_discipline() {
        let mut t = IcrWriteTracker::new();
        // 低字先写 → 违例。
        let low = compose_icr_low(0x40, DeliveryMode::Fixed, true, false, Shorthand::SelfOnly);
        match t.write_low(low) {
            Err(IpiEoiError::WriteOrderViolation) => {}
            other => panic!("低字先写必须拦截: {:?}", other),
        }
        // 合法序：高字→低字。
        t.write_high(0x0100_0000).expect("高字写");
        let f = t.write_low(low).expect("低字提交");
        assert_eq!(f.vector, 0x40);
        assert_eq!(f.mode, DeliveryMode::Fixed);
        assert_eq!(f.shorthand, Shorthand::SelfOnly);
        assert!(f.level_assert);
        assert!(!f.trigger_level);
        assert!(t.stats().sending);
        // 投递落定回 Idle。
        t.settle();
        assert!(!t.stats().sending);
        assert_eq!(t.stats().low_commits, 1);
        assert_eq!(t.stats().high_writes, 1);
    }

    /// ICR bit 域往返：7 类模式 × 4 类简写组包→解包无损。
    #[test]
    fn icr_bitfield_roundtrip_all_modes() {
        const MODES: [DeliveryMode; 7] = [
            DeliveryMode::Fixed,
            DeliveryMode::LowestPriority,
            DeliveryMode::Smi,
            DeliveryMode::Nmi,
            DeliveryMode::Init,
            DeliveryMode::Startup,
            DeliveryMode::ExtInt,
        ];
        const SHS: [Shorthand; 4] = [
            Shorthand::DestField,
            Shorthand::SelfOnly,
            Shorthand::AllIncludingSelf,
            Shorthand::AllExcludingSelf,
        ];
        for mode in MODES.iter() {
            for sh in SHS.iter() {
                for trigger in [false, true].iter() {
                    let w = compose_icr_low(0x7E, *mode, true, *trigger, *sh);
                    let f = parse_icr_low(w);
                    assert_eq!(f.vector, 0x7E);
                    assert_eq!(f.mode, *mode);
                    assert_eq!(f.shorthand, *sh);
                    assert_eq!(f.trigger_level, *trigger);
                    assert!(f.level_assert);
                }
            }
        }
        // Delivery Status 位号写入验证（bit 12）。
        let w = compose_icr_low(0x40, DeliveryMode::Fixed, true, false, Shorthand::SelfOnly);
        assert_eq!((w >> ICR_DELIVERY_STATUS_BIT) & 1, 0, "组包不置 DS 位——DS 由硬件置位");
    }

    /// 向量位图：最高位扫描、残留清单、计数与快照。
    #[test]
    fn vector_bitmap_highest_and_residue() {
        let mut b = VectorBitmap::new();
        assert_eq!(b.highest(), None);
        assert_eq!(b.count(), 0);
        b.set(0x20);
        b.set(0xFE);
        b.set(0x7F);
        assert_eq!(b.highest(), Some(0xFE));
        assert_eq!(b.count(), 3);
        let rl = b.residue_list();
        assert_eq!(rl.len(), 3);
        assert_eq!(rl[0], 0x20);
        assert_eq!(rl[2], 0xFE);
        b.clear(0xFE);
        assert_eq!(b.highest(), Some(0x7F));
        let words = b.words();
        assert_eq!(b.count(), words.iter().map(|x| x.count_ones() as u64).sum());
        b.clear(0x20);
        b.clear(0x7F);
        assert_eq!(b.highest(), None);
    }

    /// 时钟回退拦截 + 红案单调。
    #[test]
    fn clock_regression_red() {
        let mut m = IpiEoiMachine::new();
        m.advance_clock(10).expect("正向推进");
        match m.advance_clock(5) {
            Err(IpiEoiError::ClockRegression { prev, next }) => {
                assert_eq!(prev, 10);
                assert_eq!(next, 5);
            }
            other => panic!("时钟回退必须拒: {:?}", other),
        }
        assert_eq!(m.red_flags(), 1);
        assert_eq!(m.health().status, HealthStatus::Red);
        // 单调不回退的等值推进合法。
        m.advance_clock(10).expect("等值推进");
    }

    /// 错误恢复：busy 悬挂后 recover_idle 恢复发送面，账目继续可推。
    #[test]
    fn icr_recover_idle_after_busy() {
        let mut m = IpiEoiMachine::new();
        m.send_ipi(1, 0x40, DeliveryMode::Fixed, Shorthand::DestField, 0)
            .expect("首笔发送");
        // 不投递直接再发：Delivery Status=Sending → 忙重写。
        match m.send_ipi(2, 0x41, DeliveryMode::Fixed, Shorthand::DestField, 0) {
            Err(IpiEoiError::IcrBusyRewrite) => {}
            other => panic!("Sending 期重发必须拒: {:?}", other),
        }
        m.icr_recover_idle();
        // 原在途事务继续走完生命周期。
        m.settle_delivery(3).expect("投递落定");
        m.enter_service(4).expect("服务");
        m.issue_eoi(5, 0x40).expect("EOI");
        assert_eq!(m.ledger().sent_total(), 1);
        assert!(m.balance_ok());
    }

    /// 全部 16 个错误变体：诊断码落在 0x1910..=0x191F、两两互异、
    /// detail() 中文诊断非空。
    #[test]
    fn error_variants_codes_and_details() {
        let errs: [IpiEoiError; 16] = [
            IpiEoiError::WriteOrderViolation,
            IpiEoiError::IcrBusyRewrite,
            IpiEoiError::EoiResidue { vector: 0x40 },
            IpiEoiError::InvalidVector { vector: 0x1F },
            IpiEoiError::ModeEoiNotApplicable { mode_code: 4 },
            IpiEoiError::DoubleEoi { vector: 0x40 },
            IpiEoiError::LoopbackLatencyOut {
                latency_us: 51,
                bound_us: LOOPBACK_LATENCY_MAX_US,
            },
            IpiEoiError::ClockRegression { prev: 10, next: 5 },
            IpiEoiError::LedgerUnbalanced { sent: 1, retired: 0 },
            IpiEoiError::InjectionMissed { kind_code: 1 },
            IpiEoiError::ShorthandIllegal {
                shorthand_code: 1,
                mode_code: 5,
            },
            IpiEoiError::FingerprintMismatch { left: 1, right: 2 },
            IpiEoiError::CapacityExhausted,
            IpiEoiError::VectorNotInService { vector: 0x40 },
            IpiEoiError::RedFlagLatched { code: XC_EOI_RESIDUE },
            IpiEoiError::NoOutstandingSend,
        ];
        assert_eq!(errs.len(), 16);
        let mut i = 0usize;
        while i < 16 {
            assert!(!errs[i].detail().is_empty(), "变体 {} 缺诊断", i);
            assert!(
                (0x1910..=0x191F).contains(&errs[i].code()),
                "码 {:#X} 越段",
                errs[i].code()
            );
            let mut j = i + 1;
            while j < 16 {
                assert_ne!(errs[i].code(), errs[j].code(), "码位冲突");
                j += 1;
            }
            i += 1;
        }
        // 诊断码常量与枚举映射互证。
        assert_eq!(IpiEoiError::WriteOrderViolation.code(), XC_ICR_WRITE_ORDER);
        assert_eq!(IpiEoiError::EoiResidue { vector: 1 }.code(), XC_EOI_RESIDUE);
        assert_eq!(IpiEoiError::NoOutstandingSend.code(), XC_NO_OUTSTANDING);
    }

    /// 账本摘要导出冒烟：campaign + 回环 + 注入后摘要含各分账段。
    #[test]
    fn ledger_summary_smoke() {
        let mut m = IpiEoiMachine::new();
        let _ = m.run_campaign(0x0BAD_0001, 50).unwrap();
        let _ = m.self_ipi_loopback(10_000, 0x60).unwrap();
        match m.self_ipi_loopback_settle(10_260).unwrap() {
            LoopStep::Completed { .. } => {}
            other => panic!("回环应闭合: {:?}", other),
        }
        let _ = m.inject_write_order_violation(20_000).unwrap();
        let s = m.ledger_summary();
        assert!(s.contains("UNX-F0844"));
        assert!(s.contains("守恒"));
        assert!(s.contains("Fixed"));
        assert!(s.contains("p99"));
        assert!(s.contains("注入"));
        assert!(s.contains("回环"));
    }

    /// 空机健康面：全绿、守恒式成立、无注入。
    #[test]
    fn fresh_machine_health_green() {
        let m = IpiEoiMachine::new();
        let h = m.health();
        assert_eq!(h.status, HealthStatus::Green);
        assert_eq!(h.red_flags, 0);
        assert_eq!(h.residues, 0);
        assert!(h.balance_ok);
        assert_eq!(h.held_pending, 0);
        assert_eq!(h.in_service, 0);
        assert!(m.balance_ok());
        assert_eq!(m.fnv_fingerprint(), IpiEoiMachine::new().fnv_fingerprint());
    }

    /// J1 补强：第二个种子再跑 10 万（判据稳定性，非单种子单例）。
    #[test]
    fn campaign_100k_second_seed() {
        let mut m = IpiEoiMachine::new();
        let rep = m
            .run_campaign(0xF084_4002, CAMPAIGN_ITERATIONS)
            .expect("第二个种子同样零残留");
        assert_eq!(rep.residues, 0);
        assert_eq!(rep.eoi_total, CAMPAIGN_ITERATIONS);
        assert_eq!(m.detector().check_count(), CAMPAIGN_ITERATIONS);
        assert!(m.health().status == HealthStatus::Green);
    }
}
