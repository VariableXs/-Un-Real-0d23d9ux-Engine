//! =====================================================================================
//! 任务单：UNX-F0862 · RTE 间接寻址读写原语与访序纪律
//! 域/批：CoRun·UNX-A2-B04（IO-APIC 路由与 MSI-X 分配器地基，中断面第 4 批，F861–F880）
//! 册目标：300 行 ｜ 本文件实际：≥1800 行（深化版）
//! 判据原文（册内定位：CoRun Varix STAR II · Unxreal.md 第 2508 行，逐字抄录）：
//!   UNX-F0862 | RTE 间接寻址读写原语与访序纪律 | 300 | 已深化 |
//!   UNX-F0862-J1 原语读写一致性 100%（写入读回逐字段），并发访序注入 10/10 次
//!   被串行化保护，64 位拆写窗口账可见
//! 批次原文（同册第 2500 行起）：AI-02 承办｜嫁接源：Intel SDM Vol.3 §10.11/§10.12
//! （MP 规范 §4.3），只跟随｜上游：F0004 MADT 交付面、F0821 LAPIC 基址面｜
//! 防重：对 F0023（IO-APIC 纲领）为分层深化——本条把「读写原语」这一工程面铺开。
//! 诊断码段：0x1970–0x197F（本单独占，16 码闭集；0x1920 段已被
//! svstar2/ves470_s25docs.rs 占用，0x1970–0x197F 经全仓 grep 确认无诊断码占用——
//! 仓内 0x1972 命中均为 PCI Device ID 与诊断码段无涉）。
//! =====================================================================================
//!
//! ## 纯功能边界声明
//! - `no_std + alloc`；零 `unwrap()` / `expect()` / `panic!` / `todo!()`；
//!   运行期零 `assert!`（编译期 `const _: () = assert!(…)` 允许）；零浮点、
//!   零真实端口/真实 MMIO IO、零墙钟、零线程原语。
//! - 可失败面全走 `Result<T, RteRwError>`；错误闭集 16 变体，逐变体
//!   `detail(&self) -> &'static str` 中文可操作诊断（现象/位置/建议修法）。
//! - 溢出全部显性 `wrapping_*` / `saturating_*`。
//! - 时间一律为逻辑刻 `tick: u64`，由引擎内 `advance()` 单调推进，不读真实时钟。
//! - 随机性一律为确定性 LCG：
//!   `state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)`
//! - 指纹为冻结 FNV-1a：basis `0xcbf29ce484222325`，prime `0x100000001b3`。
//! - `const fn` 纪律：const 环境内只用 while+索引（不用 for / slice::get /
//!   Ord::min / Ord::max），不用派生 PartialEq 的 `==`；含 Vec 字段的结构体
//!   一律不 derive Copy。
//!
//! ## 组件清单（本单交付件）
//! 1. `RteField`：RTE 位字段模型 9 字段（向量 0:7 / 投递模式 8:10 / 目的模式 11 /
//!    投递状态 12 / 极性 13 / 远程 IRR 14 / 触发 15 / 掩码 16 / 目的地 56:63），
//!    位位置/宽度/掩码全 const fn，SDM Vol.3 §10.11 逐位对照。
//! 2. `RteWord`：64 位 RTE 字（高低双 32 位字），字段抽取 const fn。
//! 3. `RteHw`：IO-APIC 寄存器文件模型——每 RTE 四通道（低字值/高字值/低字纪元/
//!    高字纪元），纪元戳是拆写窗口账的物理基础。
//! 4. `IndirectAddrProtocol`：间接寻址协议状态机——IOREGSEL 写寄存器号→
//!    IOWIN 读/写数据→取消选通；越界 RTE 号拒收；未选通直接 IOWIN 访问拦截。
//! 5. `RteSplitWindow`：64 位拆写窗口账——RTE 是 64 位而 IOWIN 是 32 位口，
//!    高低两次写之间存在窗口期；窗口期进入/退出时戳、窗口期内并发读到的
//!    半新半旧态显性标记（半新半旧不是 bug 是窗口语义）、读方重组校验。
//! 6. `RwSerializer`：原语级互斥串行化——持有者/等待者计数/持有时长账，
//!    并发访序注入 10/10 次被串行化保护。
//! 7. `ConsistencyLedger`：写入读回一致性账——逐字段写→读回→比对，字段级 diff
//!    与不一致定位（哪个 RTE 哪个字段期望值/实际值）。
//! 8. `EventLedger`：append-only 环形事件账（容量 256，溢出丢弃显性自增）。
//! 9. 注入核：无选通注入 / 窗口期跨 RTE 切换注入 / 写序颠倒注入 / 非法向量注入，
//!    全部被拦截并留痕。
//! 10. `RteRwEngine`：编排器——访序纪律（低字→高字；窗口期内禁跨 RTE 切换选通）、
//!     健康账 `health()`、FNV-1a 指纹、双跑确定性对拍。
//!
//! ## 职责定位
//! 本条**只做 RTE 间接寻址读写原语的语义建模与访序纪律**：不写真实端口/真实
//! MMIO IO（真实映射属 F0861 探测面与 F0863 编址面）、不做 RTE 字段编码器的
//! 全量非法组合矩阵（F0865）、不做引脚掩码状态机（F0866）。本条是「怎么安全地
//! 读写一个 64 位 RTE」的原语层：IOREGSEL/IOWIN 双寄存器间接寻址的状态机纪律、
//! 64 位拆写窗口的账面可见性、以及原语级串行化保护。
//!
//! ## 与现存内核的衔接点
//! 上游：F0004 MADT 交付面给出 IO-APIC 实例清单（`IoApicEntry`），F0861 探测面
//! 给出每实例引脚容量（即 RTE 数）；本条以 `RTE_COUNT = 24`（Intel 82093AA
//! 最大重定向表项数，SDM Vol.3 §10.11）为默认容量建模。下游：F0863 GSI 编址、
//! F0864 ISA Override 裁决、F0865 字段编码器都消费本条的读写原语纪律。
//!
//! ## 与 Windows 同类能力的对照（深在哪）
//! Windows 对照：Windows 的 HAL 在改动 IO-APIC 重定向表项时走同一条
//! IOREGSEL/IOWIN 间接寻址路径，但其拆写窗口期对可观测面完全不透明——没有
//! 「窗口期内读到的半新半旧态」的账面（Windows 只保证程序自身的串行化，靠
//! IRQL 抬升而非显性互斥账）。本单把窗口期做成一等账：进入/退出时戳、窗口内
//! 并发读观测、读方重组校验全部显性可见；串行化是带等待者/持有时长账的显性
//! 互斥模型，不是隐式 IRQL 假设。这是 Windows 公开面没有的可审计性深度。
//!
//! ## 判据复测方式
//! UNX-F0862-J1 三成分：
//! 1. 原语读写一致性 100%：`observe(seed)` 跑一致性扫掠（24 RTE × 7 可编程字段
//!    = 168 次逐字段写入读回比对），`consistency_pct == 100`；
//! 2. 并发访序注入 10/10 次被串行化保护：`serialized_rounds == 10` 且
//!    `serialized_protected == 10`（每次注入都被串行化为等待者，无一穿透）；
//! 3. 64 位拆写窗口账可见：`window_visible == true`（窗口账条目 >0、窗口内
//!    半新半旧并发读观测 >0、全部条目读方重组校验通过、进入/退出时戳齐全）。
//! 附加：双跑确定性对拍（同种子两次运行指纹一致）、注入面三检出、`health()` 全绿。
#![allow(dead_code)]
extern crate alloc;
use alloc::{format, string::{String, ToString}, vec, vec::Vec};

// ===========================================================================
// 0. 常量表：判据里每一个数字阈值都是具名常量，注明来源
// ===========================================================================
/// 判据编号。
pub const CRITERION_ID: &str = "UNX-F0862-J1";
/// 判据原文（逐字，册内第 2508 行）。
pub const CRITERION_TEXT: &str = "UNX-F0862-J1 原语读写一致性 100%（写入读回逐字段），并发访序注入 10/10 次被串行化保护，64 位拆写窗口账可见";
/// 模块版本标识（写入导出组头与描述符）。
pub const VERSION_ID: u64 = 0x1086200000001;
/// 导出文本版本串。
pub const VERSION_TEXT: &str = "unx0862.rterw.v1";
/// 诊断码段基址（本单独占；0x1920 段已被 ves470_s25docs.rs 占用，故取 0x1970）。
pub const SEG_BASE: u16 = 0x1970;
/// 诊断码段掩码（段内变体序号宽度：16 码）。
pub const SEG_MASK: u16 = 0x0F;
/// 诊断码段上界（闭）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;

/// RTE 容量：24 条重定向表项（Intel 82093AA IO-APIC 最大引脚数，SDM Vol.3 §10.11）。
pub const RTE_COUNT: usize = 24;
/// IOREGSEL 寄存器在实例 MMIO 空间内的偏移（82093AA 数据手册）。
pub const IOREGSEL_OFFSET: u32 = 0x00;
/// IOWIN 寄存器在实例 MMIO 空间内的偏移（82093AA 数据手册）。
pub const IOWIN_OFFSET: u32 = 0x10;
/// RTE 寄存器号在 IOREGSEL 地址空间内的基址（0x10 起，每 RTE 占相邻两号：
/// 低字号 0x10+2n，高字号 0x11+2n，SDM Vol.3 §10.11）。
pub const RTE_BASE_INDEX: u32 = 0x10;
/// 非 RTE 寄存器号区间宽度（0x00–0x0F：IOAPICID/IOAPICVER/IOAPICARB 及保留区，
/// 本协议面只开放 RTE 间接寻址，该区间一律拒收）。
pub const NON_RTE_INDEX_SPAN: u32 = 0x10;
/// 向量字段可编程下界（0x00–0x1F 为 CPU 异常保留区，SDM Vol.3 §6.15）。
pub const VECTOR_LO: u64 = 0x20;
/// 向量字段可编程上界（0xFF 为特殊广播语义，禁用作常规向量，SDM Vol.3 §10.11）。
pub const VECTOR_HI: u64 = 0xFE;
/// 半字标识：低字。
pub const HALF_LO: u32 = 0;
/// 半字标识：高字。
pub const HALF_HI: u32 = 1;
/// 半字通道总数（每 RTE 低字值/高字值/低字纪元/高字纪元共 4 通道）。
pub const HALF_CHANNELS: usize = 2;
/// RTE 字段总数（SDM Vol.3 §10.11 重定向表项位定义）。
pub const RTE_FIELD_COUNT: usize = 9;
/// 可编程字段数（投递状态与远程 IRR 为硬件只读状态位，不参与写入读回一致性）。
pub const PROGRAMMABLE_FIELD_COUNT: usize = 7;
/// 拆写窗口滞留预算（刻）：窗口开得太久即纪律违规显性化（模型口径：高低两写
/// 之间不得跨过 8 个逻辑刻，来源：本单访序纪律建模约定）。
pub const WINDOW_OPEN_BUDGET: u64 = 8;
/// 串行化持有时长预算（刻）：持有超过即抬超预算账（模型口径：一次原语读写
/// 含读回校验不得跨过 64 个逻辑刻）。
pub const HOLD_BUDGET: u64 = 64;
/// 事件账容量（append-only 环形，溢出丢弃显性自增）。
pub const EVENT_RING_CAP: usize = 256;
/// 窗口账容量（append-only，溢出丢弃显性自增）。
pub const WINDOW_LEDGER_CAP: usize = 64;
/// 一致性 diff 账容量（定位账，溢出丢弃显性自增）。
pub const DIFF_LEDGER_CAP: usize = 32;
/// 判据要求的并发访序注入轮数（10/10 次被串行化保护）。
pub const SERIALIZED_INJECT_ROUNDS: u64 = 10;
/// 一致性目标（判据：100%）。
pub const CONSISTENCY_TARGET_PCT: u64 = 100;
/// 非法注入值（未选通注入时塞进 IOWIN 的哨兵值）。
pub const INJECT_SENTINEL: u32 = 0xDEAD_BEEF;
/// RTE 无效哨兵（事件账中「不指向具体 RTE」的占位）。
pub const RTE_NONE: u8 = 0xFF;
/// 冻结 FNV-1a basis（FNV 参考实现）。
pub const FNV_BASIS: u64 = 0xcbf29ce484222325;
/// 冻结 FNV-1a prime（FNV 参考实现）。
pub const FNV_PRIME: u64 = 0x100000001b3;
/// 确定性 LCG 乘子（Knuth MMIX 参数）。
pub const LCG_MUL: u64 = 6364136223846793005;
/// 确定性 LCG 增量（Knuth MMIX 参数）。
pub const LCG_ADD: u64 = 1442695040888963407;
/// 引擎编排者所有者号基址（串行化账中的持有者标识）。
pub const OWNER_BASE: u64 = 0xA086_2000;
/// 串行化注入的合法持有者标识。
pub const OWNER_HOLDER: u64 = 0x5EED_0001;
/// 串行化注入的入侵者标识。
pub const OWNER_INTRUDER: u64 = 0xBAD_C0DE;

// 编译期封印：段基址与段上界必须落在独占区间内。
const _: () = assert!(SEG_BASE == 0x1970);
const _: () = assert!(SEG_END == 0x197F);
const _: () = assert!(RTE_BASE_INDEX == 0x10);
const _: () = assert!(RTE_COUNT == 24);
const _: () = assert!(EVENT_RING_CAP >= 128);

// ===========================================================================
// 1. 错误闭集：每变体一码，detail() 给「现象/位置/建议修法」三要素
// ===========================================================================
/// 本单错误闭集。变体序号从 0 起，逐变体占用 `SEG_BASE + n` 一个码，
/// 16 变体恰好占满 0x1970–0x197F。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RteRwError {
    /// rte_index_out_of_range：IOREGSEL 收到非 RTE 号（越界或非 RTE 寄存器区间）。
    RteIndexOutOfRange,
    /// iowin_access_without_select：未选通即访问 IOWIN。
    AccessWithoutSelect,
    /// cross_rte_switch_in_window：拆写窗口期内跨 RTE 切换选通。
    CrossRteSwitchInWindow,
    /// write_order_inverted：高字先于低字写（写序颠倒）。
    WriteOrderInverted,
    /// window_left_unclosed：前一窗口未闭合又开新窗口。
    WindowLeftUnclosed,
    /// duplicate_half_write：同一半字在同一窗口内被重复写。
    DuplicateHalfWrite,
    /// readback_field_mismatch：写入读回逐字段比对不一致。
    ReadbackMismatch,
    /// reassembly_epoch_mismatch：读方重组校验失败（高低字纪元不一致且非窗口语义）。
    ReassemblyMismatch,
    /// serialize_hold_over_budget：串行化持有时长超预算。
    SerializeOverBudget,
    /// field_value_invalid：字段值超出位宽或落在保留区间。
    FieldInvalid,
    /// fingerprint_mismatch：指纹对拍不一致。
    FingerprintMismatch,
    /// ledger_capacity_exhausted：账容量耗尽（丢弃已显性化）。
    LedgerFull,
    /// consume_before_window_closed：窗口期内的半新半旧态被消费（消费纪律拦截）。
    ConsumeBeforeClosed,
    /// proto_state_invalid：协议状态机收到当前态不允许的迁移。
    ProtoStateInvalid,
    /// tick_regression：逻辑刻倒流。
    TickRegression,
    /// window_open_over_budget：窗口滞留超预算仍不闭合。
    WindowOpenOverBudget,
}

impl RteRwError {
    /// 诊断码（SEG_BASE + 变体序号，独占 0x1970–0x197F）。
    pub fn code(&self) -> u16 {
        let n: u16 = match self {
            RteRwError::RteIndexOutOfRange => 0x00,
            RteRwError::AccessWithoutSelect => 0x01,
            RteRwError::CrossRteSwitchInWindow => 0x02,
            RteRwError::WriteOrderInverted => 0x03,
            RteRwError::WindowLeftUnclosed => 0x04,
            RteRwError::DuplicateHalfWrite => 0x05,
            RteRwError::ReadbackMismatch => 0x06,
            RteRwError::ReassemblyMismatch => 0x07,
            RteRwError::SerializeOverBudget => 0x08,
            RteRwError::FieldInvalid => 0x09,
            RteRwError::FingerprintMismatch => 0x0A,
            RteRwError::LedgerFull => 0x0B,
            RteRwError::ConsumeBeforeClosed => 0x0C,
            RteRwError::ProtoStateInvalid => 0x0D,
            RteRwError::TickRegression => 0x0E,
            RteRwError::WindowOpenOverBudget => 0x0F,
        };
        SEG_BASE + n
    }
    /// 变体稳定名（账面与导出面共用）。
    pub fn name(&self) -> &'static str {
        match self {
            RteRwError::RteIndexOutOfRange => "rte_index_out_of_range",
            RteRwError::AccessWithoutSelect => "iowin_access_without_select",
            RteRwError::CrossRteSwitchInWindow => "cross_rte_switch_in_window",
            RteRwError::WriteOrderInverted => "write_order_inverted",
            RteRwError::WindowLeftUnclosed => "window_left_unclosed",
            RteRwError::DuplicateHalfWrite => "duplicate_half_write",
            RteRwError::ReadbackMismatch => "readback_field_mismatch",
            RteRwError::ReassemblyMismatch => "reassembly_epoch_mismatch",
            RteRwError::SerializeOverBudget => "serialize_hold_over_budget",
            RteRwError::FieldInvalid => "field_value_invalid",
            RteRwError::FingerprintMismatch => "fingerprint_mismatch",
            RteRwError::LedgerFull => "ledger_capacity_exhausted",
            RteRwError::ConsumeBeforeClosed => "consume_before_window_closed",
            RteRwError::ProtoStateInvalid => "proto_state_invalid",
            RteRwError::TickRegression => "tick_regression",
            RteRwError::WindowOpenOverBudget => "window_open_over_budget",
        }
    }
    /// 中文可操作诊断（现象/位置/建议修法三要素）。
    pub fn detail(&self) -> &'static str {
        match self {
            RteRwError::RteIndexOutOfRange =>
                "现象：IOREGSEL 收到的寄存器号不在 RTE 地址空间（0x10–0x3F）内。位置：间接寻址协议 select() 入口。建议修法：调用方先用 rte_index(rte, half) 由 RTE 序号合成寄存器号，不得手写裸号；非 RTE 寄存器（IOAPICID/VER/ARB）走 F0861 探测面专用通道。",
            RteRwError::AccessWithoutSelect =>
                "现象：IOWIN 数据口在 IOREGSEL 未选通任何寄存器号时被直接读写。位置：间接寻址协议 IOWIN 访问入口。建议修法：任何 IOWIN 访问前必须先 select() 选通并持票；此拦截是并发访序注入的第一道防线，禁止绕过。",
            RteRwError::CrossRteSwitchInWindow =>
                "现象：某 RTE 的拆写窗口尚未闭合（低字已写新纪元、高字仍持旧纪元），选通被切换到另一 RTE。位置：引擎守门 select_for_write()。建议修法：先闭合当前窗口（写完高字）再切换选通；窗口期跨 RTE 切换会让前一 RTE 永久停留在半新半旧态。",
            RteRwError::WriteOrderInverted =>
                "现象：高字写入先于低字写入（写序颠倒）。位置：写原语 write_high() 访序纪律检查。建议修法：严格执行低字→高字写序；本模型以低字纪元先推进为准，颠倒即拒。",
            RteRwError::WindowLeftUnclosed =>
                "现象：前一拆写窗口未闭合又请求开新窗口。位置：窗口账 open() 入口。建议修法：每次写 RTE 必须走完整的低字→高字两步闭合流程；半开窗口会让后续并发读者长期看到半新半旧态。",
            RteRwError::DuplicateHalfWrite =>
                "现象：同一 RTE 的同一半字在同一窗口内被写第二次。位置：写原语半字派发。建议修法：每窗口每半字只写一次；重复写说明上层重试逻辑越过了原语层，应整窗口回滚重做。",
            RteRwError::ReadbackMismatch =>
                "现象：写入读回逐字段比对发现不一致。位置：一致性账 compare()，diff 定位账给出 RTE/字段/期望/实际。建议修法：按 diff 定位账逐字段排查；先确认读回路径走的是同一纪元重组，再查上层是否在窗口期内并发改写。",
            RteRwError::ReassemblyMismatch =>
                "现象：读方重组校验失败——高低字纪元不一致且不处于窗口语义覆盖内。位置：纪律化读原语 disciplined_read()。建议修法：持串行化票重读；若连续失败说明有旁路写入者绕过了原语层。",
            RteRwError::SerializeOverBudget =>
                "现象：串行化持有时长超过 HOLD_BUDGET。位置：串行化账 release()。建议修法：把长事务拆成多次短原语调用；持票期间禁止做无界操作（如全表扫掠）。",
            RteRwError::FieldInvalid =>
                "现象：字段值超出位宽或落入保留区间（典型：向量 < 0x20 或 == 0xFF）。位置：RteWord::vector_valid() / 字段编码器。建议修法：向量取 0x20–0xFE；其余字段按位宽掩码截断前先显性校验。",
            RteRwError::FingerprintMismatch =>
                "现象：FNV-1a 指纹对拍不一致。位置：双跑对拍 observe()。建议修法：先查输入种子是否一致，再查运行路径是否混入了非确定性源（真实时钟/真实随机/地址哈希）——本单全部状态必须确定性可回放。",
            RteRwError::LedgerFull =>
                "现象：事件账/窗口账/diff 账容量耗尽，新条目被丢弃（丢弃数已显性自增）。位置：各 append-only 账 push()。建议修法：调大对应 *_CAP 常量或加快消费侧导出频率；静默丢弃在本单是禁用的。",
            RteRwError::ConsumeBeforeClosed =>
                "现象：窗口期内读到的半新半旧态被直接消费。位置：consume_word() 消费纪律。建议修法：半新半旧是窗口语义不是 bug，但消费前必须等窗口闭合（纪元一致）或持串行化票重组。",
            RteRwError::ProtoStateInvalid =>
                "现象：协议状态机收到当前态不允许的迁移（典型：已选通时再次 select）。位置：IndirectAddrProtocol 状态机。建议修法：选通→访问→取消选通三步走完再发起下一轮；中途换目标必须先取消选通。",
            RteRwError::TickRegression =>
                "现象：逻辑刻倒流（新 tick 小于已记录 tick）。位置：引擎 advance()/全部带 tick 入口。建议修法：调用方保证 tick 单调；本单零墙钟，tick 全部由编排侧注入。",
            RteRwError::WindowOpenOverBudget =>
                "现象：拆写窗口滞留超过 WINDOW_OPEN_BUDGET 仍不闭合。位置：窗口账超时检查。建议修法：高低两写之间不得插入长事务；把并发读观测挪到窗口闭合之后或压缩在预算内。",
        }
    }
    /// 全变体表（账面遍历用）。
    pub fn all() -> [RteRwError; 16] {
        [
            RteRwError::RteIndexOutOfRange,
            RteRwError::AccessWithoutSelect,
            RteRwError::CrossRteSwitchInWindow,
            RteRwError::WriteOrderInverted,
            RteRwError::WindowLeftUnclosed,
            RteRwError::DuplicateHalfWrite,
            RteRwError::ReadbackMismatch,
            RteRwError::ReassemblyMismatch,
            RteRwError::SerializeOverBudget,
            RteRwError::FieldInvalid,
            RteRwError::FingerprintMismatch,
            RteRwError::LedgerFull,
            RteRwError::ConsumeBeforeClosed,
            RteRwError::ProtoStateInvalid,
            RteRwError::TickRegression,
            RteRwError::WindowOpenOverBudget,
        ]
    }
}

// ===========================================================================
// 2. RTE 字段模型：9 字段位定义（SDM Vol.3 §10.11 逐位对照）
// ===========================================================================
/// RTE 位字段。位位置/宽度为 SDM Vol.3 §10.11 重定向表项定义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RteField {
    /// bits 0:7 中断向量。
    Vector,
    /// bits 8:10 投递模式。
    DeliveryMode,
    /// bit 11 目的模式（0 物理 / 1 逻辑）。
    DestMode,
    /// bit 12 投递状态（硬件只读：0 空闲 / 1 投递中）。
    DeliveryStatus,
    /// bit 13 极性（0 高有效 / 1 低有效）。
    Polarity,
    /// bit 14 远程 IRR（硬件只读：EOI 广播回执）。
    RemoteIrr,
    /// bit 15 触发模式（0 边沿 / 1 电平）。
    Trigger,
    /// bit 16 掩码（0 允许投递 / 1 屏蔽）。
    Mask,
    /// bits 56:63 目的地（物理模式 APIC ID / 逻辑模式集合）。
    Destination,
}

impl RteField {
    /// 字段序号（稳定序，供账面索引）。
    pub const fn index(&self) -> usize {
        match self {
            RteField::Vector => 0,
            RteField::DeliveryMode => 1,
            RteField::DestMode => 2,
            RteField::DeliveryStatus => 3,
            RteField::Polarity => 4,
            RteField::RemoteIrr => 5,
            RteField::Trigger => 6,
            RteField::Mask => 7,
            RteField::Destination => 8,
        }
    }
    /// 字段位偏移（const fn，SDM §10.11）。
    pub const fn shift(&self) -> u32 {
        match self {
            RteField::Vector => 0,
            RteField::DeliveryMode => 8,
            RteField::DestMode => 11,
            RteField::DeliveryStatus => 12,
            RteField::Polarity => 13,
            RteField::RemoteIrr => 14,
            RteField::Trigger => 15,
            RteField::Mask => 16,
            RteField::Destination => 56,
        }
    }
    /// 字段位宽（const fn，SDM §10.11）。
    pub const fn width(&self) -> u32 {
        match self {
            RteField::Vector => 8,
            RteField::DeliveryMode => 3,
            RteField::DestMode => 1,
            RteField::DeliveryStatus => 1,
            RteField::Polarity => 1,
            RteField::RemoteIrr => 1,
            RteField::Trigger => 1,
            RteField::Mask => 1,
            RteField::Destination => 8,
        }
    }
    /// 字段掩码（const fn：宽度内全 1 再左移到位）。
    pub const fn mask64(&self) -> u64 {
        let w = self.width();
        let ones: u64 = if w >= 64 { u64::MAX } else { (1u64 << w) - 1 };
        ones << self.shift()
    }
    /// 稳定名。
    pub fn name(&self) -> &'static str {
        match self {
            RteField::Vector => "vector",
            RteField::DeliveryMode => "delivery_mode",
            RteField::DestMode => "dest_mode",
            RteField::DeliveryStatus => "delivery_status",
            RteField::Polarity => "polarity",
            RteField::RemoteIrr => "remote_irr",
            RteField::Trigger => "trigger",
            RteField::Mask => "mask",
            RteField::Destination => "destination",
        }
    }
    /// 是否可编程（投递状态与远程 IRR 为硬件只读状态位——SDM §10.11：
    /// 软件写无效，读反映投递流水状态；一致性账只对可编程字段做写入读回比对）。
    pub const fn is_programmable(&self) -> bool {
        match self {
            RteField::DeliveryStatus | RteField::RemoteIrr => false,
            _ => true,
        }
    }
    /// 字段值是否落在合法区间（位宽校验 + 向量保留区间校验）。
    pub const fn value_valid(&self, v: u64) -> bool {
        let w = self.width();
        let ones: u64 = if w >= 64 { u64::MAX } else { (1u64 << w) - 1 };
        if (v & !ones) != 0 {
            return false;
        }
        // 向量额外走保留区间校验（0x00–0x1F 异常保留、0xFF 广播禁用）。
        let is_vector = match self {
            RteField::Vector => 1,
            _ => 0,
        };
        if is_vector == 1 && (v < VECTOR_LO || v > VECTOR_HI) {
            return false;
        }
        true
    }
    /// 全字段表（稳定序）。
    pub fn all() -> &'static [RteField; RTE_FIELD_COUNT] { &ALL_FIELDS }
}

/// 全字段静态表。
pub const ALL_FIELDS: [RteField; RTE_FIELD_COUNT] = [
    RteField::Vector,
    RteField::DeliveryMode,
    RteField::DestMode,
    RteField::DeliveryStatus,
    RteField::Polarity,
    RteField::RemoteIrr,
    RteField::Trigger,
    RteField::Mask,
    RteField::Destination,
];

/// 合法投递模式集合（SDM Vol.3 表 10-3：000 Fixed / 001 LowestPrio /
/// 010 SMI / 100 NMI / 101 INIT / 111 ExtINT；011/110 保留）。
pub const DELIVERY_MODES: [u64; 6] = [0b000, 0b001, 0b010, 0b100, 0b101, 0b111];

/// 投递模式名表（与 DELIVERY_MODES 一一对应）。
pub const DELIVERY_MODE_NAMES: [&str; 6] = ["Fixed", "LowestPrio", "SMI", "NMI", "INIT", "ExtINT"];

/// 通用字段抽取（const fn：右移 + 掩码，while 纪律无关，纯位运算）。
pub const fn extract_field(rte: u64, shift: u32, width: u32) -> u64 {
    let ones: u64 = if width >= 64 { u64::MAX } else { (1u64 << width) - 1 };
    (rte >> shift) & ones
}

/// 由 RTE 序号与半字合成 IOREGSEL 寄存器号（const fn）。
/// 低字号 = 0x10 + 2n，高字号 = 0x11 + 2n（SDM §10.11）。
pub const fn rte_index(rte: usize, half: u32) -> u32 {
    RTE_BASE_INDEX + (rte as u32) * 2 + half
}

// ===========================================================================
// 3. RteWord：64 位 RTE 字（高低双 32 位字）
// ===========================================================================
/// 64 位 RTE 字。IOWIN 是 32 位口，一个 RTE 永远以两个 32 位半字的面目
/// 出现在总线上——高低双字结构是拆写窗口账的根源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RteWord {
    /// 原始 64 位值。
    pub raw: u64,
}

impl RteWord {
    /// 由高低双 32 位半字组装。
    pub const fn from_parts(lo: u32, hi: u32) -> Self {
        RteWord { raw: ((hi as u64) << 32) | (lo as u64) }
    }
    /// 低 32 位半字。
    pub const fn lo(&self) -> u32 { self.raw as u32 }
    /// 高 32 位半字。
    pub const fn hi(&self) -> u32 { (self.raw >> 32) as u32 }
    /// 字段抽取（const fn）。
    pub const fn field(&self, f: &RteField) -> u64 {
        extract_field(self.raw, f.shift(), f.width())
    }
    /// 按字段序号抽取（const fn，供循环内使用）。
    pub const fn field_at(&self, idx: usize) -> u64 {
        let f = &ALL_FIELDS[idx];
        extract_field(self.raw, f.shift(), f.width())
    }
    /// 向量合法性（0x20–0xFE）。
    pub const fn vector_valid(&self) -> bool {
        let v = extract_field(self.raw, RteField::Vector.shift(), 8);
        v >= VECTOR_LO && v <= VECTOR_HI
    }
    /// 掩码位（1 = 屏蔽投递）。
    pub const fn masked(&self) -> bool {
        extract_field(self.raw, RteField::Mask.shift(), 1) == 1
    }
    /// 零字（全 0：向量 0 非法，天然不可投递——空槽位初始化态）。
    pub const fn zero() -> Self { RteWord { raw: 0 } }
}

/// 按字段枚举写入值到 raw（编码器核心：先清旧位再或新位）。
pub fn encode_field(raw: u64, f: &RteField, value: u64) -> Result<u64, RteRwError> {
    if !f.value_valid(value) {
        return Err(RteRwError::FieldInvalid);
    }
    Ok((raw & !f.mask64()) | ((value << f.shift()) & f.mask64()))
}

// ===========================================================================
// 4. RteHw：IO-APIC 寄存器文件模型（每 RTE 四通道：低字值/高字值/低字纪元/高字纪元）
// ===========================================================================
/// 寄存器文件。纪元戳（epoch）是拆写窗口账的物理基础：每写一个半字，
/// 该半字获得一个新纪元号；高低字纪元一致 ⟺ 该 RTE 处于组装一致态。
pub struct RteHw {
    /// 低 32 位半字值（每 RTE 一格）。
    lo: [u32; RTE_COUNT],
    /// 高 32 位半字值（每 RTE 一格）。
    hi: [u32; RTE_COUNT],
    /// 低字纪元戳（0 = 从未写过）。
    lo_epoch: [u32; RTE_COUNT],
    /// 高字纪元戳（0 = 从未写过）。
    hi_epoch: [u32; RTE_COUNT],
    /// 纪元时钟（单调，wrapping 回绕到 0 时跳过——0 保留为「从未写过」）。
    epoch_clock: u32,
}

impl RteHw {
    /// 空文件：全 RTE 置零字、纪元 0。
    pub fn new() -> Self {
        RteHw {
            lo: [0u32; RTE_COUNT],
            hi: [0u32; RTE_COUNT],
            lo_epoch: [0u32; RTE_COUNT],
            hi_epoch: [0u32; RTE_COUNT],
            epoch_clock: 0,
        }
    }
    /// 分配下一个纪元号（显性回绕处理：0 保留）。
    pub fn next_epoch(&mut self) -> u32 {
        self.epoch_clock = self.epoch_clock.wrapping_add(1);
        if self.epoch_clock == 0 {
            self.epoch_clock = 1;
        }
        self.epoch_clock
    }
    /// 写半字：返回落账纪元号。越界 rte 一律 saturating 落在末格之外不可达——
    /// 入口已由协议层校验，此处按 wrapping 防御（索引由调用方保证 < RTE_COUNT）。
    pub fn write_half(&mut self, rte: usize, half: u32, value: u32, epoch: u32) -> u32 {
        let i = rte % RTE_COUNT;
        if half == HALF_LO {
            self.lo[i] = value;
            self.lo_epoch[i] = epoch;
        } else {
            self.hi[i] = value;
            self.hi_epoch[i] = epoch;
        }
        epoch
    }
    /// 观测读半字：返回（值，纪元）。观测不动纪元。
    pub fn read_half_obs(&self, rte: usize, half: u32) -> (u32, u32) {
        let i = rte % RTE_COUNT;
        if half == HALF_LO { (self.lo[i], self.lo_epoch[i]) } else { (self.hi[i], self.hi_epoch[i]) }
    }
    /// 高低字纪元快照。
    pub fn half_epochs(&self, rte: usize) -> (u32, u32) {
        let i = rte % RTE_COUNT;
        (self.lo_epoch[i], self.hi_epoch[i])
    }
    /// 组装一致态：高低字纪元一致（且非从未写过态）。
    pub fn assembled(&self, rte: usize) -> bool {
        let (a, b) = self.half_epochs(rte);
        a != 0 && a == b
    }
    /// 组装读：纪元一致时返回整字，否则 None（窗口期内）。
    pub fn read_assembled(&self, rte: usize) -> Option<RteWord> {
        if self.assembled(rte) {
            let i = rte % RTE_COUNT;
            Some(RteWord::from_parts(self.lo[i], self.hi[i]))
        } else {
            None
        }
    }
    /// 纪元缺口计数（health 用：组装一致态的 RTE 数）。
    pub fn assembled_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < RTE_COUNT {
            if self.assembled(i) { n += 1; }
            i += 1;
        }
        n
    }
    /// 纪元时钟当前值。
    pub fn epoch_now(&self) -> u32 { self.epoch_clock }
}

// ===========================================================================
// 5. IndirectAddrProtocol：间接寻址协议状态机（选通→访问→取消选通）
// ===========================================================================
/// 协议状态。真实 IO-APIC 只有一个 IOREGSEL + 一个 IOWIN：选通是全局单例态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtoState {
    /// 空闲：未选通任何寄存器号（IOWIN 访问被拦截）。
    Idle,
    /// 已选通：IOREGSEL 持有某个寄存器号（低字号或高字号）。
    Selected,
}

/// 间接寻址协议状态机。纪律：
/// 1. IOWIN 访问必须处于 Selected 态，否则 `AccessWithoutSelect`；
/// 2. 寄存器号必须落在 RTE 地址空间（0x10–0x10+2*24-1），越界/非 RTE 号拒收；
/// 3. 已选通时再次 select 拒收（须先取消选通——选通→访问→取消选通三步走完）。
pub struct IndirectAddrProtocol {
    /// 当前态。
    state: ProtoState,
    /// IOREGSEL 当前持有的寄存器号（Selected 态下有效）。
    regsel: u32,
    /// IOWIN 数据寄存器（真实硬件上是 32 位数据口锁存器）。
    win: u32,
    /// 选通次数。
    select_count: u64,
    /// 取消选通次数。
    deselect_count: u64,
    /// IOWIN 读次数。
    read_count: u64,
    /// IOWIN 写次数。
    write_count: u64,
    /// 未选通访问拦截次数。
    no_select_rejects: u64,
    /// 越界寄存器号拒收次数。
    oor_rejects: u64,
    /// 重入选通拒收次数。
    reselect_rejects: u64,
}

impl IndirectAddrProtocol {
    /// 新协议机：Idle、全零账。
    pub fn new() -> Self {
        IndirectAddrProtocol {
            state: ProtoState::Idle,
            regsel: 0,
            win: 0,
            select_count: 0,
            deselect_count: 0,
            read_count: 0,
            write_count: 0,
            no_select_rejects: 0,
            oor_rejects: 0,
            reselect_rejects: 0,
        }
    }
    /// 当前态。
    pub fn state(&self) -> ProtoState { self.state }
    /// 是否空闲（health 用：静止时必须回到 Idle）。
    pub fn is_idle(&self) -> bool { matches!(self.state, ProtoState::Idle) }
    /// 当前选通号（Idle 态返回 None）。
    pub fn selected_index(&self) -> Option<u32> {
        match self.state {
            ProtoState::Selected => Some(self.regsel),
            ProtoState::Idle => None,
        }
    }
    /// 寄存器号合法性：必须落在 RTE 地址空间 [0x10, 0x10+2*24)。
    /// 非 RTE 寄存器区间（0x00–0x0F）与越界号一律拒收。
    pub const fn index_in_range(index: u32) -> bool {
        index >= RTE_BASE_INDEX && index < RTE_BASE_INDEX + (RTE_COUNT as u32) * 2
    }
    /// 选通：写 IOREGSEL。成功后进入 Selected 态。
    pub fn select(&mut self, index: u32) -> Result<(), RteRwError> {
        if !Self::index_in_range(index) {
            self.oor_rejects = self.oor_rejects.wrapping_add(1);
            return Err(RteRwError::RteIndexOutOfRange);
        }
        if matches!(self.state, ProtoState::Selected) {
            self.reselect_rejects = self.reselect_rejects.wrapping_add(1);
            return Err(RteRwError::ProtoStateInvalid);
        }
        self.state = ProtoState::Selected;
        self.regsel = index;
        self.select_count = self.select_count.wrapping_add(1);
        Ok(())
    }
    /// 取消选通：回到 Idle（选通→访问→取消选通三步的收尾步）。
    pub fn deselect(&mut self) -> Result<(), RteRwError> {
        match self.state {
            ProtoState::Idle => {
                // 空闲态重复取消是空操作纪律违例——显性拒收。
                self.reselect_rejects = self.reselect_rejects.wrapping_add(1);
                Err(RteRwError::ProtoStateInvalid)
            }
            ProtoState::Selected => {
                self.state = ProtoState::Idle;
                self.deselect_count = self.deselect_count.wrapping_add(1);
                Ok(())
            }
        }
    }
    /// IOWIN 写（数据口出向）：未选通拦截。写入落进 IOWIN 数据寄存器，
    /// 由引擎取走并落到寄存器文件（模拟总线时序：写 IOWIN 即生效于选通寄存器）。
    pub fn iowin_write(&mut self, value: u32) -> Result<(), RteRwError> {
        match self.state {
            ProtoState::Idle => {
                self.no_select_rejects = self.no_select_rejects.wrapping_add(1);
                Err(RteRwError::AccessWithoutSelect)
            }
            ProtoState::Selected => {
                self.win = value;
                self.write_count = self.write_count.wrapping_add(1);
                Ok(())
            }
        }
    }
    /// IOWIN 装载（数据口入向）：引擎把寄存器文件半字装进数据寄存器，
    /// 未选通拦截。
    pub fn iowin_load(&mut self, value: u32) -> Result<(), RteRwError> {
        match self.state {
            ProtoState::Idle => {
                self.no_select_rejects = self.no_select_rejects.wrapping_add(1);
                Err(RteRwError::AccessWithoutSelect)
            }
            ProtoState::Selected => {
                self.win = value;
                Ok(())
            }
        }
    }
    /// IOWIN 读（数据口出向读回）：未选通拦截。
    pub fn iowin_read(&mut self) -> Result<u32, RteRwError> {
        match self.state {
            ProtoState::Idle => {
                self.no_select_rejects = self.no_select_rejects.wrapping_add(1);
                Err(RteRwError::AccessWithoutSelect)
            }
            ProtoState::Selected => {
                self.read_count = self.read_count.wrapping_add(1);
                Ok(self.win)
            }
        }
    }
    /// 取走 IOWIN 数据寄存器内容（Selected 态下引擎落账用）。
    pub fn take_win(&mut self) -> Result<u32, RteRwError> {
        match self.state {
            ProtoState::Idle => Err(RteRwError::AccessWithoutSelect),
            ProtoState::Selected => Ok(self.win),
        }
    }
    /// 选通号解析为（rte, half）。越界号返回 None。
    pub fn decode_index(&self) -> Option<(usize, u32)> {
        match self.state {
            ProtoState::Idle => None,
            ProtoState::Selected => {
                if !Self::index_in_range(self.regsel) {
                    None
                } else {
                    let off = self.regsel - RTE_BASE_INDEX;
                    Some(((off / 2) as usize, off % 2))
                }
            }
        }
    }
    /// 拦截账快照（无选通/越界/重入三拒绝计数）。
    pub fn reject_counts(&self) -> (u64, u64, u64) {
        (self.no_select_rejects, self.oor_rejects, self.reselect_rejects)
    }
    /// 访问账（选通/取消/读/写四计数）。
    pub fn access_counts(&self) -> (u64, u64, u64, u64) {
        (self.select_count, self.deselect_count, self.read_count, self.write_count)
    }
}

// ===========================================================================
// 6. RwSerializer：原语级互斥串行化（等待者/持有者/持有时长账）
// ===========================================================================
/// 串行化获取结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcquireOutcome {
    /// 直接授予（无竞争）。
    Granted,
    /// 被串行化：成为等待者，`predicted_wait` 为建模预测等待时长（刻）。
    Serialized { predicted_wait: u64 },
}

/// 原语级互斥账。判据「并发访序注入 10/10 次被串行化保护」的直接承载者：
/// 第二个竞争者必须被串行化（进等待者账），绝不允许穿透。
pub struct RwSerializer {
    /// 当前持有者（None = 无主）。
    holder: Option<u64>,
    /// 本轮持有起始刻。
    hold_since: u64,
    /// 当前等待者数。
    waiters: u64,
    /// 授予次数。
    granted_count: u64,
    /// 串行化次数（被拦成等待者的竞争访问数）。
    serialized_count: u64,
    /// 释放次数。
    release_count: u64,
    /// 历史最大等待者数。
    max_waiters: u64,
    /// 累计持有时长（刻，saturating）。
    total_hold_ticks: u64,
    /// 单轮最长持有时长（刻）。
    max_hold_ticks: u64,
    /// 超预算持有时长次数。
    over_budget_count: u64,
}

impl RwSerializer {
    /// 空账。
    pub fn new() -> Self {
        RwSerializer {
            holder: None,
            hold_since: 0,
            waiters: 0,
            granted_count: 0,
            serialized_count: 0,
            release_count: 0,
            max_waiters: 0,
            total_hold_ticks: 0,
            max_hold_ticks: 0,
            over_budget_count: 0,
        }
    }
    /// 获取：无主即授予；有主且非本人即串行化（等待者账自增）。
    /// 持有者本人重入按幂等授予处理（原语层嵌套调用的防御语义）。
    pub fn acquire(&mut self, owner: u64, tick: u64) -> AcquireOutcome {
        match self.holder {
            None => {
                self.holder = Some(owner);
                self.hold_since = tick;
                self.granted_count = self.granted_count.wrapping_add(1);
                AcquireOutcome::Granted
            }
            Some(h) => {
                if h == owner {
                    AcquireOutcome::Granted
                } else {
                    self.serialized_count = self.serialized_count.wrapping_add(1);
                    self.waiters = self.waiters.wrapping_add(1);
                    if self.waiters > self.max_waiters {
                        self.max_waiters = self.waiters;
                    }
                    AcquireOutcome::Serialized { predicted_wait: tick.saturating_sub(self.hold_since) }
                }
            }
        }
    }
    /// 串行化解除：把票移交给指定等待者（等待者账回落；移交即一次授予，
    /// 与 release 对账——保证授予数 == 释放数的收支平衡不变量）。
    pub fn grant_waiter(&mut self, owner: u64, tick: u64) {
        if self.waiters > 0 {
            self.waiters = self.waiters.saturating_sub(1);
        }
        self.holder = Some(owner);
        self.hold_since = tick;
        self.granted_count = self.granted_count.wrapping_add(1);
    }
    /// 释放：持有时长落账（saturating；超预算显性计数）。
    pub fn release(&mut self, tick: u64) {
        if self.holder.is_some() {
            let held = tick.saturating_sub(self.hold_since);
            self.total_hold_ticks = self.total_hold_ticks.saturating_add(held);
            if held > self.max_hold_ticks {
                self.max_hold_ticks = held;
            }
            if held > HOLD_BUDGET {
                self.over_budget_count = self.over_budget_count.wrapping_add(1);
            }
            self.holder = None;
            self.release_count = self.release_count.wrapping_add(1);
        }
    }
    /// 当前持有者。
    pub fn holder_now(&self) -> Option<u64> { self.holder }
    /// 当前等待者数。
    pub fn waiters_now(&self) -> u64 { self.waiters }
    /// 串行化总次数。
    pub fn serialized_total(&self) -> u64 { self.serialized_count }
    /// 授予/释放/串行化三计数快照。
    pub fn counts(&self) -> (u64, u64, u64) {
        (self.granted_count, self.release_count, self.serialized_count)
    }
    /// 持有时长账快照（累计/单轮最长/超预算次数）。
    pub fn hold_stats(&self) -> (u64, u64, u64) {
        (self.total_hold_ticks, self.max_hold_ticks, self.over_budget_count)
    }
    /// 静止判定：无持有者（health 用）。
    pub fn at_rest(&self) -> bool { self.holder.is_none() }
    /// 收支平衡判定（health 用：授予数 == 释放数 ⟺ 无泄漏票）。
    pub fn balanced(&self) -> bool { self.granted_count == self.release_count }
}

// ===========================================================================
// 7. RteSplitWindow：64 位拆写窗口账（半新半旧态显性标记 + 读方重组校验）
// ===========================================================================
/// 半字新鲜度标记（窗口语义的核心显性化：半新半旧不是 bug，是窗口语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HalfFreshness {
    /// 新纪元（本窗口刚写入的半字）。
    Fresh,
    /// 旧纪元（尚未被本窗口覆盖的半字）。
    Stale,
}

/// 开窗态（窗口期内活跃条目）。
pub struct OpenWindow {
    /// 目标 RTE。
    pub rte: u8,
    /// 开窗时戳（进入窗口的刻）。
    pub open_tick: u64,
    /// 低字落账纪元。
    pub lo_epoch: u32,
    /// 窗口期内并发读到的半新半旧观测数。
    pub mixed_reads: u32,
}

/// 已闭合窗口账条目。
#[derive(Clone, Copy, Debug)]
pub struct WindowEntry {
    /// 目标 RTE。
    pub rte: u8,
    /// 开窗时戳。
    pub open_tick: u64,
    /// 闭窗时戳（退出窗口的刻）。
    pub close_tick: u64,
    /// 低字落账纪元。
    pub lo_epoch: u32,
    /// 高字闭合前旧纪元（写高字前高字持的纪元——半新半旧的「旧」）。
    pub hi_epoch_before: u32,
    /// 窗口期内并发读观测数。
    pub mixed_reads: u32,
    /// 读方重组校验：闭合后高低字纪元一致。
    pub reassembly_ok: bool,
    /// 窗口滞留时长（刻）= close_tick - open_tick。
    pub span_ticks: u64,
    /// 滞留是否超预算（显性标记，不拦截只记）。
    pub span_over_budget: bool,
}

/// 窗口观测（并发读方拿到的快照）。
#[derive(Clone, Copy, Debug)]
pub struct WindowObservation {
    /// 目标 RTE（无窗口时为当前观测 RTE）。
    pub rte: u8,
    /// 低字值。
    pub lo: u32,
    /// 高字值。
    pub hi: u32,
    /// 低字纪元。
    pub lo_epoch: u32,
    /// 高字纪元。
    pub hi_epoch: u32,
    /// 半新半旧标记：true ⟺ 窗口期内读到的高旧低新态。
    pub mixed: bool,
    /// 低字新鲜度。
    pub lo_fresh: HalfFreshness,
    /// 高字新鲜度。
    pub hi_fresh: HalfFreshness,
}

/// 拆写窗口账。RTE 是 64 位而 IOWIN 是 32 位口——高低两次写之间就是窗口期。
/// 账面三要素：进入/退出时戳、窗口内并发读观测（半新半旧显性标记）、
/// 读方重组校验（闭合后纪元一致）。
pub struct RteSplitWindow {
    /// 活跃开窗态（至多一个：选通是全局单例）。
    open: Option<OpenWindow>,
    /// 已闭合条目（append-only，溢出丢弃显性化）。
    entries: Vec<WindowEntry>,
    /// 丢弃数。
    dropped: u64,
    /// 窗口期内跨 RTE 切换拦截数。
    cross_switch_rejects: u64,
    /// 半新半旧消费拦截数。
    consume_blocks: u64,
}

impl RteSplitWindow {
    /// 空账。
    pub fn new() -> Self {
        RteSplitWindow {
            open: None,
            entries: Vec::new(),
            dropped: 0,
            cross_switch_rejects: 0,
            consume_blocks: 0,
        }
    }
    /// 开窗。已有未闭合窗口即拒（WindowLeftUnclosed）。
    pub fn open(&mut self, rte: u8, tick: u64, lo_epoch: u32) -> Result<(), RteRwError> {
        if self.open.is_some() {
            return Err(RteRwError::WindowLeftUnclosed);
        }
        self.open = Some(OpenWindow { rte, open_tick: tick, lo_epoch, mixed_reads: 0 });
        Ok(())
    }
    /// 记录窗口期内一次并发读观测（半新半旧显性计数）。
    pub fn record_mixed_read(&mut self) {
        if let Some(ow) = self.open.as_mut() {
            ow.mixed_reads = ow.mixed_reads.wrapping_add(1);
        }
    }
    /// 闭窗：时戳/旧纪元/重组校验落账。无开窗态即拒。
    pub fn close(&mut self, tick: u64, hi_epoch_before: u32) -> Result<bool, RteRwError> {
        let ow = match self.open.take() {
            Some(o) => o,
            None => return Err(RteRwError::WindowLeftUnclosed),
        };
        let span = tick.saturating_sub(ow.open_tick);
        let entry = WindowEntry {
            rte: ow.rte,
            open_tick: ow.open_tick,
            close_tick: tick,
            lo_epoch: ow.lo_epoch,
            hi_epoch_before,
            mixed_reads: ow.mixed_reads,
            // 读方重组校验：由引擎在闭窗后核对高低字纪元并回填。
            reassembly_ok: false,
            span_ticks: span,
            span_over_budget: span > WINDOW_OPEN_BUDGET,
        };
        if self.entries.len() >= WINDOW_LEDGER_CAP {
            self.dropped = self.dropped.wrapping_add(1);
        } else {
            self.entries.push(entry);
        }
        Ok(entry.span_over_budget)
    }
    /// 回填读方重组校验结果（引擎闭窗后核对纪元一致后调用）。
    pub fn mark_reassembly(&mut self, ok: bool) {
        if let Some(e) = self.entries.last_mut() {
            e.reassembly_ok = ok;
        }
    }
    /// 活跃窗口目标 RTE（None = 无窗口）。
    pub fn open_rte(&self) -> Option<u8> {
        self.open.as_ref().map(|o| o.rte)
    }
    /// 活跃窗口开窗时戳。
    pub fn open_tick(&self) -> Option<u64> {
        self.open.as_ref().map(|o| o.open_tick)
    }
    /// 活跃窗口是否存在。
    pub fn is_open(&self) -> bool { self.open.is_some() }
    /// 窗口期内跨 RTE 切换拦截计数。
    pub fn cross_switch_rejects(&self) -> u64 { self.cross_switch_rejects }
    /// 拦截自增。
    pub fn bump_cross_switch(&mut self) { self.cross_switch_rejects = self.cross_switch_rejects.wrapping_add(1); }
    /// 半新半旧消费拦截计数与自增。
    pub fn bump_consume_block(&mut self) { self.consume_blocks = self.consume_blocks.wrapping_add(1); }
    pub fn consume_blocks(&self) -> u64 { self.consume_blocks }
    /// 已闭合条目数。
    pub fn entry_count(&self) -> u64 { self.entries.len() as u64 }
    /// 丢弃数。
    pub fn dropped(&self) -> u64 { self.dropped }
    /// 条目快照（供导出；拷贝数组，避免外部持有可变引用）。
    pub fn entries(&self) -> Vec<WindowEntry> { self.entries.clone() }
    /// 账可见判定（判据三）：有闭合条目、有半新半旧观测、全部条目重组校验通过、
    /// 时戳齐全（close > open）。
    pub fn visible(&self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let mut any_mixed = false;
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.mixed_reads > 0 { any_mixed = true; }
            if !e.reassembly_ok { return false; }
            if e.close_tick <= e.open_tick { return false; }
            i += 1;
        }
        any_mixed
    }
    /// 滞留超预算条目数。
    pub fn over_budget_entries(&self) -> u64 {
        let mut n = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].span_over_budget { n += 1; }
            i += 1;
        }
        n
    }
    /// 半新半旧观测总数。
    pub fn mixed_total(&self) -> u64 {
        let mut n: u64 = 0;
        let mut i = 0;
        while i < self.entries.len() {
            n = n.wrapping_add(self.entries[i].mixed_reads as u64);
            i += 1;
        }
        n
    }
}

// ===========================================================================
// 8. ConsistencyLedger：写入读回一致性账（字段级 diff + 不一致定位）
// ===========================================================================
/// 字段级不一致定位条目。
#[derive(Clone, Copy, Debug)]
pub struct FieldDiff {
    /// RTE 序号。
    pub rte: u8,
    /// 字段序号（RteField::index()）。
    pub field_idx: usize,
    /// 期望值（写入侧编码值）。
    pub expected: u64,
    /// 实际值（读回侧解码值）。
    pub actual: u64,
}

/// 一致性账。判据一「原语读写一致性 100%（写入读回逐字段）」的直接承载者。
pub struct ConsistencyLedger {
    /// 字段比对总次数。
    fields_checked: u64,
    /// 不一致次数。
    mismatches: u64,
    /// 写入读回往返次数（一次 program = 一次往返）。
    roundtrips: u64,
    /// 全绿往返数。
    roundtrips_ok: u64,
    /// diff 定位账（append-only，溢出丢弃显性化）。
    diffs: Vec<FieldDiff>,
    /// diff 丢弃数。
    diffs_dropped: u64,
}

impl ConsistencyLedger {
    /// 空账。
    pub fn new() -> Self {
        ConsistencyLedger {
            fields_checked: 0,
            mismatches: 0,
            roundtrips: 0,
            roundtrips_ok: 0,
            diffs: Vec::new(),
            diffs_dropped: 0,
        }
    }
    /// 逐字段比对：写入侧字与读回侧字按 9 字段全量对照，可编程字段计账、
    /// 只读字段（投递状态/远程 IRR）不计——硬件只读位软件写无效（SDM §10.11）。
    /// 返回本次往返是否全绿。
    pub fn compare_roundtrip(&mut self, rte: u8, written: &RteWord, readback: &RteWord) -> bool {
        self.roundtrips = self.roundtrips.wrapping_add(1);
        let mut ok = true;
        let mut i = 0;
        while i < RTE_FIELD_COUNT {
            let f = &ALL_FIELDS[i];
            if f.is_programmable() {
                self.fields_checked = self.fields_checked.wrapping_add(1);
                let expect = written.field(f);
                let actual = readback.field(f);
                if expect != actual {
                    ok = false;
                    self.mismatches = self.mismatches.wrapping_add(1);
                    if self.diffs.len() >= DIFF_LEDGER_CAP {
                        self.diffs_dropped = self.diffs_dropped.wrapping_add(1);
                    } else {
                        self.diffs.push(FieldDiff { rte, field_idx: i, expected: expect, actual });
                    }
                }
            }
            i += 1;
        }
        if ok {
            self.roundtrips_ok = self.roundtrips_ok.wrapping_add(1);
        }
        ok
    }
    /// 一致性百分比（saturating 至 100 上界；零比对时按 100 处理——空扫掠不扣分）。
    pub fn pct(&self) -> u64 {
        if self.fields_checked == 0 {
            return CONSISTENCY_TARGET_PCT;
        }
        let good = self.fields_checked.saturating_sub(self.mismatches);
        let v = good.saturating_mul(100) / self.fields_checked;
        if v > CONSISTENCY_TARGET_PCT { CONSISTENCY_TARGET_PCT } else { v }
    }
    /// 是否达标（判据一）。
    pub fn at_target(&self) -> bool { self.pct() >= CONSISTENCY_TARGET_PCT && self.mismatches == 0 }
    /// 比对/不一致计数快照。
    pub fn counts(&self) -> (u64, u64) { (self.fields_checked, self.mismatches) }
    /// 往返计数快照。
    pub fn roundtrips(&self) -> (u64, u64) { (self.roundtrips, self.roundtrips_ok) }
    /// diff 定位账快照。
    pub fn diffs(&self) -> Vec<FieldDiff> { self.diffs.clone() }
    /// diff 丢弃数。
    pub fn diffs_dropped(&self) -> u64 { self.diffs_dropped }
    /// diff 条目数。
    pub fn diff_count(&self) -> u64 { self.diffs.len() as u64 }
}

// ===========================================================================
// 9. EventLedger：append-only 环形事件账
// ===========================================================================
/// 事件种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// 选通。
    Select,
    /// 取消选通。
    Deselect,
    /// 拒收（协议/纪律拦截）。
    Reject,
    /// 开窗。
    WindowOpen,
    /// 闭窗。
    WindowClose,
    /// 窗口内半新半旧观测。
    MixedRead,
    /// 半新半旧消费拦截。
    ConsumeBlocked,
    /// 串行化（成为等待者）。
    SerializeWait,
    /// 串行化解除移交。
    SerializeGrant,
    /// 一致性往返通过。
    ConsistencyPass,
    /// 一致性往返失败。
    ConsistencyFail,
    /// 注入检出。
    InjectDetected,
    /// 访序检查通过。
    OrderOk,
    /// 指纹落账。
    Fingerprint,
}

impl EventKind {
    /// 稳定序号（导出面用）。
    pub const fn index(&self) -> u8 {
        match self {
            EventKind::Select => 0,
            EventKind::Deselect => 1,
            EventKind::Reject => 2,
            EventKind::WindowOpen => 3,
            EventKind::WindowClose => 4,
            EventKind::MixedRead => 5,
            EventKind::ConsumeBlocked => 6,
            EventKind::SerializeWait => 7,
            EventKind::SerializeGrant => 8,
            EventKind::ConsistencyPass => 9,
            EventKind::ConsistencyFail => 10,
            EventKind::InjectDetected => 11,
            EventKind::OrderOk => 12,
            EventKind::Fingerprint => 13,
        }
    }
    /// 稳定名。
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::Select => "select",
            EventKind::Deselect => "deselect",
            EventKind::Reject => "reject",
            EventKind::WindowOpen => "window_open",
            EventKind::WindowClose => "window_close",
            EventKind::MixedRead => "mixed_read",
            EventKind::ConsumeBlocked => "consume_blocked",
            EventKind::SerializeWait => "serialize_wait",
            EventKind::SerializeGrant => "serialize_grant",
            EventKind::ConsistencyPass => "consistency_pass",
            EventKind::ConsistencyFail => "consistency_fail",
            EventKind::InjectDetected => "inject_detected",
            EventKind::OrderOk => "order_ok",
            EventKind::Fingerprint => "fingerprint",
        }
    }
}

/// 事件条目。
#[derive(Clone, Copy, Debug)]
pub struct RteEvent {
    /// 时戳（刻）。
    pub tick: u64,
    /// 种类。
    pub kind: EventKind,
    /// RTE 序号（RTE_NONE = 不指向具体 RTE）。
    pub rte: u8,
    /// 关联诊断码（0 = 无）。
    pub code: u16,
}

/// 事件账（append-only 环形，容量 EVENT_RING_CAP，溢出丢新显性自增）。
pub struct EventLedger {
    buf: Vec<RteEvent>,
    dropped: u64,
    by_kind: [u64; 14],
}

impl EventLedger {
    /// 空账。
    pub fn new() -> Self {
        EventLedger { buf: Vec::new(), dropped: 0, by_kind: [0u64; 14] }
    }
    /// 入账（满则丢新并显性计数）。
    pub fn push(&mut self, tick: u64, kind: EventKind, rte: u8, code: u16) {
        let idx = kind.index() as usize;
        self.by_kind[idx] = self.by_kind[idx].wrapping_add(1);
        if self.buf.len() >= EVENT_RING_CAP {
            self.dropped = self.dropped.wrapping_add(1);
            return;
        }
        self.buf.push(RteEvent { tick, kind, rte, code });
    }
    /// 在账条目数。
    pub fn len(&self) -> u64 { self.buf.len() as u64 }
    /// 是否为空。
    pub fn is_empty(&self) -> bool { self.buf.is_empty() }
    /// 丢弃数。
    pub fn dropped(&self) -> u64 { self.dropped }
    /// 按种类计数。
    pub fn count_of(&self, kind: &EventKind) -> u64 { self.by_kind[kind.index() as usize] }
    /// 快照导出。
    pub fn snapshot(&self) -> Vec<RteEvent> { self.buf.clone() }
}

// ===========================================================================
// 10. RteRwEngine：编排器（访序纪律 + 注入面 + 指纹 + 健康）
// ===========================================================================
/// 引擎。把协议/寄存器文件/窗口账/串行化/一致性/事件账编成一个可驱动的整体；
/// 全部操作以 `self.tick` 单调推进，零外部输入。
pub struct RteRwEngine {
    /// 逻辑刻（单调）。
    tick: u64,
    /// 寄存器文件。
    hw: RteHw,
    /// 间接寻址协议机。
    proto: IndirectAddrProtocol,
    /// 拆写窗口账。
    window: RteSplitWindow,
    /// 串行化账。
    serial: RwSerializer,
    /// 一致性账。
    consist: ConsistencyLedger,
    /// 事件账。
    events: EventLedger,
    /// 窗口期挂起标记（低字已写、待高字闭合），每 RTE 一格。
    pending_low: [bool; RTE_COUNT],
    /// 挂起窗口的目标字（供闭合步落账高字）。
    pending_word: [u64; RTE_COUNT],
    /// 当前纪元（本窗口落账纪元）。
    cur_epoch: u32,
    /// 写序颠倒拦截数。
    order_violations: u64,
    /// 非法字段拦截数。
    field_rejects: u64,
    /// 半新半旧消费拦截数（引擎级透传）。
    consume_blocked: u64,
    /// LCG 状态（确定性驱动）。
    lcg: u64,
}

impl RteRwEngine {
    /// 新引擎：种子注入 LCG。
    pub fn new(seed: u64) -> Self {
        RteRwEngine {
            tick: 0,
            hw: RteHw::new(),
            proto: IndirectAddrProtocol::new(),
            window: RteSplitWindow::new(),
            serial: RwSerializer::new(),
            consist: ConsistencyLedger::new(),
            events: EventLedger::new(),
            pending_low: [false; RTE_COUNT],
            pending_word: [0u64; RTE_COUNT],
            cur_epoch: 0,
            order_violations: 0,
            field_rejects: 0,
            consume_blocked: 0,
            lcg: seed ^ 0x0862_0000_0000_0001,
        }
    }
    /// 逻辑刻推进（单调，显性 wrapping）。
    pub fn advance(&mut self, n: u64) {
        self.tick = self.tick.wrapping_add(n);
    }
    /// 当前刻。
    pub fn tick_now(&self) -> u64 { self.tick }
    /// LCG 步进（确定性）。
    fn lcg_next(&mut self) -> u64 {
        self.lcg = self.lcg.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD);
        self.lcg
    }
    /// 确定性生成一个合法 RTE 字（全部字段落合法域，可编程字段全由 LCG 驱动）。
    pub fn gen_word(&mut self) -> RteWord {
        let r = self.lcg_next();
        let vector = VECTOR_LO + (r % (VECTOR_HI - VECTOR_LO + 1));
        let dm = DELIVERY_MODES[((r >> 8) % DELIVERY_MODES.len() as u64) as usize];
        let dest_mode = (r >> 12) % 2;
        let polarity = (r >> 14) % 2;
        let trigger = (r >> 16) % 2;
        let mask = (r >> 18) % 2;
        let dest = (r >> 20) % 256;
        // 投递状态/远程 IRR 为硬件只读位，恒 0（空闲态）。
        let raw = vector
            | (dm << 8)
            | (dest_mode << 11)
            | (polarity << 13)
            | (trigger << 15)
            | (mask << 16)
            | (dest << 56);
        RteWord { raw }
    }

    // -----------------------------------------------------------------
    // 10.1 守门原语：选通必须过两道关——窗口期跨 RTE 切换关 + 协议态关
    // -----------------------------------------------------------------
    /// 守门选通：窗口期内切换到另一 RTE 即拒（CrossRteSwitchInWindow）；
    /// 协议层再做越界与重入校验。
    pub fn select_for_write(&mut self, rte: usize, half: u32) -> Result<(), RteRwError> {
        if rte >= RTE_COUNT {
            self.events.push(self.tick, EventKind::Reject, RTE_NONE, RteRwError::RteIndexOutOfRange.code());
            return Err(RteRwError::RteIndexOutOfRange);
        }
        // 关一：窗口期跨 RTE 切换拦截。
        if let Some(ow) = self.window.open_rte() {
            if ow as usize != rte {
                self.window.bump_cross_switch();
                self.events.push(self.tick, EventKind::Reject, ow, RteRwError::CrossRteSwitchInWindow.code());
                return Err(RteRwError::CrossRteSwitchInWindow);
            }
        }
        // 关二：协议态（越界号/重入选通）。
        self.proto.select(rte_index(rte, half))
    }

    // -----------------------------------------------------------------
    // 10.2 写原语：低字→高字两步纪律
    // -----------------------------------------------------------------
    /// 写低字半步：串行化票已在手、向量已校验后调用。
    /// 选通低字号 → IOWIN 写 → 落寄存器文件（新纪元）→ 开窗 → 取消选通。
    pub fn write_low(&mut self, rte: usize, word: &RteWord) -> Result<(), RteRwError> {
        if rte >= RTE_COUNT {
            return Err(RteRwError::RteIndexOutOfRange);
        }
        if self.window.is_open() {
            return Err(RteRwError::WindowLeftUnclosed);
        }
        if self.pending_low[rte] {
            return Err(RteRwError::DuplicateHalfWrite);
        }
        self.cur_epoch = self.hw.next_epoch();
        self.select_for_write(rte, HALF_LO)?;
        self.proto.iowin_write(word.lo())?;
        let v = self.proto.take_win()?;
        self.proto.deselect()?;
        self.hw.write_half(rte, HALF_LO, v, self.cur_epoch);
        self.window.open(rte as u8, self.tick, self.cur_epoch)?;
        self.pending_low[rte] = true;
        self.pending_word[rte] = word.raw;
        self.events.push(self.tick, EventKind::WindowOpen, rte as u8, 0);
        Ok(())
    }

    /// 写高字半步（闭合步）：访序纪律检查（低字必须已写）→ 窗口期跨 RTE 关
    /// → 选通高字号 → IOWIN 写 → 落寄存器文件（同纪元）→ 闭窗 → 读方重组校验
    /// → 取消选通。
    pub fn write_high(&mut self, rte: usize, word: &RteWord) -> Result<(), RteRwError> {
        if rte >= RTE_COUNT {
            return Err(RteRwError::RteIndexOutOfRange);
        }
        // 关一：窗口期跨 RTE 切换（先于访序检查——窗口在场时这是更紧的违例）。
        if let Some(ow) = self.window.open_rte() {
            if ow as usize != rte {
                self.window.bump_cross_switch();
                self.events.push(self.tick, EventKind::Reject, ow, RteRwError::CrossRteSwitchInWindow.code());
                return Err(RteRwError::CrossRteSwitchInWindow);
            }
        }
        // 关二：访序纪律——低字必须先写（写序：低字→高字）。
        if !self.pending_low[rte] {
            self.order_violations = self.order_violations.wrapping_add(1);
            self.events.push(self.tick, EventKind::Reject, rte as u8, RteRwError::WriteOrderInverted.code());
            return Err(RteRwError::WriteOrderInverted);
        }
        let hi_before = self.hw.half_epochs(rte).1;
        self.select_for_write(rte, HALF_HI)?;
        self.proto.iowin_write(word.hi())?;
        let v = self.proto.take_win()?;
        self.proto.deselect()?;
        self.hw.write_half(rte, HALF_HI, v, self.cur_epoch);
        self.window.close(self.tick, hi_before)?;
        self.pending_low[rte] = false;
        // 读方重组校验：闭窗后高低字纪元必须一致。
        let (le, he) = self.hw.half_epochs(rte);
        let ok = le != 0 && le == he;
        self.window.mark_reassembly(ok);
        self.events.push(self.tick, EventKind::WindowClose, rte as u8, 0);
        Ok(())
    }

    /// 纪律化完整写：串行化 → 向量校验 → 低字 → 窗口内并发读观测 → 高字闭合
    /// → 读回一致性 → 释放。这是判据一/二/三的主通道。
    pub fn programmed_write(&mut self, rte: usize, word: &RteWord, owner: u64) -> Result<(), RteRwError> {
        if rte >= RTE_COUNT {
            return Err(RteRwError::RteIndexOutOfRange);
        }
        // 串行化（原语级互斥；纪律路径预期直接授予）。
        match self.serial.acquire(owner, self.tick) {
            AcquireOutcome::Granted => {}
            AcquireOutcome::Serialized { predicted_wait } => {
                self.events.push(self.tick, EventKind::SerializeWait, rte as u8, 0);
                self.serial.grant_waiter(owner, self.tick.saturating_add(predicted_wait));
            }
        }
        // 向量校验（写入前显性拦截）。
        if !word.vector_valid() {
            self.field_rejects = self.field_rejects.wrapping_add(1);
            self.events.push(self.tick, EventKind::Reject, rte as u8, RteRwError::FieldInvalid.code());
            self.serial.release(self.tick);
            return Err(RteRwError::FieldInvalid);
        }
        let lo_res = self.write_low(rte, word);
        if lo_res.is_err() {
            self.serial.release(self.tick);
            return lo_res;
        }
        // 窗口期并发读观测（半新半旧态显性化——窗口账可见判据的观测源）。
        let obs = self.concurrent_read(rte);
        if obs.mixed {
            // 消费纪律：半新半旧态不得直接消费。
            self.window.bump_consume_block();
            self.consume_blocked = self.consume_blocked.wrapping_add(1);
            self.events.push(self.tick, EventKind::ConsumeBlocked, rte as u8, RteRwError::ConsumeBeforeClosed.code());
        }
        // 窗口期推进一刻（高低两写之间的窗口期必须有时长账面——判据三时戳可见性）。
        self.advance(1);
        let hi_res = self.write_high(rte, word);
        if hi_res.is_err() {
            self.serial.release(self.tick);
            return hi_res;
        }
        // 读回一致性（纪律化读，逐字段比对）。
        let rb = self.disciplined_read(rte);
        match rb {
            Ok(back) => {
                let ok = self.consist.compare_roundtrip(rte as u8, word, &back);
                if ok {
                    self.events.push(self.tick, EventKind::ConsistencyPass, rte as u8, 0);
                } else {
                    self.events.push(self.tick, EventKind::ConsistencyFail, rte as u8, RteRwError::ReadbackMismatch.code());
                }
            }
            Err(e) => {
                self.events.push(self.tick, EventKind::Reject, rte as u8, e.code());
                self.serial.release(self.tick);
                return Err(e);
            }
        }
        self.events.push(self.tick, EventKind::OrderOk, rte as u8, 0);
        self.serial.release(self.tick);
        Ok(())
    }

    // -----------------------------------------------------------------
    // 10.3 读原语：纪律化读（两半字各走一遍选通→装载→读→取消选通）
    // -----------------------------------------------------------------
    /// 纪律化读：重组校验（纪元一致）通过才返回整字。
    pub fn disciplined_read(&mut self, rte: usize) -> Result<RteWord, RteRwError> {
        if rte >= RTE_COUNT {
            return Err(RteRwError::RteIndexOutOfRange);
        }
        // 低字半程。
        self.proto.select(rte_index(rte, HALF_LO))?;
        let (lo, _) = self.hw.read_half_obs(rte, HALF_LO);
        self.proto.iowin_load(lo)?;
        let lo_v = self.proto.iowin_read()?;
        self.proto.deselect()?;
        // 高字半程。
        self.proto.select(rte_index(rte, HALF_HI))?;
        let (hi, _) = self.hw.read_half_obs(rte, HALF_HI);
        self.proto.iowin_load(hi)?;
        let hi_v = self.proto.iowin_read()?;
        self.proto.deselect()?;
        // 读方重组校验。
        let (le, he) = self.hw.half_epochs(rte);
        if le != he {
            return Err(RteRwError::ReassemblyMismatch);
        }
        Ok(RteWord::from_parts(lo_v, hi_v))
    }

    /// 窗口期并发读观测：模拟第二个读者在窗口期内抢读。返回的观测带半新半旧
    /// 显性标记；半新半旧不是 bug 是窗口语义——但消费必须走 consume_word 纪律。
    pub fn concurrent_read(&mut self, rte: usize) -> WindowObservation {
        let window_open = self.window.is_open() && self.window.open_rte() == Some(rte as u8);
        let (lo, lo_ep) = self.hw.read_half_obs(rte, HALF_LO);
        let (hi, hi_ep) = self.hw.read_half_obs(rte, HALF_HI);
        let mixed = window_open && lo_ep != hi_ep;
        let obs = WindowObservation {
            rte: rte as u8,
            lo,
            hi,
            lo_epoch: lo_ep,
            hi_epoch: hi_ep,
            mixed,
            lo_fresh: if window_open { HalfFreshness::Fresh } else { HalfFreshness::Stale },
            hi_fresh: if mixed { HalfFreshness::Stale } else { HalfFreshness::Fresh },
        };
        if mixed {
            self.window.record_mixed_read();
            self.events.push(self.tick, EventKind::MixedRead, rte as u8, 0);
        }
        obs
    }

    /// 消费纪律：半新半旧态禁止直接消费（纪元不一致即拦）。
    pub fn consume_word(&mut self, obs: &WindowObservation) -> Result<RteWord, RteRwError> {
        if obs.mixed {
            self.window.bump_consume_block();
            self.consume_blocked = self.consume_blocked.wrapping_add(1);
            self.events.push(self.tick, EventKind::ConsumeBlocked, obs.rte, RteRwError::ConsumeBeforeClosed.code());
            return Err(RteRwError::ConsumeBeforeClosed);
        }
        Ok(RteWord::from_parts(obs.lo, obs.hi))
    }

    // -----------------------------------------------------------------
    // 10.4 注入面：四形态，全部必须被拦截并留痕
    // -----------------------------------------------------------------
    /// 注入一：无选通直接访问 IOWIN。预期 AccessWithoutSelect。
    pub fn inject_no_select(&mut self) -> RteRwError {
        let e = match self.proto.iowin_write(INJECT_SENTINEL) {
            Err(e) => e,
            Ok(()) => RteRwError::ProtoStateInvalid, // 不可达：Idle 态写必拒；真可达即本身是错。
        };
        self.events.push(self.tick, EventKind::InjectDetected, RTE_NONE, e.code());
        e
    }

    /// 注入二：非法向量（保留区 0x00–0x1F / 广播 0xFF）。预期 FieldInvalid。
    pub fn inject_bad_vector(&mut self, rte: usize) -> RteRwError {
        let bad = RteWord { raw: 0x0000_0000_0000_0003 }; // 向量 3：CPU 异常保留区。
        let e = match self.programmed_write(rte, &bad, OWNER_BASE + rte as u64) {
            Err(e) => e,
            Ok(()) => RteRwError::ProtoStateInvalid,
        };
        self.events.push(self.tick, EventKind::InjectDetected, rte as u8, e.code());
        e
    }

    /// 注入三：写序颠倒（无低字铺垫直接写高字）。预期 WriteOrderInverted。
    pub fn inject_order_inverted(&mut self, rte: usize) -> RteRwError {
        let word = self.gen_word();
        // 入侵者拿票（与正常持有者竞争建模省略——注入目标是访序纪律本身）。
        match self.serial.acquire(OWNER_INTRUDER, self.tick) {
            AcquireOutcome::Granted => {}
            AcquireOutcome::Serialized { predicted_wait } => {
                self.serial.grant_waiter(OWNER_INTRUDER, self.tick.saturating_add(predicted_wait));
            }
        }
        let e = match self.write_high(rte, &word) {
            Err(e) => e,
            Ok(()) => RteRwError::ProtoStateInvalid,
        };
        self.serial.release(self.tick);
        self.events.push(self.tick, EventKind::InjectDetected, rte as u8, e.code());
        e
    }

    /// 注入四：窗口期跨 RTE 切换选通。预期 CrossRteSwitchInWindow。
    /// 流程：对 RTE A 开窗（写低字）→ 尝试对 RTE B 走闭合步（跨 RTE）→ 拦截
    /// → 回头闭合 A 窗口（账面回到静止）。
    pub fn inject_cross_switch(&mut self, rte_a: usize, rte_b: usize) -> RteRwError {
        let word = self.gen_word();
        let owner = OWNER_BASE + rte_a as u64;
        match self.serial.acquire(owner, self.tick) {
            AcquireOutcome::Granted => {}
            AcquireOutcome::Serialized { predicted_wait } => {
                self.serial.grant_waiter(owner, self.tick.saturating_add(predicted_wait));
            }
        }
        // A 开窗（低字落账、窗口在场、高字未写）。
        let lo_res = self.write_low(rte_a, &word);
        if lo_res.is_err() {
            self.serial.release(self.tick);
            let e = match lo_res { Err(e) => e, Ok(()) => RteRwError::ProtoStateInvalid };
            self.events.push(self.tick, EventKind::InjectDetected, rte_a as u8, e.code());
            return e;
        }
        // 跨 RTE 切换尝试：对 B 走闭合步——必须被窗口期跨 RTE 关拦截。
        let e = match self.write_high(rte_b, &word) {
            Err(e) => e,
            Ok(()) => RteRwError::ProtoStateInvalid,
        };
        // 回头闭合 A：完整纪律收尾（读回一致性一并落账；先推一刻给窗口期时长）。
        self.advance(1);
        let hi_res = self.write_high(rte_a, &word);
        if hi_res.is_ok() {
            if let Ok(back) = self.disciplined_read(rte_a) {
                let ok = self.consist.compare_roundtrip(rte_a as u8, &word, &back);
                self.events.push(self.tick, if ok { EventKind::ConsistencyPass } else { EventKind::ConsistencyFail }, rte_a as u8, 0);
            }
        }
        self.serial.release(self.tick);
        self.events.push(self.tick, EventKind::InjectDetected, rte_a as u8, e.code());
        e
    }

    /// 越界 RTE 号注入（协议第一道关）。预期 RteIndexOutOfRange。
    pub fn inject_oor_index(&mut self) -> RteRwError {
        let e = match self.proto.select(0x00FF) {
            Err(e) => e,
            Ok(()) => RteRwError::ProtoStateInvalid,
        };
        self.events.push(self.tick, EventKind::InjectDetected, RTE_NONE, e.code());
        e
    }

    /// 四形态注入总闸：全部必须被拦截。
    pub fn run_injections(&mut self) -> [bool; 4] {
        let i1 = matches!(self.inject_no_select(), RteRwError::AccessWithoutSelect);
        self.advance(1);
        let i2 = matches!(self.inject_bad_vector(2), RteRwError::FieldInvalid);
        self.advance(1);
        let i3 = matches!(self.inject_order_inverted(5), RteRwError::WriteOrderInverted);
        self.advance(1);
        let i4 = matches!(self.inject_cross_switch(7, 9), RteRwError::CrossRteSwitchInWindow);
        self.advance(1);
        let _ = self.inject_oor_index();
        [i1, i2, i3, i4]
    }

    // -----------------------------------------------------------------
    // 10.5 串行化注入：并发访序注入 10/10 次被串行化保护
    // -----------------------------------------------------------------
    /// 并发访序注入 rounds 轮：合法持有者持票期间，入侵者反复抢票——
    /// 每一轮都必须被串行化（进等待者账），返回被保护轮数。
    pub fn serialize_inject_rounds(&mut self, rounds: u64) -> u64 {
        let mut protected: u64 = 0;
        let mut r = 0;
        while r < rounds {
            // 合法持有者拿票。
            let _ = self.serial.acquire(OWNER_HOLDER, self.tick);
            self.advance(1);
            // 入侵者抢票：必须被串行化。
            match self.serial.acquire(OWNER_INTRUDER, self.tick) {
                AcquireOutcome::Granted => {
                    // 穿透：不可达——账面直接可见（返回值会小于 rounds）。
                }
                AcquireOutcome::Serialized { predicted_wait } => {
                    protected = protected.wrapping_add(1);
                    self.events.push(self.tick, EventKind::SerializeWait, RTE_NONE, 0);
                    let _ = predicted_wait;
                }
            }
            // 合法持有者释放 → 票移交等待者 → 等待者用毕释放。
            self.serial.release(self.tick);
            self.serial.grant_waiter(OWNER_INTRUDER, self.tick);
            self.events.push(self.tick, EventKind::SerializeGrant, RTE_NONE, 0);
            self.serial.release(self.tick);
            self.advance(1);
            r += 1;
        }
        protected
    }

    // -----------------------------------------------------------------
    // 10.6 一致性扫掠：全 RTE × 合法字写入读回（判据一主通道）
    // -----------------------------------------------------------------
    /// 一致性扫掠：对全部 RTE 各程序化写一个确定性合法字（每窗口内含一次
    /// 并发读观测），读回逐字段比对。
    pub fn run_consistency_sweep(&mut self) -> u64 {
        let mut ok_count = 0;
        let mut rte = 0;
        while rte < RTE_COUNT {
            let word = self.gen_word();
            let owner = OWNER_BASE + rte as u64;
            if self.programmed_write(rte, &word, owner).is_ok() {
                ok_count += 1;
            }
            self.advance(1);
            rte += 1;
        }
        ok_count
    }

    // -----------------------------------------------------------------
    // 10.7 指纹与健康
    // -----------------------------------------------------------------
    /// FNV-1a 单字混入。
    fn fnv_word(h: u64, v: u64) -> u64 {
        let mut x = h;
        let mut i = 0;
        while i < 8 {
            x = (x ^ ((v >> (i * 8)) & 0xFF)).wrapping_mul(FNV_PRIME);
            i += 1;
        }
        x
    }
    /// 状态指纹：寄存器文件全量（值+纪元）+ 全部账面计数。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = Self::fnv_word(h, self.tick);
        h = Self::fnv_word(h, self.hw.epoch_now() as u64);
        let mut i = 0;
        while i < RTE_COUNT {
            let (lo, lo_ep) = self.hw.read_half_obs(i, HALF_LO);
            let (hi, hi_ep) = self.hw.read_half_obs(i, HALF_HI);
            h = Self::fnv_word(h, lo as u64);
            h = Self::fnv_word(h, hi as u64);
            h = Self::fnv_word(h, lo_ep as u64);
            h = Self::fnv_word(h, hi_ep as u64);
            h = Self::fnv_word(h, self.pending_low[i] as u64);
            h = Self::fnv_word(h, self.pending_word[i]);
            i += 1;
        }
        let (ac, rc, sc) = self.serial.counts();
        h = Self::fnv_word(h, ac);
        h = Self::fnv_word(h, rc);
        h = Self::fnv_word(h, sc);
        let (fc, mm) = self.consist.counts();
        h = Self::fnv_word(h, fc);
        h = Self::fnv_word(h, mm);
        h = Self::fnv_word(h, self.order_violations);
        h = Self::fnv_word(h, self.consume_blocked);
        h = Self::fnv_word(h, self.window.entry_count());
        h = Self::fnv_word(h, self.window.mixed_total());
        h = Self::fnv_word(h, self.window.dropped());
        h = Self::fnv_word(h, self.events.len());
        h = Self::fnv_word(h, self.events.dropped());
        h
    }

    /// 健康账：八项不变量（判据外围的结构性自证）。
    pub fn health(&self) -> HealthReport {
        let mut items: Vec<HealthItem> = Vec::new();
        // H1 协议静止：无选通挂起。
        items.push(HealthItem {
            id: "H1-proto-idle",
            pass: self.proto.is_idle(),
            note: "协议机静止态必须为 Idle（选通→访问→取消选通三步走完）",
        });
        // H2 协议收支：选通数 == 取消选通数。
        let (sel, de, _, _) = self.proto.access_counts();
        items.push(HealthItem {
            id: "H2-proto-balance",
            pass: sel == de,
            note: "选通次数与取消选通次数必须相等（无悬空选通）",
        });
        // H3 窗口静止：无未闭合窗口。
        items.push(HealthItem {
            id: "H3-window-closed",
            pass: !self.window.is_open(),
            note: "静止时不得存在未闭合拆写窗口",
        });
        // H4 窗口重组：全部闭合条目读方重组校验通过。
        let entries = self.window.entries();
        let mut all_reasm = true;
        let mut i = 0;
        while i < entries.len() {
            if !entries[i].reassembly_ok { all_reasm = false; }
            i += 1;
        }
        items.push(HealthItem {
            id: "H4-reassembly",
            pass: all_reasm,
            note: "全部窗口条目读方重组校验通过（闭窗后高低字纪元一致）",
        });
        // H5 串行化静止与收支。
        let (ac, rc, _) = self.serial.counts();
        items.push(HealthItem {
            id: "H5-serial-rest",
            pass: self.serial.at_rest() && ac == rc,
            note: "串行化账静止（无持有者）且授予数等于释放数",
        });
        // H6 串行化预算：无超预算持有。
        let (_, _, over) = self.serial.hold_stats();
        items.push(HealthItem {
            id: "H6-hold-budget",
            pass: over == 0,
            note: "串行化持有时长无超 HOLD_BUDGET 记录",
        });
        // H7 一致性达标。
        items.push(HealthItem {
            id: "H7-consistency",
            pass: self.consist.at_target(),
            note: "写入读回逐字段一致性达到 100% 目标",
        });
        // H8 访序清洁：无挂起低字（全部 RTE 高低字纪元一致或从未写过）。
        let mut no_pending = true;
        let mut j = 0;
        while j < RTE_COUNT {
            if self.pending_low[j] { no_pending = false; }
            j += 1;
        }
        items.push(HealthItem {
            id: "H8-order-clean",
            pass: no_pending,
            note: "无挂起低字（每 RTE 都处于闭合态或从未写过态）",
        });
        let mut ok_count = 0;
        let mut fails: Vec<&'static str> = Vec::new();
        let mut i = 0;
        while i < items.len() {
            if items[i].pass {
                ok_count += 1;
            } else {
                fails.push(items[i].id);
            }
            i += 1;
        }
        HealthReport { ok: fails.is_empty(), ok_count, total: items.len() as u32, fails }
    }

    // -----------------------------------------------------------------
    // 10.8 账面只读出口
    // -----------------------------------------------------------------
    /// 协议机只读出口。
    pub fn proto(&self) -> &IndirectAddrProtocol { &self.proto }
    /// 窗口账只读出口。
    pub fn window(&self) -> &RteSplitWindow { &self.window }
    /// 串行化账只读出口。
    pub fn serial(&self) -> &RwSerializer { &self.serial }
    /// 一致性账只读出口。
    pub fn consist(&self) -> &ConsistencyLedger { &self.consist }
    /// 事件账只读出口。
    pub fn events(&self) -> &EventLedger { &self.events }
    /// 寄存器文件只读出口。
    pub fn hw(&self) -> &RteHw { &self.hw }
    /// 写序颠倒拦截数。
    pub fn order_violations(&self) -> u64 { self.order_violations }
    /// 非法字段拦截数。
    pub fn field_rejects(&self) -> u64 { self.field_rejects }
    /// 半新半旧消费拦截数。
    pub fn consume_blocked(&self) -> u64 { self.consume_blocked }
}

// ===========================================================================
// 11. 观察/对拍/自检/导出
// ===========================================================================
/// 健康项。
#[derive(Clone, Debug)]
pub struct HealthItem {
    /// 项标识。
    pub id: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 说明。
    pub note: &'static str,
}
/// 健康账。
#[derive(Clone, Debug)]
pub struct HealthReport {
    /// 总判定。
    pub ok: bool,
    /// 通过项数。
    pub ok_count: u32,
    /// 总项数。
    pub total: u32,
    /// 失败项清单。
    pub fails: Vec<&'static str>,
}

/// 一次完整运行的观察账（判据三成分的直接落点）。
#[derive(Clone, Debug)]
pub struct RteRwObserve {
    /// 种子。
    pub seed: u64,
    /// 一致性百分比（判据一）。
    pub consistency_pct: u64,
    /// 字段比对总数。
    pub consistency_fields: u64,
    /// 不一致总数。
    pub consistency_mismatches: u64,
    /// 一致性达标（pct == 100 且零不一致）。
    pub consistency_ok: bool,
    /// 串行化注入轮数（判据二：10）。
    pub serialized_rounds: u64,
    /// 被串行化保护轮数（判据二：10/10）。
    pub serialized_protected: u64,
    /// 窗口账条目数（判据三）。
    pub window_entries: u64,
    /// 半新半旧观测总数（判据三）。
    pub window_mixed_reads: u64,
    /// 窗口账可见（判据三总闸）。
    pub window_visible: bool,
    /// 消费拦截数（半新半旧不得直接消费的账面证据）。
    pub consume_blocked: u64,
    /// 注入检出：无选通。
    pub inject_no_select: bool,
    /// 注入检出：非法向量。
    pub inject_bad_vector: bool,
    /// 注入检出：写序颠倒。
    pub inject_order: bool,
    /// 注入检出：窗口期跨 RTE 切换。
    pub inject_cross: bool,
    /// 四注入全检出。
    pub inject_all: bool,
    /// 双跑指纹 A。
    pub fingerprint_a: u64,
    /// 双跑指纹 B。
    pub fingerprint_b: u64,
    /// 双跑确定性（指纹一致）。
    pub dual_stable: bool,
    /// 健康全绿。
    pub health_ok: bool,
    /// 健康通过项数。
    pub health_ok_count: u32,
    /// 健康总项数。
    pub health_total: u32,
    /// 事件账在账数。
    pub event_count: u64,
    /// 事件账丢弃数。
    pub event_dropped: u64,
    /// 一致性往返总数（扫掠 + 注入收尾）。
    pub roundtrips: u64,
    /// 全绿往返数。
    pub roundtrips_ok: u64,
}

/// 单次运行：一致性扫掠 → 串行化注入 10 轮 → 四形态注入 → 指纹。
/// 全程确定性：LCG 驱动、逻辑刻内推进、零外部输入——双跑指纹必须一致。
pub fn run_program(seed: u64) -> (RteRwEngine, u64) {
    let mut e = RteRwEngine::new(seed);
    let _ = e.run_consistency_sweep();
    let _ = e.serialize_inject_rounds(SERIALIZED_INJECT_ROUNDS);
    let _ = e.run_injections();
    let fp = e.fingerprint();
    (e, fp)
}

/// 判据复测入口：同种子双跑对拍 + 全账面快照。
pub fn observe(seed: u64) -> RteRwObserve {
    let (e1, fp1) = run_program(seed);
    let (_e2, fp2) = run_program(seed);
    let dual_stable = fp1 == fp2;
    let (fc, mm) = e1.consist.counts();
    let (rt, rt_ok) = e1.consist.roundtrips();
    let health = e1.health();
    RteRwObserve {
        seed,
        consistency_pct: e1.consist.pct(),
        consistency_fields: fc,
        consistency_mismatches: mm,
        consistency_ok: e1.consist.at_target(),
        serialized_rounds: SERIALIZED_INJECT_ROUNDS,
        serialized_protected: e1.serial.serialized_total(),
        window_entries: e1.window.entry_count(),
        window_mixed_reads: e1.window.mixed_total(),
        window_visible: e1.window.visible(),
        consume_blocked: e1.consume_blocked(),
        inject_no_select: e1.proto.reject_counts().0 > 0,
        inject_bad_vector: e1.field_rejects() > 0,
        inject_order: e1.order_violations() > 0,
        inject_cross: e1.window.cross_switch_rejects() > 0,
        inject_all: e1.proto.reject_counts().0 > 0
            && e1.field_rejects() > 0
            && e1.order_violations() > 0
            && e1.window.cross_switch_rejects() > 0,
        fingerprint_a: fp1,
        fingerprint_b: fp2,
        dual_stable,
        health_ok: health.ok,
        health_ok_count: health.ok_count,
        health_total: health.total,
        event_count: e1.events.len(),
        event_dropped: e1.events.dropped(),
        roundtrips: rt,
        roundtrips_ok: rt_ok,
    }
}

/// 自检项。
#[derive(Clone, Copy, Debug)]
pub struct Check {
    /// 项号。
    pub id: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 说明。
    pub note: &'static str,
}

/// 自检套件（判据总闸，10 项）。
pub fn self_check(ob: &RteRwObserve) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    out.push(Check {
        id: "C0862-01 consistency",
        pass: ob.consistency_ok && ob.consistency_pct >= CONSISTENCY_TARGET_PCT,
        note: "判据一：原语读写一致性 100%（写入读回逐字段，零不一致）",
    });
    out.push(Check {
        id: "C0862-02 serialize",
        pass: ob.serialized_rounds == SERIALIZED_INJECT_ROUNDS
            && ob.serialized_protected == ob.serialized_rounds,
        note: "判据二：并发访序注入 10/10 次被串行化保护（零穿透）",
    });
    out.push(Check {
        id: "C0862-03 window-visible",
        pass: ob.window_visible && ob.window_entries > 0 && ob.window_mixed_reads > 0,
        note: "判据三：64 位拆写窗口账可见（条目/时戳/半新半旧观测/重组校验齐全）",
    });
    out.push(Check {
        id: "C0862-04 inject",
        pass: ob.inject_all,
        note: "注入面四形态（无选通/非法向量/写序颠倒/窗口期跨 RTE）全部检出",
    });
    out.push(Check {
        id: "C0862-05 dual-run",
        pass: ob.dual_stable && ob.fingerprint_a == ob.fingerprint_b,
        note: "双跑确定性对拍：同种子两次运行指纹一致",
    });
    out.push(Check {
        id: "C0862-06 consume",
        pass: ob.consume_blocked > 0,
        note: "半新半旧态消费纪律拦截留痕（窗口语义显性可见）",
    });
    out.push(Check {
        id: "C0862-07 health",
        pass: ob.health_ok && ob.health_ok_count == ob.health_total,
        note: "健康账八项不变量全绿",
    });
    out.push(Check {
        id: "C0862-08 seg",
        pass: err_segment_ok(),
        note: "诊断码段独占：16 变体恰好占满 0x1970–0x197F 且无越段",
    });
    out.push(Check {
        id: "C0862-09 sweep",
        pass: ob.consistency_fields >= (RTE_COUNT * PROGRAMMABLE_FIELD_COUNT) as u64
            && ob.consistency_mismatches == 0
            && ob.roundtrips_ok == ob.roundtrips,
        note: "扫掠覆盖：24 RTE × 7 可编程字段 ≥ 168 次逐字段比对（注入收尾往返计入），全部全绿",
    });
    out.push(Check {
        id: "C0862-10 events",
        pass: ob.event_count > 0,
        note: "事件账非空（全流程留痕）",
    });
    out
}

/// 自检全过。
pub fn self_check_ok(ob: &RteRwObserve) -> bool {
    let c = self_check(ob);
    let mut i = 0;
    while i < c.len() {
        if !c[i].pass {
            return false;
        }
        i += 1;
    }
    true
}

/// 自检报告串。
pub fn self_check_report(ob: &RteRwObserve) -> String {
    let mut s = String::new();
    let c = self_check(ob);
    let mut i = 0;
    while i < c.len() {
        let x = &c[i];
        s.push_str(&format!("{} [{}] {}\n", x.id, if x.pass { "PASS" } else { "FAIL" }, x.note));
        i += 1;
    }
    s
}

/// 诊断码段独占校验：全部变体码落在 [SEG_BASE, SEG_END] 且互不重复。
pub fn err_segment_ok() -> bool {
    let all = RteRwError::all();
    let mut i = 0;
    while i < all.len() {
        let c = all[i].code();
        if c < SEG_BASE || c > SEG_END {
            return false;
        }
        let mut j = 0;
        while j < i {
            if all[j].code() == c {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// KV 导出（账面并排）。
pub fn export_kv(ob: &RteRwObserve) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    v.push(("version".to_string(), VERSION_TEXT.to_string()));
    v.push(("criterion".to_string(), CRITERION_ID.to_string()));
    v.push(("seg".to_string(), format!("0x{:04X}-0x{:04X}", SEG_BASE, SEG_END)));
    v.push(("consistency_pct".to_string(), ob.consistency_pct.to_string()));
    v.push(("consistency_fields".to_string(), ob.consistency_fields.to_string()));
    v.push(("consistency_mismatches".to_string(), ob.consistency_mismatches.to_string()));
    v.push(("serialized_rounds".to_string(), ob.serialized_rounds.to_string()));
    v.push(("serialized_protected".to_string(), ob.serialized_protected.to_string()));
    v.push(("window_entries".to_string(), ob.window_entries.to_string()));
    v.push(("window_mixed_reads".to_string(), ob.window_mixed_reads.to_string()));
    v.push(("window_visible".to_string(), ob.window_visible.to_string()));
    v.push(("consume_blocked".to_string(), ob.consume_blocked.to_string()));
    v.push(("inject_no_select".to_string(), ob.inject_no_select.to_string()));
    v.push(("inject_bad_vector".to_string(), ob.inject_bad_vector.to_string()));
    v.push(("inject_order".to_string(), ob.inject_order.to_string()));
    v.push(("inject_cross".to_string(), ob.inject_cross.to_string()));
    v.push(("dual_stable".to_string(), ob.dual_stable.to_string()));
    v.push(("fingerprint".to_string(), format!("0x{:016X}", ob.fingerprint_a)));
    v.push(("health_ok".to_string(), ob.health_ok.to_string()));
    v.push(("events".to_string(), ob.event_count.to_string()));
    v.push(("events_dropped".to_string(), ob.event_dropped.to_string()));
    v
}

/// TLV 导出（type:u8 | len:u16 | value，定长小端）。
pub fn export_tlv(ob: &RteRwObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    // T0 版本串。
    let vb = VERSION_TEXT.as_bytes();
    out.push(0x01);
    out.push((vb.len() as u16 & 0xFF) as u8);
    out.push(((vb.len() as u16 >> 8) & 0xFF) as u8);
    let mut i = 0;
    while i < vb.len() { out.push(vb[i]); i += 1; }
    // T1 一致性（u64 pct, u64 fields, u64 mismatches）。
    out.push(0x11); out.push(24); out.push(0);
    push_u64(&mut out, ob.consistency_pct);
    push_u64(&mut out, ob.consistency_fields);
    push_u64(&mut out, ob.consistency_mismatches);
    // T2 串行化（u64 rounds, u64 protected）。
    out.push(0x12); out.push(16); out.push(0);
    push_u64(&mut out, ob.serialized_rounds);
    push_u64(&mut out, ob.serialized_protected);
    // T3 窗口账（u64 entries, u64 mixed, u8 visible, u64 consume_blocked）。
    out.push(0x13); out.push(25); out.push(0);
    push_u64(&mut out, ob.window_entries);
    push_u64(&mut out, ob.window_mixed_reads);
    out.push(if ob.window_visible { 1 } else { 0 });
    push_u64(&mut out, ob.consume_blocked);
    // T4 指纹（u64 a, u64 b, u8 dual）。
    out.push(0x14); out.push(17); out.push(0);
    push_u64(&mut out, ob.fingerprint_a);
    push_u64(&mut out, ob.fingerprint_b);
    out.push(if ob.dual_stable { 1 } else { 0 });
    out
}

/// 小端 u64 推入。
fn push_u64(out: &mut Vec<u8>, v: u64) {
    let mut i = 0;
    while i < 8 {
        out.push(((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
}

/// TLV 往返校验：长度域与实际载荷一致且总长守恒。
pub fn tlv_roundtrip_ok(ob: &RteRwObserve) -> bool {
    let b = export_tlv(ob);
    let mut pos = 0;
    let n = b.len();
    while pos < n {
        if pos + 3 > n { return false; }
        let l = b[pos + 1] as usize | ((b[pos + 2] as usize) << 8);
        if pos + 3 + l > n { return false; }
        pos += 3 + l;
    }
    pos == n
}

/// 不变量清单。
pub fn invariant_manifest() -> Vec<String> { vec![
    "INV-1 间接寻址三步：选通→访问→取消选通，IOWIN 访问必须处于 Selected 态".to_string(),
    "INV-2 越界拒收：寄存器号必须落在 RTE 地址空间 [0x10, 0x40)；非 RTE 号与越界号一律拒收".to_string(),
    "INV-3 写序纪律：低字→高字，颠倒即拒并留痕".to_string(),
    "INV-4 窗口单例：同一时刻至多一个拆写窗口在场；窗口期跨 RTE 切换选通即拒".to_string(),
    "INV-5 纪元一致：闭窗后高低字纪元必须一致（读方重组校验）".to_string(),
    "INV-6 半新半旧是窗口语义：窗口期内并发读到的高旧低新态显性标记，不是错误；但消费必须等闭合".to_string(),
    "INV-7 原语级串行化：第二个竞争访问必须进等待者账，绝不穿透".to_string(),
    "INV-8 一致性 100%：写入读回逐字段（7 可编程字段），只读字段不计账".to_string(),
    "INV-9 溢出显性：全部算术 wrapping/saturating；账溢出丢弃显性自增".to_string(),
    "INV-10 tick 单调：逻辑刻只前进不倒流".to_string(),
    "INV-11 确定性：同种子双跑指纹一致（零真实时钟/零真实随机）".to_string(),
    "INV-12 留痕：全部拦截/注入/窗口/串行化事件入账，禁静默".to_string(),
    ] }

/// 错误闭集清单串。
pub fn err_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let all = RteRwError::all();
    let mut i = 0;
    while i < all.len() {
        v.push(format!("0x{:04X} {} — {}", all[i].code(), all[i].name(), all[i].detail()));
        i += 1;
    }
    v
}

/// 字段清单串（SDM 逐位对照账）。
pub fn field_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0;
    while i < ALL_FIELDS.len() {
        let f = &ALL_FIELDS[i];
        v.push(format!(
            "F{} {} bits {}:{} width={} programmable={}",
            i, f.name(), f.shift(), f.shift() + f.width() - 1, f.width(), f.is_programmable()
        ));
        i += 1;
    }
    v
}

/// 版本锚。
pub const TICKET: &str = "UNX-F0862";
/// 标题。
pub const TITLE: &str = "RTE 间接寻址读写原语与访序纪律";
/// 批次。
pub const BATCH: &str = "CoRun·UNX-A2-B04";
/// 册目标行数。
pub const TARGET_LINES: u32 = 300;
/// 册内关联单号（同批 F861–F880 邻条 + 上游）。
pub const RELATED_TICKETS: [&str; 8] = ["F0861", "F0863", "F0864", "F0865", "F0866", "F0004", "F0821", "F0023"];

/// 关联单串。
pub fn related_str() -> String {
    let mut s = String::new();
    let mut i = 0;
    while i < RELATED_TICKETS.len() {
        s.push_str(RELATED_TICKETS[i]);
        s.push(' ');
        i += 1;
    }
    s
}
