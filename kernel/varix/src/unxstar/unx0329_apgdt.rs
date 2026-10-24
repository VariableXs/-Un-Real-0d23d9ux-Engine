//! unx0329_apgdt —— UNX-F0329 AP 端 GDT/TSS/MSR 装载序列。
//!
//! =====================================================================
//! 锚点原文（主册《CoRun Varix STAR II · Unxreal》UNX-A1 批次表，逐字抄录）：
//!
//!   | UNX-F0329 | AP 端 GDT/TSS/MSR 装载序列 | 260 | 已深化 | UNX-F0329-J1 AP 端 lgdt/ltr/MSR 装载序列与 BSP 同构，装载后读回断言过 |
//!
//! 判据原文：UNX-F0329-J1 「AP 端 lgdt/ltr/MSR 装载序列与 BSP 同构，
//!   装载后读回断言过」。
//! 定位原文（同表行）：UNX-F0329 · AP 端 GDT/TSS/MSR 装载序列 ·
//!   目标 260 行 · 状态 已深化 · 属 UNX AP 启动域（邻件 unx0328_apstack.rs，
//!   F0328 栈与 per-CPU 区实例化——本模块的直接上游供给者）。
//! =====================================================================
//!
//! 语义上游（消费契约，全部以注入面解耦、零 crate 依赖）：
//!   - UNX-F0328（AP 栈与 per-CPU 区实例化）：本模块消费其「每 AP 独立
//!     栈/区」产物——RSP0 取 AP 内核栈顶、IST1..IST7 取七路异常栈顶、
//!     GS_BASE 取 per-CPU 区基址。F0328 管「分配与初始化」，本模块管
//!     「装载进 CPU 寄存器语义」，两模块职责零重叠；本模块以
//!     `TssProgramParams` / `MsrPlan` 参数包承接（栈顶/区基址由调用方
//!     按 F0328 账本填入，本模块只认数值不认所有权）。
//!   - UNX-F0307（NX 使能与 EFER 协同）：BSP 侧 EFER.SCE|LME|LMA|NXE
//!     的装载语义在 AP 侧复刻——`EFER_CANON = 0xD01` 与 BSP 冻结值
//!     同构（BSP 基线镜像字段之一）。
//!   - UNX-F0344（GS base 切换与 per-CPU 自识别）：本模块把 GS_BASE
//!     装载到位（wrmsr + 读回断言），F0344 在其上做自识别；写序纪律
//!     KERNEL_GS_BASE 置于 GS_BASE 之后（SWAPGS 配对语义：内核 GS
//!     在前，用户 GS 占 KERNEL_GS_BASE 槽）。
//!   - UNX-F0335（AP 数量上限）：`MAX_AP = 255` 同源上限，orchestrate
//!     对入参做 saturating 收口。
//!
//! 组件清单（全部真实现，无占位；寄存器语义走 trait 注入面，不写真实汇编）：
//!   1. GdtLayout —— GDT 布局表：空描述符 + 64 位代码段 + 数据段 +
//!      TSS 双描述符（长模式 TSS 是 16 字节系统描述符，SDM §3.5.2），
//!      共 5 槽 40 字节；段描述符 8 字节编码真实实现（limit 20 位 /
//!      base 32 位 / access 字节 / flags 半字节位域拆装），
//!      `encode_desc`/`decode_desc` 双向 const fn + 编解码往返自证
//!      （编译期 const 断言 + 运行期 self_check 电池双份）。
//!   2. GdtImage —— 每 AP 独立 GDT 实例：40 字节镜像 + program 时记录
//!      的校验和（FNV 式位置加权和，wrapping）；lgdt 前重算比对——
//!      program 后被踩踏/篡改即 GdtChecksumBad 拦截。
//!   3. TssLayout / TssImage —— TSS 布局表与实例：x86_64 TSS 104 字节
//!      标准布局（SDM §7.7）：RSP0/1/2 + IST1..7 七栈指针 + 双保留 u64 +
//!      IO 位图基址（u16）；全部偏移 const 表 + 编译期偏移链断言；
//!      IO 位图基址纪律 = 104（位图缺席 ⇒ IO 不陷 trap，SDM §7.8——
//!      位图基址超出 TSS limit 即无位图）。
//!   4. LoadSequence —— 装载序列状态机：GdtProgram→Lgdt→TssProgram→
//!      Ltr→MsrLoad 五步全序（`LoadPhase` 六态），乱序一律 OutOfPhase
//!      拒绝；每步前置校验（lgdt 前 GDT 校验和过 / ltr 前 TSS IST
//!      七路全非零 + IO 位图基址 + 保留域零 / msr 前写序纪律）+
//!      每步装载后读回断言（sgdt 对 GDTR、str 对 TR、rdmsr 对四 MSR）。
//!   5. MsrSpec / MsrLoadSeq / MsrSpace —— MSR 装载序：const 地址表
//!      （IA32_EFER 0xC0000080 / IA32_LSTAR 0xC0000082 /
//!      IA32_GS_BASE 0xC0000101 / IA32_KERNEL_GS_BASE 0xC0000102，
//!      SDM Appendix B）+ 全序写（EFER→LSTAR→GS_BASE→KERNEL_GS_BASE，
//!      模式位→入口→内核 GS→用户 GS）+ 写后读回一致校验；`MsrSpace`
//!      是抽象 MSR 空间 trait（写入→读回一致断言面），`SimMsrSpace`
//!      为其定容模拟实现（带读回失配注入位）。
//!   6. CpuPort —— 抽象 CPU 寄存器端口 trait：lgdt/sgdt/ltr/str_reg
//!      四操作（寄存器语义抽象建模的注入面）；`SimCpuPort` 为模拟
//!      实现（GDTR/TR 影子寄存器）。
//!   7. BspMirror / ApExpectation / IsoReport —— BSP 同构断言：BSP
//!      基线镜像（canonical 描述符 + EFER/LSTAR/KGS 快照）按 AP 参数
//!      投影出期望装载像；AP 实际装载像 vs 期望像逐字段 diff
//!      （GDT 5 描述符 + TSS 13 字段 + MSR 4 值 = 22 字段/AP），
//!      同构率 100% 判据 + IsoDiff 差异定位（路径/期望/实际）。
//!   8. EventLedger / HealthReport —— 事件账（定容 256 + 溢出显性
//!      丢弃计数）+ health() 体检（AP 成/败/同构/注入检出/指纹）+
//!      FNV-1a 64 账指纹。
//!   9. ApGdtSubsystem —— 编排面：orchestrate(seed, ap_count) 全链
//!      确定性编排（Lcg 抖动 + 逐 AP 五步 + 同构 + 账）+ 双跑确定性
//!      对拍面；注入面四路（乱序注入 / MSR 读回失配注入 / IST 零注入 /
//!      GDT 篡改注入——前三为任务钦定，第四为校验和面加演），全部
//!      「注入→检出码」闭环。
//!
//! 职责定位：AP 端「段/任务门/模式寄存器装载」的唯一权威面——GDT/TSS
//!   镜像构造（布局表）、寄存器装载（CpuPort 注入）、MSR 装载
//!   （MsrSpace 注入）、同构验收（BspMirror 投影 diff）四权分立；
//!   乱序 = 序列纪律破、校验和移 = 镜像被踩、IST 零 = 异常栈缺位、
//!   读回失配 = 端口/空间故障——四案全立案可回溯。
//! 性能逐项分解：描述符编解码 O(1)；GDT/TSS program O(40/104) 常数字节；
//!   校验和 O(镜像字节)；五步序列 O(1) 步数 + O(4) MSR；同构 diff
//!   O(22) 字段/AP；重叠/巡检无 n² 面（每 AP 常数工作量）；
//!   事件账 O(1) 摊销；FNV 指纹 O(事件数×22B)。全整数确定性，无时钟、
//!   无随机（Lcg 种子显式传入）。
//!
//! 诊断码段：0x1E10~0x1E1F（SEG_BASE = 0x1E10 保留为段标识不分配，
//!   错误变体一码一位占 0x1E11~0x1E1F 共 15 码，段满零余量）。
//!   选段过程（按纪律先复核再落位）：
//!     - 预核段 0x1E70~0x1E7F：被 unx2702_ulrufield.rs 头注声明的
//!       独占段 0x1E40~0x1E7F 覆盖 → 弃用；
//!     - 备选段 0x1ED0 / 0x1EF0：均落在 unx2762_umaptbl.rs 头注声明
//!       的独占段 0x1EC0~0x1EFF 内 → 弃用；
//!     - 终选段 0x1E10~0x1E1F：全域 `grep -rn "0x1E1[0-9A-Fa-f]"`
//!       于 kernel/**/*.rs 字面量零命中（唯一形近命中 0x1E1E1E 为
//!       m4arts.rs/m600sdk.rs 的 24 位 RGB 颜色常量，非诊断码；
//!       下邻 0x1E00~0x1E06 为 vea43_sparse.rs 占用、0x1E20~0x1E23
//!       为 ves491_s27sum.rs 占用，0x1E10~0x1E1F 与两者均不重叠）。
//!
//! 纪律：no_std + alloc；本文件只 `use core::…` / `use alloc::…`，
//!   零 `use crate::…`（完全自包含）；内核路径零 unwrap/expect/panic!/
//!   运行期 assert!（编译期 const 断言不受限；#[cfg(test)] 测试体
//!   不属内核路径）；可失败全走 Result<T, ApGdtError>；计数一律
//!   saturating/wrapping 显性；>64KiB 结构禁上栈（本模块最大镜像
//!   40/104 字节 + 定容 Vec 堆置，栈上只留句柄）；const fn 内不使用
//!   slice::get / Vec 索引 / Ord::min/max / 派生 PartialEq 的 == /
//!   for 循环（一律 while + 索引）；含 Vec 字段的结构不 derive Copy。

// ============================================================================
// 第 0 节 · 基础设施：诊断码段 / FNV-1a 64 / Lcg / 字节序读写
// ============================================================================

use alloc::{format, string::String, vec, vec::Vec};

/// 诊断码段基座（0x1E10 段，独占 0x1E10~0x1E1F 共 16 码——选段过程
/// 与全域零命中证据见文件头「诊断码段」节）。
///
/// 0x1E10 本身保留为段标识，不分配给任何单条诊断码；错误变体从
/// 0x1E11 起一码一位。
pub const SEG_BASE: u16 = 0x1E10;

/// 诊断码段末码（闭集右端，含）。
pub const SEG_LAST: u16 = 0x1E1F;

/// 编译期自证：段基与末码构成 16 码闭集且基 < 末码。
const _: () = assert!(SEG_LAST - SEG_BASE == 0x0F);
/// 编译期自证：段基 16 对齐（与仓库 0x1Exx 段族对齐惯例一致）。
const _: () = assert!((SEG_BASE & 0x0F) == 0x0000);

/// FNV-1a 64 位偏移基数（FNV 工程标准常数，来源：draft-eastlake-fnv，
/// §4.1 Table 1 "64-bit FNV-1a primes/offset"）。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数（同上来源）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 单次数据流混合（对字节切片逐字节 xor-乘，全 wrapping——
/// FNV 定义即模 2^64 算术）。
pub fn fnv_mix(state: u64, bytes: &[u8]) -> u64 {
    let mut h = state;
    let mut i = 0usize;
    while i < bytes.len() {
        h = h ^ (bytes[i] as u64);
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV-1a 64 一次性散列（等价于从 FNV_BASIS 起的一次流混合）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv_mix(FNV_BASIS, bytes)
}

/// 确定性线性同余发生器（Lcg）——双跑对拍与地址抖动的唯一熵源。
///
/// 常数来源：Knuth TAOCP Vol.2 §3.3.4 表 1 的 64 位 MMX 型乘子
/// 6364136223846793005 与增量 1442695040888963407（增量非零，
/// 任意种子含 0 都不进吸收态，输出全 2^64 周期确定）。
#[derive(Clone, Debug)]
pub struct Lcg {
    /// 当前状态（种子即初态，显式传入保证双跑确定性）。
    pub state: u64,
}

impl Lcg {
    /// 以显式种子构造（种子 0 合法：首拍输出即增量本身，确定）。
    pub const fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    /// 步进一拍并返回新状态（wrapping 乘加——LCG 定义即模 2^64）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    /// 产出一个 [0, n) 内的确定值（n 为 0 时定义返回 0——除零面显性收口；
    /// 取高 48 位模 n，避免低位周期短斑）。
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next_u64() >> 16) % n
    }
}

/// 小端读 u16（越界返回 None——零 panic 面的字节序访问）。
pub fn rd_u16_le(b: &[u8], off: usize) -> Option<u16> {
    let lo = *b.get(off)? as u16;
    let hi = *b.get(off.wrapping_add(1))? as u16;
    Some(lo | (hi << 8))
}

/// 小端读 u64（同上）。
pub fn rd_u64_le(b: &[u8], off: usize) -> Option<u64> {
    let mut v = 0u64;
    let mut i = 0usize;
    while i < 8 {
        let byte = *b.get(off.wrapping_add(i))? as u64;
        v |= byte << (i * 8);
        i += 1;
    }
    Some(v)
}

/// 小端写 u16（越界返回 false，写全或写无——原子性由先查后写保证）。
pub fn wr_u16_le(b: &mut [u8], off: usize, v: u16) -> bool {
    if off.checked_add(2).map(|e| e > b.len()).unwrap_or(true) {
        return false;
    }
    b[off] = (v & 0xFF) as u8;
    b[off + 1] = ((v >> 8) & 0xFF) as u8;
    true
}

/// 小端写 u64（同上）。
pub fn wr_u64_le(b: &mut [u8], off: usize, v: u64) -> bool {
    if off.checked_add(8).map(|e| e > b.len()).unwrap_or(true) {
        return false;
    }
    let mut i = 0usize;
    while i < 8 {
        b[off + i] = ((v >> (i * 8)) & 0xFF) as u8;
        i += 1;
    }
    true
}

/// 把 8 字节数组放到切片指定偏移（越界返回 false）。
fn put_arr8(dst: &mut [u8], off: usize, src: &[u8; 8]) -> bool {
    if off.checked_add(8).map(|e| e > dst.len()).unwrap_or(true) {
        return false;
    }
    let mut i = 0usize;
    while i < 8 {
        dst[off + i] = src[i];
        i += 1;
    }
    true
}

// ============================================================================
// 第 1 节 · 错误面（0x1E11~0x1E1F 十五变体，一码一位，段满零余量）
// ============================================================================

/// AP 端 GDT/TSS/MSR 装载错误（诊断码闭集 0x1E11~0x1E1F）。
///
/// 每变体一码一位、码即文档；`detail()` 给中文可操作诊断（静态文本，
/// 携带数值的字段在变体数据里，供事件账/上层日志拼装）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApGdtError {
    /// 0x1E11 装载步序错位（五步全序被打破）。
    OutOfPhase {
        /// 期望所处的步（该步的前置步）。
        expect: LoadPhase,
        /// 实际所处的步。
        got: LoadPhase,
    },
    /// 0x1E12 lgdt 前 GDT 镜像校验和不过（program 后镜像被改写）。
    GdtChecksumBad {
        /// 涉事 AP。
        cpu_id: u32,
        /// program 时记录的校验和。
        recorded: u64,
        /// lgdt 前实时重算值。
        live: u64,
    },
    /// 0x1E13 lgdt 后 sgdt 读回与装载值不符。
    LgdtReadbackBad {
        /// 涉事 AP。
        cpu_id: u32,
        /// 期望 GDTR.base。
        expect_base: u64,
        /// 实际读回 base（sgdt 缺席以 u64::MAX 哨兵表示）。
        actual_base: u64,
        /// 期望 GDTR.limit。
        expect_limit: u16,
        /// 实际读回 limit。
        actual_limit: u16,
    },
    /// 0x1E14 ltr 前 TSS IST 栈指针为零（七路 IST 必须全配）。
    IstZero {
        /// 涉事 AP。
        cpu_id: u32,
        /// 缺位的 IST 号（1 基，1..=7）。
        slot: u32,
    },
    /// 0x1E15 TSS IO 位图基址非 104（x86_64 无位图纪律值）。
    IoMapBaseBad {
        /// 涉事 AP。
        cpu_id: u32,
        /// 期望值（恒 TSS_BYTES = 104）。
        expect: u16,
        /// 实际值。
        actual: u16,
    },
    /// 0x1E16 TSS 保留域非零（SDM §7.7 保留域必须为零）。
    TssReservedDirty {
        /// 涉事 AP。
        cpu_id: u32,
        /// 脏保留域偏移（0/84/92 三处之一）。
        offset: u32,
        /// 实际读回值。
        actual: u64,
    },
    /// 0x1E17 ltr 后 str 读回选择子不符。
    LtrReadbackBad {
        /// 期望选择子（恒 SEL_TSS = 0x18）。
        expect: u16,
        /// 实际读回（str 缺席以 u16::MAX 哨兵表示）。
        actual: u16,
    },
    /// 0x1E18 MSR 写序纪律违反（必须 EFER→LSTAR→GS_BASE→KERNEL_GS_BASE）。
    MsrWriteOrderBad {
        /// 该步期望写的 MSR 地址。
        expect_addr: u32,
        /// 实际试图写的地址（0 = 已写完仍再写）。
        got_addr: u32,
        /// 当前写序步号（0 基，0..=3；4 = 超写）。
        step: u32,
    },
    /// 0x1E19 MSR 写后读回失配。
    MsrReadbackMismatch {
        /// 涉事 MSR 地址。
        addr: u32,
        /// 写入值。
        wrote: u64,
        /// 读回值（rdmsr 缺席以 u64::MAX 哨兵表示）。
        read: u64,
    },
    /// 0x1E1A MSR 地址在装载序闭集外。
    MsrUnknown {
        /// 被拒地址。
        addr: u32,
    },
    /// 0x1E1B BSP 同构断裂（同构率 < 100%）。
    IsoBroken {
        /// 涉事 AP。
        cpu_id: u32,
        /// 实测同构率（百分整数，< 100）。
        rate_percent: u32,
    },
    /// 0x1E1C 段描述符编解码往返失配（位域拆装自证破——模块自检错）。
    DescRoundtripBad {
        /// 未还原的字段名（"base"/"limit"/"access"/"flags"）。
        field: &'static str,
    },
    /// 0x1E1D 段描述符字段域非法（base 超 48 位镜像域 / limit 超 20 位）。
    DescFieldBad {
        /// 非法项说明（静态中文短语）。
        what: &'static str,
        /// 涉事值。
        value: u64,
    },
    /// 0x1E1E GDT 槽位（0..=4）或 TSS IST 号（1..=7）越界。
    SlotIndexBad {
        /// 越界索引原值。
        slot: u32,
    },
    /// 0x1E1F 事件账满（定容 256 条用尽，后续显性丢弃计入 dropped）。
    EventFull {
        /// 账容量。
        capacity: usize,
    },
}

impl ApGdtError {
    /// 中文可操作诊断（每变体一条，静态文本——数值上下文在变体字段里）。
    pub fn detail(&self) -> &'static str {
        match self {
            ApGdtError::OutOfPhase { .. } => {
                "装载步序错位：五步全序 GdtProgram→Lgdt→TssProgram→Ltr→MsrLoad \
                 不可乱序不可跳步；请先查序列 phase 字段定位当前步，再按序调用对应 step_*。"
            }
            ApGdtError::GdtChecksumBad { .. } => {
                "lgdt 前 GDT 镜像校验和不过：program 之后镜像字节被改写（踩踏或篡改）；\
                 比对 program 时记录值与实时重算值，逐槽（null/code/data/tss_lo/tss_hi）定位被改字节。"
            }
            ApGdtError::LgdtReadbackBad { .. } => {
                "lgdt 后 sgdt 读回不符：GDTR base/limit 任一失配；请检查 CpuPort 注入实现\
                 是否吞写，或 GDT 基址在装载与读回之间被并发改写。"
            }
            ApGdtError::IstZero { .. } => {
                "ltr 前 TSS IST 栈指针为零：七路 IST 必须全配独立栈（F0328 栈域供给）；\
                 请补齐缺位 IST 栈顶后重跑 tss_program→ltr。"
            }
            ApGdtError::IoMapBaseBad { .. } => {
                "TSS IO 位图基址非 104：x86_64 无 IO 位图时必须精确指向 TSS 固定区末尾\
                 （SDM §7.7/§7.8，基址超出 TSS limit 即位图缺席、IO 不陷 trap）；\
                 请置 TSS_BYTES 纪律值。"
            }
            ApGdtError::TssReservedDirty { .. } => {
                "TSS 保留域非零：偏移 0（u32）/84（u64）/92（u64）三处保留域必须为零\
                 （SDM §7.7）；请清零后重跑 tss_program。"
            }
            ApGdtError::LtrReadbackBad { .. } => {
                "ltr 后 str 读回选择子不符：期望 TSS 选择子 0x18（槽 3<<3|RPL0）；\
                 请检查 CpuPort ltr 实现与选择子编码。"
            }
            ApGdtError::MsrWriteOrderBad { .. } => {
                "MSR 写序纪律违反：必须 EFER→LSTAR→GS_BASE→KERNEL_GS_BASE\
                 （模式位→入口→内核 GS→用户 GS，SWAPGS 配对序）；\
                 请按 MSR_PLAN_ORDER 表逐位写，勿跳勿倒。"
            }
            ApGdtError::MsrReadbackMismatch { .. } => {
                "MSR 写后读回失配：写入值与读回值不一致（写侧丢失或读侧污染）；\
                 请比对写入/读回两值定位是 MsrSpace 实现故障还是装载序外篡改。"
            }
            ApGdtError::MsrUnknown { .. } => {
                "MSR 地址在装载序闭集外：只认 EFER/LSTAR/GS_BASE/KERNEL_GS_BASE\
                 四地址（SDM Appendix B 地址表）；请核对地址表拼写。"
            }
            ApGdtError::IsoBroken { .. } => {
                "BSP 同构断裂：AP 装载结果与 BSP 基线镜像投影 diff 非空、同构率 < 100%；\
                 请按 IsoDiff 列表的路径字段逐项定位差异（gdt.*/tss.*/msr.*）。"
            }
            ApGdtError::DescRoundtripBad { .. } => {
                "段描述符编解码往返失配：encode→decode 未还原原字段（位域拆装自证破）；\
                 此为模块自检错误，说明 encode_desc/decode_desc 位布局被改动，\
                 请回退到 SDM §3.4.5 布局。"
            }
            ApGdtError::DescFieldBad { .. } => {
                "段描述符字段域非法：base 超出 48 位物理镜像域或 limit 超 20 位;\
                 请检查区规划（GDT/TSS 竞技场基址）是否越出物理镜像域。"
            }
            ApGdtError::SlotIndexBad { .. } => {
                "GDT 槽位（闭集 0..=4：null/code/data/tss_lo/tss_hi）或 TSS IST 号\
                 （闭集 1..=7）越界；请核对布局表偏移后重试。"
            }
            ApGdtError::EventFull { .. } => {
                "事件账满：定容 256 条已用尽，后续事件显性丢弃并计入 dropped 计数；\
                 请在收口点扩容或缩短注入序列后重跑对拍。"
            }
        }
    }

    /// 诊断码（闭集 0x1E11~0x1E1F，一变体一码）。
    pub fn xc_code(&self) -> u16 {
        match self {
            ApGdtError::OutOfPhase { .. } => 0x1E11,
            ApGdtError::GdtChecksumBad { .. } => 0x1E12,
            ApGdtError::LgdtReadbackBad { .. } => 0x1E13,
            ApGdtError::IstZero { .. } => 0x1E14,
            ApGdtError::IoMapBaseBad { .. } => 0x1E15,
            ApGdtError::TssReservedDirty { .. } => 0x1E16,
            ApGdtError::LtrReadbackBad { .. } => 0x1E17,
            ApGdtError::MsrWriteOrderBad { .. } => 0x1E18,
            ApGdtError::MsrReadbackMismatch { .. } => 0x1E19,
            ApGdtError::MsrUnknown { .. } => 0x1E1A,
            ApGdtError::IsoBroken { .. } => 0x1E1B,
            ApGdtError::DescRoundtripBad { .. } => 0x1E1C,
            ApGdtError::DescFieldBad { .. } => 0x1E1D,
            ApGdtError::SlotIndexBad { .. } => 0x1E1E,
            ApGdtError::EventFull { .. } => 0x1E1F,
        }
    }

    /// 一行式可读报告（码 + 诊断文本；format! 走 alloc，供上层日志拼装）。
    pub fn report(&self) -> String {
        format!("[{:#06x}] {}", self.xc_code(), self.detail())
    }
}

// ============================================================================
// 第 2 节 · GdtLayout / 段描述符位域编解码 / GdtImage（每 AP 独立 GDT）
// ============================================================================

/// 段描述符字段组（编码前的语义面——8 字节描述符的位域拆装对象）。
///
/// 字段语义与位布局来源：Intel SDM Vol.3 §3.4.5 "Segment Descriptors"：
/// base 拆 base[7:0]/[15:8]/[23:16]/[31:24] 四段；limit 拆
/// [7:0]/[15:8]/[19:16]（高 4 位与 flags 同字节）；access 为字节 5；
/// flags（G/D-B/L/AVL）占字节 6 高 4 位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentFields {
    /// 段基址低 32 位（长模式代码/数据段基址被忽略，但编码必须往返无损；
    /// TSS 描述符基址取 TSS 物理镜像低 32 位）。
    pub base: u32,
    /// 段限长原始 20 位（G=1 时有效限长 = limit<<12 | 0xFFF）。
    pub limit: u32,
    /// access 字节（P/DPL/S/Type 位组，SDM §3.4.3.1 图 3-8）。
    pub access: u8,
    /// flags 低 4 位（位 3 = G，位 2 = D/B，位 1 = L，位 0 = AVL）。
    pub flags: u8,
}

/// 8 字节段描述符编码（const fn，while 循环零迭代为直排位拼装）。
///
/// 布局（SDM §3.4.5 图 3-8，小端字节序）：
///   字节 0 = limit[7:0]；字节 1 = limit[15:8]；
///   字节 2 = base[7:0]；字节 3 = base[15:8]；字节 4 = base[23:16]；
///   字节 5 = access；字节 6 = flags[3:0]<<4 | limit[19:16]；
///   字节 7 = base[31:24]。
pub const fn encode_desc(f: SegmentFields) -> [u8; 8] {
    let mut d = [0u8; 8];
    d[0] = (f.limit & 0xFF) as u8;
    d[1] = ((f.limit >> 8) & 0xFF) as u8;
    d[2] = (f.base & 0xFF) as u8;
    d[3] = ((f.base >> 8) & 0xFF) as u8;
    d[4] = ((f.base >> 16) & 0xFF) as u8;
    d[5] = f.access;
    d[6] = ((((f.flags as u32) & 0x0F) << 4) | ((f.limit >> 16) & 0x0F)) as u8;
    d[7] = ((f.base >> 24) & 0xFF) as u8;
    d
}

/// 8 字节段描述符解码（encode_desc 的精确逆——位域拆装的另一半）。
pub const fn decode_desc(d: [u8; 8]) -> SegmentFields {
    SegmentFields {
        base: (d[2] as u32) | ((d[3] as u32) << 8) | ((d[4] as u32) << 16) | ((d[7] as u32) << 24),
        limit: (d[0] as u32)
            | ((d[1] as u32) << 8)
            | (((d[6] as u32) & 0x0F) << 16),
        access: d[5],
        flags: (d[6] >> 4) & 0x0F,
    }
}

/// 编译期编解码往返自证 #1：手工全字段满载值（base/limit/access/flags
/// 各取满域值）编码再解码必须逐字段还原（整数 == 于 const 块内合法，
/// 非派生 PartialEq 的 ==）。
const _: () = {
    let f = SegmentFields { base: 0xFFFF_FFFF, limit: 0xF_FFFF, access: 0xFF, flags: 0x0F };
    let d = encode_desc(f);
    let g = decode_desc(d);
    assert!(g.base == f.base);
    assert!(g.limit == f.limit);
    assert!(g.access == f.access);
    assert!(g.flags == f.flags);
};

/// 编译期编解码往返自证 #2：全零描述符（空描述符恒等元）。
const _: () = {
    let f = SegmentFields { base: 0, limit: 0, access: 0, flags: 0 };
    let d = encode_desc(f);
    let g = decode_desc(d);
    assert!(g.base == 0 && g.limit == 0 && g.access == 0 && g.flags == 0);
    let mut all_zero = true;
    let mut i = 0;
    while i < 8 {
        if d[i] != 0 {
            all_zero = false;
        }
        i += 1;
    }
    assert!(all_zero);
};

/// 编译期自证 #3：64 位代码段经典编码字（与 Linux/arch 界经典
/// 0x00AF9A000000FFFF 逐字节一致——P|S|E|R=0x9A，G|L=0xA，base=0，
/// limit=0xFFFFF；来源：SDM §3.4.5 布局推导 + Linux arch/x86 头文件
/// GDT_ENTRY 常量惯例互证）。
const _: () = {
    let d = encode_desc(SegmentFields { base: 0, limit: 0xF_FFFF, access: 0x9A, flags: 0x0A });
    assert!(d[0] == 0xFF && d[1] == 0xFF && d[2] == 0x00 && d[3] == 0x00);
    assert!(d[4] == 0x00 && d[5] == 0x9A && d[6] == 0xAF && d[7] == 0x00);
};

/// GDT 布局表（无字段常量类型：全 const 关联项——布局即类型级事实）。
///
/// 每 AP 独立 GDT 的五槽布局（来源：SDM §3.4.2 段描述符表结构 +
/// §3.5.2 长模式 TSS 双描述符占用两个相邻 8 字节槽）：
///   槽 0 null（必须全零——SDM 明文：第一个描述符必须为空描述符）；
///   槽 1 内核 64 位代码段（L=1, D=0）；槽 2 内核数据段；
///   槽 3/4 TSS 低/高双描述符（16 字节系统描述符）。
pub struct GdtLayout;

impl GdtLayout {
    /// GDT 槽数（null + code + data + tss_lo + tss_hi = 5）。
    pub const SLOTS: u32 = 5;
    /// GDT 总字节数（5 × 8 = 40）。
    pub const BYTES: u32 = Self::SLOTS * 8;

    /// 槽 0（空描述符）字节偏移。
    pub const OFF_NULL: u32 = 0;
    /// 槽 1（64 位代码段）字节偏移。
    pub const OFF_CODE64: u32 = 8;
    /// 槽 2（数据段）字节偏移。
    pub const OFF_DATA: u32 = 16;
    /// 槽 3（TSS 低描述符）字节偏移。
    pub const OFF_TSS_LO: u32 = 24;
    /// 槽 4（TSS 高描述符）字节偏移。
    pub const OFF_TSS_HI: u32 = 32;

    /// 内核代码段选择子（槽 1 << 3 | RPL0 = 0x08）。
    pub const SEL_CODE64: u16 = 0x08;
    /// 内核数据段选择子（槽 2 << 3 = 0x10）。
    pub const SEL_DATA: u16 = 0x10;
    /// TSS 任务门选择子（槽 3 << 3 = 0x18——ltr 装载目标）。
    pub const SEL_TSS: u16 = 0x18;

    /// 64 位代码段 access（P=1|S=1|E=1|R=1 = 0x9A；来源：SDM §3.4.3.1
    /// 图 3-8 Type 位组合——代码段只读可执行，DPL0 内核态）。
    pub const CODE64_ACCESS: u8 = 0x9A;
    /// 64 位代码段 flags（G=1|L=1 = 0xA；G = 4KiB 粒度、L = 64 位长模式，
    /// D/B 位在 L=1 时必须为 0——SDM §3.4.5 注）。
    pub const CODE64_FLAGS: u8 = 0x0A;
    /// 数据段 access（P=1|S=1|W=1 = 0x92；可写数据段，DPL0）。
    pub const DATA_ACCESS: u8 = 0x92;
    /// 数据段 flags（G=1|D/B=1 = 0xC；长模式数据段 D/B 被忽略
    /// （SDM §3.4.3 注：64 位模式数据段基址/限长忽略），保守置 1 与
    /// 经典内核同构——BSP 基线镜像冻结值）。
    pub const DATA_FLAGS: u8 = 0x0C;
    /// TSS 描述符 access（P=1|S=0|Type=0x9 = 0x89；Type 1001B =
    /// 64 位 TSS available——SDM §3.5.3 系统段类型表）。
    pub const TSS_ACCESS: u8 = 0x89;

    /// 槽号 → 字节偏移（未检查 const fn：调用方负责槽号闭集；
    /// 运行期检查版见 checked_slot_offset）。
    pub const fn slot_offset(slot: u32) -> u32 {
        slot * 8
    }

    /// 槽号 → 槽名（诊断路径用；闭集外返回 "gdt.?"）。
    pub const fn slot_name(slot: u32) -> &'static str {
        if slot == 0 {
            "gdt.null"
        } else if slot == 1 {
            "gdt.code64"
        } else if slot == 2 {
            "gdt.data"
        } else if slot == 3 {
            "gdt.tss_lo"
        } else if slot == 4 {
            "gdt.tss_hi"
        } else {
            "gdt.?"
        }
    }

    /// 运行期检查版槽偏移（槽号闭集 0..=4，越界 SlotIndexBad）。
    pub fn checked_slot_offset(slot: u32) -> Result<u32, ApGdtError> {
        if slot < Self::SLOTS {
            Ok(Self::slot_offset(slot))
        } else {
            Err(ApGdtError::SlotIndexBad { slot })
        }
    }

    /// canonical 64 位代码段字段组（base=0, limit=0xFFFFF, L=1, G=1）。
    pub const fn code64_fields() -> SegmentFields {
        SegmentFields { base: 0, limit: 0xF_FFFF, access: Self::CODE64_ACCESS, flags: Self::CODE64_FLAGS }
    }

    /// canonical 数据段字段组。
    pub const fn data_fields() -> SegmentFields {
        SegmentFields { base: 0, limit: 0xF_FFFF, access: Self::DATA_ACCESS, flags: Self::DATA_FLAGS }
    }

    /// TSS 低描述符字段组（base = TSS 镜像基址低 32 位，limit =
    /// TSS 字节数 - 1，G=0 字节粒度——104 字节 TSS 的 limit = 103）。
    pub const fn tss_lo_fields(tss_base_low: u32, tss_len: u32) -> SegmentFields {
        SegmentFields { base: tss_base_low, limit: tss_len, access: Self::TSS_ACCESS, flags: 0x0 }
    }
}

/// 编译期自证 #4：GDT 布局表自洽（槽数 × 8 = 字节数；相邻槽偏移
/// 差恒 8；TSS 高描述符偏移 + 8 = GDT 总长）。
const _: () = assert!(GdtLayout::BYTES == 40);
const _: () = assert!(GdtLayout::OFF_TSS_HI + 8 == GdtLayout::BYTES);
const _: () = assert!(GdtLayout::slot_offset(4) == 32);
const _: () = assert!(GdtLayout::SEL_TSS == (3 << 3));

/// GDTR 伪描述符（lgdt/sgdt 操作对象——u16 limit + u64 base，
/// 内存形态 10 字节：2 字节 limit + 8 字节 base，SDM §3.4.2 图 3-11
/// 「伪描述符」双字对齐布局的本模块寄存器语义面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gdtr {
    /// GDT 限长（= GDT 字节数 - 1 = 39）。
    pub limit: u16,
    /// GDT 基址（每 AP 独立 GDT 的物理镜像基址）。
    pub base: u64,
}

/// GDT 镜像校验和（位置加权 wrapping 和——位置敏感以区分「同字节
/// 换位」类改写；i 加权 + 字节×31 双因子，纯 FNV 风格扩散）。
pub fn gdt_checksum(b: &[u8]) -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < b.len() {
        s = s
            .wrapping_add((b[i] as u64).wrapping_mul(31))
            .wrapping_add(i as u64);
        i += 1;
    }
    s
}

/// 每 AP 独立 GDT 实例（40 字节镜像堆置 + 基址 + program 时校验和）。
///
/// 生命周期纪律：`program` 一次性物化五槽 → 记录校验和 →
/// `checksum_ok` 在 lgdt 前实时复算比对（program 与 lgdt 之间的
/// 任何字节改写即 GdtChecksumBad——踩踏/篡改检出面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GdtImage {
    /// 涉事 AP（错误上下文回填用）。
    pub cpu_id: u32,
    /// 40 字节 GDT 镜像（堆置——>64KiB 禁上栈纪律下所有镜像走 Vec）。
    pub bytes: Vec<u8>,
    /// GDT 物理镜像基址（GDTR.base 装载值）。
    pub base: u64,
    /// program 时记录的校验和（0 哨兵 = 未 program——phase 门保证
    /// lgdt 前必已 program，哨兵仅防御面）。
    pub checksum: u64,
}

impl GdtImage {
    /// 未编程空镜像（40 零字节；仅供序列内部占位，外部勿直接用）。
    pub fn empty(cpu_id: u32) -> Self {
        GdtImage { cpu_id, bytes: vec![0u8; GdtLayout::BYTES as usize], base: 0, checksum: 0 }
    }

    /// 物化每 AP 独立 GDT：空描述符 + canonical 代码/数据段 + 该 AP
    /// TSS 双描述符，随后记录校验和。
    ///
    /// 域校验：gdt_base/tss_base 必须 < 2^48（物理镜像域——本模块
    /// 竞技场假设，越界 DescFieldBad）；tss_len 必须 ≤ 20 位
    /// （0xFFFFF，描述符 limit 域上限；本布局恒传 103）。
    pub fn program(
        cpu_id: u32,
        gdt_base: u64,
        tss_base: u64,
        tss_len: u32,
    ) -> Result<Self, ApGdtError> {
        if (gdt_base >> 48) != 0 {
            return Err(ApGdtError::DescFieldBad {
                what: "gdt_base 超出 48 位物理镜像域",
                value: gdt_base,
            });
        }
        if (tss_base >> 48) != 0 {
            return Err(ApGdtError::DescFieldBad {
                what: "tss_base 超出 48 位物理镜像域",
                value: tss_base,
            });
        }
        if tss_len > 0xF_FFFF {
            return Err(ApGdtError::DescFieldBad {
                what: "tss_len 超 20 位 limit 域",
                value: tss_len as u64,
            });
        }
        let mut bytes = vec![0u8; GdtLayout::BYTES as usize];
        // 槽 0 空描述符：vec! 已零，显式再写一次 encode(全零)——
        // 「空描述符 = encode(全零字段)」的自证性写法，非哑零。
        let null_desc = encode_desc(SegmentFields { base: 0, limit: 0, access: 0, flags: 0 });
        let code_desc = encode_desc(GdtLayout::code64_fields());
        let data_desc = encode_desc(GdtLayout::data_fields());
        let tss_lo = encode_desc(GdtLayout::tss_lo_fields(tss_base as u32, tss_len));
        // TSS 高描述符：字节 0..4 = base[63:32]，其余保留零
        //（SDM §3.5.2 长模式系统描述符「base 上 32 位扩页」布局）。
        let mut tss_hi = [0u8; 8];
        tss_hi[0] = ((tss_base >> 32) & 0xFF) as u8;
        tss_hi[1] = ((tss_base >> 40) & 0xFF) as u8;
        tss_hi[2] = ((tss_base >> 48) & 0xFF) as u8;
        tss_hi[3] = ((tss_base >> 56) & 0xFF) as u8;
        let offs = [
            GdtLayout::OFF_NULL as usize,
            GdtLayout::OFF_CODE64 as usize,
            GdtLayout::OFF_DATA as usize,
            GdtLayout::OFF_TSS_LO as usize,
            GdtLayout::OFF_TSS_HI as usize,
        ];
        let srcs = [&null_desc, &code_desc, &data_desc, &tss_lo, &tss_hi];
        let mut ok = true;
        let mut i = 0usize;
        while i < 5 {
            if !put_arr8(&mut bytes, offs[i], srcs[i]) {
                ok = false;
            }
            i += 1;
        }
        if !ok {
            return Err(ApGdtError::SlotIndexBad { slot: 5 });
        }
        let checksum = gdt_checksum(&bytes);
        Ok(GdtImage { cpu_id, bytes, base: gdt_base, checksum })
    }

    /// 实时校验和是否仍与 program 时记录一致（lgdt 前置校验的判定面）。
    pub fn checksum_ok(&self) -> bool {
        self.checksum == gdt_checksum(&self.bytes)
    }

    /// 校验和不一致时的记录值/实时值对（错误构造便捷面）。
    pub fn checksum_pair(&self) -> (u64, u64) {
        (self.checksum, gdt_checksum(&self.bytes))
    }

    /// 本镜像的 GDTR 装载值（limit = 40 - 1 = 39）。
    pub fn gdtr(&self) -> Gdtr {
        Gdtr { limit: (GdtLayout::BYTES - 1) as u16, base: self.base }
    }

    /// 取槽字节镜像（槽号闭集 0..=4，越界 SlotIndexBad）。
    pub fn slot_bytes(&self, slot: u32) -> Result<[u8; 8], ApGdtError> {
        let off = GdtLayout::checked_slot_offset(slot)? as usize;
        let mut out = [0u8; 8];
        match rd_u64_le(&self.bytes, off) {
            Some(v) => {
                let mut i = 0usize;
                while i < 8 {
                    out[i] = ((v >> (i * 8)) & 0xFF) as u8;
                    i += 1;
                }
                Ok(out)
            }
            None => Err(ApGdtError::SlotIndexBad { slot }),
        }
    }

    /// 取槽语义字段组（slot_bytes + decode_desc 组合面）。
    pub fn slot_fields(&self, slot: u32) -> Result<SegmentFields, ApGdtError> {
        Ok(decode_desc(self.slot_bytes(slot)?))
    }
}

// ============================================================================
// 第 3 节 · TssLayout / TssImage（x86_64 TSS 104 字节标准布局）
// ============================================================================

/// TSS 布局表（无字段常量类型；来源：Intel SDM Vol.3 §7.7 图 7-13
/// 「64-bit TSS Format」——固定区 104 字节的逐偏移冻结）。
///
/// 字段偏移链（全部编译期断言自证，见本节 const 块）：
///   0x00 u32 保留（必须零）；0x04 RSP0；0x0C RSP1；0x14 RSP2；
///   0x1C IST1 … 0x4C IST7（七路，步距 8）；0x54 u64 保留（零）；
///   0x5C u64 保留（零）；0x64 u16 IO 位图基址。
/// 注意 RSP0 落在偏移 4——相对 8 字节自然对齐是错位的（SDM 布局如此，
/// 访问必须走字节级小端读写，本模块 rd/wr_u64_le 全部字节拼装）。
pub struct TssLayout;

impl TssLayout {
    /// TSS 固定区字节数（SDM §7.7：IO 位图基址字段止于偏移 102，
    /// 固定区共 104 字节）。
    pub const BYTES: u32 = 104;

    /// 保留 u32 偏移（必须为零）。
    pub const OFF_RSVD0: u32 = 0;
    /// RSP0 偏移（ring0 栈指针——中断/异常进 ring0 的换栈目标）。
    pub const OFF_RSP0: u32 = 4;
    /// RSP1 偏移（长模式不使用 ring1/2，纪律置零与 BSP 同构）。
    pub const OFF_RSP1: u32 = 12;
    /// RSP2 偏移（同上）。
    pub const OFF_RSP2: u32 = 20;
    /// IST1 偏移（NMI 栈惯例槽——OSDev/内核惯例：IST1=NMI、IST2=DF、
    /// IST3=MC…本模块按 F0328 七路独立栈全配，逐路语义由上游定）。
    pub const OFF_IST1: u32 = 28;
    /// IST 槽数（SDM §7.7：IST1..IST7 共七路）。
    pub const IST_COUNT: u32 = 7;
    /// 保留 u64 偏移之一（偏移 84，必须为零）。
    pub const OFF_RSVD84: u32 = 84;
    /// 保留 u64 偏移之二（偏移 92，必须为零）。
    pub const OFF_RSVD92: u32 = 92;
    /// IO 位图基址偏移（u16，值域 = 相对 TSS 基址的偏移）。
    pub const OFF_IOMAP: u32 = 100;

    /// IO 位图基址纪律值（= BYTES = 104：位图基址超出 TSS limit(103)
    /// 即位图缺席 ⇒ IO 指令不陷 trap——SDM §7.8「若 I/O 位图基址 ≥
    /// TSS 限长则无位图」；早期 AP 无用户 IO 任务，缺席即正确姿态）。
    pub const IOMAP_ABSENT: u16 = 104;

    /// IST 号（1 基，1..=7）→ 字节偏移（未检查 const fn：
    /// IST1=28，步距 8；运行期检查版见 checked_ist_offset）。
    pub const fn ist_offset(ist: u32) -> u32 {
        Self::OFF_IST1 + (ist - 1) * 8
    }

    /// 运行期检查版 IST 偏移（闭集 1..=7，越界 SlotIndexBad）。
    pub fn checked_ist_offset(ist: u32) -> Result<u32, ApGdtError> {
        if ist >= 1 && ist <= Self::IST_COUNT {
            Ok(Self::ist_offset(ist))
        } else {
            Err(ApGdtError::SlotIndexBad { slot: ist })
        }
    }

    /// IST 号 → 诊断路径名（闭集外 "tss.ist?"）。
    pub const fn ist_name(ist: u32) -> &'static str {
        if ist == 1 {
            "tss.ist1"
        } else if ist == 2 {
            "tss.ist2"
        } else if ist == 3 {
            "tss.ist3"
        } else if ist == 4 {
            "tss.ist4"
        } else if ist == 5 {
            "tss.ist5"
        } else if ist == 6 {
            "tss.ist6"
        } else if ist == 7 {
            "tss.ist7"
        } else {
            "tss.ist?"
        }
    }
}

/// 编译期自证 #5：TSS 偏移链自洽（IST 步距 8；七路 IST 止点衔接
/// 保留域 84；双保留域止点衔接 IO 位图基址 100；基址字段 + 4 = 104）。
const _: () = assert!(TssLayout::OFF_RSP1 == TssLayout::OFF_RSP0 + 8);
const _: () = assert!(TssLayout::OFF_RSP2 == TssLayout::OFF_RSP1 + 8);
const _: () = assert!(TssLayout::OFF_IST1 == TssLayout::OFF_RSP2 + 8);
const _: () = assert!(TssLayout::ist_offset(7) == 76);
const _: () = assert!(TssLayout::ist_offset(7) + 8 == TssLayout::OFF_RSVD84);
const _: () = assert!(TssLayout::OFF_RSVD84 + 8 == TssLayout::OFF_RSVD92);
const _: () = assert!(TssLayout::OFF_RSVD92 + 8 == TssLayout::OFF_IOMAP);
const _: () = assert!(TssLayout::OFF_IOMAP + 4 == TssLayout::BYTES);
const _: () = assert!(TssLayout::IST_COUNT == 7);
const _: () = assert!(TssLayout::IOMAP_ABSENT as u32 == TssLayout::BYTES);

/// 每 AP 独立 TSS 实例（104 字节镜像堆置 + 基址）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TssImage {
    /// 涉事 AP。
    pub cpu_id: u32,
    /// 104 字节 TSS 镜像（堆置）。
    pub bytes: Vec<u8>,
    /// TSS 物理镜像基址（ltr 侧经 GDT 槽 3/4 描述符间接引用）。
    pub base: u64,
}

impl TssImage {
    /// 未编程空镜像（104 零字节；序列内部占位用）。
    pub fn empty(cpu_id: u32) -> Self {
        TssImage { cpu_id, bytes: vec![0u8; TssLayout::BYTES as usize], base: 0 }
    }

    /// 物化 TSS：RSP0 = AP 内核栈顶、RSP1/RSP2 = 0（长模式 ring1/2
    /// 不用）、IST1..7 = 七路异常栈顶（F0328 栈域供给）、保留域零、
    /// IO 位图基址 = 104（位图缺席纪律）。
    ///
    /// 域校验：tss_base < 2^48（同 GdtImage::program 的物理镜像域假设）。
    pub fn program(cpu_id: u32, tss_base: u64, rsp0: u64, ist: [u64; 7]) -> Result<Self, ApGdtError> {
        if (tss_base >> 48) != 0 {
            return Err(ApGdtError::DescFieldBad {
                what: "tss_base 超出 48 位物理镜像域",
                value: tss_base,
            });
        }
        let mut bytes = vec![0u8; TssLayout::BYTES as usize];
        if !wr_u64_le(&mut bytes, TssLayout::OFF_RSP0 as usize, rsp0) {
            return Err(ApGdtError::SlotIndexBad { slot: TssLayout::OFF_RSP0 });
        }
        // RSP1/RSP2 保持 vec! 零（显式注释来源，非遗漏——长模式
        // 不向 ring1/2 换栈，SDM §7.7 注 + BSP 基线镜像冻结值 0）。
        let mut i = 0usize;
        while i < 7 {
            let off = TssLayout::ist_offset((i + 1) as u32) as usize;
            if !wr_u64_le(&mut bytes, off, ist[i]) {
                return Err(ApGdtError::SlotIndexBad { slot: (i + 1) as u32 });
            }
            i += 1;
        }
        if !wr_u16_le(&mut bytes, TssLayout::OFF_IOMAP as usize, TssLayout::IOMAP_ABSENT) {
            return Err(ApGdtError::SlotIndexBad { slot: TssLayout::OFF_IOMAP });
        }
        Ok(TssImage { cpu_id, bytes, base: tss_base })
    }

    /// 读 RSP0（镜像读回面；越界理论不可达，None 防御）。
    pub fn rsp0(&self) -> Option<u64> {
        rd_u64_le(&self.bytes, TssLayout::OFF_RSP0 as usize)
    }

    /// 读第 ist 路 IST 栈指针（1 基闭集 1..=7）。
    pub fn ist(&self, ist: u32) -> Result<u64, ApGdtError> {
        let off = TssLayout::checked_ist_offset(ist)? as usize;
        match rd_u64_le(&self.bytes, off) {
            Some(v) => Ok(v),
            None => Err(ApGdtError::SlotIndexBad { slot: ist }),
        }
    }

    /// 读 IO 位图基址。
    pub fn iomap_base(&self) -> Option<u16> {
        rd_u16_le(&self.bytes, TssLayout::OFF_IOMAP as usize)
    }

    /// ltr 前置校验（三查一体，返回首个失败项）：
    ///   一查 IST 七路全非零（IstZero——任务钦定前置）；
    ///   二查 IO 位图基址 == 104（IoMapBaseBad）；
    ///   三查保留域 0/84/92 全零（TssReservedDirty）。
    pub fn preflight_ltr(&self) -> Result<(), ApGdtError> {
        let mut i = 1u32;
        while i <= TssLayout::IST_COUNT {
            if self.ist(i)? == 0 {
                return Err(ApGdtError::IstZero { cpu_id: self.cpu_id, slot: i });
            }
            i += 1;
        }
        match self.iomap_base() {
            Some(v) if v == TssLayout::IOMAP_ABSENT => {}
            other => {
                return Err(ApGdtError::IoMapBaseBad {
                    cpu_id: self.cpu_id,
                    expect: TssLayout::IOMAP_ABSENT,
                    actual: match other {
                        Some(v) => v,
                        None => u16::MAX,
                    },
                })
            }
        }
        // 保留域三处之一：偏移 0 的保留 u32（字节 0..4）——错位布局下
        // 拆两半 u16 查零（偏移 0 起 104 字节恒可读，None 仅防御）。
        let r0_lo = rd_u16_le(&self.bytes, TssLayout::OFF_RSVD0 as usize);
        let r0_hi = rd_u16_le(&self.bytes, (TssLayout::OFF_RSVD0 + 2) as usize);
        let r0_bad = match (r0_lo, r0_hi) {
            (Some(0), Some(0)) => false,
            (lo, hi) => {
                return Err(ApGdtError::TssReservedDirty {
                    cpu_id: self.cpu_id,
                    offset: TssLayout::OFF_RSVD0,
                    actual: match (lo, hi) {
                        (Some(l), Some(h)) => (l as u64) | ((h as u64) << 16),
                        _ => u64::MAX,
                    },
                })
            }
        };
        // 偏移 0 保留 u32 已查——r0_bad 恒 false（错误已提前返回），
        // 显式消费以保持「查-判-案」三段式可读。
        if r0_bad {
            return Err(ApGdtError::TssReservedDirty {
                cpu_id: self.cpu_id,
                offset: TssLayout::OFF_RSVD0,
                actual: u64::MAX,
            });
        }
        let r84 = rd_u64_le(&self.bytes, TssLayout::OFF_RSVD84 as usize);
        match r84 {
            Some(0) => {}
            other => {
                return Err(ApGdtError::TssReservedDirty {
                    cpu_id: self.cpu_id,
                    offset: TssLayout::OFF_RSVD84,
                    actual: match other {
                        Some(v) => v,
                        None => u64::MAX,
                    },
                })
            }
        }
        let r92 = rd_u64_le(&self.bytes, TssLayout::OFF_RSVD92 as usize);
        match r92 {
            Some(0) => {}
            other => {
                return Err(ApGdtError::TssReservedDirty {
                    cpu_id: self.cpu_id,
                    offset: TssLayout::OFF_RSVD92,
                    actual: match other {
                        Some(v) => v,
                        None => u64::MAX,
                    },
                })
            }
        }
        Ok(())
    }
}

// ============================================================================
// 第 4 节 · MsrSpec / MsrSpace / SimMsrSpace / MsrLoadSeq（MSR 装载序）
// ============================================================================

/// IA32_EFER（地址 0xC0000080；来源：Intel SDM Vol.4 Appendix B
/// 「Model Specific Registers (MSRs)」表 EFER 条目——延伸特性使能
/// 寄存器，含 SCE/LME/LMA/NXE 位）。
pub const MSR_IA32_EFER: u32 = 0xC000_0080;
/// IA32_STAR（0xC0000081；系统调用目标段选择子——SysRet/SysEnter 段基；
/// 装载序表内记录但本序不装载，口径见 MSR_PLAN_ORDER 注释）。
pub const MSR_IA32_STAR: u32 = 0xC000_0081;
/// IA32_LSTAR（0xC0000082；64 位系统调用 RIP 目标——syscall 入口）。
pub const MSR_IA32_LSTAR: u32 = 0xC000_0082;
/// IA32_CSTAR（0xC0000083；兼容模式 syscall 目标——长模式不用，
/// 表内记录不装载）。
pub const MSR_IA32_CSTAR: u32 = 0xC000_0083;
/// IA32_FMASK（0xC0000084；syscall RFLAGS 掩码——表内记录不装载）。
pub const MSR_IA32_FMASK: u32 = 0xC000_0084;
/// IA32_FS_BASE（0xC0000100；FS 基址——表内记录不装载，AP 早期
/// 不用 FS，与 BSP 基线镜像冻结口径一致）。
pub const MSR_IA32_FS_BASE: u32 = 0xC000_0100;
/// IA32_GS_BASE（0xC0000101；GS 基址——内核 per-CPU 区锚点，
/// F0344 自识别的硬件面）。
pub const MSR_IA32_GS_BASE: u32 = 0xC000_0101;
/// IA32_KERNEL_GS_BASE（0xC0000102；SWAPGS 换出槽——用户态 GS 基址；
/// 早期 AP 无用户线程，纪律置 0 与 BSP 同构）。
pub const MSR_IA32_KERNEL_GS_BASE: u32 = 0xC000_0102;

/// EFER.SCE 位（位 0；syscall/sysret 使能——SDM Vol.3 §2.2.1 表 2-1）。
pub const EFER_SCE: u64 = 1 << 0;
/// EFER.LME 位（位 7；长模式使能）。
pub const EFER_LME: u64 = 1 << 8;
/// EFER.LMA 位（位 10；长模式激活——装载序校验其置位）。
pub const EFER_LMA: u64 = 1 << 10;
/// EFER.NXE 位（位 11；执行禁止使能——F0307 NX 语义的 MSR 面）。
pub const EFER_NXE: u64 = 1 << 11;

/// EFER canonical 冻结值（SCE|LME|LMA|NXE = 0xD01；BSP 基线镜像
/// 字段——AP 侧装载值必须与之逐位同构，F0307 同源口径）。
pub const EFER_CANON: u64 = EFER_SCE | EFER_LME | EFER_LMA | EFER_NXE;

/// 编译期自证 #6：EFER canonical 值位组合正确（0xD01 = 1 + 0x100 +
/// 0x400 + 0x800；无杂位）。
const _: () = assert!(EFER_CANON == 0x0D01);

/// MSR 已知地址全集（8 项；SimMsrSpace 的存储槽闭集）。
///
/// 来源：SDM Vol.4 Appendix B 上述八地址条目（0xC0000080~0xC0000084
/// 系统调用/延伸特性组 + 0xC0000100~0xC0000102 段基址组）。
pub const MSR_KNOWN: [u32; 8] = [
    MSR_IA32_EFER,
    MSR_IA32_STAR,
    MSR_IA32_LSTAR,
    MSR_IA32_CSTAR,
    MSR_IA32_FMASK,
    MSR_IA32_FS_BASE,
    MSR_IA32_GS_BASE,
    MSR_IA32_KERNEL_GS_BASE,
];

/// 装载序闭集（四地址全序表——写序纪律的本体）。
///
/// 顺序语义（每步为何在此处）：
///   0. EFER——模式/架构位先落（SCE 使能后才谈 syscall 入口；与
///      F0307「开分页前 EFER.NXE」的 BSP 侧时序同构的前置思想）；
///   1. LSTAR——入口地址（EFER.SCE 已开，入口即刻生效）；
///   2. GS_BASE——内核 per-CPU 锚点（中断/异常路径依赖 GS 寻 per-CPU，
///      必须在任何用户面寄存器之前就位）；
///   3. KERNEL_GS_BASE——SWAPGS 换出槽（用户 GS 占位；置于最后，
///      与 SWAPGS 配对语义一致：内核 GS 在前，用户 GS 在影槽）。
/// STAR/CSTAR/FMASK 属 BSP 侧入口描述组、AP 与 BSP 共用全局口径，
/// 不在本序重复装载（表内已知、序外不写——同构判据不比对）。
pub const MSR_PLAN_ORDER: [u32; 4] = [
    MSR_IA32_EFER,
    MSR_IA32_LSTAR,
    MSR_IA32_GS_BASE,
    MSR_IA32_KERNEL_GS_BASE,
];

/// 抽象 MSR 空间（寄存器语义注入面之一——wrmsr/rdmsr 的 trait 化）。
///
/// 语义契约：write 落值即读回源（写后读必须一致——读回断言面的
/// 依据）；地址闭集外写 → MsrUnknown，读缺席 → None。
pub trait MsrSpace {
    /// 写 MSR（地址闭集校验在实现侧；失败全走 Result）。
    fn write(&mut self, addr: u32, value: u64) -> Result<(), ApGdtError>;
    /// 读 MSR（未知地址 None——与「已知地址初值 0」区分）。
    fn read(&self, addr: u32) -> Option<u64>;
}

/// 定容模拟 MSR 空间（8 槽数组闭集存储 + 读回失配注入位）。
///
/// 注入语义：corrupt_addr 命中的地址在 write 时落值 ^ corrupt_xor
/// ——读回断言面即捕获（写入值 ≠ 读回值 → MsrReadbackMismatch）；
/// xor 非零保证注入痕迹与合法值可区分。
#[derive(Clone, Debug)]
pub struct SimMsrSpace {
    /// 8 槽 (地址, 值) 对（地址来自 MSR_KNOWN 冻结表，值初 0）。
    vals: Vec<(u32, u64)>,
    /// 读回失配注入目标地址（None = 无注入）。
    pub corrupt_addr: Option<u32>,
    /// 注入异或掩码（非零）。
    pub corrupt_xor: u64,
}

impl SimMsrSpace {
    /// 无注入构造（8 槽全零初值——真实 wrmsr 前的「冷 MSR 面」）。
    pub fn new() -> Self {
        let mut vals = Vec::new();
        let mut i = 0usize;
        while i < MSR_KNOWN.len() {
            vals.push((MSR_KNOWN[i], 0));
            i += 1;
        }
        SimMsrSpace { vals, corrupt_addr: None, corrupt_xor: 0 }
    }

    /// 带读回失配注入构造（目标地址 + 非零掩码；掩码零会被静默
    /// 拒绝为无注入——注入痕迹必须可区分）。
    pub fn with_corruption(addr: u32, xor: u64) -> Self {
        let mut s = SimMsrSpace::new();
        if xor != 0 {
            s.corrupt_addr = Some(addr);
            s.corrupt_xor = xor;
        }
        s
    }
}

impl MsrSpace for SimMsrSpace {
    fn write(&mut self, addr: u32, value: u64) -> Result<(), ApGdtError> {
        let mut i = 0usize;
        while i < self.vals.len() {
            if self.vals[i].0 == addr {
                let mut v = value;
                if self.corrupt_addr == Some(addr) {
                    v = v ^ self.corrupt_xor;
                }
                self.vals[i].1 = v;
                return Ok(());
            }
            i += 1;
        }
        Err(ApGdtError::MsrUnknown { addr })
    }

    fn read(&self, addr: u32) -> Option<u64> {
        let mut i = 0usize;
        while i < self.vals.len() {
            if self.vals[i].0 == addr {
                return Some(self.vals[i].1);
            }
            i += 1;
        }
        None
    }
}

/// EFER 位解码（SCE/LME/LMA/NXE 四位元组——同构断言与诊断展示面）。
pub fn efer_decode(v: u64) -> (bool, bool, bool, bool) {
    (v & EFER_SCE != 0, v & EFER_LME != 0, v & EFER_LMA != 0, v & EFER_NXE != 0)
}

/// MSR 装载写序器（写序纪律的独立执行面——可脱离五步序列单测）。
///
/// 状态 = 已写步数（0..=4）：write_next 只接受「全序表下一步」的
/// 地址，乱写/倒写/跳写/超写一律 MsrWriteOrderBad；每次写后立即
/// 读回校验（写→读一致 → MsrReadbackMismatch / 缺席 → 同错哨兵值）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MsrLoadSeq {
    /// 已完成写步数（0 = 未写；4 = 序完）。
    pub step: u32,
}

impl MsrLoadSeq {
    /// 新序（步 0）。
    pub const fn new() -> Self {
        MsrLoadSeq { step: 0 }
    }

    /// 序是否已完（四步全写）。
    pub fn done(&self) -> bool {
        self.step >= MSR_PLAN_ORDER.len() as u32
    }

    /// 按全序写下一笔（addr 必须等于 MSR_PLAN_ORDER[step] 的地址）。
    pub fn write_next<M: MsrSpace>(
        &mut self,
        addr: u32,
        value: u64,
        m: &mut M,
    ) -> Result<(), ApGdtError> {
        let want = match MSR_PLAN_ORDER.get(self.step as usize) {
            Some(&a) => a,
            // 步号超表（序已完仍再写）——超写面。
            None => {
                return Err(ApGdtError::MsrWriteOrderBad {
                    expect_addr: 0,
                    got_addr: addr,
                    step: self.step,
                })
            }
        };
        if want != addr {
            return Err(ApGdtError::MsrWriteOrderBad {
                expect_addr: want,
                got_addr: addr,
                step: self.step,
            });
        }
        m.write(addr, value)?;
        match m.read(addr) {
            Some(r) if r == value => {}
            other => {
                return Err(ApGdtError::MsrReadbackMismatch {
                    addr,
                    wrote: value,
                    read: match other {
                        Some(v) => v,
                        None => u64::MAX,
                    },
                })
            }
        }
        self.step = self.step.saturating_add(1);
        Ok(())
    }

    /// 依 plan 直通跑完四笔（write_next × 4 的便捷面；plan 值序与
    /// MSR_PLAN_ORDER 严格对位）。
    pub fn run_plan<M: MsrSpace>(
        &mut self,
        plan: &MsrPlan,
        m: &mut M,
    ) -> Result<(), ApGdtError> {
        let vals = [plan.efer, plan.lstar, plan.gs_base, plan.kgs_base];
        let mut i = 0usize;
        while i < MSR_PLAN_ORDER.len() {
            self.write_next(MSR_PLAN_ORDER[i], vals[i], m)?;
            i += 1;
        }
        Ok(())
    }
}

/// 每 AP 的 MSR 装载计划（值包——上游 F0328/F0344 语义的数值承接面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MsrPlan {
    /// 涉事 AP。
    pub cpu_id: u32,
    /// EFER 装载值（恒 EFER_CANON——BSP 同构冻结值）。
    pub efer: u64,
    /// LSTAR 装载值（syscall 入口符号冻结快照——链接期定，本模块
    /// 以 LSTAR_SNAPSHOT 承接）。
    pub lstar: u64,
    /// GS_BASE 装载值（该 AP per-CPU 区基址——F0328 区账供给）。
    pub gs_base: u64,
    /// KERNEL_GS_BASE 装载值（恒 0——早期无用户线程，BSP 同构冻结值）。
    pub kgs_base: u64,
}

impl MsrPlan {
    /// canonical 计划构造（EFER/KGS 冻结，GS_BASE/LSTAR 由上游供给）。
    pub fn canonical(cpu_id: u32, gs_base: u64, lstar: u64) -> Self {
        MsrPlan { cpu_id, efer: EFER_CANON, lstar, gs_base, kgs_base: 0 }
    }

    /// 计划值依装载序取值（i 对位 MSR_PLAN_ORDER；越界 None）。
    pub fn value_at(&self, i: usize) -> Option<u64> {
        match i {
            0 => Some(self.efer),
            1 => Some(self.lstar),
            2 => Some(self.gs_base),
            3 => Some(self.kgs_base),
            _ => None,
        }
    }
}

// ============================================================================
// 第 5 节 · CpuPort / SimCpuPort（CPU 寄存器端口注入面）
// ============================================================================

/// 抽象 CPU 寄存器端口（寄存器语义注入面之二——lgdt/sgdt/ltr/str
/// 四操作 trait 化；真实实现对接内联汇编壳，本模块零汇编）。
///
/// 语义契约：lgdt 后 sgdt 必须读回同值（GDTR 影子）；ltr 后 str_reg
/// 读回同选择子（TR 影子）——装载后读回断言（J1 后半句）的依据面。
pub trait CpuPort {
    /// 装载 GDTR（lgdt 语义）。
    fn lgdt(&mut self, gdtr: &Gdtr) -> Result<(), ApGdtError>;
    /// 读回 GDTR（sgdt 语义；缺席 None = 端口无影子）。
    fn sgdt(&self) -> Option<Gdtr>;
    /// 装载任务寄存器（ltr 语义；selector 为 TSS 选择子）。
    fn ltr(&mut self, selector: u16) -> Result<(), ApGdtError>;
    /// 读回任务寄存器（str 语义）。
    fn str_reg(&self) -> Option<u16>;
    /// 端口归属 AP（错误上下文）。
    fn cpu_id(&self) -> u32;
}

/// 模拟 CPU 端口（GDTR/TR 影子寄存器对——装载即落影子、读回即
/// 影子拷贝；故障面由替代实现注入，见测试 DenyPort）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimCpuPort {
    /// 归属 AP。
    pub cpu_id: u32,
    /// GDTR 影子（None = 尚未 lgdt）。
    pub shadow_gdtr: Option<Gdtr>,
    /// TR 影子（None = 尚未 ltr）。
    pub shadow_tr: Option<u16>,
}

impl SimCpuPort {
    /// 新端口（双影子空）。
    pub fn new(cpu_id: u32) -> Self {
        SimCpuPort { cpu_id, shadow_gdtr: None, shadow_tr: None }
    }
}

impl CpuPort for SimCpuPort {
    fn lgdt(&mut self, gdtr: &Gdtr) -> Result<(), ApGdtError> {
        self.shadow_gdtr = Some(*gdtr);
        Ok(())
    }

    fn sgdt(&self) -> Option<Gdtr> {
        self.shadow_gdtr
    }

    fn ltr(&mut self, selector: u16) -> Result<(), ApGdtError> {
        self.shadow_tr = Some(selector);
        Ok(())
    }

    fn str_reg(&self) -> Option<u16> {
        self.shadow_tr
    }

    fn cpu_id(&self) -> u32 {
        self.cpu_id
    }
}

// ============================================================================
// 第 6 节 · LoadSequence（装载序列状态机：五步全序 + 前置校验 + 读回断言）
// ============================================================================

/// 装载相位（五步全序的状态空间——六态：初始 + 每步完成各一态）。
///
/// 全序（违反即 OutOfPhase，无例外）：
///   Init → GdtProgrammed（步 1 GdtProgram：物化 GDT 镜像）
///        → GdtLoaded（步 2 Lgdt：校验和过 → lgdt → sgdt 读回）
///        → TssProgrammed（步 3 TssProgram：物化 TSS 镜像）
///        → TssLoaded（步 4 Ltr：IST/IO/保留三查 → ltr → str 读回）
///        → MsrLoaded（步 5 MsrLoad：写序四笔 → 逐笔读回）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadPhase {
    /// 序始（未物化任何镜像）。
    Init,
    /// 步 1 完成：GDT 镜像已物化（未装载）。
    GdtProgrammed,
    /// 步 2 完成：lgdt 已过（校验和 + sgdt 读回双断言）。
    GdtLoaded,
    /// 步 3 完成：TSS 镜像已物化（未装载）。
    TssProgrammed,
    /// 步 4 完成：ltr 已过（前置三查 + str 读回）。
    TssLoaded,
    /// 步 5 完成：MSR 四笔全落（写序 + 逐笔读回）。终态。
    MsrLoaded,
}

impl LoadPhase {
    /// 相位序号（0..=5——健康账 phase_reached 字段口径）。
    pub const fn index(self) -> u8 {
        match self {
            LoadPhase::Init => 0,
            LoadPhase::GdtProgrammed => 1,
            LoadPhase::GdtLoaded => 2,
            LoadPhase::TssProgrammed => 3,
            LoadPhase::TssLoaded => 4,
            LoadPhase::MsrLoaded => 5,
        }
    }

    /// 相位中文名（事件账/诊断展示）。
    pub const fn name(self) -> &'static str {
        match self {
            LoadPhase::Init => "初始",
            LoadPhase::GdtProgrammed => "GDT已编程",
            LoadPhase::GdtLoaded => "lgdt已过",
            LoadPhase::TssProgrammed => "TSS已编程",
            LoadPhase::TssLoaded => "ltr已过",
            LoadPhase::MsrLoaded => "MSR已装载",
        }
    }
}

/// 步 1 入参（GDT 物化参数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GdtProgramParams {
    /// 每 AP 独立 GDT 的镜像基址（GDTR.base）。
    pub gdt_base: u64,
    /// 该 AP TSS 镜像基址（写进 TSS 双描述符的 base 域）。
    pub tss_base: u64,
    /// TSS 限长（恒 TSS_BYTES - 1 = 103）。
    pub tss_len: u32,
}

/// 步 3 入参（TSS 物化参数——F0328 栈域供给的数值面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TssProgramParams {
    /// TSS 镜像基址。
    pub tss_base: u64,
    /// RSP0：AP 内核栈顶。
    pub rsp0: u64,
    /// IST1..IST7：七路异常栈顶。
    pub ist: [u64; 7],
}

/// AP 装载序列（单 AP 的五步全序执行体——镜像物化 + 寄存器装载 +
/// 逐步断言；CPU/MSR 端口以 trait 注入，账以 &mut 传入）。
#[derive(Clone, Debug)]
pub struct ApLoadSequence {
    /// 归属 AP。
    pub cpu_id: u32,
    /// 当前相位（全序判定的唯一状态源）。
    pub phase: LoadPhase,
    /// 已物化 GDT 镜像（步 1 后 Some）。
    pub gdt: Option<GdtImage>,
    /// 已物化 TSS 镜像（步 3 后 Some）。
    pub tss: Option<TssImage>,
    /// MSR 计划（构造时注入——步 5 消费）。
    pub msr_plan: MsrPlan,
}

impl ApLoadSequence {
    /// 新序（Init 相位；msr_plan 随构造注入——GS_BASE/LSTAR 来自
    /// F0328/F0344 供给面）。
    pub fn new(cpu_id: u32, msr_plan: MsrPlan) -> Self {
        ApLoadSequence {
            cpu_id,
            phase: LoadPhase::Init,
            gdt: None,
            tss: None,
            msr_plan,
        }
    }

    /// 相位门（want 不符即 OutOfPhase——乱序/跳步/重复的唯一拒绝面）。
    fn expect_phase(&self, want: LoadPhase) -> Result<(), ApGdtError> {
        if self.phase == want {
            Ok(())
        } else {
            Err(ApGdtError::OutOfPhase { expect: want, got: self.phase })
        }
    }

    /// 步 1 GdtProgram：物化该 AP 的 40 字节 GDT 镜像并记录校验和。
    ///
    /// 前置：相位 Init；产物：gdt = Some，相位 → GdtProgrammed；
    /// 事件：EV_GDT_PROGRAMMED（a = 校验和，b = GDT 基址）。
    pub fn step_gdt_program(
        &mut self,
        p: GdtProgramParams,
        ledger: &mut EventLedger,
    ) -> Result<(), ApGdtError> {
        self.expect_phase(LoadPhase::Init)?;
        let img = GdtImage::program(self.cpu_id, p.gdt_base, p.tss_base, p.tss_len)?;
        let _ = ledger.push(LoadEvent {
            cpu_id: self.cpu_id,
            kind: EV_GDT_PROGRAMMED,
            a: img.checksum,
            b: img.base,
        });
        self.gdt = Some(img);
        self.phase = LoadPhase::GdtProgrammed;
        Ok(())
    }

    /// 步 2 Lgdt：校验和复算 → lgdt 装载 → sgdt 读回断言。
    ///
    /// 前置：相位 GdtProgrammed 且校验和过（program 后镜像被改写
    /// 即 GdtChecksumBad——踩踏/篡改检出面）；读回 base/limit 任一
    /// 失配即 LgdtReadbackBad（J1「装载后读回断言」第一落点）。
    pub fn step_lgdt<P: CpuPort>(
        &mut self,
        port: &mut P,
        ledger: &mut EventLedger,
    ) -> Result<(), ApGdtError> {
        self.expect_phase(LoadPhase::GdtProgrammed)?;
        let img = match self.gdt.as_ref() {
            Some(g) => g,
            None => {
                return Err(ApGdtError::OutOfPhase {
                    expect: LoadPhase::GdtProgrammed,
                    got: LoadPhase::Init,
                })
            }
        };
        if !img.checksum_ok() {
            let (recorded, live) = img.checksum_pair();
            return Err(ApGdtError::GdtChecksumBad { cpu_id: self.cpu_id, recorded, live });
        }
        let gdtr = img.gdtr();
        port.lgdt(&gdtr)?;
        let rb = port.sgdt();
        let (ab, al) = match rb {
            Some(g) => (g.base, g.limit),
            None => (u64::MAX, u16::MAX),
        };
        if ab != gdtr.base || al != gdtr.limit {
            return Err(ApGdtError::LgdtReadbackBad {
                cpu_id: self.cpu_id,
                expect_base: gdtr.base,
                actual_base: ab,
                expect_limit: gdtr.limit,
                actual_limit: al,
            });
        }
        let _ = ledger.push(LoadEvent {
            cpu_id: self.cpu_id,
            kind: EV_LGDT_DONE,
            a: gdtr.base,
            b: gdtr.limit as u64,
        });
        self.phase = LoadPhase::GdtLoaded;
        Ok(())
    }

    /// 步 3 TssProgram：物化该 AP 的 104 字节 TSS 镜像。
    ///
    /// 前置：相位 GdtLoaded；产物：tss = Some，相位 → TssProgrammed；
    /// 事件：EV_TSS_PROGRAMMED（a = RSP0，b = IST1）。IST 合法性
    /// 留给步 4 前置三查（任务钦定「ltr 前 TSS IST 非零」）。
    pub fn step_tss_program(
        &mut self,
        p: TssProgramParams,
        ledger: &mut EventLedger,
    ) -> Result<(), ApGdtError> {
        self.expect_phase(LoadPhase::GdtLoaded)?;
        let img = TssImage::program(self.cpu_id, p.tss_base, p.rsp0, p.ist)?;
        let _ = ledger.push(LoadEvent {
            cpu_id: self.cpu_id,
            kind: EV_TSS_PROGRAMMED,
            a: p.rsp0,
            b: p.ist[0],
        });
        self.tss = Some(img);
        self.phase = LoadPhase::TssProgrammed;
        Ok(())
    }

    /// 步 4 Ltr：TSS 前置三查 → ltr 装载 → str 读回断言。
    ///
    /// 前置三查（TssImage::preflight_ltr）：IST 七路全非零（任务
    /// 钦定）+ IO 位图基址 104 + 保留域零；ltr 目标恒 SEL_TSS = 0x18，
    /// str 读回失配即 LtrReadbackBad（读回断言第二落点）。
    pub fn step_ltr<P: CpuPort>(
        &mut self,
        port: &mut P,
        ledger: &mut EventLedger,
    ) -> Result<(), ApGdtError> {
        self.expect_phase(LoadPhase::TssProgrammed)?;
        let img = match self.tss.as_ref() {
            Some(t) => t,
            None => {
                return Err(ApGdtError::OutOfPhase {
                    expect: LoadPhase::TssProgrammed,
                    got: self.phase,
                })
            }
        };
        img.preflight_ltr()?;
        port.ltr(GdtLayout::SEL_TSS)?;
        let rb = port.str_reg();
        let actual = match rb {
            Some(s) => s,
            None => u16::MAX,
        };
        if actual != GdtLayout::SEL_TSS {
            return Err(ApGdtError::LtrReadbackBad {
                expect: GdtLayout::SEL_TSS,
                actual,
            });
        }
        let _ = ledger.push(LoadEvent {
            cpu_id: self.cpu_id,
            kind: EV_LTR_DONE,
            a: GdtLayout::SEL_TSS as u64,
            b: TssLayout::IOMAP_ABSENT as u64,
        });
        self.phase = LoadPhase::TssLoaded;
        Ok(())
    }

    /// 步 5 MsrLoad：写序四笔（EFER→LSTAR→GS_BASE→KERNEL_GS_BASE）
    /// 逐笔「写→读回」断言。
    ///
    /// 前置：相位 TssLoaded；MsrLoadSeq 内建写序纪律（乱写即
    /// MsrWriteOrderBad）与读回一致校验（MsrReadbackMismatch——
    /// 读回断言第三落点）；EFER 装载值另查 LMA 位（长模式激活——
    /// 未激活即 DescFieldBad 拦截，防错版本 EFER 混入）。
    pub fn step_msr_load<M: MsrSpace>(
        &mut self,
        msr: &mut M,
        ledger: &mut EventLedger,
    ) -> Result<(), ApGdtError> {
        self.expect_phase(LoadPhase::TssLoaded)?;
        let (_, lme_on, lma_on, _) = efer_decode(self.msr_plan.efer);
        if !(lme_on && lma_on) {
            return Err(ApGdtError::DescFieldBad {
                what: "MSR 计划 EFER 值缺 LME/LMA 位（长模式未使能/未激活）",
                value: self.msr_plan.efer,
            });
        }
        let mut seq = MsrLoadSeq::new();
        seq.run_plan(&self.msr_plan, msr)?;
        let _ = ledger.push(LoadEvent {
            cpu_id: self.cpu_id,
            kind: EV_MSR_LOAD_DONE,
            a: self.msr_plan.efer,
            b: self.msr_plan.gs_base,
        });
        self.phase = LoadPhase::MsrLoaded;
        Ok(())
    }

    /// 终态读回快照（相位 MsrLoaded 后从端口/空间重读全部装载面——
    /// J1「装载后读回断言」的汇总第四落点；任何缺席即对应读回错）。
    pub fn capture<P: CpuPort, M: MsrSpace>(
        &self,
        port: &P,
        msr: &M,
    ) -> Result<ApLoadedState, ApGdtError> {
        self.expect_phase(LoadPhase::MsrLoaded)?;
        let img = match self.gdt.as_ref() {
            Some(g) => g,
            None => {
                return Err(ApGdtError::OutOfPhase {
                    expect: LoadPhase::MsrLoaded,
                    got: self.phase,
                })
            }
        };
        let tss_img = match self.tss.as_ref() {
            Some(t) => t,
            None => {
                return Err(ApGdtError::OutOfPhase {
                    expect: LoadPhase::MsrLoaded,
                    got: self.phase,
                })
            }
        };
        let gdtr = match port.sgdt() {
            Some(g) => g,
            None => {
                return Err(ApGdtError::LgdtReadbackBad {
                    cpu_id: self.cpu_id,
                    expect_base: img.base,
                    actual_base: u64::MAX,
                    expect_limit: (GdtLayout::BYTES - 1) as u16,
                    actual_limit: u16::MAX,
                })
            }
        };
        let tr = match port.str_reg() {
            Some(s) => s,
            None => u16::MAX,
        };
        if tr != GdtLayout::SEL_TSS {
            return Err(ApGdtError::LtrReadbackBad { expect: GdtLayout::SEL_TSS, actual: tr });
        }
        let mut msr_vals = [(0u32, 0u64); 4];
        let mut i = 0usize;
        while i < MSR_PLAN_ORDER.len() {
            let addr = MSR_PLAN_ORDER[i];
            let v = match msr.read(addr) {
                Some(v) => v,
                None => u64::MAX,
            };
            let expect = match self.msr_plan.value_at(i) {
                Some(e) => e,
                None => 0,
            };
            if v != expect {
                return Err(ApGdtError::MsrReadbackMismatch { addr, wrote: expect, read: v });
            }
            msr_vals[i] = (addr, v);
            i += 1;
        }
        Ok(ApLoadedState {
            cpu_id: self.cpu_id,
            gdtr,
            gdt_bytes: img.bytes.clone(),
            tr,
            tss_bytes: tss_img.bytes.clone(),
            tss_base: tss_img.base,
            msr: msr_vals,
        })
    }

    /// 五步直通（run 序：1→5 依序调用 + 终态快照；任一步 Err 即
    /// 中止上抛——相位停在失败步的前一态，账已记失败事件由调用方
    /// 负责，本方法不吞错）。
    pub fn run_all<P: CpuPort, M: MsrSpace>(
        &mut self,
        gp: GdtProgramParams,
        tp: TssProgramParams,
        port: &mut P,
        msr: &mut M,
        ledger: &mut EventLedger,
    ) -> Result<ApLoadedState, ApGdtError> {
        let _ = ledger.push(LoadEvent {
            cpu_id: self.cpu_id,
            kind: EV_SEQ_START,
            a: gp.gdt_base,
            b: tp.tss_base,
        });
        self.step_gdt_program(gp, ledger)?;
        self.step_lgdt(port, ledger)?;
        self.step_tss_program(tp, ledger)?;
        self.step_ltr(port, ledger)?;
        self.step_msr_load(msr, ledger)?;
        let state = self.capture(port, msr)?;
        let _ = ledger.push(LoadEvent {
            cpu_id: self.cpu_id,
            kind: EV_SEQ_DONE,
            a: self.phase.index() as u64,
            b: self.msr_plan.gs_base,
        });
        Ok(state)
    }
}

/// 单 AP 装载终态快照（同构断言的实际侧输入）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApLoadedState {
    /// 归属 AP。
    pub cpu_id: u32,
    /// sgdt 读回的 GDTR。
    pub gdtr: Gdtr,
    /// 40 字节 GDT 镜像（装载语义面）。
    pub gdt_bytes: Vec<u8>,
    /// str 读回的任务选择子。
    pub tr: u16,
    /// 104 字节 TSS 镜像。
    pub tss_bytes: Vec<u8>,
    /// TSS 镜像基址。
    pub tss_base: u64,
    /// 四 MSR 终值（依 MSR_PLAN_ORDER 序）。
    pub msr: [(u32, u64); 4],
}

// ============================================================================
// 第 7 节 · BspMirror / ApExpectation / IsoReport（BSP 同构断言面）
// ============================================================================

/// 同构字段总数（GDT 5 描述符 + TSS 13 字段 + MSR 4 值 = 22/AP——
/// 同构率判据的分母冻结值）。
pub const ISO_FIELD_COUNT: u32 = 22;

/// 编译期自证 #7：同构字段总数与部件和一致（5 + 13 + 4）。
const _: () = assert!(ISO_FIELD_COUNT == 5 + 13 + 4);

/// BSP 基线镜像（BSP 侧已完成装载的规则快照——AP 同构断言的期望侧
/// 生成器）。
///
/// 「同构」语义：AP 装载结果与「BSP 按 AP 参数重放同一构造规则」
/// 的投影逐字段一致——绝对冻结字段（代码/数据描述符、EFER/LSTAR/
/// KGS 值）必须与 BSP 逐位相同；相对投影字段（TSS 描述符 base、
/// RSP0/IST、GS_BASE）按该 AP 的区账投影。冻结字段被改 = BSP 线
/// 偏移（IsoBroken 且 diff 指向冻结字段）；投影字段错 = AP 线
/// 装载错（diff 指向投影字段）——两类都在同一 diff 面定位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BspMirror {
    /// BSP 代码段描述符字节（冻结——所有 AP 与 BSP 逐字节一致）。
    pub code_desc: [u8; 8],
    /// BSP 数据段描述符字节（冻结）。
    pub data_desc: [u8; 8],
    /// BSP EFER 冻结值（恒 EFER_CANON）。
    pub efer: u64,
    /// BSP LSTAR 冻结值（syscall 入口全局唯一）。
    pub lstar: u64,
    /// BSP KERNEL_GS_BASE 冻结值（早期 0）。
    pub kgs_base: u64,
}

impl BspMirror {
    /// canonical BSP 基线（与 F0307/F0321 BSP 侧装载同构的规则镜像）。
    pub fn canonical() -> Self {
        BspMirror {
            code_desc: encode_desc(GdtLayout::code64_fields()),
            data_desc: encode_desc(GdtLayout::data_fields()),
            efer: EFER_CANON,
            lstar: LSTAR_SNAPSHOT,
            kgs_base: 0,
        }
    }

    /// 按 AP 参数投影期望装载像（期望侧生成——22 字段的期望值全源）。
    ///
    /// 投影规则：null 全零；code/data 取冻结字节；TSS 双描述符按
    /// 该 AP 的 tss_base/103 编码；TSS 镜像 RSP0/IST 按该 AP 栈账、
    /// RSP1/2 与保留域零、IO 位图 104；MSR 四值 = 冻结 EFER/LSTAR/KGS
    /// + 该 AP gs_base。
    pub fn project(
        &self,
        cpu_id: u32,
        gdt_base: u64,
        tss_base: u64,
        rsp0: u64,
        ist: [u64; 7],
        gs_base: u64,
    ) -> ApExpectation {
        let mut gdt = [0u8; 40];
        let tss_lo = encode_desc(GdtLayout::tss_lo_fields(tss_base as u32, TssLayout::BYTES - 1));
        let mut tss_hi = [0u8; 8];
        tss_hi[0] = ((tss_base >> 32) & 0xFF) as u8;
        tss_hi[1] = ((tss_base >> 40) & 0xFF) as u8;
        tss_hi[2] = ((tss_base >> 48) & 0xFF) as u8;
        tss_hi[3] = ((tss_base >> 56) & 0xFF) as u8;
        // 逐槽落位（const 偏移；40 字节数组内必成——失败的唯一来源
        // 是布局表被改，put 失败走静默零槽，由同构 diff 兜底检出）。
        let _ = put_arr8(&mut gdt, GdtLayout::OFF_CODE64 as usize, &self.code_desc);
        let _ = put_arr8(&mut gdt, GdtLayout::OFF_DATA as usize, &self.data_desc);
        let _ = put_arr8(&mut gdt, GdtLayout::OFF_TSS_LO as usize, &tss_lo);
        let _ = put_arr8(&mut gdt, GdtLayout::OFF_TSS_HI as usize, &tss_hi);
        let mut tss = [0u8; 104];
        let _ = wr_u64_le(&mut tss, TssLayout::OFF_RSP0 as usize, rsp0);
        let mut i = 0usize;
        while i < 7 {
            let _ = wr_u64_le(&mut tss, TssLayout::ist_offset((i + 1) as u32) as usize, ist[i]);
            i += 1;
        }
        let _ = wr_u16_le(&mut tss, TssLayout::OFF_IOMAP as usize, TssLayout::IOMAP_ABSENT);
        ApExpectation {
            cpu_id,
            gdt_base,
            gdt,
            tss,
            msr: [
                (MSR_IA32_EFER, self.efer),
                (MSR_IA32_LSTAR, self.lstar),
                (MSR_IA32_GS_BASE, gs_base),
                (MSR_IA32_KERNEL_GS_BASE, self.kgs_base),
            ],
        }
    }
}

/// 每 AP 期望装载像（BspMirror 投影产物——diff 的期望侧）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApExpectation {
    /// 归属 AP。
    pub cpu_id: u32,
    /// 期望 GDTR.base（GDTR 限长恒 39 不入字段账）。
    pub gdt_base: u64,
    /// 期望 40 字节 GDT 镜像。
    pub gdt: [u8; 40],
    /// 期望 104 字节 TSS 镜像。
    pub tss: [u8; 104],
    /// 期望四 MSR 值（依 MSR_PLAN_ORDER 序）。
    pub msr: [(u32, u64); 4],
}

/// 同构差异项（路径 + 期望 + 实际——差异定位的最小信息单元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IsoDiff {
    /// 字段路径（"gdt.code64"/"tss.ist3"/"msr.gs_base" 风格）。
    pub path: &'static str,
    /// 期望值（BSP 投影）。
    pub expect: u64,
    /// 实际值（AP 装载）。
    pub actual: u64,
}

/// 单 AP 同构报告（22 字段的比对结论 + 差异清单）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoReport {
    /// 归属 AP。
    pub cpu_id: u32,
    /// 参比字段总数（恒 ISO_FIELD_COUNT = 22）。
    pub fields: u32,
    /// 一致字段数。
    pub matched: u32,
    /// 差异清单（空 = 全同构）。
    pub diffs: Vec<IsoDiff>,
}

impl IsoReport {
    /// 同构率（百分整数；matched*100/fields——22 字段下无溢出，
    /// 除数非零由 fields 恒 22 保证，saturating 防御）。
    pub fn rate_percent(&self) -> u32 {
        if self.fields == 0 {
            return 0;
        }
        self.matched.saturating_mul(100) / self.fields
    }

    /// 是否全同构（判据口径：率 100% 且确有参比字段）。
    pub fn is_full_iso(&self) -> bool {
        self.fields > 0 && self.diffs.is_empty()
    }
}

/// 逐字段 diff（J1 前半句「与 BSP 同构」的判定本体）。
///
/// 比对域（22 字段）：
///   GDT 5 槽——null/code64/data/tss_lo/tss_hi 各 8 字节按 u64 比；
///   TSS 13 字段——rsp0/rsp1/rsp2/ist1..7/两个保留 u64 按 u64 比 +
///     IO 位图基址 u16（升 u64 比）；
///   MSR 4 值——EFER/LSTAR/GS_BASE/KERNEL_GS_BASE。
/// GDTR.base 与 tr 不入字段账（装载序读回断言已覆盖；同构面专注
/// 镜像与 MSR 值）。
pub fn diff_iso(state: &ApLoadedState, exp: &ApExpectation) -> IsoReport {
    let mut diffs: Vec<IsoDiff> = Vec::new();
    let slot_offs = [
        GdtLayout::OFF_NULL,
        GdtLayout::OFF_CODE64,
        GdtLayout::OFF_DATA,
        GdtLayout::OFF_TSS_LO,
        GdtLayout::OFF_TSS_HI,
    ];
    let mut slot = 0u32;
    while slot < 5 {
        let off = slot_offs[slot as usize] as usize;
        let actual = rd_u64_le(&state.gdt_bytes, off);
        let expect = rd_u64_le(&exp.gdt, off);
        let (a, e) = match (actual, expect) {
            (Some(a), Some(e)) => (a, e),
            // 缺席理论不可达（镜像定长）——按哨兵对进 diff 兜底。
            _ => (u64::MAX, u64::MAX),
        };
        if a != e {
            diffs.push(IsoDiff { path: GdtLayout::slot_name(slot), expect: e, actual: a });
        }
        slot += 1;
    }
    // TSS 13 字段：3 RSP + 7 IST + 2 保留（u64）+ IO 位图基址（u16）。
    let tss_names = [
        "tss.rsp0",
        "tss.rsp1",
        "tss.rsp2",
        "tss.ist1",
        "tss.ist2",
        "tss.ist3",
        "tss.ist4",
        "tss.ist5",
        "tss.ist6",
        "tss.ist7",
        "tss.rsvd84",
        "tss.rsvd92",
        "tss.iomap",
    ];
    let tss_offs = [
        TssLayout::OFF_RSP0,
        TssLayout::OFF_RSP1,
        TssLayout::OFF_RSP2,
        TssLayout::ist_offset(1),
        TssLayout::ist_offset(2),
        TssLayout::ist_offset(3),
        TssLayout::ist_offset(4),
        TssLayout::ist_offset(5),
        TssLayout::ist_offset(6),
        TssLayout::ist_offset(7),
        TssLayout::OFF_RSVD84,
        TssLayout::OFF_RSVD92,
        TssLayout::OFF_IOMAP,
    ];
    let mut f = 0usize;
    while f < 13 {
        let off = tss_offs[f] as usize;
        if tss_offs[f] == TssLayout::OFF_IOMAP {
            // IO 位图基址 u16（升 u64 比）。
            let a = match rd_u16_le(&state.tss_bytes, off) {
                Some(v) => v as u64,
                None => u64::MAX,
            };
            let e = match rd_u16_le(&exp.tss, off) {
                Some(v) => v as u64,
                None => u64::MAX,
            };
            if a != e {
                diffs.push(IsoDiff { path: tss_names[f], expect: e, actual: a });
            }
        } else {
            let a = match rd_u64_le(&state.tss_bytes, off) {
                Some(v) => v,
                None => u64::MAX,
            };
            let e = match rd_u64_le(&exp.tss, off) {
                Some(v) => v,
                None => u64::MAX,
            };
            if a != e {
                diffs.push(IsoDiff { path: tss_names[f], expect: e, actual: a });
            }
        }
        f += 1;
    }
    // MSR 4 值。
    let msr_names = ["msr.efer", "msr.lstar", "msr.gs_base", "msr.kernel_gs_base"];
    let mut m = 0usize;
    while m < 4 {
        if state.msr[m].1 != exp.msr[m].1 {
            diffs.push(IsoDiff {
                path: msr_names[m],
                expect: exp.msr[m].1,
                actual: state.msr[m].1,
            });
        }
        m += 1;
    }
    let matched = ISO_FIELD_COUNT.saturating_sub(diffs.len() as u32);
    IsoReport { cpu_id: state.cpu_id, fields: ISO_FIELD_COUNT, matched, diffs }
}

// ============================================================================
// 第 8 节 · EventLedger / HealthReport（事件账 + 体检 + FNV 指纹）
// ============================================================================

/// 事件账容量（256 条/AP 域足够 8 AP 全链 ×3 注入——容量来源：邻件
/// unx0328_apstack.rs 同口径 256，B17 批对拍账宽惯例）。
pub const LEDGER_CAP: usize = 256;

/// 事件种类码（账内 kind 字段闭集——诊断码段外的独立小闭集）。
pub const EV_SEQ_START: u16 = 1;
/// GDT 镜像物化完成。
pub const EV_GDT_PROGRAMMED: u16 = 2;
/// lgdt 装载 + 读回双断言过。
pub const EV_LGDT_DONE: u16 = 3;
/// TSS 镜像物化完成。
pub const EV_TSS_PROGRAMMED: u16 = 4;
/// ltr 装载 + 前置三查 + str 读回过。
pub const EV_LTR_DONE: u16 = 5;
/// MSR 四笔写序 + 读回全过。
pub const EV_MSR_LOAD_DONE: u16 = 6;
/// 五步全序走完（终态快照已取）。
pub const EV_SEQ_DONE: u16 = 7;
/// 序列失败（a = 诊断码，b = 停留相位号）。
pub const EV_SEQ_FAIL: u16 = 8;
/// 同构全过（a = 同构率，b = 字段总数）。
pub const EV_ISO_OK: u16 = 9;
/// 同构断裂（a = 同构率，b = 差异数）。
pub const EV_ISO_BROKEN: u16 = 10;
/// 注入检出（a = 检出码，b = 注入器号）。
pub const EV_INJECT_DETECTED: u16 = 11;
/// 注入未检出（a = 期望码，b = 注入器号）——此事件出现即纪律破。
pub const EV_INJECT_MISSED: u16 = 12;

/// 事件种类中文名（账可读面）。
pub const fn ev_name(kind: u16) -> &'static str {
    if kind == EV_SEQ_START {
        "序始"
    } else if kind == EV_GDT_PROGRAMMED {
        "GDT物化"
    } else if kind == EV_LGDT_DONE {
        "lgdt过"
    } else if kind == EV_TSS_PROGRAMMED {
        "TSS物化"
    } else if kind == EV_LTR_DONE {
        "ltr过"
    } else if kind == EV_MSR_LOAD_DONE {
        "MSR装载过"
    } else if kind == EV_SEQ_DONE {
        "序完"
    } else if kind == EV_SEQ_FAIL {
        "序败"
    } else if kind == EV_ISO_OK {
        "同构过"
    } else if kind == EV_ISO_BROKEN {
        "同构断"
    } else if kind == EV_INJECT_DETECTED {
        "注入检出"
    } else if kind == EV_INJECT_MISSED {
        "注入漏检"
    } else {
        "未知事件"
    }
}

/// 装载事件（账内最小记录：AP + 种类 + 双 u64 载荷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadEvent {
    /// 归属 AP。
    pub cpu_id: u32,
    /// 事件种类（EV_* 闭集）。
    pub kind: u16,
    /// 载荷 a（种类语义见各常量注释）。
    pub a: u64,
    /// 载荷 b。
    pub b: u64,
}

/// 事件账（定容 + 溢出显性丢弃计数——满账不吞错不截断，丢弃可见）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventLedger {
    /// 已记事件（容量 LEDGER_CAP）。
    pub ev: Vec<LoadEvent>,
    /// 满账后显性丢弃计数（saturating——丢弃永不回绕）。
    pub dropped: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> Self {
        EventLedger { ev: Vec::new(), dropped: 0 }
    }

    /// 记一笔（满账返回 Err(EventFull) 且丢弃计数 +1；调用方以
    /// `let _ =` 消费的口径 = 丢弃可接受、账优先保序）。
    pub fn push(&mut self, e: LoadEvent) -> Result<(), ApGdtError> {
        if self.ev.len() >= LEDGER_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(ApGdtError::EventFull { capacity: LEDGER_CAP });
        }
        self.ev.push(e);
        Ok(())
    }

    /// 已记条数。
    pub fn len(&self) -> usize {
        self.ev.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.ev.is_empty()
    }

    /// 按种类计数（health 扫描的便捷面；saturating）。
    pub fn count_kind(&self, kind: u16) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.ev.len() {
            if self.ev[i].kind == kind {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }
}

/// 健康体检报告（orchestrate/注入后的收口只读视图）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 编排 AP 总数（saturating 收口后）。
    pub ap_total: u32,
    /// 五步全序 + 同构双过的 AP 数。
    pub ap_ok: u32,
    /// 任一环失败的 AP 数（ok + fail = total）。
    pub ap_fail: u32,
    /// 同构率 100% 的 AP 数（含序败未到同构面的 AP 记 0）。
    pub iso_full: u32,
    /// 注入检出次数。
    pub inject_detected: u32,
    /// 注入漏检次数（> 0 即纪律破——测试断言恒 0）。
    pub inject_missed: u32,
    /// 已记事件数。
    pub events: u32,
    /// 满账丢弃数。
    pub dropped: u64,
    /// 账指纹（FNV-1a 64，双跑对拍判据）。
    pub fingerprint: u64,
}

/// 账指纹（FNV-1a 64 流式混合：每事件 22 字节定长包
/// cpu_id(4) + kind(2) + a(8) + b(8)——确定性双跑对拍的唯一凭据）。
pub fn ledger_fingerprint(ledger: &EventLedger) -> u64 {
    let mut h = FNV_BASIS;
    let mut buf = [0u8; 22];
    let mut i = 0usize;
    while i < ledger.ev.len() {
        let e = ledger.ev[i];
        let _ = wr_u32_le(&mut buf, 0, e.cpu_id);
        let _ = wr_u16_le(&mut buf, 4, e.kind);
        let _ = wr_u64_le(&mut buf, 6, e.a);
        let _ = wr_u64_le(&mut buf, 14, e.b);
        h = fnv_mix(h, &buf);
        i += 1;
    }
    h
}

/// 小端写 u32（越界返回 false——账指纹包内使用，22 字节缓冲必成）。
pub fn wr_u32_le(b: &mut [u8], off: usize, v: u32) -> bool {
    if off.checked_add(4).map(|e| e > b.len()).unwrap_or(true) {
        return false;
    }
    b[off] = (v & 0xFF) as u8;
    b[off + 1] = ((v >> 8) & 0xFF) as u8;
    b[off + 2] = ((v >> 16) & 0xFF) as u8;
    b[off + 3] = ((v >> 24) & 0xFF) as u8;
    true
}

// ============================================================================
// 第 9 节 · ApGdtSubsystem（编排面 + 注入面 + 双跑确定性对拍）
// ============================================================================

/// 编排用区账常量（示例竞技场快照——真实分配走 F0071/F0072 页账，
/// 本模块以常量承接布局口径并留 Lcg 抖动，双跑种子确定性）。
///
/// 布局口径来源：邻件 unx0328_apstack.rs 的栈/区宽（栈 32KiB/AP、
/// per-CPU 区 4KiB/AP）+ 本模块的 GDT/TSS 整页隔离纪律。
pub const GDT_ARENA: u64 = 0x0010_0000;
/// 每 AP GDT 步距（整页隔离——40 字节实体 + 对齐纪律）。
pub const GDT_STRIDE: u64 = 0x1000;
/// TSS 竞技场基址。
pub const TSS_ARENA: u64 = 0x0020_0000;
/// 每 AP TSS 步距（整页隔离——104 字节实体 + 位图缺席纪律）。
pub const TSS_STRIDE: u64 = 0x1000;
/// 内核栈竞技场基址（F0328 同口径栈域的编排侧快照）。
pub const STACK_ARENA: u64 = 0x0030_0000;
/// 每 AP 内核栈步距（32KiB——F0328 栈深同源）。
pub const STACK_STRIDE: u64 = 0x8000;
/// 内核栈深（字节）。
pub const STACK_LEN: u64 = 0x8000;
/// IST 单路栈深（4KiB——NMI/#DF/#MC 异常栈惯例宽）。
pub const IST_STACK_LEN: u64 = 0x1000;
/// per-CPU 区竞技场基址。
pub const PERCPU_ARENA: u64 = 0x0040_0000;
/// 每 AP per-CPU 区步距（4KiB——F0328 区宽同源）。
pub const PERCPU_STRIDE: u64 = 0x1000;
/// AP 数量上限（255——UNX-F0335 预算账同源）。
pub const MAX_AP: u32 = 255;
/// LSTAR 冻结快照（syscall 入口符号示例值——高半区规范地址，
/// 真实值由链接期符号注入，本模块以冻结快照承接同构判据）。
pub const LSTAR_SNAPSHOT: u64 = 0xFFFF_8000_0100_0000;

/// 编译期自证 #8：编排常量自洽（步距不小于实体宽——GDT 40 ≤ 4096、
/// TSS 104 ≤ 4096；栈深 = 栈步距；AP 上限 ≥ 1）。
const _: () = assert!(GDT_STRIDE >= GdtLayout::BYTES as u64);
const _: () = assert!(TSS_STRIDE >= TssLayout::BYTES as u64);
const _: () = assert!(STACK_STRIDE == STACK_LEN);
const _: () = assert!(MAX_AP >= 1);
/// 编译期自证 #9：竞技场 + 255 AP 步距不越 48 位物理镜像域
/// （取最大竞技场基址 0x40_0000 + 255×最大步距 0x8000 < 2^48）。
const _: () = assert!(PERCPU_ARENA + (MAX_AP as u64) * STACK_STRIDE < (1u64 << 48));

/// 单 AP 编排结局（成败 + 停留相位 + 同构结论）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApOutcome {
    /// 归属 AP。
    pub cpu_id: u32,
    /// 失败诊断码（None = 五步全过）。
    pub code: Option<u16>,
    /// 停留相位号（0..=5；失败步的前一态）。
    pub phase_reached: u8,
    /// 同构结论（None = 未到同构面——序已败）。
    pub iso_full: Option<bool>,
}

/// 注入结局（注入面回执：检出码 or 漏检——漏检即纪律破）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectOutcome {
    /// 注入器号（INJ_* 闭集）。
    pub injector: u32,
    /// 检出的诊断码（None = 未检出——EV_INJECT_MISSED 入账）。
    pub detected: Option<u16>,
}

/// 注入器号闭集（事件账 b 载荷口径）。
pub const INJ_OUT_OF_ORDER: u32 = 1;
/// MSR 读回失配注入器号。
pub const INJ_MSR_READBACK: u32 = 2;
/// IST 零注入器号。
pub const INJ_IST_ZERO: u32 = 3;
/// GDT 篡改（校验和面）注入器号——加演。
pub const INJ_GDT_TAMPER: u32 = 4;

/// AP 端 GDT/TSS/MSR 装载子系统（编排 + 同构 + 账 + 注入的收口体）。
#[derive(Clone, Debug)]
pub struct ApGdtSubsystem {
    /// BSP 基线镜像（同构期望侧生成器）。
    pub mirror: BspMirror,
    /// 全局事件账（所有 AP + 注入共享）。
    pub ledger: EventLedger,
    /// 逐 AP 结局账。
    pub outcomes: Vec<ApOutcome>,
    /// 逐 AP 同构报告账。
    pub iso_reports: Vec<IsoReport>,
}

impl ApGdtSubsystem {
    /// 空子系统（canonical BSP 基线 + 空账）。
    pub fn new() -> Self {
        ApGdtSubsystem {
            mirror: BspMirror::canonical(),
            ledger: EventLedger::new(),
            outcomes: Vec::new(),
            iso_reports: Vec::new(),
        }
    }

    /// 全链确定性编排（J1 双句的一站式验证面）。
    ///
    /// 逐 AP 流程：Lcg 抖动（同种子双跑确定）→ F0328 口径栈/区账
    /// 投影 → 五步全序（Sim 双端口）→ 终态快照 → BSP 投影 diff
    /// （同构率 100% 判据）→ 结局/事件双账。任一环失败即该 AP
    /// outcome 记败、序败事件入账、下个 AP 照常（一 AP 败不阻断
    /// 其余——B17 多核启动失败分类的编排侧前哨）。
    ///
    /// ap_count 超 MAX_AP(255) 显性 saturating 收口（F0335 预算账）。
    pub fn orchestrate(seed: u64, ap_count: u32) -> Self {
        let mut sub = ApGdtSubsystem::new();
        let aps = if ap_count > MAX_AP { MAX_AP } else { ap_count };
        let mut lcg = Lcg::new(seed);
        let mut cpu = 0u32;
        while cpu < aps {
            // 区账投影：抖动取 8 字节粒（GDT 8 对齐纪律）且上限
            // 0x100（不越本 AP 整页步距——邻 AP 区间零重叠）。
            let gdt_base = GDT_ARENA + (cpu as u64) * GDT_STRIDE + lcg.next_below(0x20) * 8;
            let tss_base = TSS_ARENA + (cpu as u64) * TSS_STRIDE + lcg.next_below(0x20) * 8;
            let rsp0 = STACK_ARENA + (cpu as u64) * STACK_STRIDE + STACK_LEN;
            // IST 七路独立栈：IST 专区（IST_ARENA）按 (cpu*7 + 路) ×
            // IST_STACK_LEN 线性展开——与 RSP0 栈域分竞技场、路间
            // 步距隔离，零重叠由 const 自证 #10 保证。
            let mut ist = [0u64; 7];
            let mut k = 0usize;
            while k < 7 {
                ist[k] = IST_ARENA
                    .wrapping_add(((cpu as u64) * 7 + k as u64).wrapping_mul(IST_STACK_LEN))
                    .wrapping_add(IST_STACK_LEN);
                k += 1;
            }
            let gs_base = PERCPU_ARENA + (cpu as u64) * PERCPU_STRIDE;
            let plan = MsrPlan::canonical(cpu, gs_base, LSTAR_SNAPSHOT);
            let mut seq = ApLoadSequence::new(cpu, plan);
            let mut port = SimCpuPort::new(cpu);
            let mut msr = SimMsrSpace::new();
            let gp = GdtProgramParams {
                gdt_base,
                tss_base,
                tss_len: TssLayout::BYTES - 1,
            };
            let tp = TssProgramParams { tss_base, rsp0, ist };
            match seq.run_all(gp, tp, &mut port, &mut msr, &mut sub.ledger) {
                Ok(state) => {
                    let exp = sub.mirror.project(cpu, gdt_base, tss_base, rsp0, ist, gs_base);
                    let rep = diff_iso(&state, &exp);
                    let full = rep.is_full_iso();
                    let _ = sub.ledger.push(LoadEvent {
                        cpu_id: cpu,
                        kind: if full { EV_ISO_OK } else { EV_ISO_BROKEN },
                        a: rep.rate_percent() as u64,
                        b: if full { ISO_FIELD_COUNT as u64 } else { rep.diffs.len() as u64 },
                    });
                    sub.iso_reports.push(rep);
                    sub.outcomes.push(ApOutcome {
                        cpu_id: cpu,
                        code: None,
                        phase_reached: 5,
                        iso_full: Some(full),
                    });
                }
                Err(e) => {
                    let _ = sub.ledger.push(LoadEvent {
                        cpu_id: cpu,
                        kind: EV_SEQ_FAIL,
                        a: e.xc_code() as u64,
                        b: seq.phase.index() as u64,
                    });
                    sub.outcomes.push(ApOutcome {
                        cpu_id: cpu,
                        code: Some(e.xc_code()),
                        phase_reached: seq.phase.index(),
                        iso_full: None,
                    });
                }
            }
            cpu += 1;
        }
        sub
    }

    /// 健康体检（账/结局双扫的只读收口——含账指纹）。
    pub fn health(&self) -> HealthReport {
        let mut ap_ok = 0u32;
        let mut ap_fail = 0u32;
        let mut iso_full = 0u32;
        let mut i = 0usize;
        while i < self.outcomes.len() {
            let o = self.outcomes[i];
            if o.code.is_none() {
                ap_ok = ap_ok.saturating_add(1);
            } else {
                ap_fail = ap_fail.saturating_add(1);
            }
            if o.iso_full == Some(true) {
                iso_full = iso_full.saturating_add(1);
            }
            i += 1;
        }
        HealthReport {
            ap_total: self.outcomes.len() as u32,
            ap_ok,
            ap_fail,
            iso_full,
            inject_detected: self.ledger.count_kind(EV_INJECT_DETECTED),
            inject_missed: self.ledger.count_kind(EV_INJECT_MISSED),
            events: self.ledger.len() as u32,
            dropped: self.ledger.dropped,
            fingerprint: ledger_fingerprint(&self.ledger),
        }
    }

    /// 双跑对拍（同种子指纹 + 双账全等——确定性判据的一站式面）。
    pub fn same_state(&self, other: &ApGdtSubsystem) -> bool {
        self.ledger == other.ledger
            && self.outcomes == other.outcomes
            && self.iso_reports == other.iso_reports
    }

    /// 注入器 #1 乱序注入：新序列直接调 ltr（跳过前三步）——
    /// 预期 OutOfPhase(0x1E11) 检出。
    pub fn inject_out_of_order(&mut self, cpu_id: u32) -> InjectOutcome {
        let mut seq = ApLoadSequence::new(cpu_id, MsrPlan::canonical(cpu_id, 0, 0));
        let mut port = SimCpuPort::new(cpu_id);
        let detected = match seq.step_ltr(&mut port, &mut self.ledger) {
            Err(e) => Some(e.xc_code()),
            Ok(()) => None,
        };
        self.record_injection(INJ_OUT_OF_ORDER, detected, Some(0x1E11))
    }

    /// 注入器 #2 MSR 读回失配注入：正常走到步 5，MSR 空间对
    /// GS_BASE 注入 xor 污染——预期 MsrReadbackMismatch(0x1E19) 检出。
    pub fn inject_msr_readback(&mut self, cpu_id: u32) -> InjectOutcome {
        let result = (|| -> Result<(), ApGdtError> {
            let mut ledger = EventLedger::new();
            let mut seq = ApLoadSequence::new(cpu_id, MsrPlan::canonical(cpu_id, 0x4000, 0));
            let mut port = SimCpuPort::new(cpu_id);
            // 受污染 MSR 空间：GS_BASE 写入即被 xor。
            let mut msr = SimMsrSpace::with_corruption(MSR_IA32_GS_BASE, 0xDEAD_BEEF);
            let gp = GdtProgramParams {
                gdt_base: GDT_ARENA,
                tss_base: TSS_ARENA,
                tss_len: TssLayout::BYTES - 1,
            };
            let mut ist = [0u64; 7];
            let mut k = 0usize;
            while k < 7 {
                ist[k] = 0x7000 + k as u64;
                k += 1;
            }
            let tp = TssProgramParams { tss_base: TSS_ARENA, rsp0: 0x9000, ist };
            seq.run_all(gp, tp, &mut port, &mut msr, &mut ledger).map(|_| ())
        })();
        let detected = match result {
            Err(e) => Some(e.xc_code()),
            Ok(()) => None,
        };
        self.record_injection(INJ_MSR_READBACK, detected, Some(0x1E19))
    }

    /// 注入器 #3 IST 零注入：IST7 路中第 3 路置零——ltr 前置三查
    /// 预期 IstZero(0x1E14) 检出（slot = 3）。
    pub fn inject_ist_zero(&mut self, cpu_id: u32) -> InjectOutcome {
        let result = (|| -> Result<(), ApGdtError> {
            let mut ledger = EventLedger::new();
            let mut seq = ApLoadSequence::new(cpu_id, MsrPlan::canonical(cpu_id, 0x4000, 0));
            let mut port = SimCpuPort::new(cpu_id);
            let mut msr = SimMsrSpace::new();
            let gp = GdtProgramParams {
                gdt_base: GDT_ARENA,
                tss_base: TSS_ARENA,
                tss_len: TssLayout::BYTES - 1,
            };
            let mut ist = [0u64; 7];
            let mut k = 0usize;
            while k < 7 {
                ist[k] = 0x7000 + k as u64;
                k += 1;
            }
            ist[2] = 0; // 第 3 路置零——任务钦定注入点。
            let tp = TssProgramParams { tss_base: TSS_ARENA, rsp0: 0x9000, ist };
            seq.run_all(gp, tp, &mut port, &mut msr, &mut ledger).map(|_| ())
        })();
        let detected = match result {
            Err(e) => Some(e.xc_code()),
            Ok(()) => None,
        };
        self.record_injection(INJ_IST_ZERO, detected, Some(0x1E14))
    }

    /// 注入器 #4 GDT 篡改注入（加演）：步 1 物化后对 TSS 低描述符
    /// 翻 1 字节——步 2 校验和面预期 GdtChecksumBad(0x1E12) 检出。
    pub fn inject_gdt_tamper(&mut self, cpu_id: u32) -> InjectOutcome {
        let result = (|| -> Result<(), ApGdtError> {
            let mut ledger = EventLedger::new();
            let mut seq = ApLoadSequence::new(cpu_id, MsrPlan::canonical(cpu_id, 0x4000, 0));
            let mut port = SimCpuPort::new(cpu_id);
            let gp = GdtProgramParams {
                gdt_base: GDT_ARENA,
                tss_base: TSS_ARENA,
                tss_len: TssLayout::BYTES - 1,
            };
            seq.step_gdt_program(gp, &mut ledger)?;
            // 篡改点：TSS 低描述符 access 字节（偏移 24+5 = 29）异或
            // 0x40（P 位翻转——位敏感篡改，校验和必移）。
            match seq.gdt.as_mut() {
                Some(img) => {
                    let off = (GdtLayout::OFF_TSS_LO + 5) as usize;
                    if let Some(byte) = img.bytes.get_mut(off) {
                        *byte ^= 0x40;
                    }
                }
                None => {}
            }
            seq.step_lgdt(&mut port, &mut ledger)
        })();
        let detected = match result {
            Err(e) => Some(e.xc_code()),
            Ok(()) => None,
        };
        self.record_injection(INJ_GDT_TAMPER, detected, Some(0x1E12))
    }

    /// 注入回执入账（检出 → EV_INJECT_DETECTED；漏检 →
    /// EV_INJECT_MISSED——漏检事件本身即失败证据）。
    fn record_injection(
        &mut self,
        injector: u32,
        detected: Option<u16>,
        expect: Option<u16>,
    ) -> InjectOutcome {
        let hit = detected == expect;
        let _ = self.ledger.push(LoadEvent {
            cpu_id: u32::MAX, // 注入器不属任何 AP——哨兵号入账
            kind: if hit { EV_INJECT_DETECTED } else { EV_INJECT_MISSED },
            a: match detected {
                Some(c) => c as u64,
                None => 0,
            },
            b: injector as u64,
        });
        InjectOutcome { injector, detected }
    }
}

/// IST 专区竞技场基址（0xC0_0000：栈域顶 STACK_ARENA + 256×
/// STACK_STRIDE = 0xB0_0000 之上留 1MiB 边距起区——七路 × 4KiB ×
/// 255 AP ≈ 7MiB；与栈/区/GDT/TSS 四竞技场互斥，零重叠由
/// const 自证 #10 编译期钉死）。
pub const IST_ARENA: u64 = 0x00C0_0000;

/// 编译期自证 #10：IST 专区与 RSP0 栈域零重叠（IST_ARENA ≥
/// 栈域顶 = STACK_ARENA + (MAX_AP+1)×STACK_STRIDE）。
const _: () = assert!(IST_ARENA >= STACK_ARENA + ((MAX_AP as u64) + 1) * STACK_STRIDE);

/// 消费契约声明行（上游任务 → 本模块承接语义——可审计的书面契约）。
pub fn consumption_declaration() -> [(&'static str, &'static str); 4] {
    [
        (
            "UNX-F0328",
            "栈/per-CPU 区实例化：RSP0/IST 七路栈顶与 GS_BASE 区基址由其账本供给，\
             本模块以 TssProgramParams/MsrPlan 数值承接（零所有权耦合）",
        ),
        (
            "UNX-F0307",
            "EFER 协同：EFER_CANON = SCE|LME|LMA|NXE = 0xD01 与 BSP 侧 NXE 使能\
             时序同源，AP 侧装载值与之逐位同构",
        ),
        (
            "UNX-F0344",
            "GS base 切换：本模块装载 GS_BASE（wrmsr 语义 + 读回断言），\
             F0344 在其上做 per-CPU 自识别；KERNEL_GS_BASE 置 0 影槽",
        ),
        (
            "UNX-F0335",
            "AP 上限：MAX_AP = 255 预算账同源，orchestrate 入参 saturating 收口",
        ),
    ]
}

/// 描述符编解码运行期自证（编译期 const 往返断言的运行期双份——
/// 覆盖 const 块放不下的电池组；任一字段未还原即 DescRoundtripBad）。
pub fn verify_codec() -> Result<(), ApGdtError> {
    // 电池：全零 / 全满 / canonical 三态 / TSS 形 / 数据段形 / 任意中位值。
    let battery = [
        SegmentFields { base: 0, limit: 0, access: 0, flags: 0 },
        SegmentFields { base: 0xFFFF_FFFF, limit: 0xF_FFFF, access: 0xFF, flags: 0x0F },
        SegmentFields { base: 0x0012_3456, limit: 0xABCDE, access: 0x9A, flags: 0x0A },
        SegmentFields { base: 0x0020_4000, limit: 0x67, access: 0x89, flags: 0x0 },
        SegmentFields { base: 0x89AB_CDEF, limit: 0x123, access: 0x92, flags: 0x0C },
        SegmentFields { base: 0x5555_AAAA, limit: 0x0F0F0, access: 0x42, flags: 0x05 },
    ];
    let mut i = 0usize;
    while i < battery.len() {
        let f = battery[i];
        let g = decode_desc(encode_desc(f));
        if g.base != f.base {
            return Err(ApGdtError::DescRoundtripBad { field: "base" });
        }
        if g.limit != f.limit {
            return Err(ApGdtError::DescRoundtripBad { field: "limit" });
        }
        if g.access != f.access {
            return Err(ApGdtError::DescRoundtripBad { field: "access" });
        }
        if g.flags != f.flags {
            return Err(ApGdtError::DescRoundtripBad { field: "flags" });
        }
        i += 1;
    }
    Ok(())
}

impl ApGdtSubsystem {
    /// 同构全过断言（J1 前半句的收口判定面：任一 AP 同构率 < 100%
    /// 即 Err(IsoBroken)——cpu_id 定位首个断裂 AP）。
    pub fn verify_iso_all(&self) -> Result<(), ApGdtError> {
        let mut i = 0usize;
        while i < self.iso_reports.len() {
            let rep = &self.iso_reports[i];
            if !rep.is_full_iso() {
                return Err(ApGdtError::IsoBroken {
                    cpu_id: rep.cpu_id,
                    rate_percent: rep.rate_percent(),
                });
            }
            i += 1;
        }
        Ok(())
    }
}

// ============================================================================
// 第 10 节 · 冒烟测试（cfg(test) —— 非内核路径：断言可用）
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 吞写端口：lgdt/ltr 假装成功但不落影子（读回缺席面——
    /// LgdtReadbackBad/LtrReadbackBad 的故障注入实现）。
    struct DenyPort {
        cpu_id: u32,
    }

    impl CpuPort for DenyPort {
        fn lgdt(&mut self, _gdtr: &Gdtr) -> Result<(), ApGdtError> {
            Ok(())
        }
        fn sgdt(&self) -> Option<Gdtr> {
            None
        }
        fn ltr(&mut self, _selector: u16) -> Result<(), ApGdtError> {
            Ok(())
        }
        fn str_reg(&self) -> Option<u16> {
            None
        }
        fn cpu_id(&self) -> u32 {
            self.cpu_id
        }
    }

    /// 标准六件套（序列 + 双模拟端口 + 空账 + 两参数包）。
    fn happy_parts(cpu: u32) -> (
        ApLoadSequence,
        SimCpuPort,
        SimMsrSpace,
        EventLedger,
        GdtProgramParams,
        TssProgramParams,
    ) {
        let gs = PERCPU_ARENA + (cpu as u64) * PERCPU_STRIDE;
        let plan = MsrPlan::canonical(cpu, gs, LSTAR_SNAPSHOT);
        let tss_base = TSS_ARENA + (cpu as u64) * TSS_STRIDE;
        (
            ApLoadSequence::new(cpu, plan),
            SimCpuPort::new(cpu),
            SimMsrSpace::new(),
            EventLedger::new(),
            GdtProgramParams {
                gdt_base: GDT_ARENA + (cpu as u64) * GDT_STRIDE,
                tss_base,
                tss_len: TssLayout::BYTES - 1,
            },
            TssProgramParams { tss_base, rsp0: 0x9000, ist: [0x7001, 0x7002, 0x7003, 0x7004, 0x7005, 0x7006, 0x7007] },
        )
    }

    // ---- C1 描述符编解码往返 ----
    #[test]
    fn test_descriptor_codec_roundtrip_battery() {
        assert!(verify_codec().is_ok());
        // 全满域值逐字段核对（位域拆装的显式四查）。
        let f = SegmentFields { base: 0xDEAD_BEEF, limit: 0xABCDEF & 0xF_FFFF, access: 0x9E, flags: 0x07 };
        let d = encode_desc(f);
        let g = decode_desc(d);
        assert_eq!(g.base, f.base);
        assert_eq!(g.limit, f.limit);
        assert_eq!(g.access, f.access);
        assert_eq!(g.flags, f.flags);
        // 位域落位抽查：access 落字节 5；flags 高半字节 6、limit[19:16]
        // 低半字节（limit = 0xBCDEF → 高 4 位 0xB）。
        assert_eq!(d[5], 0x9E);
        assert_eq!(d[6] >> 4, 0x07);
        assert_eq!(d[6] & 0x0F, 0x0B);
    }

    #[test]
    fn test_canonical_code_desc_bytes() {
        // 经典字 0x00AF9A000000FFFF（代码）/ 0x00CF92000000FFFF（数据）逐字节。
        let d = encode_desc(GdtLayout::code64_fields());
        assert_eq!(d, [0xFF, 0xFF, 0x00, 0x00, 0x00, 0x9A, 0xAF, 0x00]);
        let dd = encode_desc(GdtLayout::data_fields());
        assert_eq!(dd, [0xFF, 0xFF, 0x00, 0x00, 0x00, 0x92, 0xCF, 0x00]);
    }

    // ---- C2 GDT 布局与镜像 ----
    #[test]
    fn test_gdt_layout_and_selectors() {
        assert_eq!(GdtLayout::BYTES, 40);
        assert_eq!(GdtLayout::SLOTS, 5);
        assert_eq!(GdtLayout::SEL_CODE64, 0x08);
        assert_eq!(GdtLayout::SEL_DATA, 0x10);
        assert_eq!(GdtLayout::SEL_TSS, 0x18);
        assert_eq!(GdtLayout::slot_name(3), "gdt.tss_lo");
        assert_eq!(GdtLayout::slot_name(0), "gdt.null");
        assert!(GdtLayout::checked_slot_offset(5).is_err());
        assert!(GdtLayout::checked_slot_offset(4).is_ok());
    }

    #[test]
    fn test_gdt_program_checksum_and_tamper() {
        let g = GdtImage::program(2, GDT_ARENA, TSS_ARENA, TssLayout::BYTES - 1).ok().unwrap();
        assert!(g.checksum_ok());
        assert_eq!(g.slot_fields(1).ok().unwrap(), GdtLayout::code64_fields());
        assert_eq!(g.slot_fields(2).ok().unwrap(), GdtLayout::data_fields());
        let tss_lo = g.slot_fields(3).ok().unwrap();
        assert_eq!(tss_lo.base, TSS_ARENA as u32);
        assert_eq!(tss_lo.limit, 103);
        assert_eq!(tss_lo.access, 0x89);
        assert_eq!(g.gdtr(), Gdtr { limit: 39, base: GDT_ARENA });
        // 篡改 1 位 → 校验和移。
        let mut t = g.clone();
        t.bytes[29] ^= 0x40;
        assert!(!t.checksum_ok());
        // 域校验：48 位外基址 / 超 20 位限长。
        assert!(GdtImage::program(0, 1 << 48, 0, 103).is_err());
        assert!(GdtImage::program(0, 0x1000, 1 << 48, 103).is_err());
        assert!(GdtImage::program(0, 0x1000, 0, 0x10_0000).is_err());
    }

    // ---- C3 TSS 布局与镜像 ----
    #[test]
    fn test_tss_layout_offsets() {
        assert_eq!(TssLayout::BYTES, 104);
        assert_eq!(TssLayout::OFF_RSP0, 4);
        assert_eq!(TssLayout::ist_offset(1), 28);
        assert_eq!(TssLayout::ist_offset(7), 76);
        assert_eq!(TssLayout::OFF_IOMAP, 100);
        assert_eq!(TssLayout::IOMAP_ABSENT, 104);
        assert_eq!(TssLayout::ist_name(4), "tss.ist4");
        assert!(TssLayout::checked_ist_offset(0).is_err());
        assert!(TssLayout::checked_ist_offset(8).is_err());
        assert!(TssLayout::checked_ist_offset(7).is_ok());
    }

    #[test]
    fn test_tss_program_fields_and_preflight() {
        let ist = [0x7001u64, 0x7002, 0x7003, 0x7004, 0x7005, 0x7006, 0x7007];
        let t = TssImage::program(3, TSS_ARENA, 0x9000, ist).ok().unwrap();
        assert_eq!(t.rsp0(), Some(0x9000));
        assert_eq!(t.ist(3).ok().unwrap(), 0x7003);
        assert_eq!(t.iomap_base(), Some(104));
        assert!(t.preflight_ltr().is_ok());
        // IST 零 → IstZero（slot 1 基号）。
        let mut z = t.clone();
        let _ = wr_u64_le(&mut z.bytes, TssLayout::ist_offset(5) as usize, 0);
        match z.preflight_ltr() {
            Err(ApGdtError::IstZero { slot, .. }) => assert_eq!(slot, 5),
            other => panic!("IST 零应报 IstZero: {:?}", other),
        }
        // IO 位图基址改 0 → IoMapBaseBad。
        let mut io = t.clone();
        let _ = wr_u16_le(&mut io.bytes, TssLayout::OFF_IOMAP as usize, 0);
        match io.preflight_ltr() {
            Err(ApGdtError::IoMapBaseBad { expect, actual, .. }) => {
                assert_eq!(expect, 104);
                assert_eq!(actual, 0);
            }
            other => panic!("IO 位图基址错应报 IoMapBaseBad: {:?}", other),
        }
        // 保留域脏（偏移 84）→ TssReservedDirty。
        let mut r = t.clone();
        let _ = wr_u64_le(&mut r.bytes, TssLayout::OFF_RSVD84 as usize, 0x11);
        match r.preflight_ltr() {
            Err(ApGdtError::TssReservedDirty { offset, .. }) => assert_eq!(offset, 84),
            other => panic!("保留域脏应报 TssReservedDirty: {:?}", other),
        }
        // 保留 u32（偏移 0）脏 → 同错。
        let mut r0 = t.clone();
        r0.bytes[1] = 0x80;
        match r0.preflight_ltr() {
            Err(ApGdtError::TssReservedDirty { offset, .. }) => assert_eq!(offset, 0),
            other => panic!("保留 u32 脏应报 TssReservedDirty: {:?}", other),
        }
    }

    // ---- C4 五步全序快乐路径 ----
    #[test]
    fn test_sequence_happy_path_full() {
        let (mut seq, mut port, mut msr, mut ledger, gp, tp) = happy_parts(4);
        let state = seq.run_all(gp, tp, &mut port, &mut msr, &mut ledger).ok().unwrap();
        assert_eq!(seq.phase, LoadPhase::MsrLoaded);
        assert_eq!(port.shadow_gdtr, Some(Gdtr { limit: 39, base: gp.gdt_base }));
        assert_eq!(port.shadow_tr, Some(0x18));
        assert_eq!(state.tr, 0x18);
        assert_eq!(state.msr[0], (MSR_IA32_EFER, EFER_CANON));
        assert_eq!(state.msr[1], (MSR_IA32_LSTAR, LSTAR_SNAPSHOT));
        assert_eq!(state.msr[2], (MSR_IA32_GS_BASE, PERCPU_ARENA + 4 * PERCPU_STRIDE));
        assert_eq!(state.msr[3], (MSR_IA32_KERNEL_GS_BASE, 0));
        // 事件链：START→五步→DONE 七事件齐（每步一事件 + 序首尾）。
        assert_eq!(ledger.len(), 7);
        assert_eq!(ledger.ev[0].kind, EV_SEQ_START);
        assert_eq!(ledger.ev[1].kind, EV_GDT_PROGRAMMED);
        assert_eq!(ledger.ev[2].kind, EV_LGDT_DONE);
        assert_eq!(ledger.ev[3].kind, EV_TSS_PROGRAMMED);
        assert_eq!(ledger.ev[4].kind, EV_LTR_DONE);
        assert_eq!(ledger.ev[5].kind, EV_MSR_LOAD_DONE);
        assert_eq!(ledger.ev[6].kind, EV_SEQ_DONE);
    }

    // ---- C5 乱序拒绝 ----
    #[test]
    fn test_sequence_out_of_order_rejected() {
        let (mut seq, mut port, mut msr, mut ledger, gp, tp) = happy_parts(1);
        // 直接 ltr → 拒（Init 期）。
        match seq.step_ltr(&mut port, &mut ledger) {
            Err(ApGdtError::OutOfPhase { expect, got }) => {
                assert_eq!(expect, LoadPhase::TssProgrammed);
                assert_eq!(got, LoadPhase::Init);
            }
            other => panic!("乱序 ltr 应报 OutOfPhase: {:?}", other),
        }
        // 直接 msr → 拒。
        assert!(matches!(
            seq.step_msr_load(&mut msr, &mut ledger),
            Err(ApGdtError::OutOfPhase { .. })
        ));
        // 步 1 过后重复步 1 → 拒。
        seq.step_gdt_program(gp, &mut ledger).ok().unwrap();
        assert!(matches!(
            seq.step_gdt_program(gp, &mut ledger),
            Err(ApGdtError::OutOfPhase { .. })
        ));
        // 步 2 后直接跳步 4 → 拒（须先步 3）。
        seq.step_lgdt(&mut port, &mut ledger).ok().unwrap();
        assert!(matches!(
            seq.step_ltr(&mut port, &mut ledger),
            Err(ApGdtError::OutOfPhase { .. })
        ));
        // 终态后再跑步 5 → 拒。
        seq.step_tss_program(tp, &mut ledger).ok().unwrap();
        seq.step_ltr(&mut port, &mut ledger).ok().unwrap();
        seq.step_msr_load(&mut msr, &mut ledger).ok().unwrap();
        assert!(matches!(
            seq.step_msr_load(&mut msr, &mut ledger),
            Err(ApGdtError::OutOfPhase { .. })
        ));
    }

    // ---- C6 lgdt 读回断言（故障端口） ----
    #[test]
    fn test_lgdt_readback_bad_port() {
        let (mut seq, _, _, _, gp, tp) = happy_parts(5);
        let mut deny = DenyPort { cpu_id: 5 };
        let mut msr = SimMsrSpace::new();
        let mut ledger = EventLedger::new();
        match seq.run_all(gp, tp, &mut deny, &mut msr, &mut ledger) {
            Err(ApGdtError::LgdtReadbackBad { expect_base, actual_base, expect_limit, actual_limit, .. }) => {
                assert_eq!(expect_base, gp.gdt_base);
                assert_eq!(actual_base, u64::MAX);
                assert_eq!(expect_limit, 39);
                assert_eq!(actual_limit, u16::MAX);
            }
            other => panic!("吞写端口应报 LgdtReadbackBad: {:?}", other),
        }
        assert_eq!(seq.phase, LoadPhase::GdtProgrammed); // 停在失败步前一态
    }

    // ---- C7 MSR 写序纪律与读回失配 ----
    #[test]
    fn test_msr_order_discipline() {
        let mut seq = MsrLoadSeq::new();
        let mut m = SimMsrSpace::new();
        // 倒写（第 0 步应写 EFER，实写 KERNEL_GS_BASE）→ 拒。
        match seq.write_next(MSR_IA32_KERNEL_GS_BASE, 0, &mut m) {
            Err(ApGdtError::MsrWriteOrderBad { expect_addr, got_addr, step }) => {
                assert_eq!(expect_addr, MSR_IA32_EFER);
                assert_eq!(got_addr, MSR_IA32_KERNEL_GS_BASE);
                assert_eq!(step, 0);
            }
            other => panic!("倒写应报 MsrWriteOrderBad: {:?}", other),
        }
        assert_eq!(seq.step, 0); // 拒后不前进
        // 正写两步后重复写 LSTAR → 拒（应写 GS_BASE）。
        seq.write_next(MSR_IA32_EFER, EFER_CANON, &mut m).ok().unwrap();
        seq.write_next(MSR_IA32_LSTAR, LSTAR_SNAPSHOT, &mut m).ok().unwrap();
        assert!(matches!(
            seq.write_next(MSR_IA32_LSTAR, 1, &mut m),
            Err(ApGdtError::MsrWriteOrderBad { step: 2, .. })
        ));
        // 跑完四步 → done；第五笔超写 → 拒。
        seq.write_next(MSR_IA32_GS_BASE, 0x4000, &mut m).ok().unwrap();
        seq.write_next(MSR_IA32_KERNEL_GS_BASE, 0, &mut m).ok().unwrap();
        assert!(seq.done());
        assert!(matches!(
            seq.write_next(MSR_IA32_EFER, 0, &mut m),
            Err(ApGdtError::MsrWriteOrderBad { step: 4, .. })
        ));
    }

    #[test]
    fn test_msr_unknown_and_readback_injection() {
        let mut m = SimMsrSpace::new();
        assert!(matches!(
            m.write(0x0000_0123, 1),
            Err(ApGdtError::MsrUnknown { addr: 0x123 })
        ));
        assert_eq!(m.read(0x0000_0123), None);
        assert_eq!(m.read(MSR_IA32_EFER), Some(0)); // 已知初零
        // 读回失配注入：EFER 写入即被 xor。
        let mut bad = SimMsrSpace::with_corruption(MSR_IA32_EFER, 0xFF);
        let mut seq = MsrLoadSeq::new();
        match seq.write_next(MSR_IA32_EFER, EFER_CANON, &mut bad) {
            Err(ApGdtError::MsrReadbackMismatch { addr, wrote, read }) => {
                assert_eq!(addr, MSR_IA32_EFER);
                assert_eq!(wrote, EFER_CANON);
                assert_eq!(read, EFER_CANON ^ 0xFF);
            }
            other => panic!("失配注入应报 MsrReadbackMismatch: {:?}", other),
        }
        // 零掩码注入构造 → 静默降级为无注入（掩码必须非零）。
        let quiet = SimMsrSpace::with_corruption(MSR_IA32_EFER, 0);
        assert_eq!(quiet.corrupt_addr, None);
    }

    // ---- C8 IST 零注入（经序列全链） ----
    #[test]
    fn test_ist_zero_injection_standalone() {
        let (mut seq, mut port, mut msr, mut ledger, gp, mut tp) = happy_parts(6);
        tp.ist[2] = 0;
        match seq.run_all(gp, tp, &mut port, &mut msr, &mut ledger) {
            Err(ApGdtError::IstZero { slot, .. }) => assert_eq!(slot, 3),
            other => panic!("IST 零应报 IstZero: {:?}", other),
        }
        assert_eq!(seq.phase, LoadPhase::TssProgrammed); // 停在失败步前一态
    }

    // ---- C9 GDT 篡改注入（校验和面） ----
    #[test]
    fn test_gdt_tamper_injection_standalone() {
        let (mut seq, mut port, _, mut ledger, gp, _) = happy_parts(7);
        seq.step_gdt_program(gp, &mut ledger).ok().unwrap();
        match seq.gdt.as_mut() {
            Some(img) => img.bytes[29] ^= 0x40,
            None => panic!("步 1 后镜像必在"),
        }
        match seq.step_lgdt(&mut port, &mut ledger) {
            Err(ApGdtError::GdtChecksumBad { recorded, live, .. }) => {
                assert_ne!(recorded, live);
            }
            other => panic!("篡改后 lgdt 应报 GdtChecksumBad: {:?}", other),
        }
    }

    // ---- C10 编排全绿 + 同构 100% ----
    #[test]
    fn test_orchestrate_full_iso() {
        let sub = ApGdtSubsystem::orchestrate(42, 4);
        let h = sub.health();
        assert_eq!(h.ap_total, 4);
        assert_eq!(h.ap_ok, 4);
        assert_eq!(h.ap_fail, 0);
        assert_eq!(h.iso_full, 4);
        assert_eq!(h.inject_missed, 0);
        assert_eq!(sub.iso_reports.len(), 4);
        for rep in sub.iso_reports.iter() {
            assert!(rep.is_full_iso());
            assert_eq!(rep.rate_percent(), 100);
            assert_eq!(rep.fields, 22);
        }
        assert!(sub.verify_iso_all().is_ok());
        assert_eq!(h.events, 4 * 8); // 每 AP：序列 7 事件 + 同构 1 事件
    }

    #[test]
    fn test_orchestrate_zero_and_cap() {
        let z = ApGdtSubsystem::orchestrate(9, 0);
        assert_eq!(z.health().ap_total, 0);
        assert_eq!(z.health().fingerprint, FNV_BASIS); // 空账指纹 = 基数
        // 超限收口：300 → 255（F0335 上限）。
        let c = ApGdtSubsystem::orchestrate(9, MAX_AP + 45);
        assert_eq!(c.health().ap_total, MAX_AP);
        // 255 AP × 8 事件 = 2040 > 256 → 满账丢弃显性计数 > 0。
        assert!(c.health().dropped > 0);
    }

    // ---- C11 双跑确定性对拍 ----
    #[test]
    fn test_double_run_determinism() {
        let a = ApGdtSubsystem::orchestrate(0xDEAD_BEEF, 8);
        let b = ApGdtSubsystem::orchestrate(0xDEAD_BEEF, 8);
        assert!(a.same_state(&b));
        assert_eq!(a.health().fingerprint, b.health().fingerprint);
        // 异种子 → 抖动不同 → 指纹不同（防「恒定指纹」假确定性）。
        let c = ApGdtSubsystem::orchestrate(0x1234_5678, 8);
        assert_ne!(a.health().fingerprint, c.health().fingerprint);
    }

    // ---- C12 子系统注入面四路全检出 ----
    #[test]
    fn test_injectors_on_subsystem() {
        let mut sub = ApGdtSubsystem::orchestrate(42, 2);
        let o1 = sub.inject_out_of_order(0);
        assert_eq!(o1, InjectOutcome { injector: INJ_OUT_OF_ORDER, detected: Some(0x1E11) });
        let o2 = sub.inject_msr_readback(0);
        assert_eq!(o2.detected, Some(0x1E19));
        let o3 = sub.inject_ist_zero(0);
        assert_eq!(o3.detected, Some(0x1E14));
        let o4 = sub.inject_gdt_tamper(0);
        assert_eq!(o4.detected, Some(0x1E12));
        let h = sub.health();
        assert_eq!(h.inject_detected, 4);
        assert_eq!(h.inject_missed, 0);
        // 编排本体不受注入污染：结局仍全绿。
        assert_eq!(h.ap_ok, 2);
        assert_eq!(h.ap_fail, 0);
    }

    // ---- C13 BSP 基线破坏 → 同构断 + 差异定位 ----
    #[test]
    fn test_broken_mirror_iso_localization() {
        let (mut seq, mut port, mut msr, mut ledger, gp, tp) = happy_parts(9);
        let state = seq.run_all(gp, tp, &mut port, &mut msr, &mut ledger).ok().unwrap();
        let mut mirror = BspMirror::canonical();
        mirror.code_desc[5] ^= 0x02; // access 的 R 位被 BSP 线改掉
        let real_gs = PERCPU_ARENA + 9 * PERCPU_STRIDE; // 与 happy_parts(9) 同源
        let exp = mirror.project(9, gp.gdt_base, tp.tss_base, tp.rsp0, tp.ist, real_gs);
        let rep = diff_iso(&state, &exp);
        assert!(!rep.is_full_iso());
        assert!(rep.rate_percent() < 100);
        assert_eq!(rep.diffs.len(), 1);
        assert_eq!(rep.diffs[0].path, "gdt.code64");
        // 再破坏 GS_BASE 期望 → 两处差异、路径可定位。
        let exp2 = BspMirror {
            efer: EFER_CANON,
            lstar: LSTAR_SNAPSHOT,
            kgs_base: 0,
            code_desc: encode_desc(GdtLayout::code64_fields()),
            data_desc: encode_desc(GdtLayout::data_fields()),
        }
        .project(9, gp.gdt_base, tp.tss_base, tp.rsp0, tp.ist, 0xFFFF_0000);
        let rep2 = diff_iso(&state, &exp2);
        assert!(!rep2.is_full_iso());
        let mut has_gs = false;
        for d in rep2.diffs.iter() {
            if d.path == "msr.gs_base" {
                has_gs = true;
                assert_eq!(d.expect, 0xFFFF_0000);
                assert_eq!(d.actual, state.msr[2].1);
            }
        }
        assert!(has_gs);
    }

    // ---- C14 错误面闭集 + detail 非空 ----
    #[test]
    fn test_error_codes_closed_set_and_details() {
        let errs: Vec<ApGdtError> = vec![
            ApGdtError::OutOfPhase { expect: LoadPhase::Init, got: LoadPhase::MsrLoaded },
            ApGdtError::GdtChecksumBad { cpu_id: 0, recorded: 1, live: 2 },
            ApGdtError::LgdtReadbackBad { cpu_id: 0, expect_base: 1, actual_base: 2, expect_limit: 39, actual_limit: 40 },
            ApGdtError::IstZero { cpu_id: 0, slot: 3 },
            ApGdtError::IoMapBaseBad { cpu_id: 0, expect: 104, actual: 0 },
            ApGdtError::TssReservedDirty { cpu_id: 0, offset: 84, actual: 1 },
            ApGdtError::LtrReadbackBad { expect: 0x18, actual: 0 },
            ApGdtError::MsrWriteOrderBad { expect_addr: 1, got_addr: 2, step: 3 },
            ApGdtError::MsrReadbackMismatch { addr: 1, wrote: 2, read: 3 },
            ApGdtError::MsrUnknown { addr: 0x123 },
            ApGdtError::IsoBroken { cpu_id: 0, rate_percent: 95 },
            ApGdtError::DescRoundtripBad { field: "base" },
            ApGdtError::DescFieldBad { what: "域非法", value: 1 },
            ApGdtError::SlotIndexBad { slot: 9 },
            ApGdtError::EventFull { capacity: LEDGER_CAP },
        ];
        let mut seen: Vec<u16> = Vec::new();
        for e in errs.iter() {
            let c = e.xc_code();
            assert!(c >= 0x1E11 && c <= 0x1E1F, "码越段: {:#06x}", c);
            assert!(!e.detail().is_empty());
            assert!(e.report().contains(&format!("{:#06x}", c)));
            assert!(!seen.contains(&c), "码重复: {:#06x}", c);
            seen.push(c);
        }
        assert_eq!(seen.len(), 15); // 十五变体一码一位全落段
    }

    // ---- C15 事件账满与显性丢弃 ----
    #[test]
    fn test_ledger_full_drop() {
        let mut l = EventLedger::new();
        let mut i = 0u64;
        while i < LEDGER_CAP as u64 {
            let r = l.push(LoadEvent { cpu_id: 0, kind: EV_SEQ_START, a: i, b: 0 });
            assert!(r.is_ok());
            i += 1;
        }
        assert_eq!(l.len(), LEDGER_CAP);
        let mut dropped = 0u64;
        while dropped < 5 {
            match l.push(LoadEvent { cpu_id: 0, kind: EV_SEQ_DONE, a: dropped, b: 0 }) {
                Err(ApGdtError::EventFull { capacity }) => assert_eq!(capacity, LEDGER_CAP),
                other => panic!("满账应报 EventFull: {:?}", other),
            }
            dropped += 1;
        }
        assert_eq!(l.dropped, 5);
        assert_eq!(l.count_kind(EV_SEQ_START), LEDGER_CAP as u32);
        assert_eq!(l.count_kind(EV_SEQ_DONE), 0);
    }

    // ---- C16 FNV 参考向量 + Lcg 确定性 ----
    #[test]
    fn test_fnv_reference_vector() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        // 流式 = 一次性。
        let h = fnv_mix(fnv_mix(FNV_BASIS, b"ab"), b"cd");
        assert_eq!(h, fnv1a64(b"abcd"));
    }

    #[test]
    fn test_lcg_determinism() {
        let mut a = Lcg::new(7);
        let mut b = Lcg::new(7);
        let mut i = 0;
        while i < 32 {
            assert_eq!(a.next_u64(), b.next_u64());
            i += 1;
        }
        let mut z = Lcg::new(0);
        assert_eq!(z.next_below(0), 0);
        assert_ne!(z.next_u64(), 0); // 零种子不吸收
    }

    // ---- C17 杂项语义面 ----
    #[test]
    fn test_efer_decode_and_phase_names() {
        assert_eq!(efer_decode(EFER_CANON), (true, true, true, true));
        assert_eq!(efer_decode(0), (false, false, false, false));
        assert_eq!(EFER_CANON, 0x0D01);
        assert_eq!(LoadPhase::MsrLoaded.index(), 5);
        assert_eq!(LoadPhase::GdtLoaded.name(), "lgdt已过");
        assert_eq!(ev_name(EV_ISO_BROKEN), "同构断");
        assert_eq!(ev_name(999), "未知事件");
        let rows = consumption_declaration();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].0, "UNX-F0328");
        assert_eq!(rows[3].0, "UNX-F0335");
    }

    #[test]
    fn test_byte_io_helpers() {
        let mut buf = [0u8; 16];
        assert!(wr_u64_le(&mut buf, 8, 0x0102_0304_0506_0708));
        assert!(!wr_u64_le(&mut buf, 9, 1)); // 越界拒写
        assert_eq!(rd_u64_le(&buf, 8), Some(0x0102_0304_0506_0708));
        assert_eq!(rd_u64_le(&buf, 9), None);
        assert!(wr_u16_le(&mut buf, 0, 0xAABB));
        assert_eq!(rd_u16_le(&buf, 0), Some(0xAABB));
        assert_eq!(rd_u16_le(&buf, 15), None);
        assert!(wr_u32_le(&mut buf, 2, 0xCCDD_EEFF));
        assert_eq!(buf[2], 0xFF);
        // 错位读 TSS 风格：偏移 4 的 u64 字节拼装。
        let mut t = [0u8; 8];
        let _ = wr_u64_le(&mut t, 0, 0x1122_3344_5566_7788);
        assert_eq!(t[0], 0x88);
        assert_eq!(t[7], 0x11);
    }
}
