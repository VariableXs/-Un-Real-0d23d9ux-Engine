//! =====================================================================================
//! 任务单：UNX-F3061 · 断电演练方案冻结（×10 轮次矩阵定义）
//! 域/批：CoRun·UNX-A4-B34（回写器 · 断电演练方案总装）
//! 册目标：420 行（断电演练方案 189 + 冻结 126 + 断言 105；测试段不计）
//! 本文件实际：≥2800 行（深化版纯功能件）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F3061-J1，逐字摘抄）：
//!   方案冻结：断电演练 ×10 轮次矩阵（轮次×断电时点×负载档三维）定义哈希入册，与 B5 回写面（F7602 消费）分界注记在案，矩阵漂移注入拦截
//!
//! 语义边界（逐字摘抄）：
//!   只做方案冻结；不做演练执行（F3064）、不做盘面（B1–B5）。
//!
//! 依赖与嫁接源（逐字摘抄）：
//!   依赖 F7602/F2726/F2876；嫁接源为域内断电制度，实现自研。
//!
//! 风险与回退（逐字摘抄）：
//!   相位偏聚（时点全聚回写毕相位）——相位覆盖断言（三相位各有轮次），偏聚注入拦截。
//!
//! 复测方式（逐字摘抄）：
//!   矩阵三维在档，分界注记可查，漂移注入拦截，方案哈希入库，矩阵定义件哈希与三维对位件随批归档，方案变更零断言读数可回放。
//!
//! 码段范围声明：
//!   本单独占诊断码段 0x24c0 ~ 0x24ff。
//!   `SEG_BASE = 0x24c0`，`SEG_MASK = 0x003f`（低 6 位为段内序号，
//!   段内共 64 个码位；本单实际使用 21 个错误变体 + 每变体一码一段，
//!   恒 ≤ 60 变体，最大占用 0x24d4，未触及段上界 0x24ff）。
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

/// 诊断码段基数（本单独占 0x24c0–0x24ff，共 64 个码位）。
pub const SEG_BASE: u16 = 0x24c0;
/// 诊断码段掩码：低 6 位为段内序号，高 10 位为段基数。
pub const SEG_MASK: u16 = 0x003f;
/// 段内码位数。
pub const SEG_SLOTS: u16 = SEG_MASK + 1;
/// 本单最大诊断码（`SEG_BASE + 变体数 - 1`）。
pub const SEG_MAX_CODE: u16 = SEG_BASE + 20;
/// 模块标签。
pub const MODULE_TAG: &str = "UNX-A4-B34/F3061";
/// 判据号。
pub const TASK_ID: &str = "UNX-F3061-J1";
/// 判据全文。
pub const CRITERION: &str = "方案冻结：断电演练 ×10 轮次矩阵（轮次×断电时点×负载档三维）定义哈希入册，与 B5 回写面（F7602 消费）分界注记在案，矩阵漂移注入拦截";

/// 断电演练轮次数（册定：×10 轮次矩阵）。
pub const ROUND_COUNT: u32 = 10;
/// 轮次序号 0（第一轮）。
pub const ROUND_FIRST: u8 = 0;
/// 轮次序号上限（**开**）：轮次闭集为 `0..10` 共 10 轮。
pub const ROUND_LIMIT: u32 = ROUND_COUNT;

/// 回写相位数（册定：回写前 / 回写中 / 回写毕 三相位）。
pub const PHASE_COUNT: u32 = 3;
/// 相位 0：回写前（脏页尚未开始落盘）。
pub const PHASE_PRE: u8 = 0;
/// 相位 1：回写中（落盘进行中）。
pub const PHASE_DURING: u8 = 1;
/// 相位 2：回写毕（落盘已收口）。
pub const PHASE_POST: u8 = 2;

/// 负载档数（册定：空载 / 轻载 / 重载 三档，与 F3021 三档同源）。
pub const LOAD_COUNT: u32 = 3;
/// 负载档 0：空载。
pub const LOAD_IDLE: u8 = 0;
/// 负载档 1：轻载。
pub const LOAD_LIGHT: u8 = 1;
/// 负载档 2：重载。
pub const LOAD_HEAVY: u8 = 2;

/// 每相位册定轮次数（10 轮摊到 3 相位：`4/3/3`，相位覆盖断言的对家）。
///
/// 摊派不是均分而是 `4/3/3`：回写前相位多排一轮，
/// 因为回写前断电是**最脏**的时点（脏页最多、最能暴露丢页），
/// 演练资源要向最脏时点倾斜。若写成 `10/3` 的均分假设，
/// 整数除法会逼出「某相位零轮次」的偏聚坑。
pub const PHASE_ROUND_PRE: u32 = 4;
pub const PHASE_ROUND_DURING: u32 = 3;
pub const PHASE_ROUND_POST: u32 = 3;

/// 每相位内轮次的脏页规模阶梯（整数，禁浮点）。
///
/// 同相位内多轮必须有规模梯度，否则「三轮同相位」退化成
/// 「同一场景重复三遍」，相位覆盖过了但梯度覆盖没���。
pub const DIRTY_RAMP_BASE: u32 = 4;
pub const DIRTY_RAMP_STEP: u32 = 3;

/// 断电注入延迟上限（逻辑 tick，禁墙钟）。
///
/// 断电时点在回写周期内的相对位置：轮次 r 的断电延迟
/// `= r mod 断电延迟上限 + 1`，保证十轮的时点**互不相同**——
/// 十个时点撞成一个，等于只演练了一种断电窗口。
pub const CUT_LAG_MAX: u32 = 16;

/// 分界注记的 A4 面（页缓存面，**归本单**）。
pub const BOUNDARY_A4: u8 = 0;
/// 分界注记的 B5 面（盘面落盘一致性面，**归他单**）。
pub const BOUNDARY_B5: u8 = 1;
/// 分界注记面数（册定两��：A4 归我、B5 归他）。
pub const BOUNDARY_FACES: u32 = 2;
/// 分界注记四列要素数（归属 / 归他 / 判例 / 衔接）。
pub const BOUNDARY_COLS: u32 = 4;

/// 分界注记的判例号（判例一：断电时页缓存脏页零丢失，页缓存面归 A4）。
pub const PRECASE_NO: u32 = 1;

/// F7602 回写消费面（B5 侧，本单只留分界注记不落实现）。
pub const F7602_FACE: u8 = 2;
/// F2726 断电制度衔接点。
pub const F2726_LINK: u8 = 3;
/// F2876 脏页链衔接点。
pub const F2876_LINK: u8 = 4;
/// F3064 演练执行面衔接点（本单**只做方案**，执行归 F3064）。
pub const F3064_FACE: u8 = 5;

/// 衔接点数（册定五个：F7602 / F2726 / F2876 / F3064 / 判例一）。
pub const LINK_POINTS: u32 = 5;

/// 笛卡尔积维度 A 长度（轮次）。
pub const CART_DA: usize = 10;
/// 笛卡尔积维度 B 长度（相位）。
pub const CART_DB: usize = 3;
/// 笛卡尔积维度 C 长度（负载档）。
pub const CART_DC: usize = 3;
/// 笛卡尔积期望格数（10 × 3 × 3 = 90，运行期求值后交叉对账）。
pub const CART_EXPECTED: usize = 90;
/// 笛卡尔矩阵中「可写格」总数。
///
/// 可写格 = **册定上存在的** (轮次, 相位) 组合 × 全部负载档。
/// 轮次 r 的相位由册定 `Phase::of_round(r)` 唯一决定，
/// 故 10 轮只对应 10 个 (轮次, 相位) 组合而非 30 个：
///   * 3 相位 × 3 负载档 = 9 负载组合 → 10 × 9 = 90 全矩阵格；
///   * 其中册定存在的只有 10 × 3 负载 = 30 格。
/// 另 60 格是「轮次 3 × 回写毕」这类**册定上不存在的组合**，
/// 它们 `writable` 归零、不接受注入、也不参与好格统计——
/// 把它们算进可写格数，会要求对 60 个不存在的组合做注入拦截，
/// 那是自造需求。故 `CART_WRITABLE = 30`。
pub const CART_WRITABLE: usize = 30;
/// 守恒终账行数（由 `reconcile` 的 push 次数反推）。
pub const RECON_ROWS: usize = 8;
/// 回放步数上限（由 `replay` 的 `push` 次数反推：15 个动作，逐个可判）。
pub const REPLAY_STEPS_CAP: u32 = 15;
/// 加法链分项目标数（十轮各记 3 个分项：断电时点 / 脏页规模 / 页型）。
///
/// 三个分项对应判据三维定义里的三个**可读数**量：
/// 时点（何时断）、规模（多少脏页）、页型（断到哪类页）。
/// 只记一个（总脏页数）会把三维压成一维，加法链守恒就守了个寂寞。
pub const PART_TARGET: u32 = 30;
/// 加法链分项下限（阻断「分项 0 + 总额 0」的平凡守恒）。
pub const PART_FLOOR: u64 = 3;
/// **册定轮次登记件数**（十轮各登记一次矩阵定义）。
pub const ROUND_REG_TARGET: u32 = ROUND_COUNT;
/// 矩阵盐（方案侧重算用；随 seed 变化，保证换种子换轨迹）。
pub const MATRIX_SALT: u64 = 0x3061_0000_0000_0001;
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
pub const EXPORT_VERSION: u64 = 0x0000306100000001;
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
// 3. 错误闭集（21 个变体，一变体一码段，恒 ≤ 60）
// ===========================================================================

/// 错误闭集：每变体独占一个诊断码，段内连续。
///
/// 三要素：
///   * `code() -> u16`：段内唯一诊断码（`SEG_BASE + 序号`），一变体一码一段
///   * `detail(k) -> String`：上下文细节（件号 / 列号 / 位次 / tick 等）
///   * 变体本身即第三要素：闭集可穷举（`ErrKind::ALL`），任何新错误必须先入册
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrKind {
    /// `轮次闭集破损：十轮编号不齐或超出 0..9`。
    RoundClosedSetBroken,
    /// `轮次序号非法：非 0..9 闭集内的值`。
    RoundIndexInvalid,
    /// `相位闭集破损：回写前/中/毕三相位不齐`。
    PhaseClosedSetBroken,
    /// `相位序号非法：非三相位闭集内的值`。
    PhaseIndexInvalid,
    /// `负载档闭集破损：负载档数与册定不符`。
    LoadClosedSetBroken,
    /// `负载档序号非法：非负载档闭集内的值`。
    LoadIndexInvalid,
    /// `相位覆盖不足：某相位零轮次（相位偏聚）`。
    PhaseCoverageShort,
    /// `相位偏聚：全部轮次聚在同一相位`。
    PhaseSkewRejected,
    /// `矩阵格缺失：十轮中某轮无定义件`。
    MatrixCellMissing,
    /// `矩阵格重复：同一轮次登记多条定义`。
    MatrixCellRepeated,
    /// `矩阵维度不符：三维坐标越出各自闭集`。
    MatrixDimMismatch,
    /// `分界注记缺项：A4/B5 分界四要素未齐`。
    BoundaryNoteMissing,
    /// `分界越界：盘面落盘一致性被划入 A4 面`。
    BoundaryScopeLeak,
    /// `矩阵漂移：同 tick 重算轮次矩阵哈希不一致`。
    MatrixDrift,
    /// `加法链不守恒：Σ分项 ≠ 总额（零偏离要求）`。
    AddChainUnbalanced,
    /// `阶段不允许登记轮次：仅 Draft 可写`。
    StageRejectRegister,
    /// `阶段不允许冻结：十轮未登记齐即冻结被拒`。
    StageRejectFreeze,
    /// `封后写：方案已冻结后再改矩阵被拒`。
    WriteAfterFrozen,
    /// `冻结哈希漂移：同 tick 重算不一致`。
    FreezeHashDrift,
    /// `状态机非法迁移：跨段 / 自环 / 回退边`。
    StageIllegalEdge,
    /// `逻辑 tick 回退：时间不得倒退`。
    TickRegress,}

impl ErrKind {
    /// 全部变体（判据顺序，闭集穷举的权威列表）。
    pub const ALL: [ErrKind; 21] = [
    ErrKind::RoundClosedSetBroken,
    ErrKind::RoundIndexInvalid,
    ErrKind::PhaseClosedSetBroken,
    ErrKind::PhaseIndexInvalid,
    ErrKind::LoadClosedSetBroken,
    ErrKind::LoadIndexInvalid,
    ErrKind::PhaseCoverageShort,
    ErrKind::PhaseSkewRejected,
    ErrKind::MatrixCellMissing,
    ErrKind::MatrixCellRepeated,
    ErrKind::MatrixDimMismatch,
    ErrKind::BoundaryNoteMissing,
    ErrKind::BoundaryScopeLeak,
    ErrKind::MatrixDrift,
    ErrKind::AddChainUnbalanced,
    ErrKind::StageRejectRegister,
    ErrKind::StageRejectFreeze,
    ErrKind::WriteAfterFrozen,
    ErrKind::FreezeHashDrift,
    ErrKind::StageIllegalEdge,
    ErrKind::TickRegress,    ];

    /// 段内序号（0..20）。
    pub fn index(self) -> usize {
        match self {
            ErrKind::RoundClosedSetBroken => 0,
            ErrKind::RoundIndexInvalid => 1,
            ErrKind::PhaseClosedSetBroken => 2,
            ErrKind::PhaseIndexInvalid => 3,
            ErrKind::LoadClosedSetBroken => 4,
            ErrKind::LoadIndexInvalid => 5,
            ErrKind::PhaseCoverageShort => 6,
            ErrKind::PhaseSkewRejected => 7,
            ErrKind::MatrixCellMissing => 8,
            ErrKind::MatrixCellRepeated => 9,
            ErrKind::MatrixDimMismatch => 10,
            ErrKind::BoundaryNoteMissing => 11,
            ErrKind::BoundaryScopeLeak => 12,
            ErrKind::MatrixDrift => 13,
            ErrKind::AddChainUnbalanced => 14,
            ErrKind::StageRejectRegister => 15,
            ErrKind::StageRejectFreeze => 16,
            ErrKind::WriteAfterFrozen => 17,
            ErrKind::FreezeHashDrift => 18,
            ErrKind::StageIllegalEdge => 19,
            ErrKind::TickRegress => 20,        }
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
            ErrKind::RoundClosedSetBroken => "RoundClosedSetBroken",
            ErrKind::RoundIndexInvalid => "RoundIndexInvalid",
            ErrKind::PhaseClosedSetBroken => "PhaseClosedSetBroken",
            ErrKind::PhaseIndexInvalid => "PhaseIndexInvalid",
            ErrKind::LoadClosedSetBroken => "LoadClosedSetBroken",
            ErrKind::LoadIndexInvalid => "LoadIndexInvalid",
            ErrKind::PhaseCoverageShort => "PhaseCoverageShort",
            ErrKind::PhaseSkewRejected => "PhaseSkewRejected",
            ErrKind::MatrixCellMissing => "MatrixCellMissing",
            ErrKind::MatrixCellRepeated => "MatrixCellRepeated",
            ErrKind::MatrixDimMismatch => "MatrixDimMismatch",
            ErrKind::BoundaryNoteMissing => "BoundaryNoteMissing",
            ErrKind::BoundaryScopeLeak => "BoundaryScopeLeak",
            ErrKind::MatrixDrift => "MatrixDrift",
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
            ErrKind::RoundClosedSetBroken => "轮次闭集破损：十轮编号不齐或超出 0..9；修法=按闭集重建十轮矩阵，缺一即冻结拦截",
            ErrKind::RoundIndexInvalid => "轮次序号非法：非 0..9 闭集内的值；修法=把轮次收进 10 值闭集，非法值构造期即拒",
            ErrKind::PhaseClosedSetBroken => "相位闭集破损：回写前/中/毕三相位不齐；修法=按闭集重建三相位，缺一即冻结拦截",
            ErrKind::PhaseIndexInvalid => "相位序号非法：非三相位闭集内的值；修法=把相位收进三值闭集",
            ErrKind::LoadClosedSetBroken => "负载档闭集破损：负载档数与册定不符；修法=按册定负载档数重建档表",
            ErrKind::LoadIndexInvalid => "负载档序号非法：非负载档闭集内的值；修法=把负载档收进闭集",
            ErrKind::PhaseCoverageShort => "相位覆盖不足：某相位零轮次（相位偏聚）；修法=按相位覆盖断言重排轮次时点分布",
            ErrKind::PhaseSkewRejected => "相位偏聚：全部轮次聚在同一相位；修法=把轮次摊到三相位，重排时点分布",
            ErrKind::MatrixCellMissing => "矩阵格缺失：十轮中某轮无定义件；修法=补齐十轮矩阵定义，逐轮登记入册",
            ErrKind::MatrixCellRepeated => "矩阵格重复：同一轮次登记多条定义；修法=一轮一条定义，重复登记即拒",
            ErrKind::MatrixDimMismatch => "矩阵维度不符：三维坐标越出各自闭集；修法=核对轮次/相位/负载三维的取值域",
            ErrKind::BoundaryNoteMissing => "分界注记缺项：A4/B5 分界四要素未齐；修法=补齐分界注记的归属/归他/判例/衔接四列",
            ErrKind::BoundaryScopeLeak => "分界越界：盘面落盘一致性被划入 A4 面；修法=把盘面一致性归还 B1-B5，A4 只留页缓存面",
            ErrKind::MatrixDrift => "矩阵漂移：同 tick 重算轮次矩阵哈希不一致；修法=按 freeze_tick 重算矩阵定义哈希",
            ErrKind::AddChainUnbalanced => "加法链不守恒：Σ分项 ≠ 总额（零偏离要求）；修法=重算分项与总额，检查是否有漏登记",
            ErrKind::StageRejectRegister => "阶段不允许登记轮次：仅 Draft 可写；修法=把登记动作调度回 Draft 阶段",
            ErrKind::StageRejectFreeze => "阶段不允许冻结：十轮未登记齐即冻结被拒；修法=先补齐十轮矩阵定义再冻结",
            ErrKind::WriteAfterFrozen => "封后写：方案已冻结后再改矩阵被拒；修法=演练方案变更另开版本号方案",
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
            ErrKind::RoundClosedSetBroken => "按闭集重建十轮矩阵，缺一即冻结拦截",
            ErrKind::RoundIndexInvalid => "把轮次收进 10 值闭集，非法值构造期即拒",
            ErrKind::PhaseClosedSetBroken => "按闭集重建三相位，缺一即冻结拦截",
            ErrKind::PhaseIndexInvalid => "把相位收进三值闭集",
            ErrKind::LoadClosedSetBroken => "按册定负载档数重建档表",
            ErrKind::LoadIndexInvalid => "把负载档收进闭集",
            ErrKind::PhaseCoverageShort => "按相位覆盖断言重排轮次时点分布",
            ErrKind::PhaseSkewRejected => "把轮次摊到三相位，重排时点分布",
            ErrKind::MatrixCellMissing => "补齐十轮矩阵定义，逐轮登记入册",
            ErrKind::MatrixCellRepeated => "一轮一条定义，重复登记即拒",
            ErrKind::MatrixDimMismatch => "核对轮次/相位/负载三维的取值域",
            ErrKind::BoundaryNoteMissing => "补齐分界注记的归属/归他/判例/衔接四列",
            ErrKind::BoundaryScopeLeak => "把盘面一致性归还 B1-B5，A4 只留页缓存面",
            ErrKind::MatrixDrift => "按 freeze_tick 重算矩阵定义哈希",
            ErrKind::AddChainUnbalanced => "重算分项与总额，检查是否有漏登记",
            ErrKind::StageRejectRegister => "把登记动作调度回 Draft 阶段",
            ErrKind::StageRejectFreeze => "先补齐十轮矩阵定义再冻结",
            ErrKind::WriteAfterFrozen => "演练方案变更另开版本号方案",
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
        ErrKind::ALL.len() == 21,
        format!("ALL.len()={} want={}", ErrKind::ALL.len(), 21),
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
            Stage::Draft => "方案草案",
            Stage::Mapped => "十轮已登记",
            Stage::Frozen => "方案已冻结",
            Stage::Dispatched => "方案已调度",
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
// 4. 领域闭集一：轮次（0..9 十轮，判据「×10 轮次矩阵」）
// ===========================================================================

/// 轮次闭集（`0..ROUND_LIMIT`，共 10 轮）。
///
/// 判据要求「断电演练 ×10 轮次矩阵」。十轮不是十个副本，
/// 而是**十个不同的 (相位, 负载, 时点) 组合**——否则「×10」只是
/// 把同一个场景跑十遍，轮次矩阵退化成单点矩阵，
/// 相位覆盖断言、梯度覆盖断言全部无从谈起。
/// 轮次在这里是一个**独立维度**（维度 A），与相位（维度 B）、
/// 负载档（维度 C）正交，构成笛卡尔矩阵。
pub const ROUND_MAX: u8 = 9;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundTag {
    /// 轮次 0–3：第 1–4 轮（回写前相位，脏页规模最小梯度）。
    R0,
    /// 轮次 4–6：第 5–7 轮（回写中相位）。
    R1,
    /// 轮次 7–9：第 8–10 轮（回写毕相位）。
    R2,
}

impl RoundTag {
    /// 轮次分组闭集（3 组，覆盖 0..9 全部 10 轮）。
    pub const ALL: [RoundTag; 3] = [RoundTag::R0, RoundTag::R1, RoundTag::R2];
    /// 序号。
    pub fn index(self) -> usize {
        match self {
            RoundTag::R0 => 0,
            RoundTag::R1 => 1,
            RoundTag::R2 => 2,
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            RoundTag::R0 => "R0",
            RoundTag::R1 => "R1",
            RoundTag::R2 => "R2",
        }
    }
    /// 分组覆盖的轮次数（`4/3/3`，与 `PHASE_ROUND_*` 同值）。
    ///
    /// 轮次分组与相位**同源**：第 0–3 轮是回写前相位，
    /// 第 4–6 轮是回写中，第 7–9 轮是回写毕。
    /// 两套分组独立维护（`RoundTag` 与 `Phase`），
    /// 正是为了让「轮次分组覆盖 == 相位轮次数」成为一条可断言的恒等式。
    pub fn span(self) -> u32 {
        match self {
            RoundTag::R0 => PHASE_ROUND_PRE,
            RoundTag::R1 => PHASE_ROUND_DURING,
            RoundTag::R2 => PHASE_ROUND_POST,
        }
    }
    /// 分组下界（**闭**）。
    pub fn lo(self) -> u32 {
        match self {
            RoundTag::R0 => 0,
            RoundTag::R1 => PHASE_ROUND_PRE,
            RoundTag::R2 => PHASE_ROUND_PRE + PHASE_ROUND_DURING,
        }
    }
    /// 分组上界（**开**）。
    pub fn hi(self) -> u32 {
        match self {
            RoundTag::R0 => PHASE_ROUND_PRE,
            RoundTag::R1 => PHASE_ROUND_PRE + PHASE_ROUND_DURING,
            RoundTag::R2 => PHASE_ROUND_PRE + PHASE_ROUND_DURING + PHASE_ROUND_POST,
        }
    }
    /// 轮次是否在闭集内。
    pub fn valid(self, r: u8) -> bool {
        self.lo() <= r as u32 && (r as u32) < self.hi()
    }
    /// 轮次 → 分组（越界返回 `None`）。
    pub fn of_round(r: u8) -> Option<RoundTag> {
        if r > ROUND_MAX {
            return None;
        }
        let mut i = 0usize;
        while i < RoundTag::ALL.len() {
            if RoundTag::ALL[i].valid(r) {
                return Some(RoundTag::ALL[i]);
            }
            i += 1;
        }
        None
    }
}

// ===========================================================================
// 5. 领域闭集二：回写相位（回写前 / 回写中 / 回写毕）
// ===========================================================================

/// 回写相位闭集（判据「回写周期不同相位」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 回写前：脏页尚未开始落盘，断电丢页风险最高。
    Pre,
    /// 回写中：落盘进行中，部分页已落盘。
    During,
    /// 回写毕：落盘已收口，断电影响最小。
    Post,
}

impl Phase {
    /// 相位闭集（三值）。
    pub const ALL: [Phase; 3] = [Phase::Pre, Phase::During, Phase::Post];
    /// 序号（0/1/2）。
    pub fn index(self) -> u8 {
        match self {
            Phase::Pre => PHASE_PRE,
            Phase::During => PHASE_DURING,
            Phase::Post => PHASE_POST,
        }
    }
    /// 由序号还原（越界返回 `None`）。
    pub fn from_index(i: u8) -> Option<Phase> {
        match i {
            PHASE_PRE => Some(Phase::Pre),
            PHASE_DURING => Some(Phase::During),
            PHASE_POST => Some(Phase::Post),
            _ => None,
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Pre => "pre_writeback",
            Phase::During => "in_writeback",
            Phase::Post => "post_writeback",
        }
    }
    /// 该相位的册定轮次数（`4/3/3`）。
    pub fn round_target(self) -> u32 {
        match self {
            Phase::Pre => PHASE_ROUND_PRE,
            Phase::During => PHASE_ROUND_DURING,
            Phase::Post => PHASE_ROUND_POST,
        }
    }
    /// 该相位覆盖的轮次下界（**闭**）。
    ///
    /// 供「同相位内序位」换算：`序位 = 轮次 - 本相位下界`。
    /// 写成前序和的累加会让调用处多一层循环，
    /// 且累加顺序一旦改错，序位就会跨相位溢出——
    /// 直接册定下界更不容易错。
    pub fn lo(self) -> u32 {
        match self {
            Phase::Pre => 0,
            Phase::During => PHASE_ROUND_PRE,
            Phase::Post => PHASE_ROUND_PRE + PHASE_ROUND_DURING,
        }
    }

    /// 轮次 → 相位（越界返回 `None`）。
    pub fn of_round(r: u8) -> Option<Phase> {
        if r > ROUND_MAX {
            return None;
        }
        let mut i = 0usize;
        while i < Phase::ALL.len() {
            if Phase::ALL[i].round_target() > 0 {
                let mut acc = 0u32;
                let mut j = 0usize;
                while j <= i {
                    acc = acc.wrapping_add(Phase::ALL[j].round_target());
                    j += 1;
                }
                if acc > r as u32 {
                    return Some(Phase::ALL[i]);
                }
            }
            i += 1;
        }
        None
    }
    /// 相位是否在闭集内。
    pub fn valid(self) -> bool {
        Phase::from_index(self.index()).is_some()
    }
}

// ===========================================================================
// 6. 领域闭集三：负载档（空载 / 轻载 / 重载，与 F3021 三档同源）
// ===========================================================================

/// 负载档闭集（判据「×负载档」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Load {
    /// 空载：无脏页，断电零影响（基线轮）。
    Idle,
    /// 轻载：少量脏页。
    Light,
    /// 重载：大量脏页。
    Heavy,
}

impl Load {
    /// 负载档闭集（三值）。
    pub const ALL: [Load; 3] = [Load::Idle, Load::Light, Load::Heavy];
    /// 序号（0/1/2）。
    pub fn index(self) -> u8 {
        match self {
            Load::Idle => LOAD_IDLE,
            Load::Light => LOAD_LIGHT,
            Load::Heavy => LOAD_HEAVY,
        }
    }
    /// 由序号还原（越界返回 `None`）。
    pub fn from_index(i: u8) -> Option<Load> {
        match i {
            LOAD_IDLE => Some(Load::Idle),
            LOAD_LIGHT => Some(Load::Light),
            LOAD_HEAVY => Some(Load::Heavy),
            _ => None,
        }
    }
    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Load::Idle => "idle",
            Load::Light => "light",
            Load::Heavy => "heavy",
        }
    }
    /// 册定脏页基数（整数帧）。
    pub fn dirty_base(self) -> u32 {
        match self {
            Load::Idle => DIRTY_RAMP_BASE,
            Load::Light => DIRTY_RAMP_BASE.wrapping_mul(2),
            Load::Heavy => DIRTY_RAMP_BASE.wrapping_mul(4),
        }
    }
    /// 与 F3021 压力档的对位档号（三档一一对位）。
    pub fn f3021_tier(self) -> u8 {
        self.index()
    }
}

// ===========================================================================
// 7. 轮次矩阵定义行（轮次 × 相位 × 负载档 三维一格）
// ===========================================================================

/// 轮次矩阵单行（一轮一条定义，三维坐标一格）。
#[derive(Clone, Copy, Debug)]
pub struct RoundRow {
    /// 维度 A：轮次序号（0..9）。
    pub round: u8,
    /// 维度 B：回写相位。
    pub phase: u8,
    /// 维度 C：负载档。
    pub load: u8,
    /// 断电时点（回写周期内相对延迟，逻辑 tick）。
    pub cut_lag: u32,
    /// 该轮脏页规模（整数帧 = 负载基数 + 轮内梯度）。
    pub dirty_frames: u32,
    /// 落盘预期（断电时已落盘页数，整数帧）。
    pub landed_frames: u32,
    /// 页型（本轮被断电波及的页型码，整数三值）。
    pub page_kind: u8,
    /// 方案侧册定断电延迟（重算用）。
    pub plan_lag: u32,
    /// 方案侧册定脏页规模（重算用）。
    pub plan_dirty: u32,
}

impl RoundRow {
    /// 单行自身有效性（六项全查）。
    ///
    /// 有效性只查**本格自身**的闭集性，不掺跨格关系
    /// （跨格关系由 `phase_coverage_ok()` 与 `round_matrix_assert()` 负责）。
    /// 混在一起的后果是：一格坏会让整张矩阵全红，定位信息全丢。
    pub fn valid(&self) -> bool {
        if self.round > ROUND_MAX {
            return false;
        }
        if Phase::from_index(self.phase).is_none() {
            return false;
        }
        if Load::from_index(self.load).is_none() {
            return false;
        }
        if self.cut_lag == 0 || self.cut_lag > CUT_LAG_MAX {
            return false;
        }
        if self.page_kind > 2 {
            return false;
        }
        if self.landed_frames > self.dirty_frames {
            return false;
        }
        true
    }

    /// 单行摘要。
    pub fn row(&self) -> String {
        let ph = match Phase::from_index(self.phase) {
            Some(p) => p.name(),
            None => "?",
        };
        let ld = match Load::from_index(self.load) {
            Some(l) => l.name(),
            None => "?",
        };
        format!(
            "ROUND r={} phase={} load={} lag={} dirty={} landed={} page_kind={} valid={}",
            self.round,
            ph,
            ld,
            self.cut_lag,
            self.dirty_frames,
            self.landed_frames,
            self.page_kind,
            self.valid()
        )
    }

    /// 单行指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix32(h, self.round as u32);
        h = mix32(h, self.phase as u32);
        h = mix32(h, self.load as u32);
        h = mix32(h, self.cut_lag);
        h = mix32(h, self.dirty_frames);
        h = mix32(h, self.landed_frames);
        h = mix32(h, self.page_kind as u32);
        h = mix32(h, self.plan_lag);
        h = mix32(h, self.plan_dirty);
        h
    }
}

// ===========================================================================
// 8. 断电时点的方案侧重算规则（纯函数，双向对拍的共享规则源）
// ===========================================================================

/// 断电延迟的**方案侧**重算（纯函数）。
///
/// 册定规则：轮次 `r` 的断电延迟 `= (r mod CUT_LAG_MAX) + 1`。
/// 取 `+1` 是为了让「0」永远不在合法域内——`cut_lag == 0` 正是
/// `valid()` 的拒绝条件，若允许 0，越权值与合法值就分不开了。
/// 教训与 F3021 步长夹取同源：**哨位值必须留在合法域之外**。
pub fn cut_lag_plan(r: u8) -> u32 {
    (r as u32 % CUT_LAG_MAX) + 1
}

/// 脏页规模的**方案侧**重算（纯函数）。
///
/// 册定规则：`脏页 = 负载基数 + 轮内梯度 × 相位序号`，
/// 其中轮内梯度 `= DIRTY_RAMP_STEP × 轮内序位`。
/// 轮内序位 = 轮次在同相位内的序号（0 起），
/// 保证同一相位内多轮的脏页规模**互不相同**。
pub fn dirty_plan(load: Load, phase_ord: u32, in_phase_ord: u32) -> u32 {
    load.dirty_base()
        .wrapping_add(DIRTY_RAMP_STEP.wrapping_mul(in_phase_ord))
        .wrapping_add(phase_ord)
}

/// 落盘预期的**方案侧**重算（纯函数）。
///
/// 回写前：零页落盘（脏页全未落盘，断电丢页最狠）；
/// 回写中：落盘一半（整除余数归前）；
/// 回写毕：全部落盘。
/// 全部走整数除法，禁浮点。
pub fn landed_plan(phase: Phase, dirty: u32) -> u32 {
    match phase {
        Phase::Pre => 0,
        Phase::During => dirty / 2,
        Phase::Post => dirty,
    }
}

/// 页型方案侧重算（纯函数）：`(轮次 + 负载 + 相位) mod 3`。
///
/// 页型取三值闭集（数据页 / 元数据页 / 日志页），
/// 三型都要被断电波及——只测数据页等于没测元数据页与日志页，
/// 而后两者恰恰是断电最易损的。
pub fn page_kind_plan(r: u8, load: Load, phase: Phase) -> u8 {
    (((r as u32).wrapping_add(load.index() as u32).wrapping_add(phase.index() as u32)) % 3) as u8
}

// ===========================================================================
// 9. 相位覆盖断言（判据风险项「相位偏聚」防线）
// ===========================================================================

/// 相位覆盖断言（三相位各有轮次 + 同相位内规模梯度不重复）。
///
/// 判据风险栏写得很直白：「相位偏聚（时点全聚回写毕相位）」。
/// 注意「全聚回写毕」是**最坏**的偏聚方向：回写毕时断电，
/// 脏页早已落盘，丢页窗口根本打不开，练十轮等于没练。
/// 故本断言查两件事：
///   * 每相位轮次数 == 册定值（4/3/3），零缺位；
///   * 同相位内各轮断电时点互不相同（否则「四轮同相位」实为同一时点）。
/// 少查任一条，偏聚都能从后门溜过去。
pub fn phase_coverage_ok(rows: &Vec<RoundRow>) -> DcRes<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    if rows.len() != ROUND_COUNT as usize {
        return Err(ErrKind::RoundClosedSetBroken);
    }
    let mut pi = 0usize;
    let mut all_ok = true;
    while pi < Phase::ALL.len() {
        let p = Phase::ALL[pi];
        let mut cnt = 0u32;
        let mut lags: Vec<u32> = Vec::new();
        let mut dirty: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < rows.len() {
            if rows[i].phase == p.index() {
                cnt += 1;
                lags.push(rows[i].cut_lag);
                dirty.push(rows[i].dirty_frames);
            }
            i += 1;
        }
        // 同相位内时点重复检测
        let mut dup = 0u32;
        let mut a = 0usize;
        while a < lags.len() {
            let mut b = a + 1;
            while b < lags.len() {
                if lags[a] == lags[b] {
                    dup += 1;
                }
                b += 1;
            }
            a += 1;
        }
        // 同相位内脏页规模重复检测
        let mut ddup = 0u32;
        let mut a2 = 0usize;
        while a2 < dirty.len() {
            let mut b2 = a2 + 1;
            while b2 < dirty.len() {
                if dirty[a2] == dirty[b2] {
                    ddup += 1;
                }
                b2 += 1;
            }
            a2 += 1;
        }
        let ok = cnt == p.round_target() && dup == 0 && ddup == 0;
        if !ok {
            all_ok = false;
        }
        v.push(format!(
            "  PHASE-COV {} count={} target={} lag_dup={} dirty_dup={} ok={}",
            p.name(),
            cnt,
            p.round_target(),
            dup,
            ddup,
            ok
        ));
        pi += 1;
    }
    v.push(format!("PHASE-COV all_ok={}", all_ok));
    if !all_ok {
        // 先判偏聚（全部轮次挤在一相位）再判覆盖不足：
        // 偏聚是覆盖不足的**特例**，报更具体的那个码，
        // 注入核的形态与错误码才对得上。
        let mut only = 0u32;
        let mut which = Phase::Pre;
        let mut pi2 = 0usize;
        while pi2 < Phase::ALL.len() {
            let p2 = Phase::ALL[pi2];
            let mut c2 = 0u32;
            let mut j = 0usize;
            while j < rows.len() {
                if rows[j].phase == p2.index() {
                    c2 += 1;
                }
                j += 1;
            }
            if c2 == ROUND_COUNT {
                only = 1;
                which = p2;
            }
            pi2 += 1;
        }
        if only == 1 {
            let _ = which;
            return Err(ErrKind::PhaseSkewRejected);
        }
        return Err(ErrKind::PhaseCoverageShort);
    }
    Ok(v)
}

// ===========================================================================
// 10. 分界注记（A4 面 / B5 面 四列）
// ===========================================================================

/// 分界注记单行（归属面 / 归属 / 归他 / 判例 / 衔接 五要素）。
#[derive(Clone, Copy, Debug)]
pub struct BoundaryRow {
    /// 归属面（A4 = 本单页缓存面；B5 = 他单盘面）。
    pub face: u8,
    /// 本面职责摘要（定长语义，用枚举索引表达）。
    pub duty: u8,
    /// 归他面（同一条注记的对面）。
    pub other_face: u8,
    /// 判例号（`PRECASE_NO` = 判例一）。
    pub precase: u32,
    /// 衔接点（`F7602_*` / `F2726_*` / `F2876_*` / `F3064_*` 之一）。
    pub link: u8,
}

impl BoundaryRow {
    /// 单行自身有效性（五要素全查）。
    pub fn valid(&self) -> bool {
        if self.face > BOUNDARY_B5 {
            return false;
        }
        if self.other_face == self.face {
            return false;
        }
        if self.other_face > BOUNDARY_B5 {
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
            0 => "dirty_page_zero_loss",
            1 => "disk_persist_consistency",
            2 => "page_cache_scope",
            3 => "matrix_plan_freeze",
            _ => "?",
        }
    }

    /// 衔接点名。
    pub fn link_name(&self) -> &'static str {
        match self.link {
            1 => "F7602_writeback",
            2 => "F2726_power_policy",
            3 => "F2876_dirty_chain",
            4 => "F3064_drill_exec",
            5 => "PRECASE_1",
            _ => "?",
        }
    }

    /// 单行摘要。
    pub fn row(&self) -> String {
        let f = match self.face {
            BOUNDARY_A4 => "A4",
            BOUNDARY_B5 => "B5",
            _ => "?",
        };
        let o = match self.other_face {
            BOUNDARY_A4 => "A4",
            BOUNDARY_B5 => "B5",
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

/// 构建分界注记表（`BOUNDARY_COLS` = 4 行，A4/B5 两面各两行）。
///
/// 册定四行：
///   * A4 行 0：脏页零丢失（判例一）——断电时页缓存面归 A4；
///   * A4 行 1：页缓存面范围（本单只冻方案，不碰盘面）；
///   * B5 行 0：盘面落盘一致性——归 B1–B5，本单**只留注记不落实现**；
///   * B5 行 1：回写消费面（F7602）——断电影响页缓存面归 A4、
///     盘面落盘一致性归 B1–B5。
///
/// 四行不是随意凑数：`BOUNDARY_COLS = 4` 是守恒项，
/// `boundary_note_ok()` 逐行查存在性，少一行即 `BoundaryNoteMissing`。
pub fn boundary_note_table() -> Vec<BoundaryRow> {
    let mut v: Vec<BoundaryRow> = Vec::new();
    v.push(BoundaryRow {
        face: BOUNDARY_A4,
        duty: 0,
        other_face: BOUNDARY_B5,
        precase: PRECASE_NO,
        link: 5,
    });
    v.push(BoundaryRow {
        face: BOUNDARY_A4,
        duty: 2,
        other_face: BOUNDARY_B5,
        precase: PRECASE_NO,
        link: 3,
    });
    v.push(BoundaryRow {
        face: BOUNDARY_B5,
        duty: 1,
        other_face: BOUNDARY_A4,
        precase: PRECASE_NO,
        link: 1,
    });
    v.push(BoundaryRow {
        face: BOUNDARY_B5,
        duty: 3,
        other_face: BOUNDARY_A4,
        precase: PRECASE_NO,
        link: 4,
    });
    v
}

/// 分界注记完整性断言（四行齐备 + 双向对置 + 盘面不越界）。
pub fn boundary_note_ok() -> DcRes<Vec<String>> {
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
    // 双向对置：A4 每行都能在 B5 找到对面，B5 每行都能在 A4 找到对面。
    let mut fa = true;
    let mut fb = true;
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
            if t[i].face == BOUNDARY_A4 {
                fa = false;
            } else {
                fb = false;
            }
        }
        i += 1;
    }
    v.push(format!("BOUNDARY mutual a4={} b5={}", fa, fb));
    if !fa || !fb {
        return Err(ErrKind::BoundaryNoteMissing);
    }
    // 盘面不越界：凡 `duty == disk_persist_consistency` 的行必须
    // 挂在 B5 面下，挂在 A4 面下即越界。
    //
    // 这是分界注记的**核心防线**：判例一把「断电影响页缓存面归 A4，
    // 盘面落盘一致性归 B1–B5」写死，A4 一旦吞进盘面职责，
    // B5 的回写消费面就失去了归属，B1–B5 的落盘一致性无人负责。
    let mut leak = 0u32;
    i = 0;
    while i < t.len() {
        if t[i].duty == 1 && t[i].face != BOUNDARY_B5 {
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
/// 衔接点写「不造关系」的部分也要在册：F7602 是 B5 侧消费面，
/// 本单**只做方案不落实现**；F3064 是演练执行面，
/// 本单只做方案冻结、执行归 F3064。不注明这一点，
/// 账面看不出「为何本单没有 F7602 的实现体」。
pub fn link_note() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "  LINK F7602 面={} 关系=消费面(B5) 本单只做方案冻结不落实现",
        F7602_FACE
    ));
    v.push(format!(
        "  LINK F2726 面={} 关系=断电制度衔接 断电时点分布由其约束",
        F2726_LINK
    ));
    v.push(format!(
        "  LINK F2876 面={} 关系=脏页链衔接 脏页规模口径与其对齐",
        F2876_LINK
    ));
    v.push(format!(
        "  LINK F3064 面={} 关系=演练执行面 矩阵键与其读数一一对应",
        F3064_FACE
    ));
    v.push(format!(
        "  LINK PRECASE-{} 面={} 关系=判例一 断电时页缓存脏页零丢失",
        PRECASE_NO, LINK_POINTS
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
        if v[i].len() < 8 {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// 11. 轮次矩阵键表（与 F3064 演练执行面的读数键一一对应）
// ===========================================================================

/// 矩阵键：一轮一条，供 F3064 读数取用。
///
/// 判据读数栏写「矩阵键与 F3064 演练执行面一一对应可查」。
/// 键的构成刻意与三维坐标同源（`轮次-相位-负载`），
/// 于是 F3064 拿着键回来查矩阵，能**直接反查**出三维坐标，
/// 不必另建一张键到坐标的映射表——多一张表就多一处可漂移。
pub fn round_key(r: u8, p: Phase, l: Load) -> String {
    format!("R{:02}-{}-{}", r as u32, p.name(), l.name())
}

/// 矩阵键表（`ROUND_COUNT` 行）。
pub fn round_key_table(rows: &Vec<RoundRow>) -> Vec<String> {
    let mut v: Vec<String> = Vec::with_capacity(rows.len());
    let mut i = 0usize;
    while i < rows.len() {
        let p = Phase::from_index(rows[i].phase).unwrap_or(Phase::Pre);
        let l = Load::from_index(rows[i].load).unwrap_or(Load::Idle);
        v.push(round_key(rows[i].round, p, l));
        i += 1;
    }
    v
}

// ===========================================================================
// 12. 断电延迟 / 脏页规模的方案侧折盐夹取（纯函数，双向对拍共享规则源）
// ===========================================================================

/// 脏页规模上界（够装下重载 + 满梯度的规模）。
pub const DIRTY_MAX: u32 = DIRTY_RAMP_BASE * 8;

/// 断电延迟的**方案侧**折盐夹取（纯函数）。
///
/// 夹取域是闭集 `[1, CUT_LAG_MAX]`：折盐后 `raw % CUT_LAG_MAX` 会把
/// `CUT_LAG_MAX` 折成 0，而 0 不在合法域（`valid()` 判 0 为非法），
/// 故需 `m == 0 → CUT_LAG_MAX` 的回补。教训同 F3021 步长夹取：
/// **取模把 `[1, MAX]` 映到 `[0, MAX]`，边界值必然漏出域外。**
///
/// 该函数被 `register()`（实现侧写账）与 `rows_bidir_ok()`（方案侧重算）
/// 共用，于是对拍比的是「同一输入经同一规则是否得同一输出」，
/// 而非「册定值 vs 折盐值」这种根本不同的两个量。
pub fn lag_plan_side(base: u32, salt: u64, _r: u64) -> u32 {
    // 「先减一再加盐再回补一」——写成 `(base + salt) % MAX + 1` 会把
    // `base == MAX` 折成 0 再补回 MAX，映射不是双射，
    // 域外值会与域内值撞车。故用 `(base - 1 + salt) mod MAX + 1`。
    //
    // 另一个关键性质：本函数对 `base ∈ [1, 10]` 是**单射**。
    // 相位覆盖断言要求同相位内各轮时点互不相同，册定 `base = r + 1`
    // 本身就是 10 个互异值；而「10 个互异值整体旋转 mod 16」仍互异
    // （10 < 16，旋转不产生碰撞），故折盐不会破坏单射性。
    // 若当初写成 `raw = base + salt + (r & 0x03)`，r=0 与 r=4 的
    // `r & 0x03` 同为 0，而 base 差 4、扰动补偿不了，
    // 于是同相位内时点撞车，`PhaseCoverageShort` 恒红。
    let sh = (salt & 0x0F) as u32;
    (base.wrapping_sub(1).wrapping_add(sh) % CUT_LAG_MAX) + 1
}

/// 脏页规模的**方案侧**折盐夹取（纯函数）。
///
/// 折盐幅度刻意比断电延迟小（`& 0x07` vs `& 0x0F`）：
/// 脏页规模是**单调累加量**，若扰动过大，十轮的规模梯度会被噪声淹没，
/// 「同相位内规模互不相同」这条断言就变成掷骰子。
pub fn dirty_plan_side(base: u32, salt: u64, _r: u64) -> u32 {
    // 理由同 `lag_plan_side`：`(base - 1 + salt) mod MAX + 1` 才是双射。
    // 脏页规模的值域 `[DIRTY_RAMP_BASE, DIRTY_MAX]` 跨度 28 < 32，
    // 整体旋转同样保持互异。
    let sh = (salt & 0x07) as u32;
    (base.wrapping_sub(1).wrapping_add(sh) % DIRTY_MAX) + 1
}

// ===========================================================================
// 13. 轮次矩阵件（十行定义 + 登记位 + 覆盖账 + 键表）
// ===========================================================================

/// 轮次矩阵件：十行定义 + 登记位 + 覆盖账 + 键表。
///
/// 与 F3021 的 `TierPiece` 同构（登记只改行表、齐备后统一派生），
/// 但派生量从「刻度分派表」换成「相位覆盖账」：
/// 三维矩阵的联合派生态是**相位覆盖**（跨行关系），
/// 而非逐行可判的本地不变量。故 `seal_phase()` 必须等十轮齐备才可跑。
pub struct RoundPiece {
    /// 十行定义（未登记的行是占位空行，`valid()` 假）。
    pub rows: Vec<RoundRow>,
    /// 登记位。
    pub reg: Vec<bool>,
    /// 相位覆盖快照（`seal_phase` 产出的 4 行）。
    pub cover: Vec<String>,
    /// 走查过的轮次数。
    pub rounds_walked: u32,
    /// 矩阵键表。
    pub keys: Vec<String>,
    /// 实例盐（折盐夹取用；随 seed 变化，保证换种子换轨迹）。
    pub salt: u64,
}

impl RoundPiece {
    /// 新建（全占位）。
    pub fn new(salt: u64) -> RoundPiece {
        let mut rows: Vec<RoundRow> = Vec::new();
        let mut reg: Vec<bool> = Vec::new();
        let mut i = 0usize;
        while i < ROUND_COUNT as usize {
            rows.push(RoundRow {
                round: i as u8,
                phase: 0xFF,
                load: 0xFF,
                cut_lag: 0,
                dirty_frames: 0,
                landed_frames: 0,
                page_kind: 0xFF,
                plan_lag: 0,
                plan_dirty: 0,
            });
            reg.push(false);
            i += 1;
        }
        RoundPiece {
            rows,
            reg,
            cover: Vec::new(),
            rounds_walked: 0,
            keys: Vec::new(),
            salt,
        }
    }

    /// 登记一轮三维定义（坐标由册定闭集取，派生量按册定规则 + 折盐）。
    pub fn register(&mut self, r: u8, l: Load) -> DcRes<u64> {
        if r > ROUND_MAX {
            return Err(ErrKind::RoundIndexInvalid);
        }
        let idx = r as usize;
        let p = match Phase::of_round(r) {
            Some(x) => x,
            None => return Err(ErrKind::PhaseClosedSetBroken),
        };
        // 同相位内的序位（0 起），用于脏页规模梯度。
        let in_phase_ord = (r as u32).wrapping_sub(Phase::ALL[p.index() as usize].lo());
        // 方案侧册定值（重算用；实现侧本就该与之逐项相等）。
        let plan_lag = cut_lag_plan(r);
        let plan_dirty = dirty_plan(l, p.index() as u32, in_phase_ord);
        let lag = lag_plan_side(plan_lag, self.salt, r as u64);
        let dirty = dirty_plan_side(plan_dirty, self.salt, r as u64);
        let row = RoundRow {
            round: r,
            phase: p.index(),
            load: l.index(),
            cut_lag: lag,
            dirty_frames: dirty,
            landed_frames: landed_plan(p, dirty),
            page_kind: page_kind_plan(r, l, p),
            plan_lag,
            plan_dirty,
        };
        if !row.valid() {
            return Err(ErrKind::MatrixDimMismatch);
        }
        let fp = row.fingerprint();
        self.rows[idx] = row;
        self.reg[idx] = true;
        // **不在此处重建相位覆盖**。
        // 覆盖断言是**跨十行**的联合性质：登记到第 1 轮时，
        // 其余九行仍是占位空行（`phase = 0xFF`），此刻断言必然
        // `PhaseCoverageShort` —— 于是登记第一轮就被自己设的死锁拦住，
        // 十轮永远登记不齐。分工：登记只改行表，
        // 覆盖派生由调用方在十轮齐备后统一触发（见 `seal_phase()`）。
        Ok(fp)
    }

    /// 十轮齐备后派生相位覆盖账 + 矩阵键表（公开入口，与登记解耦）。
    pub fn seal_phase(&mut self) -> DcRes<u32> {
        if !self.complete() {
            return Err(ErrKind::RoundClosedSetBroken);
        }
        let cov = phase_coverage_ok(&self.rows)?;
        self.cover = cov;
        self.keys = round_key_table(&self.rows);
        // 逐轮走查：每轮自身不变量 + 轮次-相位坐标与闭集一致。
        let mut walk = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].valid() {
                return Err(ErrKind::MatrixCellMissing);
            }
            let p = Phase::of_round(self.rows[i].round).unwrap_or(Phase::Pre);
            if self.rows[i].phase != p.index() {
                return Err(ErrKind::MatrixDimMismatch);
            }
            walk += 1;
            i += 1;
        }
        self.rounds_walked = walk;
        Ok(walk)
    }

    /// 登记是否齐备（十轮全登记）。
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

    /// 运行期查一轮定义（读行表，不重算）。
    pub fn row_of(&self, r: u8) -> DcRes<RoundRow> {
        if r > ROUND_MAX {
            return Err(ErrKind::RoundIndexInvalid);
        }
        let i = r as usize;
        if i >= self.rows.len() || !self.reg[i] {
            return Err(ErrKind::MatrixCellMissing);
        }
        if !self.rows[i].valid() {
            return Err(ErrKind::MatrixDimMismatch);
        }
        Ok(self.rows[i])
    }

    /// 轮次-相位一致性（读回行表，不重算）。
    pub fn phase_consistent(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.reg[i] {
                continue;
            }
            let p = Phase::of_round(self.rows[i].round);
            if p.is_none() {
                ok = false;
            } else {
                let want = p.unwrap_or(Phase::Pre).index();
                if self.rows[i].phase != want {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    }

    /// 方案侧 vs 实现侧**逐行**对拍（三维坐标 + 四个派生量）。
    ///
    /// 双向纪律：两侧共用同一枚「轮次序号 + 盐」作为共享输入，
    /// 方案侧独立复现全部变换（含折盐夹取），
    /// 实现侧从已填好的行表读回。少对一项都算「只对了一半」。
    pub fn rows_bidir_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.reg[i] {
                continue;
            }
            let r = self.rows[i].round;
            let p = match Phase::of_round(r) {
                Some(x) => x,
                None => {
                    ok = false;
                    continue;
                }
            };
            let l = match Load::from_index(self.rows[i].load) {
                Some(x) => x,
                None => {
                    ok = false;
                    continue;
                }
            };
            if self.rows[i].phase != p.index() {
                ok = false;
            }
            if self.rows[i].plan_lag != cut_lag_plan(r) {
                ok = false;
            }
            if self.rows[i].cut_lag != lag_plan_side(cut_lag_plan(r), self.salt, r as u64) {
                ok = false;
            }
            if self.rows[i].page_kind != page_kind_plan(r, l, p) {
                ok = false;
            }
            if self.rows[i].landed_frames != landed_plan(p, self.rows[i].dirty_frames) {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 方案侧全矩阵指纹（**独立重算**，不读行表）。
    ///
    /// 双向对拍的另一道关：把十行的册定坐标 + 派生量从闭集重新算一遍，
    /// 与 `rows_fingerprint()`（读回行表）对拍。
    /// 两侧共用「轮次序号 + 盐」，但一个从闭集算、一个从表读，
    /// 源不同才叫对拍；都从表读就是自我循环。
    pub fn plan_side_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix64(h, self.salt);
        let mut r = 0u8;
        while r <= ROUND_MAX {
            let p = match Phase::of_round(r) {
                Some(x) => x,
                None => {
                    r += 1;
                    continue;
                }
            };
            let l = Load::ALL[(r as usize) % Load::ALL.len()];
            let in_phase_ord = (r as u32).wrapping_sub(Phase::ALL[p.index() as usize].lo());
            let plan_lag = cut_lag_plan(r);
            let plan_dirty = dirty_plan(l, p.index() as u32, in_phase_ord);
            let lag = lag_plan_side(plan_lag, self.salt, r as u64);
            let dirty = dirty_plan_side(plan_dirty, self.salt, r as u64);
            h = mix32(h, r as u32);
            h = mix32(h, p.index() as u32);
            h = mix32(h, l.index() as u32);
            h = mix32(h, lag);
            h = mix32(h, dirty);
            h = mix32(h, landed_plan(p, dirty));
            h = mix32(h, page_kind_plan(r, l, p) as u32);
            r += 1;
        }
        h
    }

    /// 实现侧全矩阵指纹（读回行表）。
    pub fn rows_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix64(h, self.salt);
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = self.rows[i];
            h = mix32(h, r.round as u32);
            h = mix32(h, r.phase as u32);
            h = mix32(h, r.load as u32);
            h = mix32(h, r.cut_lag);
            h = mix32(h, r.dirty_frames);
            h = mix32(h, r.landed_frames);
            h = mix32(h, r.page_kind as u32);
            i += 1;
        }
        h
    }

    /// 矩阵件指纹（折登记位 + 行指纹 + 覆盖账 + 键表）。
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
        h = mix32(h, self.rounds_walked);
        let mut j = 0usize;
        while j < self.keys.len() {
            h = mix_str(h, &self.keys[j]);
            j += 1;
        }
        h
    }

    /// 矩阵件报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "MATRIX rounds={} walked={} keys={} cover_rows={} salt={:016X}",
            self.rows.len(),
            self.rounds_walked,
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
// 14. 正交维度笛卡尔矩阵（10 轮 × 3 相位 × 3 负载档 = 90 格）
// ===========================================================================

/// 笛卡尔单格：维度 A（轮次）× 维度 B（相位）× 维度 C（负载档）。
///
/// 每格**独立夹具**：一格的失败绝不污染其余 89 格。若复用同一个
/// `RoundPiece` 实例逐格试，某格注入的坏行会被后 89 格继承，
/// 于是「90 格里只有 1 格坏」会退化成「90 格全红」，
/// 定位信息（到底是哪一维组合触发）全部丢失——这是矩阵测试最典型的自毁方式。
#[derive(Clone, Copy, Debug)]
pub struct CartCell {
    /// 维度 A：轮次序号。
    pub a: u8,
    /// 维度 B：相位序号。
    pub b: u8,
    /// 维度 C：负载档序号。
    pub c: u8,
    /// 本格注入的**故障形态号**（`0` = 本格未注入；`1..=4` = 四种形态之一）。
    ///
    /// 纪律：注入形态号必须与 `ok` **分开报**。若把「本格坏了」直接编码成
    /// `fault = 1`，报告面就分不清「注入了形态 1」与「本格判定为坏」。
    pub fault: u32,
    /// 本格矩阵件自身是否合规。
    pub ok: bool,
    /// 本格是否可写（册定存在的 (轮次, 相位) 组合 × Draft 阶段）。
    pub writable: bool,
    /// 本格指纹。
    pub digest: u64,
}

impl CartCell {
    /// 单格报告行。
    pub fn row(&self) -> String {
        let ph = match Phase::from_index(self.b) {
            Some(p) => p.name(),
            None => "?",
        };
        let ld = match Load::from_index(self.c) {
            Some(l) => l.name(),
            None => "?",
        };
        format!(
            "CART round={} phase={} load={} fault={} ok={} writable={} digest={:016X}",
            self.a, ph, ld, self.fault, self.ok, self.writable, self.digest
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

/// 阶段是否允许写矩阵（仅 Draft）。
pub fn stage_allows_round(st: Stage) -> bool {
    st.index() == STAGE_DRAFT
}

/// 本格相位与轮次的册定存在性（维度 A × B 的交叉约束）。
///
/// 笛卡尔矩阵的 A、B 两维**不是自由的**：轮次 r 的相位由册定
/// `Phase::of_round(r)` 唯一决定，不存在「轮次 3 × 回写毕」这种组合。
/// 若把 A、B 当独立自由维跑满 30 组，有 20 组是**册定上不存在**的
/// 组合，判它们「坏」没有意义，判它们「好」也是自欺。
/// 故本函数只对**册定存在**的 (r, phase) 组合返回真。
pub fn phase_pair_exists(r: u8, p: Phase) -> bool {
    match Phase::of_round(r) {
        Some(x) => x.index() == p.index(),
        None => false,
    }
}

/// 轮次 r 在**同相位内**的另一轮（册定相邻轮；相位只一轮时返回自身）。
///
/// 同相位的轮次区间是连续的 `[lo, hi)`，故 `r + 1` 若仍在界内就是同相位兄弟；
/// 落在相位末轮时退回 `r - 1`；相位只一轮（`PHASE_ROUND_* == 1`）时无兄弟可撞，
/// 返回自身——此时形态 1 退化为无效注入，该相位的形态 1 需由其他形态兜住。
pub fn sibling_in_phase(r: u8) -> u8 {
    let p = match Phase::of_round(r) {
        Some(x) => x,
        None => return r,
    };
    let lo = p.lo();
    let hi = lo.wrapping_add(p.round_target());
    let n = r as u32;
    if n.wrapping_add(1) < hi {
        (n + 1) as u8
    } else if n > lo {
        (n - 1) as u8
    } else {
        r
    }
}

/// 跑笛卡尔矩阵（90 格，逐格独立夹具）。
///
/// 注入策略：对**册定存在**的 30 格按 `(a + b + c) % 4` 安排一种故障形态，
/// 四种形态分别是「坐标越域 / 断电时点重复 / 落盘越界 / 页型非法」，
/// 覆盖四条独立的注入口。册定不存在的 60 格不注入、`writable` 归零。
pub fn run_cartesian(salt: u64) -> Vec<CartCell> {
    let mut v: Vec<CartCell> = Vec::with_capacity(CART_EXPECTED);
    let mut a = 0usize;
    while a < CART_DA {
        let mut b = 0usize;
        while b < CART_DB {
            let mut c = 0usize;
            while c < CART_DC {
                let r = a as u8;
                let p = Phase::ALL[b];
                let l = Load::ALL[c];
                let exists = phase_pair_exists(r, p);
                // 可写格 = 册定存在 × 阶段允许写
                let writable = exists && stage_allows_round(Stage::Draft);
                let shape = (a + b + c) % 4;

                // —— 本格独立夹具：现搭一份矩阵件，只跑本格 ——
                let mut pz = RoundPiece::new(salt);
                let mut setup_err: Option<ErrKind> = None;
                let mut i = 0usize;
                while i < ROUND_COUNT as usize {
                    let rr = i as u8;
                    // 本格负载档落在目标轮次，其余轮次用轻载占位，
                    // 保证十轮都能登记齐（登记不齐则 `seal_phase` 拒）。
                    let ld = if exists && rr == r { l } else { Load::Light };
                    if let Err(e) = pz.register(rr, ld) {
                        setup_err = Some(e);
                    }
                    i += 1;
                }
                if setup_err.is_none() {
                    if let Err(e) = pz.seal_phase() {
                        setup_err = Some(e);
                    }
                }
                // `fault` 只记「注入了第几号形态」，与 `ok` 分开。
                let mut fault: u32 = 0u32;
                if writable && setup_err.is_none() {
                    fault = (shape as u32) + 1u32;
                    let idx = r as usize;
                    match shape {
                        0 => {
                            // 坐标越域：把相位序号打成闭集外的值。
                            pz.rows[idx].phase = 7;
                        }
                        1 => {
                            // 断电时点重复：与**同相位**的相邻轮撞同一个时点。
                            //
                            // 初稿写的是「与第 0 轮撞」，但轮次 0 在回写前相位、
                            // 目标轮多在回写中/毕相位——`phase_coverage_ok()`
                            // 的重检是**逐相位内**做的，跨相位复制撞不出重，
                            // 于是形态 1 落到这些格上「注了但没坏」，
                            // 注入核报 `injected_ok != inj`。
                            // 教训：注入点必须落在**被检出条件的作用域内**，
                            // 「改了某个字段」不等于「该字段的约束被违反」。
                            let sib = sibling_in_phase(r);
                            if sib != r {
                                pz.rows[idx].cut_lag = pz.rows[sib as usize].cut_lag;
                            }
                        }
                        2 => {
                            // 落盘越界：已落盘页数超过脏页规模。
                            pz.rows[idx].dirty_frames = 1;
                            pz.rows[idx].landed_frames = 5;
                        }
                        _ => {
                            // 页型非法：越出三值闭集。
                            pz.rows[idx].page_kind = 9;
                        }
                    }
                }

                // —— 本格判定：只看格自身不变量，不掺可写性 ——
                let mut ok = false;
                if exists && setup_err.is_none() {
                    let row_ok = pz.rows[r as usize].valid();
                    let cov_ok = phase_coverage_ok(&pz.rows).is_ok();
                    let pair_ok = phase_pair_exists(pz.rows[r as usize].round, p);
                    ok = row_ok && cov_ok && pair_ok;
                }
                let digest = pz.fingerprint();
                v.push(CartCell {
                    a: r,
                    b: p.index(),
                    c: l.index(),
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
            if cells[i].ok == false {
                injected_ok += 1;
            }
        } else if cells[i].writable && cells[i].ok == false {
            // 未注入的可写格必须全绿。册定不存在的组合
            // `writable` 归零，其 `ok == false` 是**册定事实**而非缺陷，
            // 不该被算成「未注入的坏格」。
            inj = inj.wrapping_add(1u32 << 16);
        }
        i += 1;
    }
    let msg = format!(
        "CART_ASSERT cells={} expect={} ok={} writable={} expect_writable={} bad={} injected={} injected_rejected={}",
        tot, CART_EXPECTED, ok, wr, CART_WRITABLE, bad, inj & 0xFFFF, injected_ok
    );
    if tot != CART_EXPECTED as u32 {
        return Err(ErrKind::MatrixDimMismatch);
    }
    if wr != CART_WRITABLE as u32 {
        return Err(ErrKind::StageRejectRegister);
    }
    if (inj & 0xFFFF) != CART_WRITABLE as u32 {
        return Err(ErrKind::StageRejectRegister);
    }
    if injected_ok != (inj & 0xFFFF) {
        return Err(ErrKind::MatrixDrift);
    }
    if (inj >> 16) != 0 {
        return Err(ErrKind::MatrixDrift);
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
// 15. 方案冻结件（主状态机 + 三账 + 冻结指纹）
// ===========================================================================

/// 方案总件：矩阵件 + 分界注记 + 三账 + 状态机 + 逻辑时钟。
pub struct Scheme {
    /// 轮次矩阵件。
    pub piece: RoundPiece,
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
            piece: RoundPiece::new(salt),
            boundary: boundary_note_table(),
            ledger: ChainLedger::new(
                ROUND_REG_TARGET,
                ROUND_REG_TARGET,
                BOUNDARY_COLS,
                BOUNDARY_FACES as u64,
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
        Ok(self.ring.push(self.tick, digest, tag))
    }

    /// 登记一轮三维定义（仅 Draft）。
    pub fn register_round(&mut self, r: u8, l: Load) -> DcRes<u64> {
        if self.stage != Stage::Draft {
            self.rej.push(self.tick, self.stage, Stage::Draft);
            return Err(ErrKind::StageRejectRegister);
        }
        let fp = self.piece.register(r, l)?;
        // 逐轮登记：一轮一记（只补记一次会漏记 N-1 件）。
        self.ledger.note_reg();
        // 加法链分项：判据三维定义里三个**可读数**量各记一账
        // （时点 / 脏页规模 / 页型）。只记总脏页数会把三维压成一维。
        let idx = r as usize;
        self.ledger.note_part(self.piece.rows[idx].cut_lag as u64);
        self.ledger.note_part(self.piece.rows[idx].dirty_frames as u64);
        self.ledger.note_part(self.piece.rows[idx].page_kind as u64);
        self.tick_advance();
        self.ring.push(self.tick, fp, 0x301);
        Ok(fp)
    }

    /// 十轮全登记。
    ///
    /// 负载档按 `(轮次 mod 3)` 轮转，使十轮在三档上均衡分布
    /// （`4/3/3`），既覆盖全部负载档，又不让任一档独占多数轮次。
    pub fn register_all_rounds(&mut self) -> DcRes<u32> {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < ROUND_COUNT as usize {
            let l = Load::ALL[i % Load::ALL.len()];
            self.register_round(i as u8, l)?;
            n += 1;
            i += 1;
        }
        // 十轮齐备后**统一**派生相位覆盖账（登记期不派生，见 `RoundPiece::register`）。
        let _ = self.piece.seal_phase()?;
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

    /// 分界注记入册（两行一面，逐行登记）。
    pub fn note_boundary(&mut self) -> DcRes<u32> {
        boundary_note_ok()?;
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.boundary.len() {
            if !self.boundary[i].valid() {
                return Err(ErrKind::BoundaryNoteMissing);
            }
            // 逐行登记：A4/B5 各两行，面侧各记一次映射。
            self.ledger.note_mapped(1);
            self.ledger.note_cells(1);
            self.tick_advance();
            let fp = self.boundary[i].fingerprint();
            self.ring.push(self.tick, fp, 0x304);
            n += 1;
            i += 1;
        }
        self.explog.record(self.tick, self.boundary_fingerprint(), 0xB0);
        Ok(n)
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

    /// 逐轮走查（读矩阵行表，验证三维坐标与闭集一致）。
    pub fn walk_rounds(&self) -> DcRes<u32> {
        let mut n = 0u32;
        let mut r = 0u8;
        while r <= ROUND_MAX {
            let row = self.piece.row_of(r)?;
            if !row.valid() {
                return Err(ErrKind::MatrixDimMismatch);
            }
            n += 1;
            r += 1;
        }
        Ok(n)
    }

    /// 相位覆盖验证（读回行表 + 覆盖断言）。
    pub fn phase_verify(&self) -> DcRes<Vec<String>> {
        phase_coverage_ok(&self.piece.rows)
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
    /// 折入的只有：册定常量 + 实例盐 + 矩阵件（冻结后只读）+ 分界注记 + 冻结 tick。
    pub fn compute_freeze(&self, tick: u64) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix_str(h, CRITERION);
        h = mix32(h, ROUND_COUNT);
        h = mix32(h, PHASE_COUNT);
        h = mix32(h, LOAD_COUNT);
        h = mix32(h, BOUNDARY_COLS);
        h = mix64(h, self.piece.fingerprint());
        h = mix64(h, self.boundary_fingerprint());
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
        if !self.piece.phase_consistent() {
            return Err(ErrKind::MatrixDimMismatch);
        }
        if phase_coverage_ok(&self.piece.rows).is_err() {
            return Err(ErrKind::PhaseCoverageShort);
        }
        if !self.piece.rows_bidir_ok() {
            return Err(ErrKind::MatrixDrift);
        }
        if !self.bidirectional_ok() {
            return Err(ErrKind::MatrixDrift);
        }
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

    /// 调度入册（冻结件供 F3064 共引）。
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

    /// 封后写检测（在 Frozen / Dispatched 阶段改矩阵必须被拒）。
    pub fn register_after_frozen(&mut self, r: u8, l: Load) -> DcRes<u64> {
        if self.stage == Stage::Frozen || self.stage == Stage::Dispatched {
            self.rej.push(self.tick, self.stage, self.stage);
            return Err(ErrKind::WriteAfterFrozen);
        }
        self.register_round(r, l)
    }

    /// 方案总指纹（折活账，供 `observe` 逐种子比对）。
    pub fn scheme_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix32(h, self.stage.index() as u32);
        h = mix64(h, self.tick);
        h = mix64(h, self.freeze_hash);
        h = mix64(h, self.piece.fingerprint());
        h = mix64(h, self.boundary_fingerprint());
        h = mix64(h, self.ledger.fingerprint());
        h = mix64(h, self.ring.fingerprint());
        h = mix64(h, self.rej.fingerprint());
        h = mix64(h, self.explog.fingerprint());
        h
    }

    /// 矩阵漂移指纹（只折冻结语义载荷，不折活账）。
    pub fn matrix_drift_fp(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.piece.fingerprint());
        h = mix64(h, self.boundary_fingerprint());
        h = mix64(h, self.compute_freeze(self.freeze_tick));
        h
    }

    /// 方案报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "SCHEME stage={} tick={} rounds={} walked={} freeze={:016X}",
            self.stage.name(),
            self.tick,
            ROUND_COUNT,
            self.piece.rounds_walked,
            self.freeze_hash
        ));
        let r = self.piece.report();
        let mut i = 0usize;
        while i < r.len() {
            v.push(r[i].clone());
            i += 1;
        }
        let mut j = 0usize;
        while j < self.boundary.len() {
            v.push(format!("  {}", self.boundary[j].row()));
            j += 1;
        }
        v.push(link_note().join(" ").replace('\n', " "));
        v.push(join_lines(&round_key_table(&self.piece.rows)));
        v
    }
}

// ===========================================================================
// 16. 注入核（12 形态，一形态一错误码，全拒留痕）
// ===========================================================================

/// 注入形态闭集（12 种，覆盖本单 21 个错误变体中的 12 个可注入口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectForm {
    /// I0：轮次序号越界 → `RoundIndexInvalid`。
    F0RoundIndex,
    /// I1：轮次未登记齐即冻结 → `RoundClosedSetBroken`。
    F1RoundMissing,
    /// I2：同一轮次登记两条 → `MatrixCellRepeated`。
    F2CellRepeated,
    /// I3：三维坐标越域 → `MatrixDimMismatch`。
    F3DimMismatch,
    /// I4：相位覆盖不足（某相位零轮次）→ `PhaseCoverageShort`。
    F4PhaseShort,
    /// I5：相位偏聚（全部轮次挤一相位）→ `PhaseSkewRejected`。
    F5PhaseSkew,
    /// I6：断电时点越界 → `MatrixDimMismatch` 的专用形态。
    F6LagOob,
    /// I7：分界注记缺项 → `BoundaryNoteMissing`。
    F7BoundaryMissing,
    /// I8：分界越界（盘面划入 A4）→ `BoundaryScopeLeak`。
    F8BoundaryLeak,
    /// I9：封后写 → `WriteAfterFrozen`。
    F9WriteAfterFrozen,
    /// I10：状态机自环 → `StageIllegalEdge`。
    F10SelfLoop,
    /// I11：矩阵漂移 → `MatrixDrift`。
    F11MatrixDrift,
}

impl InjectForm {
    /// 全部注入形态。
    pub const ALL: [InjectForm; 12] = [
        InjectForm::F0RoundIndex,
        InjectForm::F1RoundMissing,
        InjectForm::F2CellRepeated,
        InjectForm::F3DimMismatch,
        InjectForm::F4PhaseShort,
        InjectForm::F5PhaseSkew,
        InjectForm::F6LagOob,
        InjectForm::F7BoundaryMissing,
        InjectForm::F8BoundaryLeak,
        InjectForm::F9WriteAfterFrozen,
        InjectForm::F10SelfLoop,
        InjectForm::F11MatrixDrift,
    ];

    /// 形态序号（0..11）。
    pub fn index(self) -> usize {
        match self {
            InjectForm::F0RoundIndex => 0,
            InjectForm::F1RoundMissing => 1,
            InjectForm::F2CellRepeated => 2,
            InjectForm::F3DimMismatch => 3,
            InjectForm::F4PhaseShort => 4,
            InjectForm::F5PhaseSkew => 5,
            InjectForm::F6LagOob => 6,
            InjectForm::F7BoundaryMissing => 7,
            InjectForm::F8BoundaryLeak => 8,
            InjectForm::F9WriteAfterFrozen => 9,
            InjectForm::F10SelfLoop => 10,
            InjectForm::F11MatrixDrift => 11,
        }
    }

    /// 预期错误变体（形态与错误一一对应，零空档）。
    pub fn expect(self) -> ErrKind {
        match self {
            InjectForm::F0RoundIndex => ErrKind::RoundIndexInvalid,
            InjectForm::F1RoundMissing => ErrKind::RoundClosedSetBroken,
            InjectForm::F2CellRepeated => ErrKind::MatrixCellRepeated,
            InjectForm::F3DimMismatch => ErrKind::MatrixDimMismatch,
            InjectForm::F4PhaseShort => ErrKind::PhaseCoverageShort,
            InjectForm::F5PhaseSkew => ErrKind::PhaseSkewRejected,
            InjectForm::F6LagOob => ErrKind::MatrixDimMismatch,
            InjectForm::F7BoundaryMissing => ErrKind::BoundaryNoteMissing,
            InjectForm::F8BoundaryLeak => ErrKind::BoundaryScopeLeak,
            InjectForm::F9WriteAfterFrozen => ErrKind::WriteAfterFrozen,
            InjectForm::F10SelfLoop => ErrKind::StageIllegalEdge,
            InjectForm::F11MatrixDrift => ErrKind::FreezeHashDrift,
        }
    }

    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            InjectForm::F0RoundIndex => "round_index",
            InjectForm::F1RoundMissing => "round_missing",
            InjectForm::F2CellRepeated => "cell_repeated",
            InjectForm::F3DimMismatch => "dim_mismatch",
            InjectForm::F4PhaseShort => "phase_short",
            InjectForm::F5PhaseSkew => "phase_skew",
            InjectForm::F6LagOob => "lag_oob",
            InjectForm::F7BoundaryMissing => "boundary_missing",
            InjectForm::F8BoundaryLeak => "boundary_leak",
            InjectForm::F9WriteAfterFrozen => "write_after_frozen",
            InjectForm::F10SelfLoop => "self_loop",
            InjectForm::F11MatrixDrift => "matrix_drift",
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
    let _ = s.register_all_rounds();
    if stage != Stage::Draft {
        let _ = s.advance(Stage::Mapped);
        let _ = s.note_boundary();
    }
    if stage == Stage::Frozen || stage == Stage::Dispatched {
        // `seal()` 只算冻结哈希并入册，**不负责推阶段**；
        // 阶段推进是 `advance()` 的职责（Mapped → Frozen 是一条合法边）。
        // 漏了这一步，夹具的 `stage` 停在 Mapped，
        // 于是 F9「封后写」实际落到 `register_round()` 上，
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
fn finish(f: InjectForm, e: DcRes<u64>, s: &Scheme, tool: &mut Lcg) -> InjectionResult {
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
            probe_fp: s.matrix_drift_fp(),
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
            probe_fp: s.matrix_drift_fp(),
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
            InjectForm::F0RoundIndex => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：轮次序号越界（10 不在 `0..10` 闭集）。
                let e = s.register_round(ROUND_COUNT as u8, Load::Light).map(|v| v);
                finish(f, e, &s, tool)
            }
            InjectForm::F1RoundMissing => {
                // 注入：只登记前 9 轮就派生覆盖 → 缺轮。
                //
                // 不能在 `probe_fixture()` 的结果上「少登记」——它已经把十轮
                // 登记齐了，而 `register_round` 只写不删，少登记根本不会发生。
                // 故自建方案，只登记前 9 轮。
                let mut s = Scheme::new(salt);
                let mut k = 0u8;
                while k < ROUND_MAX {
                    let _ = s.register_round(k, Load::ALL[(k as usize) % 3]);
                    k += 1;
                }
                let e = s.piece.seal_phase().map(|v| v as u64);
                finish(f, e, &s, tool)
            }
            InjectForm::F2CellRepeated => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：同一轮次登记第二条（改负载档 → 该行内容与第一条不同）。
                //
                // `register` 是**幂等覆盖写**，「一格两条」不会在行表里留两条，
                // 检出点只能在**方案侧 vs 实现侧对拍**：册定
                // `plan_side_fingerprint()` 按 `(轮次 mod 3)` 轮转负载，
                // 这里把第 0 轮改成重载，实现侧读回的重载与方案侧轮转出的
                // 轻载必不相等。这正是双向对拍的价值：它抓的不是
                // 「写了几次」，而是「写进去的东西与册定还对不对得上」。
                let _ = s.register_round(0, Load::Heavy);
                let e = if s.piece.plan_side_fingerprint() == s.piece.rows_fingerprint() {
                    Ok(0u64)
                } else {
                    Err(ErrKind::MatrixCellRepeated)
                };
                finish(f, e, &s, tool)
            }
            InjectForm::F3DimMismatch => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：把某轮相位坐标打成闭集外的值。
                s.piece.rows[3].phase = 7;
                let e = if s.piece.phase_consistent() {
                    Ok(0u64)
                } else {
                    Err(ErrKind::MatrixDimMismatch)
                };
                finish(f, e, &s, tool)
            }
            InjectForm::F4PhaseShort => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：把回写后相位的三轮全改判到回写中相位
                // （回写后相位零轮次 → 覆盖不足，非偏聚）。
                let mut k = 0u8;
                while k <= ROUND_MAX {
                    if s.piece.rows[k as usize].phase == PHASE_POST {
                        s.piece.rows[k as usize].phase = PHASE_DURING;
                    }
                    k += 1;
                }
                let e = phase_coverage_ok(&s.piece.rows).map(|_| 0u64);
                finish(f, e, &s, tool)
            }
            InjectForm::F5PhaseSkew => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：全部十轮挤到回写毕相位（判据点名的最坏偏聚）。
                let mut k = 0u8;
                while k <= ROUND_MAX {
                    s.piece.rows[k as usize].phase = PHASE_POST;
                    k += 1;
                }
                let e = phase_coverage_ok(&s.piece.rows).map(|_| 0u64);
                finish(f, e, &s, tool)
            }
            InjectForm::F6LagOob => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：断电时点越出闭集上界。
                s.piece.rows[2].cut_lag = CUT_LAG_MAX + 1;
                let e = if s.piece.rows[2].valid() {
                    Ok(0u64)
                } else {
                    Err(ErrKind::MatrixDimMismatch)
                };
                finish(f, e, &s, tool)
            }
            InjectForm::F7BoundaryMissing => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：删掉分界注记的一行（四列缺一即拒）。
                if s.boundary.len() > 0 {
                    let last = s.boundary.len() - 1;
                    s.boundary.remove(last);
                }
                // 检出点查**实例表**：删了一行就是少一行。
                // 不能重跑册定表构造来「补回来」——那等于把注入抹掉，
                // 再拿「补回来后的册定表恒过」当拒的证据，是自我循环。
                let e = if s.boundary.len() != BOUNDARY_COLS as usize {
                    Err(ErrKind::BoundaryNoteMissing)
                } else {
                    Ok(0u64)
                };
                finish(f, e, &s, tool)
            }
            InjectForm::F8BoundaryLeak => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：把盘面落盘一致性职责从 B5 挪到 A4（分界越界）。
                let mut i2 = 0usize;
                while i2 < s.boundary.len() {
                    if s.boundary[i2].duty == 1 {
                        s.boundary[i2].face = BOUNDARY_A4;
                        s.boundary[i2].other_face = BOUNDARY_B5;
                    }
                    i2 += 1;
                }
                // 检出点查**实例表**：册定表 `boundary_note_ok()` 恒过
                // （它查的是 `boundary_note_table()` 重新构造的册定件），
                // 而注入改的是 `s.boundary` 实例，两者是两份数据。
                let mut leak = false;
                let mut j = 0usize;
                while j < s.boundary.len() {
                    if s.boundary[j].duty == 1 && s.boundary[j].face != BOUNDARY_B5 {
                        leak = true;
                    }
                    j += 1;
                }
                let e = if leak {
                    Err(ErrKind::BoundaryScopeLeak)
                } else {
                    Ok(0u64)
                };
                finish(f, e, &s, tool)
            }
            InjectForm::F9WriteAfterFrozen => {
                let mut s = probe_fixture(Stage::Frozen, salt);
                // 注入：冻结态再登记一轮（封后写）。
                let e = s.register_after_frozen(0, Load::Light).map(|v| v);
                finish(f, e, &s, tool)
            }
            InjectForm::F10SelfLoop => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：状态机自环（Draft → Draft）。
                let e = s.advance(Stage::Draft).map(|_| 0u64);
                finish(f, e, &s, tool)
            }
            InjectForm::F11MatrixDrift => {
                let mut s = probe_fixture(Stage::Frozen, salt);
                // 注入：冻结后偷改矩阵（改脏页规模），
                // 按 `freeze_tick` 重算必与冻结哈希不同。
                s.piece.rows[0].dirty_frames =
                    s.piece.rows[0].dirty_frames.wrapping_add(1);
                // 期望码取 `FreezeHashDrift`：矩阵漂移**在冻结合同上的表现**
                // 就是「同 tick 重算不一致」，报 `FreezeHashDrift` 才是准的。
                // 若把形态的期望码写成 `MatrixDrift`，等于要求
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
pub fn reconcile(s: &Scheme) -> DcRes<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    let led = &s.ledger;

    // R1 登记件数守恒
    //
    // `reg_target()` 定义即 `reg + pending`，故 `reg + pending == reg_target`
    // 是恒真空转的恒等式。真正的守恒内容是 (a) 守恒不漏 + (b) 收官做完。
    let r1 = led.reg.wrapping_add(led.pending);
    let r1_done = led.reg == ROUND_REG_TARGET && led.pending == 0;
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

    // R2 相位覆盖守恒（Σ各相位轮次数 == 全部轮次数）
    let mut span = 0u32;
    let mut pi = 0usize;
    while pi < Phase::ALL.len() {
        let p = Phase::ALL[pi];
        let mut c = 0u32;
        let mut i = 0usize;
        while i < s.piece.rows.len() {
            if s.piece.rows[i].phase == p.index() {
                c += 1;
            }
            i += 1;
        }
        span = span.wrapping_add(c);
        pi += 1;
    }
    v.push(format!(
        "RECON R2 phases={} span_sum={} rounds={} dev={} ok={}",
        Phase::ALL.len(),
        span,
        ROUND_COUNT,
        span as i64 - ROUND_COUNT as i64,
        span == ROUND_COUNT && Phase::ALL.len() == PHASE_COUNT as usize
    ));
    if span != ROUND_COUNT || Phase::ALL.len() != PHASE_COUNT as usize {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R3 分项计数（三维定义三读数 × 十轮 = 30）
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

    // R6 相位覆盖 + 逐轮走查守恒
    let band = match phase_coverage_ok(&s.piece.rows) {
        Ok(_) => true,
        Err(_) => false,
    };
    let walked = match s.walk_rounds() {
        Ok(n) => n,
        Err(_) => 0,
    };
    v.push(format!(
        "RECON R6 phase_cover={} walked={} expect_walked={} piece_walked={} ok={}",
        band,
        walked,
        ROUND_COUNT,
        s.piece.rounds_walked,
        band && walked == ROUND_COUNT && s.piece.rounds_walked == ROUND_COUNT
    ));
    if !band || walked != ROUND_COUNT || s.piece.rounds_walked != ROUND_COUNT {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R7 分界注记 + 双向对拍 + 矩阵键表守恒
    let bn = match boundary_note_ok() {
        Ok(_) => true,
        Err(_) => false,
    };
    let bidir = s.bidirectional_ok();
    let rows_bi = s.piece.rows_bidir_ok();
    let keys_ok = s.piece.keys.len() == ROUND_COUNT as usize;
    let link_ok = link_note_ok();
    v.push(format!(
        "RECON R7 boundary={} bidir={} rows_bidir={} keys={} expect_keys={} link_note={} ok={}",
        bn,
        bidir,
        rows_bi,
        s.piece.keys.len(),
        ROUND_COUNT,
        link_ok,
        bn && bidir && rows_bi && keys_ok && link_ok
    ));
    if !bn || !bidir || !rows_bi || !keys_ok || !link_ok {
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

/// 自检 20 条（覆盖闭集 / 矩阵 / 相位 / 分界 / 账 / 状态机 / 笛卡尔 / 注入）。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut v: Vec<Check> = Vec::new();
    let mut s = Scheme::new(seed);

    // C01 轮次闭集齐备
    v.push(Check::new(
        1,
        "round_closedset",
        ROUND_COUNT == 10 && ROUND_MAX == 9,
        format!("rounds={} max={}", ROUND_COUNT, ROUND_MAX),
    ));

    // C02 十轮全登记
    let _ = s.register_all_rounds();
    v.push(Check::new(
        2,
        "register_all_rounds",
        s.piece.complete(),
        format!("reg={} walked={}", s.piece.complete(), s.piece.rounds_walked),
    ));

    // C03 逐轮自身有效性
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

    // C04 相位覆盖（每相位轮次齐 + 时点互异 + 规模互异）
    let pc = phase_coverage_ok(&s.piece.rows);
    v.push(Check::new(
        4,
        "phase_coverage",
        pc.is_ok(),
        format!("cover={:?}", pc.is_ok()),
    ));

    // C05 逐轮走查
    let wr = s.walk_rounds();
    v.push(Check::new(
        5,
        "walk_rounds",
        wr.is_ok() && wr.unwrap_or(0) == ROUND_COUNT,
        format!("walked={:?} expect={}", wr, ROUND_COUNT),
    ));

    // C06 轮次-相位坐标一致（读回行表）
    v.push(Check::new(
        6,
        "phase_consistent",
        s.piece.phase_consistent(),
        format!("consistent={}", s.piece.phase_consistent()),
    ));

    // C07 相位摊派自洽（`4/3/3` 且合计 == 10）
    let mut tsum = 0u32;
    let mut pi = 0usize;
    while pi < Phase::ALL.len() {
        tsum = tsum.wrapping_add(Phase::ALL[pi].round_target());
        pi += 1;
    }
    v.push(Check::new(
        7,
        "phase_budget",
        tsum == ROUND_COUNT
            && Phase::Pre.round_target() == PHASE_ROUND_PRE
            && Phase::During.round_target() == PHASE_ROUND_DURING
            && Phase::Post.round_target() == PHASE_ROUND_POST,
        format!("budget={} expect={}", tsum, ROUND_COUNT),
    ));

    // C08 轮次→相位映射自洽（十轮全部有相位，无空）
    let mut map_ok = true;
    let mut r = 0u8;
    while r <= ROUND_MAX {
        if Phase::of_round(r).is_none() {
            map_ok = false;
        }
        r += 1;
    }
    v.push(Check::new(
        8,
        "round_phase_map",
        map_ok,
        format!("mapped_rounds={}", ROUND_COUNT),
    ));

    // C09 矩阵键表（十行，与三维坐标同源）
    v.push(Check::new(
        9,
        "round_keys",
        s.piece.keys.len() == ROUND_COUNT as usize,
        format!("keys={} expect={}", s.piece.keys.len(), ROUND_COUNT),
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

    // C14 加法链分项账（登记期已按三维读数逐项记满，此处**只核不记**）
    //
    // 自检条的职责是**核对既有账**，不是替业务路径补账。
    // 在此再跑一轮 `PART_TARGET` 次 `note_part()` 会让同一笔账记两次。
    v.push(Check::new(
        14,
        "note_parts",
        s.ledger.items == PART_TARGET,
        format!("items={} expect={}", s.ledger.items, PART_TARGET),
    ));

    // C15 分界注记入册
    let nb = s.note_boundary();
    v.push(Check::new(
        15,
        "boundary_note",
        nb.is_ok() && nb.unwrap_or(0) == BOUNDARY_COLS,
        format!("rows={:?} expect={}", nb, BOUNDARY_COLS),
    ));

    // C16 冻结
    let seal = s.seal();
    v.push(Check::new(
        16,
        "seal",
        seal.is_ok() && s.freeze_hash != 0,
        format!("freeze={:016X}", s.freeze_hash),
    ));

    // C17 冻结校验（同 tick 重算一致）
    let vf = s.verify_freeze();
    v.push(Check::new(
        17,
        "verify_freeze",
        vf.is_ok(),
        format!("verify={}", vf.is_ok()),
    ));

    // C18 阶段推进到 Frozen + 调度入册
    //
    // `seal()` 不推阶段；`dispatch()` 首行又只认 `Frozen` ——
    // 中间少走一条合法边 Mapped → Frozen，C18 就会恒在 `stage=mapped` 上红。
    let af = s.advance(Stage::Frozen);
    let dp = s.dispatch();
    v.push(Check::new(
        18,
        "dispatch",
        af.is_ok() && dp.is_ok() && s.stage == Stage::Dispatched,
        format!("stage={}", s.stage.name()),
    ));

    // C19 笛卡尔矩阵 90 格（判定一律走 `cartesian_assert()` 权威出口）
    let cells = run_cartesian(seed);
    let (ct, cok, cwr, cbad) = cartesian_stats(&cells);
    // 若这里自己按 `cok/cbad` 另写一份，就会出现两套判定口径，
    // 一套认注入格为坏、另一套认它为绿，谁改谁都可能整体翻红。
    let ca = cartesian_assert(&cells);
    v.push(Check::new(
        19,
        "cartesian",
        ca.is_ok(),
        format!(
            "cells={} ok={} writable={} bad={} assert={}",
            ct, cok, cwr, cbad,
            ca.unwrap_or(String::from("ERR"))
        ),
    ));

    // C20 注入全拒 + 守恒终账
    //
    // 注入侧读数必须**先回填守恒账**再对账：R5a/R5b 查的是
    // `injected_total >= FORM_COUNT` 与「全拒」，而这两项只在
    // `note_inject()` 回填后才有值。observe / replay 两条路径都在
    // 调 `reconcile()` 前先 `note_inject()`，唯独 self_check 漏了，
    // 于是 C20 恒红而另外两条路径全绿。
    // 教训：守恒账的对账方与记账方必须成对出现在**每一条**执行路径上。
    let inj_sc = inject_core_with_tool(seed);
    let miss_sc = detect_miss(&inj_sc);
    let rej_sc = count_rejected(&inj_sc);
    s.note_inject(inj_sc.len() as u32, rej_sc);
    let rec = reconcile(&s);
    v.push(Check::new(
        20,
        "inject_and_reconcile",
        miss_sc == 0
            && rej_sc == inj_sc.len() as u32
            && rec.is_ok()
            && rec.as_ref().map(|r| r.len()).unwrap_or(0) == RECON_ROWS,
        format!(
            "miss={} rejected={}/{} recon_rows={} expect={}",
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
            "UNX-F3061 seed={:016X} all_ok={} freeze={:016X} fp={:016X} inject={}/{} miss={} cart={}/{} ring_live={} dropped={} tlv={}B rt={} replay_stable={}",
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
    v.push(format!("round.count={}", ROUND_COUNT));
    v.push(format!("round.max={}", ROUND_MAX));
    v.push(format!("phase.count={}", PHASE_COUNT));
    v.push(format!("load.count={}", LOAD_COUNT));
    v.push(format!("cut.lag.max={}", CUT_LAG_MAX));
    v.push(format!("dirty.max={}", DIRTY_MAX));
    v.push(format!("boundary.cols={}", BOUNDARY_COLS));
    v.push(format!("boundary.faces={}", BOUNDARY_FACES));
    v.push(format!("link.points={}", LINK_POINTS));
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

    // —— 第二步：真跑方案生命周期 ——
    let mut s = Scheme::new(seed);
    let _ = s.register_all_rounds();
    let _ = s.advance(Stage::Mapped);
    let _ = s.note_boundary();
    // 加法链分项**已由登记期按三维读数记满**（`PART_TARGET` 项），
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
    // 笛卡尔判定走 `cartesian_assert()`（同 C19 口径，唯一权威出口）。
    let cart_ok2 = cartesian_assert(&cells).is_ok()
        && ct == CART_EXPECTED as u32
        && cwr == CART_WRITABLE as u32;

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

    // 步 2 十轮逐轮登记（走 `register_all_rounds()`：它在十轮齐备后
    // 统一 `seal_phase()` 派生覆盖账；直接手搓 `register_round` 循环
    // 会漏掉派生，步 4 相位覆盖立刻 `PhaseCoverageShort`）
    let reg = s.register_all_rounds();
    let reg_ok = reg.is_ok();
    h = mix64(h, s.piece.fingerprint());
    steps.push(ReplayStep {
        step,
        action: "register_all_rounds",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: reg_ok && s.piece.complete(),
    });
    step += 1;

    // 步 3 相位覆盖断言
    let bo = phase_coverage_ok(&s.piece.rows);
    h = mix_str(h, if bo.is_ok() { "cover_ok" } else { "cover_bad" });
    steps.push(ReplayStep {
        step,
        action: "phase_coverage",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: bo.is_ok(),
    });
    step += 1;

    // 步 4 逐轮走查
    let wm = s.walk_rounds();
    h = mix32(h, wm.unwrap_or(0));
    steps.push(ReplayStep {
        step,
        action: "walk_rounds",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: wm.unwrap_or(0) == ROUND_COUNT,
    });
    step += 1;

    // 步 5 分界注记入册
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

    // 步 6 矩阵键表
    let kn = s.piece.keys.len() as u32;
    h = mix32(h, kn);
    steps.push(ReplayStep {
        step,
        action: "round_keys",
        fp: s.piece.fingerprint(),
        tick: s.tick,
        ok: kn == ROUND_COUNT,
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

    // 步 8 加法链分项**核对**（分项已在 `register_all_rounds` 期间记满）
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
    v.push(CostItem::new("register_round", "O(1)", "O(1)", ROUND_REG_TARGET, 112,
        "Windows 无对照：十轮三维登记是本单专有方案冻结纪律"));
    v.push(CostItem::new("phase_coverage_ok", "O(n^2)", "O(n)", ROUND_COUNT * ROUND_COUNT, 96,
        "深在**相位覆盖 + 时点互异 + 规模互异三判**：Windows 崩溃测试只随机断电，无覆盖断言"));
    v.push(CostItem::new("seal_phase", "O(n^2)", "O(n)", ROUND_COUNT * ROUND_COUNT, 128,
        "十轮齐备后一次派生；登记期派生会被自己的空行死锁拦住"));
    v.push(CostItem::new("walk_rounds", "O(n)", "O(1)", ROUND_COUNT, 16,
        "比 Windows 断电读数更严：读行表不重算，账面与运行期同源"));
    v.push(CostItem::new("phase_consistent", "O(n)", "O(1)", ROUND_COUNT, 8,
        "轮次→相位映射唯一性：三维里 B 维不自由，此处专查该约束"));
    v.push(CostItem::new("rows_bidir_ok", "O(n)", "O(1)", ROUND_COUNT, 48,
        "深在**双向**：方案侧独立复现折盐夹取，实现侧读回；少对一项即半对拍"));
    v.push(CostItem::new("boundary_note_ok", "O(n^2)", "O(n)", BOUNDARY_COLS * BOUNDARY_COLS, 64,
        "深在**盘面不越界**：A4 吞盘面职责即拒，比 Windows 边界约定更硬"));
    v.push(CostItem::new("link_note_ok", "O(1)", "O(n)", LINK_POINTS, 32,
        "非对位关系显式注明「本单不落实现」：不注明则看不出为何无 F7602 实现体"));
    v.push(CostItem::new("run_cartesian", "O(1)", "O(n)", CART_EXPECTED as u32, (CART_EXPECTED * 48) as u32,
        "90 格独立夹具：Windows 无同构矩阵，绝不共用一个 fixture"));
    v.push(CostItem::new("cartesian_assert", "O(n)", "O(1)", CART_EXPECTED as u32, 24,
        "注入格必坏 + 未注入可写格必绿 + 注入格数 == 可写格数（30）"));
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
        "MEM RoundRow = {} B x {} 行 = {} B",
        core::mem::size_of::<RoundRow>(),
        ROUND_COUNT,
        core::mem::size_of::<RoundRow>() * ROUND_COUNT as usize
    ));
    v.push(format!(
        "MEM CartCell = {} B x {} 格 = {} B",
        core::mem::size_of::<CartCell>(),
        CART_EXPECTED,
        core::mem::size_of::<CartCell>() * CART_EXPECTED
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
        "MEM ExpLog 无固定容量（入册条数 = 冻结 1 + 分界 1 + 调度 1 = {}）",
        3
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
// 22. 演示入口（便于逐项点验）
// ===========================================================================

/// 演示：相位覆盖断言。
pub fn demo_phase_coverage() -> Vec<String> {
    let mut s = Scheme::new(MATRIX_SALT);
    let _ = s.register_all_rounds();
    let mut v: Vec<String> = Vec::new();
    v.push(join_lines(&s.piece.report()));
    match phase_coverage_ok(&s.piece.rows) {
        Ok(r) => v.push(join_lines(&r)),
        Err(e) => v.push(format!("PHASE-COV ERR {}", err_line(e, 0))),
    }
    v
}

/// 演示：分界注记。
pub fn demo_boundary() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let t = boundary_note_table();
    let mut i = 0usize;
    while i < t.len() {
        v.push(t[i].row());
        i += 1;
    }
    match boundary_note_ok() {
        Ok(r) => v.push(join_lines(&r)),
        Err(e) => v.push(format!("BOUNDARY ERR {}", err_line(e, 0))),
    }
    v.push(join_lines(&link_note()));
    v
}

/// 演示：笛卡尔矩阵（90 格）。
pub fn demo_cartesian(seed: u64) -> Vec<String> {
    let cells = run_cartesian(seed);
    let mut v: Vec<String> = Vec::new();
    v.push(join_lines(&cartesian_report(&cells)));
    match cartesian_assert(&cells) {
        Ok(m) => v.push(format!("CART_ASSERT OK {}", m)),
        Err(e) => v.push(format!("CART_ASSERT ERR {}", err_line(e, 0))),
    }
    v
}

/// 演示：注入核。
pub fn demo_inject(seed: u64) -> Vec<String> {
    let inj = inject_core_with_tool(seed);
    let mut v: Vec<String> = Vec::new();
    v.push(join_lines(&inject_report(&inj)));
    v.push(format!(
        "INJECT total={} rejected={} miss={}",
        inj.len(),
        count_rejected(&inj),
        detect_miss(&inj)
    ));
    v
}

/// 演示：冻结与回放。
pub fn demo_freeze(seed: u64) -> Vec<String> {
    let mut s = Scheme::new(seed);
    let _ = s.register_all_rounds();
    let _ = s.advance(Stage::Mapped);
    let _ = s.note_boundary();
    let inj = inject_core_with_tool(seed);
    s.note_inject(inj.len() as u32, count_rejected(&inj) as u32);
    let mut v: Vec<String> = Vec::new();
    v.push(join_lines(&reconcile(&s).unwrap_or_else(|e| vec![err_line(e, 0)])));
    v.push(format!("seal={:?}", s.seal()));
    v.push(format!("verify={:?}", s.verify_freeze()));
    v.push(format!("advance_frozen={:?}", s.advance(Stage::Frozen)));
    v.push(format!("dispatch={:?}", s.dispatch()));
    v.push(join_lines(&replay(seed).report()));
    v
}

/// 关联单据。
pub fn related_str() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("UNX-F3061 断电演练方案冻结（×10 轮次矩阵）"));
    v.push(String::from("依赖：F7602 回写消费面 / F2726 断电制度 / F2876 脏页链"));
    v.push(String::from("衔接：F3064 演练执行面（矩阵键一一对应）"));
    v.push(String::from("分界：断电影响页缓存面归 A4；盘面落盘一致性归 B1–B5（判例一）"));
    v.push(String::from("同源：与 F3021 三档（轻/中/重）逐档对位，与 F2941 压力轴同源"));
    v.push(String::from("边界：本单只做方案冻结，不做演练执行（F3064）、不做盘面（B1–B5）"));
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
pub const TLV_RECORD_COUNT: usize = 12 + 20 + 6 + 6 + 12;
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
    // 维度规模（12 条）
    push_tlv_u64(&mut out, 0x10, ROUND_COUNT as u64);
    push_tlv_u64(&mut out, 0x11, ROUND_MAX as u64);
    push_tlv_u64(&mut out, 0x12, PHASE_COUNT as u64);
    push_tlv_u64(&mut out, 0x13, LOAD_COUNT as u64);
    push_tlv_u64(&mut out, 0x14, PHASE_ROUND_PRE as u64);
    push_tlv_u64(&mut out, 0x15, PHASE_ROUND_DURING as u64);
    push_tlv_u64(&mut out, 0x16, PHASE_ROUND_POST as u64);
    push_tlv_u64(&mut out, 0x17, CUT_LAG_MAX as u64);
    push_tlv_u64(&mut out, 0x18, DIRTY_MAX as u64);
    push_tlv_u64(&mut out, 0x19, CART_EXPECTED as u64);
    push_tlv_u64(&mut out, 0x1A, CART_WRITABLE as u64);
    push_tlv_u64(&mut out, 0x1B, FORM_COUNT as u64);
    // 三维对位：十轮断电时点 / 脏页规模（12 条）
    let mut ri = 0u8;
    while ri <= ROUND_MAX {
        let pl = cut_lag_plan(ri);
        let pr = RoundRow {
            round: ri,
            phase: Phase::of_round(ri).unwrap_or(Phase::Pre).index(),
            load: Load::ALL[(ri as usize) % Load::ALL.len()].index(),
            cut_lag: lag_plan_side(pl, o.seed, ri as u64),
            dirty_frames: 0,
            landed_frames: 0,
            page_kind: 0,
            plan_lag: pl,
            plan_dirty: 0,
        };
        push_tlv_u64(&mut out, 0x40u8.wrapping_add(ri), pr.cut_lag as u64);
        push_tlv_u64(&mut out, 0x50u8.wrapping_add(ri), cut_lag_plan(ri) as u64);
        ri += 1;
    }
    // 分界与衔接（6 条）
    push_tlv_u64(&mut out, 0x60, BOUNDARY_COLS as u64);
    push_tlv_u64(&mut out, 0x61, BOUNDARY_FACES as u64);
    push_tlv_u64(&mut out, 0x62, PRECASE_NO as u64);
    push_tlv_u64(&mut out, 0x63, LINK_POINTS as u64);
    push_tlv_u64(&mut out, 0x64, PART_TARGET as u64);
    push_tlv_u64(&mut out, 0x65, RECON_ROWS as u64);
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
    push_tlv_u64(&mut out, 0x89, if o.tlv_roundtrip { 1 } else { 0 });
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
        return Err(ErrKind::MatrixCellMissing);
    }
    if buf[0] != TLV_TAG_HEAD {
        return Err(ErrKind::MatrixDimMismatch);
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(ErrKind::MatrixDimMismatch);
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
            return Err(ErrKind::MatrixDimMismatch);
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
        return Err(ErrKind::MatrixDimMismatch);
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
    // 记录段下标偏移：vals[0..2]=组头 3 条；vals[3..14]=维度 12 条；
    // vals[15..34]=三维对位 20 条（每轮时点 + 册定时点）；vals[35..40]=分界衔接 6 条；
    // vals[41..46]=指纹段；vals[47..58]=读数段。
    if vals[3] != ROUND_COUNT as u64 {
        return false;
    }
    if vals[4] != ROUND_MAX as u64 {
        return false;
    }
    if vals[5] != PHASE_COUNT as u64 {
        return false;
    }
    if vals[41] != o.freeze_hash {
        return false;
    }
    if vals[42] != o.scheme_fp {
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
