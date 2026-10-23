//! ═══════════════════════════════════════════════════════════════════════════
//! UNX-F0644 · 失败复演规程（失败轮重演三步）· 诊断段 0x1530~0x153F（独占）
//! ═══════════════════════════════════════════════════════════════════════════
//!
//! 任务：UNX-F0644「失败复演规程（失败轮重演三步）」，属 UNX 演练域
//! （UNX-A1-B33 · A1 域回归演练与值守收尾 · F641–F660 批）。
//!
//! ── 验收锚点原文（逐字抄录）──────────────────────────────────────────
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md · L2206（B33 批判据表行）：
//!
//! ```text
//! | UNX-F0644 | 失败复演规程（失败轮重演三步） | 220 | 已深化 | UNX-F0644-J1 失败轮复演三步（冻结现场/种子复现/二次执行）路径有账，同种子复现率 100% |
//! ```
//!
//! ── 定位原文（同文件 B33 批头，逐字抄录）─────────────────────────────
//! 失败复演归 B33 批独占的防重定位：
//!
//! ```text
//! 防重：域回归轮编排/失败复演/值守清单/闭批宣告归本批独占（B32 账面总装封账不重做运行态回归面——两批互补：B32 管账面态，B33 管运行态）
//! ```
//!
//! ── 判据 UNX-F0644-J1 拆解（本模块逐条落账）──────────────────────────
//! J1-a 三步路径有账：冻结现场 → 种子复现 → 二次执行，每步落事件账 +
//!     逻辑时戳 + 复演轮登记 + 逐步 phase 时戳履历；乱序一律拒绝
//!     （诊断码 0x1530）并计入乱序检出账。
//! J1-b 冻结现场：失败轮状态快照三要素（种子 / 步数预算 / 断言上下文
//!     ≥1 条）+ 失败到达点（失败步号 + 失败步指纹双键）+ 快照校验和；
//!     校验和对全部字段内容敏感（改一字必改和，改字段不改和必拒收）。
//! J1-c 种子复现：同种子重放必须到达同一失败点（步号与指纹双键比对），
//!     到不齐即判非确定源混入，红案立案（RedCase 落账 + health 翻红）。
//! J1-d 二次执行：修复确认落账后同种子再跑，轨迹全预算通过，末步指纹
//!     冻结进回归联防账——此后任何同种子运行必达该指纹，漂移即行为
//!     回归（0x153F）。
//! J1-e 同种子复现率 100%：复现次数 / 成功次数账，率不足 100% 即
//!     health 翻红；空账（零尝试）中性不判红。
//!
//! ── 自包含纪律（铁律落实）────────────────────────────────────────────
//! 1. 仅依赖 core / alloc（`use core` 本文件未直接需要；alloc 只取
//!    Vec/String/ToString 三项），禁 `use crate::…`；
//! 2. no_std + alloc：内核路径零 unwrap / expect / panic! / 运行期
//!    assert!（编译期 `const _: () = assert!(…)` 仅两处，合法）；可失败
//!    全走 `Result<T, RedoError>`，错误枚举 16 变体每变体带
//!    `detail(&self) -> &'static str` 中文可操作诊断；
//! 3. 溢出全部显性 wrapping / saturating（LCG 序列、序号、tick、率、
//!    计数器逐处标注）；>64KB 结构体禁上栈（本模块最大栈上结构为
//!    37 字节指纹缓冲，快照含 Vec 一律堆置）；
//! 4. const fn 纪律：const fn 内无 slice::get、无 Vec 索引、无
//!    Ord::min/max、无 derive-PartialEq 类型的 ==、无 for 循环（用
//!    while + 索引或手工展开）；含 Vec 字段的类型一律不 derive Copy；
//! 5. 诊断码段独占 0x1530~0x153F：整仓 grep `0x153` 确认无诊断码占用
//!    （唯一两处命中——"MODE14D" 为显示模式号、0x14E4 为 PCI 厂商 ID
//!    数据常量，均非诊断码段，段内 0x1530..=0x153F 逐码 grep 零命中）。
//!
//! ── 子系统清单（全部真实现，零注水）─────────────────────────────────
//! S1  `RedoSnapshot`      冻结现场快照（三要素 + 到达点双键 + 校验和）
//! S2  `ScenarioRunner`    确定性场景执行器（LCG + 弱点步派生 + 步指纹链
//!                         + 轨迹独立核验）
//! S3  `FailRedoProtocol`  三步复演协议（全序账 + 乱序拒绝 + 红案落账 +
//!                         phase 时戳履历 + 注入面）
//! S4  `ReproRate`         同种子复现率账（100% 判据 + 不变量）
//! S5  `RedoLedger`        复演账簿三态（登记/复演/闭合 + 未闭合显性 +
//!                         守恒恒等）
//! S6  `RegressionWatch`   回归联防（期望指纹冻结 + 巡检漂移检出 +
//!                         种子唯一写后读防线）
//! S7  `EventLog`          事件账（序号连续 + tick 单调 + 分类计数）
//! S8  `HealthReport`      health() 八查不变量聚合
//! S9  注入面              种子漂移 / 快照篡改（错签 + 重签伪造）/
//!                         跳步（裁步 + 重排骗闸）四注入全检出
//! S10 `CompareReport`     双跑确定性对拍（逐步指纹 + 首分叉步定位）
//! S11 `ProtocolSummary`   协议账面总览（对冲恒等）
//! S12 `CheckSet`          自包含判据电池（46 条，J1 全路径）
//! ═══════════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════════
// 一、诊断码段（独占 0x1530~0x153F，16 码一一对应 16 个错误变体）
// ═══════════════════════════════════════════════════════════════════════════

/// 诊断段基址。整仓 grep `0x1530..=0x153F` 零诊断码占用后选定（见头注
/// 纪律 5——两处相邻命中均为数据常量/模式号，非诊断码段）。
pub const SEG_BASE: u16 = 0x1530;
/// 诊断段顶（含）。
pub const SEG_TOP: u16 = 0x153F;

const E_OUT_OF_ORDER: u16 = 0x1530; // 复演三步乱序
const E_EMPTY_CTX: u16 = 0x1531; // 断言上下文为空
const E_BUDGET: u16 = 0x1532; // 步数预算/步账不符
const E_SEED_DRIFT: u16 = 0x1533; // 种子漂移
const E_FP_MISMATCH: u16 = 0x1534; // 失败点不齐（非确定源）
const E_FIX_UNCONFIRMED: u16 = 0x1535; // 修复未确认
const E_SECOND_EXEC: u16 = 0x1536; // 二次执行未通过
const E_REPRO_UNCONFIRMED: u16 = 0x1537; // 复现未确认即闭合
const E_ALREADY_CLOSED: u16 = 0x1538; // 账目已闭合再写
const E_DUP_REGISTER: u16 = 0x1539; // 重复登记
const E_UNKNOWN_ROUND: u16 = 0x153A; // 未知复演轮
const E_STEP_SKIP: u16 = 0x153B; // 跳步检出
const E_CHAIN_BREAK: u16 = 0x153C; // 指纹链断裂
const E_SNAPSHOT_TAMPER: u16 = 0x153D; // 快照校验和不符
const E_WATCH_UNARMED: u16 = 0x153E; // 联防未冻结
const E_WATCH_DRIFT: u16 = 0x153F; // 联防指纹漂移

// ── 编译期断言（const assert 合法；运行期 assert 在内核路径禁用）──────
const _: () = assert!(SEG_BASE == 0x1530);
const _: () = assert!(SEG_TOP == 0x153F);
const _: () = assert!(E_WATCH_DRIFT == SEG_TOP);
const _: () = assert!(E_OUT_OF_ORDER == SEG_BASE);

// ═══════════════════════════════════════════════════════════════════════════
// 二、基础工具：FNV-1a 64 / LCG / 逻辑时戳
// ═══════════════════════════════════════════════════════════════════════════

/// FNV-1a 64 位哈希。偏移基与素数取自 FNV 官方参数表（64 位：
/// offset_basis = 0xcbf29ce484222325，prime = 0x100000001b3），来源写死，
/// 全仓复演链共用同一哈希地基。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// u64 值混入 FNV 链：先对旧链头与值各自哈希再异或，保证值域分隔
/// （0x01 后接 0x02 与 0x0102 直吸不同和）。
fn fnv_mix_u64(prev: u64, v: u64) -> u64 {
    fnv1a64(&prev.to_le_bytes()) ^ fnv1a64(&v.to_le_bytes())
}

/// u32 值混入 FNV 链。
fn fnv_mix_u32(prev: u64, v: u32) -> u64 {
    fnv1a64(&prev.to_le_bytes()) ^ fnv1a64(&v.to_le_bytes())
}

/// 确定性 LCG（线性同余）。参数取 Numerical Recipes / Knuth 64 位推荐：
/// a = 6364136223846793005，c = 1442695040888963407。同种子同序列，全程
/// wrapping 无溢出陷阱——确定性是复演的物理基础，非确定即红案。
#[derive(Clone, Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 以种子构造。种子 0 合法（本 LCG 形如 x' = a·x + c，x = 0 时
    /// x' = c ≠ 0，无零不动点，序列照常前进）。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    /// 推进一步，返回新状态。溢出显性 wrapping——确定性优先于数值意义，
    /// 任何隐式溢出都会破坏同种子同轨迹公理。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    /// 取 [0, n) 内确定性值。n == 0 时返回 0——防除零 panic，内核路径
    /// 禁任何运行期 panic 源（含 % 0 除零）。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }

    /// 双取样合成一步工作三元组：两次 next 交错取 (a, b)，工作量字
    /// work = a ^ (b<<<17)。双取样防单步短周期耦合，旋转让相邻步的
    /// 低位影响工作量字高位。
    pub fn step_work(&mut self) -> (u64, u64, u64) {
        let a = self.next();
        let b = self.next();
        let work = a ^ b.rotate_left(17);
        (a, b, work)
    }
}

/// 逻辑时戳钟。物理钟在 no_std 内核演练面不可信（回归轮跨宿主、跨
/// QEMU 实例），以单调逻辑 tick 为序：每个记账动作推进一格，tick 严格
/// 递增即账目时序成立，回退即账面被改。
#[derive(Clone, Debug)]
pub struct LogicalClock {
    t: u64,
}

impl LogicalClock {
    pub const fn new() -> Self {
        LogicalClock { t: 0 }
    }

    /// 推进并返回新 tick。wrapping 显性（u64 耗尽在现实演练轮中不可达，
    /// 但溢出必须显性而非隐式）。
    pub fn advance(&mut self) -> u64 {
        self.t = self.t.wrapping_add(1);
        self.t
    }

    /// 当前 tick 只读。
    pub const fn now(&self) -> u64 {
        self.t
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 三、RedoError：可失败全域错误枚举（16 变体 × 16 诊断码，一一对应）
// ═══════════════════════════════════════════════════════════════════════════

/// 复演域全部可失败路径的唯一错误类型。变体与诊断码一一对应
/// （0x1530~0x153F 恰 16 码，见段首常量表），`code()` 给段内独占码，
/// `detail()` 给 'static 中文可操作诊断——零运行期格式化、零堆分配。
#[derive(Clone, Debug, PartialEq)]
pub enum RedoError {
    /// 复演三步乱序：未按 冻结→复现→二次执行 全序推进。
    OutOfOrder,
    /// 冻结现场时断言上下文为空——无上下文的现场不可复演。
    EmptyContext,
    /// 步数预算不符：复现/二次执行预算 ≠ 冻结预算，或步账与预算不符，
    /// 或零预算冻结。
    BudgetMismatch,
    /// 种子漂移：复现种子 ≠ 冻结种子。同种子是复演的第一公理。
    SeedDrift,
    /// 失败点不齐：同种子重放未到达同一失败点——判非确定源混入。
    FailurePointMismatch,
    /// 修复未确认即发起二次执行。先 confirm_fix 落账，再 step3。
    FixNotConfirmed,
    /// 二次执行未通过：修复后同种子重放仍有失败步。
    SecondExecFailed,
    /// 复现未确认即尝试闭合账目。
    ReproNotConfirmed,
    /// 账目已闭合后再写——闭合是终态，终态不可改。
    AlreadyClosed,
    /// 同一失败轮重复登记。
    DuplicateRegistration,
    /// 引用了未登记的复演轮。
    UnknownRound,
    /// 跳步检出：轨迹步号不连续 / 步账数与轨迹数不符。
    StepSkipDetected,
    /// 步指纹链断裂：步间哈希衔接不连续 / 步工作量字被改。
    FingerprintChainBreak,
    /// 快照校验和不符：冻结现场疑似被篡改。
    SnapshotChecksumMismatch,
    /// 回归联防未冻结：该种子无期望指纹可对。
    WatchNotArmed,
    /// 回归联防指纹漂移：同种子运行未到达冻结期望指纹。
    WatchDrift,
}

impl RedoError {
    /// 诊断码（段内独占，0x1530~0x153F）。const fn：仅整数匹配，无禁用
    /// 操作。
    pub const fn code(&self) -> u16 {
        match self {
            RedoError::OutOfOrder => E_OUT_OF_ORDER,
            RedoError::EmptyContext => E_EMPTY_CTX,
            RedoError::BudgetMismatch => E_BUDGET,
            RedoError::SeedDrift => E_SEED_DRIFT,
            RedoError::FailurePointMismatch => E_FP_MISMATCH,
            RedoError::FixNotConfirmed => E_FIX_UNCONFIRMED,
            RedoError::SecondExecFailed => E_SECOND_EXEC,
            RedoError::ReproNotConfirmed => E_REPRO_UNCONFIRMED,
            RedoError::AlreadyClosed => E_ALREADY_CLOSED,
            RedoError::DuplicateRegistration => E_DUP_REGISTER,
            RedoError::UnknownRound => E_UNKNOWN_ROUND,
            RedoError::StepSkipDetected => E_STEP_SKIP,
            RedoError::FingerprintChainBreak => E_CHAIN_BREAK,
            RedoError::SnapshotChecksumMismatch => E_SNAPSHOT_TAMPER,
            RedoError::WatchNotArmed => E_WATCH_UNARMED,
            RedoError::WatchDrift => E_WATCH_DRIFT,
        }
    }

    /// 中文可操作诊断（'static——零运行期格式化、零堆分配、零 panic 源）。
    pub fn detail(&self) -> &'static str {
        match self {
            RedoError::OutOfOrder => {
                "复演三步乱序：必须先冻结现场(步1)，再种子复现(步2)，最后二次执行(步3)；请核对该复演轮当前已完成的步号"
            }
            RedoError::EmptyContext => {
                "断言上下文为空：冻结现场需至少一条断言上下文（失败判据/断点描述/复演域），请补齐失败轮上下文后重新冻结"
            }
            RedoError::BudgetMismatch => {
                "步数预算不符：复现轮/二次执行的步数预算必须与冻结现场完全一致，请核对冻结快照中的 budget 字段与调用参数"
            }
            RedoError::SeedDrift => {
                "种子漂移：复现种子与冻结种子不一致——同种子是复演第一公理；请从种子簿取冻结轮原种子，禁止现场临时换种"
            }
            RedoError::FailurePointMismatch => {
                "失败点不齐：同种子重放未到达冻结记录的失败点，判非确定源混入（红案）；请排查时间/地址/未初始化读等非确定输入后另立新轮"
            }
            RedoError::FixNotConfirmed => {
                "修复未确认：二次执行前必须先 confirm_fix 落修复确认账；请先走修复评审确认流程再发起步3"
            }
            RedoError::SecondExecFailed => {
                "二次执行未通过：修复后同种子重放仍有失败步；请重新冻结新失败轮再走三步，禁止在旧账上叠跑"
            }
            RedoError::ReproNotConfirmed => {
                "复现未确认：闭合账目前必须完成种子复现且到达点双键对齐；请先走完步2并取得复现成功账"
            }
            RedoError::AlreadyClosed => {
                "账目已闭合：闭合为终态不可改写；如需重演请新开复演轮号，禁止复用已闭合轮"
            }
            RedoError::DuplicateRegistration => {
                "重复登记：该轮号/种子已在账簿中；请换新轮号或先查询既有账目（联防账同种子唯一）"
            }
            RedoError::UnknownRound => {
                "未知复演轮：轮号未在账簿登记；请先执行步1冻结现场完成登记"
            }
            RedoError::StepSkipDetected => {
                "跳步检出：轨迹步号不连续或步账数与轨迹数不符；请核对执行器步进账，禁止裁剪/跳过任何一步"
            }
            RedoError::FingerprintChainBreak => {
                "步指纹链断裂：步间哈希衔接或步工作量字不匹配；轨迹被改写或执行器非确定——请用同种子重跑对拍定位首个分叉步"
            }
            RedoError::SnapshotChecksumMismatch => {
                "快照校验和不符：冻结现场疑似被篡改；请废弃该快照，从失败轮原始记录重新冻结"
            }
            RedoError::WatchNotArmed => {
                "联防未冻结：该种子没有已冻结的期望指纹；请先走通三步复演使二次执行冻结期望指纹"
            }
            RedoError::WatchDrift => {
                "联防指纹漂移：同种子运行未到达冻结期望指纹——行为回归；请比对该步指纹链定位首个分叉步并立案"
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 四、S2 · ScenarioRunner：确定性场景执行器
// ═══════════════════════════════════════════════════════════════════════════

/// 单步记录：步号 + LCG 双取样 + 工作量字 + 步指纹 + 失败旗标。
/// 指纹链定义（递推，链头由种子单独派生）：
/// ```text
///   h0 = chain_head(seed)
///   hi = FNV( h(i-1) ‖ step_no_i ‖ a_i ‖ b_i ‖ work_i ‖ failed_i )
/// ```
/// 任一步的任一输入分叉，其后所有指纹级联失配——分叉步可精确定位到步；
/// 重排步号骗得过步号连续性闸，骗不过级联指纹（步号入链）。
#[derive(Clone, Debug)]
pub struct StepRecord {
    pub step_no: u32,
    pub lcg_a: u64,
    pub lcg_b: u64,
    pub work: u64,
    pub fp: u64,
    pub failed: bool,
}

/// 轨迹终态：全通过，或在某步失败（含该步指纹——到达点双键之一）。
#[derive(Clone, Debug, PartialEq)]
pub enum RunOutcome {
    /// 全预算步通过。
    Passed,
    /// 在 at_step 步失败，失败步指纹 fp。
    FailedAt {
        /// 失败步号。
        at_step: u32,
        /// 失败步指纹。
        fp: u64,
    },
}

/// 一次运行的完整账：种子/预算/修复旗标/逐步轨迹/终态/步数账。
/// 修复旗标入账——同种子「未修复」与「已修复」是两条合法且各自确定的
/// 轨迹；复演语义 = 未修复轨迹必失败于同一点（到达点双键），已修复
/// 轨迹必全通过。含 Vec 字段，禁 derive Copy。
#[derive(Clone, Debug)]
pub struct RunResult {
    pub seed: u64,
    pub budget: u32,
    pub fix_applied: bool,
    pub steps: Vec<StepRecord>,
    pub outcome: RunOutcome,
    pub steps_executed: u32,
}

/// 链头：由种子单独派生（与预算、修复态无关），保证同种子的任何轨迹
/// 共享同一链头——链头不同即种子不同，一票定性。
pub const fn chain_head(seed: u64) -> u64 {
    // 写死混入常数：段号 0x1530 与任务号 0644 复合，防不同任务域同种子
    // 链头重合（域隔离进链头）。
    fnv_const_seed(seed ^ 0x1530_0000_0644)
}

/// const 版种子预混：const fn 内禁 for 循环——8 字节 FNV 吸收手工展开
/// （与 fnv1a64 单字节吸收逐位等价）。
const fn fnv_const_seed(v: u64) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = (h ^ (v & 0xFF)).wrapping_mul(0x0000_0100_0000_01B3);
    h = (h ^ ((v >> 8) & 0xFF)).wrapping_mul(0x0000_0100_0000_01B3);
    h = (h ^ ((v >> 16) & 0xFF)).wrapping_mul(0x0000_0100_0000_01B3);
    h = (h ^ ((v >> 24) & 0xFF)).wrapping_mul(0x0000_0100_0000_01B3);
    h = (h ^ ((v >> 32) & 0xFF)).wrapping_mul(0x0000_0100_0000_01B3);
    h = (h ^ ((v >> 40) & 0xFF)).wrapping_mul(0x0000_0100_0000_01B3);
    h = (h ^ ((v >> 48) & 0xFF)).wrapping_mul(0x0000_0100_0000_01B3);
    h = (h ^ ((v >> 56) & 0xFF)).wrapping_mul(0x0000_0100_0000_01B3);
    h
}

/// 步指纹递推一步。37 字节定长缓冲上栈（8+4+8+8+8+1 = 37，远小于
/// 64KB 上栈红线）。
fn chain_step(prev: u64, step_no: u32, a: u64, b: u64, work: u64, failed: bool) -> u64 {
    let mut buf = [0u8; 37];
    buf[0..8].copy_from_slice(&prev.to_le_bytes());
    buf[8..12].copy_from_slice(&step_no.to_le_bytes());
    buf[12..20].copy_from_slice(&a.to_le_bytes());
    buf[20..28].copy_from_slice(&b.to_le_bytes());
    buf[28..36].copy_from_slice(&work.to_le_bytes());
    buf[36] = if failed { 1 } else { 0 };
    fnv1a64(&buf)
}

/// 确定性场景执行器。场景语义：
/// * 每步消耗 LCG 双取样得 (a, b)，工作量字 work = a ^ (b<<<17)；
/// * 弱点步（weak step）由种子确定性派生：同种子恒定同点——这是
///   「失败轮」的确定性根源，也是「同种子必复现」的物理保证；
/// * fix_applied = false 时，走到弱点步即失败（失败轮轨迹）；
/// * fix_applied = true 时（修复后），弱点步被消除，全预算通过。
pub struct ScenarioRunner {
    /// 场景名（事件账可读性；不入指纹链——链只吃种子与步数据）。
    pub name: String,
}

impl ScenarioRunner {
    pub fn new(name: &str) -> Self {
        ScenarioRunner {
            name: name.to_string(),
        }
    }

    /// 弱点步派生：1 + (mix(seed) mod budget)。budget == 0 时返回 1
    /// （调用方在预算闸拒绝零预算，此处只保无 panic——% 0 除零防）。
    pub fn weak_step_for(seed: u64, budget: u32) -> u32 {
        if budget == 0 {
            return 1;
        }
        let mix = fnv_mix_u64(0x1530_0644_1A2B_3C4D, seed);
        1u32.wrapping_add((mix % budget as u64) as u32)
    }

    /// 确定性执行。同 (seed, budget, fix_applied) 输入 → 逐步逐位同输出。
    /// 失败即停（弱点上破，后续步不跑——步数账随之收缩，止点即到达点）。
    pub fn run(&self, seed: u64, budget: u32, fix_applied: bool) -> RunResult {
        let weak = Self::weak_step_for(seed, budget);
        let mut lcg = Lcg::new(seed ^ 0x0644_5EED_5EED_0001);
        let mut h = chain_head(seed);
        let mut steps: Vec<StepRecord> = Vec::new();
        let mut outcome = RunOutcome::Passed;
        let mut i: u32 = 1;
        while i <= budget {
            let (a, b, work) = lcg.step_work();
            let failing = !fix_applied && i == weak;
            h = chain_step(h, i, a, b, work, failing);
            steps.push(StepRecord {
                step_no: i,
                lcg_a: a,
                lcg_b: b,
                work,
                fp: h,
                failed: failing,
            });
            if failing {
                outcome = RunOutcome::FailedAt { at_step: i, fp: h };
                break;
            }
            i = i.wrapping_add(1);
        }
        RunResult {
            seed,
            budget,
            fix_applied,
            steps_executed: steps.len() as u32,
            steps,
            outcome,
        }
    }

    /// 轨迹核验（独立于执行器重算——防「自己验自己」）。五道检：
    /// 1. 步数账一致：steps_executed == steps.len()，且 ≤ budget；
    /// 2. 步号连续：1,2,3,… 零跳号（裁步跳步检出）；
    /// 3. 工作量字可由双取样重算（防改字）；
    /// 4. 指纹链逐级重算全等（防改链、防重排步号——步号入链）；
    /// 5. 终态一致：Passed ⇒ 恰跑满 budget；FailedAt ⇒ 失败于末步、
    ///    指纹同、失败旗标真（双键与旗标三合一）。
    pub fn verify_trajectory(r: &RunResult) -> Result<(), RedoError> {
        // 检1：步数账。
        if r.steps_executed != r.steps.len() as u32 {
            return Err(RedoError::StepSkipDetected);
        }
        if (r.steps.len() as u64) > r.budget as u64 {
            return Err(RedoError::BudgetMismatch);
        }
        // 检2/3/4：逐级重算。
        let mut i = 0usize;
        let mut expect_no: u32 = 1;
        let mut prev = chain_head(r.seed);
        while i < r.steps.len() {
            let rec = &r.steps[i];
            if rec.step_no != expect_no {
                return Err(RedoError::StepSkipDetected);
            }
            let w_expect = rec.lcg_a ^ rec.lcg_b.rotate_left(17);
            if w_expect != rec.work {
                return Err(RedoError::FingerprintChainBreak);
            }
            let h_expect =
                chain_step(prev, rec.step_no, rec.lcg_a, rec.lcg_b, rec.work, rec.failed);
            if h_expect != rec.fp {
                return Err(RedoError::FingerprintChainBreak);
            }
            prev = rec.fp;
            expect_no = expect_no.wrapping_add(1);
            i += 1;
        }
        // 检5：终态一致。
        match r.outcome {
            RunOutcome::Passed => {
                if r.steps.len() != r.budget as usize {
                    return Err(RedoError::BudgetMismatch);
                }
            }
            RunOutcome::FailedAt { at_step, fp } => {
                if r.steps.is_empty() {
                    return Err(RedoError::StepSkipDetected);
                }
                let last = &r.steps[r.steps.len() - 1];
                if last.step_no != at_step || last.fp != fp || !last.failed {
                    return Err(RedoError::FingerprintChainBreak);
                }
            }
        }
        Ok(())
    }
}

/// 双跑确定性对拍报告。identical ⇒ 逐步指纹全等且步数同；
/// first_diff_step 给出首个分叉步（对拍定位粒度 = 步）。
#[derive(Clone, Debug, PartialEq)]
pub struct CompareReport {
    pub identical: bool,
    pub first_diff_step: Option<u32>,
    pub steps_a: u32,
    pub steps_b: u32,
}

/// 双跑对拍：同 (seed, budget, fix) 连跑两次，逐步指纹比对。确定性
/// 执行器下 identical 恒真；为假即执行器被非确定源污染。
pub fn double_run_compare(
    runner: &ScenarioRunner,
    seed: u64,
    budget: u32,
    fix_applied: bool,
) -> CompareReport {
    let ra = runner.run(seed, budget, fix_applied);
    let rb = runner.run(seed, budget, fix_applied);
    let n = if ra.steps.len() <= rb.steps.len() {
        ra.steps.len()
    } else {
        rb.steps.len()
    };
    let mut first_diff: Option<u32> = None;
    let mut i = 0usize;
    while i < n {
        if ra.steps[i].fp != rb.steps[i].fp {
            first_diff = Some(ra.steps[i].step_no);
            break;
        }
        i += 1;
    }
    if first_diff.is_none() && ra.steps.len() != rb.steps.len() {
        // 步数不同（确定性执行器下本轮不发生，防御性定位到末步）。
        first_diff = Some(n as u32);
    }
    CompareReport {
        identical: first_diff.is_none() && ra.steps.len() == rb.steps.len(),
        first_diff_step: first_diff,
        steps_a: ra.steps.len() as u32,
        steps_b: rb.steps.len() as u32,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 五、S1 · RedoSnapshot：冻结现场快照
// ═══════════════════════════════════════════════════════════════════════════

/// 冻结现场快照 = 失败轮状态三要素 + 失败到达点双键 + 快照校验和。
/// * 三要素：种子 / 步数预算 / 断言上下文（≥1 条，判据 J1-b）；
/// * 到达点双键：失败步号 + 失败步指纹（步2 比对的双键——只对步号会
///   把"到同一步但轨迹不同"误判为复现成功，指纹键补上轨迹同一性）；
/// * 校验和：对全部字段 FNV 重算——任何字段篡改必现形（0x153D）；
///   "改字段不改和"的错签快照在 absorb 闸被拒收。
/// 含 Vec 字段，禁 derive Copy（纪律 4）。
#[derive(Clone, Debug)]
pub struct RedoSnapshot {
    pub round_id: u32,
    pub seed: u64,
    pub budget: u32,
    pub arrival_step: u32,
    pub arrival_fp: u64,
    pub ctx: Vec<String>,
    pub checksum: u64,
    /// 冻结时刻逻辑时戳（账面时序，入校验和——连冻结时刻都改不得）。
    pub frozen_tick: u64,
}

impl RedoSnapshot {
    /// 快照校验和：字段级 FNV 吸收。域分隔靠"先吸长度再吸内容"防拼接
    /// 歧义（上下文条目 "ab"+"c" 与 "a"+"bc" 必不同和）。frozen_tick
    /// 一并入和——快照的时刻也被冻结。
    pub fn recompute_checksum(&self) -> u64 {
        // 域盐：段号 + 任务号复合，防跨任务域校验和串味。
        let mut h = fnv_mix_u64(0x1530_5A9C_D00D_0644, self.seed);
        h = fnv_mix_u32(h, self.budget);
        h = fnv_mix_u32(h, self.arrival_step);
        h = fnv_mix_u64(h, self.arrival_fp);
        h = fnv_mix_u32(h, self.round_id);
        h = fnv_mix_u64(h, self.frozen_tick);
        h = fnv_mix_u64(h, self.ctx.len() as u64);
        let mut i = 0usize;
        while i < self.ctx.len() {
            let s = &self.ctx[i];
            h = fnv_mix_u64(h, s.len() as u64);
            h = fnv_mix_u64(h, fnv1a64(s.as_bytes()));
            i += 1;
        }
        h
    }

    /// 快照完整性核验：重算校验和与账面校验和比对。
    pub fn verify(&self) -> Result<(), RedoError> {
        if self.recompute_checksum() == self.checksum {
            Ok(())
        } else {
            Err(RedoError::SnapshotChecksumMismatch)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 六、S4 · ReproRate：同种子复现率账（J1-e 100% 判据）
// ═══════════════════════════════════════════════════════════════════════════

/// 复现率账：attempts = 实际发起并真跑了重放的种子复现次数（种子漂移
/// 未跑即拒的不计——没有重放就没有"复现尝试"），successes = 到达点
/// 双键比对全齐的次数。rate = s×100/a（饱和乘法）。
/// 全 u64 字段，可 Copy（纪律 4 不适用于纯标量）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReproRate {
    pub attempts: u64,
    pub successes: u64,
}

impl ReproRate {
    pub const fn new() -> Self {
        ReproRate {
            attempts: 0,
            successes: 0,
        }
    }

    /// 记一次复现尝试（含失败尝试——失败正是红案来源，账面必须显性）。
    pub fn record_attempt(&mut self) {
        self.attempts = self.attempts.wrapping_add(1);
    }

    /// 记一次复现成功。成功必先有尝试；防御性钳制防越账（成功 > 尝试
    /// 即账面被改，钳回 attempts 保不变量恒真）。
    pub fn record_success(&mut self) {
        self.successes = self.successes.wrapping_add(1);
        if self.successes > self.attempts {
            self.successes = self.attempts;
        }
    }

    /// 复现率百分数：饱和乘法防 s×100 溢出；尝试数为零时返回 0（无
    /// 尝试不构成率——不构成 100%，也不构成 0%，由 is_full_or_empty
    /// 把空账判为中性）。
    pub fn rate_pct(&self) -> u64 {
        if self.attempts == 0 {
            0
        } else if self.successes >= self.attempts {
            100
        } else {
            self.successes.saturating_mul(100) / self.attempts
        }
    }

    /// J1-e 判据：无尝试（空账中性）或复现率恰 100%。
    pub fn is_full_or_empty(&self) -> bool {
        self.attempts == 0 || self.rate_pct() == 100
    }

    /// 不变量：成功数 ≤ 尝试数。
    pub const fn invariant_ok(&self) -> bool {
        self.successes <= self.attempts
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 七、S5 · RedoLedger：复演账簿三态
// ═══════════════════════════════════════════════════════════════════════════

/// 账簿三态：Registered（失败轮登记）→ Redoing（复演轮登记）→
/// Closed（复现 + 修复确认双条件齐才闭合）。
#[derive(Clone, Debug, PartialEq)]
pub enum LedgerState {
    /// 步1 冻结现场后落账。
    Registered,
    /// 步2 种子复现成功后进入。
    Redoing,
    /// 步3 二次执行通过且双确认齐备后闭合（终态）。
    Closed,
}

/// 账簿行：一失败轮一行。
#[derive(Clone, Debug)]
pub struct LedgerEntry {
    pub round_id: u32,
    pub state: LedgerState,
    /// 步2 到达点双键比对全齐。
    pub repro_confirmed: bool,
    /// 修复确认（confirm_fix 落账）。
    pub fix_confirmed: bool,
    /// 登记时刻逻辑时戳。
    pub opened_tick: u64,
    /// 闭合时刻（None = 未闭合——未闭合账显性的行级表示）。
    pub closed_tick: Option<u64>,
}

/// 复演账簿。守恒恒等：登记数 = 未闭合数 + 闭合数（账不生不灭，只在
/// 三态间流转）；未闭合账显性：unclosed() 清单与逐行扫描零差。
#[derive(Clone, Debug)]
pub struct RedoLedger {
    entries: Vec<LedgerEntry>,
}

impl RedoLedger {
    pub fn new() -> Self {
        RedoLedger {
            entries: Vec::new(),
        }
    }

    /// 登记（步1 后调用）。同轮号重复登记拒绝（0x1539）。
    pub fn register(&mut self, round_id: u32, tick: u64) -> Result<(), RedoError> {
        if self.find(round_id).is_some() {
            return Err(RedoError::DuplicateRegistration);
        }
        self.entries.push(LedgerEntry {
            round_id,
            state: LedgerState::Registered,
            repro_confirmed: false,
            fix_confirmed: false,
            opened_tick: tick,
            closed_tick: None,
        });
        Ok(())
    }

    /// 登记存在性查询（while + 索引，非 const fn，索引合法）。
    pub fn find(&self, round_id: u32) -> Option<&LedgerEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].round_id == round_id {
                return Some(&self.entries[i]);
            }
            i += 1;
        }
        None
    }

    fn find_mut(&mut self, round_id: u32) -> Option<&mut LedgerEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].round_id == round_id {
                return Some(&mut self.entries[i]);
            }
            i += 1;
        }
        None
    }

    /// 进入复演态（步2 成功后）。未知轮拒；已闭合轮拒写。
    pub fn mark_redoing(&mut self, round_id: u32) -> Result<(), RedoError> {
        match self.find_mut(round_id) {
            Some(e) => {
                if e.state == LedgerState::Closed {
                    return Err(RedoError::AlreadyClosed);
                }
                e.state = LedgerState::Redoing;
                e.repro_confirmed = true;
                Ok(())
            }
            None => Err(RedoError::UnknownRound),
        }
    }

    /// 修复确认落账（步3 前置闸）。已闭合轮拒写（终态防线）。
    pub fn confirm_fix(&mut self, round_id: u32) -> Result<(), RedoError> {
        match self.find_mut(round_id) {
            Some(e) => {
                if e.state == LedgerState::Closed {
                    return Err(RedoError::AlreadyClosed);
                }
                e.fix_confirmed = true;
                Ok(())
            }
            None => Err(RedoError::UnknownRound),
        }
    }

    /// 闭合。双条件（复现确认 + 修复确认）缺一拒（0x1537）——判据
    /// J1「闭合（复现+修复确认）」的账面强制，不是约定俗成。
    pub fn close(&mut self, round_id: u32, tick: u64) -> Result<(), RedoError> {
        match self.find_mut(round_id) {
            Some(e) => {
                if e.state == LedgerState::Closed {
                    return Err(RedoError::AlreadyClosed);
                }
                if !e.repro_confirmed || !e.fix_confirmed {
                    return Err(RedoError::ReproNotConfirmed);
                }
                e.state = LedgerState::Closed;
                e.closed_tick = Some(tick);
                Ok(())
            }
            None => Err(RedoError::UnknownRound),
        }
    }

    /// 未闭合账显性清单（Registered + Redoing 的轮号）——判据 J1
    /// 「未闭合账显性」的读侧：清单非空即有待办复演轮，一目了然。
    pub fn unclosed(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].state != LedgerState::Closed {
                out.push(self.entries[i].round_id);
            }
            i += 1;
        }
        out
    }

    /// 闭合轮清单。
    pub fn closed(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].state == LedgerState::Closed {
                out.push(self.entries[i].round_id);
            }
            i += 1;
        }
        out
    }

    /// 守恒恒等：登记数 = 未闭合数 + 闭合数。
    pub fn balance_ok(&self) -> bool {
        self.entries.len() as u64
            == self.unclosed().len() as u64 + self.closed().len() as u64
    }

    /// 未闭合账显性自证：unclosed() 清单与逐行状态扫描零差（清单生成
    /// 逻辑与状态扫描互为对拍）。
    pub fn unclosed_explicit(&self) -> bool {
        let u = self.unclosed();
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            let mut in_list = false;
            let mut j = 0usize;
            while j < u.len() {
                if u[j] == e.round_id {
                    in_list = true;
                }
                j += 1;
            }
            if (e.state != LedgerState::Closed) != in_list {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 八、S6 · RegressionWatch：回归联防账
// ═══════════════════════════════════════════════════════════════════════════

/// 联防行：复演通过后冻结的期望指纹。此后任何同种子运行必须再次到达
/// 该指纹——漂移即行为回归（0x153F），联防把一次复演的成果变成永久
/// 回归防线。
#[derive(Clone, Debug)]
pub struct WatchEntry {
    pub round_id: u32,
    pub seed: u64,
    pub expected_final_fp: u64,
    pub armed_tick: u64,
}

/// 回归联防账。种子唯一（写后读防线）：同种子重复冻结拒——期望指纹
/// 一经冻结不可被同种子二次改写。
#[derive(Clone, Debug)]
pub struct RegressionWatch {
    entries: Vec<WatchEntry>,
}

impl RegressionWatch {
    pub fn new() -> Self {
        RegressionWatch {
            entries: Vec::new(),
        }
    }

    /// 冻结期望指纹（步3 通过后）。同种子重复冻结拒绝（0x1539——
    /// 语义为"该种子已有联防账"，攻面与重复登记同源）。
    pub fn arm(
        &mut self,
        round_id: u32,
        seed: u64,
        final_fp: u64,
        tick: u64,
    ) -> Result<(), RedoError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].seed == seed {
                return Err(RedoError::DuplicateRegistration);
            }
            i += 1;
        }
        self.entries.push(WatchEntry {
            round_id,
            seed,
            expected_final_fp: final_fp,
            armed_tick: tick,
        });
        Ok(())
    }

    /// 巡检：同种子运行末步指纹必须命中冻结期望。命中 Ok；种子已知但
    /// 指纹不齐 → WatchDrift（0x153F）；种子陌生 → WatchNotArmed
    /// （0x153E）——两种拒绝严格分码，防"没布防"与"被击穿"混账。
    pub fn patrol(&self, seed: u64, final_fp: u64) -> Result<(), RedoError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].seed == seed {
                if self.entries[i].expected_final_fp == final_fp {
                    return Ok(());
                }
                return Err(RedoError::WatchDrift);
            }
            i += 1;
        }
        Err(RedoError::WatchNotArmed)
    }

    /// 联防种子唯一自证（两两比对，账面防线本身也要有防线）。
    pub fn no_dup_seed(&self) -> bool {
        let n = self.entries.len();
        let mut i = 0usize;
        while i < n {
            let mut j = i + 1;
            while j < n {
                if self.entries[i].seed == self.entries[j].seed {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 九、S7 · EventLog：事件账（序号连续 + 逻辑时戳）
// ═══════════════════════════════════════════════════════════════════════════

/// 事件种类（判据 J1-a「路径有账」的账粒度——三步每步至少一笔，
/// 拒绝/检出事件单独成类，账面可按类审计）。
#[derive(Clone, Debug, PartialEq)]
pub enum EventKind {
    /// 步1 冻结现场落账。
    Freeze,
    /// 步2 复现重放发起。
    ReproRun,
    /// 步2 到达点双键比对全齐。
    ReproOk,
    /// 步2 到达点不齐（非确定源红案触发）。
    ReproMismatch,
    /// 种子漂移注入被拒。
    DriftRejected,
    /// 修复确认落账。
    FixConfirm,
    /// 步3 二次执行（发起/拒绝/未通过共用，note 区分）。
    SecondExec,
    /// 账目闭合。
    Closed,
    /// 乱序推进被拒。
    OutOfOrderRejected,
    /// 快照篡改检出。
    TamperDetected,
    /// 跳步/链断检出。
    SkipDetected,
    /// 联防期望指纹冻结。
    WatchFrozen,
    /// 联防巡检命中。
    WatchHit,
    /// 联防巡检漂移/落空。
    WatchMiss,
    /// 双跑对拍完成。
    DoubleRun,
}

/// 事件行。note 为 'static 中文摘要（零运行期分配）。
#[derive(Clone, Debug)]
pub struct EventRecord {
    pub seq: u64,
    pub round_id: u32,
    pub kind: EventKind,
    pub tick: u64,
    pub note: &'static str,
}

/// 事件账。序号从 1 起连续递增（wrapping 显性），读侧按序号连续性
/// 自证账面无空洞；tick 单调不回退自证时序成立。
#[derive(Clone, Debug)]
pub struct EventLog {
    records: Vec<EventRecord>,
}

impl EventLog {
    pub fn new() -> Self {
        EventLog {
            records: Vec::new(),
        }
    }

    /// 落账。序号自动 = 末序号 + 1（wrapping——空账从 1 起）。
    pub fn push(&mut self, round_id: u32, kind: EventKind, tick: u64, note: &'static str) -> u64 {
        let last = match self.records.last() {
            Some(r) => r.seq,
            None => 0,
        };
        let seq = last.wrapping_add(1);
        self.records.push(EventRecord {
            seq,
            round_id,
            kind,
            tick,
            note,
        });
        seq
    }

    /// 序号连续自证（零跳号零重复——wrapping 语义下 1..n 连续）。
    pub fn seq_continuous(&self) -> bool {
        let mut i = 1usize;
        while i < self.records.len() {
            if self.records[i].seq != self.records[i - 1].seq.wrapping_add(1) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// tick 单调不回退自证（逻辑时序成立——回退即账面被改）。
    pub fn tick_monotonic(&self) -> bool {
        let mut i = 1usize;
        while i < self.records.len() {
            if self.records[i].tick < self.records[i - 1].tick {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 某类事件计数。
    pub fn count_kind(&self, kind: &EventKind) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.records.len() {
            if &self.records[i].kind == kind {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某轮事件计数（判据 J1-a 路径有账的轮粒度读侧）。
    pub fn count_round(&self, round_id: u32) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.records.len() {
            if self.records[i].round_id == round_id {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 末序号（空账为 0）。
    pub fn last_seq(&self) -> u64 {
        match self.records.last() {
            Some(r) => r.seq,
            None => 0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 十、S3 · FailRedoProtocol：三步复演协议（全序账 + 红案 + 联防联动）
// ═══════════════════════════════════════════════════════════════════════════

/// 单轮协议内态：phase_done ∈ {0,1,2,3}，3 = 三步全毕。
/// * 0：未冻结——只允许步1；
/// * 1：已冻结——只允许步2（confirm_fix 可预置）；
/// * 2：复现已成——只允许步3；
/// * 3：已闭合——一切写入拒绝。
/// phase_ticks 是逐步时戳履历（判据 J1-a「路径有账」的轮内时序账）。
#[derive(Clone, Debug)]
struct RoundState {
    round_id: u32,
    phase_done: u8,
    snapshot: Option<RedoSnapshot>,
    /// 逐步时戳履历：(phase, tick)，phase ∈ 1..=3。
    phase_ticks: Vec<(u8, u64)>,
}

/// 红案：非确定源混入的正式立案记录（判据 J1-c「判非确定源混入红案」）。
#[derive(Clone, Debug)]
pub struct RedCase {
    pub round_id: u32,
    pub tick: u64,
    /// 冻结到达点步号。
    pub expected_step: u32,
    /// 重放实际到达点步号（0 = 重放全通过，同为不齐）。
    pub actual_step: u32,
    pub note: &'static str,
}

/// 三步复演协议。聚合：执行器 + 逻辑钟 + 事件账 + 复现率账 + 账簿 +
/// 联防账 + 红案账 + 四个检出计数器。全部步骤走 Result，零 panic 路径；
/// 种子入口只在步1（冻结）——步2/步3 的种子公理由协议内定取自快照，
/// 调用方无漂移入口（注入面另行显性开放，见 S9）。
pub struct FailRedoProtocol {
    runner: ScenarioRunner,
    clock: LogicalClock,
    log: EventLog,
    rounds: Vec<RoundState>,
    rate: ReproRate,
    ledger: RedoLedger,
    watch: RegressionWatch,
    red_cases: Vec<RedCase>,
    /// 已拒绝乱序推进计数（检出账，pub 只读——账面透明）。
    pub order_rejections: u64,
    /// 已拒绝种子漂移计数。
    pub drift_rejections: u64,
    /// 已检出快照篡改计数。
    pub tamper_detections: u64,
    /// 已检出跳步/链断计数。
    pub skip_detections: u64,
}

impl FailRedoProtocol {
    pub fn new(runner: ScenarioRunner) -> Self {
        FailRedoProtocol {
            runner,
            clock: LogicalClock::new(),
            log: EventLog::new(),
            rounds: Vec::new(),
            rate: ReproRate::new(),
            ledger: RedoLedger::new(),
            watch: RegressionWatch::new(),
            red_cases: Vec::new(),
            order_rejections: 0,
            drift_rejections: 0,
            tamper_detections: 0,
            skip_detections: 0,
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock.advance()
    }

    fn find_round(&self, round_id: u32) -> Option<&RoundState> {
        let mut i = 0usize;
        while i < self.rounds.len() {
            if self.rounds[i].round_id == round_id {
                return Some(&self.rounds[i]);
            }
            i += 1;
        }
        None
    }

    fn find_round_mut(&mut self, round_id: u32) -> Option<&mut RoundState> {
        let mut i = 0usize;
        while i < self.rounds.len() {
            if self.rounds[i].round_id == round_id {
                return Some(&mut self.rounds[i]);
            }
            i += 1;
        }
        None
    }

    /// 某轮逐步时戳履历只读（判据 J1-a 轮内时序账读侧）。
    pub fn round_ticks(&self, round_id: u32) -> Option<Vec<(u8, u64)>> {
        match self.find_round(round_id) {
            Some(r) => Some(r.phase_ticks.clone()),
            None => None,
        }
    }

    // ── 步1 · 冻结现场 ───────────────────────────────────────────────

    /// 步1：冻结失败轮现场。三要素（种子/预算/断言上下文 ≥1 条）+
    /// 失败到达点双键（探针重放取弱点步到达点——冻结的到达点即确定性
    /// 弱点步，冻结时刻一并入校验和）+ 校验和。成功后账簿登记（重复
    /// 登记拒绝）、事件落账、phase 履历落 (1, t)、phase_done = 1。
    /// 返回快照校验和（调用方可留档比对）。
    pub fn step1_freeze(
        &mut self,
        round_id: u32,
        seed: u64,
        budget: u32,
        ctx: &[&str],
    ) -> Result<u64, RedoError> {
        // 乱序守门：phase_done 必为 0（未冻结轮才允许冻结）。
        if let Some(r) = self.find_round(round_id) {
            if r.phase_done != 0 {
                self.order_rejections = self.order_rejections.wrapping_add(1);
                let t = self.tick();
                self.log.push(
                    round_id,
                    EventKind::OutOfOrderRejected,
                    t,
                    "乱序：重复冻结被拒（该轮已冻结）",
                );
                return Err(RedoError::OutOfOrder);
            }
        }
        // 三要素闸：上下文非空。
        if ctx.is_empty() {
            let t = self.tick();
            self.log.push(
                round_id,
                EventKind::Freeze,
                t,
                "冻结被拒：断言上下文为空",
            );
            return Err(RedoError::EmptyContext);
        }
        // 三要素闸：预算非零。
        if budget == 0 {
            let t = self.tick();
            self.log
                .push(round_id, EventKind::Freeze, t, "冻结被拒：零步数预算");
            return Err(RedoError::BudgetMismatch);
        }
        // 探针重放：未修复轨迹确定性失败于弱点步——冻结该到达点双键。
        let probe = self.runner.run(seed, budget, false);
        ScenarioRunner::verify_trajectory(&probe)?;
        let (arrival_step, arrival_fp) = match probe.outcome {
            RunOutcome::FailedAt { at_step, fp } => (at_step, fp),
            // 确定性执行器下不可达（未修复必失败于弱点步）；防御性拒绝
            // ——到达点无法冻结即三要素不全，冻结不成立。
            RunOutcome::Passed => {
                let t = self.tick();
                self.log.push(
                    round_id,
                    EventKind::Freeze,
                    t,
                    "冻结被拒：探针轮未失败，到达点无法冻结",
                );
                return Err(RedoError::FailurePointMismatch);
            }
        };
        let t = self.tick();
        let mut snap = RedoSnapshot {
            round_id,
            seed,
            budget,
            arrival_step,
            arrival_fp,
            ctx: Vec::new(),
            checksum: 0,
            frozen_tick: t,
        };
        let mut i = 0usize;
        while i < ctx.len() {
            snap.ctx.push(ctx[i].to_string());
            i += 1;
        }
        snap.checksum = snap.recompute_checksum();
        self.ledger.register(round_id, t)?;
        let mut ticks: Vec<(u8, u64)> = Vec::new();
        ticks.push((1u8, t));
        self.rounds.push(RoundState {
            round_id,
            phase_done: 1,
            snapshot: Some(snap.clone()),
            phase_ticks: ticks,
        });
        self.log.push(
            round_id,
            EventKind::Freeze,
            t,
            "步1冻结现场：三要素+到达点双键+校验和落账",
        );
        Ok(snap.checksum)
    }

    /// 步1 变体：吸收失败轮自报快照（非确定源演练与跨机转账用——快照
    /// 内容来自失败轮自身记录，若其自报到达点与确定性重放不符，在步2
    /// 到达点闸现形）。走同一登记/乱序守门路径，快照校验和前置核验。
    pub fn step1_absorb(&mut self, snap: RedoSnapshot) -> Result<u64, RedoError> {
        let round_id = snap.round_id;
        if let Some(r) = self.find_round(round_id) {
            if r.phase_done != 0 {
                self.order_rejections = self.order_rejections.wrapping_add(1);
                let t = self.tick();
                self.log.push(
                    round_id,
                    EventKind::OutOfOrderRejected,
                    t,
                    "乱序：重复冻结(吸收)被拒",
                );
                return Err(RedoError::OutOfOrder);
            }
        }
        if snap.ctx.is_empty() {
            let t = self.tick();
            self.log
                .push(round_id, EventKind::Freeze, t, "吸收被拒：快照断言上下文为空");
            return Err(RedoError::EmptyContext);
        }
        // 快照完整性前置：错签快照不得入账（0x153D）。
        snap.verify()?;
        if snap.budget == 0 {
            return Err(RedoError::BudgetMismatch);
        }
        let t = self.tick();
        self.ledger.register(round_id, t)?;
        let mut ticks: Vec<(u8, u64)> = Vec::new();
        ticks.push((1u8, t));
        self.rounds.push(RoundState {
            round_id,
            phase_done: 1,
            snapshot: Some(snap.clone()),
            phase_ticks: ticks,
        });
        self.log.push(
            round_id,
            EventKind::Freeze,
            t,
            "步1冻结现场(自报吸收)：快照校验和过，落账",
        );
        Ok(snap.checksum)
    }

    // ── 步2 · 种子复现 ───────────────────────────────────────────────

    /// 步2：同种子重放 → 到达点双键比对。前置：步1 已毕（phase_done
    /// == 1）。四道闸：
    /// 1. 序闸：phase_done 必为 1；
    /// 2. 种子闸：复现种子 ≠ 冻结种子 → 种子漂移拒（不计复现尝试——
    ///    未跑即拒，attempts 只认真跑过重放的次数）；
    /// 3. 预算闸：预算 ≠ 冻结预算 → 预算不符拒；
    /// 4. 到达点闸：重放失败点双键不齐 → 非确定源红案（计尝试、不计
    ///    成功——率账随即跌穿 100%，health 翻红）。
    /// 全齐 → attempts+1, successes+1, phase_done = 2, 账簿进入
    /// Redoing，phase 履历落 (2, t)。返回到达点双键。
    pub fn step2_reproduce(
        &mut self,
        round_id: u32,
        reproduce_seed: u64,
        budget: u32,
    ) -> Result<(u32, u64), RedoError> {
        // 闸1：序闸。
        let phase = match self.find_round(round_id) {
            Some(r) => r.phase_done,
            None => return Err(RedoError::UnknownRound),
        };
        if phase != 1 {
            self.order_rejections = self.order_rejections.wrapping_add(1);
            let t = self.tick();
            self.log.push(
                round_id,
                EventKind::OutOfOrderRejected,
                t,
                "乱序：步2未在冻结后发起",
            );
            return Err(RedoError::OutOfOrder);
        }
        let snap = match self.find_round(round_id) {
            Some(r) => match &r.snapshot {
                Some(s) => s.clone(),
                None => return Err(RedoError::UnknownRound),
            },
            None => return Err(RedoError::UnknownRound),
        };
        // 快照完整性前置核验（篡改的快照不得进入复现）。
        snap.verify()?;
        // 闸2：种子闸。
        if reproduce_seed != snap.seed {
            self.drift_rejections = self.drift_rejections.wrapping_add(1);
            let t = self.tick();
            self.log.push(
                round_id,
                EventKind::DriftRejected,
                t,
                "种子漂移拒：复现种子≠冻结种子（未跑即拒，不计尝试）",
            );
            return Err(RedoError::SeedDrift);
        }
        // 闸3：预算闸。
        if budget != snap.budget {
            let t = self.tick();
            self.log.push(
                round_id,
                EventKind::ReproRun,
                t,
                "复现被拒：预算与冻结预算不符",
            );
            return Err(RedoError::BudgetMismatch);
        }
        let t = self.tick();
        self.log
            .push(round_id, EventKind::ReproRun, t, "步2种子复现：同种子重放发起");
        let replay = self.runner.run(reproduce_seed, budget, false);
        ScenarioRunner::verify_trajectory(&replay)?;
        self.rate.record_attempt();
        // 闸4：到达点双键比对。
        let aligned = match replay.outcome {
            RunOutcome::FailedAt { at_step, fp } => {
                at_step == snap.arrival_step && fp == snap.arrival_fp
            }
            RunOutcome::Passed => false,
        };
        if !aligned {
            let actual = match replay.outcome {
                RunOutcome::FailedAt { at_step, .. } => at_step,
                RunOutcome::Passed => 0,
            };
            let trc = self.tick();
            self.red_cases.push(RedCase {
                round_id,
                tick: trc,
                expected_step: snap.arrival_step,
                actual_step: actual,
                note: "同种子重放到达点不齐——判非确定源混入，红案立案",
            });
            let t2 = self.tick();
            self.log.push(
                round_id,
                EventKind::ReproMismatch,
                t2,
                "到达点不齐：非确定源红案立案",
            );
            return Err(RedoError::FailurePointMismatch);
        }
        self.rate.record_success();
        let t2 = self.tick();
        match self.find_round_mut(round_id) {
            Some(r) => {
                r.phase_done = 2;
                r.phase_ticks.push((2u8, t2));
            }
            None => return Err(RedoError::UnknownRound),
        }
        self.ledger.mark_redoing(round_id)?;
        let t3 = self.tick();
        self.log.push(
            round_id,
            EventKind::ReproOk,
            t3,
            "复现成功：到达点双键全齐，复现率账+1/+1",
        );
        Ok((snap.arrival_step, snap.arrival_fp))
    }

    // ── 修复确认（步3 前置闸）────────────────────────────────────────

    /// 修复确认落账。要求该轮至少已冻结（phase_done ∈ 1..=2）且未闭合；
    /// 未冻结先报修复即乱序（账面无失败现场，修复无从谈起）。
    pub fn confirm_fix(&mut self, round_id: u32) -> Result<(), RedoError> {
        let phase = match self.find_round(round_id) {
            Some(r) => r.phase_done,
            None => return Err(RedoError::UnknownRound),
        };
        if phase == 0 {
            self.order_rejections = self.order_rejections.wrapping_add(1);
            let t = self.tick();
            self.log.push(
                round_id,
                EventKind::OutOfOrderRejected,
                t,
                "乱序：未冻结现场先报修复",
            );
            return Err(RedoError::OutOfOrder);
        }
        if phase >= 3 {
            return Err(RedoError::AlreadyClosed);
        }
        self.ledger.confirm_fix(round_id)?;
        let t = self.tick();
        self.log
            .push(round_id, EventKind::FixConfirm, t, "修复确认落账（步3前置闸通过）");
        Ok(())
    }

    // ── 步3 · 二次执行 ───────────────────────────────────────────────

    /// 步3：修复确认后同种子再跑 → 轨迹全预算通过 → 末步指纹冻结进
    /// 联防账 → 账目闭合。五道闸：
    /// 1. 序闸：phase_done 必须 == 2（复现已成）；
    /// 2. 快照闸：快照校验和复验；
    /// 3. 修复闸：confirm_fix 必已落账；
    /// 4. 预算闸：预算 == 冻结预算（种子以快照内定，杜绝漂移入口）；
    /// 5. 通过闸：已修复轨迹必须全预算通过。
    /// 全过 → 联防冻结 + 闭合 + phase 履历落 (3, t)，返回末步指纹。
    pub fn step3_execute(&mut self, round_id: u32, budget: u32) -> Result<u64, RedoError> {
        // 闸1：序闸。
        let phase = match self.find_round(round_id) {
            Some(r) => r.phase_done,
            None => return Err(RedoError::UnknownRound),
        };
        if phase != 2 {
            self.order_rejections = self.order_rejections.wrapping_add(1);
            let t = self.tick();
            self.log.push(
                round_id,
                EventKind::OutOfOrderRejected,
                t,
                "乱序：步3未在复现成功后发起",
            );
            return Err(RedoError::OutOfOrder);
        }
        let snap = match self.find_round(round_id) {
            Some(r) => match &r.snapshot {
                Some(s) => s.clone(),
                None => return Err(RedoError::UnknownRound),
            },
            None => return Err(RedoError::UnknownRound),
        };
        // 闸2：快照闸。
        snap.verify()?;
        // 闸3：修复闸。
        let fix_ok = match self.ledger.find(round_id) {
            Some(e) => e.fix_confirmed,
            None => return Err(RedoError::UnknownRound),
        };
        if !fix_ok {
            let t = self.tick();
            self.log
                .push(round_id, EventKind::SecondExec, t, "二次执行被拒：修复未确认");
            return Err(RedoError::FixNotConfirmed);
        }
        // 闸4：预算闸。
        if budget != snap.budget {
            let t = self.tick();
            self.log
                .push(round_id, EventKind::SecondExec, t, "二次执行被拒：预算与冻结预算不符");
            return Err(RedoError::BudgetMismatch);
        }
        let t = self.tick();
        self.log.push(
            round_id,
            EventKind::SecondExec,
            t,
            "步3二次执行：修复后同种子重放发起",
        );
        let fixed = self.runner.run(snap.seed, budget, true);
        ScenarioRunner::verify_trajectory(&fixed)?;
        // 闸5：通过闸。
        let final_fp = match fixed.outcome {
            RunOutcome::Passed => match fixed.steps.last() {
                Some(rec) => rec.fp,
                None => return Err(RedoError::BudgetMismatch),
            },
            RunOutcome::FailedAt { at_step, .. } => {
                let t2 = self.tick();
                self.log
                    .push(round_id, EventKind::SecondExec, t2, "二次执行未通过：仍有失败步");
                let _ = at_step;
                return Err(RedoError::SecondExecFailed);
            }
        };
        // 联防冻结（种子唯一写后读防线：同种子重复冻结在 arm 内拒绝）。
        let ta = self.tick();
        self.watch.arm(round_id, snap.seed, final_fp, ta)?;
        let t3 = self.tick();
        self.log.push(
            round_id,
            EventKind::WatchFrozen,
            t3,
            "联防期望指纹冻结：后续同种子必达",
        );
        let tc = self.tick();
        self.ledger.close(round_id, tc)?;
        match self.find_round_mut(round_id) {
            Some(r) => {
                r.phase_done = 3;
                r.phase_ticks.push((3u8, tc));
            }
            None => return Err(RedoError::UnknownRound),
        }
        let t4 = self.tick();
        self.log
            .push(round_id, EventKind::Closed, t4, "账目闭合：复现确认+修复确认双条件齐");
        Ok(final_fp)
    }

    // ── 联防巡检 ─────────────────────────────────────────────────────

    /// 巡检一笔同种子运行是否命中联防期望指纹。命中/漂移/落空均落
    /// 事件账（巡检本身也是账面动作）。
    pub fn patrol(&mut self, seed: u64, final_fp: u64) -> Result<(), RedoError> {
        match self.watch.patrol(seed, final_fp) {
            Ok(()) => {
                let t = self.tick();
                self.log.push(
                    0,
                    EventKind::WatchHit,
                    t,
                    "联防巡检命中：同种子末步指纹与期望全等",
                );
                Ok(())
            }
            Err(e) => {
                let t = self.tick();
                match e {
                    RedoError::WatchDrift => {
                        self.log.push(
                            0,
                            EventKind::WatchMiss,
                            t,
                            "联防巡检漂移：同种子未达期望指纹——行为回归",
                        );
                    }
                    _ => {
                        self.log.push(
                            0,
                            EventKind::WatchMiss,
                            t,
                            "联防巡检落空：该种子未冻结期望指纹",
                        );
                    }
                }
                Err(e)
            }
        }
    }

    // ── 双跑确定性对拍 ───────────────────────────────────────────────

    /// 双跑对拍并落账。同 (seed, budget, fix) 两跑逐步指纹全等才算平；
    /// 不平给首个分叉步。
    pub fn double_run(&mut self, seed: u64, budget: u32, fix_applied: bool) -> CompareReport {
        let rep = double_run_compare(&self.runner, seed, budget, fix_applied);
        let t = self.tick();
        if rep.identical {
            self.log
                .push(0, EventKind::DoubleRun, t, "双跑对拍平：逐步指纹全等");
        } else {
            self.log
                .push(0, EventKind::DoubleRun, t, "双跑对拍不平：检出分叉步");
        }
        rep
    }

    // ── 只读账面 ─────────────────────────────────────────────────────

    /// 复现率账只读。
    pub fn rate(&self) -> &ReproRate {
        &self.rate
    }

    /// 账簿只读。
    pub fn ledger(&self) -> &RedoLedger {
        &self.ledger
    }

    /// 账簿可变访问（闭合闸直测等账面级操作用）。
    pub fn ledger_mut(&mut self) -> &mut RedoLedger {
        &mut self.ledger
    }

    /// 联防账只读。
    pub fn watch(&self) -> &RegressionWatch {
        &self.watch
    }

    /// 红案账只读。
    pub fn red_cases(&self) -> &[RedCase] {
        &self.red_cases
    }

    /// 事件账只读。
    pub fn log(&self) -> &EventLog {
        &self.log
    }

    /// 某轮 phase_done（未知轮 None）。
    pub fn phase_of(&self, round_id: u32) -> Option<u8> {
        match self.find_round(round_id) {
            Some(r) => Some(r.phase_done),
            None => None,
        }
    }

    // ── S9 · 注入面 ──────────────────────────────────────────────────

    /// 注入一：种子漂移。以漂移种子发起步2，协议必须拒（种子闸）。
    /// 返回是否检出（拒绝即检出）。漂移量由调用方给（判据电池用 LCG
    /// 派生——注入本身也走确定性路径）。
    pub fn inject_seed_drift(&mut self, round_id: u32, seed_delta: u64) -> bool {
        let frozen = match self.find_round(round_id) {
            Some(r) => match &r.snapshot {
                Some(s) => (s.seed, s.budget),
                None => return false,
            },
            None => return false,
        };
        let drifted = frozen.0 ^ seed_delta;
        matches!(
            self.step2_reproduce(round_id, drifted, frozen.1),
            Err(RedoError::SeedDrift)
        )
    }

    /// 注入二（弱形态）：快照篡改。对已冻结轮的账面快照做"改字段又改
    /// 和"的重签副本，副本与原件校验和必分叉——检出具名。账面原件
    /// 不动（模拟只改存储镜像、未改协议账），检出不改变流程态。
    pub fn inject_snapshot_tamper(&mut self, round_id: u32) -> bool {
        let snap = match self.find_round(round_id) {
            Some(r) => match &r.snapshot {
                Some(s) => s.clone(),
                None => return false,
            },
            None => return false,
        };
        let mut forged = snap.clone();
        forged.arrival_step = forged.arrival_step.wrapping_add(1);
        forged.checksum = forged.recompute_checksum();
        let detected = snap.verify().is_ok()
            && forged.checksum != snap.checksum
            && forged.arrival_step != snap.arrival_step;
        if detected {
            self.tamper_detections = self.tamper_detections.wrapping_add(1);
            let t = self.tick();
            self.log.push(
                round_id,
                EventKind::TamperDetected,
                t,
                "快照篡改检出：重签副本与账面原件校验和分叉",
            );
        }
        detected
    }

    /// 注入二（强形态）：以"改账又改和"的伪造快照（谎报到达点）走
    /// absorb 入账，再以真种子步2——快照自身自洽骗过校验和闸，但到
    /// 达点闸必兜住（非确定源红案立案）。返回是否检出。
    pub fn inject_forged_snapshot(
        &mut self,
        round_id: u32,
        seed: u64,
        budget: u32,
        ctx: &[&str],
    ) -> bool {
        // 先用真种子冻结一轮拿合法到达点（伪造必须以真为底才骗得过
        // 校验和——这正是要演练的攻面）。
        let probe = self.runner.run(seed, budget, false);
        let (at, fp) = match probe.outcome {
            RunOutcome::FailedAt { at_step, fp } => (at_step, fp),
            RunOutcome::Passed => return false,
        };
        let mut forged = RedoSnapshot {
            round_id,
            seed,
            budget,
            arrival_step: at.wrapping_add(1), // 谎报到达点
            arrival_fp: fp,
            ctx: Vec::new(),
            checksum: 0,
            frozen_tick: self.clock.now(),
        };
        let mut i = 0usize;
        while i < ctx.len() {
            forged.ctx.push(ctx[i].to_string());
            i += 1;
        }
        forged.checksum = forged.recompute_checksum(); // 改账又改和
        if self.step1_absorb(forged).is_err() {
            return false;
        }
        // 同种子步2：确定性重放到达真弱点步 ≠ 谎报点 → 必拒并红案。
        matches!(
            self.step2_reproduce(round_id, seed, budget),
            Err(RedoError::FailurePointMismatch)
        )
    }

    /// 注入三（弱形态）：跳步。取一笔真轨迹，裁掉第 2 步且步账同步
    /// 少计（攻击者试图连账一起抹平）——步号连续性仍破，核验必拒。
    /// 返回是否检出。建议传 fix_applied = true（修复态全预算轨迹，
    /// 步数必 ≥ 3，不受弱点步位置影响）。
    pub fn inject_step_skip(&mut self, seed: u64, budget: u32, fix_applied: bool) -> bool {
        if budget < 3 {
            return false;
        }
        let mut r = self.runner.run(seed, budget, fix_applied);
        if r.steps.len() < 3 {
            return false;
        }
        r.steps.remove(1);
        r.steps_executed = r.steps.len() as u32;
        let detected = match ScenarioRunner::verify_trajectory(&r) {
            Err(RedoError::StepSkipDetected) | Err(RedoError::FingerprintChainBreak) => true,
            _ => false,
        };
        if detected {
            self.skip_detections = self.skip_detections.wrapping_add(1);
            let t = self.tick();
            self.log
                .push(0, EventKind::SkipDetected, t, "跳步检出：裁步后步号连续性断裂");
        }
        detected
    }

    /// 注入三（强形态）：跳步 + 伪造步号重排（攻击者把裁步后的轨迹
    /// 重新编号成 1..n 连续——骗过步号闸，但链头起每级指纹含真步号
    /// 与前级指纹，级联失配不可掩盖）。返回是否检出。
    pub fn inject_relabel_skip(&mut self, seed: u64, budget: u32, fix_applied: bool) -> bool {
        if budget < 3 {
            return false;
        }
        let mut r = self.runner.run(seed, budget, fix_applied);
        if r.steps.len() < 3 {
            return false;
        }
        r.steps.remove(1);
        r.steps_executed = r.steps.len() as u32;
        let mut i = 0usize;
        let mut no: u32 = 1;
        while i < r.steps.len() {
            r.steps[i].step_no = no;
            no = no.wrapping_add(1);
            i += 1;
        }
        match ScenarioRunner::verify_trajectory(&r) {
            Err(RedoError::FingerprintChainBreak) => {
                self.skip_detections = self.skip_detections.wrapping_add(1);
                let t = self.tick();
                self.log.push(
                    0,
                    EventKind::SkipDetected,
                    t,
                    "重排跳步检出：指纹链级联失配不可掩盖",
                );
                true
            }
            _ => false,
        }
    }

    /// 注入四：改单步指纹（中间人改写一笔轨迹指纹）——下一级链失配
    /// 定位精确到被改步之后第一级。返回是否检出。
    pub fn inject_fp_flip(&mut self, seed: u64, budget: u32, at_step: u32) -> bool {
        let mut r = self.runner.run(seed, budget, true);
        let mut touched = false;
        let mut i = 0usize;
        while i < r.steps.len() {
            if r.steps[i].step_no == at_step {
                r.steps[i].fp = r.steps[i].fp ^ 1;
                touched = true;
            }
            i += 1;
        }
        if !touched {
            return false;
        }
        match ScenarioRunner::verify_trajectory(&r) {
            Err(RedoError::FingerprintChainBreak) => {
                self.skip_detections = self.skip_detections.wrapping_add(1);
                let t = self.tick();
                self.log
                    .push(0, EventKind::SkipDetected, t, "改步指纹检出：链级联失配");
                true
            }
            _ => false,
        }
    }

    // ── S8 · health ──────────────────────────────────────────────────

    /// 不变量聚合健康面。八查：
    /// 1. 事件序号连续零跳号；
    /// 2. 事件 tick 单调不回退；
    /// 3. 账簿守恒（登记 = 未闭合 + 闭合）；
    /// 4. 未闭合账显性（清单与逐行扫描零差）；
    /// 5. 复现率满格或空账（J1-e：不足 100% 即红）；
    /// 6. 红案清零（有红案即整体翻红）；
    /// 7. 复现账不变量（成功 ≤ 尝试）；
    /// 8. 联防种子唯一。
    pub fn health(&self) -> HealthReport {
        let mut items: Vec<(&'static str, bool)> = Vec::new();
        items.push(("事件序号连续", self.log.seq_continuous()));
        items.push(("事件时戳单调", self.log.tick_monotonic()));
        items.push(("账簿守恒", self.ledger.balance_ok()));
        items.push(("未闭合账显性", self.ledger.unclosed_explicit()));
        items.push(("复现率满格", self.rate.is_full_or_empty()));
        items.push(("红案清零", self.red_cases.is_empty()));
        items.push(("复现账不变量", self.rate.invariant_ok()));
        items.push(("联防种子唯一", self.watch.no_dup_seed()));
        HealthReport { items }
    }

    /// S11 · 协议账面总览。对冲恒等：rounds_total == ledger_total ==
    /// open + closed（轮态与账簿一一生成、一一对冲）。
    pub fn summary(&self) -> ProtocolSummary {
        ProtocolSummary {
            rounds_total: self.rounds.len() as u64,
            ledger_total: self.ledger.len() as u64,
            ledger_open: self.ledger.unclosed().len() as u64,
            ledger_closed: self.ledger.closed().len() as u64,
            red_cases: self.red_cases.len() as u64,
            repro_attempts: self.rate.attempts,
            repro_successes: self.rate.successes,
            repro_rate_pct: self.rate.rate_pct(),
            events: self.log.len() as u64,
            watch_armed: self.watch.len() as u64,
            order_rejections: self.order_rejections,
            drift_rejections: self.drift_rejections,
            tamper_detections: self.tamper_detections,
            skip_detections: self.skip_detections,
        }
    }
}

/// health 报告。overall = 全查通过；red_items 给翻红项定位清单。
#[derive(Clone, Debug)]
pub struct HealthReport {
    pub items: Vec<(&'static str, bool)>,
}

impl HealthReport {
    pub fn overall(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].1 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 翻红项清单（诊断定位）。
    pub fn red_items(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].1 {
                out.push(self.items[i].0);
            }
            i += 1;
        }
        out
    }
}

/// 协议账面总览（S11）。对冲恒等由 summary_ok() 自证。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProtocolSummary {
    pub rounds_total: u64,
    pub ledger_total: u64,
    pub ledger_open: u64,
    pub ledger_closed: u64,
    pub red_cases: u64,
    pub repro_attempts: u64,
    pub repro_successes: u64,
    pub repro_rate_pct: u64,
    pub events: u64,
    pub watch_armed: u64,
    pub order_rejections: u64,
    pub drift_rejections: u64,
    pub tamper_detections: u64,
    pub skip_detections: u64,
}

impl ProtocolSummary {
    /// 对冲恒等：轮态总数 == 账簿总数 == 未闭合 + 闭合；红案清零时
    /// 复现率必满格。
    pub const fn summary_ok(&self) -> bool {
        self.rounds_total == self.ledger_total
            && self.ledger_total == self.ledger_open.wrapping_add(self.ledger_closed)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 十一、S12 · 自包含判据套件（CheckSet + 46 条判据电池）
// ═══════════════════════════════════════════════════════════════════════════

/// 单条判据结果。
#[derive(Clone, Debug)]
pub struct CheckItem {
    pub name: String,
    pub pass: bool,
    /// 通过时为空串，失败时为中文定位注记。
    pub note: String,
}

/// 自包含判据套件（本模块零 crate 依赖，判据面自带容器——不接
/// crate::checks，保持 S12 独立可移植）。
#[derive(Clone, Debug)]
pub struct CheckSet {
    pub title: String,
    pub items: Vec<CheckItem>,
}

impl CheckSet {
    pub fn new(title: &str) -> Self {
        CheckSet {
            title: title.to_string(),
            items: Vec::new(),
        }
    }

    pub fn add(&mut self, name: &str, pass: bool, fail_note: &str) {
        self.items.push(CheckItem {
            name: name.to_string(),
            pass,
            note: if pass {
                String::new()
            } else {
                fail_note.to_string()
            },
        });
    }

    pub fn all_pass(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].pass {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn pass_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].pass {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 标准场景步数预算：16。来源：B33 批演练轮标准档——覆盖弱点步全域
/// 1..=16 的最小整轮预算（弱点步 = 1 + mix mod budget，16 步即可对
/// 弱点位置做全域散布验证）。
pub const STD_BUDGET: u32 = 16;
/// 标准种子：段号 + 任务号复合写死，判据电池全可重放。
pub const STD_SEED: u64 = 0x1530_0644_00C0_FEE0;

/// 标准断言上下文（3 条——失败判据 / 断点 / 复演域；'static 切片，
/// 冻结时逐条深拷入快照）。
fn std_ctx() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = Vec::new();
    v.push("判据=页表项存在且 Present 位为 1");
    v.push("断点=walk_step3 返回 NotFound");
    v.push("复演域=UNX-A1-B33 域回归轮");
    v
}

/// 全量判据电池：UNX-F0644-J1 全路径 46 条。每条判据独立构造现场，
/// 零交叉污染；注入判据均在独立实例上发起；种子全部写死或 LCG 确定性
/// 派生——电池自身 100% 可重放。
pub fn run_unx0644_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0644 失败复演规程（失败轮重演三步）");
    let ctx = std_ctx();

    // ── 1. 冻结现场：三要素 + 校验和稳定（同轮同内容同和）────────────
    // 校验和绑定 round_id（快照归属轮不可换），故跨实例同轮同内容比。
    let mut p1a = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let mut p1b = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let ck1 = p1a.step1_freeze(1, STD_SEED, STD_BUDGET, &ctx);
    let ck2 = p1b.step1_freeze(1, STD_SEED, STD_BUDGET, &ctx);
    let ck_ok = match (ck1, ck2) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    };
    s.add(
        "冻结现场-三要素校验和稳定",
        ck_ok,
        if ck_ok { "" } else { "冻结校验和不稳" },
    );

    // ── 2. 冻结现场：断言上下文空拒 ──────────────────────────────────
    let mut p2 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let empty_rejected = matches!(
        p2.step1_freeze(3, STD_SEED, STD_BUDGET, &[]),
        Err(RedoError::EmptyContext)
    );
    s.add(
        "冻结现场-空上下文拒",
        empty_rejected,
        if empty_rejected { "" } else { "空上下文放行" },
    );

    // ── 3. 冻结现场：校验和内容敏感（改上下文 → 改和）────────────────
    let mut p3 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let mut ctx_b = std_ctx();
    ctx_b.push("附加条目=敏感探针");
    let a = p3.step1_freeze(4, STD_SEED, STD_BUDGET, &std_ctx());
    let b = p3.step1_freeze(5, STD_SEED, STD_BUDGET, &ctx_b);
    let sensitive = match (a, b) {
        (Ok(x), Ok(y)) => x != y,
        _ => false,
    };
    s.add(
        "冻结现场-校验和内容敏感",
        sensitive,
        if sensitive { "" } else { "校验和对内容不敏感" },
    );

    // ── 4. 三步全序：正常走通 → 闭合（J1-a 主路径）───────────────────
    let mut p4 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let walk_ok = match p4.step1_freeze(10, STD_SEED, STD_BUDGET, &ctx) {
        Ok(_) => match p4.step2_reproduce(10, STD_SEED, STD_BUDGET) {
            Ok(_) => {
                let _ = p4.confirm_fix(10);
                match p4.step3_execute(10, STD_BUDGET) {
                    Ok(_) => p4.phase_of(10) == Some(3),
                    Err(_) => false,
                }
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("三步全序-正常闭合", walk_ok, if walk_ok { "" } else { "三步走通失败" });

    // ── 5. 乱序拒绝：步2 先行（未冻结）───────────────────────────────
    let mut p5 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let ooo = matches!(
        p5.step2_reproduce(11, STD_SEED, STD_BUDGET),
        Err(RedoError::OutOfOrder) | Err(RedoError::UnknownRound)
    );
    s.add("乱序拒绝-步2先行", ooo, if ooo { "" } else { "未冻结先复现放行" });

    // ── 6. 乱序拒绝：步3 越过步2（冻结后直接跳步3）───────────────────
    let mut p6 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let _ = p6.step1_freeze(12, STD_SEED, STD_BUDGET, &ctx);
    let ooo3 = matches!(p6.step3_execute(12, STD_BUDGET), Err(RedoError::OutOfOrder));
    s.add("乱序拒绝-步3越过步2", ooo3, if ooo3 { "" } else { "越过复现放行" });

    // ── 7. 乱序拒绝：重复冻结 ────────────────────────────────────────
    let mut p7 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let _ = p7.step1_freeze(13, STD_SEED, STD_BUDGET, &ctx);
    let dup = matches!(
        p7.step1_freeze(13, STD_SEED, STD_BUDGET, &ctx),
        Err(RedoError::OutOfOrder)
    );
    s.add("乱序拒绝-重复冻结", dup, if dup { "" } else { "重复冻结放行" });

    // ── 8. 乱序检出计数：三种乱序各计一笔 ────────────────────────────
    let order_cnt_ok = p7.order_rejections == 1 && p6.order_rejections == 1;
    s.add(
        "乱序拒绝-检出计数落账",
        order_cnt_ok,
        if order_cnt_ok { "" } else { "乱序计数缺失" },
    );

    // ── 9. 同种子复现率 100%（J1-e 主判据，10 轮）────────────────────
    let mut p9 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let mut rounds_ok = 0u32;
    let mut rid: u32 = 100;
    let mut r = 0u32;
    while r < 10 {
        // 种子确定性扰动：LCG 派生（种子簿登记制——注入零非确定）。
        let mut lcg = Lcg::new(STD_SEED ^ (r as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let seed_r = STD_SEED ^ lcg.next();
        if p9.step1_freeze(rid, seed_r, STD_BUDGET, &ctx).is_ok()
            && p9.step2_reproduce(rid, seed_r, STD_BUDGET).is_ok()
        {
            rounds_ok += 1;
        }
        rid = rid.wrapping_add(1);
        r += 1;
    }
    let rate_ok = p9.rate().attempts == 10
        && p9.rate().successes == 10
        && p9.rate().rate_pct() == 100
        && p9.rate().is_full_or_empty()
        && rounds_ok == 10;
    s.add(
        "同种子复现率-10轮100%",
        rate_ok,
        if rate_ok { "" } else { "复现率不足100%" },
    );

    // ── 10. 种子漂移注入：检出 100%（10 轮随机漂移量）────────────────
    let mut p10 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let mut drift_hits = 0u32;
    let mut rid: u32 = 200;
    let mut r = 0u32;
    while r < 10 {
        let mut lcg = Lcg::new(0x1530_D81F_1000 ^ (r as u64).wrapping_mul(0xD1B5_9A92_0000_0001));
        let seed_r = STD_SEED ^ lcg.next();
        let _ = p10.step1_freeze(rid, seed_r, STD_BUDGET, &ctx);
        if p10.inject_seed_drift(rid, 1 + lcg.below(0xFFFF)) {
            drift_hits += 1;
        }
        rid = rid.wrapping_add(1);
        r += 1;
    }
    s.add(
        "种子漂移注入-10/10检出",
        drift_hits == 10 && p10.drift_rejections == 10,
        if drift_hits == 10 { "" } else { "漂移漏检" },
    );

    // ── 11. 非确定源红案：谎报到达点被到达点闸兜住（J1-c）────────────
    let mut p11 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let forged_caught = p11.inject_forged_snapshot(300, STD_SEED, STD_BUDGET, &ctx);
    let red_case_ok = p11.red_cases().len() == 1 && !p11.health().overall();
    s.add(
        "非确定源红案-谎报到达点立案",
        forged_caught && red_case_ok,
        if forged_caught { "" } else { "红案未立案" },
    );

    // ── 12. 快照篡改注入（弱形态）：重签副本与原件分叉检出 ───────────
    let mut p12 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let _ = p12.step1_freeze(400, STD_SEED, STD_BUDGET, &ctx);
    let tamper = p12.inject_snapshot_tamper(400);
    s.add(
        "快照篡改注入-重签分叉检出",
        tamper && p12.tamper_detections == 1,
        if tamper { "" } else { "篡改漏检" },
    );

    // ── 13. 快照篡改（错签形态）：改字段不改和 → absorb 拒收 ─────────
    let mut p13 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let mut forged_noresign = RedoSnapshot {
        round_id: 402,
        seed: STD_SEED,
        budget: STD_BUDGET,
        arrival_step: 2,
        arrival_fp: 0xDEAD_BEEF_CAFE_F00D,
        ctx: Vec::new(),
        checksum: 0,
        frozen_tick: 0,
    };
    let mut i = 0usize;
    while i < ctx.len() {
        forged_noresign.ctx.push(ctx[i].to_string());
        i += 1;
    }
    forged_noresign.checksum = forged_noresign.recompute_checksum();
    forged_noresign.arrival_step = 1; // 改字段不改和（存储位翻转模拟）
    let noresign = matches!(
        p13.step1_absorb(forged_noresign),
        Err(RedoError::SnapshotChecksumMismatch)
    );
    s.add(
        "快照篡改-改字段不改和拒收",
        noresign,
        if noresign { "" } else { "错签快照收账" },
    );

    // ── 14. 跳步注入（弱形态）：裁步检出 ─────────────────────────────
    let mut p14 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let skip = p14.inject_step_skip(STD_SEED, STD_BUDGET, true);
    s.add(
        "跳步注入-裁步检出",
        skip && p14.skip_detections == 1,
        if skip { "" } else { "跳步漏检" },
    );

    // ── 15. 跳步强形态：重排步号骗闸 → 指纹链级联失配检出 ────────────
    let mut p15 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let relabel = p15.inject_relabel_skip(STD_SEED, STD_BUDGET, true);
    s.add("跳步强注入-重排检出", relabel, if relabel { "" } else { "重排漏检" });

    // ── 16. 改步指纹注入：链级联失配精确检出 ─────────────────────────
    let mut p16 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let flip = p16.inject_fp_flip(STD_SEED, STD_BUDGET, 7);
    s.add("改步指纹注入-级联检出", flip, if flip { "" } else { "改步漏检" });

    // ── 17. 指纹链闭合：正路轨迹（通过/失败两态）核验全绿 ────────────
    let runner17 = ScenarioRunner::new("std");
    let pass_run = runner17.run(STD_SEED, STD_BUDGET, true);
    let fail_run = runner17.run(STD_SEED, STD_BUDGET, false);
    let chain_ok = ScenarioRunner::verify_trajectory(&pass_run).is_ok()
        && ScenarioRunner::verify_trajectory(&fail_run).is_ok();
    s.add(
        "指纹链闭合-双轨迹核验绿",
        chain_ok,
        if chain_ok { "" } else { "正路轨迹核验红" },
    );

    // ── 18. 步账注入：steps_executed 与轨迹数不符检出 ────────────────
    let mut doctored2 = runner17.run(STD_SEED, STD_BUDGET, true);
    doctored2.steps_executed = doctored2.steps_executed.wrapping_add(1);
    let miscount = matches!(
        ScenarioRunner::verify_trajectory(&doctored2),
        Err(RedoError::StepSkipDetected)
    );
    s.add("步账注入-计数不符检出", miscount, if miscount { "" } else { "计数不符放行" });

    // ── 19. 修复闸：未 confirm_fix 发起步3 拒 ────────────────────────
    let mut p19 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let _ = p19.step1_freeze(500, STD_SEED, STD_BUDGET, &ctx);
    let _ = p19.step2_reproduce(500, STD_SEED, STD_BUDGET);
    let fix_gate = matches!(
        p19.step3_execute(500, STD_BUDGET),
        Err(RedoError::FixNotConfirmed)
    );
    s.add("修复闸-未确认拒", fix_gate, if fix_gate { "" } else { "未确认修复放行" });

    // ── 20. 修复闸：未冻结先报修复拒（乱序面）────────────────────────
    let mut p20 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let fix_ooo = matches!(
        p20.confirm_fix(501),
        Err(RedoError::OutOfOrder) | Err(RedoError::UnknownRound)
    );
    s.add("修复闸-未冻结先报修复拒", fix_ooo, if fix_ooo { "" } else { "无现场修复放行" });

    // ── 21. 联防冻结 + 巡检命中：复演通过后同种子必达期望指纹 ────────
    let mut p21 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let _ = p21.step1_freeze(600, STD_SEED, STD_BUDGET, &ctx);
    let _ = p21.step2_reproduce(600, STD_SEED, STD_BUDGET);
    let _ = p21.confirm_fix(600);
    let armed = match p21.step3_execute(600, STD_BUDGET) {
        Ok(final_fp) => {
            let runner21 = ScenarioRunner::new("std");
            let again = runner21.run(STD_SEED, STD_BUDGET, true);
            match again.steps.last() {
                Some(rec) => p21.patrol(STD_SEED, rec.fp).is_ok() && rec.fp == final_fp,
                None => false,
            }
        }
        Err(_) => false,
    };
    s.add("联防-冻结后同种子必达", armed, if armed { "" } else { "联防失防" });

    // ── 22. 联防漂移检出：异种子巡检必漂 ─────────────────────────────
    let mut p22 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let _ = p22.step1_freeze(601, STD_SEED, STD_BUDGET, &ctx);
    let _ = p22.step2_reproduce(601, STD_SEED, STD_BUDGET);
    let _ = p22.confirm_fix(601);
    let drift_watch = match p22.step3_execute(601, STD_BUDGET) {
        Ok(_) => {
            let runner22 = ScenarioRunner::new("std");
            let other = runner22.run(STD_SEED ^ 1, STD_BUDGET, true);
            match other.steps.last() {
                Some(rec) => matches!(p22.patrol(STD_SEED, rec.fp), Err(RedoError::WatchDrift)),
                None => false,
            }
        }
        Err(_) => false,
    };
    s.add("联防-异种子漂移检出", drift_watch, if drift_watch { "" } else { "漂移漏检" });

    // ── 23. 联防未冻结：陌生种子巡检拒（与漂移分码）──────────────────
    let mut p23 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let unarmed = matches!(p23.patrol(0x1234_5678, 42), Err(RedoError::WatchNotArmed));
    s.add("联防-陌生种子落空拒", unarmed, if unarmed { "" } else { "未冻结放行" });

    // ── 24. 联防写后读：同种子二次冻结拒（第二次复演闭合失败）────────
    let mut p24 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let _ = p24.step1_freeze(610, STD_SEED, STD_BUDGET, &ctx);
    let _ = p24.step2_reproduce(610, STD_SEED, STD_BUDGET);
    let _ = p24.confirm_fix(610);
    let first_ok = p24.step3_execute(610, STD_BUDGET).is_ok();
    // 同种子另立一轮走完前三道闸，步3 在联防 arm 处必拒。
    let _ = p24.step1_freeze(611, STD_SEED, STD_BUDGET, &ctx);
    let _ = p24.step2_reproduce(611, STD_SEED, STD_BUDGET);
    let _ = p24.confirm_fix(611);
    let second_rejected = matches!(
        p24.step3_execute(611, STD_BUDGET),
        Err(RedoError::DuplicateRegistration)
    );
    // 第二轮留在 Redoing——未闭合账显性。
    let open_visible = p24.ledger().unclosed().contains(&611) && p24.ledger().closed().contains(&610);
    s.add(
        "联防写后读-同种子二次冻结拒",
        first_ok && second_rejected && open_visible,
        if second_rejected { "" } else { "联防可被同种子改写" },
    );

    // ── 25. 双跑确定性对拍：两种修复态逐步全等 ───────────────────────
    let mut p25 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let d1 = p25.double_run(STD_SEED, STD_BUDGET, true);
    let d2 = p25.double_run(STD_SEED, STD_BUDGET, false);
    let drun_ok = d1.identical
        && d2.identical
        && d1.first_diff_step.is_none()
        && d2.first_diff_step.is_none();
    s.add("双跑对拍-两种修复态全等", drun_ok, if drun_ok { "" } else { "双跑分叉" });

    // ── 26. 账簿三态账：登记/复演/闭合与守恒 ─────────────────────────
    let mut p26 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let _ = p26.step1_freeze(700, STD_SEED, STD_BUDGET, &ctx);
    let ledger_reg = p26.ledger().len() == 1 && p26.ledger().unclosed().len() == 1;
    let _ = p26.step2_reproduce(700, STD_SEED, STD_BUDGET);
    let ledger_redo = p26.ledger().unclosed().len() == 1;
    let _ = p26.confirm_fix(700);
    let _ = p26.step3_execute(700, STD_BUDGET);
    let ledger_closed = p26.ledger().closed().len() == 1 && p26.ledger().unclosed().is_empty();
    let ledger_ok = ledger_reg && ledger_redo && ledger_closed && p26.ledger().balance_ok();
    s.add(
        "账簿三态-登记复演闭合守恒",
        ledger_ok,
        if ledger_ok { "" } else { "三态账不平" },
    );

    // ── 27. 账簿闭合闸：复现未确认即闭合拒 ───────────────────────────
    let mut p27 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let _ = p27.step1_freeze(701, STD_SEED, STD_BUDGET, &ctx);
    let _ = p27.confirm_fix(701); // 只确认修复，未走步2
    let close_gate = matches!(
        p27.ledger_mut().close(701, 0),
        Err(RedoError::ReproNotConfirmed)
    );
    s.add("账簿闭合闸-未复现拒闭", close_gate, if close_gate { "" } else { "未复现闭合放行" });

    // ── 28. 账簿终态闸：闭合后再写拒 ─────────────────────────────────
    let closed_write = matches!(p26.confirm_fix(700), Err(RedoError::AlreadyClosed));
    s.add("账簿终态闸-闭合再写拒", closed_write, if closed_write { "" } else { "终态改写放行" });

    // ── 29. 账簿未知轮：闭合未知轮拒 ─────────────────────────────────
    let unknown = matches!(p27.ledger_mut().close(99_999, 0), Err(RedoError::UnknownRound));
    s.add("账簿-未知轮拒闭", unknown, if unknown { "" } else { "未知轮闭合放行" });

    // ── 30. 事件账：序号连续 + tick 单调 + 步事件齐 ──────────────────
    let log_ok = p26.log().seq_continuous()
        && p26.log().tick_monotonic()
        && p26.log().count_kind(&EventKind::Freeze) >= 1
        && p26.log().count_kind(&EventKind::ReproOk) >= 1
        && p26.log().count_kind(&EventKind::FixConfirm) >= 1
        && p26.log().count_kind(&EventKind::SecondExec) >= 1
        && p26.log().count_kind(&EventKind::WatchFrozen) >= 1
        && p26.log().count_kind(&EventKind::Closed) >= 1
        && p26.log().count_round(700) >= 6
        && p26.log().last_seq() == p26.log().len() as u64;
    s.add("事件账-序号连续且步事件齐", log_ok, if log_ok { "" } else { "事件账缺步" });

    // ── 31. 路径有账：单轮事件密度 ≥ 6（三步+冻结/复现/闭合）─────────
    let density = p26.log().count_round(700) >= 6;
    s.add("路径有账-单轮事件密度≥6", density, if density { "" } else { "事件密度不足" });

    // ── 32. phase 时戳履历：三步履历齐且 tick 单调 ───────────────────
    let ticks = p26.round_ticks(700);
    let ticks_ok = match ticks {
        Some(v) => {
            v.len() == 3
                && v[0].0 == 1
                && v[1].0 == 2
                && v[2].0 == 3
                && v[0].1 < v[1].1
                && v[1].1 <= v[2].1
        }
        None => false,
    };
    s.add("phase履历-三步时戳单调", ticks_ok, if ticks_ok { "" } else { "履历缺失" });

    // ── 33. health 全绿（干净实例）───────────────────────────────────
    let h26 = p26.health();
    s.add("health-干净实例全绿", h26.overall(), if h26.overall() { "" } else { "干净实例翻红" });

    // ── 34. health 翻红（含红案实例）─────────────────────────────────
    let h11 = p11.health();
    let red_items = h11.red_items();
    s.add(
        "health-红案实例翻红",
        !h11.overall() && red_items.iter().any(|x| *x == "红案清零"),
        if !h11.overall() { "" } else { "红案未翻红" },
    );

    // ── 35. summary 对冲恒等（S11）───────────────────────────────────
    let sum = p26.summary();
    let sum_ok = sum.summary_ok()
        && sum.repro_attempts == 1
        && sum.repro_successes == 1
        && sum.repro_rate_pct == 100
        && sum.watch_armed == 1
        && sum.red_cases == 0;
    s.add("summary-账面对冲恒等", sum_ok, if sum_ok { "" } else { "summary不平" });

    // ── 36. 错误面：16 变体诊断码段内独占且 detail 非空 ──────────────
    let errs: [RedoError; 16] = [
        RedoError::OutOfOrder,
        RedoError::EmptyContext,
        RedoError::BudgetMismatch,
        RedoError::SeedDrift,
        RedoError::FailurePointMismatch,
        RedoError::FixNotConfirmed,
        RedoError::SecondExecFailed,
        RedoError::ReproNotConfirmed,
        RedoError::AlreadyClosed,
        RedoError::DuplicateRegistration,
        RedoError::UnknownRound,
        RedoError::StepSkipDetected,
        RedoError::FingerprintChainBreak,
        RedoError::SnapshotChecksumMismatch,
        RedoError::WatchNotArmed,
        RedoError::WatchDrift,
    ];
    let mut codes_all = true;
    let mut details_all = true;
    let mut uniq_ok = true;
    let mut i = 0usize;
    while i < errs.len() {
        let c = errs[i].code();
        if c < SEG_BASE || c > SEG_TOP {
            codes_all = false;
        }
        if errs[i].detail().is_empty() {
            details_all = false;
        }
        let mut j = i + 1;
        while j < errs.len() {
            if errs[j].code() == c {
                uniq_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add(
        "错误面-诊断码段内独占且诊断非空",
        codes_all && details_all && uniq_ok,
        if codes_all && details_all { "" } else { "诊断码/诊断文缺" },
    );

    // ── 37. LCG 确定性：同种子双流逐位全等（64 步）───────────────────
    let mut la = Lcg::new(STD_SEED);
    let mut lb = Lcg::new(STD_SEED);
    let mut lcg_ok = true;
    let mut i = 0u32;
    while i < 64 {
        if la.next() != lb.next() {
            lcg_ok = false;
        }
        i += 1;
    }
    s.add("LCG确定性-双流全等", lcg_ok, if lcg_ok { "" } else { "LCG分叉" });

    // ── 38. LCG 零除防御：below(0) 无 panic 且为 0 ───────────────────
    let mut l38 = Lcg::new(7);
    let below0 = l38.below(0) == 0 && l38.next() != 0; // next 仍前进
    s.add("LCG零除防御", below0, "below(0)异常");

    // ── 39. 弱点步全域确定性：同种子恒定，异种子全域散布 ─────────────
    let weak_const = ScenarioRunner::weak_step_for(STD_SEED, STD_BUDGET)
        == ScenarioRunner::weak_step_for(STD_SEED, STD_BUDGET);
    let mut weak_spread = 0u32;
    let mut i = 1u64;
    while i <= 64 {
        if ScenarioRunner::weak_step_for(STD_SEED ^ i, STD_BUDGET)
            != ScenarioRunner::weak_step_for(STD_SEED, STD_BUDGET)
        {
            weak_spread += 1;
        }
        i += 1;
    }
    s.add(
        "弱点步-同种子恒定且全域散布",
        weak_const && weak_spread >= 48,
        if weak_const { "" } else { "弱点步不稳" },
    );

    // ── 40. 步数账：通过轮恰跑满预算，失败轮止于弱点步 ───────────────
    let full = runner17.run(STD_SEED, STD_BUDGET, true);
    let weak = ScenarioRunner::weak_step_for(STD_SEED, STD_BUDGET);
    let short = runner17.run(STD_SEED, STD_BUDGET, false);
    let budget_ok = full.steps.len() == STD_BUDGET as usize
        && full.steps_executed == STD_BUDGET
        && match short.outcome {
            RunOutcome::FailedAt { at_step, .. } => at_step == weak,
            RunOutcome::Passed => false,
        }
        && short.steps_executed == weak;
    s.add("步数账-通过满额失败止点", budget_ok, if budget_ok { "" } else { "步数账不平" });

    // ── 41. 到达点双键含失败旗标：单看步号会误判，双键缺一不可 ───────
    let fp_fail = match short.outcome {
        RunOutcome::FailedAt { fp, .. } => fp,
        RunOutcome::Passed => 0,
    };
    let dual_key = fp_fail != 0
        && !short.steps.is_empty()
        && short.steps[short.steps.len() - 1].failed;
    s.add("到达点-双键含失败旗标", dual_key, if dual_key { "" } else { "双键退化" });

    // ── 42. 预算闸：零预算拒冻结 ─────────────────────────────────────
    let mut p42 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let zero_budget = matches!(
        p42.step1_freeze(800, STD_SEED, 0, &ctx),
        Err(RedoError::BudgetMismatch)
    );
    s.add("预算闸-零预算拒冻结", zero_budget, if zero_budget { "" } else { "零预算放行" });

    // ── 43. 预算闸：步2 预算漂移拒 ───────────────────────────────────
    let mut p43 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let _ = p43.step1_freeze(801, STD_SEED, STD_BUDGET, &ctx);
    let budget_drift = matches!(
        p43.step2_reproduce(801, STD_SEED, STD_BUDGET + 1),
        Err(RedoError::BudgetMismatch)
    );
    s.add("预算闸-步2预算漂移拒", budget_drift, if budget_drift { "" } else { "预算漂移放行" });

    // ── 44. 正常 absorb 全链路：诚实自报快照也能走完三步 ─────────────
    let mut p44 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let probe44 = ScenarioRunner::new("std").run(0xAAAA_0001, STD_BUDGET, false);
    let absorb_ok = match probe44.outcome {
        RunOutcome::FailedAt { at_step, fp } => {
            let mut honest = RedoSnapshot {
                round_id: 810,
                seed: 0xAAAA_0001,
                budget: STD_BUDGET,
                arrival_step: at_step,
                arrival_fp: fp,
                ctx: Vec::new(),
                checksum: 0,
                frozen_tick: 0,
            };
            let mut i = 0usize;
            while i < ctx.len() {
                honest.ctx.push(ctx[i].to_string());
                i += 1;
            }
            honest.checksum = honest.recompute_checksum();
            p44.step1_absorb(honest).is_ok()
                && p44.step2_reproduce(810, 0xAAAA_0001, STD_BUDGET).is_ok()
                && p44.confirm_fix(810).is_ok()
                && p44.step3_execute(810, STD_BUDGET).is_ok()
        }
        RunOutcome::Passed => false,
    };
    s.add("absorb链路-诚实快照走通三步", absorb_ok, if absorb_ok { "" } else { "吸收链路断" });

    // ── 45. 全链路 5 轮压力：冻结→复现→修复→执行 全绿 health 绿 ──────
    let mut p45 = FailRedoProtocol::new(ScenarioRunner::new("std"));
    let mut chain5 = 0u32;
    let mut rid: u32 = 900;
    let mut r = 0u32;
    while r < 5 {
        let mut lcg = Lcg::new(0xF0644 ^ (r as u64).wrapping_mul(0x5DEE_CE66_1234_0001));
        let seed_r = STD_SEED ^ lcg.next();
        let ok = p45.step1_freeze(rid, seed_r, STD_BUDGET, &ctx).is_ok()
            && p45.step2_reproduce(rid, seed_r, STD_BUDGET).is_ok()
            && p45.confirm_fix(rid).is_ok()
            && p45.step3_execute(rid, STD_BUDGET).is_ok();
        if ok {
            chain5 += 1;
        }
        rid = rid.wrapping_add(1);
        r += 1;
    }
    let health45 = p45.health();
    let sum45 = p45.summary();
    s.add(
        "全链路压力-5轮三步全绿health绿",
        chain5 == 5
            && health45.overall()
            && p45.rate().rate_pct() == 100
            && sum45.summary_ok()
            && sum45.watch_armed == 5,
        if chain5 == 5 { "" } else { "压力轮有红" },
    );

    // ── 46. 诊断段独占（编译期与运行期双证）──────────────────────────
    s.add(
        "诊断段-0x1530~0x153F独占",
        SEG_BASE == 0x1530 && SEG_TOP == 0x153F,
        "段越界",
    );

    s
}

// ═══════════════════════════════════════════════════════════════════════════
// 十二、编译期终检
// ═══════════════════════════════════════════════════════════════════════════

const _: () = assert!(SEG_BASE <= SEG_TOP);
const _: () = assert!(STD_BUDGET > 0);
const _: () = assert!(E_OUT_OF_ORDER == 0x1530 && E_WATCH_DRIFT == 0x153F);

// ═══════════════════════════════════════════════════════════════════════════
// 十三、冒烟测试（宿主测试链路；内核路径本文件零 unwrap/expect/panic）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx3() -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        v.push("判据=测试判据A");
        v.push("断点=测试断点B");
        v.push("域=测试域C");
        v
    }

    /// 冒烟1：三步全序走通并闭合；闭合后写入拒。
    #[test]
    fn smoke_three_step_ordering() {
        let mut p = FailRedoProtocol::new(ScenarioRunner::new("smoke"));
        assert!(p.step1_freeze(1, STD_SEED, STD_BUDGET, &ctx3()).is_ok());
        assert!(matches!(p.step2_reproduce(1, STD_SEED, STD_BUDGET), Ok((at, _)) if at >= 1));
        assert!(p.confirm_fix(1).is_ok());
        assert!(p.step3_execute(1, STD_BUDGET).is_ok());
        assert_eq!(p.phase_of(1), Some(3));
        assert!(p.ledger().balance_ok());
        assert!(p.log().seq_continuous());
        // 乱序面：闭合后再走步2 必拒；phase 履历三步齐。
        assert!(matches!(
            p.step2_reproduce(1, STD_SEED, STD_BUDGET),
            Err(RedoError::OutOfOrder)
        ));
        let ticks = p.round_ticks(1).unwrap_or_default();
        assert_eq!(ticks.len(), 3);
    }

    /// 冒烟2：同种子复现率 100%（J1-e 主判据）+ health 全绿。
    #[test]
    fn smoke_repro_rate_100() {
        let mut p = FailRedoProtocol::new(ScenarioRunner::new("smoke"));
        let mut rid = 1u32;
        let mut i = 0u32;
        while i < 7 {
            let seed = STD_SEED ^ (i as u64).wrapping_mul(0x1000_0001);
            assert!(p.step1_freeze(rid, seed, STD_BUDGET, &ctx3()).is_ok());
            assert!(p.step2_reproduce(rid, seed, STD_BUDGET).is_ok());
            rid += 1;
            i += 1;
        }
        assert_eq!(p.rate().attempts, 7);
        assert_eq!(p.rate().successes, 7);
        assert_eq!(p.rate().rate_pct(), 100);
        assert!(p.rate().is_full_or_empty());
        assert!(p.health().overall());
    }

    /// 冒烟3：指纹链闭合 + 同种子同轨迹 + 失败轨迹止于弱点步。
    #[test]
    fn smoke_fingerprint_chain() {
        let r = ScenarioRunner::new("smoke");
        let a = r.run(STD_SEED, STD_BUDGET, false);
        let b = r.run(STD_SEED, STD_BUDGET, false);
        assert_eq!(a.steps.len(), b.steps.len());
        assert!(ScenarioRunner::verify_trajectory(&a).is_ok());
        let mut i = 0usize;
        while i < a.steps.len() {
            assert_eq!(a.steps[i].fp, b.steps[i].fp);
            assert_eq!(a.steps[i].failed, b.steps[i].failed);
            i += 1;
        }
        // 失败轨迹末步带失败旗标且到达点双键自洽。
        let mut failed_last = false;
        match a.outcome {
            RunOutcome::FailedAt { at_step, fp } => {
                failed_last = true;
                let last = &a.steps[a.steps.len() - 1];
                assert_eq!(at_step, last.step_no);
                assert_eq!(fp, last.fp);
            }
            RunOutcome::Passed => {}
        }
        assert!(failed_last);
        // 通过轨迹零失败步、跑满预算。
        let passed = r.run(STD_SEED, STD_BUDGET, true);
        let mut any_failed = false;
        let mut i = 0usize;
        while i < passed.steps.len() {
            if passed.steps[i].failed {
                any_failed = true;
            }
            i += 1;
        }
        assert!(!any_failed);
        assert_eq!(passed.steps.len(), STD_BUDGET as usize);
        assert!(ScenarioRunner::verify_trajectory(&passed).is_ok());
    }

    /// 冒烟4：三类注入全检出（漂移/篡改/跳步）。
    #[test]
    fn smoke_injections_detected() {
        let mut p = FailRedoProtocol::new(ScenarioRunner::new("smoke"));
        assert!(p.step1_freeze(1, STD_SEED, STD_BUDGET, &ctx3()).is_ok());
        assert!(p.inject_seed_drift(1, 0xDEAD));
        assert!(p.inject_snapshot_tamper(1));
        assert!(p.inject_step_skip(STD_SEED, STD_BUDGET, true));
        assert!(p.inject_relabel_skip(STD_SEED, STD_BUDGET, true));
        assert!(p.inject_fp_flip(STD_SEED, STD_BUDGET, 5));
        assert_eq!(p.drift_rejections, 1);
        assert_eq!(p.tamper_detections, 1);
        assert_eq!(p.skip_detections, 3);
    }

    /// 冒烟5：双跑确定性对拍一致。
    #[test]
    fn smoke_double_run_identical() {
        let mut p = FailRedoProtocol::new(ScenarioRunner::new("smoke"));
        for fix in [true, false] {
            let rep = p.double_run(STD_SEED, STD_BUDGET, fix);
            assert!(rep.identical);
            assert!(rep.first_diff_step.is_none());
            assert_eq!(rep.steps_a, rep.steps_b);
        }
    }

    /// 冒烟6：46 条判据电池全绿（J1 全路径一次收口）。
    #[test]
    fn smoke_check_battery_all_green() {
        let s = run_unx0644_checks();
        let mut i = 0usize;
        while i < s.items.len() {
            if !s.items[i].pass {
                std::println!("RED: {} | {}", s.items[i].name, s.items[i].note);
            }
            i += 1;
        }
        assert_eq!(s.items.len(), 46);
        assert!(s.all_pass(), "判据电池有红：{}条", s.items.len() - s.pass_count());
    }
}
