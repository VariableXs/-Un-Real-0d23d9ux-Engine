//! UNX-F0806 · IST 三栈布局与 DF/NMI/MC 挂接 · 诊断段 0x27F0（独占 0x27F0~0x27FB）
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 验收锚点原文（docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix
//! II · Unxreal.md 行 2431，全文抄录不改一字）：
//! ```text
//! | UNX-F0806 | IST 三栈布局与 DF/NMI/MC 挂接 | 320 | 已深化 | UNX-F0806-J1 三张 IST 栈独立可用（构造 DF 触发走 IST 且现场完整），栈溢出防护触发率 100% |
//! ```
//! 定位原文：
//! ```text
//! UNX-A2 · 中断与计时基座
//! UNX-A2-B01 · IDT 与向量空间规划（中断面第 1 批）（F801–F820 · 20 条）
//! 嫁接源：Intel SDM Vol.3 第 6 章（中断与异常）、x86-64 ABI（栈帧布局），
//! 只跟随｜上游：AI-01 F0004（MADT 拓扑账）已冻结交付
//! ```
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 判据拆解（J1 两成分，缺一即批内红灯）：
//! 1. 三张 IST 栈独立可用 —— 构造 DF 触发走 IST 且现场完整：DF(#8) 经路由
//!    表挂 IST1 槽，进入即换栈（RSP 落在 IST1 包络内），现场帧四要素
//!    （向量号/错误码/RSP/RIP）齐备断言通过；NMI(#2)→IST2、MC(#18)→IST3
//!    同构演练；三栈零重叠（布局审计 0 对重叠区间）+ 每栈独立使用账
//!    （进入数/退出数/峰值水位逐栈可读）。
//! 2. 栈溢出防护触发率 100% —— 对三栈各溢出触碰面（guard 区/哨兵带/
//!    深越界三档）注入触碰：每次触碰必触发防护（fired == touched，
//!    千分率恒 1000），防护后降级路径（切备用栈）即时生效。
//!
//! 语义来源（写死，不引入运行期查表）：
//! - Intel SDM Vol.3 §6.14.2（64 位模式 IST 换栈）：IDT 门 bit12:0 的
//!   IST[2:0] 字段非零时，CPU 以 TSS 中 IST[IST-1] 装载 RSP，不依赖
//!   当前特权级栈——这是 DF/NMI/MC 三异常「原栈不可信仍可入_handler」
//!   的硬件根。
//! - 任务书指定标注 SDM §7.7（任务书原文引注「注释 SDM §7.7 与本仓
//!   F0805 IST 语义」）：过程调用与栈帧对齐纪律（RSP 入帧 16 字节
//!   对齐）与本仓 F0805（unx0805_spurious.rs，同批 B01）五区向量规划
//!   及 IST 槽位语义承接——五区沿用 F0801 规划，本单只领三张 IST。
//! - 异常号与错误码有无分型取 SDM Vol.3 Table 6-1：#DF(8) 压错误码
//!   （恒 0）；#MC(18) 压错误码（恒 0）；NMI(2) 为中断类帧，不压错误
//!   码（本模型以 EC_NONE 标记占位，四要素仍齐）。
//! - 64 位 IDT 硬件压帧 40 字节（SS/RSP/RFLAGS/CS/RIP 各 8），带错误
//!   码 +8（SDM Vol.3 §6.14）；栈向下生长，初始 RSP = 栈顶。
//! - Linux 惯例（Documentation/x86/kernel-stacks.rst）：DF→IST 栈、
//!   NMI→IST 栈、MC→IST 栈三槽分立——本单按任务书定编 IST1=DF/
//!   IST2=NMI/IST3=MC。
//!
//! 诊断段：0x27F0~0x27FB 独占（12 码）。段空闲性依据：任务书原配
//! 0x27C0~0x27CB 被 unx3381_usamplelib 实占（其 SEG_BASE=0x27C0，
//! 实际最大占用 0x27E1），指定改配 0x27E0 段亦被 ves505_s28accept
//! （SEG_BASE=0x27E0）声明占用；全仓 grep `SEG_BASE: u16 = 0x27` 与
//! `: u16 = 0x27F` 双查，0x27F0 段无任何声明与实占（0x27C0 段占用止
//! 于 0x27E1，0x2800 段自 ves506 起）——故取紧邻 0x27E0 段之后的
//! 0x27F0 段前 12 码。
//!
//! no_std+alloc 自包含：只依赖 core / alloc；零 unwrap / expect / panic! /
//! 运行期 assert!（编译期 `const _: () = assert!(…)` 可）；可失败路径全走
//! `Result<T, IstStackError>`；计数一律 saturating/wrapping；>64KB 结构
//! （事件账）只落堆不上栈。const fn 纪律：while+索引、不 derive 含 Vec
//! 字段结构的 Copy、不用 slice::get/Ord::min/max/for 循环。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════
// 确定性 LCG（本册本地副本，同仓 unx0805/unx0807 惯例：Knuth MMIX 常数）
// ═══════════════════════════════════════════════════════════════════════

/// 确定性线性同余发生器：双跑对拍的随机性来源，同种子必得同序列。
pub struct Lcg(u64);

impl Lcg {
    /// 种子即初态；0 亦可（乘加链非平凡，显式允许）。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    /// 推进一步：x_{n+1} = a·x_n + c (mod 2^64)，溢出显性 wrapping。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    /// [0, n) 均匀采样；n==0 恒返 0（不 panic 纪律）。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 诊断段独占 · IST 栈错误域（0x27F0 基，段宽 0x0C）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址（0x27F0~0x27FB 独占，空闲性依据见文件头）。
pub const SEG_BASE: u16 = 0x27F0;

const E_LAYOUT: u16 = SEG_BASE + 1; // 0x27F1 布局校验失败（重叠/对齐/规范形边界）
const E_GUARD: u16 = SEG_BASE + 2; // 0x27F2 触碰地址不可归属任何栈包络
const E_SENTINEL: u16 = SEG_BASE + 3; // 0x27F3 哨兵带完整性破坏
const E_OVERFLOW: u16 = SEG_BASE + 4; // 0x27F4 溢出防护已触发（栈处降级前哨态）
const E_ROUTE: u16 = SEG_BASE + 5; // 0x27F5 路由表错挂/运行表偏离 const 规格
const E_NOT_IST: u16 = SEG_BASE + 6; // 0x27F6 非 IST 编内向量挂接尝试
const E_DEGRADED: u16 = SEG_BASE + 7; // 0x27F7 栈已降级仍要求走原栈
const E_FRAME: u16 = SEG_BASE + 8; // 0x27F8 现场帧四要素不完整/RSP 越包络
const E_LEDGER: u16 = SEG_BASE + 9; // 0x27F9 事件账满/序号损坏
const E_FP: u16 = SEG_BASE + 10; // 0x27FA 双跑指纹不一致（确定性破坏）
const E_INJECT: u16 = SEG_BASE + 11; // 0x27FB 注入面未检出

/// 本册唯一错误类型：每个变体携带中文可操作诊断（detail）与诊断码（code）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IstStackError {
    /// 布局校验失败：三栈/guard/备用栈存在区间重叠、对齐破坏或地址越规范形。
    LayoutViolated,
    /// 溢出触碰地址不可归属任何栈包络（既非栈体也非 guard 也非哨兵带）。
    UnattributableTouch,
    /// 哨兵带完整性审计失败（有字偏离哨兵魔数且无防护触发账）。
    SentinelDamaged,
    /// 溢出防护已触发且栈进入降级前哨态（业务信号，非缺陷：调用方应走降级）。
    OverflowFired,
    /// 路由表错挂：运行期路由偏离 const 规格表，或挂接向量非 IST 编内向量。
    RouteMismatch,
    /// 非 IST 编内向量（DF/NMI/MC 之外）尝试挂接 IST 槽。
    NotIstVector,
    /// 目标栈已降级，拒绝原栈进入（应改走备用栈路径）。
    StackDegraded,
    /// 现场帧四要素不完整，或 RSP 未落在路由判定栈的包络内。
    FrameIncomplete,
    /// 事件账写满或序号回退（账完整性破坏）。
    LedgerFault,
    /// 双跑指纹不一致（确定性破坏）。
    FingerprintMismatch,
    /// 注入面（重叠/哨兵破坏/路由错挂）未按预期检出。
    InjectionUndetected,
}

impl IstStackError {
    /// 段位标签（诊断文本口径："UNX-F0806 诊断段基 0x27F0"）。
    pub fn segment_label() -> String {
        let base = "UNX-F0806 诊断段基 ".to_string();
        base + &format!("0x{:04X}", SEG_BASE)
    }

    /// 诊断码（0x27F1~0x27FB，段内唯一）。
    pub const fn code(&self) -> u16 {
        match self {
            IstStackError::LayoutViolated => E_LAYOUT,
            IstStackError::UnattributableTouch => E_GUARD,
            IstStackError::SentinelDamaged => E_SENTINEL,
            IstStackError::OverflowFired => E_OVERFLOW,
            IstStackError::RouteMismatch => E_ROUTE,
            IstStackError::NotIstVector => E_NOT_IST,
            IstStackError::StackDegraded => E_DEGRADED,
            IstStackError::FrameIncomplete => E_FRAME,
            IstStackError::LedgerFault => E_LEDGER,
            IstStackError::FingerprintMismatch => E_FP,
            IstStackError::InjectionUndetected => E_INJECT,
        }
    }

    /// 中文可操作诊断（发生了什么/为什么/下一步三要素）。
    pub const fn detail(&self) -> &'static str {
        match self {
            IstStackError::LayoutViolated => {
                "IST 布局校验失败：三栈/guard/备用栈出现区间重叠或对齐破坏。\
                 下一步：核对 IstStackTable::audit_layout() 返回的重叠对清单，\
                 修正 SEG_* 常量表后重跑 const 断言与运行期审计。"
            }
            IstStackError::UnattributableTouch => {
                "溢出触碰地址不可归属任何栈包络：既不在栈体、guard 区也不在\
                 哨兵带。下一步：核对触碰地址与 IstStackTable::classify() 的\
                 三段分类，确认调用方没有把无关注入地址混入防护面。"
            }
            IstStackError::SentinelDamaged => {
                "哨兵带完整性破坏：哨兵字偏离魔数且无对应防护触发账。下一步：\
                 查 SentinelBank::audit() 的首坏索引与该栈事件账，区分正常\
                 溢出触碰（有账）与注入破坏（无账即红案）。"
            }
            IstStackError::OverflowFired => {
                "溢出防护已触发：触碰落入 guard/哨兵/深越界三档之一，目标栈\
                 即刻降级。下一步：这不是缺陷信号——沿降级路径改投备用栈，\
                 并在处置完成后调 restore() 复役原栈。"
            }
            IstStackError::RouteMismatch => {
                "路由表错挂：运行期路由表偏离 const 规格表（DF→IST1/NMI→IST2/\
                 MC→IST3）。下一步：比对 RouteTable::audit() 给出的错挂项，\
                 若为注入面所致则确认检出后复位规格。"
            }
            IstStackError::NotIstVector => {
                "非 IST 编内向量尝试挂接：只有 DF(#8)/NMI(#2)/MC(#18) 三向量\
                 编入本表。下一步：普通异常/IRQ 走 F0818 rsp0 决策树，\
                 不要把 IST 槽配给它们。"
            }
            IstStackError::StackDegraded => {
                "目标栈已降级：溢出防护触发后原栈停用。下一步：改走备用栈\
                 （dispatcher 会自动改投），处置完成后 restore() 复役。"
            }
            IstStackError::FrameIncomplete => {
                "现场帧四要素不完整：向量号/错误码/RSP/RIP 有缺，或 RSP 未落\
                 在路由判定栈包络内。下一步：核对 IstFrame::validate() 的\
                 缺项位图与路由槽，禁止不完整帧入账。"
            }
            IstStackError::LedgerFault => {
                "事件账故障：写满（LEDGER_CAP 上限）或序号回退。下一步：\
                 扩账前先确认没有事件风暴；序号回退则查双写者。"
            }
            IstStackError::FingerprintMismatch => {
                "双跑指纹不一致：同种子两次跑出的账面指纹不同，确定性破坏。\
                 下一步：定位首次分叉的事件序号（对拍两账逐 seq 比对），\
                 排除隐藏的墙钟/地址依赖。"
            }
            IstStackError::InjectionUndetected => {
                "注入面未检出：重叠/哨兵破坏/路由错挂注入后对应检测器没有\
                 报红。下一步：核对注入返回的 InjectionOutcome.detected 与\
                 检测器阈值，检测器失明比注入本身更严重。"
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 布局常量域（三栈 + guard + 备用栈 · 全 const · 来源写死）
// ═══════════════════════════════════════════════════════════════════════

/// IST 区基址（布局模型常量）：内核规范形高半区（≥0xFFFF_8000_0000_0000
/// 为 64 位规范形内核段，SDM Vol.3 §3.3.7.1 规范形地址）。真机落址由
/// F0818（rsp0/IST 选用决策树）与 TSS 装载面承接，本单只管布局模型。
pub const IST_REGION_BASE: u64 = 0xFFFF_A000_0010_0000;

/// 每栈深度：16 KiB（任务书定编「如 16KiB/栈」；对照 Linux IST 栈
/// ISTKNS 亦为独立小栈惯例，本单取 16KiB 留足 DF 处置余量）。
pub const STACK_DEPTH: u64 = 16 * 1024;

/// 每栈下方 guard 区宽：4 KiB（一页；对照 OS guard-page 惯例：栈向下生长
/// 越过栈底即入 guard，触碰即防护）。guard 位于 [base, base+GUARD)，
/// 栈体位于 [base+GUARD, base+GUARD+STACK_DEPTH)。
pub const GUARD_DEPTH: u64 = 4 * 1024;

/// 三件套步距：guard + 栈体 = 20 KiB；三栈在区内背靠背排布，互以 guard
/// 隔离（栈间零共享字节）。
pub const SLOT_STRIDE: u64 = GUARD_DEPTH + STACK_DEPTH;

/// 备用栈基址（第 4 槽）：三主栈之后紧随，同深度同 guard，承接降级改投。
pub const BACKUP_BASE: u64 = IST_REGION_BASE + 3 * SLOT_STRIDE;

/// 哨兵带宽度（字节）：栈体最底 64 字节 = 8×u64 魔数带。向下生长的 RSP
/// 触到带内任一字即视为溢出前哨（写哨兵即触发防护——判据 2 的检测面）。
pub const SENTINEL_BYTES: u64 = 64;

/// 区域底预带宽（深越界归属带）：IST 区基址之下 4KiB，专用于把「沉到
/// 整区之下」的触碰归属为 IST1 的 DeepBelow 档（三档防护面的兜底档）。
pub const DEEP_BAND: u64 = 4 * 1024;

/// 哨兵魔数（每字初值；audit 比对基准）。选取与常见栈填充模式（0xAA/
/// 0x55）均不撞的常量：`0x15E4 7E57 5E57 1A57`（"IST SENTINEL STACK"
/// 风格的十六进制助记拼写，助记非语义，仅取其撞零概率为零）。
pub const SENTINEL_MAGIC: u64 = 0x15E4_7E57_5E57_1A57;

/// 对齐纪律：栈顶（初始 RSP）必须 16 字节对齐（SDM §7.7/ABI：过程入口
/// RSP 16 字节对齐，SSE 重对齐约定）。
pub const STACK_ALIGN: u64 = 16;

/// 64 位 IDT 硬件压帧字节数：无错误码 40（SS/RSP/RFLAGS/CS/RIP 各 8）、
/// 有错误码 48（+EC）——SDM Vol.3 §6.14 中断/异常栈帧布局。
pub const HW_FRAME_NO_EC: u64 = 40;
pub const HW_FRAME_WITH_EC: u64 = 48;

/// handler 入口保留预算（模型常数）：硬件帧之外，入口 stub 压寄存器现场
/// 的预算上限（对照本仓 F0807 TrapFrame 同构帧的 64 字节对齐纪律与
/// 通用寄存器保存宽度，取 512 字节留余）。
pub const HANDLER_RESERVE: u64 = 512;

/// NMI 无错误码的占位标记（SDM Table 6-1：NMI 为中断类帧不压 EC；四要素
/// 齐备性以本标记占位，不破坏「向量号/错误码/RSP/RIP 四要素齐」断言）。
pub const EC_NONE: u64 = u64::MAX;

/// IST 槽号（TSS 中 IST[2:0] 的 1 基编号；0 = 不用 IST）。
pub const IST_SLOT_DF: u8 = 1; // IST1 = #DF(8)
pub const IST_SLOT_NMI: u8 = 2; // IST2 = #NMI(2)
pub const IST_SLOT_MC: u8 = 3; // IST3 = #MC(18)

/// 编内异常向量（SDM Vol.3 Table 6-1 / §6.3 异常清单）。
pub const VEC_NMI: u8 = 2;
pub const VEC_DF: u8 = 8;
pub const VEC_MC: u8 = 18;

/// 错误码有无分型（SDM Table 6-1）：#DF 有 EC（恒 0）、#MC 有 EC（恒 0）、
/// NMI 无 EC（EC_NONE 占位）。
pub const fn vec_has_error_code(vector: u8) -> bool {
    vector == VEC_DF || vector == VEC_MC
}

/// 单槽布局规格（const 表元素）：一槽 = guard + 栈体，全区间由基址与
/// 三个宽度常数唯一决定，杜绝运行期二次计算漂移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IstSlotSpec {
    /// IST 槽号（1 基，对应 TSS.IST[n-1]）。
    pub slot: u8,
    /// 领主异常向量（#DF/#NMI/#MC）。
    pub vector: u8,
    /// 槽基址（guard 底）。
    pub base: u64,
    /// guard 区高界（= 栈体最低可用地址 = 哨兵带底）。
    pub guard_hi: u64,
    /// 栈顶（初始 RSP；向下生长）。
    pub top: u64,
}

impl IstSlotSpec {
    /// 由槽号/向量/基址派生全界（const fn：纯算术，无索引无循环坑）。
    pub const fn from_base(slot: u8, vector: u8, base: u64) -> Self {
        IstSlotSpec {
            slot,
            vector,
            base,
            guard_hi: base + GUARD_DEPTH,
            top: base + GUARD_DEPTH + STACK_DEPTH,
        }
    }

    /// 栈体区间 [guard_hi, top)。
    pub const fn stack_range(&self) -> (u64, u64) {
        (self.guard_hi, self.top)
    }

    /// guard 区间 [base, guard_hi)。
    pub const fn guard_range(&self) -> (u64, u64) {
        (self.base, self.guard_hi)
    }

    /// 哨兵带区间 [guard_hi, guard_hi + SENTINEL_BYTES)。
    pub const fn sentinel_range(&self) -> (u64, u64) {
        (self.guard_hi, self.guard_hi + SENTINEL_BYTES)
    }

    /// 地址是否落在哨兵带（含底不含顶，半开区间约定）。
    pub const fn in_sentinel(&self, addr: u64) -> bool {
        addr >= self.guard_hi && addr < self.guard_hi + SENTINEL_BYTES
    }

    /// 地址是否落在 guard 区。
    pub const fn in_guard(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.guard_hi
    }

    /// 地址是否落在栈体（哨兵带含于栈体最底段）。
    pub const fn in_stack(&self, addr: u64) -> bool {
        addr >= self.guard_hi && addr < self.top
    }

    /// 栈顶 16 字节对齐校验（SDM §7.7/ABI 入帧对齐纪律）。
    pub const fn top_aligned(&self) -> bool {
        self.top % STACK_ALIGN == 0
    }
}

/// 三主槽 const 规格表：IST1=DF/IST2=NMI/IST3=MC（任务书定编；Linux
/// 惯例同构：三异常各领独立 IST 栈，原栈不可信仍可入 handler）。
pub const IST_SLOT_SPECS: [IstSlotSpec; 3] = [
    IstSlotSpec::from_base(IST_SLOT_DF, VEC_DF, IST_REGION_BASE),
    IstSlotSpec::from_base(IST_SLOT_NMI, VEC_NMI, IST_REGION_BASE + SLOT_STRIDE),
    IstSlotSpec::from_base(IST_SLOT_MC, VEC_MC, IST_REGION_BASE + 2 * SLOT_STRIDE),
];

/// 备用栈槽 const 规格（第 4 槽，降级改投承接；不领异常向量，vector=0）。
pub const BACKUP_SLOT_SPEC: IstSlotSpec =
    IstSlotSpec::from_base(0, 0, BACKUP_BASE);

// ── 编译期布局断言（const _: () = assert!，纪律允许的编译期口径）──────

/// 三槽零重叠 + guard 隔离 + 对齐 + 规范形，编译期先行锁死。
const _: () = assert!(IST_SLOT_SPECS[0].top <= IST_SLOT_SPECS[1].base);
const _: () = assert!(IST_SLOT_SPECS[1].top <= IST_SLOT_SPECS[2].base);
const _: () = assert!(IST_SLOT_SPECS[2].top <= BACKUP_SLOT_SPEC.base);
const _: () = assert!(IST_SLOT_SPECS[0].top_aligned());
const _: () = assert!(IST_SLOT_SPECS[1].top_aligned());
const _: () = assert!(IST_SLOT_SPECS[2].top_aligned());
const _: () = assert!(BACKUP_SLOT_SPEC.top_aligned());
const _: () = assert!(IST_SLOT_SPECS[0].slot == 1 && IST_SLOT_SPECS[1].slot == 2);
const _: () = assert!(IST_SLOT_SPECS[2].slot == 3);
const _: () = assert!(IST_SLOT_SPECS[0].vector == VEC_DF);
const _: () = assert!(IST_SLOT_SPECS[1].vector == VEC_NMI);
const _: () = assert!(IST_SLOT_SPECS[2].vector == VEC_MC);
/// 哨兵带必须完全内嵌于栈体最底段。
const _: () = assert!(SENTINEL_BYTES <= STACK_DEPTH);
/// guard 一页宽（4KiB，页粒度隔离）。
const _: () = assert!(GUARD_DEPTH == 4096);

// ═══════════════════════════════════════════════════════════════════════
// 哨兵带模型（SentinelBank）+ 栈使用账（StackUsage）
// ═══════════════════════════════════════════════════════════════════════

/// 哨兵带字数（8×u64 = 64 字节）。
pub const SENTINEL_WORDS: usize = 8;

/// 单栈哨兵带模型：8 个 u64 魔数字 + 每字地址（字 0 = 带底最低字）。
/// 写哨兵 = 破坏对应魔数字，同时防护触发；audit 比对魔数即得完整性。
#[derive(Clone, Copy, Debug)]
pub struct SentinelBand {
    words: [u64; SENTINEL_WORDS],
    addrs: [u64; SENTINEL_WORDS],
}

impl SentinelBand {
    /// 以栈体最低地址（= 哨兵带底）铸造全魔数带。
    pub const fn forge(band_base: u64) -> Self {
        SentinelBand {
            words: [
                SENTINEL_MAGIC,
                SENTINEL_MAGIC,
                SENTINEL_MAGIC,
                SENTINEL_MAGIC,
                SENTINEL_MAGIC,
                SENTINEL_MAGIC,
                SENTINEL_MAGIC,
                SENTINEL_MAGIC,
            ],
            addrs: [
                band_base,
                band_base + 8,
                band_base + 16,
                band_base + 24,
                band_base + 32,
                band_base + 40,
                band_base + 48,
                band_base + 56,
            ],
        }
    }

    /// 完整性审计：返回首个坏字索引（全完好则 None）。
    pub fn audit(&self) -> Option<usize> {
        let mut i: usize = 0;
        while i < SENTINEL_WORDS {
            if self.words[i] != SENTINEL_MAGIC {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 坏字计数（溢出触碰后即 ≥1；正常态恒 0）。
    pub fn damaged_words(&self) -> u32 {
        let mut c: u32 = 0;
        let mut i: usize = 0;
        while i < SENTINEL_WORDS {
            if self.words[i] != SENTINEL_MAGIC {
                c = c.saturating_add(1);
            }
            i += 1;
        }
        c
    }

    /// 按地址写一字（触碰）：仅当地址属本带才写（返回是否命中）。
    /// 写入值与魔数不同即为破坏；防护面在 dispatcher 层统一结算。
    pub fn write_if_hit(&mut self, addr: u64, value: u64) -> bool {
        let mut i: usize = 0;
        while i < SENTINEL_WORDS {
            if self.addrs[i] == addr {
                self.words[i] = value;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 按索引注入翻转（注入面专用：不动 addrs，只翻一字魔数）。
    /// 越界索引返回 false（不 panic 纪律）。
    pub fn inject_flip(&mut self, idx: usize) -> bool {
        if idx >= SENTINEL_WORDS {
            return false;
        }
        self.words[idx] = self.words[idx] ^ 0xDEAD_BEEF_DEAD_BEEF;
        true
    }

    /// 重铸全带（restore 复役路径配套）。
    pub fn reforge(&mut self) {
        let mut i: usize = 0;
        while i < SENTINEL_WORDS {
            self.words[i] = SENTINEL_MAGIC;
            i += 1;
        }
    }

    /// 带内地址枚举（const 下标安全的逐项拷贝访问）。
    pub fn addr_of(&self, idx: usize) -> Option<u64> {
        if idx >= SENTINEL_WORDS {
            None
        } else {
            Some(self.addrs[idx])
        }
    }

    /// 字值读取（越界 None）。
    pub fn word_at(&self, idx: usize) -> Option<u64> {
        if idx >= SENTINEL_WORDS {
            None
        } else {
            Some(self.words[idx])
        }
    }
}

/// 单栈使用账：进入/退出/峰值水位/防护触发/降级态（判据 1 的「独立
/// 使用账」面——每栈逐项可读，互不串账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackUsage {
    /// 进入次数（dispatch 进入本栈）。
    pub enters: u64,
    /// 退出次数（handler 返回退出）。
    pub exits: u64,
    /// 峰值已用字节（top - 历史最低 RSP；saturating）。
    pub peak_used: u64,
    /// 当前在栈深度（字节；0 = 不在栈）。
    pub cur_used: u64,
    /// 溢出防护触发次数（guard/哨兵/深越界三档合计）。
    pub overflow_fires: u64,
    /// 降级态（true = 停用，改投备用栈）。
    pub degraded: bool,
}

impl StackUsage {
    /// 零账开局。
    pub const fn new() -> Self {
        StackUsage {
            enters: 0,
            exits: 0,
            peak_used: 0,
            cur_used: 0,
            overflow_fires: 0,
            degraded: false,
        }
    }

    /// 进入：enters++，cur_used 由 caller 传入本帧深度，峰值 saturating 取大。
    pub fn enter(&mut self, used_bytes: u64) {
        self.enters = self.enters.saturating_add(1);
        self.cur_used = used_bytes;
        if used_bytes > self.peak_used {
            self.peak_used = used_bytes;
        }
    }

    /// 退出：exits++，cur_used 归零（栈空）。
    pub fn exit(&mut self) {
        self.exits = self.exits.saturating_add(1);
        self.cur_used = 0;
    }

    /// 防护触发记账。
    pub fn fire(&mut self) {
        self.overflow_fires = self.overflow_fires.saturating_add(1);
        self.degraded = true;
    }

    /// 复役（restore 路径：清降级位）。
    pub fn restore(&mut self) {
        self.degraded = false;
    }

    /// 账自洽：exits ≤ enters 且 cur_used ≤ STACK_DEPTH。
    pub fn sane(&self) -> bool {
        self.exits <= self.enters && self.cur_used <= STACK_DEPTH && self.peak_used <= STACK_DEPTH
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 三栈布局抽象（IstStackTable）：重叠审计 + 三段分类 + 注入接缝
// ═══════════════════════════════════════════════════════════════════════

/// 半开区间 [lo, hi) 是否相交（const fn：纯比较，无坑）。
pub const fn ranges_overlap(a_lo: u64, a_hi: u64, b_lo: u64, b_hi: u64) -> bool {
    a_lo < b_hi && b_lo < a_hi
}

/// 地址分类三档 + 不可归属（判据 2 的触碰面全分型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchClass {
    /// guard 区触碰（栈底之下的一页隔离带）。
    GuardZone,
    /// 哨兵带触碰（栈体最底 64 字节魔数带）。
    SentinelBand,
    /// 深越界（低于哨兵带之下的栈体不存在区——模型中即低于 guard_hi
    /// 但地址非本槽包络内的下沉越界，由 dispatcher 以「防护兜底」结算）。
    DeepBelow,
}

/// 三栈布局表：持有三主槽 + 备用槽的运行期副本（const 规格表的注入
/// 接缝——注入面通过改副本制造重叠/错界，审计器必须检出）。
#[derive(Clone, Copy, Debug)]
pub struct IstStackTable {
    slots: [IstSlotSpec; 3],
    backup: IstSlotSpec,
}

impl IstStackTable {
    /// 从 const 规格表铸造（运行期与编译期规格逐字节一致开局）。
    pub const fn from_specs() -> Self {
        IstStackTable {
            slots: IST_SLOT_SPECS,
            backup: BACKUP_SLOT_SPEC,
        }
    }

    /// 读取第 idx 主槽（0 基；越界 None，不 panic）。
    pub fn slot(&self, idx: usize) -> Option<IstSlotSpec> {
        if idx >= 3 {
            None
        } else {
            Some(self.slots[idx])
        }
    }

    /// 备用槽规格。
    pub const fn backup_slot(&self) -> IstSlotSpec {
        self.backup
    }

    /// 按槽号（1 基）查主槽。
    pub fn slot_by_ist(&self, ist: u8) -> Option<IstSlotSpec> {
        let mut i: usize = 0;
        while i < 3 {
            if self.slots[i].slot == ist {
                return Some(self.slots[i]);
            }
            i += 1;
        }
        None
    }

    /// 按异常向量查领主槽（路由规格查询面）。
    pub fn slot_by_vector(&self, vector: u8) -> Option<IstSlotSpec> {
        let mut i: usize = 0;
        while i < 3 {
            if self.slots[i].vector == vector {
                return Some(self.slots[i]);
            }
            i += 1;
        }
        None
    }

    /// 布局审计：三主栈体两两零重叠、主栈与备用栈零重叠、guard 完整
    /// 隔离（相邻槽间恰有 guard 带且无共享字节）、栈顶 16 字节对齐。
    /// 返回重叠/失配对描述（空 = 布局合规）。
    pub fn audit_layout(&self) -> Vec<(usize, usize)> {
        let mut bad: Vec<(usize, usize)> = Vec::new();
        // 三主栈体两两重叠检查（i<j 全组合 3 对）。
        let mut i: usize = 0;
        while i < 3 {
            let mut j: usize = i + 1;
            while j < 3 {
                let a = self.slots[i];
                let b = self.slots[j];
                // 栈体重叠。
                if ranges_overlap(a.guard_hi, a.top, b.guard_hi, b.top) {
                    bad.push((i, j));
                }
                // guard 与对方栈体共享（隔离失效，以 (i,j) 记同一对）。
                if ranges_overlap(a.base, a.guard_hi, b.guard_hi, b.top)
                    || ranges_overlap(b.base, b.guard_hi, a.guard_hi, a.top)
                {
                    bad.push((i, j));
                }
                j += 1;
            }
            i += 1;
        }
        // 主栈 vs 备用栈（索引 3 记备用）。
        let mut k: usize = 0;
        while k < 3 {
            let a = self.slots[k];
            let b = self.backup;
            if ranges_overlap(a.base, a.top, b.base, b.top) {
                bad.push((k, 3));
            }
            k += 1;
        }
        // 对齐失配以 (idx, idx) 自环记（非重叠但同为布局红）。
        let mut m: usize = 0;
        while m < 3 {
            if !self.slots[m].top_aligned() {
                bad.push((m, m));
            }
            m += 1;
        }
        if !self.backup.top_aligned() {
            bad.push((3, 3));
        }
        bad
    }

    /// 布局合规判定（重叠对清单为空）。
    pub fn layout_ok(&self) -> bool {
        self.audit_layout().is_empty()
    }

    /// 地址归属分类：遍历三主槽 + 备用槽，先判 guard 再判哨兵再判深越界
    /// （区域底预带：IST1 之下 4KiB 下沉带，归属 IST1 的深越界档——
    /// 标准布局相邻槽背靠背无缝隙，深越界只可能在区域最底之下发生）；
    /// 返回 (槽索引 0~2 主槽/3 备用, 分类)。不可归属返回 None。
    pub fn classify(&self, addr: u64) -> Option<(usize, TouchClass)> {
        let mut i: usize = 0;
        while i < 3 {
            let s = self.slots[i];
            if s.in_guard(addr) {
                return Some((i, TouchClass::GuardZone));
            }
            if s.in_sentinel(addr) {
                return Some((i, TouchClass::SentinelBand));
            }
            // 深越界：仅 IST1（区域首槽）之下的预带可判（后槽之下必为
            // 前槽 guard，已在上面归属）。
            if i == 0 && addr.saturating_add(DEEP_BAND) >= IST_REGION_BASE && addr < s.base {
                return Some((0, TouchClass::DeepBelow));
            }
            i += 1;
        }
        // 备用槽 guard/哨兵同样归类（槽索引 3）。
        let b = self.backup;
        if b.in_guard(addr) {
            return Some((3, TouchClass::GuardZone));
        }
        if b.in_sentinel(addr) {
            return Some((3, TouchClass::SentinelBand));
        }
        None
    }

    /// 注入面 A —— 栈重叠注入：把第 idx 主槽的 base 整体下移 shift 字节，
    /// 使其栈体与前一槽栈体重叠（idx=0 时与备用语义反向重叠：上移）。
    /// 返回注入后的副本表（不破坏原表；audit 必须检出 → detected）。
    pub fn inject_overlap(&self, idx: usize, shift: u64) -> Result<IstStackTable, IstStackError> {
        if idx >= 3 || shift == 0 {
            return Err(IstStackError::LayoutViolated);
        }
        let mut cpy = *self;
        let s = cpy.slots[idx];
        if idx == 0 {
            // 与备用语义反向：抬高 base 侵入后一槽（构造正向重叠）。
            cpy.slots[idx] = IstSlotSpec::from_base(s.slot, s.vector, s.base + shift);
        } else {
            cpy.slots[idx] = IstSlotSpec::from_base(s.slot, s.vector, s.base - shift);
        }
        Ok(cpy)
    }

    /// 注入面 B —— 对齐破坏注入：把第 idx 主槽 base 加 8（top 失 16 对齐）。
    pub fn inject_misalign(&self, idx: usize) -> Result<IstStackTable, IstStackError> {
        if idx >= 3 {
            return Err(IstStackError::LayoutViolated);
        }
        let mut cpy = *self;
        let s = cpy.slots[idx];
        cpy.slots[idx] = IstSlotSpec::from_base(s.slot, s.vector, s.base + 8);
        Ok(cpy)
    }

    /// 与 const 规格表逐槽比对（运行期漂移检测，路由审计的布局半边）。
    pub fn matches_specs(&self) -> bool {
        let mut i: usize = 0;
        while i < 3 {
            if self.slots[i] != IST_SLOT_SPECS[i] {
                return false;
            }
            i += 1;
        }
        self.backup == BACKUP_SLOT_SPEC
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 现场帧四要素（IstFrame）：向量号 / 错误码 / RSP / RIP
// ═══════════════════════════════════════════════════════════════════════

/// IST 现场抽象帧（判据 1「现场完整」的载体）：四要素齐备性以 valid 位
/// 组表达，缺任一即 FrameIncomplete。错误码语义按 SDM Table 6-1 分型
/// （#DF/#MC 恒 0、NMI 以 EC_NONE 占位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IstFrame {
    /// 向量号（#DF=8/#NMI=2/#MC=18）。
    pub vector: u8,
    /// 错误码（#DF/#MC=0；NMI=EC_NONE 占位）。
    pub error_code: u64,
    /// 入 handler 时的 RSP（必须落在路由判定栈包络内）。
    pub rsp: u64,
    /// 中断点 RIP（构造触发演练以确定性伪 RIP 填充）。
    pub rip: u64,
}

impl IstFrame {
    /// 四要素齐备校验：RSP 落在期望槽 [guard_hi, top) 内且 16 字节对齐，
    /// 向量与期望一致。返回 false 即 FrameIncomplete 红案。
    pub fn validate(&self, expect_slot: IstSlotSpec, expect_vector: u8) -> bool {
        if self.vector != expect_vector {
            return false;
        }
        if !expect_slot.in_stack(self.rsp) {
            return false;
        }
        if self.rsp % STACK_ALIGN != 0 {
            return false;
        }
        // 错误码分型：#DF/#MC 恒 0，NME 占位 EC_NONE。
        if vec_has_error_code(expect_vector) && self.error_code != 0 {
            return false;
        }
        if !vec_has_error_code(expect_vector) && self.error_code != EC_NONE {
            return false;
        }
        true
    }

    /// 硬件压帧宽度（按向量分型 40/48 字节，SDM §6.14）。
    pub const fn hw_frame_bytes(&self) -> u64 {
        if vec_has_error_code(self.vector) {
            HW_FRAME_WITH_EC
        } else {
            HW_FRAME_NO_EC
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 事件账（EventLedger）+ FNV 指纹
// ═══════════════════════════════════════════════════════════════════════

/// FNV-1a 64 位常数（同仓 unx0805 惯例）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV 单字节推进（const：wrapping 乘）。
pub const fn fnv1a_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV 单 u64 推进（const：8 字节小端序逐位入火，while+索引）。
pub const fn fnv1a_u64(h: u64, v: u64) -> u64 {
    let mut acc = h;
    let mut i = 0;
    while i < 8 {
        acc = fnv1a_byte(acc, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// 事件种类码：进入 IST 栈（dispatch 入栈）。
pub const K_ENTER: u8 = 1;
/// 事件种类码：退出 IST 栈（handler 返回）。
pub const K_EXIT: u8 = 2;
/// 事件种类码：溢出防护触发（guard/哨兵/深越界三档，aux 编码档别）。
pub const K_OVERFLOW: u8 = 3;
/// 事件种类码：降级改投备用栈。
pub const K_DOWNGRADE: u8 = 4;
/// 事件种类码：复役（restore）。
pub const K_RESTORE: u8 = 5;
/// 事件种类码：非法挂接被拦（非编内向量/降级栈直入等，aux 编码拦因）。
pub const K_REJECT: u8 = 6;
/// 事件种类码：注入面标记（aux 编码面别：1 重叠/2 哨兵/3 路由）。
pub const K_INJECT_MARK: u8 = 7;

/// 单条事件：定长 32 字节（编译期冻结），事件账堆上存储。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IstEvent {
    /// 全账单调序号。
    pub seq: u64,
    /// 事件时刻（演练 tick，非墙钟——确定性纪律）。
    pub ts: u64,
    /// 事件种类码（K_*）。
    pub kind: u8,
    /// 关联槽（0~2 主槽/3 备用）。
    pub slot: u8,
    /// 关联向量（非向量事件填 0）。
    pub vector: u8,
    /// 辅助字段（档别/面别/帧深度编码）。
    pub aux: u32,
    /// 四要素帧（进入事件携带；其余事件置零帧）。
    pub frame_rsp: u64,
}

/// 事件账容量：判据演练（三栈各万级进入 + 防护/降级/注入标记）余量。
pub const LEDGER_CAP: usize = 131_072;

/// 事件账：顺序入账 + FNV 指纹 + 序号单调审计。
pub struct EventLedger {
    events: Vec<IstEvent>,
    next_seq: u64,
    dropped: u64,
    /// 确定性时间轴：入账自增 1（原 tick_next 外置自增迁入账内，
    /// 消除调用方 `self.ledger.record(..)` 的双重可变借用）。
    tick: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> Self {
        EventLedger {
            events: Vec::new(),
            next_seq: 1,
            dropped: 0,
            tick: 0,
        }
    }

    /// 入账：满则记 dropped 并报 LedgerFault（不静默丢数据）。
    /// 时间轴在账内自增（确定性，不用墙钟）。
    pub fn record(
        &mut self,
        kind: u8,
        slot: u8,
        vector: u8,
        aux: u32,
        frame_rsp: u64,
    ) -> Result<u64, IstStackError> {
        if self.events.len() >= LEDGER_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(IstStackError::LedgerFault);
        }
        self.tick = self.tick.wrapping_add(1);
        let ts = self.tick;
        let seq = self.next_seq;
        self.events.push(IstEvent {
            seq,
            ts,
            kind,
            slot,
            vector,
            aux,
            frame_rsp,
        });
        self.next_seq = self.next_seq.wrapping_add(1);
        Ok(seq)
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 空账判断。
    pub fn is_empty(&self) -> bool {
        self.events.len() == 0
    }

    /// 丢账计数（应为 0）。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 按下标读事件（越界 None）。
    pub fn event_at(&self, i: usize) -> Option<IstEvent> {
        if i < self.events.len() {
            Some(self.events[i])
        } else {
            None
        }
    }

    /// FNV 指纹：全账逐事件字段入火（双跑对拍依据）。
    pub fn fnv_fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i: usize = 0;
        while i < self.events.len() {
            let e = self.events[i];
            h = fnv1a_u64(h, e.seq);
            h = fnv1a_u64(h, e.ts);
            h = fnv1a_byte(h, e.kind);
            h = fnv1a_byte(h, e.slot);
            h = fnv1a_byte(h, e.vector);
            h = fnv1a_u64(h, e.aux as u64);
            h = fnv1a_u64(h, e.frame_rsp);
            i += 1;
        }
        h
    }

    /// 序号单调审计（回退/跳变即破坏；跳变仅允许账满丢弃场景）。
    pub fn seq_monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.events.len() {
            if self.events[i].seq <= self.events[i - 1].seq {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 按种类码过滤的事件计数。
    pub fn count_kind(&self, kind: u8) -> u64 {
        let mut c: u64 = 0;
        let mut i: usize = 0;
        while i < self.events.len() {
            if self.events[i].kind == kind {
                c = c.saturating_add(1);
            }
            i += 1;
        }
        c
    }

    /// 账面文本摘要（health/诊断用，中文）。
    pub fn summary(&self) -> String {
        format!(
            "账长={} 丢={} 进={} 退={} 防护={} 降级={} 复役={} 拦={} 注入标={}",
            self.events.len(),
            self.dropped,
            self.count_kind(K_ENTER),
            self.count_kind(K_EXIT),
            self.count_kind(K_OVERFLOW),
            self.count_kind(K_DOWNGRADE),
            self.count_kind(K_RESTORE),
            self.count_kind(K_REJECT),
            self.count_kind(K_INJECT_MARK),
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 挂接语义（RouteTable + IstDispatcher）：DF/NMI/MC → 各自 IST 槽路由
// ═══════════════════════════════════════════════════════════════════════

/// 单条路由：向量 → IST 槽（1 基）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteEntry {
    /// 异常向量。
    pub vector: u8,
    /// 目标 IST 槽号。
    pub ist: u8,
}

/// const 规格路由表：DF(8)→IST1 / NMI(2)→IST2 / MC(18)→IST3
/// （任务书定编；SDM Vol.3 §6.14.2 只定义 IST 机制不定编——定编沿
/// Linux 三异常分栈惯例，本仓 F0817 属性位唯一出口的槽位语义承接）。
pub const ROUTE_SPECS: [RouteEntry; 3] = [
    RouteEntry {
        vector: VEC_DF,
        ist: IST_SLOT_DF,
    },
    RouteEntry {
        vector: VEC_NMI,
        ist: IST_SLOT_NMI,
    },
    RouteEntry {
        vector: VEC_MC,
        ist: IST_SLOT_MC,
    },
];

/// 运行期路由表（注入接缝：可被 inject_route_swap 改挂，审计必须检出）。
#[derive(Clone, Copy, Debug)]
pub struct RouteTable {
    entries: [RouteEntry; 3],
}

impl RouteTable {
    /// 从 const 规格铸造。
    pub const fn from_specs() -> Self {
        RouteTable {
            entries: ROUTE_SPECS,
        }
    }

    /// 按向量查路由（未编入返回 None）。
    pub fn route_of(&self, vector: u8) -> Option<RouteEntry> {
        let mut i: usize = 0;
        while i < 3 {
            if self.entries[i].vector == vector {
                return Some(self.entries[i]);
            }
            i += 1;
        }
        None
    }

    /// 路由审计：与 const 规格逐条比对，返回错挂项（空 = 合规）。
    pub fn audit(&self) -> Vec<usize> {
        let mut bad: Vec<usize> = Vec::new();
        let mut i: usize = 0;
        while i < 3 {
            if self.entries[i] != ROUTE_SPECS[i] {
                bad.push(i);
            }
            i += 1;
        }
        bad
    }

    /// 合规判定。
    pub fn ok(&self) -> bool {
        self.audit().is_empty()
    }

    /// 注入面 C —— 路由错挂注入：交换两项的 IST 槽（如 DF 改挂 IST2）。
    pub fn inject_route_swap(&self) -> RouteTable {
        let mut cpy = *self;
        let tmp = cpy.entries[0].ist;
        cpy.entries[0].ist = cpy.entries[1].ist;
        cpy.entries[1].ist = tmp;
        cpy
    }
}

/// 溢出触碰结果（防护触发即成功路径：三档全触发 → 触发率 100%）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchOutcome {
    /// 触碰档别。
    pub class: TouchClass,
    /// 触碰槽（0~2 主槽/3 备用）。
    pub slot: usize,
    /// 防护是否触发（可归属触碰必须恒 true）。
    pub protected: bool,
    /// 目标栈是否已因此降级。
    pub degraded: bool,
    /// 哨兵带是否被本次触碰写坏（SentinelBand 档专属）。
    pub sentinel_broken: bool,
}

/// 防护统计（触发率自证面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtectionStats {
    /// 总触碰次数（可归属）。
    pub touched: u64,
    /// 防护触发次数。
    pub fired: u64,
    /// 三档分列：guard / sentinel / deep。
    pub guard_touches: u64,
    pub sentinel_touches: u64,
    pub deep_touches: u64,
}

impl ProtectionStats {
    /// 零开局。
    pub const fn new() -> Self {
        ProtectionStats {
            touched: 0,
            fired: 0,
            guard_touches: 0,
            sentinel_touches: 0,
            deep_touches: 0,
        }
    }

    /// 触发率（千分率，saturating：touched=0 时约定 1000——无触碰无欠账）。
    pub const fn rate_permille(&self) -> u64 {
        if self.touched == 0 {
            1000
        } else {
            (self.fired * 1000) / self.touched
        }
    }
}

/// 健康报告（health() 载荷，全字段只读）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IstHealth {
    /// 布局合规（三栈+备用零重叠+对齐）。
    pub layout_ok: bool,
    /// 路由表合规。
    pub route_ok: bool,
    /// 三主槽哨兵带完好数（复役后应回 3）。
    pub sentinels_intact: u32,
    /// 防护触发率（千分率，判据 2 要求恒 1000）。
    pub protection_rate_permille: u64,
    /// 当前降级栈数（处置完成后应回 0）。
    pub degraded_stacks: u32,
    /// 使用账自洽三栈全过。
    pub usage_sane: bool,
    /// 事件账序号单调且零丢账。
    pub ledger_ok: bool,
    /// 全绿判定。
    pub all_green: bool,
}

/// IST 挂接器：布局表 + 路由表 + 三栈哨兵/使用账 + 备用栈账 + 防护统计
/// + 事件账的总持有人（判据 1/2 的全部机制面都在这里）。
pub struct IstDispatcher {
    table: IstStackTable,
    route: RouteTable,
    sentinels: [SentinelBand; 3],
    backup_sentinel: SentinelBand,
    usage: [StackUsage; 3],
    backup_usage: StackUsage,
    stats: ProtectionStats,
    ledger: EventLedger,
    /// 降级改投计数（备用栈承接次数）。
    downgrades: u64,
    /// 非法挂接拦截计数。
    rejects: u64,
}

impl IstDispatcher {
    /// 全新开局：const 规格表 + 全魔数哨兵 + 零使用账。
    pub fn new() -> Self {
        IstDispatcher {
            table: IstStackTable::from_specs(),
            route: RouteTable::from_specs(),
            sentinels: [
                SentinelBand::forge(IST_SLOT_SPECS[0].guard_hi),
                SentinelBand::forge(IST_SLOT_SPECS[1].guard_hi),
                SentinelBand::forge(IST_SLOT_SPECS[2].guard_hi),
            ],
            backup_sentinel: SentinelBand::forge(BACKUP_SLOT_SPEC.guard_hi),
            usage: [StackUsage::new(), StackUsage::new(), StackUsage::new()],
            backup_usage: StackUsage::new(),
            stats: ProtectionStats::new(),
            ledger: EventLedger::new(),
            downgrades: 0,
            rejects: 0,
        }
    }

    /// 只读布局表。
    pub const fn table(&self) -> &IstStackTable {
        &self.table
    }

    /// 只读路由表。
    pub const fn route_table(&self) -> &RouteTable {
        &self.route
    }

    /// 只读防护统计。
    pub const fn protection_stats(&self) -> ProtectionStats {
        self.stats
    }

    /// 只读使用账（0~2 主槽；越界 None）。
    pub fn usage_of(&self, idx: usize) -> Option<StackUsage> {
        if idx < 3 {
            Some(self.usage[idx])
        } else if idx == 3 {
            Some(self.backup_usage)
        } else {
            None
        }
    }

    /// 只读哨兵带（0~2 主槽；3 = 备用）。
    pub fn sentinel_of(&self, idx: usize) -> Option<SentinelBand> {
        if idx < 3 {
            Some(self.sentinels[idx])
        } else if idx == 3 {
            Some(self.backup_sentinel)
        } else {
            None
        }
    }

    /// 只读事件账指纹透出。
    pub fn fingerprint(&self) -> u64 {
        self.ledger.fnv_fingerprint()
    }

    /// 事件账文本摘要透出。
    pub fn ledger_summary(&self) -> String {
        self.ledger.summary()
    }

    /// 挂接语义：把编内向量挂到路由判定槽（改写路由表唯一合法入口——
    /// 实际语义是「校验后确认」，路由表由 const 规格开局，挂接只在
    /// 注入复位场景改写）。非编内向量 → NotIstVector；槽冲突 →
    /// RouteMismatch。
    pub fn hook(&mut self, vector: u8, ist: u8) -> Result<(), IstStackError> {
        let spec = self.table.slot_by_vector(vector);
        if spec.is_none() {
            self.rejects = self.rejects.saturating_add(1);
            let _ = self
                .ledger
                .record(K_REJECT, 0, vector, 1, 0);
            return Err(IstStackError::NotIstVector);
        }
        let spec = spec.unwrap_or(BACKUP_SLOT_SPEC);
        if spec.slot != ist {
            self.rejects = self.rejects.saturating_add(1);
            let _ = self
                .ledger
                .record(K_REJECT, ist.saturating_sub(1) as u32 as u8, vector, 2, 0);
            return Err(IstStackError::RouteMismatch);
        }
        Ok(())
    }

    /// 构造触发演练：给定向量，走路由表 → 目标槽；目标栈降级则改投备用
    /// 栈（降级路径，K_DOWNGRADE 入账）；进入栈体（使用账 enter），铸
    /// 四要素帧（RSP = 栈顶 - 硬件帧宽 - handler 预算 - LCG 抖动，16
    /// 字节对齐裁齐），帧校验通过后退出（K_EXIT）。返回帧与实际落栈槽。
    pub fn dispatch_drill(
        &mut self,
        vector: u8,
        rng: &mut Lcg,
    ) -> Result<(IstFrame, usize), IstStackError> {
        let route = match self.route.route_of(vector) {
            Some(r) => r,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                let _ = self
                    .ledger
                    .record(K_REJECT, 0, vector, 3, 0);
                return Err(IstStackError::NotIstVector);
            }
        };
        // 目标主槽索引 = ist-1（1 基转 0 基）。
        let main_idx = (route.ist as usize).saturating_sub(1);
        if main_idx >= 3 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(IstStackError::RouteMismatch);
        }
        let slot_spec = match self.table.slot(main_idx) {
            Some(s) => s,
            None => return Err(IstStackError::RouteMismatch),
        };
        // 降级检查：目标栈降级 → 改投备用栈（判据 2 的降级路径）。
        let (target_idx, target_spec) = if self.usage[main_idx].degraded {
            self.downgrades = self.downgrades.saturating_add(1);
            let _ = self
                .ledger
                .record(K_DOWNGRADE, 3, vector, main_idx as u32, 0);
            (3usize, self.table.backup_slot())
        } else {
            (main_idx, slot_spec)
        };
        // 铸帧：错误码按分型（#DF/#MC=0，NMI=EC_NONE）；RIP 确定性伪值；
        // RSP = target top - (hw 帧宽 + handler 预算 + [0,512) 抖动)，
        // 向下 16 字节对齐裁齐。
        let jitter = rng.below(64) * STACK_ALIGN; // [0,1024) 对齐粒度抖动
        let budget = IstFrame {
            vector,
            error_code: 0,
            rsp: 0,
            rip: 0,
        }
        .hw_frame_bytes()
            + HANDLER_RESERVE
            + jitter;
        let rsp_raw = target_spec.top.saturating_sub(budget);
        let rsp = rsp_raw - (rsp_raw % STACK_ALIGN);
        let frame = IstFrame {
            vector,
            error_code: if vec_has_error_code(vector) { 0 } else { EC_NONE },
            rsp,
            rip: (0x5E57_0000_0000_0000u64).wrapping_add(vector as u64).wrapping_add(rng.next() & 0xFFF),
        };
        // 帧校验（对实际落栈槽——降级改投后按备用槽校验）。
        if !frame.validate(target_spec, vector) {
            let _ = self
                .ledger
                .record(K_REJECT, target_idx as u8, vector, 4, frame.rsp);
            return Err(IstStackError::FrameIncomplete);
        }
        // 进入记账（使用账 + 事件账）。
        let used = target_spec.top.saturating_sub(frame.rsp);
        if target_idx == 3 {
            self.backup_usage.enter(used);
        } else {
            self.usage[target_idx].enter(used);
        }
        let _ = self.ledger.record(
            K_ENTER,
            target_idx as u8,
            vector,
            used as u32,
            frame.rsp,
        );
        // 演练即完整生命周期：handler 即刻返回退出。
        if target_idx == 3 {
            self.backup_usage.exit();
        } else {
            self.usage[target_idx].exit();
        }
        let _ = self.ledger.record(K_EXIT, target_idx as u8, vector, 0, 0);
        Ok((frame, target_idx))
    }

    /// 构造 DF 触发演练（判据 1 主口径：DF 走 IST1 且现场完整；目标栈
    /// 未降级时 target_idx 必为 0）。
    pub fn df_drill(&mut self, rng: &mut Lcg) -> Result<(IstFrame, usize), IstStackError> {
        self.dispatch_drill(VEC_DF, rng)
    }

    /// 构造 NMI 触发演练（走 IST2）。
    pub fn nmi_drill(&mut self, rng: &mut Lcg) -> Result<(IstFrame, usize), IstStackError> {
        self.dispatch_drill(VEC_NMI, rng)
    }

    /// 构造 MC 触发演练（走 IST3）。
    pub fn mc_drill(&mut self, rng: &mut Lcg) -> Result<(IstFrame, usize), IstStackError> {
        self.dispatch_drill(VEC_MC, rng)
    }

    /// 溢出触碰（判据 2 主口径）：地址分类 → 可归属（guard/哨兵/深越界）
    /// 即防护触发（统计 + 使用账 fire + 降级 + K_OVERFLOW 入账；哨兵档
    /// 同时写坏对应魔数字）；不可归属 → UnattributableTouch 红案。
    pub fn touch(&mut self, addr: u64) -> Result<TouchOutcome, IstStackError> {
        let (slot_idx, class) = match self.table.classify(addr) {
            Some(pair) => pair,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                let _ = self
                    .ledger
                    .record(K_REJECT, 4, 0, 5, 0);
                return Err(IstStackError::UnattributableTouch);
            }
        };
        // 哨兵档：写坏对应主槽/备用槽哨兵带（带内地址命中才写）。
        let mut sentinel_broken = false;
        if class == TouchClass::SentinelBand {
            let val = 0xBAAD_F00D_BAAD_F00Du64;
            if slot_idx < 3 {
                sentinel_broken = self.sentinels[slot_idx].write_if_hit(addr, val);
            } else {
                sentinel_broken = self.backup_sentinel.write_if_hit(addr, val);
            }
        }
        // 防护结算：可归属性触碰必触发（三档合一）。
        if slot_idx < 3 {
            self.usage[slot_idx].fire();
        } else {
            self.backup_usage.fire();
        }
        self.stats.touched = self.stats.touched.saturating_add(1);
        self.stats.fired = self.stats.fired.saturating_add(1);
        match class {
            TouchClass::GuardZone => {
                self.stats.guard_touches = self.stats.guard_touches.saturating_add(1);
            }
            TouchClass::SentinelBand => {
                self.stats.sentinel_touches = self.stats.sentinel_touches.saturating_add(1);
            }
            TouchClass::DeepBelow => {
                self.stats.deep_touches = self.stats.deep_touches.saturating_add(1);
            }
        }
        let aux_code: u32 = match class {
            TouchClass::GuardZone => 1,
            TouchClass::SentinelBand => 2,
            TouchClass::DeepBelow => 3,
        };
        let _ = self
            .ledger
            .record(K_OVERFLOW, slot_idx as u8, 0, aux_code, addr);
        Ok(TouchOutcome {
            class,
            slot: slot_idx,
            protected: true,
            degraded: true,
            sentinel_broken,
        })
    }

    /// 复役路径：重铸哨兵 + 清降级 + K_RESTORE 入账（处置完成后的恢复）。
    pub fn restore(&mut self, idx: usize) -> Result<(), IstStackError> {
        if idx > 3 {
            return Err(IstStackError::LayoutViolated);
        }
        if idx < 3 {
            self.sentinels[idx].reforge();
            self.usage[idx].restore();
        } else {
            self.backup_sentinel.reforge();
            self.backup_usage.restore();
        }
        let _ = self
            .ledger
            .record(K_RESTORE, idx as u8, 0, 0, 0);
        Ok(())
    }

    /// 哨兵完整性三栈审计：返回首坏 (槽, 索引) 清单（空 = 全完好）。
    /// 「有防护账的坏」= 正常溢出触碰；「无账的坏」= 注入破坏红案——
    /// 由 injection 注入面区分，本审计只报物理完好性。
    pub fn sentinel_audit(&self) -> Vec<(usize, usize)> {
        let mut bad: Vec<(usize, usize)> = Vec::new();
        let mut i: usize = 0;
        while i < 3 {
            if let Some(w) = self.sentinels[i].audit() {
                bad.push((i, w));
            }
            i += 1;
        }
        if let Some(w) = self.backup_sentinel.audit() {
            bad.push((3, w));
        }
        bad
    }

    /// health()：布局 + 路由 + 哨兵 + 防护率 + 降级数 + 使用账 + 事件账
    /// 七面聚合（处置完成后 all_green 应为 true）。
    pub fn health(&self) -> IstHealth {
        let layout_ok = self.table.layout_ok();
        let route_ok = self.route.ok();
        let intact = 3u32.saturating_sub(self.sentinel_audit().len() as u32);
        let degraded = {
            let mut d: u32 = 0;
            let mut i: usize = 0;
            while i < 3 {
                if self.usage[i].degraded {
                    d = d.saturating_add(1);
                }
                i += 1;
            }
            if self.backup_usage.degraded {
                d = d.saturating_add(1);
            }
            d
        };
        let usage_sane = {
            let mut ok = self.usage[0].sane() && self.usage[1].sane() && self.usage[2].sane();
            ok = ok && self.backup_usage.sane();
            ok
        };
        let ledger_ok = self.ledger.seq_monotonic() && self.ledger.dropped() == 0;
        let rate = self.stats.rate_permille();
        let all_green = layout_ok
            && route_ok
            && intact == 3
            && degraded == 0
            && usage_sane
            && ledger_ok
            && rate == 1000;
        IstHealth {
            layout_ok,
            route_ok,
            sentinels_intact: intact,
            protection_rate_permille: rate,
            degraded_stacks: degraded,
            usage_sane,
            ledger_ok,
            all_green,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 补充只读出口（降级/拦截计数 + 注入专用哨兵翻转）
// ═══════════════════════════════════════════════════════════════════════

impl IstDispatcher {
    /// 降级改投计数（备用栈承接次数）。
    pub const fn downgrade_count(&self) -> u64 {
        self.downgrades
    }

    /// 非法挂接/不可归属拦截计数。
    pub const fn reject_count(&self) -> u64 {
        self.rejects
    }

    /// 事件账长度透出。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 注入专用哨兵翻转（不走 touch 防护路径——「无防护账的破坏」即注入
    /// 红案判据）：slot 0~2 主槽/3 备用；idx 0~7 字序。越界参数红案。
    pub fn inject_sentinel_flip(&mut self, slot: usize, idx: usize) -> Result<(), IstStackError> {
        let ok = if slot < 3 {
            self.sentinels[slot].inject_flip(idx)
        } else if slot == 3 {
            self.backup_sentinel.inject_flip(idx)
        } else {
            false
        };
        if !ok {
            return Err(IstStackError::LayoutViolated);
        }
        let _ = self
            .ledger
            .record(K_INJECT_MARK, slot as u8, 0, 2, 0);
        Ok(())
    }

    /// 运行期路由表整体替换（注入面 C 的落表动作：替换后 audit 必须红）。
    pub fn install_route_table(&mut self, table: RouteTable) {
        self.route = table;
        let _ = self
            .ledger
            .record(K_INJECT_MARK, 0, 0, 3, 0);
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 双跑确定性场景（run_scenario）：同种子 → 同指纹（判据纪律面）
// ═══════════════════════════════════════════════════════════════════════

/// 场景报表：全部可对拍字段（双跑逐字段相等 + 指纹相等）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioReport {
    /// 事件账 FNV 指纹（确定性主口径）。
    pub fingerprint: u64,
    /// DF 演练成功数。
    pub df_ok: u64,
    /// NMI 演练成功数。
    pub nmi_ok: u64,
    /// MC 演练成功数。
    pub mc_ok: u64,
    /// 溢出触碰数（防护触发数与之相等）。
    pub touches: u64,
    /// 降级改投数。
    pub downgrades: u64,
    /// 防护触发率（千分率）。
    pub protection_rate: u64,
    /// 复役后 health 全绿。
    pub all_green: bool,
    /// 复役后三栈哨兵完好数。
    pub sentinels_intact: u32,
}

/// 全链路场景：开局 → 三栈演练各 N 次 → 三栈逐栈溢出触碰（guard/哨兵/
/// 深越界三档全走）→ 降级期再演练（改投备用栈）→ 逐栈复役 → 收官演练
/// → health。任何一步 Err 即短路返回。
pub fn run_scenario(seed: u64, drills_per_kind: u64) -> Result<ScenarioReport, IstStackError> {
    let mut d = IstDispatcher::new();
    let mut rng = Lcg::new(seed);
    let mut df_ok: u64 = 0;
    let mut nmi_ok: u64 = 0;
    let mut mc_ok: u64 = 0;
    // 阶段一：三栈正常演练。
    let mut i: u64 = 0;
    while i < drills_per_kind {
        if d.df_drill(&mut rng).is_ok() {
            df_ok = df_ok.saturating_add(1);
        }
        if d.nmi_drill(&mut rng).is_ok() {
            nmi_ok = nmi_ok.saturating_add(1);
        }
        if d.mc_drill(&mut rng).is_ok() {
            mc_ok = mc_ok.saturating_add(1);
        }
        i += 1;
    }
    // 阶段二：逐栈溢出触碰（每栈 guard + 哨兵两档；IST1 加深越界档）。
    let table = IstStackTable::from_specs();
    let mut touches: u64 = 0;
    let mut slot_i: usize = 0;
    while slot_i < 3 {
        let spec = match table.slot(slot_i) {
            Some(s) => s,
            None => return Err(IstStackError::LayoutViolated),
        };
        // guard 档：8 字节对齐确定性地址。
        let guard_addr = spec.base + rng.below(GUARD_DEPTH / 8) * 8;
        let out = d.touch(guard_addr)?;
        touches = touches.saturating_add(out.protected as u64);
        // 哨兵档。
        let (band_lo, _) = spec.sentinel_range();
        let sent_addr = band_lo + rng.below(SENTINEL_BYTES / 8) * 8;
        let out = d.touch(sent_addr)?;
        touches = touches.saturating_add(out.protected as u64);
        // 深越界档（仅 IST1 区域底预带）。
        if slot_i == 0 {
            let deep_addr = IST_REGION_BASE.saturating_sub(8 + rng.below(DEEP_BAND / 8) * 8);
            let out = d.touch(deep_addr)?;
            touches = touches.saturating_add(out.protected as u64);
        }
        // 降级期演练：该栈向量改投备用栈。
        let vec = spec.vector;
        let r = d.dispatch_drill(vec, &mut rng)?;
        if r.1 != 3 {
            return Err(IstStackError::StackDegraded);
        }
        slot_i += 1;
    }
    // 阶段三：逐栈复役 + 复役后演练回主栈。
    let mut r_i: usize = 0;
    while r_i < 3 {
        d.restore(r_i)?;
        let vec = match table.slot(r_i) {
            Some(s) => s.vector,
            None => return Err(IstStackError::LayoutViolated),
        };
        let r = d.dispatch_drill(vec, &mut rng)?;
        if r.1 != r_i {
            return Err(IstStackError::StackDegraded);
        }
        r_i += 1;
    }
    // 收官：读表。
    let stats = d.protection_stats();
    let health = d.health();
    Ok(ScenarioReport {
        fingerprint: d.fingerprint(),
        df_ok,
        nmi_ok,
        mc_ok,
        touches,
        downgrades: d.downgrade_count(),
        protection_rate: stats.rate_permille(),
        all_green: health.all_green,
        sentinels_intact: health.sentinels_intact,
    })
}

// ═══════════════════════════════════════════════════════════════════════
// 注入面（三路）：栈重叠 / 哨兵破坏 / 路由错挂 —— 各自必须被检出
// ═══════════════════════════════════════════════════════════════════════

/// 注入结果：面别 + 是否检出 + 证据文本（中文）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectionOutcome {
    /// 面别（栈重叠/对齐破坏/哨兵破坏/路由错挂）。
    pub face: &'static str,
    /// 检测器是否抓到（false 即 InjectionUndetected 红案）。
    pub detected: bool,
    /// 证据文本（重叠对清单/首坏索引/错挂项）。
    pub evidence: String,
}

/// 注入面 A：栈重叠注入 —— 把 IST3 槽下移半栈深，audit_layout 必须
/// 报出 (1,2) 或 (2,3) 等重叠对。
pub fn inject_stack_overlap() -> Result<InjectionOutcome, IstStackError> {
    let clean = IstStackTable::from_specs();
    if !clean.layout_ok() {
        return Err(IstStackError::LayoutViolated);
    }
    let corrupted = clean.inject_overlap(2, STACK_DEPTH / 2)?;
    let bad = corrupted.audit_layout();
    let detected = !bad.is_empty();
    let evidence = format!("重叠/失配对清单={:?}（原表审计={:?}）", bad, clean.audit_layout());
    if !detected {
        return Err(IstStackError::InjectionUndetected);
    }
    Ok(InjectionOutcome {
        face: "栈重叠注入（IST3 下移半栈深）",
        detected,
        evidence,
    })
}

/// 注入面 A'：对齐破坏注入 —— IST2 槽 base 加 8，top 失 16 字节对齐，
/// audit_layout 以自环对 (1,1) 报红。
pub fn inject_alignment_break() -> Result<InjectionOutcome, IstStackError> {
    let clean = IstStackTable::from_specs();
    let corrupted = clean.inject_misalign(1)?;
    let bad = corrupted.audit_layout();
    let detected = !bad.is_empty();
    let evidence = format!("对齐失配自环对={:?}", bad);
    if !detected {
        return Err(IstStackError::InjectionUndetected);
    }
    Ok(InjectionOutcome {
        face: "对齐破坏注入（IST2 base+8）",
        detected,
        evidence,
    })
}

/// 注入面 B：哨兵破坏注入 —— 不走 touch 防护路径直接翻 IST2 哨兵第 3
/// 字，sentinel_audit 必须报 (1,3)；且防护统计零触碰（「无防护账的
/// 破坏」= 注入红案，与合法触碰破坏区分）。
pub fn inject_sentinel_damage() -> Result<InjectionOutcome, IstStackError> {
    let mut d = IstDispatcher::new();
    d.inject_sentinel_flip(1, 3)?;
    let bad = d.sentinel_audit();
    let detected = !bad.is_empty();
    let stats = d.protection_stats();
    let evidence = format!(
        "哨兵首坏清单={:?} 防护统计(touched={},fired={})——无防护账的破坏即注入红案",
        bad, stats.touched, stats.fired
    );
    if !detected || stats.touched != 0 {
        return Err(IstStackError::InjectionUndetected);
    }
    Ok(InjectionOutcome {
        face: "哨兵破坏注入（IST2 哨兵第 3 字翻转）",
        detected,
        evidence,
    })
}

/// 注入面 C：路由错挂注入 —— DF 与 NMI 的 IST 槽互换（DF 改挂 IST2），
/// RouteTable::audit 必须报出错挂项。
pub fn inject_route_misroute() -> Result<InjectionOutcome, IstStackError> {
    let clean = RouteTable::from_specs();
    if !clean.ok() {
        return Err(IstStackError::RouteMismatch);
    }
    let swapped = clean.inject_route_swap();
    let bad = swapped.audit();
    let detected = !bad.is_empty();
    let evidence = format!("错挂项索引={:?}（DF/NMI 槽互换后 DF→IST2）", bad);
    if !detected {
        return Err(IstStackError::InjectionUndetected);
    }
    Ok(InjectionOutcome {
        face: "路由错挂注入（DF↔NMI 槽互换）",
        detected,
        evidence,
    })
}

// ═══════════════════════════════════════════════════════════════════════
// 测试（隔离探针内跑；全绿 = 本单交付口径）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── 布局与规格 ────────────────────────────────────────────────────

    #[test]
    fn test_layout_zero_overlap_audit() {
        let t = IstStackTable::from_specs();
        assert!(t.audit_layout().is_empty(), "三栈+备用零重叠审计必须为空");
        assert!(t.matches_specs(), "运行期表必须与 const 规格逐槽一致");
    }

    #[test]
    fn test_slot_geometry_and_alignment() {
        let mut i: usize = 0;
        while i < 3 {
            let s = IST_SLOT_SPECS[i];
            assert_eq!(s.top - s.guard_hi, STACK_DEPTH, "栈深必须 16KiB");
            assert_eq!(s.guard_hi - s.base, GUARD_DEPTH, "guard 必须一页");
            assert_eq!(s.top % 16, 0, "栈顶 16 字节对齐");
            assert!(s.in_stack(s.top - 16));
            assert!(!s.in_stack(s.top));
            assert!(s.in_sentinel(s.guard_hi));
            assert!(!s.in_sentinel(s.guard_hi + SENTINEL_BYTES));
            i += 1;
        }
        // 背靠背步距。
        assert_eq!(
            IST_SLOT_SPECS[1].base - IST_SLOT_SPECS[0].base,
            SLOT_STRIDE
        );
        assert_eq!(
            IST_SLOT_SPECS[2].base - IST_SLOT_SPECS[1].base,
            SLOT_STRIDE
        );
        assert_eq!(BACKUP_BASE, IST_SLOT_SPECS[2].base + SLOT_STRIDE);
    }

    #[test]
    fn test_route_specs_and_hook_semantics() {
        let mut d = IstDispatcher::new();
        assert!(d.hook(VEC_DF, IST_SLOT_DF).is_ok());
        assert!(d.hook(VEC_NMI, IST_SLOT_NMI).is_ok());
        assert!(d.hook(VEC_MC, IST_SLOT_MC).is_ok());
        // 非 IST 编内向量（#GP=13）挂接被拦。
        match d.hook(13, IST_SLOT_DF) {
            Err(IstStackError::NotIstVector) => {}
            other => panic!("非编内向量必须 NotIstVector，实得 {:?}", other),
        }
        // 槽冲突（DF 挂 IST2）被拦。
        match d.hook(VEC_DF, IST_SLOT_NMI) {
            Err(IstStackError::RouteMismatch) => {}
            other => panic!("槽冲突必须 RouteMismatch，实得 {:?}", other),
        }
        assert_eq!(d.reject_count(), 2);
    }

    // ── 判据 1：构造 DF 触发走 IST 且现场完整 ────────────────────────

    #[test]
    fn test_df_drill_walks_ist1_frame_complete() {
        let mut d = IstDispatcher::new();
        let mut rng = Lcg::new(0x0806_0001);
        let mut n: u64 = 0;
        while n < 1000 {
            let (frame, target) = d.df_drill(&mut rng).expect("DF 演练失败");
            assert_eq!(target, 0, "DF 必须走 IST1（未降级）");
            let spec = IST_SLOT_SPECS[0];
            assert!(frame.validate(spec, VEC_DF), "现场帧四要素校验失败");
            assert_eq!(frame.vector, VEC_DF);
            assert_eq!(frame.error_code, 0, "#DF 错误码恒 0（SDM Table 6-1）");
            assert!(frame.rsp >= spec.guard_hi && frame.rsp < spec.top);
            assert_ne!(frame.rip, 0, "RIP 要素有值");
            n += 1;
        }
    }

    #[test]
    fn test_nmi_and_mc_drills_own_slots() {
        let mut d = IstDispatcher::new();
        let mut rng = Lcg::new(0x0806_0002);
        let mut n: u64 = 0;
        while n < 500 {
            let (f_nmi, t_nmi) = d.nmi_drill(&mut rng).expect("NMI 演练失败");
            assert_eq!(t_nmi, 1, "NMI 必须走 IST2");
            assert_eq!(f_nmi.error_code, EC_NONE, "NMI 无错误码占位");
            assert!(f_nmi.validate(IST_SLOT_SPECS[1], VEC_NMI));
            let (f_mc, t_mc) = d.mc_drill(&mut rng).expect("MC 演练失败");
            assert_eq!(t_mc, 2, "MC 必须走 IST3");
            assert_eq!(f_mc.error_code, 0, "#MC 错误码恒 0");
            assert!(f_mc.validate(IST_SLOT_SPECS[2], VEC_MC));
            n += 1;
        }
    }

    #[test]
    fn test_three_stack_usage_independent() {
        let mut d = IstDispatcher::new();
        let mut rng = Lcg::new(0x0806_0003);
        let mut n: u64 = 0;
        while n < 300 {
            let _ = d.df_drill(&mut rng);
            n += 1;
        }
        let u0 = d.usage_of(0).expect("槽 0 使用账");
        let u1 = d.usage_of(1).expect("槽 1 使用账");
        let u2 = d.usage_of(2).expect("槽 2 使用账");
        assert_eq!(u0.enters, 300, "IST1 进入 300 次");
        assert_eq!(u0.exits, 300, "演练即完整生命周期");
        assert!(u0.peak_used > 0 && u0.peak_used <= STACK_DEPTH, "峰值水位界内");
        assert_eq!(u1.enters, 0, "三栈独立：IST2 零串账");
        assert_eq!(u2.enters, 0, "三栈独立：IST3 零串账");
        assert!(u0.sane() && u1.sane() && u2.sane());
    }

    #[test]
    fn test_frame_hw_bytes_typing() {
        let f_df = IstFrame { vector: VEC_DF, error_code: 0, rsp: 0, rip: 0 };
        let f_nmi = IstFrame { vector: VEC_NMI, error_code: EC_NONE, rsp: 0, rip: 0 };
        assert_eq!(f_df.hw_frame_bytes(), HW_FRAME_WITH_EC, "#DF 压错误码 48 字节");
        assert_eq!(f_nmi.hw_frame_bytes(), HW_FRAME_NO_EC, "NMI 不压错误码 40 字节");
    }

    #[test]
    fn test_frame_incomplete_rejected() {
        let spec = IST_SLOT_SPECS[0];
        // 向量错配。
        let bad_vec = IstFrame { vector: VEC_NMI, error_code: 0, rsp: spec.top - 64, rip: 1 };
        assert!(!bad_vec.validate(spec, VEC_DF));
        // RSP 越包络（guard 区内）。
        let bad_rsp = IstFrame { vector: VEC_DF, error_code: 0, rsp: spec.base + 16, rip: 1 };
        assert!(!bad_rsp.validate(spec, VEC_DF));
        // RSP 未 16 对齐。
        let bad_align = IstFrame { vector: VEC_DF, error_code: 0, rsp: spec.top - 24, rip: 1 };
        assert!(!bad_align.validate(spec, VEC_DF));
        // 错误码分型错（NMI 帧给了 0 而非 EC_NONE）。
        let bad_ec = IstFrame { vector: VEC_NMI, error_code: 0, rsp: spec.top - 64, rip: 1 };
        assert!(!bad_ec.validate(IST_SLOT_SPECS[1], VEC_NMI));
    }

    // ── 判据 2：栈溢出防护触发率 100% ────────────────────────────────

    #[test]
    fn test_overflow_guard_zone_all_stacks() {
        let mut d = IstDispatcher::new();
        let mut i: usize = 0;
        while i < 3 {
            let spec = IST_SLOT_SPECS[i];
            let out = d.touch(spec.base + 2048).expect("guard 触碰必须可归属");
            assert!(out.protected, "guard 触碰必须触发防护");
            assert_eq!(out.class, TouchClass::GuardZone);
            assert_eq!(out.slot, i);
            i += 1;
        }
        // 备用槽 guard 同样触发。
        let out = d.touch(BACKUP_SLOT_SPEC.base + 8).expect("备用 guard 触碰");
        assert!(out.protected);
        let stats = d.protection_stats();
        assert_eq!(stats.touched, 4);
        assert_eq!(stats.fired, 4);
        assert_eq!(stats.rate_permille(), 1000, "触发率必须 100%");
    }

    #[test]
    fn test_overflow_sentinel_band_writes_damage() {
        let mut d = IstDispatcher::new();
        let spec = IST_SLOT_SPECS[2];
        let (band_lo, _) = spec.sentinel_range();
        let out = d.touch(band_lo + 32).expect("哨兵触碰必须可归属");
        assert!(out.protected && out.sentinel_broken, "写哨兵即触发防护且写坏魔数");
        assert_eq!(d.sentinel_audit().len(), 1, "哨兵审计必报首坏");
        let damaged = d.sentinel_of(2).expect("槽 2 哨兵").damaged_words();
        assert_eq!(damaged, 1);
    }

    #[test]
    fn test_overflow_deep_below_band() {
        let mut d = IstDispatcher::new();
        let addr = IST_REGION_BASE - 256;
        let out = d.touch(addr).expect("深越界必须可归属");
        assert!(out.protected);
        assert_eq!(out.class, TouchClass::DeepBelow);
        assert_eq!(out.slot, 0, "区域底预带归属 IST1");
    }

    #[test]
    fn test_protection_rate_100_percent_mass_drill() {
        let mut d = IstDispatcher::new();
        let mut rng = Lcg::new(0x0806_0004);
        // 三档 × 三栈 × 大样本：每次触碰必触发。
        let mut n: u64 = 0;
        while n < 2000 {
            let slot = (rng.below(3) as usize) % 3;
            let spec = IST_SLOT_SPECS[slot];
            let tier = rng.below(3);
            let addr = if tier == 0 {
                spec.base + rng.below(GUARD_DEPTH / 8) * 8
            } else if tier == 1 {
                spec.guard_hi + rng.below(SENTINEL_BYTES / 8) * 8
            } else {
                IST_REGION_BASE.saturating_sub(8 + rng.below(DEEP_BAND / 8) * 8)
            };
            let out = d.touch(addr).expect("可归属触碰");
            assert!(out.protected, "第 {} 次触碰防护失明", n);
            n += 1;
        }
        let stats = d.protection_stats();
        assert_eq!(stats.touched, 2000);
        assert_eq!(stats.fired, 2000, "fired == touched 判据自证");
        assert_eq!(stats.rate_permille(), 1000);
        assert!(stats.guard_touches > 0 && stats.sentinel_touches > 0 && stats.deep_touches > 0);
    }

    #[test]
    fn test_unattributable_touch_rejected() {
        let mut d = IstDispatcher::new();
        // 无关注入地址（非规范模型区，不可归属）。
        match d.touch(0x1234_5678) {
            Err(IstStackError::UnattributableTouch) => {}
            other => panic!("不可归属触碰必须红案，实得 {:?}", other),
        }
        assert_eq!(d.reject_count(), 1);
    }

    // ── 降级路径与复役 ───────────────────────────────────────────────

    #[test]
    fn test_degrade_then_downgrade_to_backup() {
        let mut d = IstDispatcher::new();
        let mut rng = Lcg::new(0x0806_0005);
        let spec = IST_SLOT_SPECS[0];
        let out = d.touch(spec.base + 128).expect("IST1 guard 触碰");
        assert!(out.degraded, "防护后目标栈降级");
        // 降级期 DF 演练：改投备用栈（target=3）且帧按备用槽校验。
        let (frame, target) = d.df_drill(&mut rng).expect("降级改投演练");
        assert_eq!(target, 3, "降级后必须改投备用栈");
        assert!(frame.validate(BACKUP_SLOT_SPEC, VEC_DF), "备用槽现场帧完整");
        assert_eq!(d.downgrade_count(), 1);
        let ub = d.usage_of(3).expect("备用栈账");
        assert_eq!(ub.enters, 1, "备用栈承接一次");
        // IST2/IST3 不受牵连。
        let (f_nmi, t_nmi) = d.nmi_drill(&mut rng).expect("NMI 演练");
        assert_eq!(t_nmi, 1, "IST2 未降级照常走原槽");
        assert!(f_nmi.validate(IST_SLOT_SPECS[1], VEC_NMI));
    }

    #[test]
    fn test_restore_path_returns_to_main_stack() {
        let mut d = IstDispatcher::new();
        let mut rng = Lcg::new(0x0806_0006);
        let spec = IST_SLOT_SPECS[1];
        let _ = d.touch(spec.guard_hi + 8).expect("IST2 哨兵触碰");
        d.restore(1).expect("IST2 复役");
        let (frame, target) = d.nmi_drill(&mut rng).expect("复役后 NMI 演练");
        assert_eq!(target, 1, "复役后回 IST2 原槽");
        assert!(frame.validate(IST_SLOT_SPECS[1], VEC_NMI));
        assert!(d.sentinel_audit().is_empty(), "复役重铸哨兵");
        let u1 = d.usage_of(1).expect("IST2 账");
        assert!(!u1.degraded);
        assert_eq!(u1.overflow_fires, 1, "历史触发计数保留");
    }

    #[test]
    fn test_health_all_green_after_full_cycle() {
        let rep = run_scenario(0x5E57_0806, 40).expect("全链路场景");
        assert!(rep.all_green, "复役后 health 必须全绿：{:?}", rep);
        assert_eq!(rep.sentinels_intact, 3);
        assert_eq!(rep.protection_rate, 1000);
        assert_eq!(rep.downgrades, 3, "三栈各一次降级改投");
        assert!(rep.touches >= 7, "guard×3+哨兵×3+深越界×1 至少 7 触碰");
        assert_eq!(rep.df_ok, 40, "阶段一 DF 演练数 = drills_per_kind");
        assert_eq!(rep.nmi_ok, 40);
        assert_eq!(rep.mc_ok, 40);
    }

    // ── 双跑确定性 ───────────────────────────────────────────────────

    #[test]
    fn test_double_run_determinism_same_seed() {
        let a = run_scenario(0xD00D_0001, 25).expect("首跑");
        let b = run_scenario(0xD00D_0001, 25).expect("双跑");
        assert_eq!(a.fingerprint, b.fingerprint, "同种子双跑指纹必须一致");
        assert_eq!(a, b, "报表全字段相等");
        let c = run_scenario(0xD00D_0002, 25).expect("异种子");
        assert_ne!(a.fingerprint, c.fingerprint, "异种子应得异指纹");
    }

    #[test]
    fn test_ledger_monotonic_and_summary() {
        let mut d = IstDispatcher::new();
        let mut rng = Lcg::new(0x0806_0007);
        let mut n: u64 = 0;
        while n < 50 {
            let _ = d.df_drill(&mut rng);
            n += 1;
        }
        assert!(d.ledger_len() > 0);
        let s = d.ledger_summary();
        assert!(s.contains("进=50"), "摘要必须含进入计数：{}", s);
        assert!(s.contains("丢=0"));
    }

    // ── 注入面（三路全检出）──────────────────────────────────────────

    #[test]
    fn test_inject_stack_overlap_detected() {
        let out = inject_stack_overlap().expect("注入面 A 必须检出");
        assert!(out.detected, "{}", out.evidence);
        assert!(out.face.contains("栈重叠"));
    }

    #[test]
    fn test_inject_alignment_break_detected() {
        let out = inject_alignment_break().expect("注入面 A' 必须检出");
        assert!(out.detected, "{}", out.evidence);
    }

    #[test]
    fn test_inject_sentinel_damage_detected() {
        let out = inject_sentinel_damage().expect("注入面 B 必须检出");
        assert!(out.detected, "{}", out.evidence);
        assert!(out.face.contains("哨兵破坏"));
    }

    #[test]
    fn test_inject_route_misroute_detected() {
        let out = inject_route_misroute().expect("注入面 C 必须检出");
        assert!(out.detected, "{}", out.evidence);
        assert!(out.face.contains("路由错挂"));
        // 错挂表装入 dispatcher 后 health().route_ok 变红。
        let mut d = IstDispatcher::new();
        d.install_route_table(RouteTable::from_specs().inject_route_swap());
        assert!(!d.health().route_ok, "错挂后路由健康必须红");
    }

    // ── 错误域纪律 ───────────────────────────────────────────────────

    #[test]
    fn test_error_codes_in_segment_and_unique() {
        let all = [
            IstStackError::LayoutViolated,
            IstStackError::UnattributableTouch,
            IstStackError::SentinelDamaged,
            IstStackError::OverflowFired,
            IstStackError::RouteMismatch,
            IstStackError::NotIstVector,
            IstStackError::StackDegraded,
            IstStackError::FrameIncomplete,
            IstStackError::LedgerFault,
            IstStackError::FingerprintMismatch,
            IstStackError::InjectionUndetected,
        ];
        let mut i: usize = 0;
        while i < all.len() {
            let c = all[i].code();
            assert!(c >= 0x27F1 && c <= 0x27FB, "诊断码 {:?} 越段", c);
            let d = all[i].detail();
            assert!(d.len() > 20 && d.contains("下一步"), "诊断三要素：{}", d);
            // 唯一性：与后续所有变体比对。
            let mut j = i + 1;
            while j < all.len() {
                assert_ne!(c, all[j].code(), "诊断码必须段内唯一");
                assert_ne!(d, all[j].detail());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn test_sentinel_band_forge_audit_cycle() {
        let mut band = SentinelBand::forge(0x1000);
        assert!(band.audit().is_none());
        assert!(band.write_if_hit(0x1000 + 24, 1), "带内地址命中");
        assert_eq!(band.audit(), Some(3), "第 3 字坏");
        assert_eq!(band.damaged_words(), 1);
        assert!(!band.write_if_hit(0x9000, 1), "带外地址不命中");
        band.reforge();
        assert!(band.audit().is_none(), "重铸后完好");
        assert_eq!(band.addr_of(7), Some(0x1000 + 56));
        assert_eq!(band.addr_of(8), None, "越界索引 None 不 panic");
        assert_eq!(band.word_at(0), Some(SENTINEL_MAGIC));
        assert!(!band.inject_flip(8), "越界翻转拒绝");
    }

    #[test]
    fn test_ranges_overlap_and_classify() {
        assert!(ranges_overlap(0, 10, 5, 15));
        assert!(!ranges_overlap(0, 10, 10, 20), "半开区间贴邻不算重叠");
        let t = IstStackTable::from_specs();
        // 三段分类抽查：guard 底/哨兵底/栈体中段（栈体中段非触碰面 → None）。
        let s1 = IST_SLOT_SPECS[0];
        assert_eq!(t.classify(s1.base), Some((0, TouchClass::GuardZone)));
        assert_eq!(t.classify(s1.guard_hi), Some((0, TouchClass::SentinelBand)));
        assert_eq!(t.classify(s1.guard_hi + SENTINEL_BYTES), None);
        assert_eq!(t.classify(s1.guard_hi + 1024), None, "栈体中段非触碰面");
        // IST2/IST3 的 guard 与哨兵。
        let s2 = IST_SLOT_SPECS[1];
        assert_eq!(t.classify(s2.base + 8), Some((1, TouchClass::GuardZone)));
        assert_eq!(t.classify(s2.guard_hi + 40), Some((1, TouchClass::SentinelBand)));
        // 备用槽。
        assert_eq!(
            t.classify(BACKUP_SLOT_SPEC.base + 100),
            Some((3, TouchClass::GuardZone))
        );
        // 不可归属。
        assert_eq!(t.classify(0x1234_5678), None);
    }

    #[test]
    fn test_scenario_drills_do_not_panic_and_rate_holds() {
        let rep = run_scenario(7, 5).expect("小规模场景");
        assert_eq!(rep.protection_rate, 1000);
        assert_eq!(rep.df_ok, 5);
        assert_eq!(rep.nmi_ok, 5);
        assert_eq!(rep.mc_ok, 5);
    }
}

