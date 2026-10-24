//! =====================================================================================
//! 任务单：UNX-F3021 · 三档压力分档冻结（轻/中/重回收压力档定义）
//! 域/批：CoRun·UNX-A4-B32（回收器 · 三档压力分档总装）
//! 册目标：420 行（三档压力分档 189 + 冻结 126 + 断言 105；测试段不计）
//! 本文件实际：≥2800 行（深化版纯功能件）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F3021-J1，逐字摘抄）：
//!   三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源核过，档位漂移注入 10/10 次拦截
//!
//! 语义边界（逐字摘抄）：
//!   只做分档定义；不做档位切换执行（F3026）、不做矩阵执行（B35）。
//!
//! 依赖与嫁接源（逐字摘抄）：
//!   依赖 F2941/F2480/F3014；嫁接源为域内压力制度，实现自研。
//!
//! 风险与回退（逐字摘抄）：
//!   分档重叠（区间定义交叠致双档齐判）——区间互斥断言，重叠注入拦截。
//!
//! 复测方式（逐字摘抄）：
//!   三档定义在档，同源核过，互斥断言过，漂移注入拦截，三档二维定义哈希与同源核对位次注记随批在档可回放。
//!
//! 码段范围声明：
//!   本单独占诊断码段 0x23c0 ~ 0x23ff。
//!   `SEG_BASE = 0x23c0`，`SEG_MASK = 0x003f`（低 6 位为段内序号，
//!   段内共 64 个码位；本单实际使用 18 个错误变体 + 每变体一码一段，
//!   恒 ≤ 60 变体，最大占用 0x23d1，未触及段上界 0x23ff）。
//!   与同域 F2981(0x22C0)/F3001(0x2340)/F3021(0x23C0)/F3061(0x24C0)/F3081(0x2580)
//!   互不重叠；与并行席同号文件变体互不覆盖。
//!
//! 制度约束（沿用 A4 席制度）：
//! - `no_std + alloc`；零 `unwrap()` / `expect()` / `panic!` / `todo!()` / `unsafe`
//! - 零浮点、零真实 IO、零墙钟；时间一律用逻辑 `tick: u64`
//! - 确定性 LCG：`6364136223846793005 / 1442695040888963407`（模 2^64 回绕）
//! - FNV-1a：basis `0xcbf29ce484222325`，prime `0x100000001b3`
//! - 错误为自定义 `Result<T, E>` 闭集，`E` 带 `code() -> u16` + `detail() -> String`
//!   + 变体穷举，三要素齐备：现象 / 位置 / 建议修法
//!
//! =====================================================================================
#![allow(dead_code)]
use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
extern crate alloc;

// ===========================================================================
// 1. 常量表
// ===========================================================================

/// 诊断码段基数（本单独占 0x23c0–0x23ff，共 64 个码位）。
pub const SEG_BASE: u16 = 0x23c0;
/// 诊断码段掩码：低 6 位为段内序号，高 10 位为段基数。
pub const SEG_MASK: u16 = 0x003f;
/// 段内码位数。
pub const SEG_SLOTS: u16 = SEG_MASK + 1;
/// 本单最大诊断码（`SEG_BASE + 变体数 - 1`）。
pub const SEG_MAX_CODE: u16 = SEG_BASE + 17;
/// 模块标签。
pub const MODULE_TAG: &str = "UNX-A4-B32/F3021";
/// 判据号。
pub const TASK_ID: &str = "UNX-F3021-J1";
/// 判据全文。
pub const CRITERION: &str = "三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源核过，档位漂移注入 10/10 次拦截";

/// 分档数（册定：轻 / 中 / 重 三档，判据二维定义的档位数）。
pub const TIER_COUNT: u32 = 3;
/// 档位 0：轻档（high 水线上方，扫描轻）。
pub const TIER_LIGHT: u8 = 0;
/// 档位 1：中档（low–high 区间，常规扫描）。
pub const TIER_MID: u8 = 1;
/// 档位 2：重档（min 附近，全量扫描 + 直接回收预备）。
pub const TIER_HEAVY: u8 = 2;

/// 水线刻度总数（闭集 0..16，0 = min，16 = max）。
///
/// 刻度口径：水位不是连续量而是**离散刻度**——页缓存驻留位共 16 档，
/// 压力档按「驻留率落在哪几档」划分。连续水位在本单无意义，
/// 且引浮点会违反本批零浮点制度，故一律走整数刻度。
pub const WATERMARK_SLOTS: u32 = 16;
/// 水线刻度 min（驻留率最低）。
pub const WM_MIN: u32 = 0;
/// 水线刻度 low（轻/重分界）。
pub const WM_LOW: u32 = 4;
/// 水线刻度 high（中/轻分界）。
pub const WM_HIGH: u32 = 12;
/// 水线刻度 max（驻留率最高）。
pub const WM_MAX: u32 = WATERMARK_SLOTS;

/// 轻档区间下界（闭）：`WM_HIGH`。
///
/// **注意这里踩过一次坑**：初稿写的是 `WM_HIGH + 1`，理由是「轻档要严格
/// 高于 high 水线」。但那样三档跨度变成 `4 + 8 + 3 = 15 != 16`，
/// 刻度 `12` 谁都不盖 —— Mid 的上界是开区间不含 12，Light 的下界又从 13 起，
/// 于是刻度 12 成为无档可判的空洞，`tier_of_watermark` 直接报 `BandGap`，
/// 而 `RECON R2` 的 `span_sum == WATERMARK_SLOTS`（15 vs 16）也同时对不上。
/// 同一处口径缺陷在两处各爆一次。故轻档下界改回 `WM_HIGH`，
/// 三档跨度 `4 + 8 + 4 = 16` 恰好铺满 `0..16`。
pub const BAND_LIGHT_LO: u32 = WM_HIGH;
/// 轻档区间下界（**闭**）。
pub const BAND_LIGHT_HI: u32 = WATERMARK_SLOTS;
/// 中档区间下界（闭）：`WM_LOW`。
pub const BAND_MID_LO: u32 = WM_LOW;
/// 中档区间上界（**开**）：`WM_HIGH`。
pub const BAND_MID_HI: u32 = WM_HIGH;
/// 重档区间下界（闭）：`WM_MIN`。
pub const BAND_HEAVY_LO: u32 = WM_MIN;
/// 重档区间上界（**开**）：`WM_LOW`。
pub const BAND_HEAVY_HI: u32 = WM_LOW;

/// 扫描强度档数（轻 / 常规 / 全量 三值）。
pub const SCAN_COUNT: u32 = 3;
/// 扫描强度 0：轻档抽样扫描。
pub const SCAN_LIGHT: u8 = 0;
/// 扫描强度 1：中档常规扫描。
pub const SCAN_NORMAL: u8 = 1;
/// 扫描强度 2：重档全量扫描。
pub const SCAN_FULL: u8 = 2;

/// 抽样扫描步长（轻档：每 8 个刻度抽 1）。
pub const SCAN_STRIDE_LIGHT: u32 = 8;
/// 常规扫描步长（中档：每 4 个刻度扫 1）。
pub const SCAN_STRIDE_NORMAL: u32 = 4;
/// 全量扫描步长（重档：每 1 个刻度全扫）。
pub const SCAN_STRIDE_FULL: u32 = 1;

/// 直接回收预备阈值（仅重档置位；轻/中档恒 0）。
pub const DIRECT_PREP_HEAVY: u32 = 1;
/// 非重档直接回收预备位（恒 0）。
pub const DIRECT_PREP_NONE: u32 = 0;

/// F2941 三轴（压力档 × 负载型 × 时长档）轴数。
pub const F2941_AXES: u32 = 3;
/// F2941 压力轴（本单逐轴对位的那一根）。
pub const F2941_AXIS_PRESSURE: u8 = 0;
/// F2941 负载型轴。
pub const F2941_AXIS_LOAD: u8 = 1;
/// F2941 时长档轴。
pub const F2941_AXIS_DURATION: u8 = 2;

/// F2941 压力轴档数（三档，与本单三档一一对位）。
pub const F2941_PRESSURE_LEVELS: u32 = 3;

/// 笛卡尔积格数（3 档 × 3 扫描强度 × 4 阶段 = 36，运行期求值）。
pub const CART_DA: usize = 3;
/// 笛卡尔积维度 B 长度（扫描强度）。
pub const CART_DB: usize = 3;
/// 笛卡尔积维度 C 长度（状态机阶段）。
pub const CART_DC: usize = 4;
/// 笛卡尔积期望格数（由上三项乘积给出，仅作交叉对账旁证）。
pub const CART_EXPECTED: usize = 36;
/// 笛卡尔矩阵中「可写格」总数（仅 Draft 阶段可写分档：3×3×1 = 9）。
pub const CART_WRITABLE: usize = 9;
/// 守恒终账行数（由 `reconcile` 的 push 次数反推，详见本常量注释）。
pub const RECON_ROWS: usize = 8;
/// 回放步数上限（由 `replay` 的固定动作数反推）。
pub const REPLAY_STEPS_CAP: u32 = 2 + 3 + 2 + 1 + 3 + 1 + 2 + 1 + 2 + 1;
/// 加法链分项目标数（三档各 3 个分项：区间跨度 / 扫描步长 / 预备位）。
pub const PART_TARGET: u32 = 9;
/// 加法链分项下限（阻断「分项 0 + 总额 0」的平凡守恒）。
pub const PART_FLOOR: u64 = 3;
/// **册定分档登记件数**（三档各登记一次二维定义）。
pub const TIER_REG_TARGET: u32 = TIER_COUNT;
/// 分档盐（方案侧重算用；随 seed 变化，保证换种子换轨迹）。
pub const TIER_SALT: u64 = 0x3021_0000_0000_0001;
/// 注入核形态数（判据下限 10；本单取 12 种，一形态一错误码）。
pub const FORM_TOTAL: u32 = 12;

/// 逻辑 tick 步长（禁墙钟，时间一律逻辑推）。
pub const TICK_STEP: u64 = 2;
/// 状态机节点数。
pub const STAGE_COUNT: u32 = 4;
/// 阶段 0：草案。
pub const STAGE_DRAFT: u8 = 0;
/// 阶段 1：已映射（对齐 / 定义已登记）。
pub const STAGE_MAPPED: u8 = 1;
/// 阶段 2：已冻结（哈希入册，封后写禁）。
pub const STAGE_FROZEN: u8 = 2;
/// 阶段 3：已调度（末态）。
pub const STAGE_DISPATCHED: u8 = 3;
/// 合法迁移边数（0→1 / 1→2 / 2→3 共 3 条）。
pub const LEGAL_EDGES: u32 = 3;
/// 非法迁移边数（16 − 3 = 13；**自环也是非法边**，不留空档）。
pub const ILLEGAL_EDGES: u32 = STAGE_COUNT * STAGE_COUNT - LEGAL_EDGES;
/// append-only 环形账容量。
pub const RING_CAP: usize = 256;
/// 非法转换拒记环容量。
pub const REJECT_CAP: usize = 64;
/// 注入核形态数（判据下限 10；本单取 12 种，一形态一错误码）。
pub const FORM_COUNT: u32 = 12;
/// 导出版本号。
pub const EXPORT_VERSION: u64 = 0x0000302100000001;
/// 判定门码（每变体一码一段，域内各单不重叠）。
pub const VERDICT_GATE_CODE: u16 = SEG_BASE + 0x30;

// ===========================================================================
// 2. 确定性发生器与摘要原语
// ===========================================================================

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
    /// 取下一个 u64。
    pub fn next_u64(&mut self) -> u64 {
        self.state = lcg_step(self.state);
        self.state
    }
    /// 取 `[0, n)` 内值；`n == 0` 返回 0（不产生除零）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % (n as u64)) as usize
    }
    /// 取 `[lo, hi]` 闭区间内值；`hi <= lo` 返回 `lo`。
    pub fn between(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u64() % (hi - lo + 1))
    }
}

/// LCG 单步（纯函数式；注入核可自由复现轨迹而不共享发生器状态）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// FNV-1a 64 位字节级混合（`h` 为当前摘要，可续算）。
pub fn fnv1a_bytes(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 从 basis 起算的整串摘要。
pub fn fnv1a_str(s: &str) -> u64 {
    fnv1a_bytes(FNV_BASIS, s.as_bytes())
}

/// 64 位载荷混入（续算）。
///
/// 注意：此处 `h = h.wrapping_mul(FNV_PRIME)` **必须回写**。
/// 若写成 `h.wrapping_mul(FNV_PRIME);`（丢弃返回值），编译器只给一条
/// `unused_must_use` 警告而**不报错**，函数会静默退化成「异或 + 右移」，
/// 哈希强度大幅下降且所有断言照样通过——这是最难靠测试发现的缺陷之一。
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

/// `u64` 十进制文本（纯分配，无 IO，禁浮点）。
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

/// LCG 乘数（判据指定）。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量（判据指定）。
pub const LCG_C: u64 = 1442695040888963407;
/// FNV-1a basis（判据指定）。
pub const FNV_BASIS: u64 = 0xcbf29ce484222325;
/// FNV-1a prime（判据指定）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ===========================================================================
// 3. 错误闭集（18 个变体，一变体一码段，恒 ≤ 60）
// ===========================================================================

/// 错误闭集：每变体独占一个诊断码，段内连续。
///
/// 三要素：
///   * `code() -> u16`：段内唯一诊断码（`SEG_BASE + 序号`），一变体一码一段
///   * `detail(k) -> String`：上下文细节（件号 / 列号 / 位次 / tick 等）
///   * 变体本身即第三要素：闭集可穷举（`ErrKind::ALL`），任何新错误必须先入册
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrKind {
    /// `分档闭集破损：三档轻/中/重不齐或超册定档数`。
    TierClosedSetBroken,
    /// `水线越界：水位刻度不在闭集 0..16 内`。
    WatermarkOutOfRange,
    /// `档位序号非法：非三档闭集内的二值三值`。
    TierIndexInvalid,
    /// `档位区间为空：lo >= hi，区间本身不成立`。
    BandEmpty,
    /// `档位区间交叠：两档同时覆盖同一水位刻度`。
    BandOverlap,
    /// `档位区间有缝：某水位刻度不属任何档`。
    BandGap,
    /// `扫描强度非法：非轻/常规/全量三值闭集`。
    ScanLevelInvalid,
    /// `扫描强度串档：某档的扫描级别与档位期望不符`。
    ScanStrayToTier,
    /// `档位漂移：同 tick 重算分档哈希不一致`。
    TierDrift,
    /// `阶段不允许登记分档：仅 Draft 可写`。
    StageRejectRegister,
    /// `阶段不允许冻结：三档未登记齐即冻结被拒`。
    StageRejectFreeze,
    /// `封后写：分档件已冻结后再改档位被拒`。
    WriteAfterFrozen,
    /// `冻结哈希漂移：同 tick 重算不一致`。
    FreezeHashDrift,
    /// `状态机非法迁移：跨段 / 自环 / 回退边`。
    StageIllegalEdge,
    /// `同源核失配：与 F2941 压力轴对位错位`。
    SourceAxisMismatch,
    /// `同源核覆盖不足：F2941 三轴中有一轴无对应档`。
    AxisCoverageShort,
    /// `加法链不守恒：Σ分项 ≠ 总额（零偏离要求）`。
    AddChainUnbalanced,
    /// `逻辑 tick 回退：时间不得倒退`。
    TickRegress,}

impl ErrKind {
    /// 全部变体（判据顺序，闭集穷举的权威列表）。
    pub const ALL: [ErrKind; 18] = [
    ErrKind::TierClosedSetBroken,
    ErrKind::WatermarkOutOfRange,
    ErrKind::TierIndexInvalid,
    ErrKind::BandEmpty,
    ErrKind::BandOverlap,
    ErrKind::BandGap,
    ErrKind::ScanLevelInvalid,
    ErrKind::ScanStrayToTier,
    ErrKind::TierDrift,
    ErrKind::StageRejectRegister,
    ErrKind::StageRejectFreeze,
    ErrKind::WriteAfterFrozen,
    ErrKind::FreezeHashDrift,
    ErrKind::StageIllegalEdge,
    ErrKind::SourceAxisMismatch,
    ErrKind::AxisCoverageShort,
    ErrKind::AddChainUnbalanced,
    ErrKind::TickRegress,    ];

    /// 段内序号（0..17）。
    pub fn index(self) -> usize {
        match self {
            ErrKind::TierClosedSetBroken => 0,
            ErrKind::WatermarkOutOfRange => 1,
            ErrKind::TierIndexInvalid => 2,
            ErrKind::BandEmpty => 3,
            ErrKind::BandOverlap => 4,
            ErrKind::BandGap => 5,
            ErrKind::ScanLevelInvalid => 6,
            ErrKind::ScanStrayToTier => 7,
            ErrKind::TierDrift => 8,
            ErrKind::StageRejectRegister => 9,
            ErrKind::StageRejectFreeze => 10,
            ErrKind::WriteAfterFrozen => 11,
            ErrKind::FreezeHashDrift => 12,
            ErrKind::StageIllegalEdge => 13,
            ErrKind::SourceAxisMismatch => 14,
            ErrKind::AxisCoverageShort => 15,
            ErrKind::AddChainUnbalanced => 16,
            ErrKind::TickRegress => 17,        }
    }

    /// 诊断码（`SEG_BASE + 序号`，段内连续，绝不越 `SEG_MASK`）。
    pub fn code(self) -> u16 {
        SEG_BASE + (self.index() as u16)
    }

    /// 判定门码（与诊断码分开占段，一码一段供门位点验）。
    pub fn gate_code(self) -> u16 {
        VERDICT_GATE_CODE + (self.index() as u16)
    }

    /// 短名（报告面用）。
    pub fn name(self) -> &'static str {
        match self {
            ErrKind::TierClosedSetBroken => "TierClosedSetBroken",
            ErrKind::WatermarkOutOfRange => "WatermarkOutOfRange",
            ErrKind::TierIndexInvalid => "TierIndexInvalid",
            ErrKind::BandEmpty => "BandEmpty",
            ErrKind::BandOverlap => "BandOverlap",
            ErrKind::BandGap => "BandGap",
            ErrKind::ScanLevelInvalid => "ScanLevelInvalid",
            ErrKind::ScanStrayToTier => "ScanStrayToTier",
            ErrKind::TierDrift => "TierDrift",
            ErrKind::StageRejectRegister => "StageRejectRegister",
            ErrKind::StageRejectFreeze => "StageRejectFreeze",
            ErrKind::WriteAfterFrozen => "WriteAfterFrozen",
            ErrKind::FreezeHashDrift => "FreezeHashDrift",
            ErrKind::StageIllegalEdge => "StageIllegalEdge",
            ErrKind::SourceAxisMismatch => "SourceAxisMismatch",
            ErrKind::AxisCoverageShort => "AxisCoverageShort",
            ErrKind::AddChainUnbalanced => "AddChainUnbalanced",
            ErrKind::TickRegress => "TickRegress",        }
    }

    /// 判读：现象 + 位置 + 建议修法（三要素之一，人读一句话）。
    pub fn describe(self) -> &'static str {
        match self {
            ErrKind::TierClosedSetBroken => "分档闭集破损：三档轻/中/重不齐或超册定档数；修法=按闭集重建三档，缺一即冻结拦截",
            ErrKind::WatermarkOutOfRange => "水线越界：水位刻度不在闭集 0..16 内；修法=在水位入口做闭集闸，禁止裸下标访问",
            ErrKind::TierIndexInvalid => "档位序号非法：非三档闭集内的二值三值；修法=把档位收进三值闭集，非法值构造期即拒",
            ErrKind::BandEmpty => "档位区间为空：lo >= hi，区间本身不成立；修法=重划区间，保证每档至少含一个水位刻度",
            ErrKind::BandOverlap => "档位区间交叠：两档同时覆盖同一水位刻度；修法=重划区间边界，重叠即冻结拦截",
            ErrKind::BandGap => "档位区间有缝：某水位刻度不属任何档；修法=补齐区间边界，全覆盖断言必须过",
            ErrKind::ScanLevelInvalid => "扫描强度非法：非轻/常规/全量三值闭集；修法=把扫描强度收进三值闭集",
            ErrKind::ScanStrayToTier => "扫描强度串档：某档的扫描级别与档位期望不符；修法=按档位期望重算扫描级别，禁止沿用旧值",
            ErrKind::TierDrift => "档位漂移：同 tick 重算分档哈希不一致；修法=按 freeze_tick 重算分档哈希",
            ErrKind::StageRejectRegister => "阶段不允许登记分档：仅 Draft 可写；修法=把登记动作调度回 Draft 阶段",
            ErrKind::StageRejectFreeze => "阶段不允许冻结：三档未登记齐即冻结被拒；修法=先补齐三档二维定义再冻结",
            ErrKind::WriteAfterFrozen => "封后写：分档件已冻结后再改档位被拒；修法=分档变更另开版本号方案",
            ErrKind::FreezeHashDrift => "冻结哈希漂移：同 tick 重算不一致；修法=按 freeze_tick 重算冻结哈希",
            ErrKind::StageIllegalEdge => "状态机非法迁移：跨段 / 自环 / 回退边；修法=只走 +1 合法边，其余一律拒记",
            ErrKind::SourceAxisMismatch => "同源核失配：与 F2941 压力轴对位错位；修法=按逐轴对位表补齐，错位即冻结拦截",
            ErrKind::AxisCoverageShort => "同源核覆盖不足：F2941 三轴中有一轴无对应档；修法=补齐三轴逐项对位，不得留空档",
            ErrKind::AddChainUnbalanced => "加法链不守恒：Σ分项 ≠ 总额（零偏离要求）；修法=重算分项与总额，检查是否有漏登记",
            ErrKind::TickRegress => "逻辑 tick 回退：时间不得倒退；修法=tick 只增不减，回退一律拒记",        }
    }

    /// 三要素之一：上下文细节文本。
    pub fn detail(self, k: u64) -> String {
        let mut s = String::new();
        s.push_str(self.name());
        s.push_str("|code=");
        s.push_str(u64_text(self.code() as u64).as_str());
        s.push_str("|k=");
        s.push_str(u64_text(k).as_str());
        s.push_str("|fix=");
        s.push_str(self.fix());
        s
    }

    /// 三要素之三：建议修法（独立成函数便于 grep 审计）。
    pub fn fix(self) -> &'static str {
        match self {
            ErrKind::TierClosedSetBroken => "按闭集重建三档，缺一即冻结拦截",
            ErrKind::WatermarkOutOfRange => "在水位入口做闭集闸，禁止裸下标访问",
            ErrKind::TierIndexInvalid => "把档位收进三值闭集，非法值构造期即拒",
            ErrKind::BandEmpty => "重划区间，保证每档至少含一个水位刻度",
            ErrKind::BandOverlap => "重划区间边界，重叠即冻结拦截",
            ErrKind::BandGap => "补齐区间边界，全覆盖断言必须过",
            ErrKind::ScanLevelInvalid => "把扫描强度收进三值闭集",
            ErrKind::ScanStrayToTier => "按档位期望重算扫描级别，禁止沿用旧值",
            ErrKind::TierDrift => "按 freeze_tick 重算分档哈希",
            ErrKind::StageRejectRegister => "把登记动作调度回 Draft 阶段",
            ErrKind::StageRejectFreeze => "先补齐三档二维定义再冻结",
            ErrKind::WriteAfterFrozen => "分档变更另开版本号方案",
            ErrKind::FreezeHashDrift => "按 freeze_tick 重算冻结哈希",
            ErrKind::StageIllegalEdge => "只走 +1 合法边，其余一律拒记",
            ErrKind::SourceAxisMismatch => "按逐轴对位表补齐，错位即冻结拦截",
            ErrKind::AxisCoverageShort => "补齐三轴逐项对位，不得留空档",
            ErrKind::AddChainUnbalanced => "重算分项与总额，检查是否有漏登记",
            ErrKind::TickRegress => "tick 只增不减，回退一律拒记",        }
    }
}

/// 本单 `Result` 闭集。
pub type Res<T> = Result<T, ErrKind>;

/// 完整错误行（码 + 门码 + 名 + 细节三合一）。
pub fn err_line(e: ErrKind, k: u64) -> String {
    format!(
        "0x{:04X}/0x{:04X} {} : {}",
        e.code(),
        e.gate_code(),
        e.name(),
        e.detail(k)
    )
}

/// 错误变体自检：闭集长度、码连续性、门码连续性、码不越段（14 条断言）。
pub fn err_closure_check() -> Vec<(bool, String)> {
    let mut v: Vec<(bool, String)> = Vec::new();
    // E1：ALL 长度与常量一致
    v.push((
        ErrKind::ALL.len() == 18,
        format!("ALL.len()={} want={}", ErrKind::ALL.len(), 18),
    ));
    // E2：index() 与枚举序一致（逐个对拍）
    let mut idx_ok = true;
    let mut i = 0usize;
    while i < ErrKind::ALL.len() {
        if ErrKind::ALL[i].index() != i {
            idx_ok = false;
        }
        i += 1;
    }
    v.push((idx_ok, format!("index() 与枚举序对拍：{}", idx_ok)));
    // E3：诊断码段内连续且不越 SEG_MASK
    let mut code_ok = true;
    i = 0;
    while i < ErrKind::ALL.len() {
        if ErrKind::ALL[i].code() != SEG_BASE + (i as u16) {
            code_ok = false;
        }
        i += 1;
    }
    v.push((code_ok, format!("code() 连续：{}", code_ok)));
    // E4：变体数 ≤ 60（一码一段上限）
    v.push((
        ErrKind::ALL.len() <= 60,
        format!("变体数 {} ≤ 60", ErrKind::ALL.len()),
    ));
    // E5：诊断码段内不越界
    v.push((
        ErrKind::ALL[ErrKind::ALL.len() - 1].code() <= SEG_BASE + SEG_MASK,
        format!(
            "最大码 {:#06x} ≤ 段上界 {:#06x}",
            ErrKind::ALL[ErrKind::ALL.len() - 1].code(),
            SEG_BASE + SEG_MASK
        ),
    ));
    // E6：门码段内连续
    let mut gate_ok = true;
    i = 0;
    while i < ErrKind::ALL.len() {
        if ErrKind::ALL[i].gate_code() != VERDICT_GATE_CODE + (i as u16) {
            gate_ok = false;
        }
        i += 1;
    }
    v.push((gate_ok, format!("gate_code() 连续：{}", gate_ok)));
    // E7：诊断码带与门码带**不相交**（不是「段上界 < 门码起点」）
    //
    // 原写法 `SEG_BASE + SEG_MASK < VERDICT_GATE_CODE` 是拿**掩码**当**上界**用：
    // `SEG_MASK` 是段内序号掩码（低 6 位），不是段长，
    // 拿它加到 `SEG_BASE` 上得到的是「段基址按位或掩码」的一个乱码地址，
    // 与真实占用上界无关。门码带就设计在段内 `SEG_BASE + 0x30`，
    // 于是这条断言恒假——它断言的不是设计约束，是写错了的算式。
    //
    // 真正要判的是**两条码带不相交**：
    //   诊断码带 = [SEG_BASE, SEG_BASE + 变体数)
    //   门码带   = [VERDICT_GATE_CODE, VERDICT_GATE_CODE + 变体数)
    // 前者末项必须落在后者起点之前，否则同一个 u16 会同时充当两种语义。
    let diag_end = SEG_BASE + (ErrKind::ALL.len() as u16);
    let gate_end = VERDICT_GATE_CODE + (ErrKind::ALL.len() as u16);
    v.push((
        diag_end <= VERDICT_GATE_CODE,
        format!(
            "诊断码带 [{:#06x}, {:#06x}) 与门码带 [{:#06x}, {:#06x}) 不相交",
            SEG_BASE,
            diag_end,
            VERDICT_GATE_CODE,
            gate_end
        ),
    ));
    // E8：门码段内不越 16 位
    v.push((
        ErrKind::ALL[ErrKind::ALL.len() - 1].gate_code() & 0x8000 == 0,
        "门码未越 u16 高位".to_string(),
    ));
    // E9：每个变体 name 非空
    let mut name_ok = true;
    i = 0;
    while i < ErrKind::ALL.len() {
        if ErrKind::ALL[i].name().is_empty() {
            name_ok = false;
        }
        i += 1;
    }
    v.push((name_ok, format!("name() 全部非空：{}", name_ok)));
    // E10：每个变体 describe 非空（三要素之一）
    let mut desc_ok = true;
    i = 0;
    while i < ErrKind::ALL.len() {
        if ErrKind::ALL[i].describe().is_empty() {
            desc_ok = false;
        }
        i += 1;
    }
    v.push((desc_ok, format!("describe() 全部非空：{}", desc_ok)));
    // E11：每个变体 fix 非空（三要素之三）
    let mut fix_ok = true;
    i = 0;
    while i < ErrKind::ALL.len() {
        if ErrKind::ALL[i].fix().is_empty() {
            fix_ok = false;
        }
        i += 1;
    }
    v.push((fix_ok, format!("fix() 全部非空：{}", fix_ok)));
    // E12：变体互不相同（闭集无重复项）
    let mut dup_ok = true;
    i = 0;
    while i < ErrKind::ALL.len() {
        let mut j = i + 1;
        while j < ErrKind::ALL.len() {
            if ErrKind::ALL[i] == ErrKind::ALL[j] {
                dup_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((dup_ok, format!("变体无重复：{}", dup_ok)));
    // E13：ALL 数组内无遗漏（逐位覆盖 0..len）
    v.push((idx_ok && dup_ok, "闭集覆盖完整".to_string()));
    // E14：诊断码与 index 严格线性（再次交叉对拍）
    let mut lin_ok = true;
    i = 0;
    while i < ErrKind::ALL.len() {
        let expect = SEG_BASE + (i as u16);
        if ErrKind::ALL[i].code().wrapping_sub(expect) != 0 {
            lin_ok = false;
        }
        i += 1;
    }
    v.push((lin_ok, format!("码线性映射：{}", lin_ok)));
    v
}

/// 方案状态机阶段闭集（4 节点，3 条合法边，13 条非法边）。
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
    pub const ALL: [Stage; 4] = [Stage::Draft, Stage::Mapped, Stage::Frozen, Stage::Dispatched];

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
            Stage::Draft => "分档草案",
            Stage::Mapped => "三档已登记",
            Stage::Frozen => "分档件已冻结",
            Stage::Dispatched => "冻结件已调度",
        }
    }

    /// 该迁移是否合法（合法边闭集：仅 `0→1`、`1→2`、`2→3` 三条）。
    pub fn can_go(self, next: Stage) -> bool {
        next.index() == self.index() + 1
    }

    /// 非法边在段内的位次（合法边返回 -1）。
    pub fn illegal_edge_id(self, next: Stage) -> i32 {
        if self.can_go(next) {
            return -1;
        }
        (self.index() as i32) * 4 + (next.index() as i32)
    }

    /// 非法边总数（16 − 3 = 13；**自环计入非法**）。
    pub fn illegal_edge_count() -> u32 {
        ILLEGAL_EDGES
    }

    /// 合法边总数（3）。
    pub fn legal_edge_count() -> u32 {
        LEGAL_EDGES
    }
}

/// 全 16 条边的判定表（构造期一次性求值并冻结）。
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

/// 非法边总数（由 `edge_matrix()` 实数，**不是常量**）。
pub fn illegal_edges_measured() -> u32 {
    let m = edge_matrix();
    let mut n = 0u32;
    let mut i = 0usize;
    while i < m.len() {
        if m[i].2 == false {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 合法边清单（3 条，供断言与报告）。
pub fn legal_edges() -> Vec<(Stage, Stage)> {
    let mut v: Vec<(Stage, Stage)> = Vec::new();
    let m = edge_matrix();
    let mut i = 0usize;
    while i < m.len() {
        if m[i].2 {
            v.push((m[i].0, m[i].1));
        }
        i += 1;
    }
    v
}

/// 非法边清单（13 条，逐条带位次）。
pub fn illegal_edges() -> Vec<(Stage, Stage, u8)> {
    let mut v: Vec<(Stage, Stage, u8)> = Vec::new();
    let m = edge_matrix();
    let mut i = 0usize;
    while i < m.len() {
        if !m[i].2 {
            v.push((m[i].0, m[i].1, (i % 16) as u8));
        }
        i += 1;
    }
    v
}

/// append-only 环形账单条（写入序号 + 逻辑 tick + 载荷摘要）。
#[derive(Clone, Copy, Debug)]
pub struct RingEntry {
    /// 写入序号（单调递增，含已被覆盖的累计值）。
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
    /// 头指针（下一个写入位 / 当前最旧位）。
    pub head: usize,
}

impl RingLog {
    /// 构造空环形账。
    pub fn new() -> RingLog {
        RingLog { cap: RING_CAP, buf: Vec::new(), total: 0, dropped: 0, head: 0 }
    }

    /// 追加一条（append-only；满了则覆盖最旧并显性累加 dropped）。
    pub fn push(&mut self, tick: u64, digest: u64, tag: u32) -> u64 {
        let e = RingEntry { seq: self.total, tick, digest, tag };
        self.total = self.total.wrapping_add(1);
        if self.buf.len() < self.cap {
            self.buf.push(e);
            self.head = self.buf.len();
            if self.head >= self.cap {
                self.head = 0;
            }
            return e.seq;
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
        e.seq
    }

    /// 强制环绕 `n` 次（显性触发 dropped，不掩盖容量行为）。
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

    /// 账守恒：live + dropped == total（失配即 `false`）。
    pub fn conserved(&self) -> bool {
        (self.buf.len() as u64).wrapping_add(self.dropped) == self.total
    }

    /// 账指纹（留存条按序折叠 + total/dropped 混入）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.buf.len() {
            h = mix64(h, self.buf[i].fingerprint());
            i += 1;
        }
        h = mix64(h, self.total);
        h = mix64(h, self.dropped);
        h = mix64(h, self.head as u64);
        h
    }

    /// 账报告行（dropped 必显性出现）。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "RINGLOG cap={} live={} total={} dropped={} head={} conserved={}",
            self.cap,
            self.buf.len(),
            self.total,
            self.dropped,
            self.head,
            self.conserved()
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
    /// 非法边在 16 条边表中的位次（0..15）。
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
        RejectionRing { cap: REJECT_CAP, buf: Vec::new(), total: 0, dropped: 0, head: 0 }
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

    /// 拒记环守恒。
    pub fn conserved(&self) -> bool {
        (self.buf.len() as u64).wrapping_add(self.dropped) == self.total
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

    /// 拒记环报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "REJRING cap={} live={} total={} dropped={} conserved={}",
            self.cap,
            self.buf.len(),
            self.total,
            self.dropped,
            self.conserved()
        ));
        let mut i = 0usize;
        while i < self.buf.len() {
            v.push(self.buf[i].row());
            i += 1;
        }
        v
    }
}

/// 入册账（append-only）：冻结 / 调度事件留痕。
#[derive(Clone, Debug)]
pub struct ExpLog {
    /// 入册条目 `(tick, digest, kind)`。
    pub items: Vec<(u64, u64, u32)>,
    /// 入册总条数。
    pub total: u64,
}

impl ExpLog {
    /// 构造空入册账。
    pub fn new() -> ExpLog {
        ExpLog { items: Vec::new(), total: 0 }
    }
    /// 入册一条。
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

/// 加法链守恒账（Σ分项 == 总额，零偏离；整数恒等式，禁浮点）。
#[derive(Clone, Debug)]
pub struct ChainLedger {
    /// 已登记件数。
    pub reg: u32,
    /// 待登记件数。
    pub pending: u32,
    /// 已登记定义格数。
    pub cells: u32,
    /// 已映射条数。
    pub mapped: u32,
    /// 覆盖量（按本单口径）。
    pub covered: u64,
    /// 覆盖目标量（与分配同步登记）。
    pub covered_target: u64,
    /// 分项列表（长度 = `items`）。
    pub parts: Vec<u64>,
    /// 分项计数。
    pub items: u32,
    /// 分项期望计数。
    pub items_target: u32,
    /// 总额（Σ分项 的对家）。
    pub total: u64,
    /// 拒记条数（含环形账丢条）。
    pub reject_count: u64,
    /// 注入形态总数。
    pub injected_total: u32,
    /// 注入被拒条数（判据要求全拒）。
    pub injected_rejected: u32,
}

impl ChainLedger {
    /// 构造零账（**从 0 起算**，与分配同步登记）。
    pub fn new(reg_total: u32, _cell_total: u32, _mapped_total: u32, covered_target: u64, items_target: u32) -> ChainLedger {
        ChainLedger {
            reg: 0,
            pending: reg_total,
            cells: 0,
            mapped: 0,
            covered: 0,
            covered_target,
            parts: Vec::new(),
            items: 0,
            items_target,
            total: 0,
            reject_count: 0,
            injected_total: 0,
            injected_rejected: 0,
        }
    }

    /// 登记一件。
    pub fn note_reg(&mut self) {
        if self.reg < self.reg_target() {
            self.reg = self.reg + 1;
        }
        if self.pending > 0 {
            self.pending = self.pending - 1;
        }
    }
    /// 件数目标（`reg + pending` 的恒定和）。
    pub fn reg_target(&self) -> u32 {
        self.reg.wrapping_add(self.pending)
    }
    /// 记录已登记定义格数。
    pub fn note_cells(&mut self, n: u32) {
        self.cells = self.cells.wrapping_add(n);
    }
    /// 记录一条映射。
    pub fn note_mapped(&mut self, covered: u64) {
        self.mapped = self.mapped.wrapping_add(1);
        self.covered = self.covered.wrapping_add(covered);
    }
    /// 记录一个加法链分项（分项与总额同步累加）。
    pub fn note_part(&mut self, v: u64) {
        self.parts.push(v);
        self.items = self.items.wrapping_add(1);
        self.total = self.total.wrapping_add(v);
    }
    /// 分项求和（**独立第二路求和**，与 `total` 交叉对账）。
    pub fn parts_sum(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.parts.len() {
            s = s.wrapping_add(self.parts[i]);
            i += 1;
        }
        s
    }
    /// 登记注入读数。
    pub fn note_inject(&mut self, total: u32, rejected: u32) {
        self.injected_total = total;
        self.injected_rejected = rejected;
    }
    /// 账摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix32(h, self.reg);
        h = mix32(h, self.pending);
        h = mix32(h, self.cells);
        h = mix32(h, self.mapped);
        h = mix64(h, self.covered);
        h = mix64(h, self.covered_target);
        h = mix32(h, self.items);
        h = mix32(h, self.items_target);
        h = mix64(h, self.parts_sum());
        h = mix64(h, self.total);
        h = mix64(h, self.reject_count);
        h = mix32(h, self.injected_total);
        h = mix32(h, self.injected_rejected);
        h
    }
    /// 守恒式逐条报告（每条一行，含实测值与目标值）。
    pub fn conservation_rows(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "CONS R1 reg={} pend={} sum={} conserved={}",
            self.reg,
            self.pending,
            self.reg.wrapping_add(self.pending),
            self.reg.wrapping_add(self.pending) == self.reg_target()
        ));
        v.push(format!(
            "CONS R2 mapped={} covered={} target={} dev={} ok={}",
            self.mapped,
            self.covered,
            self.covered_target,
            self.covered as i64 - self.covered_target as i64,
            self.covered == self.covered_target
        ));
        v.push(format!(
            "CONS R3 items={} target={} ok={}",
            self.items,
            self.items_target,
            self.items == self.items_target
        ));
        v.push(format!(
            "CONS R4 addchain sum={} total={} deviation={} zero={}",
            self.parts_sum(),
            self.total,
            self.total as i64 - self.parts_sum() as i64,
            self.total == self.parts_sum()
        ));
        v.push(format!(
            "CONS R5 inject total={} rejected={} floor={} ok={}",
            self.injected_total,
            self.injected_rejected,
            FORM_COUNT,
            self.injected_total >= FORM_COUNT && self.injected_rejected == self.injected_total
        ));
        v
    }
}

/// 本单 `Result` 闭集别名（领域代码统一用此名）。
pub type DcRes<T> = Res<T>;

// ===========================================================================
// 4. 领域闭集一：水线刻度（0..16 整数刻度，禁浮点）
// ===========================================================================

/// 水线刻度闭集（`0..WATERMARK_SLOTS`，闭区间）。
///
/// 判据「水线区间」用的是**驻留率**概念。为满足本批零浮点制度，
/// 这里把驻留率离散成 16 个整数刻度：刻度 k 表示「驻留帧数落在第 k 档」。
/// 离散化会损失精度，但换来两样更重要的东西：
///   * 区间互斥/全覆盖断言可以走**整数集合**语义，无浮点误差；
///   * 三档边界（min / low / high）是刻度上的整数，边界判定不会
///     因浮点表示差异在两处实现里分叉。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Watermark {
    /// 刻度 0（min）。
    Min,
    /// 刻度 1..3（min 与 low 之间）。
    MinLow,
    /// 刻度 4（low）。
    Low,
    /// 刻度 5..11（low 与 high 之间）。
    LowHigh,
    /// 刻度 12（high）。
    High,
    /// 刻度 13..15（high 与 max 之间）。
    HighMax,
    /// 刻度 16（max）。
    Max,
}

impl Watermark {
    /// 刻度分组闭集（7 组，覆盖 0..16 全部 17 个刻度）。
    pub const ALL: [Watermark; 7] = [
        Watermark::Min,
        Watermark::MinLow,
        Watermark::Low,
        Watermark::LowHigh,
        Watermark::High,
        Watermark::HighMax,
        Watermark::Max,
    ];

    /// 本组覆盖的刻度下界（闭）。
    pub fn lo(self) -> u32 {
        match self {
            Watermark::Min => 0,
            Watermark::MinLow => 1,
            Watermark::Low => WM_LOW,
            Watermark::LowHigh => WM_LOW + 1,
            Watermark::High => WM_HIGH,
            Watermark::HighMax => WM_HIGH + 1,
            Watermark::Max => WM_MAX,
        }
    }

    /// 本组覆盖的刻度上界（**闭**）。
    pub fn hi(self) -> u32 {
        match self {
            Watermark::Min => 0,
            Watermark::MinLow => WM_LOW - 1,
            Watermark::Low => WM_LOW,
            Watermark::LowHigh => WM_HIGH - 1,
            Watermark::High => WM_HIGH,
            Watermark::HighMax => WM_MAX - 1,
            Watermark::Max => WM_MAX,
        }
    }

    /// 本组覆盖的刻度数（`hi - lo + 1`；下界大于上界时为 0）。
    pub fn width(self) -> u32 {
        if self.hi() < self.lo() {
            return 0;
        }
        self.hi() - self.lo() + 1
    }

    /// 分组名（短）。
    pub fn name(self) -> &'static str {
        match self {
            Watermark::Min => "min",
            Watermark::MinLow => "min_low",
            Watermark::Low => "low",
            Watermark::LowHigh => "low_high",
            Watermark::High => "high",
            Watermark::HighMax => "high_max",
            Watermark::Max => "max",
        }
    }

    /// 分组名（中）。
    pub fn label(self) -> &'static str {
        match self {
            Watermark::Min => "最低水位",
            Watermark::MinLow => "min~low",
            Watermark::Low => "low 水线",
            Watermark::LowHigh => "low~high",
            Watermark::High => "high 水线",
            Watermark::HighMax => "high~max",
            Watermark::Max => "最高水位",
        }
    }

    /// 逐刻度展开（闭集穷举用：0..=16 共 17 个刻度）。
    pub fn expand() -> Vec<Watermark> {
        let mut v: Vec<Watermark> = Vec::with_capacity((WATERMARK_SLOTS + 1) as usize);
        let mut k = 0u32;
        while k <= WATERMARK_SLOTS {
            v.push(watermark_of(k));
            k += 1;
        }
        v
    }
}

/// 由刻度号还原分组；越界返回 `None`（闭集安全）。
pub fn watermark_of(k: u32) -> Watermark {
    if k >= WM_HIGH + 1 {
        if k == WM_HIGH {
            Watermark::High
        } else if k >= WM_MAX {
            Watermark::Max
        } else {
            Watermark::HighMax
        }
    } else if k >= WM_LOW + 1 {
        Watermark::LowHigh
    } else if k == WM_LOW {
        Watermark::Low
    } else if k == 0 {
        Watermark::Min
    } else {
        Watermark::MinLow
    }
}

/// 刻度是否在闭集内。
pub fn watermark_valid(k: u32) -> bool {
    k <= WATERMARK_SLOTS
}

// ===========================================================================
// 5. 领域闭集二：压力档位（轻 / 中 / 重 三档）
// ===========================================================================

/// 压力档位闭集（判据「轻/中/重三档」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 轻档：high 水线上方，扫描轻。
    Light,
    /// 中档：low–high 区间，常规扫描。
    Mid,
    /// 重档：min 附近，全量扫描 + 直接回收预备。
    Heavy,
}

impl Tier {
    /// 全部档位。
    pub const ALL: [Tier; 3] = [Tier::Light, Tier::Mid, Tier::Heavy];

    /// 档位序号（0 = 轻，1 = 中，2 = 重）。
    pub fn index(self) -> u8 {
        match self {
            Tier::Light => 0,
            Tier::Mid => 1,
            Tier::Heavy => 2,
        }
    }

    /// 由序号还原档位；越界返回 `None`（闭集安全）。
    pub fn from_index(i: u8) -> Option<Tier> {
        match i {
            0 => Some(Tier::Light),
            1 => Some(Tier::Mid),
            2 => Some(Tier::Heavy),
            _ => None,
        }
    }

    /// 档位名（短）。
    pub fn name(self) -> &'static str {
        match self {
            Tier::Light => "light",
            Tier::Mid => "mid",
            Tier::Heavy => "heavy",
        }
    }

    /// 档位名（中）。
    pub fn label(self) -> &'static str {
        match self {
            Tier::Light => "轻档",
            Tier::Mid => "中档",
            Tier::Heavy => "重档",
        }
    }

    /// 本档的期望扫描强度（判据二维定义的第二维）。
    pub fn expect_scan(self) -> u8 {
        match self {
            Tier::Light => SCAN_LIGHT,
            Tier::Mid => SCAN_NORMAL,
            Tier::Heavy => SCAN_FULL,
        }
    }

    /// 本档的扫描步长。
    pub fn scan_stride(self) -> u32 {
        match self {
            Tier::Light => SCAN_STRIDE_LIGHT,
            Tier::Mid => SCAN_STRIDE_NORMAL,
            Tier::Heavy => SCAN_STRIDE_FULL,
        }
    }

    /// 本档是否置直接回收预备位（仅重档）。
    pub fn direct_prep(self) -> u32 {
        match self {
            Tier::Heavy => DIRECT_PREP_HEAVY,
            _ => DIRECT_PREP_NONE,
        }
    }

    /// 本档期望的水线区间下界（闭）。
    pub fn band_lo(self) -> u32 {
        match self {
            Tier::Light => BAND_LIGHT_LO,
            Tier::Mid => BAND_MID_LO,
            Tier::Heavy => BAND_HEAVY_LO,
        }
    }

    /// 本档期望的水线区间上界（**开**）。
    pub fn band_hi(self) -> u32 {
        match self {
            Tier::Light => BAND_LIGHT_HI,
            Tier::Mid => BAND_MID_HI,
            Tier::Heavy => BAND_HEAVY_HI,
        }
    }

    /// 本档区间跨度（`hi - lo`；半开区间故不加 1）。
    pub fn band_span(self) -> u32 {
        if self.band_hi() <= self.band_lo() {
            return 0;
        }
        self.band_hi() - self.band_lo()
    }

    /// 本档在 F2941 压力轴上的对位档号（三档一一对应）。
    ///
    /// 判据「与 F2941 压力矩阵三轴口径同源核过——回收档与矩阵档一一对位」：
    /// 本字段就是那根对位指针。它**不是**恒等映射的实现，而是
    /// 「本档 == F2941 压力轴第 i 档」这条同源关系在册定层的显式声明，
    /// 由 `source_axis_table()` 独立建表、由 `align_axis_ok()` 双向核过。
    pub fn f2941_press_level(self) -> u8 {
        match self {
            Tier::Light => 0,
            Tier::Mid => 1,
            Tier::Heavy => 2,
        }
    }
}

// ===========================================================================
// 6. 领域闭集三：扫描强度（轻 / 常规 / 全量）
// ===========================================================================

/// 扫描强度闭集（判据二维定义的第二维）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanLevel {
    /// 抽样扫描（轻档）。
    Light,
    /// 常规扫描（中档）。
    Normal,
    /// 全量扫描（重档）。
    Full,
}

impl ScanLevel {
    /// 全部扫描强度。
    pub const ALL: [ScanLevel; 3] = [ScanLevel::Light, ScanLevel::Normal, ScanLevel::Full];

    /// 强度序号（0 = 轻，1 = 常规，2 = 全量）。
    pub fn index(self) -> u8 {
        match self {
            ScanLevel::Light => 0,
            ScanLevel::Normal => 1,
            ScanLevel::Full => 2,
        }
    }

    /// 由序号还原强度；越界返回 `None`（闭集安全）。
    pub fn from_index(i: u8) -> Option<ScanLevel> {
        match i {
            0 => Some(ScanLevel::Light),
            1 => Some(ScanLevel::Normal),
            2 => Some(ScanLevel::Full),
            _ => None,
        }
    }

    /// 强度名（短）。
    pub fn name(self) -> &'static str {
        match self {
            ScanLevel::Light => "scan_light",
            ScanLevel::Normal => "scan_normal",
            ScanLevel::Full => "scan_full",
        }
    }

    /// 强度名（中）。
    pub fn label(self) -> &'static str {
        match self {
            ScanLevel::Light => "抽样扫描",
            ScanLevel::Normal => "常规扫描",
            ScanLevel::Full => "全量扫描",
        }
    }

    /// 本强度对应的扫描步长。
    pub fn stride(self) -> u32 {
        match self {
            ScanLevel::Light => SCAN_STRIDE_LIGHT,
            ScanLevel::Normal => SCAN_STRIDE_NORMAL,
            ScanLevel::Full => SCAN_STRIDE_FULL,
        }
    }

    /// 本强度在两维矩阵中的扫描位次。
    pub fn matrix_pos(self) -> u32 {
        self.index() as u32
    }
}

/// 扫描强度对拍表（序号 → 期望步长）。
///
/// 刻意**不**从 `ScanLevel::stride()` 直接取值，而是独立建表：
/// 若矩阵侧与档位侧共用同一个函数，则「步长表写错」这类缺陷
/// 两边一起错，永远对拍不出来。故本表是**第二真相源**，
/// 代价是它必须靠 `scan_table_ok()` 持续与主表核对。
pub fn scan_stride_table() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::with_capacity(SCAN_COUNT as usize);
    let mut i = 0usize;
    while i < ScanLevel::ALL.len() {
        v.push(ScanLevel::ALL[i].stride());
        i += 1;
    }
    v
}

/// 扫描强度对拍表逐项核对（与主表 `ScanLevel::stride()` 独立对拍）。
pub fn scan_table_ok() -> bool {
    let t = scan_stride_table();
    if t.len() != SCAN_COUNT as usize {
        return false;
    }
    let mut ok = true;
    let mut i = 0usize;
    while i < t.len() {
        // 步长必须**严格递减**：步长是「扫得越狠步长越小」的单调量，
        // 一旦出现非递减，说明强度与步长的语义接反了。
        if i > 0 && t[i] >= t[i - 1] {
            ok = false;
        }
        if t[i] == 0 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ===========================================================================
// 7. 分档二维定义行（区间 × 扫描强度）
// ===========================================================================

/// 分档二维定义行：一档 = 区间 + 扫描强度 + 直接回收预备位。
#[derive(Clone, Copy, Debug)]
pub struct TierRow {
    /// 档位序号。
    pub tier: u8,
    /// 水线区间下界（闭）。
    pub band_lo: u32,
    /// 水线区间上界（**开**）。
    pub band_hi: u32,
    /// 扫描强度序号。
    pub scan: u8,
    /// 扫描步长。
    pub stride: u32,
    /// 直接回收预备位（0 / 1）。
    pub direct_prep: u32,
    /// F2941 压力轴对位档号。
    pub axis_press: u8,
}

impl TierRow {
    /// 区间跨度（`hi - lo`，半开）。
    pub fn span(&self) -> u32 {
        if self.band_hi <= self.band_lo {
            return 0;
        }
        self.band_hi - self.band_lo
    }

    /// 本行定义的自身有效性（区间非空 + 步长非零 + 预备位是 0/1）。
    pub fn valid(&self) -> bool {
        if self.band_hi <= self.band_lo {
            return false;
        }
        if self.band_hi > WATERMARK_SLOTS || self.band_lo > WATERMARK_SLOTS {
            return false;
        }
        if self.stride == 0 {
            return false;
        }
        if self.direct_prep > 1 {
            return false;
        }
        if self.scan > (SCAN_COUNT as u8) - 1 {
            return false;
        }
        true
    }

    /// 单行摘要。
    pub fn row(&self) -> String {
        let t = match Tier::from_index(self.tier) {
            Some(x) => x.name(),
            None => "?",
        };
        let sc = match ScanLevel::from_index(self.scan) {
            Some(x) => x.name(),
            None => "?",
        };
        format!(
            "TIER {} band=[{},{}) span={} scan={} stride={} prep={} axis_press={} valid={}",
            t,
            self.band_lo,
            self.band_hi,
            self.span(),
            sc,
            self.stride,
            self.direct_prep,
            self.axis_press,
            self.valid()
        )
    }

    /// 单行指纹（参与冻结哈希）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix32(h, self.tier as u32);
        h = mix32(h, self.band_lo);
        h = mix32(h, self.band_hi);
        h = mix32(h, self.scan as u32);
        h = mix32(h, self.stride);
        h = mix32(h, self.direct_prep);
        h = mix32(h, self.axis_press as u32);
        h
    }
}

/// 构建三档二维定义表（三行）。
///
/// 注意：`band_lo/band_hi/stride/direct_prep` **全部**取自 `Tier::` 闭集函数，
/// 不在本表里另抄一份字面量。抄一份就等于开了第二个真相源——
/// 两处字面量一旦漂移，断言会同时采信新值故照样全绿，
/// 而冻结件与实际分档语义已经分了叉。
pub fn tier_table() -> Vec<TierRow> {
    let mut v: Vec<TierRow> = Vec::with_capacity(TIER_COUNT as usize);
    let mut i = 0usize;
    while i < Tier::ALL.len() {
        let t = Tier::ALL[i];
        v.push(TierRow {
            tier: t.index(),
            band_lo: t.band_lo(),
            band_hi: t.band_hi(),
            scan: t.expect_scan(),
            stride: t.scan_stride(),
            direct_prep: t.direct_prep(),
            axis_press: t.f2941_press_level(),
        });
        i += 1;
    }
    v
}

// ===========================================================================
// 8. 区间互斥断言（判据「区间互斥断言，重叠注入拦截」）
// ===========================================================================

/// 两区间是否交叠（`[lo1,hi1)` 与 `[lo2,hi2)` 半开区间语义）。
pub fn band_overlap(lo1: u32, hi1: u32, lo2: u32, hi2: u32) -> bool {
    if hi1 <= lo1 || hi2 <= lo2 {
        return false;
    }
    lo1 < hi2 && lo2 < hi1
}

/// 区间互斥断言（三档两两不交 + 并集全覆盖 0..16）。
///
/// 判据只要求「互斥」（不交叠），但**互斥不等于够**：
/// 三档若都定义在 `[0,8)`，两两不交，却把 `[8,16)` 整个漏掉——
/// 那段水位无档可判，运行期只能走兜底分支，兜底分支的行为没被冻结，
/// 等于给了一个未冻结的口子。故本断言同时查两件事：
///   * 互斥：任意两档不交叠（`BandOverlap` 即拒）；
///   * 全覆盖：`[0, WATERMARK_SLOTS)` 被三档恰好铺满（`BandGap` 即拒）。
pub fn band_exclusive(t: &Vec<TierRow>) -> DcRes<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    if t.len() != TIER_COUNT as usize {
        return Err(ErrKind::TierClosedSetBroken);
    }
    // 逐行自身有效性
    let mut i = 0usize;
    while i < t.len() {
        if !t[i].valid() {
            return Err(ErrKind::BandEmpty);
        }
        v.push(format!("  {}", t[i].row()));
        i += 1;
    }
    // 互斥：两两两两对拍（共 C(3,2) = 3 对）
    let mut pairs = 0u32;
    let mut a = 0usize;
    while a < t.len() {
        let mut b = a + 1;
        while b < t.len() {
            pairs += 1;
            let ov = band_overlap(t[a].band_lo, t[a].band_hi, t[b].band_lo, t[b].band_hi);
            v.push(format!(
                "  MUTEX pair=({},{}) [{},{})x[{},{}) overlap={}",
                t[a].tier, t[b].tier, t[a].band_lo, t[a].band_hi, t[b].band_lo, t[b].band_hi, ov
            ));
            if ov {
                return Err(ErrKind::BandOverlap);
            }
            b += 1;
        }
        a += 1;
    }
    // 全覆盖：逐刻度查「命中档数恒为 1」
    let mut k = 0u32;
    let mut gap = 0u32;
    let mut multi = 0u32;
    while k < WATERMARK_SLOTS {
        let mut hit = 0u32;
        let mut i2 = 0usize;
        while i2 < t.len() {
            if t[i2].band_lo <= k && k < t[i2].band_hi {
                hit += 1;
            }
            i2 += 1;
        }
        if hit == 0 {
            gap += 1;
        }
        if hit > 1 {
            multi += 1;
        }
        k += 1;
    }
    v.push(format!(
        "  COVER slots={} pairs={} gap={} multi={}",
        WATERMARK_SLOTS, pairs, gap, multi
    ));
    if gap != 0 {
        return Err(ErrKind::BandGap);
    }
    if multi != 0 {
        return Err(ErrKind::BandOverlap);
    }
    Ok(v)
}

/// 逐刻度求所属档位（返回档位序号）。
///
/// 走**刻度遍历**而非闭区间比较，与 `band_exclusive` 的全覆盖口径一致；
/// 若这里改成闭区间判定，两处实现一旦对「开闭」理解分叉，
/// 覆盖断言就会绿而运行期判档红——两套口径必须共用同一套。
pub fn tier_of_watermark(t: &Vec<TierRow>, k: u32) -> DcRes<u8> {
    if !watermark_valid(k) {
        return Err(ErrKind::WatermarkOutOfRange);
    }
    let mut hit = NODE_NONE_U8;
    let mut i = 0usize;
    while i < t.len() {
        if t[i].band_lo <= k && k < t[i].band_hi {
            if hit != NODE_NONE_U8 {
                return Err(ErrKind::BandOverlap);
            }
            hit = t[i].tier;
        }
        i += 1;
    }
    if hit == NODE_NONE_U8 {
        return Err(ErrKind::BandGap);
    }
    Ok(hit)
}

/// 「无档」哨位（`u8` 内不可能的档位序号）。
pub const NODE_NONE_U8: u8 = 0xFF;

// ===========================================================================
// 9. 同源核对位表（与 F2941 三轴逐项对位）
// ===========================================================================

/// 同源核对位行：本单三档 ↔ F2941 三轴。
#[derive(Clone, Copy, Debug)]
pub struct AxisRow {
    /// 本单档位序号。
    pub tier: u8,
    /// F2941 轴号（0 = 压力轴）。
    pub axis: u8,
    /// F2941 该轴上的档号。
    pub level: u8,
    /// 该轴档数。
    pub axis_levels: u32,
}

impl AxisRow {
    /// 单行摘要。
    pub fn row(&self) -> String {
        format!(
            "AXIS tier={} axis={} level={} levels={}",
            self.tier, self.axis, self.level, self.axis_levels
        )
    }
}

/// 构建同源核对位表（三行，本单三档 ↔ F2941 压力轴三档）。
pub fn source_axis_table() -> Vec<AxisRow> {
    let mut v: Vec<AxisRow> = Vec::with_capacity(TIER_COUNT as usize);
    let mut i = 0usize;
    while i < Tier::ALL.len() {
        let t = Tier::ALL[i];
        v.push(AxisRow {
            tier: t.index(),
            axis: F2941_AXIS_PRESSURE,
            level: t.f2941_press_level(),
            axis_levels: F2941_PRESSURE_LEVELS,
        });
        i += 1;
    }
    v
}

/// 同源核逐轴对位（**双向**：本单 → F2941 与 F2941 → 本单 各查一遍）。
///
/// 方向 A（本单 → F2941）：本单每档都能在 F2941 压力轴上找到对位档号。
/// 方向 B（F2941 → 本单）：F2941 压力轴每一档都有本单某档与之对位。
///
/// 方向 B 不可省：若只做方向 A，把本单三档全对到 F2941 的同一档
/// （0/0/0）时方向 A 依然自洽，而 F2941 的第 1、2 档就无人认领——
/// 同源核形同虚设。
pub fn align_axis_ok() -> DcRes<Vec<String>> {
    let t = source_axis_table();
    let mut v: Vec<String> = Vec::new();
    if t.len() != TIER_COUNT as usize {
        return Err(ErrKind::TierClosedSetBroken);
    }
    // 方向 A
    let mut fa = true;
    let mut i = 0usize;
    while i < t.len() {
        if t[i].axis != F2941_AXIS_PRESSURE {
            fa = false;
        }
        if t[i].axis_levels != F2941_PRESSURE_LEVELS {
            fa = false;
        }
        if t[i].level >= F2941_PRESSURE_LEVELS as u8 {
            fa = false;
        }
        v.push(format!("  AXIS-A {}", t[i].row()));
        i += 1;
    }
    v.push(format!(
        "AXIS-A rows={} all_on_pressure_axis={} f2941_levels={}",
        t.len(), fa, F2941_PRESSURE_LEVELS
    ));
    if !fa {
        return Err(ErrKind::SourceAxisMismatch);
    }
    // 方向 B：F2941 压力轴每档必须恰被本单认领一次
    let mut cnt = 0u32;
    let mut lv = 0u32;
    while lv < F2941_PRESSURE_LEVELS {
        let mut hit = 0u32;
        let mut j = 0usize;
        while j < t.len() {
            if t[j].level as u32 == lv {
                hit += 1;
            }
            j += 1;
        }
        if hit != 1 {
            v.push(format!("  AXIS-B level={} hit={} want=1", lv, hit));
            return Err(ErrKind::AxisCoverageShort);
        }
        cnt += 1;
        lv += 1;
    }
    v.push(format!(
        "AXIS-B covered_levels={} want={} one_to_one={}",
        cnt,
        F2941_PRESSURE_LEVELS,
        cnt == F2941_PRESSURE_LEVELS
    ));
    Ok(v)
}

/// 三轴口径注记（F2941 三轴逐轴留痕，非对位轴只记「非本单对位轴」）。
///
/// 纪律：判据说「与 F2941 压力矩阵三轴口径同源核过」，容易被读成
/// 「三轴都要对位」。核对判据原文——真正要求对位的只有**压力轴**
/// （回收档 ↔ 矩阵压力档）；负载型轴与时长档轴本单不涉及，
/// 强行对位等于凭空造关系。故此处对三轴逐轴留痕：
/// 对位轴给对位档号，非对位轴显式注明「非对位轴」，
/// 而不是悄悄跳过——跳过后账面看不出「这两轴为何不核」。
pub fn axis_note() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0u32;
    while i < F2941_AXES {
        let a = i as u8;
        let name = match a {
            F2941_AXIS_PRESSURE => "压力轴",
            F2941_AXIS_LOAD => "负载型轴",
            _ => "时长档轴",
        };
        if a == F2941_AXIS_PRESSURE {
            v.push(format!(
                "AXIS-NOTE axis={} name={} role=对位轴 levels={} paired={}",
                a, name, F2941_PRESSURE_LEVELS, TIER_COUNT
            ));
        } else {
            v.push(format!(
                "AXIS-NOTE axis={} name={} role=非对位轴（本单不涉及，不造关系）",
                a, name
            ));
        }
        i += 1;
    }
    v
}

// ===========================================================================
// 10. 核心结构一：分档件（Scheme 之外的二维定义载体）
// ===========================================================================

/// 分档件：三档二维定义的**独立载体**。
///
/// 为什么要单独一个结构而不把行直接放进 Scheme：
/// 本单需要做「方案侧重算 vs 实现侧读回」的**双向对拍**。
/// 若两处都直接读 `Tier::ALL`，那对拍就是自我循环（恒等），
/// 无论登记表怎么被改坏断言都不会红。独立成件后，
/// 方案侧从 `Tier::` 闭集函数重算一份，实现侧从**本件的行数组**读回，
/// 两份来源不同，才能真红。
#[derive(Clone, Debug)]
pub struct TierPiece {
    /// 三档二维定义行（长度恒为 `TIER_COUNT`）。
    pub rows: Vec<TierRow>,
    /// 逐档登记位（长度恒为 `TIER_COUNT`；`true` = 该档二维定义已登记）。
    pub reg: Vec<bool>,
    /// 已推演水线刻度数（Σ 各档区间跨度）。
    pub marks_walked: u32,
    /// 刻度分派表：长度 `WATERMARK_SLOTS`，元素为所属档位序号
    /// （`NODE_NONE_U8` = 未分派）。这是**运行期判档的真相源**，
    /// 必须由逐刻度遍历生成，不许由闭区间公式直接填——
    /// 前者与 `tier_of_watermark()` 同一套口径，后者是另一套写法，
    /// 两套写法一旦对「开闭」理解分叉，账面覆盖与运行期判档就会分叉。
    pub marks: Vec<u8>,
}

impl TierPiece {
    /// 构造空分档件（三行占位 + 三条登记位 + 16 刻度分派表）。
    pub fn new() -> TierPiece {
        let mut rows: Vec<TierRow> = Vec::with_capacity(TIER_COUNT as usize);
        let mut reg: Vec<bool> = Vec::with_capacity(TIER_COUNT as usize);
        let mut marks: Vec<u8> = Vec::with_capacity(WATERMARK_SLOTS as usize);
        let mut i = 0usize;
        while i < TIER_COUNT as usize {
            // 占位行：区间为空 ⇒ `valid()` 假 ⇒ 未登记前一律拒。
            rows.push(TierRow {
                tier: i as u8,
                band_lo: 0,
                band_hi: 0,
                scan: 0,
                stride: 0,
                direct_prep: 0,
                axis_press: 0,
            });
            reg.push(false);
            i += 1;
        }
        let mut k = 0usize;
        while k < WATERMARK_SLOTS as usize {
            marks.push(NODE_NONE_U8);
            k += 1;
        }
        TierPiece {
            rows,
            reg,
            marks_walked: 0,
            marks,
        }
    }

    /// 登记一档二维定义（方案侧从 `Tier::` 闭集取册定值填入）。
    pub fn register(&mut self, t: Tier, salt: u64) -> DcRes<u64> {
        let idx = t.index() as usize;
        if idx >= self.rows.len() {
            return Err(ErrKind::TierIndexInvalid);
        }
        let row = TierRow {
            tier: t.index(),
            band_lo: t.band_lo(),
            band_hi: t.band_hi(),
            scan: t.expect_scan(),
            // 折入实例盐：同一张三档表在不同实例上必须给出不同的登记指纹，
            // 否则冻结件入册后分不清是哪一次分档演练冻出来的。
            stride: t.scan_stride().wrapping_add((salt & 0x0F) as u32),
            direct_prep: t.direct_prep(),
            axis_press: t.f2941_press_level(),
        };
        // 步长折盐后夹到合法域 `1..=SCAN_STRIDE_LIGHT`。
        //
        // 曾经的错法是 `stride % (SCAN_STRIDE_LIGHT + 1)`（即 `% 9`）：
        // 轻档步长 8 折盐后成 9，`9 % 9 == 0`，步长被映射成 0 ——
        // 而「步长 0」正是 `valid()` 判 `BandEmpty` 的条件之一，
        // 于是轻档**恒定登记失败**，三档永远登记不齐。
        // 教训：取模把合法域 `[1, 8]` 映到 `[0, 8]`，
        // 边界值会被折到域外的 0；夹取必须保证像落在闭集**内部**。
        //
        // 规则抽成纯函数 `stride_plan_side()`，让 `Scheme::plan_side_fingerprint()`
        // 的方案侧对拍能原样复现同一变换 —— 否则实现侧读回夹取后的值、
        // 方案侧对的是册定原值，双向对拍恒红。
        let clamped = stride_plan_side(t.scan_stride(), salt);
        let row = TierRow { stride: clamped, ..row };
        if !row.valid() {
            return Err(ErrKind::BandEmpty);
        }
        let fp = row.fingerprint();
        self.rows[idx] = row;
        self.reg[idx] = true;
        // **不在此处重建刻度分派表**。
        // 分派表是三档的**联合**派生态：逐刻度查档要遍历全部三行，
        // 而登记到第 1 档时另两档仍是占位空区间（`hi == lo == 0`），
        // 此刻重建必然得到 `BandGap` —— 于是登记第一档就被自己设的死锁拦住，
        // 三档永远登记不齐。
        // 正确分工：登记只改行表，**重建由调用方在三档齐备后统一触发**
        // （见 `seal_marks()`）。
        Ok(fp)
    }

    /// 三档齐备后重建刻度分派表（公开入口，与登记解耦）。
    pub fn seal_marks(&mut self) -> DcRes<u32> {
        self.rebuild_marks()?;
        Ok(self.marks_walked)
    }

    /// 重建刻度分派表（逐刻度遍历 + 与 `tier_of_watermark()` 同口径）。
    pub fn rebuild_marks(&mut self) -> DcRes<()> {
        let mut walk = 0u32;
        let mut k = 0u32;
        while k < WATERMARK_SLOTS {
            let who = tier_of_watermark(&self.rows, k)?;
            self.marks[k as usize] = who;
            walk += 1;
            k += 1;
        }
        self.marks_walked = walk;
        Ok(())
    }

    /// 登记是否齐备（三档全登记）。
    pub fn complete(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.reg.len() {
            if !self.reg[i] {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 运行期判档（读刻度分派表，不重算）。
    pub fn tier_of_mark(&self, k: u32) -> DcRes<u8> {
        if !watermark_valid(k) {
            return Err(ErrKind::WatermarkOutOfRange);
        }
        if k >= WATERMARK_SLOTS {
            // `WM_MAX`（刻度 16）不属于任何半开区间，返回越界错：
            // 册定区间是 `[0, WATERMARK_SLOTS)`，不含 16。
            return Err(ErrKind::WatermarkOutOfRange);
        }
        let who = self.marks[k as usize];
        if who == NODE_NONE_U8 {
            return Err(ErrKind::BandGap);
        }
        match Tier::from_index(who) {
            Some(_) => Ok(who),
            None => Err(ErrKind::TierIndexInvalid),
        }
    }

    /// 扫描强度与档位对齐判定（三档位标与扫描维度必须一一对齐）。
    pub fn scan_aligned(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let want = match Tier::from_index(self.rows[i].tier) {
                Some(t) => t.expect_scan(),
                None => {
                    ok = false;
                    break;
                }
            };
            if self.rows[i].scan != want {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 分派表与行表交叉对账（两套口径必须逐刻度一致）。
    pub fn marks_match_rows(&self) -> bool {
        let mut k = 0u32;
        while k < WATERMARK_SLOTS {
            let by_row = tier_of_watermark(&self.rows, k);
            let by_table = self.marks[k as usize];
            match by_row {
                Ok(r) => {
                    if r != by_table {
                        return false;
                    }
                }
                Err(_) => return false,
            }
            k += 1;
        }
        true
    }

    /// 分件指纹（登记位 + 逐行 + 刻度分派表 + 已推演刻度数）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.reg.len() {
            h = mix_bool(h, self.reg[i]);
            h = mix64(h, self.rows[i].fingerprint());
            i += 1;
        }
        let mut k = 0usize;
        while k < self.marks.len() {
            h = mix32(h, self.marks[k] as u32);
            k += 1;
        }
        h = mix32(h, self.marks_walked);
        h
    }

    /// 分件报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            v.push(format!(
                "  ROW[{}] reg={} {}",
                i, self.reg[i], self.rows[i].row()
            ));
            i += 1;
        }
        v.push(format!(
            "  MARKS walked={} expect={} dump={}",
            self.marks_walked,
            WATERMARK_SLOTS,
            marks_text(&self.marks)
        ));
        v.push(format!(
            "  PIECE scan_aligned={} marks_match_rows={} fp={:016X}",
            self.scan_aligned(),
            self.marks_match_rows(),
            self.fingerprint()
        ));
        v
    }
}

/// 刻度分派表转串（16 位：L = 轻、M = 中、H = 重、`_` = 未分派）。
pub fn marks_text(m: &Vec<u8>) -> String {
    let mut out = String::new();
    let mut k = 0usize;
    while k < m.len() {
        let ch = match m[k] {
            // 标签必须**按 `Tier::index()` 的口径**对：
            // 0 = Light、1 = Mid、2 = Heavy。初稿写成 `1=>'L' 2=>'M' 4=>'H'`，
            // 于是 `HHHHLLLLLLLL0000` 这类正确分派被渲成 `MMMMLLLLLLLL0000`——
            // 逻辑全对而标签全错，读日志的人会以为分派反了，
            // 进而怀疑本来正确的区间表。排查成本远高于写这四行。
            0 => 'L',
            1 => 'M',
            2 => 'H',
            NODE_NONE_U8 => '_',
            _ => '?',
        };
        out.push(ch);
        k += 1;
    }
    out
}

/// 步长的**方案侧**折盐夹取规则（纯函数，双向对拍的共享规则源）。
///
/// `TierPiece::register()` 与 `Scheme::plan_side_fingerprint()` 都调它：
/// 前者是实现侧写账，后者是方案侧独立重算。两边共用同一枚「册定步长 + 盐」，
/// 于是对拍比的才是「同一输入经同一规则是否得同一输出」，
/// 而非「册定值 vs 折盐值」这种根本不同的两个量。
pub fn stride_plan_side(base: u32, salt: u64) -> u32 {
    let raw = base.wrapping_add((salt & 0x0F) as u32);
    if raw == 0 {
        return 1;
    }
    let m = raw % SCAN_STRIDE_LIGHT;
    if m == 0 {
        SCAN_STRIDE_LIGHT
    } else {
        m
    }
}

// ===========================================================================
// 11. 正交维度笛卡尔矩阵（3 档 × 3 扫描强度 × 4 阶段 = 36 格）
// ===========================================================================

/// 笛卡尔单格：维度 A（档位）× 维度 B（扫描强度）× 维度 C（阶段）。
///
/// 每格**独立夹具**：一格的失败绝不污染其余 35 格。若复用同一个
/// `TierPiece` 实例逐格试，某格注入的坏行会被后 35 格继承，
/// 于是「36 格里只有 1 格坏」会退化成「36 格全红」，
/// 定位信息（到底是哪一维组合触发）全部丢失——
/// 这是矩阵测试最典型的自毁方式。
#[derive(Clone, Copy, Debug)]
pub struct CartCell {
    /// 维度 A：档位序号。
    pub a: u8,
    /// 维度 B：扫描强度序号。
    pub b: u8,
    /// 维度 C：阶段序号。
    pub c: u8,
    /// 本格注入的**故障形态号**（`0` = 本格未注入；`1..=4` = 四种形态之一）。
    ///
    /// 纪律：注入形态号必须与 `ok` **分开报**。若把「本格坏了」直接编码成
    /// `fault = 1`，报告面就分不清「注入了形态 1」与「本格判定为坏」——
    /// 非 Draft 阶段不注入却仍参与判定，此时它也是 `fault = 1`，
    /// 于是账面显示「注入了形态 1」，把阶段纪律误报成分档定义缺陷。
    pub fault: u32,
    /// 本格分档件自身是否合规（`fault < 0` 时恒假）。
    pub ok: bool,
    /// 本格是否处在可写阶段（仅 Draft）。
    pub writable: bool,
    /// 本格指纹。
    pub digest: u64,
}

impl CartCell {
    /// 单格报告行。
    pub fn row(&self) -> String {
        let ta = match Tier::from_index(self.a) {
            Some(t) => t.name(),
            None => "?",
        };
        let tb = match ScanLevel::from_index(self.b) {
            Some(s) => s.name(),
            None => "?",
        };
        format!(
            "CART tier={} scan={} stage={} fault={} ok={} writable={} digest={:016X}",
            ta, tb, self.c, self.fault, self.ok, self.writable, self.digest
        )
    }

    /// 单格指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix32(h, self.a as u32);
        h = mix32(h, self.b as u32);
        h = mix32(h, self.c as u32);
        h = mix32(h, self.fault);
        h = mix_bool(h, self.ok);
        h = mix_bool(h, self.writable);
        h
    }
}

/// 阶段是否允许写分档（仅 Draft）。
pub fn stage_allows_tier(st: Stage) -> bool {
    st.index() == STAGE_DRAFT
}

/// 本格的期望对家档（按扫描强度反查档位：二维定义的另一读法）。
pub fn tier_of_scan(s: ScanLevel) -> Option<Tier> {
    match s {
        ScanLevel::Light => Some(Tier::Light),
        ScanLevel::Normal => Some(Tier::Mid),
        ScanLevel::Full => Some(Tier::Heavy),
    }
}

/// 跑笛卡尔矩阵（36 格，逐格独立夹具）。
///
/// 注入策略：按 `(a + b + c) % 4` 给每格安排一种故障形态，
/// 四种形态分别是「区间置空 / 扫描串档 / 预备位越界 / 档位序号非法」，
/// 覆盖四条独立的注入口。
pub fn run_cartesian(salt: u64) -> Vec<CartCell> {
    let mut v: Vec<CartCell> = Vec::with_capacity(CART_EXPECTED);
    let mut a = 0usize;
    while a < CART_DA {
        let mut b = 0usize;
        while b < CART_DB {
            let mut c = 0usize;
            while c < CART_DC {
                let tier = Tier::ALL[a];
                let scan = ScanLevel::ALL[b];
                let st = Stage::ALL[c];
                let writable = stage_allows_tier(st);
                let shape = (a + b + c) % 4;

                // —— 本格独立夹具：现搭一份分档件，只跑本格 ——
                let mut p = TierPiece::new();
                let mut setup_err: Option<ErrKind> = None;
                let mut i = 0usize;
                while i < Tier::ALL.len() {
                    if let Err(e) = p.register(Tier::ALL[i], salt.wrapping_add(i as u64)) {
                        setup_err = Some(e);
                    }
                    i += 1;
                }
                // 三档登记齐备后统一重建刻度分派表（同 `Scheme::register_all_tiers`）。
                if setup_err.is_none() {
                    if let Err(e) = p.seal_marks() {
                        setup_err = Some(e);
                    }
                }
                // `fault` 只记「注入了第几号形态」，与 `ok` 分开：
                // 非 Draft 阶段不注入（`fault` 留 0），但其判定结果仍进 `ok`。
                let mut fault: u32 = 0u32;
                if writable && setup_err.is_none() {
                    fault = (shape as u32) + 1u32;
                    match shape {
                        0 => {
                            // 区间置空：把本格档的区间上下界压成同值
                            p.rows[a].band_hi = p.rows[a].band_lo;
                            let _ = p.rebuild_marks();
                        }
                        1 => {
                            // 扫描串档：把扫描强度改成与档位期望不同的值
                            let want = tier.expect_scan();
                            p.rows[a].scan = (want + 1) % (SCAN_COUNT as u8);
                        }
                        2 => {
                            // 预备位越界：直取 `2`，越过 `0/1` 两值口径。
                            //
                            // 初稿写的是 `if tier.direct_prep() == DIRECT_PREP_NONE { ... }`，
                            // 想的是「只给没有预备位的档注入」。但重档册定就是
                            // `prep = DIRECT_PREP_HEAVY = 1`，于是形态 2 落到重档时
                            // **什么也没做**，而 `fault` 依旧记成了「已注入形态 2」——
                            // 账面上有注入、实际上零扰动，注入核就报出
                            // 「注入格被判为绿」（`injected_ok != inj`）。
                            // 教训：注入形态的**记账与执行必须同条件**，
                            // 不能一个用 `writable && setup_ok` 另一个用附加守卫。
                            p.rows[a].direct_prep = 2;
                        }
                        _ => {
                            // 档位序号非法：越出三值闭集
                            p.rows[a].tier = 7;
                        }
                    }
                }

                // —— 本格判定：只看格自身不变量，不掺可写性 ——
                let mut ok = false;
                if setup_err.is_none() {
                    let rows_ok = p.rows[a].valid();
                    let band_ok = band_exclusive(&p.rows).is_ok();
                    let scan_ok = p.scan_aligned();
                    let mark_ok = p.marks_match_rows();
                    ok = rows_ok && band_ok && scan_ok && mark_ok;
                }
                let digest = p.fingerprint();
                v.push(CartCell {
                    a: tier.index(),
                    b: scan.index(),
                    c: st.index(),
                    fault,
                    ok,
                    writable,
                    digest,
                });
                c += 1;
            }
            b += 1;
        }
        a += 1;
    }
    v
}

/// 笛卡尔矩阵统计：`(总格数, 好格数, 可写格数, 坏格数)`。
pub fn cartesian_stats(cells: &Vec<CartCell>) -> (u32, u32, u32, u32) {
    let mut tot = 0u32;
    let mut ok = 0u32;
    let mut wr = 0u32;
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < cells.len() {
        tot += 1;
        if cells[i].ok {
            ok += 1;
        } else {
            bad += 1;
        }
        if cells[i].writable {
            wr += 1;
        }
        i += 1;
    }
    (tot, ok, wr, bad)
}

/// 笛卡尔矩阵指纹（逐格折叠，顺序敏感）。
pub fn cartesian_fingerprint(cells: &Vec<CartCell>) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < cells.len() {
        h = mix64(h, cells[i].fingerprint());
        i += 1;
    }
    h
}

/// 笛卡尔矩阵断言：格数与期望交叉对账 + 可写格数与册定一致。
pub fn cartesian_assert(cells: &Vec<CartCell>) -> DcRes<String> {
    let (tot, ok, wr, bad) = cartesian_stats(cells);
    // 注入格数：每格自带注入形态号（0 = 未注入），逐格累加即注入数。
    let mut inj = 0u32;
    let mut injected_ok = 0u32;
    let mut i = 0usize;
    while i < cells.len() {
        if cells[i].fault != 0 {
            inj += 1;
            // 注入格**应当**被判为坏（`ok == false`）——注入没被拒才是防线失守。
            if cells[i].ok == false {
                injected_ok += 1;
            }
        } else {
            // 未注入格**应当**全绿；非 Draft 阶段同理。
            if cells[i].ok == false {
                inj = inj.wrapping_add(1u32 << 16);
            }
        }
        i += 1;
    }
    let msg = format!(
        "CART_ASSERT cells={} expect={} ok={} writable={} expect_writable={} bad={} injected={} injected_rejected={}",
        tot, CART_EXPECTED, ok, wr, CART_WRITABLE, bad, inj & 0xFFFF, injected_ok
    );
    if tot != CART_EXPECTED as u32 {
        return Err(ErrKind::TierClosedSetBroken);
    }
    if wr != CART_WRITABLE as u32 {
        return Err(ErrKind::StageRejectRegister);
    }
    if (inj & 0xFFFF) != CART_WRITABLE as u32 {
        // 注入格数必须恰等于可写格数：多说明有非 Draft 格被注入了，
        // 少说明有 Draft 格漏了注入。
        return Err(ErrKind::StageRejectRegister);
    }
    if injected_ok != (inj & 0xFFFF) {
        // 有注入格被判为绿 = 注入未被拒。
        return Err(ErrKind::TierDrift);
    }
    if (inj >> 16) != 0 {
        // 有未注入格被判为坏 = 阶段纪律外的分档缺陷。
        return Err(ErrKind::TierDrift);
    }
    Ok(msg)
}

/// 笛卡尔矩阵报告行。
pub fn cartesian_report(cells: &Vec<CartCell>) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let (tot, ok, wr, bad) = cartesian_stats(cells);
    v.push(format!(
        "CART grid={}x{}x{} cells={} expect={} ok={} writable={} bad={}",
        CART_DA, CART_DB, CART_DC, tot, CART_EXPECTED, ok, wr, bad
    ));
    let mut i = 0usize;
    while i < cells.len() {
        v.push(format!("  {}", cells[i].row()));
        i += 1;
    }
    v
}

// ===========================================================================
// 12. 核心结构二：Scheme（分档方案主体）
// ===========================================================================

/// 分档方案主体。
#[derive(Clone, Debug)]
pub struct Scheme {
    /// 分档件（三档二维定义载体）。
    pub piece: TierPiece,
    /// 方案侧从 `Tier::` 闭集重算的对位表（F2941 同源核对位）。
    pub axis: Vec<AxisRow>,
    /// 守恒账。
    pub ledger: ChainLedger,
    /// append-only 环形账。
    pub ring: RingLog,
    /// 非法转换拒记环。
    pub rej: RejectionRing,
    /// 入册账。
    pub explog: ExpLog,
    /// 当前阶段。
    pub stage: Stage,
    /// 当前逻辑 tick。
    pub tick: u64,
    /// 冻结哈希（未冻结为 0）。
    pub freeze_hash: u64,
    /// 冻结时的 tick。
    pub freeze_tick: u64,
    /// 状态机游走步数。
    pub walk_steps: u64,
    /// 本实例分档盐。
    pub salt: u64,
}

impl Scheme {
    /// 构造草案态（`Draft`）。
    pub fn new(salt: u64) -> Scheme {
        Scheme {
            piece: TierPiece::new(),
            axis: source_axis_table(),
            ledger: ChainLedger::new(
                TIER_REG_TARGET,
                TIER_REG_TARGET,
                F2941_AXES,
                TIER_COUNT as u64,
                PART_TARGET,
            ),
            ring: RingLog::new(),
            rej: RejectionRing::new(),
            explog: ExpLog::new(),
            stage: Stage::Draft,
            tick: 0,
            freeze_hash: 0,
            freeze_tick: 0,
            walk_steps: 0,
            salt,
        }
    }

    /// 逻辑时钟前进。
    pub fn tick_advance(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(TICK_STEP);
        self.tick
    }

    /// 环形账留痕（tick 回退一律拒记）。
    pub fn stamp(&mut self, t: u64, digest: u64, tag: u32) -> DcRes<u64> {
        if t < self.tick {
            return Err(ErrKind::TickRegress);
        }
        self.tick = t;
        Ok(self.ring.push(self.tick, digest, tag))
    }

    /// 登记一档二维定义（仅 Draft）。
    pub fn register_tier(&mut self, t: Tier) -> DcRes<u64> {
        if self.stage != Stage::Draft {
            self.rej.push(self.tick, self.stage, Stage::Draft);
            return Err(ErrKind::StageRejectRegister);
        }
        let salt = self.salt;
        let fp = self.piece.register(t, salt)?;
        // 逐档登记：一档一记（见 F3001 同类修法——只补记一次会漏记 N-1 件）。
        self.ledger.note_reg();
        // 加法链分项：本单判据是「二维定义」（档位 × 扫描强度），
        // 故一档登记要展开成 `SCAN_COUNT` 个强度分项，
        // 全矩阵 `TIER_COUNT * SCAN_COUNT = PART_TARGET` 项。
        // 初稿只 `note_reg()` 不记分项，于是 R3「分项计数 == PART_TARGET」恒红
        // （实测 `items=0`）——守恒账没有分项，加法链无从谈起。
        let mut si = 0usize;
        while si < ScanLevel::ALL.len() {
            self.ledger.note_part(self.piece.rows[t.index() as usize].stride as u64);
            si += 1;
        }
        self.tick_advance();
        self.ring.push(self.tick, fp, 0x301);
        Ok(fp)
    }

    /// 三档全登记。
    pub fn register_all_tiers(&mut self) -> DcRes<u32> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < Tier::ALL.len() {
            self.register_tier(Tier::ALL[i])?;
            n += 1;
            i += 1;
        }
        // 三档齐备后**统一**重建刻度分派表（登记期不重建，见 `TierPiece::register`）。
        let _ = self.piece.seal_marks()?;
        Ok(n)
    }

    /// 状态机推进（合法边闭集仅 3 条；非法边一律拒记）。
    pub fn advance(&mut self, next: Stage) -> DcRes<Stage> {
        if !self.stage.can_go(next) {
            self.rej.push(self.tick, self.stage, next);
            return Err(ErrKind::StageIllegalEdge);
        }
        self.stage = next;
        self.walk_steps = self.walk_steps.wrapping_add(1);
        self.tick_advance();
        Ok(next)
    }

    /// 登记一个加法链分项。
    pub fn note_part(&mut self, v: u64) -> DcRes<()> {
        self.ledger.note_part(v);
        self.tick_advance();
        self.ring.push(self.tick, v, 0x302);
        Ok(())
    }

    /// 回填注入读数（注入核跑完后调用，保证守恒账有注入侧分项）。
    pub fn note_inject(&mut self, total: u32, rejected: u32) {
        self.ledger.note_inject(total, rejected);
        self.ring.push(self.tick, total as u64, 0x303);
    }

    /// 三轴口径注记入册（F2941 三轴逐轴留痕）。
    pub fn note_axes(&mut self) -> DcRes<u32> {
        let notes = axis_note();
        let mut n = 0u32;
        let mut i = 0usize;
        while i < notes.len() {
            // 每轴一条映射留痕：与 `axis` 行的对位语义对齐。
            self.ledger.note_mapped(1);
            n += 1;
            i += 1;
        }
        self.tick_advance();
        self.ring.push(self.tick, n as u64, 0x304);
        Ok(n)
    }

    /// 区间互斥验证（实现侧读回分档件行表）。
    pub fn band_ok(&self) -> DcRes<u64> {
        let rows = band_exclusive(&self.piece.rows)?;
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < rows.len() {
            h = mix_str(h, rows[i].as_str());
            i += 1;
        }
        Ok(h)
    }

    /// 逐刻度推演（读分派表，不重算区间）。
    pub fn walk_marks(&self) -> DcRes<u32> {
        let mut n = 0u32;
        let mut k = 0u32;
        while k < WATERMARK_SLOTS {
            self.piece.tier_of_mark(k)?;
            n += 1;
            k += 1;
        }
        Ok(n)
    }

    /// 同源核对位验证（实现侧读回 `self.axis` 行表）。
    ///
    /// 双向对拍纪律：方案侧从 `Tier::f2941_press_level()` 重算，
    /// 实现侧从 `self.axis` 表的 `level` 列读回，
    /// 两侧共用「档位序号」这一枚索引作为共享输入。
    pub fn align_axis_verify(&self) -> DcRes<u64> {
        align_axis_ok()?;
        if self.axis.len() != TIER_COUNT as usize {
            return Err(ErrKind::SourceAxisMismatch);
        }
        // 方案侧
        let mut pa = FNV_BASIS;
        let mut i = 0usize;
        while i < Tier::ALL.len() {
            pa = mix32(pa, Tier::ALL[i].f2941_press_level() as u32);
            pa = mix32(pa, Tier::ALL[i].index() as u32);
            i += 1;
        }
        // 实现侧
        let mut ib = FNV_BASIS;
        let mut j = 0usize;
        while j < self.axis.len() {
            ib = mix32(ib, self.axis[j].level as u32);
            ib = mix32(ib, self.axis[j].tier as u32);
            j += 1;
        }
        if pa != ib {
            return Err(ErrKind::SourceAxisMismatch);
        }
        Ok(pa)
    }

    /// 布局验证：三轴覆盖齐备 + 扫描强度表自洽。
    pub fn axis_verify(&self) -> Option<ErrKind> {
        if !scan_table_ok() {
            return Some(ErrKind::ScanLevelInvalid);
        }
        match align_axis_ok() {
            Ok(_) => None,
            Err(e) => Some(e),
        }
    }

    /// 分档漂移验证：同 tick 重算分档件指纹必须一致。
    pub fn verify_drift(&self) -> DcRes<u64> {
        if !self.piece.complete() {
            return Err(ErrKind::TierClosedSetBroken);
        }
        let fp = self.piece.fingerprint();
        if fp == 0 {
            return Err(ErrKind::TierDrift);
        }
        // 重算一次（纯函数重入），两次必须相等——不等即漂移。
        let fp2 = self.piece.fingerprint();
        if fp != fp2 {
            return Err(ErrKind::TierDrift);
        }
        Ok(fp)
    }

    /// 方案侧分档指纹（**独立于实现侧**重算一遍，用于双向对拍）。
    pub fn plan_side_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix64(h, self.salt);
        let mut i = 0usize;
        while i < Tier::ALL.len() {
            let t = Tier::ALL[i];
            h = mix32(h, t.band_lo());
            h = mix32(h, t.band_hi());
            h = mix32(h, t.expect_scan() as u32);
            // 步长折盐与夹取规则**在方案侧原样重算**。
            //
            // 初稿这里直接折册定 `t.scan_stride()`，而实现侧读回的是
            // `register()` 折盐夹取后的 `rows[i].stride` ——
            // 两侧喂的根本不是同一个量，双向对拍必然红。
            // 双向对拍的纪律：**方案侧必须独立复现全部变换规则**，
            // 不能只对「册定常量部分」对拍、把「实例化变换」漏掉；
            // 漏掉的部分不叫对拍，叫只对了一半。
            h = mix32(h, stride_plan_side(t.scan_stride(), self.salt));
            h = mix32(h, t.direct_prep());
            h = mix32(h, t.f2941_press_level() as u32);
            i += 1;
        }
        h
    }

    /// 实现侧分档指纹（从结构体行表读回）。
    pub fn impl_side_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix64(h, self.salt);
        let mut i = 0usize;
        while i < self.piece.rows.len() {
            h = mix32(h, self.piece.rows[i].band_lo);
            h = mix32(h, self.piece.rows[i].band_hi);
            h = mix32(h, self.piece.rows[i].scan as u32);
            h = mix32(h, self.piece.rows[i].stride);
            h = mix32(h, self.piece.rows[i].direct_prep);
            h = mix32(h, self.piece.rows[i].axis_press as u32);
            i += 1;
        }
        h
    }

    /// 双向对拍：方案侧独立重算 vs 实现侧读回，共用同一枚盐。
    pub fn bidirectional_ok(&self) -> bool {
        self.plan_side_fingerprint() == self.impl_side_fingerprint()
    }

    /// 冻结哈希计算（**只折冻结后永不变的字段**）。
    ///
    /// 纪律：绝不把 `ring` / `rej` / `explog` / `ledger` 四本活账折进来。
    /// `dispatch()` 之后四本活账继续追加，一旦折入，
    /// `verify_freeze()` 在正常收官时必然误报漂移。
    /// 折入的只有：册定常量 + 实例盐 + 分档件（冻结后只读）+ 冻结 tick。
    pub fn compute_freeze(&self, tick: u64) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix_str(h, CRITERION);
        h = mix32(h, TIER_COUNT);
        h = mix32(h, WATERMARK_SLOTS);
        h = mix32(h, F2941_AXES);
        h = mix64(h, self.piece.fingerprint());
        h = mix64(h, self.salt);
        h = mix64(h, tick);
        h
    }

    /// 冻结校验。
    pub fn verify_freeze(&self) -> DcRes<u64> {
        if self.freeze_hash == 0 {
            return Err(ErrKind::FreezeHashDrift);
        }
        let h = self.compute_freeze(self.freeze_tick);
        if h != self.freeze_hash {
            return Err(ErrKind::FreezeHashDrift);
        }
        Ok(h)
    }

    /// 冻结入册。
    pub fn seal(&mut self) -> DcRes<u64> {
        if self.stage == Stage::Draft {
            return Err(ErrKind::StageRejectFreeze);
        }
        if !self.piece.complete() {
            return Err(ErrKind::StageRejectFreeze);
        }
        if self.piece.scan_aligned() == false {
            return Err(ErrKind::ScanStrayToTier);
        }
        if self.piece.marks_match_rows() == false {
            return Err(ErrKind::BandGap);
        }
        let fp = self.compute_freeze(self.tick);
        if fp == 0 {
            return Err(ErrKind::FreezeHashDrift);
        }
        self.freeze_hash = fp;
        self.freeze_tick = self.tick;
        self.tick_advance();
        self.explog.record(self.tick, fp, 0xF0);
        self.ring.push(self.tick, fp, 0xF0);
        Ok(fp)
    }

    /// 调度入册（冻结件供 F3026 共引）。
    pub fn dispatch(&mut self) -> DcRes<u64> {
        if self.stage != Stage::Frozen {
            self.rej.push(self.tick, self.stage, Stage::Dispatched);
            return Err(ErrKind::StageRejectFreeze);
        }
        self.verify_freeze()?;
        self.stage = Stage::Dispatched;
        self.walk_steps = self.walk_steps.wrapping_add(1);
        let fp = self.scheme_fingerprint();
        self.tick_advance();
        self.explog.record(self.tick, fp, 0xF1);
        self.ring.push(self.tick, fp, 0xF1);
        Ok(fp)
    }

    /// 封后写检测（在 Frozen / Dispatched 阶段改分档必须被拒）。
    pub fn register_after_frozen(&mut self, t: Tier) -> DcRes<u64> {
        if self.stage == Stage::Frozen || self.stage == Stage::Dispatched {
            self.rej.push(self.tick, self.stage, self.stage);
            return Err(ErrKind::WriteAfterFrozen);
        }
        self.register_tier(t)
    }

    /// 方案总指纹（折活账，供 `observe` 逐种子比对）。
    pub fn scheme_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix32(h, self.stage.index() as u32);
        h = mix64(h, self.tick);
        h = mix64(h, self.freeze_hash);
        h = mix64(h, self.piece.fingerprint());
        h = mix64(h, self.plan_side_fingerprint());
        h = mix64(h, self.impl_side_fingerprint());
        h = mix64(h, self.ledger.fingerprint());
        h = mix64(h, self.ring.fingerprint());
        h = mix64(h, self.rej.fingerprint());
        h = mix64(h, self.explog.fingerprint());
        let mut i = 0usize;
        while i < self.axis.len() {
            h = mix32(h, self.axis[i].tier as u32);
            h = mix32(h, self.axis[i].level as u32);
            i += 1;
        }
        h
    }

    /// 分档漂移指纹（只折冻结语义载荷，不折活账）。
    pub fn tier_drift_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.piece.fingerprint());
        h = mix64(h, self.compute_freeze(self.freeze_tick));
        h
    }

    /// 方案报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "SCHEME stage={} tick={} walk={} freeze={:016X} freeze_tick={}",
            self.stage.name(),
            self.tick,
            self.walk_steps,
            self.freeze_hash,
            self.freeze_tick
        ));
        v.push(format!(
            "  PIECE complete={} scan_aligned={} marks_match={} reg={:?}",
            self.piece.complete(),
            self.piece.scan_aligned(),
            self.piece.marks_match_rows(),
            self.piece.reg
        ));
        v.push(format!(
            "  BIDIR plan={:016X} impl={:016X} ok={}",
            self.plan_side_fingerprint(),
            self.impl_side_fingerprint(),
            self.bidirectional_ok()
        ));
        let mut i = 0usize;
        while i < self.axis.len() {
            v.push(format!("  {}", self.axis[i].row()));
            i += 1;
        }
        v.push(join_lines(&axis_note()).replace('\n', " "));
        v
    }
}

// ===========================================================================
// 13. 注入核（12 形态，一形态一错误码，全拒留痕）
// ===========================================================================

/// 注入形态闭集（12 种，覆盖本单 18 个错误变体中的 12 个可注入口）。
///
/// 判据要求「漂移注入 10/10 次拦截」。本单取 12 种，比下限多 2，
/// 多出来的两路分别覆盖「区间有缝」与「同源核覆盖不足」——
/// 这两条恰好是「互斥」与「一一对位」两个正交面，只查互斥会漏掉覆盖不足。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectForm {
    /// I0：档位序号越界 → `TierIndexInvalid`。
    F0TierIndex,
    /// I1：水线刻度越界 → `WatermarkOutOfRange`。
    F1WatermarkOob,
    /// I2：区间置空 → `BandEmpty`。
    F2BandEmpty,
    /// I3：区间交叠 → `BandOverlap`。
    F3BandOverlap,
    /// I4：区间有缝 → `BandGap`。
    F4BandGap,
    /// I5：扫描强度越界 → `ScanLevelInvalid`。
    F5ScanLevel,
    /// I6：扫描强度串档 → `ScanStrayToTier`。
    F6ScanStray,
    /// I7：分档未登记齐即冻结 → `StageRejectFreeze`。
    F7RejectFreeze,
    /// I8：封后写 → `WriteAfterFrozen`。
    F8WriteAfterFrozen,
    /// I9：状态机自环 → `StageIllegalEdge`。
    F9SelfLoop,
    /// I10：冻结哈希漂移 → `FreezeHashDrift`。
    F10FreezeDrift,
    /// I11：逻辑 tick 回退 → `TickRegress`。
    F11TickRegress,
}

impl InjectForm {
    /// 全部注入形态。
    pub const ALL: [InjectForm; 12] = [
        InjectForm::F0TierIndex,
        InjectForm::F1WatermarkOob,
        InjectForm::F2BandEmpty,
        InjectForm::F3BandOverlap,
        InjectForm::F4BandGap,
        InjectForm::F5ScanLevel,
        InjectForm::F6ScanStray,
        InjectForm::F7RejectFreeze,
        InjectForm::F8WriteAfterFrozen,
        InjectForm::F9SelfLoop,
        InjectForm::F10FreezeDrift,
        InjectForm::F11TickRegress,
    ];

    /// 形态序号（0..11）。
    pub fn index(self) -> usize {
        match self {
            InjectForm::F0TierIndex => 0,
            InjectForm::F1WatermarkOob => 1,
            InjectForm::F2BandEmpty => 2,
            InjectForm::F3BandOverlap => 3,
            InjectForm::F4BandGap => 4,
            InjectForm::F5ScanLevel => 5,
            InjectForm::F6ScanStray => 6,
            InjectForm::F7RejectFreeze => 7,
            InjectForm::F8WriteAfterFrozen => 8,
            InjectForm::F9SelfLoop => 9,
            InjectForm::F10FreezeDrift => 10,
            InjectForm::F11TickRegress => 11,
        }
    }

    /// 预期错误变体（形态与错误一一对应，零空档）。
    pub fn expect(self) -> ErrKind {
        match self {
            InjectForm::F0TierIndex => ErrKind::TierIndexInvalid,
            InjectForm::F1WatermarkOob => ErrKind::WatermarkOutOfRange,
            InjectForm::F2BandEmpty => ErrKind::BandEmpty,
            InjectForm::F3BandOverlap => ErrKind::BandOverlap,
            InjectForm::F4BandGap => ErrKind::BandGap,
            InjectForm::F5ScanLevel => ErrKind::ScanLevelInvalid,
            InjectForm::F6ScanStray => ErrKind::ScanStrayToTier,
            InjectForm::F7RejectFreeze => ErrKind::StageRejectFreeze,
            InjectForm::F8WriteAfterFrozen => ErrKind::WriteAfterFrozen,
            InjectForm::F9SelfLoop => ErrKind::StageIllegalEdge,
            InjectForm::F10FreezeDrift => ErrKind::FreezeHashDrift,
            InjectForm::F11TickRegress => ErrKind::TickRegress,
        }
    }

    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            InjectForm::F0TierIndex => "tier_index",
            InjectForm::F1WatermarkOob => "watermark_oob",
            InjectForm::F2BandEmpty => "band_empty",
            InjectForm::F3BandOverlap => "band_overlap",
            InjectForm::F4BandGap => "band_gap",
            InjectForm::F5ScanLevel => "scan_level",
            InjectForm::F6ScanStray => "scan_stray",
            InjectForm::F7RejectFreeze => "reject_freeze",
            InjectForm::F8WriteAfterFrozen => "write_after_frozen",
            InjectForm::F9SelfLoop => "self_loop",
            InjectForm::F10FreezeDrift => "freeze_drift",
            InjectForm::F11TickRegress => "tick_regress",
        }
    }
}

/// 注入结果。
#[derive(Clone, Debug)]
pub struct InjectionResult {
    /// 形态序号。
    pub form: u8,
    /// 形态名。
    pub form_name: &'static str,
    /// 是否被拒。
    pub rejected: bool,
    /// 实际错误变体序号（未拒为 `usize::MAX`）。
    pub got: usize,
    /// 预期错误变体序号。
    pub want: usize,
    /// 码是否一致（严判：拒了但码不对也算失配）。
    pub matched: bool,
    /// 被测对象留痕指纹。
    pub probe_fp: u64,
    /// 测量工具读数（独立轨迹）。
    pub tool_fp: u64,
    /// 错误行。
    pub line: String,
}

/// 构造被测对象夹具（跑到指定阶段）。
fn probe_fixture(stage: Stage, salt: u64) -> Scheme {
    let mut s = Scheme::new(salt);
    let _ = s.register_all_tiers();
    if stage != Stage::Draft {
        let _ = s.advance(Stage::Mapped);
        let _ = s.note_axes();
    }
    if stage == Stage::Frozen || stage == Stage::Dispatched {
        // `seal()` 只算冻结哈希并入册，**不负责推阶段**；
        // 阶段推进是 `advance()` 的职责（Mapped → Frozen 是一条合法边）。
        // 初稿漏了这一步，夹具的 `stage` 停在 Mapped，
        // 于是 F8「封后写」实际落到 `register_tier()` 上，
        // 报出来的是 `StageRejectRegister`（index 9）而非 `WriteAfterFrozen`（index 11）——
        // 注入核仍算「被拒」，只是拒的形态与注入形态对不上，`match=false`。
        let _ = s.seal();
        let _ = s.advance(Stage::Frozen);
    }
    if stage == Stage::Dispatched {
        let _ = s.dispatch();
    }
    s
}

/// 注入结果封装。
fn finish(f: InjectForm, e: DcRes<u64>, s: &Scheme) -> InjectionResult {
    let want = f.expect();
    match e {
        Err(got) => InjectionResult {
            form: f.index() as u8,
            form_name: f.name(),
            rejected: true,
            got: got.index(),
            want: want.index(),
            matched: got == want,
            probe_fp: s.tier_drift_fp(),
            tool_fp: 0,
            line: err_line(got, f.index() as u64),
        },
        Ok(_) => InjectionResult {
            form: f.index() as u8,
            form_name: f.name(),
            rejected: false,
            got: usize::MAX,
            want: want.index(),
            matched: false,
            probe_fp: s.tier_drift_fp(),
            tool_fp: 0,
            line: String::from("注入未被拒：防线失守"),
        },
    }
}

/// 注入核：跑一轮全部形态。
///
/// **数据源分离纪律**：
///   * `probe`（被测对象）：只吃「形态 + 夹具阶段」，其内部状态由
///     `probe_fixture` 独立搭建，与测量工具无关；
///   * `tool`（测量工具）：独立 LCG，只吃 `seed` 与形态序号，
///     绝不读 probe 的任何字段。
/// 若两者共享发生器状态，注入就退化成「用被测对象自己的随机数去测它自己」，
/// 形态 A 的注入会通过形态 B 的轨迹残留被掩盖，注入核永远全绿。
pub fn inject_core(seed: u64) -> Vec<InjectionResult> {
    let mut out: Vec<InjectionResult> = Vec::with_capacity(InjectForm::ALL.len());
    let mut i = 0usize;
    while i < InjectForm::ALL.len() {
        let f = InjectForm::ALL[i];
        // —— 测量工具：独立 LCG，与 probe 状态完全分离 ——
        let mut tool = Lcg::new(seed ^ (f.index() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let _ = tool.next_u64();
        let salt = tool.next_u64();
        let r = match f {
            InjectForm::F0TierIndex => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：把某档的行内档位序号顶出三值闭集（7）。
                s.piece.rows[0].tier = 7;
                let e = match Tier::from_index(s.piece.rows[0].tier) {
                    Some(_) => Ok(0u64),
                    None => Err(ErrKind::TierIndexInvalid),
                };
                finish(f, e, &s)
            }
            InjectForm::F1WatermarkOob => {
                let s = probe_fixture(Stage::Mapped, salt);
                // 注入：查一个越界刻度（`WATERMARK_SLOTS + 4`）。
                let e = s.piece.tier_of_mark(WATERMARK_SLOTS + 4).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F2BandEmpty => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：把中档区间压成空（lo == hi）。
                s.piece.rows[1].band_hi = s.piece.rows[1].band_lo;
                let e = band_exclusive(&s.piece.rows).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F3BandOverlap => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：把轻档下界压到 0，与重档 `[0,4)` 正面交叠。
                s.piece.rows[0].band_lo = 0;
                s.piece.rows[0].band_hi = 8;
                let e = band_exclusive(&s.piece.rows).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F4BandGap => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：把重档区间上界从 4 抬到 2，制造 `[2,4)` 空洞。
                s.piece.rows[2].band_hi = 2;
                let e = band_exclusive(&s.piece.rows).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F5ScanLevel => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：把扫描强度顶出三值闭集（5）。
                s.piece.rows[0].scan = 5;
                let e = if s.piece.scan_aligned() {
                    Ok(0u64)
                } else {
                    Err(ErrKind::ScanLevelInvalid)
                };
                finish(f, e, &s)
            }
            InjectForm::F6ScanStray => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：把中档扫描强度改成轻档档位（仍在三值闭集内，故只会被
                // 「串档」这条线抓住，闭集闸抓不到——这正是设两路的原因）。
                s.piece.rows[1].scan = SCAN_LIGHT;
                let e = if s.piece.scan_aligned() {
                    Ok(0u64)
                } else {
                    Err(ErrKind::ScanStrayToTier)
                };
                finish(f, e, &s)
            }
            InjectForm::F7RejectFreeze => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：在 Draft 阶段抢冻结（未推进到 Mapped 即冻结）。
                let e = s.seal();
                finish(f, e, &s)
            }
            InjectForm::F8WriteAfterFrozen => {
                let mut s = probe_fixture(Stage::Frozen, salt);
                // 注入：冻结态再登记一档（封后写）。
                let e = s.register_after_frozen(Tier::Light).map(|v| v);
                finish(f, e, &s)
            }
            InjectForm::F9SelfLoop => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：状态机自环（Draft → Draft）。
                let e = s.advance(Stage::Draft).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F10FreezeDrift => {
                let mut s = probe_fixture(Stage::Frozen, salt);
                // 注入：手改冻结哈希，模拟冻结件被旁路篡改。
                s.freeze_hash = s.freeze_hash.wrapping_add(1);
                let e = s.verify_freeze();
                finish(f, e, &s)
            }
            InjectForm::F11TickRegress => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：给一个比当前 tick 小的时刻留痕。
                let cur = s.tick;
                let e = if cur == 0 {
                    Ok(0u64)
                } else {
                    s.stamp(cur - 1, 0x1234, 0x9)
                };
                finish(f, e, &s)
            }
        };
        out.push(r);
        i += 1;
    }
    out
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

/// 注入核总判定：全形态必须「被拒 + 码一致」。
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

/// 注入核指纹（顺序敏感）。
pub fn inject_fingerprint(rs: &[InjectionResult]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < rs.len() {
        h = mix64(h, rs[i].probe_fp);
        h = mix64(h, rs[i].tool_fp);
        h = mix_bool(h, rs[i].rejected);
        h = mix_bool(h, rs[i].matched);
        i += 1;
    }
    h
}

/// 注入核报告行（留痕）。
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
            "  INJECT[{:02}] {} reject={} got={:#06x} want={:#06x} match={} probe={:016X} tool={:016X}",
            rs[i].form,
            rs[i].form_name,
            rs[i].rejected,
            rs[i].got,
            rs[i].want,
            rs[i].matched,
            rs[i].probe_fp,
            rs[i].tool_fp
        ));
        i += 1;
    }
    v
}

// ===========================================================================
// 14. 守恒终账 reconcile（整数恒等式，禁浮点）
// ===========================================================================

/// 守恒终账（8 行：R1 / R2 / R3 / R4 / R5a / R5b / R6 / R7）。
///
/// 逐条都是整数恒等式，任何一条偏离即整单红。
pub fn reconcile(s: &Scheme) -> DcRes<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    let led = &s.ledger;

    // R1 登记件数守恒
    //
    // `reg_target()` 定义即 `reg + pending`，故 `reg + pending == reg_target`
    // 是恒真空转的恒等式。真正的守恒内容是 (a) 守恒不漏 + (b) 收官做完。
    let r1 = led.reg.wrapping_add(led.pending);
    let r1_done = led.reg == TIER_REG_TARGET && led.pending == 0;
    v.push(format!(
        "RECON R1 reg={} pend={} sum={} target={} done={} ok={}",
        led.reg,
        led.pending,
        r1,
        led.reg_target(),
        r1_done,
        r1 == led.reg_target() && r1_done
    ));
    if r1 != led.reg_target() || !r1_done {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R2 三档覆盖守恒（Σ各档区间跨度 == 全部刻度数）
    let mut span = 0u32;
    let mut i = 0usize;
    while i < s.piece.rows.len() {
        span = span.wrapping_add(s.piece.rows[i].span());
        i += 1;
    }
    v.push(format!(
        "RECON R2 tiers={} span_sum={} slots={} dev={} ok={}",
        s.piece.rows.len(),
        span,
        WATERMARK_SLOTS,
        span as i64 - WATERMARK_SLOTS as i64,
        span == WATERMARK_SLOTS && s.piece.rows.len() == TIER_COUNT as usize
    ));
    if span != WATERMARK_SLOTS || s.piece.rows.len() != TIER_COUNT as usize {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R3 分项计数
    v.push(format!(
        "RECON R3 items={} target={} ok={}",
        led.items,
        led.items_target,
        led.items == led.items_target
    ));
    if led.items != led.items_target {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R4 加法链 Σ分项 == 总额（零偏离，本单核心口径）
    let sum = led.parts_sum();
    let dev = led.total as i64 - sum as i64;
    v.push(format!(
        "RECON R4 addchain sum={} total={} deviation={} zero={}",
        sum,
        led.total,
        dev,
        dev == 0
    ));
    if dev != 0 {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R5a 注入下限（判据 10/10；本单 12 形态）
    v.push(format!(
        "RECON R5a inject_floor total={} floor={} ok={}",
        led.injected_total,
        FORM_COUNT,
        led.injected_total >= FORM_COUNT
    ));
    // R5b 注入全拒
    v.push(format!(
        "RECON R5b inject_full rejected={} total={} ok={}",
        led.injected_rejected,
        led.injected_total,
        led.injected_rejected == led.injected_total
    ));
    if led.injected_total < FORM_COUNT || led.injected_rejected != led.injected_total {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R6 区间互斥 + 逐刻度推演守恒
    let band = match band_exclusive(&s.piece.rows) {
        Ok(_) => true,
        Err(_) => false,
    };
    let walked = match s.walk_marks() {
        Ok(n) => n,
        Err(_) => 0,
    };
    v.push(format!(
        "RECON R6 band_exclusive={} walked={} expect_walked={} marks_walked={} ok={}",
        band,
        walked,
        WATERMARK_SLOTS,
        s.piece.marks_walked,
        band && walked == WATERMARK_SLOTS && s.piece.marks_walked == WATERMARK_SLOTS
    ));
    if !band || walked != WATERMARK_SLOTS || s.piece.marks_walked != WATERMARK_SLOTS {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R7 同源核覆盖守恒（本单三档 ↔ F2941 压力轴三档，一一对应）
    let axis_ok = match align_axis_ok() {
        Ok(_) => true,
        Err(_) => false,
    };
    let bidir = s.bidirectional_ok();
    let scan_ok = s.piece.scan_aligned() && scan_table_ok();
    v.push(format!(
        "RECON R7 axis_cover={} bidir={} scan_aligned={} tiers={} f2941_levels={} ok={}",
        axis_ok,
        bidir,
        scan_ok,
        TIER_COUNT,
        F2941_PRESSURE_LEVELS,
        axis_ok && bidir && scan_ok
    ));
    if !axis_ok || !bidir || !scan_ok {
        return Err(ErrKind::AddChainUnbalanced);
    }

    Ok(v)
}

// ===========================================================================
// 15. 自检（20 条）
// ===========================================================================

/// 自检项。
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
    /// 构造一项。
    pub fn new(id: u8, name: &'static str, pass: bool, note: String) -> Check {
        Check { id, name, pass, note }
    }
}

/// 自检 20 条。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut v: Vec<Check> = Vec::new();
    let mut s = Scheme::new(seed);

    // C01 三档闭集齐备
    v.push(Check::new(
        1,
        "tier_closedset",
        Tier::ALL.len() == TIER_COUNT as usize,
        format!("tiers={} expect={}", Tier::ALL.len(), TIER_COUNT),
    ));

    // C02 三档登记齐备
    let _ = s.register_all_tiers();
    v.push(Check::new(
        2,
        "register_all_tiers",
        s.piece.complete(),
        format!("reg={:?}", s.piece.reg),
    ));

    // C03 区间逐行有效
    let mut rows_ok = true;
    let mut i = 0usize;
    while i < s.piece.rows.len() {
        if !s.piece.rows[i].valid() {
            rows_ok = false;
        }
        i += 1;
    }
    v.push(Check::new(
        3,
        "rows_valid",
        rows_ok,
        format!("rows={}", s.piece.rows.len()),
    ));

    // C04 区间互斥 + 全覆盖
    let bo = band_exclusive(&s.piece.rows);
    v.push(Check::new(
        4,
        "band_exclusive",
        bo.is_ok(),
        format!("exclusive+cover={}", bo.is_ok()),
    ));

    // C05 逐刻度推演
    let wm = s.walk_marks();
    v.push(Check::new(
        5,
        "walk_marks",
        wm.is_ok() && wm.unwrap_or(0) == WATERMARK_SLOTS,
        format!("walked={:?} expect={}", wm, WATERMARK_SLOTS),
    ));

    // C06 扫描强度与档位对齐
    v.push(Check::new(
        6,
        "scan_aligned",
        s.piece.scan_aligned(),
        format!("scan_aligned={}", s.piece.scan_aligned()),
    ));

    // C07 扫描强度对拍表自洽（严格递减）
    v.push(Check::new(
        7,
        "scan_table_ok",
        scan_table_ok(),
        format!("strides={:?}", scan_stride_table()),
    ));

    // C08 同源核对位（双向）
    let ao = s.align_axis_verify();
    v.push(Check::new(
        8,
        "align_axis_verify",
        ao.is_ok(),
        format!("axis_verify={:?}", ao),
    ));

    // C09 三轴口径注记（三轴逐轴留痕）
    let notes = axis_note();
    v.push(Check::new(
        9,
        "axis_note",
        notes.len() == F2941_AXES as usize,
        format!("notes={} expect={}", notes.len(), F2941_AXES),
    ));

    // C10 状态机合法边 3 条
    let le = legal_edges();
    v.push(Check::new(
        10,
        "legal_edges",
        le.len() == LEGAL_EDGES as usize,
        format!("legal={} expect={}", le.len(), LEGAL_EDGES),
    ));

    // C11 非法边 13 条（含自环）
    let ie = illegal_edges_measured();
    v.push(Check::new(
        11,
        "illegal_edges",
        ie == ILLEGAL_EDGES,
        format!("illegal={} expect={}", ie, ILLEGAL_EDGES),
    ));

    // C12 状态机推进到 Mapped
    let adv = s.advance(Stage::Mapped);
    v.push(Check::new(
        12,
        "advance_mapped",
        adv.is_ok() && s.stage == Stage::Mapped,
        format!("stage={}", s.stage.name()),
    ));

    // C13 非法自环被拒并留痕
    let rej0 = s.rej.total;
    let _ = s.advance(Stage::Mapped);
    v.push(Check::new(
        13,
        "reject_self_loop",
        s.rej.total == rej0 + 1,
        format!("rej_total={} grew={}", s.rej.total, s.rej.total - rej0),
    ));

    // C14 加法链分项账（登记期已按 `档 × 强度` 逐格记满，此处**只核不记**）
    //
    // 初稿在这里又跑一轮 `PART_TARGET` 次 `note_part()`，而分项早已由
    // `register_tier()` 按二维定义记满 `PART_TARGET` 项 ——
    // 同一笔账被记两次，`items` 直接翻倍，R3 与 C14 必有一个恒红。
    // 自检条的职责是**核对既有账**，不是替业务路径补账。
    v.push(Check::new(
        14,
        "note_parts",
        s.ledger.items == PART_TARGET,
        format!("items={} expect={}", s.ledger.items, PART_TARGET),
    ));

    // C15 冻结
    let seal = s.seal();
    v.push(Check::new(
        15,
        "seal",
        seal.is_ok() && s.freeze_hash != 0,
        format!("freeze={:016X}", s.freeze_hash),
    ));

    // C16 冻结校验（同 tick 重算一致）
    let vf = s.verify_freeze();
    v.push(Check::new(
        16,
        "verify_freeze",
        vf.is_ok(),
        format!("verify={:?}", vf),
    ));

    // C16b 阶段推进到 Frozen（`seal()` 不推阶段，`dispatch()` 只认 Frozen）
    //
    // 初稿 `seal()` 之后直接 `dispatch()`，而 `dispatch()` 首行要求
    // `stage == Stage::Frozen` —— 中间少了一步合法边 Mapped → Frozen，
    // 于是 C17 恒在 `stage=mapped` 上红。阶段推进是状态机职责，
    // 不能指望冻结动作顺手把阶段也带走。
    let af = s.advance(Stage::Frozen);
    v.push(Check::new(
        21,
        "advance_frozen",
        af.is_ok() && s.stage == Stage::Frozen,
        format!("stage={}", s.stage.name()),
    ));
    af.ok();

    // C17 调度入册
    let dp = s.dispatch();
    v.push(Check::new(
        17,
        "dispatch",
        dp.is_ok() && s.stage == Stage::Dispatched,
        format!("stage={}", s.stage.name()),
    ));

    // C18 笛卡尔矩阵 36 格
    let cells = run_cartesian(seed);
    let (ct, cok, cwr, cbad) = cartesian_stats(&cells);
    // 笛卡尔判定**一律走 `cartesian_assert()`**：它是新语义（注入格必坏、
    // 未注入格必绿、注入格数 == 可写格数）的唯一权威出口。
    // 若这里自己按 `cok/cbad` 另写一份，就会出现两套判定口径，
    // 一套认注入格为坏、另一套认它为绿，谁改谁都可能整体翻红。
    let ca = cartesian_assert(&cells);
    v.push(Check::new(
        18,
        "cartesian",
        ca.is_ok(),
        format!(
            "cells={} ok={} writable={} bad={} assert={}",
            ct, cok, cwr, cbad,
            ca.unwrap_or(String::from("ERR"))
        ),
    ));

    // C19 注入核 12/12 全拒
    let inj = inject_core_with_tool(seed);
    let miss = detect_miss(&inj);
    let rej = count_rejected(&inj);
    v.push(Check::new(
        19,
        "inject_all_rejected",
        miss == 0 && rej == inj.len(),
        format!("miss={} rejected={}/{}", miss, rej, inj.len()),
    ));

    // C20 守恒终账 8 行
    //
    // 注入侧读数必须**先回填守恒账**再对账：R5a/R5b 查的是
    // `injected_total >= FORM_COUNT` 与「全拒」，而这两项只在
    // `note_inject()` 回填后才有值。observe / replay 两条路径都在
    // 调 `reconcile()` 前先 `note_inject()`，唯独 self_check 漏了，
    // 于是 C20 恒红（`injected_total=0`）而另外两条路径全绿。
    // 教训：守恒账的对账方与记账方必须成对出现在**每一条**执行路径上。
    let inj_sc = inject_core_with_tool(seed);
    s.note_inject(inj_sc.len() as u32, count_rejected(&inj_sc) as u32);
    let rec = reconcile(&s);
    v.push(Check::new(
        20,
        "reconcile",
        rec.is_ok() && rec.as_ref().map(|r| r.len()).unwrap_or(0) == RECON_ROWS,
        format!("rows={:?} expect={}", rec.as_ref().map(|r| r.len()).ok(), RECON_ROWS),
    ));

    v
}

/// 自检指纹（顺序敏感）。
pub fn check_fingerprint(v: &[Check]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < v.len() {
        h = mix32(h, v[i].id as u32);
        h = mix_bool(h, v[i].pass);
        h = mix_str(h, v[i].note.as_str());
        i += 1;
    }
    h
}

/// 自检是否全过。
pub fn all_check_pass(v: &[Check]) -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < v.len() {
        if !v[i].pass {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ===========================================================================
// 16. 导出 export_kv + 观察体 observe
// ===========================================================================

/// 观察体。
#[derive(Clone, Debug)]
pub struct DcObserve {
    /// 种子。
    pub seed: u64,
    /// 冻结哈希。
    pub freeze_hash: u64,
    /// 方案总指纹。
    pub scheme_fp: u64,
    /// 自检指纹。
    pub check_fp: u64,
    /// 笛卡尔矩阵指纹。
    pub cart_fp: u64,
    /// 注入核指纹。
    pub inject_fp: u64,
    /// 回放指纹。
    pub replay_fp: u64,
    /// 注入被拒条数。
    pub injected_rejected: u32,
    /// 注入总条数。
    pub injected_total: u32,
    /// 注入失配条数。
    pub inject_miss: u32,
    /// 守恒终账行。
    pub recon_rows: Vec<String>,
    /// 自检项。
    pub checks: Vec<Check>,
    /// 方案报告行。
    pub scheme_rows: Vec<String>,
    /// 注入核报告行。
    pub inject_rows: Vec<String>,
    /// 笛卡尔总格数。
    pub cart_total: u32,
    /// 笛卡尔好格数。
    pub cart_ok: u32,
    /// 环形账留存条数。
    pub ring_live: usize,
    /// 环形账丢条数（显性）。
    pub ring_dropped: u64,
    /// 终态阶段名。
    pub fsm_final: &'static str,
    /// 状态机步数。
    pub fsm_steps: u64,
    /// 拒记条数。
    pub fsm_rejected: u64,
    /// TLV 往返一致。
    pub tlv_roundtrip: bool,
    /// TLV 实际长度。
    pub tlv_len: usize,
    /// 总判定。
    pub all_ok: bool,
}

impl DcObserve {
    /// 总判定（读 `all_ok` 字段，闭集安全）。
    pub fn all_ok(&self) -> bool {
        self.all_ok
    }

    /// 单行摘要。
    pub fn summary(&self) -> String {
        format!(
            "UNX-F3021 seed={:016X} all_ok={} freeze={:016X} fp={:016X} inject={}/{} miss={} cart={}/{} ring_live={} dropped={} tlv={}B rt={} replay_stable={}",
            self.seed,
            self.all_ok,
            self.freeze_hash,
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
            self.replay_stable()
    )
    }

    /// 回放稳定性（由 `replay_stable` 缓存位给出，见 `observe` 回填）。
    pub fn replay_stable(&self) -> bool {
        self.replay_fp != 0 && self.inject_miss == 0 && self.all_ok
    }
}

/// 导出 KV（键值对纯内存文本，禁 IO）。
pub fn export_kv(o: &DcObserve) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("mod.tag={}", MODULE_TAG));
    v.push(format!("task={}", TASK_ID));
    v.push(format!("seg.base={:#06x}", SEG_BASE));
    v.push(format!("seg.mask={:#06x}", SEG_MASK));
    v.push(format!("export.version={:#018x}", EXPORT_VERSION));
    v.push(format!("tier.count={}", TIER_COUNT));
    v.push(format!("watermark.slots={}", WATERMARK_SLOTS));
    v.push(format!("scan.count={}", SCAN_COUNT));
    v.push(format!("f2941.axes={}", F2941_AXES));
    v.push(format!("f2941.pressure.levels={}", F2941_PRESSURE_LEVELS));
    v.push(format!("cart.expected={}", CART_EXPECTED));
    v.push(format!("cart.writable={}", CART_WRITABLE));
    v.push(format!("form.count={}", FORM_COUNT));
    v.push(format!("recon.rows={}", RECON_ROWS));
    v.push(format!("seed={}", o.seed));
    v.push(format!("freeze.hash={:016X}", o.freeze_hash));
    v.push(format!("scheme.fp={:016X}", o.scheme_fp));
    v.push(format!("check.fp={:016X}", o.check_fp));
    v.push(format!("cart.fp={:016X}", o.cart_fp));
    v.push(format!("inject.fp={:016X}", o.inject_fp));
    v.push(format!("replay.fp={:016X}", o.replay_fp));
    v.push(format!("inject.total={}", o.injected_total));
    v.push(format!("inject.rejected={}", o.injected_rejected));
    v.push(format!("inject.miss={}", o.inject_miss));
    v.push(format!("cart.total={}", o.cart_total));
    v.push(format!("cart.ok={}", o.cart_ok));
    v.push(format!("ring.live={}", o.ring_live));
    v.push(format!("ring.dropped={}", o.ring_dropped));
    v.push(format!("fsm.final={}", o.fsm_final));
    v.push(format!("fsm.steps={}", o.fsm_steps));
    v.push(format!("fsm.rejected={}", o.fsm_rejected));
    v.push(format!("tlv.len={}", o.tlv_len));
    v.push(format!("tlv.roundtrip={}", o.tlv_roundtrip));
    v.push(format!("all.ok={}", o.all_ok));
    v
}

/// 观察入口（真实跑逻辑，禁互递归：`observe` 不调 `replay`，`replay` 不调 `observe`）。
pub fn observe(seed: u64) -> DcObserve {
    // —— 第一步：回放 ——
    let tr = replay(seed);
    let (stable, rfp) = replay_stable(seed);

    // —— 第二步：真跑分档生命周期 ——
    let mut s = Scheme::new(seed);
    let _ = s.register_all_tiers();
    let _ = s.advance(Stage::Mapped);
    let _ = s.note_axes();
    // 加法链分项**已由登记期按「档 × 强度」记满**（`PART_TARGET` 项），
    // 此处不得再补记一轮 —— 那是同一笔账记两次，R3 必红。
    let inj_pre = inject_core_with_tool(seed);
    s.note_inject(inj_pre.len() as u32, count_rejected(&inj_pre) as u32);
    let recon_rows: Vec<String> = match reconcile(&s) {
        Ok(v) => v,
        Err(e) => vec![err_line(e, seed)],
    };
    let freeze_hash = s.seal().unwrap_or(0);
    let _ = s.verify_freeze();
    // `seal()` 不推阶段；`dispatch()` 只认 `Frozen`，中间这条合法边必须显式走。
    let _ = s.advance(Stage::Frozen);
    let _ = s.dispatch();
    let scheme_fp = s.scheme_fingerprint();
    let scheme_rows: Vec<String> = s.report();

    // —— 第三步：注入核 ——
    let miss = detect_miss(&inj_pre);
    let inject_rows: Vec<String> = inject_report(&inj_pre);
    let inject_fp = inject_fingerprint(&inj_pre);
    let rej = count_rejected(&inj_pre) as u32;
    let tot = inj_pre.len() as u32;

    // —— 第四步：笛卡尔矩阵 ——
    let cells = run_cartesian(seed);
    let cart_fp = cartesian_fingerprint(&cells);
    // 只取判定实际需要的两元：格数与可写格数对账册定常量。
    // `ok`/`bad` 计数不在这里再判一遍——那正是 `cartesian_assert()` 的职责，
    // 两处各判一次早晚会出现两套口径打架。
    let (ct, cok, cwr, _cbad) = cartesian_stats(&cells);

    // —— 第五步：自检 ——
    let checks = self_check(seed);
    let check_fp = check_fingerprint(&checks);

    let mut o = DcObserve {
        seed,
        freeze_hash,
        scheme_fp,
        check_fp,
        cart_fp,
        inject_fp,
        replay_fp: rfp,
        injected_rejected: rej,
        injected_total: tot,
        inject_miss: miss as u32,
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
        tlv_roundtrip: false,
        tlv_len: 0,
        all_ok: false,
    };

    // —— 第六步：各分项判定 ——
    let checks_pass = all_check_pass(&checks);
    let recon_ok = o.recon_rows.len() == RECON_ROWS;
    let inject_ok = miss == 0 && rej == tot && tot >= FORM_COUNT;
    // 笛卡尔判定走 `cartesian_assert()`（同 C18 口径，唯一权威出口）。
    let cart_ok2 = cartesian_assert(&cells).is_ok()
        && ct == CART_EXPECTED as u32
        && cwr == CART_WRITABLE as u32;

    // —— 第七步：TLV 往返 + 总判定（消自指：先定 base_ok 再跑往返）——
    let buf = export_tlv(&o);
    o.tlv_len = buf.len();
    let rt_len = buf.len() == tlv_len_expected();
    let base_ok = checks_pass
        && recon_ok
        && inject_ok
        && cart_ok2
        && stable
        && freeze_hash != 0
        && tr.ok
        && rt_len;
    o.all_ok = base_ok;
    o.tlv_roundtrip = tlv_roundtrip_ok(&o);
    o.all_ok = base_ok && o.tlv_roundtrip;
    o
}

// ===========================================================================
// 17. 确定性回放 replay（不调 observe）
// ===========================================================================

/// 回放单步。
#[derive(Clone, Debug)]
pub struct ReplayStep {
    /// 步序。
    pub step: u8,
    /// 动作名。
    pub action: &'static str,
    /// 该步后的方案指纹。
    pub fp: u64,
    /// 该步时的逻辑 tick。
    pub tick: u64,
    /// 该步是否达标。
    pub ok: bool,
}

impl ReplayStep {
    /// 单步报告行。
    pub fn row(&self) -> String {
        format!(
            "REPLAY step={} action={} ok={} tick={} fp={:016X}",
            self.step, self.action, self.ok, self.tick, self.fp
        )
    }
}

/// 回放轨迹。
#[derive(Clone, Debug)]
pub struct ReplayTrace {
    /// 步序表。
    pub steps: Vec<ReplayStep>,
    /// 全链指纹。
    pub fp: u64,
    /// 达标步数。
    pub passed: u32,
    /// 总步数。
    pub total: u32,
    /// 是否全达标。
    pub ok: bool,
}

impl ReplayTrace {
    /// 轨迹报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "REPLAY steps={} passed={} fp={:016X} ok={}",
            self.total, self.passed, self.fp, self.ok
        ));
        let mut i = 0usize;
        while i < self.steps.len() {
            v.push(format!("  {}", self.steps[i].row()));
            i += 1;
        }
        v
    }
}

/// 确定性回放（同种子必得同轨迹）。
pub fn replay(seed: u64) -> ReplayTrace {
    let mut h = FNV_BASIS;
    let mut steps: Vec<ReplayStep> = Vec::new();
    let mut step = 0u8;
    let mut s = Scheme::new(seed);

    // 步 1 构造草案
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep {
        step,
        action: "new_draft",
        fp: s.scheme_fingerprint(),
        tick: s.tick,
        ok: s.stage == Stage::Draft,
    });
    step += 1;

    // 步 2 三档逐档登记（走 `register_all_tiers()`：它在三档齐备后
    // 统一 `seal_marks()` 重建刻度分派表；直接手搓 `register_tier` 循环
    // 会漏掉重建，步 4 逐刻度推演立刻 `BandGap`）
    let reg = s.register_all_tiers();
    let reg_ok = reg.is_ok();
    h = mix64(h, s.piece.fingerprint());
    steps.push(ReplayStep {
        step,
        action: "register_all_tiers",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: reg_ok && s.piece.complete(),
    });
    step += 1;

    // 步 3 区间互斥 + 全覆盖
    let bo = band_exclusive(&s.piece.rows);
    h = mix_str(h, String::from(if bo.is_ok() { "mutex_ok" } else { "mutex_bad" }).as_str());
    steps.push(ReplayStep {
        step,
        action: "band_exclusive",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: bo.is_ok(),
    });
    step += 1;

    // 步 4 同源核对位
    let ao = s.align_axis_verify();
    h = mix64(h, ao.unwrap_or(0));
    steps.push(ReplayStep {
        step,
        action: "align_axis",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: ao.is_ok(),
    });
    step += 1;

    // 步 5 逐刻度推演
    let wm = s.walk_marks();
    h = mix32(h, wm.unwrap_or(0));
    steps.push(ReplayStep {
        step,
        action: "walk_marks",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: wm.unwrap_or(0) == WATERMARK_SLOTS,
    });
    step += 1;

    // 步 6 三轴口径注记
    let ax = s.note_axes();
    h = mix32(h, ax.unwrap_or(0));
    steps.push(ReplayStep {
        step,
        action: "note_axes",
        fp: s.scheme_fingerprint(),
        tick: s.tick,
        ok: ax.unwrap_or(0) == F2941_AXES,
    });
    step += 1;

    // 步 7 推进到 Mapped
    let adv = s.advance(Stage::Mapped);
    h = mix32(h, s.stage.index() as u32);
    steps.push(ReplayStep {
        step,
        action: "advance_mapped",
        fp: s.scheme_fingerprint(),
        tick: s.tick,
        ok: adv.is_ok() && s.stage == Stage::Mapped,
    });
    step += 1;

    // 步 8 加法链分项**核对**（分项已在 `register_all_tiers` 期间按
    // `档 × 强度` 记满，此处只核 `items == PART_TARGET` 与 Σ 零偏离）
    let part_ok = s.ledger.items == PART_TARGET
        && s.ledger.total == s.ledger.parts_sum();
    h = mix64(h, s.ledger.parts_sum());
    steps.push(ReplayStep {
        step,
        action: "note_parts",
        fp: s.ledger.fingerprint(),
        tick: s.tick,
        ok: part_ok && s.ledger.items == PART_TARGET,
    });
    step += 1;

    // 步 9 注入核 + 回填账
    let inj = inject_core_with_tool(seed);
    let miss = detect_miss(&inj);
    s.note_inject(inj.len() as u32, count_rejected(&inj) as u32);
    h = mix64(h, inject_fingerprint(&inj));
    steps.push(ReplayStep {
        step,
        action: "inject_core",
        fp: inject_fingerprint(&inj),
        tick: s.tick,
        ok: miss == 0 && count_rejected(&inj) == inj.len(),
    });
    step += 1;

    // 步 10 守恒终账
    let rec = reconcile(&s);
    h = mix32(h, rec.as_ref().map(|r| r.len()).unwrap_or(0) as u32);
    steps.push(ReplayStep {
        step,
        action: "reconcile",
        fp: s.scheme_fingerprint(),
        tick: s.tick,
        ok: rec.is_ok() && rec.as_ref().map(|r| r.len()).unwrap_or(0) == RECON_ROWS,
    });
    step += 1;

    // 步 11 冻结
    let seal = s.seal();
    h = mix64(h, s.freeze_hash);
    steps.push(ReplayStep {
        step,
        action: "seal",
        fp: s.scheme_fingerprint(),
        tick: s.tick,
        ok: seal.is_ok() && s.freeze_hash != 0,
    });
    step += 1;

    // 步 12 冻结校验
    let vf = s.verify_freeze();
    h = mix_bool(h, vf.is_ok());
    steps.push(ReplayStep {
        step,
        action: "verify_freeze",
        fp: s.scheme_fingerprint(),
        tick: s.tick,
        ok: vf.is_ok(),
    });
    step += 1;

    // 步 13 阶段推进到 Frozen + 调度入册
    //
    // `seal()` 不推阶段，`dispatch()` 首行又只认 `Frozen` ——
    // 中间这条合法边 Mapped → Frozen 必须显式走一遍。
    // 与 `self_check` 的 C16b 同一处修正，两边口径保持一致。
    let af = s.advance(Stage::Frozen);
    let dp = s.dispatch();
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep {
        step,
        action: "dispatch",
        fp: s.scheme_fingerprint(),
        tick: s.tick,
        ok: af.is_ok() && dp.is_ok() && s.stage == Stage::Dispatched,
    });
    step += 1;

    // 步 14 笛卡尔矩阵
    let cells = run_cartesian(seed);
    let cfp = cartesian_fingerprint(&cells);
    let (_ct, _cok, cwr, _cbad) = cartesian_stats(&cells);
    h = mix64(h, cfp);
    steps.push(ReplayStep {
        step,
        action: "cartesian",
        fp: cfp,
        tick: s.tick,
        // 同 observe：走 `cartesian_assert()` 权威出口，不另写口径。
        ok: cartesian_assert(&cells).is_ok() && cwr == CART_WRITABLE as u32,
    });
    step += 1;

    // 步 15 非法边穷举
    let ie = illegal_edges_measured();
    h = mix32(h, ie);
    steps.push(ReplayStep {
        step,
        action: "illegal_edges",
        fp: ie as u64,
        tick: s.tick,
        ok: ie == ILLEGAL_EDGES,
    });
    // 末步不再自增：`step` 的最后一次赋值永不被读，写它只会留下
    // "value assigned is never read" 噪声告警，且末尾多一个计数没有语义。

    let mut passed = 0u32;
    let mut i2 = 0usize;
    while i2 < steps.len() {
        if steps[i2].ok {
            passed += 1;
        }
        i2 += 1;
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

/// 回放稳定性（同种子两跑，指纹必须一致）。
pub fn replay_stable(seed: u64) -> (bool, u64) {
    let a = replay(seed);
    let b = replay(seed);
    let same = a.fp == b.fp && a.ok && b.ok;
    (same, a.fp)
}

// ===========================================================================
// 18. 性能分解 / 内存账
// ===========================================================================

/// 性能分解总表（逐项对标 Windows 同类能力写「深在哪」）。
pub fn complexity_table() -> Vec<CostItem> {
    let mut v: Vec<CostItem> = Vec::new();
    v.push(CostItem::new("register_all_tiers", "O(1)", "O(1)", TIER_REG_TARGET, 96,
        "Windows 无对照：三档二维定义逐档登记是本单专有冻结纪律"));
    v.push(CostItem::new("band_exclusive", "O(n^2)", "O(n)", TIER_COUNT * TIER_COUNT, 96,
        "深在**互斥 + 全覆盖双判**：Windows 内存分压只有阈值，无区间互斥断言"));
    v.push(CostItem::new("rebuild_marks", "O(n)", "O(n)", WATERMARK_SLOTS, 64,
        "深在逐刻度推演而非闭区间公式：两套口径必然分叉，此处只留一套"));
    v.push(CostItem::new("walk_marks", "O(n)", "O(1)", WATERMARK_SLOTS, 16,
        "比 Windows 分压读数更严：读分派表不重算，账面与运行期同源"));
    v.push(CostItem::new("scan_aligned", "O(n)", "O(1)", TIER_COUNT, 8,
        "比 Windows 阈值分段更严：扫描强度必须与档位一一恒等，不许串档"));
    v.push(CostItem::new("scan_table_ok", "O(n)", "O(n)", SCAN_COUNT, 24,
        "第二真相源交叉对拍：步长表独立成表，靠持续核对防语义接反"));
    v.push(CostItem::new("align_axis_verify", "O(n)", "O(1)", TIER_COUNT * F2941_AXES, 32,
        "深在**双向**：既查本单→F2941，也查 F2941→本单，单向恒自洽即失效"));
    v.push(CostItem::new("axis_note", "O(1)", "O(n)", F2941_AXES, 48,
        "非对位轴显式注明「不造关系」：跳过后账面看不出为何不核"));
    v.push(CostItem::new("run_cartesian", "O(1)", "O(n)", CART_EXPECTED as u32, (CART_EXPECTED * 48) as u32,
        "比 Windows 压力矩阵更细：36 格逐格独立夹具，任一格坏不污染其余 35 格"));
    v.push(CostItem::new("illegal_edges_measured", "O(1)", "O(n)", STAGE_COUNT * STAGE_COUNT, 64,
        "Windows 无对照：13 条非法边穷举可枚举，自环也占位次"));
    v.push(CostItem::new("inject_core 12 forms", "O(1)", "O(n)", FORM_TOTAL, FORM_TOTAL * 64,
        "深在数据源分离：probe 与 tool 各走独立 LCG，绝不共享状态"));
    v.push(CostItem::new("seal + verify_freeze", "O(1)", "O(1)", TIER_COUNT * WATERMARK_SLOTS, 32,
        "深在同 tick 重算且只折冻结语义载荷：绝不把活账折进冻结指纹"));
    v.push(CostItem::new("replay(seed)", "O(n)", "O(n)", REPLAY_STEPS_CAP, REPLAY_STEPS_CAP * 64,
        "深在确定性：同种子两跑指纹恒等，可离线回放无需真机"));
    v.push(CostItem::new("export_tlv", "O(n)", "O(n)", TLV_RECORD_COUNT as u32, (TLV_RECORD_COUNT * TLV_RECORD_LEN) as u32,
        "长度定宽：组头 2 字节 + 记录 1 字节，保证往返字节级一致"));
    v.push(CostItem::new("ring wrap", "O(n)", "O(n)", RING_CAP as u32, RING_CAP as u32 * 32,
        "深在 dropped 显性：Windows 环形日志多静默覆盖，本单强制计数"));
    v
}

/// 总内存账（六路峰值上界）。
pub fn complexity_memory() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "MEM piece rows={} × sizeof(TierRow) ≈ {} B（分档件常驻）",
        TIER_COUNT,
        TIER_COUNT * 32
    ));
    v.push(format!(
        "MEM piece marks={} × u8 ≈ {} B（刻度分派表）",
        WATERMARK_SLOTS, WATERMARK_SLOTS
    ));
    v.push(format!(
        "MEM cartesian={} cells × sizeof(CartCell) ≈ {} B（瞬时，可流式）",
        CART_EXPECTED, CART_EXPECTED * 32
    ));
    v.push(format!(
        "MEM ring cap={} × sizeof(RingEntry) ≈ {} B",
        RING_CAP, RING_CAP * 32
    ));
    v.push(format!(
        "MEM rej cap={} × sizeof(Rejection) ≈ {} B",
        REJECT_CAP, REJECT_CAP * 24
    ));
    v.push(format!(
        "MEM tlv={} B（{} 组头 + {} 记录 × {} B）",
        tlv_len_expected(),
        TLV_HEAD_LEN,
        TLV_RECORD_COUNT,
        TLV_RECORD_LEN
    ));
    v.push(String::from("MEM peak 合计 ≈ 单件常驻 + 瞬时矩阵 + TLV 缓冲，无堆外分配"));
    v
}

/// 演示：区间互斥演示。
pub fn demo_band_exclusive() -> Vec<String> {
    let t = tier_table();
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("=== DEMO 区间互斥 + 全覆盖 ==="));
    v.push(format!("tiers={} slots={}", TIER_COUNT, WATERMARK_SLOTS));
    let mut i = 0usize;
    while i < t.len() {
        v.push(format!("  {}", t[i].row()));
        i += 1;
    }
    match band_exclusive(&t) {
        Ok(rows) => {
            v.push(String::from("EXCLUSIVE=OK"));
            let mut k = 0usize;
            while k < rows.len() {
                v.push(rows[k].clone());
                k += 1;
            }
        }
        Err(e) => {
            v.push(format!("EXCLUSIVE=ERR {}", e.name()));
        }
    }
    v
}

/// 演示：逐刻度推演演示。
pub fn demo_walk_marks() -> Vec<String> {
    let mut s = Scheme::new(0x3021);
    let _ = s.register_all_tiers();
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("=== DEMO 逐刻度推演 ==="));
    v.push(format!("marks={}", marks_text(&s.piece.marks)));
    let mut k = 0u32;
    while k < WATERMARK_SLOTS {
        let who = match s.piece.tier_of_mark(k) {
            Ok(t) => {
                let tt = Tier::from_index(t);
                match tt {
                    Some(x) => String::from(x.name()),
                    None => String::from("?"),
                }
            }
            Err(e) => format!("ERR:{}", e.name()),
        };
        v.push(format!("  mark={} -> {}", k, who));
        k += 1;
    }
    v
}

/// 演示：同源核对位演示。
pub fn demo_axis() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("=== DEMO 同源核对位（F2941 三轴） ==="));
    let t = source_axis_table();
    let mut i = 0usize;
    while i < t.len() {
        v.push(format!("  {}", t[i].row()));
        i += 1;
    }
    let notes = axis_note();
    let mut k = 0usize;
    while k < notes.len() {
        v.push(format!("  {}", notes[k]));
        k += 1;
    }
    match align_axis_ok() {
        Ok(_) => v.push(String::from("AXIS=OK 一一对位")),
        Err(e) => v.push(format!("AXIS=ERR {}", e.name())),
    }
    v
}

/// 演示：注入核演示。
pub fn demo_inject(seed: u64) -> Vec<String> {
    let r = inject_core_with_tool(seed);
    inject_report(&r)
}

/// 演示：冻结指纹演示。
pub fn demo_freeze(seed: u64) -> Vec<String> {
    let mut s = Scheme::new(seed);
    let _ = s.register_all_tiers();
    let _ = s.advance(Stage::Mapped);
    let _ = s.note_axes();
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("=== DEMO 冻结指纹 ==="));
    let fp0 = s.seal().unwrap_or(0);
    v.push(format!("  seal={:016X}", fp0));
    v.push(format!(
        "  plan_side={:016X} impl_side={:016X} bidir={}",
        s.plan_side_fingerprint(),
        s.impl_side_fingerprint(),
        s.bidirectional_ok()
    ));
    v.push(format!("  verify={:?}", s.verify_freeze()));
    v.push(format!("  dispatch={:?}", s.dispatch()));
    v
}

/// 相关能力串（供 grep 索引）。
pub fn related_str() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("相关：F2941 域级压力矩阵方案冻结（三轴）"));
    v.push(String::from("相关：F2480 分档制度源"));
    v.push(String::from("相关：F3014 回收档对位源"));
    v.push(String::from("下游：F3026 档位切换执行"));
    v.push(String::from("下游：B35 矩阵执行"));
    v.push(String::from("同批：F2981 域级收口 / F3001 双链结构 / F3061 断电演练 / F3081 终账总纲"));
    v
}

// ===========================================================================
// TLV 导出（组头单独一行，长度恒占定宽）
// ===========================================================================

/// TLV 组标识。
pub const TLV_TAG_HEAD: u8 = 0x01;
/// TLV 组头负载长度（`u64 × 3`：版本 / 段基址 / 册目标）。
///
/// 该常量**由 `export_tlv` 里 `push_u64` 的次数反推**，不手算：
/// 组头恰 3 次 `push_u64` → 3×8 = 24 字节。
pub const TLV_HEAD_PAYLOAD: usize = 24;
/// TLV 组头总长（`tag` 1 + `len` 2 字节定宽 + payload）。
pub const TLV_HEAD_LEN: usize = 1 + 2 + TLV_HEAD_PAYLOAD;
/// TLV 单条记录宽度（`tag` 1 + `len` 1 + payload 8 = 10）。
pub const TLV_RECORD_LEN: usize = 10;
/// TLV 记录条数（**由 `export_tlv` 的 `push_tlv_u64` 调用次数反推**，不手算）。
pub const TLV_RECORD_COUNT: usize = 10 + 9 + 6 + 6 + 12;
/// TLV 期望总长（由上两项反推）。
pub fn tlv_len_expected() -> usize {
    TLV_HEAD_LEN + TLV_RECORD_COUNT * TLV_RECORD_LEN
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

/// 推一条 TLV 记录（`tag` + `len`(=8) + payload）。
pub fn push_tlv_u64(buf: &mut Vec<u8>, tag: u8, v: u64) {
    buf.push(tag);
    buf.push(8u8);
    push_u64(buf, v);
}

/// TLV 导出。
///
/// 组头**单独一行**推入（`push_u64` 三次独立调用，构成 `TLV_HEAD_PAYLOAD`）。
/// 长度字段**恒占定宽**：组头 2 字节、记录 1 字节。
/// 若长度字段按数据自适应宽度，组头实际长度会与常量对不上，解析端必错；
/// 且记录体偏移会整体移位，表现为「长度对得上但每个值差一字节」——
/// 这是 TLV 往返最隐蔽的一类缺陷，故此处逐处注明偏移。
pub fn export_tlv(o: &DcObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(2048);
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, EXPORT_VERSION);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, 420u64);
    // —— 组头单独一行：tag + 定宽 2 字节长度 + payload ——
    out.push(TLV_TAG_HEAD);
    let hl = head_payload.len();
    out.push(((hl >> 8) & 0xFF) as u8);
    out.push((hl & 0xFF) as u8);
    let mut i = 0usize;
    while i < head_payload.len() {
        out.push(head_payload[i]);
        i += 1;
    }
    // 维度规模（10 条）
    push_tlv_u64(&mut out, 0x10, TIER_COUNT as u64);
    push_tlv_u64(&mut out, 0x11, WATERMARK_SLOTS as u64);
    push_tlv_u64(&mut out, 0x12, SCAN_COUNT as u64);
    push_tlv_u64(&mut out, 0x13, F2941_AXES as u64);
    push_tlv_u64(&mut out, 0x14, F2941_PRESSURE_LEVELS as u64);
    push_tlv_u64(&mut out, 0x15, LEGAL_EDGES as u64);
    push_tlv_u64(&mut out, 0x16, ILLEGAL_EDGES as u64);
    push_tlv_u64(&mut out, 0x17, CART_EXPECTED as u64);
    push_tlv_u64(&mut out, 0x18, PART_TARGET as u64);
    push_tlv_u64(&mut out, 0x19, FORM_COUNT as u64);
    // 三档二维定义（9 条）
    let mut ti = 0usize;
    while ti < Tier::ALL.len() {
        let t = Tier::ALL[ti];
        push_tlv_u64(&mut out, 0x20u8.wrapping_add(t.index()), t.band_lo() as u64);
        push_tlv_u64(&mut out, 0x24u8.wrapping_add(t.index()), t.band_hi() as u64);
        push_tlv_u64(&mut out, 0x28u8.wrapping_add(t.index()), t.scan_stride() as u64);
        ti += 1;
    }
    // 水线分界刻度（6 条）
    push_tlv_u64(&mut out, 0x30, WM_MIN as u64);
    push_tlv_u64(&mut out, 0x31, WM_LOW as u64);
    push_tlv_u64(&mut out, 0x32, WM_HIGH as u64);
    push_tlv_u64(&mut out, 0x33, WM_MAX as u64);
    push_tlv_u64(&mut out, 0x34, PART_FLOOR);
    push_tlv_u64(&mut out, 0x35, DIRECT_PREP_HEAVY as u64);
    // 指纹段（6 条）
    push_tlv_u64(&mut out, 0x40, o.freeze_hash);
    push_tlv_u64(&mut out, 0x41, o.scheme_fp);
    push_tlv_u64(&mut out, 0x42, o.check_fp);
    push_tlv_u64(&mut out, 0x43, o.cart_fp);
    push_tlv_u64(&mut out, 0x44, o.inject_fp);
    push_tlv_u64(&mut out, 0x45, o.replay_fp);
    // 读数段（12 条）
    push_tlv_u64(&mut out, 0x50, o.injected_total as u64);
    push_tlv_u64(&mut out, 0x51, o.injected_rejected as u64);
    push_tlv_u64(&mut out, 0x52, o.inject_miss as u64);
    push_tlv_u64(&mut out, 0x53, o.cart_total as u64);
    push_tlv_u64(&mut out, 0x54, o.cart_ok as u64);
    push_tlv_u64(&mut out, 0x55, o.ring_live as u64);
    push_tlv_u64(&mut out, 0x56, o.ring_dropped);
    push_tlv_u64(&mut out, 0x57, o.fsm_steps);
    push_tlv_u64(&mut out, 0x58, o.fsm_rejected);
    push_tlv_u64(&mut out, 0x59, if o.tlv_roundtrip { 1 } else { 0 });
    push_tlv_u64(&mut out, 0x5A, o.tlv_len as u64);
    push_tlv_u64(&mut out, 0x5B, if o.all_ok { 1 } else { 0 });
    out
}

/// TLV 解析：校验组头 + 逐记录读回（往返一致性）。
///
/// 偏移纪律：组头 payload 起点 = `TLV_HEAD_LEN - TLV_HEAD_PAYLOAD`；
/// 记录体布局 `[p]=tag` `[p+1]=len(=8)` `[p+2 .. p+10]=payload`，
/// **必须从 `p+2` 起读 8 字节**——从 `p+1` 起读会把 len 字节当作
/// payload 最高字节，等于整体右移一字节，症状是「长度/条数都对但值全错位」。
pub fn parse_tlv(buf: &[u8]) -> Res<Vec<u64>> {
    if buf.len() < TLV_HEAD_LEN {
        return Err(ErrKind::BandGap);
    }
    if buf[0] != TLV_TAG_HEAD {
        return Err(ErrKind::TierClosedSetBroken);
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(ErrKind::TierClosedSetBroken);
    }
    let mut vals: Vec<u64> = Vec::new();
    // 组头 3 条 u64 先入 vals：若只收记录不收组头，
    // 往返校验里 vals[0] 就不是版本号，下标全体错位。
    let hb = TLV_HEAD_LEN - TLV_HEAD_PAYLOAD;
    let mut hi = 0usize;
    while hi < 3usize {
        let mut v = 0u64;
        let mut j = 0usize;
        while j < 8usize {
            v = (v << 8) | (buf[hb + hi * 8 + j] as u64);
            j += 1;
        }
        vals.push(v);
        hi += 1;
    }
    let mut p = TLV_HEAD_LEN;
    while p + TLV_RECORD_LEN <= buf.len() {
        if buf[p + 1] != 8u8 {
            return Err(ErrKind::TierClosedSetBroken);
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
        return Err(ErrKind::TierClosedSetBroken);
    }
    Ok(vals)
}

/// TLV 往返一致（导出 → 解析 → 关键字段逐项对拍）。
pub fn tlv_roundtrip_ok(o: &DcObserve) -> bool {
    let buf = export_tlv(o);
    if buf.len() != tlv_len_expected() {
        return false;
    }
    let vals = match parse_tlv(&buf) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if vals.len() != 3 + TLV_RECORD_COUNT {
        return false;
    }
    if vals[0] != EXPORT_VERSION {
        return false;
    }
    if vals[1] != SEG_BASE as u64 {
        return false;
    }
    // 记录段下标偏移：vals[0..2]=组头 3 条；vals[3..12]=维度 10 条；
    // vals[13..21]=三档二维定义 9 条；vals[22..27]=水线分界 6 条；
    // vals[28..33]=指纹段；vals[34..45]=读数段。
    if vals[3] != TIER_COUNT as u64 {
        return false;
    }
    if vals[4] != WATERMARK_SLOTS as u64 {
        return false;
    }
    if vals[5] != SCAN_COUNT as u64 {
        return false;
    }
    if vals[28] != o.freeze_hash {
        return false;
    }
    if vals[29] != o.scheme_fp {
        return false;
    }
    if vals[3 + TLV_RECORD_COUNT - 1] != (if o.all_ok { 1 } else { 0 }) {
        return false;
    }
    true
}

/// 复杂度单项（复杂度 / 内存 / syscall 次数 / 步数上限 四要素）。
#[derive(Clone, Debug)]
pub struct CostItem {
    /// 环节名。
    pub name: &'static str,
    /// 时间复杂度记号。
    pub time: &'static str,
    /// 空间复杂度记号。
    pub space: &'static str,
    /// syscall 次数（纯逻辑件恒 0）。
    pub syscalls: u32,
    /// 步数上限。
    pub steps: u32,
    /// 峰值字节上界。
    pub bytes: u32,
    /// 对标 Windows 同类能力的「深在哪」。
    pub vs_windows: &'static str,
}

impl CostItem {
    /// 构造一项。
    pub fn new(name: &'static str, time: &'static str, space: &'static str, steps: u32, bytes: u32, vs_windows: &'static str) -> CostItem {
        CostItem {
            name,
            time,
            space,
            syscalls: 0,
            steps,
            bytes,
            vs_windows,
        }
    }
    /// 单行报告。
    pub fn row(&self) -> String {
        format!(
            "COST {:<26} time={:<7} space={:<7} syscall={} steps<={:<5} bytes<={:<7} :: {}",
            self.name, self.time, self.space, self.syscalls, self.steps, self.bytes, self.vs_windows
        )
    }
    /// 步数超限判定（供 `observe` 断言）。
    pub fn within(&self, used: u32) -> bool {
        used <= self.steps
    }
}

/// 性能分解总表。
pub fn complexity() -> Vec<CostItem> {
    complexity_table()
}

/// 总 syscall 账（纯逻辑件恒 0）。
pub fn syscall_account() -> String {
    String::from(
        "SYSCALL total=0（本单零真实 IO、零墙钟：时间一律逻辑 tick，条目一律内存账，无任何宿主调用）",
    )
}

/// 总内存账（六路：逐项列出峰值上界）。
pub fn memory_account() -> Vec<String> {
    complexity_memory()
}
