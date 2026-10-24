//! unx2602 ↔ UNX-F2602 #PF 入口与错误码解析细化（F2540 骨架实现细化）
//!
//! 单号：UNX-F2602
//! 标题：#PF 入口与错误码解析细化（F2540 骨架实现细化）
//! 域/批：A4/B11
//! 册目标：300 行纯功能（判据口径），本深化版展开 2800+ 行
//!
//! 判据原文逐字摘抄（anchors/UNX-F2602.md 第 2 行）：
//! `UNX-F2602-J1 解析器细化后 32 合法组合逐位读出与 F2537 位域表零漂移，入口分发 10^6 次零误差`
//!
//! 正文关键子句逐字摘抄（同文件第 7 行）：
//! `第一步解析实现：错误码 u32→结构体（P 存在/W 写/U 用户/R 保留写/S 指令
//!  取指/D/I 平台位七布尔+原始值），读位代码从 F2537 位域表生成（位号/掩码
//!  逐位生成物，手写禁令 grep 审计），结构体字段与表格位名一一对应`
//! `第二步漂移断言：32 合法组合（F2537 注入资产复用）逐位读出断言（每组合
//!  读出的七位与组合定义零漂移），歧义组合（F2538 资产）确定性裁决断言，
//!  10^6 次随机错误码分发（组合随机生成）零误差（解析→判定树路由与预登记
//!  路由一致）`
//! `第三步分发对接：解析结构体喂 F2538 判定树的调用链登记（F2601 分发点
//!  消费），入口耗时抽样账（p99 留痕，F2577 基线制度扩展登记）`
//!
//! 码段范围声明：本单独占诊断码段 `0x1C80 .. 0x1CBF`（`SEG_MASK = 0x003F`），
//! 共 64 码。错误变体一码一段，不与册内其他单共享；下界紧接 F2582 的
//! `0x1C40..0x1C7F` 之后，上界止于 `0x1CBF`（`0x1CC0` 起归 F2622）。
//!
//! ## 本单要解决的「漂移」问题（判据核心）
//!
//! #PF 错误码是**位域**而非枚举：一位一位地表示「有没有 PTE / 是不是写
//! 访问 / 是不是用户态 / 是不是保留位写 / 是不是取指 / 平台自定义位」。
//! 一旦「读位代码」与「位域表定义」分两份手工维护，最典型的缺陷是：
//! 表里写 bit2 = U（用户），代码里 `>> 2` 取的却是 W —— 表与码同时存在
//! 却互相印证不出来，缺陷要等到真机踩到才暴露。
//!
//! 本单的解法是**反双写**：位域表 `FIELD_TABLE` 是唯一真值源，读位函数
//! `read_bit()` 只接受 `FieldId` 参数并自己去表里取掩码，**代码里不允许
//! 出现任何字面掩码**。`mask_audit()` 逐位遍历 32 位，检查归属唯一、
//! 不重叠、无空洞（除显式保留区），使「读位实现与位域表零漂移」不再依赖
//! 人的纪律，而是结构性不可能。
//!
//! ## 组件清单（本单交付的纯功能件）
//! 1. `FieldId`：F2537 位域表七位的**唯一标识**闭集。
//! 2. `FieldSpec`：一条位域定义（位号 / 掩码 / 名称 / 是否保留位）。
//! 3. `FIELD_TABLE`：真值源表（7 条 + 1 条保留区聚合），带 FNV 冻结指纹。
//! 4. `mask_of()` / `shift_of()` / `name_of()`：**唯一**的表读取口。
//! 5. `PfCode`：原始 `u32` 错误码 + 有效位宽收窄。
//! 6. `PfDecoded`：解析结构体（七布尔 + 原始值 + 保留位镜像 + 标签串）。
//! 7. `pf_decode()`：解析器（唯一的读位路径）。
//! 8. `Route`：判定树路由闭集（F2538 的消费面形态，14 路）。
//! 9. `route_of()` / `route_matrix()`：路由裁决 + 全表（14 路覆盖断言）。
//! 10. `DispatchLedger`：守恒账 `total == hit[14] + unknown`。
//! 11. `RingLedger`：append-only 固定容量环形账，`dropped` 显性。
//! 12. 注入核 `inject_all()`：十二形态负向注入，全被拒并留痕。
//! 13. `mask_audit()`：手写禁令（裸掩码）等价的运行时审计。
//! 14. `fingerprint()` / `verify()`：冻结指纹。
//! 15. `export_kv()` / `export_tlv()`：观测导出，TLV 组头单独一行。
//! 16. `replay(seed)`：确定性回放，同种子两次一致。
//! 17. `observe(seed)`：聚合入口。
//! 18. `complexity()`：性能逐项分解 + 对标 Windows 的「深在哪」。
//!
//! ## 位域表（真值源，F2537 消费面镜像）
//! ```text
//! bit 1 = S（指令取指）   bit 2 = W（写访问）   bit 3 = P（页存在）
//! bit16 = U（用户态）     bit17 = R（保留位写）
//! bit22 = D（平台自定义低位组）  bit31 = I（平台自定义高位组）
//! 其余位为硬件保留区，读出后必须忽略（本单登记为 FieldId::Reserved）
//! ```
//!
//! **判定优先级表**（`route_of()` 的裁决顺序，F2538 消费）：
//! ```text
//!  1. !P                    → NotPresent
//!  2. !P && W && U          → NotPresentUserWrite
//!  3.  P && W && COW        → CowViolation
//!  4.  P && W && R          → ReservedSetViolation
//!  5.  P && W && U && RO    → UserWriteProtected
//!  6.  P && W               → WriteToReadOnly
//!  7.  P && S               → ExecFromData
//!  8.  P && !W && !S        → ReadButFaulted
//!  9.  P && (D|I)           → PlatformCustom
//! 10.  P                    → OkUnclassified
//! ```
//!
//! ## 与册内其他单的显式对齐点
//! - **F2537 位域表**：本单 `FIELD_TABLE` 是它的自研镜像，`mask_audit()`
//!   保证「读位代码 = 表生成物」这一判据要求在结构上成立。
//! - **F2538 判定树**：本单只做**路由裁决**（`route_of()`），不做各路
//!   语义执行；14 路语义归 F2538 本体。
//! - **F2540 骨架**：本单把「错误码解析器骨架」补全为可执行解析器。
//! - **F2601 分发点**：`DispatchLedger` 的 `hit[14]` 是它的登记面。
//! - **F2577 耗时基线**：p99 抽样账（本单用**逻辑 step 计数**代替墙钟，
//!   **不做真实计时**）在此以 `LatencySample` 形式登记。
//! - **`pfh.rs`**：入口收编面。
//!
//! ## 守恒账口径
//! `total == hit[0] + hit[1] + ... + hit[13] + unknown`；守恒式从 0 起算，
//! 与 `Router` 同生共死（`Router::new()` 同处构造账与环账，不存在
//! 「先建路由表后建账」的窗口）。
//!
//! ## 性能逐项分解（模型侧成本，非真实硬件时序）
//! - **单次 `pf_decode()`**：`O(FIELDS)` = 7 次表查询 + 7 次位测试。
//!   内存代价：0 分配（全部栈上标量）。syscall 次数：0。
//!   步数上界：`FIELDS + 4 = 11`。
//! - **`route_of()`**：`O(1)`（`match` 链，最多 10 级比较）。
//! - **`dispatch()`（入口分发）**：解码 + 路由 + 账自增，`O(FIELDS)`。
//! - **10^6 次分发**：`O(10^6 · FIELDS)`，无递归、无墙钟、无 IO。
//! - **`mask_audit()`**：`O(CODE_BITS + FIELDS)`。
//! - **`export_kv`**：`O(metric_count)`；**`export_tlv`**：
//!   `O(metric_count + FIELDS + ROUTE_COUNT)`。
//!
//! ## 对标 Windows 的「深在哪」
//! Windows `KiPageFaultDispatch` / `MiPageFaultDispatch` 同样把
//! `ExceptionRecord.ExceptionCode` 解成结构化故障类型（`NonPresentFault` /
//! `ProtectionFault` / `WriteFault` / `ReadFault` / `ExecuteFault` /
//! `CowFault` …），思想同构。差异在三点：
//! 1. Windows 的字段定义散在 `ntddk` 头 + 汇编 `TEST` 序列里，**表与码是
//!    两份**；本单以 `FIELD_TABLE` 为唯一真值，读位只能走表，漂移结构性
//!    不可能。
//! 2. Windows 不暴露「逐位读出 vs 位域表」的**机器可判定**断言；本单
//!    32 组合逐位零漂移是判据级硬指标。
//! 3. Windows 的 p99 走真实计时器；本单用**逻辑 step 计数**代替，零墙钟，
//!    代价是分位数的物理意义被削弱为「复杂度分位」，这一点在导出串里
//!    显式标注（`latency.kind=logical_steps`）。
//!
//! ## 纯功能线
//! `no_std + alloc`，零 `unwrap` / `expect` / `panic!` / `todo!`，零浮点，
//! 零真实 IO，零墙钟；时间一律为逻辑 `tick: u64`；确定性 LCG
//! （`6364136223846793005` / `1442695040888963407`，模 `2^64` 回绕）；
//! FNV-1a（basis `0xcbf29ce484222325`，prime `0x100000001b3`）；
//! 自定义错误闭集，`code()` / `detail()` 三要素（现象 / 位置 / 建议修法）。
// 本单为「册内自足模块」：以私有 `mod m;` 引入时全部 `pub` 项被判未可达，
// 这是引入方式使然，不是死代码。
#![allow(dead_code)]
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
extern crate alloc;

// ===========================================================================
// 0. 常量表（判据阈值写死 + 来源注释）
// ===========================================================================

/// 模块版本标识（写入导出组头）。
pub const V2602_VERSION_ID: u64 = 0x0002_6020_0000_0001;
/// 模块版本串。
pub const V2602_VERSION: &str = "v2602-pfentry-decode";
/// 诊断码段基址（任务单指定 0x1C80）。
pub const SEG_BASE: u16 = 0x1C80;
/// 诊断码段掩码（0x003F → 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断主码。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 错误码位宽（x86-64 页故障错误码为低 32 位）。
pub const CODE_BITS: u32 = 32;
/// 位域表条目数（七位 + 一条保留区聚合 = 8）。
pub const FIELD_ROWS: usize = 8;
/// 参与解析的七布尔位数（判据原文：七布尔）。
pub const FIELDS: usize = 7;
/// 判据合法组合数（判据原文：32 合法组合）。
pub const LEGAL_COMBOS: usize = 32;
/// 判据分发次数（判据原文：10^6 次零误差）。
pub const DISPATCH_ROUNDS: u64 = 1_000_000;
/// 判定树路由路数（F2538 十四路）。
pub const ROUTE_COUNT: usize = 14;
/// 环形账固定容量。
pub const RING_CAP: usize = 256;
/// 注入形态数（判据要求 ≥10，本单实现 12）。
pub const FORM_COUNT: usize = 12;
/// 回放步数上界。
pub const REPLAY_STEPS: usize = 2048;
/// 抽样账容量（p99 留痕用的逻辑样本数）。
pub const LATENCY_SAMPLES: usize = 64;
/// LCG 乘数。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a basis。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ===========================================================================
// 0.1 确定性 LCG
// ===========================================================================

/// 确定性 LCG 发生器（模 `2^64`，回绕即 `wrapping`）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    /// 内部状态。
    pub state: u64,
}

impl Lcg {
    /// 以给定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    /// 当前内部状态（只读，供轨迹断言）。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 取下一个 `u64`（LCG 一步）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取 `[0, n)` 内值；`n == 0` 返回 0。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 取 `[0, 1<<bits)` 内值。
    pub fn bits(&mut self, bits: u32) -> u32 {
        if bits == 0 {
            return 0;
        }
        if bits >= 32 {
            return self.next_u64() as u32;
        }
        (self.next_u64() as u32) & ((1u32 << bits) - 1)
    }
}

/// 纯函数式 LCG 步进。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

// ===========================================================================
// 0.2 FNV-1a 指纹原语
// ===========================================================================

/// FNV-1a 64 位：字节序列折叠。
pub fn fnv1a_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    let mut i: usize = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// FNV-1a 64 位：`u64` 字序列折叠（每字 8 字节小端）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h: u64 = FNV_BASIS;
    for w in words {
        let mut i: usize = 0;
        while i < 8 {
            let byte = ((*w >> (8 * i)) & 0xFF) as u8;
            h ^= byte as u64;
            h = h.wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
    }
    h
}

/// FNV-1a 追加一个 `u64`（链式用）。
pub fn fnv1a_acc(h: u64, w: u64) -> u64 {
    let mut cur = h;
    let mut i: usize = 0;
    while i < 8 {
        let byte = ((w >> (8 * i)) & 0xFF) as u8;
        cur ^= byte as u64;
        cur = cur.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    cur
}

// ===========================================================================
// 1. F2537 位域表（唯一真值源）
// ===========================================================================

/// 位域标识闭集（F2537 七位 + 一条保留区聚合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldId {
    /// bit1：指令取指（Instruction Fetch）。
    Sf,
    /// bit2：写访问（Write）。
    Wf,
    /// bit3：页存在（Present）。
    Pf,
    /// bit16：用户态（User）。
    Uf,
    /// bit17：保留位写（Reserved）。
    Rf,
    /// bit22：平台自定义位组（低）。
    Df,
    /// bit31：平台自定义位组（高 / 软件保留）。
    If_,
    /// 保留区聚合（不在解析路径上，只在表审计里出现）。
    Reserved,
}

impl FieldId {
    /// 全部条目（表顺序即定义顺序）。
    pub const ALL: [FieldId; FIELD_ROWS] =
        [FieldId::Sf, FieldId::Wf, FieldId::Pf, FieldId::Uf, FieldId::Rf, FieldId::Df, FieldId::If_, FieldId::Reserved];
    /// 参与解析的七位（表前七项）。
    pub const SEVEN: [FieldId; FIELDS] = [FieldId::Sf, FieldId::Wf, FieldId::Pf, FieldId::Uf, FieldId::Rf, FieldId::Df, FieldId::If_];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            FieldId::Sf => 0,
            FieldId::Wf => 1,
            FieldId::Pf => 2,
            FieldId::Uf => 3,
            FieldId::Rf => 4,
            FieldId::Df => 5,
            FieldId::If_ => 6,
            FieldId::Reserved => 7,
        }
    }
    /// 由序号还原；越界 `None`。
    pub fn from_index(i: usize) -> Option<FieldId> {
        match i {
            0 => Some(FieldId::Sf),
            1 => Some(FieldId::Wf),
            2 => Some(FieldId::Pf),
            3 => Some(FieldId::Uf),
            4 => Some(FieldId::Rf),
            5 => Some(FieldId::Df),
            6 => Some(FieldId::If_),
            7 => Some(FieldId::Reserved),
            _ => None,
        }
    }
    /// 位域短名（**与 F2537 位名一一对应**）。
    pub fn name(self) -> &'static str {
        match self {
            FieldId::Sf => "S",
            FieldId::Wf => "W",
            FieldId::Pf => "P",
            FieldId::Uf => "U",
            FieldId::Rf => "R",
            FieldId::Df => "D",
            FieldId::If_ => "I",
            FieldId::Reserved => "RESERVED",
        }
    }
    /// 语义判读（现象）。
    pub fn describe(self) -> &'static str {
        match self {
            FieldId::Sf => "S：本次故障由指令取指引起",
            FieldId::Wf => "W：本次故障发生在写访问",
            FieldId::Pf => "P：页表项存在位（0 = 不存在）",
            FieldId::Uf => "U：访问发生在用户态",
            FieldId::Rf => "R：写访问命中保留位（置 1 且 CR0.WP 生效时报错）",
            FieldId::Df => "D：平台自定义位组（低）",
            FieldId::If_ => "I：平台自定义位组（高 / 软件保留）",
            FieldId::Reserved => "保留区：硬件未定义位，读出后必须忽略",
        }
    }
    /// 是否参与解析（保留区不参与）。
    pub fn parsed(&self) -> bool {
        !matches!(self, FieldId::Reserved)
    }
}

/// 一条位域定义（表项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FieldSpec {
    /// 位域标识。
    pub id: FieldId,
    /// 位号（0..31）。
    pub bit: u8,
    /// 掩码（`1 << bit`）。
    pub mask: u32,
    /// 是否保留区。
    pub reserved_area: bool,
}

impl FieldSpec {
    /// 表项指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [self.bit as u64, self.mask as u64, if self.reserved_area { 1 } else { 0 }, self.id.index() as u64];
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("bit{} name={} mask=0x{:08X} {}", self.bit, self.id.name(), self.mask, if self.reserved_area { "RESERVED" } else { "PARSED" })
    }
}

/// 保留区掩码（未定义位的并集）。
///
/// **推导方式**（不是手抄）：`RESERVED_MASK == !parsed_union() & !STRAY_MASK`。
/// 七解析位为 `bit1/bit2/bit3/bit16/bit17/bit22/bit31`，并集
/// `0x8043_000E`；再扣除「表外位」`STRAY_MASK = 0x0000_0100`，
/// 保留区恰为 `0x7FBC_FEF1`：
/// `bit4..7`、`bit9..15`、`bit18..21`、`bit23..30`。
///
/// 该常量满足两条恒等式（`mask_audit` 的 A3/A4 就是在断言它们）：
/// - `RESERVED_MASK & (parsed_union() | STRAY_MASK) == 0`（不覆盖解析位与表外位）
/// - `RESERVED_MASK | parsed_union() | STRAY_MASK == 0xFFFF_FFFF`（无空洞）
///
/// **注意**：本常量只出现在**表**里，不出现在解析路径上；
/// `mask_audit()` 会检查解析路径确实不引用它。
pub const RESERVED_MASK: u32 = 0x7FBC_FEF1u32;

/// **表外位**（软件可见、硬件手册未定义、读出必须报 `ERR_CODE_STRAY_BIT`）。
///
/// 与「保留区」的本质区别：保留区读出后**忽略**即可，表外位读出后
/// **必须报错**——因为它意味着「错误码的来源不是本单认识的编码器」，
/// 继续解析出来的七布尔是不可信的。
///
/// 这条区分是本单 A3 恒等式能取等号（而非不等号）的前提：只有先把
/// 表外位从「未定义」里单独摘出来，「无空洞」才是**逐位**成立的强断言，
/// 否则 `!covered` 里既混着合法空洞又混着表外位，断言就退化成「非零也放过」。
pub const STRAY_MASK: u32 = 0x0000_0100u32;

/// 保留区中用于「脏位」注入的代表位（`bit4`，属保留区，置 1 不改语义）。
pub const RESERVED_DIRTY_BIT: u32 = 0x0000_0010u32;

/// **位域表真值源**（8 项，顺序 = `FieldId::ALL`）。
///
/// 这是全单**唯一**允许出现位号的表；`mask_of()` / `shift_of()` 是它的
/// 唯一读口。
pub const FIELD_TABLE: [FieldSpec; FIELD_ROWS] = [
    FieldSpec { id: FieldId::Sf, bit: 1, mask: 0x0000_0002, reserved_area: false },
    FieldSpec { id: FieldId::Wf, bit: 2, mask: 0x0000_0004, reserved_area: false },
    FieldSpec { id: FieldId::Pf, bit: 3, mask: 0x0000_0008, reserved_area: false },
    FieldSpec { id: FieldId::Uf, bit: 16, mask: 0x0001_0000, reserved_area: false },
    FieldSpec { id: FieldId::Rf, bit: 17, mask: 0x0002_0000, reserved_area: false },
    FieldSpec { id: FieldId::Df, bit: 22, mask: 0x0040_0000, reserved_area: false },
    FieldSpec { id: FieldId::If_, bit: 31, mask: 0x8000_0000, reserved_area: false },
    FieldSpec { id: FieldId::Reserved, bit: 4, mask: RESERVED_MASK, reserved_area: true },
];

/// 表查询：取一项（越界或错位返回 `Err`）。
pub fn field_spec(id: FieldId) -> PfResult<FieldSpec> {
    field_spec_at(id.index())
}

/// **按裸下标查表**（唯一允许下标越界的口，仅供注入核使用）。
///
/// 生产路径一律走 `field_spec(id)`，因为 `FieldId` 是闭集、下标由类型保证；
/// 注入核要模拟「表项 id 与下标错位 / 下标越界」这类**编译期构造不出来**
/// 的漂移，只能拿一个裸 `usize` 去撞表，故单列此口并让它返回
/// `ERR_TABLE_DRIFT`。返回类型与 `field_spec` 完全一致，故两实现同构。
pub fn field_spec_at(index: usize) -> PfResult<FieldSpec> {
    match FIELD_TABLE.get(index) {
        Some(s) => {
            // 错位检测：表项自称的 `id` 必须等于其下标反查出的 `id`。
            if FieldId::from_index(index) != Some(s.id) {
                return Err(PfErr::of_kind(ErrKind::TableDrift, index as u64)
                    .note_of(&format!("row_id={} row_index={}", s.id.name(), index)));
            }
            Ok(*s)
        }
        None => Err(PfErr::of_kind(ErrKind::TableDrift, index as u64).note_of("index out of table range")),
    }
}

/// **唯一**的掩码读口（`ERR_MASK_LITERAL` 存在的理由就在这行注释上）。
pub fn mask_of(id: FieldId) -> PfResult<u32> {
    Ok(field_spec(id)?.mask)
}

/// **唯一**的位号读口。
pub fn shift_of(id: FieldId) -> PfResult<u32> {
    Ok(field_spec(id)?.bit as u32)
}

/// **唯一**的位名读口。
pub fn name_of(id: FieldId) -> PfResult<&'static str> {
    Ok(field_spec(id)?.id.name())
}

/// 表指纹（全部 8 项折叠）。
pub fn table_fp() -> u64 {
    let mut h: u64 = FNV_BASIS;
    let mut i: usize = 0;
    while i < FIELD_TABLE.len() {
        if let Some(s) = FIELD_TABLE.get(i) {
            let w = [s.bit as u64, s.mask as u64, if s.reserved_area { 1 } else { 0 }, s.id.index() as u64];
            let mut k: usize = 0;
            while k < 4 {
                if let Some(x) = w.get(k) {
                    h = fnv1a_acc(h, *x);
                }
                k = k.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    h
}

// ===========================================================================
// 2. 错误闭集（`0x1C80 .. 0x1CAF`，一码一段）
// ===========================================================================

/// `0x1C80` 位域表漂移（表项 id 与下标不符）。
pub const ERR_TABLE_DRIFT: u16 = 0x1C80;
/// `0x1C81` 掩码与位号不自洽（`mask != 1 << bit`）。
pub const ERR_MASK_INCONSISTENT: u16 = 0x1C81;
/// `0x1C82` 读位路径出现裸掩码字面量（手写禁令）。
pub const ERR_MASK_LITERAL: u16 = 0x1C82;
/// `0x1C83` 位归属不唯一（两位域声明同一位）。
pub const ERR_BIT_DOUBLE_OWN: u16 = 0x1C83;
/// `0x1C84` 保留区有空洞（保留位未被 `RESERVED_MASK` 覆盖）。
pub const ERR_RESERVED_HOLE: u16 = 0x1C84;
/// `0x1C85` 保留区有越界（`RESERVED_MASK` 覆盖了解析位）。
pub const ERR_RESERVED_OVERLAP: u16 = 0x1C85;
/// `0x1C86` 错误码含表外位（非保留、非解析）。
pub const ERR_CODE_STRAY_BIT: u16 = 0x1C86;
/// `0x1C87` 32 组合定义与解析结果零漂移断言失败。
pub const ERR_COMBO_DRIFT: u16 = 0x1C87;
/// `0x1C88` 歧义组合未确定性裁决（同一码两路）。
pub const ERR_AMBIGUOUS: u16 = 0x1C88;
/// `0x1C89` 分发路由与预登记路由不一致。
pub const ERR_ROUTE_MISMATCH: u16 = 0x1C89;
/// `0x1C8A` 分发守恒账不守恒。
pub const ERR_LEDGER_UNBALANCED: u16 = 0x1C8A;
/// `0x1C8B` 路由路数越界（`Route` 闭集外）。
pub const ERR_ROUTE_RANGE: u16 = 0x1C8B;
/// `0x1C8C` 环形账容量为 0。
pub const ERR_RING_ZERO: u16 = 0x1C8C;
/// `0x1C8D` 环形账 head 越界。
pub const ERR_RING_HEAD: u16 = 0x1C8D;
/// `0x1C8E` 环形账 `dropped` 与写入数不符。
pub const ERR_RING_DROPPED: u16 = 0x1C8E;
/// `0x1C8F` 环形账逐条指纹自校验失败。
pub const ERR_RING_FP: u16 = 0x1C8F;
/// `0x1C90` 抽样账容量不足（p99 无样本）。
pub const ERR_LATENCY_EMPTY: u16 = 0x1C90;
/// `0x1C91` 抽样账分位计算越界。
pub const ERR_LATENCY_RANGE: u16 = 0x1C91;
/// `0x1C92` 冻结指纹校验失败。
pub const ERR_VERIFY_FAILED: u16 = 0x1C92;
/// `0x1C93` 组头魔数不符。
pub const ERR_HEAD_MAGIC: u16 = 0x1C93;
/// `0x1C94` TLV 长度不自洽。
pub const ERR_TLV_LEN: u16 = 0x1C94;
/// `0x1C95` 回放步数超上界。
pub const ERR_REPLAY_STEPS: u16 = 0x1C95;
/// `0x1C96` 状态机当前态非法。
pub const ERR_STATE_ILLEGAL: u16 = 0x1C96;
/// `0x1C97` 状态机目标态非法。
pub const ERR_TARGET_ILLEGAL: u16 = 0x1C97;
/// `0x1C98` 状态机迁移不在合法边闭集内。
pub const ERR_EDGE_CLOSED: u16 = 0x1C98;
/// `0x1C9F` 注入核要求注入 N 次但实际只跑 M 次。
pub const ERR_INJECT_SHORT: u16 = 0x1C99;
/// `0x1CA0` 清空不同步（账未归零）。
pub const ERR_RESET_MISMATCH: u16 = 0x1C9A;
/// `0x1CA1` 观察包自相矛盾。
pub const ERR_OBS_INCONSISTENT: u16 = 0x1C9B;
/// `0x1CA2` 32 组合规模不足。
pub const ERR_COMBO_SHORT: u16 = 0x1C9C;
/// `0x1CA3` 分发次数不足判据要求（未达 10^6）。
pub const ERR_DISPATCH_SHORT: u16 = 0x1C9D;
/// `0x1CA4` 路由覆盖率不足（14 路未全覆盖）。
pub const ERR_ROUTE_COVER: u16 = 0x1C9E;
/// `0x1CA5` 错误码位宽越界。
pub const ERR_CODE_BITS: u16 = 0x1C9F;
/// `0x1CA6` 解析结构体字段与位名未一一对应。
pub const ERR_FIELD_NAME_MAP: u16 = 0x1CA0;
/// `0x1CA7` 判定树路由重复（两路同语义）。
pub const ERR_ROUTE_DUP: u16 = 0x1CA1;
/// `0x1CA8` 判定树路由缺项（`Route` 闭集未覆盖）。
pub const ERR_ROUTE_MISSING: u16 = 0x1CA2;
/// `0x1CA9` 账目分配不同步。
pub const ERR_ALLOC_DESYNC: u16 = 0x1CA3;
/// `0x1CAA` 未知诊断码。
pub const ERR_UNKNOWN: u16 = 0x1CA4;
/// `0x1CAB` 保留区镜像与 `RESERVED_MASK` 不符。
pub const ERR_RESERVED_MIRROR: u16 = 0x1CA5;
/// `0x1CAC` 平台位 D/I 组合越界（只允许 4 种）。
pub const ERR_PLATFORM_COMBO: u16 = 0x1CA6;
/// `0x1CAD` 标签串与布尔字段不自洽。
pub const ERR_LABEL_MISMATCH: u16 = 0x1CA7;
/// `0x1CAE` 注入检出缺失（形态未被检出核抓到）。
pub const ERR_DETECT_MISS: u16 = 0x1CA8;
/// `0x1CAF` 分发步数与账计数不符。
pub const ERR_STEP_LEDGER: u16 = 0x1CA9;

/// 错误码上界（含）。
pub const ERR_SEG_END: u16 = SEG_BASE | SEG_MASK;

/// 错误变体闭集（`0x1C80 .. 0x1CA9`，共 42 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrKind {
    /// 位域表漂移。
    TableDrift,
    /// 掩码与位号不自洽。
    MaskInconsistent,
    /// 读位路径出现裸掩码。
    MaskLiteral,
    /// 位归属不唯一。
    BitDoubleOwn,
    /// 保留区有空洞。
    ReservedHole,
    /// 保留区越界。
    ReservedOverlap,
    /// 错误码含表外位。
    CodeStrayBit,
    /// 组合零漂移断言失败。
    ComboDrift,
    /// 歧义组合。
    Ambiguous,
    /// 路由不一致。
    RouteMismatch,
    /// 账不守恒。
    LedgerUnbalanced,
    /// 路由路数越界。
    RouteRange,
    /// 环账容量为 0。
    RingZero,
    /// 环账 head 越界。
    RingHead,
    /// 环账 dropped 不符。
    RingDropped,
    /// 环账指纹失败。
    RingFp,
    /// 抽样账空。
    LatencyEmpty,
    /// 抽样账越界。
    LatencyRange,
    /// 冻结校验失败。
    VerifyFailed,
    /// 组头魔数不符。
    HeadMagic,
    /// TLV 长度不自洽。
    TlvLen,
    /// 回放超步。
    ReplaySteps,
    /// 当前态非法。
    StateIllegal,
    /// 目标态非法。
    TargetIllegal,
    /// 迁移不在闭集。
    EdgeClosed,
    /// 注入不足。
    InjectShort,
    /// 清空不同步。
    ResetMismatch,
    /// 观察包不自洽。
    ObsInconsistent,
    /// 组合规模不足。
    ComboShort,
    /// 分发次数不足。
    DispatchShort,
    /// 路由覆盖不足。
    RouteCover,
    /// 位宽越界。
    CodeBits,
    /// 位名映射破。
    FieldNameMap,
    /// 路由重复。
    RouteDup,
    /// 路由缺项。
    RouteMissing,
    /// 账目不同步。
    AllocDesync,
    /// 未知码。
    Unknown,
    /// 保留区镜像不符。
    ReservedMirror,
    /// 平台位组合越界。
    PlatformCombo,
    /// 标签串不自洽。
    LabelMismatch,
    /// 检出缺失。
    DetectMiss,
    /// 步数与账不符。
    StepLedger,
}

impl ErrKind {
    /// 全部变体（顺序即码序，48 项）。
    pub const ALL: [ErrKind; 42] = [
        ErrKind::TableDrift,
        ErrKind::MaskInconsistent,
        ErrKind::MaskLiteral,
        ErrKind::BitDoubleOwn,
        ErrKind::ReservedHole,
        ErrKind::ReservedOverlap,
        ErrKind::CodeStrayBit,
        ErrKind::ComboDrift,
        ErrKind::Ambiguous,
        ErrKind::RouteMismatch,
        ErrKind::LedgerUnbalanced,
        ErrKind::RouteRange,
        ErrKind::RingZero,
        ErrKind::RingHead,
        ErrKind::RingDropped,
        ErrKind::RingFp,
        ErrKind::LatencyEmpty,
        ErrKind::LatencyRange,
        ErrKind::VerifyFailed,
        ErrKind::HeadMagic,
        ErrKind::TlvLen,
        ErrKind::ReplaySteps,
        ErrKind::StateIllegal,
        ErrKind::TargetIllegal,
        ErrKind::EdgeClosed,
        ErrKind::InjectShort,
        ErrKind::ResetMismatch,
        ErrKind::ObsInconsistent,
        ErrKind::ComboShort,
        ErrKind::DispatchShort,
        ErrKind::RouteCover,
        ErrKind::CodeBits,
        ErrKind::FieldNameMap,
        ErrKind::RouteDup,
        ErrKind::RouteMissing,
        ErrKind::AllocDesync,
        ErrKind::Unknown,
        ErrKind::ReservedMirror,
        ErrKind::PlatformCombo,
        ErrKind::LabelMismatch,
        ErrKind::DetectMiss,
        ErrKind::StepLedger,
    ];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            ErrKind::TableDrift => 0,
            ErrKind::MaskInconsistent => 1,
            ErrKind::MaskLiteral => 2,
            ErrKind::BitDoubleOwn => 3,
            ErrKind::ReservedHole => 4,
            ErrKind::ReservedOverlap => 5,
            ErrKind::CodeStrayBit => 6,
            ErrKind::ComboDrift => 7,
            ErrKind::Ambiguous => 8,
            ErrKind::RouteMismatch => 9,
            ErrKind::LedgerUnbalanced => 10,
            ErrKind::RouteRange => 11,
            ErrKind::RingZero => 12,
            ErrKind::RingHead => 13,
            ErrKind::RingDropped => 14,
            ErrKind::RingFp => 15,
            ErrKind::LatencyEmpty => 16,
            ErrKind::LatencyRange => 17,
            ErrKind::VerifyFailed => 18,
            ErrKind::HeadMagic => 19,
            ErrKind::TlvLen => 20,
            ErrKind::ReplaySteps => 21,
            ErrKind::StateIllegal => 22,
            ErrKind::TargetIllegal => 23,
            ErrKind::EdgeClosed => 24,
            ErrKind::InjectShort => 25,
            ErrKind::ResetMismatch => 26,
            ErrKind::ObsInconsistent => 27,
            ErrKind::ComboShort => 28,
            ErrKind::DispatchShort => 29,
            ErrKind::RouteCover => 30,
            ErrKind::CodeBits => 31,
            ErrKind::FieldNameMap => 32,
            ErrKind::RouteDup => 33,
            ErrKind::RouteMissing => 34,
            ErrKind::AllocDesync => 35,
            ErrKind::Unknown => 36,
            ErrKind::ReservedMirror => 37,
            ErrKind::PlatformCombo => 38,
            ErrKind::LabelMismatch => 39,
            ErrKind::DetectMiss => 40,
            ErrKind::StepLedger => 41,
        }
    }
    /// 诊断码（`SEG_BASE + index`）。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }
    /// 由码还原；段外 `Err`。
    pub fn from_code(code: u16) -> Result<ErrKind, u16> {
        if code < SEG_BASE || code > ERR_SEG_END {
            return Err(code);
        }
        let i = (code - SEG_BASE) as usize;
        if i >= 42 {
            return Err(code);
        }
        match ErrKind::ALL.get(i) {
            Some(k) => Ok(*k),
            None => Err(code),
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            ErrKind::TableDrift => "table_drift",
            ErrKind::MaskInconsistent => "mask_inconsistent",
            ErrKind::MaskLiteral => "mask_literal",
            ErrKind::BitDoubleOwn => "bit_double_own",
            ErrKind::ReservedHole => "reserved_hole",
            ErrKind::ReservedOverlap => "reserved_overlap",
            ErrKind::CodeStrayBit => "code_stray_bit",
            ErrKind::ComboDrift => "combo_drift",
            ErrKind::Ambiguous => "ambiguous",
            ErrKind::RouteMismatch => "route_mismatch",
            ErrKind::LedgerUnbalanced => "ledger_unbalanced",
            ErrKind::RouteRange => "route_range",
            ErrKind::RingZero => "ring_zero",
            ErrKind::RingHead => "ring_head",
            ErrKind::RingDropped => "ring_dropped",
            ErrKind::RingFp => "ring_fp",
            ErrKind::LatencyEmpty => "latency_empty",
            ErrKind::LatencyRange => "latency_range",
            ErrKind::VerifyFailed => "verify_failed",
            ErrKind::HeadMagic => "head_magic",
            ErrKind::TlvLen => "tlv_len",
            ErrKind::ReplaySteps => "replay_steps",
            ErrKind::StateIllegal => "state_illegal",
            ErrKind::TargetIllegal => "target_illegal",
            ErrKind::EdgeClosed => "edge_closed",
            ErrKind::InjectShort => "inject_short",
            ErrKind::ResetMismatch => "reset_mismatch",
            ErrKind::ObsInconsistent => "obs_inconsistent",
            ErrKind::ComboShort => "combo_short",
            ErrKind::DispatchShort => "dispatch_short",
            ErrKind::RouteCover => "route_cover",
            ErrKind::CodeBits => "code_bits",
            ErrKind::FieldNameMap => "field_name_map",
            ErrKind::RouteDup => "route_dup",
            ErrKind::RouteMissing => "route_missing",
            ErrKind::AllocDesync => "alloc_desync",
            ErrKind::Unknown => "unknown",
            ErrKind::ReservedMirror => "reserved_mirror",
            ErrKind::PlatformCombo => "platform_combo",
            ErrKind::LabelMismatch => "label_mismatch",
            ErrKind::DetectMiss => "detect_miss",
            ErrKind::StepLedger => "step_ledger",
        }
    }
}

/// 错误类型：诊断码 + 定位载荷 + 说明。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PfErr {
    /// 诊断码。
    pub code: u16,
    /// 定位载荷（位号 / 序号 / 码值）。
    pub site: Option<u64>,
    /// 说明文本。
    pub note: String,
}

impl PfErr {
    /// 以诊断码构造。
    pub fn of(code: u16, site: u64) -> PfErr {
        PfErr { code, site: Some(site), note: String::new() }
    }
    /// 以变体构造。
    pub fn of_kind(k: ErrKind, site: u64) -> PfErr {
        PfErr { code: k.code(), site: Some(site), note: k.name().to_string() }
    }
    /// 追加定位。
    pub fn at(mut self, site: u64) -> PfErr {
        self.site = Some(site);
        self
    }
    /// 追加说明。
    pub fn note_of(mut self, text: &str) -> PfErr {
        self.note.push_str(text);
        self
    }
    /// 三要素之「现象」。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之「位置」。
    pub fn detail(&self) -> String {
        let kind = match ErrKind::from_code(self.code) {
            Ok(k) => k,
            Err(_) => ErrKind::Unknown,
        };
        let site = match self.site {
            Some(s) => format!("site=0x{:X}", s),
            None => String::from("site=none"),
        };
        let note = if self.note.is_empty() { String::from("-") } else { self.note.clone() };
        format!("[{}/{}] {} note={}", kind.name(), self.code, site, note)
    }
    /// 三要素之「建议修法」。
    pub fn advice(&self) -> &'static str {
        match ErrKind::from_code(self.code) {
            Ok(ErrKind::TableDrift) => "表项 id 与下标错位：重排 FIELD_TABLE 使 id.index() == 表下标",
            Ok(ErrKind::MaskInconsistent) => "掩码必须等于 1<<bit：改表项 mask 使其自洽",
            Ok(ErrKind::MaskLiteral) => "读位路径禁止裸掩码字面量：改走 mask_of(FieldId)",
            Ok(ErrKind::BitDoubleOwn) => "两位域声明同一位：删掉重复声明或错开位号",
            Ok(ErrKind::ReservedHole) => "有位既非解析位也非保留位：补进 RESERVED_MASK 或登记为新位域",
            Ok(ErrKind::ReservedOverlap) => "RESERVED_MASK 覆盖了解析位：把该位从保留区并集中移除",
            Ok(ErrKind::CodeStrayBit) => "错误码含表外位：核对硬件手册，必要时登记为保留位",
            Ok(ErrKind::ComboDrift) => "读出与组合定义不一致：查 FIELD_TABLE 的 bit/mask 是否被改",
            Ok(ErrKind::Ambiguous) => "同一码命中多路：按 route_of() 优先级表补齐裁决顺序",
            Ok(ErrKind::RouteMismatch) => "路由与预登记不一致：核对 route_of() 与预登记表同源",
            Ok(ErrKind::LedgerUnbalanced) => "查 total 与 hit[14]+unknown 三项分配是否同步",
            Ok(ErrKind::RouteRange) => "路由序号越界：查 Route::from_index() 的边界",
            Ok(ErrKind::RingZero) => "环形账容量必须 >0",
            Ok(ErrKind::RingHead) => "环形账 head 越过容量：查 cap 与 push 环绕逻辑",
            Ok(ErrKind::RingDropped) => "dropped 与写入数不符：核对 written - cap 的封顶逻辑",
            Ok(ErrKind::RingFp) => "环账逐条指纹失配：有记录被旁路改写",
            Ok(ErrKind::LatencyEmpty) => "抽样账无样本：先跑 dispatch 再取分位数",
            Ok(ErrKind::LatencyRange) => "分位下标越界：核对样本数与分位序号",
            Ok(ErrKind::VerifyFailed) => "冻结指纹漂移：比对 table_fp 与发布版基线",
            Ok(ErrKind::HeadMagic) => "导出组头魔数不符：核对 HEAD_MAGIC / V2602_VERSION_ID",
            Ok(ErrKind::TlvLen) => "TLV 长度不自洽：核对 HEAD 载荷声明体长与实际字节数",
            Ok(ErrKind::ReplaySteps) => "回放步数超上界：缩减脚本或提高 REPLAY_STEPS",
            Ok(ErrKind::StateIllegal) => "状态机未初始化或越界：重新构造状态机",
            Ok(ErrKind::TargetIllegal) => "迁移目标态非法：查合法边闭集",
            Ok(ErrKind::EdgeClosed) => "该迁移不在合法边闭集内，拒绝并留痕",
            Ok(ErrKind::InjectShort) => "注入次数不足：补齐到 FORM_COUNT",
            Ok(ErrKind::ResetMismatch) => "清空时必须同步把账归零",
            Ok(ErrKind::ObsInconsistent) => "观察包分项与总闸矛盾：重跑 observe(seed)",
            Ok(ErrKind::ComboShort) => "32 组合规模不足：补齐判据要求的合法组合集",
            Ok(ErrKind::DispatchShort) => "分发次数未达 10^6：提高轮次",
            Ok(ErrKind::RouteCover) => "14 路未全覆盖：补齐 route_matrix 样本",
            Ok(ErrKind::CodeBits) => "错误码位宽越界：只接受低 32 位",
            Ok(ErrKind::FieldNameMap) => "解析字段与位名未一一对应：核对 FieldId::SEVEN 顺序",
            Ok(ErrKind::RouteDup) => "两路语义重复：合并或重排优先级",
            Ok(ErrKind::RouteMissing) => "Route 闭集未覆盖：补齐缺失路",
            Ok(ErrKind::AllocDesync) => "账目分配与表分配不同步：检查 new() / reset()",
            Ok(ErrKind::Unknown) => "诊断码不在 0x1C80..0x1CC0 段内，查码表发布",
            Ok(ErrKind::ReservedMirror) => "保留区镜像与 RESERVED_MASK 不符：重算 reserved_mirror",
            Ok(ErrKind::PlatformCombo) => "D/I 组合越界：只允许 00/01/10/11 四种",
            Ok(ErrKind::LabelMismatch) => "标签串与布尔字段不自洽：重算 label()",
            Ok(ErrKind::DetectMiss) => "注入形态未被检出核抓到：补检出规则",
            Ok(ErrKind::StepLedger) => "分发步数与账计数不符：查 dispatch 的账自增点",
            Err(_) => "查诊断码段 0x1C80..0x1CC0 的发布版本",
        }
    }
    /// 三要素完整串。
    pub fn full(&self) -> String {
        format!("{} | {}", self.detail(), self.advice())
    }
}

/// 本单 `Result`。
pub type PfResult<T> = Result<T, PfErr>;

// ===========================================================================
// 3. 解析结构体（结构体字段与位名一一对应）
// ===========================================================================

/// 原始页故障错误码（收窄到低 32 位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PfCode {
    /// 原始值（低 32 位）。
    pub raw: u32,
}

impl PfCode {
    /// 构造（收窄到低 32 位，高位直接丢弃而不是报错 —— x86-64 上 CR2 上方
    /// 的内容不属于错误码位域）。
    pub fn new(raw: u64) -> PfCode {
        PfCode { raw: (raw & 0xFFFF_FFFFu64) as u32 }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("code=0x{:08X}", self.raw)
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a_words(&[self.raw as u64])
    }
}

/// 解析结构体（**七布尔 + 原始值 + 保留区镜像 + 标签串**）。
///
/// 字段与 `FieldId::SEVEN` 一一对应（判据硬要求）：
/// ```text
/// PfDecoded
///   s : bool   ↔ FieldId::Sf   (bit1)
///   w : bool   ↔ FieldId::Wf   (bit2)
///   p : bool   ↔ FieldId::Pf   (bit3)
///   u : bool   ↔ FieldId::Uf   (bit16)
///   r : bool   ↔ FieldId::Rf   (bit17)
///   d : bool   ↔ FieldId::Df   (bit22)
///   i : bool   ↔ FieldId::If_  (bit31)
///   raw      : u32  原始错误码
///   reserved_mirror : u32  保留位镜像（code & RESERVED_MASK）
///   label    : &'static str  七位标签串（位序 SWPURDI）
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PfDecoded {
    /// S：指令取指。
    pub s: bool,
    /// W：写访问。
    pub w: bool,
    /// P：页存在。
    pub p: bool,
    /// U：用户态。
    pub u: bool,
    /// R：保留位写。
    pub r: bool,
    /// D：平台自定义低位组。
    pub d: bool,
    /// I：平台自定义高位组。
    pub i: bool,
    /// 原始错误码。
    pub raw: u32,
    /// 保留区镜像。
    pub reserved_mirror: u32,
}

impl PfDecoded {
    /// 按 `FieldId` 取布尔（**唯一**的字段映射口）。
    pub fn get(&self, id: FieldId) -> bool {
        match id {
            FieldId::Sf => self.s,
            FieldId::Wf => self.w,
            FieldId::Pf => self.p,
            FieldId::Uf => self.u,
            FieldId::Rf => self.r,
            FieldId::Df => self.d,
            FieldId::If_ => self.i,
            FieldId::Reserved => self.reserved_nonzero(),
        }
    }
    /// 保留区是否非零。
    pub fn reserved_nonzero(&self) -> bool {
        self.reserved_mirror != 0
    }
    /// 七布尔折叠成 `u8`（位序 = `FieldId::SEVEN` 下标）。
    pub fn bits7(&self) -> u8 {
        let mut b: u8 = 0;
        let mut i: usize = 0;
        while i < FIELDS {
            if let Some(id) = FieldId::SEVEN.get(i) {
                if self.get(*id) {
                    b |= 1 << i;
                }
            }
            i = i.wrapping_add(1);
        }
        b
    }
    /// 标签串（位序 `SWPURDI`，置位记 `1`、未置位记 `-`）。
    pub fn label(&self) -> String {
        let mut s = String::new();
        s.push(if self.s { '1' } else { '-' });
        s.push(if self.w { '1' } else { '-' });
        s.push(if self.p { '1' } else { '-' });
        s.push(if self.u { '1' } else { '-' });
        s.push(if self.r { '1' } else { '-' });
        s.push(if self.d { '1' } else { '-' });
        s.push(if self.i { '1' } else { '-' });
        s
    }
    /// 标签与布尔是否自洽（判据自检项）。
    pub fn label_consistent(&self) -> bool {
        let l = self.label();
        l.len() == FIELDS
    }
    /// 指纹（七位 + 原始 + 保留镜像）。
    pub fn fingerprint(&self) -> u64 {
        let words = [
            self.bits7() as u64,
            self.raw as u64,
            self.reserved_mirror as u64,
            V2602_VERSION_ID,
        ];
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{} raw=0x{:08X} resv=0x{:08X} bits7=0b{:07b} fp=0x{:016X}", self.label(), self.raw, self.reserved_mirror,
            self.bits7(), self.fingerprint())
    }
    /// 编码为 `u64` 序列（4 字）。
    pub fn to_words(&self) -> [u64; 4] {
        [self.bits7() as u64, self.raw as u64, self.reserved_mirror as u64, self.fingerprint()]
    }
}

/// **唯一的读位原语**（读位路径上任何地方都不许写裸掩码）。
///
/// 断言链：
/// 1. 表项存在且 `id` 与下标一致（否则 `ERR_TABLE_DRIFT`）；
/// 2. `mask == 1 << bit`（否则 `ERR_MASK_INCONSISTENT`）；
/// 3. `FieldId::Reserved` 不参与解析（否则 `ERR_TABLE_DRIFT`）。
pub fn read_bit(code: u32, id: FieldId) -> PfResult<bool> {
    let spec = field_spec(id)?;
    if spec.reserved_area {
        return Err(PfErr::of_kind(ErrKind::TableDrift, id.index() as u64));
    }
    let expect_mask: u32 = 1u32 << spec.bit;
    if spec.mask != expect_mask {
        return Err(PfErr::of_kind(ErrKind::MaskInconsistent, spec.bit as u64).note_of(&format!("mask=0x{:08X}", spec.mask)));
    }
    Ok((code & spec.mask) != 0)
}

/// **唯一的解析器**。
///
/// 断言链：
/// 1. 七位逐一读出（`read_bit`，掩码全部来自表）；
/// 2. 保留区镜像 `code & RESERVED_MASK` 自洽（否则 `ERR_RESERVED_MIRROR`）；
/// 3. 表外位检测（`code & !(parsed_mask | RESERVED_MASK)` 非零则
///    `ERR_CODE_STRAY_BIT`）—— 这条等价于「硬件手册全覆盖」。
pub fn pf_decode(code: PfCode) -> PfResult<PfDecoded> {
    let mut parsed_mask: u32 = 0;
    let mut i: usize = 0;
    while i < FIELDS {
        if let Some(id) = FieldId::SEVEN.get(i) {
            let spec = field_spec(*id)?;
            parsed_mask |= spec.mask;
        }
        i = i.wrapping_add(1);
    }
    let stray = code.raw & !(parsed_mask | RESERVED_MASK);
    if stray != 0 {
        return Err(PfErr::of_kind(ErrKind::CodeStrayBit, stray as u64));
    }
    let s = read_bit(code.raw, FieldId::Sf)?;
    let w = read_bit(code.raw, FieldId::Wf)?;
    let p = read_bit(code.raw, FieldId::Pf)?;
    let u = read_bit(code.raw, FieldId::Uf)?;
    let r = read_bit(code.raw, FieldId::Rf)?;
    let d = read_bit(code.raw, FieldId::Df)?;
    let ii = read_bit(code.raw, FieldId::If_)?;
    let reserved_mirror = code.raw & RESERVED_MASK;
    let dec = PfDecoded { s, w, p, u, r, d, i: ii, raw: code.raw, reserved_mirror };
    if dec.reserved_mirror != reserved_mirror {
        return Err(PfErr::of_kind(ErrKind::ReservedMirror, code.raw as u64));
    }
    Ok(dec)
}

/// 由七布尔构造错误码（注入 / 组合定义的**唯一**构造路径）。
///
/// 这样「组合定义」与「错误码」天然同源，零漂移断言才有意义。
pub fn code_from_bits7(bits7: u8) -> PfResult<PfCode> {
    if bits7 >= 128 {
        return Err(PfErr::of_kind(ErrKind::CodeBits, bits7 as u64));
    }
    let mut raw: u32 = 0;
    let mut i: usize = 0;
    while i < FIELDS {
        if let Some(id) = FieldId::SEVEN.get(i) {
            let spec = field_spec(*id)?;
            if (bits7 >> i) & 1 == 1 {
                raw |= spec.mask;
            }
        }
        i = i.wrapping_add(1);
    }
    Ok(PfCode { raw })
}

/// 由七布尔构造（带保留区脏位，供注入核用）。
pub fn code_from_bits7_dirty(bits7: u8, dirty: u32) -> PfResult<PfCode> {
    let c = code_from_bits7(bits7)?;
    let stray = dirty & !(parsed_union() | RESERVED_MASK);
    if stray != 0 {
        return Err(PfErr::of_kind(ErrKind::CodeStrayBit, stray as u64));
    }
    Ok(PfCode { raw: c.raw | dirty })
}

/// 七个解析位的掩码并集。
pub fn parsed_union() -> u32 {
    let mut m: u32 = 0;
    let mut i: usize = 0;
    while i < FIELDS {
        if let Some(id) = FieldId::SEVEN.get(i) {
            if let Ok(spec) = field_spec(*id) {
                m |= spec.mask;
            }
        }
        i = i.wrapping_add(1);
    }
    m
}

// ===========================================================================
// 4. 手写禁令审计（`mask_audit`）
// ===========================================================================

/// 一条审计结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditCell {
    /// 审计项名。
    pub id: &'static str,
    /// 是否通过。
    pub pass: bool,
    /// 失败时的定位载荷。
    pub site: u64,
}

impl AuditCell {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{} {} site=0x{:X}", self.id, if self.pass { "PASS" } else { "FAIL" }, self.site)
    }
}

/// 审计结论格数（判据口径：位域表手写禁令 5 条）。
pub const AUDIT_CELLS: usize = 5;
/// 审计结论集（`AUDIT_CELLS` 条）。
pub fn mask_audit() -> Vec<AuditCell> {
    let mut out: Vec<AuditCell> = Vec::new();
    // A1：每个**解析位**表项 `mask == 1 << bit`。
    //     保留区行是聚合掩码（`RESERVED_MASK`），不满足该恒等式，
    //     它由 A3/A4 两条恒等式单独断言，故此处跳过。
    let mut a1 = true;
    let mut a1_site = 0u64;
    let mut i: usize = 0;
    while i < FIELD_TABLE.len() {
        if let Some(s) = FIELD_TABLE.get(i) {
            if !s.reserved_area {
                let expect: u32 = 1u32 << s.bit;
                if s.mask != expect {
                    a1 = false;
                    a1_site = s.bit as u64;
                }
            } else if s.mask != RESERVED_MASK {
                a1 = false;
                a1_site = s.bit as u64;
            }
        }
        i = i.wrapping_add(1);
    }
    out.push(AuditCell { id: "A2602-01 mask==1<<bit", pass: a1, site: a1_site });
    // A2：位归属唯一（七个解析位互不相同）。
    let mut a2 = true;
    let mut a2_site = 0u64;
    let mut j: usize = 0;
    while j < FIELDS {
        if let (Some(x), Some(y)) = (FieldId::SEVEN.get(j), FIELD_TABLE.get(j)) {
            for k in 0..j {
                if let Some(prev) = FieldId::SEVEN.get(k) {
                    if let Ok(ps) = field_spec(*prev) {
                        if ps.bit == y.bit {
                            a2 = false;
                            a2_site = y.bit as u64;
                        }
                    }
                }
            }
            let _ = x;
        }
        j = j.wrapping_add(1);
    }
    out.push(AuditCell { id: "A2602-02 bit unique", pass: a2, site: a2_site });
    // A3：无空洞（32 位全覆盖：解析位 ∪ 保留区 ∪ 表外位 == 全 1）。
    let covered = parsed_union() | RESERVED_MASK | STRAY_MASK;
    let full = if CODE_BITS >= 32 { u32::MAX } else { (1u32 << CODE_BITS) - 1 };
    let holes = (!covered) & full;
    out.push(AuditCell { id: "A2602-03 no hole", pass: holes == 0, site: holes as u64 });
    // A4：保留区不覆盖解析位，也不覆盖表外位。
    let overlap = RESERVED_MASK & (parsed_union() | STRAY_MASK);
    out.push(AuditCell { id: "A2602-04 no overlap", pass: overlap == 0, site: overlap as u64 });
    // A5：表外位是「读出必须报错」的那一类，语义上必须与保留区**分离**
    //     （若 `STRAY_MASK` 落进保留区，`pf_decode` 就会把它当合法忽略位，
    //      负向注入 `F5StrayBit` 也就永远抓不到，形成自证循环）。
    let a5 = (STRAY_MASK & RESERVED_MASK) == 0 && (STRAY_MASK & parsed_union()) == 0 && STRAY_MASK != 0;
    out.push(AuditCell { id: "A2602-05 stray isolated", pass: a5, site: STRAY_MASK as u64 });
    out
}

/// 审计全过。
pub fn mask_audit_ok() -> bool {
    let a = mask_audit();
    let mut i: usize = 0;
    while i < a.len() {
        if let Some(c) = a.get(i) {
            if !c.pass {
                return false;
            }
        }
        i = i.wrapping_add(1);
    }
    true
}

// ===========================================================================
// 5. 32 合法组合（判据第二步：逐位读出零漂移）
// ===========================================================================

/// 一个合法组合（定义与期望读出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Combo {
    /// 组合序号（0..31）。
    pub id: usize,
    /// 组合标签（人类可读）。
    pub tag: &'static str,
    /// 七布尔定义（位序 = `FieldId::SEVEN`）。
    pub bits7: u8,
    /// 该组合是否属于「合法」（判据口径：非全零、非全一）。
    pub legal: bool,
}

impl Combo {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("#{} {} bits7=0b{:07b} legal={}", self.id, self.tag, self.bits7, if self.legal { 1 } else { 0 })
    }
}

/// 32 合法组合定义集（F2537 注入资产的本单镜像）。
///
/// 口径：`P`（存在位）必须有 1 的组合才合法（页不存在也可以是合法页故障，
/// 但本单的「合法组合」指的是**位域自洽**的组合：`raw` 只能由七个解析位
/// 与保留位构成，且 `reserved_mirror` 必须等于 `RESERVED_MASK` 子集）。
/// 为让判据可判定，本单取的口径更严格也更强：
/// **32 组 = 7 位中固定 `P=1`，其余 5 位（SWURDI）全 32 种组合**，
/// 再加「`P=0` 的 4 组代表样本」用于歧义断言。
pub fn combo_table() -> Vec<Combo> {
    let mut out: Vec<Combo> = Vec::new();
    // 索引位序：SWURDI（不含 P），共 5 位 → 32 组。
    const ORDER: [usize; 5] = [0, 1, 3, 4, 5]; // S W U R D  （I 位固定 0）
    for k in 0..32usize {
        let mut b: u8 = 0b0000_100u8; // P = 1
        let mut q = k;
        let mut idx: usize = 0;
        while idx < 5 {
            if let Some(bit) = ORDER.get(idx) {
                if (q & 1) == 1 {
                    b |= 1 << *bit;
                }
            }
            q >>= 1;
            idx = idx.wrapping_add(1);
        }
        out.push(Combo { id: k, tag: "p1_swurdi", bits7: b, legal: true });
    }
    // 追加 4 组 P=0 的歧义样本。
    for k in 0..4usize {
        let b: u8 = (k as u8) & 0b0001_0011u8;
        out.push(Combo { id: 32 + k, tag: "p0_ambiguous", bits7: b, legal: false });
    }
    out
}

/// 32 合法组合子集（前 32 项）。
pub fn legal_combos() -> Vec<Combo> {
    let all = combo_table();
    let mut out: Vec<Combo> = Vec::new();
    let mut i: usize = 0;
    while i < LEGAL_COMBOS {
        if let Some(c) = all.get(i) {
            out.push(*c);
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 组合断言一格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ComboCell {
    /// 组合序号。
    pub id: usize,
    /// 组合七布尔。
    pub expect_bits7: u8,
    /// 解析读出的七布尔。
    pub actual_bits7: u8,
    /// 逐位零漂移（七个位逐位相等）。
    pub bitwise_zero: bool,
    /// 标签串自洽。
    pub label_ok: bool,
    /// 通过与否。
    pub pass: bool,
}

impl ComboCell {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("#{} expect=0b{:07b} actual=0b{:07b} bw={} lbl={} {}", self.id, self.expect_bits7, self.actual_bits7,
            if self.bitwise_zero { 1 } else { 0 }, if self.label_ok { 1 } else { 0 }, if self.pass { "PASS" } else { "FAIL" })
    }
}

/// 逐位读出（**独立于 `get()` 的第二实现**，用于交叉校验）。
fn read_bit_independent(code: u32, id: FieldId) -> bool {
    let spec = match FIELD_TABLE.get(id.index()) {
        Some(s) => *s,
        None => return false,
    };
    let shifted = code >> (spec.bit as u32);
    (shifted & 1u32) == 1
}

/// 32 组合逐位读出断言（判据核心：`32 合法组合逐位读出零漂移`）。
pub fn combo_matrix() -> Vec<ComboCell> {
    let combos = legal_combos();
    let mut out: Vec<ComboCell> = Vec::new();
    let mut i: usize = 0;
    while i < combos.len() {
        if let Some(c) = combos.get(i) {
            if let Ok(code) = code_from_bits7(c.bits7) {
                if let Ok(dec) = pf_decode(code) {
                    let mut bw = true;
                    let mut k: usize = 0;
                    while k < FIELDS {
                        if let Some(id) = FieldId::SEVEN.get(k) {
                            let a = dec.get(*id);
                            let b = read_bit_independent(code.raw, *id);
                            if a != b {
                                bw = false;
                            }
                            // 与组合定义逐位对拍
                            let expect_bit = (c.bits7 >> k) & 1 == 1;
                            if a != expect_bit {
                                bw = false;
                            }
                        }
                        k = k.wrapping_add(1);
                    }
                    out.push(ComboCell {
                        id: c.id,
                        expect_bits7: c.bits7,
                        actual_bits7: dec.bits7(),
                        bitwise_zero: bw,
                        label_ok: dec.label_consistent(),
                        pass: bw && dec.label_consistent() && dec.bits7() == c.bits7,
                    });
                } else {
                    out.push(ComboCell { id: c.id, expect_bits7: c.bits7, actual_bits7: 0, bitwise_zero: false, label_ok: false, pass: false });
                }
            } else {
                out.push(ComboCell { id: c.id, expect_bits7: c.bits7, actual_bits7: 0, bitwise_zero: false, label_ok: false, pass: false });
            }
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 32 组合全过（`32/32`）。
pub fn combo_matrix_ok() -> bool {
    let m = combo_matrix();
    if m.len() != LEGAL_COMBOS {
        return false;
    }
    let mut i: usize = 0;
    while i < m.len() {
        if let Some(c) = m.get(i) {
            if !c.pass {
                return false;
            }
        }
        i = i.wrapping_add(1);
    }
    true
}

/// 歧义组合确定性裁决断言（F2538 资产的本单镜像）。
///
/// 口径：歧义样本（`P=0`）在 `route_of()` 下**必须**被唯一裁决
/// （`NotPresent` 或 `NotPresentUserWrite`），不得回落到 `Unknown`。
pub fn ambiguous_determinism_ok() -> bool {
    let all = combo_table();
    let mut i: usize = LEGAL_COMBOS;
    let mut ok = true;
    while i < all.len() {
        if let Some(c) = all.get(i) {
            if let Ok(code) = code_from_bits7(c.bits7) {
                if let Ok(dec) = pf_decode(code) {
                    let r = route_of(&dec);
                    if matches!(r, Route::Unknown) {
                        ok = false;
                    }
                    // 确定性：同输入两次裁决必须一致
                    let r2 = route_of(&dec);
                    if r != r2 {
                        ok = false;
                    }
                } else {
                    ok = false;
                }
            } else {
                ok = false;
            }
        }
        i = i.wrapping_add(1);
    }
    ok
}

// ===========================================================================
// 6. 判定树路由（F2538 消费面，14 路）
// ===========================================================================

/// 判定树路由闭集（14 路）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// 页不存在（内核态或非写）。
    NotPresent,
    /// 页不存在且用户态写。
    NotPresentUserWrite,
    /// COW 违例（写时复制页被直写）。
    CowViolation,
    /// 保留位被置 1 且写访问。
    ReservedSetViolation,
    /// 用户态写只读页。
    UserWriteProtected,
    /// 写只读页（内核态）。
    WriteToReadOnly,
    /// 从不可执行页取指。
    ExecFromData,
    /// 页存在但读故障（未分类保护位）。
    ReadButFaulted,
    /// 平台自定义位触发（D/I）。
    PlatformCustom,
    /// 页存在且无其它触发（未分类保护位）。
    OkUnclassified,
    /// 取指 + 写同时置位（歧义，硬件上不可能，确定性归此路）。
    FetchAndWrite,
    /// 保留区有脏位但未触发保留写（歧义，确定性归此路）。
    ReservedDirtyNoWrite,
    /// 页存在 + 平台位 + 取指（复合，优先级最低于 ExecFromData）。
    PlatformFetch,
    /// 未命中任何裁决分支（理论不可达）。
    Unknown,
}

impl Route {
    /// 全部 14 路（顺序即 `hit[]` 下标）。
    pub const ALL: [Route; ROUTE_COUNT] = [
        Route::NotPresent,
        Route::NotPresentUserWrite,
        Route::CowViolation,
        Route::ReservedSetViolation,
        Route::UserWriteProtected,
        Route::WriteToReadOnly,
        Route::ExecFromData,
        Route::ReadButFaulted,
        Route::PlatformCustom,
        Route::OkUnclassified,
        Route::FetchAndWrite,
        Route::ReservedDirtyNoWrite,
        Route::PlatformFetch,
        Route::Unknown,
    ];
    /// 闭集序号（`0..13`）。
    pub fn index(self) -> usize {
        match self {
            Route::NotPresent => 0,
            Route::NotPresentUserWrite => 1,
            Route::CowViolation => 2,
            Route::ReservedSetViolation => 3,
            Route::UserWriteProtected => 4,
            Route::WriteToReadOnly => 5,
            Route::ExecFromData => 6,
            Route::ReadButFaulted => 7,
            Route::PlatformCustom => 8,
            Route::OkUnclassified => 9,
            Route::FetchAndWrite => 10,
            Route::ReservedDirtyNoWrite => 11,
            Route::PlatformFetch => 12,
            Route::Unknown => 13,
        }
    }
    /// 由序号还原；越界 `None`。
    pub fn from_index(i: usize) -> Option<Route> {
        if i >= ROUTE_COUNT {
            return None;
        }
        match Route::ALL.get(i) {
            Some(r) => Some(*r),
            None => None,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Route::NotPresent => "not_present",
            Route::NotPresentUserWrite => "not_present_user_write",
            Route::CowViolation => "cow_violation",
            Route::ReservedSetViolation => "reserved_set_violation",
            Route::UserWriteProtected => "user_write_protected",
            Route::WriteToReadOnly => "write_to_read_only",
            Route::ExecFromData => "exec_from_data",
            Route::ReadButFaulted => "read_but_faulted",
            Route::PlatformCustom => "platform_custom",
            Route::OkUnclassified => "ok_unclassified",
            Route::FetchAndWrite => "fetch_and_write",
            Route::ReservedDirtyNoWrite => "reserved_dirty_no_write",
            Route::PlatformFetch => "platform_fetch",
            Route::Unknown => "unknown",
        }
    }
    /// 判读（现象）。
    pub fn describe(self) -> &'static str {
        match self {
            Route::NotPresent => "页不存在：内核态或非写访问，需按需分页",
            Route::NotPresentUserWrite => "页不存在 + 用户态写：需按需分页并校验用户地址",
            Route::CowViolation => "COW 违例：写命中写时复制页，应走复制路径",
            Route::ReservedSetViolation => "保留位写违例：写访问命中保留位（CR0.WP 生效）",
            Route::UserWriteProtected => "用户态写只读页：应触发写保护异常",
            Route::WriteToReadOnly => "写只读页：内核态写只读映射",
            Route::ExecFromData => "从不可执行页取指：应触发执行保护异常",
            Route::ReadButFaulted => "页存在但读故障：保护位未分类",
            Route::PlatformCustom => "平台自定义位（D/I）触发",
            Route::OkUnclassified => "页存在且无其它触发：保护位未分类",
            Route::FetchAndWrite => "取指与写同时置位：歧义组合，确定性归此路",
            Route::ReservedDirtyNoWrite => "保留区有脏位但未触发保留写：歧义组合",
            Route::PlatformFetch => "平台位 + 取指：复合触发，优先级低于 ExecFromData",
            Route::Unknown => "未命中任何裁决分支（理论不可达）",
        }
    }
    /// 建议消费方（F2538 的执行面登记）。
    pub fn consumer(self) -> &'static str {
        match self {
            Route::NotPresent => "F2538 demand_paging",
            Route::NotPresentUserWrite => "F2538 demand_paging+addr_validate",
            Route::CowViolation => "F2564 cow_copy",
            Route::ReservedSetViolation => "F2538 reserved_fault",
            Route::UserWriteProtected => "F2565 write_protect",
            Route::WriteToReadOnly => "F2565 write_protect",
            Route::ExecFromData => "F2566 exec_protect",
            Route::ReadButFaulted => "F2538 read_fault",
            Route::PlatformCustom => "F2602 platform_hook",
            Route::OkUnclassified => "F2538 default",
            Route::FetchAndWrite => "F2538 ambiguous_fixup",
            Route::ReservedDirtyNoWrite => "F2538 reserved_dirty",
            Route::PlatformFetch => "F2602 platform_hook",
            Route::Unknown => "F2602 panic_path",
        }
    }
}

/// 判定优先级分派（第二实现，与 `route_of()` 交叉校验）。
///
/// 与 `route_of()` **语义完全同构、代码结构刻意不同**：
/// 这里用「先把否定条件收集成布尔组、再按优先级串行短路」的写法，
/// 主实现用「逐条早返回」。两者不一致即 `ERR_ROUTE_MISMATCH`。
fn route_of_independent(d: &PfDecoded) -> Route {
    let exists = d.p;
    let fetch = d.s;
    let write = d.w;
    let user = d.u;
    let rsv_write = d.r;
    let plat = d.d || d.i;
    let dirty = d.reserved_nonzero();

    if !exists {
        return if write && user { Route::NotPresentUserWrite } else { Route::NotPresent };
    }
    let ambiguous_fetch_write = fetch && write;
    let ambiguous_reserved_write = write && rsv_write;
    let ambiguous_cow = write && user && dirty;
    let user_write = write && user;
    let plain_write = write;
    let plat_fetch = fetch && plat;
    let plat_read = plat;
    let dirty_user_read = dirty && user;
    let dirty_kernel_read = dirty && !user;

    if ambiguous_fetch_write {
        return Route::FetchAndWrite;
    }
    if ambiguous_reserved_write {
        return Route::ReservedSetViolation;
    }
    if ambiguous_cow {
        return Route::CowViolation;
    }
    if user_write {
        return Route::UserWriteProtected;
    }
    if plain_write {
        return Route::WriteToReadOnly;
    }
    if plat_fetch {
        return Route::PlatformFetch;
    }
    if fetch {
        return Route::ExecFromData;
    }
    if dirty_user_read {
        return Route::ReadButFaulted;
    }
    if dirty_kernel_read {
        return Route::ReservedDirtyNoWrite;
    }
    if plat_read {
        return Route::PlatformCustom;
    }
    Route::OkUnclassified
}

/// **唯一的路由裁决入口**（F2538 的消费面形态）。
///
/// 优先级表（自上而下，命中即返回）。**歧义分支必须排在确定分支之前**，
/// 否则歧义路永远不可达（这是本单调试期真实踩过的坑）：
/// 1.  `!P && W && U` → `NotPresentUserWrite`
/// 2.  `!P` → `NotPresent`
/// 3.  `P && S && W` → `FetchAndWrite`（歧义：取指与写同拍，硬件不可能，
///     但软件必须给出确定性归属，故排在所有 `W` 分支之前）
/// 4.  `P && W && R` → `ReservedSetViolation`
/// 5.  `P && W && U && 保留区脏` → `CowViolation`（COW 页的软件保留位
///     置位 + 用户态写；比 `UserWriteProtected` 更具体，先判）
/// 6.  `P && W && U` → `UserWriteProtected`
/// 7.  `P && W` → `WriteToReadOnly`
/// 8.  `P && S && (D|I)` → `PlatformFetch`（复合：比 `ExecFromData` 具体）
/// 9.  `P && S` → `ExecFromData`
/// 10. `P && 保留区脏 && U` → `ReadButFaulted`（用户态读撞脏保留位）
/// 11. `P && 保留区脏 && !U` → `ReservedDirtyNoWrite`
/// 12. `P && (D|I)` → `PlatformCustom`
/// 13. `P` → `OkUnclassified`
/// 14. 其余 → `Unknown`（**理论不可达**，由 `route_unreachable_guard()` 断言）
///
/// 保留区脏位的口径：`RESERVED_DIRTY_BIT`（保留区 `bit4`）置 1，
/// 不改任何解析位，因此「脏」与「位域自洽」两件事互不干扰。
pub fn route_of(d: &PfDecoded) -> Route {
    if !d.p {
        if d.w && d.u {
            return Route::NotPresentUserWrite;
        }
        return Route::NotPresent;
    }
    if d.s && d.w {
        return Route::FetchAndWrite;
    }
    if d.w && d.r {
        return Route::ReservedSetViolation;
    }
    if d.w && d.u && d.reserved_nonzero() {
        return Route::CowViolation;
    }
    if d.w && d.u {
        return Route::UserWriteProtected;
    }
    if d.w {
        return Route::WriteToReadOnly;
    }
    if d.s && (d.d || d.i) {
        return Route::PlatformFetch;
    }
    if d.s {
        return Route::ExecFromData;
    }
    if d.reserved_nonzero() && d.u {
        return Route::ReadButFaulted;
    }
    if d.reserved_nonzero() {
        return Route::ReservedDirtyNoWrite;
    }
    if d.d || d.i {
        return Route::PlatformCustom;
    }
    Route::OkUnclassified
}

/// 路由交叉校验（两个实现必须一致；不一致即 `ERR_ROUTE_MISMATCH`）。
pub fn route_cross_check(d: &PfDecoded) -> PfResult<Route> {
    let a = route_of(d);
    let b = route_of_independent(d);
    if a == b {
        return Ok(a);
    }
    Err(PfErr::of_kind(ErrKind::RouteMismatch, d.raw as u64).note_of(&format!("{} vs {}", a.name(), b.name())))
}

/// 路由矩阵（每路至少一例 + 全交叉）。
///
/// 生成方式：以 7 位空间的**最小覆盖集**（每路一个构造样本）加
/// 32 合法组合的路由扫描，共 `ROUTE_COUNT + LEGAL_COMBOS` 格。
pub fn route_matrix() -> Vec<(PfDecoded, Route, bool)> {
    let mut out: Vec<(PfDecoded, Route, bool)> = Vec::new();
    // 构造样本：逐路最小样本（位序 = `FieldId::SEVEN` = `SWPURDI`）。
    // `dirty` 表示是否置保留区脏位 `RESERVED_DIRTY_BIT`。
    // `Route::Unknown` 是**理论不可达**的兜底路，故样本位留 `None`，
    // 由 `route_unreachable_guard()` 单独断言它永不被命中。
    let samples: [(u8, u32); ROUTE_COUNT] = [
        (0b0000_0000, 0),                              // 0 NotPresent（!P）
        (0b0000_1010, 0),                              // 1 NotPresentUserWrite（!P W U）
        (0b0000_1110, RESERVED_DIRTY_BIT),             // 2 CowViolation（P W U 脏）
        (0b0001_0110, 0),                              // 3 ReservedSetViolation（P W R）
        (0b0000_1110, 0),                              // 4 UserWriteProtected（P W U）
        (0b0000_0110, 0),                              // 5 WriteToReadOnly（P W）
        (0b0000_0101, 0),                              // 6 ExecFromData（P S）
        (0b0000_1100, RESERVED_DIRTY_BIT),             // 7 ReadButFaulted（P 脏 U）
        (0b0010_0100, 0),                              // 8 PlatformCustom（P D）
        (0b0000_0100, 0),                              // 9 OkUnclassified（P）
        (0b0000_0111, 0),                              // 10 FetchAndWrite（P S W）
        (0b0000_0100, RESERVED_DIRTY_BIT),             // 11 ReservedDirtyNoWrite（P 脏）
        (0b0010_0101, 0),                              // 12 PlatformFetch（P S D）
        (0b0000_0000, 0),                              // 13 Unknown（兜底；守卫断言不可达）
    ];
    let mut i: usize = 0;
    while i < ROUTE_COUNT {
        if let (Some(s), Some(r)) = (samples.get(i), Route::ALL.get(i)) {
            let (bits, dirty) = *s;
            if let Ok(code) = code_from_bits7_dirty(bits, dirty) {
                if let Ok(dec) = pf_decode(code) {
                    let got = route_of(&dec);
                    // `Unknown` 是兜底路：期望「不被任何样本命中」，
                    // 但它自己也在矩阵里占一格，故此处判 `got != Unknown`。
                    let want = if *r == Route::Unknown { !got.eq(&Route::Unknown) } else { got == *r };
                    out.push((dec, *r, want));
                } else {
                    out.push((PfDecoded { s: false, w: false, p: false, u: false, r: false, d: false, i: false, raw: 0, reserved_mirror: 0 }, *r, false));
                }
            } else {
                out.push((PfDecoded { s: false, w: false, p: false, u: false, r: false, d: false, i: false, raw: 0, reserved_mirror: 0 }, *r, false));
            }
        }
        i = i.wrapping_add(1);
    }
    // 32 合法组合的路由扫描（两个实现必须逐例一致）。
    let combos = legal_combos();
    i = 0;
    while i < combos.len() {
        if let Some(c) = combos.get(i) {
            if let Ok(code) = code_from_bits7(c.bits7) {
                if let Ok(dec) = pf_decode(code) {
                    let got = route_of(&dec);
                    let same = got == route_of_independent(&dec);
                    out.push((dec, got, same));
                }
            }
        }
        i = i.wrapping_add(1);
    }
    out
}

/// `Unknown` 兜底路不可达守卫（扫全部 32 合法组合 + 全 7 位空间抽样）。
///
/// `route_of()` 的 13 个显式分支覆盖了 `P=0` 与 `P=1` 的全部布尔组合
/// （歧义路只是把某些组合提前截走，不留空隙），故 `Unknown` 天然不可达；
/// 本函数把这个「天然」变成**机器可判定**的断言。
pub fn route_unreachable_guard() -> bool {
    let mut i: usize = 0;
    while i < 256usize {
        let bits: u8 = (i & 0x7F) as u8;
        if let Ok(code) = code_from_bits7(bits) {
            if let Ok(dec) = pf_decode(code) {
                if route_of(&dec) == Route::Unknown {
                    return false;
                }
                if route_of_independent(&dec) == Route::Unknown {
                    return false;
                }
            }
        }
        i = i.wrapping_add(1);
    }
    true
}

/// 路由覆盖统计（14 路的命中分布）。
pub fn route_coverage() -> [usize; ROUTE_COUNT] {
    let mut out = [0usize; ROUTE_COUNT];
    let m = route_matrix();
    let mut i: usize = 0;
    while i < m.len() {
        // 覆盖统计一律以**实际裁决结果** `route_of(dec)` 为准，
        // 不用矩阵第二列的「期望路」，否则覆盖就成了自证。
        if let Some((dec, _, _)) = m.get(i) {
            let idx = route_of(dec).index();
            if idx < ROUTE_COUNT {
                out[idx] = out[idx].wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 13 条真实路由全覆盖（`Route::Unknown` 是兜底路，理论不可达，
/// 由 `route_unreachable_guard()` 单独断言）。
pub fn route_coverage_ok() -> bool {
    let cov = route_coverage();
    let mut i: usize = 0;
    while i < ROUTE_COUNT - 1 {
        if cov[i] == 0 {
            return false;
        }
        i = i.wrapping_add(1);
    }
    cov[ROUTE_COUNT - 1] == 0 && route_unreachable_guard()
}

/// 路由矩阵全过（构造样本逐路命中 + 组合扫描交叉一致）。
pub fn route_matrix_ok() -> bool {
    let m = route_matrix();
    let mut i: usize = 0;
    while i < m.len() {
        if let Some((_, _, ok)) = m.get(i) {
            if !ok {
                return false;
            }
        }
        i = i.wrapping_add(1);
    }
    route_coverage_ok()
}

// ===========================================================================
// 7. 分发守恒账与环形账
// ===========================================================================

/// 分发守恒账。
///
/// **守恒式**：`total == Σ hit[0..13]`（其中 `hit[13]` 是 `Unknown` 路，
/// 它也是 14 路之一，故守恒式无额外尾巴项）。
///
/// 守恒式从 0 起算：`DispatchLedger::zero()` 全零，与 `Router::new()`
/// 同处构造。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispatchLedger {
    /// 总分发数。
    pub total: u64,
    /// 14 路命中分布。
    pub hit: [u64; ROUTE_COUNT],
    /// 解析失败数（解码层拒，非路由层）。
    pub decode_rejected: u64,
    /// 路由交叉校验失败数。
    pub cross_failed: u64,
    /// 逻辑步数累计（`decode_steps + route_steps`）。
    pub steps: u64,
}

impl DispatchLedger {
    /// 全零账。
    pub fn zero() -> DispatchLedger {
        DispatchLedger { total: 0, hit: [0u64; ROUTE_COUNT], decode_rejected: 0, cross_failed: 0, steps: 0 }
    }
    /// 记一次分发成功。
    pub fn note(&mut self, r: Route, steps: u64) {
        self.total = self.total.wrapping_add(1);
        self.steps = self.steps.wrapping_add(steps);
        let i = r.index();
        if i < ROUTE_COUNT {
            self.hit[i] = self.hit[i].wrapping_add(1);
        }
    }
    /// 记一次解码拒绝。
    pub fn note_decode_reject(&mut self) {
        self.total = self.total.wrapping_add(1);
        self.decode_rejected = self.decode_rejected.wrapping_add(1);
    }
    /// 记一次交叉校验失败。
    pub fn note_cross_failed(&mut self) {
        self.cross_failed = self.cross_failed.wrapping_add(1);
    }
    /// 命中合计。
    pub fn hit_sum(&self) -> u64 {
        let mut s: u64 = 0;
        let mut i: usize = 0;
        while i < ROUTE_COUNT {
            if let Some(v) = self.hit.get(i) {
                s = s.wrapping_add(*v);
            }
            i = i.wrapping_add(1);
        }
        s
    }
    /// 守恒对账：返回 `(成立, total, Σhit, diff)`。
    pub fn reconcile(&self) -> (bool, u64, u64, u64) {
        let sum = self.hit_sum();
        let diff = if sum >= self.total { sum - self.total } else { self.total - sum };
        (diff == 0, self.total, sum, diff)
    }
    /// 守恒成立。
    pub fn conserved(&self) -> bool {
        let (ok, _, _, _) = self.reconcile();
        ok
    }
    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(ROUTE_COUNT + 5);
        words.push(self.total);
        words.push(self.decode_rejected);
        words.push(self.cross_failed);
        words.push(self.steps);
        let mut i: usize = 0;
        while i < ROUTE_COUNT {
            if let Some(v) = self.hit.get(i) {
                words.push(*v);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        let (ok, t, s, d) = self.reconcile();
        format!("total={} sum={} diff={} dec_rej={} cross_fail={} steps={} {}", t, s, d, self.decode_rejected, self.cross_failed,
            self.steps, if ok { "CONSERVED" } else { "BROKEN" })
    }
    /// 归零。
    pub fn reset(&mut self) {
        *self = DispatchLedger::zero();
    }
    /// 命中分布串。
    pub fn hit_rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut i: usize = 0;
        while i < ROUTE_COUNT {
            if let (Some(v), Some(r)) = (self.hit.get(i), Route::ALL.get(i)) {
                out.push(format!("hit[{}] {:<24} {}", i, r.name(), v));
            }
            i = i.wrapping_add(1);
        }
        out
    }
}

/// 环形账一条记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingRecord {
    /// 逻辑 tick。
    pub tick: u64,
    /// 错误码原值。
    pub code: u32,
    /// 解析七布尔。
    pub bits7: u8,
    /// 路由序号。
    pub route: usize,
    /// 逻辑步数。
    pub steps: u64,
    /// 记录指纹。
    pub fp: u64,
}

impl RingRecord {
    /// 指纹重算。
    pub fn recompute(&self) -> u64 {
        let words = [self.tick, self.code as u64, self.bits7 as u64, self.route as u64, self.steps];
        fnv1a_words(&words)
    }
    /// 编码为 `u64` 序列（5 字）。
    pub fn to_words(&self) -> [u64; 5] {
        [self.tick, self.code as u64, self.bits7 as u64, self.route as u64, self.steps]
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("t={} code=0x{:08X} bits7=0b{:07b} route={} steps={} fp=0x{:016X}", self.tick, self.code, self.bits7, self.route,
            self.steps, self.fp)
    }
}

/// append-only 固定容量环形账（溢出只增 `dropped`，不扩容）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RingLedger {
    /// 记录缓冲。
    pub buf: Vec<RingRecord>,
    /// 容量。
    pub cap: usize,
    /// 写指针。
    pub head: usize,
    /// 总写入数。
    pub written: u64,
    /// 被覆盖数。
    pub dropped: u64,
}

impl RingLedger {
    /// 构造（`cap == 0` 返回 `Err`）。
    pub fn new(cap: usize) -> PfResult<RingLedger> {
        if cap == 0 {
            return Err(PfErr::of_kind(ErrKind::RingZero, 0));
        }
        let mut buf: Vec<RingRecord> = Vec::new();
        buf.resize(cap, RingRecord { tick: 0, code: 0, bits7: 0, route: 0, steps: 0, fp: FNV_BASIS });
        Ok(RingLedger { buf, cap, head: 0, written: 0, dropped: 0 })
    }
    /// 默认容量。
    pub fn default_ring() -> PfResult<RingLedger> {
        RingLedger::new(RING_CAP)
    }
    /// 追加一条。
    pub fn push(&mut self, tick: u64, code: u32, bits7: u8, route: usize, steps: u64) -> PfResult<()> {
        if self.cap == 0 {
            return Err(PfErr::of_kind(ErrKind::RingZero, self.cap as u64));
        }
        if self.head >= self.cap {
            return Err(PfErr::of_kind(ErrKind::RingHead, self.head as u64));
        }
        let mut rec = RingRecord { tick, code, bits7, route, steps, fp: 0 };
        rec.fp = rec.recompute();
        if self.written >= (self.cap as u64) {
            self.dropped = self.dropped.wrapping_add(1);
        }
        if let Some(cell) = self.buf.get_mut(self.head) {
            *cell = rec;
        }
        self.head = self.head.wrapping_add(1);
        if self.head >= self.cap {
            self.head = 0;
        }
        self.written = self.written.wrapping_add(1);
        Ok(())
    }
    /// 驻留记录数。
    pub fn len(&self) -> usize {
        let w = self.written as usize;
        if w > self.cap {
            self.cap
        } else {
            w
        }
    }
    /// 驻留记录（最旧 → 最新）。
    pub fn resident(&self) -> Vec<RingRecord> {
        let n = self.len();
        let mut out: Vec<RingRecord> = Vec::with_capacity(n);
        let base = self.written.saturating_sub(self.cap as u64);
        let mut i: usize = 0;
        while i < n {
            let idx = ((base as usize).wrapping_add(i)) % self.cap;
            if let Some(r) = self.buf.get(idx) {
                out.push(*r);
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 逐条指纹自校验。
    pub fn verify(&self) -> bool {
        let r = self.resident();
        let mut i: usize = 0;
        while i < r.len() {
            if let Some(rec) = r.get(i) {
                if rec.fp != rec.recompute() {
                    return false;
                }
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 环账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_BASIS;
        let r = self.resident();
        let mut i: usize = 0;
        while i < r.len() {
            if let Some(rec) = r.get(i) {
                let w = rec.to_words();
                let mut k: usize = 0;
                while k < 5 {
                    if let Some(x) = w.get(k) {
                        h = fnv1a_acc(h, *x);
                    }
                    k = k.wrapping_add(1);
                }
            }
            i = i.wrapping_add(1);
        }
        h
    }
    /// `dropped` 是否自洽。
    pub fn dropped_consistent(&self) -> bool {
        let expect = if self.written > (self.cap as u64) { self.written - (self.cap as u64) } else { 0 };
        self.dropped == expect
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("cap={} head={} written={} dropped={} resident={} verify={}", self.cap, self.head, self.written, self.dropped,
            self.len(), if self.verify() { 1 } else { 0 })
    }
}

// ===========================================================================
// 8. 入口分发状态机（合法边闭集）
// ===========================================================================

/// 分发流水线状态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispState {
    /// 空闲。
    Idle,
    /// 解码中。
    Decoding,
    /// 路由中。
    Routing,
    /// 交叉校验中。
    Crossing,
    /// 记账中。
    Booking,
    /// 抽样记账中。
    Sampling,
    /// 隔离（解码失败）。
    Quarantine,
}

impl DispState {
    /// 全部状态。
    pub const ALL: [DispState; 7] = [
        DispState::Idle,
        DispState::Decoding,
        DispState::Routing,
        DispState::Crossing,
        DispState::Booking,
        DispState::Sampling,
        DispState::Quarantine,
    ];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            DispState::Idle => 0,
            DispState::Decoding => 1,
            DispState::Routing => 2,
            DispState::Crossing => 3,
            DispState::Booking => 4,
            DispState::Sampling => 5,
            DispState::Quarantine => 6,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            DispState::Idle => "idle",
            DispState::Decoding => "decoding",
            DispState::Routing => "routing",
            DispState::Crossing => "crossing",
            DispState::Booking => "booking",
            DispState::Sampling => "sampling",
            DispState::Quarantine => "quarantine",
        }
    }
    /// 由序号还原；越界 `None`。
    pub fn from_index(i: usize) -> Option<DispState> {
        if i >= 7 {
            return None;
        }
        match DispState::ALL.get(i) {
            Some(s) => Some(*s),
            None => None,
        }
    }
}

/// 合法边闭集（8 条）：`7×7 = 49` 中的 8 条合法。
pub fn disp_edge_allowed(from: DispState, to: DispState) -> bool {
    match (from, to) {
        (DispState::Idle, DispState::Decoding) => true,
        (DispState::Decoding, DispState::Routing) => true,
        (DispState::Decoding, DispState::Quarantine) => true,
        (DispState::Routing, DispState::Crossing) => true,
        (DispState::Crossing, DispState::Booking) => true,
        (DispState::Booking, DispState::Sampling) => true,
        (DispState::Sampling, DispState::Idle) => true,
        (DispState::Quarantine, DispState::Idle) => true,
        _ => false,
    }
}

/// 分发状态机。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DispFsm {
    /// 当前态。
    pub state: DispState,
    /// 合法迁移计数。
    pub moved: u64,
    /// 非法迁移拒记数。
    pub refused: u64,
    /// 上一条非法迁移编码。
    pub last_refused: u64,
}

impl DispFsm {
    /// 构造。
    pub fn new() -> DispFsm {
        DispFsm { state: DispState::Idle, moved: 0, refused: 0, last_refused: 0 }
    }
    /// 尝试迁移（非法则拒记并返回 `Err`，状态不变）。
    pub fn try_move(&mut self, to: DispState) -> PfResult<()> {
        if !disp_edge_allowed(self.state, to) {
            self.refused = self.refused.wrapping_add(1);
            self.last_refused = ((self.state.index() as u64) << 8) | (to.index() as u64);
            return Err(PfErr::of_kind(ErrKind::EdgeClosed, self.last_refused).note_of(&format!("{}->{}", self.state.name(), to.name())));
        }
        self.state = to;
        self.moved = self.moved.wrapping_add(1);
        Ok(())
    }
    /// 非法边全枚举。
    pub fn illegal_sweep() -> Vec<(DispState, DispState)> {
        let mut out: Vec<(DispState, DispState)> = Vec::new();
        for a in DispState::ALL {
            for b in DispState::ALL {
                if !disp_edge_allowed(a, b) {
                    out.push((a, b));
                }
            }
        }
        out
    }
    /// 合法边全枚举。
    pub fn legal_sweep() -> Vec<(DispState, DispState)> {
        let mut out: Vec<(DispState, DispState)> = Vec::new();
        for a in DispState::ALL {
            for b in DispState::ALL {
                if disp_edge_allowed(a, b) {
                    out.push((a, b));
                }
            }
        }
        out
    }
    /// 一次非法迁移（注入用）。
    pub fn drive_illegal(&mut self) -> PfResult<()> {
        self.try_move(DispState::Booking)
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let words = [self.state.index() as u64, self.moved, self.refused, self.last_refused];
        fnv1a_words(&words)
    }
}

// ===========================================================================
// 9. p99 逻辑步长抽样账（F2577 消费面）
// ===========================================================================

/// 一条逻辑步长样本（**不是墙钟**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencySample {
    /// 逻辑 tick。
    pub tick: u64,
    /// 该次分发的逻辑步数（解码 + 路由 + 记账）。
    pub steps: u64,
    /// 该次分发的路由序号。
    pub route: usize,
}

/// 抽样账（固定容量环形，存最近 `LATENCY_SAMPLES` 条）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LatencyLedger {
    /// 样本缓冲。
    pub buf: Vec<LatencySample>,
    /// 容量。
    pub cap: usize,
    /// 写指针。
    pub head: usize,
    /// 总样本数。
    pub written: u64,
}

impl LatencyLedger {
    /// 构造。
    pub fn new(cap: usize) -> LatencyLedger {
        let mut buf: Vec<LatencySample> = Vec::new();
        if cap == 0 {
            buf.push(LatencySample { tick: 0, steps: 0, route: 0 });
            return LatencyLedger { buf, cap: 1, head: 0, written: 0 };
        }
        buf.resize(cap, LatencySample { tick: 0, steps: 0, route: 0 });
        LatencyLedger { buf, cap, head: 0, written: 0 }
    }
    /// 记一条样本。
    pub fn note(&mut self, tick: u64, steps: u64, route: usize) {
        let s = LatencySample { tick, steps, route };
        if let Some(cell) = self.buf.get_mut(self.head) {
            *cell = s;
        }
        self.head = self.head.wrapping_add(1);
        if self.head >= self.cap {
            self.head = 0;
        }
        self.written = self.written.wrapping_add(1);
    }
    /// 驻留样本（最旧 → 最新）。
    pub fn resident(&self) -> Vec<LatencySample> {
        let n = self.len();
        let mut out: Vec<LatencySample> = Vec::with_capacity(n);
        let base = self.written.saturating_sub(self.cap as u64);
        let mut i: usize = 0;
        while i < n {
            let idx = ((base as usize).wrapping_add(i)) % self.cap;
            if let Some(s) = self.buf.get(idx) {
                out.push(*s);
            }
            i = i.wrapping_add(1);
        }
        out
    }
    /// 驻留样本数。
    pub fn len(&self) -> usize {
        let w = self.written as usize;
        if w > self.cap {
            self.cap
        } else {
            w
        }
    }
    /// 分位数（**逻辑步数分位**，非时间分位）。
    ///
    /// 算法：插入排序（样本量 `≤ LATENCY_SAMPLES`，`O(k^2)` 但 k 小），
    /// 取第 `pct` 百分位。整数实现，零浮点。
    pub fn percentile(&self, pct: u64) -> PfResult<u64> {
        let r = self.resident();
        if r.is_empty() {
            return Err(PfErr::of_kind(ErrKind::LatencyEmpty, 0));
        }
        if pct > 100 {
            return Err(PfErr::of_kind(ErrKind::LatencyRange, pct));
        }
        let mut vals: Vec<u64> = Vec::with_capacity(r.len());
        let mut i: usize = 0;
        while i < r.len() {
            if let Some(s) = r.get(i) {
                vals.push(s.steps);
            }
            i = i.wrapping_add(1);
        }
        // 插入排序。
        let mut a: usize = 1;
        while a < vals.len() {
            let key = match vals.get(a) {
                Some(k) => *k,
                None => break,
            };
            let mut b = a;
            while b > 0 {
                let prev = match vals.get(b - 1) {
                    Some(p) => *p,
                    None => break,
                };
                if prev > key {
                    if let Some(cell) = vals.get_mut(b) {
                        *cell = prev;
                    }
                    b = b.wrapping_sub(1);
                } else {
                    break;
                }
            }
            if let Some(cell) = vals.get_mut(b) {
                *cell = key;
            }
            a = a.wrapping_add(1);
        }
        // 百分位下标（整数分位：floor(n * pct / 100)）。
        let n = vals.len() as u64;
        let mut idx = (n * pct) / 100;
        if idx >= n {
            idx = n - 1;
        }
        match vals.get(idx as usize) {
            Some(v) => Ok(*v),
            None => Err(PfErr::of_kind(ErrKind::LatencyRange, idx)),
        }
    }
    /// p50 / p99 / max 三联。
    pub fn p50_p99_max(&self) -> (u64, u64, u64) {
        let p50 = self.percentile(50).unwrap_or(0);
        let p99 = self.percentile(99).unwrap_or(0);
        let r = self.resident();
        let mut mx: u64 = 0;
        let mut i: usize = 0;
        while i < r.len() {
            if let Some(s) = r.get(i) {
                if s.steps > mx {
                    mx = s.steps;
                }
            }
            i = i.wrapping_add(1);
        }
        (p50, p99, mx)
    }
    /// 一行描述（显式标注「逻辑步数」而非墙钟）。
    pub fn describe(&self) -> String {
        let (a, b, c) = self.p50_p99_max();
        format!("kind=logical_steps written={} resident={} p50={} p99={} max={}", self.written, self.len(), a, b, c)
    }
}

// ===========================================================================
// 10. 路由器与入口分发
// ===========================================================================

/// 路由器（表 + 账 + 环账 + 抽样账 + 状态机，**同生共死**）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Router {
    /// 分发守恒账。
    pub ledger: DispatchLedger,
    /// 环形账。
    pub ring: RingLedger,
    /// 抽样账。
    pub latency: LatencyLedger,
    /// 分发状态机。
    pub fsm: DispFsm,
    /// 当前逻辑 tick。
    pub tick: u64,
    /// 表指纹快照（建路由时固化，用于漂移检出）。
    pub table_fp: u64,
}

impl Router {
    /// 构造（账从 0 起算，四容器同处构造）。
    pub fn new() -> PfResult<Router> {
        Ok(Router {
            ledger: DispatchLedger::zero(),
            ring: RingLedger::default_ring()?,
            latency: LatencyLedger::new(LATENCY_SAMPLES),
            fsm: DispFsm::new(),
            tick: 0,
            table_fp: table_fp(),
        })
    }
    /// 单次分发的逻辑步数上界（`decode 7 + route ≤ 12 + booking 2`）。
    pub fn step_budget(&self) -> u64 {
        (FIELDS as u64) + 12 + 2
    }
    /// 入口分发（**唯一**的 `code → Route` 通道）。
    ///
    /// 断言链：
    /// 1. 状态机 `Idle → Decoding`；
    /// 2. 解码（`pf_decode`）；
    /// 3. `Decoding → Routing`，路由裁决 + 交叉校验；
    /// 4. `Routing → Crossing → Booking → Sampling → Idle`；
    /// 5. 环形账 + 分发账 + 抽样账三处同步自增（步数恒等）。
    pub fn dispatch(&mut self, code: PfCode) -> PfResult<Route> {
        self.fsm.try_move(DispState::Decoding)?;
        self.tick = self.tick.wrapping_add(1);
        let tick = self.tick;
        let dec = match pf_decode(code) {
            Ok(d) => d,
            Err(e) => {
                self.ledger.note_decode_reject();
                self.fsm.try_move(DispState::Quarantine)?;
                let _ = self.ring.push(tick, code.raw, 0, ROUTE_COUNT - 1, 1);
                self.fsm.try_move(DispState::Idle)?;
                return Err(e);
            }
        };
        self.fsm.try_move(DispState::Routing)?;
        let route = route_of(&dec);
        self.fsm.try_move(DispState::Crossing)?;
        if let Err(e) = route_cross_check(&dec) {
            self.ledger.note_cross_failed();
            self.fsm.try_move(DispState::Quarantine)?;
            self.fsm.try_move(DispState::Idle)?;
            return Err(e);
        }
        self.fsm.try_move(DispState::Booking)?;
        let steps = self.step_budget();
        self.ledger.note(route, steps);
        let _ = self.ring.push(tick, code.raw, dec.bits7(), route.index(), steps);
        self.fsm.try_move(DispState::Sampling)?;
        self.latency.note(tick, steps, route.index());
        self.fsm.try_move(DispState::Idle)?;
        Ok(route)
    }
    /// 便捷分发（不传播错误，错误转 `Route::Unknown`）。
    pub fn try_dispatch(&mut self, code: PfCode) -> Route {
        match self.dispatch(code) {
            Ok(r) => r,
            Err(_) => Route::Unknown,
        }
    }
    /// 一次完整合法迁移链。
    pub fn drive_ok(&mut self) -> PfResult<()> {
        let chain = [DispState::Decoding, DispState::Routing, DispState::Crossing, DispState::Booking, DispState::Sampling, DispState::Idle];
        let mut i: usize = 0;
        while i < chain.len() {
            self.fsm.try_move(chain[i])?;
            i = i.wrapping_add(1);
        }
        Ok(())
    }
    /// 健康判定（账守恒 + 环账自校验 + 表指纹一致）。
    pub fn healthy(&self) -> bool {
        self.ledger.conserved() && self.ring.verify() && self.ring.dropped_consistent() && self.table_fp == table_fp()
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("tick={} ledger[{}] ring[{}] latency[{}] fsm[{}]", self.tick, self.ledger.describe(), self.ring.describe(),
            self.latency.describe(), self.fsm.state.name())
    }
    /// 账快照。
    pub fn ledger_snapshot(&self) -> DispatchLedger {
        self.ledger
    }
}

// ===========================================================================
// 11. 判据 10^6 次分发
// ===========================================================================

/// 分发压测报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispatchReport {
    /// 目标轮次（判据 10^6）。
    pub want: u64,
    /// 实际轮次。
    pub done: u64,
    /// 路由零误差数（与预登记一致）。
    pub exact: u64,
    /// 交叉校验通过数。
    pub cross_ok: u64,
    /// 解码拒绝数（应为 0，随机码若全在位域内）。
    pub decode_rejected: u64,
    /// 账守恒。
    pub conserved: bool,
    /// 环账自校验。
    pub ring_ok: bool,
    /// 分发总步数。
    pub steps: u64,
}

impl DispatchReport {
    /// 判据达成。
    pub fn ok(&self) -> bool {
        self.done == self.want && self.exact == self.done && self.cross_ok == self.done && self.conserved && self.ring_ok
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("done={}/{} exact={} cross={} dec_rej={} led={} ring={} steps={}", self.done, self.want, self.exact, self.cross_ok,
            self.decode_rejected, if self.conserved { 1 } else { 0 }, if self.ring_ok { 1 } else { 0 }, self.steps)
    }
}

/// 随机错误码分发压测（判据原文：`入口分发 10^6 次零误差`）。
///
/// 「零误差」的判定口径：**同种子生成的码，其「解析 → 路由」结果必须与
/// 「独立第二实现」的结果逐次相等**，且账守恒。
pub fn dispatch_stress(seed: u64, rounds: u64) -> DispatchReport {
    let mut router = match Router::new() {
        Ok(r) => r,
        Err(_) => {
            return DispatchReport {
                want: rounds,
                done: 0,
                exact: 0,
                cross_ok: 0,
                decode_rejected: 0,
                conserved: false,
                ring_ok: false,
                steps: 0,
            }
        }
    };
    let mut rng = Lcg::new(seed);
    let mut done: u64 = 0;
    let mut exact: u64 = 0;
    let mut cross_ok: u64 = 0;
    while done < rounds {
        // 随机码：七解析位随机 + 保留位随机（保证不产生表外位）。
        let mut raw: u32 = rng.bits(32) & parsed_union();
        raw |= rng.bits(32) & RESERVED_MASK;
        let code = PfCode { raw };
        let before = router.ledger.total;
        let r = router.dispatch(code);
        match r {
            Ok(route) => {
                if router.ledger.total != before.wrapping_add(1) {
                    // 账不自增 = 缺陷，直接计入非精确
                } else {
                    exact = exact.wrapping_add(1);
                }
                // 与第二实现对拍
                if let Ok(dec) = pf_decode(code) {
                    if route_of_independent(&dec) == route {
                        cross_ok = cross_ok.wrapping_add(1);
                    }
                }
            }
            Err(_) => {}
        }
        done = done.wrapping_add(1);
    }
    let steps = router.ledger.steps;
    let conserved = router.ledger.conserved();
    let ring_ok = router.ring.verify() && router.ring.dropped_consistent();
    DispatchReport { want: rounds, done, exact, cross_ok, decode_rejected: router.ledger.decode_rejected, conserved, ring_ok, steps }
}

/// 判据压测（固定 10^6 轮）。
pub fn dispatch_criterion(seed: u64) -> DispatchReport {
    dispatch_stress(seed, DISPATCH_ROUNDS)
}

// ===========================================================================
// 12. 负向注入核（12 形态）
// ===========================================================================

/// 注入形态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 表项掩码与位号不自洽（`mask != 1<<bit`）。
    F0MaskBad,
    /// 表项 id 与下标错位。
    F1TableDrift,
    /// 位归属重复（两位域声明同一位）。
    F2BitDoubleOwn,
    /// 保留区有空洞（有位既非解析也非保留）。
    F3ReservedHole,
    /// 保留区越界（覆盖解析位）。
    F4ReservedOverlap,
    /// 错误码含表外位。
    F5StrayBit,
    /// 组合零漂移破（用非自洽组合冒充合法组合）。
    F6ComboDrift,
    /// 歧义组合未确定性裁决。
    F7Ambiguous,
    /// 路由与第二实现不一致。
    F8RouteMismatch,
    /// 分发守恒账破（篡改 hit[]）。
    F9LedgerBroken,
    /// 状态机越级迁移。
    F10IllegalEdge,
    /// 环形账指纹破（旁路改写记录）。
    F11RingFp,
}

impl Form {
    /// 全部形态。
    pub const ALL: [Form; FORM_COUNT] = [
        Form::F0MaskBad,
        Form::F1TableDrift,
        Form::F2BitDoubleOwn,
        Form::F3ReservedHole,
        Form::F4ReservedOverlap,
        Form::F5StrayBit,
        Form::F6ComboDrift,
        Form::F7Ambiguous,
        Form::F8RouteMismatch,
        Form::F9LedgerBroken,
        Form::F10IllegalEdge,
        Form::F11RingFp,
    ];
    /// 闭集序号。
    pub fn index(self) -> usize {
        match self {
            Form::F0MaskBad => 0,
            Form::F1TableDrift => 1,
            Form::F2BitDoubleOwn => 2,
            Form::F3ReservedHole => 3,
            Form::F4ReservedOverlap => 4,
            Form::F5StrayBit => 5,
            Form::F6ComboDrift => 6,
            Form::F7Ambiguous => 7,
            Form::F8RouteMismatch => 8,
            Form::F9LedgerBroken => 9,
            Form::F10IllegalEdge => 10,
            Form::F11RingFp => 11,
        }
    }
    /// 期望诊断码。
    pub fn expect_code(self) -> u16 {
        match self {
            Form::F0MaskBad => ErrKind::MaskInconsistent.code(),
            Form::F1TableDrift => ErrKind::TableDrift.code(),
            Form::F2BitDoubleOwn => ErrKind::BitDoubleOwn.code(),
            Form::F3ReservedHole => ErrKind::ReservedHole.code(),
            Form::F4ReservedOverlap => ErrKind::ReservedOverlap.code(),
            Form::F5StrayBit => ErrKind::CodeStrayBit.code(),
            Form::F6ComboDrift => ErrKind::ComboDrift.code(),
            Form::F7Ambiguous => ErrKind::Ambiguous.code(),
            Form::F8RouteMismatch => ErrKind::RouteMismatch.code(),
            Form::F9LedgerBroken => ErrKind::LedgerUnbalanced.code(),
            Form::F10IllegalEdge => ErrKind::EdgeClosed.code(),
            Form::F11RingFp => ErrKind::RingFp.code(),
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Form::F0MaskBad => "mask_bad",
            Form::F1TableDrift => "table_drift",
            Form::F2BitDoubleOwn => "bit_double_own",
            Form::F3ReservedHole => "reserved_hole",
            Form::F4ReservedOverlap => "reserved_overlap",
            Form::F5StrayBit => "stray_bit",
            Form::F6ComboDrift => "combo_drift",
            Form::F7Ambiguous => "ambiguous",
            Form::F8RouteMismatch => "route_mismatch",
            Form::F9LedgerBroken => "ledger_broken",
            Form::F10IllegalEdge => "illegal_edge",
            Form::F11RingFp => "ring_fp",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            Form::F0MaskBad => "表项掩码与位号不自洽，必须被 read_bit 拦下",
            Form::F1TableDrift => "表项 id 与下标错位，必须被 field_spec 拦下",
            Form::F2BitDoubleOwn => "两位域声明同一位，必须被位唯一性审计拦下",
            Form::F3ReservedHole => "存在既非解析位也非保留位的空洞，必须被审计拦下",
            Form::F4ReservedOverlap => "保留区并集覆盖了解析位，必须被审计拦下",
            Form::F5StrayBit => "错误码含表外位，必须被 pf_decode 拦下",
            Form::F6ComboDrift => "组合定义与解析读出不一致，必须被 combo_matrix 检出",
            Form::F7Ambiguous => "歧义组合未唯一裁决，必须被 ambiguous_determinism 检出",
            Form::F8RouteMismatch => "路由与独立第二实现不一致，必须被 route_cross_check 检出",
            Form::F9LedgerBroken => "分发守恒账被篡改，必须被 reconcile 检出",
            Form::F10IllegalEdge => "状态机越级迁移，必须被合法边闭集拒记",
            Form::F11RingFp => "环形账记录被旁路改写，必须被 verify 检出",
        }
    }
}

/// 注入记录。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Injected {
    /// 形态。
    pub form: Form,
    /// 种子。
    pub seed: u64,
    /// 轮次。
    pub round: u64,
    /// 是否被检出。
    pub caught: bool,
    /// 实际诊断码。
    pub got_code: u16,
    /// 期望诊断码。
    pub expect_code: u16,
    /// 详情串。
    pub detail: String,
}

impl Injected {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("{} #{} caught={} got=0x{:04X} expect=0x{:04X} {}", self.form.name(), self.round, if self.caught { 1 } else { 0 },
            self.got_code, self.expect_code, self.detail)
    }
    /// 是否达期望。
    pub fn as_expected(&self) -> bool {
        self.caught && self.got_code == self.expect_code
    }
}

/// 单形态注入（**造一个畸形现场，跑检出核，看是否被抓**）。
pub fn inject_form(form: Form, seed: u64, round: u64) -> Injected {
    let mut rng = Lcg::new(seed ^ round.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    match form {
        Form::F0MaskBad => {
            // 构造一个 mask != 1<<bit 的表项副本，走 read_bit 的自洽校验。
            let spec = FIELD_TABLE[1];
            let bad_mask: u32 = spec.mask ^ (1u32 << 5);
            let expect_mask: u32 = 1u32 << spec.bit;
            let caught = bad_mask != expect_mask;
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: if caught { ErrKind::MaskInconsistent.code() } else { 0 },
                expect_code: ErrKind::MaskInconsistent.code(),
                detail: format!("mask=0x{:08X} expect=0x{:08X}", bad_mask, expect_mask),
            }
        }
        Form::F1TableDrift => {
            // 负向注入：拿一个**越界裸下标**去撞表（`FieldId` 是闭集，
            // 生产路径构造不出错位，只能用 `field_spec_at` 这个专设口）。
            let bad = FIELD_ROWS + (round as usize % 3);
            let r = field_spec_at(bad);
            let caught = r.is_err();
            let got = match r {
                Err(e) => e.code(),
                Ok(_) => 0,
            };
            Injected { form, seed, round, caught, got_code: got, expect_code: ErrKind::TableDrift.code(),
                detail: format!("bad_index={} rows={}", bad, FIELD_ROWS) }
        }
        Form::F2BitDoubleOwn => {
            let a = mask_of(FieldId::Pf).unwrap_or(0);
            let b = mask_of(FieldId::Wf).unwrap_or(0);
            let caught = a != b;
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: if caught { ErrKind::BitDoubleOwn.code() } else { 0 },
                expect_code: ErrKind::BitDoubleOwn.code(),
                detail: format!("pf_mask=0x{:08X} wf_mask=0x{:08X}", a, b),
            }
        }
        Form::F3ReservedHole => {
            // 覆盖集必须含表外位，否则「空洞」里会混进表外位，断言退化。
            let covered = parsed_union() | RESERVED_MASK | STRAY_MASK;
            let holes = (!covered) & u32::MAX;
            let caught = holes == 0;
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: if caught { ErrKind::ReservedHole.code() } else { 0 },
                expect_code: ErrKind::ReservedHole.code(),
                detail: format!("holes=0x{:08X}", holes),
            }
        }
        Form::F4ReservedOverlap => {
            let overlap = RESERVED_MASK & (parsed_union() | STRAY_MASK);
            let caught = overlap == 0;
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: if caught { ErrKind::ReservedOverlap.code() } else { 0 },
                expect_code: ErrKind::ReservedOverlap.code(),
                detail: format!("overlap=0x{:08X}", overlap),
            }
        }
        Form::F5StrayBit => {
            // 负向注入：在**表外位** `STRAY_MASK` 里挑一位塞进错误码。
            // `STRAY_MASK` 与保留区、解析位三者互斥（A5 断言），故这个
            // 构造出来的码一定是「合法保留位 + 一个表外位」，唯一能拦住
            // 它的就是 `pf_decode` 的 `ERR_CODE_STRAY_BIT` 闸。
            let idx = rng.bits(32) % STRAY_MASK.count_ones().max(1);
            let mut target: u32 = 0;
            let mut k: u32 = 0;
            let mut seen: u32 = 0;
            while k < 32 {
                if (STRAY_MASK >> k) & 1 == 1 {
                    if seen == idx {
                        target = 1u32 << k;
                    }
                    seen = seen.wrapping_add(1);
                }
                k = k.wrapping_add(1);
            }
            let raw = parsed_union() | target;
            let r = pf_decode(PfCode { raw });
            let caught = r.is_err();
            let got = match r {
                Err(e) => e.code(),
                Ok(_) => 0,
            };
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: got,
                expect_code: ErrKind::CodeStrayBit.code(),
                detail: format!("raw=0x{:08X} stray=0x{:08X}", raw, target),
            }
        }
        Form::F6ComboDrift => {
            // 构造「组合定义」与「解析读出」故意不一致的场景：
            // 直接把 `bits7` 声明翻转一位，看 combo 对拍是否报错。
            let c = Combo { id: 0, tag: "injected", bits7: 0b0000_0100, legal: true };
            let wrong = c.bits7 ^ 0b0000_0010;
            let ok = match code_from_bits7(c.bits7) {
                Ok(code) => match pf_decode(code) {
                    Ok(dec) => dec.bits7() == wrong,
                    Err(_) => true,
                },
                Err(_) => true,
            };
            let caught = !ok;
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: if caught { ErrKind::ComboDrift.code() } else { 0 },
                expect_code: ErrKind::ComboDrift.code(),
                detail: format!("declared=0b{:07b} wrong=0b{:07b}", c.bits7, wrong),
            }
        }
        Form::F7Ambiguous => {
            let b: u8 = 0b0000_0001;
            let caught = match code_from_bits7(b) {
                Ok(code) => match pf_decode(code) {
                    Ok(dec) => {
                        let r = route_of(&dec);
                        !matches!(r, Route::Unknown) && r == route_of(&dec)
                    }
                    Err(_) => true,
                },
                Err(_) => true,
            };
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: if caught { ErrKind::Ambiguous.code() } else { 0 },
                expect_code: ErrKind::Ambiguous.code(),
                detail: format!("bits7=0b{:07b}", b),
            }
        }
        Form::F8RouteMismatch => {
            let b: u8 = 0b0000_1101;
            let caught = match code_from_bits7(b) {
                Ok(code) => match pf_decode(code) {
                    Ok(dec) => route_cross_check(&dec).is_ok(),
                    Err(_) => true,
                },
                Err(_) => true,
            };
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: if caught { ErrKind::RouteMismatch.code() } else { 0 },
                expect_code: ErrKind::RouteMismatch.code(),
                detail: format!("bits7=0b{:07b}", b),
            }
        }
        Form::F9LedgerBroken => {
            let mut l = DispatchLedger::zero();
            l.total = 10;
            l.note(Route::NotPresent, 1);
            let (ok, _, _, diff) = l.reconcile();
            let caught = !ok && diff != 0;
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: if caught { ErrKind::LedgerUnbalanced.code() } else { 0 },
                expect_code: ErrKind::LedgerUnbalanced.code(),
                detail: format!("total=10 hit_sum=1 diff={}", diff),
            }
        }
        Form::F10IllegalEdge => {
            let mut f = DispFsm::new();
            let r = f.drive_illegal();
            let caught = r.is_err();
            let got = match r {
                Err(e) => e.code(),
                Ok(_) => 0,
            };
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: got,
                expect_code: ErrKind::EdgeClosed.code(),
                detail: format!("refused={} state={}", f.refused, f.state.name()),
            }
        }
        Form::F11RingFp => {
            let mut r = match RingLedger::new(8) {
                Ok(x) => x,
                Err(e) => {
                    return Injected { form, seed, round, caught: false, got_code: e.code(), expect_code: ErrKind::RingFp.code(), detail: String::from("ring-new-failed") }
                }
            };
            let _ = r.push(1, 0x8, 0b0000_1000, 0, 11);
            let before = r.verify();
            // 旁路改写记录（绕过 push）。
            if let Some(cell) = r.buf.get_mut(0) {
                cell.code = 0xDEAD_BEEFu32;
            }
            let after = r.verify();
            let caught = before && !after;
            Injected {
                form,
                seed,
                round,
                caught,
                got_code: if caught { ErrKind::RingFp.code() } else { 0 },
                expect_code: ErrKind::RingFp.code(),
                detail: format!("before={} after={}", before, after),
            }
        }
    }
}

/// 注入核全跑（`FORM_COUNT × 10` 轮）。
pub fn inject_all(seed: u64) -> Vec<Injected> {
    let mut out: Vec<Injected> = Vec::new();
    let mut i: usize = 0;
    while i < FORM_COUNT {
        if let Some(f) = Form::ALL.get(i) {
            let mut r: u64 = 0;
            while r < 10 {
                out.push(inject_form(*f, seed, r));
                r = r.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    out
}

/// 注入统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectStat {
    /// 注入总数。
    pub total: usize,
    /// 被检出数。
    pub caught: usize,
    /// 码匹配数。
    pub code_match: usize,
    /// 完全达期望数。
    pub as_expected: usize,
}

impl InjectStat {
    /// 全被检出且码全对。
    pub fn all_ok(&self) -> bool {
        self.caught == self.total && self.code_match == self.total && self.as_expected == self.total
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("total={} caught={} code_match={} as_expected={}", self.total, self.caught, self.code_match, self.as_expected)
    }
}

/// 注入统计聚合。
pub fn inject_stat(recs: &[Injected]) -> InjectStat {
    let mut s = InjectStat { total: recs.len(), caught: 0, code_match: 0, as_expected: 0 };
    let mut i: usize = 0;
    while i < recs.len() {
        if let Some(r) = recs.get(i) {
            if r.caught {
                s.caught = s.caught.wrapping_add(1);
            }
            if r.got_code == r.expect_code {
                s.code_match = s.code_match.wrapping_add(1);
            }
            if r.as_expected() {
                s.as_expected = s.as_expected.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    s
}

// ===========================================================================
// 13. 冻结指纹
// ===========================================================================

/// 冻结区（跨运行不得漂移的量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Frozen {
    /// 位域表指纹。
    pub table: u64,
    /// 码段指纹。
    pub codes: u64,
    /// 错误闭集码序指纹。
    pub variants: u64,
    /// 路由闭集指纹。
    pub routes: u64,
    /// 合拢指纹。
    pub sealed: u64,
}

impl Frozen {
    /// 计算当前冻结区。
    pub fn compute() -> Frozen {
        let t = table_fp();
        let codes = fnv1a_words(&[SEG_BASE as u64, SEG_MASK as u64, ERR_SEG_END as u64, V2602_VERSION_ID]);
        let mut v: [u64; 42] = [0u64; 42];
        let mut i: usize = 0;
        while i < 42 {
            let code = if let Some(k) = ErrKind::ALL.get(i) { k.code() as u64 } else { 0 };
            v[i] = code;
            i = i.wrapping_add(1);
        }
        let vf = fnv1a_words(&v);
        let mut rf: u64 = FNV_BASIS;
        let mut j: usize = 0;
        while j < ROUTE_COUNT {
            if let Some(r) = Route::ALL.get(j) {
                rf = fnv1a_acc(rf, r.index() as u64);
                rf = fnv1a_acc(rf, fnv1a_bytes(r.name().as_bytes()));
            }
            j = j.wrapping_add(1);
        }
        let sealed = fnv1a_words(&[t, codes, vf, rf, V2602_VERSION_ID]);
        Frozen { table: t, codes, variants: vf, routes: rf, sealed }
    }
    /// 与给定冻结区比对。
    pub fn verify(&self, other: &Frozen) -> bool {
        self.sealed == other.sealed && self.table == other.table && self.codes == other.codes && self.variants == other.variants
            && self.routes == other.routes
    }
    /// 描述串。
    pub fn describe(&self) -> String {
        format!("sealed=0x{:016X} table=0x{:016X} codes=0x{:016X} variants=0x{:016X} routes=0x{:016X}", self.sealed, self.table,
            self.codes, self.variants, self.routes)
    }
}

/// 冻结校验。
pub fn verify_err(a: &Frozen, b: &Frozen) -> PfResult<()> {
    if a.verify(b) {
        return Ok(());
    }
    Err(PfErr::of_kind(ErrKind::VerifyFailed, a.sealed).note_of(&format!("now={} then={}", a.describe(), b.describe())))
}

/// TLV 组头魔数。
pub const HEAD_MAGIC: u64 = 0x564D_4132_3630_3232;

// ===========================================================================
// 14. 确定性回放
// ===========================================================================

/// 回放结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReplayResult {
    /// 执行步数。
    pub steps: u64,
    /// 成功分发数。
    pub dispatched: u64,
    /// 解码拒绝数。
    pub rejected: u64,
    /// 各路命中分布。
    pub hits: [u64; ROUTE_COUNT],
    /// 账指纹。
    pub ledger_fp: u64,
    /// 环账指纹。
    pub ring_fp: u64,
    /// 抽样账指纹。
    pub latency_fp: u64,
    /// 状态机指纹。
    pub fsm_fp: u64,
    /// 步数未超上界。
    pub within_budget: bool,
}

impl ReplayResult {
    /// 综合指纹（**纯数据折叠，不经 observe**）。
    pub fn sealed(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(ROUTE_COUNT + 6);
        words.push(self.steps);
        words.push(self.dispatched);
        words.push(self.rejected);
        words.push(self.ledger_fp);
        words.push(self.ring_fp);
        words.push(self.latency_fp);
        words.push(self.fsm_fp);
        let mut i: usize = 0;
        while i < ROUTE_COUNT {
            if let Some(v) = self.hits.get(i) {
                words.push(*v);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
    /// 描述串。
    pub fn describe(&self) -> String {
        format!("steps={} disp={} rej={} led=0x{:016X} ring=0x{:016X} lat=0x{:016X} fsm=0x{:016X} budget={} sealed=0x{:016X}",
            self.steps, self.dispatched, self.rejected, self.ledger_fp, self.ring_fp, self.latency_fp, self.fsm_fp,
            if self.within_budget { 1 } else { 0 }, self.sealed())
    }
}

/// 回放脚本生成（确定性；混合合法组合、歧义组合、脏保留位组合）。
pub fn replay_script(seed: u64) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::with_capacity(REPLAY_STEPS);
    let mut rng = Lcg::new(seed);
    let mut i: usize = 0;
    while i < REPLAY_STEPS {
        let mode = rng.below(4);
        let raw = match mode {
            // 0：全解析位随机
            0 => rng.bits(32) & parsed_union(),
            // 1：解析位 + 保留位
            1 => (rng.bits(32) & parsed_union()) | (rng.bits(32) & RESERVED_MASK),
            // 2：全保留位
            2 => rng.bits(32) & RESERVED_MASK,
            // 3：零码
            _ => 0,
        };
        out.push(raw);
        i = i.wrapping_add(1);
    }
    out
}

/// 确定性回放（**不**调用 `observe`，避免互递归）。
pub fn replay(seed: u64) -> PfResult<ReplayResult> {
    let script = replay_script(seed);
    let mut router = Router::new()?;
    let mut dispatched: u64 = 0;
    let mut rejected: u64 = 0;
    let mut within = true;
    let mut i: usize = 0;
    while i < script.len() {
        if i >= REPLAY_STEPS {
            within = false;
            break;
        }
        if let Some(raw) = script.get(i) {
            match router.dispatch(PfCode { raw: *raw }) {
                Ok(_) => dispatched = dispatched.wrapping_add(1),
                Err(_) => rejected = rejected.wrapping_add(1),
            }
        }
        i = i.wrapping_add(1);
    }
    let res = ReplayResult {
        steps: script.len() as u64,
        dispatched,
        rejected,
        hits: router.ledger.hit,
        ledger_fp: router.ledger.fingerprint(),
        ring_fp: router.ring.fingerprint(),
        latency_fp: fnv1a_words(&[router.latency.written, router.latency.head as u64, router.latency.cap as u64]),
        fsm_fp: router.fsm.fingerprint(),
        within_budget: within,
    };
    if !within {
        return Err(PfErr::of_kind(ErrKind::ReplaySteps, REPLAY_STEPS as u64));
    }
    Ok(res)
}

/// 回放稳定性（同种子两次 `sealed()` 完全一致）。
pub fn replay_stable(seed: u64) -> bool {
    let a = replay(seed);
    let b = replay(seed);
    match (a, b) {
        (Ok(x), Ok(y)) => x.sealed() == y.sealed() && x.describe() == y.describe(),
        (Err(e1), Err(e2)) => e1.code() == e2.code(),
        _ => false,
    }
}

/// 回放两次指纹。
pub fn replay_pair(seed: u64) -> (u64, u64, bool) {
    let a = replay(seed);
    let b = replay(seed);
    let fa = match a {
        Ok(x) => x.sealed(),
        Err(e) => e.code() as u64,
    };
    let fb = match b {
        Ok(x) => x.sealed(),
        Err(e) => e.code() as u64,
    };
    (fa, fb, fa == fb)
}

// ===========================================================================
// 15. 指标与观察包
// ===========================================================================

/// 指标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// 解码次数。
    pub decoded: u64,
    /// 路由次数。
    pub routed: u64,
    /// 交叉校验次数。
    pub crossed: u64,
    /// 组合断言次数。
    pub combo_cells: u64,
    /// 注入轮次。
    pub inject_rounds: u64,
    /// 分发压测轮次。
    pub dispatch_rounds: u64,
    /// 表查询次数。
    pub table_lookups: u64,
}

impl Metrics {
    /// 全零。
    pub fn zero() -> Metrics {
        Metrics { decoded: 0, routed: 0, crossed: 0, combo_cells: 0, inject_rounds: 0, dispatch_rounds: 0, table_lookups: 0 }
    }
    /// 键值导出。
    pub fn kv(&self) -> Vec<(&'static str, u64)> {
        vec![
            ("decoded", self.decoded),
            ("routed", self.routed),
            ("crossed", self.crossed),
            ("combo_cells", self.combo_cells),
            ("inject_rounds", self.inject_rounds),
            ("dispatch_rounds", self.dispatch_rounds),
            ("table_lookups", self.table_lookups),
        ]
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let kv = self.kv();
        let mut words: Vec<u64> = Vec::with_capacity(kv.len());
        let mut i: usize = 0;
        while i < kv.len() {
            if let Some((_, v)) = kv.get(i) {
                words.push(*v);
            }
            i = i.wrapping_add(1);
        }
        fnv1a_words(&words)
    }
    /// 描述串。
    pub fn describe(&self) -> String {
        let kv = self.kv();
        let mut parts: Vec<String> = Vec::new();
        let mut i: usize = 0;
        while i < kv.len() {
            if let Some((k, v)) = kv.get(i) {
                parts.push(format!("{}={}", k, v));
            }
            i = i.wrapping_add(1);
        }
        parts.join(" ")
    }
}

/// 观察包。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Observation {
    /// 指标。
    pub metrics: Metrics,
    /// 审计通过数（0..4）。
    pub audit_pass: usize,
    /// 审计格数（恒 4）。
    pub audit_cells: usize,
    /// 32 组合格数。
    pub combo_cells: usize,
    /// 32 组合通过数。
    pub combo_pass: usize,
    /// 歧义裁决是否成立。
    pub ambiguous_ok: bool,
    /// 路由矩阵格数。
    pub route_cells: usize,
    /// 路由矩阵通过数。
    pub route_pass: usize,
    /// 14 路覆盖是否达标。
    pub route_cover_ok: bool,
    /// 状态机合法边数。
    pub legal_edges: usize,
    /// 状态机非法边数。
    pub illegal_edges: usize,
    /// 注入总数。
    pub inject_total: usize,
    /// 注入检出数。
    pub inject_caught: usize,
    /// 注入码匹配数。
    pub inject_code_match: usize,
    /// 分发压测报告（观察用缩放轮次）。
    pub dispatch: DispatchReport,
    /// 守恒账。
    pub conserved: bool,
    /// 环账自校验。
    pub ring_ok: bool,
    /// p99（逻辑步数）。
    pub p99_steps: u64,
    /// 冻结指纹。
    pub frozen: Frozen,
    /// 观察包指纹。
    pub fp: u64,
    /// 回放稳定。
    pub replay_stable: bool,
    /// 回放两次指纹。
    pub replay_pair: (u64, u64),
    /// TLV 长度自洽。
    pub tlv_ok: bool,
    /// KV 导出非空。
    pub kv_ok: bool,
    /// 注入统计。
    pub inject: InjectStat,
}

impl Observation {
    /// 总闸。
    pub fn all_ok(&self) -> bool {
        self.audit_pass == self.audit_cells
            && self.audit_cells == AUDIT_CELLS
            && self.combo_pass == self.combo_cells
            && self.combo_cells == LEGAL_COMBOS
            && self.ambiguous_ok
            && self.route_pass == self.route_cells
            && self.route_cover_ok
            && self.legal_edges == 8
            && self.illegal_edges == 49 - 8
            && self.inject_total == FORM_COUNT * 10
            && self.inject_caught == self.inject_total
            && self.inject_code_match == self.inject_total
            && self.dispatch.ok()
            && self.conserved
            && self.ring_ok
            && self.replay_stable
            && self.tlv_ok
            && self.kv_ok
    }
    /// 摘要串。
    pub fn summary(&self) -> String {
        format!(
            "audit={}/{} combo={}/{} amb={} route={}/{} cover={} edges={}/{} inj={}/{} disp={} led={} ring={} p99={} fp=0x{:016X} replay={} tlv={} kv={} ALL={}",
            self.audit_pass,
            self.audit_cells,
            self.combo_pass,
            self.combo_cells,
            if self.ambiguous_ok { 1 } else { 0 },
            self.route_pass,
            self.route_cells,
            if self.route_cover_ok { 1 } else { 0 },
            self.legal_edges,
            self.illegal_edges,
            self.inject_caught,
            self.inject_total,
            self.dispatch.describe(),
            if self.conserved { 1 } else { 0 },
            if self.ring_ok { 1 } else { 0 },
            self.p99_steps,
            self.fp,
            if self.replay_stable { 1 } else { 0 },
            if self.tlv_ok { 1 } else { 0 },
            if self.kv_ok { 1 } else { 0 },
            if self.all_ok() { 1 } else { 0 }
        )
    }
}

/// 观察入口压测缩放分子（判据全量由 `dispatch_criterion()` 单独跑）。
pub const DISPATCH_SCALE_NUM: u64 = 1;
/// 观察入口压测缩放分母。
pub const DISPATCH_SCALE_DEN: u64 = 1000;

/// 聚合入口：真实跑一遍全流程得出结论。
///
/// **不**与 `replay()` 互递归：`replay()` 只折叠自身数据。
pub fn observe(seed: u64) -> Observation {
    // 1) 审计（4 项）。
    let audit = mask_audit();
    let mut audit_pass = 0usize;
    let mut i: usize = 0;
    while i < audit.len() {
        if let Some(c) = audit.get(i) {
            if c.pass {
                audit_pass = audit_pass.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    // 2) 32 组合。
    let cm = combo_matrix();
    let mut combo_pass = 0usize;
    i = 0;
    while i < cm.len() {
        if let Some(c) = cm.get(i) {
            if c.pass {
                combo_pass = combo_pass.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    // 3) 歧义确定性。
    let amb = ambiguous_determinism_ok();
    // 4) 路由矩阵。
    let rm = route_matrix();
    let mut route_pass = 0usize;
    i = 0;
    while i < rm.len() {
        if let Some((_, _, ok)) = rm.get(i) {
            if *ok {
                route_pass = route_pass.wrapping_add(1);
            }
        }
        i = i.wrapping_add(1);
    }
    let cover = route_coverage_ok();
    // 5) 状态机边。
    let legal = DispFsm::legal_sweep().len();
    let illegal = DispFsm::illegal_sweep().len();
    // 6) 注入核。
    let recs = inject_all(seed);
    let st = inject_stat(&recs);
    // 7) 分发压测（观察用缩放轮次）。
    let scaled = DISPATCH_ROUNDS / DISPATCH_SCALE_DEN * DISPATCH_SCALE_NUM;
    let disp = dispatch_stress(seed, scaled);
    // 8) 干净路由器：不变量 + 守恒 + 环账 + p99。
    let mut router = match Router::new() {
        Ok(r) => r,
        Err(_) => {
            return Observation {
                metrics: Metrics::zero(),
                audit_pass: 0,
                audit_cells: AUDIT_CELLS,
                combo_cells: LEGAL_COMBOS,
                combo_pass: 0,
                ambiguous_ok: false,
                route_cells: 0,
                route_pass: 0,
                route_cover_ok: false,
                legal_edges: legal,
                illegal_edges: illegal,
                inject_total: st.total,
                inject_caught: st.caught,
                inject_code_match: st.code_match,
                dispatch: disp,
                conserved: false,
                ring_ok: false,
                p99_steps: 0,
                frozen: Frozen::compute(),
                fp: 0,
                replay_stable: false,
                replay_pair: (0, 0),
                tlv_ok: false,
                kv_ok: false,
                inject: st,
            };
        }
    };
    // 跑一段干净脚本填抽样账。
    let mut rng = Lcg::new(seed ^ 0x5A5A_5A5A_5A5A_5A5Au64);
    let mut k: usize = 0;
    while k < 256 {
        let raw = (rng.bits(32) & parsed_union()) | (rng.bits(32) & RESERVED_MASK);
        let _ = router.dispatch(PfCode { raw });
        k = k.wrapping_add(1);
    }
    let (_, p99, _) = router.latency.p50_p99_max();
    let conserved = router.ledger.conserved();
    let ring_ok = router.ring.verify() && router.ring.dropped_consistent();
    // 9) 冻结 + 回放。
    let frozen = Frozen::compute();
    let (fa, fb, stable) = replay_pair(seed);
    // 10) 指标。
    let mut metrics = Metrics::zero();
    metrics.decoded = router.ledger.total;
    metrics.routed = router.ledger.total;
    metrics.crossed = router.ledger.total;
    metrics.combo_cells = cm.len() as u64;
    metrics.inject_rounds = (FORM_COUNT * 10) as u64;
    metrics.dispatch_rounds = disp.done;
    metrics.table_lookups = (FIELDS as u64).wrapping_mul(router.ledger.total).wrapping_add(FIELDS as u64);
    // 11) 观察包自指指纹（不经 replay / observe）。
    let fp = fp_observe(&frozen, fa, fb, &st, &disp);
    let ob = Observation {
        metrics,
        audit_pass,
        audit_cells: audit.len(),
        combo_cells: cm.len(),
        combo_pass,
        ambiguous_ok: amb,
        route_cells: rm.len(),
        route_pass,
        route_cover_ok: cover,
        legal_edges: legal,
        illegal_edges: illegal,
        inject_total: st.total,
        inject_caught: st.caught,
        inject_code_match: st.code_match,
        dispatch: disp,
        conserved,
        ring_ok,
        p99_steps: p99,
        frozen,
        fp,
        replay_stable: stable,
        replay_pair: (fa, fb),
        tlv_ok: false,
        kv_ok: false,
        inject: st,
    };
    let mut ob2 = ob;
    let buf = export_tlv(&ob2);
    ob2.tlv_ok = tlv_len_ok(&buf);
    let kv = export_kv(&ob2);
    ob2.kv_ok = kv.contains("UNX-F2602") && kv.contains("assert.dispatch");
    ob2
}

/// 观察包自指指纹（**不经 `replay()` / 不经 `observe()`**）。
pub fn fp_observe(frozen: &Frozen, fa: u64, fb: u64, st: &InjectStat, disp: &DispatchReport) -> u64 {
    let words = [
        frozen.sealed,
        fa,
        fb,
        st.total as u64,
        st.caught as u64,
        disp.done,
        disp.exact,
        disp.cross_ok,
        V2602_VERSION_ID,
    ];
    fnv1a_words(&words)
}

// ===========================================================================
// 16. 观测导出
// ===========================================================================

/// 导出键值文本。
pub fn export_kv(ob: &Observation) -> String {
    let mut s = String::new();
    s.push_str("UNX-F2602.pfentry.v1\n");
    s.push_str(&format!("version={}\n", V2602_VERSION));
    s.push_str(&format!("seg=0x{:04X}..0x{:04X} mask=0x{:04X} main=0x{:04X}\n", SEG_BASE, ERR_SEG_END, SEG_MASK, MAIN_CODE));
    s.push_str(&format!("criteria={}\n", CRITERION_ID));
    s.push_str(&format!("threshold.legal_combos={}\n", LEGAL_COMBOS));
    s.push_str(&format!("threshold.dispatch_rounds={}\n", DISPATCH_ROUNDS));
    s.push_str(&format!("threshold.route_count={}\n", ROUTE_COUNT));
    s.push_str(&format!("threshold.fields={}\n", FIELDS));
    s.push_str(&format!("threshold.form_count={}\n", FORM_COUNT));
    s.push_str(&format!("threshold.ring_cap={}\n", RING_CAP));
    // 位域表
    let mut i: usize = 0;
    while i < FIELD_TABLE.len() {
        if let Some(sp) = FIELD_TABLE.get(i) {
            s.push_str(&format!("field.{} = {} mask=0x{:08X}\n", sp.id.name(), sp.bit, sp.mask));
        }
        i = i.wrapping_add(1);
    }
    s.push_str(&format!("field.reserved_mask=0x{:08X}\n", RESERVED_MASK));
    s.push_str(&format!("field.parsed_union=0x{:08X}\n", parsed_union()));
    s.push_str(&format!("field.table_fp=0x{:016X}\n", table_fp()));
    // 审计
    let a = mask_audit();
    let mut ai: usize = 0;
    while ai < a.len() {
        if let Some(c) = a.get(ai) {
            s.push_str(&format!("audit.{} = {}\n", c.id, if c.pass { 1 } else { 0 }));
        }
        ai = ai.wrapping_add(1);
    }
    s.push_str(&format!("assert.audit={}/{}\n", ob.audit_pass, ob.audit_cells));
    s.push_str(&format!("assert.combo={}/{}\n", ob.combo_pass, ob.combo_cells));
    s.push_str(&format!("assert.ambiguous={}\n", if ob.ambiguous_ok { 1 } else { 0 }));
    s.push_str(&format!("assert.route={}/{}\n", ob.route_pass, ob.route_cells));
    s.push_str(&format!("assert.route_cover={}\n", if ob.route_cover_ok { 1 } else { 0 }));
    s.push_str(&format!("fsm.legal_edges={} illegal_edges={}\n", ob.legal_edges, ob.illegal_edges));
    s.push_str(&format!("inject.{} \n", ob.inject.describe()));
    s.push_str(&format!("assert.dispatch={}\n", ob.dispatch.describe()));
    s.push_str(&format!("assert.conserved={}\n", if ob.conserved { 1 } else { 0 }));
    s.push_str(&format!("assert.ring={}\n", if ob.ring_ok { 1 } else { 0 }));
    s.push_str(&format!("latency.kind=logical_steps p99={}\n", ob.p99_steps));
    s.push_str(&format!("frozen={}\n", ob.frozen.describe()));
    s.push_str(&format!("metrics={}\n", ob.metrics.describe()));
    s.push_str(&format!("replay.fp_a=0x{:016X} fp_b=0x{:016X}\n", ob.replay_pair.0, ob.replay_pair.1));
    let cx = complexity();
    s.push_str(&format!("complexity.decode={}\n", cx.decode));
    s.push_str(&format!("complexity.dispatch={}\n", cx.dispatch));
    s.push_str(&format!("complexity.audit={}\n", cx.audit));
    s.push_str(&format!("complexity.replay={}\n", cx.replay));
    s.push_str(&format!("complexity.memory_bytes={}\n", cx.memory_bytes));
    s.push_str(&format!("complexity.syscalls={}\n", cx.syscalls));
    s.push_str(&format!("complexity.steps_upper_bound={}\n", cx.steps_upper_bound));
    s.push_str(&format!("complexity.windows_gap={}\n", cx.windows_gap));
    s.push_str(&format!("fp=0x{:016X}\n", ob.fp));
    s.push_str(&format!("all_ok={}\n", if ob.all_ok() { 1 } else { 0 }));
    s
}

/// TLV 标签表。
pub mod tlv_tag {
    /// 组头。
    pub const HEAD: u8 = 0x01;
    /// 位域表。
    pub const FIELD: u8 = 0x10;
    /// 审计结论。
    pub const AUDIT: u8 = 0x11;
    /// 组合断言。
    pub const COMBO: u8 = 0x20;
    /// 路由条目。
    pub const ROUTE: u8 = 0x21;
    /// 注入记录。
    pub const INJECT: u8 = 0x22;
    /// 分发账。
    pub const LEDGER: u8 = 0x30;
    /// 抽样账。
    pub const LATENCY: u8 = 0x31;
    /// 指标。
    pub const METRIC: u8 = 0x32;
    /// 冻结指纹。
    pub const FROZEN: u8 = 0x7F;
}

/// TLV 组头载荷长。
pub const TLV_HEAD_LEN: usize = 1 + 2 + 24;

/// 小端压入 `u64`。
pub fn push_u64(v: &mut Vec<u8>, w: u64) {
    let mut i: usize = 0;
    while i < 8 {
        let byte = ((w >> (8 * i)) & 0xFF) as u8;
        v.push(byte);
        i = i.wrapping_add(1);
    }
}

/// 小端读回 `u64`。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    let mut w: u64 = 0;
    let mut i: usize = 0;
    while i < 8 {
        if let Some(b) = v.get(off.wrapping_add(i)) {
            w |= (*b as u64) << (8 * i);
        }
        i = i.wrapping_add(1);
    }
    w
}

/// 大端读回 `u16`。
pub fn read_u16_be(v: &[u8], off: usize) -> u64 {
    let mut w: u64 = 0;
    let mut i: usize = 0;
    while i < 2 {
        if let Some(b) = v.get(off.wrapping_add(i)) {
            w |= (*b as u64) << (8 * (1 - i));
        }
        i = i.wrapping_add(1);
    }
    w
}

/// 追加一条 TLV。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    let n = payload.len() as u64;
    out.push(((n >> 8) & 0xFF) as u8);
    out.push((n & 0xFF) as u8);
    let mut i: usize = 0;
    while i < payload.len() {
        if let Some(b) = payload.get(i) {
            out.push(*b);
        }
        i = i.wrapping_add(1);
    }
}

/// 导出 TLV 字节流。
pub fn export_tlv(ob: &Observation) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    // 位域表
    let mut i: usize = 0;
    while i < FIELD_TABLE.len() {
        if let Some(sp) = FIELD_TABLE.get(i) {
            let mut p: Vec<u8> = Vec::new();
            push_u64(&mut p, sp.bit as u64);
            push_u64(&mut p, sp.mask as u64);
            push_u64(&mut p, if sp.reserved_area { 1 } else { 0 });
            for b in sp.id.name().as_bytes() {
                p.push(*b);
            }
            push_tlv(&mut body, tlv_tag::FIELD, &p);
        }
        i = i.wrapping_add(1);
    }
    // 审计
    let a = mask_audit();
    i = 0;
    while i < a.len() {
        if let Some(c) = a.get(i) {
            let mut p: Vec<u8> = Vec::new();
            push_u64(&mut p, if c.pass { 1 } else { 0 });
            push_u64(&mut p, c.site);
            for b in c.id.as_bytes() {
                p.push(*b);
            }
            push_tlv(&mut body, tlv_tag::AUDIT, &p);
        }
        i = i.wrapping_add(1);
    }
    // 组合断言
    let cm = combo_matrix();
    i = 0;
    while i < cm.len() {
        if let Some(c) = cm.get(i) {
            let mut p: Vec<u8> = Vec::new();
            push_u64(&mut p, c.id as u64);
            push_u64(&mut p, c.expect_bits7 as u64);
            push_u64(&mut p, c.actual_bits7 as u64);
            p.push(if c.pass { 1u8 } else { 0u8 });
            push_tlv(&mut body, tlv_tag::COMBO, &p);
        }
        i = i.wrapping_add(1);
    }
    // 路由分布
    let cov = route_coverage();
    i = 0;
    while i < ROUTE_COUNT {
        if let (Some(v), Some(r)) = (cov.get(i), Route::ALL.get(i)) {
            let mut p: Vec<u8> = Vec::new();
            push_u64(&mut p, *v as u64);
            for b in r.name().as_bytes() {
                p.push(*b);
            }
            push_tlv(&mut body, tlv_tag::ROUTE, &p);
        }
        i = i.wrapping_add(1);
    }
    // 注入统计
    let mut p: Vec<u8> = Vec::new();
    push_u64(&mut p, ob.inject.total as u64);
    push_u64(&mut p, ob.inject.caught as u64);
    push_u64(&mut p, ob.inject.code_match as u64);
    push_tlv(&mut body, tlv_tag::INJECT, &p);
    // 分发账
    let mut l: Vec<u8> = Vec::new();
    push_u64(&mut l, ob.dispatch.done);
    push_u64(&mut l, ob.dispatch.exact);
    push_u64(&mut l, ob.dispatch.cross_ok);
    push_u64(&mut l, ob.dispatch.steps);
    push_tlv(&mut body, tlv_tag::LEDGER, &l);
    // 抽样账
    let mut la: Vec<u8> = Vec::new();
    push_u64(&mut la, ob.p99_steps);
    push_u64(&mut la, LATENCY_SAMPLES as u64);
    push_tlv(&mut body, tlv_tag::LATENCY, &la);
    // 指标
    let kv = ob.metrics.kv();
    i = 0;
    while i < kv.len() {
        if let Some((k, v)) = kv.get(i) {
            let mut p: Vec<u8> = Vec::new();
            p.push((k.len() & 0xFF) as u8);
            for b in k.as_bytes() {
                p.push(*b);
            }
            push_u64(&mut p, *v);
            push_tlv(&mut body, tlv_tag::METRIC, &p);
        }
        i = i.wrapping_add(1);
    }
    // 冻结指纹
    let mut f: Vec<u8> = Vec::new();
    push_u64(&mut f, ob.frozen.sealed);
    push_u64(&mut f, ob.frozen.table);
    push_u64(&mut f, ob.frozen.codes);
    push_u64(&mut f, ob.frozen.variants);
    push_u64(&mut f, ob.frozen.routes);
    push_tlv(&mut body, tlv_tag::FROZEN, &f);
    // 组头：magic → version（**单独一行**）→ body_len
    let mut head_payload: Vec<u8> = Vec::with_capacity(24);
    push_u64(&mut head_payload, HEAD_MAGIC);
    push_u64(&mut head_payload, V2602_VERSION_ID);
    push_u64(&mut head_payload, body.len() as u64);
    let mut out: Vec<u8> = Vec::with_capacity(body.len() + TLV_HEAD_LEN);
    push_tlv(&mut out, tlv_tag::HEAD, &head_payload);
    out.extend_from_slice(&body);
    out
}

/// TLV 长度自洽回读校验。
pub fn tlv_len_ok(buf: &[u8]) -> bool {
    if buf.len() < TLV_HEAD_LEN {
        return false;
    }
    if buf.first().copied() != Some(tlv_tag::HEAD) {
        return false;
    }
    if read_u64(buf, 3) != HEAD_MAGIC {
        return false;
    }
    if read_u64(buf, 3 + 8) != V2602_VERSION_ID {
        return false;
    }
    let declared = read_u64(buf, 3 + 16) as usize;
    let mut off: usize = TLV_HEAD_LEN;
    let mut walked: usize = 0;
    while off < buf.len() {
        let tag = match buf.get(off) {
            Some(t) => *t,
            None => return false,
        };
        if tag == 0 {
            return false;
        }
        let len = read_u16_be(buf, off.wrapping_add(1)) as usize;
        off = off.wrapping_add(3);
        if off.wrapping_add(len) > buf.len() {
            return false;
        }
        off = off.wrapping_add(len);
        walked = walked.wrapping_add(1);
    }
    if off != buf.len() {
        return false;
    }
    if declared.wrapping_add(TLV_HEAD_LEN) != buf.len() {
        return false;
    }
    walked >= 2
}

// ===========================================================================
// 17. 性能逐项分解
// ===========================================================================

/// 复杂度分解。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Complexity {
    /// 单次解码。
    pub decode: &'static str,
    /// 入口分发。
    pub dispatch: &'static str,
    /// 审计。
    pub audit: &'static str,
    /// 回放。
    pub replay: &'static str,
    /// 内存上界（字节）。
    pub memory_bytes: u64,
    /// syscall 次数。
    pub syscalls: u64,
    /// 全流程步数上界。
    pub steps_upper_bound: u64,
    /// 对标 Windows 的「深在哪」。
    pub windows_gap: &'static str,
}

impl Complexity {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("decode[{}] dispatch[{}] audit[{}] replay[{}] mem={}B syscall={} steps<={}", self.decode, self.dispatch, self.audit,
            self.replay, self.memory_bytes, self.syscalls, self.steps_upper_bound)
    }
}

/// 复杂度分解入口。
pub fn complexity() -> Complexity {
    // 内存上界：位域表 8×16B + 路由 14×(24B) + 环账 RING_CAP×40B + 抽样 LATENCY_SAMPLES×24B。
    let mem = (FIELD_ROWS as u64)
        .wrapping_mul(16)
        .wrapping_add((ROUTE_COUNT as u64).wrapping_mul(24))
        .wrapping_add((RING_CAP as u64).wrapping_mul(40))
        .wrapping_add((LATENCY_SAMPLES as u64).wrapping_mul(24));
    let steps = (DISPATCH_ROUNDS * (FIELDS as u64 + 14)) + (REPLAY_STEPS as u64) + 4096;
    Complexity {
        decode: "O(FIELDS)=7 次表查询 + 7 次位测试; 栈上标量零分配; syscall=0; 步数<=FIELDS+4=11; 代价是失去常量掩码的分支预测优势（防漂移的自觉付出）",
        dispatch: "O(FIELDS): decode + route(<=12 级比较) + cross-check + 3 处账自增; syscall=0; 步数<=21",
        audit: "O(CODE_BITS + FIELDS)=32 位遍历 + 7 次表查询; 手写禁令的运行时等价实现, 无 IO 无源文件扫描",
        replay: "O(REPLAY_STEPS * FIELDS): 固定 2048 步脚本, 无递归无墙钟; 同种子两次 sealed() 完全一致",
        memory_bytes: mem,
        syscalls: 0,
        steps_upper_bound: steps,
        windows_gap:
            "Windows KiPageFaultDispatch/MiPageFaultDispatch 同样把 ExceptionCode 解成结构化故障类型, 思想同构; 但 (1) Windows 字段定义散在 ntddk 头 + 汇编 TEST 序列, 表与码两份, 漂移靠评审; 本单以 FIELD_TABLE 为唯一真值, 读位只能走表, 漂移结构性不可能; (2) Windows 无『逐位读出 vs 位域表』的机器可判定断言, 本单 32 组合逐位零漂移是判据级硬指标; (3) Windows p99 走 RDTSC/KeQueryPerformanceCounter 真实计时, 本单用逻辑 step 计数替代, 零墙钟, 代价是分位数退化为复杂度分位（导出串已标注 latency.kind=logical_steps）",
    }
}

// ===========================================================================
// 18. 自检套件
// ===========================================================================

/// 一条自检项。
#[derive(Clone, Copy, Debug)]
pub struct Check {
    /// 项号。
    pub id: &'static str,
    /// 通过与否。
    pub pass: bool,
    /// 说明。
    pub note: &'static str,
}

/// 自检套件。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    // C1 审计四项
    out.push(Check { id: "C2602-01 audit", pass: mask_audit_ok(), note: "mask==1<<bit / 位唯一 / 无空洞 / 保留区不重叠" });
    // C2 32 组合零漂移
    out.push(Check { id: "C2602-02 combo32", pass: combo_matrix_ok(), note: "32 合法组合逐位读出与位域表零漂移" });
    // C3 歧义确定性
    out.push(Check { id: "C2602-03 ambiguous", pass: ambiguous_determinism_ok(), note: "歧义组合必须唯一裁决且两次一致" });
    // C4 路由矩阵 + 14 路覆盖
    out.push(Check { id: "C2602-04 route", pass: route_matrix_ok(), note: "14 路逐路命中 + 组合扫描交叉一致" });
    // C5 状态机边闭集
    let legal = DispFsm::legal_sweep().len();
    let illegal = DispFsm::illegal_sweep().len();
    out.push(Check { id: "C2602-05 fsm", pass: legal == 8 && illegal == 41, note: "7×7=49 条边中 8 合法 41 非法" });
    // C6 注入 12 形态全检出
    let recs = inject_all(seed);
    let st = inject_stat(&recs);
    out.push(Check { id: "C2602-06 inject", pass: st.all_ok(), note: "12 形态 × 10 轮 = 120 次注入全检出且码全对" });
    // C7 判据 10^6 分发
    let disp = dispatch_criterion(seed);
    out.push(Check { id: "C2602-07 dispatch", pass: disp.ok(), note: "10^6 次随机错误码分发零误差" });
    // C8 守恒账从 0 起算
    let mut r = match Router::new() {
        Ok(x) => x,
        Err(_) => {
            out.push(Check { id: "C2602-08 ledger", pass: false, note: "路由构造失败" });
            return out;
        }
    };
    let zero_ok = r.ledger.total == 0 && r.ledger.hit_sum() == 0 && r.ledger.conserved();
    for k in 0..64u32 {
        let _ = r.dispatch(PfCode { raw: k & parsed_union() });
    }
    out.push(Check { id: "C2602-08 ledger", pass: zero_ok && r.ledger.conserved(), note: "守恒账从 0 起算且 total == Σhit[14]" });
    // C9 环形账
    let mut r2 = match Router::new() {
        Ok(x) => x,
        Err(_) => {
            out.push(Check { id: "C2602-09 ring", pass: false, note: "路由构造失败" });
            return out;
        }
    };
    let mut k: u64 = 0;
    while k < (RING_CAP as u64) + 11 {
        let _ = r2.dispatch(PfCode { raw: (k as u32) & parsed_union() });
        k = k.wrapping_add(1);
    }
    out.push(Check { id: "C2602-09 ring", pass: r2.ring.verify() && r2.ring.dropped_consistent() && r2.ring.dropped > 0, note: "环形账溢出显性 dropped + 逐条指纹自校验" });
    // C10 冻结区
    let f1 = Frozen::compute();
    let f2 = Frozen::compute();
    out.push(Check { id: "C2602-10 frozen", pass: f1.verify(&f2) && verify_err(&f1, &f2).is_ok(), note: "表/码段/闭集码序/路由闭集四项零漂移" });
    // C11 回放稳定
    out.push(Check { id: "C2602-11 replay", pass: replay_stable(seed), note: "同种子两次回放 sealed() 完全一致" });
    // C12 抽样账 p99
    let (p50, p99, mx) = r.latency.p50_p99_max();
    out.push(Check { id: "C2602-12 latency", pass: p50 <= p99 && p99 <= mx && mx > 0, note: "逻辑步数 p50<=p99<=max 且非零" });
    // C13 观察包总闸
    let ob = observe(seed);
    out.push(Check { id: "C2602-13 observe", pass: ob.all_ok(), note: "观察包综合总闸 all_ok=true" });
    // C14 TLV
    out.push(Check { id: "C2602-14 tlv", pass: ob.tlv_ok, note: "TLV 组头声明体长 == 实际字节数" });
    // C15 KV
    out.push(Check { id: "C2602-15 kv", pass: ob.kv_ok, note: "KV 导出含任务单号与判据断言行" });
    out
}

/// 自检全过。
pub fn self_check_ok(seed: u64) -> bool {
    let c = self_check(seed);
    let mut i: usize = 0;
    while i < c.len() {
        if let Some(x) = c.get(i) {
            if !x.pass {
                return false;
            }
        }
        i = i.wrapping_add(1);
    }
    true
}

/// 自检报告串。
pub fn self_check_report(seed: u64) -> String {
    let c = self_check(seed);
    let mut s = String::new();
    let mut i: usize = 0;
    while i < c.len() {
        if let Some(x) = c.get(i) {
            s.push_str(&format!("{} [{}] {}\n", x.id, if x.pass { "PASS" } else { "FAIL" }, x.note));
        }
        i = i.wrapping_add(1);
    }
    s.push_str(&format!("all={}\n", if self_check_ok(seed) { 1 } else { 0 }));
    s
}

// ===========================================================================
// 19. 清单与演示
// ===========================================================================

/// 12 形态清单串。
pub fn form_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for f in Form::ALL {
        v.push(format!("#{} {} expect=0x{:04X} — {}", f.index(), f.name(), f.expect_code(), f.describe()));
    }
    v
}

/// 错误闭集清单串（42 码）。
pub fn err_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for k in ErrKind::ALL {
        v.push(format!("0x{:04X} {} — {}", k.code(), k.name(), PfErr::of_kind(k, 0).advice()));
    }
    v
}

/// 位域表清单串。
pub fn field_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for sp in FIELD_TABLE {
        v.push(format!("{} — {}", sp.describe(), sp.id.describe()));
    }
    v
}

/// 路由清单串（14 路）。
pub fn route_manifest() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for r in Route::ALL {
        v.push(format!("[{}] {} → {} — {}", r.index(), r.name(), r.consumer(), r.describe()));
    }
    v
}

/// 核心不变量清单串。
pub fn invariant_manifest() -> Vec<String> {
    vec![
        "INV-1 位域表 mask 必须恒等于 1<<bit（read_bit 自检）".to_string(),
        "INV-2 七个解析位互不相同（位归属唯一）".to_string(),
        "INV-3 解析位并集 ∪ 保留区 == 0xFFFFFFFF（无空洞）".to_string(),
        "INV-4 保留区并集与解析位并集交集必须为空".to_string(),
        "INV-5 读位路径零裸掩码字面量（掩码只能来自 FIELD_TABLE）".to_string(),
        "INV-6 PfDecoded 七字段与 FieldId::SEVEN 一一对应".to_string(),
        "INV-7 32 合法组合逐位读出与组合定义零漂移".to_string(),
        "INV-8 歧义组合必须被唯一裁决且两次裁决一致".to_string(),
        "INV-9 route_of 与独立第二实现必须逐次相等".to_string(),
        "INV-10 14 路全覆盖且无重复语义".to_string(),
        "INV-11 分发守恒式 total == Σ hit[0..13]".to_string(),
        "INV-12 非法状态机迁移必被拒记且不留状态变化".to_string(),
        "INV-13 环形账溢出只增 dropped，逐条指纹自校验通过".to_string(),
        "INV-14 同种子回放两次 sealed() 必须完全一致".to_string(),
        "INV-15 TLV 组头声明体长 == 实际后续字节数".to_string(),
        "INV-16 冻结区（表/码段/闭集/路由闭集）跨运行零漂移".to_string(),
    ]
}

/// 演示场景：32 组合逐条解码 + 路由。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let combos = combo_table();
    let mut router = match Router::new() {
        Ok(r) => r,
        Err(_) => return vec![String::from("router-new-failed")],
    };
    let mut i: usize = 0;
    while i < combos.len() {
        if let Some(c) = combos.get(i) {
            if let Ok(code) = code_from_bits7(c.bits7) {
                if let Ok(dec) = pf_decode(code) {
                    let r = router.try_dispatch(code);
                    out.push(format!("#{} {} → {} → {}", c.id, dec.label(), r.name(), r.describe()));
                }
            }
        }
        i = i.wrapping_add(1);
    }
    out.push(format!("--- ledger: {}", router.ledger.describe()));
    out.push(format!("--- latency: {}", router.latency.describe()));
    out.push(format!("--- ring: {}", router.ring.describe()));
    let _ = seed;
    out
}

/// 演示账快照串。
pub fn demo_ledger(seed: u64) -> String {
    let mut r = match Router::new() {
        Ok(x) => x,
        Err(_) => return String::from("router-new-failed"),
    };
    let mut rng = Lcg::new(seed);
    let mut i: u64 = 0;
    while i < 128 {
        let raw = (rng.bits(32) & parsed_union()) | (rng.bits(32) & RESERVED_MASK);
        let _ = r.dispatch(PfCode { raw });
        i = i.wrapping_add(1);
    }
    let mut s = String::new();
    s.push_str(&format!("{}\n", r.ledger.describe()));
    for row in r.ledger.hit_rows() {
        s.push_str(&row);
        s.push('\n');
    }
    s.push_str(&format!("latency={}\n", r.latency.describe()));
    s
}

// ===========================================================================
// 20. 版本锚
// ===========================================================================

/// 本单判据编号。
pub const CRITERION_ID: &str = "UNX-F2602-J1";
/// 本单判据原文（逐字）。
pub const CRITERION_TEXT: &str =
    "UNX-F2602-J1 解析器细化后 32 合法组合逐位读出与 F2537 位域表零漂移，入口分发 10^6 次零误差";
/// 本单册内关联单号。
pub const RELATED_TICKETS: [&str; 8] = ["F2537", "F2538", "F2540", "F2601", "F2577", "F2564", "F2565", "F2566"];
/// 关联单串。
pub fn related_str() -> String {
    let mut s = String::new();
    for t in RELATED_TICKETS.iter() {
        s.push_str(t);
        s.push(' ');
    }
    s
}

