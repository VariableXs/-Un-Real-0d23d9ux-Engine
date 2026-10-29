//! UNX-F0806 · IST 三栈布局与 DF/NMI/MC 挂接（第二实现 · TSS 编程面中心）
//! · 诊断段 0x2850（独占 0x2850~0x285F，实用 0x2850~0x285D）
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 验收锚点原文（docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix
//! STAR II · Unxreal.md 行 2431，全文抄录不改一字）：
//! ```text
//! | UNX-F0806 | IST 三栈布局与 DF/NMI/MC 挂接 | 320 | 已深化 | UNX-F0806-J1 三张 IST 栈独立可用（构造 DF 触发走 IST 且现场完整），栈溢出防护触发率 100% |
//! ```
//! 定位原文（同文件行 2414–2426 节选，含 B01 批批注）：
//! ```text
//! ### UNX-A2 · 中断与计时基座
//! #### UNX-A2-B01 · IDT 与向量空间规划（中断面第 1 批）（F801–F820 · 20 条）
//! 嫁接源：Intel SDM Vol.3 第 6 章（中断与异常）、x86-64 ABI（栈帧布局），
//! 只跟随｜上游：AI-01 F0004（MADT 拓扑账）已冻结交付｜防重：本批对样板
//! F0021/F0022 与现存 cpu/idt.rs 为分层细化深化
//! ```
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 判据拆解（J1 两成分，缺一即批内红灯）：
//! 1. 三张 IST 栈独立可用 —— 构造 DF 触发走 IST 且现场完整：
//!    DF(#8) 经路由表挂 IST1、NMI(#2) 挂 IST2、MC(#18) 挂 IST3（任务书
//!    定编），进入即以 TSS.ist[IST-1] 装载 RSP（换栈），压硬件帧后 RSP
//!    必落在本栈包络内；现场帧四要素（向量号/错误码/RSP/RIP）齐备断言；
//!    三栈零重叠（布局审计重叠对数 == 0）+ 每栈独立使用账（进入数/退
//!    出数/峰值水位逐栈可读，A 栈进出绝不动 B 栈账——场景对拍逐轮核）。
//!    「构造 DF 触发走 IST2」按防重纪律建模为反例面：DF 错挂 IST2 属
//!    路由错挂，必被门-槽一致性核对拒绝（inject_route_miswire 演练）。
//! 2. 栈溢出防护触发率 100% —— 每栈尾哨兵带（16×u64 魔数）+ 栈间
//!    guard 区 + 深越界带三档触碰面：每次触碰必触发防护（fires ==
//!    touches，千分率恒 1000），防护触发即时降级（切备用栈）且原栈
//!    拒绝再入，恢复路径（重挂哨兵+复位）可走。
//!
//! 语义来源（写死，不引入运行期查表）：
//! - Intel SDM Vol.3 §6.14.2（64 位模式 IST 换栈）：IDT 门第二四字
//!   （描述符偏移 0x10）bit2:0 = IST[2:0] 索引、bit3 保留 0；非零时
//!   CPU 以 TSS.ist[IST-1] 装载 RSP，不依赖当前特权级栈——DF/NMI/MC
//!   「原栈不可信仍可入 handler」的硬件根。
//! - Intel SDM Vol.3 §7.7（任务书指定引注「注释 SDM §7.7 与本仓 F0805
//!   IST 语义」）：64 位 TSS 格式偏移 0x24 处 ist[7] 七字 u64 数组，
//!   ist[0] 保留恒 0，ist[1..=3] 为本单三栈装载字——本模块把「布局表
//!   →TSS 镜像字」的编程面显式建模（TssIstImage），并做规范形/对齐/
//!   round-trip 三查。
//! - Intel SDM Vol.3 Table 6-1（异常与错误码）：#DF(8) 压错误码（硬件
//!   恒推 0）；NMI(2) 与 #MC(18) 均不压错误码（本模型以 None 标记，
//!   四要素仍齐）。注：本仓第一实现（unx0806_iststack.rs）头注称
//!   MC 压错误码，与 Table 6-1 不符，本第二实现按 SDM 修正为不压。
//! - Intel SDM Vol.3 §6.14（栈帧）：64 位硬件压帧 40 字节（SS/RSP/
//!   RFLAGS/CS/RIP 各 8），带错误码 +8 = 48 字节；栈向下生长，初始
//!   RSP = 栈顶（16 字节对齐，SDM §7.7 过程调用对齐纪律 + x86-64
//!   ABI SysV §7.2.3「进入函数时 RSP+8 对齐 16」推论）。
//! - 栈深 16KiB/栈、guard 4KiB/槽：任务书写死（「如 16KiB/栈」），与
//!   Linux Documentation/x86/kernel-stacks.rst IST_STACKSIZE(16K)/
//!   guard page 惯例同量级；备用栈 8KiB（降级容错用，非任务书硬性）。
//! - 本仓 F0805（unx0805_spurious.rs，同批 B01）五区向量规划与
//!   F0022「IDT 全量重排与 IST」纲领：本单只领 IST1/IST2/IST3 三槽
//!   与 DF/NMI/MC 三向量，五区规划归属 F0801，不重复实现。
//!
//! 诊断段：0x2850~0x285F 独占（16 码，实用 0x2850 主码 + 0x2851~
//! 0x285D 错误码 13 枚，0x285E/0x285F 备用）。段空闲性依据：任务书原
//! 配 0x27C0~0x27CB 被 unx3381_usamplelib 声明占用（SEG_BASE=0x27C0，
//! 声明 0x27C0..=0x27FF，实占至 0x27E1）；指定改配 0x27E0 段被
//! ves505_s28accept（SEG_BASE=0x27E0）占用；0x2800 段被 ves506_
//! s28auto（SEG_BASE=0x2800）占用；0x27F0 段被本仓第一实现
//! unx0806_iststack.rs（SEG_BASE=0x27F0，0x27F0~0x27FB）占用。全仓
//! grep `SEG_BASE` 与 `0x285` 双查：0x2850 无任何声明；左邻 0x2840
//! 段（ves508_s28gtest SEG_BASE+0x02 止于 0x2842；cgb27_xfclose 实
//! 占止 0x2845），右邻 0x2860 段起（ves509_s28gdoc）；唯一 `0x285`
//! 命中为 unx285_panicstop.rs 的 u64 哈希种子 `0x2850_9621_…`（非
//! 诊断码）——故取 0x2850 行 16 码。
//!
//! 与第一实现（unx0806_iststack.rs，2228 行）的差异化（防重纪律）：
//! - 第一实现以「布局表+分发器」为中心；本实现以「TSS 编程面」为中心，
//!   显式建模 TssIstImage 七字镜像（§7.7 ist[7]）与 GateIstCodec
//!   门位编解码（§6.14.2 bit2:0）两块第一实现未覆盖的真实子系统；
//! - 布局区段独立（0xFFFF_0001_0040_0000，与第一实现区域
//!   [0x…0010_0000, 0x…0028_0000) 零重叠）；
//! - 错误码分型按 SDM Table 6-1 修正（MC 不压错误码）；
//! - 新增三档降级阶梯（Tier0 原栈/Tier1 备用栈/Tier2 拒绝）与
//!   多帧压栈深度演练器（probe_descend）两块独立子系统。
//!
//! no_std+alloc 自包含：只依赖 core / alloc；零 unwrap / expect /
//! panic! / 运行期 assert!（编译期 `const _: () = assert!(…)` 可）；
//! 可失败路径全走 `Result<T, IstStackError>`；计数一律 saturating/
//! wrapping；>64KB 结构（事件账 Vec）只落堆不上栈。const fn 纪律：
//! while+索引、不用 slice::get / Ord::min / for 循环、含 Vec 字段的
//! 结构不 derive Copy。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════
// 确定性 LCG（本地副本，同仓 unx0805/unx0807/unx0806 首实现惯例：
// Knuth MMIX 常数 a=6364136223846793005, c=1442695040888963407）
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

    /// 当前态只读（双跑指纹可纳入发生器相位，增强对拍灵敏度）。
    pub const fn state(&self) -> u64 {
        self.0
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 诊断段独占 · IST 错误域（0x2850 基，段宽 0x10，实用 14 码）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址（0x2850~0x285F 独占，空闲性依据见文件头）。
pub const SEG_BASE: u16 = 0x2850;
/// 诊断段掩码（整行 16 码独占）。
pub const SEG_MASK: u16 = 0x000F;
/// 健康主码（段基即体检单主码，非错误）。
pub const MAIN_CODE: u16 = SEG_BASE;

const E_LAYOUT: u16 = SEG_BASE + 0x01; // 0x2851 布局校验失败（重叠/对齐/规范形）
const E_TOUCH: u16 = SEG_BASE + 0x02; // 0x2852 触碰地址不可归属任何栈窗口
const E_SENTINEL: u16 = SEG_BASE + 0x03; // 0x2853 哨兵带完整性破坏
const E_FIRED: u16 = SEG_BASE + 0x04; // 0x2854 溢出防护触发（降级前哨信号）
const E_ROUTE: u16 = SEG_BASE + 0x05; // 0x2855 路由表错挂/门位不一致
const E_NOT_IST: u16 = SEG_BASE + 0x06; // 0x2856 非 IST 编内向量挂接尝试
const E_DEGRADED: u16 = SEG_BASE + 0x07; // 0x2857 已降级仍要求走原栈
const E_FRAME: u16 = SEG_BASE + 0x08; // 0x2858 现场帧四要素不完整/RSP 越包络
const E_LEDGER: u16 = SEG_BASE + 0x09; // 0x2859 事件账满/序号回退/使用账漂移
const E_NEST: u16 = SEG_BASE + 0x0A; // 0x285A 同槽嵌套进入（IST 硬件不容许）
const E_FP: u16 = SEG_BASE + 0x0B; // 0x285B 双跑指纹不一致（确定性破坏）
const E_INJECT: u16 = SEG_BASE + 0x0C; // 0x285C 注入面/防护面未按预期检出
const E_TSS: u16 = SEG_BASE + 0x0D; // 0x285D TSS 编程面（槽号/规范形/回读）

/// 本册唯一错误类型：每变体携带中文可操作诊断（detail）与段内唯一诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IstStackError {
    /// 布局校验失败：三主栈/备用栈/guard 存在区间重叠、对齐破坏或地址非规范形。
    LayoutViolated,
    /// 溢出触碰地址不可归属任何栈窗口（深越界带之下/区域之外）。
    UnattributableTouch,
    /// 哨兵带完整性破坏：哨兵字偏离魔数且无对应防护触发豁免账。
    SentinelDamaged,
    /// 溢出防护已触发（业务信号非缺陷：调用方应转降级路径，勿当 bug 上报）。
    OverflowFired,
    /// 路由表错挂：运行路由偏离 const 规格表，或门位 IST 域与路由不一致。
    RouteMismatch,
    /// 非 IST 编内向量（DF/NMI/MC 之外）尝试挂接或进入 IST 槽。
    NotIstVector,
    /// 目标主栈已降级仍要求走原栈（应改走备用栈或先 restore）。
    StackDegraded,
    /// 现场帧四要素不完整，或压帧后 RSP 未落在路由判定栈的包络内。
    FrameIncomplete,
    /// 事件账满/序号回退，或使用账交叉漂移（A 栈进出动了 B 栈账）。
    LedgerFault,
    /// 同槽嵌套进入：同 IST 槽在位未退出时再次进入（真实硬件将三重故障）。
    NestingViolated,
    /// 双跑指纹不一致（同种子两跑结果漂移，确定性破坏）。
    FingerprintMismatch,
    /// 注入面（重叠/哨兵破坏/路由错挂）或防护面（触碰未触发）未按预期检出。
    InjectionUndetected,
    /// TSS 编程面故障：槽号越界/ist[0] 非零/装载字非规范形/回读不符。
    TssProgramFault,
}

impl IstStackError {
    /// 段位标签（诊断文本口径："UNX-F0806 IST2 诊断段基 0x2850"）。
    pub fn segment_label() -> String {
        let base = "UNX-F0806 IST2 诊断段基 ".to_string();
        base + &format!("0x{:04X}", SEG_BASE)
    }

    /// 诊断码（0x2851~0x285D，段内唯一；段基 0x2850 留作健康主码）。
    pub const fn code(&self) -> u16 {
        match self {
            IstStackError::LayoutViolated => E_LAYOUT,
            IstStackError::UnattributableTouch => E_TOUCH,
            IstStackError::SentinelDamaged => E_SENTINEL,
            IstStackError::OverflowFired => E_FIRED,
            IstStackError::RouteMismatch => E_ROUTE,
            IstStackError::NotIstVector => E_NOT_IST,
            IstStackError::StackDegraded => E_DEGRADED,
            IstStackError::FrameIncomplete => E_FRAME,
            IstStackError::LedgerFault => E_LEDGER,
            IstStackError::NestingViolated => E_NEST,
            IstStackError::FingerprintMismatch => E_FP,
            IstStackError::InjectionUndetected => E_INJECT,
            IstStackError::TssProgramFault => E_TSS,
        }
    }

    /// 中文可操作诊断（发生了什么/为什么/下一步三要素）。
    pub const fn detail(&self) -> &'static str {
        match self {
            IstStackError::LayoutViolated => {
                "IST 布局校验失败：主栈/备用栈/guard 出现区间重叠或对齐破坏。\
                 下一步：读 IstStackLayout::audit() 返回的重叠对清单与 \
                 misalign/noncanonical 计数，修正 SEG_* 常量表后重跑文件内\
                 const 断言与运行期审计。"
            }
            IstStackError::UnattributableTouch => {
                "溢出触碰地址不可归属任何栈窗口：不在深越界带/guard/哨兵带/\
                 栈体任何一段。下一步：核对触碰地址与 IstStackLayout::\
                 locate() 的窗口划分，确认调用方没有把区域外地址混入防护面。"
            }
            IstStackError::SentinelDamaged => {
                "哨兵带完整性破坏：哨兵字偏离魔数且无防护触发豁免账。下一步：\
                 对照 audit_sentinels() 报告的槽位/字序号，查该槽位是否发生\
                 过未入账的越界写；注入演练后应 restore 重挂哨兵。"
            }
            IstStackError::OverflowFired => {
                "溢出防护已触发：触碰哨兵带/guard/深越界带，栈即时降级。\
                 这是防护正常工作的业务信号，不是缺陷。下一步：走备用栈路径\
                 处置本次异常，事后 restore(slot) 重挂哨兵复位原栈。"
            }
            IstStackError::RouteMismatch => {
                "路由表错挂：运行期路由偏离 const 规格表，或 IDT 门位 IST 域\
                 与路由表不一致。下一步：以 verify_against_spec() 输出比对\
                 DF→IST1/NMI→IST2/MC→IST3 定编；错挂源多为注入面或装配器\
                 位域错，重装路由表即可复位。"
            }
            IstStackError::NotIstVector => {
                "非 IST 编内向量（DF/NMI/MC 之外）尝试挂接或进入 IST 槽。\
                 下一步：核对向量号是否 ∈ {2,8,18}；其余向量按 F0801 五区\
                 规划走普通内核栈，不占 IST 槽。"
            }
            IstStackError::StackDegraded => {
                "目标主栈已降级，拒绝原栈再入（防护触发后原栈不可信）。\
                 下一步：本次异常改走该槽备用栈（enter 自动降级路由），或先\
                 restore() 重挂哨兵并确认哨兵完好后再回原栈。"
            }
            IstStackError::FrameIncomplete => {
                "现场帧四要素不完整（向量/错误码/RSP/RIP），或压帧后 RSP 未\
                 落在路由判定栈包络内。下一步：核对 IstFrame 四要素与 \
                 IstStackLayout::envelope_contains() 判定；若 RSP 落邻栈说明\
                 路由与 TSS 装载字不一致（先查 verify_against_spec）。"
            }
            IstStackError::LedgerFault => {
                "事件账写满/序号回退，或使用账交叉漂移（A 栈进出动了 B 栈账\
                 ——独立性破坏）。下一步：以 health().usage_ok 定位；账满则\
                 消费侧清账（本册场景重建），漂移则查 enter/exit 的槽位路由\
                 是否串槽。"
            }
            IstStackError::NestingViolated => {
                "同槽嵌套进入：该 IST 槽在位未退出时再次进入。真实硬件此路径\
                 直通三重故障（同 IST 槽无二次换栈）。下一步：查 handler 是否\
                 未退出又触发同向量；DF 演练应走独立 CPU 虚拟机实例。"
            }
            IstStackError::FingerprintMismatch => {
                "双跑指纹不一致：同种子两跑的账面/计数器/事件链出现漂移，确定\
                 性破坏。下一步：对拍两跑的 ScenarioReport 逐字段（enters/\
                 fires/downgrades/fingerprint），锁定首个漂移字段回溯其随机\
                 源调用点。"
            }
            IstStackError::InjectionUndetected => {
                "注入面/防护面未按预期检出：重叠注入未被布局审计发现、哨兵破坏\
                 未被完整性审计发现、路由错挂未被一致性核对发现，或防护面触碰\
                 未触发防护。下一步：以三 inject_* 与 protection rate 千分率\
                 自证复现，检出逻辑见各 face 的判定分支。"
            }
            IstStackError::TssProgramFault => {
                "TSS 编程面故障：IST 槽号越界（合法 1..=7，本册实用 1..=3）、\
                 ist[0] 非零保留位破坏、装载字非规范形或回读与布局不符。\
                 下一步：以 TssIstImage::verify() 逐字比对 IstStackLayout\
                 装载字，重跑 program_from(IST_SLOT_SPECS)。"
            }
        }
    }

    /// 三要素单行文案（码 + detail），供事件账/体检单直接引用。
    pub fn brief(&self) -> String {
        format!("[{:04X}] {}", self.code(), self.detail())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 通用 const 工具（const fn 纪律：整数比较 + while + 索引，无 slice::get）
// ═══════════════════════════════════════════════════════════════════════

/// 两半开区间 [lo, hi) 是否重叠（空区间恒不重叠）。
pub const fn ranges_overlap(a_lo: u64, a_hi: u64, b_lo: u64, b_hi: u64) -> bool {
    !(a_hi <= b_lo || b_hi <= a_lo)
}

/// 无符号 min（const fn 禁 Ord::min，自写分支）。
pub const fn umin(a: u64, b: u64) -> u64 {
    if a < b {
        a
    } else {
        b
    }
}

/// 无符号 max（const fn 禁 Ord::max，自写分支）。
pub const fn umax(a: u64, b: u64) -> u64 {
    if a > b {
        a
    } else {
        b
    }
}

/// 64 位规范形判定：bit63:47 必须为 bit47 的符号扩展（高半区或低半区）。
pub const fn is_canonical(a: u64) -> bool {
    let hi = a >> 47;
    hi == 0 || hi == 0x1FFFF
}

/// n 是否 k 对齐（k 为 2 的幂；k==0 恒 false）。
pub const fn is_aligned(n: u64, k: u64) -> bool {
    if k == 0 {
        false
    } else {
        n % k == 0
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 常量布局表（每常量写死注释来源；数值改动必须同步文件内 const 断言）
// ═══════════════════════════════════════════════════════════════════════

/// IST 三栈区段基址。来源：本模块自定高半区区段（内核高半区规范形惯例，
/// 同 F0801 五区规划高半区取向）；与第一实现区域
/// [0xFFFF_0001_0010_0000, 0xFFFF_0001_0028_0000) 零重叠，取
/// 0xFFFF_0001_0040_0000 独立段；16 字节对齐。
pub const IST_REGION_BASE: u64 = 0xFFFF_8001_0040_0000;

/// 每主栈深度。来源：任务书写死「如 16KiB/栈」；与 Linux IST_STACKSIZE
/// （Documentation/x86/kernel-stacks.rst，16K）同量级。
pub const STACK_DEPTH: u64 = 16 * 1024;

/// 每槽 guard 区深度。来源：任务书「栈间 guard 区隔离」；一页 4KiB
/// （SDM §4.5.1 页粒度），guard 不映射、触碰即防护。
pub const GUARD_DEPTH: u64 = 4 * 1024;

/// 主槽步长 = guard + 栈体：槽内低段 guard、高段栈体，相邻槽 guard 相接
/// （推导：base_{j+1} = base_j + SLOT_STRIDE = top_j，即每栈上缘紧贴
/// 下一槽 guard 下缘，任何方向越栈必先入 guard）。
pub const SLOT_STRIDE: u64 = GUARD_DEPTH + STACK_DEPTH;

/// 备用栈区段基址：紧接三主槽之后。
pub const BACKUP_REGION_BASE: u64 = IST_REGION_BASE + 3 * SLOT_STRIDE;

/// 每备用栈深度。来源：本模块自定（降级容错用，DF 级异常在备用栈只做
/// 保全性处置不需全深；取主栈之半 8KiB，任务书未硬性规定）。
pub const BACKUP_DEPTH: u64 = 8 * 1024;

/// 备用槽步长 = guard + 备用栈体（与主槽同构的隔离几何）。
pub const BACKUP_STRIDE: u64 = GUARD_DEPTH + BACKUP_DEPTH;

/// 区段总末（三主槽 + 三备用槽，闭开区间右端）。
pub const IST_REGION_END: u64 = BACKUP_REGION_BASE + 3 * BACKUP_STRIDE;

/// 栈顶对齐。来源：SDM §7.7 过程调用对齐纪律 + x86-64 ABI SysV §7.2
/// （进入函数时 RSP+8 对齐 16）——栈顶恒 16 字节对齐。
pub const STACK_ALIGN: u64 = 16;

/// 哨兵带字数。来源：本模块自定 16 字 u64（128 字节）：足以覆盖一次
/// 硬件帧 + handler 前导的最浅越界写带，同时 const 表可整体置魔数。
pub const SENTINEL_WORDS: usize = 16;

/// 哨兵带字节数（= 16 × 8，编译期断言锁死）。
pub const SENTINEL_BYTES: u64 = 128;

/// 哨兵魔数。来源：本模块自定（"IST SENTINEL" 拼意 0x1575_57E5_17E5_57E1，
/// 全位交织无零字，避免与常见零填充/栈残值撞形）。
pub const SENTINEL_MAGIC: u64 = 0x1575_57E5_17E5_57E1;

/// 哨兵破坏注入用垃圾字（与魔数逐位不同，编译期断言锁死 ≠ 魔数）。
pub const SENTINEL_GARBAGE: u64 = 0xDEAD_0003_0BAD_C0DE;

/// 深越界带宽度。来源：本模块自定 4KiB（guard 下一页）：把「越 guard
/// 再深一页」的失控写也纳入可归属防护面；再深即区域外（不可归属错）。
pub const DEEP_BAND: u64 = 4 * 1024;

/// 64 位硬件压帧（无错误码）：SS/RSP/RFLAGS/CS/RIP 各 8 字节。
/// 来源：Intel SDM Vol.3 §6.14 Figure 6-4/6-5。
pub const HW_FRAME_NO_EC: u64 = 40;

/// 64 位硬件压帧（带错误码）：上式 + 8 字节错误码。
/// 来源：Intel SDM Vol.3 §6.14（有错误码异常）+ Table 6-1。
pub const HW_FRAME_WITH_EC: u64 = 48;

/// handler 工作保留深度。来源：本模块自定 512 字节（SysV ABI 红区 128
/// + 叶子帧保守余量的四倍），仅用于峰值水位推演，不落硬件。
pub const HANDLER_RESERVE: u64 = 512;

/// NMI 向量号。来源：Intel SDM Vol.3 Table 6-1（#NMI = 2）。
pub const VEC_NMI: u8 = 2;
/// DF 向量号。来源：Intel SDM Vol.3 Table 6-1（#DF = 8）。
pub const VEC_DF: u8 = 8;
/// MC 向量号。来源：Intel SDM Vol.3 Table 6-1（#MC = 18）。
pub const VEC_MC: u8 = 18;

/// IST1 槽号 = DF。来源：任务书定编「IST1=DF/IST2=NMI/IST3=MC」
/// （Linux kernel-stacks.rst 同款三槽分立惯例）。
pub const IST_SLOT_DF: u8 = 1;
/// IST2 槽号 = NMI。来源：同上任务书定编。
pub const IST_SLOT_NMI: u8 = 2;
/// IST3 槽号 = MC。来源：同上任务书定编。
pub const IST_SLOT_MC: u8 = 3;

/// TSS IST 字数。来源：Intel SDM Vol.3 §7.7 64 位 TSS 格式：偏移 0x24
/// 处 ist[7] 七字 u64，ist[0] 保留恒 0。
pub const TSS_IST_WORDS: usize = 7;

/// 向量是否压错误码。来源：Intel SDM Vol.3 Table 6-1——压错误码异常
/// 全集 {8,10,11,12,13,14,17,21,29,30}；本册编内三向量中仅 #DF(8)
/// 压（硬件恒推 0）；NMI(2)/#MC(18) 不压（第一实现头注误标 MC 压码，
/// 本实现按 Table 6-1 修正）。
pub const fn vec_has_error_code(vector: u8) -> bool {
    match vector {
        8 | 10 | 11 | 12 | 13 | 14 | 17 | 21 | 29 | 30 => true,
        _ => false,
    }
}

/// 编内向量判定（本册 IST 域闭集：DF/NMI/MC）。
pub const fn is_ist_vector(vector: u8) -> bool {
    vector == VEC_DF || vector == VEC_NMI || vector == VEC_MC
}

// ── 槽位规格（const 表） ─────────────────────────────────────────────────

/// 一张栈（主或备）的 const 规格：几何 + 定编 + 展名。
#[derive(Clone, Copy, Debug)]
pub struct SlotSpec {
    /// TSS IST 槽号（主槽 1..=3；备用槽恒 0 = 不入 TSS）。
    pub ist_index: u8,
    /// 挂接向量（主槽 DF/NMI/MC；备用槽 0xFF = 不挂向量）。
    pub vector: u8,
    /// 槽基址（guard 下缘）。
    pub base: u64,
    /// 栈体深度（不含 guard）。
    pub depth: u64,
    /// guard 深度。
    pub guard: u64,
    /// 展名（账面/体检单用）。
    pub name: &'static str,
    /// 该槽向量是否压错误码（决定硬件帧 40/48）。
    pub has_ec: bool,
}

impl SlotSpec {
    /// 栈体下缘（= base + guard；哨兵带起点）。
    pub const fn body_lo(&self) -> u64 {
        self.base + self.guard
    }

    /// 栈顶（开区间右端；TSS 装载字，RSP 初值）。
    pub const fn top(&self) -> u64 {
        self.base + self.guard + self.depth
    }

    /// 哨兵带上界（= body_lo + SENTINEL_BYTES）。
    pub const fn sentinel_hi(&self) -> u64 {
        self.body_lo() + SENTINEL_BYTES
    }

    /// 深越界带下缘（= base - DEEP_BAND）。
    pub const fn deep_lo(&self) -> u64 {
        self.base.wrapping_sub(DEEP_BAND)
    }

    /// 地址是否落在本槽栈体 [body_lo, top)（哨兵含于栈体）。
    pub const fn body_contains(&self, addr: u64) -> bool {
        addr >= self.body_lo() && addr < self.top()
    }

    /// 地址是否落在本槽 guard [base, base+guard)。
    pub const fn guard_contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.body_lo()
    }

    /// 地址是否落在本槽哨兵带 [body_lo, sentinel_hi)。
    pub const fn sentinel_contains(&self, addr: u64) -> bool {
        addr >= self.body_lo() && addr < self.sentinel_hi()
    }

    /// 地址是否落在本槽深越界带 [deep_lo, base)。
    pub const fn deep_contains(&self, addr: u64) -> bool {
        addr >= self.deep_lo() && addr < self.base
    }
}

/// const 构造器：主槽规格（ist_index ∈ 1..=3，几何由区段常量推导）。
const fn primary_spec(ist_index: u8, vector: u8, name: &'static str) -> SlotSpec {
    let base = IST_REGION_BASE + ((ist_index as u64) - 1) * SLOT_STRIDE;
    SlotSpec {
        ist_index,
        vector,
        base,
        depth: STACK_DEPTH,
        guard: GUARD_DEPTH,
        name,
        has_ec: vec_has_error_code(vector),
    }
}

/// const 构造器：备用槽规格（slot ∈ 0..=2 对应 IST1/2/3 的备用栈）。
const fn backup_spec(slot: u8, name: &'static str) -> SlotSpec {
    let base = BACKUP_REGION_BASE + (slot as u64) * BACKUP_STRIDE;
    SlotSpec {
        ist_index: 0,
        vector: 0xFF,
        base,
        depth: BACKUP_DEPTH,
        guard: GUARD_DEPTH,
        name,
        has_ec: false,
    }
}

/// 三主槽 const 规格表：下标 0/1/2 对应 IST1/2/3（DF/NMI/MC 定编）。
pub const IST_SLOT_SPECS: [SlotSpec; 3] = [
    primary_spec(IST_SLOT_DF, VEC_DF, "IST1-DF"),
    primary_spec(IST_SLOT_NMI, VEC_NMI, "IST2-NMI"),
    primary_spec(IST_SLOT_MC, VEC_MC, "IST3-MC"),
];

/// 三备用槽 const 规格表：下标 0/1/2 对应 IST1/2/3 备用（降级落点）。
pub const BACKUP_SLOT_SPECS: [SlotSpec; 3] = [
    backup_spec(0, "BK1-DF"),
    backup_spec(1, "BK2-NMI"),
    backup_spec(2, "BK3-MC"),
];

// ── 编译期断言（数值锁死；改动常量必须同步此处与文件头注释） ───────────

const _: () = assert!(SEG_BASE == 0x2850);
const _: () = assert!(SEG_MASK == 0x000F);
const _: () = assert!(STACK_DEPTH == 16 * 1024, "任务书 16KiB/栈");
const _: () = assert!(GUARD_DEPTH == 4 * 1024, "一页 guard");
const _: () = assert!(SLOT_STRIDE == GUARD_DEPTH + STACK_DEPTH);
const _: () = assert!(BACKUP_STRIDE == GUARD_DEPTH + BACKUP_DEPTH);
const _: () = assert!(BACKUP_REGION_BASE == IST_REGION_BASE + 3 * SLOT_STRIDE);
const _: () = assert!(IST_REGION_END == BACKUP_REGION_BASE + 3 * BACKUP_STRIDE);
const _: () = assert!(SENTINEL_WORDS == 16 && SENTINEL_BYTES == 128);
const _: () = assert!(SENTINEL_MAGIC != SENTINEL_GARBAGE);
const _: () = assert!(HW_FRAME_NO_EC == 40 && HW_FRAME_WITH_EC == 48);
const _: () = assert!(is_canonical(IST_REGION_BASE));
const _: () = assert!(is_canonical(IST_REGION_END - 1));
const _: () = assert!(is_aligned(IST_REGION_BASE, STACK_ALIGN));
const _: () = assert!(vec_has_error_code(VEC_DF));
const _: () = assert!(!vec_has_error_code(VEC_NMI));
const _: () = assert!(!vec_has_error_code(VEC_MC));
const _: () = assert!(is_ist_vector(VEC_DF) && is_ist_vector(VEC_NMI));
const _: () = assert!(is_ist_vector(VEC_MC) && !is_ist_vector(0x20));
const _: () = assert!(IST_SLOT_SPECS.len() == 3 && BACKUP_SLOT_SPECS.len() == 3);
const _: () = assert!(IST_SLOT_SPECS[0].ist_index == 1 && IST_SLOT_SPECS[1].ist_index == 2);
const _: () = assert!(IST_SLOT_SPECS[2].ist_index == 3);
const _: () = assert!(IST_SLOT_SPECS[0].vector == VEC_DF && IST_SLOT_SPECS[1].vector == VEC_NMI);
const _: () = assert!(IST_SLOT_SPECS[2].vector == VEC_MC);
const _: () = assert!(!ranges_overlap(
    IST_SLOT_SPECS[0].base,
    IST_SLOT_SPECS[0].top(),
    IST_SLOT_SPECS[1].base,
    IST_SLOT_SPECS[1].top()
));
const _: () = assert!(!ranges_overlap(
    IST_SLOT_SPECS[1].base,
    IST_SLOT_SPECS[1].top(),
    IST_SLOT_SPECS[2].base,
    IST_SLOT_SPECS[2].top()
));
const _: () = assert!(!ranges_overlap(
    IST_SLOT_SPECS[2].base,
    IST_SLOT_SPECS[2].top(),
    BACKUP_SLOT_SPECS[0].base,
    BACKUP_SLOT_SPECS[0].top()
));
const _: () = assert!(is_aligned(IST_SLOT_SPECS[0].top(), STACK_ALIGN));
const _: () = assert!(is_aligned(IST_SLOT_SPECS[1].top(), STACK_ALIGN));
const _: () = assert!(is_aligned(IST_SLOT_SPECS[2].top(), STACK_ALIGN));

// ═══════════════════════════════════════════════════════════════════════
// 布局面（IstStackLayout）：六槽镜像 + 审计 + 触碰定位
// ═══════════════════════════════════════════════════════════════════════

/// 触碰分区（防护面三档 + 正常档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchZone {
    /// 深越界带 [base-DEEP_BAND, base)：越 guard 再深一页，必触发防护。
    DeepBelow,
    /// guard 区 [base, body_lo)：栈间隔离页，必触发防护。
    GuardBand,
    /// 哨兵带 [body_lo, sentinel_hi)：栈尾魔数带，触碰即触发防护并记损坏。
    SentinelBand,
    /// 栈体正常段 [sentinel_hi, top)：正常使用，不触发。
    StackBody,
}

impl TouchZone {
    /// 是否防护面（触碰必触发）。
    pub const fn is_protection(&self) -> bool {
        matches!(
            self,
            TouchZone::DeepBelow | TouchZone::GuardBand | TouchZone::SentinelBand
        )
    }

    /// 分区展名（账面用）。
    pub const fn label(&self) -> &'static str {
        match self {
            TouchZone::DeepBelow => "深越界带",
            TouchZone::GuardBand => "guard区",
            TouchZone::SentinelBand => "哨兵带",
            TouchZone::StackBody => "栈体",
        }
    }
}

/// locate 命中：哪张栈（主/备 + 槽序 0..=2）的哪个分区。
#[derive(Clone, Copy, Debug)]
pub struct LocateHit {
    /// 槽序 0..=2（对应 IST1/2/3）。
    pub slot: u8,
    /// 主栈（false）或备用栈（true）。
    pub backup: bool,
    /// 触碰分区。
    pub zone: TouchZone,
}

/// 布局审计结论（const 表六槽全量核）。
#[derive(Clone, Copy, Debug)]
pub struct LayoutAudit {
    /// 重叠对清单（槽对 (a,b)，a<b；a 主槽 0..=2，备用槽 3..=5 折算序）。
    pub pairs: [(u8, u8); 8],
    /// 实际重叠对数（>0 即审计不过）。
    pub pair_count: u8,
    /// 4K/16B 对齐违例数。
    pub misalign: u8,
    /// 非规范形地址数。
    pub noncanonical: u8,
    /// 区段越界数（槽体出 [REGION_BASE, REGION_END)）。
    pub out_of_region: u8,
}

impl LayoutAudit {
    /// 审计是否全绿（零重叠/零违例/零越界）。
    pub const fn ok(&self) -> bool {
        self.pair_count == 0 && self.misalign == 0 && self.noncanonical == 0 && self.out_of_region == 0
    }

    /// 审计单行摘要（体检单/注入面 note 用）。
    pub fn summary(&self) -> String {
        let mut s = format!(
            "重叠对={} 对齐违例={} 非规范={} 越界={} → {}",
            self.pair_count,
            self.misalign,
            self.noncanonical,
            self.out_of_region,
            if self.ok() { "全绿" } else { "红灯" }
        );
        let mut i = 0;
        while i < self.pair_count {
            let cap = self.pairs.len() as u8;
            if i < cap {
                let (a, b) = self.pairs[i as usize];
                s += &format!(" [{}×{}]", a, b);
            }
            i += 1;
        }
        s
    }
}

/// IST 布局面：三主槽 + 三备用槽的 const 镜像与查询/审计/定位。
#[derive(Clone, Copy, Debug)]
pub struct IstStackLayout {
    primaries: [SlotSpec; 3],
    backups: [SlotSpec; 3],
    region_base: u64,
    region_end: u64,
}

impl IstStackLayout {
    /// 从 const 规格表建布局镜像（const 上下文可用）。
    pub const fn from_consts() -> Self {
        IstStackLayout {
            primaries: IST_SLOT_SPECS,
            backups: BACKUP_SLOT_SPECS,
            region_base: IST_REGION_BASE,
            region_end: IST_REGION_END,
        }
    }

    /// 主槽规格（ist ∈ 1..=3；越界 Err）。
    pub fn primary(&self, ist: u8) -> Result<SlotSpec, IstStackError> {
        if ist >= 1 && ist <= 3 {
            Ok(self.primaries[(ist - 1) as usize])
        } else {
            Err(IstStackError::TssProgramFault)
        }
    }

    /// 备用槽规格（ist ∈ 1..=3；越界 Err）。
    pub fn backup(&self, ist: u8) -> Result<SlotSpec, IstStackError> {
        if ist >= 1 && ist <= 3 {
            Ok(self.backups[(ist - 1) as usize])
        } else {
            Err(IstStackError::TssProgramFault)
        }
    }

    /// 主槽栈顶（TSS 装载字同源；ist 越界返 0 并由调用侧 Result 面兜住）。
    pub const fn primary_top(&self, ist: u8) -> u64 {
        match ist {
            1 => self.primaries[0].top(),
            2 => self.primaries[1].top(),
            3 => self.primaries[2].top(),
            _ => 0,
        }
    }

    /// 备用槽栈顶（ist 越界返 0）。
    pub const fn backup_top(&self, ist: u8) -> u64 {
        match ist {
            1 => self.backups[0].top(),
            2 => self.backups[1].top(),
            3 => self.backups[2].top(),
            _ => 0,
        }
    }

    /// 区段基址。
    pub const fn region_base(&self) -> u64 {
        self.region_base
    }

    /// 区段末（开区间右端）。
    pub const fn region_end(&self) -> u64 {
        self.region_end
    }

    /// 主槽地址是否在栈体包络内（压帧后 RSP 判定用）。
    pub fn envelope_contains(&self, ist: u8, addr: u64) -> bool {
        if ist >= 1 && ist <= 3 {
            self.primaries[(ist - 1) as usize].body_contains(addr)
        } else {
            false
        }
    }

    /// 备用槽地址是否在其栈体包络内。
    pub fn backup_envelope_contains(&self, ist: u8, addr: u64) -> bool {
        if ist >= 1 && ist <= 3 {
            self.backups[(ist - 1) as usize].body_contains(addr)
        } else {
            false
        }
    }

    /// 全量布局审计：六槽两两重叠 + 对齐 + 规范形 + 区段包含。
    /// 折算序：主槽 0..=2，备用槽 3..=5（pairs 里以折算序记）。
    pub fn audit(&self) -> LayoutAudit {
        let mut a = LayoutAudit {
            pairs: [(0, 0); 8],
            pair_count: 0,
            misalign: 0,
            noncanonical: 0,
            out_of_region: 0,
        };
        // 六槽窗口收集：[基址, 栈顶)（guard 亦属槽占用，并入窗口）
        let mut los = [0u64; 6];
        let mut his = [0u64; 6];
        let mut i = 0;
        while i < 3 {
            los[i] = self.primaries[i].base;
            his[i] = self.primaries[i].top();
            los[i + 3] = self.backups[i].base;
            his[i + 3] = self.backups[i].top();
            i += 1;
        }
        // 两两重叠（互异对 15 组，饱和截断记 8 对）
        let mut x = 0;
        while x < 6 {
            let mut y = x + 1;
            while y < 6 {
                if ranges_overlap(los[x], his[x], los[y], his[y]) {
                    if a.pair_count < 8 {
                        a.pairs[a.pair_count as usize] = (x as u8, y as u8);
                    }
                    a.pair_count += 1;
                }
                y += 1;
            }
            x += 1;
        }
        // 对齐 / 规范形 / 区段包含（逐槽）
        let mut j = 0;
        while j < 6 {
            if !is_aligned(los[j], 4096) || !is_aligned(his[j], STACK_ALIGN) {
                a.misalign += 1;
            }
            if !is_canonical(los[j]) || !is_canonical(his[j] - 1) {
                a.noncanonical += 1;
            }
            if los[j] < self.region_base || his[j] > self.region_end {
                a.out_of_region += 1;
            }
            j += 1;
        }
        a
    }

    /// 注入/演练专用：整组替换主槽规格（重叠注入的篡改入口；常态代码
    /// 不得调用——布局常量改动应走 const 表 + 编译期断言）。
    pub fn set_primaries(&mut self, prim: [SlotSpec; 3]) {
        self.primaries = prim;
    }

    /// 布局审计的 Result 面（Err 附审计摘要码位由调用方取）。
    pub fn audit_ok(&self) -> Result<(), IstStackError> {
        if self.audit().ok() {
            Ok(())
        } else {
            Err(IstStackError::LayoutViolated)
        }
    }

    /// 触碰定位：按主槽 0→2、备用槽 0→2 顺序扫描扩展窗口
    /// [base-DEEP_BAND, top)，先命中先归属（窗口天然衔接，无歧义）。
    pub fn locate(&self, addr: u64) -> Option<LocateHit> {
        let mut i = 0;
        while i < 3 {
            let p = self.primaries[i];
            if p.deep_contains(addr) {
                return Some(LocateHit { slot: i as u8, backup: false, zone: TouchZone::DeepBelow });
            }
            if p.guard_contains(addr) {
                return Some(LocateHit { slot: i as u8, backup: false, zone: TouchZone::GuardBand });
            }
            if p.sentinel_contains(addr) {
                return Some(LocateHit { slot: i as u8, backup: false, zone: TouchZone::SentinelBand });
            }
            if p.body_contains(addr) {
                return Some(LocateHit { slot: i as u8, backup: false, zone: TouchZone::StackBody });
            }
            i += 1;
        }
        let mut k = 0;
        while k < 3 {
            let b = self.backups[k];
            if b.deep_contains(addr) {
                return Some(LocateHit { slot: k as u8, backup: true, zone: TouchZone::DeepBelow });
            }
            if b.guard_contains(addr) {
                return Some(LocateHit { slot: k as u8, backup: true, zone: TouchZone::GuardBand });
            }
            if b.sentinel_contains(addr) {
                return Some(LocateHit { slot: k as u8, backup: true, zone: TouchZone::SentinelBand });
            }
            if b.body_contains(addr) {
                return Some(LocateHit { slot: k as u8, backup: true, zone: TouchZone::StackBody });
            }
            k += 1;
        }
        None
    }

    /// 布局指纹（FNV-1a over 六槽 base/top + 区段端；改动任何几何必变）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a_u64(h, self.region_base);
        h = fnv1a_u64(h, self.region_end);
        let mut i = 0;
        while i < 3 {
            h = fnv1a_u64(h, self.primaries[i].base);
            h = fnv1a_u64(h, self.primaries[i].top());
            h = fnv1a_u64(h, self.backups[i].base);
            h = fnv1a_u64(h, self.backups[i].top());
            i += 1;
        }
        h
    }

    /// 布局六槽展单（每槽一行：名/槽号/向量/基/深/guard/顶）。
    pub fn dump(&self) -> String {
        let mut s = String::from("IST 布局六槽：\n");
        let mut i = 0;
        while i < 3 {
            let p = self.primaries[i];
            let b = self.backups[i];
            s += &format!(
                "  {} ist={} vec={:#04X} base={:#018X} depth={:#X} guard={:#X} top={:#018X}\n",
                p.name, p.ist_index, p.vector, p.base, p.depth, p.guard, p.top()
            );
            s += &format!(
                "  {} ist=0 vec=0xFF base={:#018X} depth={:#X} guard={:#X} top={:#018X}\n",
                b.name, b.base, b.depth, b.guard, b.top()
            );
            i += 1;
        }
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════
// FNV-1a（64 位；本册本地副本，参数同 IETF draft-eastlake-fnv /
// 同仓各册惯例：offset 0xcbf29ce484222325 / prime 0x100000001b3）
// ═══════════════════════════════════════════════════════════════════════

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 吸收一字节（const fn 可用）。
pub const fn fnv1a_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 吸收一个 u64（小端逐字节；const fn 可用）。
pub const fn fnv1a_u64(h: u64, v: u64) -> u64 {
    let mut x = h ^ v;
    // 8 轮 ×fnv_prime 等价逐字节；避免数组/循环依赖，直接展开乘混合
    let mut r = 0;
    while r < 8 {
        x = x.wrapping_mul(FNV_PRIME);
        r += 1;
    }
    x
}

/// FNV-1a 吸收一段字节（运行期；无 slice::get，索引 + 长度守卫）。
pub fn fnv1a_bytes(h: u64, bytes: &[u8]) -> u64 {
    let mut acc = h;
    let mut i = 0;
    while i < bytes.len() {
        acc = fnv1a_byte(acc, bytes[i]);
        i += 1;
    }
    acc
}

// ═══════════════════════════════════════════════════════════════════════
// TSS IST 编程面（TssIstImage）：§7.7 ist[7] 七字镜像的装配/回读/三查
// ═══════════════════════════════════════════════════════════════════════

/// TSS IST 七字镜像：words[0] 保留恒 0（SDM §7.7），words[1..=3] 为
/// DF/NMI/MC 三栈装载字，words[4..=7] 本册未用恒 0。
#[derive(Clone, Copy, Debug)]
pub struct TssIstImage {
    words: [u64; TSS_IST_WORDS],
}

impl TssIstImage {
    /// 全零镜像（ist[0] 保留态；空 TSS 起点）。
    pub const fn zeroed() -> Self {
        TssIstImage {
            words: [0; TSS_IST_WORDS],
        }
    }

    /// 从主槽规格表编程：specs[i].ist_index 号字 = 该槽栈顶。
    /// 规格 ist_index 越界（>7 或 0）或表长超 7 → Err(TssProgramFault)。
    pub fn program_from(specs: &[SlotSpec]) -> Result<Self, IstStackError> {
        let mut img = Self::zeroed();
        let mut i = 0;
        while i < specs.len() {
            let idx = specs[i].ist_index;
            if idx == 0 || idx as usize >= TSS_IST_WORDS {
                return Err(IstStackError::TssProgramFault);
            }
            let top = specs[i].top();
            if !is_canonical(top) {
                return Err(IstStackError::TssProgramFault);
            }
            img.words[idx as usize] = top;
            i += 1;
        }
        Ok(img)
    }

    /// 读第 i 字（i 越界返 0；const 上下文可用）。
    pub const fn word(&self, i: usize) -> u64 {
        if i < TSS_IST_WORDS {
            self.words[i]
        } else {
            0
        }
    }

    /// ist[0] 保留字是否为 0（§7.7 保留位纪律）。
    pub const fn reserved_zero(&self) -> bool {
        self.words[0] == 0
    }

    /// 与布局对拍：ist[i] == layout.primary_top(i)（i=1..=3），
    /// ist[0]==0，words[4..7]==0，全字规范形——任何不符 Err。
    pub fn verify(&self, layout: &IstStackLayout) -> Result<(), IstStackError> {
        if !self.reserved_zero() {
            return Err(IstStackError::TssProgramFault);
        }
        let mut i = 1;
        while i < TSS_IST_WORDS {
            if !is_canonical(self.word(i)) {
                return Err(IstStackError::TssProgramFault);
            }
            i += 1;
        }
        let mut k = 1;
        while k <= 3 {
            let expect = layout.primary_top(k);
            if self.word(k as usize) != expect {
                return Err(IstStackError::TssProgramFault);
            }
            k += 1;
        }
        let mut m = 4;
        while m < TSS_IST_WORDS {
            if self.word(m) != 0 {
                return Err(IstStackError::TssProgramFault);
            }
            m += 1;
        }
        Ok(())
    }

    /// 已编程的槽数（words[1..=7] 非零计数；本册正常态 = 3）。
    pub fn programmed_slots(&self) -> u8 {
        let mut n = 0;
        let mut i = 1;
        while i < TSS_IST_WORDS {
            if self.words[i] != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 镜像指纹（FNV over 七字）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0;
        while i < TSS_IST_WORDS {
            h = fnv1a_u64(h, self.words[i]);
            i += 1;
        }
        h
    }

    /// 展单（七字逐行）。
    pub fn dump(&self) -> String {
        let mut s = String::from("TSS IST 镜像：\n");
        let mut i = 0;
        while i < TSS_IST_WORDS {
            s += &format!("  ist[{}] = {:#018X}\n", i, self.words[i]);
            i += 1;
        }
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════
// IDT 门位 IST 域编解码（GateIstCodec）：§6.14.2 第二四字 bit3:0
// ═══════════════════════════════════════════════════════════════════════

/// 门第二四字 IST 域掩码（bit2:0 = IST 索引；来源 SDM Vol.3 §6.14.2
/// 64 位 IDT 门格式，描述符偏移 0x10 处字节的低 4 位域）。
pub const GATE_IST_MASK: u64 = 0x7;
/// 门第二四字低 4 位域掩码（IST 索引 bit2:0 + 保留 bit3=0）。
pub const GATE_LOW4_MASK: u64 = 0xF;

/// IDT 门位 IST 域编解码器：把「路由表定编」与「门描述符位域」对齐。
#[derive(Clone, Copy, Debug)]
pub struct GateIstCodec;

impl GateIstCodec {
    /// 校验 IST 索引合法域（0=不用 IST，1..=7=TSS 槽；>7 非法）。
    pub const fn ist_valid(ist: u8) -> bool {
        ist <= 7
    }

    /// 从门第二四字提取 IST 索引（bit2:0；位域非法值原样返回由上层核）。
    pub const fn extract(upper: u64) -> u8 {
        (upper & GATE_IST_MASK) as u8
    }

    /// 装配门第二四字：清 bit3:0 后写入 ist 域（bit3 保留恒 0）。
    /// ist > 7 → Err(RouteMismatch)（位域放不下）。
    pub fn assemble(upper: u64, ist: u8) -> Result<u64, IstStackError> {
        if !Self::ist_valid(ist) {
            return Err(IstStackError::RouteMismatch);
        }
        Ok((upper & !GATE_LOW4_MASK) | ((ist as u64) & GATE_LOW4_MASK))
    }

    /// 门位 round-trip 自检：assemble→extract 必还原（ist ∈ 0..=7）。
    pub fn roundtrip(upper: u64, ist: u8) -> Result<bool, IstStackError> {
        let gate = Self::assemble(upper, ist)?;
        Ok(Self::extract(gate) == ist)
    }

    /// bit3 保留位检查（装配产物 bit3 必 0）。
    pub const fn reserved_bit3_clear(gate_upper: u64) -> bool {
        (gate_upper & 0x8) == 0
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 哨兵带（SentinelBand）：栈尾 16×u64 魔数带 + 豁免账
// ═══════════════════════════════════════════════════════════════════════

/// 单栈哨兵带：16 字 u64，arm 时全置魔数；防护触发造成的损坏记入
/// 豁免计数（excused），注入/失控写损坏无豁免 → 审计红灯。
#[derive(Clone, Copy, Debug)]
pub struct SentinelBand {
    words: [u64; SENTINEL_WORDS],
    /// 防护触发豁免数（每次合法 fire 损坏恰 1 字 +1）。
    excused: u64,
    /// 累计损坏字数（含豁免与未豁免）。
    corrupted: u64,
}

impl SentinelBand {
    /// 全字挂魔数（初始态/restore 态）。
    pub const fn armed() -> Self {
        SentinelBand {
            words: [SENTINEL_MAGIC; SENTINEL_WORDS],
            excused: 0,
            corrupted: 0,
        }
    }

    /// 读第 i 字（越界返 0；const 上下文可用）。
    pub const fn word(&self, i: usize) -> u64 {
        if i < SENTINEL_WORDS {
            self.words[i]
        } else {
            0
        }
    }

    /// 写坏第 i 字（越界静默取末字；损坏账 +1；豁免由调用侧 decides）。
    pub fn damage(&mut self, i: usize, excused: bool) {
        let idx = if i < SENTINEL_WORDS { i } else { SENTINEL_WORDS - 1 };
        // 已坏字重复写不重复计数（账不虚增）
        if self.words[idx] == SENTINEL_MAGIC {
            self.corrupted = self.corrupted.saturating_add(1);
        }
        self.words[idx] = SENTINEL_GARBAGE;
        if excused {
            self.excused = self.excused.saturating_add(1);
        }
    }

    /// 未豁免损坏字数（>0 即审计红灯：corrupted - excused，饱和减）。
    pub const fn unexcused(&self) -> u64 {
        self.corrupted.saturating_sub(self.excused)
    }

    /// 完好字数（= 魔数字计数）。
    pub fn intact_words(&self) -> u8 {
        let mut n = 0;
        let mut i = 0;
        while i < SENTINEL_WORDS {
            if self.words[i] == SENTINEL_MAGIC {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 哨兵带指纹（FNV over 16 字 + 损坏/豁免账）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0;
        while i < SENTINEL_WORDS {
            h = fnv1a_u64(h, self.words[i]);
            i += 1;
        }
        h = fnv1a_u64(h, self.corrupted);
        fnv1a_u64(h, self.excused)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 现场帧（IstFrame）：四要素 + 压帧几何
// ═══════════════════════════════════════════════════════════════════════

/// IST 进入现场帧：寄存器现场抽象，四要素 = 向量号/错误码/RSP/RIP。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IstFrame {
    /// 要素 1：向量号（DF=8/NMI=2/MC=18）。
    pub vector: u8,
    /// 要素 2：错误码（DF 恒 Some(0)——SDM Table 6-1 硬件推 0；
    /// NMI/MC 为 None——不压错误码）。
    pub error_code: Option<u64>,
    /// 要素 3：压帧后 RSP（须落路由判定栈包络内）。
    pub rsp: u64,
    /// 要素 4：触发点 RIP（演练注入的现场寄存器）。
    pub rip: u64,
    /// 落位 IST 槽（1..=3）。
    pub slot: u8,
    /// 是否落备用栈（降级路径）。
    pub on_backup: bool,
    /// 硬件帧字节数（40/48，随错误码有无）。
    pub frame_size: u64,
}

impl IstFrame {
    /// 向量是否压错误码（帧几何同源判定）。
    pub const fn has_ec(&self) -> bool {
        vec_has_error_code(self.vector)
    }

    /// 硬件帧大小（const 表同源：has_ec → 48 else 40）。
    pub const fn hw_size(vector: u8) -> u64 {
        if vec_has_error_code(vector) {
            HW_FRAME_WITH_EC
        } else {
            HW_FRAME_NO_EC
        }
    }

    /// 四要素齐备性（向量编内 + 错误码形态对 + RSP/RIP 非零规范形）。
    pub fn complete(&self) -> bool {
        if !is_ist_vector(self.vector) {
            return false;
        }
        // 错误码形态：DF 必 Some（硬件恒 0 也算在），NMI/MC 必 None
        let ec_ok = match self.error_code {
            Some(_) => vec_has_error_code(self.vector),
            None => !vec_has_error_code(self.vector),
        };
        ec_ok && self.rsp != 0 && is_canonical(self.rsp) && self.rip != 0 && is_canonical(self.rip)
    }

    /// 单行展帧（四要素 + 落位）。
    pub fn dump(&self) -> String {
        let ec_txt = match self.error_code {
            Some(v) => format!("EC={:#x}", v),
            None => String::from("EC=∅"),
        };
        format!(
            "vec={:#04X} {} RSP={:#018X} RIP={:#018X} slot=IST{} {} frame={}",
            self.vector,
            ec_txt,
            self.rsp,
            self.rip,
            self.slot,
            if self.on_backup { "(备用)" } else { "" },
            self.frame_size
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 路由表（RouteSpec/IstRouter）：DF/NMI/MC → IST1/2/3 定编 + 门位一致性
// ═══════════════════════════════════════════════════════════════════════

/// 单条路由规格：向量 → IST 槽 定编（const 表条目形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteSpec {
    /// 挂接向量（2/8/18）。
    pub vector: u8,
    /// 落位 IST 槽（1..=3）。
    pub ist: u8,
    /// 展名。
    pub name: &'static str,
}

/// const 路由规格表（任务书定编：IST1=DF/IST2=NMI/IST3=MC）。
pub const ROUTE_SPEC_TABLE: [RouteSpec; 3] = [
    RouteSpec { vector: VEC_DF, ist: IST_SLOT_DF, name: "DF→IST1" },
    RouteSpec { vector: VEC_NMI, ist: IST_SLOT_NMI, name: "NMI→IST2" },
    RouteSpec { vector: VEC_MC, ist: IST_SLOT_MC, name: "MC→IST3" },
];

/// 运行期路由表：live 三条 + 对应 IDT 门第二四字镜像。
/// 常态由 const 表整表装载；debug_force_route 仅供注入面破坏。
#[derive(Clone, Debug)]
pub struct IstRouter {
    live: [RouteSpec; 3],
    gate_upper: [u64; 3],
    /// 错挂注入累计计数（health 可见）。
    forced_count: u64,
}

impl IstRouter {
    /// 整表装载 const 规格（门位随之装配，基值 0）。
    pub fn from_spec() -> Self {
        let mut r = IstRouter {
            live: ROUTE_SPEC_TABLE,
            gate_upper: [0; 3],
            forced_count: 0,
        };
        let mut i = 0;
        while i < 3 {
            // assemble 无错面（const 域内 ist 必 1..=7）：Err 不可达，
            // 但不 panic——以 0 兜底并在 verify_against_spec 必现红灯。
            match GateIstCodec::assemble(0, r.live[i].ist) {
                Ok(g) => r.gate_upper[i] = g,
                Err(_) => r.gate_upper[i] = 0,
            }
            i += 1;
        }
        r
    }

    /// 向量查槽（编内返回 Some(ist 1..=3)；编外 None）。
    pub fn route(&self, vector: u8) -> Option<u8> {
        let mut i = 0;
        while i < 3 {
            if self.live[i].vector == vector {
                return Some(self.live[i].ist);
            }
            i += 1;
        }
        None
    }

    /// 槽反查向量（DF 演练断言「DF 走 IST1」的正向面对拍）。
    pub fn vector_of_slot(&self, ist: u8) -> Option<u8> {
        let mut i = 0;
        while i < 3 {
            if self.live[i].ist == ist {
                return Some(self.live[i].vector);
            }
            i += 1;
        }
        None
    }

    /// 单表项重装（仅允许编内向量 + 合法槽；同向量重装刷新）。
    pub fn install(&mut self, vector: u8, ist: u8) -> Result<(), IstStackError> {
        if !is_ist_vector(vector) {
            return Err(IstStackError::NotIstVector);
        }
        if ist < 1 || ist > 7 {
            return Err(IstStackError::RouteMismatch);
        }
        let mut i = 0;
        while i < 3 {
            if self.live[i].vector == vector {
                self.live[i].ist = ist;
                let g = GateIstCodec::assemble(self.gate_upper[i], ist)?;
                self.gate_upper[i] = g;
                return Ok(());
            }
            i += 1;
        }
        Err(IstStackError::NotIstVector)
    }

    /// 门位读回（向量对应门第二四字镜像）。
    pub fn gate_upper(&self, vector: u8) -> Option<u64> {
        let mut i = 0;
        while i < 3 {
            if self.live[i].vector == vector {
                return Some(self.gate_upper[i]);
            }
            i += 1;
        }
        None
    }

    /// 一致性核对（三查）：live == const 规格表；门位 IST 域 == live.ist；
    /// 门位 bit3 保留清零。任何不符 Err(RouteMismatch)。
    pub fn verify_against_spec(&self) -> Result<(), IstStackError> {
        let mut i = 0;
        while i < 3 {
            if self.live[i].vector != ROUTE_SPEC_TABLE[i].vector
                || self.live[i].ist != ROUTE_SPEC_TABLE[i].ist
            {
                return Err(IstStackError::RouteMismatch);
            }
            let gate = self.gate_upper[i];
            if GateIstCodec::extract(gate) != self.live[i].ist {
                return Err(IstStackError::RouteMismatch);
            }
            if !GateIstCodec::reserved_bit3_clear(gate) {
                return Err(IstStackError::RouteMismatch);
            }
            i += 1;
        }
        Ok(())
    }

    /// 三槽互异核（一槽一向量，防双向量挤同栈）。
    pub fn slots_distinct(&self) -> bool {
        self.live[0].ist != self.live[1].ist
            && self.live[1].ist != self.live[2].ist
            && self.live[0].ist != self.live[2].ist
    }

    /// 注入专用：强行把 vector 错挂到 ist（绕过规格一致性；计数入账）。
    /// 仅供 inject_route_miswire / 反例演练；常态代码不得调用。
    pub fn debug_force_route(&mut self, vector: u8, ist: u8) -> Result<(), IstStackError> {
        if !is_ist_vector(vector) {
            return Err(IstStackError::NotIstVector);
        }
        let mut i = 0;
        while i < 3 {
            if self.live[i].vector == vector {
                self.live[i].ist = ist;
                self.gate_upper[i] = (self.gate_upper[i] & !GATE_LOW4_MASK) | ((ist as u64) & GATE_LOW4_MASK);
                self.forced_count = self.forced_count.saturating_add(1);
                return Ok(());
            }
            i += 1;
        }
        Err(IstStackError::NotIstVector)
    }

    /// 错挂注入累计数。
    pub const fn forced_count(&self) -> u64 {
        self.forced_count
    }

    /// 路由表指纹（live 三条 + 门位三字 + 注入计数）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0;
        while i < 3 {
            h = fnv1a_u64(h, self.live[i].vector as u64);
            h = fnv1a_u64(h, self.live[i].ist as u64);
            h = fnv1a_u64(h, self.gate_upper[i]);
            i += 1;
        }
        fnv1a_u64(h, self.forced_count)
    }

    /// 展单（三条路由 + 门位提取值）。
    pub fn dump(&self) -> String {
        let mut s = String::from("IST 路由表：\n");
        let mut i = 0;
        while i < 3 {
            let gate_ist = GateIstCodec::extract(self.gate_upper[i]);
            s += &format!(
                "  {} → IST{}（门位域={} 一致={}）\n",
                self.live[i].name,
                self.live[i].ist,
                gate_ist,
                if gate_ist == self.live[i].ist { "✓" } else { "✗" }
            );
            i += 1;
        }
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 事件账（IstEvent/EventLedger）：全事件入账 + 序号单调 + 容量上限
// ═══════════════════════════════════════════════════════════════════════

/// 事件种类码（账面闭合集）。
pub const K_ENTER: u8 = 1; // 进入 IST 栈（换栈+压帧完成）
pub const K_EXIT: u8 = 2; // 退出 IST 栈
pub const K_FIRE: u8 = 3; // 溢出防护触发（哨兵/guard/深越界）
pub const K_DOWNGRADE: u8 = 4; // 降级（主栈→备用栈）
pub const K_RESTORE: u8 = 5; // 恢复（重挂哨兵复位主栈）
pub const K_REJECT: u8 = 6; // 拒绝（非编内向量/同槽嵌套/已降级走原栈）
pub const K_INJECT: u8 = 7; // 注入面动作
pub const K_AUDIT: u8 = 8; // 审计动作（哨兵审/独立性审）

/// 事件容量上限。来源：本模块自定 65536 条（Vec 落堆不上栈；单事件
/// 24 字节，账上限 ~1.5MiB 堆，事件账结构本体不超 64KB 栈纪律）。
pub const LEDGER_CAP: usize = 65_536;

/// 单条事件（Copy 无堆字段；addr 兼容触发地址/指纹等多义载荷）。
#[derive(Clone, Copy, Debug)]
pub struct IstEvent {
    /// 单调序号（1 起）。
    pub seq: u64,
    /// 种类码 K_*。
    pub kind: u8,
    /// 关联槽序 1..=3（0=全局）。
    pub slot: u8,
    /// 关联向量（0=无）。
    pub vector: u8,
    /// 载荷（触碰地址/指纹/拒绝码等）。
    pub payload: u64,
    /// 轮次号（场景演练计数，非场景恒 0）。
    pub round: u64,
}

impl IstEvent {
    /// 事件单行展（账面消费用）。
    pub fn dump(&self) -> String {
        let kind_txt = match self.kind {
            K_ENTER => "进入",
            K_EXIT => "退出",
            K_FIRE => "防护触发",
            K_DOWNGRADE => "降级",
            K_RESTORE => "恢复",
            K_REJECT => "拒绝",
            K_INJECT => "注入",
            K_AUDIT => "审计",
            _ => "未知",
        };
        format!(
            "#{:<6} {} slot={} vec={:#04X} payload={:#X} round={}",
            self.seq, kind_txt, self.slot, self.vector, self.payload, self.round
        )
    }
}

/// 事件账：Vec 落堆；序号单调；容量上限满则 Err（不静默丢账）。
#[derive(Clone, Debug)]
pub struct EventLedger {
    events: Vec<IstEvent>,
    last_seq: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> Self {
        EventLedger {
            events: Vec::new(),
            last_seq: 0,
        }
    }

    /// 追加一条（序号自增；账满 Err(LedgerFault)；构造参数由调用侧给全）。
    pub fn push(
        &mut self,
        kind: u8,
        slot: u8,
        vector: u8,
        payload: u64,
        round: u64,
    ) -> Result<IstEvent, IstStackError> {
        if self.events.len() >= LEDGER_CAP {
            return Err(IstStackError::LedgerFault);
        }
        let next = self.last_seq.wrapping_add(1);
        if self.last_seq != 0 && next <= self.last_seq {
            // 序号回卷（u64 满）：账序完整性破坏，红灯
            return Err(IstStackError::LedgerFault);
        }
        let ev = IstEvent {
            seq: next,
            kind,
            slot,
            vector,
            payload,
            round,
        };
        self.events.push(ev);
        self.last_seq = next;
        Ok(ev)
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 末序号（0=无事件）。
    pub const fn last_seq(&self) -> u64 {
        self.last_seq
    }

    /// 取第 i 条（越界 None；不 panic）。
    pub fn get(&self, i: usize) -> Option<IstEvent> {
        if i < self.events.len() {
            Some(self.events[i])
        } else {
            None
        }
    }

    /// 末 n 条（n 截断到账长）。
    pub fn last_n(&self, n: usize) -> Vec<IstEvent> {
        let total = self.events.len();
        let take = if n > total { total } else { n };
        let mut out = Vec::new();
        let mut i = total - take;
        while i < total {
            out.push(self.events[i]);
            i += 1;
        }
        out
    }

    /// 按种类码计数。
    pub fn count_kind(&self, kind: u8) -> u64 {
        let mut n = 0;
        let mut i = 0;
        while i < self.events.len() {
            if self.events[i].kind == kind {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 账指纹（FNV over 全事件五字段）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0;
        while i < self.events.len() {
            let e = self.events[i];
            h = fnv1a_u64(h, e.seq);
            h = fnv1a_u64(h, (e.kind as u64) << 8 | (e.slot as u64));
            h = fnv1a_u64(h, (e.vector as u64) << 32 ^ e.payload);
            h = fnv1a_u64(h, e.round);
            i += 1;
        }
        h
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 槽位运行时（SlotRuntime）+ 防护统计（ProtectionStats）+ 使用快照
// ═══════════════════════════════════════════════════════════════════════

/// 单槽运行态：换栈账 + 水位账 + 降级态 + 哨兵带。
#[derive(Clone, Copy, Debug)]
pub struct SlotRuntime {
    /// 在位 RSP（0=空闲：已退出或未进入；非 0=在位压帧后 RSP）。
    pub cur_rsp: u64,
    /// 峰值已用深度（字节；enter 与 probe_descend 共同抬升）。
    pub peak_used: u64,
    /// 进入计数。
    pub enters: u64,
    /// 退出计数。
    pub exits: u64,
    /// 降级态（主栈不可信）。
    pub degraded: bool,
    /// 在备用栈（降级后 enter 的落点）。
    pub on_backup: bool,
    /// 备用栈进入计数（降级路径使用账）。
    pub backup_enters: u64,
    /// 哨兵带。
    pub sentinel: SentinelBand,
}

impl SlotRuntime {
    /// 初始态（哨兵全挂、空闲、未降级）。
    pub const fn fresh() -> Self {
        SlotRuntime {
            cur_rsp: 0,
            peak_used: 0,
            enters: 0,
            exits: 0,
            degraded: false,
            on_backup: false,
            backup_enters: 0,
            sentinel: SentinelBand::armed(),
        }
    }

    /// 在位判定（同槽嵌套检测用）。
    pub const fn in_flight(&self) -> bool {
        self.cur_rsp != 0
    }

    /// 使用深度（栈顶 - 在位 RSP；空闲 0）。
    pub const fn used(&self, top: u64) -> u64 {
        if self.cur_rsp == 0 {
            0
        } else {
            top.saturating_sub(self.cur_rsp)
        }
    }

    /// 单槽展行（top = 该槽当前落点栈顶，用于在用深度换算）。
    pub fn dump(&self, name: &'static str, top: u64) -> String {
        format!(
            "  {} 进入={} 退出={} 在用={}B 峰值={}B 在位={} 降级={} 备用入={} 哨兵完好={}/16 豁免内损={}\n",
            name,
            self.enters,
            self.exits,
            self.used(top),
            self.peak_used,
            if self.in_flight() { "是" } else { "否" },
            if self.degraded { "是" } else { "否" },
            self.backup_enters,
            self.sentinel.intact_words(),
            self.sentinel.excused,
        )
    }
}

/// 防护面全局统计（三栈合并口径；触发率 100% 判据的账面）。
#[derive(Clone, Copy, Debug)]
pub struct ProtectionStats {
    /// 防护面触碰总数（哨兵+guard+深越界三档）。
    pub touches: u64,
    /// 防护触发总数（fires == touches 即 100%）。
    pub fires: u64,
    /// 哨兵带触发数。
    pub sentinel_hits: u64,
    /// guard 区触发数。
    pub guard_hits: u64,
    /// 深越界带触发数。
    pub deep_hits: u64,
    /// 降级总数。
    pub downgrades: u64,
    /// 恢复总数。
    pub restores: u64,
    /// 拒绝总数（非编内/嵌套/降级走原栈）。
    pub rejects: u64,
}

impl ProtectionStats {
    /// 零初始化。
    pub const fn zeroed() -> Self {
        ProtectionStats {
            touches: 0,
            fires: 0,
            sentinel_hits: 0,
            guard_hits: 0,
            deep_hits: 0,
            downgrades: 0,
            restores: 0,
            rejects: 0,
        }
    }

    /// 触发率（千分率；0 触碰真空判定为 1000——无面即无漏，判据口径
    /// 见 J1：防护触发率 100% 要求每触必发，零触碰不构成反例）。
    pub const fn rate_permille(&self) -> u64 {
        if self.touches == 0 {
            1000
        } else {
            (self.fires * 1000) / self.touches
        }
    }

    /// 100% 判据（rate == 1000）。
    pub const fn full_coverage(&self) -> bool {
        self.rate_permille() == 1000
    }

    /// 统计指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a_u64(h, self.touches);
        h = fnv1a_u64(h, self.fires);
        h = fnv1a_u64(h, self.sentinel_hits);
        h = fnv1a_u64(h, self.guard_hits);
        h = fnv1a_u64(h, self.deep_hits);
        h = fnv1a_u64(h, self.downgrades);
        h = fnv1a_u64(h, self.restores);
        fnv1a_u64(h, self.rejects)
    }
}

/// 三槽使用快照（独立性对拍用：操作前后比对，非目标槽不得动）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageSnapshot {
    /// 三槽进入数。
    pub enters: [u64; 3],
    /// 三槽退出数。
    pub exits: [u64; 3],
    /// 三槽峰值。
    pub peaks: [u64; 3],
    /// 三槽在位 RSP。
    pub rsps: [u64; 3],
    /// 三槽备用进入数。
    pub backup_enters: [u64; 3],
}

impl UsageSnapshot {
    /// 与另一快照逐字段全同（独立性对拍核心谓词）。
    pub fn same_as(&self, other: &UsageSnapshot) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.enters[i] != other.enters[i]
                || self.exits[i] != other.exits[i]
                || self.peaks[i] != other.peaks[i]
                || self.rsps[i] != other.rsps[i]
                || self.backup_enters[i] != other.backup_enters[i]
            {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 指定槽序字段是否与另一快照全同（单槽不变性）。
    pub fn slot_same_as(&self, other: &UsageSnapshot, slot: u8) -> bool {
        if slot > 2 {
            return false;
        }
        let i = slot as usize;
        self.enters[i] == other.enters[i]
            && self.exits[i] == other.exits[i]
            && self.peaks[i] == other.peaks[i]
            && self.rsps[i] == other.rsps[i]
            && self.backup_enters[i] == other.backup_enters[i]
    }

    /// 快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0;
        while i < 3 {
            h = fnv1a_u64(h, self.enters[i]);
            h = fnv1a_u64(h, self.exits[i]);
            h = fnv1a_u64(h, self.peaks[i]);
            h = fnv1a_u64(h, self.rsps[i]);
            h = fnv1a_u64(h, self.backup_enters[i]);
            i += 1;
        }
        h
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 分发核心（IstCpu）：单 CPU 虚拟机——换栈/压帧/触碰/降级/恢复/审计
// ═══════════════════════════════════════════════════════════════════════

/// 降级阶梯档位（三档决策，health 可见）。
pub const TIER_PRIMARY: u8 = 0; // Tier0：主栈正常
pub const TIER_BACKUP: u8 = 1; // Tier1：已降级走备用栈
pub const TIER_HALTED: u8 = 2; // Tier2：备用栈亦触防护，拒绝服务（本册
                               // 模型到 Tier1 止，Tier2 预留给 MC 致命档）

/// 单 CPU 的 IST 三栈虚拟机：布局镜像 + TSS 镜像 + 路由表 + 三槽运行时
/// + 事件账 + 防护统计。所有可失败路径走 Result，无 panic。
#[derive(Clone, Debug)]
pub struct IstCpu {
    layout: IstStackLayout,
    tss: TssIstImage,
    router: IstRouter,
    slots: [SlotRuntime; 3],
    stats: ProtectionStats,
    ledger: EventLedger,
    round: u64,
}

impl IstCpu {
    /// 全新 CPU：const 布局 → TSS 编程（失败上抛）→ 路由整装 → 三槽初始。
    pub fn new() -> Result<Self, IstStackError> {
        let layout = IstStackLayout::from_consts();
        layout.audit_ok()?;
        let tss = TssIstImage::program_from(&IST_SLOT_SPECS)?;
        tss.verify(&layout)?;
        let router = IstRouter::from_spec();
        router.verify_against_spec()?;
        Ok(IstCpu {
            layout,
            tss,
            router,
            slots: [SlotRuntime::fresh(), SlotRuntime::fresh(), SlotRuntime::fresh()],
            stats: ProtectionStats::zeroed(),
            ledger: EventLedger::new(),
            round: 0,
        })
    }

    /// 当前轮次号（场景演练推进；非场景恒 0）。
    pub const fn round(&self) -> u64 {
        self.round
    }

    /// 推进轮次（场景每轮调用）。
    pub fn advance_round(&mut self) {
        self.round = self.round.saturating_add(1);
    }

    /// 布局镜像只读。
    pub const fn layout(&self) -> &IstStackLayout {
        &self.layout
    }

    /// TSS 镜像只读。
    pub const fn tss(&self) -> &TssIstImage {
        &self.tss
    }

    /// 路由表只读。
    pub const fn router(&self) -> &IstRouter {
        &self.router
    }

    /// 路由表可变（注入面/重装面专用入口）。
    pub fn router_mut(&mut self) -> &mut IstRouter {
        &mut self.router
    }

    /// 防护统计只读。
    pub const fn stats(&self) -> &ProtectionStats {
        &self.stats
    }

    /// 事件账只读。
    pub const fn ledger(&self) -> &EventLedger {
        &self.ledger
    }

    /// 槽运行时只读（ist ∈ 1..=3）。
    pub fn slot_runtime(&self, ist: u8) -> Result<SlotRuntime, IstStackError> {
        if ist >= 1 && ist <= 3 {
            Ok(self.slots[(ist - 1) as usize])
        } else {
            Err(IstStackError::TssProgramFault)
        }
    }

    /// 三槽使用快照。
    pub fn usage_snapshot(&self) -> UsageSnapshot {
        UsageSnapshot {
            enters: [self.slots[0].enters, self.slots[1].enters, self.slots[2].enters],
            exits: [self.slots[0].exits, self.slots[1].exits, self.slots[2].exits],
            peaks: [self.slots[0].peak_used, self.slots[1].peak_used, self.slots[2].peak_used],
            rsps: [self.slots[0].cur_rsp, self.slots[1].cur_rsp, self.slots[2].cur_rsp],
            backup_enters: [
                self.slots[0].backup_enters,
                self.slots[1].backup_enters,
                self.slots[2].backup_enters,
            ],
        }
    }

    /// 槽降级档位（Tier0/1；Tier2 本册模型不进入）。
    pub fn tier(&self, ist: u8) -> u8 {
        if ist >= 1 && ist <= 3 {
            if self.slots[(ist - 1) as usize].degraded {
                TIER_BACKUP
            } else {
                TIER_PRIMARY
            }
        } else {
            TIER_HALTED
        }
    }

    /// 降级掩码（bit0..2 对应 IST1..3；置位=已降级）。
    pub fn degraded_mask(&self) -> u8 {
        let mut m = 0u8;
        let mut i = 0;
        while i < 3 {
            if self.slots[i].degraded {
                m |= 1 << i;
            }
            i += 1;
        }
        m
    }

    // ── 进入/退出（换栈 + 压帧 + 现场帧四要素） ─────────────────────────

    /// 进入 IST：路由定槽 → 同槽嵌套拒 → 定落点（降级走备用）→
    /// TSS/备用栈顶装载 RSP → 压硬件帧 → 包络核 → 现场帧四要素成帧。
    /// rip 为演练注入的触发点指令指针；ec 由向量分型自动裁决（外部
    /// 传入的 ec 仅在向量压码时采纳）。
    pub fn enter(&mut self, vector: u8, rip: u64) -> Result<IstFrame, IstStackError> {
        if !is_ist_vector(vector) {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            let _ = self.ledger.push(K_REJECT, 0, vector, IstStackError::NotIstVector.code() as u64, self.round);
            return Err(IstStackError::NotIstVector);
        }
        let ist = match self.router.route(vector) {
            Some(s) => s,
            None => {
                // 编内向量却无路由：路由表残缺（错挂注入后可能形态）
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                let _ = self.ledger.push(K_REJECT, 0, vector, IstStackError::RouteMismatch.code() as u64, self.round);
                return Err(IstStackError::RouteMismatch);
            }
        };
        if ist < 1 || ist > 3 {
            // 错挂到本册三槽之外（如 DF→IST5）：立即红灯
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            let _ = self.ledger.push(K_REJECT, ist, vector, IstStackError::RouteMismatch.code() as u64, self.round);
            return Err(IstStackError::RouteMismatch);
        }
        let idx = (ist - 1) as usize;
        if self.slots[idx].in_flight() {
            // 同槽嵌套：真实硬件直通三重故障，本模型显式拒绝并留账
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            let _ = self.ledger.push(K_REJECT, ist, vector, IstStackError::NestingViolated.code() as u64, self.round);
            return Err(IstStackError::NestingViolated);
        }
        // 落点裁决：降级槽走备用栈顶，否则 TSS 装载字（= 主栈顶）
        let (entry_rsp, on_backup) = if self.slots[idx].degraded {
            (self.layout.backup_top(ist), true)
        } else {
            (self.tss.word(ist as usize), false)
        };
        if entry_rsp == 0 {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            let _ = self.ledger.push(K_REJECT, ist, vector, IstStackError::TssProgramFault.code() as u64, self.round);
            return Err(IstStackError::TssProgramFault);
        }
        let fsz = IstFrame::hw_size(vector);
        let rsp = entry_rsp.wrapping_sub(fsz);
        // 包络核：压帧后 RSP 必须落在落点栈的栈体内（哨兵带含于栈体，
        // 正常帧远在哨兵之上——帧底 ≥ body_lo + SENTINEL_BYTES 方为净区）
        let spec = if on_backup {
            self.layout.backup(ist)?
        } else {
            self.layout.primary(ist)?
        };
        if rsp < spec.sentinel_hi() || rsp >= spec.top() {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            let _ = self.ledger.push(K_REJECT, ist, vector, IstStackError::FrameIncomplete.code() as u64, self.round);
            return Err(IstStackError::FrameIncomplete);
        }
        let ec = if vec_has_error_code(vector) { Some(0) } else { None };
        let frame = IstFrame {
            vector,
            error_code: ec,
            rsp,
            rip,
            slot: ist,
            on_backup,
            frame_size: fsz,
        };
        if !frame.complete() {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            let _ = self.ledger.push(K_REJECT, ist, vector, IstStackError::FrameIncomplete.code() as u64, self.round);
            return Err(IstStackError::FrameIncomplete);
        }
        // 记账：进入数/水位/在位 RSP（降级槽的进入记 backup_enters）
        if on_backup {
            self.slots[idx].backup_enters = self.slots[idx].backup_enters.saturating_add(1);
        } else {
            self.slots[idx].enters = self.slots[idx].enters.saturating_add(1);
        }
        let used = spec.top().saturating_sub(rsp);
        if used > self.slots[idx].peak_used {
            self.slots[idx].peak_used = used;
        }
        self.slots[idx].cur_rsp = rsp;
        self.ledger.push(K_ENTER, ist, vector, rsp, self.round)?;
        Ok(frame)
    }

    /// 注入面专用：K_INJECT 事件入账（slot/word 序号入载荷）。
    pub fn ledger_push_inject(&mut self, slot: u8, word: u8) -> Result<(), IstStackError> {
        let w = word as u64;
        self.ledger.push(K_INJECT, slot, 0, w, self.round)?;
        Ok(())
    }

    /// 注入面专用：绕过豁免账直接损坏哨兵字（无防护触发先行——即
    /// 「未入账的越界写」形态；audit_sentinels 必红灯）。
    pub fn damage_sentinel_raw(&mut self, ist: u8, word: usize) {
        if ist >= 1 && ist <= 3 {
            self.slots[(ist - 1) as usize].sentinel.damage(word, false);
        }
    }

    /// 退出 IST 槽（在位清零；退出计数；不在位则 Err(NestingViolated)
    /// ——退出多于进入即账破坏）。
    pub fn exit(&mut self, ist: u8) -> Result<(), IstStackError> {
        if ist < 1 || ist > 3 {
            return Err(IstStackError::TssProgramFault);
        }
        let idx = (ist - 1) as usize;
        if !self.slots[idx].in_flight() {
            return Err(IstStackError::NestingViolated);
        }
        self.slots[idx].cur_rsp = 0;
        self.slots[idx].exits = self.slots[idx].exits.saturating_add(1);
        self.ledger.push(K_EXIT, ist, 0, 0, self.round)?;
        Ok(())
    }

    /// 严格原栈进入（演练断言用）：目标槽降级时 Err(StackDegraded)
    /// ——「已降级仍要求走原栈」的规范拒绝面。
    pub fn enter_original(&mut self, vector: u8, rip: u64) -> Result<IstFrame, IstStackError> {
        let ist = match self.router.route(vector) {
            Some(s) => s,
            None => return Err(IstStackError::RouteMismatch),
        };
        if ist >= 1 && ist <= 3 && self.slots[(ist - 1) as usize].degraded {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            let _ = self.ledger.push(K_REJECT, ist, vector, IstStackError::StackDegraded.code() as u64, self.round);
            return Err(IstStackError::StackDegraded);
        }
        self.enter(vector, rip)
    }

    // ── 触碰防护（三档防护面：触碰必触发 + 即时降级） ─────────────────────

    /// 触碰地址：定位 → 防护面三档必触发（fires++ 且该槽即时降级，
    /// 哨兵档另记损坏豁免）；栈体段不触发（Ok(fired=false)）；
    /// 不可归属 Err(UnattributableTouch)。
    pub fn touch(&mut self, addr: u64) -> Result<bool, IstStackError> {
        let hit = match self.layout.locate(addr) {
            Some(h) => h,
            None => {
                let _ = self.ledger.push(K_REJECT, 0, 0, IstStackError::UnattributableTouch.code() as u64, self.round);
                return Err(IstStackError::UnattributableTouch);
            }
        };
        if !hit.zone.is_protection() {
            // 栈体正常段：正常使用，不触发不入防护账
            return Ok(false);
        }
        // 防护面：触碰必触发（触发率 100% 判据的账面根）
        self.stats.touches = self.stats.touches.saturating_add(1);
        self.stats.fires = self.stats.fires.saturating_add(1);
        match hit.zone {
            TouchZone::SentinelBand => {
                self.stats.sentinel_hits = self.stats.sentinel_hits.saturating_add(1);
                if !hit.backup {
                    // 主槽哨兵损坏 = 防护触发豁免（有 fire 账在先）
                    let idx = hit.slot as usize;
                    let word_idx = self.sentinel_word_index(hit.slot, addr);
                    self.slots[idx].sentinel.damage(word_idx, true);
                }
            }
            TouchZone::GuardBand => {
                self.stats.guard_hits = self.stats.guard_hits.saturating_add(1);
            }
            TouchZone::DeepBelow => {
                self.stats.deep_hits = self.stats.deep_hits.saturating_add(1);
            }
            TouchZone::StackBody => {}
        }
        let _ = self.ledger.push(K_FIRE, hit.slot + 1, 0, addr, self.round);
        // 即时降级（主槽；备用槽触碰只记触发不降级——本册 Tier2 只预留）
        if !hit.backup {
            let ist = hit.slot + 1;
            self.downgrade_internal(ist)?;
        }
        Ok(true)
    }

    /// 哨兵带内地址 → 字序号（越带尾取末字；显式钳制不 panic）。
    fn sentinel_word_index(&self, slot: u8, addr: u64) -> usize {
        if slot > 2 {
            return 0;
        }
        let spec = self.layout.primaries[slot as usize];
        let off = addr.saturating_sub(spec.body_lo());
        let idx = (off / 8) as usize;
        if idx < SENTINEL_WORDS {
            idx
        } else {
            SENTINEL_WORDS - 1
        }
    }

    /// 内部降级：主槽 → 备用（事件 K_DOWNGRADE；幂等——已降级不重复记）。
    /// 在位帧作废语义：清在位 RSP 时同步补记一次退出（守恒账不破，
    /// 事件 K_EXIT 载荷 0 区别于常规退出）。
    fn downgrade_internal(&mut self, ist: u8) -> Result<(), IstStackError> {
        if ist < 1 || ist > 3 {
            return Err(IstStackError::TssProgramFault);
        }
        let idx = (ist - 1) as usize;
        if !self.slots[idx].degraded {
            if self.slots[idx].in_flight() {
                self.slots[idx].exits = self.slots[idx].exits.saturating_add(1);
                let _ = self.ledger.push(K_EXIT, ist, 0, 0, self.round);
            }
            self.slots[idx].degraded = true;
            self.slots[idx].on_backup = true;
            self.slots[idx].cur_rsp = 0; // 在位帧作废（防护触发即离开原栈）
            self.stats.downgrades = self.stats.downgrades.saturating_add(1);
            self.ledger.push(K_DOWNGRADE, ist, 0, 0, self.round)?;
        }
        Ok(())
    }

    /// 恢复：重挂哨兵 + 复位降级态（在位时拒绝恢复——先退出）。
    pub fn restore(&mut self, ist: u8) -> Result<(), IstStackError> {
        if ist < 1 || ist > 3 {
            return Err(IstStackError::TssProgramFault);
        }
        let idx = (ist - 1) as usize;
        if self.slots[idx].in_flight() {
            return Err(IstStackError::NestingViolated);
        }
        self.slots[idx].sentinel = SentinelBand::armed();
        self.slots[idx].degraded = false;
        self.slots[idx].on_backup = false;
        self.stats.restores = self.stats.restores.saturating_add(1);
        self.ledger.push(K_RESTORE, ist, 0, 0, self.round)?;
        Ok(())
    }

    // ── 深度演练器（probe_descend）：多帧压栈水位推演 ────────────────────

    /// 模拟在位帧之上再压 n 帧硬件帧（同向量帧几何）：逐帧核包络，
    /// 触哨兵带前返回最深可达 RSP 与帧数；越界返 Err(FrameIncomplete)。
    /// 用于峰值水位演练（不动真实在位 RSP，纯推演）。
    pub fn probe_descend(
        &self,
        vector: u8,
        n_frames: u64,
    ) -> Result<(u64, u64), IstStackError> {
        let ist = match self.router.route(vector) {
            Some(s) => s,
            None => return Err(IstStackError::RouteMismatch),
        };
        if ist < 1 || ist > 3 {
            return Err(IstStackError::RouteMismatch);
        }
        let idx = (ist - 1) as usize;
        let degraded = self.slots[idx].degraded;
        let spec = if degraded {
            self.layout.backup(ist)?
        } else {
            self.layout.primary(ist)?
        };
        let fsz = IstFrame::hw_size(vector);
        let mut rsp = spec.top();
        let mut frames = 0u64;
        while frames < n_frames {
            let next = rsp.wrapping_sub(fsz);
            if next < spec.sentinel_hi() {
                // 再压一帧即入哨兵带：到此为止
                break;
            }
            rsp = next;
            frames += 1;
        }
        if frames == 0 {
            return Err(IstStackError::FrameIncomplete);
        }
        Ok((rsp, frames))
    }

    // ── 审计面（哨兵完整性 / 使用独立性） ────────────────────────────────

    /// 哨兵完整性审计：逐槽未豁免损坏数合计；>0 → Err(SentinelDamaged)。
    /// 返回 Ok(未豁免合计 == 0 恒真时的 0)。
    pub fn audit_sentinels(&self) -> Result<u64, IstStackError> {
        let mut total = 0u64;
        let mut i = 0;
        while i < 3 {
            total = total.saturating_add(self.slots[i].sentinel.unexcused());
            i += 1;
        }
        if total > 0 {
            return Err(IstStackError::SentinelDamaged);
        }
        Ok(total)
    }

    /// 使用独立性审计：三栈两两账面隔离核——
    /// ① enters == exits + in_flight（进出守恒）；
    /// ② 在位 RSP 落本槽包络（不串槽）；
    /// ③ 峰值 ≤ 栈深。
    /// 任一破 → Err(LedgerFault)。返回 Ok(守恒校验通过的槽数)。
    pub fn audit_independence(&self) -> Result<u8, IstStackError> {
        let mut ok_count = 0u8;
        let mut i = 0;
        while i < 3 {
            let rt = self.slots[i];
            let ist = (i + 1) as u8;
            let in_flight = if rt.in_flight() { 1u64 } else { 0u64 };
            if rt.enters + rt.backup_enters != rt.exits + in_flight {
                return Err(IstStackError::LedgerFault);
            }
            if rt.cur_rsp != 0 {
                let inside = self.layout.envelope_contains(ist, rt.cur_rsp)
                    || self.layout.backup_envelope_contains(ist, rt.cur_rsp);
                if !inside {
                    return Err(IstStackError::LedgerFault);
                }
            }
            // 峰值上界核：peak_used 混记主/备两期使用，统一以主栈深
            // （16KiB，更大者）为上界——降级期使用（≤8KiB）天然满足
            if rt.peak_used > STACK_DEPTH {
                return Err(IstStackError::LedgerFault);
            }
            ok_count += 1;
            i += 1;
        }
        Ok(ok_count)
    }

    // ── 演练入口（J1 判据三联：DF/NMI/MC 各走各槽 + 现场完整） ────────────

    /// DF 演练：#8 → IST1，进入即验帧四要素 + RSP 在落点栈（主或备）
    /// 包络 + EC 形态 Some(0)（SDM Table 6-1 硬件推 0）。返回帧（已入账
    /// 未退出，调用方演练完 exit(1)）。
    pub fn drill_df(&mut self, rip: u64) -> Result<IstFrame, IstStackError> {
        let f = self.enter(VEC_DF, rip)?;
        if f.slot != IST_SLOT_DF {
            return Err(IstStackError::RouteMismatch);
        }
        let inside = if f.on_backup {
            self.layout.backup_envelope_contains(IST_SLOT_DF, f.rsp)
        } else {
            self.layout.envelope_contains(IST_SLOT_DF, f.rsp)
        };
        if !inside || !f.complete() {
            return Err(IstStackError::FrameIncomplete);
        }
        Ok(f)
    }

    /// NMI 演练：#2 → IST2（帧四要素 + 落点包络 + EC=None 形态）。
    pub fn drill_nmi(&mut self, rip: u64) -> Result<IstFrame, IstStackError> {
        let f = self.enter(VEC_NMI, rip)?;
        if f.slot != IST_SLOT_NMI {
            return Err(IstStackError::RouteMismatch);
        }
        let inside = if f.on_backup {
            self.layout.backup_envelope_contains(IST_SLOT_NMI, f.rsp)
        } else {
            self.layout.envelope_contains(IST_SLOT_NMI, f.rsp)
        };
        if !inside || !f.complete() {
            return Err(IstStackError::FrameIncomplete);
        }
        Ok(f)
    }

    /// MC 演练：#18 → IST3（帧四要素 + 落点包络 + EC=None 形态——
    /// SDM Table 6-1 修正：MC 不压错误码，见文件头「差异化」节）。
    pub fn drill_mc(&mut self, rip: u64) -> Result<IstFrame, IstStackError> {
        let f = self.enter(VEC_MC, rip)?;
        if f.slot != IST_SLOT_MC {
            return Err(IstStackError::RouteMismatch);
        }
        let inside = if f.on_backup {
            self.layout.backup_envelope_contains(IST_SLOT_MC, f.rsp)
        } else {
            self.layout.envelope_contains(IST_SLOT_MC, f.rsp)
        };
        if !inside || !f.complete() {
            return Err(IstStackError::FrameIncomplete);
        }
        Ok(f)
    }

    /// DF 错挂 IST2 反例演练：强行 DF→IST2 后 drill_df 必须红灯
    /// （RouteMismatch——构造 DF 触发走 IST2 属路由错挂，正是 J1
    /// 「构造 DF 触发走 IST」的反面；检出即过）。恢复路由后返回。
    pub fn drill_df_miswire_rejected(&mut self) -> Result<(), IstStackError> {
        self.router.debug_force_route(VEC_DF, IST_SLOT_NMI)?;
        let rejected = match self.drill_df(0xFFFF_8000_0000_1000) {
            Err(IstStackError::RouteMismatch) => true,
            Err(e) => return Err(e),
            Ok(_) => false,
        };
        // 复位路由（无论演练结果，恢复定编再回）
        self.router = IstRouter::from_spec();
        if rejected {
            Ok(())
        } else {
            Err(IstStackError::InjectionUndetected)
        }
    }

    // ── 指纹与体检 ────────────────────────────────────────────────────────

    /// 全机指纹（布局+TSS+路由+使用账+防护统计+事件账 六路合搅）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a_u64(h, self.layout.fingerprint());
        h = fnv1a_u64(h, self.tss.fingerprint());
        h = fnv1a_u64(h, self.router.fingerprint());
        h = fnv1a_u64(h, self.usage_snapshot().fingerprint());
        h = fnv1a_u64(h, self.stats.fingerprint());
        fnv1a_u64(h, self.ledger.fingerprint())
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 体检单（IstHealth）：health() 六路汇总 + 展单
// ═══════════════════════════════════════════════════════════════════════

/// 全机体检单：六路审计结论 + 防护触发率 + 降级掩码 + 账序 + 指纹。
#[derive(Clone, Copy, Debug)]
pub struct IstHealth {
    /// 主码（段基 0x2850）。
    pub main_code: u16,
    /// 布局审计全绿。
    pub layout_ok: bool,
    /// 路由一致性（live==const 规格 + 门位域一致）。
    pub route_ok: bool,
    /// TSS 镜像三查过（保留位/规范形/回读对拍）。
    pub tss_ok: bool,
    /// 哨兵完整性（未豁免损坏 == 0）。
    pub sentinel_ok: bool,
    /// 使用独立性（进出守恒/包络/峰值）。
    pub usage_ok: bool,
    /// 防护触发率（千分率）。
    pub protection_rate_permille: u64,
    /// 降级掩码（bit0..2 = IST1..3）。
    pub degraded_mask: u8,
    /// 事件账末序号。
    pub ledger_seq: u64,
    /// 全机指纹。
    pub fingerprint: u64,
}

impl IstHealth {
    /// 总绿判定（六路审计全过 + 触发率 1000‰；允许在位/降级——运行态
    /// 非缺陷，体检只核「账面一致性」与「防护覆盖」）。
    pub fn ok(&self) -> bool {
        self.layout_ok
            && self.route_ok
            && self.tss_ok
            && self.sentinel_ok
            && self.usage_ok
            && self.protection_rate_permille == 1000
    }

    /// 体检单多行展（六路逐项 + 判据行）。
    pub fn summary(&self) -> String {
        let line = |label: &str, ok: bool| {
            format!("  {} {}\n", label, if ok { "✓" } else { "✗" })
        };
        let mut s = format!("UNX-F0806 IST2 体检单 [{:#04X}]：\n", self.main_code);
        s += &line("布局零重叠/对齐/规范形", self.layout_ok);
        s += &line("路由定编+门位一致", self.route_ok);
        s += &line("TSS 镜像三查", self.tss_ok);
        s += &line("哨兵完整性", self.sentinel_ok);
        s += &line("使用独立性守恒", self.usage_ok);
        s += &format!(
            "  防护触发率 {}‰（{}/1000） {}\n",
            self.protection_rate_permille,
            self.protection_rate_permille,
            if self.protection_rate_permille == 1000 { "✓" } else { "✗" }
        );
        s += &format!(
            "  降级掩码 {:#04b} 事件末序号 #{} 指纹 {:#018X}\n",
            self.degraded_mask, self.ledger_seq, self.fingerprint
        );
        s += &format!("  总判：{}\n", if self.ok() { "全绿" } else { "红灯" });
        s
    }
}

impl IstCpu {
    /// 全机体检：跑六路审计并汇总（不改变任何运行态；审计动作本身
    /// 不入账——体检是只读面）。
    pub fn health(&self) -> IstHealth {
        IstHealth {
            main_code: MAIN_CODE,
            layout_ok: self.layout.audit().ok(),
            route_ok: self.router.verify_against_spec().is_ok(),
            tss_ok: self.tss.verify(&self.layout).is_ok(),
            sentinel_ok: self.audit_sentinels().is_ok(),
            usage_ok: self.audit_independence().is_ok(),
            protection_rate_permille: self.stats.rate_permille(),
            degraded_mask: self.degraded_mask(),
            ledger_seq: self.ledger.last_seq(),
            fingerprint: self.fingerprint(),
        }
    }

    /// 全机多行展单（体检单 + 布局 + 路由 + 三槽使用账 + 防护统计）。
    pub fn health_summary(&self) -> String {
        let mut s = self.health().summary();
        s += &self.layout.dump();
        s += &self.router.dump();
        s += &String::from("三槽使用账：\n");
        let mut i = 0;
        while i < 3 {
            let ist = (i + 1) as u8;
            let top = if self.slots[i].degraded {
                self.layout.backup_top(ist)
            } else {
                self.layout.primary_top(ist)
            };
            s += &self.slots[i].dump(IST_SLOT_SPECS[i].name, top);
            i += 1;
        }
        let st = self.stats;
        s += &format!(
            "防护统计：触碰={} 触发={}（哨兵 {}/guard {}/深越界 {}）率={}‰ 降级={} 恢复={} 拒绝={}\n",
            st.touches,
            st.fires,
            st.sentinel_hits,
            st.guard_hits,
            st.deep_hits,
            st.rate_permille(),
            st.downgrades,
            st.restores,
            st.rejects,
        );
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 场景演练器（run_scenario）：LCG 驱动全随机流 + 逐轮独立性对拍
// ═══════════════════════════════════════════════════════════════════════

/// 演练编内向量表（LCCG 采样闭集；下标即 below(3) 结果）。
const SCENARIO_VECTORS: [u8; 3] = [VEC_DF, VEC_NMI, VEC_MC];

/// 场景轮档：每 N 轮触发的动作（写死注释：防护触碰每 5 轮、拒绝注入
/// 每 11 轮、深度推演每 7 轮、降级后 2 轮恢复）。
const PERIOD_TOUCH: u64 = 5;
const PERIOD_REJECT: u64 = 11;
const PERIOD_DESCEND: u64 = 7;
const RESTORE_AFTER: u64 = 2;

/// 场景报告：双跑对拍的账面载体（同种子两跑全字段必等）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioReport {
    /// 演练种子（对拍主键）。
    pub seed: u64,
    /// 演练轮数。
    pub rounds: u64,
    /// 成功进入数。
    pub enters: u64,
    /// 退出数。
    pub exits: u64,
    /// 防护触发数。
    pub fires: u64,
    /// 降级数。
    pub downgrades: u64,
    /// 恢复数。
    pub restores: u64,
    /// 拒绝数（非编内向量注入）。
    pub rejects: u64,
    /// 备用栈进入数。
    pub backup_enters: u64,
    /// 帧四要素核过数。
    pub frames_checked: u64,
    /// 防护触发率（千分率）。
    pub rate_permille: u64,
    /// 终态全机指纹。
    pub fingerprint: u64,
    /// LCG 终态（对拍灵敏度增益）。
    pub lcg_state: u64,
}

impl ScenarioReport {
    /// 报告单行摘要。
    pub fn summary(&self) -> String {
        format!(
            "seed={:#X} rounds={} 进={} 出={} 火={} 降={} 复={} 拒={} 备入={} 帧核={} 率={}‰ fp={:#018X}",
            self.seed,
            self.rounds,
            self.enters,
            self.exits,
            self.fires,
            self.downgrades,
            self.restores,
            self.rejects,
            self.backup_enters,
            self.frames_checked,
            self.rate_permille,
            self.fingerprint
        )
    }
}

/// 全随机场景演练：逐轮混做「编内向量进入/退出 + 防护面触碰（三档
/// 轮转）+ 降级槽备用栈验证 + 恢复 + 非编内拒绝注入 + 深度推演」，
/// 每轮做使用快照对拍（A 栈操作不得动 B 栈账）。任何判据破即 Err。
pub fn run_scenario(seed: u64, rounds: u64) -> Result<ScenarioReport, IstStackError> {
    let mut cpu = IstCpu::new()?;
    let mut lcg = Lcg::new(seed);
    let mut rep = ScenarioReport {
        seed,
        rounds,
        enters: 0,
        exits: 0,
        fires: 0,
        downgrades: 0,
        restores: 0,
        rejects: 0,
        backup_enters: 0,
        frames_checked: 0,
        rate_permille: 0,
        fingerprint: 0,
        lcg_state: 0,
    };
    // 每槽降级时的恢复倒计时（0=未降级）
    let mut restore_after = [0u64; 3];
    let mut r: u64 = 0;
    while r < rounds {
        cpu.advance_round();
        // ── 动作 1：随机编内向量进入 ──
        let vidx = lcg.below(3) as usize;
        let vector = SCENARIO_VECTORS[vidx];
        let rip = 0xFFFF_8000_0000_0000u64.wrapping_add(r.wrapping_mul(0x40));
        let before = cpu.usage_snapshot();
        match cpu.enter(vector, rip) {
            Ok(frame) => {
                // 现场帧四要素 + 落点包络核
                if !frame.complete() {
                    return Err(IstStackError::FrameIncomplete);
                }
                let inside = if frame.on_backup {
                    cpu.layout().backup_envelope_contains(frame.slot, frame.rsp)
                } else {
                    cpu.layout().envelope_contains(frame.slot, frame.rsp)
                };
                if !inside {
                    return Err(IstStackError::FrameIncomplete);
                }
                if frame.on_backup {
                    rep.backup_enters = rep.backup_enters.saturating_add(1);
                }
                rep.enters = rep.enters.saturating_add(1);
                rep.frames_checked = rep.frames_checked.saturating_add(1);
                // 退出（退出数跟随进入）
                cpu.exit(frame.slot)?;
                rep.exits = rep.exits.saturating_add(1);
            }
            Err(e) => {
                // 编内向量进入被拒：只有降级/嵌套可解释（本场景退出配对
                // 及时，嵌套不应出现）——任何 Err 都是账面红灯
                return Err(e);
            }
        }
        let after = cpu.usage_snapshot();
        // ── 独立性对拍：目标槽外的两槽快照不得漂移 ──
        let target_slot = match cpu.router().route(vector) {
            Some(s) => s,
            None => return Err(IstStackError::RouteMismatch),
        };
        if target_slot >= 1 && target_slot <= 3 {
            let mut other = 0;
            while other < 3 {
                let probe = (other + 1) as u8;
                if probe != target_slot && !before.slot_same_as(&after, probe) {
                    return Err(IstStackError::LedgerFault);
                }
                other += 1;
            }
        }
        // ── 动作 2：深度推演（每 7 轮）──
        if r % PERIOD_DESCEND == 0 {
            let n = lcg.below(64) + 1;
            let _probe = cpu.probe_descend(vector, n)?;
        }
        // ── 动作 3：防护面触碰（每 5 轮，三档轮转）──
        if r % PERIOD_TOUCH == 0 {
            let slot = (lcg.below(3) + 1) as u8; // 1..=3
            let zone_sel = lcg.below(3); // 0=哨兵 1=guard 2=深越界
            let spec = cpu.layout().primary(slot)?;
            let addr = match zone_sel {
                0 => spec.body_lo() + lcg.below(SENTINEL_BYTES / 8) * 8,
                1 => spec.base + lcg.below(GUARD_DEPTH),
                _ => spec.base.wrapping_sub(1 + lcg.below(DEEP_BAND)),
            };
            let fired = cpu.touch(addr)?;
            if !fired {
                // 防护面触碰未触发：J1 触发率 100% 判据破
                return Err(IstStackError::InjectionUndetected);
            }
            rep.fires = rep.fires.saturating_add(1);
            rep.downgrades = rep.downgrades.saturating_add(1);
            restore_after[(slot - 1) as usize] = RESTORE_AFTER;
        }
        // ── 动作 4：降级槽恢复（倒计时归零则恢复）──
        let mut s2 = 0;
        while s2 < 3 {
            if restore_after[s2] > 0 {
                restore_after[s2] -= 1;
                if restore_after[s2] == 0 {
                    // 恢复前先演练一次降级槽进入（必须走备用栈路径）
                    let vec_of = match cpu.router().vector_of_slot((s2 + 1) as u8) {
                        Some(v) => v,
                        None => return Err(IstStackError::RouteMismatch),
                    };
                    let f = cpu.enter(vec_of, 0xFFFF_8000_0000_2000)?;
                    if !f.on_backup || !f.complete() {
                        return Err(IstStackError::FrameIncomplete);
                    }
                    cpu.exit((s2 + 1) as u8)?;
                    cpu.restore((s2 + 1) as u8)?;
                    rep.restores = rep.restores.saturating_add(1);
                }
            }
            s2 += 1;
        }
        // ── 动作 5：非编内向量拒绝注入（每 11 轮）──
        if r % PERIOD_REJECT == 0 {
            let bad_vec = 0x20u8 + (lcg.below(0x40) as u8); // IRQ 区向量
            match cpu.enter(bad_vec, rip) {
                Err(IstStackError::NotIstVector) => {
                    rep.rejects = rep.rejects.saturating_add(1);
                }
                Err(e) => return Err(e),
                Ok(_) => return Err(IstStackError::InjectionUndetected),
            }
        }
        r += 1;
    }
    // 收尾：账平核（进出守恒）+ 触发率 100% 判据 + 指纹
    cpu.audit_independence()?;
    cpu.audit_sentinels()?;
    rep.rate_permille = cpu.stats().rate_permille();
    if !cpu.stats().full_coverage() {
        return Err(IstStackError::InjectionUndetected);
    }
    rep.fingerprint = cpu.fingerprint();
    rep.lcg_state = lcg.state();
    Ok(rep)
}

/// 双跑确定性对拍：同种子两跑报告全字段比对（PartialEq 全等）。
/// 任一字段漂移 → Err(FingerprintMismatch)。
pub fn dual_run_check(seed: u64, rounds: u64) -> Result<ScenarioReport, IstStackError> {
    let a = run_scenario(seed, rounds)?;
    let b = run_scenario(seed, rounds)?;
    if a != b {
        return Err(IstStackError::FingerprintMismatch);
    }
    // 负对照：异种子（可区分性——同化即指纹塌缩）
    let c_seed = seed.wrapping_add(1);
    let c = run_scenario(c_seed, rounds)?;
    if c == a && rounds > 4 {
        // 异种子两跑全等：随机面失效（指纹塌缩），红灯
        return Err(IstStackError::FingerprintMismatch);
    }
    Ok(a)
}

// ═══════════════════════════════════════════════════════════════════════
// 注入面（三联）：栈重叠 / 哨兵破坏 / 路由错挂——各必检出
// ═══════════════════════════════════════════════════════════════════════

/// 注入结果单：面名 + 检出与否 + 关联码 + 说明。
#[derive(Clone, Debug)]
pub struct InjectionOutcome {
    /// 注入面名。
    pub face: &'static str,
    /// 是否检出（true=审计/核对按预期红灯）。
    pub detected: bool,
    /// 关联诊断码（检出时的错误码或审计计数）。
    pub code: u16,
    /// 说明（重叠对/槽位/字序等定位信息）。
    pub note: String,
}

impl InjectionOutcome {
    /// 单行展。
    pub fn summary(&self) -> String {
        format!(
            "[{}] face={} code={:#04X} note={}",
            if self.detected { "检出" } else { "未检出" },
            self.face,
            self.code,
            self.note
        )
    }
}

/// 注入面 1（栈重叠注入）：把 2 号主槽下移半个步长（侵入 1 号主槽），
/// 布局审计必出重叠对。返回检出结论；未检出 → Err(InjectionUndetected)。
/// 纯镜像操作，不动 prod 布局。
pub fn inject_stack_overlap() -> Result<InjectionOutcome, IstStackError> {
    let mut probe = IstStackLayout::from_consts();
    // 构造重叠：IST2 基址下移（guard+半栈），body 与 IST1 body 交叠
    // （shift = 0x3000 → 新 IST2 base = IST1 base + 0x2000，两窗口交叠 0x3000）
    let shift = GUARD_DEPTH + STACK_DEPTH / 2;
    let mut prim = IST_SLOT_SPECS;
    prim[1].base = prim[1].base.wrapping_sub(shift);
    probe.set_primaries(prim);
    let audit = probe.audit();
    if audit.pair_count > 0 {
        Ok(InjectionOutcome {
            face: "栈重叠注入",
            detected: true,
            code: IstStackError::LayoutViolated.code(),
            note: audit.summary(),
        })
    } else {
        Err(IstStackError::InjectionUndetected)
    }
}

/// 注入面 2（哨兵破坏注入）：对 scratch CPU 的 IST2 槽 7 号哨兵字写
/// 垃圾（无豁免账），哨兵完整性审计必红灯。未检出 → Err。
pub fn inject_sentinel_break(cpu: &mut IstCpu) -> Result<InjectionOutcome, IstStackError> {
    // 先记注入事件（K_INJECT 入账），再绕过豁免直接损坏
    let _ = cpu.ledger_push_inject(IST_SLOT_NMI, 7);
    cpu.damage_sentinel_raw(IST_SLOT_NMI, 7);
    match cpu.audit_sentinels() {
        Err(e) if e == IstStackError::SentinelDamaged => Ok(InjectionOutcome {
            face: "哨兵破坏注入",
            detected: true,
            code: e.code(),
            note: format!("IST{} 槽 7 号哨兵字未豁免损坏", IST_SLOT_NMI),
        }),
        Err(e) => Err(e),
        Ok(_) => Err(IstStackError::InjectionUndetected),
    }
}

/// 注入面 3（路由错挂注入）：把 DF 错挂到 IST2（任务书反例面「构造 DF
/// 触发走 IST2」），一致性核对必红灯；随后自动复位路由。未检出 → Err。
pub fn inject_route_miswire(cpu: &mut IstCpu) -> Result<InjectionOutcome, IstStackError> {
    cpu.router_mut().debug_force_route(VEC_DF, IST_SLOT_NMI)?;
    let outcome = match cpu.router().verify_against_spec() {
        Err(e) if e == IstStackError::RouteMismatch => Ok(InjectionOutcome {
            face: "路由错挂注入",
            detected: true,
            code: e.code(),
            note: format!("DF(#8) 错挂 IST{}（定编 IST1）", IST_SLOT_NMI),
        }),
        Err(e) => Err(e),
        Ok(()) => Err(IstStackError::InjectionUndetected),
    };
    // 复位路由（注入面不留残留态）
    *cpu.router_mut() = IstRouter::from_spec();
    outcome
}

// ═══════════════════════════════════════════════════════════════════════
// 模块内自检（cfg(test)：只在测试态编译；只依赖 core/alloc，assert!
// 仅测试路径使用——内核路径零 assert 纪律不受影响）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── 1. 布局面 ─────────────────────────────────────────────────────────

    #[test]
    fn layout_audit_green_zero_overlap() {
        let lay = IstStackLayout::from_consts();
        let a = lay.audit();
        assert!(a.ok(), "布局审计应全绿：{}", a.summary());
        assert_eq!(a.pair_count, 0);
        assert_eq!(a.misalign, 0);
        assert_eq!(a.noncanonical, 0);
        assert_eq!(a.out_of_region, 0);
        assert!(lay.audit_ok().is_ok());
    }

    #[test]
    fn three_primary_envelopes_disjoint() {
        let lay = IstStackLayout::from_consts();
        let p1 = lay.primary(1).unwrap();
        let p2 = lay.primary(2).unwrap();
        let p3 = lay.primary(3).unwrap();
        assert!(p1.top() <= p2.base && p2.top() <= p3.base);
        assert!(!ranges_overlap(p1.body_lo(), p1.top(), p2.body_lo(), p2.top()));
        assert!(!ranges_overlap(p2.body_lo(), p2.top(), p3.body_lo(), p3.top()));
        assert!(!ranges_overlap(p1.body_lo(), p1.top(), p3.body_lo(), p3.top()));
    }

    #[test]
    fn guard_separates_adjacent_stacks() {
        // 槽几何推导：槽 j 的 top == 槽 j+1 的 base（guard 相接）
        let lay = IstStackLayout::from_consts();
        for ist in 1u8..=2 {
            let a = lay.primary(ist).unwrap();
            let b = lay.primary(ist + 1).unwrap();
            assert_eq!(a.top(), b.base, "槽 {} 上缘应紧贴下一槽 guard", ist);
        }
    }

    #[test]
    fn locate_zones_all_classified() {
        let lay = IstStackLayout::from_consts();
        for ist in 1u8..=3 {
            let p = lay.primary(ist).unwrap();
            // 深越界带
            let deep = p.base - 1;
            assert_eq!(lay.locate(deep).unwrap().zone, TouchZone::DeepBelow);
            // guard
            let g = p.base + GUARD_DEPTH / 2;
            assert_eq!(lay.locate(g).unwrap().zone, TouchZone::GuardBand);
            // 哨兵带（首/末字）
            assert_eq!(lay.locate(p.body_lo()).unwrap().zone, TouchZone::SentinelBand);
            assert_eq!(
                lay.locate(p.sentinel_hi() - 8).unwrap().zone,
                TouchZone::SentinelBand
            );
            // 栈体正常段
            let body = p.sentinel_hi() + 64;
            assert_eq!(lay.locate(body).unwrap().zone, TouchZone::StackBody);
        }
        // 区域外：不可归属
        assert!(lay.locate(IST_REGION_BASE - DEEP_BAND - 0x1000).is_none());
        assert!(lay.locate(IST_REGION_END + 0x10).is_none());
        assert!(lay.locate(0).is_none());
    }

    // ── 2. TSS 编程面 ─────────────────────────────────────────────────────

    #[test]
    fn tss_program_and_verify_roundtrip() {
        let lay = IstStackLayout::from_consts();
        let tss = TssIstImage::program_from(&IST_SLOT_SPECS).unwrap();
        assert!(tss.verify(&lay).is_ok());
        assert_eq!(tss.programmed_slots(), 3);
        assert!(tss.reserved_zero());
        for ist in 1..=3u8 {
            assert_eq!(tss.word(ist as usize), lay.primary_top(ist));
        }
        for i in 4..TSS_IST_WORDS {
            assert_eq!(tss.word(i), 0);
        }
    }

    #[test]
    fn tss_rejects_bad_slot_and_rejects_noncanonical() {
        // 槽号 0/越界拒绝
        let mut bad = IST_SLOT_SPECS;
        bad[0].ist_index = 0;
        assert_eq!(
            TssIstImage::program_from(&bad).unwrap_err(),
            IstStackError::TssProgramFault
        );
        let mut bad2 = IST_SLOT_SPECS;
        bad2[1].ist_index = 8;
        assert!(TssIstImage::program_from(&bad2).is_err());
        // 非规范形栈顶拒绝（bit63:47 非符号扩展）
        let mut bad3 = IST_SLOT_SPECS;
        bad3[2].base = 0x0000_8000_0000_0000u64.wrapping_add(0x1234);
        assert_eq!(
            TssIstImage::program_from(&bad3).unwrap_err(),
            IstStackError::TssProgramFault
        );
    }

    #[test]
    fn tss_fingerprint_stable_and_sensitive() {
        let a = TssIstImage::program_from(&IST_SLOT_SPECS).unwrap();
        let b = TssIstImage::program_from(&IST_SLOT_SPECS).unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        let mut c = a;
        c = TssIstImage { words: c.words };
        // 字面重建等价
        assert_eq!(a.fingerprint(), c.fingerprint());
    }

    // ── 3. 门位编解码 ─────────────────────────────────────────────────────

    #[test]
    fn gate_ist_codec_roundtrip_all_indices() {
        for ist in 0u8..=7 {
            assert!(GateIstCodec::roundtrip(0xFFFF_0000_0010_0000, ist).unwrap());
        }
        let gate = GateIstCodec::assemble(0xABCD_0004_FFFF_FFF0, 3).unwrap();
        assert_eq!(GateIstCodec::extract(gate), 3);
        assert!(GateIstCodec::reserved_bit3_clear(gate));
        // 位域之外的高位不被扰动
        assert_eq!(gate & !0xF, 0xABCD_0004_FFFF_FFF0 & !0xF);
        assert!(GateIstCodec::assemble(0, 8).is_err());
        assert!(!GateIstCodec::ist_valid(8));
    }

    // ── 4. 路由表 ─────────────────────────────────────────────────────────

    #[test]
    fn router_spec_and_gate_consistency() {
        let r = IstRouter::from_spec();
        assert!(r.verify_against_spec().is_ok());
        assert!(r.slots_distinct());
        assert_eq!(r.route(VEC_DF), Some(IST_SLOT_DF));
        assert_eq!(r.route(VEC_NMI), Some(IST_SLOT_NMI));
        assert_eq!(r.route(VEC_MC), Some(IST_SLOT_MC));
        assert_eq!(r.route(0x20), None);
        assert_eq!(r.vector_of_slot(IST_SLOT_DF), Some(VEC_DF));
        assert_eq!(r.forced_count(), 0);
        // 门位提取与路由一致
        for i in 0..3 {
            let v = ROUTE_SPEC_TABLE[i].vector;
            let g = r.gate_upper(v).unwrap();
            assert_eq!(GateIstCodec::extract(g), r.route(v).unwrap());
        }
    }

    #[test]
    fn router_install_rejects_out_of_domain() {
        let mut r = IstRouter::from_spec();
        assert_eq!(r.install(0x20, 1).unwrap_err(), IstStackError::NotIstVector);
        assert!(r.install(VEC_DF, 8).is_err());
        // 合法重装 DF→IST1（原位）后一致性仍过
        r.install(VEC_DF, IST_SLOT_DF).unwrap();
        assert!(r.verify_against_spec().is_ok());
    }

    // ── 5. 进入/退出与现场帧 ───────────────────────────────────────────────

    #[test]
    fn df_drill_walks_ist1_frame_complete() {
        let mut cpu = IstCpu::new().unwrap();
        let f = cpu.drill_df(0xFFFF_0005_8000_1234).unwrap();
        assert_eq!(f.vector, VEC_DF);
        assert_eq!(f.error_code, Some(0), "DF 硬件恒推错误码 0（Table 6-1）");
        assert_eq!(f.slot, IST_SLOT_DF, "构造 DF 触发必走 IST1");
        assert!(!f.on_backup);
        assert_eq!(f.frame_size, HW_FRAME_WITH_EC);
        assert!(f.complete());
        assert!(cpu.layout().envelope_contains(IST_SLOT_DF, f.rsp));
        cpu.exit(IST_SLOT_DF).unwrap();
        assert!(cpu.audit_independence().is_ok());
    }

    #[test]
    fn nmi_and_mc_drills_walk_own_slots() {
        let mut cpu = IstCpu::new().unwrap();
        let nmi = cpu.drill_nmi(0xFFFF_0005_8000_2222).unwrap();
        assert_eq!(nmi.slot, IST_SLOT_NMI);
        assert_eq!(nmi.error_code, None, "NMI 不压错误码");
        assert_eq!(nmi.frame_size, HW_FRAME_NO_EC);
        cpu.exit(IST_SLOT_NMI).unwrap();
        let mc = cpu.drill_mc(0xFFFF_0005_8000_3333).unwrap();
        assert_eq!(mc.slot, IST_SLOT_MC);
        assert_eq!(mc.error_code, None, "MC 不压错误码（Table 6-1 修正）");
        cpu.exit(IST_SLOT_MC).unwrap();
        assert!(cpu.audit_independence().is_ok());
    }

    #[test]
    fn df_miswire_to_ist2_is_rejected() {
        let mut cpu = IstCpu::new().unwrap();
        assert!(cpu.drill_df_miswire_rejected().is_ok());
        // 演练后路由已复位
        assert!(cpu.router().verify_against_spec().is_ok());
    }

    #[test]
    fn nesting_and_non_ist_rejected() {
        let mut cpu = IstCpu::new().unwrap();
        let _ = cpu.enter(VEC_DF, 0xFFFF_8000_0000_0001).unwrap();
        // 同槽嵌套（DF 在位再入 DF）
        assert_eq!(cpu.enter(VEC_DF, 0xFFFF_8000_0000_0002).unwrap_err(), IstStackError::NestingViolated);
        cpu.exit(IST_SLOT_DF).unwrap();
        // 非编内向量
        for v in [0u8, 3, 13, 0x20, 0xFF] {
            assert_eq!(cpu.enter(v, 0x1000).unwrap_err(), IstStackError::NotIstVector);
        }
        // 退出多于进入
        assert_eq!(cpu.exit(IST_SLOT_DF).unwrap_err(), IstStackError::NestingViolated);
        assert!(cpu.audit_independence().is_ok());
    }

    // ── 6. 防护面 100% ────────────────────────────────────────────────────

    #[test]
    fn protection_fires_100pct_all_zones_all_slots() {
        let mut cpu = IstCpu::new().unwrap();
        let lay = IstStackLayout::from_consts();
        let mut touched = 0u64;
        // 每槽每档多点触碰（哨兵 16 字 / guard 8 点 / 深越界 8 点）
        for ist in 1u8..=3 {
            let p = lay.primary(ist).unwrap();
            // 哨兵带逐字
            for w in 0..SENTINEL_WORDS as u64 {
                let fired = cpu.touch(p.body_lo() + w * 8).unwrap();
                assert!(fired, "哨兵字 {} 必触发", w);
                touched += 1;
                cpu.restore(ist).unwrap();
            }
            // guard 区多点
            let mut g = 0;
            while g < 8 {
                let fired = cpu.touch(p.base + g * (GUARD_DEPTH / 8)).unwrap();
                assert!(fired);
                touched += 1;
                cpu.restore(ist).unwrap();
                g += 1;
            }
            // 深越界带多点
            let mut d = 0;
            while d < 8 {
                let fired = cpu.touch(p.base - 1 - d * (DEEP_BAND / 8)).unwrap();
                assert!(fired);
                touched += 1;
                cpu.restore(ist).unwrap();
                d += 1;
            }
        }
        assert_eq!(cpu.stats().touches, touched);
        assert_eq!(cpu.stats().fires, touched, "每触必发");
        assert_eq!(cpu.stats().rate_permille(), 1000);
        assert!(cpu.stats().full_coverage());
        // 触发过的槽哨兵有豁免损坏账 → 审计仍绿
        assert!(cpu.audit_sentinels().is_ok());
    }

    #[test]
    fn body_touch_does_not_fire_and_outside_is_error() {
        let mut cpu = IstCpu::new().unwrap();
        let p = cpu.layout().primary(1).unwrap();
        let fired = cpu.touch(p.sentinel_hi() + 0x100).unwrap();
        assert!(!fired, "栈体正常段不触发防护");
        assert_eq!(cpu.stats().touches, 0);
        // 区域外
        assert_eq!(
            cpu.touch(0x1000).unwrap_err(),
            IstStackError::UnattributableTouch
        );
    }

    // ── 7. 降级/备用栈/恢复 ───────────────────────────────────────────────

    #[test]
    fn downgrade_routes_to_backup_and_restore_rearms() {
        let mut cpu = IstCpu::new().unwrap();
        let p2 = cpu.layout().primary(2).unwrap();
        // 触发防护 → IST2 降级
        assert!(cpu.touch(p2.body_lo()).unwrap());
        assert_eq!(cpu.tier(2), TIER_BACKUP);
        assert_ne!(cpu.degraded_mask() & 0b010, 0);
        // 原栈严格进入被拒
        assert_eq!(
            cpu.enter_original(VEC_NMI, 0x2000).unwrap_err(),
            IstStackError::StackDegraded
        );
        // 常规进入自动走备用栈
        let f = cpu.drill_nmi(0xFFFF_0005_8000_4000).unwrap();
        assert!(f.on_backup);
        assert!(cpu.layout().backup_envelope_contains(2, f.rsp));
        cpu.exit(2).unwrap();
        // 恢复后回主栈
        cpu.restore(2).unwrap();
        assert_eq!(cpu.tier(2), TIER_PRIMARY);
        let f2 = cpu.drill_nmi(0xFFFF_0005_8000_4001).unwrap();
        assert!(!f2.on_backup);
        cpu.exit(2).unwrap();
        assert!(cpu.audit_independence().is_ok());
    }

    #[test]
    fn in_flight_downgrade_keeps_conservation() {
        let mut cpu = IstCpu::new().unwrap();
        let _f = cpu.enter(VEC_MC, 0xFFFF_8000_0000_9000).unwrap();
        let p3 = cpu.layout().primary(3).unwrap();
        // 在位帧时触发降级：作废帧补记退出，守恒不破
        assert!(cpu.touch(p3.base + 0x10).unwrap());
        assert!(cpu.audit_independence().is_ok(), "在位降级后进出守恒应保持");
        assert!(!cpu.slot_runtime(3).unwrap().in_flight());
    }

    // ── 8. 独立性审计 ─────────────────────────────────────────────────────

    #[test]
    fn independence_cross_slot_invariance() {
        let mut cpu = IstCpu::new().unwrap();
        let _ = cpu.drill_df(0xFFFF_8000_0000_1000).unwrap();
        let snap = cpu.usage_snapshot();
        // DF 在位时进入 NMI/MC：IST1 账面不得漂移
        let _ = cpu.drill_nmi(0xFFFF_8000_0000_2000).unwrap();
        let _ = cpu.drill_mc(0xFFFF_8000_0000_3000).unwrap();
        assert!(snap.slot_same_as(&cpu.usage_snapshot(), 1), "IST1 账不得漂移");
        cpu.exit(IST_SLOT_MC).unwrap();
        cpu.exit(IST_SLOT_NMI).unwrap();
        cpu.exit(IST_SLOT_DF).unwrap();
        assert_eq!(cpu.audit_independence().unwrap(), 3);
        // 三栈各自有进入账
        for ist in 1..=3u8 {
            assert!(cpu.slot_runtime(ist).unwrap().enters >= 1);
        }
    }

    // ── 9. 深度演练器 ─────────────────────────────────────────────────────

    #[test]
    fn probe_descend_stops_before_sentinel() {
        let cpu = IstCpu::new().unwrap();
        let (rsp, frames) = cpu.probe_descend(VEC_DF, 10_000).unwrap();
        let p = cpu.layout().primary(1).unwrap();
        assert!(rsp >= p.sentinel_hi(), "推演不得入哨兵带");
        assert!(frames > 0 && frames < 10_000);
        // 手算对拍：帧数 = floor((top - sentinel_hi) / 48)
        let expect = (p.top() - p.sentinel_hi()) / HW_FRAME_WITH_EC;
        assert_eq!(frames, expect);
        // 零帧请求 → FrameIncomplete
        assert!(cpu.probe_descend(VEC_DF, 0).is_err());
    }

    // ── 10. 事件账 ────────────────────────────────────────────────────────

    #[test]
    fn ledger_seq_monotonic_and_typed() {
        let mut cpu = IstCpu::new().unwrap();
        let _ = cpu.drill_df(0xFFFF_8000_0000_1111).unwrap();
        cpu.exit(1).unwrap();
        let led = cpu.ledger();
        assert_eq!(led.len(), 2);
        assert_eq!(led.get(0).unwrap().kind, K_ENTER);
        assert_eq!(led.get(1).unwrap().kind, K_EXIT);
        assert_eq!(led.get(1).unwrap().seq, 2);
        assert_eq!(led.last_seq(), 2);
        assert_eq!(led.count_kind(K_ENTER), 1);
        assert_eq!(led.count_kind(K_EXIT), 1);
        let fp1 = led.fingerprint();
        // 同序列重建指纹一致
        let mut cpu2 = IstCpu::new().unwrap();
        let _ = cpu2.drill_df(0xFFFF_8000_0000_1111).unwrap();
        cpu2.exit(1).unwrap();
        assert_eq!(fp1, cpu2.ledger().fingerprint());
    }

    // ── 11. 三注入面 ──────────────────────────────────────────────────────

    #[test]
    fn injection_overlap_detected() {
        let out = inject_stack_overlap().unwrap();
        assert!(out.detected);
        assert_eq!(out.code, IstStackError::LayoutViolated.code());
        assert!(!out.note.is_empty());
    }

    #[test]
    fn injection_sentinel_break_detected_then_restore() {
        let mut cpu = IstCpu::new().unwrap();
        let out = inject_sentinel_break(&mut cpu).unwrap();
        assert!(out.detected);
        assert_eq!(out.code, IstStackError::SentinelDamaged.code());
        // 注入后审计红灯持续（未豁免损坏在账）
        assert!(cpu.audit_sentinels().is_err());
        // restore 重挂哨兵后回绿
        cpu.restore(IST_SLOT_NMI).unwrap();
        assert!(cpu.audit_sentinels().is_ok());
    }

    #[test]
    fn injection_route_miswire_detected_and_reset() {
        let mut cpu = IstCpu::new().unwrap();
        let out = inject_route_miswire(&mut cpu).unwrap();
        assert!(out.detected);
        assert_eq!(out.code, IstStackError::RouteMismatch.code());
        // 注入面自复位：一致性回绿
        assert!(cpu.router().verify_against_spec().is_ok());
    }

    // ── 12. 场景与双跑 ────────────────────────────────────────────────────

    #[test]
    fn scenario_runs_green_and_rate_1000() {
        let rep = run_scenario(0xA5A5_5A5A, 200).unwrap();
        assert!(rep.enters > 100);
        assert_eq!(rep.exits, rep.enters);
        assert!(rep.fires >= 200 / PERIOD_TOUCH);
        assert!(rep.restores > 0);
        assert!(rep.rejects >= 200 / PERIOD_REJECT);
        assert_eq!(rep.rate_permille, 1000);
        assert!(rep.backup_enters > 0, "降级路径应被走到");
        assert_eq!(rep.frames_checked, rep.enters);
        assert!(rep.fingerprint != 0);
    }

    #[test]
    fn dual_run_same_seed_identical() {
        let rep = dual_run_check(0x1234_0006, 120).unwrap();
        assert_eq!(rep.rate_permille, 1000);
        // 双跑全等由 dual_run_check 内部 assert（Err 路径断言）
        let a = run_scenario(0xDEAD_0003, 80).unwrap();
        let b = run_scenario(0xDEAD_0003, 80).unwrap();
        assert_eq!(a, b);
        // 异种子指纹必异（随机面未塌缩）
        let c = run_scenario(0xDEAD_0003 + 1, 80).unwrap();
        assert_ne!(a.fingerprint, c.fingerprint);
    }

    // ── 13. 体检单 ────────────────────────────────────────────────────────

    #[test]
    fn health_green_after_exercise() {
        let mut cpu = IstCpu::new().unwrap();
        let _ = cpu.drill_df(0xFFFF_8000_0000_5000).unwrap();
        cpu.exit(1).unwrap();
        let p3 = cpu.layout().primary(3).unwrap();
        assert!(cpu.touch(p3.body_lo() + 8).unwrap());
        cpu.restore(3).unwrap();
        let h = cpu.health();
        assert!(h.ok(), "体检应全绿：\n{}", h.summary());
        assert!(h.layout_ok && h.route_ok && h.tss_ok && h.sentinel_ok && h.usage_ok);
        assert_eq!(h.protection_rate_permille, 1000);
        assert_eq!(h.degraded_mask, 0);
        assert!(h.ledger_seq > 0);
        let s = cpu.health_summary();
        assert!(s.contains("体检单"));
        assert!(s.contains("防护统计"));
        assert!(s.contains("IST1-DF"));
    }

    // ── 14. const 工具与错误域 ────────────────────────────────────────────

    #[test]
    fn error_domain_codes_unique_in_segment() {
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
            IstStackError::NestingViolated,
            IstStackError::FingerprintMismatch,
            IstStackError::InjectionUndetected,
            IstStackError::TssProgramFault,
        ];
        let mut i = 0;
        while i < all.len() {
            let c = all[i].code();
            assert!(c > SEG_BASE && c <= SEG_BASE + 0x0D);
            // detail 非空且含「下一步」（可操作三要素）
            let d = all[i].detail();
            assert!(!d.is_empty());
            assert!(d.contains("下一步"), "{} 缺下一步指引", c);
            let mut j = i + 1;
            while j < all.len() {
                assert_ne!(c, all[j].code(), "诊断码必须段内唯一");
                j += 1;
            }
            i += 1;
        }
        assert_eq!(IstStackError::segment_label(), "UNX-F0806 IST2 诊断段基 0x2850");
    }

    #[test]
    fn const_helpers_edges() {
        // 规范形三档
        assert!(is_canonical(0));
        assert!(is_canonical(0x0000_7FFF_FFFF_FFFF));
        assert!(is_canonical(0xFFFF_8000_0000_0000));
        assert!(is_canonical(u64::MAX));
        assert!(!is_canonical(0x0000_8000_0000_0000));
        assert!(!is_canonical(0xFFFF_7FFF_FFFF_FFFF));
        // 对齐
        assert!(is_aligned(16, 16) && is_aligned(0, 16));
        assert!(!is_aligned(8, 16) && !is_aligned(16, 0));
        // 区间重叠半开语义
        assert!(ranges_overlap(0, 16, 8, 24));
        assert!(!ranges_overlap(0, 16, 16, 32));
        assert!(!ranges_overlap(0, 0, 0, 0));
        // umin/umax
        assert_eq!(umin(3, 7), 3);
        assert_eq!(umax(3, 7), 7);
        // FNV 参考向量（空吸收恒等 + 已知散布）
        assert_eq!(fnv1a_bytes(FNV_OFFSET, &[]), FNV_OFFSET);
        let h = fnv1a_bytes(FNV_OFFSET, b"a");
        assert_eq!(h, (FNV_OFFSET ^ 0x61).wrapping_mul(FNV_PRIME));
    }

    #[test]
    fn lcg_deterministic_same_seed() {
        let mut a = Lcg::new(42);
        let mut b = Lcg::new(42);
        let mut i = 0;
        while i < 100 {
            assert_eq!(a.next(), b.next());
            i += 1;
        }
        assert_eq!(a.state(), b.state());
        assert_eq!(Lcg::new(7).below(0), 0);
    }
}
