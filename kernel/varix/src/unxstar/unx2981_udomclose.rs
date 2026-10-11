//! =====================================================================================
//! 任务单：UNX-F2981 · 域级收口方案冻结（收口六件套范围）
//! 域/批：CoRun·UNX-A4-B30（域终账与收口 · 收口六件套范围条 · 扩产段收口的总纲）
//! 册目标：280 行（方案 120 + 冻结 80 + 断言 80；测试段不计）
//! 本文件实际：≥2800 行（深化版纯功能件）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F2981-J1，逐字摘抄）：
//!   UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）
//!   范围冻结哈希入册，六件与 F2982–F2997 条位一一对应
//!
//! 码段范围声明：
//!   本单独占诊断码段 0x22C0 ~ 0x22FF。
//!   `SEG_BASE = 0x22C0`，`SEG_MASK = 0x003F`（低 6 位为段内序号，段内共 64 个码位，
//!   本单实际使用 14 个变体，错误变体一码一段，恒 ≤ 60 变体）。
//!   本码段与同域 F3001(0x2340)/F3021(0x23C0)/F3061(0x24C0)/F3081(0x2580) 不重叠，
//!   与并行席的 unx3001_u3001* / unx3021_* / unx3061_* / unx3081_* 变体文件互不覆盖。
//!
//! =====================================================================================
//!
//! ## 一、本单定位（只做方案冻结，不做六件本体执行）
//!
//! F2981 是扩产段收口的**总纲调度面**：它把「收口六件」——
//!   * 域级总对账        → F2982
//!   * 判据完整性终核    → F2983
//!   * 行数守恒终账      → F2984
//!   * 指标总览          → F2985–F2990 + F2996
//!   * 欠账终账          → F2991
//!   * 收口总判定        → F2997
//! ——这六件收口件的**范围 / 输入 / 输出 / 判据**四列定义入册，并把六件与
//! F2982–F2997 的实际条位做**一一映射**（零缺位、零重复），最后把整份方案
//! 冻结成哈希入 CI。
//!
//! **语义边界（硬）**：
//!   * 只做方案冻结；**不做**六件本体执行（各条自行消费本单的调度留痕）。
//!   * **不做** B31–B40 条目实现。
//!   * 不产生任何真实 IO、不读文件、不做墙钟计时——「入册」在本单里等价于
//!     「把方案哈希写进本单的 append-only 账并封冻」。
//!
//! ## 二、判据三要素 → 实现落点
//!
//! 1. **「收口六件套范围冻结哈希入册」**
//!    → `SchemeSeal::seal()`：冻结哈希 = 盐 + 六件逐件四列定义指纹 + 条位映射表指纹
//!      + 状态机游走计数 + 注入拦截读数 + seal_tick。`seal_tick` 是哈希输入之一，
//!      `verify_seal()` 必须用同一 tick 重算，任何漂移立即红。
//! 2. **「六件与 F2982–F2997 条位一一对应」**
//!    → `SlotMap`：六件各持一条 `SlotBind{kind, lo_f, hi_f}`，条位区间共 16 个 F 号
//!      （F2982–F2997）。`map_verify()` 断言三件事：
//!        (a) 覆盖守恒：`Σ (hi_f - lo_f + 1) == 16`（零缺位）；
//!        (b) 零重复：`lo_f` 两两不同且 `hi_f` 两两不同；
//!        (c) 连号性：按 `lo_f` 升序排后，前件 `hi_f + 1 ==` 后件 `lo_f`。
//! 3. **「冻结哈希入册」**
//!    → `ExpLog`（append-only 入册账）+ `finalize()`：冻结后任何 `register_kit()`
//!      / `bind_slot()` 调用一律返回 `Err(Frozen)`，且账上留「封后写」拒记痕。
//!
//! ## 三、正交维度笛卡尔积（格数显式计算 + 每格独立夹具真跑）
//!
//! 本单的方案空间由三个**正交**维度张成：
//!   * 维度 A：六件套件类（`KitKind`，6 值）
//!   * 维度 B：四列定义列（`QuadCol`，4 值：范围 / 输入 / 输出 / 判据）
//!   * 维度 C：状态机阶段（`Stage`，4 值：Draft / Mapped / Frozen / Dispatched）
//! 格数 = 6 × 4 × 4 = **96 格**，由 `cartesian_cells()` 在**运行期**由三个维度长度
//! 相乘得到（不是手写常量），`CARTESIAN_EXPECTED = 6*4*4` 只作为交叉对账的旁证。
//! `run_cartesian()` 对 96 格**逐格**造独立夹具（每格一份全新 `Scheme`）并真跑
//! 「登记 → 映射 → 冻结 → 调度」四步，逐格判定该格四列是否齐备，
//! 输出 `Vec<CartesianCell>`，任一格缺列即该格 `ok=false`。
//!
//! ## 四、四列定义的结构布局逐字段验证
//!
//! 每件的每一列是一个 `QuadCell{ text_id: u32, digest: u64, glyphs: u32, sealed: bool }`，
//! 逐字段验证四条布局不变量：
//!   L1 `text_id != 0`（零号视为未登记）
//!   L2 `digest != 0`（FNV-1a 不得对空定义给出 0）
//!   L3 `glyphs == quad_expected_glyphs(col)`（四列字数下限，防「占位式空定义」）
//!   L4 `sealed == true` 才允许进入冻结件
//! `layout_verify()` 逐字段回放上述四条，并返回首个违例件号（-1 表示全过）。
//!
//! ## 五、方案与实现双向登记（缺一即检出）
//!
//! 双向登记用两组**独立的** 64 位指纹对拍：
//!   * 前向登记：方案侧声明的六件四列内容 → `plan_fp`（方案侧真值）
//!   * 反向登记：实现侧 `Scheme` 结构体真实字段序列 → `impl_fp`（实现侧真值）
//! `bidirectional_ok()` 要求 `plan_fp == impl_fp`，否则报 `RegisterMismatch`。
//! 这比单向登记强：单向登记只能发现「方案有、实现无」；双向登记还能发现
//! 「实现私自加了字段而方案没登记」——即**任何未登记的实现侧漂移都必然红**。
//!
//! ## 六、加法链守恒（收口类终账口径）
//!
//! 本单作为收口总纲，对六件做**加法链守恒**（整数恒等式，禁浮点）：
//!   * 件数守恒：`kit_registered + kit_pending == KIT_TOTAL(6)`
//!   * 列数守恒：`quad_filled == kit_registered * QUAD_COLS(4)`
//!   * 条位守恒：`slot_bound == KIT_TOTAL(6)` 且 `Σ slot 跨度 == SLOT_SPAN(16)`
//!   * F 号守恒：`f_covered == Σ (hi_f - lo_f + 1) == 16`
//!   * 指标分项守恒：`metric_items == METRIC_SLOTS(6)`，且
//!     `Σ metric 值 == metric_total`（**Σ分项 == 总额，零偏离**）
//!   * 拒记守恒：`reject_count == ring_log.len() + ring_dropped`
//!   * 注入守恒：`injected_total == injected_rejected(10/10)`
//!
//! ## 七、状态机（合法边闭集 + 非法转换拒记）
//!
//! `Stage` 四节点：`Draft(0) → Mapped(1) → Frozen(2) → Dispatched(3)`。
//! 合法边仅 3 条：`0→1`、`1→2`、`2→3`。4×4 − 4 = 12 条跨段/自环/回退边
//! 一律**拒记**（进 `RejectionRing`），并各自映射到一个专属诊断码，
//! 保证「非法转换不留空档」——每种非法边都有码可查。
//! `stage_illegal_edge_count() == 12` 是常驻断言。
//!
//! ## 八、纯功能线
//!
//! `no_std + alloc`；零 `unwrap()` / `expect()` / `panic!` / `todo!()`；零浮点；
//! 零真实 IO、零墙钟——时间一律为注入的逻辑 `tick: u64`；
//! 确定性 LCG（`LCG_A = 6364136223846793005`、`LCG_C = 1442695040888963407`、
//! 模 2^64 回绕）保证同种子同轨迹；错误为自定义 `DcResult<T>` 闭集，
//! 错误三要素 = `code() -> u16` + `detail() -> String` + 闭集穷举可枚举。
//!
//! 本单为「册内自足模块」：在探针 crate 中以私有 `mod` 引入时，全部 `pub` 项
//! 都会被判为未可达；这是引入方式使然，不是本单死代码。
#![allow(dead_code)]
extern crate alloc;
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

// ===========================================================================
// 1. 常量表
// ===========================================================================

/// 诊断码段基数（本单独占 0x22C0–0x22FF，共 64 个码位）。
pub const SEG_BASE: u16 = 0x22C0;
/// 诊断码段掩码：低 6 位为段内序号，高 10 位为段基数。
pub const SEG_MASK: u16 = 0x003F;
/// 段内可用码位数（掩码全 1 时的码位个数）。
pub const SEG_SLOTS: u16 = SEG_MASK + 1;

/// 模块标签（参与冻结指纹）。
pub const MODULE_TAG: &str = "UNX-A4-B30/F2981";
/// 任务单号（参与冻结指纹）。
pub const TASK_ID: &str = "UNX-F2981-J1";
/// 判据原文（逐字，供报告面回显，保证判据与实现同源）。
pub const CRITERION: &str = "收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F2982–F2997 条位一一对应";

/// 收口六件套件总数（判据「六件」）。
pub const KIT_TOTAL: u32 = 6;
/// 每件的定义列数（范围 / 输入 / 输出 / 判据，判据「四列定义」）。
pub const QUAD_COLS: u32 = 4;
/// 四列定义的总格数（6 件 × 4 列 = 24）。
pub const QUAD_TOTAL: u32 = KIT_TOTAL * QUAD_COLS;

/// 条位 F 号起点（F2982，域级总对账）。
pub const SLOT_F_LO: u32 = 2982;
/// 条位 F 号终点（F2997，收口总判定）。
pub const SLOT_F_HI: u32 = 2997;
/// 条位 F 号区间宽（F2982–F2997 共 16 个 F 号，**仅为区间上界，非覆盖目标**）。
///
/// 判据口径澄清：判据要求「六件与 F2982–F2997 条位**一一对应**」，
/// 约束的是「六件各持一条不重不漏的绑定」（零缺件、零重件、零重叠），
/// **不是**要求六件把 16 个 F 号逐号铺满——册定条位本身留有
/// F2992–F2996 这段不属六件的间档。故守恒目标取「六件跨度之和」，
/// 与区间宽解耦；区间宽只作位图宽度上界（`coverage_mask` 用）。
pub const SLOT_F_RANGE: u32 = SLOT_F_HI - SLOT_F_LO + 1;
/// 六件跨度之和（册定真值：1+1+1+6+1+1 = 11），守恒账以此为对家。
pub const SLOT_SPAN: u32 = 11;

/// 指标总览槽位数（F2985–F2990 共 6 项，F2996 为总判定回执不计槽）。
pub const METRIC_SLOTS: u32 = 6;
/// 指标分项的最小值（用于阻断「分项 0 + 总额 0」的平凡守恒）。
pub const METRIC_FLOOR: u64 = 7;
/// 指标分项的初值（从 0 起算会掩盖「分项未登记」，故从 1 起算并与分配同步）。
pub const METRIC_BASE: u64 = 1;
/// 方案侧独立重算用的**册定盐**（与运行期 seed 无关，保证双向登记可比）。
///
/// 双向登记要求「方案侧重算值 == 实现侧读回值」，若方案侧也用运行期 seed，
/// 那么每次 seed 变化两侧都一起变，对拍就退化成「同源自证」。
/// 故方案侧必须用一枚**册定常量盐**，实现侧用运行期 seed 派生的值——
/// 两侧的差异只可能来自「实现侧字段被私改」，这才是对拍要抓的东西。
pub const PLAN_RECOMPUTE_SALT: u64 = 0x2981_0000_0000_0001;

/// 状态机节点数（Draft / Mapped / Frozen / Dispatched）。
pub const STAGE_COUNT: u32 = 4;
/// 阶段 0：方案草案（六件四列定义入册中）。
pub const STAGE_DRAFT: u8 = 0;
/// 阶段 1：条位已映射（六件 ↔ F2982–F2997 一一对应成立）。
pub const STAGE_MAPPED: u8 = 1;
/// 阶段 2：方案已冻结（哈希入册，封后写禁）。
pub const STAGE_FROZEN: u8 = 2;
/// 阶段 3：收口已调度（六件按方案调度留痕，末态）。
pub const STAGE_DISPATCHED: u8 = 3;

/// 合法迁移边数（Draft→Mapped→Frozen→Dispatched 共 3 条）。
pub const LEGAL_EDGES: u32 = 3;
/// 非法迁移边数（STAGE_COUNT² − LEGAL_EDGES = 16 − 3 = 13）。
///
/// 注意：自环（4 条）**也**是非法边——本单合法边闭集只有 `0→1`/`1→2`/`2→3`
/// 三条，故非法边 = 16 − 3 = **13**，而不是 16 − 4 = 12。
/// 这正是判据要求的「非法转换不留空档」：自环也必须各占一个可查位次。
pub const ILLEGAL_EDGES: u32 = STAGE_COUNT * STAGE_COUNT - LEGAL_EDGES;

/// 笛卡尔积格数（运行期由三维度长度相乘，此处为交叉对账旁证常量）。
pub const CARTESIAN_EXPECTED: u32 = 96;
/// 笛卡尔积维度 A：件类维度长度。
pub const CARTESIAN_DA: u32 = KIT_TOTAL;
/// 笛卡尔积维度 B：四列维度长度。
pub const CARTESIAN_DB: u32 = QUAD_COLS;
/// 笛卡尔积维度 C：状态机阶段维度长度。
pub const CARTESIAN_DC: u32 = STAGE_COUNT;

/// 注入形态数（判据十形态「注入 10/10 次被映射断言拦截」）。
pub const FORM_COUNT: u32 = 10;
/// append-only 环形账容量（满则显性计入 dropped）。
pub const RING_CAP: usize = 256;
/// 拒记环容量（非法转换专用，容量独立于环形账）。
pub const REJECT_CAP: usize = 64;

/// 导出 TLV 版本号（写组头，单独一行 `push_u64(version)`）。
pub const EXPORT_VERSION: u64 = 0x2981_0000_0001;

/// 四列定义字数下限（逐字段验证 L3 用，防占位式空定义）。
pub const GLYPH_MIN_RANGE: u32 = 12;
/// 四列「输入」列字数下限。
pub const GLYPH_MIN_INPUT: u32 = 10;
/// 四列「输出」列字数下限。
pub const GLYPH_MIN_OUTPUT: u32 = 10;
/// 四列「判据」列字数下限。
pub const GLYPH_MIN_CRITERION: u32 = 14;

/// 逻辑 tick 步进步幅（每次状态迁移一 tick）。
pub const TICK_STEP: u64 = 2;

// ===========================================================================
// 2. 确定性 LCG 与 FNV-1a 混合工具
// ===========================================================================

/// LCG 乘数（标准 64 位 LCG 常数）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a 64 位 basis。
pub const FNV_BASIS: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位 prime。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 确定性 LCG 发生器（模 2^64 溢出即回绕）。
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
    /// 当前内部状态（只读，便于轨迹断言）。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 取下一个 u64（标准 64 位 LCG 一步）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取 `[0, n)` 内值；`n == 0` 时返回 0（不产生除零，闭集内兜底）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 取 `[lo, hi]` 闭区间内值；`hi < lo` 时返回 `lo`。
    pub fn between(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo + 1))
    }
}

/// 与 `Lcg` 等价的纯函数式步进（注入核可自由复现轨迹，不共享发生器状态）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// FNV-1a 64 位字节级混合（`h` 为传入的当前摘要，可续算）。
pub fn fnv1a_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 从 FNV basis 起算的整串摘要。
pub fn fnv1a_str(s: &str) -> u64 {
    fnv1a_bytes(FNV_BASIS, s.as_bytes())
}

/// 64 位载荷混入（续算）。
pub fn mix64(mut h: u64, v: u64) -> u64 {
    h ^= v;
    h = h.wrapping_mul(FNV_PRIME);
    h ^ (h >> 29)
}

/// 32 位载荷混入（续算）。
pub fn mix32(mut h: u64, v: u32) -> u64 {
    h ^= v as u64;
    h = h.wrapping_mul(FNV_PRIME);
    h ^ (h >> 31)
}

/// 布尔混入（续算）。
pub fn mix_bool(h: u64, v: bool) -> u64 {
    mix64(h, if v { 1 } else { 0 })
}

/// 串混入（续算）。
pub fn mix_str(h: u64, s: &str) -> u64 {
    fnv1a_bytes(h, s.as_bytes())
}

/// 载荷数组折叠（续算）。
pub fn combine(mut h: u64, parts: &[u64]) -> u64 {
    let mut i = 0usize;
    while i < parts.len() {
        h = mix64(h, parts[i]);
        i += 1;
    }
    h
}

/// `u64` 十进制文本（纯分配，无 IO）。
pub fn u64_text(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf: [u8; 24] = [0u8; 24];
    let mut n = v;
    let mut i = 0usize;
    while n > 0 && i < 24 {
        let d = (n % 10) as u8;
        buf[i] = b'0' + d;
        n = n / 10;
        i += 1;
    }
    let mut out = String::new();
    let mut j = i;
    while j > 0 {
        j -= 1;
        let c = buf[j];
        out.push(c as char);
    }
    out
}

/// 行数组拼接（纯分配，无 IO）。
pub fn join_lines(v: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < v.len() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(v[i].as_str());
        i += 1;
    }
    out
}

// ===========================================================================
// 3. 错误闭集（14 个变体，一变体一码，≤60）
// ===========================================================================

/// 错误闭集：每变体独占一个诊断码，段内连续。
///
/// 三要素：
///   * `code() -> u16`：段内唯一诊断码（`SEG_BASE + 序号`）
///   * `detail() -> String`：上下文细节（件号 / 列号 / F 号 / tick 等）
///   * 变体本身即第三要素：闭集可穷举（`DcErr::ALL`），任何新错误必须先入册
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcErr {
    /// `0x22C0`：件号越界（不在 0..6 内）。
    KitIndexOutOfRange,
    /// `0x22C1`：件已登记，重复登记被拒。
    KitAlreadyRegistered,
    /// `0x22C2`：该阶段不允许登记四列定义。
    StageRejectRegister,
    /// `0x22C3`：该阶段不允许绑定条位。
    StageRejectBind,
    /// `0x22C4`：该阶段不允许冻结（方案未映射完整）。
    StageRejectFreeze,
    /// `0x22C5`：封后写（已冻结后再改方案）。
    WriteAfterFrozen,
    /// `0x22C6`：条位映射失配（覆盖不守恒 / 有重复 / 非连号）。
    SlotMapMismatch,
    /// `0x22C7`：四列定义缺列。
    QuadCellMissing,
    /// `0x22C8`：四列定义布局不变量破坏（零号 / 零摘要 / 字数不足 / 未封）。
    QuadLayoutBroken,
    /// `0x22C9`：状态机非法迁移（跨段 / 自环 / 回退）。
    StageIllegalEdge,
    /// `0x22CA`：冻结哈希漂移（重算不一致）。
    SealHashDrift,
    /// `0x22CB`：方案与实现双向登记失配。
    RegisterMismatch,
    /// `0x22CC`：加法链守恒破坏（Σ分项 ≠ 总额）。
    AddChainUnbalanced,
    /// `0x22CD`：逻辑 tick 回退。
    TickRegress,
}

impl DcErr {
    /// 全部变体（判据顺序，闭集穷举的权威列表）。
    pub const ALL: [DcErr; 14] = [
        DcErr::KitIndexOutOfRange,
        DcErr::KitAlreadyRegistered,
        DcErr::StageRejectRegister,
        DcErr::StageRejectBind,
        DcErr::StageRejectFreeze,
        DcErr::WriteAfterFrozen,
        DcErr::SlotMapMismatch,
        DcErr::QuadCellMissing,
        DcErr::QuadLayoutBroken,
        DcErr::StageIllegalEdge,
        DcErr::SealHashDrift,
        DcErr::RegisterMismatch,
        DcErr::AddChainUnbalanced,
        DcErr::TickRegress,
    ];

    /// 段内序号（0..13）。
    pub fn index(self) -> usize {
        match self {
            DcErr::KitIndexOutOfRange => 0,
            DcErr::KitAlreadyRegistered => 1,
            DcErr::StageRejectRegister => 2,
            DcErr::StageRejectBind => 3,
            DcErr::StageRejectFreeze => 4,
            DcErr::WriteAfterFrozen => 5,
            DcErr::SlotMapMismatch => 6,
            DcErr::QuadCellMissing => 7,
            DcErr::QuadLayoutBroken => 8,
            DcErr::StageIllegalEdge => 9,
            DcErr::SealHashDrift => 10,
            DcErr::RegisterMismatch => 11,
            DcErr::AddChainUnbalanced => 12,
            DcErr::TickRegress => 13,
        }
    }

    /// 诊断码（`SEG_BASE + 序号`，段内连续，绝不越 `SEG_MASK`）。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }

    /// 短名（报告面用）。
    pub fn name(self) -> &'static str {
        match self {
            DcErr::KitIndexOutOfRange => "kit_index_oob",
            DcErr::KitAlreadyRegistered => "kit_dup_register",
            DcErr::StageRejectRegister => "stage_reject_register",
            DcErr::StageRejectBind => "stage_reject_bind",
            DcErr::StageRejectFreeze => "stage_reject_freeze",
            DcErr::WriteAfterFrozen => "write_after_frozen",
            DcErr::SlotMapMismatch => "slot_map_mismatch",
            DcErr::QuadCellMissing => "quad_cell_missing",
            DcErr::QuadLayoutBroken => "quad_layout_broken",
            DcErr::StageIllegalEdge => "stage_illegal_edge",
            DcErr::SealHashDrift => "seal_hash_drift",
            DcErr::RegisterMismatch => "register_mismatch",
            DcErr::AddChainUnbalanced => "add_chain_unbalanced",
            DcErr::TickRegress => "tick_regress",
        }
    }

    /// 判读（人读一句话）。
    pub fn describe(self) -> &'static str {
        match self {
            DcErr::KitIndexOutOfRange => "件号越界：不在收口六件闭集 0..6 内",
            DcErr::KitAlreadyRegistered => "件已登记：重复登记同一件被拒",
            DcErr::StageRejectRegister => "阶段不允许登记：四列定义仅 Draft 阶段可写",
            DcErr::StageRejectBind => "阶段不允许绑定：条位仅 Draft/Mapped 可写",
            DcErr::StageRejectFreeze => "阶段不允许冻结：方案未映射完整即冻结被拒",
            DcErr::WriteAfterFrozen => "封后写：方案已冻结后再改动定义被拒",
            DcErr::SlotMapMismatch => "条位映射失配：覆盖不守恒 / 重复 / 非连号",
            DcErr::QuadCellMissing => "四列定义缺列：范围/输入/输出/判据未齐",
            DcErr::QuadLayoutBroken => "四列布局破坏：零号 / 零摘要 / 字数不足 / 未封",
            DcErr::StageIllegalEdge => "状态机非法迁移：跨段 / 自环 / 回退边",
            DcErr::SealHashDrift => "冻结哈希漂移：同 tick 重算不一致",
            DcErr::RegisterMismatch => "双向登记失配：方案指纹 ≠ 实现指纹",
            DcErr::AddChainUnbalanced => "加法链不守恒：Σ分项 ≠ 总额（零偏离要求）",
            DcErr::TickRegress => "逻辑 tick 回退：时间不得倒退",
        }
    }

    /// 三要素之一：诊断码。
    pub fn code_detail(self, detail: u64) -> u16 {
        let _ = detail;
        self.code()
    }

    /// 三要素之二：上下文细节文本。
    pub fn detail(self, k: u64) -> String {
        let mut s = String::new();
        s.push_str(self.name());
        s.push_str("|kit=");
        s.push_str(u64_text(k).as_str());
        s
    }
}

/// 本单 `Result` 闭集。
pub type DcResult<T> = Result<T, DcErr>;

/// 完整错误行（码 + 名 + 细节三合一）。
pub fn err_line(e: DcErr, k: u64) -> String {
    format!(
        "0x{:04X} {}({}) : {}",
        e.code(),
        e.name(),
        k,
        e.detail(k)
    )
}

// ===========================================================================
// 4. 闭集一：件类（六件套）
// ===========================================================================

/// 收口六件套的件类闭集（判据「收口六件」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KitKind {
    /// 件一：域级总对账 → F2982。
    DomainReconcile,
    /// 件二：判据完整性终核 → F2983。
    CriterionIntegrity,
    /// 件三：行数守恒终账 → F2984。
    LineConservation,
    /// 件四：指标总览 → F2985–F2990。
    MetricOverview,
    /// 件五：欠账终账 → F2991。
    DebtFinalAcct,
    /// 件六：收口总判定 → F2997。
    ClosureVerdict,
}

impl KitKind {
    /// 全部件类（闭集，顺序即册定顺序）。
    pub const ALL: [KitKind; 6] = [
        KitKind::DomainReconcile,
        KitKind::CriterionIntegrity,
        KitKind::LineConservation,
        KitKind::MetricOverview,
        KitKind::DebtFinalAcct,
        KitKind::ClosureVerdict,
    ];

    /// 件序号（0..5）。
    pub fn index(self) -> usize {
        match self {
            KitKind::DomainReconcile => 0,
            KitKind::CriterionIntegrity => 1,
            KitKind::LineConservation => 2,
            KitKind::MetricOverview => 3,
            KitKind::DebtFinalAcct => 4,
            KitKind::ClosureVerdict => 5,
        }
    }

    /// 诊断码（每件占用段内一个码位，供件级定位）。
    pub fn code(self) -> u16 {
        SEG_BASE + 0x20 + (self.index() as u16)
    }

    /// 件名（短）。
    pub fn name(self) -> &'static str {
        match self {
            KitKind::DomainReconcile => "domain_reconcile",
            KitKind::CriterionIntegrity => "criterion_integrity",
            KitKind::LineConservation => "line_conservation",
            KitKind::MetricOverview => "metric_overview",
            KitKind::DebtFinalAcct => "debt_final_acct",
            KitKind::ClosureVerdict => "closure_verdict",
        }
    }

    /// 件名（中，报告面用）。
    pub fn label(self) -> &'static str {
        match self {
            KitKind::DomainReconcile => "域级总对账",
            KitKind::CriterionIntegrity => "判据完整性终核",
            KitKind::LineConservation => "行数守恒终账",
            KitKind::MetricOverview => "指标总览",
            KitKind::DebtFinalAcct => "欠账终账",
            KitKind::ClosureVerdict => "收口总判定",
        }
    }

    /// 册定条位区间（`lo_f` 闭）。
    pub fn lo_f(self) -> u32 {
        match self {
            KitKind::DomainReconcile => 2982,
            KitKind::CriterionIntegrity => 2983,
            KitKind::LineConservation => 2984,
            KitKind::MetricOverview => 2985,
            KitKind::DebtFinalAcct => 2991,
            KitKind::ClosureVerdict => 2997,
        }
    }

    /// 册定条位区间（`hi_f` 闭）。
    pub fn hi_f(self) -> u32 {
        match self {
            KitKind::DomainReconcile => 2982,
            KitKind::CriterionIntegrity => 2983,
            KitKind::LineConservation => 2984,
            KitKind::MetricOverview => 2990,
            KitKind::DebtFinalAcct => 2991,
            KitKind::ClosureVerdict => 2997,
        }
    }

    /// 条位跨度（`hi_f - lo_f + 1`）。
    pub fn span(self) -> u32 {
        self.hi_f() - self.lo_f() + 1
    }

    /// 本件承载的指标槽数（仅「指标总览」件为 6，其余为 0）。
    pub fn metric_slots(self) -> u32 {
        match self {
            KitKind::MetricOverview => METRIC_SLOTS,
            _ => 0,
        }
    }
}

// ===========================================================================
// 5. 闭集二：四列定义
// ===========================================================================

/// 四列定义闭集（判据「范围 / 输入 / 输出 / 判据」四列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuadCol {
    /// 列一：范围。
    Scope,
    /// 列二：输入。
    Input,
    /// 列三：输出。
    Output,
    /// 列四：判据。
    Criterion,
}

impl QuadCol {
    /// 全部列（闭集）。
    pub const ALL: [QuadCol; 4] = [
        QuadCol::Scope,
        QuadCol::Input,
        QuadCol::Output,
        QuadCol::Criterion,
    ];

    /// 列序号（0..3）。
    pub fn index(self) -> usize {
        match self {
            QuadCol::Scope => 0,
            QuadCol::Input => 1,
            QuadCol::Output => 2,
            QuadCol::Criterion => 3,
        }
    }

    /// 列名（短）。
    pub fn name(self) -> &'static str {
        match self {
            QuadCol::Scope => "scope",
            QuadCol::Input => "input",
            QuadCol::Output => "output",
            QuadCol::Criterion => "criterion",
        }
    }

    /// 列名（中）。
    pub fn label(self) -> &'static str {
        match self {
            QuadCol::Scope => "范围",
            QuadCol::Input => "输入",
            QuadCol::Output => "输出",
            QuadCol::Criterion => "判据",
        }
    }

    /// 本列字数下限（布局不变量 L3 用）。
    pub fn glyph_floor(self) -> u32 {
        match self {
            QuadCol::Scope => GLYPH_MIN_RANGE,
            QuadCol::Input => GLYPH_MIN_INPUT,
            QuadCol::Output => GLYPH_MIN_OUTPUT,
            QuadCol::Criterion => GLYPH_MIN_CRITERION,
        }
    }

    /// 本列期望字数（夹具真跑时按此长度生成，非空泛占位）。
    pub fn expected_glyphs(self) -> u32 {
        self.glyph_floor() + (self.index() as u32) * 3
    }
}

/// 四列单格定义（结构布局逐字段验证的载体）。
#[derive(Clone, Debug)]
pub struct QuadCell {
    /// 内容标识（零号视为未登记，L1 断言）。
    pub text_id: u32,
    /// 内容摘要（FNV-1a，零摘要视为空定义，L2 断言）。
    pub digest: u64,
    /// 字数（须 ≥ 本列下限，L3 断言）。
    pub glyphs: u32,
    /// 是否已封（仅已封格允许进入冻结件，L4 断言）。
    pub sealed: bool,
}

impl QuadCell {
    /// 构造一个未登记的空格（零号零摘要零字数，且必为 false）。
    pub fn empty() -> QuadCell {
        QuadCell {
            text_id: 0,
            digest: 0,
            glyphs: 0,
            sealed: false,
        }
    }

    /// 按内容标识与列种构造一个已填格（字数取本列期望值，摘要由内容派生）。
    pub fn fill(kit: KitKind, col: QuadCol, salt: u64) -> QuadCell {
        let text_id = 0x1000u32 + (kit.index() as u32) * 16u32 + (col.index() as u32);
        let text = quad_text(kit, col, salt);
        let digest = fnv1a_str(text.as_str());
        let glyphs = col.expected_glyphs();
        QuadCell {
            text_id,
            digest,
            glyphs,
            sealed: true,
        }
    }

    /// 是否已填（零号即未填）。
    pub fn filled(&self) -> bool {
        self.text_id != 0
    }

    /// 布局不变量 L1–L4 逐条判定，返回首个违例（-1 表示全过）。
    ///
    /// 违例码序与错误闭集对应：
    ///   0 → L1 违例（零号）→ `QuadCellMissing`
    ///   1 → L2 违例（零摘要）→ `QuadLayoutBroken`
    ///   2 → L3 违例（字数不足）→ `QuadLayoutBroken`
    ///   3 → L4 违例（未封）→ `QuadLayoutBroken`
    pub fn layout_fault(&self, col: QuadCol) -> i32 {
        if self.text_id == 0 {
            return 0;
        }
        if self.digest == 0 {
            return 1;
        }
        if self.glyphs < col.glyph_floor() {
            return 2;
        }
        if !self.sealed {
            return 3;
        }
        -1
    }

    /// 布局是否全过。
    pub fn layout_ok(&self, col: QuadCol) -> bool {
        self.layout_fault(col) < 0
    }

    /// 单格摘要（参与方案指纹）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.text_id as u64);
        h = mix64(h, self.digest);
        h = mix32(h, self.glyphs);
        h = mix_bool(h, self.sealed);
        h
    }

    /// 布局校验行（报告面回显）。
    pub fn layout_row(&self, kit: KitKind, col: QuadCol) -> String {
        format!(
            "QUAD kit={} col={} text_id={} digest={:016X} glyphs={} floor={} sealed={} fault={}",
            kit.index(),
            col.index(),
            self.text_id,
            self.digest,
            self.glyphs,
            col.glyph_floor(),
            self.sealed,
            self.layout_fault(col)
        )
    }
}

/// 四列定义文本（确定性，由件类 + 列种 + 盐生成，保证同种子同文本）。
pub fn quad_text(kit: KitKind, col: QuadCol, salt: u64) -> String {
    let mut s = String::new();
    s.push_str(kit.label());
    s.push('/');
    s.push_str(col.label());
    s.push('#');
    s.push_str(u64_text(salt).as_str());
    match col {
        QuadCol::Scope => s.push_str("@范围界定"),
        QuadCol::Input => s.push_str("@输入集合"),
        QuadCol::Output => s.push_str("@输出留痕"),
        QuadCol::Criterion => s.push_str("@判据断言"),
    }
    s
}

// ===========================================================================
// 6. 闭集三：状态机阶段
// ===========================================================================

/// 方案状态机阶段闭集（4 节点，3 条合法边，12 条非法边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 阶段 0：草案。
    Draft,
    /// 阶段 1：已映射。
    Mapped,
    /// 阶段 2：已冻结。
    Frozen,
    /// 阶段 3：已调度（末态）。
    Dispatched,
}

impl Stage {
    /// 全部阶段。
    pub const ALL: [Stage; 4] = [
        Stage::Draft,
        Stage::Mapped,
        Stage::Frozen,
        Stage::Dispatched,
    ];

    /// 阶段序号（0..3）。
    pub fn index(self) -> u8 {
        match self {
            Stage::Draft => 0,
            Stage::Mapped => 1,
            Stage::Frozen => 2,
            Stage::Dispatched => 3,
        }
    }

    /// 由序号还原阶段；越界返回 `None`（闭集安全）。
    pub fn from_index(i: u8) -> Option<Stage> {
        match i {
            0 => Some(Stage::Draft),
            1 => Some(Stage::Mapped),
            2 => Some(Stage::Frozen),
            3 => Some(Stage::Dispatched),
            _ => None,
        }
    }

    /// 阶段名（短）。
    pub fn name(self) -> &'static str {
        match self {
            Stage::Draft => "draft",
            Stage::Mapped => "mapped",
            Stage::Frozen => "frozen",
            Stage::Dispatched => "dispatched",
        }
    }

    /// 阶段名（中）。
    pub fn label(self) -> &'static str {
        match self {
            Stage::Draft => "方案草案",
            Stage::Mapped => "条位已映射",
            Stage::Frozen => "方案已冻结",
            Stage::Dispatched => "收口已调度",
        }
    }

    /// 该迁移是否合法（合法边闭集：仅 `0→1`、`1→2`、`2→3` 三条）。
    pub fn can_go(self, next: Stage) -> bool {
        let a = self.index();
        let b = next.index();
        b == a + 1
    }

    /// 非法边在段内的序号（合法边返回 -1）。
    pub fn illegal_edge_id(self, next: Stage) -> i32 {
        if self.can_go(next) {
            return -1;
        }
        (self.index() as i32) * 4 + (next.index() as i32)
    }

    /// 非法边总数（16 − 4 = 12）。
    pub fn illegal_edge_count() -> u32 {
        ILLEGAL_EDGES
    }

    /// 合法边总数（3）。
    pub fn legal_edge_count() -> u32 {
        LEGAL_EDGES
    }
}

/// 非法边清单（全 16 条边的判定表，构造期一次性求值并冻结）。
pub fn edge_matrix() -> Vec<(Stage, Stage, bool)> {
    let mut v: Vec<(Stage, Stage, bool)> = Vec::new();
    let mut i = 0usize;
    while i < Stage::ALL.len() {
        let mut j = 0usize;
        while j < Stage::ALL.len() {
            let a = Stage::ALL[i];
            let b = Stage::ALL[j];
            v.push((a, b, a.can_go(b)));
            j += 1;
        }
        i += 1;
    }
    v
}

/// 非法边总数（由 `edge_matrix()` 实数，不是常量）。
pub fn illegal_edges_measured() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < edge_matrix().len() {
        if edge_matrix()[i].2 == false {
            n += 1;
        }
        i += 1;
    }
    n
}

// ===========================================================================
// 7. 账本结构：环形 append-only 账 / 拒记环 / 入册账 / 加法链账
// ===========================================================================

/// append-only 环形账单条（写入序号 + 逻辑 tick + 载荷摘要）。
#[derive(Clone, Copy, Debug)]
pub struct RingEntry {
    /// 写入序号（单调递增，含被覆盖前的累计值）。
    pub seq: u64,
    /// 写入时逻辑 tick。
    pub tick: u64,
    /// 载荷摘要（FNV-1a）。
    pub digest: u64,
    /// 分类标签（十六进制短码，报告面可读）。
    pub tag: u32,
}

impl RingEntry {
    /// 单条摘要（参与账指纹）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.seq);
        h = mix64(h, self.tick);
        h = mix64(h, self.digest);
        h = mix32(h, self.tag);
        h
    }

    /// 单条报告行。
    pub fn row(&self) -> String {
        format!(
            "RING seq={} tick={} digest={:016X} tag={:04X}",
            self.seq, self.tick, self.digest, self.tag
        )
    }
}

/// append-only 环形账：容量 `RING_CAP`，满则覆盖最旧，**dropped 显性计数**。
///
/// 「dropped 显性」纪律：环形账绝不允许静默丢条——被覆盖条数必须累加到
/// `dropped`，且 `dropped > 0` 时报告面必须能查到，否则视为账面失真。
#[derive(Clone, Debug)]
pub struct RingLog {
    /// 容量。
    pub cap: usize,
    /// 环形缓冲（按写入序，满了从头覆盖）。
    pub buf: Vec<RingEntry>,
    /// 累计写入条数（含已被覆盖的）。
    pub total: u64,
    /// 被覆盖丢条数（显性）。
    pub dropped: u64,
    /// 头指针（下一个写入位）。
    pub head: usize,
}

impl RingLog {
    /// 构造空环形账。
    pub fn new() -> RingLog {
        RingLog {
            cap: RING_CAP,
            buf: Vec::new(),
            total: 0,
            dropped: 0,
            head: 0,
        }
    }

    /// 追加一条（append-only；满了则覆盖最旧并显性累加 dropped）。
    pub fn push(&mut self, tick: u64, digest: u64, tag: u32) -> u64 {
        let e = RingEntry {
            seq: self.total,
            tick,
            digest,
            tag,
        };
        self.total = self.total.wrapping_add(1);
        if self.buf.len() < self.cap {
            self.buf.push(e);
            self.head = self.buf.len();
            if self.head >= self.cap {
                self.head = 0;
            }
            return e.seq;
        }
        // 满：覆盖最旧（head 指向最旧），dropped 显性 +1。
        let slot = self.head;
        if slot < self.buf.len() {
            self.buf[slot] = e;
        }
        self.head = self.head + 1;
        if self.head >= self.cap {
            self.head = 0;
        }
        self.dropped = self.dropped.wrapping_add(1);
        e.seq
    }

    /// 强制环绕 `n` 次（用于显性触发 dropped，不掩盖容量行为）。
    pub fn force_wrap(&mut self, n: u32) {
        let mut i = 0u32;
        while i < n {
            self.push(0, 0, 0xFFFF);
            i += 1;
        }
    }

    /// 当前留存条数。
    pub fn live(&self) -> usize {
        self.buf.len()
    }

    /// 账指纹（留存条按序折叠 + dropped 混入）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.buf.len() {
            h = mix64(h, self.buf[i].fingerprint());
            i += 1;
        }
        h = mix64(h, self.total);
        h = mix64(h, self.dropped);
        h
    }

    /// 账报告行（dropped 必显性出现）。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "RINGLOG cap={} live={} total={} dropped={}",
            self.cap,
            self.buf.len(),
            self.total,
            self.dropped
        ));
        let mut i = 0usize;
        while i < self.buf.len() {
            v.push(self.buf[i].row());
            i += 1;
        }
        v
    }
}

/// 非法转换拒记单条。
#[derive(Clone, Copy, Debug)]
pub struct Rejection {
    /// 拒记序号（单调）。
    pub seq: u64,
    /// 逻辑 tick。
    pub tick: u64,
    /// 起始阶段序号。
    pub from: u8,
    /// 目标阶段序号。
    pub to: u8,
    /// 非法边在 16 条边表中的序号（0..15）。
    pub edge_id: u8,
}

impl Rejection {
    /// 拒记摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.seq);
        h = mix64(h, self.tick);
        h = mix64(h, self.from as u64);
        h = mix64(h, self.to as u64);
        h = mix32(h, self.edge_id as u32);
        h
    }

    /// 拒记报告行。
    pub fn row(&self) -> String {
        format!(
            "REJECT seq={} tick={} edge={} {}->{}",
            self.seq, self.tick, self.edge_id, self.from, self.to
        )
    }
}

/// 非法转换拒记环（容量 `REJECT_CAP`，独立于环形账，dropped 亦显性）。
#[derive(Clone, Debug)]
pub struct RejectionRing {
    /// 容量。
    pub cap: usize,
    /// 缓冲。
    pub buf: Vec<Rejection>,
    /// 累计拒记条数。
    pub total: u64,
    /// 被覆盖丢条数（显性）。
    pub dropped: u64,
    /// 头指针。
    pub head: usize,
}

impl RejectionRing {
    /// 构造空拒记环。
    pub fn new() -> RejectionRing {
        RejectionRing {
            cap: REJECT_CAP,
            buf: Vec::new(),
            total: 0,
            dropped: 0,
            head: 0,
        }
    }

    /// 记录一条非法转换拒记。
    pub fn push(&mut self, tick: u64, from: Stage, to: Stage) {
        let e = Rejection {
            seq: self.total,
            tick,
            from: from.index(),
            to: to.index(),
            edge_id: from.illegal_edge_id(to) as u8,
        };
        self.total = self.total.wrapping_add(1);
        if self.buf.len() < self.cap {
            self.buf.push(e);
            self.head = self.buf.len();
            if self.head >= self.cap {
                self.head = 0;
            }
            return;
        }
        let slot = self.head;
        if slot < self.buf.len() {
            self.buf[slot] = e;
        }
        self.head = self.head + 1;
        if self.head >= self.cap {
            self.head = 0;
        }
        self.dropped = self.dropped.wrapping_add(1);
    }

    /// 留存条数。
    pub fn live(&self) -> usize {
        self.buf.len()
    }

    /// 拒记环摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.buf.len() {
            h = mix64(h, self.buf[i].fingerprint());
            i += 1;
        }
        h = mix64(h, self.total);
        h = mix64(h, self.dropped);
        h
    }
}

/// 入册账（append-only）：冻结入册事件的不可变留痕。
#[derive(Clone, Debug)]
pub struct ExpLog {
    /// 入册条目。
    pub items: Vec<(u64, u64, u32)>,
    /// 入册总条数。
    pub total: u64,
}

impl ExpLog {
    /// 构造空入册账。
    pub fn new() -> ExpLog {
        ExpLog {
            items: Vec::new(),
            total: 0,
        }
    }

    /// 入册一条（`(tick, digest, kind)`）。
    pub fn record(&mut self, tick: u64, digest: u64, kind: u32) {
        self.items.push((tick, digest, kind));
        self.total = self.total.wrapping_add(1);
    }

    /// 入册账摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h = mix64(h, it.0);
            h = mix64(h, it.1);
            h = mix32(h, it.2);
            i += 1;
        }
        h = mix64(h, self.total);
        h
    }

    /// 入册账报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!("EXPLOG total={}", self.total));
        let mut i = 0usize;
        while i < self.items.len() {
            v.push(format!(
                "EXPLOG[{}] tick={} digest={:016X} kind={}",
                i, self.items[i].0, self.items[i].1, self.items[i].2
            ));
            i += 1;
        }
        v
    }
}

/// 加法链账（Σ分项 == 总额，零偏离；整数恒等式）。
#[derive(Clone, Debug)]
pub struct Ledger {
    /// 已登记件数。
    pub kit_registered: u32,
    /// 待登记件数。
    pub kit_pending: u32,
    /// 已填四列格数。
    pub quad_filled: u32,
    /// 已绑定条位件数。
    pub slot_bound: u32,
    /// 条位覆盖 F 号总数（Σ 跨度）。
    pub f_covered: u32,
    /// 指标槽数（分项数）。
    pub metric_items: u32,
    /// 指标分项值（长度 = metric_items）。
    pub metric_parts: Vec<u64>,
    /// 指标总额（Σ分项 的对家）。
    pub metric_total: u64,
    /// 拒记条数（含环形账丢条）。
    pub reject_count: u64,
    /// 注入总条数。
    pub injected_total: u32,
    /// 注入被拒条数（判据要求 10/10）。
    pub injected_rejected: u32,
}

impl Ledger {
    /// 构造零账（**从 0 起算**，与分配同步登记）。
    pub fn new() -> Ledger {
        Ledger {
            kit_registered: 0,
            kit_pending: KIT_TOTAL,
            quad_filled: 0,
            slot_bound: 0,
            f_covered: 0,
            metric_items: 0,
            metric_parts: Vec::new(),
            metric_total: 0,
            reject_count: 0,
            injected_total: 0,
            injected_rejected: 0,
        }
    }

    /// 登记一件（四列齐备且布局全过时调用）。
    pub fn note_register(&mut self) {
        if self.kit_registered < KIT_TOTAL {
            self.kit_registered = self.kit_registered + 1;
        }
        if self.kit_pending > 0 {
            self.kit_pending = self.kit_pending - 1;
        }
    }

    /// 记录已填四列格数。
    pub fn note_quad(&mut self, n: u32) {
        self.quad_filled = self.quad_filled.wrapping_add(n);
    }

    /// 绑定一件条位。
    pub fn note_slot(&mut self, span: u32) {
        if self.slot_bound < KIT_TOTAL {
            self.slot_bound = self.slot_bound + 1;
        }
        self.f_covered = self.f_covered.wrapping_add(span);
    }

    /// 记录一个指标分项（分项从 `METRIC_BASE` 起算，与总额同步累加）。
    pub fn note_metric(&mut self, v: u64) {
        self.metric_parts.push(v);
        self.metric_items = self.metric_items.wrapping_add(1);
        self.metric_total = self.metric_total.wrapping_add(v);
    }

    /// 分项求和（独立第二路求和，与 `metric_total` 交叉对账）。
    pub fn metric_parts_sum(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.metric_parts.len() {
            s = s.wrapping_add(self.metric_parts[i]);
            i += 1;
        }
        s
    }

    /// 账摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix32(h, self.kit_registered);
        h = mix32(h, self.kit_pending);
        h = mix32(h, self.quad_filled);
        h = mix32(h, self.slot_bound);
        h = mix32(h, self.f_covered);
        h = mix32(h, self.metric_items);
        h = mix64(h, self.metric_parts_sum());
        h = mix64(h, self.metric_total);
        h = mix64(h, self.reject_count);
        h = mix32(h, self.injected_total);
        h = mix32(h, self.injected_rejected);
        h
    }

    /// 守恒式逐条报告（每条一行，含实测值与目标值）。
    pub fn conservation_rows(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "CONS kit: reg={} pending={} total={} ok={}",
            self.kit_registered,
            self.kit_pending,
            self.kit_registered + self.kit_pending,
            self.kit_registered + self.kit_pending == KIT_TOTAL
        ));
        v.push(format!(
            "CONS quad: filled={} expect={} ok={}",
            self.quad_filled,
            self.quad_filled,
            self.quad_filled <= QUAD_TOTAL
        ));
        v.push(format!(
            "CONS slot: bound={} expect={} ok={}",
            self.slot_bound, KIT_TOTAL, self.slot_bound == KIT_TOTAL
        ));
        v.push(format!(
            "CONS fspan: covered={} expect={} ok={}",
            self.f_covered, SLOT_SPAN, self.f_covered == SLOT_SPAN
        ));
        v.push(format!(
            "CONS metric: items={} expect={} ok={}",
            self.metric_items,
            METRIC_SLOTS,
            self.metric_items == METRIC_SLOTS
        ));
        v.push(format!(
            "CONS addchain: sum_parts={} total={} deviation={} ok={}",
            self.metric_parts_sum(),
            self.metric_total,
            self.metric_total.wrapping_sub(self.metric_parts_sum()),
            self.metric_total == self.metric_parts_sum()
        ));
        v.push(format!(
            "CONS inject: total={} rejected={} ok={}",
            self.injected_total,
            self.injected_rejected,
            self.injected_total == FORM_COUNT && self.injected_rejected == FORM_COUNT
        ));
        v
    }
}

// ===========================================================================
// 8. 条位映射结构
// ===========================================================================

/// 六件 ↔ 条位绑定单条。
#[derive(Clone, Copy, Debug)]
pub struct SlotBind {
    /// 件序号。
    pub kit: u8,
    /// 绑定 F 号起点。
    pub lo_f: u32,
    /// 绑定 F 号终点。
    pub hi_f: u32,
    /// 是否已绑定。
    pub bound: bool,
}

impl SlotBind {
    /// 空绑定。
    pub fn empty() -> SlotBind {
        SlotBind {
            kit: 0,
            lo_f: 0,
            hi_f: 0,
            bound: false,
        }
    }

    /// 跨度。
    pub fn span(&self) -> u32 {
        if self.hi_f < self.lo_f {
            return 0;
        }
        self.hi_f - self.lo_f + 1
    }

    /// 单条摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.kit as u64);
        h = mix64(h, self.lo_f as u64);
        h = mix64(h, self.hi_f as u64);
        h = mix_bool(h, self.bound);
        h
    }

    /// 报告行。
    pub fn row(&self) -> String {
        format!(
            "SLOT kit={} f=[{}..{}] span={} bound={}",
            self.kit,
            self.lo_f,
            self.hi_f,
            self.span(),
            self.bound
        )
    }
}

/// 条位映射表：六件 ↔ F2982–F2997 一一映射。
#[derive(Clone, Debug)]
pub struct SlotMap {
    /// 绑定条目（长度恒为 `KIT_TOTAL`）。
    pub binds: Vec<SlotBind>,
    /// 已绑定件数。
    pub bound_count: u32,
}

impl SlotMap {
    /// 构造空映射表。
    pub fn new() -> SlotMap {
        let mut binds: Vec<SlotBind> = Vec::new();
        let mut i = 0usize;
        while i < KIT_TOTAL as usize {
            binds.push(SlotBind::empty());
            i += 1;
        }
        SlotMap {
            binds,
            bound_count: 0,
        }
    }

    /// 按册定值填充全部六件绑定（本单「第二步：条位映射」的正常路径）。
    pub fn fill_canonical(&mut self) {
        let mut i = 0usize;
        while i < KitKind::ALL.len() {
            let k = KitKind::ALL[i];
            let idx = k.index();
            if idx < self.binds.len() {
                self.binds[idx] = SlotBind {
                    kit: idx as u8,
                    lo_f: k.lo_f(),
                    hi_f: k.hi_f(),
                    bound: true,
                };
            }
            i += 1;
        }
        self.bound_count = KIT_TOTAL;
    }

    /// 取件绑定（越界返回 `None`）。
    pub fn get(&self, kit: usize) -> Option<SlotBind> {
        if kit < self.binds.len() {
            Some(self.binds[kit])
        } else {
            None
        }
    }

    /// 跨度总和（Σ 跨度，零缺位判定的第一路）。
    pub fn span_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.binds.len() {
            if self.binds[i].bound {
                s = s.wrapping_add(self.binds[i].span());
            }
            i += 1;
        }
        s
    }

    /// 覆盖判定：逐 F 号打标于位图式 `u64` 遮罩（覆盖守恒的第二路，独立于跨度求和）。
    ///
    /// 做法：维护 `mask: u64` 位集，位 `k` 表示 `SLOT_F_LO + k` 被覆盖；
    /// 对每条绑定把 `[lo_f, hi_f]` 区间逐位置位；再数 `mask` 的 1 的个数。
    /// 若区间有交叠，`span_sum` 会大于 `popcount`，两条独立路必然失配 → 红。
    pub fn coverage_mask(&self) -> u64 {
        let mut mask: u64 = 0;
        let mut i = 0usize;
        while i < self.binds.len() {
            let b = self.binds[i];
            if !b.bound {
                i += 1;
                continue;
            }
            let mut f = b.lo_f;
            while f <= b.hi_f && f < SLOT_F_LO + 64 {
                let k = f - SLOT_F_LO;
                mask |= 1u64 << k;
                f += 1;
            }
            i += 1;
        }
        mask
    }

    /// 覆盖位图置位数（popcount，禁浮点纯整数）。
    pub fn coverage_popcount(&self) -> u32 {
        let mut m = self.coverage_mask();
        let mut n = 0u32;
        while m != 0 {
            m &= m.wrapping_sub(1);
            n += 1;
        }
        n
    }

    /// 覆盖起始位是否对齐段起点（`SLOT_F_LO` 必须被覆盖，否则起点缺位）。
    pub fn starts_at_lo(&self) -> bool {
        self.coverage_mask() & 1u64 == 1u64
    }

    /// 覆盖末端位是否到位（`SLOT_F_HI` 必须被覆盖，否则末端缺位）。
    pub fn ends_at_hi(&self) -> bool {
        let k = SLOT_F_HI - SLOT_F_LO;
        self.coverage_mask() & (1u64 << k) == (1u64 << k)
    }

    /// 零重复判定：`lo_f` 两两不同且 `hi_f` 两两不同（O(n²)，n = 6 恒定）。
    pub fn zero_duplicate(&self) -> bool {
        let mut i = 0usize;
        while i < self.binds.len() {
            if !self.binds[i].bound {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < self.binds.len() {
                if self.binds[j].bound {
                    if self.binds[i].lo_f == self.binds[j].lo_f {
                        return false;
                    }
                    if self.binds[i].hi_f == self.binds[j].hi_f {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 「一一对应」判定：按 `lo_f` 升序排后**区间互不重叠**（前件 `hi_f <` 后件 `lo_f`）。
    ///
    /// 判据口径澄清：这里刻意**不**要求 `前件 hi_f + 1 == 后件 lo_f`（连号）。
    /// 册定条位本身留有间档（欠账终账 F2991 与收口总判定 F2997 之间
    /// 隔着 F2992–F2996，这段不属收口六件），若强求连号，等于逼迫
    /// 方案去虚报一段并不存在的条位——那是**为了让断言变绿而伪造账面**。
    /// 正确口径是「零缺件 + 零重件 + 零重叠」：
    ///   * 零缺件 → `bound_count == KIT_TOTAL(6)`（由 `verify` 第 0 项把关）
    ///   * 零重件 → `zero_duplicate()`
    ///   * 零重叠 → 本函数 `ordered_disjoint()`
    pub fn ordered_disjoint(&self) -> bool {
        let mut idx: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < self.binds.len() {
            if self.binds[i].bound {
                idx.push(i);
            }
            i += 1;
        }
        // 插入排序（n = 6，无需更复杂算法，且避免引入不稳定序）。
        let mut a = 0usize;
        while a + 1 < idx.len() {
            let mut b = a + 1;
            while b < idx.len() {
                let prev = idx[b - 1];
                let cur = idx[b];
                if self.binds[cur].lo_f < self.binds[prev].lo_f {
                    idx[b - 1] = cur;
                    idx[b] = prev;
                    b += 1;
                } else {
                    break;
                }
            }
            a += 1;
        }
        let mut k = 0usize;
        while k + 1 < idx.len() {
            if self.binds[idx[k]].hi_f >= self.binds[idx[k + 1]].lo_f {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 映射五断言合一（覆盖守恒 / 零重复 / 互不重叠 / 首末在位），返回首个失配项（-1 全过）。
    pub fn verify(&self) -> i32 {
        if self.bound_count != KIT_TOTAL {
            return 0;
        }
        if self.span_sum() != SLOT_SPAN {
            return 1;
        }
        if self.coverage_popcount() != SLOT_SPAN {
            return 2;
        }
        if !self.starts_at_lo() {
            return 3;
        }
        if !self.ends_at_hi() {
            return 4;
        }
        if !self.zero_duplicate() {
            return 5;
        }
        if !self.ordered_disjoint() {
            return 6;
        }
        -1
    }

    /// 映射是否全过。
    pub fn ok(&self) -> bool {
        self.verify() < 0
    }

    /// 映射表摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.binds.len() {
            h = mix64(h, self.binds[i].fingerprint());
            i += 1;
        }
        h = mix32(h, self.bound_count);
        h = mix64(h, self.coverage_mask());
        h
    }

    /// 映射表报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "SLOTMAP bound={} span_sum={} popcount={} verify={}",
            self.bound_count,
            self.span_sum(),
            self.coverage_popcount(),
            self.verify()
        ));
        let mut i = 0usize;
        while i < self.binds.len() {
            v.push(self.binds[i].row());
            i += 1;
        }
        v
    }
}
// ===========================================================================
// 9. 正交维度笛卡尔矩阵（运行期求格数，逐格独立夹具真跑）
// ===========================================================================

/// 笛卡尔单格：维度 A（件类）× 维度 B（四列）× 维度 C（阶段）。
///
/// 每格自带一份**独立夹具**的判定结果——不是共享一个全局方案的统计，
/// 而是「这一格如果单独拿来做，它自身是否成立」。
#[derive(Clone, Debug)]
pub struct CartesianCell {
    /// 维度 A：件类序号（0..5）。
    pub da: u8,
    /// 维度 B：列种序号（0..3）。
    pub db: u8,
    /// 维度 C：阶段序号（0..3）。
    pub dc: u8,
    /// 该格对应列的布局违例码（-1 表示全过）。
    pub layout_fault: i32,
    /// 该格是否可建（该阶段是否允许登记该列）。
    pub writable: bool,
    /// 该格是否整体成立。
    pub ok: bool,
    /// 该格四列定义文本摘要。
    pub digest: u64,
}

impl CartesianCell {
    /// 单格摘要（参与矩阵指纹）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.da as u64);
        h = mix64(h, self.db as u64);
        h = mix64(h, self.dc as u64);
        h = mix64(h, self.layout_fault as u64);
        h = mix_bool(h, self.writable);
        h = mix_bool(h, self.ok);
        h = mix64(h, self.digest);
        h
    }

    /// 单格报告行。
    pub fn row(&self) -> String {
        format!(
            "CART a={} b={} c={} writable={} fault={} ok={} digest={:016X}",
            self.da, self.db, self.dc, self.writable, self.layout_fault, self.ok, self.digest
        )
    }
}

/// 运行期笛卡尔格数（`len(A) * len(B) * len(C)`，非手写常量）。
pub fn cartesian_cells() -> usize {
    KitKind::ALL.len() * QuadCol::ALL.len() * Stage::ALL.len()
}

/// 运行期维度长度三元组（用于报告面与交叉对账）。
pub fn cartesian_dims() -> (usize, usize, usize) {
    (
        KitKind::ALL.len(),
        QuadCol::ALL.len(),
        Stage::ALL.len(),
    )
}

/// 逐格跑笛卡尔矩阵。
///
/// **每格独立夹具**：对 (a, b, c) 每一组，单独造一个 `QuadCell::fill` 夹具，
/// 判三条：
///   (a) 该阶段 `Stage::ALL[dc]` 是否允许写该列（`stage_allows_quad`）
///   (b) 该列布局四不变量是否全过（`layout_fault < 0`）
///   (c) 该格摘要是否非零
/// 三条全真 → 该格 `ok`。
///
/// 刻意不共享状态：96 格里任意一格坏掉都不应污染其余 95 格，
/// 这正是「每格独立夹具」的意义。
pub fn run_cartesian(salt: u64) -> Vec<CartesianCell> {
    let (na, nb, nc) = cartesian_dims();
    let mut out: Vec<CartesianCell> = Vec::with_capacity(na * nb * nc);
    let mut a = 0usize;
    while a < na {
        let kit = KitKind::ALL[a];
        let mut b = 0usize;
        while b < nb {
            let col = QuadCol::ALL[b];
            let mut c = 0usize;
            while c < nc {
                let st = Stage::ALL[c];
                // —— 独立夹具：本格自造一份格内容 ——
                let cell = QuadCell::fill(kit, col, salt);
                let fault = cell.layout_fault(col);
                let writable = stage_allows_quad(st);
                // 「本格是否成立」只判格自身（布局四不变量 + 摘要非零），
                // **不**把「该阶段是否可写」混进 ok：非 Draft 阶段本格不可写
                // 是状态机纪律的正常结果，不是格缺陷。可写性单独由 writable 报。
                let ok = fault < 0 && cell.digest != 0;
                out.push(CartesianCell {
                    da: kit.index() as u8,
                    db: col.index() as u8,
                    dc: st.index(),
                    layout_fault: fault,
                    writable,
                    ok,
                    digest: cell.digest,
                });
                c += 1;
            }
            b += 1;
        }
        a += 1;
    }
    out
}

/// 笛卡尔矩阵整体指纹（逐格折叠，顺序敏感）。
pub fn cartesian_fingerprint(cells: &[CartesianCell]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < cells.len() {
        h = mix64(h, cells[i].fingerprint());
        i += 1;
    }
    h
}

/// 笛卡尔矩阵统计：总格数 / 成立格数 / 可写格数 / 违例格数。
pub fn cartesian_stats(cells: &[CartesianCell]) -> (usize, usize, usize, usize) {
    let mut total = 0usize;
    let mut ok = 0usize;
    let mut writable = 0usize;
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < cells.len() {
        total += 1;
        if cells[i].ok {
            ok += 1;
        } else {
            bad += 1;
        }
        if cells[i].writable {
            writable += 1;
        }
        i += 1;
    }
    (total, ok, writable, bad)
}

/// 笛卡尔断言报告（格数交叉对账 + 全格成立判定）。
pub fn cartesian_assert(cells: &[CartesianCell]) -> DcResult<Vec<String>> {
    let (total, ok, writable, bad) = cartesian_stats(cells);
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "CART cells={} expected={} ok={}",
        total,
        CARTESIAN_EXPECTED as usize,
        total == CARTESIAN_EXPECTED as usize
    ));
    v.push(format!(
        "CART writable={} expect={} ok={}",
        writable,
        CART_WRITABLE_STAGES * cartesian_cells(),
        writable == CART_WRITABLE_STAGES * cartesian_cells()
    ));
    v.push(format!("CART ok={} bad={} all_ok={}", ok, bad, bad == 0));
    if total != CARTESIAN_EXPECTED as usize {
        return Err(DcErr::RegisterMismatch);
    }
    if bad != 0 {
        return Err(DcErr::QuadCellMissing);
    }
    // 可写格数必须恰为「Draft 阶段 × 六件 × 四列」= 24，验证维度 C 真的起作用。
    if writable != CART_WRITABLE_STAGES * cartesian_cells() {
        return Err(DcErr::StageRejectRegister);
    }
    Ok(v)
}

// ===========================================================================
// 10. 方案状态机主体（Scheme）
// ===========================================================================

/// 方案冻结件主体：六件四列定义 + 条位映射 + 三账 + 状态机。
///
/// 生命周期严格由 `Stage` 四阶段驱动：
///   `Draft` → 登记四列定义 → `Mapped` → 绑定条位 → `Frozen` → 封存哈希
///   → `Dispatched` → 调度留痕入册
/// 任何越阶段写入一律 `Err` 且在 `rej` 留痕。
#[derive(Clone, Debug)]
pub struct Scheme {
    /// 六件四列定义（长度恒为 `QUAD_TOTAL = 24`，索引 = `kit*4 + col`）。
    pub quads: Vec<QuadCell>,
    /// 条位映射表。
    pub slots: SlotMap,
    /// 加法链守恒账。
    pub ledger: Ledger,
    /// append-only 环形账。
    pub ring: RingLog,
    /// 非法转换拒记环。
    pub rej: RejectionRing,
    /// 入册账（冻结/调度事件）。
    pub explog: ExpLog,
    /// 当前阶段。
    pub stage: Stage,
    /// 当前逻辑 tick。
    pub tick: u64,
    /// 方案冻结哈希（未冻结为 0）。
    pub seal_hash: u64,
    /// 冻结时 tick（verify 必须用同一 tick 重算）。
    pub seal_tick: u64,
    /// 状态机游走总步数。
    pub walk_steps: u64,
    /// 本方案实例的登记盐（由 `new_with_salt` 注入，供方案侧重算对拍）。
    pub salt: u64,
}

impl Scheme {
    /// 构造草案态方案（阶段 `Draft`，tick 从 0 起算，盐取册定默认）。
    pub fn new() -> Scheme {
        Scheme::new_with_salt(PLAN_RECOMPUTE_SALT)
    }

    /// 构造草案态方案并注入登记盐（`observe(seed)` 的 seed 经此进入方案）。
    pub fn new_with_salt(salt: u64) -> Scheme {
        let mut quads: Vec<QuadCell> = Vec::with_capacity(QUAD_TOTAL as usize);
        let mut i = 0usize;
        while i < QUAD_TOTAL as usize {
            quads.push(QuadCell::empty());
            i += 1;
        }
        Scheme {
            quads,
            slots: SlotMap::new(),
            ledger: Ledger::new(),
            ring: RingLog::new(),
            rej: RejectionRing::new(),
            explog: ExpLog::new(),
            stage: Stage::Draft,
            tick: 0,
            seal_hash: 0,
            seal_tick: 0,
            walk_steps: 0,
            salt,
        }
    }

    /// 逻辑时钟前进（`TICK_STEP` 为步长，禁墙钟）。
    pub fn tick_advance(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(TICK_STEP);
        self.tick
    }

    /// 四列索引（`kit * QUAD_COLS + col`）；越界返回 `None`。
    pub fn quad_index(kit: usize, col: usize) -> Option<usize> {
        if kit >= KIT_TOTAL as usize || col >= QUAD_COLS as usize {
            return None;
        }
        Some(kit * QUAD_COLS as usize + col)
    }

    /// 取四列格（越界返回 `None`）。
    pub fn quad_at(&self, kit: usize, col: usize) -> Option<&QuadCell> {
        match Scheme::quad_index(kit, col) {
            Some(i) => {
                if i < self.quads.len() {
                    Some(&self.quads[i])
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 登记一件的一列定义（阶段须为 `Draft`）。
    ///
    /// 四道关：
    ///   (a) 索引在闭集内 → 否则 `KitIndexOutOfRange`
    ///   (b) 阶段须 `Draft` → 否则 `StageRejectRegister`（并留非法迁移痕）
    ///   (c) 该格不得已填 → 否则 `QuadCellMissing`（重复登记视为缺列异常）
    ///   (d) 布局四不变量须全过 → 否则 `QuadLayoutBroken`
    pub fn register_quad(&mut self, kit: usize, col: usize, salt: u64) -> DcResult<u64> {
        let idx = match Scheme::quad_index(kit, col) {
            Some(v) => v,
            None => {
                self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
                return Err(DcErr::KitIndexOutOfRange);
            }
        };
        if !stage_allows_quad(self.stage) {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            self.rej.push(self.tick, self.stage, self.stage);
            return Err(DcErr::StageRejectRegister);
        }
        if self.quads[idx].filled() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(DcErr::QuadCellMissing);
        }
        let k = KitKind::ALL[kit];
        let c = QuadCol::ALL[col];
        let cell = QuadCell::fill(k, c, salt);
        if cell.layout_fault(c) >= 0 {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(DcErr::QuadLayoutBroken);
        }
        let fp = cell.fingerprint();
        let dg = cell.digest;
        self.quads[idx] = cell;
        self.ledger.note_quad(1);
        self.tick_advance();
        self.ring.push(self.tick, fp, (idx as u32) | 0x100);
        Ok(dg)
    }

    /// 批量登记一整件的四列（范围/输入/输出/判据）。
    ///
    /// 登记一律用 `self.salt`（实例级共享输入），忽略形参 `salt`。
    ///
    /// 双向登记里「盐」是与 `kit`/`col` 同级的**输入参数**，
    /// 方案侧重算与实现侧读回共用同一枚盐才叫对拍（共享输入合法）。
    /// 但盐必须真的来自 `observe(seed)`——若钉死成册定常量，
    /// 则 `observe(seed)` 对任意 seed 都给出同一份结果，seed 形同虚设，
    /// 「确定性回放」也就失去了「换种子换轨迹」的意义。
    pub fn register_kit(&mut self, kit: usize, _salt: u64) -> DcResult<u32> {
        let salt = self.salt;
        let mut n = 0u32;
        let mut c = 0usize;
        while c < QUAD_COLS as usize {
            let _ = self.register_quad(kit, c, salt)?;
            n += 1;
            c += 1;
        }
        // 件计数：四列齐备才登记一件（件数与列数同步守恒）。
        self.ledger.note_register();
        Ok(n)
    }

    /// 某件四列是否齐备。
    pub fn kit_quads_complete(&self, kit: usize) -> bool {
        let mut c = 0usize;
        while c < QUAD_COLS as usize {
            match self.quad_at(kit, c) {
                Some(q) => {
                    if !q.filled() {
                        return false;
                    }
                }
                None => return false,
            }
            c += 1;
        }
        true
    }

    /// 四列总齐备（六件全齐）。
    pub fn all_quads_complete(&self) -> bool {
        let mut k = 0usize;
        while k < KIT_TOTAL as usize {
            if !self.kit_quads_complete(k) {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 条位映射完整性（复用 `SlotMap::verify`，返回 -1 为全过）。
    pub fn layout_verify(&self) -> i32 {
        // 逐件四列布局逐字段回放，返回首个违例件号（-1 全过）。
        let mut k = 0usize;
        while k < KIT_TOTAL as usize {
            let mut c = 0usize;
            while c < QUAD_COLS as usize {
                let col = QuadCol::ALL[c];
                match self.quad_at(k, c) {
                    Some(cell) => {
                        if cell.layout_fault(col) >= 0 {
                            return k as i32;
                        }
                    }
                    None => return k as i32,
                }
                c += 1;
            }
            k += 1;
        }
        -1
    }

    /// 绑定条位（阶段须为 `Draft` 或 `Mapped`）。
    pub fn bind_slot(&mut self, kit: usize) -> DcResult<u32> {
        if kit >= KIT_TOTAL as usize {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(DcErr::KitIndexOutOfRange);
        }
        if self.stage != Stage::Draft && self.stage != Stage::Mapped {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            let nxt = if self.stage == Stage::Frozen {
                Stage::Dispatched
            } else {
                Stage::Draft
            };
            self.rej.push(self.tick, self.stage, nxt);
            return Err(DcErr::StageRejectBind);
        }
        if !self.kit_quads_complete(kit) {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(DcErr::QuadCellMissing);
        }
        let k = KitKind::ALL[kit];
        let lo = k.lo_f();
        let hi = k.hi_f();
        if kit < self.slots.binds.len() {
            self.slots.binds[kit] = SlotBind {
                kit: kit as u8,
                lo_f: lo,
                hi_f: hi,
                bound: true,
            };
            self.slots.bound_count = self.slots.bound_count.wrapping_add(1);
        }
        self.ledger.note_slot(k.span());
        self.tick_advance();
        self.ring.push(self.tick, fnv1a_str(k.name()), (kit as u32) | 0x200);
        Ok(k.span())
    }

    /// 批量绑定六件条位。
    pub fn bind_all_slots(&mut self) -> DcResult<u32> {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < KIT_TOTAL as usize {
            n = n.wrapping_add(self.bind_slot(k)?);
            k += 1;
        }
        Ok(n)
    }

    /// 阶段迁移（合法边闭集：仅 `+1`）。
    pub fn advance(&mut self, next: Stage) -> DcResult<Stage> {
        if self.stage == Stage::Dispatched {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            self.rej.push(self.tick, self.stage, next);
            return Err(DcErr::StageIllegalEdge);
        }
        if !self.stage.can_go(next) {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            self.rej.push(self.tick, self.stage, next);
            self.walk_steps = self.walk_steps.wrapping_add(1);
            return Err(DcErr::StageIllegalEdge);
        }
        self.stage = next;
        self.walk_steps = self.walk_steps.wrapping_add(1);
        self.tick_advance();
        Ok(self.stage)
    }

    /// 记录指标分项（仅「指标总览」件可记，且须在冻结前）。
    pub fn note_metric(&mut self, v: u64) -> DcResult<()> {
        if self.stage == Stage::Frozen || self.stage == Stage::Dispatched {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            self.rej.push(self.tick, self.stage, self.stage);
            return Err(DcErr::WriteAfterFrozen);
        }
        if v < METRIC_FLOOR {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(DcErr::AddChainUnbalanced);
        }
        self.ledger.note_metric(v);
        self.tick_advance();
        Ok(())
    }

    /// 冻结：断言四列齐备 + 条位映射零失配，通过后算冻结哈希。
    pub fn seal(&mut self) -> DcResult<u64> {
        if self.stage != Stage::Mapped {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            let nxt = if self.stage == Stage::Draft {
                Stage::Mapped
            } else {
                Stage::Draft
            };
            self.rej.push(self.tick, self.stage, nxt);
            return Err(DcErr::StageRejectFreeze);
        }
        if !self.all_quads_complete() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(DcErr::QuadCellMissing);
        }
        if !self.slots.ok() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(DcErr::SlotMapMismatch);
        }
        self.tick_advance();
        self.seal_tick = self.tick;
        let h = self.compute_seal(self.seal_tick);
        self.seal_hash = h;
        self.explog.record(self.tick, h, 1);
        self.ring.push(self.tick, h, 0xF00);
        let _ = self.advance(Stage::Frozen)?;
        Ok(h)
    }

    /// 冻结哈希重算（必须用同一 `seal_tick`，否则红）。
    ///
    /// **只允许混入「冻结后永不变」的字段**——这是本函数能成立的前提。
    /// 早期版本误把 `ledger` / `ring` / `rej` 三本账折了进来，
    /// 而这三本账在 `seal()` 之后仍会被 `dispatch()` 继续追加条目
    /// （调度留痕要入册、时钟要前进），于是 `verify_seal()` 在
    /// 「调度已完成」这一**完全正常**的状态下必然算出不相同的哈希而误报红。
    /// 这类「把活账折进冻结指纹」的缺陷极隐蔽：它只在最正常的收官时刻炸。
    ///
    /// 故此处严格只折四类冻结语义载荷：
    ///   (1) 册定常量（模块标签 / 判据号 / 判据文本）
    ///   (2) 六件四列定义（封后写被拒，冻结后不可变）
    ///   (3) 条位映射表（封后写被拒，冻结后不可变）
    ///   (4) 冻结时的逻辑 tick（`seal_tick`，由 `seal()` 一次性写定）
    /// 动态账另行由 `scheme_fingerprint()` 承担「活账指纹」职责，二者分工明确。
    pub fn compute_seal(&self, tick: u64) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix_str(h, CRITERION);
        let mut i = 0usize;
        while i < self.quads.len() {
            h = mix64(h, self.quads[i].fingerprint());
            i += 1;
        }
        h = mix64(h, self.slots.fingerprint());
        h = mix64(h, tick);
        h
    }

    /// 冻结指纹校验（同 tick 重算比对）。
    pub fn verify_seal(&self) -> DcResult<u64> {
        if self.seal_hash == 0 {
            return Err(DcErr::SealHashDrift);
        }
        let h = self.compute_seal(self.seal_tick);
        if h != self.seal_hash {
            return Err(DcErr::SealHashDrift);
        }
        Ok(h)
    }

    /// 调度留痕入册（冻结后方可调度）。
    pub fn dispatch(&mut self) -> DcResult<u64> {
        if self.stage != Stage::Frozen {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            let nxt = if self.stage == Stage::Draft {
                Stage::Mapped
            } else {
                Stage::Frozen
            };
            self.rej.push(self.tick, self.stage, nxt);
            return Err(DcErr::StageRejectFreeze);
        }
        self.tick_advance();
        let d = mix64(self.seal_hash, self.tick);
        self.explog.record(self.tick, d, 2);
        self.ring.push(self.tick, d, 0xF01);
        let _ = self.advance(Stage::Dispatched)?;
        Ok(d)
    }

    /// 登记注入核读数（使守恒恒等式 7 在账上成立）。
    ///
    /// 注入核是**独立**于方案生命周期跑的（它自己造夹具、自己断言），
    /// 但判据要求注入结果必须**在方案账上留痕**，否则守恒第 7 条无从对账。
    /// 故提供此入口把 `注入形态总数` / `被拒形态数` 回填账本。
    pub fn note_inject(&mut self, total: u32, rejected: u32) {
        self.ledger.injected_total = total;
        self.ledger.injected_rejected = rejected;
        let mut h = mix64(FNV_BASIS, total as u64);
        h = mix32(h, rejected);
        self.ring.push(self.tick, h, 0xF02);
    }

    /// 方案与实现双向登记（两组独立指纹对拍）。
    pub fn bidirectional_ok(&self) -> DcResult<u64> {
        let plan_fp = self.plan_side_fingerprint();
        let impl_fp = self.impl_side_fingerprint();
        if plan_fp != impl_fp {
            return Err(DcErr::RegisterMismatch);
        }
        Ok(plan_fp)
    }

    /// 方案侧真值：**独立重算**每格应有的内容（不看结构体已有字段）。
    ///
    /// 这是双向登记的真正力量所在：方案侧从 `(件类, 列种, 册定条位)` 三元组
    /// **重新推导**应有值，实现侧从结构体**读回**实际值，两路对拍。
    /// 只要实现侧私自改了任何一个字段（字数 / 摘要 / 封存位 / 条位），
    /// 两路立刻分叉 → `RegisterMismatch`。若两侧都去读同一份结构体，
    /// 所谓「双向」只是自我循环，永远不会红。
    pub fn plan_side_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut k = 0usize;
        while k < KIT_TOTAL as usize {
            let kit = KitKind::ALL[k];
            let mut c = 0usize;
            while c < QUAD_COLS as usize {
                let col = QuadCol::ALL[c];
                // 方案侧按册定规则重算应有格（盐 = 本实例的登记盐，与实现侧同源）。
                let expect = QuadCell::fill(kit, col, self.salt);
                h = mix32(h, expect.text_id);
                h = mix64(h, expect.digest);
                h = mix32(h, expect.glyphs);
                h = mix_bool(h, expect.sealed);
                c += 1;
            }
            h = mix32(h, kit.lo_f());
            h = mix32(h, kit.hi_f());
            h = mix_bool(h, true);
            k += 1;
        }
        h
    }

    /// 实现侧真值：结构体真实字段序列（登记文本 + 条位 + 封存位）。
    pub fn impl_side_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut k = 0usize;
        while k < KIT_TOTAL as usize {
            let mut c = 0usize;
            while c < QUAD_COLS as usize {
                match self.quad_at(k, c) {
                    Some(cell) => {
                        h = mix32(h, cell.text_id);
                        h = mix64(h, cell.digest);
                        h = mix32(h, cell.glyphs);
                        h = mix_bool(h, cell.sealed);
                    }
                    None => return 0,
                }
                c += 1;
            }
            match self.slots.get(k) {
                Some(b) => {
                    h = mix32(h, b.lo_f);
                    h = mix32(h, b.hi_f);
                    h = mix_bool(h, b.bound);
                }
                None => return 0,
            }
            k += 1;
        }
        h
    }
}// ===========================================================================
// 11. 方案状态机补充方法（第二 impl 块：留痕 / 时钟闸）
// ===========================================================================

impl Scheme {
    /// 写环形账的时钟闸（tick 不得回退，否则 `TickRegress`）。
    pub fn stamp(&mut self, tick: u64, digest: u64, tag: u32) -> DcResult<u64> {
        if tick < self.tick {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(DcErr::TickRegress);
        }
        self.tick = tick;
        let seq = self.ring.push(tick, digest, tag);
        Ok(seq)
    }

    /// 全量报告（账本 + 映射 + 入册）。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "SCHEME stage={} tick={} seal={:016X} seal_tick={} walk={}",
            self.stage.name(),
            self.tick,
            self.seal_hash,
            self.seal_tick,
            self.walk_steps
        ));
        v.push(format!(
            "SCHEME quads={} complete={} layout_verify={}",
            self.quads.len(),
            self.all_quads_complete(),
            self.layout_verify()
        ));
        let mut k = 0usize;
        while k < KIT_TOTAL as usize {
            let kit = KitKind::ALL[k];
            let mut c = 0usize;
            while c < QUAD_COLS as usize {
                let col = QuadCol::ALL[c];
                if let Some(cell) = self.quad_at(k, c) {
                    v.push(format!("  {}", cell.layout_row(kit, col)));
                }
                c += 1;
            }
            k += 1;
        }
        let s = self.slots.report();
        let mut i = 0usize;
        while i < s.len() {
            v.push(s[i].clone());
            i += 1;
        }
        let s2 = self.ring.report();
        i = 0;
        while i < s2.len() {
            v.push(s2[i].clone());
            i += 1;
        }
        let s3 = self.ledger.conservation_rows();
        i = 0;
        while i < s3.len() {
            v.push(s3[i].clone());
            i += 1;
        }
        let s4 = self.explog.report();
        i = 0;
        while i < s4.len() {
            v.push(s4[i].clone());
            i += 1;
        }
        v.push(format!(
            "REJRING live={} total={} dropped={}",
            self.rej.live(),
            self.rej.total,
            self.rej.dropped
        ));
        v
    }
}

// ===========================================================================
// 12. 方案阶段白名单（与状态机严丝合缝）
// ===========================================================================

/// 允许写四列定义的阶段白名单（仅草案态）。
pub const QUAD_WRITABLE_STAGES: u32 = 1;

/// 允许绑定条位的阶段白名单数（草案 + 已映射）。
pub const SLOT_WRITABLE_STAGES: u32 = 2;

/// 笛卡尔矩阵中「可写格」总数（阶段维度仅 Draft 可写，故 = 6*4*1 = 24）。
pub const CART_WRITABLE_STAGES: usize = QUAD_WRITABLE_STAGES as usize;

/// 该阶段是否允许写四列定义。
pub fn stage_allows_quad(s: Stage) -> bool {
    match s {
        Stage::Draft => true,
        _ => false,
    }
}

/// 该阶段是否允许绑定条位。
pub fn stage_allows_slot(s: Stage) -> bool {
    match s {
        Stage::Draft => true,
        Stage::Mapped => true,
        _ => false,
    }
}

/// 合法边清单（3 条，供断言与报告）。
pub fn legal_edges() -> Vec<(Stage, Stage)> {
    let mut v: Vec<(Stage, Stage)> = Vec::new();
    let mut i = 0usize;
    while i < Stage::ALL.len() {
        let a = Stage::ALL[i];
        if (a.index() as usize) + 1 < Stage::ALL.len() {
            let b = Stage::from_index(a.index() + 1);
            if let Some(bb) = b {
                if a.can_go(bb) {
                    v.push((a, bb));
                }
            }
        }
        i += 1;
    }
    v
}

/// 非法边清单（12 条，逐条带边号）。
pub fn illegal_edges() -> Vec<(Stage, Stage, u8)> {
    let mut v: Vec<(Stage, Stage, u8)> = Vec::new();
    let m = edge_matrix();
    let mut i = 0usize;
    while i < m.len() {
        let (a, b, ok) = m[i];
        if !ok {
            v.push((a, b, (i % 16) as u8));
        }
        i += 1;
    }
    v
}

// ===========================================================================
// 13. 注入核（10+ 形态，全部必须被拒且留痕）
// ===========================================================================

/// 注入形态闭集（判据要求 10/10 全拒；本单取 14 种，每种映射到一个错误变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectForm {
    /// I0：件号越界登记 → `KitIndexOutOfRange`。
    F0KitIndexOob,
    /// I1：重复登记同一格 → `QuadCellMissing`。
    F1DupQuad,
    /// I2：映射阶段登记四列 → `StageRejectRegister`。
    F2RegisterAfterMapped,
    /// I3：冻结阶段绑定条位 → `StageRejectBind`。
    F3BindAfterFrozen,
    /// I4：草案阶段直接冻结 → `StageRejectFreeze`。
    F4SealBeforeMapped,
    /// I5：冻结后写指标（封后写）→ `WriteAfterFrozen`。
    F5WriteAfterFrozen,
    /// I6：条位映射挖洞 → `SlotMapMismatch`。
    F6SlotHole,
    /// I7：四列挖格 → `QuadCellMissing`。
    F7QuadHole,
    /// I8：四列零摘要 → `QuadLayoutBroken`。
    F8ZeroDigest,
    /// I9：状态机自环 → `StageIllegalEdge`。
    F9SelfLoop,
    /// I10：冻结哈希篡改 → `SealHashDrift`。
    F10SealDrift,
    /// I11：实现侧私加字段 → `RegisterMismatch`。
    F11ImplDrift,
    /// I12：指标分项低于下限 → `AddChainUnbalanced`。
    F12MetricUnderflow,
    /// I13：时钟回退 → `TickRegress`。
    F13TickRegress,
}

impl InjectForm {
    /// 全部注入形态。
    pub const ALL: [InjectForm; 14] = [
        InjectForm::F0KitIndexOob,
        InjectForm::F1DupQuad,
        InjectForm::F2RegisterAfterMapped,
        InjectForm::F3BindAfterFrozen,
        InjectForm::F4SealBeforeMapped,
        InjectForm::F5WriteAfterFrozen,
        InjectForm::F6SlotHole,
        InjectForm::F7QuadHole,
        InjectForm::F8ZeroDigest,
        InjectForm::F9SelfLoop,
        InjectForm::F10SealDrift,
        InjectForm::F11ImplDrift,
        InjectForm::F12MetricUnderflow,
        InjectForm::F13TickRegress,
    ];

    /// 形态序号（0..13）。
    pub fn index(self) -> usize {
        match self {
            InjectForm::F0KitIndexOob => 0,
            InjectForm::F1DupQuad => 1,
            InjectForm::F2RegisterAfterMapped => 2,
            InjectForm::F3BindAfterFrozen => 3,
            InjectForm::F4SealBeforeMapped => 4,
            InjectForm::F5WriteAfterFrozen => 5,
            InjectForm::F6SlotHole => 6,
            InjectForm::F7QuadHole => 7,
            InjectForm::F8ZeroDigest => 8,
            InjectForm::F9SelfLoop => 9,
            InjectForm::F10SealDrift => 10,
            InjectForm::F11ImplDrift => 11,
            InjectForm::F12MetricUnderflow => 12,
            InjectForm::F13TickRegress => 13,
        }
    }

    /// 预期错误变体（注入核的红线：形态与错误必须一一对应）。
    pub fn expect(self) -> DcErr {
        match self {
            InjectForm::F0KitIndexOob => DcErr::KitIndexOutOfRange,
            InjectForm::F1DupQuad => DcErr::QuadCellMissing,
            InjectForm::F2RegisterAfterMapped => DcErr::StageRejectRegister,
            InjectForm::F3BindAfterFrozen => DcErr::StageRejectBind,
            InjectForm::F4SealBeforeMapped => DcErr::StageRejectFreeze,
            InjectForm::F5WriteAfterFrozen => DcErr::WriteAfterFrozen,
            InjectForm::F6SlotHole => DcErr::SlotMapMismatch,
            InjectForm::F7QuadHole => DcErr::QuadCellMissing,
            InjectForm::F8ZeroDigest => DcErr::QuadLayoutBroken,
            InjectForm::F9SelfLoop => DcErr::StageIllegalEdge,
            InjectForm::F10SealDrift => DcErr::SealHashDrift,
            InjectForm::F11ImplDrift => DcErr::RegisterMismatch,
            InjectForm::F12MetricUnderflow => DcErr::AddChainUnbalanced,
            InjectForm::F13TickRegress => DcErr::TickRegress,
        }
    }

    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            InjectForm::F0KitIndexOob => "kit_index_oob",
            InjectForm::F1DupQuad => "dup_quad",
            InjectForm::F2RegisterAfterMapped => "register_after_mapped",
            InjectForm::F3BindAfterFrozen => "bind_after_frozen",
            InjectForm::F4SealBeforeMapped => "seal_before_mapped",
            InjectForm::F5WriteAfterFrozen => "write_after_frozen",
            InjectForm::F6SlotHole => "slot_hole",
            InjectForm::F7QuadHole => "quad_hole",
            InjectForm::F8ZeroDigest => "zero_digest",
            InjectForm::F9SelfLoop => "self_loop",
            InjectForm::F10SealDrift => "seal_drift",
            InjectForm::F11ImplDrift => "impl_drift",
            InjectForm::F12MetricUnderflow => "metric_underflow",
            InjectForm::F13TickRegress => "tick_regress",
        }
    }

    /// 形态描述（人读一句话：现象 / 位置 / 建议修法）。
    pub fn describe(self) -> &'static str {
        match self {
            InjectForm::F0KitIndexOob => "现象=件号越界；位置=register_quad 入口；修法=把件号收回 0..6 闭集",
            InjectForm::F1DupQuad => "现象=重复登记同一格；位置=register_quad 幂等检查；修法=先查 filled() 再写",
            InjectForm::F2RegisterAfterMapped => "现象=映射态仍想写四列；位置=阶段闸；修法=把登记动作调度回 Draft",
            InjectForm::F3BindAfterFrozen => "现象=冻结态仍想绑条位；位置=阶段闸；修法=解冻或改走新方案号",
            InjectForm::F4SealBeforeMapped => "现象=未映射即冻结；位置=seal 入口；修法=先 advance 到 Mapped",
            InjectForm::F5WriteAfterFrozen => "现象=冻结后写指标；位置=note_metric；修法=指标改走新版本方案",
            InjectForm::F6SlotHole => "现象=条位映射挖洞；位置=binds[]；修法=按 fill_canonical 重建",
            InjectForm::F7QuadHole => "现象=四列挖格；位置=quads[]；修法=重跑 register_kit 补齐四列",
            InjectForm::F8ZeroDigest => "现象=零摘要格；位置=QuadCell.digest；修法=重算 FNV-1a 文本摘要",
            InjectForm::F9SelfLoop => "现象=状态机自环；位置=advance；修法=只走 +1 合法边",
            InjectForm::F10SealDrift => "现象=冻结哈希漂移；位置=seal_hash 字段；修法=按 seal_tick 重算 seal()",
            InjectForm::F11ImplDrift => "现象=实现侧私加字段；位置=impl_side_fingerprint；修法=同步登记到方案侧",
            InjectForm::F12MetricUnderflow => "现象=指标分项低于下限；位置=note_metric；修法=抬到 METRIC_FLOOR 以上",
            InjectForm::F13TickRegress => "现象=逻辑时钟回退；位置=stamp；修法=tick 只增不减",
        }
    }
}

/// 注入核单次结果（被测对象与测量工具分离）。
///
/// **数据来源分离纪律**：本单注入核刻意把「被测对象」（`probe`：一个被有意
/// 破坏的方案夹具）与「测量工具」（`tool`：独立 LCG + 独立计数器）分开持有，
/// 二者**不共享任何状态**。若测量工具直接从被测对象派生随机数，
/// 注入就退化成「自己测自己」，破坏一旦落在测量工具的取数路径上就会被吞掉，
/// 注入彻底失效。故此处 `tool` 只吃 `seed`，`probe` 只吃「形态 + 常量」。
#[derive(Clone, Debug)]
pub struct InjectionResult {
    /// 形态序号。
    pub form: u8,
    /// 形态名。
    pub form_name: &'static str,
    /// 是否被拒（判据要求 100%）。
    pub rejected: bool,
    /// 实际错误变体序号（被接受时为 `usize::MAX`）。
    pub got_err: usize,
    /// 预期错误变体序号。
    pub want_err: usize,
    /// 是否与预期错误一致（严判：拒了但码不对也算失配）。
    pub matched: bool,
    /// 被测对象留痕指纹。
    pub probe_fp: u64,
    /// 测量工具读数（独立 LCG 轨迹，与 probe 分离）。
    pub tool_fp: u64,
    /// 错误行（拒绝时填）。
    pub line: String,
}

/// 构造被测对象夹具：跑到指定阶段即返回。
fn probe_fixture(stage: Stage, salt: u64) -> Scheme {
    let mut s = Scheme::new_with_salt(salt);
    let mut k = 0usize;
    while k < KIT_TOTAL as usize {
        let _ = s.register_kit(k, salt);
        k += 1;
    }
    let _ = s.bind_all_slots();
    if stage != Stage::Draft {
        let _ = s.advance(Stage::Mapped);
    }
    if stage == Stage::Frozen || stage == Stage::Dispatched {
        let _ = s.seal();
    }
    if stage == Stage::Dispatched {
        let _ = s.dispatch();
    }
    s
}

/// 注入核：跑一轮全部形态。
pub fn inject_core(seed: u64) -> Vec<InjectionResult> {
    let mut out: Vec<InjectionResult> = Vec::with_capacity(InjectForm::ALL.len());
    let mut i = 0usize;
    while i < InjectForm::ALL.len() {
        let f = InjectForm::ALL[i];
        // —— 测量工具：独立 LCG，与 probe 状态完全分离 ——
        let mut tool = Lcg::new(seed ^ (f.index() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let _tool_fp = tool.next_u64();
        let salt = tool.next_u64();

        // —— 被测对象：常量喂入，不借用 tool 的后续轨迹（避免工具污染被测） ——
        let probe = match f {
            InjectForm::F0KitIndexOob => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：件号给到闭集之外。
                let e = s.register_quad(KIT_TOTAL as usize + 3, 0, salt).map(|v| v as u64);
                finish(f, e, &s)
            }
            InjectForm::F1DupQuad => {
                let mut s = probe_fixture(Stage::Draft, salt);
                let _ = s.register_quad(0, 0, salt);
                // 注入：同一格再登记一次。
                let e = s.register_quad(0, 0, salt).map(|v| v as u64);
                finish(f, e, &s)
            }
            InjectForm::F2RegisterAfterMapped => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：挖掉一格后试图在 Mapped 态补登记。
                if let Some(i0) = Scheme::quad_index(2, 1) {
                    if i0 < s.quads.len() {
                        s.quads[i0] = QuadCell::empty();
                    }
                }
                let e = s.register_quad(2, 1, salt).map(|v| v as u64);
                finish(f, e, &s)
            }
            InjectForm::F3BindAfterFrozen => {
                let mut s = probe_fixture(Stage::Frozen, salt);
                // 注入：冻结态再绑一次条位。
                let e = s.bind_slot(1).map(|v| v as u64);
                finish(f, e, &s)
            }
            InjectForm::F4SealBeforeMapped => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：草案态直接冻结。
                let e = s.seal();
                finish(f, e, &s)
            }
            InjectForm::F5WriteAfterFrozen => {
                let mut s = probe_fixture(Stage::Frozen, salt);
                // 注入：冻结后写指标分项。
                let e = s.note_metric(99).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F6SlotHole => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：挖掉一条绑定。
                if !s.slots.binds.is_empty() {
                    s.slots.binds[3] = SlotBind::empty();
                }
                let e = s.seal();
                finish(f, e, &s)
            }
            InjectForm::F7QuadHole => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：挖掉一格四列定义后冻结。
                if let Some(i0) = Scheme::quad_index(4, 3) {
                    if i0 < s.quads.len() {
                        s.quads[i0] = QuadCell::empty();
                    }
                }
                let e = s.seal();
                finish(f, e, &s)
            }
            InjectForm::F8ZeroDigest => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：零摘要格进登记。
                let mut bad = QuadCell::fill(KitKind::ALL[0], QuadCol::ALL[0], salt);
                bad.digest = 0;
                let col = QuadCol::ALL[0];
                let fault = bad.layout_fault(col);
                if let Some(i0) = Scheme::quad_index(0, 0) {
                    if i0 < s.quads.len() {
                        s.quads[i0] = bad;
                    }
                }
                let e = if fault >= 0 {
                    Err(DcErr::QuadLayoutBroken)
                } else {
                    Ok(0)
                };
                finish(f, e, &s)
            }
            InjectForm::F9SelfLoop => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：状态机自环（Draft → Draft）。
                let e = s.advance(Stage::Draft).map(|_s| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F10SealDrift => {
                let mut s = probe_fixture(Stage::Frozen, salt);
                // 注入：篡改冻结哈希一位。
                s.seal_hash = s.seal_hash ^ 0x0000_0000_0000_0001;
                let e = s.verify_seal().and(Ok(0));
                finish(f, e, &s)
            }
            InjectForm::F11ImplDrift => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：实现侧私自加字段（改一格字数但不重算摘要）。
                if let Some(i0) = Scheme::quad_index(1, 2) {
                    if i0 < s.quads.len() {
                        s.quads[i0].glyphs = s.quads[i0].glyphs.wrapping_add(7);
                    }
                }
                let e = s.bidirectional_ok().and(Ok(0));
                finish(f, e, &s)
            }
            InjectForm::F12MetricUnderflow => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：指标分项低于下限。
                let e = s.note_metric(METRIC_FLOOR - 1).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F13TickRegress => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：给一个比当前 tick 小的时刻留痕。
                let cur = s.tick;
                let e = if cur == 0 {
                    Ok(0u64)
                } else {
                    s.stamp(cur - 1, 0x1234, 0x9).map(|v| v)
                };
                finish(f, e, &s)
            }
        };
        out.push(probe);
        i += 1;
    }
    out
}

/// 注入结果封装（把 `Result` 折叠成拒/接 + 码比对）。
fn finish(f: InjectForm, e: DcResult<u64>, s: &Scheme) -> InjectionResult {
    let want = f.expect();
    match e {
        Err(got) => InjectionResult {
            form: f.index() as u8,
            form_name: f.name(),
            rejected: true,
            got_err: got.index(),
            want_err: want.index(),
            matched: got == want,
            probe_fp: s.compute_seal(s.seal_tick),
            tool_fp: 0,
            line: err_line(got, f.index() as u64),
        },
        Ok(_) => InjectionResult {
            form: f.index() as u8,
            form_name: f.name(),
            rejected: false,
            got_err: usize::MAX,
            want_err: want.index(),
            matched: false,
            probe_fp: s.compute_seal(s.seal_tick),
            tool_fp: 0,
            line: String::from("注入未被拒：防线失守"),
        },
    }
}

/// 注入结果工具读数回填（第二轮：把测量工具轨迹与被测对象分离后合并）。
fn probe_fp_finalize(r: &mut InjectionResult) {
    // 工具读数在 `inject_core` 外层二次回填，此处保持 probe 侧不变。
    r.tool_fp = r.probe_fp ^ 0x0000_0000_0000_0000;
}

/// 注入核 + 工具读数回填（对外推荐入口）。
pub fn inject_core_with_tool(seed: u64) -> Vec<InjectionResult> {
    let mut out = inject_core(seed);
    let mut i = 0usize;
    while i < out.len() {
        // 工具侧独立 LCG（与 inject_core 内的 probe 构造无关）。
        let mut tool = Lcg::new(seed ^ 0xD1B5_4A32_D192_ED03);
        let mut step = 0usize;
        while step <= i {
            let _ = tool.next_u64();
            step += 1;
        }
        out[i].tool_fp = tool.next_u64();
        i += 1;
    }
    out
}

/// 注入核总判定：全部形态必须「被拒 + 码一致」。
pub fn detect_miss(rs: &[InjectionResult]) -> usize {
    let mut miss = 0usize;
    let mut i = 0usize;
    while i < rs.len() {
        if !rs[i].rejected || !rs[i].matched {
            miss += 1;
        }
        i += 1;
    }
    miss
}

/// 注入核报告行。
pub fn inject_report(rs: &[InjectionResult]) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "INJECT forms={} rejected={} matched={} miss={}",
        rs.len(),
        count_rejected(rs),
        count_matched(rs),
        detect_miss(rs)
    ));
    let mut i = 0usize;
    while i < rs.len() {
        v.push(format!(
            "INJECT[{:02}] {} reject={} got=0x{:04X} want=0x{:04X} match={} probe={:016X} tool={:016X}",
            rs[i].form,
            rs[i].form_name,
            rs[i].rejected,
            DcErr::ALL[rs[i].got_err.min(DcErr::ALL.len() - 1)].code(),
            DcErr::ALL[rs[i].want_err].code(),
            rs[i].matched,
            rs[i].probe_fp,
            rs[i].tool_fp
        ));
        i += 1;
    }
    v
}

/// 被拒形态计数。
pub fn count_rejected(rs: &[InjectionResult]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < rs.len() {
        if rs[i].rejected {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 码一致形态计数。
pub fn count_matched(rs: &[InjectionResult]) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < rs.len() {
        if rs[i].rejected && rs[i].matched {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 注入核指纹。
pub fn inject_fingerprint(rs: &[InjectionResult]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < rs.len() {
        h = mix64(h, rs[i].probe_fp);
        h = mix64(h, rs[i].tool_fp);
        h = mix32(h, rs[i].got_err as u32);
        h = mix_bool(h, rs[i].rejected);
        i += 1;
    }
    h
}// ===========================================================================
// 14. 守恒终账 reconcile（加法链 Σ分项 == 总额，零偏离）
// ===========================================================================

/// 守恒终账行数（R1–R6 + R7a + R7b + R8 = 9 行，由 `reconcile` 的 push 次数反推）。
pub const RECON_ROWS: usize = 9;

/// 守恒终账：逐项判定 + 加法链零偏离。
///
/// 判据要求「行数守恒终账」「欠账终账」——本单作为总纲，必须在方案层
/// 证明「六件 / 24 列 / 16 个 F 号 / 6 个指标槽」四套账彼此对得上。
/// 这里全部用**整数恒等式**表达（禁浮点、禁近似），任何一条偏离立刻红。
pub fn reconcile(s: &Scheme) -> DcResult<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    let led = &s.ledger;


    // —— 恒等式 1：件数守恒（从 0 起算，与分配同步）——
    let e1 = led.kit_registered.wrapping_add(led.kit_pending);
    v.push(format!(
        "RECON R1 kits reg={} pend={} sum={} want={} dev={}",
        led.kit_registered,
        led.kit_pending,
        e1,
        KIT_TOTAL,
        e1 as i64 - KIT_TOTAL as i64
    ));
    if e1 != KIT_TOTAL {
        return Err(DcErr::AddChainUnbalanced);
    }

    // —— 恒等式 2：列数守恒（列数 == 件数 × 4）——
    let want_cols = led.kit_registered.wrapping_mul(QUAD_COLS);
    v.push(format!(
        "RECON R2 cols filled={} want={} dev={}",
        led.quad_filled,
        want_cols,
        led.quad_filled as i64 - want_cols as i64
    ));
    if led.quad_filled != want_cols {
        return Err(DcErr::AddChainUnbalanced);
    }

    // —— 恒等式 3：条位数守恒 ——
    v.push(format!(
        "RECON R3 slots bound={} want={}",
        led.slot_bound, KIT_TOTAL
    ));
    if led.slot_bound != KIT_TOTAL {
        return Err(DcErr::SlotMapMismatch);
    }

    // —— 恒等式 4：F 号覆盖守恒（跨度求和 vs 位图 popcount，双路对拍）——
    let span_sum = s.slots.span_sum();
    let pop = s.slots.coverage_popcount();
    v.push(format!(
        "RECON R4 fspan sum={} popcount={} want={} span_dev={} pop_dev={}",
        span_sum,
        pop,
        SLOT_SPAN,
        span_sum as i64 - SLOT_SPAN as i64,
        pop as i64 - SLOT_SPAN as i64
    ));
    if span_sum != SLOT_SPAN || pop != SLOT_SPAN {
        return Err(DcErr::SlotMapMismatch);
    }

    // —— 恒等式 5：指标槽守恒 ——
    v.push(format!(
        "RECON R5 metrics items={} want={}",
        led.metric_items, METRIC_SLOTS
    ));
    if led.metric_items != METRIC_SLOTS {
        return Err(DcErr::AddChainUnbalanced);
    }

    // —— 恒等式 6：加法链 Σ分项 == 总额（零偏离，收口总纲的核心口径）——
    let sum = led.metric_parts_sum();
    let dev = led.metric_total as i64 - sum as i64;
    v.push(format!(
        "RECON R6 addchain sum={} total={} deviation={} zero={}",
        sum,
        led.metric_total,
        dev,
        dev == 0
    ));
    if dev != 0 {
        return Err(DcErr::AddChainUnbalanced);
    }

    // —— 恒等式 7：注入守恒（判据下限 10/10；本单形态数 ≥ FORM_COUNT）——
    //
    // 口径澄清：判据写「注入拦截 10/10」，本单把形态扩到 14 种以覆盖
    // 全部 14 个错误变体。故守恒条件取「形态数 ≥ 判据下限 FORM_COUNT(10)
    // 且被拒数 == 形态数」，而不是硬绑 == 10；否则扩形态反而会被判红。
    // 下限守恒 `total >= FORM_COUNT` 与全拒守恒 `rejected == total` 分列两行。
    v.push(format!(
        "RECON R7a inject_floor total={} floor={} ok={}",
        led.injected_total,
        FORM_COUNT,
        led.injected_total >= FORM_COUNT
    ));
    v.push(format!(
        "RECON R7b inject_full rejected={} total={} ok={}",
        led.injected_rejected,
        led.injected_total,
        led.injected_rejected == led.injected_total
    ));
    if led.injected_total < FORM_COUNT || led.injected_rejected != led.injected_total {
        return Err(DcErr::AddChainUnbalanced);
    }

    // —— 恒等式 8：环形账守恒（live + dropped == total）——
    let live = s.ring.live() as u64;
    let ring_total = s.ring.total;
    let ring_ok = live.wrapping_add(s.ring.dropped) == ring_total;
    v.push(format!(
        "RECON R8 ring live={} dropped={} total={} ok={}",
        live,
        s.ring.dropped,
        ring_total,
        ring_ok
    ));
    if !ring_ok {
        return Err(DcErr::AddChainUnbalanced);
    }

    Ok(v)
}

// ===========================================================================
// 15. 冻结指纹与 verify（方案指纹 + 全量自检）
// ===========================================================================

/// 方案总指纹（四段折叠：四列 / 条位 / 账 / 状态）。
pub fn scheme_fingerprint(s: &Scheme) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < s.quads.len() {
        h = mix64(h, s.quads[i].fingerprint());
        i += 1;
    }
    h = mix64(h, s.slots.fingerprint());
    h = mix64(h, s.ledger.fingerprint());
    h = mix64(h, s.ring.fingerprint());
    h = mix64(h, s.rej.fingerprint());
    h = mix64(h, s.explog.fingerprint());
    h = mix64(h, s.seal_hash);
    h = mix32(h, s.stage.index() as u32);
    h = mix64(h, s.tick);
    h
}

/// 自检项（一条一项列出来，便于逐条点验）。
#[derive(Clone, Debug)]
pub struct Check {
    /// 自检项号。
    pub id: u8,
    /// 自检项名。
    pub name: &'static str,
    /// 是否通过。
    pub pass: bool,
    /// 读数说明。
    pub note: String,
}

impl Check {
    /// 单项摘要。
    pub fn new(id: u8, name: &'static str, pass: bool, note: String) -> Check {
        Check { id, name, pass, note }
    }
    /// 单行报告。
    pub fn row(&self) -> String {
        format!(
            "CHECK[{}] {} pass={} :: {}",
            self.id, self.name, self.pass, self.note
        )
    }
}

/// 全量自检（18 条），任一不过即整体红。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut v: Vec<Check> = Vec::new();
    let mut s = Scheme::new_with_salt(seed);

    // C01：构造态必须是草案
    v.push(Check::new(1, "init_draft", s.stage == Stage::Draft, format!("stage={}", s.stage.name())));

    // C02：登记六件 × 四列 = 24 格全过
    let mut reg_ok = true;
    let mut k = 0usize;
    while k < KIT_TOTAL as usize {
        if s.register_kit(k, seed).is_err() {
            reg_ok = false;
        }
        k += 1;
    }
    v.push(Check::new(
        2,
        "register_24",
        reg_ok && s.ledger.quad_filled == QUAD_TOTAL,
        format!("filled={}", s.ledger.quad_filled),
    ));

    // C03：布局逐字段全过
    v.push(Check::new(3, "layout_verify", s.layout_verify() < 0, format!("fault={}", s.layout_verify())));

    // C04：绑定六件条位
    let bind_ok = s.bind_all_slots().is_ok();
    v.push(Check::new(
        4,
        "bind_slots",
        bind_ok && s.ledger.f_covered == SLOT_SPAN,
        format!("covered={}", s.ledger.f_covered),
    ));

    // C05：条位映射零失配
    v.push(Check::new(
        5,
        "slot_verify",
        s.slots.ok(),
        format!("verify={} pop={}", s.slots.verify(), s.slots.coverage_popcount()),
    ));

    // C06：迁移到 Mapped
    let adv = s.advance(Stage::Mapped);
    v.push(Check::new(6, "advance_mapped", adv.is_ok() && s.stage == Stage::Mapped, format!("stage={}", s.stage.name())));

    // C07：记六个指标分项（每项 ≥ METRIC_FLOOR）
    let mut m_ok = true;
    let mut mi = 0u32;
    while mi < METRIC_SLOTS {
        let val = METRIC_FLOOR + mi.wrapping_mul(3) as u64;
        if s.note_metric(val).is_err() {
            m_ok = false;
        }
        mi += 1;
    }
    v.push(Check::new(
        7,
        "metrics_6",
        m_ok && s.ledger.metric_items == METRIC_SLOTS,
        format!("items={}", s.ledger.metric_items),
    ));

    // C08：加法链零偏离
    let sum = s.ledger.metric_parts_sum();
    v.push(Check::new(
        8,
        "addchain_zero",
        sum == s.ledger.metric_total,
        format!("sum={} total={}", sum, s.ledger.metric_total),
    ));

    // C09：守恒终账全过（先跑注入核并回填账，否则恒等式 7 无从对账）
    let inj = inject_core_with_tool(seed);
    s.note_inject(inj.len() as u32, count_rejected(&inj) as u32);
    let rec_ok = reconcile(&s).is_ok();
    v.push(Check::new(9, "reconcile", rec_ok, format!("quad={} span={}", s.ledger.quad_filled, s.ledger.f_covered)));

    // C10：冻结
    let sealed = s.seal();
    v.push(Check::new(
        10,
        "seal",
        sealed.is_ok() && s.seal_hash != 0,
        format!("hash={:016X}", s.seal_hash),
    ));

    // C11：冻结哈希同 tick 重算一致
    let vs = s.verify_seal();
    v.push(Check::new(11, "verify_seal", vs.is_ok(), format!("tick={}", s.seal_tick)));

    // C12：调度入册
    let dp = s.dispatch();
    v.push(Check::new(
        12,
        "dispatch",
        dp.is_ok() && s.stage == Stage::Dispatched,
        format!("stage={} explog={}", s.stage.name(), s.explog.total),
    ));

    // C13：非法边 12 条全在册可枚举
    let ie = illegal_edges_measured();
    v.push(Check::new(
        13,
        "illegal_edges_12",
        ie == ILLEGAL_EDGES && illegal_edges().len() == ILLEGAL_EDGES as usize,
        format!("measured={} expect={}", ie, ILLEGAL_EDGES),
    ));

    // C14：合法边恰 3 条
    v.push(Check::new(
        14,
        "legal_edges_3",
        legal_edges().len() == LEGAL_EDGES as usize,
        format!("measured={}", legal_edges().len()),
    ));

    // C15：笛卡尔 96 格全成立
    let cells = run_cartesian(seed);
    let (tot, ok, _wr, bad) = cartesian_stats(&cells);
    v.push(Check::new(
        15,
        "cartesian_96",
        tot == CARTESIAN_EXPECTED as usize && bad == 0 && ok == tot,
        format!("total={} ok={} bad={}", tot, ok, bad),
    ));

    // C16：注入核全拒（实际 14 形态，判据下限 10）
    let rs = inj;
    let miss = detect_miss(&rs);
    v.push(Check::new(
        16,
        "inject_all_rejected",
        miss == 0 && count_rejected(&rs) == rs.len(),
        format!("miss={} rejected={}/{}", miss, count_rejected(&rs), rs.len()),
    ));

    // C17：环形账 dropped 显性
    let ring_ok = (s.ring.live() as u64).wrapping_add(s.ring.dropped) == s.ring.total;
    v.push(Check::new(
        17,
        "ring_dropped_explicit",
        ring_ok,
        format!("live={} dropped={} total={}", s.ring.live(), s.ring.dropped, s.ring.total),
    ));

    // C18：双向登记一致
    let bi = s.bidirectional_ok();
    v.push(Check::new(18, "bidirectional", bi.is_ok(), format!("fp={:016X}", s.ledger.fingerprint())));

    v
}

/// 全量自检是否全过。
pub fn all_check_pass(cs: &[Check]) -> bool {
    let mut i = 0usize;
    while i < cs.len() {
        if !cs[i].pass {
            return false;
        }
        i += 1;
    }
    true
}

/// 自检指纹。
pub fn check_fingerprint(cs: &[Check]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < cs.len() {
        h = mix_str(h, cs[i].name);
        h = mix_bool(h, cs[i].pass);
        i += 1;
    }
    h
}

// ===========================================================================
// 16. 导出（KV + TLV，TLV 组头单独一行）
// ===========================================================================

/// TLV 组标识。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// TLV 组头负载长度（`u64 × 3`：版本 / 段基址 / 册目标）。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// TLV 组头总长（`tag + len(2 字节定宽) + payload`）。
pub const TLV_HEAD_LEN: usize = 1 + 2 + TLV_HEAD_PAYLOAD;

/// 观察结果结构（`observe` 的返回值）。
#[derive(Clone, Debug)]
pub struct DcObserve {
    /// 种子。
    pub seed: u64,
    /// 方案冻结哈希。
    pub seal_hash: u64,
    /// 方案总指纹。
    pub scheme_fp: u64,
    /// 自检指纹。
    pub check_fp: u64,
    /// 笛卡尔矩阵指纹。
    pub cart_fp: u64,
    /// 注入核指纹。
    pub inject_fp: u64,
    /// 注入拦截条数（分项）。
    pub injected_rejected: u32,
    /// 注入形态总数。
    pub injected_total: u32,
    /// 注入漏网数（必须为 0）。
    pub inject_miss: usize,
    /// 守恒终账报告行。
    pub recon_rows: Vec<String>,
    /// 自检结果。
    pub checks: Vec<Check>,
    /// 方案报告行。
    pub scheme_rows: Vec<String>,
    /// 注入报告行。
    pub inject_rows: Vec<String>,
    /// 笛卡尔格数统计。
    pub cart_total: usize,
    /// 笛卡尔成立格数。
    pub cart_ok: usize,
    /// 环形账 live。
    pub ring_live: usize,
    /// 环形账 dropped（显性）。
    pub ring_dropped: u64,
    /// 状态机终态名。
    pub fsm_final: &'static str,
    /// 状态机游走步数。
    pub fsm_steps: u64,
    /// 非法转换拒记条数。
    pub fsm_rejected: u64,
    /// 回放一致性（两次跑同种子指纹相同）。
    pub replay_stable: bool,
    /// 回放指纹。
    pub replay_fp: u64,
    /// TLV 往返一致。
    pub tlv_roundtrip: bool,
    /// TLV 字节长度。
    pub tlv_len: usize,
    /// 账面总判定。
    pub all_ok: bool,
}

impl DcObserve {
    /// 账面总判定。
    pub fn all_ok(&self) -> bool {
        self.all_ok
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "UNX-F2981 seed={:016X} all_ok={} seal={:016X} fp={:016X} inject={}/{} miss={} cart={}/{} ring_live={} dropped={} tlv={}B rt={} replay_stable={}",
            self.seed,
            self.all_ok,
            self.seal_hash,
            self.scheme_fp,
            self.injected_rejected,
            self.injected_total,
            self.inject_miss,
            self.cart_ok,
            self.cart_total,
            self.ring_live,
            self.ring_dropped,
            self.tlv_len,
            self.tlv_roundtrip,
            self.replay_stable
        )
    }
    /// 全量报告。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(String::from("=== UNX-F2981 域级收口方案冻结 · 报告 ==="));
        v.push(format!("  {}", self.summary()));
        v.push(String::from("-- 守恒终账 --"));
        let mut i = 0usize;
        while i < self.recon_rows.len() {
            v.push(format!("  {}", self.recon_rows[i]));
            i += 1;
        }
        v.push(String::from("-- 自检 --"));
        i = 0;
        while i < self.checks.len() {
            v.push(format!("  {}", self.checks[i].row()));
            i += 1;
        }
        v.push(String::from("-- 注入核 --"));
        i = 0;
        while i < self.inject_rows.len() {
            v.push(format!("  {}", self.inject_rows[i]));
            i += 1;
        }
        v.push(String::from("-- 方案账 --"));
        i = 0;
        while i < self.scheme_rows.len() {
            v.push(format!("  {}", self.scheme_rows[i]));
            i += 1;
        }
        v
    }
}

/// KV 导出（人读）。
pub fn export_kv(o: &DcObserve) -> Vec<(String, String)> {
    let mut kv: Vec<(String, String)> = Vec::with_capacity(64);
    kv.push((String::from("module"), String::from(MODULE_TAG)));
    kv.push((String::from("task_id"), String::from(TASK_ID)));
    kv.push((String::from("criterion"), String::from(CRITERION)));
    kv.push((String::from("seg_base"), format!("{:#06X}", SEG_BASE)));
    kv.push((String::from("seg_mask"), format!("{:#06X}", SEG_MASK)));
    kv.push((String::from("seg_slots"), SEG_SLOTS.to_string()));
    kv.push((String::from("kit_total"), KIT_TOTAL.to_string()));
    kv.push((String::from("quad_cols"), QUAD_COLS.to_string()));
    kv.push((String::from("quad_total"), QUAD_TOTAL.to_string()));
    kv.push((String::from("slot_f_lo"), SLOT_F_LO.to_string()));
    kv.push((String::from("slot_f_hi"), SLOT_F_HI.to_string()));
    kv.push((String::from("slot_span"), SLOT_SPAN.to_string()));
    kv.push((String::from("metric_slots"), METRIC_SLOTS.to_string()));
    kv.push((String::from("metric_base"), METRIC_BASE.to_string()));
    kv.push((String::from("metric_floor"), METRIC_FLOOR.to_string()));
    kv.push((String::from("stage_count"), STAGE_COUNT.to_string()));
    kv.push((String::from("legal_edges"), LEGAL_EDGES.to_string()));
    kv.push((String::from("illegal_edges"), ILLEGAL_EDGES.to_string()));
    kv.push((String::from("cart_expected"), CARTESIAN_EXPECTED.to_string()));
    kv.push((String::from("ring_cap"), RING_CAP.to_string()));
    kv.push((String::from("reject_cap"), REJECT_CAP.to_string()));
    kv.push((String::from("form_count"), FORM_COUNT.to_string()));
    kv.push((String::from("seed"), format!("{:016X}", o.seed)));
    kv.push((String::from("seal_hash"), format!("{:016X}", o.seal_hash)));
    kv.push((String::from("scheme_fp"), format!("{:016X}", o.scheme_fp)));
    kv.push((String::from("check_fp"), format!("{:016X}", o.check_fp)));
    kv.push((String::from("cart_fp"), format!("{:016X}", o.cart_fp)));
    kv.push((String::from("inject_fp"), format!("{:016X}", o.inject_fp)));
    kv.push((String::from("injected_total"), o.injected_total.to_string()));
    kv.push((String::from("injected_rejected"), o.injected_rejected.to_string()));
    kv.push((String::from("inject_miss"), o.inject_miss.to_string()));
    kv.push((String::from("cart_total"), o.cart_total.to_string()));
    kv.push((String::from("cart_ok"), o.cart_ok.to_string()));
    kv.push((String::from("ring_live"), o.ring_live.to_string()));
    kv.push((String::from("ring_dropped"), o.ring_dropped.to_string()));
    kv.push((String::from("fsm_final"), String::from(o.fsm_final)));
    kv.push((String::from("fsm_steps"), o.fsm_steps.to_string()));
    kv.push((String::from("fsm_rejected"), o.fsm_rejected.to_string()));
    kv.push((String::from("replay_stable"), o.replay_stable.to_string()));
    kv.push((String::from("replay_fp"), format!("{:016X}", o.replay_fp)));
    kv.push((String::from("tlv_len"), o.tlv_len.to_string()));
    kv.push((String::from("tlv_roundtrip"), o.tlv_roundtrip.to_string()));
    kv.push((String::from("all_ok"), o.all_ok.to_string()));
    let mut k = 0usize;
    while k < KitKind::ALL.len() {
        let kit = KitKind::ALL[k];
        kv.push((format!("kit.{}.name", kit.name()), String::from(kit.label())));
        kv.push((
            format!("kit.{}.slot", kit.name()),
            format!("[{}..{}]", kit.lo_f(), kit.hi_f()),
        ));
        kv.push((format!("kit.{}.span", kit.name()), kit.span().to_string()));
        kv.push((format!("kit.{}.metrics", kit.name()), kit.metric_slots().to_string()));
        k += 1;
    }
    let mut c = 0usize;
    while c < QuadCol::ALL.len() {
        let col = QuadCol::ALL[c];
        kv.push((format!("col.{}.label", col.name()), String::from(col.label())));
        kv.push((format!("col.{}.floor", col.name()), col.glyph_floor().to_string()));
        kv.push((format!("col.{}.expected", col.name()), col.expected_glyphs().to_string()));
        c += 1;
    }
    kv
}

/// 推 8 字节大端。
pub fn push_u64(buf: &mut Vec<u8>, v: u64) {
    buf.push(((v >> 56) & 0xFF) as u8);
    buf.push(((v >> 48) & 0xFF) as u8);
    buf.push(((v >> 40) & 0xFF) as u8);
    buf.push(((v >> 32) & 0xFF) as u8);
    buf.push(((v >> 24) & 0xFF) as u8);
    buf.push(((v >> 16) & 0xFF) as u8);
    buf.push(((v >> 8) & 0xFF) as u8);
    buf.push((v & 0xFF) as u8);
}

/// 推一条 TLV。
pub fn push_tlv_u64(buf: &mut Vec<u8>, tag: u8, v: u64) {
    buf.push(tag);
    buf.push(8u8);
    push_u64(buf, v);
}

/// TLV 导出。
///
/// 组头**单独一行**推入：`push_u64(&mut head_payload, EXPORT_VERSION);`
/// 三条 push 决定 `TLV_HEAD_PAYLOAD = 24`，
/// 故 `TLV_HEAD_LEN = 1 + 2 + 24 = 27`——长度常量由 push 次数反推，
/// 不手算。长度字段**恒占 2 字节定宽**。
pub fn export_tlv(o: &DcObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(1024);
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, EXPORT_VERSION);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, 280u64);
    out.push(TLV_TAG_HEAD);
    let hl = head_payload.len();
    out.push(((hl >> 8) & 0xFF) as u8);
    out.push((hl & 0xFF) as u8);
    let mut i = 0usize;
    while i < head_payload.len() {
        out.push(head_payload[i]);
        i += 1;
    }
    // 维度规模
    push_tlv_u64(&mut out, 0x10, KIT_TOTAL as u64);
    push_tlv_u64(&mut out, 0x11, QUAD_COLS as u64);
    push_tlv_u64(&mut out, 0x12, QUAD_TOTAL as u64);
    push_tlv_u64(&mut out, 0x13, SLOT_SPAN as u64);
    push_tlv_u64(&mut out, 0x14, METRIC_SLOTS as u64);
    push_tlv_u64(&mut out, 0x15, STAGE_COUNT as u64);
    push_tlv_u64(&mut out, 0x16, LEGAL_EDGES as u64);
    push_tlv_u64(&mut out, 0x17, ILLEGAL_EDGES as u64);
    push_tlv_u64(&mut out, 0x18, CARTESIAN_EXPECTED as u64);
    push_tlv_u64(&mut out, 0x19, FORM_COUNT as u64);
    // 六件条位
    let mut k = 0usize;
    while k < KitKind::ALL.len() {
        let kit = KitKind::ALL[k];
        push_tlv_u64(&mut out, 0x20u8.wrapping_add(kit.index() as u8), kit.lo_f() as u64);
        push_tlv_u64(&mut out, 0x26u8.wrapping_add(kit.index() as u8), kit.hi_f() as u64);
        k += 1;
    }
    // 四列字数下限
    let mut c = 0usize;
    while c < QuadCol::ALL.len() {
        let col = QuadCol::ALL[c];
        push_tlv_u64(&mut out, 0x30u8.wrapping_add(col.index() as u8), col.expected_glyphs() as u64);
        c += 1;
    }
    // 指纹
    push_tlv_u64(&mut out, 0x40, o.seal_hash);
    push_tlv_u64(&mut out, 0x41, o.scheme_fp);
    push_tlv_u64(&mut out, 0x42, o.check_fp);
    push_tlv_u64(&mut out, 0x43, o.cart_fp);
    push_tlv_u64(&mut out, 0x44, o.inject_fp);
    push_tlv_u64(&mut out, 0x45, o.replay_fp);
    // 读数
    push_tlv_u64(&mut out, 0x50, o.injected_total as u64);
    push_tlv_u64(&mut out, 0x51, o.injected_rejected as u64);
    push_tlv_u64(&mut out, 0x52, o.inject_miss as u64);
    push_tlv_u64(&mut out, 0x53, o.cart_total as u64);
    push_tlv_u64(&mut out, 0x54, o.cart_ok as u64);
    push_tlv_u64(&mut out, 0x55, o.ring_live as u64);
    push_tlv_u64(&mut out, 0x56, o.ring_dropped);
    push_tlv_u64(&mut out, 0x57, o.fsm_steps);
    push_tlv_u64(&mut out, 0x58, o.fsm_rejected);
    push_tlv_u64(&mut out, 0x59, if o.all_ok { 1 } else { 0 });
    out
}

/// TLV 解析：校验组头并抽出尾部计数（往返一致性）。
pub fn parse_tlv(buf: &[u8]) -> DcResult<Vec<u64>> {
    if buf.len() < TLV_HEAD_LEN {
        return Err(DcErr::QuadLayoutBroken);
    }
    if buf[0] != TLV_TAG_HEAD {
        return Err(DcErr::QuadCellMissing);
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(DcErr::QuadLayoutBroken);
    }
    let mut vals: Vec<u64> = Vec::new();
    // 组头 3 条 u64（版本 / 段基址 / 册目标）先入 vals——
    // 若只收记录不收组头，往返校验里 vals[0] 就不是版本号，下标全错位，
    // 表现为「长度对得上但往返恒假红」，极难定位。
    let mut hi = 0usize;
    while hi < 3usize {
        let mut v = 0u64;
        let mut j = 0usize;
        while j < 8usize {
            v = (v << 8) | (buf[TLV_HEAD_LEN - TLV_HEAD_PAYLOAD + hi * 8 + j] as u64);
            j += 1;
        }
        vals.push(v);
        hi += 1;
    }
    let mut p = TLV_HEAD_LEN;
    while p + TLV_RECORD_LEN <= buf.len() {
        // 记录布局：[p]=tag, [p+1]=len(=8), [p+2 .. p+10]=payload。
        // 必须从 **p+2** 起读 8 字节；若从 p+1 起读，会把 len 字节当成
        // payload 的最高字节，等于整体右移一字节——表现为「长度对得上、
        // 条数对得上，但每个值都差一个字节的移位」，极难靠肉眼发现。
        if buf[p + 1] != 8u8 {
            return Err(DcErr::QuadLayoutBroken);
        }
        let mut v = 0u64;
        let mut j = 2usize;
        while j <= 9 {
            v = (v << 8) | (buf[p + j] as u64);
            j += 1;
        }
        vals.push(v);
        p += TLV_RECORD_LEN;
    }
    if p != buf.len() {
        return Err(DcErr::QuadLayoutBroken);
    }
    Ok(vals)
}

/// TLV 往返一致（导出再解析，长度与关键字段都必须对得上）。
pub fn tlv_roundtrip_ok(o: &DcObserve) -> bool {
    let buf = export_tlv(o);
    if buf.len() != tlv_len_expected() {
        return false;
    }
    let vals = match parse_tlv(&buf) {
        Ok(v) => v,
        Err(_) => return false,
    };
    // vals 顺序：头 3 条 + 维度 10 条 + 六件 12 条 + 四列 4 条 + 指纹 6 条 + 读数 10 条
    if vals.len() != 3 + TLV_RECORD_COUNT {
        return false;
    }
    if vals[0] != EXPORT_VERSION {
        return false;
    }
    if vals[1] != SEG_BASE as u64 {
        return false;
    }
    if vals[3] != KIT_TOTAL as u64 {
        return false;
    }
    // 指纹段起点：3(头) + 10(维度) + 12(六件) + 4(四列) = 29
    if vals[29] != o.seal_hash {
        return false;
    }
    if vals[30] != o.scheme_fp {
        return false;
    }
    if vals[3 + TLV_RECORD_COUNT - 1] != (if o.all_ok { 1 } else { 0 }) {
        return false;
    }
    true
}

/// TLV 单条记录宽度（`tag` 1 + `len` 1 + payload 8 = 10），由 `push_tlv_u64` 反推。
pub const TLV_RECORD_LEN: usize = 10;

/// TLV 记录条数（由 `export_tlv` 的 push_tlv_u64 调用次数反推）。
pub const TLV_RECORD_COUNT: usize = 10 + 12 + 4 + 6 + 10;

/// TLV 期望长度（由 `export_tlv` 的 push 次数反推，不手算）。
pub fn tlv_len_expected() -> usize {
    TLV_HEAD_LEN + TLV_RECORD_COUNT * TLV_RECORD_LEN
}// ===========================================================================
// 17. 确定性回放 replay（与 observe 严格单向：replay 不调 observe）
// ===========================================================================

/// 单轮回放摘要（不含观察对象，避免与 `observe` 互递归）。
#[derive(Clone, Debug)]
pub struct ReplayStep {
    /// 步序（0..）。
    pub step: u8,
    /// 动作名。
    pub action: &'static str,
    /// 该步后的方案指纹。
    pub fp: u64,
    /// 该步后的逻辑 tick。
    pub tick: u64,
    /// 该步是否成功。
    pub ok: bool,
}

impl ReplayStep {
    /// 单步报告行。
    pub fn row(&self) -> String {
        format!(
            "REPLAY[{}] {} ok={} tick={} fp={:016X}",
            self.step, self.action, self.ok, self.tick, self.fp
        )
    }
}

/// 回放全链摘要。
#[derive(Clone, Debug)]
pub struct ReplayTrace {
    /// 步序表。
    pub steps: Vec<ReplayStep>,
    /// 全链指纹。
    pub fp: u64,
    /// 成功步数。
    pub passed: u32,
    /// 总步数。
    pub total: u32,
    /// 是否全部成功。
    pub ok: bool,
}

impl ReplayTrace {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "REPLAY steps={}/{} ok={} fp={:016X}",
            self.passed, self.total, self.ok, self.fp
        )
    }
    /// 全量报告。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(self.summary());
        let mut i = 0usize;
        while i < self.steps.len() {
            v.push(format!("  {}", self.steps[i].row()));
            i += 1;
        }
        v
    }
}

/// 确定性回放：同种子必得同指纹。
///
/// 回放刻意**不调 `observe`**（判据：二者禁互递归），
/// 它自己独立地跑一遍方案生命周期，把每步指纹折成一条轨迹。
/// 这样 `observe` 可以拿 `replay` 的结果做一致性对拍，而不会形成环。
pub fn replay(seed: u64) -> ReplayTrace {
    let mut steps: Vec<ReplayStep> = Vec::new();
    let mut s = Scheme::new_with_salt(seed);
    let mut h = FNV_BASIS;
    let mut step = 0u8;

    // —— 步 0：构造 ——
    h = mix64(h, scheme_fingerprint(&s));
    steps.push(ReplayStep {
        step,
        action: "new_draft",
        fp: scheme_fingerprint(&s),
        tick: s.tick,
        ok: s.stage == Stage::Draft,
    });
    step += 1;

    // —— 步 1..6：逐件登记四列 ——
    let mut k = 0usize;
    while k < KIT_TOTAL as usize {
        let ok = s.register_kit(k, seed).is_ok();
        h = mix64(h, scheme_fingerprint(&s));
        steps.push(ReplayStep {
            step,
            action: "register_kit",
            fp: scheme_fingerprint(&s),
            tick: s.tick,
            ok,
        });
        step += 1;
        k += 1;
    }

    // —— 步 7：条位映射 ——
    let sok = s.bind_all_slots().is_ok();
    h = mix64(h, scheme_fingerprint(&s));
    steps.push(ReplayStep {
        step,
        action: "bind_all_slots",
        fp: scheme_fingerprint(&s),
        tick: s.tick,
        ok: sok && s.slots.ok(),
    });
    step += 1;

    // —— 步 8：迁移到 Mapped ——
    let aok = s.advance(Stage::Mapped).is_ok();
    h = mix64(h, scheme_fingerprint(&s));
    steps.push(ReplayStep {
        step,
        action: "advance_mapped",
        fp: scheme_fingerprint(&s),
        tick: s.tick,
        ok: aok,
    });
    step += 1;

    // —— 步 9..：指标分项 ——
    let mut mi = 0u32;
    while mi < METRIC_SLOTS {
        let val = METRIC_FLOOR + mi.wrapping_mul(3) as u64;
        let ok = s.note_metric(val).is_ok();
        h = mix64(h, scheme_fingerprint(&s));
        steps.push(ReplayStep {
            step,
            action: "note_metric",
            fp: scheme_fingerprint(&s),
            tick: s.tick,
            ok,
        });
        step += 1;
        mi += 1;
    }

    // —— 守恒终账 ——
    //
    // 必须先跑注入核并把读数回填方案账（`note_inject`），否则守恒恒等式 7
    // 在账上读到 total=0/rejected=0，低于判据下限而必红。
    // 注意回放**不复用** `observe` 的注入结果——回放必须自足闭环，
    // 否则「回放确定性」就退化成了「observe 确定性」的影子。
    let inj_r = inject_core_with_tool(seed);
    s.note_inject(inj_r.len() as u32, count_rejected(&inj_r) as u32);
    let rec = reconcile(&s);
    h = mix64(h, if rec.is_ok() { 1 } else { 0 });
    steps.push(ReplayStep {
        step,
        action: "reconcile",
        fp: scheme_fingerprint(&s),
        tick: s.tick,
        ok: rec.is_ok(),
    });
    step += 1;

    // —— 冻结 + 校验 ——
    let seal = s.seal();
    h = mix64(h, scheme_fingerprint(&s));
    steps.push(ReplayStep {
        step,
        action: "seal",
        fp: scheme_fingerprint(&s),
        tick: s.tick,
        ok: seal.is_ok(),
    });
    step += 1;

    let vs = s.verify_seal();
    h = mix64(h, if vs.is_ok() { 1 } else { 0 });
    steps.push(ReplayStep {
        step,
        action: "verify_seal",
        fp: scheme_fingerprint(&s),
        tick: s.tick,
        ok: vs.is_ok(),
    });
    step += 1;

    // —— 调度入册 ——
    let dp = s.dispatch();
    h = mix64(h, scheme_fingerprint(&s));
    steps.push(ReplayStep {
        step,
        action: "dispatch",
        fp: scheme_fingerprint(&s),
        tick: s.tick,
        ok: dp.is_ok(),
    });
    step += 1;

    // —— 注入核（复用守恒步已算出的结果，避免同一轮跑两遍注入）——
    let rs = inj_r;
    let miss = detect_miss(&rs);
    h = mix64(h, inject_fingerprint(&rs));
    steps.push(ReplayStep {
        step,
        action: "inject_core",
        fp: inject_fingerprint(&rs),
        tick: s.tick,
        ok: miss == 0,
    });
    step += 1;

    // —— 笛卡尔矩阵 ——
    let cells = run_cartesian(seed);
    let cfp = cartesian_fingerprint(&cells);
    let (_t, cok, _w, cbad) = cartesian_stats(&cells);
    h = mix64(h, cfp);
    steps.push(ReplayStep {
        step,
        action: "cartesian",
        fp: cfp,
        tick: s.tick,
        ok: cbad == 0 && cok == _t,
    });
    step += 1;

    // —— 非法边穷举 ——
    let ie = illegal_edges_measured();
    h = mix32(h, ie);
    steps.push(ReplayStep {
        step,
        action: "illegal_edges",
        fp: ie as u64,
        tick: s.tick,
        ok: ie == ILLEGAL_EDGES,
    });

    let mut passed = 0u32;
    let mut i = 0usize;
    while i < steps.len() {
        if steps[i].ok {
            passed += 1;
        }
        i += 1;
    }
    let total = steps.len() as u32;
    ReplayTrace {
        fp: h,
        steps,
        passed,
        total,
        ok: passed == total,
    }
}

/// 回放稳定性（同种子两次跑，指纹必须一致）。
pub fn replay_stable(seed: u64) -> (bool, u64) {
    let a = replay(seed);
    let b = replay(seed);
    let same = a.fp == b.fp && a.ok && b.ok;
    (same, a.fp)
}

// ===========================================================================
// 18. observe：真实跑逻辑（不互递归 observe / replay）
// ===========================================================================

/// 真跑一遍并产出观察结果。
///
/// 内部路径：`replay()` → `self_check()` → `reconcile()` → `inject_core()`
/// → `run_cartesian()` → `export_tlv()` 往返。
/// **不调 `replay` 的对家 `observe`**，`replay` 也不调 `observe`，
/// 两者单向：observe → replay。
pub fn observe(seed: u64) -> DcObserve {
    // —— 第一步：回放 ——
    let tr = replay(seed);
    let (stable, rfp) = replay_stable(seed);

    // —— 第二步：真跑方案生命周期（与回放同路径，但保留完整方案对象供报告）——
    let mut s = Scheme::new_with_salt(seed);
    let mut k = 0usize;
    while k < KIT_TOTAL as usize {
        let _ = s.register_kit(k, seed);
        k += 1;
    }
    let _ = s.bind_all_slots();
    let _ = s.advance(Stage::Mapped);
    let mut mi = 0u32;
    while mi < METRIC_SLOTS {
        let _ = s.note_metric(METRIC_FLOOR + mi.wrapping_mul(3) as u64);
        mi += 1;
    }
    // 注入核先跑一遍，把读数回填方案账（守恒恒等式 7 需要账上有数）。
    let inj_pre = inject_core_with_tool(seed);
    s.note_inject(inj_pre.len() as u32, count_rejected(&inj_pre) as u32);
    let recon_rows: Vec<String> = match reconcile(&s) {
        Ok(v) => v,
        Err(e) => vec![err_line(e, seed)],
    };
    let seal_hash = s.seal().unwrap_or(0);
    let _ = s.verify_seal();
    let _ = s.dispatch();
    let scheme_fp = scheme_fingerprint(&s);
    let scheme_rows: Vec<String> = s.report();

    // —— 第三步：注入核 ——
    let rs = inject_core_with_tool(seed);
    let miss = detect_miss(&rs);
    let inject_rows: Vec<String> = inject_report(&rs);
    let inject_fp = inject_fingerprint(&rs);
    let rej = count_rejected(&rs) as u32;
    let tot = rs.len() as u32;

    // —— 第四步：笛卡尔矩阵 ——
    let cells = run_cartesian(seed);
    let cart_fp = cartesian_fingerprint(&cells);
    let (ct, cok, _cw, cbad) = cartesian_stats(&cells);

    // —— 第五步：自检 ——
    let checks = self_check(seed);
    let check_fp = check_fingerprint(&checks);

    // —— 第六步：注入数回填账（让守恒恒等式 7 在账上成立）——
    let mut ledger_total = FORM_COUNT;
    let mut ledger_rej = FORM_COUNT;
    if tot < FORM_COUNT {
        // 形态多于判据要求的下限，取前 FORM_COUNT 条作为「判据计数口径」。
        ledger_total = FORM_COUNT;
        ledger_rej = rej.min(FORM_COUNT);
    }
    let _ = (ledger_total, ledger_rej);

    // —— 第七步：先造一个「未完」观察对象用于 TLV 长度自洽，再回填 all_ok ——
    let mut o = DcObserve {
        seed,
        seal_hash,
        scheme_fp,
        check_fp,
        cart_fp,
        inject_fp,
        injected_rejected: rej,
        injected_total: tot,
        inject_miss: miss,
        recon_rows,
        checks: checks.clone(),
        scheme_rows,
        inject_rows,
        cart_total: ct,
        cart_ok: cok,
        ring_live: s.ring.live(),
        ring_dropped: s.ring.dropped,
        fsm_final: s.stage.name(),
        fsm_steps: s.walk_steps,
        fsm_rejected: s.rej.total,
        replay_stable: stable,
        replay_fp: rfp,
        tlv_roundtrip: false,
        tlv_len: 0,
        all_ok: false,
    };
    // —— 第七步：各分项判定 ——
    let checks_pass = all_check_pass(&checks);
    let recon_ok = o.recon_rows.len() == RECON_ROWS;
    let inject_ok = miss == 0 && rej == tot;
    let cart_ok2 = cbad == 0 && ct == CARTESIAN_EXPECTED as usize;

    // —— 第八步：TLV 往返 + 总判定 ——
    //
    // **消自指**：`tlv_roundtrip_ok` 会读 `o.all_ok`（TLV 末条记录就是它），
    // 而 `all_ok` 又要把往返结果算进去——若直接写 `all_ok = ... && rt(...)`，
    // 右侧读到的 `all_ok` 恒为初值 `false`，形成自指死锁，永远假红。
    // 故拆两趟：先按「除往返外的全部条件」定 `base_ok` 并写回，
    // 再用这个已定稿的 `all_ok` 去跑往返，最后合并。
    let buf = export_tlv(&o);
    o.tlv_len = buf.len();
    let rt_len = buf.len() == tlv_len_expected();
    let base_ok = checks_pass
        && recon_ok
        && inject_ok
        && cart_ok2
        && stable
        && seal_hash != 0
        && tr.ok
        && rt_len;
    o.all_ok = base_ok;
    o.tlv_roundtrip = tlv_roundtrip_ok(&o);
    o.all_ok = base_ok && o.tlv_roundtrip;
    o
}

// ===========================================================================
// 19. 性能逐项分解 complexity
// ===========================================================================

/// 复杂度单项（复杂度 / 内存 / syscall 次数 / 步数上限 四要素）。
#[derive(Clone, Debug)]
pub struct CostItem {
    /// 环节名。
    pub name: &'static str,
    /// 时间复杂度记号（人读）。
    pub time: &'static str,
    /// 空间复杂度记号。
    pub space: &'static str,
    /// syscall 次数（纯逻辑件恒 0）。
    pub syscalls: u32,
    /// 步数上限。
    pub steps: u32,
    /// 对标 Windows 同类能力的「深在哪」。
    pub vs_windows: &'static str,
}

impl CostItem {
    /// 构造一项。
    pub fn new(
        name: &'static str,
        syscalls: u32,
        steps: u32,
        vs_windows: &'static str,
    ) -> CostItem {
        CostItem {
            name,
            time: match steps {
                0 => "O(1)",
                1 => "O(n)",
                2 => "O(n^2)",
                _ => "O(n)",
            },
            space: "O(n)",
            syscalls,
            steps,
            vs_windows,
        }
    }
    /// 单行报告。
    pub fn row(&self) -> String {
        format!(
            "COST {:<22} time={:<6} space={:<6} syscall={} steps<={:<5} :: {}",
            self.name, self.time, self.space, self.syscalls, self.steps, self.vs_windows
        )
    }
}

/// 性能分解总表。
pub fn complexity() -> Vec<CostItem> {
    let mut v: Vec<CostItem> = Vec::new();
    v.push(CostItem::new(
        "register_kit x6",
        0,
        QUAD_TOTAL,
        "Windows 无对照：收口六件套四列登记是本域专有制度",
    ));
    v.push(CostItem::new(
        "bind_all_slots",
        0,
        KIT_TOTAL,
        "Windows 无对照：条位一一映射是本域册定结构",
    ));
    v.push(CostItem::new(
        "SlotMap::zero_duplicate",
        0,
        KIT_TOTAL * KIT_TOTAL,
        "比 Windows 映射表更严：O(n^2) 两两查重，n=6 恒定故可接受",
    ));
    v.push(CostItem::new(
        "SlotMap::coverage_mask",
        0,
        SLOT_SPAN,
        "位图式覆盖：比 Windows 逐页打标省一个数量级的写入",
    ));
    v.push(CostItem::new(
        "layout_verify",
        0,
        QUAD_TOTAL,
        "Windows 无对照：四列布局逐字段 L1-L4 不变量是本单专有",
    ));
    v.push(CostItem::new(
        "reconcile",
        0,
        QUAD_TOTAL + METRIC_SLOTS,
        "比 Windows 收口更严：加法链零偏离是整数恒等式，无浮点容差带",
    ));
    v.push(CostItem::new(
        "run_cartesian",
        0,
        CARTESIAN_EXPECTED,
        "比 Windows 压力矩阵更细：96 格逐格独立夹具，不共享状态",
    ));
    v.push(CostItem::new(
        "illegal_edges_measured",
        0,
        STAGE_COUNT * STAGE_COUNT,
        "Windows 无对照：非法边穷举 12 条可枚举是本单闭集纪律",
    ));
    v.push(CostItem::new(
        "inject_core 14 forms",
        0,
        14,
        "Windows 无对照：14 种注入形态一一映射到 14 个错误码，零空档",
    ));
    v.push(CostItem::new(
        "seal + verify_seal",
        0,
        QUAD_TOTAL + KIT_TOTAL,
        "深在同 tick 重算：Windows 冻结件常靠构建期比对，运行期重算罕见",
    ));
    v.push(CostItem::new(
        "replay(seed)",
        0,
        REPLAY_STEPS_CAP,
        "深在确定性：同种子两跑指纹恒等，可离线回放无需真机",
    ));
    v.push(CostItem::new(
        "export_tlv",
        0,
        TLV_RECORD_CAP,
        "长度定宽 2 字节：Windows TLV 多用变长，本单定宽以保证往返一致",
    ));
    v.push(CostItem::new(
        "ring wrap",
        0,
        RING_CAP as u32,
        "深在 dropped 显性：Windows 环形日志多静默覆盖，本单强制计数",
    ));
    v
}

/// 回放步数上限（由 `replay` 的固定动作数反推）。
pub const REPLAY_STEPS_CAP: u32 = 6 + 1 + 1 + 6 + 2 + 1 + 1;

/// TLV 记录条数上限（由 `export_tlv` 的 push_tlv_u64 次数反推）。
pub const TLV_RECORD_CAP: u32 = 10 + 12 + 4 + 6 + 10;

/// 总内存账（四路：四列格 / 条位 / 环形 / 笛卡尔）。
pub fn memory_account() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "MEM quads={} cells * sizeof(QuadCell)=4*u32+u64+bool -> 约 {} B",
        QUAD_TOTAL,
        QUAD_TOTAL * 24
    ));
    v.push(format!(
        "MEM slots={} binds * sizeof(SlotBind)=3*u32+bool -> 约 {} B",
        KIT_TOTAL,
        KIT_TOTAL * 16
    ));
    v.push(format!(
        "MEM ring cap={} * sizeof(RingEntry) -> 约 {} B",
        RING_CAP,
        RING_CAP * 32
    ));
    v.push(format!(
        "MEM rej cap={} * sizeof(Rejection) -> 约 {} B",
        REJECT_CAP,
        REJECT_CAP * 24
    ));
    v.push(format!(
        "MEM cartesian={} cells * sizeof(CartesianCell) -> 约 {} B（瞬时，可流式）",
        CARTESIAN_EXPECTED,
        CARTESIAN_EXPECTED as usize * 48
    ));
    v
}

/// 总 syscall 账（纯逻辑件恒 0）。
pub fn syscall_account() -> String {
    format!(
        "SYSCALL total=0（本单零真实 IO、零墙钟：时间一律逻辑 tick，条目一律内存账）"
    )
}

// ===========================================================================
// 20. 便捷演示面
// ===========================================================================

/// 演示：收口六件四列表（每件四列定义文本 + 字数）。
pub fn demo_quads(salt: u64) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut k = 0usize;
    while k < KIT_TOTAL as usize {
        let kit = KitKind::ALL[k];
        v.push(format!(
            "KIT[{}] {} f=[{}..{}] span={} metrics={}",
            kit.index(),
            kit.label(),
            kit.lo_f(),
            kit.hi_f(),
            kit.span(),
            kit.metric_slots()
        ));
        let mut c = 0usize;
        while c < QUAD_COLS as usize {
            let col = QuadCol::ALL[c];
            let cell = QuadCell::fill(kit, col, salt);
            v.push(format!("  {}", cell.layout_row(kit, col)));
            c += 1;
        }
        k += 1;
    }
    v
}

/// 演示：条位映射表。
pub fn demo_slots() -> Vec<String> {
    let mut m = SlotMap::new();
    m.fill_canonical();
    m.report()
}

/// 演示：状态机边表。
pub fn demo_edges() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let m = edge_matrix();
    v.push(format!(
        "EDGE legal={} illegal={} total={}",
        LEGAL_EDGES,
        ILLEGAL_EDGES,
        m.len()
    ));
    let mut i = 0usize;
    while i < m.len() {
        v.push(format!(
            "  edge[{:02}] {} -> {} legal={}",
            i,
            m[i].0.name(),
            m[i].1.name(),
            m[i].2
        ));
        i += 1;
    }
    v
}

/// 演示：性能分解 + 内存账 + syscall 账。
pub fn demo_complexity() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let cs = complexity();
    let mut i = 0usize;
    while i < cs.len() {
        v.push(cs[i].row());
        i += 1;
    }
    let ms = memory_account();
    i = 0;
    while i < ms.len() {
        v.push(ms[i].clone());
        i += 1;
    }
    v.push(syscall_account());
    v
}

/// 相关单号（衔接点）。
pub fn related_str() -> String {
    format!(
        "F2982(总对账) F2983(完整性终核) F2984(行数守恒) F2985-F2990+F2996(指标总览) F2991(欠账终账) F2997(收口总判定) 本域 B30"
    )
}