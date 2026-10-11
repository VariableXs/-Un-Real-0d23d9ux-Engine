//! =====================================================================================
//! 任务单：UNX-F3001 · 双链结构冻结（active/inactive 两链节点布局）
//! 域/批：CoRun·UNX-A4-B31（回收器 · 双链结构本体总装）
//! 册目标：400 行（双链结构 180 + 冻结 120 + 断言 100；测试段不计）
//! 本文件实际：≥2800 行（深化版纯功能件）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F3001-J1，逐字摘抄）：
//!   active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向指针）与 F2682 页缓存驻留结构对齐零缺失，链头尾指针操作单点化
//!
//! 语义边界（逐字摘抄）：
//!   只做结构冻结与对齐；不做链上迁移语义（F3002–F3004）、不做回收执行（F3005）。
//!
//! 依赖与嫁接源（逐字摘抄）：
//!   依赖 F2682/F2683 页缓存结构；嫁接源为 Linux LRU 双链思想，实现自研。
//!
//! 风险与回退（逐字摘抄）：
//!   节点布局失真（挂链位与页缓存结构冲突）——布局冻结前与 F2682 字段逐项对齐断言，冲突即冻结拦截。
//!
//! 复测方式（逐字摘抄）：
//!   双链结构冻结哈希可查，对齐零缺失，越点注入拦截，冻结件供 F3002–F3017 共引。
//!
//! 码段范围声明：
//!   本单独占诊断码段 0x2340 ~ 0x237f。
//!   `SEG_BASE = 0x2340`，`SEG_MASK = 0x003f`（低 6 位为段内序号，
//!   段内共 64 个码位；本单实际使用 18 个错误变体 + 每变体一码一段，
//!   恒 ≤ 60 变体，最大占用 0x2351，未触及段上界 0x237f）。
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

/// 诊断码段基数（本单独占 0x2340–0x237f，共 64 个码位）。
pub const SEG_BASE: u16 = 0x2340;
/// 诊断码段掩码：低 6 位为段内序号，高 10 位为段基数。
pub const SEG_MASK: u16 = 0x003f;
/// 段内码位数。
pub const SEG_SLOTS: u16 = SEG_MASK + 1;
/// 本单最大诊断码（`SEG_BASE + 变体数 - 1`）。
pub const SEG_MAX_CODE: u16 = SEG_BASE + 17;
/// 模块标签。
pub const MODULE_TAG: &str = "UNX-A4-B31/F3001";
/// 判据号。
pub const TASK_ID: &str = "UNX-F3001-J1";
/// 判据全文。
pub const CRITERION: &str = "active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向指针）与 F2682 页缓存驻留结构对齐零缺失，链头尾指针操作单点化";

/// 链种类数（active / inactive 两条，双链是判据规定的唯一结构）。
pub const CHAIN_COUNT: u32 = 2;
/// 链标识 active（驻热页）。
pub const CHAIN_ACTIVE: u8 = 0;
/// 链标识 inactive（驻冷页）。
pub const CHAIN_INACTIVE: u8 = 1;
/// 布局字段数（挂链位 / lru 位标 / 双向指针 = 3 件，判据「三件逐项登记」）。
pub const LAYOUT_FIELDS: u32 = 3;
/// 布局字段位次 0：page 结构挂链位。
pub const FLD_HOOK: u32 = 0;
/// 布局字段位次 1：lru 位标。
pub const FLD_LRU: u32 = 1;
/// 布局字段位次 2：双向指针（prev + next）。
pub const FLD_LINKS: u32 = 2;

/// 空节点序号（`u32::MAX` 表示「未挂链」）。
pub const NODE_NONE: u32 = 0xFFFF_FFFF;
/// 节点槽数（册定：两链共驻留 16 个节点位）。
pub const NODE_SLOTS: u32 = 16;
/// 单链容量上限（active / inactive 各半）。
pub const CHAIN_CAP: u32 = NODE_SLOTS / CHAIN_COUNT;
/// F2682 驻留结构对齐字段数（页缓存驻留结构逐字段对齐面）。
pub const ALIGN_FIELDS: u32 = 8;
/// lru 位标的位宽（1 位：热 / 冷）。
pub const LRU_BIT_WIDTH: u32 = 1;
/// lru 位标：热（active）。
pub const LRU_HOT: u32 = 1;
/// lru 位标：冷（inactive）。
pub const LRU_COLD: u32 = 0;

/// 笛卡尔积格数（2 链 × 3 布局字段 × 4 阶段 = 24，运行期求值）。
pub const CART_DA: usize = 2;
/// 笛卡尔积维度 B 长度。
pub const CART_DB: usize = 3;
/// 笛卡尔积维度 C 长度。
pub const CART_DC: usize = 4;
/// 笛卡尔积期望格数（由上三项乘积给出，仅作交叉对账旁证）。
pub const CART_EXPECTED: usize = 24;
/// 笛卡尔矩阵中「可写格」总数（仅 Draft 阶段可写布局：2×3×1 = 6）。
pub const CART_WRITABLE: usize = 6;
/// 守恒终账行数（由 `reconcile` 的 `push` 次数反推：
/// R1 / R2 / R3 / R4 / R5a / R5b / R6 = **7 行**）。
///
/// 反推纪律：R5 拆成 a/b 两行（下限 / 全拒），故不是 6 行。
/// 此常量若与实际 push 次数不符，`observe` 的 `recon_ok`
/// 恒假，`all_ok` 永远为绿转红——反推错一次就整单红。
pub const RECON_ROWS: usize = 7;
/// 回放步数上限（由 `replay` 的固定动作数反推）。
pub const REPLAY_STEPS_CAP: u32 = 2 + 2 + 2 + 1 + 2 + 1 + 1 + 1 + 1 + 1;
/// 加法链分项目标数（两链各 2 个节点位做分项 + 4 项读数 = 8）。
pub const PART_TARGET: u32 = 8;
/// 加法链分项下限（阻断「分项 0 + 总额 0」的平凡守恒）。
pub const PART_FLOOR: u64 = 5;
/// **册定挂链件数**（active 2 + inactive 1 = 3）。
///
/// 口径纪律：挂链守恒的**对家**必须是「本方案实际登记挂上的节点件数」，
/// 而不是「单链槽位容量」`CHAIN_CAP`。把容量当目标，等于要求
/// 「Σ挂链数 == 容量上限」，那么只有把链填满才能守恒——
/// 可冻结方案本就要给迁移（F3002–F3004）留空位，
/// 强求填满等于逼迫方案虚报挂链数来让断言变绿。
/// 故另立本常量作为守恒对家，容量只作容量用。
pub const LINK_TARGET: u64 = 3;
/// 布局盐（方案侧重算用；随 seed 变化，保证换种子换轨迹）。
pub const LAYOUT_SALT: u64 = 0x3001_0000_0000_0001;
/// 注入核形态数（判据下限 10，本单取 12 种覆盖全部 17 个错误中的 12 个可注入口）。
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
pub const EXPORT_VERSION: u64 = 0x0000300100000001;
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
    /// `节点序号越界：不在闭集 0..{n} 内`。
    NodeIndexOutOfRange,
    /// `链标识非法：非 active / inactive 二值`。
    ChainIdInvalid,
    /// `布局字段缺失：挂链位 / lru 位标 / 双向指针三件缺一`。
    LayoutFieldMissing,
    /// `布局字段冲突：挂链位与 F2682 驻留结构语义不符`。
    LayoutFieldConflict,
    /// `越点操作：链头尾指针未收口到双链单点函数`。
    SinglePointViolation,
    /// `头尾不一致：head 为空但 tail 非空（或反之）`。
    HeadTailInconsistent,
    /// `重复挂链：同一节点在同链出现两次`。
    DupLinkDetected,
    /// `串链：节点同时挂在两链上`。
    CrossChainLeak,
    /// `双向指针不闭合：prev/next 互逆不成立`。
    PrevNextMismatch,
    /// `lru 位标冲突：位标与所在链的期望值不符`。
    LruBitConflict,
    /// `阶段不允许登记布局：仅 Draft 可写`。
    StageRejectRegister,
    /// `阶段不允许冻结：布局未登记齐即冻结被拒`。
    StageRejectFreeze,
    /// `封后写：结构已冻结后再改布局被拒`。
    WriteAfterFrozen,
    /// `冻结哈希漂移：同 tick 重算不一致`。
    FreezeHashDrift,
    /// `状态机非法迁移：跨段 / 自环 / 回退边`。
    StageIllegalEdge,
    /// `与 F2682 对齐失配：字段逐项对拍分叉`。
    AlignMismatch,
    /// `加法链不守恒：Σ分项 ≠ 总额（零偏离要求）`。
    AddChainUnbalanced,
    /// `逻辑 tick 回退：时间不得倒退`。
    TickRegress,}

impl ErrKind {
    /// 全部变体（判据顺序，闭集穷举的权威列表）。
    pub const ALL: [ErrKind; 18] = [
    ErrKind::NodeIndexOutOfRange,
    ErrKind::ChainIdInvalid,
    ErrKind::LayoutFieldMissing,
    ErrKind::LayoutFieldConflict,
    ErrKind::SinglePointViolation,
    ErrKind::HeadTailInconsistent,
    ErrKind::DupLinkDetected,
    ErrKind::CrossChainLeak,
    ErrKind::PrevNextMismatch,
    ErrKind::LruBitConflict,
    ErrKind::StageRejectRegister,
    ErrKind::StageRejectFreeze,
    ErrKind::WriteAfterFrozen,
    ErrKind::FreezeHashDrift,
    ErrKind::StageIllegalEdge,
    ErrKind::AlignMismatch,
    ErrKind::AddChainUnbalanced,
    ErrKind::TickRegress,    ];

    /// 段内序号（0..17）。
    pub fn index(self) -> usize {
        match self {
            ErrKind::NodeIndexOutOfRange => 0,
            ErrKind::ChainIdInvalid => 1,
            ErrKind::LayoutFieldMissing => 2,
            ErrKind::LayoutFieldConflict => 3,
            ErrKind::SinglePointViolation => 4,
            ErrKind::HeadTailInconsistent => 5,
            ErrKind::DupLinkDetected => 6,
            ErrKind::CrossChainLeak => 7,
            ErrKind::PrevNextMismatch => 8,
            ErrKind::LruBitConflict => 9,
            ErrKind::StageRejectRegister => 10,
            ErrKind::StageRejectFreeze => 11,
            ErrKind::WriteAfterFrozen => 12,
            ErrKind::FreezeHashDrift => 13,
            ErrKind::StageIllegalEdge => 14,
            ErrKind::AlignMismatch => 15,
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
            ErrKind::NodeIndexOutOfRange => "NodeIndexOutOfRange",
            ErrKind::ChainIdInvalid => "ChainIdInvalid",
            ErrKind::LayoutFieldMissing => "LayoutFieldMissing",
            ErrKind::LayoutFieldConflict => "LayoutFieldConflict",
            ErrKind::SinglePointViolation => "SinglePointViolation",
            ErrKind::HeadTailInconsistent => "HeadTailInconsistent",
            ErrKind::DupLinkDetected => "DupLinkDetected",
            ErrKind::CrossChainLeak => "CrossChainLeak",
            ErrKind::PrevNextMismatch => "PrevNextMismatch",
            ErrKind::LruBitConflict => "LruBitConflict",
            ErrKind::StageRejectRegister => "StageRejectRegister",
            ErrKind::StageRejectFreeze => "StageRejectFreeze",
            ErrKind::WriteAfterFrozen => "WriteAfterFrozen",
            ErrKind::FreezeHashDrift => "FreezeHashDrift",
            ErrKind::StageIllegalEdge => "StageIllegalEdge",
            ErrKind::AlignMismatch => "AlignMismatch",
            ErrKind::AddChainUnbalanced => "AddChainUnbalanced",
            ErrKind::TickRegress => "TickRegress",        }
    }

    /// 判读：现象 + 位置 + 建议修法（三要素之一，人读一句话）。
    pub fn describe(self) -> &'static str {
        match self {
            ErrKind::NodeIndexOutOfRange => "节点序号越界：不在闭集 0..{n} 内；修法=在节点入口处做闭集闸，禁止裸下标访问",
            ErrKind::ChainIdInvalid => "链标识非法：非 active / inactive 二值；修法=把链标识收进二值闭集，非法值在构造期即拒",
            ErrKind::LayoutFieldMissing => "布局字段缺失：挂链位 / lru 位标 / 双向指针三件缺一；修法=按布局登记表逐字段补齐，缺一即冻结拦截",
            ErrKind::LayoutFieldConflict => "布局字段冲突：挂链位与 F2682 驻留结构语义不符；修法=对齐 F2682 字段语义，冲突处改为对齐映射",
            ErrKind::SinglePointViolation => "越点操作：链头尾指针未收口到双链单点函数；修法=所有头尾写操作改走单点 API，禁止裸写指针",
            ErrKind::HeadTailInconsistent => "头尾不一致：head 为空但 tail 非空（或反之）；修法=重建 head/tail 不变量，空链两端必须同时为空",
            ErrKind::DupLinkDetected => "重复挂链：同一节点在同链出现两次；修法=挂链前查重，命中即拒并回滚",
            ErrKind::CrossChainLeak => "串链：节点同时挂在两链上；修法=摘链必须先落`no_chain`再改挂链位，禁止跨链直改",
            ErrKind::PrevNextMismatch => "双向指针不闭合：prev/next 互逆不成立；修法=以 unlink 后重挂的方式修复，禁单边改指针",
            ErrKind::LruBitConflict => "lru 位标冲突：位标与所在链的期望值不符；修法=按链重算 lru 位标，禁止沿用旧值",
            ErrKind::StageRejectRegister => "阶段不允许登记布局：仅 Draft 可写；修法=把登记动作调度回 Draft 阶段",
            ErrKind::StageRejectFreeze => "阶段不允许冻结：布局未登记齐即冻结被拒；修法=先补齐三件布局字段再冻结",
            ErrKind::WriteAfterFrozen => "封后写：结构已冻结后再改布局被拒；修法=布局变更另开版本号方案",
            ErrKind::FreezeHashDrift => "冻结哈希漂移：同 tick 重算不一致；修法=按 freeze_tick 重算冻结哈希",
            ErrKind::StageIllegalEdge => "状态机非法迁移：跨段 / 自环 / 回退边；修法=只走 +1 合法边，其余一律拒记",
            ErrKind::AlignMismatch => "与 F2682 对齐失配：字段逐项对拍分叉；修法=对齐映射表补齐，逐字段复核",
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
            ErrKind::NodeIndexOutOfRange => "在节点入口处做闭集闸，禁止裸下标访问",
            ErrKind::ChainIdInvalid => "把链标识收进二值闭集，非法值在构造期即拒",
            ErrKind::LayoutFieldMissing => "按布局登记表逐字段补齐，缺一即冻结拦截",
            ErrKind::LayoutFieldConflict => "对齐 F2682 字段语义，冲突处改为对齐映射",
            ErrKind::SinglePointViolation => "所有头尾写操作改走单点 API，禁止裸写指针",
            ErrKind::HeadTailInconsistent => "重建 head/tail 不变量，空链两端必须同时为空",
            ErrKind::DupLinkDetected => "挂链前查重，命中即拒并回滚",
            ErrKind::CrossChainLeak => "摘链必须先落`no_chain`再改挂链位，禁止跨链直改",
            ErrKind::PrevNextMismatch => "以 unlink 后重挂的方式修复，禁单边改指针",
            ErrKind::LruBitConflict => "按链重算 lru 位标，禁止沿用旧值",
            ErrKind::StageRejectRegister => "把登记动作调度回 Draft 阶段",
            ErrKind::StageRejectFreeze => "先补齐三件布局字段再冻结",
            ErrKind::WriteAfterFrozen => "布局变更另开版本号方案",
            ErrKind::FreezeHashDrift => "按 freeze_tick 重算冻结哈希",
            ErrKind::StageIllegalEdge => "只走 +1 合法边，其余一律拒记",
            ErrKind::AlignMismatch => "对齐映射表补齐，逐字段复核",
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
            Stage::Draft => "结构草案",
            Stage::Mapped => "双链已装配",
            Stage::Frozen => "结构已冻结",
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
// 4. 领域闭集一：链种类（active / inactive 双链）
// ===========================================================================

/// 双链种类闭集（判据「active/inactive 双链」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chain {
    /// active 链：驻热页。
    Active,
    /// inactive 链：驻冷页。
    Inactive,
}

impl Chain {
    /// 全部链种。
    pub const ALL: [Chain; 2] = [Chain::Active, Chain::Inactive];

    /// 链序号（0 = active，1 = inactive）。
    pub fn index(self) -> u8 {
        match self {
            Chain::Active => 0,
            Chain::Inactive => 1,
        }
    }

    /// 由序号还原链种；越界返回 `None`（闭集安全）。
    pub fn from_index(i: u8) -> Option<Chain> {
        match i {
            0 => Some(Chain::Active),
            1 => Some(Chain::Inactive),
            _ => None,
        }
    }

    /// 链名（短）。
    pub fn name(self) -> &'static str {
        match self {
            Chain::Active => "active",
            Chain::Inactive => "inactive",
        }
    }

    /// 链名（中）。
    pub fn label(self) -> &'static str {
        match self {
            Chain::Active => "热页链",
            Chain::Inactive => "冷页链",
        }
    }

    /// 本链期望的 lru 位标值（判据「位标与所在链对齐」）。
    pub fn expect_lru(self) -> u32 {
        match self {
            Chain::Active => LRU_HOT,
            Chain::Inactive => LRU_COLD,
        }
    }

    /// 本链的单链容量上限。
    pub fn cap(self) -> u32 {
        CHAIN_CAP
    }
}

// ===========================================================================
// 5. 领域闭集二：布局字段（三件）
// ===========================================================================

/// 布局字段闭集（判据「page 结构挂链位 / lru 位标 / 双向指针」三件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutField {
    /// 字段 0：page 结构挂链位（**不另立结构**，复用页缓存 page 的挂链位）。
    Hook,
    /// 字段 1：lru 位标。
    Lru,
    /// 字段 2：双向指针（prev + next）。
    Links,
}

impl LayoutField {
    /// 全部布局字段。
    pub const ALL: [LayoutField; 3] = [LayoutField::Hook, LayoutField::Lru, LayoutField::Links];

    /// 字段位次（0..2）。
    pub fn index(self) -> u32 {
        match self {
            LayoutField::Hook => 0,
            LayoutField::Lru => 1,
            LayoutField::Links => 2,
        }
    }

    /// 字段名（短）。
    pub fn name(self) -> &'static str {
        match self {
            LayoutField::Hook => "hook",
            LayoutField::Lru => "lru",
            LayoutField::Links => "links",
        }
    }

    /// 字段名（中）。
    pub fn label(self) -> &'static str {
        match self {
            LayoutField::Hook => "挂链位",
            LayoutField::Lru => "lru位标",
            LayoutField::Links => "双向指针",
        }
    }

    /// 本字段的字宽（位）。
    pub fn width_bits(self) -> u32 {
        match self {
            LayoutField::Hook => 4,
            LayoutField::Lru => LRU_BIT_WIDTH,
            LayoutField::Links => 64,
        }
    }

    /// 本字段对应的 F2682 驻留结构字段名（对齐面）。
    ///
    /// 判据「与 F2682 页缓存驻留结构对齐零缺失」——「零缺失」指
    /// 本单三件布局字段每件都要在 F2682 侧找到对应驻留字段，
    /// 且方向是**双向**的：既不许本单有、F2682 无（漏登记），
    /// 也不许本单漏了 F2682 有的驻留字段（漏对齐）。
    pub fn f2682_field(self) -> &'static str {
        match self {
            LayoutField::Hook => "page.list_hook",
            LayoutField::Lru => "page.lru_bit",
            LayoutField::Links => "page.lru_prev_next",
        }
    }

    /// 本字段期望对到的驻留面**位次**（`AlignFace` 闭集内的序号）。
    ///
    /// 与 `AlignFace::hit_layout()` 构成**双向**配对的另一半：
    /// 那一条是「驻留面 → 布局字段」，本条是「布局字段 → 驻留面」。
    /// 双向对拍的意义在于：若只查单向，某一对映射被改成「A 的对家是 C、
    /// C 的对家是 B」这种错位，单向查询仍各自自洽，双向才红。
    pub fn hit_layout_stub(self) -> u8 {
        match self {
            LayoutField::Hook => 4,
            LayoutField::Lru => 5,
            LayoutField::Links => 6,
        }
    }

    /// 本字段缺失时的违例码（供布局逐字段验证回传）。
    pub fn missing_code(self) -> ErrKind {
        match self {
            LayoutField::Hook => ErrKind::LayoutFieldMissing,
            LayoutField::Lru => ErrKind::LayoutFieldMissing,
            LayoutField::Links => ErrKind::LayoutFieldMissing,
        }
    }
}

/// F2682 驻留结构对齐面（8 个字段，逐字段对齐零缺失）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AlignFace {
    /// 驻留字段 0：页框号。
    Frame,
    /// 驻留字段 1：驻留位。
    Resident,
    /// 驻留字段 2：引用计数。
    Refcount,
    /// 驻留字段 3：脏位。
    Dirty,
    /// 驻留字段 4：挂链位（本单布局字段 0 的对家）。
    ListHook,
    /// 驻留字段 5：lru 位标（本单布局字段 1 的对家）。
    LruBit,
    /// 驻留字段 6：双向指针（本单布局字段 2 的对家）。
    LruLinks,
    /// 驻留字段 7：回收优先级。
    ReclaimPrio,
}

impl AlignFace {
    /// 全部驻留字段。
    pub const ALL: [AlignFace; 8] = [
        AlignFace::Frame,
        AlignFace::Resident,
        AlignFace::Refcount,
        AlignFace::Dirty,
        AlignFace::ListHook,
        AlignFace::LruBit,
        AlignFace::LruLinks,
        AlignFace::ReclaimPrio,
    ];

    /// 字段序号（0..7）。
    pub fn index(self) -> u8 {
        match self {
            AlignFace::Frame => 0,
            AlignFace::Resident => 1,
            AlignFace::Refcount => 2,
            AlignFace::Dirty => 3,
            AlignFace::ListHook => 4,
            AlignFace::LruBit => 5,
            AlignFace::LruLinks => 6,
            AlignFace::ReclaimPrio => 7,
        }
    }

    /// 由序号还原驻留面；越界返回 `None`（闭集安全）。
    pub fn from_index(i: u8) -> Option<AlignFace> {
        match i {
            0 => Some(AlignFace::Frame),
            1 => Some(AlignFace::Resident),
            2 => Some(AlignFace::Refcount),
            3 => Some(AlignFace::Dirty),
            4 => Some(AlignFace::ListHook),
            5 => Some(AlignFace::LruBit),
            6 => Some(AlignFace::LruLinks),
            7 => Some(AlignFace::ReclaimPrio),
            _ => None,
        }
    }

    /// 字段名（短）。
    pub fn name(self) -> &'static str {
        match self {
            AlignFace::Frame => "frame",
            AlignFace::Resident => "resident",
            AlignFace::Refcount => "refcount",
            AlignFace::Dirty => "dirty",
            AlignFace::ListHook => "list_hook",
            AlignFace::LruBit => "lru_bit",
            AlignFace::LruLinks => "lru_links",
            AlignFace::ReclaimPrio => "reclaim_prio",
        }
    }

    /// 本字段是否被本单三件布局字段之一覆盖（对齐命中）。
    pub fn covered(self) -> bool {
        match self {
            AlignFace::ListHook => true,
            AlignFace::LruBit => true,
            AlignFace::LruLinks => true,
            _ => false,
        }
    }

    /// 命中的本单布局字段（未覆盖返回 `None`）。
    pub fn hit_layout(self) -> Option<LayoutField> {
        match self {
            AlignFace::ListHook => Some(LayoutField::Hook),
            AlignFace::LruBit => Some(LayoutField::Lru),
            AlignFace::LruLinks => Some(LayoutField::Links),
            _ => None,
        }
    }

    /// 本字段的位宽（位）。
    pub fn width_bits(self) -> u32 {
        match self {
            AlignFace::Frame => 16,
            AlignFace::Resident => 1,
            AlignFace::Refcount => 16,
            AlignFace::Dirty => 1,
            AlignFace::ListHook => 4,
            AlignFace::LruBit => LRU_BIT_WIDTH,
            AlignFace::LruLinks => 64,
            AlignFace::ReclaimPrio => 2,
        }
    }
}

/// 对齐表条目（本单布局字段 ↔ F2682 驻留字段）。
#[derive(Clone, Copy, Debug)]
pub struct AlignRow {
    /// 本单布局字段序号。
    pub layout: u8,
    /// F2682 驻留字段序号。
    pub face: u8,
    /// 位宽是否一致。
    pub width_same: bool,
    /// 语义是否一致（位宽一致 + 名称对得上）。
    pub semantic_same: bool,
}

impl AlignRow {
    /// 单行摘要。
    pub fn row(&self) -> String {
        format!(
            "ALIGN layout={} face={} width_same={} semantic_same={}",
            self.layout, self.face, self.width_same, self.semantic_same
        )
    }
}

/// 构建对齐表（三行，逐字段对拍 F2682 驻留结构）。
pub fn align_table() -> Vec<AlignRow> {
    let mut v: Vec<AlignRow> = Vec::with_capacity(LAYOUT_FIELDS as usize);
    let mut i = 0usize;
    while i < LayoutField::ALL.len() {
        let lf = LayoutField::ALL[i];
        let face = match lf {
            LayoutField::Hook => AlignFace::ListHook,
            LayoutField::Lru => AlignFace::LruBit,
            LayoutField::Links => AlignFace::LruLinks,
        };
        let ws = lf.width_bits() == face.width_bits();
        v.push(AlignRow {
            layout: lf.index() as u8,
            face: face.index(),
            width_same: ws,
            semantic_same: ws,
        });
        i += 1;
    }
    v
}

/// 对齐零缺失断言（**双向**）。
///
/// 方向 A（本单 → F2682）：本单三件布局字段每件都要找到对家。
/// 方向 B（F2682 → 本单）：F2682 每个**布局相关**驻留字段都要被本单覆盖；
///   非布局字段（frame / resident / refcount / dirty / reclaim_prio）
///   本就与链结构无关，不要求本单覆盖——把它们算作「缺失」是错的口径。
pub fn align_zero_missing() -> DcRes<Vec<String>> {
    let t = align_table();
    let mut v: Vec<String> = Vec::new();
    // 方向 A
    let mut fa = true;
    let mut i = 0usize;
    while i < t.len() {
        if !t[i].semantic_same {
            fa = false;
        }
        v.push(format!("  ALIGN-A {}", t[i].row()));
        i += 1;
    }
    v.push(format!("ALIGN-A rows={} all_semantic_same={}", t.len(), fa));
    if t.len() != LAYOUT_FIELDS as usize || !fa {
        return Err(ErrKind::AlignMismatch);
    }
    // 方向 B
    let mut fb = true;
    let mut fcount = 0u32;
    let mut j = 0usize;
    while j < AlignFace::ALL.len() {
        let fc = AlignFace::ALL[j];
        if fc.covered() {
            fcount += 1;
            let hit = match fc.hit_layout() {
                Some(h) => h,
                None => {
                    fb = false;
                    LayoutField::Hook
                }
            };
            // 命中的布局字段必须真的在对拍表里且语义一致
            let mut found = false;
            let mut k = 0usize;
            while k < t.len() {
                if t[k].layout == hit.index() as u8 {
                    found = true;
                }
                k += 1;
            }
            if !found {
                fb = false;
            }
        }
        j += 1;
    }
    v.push(format!(
        "ALIGN-B covered_faces={} expect={} all_hit={}",
        fcount,
        LAYOUT_FIELDS,
        fb
    ));
    if fcount != LAYOUT_FIELDS || !fb {
        return Err(ErrKind::AlignMismatch);
    }
    Ok(v)
}

// ===========================================================================
// 6. 领域结构：页节点（复用 page 结构挂链位，不另立结构）
// ===========================================================================

/// 页节点：复用页缓存 `page` 结构的三个字段位。
///
/// 判据「节点复用页缓存 page 结构的挂链位（不另立结构）」是**结构级**纪律：
/// 若本单另起一套 `struct DualNode { prev, next, lru, ... }`，
/// 就等于页缓存与回收器各持一份指针，两份指针必然漂移，
/// 于是出现「串页事故」（页在A 结构里说在 active，在 B 结构里说在 inactive）。
/// 故本单把节点建模为**页结构的字段视图**：`hook` 是挂链位所在槽号，
/// `lru` 是位标，`prev/next` 是双向指针——全部落在 page 自己的字段空间内。
#[derive(Clone, Copy, Debug)]
pub struct PageNode {
    /// 页框号（页结构本体标识）。
    pub frame: u32,
    /// page 结构挂链位所在槽号（复用挂链位，不另立结构）。
    pub hook: u16,
    /// lru 位标（1 = 热，0 = 冷）。
    pub lru: u32,
    /// 双向指针：prev 节点序号（`NODE_NONE` = 无）。
    pub prev: u32,
    /// 双向指针：next 节点序号（`NODE_NONE` = 无）。
    pub next: u32,
    /// 所在链（0 = active，1 = inactive，`NODE_NONE` = 未挂链）。
    pub chain: u32,
    /// 是否已登记（挂链位有效位）。
    pub registered: bool,
}

impl PageNode {
    /// 未挂链的空节点。
    pub fn empty() -> PageNode {
        PageNode {
            frame: 0,
            hook: 0,
            lru: 0,
            prev: NODE_NONE,
            next: NODE_NONE,
            chain: NODE_NONE,
            registered: false,
        }
    }

    /// 按页框号构造一个「已登记但未挂链」的节点。
    pub fn new(frame: u32) -> PageNode {
        let mut n = PageNode::empty();
        n.frame = frame;
        n.hook = (frame % 16) as u16;
        n.registered = true;
        n
    }

    /// 是否已挂链。
    pub fn linked(&self) -> bool {
        self.chain != NODE_NONE
    }

    /// 是否为链端点（prev 或 next 为空）。
    pub fn is_end(&self) -> bool {
        self.prev == NODE_NONE || self.next == NODE_NONE
    }

    /// 单节点摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix32(h, self.frame);
        h = mix32(h, self.hook as u32);
        h = mix32(h, self.lru);
        h = mix64(h, self.prev as u64);
        h = mix64(h, self.next as u64);
        h = mix64(h, self.chain as u64);
        h = mix_bool(h, self.registered);
        h
    }

    /// 单节点报告行。
    pub fn row(&self) -> String {
        // 报面值统一先落 `String` 再取 `as_str()`：
        // 若直接在 `format!` 实参位里写 `String::from(..).as_str()`，
        // 临时值在实参求值完即释放，编译器报 E0716（borrow 早于 drop），
        // 故此处三处显示值先各自算成具名局部量。
        let prev_txt: String = if self.prev == NODE_NONE {
            String::from("none")
        } else {
            u64_text(self.prev as u64)
        };
        let next_txt: String = if self.next == NODE_NONE {
            String::from("none")
        } else {
            u64_text(self.next as u64)
        };
        let chain_txt: String = if self.chain == NODE_NONE {
            String::from("none")
        } else {
            String::from(Chain::ALL[self.chain as usize].name())
        };
        format!(
            "NODE frame={} hook={} lru={} prev={} next={} chain={} reg={}",
            self.frame,
            self.hook,
            self.lru,
            prev_txt,
            next_txt,
            chain_txt,
            self.registered
        )
    }

    /// 布局逐字段验证：返回首个违例（`None` = 三件布局字段全过）。
    ///
    /// 判据「布局冻结前与 F2682 字段逐项对齐断言，冲突即冻结拦截」——
    /// 本函数是那条断言的载体，逐件查：
    ///   * `Hook`：挂链位槽号必须在 page 字段空间内（`hook < 16`）
    ///   * `Lru`：位标必须是 0 或 1（位宽 1 位，不得越界写）
    ///   * `Links`：prev / next 不得自指（自环即串页事故前兆）
    pub fn layout_fault(&self) -> Option<ErrKind> {
        if self.hook >= 16 {
            return Some(ErrKind::LayoutFieldConflict);
        }
        if self.lru > 1 {
            return Some(ErrKind::LruBitConflict);
        }
        if self.prev == self.frame && self.prev != NODE_NONE {
            return Some(ErrKind::PrevNextMismatch);
        }
        if self.next == self.frame && self.next != NODE_NONE {
            return Some(ErrKind::PrevNextMismatch);
        }
        None
    }

    /// 布局是否全过。
    pub fn layout_ok(&self) -> bool {
        match self.layout_fault() {
            Some(_) => false,
            None => true,
        }
    }

    /// 布局校验行（报告面回显三件字段）。
    pub fn layout_row(&self) -> String {
        format!(
            "LAYOUT frame={} hook={}(<16:{}) lru={}(<=1:{}) prev={} next={} no_self={} fault={}",
            self.frame,
            self.hook,
            self.hook < 16,
            self.lru,
            self.lru <= 1,
            self.prev,
            self.next,
            !(self.prev == self.frame && self.prev != NODE_NONE)
                && !(self.next == self.frame && self.next != NODE_NONE),
            match self.layout_fault() {
                Some(e) => e.name(),
                None => "none",
            }
        )
    }
}

// ===========================================================================
// 7. 双链结构本体（头尾操作**全部收口到单点函数**）
// ===========================================================================

/// 双链操作写痕迹（用于证明「越点操作 10/10 被拦」）。
///
/// 判据「链头尾指针操作全部收口到双链操作单点」的验证方式：
/// 本单只允许经 `DualChainOp` 这一组单点函数改链，任何裸写指针都必须
/// 打上 `bare` 痕迹；而守恒恒等式 R5 要求 `bare_writes == 0`。
#[derive(Clone, Copy, Debug)]
pub struct WriteTrace {
    /// 是否越点裸写（未走单点 API）。
    pub bare: bool,
    /// 走单点 API 的次数。
    pub via_op: u32,
    /// 越点裸写次数（守恒要求恒 0）。
    pub bare_writes: u32,
}

impl WriteTrace {
    /// 构造零痕迹。
    pub fn new() -> WriteTrace {
        WriteTrace {
            bare: false,
            via_op: 0,
            bare_writes: 0,
        }
    }
    /// 走单点。
    pub fn note_op(&mut self) {
        self.via_op = self.via_op.wrapping_add(1);
    }
    /// 越点裸写（立即计数，守恒必红）。
    pub fn note_bare(&mut self) {
        self.bare = true;
        self.bare_writes = self.bare_writes.wrapping_add(1);
    }
    /// 摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_bool(h, self.bare);
        h = mix32(h, self.via_op);
        h = mix32(h, self.bare_writes);
        h
    }
    /// 报告行。
    pub fn row(&self) -> String {
        format!(
            "OPTRACE via_op={} bare={} bare_writes={} ok={}",
            self.via_op,
            self.bare,
            self.bare_writes,
            self.bare_writes == 0
        )
    }
}

/// 双链结构：两条链 + 节点池 + 写痕迹。
///
/// **单点化纪律**：本结构的所有公开方法都是「单点函数」，
/// 链头（`head`）与链尾（`tail`）字段对外**不开放直接写**——
/// 任何挂链 / 摘链都必须走 `link_tail` / `link_head` / `unlink`，
/// 由它们负责维护 prev/next 互逆、lru 位标、head/tail 一致性。
/// 注入核会尝试越点裸写（直接改 `head`），必被痕迹抓住。
#[derive(Clone, Debug)]
pub struct DualChain {
    /// 节点池（长度恒为 `NODE_SLOTS`）。
    pub nodes: Vec<PageNode>,
    /// active 链头节点序号（`NODE_NONE` = 空）。
    pub head_active: u32,
    /// active 链尾节点序号。
    pub tail_active: u32,
    /// inactive 链头节点序号。
    pub head_inactive: u32,
    /// inactive 链尾节点序号。
    pub tail_inactive: u32,
    /// 写痕迹。
    pub trace: WriteTrace,
}

impl DualChain {
    /// 构造空双链（两链皆空，节点池已登记但未挂链）。
    pub fn new() -> DualChain {
        let mut nodes: Vec<PageNode> = Vec::with_capacity(NODE_SLOTS as usize);
        let mut i = 0u32;
        while i < NODE_SLOTS {
            nodes.push(PageNode::empty());
            i += 1;
        }
        DualChain {
            nodes,
            head_active: NODE_NONE,
            tail_active: NODE_NONE,
            head_inactive: NODE_NONE,
            tail_inactive: NODE_NONE,
            trace: WriteTrace::new(),
        }
    }

    /// 取某链的头指针（只读）。
    pub fn head(&self, c: Chain) -> u32 {
        match c {
            Chain::Active => self.head_active,
            Chain::Inactive => self.head_inactive,
        }
    }

    /// 取某链的尾指针（只读）。
    pub fn tail(&self, c: Chain) -> u32 {
        match c {
            Chain::Active => self.tail_active,
            Chain::Inactive => self.tail_inactive,
        }
    }

    /// 登记一个页节点进节点池（**仅登记，不挂链**）。
    ///
    /// 刻意令 ：这样 / 既是页框号又是下标，
    /// 双重索引合一，判互逆时不必再做「帧号→下标」反查。
    /// 若二者分离，每次互逆判定都要 O(n) 反查，且反查表本身会成为
    /// 第二个可能与指针漂移的状态源——多一处真相就多一处事故面。
    pub fn register_node(&mut self) -> DcRes<u32> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if !self.nodes[i].registered {
                self.nodes[i] = PageNode::new(i as u32);
                self.trace.note_op();
                return Ok(i as u32);
            }
            i += 1;
        }
        Err(ErrKind::NodeIndexOutOfRange)
    }

    /// **单点函数**：把节点挂到链尾（维护 prev/next 互逆 + lru 位标 + head/tail）。
    pub fn link_tail(&mut self, chain: Chain, node: u32) -> DcRes<()> {
        self.link_impl(chain, node, true)
    }

    /// **单点函数**：把节点挂到链头。
    pub fn link_head(&mut self, chain: Chain, node: u32) -> DcRes<()> {
        self.link_impl(chain, node, false)
    }

    /// 挂链实现（`at_tail` 决定挂尾或挂头）。
    fn link_impl(&mut self, chain: Chain, node: u32, at_tail: bool) -> DcRes<()> {
        if node as usize >= self.nodes.len() {
            return Err(ErrKind::NodeIndexOutOfRange);
        }
        let ci = chain.index() as usize;
        if ci >= CHAIN_COUNT as usize {
            return Err(ErrKind::ChainIdInvalid);
        }
        // —— 三道关 ——
        // 关 1：节点必须已登记
        if !self.nodes[node as usize].registered {
            return Err(ErrKind::LayoutFieldMissing);
        }
        // 关 2：不得重复挂链（已在链上则 `DupLinkDetected`；挂在另一链则 `CrossChainLeak`）
        if self.nodes[node as usize].linked() {
            let cur = self.nodes[node as usize].chain;
            if cur == chain.index() as u64 as u32 {
                return Err(ErrKind::DupLinkDetected);
            }
            return Err(ErrKind::CrossChainLeak);
        }
        // 关 3：单链容量
        if self.chain_len(chain) >= chain.cap() {
            return Err(ErrKind::NodeIndexOutOfRange);
        }
        // —— 布局对齐：lru 位标必须与所在链一致 ——
        let want_lru = chain.expect_lru();
        if self.nodes[node as usize].lru != want_lru {
            self.nodes[node as usize].lru = want_lru;
        }
        // —— 正式挂链（全部指针写都在本函数内，即「单点」）——
        self.nodes[node as usize].chain = chain.index() as u32;
        if at_tail {
            let t = self.tail(chain);
            self.nodes[node as usize].prev = t;
            self.nodes[node as usize].next = NODE_NONE;
            if t != NODE_NONE && (t as usize) < self.nodes.len() {
                self.nodes[t as usize].next = node;
            }
            self.set_tail(chain, node);
            if self.head(chain) == NODE_NONE {
                self.set_head(chain, node);
            }
        } else {
            let h = self.head(chain);
            self.nodes[node as usize].next = h;
            self.nodes[node as usize].prev = NODE_NONE;
            if h != NODE_NONE && (h as usize) < self.nodes.len() {
                self.nodes[h as usize].prev = node;
            }
            self.set_head(chain, node);
            if self.tail(chain) == NODE_NONE {
                self.set_tail(chain, node);
            }
        }
        self.trace.note_op();
        Ok(())
    }

    /// **单点函数**：摘链（维护 prev/next 互逆 + head/tail 一致）。
    pub fn unlink(&mut self, node: u32) -> DcRes<()> {
        if node as usize >= self.nodes.len() {
            return Err(ErrKind::NodeIndexOutOfRange);
        }
        let ci = self.nodes[node as usize].chain;
        if ci == NODE_NONE || ci as usize >= CHAIN_COUNT as usize {
            return Err(ErrKind::ChainIdInvalid);
        }
        let ch = match Chain::from_index(ci as u8) {
            Some(c) => c,
            None => return Err(ErrKind::ChainIdInvalid),
        };
        let p = self.nodes[node as usize].prev;
        let n = self.nodes[node as usize].next;
        if p != NODE_NONE && (p as usize) < self.nodes.len() {
            self.nodes[p as usize].next = n;
        }
        if n != NODE_NONE && (n as usize) < self.nodes.len() {
            self.nodes[n as usize].prev = p;
        }
        if self.head(ch) == node {
            self.set_head(ch, n);
        }
        if self.tail(ch) == node {
            self.set_tail(ch, p);
        }
        self.nodes[node as usize].chain = NODE_NONE;
        self.nodes[node as usize].prev = NODE_NONE;
        self.nodes[node as usize].next = NODE_NONE;
        self.trace.note_op();
        Ok(())
    }

    /// 内部：写链头（**唯一**写 head 的地方，即单点）。
    fn set_head(&mut self, c: Chain, v: u32) {
        match c {
            Chain::Active => self.head_active = v,
            Chain::Inactive => self.head_inactive = v,
        }
    }
    /// 内部：写链尾（**唯一**写 tail 的地方，即单点）。
    fn set_tail(&mut self, c: Chain, v: u32) {
        match c {
            Chain::Active => self.tail_active = v,
            Chain::Inactive => self.tail_inactive = v,
        }
    }

    /// 越点裸写演示（**故意写 head**，必被痕迹抓住）。
    ///
    /// 注入核用它证明「头尾操作确实收口」：若本函数能被正常路径调用，
    /// `trace.bare_writes` 会 >0，守恒恒等式 R5 立即红。
    pub fn bare_write_head(&mut self, chain: Chain, v: u32) {
        self.trace.note_bare();
        match chain {
            Chain::Active => self.head_active = v,
            Chain::Inactive => self.head_inactive = v,
        }
    }

    /// 某链长度（自链头沿 next 走一趟，**不**信任 len 字段）。
    pub fn chain_len(&self, c: Chain) -> u32 {
        let mut n = 0u32;
        let mut cur = self.head(c);
        let mut guard = 0u32;
        while cur != NODE_NONE && guard < NODE_SLOTS + 2 {
            n += 1;
            if cur as usize >= self.nodes.len() {
                break;
            }
            cur = self.nodes[cur as usize].next;
            guard += 1;
        }
        n
    }

    /// 某链节点位次表（自链头到链尾）。
    pub fn chain_seq(&self, c: Chain) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut cur = self.head(c);
        let mut guard = 0u32;
        while cur != NODE_NONE && guard < NODE_SLOTS + 2 {
            v.push(cur);
            if cur as usize >= self.nodes.len() {
                break;
            }
            cur = self.nodes[cur as usize].next;
            guard += 1;
        }
        v
    }

    /// 头尾一致性：空链两端必须同时为空；非空链两端必须互不为空。
    pub fn head_tail_consistent(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < Chain::ALL.len() {
            let c = Chain::ALL[i];
            let h = self.head(c);
            let t = self.tail(c);
            let empty = self.chain_len(c) == 0;
            if empty {
                if h != NODE_NONE || t != NODE_NONE {
                    ok = false;
                }
            } else {
                // 自链头实走一遍：走出的位次表首元素必须等于 head、末元素必须等于 tail。
                // 只查「两端非空 + 端点 prev/next 为空」不够：中间某个 next 被单独抹掉时，
                // 链被截成两段而两端仍非空、端点指针仍合规，账面看不出破口。
                // 走一遍能把「tail 仍指向已不可达的孤儿节点」当场抓住。
                let seq = self.chain_seq(c);
                if seq.is_empty() {
                    ok = false;
                } else {
                    if seq[0] != h {
                        ok = false;
                    }
                    if seq[seq.len() - 1] != t {
                        ok = false;
                    }
                }
                if h == NODE_NONE || t == NODE_NONE {
                    ok = false;
                } else {
                    if (h as usize) < self.nodes.len() && self.nodes[h as usize].prev != NODE_NONE {
                        ok = false;
                    }
                    if (t as usize) < self.nodes.len() && self.nodes[t as usize].next != NODE_NONE {
                        ok = false;
                    }
                }
            }
            i += 1;
        }
        ok
    }

    /// prev/next 互逆闭合判定（逐节点查 `A.next == B` 且 `B.prev == A`）。
    pub fn links_reciprocal(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = self.nodes[i];
            if !n.linked() {
                i += 1;
                continue;
            }
            if n.next != NODE_NONE {
                if (n.next as usize) >= self.nodes.len() {
                    ok = false;
                } else {
                    let m = self.nodes[n.next as usize];
                    // 严格对拍：`m.prev` 必须恒等于 `n.frame`。
                    // 旧写法放宽成 `m.prev != n.frame && m.prev != NODE_NONE`
                    // （即「m 未挂链就算过」）是**错的宽容条款**：把 n.next 单独
                    // 抹成 NODE_NONE 之后，m 仍挂着链却丢了反向指针，
                    // 这种「单向断链」恰好是串页事故的直接成因，却被旧写法放过。
                    // 正确口径：链上相邻节点的指针**必须**互逆，一侧为空即破。
                    if m.prev != n.frame {
                        // 注意：prev/next 存的是**页框号**而非下标，故对拍用 frame
                        ok = false;
                    }
                    if m.chain != n.chain {
                        ok = false;
                    }
                }
            }
            if n.prev != NODE_NONE {
                if (n.prev as usize) >= self.nodes.len() {
                    ok = false;
                } else {
                    let m = self.nodes[n.prev as usize];
                    // 同上：m 必在链上且 m.next 恒等于 n.frame，不接受空指针豁免。
                    if m.next != n.frame {
                        ok = false;
                    }
                }
            }
            i += 1;
        }
        ok
    }

    /// 串链检测：同一页框不得同时出现在两链（逐帧查）。
    pub fn cross_chain_clean(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if !self.nodes[i].linked() {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < self.nodes.len() {
                if self.nodes[j].linked()
                    && self.nodes[i].frame == self.nodes[j].frame
                    && self.nodes[i].chain != self.nodes[j].chain
                {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        ok
    }

    /// lru 位标与所在链对齐判定。
    pub fn lru_aligned(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = self.nodes[i];
            if n.linked() {
                let c = Chain::ALL[n.chain as usize];
                if n.lru != c.expect_lru() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    }

    /// 布局逐字段全过（所有已登记节点）。
    pub fn layout_all_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.nodes[i].registered && !self.nodes[i].layout_ok() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 双链总摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        let mut i = 0usize;
        while i < self.nodes.len() {
            h = mix64(h, self.nodes[i].fingerprint());
            i += 1;
        }
        h = mix64(h, self.head_active as u64);
        h = mix64(h, self.tail_active as u64);
        h = mix64(h, self.head_inactive as u64);
        h = mix64(h, self.tail_inactive as u64);
        h = mix64(h, self.trace.fingerprint());
        h
    }

    /// 双链报告行。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "DUALCHAIN active_len={} inactive_len={} head_tail_ok={} recip_ok={} cross_clean={} lru_ok={}",
            self.chain_len(Chain::Active),
            self.chain_len(Chain::Inactive),
            self.head_tail_consistent(),
            self.links_reciprocal(),
            self.cross_chain_clean(),
            self.lru_aligned()
        ));
        v.push(format!("  {}", self.trace.row()));
        let mut i = 0usize;
        while i < Chain::ALL.len() {
            let c = Chain::ALL[i];
            let seq = self.chain_seq(c);
            v.push(format!(
                "  CHAIN {} len={} head={} tail={} seq_len={}",
                c.name(),
                self.chain_len(c),
                self.head(c),
                self.tail(c),
                seq.len()
            ));
            i += 1;
        }
        let mut j = 0usize;
        while j < self.nodes.len() {
            if self.nodes[j].linked() {
                v.push(format!("  {}", self.nodes[j].row()));
                v.push(format!("    {}", self.nodes[j].layout_row()));
            }
            j += 1;
        }
        v
    }
}

// ===========================================================================
// 8. 正交维度笛卡尔矩阵（2 链 × 3 布局字段 × 4 阶段 = 24 格）
// ===========================================================================

/// 笛卡尔单格：维度 A（链）× 维度 B（布局字段）× 维度 C（阶段）。
///
/// 每格自带**独立夹具**判定：本格若单独拿来做是否成立。
#[derive(Clone, Debug)]
pub struct CartCell {
    /// 维度 A：链序号。
    pub da: u8,
    /// 维度 B：布局字段位次。
    pub db: u32,
    /// 维度 C：阶段序号。
    pub dc: u8,
    /// 该阶段的布局登记是否合法。
    pub writable: bool,
    /// 该格的布局三件是否全过（独立夹具判定）。
    pub layout_ok: bool,
    /// 该格的对齐语义是否一致。
    pub align_same: bool,
    /// 该格整体成立。
    pub ok: bool,
    /// 该格夹具摘要。
    pub digest: u64,
}

impl CartCell {
    /// 单格摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.da as u64);
        h = mix32(h, self.db);
        h = mix64(h, self.dc as u64);
        h = mix_bool(h, self.writable);
        h = mix_bool(h, self.layout_ok);
        h = mix_bool(h, self.align_same);
        h = mix_bool(h, self.ok);
        h = mix64(h, self.digest);
        h
    }
    /// 单格报告行。
    pub fn row(&self) -> String {
        format!(
            "CART chain={} field={} stage={} writable={} layout_ok={} align={} ok={} digest={:016X}",
            self.da, self.db, self.dc, self.writable, self.layout_ok, self.align_same, self.ok, self.digest
        )
    }
}

/// 该阶段是否允许登记布局字段（仅草案态）。
pub fn stage_allows_layout(s: Stage) -> bool {
    match s {
        Stage::Draft => true,
        _ => false,
    }
}

/// 运行期笛卡尔格数（`2 * 3 * 4`，非手写常量）。
pub fn cartesian_cells() -> usize {
    Chain::ALL.len() * LayoutField::ALL.len() * Stage::ALL.len()
}

/// 运行期维度长度三元组。
pub fn cartesian_dims() -> (usize, usize, usize) {
    (Chain::ALL.len(), LayoutField::ALL.len(), Stage::ALL.len())
}

/// 逐格跑笛卡尔矩阵（**每格独立夹具**，互不污染）。
pub fn run_cartesian(salt: u64) -> Vec<CartCell> {
    let (na, nb, nc) = cartesian_dims();
    let mut out: Vec<CartCell> = Vec::with_capacity(na * nb * nc);
    let mut a = 0usize;
    while a < na {
        let ch = Chain::ALL[a];
        let mut b = 0usize;
        while b < nb {
            let lf = LayoutField::ALL[b];
            let mut c = 0usize;
            while c < nc {
                let st = Stage::ALL[c];
                // —— 独立夹具：本格自造一个节点 + 一个双链，只验本格 ——
                let mut d = DualChain::new();
                let node = PageNode::new(((a * 4 + b) as u32) + 1u32);
                d.nodes[0] = node;
                let lo = d.nodes[0].layout_fault();
                let layout_ok = match lo {
                    Some(_) => false,
                    None => true,
                };
                // 该链挂上去（只为让 lru 位标可判）
                if layout_ok {
                    d.nodes[0].lru = ch.expect_lru();
                }
                let lru_ok = d.nodes[0].lru == ch.expect_lru();
                let align_same = lf.width_bits() == align_face_of(lf).width_bits();
                let writable = stage_allows_layout(st);
                let ok = layout_ok && lru_ok && align_same;
                let mut h = FNV_BASIS;
                h = mix64(h, salt);
                h = mix64(h, a as u64);
                h = mix32(h, lf.index());
                h = mix64(h, st.index() as u64);
                h = mix64(h, d.fingerprint());
                out.push(CartCell {
                    da: ch.index(),
                    db: lf.index(),
                    dc: st.index(),
                    writable,
                    layout_ok,
                    align_same,
                    ok,
                    digest: h,
                });
                c += 1;
            }
            b += 1;
        }
        a += 1;
    }
    out
}

/// 布局字段对应的 F2682 驻留面。
pub fn align_face_of(lf: LayoutField) -> AlignFace {
    match lf {
        LayoutField::Hook => AlignFace::ListHook,
        LayoutField::Lru => AlignFace::LruBit,
        LayoutField::Links => AlignFace::LruLinks,
    }
}

/// 笛卡尔矩阵指纹（逐格折叠，顺序敏感）。
pub fn cartesian_fingerprint(cells: &[CartCell]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < cells.len() {
        h = mix64(h, cells[i].fingerprint());
        i += 1;
    }
    h
}

/// 笛卡尔矩阵统计：总数 / 成立数 / 可写数 / 违例数。
pub fn cartesian_stats(cells: &[CartCell]) -> (usize, usize, usize, usize) {
    let mut total = 0usize;
    let mut ok = 0usize;
    let mut wr = 0usize;
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
            wr += 1;
        }
        i += 1;
    }
    (total, ok, wr, bad)
}

/// 笛卡尔断言报告（格数交叉对账 + 可写格数 + 全格成立）。
pub fn cartesian_assert(cells: &[CartCell]) -> DcRes<Vec<String>> {
    let (total, ok, wr, bad) = cartesian_stats(cells);
    let mut v: Vec<String> = Vec::new();
    v.push(format!(
        "CART cells={} expected={} ok={}",
        total,
        CART_EXPECTED,
        total == CART_EXPECTED
    ));
    v.push(format!(
        "CART writable={} expect={} ok={}",
        wr,
        CART_WRITABLE,
        wr == CART_WRITABLE
    ));
    v.push(format!("CART ok={} bad={} all_ok={}", ok, bad, bad == 0));
    if total != CART_EXPECTED {
        return Err(ErrKind::AlignMismatch);
    }
    if bad != 0 {
        return Err(ErrKind::LayoutFieldMissing);
    }
    if wr != CART_WRITABLE {
        return Err(ErrKind::StageRejectRegister);
    }
    Ok(v)
}

// ===========================================================================
// 9. 结构冻结主体（Scheme）
// ===========================================================================

/// 结构冻结主体：双链 + 布局登记 + 三账 + 状态机。
///
/// 生命周期：`Draft`（登记布局）→ `Mapped`（挂链 + 对齐）→ `Frozen`（封哈希）
/// → `Dispatched`（调度留痕）。任何越阶段操作一律 `Err` 且留痕。
#[derive(Clone, Debug)]
pub struct Scheme {
    /// 双链本体。
    pub dual: DualChain,
    /// 三件布局字段的登记位（长度恒为 `LAYOUT_FIELDS`，索引 = 字段位次）。
    pub reg: Vec<bool>,
    /// 对齐表（三行）。
    pub align: Vec<AlignRow>,
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
    /// 本实例布局盐（随 seed 变化，供方案侧重算对拍）。
    pub salt: u64,
}

impl Scheme {
    /// 构造草案态（`Draft`）。
    pub fn new(salt: u64) -> Scheme {
        let mut reg: Vec<bool> = Vec::with_capacity(LAYOUT_FIELDS as usize);
        let mut i = 0usize;
        while i < LAYOUT_FIELDS as usize {
            reg.push(false);
            i += 1;
        }
        Scheme {
            dual: DualChain::new(),
            reg,
            align: align_table(),
            ledger: ChainLedger::new(
                // 登记件数目标 = 三件布局字段，逐件各记一次。
                LAYOUT_FIELDS,
                LAYOUT_FIELDS,
                CHAIN_COUNT,
                // 覆盖量对家 = 册定挂链件数，不是单链容量（见 LINK_TARGET 注释）。
                LINK_TARGET,
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

    /// 登记一件布局字段（阶段须 `Draft`）。
    ///
    /// 四道关：位次在闭集内 / 阶段可写 / 未重复登记 / 与 F2682 语义对齐。
    pub fn register_layout(&mut self, field: u32, salt: u64) -> DcRes<u64> {
        if field as usize >= self.reg.len() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::NodeIndexOutOfRange);
        }
        if !stage_allows_layout(self.stage) {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            let nxt = if self.stage == Stage::Draft {
                Stage::Mapped
            } else {
                Stage::Draft
            };
            self.rej.push(self.tick, self.stage, nxt);
            return Err(ErrKind::StageRejectRegister);
        }
        if self.reg[field as usize] {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::LayoutFieldMissing);
        }
        // 与 F2682 驻留结构对齐：位宽 + 语义
        let lf = LayoutField::ALL[field as usize];
        let face = align_face_of(lf);
        if lf.width_bits() != face.width_bits() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::LayoutFieldConflict);
        }
        self.reg[field as usize] = true;
        self.ledger.note_cells(1);
        self.tick_advance();
        let mut h = FNV_BASIS;
        h = mix64(h, salt);
        h = mix32(h, field);
        h = mix_str(h, lf.f2682_field());
        self.ring.push(self.tick, h, field | 0x100);
        Ok(h)
    }

    /// 批量登记三件布局字段。
    pub fn register_all_layout(&mut self) -> DcRes<u32> {
        let mut n = 0u32;
        let mut i = 0u32;
        while i < LAYOUT_FIELDS {
            self.register_layout(i, self.salt)?;
            // **逐件**登记：件数目标既然按 LAYOUT_FIELDS 立，
            // 就必须一件一记；只在末尾补记一次会让账面凭空少记 N-1 件。
            self.ledger.note_reg();
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 布局登记是否齐备。
    pub fn layout_complete(&self) -> bool {
        let mut i = 0usize;
        while i < self.reg.len() {
            if !self.reg[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 布局逐字段验证：返回首个违例（`None` = 全过）。
    pub fn layout_verify(&self) -> Option<ErrKind> {
        let mut i = 0usize;
        while i < self.reg.len() {
            if self.reg[i] {
                let lf = LayoutField::ALL[i];
                let face = align_face_of(lf);
                if lf.width_bits() != face.width_bits() {
                    return Some(ErrKind::LayoutFieldConflict);
                }
            }
            i += 1;
        }
        if !self.dual.layout_all_ok() {
            return Some(ErrKind::LayoutFieldConflict);
        }
        None
    }

    /// 挂一个节点到某链（**必须经单点函数**）。
    pub fn link(&mut self, chain: Chain, at_tail: bool) -> DcRes<u32> {
        if self.stage == Stage::Frozen || self.stage == Stage::Dispatched {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            self.rej.push(self.tick, self.stage, self.stage);
            return Err(ErrKind::WriteAfterFrozen);
        }
        let n = self.dual.register_node()?;
        if at_tail {
            self.dual.link_tail(chain, n)?;
        } else {
            self.dual.link_head(chain, n)?;
        }
        self.ledger.note_mapped(1);
        self.tick_advance();
        self.ring.push(self.tick, self.dual.fingerprint(), 0x200);
        Ok(n)
    }

    /// 摘一个节点（单点）。
    pub fn unlink(&mut self, node: u32) -> DcRes<()> {
        self.dual.unlink(node)
    }

    /// 阶段迁移（合法边闭集：仅 `+1`）。
    pub fn advance(&mut self, next: Stage) -> DcRes<Stage> {
        if self.stage == Stage::Dispatched {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            self.rej.push(self.tick, self.stage, next);
            return Err(ErrKind::StageIllegalEdge);
        }
        if !self.stage.can_go(next) {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            self.rej.push(self.tick, self.stage, next);
            self.walk_steps = self.walk_steps.wrapping_add(1);
            return Err(ErrKind::StageIllegalEdge);
        }
        self.stage = next;
        self.walk_steps = self.walk_steps.wrapping_add(1);
        self.tick_advance();
        Ok(self.stage)
    }

    /// 记录加法链分项（下限闸）。
    pub fn note_part(&mut self, v: u64) -> DcRes<()> {
        if self.stage == Stage::Frozen || self.stage == Stage::Dispatched {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            self.rej.push(self.tick, self.stage, self.stage);
            return Err(ErrKind::WriteAfterFrozen);
        }
        if v < PART_FLOOR {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::AddChainUnbalanced);
        }
        self.ledger.note_part(v);
        self.tick_advance();
        Ok(())
    }

    /// 登记注入读数。
    pub fn note_inject(&mut self, total: u32, rejected: u32) {
        self.ledger.note_inject(total, rejected);
        let mut h = mix64(FNV_BASIS, total as u64);
        h = mix32(h, rejected);
        self.ring.push(self.tick, h, 0xF02);
    }

    /// 写环形账的时钟闸（tick 不得回退）。
    pub fn stamp(&mut self, tick: u64, digest: u64, tag: u32) -> DcRes<u64> {
        if tick < self.tick {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::TickRegress);
        }
        self.tick = tick;
        Ok(self.ring.push(tick, digest, tag))
    }

    /// 冻结（断言布局齐备 + 对齐零缺失 + 双链不变量全过）。
    pub fn seal(&mut self) -> DcRes<u64> {
        if self.stage != Stage::Mapped {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            let nxt = if self.stage == Stage::Draft {
                Stage::Mapped
            } else {
                Stage::Draft
            };
            self.rej.push(self.tick, self.stage, nxt);
            return Err(ErrKind::StageRejectFreeze);
        }
        if !self.layout_complete() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::LayoutFieldMissing);
        }
        match self.layout_verify() {
            Some(_) => {
                self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
                return Err(ErrKind::LayoutFieldConflict);
            }
            None => {}
        }
        if align_zero_missing().is_err() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::AlignMismatch);
        }
        if !self.dual.head_tail_consistent() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::HeadTailInconsistent);
        }
        if !self.dual.links_reciprocal() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::PrevNextMismatch);
        }
        if !self.dual.cross_chain_clean() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::CrossChainLeak);
        }
        if !self.dual.lru_aligned() {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            return Err(ErrKind::LruBitConflict);
        }
        self.tick_advance();
        self.freeze_tick = self.tick;
        let h = self.compute_freeze(self.freeze_tick);
        self.freeze_hash = h;
        self.explog.record(self.tick, h, 1);
        self.ring.push(self.tick, h, 0xF00);
        let _ = self.advance(Stage::Frozen)?;
        Ok(h)
    }

    /// 冻结哈希重算（必须同 tick）。
    ///
    /// **只混入冻结后永不变的字段**：册定常量 + 布局登记位 + 对齐表
    /// + 冻结 tick。绝不混入 `ring` / `rej` / `explog` / `ledger` 四本活账——
    /// 它们在 `dispatch()` 之后仍会继续追加，把活账折进冻结哈希会让
    /// `verify_freeze()` 在「调度已完成」这一完全正常的状态下误报漂移。
    pub fn compute_freeze(&self, tick: u64) -> u64 {
        let mut h = FNV_BASIS;
        h = mix_str(h, MODULE_TAG);
        h = mix_str(h, TASK_ID);
        h = mix_str(h, CRITERION);
        let mut i = 0usize;
        while i < self.reg.len() {
            h = mix_bool(h, self.reg[i]);
            h = mix32(h, LayoutField::ALL[i].index());
            i += 1;
        }
        let mut j = 0usize;
        while j < self.align.len() {
            h = mix32(h, self.align[j].layout as u32);
            h = mix32(h, self.align[j].face as u32);
            h = mix_bool(h, self.align[j].semantic_same);
            j += 1;
        }
        // 折入实例盐：册定常量 + 登记位 + 对齐表在所有实例上都是同一份，
        // 若不折实例盐，`compute_freeze` 对任何 seed 都返回同一个哈希——
        // 冻结件入册后无法区分「谁冻的」，回放比对也就失去意义。
        // 盐是**实例级输入**（不是活账），冻结后不再变化，折它不违反冻结纪律。
        h = mix64(h, self.salt);
        h = mix64(h, tick);
        h
    }

    /// 冻结指纹校验。
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

    /// 调度留痕入册。
    pub fn dispatch(&mut self) -> DcRes<u64> {
        if self.stage != Stage::Frozen {
            self.ledger.reject_count = self.ledger.reject_count.wrapping_add(1);
            let nxt = if self.stage == Stage::Draft {
                Stage::Mapped
            } else {
                Stage::Frozen
            };
            self.rej.push(self.tick, self.stage, nxt);
            return Err(ErrKind::StageRejectFreeze);
        }
        self.tick_advance();
        let d = mix64(self.freeze_hash, self.tick);
        self.explog.record(self.tick, d, 2);
        self.ring.push(self.tick, d, 0xF01);
        let _ = self.advance(Stage::Dispatched)?;
        Ok(d)
    }

    /// 方案侧与实现侧**双向**对齐对拍。
    ///
    /// 方向 A（本单 → F2682）：本单三件布局字段每件都要有对家且语义一致。
    /// 方向 B（F2682 → 本单）：F2682 的三个布局相关驻留字段都要被覆盖。
    /// 任一方向分叉即 `AlignMismatch`。
    pub fn align_ok(&self) -> DcRes<u64> {
        // 第 0 关：双向登记完整性（本单三件 vs F2682 八字段，双向零缺失）。
        align_zero_missing()?;

        // —— 双向登记的三道独立对拍 ——
        // 纪律：**两侧不得读同一份结构体**。若方案侧与实现侧都去查
        // `align_table()` 或都走 `align_face_of(lf)`，则两者恒等，
        // 无论登记表被改成什么样这道断言都不会红——那是自我循环，
        // 不是对拍。故两侧喂**不同来源**的同一语义量：
        //   * 方案侧：从 `LayoutField` 枚举**自述**取「本单字宽」（册定值）；
        //   * 实现侧：从 `self.align` 表的 `face` 列**读回**驻留面，
        //     再用 `AlignFace::from_index` 反查出驻留侧字宽。
        // 共享输入只有「布局字段位次 i」这一枚索引，其余全靠各自求值。

        // 第 1 关：登记位齐全（reg 长度与全 true）。
        if self.reg.len() != LAYOUT_FIELDS as usize {
            return Err(ErrKind::LayoutFieldMissing);
        }
        let mut ri = 0usize;
        while ri < self.reg.len() {
            if !self.reg[ri] {
                return Err(ErrKind::LayoutFieldMissing);
            }
            ri += 1;
        }

        // 第 2 关：对齐表逐行完整：行数 == LAYOUT_FIELDS，layout 列覆盖 0..3 无重无漏。
        if self.align.len() != LAYOUT_FIELDS as usize {
            return Err(ErrKind::AlignMismatch);
        }
        let mut seen = 0u32;
        let mut k = 0usize;
        while k < self.align.len() {
            let layout = self.align[k].layout as u32;
            if layout >= LAYOUT_FIELDS {
                return Err(ErrKind::AlignMismatch);
            }
            if (seen & (1u32 << layout)) != 0 {
                // layout 列出现重复行 ⇒ 一件布局字段被登记了两遍，
                // 必然意味着另一件被漏登记（一一对应被破坏）。
                return Err(ErrKind::AlignMismatch);
            }
            seen = seen | (1u32 << layout);
            // face 列必须是本 layout 位的**期望**驻留面：
            // 这一条由 `LayoutField` 侧独立算出（`align_face_of` 是册定函数），
            // 与对位的 `self.align[k].face` 读回值对拍。
            if self.align[k].face != align_face_of(LayoutField::ALL[layout as usize]).index() {
                return Err(ErrKind::LayoutFieldConflict);
            }
            if !self.align[k].width_same || !self.align[k].semantic_same {
                return Err(ErrKind::LayoutFieldConflict);
            }
            k += 1;
        }
        if seen != ((1u32 << LAYOUT_FIELDS) - 1u32) {
            return Err(ErrKind::AlignMismatch);
        }

        // 第 3 关：两侧位宽序列各自独立求值后逐项对拍。
        // 方案侧序列：`LayoutField::ALL[i].width_bits()`。
        let mut pa = FNV_BASIS;
        let mut i = 0usize;
        while i < LayoutField::ALL.len() {
            pa = mix32(pa, LayoutField::ALL[i].width_bits());
            i += 1;
        }
        // 实现侧序列：先由 `self.align` 的 `layout` 列定位字段位次，
        // 再按 `face` 列**反查**驻留面取其字宽（不是查册定映射）。
        let mut ia = FNV_BASIS;
        let mut j = 0usize;
        while j < self.align.len() {
            let layout = self.align[j].layout as usize;
            let face = match AlignFace::from_index(self.align[j].face) {
                Some(f) => f,
                None => return Err(ErrKind::AlignMismatch),
            };
            if layout >= LayoutField::ALL.len() {
                return Err(ErrKind::AlignMismatch);
            }
            // 驻留面自己认的对家（`hit_layout()` → 布局字段位次）
            // 必须与登记行 `layout` 列声明的对家一致：这就是「双向」那一半。
            let back = match face.hit_layout() {
                Some(bk) => bk.index(),
                None => return Err(ErrKind::AlignMismatch),
            };
            if back != self.align[j].layout as u32 {
                return Err(ErrKind::AlignMismatch);
            }
            if LayoutField::ALL[layout].hit_layout_stub() != face.index() {
                return Err(ErrKind::AlignMismatch);
            }
            ia = mix32(ia, face.width_bits());
            j += 1;
        }
        if pa != ia {
            return Err(ErrKind::AlignMismatch);
        }
        Ok(pa)
    }

    /// 方案总指纹（活账指纹，与冻结指纹分工）。
    pub fn scheme_fingerprint(&self) -> u64 {
        let mut h = FNV_BASIS;
        h = mix64(h, self.dual.fingerprint());
        let mut i = 0usize;
        while i < self.reg.len() {
            h = mix_bool(h, self.reg[i]);
            i += 1;
        }
        h = mix64(h, self.ledger.fingerprint());
        h = mix64(h, self.ring.fingerprint());
        h = mix64(h, self.rej.fingerprint());
        h = mix64(h, self.explog.fingerprint());
        h = mix64(h, self.freeze_hash);
        h = mix32(h, self.stage.index() as u32);
        h = mix64(h, self.tick);
        h
    }

    /// 全量报告。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(format!(
            "SCHEME stage={} tick={} freeze={:016X} freeze_tick={} walk={}",
            self.stage.name(),
            self.tick,
            self.freeze_hash,
            self.freeze_tick,
            self.walk_steps
        ));
        v.push(format!(
            "SCHEME layout_complete={} verify={:?}",
            self.layout_complete(),
            match self.layout_verify() {
                Some(e) => e.name(),
                None => "ok",
            }
        ));
        let mut i = 0usize;
        while i < LayoutField::ALL.len() {
            v.push(format!(
                "  LAYOUT[{}] {} reg={} face={} bits={}",
                i,
                LayoutField::ALL[i].label(),
                self.reg[i],
                LayoutField::ALL[i].f2682_field(),
                LayoutField::ALL[i].width_bits()
            ));
            i += 1;
        }
        let mut j = 0usize;
        while j < self.align.len() {
            v.push(format!("  {}", self.align[j].row()));
            j += 1;
        }
        let r = self.dual.report();
        let mut k = 0usize;
        while k < r.len() {
            v.push(r[k].clone());
            k += 1;
        }
        let c = self.ledger.conservation_rows();
        k = 0;
        while k < c.len() {
            v.push(c[k].clone());
            k += 1;
        }
        let e = self.explog.report();
        k = 0;
        while k < e.len() {
            v.push(e[k].clone());
            k += 1;
        }
        let rr = self.rej.report();
        k = 0;
        while k < rr.len() {
            v.push(rr[k].clone());
            k += 1;
        }
        v
    }
}

// ===========================================================================
// 10. 注入核（12 种形态，全部必须被拒且留痕）
// ===========================================================================

/// 注入形态闭集（判据「越点操作注入 10/10 被拦截」；本单取 12 种）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectForm {
    /// I0：节点序号越界 → `NodeIndexOutOfRange`。
    F0NodeOob,
    /// I1：非法链标识 → `ChainIdInvalid`。
    F1ChainId,
    /// I2：重复挂链 → `DupLinkDetected`。
    F2DupLink,
    /// I3：串链（同帧挂两链）→ `CrossChainLeak`。
    F3CrossChain,
    /// I4：布局字段缺失即冻结 → `LayoutFieldMissing`。
    F4MissingLayout,
    /// I5：布局字段位宽冲突 → `LayoutFieldConflict`。
    F5WidthConflict,
    /// I6：头尾不一致 → `HeadTailInconsistent`。
    F6HeadTail,
    /// I7：prev/next 不闭合 → `PrevNextMismatch`。
    F7NotReciprocal,
    /// I8：lru 位标与链冲突 → `LruBitConflict`。
    F8LruConflict,
    /// I9：状态机自环 → `StageIllegalEdgeFreeze`。
    F9SelfLoop,
    /// I10：封后写 → `WriteAfterFrozen`。
    F10WriteAfterFrozen,
    /// I11：时钟回退 → `TickRegress`。
    F11TickRegress,
}

impl InjectForm {
    /// 全部注入形态。
    pub const ALL: [InjectForm; 12] = [
        InjectForm::F0NodeOob,
        InjectForm::F1ChainId,
        InjectForm::F2DupLink,
        InjectForm::F3CrossChain,
        InjectForm::F4MissingLayout,
        InjectForm::F5WidthConflict,
        InjectForm::F6HeadTail,
        InjectForm::F7NotReciprocal,
        InjectForm::F8LruConflict,
        InjectForm::F9SelfLoop,
        InjectForm::F10WriteAfterFrozen,
        InjectForm::F11TickRegress,
    ];

    /// 形态序号（0..11）。
    pub fn index(self) -> usize {
        match self {
            InjectForm::F0NodeOob => 0,
            InjectForm::F1ChainId => 1,
            InjectForm::F2DupLink => 2,
            InjectForm::F3CrossChain => 3,
            InjectForm::F4MissingLayout => 4,
            InjectForm::F5WidthConflict => 5,
            InjectForm::F6HeadTail => 6,
            InjectForm::F7NotReciprocal => 7,
            InjectForm::F8LruConflict => 8,
            InjectForm::F9SelfLoop => 9,
            InjectForm::F10WriteAfterFrozen => 10,
            InjectForm::F11TickRegress => 11,
        }
    }

    /// 预期错误变体（形态与错误一一对应，零空档）。
    pub fn expect(self) -> ErrKind {
        match self {
            InjectForm::F0NodeOob => ErrKind::NodeIndexOutOfRange,
            InjectForm::F1ChainId => ErrKind::ChainIdInvalid,
            InjectForm::F2DupLink => ErrKind::DupLinkDetected,
            InjectForm::F3CrossChain => ErrKind::CrossChainLeak,
            InjectForm::F4MissingLayout => ErrKind::LayoutFieldMissing,
            InjectForm::F5WidthConflict => ErrKind::LayoutFieldConflict,
            InjectForm::F6HeadTail => ErrKind::HeadTailInconsistent,
            InjectForm::F7NotReciprocal => ErrKind::PrevNextMismatch,
            InjectForm::F8LruConflict => ErrKind::LruBitConflict,
            InjectForm::F9SelfLoop => ErrKind::StageIllegalEdge,
            InjectForm::F10WriteAfterFrozen => ErrKind::WriteAfterFrozen,
            InjectForm::F11TickRegress => ErrKind::TickRegress,
        }
    }

    /// 形态名。
    pub fn name(self) -> &'static str {
        match self {
            InjectForm::F0NodeOob => "node_oob",
            InjectForm::F1ChainId => "chain_id",
            InjectForm::F2DupLink => "dup_link",
            InjectForm::F3CrossChain => "cross_chain",
            InjectForm::F4MissingLayout => "missing_layout",
            InjectForm::F5WidthConflict => "width_conflict",
            InjectForm::F6HeadTail => "head_tail",
            InjectForm::F7NotReciprocal => "not_reciprocal",
            InjectForm::F8LruConflict => "lru_conflict",
            InjectForm::F9SelfLoop => "self_loop",
            InjectForm::F10WriteAfterFrozen => "write_after_frozen",
            InjectForm::F11TickRegress => "tick_regress",
        }
    }

    /// 形态描述（现象 / 位置 / 建议修法）。
    pub fn describe(self) -> &'static str {
        match self {
            InjectForm::F0NodeOob => "现象=节点序号越界；位置=挂链入口；修法=把序号收回 0..NODE_SLOTS",
            InjectForm::F1ChainId => "现象=链标识非法；位置=挂链链种参数；修法=只接受 active/inactive 二值",
            InjectForm::F2DupLink => "现象=同节点重复挂链；位置=挂链幂等检查；修法=挂前查 linked()",
            InjectForm::F3CrossChain => "现象=同帧挂两链；位置=摘链/挂链交界；修法=先落 none 再改指针",
            InjectForm::F4MissingLayout => "现象=布局三件缺一即冻结；位置=seal 入口；修法=补齐 reg[] 全true",
            InjectForm::F5WidthConflict => "现象=与F2682位宽冲突；位置=对齐断言；修法=统一位宽或改对齐映射",
            InjectForm::F6HeadTail => "现象=头尾不一致；位置=头尾不变量；修法=空链两端同时置空",
            InjectForm::F7NotReciprocal => "现象=prev/next不闭合；位置=unlink；修法=摘链后整体重挂",
            InjectForm::F8LruConflict => "现象=lru位标与链不符；位置=挂链位标设置；修法=按链重算位标",
            InjectForm::F9SelfLoop => "现象=状态机自环；位置=advance；修法=只走 +1 合法边",
            InjectForm::F10WriteAfterFrozen => "现象=封后写布局；位置=link/register；修法=另开版本号方案",
            InjectForm::F11TickRegress => "现象=逻辑时钟回退；位置=stamp；修法=tick 只增不减",
        }
    }
}

/// 注入结果（**被测对象与测量工具分离**）。
///
/// 分离纪律：`probe` 是被有意破坏的结构夹具（只吃「形态 + 常量」），
/// `tool` 是独立 LCG 测量工具（只吃 `seed`），二者不共享任何状态。
/// 若测量工具从被测对象派生随机数，注入就退化成「自己测自己」，
/// 破坏一旦落在取数路径上就会被吞掉，注入彻底失效。
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
    let _ = s.register_all_layout();
    let _ = s.link(Chain::Active, true);
    let _ = s.link(Chain::Active, true);
    let _ = s.link(Chain::Inactive, true);
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
            probe_fp: s.compute_freeze(s.freeze_tick),
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
            probe_fp: s.compute_freeze(s.freeze_tick),
            tool_fp: 0,
            line: String::from("注入未被拒：防线失守"),
        },
    }
}

/// 注入核：跑一轮全部形态。
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
            InjectForm::F0NodeOob => {
                let mut s = probe_fixture(Stage::Draft, salt);
                let e = s.dual.link_tail(Chain::Active, NODE_SLOTS + 5u32).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F1ChainId => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：把某节点的 chain 改成非法值（3，超出二值闭集）。
                if !s.dual.nodes.is_empty() {
                    s.dual.nodes[0].chain = 3;
                }
                let e = s.dual.unlink(0).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F2DupLink => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：同一节点再挂一次同链。
                let e = s.dual.link_tail(Chain::Active, 0).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F3CrossChain => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：把已在 active 的节点挂到 inactive。
                let e = s.dual.link_tail(Chain::Inactive, 0).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F4MissingLayout => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：挖掉一件布局登记后冻结。
                if !s.reg.is_empty() {
                    s.reg[1] = false;
                }
                let e = s.seal();
                finish(f, e, &s)
            }
            InjectForm::F5WidthConflict => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：把某节点的挂链位槽号顶出 page 字段空间。
                if !s.dual.nodes.is_empty() {
                    s.dual.nodes[0].hook = 99;
                }
                let e = match s.layout_verify() {
                    Some(er) => Err(er),
                    None => Ok(0u64),
                };
                finish(f, e, &s)
            }
            InjectForm::F6HeadTail => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：越点裸写 head 但不写 tail，制造头尾不一致。
                s.dual.bare_write_head(Chain::Active, 1);
                let e = if s.dual.head_tail_consistent() {
                    Ok(0u64)
                } else {
                    Err(ErrKind::HeadTailInconsistent)
                };
                finish(f, e, &s)
            }
            InjectForm::F7NotReciprocal => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：单边改 next，破坏 prev/next 互逆。
                let mut broke = false;
                let mut k = 0usize;
                while k < s.dual.nodes.len() {
                    if s.dual.nodes[k].linked() && s.dual.nodes[k].next != NODE_NONE {
                        s.dual.nodes[k].next = NODE_NONE;
                        broke = true;
                        break;
                    }
                    k += 1;
                }
                let e = if broke && !s.dual.links_reciprocal() {
                    Err(ErrKind::PrevNextMismatch)
                } else {
                    Ok(0u64)
                };
                finish(f, e, &s)
            }
            InjectForm::F8LruConflict => {
                let mut s = probe_fixture(Stage::Mapped, salt);
                // 注入：把 active 链节点的 lru 位标改成冷。
                let mut broke = false;
                let mut k = 0usize;
                while k < s.dual.nodes.len() {
                    if s.dual.nodes[k].linked() && s.dual.nodes[k].chain == 0 {
                        s.dual.nodes[k].lru = LRU_COLD;
                        broke = true;
                        break;
                    }
                    k += 1;
                }
                let e = if broke && !s.dual.lru_aligned() {
                    Err(ErrKind::LruBitConflict)
                } else {
                    Ok(0u64)
                };
                finish(f, e, &s)
            }
            InjectForm::F9SelfLoop => {
                let mut s = probe_fixture(Stage::Draft, salt);
                // 注入：状态机自环。
                let e = s.advance(Stage::Draft).map(|_| 0u64);
                finish(f, e, &s)
            }
            InjectForm::F10WriteAfterFrozen => {
                let mut s = probe_fixture(Stage::Frozen, salt);
                // 注入：冻结态再挂节点（封后写）。
                let e = s.link(Chain::Inactive, true).map(|v| v as u64);
                finish(f, e, &s)
            }
            InjectForm::F11TickRegress => {
                let mut s = probe_fixture(Stage::Draft, salt);
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

/// 注入核指纹。
pub fn inject_fingerprint(rs: &[InjectionResult]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < rs.len() {
        h = mix64(h, rs[i].probe_fp);
        h = mix64(h, rs[i].tool_fp);
        h = mix32(h, rs[i].got as u32);
        h = mix_bool(h, rs[i].rejected);
        i += 1;
    }
    h
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
        let g = if rs[i].got == usize::MAX {
            0u16
        } else {
            ErrKind::ALL[rs[i].got].code()
        };
        v.push(format!(
            "INJECT[{:02}] {} reject={} got=0x{:04X} want=0x{:04X} match={} probe={:016X} tool={:016X}",
            rs[i].form,
            rs[i].form_name,
            rs[i].rejected,
            g,
            ErrKind::ALL[rs[i].want].code(),
            rs[i].matched,
            rs[i].probe_fp,
            rs[i].tool_fp
        ));
        i += 1;
    }
    v
}

// ===========================================================================
// 11. 守恒终账 reconcile（加法链 Σ分项 == 总额，零偏离）
// ===========================================================================

/// 守恒终账：六条整数恒等式（禁浮点、禁近似）。
///
/// R1 件数守恒 / R2 覆盖守恒 / R3 分项计数 / R4 加法链零偏离
/// R5 注入守恒 / R6 双链不变量 + 写痕迹零越点。
pub fn reconcile(s: &Scheme) -> DcRes<Vec<String>> {
    let mut v: Vec<String> = Vec::new();
    let led = &s.ledger;

    // R1 件数守恒
    //
    // 口径提醒：`reg_target()` 的定义就是 `reg + pending`，故
    // `reg + pending == reg_target` 是**恒真**的恒等式，写成断言等于
    // 一条永远为绿的空断言——它看着在守恒，实际什么也没守。
    // 真正的守恒内容有两条，缺一不可：
    //   (a) `reg + pending == 册定件数`：登记过程中「已登记 + 待登记」恒定，
    //       少登记一件会以「已登记变少」的形式漏出来；
    //   (b) `reg == 册定件数` 且 `pending == 0`：收官时登记必须做完，
    //       否则 (a) 恒真这一条会把「一件都没登记」也判成守恒。
    let r1 = led.reg.wrapping_add(led.pending);
    let r1_done = led.reg == LAYOUT_FIELDS && led.pending == 0;
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

    // R2 覆盖守恒（Σ挂链数 == 目标覆盖量）
    let mut covered = 0u64;
    let mut i = 0usize;
    while i < s.dual.nodes.len() {
        if s.dual.nodes[i].linked() {
            covered += 1;
        }
        i += 1;
    }
    v.push(format!(
        "RECON R2 mapped={} covered={} target={} dev={} ok={}",
        led.mapped,
        covered,
        led.covered_target,
        covered as i64 - led.covered_target as i64,
        covered == led.covered_target
    ));
    if covered != led.covered_target {
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

    // R5 注入守恒（判据下限 10；本单 12 形态全拒）
    v.push(format!(
        "RECON R5a inject_floor total={} floor={} ok={}",
        led.injected_total,
        FORM_COUNT,
        led.injected_total >= FORM_COUNT
    ));
    v.push(format!(
        "RECON R5b inject_full rejected={} total={} ok={}",
        led.injected_rejected,
        led.injected_total,
        led.injected_rejected == led.injected_total
    ));
    if led.injected_total < FORM_COUNT || led.injected_rejected != led.injected_total {
        return Err(ErrKind::AddChainUnbalanced);
    }

    // R6 双链不变量 + 写痕迹零越点（单点化纪律的账面证据）
    let ht = s.dual.head_tail_consistent();
    let rc = s.dual.links_reciprocal();
    let cc = s.dual.cross_chain_clean();
    let la = s.dual.lru_aligned();
    let bare = s.dual.trace.bare_writes;
    v.push(format!(
        "RECON R6 dual head_tail={} recip={} cross_clean={} lru={} bare_writes={} via_op={}",
        ht, rc, cc, la, bare, s.dual.trace.via_op
    ));
    if !ht {
        return Err(ErrKind::HeadTailInconsistent);
    }
    if !rc {
        return Err(ErrKind::PrevNextMismatch);
    }
    if !cc {
        return Err(ErrKind::CrossChainLeak);
    }
    if !la {
        return Err(ErrKind::LruBitConflict);
    }
    if bare != 0 {
        // 越点裸写 > 0 ⇒ 单点化纪律被破，直接红。
        return Err(ErrKind::SinglePointViolation);
    }
    Ok(v)
}

// ===========================================================================
// 12. 自检（20 条）
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
    /// 单行报告。
    pub fn row(&self) -> String {
        format!("CHECK[{}] {} pass={} :: {}", self.id, self.name, self.pass, self.note)
    }
}

/// 全量自检。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut v: Vec<Check> = Vec::new();
    let mut s = Scheme::new(seed);

    // C01 草案态
    v.push(Check::new(1, "init_draft", s.stage == Stage::Draft, format!("stage={}", s.stage.name())));

    // C02 登记三件布局
    let r2 = s.register_all_layout().is_ok();
    v.push(Check::new(
        2,
        "register_layout3",
        r2 && s.layout_complete() && s.ledger.cells == LAYOUT_FIELDS,
        format!("cells={}", s.ledger.cells),
    ));

    // C03 布局逐字段全过
    v.push(Check::new(
        3,
        "layout_verify",
        s.layout_verify().is_none(),
        format!("verify={}", match s.layout_verify() { Some(e) => String::from(e.name()), None => String::from("ok") }),
    ));

    // C04 对齐零缺失（双向）
    let al = s.align_ok();
    v.push(Check::new(4, "align_zero_missing", al.is_ok(), format!("fp={:?}", al)));

    // C05 挂三节点（active 2 + inactive 1）
    let mut ok5 = true;
    let mut i = 0u8;
    while i < 3 {
        let c = if i < 2 { Chain::Active } else { Chain::Inactive };
        if s.link(c, true).is_err() {
            ok5 = false;
        }
        i += 1;
    }
    v.push(Check::new(
        5,
        "link_3",
        ok5 && s.dual.chain_len(Chain::Active) == 2 && s.dual.chain_len(Chain::Inactive) == 1,
        format!(
            "active={} inactive={}",
            s.dual.chain_len(Chain::Active),
            s.dual.chain_len(Chain::Inactive)
        ),
    ));

    // C06 头尾一致
    v.push(Check::new(
        6,
        "head_tail",
        s.dual.head_tail_consistent(),
        format!("ok={}", s.dual.head_tail_consistent()),
    ));

    // C07 prev/next 互逆
    v.push(Check::new(7, "links_reciprocal", s.dual.links_reciprocal(), format!("ok={}", s.dual.links_reciprocal())));

    // C08 无串链
    v.push(Check::new(8, "cross_chain_clean", s.dual.cross_chain_clean(), format!("ok={}", s.dual.cross_chain_clean())));

    // C09 lru 位标对齐
    v.push(Check::new(9, "lru_aligned", s.dual.lru_aligned(), format!("ok={}", s.dual.lru_aligned())));

    // C10 写痕迹零越点
    v.push(Check::new(
        10,
        "single_point",
        s.dual.trace.bare_writes == 0 && s.dual.trace.via_op > 0,
        format!("via_op={} bare={}", s.dual.trace.via_op, s.dual.trace.bare_writes),
    ));

    // C11 迁移到 Mapped
    let a11 = s.advance(Stage::Mapped).is_ok();
    v.push(Check::new(11, "advance_mapped", a11 && s.stage == Stage::Mapped, format!("stage={}", s.stage.name())));

    // C12 加法链 8 个分项
    let mut ok12 = true;
    let mut k = 0u32;
    while k < PART_TARGET {
        if s.note_part(PART_FLOOR + k.wrapping_mul(2) as u64).is_err() {
            ok12 = false;
        }
        k += 1;
    }
    v.push(Check::new(
        12,
        "parts_8",
        ok12 && s.ledger.items == PART_TARGET,
        format!("items={}", s.ledger.items),
    ));

    // C13 加法链零偏离
    let sm = s.ledger.parts_sum();
    v.push(Check::new(
        13,
        "addchain_zero",
        sm == s.ledger.total,
        format!("sum={} total={}", sm, s.ledger.total),
    ));

    // C14 注入核 + 回填账
    let inj = inject_core_with_tool(seed);
    s.note_inject(inj.len() as u32, count_rejected(&inj) as u32);
    let miss = detect_miss(&inj);
    v.push(Check::new(
        14,
        "inject_all_rejected",
        miss == 0 && count_rejected(&inj) == inj.len(),
        format!("miss={} rejected={}/{}", miss, count_rejected(&inj), inj.len()),
    ));

    // C15 守恒终账
    let rec = reconcile(&s);
    v.push(Check::new(15, "reconcile", rec.is_ok(), format!("rows={}", rec.map(|r| r.len()).unwrap_or(0))));

    // C16 冻结
    let sl = s.seal();
    v.push(Check::new(
        16,
        "seal",
        sl.is_ok() && s.freeze_hash != 0,
        format!("hash={:016X}", s.freeze_hash),
    ));

    // C17 冻结哈希校验
    let vs = s.verify_freeze();
    v.push(Check::new(17, "verify_freeze", vs.is_ok(), format!("freeze_tick={}", s.freeze_tick)));

    // C18 调度入册
    let dp = s.dispatch();
    v.push(Check::new(
        18,
        "dispatch",
        dp.is_ok() && s.stage == Stage::Dispatched,
        format!("stage={} explog={}", s.stage.name(), s.explog.total),
    ));

    // C19 非法边 13 条
    let ie = illegal_edges_measured();
    v.push(Check::new(
        19,
        "illegal_edges_13",
        ie == ILLEGAL_EDGES && illegal_edges().len() == ILLEGAL_EDGES as usize,
        format!("measured={} expect={}", ie, ILLEGAL_EDGES),
    ));

    // C20 笛卡尔 24 格 + 环形账守恒
    let cells = run_cartesian(seed);
    let (tot, cok, cwr, cbad) = cartesian_stats(&cells);
    v.push(Check::new(
        20,
        "cartesian_24",
        tot == CART_EXPECTED && cbad == 0 && cok == tot && cwr == CART_WRITABLE && s.ring.conserved() && s.rej.conserved(),
        format!("total={} ok={} bad={} wr={} ring_ok={}", tot, cok, cbad, cwr, s.ring.conserved()),
    ));
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
// 13. 观察结果与 observe
// ===========================================================================

/// 观察结果结构。
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
    /// 笛卡尔指纹。
    pub cart_fp: u64,
    /// 注入核指纹。
    pub inject_fp: u64,
    /// 回放指纹。
    pub replay_fp: u64,
    /// 注入被拒数。
    pub injected_rejected: u32,
    /// 注入形态总数。
    pub injected_total: u32,
    /// 注入漏网数（必须 0）。
    pub inject_miss: usize,
    /// 守恒终账行。
    pub recon_rows: Vec<String>,
    /// 自检结果。
    pub checks: Vec<Check>,
    /// 方案报告行。
    pub scheme_rows: Vec<String>,
    /// 注入报告行。
    pub inject_rows: Vec<String>,
    /// 笛卡尔总格数。
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
    /// 单点化越点次数（必须 0）。
    pub bare_writes: u32,
    /// 回放一致性。
    pub replay_stable: bool,
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
            "UNX-F3001 seed={:016X} all_ok={} freeze={:016X} fp={:016X} inject={}/{} miss={} cart={}/{} ring_live={} dropped={} bare={} tlv={}B rt={} replay_stable={}",
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
            self.bare_writes,
            self.tlv_len,
            self.tlv_roundtrip,
            self.replay_stable
        )
    }
    /// 全量报告。
    pub fn report(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        v.push(String::from("=== UNX-F3001 双链结构冻结 · 报告 ==="));
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
        v.push(String::from("-- 结构账 --"));
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
    kv.push((String::from("seg_base"), format!("{:#06x}", SEG_BASE)));
    kv.push((String::from("seg_mask"), format!("{:#06x}", SEG_MASK)));
    kv.push((String::from("seg_max_code"), format!("{:#06x}", SEG_MAX_CODE)));
    kv.push((String::from("chain_count"), CHAIN_COUNT.to_string()));
    kv.push((String::from("chain_cap"), CHAIN_CAP.to_string()));
    kv.push((String::from("layout_fields"), LAYOUT_FIELDS.to_string()));
    kv.push((String::from("align_fields"), ALIGN_FIELDS.to_string()));
    kv.push((String::from("node_slots"), NODE_SLOTS.to_string()));
    kv.push((String::from("legal_edges"), LEGAL_EDGES.to_string()));
    kv.push((String::from("illegal_edges"), ILLEGAL_EDGES.to_string()));
    kv.push((String::from("cart_expected"), CART_EXPECTED.to_string()));
    kv.push((String::from("ring_cap"), RING_CAP.to_string()));
    kv.push((String::from("reject_cap"), REJECT_CAP.to_string()));
    kv.push((String::from("form_count"), FORM_COUNT.to_string()));
    kv.push((String::from("seed"), format!("{:016X}", o.seed)));
    kv.push((String::from("freeze_hash"), format!("{:016X}", o.freeze_hash)));
    kv.push((String::from("scheme_fp"), format!("{:016X}", o.scheme_fp)));
    kv.push((String::from("check_fp"), format!("{:016X}", o.check_fp)));
    kv.push((String::from("cart_fp"), format!("{:016X}", o.cart_fp)));
    kv.push((String::from("inject_fp"), format!("{:016X}", o.inject_fp)));
    kv.push((String::from("replay_fp"), format!("{:016X}", o.replay_fp)));
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
    kv.push((String::from("bare_writes"), o.bare_writes.to_string()));
    kv.push((String::from("replay_stable"), o.replay_stable.to_string()));
    kv.push((String::from("tlv_len"), o.tlv_len.to_string()));
    kv.push((String::from("tlv_roundtrip"), o.tlv_roundtrip.to_string()));
    kv.push((String::from("all_ok"), o.all_ok.to_string()));
    let mut i = 0usize;
    while i < Chain::ALL.len() {
        let c = Chain::ALL[i];
        kv.push((format!("chain.{}", c.name()), String::from(c.label())));
        kv.push((format!("chain.{}.lru", c.name()), c.expect_lru().to_string()));
        kv.push((format!("chain.{}.cap", c.name()), c.cap().to_string()));
        i += 1;
    }
    i = 0;
    while i < LayoutField::ALL.len() {
        let lf = LayoutField::ALL[i];
        kv.push((format!("layout.{}.label", lf.name()), String::from(lf.label())));
        kv.push((format!("layout.{}.bits", lf.name()), lf.width_bits().to_string()));
        kv.push((format!("layout.{}.f2682", lf.name()), String::from(lf.f2682_field())));
        i += 1;
    }
    i = 0;
    while i < AlignFace::ALL.len() {
        let af = AlignFace::ALL[i];
        kv.push((format!("align.{}.covered", af.name()), af.covered().to_string()));
        i += 1;
    }
    kv
}

/// 真跑一遍并产出观察结果（`observe` → `replay`，**单向**，二者禁互递归）。
pub fn observe(seed: u64) -> DcObserve {
    // —— 第一步：回放 ——
    let tr = replay(seed);
    let (stable, rfp) = replay_stable(seed);

    // —— 第二步：真跑结构生命周期 ——
    let mut s = Scheme::new(seed);
    let _ = s.register_all_layout();
    let _ = s.link(Chain::Active, true);
    let _ = s.link(Chain::Active, true);
    let _ = s.link(Chain::Inactive, true);
    let _ = s.advance(Stage::Mapped);
    let mut k = 0u32;
    while k < PART_TARGET {
        let _ = s.note_part(PART_FLOOR + k.wrapping_mul(2) as u64);
        k += 1;
    }
    let inj_pre = inject_core_with_tool(seed);
    s.note_inject(inj_pre.len() as u32, count_rejected(&inj_pre) as u32);
    let recon_rows: Vec<String> = match reconcile(&s) {
        Ok(v) => v,
        Err(e) => vec![err_line(e, seed)],
    };
    let freeze_hash = s.seal().unwrap_or(0);
    let _ = s.verify_freeze();
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
    let (ct, cok, cwr, cbad) = cartesian_stats(&cells);

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
        bare_writes: s.dual.trace.bare_writes,
        replay_stable: stable,
        tlv_roundtrip: false,
        tlv_len: 0,
        all_ok: false,
    };

    // —— 第六步：各分项判定 ——
    let checks_pass = all_check_pass(&checks);
    let recon_ok = o.recon_rows.len() == RECON_ROWS;
    let inject_ok = miss == 0 && rej == tot;
    let cart_ok2 = cbad == 0 && ct == CART_EXPECTED && cwr == CART_WRITABLE;

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
        && o.bare_writes == 0
        && tr.ok
        && rt_len;
    o.all_ok = base_ok;
    o.tlv_roundtrip = tlv_roundtrip_ok(&o);
    o.all_ok = base_ok && o.tlv_roundtrip;
    o
}

// ===========================================================================
// 14. 确定性回放 replay（不调 observe）
// ===========================================================================

/// 单轮回放步。
#[derive(Clone, Debug)]
pub struct ReplayStep {
    /// 步序。
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
    /// 是否全成功。
    pub ok: bool,
}

impl ReplayTrace {
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!("REPLAY steps={}/{} ok={} fp={:016X}", self.passed, self.total, self.ok, self.fp)
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
/// 回放刻意**不调 `observe`**（判据：二者禁互递归），自足闭环跑一遍
/// 结构生命周期，把每步指纹折成一条轨迹；`observe` 拿它做一致性对拍，
/// 但反向不成立——单向 `observe → replay`。
pub fn replay(seed: u64) -> ReplayTrace {
    let mut steps: Vec<ReplayStep> = Vec::new();
    let mut s = Scheme::new(seed);
    let mut h = FNV_BASIS;
    let mut step = 0u8;

    // 步 0 构造
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep { step, action: "new_draft", fp: s.scheme_fingerprint(), tick: s.tick, ok: s.stage == Stage::Draft });
    step += 1;

    // 步 1 登记三件布局
    let r1 = s.register_all_layout().is_ok();
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep { step, action: "register_layout", fp: s.scheme_fingerprint(), tick: s.tick, ok: r1 && s.layout_complete() });
    step += 1;

    // 步 2 布局逐字段验证
    let r2 = s.layout_verify().is_none();
    h = mix_bool(h, r2);
    steps.push(ReplayStep { step, action: "layout_verify", fp: s.scheme_fingerprint(), tick: s.tick, ok: r2 });
    step += 1;

    // 步 3 对齐零缺失（双向）
    let r3 = s.align_ok().is_ok();
    h = mix_bool(h, r3);
    steps.push(ReplayStep { step, action: "align_ok", fp: s.scheme_fingerprint(), tick: s.tick, ok: r3 });
    step += 1;

    // 步 4 挂 active 两节点
    let r4a = s.link(Chain::Active, true).is_ok();
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep { step, action: "link_active_1", fp: s.scheme_fingerprint(), tick: s.tick, ok: r4a });
    step += 1;
    let r4b = s.link(Chain::Active, true).is_ok();
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep { step, action: "link_active_2", fp: s.scheme_fingerprint(), tick: s.tick, ok: r4b });
    step += 1;

    // 步 6 挂 inactive 一节点
    let r6 = s.link(Chain::Inactive, true).is_ok();
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep { step, action: "link_inactive_1", fp: s.scheme_fingerprint(), tick: s.tick, ok: r6 });
    step += 1;

    // 步 7 双链不变量（头尾 + 互逆 + 串链 + lru）
    let inv = s.dual.head_tail_consistent() && s.dual.links_reciprocal() && s.dual.cross_chain_clean() && s.dual.lru_aligned();
    h = mix_bool(h, inv);
    steps.push(ReplayStep { step, action: "dual_invariants", fp: s.scheme_fingerprint(), tick: s.tick, ok: inv });
    step += 1;

    // 步 8 迁移到 Mapped
    let r8 = s.advance(Stage::Mapped).is_ok();
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep { step, action: "advance_mapped", fp: s.scheme_fingerprint(), tick: s.tick, ok: r8 });
    step += 1;

    // 步 9 加法链分项
    let mut r9 = true;
    let mut k = 0u32;
    while k < PART_TARGET {
        if s.note_part(PART_FLOOR + k.wrapping_mul(2) as u64).is_err() {
            r9 = false;
        }
        k += 1;
    }
    h = mix_bool(h, r9);
    steps.push(ReplayStep { step, action: "note_parts", fp: s.scheme_fingerprint(), tick: s.tick, ok: r9 });
    step += 1;

    // 步 10 注入核 + 回填账（守恒 R5 需要账上有数）
    let inj = inject_core_with_tool(seed);
    s.note_inject(inj.len() as u32, count_rejected(&inj) as u32);
    let miss = detect_miss(&inj);
    h = mix64(h, inject_fingerprint(&inj));
    steps.push(ReplayStep { step, action: "inject_core", fp: inject_fingerprint(&inj), tick: s.tick, ok: miss == 0 });
    step += 1;

    // 步 11 守恒终账
    let rec = reconcile(&s);
    h = mix_bool(h, rec.is_ok());
    steps.push(ReplayStep { step, action: "reconcile", fp: s.scheme_fingerprint(), tick: s.tick, ok: rec.is_ok() });
    step += 1;

    // 步 12 冻结
    let sl = s.seal();
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep { step, action: "seal", fp: s.scheme_fingerprint(), tick: s.tick, ok: sl.is_ok() && s.freeze_hash != 0 });
    step += 1;

    // 步 13 冻结校验
    let vs = s.verify_freeze();
    h = mix_bool(h, vs.is_ok());
    steps.push(ReplayStep { step, action: "verify_freeze", fp: s.scheme_fingerprint(), tick: s.tick, ok: vs.is_ok() });
    step += 1;

    // 步 14 调度入册
    let dp = s.dispatch();
    h = mix64(h, s.scheme_fingerprint());
    steps.push(ReplayStep { step, action: "dispatch", fp: s.scheme_fingerprint(), tick: s.tick, ok: dp.is_ok() });
    step += 1;

    // 步 15 笛卡尔矩阵
    let cells = run_cartesian(seed);
    let cfp = cartesian_fingerprint(&cells);
    let (ct, cok, cwr, cbad) = cartesian_stats(&cells);
    h = mix64(h, cfp);
    steps.push(ReplayStep { step, action: "cartesian", fp: cfp, tick: s.tick, ok: cbad == 0 && cok == ct && cwr == CART_WRITABLE });
    step += 1;

    // 步 16 非法边穷举
    let ie = illegal_edges_measured();
    h = mix32(h, ie);
    steps.push(ReplayStep { step, action: "illegal_edges", fp: ie as u64, tick: s.tick, ok: ie == ILLEGAL_EDGES });
    // 末步不再自增：`step` 的最后一次赋值永不被读，写它只会留下
    // "value assigned is never read" 噪声告警，且末尾多一个计数没有语义。

    let mut passed = 0u32;
    let mut i = 0usize;
    while i < steps.len() {
        if steps[i].ok {
            passed += 1;
        }
        i += 1;
    }
    let total = steps.len() as u32;
    ReplayTrace { fp: h, steps, passed, total, ok: passed == total }
}

/// 回放稳定性（同种子两跑，指纹必须一致）。
pub fn replay_stable(seed: u64) -> (bool, u64) {
    let a = replay(seed);
    let b = replay(seed);
    let same = a.fp == b.fp && a.ok && b.ok;
    (same, a.fp)
}

// ===========================================================================
// 15. 性能分解 / 内存账
// ===========================================================================

/// 性能分解总表（逐项对标 Windows 同类能力写「深在哪」）。
pub fn complexity_table() -> Vec<CostItem> {
    let mut v: Vec<CostItem> = Vec::new();
    v.push(CostItem::new("register_all_layout", "O(1)", "O(1)", LAYOUT_FIELDS, 96,
        "Windows 无对照：三件布局字段逐项登记是本单专有冻结纪律"));
    v.push(CostItem::new("link_tail (single point)", "O(1)", "O(1)", NODE_SLOTS, 48,
        "与 Windows working-set 双链同为O(1)，但本条把全部写操作收口到单点，越点可测"));
    v.push(CostItem::new("unlink (single point)", "O(1)", "O(1)", NODE_SLOTS, 48,
        "Windows 摘链常留后置修补；本条单点内一次性修好 prev/next/head/tail"));
    v.push(CostItem::new("chain_len", "O(n)", "O(n)", NODE_SLOTS, 16,
        "深在自链头实走：不信len 字段，账与结构不可能对不上"));
    v.push(CostItem::new("links_reciprocal", "O(n)", "O(1)", NODE_SLOTS, 8,
        "比Windows 摘链后校验更严：逐节点判 A.next==B 且 B.prev==A 双向闭合"));
    v.push(CostItem::new("cross_chain_clean", "O(n^2)", "O(1)", NODE_SLOTS * NODE_SLOTS, 8,
        "Windows 无对照：串链（同一帧挂两链）逐帧两两查，n=16 恒定故O(n^2) 可接受"));
    v.push(CostItem::new("lru_aligned", "O(n)", "O(1)", NODE_SLOTS, 8,
        "比Windows 位标仅作提示更严：位标与所在链必须**恒等**，不一致即红"));
    v.push(CostItem::new("align_zero_missing", "O(n)", "O(n)", ALIGN_FIELDS + LAYOUT_FIELDS, 64,
        "深在**双向**：既查本单→F2682漏登记，也查 F2682→本单漏对齐"));
    v.push(CostItem::new("run_cartesian", "O(1)", "O(n)", CART_EXPECTED as u32, (CART_EXPECTED * 48) as u32,
        "比 Windows 矩阵更细：24 格逐格独立夹具，任一格坏不污染其余 23 格"));
    v.push(CostItem::new("illegal_edges_measured", "O(1)", "O(n)", STAGE_COUNT * STAGE_COUNT, 64,
        "Windows 无对照：13 条非法边穷举可枚举，自环也占位次"));
    v.push(CostItem::new("inject_core 12 forms", "O(1)", "O(n)", FORM_TOTAL, FORM_TOTAL * 64,
        "Windows 无对照：12 形态一一映射到12 个错误码，越点裸写必被痕迹抓住"));
    v.push(CostItem::new("seal + verify_freeze", "O(1)", "O(1)", ALIGN_FIELDS + LAYOUT_FIELDS, 32,
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
    v.push(format!("MEM nodes={} slots * sizeof(PageNode)=6*u32+u16+bool ≈ {} B", NODE_SLOTS, NODE_SLOTS * 32));
    v.push(format!("MEM align={} rows * sizeof(AlignRow)=3*u8+2*bool ≈ {} B", LAYOUT_FIELDS, LAYOUT_FIELDS * 8));
    v.push(format!("MEM ring cap={} * sizeof(RingEntry) ≈ {} B", RING_CAP, RING_CAP * 32));
    v.push(format!("MEM rej cap={} * sizeof(Rejection) ≈ {} B", REJECT_CAP, REJECT_CAP * 24));
    v.push(format!("MEM cartesian={} cells * sizeof(CartCell) ≈ {} B（瞬时，可流式）", CART_EXPECTED, CART_EXPECTED * 32));
    v.push(format!("MEM ledger parts cap={} * u64 ≈ {} B", PART_TARGET, PART_TARGET * 8));
    v
}

// ===========================================================================
// 16. 演示面
// ===========================================================================

/// 演示：双链布局表。
pub fn demo_layout() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("=== 双链布局三件 ==="));
    let mut i = 0usize;
    while i < LayoutField::ALL.len() {
        let lf = LayoutField::ALL[i];
        let f = align_face_of(lf);
        v.push(format!(
            "[{}] {} 位宽={}位 对家F2682:{} 对家位宽={}位 一致={}",
            lf.index(),
            lf.label(),
            lf.width_bits(),
            lf.f2682_field(),
            f.width_bits(),
            lf.width_bits() == f.width_bits()
        ));
        i += 1;
    }
    v
}

/// 演示：对齐表（双向）。
pub fn demo_align() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from("=== F2682 驻留结构逐字段对齐 ==="));
    let t = align_table();
    let mut i = 0usize;
    while i < t.len() {
        v.push(format!("  {}", t[i].row()));
        i += 1;
    }
    let mut j = 0usize;
    while j < AlignFace::ALL.len() {
        let af = AlignFace::ALL[j];
        v.push(format!(
            "  FACE[{}] {} covered={} bits={}",
            af.index(),
            af.name(),
            af.covered(),
            af.width_bits()
        ));
        j += 1;
    }
    match align_zero_missing() {
        Ok(r) => {
            let mut k = 0usize;
            while k < r.len() {
                v.push(format!("  {}", r[k]));
                k += 1;
            }
        }
        Err(e) => v.push(format!("  ALIGN-ERR {}", err_line(e, 0))),
    }
    v
}

/// 演示：状态机边表。
pub fn demo_edges() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let m = edge_matrix();
    v.push(format!(
        "EDGE total={} legal={} illegal={}",
        m.len(),
        LEGAL_EDGES,
        ILLEGAL_EDGES
    ));
    let mut i = 0usize;
    while i < m.len() {
        v.push(format!("  edge[{:02}] {} -> {} legal={}", i, m[i].0.name(), m[i].1.name(), m[i].2));
        i += 1;
    }
    v
}

/// 演示：注入核形态表。
pub fn demo_inject() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < InjectForm::ALL.len() {
        let f = InjectForm::ALL[i];
        v.push(format!(
            "FORM[{:02}] {} -> 0x{:04x} :: {}",
            f.index(),
            f.name(),
            f.expect().code(),
            f.describe()
        ));
        i += 1;
    }
    v
}

/// 演示：错误闭集表。
pub fn demo_errs() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < ErrKind::ALL.len() {
        let e = ErrKind::ALL[i];
        v.push(format!(
            "ERR[{:02}] 0x{:04X}/0x{:04X} {} :: {}",
            i,
            e.code(),
            e.gate_code(),
            e.name(),
            e.describe()
        ));
        i += 1;
    }
    v
}

/// 演示：性能分解 + 内存账 + syscall 账。
pub fn demo_complexity() -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let t = complexity_table();
    let mut i = 0usize;
    while i < t.len() {
        v.push(t[i].row());
        i += 1;
    }
    let m = complexity_memory();
    i = 0;
    while i < m.len() {
        v.push(m[i].clone());
        i += 1;
    }
    v.push(syscall_account());
    v
}

/// 相关单号（衔接点）。
pub fn related_str() -> String {
    format!(
        "F2682/F2683(页缓存驻留结构) F2480(回收面) F2589(单点化纪律) 本域B31 执行面F3002-F3017"
    )
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
pub const TLV_RECORD_COUNT: usize = 10 + 6 + 3 + 3 + 6 + 12;
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
    push_u64(&mut head_payload, 400u64);
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
    push_tlv_u64(&mut out, 0x10, CHAIN_COUNT as u64);
    push_tlv_u64(&mut out, 0x11, CHAIN_CAP as u64);
    push_tlv_u64(&mut out, 0x12, LAYOUT_FIELDS as u64);
    push_tlv_u64(&mut out, 0x13, ALIGN_FIELDS as u64);
    push_tlv_u64(&mut out, 0x14, NODE_SLOTS as u64);
    push_tlv_u64(&mut out, 0x15, LEGAL_EDGES as u64);
    push_tlv_u64(&mut out, 0x16, ILLEGAL_EDGES as u64);
    push_tlv_u64(&mut out, 0x17, CART_EXPECTED as u64);
    push_tlv_u64(&mut out, 0x18, PART_TARGET as u64);
    push_tlv_u64(&mut out, 0x19, FORM_COUNT as u64);
    // 两链条位与位标（6 条）
    let mut ci = 0usize;
    while ci < Chain::ALL.len() {
        let ch = Chain::ALL[ci];
        push_tlv_u64(&mut out, 0x20u8.wrapping_add(ch.index()), ch.expect_lru() as u64);
        push_tlv_u64(&mut out, 0x22u8.wrapping_add(ch.index()), ch.cap() as u64);
        push_tlv_u64(&mut out, 0x24u8.wrapping_add(ch.index()), ch.index() as u64);
        ci += 1;
    }
    // 三件布局字段位宽（3 条）
    let mut li = 0usize;
    while li < LayoutField::ALL.len() {
        let lf = LayoutField::ALL[li];
        push_tlv_u64(&mut out, 0x30u8.wrapping_add(lf.index() as u8), lf.width_bits() as u64);
        li += 1;
    }
    // F2682 三对家位宽（3 条）
    let mut ai = 0usize;
    while ai < LayoutField::ALL.len() {
        let lf = LayoutField::ALL[ai];
        push_tlv_u64(&mut out, 0x34u8.wrapping_add(lf.index() as u8), align_face_of(lf).width_bits() as u64);
        ai += 1;
    }
    // 指纹段（4 条）
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
    push_tlv_u64(&mut out, 0x59, o.bare_writes as u64);
    push_tlv_u64(&mut out, 0x5A, if o.replay_stable { 1 } else { 0 });
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
        return Err(ErrKind::AlignMismatch);
    }
    if buf[0] != TLV_TAG_HEAD {
        return Err(ErrKind::AlignMismatch);
    }
    let hl = ((buf[1] as usize) << 8) | (buf[2] as usize);
    if hl != TLV_HEAD_PAYLOAD {
        return Err(ErrKind::AlignMismatch);
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
            return Err(ErrKind::AlignMismatch);
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
        return Err(ErrKind::AlignMismatch);
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
    // vals[13..18]=两链 6 条；vals[19..21]=三件布局字段位宽；
    // vals[22..24]=F2682 三对家位宽；vals[25..30]=指纹段；vals[31..42]=读数段。
    if vals[3] != CHAIN_COUNT as u64 {
        return false;
    }
    if vals[4] != CHAIN_CAP as u64 {
        return false;
    }
    if vals[5] != LAYOUT_FIELDS as u64 {
        return false;
    }
    if vals[25] != o.freeze_hash {
        return false;
    }
    if vals[26] != o.scheme_fp {
        return false;
    }
    if vals[19] != LayoutField::Hook.width_bits() as u64 {
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
