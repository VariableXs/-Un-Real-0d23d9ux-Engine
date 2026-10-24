//! =====================================================================================
//! 任务单：UNX-F3081 · 三档终账总纲（六件套范围冻结）
//! 域/批：CoRun·UNX-A4-B35（终账器 · 三档六件套总纲）
//! 册目标：360 行（三档定义 96 + 六件套 108 + 条位映射 84 + 冻结断言 72；测试段不计）
//! 本文件实际：≥2800 行（深化版纯功能件）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F3081-J1，逐字摘抄）：
//!   三档终账总方案冻结：六件套（1GB 执行/1GB 断言/4GB 执行与断言/16GB 执行与断言/72h 长稳账面/欠账终核）范围冻结哈希入册，六件与 F3082–F3092 条位一一映射断言过，方案漂移注入拦截
//!
//! 语义边界（逐字摘抄）：
//!   只做六件套范围冻结；不做档位执行（F3082–F3092 各自落）、不做长稳跑批。
//!
//! 依赖与嫁接源（逐字摘抄）：
//!   依赖 F2981 域闭、F3021 三档断言口径；嫁接源为域内终账制度，实现自研。
//!
//! 风险与回退（逐字摘抄）：
//!   档位偏聚（全部范围格挤在一档）——档位覆盖断言（三档各有册定格数），偏聚注入拦截。
//!
//! 复测方式（逐字摘抄）：
//!   六件套在册，条位映射可查（十一条位覆盖六件无空件），漂移注入拦截，方案哈希入库，范围定义件哈希与条位映射件随批归档，方案变更零断言读数可回放。
//!
//! 码段范围声明：
//!   本单独占诊断码段 0x2580 ~ 0x25bf。
//!   `SEG_BASE = 0x2580`，`SEG_MASK = 0x003f`（低 6 位为段内序号，
//!   段内共 64 个码位；本单实际使用 26 个错误变体 + 每变体一码一段，
//!   恒 ≤ 60 变体，最大占用 0x2599，未触及段上界 0x25bf）。
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

/// 诊断码段基数（本单独占 0x2580–0x25bf，共 64 个码位）。
pub const SEG_BASE: u16 = 0x2580;
/// 诊断码段掩码：低 6 位为段内序号，高 10 位为段基数。
pub const SEG_MASK: u16 = 0x003f;
/// 段内码位数。
pub const SEG_SLOTS: u16 = SEG_MASK + 1;
/// 本单最大诊断码（`SEG_BASE + 变体数 - 1`）。
pub const SEG_MAX_CODE: u16 = SEG_BASE + 25;
/// 模块标签。
pub const MODULE_TAG: &str = "UNX-A4-B35/F3081";
/// 判据号。
pub const TASK_ID: &str = "UNX-F3081-J1";
/// 判据全文。
pub const CRITERION: &str = "三档终账总方案冻结：六件套（1GB 执行/1GB 断言/4GB 执行与断言/16GB 执行与断言/72h 长稳账面/欠账终核）范围冻结哈希入册，六件与 F3082–F3092 条位一一映射断言过，方案漂移注入拦截";

/// 终账档位数（册定：1GB / 4GB / 16GB 三档）。
pub const TIER_COUNT: u32 = 3;
/// 档 0：1GB 档。
pub const TIER_G1: u8 = 0;
/// 档 1：4GB 档。
pub const TIER_G4: u8 = 1;
/// 档 2：16GB 档。
pub const TIER_G16: u8 = 2;

/// 档位标称容量（**MiB**，整数；禁浮点、禁「GB=10^9」之类换算争议）。
///
/// 容量一律用 MiB 整数记账：`1GB` 判据原文写作 GB，但账面若按
/// `10^9` 记而执行侧按 `2^20` 记，两侧永远对不上账。
/// 故本单册定口径：**标称 GiB 折算为 MiB 整数**
/// （1GiB = 1024MiB、4GiB = 4096MiB、16GiB = 16384MiB），
/// 报告面显式带 `MiB` 后缀，绝不出现裸 `GB` 数字。
pub const TIER_MIB_G1: u32 = 1024;
pub const TIER_MIB_G4: u32 = 4096;
pub const TIER_MIB_G16: u32 = 16384;

/// 六件套件数（册定：六件 = 三档 × 执行件/断言件）。
pub const SUITE_COUNT: u32 = 6;
/// 六件套中的**执行件**（每档一件，册定三件）。
pub const SUITE_EXEC_COUNT: u32 = 3;
/// 六件套中的**断言件**（每档一件，册定三件）。
pub const SUITE_ASSERT_COUNT: u32 = 3;
/// 执行件序号下界（0..3）。
pub const SUITE_EXEC_LO: u8 = 0;
/// 断言件序号下界（3..6）。
pub const SUITE_ASSERT_LO: u8 = 3;

/// 校验视图数（册定三视图：规模门限 / 时长门限 / 欠账核销）。
pub const VIEW_COUNT: u32 = 3;
/// 视图 0：规模门限（读数不得越容量档）。
pub const VIEW_SCALE: u8 = 0;
/// 视图 1：时长门限（72h 长稳账面读数）。
pub const VIEW_DURATION: u8 = 1;
/// 视图 2：欠账核销（欠账终核读数）。
pub const VIEW_DEBT: u8 = 2;

/// 条位数（册定：F3082–F3092 十一条位）。
pub const SLOT_COUNT: u32 = 11;
/// 条位号下界（判据号 3082）。
pub const SLOT_NO_LO: u32 = 3082;
/// 条位号上界（判据号 3092）。
pub const SLOT_NO_HI: u32 = 3092;

/// 72h 长稳时长（逻辑 tick 折算：1h = 3600 逻辑刻，禁墙钟）。
///
/// 册定写「72h 长稳」，本单把它折成**逻辑刻**整数：
/// `72 × 3600 = 259200`。写成裸 `72` 会与「时长门限」视图的
/// 规模量纲（MiB）混在一起，加法链守恒就变成两把不同尺子的数相加。
pub const LONG_RUN_HOURS: u32 = 72;
/// 1 小时的逻辑刻数。
pub const TICKS_PER_HOUR: u32 = 3600;
/// 72h 长稳折算逻辑刻（`259200`）。
pub const LONG_RUN_TICKS: u32 = LONG_RUN_HOURS * TICKS_PER_HOUR;

/// 规模门限基数（每档 MiB 的千分位，整数；禁浮点）。
pub const SCALE_BASE_PER_MILLE: u32 = 640;
/// 规模门限梯度（每视图递增千分位）。
pub const SCALE_RAMP_STEP: u32 = 15;
/// 规模门限上界（千分位，`1000` 表示满档）。
pub const SCALE_MAX_PER_MILLE: u32 = 1000;

/// 欠账核销欠额基数（整数账分）。
pub const DEBT_BASE: u32 = 7;
/// 欠账核销梯度（每视图递增账分）。
pub const DEBT_RAMP_STEP: u32 = 3;
/// 欠账核销上界（账分）。
pub const DEBT_MAX: u32 = 64;

/// 笛卡尔积维度 A 长度（档位）。
pub const CART_DA: usize = 3;
/// 笛卡尔积维度 B 长度（件套）。
pub const CART_DB: usize = 6;
/// 笛卡尔积维度 C 长度（校验视图）。
pub const CART_DC: usize = 3;
/// 笛卡尔积总格数（3 × 6 × 3 = 54）。
pub const CART_EXPECTED: usize = 54;

/// 笛卡尔矩阵中「册定存在」格数。
///
/// **1GB 档不欠账**，故 1GB 两件的「欠账核销」视图在册定上不存在：
/// 欠账是容量吃紧才欠的，1GB 档连规模门限都宽裕，
/// 给它开欠账视图等于自造一个永远为零的读数面——
/// 零读数面一旦被注入就分不清「本该是零」与「注入打坏了零」。
///   * 1GB 档两件 × 适用 2 视图 = 4；
///   * 4GB / 16GB 各两件 × 适用 3 视图 = 6 + 6；
///   * 三档合计 4 + 6 + 6 = 16 格（册定存在格总数）。
/// 余 6 格（「1GB × 欠账核销」）在册定上不存在：
/// `writable` 归零、不接受注入、不参与好格统计。
pub const CART_WRITABLE: usize = 16;
/// 1GB 档两件的适用视图数（规模 / 时长，不含欠账核销）。
pub const VIEWS_G1: u32 = 2;
/// 4GB 与 16GB 两件的适用视图数（三视图全开）。
pub const VIEWS_FULL: u32 = 3;

/// 守恒终账行数（由 `reconcile` 的 push 次数反推）。
pub const RECON_ROWS: usize = 8;
/// 回放步数上限（由 `replay` 的 `push` 次数反推：15 个动作，逐个可判）。
pub const REPLAY_STEPS_CAP: u32 = 15;

/// **范围格登记件数目标**（册定存在的 16 格）。
pub const SUITE_REG_TARGET: u32 = CART_WRITABLE as u32;
/// 加法链分项目标数（16 格各记 1 个分项：本视图那一项门限读数）。
///
/// 分项取「本视图那一项」而非三项之和：另两个视图的读数字段
/// 在册定上恒为 0（互斥占用），把它们也记进来只会把
/// 「48 项」变成「144 项」而分项值不变——
/// 账面看着更密，实际是同一笔账数了三遍。
/// 判据要的是**终账总额与分项之和零偏离**，
/// 一格一账最省也最实；三维的丰富性由
/// 「三维坐标 + 三种视图」在 `SuiteRow` 上体现，不靠重复记账体现。
pub const PART_TARGET: u32 = CART_WRITABLE as u32;
/// 加法链分项下限（阻断「分项 0 + 总额 0」的平凡守恒）。
pub const PART_FLOOR: u64 = 3;
/// 条位映射记账目标（十一条位各记一条映射）。
pub const SLOT_MAP_TARGET: u32 = SLOT_COUNT;
/// 1GiB 档册定范围格数（`2 件 × 2 视图`，无欠账核销）。
pub const TIER_CELL_G1: u32 = 4;
/// 4GiB 档册定范围格数（`2 件 × 3 视图`）。
pub const TIER_CELL_FULL: u32 = 6;

/// 范围盐（方案侧重算用；随 seed 变化，保证换种子换轨迹）。
pub const SCOPE_SALT: u64 = 0x3081_0000_0000_0001;
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
pub const EXPORT_VERSION: u64 = 0x0000308100000001;
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
// 3. 错误闭集（26 个变体，一变体一码段，恒 ≤ 60）
// ===========================================================================

/// 错误闭集：每变体独占一个诊断码，段内连续。
///
/// 三要素：
///   * `code() -> u16`：段内唯一诊断码（`SEG_BASE + 序号`），一变体一码一段
///   * `detail(k) -> String`：上下文细节（件号 / 列号 / 位次 / tick 等）
///   * 变体本身即第三要素：闭集可穷举（`ErrKind::ALL`），任何新错误必须先入册
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrKind {
    /// `档位闭集破损：三档（1GB/4GB/16GB）不齐或越域`。
    TierClosedSetBroken,
    /// `档位序号非法：非 0..2 闭集内的值`。
    TierIndexInvalid,
    /// `件套闭集破损：六件套不齐或超出 6 件`。
    SuiteClosedSetBroken,
    /// `件套序号非法：非 0..5 闭集内的值`。
    SuiteIndexInvalid,
    /// `校验视图闭集破损：规模/时长/欠账核销三视图不齐`。
    ViewClosedSetBroken,
    /// `校验视图序号非法：非 0..2 闭集内的值`。
    ViewIndexInvalid,
    /// `条位闭集破损：F3082–F3092 十一条位不齐`。
    SlotClosedSetBroken,
    /// `条位序号非法：非 0..10 闭集内的值`。
    SlotIndexInvalid,
    /// `条位映射缺项：某条位在册定映射表中查无所属件`。
    SlotMapMissing,
    /// `条位映射重复：同一条位被映射到两件以上`。
    SlotMapDuplicated,
    /// `件套无条位：某件套在映射表中零条位覆盖`。
    SlotMapOrphan,
    /// `件套覆盖不足：六件中某件零登记格`。
    SuiteCoverageShort,
    /// `档位偏聚：全部登记格挤在同一档（三档退化成一档）`。
    TierSkewRejected,
    /// `范围格缺失：某（档 × 件 × 视图）组合无定义件`。
    SuiteCellMissing,
    /// `范围格重复：同一（档 × 件 × 视图）登记两条`。
    SuiteCellRepeated,
    /// `范围维度越域：三维坐标越出各自闭集`。
    SuiteDimMismatch,
    /// `分界注记缺项：范围面/执行面四要素未齐`。
    BoundaryNoteMissing,
    /// `分界越界：档位执行职责被划入本单范围面`。
    BoundaryScopeLeak,
    /// `范围冻结哈希漂移：同 tick 重算六件套范围不一致`。
    ScopeHashDrift,
    /// `加法链不守恒：Σ分项 ≠ 总额（零偏离要求）`。
    AddChainUnbalanced,
    /// `阶段不允许登记范围格：仅 Draft 可写`。
    StageRejectRegister,
    /// `阶段不允许范围冻结：六件未登记齐即冻结被拒`。
    StageRejectFreeze,
    /// `封后写：范围已冻结后再改六件套被拒`。
    WriteAfterFrozen,
    /// `冻结哈希漂移：同 tick 重算不一致`。
    FreezeHashDrift,
    /// `状态机非法迁移：跨段 / 自环 / 回退边`。
    StageIllegalEdge,
    /// `逻辑 tick 回退：时间不得倒退`。
    TickRegress,}

impl ErrKind {
    /// 全部变体（判据顺序，闭集穷举的权威列表）。
    pub const ALL: [ErrKind; 26] = [
    ErrKind::TierClosedSetBroken,
    ErrKind::TierIndexInvalid,
    ErrKind::SuiteClosedSetBroken,
    ErrKind::SuiteIndexInvalid,
    ErrKind::ViewClosedSetBroken,
    ErrKind::ViewIndexInvalid,
    ErrKind::SlotClosedSetBroken,
    ErrKind::SlotIndexInvalid,
    ErrKind::SlotMapMissing,
    ErrKind::SlotMapDuplicated,
    ErrKind::SlotMapOrphan,
    ErrKind::SuiteCoverageShort,
    ErrKind::TierSkewRejected,
    ErrKind::SuiteCellMissing,
    ErrKind::SuiteCellRepeated,
    ErrKind::SuiteDimMismatch,
    ErrKind::BoundaryNoteMissing,
    ErrKind::BoundaryScopeLeak,
    ErrKind::ScopeHashDrift,
    ErrKind::AddChainUnbalanced,
    ErrKind::StageRejectRegister,
    ErrKind::StageRejectFreeze,
    ErrKind::WriteAfterFrozen,
    ErrKind::FreezeHashDrift,
    ErrKind::StageIllegalEdge,
    ErrKind::TickRegress,    ];

    /// 段内序号（0..25）。
    pub fn index(self) -> usize {
        match self {
            ErrKind::TierClosedSetBroken => 0,
            ErrKind::TierIndexInvalid => 1,
            ErrKind::SuiteClosedSetBroken => 2,
            ErrKind::SuiteIndexInvalid => 3,
            ErrKind::ViewClosedSetBroken => 4,
            ErrKind::ViewIndexInvalid => 5,
            ErrKind::SlotClosedSetBroken => 6,
            ErrKind::SlotIndexInvalid => 7,
            ErrKind::SlotMapMissing => 8,
            ErrKind::SlotMapDuplicated => 9,
            ErrKind::SlotMapOrphan => 10,
            ErrKind::SuiteCoverageShort => 11,
            ErrKind::TierSkewRejected => 12,
            ErrKind::SuiteCellMissing => 13,
            ErrKind::SuiteCellRepeated => 14,
            ErrKind::SuiteDimMismatch => 15,
            ErrKind::BoundaryNoteMissing => 16,
            ErrKind::BoundaryScopeLeak => 17,
            ErrKind::ScopeHashDrift => 18,
            ErrKind::AddChainUnbalanced => 19,
            ErrKind::StageRejectRegister => 20,
            ErrKind::StageRejectFreeze => 21,
            ErrKind::WriteAfterFrozen => 22,
            ErrKind::FreezeHashDrift => 23,
            ErrKind::StageIllegalEdge => 24,
            ErrKind::TickRegress => 25,        }
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
            ErrKind::TierIndexInvalid => "TierIndexInvalid",
            ErrKind::SuiteClosedSetBroken => "SuiteClosedSetBroken",
            ErrKind::SuiteIndexInvalid => "SuiteIndexInvalid",
            ErrKind::ViewClosedSetBroken => "ViewClosedSetBroken",
            ErrKind::ViewIndexInvalid => "ViewIndexInvalid",
            ErrKind::SlotClosedSetBroken => "SlotClosedSetBroken",
            ErrKind::SlotIndexInvalid => "SlotIndexInvalid",
            ErrKind::SlotMapMissing => "SlotMapMissing",
            ErrKind::SlotMapDuplicated => "SlotMapDuplicated",
            ErrKind::SlotMapOrphan => "SlotMapOrphan",
            ErrKind::SuiteCoverageShort => "SuiteCoverageShort",
            ErrKind::TierSkewRejected => "TierSkewRejected",
            ErrKind::SuiteCellMissing => "SuiteCellMissing",
            ErrKind::SuiteCellRepeated => "SuiteCellRepeated",
            ErrKind::SuiteDimMismatch => "SuiteDimMismatch",
            ErrKind::BoundaryNoteMissing => "BoundaryNoteMissing",
            ErrKind::BoundaryScopeLeak => "BoundaryScopeLeak",
            ErrKind::ScopeHashDrift => "ScopeHashDrift",
            ErrKind::AddChainUnbalanced => "AddChainUnbalanced",
            ErrKind::StageRejectRegister => "StageRejectRegister",
            ErrKind::StageRejectFreeze => "StageRejectFreeze",
            ErrKind::WriteAfterFrozen => "WriteAfterFrozen",
            ErrKind::FreezeHashDrift => "FreezeHashDrift",
            ErrKind::StageIllegalEdge => "StageIllegalEdge",
            ErrKind::TickRegress => "TickRegress",        }
    }

    /// 判读：现象 + 位置 + 建议修法（三要素之一，人读一句话）。
    pub fn describe(self) -> &'static str {
        match self {
            ErrKind::TierClosedSetBroken => "档位闭集破损：三档（1GB/4GB/16GB）不齐或越域；修法=按册定三档重建档表，缺一即范围冻结拦截",
            ErrKind::TierIndexInvalid => "档位序号非法：非 0..2 闭集内的值；修法=把档位收进三值闭集，非法值构造期即拒",
            ErrKind::SuiteClosedSetBroken => "件套闭集破损：六件套不齐或超出 6 件；修法=按册定六件套重建件表，缺一即范围冻结拦截",
            ErrKind::SuiteIndexInvalid => "件套序号非法：非 0..5 闭集内的值；修法=把件套收进六值闭集，非法值构造期即拒",
            ErrKind::ViewClosedSetBroken => "校验视图闭集破损：规模/时长/欠账核销三视图不齐；修法=按册定三视图重建视图表",
            ErrKind::ViewIndexInvalid => "校验视图序号非法：非 0..2 闭集内的值；修法=把视图收进三值闭集",
            ErrKind::SlotClosedSetBroken => "条位闭集破损：F3082–F3092 十一条位不齐；修法=按册定十一条位重建条位表，缺一即映射断言拦截",
            ErrKind::SlotIndexInvalid => "条位序号非法：非 0..10 闭集内的值；修法=把条位收进十一值闭集",
            ErrKind::SlotMapMissing => "条位映射缺项：某条位在册定映射表中查无所属件；修法=补齐该条位的件套归属，一条位一件不许悬空",
            ErrKind::SlotMapDuplicated => "条位映射重复：同一条位被映射到两件以上；修法=一条位只许归一件，重复映射即删掉多余归属",
            ErrKind::SlotMapOrphan => "件套无条位：某件套在映射表中零条位覆盖；修法=补齐该件套的条位清单，六件全覆盖不许留空件",
            ErrKind::SuiteCoverageShort => "件套覆盖不足：六件中某件零登记格；修法=按件套覆盖断言补齐该件的三视图登记",
            ErrKind::TierSkewRejected => "档位偏聚：全部登记格挤在同一档（三档退化成一档）；修法=把登记格摊回三档，每档 16 格",
            ErrKind::SuiteCellMissing => "范围格缺失：某（档 × 件 × 视图）组合无定义件；修法=补齐该格的范围定义，逐格登记入册",
            ErrKind::SuiteCellRepeated => "范围格重复：同一（档 × 件 × 视图）登记两条；修法=一格一条定义，重复登记即拒",
            ErrKind::SuiteDimMismatch => "范围维度越域：三维坐标越出各自闭集；修法=核对档位/件套/视图三维的取值域",
            ErrKind::BoundaryNoteMissing => "分界注记缺项：范围面/执行面四要素未齐；修法=补齐分界注记的归属/归他/判例/衔接四列",
            ErrKind::BoundaryScopeLeak => "分界越界：档位执行职责被划入本单范围面；修法=把执行职责归还 F3082–F3092 条位，本单只冻范围不落执行",
            ErrKind::ScopeHashDrift => "范围冻结哈希漂移：同 tick 重算六件套范围不一致；修法=按 freeze_tick 重算范围冻结哈希",
            ErrKind::AddChainUnbalanced => "加法链不守恒：Σ分项 ≠ 总额（零偏离要求）；修法=重算分项与总额，检查是否有漏登记",
            ErrKind::StageRejectRegister => "阶段不允许登记范围格：仅 Draft 可写；修法=把登记动作调度回 Draft 阶段",
            ErrKind::StageRejectFreeze => "阶段不允许范围冻结：六件未登记齐即冻结被拒；修法=先补齐六件套全部范围格再冻结",
            ErrKind::WriteAfterFrozen => "封后写：范围已冻结后再改六件套被拒；修法=终账方案变更另开版本号方案",
            ErrKind::FreezeHashDrift => "冻结哈希漂移：同 tick 重算不一致；修法=按 freeze_tick 重算冻结哈希",
            ErrKind::StageIllegalEdge => "状态机非法迁移：跨段 / 自环 / 回退边；修法=只走 +1 合法边，其余一律拒记",
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
            ErrKind::TierClosedSetBroken => "按册定三档重建档表，缺一即范围冻结拦截",
            ErrKind::TierIndexInvalid => "把档位收进三值闭集，非法值构造期即拒",
            ErrKind::SuiteClosedSetBroken => "按册定六件套重建件表，缺一即范围冻结拦截",
            ErrKind::SuiteIndexInvalid => "把件套收进六值闭集，非法值构造期即拒",
            ErrKind::ViewClosedSetBroken => "按册定三视图重建视图表",
            ErrKind::ViewIndexInvalid => "把视图收进三值闭集",
            ErrKind::SlotClosedSetBroken => "按册定十一条位重建条位表，缺一即映射断言拦截",
            ErrKind::SlotIndexInvalid => "把条位收进十一值闭集",
            ErrKind::SlotMapMissing => "补齐该条位的件套归属，一条位一件不许悬空",
            ErrKind::SlotMapDuplicated => "一条位只许归一件，重复映射即删掉多余归属",
            ErrKind::SlotMapOrphan => "补齐该件套的条位清单，六件全覆盖不许留空件",
            ErrKind::SuiteCoverageShort => "按件套覆盖断言补齐该件的三视图登记",
            ErrKind::TierSkewRejected => "把登记格摊回三档，每档 16 格",
            ErrKind::SuiteCellMissing => "补齐该格的范围定义，逐格登记入册",
            ErrKind::SuiteCellRepeated => "一格一条定义，重复登记即拒",
            ErrKind::SuiteDimMismatch => "核对档位/件套/视图三维的取值域",
            ErrKind::BoundaryNoteMissing => "补齐分界注记的归属/归他/判例/衔接四列",
            ErrKind::BoundaryScopeLeak => "把执行职责归还 F3082–F3092 条位，本单只冻范围不落执行",
            ErrKind::ScopeHashDrift => "按 freeze_tick 重算范围冻结哈希",
            ErrKind::AddChainUnbalanced => "重算分项与总额，检查是否有漏登记",
            ErrKind::StageRejectRegister => "把登记动作调度回 Draft 阶段",
            ErrKind::StageRejectFreeze => "先补齐六件套全部范围格再冻结",
            ErrKind::WriteAfterFrozen => "终账方案变更另开版本号方案",
            ErrKind::FreezeHashDrift => "按 freeze_tick 重算冻结哈希",
            ErrKind::StageIllegalEdge => "只走 +1 合法边，其余一律拒记",
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
        ErrKind::ALL.len() == 26,
        format!("ALL.len()={} want={}", ErrKind::ALL.len(), 26),
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
            Stage::Draft => "范围草案",
            Stage::Mapped => "四十八格已登记",
            Stage::Frozen => "范围已冻结",
            Stage::Dispatched => "范围已调度",
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
pub type ScRes<T> = Res<T>;

// ===========================================================================
// 4. 领域闭集一：终账档位（1GiB / 4GiB / 16GiB 三档）
// ===========================================================================

/// 档位闭集序号上界（**开**）：档位闭集为 `0..3` 共三档。
pub const TIER_MAX: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 1GiB 档。
    G1,
    /// 4GiB 档。
    G4,
    /// 16GiB 档。
    G16,
}

impl Tier {
    /// 档位闭集（三值）。
    pub const ALL: [Tier; 3] = [Tier::G1, Tier::G4, Tier::G16];
    /// 序号（0/1/2）。
    pub fn index(self) -> u8 {
        match self {
            Tier::G1 => TIER_G1,
            Tier::G4 => TIER_G4,
            Tier::G16 => TIER_G16,
        }
    }
    /// 由序号还原（越界返回 `None`）。
    pub fn from_index(i: u8) -> Option<Tier> {
        match i {
            TIER_G1 => Some(Tier::G1),
            TIER_G4 => Some(Tier::G4),
            TIER_G16 => Some(Tier::G16),
            _ => None,
        }
    }
    /// 名称（短）。
    pub fn name(self) -> &'static str {
        match self {
            Tier::G1 => "g1",
            Tier::G4 => "g4",
            Tier::G16 => "g16",
        }
    }
    /// 报告面标签（带容量单位，禁裸 GB 数字）。
    pub fn label(self) -> &'static str {
        match self {
            Tier::G1 => "1GiB",
            Tier::G4 => "4GiB",
            Tier::G16 => "16GiB",
        }
    }
    /// 标称容量（MiB，整数）。
    pub fn mib(self) -> u32 {
        match self {
            Tier::G1 => TIER_MIB_G1,
            Tier::G4 => TIER_MIB_G4,
            Tier::G16 => TIER_MIB_G16,
        }
    }
    /// 该档适用的校验视图数（1GiB 档不欠账，故两视图）。
    pub fn view_count(self) -> u32 {
        match self {
            Tier::G1 => VIEWS_G1,
            _ => VIEWS_FULL,
        }
    }
    /// 该档的件套下界（件套按档顺序排：1GiB 两件、4GiB 两件、16GiB 两件）。
    pub fn suite_lo(self) -> u8 {
        match self {
            Tier::G1 => 0,
            Tier::G4 => 2,
            Tier::G16 => 4,
        }
    }
    /// 该档的件套数（每档两件：执行件 + 断言件）。
    pub fn suite_count(self) -> u32 {
        2
    }
    /// 该档的册定范围格数（`视图数 × 件数`）。
    pub fn cell_target(self) -> u32 {
        self.view_count() * self.suite_count()
    }
    /// 该档是否在闭集内。
    pub fn valid(self) -> bool {
        Tier::from_index(self.index()).is_some()
    }
    /// 该档是否拥有该件套（档位-件套三维**不是自由的**）。
    pub fn owns_suite(self, s: u8) -> bool {
        s >= self.suite_lo() && s < self.suite_lo() + 2
    }
    /// 该档内第 `k` 件（`k ∈ 0..2`）的件套序号。
    pub fn suite_at(self, k: u32) -> u8 {
        self.suite_lo().wrapping_add(k as u8)
    }
}

// ===========================================================================
// 5. 领域闭集二：六件套（三档 × 执行件/断言件）
// ===========================================================================

/// 六件套件号闭集（`0..SUITE_COUNT`，共 6 件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Suite {
    /// 件 0：1GiB 执行件。
    G1Exec,
    /// 件 1：1GiB 断言件。
    G1Assert,
    /// 件 2：4GiB 执行件。
    G4Exec,
    /// 件 3：4GiB 断言件。
    G4Assert,
    /// 件 4：16GiB 执行件。
    G16Exec,
    /// 件 5：16GiB 断言件。
    G16Assert,
}

impl Suite {
    /// 六件套闭集（六值）。
    pub const ALL: [Suite; 6] = [
        Suite::G1Exec,
        Suite::G1Assert,
        Suite::G4Exec,
        Suite::G4Assert,
        Suite::G16Exec,
        Suite::G16Assert,
    ];
    /// 序号（0..5）。
    pub fn index(self) -> u8 {
        match self {
            Suite::G1Exec => 0,
            Suite::G1Assert => 1,
            Suite::G4Exec => 2,
            Suite::G4Assert => 3,
            Suite::G16Exec => 4,
            Suite::G16Assert => 5,
        }
    }
    /// 由序号还原（越界返回 `None`）。
    pub fn from_index(i: u8) -> Option<Suite> {
        match i {
            0 => Some(Suite::G1Exec),
            1 => Some(Suite::G1Assert),
            2 => Some(Suite::G4Exec),
            3 => Some(Suite::G4Assert),
            4 => Some(Suite::G16Exec),
            5 => Some(Suite::G16Assert),
            _ => None,
        }
    }
    /// 名称（短）。
    pub fn name(self) -> &'static str {
        match self {
            Suite::G1Exec => "g1_exec",
            Suite::G1Assert => "g1_assert",
            Suite::G4Exec => "g4_exec",
            Suite::G4Assert => "g4_assert",
            Suite::G16Exec => "g16_exec",
            Suite::G16Assert => "g16_assert",
        }
    }
    /// 所属档位。
    pub fn tier(self) -> Tier {
        match self {
            Suite::G1Exec => Tier::G1,
            Suite::G1Assert => Tier::G1,
            Suite::G4Exec => Tier::G4,
            Suite::G4Assert => Tier::G4,
            Suite::G16Exec => Tier::G16,
            Suite::G16Assert => Tier::G16,
        }
    }
    /// 职能（0 = 执行件，1 = 断言件）。
    pub fn duty(self) -> u8 {
        match self {
            Suite::G1Exec => 0,
            Suite::G4Exec => 0,
            Suite::G16Exec => 0,
            Suite::G1Assert => 1,
            Suite::G4Assert => 1,
            Suite::G16Assert => 1,
        }
    }
    /// 是否执行件。
    pub fn is_exec(self) -> bool {
        self.duty() == 0
    }
    /// 报告面标签。
    pub fn label(self) -> &'static str {
        match self {
            Suite::G1Exec => "1GiB-执行",
            Suite::G1Assert => "1GiB-断言",
            Suite::G4Exec => "4GiB-执行",
            Suite::G4Assert => "4GiB-断言",
            Suite::G16Exec => "16GiB-执行",
            Suite::G16Assert => "16GiB-断言",
        }
    }
    /// 该件是否在闭集内。
    pub fn valid(self) -> bool {
        Suite::from_index(self.index()).is_some()
    }
}

// ===========================================================================
// 6. 领域闭集三：校验视图（规模门限 / 时长门限 / 欠账核销）
// ===========================================================================

/// 校验视图闭集（三值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    /// 规模门限：读数不得越容量档（千分位）。
    Scale,
    /// 时长门限：72h 长稳账面读数（逻辑刻千分位）。
    Duration,
    /// 欠账核销：欠账终核读数（账分）。
    Debt,
}

impl View {
    /// 视图闭集（三值）。
    pub const ALL: [View; 3] = [View::Scale, View::Duration, View::Debt];
    /// 序号（0/1/2）。
    pub fn index(self) -> u8 {
        match self {
            View::Scale => VIEW_SCALE,
            View::Duration => VIEW_DURATION,
            View::Debt => VIEW_DEBT,
        }
    }
    /// 由序号还原（越界返回 `None`）。
    pub fn from_index(i: u8) -> Option<View> {
        match i {
            VIEW_SCALE => Some(View::Scale),
            VIEW_DURATION => Some(View::Duration),
            VIEW_DEBT => Some(View::Debt),
            _ => None,
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            View::Scale => "scale_gate",
            View::Duration => "duration_gate",
            View::Debt => "debt_clear",
        }
    }
    /// 该视图在给定档位上是否册定存在（1GiB 档不欠账）。
    pub fn applies(self, t: Tier) -> bool {
        match self {
            View::Debt => t.view_count() == VIEWS_FULL,
            _ => true,
        }
    }
    /// 该视图是否在闭集内。
    pub fn valid(self) -> bool {
        View::from_index(self.index()).is_some()
    }
}

// ===========================================================================
// 7. 领域闭集四：条位（F3082–F3092 十一条位）
// ===========================================================================

/// 条位闭集序号上界（**开**）：条位闭集为 `0..11` 共 11 条。
pub const SLOT_MAX: u8 = 10;

/// 一条位（判据号 `F308x` ↔ 册位号 `308x`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotRow {
    /// 册位序号（0..10）。
    pub index: u8,
    /// 判据号（`SLOT_NO_LO + index`）。
    pub task_no: u32,
    /// 归属件套序号（`0..SUITE_COUNT`）。
    pub suite: u8,
    /// 该条位承担的映射职责（0 = 范围冻结 / 1 = 门限账 / 2 = 欠账核销）。
    pub role: u8,
}

impl SlotRow {
    /// 单条位有效性（四项全查）。
    pub fn valid(&self) -> bool {
        if self.index > SLOT_MAX {
            return false;
        }
        if self.task_no != SLOT_NO_LO + self.index as u32 {
            return false;
        }
        if Suite::from_index(self.suite).is_none() {
            return false;
        }
        if self.role > 2 {
            return false;
        }
        true
    }

    /// 职责名。
    pub fn role_name(&self) -> &'static str {
        match self.role {
            0 => "scope_freeze",
            1 => "gate_ledger",
            2 => "debt_settle",
            _ => "?",
        }
    }

    /// 单条位报告行。
    pub fn row(&self) -> String {
        let sn = match Suite::from_index(self.suite) {
            Some(s) => s.label(),
            None => "?",
        };
        format!(
            "SLOT F{} index={} suite={} role={} valid={}",
            self.task_no,
            self.index,
            sn,
            self.role_name(),
            self.valid()
        )
    }

    /// 单条位指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix32(h, self.index as u32);
        h = mix32(h, self.task_no);
        h = mix32(h, self.suite as u32);
        h = mix32(h, self.role as u32);
        h
    }
}

/// 册定条位归属表（11 项，件套序号；**唯一权威源**）。
///
/// 六件 < 十一条位，故映射是「多对一」的覆盖关系（一件管多条位），
/// 册定分配刻意做成 `3/2/2/2/1/1`：
///
///   * 件 0（1GiB 执行）：F3082 / F3083 / F3084 —— 三条位
///   * 件 1（1GiB 断言）：F3085 / F3086 —— 两条位
///   * 件 2（4GiB 执行）：F3087 / F3088 —— 两条位
///   * 件 3（4GiB 断言）：F3089 / F3090 —— 两条位
///   * 件 4（16GiB 执行）：F3091 —— 一条位
///   * 件 5（16GiB 断言）：F3092 —— 一条位
///
/// 分配不均是**有意**的：档位越大、条位越少（16GiB 只剩一条），
/// 因为大档的执行与断言逻辑已在前两档被穷举过，
/// 再加条位只是重复覆盖同一批读数。
/// 若强行摊成 `2/2/2/2/2/1`，账面看着匀，
/// 却暗示「16GiB 也只被覆盖了一半」——那是假的均匀。
pub const SLOT_SUITE_OF: [u8; 11] = [0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 5];

/// 构建条位映射表（`SLOT_COUNT` = 11 行）。
pub fn slot_map_table() -> Vec<SlotRow> {
    let mut v: Vec<SlotRow> = Vec::new();
    let mut i = 0usize;
    while i < SLOT_COUNT as usize {
        v.push(SlotRow {
            index: i as u8,
            task_no: SLOT_NO_LO + i as u32,
            suite: slot_to_suite(i as u8).unwrap_or(SUITE_COUNT as u8),
            role: slot_to_role(i as u8),
        });
        i += 1;
    }
    v
}

/// 条位→件套映射（**纯函数**，册定分配的唯一权威源）。
///
/// 该函数与 `slot_map_table()` 是**同一份册定的两个视角**：
/// 表是可读报告面，函数是对拍面。
/// 双向对拍时两侧都调本函数，才叫对拍；
/// 一侧调表、一侧调函数，那是两份数据互相比对，恒不相等。
pub fn slot_to_suite(i: u8) -> Option<u8> {
    if i > SLOT_MAX {
        return None;
    }
    let j = i as usize;
    Some(SLOT_SUITE_OF[j])
}

/// 条位→职能映射（**纯函数**）。
///
/// 职责按 `i mod 3` 轮转：`0 → 范围冻结`、`1 → 门限账`、`2 → 欠账核销`。
/// 轮转而非固定分配，是为了让三条职责在十一条位上分布均衡
/// （`4/4/3`），避免「欠账核销只挂一条位、看着像漏了」。
pub fn slot_to_role(i: u8) -> u8 {
    if i > SLOT_MAX {
        return 3;
    }
    (i % 3) as u8
}

/// 条位映射完整性断言（三查：十一条齐 / 一条位一件 / 六件全覆盖）。
///
/// 判据原文「六件与 F3082–F3092 条位一一映射断言过」。
/// 「一一映射」在这里必须落成**三条可查的恒等式**，
/// 否则「映射过了」只是一句口号：
///   1. 条位闭集齐备：十一条位一条不少（`SLOT_COUNT`）；
///   2. 单射性：任一条位只归一件，且实例表与册定函数逐位相等；
///   3. 满射性：任一件套至少一条位（不许出现零条位的空件）。
/// 只查 (1) 会漏掉「一条位归两件」的重复挂载；
/// 只查 (3) 会漏掉「某条位查无所属」的悬空条位。
pub fn slot_map_ok(v: &Vec<SlotRow>) -> ScRes<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    if v.len() != SLOT_COUNT as usize {
        return Err(ErrKind::SlotClosedSetBroken);
    }
    // 逐条位自身有效性
    let mut i = 0usize;
    while i < v.len() {
        if !v[i].valid() {
            return Err(ErrKind::SlotIndexInvalid);
        }
        i += 1;
    }
    // 查 1：条位闭集齐备（判据号两两互异，无重复挂载）
    let mut a = 0usize;
    while a < v.len() {
        let mut b = a + 1;
        while b < v.len() {
            if v[a].task_no == v[b].task_no {
                return Err(ErrKind::SlotMapDuplicated);
            }
            b += 1;
        }
        a += 1;
    }
    out.push(format!(
        "SLOTMAP rows={} expect={} closed=true",
        v.len(),
        SLOT_COUNT
    ));
    // 查 2：满射性——六件每件至少一条位
    //
    // **先判满射再判单射**（次序纪律）：
    // 把全部条位改挂到件 0 时，单射性先炸，报 `SlotMapDuplicated`——
    // 但那些条位并没有「重复挂载」，重复的是**它们本该分散这件事**；
    // 真正的缺陷是件 4 / 件 5 成了零条位空件。
    // 检出次序应当**先粗后细**：满射性问「件还在不在」，
    // 单射性问「挂得对不对」；件都没了还去纠挂载点是本末倒置。
    let mut cnt: [u32; 6] = [0u32; 6];
    i = 0;
    while i < v.len() {
        let s = v[i].suite as usize;
        if s < cnt.len() {
            cnt[s] = cnt[s].wrapping_add(1);
        } else {
            return Err(ErrKind::SuiteIndexInvalid);
        }
        i += 1;
    }
    let mut si = 0usize;
    while si < cnt.len() {
        out.push(format!(
            "  SLOTMAP suite={} slots={}",
            Suite::ALL[si].label(),
            cnt[si]
        ));
        si += 1;
    }
    let mut orphan = 0u32;
    si = 0;
    while si < cnt.len() {
        if cnt[si] == 0 {
            orphan += 1;
        }
        si += 1;
    }
    out.push(format!("SLOTMAP orphan={}", orphan));
    if orphan != 0 {
        return Err(ErrKind::SlotMapOrphan);
    }
    // 查 3：单射性——册定映射函数与实例表逐位相等。
    let mut bidir = true;
    i = 0;
    while i < v.len() {
        let want = match slot_to_suite(v[i].index) {
            Some(x) => x,
            None => {
                bidir = false;
                i += 1;
                continue;
            }
        };
        let want_role = slot_to_role(v[i].index);
        if v[i].suite != want || v[i].role != want_role {
            bidir = false;
        }
        i += 1;
    }
    out.push(format!("SLOTMAP bidir={}", bidir));
    if !bidir {
        return Err(ErrKind::SlotMapDuplicated);
    }
    Ok(out)
}

// ===========================================================================
// 8. 六件套范围格定义行（档位 × 件套 × 视图 三维一格）
// ===========================================================================

/// 范围单行（一格一条定义，三维坐标一格）。
#[derive(Clone, Copy, Debug)]
pub struct SuiteRow {
    /// 维度 A：档位序号（0..2）。
    pub tier: u8,
    /// 维度 B：件套序号（0..5）。
    pub suite: u8,
    /// 维度 C：校验视图序号（0..2）。
    pub view: u8,
    /// 规模门限读数（千分位；仅规模视图有值，其余视图为 0）。
    pub scale_gate: u32,
    /// 时长门限读数（72h 长稳逻辑刻千分位；仅时长视图有值）。
    pub duration_gate: u32,
    /// 欠账核销读数（账分；仅欠账视图有值）。
    pub debt_clear: u32,
    /// 方案侧册定规模门限（重算用）。
    pub plan_scale: u32,
    /// 方案侧册定时长门限（重算用）。
    pub plan_duration: u32,
    /// 方案侧册定欠账核销（重算用）。
    pub plan_debt: u32,
}

impl SuiteRow {
    /// 单行自身有效性（八项全查）。
    ///
    /// 有效性只查**本格自身**的闭集性，不掺跨格关系
    /// （跨格关系由 `suite_coverage_ok()` 与 `cartesian_assert()` 负责）。
    /// 混在一起的后果是：一格坏会让整张范围表全红，定位信息全丢。
    pub fn valid(&self) -> bool {
        if self.tier > TIER_MAX {
            return false;
        }
        let t = match Tier::from_index(self.tier) {
            Some(x) => x,
            None => return false,
        };
        if Suite::from_index(self.suite).is_none() {
            return false;
        }
        let v = match View::from_index(self.view) {
            Some(x) => x,
            None => return false,
        };
        // 件套必须归本档所有（档位-件套三维不是自由的）。
        if !t.owns_suite(self.suite) {
            return false;
        }
        // 视图必须在本档册定存在（1GiB 档不欠账）。
        if !v.applies(t) {
            return false;
        }
        // 读数域：三个视图各自一个上界，且**互斥占用**——
        // 规模视图只有 `scale_gate` 非零，另两个必须为 0。
        // 不互斥的话，「把时长读数误写进规模字段」这类错位就查不出。
        if self.scale_gate > SCALE_MAX_PER_MILLE {
            return false;
        }
        if self.duration_gate > TICKS_PER_HOUR {
            return false;
        }
        if self.debt_clear > DEBT_MAX {
            return false;
        }
        match v {
            View::Scale => {
                if self.scale_gate == 0 {
                    return false;
                }
                if self.duration_gate != 0 || self.debt_clear != 0 {
                    return false;
                }
            }
            View::Duration => {
                if self.duration_gate == 0 {
                    return false;
                }
                if self.scale_gate != 0 || self.debt_clear != 0 {
                    return false;
                }
            }
            View::Debt => {
                if self.debt_clear == 0 {
                    return false;
                }
                if self.scale_gate != 0 || self.duration_gate != 0 {
                    return false;
                }
            }
        }
        true
    }

    /// 单行摘要。
    pub fn row(&self) -> String {
        let tn = match Tier::from_index(self.tier) {
            Some(t) => t.label(),
            None => "?",
        };
        let sn = match Suite::from_index(self.suite) {
            Some(s) => s.label(),
            None => "?",
        };
        let vn = match View::from_index(self.view) {
            Some(v) => v.name(),
            None => "?",
        };
        format!(
            "SUITE tier={} suite={} view={} scale={} duration={} debt={} valid={}",
            tn,
            sn,
            vn,
            self.scale_gate,
            self.duration_gate,
            self.debt_clear,
            self.valid()
        )
    }

    /// 单行指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix32(h, self.tier as u32);
        h = mix32(h, self.suite as u32);
        h = mix32(h, self.view as u32);
        h = mix32(h, self.scale_gate);
        h = mix32(h, self.duration_gate);
        h = mix32(h, self.debt_clear);
        h = mix32(h, self.plan_scale);
        h = mix32(h, self.plan_duration);
        h = mix32(h, self.plan_debt);
        h
    }
}

// ===========================================================================
// 9. 六件套读数的方案侧重算规则（纯函数，双向对拍共享规则源）
// ===========================================================================

/// 规模门限的**方案侧**重算（纯函数，千分位）。
///
/// 册定规则：`门限 = 基数 + 梯度 × (件序 + 视图序)`。
/// 拆成「件序 + 视图序」而不是 `(件序 × 视图数 + 视图序)`，
/// 是为了让门限随档位**单调递增**：件 5（16GiB 断言）
/// 必须比件 0（1GiB 执行）宽裕，否则「大档门限反比小档紧」，
/// 终账的语义就倒了。
pub fn scale_gate_plan(s: Suite, v: View) -> u32 {
    SCALE_BASE_PER_MILLE
        .wrapping_add(SCALE_RAMP_STEP.wrapping_mul(s.index() as u32))
        .wrapping_add(v.index() as u32)
}

/// 时长门限的**方案侧**重算（纯函数，72h 长稳逻辑刻千分位）。
///
/// 册定规则：`门限 = 基础千分位 + 梯度 × (件序 + 视图序)`。
/// 基础取 `LONG_RUN_TICKS` 的千分位（`259`），
/// 即长稳账面允许跑到 72h 的百分之一处仍不报警，超过则欠账。
/// 取整千分位是为了禁浮点：`72h / 1000` 若按 0.259 算就引入浮点，
/// 那是制度红线。
pub fn duration_gate_plan(s: Suite, v: View) -> u32 {
    let base = LONG_RUN_TICKS / 1000;
    base.wrapping_add(SCALE_RAMP_STEP.wrapping_mul(s.index() as u32))
        .wrapping_add(v.index() as u32)
}

/// 欠账核销的**方案侧**重算（纯函数，账分）。
///
/// 册定规则：`核销 = 基数 + 梯度 × (件序 + 视图序)`。
/// 欠账随件序递增：大档吃紧时欠得多，核销额度也高。
pub fn debt_clear_plan(s: Suite, v: View) -> u32 {
    DEBT_BASE
        .wrapping_add(DEBT_RAMP_STEP.wrapping_mul(s.index() as u32))
        .wrapping_add(v.index() as u32)
}

/// 视图分派（纯函数）：把三个读数按视图**互斥**地填进行。
///
/// 「互斥」纪律在此处落地：非本视图的读数一律写 0，
/// 而不是「留着上一个视图的值」。留着旧值会让
/// `valid()` 的互斥检查永远通不过，或者更糟——
/// 让人误以为该格同时受两个视图约束，
/// 而账面上两个读数其实是同一笔的两次计数。
pub fn assign_view(r: &mut SuiteRow, v: View) {
    let s = match Suite::from_index(r.suite) {
        Some(x) => x,
        None => return,
    };
    match v {
        View::Scale => {
            r.scale_gate = scale_gate_plan(s, v);
            r.duration_gate = 0;
            r.debt_clear = 0;
            r.plan_scale = scale_gate_plan(s, v);
            r.plan_duration = 0;
            r.plan_debt = 0;
        }
        View::Duration => {
            r.scale_gate = 0;
            r.duration_gate = duration_gate_plan(s, v);
            r.debt_clear = 0;
            r.plan_scale = 0;
            r.plan_duration = duration_gate_plan(s, v);
            r.plan_debt = 0;
        }
        View::Debt => {
            r.scale_gate = 0;
            r.duration_gate = 0;
            r.debt_clear = debt_clear_plan(s, v);
            r.plan_scale = 0;
            r.plan_duration = 0;
            r.plan_debt = debt_clear_plan(s, v);
        }
    }
}

/// 该格读数的**唯一有效值**（取本视图那一项；供加法链分项记账）。
///
/// 「取本视图那一项」而不是「三项相加」：三项里两项恒为 0，
/// 相加与取一项数值相同，但相加会让读者以为每格记了三笔账，
/// 分项计数与实际记账笔数对不上，R3 必红。
pub fn gate_value(r: &SuiteRow) -> u64 {
    match View::from_index(r.view) {
        Some(View::Scale) => r.scale_gate as u64,
        Some(View::Duration) => r.duration_gate as u64,
        Some(View::Debt) => r.debt_clear as u64,
        None => 0,
    }
}

// ===========================================================================
// 10. 分界注记（本单范围面 / 下游执行面 四列）
// ===========================================================================

/// 分界注记的本单面（**归本单**：只冻范围，不落执行）。
pub const SCOPE_SELF: u8 = 0;
/// 分界注记的下游面（**归他单**：F3082–F3092 各条位执行）。
pub const SCOPE_DOWN: u8 = 1;
/// 分界注记面数（册定两面：本单范围面 / 下游执行面）。
pub const BOUNDARY_FACES: u32 = 2;
/// 分界注记四列要素数（归属 / 归他 / 判例 / 衔接）。
pub const BOUNDARY_COLS: u32 = 4;
/// 分界注记的判例号（判例一：六件套范围由本单冻结，执行归条位）。
pub const PRECASE_NO: u32 = 1;

/// 分界注记衔接点数（册定五个）。
pub const LINK_POINTS: u32 = 5;
/// 衔接点 1：F3082–F3084（1GiB 执行件）。
pub const LINK_G1_EXEC: u8 = 1;
/// 衔接点 2：F3085–F3086（1GiB 断言件）。
pub const LINK_G1_ASSERT: u8 = 2;
/// 衔接点 3：F3087–F3090（4GiB 执行与断言件）。
pub const LINK_G4: u8 = 3;
/// 衔接点 4：F3091–F3092（16GiB 执行与断言件）。
pub const LINK_G16: u8 = 4;
/// 衔接点 5：判例一（本单范围冻结的唯一凭据）。
pub const LINK_PRECASE: u8 = 5;

/// 分界注记单行（归属面 / 职责 / 归他面 / 判例 / 衔接 五要素）。
#[derive(Clone, Copy, Debug)]
pub struct BoundaryRow {
    /// 归属面（`SCOPE_SELF` = 本单范围面；`SCOPE_DOWN` = 下游执行面）。
    pub face: u8,
    /// 本面职责摘要（定长语义，用闭集序号表达）。
    pub duty: u8,
    /// 归他面（同一条注记的对面）。
    pub other_face: u8,
    /// 判例号（`PRECASE_NO` = 判例一）。
    pub precase: u32,
    /// 衔接点（`LINK_G1_EXEC` … `LINK_PRECASE` 之一）。
    pub link: u8,
}

impl BoundaryRow {
    /// 单行自身有效性（五要素全查）。
    pub fn valid(&self) -> bool {
        if self.face > SCOPE_DOWN {
            return false;
        }
        if self.other_face == self.face {
            return false;
        }
        if self.other_face > SCOPE_DOWN {
            return false;
        }
        if (self.duty as u32) >= BOUNDARY_COLS {
            return false;
        }
        if self.precase == 0 || self.precase > PRECASE_NO {
            return false;
        }
        if self.link == 0 || (self.link as u32) > LINK_POINTS {
            return false;
        }
        true
    }

    /// 职责名。
    pub fn duty_name(&self) -> &'static str {
        match self.duty {
            0 => "scope_hash_freeze",
            1 => "tier_exec_run",
            2 => "tier_assert_run",
            3 => "slot_map_cover",
            _ => "?",
        }
    }

    /// 衔接点名。
    pub fn link_name(&self) -> &'static str {
        match self.link {
            LINK_G1_EXEC => "F3082-F3084_g1_exec",
            LINK_G1_ASSERT => "F3085-F3086_g1_assert",
            LINK_G4 => "F3087-F3090_g4",
            LINK_G16 => "F3091-F3092_g16",
            LINK_PRECASE => "PRECASE_1",
            _ => "?",
        }
    }

    /// 单行摘要。
    pub fn row(&self) -> String {
        let f = match self.face {
            SCOPE_SELF => "self",
            SCOPE_DOWN => "down",
            _ => "?",
        };
        let o = match self.other_face {
            SCOPE_SELF => "self",
            SCOPE_DOWN => "down",
            _ => "?",
        };
        format!(
            "BOUNDARY face={} duty={} other={} precase={} link={} valid={}",
            f,
            self.duty_name(),
            o,
            self.precase,
            self.link_name(),
            self.valid()
        )
    }

    /// 单行指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix32(h, self.face as u32);
        h = mix32(h, self.duty as u32);
        h = mix32(h, self.other_face as u32);
        h = mix32(h, self.precase);
        h = mix32(h, self.link as u32);
        h
    }
}

/// 构建分界注记表（`BOUNDARY_COLS` = 4 行，两面各两行）。
///
/// 册定四行：
///   * 本单面行 0：范围哈希冻结（判例一）——六件套范围由本单冻结；
///   * 本单面行 1：条位映射覆盖——十一条位映射归本单账面；
///   * 下游面行 0：档位执行——三档执行读数归 F3082–F3090；
///   * 下游面行 1：档位断言——三档断言读数归 F3085–F3092。
///
/// 四行不是随意凑数：`BOUNDARY_COLS = 4` 是守恒项，
/// `boundary_note_ok()` 逐行查存在性，少一行即 `BoundaryNoteMissing`。
pub fn boundary_note_table() -> Vec<BoundaryRow> {
    let mut v: Vec<BoundaryRow> = Vec::new();
    v.push(BoundaryRow {
        face: SCOPE_SELF,
        duty: 0,
        other_face: SCOPE_DOWN,
        precase: PRECASE_NO,
        link: LINK_PRECASE,
    });
    v.push(BoundaryRow {
        face: SCOPE_SELF,
        duty: 3,
        other_face: SCOPE_DOWN,
        precase: PRECASE_NO,
        link: LINK_G16,
    });
    v.push(BoundaryRow {
        face: SCOPE_DOWN,
        duty: 1,
        other_face: SCOPE_SELF,
        precase: PRECASE_NO,
        link: LINK_G4,
    });
    v.push(BoundaryRow {
        face: SCOPE_DOWN,
        duty: 2,
        other_face: SCOPE_SELF,
        precase: PRECASE_NO,
        link: LINK_G1_EXEC,
    });
    v
}

/// 分界注记完整性断言（四行齐备 + 双向对置 + 执行职责不越界）。
pub fn boundary_note_ok() -> ScRes<Vec<String>> {
    let t = boundary_note_table();
    let mut v: Vec<String> = Vec::new();
    if t.len() != BOUNDARY_COLS as usize {
        return Err(ErrKind::BoundaryNoteMissing);
    }
    let mut i = 0usize;
    while i < t.len() {
        if !t[i].valid() {
            return Err(ErrKind::BoundaryNoteMissing);
        }
        v.push(format!("  {}", t[i].row()));
        i += 1;
    }
    // 双向对置：本单面每行都能在下游面找到对面，反之亦然。
    let mut fs = true;
    let mut fd = true;
    i = 0;
    while i < t.len() {
        let mut j = 0usize;
        let mut found = false;
        while j < t.len() {
            if t[j].face == t[i].other_face && t[j].precase == t[i].precase {
                found = true;
            }
            j += 1;
        }
        if !found {
            if t[i].face == SCOPE_SELF {
                fs = false;
            } else {
                fd = false;
            }
        }
        i += 1;
    }
    v.push(format!("BOUNDARY mutual self={} down={}", fs, fd));
    if !fs || !fd {
        return Err(ErrKind::BoundaryNoteMissing);
    }
    // 执行职责不越界：凡 `duty` 为档位执行 / 档位断言的行必须
    // 挂在下游面下，挂在本单范围面下即越界。
    //
    // 这是分界注记的**核心防线**：判例一把「范围由本单冻结、
    // 执行归 F3082–F3092 条位」写死，本单一旦吞进执行职责，
    // 十一条位的账面就失去了归属，档位执行无人负责——
    // 而终账最怕的正是「账面说自己执行过，实际没人跑」。
    let mut leak = 0u32;
    i = 0;
    while i < t.len() {
        if (t[i].duty == 1 || t[i].duty == 2) && t[i].face != SCOPE_DOWN {
            leak += 1;
        }
        i += 1;
    }
    v.push(format!("BOUNDARY leak={} ok={}", leak, leak == 0));
    if leak != 0 {
        return Err(ErrKind::BoundaryScopeLeak);
    }
    Ok(v)
}

/// 衔接点注记（`LINK_POINTS` 行）。
///
/// 衔接点写「不落实现」的部分也要在册：F3082–F3092 十一条位
/// 各自只负责**消费**本单的冻结范围，本单**只做范围冻结**，
/// 不替任何条位跑执行。不注明这一点，
/// 账面看不出「为何本单没有十一条位各自的实现体」。
pub fn link_note() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "  LINK 面={} 条位=F3082-F3084 件=1GiB-执行 关系=消费冻结范围 本单只冻范围不落执行",
        LINK_G1_EXEC
    ));
    v.push(format!(
        "  LINK 面={} 条位=F3085-F3086 件=1GiB-断言 关系=消费冻结范围 本单只冻范围不落执行",
        LINK_G1_ASSERT
    ));
    v.push(format!(
        "  LINK 面={} 条位=F3087-F3090 件=4GiB 关系=消费冻结范围 本单只冻范围不落执行",
        LINK_G4
    ));
    v.push(format!(
        "  LINK 面={} 条位=F3091-F3092 件=16GiB 关系=消费冻结范围 本单只冻范围不落执行",
        LINK_G16
    ));
    v.push(format!(
        "  LINK 面={} 关系=判例一 六件套范围由本单冻结 入册哈希为准",
        LINK_PRECASE
    ));
    v
}

/// 衔接点自洽断言（`link_note()` 行数 == `LINK_POINTS`，非空注记）。
pub fn link_note_ok() -> bool {
    let v = link_note();
    if v.len() != LINK_POINTS as usize {
        return false;
    }
    let mut i = 0usize;
    while i < v.len() {
        if v[i].len() < 16 {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// 11. 范围格键表（与 F3082–F3092 条位读数键一一对应）
// ===========================================================================

/// 范围格键：一格一条，供条位读数取用。
///
/// 判据读数栏要求「条位映射可查」。键的构成刻意与三维坐标同源
/// （`档位-件套-视图`），于是条位拿着键回来查范围表，
/// 能**直接反查**出三维坐标，不必另建一张键到坐标的映射表——
/// 多一张表就多一处可漂移。
pub fn suite_key(t: Tier, s: Suite, v: View) -> String {
    format!("{}-{}-{}", t.label(), s.name(), v.name())
}

/// 范围格键表（按登记序）。
pub fn suite_key_table(rows: &Vec<SuiteRow>) -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(rows.len());
    let mut i = 0usize;
    while i < rows.len() {
        let t = Tier::from_index(rows[i].tier).unwrap_or(Tier::G1);
        let s = Suite::from_index(rows[i].suite).unwrap_or(Suite::G1Exec);
        let vw = View::from_index(rows[i].view).unwrap_or(View::Scale);
        v.push(suite_key(t, s, vw));
        i += 1;
    }
    v
}

/// 范围覆盖断言（每档格数 == 册定 + 同件套内视图读数互异 + 偏聚拦截）。
///
/// 判据风险栏写「档位偏聚（全部范围格挤在一档）」。
/// 注意「全聚 16GiB 档」是**最坏**的偏聚方向：
/// 大档门限最宽，全聚大档等于终账从最松的那头开始，
/// 三档递进关系彻底消失。故本断言查两件事：
///   * 每档格数 == 册定值（1GiB 4 / 4GiB 6 / 16GiB 6），零缺位；
///   * 同件套内各视图读数互不相同
///     （否则「三视图」实为同一读数测三遍）。
/// 少查任一条，偏聚都能从后门溜过去。
pub fn suite_coverage_ok(rows: &Vec<SuiteRow>) -> ScRes<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    if rows.len() != SUITE_REG_TARGET as usize {
        return Err(ErrKind::SuiteClosedSetBroken);
    }
    let mut ti = 0usize;
    let mut all_ok = true;
    while ti < Tier::ALL.len() {
        let t = Tier::ALL[ti];
        let mut cnt = 0u32;
        let mut gates: Vec<u32> = Vec::new();
        let mut si = 0usize;
        while si < Suite::ALL.len() {
            if Suite::ALL[si].tier().index() != t.index() {
                si += 1;
                continue;
            }
            let mut vi = 0usize;
            while vi < View::ALL.len() {
                if View::ALL[vi].applies(t) {
                    let mut ri = 0usize;
                    while ri < rows.len() {
                        let r = rows[ri];
                        if r.tier == t.index()
                            && r.suite == Suite::ALL[si].index()
                            && r.view == View::ALL[vi].index()
                        {
                            cnt += 1;
                            gates.push(gate_value(&r) as u32);
                        }
                        ri += 1;
                    }
                }
                vi += 1;
            }
            si += 1;
        }
        // 同档内读数重复检测
        let mut dup = 0u32;
        let mut a = 0usize;
        while a < gates.len() {
            let mut b = a + 1;
            while b < gates.len() {
                if gates[a] == gates[b] {
                    dup += 1;
                }
                b += 1;
            }
            a += 1;
        }
        let ok = cnt == t.cell_target() && dup == 0;
        if !ok {
            all_ok = false;
        }
        // **每件至少一格**（满射性在本层的投影）。
        //
        // 只查「每档格数 == 目标」是不够的：把某件的格整体改判到
        // 同档另一件名下时，该档格数**分文未变**，读数也仍然互异，
        // 于是「档级格数 + 档内读数互异」两条全过——
        // 而被抹掉的那件已成零格空件，覆盖不足就这么漏过去了。
        // 教训：**聚合量守恒不等于被聚合的对象还在**。
        // 档级断言只能保证「总数对」，要保证「每件都在」必须逐件点名。
        let mut zero_suite = 0u32;
        let mut zi = 0usize;
        while zi < Suite::ALL.len() {
            // 只数**本档拥有**的件：他档的件在本档必然零格，
            // 那是闭集设计（1GiB 档不挂 16GiB 件），不是覆盖缺口。
            // 不加这道过滤，1GiB 档会因为「件 2..5 一格没有」而被误判，
            // 册定基线反而被判坏——比漏检更糟：防线自己先红。
            if Suite::ALL[zi].tier().index() != t.index() {
                zi += 1;
                continue;
            }
            let mut c = 0u32;
            let mut ri = 0usize;
            while ri < rows.len() {
                if rows[ri].tier == t.index() && rows[ri].suite == Suite::ALL[zi].index() {
                    c += 1;
                }
                ri += 1;
            }
            if c == 0 {
                zero_suite += 1;
            }
            zi += 1;
        }
        let ok = ok && zero_suite == 0;
        if !ok {
            all_ok = false;
        }
        v.push(format!(
            "  SUITE-COV {} cells={} target={} gate_dup={} zero_suite={} ok={}",
            t.label(),
            cnt,
            t.cell_target(),
            dup,
            zero_suite,
            ok
        ));
        ti += 1;
    }
    v.push(format!("SUITE-COV all_ok={}", all_ok));
    if !all_ok {
        // 先判偏聚（全部格挤在一档）再判覆盖不足：
        // 偏聚是覆盖不足的**特例**，报更具体的那个码，
        // 注入核的形态与错误码才对得上。
        let mut only = 0u32;
        let mut ti2 = 0usize;
        while ti2 < Tier::ALL.len() {
            let t2 = Tier::ALL[ti2];
            let mut c2 = 0u32;
            let mut j = 0usize;
            while j < rows.len() {
                if rows[j].tier == t2.index() {
                    c2 += 1;
                }
                j += 1;
            }
            if c2 == SUITE_REG_TARGET {
                only = 1;
            }
            ti2 += 1;
        }
        if only == 1 {
            return Err(ErrKind::TierSkewRejected);
        }
        return Err(ErrKind::SuiteCoverageShort);
    }
    Ok(v)
}

// ===========================================================================
// 12. 六件套读数的方案侧折盐夹取（纯函数，双向对拍共享规则源）
// ===========================================================================

/// 规模门限的**方案侧**折盐夹取（纯函数）。
///
/// 夹取域是闭集 `[1, SCALE_MAX_PER_MILLE]`：
/// 折盐后 `raw % SCALE_MAX_PER_MILLE` 会把 `SCALE_MAX_PER_MILLE` 折成 0，
/// 而 0 不在合法域（`valid()` 判 0 为非法），
/// 故需用 `(base - 1 + salt) mod MAX + 1` 的**单射**写法。
/// 教训同 F3061 断电延迟夹取：
/// **取模把 `[1, MAX]` 映到 `[0, MAX]`，边界值必然漏出域外。**
pub fn scale_plan_side(base: u32, salt: u64, _cell: u64) -> u32 {
    let sh = (salt & 0x0F) as u32;
    (base.wrapping_sub(1).wrapping_add(sh) % SCALE_MAX_PER_MILLE) + 1
}

/// 时长门限的**方案侧**折盐夹取（纯函数）。
///
/// 折盐幅度刻意比规模门限小（`& 0x07` vs `& 0x0F`）：
/// 时长门限是**单调累加量**，扰动过大则三档递进关系被噪声淹没，
/// 「同件三视图读数互异」这条断言就变成掷骰子。
pub fn duration_plan_side(base: u32, salt: u64, _cell: u64) -> u32 {
    let sh = (salt & 0x07) as u32;
    (base.wrapping_sub(1).wrapping_add(sh) % TICKS_PER_HOUR) + 1
}

/// 欠账核销的**方案侧**折盐夹取（纯函数）。
///
/// 折盐幅度取 `& 0x03`：欠账值域窄（`1..=64`），
/// 扰动再大就会让大档的核销额度反而小于小档，终账语义倒挂。
pub fn debt_plan_side(base: u32, salt: u64, _cell: u64) -> u32 {
    let sh = (salt & 0x03) as u32;
    (base.wrapping_sub(1).wrapping_add(sh) % DEBT_MAX) + 1
}

// ===========================================================================
// 13. 六件套范围件（四十八格定义 + 登记位 + 覆盖账 + 键表）
// ===========================================================================

/// 范围件：四十八格定义 + 登记位 + 覆盖账 + 键表。
///
/// 与 F3061 的 `RoundPiece` 同构（登记只改行表、齐备后统一派生），
/// 但派生量从「相位覆盖账」换成「档位覆盖账 + 条位映射账」：
/// 三维范围表的联合派生态是**档位覆盖**与**条位覆盖**（跨行关系），
/// 而非逐行可判的本地不变量。
/// 故 `seal_scope()` 必须等 48 格齐备才可跑。
pub struct SuitePiece {
    /// 四十八格定义（未登记的格是占位空行，`valid()` 假）。
    pub rows: Vec<SuiteRow>,
    /// 登记位。
    pub reg: Vec<bool>,
    /// 档位覆盖快照（`seal_scope` 产出的 4 行）。
    pub cover: Vec<String>,
    /// 走查过的范围格数。
    pub cells_walked: u32,
    /// 范围格键表。
    pub keys: Vec<String>,
    /// 实例盐（折盐夹取用；随 seed 变化，保证换种子换轨迹）。
    pub salt: u64,
}

impl SuitePiece {
    /// 新建（全占位）。
    pub fn new(salt: u64) -> SuitePiece {
        let mut rows: Vec<SuiteRow> = Vec::new();
        let mut reg: Vec<bool> = Vec::new();
        let mut t = 0u8;
        while t <= TIER_MAX {
            let mut k = 0u32;
            while k < Tier::from_index(t).unwrap_or(Tier::G1).suite_count() {
                let s = Tier::from_index(t).unwrap_or(Tier::G1).suite_at(k);
                let mut vi = 0usize;
                while vi < View::ALL.len() {
                    let v = View::ALL[vi];
                    if v.applies(Tier::from_index(t).unwrap_or(Tier::G1)) {
                        rows.push(SuiteRow {
                            tier: t,
                            suite: s,
                            view: v.index(),
                            scale_gate: 0,
                            duration_gate: 0,
                            debt_clear: 0,
                            plan_scale: 0,
                            plan_duration: 0,
                            plan_debt: 0,
                        });
                        reg.push(false);
                    }
                    vi += 1;
                }
                k += 1;
            }
            t += 1;
        }
        SuitePiece {
            rows,
            reg,
            cover: Vec::new(),
            cells_walked: 0,
            keys: Vec::new(),
            salt,
        }
    }

    /// 定位格下标（`None` = 册定上不存在该格）。
    ///
    /// 册定上「不存在」的格有两种：
    ///   * 件套不属于该档（`tier × suite` 越界）；
    ///   * 视图在该档不适用（1GiB 档的欠账核销视图）。
    /// 二者都必须返回 `None` 而不是「造一格占位」——
    /// 造了占位，笛卡尔矩阵的 `writable` 判定就永远为真，
    /// 「册定不存在的格不接受注入」这条纪律随之失效。
    pub fn index_of(&self, t: u8, s: u8, v: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].tier == t && self.rows[i].suite == s && self.rows[i].view == v {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记一格三维定义（坐标由册定闭集取，读数按册定规则 + 折盐）。
    pub fn register(&mut self, t: u8, s: u8, v: u8) -> ScRes<u64> {
        if t > TIER_MAX {
            return Err(ErrKind::TierIndexInvalid);
        }
        let tr = match Tier::from_index(t) {
            Some(x) => x,
            None => return Err(ErrKind::TierClosedSetBroken),
        };
        let sr = match Suite::from_index(s) {
            Some(x) => x,
            None => return Err(ErrKind::SuiteIndexInvalid),
        };
        let vr = match View::from_index(v) {
            Some(x) => x,
            None => return Err(ErrKind::ViewIndexInvalid),
        };
        if !tr.owns_suite(s) {
            return Err(ErrKind::SuiteDimMismatch);
        }
        if !vr.applies(tr) {
            return Err(ErrKind::ViewClosedSetBroken);
        }
        let idx = match self.index_of(t, s, v) {
            Some(x) => x,
            None => return Err(ErrKind::SuiteCellMissing),
        };
        // 方案侧册定值（重算用；实现侧本就该与之逐项相等）。
        let plan_scale = scale_gate_plan(sr, vr);
        let plan_duration = duration_gate_plan(sr, vr);
        let plan_debt = debt_clear_plan(sr, vr);
        // 折盐夹取：视图决定哪个读数被真正占用，另两个恒 0。
        let gate = match vr {
            View::Scale => scale_plan_side(plan_scale, self.salt, idx as u64),
            View::Duration => duration_plan_side(plan_duration, self.salt, idx as u64),
            View::Debt => debt_plan_side(plan_debt, self.salt, idx as u64),
        };
        let mut row = SuiteRow {
            tier: t,
            suite: s,
            view: v,
            scale_gate: 0,
            duration_gate: 0,
            debt_clear: 0,
            plan_scale,
            plan_duration,
            plan_debt,
        };
        assign_view(&mut row, vr);
        // 折盐值落到本视图那一项（互斥纪律：另两项保持 0）。
        match vr {
            View::Scale => row.scale_gate = gate,
            View::Duration => row.duration_gate = gate,
            View::Debt => row.debt_clear = gate,
        }
        if !row.valid() {
            return Err(ErrKind::SuiteDimMismatch);
        }
        let fp = row.fingerprint();
        self.rows[idx] = row;
        self.reg[idx] = true;
        // **不在此处重建覆盖账**。
        // 覆盖断言是**跨 48 格**的联合性质：登记到第 1 格时，
        // 其余四十七格仍是占位空行（读数全 0），此刻断言必然
        // `SuiteCoverageShort` —— 于是登记第一格就被自己设的死锁拦住，
        // 四十八格永远登记不齐。分工：登记只改行表，
        // 覆盖派生由调用方在齐备后统一触发（见 `seal_scope()`）。
        Ok(fp)
    }

    /// 册定存在格的全登记（四十八格，按 `档 × 件 × 视图` 顺序）。
    pub fn register_all(&mut self) -> ScRes<u32> {
        let mut n = 0u32;
        let mut t = 0u8;
        while t <= TIER_MAX {
            let tr = Tier::from_index(t).unwrap_or(Tier::G1);
            let mut k = 0u32;
            while k < tr.suite_count() {
                let s = tr.suite_at(k);
                let mut vi = 0usize;
                while vi < View::ALL.len() {
                    let v = View::ALL[vi];
                    if v.applies(tr) {
                        self.register(t, s, v.index())?;
                        n += 1;
                    }
                    vi += 1;
                }
                k += 1;
            }
            t += 1;
        }
        Ok(n)
    }

    /// 四十八格齐备后派生档位覆盖账 + 范围格键表（公开入口，与登记解耦）。
    pub fn seal_scope(&mut self) -> ScRes<u32> {
        if !self.complete() {
            return Err(ErrKind::SuiteClosedSetBroken);
        }
        let cov = suite_coverage_ok(&self.rows)?;
        self.cover = cov;
        self.keys = suite_key_table(&self.rows);
        // 逐格走查：每格自身不变量 + 三维坐标与闭集一致。
        let mut walk = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.reg[i] {
                return Err(ErrKind::SuiteCellMissing);
            }
            if !self.rows[i].valid() {
                return Err(ErrKind::SuiteCellMissing);
            }
            let t = match Tier::from_index(self.rows[i].tier) {
                Some(x) => x,
                None => return Err(ErrKind::TierClosedSetBroken),
            };
            let s = match Suite::from_index(self.rows[i].suite) {
                Some(x) => x,
                None => return Err(ErrKind::SuiteClosedSetBroken),
            };
            let v = match View::from_index(self.rows[i].view) {
                Some(x) => x,
                None => return Err(ErrKind::ViewClosedSetBroken),
            };
            if !t.owns_suite(self.rows[i].suite) {
                return Err(ErrKind::SuiteDimMismatch);
            }
            if !v.applies(t) {
                return Err(ErrKind::SuiteDimMismatch);
            }
            if s.tier().index() != self.rows[i].tier {
                return Err(ErrKind::SuiteDimMismatch);
            }
            walk += 1;
            i += 1;
        }
        self.cells_walked = walk;
        Ok(walk)
    }

    /// 登记是否齐备（四十八格全登记）。
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

    /// 运行期查一格定义（读行表，不重算）。
    pub fn row_of(&self, t: u8, s: u8, v: u8) -> ScRes<SuiteRow> {
        if t > TIER_MAX {
            return Err(ErrKind::TierIndexInvalid);
        }
        let i = match self.index_of(t, s, v) {
            Some(x) => x,
            None => return Err(ErrKind::SuiteCellMissing),
        };
        if !self.reg[i] {
            return Err(ErrKind::SuiteCellMissing);
        }
        if !self.rows[i].valid() {
            return Err(ErrKind::SuiteDimMismatch);
        }
        Ok(self.rows[i])
    }

    /// 档位-件套-视图三维坐标一致性（读回行表，不重算）。
    pub fn coord_consistent(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.reg[i] {
                ok = false;
                i += 1;
                continue;
            }
            let t = match Tier::from_index(self.rows[i].tier) {
                Some(x) => x,
                None => {
                    ok = false;
                    i += 1;
                    continue;
                }
            };
            let s = match Suite::from_index(self.rows[i].suite) {
                Some(x) => x,
                None => {
                    ok = false;
                    i += 1;
                    continue;
                }
            };
            let v = match View::from_index(self.rows[i].view) {
                Some(x) => x,
                None => {
                    ok = false;
                    i += 1;
                    continue;
                }
            };
            if !t.owns_suite(self.rows[i].suite) {
                ok = false;
            }
            if !v.applies(t) {
                ok = false;
            }
            if s.tier().index() != self.rows[i].tier {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 方案侧 vs 实现侧**逐格**对拍（三维坐标 + 三个读数 + 册定值）。
    ///
    /// 双向纪律：两侧共用同一枚「盐 + 格序」作为共享输入，
    /// 方案侧独立复现全部变换（含折盐夹取），
    /// 实现侧从已填好的行表读回。少对一项都算「只对了一半」。
    pub fn rows_bidir_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.reg[i] {
                continue;
            }
            let t = match Tier::from_index(self.rows[i].tier) {
                Some(x) => x,
                None => {
                    ok = false;
                    i += 1;
                    continue;
                }
            };
            let s = match Suite::from_index(self.rows[i].suite) {
                Some(x) => x,
                None => {
                    ok = false;
                    i += 1;
                    continue;
                }
            };
            let v = match View::from_index(self.rows[i].view) {
                Some(x) => x,
                None => {
                    ok = false;
                    i += 1;
                    continue;
                }
            };
            if !t.owns_suite(self.rows[i].suite) {
                ok = false;
            }
            if !v.applies(t) {
                ok = false;
            }
            // 册定值对拍必须**逐视图**比，不能三项都比：
            // `assign_view()` 按互斥纪律把非本视图的册定值清 0，
            // 于是本视图那一项等于 `*_gate_plan(s, v)`，另两项恒 0。
            // 若写成「三项各自都等于 `*_gate_plan(s, v)`」，
            // 等于要求非本视图的册定值非 0，与互斥纪律直接冲突——
            // 两条纪律同时为真时，这一格必然红，且红得毫无道理。
            let want_ps = if v == View::Scale { scale_gate_plan(s, v) } else { 0 };
            let want_pd = if v == View::Duration { duration_gate_plan(s, v) } else { 0 };
            let want_pde = if v == View::Debt { debt_clear_plan(s, v) } else { 0 };
            if self.rows[i].plan_scale != want_ps {
                ok = false;
            }
            if self.rows[i].plan_duration != want_pd {
                ok = false;
            }
            if self.rows[i].plan_debt != want_pde {
                ok = false;
            }
            // 实现侧读回的本视图读数必须等于「册定值折盐夹取」的结果。
            let want = match v {
                View::Scale => scale_plan_side(scale_gate_plan(s, v), self.salt, i as u64),
                View::Duration => duration_plan_side(duration_gate_plan(s, v), self.salt, i as u64),
                View::Debt => debt_plan_side(debt_clear_plan(s, v), self.salt, i as u64),
            };
            if gate_value(&self.rows[i]) != want as u64 {
                ok = false;
            }
            // 互斥纪律：另两个读数必须恒 0。
            match v {
                View::Scale => {
                    if self.rows[i].duration_gate != 0 || self.rows[i].debt_clear != 0 {
                        ok = false;
                    }
                }
                View::Duration => {
                    if self.rows[i].scale_gate != 0 || self.rows[i].debt_clear != 0 {
                        ok = false;
                    }
                }
                View::Debt => {
                    if self.rows[i].scale_gate != 0 || self.rows[i].duration_gate != 0 {
                        ok = false;
                    }
                }
            }
            i += 1;
        }
        ok
    }

    /// 方案侧全范围指纹（**独立重算**，不读行表）。
    ///
    /// 双向对拍的另一道关：把四十八格的册定坐标 + 读数从闭集重新算一遍，
    /// 与 `rows_fingerprint()`（读回行表）对拍。
    /// 两侧共用「盐 + 格序」，但一个从闭集算、一个从表读，
    /// 源不同才叫对拍；都从表读就是自我循环。
    pub fn plan_side_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix64(h, self.salt);
        // 格序必须与 `SuitePiece::new()` 的登记顺序完全一致，
        // 否则两侧的「格序」不是同一件事，对拍恒不相等。
        let mut t = 0u8;
        let mut ord = 0u64;
        while t <= TIER_MAX {
            let tr = Tier::from_index(t).unwrap_or(Tier::G1);
            let mut k = 0u32;
            while k < tr.suite_count() {
                let s = tr.suite_at(k);
                let mut vi = 0usize;
                while vi < View::ALL.len() {
                    let v = View::ALL[vi];
                    if v.applies(tr) {
                        let sr = Suite::from_index(s).unwrap_or(Suite::G1Exec);
                        let gate = match v {
                            View::Scale => {
                                scale_plan_side(scale_gate_plan(sr, v), self.salt, ord)
                            }
                            View::Duration => {
                                duration_plan_side(duration_gate_plan(sr, v), self.salt, ord)
                            }
                            View::Debt => debt_plan_side(debt_clear_plan(sr, v), self.salt, ord),
                        };
                        // 对拍的**项数必须两侧一致**：实现侧折的是
                        // 「坐标三项 + 读数三项」，方案侧也必须折同样的六项。
                        // 只折「坐标三项 + 本视图那一项」是 tempting 的写法，
                        // 但两侧项数不等时哈希链长度不同，
                        // `plan != rows` 恒成立且与数据无关——
                        // 那不是发现了漂移，是两侧压根没在比同一串东西。
                        // 故方案侧按互斥纪律把 gate 摆进本视图那一槽，
                        // 另两槽补 0，与实现侧逐槽对齐。
                        h = mix32(h, t as u32);
                        h = mix32(h, s as u32);
                        h = mix32(h, v.index() as u32);
                        h = mix32(h, if v == View::Scale { gate } else { 0 });
                        h = mix32(h, if v == View::Duration { gate } else { 0 });
                        h = mix32(h, if v == View::Debt { gate } else { 0 });
                        ord = ord.wrapping_add(1);
                    }
                    vi += 1;
                }
                k += 1;
            }
            t += 1;
        }
        h
    }

    /// 实现侧全范围指纹（读回行表）。
    pub fn rows_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix64(h, self.salt);
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            h = mix32(h, r.tier as u32);
            h = mix32(h, r.suite as u32);
            h = mix32(h, r.view as u32);
            h = mix32(h, r.scale_gate);
            h = mix32(h, r.duration_gate);
            h = mix32(h, r.debt_clear);
            i += 1;
        }
        h
    }

    /// 范围件指纹（折登记位 + 行指纹 + 覆盖账 + 键表）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.reg[i] {
                h = mix64(h, self.rows[i].fingerprint());
            } else {
                h = mix32(h, 0xFFFF_FFFF);
            }
            i += 1;
        }
        h = mix32(h, self.cells_walked);
        let mut j = 0usize;
        while j < self.keys.len() {
            h = mix_str(h, &self.keys[j]);
            j += 1;
        }
        h
    }

    /// 范围件报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "RANGE cells={} walked={} keys={} cover_rows={} salt={:016X}",
            self.rows.len(),
            self.cells_walked,
            self.keys.len(),
            self.cover.len(),
            self.salt
        ));
        let mut i = 0usize;
        while i < self.rows.len() {
            v.push(format!("  {}", self.rows[i].row()));
            i += 1;
        }
        let mut j = 0usize;
        while j < self.cover.len() {
            v.push(format!("  {}", self.cover[j]));
            j += 1;
        }
        v
    }
}

// ===========================================================================
// 14. 正交维度笛卡尔矩阵（3 档 × 6 件 × 3 视图 = 54 格）
// ===========================================================================

/// 笛卡尔单格：维度 A（档位）× 维度 B（件套）× 维度 C（校验视图）。
///
/// 每格**独立夹具**：一格的失败绝不污染其余 53 格。若复用同一个
/// `SuitePiece` 实例逐格试，某格注入的坏行会被后 53 格继承，
/// 于是「54 格里只有 1 格坏」会退化成「54 格全红」，
/// 定位信息（到底是哪一维组合触发）全部丢失——这是矩阵测试最典型的自毁方式。
#[derive(Clone, Copy, Debug)]
pub struct CartCell {
    /// 维度 A：档位序号。
    pub a: u8,
    /// 维度 B：件套序号。
    pub b: u8,
    /// 维度 C：校验视图序号。
    pub c: u8,
    /// 本格注入的**故障形态号**（`0` = 本格未注入；`1..=4` = 四种形态之一）。
    ///
    /// 纪律：注入形态号必须与 `ok` **分开报**。若把「本格坏了」直接编码成
    /// `fault = 1`，报告面就分不清「注入了形态 1」与「本格判定为坏」。
    pub fault: u32,
    /// 本格范围件自身是否合规。
    pub ok: bool,
    /// 本格是否可写（册定存在的 (档, 件, 视图) 组合 × Draft 阶段）。
    pub writable: bool,
    /// 本格指纹。
    pub digest: u64,
}

impl CartCell {
    /// 单格报告行。
    pub fn row(&self) -> String {
        let tn = match Tier::from_index(self.a) {
            Some(t) => t.label(),
            None => "?",
        };
        let sn = match Suite::from_index(self.b) {
            Some(s) => s.label(),
            None => "?",
        };
        let vn = match View::from_index(self.c) {
            Some(v) => v.name(),
            None => "?",
        };
        format!(
            "CART tier={} suite={} view={} fault={} ok={} writable={} digest={:016X}",
            tn, sn, vn, self.fault, self.ok, self.writable, self.digest
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

/// 阶段是否允许写范围格（仅 Draft）。
pub fn stage_allows_cell(st: Stage) -> bool {
    st.index() == STAGE_DRAFT
}

/// 本格三维组合在册定上是否存在（维度 A × B × C 的交叉约束）。
///
/// 笛卡尔矩阵的三维**不是自由的**：
///   * 件套 `b` 必须属于档位 `a`（每档两件），故 `a × b` 只有 6 组而非 18；
///   * 视图 `c` 必须在该档册定存在（1GiB 档无欠账核销视图）。
/// 若把三维当独立自由维跑满 54 组，有 18 组是**册定上不存在**的组合，
/// 判它们「坏」没有意义，判它们「好」也是自欺。
/// 故本函数只对**册定存在**的组合返回真。
pub fn triple_exists(a: u8, b: u8, c: u8) -> bool {
    let t = match Tier::from_index(a) {
        Some(x) => x,
        None => return false,
    };
    if !t.owns_suite(b) {
        return false;
    }
    let v = match View::from_index(c) {
        Some(x) => x,
        None => return false,
    };
    v.applies(t)
}

/// 同档内的另一件（册定相邻件；断言件返回执行件，执行件返回断言件）。
///
/// 笛卡尔形态 1「读数撞车」需要与**同档的另一件**撞同一个读数。
/// 注意不能跨档撞：跨档撞出来的重复值，
/// `suite_coverage_ok()` 的重检是**逐档内**做的，跨档撞不出重，
/// 于是形态落到那些格上「注了但没坏」，注入核报 `injected_ok != inj`。
/// 教训同 F3061：注入点必须落在**被检出条件的作用域内**，
/// 「改了某个字段」不等于「该字段的约束被违反」。
pub fn sibling_in_tier(a: u8, b: u8) -> u8 {
    let t = match Tier::from_index(a) {
        Some(x) => x,
        None => return b,
    };
    let lo = t.suite_lo();
    if b == lo {
        lo.wrapping_add(1)
    } else {
        lo
    }
}

/// 同档内的另一视图（册定相邻视图；末视图退回首视图）。
pub fn sibling_view(t: Tier, c: u8) -> u8 {
    let mut i = 0usize;
    while i < View::ALL.len() {
        let v = View::ALL[i];
        if !v.applies(t) {
            i += 1;
            continue;
        }
        if v.index() != c {
            return v.index();
        }
        i += 1;
    }
    c
}

/// 跑笛卡尔矩阵（54 格，逐格独立夹具）。
///
/// 注入策略：对**册定存在**的 48 格按 `(a + b + c) % 4` 安排一种故障形态，
/// 四种形态分别是「坐标越域 / 读数撞车 / 互斥破坏 / 视图越域」，
/// 覆盖四条独立的注入口。册定不存在的 6 格不注入、`writable` 归零。
pub fn run_cartesian(salt: u64) -> Vec<CartCell> {
    let mut v: Vec<CartCell> = Vec::with_capacity(CART_EXPECTED);
    let mut a = 0usize;
    while a < CART_DA {
        let mut b = 0usize;
        while b < CART_DB {
            let mut c = 0usize;
            while c < CART_DC {
                let ti = a as u8;
                let si = b as u8;
                let vi = c as u8;
                let exists = triple_exists(ti, si, vi);
                // 可写格 = 册定存在 × 阶段允许写
                let writable = exists && stage_allows_cell(Stage::Draft);
                let shape = (a + b + c) % 4;

                // —— 本格独立夹具：现搭一份范围件，只跑本格 ——
                let mut pz = SuitePiece::new(salt);
                let mut setup_err: Option<ErrKind> = None;
                if pz.register_all().is_err() {
                    setup_err = Some(ErrKind::SuiteClosedSetBroken);
                }
                // `fault` 只记「注入了第几号形态」，与 `ok` 分开。
                let mut fault: u32 = 0u32;
                if writable && setup_err.is_none() {
                    fault = (shape as u32) + 1u32;
                    match pz.index_of(ti, si, vi) {
                        Some(idx) => match shape {
                            0 => {
                                // 坐标越域：把件套序号打成闭集外的值。
                                pz.rows[idx].suite = 9;
                            }
                            1 => {
                                // 读数撞车：与**同档另一件的同一视图**撞读数。
                                //
                                // 初稿写的是「跨件跨视图」，但
                                // `suite_coverage_ok()` 的重检是**逐档内**做的，
                                // 且撞车源格必须册定存在（否则 `index_of` 返回
                                // `None`，注入根本没落下），
                                // 于是「注了但没坏」，注入核报 `injected_bad != inj`。
                                // 教训：注入点必须落在**被检出条件的作用域内**，
                                // 「改了某个字段」不等于「该字段的约束被违反」。
                                let sib = sibling_in_tier(ti, si);
                                match pz.index_of(ti, sib, vi) {
                                    Some(sidx) => {
                                        pz.rows[idx].scale_gate = pz.rows[sidx].scale_gate;
                                        pz.rows[idx].duration_gate = pz.rows[sidx].duration_gate;
                                        pz.rows[idx].debt_clear = pz.rows[sidx].debt_clear;
                                    }
                                    None => {
                                        fault = 0;
                                    }
                                }
                            }
                            2 => {
                                // 互斥破坏：**往非本视图的槽里塞非 0**。
                                //
                                // 纪律同 shape 1：注入点必须落在
                                // 被检出条件的作用域内。
                                // `valid()` 的互斥检查是
                                // 「本视图那一项非 0 且另两项恒 0」，
                                // 于是往**本视图**那一项写值根本破坏不了互斥
                                // —— 那是合法赋值，`valid()` 照过不误。
                                // 真正能踩中互斥检查的只有
                                // 「让另两项之一变成非 0」。
                                match pz.rows[idx].view {
                                    0 => pz.rows[idx].duration_gate = 1,
                                    _ => pz.rows[idx].scale_gate = 1,
                                }
                            }
                            _ => {
                                // 视图越域：把视图序号打成闭集外的值。
                                pz.rows[idx].view = 9;
                            }
                        },
                        None => {
                            fault = 0;
                        }
                    }
                }

                // —— 本格判定：只看格自身不变量，不掺可写性 ——
                let mut ok = false;
                if exists && setup_err.is_none() {
                    let cell_ok = match pz.index_of(ti, si, vi) {
                        Some(x) => pz.rows[x].valid(),
                        None => false,
                    };
                    let cov_ok = suite_coverage_ok(&pz.rows).is_ok();
                    let pair_ok = triple_exists(ti, si, vi);
                    ok = cell_ok && cov_ok && pair_ok;
                }
                let digest = pz.fingerprint();
                v.push(CartCell {
                    a: ti,
                    b: si,
                    c: vi,
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
pub fn cartesian_assert(cells: &Vec<CartCell>) -> ScRes<String> {
    let (tot, ok, wr, bad) = cartesian_stats(cells);
    // 注入格数：每格自带注入形态号（0 = 未注入），逐格累加即注入数。
    // 未注入的可写格坏格数另计在高 16 位，避免与注入计数混淆。
    let mut inj = 0u32;
    let mut injected_bad = 0u32;
    let mut noinj_writable_bad = 0u32;
    let mut i = 0usize;
    while i < cells.len() {
        if cells[i].fault != 0 {
            inj += 1;
            if cells[i].ok == false {
                injected_bad += 1;
            }
        } else if cells[i].writable && cells[i].ok == false {
            // 未注入的可写格必须全绿。册定不存在的组合
            // `writable` 归零，其 `ok == false` 是**册定事实**而非缺陷，
            // 不该被算成「未注入的坏格」。
            noinj_writable_bad += 1;
        }
        i += 1;
    }
    let msg = format!(
        "CART_ASSERT cells={} expect={} ok={} writable={} expect_writable={} bad={} injected={} injected_rejected={} noinj_writable_bad={}",
        tot,
        CART_EXPECTED,
        ok,
        wr,
        CART_WRITABLE,
        bad,
        inj,
        injected_bad,
        noinj_writable_bad
    );
    if tot != CART_EXPECTED as u32 {
        return Err(ErrKind::SuiteDimMismatch);
    }
    if wr != CART_WRITABLE as u32 {
        return Err(ErrKind::StageRejectRegister);
    }
    if inj != CART_WRITABLE as u32 {
        return Err(ErrKind::StageRejectRegister);
    }
    if injected_bad != inj {
        return Err(ErrKind::ScopeHashDrift);
    }
    if noinj_writable_bad != 0 {
        return Err(ErrKind::ScopeHashDrift);
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
// 15. 范围冻结件（主状态机 + 四账 + 冻结指纹）
// ===========================================================================

/// 范围总件：范围件 + 条位映射 + 分界注记 + 四账 + 状态机 + 逻辑时钟。
pub struct Scheme {
    /// 六件套范围件。
    pub piece: SuitePiece,
    /// 条位映射表（`SLOT_COUNT` 行）。
    pub slots: Vec<SlotRow>,
    /// 分界注记表。
    pub boundary: Vec<BoundaryRow>,
    /// 加法链守恒账。
    pub ledger: ChainLedger,
    /// append-only 环形账。
    pub ring: RingLog,
    /// 非法迁移拒记环。
    pub rej: RejectionRing,
    /// 冻结 / 调度入册。
    pub explog: ExpLog,
    /// 主状态机阶段。
    pub stage: Stage,
    /// 逻辑时钟。
    pub tick: u64,
    /// 冻结哈希。
    pub freeze_hash: u64,
    /// 冻结时的 tick（`verify_freeze` 按它重算）。
    pub freeze_tick: u64,
    /// 状态机合法边走查步数。
    pub walk_steps: u32,
    /// 实例盐。
    pub salt: u64,
}

impl Scheme {
    /// 新建（Draft 态，全占位）。
    pub fn new(salt: u64) -> Scheme {
        Scheme {
            piece: SuitePiece::new(salt),
            slots: slot_map_table(),
            boundary: boundary_note_table(),
            ledger: ChainLedger::new(
                SUITE_REG_TARGET,
                SUITE_REG_TARGET,
                BOUNDARY_COLS,
                SLOT_MAP_TARGET as u64,
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
    pub fn stamp(&mut self, t: u64, digest: u64, tag: u32) -> ScRes<u64> {
        if t < self.tick {
            return Err(ErrKind::TickRegress);
        }
        Ok(self.ring.push(self.tick, digest, tag))
    }

    /// 登记一格范围定义（仅 Draft）。
    pub fn register_cell(&mut self, t: u8, s: u8, v: u8) -> ScRes<u64> {
        if self.stage != Stage::Draft {
            self.rej.push(self.tick, self.stage, Stage::Draft);
            return Err(ErrKind::StageRejectRegister);
        }
        let fp = self.piece.register(t, s, v)?;
        // 逐格登记：一格一记（只补记一次会漏记 N-1 件）。
        self.ledger.note_reg();
        // 加法链分项：终账的**可读数**按视图分派，一格记一账。
        // 注意：分项取的是「本视图那一项」（`gate_value`），
        // 而不是三项之和——另两项恒 0，相加与取一项数值相同，
        // 但会让分项计数与记账笔数对不上，R3 必红。
        if let Some(idx) = self.piece.index_of(t, s, v) {
            self.ledger.note_part(gate_value(&self.piece.rows[idx]));
        }
        self.tick_advance();
        self.ring.push(self.tick, fp, 0x381);
        Ok(fp)
    }

    /// 册定存在格的全登记（四十八格）。
    ///
    /// 顺序按 `档 × 件 × 视图` 展开，与 `SuitePiece::new()` 的行序严格一致——
    /// 格序一旦错位，`plan_side_fingerprint()`（按同一顺序重算）
    /// 与 `rows_fingerprint()`（按行表读回）的对拍会恒不相等。
    pub fn register_all_cells(&mut self) -> ScRes<u32> {
        let mut n = 0u32;
        let mut t = 0u8;
        while t <= TIER_MAX {
            let tr = Tier::from_index(t).unwrap_or(Tier::G1);
            let mut k = 0u32;
            while k < tr.suite_count() {
                let s = tr.suite_at(k);
                let mut vi = 0usize;
                while vi < View::ALL.len() {
                    let v = View::ALL[vi];
                    if v.applies(tr) {
                        self.register_cell(t, s, v.index())?;
                        n += 1;
                    }
                    vi += 1;
                }
                k += 1;
            }
            t += 1;
        }
        // 四十八格齐备后**统一**派生档位覆盖账
        // （登记期不派生，见 `SuitePiece::register`）。
        let _ = self.piece.seal_scope()?;
        Ok(n)
    }

    /// 状态机推进（合法边闭集仅 3 条；非法边一律拒记）。
    pub fn advance(&mut self, next: Stage) -> ScRes<Stage> {
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
    pub fn note_part(&mut self, v: u64) -> ScRes<()> {
        self.ledger.note_part(v);
        self.tick_advance();
        self.ring.push(self.tick, v, 0x382);
        Ok(())
    }

    /// 回填注入读数（注入核跑完后调用，保证守恒账有注入侧读数）。
    pub fn note_inject(&mut self, total: u32, rejected: u32) {
        self.ledger.note_inject(total, rejected);
        self.ring.push(self.tick, total as u64, 0x383);
    }

    /// 条位映射入册（十一条位，逐条登记）。
    pub fn note_slots(&mut self) -> ScRes<u32> {
        slot_map_ok(&self.slots)?;
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.slots.len() {
            if !self.slots[i].valid() {
                return Err(ErrKind::SlotIndexInvalid);
            }
            // 逐条位登记：一条位一记（映射覆盖量 +1）。
            self.ledger.note_mapped(1);
            self.ledger.note_cells(1);
            self.tick_advance();
            let fp = self.slots[i].fingerprint();
            self.ring.push(self.tick, fp, 0x384);
            n += 1;
            i += 1;
        }
        self.explog.record(self.tick, self.slot_fingerprint(), 0xB0);
        Ok(n)
    }

    /// 分界注记入册（四行，逐行登记）。
    pub fn note_boundary(&mut self) -> ScRes<u32> {
        boundary_note_ok()?;
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.boundary.len() {
            if !self.boundary[i].valid() {
                return Err(ErrKind::BoundaryNoteMissing);
            }
            // 逐行登记：两面各两行，面侧各记一次映射。
            self.ledger.note_mapped(1);
            self.ledger.note_cells(1);
            self.tick_advance();
            let fp = self.boundary[i].fingerprint();
            self.ring.push(self.tick, fp, 0x385);
            n += 1;
            i += 1;
        }
        self.explog.record(self.tick, self.boundary_fingerprint(), 0xB1);
        Ok(n)
    }

    /// 条位映射指纹。
    pub fn slot_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        let mut i = 0usize;
        while i < self.slots.len() {
            h = mix64(h, self.slots[i].fingerprint());
            i += 1;
        }
        h
    }

    /// 分界注记指纹。
    pub fn boundary_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        let mut i = 0usize;
        while i < self.boundary.len() {
            h = mix64(h, self.boundary[i].fingerprint());
            i += 1;
        }
        h = mix_str(h, &link_note().join("|"));
        h
    }

    /// 逐格走查（读范围行表，验证三维坐标与闭集一致）。
    pub fn walk_cells(&self) -> ScRes<u32> {
        let mut n = 0u32;
        let mut t = 0u8;
        while t <= TIER_MAX {
            let tr = Tier::from_index(t).unwrap_or(Tier::G1);
            let mut k = 0u32;
            while k < tr.suite_count() {
                let s = tr.suite_at(k);
                let mut vi = 0usize;
                while vi < View::ALL.len() {
                    let v = View::ALL[vi];
                    if v.applies(tr) {
                        let row = self.piece.row_of(t, s, v.index())?;
                        if !row.valid() {
                            return Err(ErrKind::SuiteDimMismatch);
                        }
                        n += 1;
                    }
                    vi += 1;
                }
                k += 1;
            }
            t += 1;
        }
        Ok(n)
    }

    /// 档位覆盖验证（读回行表 + 覆盖断言）。
    pub fn suite_verify(&self) -> ScRes<Vec<String>> {
        suite_coverage_ok(&self.piece.rows)
    }

    /// 双向对拍（方案侧重算 vs 实现侧读回，共用同一枚盐）。
    pub fn bidirectional_ok(&self) -> bool {
        self.piece.plan_side_fingerprint() == self.piece.rows_fingerprint()
    }

    /// 冻结哈希计算（**只折冻结后永不变的字段**）。
    ///
    /// 纪律：绝不把 `ring` / `rej` / `explog` / `ledger` 四本活账折进来。
    /// `dispatch()` 之后四本活账继续追加，一旦折入，
    /// `verify_freeze()` 在正常收官时必然误报漂移。
    /// 折入的只有：册定常量 + 实例盐 + 范围件（冻结后只读）
    /// + 条位映射 + 分界注记 + 冻结 tick。
    pub fn compute_freeze(&self, tick: u64) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix_str(h, CRITERION);
        h = mix32(h, TIER_COUNT);
        h = mix32(h, SUITE_COUNT);
        h = mix32(h, VIEW_COUNT);
        h = mix32(h, SLOT_COUNT);
        h = mix32(h, BOUNDARY_COLS);
        h = mix32(h, CART_WRITABLE as u32);
        h = mix64(h, self.piece.fingerprint());
        h = mix64(h, self.slot_fingerprint());
        h = mix64(h, self.boundary_fingerprint());
        h = mix64(h, self.salt);
        h = mix64(h, tick);
        h
    }

    /// 冻结校验。
    pub fn verify_freeze(&self) -> ScRes<u64> {
        if self.freeze_hash == 0 {
            return Err(ErrKind::FreezeHashDrift);
        }
        let h = self.compute_freeze(self.freeze_tick);
        if h != self.freeze_hash {
            return Err(ErrKind::FreezeHashDrift);
        }
        Ok(h)
    }

    /// 范围冻结入册。
    pub fn seal(&mut self) -> ScRes<u64> {
        if self.stage == Stage::Draft {
            return Err(ErrKind::StageRejectFreeze);
        }
        if !self.piece.complete() {
            return Err(ErrKind::StageRejectFreeze);
        }
        if !self.piece.coord_consistent() {
            return Err(ErrKind::SuiteDimMismatch);
        }
        if suite_coverage_ok(&self.piece.rows).is_err() {
            return Err(ErrKind::SuiteCoverageShort);
        }
        if !self.piece.rows_bidir_ok() {
            return Err(ErrKind::ScopeHashDrift);
        }
        if !self.bidirectional_ok() {
            return Err(ErrKind::ScopeHashDrift);
        }
        slot_map_ok(&self.slots)?;
        boundary_note_ok()?;
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

    /// 调度入册（冻结件供 F3082–F3092 十一条位共引）。
    pub fn dispatch(&mut self) -> ScRes<u64> {
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

    /// 封后写检测（在 Frozen / Dispatched 阶段改范围必须被拒）。
    pub fn register_after_frozen(&mut self, t: u8, s: u8, v: u8) -> ScRes<u64> {
        if self.stage == Stage::Frozen || self.stage == Stage::Dispatched {
            self.rej.push(self.tick, self.stage, self.stage);
            return Err(ErrKind::WriteAfterFrozen);
        }
        self.register_cell(t, s, v)
    }

    /// 方案总指纹（折活账，供 `observe` 逐种子比对）。
    pub fn scheme_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix32(h, self.stage.index() as u32);
        h = mix64(h, self.tick);
        h = mix64(h, self.freeze_hash);
        h = mix64(h, self.piece.fingerprint());
        h = mix64(h, self.slot_fingerprint());
        h = mix64(h, self.boundary_fingerprint());
        h = mix64(h, self.ledger.fingerprint());
        h = mix64(h, self.ring.fingerprint());
        h = mix64(h, self.rej.fingerprint());
        h = mix64(h, self.explog.fingerprint());
        h
    }

    /// 范围漂移指纹（只折冻结语义载荷，不折活账）。
    pub fn scope_drift_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.piece.fingerprint());
        h = mix64(h, self.slot_fingerprint());
        h = mix64(h, self.boundary_fingerprint());
        h = mix64(h, self.compute_freeze(self.freeze_tick));
        h
    }

    /// 方案报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "SCHEME stage={} tick={} cells={} walked={} slots={} freeze={:016X}",
            self.stage.name(),
            self.tick,
            SUITE_REG_TARGET,
            self.piece.cells_walked,
            self.slots.len(),
            self.freeze_hash
        ));
        let r = self.piece.report();
        let mut i = 0usize;
        while i < r.len() {
            v.push(r[i].clone());
            i += 1;
        }
        let mut j = 0usize;
        while j < self.slots.len() {
            v.push(format!("  {}", self.slots[j].row()));
            j += 1;
        }
        let mut k = 0usize;
        while k < self.boundary.len() {
            v.push(format!("  {}", self.boundary[k].row()));
            k += 1;
        }
        v.push(link_note().join(" ").replace('\n', " "));
        v.push(join_lines(&suite_key_table(&self.piece.rows)));
        v
    }
}

// ===========================================================================
// 16. 注入核（12 形态，一形态一错误码，全拒留痕）
// ===========================================================================

/// 注入形态闭集（12 种，覆盖本单 26 个错误变体中的 12 个可注入口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectForm {
    /// I0：档位序号越界 → `TierIndexInvalid`。
    F0TierIndex,
    /// I1：范围格未登记齐即冻结 → `SuiteClosedSetBroken`。
    F1CellMissing,
    /// I2：同一格登记两条 → `SuiteCellRepeated`。
    F2CellRepeated,
    /// I3：三维坐标越域 → `SuiteDimMismatch`。
    F3DimMismatch,
    /// I4：件套覆盖不足（某件零格）→ `SuiteCoverageShort`。
    F4SuiteShort,
    /// I5：档位偏聚（全部格挤一档）→ `TierSkewRejected`。
    F5TierSkew,
    /// I6：读数互斥破坏 → `SuiteDimMismatch` 的专用形态。
    F6GateMix,
    /// I7：条位映射重复挂载 → `SlotMapDuplicated`。
    F7SlotDup,
    /// I8：条位映射缺项（件套零条位）→ `SlotMapOrphan`。
    F8SlotOrphan,
    /// I9：封后写 → `WriteAfterFrozen`。
    F9WriteAfterFrozen,
    /// I10：状态机自环 → `StageIllegalEdge`。
    F10SelfLoop,
    /// I11：范围漂移 → `FreezeHashDrift`。
    F11ScopeDrift,
}

impl InjectForm {
    /// 全部注入形态。
    pub const ALL: [InjectForm; 12] = [
        InjectForm::F0TierIndex,
        InjectForm::F1CellMissing,
        InjectForm::F2CellRepeated,
        InjectForm::F3DimMismatch,
        InjectForm::F4SuiteShort,
        InjectForm::F5TierSkew,
        InjectForm::F6GateMix,
        InjectForm::F7SlotDup,
        InjectForm::F8SlotOrphan,
        InjectForm::F9WriteAfterFrozen,
        InjectForm::F10SelfLoop,
        InjectForm::F11ScopeDrift,
    ];

    /// 形态序号（0..11）。
    pub fn index(self) -> usize {
        match self {
            InjectForm::F0TierIndex => 0,
            InjectForm::F1CellMissing => 1,
            InjectForm::F2CellRepeated => 2,
            InjectForm::F3DimMismatch => 3,
            InjectForm::F4SuiteShort => 4,
            InjectForm::F5TierSkew => 5,
            InjectForm::F6GateMix => 6,
            InjectForm::F7SlotDup => 7,
            InjectForm::F8SlotOrphan => 8,
            InjectForm::F9WriteAfterFrozen => 9,
            InjectForm::F10SelfLoop => 10,
            InjectForm::F11ScopeDrift => 11,
        }
    }

    /// 预期错误变体（形态与错误一一对应，零空档）。
    pub fn expect(self) -> ErrKind {
        match self {
            InjectForm::F0TierIndex => ErrKind::TierIndexInvalid,
            InjectForm::F1CellMissing => ErrKind::SuiteClosedSetBroken,
            InjectForm::F2CellRepeated => ErrKind::SuiteCellRepeated,
            InjectForm::F3DimMismatch => ErrKind::SuiteDimMismatch,
            InjectForm::F4SuiteShort => ErrKind::SuiteCoverageShort,
            InjectForm::F5TierSkew => ErrKind::TierSkewRejected,
            InjectForm::F6GateMix => ErrKind::SuiteDimMismatch,
            InjectForm::F7SlotDup => ErrKind::SlotMapDuplicated,
            InjectForm::F8SlotOrphan => ErrKind::SlotMapOrphan,
            InjectForm::F9WriteAfterFrozen => ErrKind::WriteAfterFrozen,
            InjectForm::F10SelfLoop => ErrKind::StageIllegalEdge,
            InjectForm::F11ScopeDrift => ErrKind::FreezeHashDrift,
        }
    }

    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            InjectForm::F0TierIndex => "tier_index",
            InjectForm::F1CellMissing => "cell_missing",
            InjectForm::F2CellRepeated => "cell_repeated",
            InjectForm::F3DimMismatch => "dim_mismatch",
            InjectForm::F4SuiteShort => "suite_short",
            InjectForm::F5TierSkew => "tier_skew",
            InjectForm::F6GateMix => "gate_mix",
            InjectForm::F7SlotDup => "slot_dup",
            InjectForm::F8SlotOrphan => "slot_orphan",
            InjectForm::F9WriteAfterFrozen => "write_after_frozen",
            InjectForm::F10SelfLoop => "self_loop",
            InjectForm::F11ScopeDrift => "scope_drift",
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
    /// 测量工具留痕指纹（**独立 LCG**，与被测对象不共享状态）。
    pub tool_fp: u64,
    /// 诊断行。
    pub line: String,
}

/// 造一份**合法**基线夹具（注入必须在好基线上跑）。
fn probe_fixture(stage: Stage, salt: u64) -> Scheme {
    let mut s = Scheme::new(salt);
    let _ = s.register_all_cells();
    if stage != Stage::Draft {
        let _ = s.advance(Stage::Mapped);
        let _ = s.note_slots();
        let _ = s.note_boundary();
    }
    if stage == Stage::Frozen || stage == Stage::Dispatched {
        // `seal()` 只算冻结哈希并入册，**不负责推阶段**；
        // 阶段推进是 `advance()` 的职责（Mapped → Frozen 是一条合法边）。
        // 漏了这一步，夹具的 `stage` 停在 Mapped，
        // 于是 F9「封后写」实际落到 `register_cell()` 上，
        // 报出来的是 `StageRejectRegister` 而非 `WriteAfterFrozen`。
        let _ = s.seal();
        let _ = s.advance(Stage::Frozen);
    }
    if stage == Stage::Dispatched {
        let _ = s.dispatch();
    }
    s
}

/// 注入结果封装。
fn finish(f: InjectForm, e: ScRes<u64>, s: &Scheme, tool: &mut Lcg) -> InjectionResult {
    let want = f.expect();
    // 测量工具与被测对象**数据来源分离**：工具走独立 LCG，
    // 绝不复用 `s` 的任何状态，否则「工具的读数」与「被测对象的读数」
    // 同源，测出来的偏移恒为 0，注入核就成了自证。
    let tool_fp = tool.next_u64();
    match e {
        Err(got) => InjectionResult {
            form: f.index() as u8,
            form_name: f.name(),
            rejected: true,
            got: got.index(),
            want: want.index(),
            matched: got == want,
            probe_fp: s.scope_drift_fp(),
            tool_fp,
            line: err_line(got, f.index() as u64),
        },
        Ok(_) => InjectionResult {
            form: f.index() as u8,
            form_name: f.name(),
            rejected: false,
            got: usize::MAX,
            want: want.index(),
            matched: false,
            probe_fp: s.scope_drift_fp(),
            tool_fp,
            line: String::from("注入未被拒：防线失守"),
        },
    }
}

/// 注入核（跑 12 形态，`tool` 为独立测量 LCG）。
pub fn inject_core(tool: &mut Lcg) -> Vec<InjectionResult> {
    let mut v: Vec<InjectionResult> = Vec::with_capacity(FORM_TOTAL as usize);
    let mut i = 0usize;
    while i < InjectForm::ALL.len() {
        let f = InjectForm::ALL[i];
        let salt = tool.next_u64();
        let r = match f {
            InjectForm::F0TierIndex => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：档位序号越界（3 不在 `0..3` 闭集）。
                let e = s
                    .register_cell(TIER_COUNT as u8, 0, VIEW_SCALE)
                    .map(|v| v);
                finish(f, e, &s, tool)
            }
            InjectForm::F1CellMissing => {
                // 注入：只登记 47 格就派生覆盖 → 缺格。
                //
                // 不能在 `probe_fixture()` 的结果上「少登记」——它已经把四十八格
                // 登记齐了，而 `register_cell` 只写不删，少登记根本不会发生。
                // 故自建方案，只登记 47 格。
                let mut s = Scheme::new(salt);
                let mut t = 0u8;
                while t <= TIER_MAX {
                    let tr = Tier::from_index(t).unwrap_or(Tier::G1);
                    let mut k = 0u32;
                    while k < tr.suite_count() {
                        let si = tr.suite_at(k);
                        let mut vi = 0usize;
                        while vi < View::ALL.len() {
                            let v = View::ALL[vi];
                            if v.applies(tr) {
                                // 跳过最后一格（16GiB 断言件 × 欠账核销）。
                                if !(t == TIER_G16 && si == 5 && v.index() == VIEW_DEBT) {
                                    let _ = s.register_cell(t, si, v.index());
                                }
                            }
                            vi += 1;
                        }
                        k += 1;
                    }
                    t += 1;
                }
                let e = s.piece.seal_scope().map(|v| v as u64);
                finish(f, e, &s, tool)
            }
            InjectForm::F2CellRepeated => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：同一格登记第二条（**换一个坐标重写同一行下标**）。
                //
                // `register` 是**幂等覆盖写**，「一格两条」不会在行表里留两条，
                // 检出点只能在**方案侧 vs 实现侧对拍**：册定
                // `plan_side_fingerprint()` 按 `档 × 件 × 视图` 顺序重算，
                // 这里把 1GiB 执行件 × 规模视图那格改成 4GiB 执行件 × 规模视图，
                // 实现侧读回的坐标与方案侧重算的坐标必不相等。
                // 这正是双向对拍的价值：它抓的不是「写了几次」，
                // 而是「写进去的东西与册定还对不对得上」。
                let _ = s.register_cell(TIER_G1, 0, VIEW_SCALE);
                let _ = s.piece.register(TIER_G4, 2, VIEW_SCALE);
                if let Some(i0) = s.piece.index_of(TIER_G1, 0, VIEW_SCALE) {
                    let r0 = s.piece.rows[i0];
                    s.piece.rows[i0].suite = 2;
                    s.piece.rows[i0].scale_gate = r0.scale_gate;
                }
                let e = if s.piece.plan_side_fingerprint() == s.piece.rows_fingerprint() {
                    Ok(0u64)
                } else {
                    Err(ErrKind::SuiteCellRepeated)
                };
                finish(f, e, &s, tool)
            }
            InjectForm::F3DimMismatch => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：把某格件套坐标改成非本档所有（1GiB 档挂 16GiB 件）。
                if let Some(idx) = s.piece.index_of(TIER_G1, 0, VIEW_SCALE) {
                    s.piece.rows[idx].suite = 4;
                }
                let e = if s.piece.coord_consistent() {
                    Ok(0u64)
                } else {
                    Err(ErrKind::SuiteDimMismatch)
                };
                finish(f, e, &s, tool)
            }
            InjectForm::F4SuiteShort => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：把 16GiB 断言件的六格全改判到 16GiB 执行件
                // （断言件零格 → 覆盖不足，非偏聚）。
                let mut si = 0usize;
                while si < s.piece.rows.len() {
                    if s.piece.rows[si].suite == 5 {
                        s.piece.rows[si].suite = 4;
                    }
                    si += 1;
                }
                let e = suite_coverage_ok(&s.piece.rows).map(|_| 0u64);
                finish(f, e, &s, tool)
            }
            InjectForm::F5TierSkew => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：全部四十八格挤到 16GiB 档（判据点名的最坏偏聚）。
                let mut si = 0usize;
                while si < s.piece.rows.len() {
                    s.piece.rows[si].tier = TIER_G16;
                    si += 1;
                }
                let e = suite_coverage_ok(&s.piece.rows).map(|_| 0u64);
                finish(f, e, &s, tool)
            }
            InjectForm::F6GateMix => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：读数互斥破坏——在规模视图那格塞进时长读数。
                if let Some(idx) = s.piece.index_of(TIER_G4, 2, VIEW_SCALE) {
                    s.piece.rows[idx].duration_gate = 1;
                }
                let e = match s.piece.index_of(TIER_G4, 2, VIEW_SCALE) {
                    Some(idx) => {
                        if s.piece.rows[idx].valid() {
                            Ok(0u64)
                        } else {
                            Err(ErrKind::SuiteDimMismatch)
                        }
                    }
                    None => Ok(0u64),
                };
                finish(f, e, &s, tool)
            }
            InjectForm::F7SlotDup => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：把一条位的归属件改成另一件（与册定映射函数不符）。
                //
                // 取的必须是「册定值 ≠ 注入值」的那一行：
                // `SLOT_SUITE_OF[1] = 0`，若注入也写 0 就是**原地未改**，
                // 检出点自然全过——注入核却会因「没报错」而记一条未拒。
                // 纪律：注入值必须**确实与基线不同**，
                // 改之前先看一眼册定那张表，别凭直觉写个「看起来不同」的值。
                //
                // 改到件 2 而不是件 5：改到件 5 会让件 5 有两条位、
                // 件 2 仍有一条位（满射仍成立），
                // 恰好避开满射检查、只触发单射检查，
                // 形态名「重复挂载」与报出的 `SlotMapDuplicated` 才对得上。
                if s.slots.len() > 1 {
                    s.slots[1].suite = 2;
                }
                // 检出点查**实例表**：册定表 `slot_map_ok(slot_map_table())` 恒过
                // （它查的是重新构造的册定件），而注入改的是 `s.slots` 实例，
                // 两者是两份数据。
                let e = match slot_map_ok(&s.slots) {
                    Err(g) => Err(g),
                    Ok(_) => Ok(0u64),
                };
                finish(f, e, &s, tool)
            }
            InjectForm::F8SlotOrphan => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：把全部条位改挂到件 0，件 4 / 件 5 变成零条位空件。
                let mut si = 0usize;
                while si < s.slots.len() {
                    s.slots[si].suite = 0;
                    si += 1;
                }
                let e = match slot_map_ok(&s.slots) {
                    Err(g) => Err(g),
                    Ok(_) => Ok(0u64),
                };
                finish(f, e, &s, tool)
            }
            InjectForm::F9WriteAfterFrozen => {
                let mut s = probe_fixture(Stage::Frozen, salt);
                // 注入：冻结态再登记一格（封后写）。
                let e = s
                    .register_after_frozen(TIER_G1, 0, VIEW_SCALE)
                    .map(|v| v);
                finish(f, e, &s, tool)
            }
            InjectForm::F10SelfLoop => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：状态机自环（Draft → Draft）。
                let e = s.advance(Stage::Draft).map(|_| 0u64);
                finish(f, e, &s, tool)
            }
            InjectForm::F11ScopeDrift => {
                let mut s = probe_fixture(Stage::Frozen, salt);
                // 注入：冻结后偷改范围（改某格读数），
                // 按 `freeze_tick` 重算必与冻结哈希不同。
                if let Some(idx) = s.piece.index_of(TIER_G16, 4, VIEW_SCALE) {
                    s.piece.rows[idx].scale_gate =
                        s.piece.rows[idx].scale_gate.wrapping_add(1);
                }
                // 期望码取 `FreezeHashDrift`：范围漂移**在冻结合同上的表现**
                // 就是「同 tick 重算不一致」，报 `FreezeHashDrift` 才是准的。
                // 若把形态的期望码写成 `ScopeHashDrift`，等于要求
                // `verify_freeze()` 换另一个错码表达同一件事，
                // 那是「为迁就注入而扭曲主流程」的倒挂。
                let e = s.verify_freeze().map(|v| v);
                finish(f, e, &s, tool)
            }
        };
        v.push(r);
        i += 1;
    }
    v
}

/// 注入核 + 独立测量工具（被测对象与测量工具数据源分离）。
pub fn inject_core_with_tool(seed: u64) -> Vec<InjectionResult> {
    // 工具种子由 seed 派生但**加一枚独立盐**，与被测对象同 seed 的
    // LCG 序列不同源。少这一步，工具读数与被测读数会同步偏移，
    // 交叉对拍恒为 0。
    let mut tool = Lcg::new(seed ^ 0x9E37_79B9_7F4A_7C15);
    inject_core(&mut tool)
}

/// 注入漏网检测（**严判**：必须「被拒」且「码一致」同时成立）。
pub fn detect_miss(v: &Vec<InjectionResult>) -> u32 {
    let mut m = 0u32;
    let mut i = 0usize;
    while i < v.len() {
        if !v[i].rejected || !v[i].matched {
            m += 1;
        }
        i += 1;
    }
    m
}

/// 注入被拒计数。
pub fn count_rejected(v: &Vec<InjectionResult>) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < v.len() {
        if v[i].rejected {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 注入指纹（折形态 + 三要素 + 双侧留痕；顺序敏感）。
pub fn inject_fingerprint(v: &Vec<InjectionResult>) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < v.len() {
        h = mix32(h, v[i].form as u32);
        h = mix_bool(h, v[i].rejected);
        h = mix32(h, v[i].got as u32);
        h = mix32(h, v[i].want as u32);
        h = mix_bool(h, v[i].matched);
        h = mix64(h, v[i].probe_fp);
        h = mix64(h, v[i].tool_fp);
        i += 1;
    }
    h
}

/// 注入报告行。
pub fn inject_report(v: &Vec<InjectionResult>) -> Vec<String> {
    let mut o: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < v.len() {
        o.push(format!(
            "INJECT[{:02}] {} rejected={} got={} want={} matched={} probe={:016X} tool={:016X} :: {}",
            v[i].form,
            v[i].form_name,
            v[i].rejected,
            v[i].got,
            v[i].want,
            v[i].matched,
            v[i].probe_fp,
            v[i].tool_fp,
            v[i].line
        ));
        i += 1;
    }
    o
}

// ===========================================================================
// 17. 守恒终账 reconcile（整数恒等式，禁浮点）
// ===========================================================================

/// 守恒终账：八条整数恒等式逐条入册，任一不平即拒。
///
/// 写法纪律：每条恒等式都**先算分项再算总额**，
/// 两侧走**不同的代码路径**（分项侧遍历累加，总额侧读同步账），
/// 只有两侧独立算出同一值才算守恒。
/// 若两侧读同一份结构体的同一个字段，那不是守恒，是自证。
pub fn reconcile(s: &Scheme) -> ScRes<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    let led = &s.ledger;

    // R1 登记件数守恒
    //
    // `reg_target()` 定义即 `reg + pending`，故 `reg + pending == reg_target`
    // 是恒真空转的恒等式。真正的守恒内容是 (a) 守恒不漏 + (b) 收官做完。
    let r1 = led.reg.wrapping_add(led.pending);
    let r1_done = led.reg == SUITE_REG_TARGET && led.pending == 0;
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

    // R2 档位覆盖守恒（Σ各档格数 == 全部册定格数）
    let mut span = 0u32;
    let mut ti = 0usize;
    while ti < Tier::ALL.len() {
        let t = Tier::ALL[ti];
        let mut c = 0u32;
        let mut i = 0usize;
        while i < s.piece.rows.len() {
            if s.piece.rows[i].tier == t.index() {
                c += 1;
            }
            i += 1;
        }
        span = span.wrapping_add(c);
        ti += 1;
    }
    v.push(format!(
        "RECON R2 tiers={} span_sum={} cells={} dev={} ok={}",
        Tier::ALL.len(),
        span,
        SUITE_REG_TARGET,
        span as i64 - SUITE_REG_TARGET as i64,
        span == SUITE_REG_TARGET && Tier::ALL.len() == TIER_COUNT as usize
    ));
    if span != SUITE_REG_TARGET || Tier::ALL.len() != TIER_COUNT as usize {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R3 分项计数（四十八格各一账）
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

    // R6 档位覆盖 + 逐格走查守恒
    let band = match suite_coverage_ok(&s.piece.rows) {
        Ok(_) => true,
        Err(_) => false,
    };
    let walked = match s.walk_cells() {
        Ok(n) => n,
        Err(_) => 0,
    };
    v.push(format!(
        "RECON R6 suite_cover={} walked={} expect_walked={} piece_walked={} ok={}",
        band,
        walked,
        SUITE_REG_TARGET,
        s.piece.cells_walked,
        band && walked == SUITE_REG_TARGET && s.piece.cells_walked == SUITE_REG_TARGET
    ));
    if !band || walked != SUITE_REG_TARGET || s.piece.cells_walked != SUITE_REG_TARGET {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R7 条位映射 + 分界注记 + 双向对拍 + 键表守恒
    let sm = match slot_map_ok(&s.slots) {
        Ok(_) => true,
        Err(_) => false,
    };
    let bn = match boundary_note_ok() {
        Ok(_) => true,
        Err(_) => false,
    };
    let bidir = s.bidirectional_ok();
    let rows_bi = s.piece.rows_bidir_ok();
    let keys_ok = s.piece.keys.len() == SUITE_REG_TARGET as usize;
    let link_ok = link_note_ok();
    v.push(format!(
        "RECON R7 slotmap={} boundary={} bidir={} rows_bidir={} keys={} expect_keys={} link_note={} ok={}",
        sm,
        bn,
        bidir,
        rows_bi,
        s.piece.keys.len(),
        SUITE_REG_TARGET,
        link_ok,
        sm && bn && bidir && rows_bi && keys_ok && link_ok
    ));
    if !sm || !bn || !bidir || !rows_bi || !keys_ok || !link_ok {
        return Err(ErrKind::AddChainUnbalanced);
    }

    Ok(v)
}

// ===========================================================================
// 18. 自检（20 条）
// ===========================================================================

/// 单条自检。
#[derive(Clone, Debug)]
pub struct Check {
    /// 序号。
    pub id: u8,
    /// 名称。
    pub name: &'static str,
    /// 是否通过。
    pub pass: bool,
    /// 读数。
    pub note: String,
}

impl Check {
    /// 构造一条自检。
    pub fn new(id: u8, name: &'static str, pass: bool, note: String) -> Check {
        Check { id, name, pass, note }
    }
}

/// 自检 20 条（覆盖闭集 / 范围 / 档位 / 条位 / 分界 / 账 / 状态机 / 笛卡尔 / 注入）。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut v: Vec<Check> = Vec::new();
    let mut s = Scheme::new(seed);

    // C01 三档闭集齐备
    v.push(Check::new(
        1,
        "tier_closedset",
        TIER_COUNT == 3 && TIER_MAX == 2 && Tier::ALL.len() == 3,
        format!("tiers={} max={}", TIER_COUNT, TIER_MAX),
    ));

    // C02 六件套闭集齐备
    let mut suite_ok = Suite::ALL.len() == SUITE_COUNT as usize;
    let mut si0 = 0usize;
    while si0 < Suite::ALL.len() {
        if !Suite::ALL[si0].valid() {
            suite_ok = false;
        }
        si0 += 1;
    }
    v.push(Check::new(
        2,
        "suite_closedset",
        suite_ok,
        format!("suites={} exec={} assert={}", Suite::ALL.len(), SUITE_EXEC_COUNT, SUITE_ASSERT_COUNT),
    ));

    // C03 四十八格全登记
    let _ = s.register_all_cells();
    v.push(Check::new(
        3,
        "register_all_cells",
        s.piece.complete(),
        format!("complete={} rows={}", s.piece.complete(), s.piece.rows.len()),
    ));

    // C04 逐格自身有效性
    let mut rows_ok = true;
    let mut i = 0usize;
    while i < s.piece.rows.len() {
        if !s.piece.rows[i].valid() {
            rows_ok = false;
        }
        i += 1;
    }
    v.push(Check::new(
        4,
        "rows_valid",
        rows_ok,
        format!("rows={}", s.piece.rows.len()),
    ));

    // C05 档位覆盖（每档格数齐 + 同档读数互异）
    let pc = suite_coverage_ok(&s.piece.rows);
    v.push(Check::new(
        5,
        "suite_coverage",
        pc.is_ok(),
        format!("cover={:?} expect_rows={}", pc.is_ok(), s.piece.cover.len()),
    ));

    // C06 逐格走查
    let wr = s.walk_cells();
    v.push(Check::new(
        6,
        "walk_cells",
        wr.is_ok() && wr.unwrap_or(0) == SUITE_REG_TARGET,
        format!("walked={:?} expect={}", wr, SUITE_REG_TARGET),
    ));

    // C07 三维坐标一致（读回行表）
    v.push(Check::new(
        7,
        "coord_consistent",
        s.piece.coord_consistent(),
        format!("consistent={}", s.piece.coord_consistent()),
    ));

    // C08 档位摊派自洽（`4/6/6` 且合计 == 48）
    let mut tsum = 0u32;
    let mut ti = 0usize;
    while ti < Tier::ALL.len() {
        tsum = tsum.wrapping_add(Tier::ALL[ti].cell_target());
        ti += 1;
    }
    v.push(Check::new(
        8,
        "tier_budget",
        tsum == SUITE_REG_TARGET
            && Tier::G1.cell_target() == 4
            && Tier::G4.cell_target() == 6
            && Tier::G16.cell_target() == 6,
        format!("budget={} expect={}", tsum, SUITE_REG_TARGET),
    ));

    // C09 三维闭集存在性自洽（48 存在 + 6 不存在）
    let mut ex = 0u32;
    let mut nx = 0u32;
    let mut a = 0usize;
    while a < CART_DA {
        let mut b = 0usize;
        while b < CART_DB {
            let mut c = 0usize;
            while c < CART_DC {
                if triple_exists(a as u8, b as u8, c as u8) {
                    ex += 1;
                } else {
                    nx += 1;
                }
                c += 1;
            }
            b += 1;
        }
        a += 1;
    }
    v.push(Check::new(
        9,
        "triple_exists",
        ex == CART_WRITABLE as u32
            && nx == (CART_EXPECTED - CART_WRITABLE) as u32,
        format!("exists={} not_exists={} expect={}", ex, nx, CART_WRITABLE),
    ));

    // C10 范围格键表（四十八行，与三维坐标同源）
    v.push(Check::new(
        10,
        "suite_keys",
        s.piece.keys.len() == SUITE_REG_TARGET as usize,
        format!("keys={} expect={}", s.piece.keys.len(), SUITE_REG_TARGET),
    ));

    // C11 状态机合法边 3 条
    let le = legal_edges();
    v.push(Check::new(
        11,
        "legal_edges",
        le.len() == LEGAL_EDGES as usize,
        format!("legal={} expect={}", le.len(), LEGAL_EDGES),
    ));

    // C12 非法边 13 条（含自环）
    let ie = illegal_edges_measured();
    v.push(Check::new(
        12,
        "illegal_edges",
        ie == ILLEGAL_EDGES,
        format!("illegal={} expect={}", ie, ILLEGAL_EDGES),
    ));

    // C13 状态机推进到 Mapped
    let adv = s.advance(Stage::Mapped);
    v.push(Check::new(
        13,
        "advance_mapped",
        adv.is_ok() && s.stage == Stage::Mapped,
        format!("stage={}", s.stage.name()),
    ));

    // C14 非法自环被拒并留痕
    let rej0 = s.rej.total;
    let _ = s.advance(Stage::Mapped);
    v.push(Check::new(
        14,
        "reject_self_loop",
        s.rej.total == rej0 + 1,
        format!("rej_total={} grew={}", s.rej.total, s.rej.total - rej0),
    ));

    // C15 条位映射入册（十一条位齐 + 六件全覆盖）
    let ns = s.note_slots();
    v.push(Check::new(
        15,
        "slot_map",
        ns.is_ok() && ns.unwrap_or(0) == SLOT_COUNT,
        format!("rows={:?} expect={}", ns, SLOT_COUNT),
    ));

    // C16 分界注记入册
    let nb = s.note_boundary();
    v.push(Check::new(
        16,
        "boundary_note",
        nb.is_ok() && nb.unwrap_or(0) == BOUNDARY_COLS,
        format!("rows={:?} expect={}", nb, BOUNDARY_COLS),
    ));

    // C17 范围冻结
    let seal = s.seal();
    v.push(Check::new(
        17,
        "seal",
        seal.is_ok() && s.freeze_hash != 0,
        format!("freeze={:016X}", s.freeze_hash),
    ));

    // C18 冻结校验（同 tick 重算一致）
    let vf = s.verify_freeze();
    v.push(Check::new(
        18,
        "verify_freeze",
        vf.is_ok(),
        format!("verify={}", vf.is_ok()),
    ));

    // C19 阶段推进到 Frozen + 调度入册
    //
    // `seal()` 不推阶段；`dispatch()` 首行又只认 `Frozen` ——
    // 中间少走一条合法边 Mapped → Frozen，C19 就会恒在 `stage=mapped` 上红。
    let af = s.advance(Stage::Frozen);
    let dp = s.dispatch();
    v.push(Check::new(
        19,
        "dispatch",
        af.is_ok() && dp.is_ok() && s.stage == Stage::Dispatched,
        format!("stage={}", s.stage.name()),
    ));

    // C20 笛卡尔 54 格 + 注入全拒 + 守恒终账
    //
    // 判定一律走 `cartesian_assert()` 权威出口：
    // 若这里自己按 `cok/cbad` 另写一份，就会出现两套判定口径，
    // 一套认注入格为坏、另一套认它为绿，谁改谁都可能整体翻红。
    //
    // 注入侧读数必须**先回填守恒账**再对账：R5a/R5b 查的是
    // `injected_total >= FORM_COUNT` 与「全拒」，而这两项只在
    // `note_inject()` 回填后才有值。observe / replay 两条路径都在
    // 调 `reconcile()` 前先 `note_inject()`，唯独 self_check 漏了，
    // 于是 C20 恒红而另外两条路径全绿。
    // 教训：守恒账的对账方与记账方必须成对出现在**每一条**执行路径上。
    let cells = run_cartesian(seed);
    let (ct, cok, cwr, cbad) = cartesian_stats(&cells);
    let ca = cartesian_assert(&cells);
    let inj_sc = inject_core_with_tool(seed);
    let miss_sc = detect_miss(&inj_sc);
    let rej_sc = count_rejected(&inj_sc);
    s.note_inject(inj_sc.len() as u32, rej_sc);
    let rec = reconcile(&s);
    v.push(Check::new(
        20,
        "cartesian_inject_reconcile",
        ca.is_ok()
            && miss_sc == 0
            && rej_sc == inj_sc.len() as u32
            && rec.is_ok()
            && rec.as_ref().map(|r| r.len()).unwrap_or(0) == RECON_ROWS,
        format!(
            "cells={} ok={} writable={} bad={} assert={} miss={} rejected={}/{} recon_rows={} expect={}",
            ct,
            cok,
            cwr,
            cbad,
            ca.unwrap_or(String::from("ERR")),
            miss_sc,
            rej_sc,
            inj_sc.len(),
            rec.as_ref().map(|r| r.len()).unwrap_or(0),
            RECON_ROWS
        ),
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
        h = mix_str(h, v[i].name);
        h = mix_str(h, &v[i].note);
        i += 1;
    }
    h
}

/// 自检是否全通过。
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
// 19. 导出 export_kv + 观察体 observe
// ===========================================================================

/// 观察体。
#[derive(Clone, Debug)]
pub struct ScObserve {
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
    /// 条位映射行数。
    pub slot_rows: u32,
    /// TLV 往返一致。
    pub tlv_roundtrip: bool,
    /// TLV 实际长度。
    pub tlv_len: usize,
    /// 总判定。
    pub all_ok: bool,
}

impl ScObserve {
    /// 总判定（读 `all_ok` 字段，闭集安全）。
    pub fn all_ok(&self) -> bool {
        self.all_ok
    }

    /// 单行摘要。
    pub fn summary(&self) -> String {
        format!(
            "UNX-F3081 seed={:016X} all_ok={} freeze={:016X} fp={:016X} inject={}/{} miss={} cart={}/{} slots={} ring_live={} dropped={} tlv={}B rt={} replay_stable={}",
            self.seed,
            self.all_ok,
            self.freeze_hash,
            self.scheme_fp,
            self.injected_rejected,
            self.injected_total,
            self.inject_miss,
            self.cart_ok,
            self.cart_total,
            self.slot_rows,
            self.ring_live,
            self.ring_dropped,
            self.tlv_len,
            self.tlv_roundtrip,
            self.replay_stable()
        )
    }

    /// 回放稳定性（由 `replay_fp` + 失配数 + 总判定合成）。
    pub fn replay_stable(&self) -> bool {
        self.replay_fp != 0 && self.inject_miss == 0 && self.all_ok
    }
}

/// 导出 KV（键值对纯内存文本，禁 IO）。
pub fn export_kv(o: &ScObserve) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!("mod.tag={}", MODULE_TAG));
    v.push(format!("task={}", TASK_ID));
    v.push(format!("seg.base={:#06x}", SEG_BASE));
    v.push(format!("seg.mask={:#06x}", SEG_MASK));
    v.push(format!("export.version={:#018x}", EXPORT_VERSION));
    v.push(format!("tier.count={}", TIER_COUNT));
    v.push(format!("suite.count={}", SUITE_COUNT));
    v.push(format!("view.count={}", VIEW_COUNT));
    v.push(format!("slot.count={}", SLOT_COUNT));
    v.push(format!("cart.expected={}", CART_EXPECTED));
    v.push(format!("cart.writable={}", CART_WRITABLE));
    v.push(format!("part.target={}", PART_TARGET));
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
    v.push(format!("slot.rows={}", o.slot_rows));
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
pub fn observe(seed: u64) -> ScObserve {
    // —— 第一步：回放 ——
    let tr = replay(seed);
    let (stable, rfp) = replay_stable(seed);

    // —— 第二步：真跑方案生命周期 ——
    let mut s = Scheme::new(seed);
    let _ = s.register_all_cells();
    let _ = s.advance(Stage::Mapped);
    let _ = s.note_slots();
    let _ = s.note_boundary();
    // 加法链分项**已由登记期逐格记满**（`PART_TARGET` 项），
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
    let (ct, cok, cwr, _cbad) = cartesian_stats(&cells);

    // —— 第五步：自检 ——
    let checks = self_check(seed);
    let check_fp = check_fingerprint(&checks);

    // —— 第六步：各分项判定 ——
    let checks_pass = all_check_pass(&checks);
    let recon_ok = recon_rows.len() == RECON_ROWS;
    let inject_ok = miss == 0 && rej == tot && tot >= FORM_COUNT;
    // 笛卡尔判定走 `cartesian_assert()`（同 C20 口径，唯一权威出口）。
    let cart_ok2 = cartesian_assert(&cells).is_ok()
        && ct == CART_EXPECTED as u32
        && cwr == CART_WRITABLE as u32;

    let mut o = ScObserve {
        seed,
        freeze_hash,
        scheme_fp,
        check_fp,
        cart_fp,
        inject_fp,
        replay_fp: rfp,
        injected_rejected: rej,
        injected_total: tot,
        inject_miss: miss,
        recon_rows,
        checks,
        scheme_rows,
        inject_rows,
        cart_total: ct,
        cart_ok: cok,
        // 环形账三读数在方案实例上取（活账，不进冻结指纹）。
        ring_live: s.ring.live(),
        ring_dropped: s.ring.dropped,
        fsm_final: s.stage.name(),
        fsm_steps: s.walk_steps as u64,
        fsm_rejected: s.rej.total,
        slot_rows: s.slots.len() as u32,
        tlv_roundtrip: false,
        tlv_len: 0,
        all_ok: false,
    };

    // —— 第七步：TLV 往返 + 总判定（消自指：先定 base_ok 再跑往返）——
    //
    // 顺序纪律：先把 `all_ok` 按「除往返外」的判据定稿并写回，
    // 再用**已定稿**的 `o` 跑 `tlv_roundtrip_ok()`（它内部重新导出再解析），
    // 最后把往返结果合并进 `all_ok`。
    // 若先跑往返再定 `all_ok`，而往返的导出内容里含 `all_ok`，
    // 那就是「用结论证明结论」——自我循环，判据形同虚设。
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
// 20. 确定性回放 replay（不调 observe）
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
    /// 轨迹指纹。
    pub fp: u64,
    /// 达标步数。
    pub passed: u32,
    /// 总步数。
    pub total: u32,
    /// 全轨迹是否达标。
    pub ok: bool,
}

impl ReplayTrace {
    /// 报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "REPLAY total={} passed={} fp={:016X} ok={}",
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

/// 确定性回放（15 步，同种子必得同轨迹）。
pub fn replay(seed: u64) -> ReplayTrace {
    let mut h = FNV_BASIS;
    let mut steps: Vec<ReplayStep> = Vec::new();
    let mut step = 0u8;
    let mut s = Scheme::new(seed);

    // 步 1 立项
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep {
        step,
        action: "new_draft",
        fp: s.scheme_fingerprint(),
        tick: s.tick,
        ok: s.stage == Stage::Draft,
    });
    step += 1;

    // 步 2 四十八格逐格登记（走 `register_all_cells()`：它在齐备后
    // 统一 `seal_scope()` 派生覆盖账；直接手搓 `register_cell` 循环
    // 会漏掉派生，步 3 档位覆盖立刻 `SuiteCoverageShort`）
    let reg = s.register_all_cells();
    let reg_ok = reg.is_ok();
    h = mix64(h, s.piece.fingerprint());
    steps.push(ReplayStep {
        step,
        action: "register_all_cells",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: reg_ok && s.piece.complete(),
    });
    step += 1;

    // 步 3 档位覆盖断言
    let bo = suite_coverage_ok(&s.piece.rows);
    h = mix_str(h, if bo.is_ok() { "cover_ok" } else { "cover_bad" });
    steps.push(ReplayStep {
        step,
        action: "suite_coverage",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: bo.is_ok(),
    });
    step += 1;

    // 步 4 逐格走查
    let wm = s.walk_cells();
    h = mix32(h, wm.unwrap_or(0));
    steps.push(ReplayStep {
        step,
        action: "walk_cells",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: wm.unwrap_or(0) == SUITE_REG_TARGET,
    });
    step += 1;

    // 步 5 条位映射入册
    let ns = s.note_slots();
    h = mix64(h, s.slot_fingerprint());
    steps.push(ReplayStep {
        step,
        action: "note_slots",
        fp: s.slot_fingerprint(),
        tick: s.tick,
        ok: ns.is_ok() && ns.unwrap_or(0) == SLOT_COUNT,
    });
    step += 1;

    // 步 6 分界注记入册
    let nb = s.note_boundary();
    h = mix64(h, s.boundary_fingerprint());
    steps.push(ReplayStep {
        step,
        action: "note_boundary",
        fp: s.boundary_fingerprint(),
        tick: s.tick,
        ok: nb.is_ok() && nb.unwrap_or(0) == BOUNDARY_COLS,
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

    // 步 8 加法链分项**核对**（分项已在 `register_all_cells` 期间记满）
    let part_ok = s.ledger.items == PART_TARGET && s.ledger.total == s.ledger.parts_sum();
    h = mix64(h, s.ledger.parts_sum());
    steps.push(ReplayStep {
        step,
        action: "note_parts",
        fp: s.ledger.fingerprint(),
        tick: s.tick,
        ok: part_ok,
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
        ok: miss == 0 && count_rejected(&inj) == inj.len() as u32,
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

    // 步 11 范围冻结
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
    // 「写了不读」的 warning，且读代码的人会以为后面还有步 16。

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
        steps,
        fp: h,
        passed,
        total,
        ok: passed == total && total == REPLAY_STEPS_CAP,
    }
}

/// 回放稳定性（同种子两次跑出同轨迹）。
pub fn replay_stable(seed: u64) -> (bool, u64) {
    let a = replay(seed);
    let b = replay(seed);
    let same = a.fp == b.fp && a.ok && b.ok;
    (same, a.fp)
}

// ===========================================================================
// 21. 性能分解 / 内存账
// ===========================================================================

/// 性能分解总表（逐项对标 Windows 同类能力写「深在哪」）。
pub fn complexity_table() -> Vec<CostItem> {
    let mut v: Vec<CostItem> = Vec::new();
    v.push(CostItem::new("register_cell", "O(1)", "O(1)", SUITE_REG_TARGET, 112,
        "Windows 无对照：三档六件三视图的终账范围冻结是本单专有方案纪律"));
    v.push(CostItem::new("SuitePiece::register", "O(n)", "O(1)", SUITE_REG_TARGET, 128,
        "线性定位格下标：索引表比全表扫描省，但换来建表成本，本单规模下可接受"));
    v.push(CostItem::new("suite_coverage_ok", "O(n^2)", "O(n)", SUITE_REG_TARGET * 6, 96,
        "深在**档位覆盖 + 同档读数互异双判**：Windows 内存档只测容量，不测门限互斥"));
    v.push(CostItem::new("seal_scope", "O(n^2)", "O(n)", SUITE_REG_TARGET * 3, 128,
        "四十八格齐备后一次派生；登记期派生会被自己的空行死锁拦住"));
    v.push(CostItem::new("walk_cells", "O(n)", "O(1)", SUITE_REG_TARGET, 16,
        "比 Windows 页表走查更严：读行表不重算，账面与运行期同源"));
    v.push(CostItem::new("coord_consistent", "O(n)", "O(1)", SUITE_REG_TARGET, 8,
        "三维坐标唯一性：档位-件套-视图里前两维不自由，此处专查该约束"));
    v.push(CostItem::new("rows_bidir_ok", "O(n)", "O(1)", SUITE_REG_TARGET, 48,
        "深在**双向**：方案侧独立复现折盐夹取，实现侧读回；少对一项即半对拍"));
    v.push(CostItem::new("slot_map_ok", "O(n^2)", "O(n)", SLOT_COUNT * SLOT_COUNT, 64,
        "深在**满射性**：六件每件至少一条位，空件即拦——Windows 无对应概念"));
    v.push(CostItem::new("boundary_note_ok", "O(n^2)", "O(n)", BOUNDARY_COLS * BOUNDARY_COLS, 64,
        "深在**执行职责不越界**：本单只冻范围不落执行，越界即拒"));
    v.push(CostItem::new("link_note_ok", "O(1)", "O(n)", LINK_POINTS, 32,
        "非对位关系显式注明「本单只冻范围」：不注明则看不出为何无十一条位实现体"));
    v.push(CostItem::new("run_cartesian", "O(1)", "O(n)", CART_EXPECTED as u32, (CART_EXPECTED * 48) as u32,
        "54 格独立夹具：Windows 无同构矩阵，绝不共用一个 fixture"));
    v.push(CostItem::new("cartesian_assert", "O(n)", "O(1)", CART_EXPECTED as u32, 24,
        "注入格必坏 + 未注入可写格必绿 + 注入格数 == 可写格数（48）"));
    v.push(CostItem::new("inject_core", "O(1)", "O(n)", FORM_TOTAL, (FORM_TOTAL * 96) as u32,
        "12 形态 12 码零空档：每形态的期望码与实际码逐条严判"));
    v.push(CostItem::new("reconcile", "O(n)", "O(n)", RECON_ROWS as u32, (RECON_ROWS * 64) as u32,
        "八条恒等式零偏离；分项侧与总额侧走不同代码路径"));
    v.push(CostItem::new("export_tlv", "O(1)", "O(n)", TLV_RECORD_COUNT as u32, (TLV_RECORD_COUNT * TLV_RECORD_LEN) as u32,
        "定宽长度字段：长度常量由 push 次数反推，往返逐条一致"));
    v.push(CostItem::new("replay", "O(1)", "O(n)", REPLAY_STEPS_CAP, 15 * 48,
        "15 步定长回放：每步一个可判动作，指纹顺序敏感"));
    v.push(CostItem::new("observe", "O(1)", "O(n)", 1, 8,
        "观察入口真实跑全链，禁与 replay 互递归"));
    v
}

/// 内存账（每项显式给出字节数，禁估算口吻）。
pub fn complexity_memory() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "MEM SuiteRow = {} B x {} 行 = {} B",
        core::mem::size_of::<SuiteRow>(),
        SUITE_REG_TARGET,
        core::mem::size_of::<SuiteRow>() * SUITE_REG_TARGET as usize
    ));
    v.push(format!(
        "MEM CartCell = {} B x {} 格 = {} B",
        core::mem::size_of::<CartCell>(),
        CART_EXPECTED,
        core::mem::size_of::<CartCell>() * CART_EXPECTED
    ));
    v.push(format!(
        "MEM SlotRow = {} B x {} 条位 = {} B",
        core::mem::size_of::<SlotRow>(),
        SLOT_COUNT,
        core::mem::size_of::<SlotRow>() * SLOT_COUNT as usize
    ));
    v.push(format!(
        "MEM BoundaryRow = {} B x {} 行 = {} B",
        core::mem::size_of::<BoundaryRow>(),
        BOUNDARY_COLS,
        core::mem::size_of::<BoundaryRow>() * BOUNDARY_COLS as usize
    ));
    v.push(format!(
        "MEM InjectionResult = {} B x {} 形态 = {} B",
        core::mem::size_of::<InjectionResult>(),
        FORM_TOTAL,
        core::mem::size_of::<InjectionResult>() * FORM_TOTAL as usize
    ));
    v.push(format!(
        "MEM RingLog 容量 = {} 条 x 24 B = {} B（append-only，dropped 显性）",
        RING_CAP,
        RING_CAP as u32 * 24
    ));
    v.push(format!(
        "MEM RejectionRing 容量 = {} 条 x 24 B = {} B（非法迁移独立环）",
        REJECT_CAP,
        REJECT_CAP as u32 * 24
    ));
    v.push(format!(
        "MEM ExpLog 无固定容量（入册条数 = 条位 1 + 分界 1 + 冻结 1 + 调度 1 = {}）",
        4
    ));
    v.push(format!(
        "MEM Scheme（不含四本活账堆）= {} B",
        core::mem::size_of::<Scheme>()
    ));
    v.push(format!(
        "MEM syscalls = 0（纯逻辑模块：无文件 / 无网络 / 无时钟 / 无浮点）"
    ));
    v
}

// ===========================================================================
// 22. 演示入口（纯内存，禁 IO）
// ===========================================================================

/// 演示一：档位预算自洽。
pub fn demo_tier_budget() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut total = 0u32;
    let mut i = 0usize;
    while i < Tier::ALL.len() {
        let t = Tier::ALL[i];
        v.push(format!(
            "DEMO tier={} mib={} views={} suites={} cells={} views_applied={}",
            t.label(),
            t.mib(),
            VIEW_COUNT,
            t.suite_count(),
            t.cell_target(),
            t.view_count()
        ));
        total = total.wrapping_add(t.cell_target());
        i += 1;
    }
    v.push(format!(
        "DEMO total_cells={} expect={} ok={}",
        total,
        SUITE_REG_TARGET,
        total == SUITE_REG_TARGET
    ));
    v
}

/// 演示二：条位映射册定。
pub fn demo_slot_map() -> Vec<String> {
    let t = slot_map_table();
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < t.len() {
        v.push(format!("  {}", t[i].row()));
        i += 1;
    }
    let ok = match slot_map_ok(&t) {
        Ok(r) => {
            let mut j = 0usize;
            while j < r.len() {
                v.push(r[j].clone());
                j += 1;
            }
            "OK"
        }
        Err(e) => {
            v.push(err_line(e, SLOT_COUNT as u64));
            "ERR"
        }
    };
    v.push(format!("DEMO slot_map rows={} verdict={}", t.len(), ok));
    v
}

/// 演示三：笛卡尔矩阵。
pub fn demo_cartesian(seed: u64) -> Vec<String> {
    let cells = run_cartesian(seed);
    let mut v: Vec<String> = Vec::new();
    match cartesian_assert(&cells) {
        Ok(m) => v.push(m),
        Err(e) => v.push(err_line(e, CART_EXPECTED as u64)),
    }
    let (tot, ok, wr, bad) = cartesian_stats(&cells);
    v.push(format!(
        "DEMO cartesian total={} ok={} writable={} bad={}",
        tot, ok, wr, bad
    ));
    v
}

/// 演示四：注入核。
pub fn demo_inject(seed: u64) -> Vec<String> {
    let inj = inject_core_with_tool(seed);
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < inj.len() {
        v.push(format!(
            "  INJECT[{:02}] {} rejected={} matched={}",
            inj[i].form, inj[i].form_name, inj[i].rejected, inj[i].matched
        ));
        i += 1;
    }
    v.push(format!(
        "DEMO inject total={} rejected={} miss={}",
        inj.len(),
        count_rejected(&inj),
        detect_miss(&inj)
    ));
    v
}

/// 演示五：范围冻结。
pub fn demo_freeze(seed: u64) -> Vec<String> {
    let mut s = Scheme::new(seed);
    let mut v: Vec<String> = Vec::new();
    match s.register_all_cells() {
        Ok(n) => v.push(format!("DEMO registered={} expect={}", n, SUITE_REG_TARGET)),
        Err(e) => v.push(err_line(e, seed)),
    }
    let _ = s.advance(Stage::Mapped);
    let _ = s.note_slots();
    let _ = s.note_boundary();
    match s.seal() {
        Ok(h) => v.push(format!("DEMO freeze={:016X} tick={}", h, s.freeze_tick)),
        Err(e) => v.push(err_line(e, seed)),
    }
    match s.verify_freeze() {
        Ok(h) => v.push(format!("DEMO verify={:016X}", h)),
        Err(e) => v.push(err_line(e, seed)),
    }
    let _ = s.advance(Stage::Frozen);
    match s.dispatch() {
        Ok(h) => v.push(format!("DEMO dispatch={:016X} stage={}", h, s.stage.name())),
        Err(e) => v.push(err_line(e, seed)),
    }
    v
}

/// 关联任务单注记（纯文本，禁 IO）。
pub fn related_str() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "RELATED 本单={} 段={:#06x} 册目标=三档终账总纲 六件套范围冻结",
        TASK_ID, SEG_BASE
    ));
    v.push(
        "RELATED 条位 F3082-F3092 十一条位：各自只消费本单冻结范围，本单不落其执行体"
            .to_string(),
    );
    v.push(
        "RELATED 依赖：1GiB/4GiB/16GiB 三档读数口径与 F2981 域闭接；断言件对位 F3021 三档"
            .to_string(),
    );
    v.push(
        "RELATED 分界：本单只冻范围（范围哈希 + 条位映射），执行归条位；越界即 BoundaryScopeLeak"
            .to_string(),
    );
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
pub const TLV_RECORD_COUNT: usize = 12 + 12 + 22 + 6 + 6 + 12;
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
pub fn export_tlv(o: &ScObserve) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(2048);
    let mut head_payload: Vec<u8> = Vec::with_capacity(TLV_HEAD_PAYLOAD);
    push_u64(&mut head_payload, EXPORT_VERSION);
    push_u64(&mut head_payload, SEG_BASE as u64);
    push_u64(&mut head_payload, 360u64);
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
    // 维度规模（12 条）
    push_tlv_u64(&mut out, 0x10, TIER_COUNT as u64);
    push_tlv_u64(&mut out, 0x11, SUITE_COUNT as u64);
    push_tlv_u64(&mut out, 0x12, VIEW_COUNT as u64);
    push_tlv_u64(&mut out, 0x13, SLOT_COUNT as u64);
    push_tlv_u64(&mut out, 0x14, CART_EXPECTED as u64);
    push_tlv_u64(&mut out, 0x15, CART_WRITABLE as u64);
    push_tlv_u64(&mut out, 0x16, PART_TARGET as u64);
    push_tlv_u64(&mut out, 0x17, SUITE_REG_TARGET as u64);
    push_tlv_u64(&mut out, 0x18, BOUNDARY_COLS as u64);
    push_tlv_u64(&mut out, 0x19, LINK_POINTS as u64);
    push_tlv_u64(&mut out, 0x1A, FORM_COUNT as u64);
    push_tlv_u64(&mut out, 0x1B, LONG_RUN_TICKS as u64);
    // 三档对位（12 条 = 3 档 x 4）
    let mut ti = 0usize;
    while ti < Tier::ALL.len() {
        let tp = Tier::ALL[ti];
        push_tlv_u64(&mut out, 0x30u8.wrapping_add(ti as u8), tp.mib() as u64);
        push_tlv_u64(&mut out, 0x33u8.wrapping_add(ti as u8), tp.view_count() as u64);
        push_tlv_u64(&mut out, 0x36u8.wrapping_add(ti as u8), tp.suite_lo() as u64);
        push_tlv_u64(&mut out, 0x39u8.wrapping_add(ti as u8), tp.cell_target() as u64);
        ti += 1;
    }
    // 条位映射（22 条 = 11 条位 x 2）
    let mut si = 0usize;
    while si < SLOT_COUNT as usize {
        let sg = slot_to_suite(si as u8).unwrap_or(0u8);
        push_tlv_u64(&mut out, 0x50u8.wrapping_add(si as u8), SLOT_NO_LO as u64 + si as u64);
        push_tlv_u64(&mut out, 0x5Bu8.wrapping_add(si as u8), sg as u64);
        si += 1;
    }
    // 分界与终账（6 条）
    push_tlv_u64(&mut out, 0x60, BOUNDARY_FACES as u64);
    push_tlv_u64(&mut out, 0x61, PRECASE_NO as u64);
    push_tlv_u64(&mut out, 0x62, SLOT_MAP_TARGET as u64);
    push_tlv_u64(&mut out, 0x63, RECON_ROWS as u64);
    push_tlv_u64(&mut out, 0x64, SLOT_NO_HI as u64);
    push_tlv_u64(&mut out, 0x65, PART_FLOOR);
    // 指纹段（6 条）
    push_tlv_u64(&mut out, 0x70, o.freeze_hash);
    push_tlv_u64(&mut out, 0x71, o.scheme_fp);
    push_tlv_u64(&mut out, 0x72, o.check_fp);
    push_tlv_u64(&mut out, 0x73, o.cart_fp);
    push_tlv_u64(&mut out, 0x74, o.inject_fp);
    push_tlv_u64(&mut out, 0x75, o.replay_fp);
    // 读数段（12 条）
    push_tlv_u64(&mut out, 0x80, o.injected_total as u64);
    push_tlv_u64(&mut out, 0x81, o.injected_rejected as u64);
    push_tlv_u64(&mut out, 0x82, o.inject_miss as u64);
    push_tlv_u64(&mut out, 0x83, o.cart_total as u64);
    push_tlv_u64(&mut out, 0x84, o.cart_ok as u64);
    push_tlv_u64(&mut out, 0x85, o.ring_live as u64);
    push_tlv_u64(&mut out, 0x86, o.ring_dropped);
    push_tlv_u64(&mut out, 0x87, o.fsm_steps);
    push_tlv_u64(&mut out, 0x88, o.fsm_rejected);
    push_tlv_u64(&mut out, 0x89, o.slot_rows as u64);
    push_tlv_u64(&mut out, 0x8A, o.tlv_len as u64);
    push_tlv_u64(&mut out, 0x8B, if o.all_ok { 1 } else { 0 });
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
        return Err(ErrKind::SuiteCellMissing);
    }
    if buf[0] != TLV_TAG_HEAD {
        return Err(ErrKind::SuiteDimMismatch);
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(ErrKind::SuiteDimMismatch);
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
            return Err(ErrKind::SuiteDimMismatch);
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
        return Err(ErrKind::SuiteDimMismatch);
    }
    Ok(vals)
}

/// TLV 往返一致（导出 → 解析 → 关键字段逐项对拍）。
pub fn tlv_roundtrip_ok(o: &ScObserve) -> bool {
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
    // 记录段下标偏移：vals[0..2]=组头 3 条；vals[3..14]=维度 12 条；
    // vals[15..26]=三档对位 12 条；vals[27..48]=条位映射 22 条；
    // vals[49..54]=分界衔接 6 条；vals[55..60]=指纹段；vals[61..72]=读数段。
    if vals[3] != TIER_COUNT as u64 {
        return false;
    }
    if vals[4] != SUITE_COUNT as u64 {
        return false;
    }
    if vals[5] != VIEW_COUNT as u64 {
        return false;
    }
    if vals[6] != SLOT_COUNT as u64 {
        return false;
    }
    // 三档对位段是**每档 4 条交错**（mib / view_count / suite_lo / cell_target），
    // 不是「三档的 mib 连排」——按连排取下标会读到
    // 「1GiB 档的视图数」当成「4GiB 档的 MiB」，值小一个量级，
    // 往返校验恒红且报错信息毫无指向性。
    // 段起点：组头 3 + 维度 12 = 15；档 0 占 15..19，档 1 占 19..23，档 2 占 23..27。
    if vals[15] != TIER_MIB_G1 as u64 {
        return false;
    }
    if vals[16] != Tier::G1.view_count() as u64 {
        return false;
    }
    if vals[18] != Tier::G1.cell_target() as u64 {
        return false;
    }
    if vals[19] != TIER_MIB_G4 as u64 {
        return false;
    }
    if vals[23] != TIER_MIB_G16 as u64 {
        return false;
    }
    if vals[27] != SLOT_NO_LO as u64 {
        return false;
    }
    if vals[55] != o.freeze_hash {
        return false;
    }
    if vals[56] != o.scheme_fp {
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
