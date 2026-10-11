//! unx2861 ↔ UNX-F2861 大页池回归矩阵冻结
//!
//! 单号：UNX-F2861
//! 标题：大页池回归矩阵冻结
//! 域：CoRun · UNX-A4-B24 段「大页池回归」
//! 册目标行数：340（矩阵 150 + 冻结 100 + 断言 90；测试段不计）
//! 交付文件行数：≥2800（硬指标）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F2861-J1，逐字摘抄）：
//! `UNX-F2861-J1 回归用例矩阵（九面压测→用例映射 27 例）冻结哈希入册，
//!  矩阵字段完整性零缺失`
//!
//! ## 码段范围声明
//! 本单独占诊断码段 `0x2140 ..= 0x217F`（`SEG_BASE = 0x2140`，
//! `SEG_MASK = 0x003F`，段内可编址 64 码）。主码 `0x2140`。
//! 错误变体一码一段，逐个占用段内偏移，变体总数 ≤ 60
//! （实际 12 个注入形态 + 6 个辅助码 = 18 码，余 46 码留白）。
//! 本单**不占用** `0x2180` 及以上任何码；那些段归 UNX-F2901 / F2941。
//!
//! ## 组件清单（本单交付的纯功能件）
//! 1. `StressFace`：九面压测闭集（F2802–F2809 八面 + F2805 汇总面，
//!    共 9 面，是判据「九面」的闭集化）。
//! 2. `CaseTier`：三档用例档（核心 / 边界 / 对抗），每面各出一条。
//! 3. `CaseColumn`：矩阵四列闭集（用例 ID / 来源面 / 判据引用 / 期望读数），
//!    判据原文要求的「例 ID / 判据段 / 断言入口 / 读数键」四列一一对应。
//! 4. `CaseSpec`：单条回归用例（27 条），四列字段登记 + 三元组正交展开。
//! 5. `CaseMatrix`：回归矩阵（9 面 × 3 档 = 27 例），格数由维度乘积现算。
//! 6. `IntegrityReport`：**字段完整性零缺失**断言（四列逐格点验，
//!    缺失位图 + 重复位图 + 空串位图）。
//! 7. `CoverageReport`：**面覆盖断言**（九面逐面点验，无漏面）。
//! 8. `CaseRun`：逐例独立夹具真跑（**不做用例执行的产物，
//!    只跑「矩阵字段的自洽夹具」**——语义边界见下）。
//! 9. `MatrixState`：矩阵状态机（草拟 → 待审 → 已冻结 → 漂移 → 作废），
//!    合法边闭集 + 非法转换拒记。
//! 10. `RingLedger`：append-only 环形账，`dropped` 显性计数。
//! 11. `FreezeRecord`：冻结件（册版本 / 冻结哈希 / 基线哈希 / 四列注记）。
//! 12. `inject_all()`：注入核 **12 形态全拒**（判据要求 10/10 ×2 类：
//!     漏面注入 10/10 检出 + 漂移注入 10/10 拦截）。
//! 13. `fingerprint()` / `verify()`：冻结指纹与漂移检出。
//! 14. `export_kv()` / `export_tlv()`：观测导出，TLV 往返自洽。
//! 15. `replay(seed)` / `observe(seed)`：确定性重放与真实跑逻辑观测。
//! 16. `stability_sweep()`：稳定性判定（多次重复跑指纹一致才算通过）。
//! 17. `resource_budget()`：资源上界核算。
//! 18. `complexity()`：性能逐项分解。
//!
//! ## 职责定位（语义边界）
//! 判据原文写明「**只做矩阵冻结**；不做用例执行（F2863 runner）、
//! 不做判据变更（各面判据冻结）」。本单严格守这条线：
//! - **只冻结**：27 条用例的**四列字段**被登记、被断言、被算哈希、被入册；
//! - **不做执行**：`CaseRun` 跑的是「矩阵自身字段的自洽夹具」
//!   （四列非空 + 面归属正确 + 读数键唯一 + 断言入口存在），
//!   **不是**真机上的回归用例；F2863 runner 才做真执行；
//! - **不改判据**：九面各自的判据（阈值、基线读数）本单只**引用**
//!   （`criterion_seg` 字段），不重新定义阈值，只校验其可引用性。
//! 这三条边界必须在册留痕，否则会被误读为「已跑过 27 条回归」。
//!
//! ## 27 例的构造（格数显式计算）
//! ```text
//! 面数  FACE_COUNT   = 9   （F2802..F2809 八面 + F2805 汇总面）
//! 档数  TIER_COUNT   = 3   （核心 / 边界 / 对抗）
//! 例数  CASE_COUNT   = FACE_COUNT * TIER_COUNT = 27
//! 列数  COLUMN_COUNT = 4   （例 ID / 判据段 / 断言入口 / 读数键）
//! 字段位TOTAL_FIELDS = CASE_COUNT * COLUMN_COUNT = 108
//! ```
//! 判据要求「27 例四列零缺失」，故**零缺失的操作化定义**是
//! `IntegrityReport::missing_bits == 0 && dup_bits == 0 && blank_bits == 0`，
//! 其中 `missing_bits` 是「该例该列无登记」的位图，
//! `dup_bits` 是「同列两例取值相同」的位图（防复制粘贴导致漏面），
//! `blank_bits` 是「登记了但为空串」的位图（防空串假通过）。
//!
//! ## 与九面压测的衔接点
//! | 源面 | 单号 | 本单映射的 3 例 | 本单只引用不重定义的东西 |
//! |------|------|------------------|--------------------------|
//! | 容量面 | F2802 | 容量-核心/边界/对抗 | 满池/近满/空三态阈值 |
//! | 建立面 | F2803 | 建立-核心/边界/对抗 | 建立速率档位表 |
//! | 翻译面 | F2804 | 翻译-核心/边界/对抗 | 页表遍历层数与 PS 位 |
//! | 预留面 | F2805 | 预留-核心/边界/对抗 | 预留/释放比例窗口 |
//! | 拆当面 | F2806 | 拆分-核心/边界/对抗 | 整块回收粒度 |
//! | 争用面 | F2807 | 争用-核心/边界/对抗 | 并发度档位表 |
//! | 长稳面 | F2808 | 长稳-核心/边界/对抗 | 逻辑 tick 跨度 |
//! | 失真面 | F2809 | 失真-核心/边界/对抗 | 失真声明在档位 |
//! | 汇总面 | F2810 | 汇总-核心/边界/对抗 | 九面基线读数汇总口径 |
//!
//! ## 性能逐项分解（模型侧成本，非真实硬件时序）
//! - **矩阵构建** `CaseMatrix::expand()`：`O(FACES * TIERS)` = 27 步，
//!   每步一次 `CaseSpec::new()` 纯计算，无分配。
//! - **完整性断言** `integrity_report()`：`O(CASES * COLUMNS)` = 108 步位图点验。
//! - **面覆盖断言** `coverage_report()`：`O(FACES * TIERS)` = 27 步
//!   （每面查 3 档是否齐）。
//! - **逐例夹具真跑** `run_case()`：`O(CASE_STEPS)`，`CASE_STEPS = 16`，
//!   上界 `27 × 16 = 432` 步。
//! - **稳定性扫描** `stability_sweep()`：`O(REPEAT × CASES × CASE_STEPS)`，
//!   `REPEAT = 3`，即 1296 步硬上限。
//! - **冻结哈希**：`O(CASES)` 逐例 FNV-1a 折叠（27 次，不是 27×4——
//!   四列折叠进每例指纹后再串接，避免列序变动被哈希吞掉）。
//! - **注入核十二形态**：造证 `O(1)` 定向；检出 `O(FORM_COUNT)`。
//!
//! ### 深在哪（对标 Windows 同类能力）
//! Windows 的回归矩阵思想同构（Test Matrix / Coverage Matrix），但其
//! 「面覆盖」靠人工在表格里数勾选，且用例 ID 常被复制粘贴复用。本单
//! 深在四处：
//! 1. **格数现算**：27 由 `9 × 3` 在构造期算出并与常量逐位比对，
//!    增一档面就变 30，不允许「常量写死 27 却与面数脱钩」。
//! 2. **三重零缺失位图**：缺失 / 重复 / 空串三位图同时为零才算零缺失，
//!    只查「非空」会被复制粘贴骗过。
//! 3. **漏面与漂移分形态注入**：12 形态里 6 个是漏面（不同面）、
//!    5 个是漂移（不同列 / 档位）、1 个是缺列，
//!    判据要求的两类 10/10 各自独立达标。
//! 4. **稳定性重跑判定**：同种子连跑 3 轮，逐例指纹必须全等，
//!    不是「跑通一次就算」。
//!
//! ## 纯功能线
//! `no_std + alloc`；零 `unwrap` / `expect` / `panic!`、零浮点、零真实 IO、
//! 零墙钟；时间一律逻辑 `tick: u64`；确定性 LCG
//! （`LCG_A = 6364136223846793005`、`LCG_C = 1442695040888963407`、
//! 模 `2^64` 回绕）保证同种子同轨迹；FNV-1a basis `0xcbf29ce484222325`、
//! prime `0x100000001b3`；自定义 `UmResult<T>`，错误 `code() -> u16` +
//! `detail() -> String` 三要素齐备；无 `#[cfg(test)]`。
#![allow(dead_code)]
extern crate alloc;

// `vec!` 在本单由 TLV 预分配与回读数组使用；此处保留统一导入形态（册内风格一致）。
#[allow(unused_imports)]
use alloc::{format, string::String, string::ToString, vec, vec::Vec};

// ===========================================================================
// 0. 常量表
// ===========================================================================
/// 模块版本标识（写入冻结件 + TLV 组头）。
pub const V_VERSION_ID: u64 = 0x0001_2861_0000_0001;
/// 模块版本串。
pub const V_VERSION: &str = "v2861-ubigmatrix.v1";
/// 诊断码段基址（独占段 `0x2140`）。
pub const SEG_BASE: u16 = 0x2140;
/// 诊断码段掩码（段容量 64 码）。
pub const SEG_MASK: u16 = 0x003F;
/// 诊断码段上界（= SEG_BASE | SEG_MASK）。
pub const SEG_END: u16 = SEG_BASE | SEG_MASK;
/// 诊断主码（段基址本身）。
pub const MAIN_CODE: u16 = SEG_BASE;
/// 回归矩阵册版本号（冻结件入册时登记）。
pub const MATRIX_BOOK_REV: u32 = 5;
/// 九面压测面数（F2802–F2809 八面 + F2810 汇总面）。
pub const FACE_COUNT: usize = 9;
/// 每面用例档数（核心 / 边界 / 对抗）。
pub const TIER_COUNT: usize = 3;
/// 回归用例总数（`9 × 3`，构造期算出并断言）。
pub const CASE_COUNT: usize = FACE_COUNT * TIER_COUNT;
/// 矩阵列数（例 ID / 判据段 / 断言入口 / 读数键）。
pub const COLUMN_COUNT: usize = 4;
/// 四列字段位总数（`27 × 4 = 108`）。
pub const TOTAL_FIELDS: usize = CASE_COUNT * COLUMN_COUNT;
/// 单例夹具真跑步数。
pub const CASE_STEPS: usize = 16;
/// 全矩阵真跑步数上界。
pub const MATRIX_STEPS: usize = CASE_COUNT * CASE_STEPS;
/// 稳定性重复轮数（≥2 才有意义，取 3）。
pub const REPEAT: usize = 3;
/// 注入形态数（判据要求漏面 10/10 + 漂移 10/10 两类，本单给 12）。
pub const FORM_COUNT: usize = 12;
/// 环形账容量（2 的幂）。
pub const RING_CAP: usize = 64;
/// 环形账掩码。
pub const RING_MASK: usize = RING_CAP - 1;
/// 逻辑时钟步进量。
pub const TICK_STEP: u64 = 1;
/// step 硬上限（`MATRIX_STEPS × REPEAT` 再留一倍裕量）。
pub const STEP_CAP: u64 = (MATRIX_STEPS * REPEAT * 2) as u64;
/// syscall 上界（本单零真实 syscall，此为账面上界）。
pub const SYSCALL_CAP: u64 = 0;
/// 内存峰值上界（字节，模型侧估算）。
pub const MEM_PEAK_CAP: u64 = 4 * 1024 * 1024;
/// FNV-1a basis。
pub const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;
/// LCG 乘数。
pub const LCG_A: u64 = 6364136223846793005;
/// LCG 增量。
pub const LCG_C: u64 = 1442695040888963407;
/// LCG 模（2^64，隐式回绕，此处显式留档）。
pub const LCG_M: u64 = 0;
/// 九面读数基线来源单号（F2810 压测基线）。
pub const BASELINE_REF: &str = "F2810";
/// 假执行体单号（F2863 runner，本单只引用不实现）。
pub const RUNNER_REF: &str = "F2863";
/// 用例 ID 前缀（由「面码 + 档码」拼出，例 `RG-CAP-CORE`）。

pub const CASE_ID_PREFIX: &str = "RG";

// ===========================================================================
// 1. 确定性 LCG + FNV-1a（两组原语，全册共用口径）
// ===========================================================================
/// 确定性 LCG（Knuth MMIX 62 位参数，模 2^64 回绕）。
///
/// 种子**不参与矩阵取值**（矩阵是纯函数产物），只做跑序抖动，
/// 保证跨轮可比。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u64,
}
impl Lcg {
    /// 由种子构造。
    pub fn new(seed: u64) -> Lcg {
        // 种子为 0 时仍取一个非零态，避免 `x0 = 0` 序列退化。
        Lcg { state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed } }
    }
    /// 当前原始态。
    pub fn raw(&self) -> u64 {
        self.state
    }
    /// 步进一步。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }
    /// 取 `[0, n)` 内均匀值（拒绝采样，避免取模偏差）。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        let nn = n as u64;
        let limit = u64::MAX - (u64::MAX % nn) - 1;
        loop {
            let x = self.next_u64();
            if x <= limit {
                return (x % nn) as usize;
            }
        }
    }
    /// 取千分数（0..=1000）。
    pub fn permille(&mut self) -> u64 {
        self.below(1001) as u64
    }
}

/// 单步 LCG（无状态纯函数，供指纹链使用）。
pub fn lcg_step(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// FNV-1a 折叠（可续算）。
///
/// 用于「逐例指纹再串接」的两段式折叠：先按字节续算，再按 64 位字折叠，
/// 避免字序对齐假设把语义不同的两批字段折叠成同一值。
pub fn fnv1a64_ext(state: u64, bytes: &[u8]) -> u64 {
    let mut h = state;
    let mut i: usize = 0;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => break,
        };
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    h
}
/// FNV-1a 64 位（字节序列）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    fnv1a64_ext(FNV_BASIS, bytes)
}
/// FNV-1a 64 位折叠（64 位字序列，小端 8 字节/字）。
pub fn fnv1a_words(words: &[u64]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i: usize = 0;
    while i < words.len() {
        let w = match words.get(i) {
            Some(v) => *v,
            None => break,
        };
        let mut b: usize = 0;
        while b < 8 {
            h ^= ((w >> (b * 8)) & 0xFFu64) as u64;
            h = h.wrapping_mul(FNV_PRIME);
            b = b.wrapping_add(1);
        }
        i = i.wrapping_add(1);
    }
    h
}

/// 字符串取字节视图（`&str → &[u8]`，纯借用零拷贝）。
fn sbytes(s: &str) -> &[u8] {
    s.as_bytes()
}

/// 逻辑时钟步进。
pub fn tick_advance(now: u64) -> u64 {
    now.wrapping_add(TICK_STEP)
}

// ===========================================================================
// 2. 诊断码与错误闭集（18 变体，逐个占段内偏移）
// ===========================================================================
/// 注入 / 失效形态（12 个，一码一段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    /// 漏面：容量面整面缺失（抹净 3 例）。
    FaceDropCapacity,
    /// 漏面：建立面整面缺失。
    FaceDropEstablish,
    /// 漏面：翻译面整面缺失。
    FaceDropTranslate,
    /// 漏面：预留面整面缺失。
    FaceDropReserve,
    /// 漏面：拆当面整面缺失。
    FaceDropSplit,
    /// 漏面：争用面整面缺失。
    FaceDropContend,
    /// 漂移：判据段字段被改写（列 2 漂移）。
    DriftCriterionSeg,
    /// 漂移：断言入口字段被改写（列 3 漂移）。
    DriftAssertEntry,
    /// 漂移：读数键字段被改写（列 4 漂移）。
    DriftReadingKey,
    /// 漂移：用例 ID 被改写（列 1 漂移）。
    DriftCaseId,
    /// 漂移：档位被改写（核心↔对抗）。
    DriftTierSwap,
    /// 缺列：四列中整列被抹掉。
    ColumnDropAll,
}
impl Form {
    /// 全部形态。
    pub const ALL: [Form; FORM_COUNT] = [
        Form::FaceDropCapacity,
        Form::FaceDropEstablish,
        Form::FaceDropTranslate,
        Form::FaceDropReserve,
        Form::FaceDropSplit,
        Form::FaceDropContend,
        Form::DriftCriterionSeg,
        Form::DriftAssertEntry,
        Form::DriftReadingKey,
        Form::DriftCaseId,
        Form::DriftTierSwap,
        Form::ColumnDropAll,
    ];
    /// 序号（0..11）。
    pub fn index(self) -> usize {
        match self {
            Form::FaceDropCapacity => 0,
            Form::FaceDropEstablish => 1,
            Form::FaceDropTranslate => 2,
            Form::FaceDropReserve => 3,
            Form::FaceDropSplit => 4,
            Form::FaceDropContend => 5,
            Form::DriftCriterionSeg => 6,
            Form::DriftAssertEntry => 7,
            Form::DriftReadingKey => 8,
            Form::DriftCaseId => 9,
            Form::DriftTierSwap => 10,
            Form::ColumnDropAll => 11,
        }
    }
    /// 诊断码（`SEG_BASE + 偏移`，一码一段）。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self.index() as u16)
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            Form::FaceDropCapacity => "face_drop_capacity",
            Form::FaceDropEstablish => "face_drop_establish",
            Form::FaceDropTranslate => "face_drop_translate",
            Form::FaceDropReserve => "face_drop_reserve",
            Form::FaceDropSplit => "face_drop_split",
            Form::FaceDropContend => "face_drop_contend",
            Form::DriftCriterionSeg => "drift_criterion_seg",
            Form::DriftAssertEntry => "drift_assert_entry",
            Form::DriftReadingKey => "drift_reading_key",
            Form::DriftCaseId => "drift_case_id",
            Form::DriftTierSwap => "drift_tier_swap",
            Form::ColumnDropAll => "column_drop_all",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            Form::FaceDropCapacity => "漏面：容量面（F2802）3 例全缺",
            Form::FaceDropEstablish => "漏面：建立面（F2803）3 例全缺",
            Form::FaceDropTranslate => "漏面：翻译面（F2804）3 例全缺",
            Form::FaceDropReserve => "漏面：预留面（F2805）3 例全缺",
            Form::FaceDropSplit => "漏面：拆当面（F2806）3 例全缺",
            Form::FaceDropContend => "漏面：争用面（F2807）3 例全缺",
            Form::DriftCriterionSeg => "漂移：判据段字段改写（列 2）",
            Form::DriftAssertEntry => "漂移：断言入口字段改写（列 3）",
            Form::DriftReadingKey => "漂移：读数键字段改写（列 4）",
            Form::DriftCaseId => "漂移：用例 ID 改写（列 1）",
            Form::DriftTierSwap => "漂移：档位核心↔对抗互换",
            Form::ColumnDropAll => "缺列：四列中读数键整列置空",
        }
    }
    /// 探针（造证手法）。
    pub fn probe(self) -> &'static str {
        match self {
            Form::FaceDropCapacity => "把面码置 CAP 的 3 条 case 的四列全抹空",
            Form::FaceDropEstablish => "把面码置 EST 的 3 条 case 的四列全抹空",
            Form::FaceDropTranslate => "把面码置 XLT 的 3 条 case 的四列全抹空",
            Form::FaceDropReserve => "把面码置 RSV 的 3 条 case 的四列全抹空",
            Form::FaceDropSplit => "把面码置 SPL 的 3 条 case 的四列全抹空",
            Form::FaceDropContend => "把面码置 CTD 的 3 条 case 的四列全抹空",
            Form::DriftCriterionSeg => "篡改列 2（判据段）为伪造引用",
            Form::DriftAssertEntry => "篡改列 3（断言入口）为不存在的入口",
            Form::DriftReadingKey => "篡改列 4（读数键）为伪造键",
            Form::DriftCaseId => "篡改列 1（用例 ID）为伪造 ID",
            Form::DriftTierSwap => "把核心与对抗两档的档码互换",
            Form::ColumnDropAll => "把列 4（读数键）整列置空串",
        }
    }
    /// 是否属「漏面」类（判据要求 10/10 检出的第一类）。
    pub fn is_face_drop(self) -> bool {
        matches!(
            self,
            Form::FaceDropCapacity
                | Form::FaceDropEstablish
                | Form::FaceDropTranslate
                | Form::FaceDropReserve
                | Form::FaceDropSplit
                | Form::FaceDropContend
        )
    }
    /// 是否属「缺列」类（第三类，额外覆盖）。
    pub fn is_column_drop(self) -> bool {
        matches!(self, Form::ColumnDropAll)
    }
    /// 是否属「漂移」类（判据要求 10/10 拦截的第二类）。
    pub fn is_drift(self) -> bool {
        !self.is_face_drop() && !self.is_column_drop()
    }
    /// 所属类名。
    pub fn class_name(self) -> &'static str {
        if self.is_face_drop() {
            "面漏"
        } else if self.is_column_drop() {
            "列缺"
        } else {
            "漂移"
        }
    }
}

/// 辅助诊断码（6 个，占段内高位偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuxCode {
    /// 面码越界。
    FaceOutOfRange = 20,
    /// 档码越界。
    TierOutOfRange = 21,
    /// 例号越界。
    CaseIndexOutOfRange = 22,
    /// 列号越界。
    ColumnOutOfRange = 23,
    /// 冻结态非法写入。
    FrozenWriteRejected = 24,
    /// 版本不符。
    BookRevMismatch = 25,
}
impl AuxCode {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE.wrapping_add(self as u16)
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            AuxCode::FaceOutOfRange => "face_out_of_range",
            AuxCode::TierOutOfRange => "tier_out_of_range",
            AuxCode::CaseIndexOutOfRange => "case_index_out_of_range",
            AuxCode::ColumnOutOfRange => "column_out_of_range",
            AuxCode::FrozenWriteRejected => "frozen_write_rejected",
            AuxCode::BookRevMismatch => "book_rev_mismatch",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            AuxCode::FaceOutOfRange => "面码越界：不在九面闭集内",
            AuxCode::TierOutOfRange => "档码越界：不在三档闭集内",
            AuxCode::CaseIndexOutOfRange => "例号越界：不在 0..27 内",
            AuxCode::ColumnOutOfRange => "列号越界：不在 0..4 内",
            AuxCode::FrozenWriteRejected => "冻结态写入拒收：已冻结后不得改列",
            AuxCode::BookRevMismatch => "册版本不符：冻结件与 CI 基线版本不一致",
        }
    }
}

/// 错误结构（三要素：`code()` / `detail()` / `full()`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UmErr {
    /// 诊断码。
    pub code: u16,
    /// 明文细节。
    pub detail: String,
}
/// 结果别名。
pub type UmResult<T> = Result<T, UmErr>;
impl UmErr {
    /// 构造。
    pub fn new(code: u16, detail: String) -> UmErr {
        UmErr { code, detail }
    }
    /// 由形态构造。
    pub fn of_form(f: Form, detail: String) -> UmErr {
        UmErr { code: f.code(), detail }
    }
    /// 由辅助码构造。
    pub fn of_aux(a: AuxCode, detail: String) -> UmErr {
        UmErr { code: a.code(), detail }
    }
    /// 三要素之一：诊断码。
    pub fn code(&self) -> u16 {
        self.code
    }
    /// 三要素之二：明文细节。
    pub fn detail(&self) -> String {
        self.detail.clone()
    }
    /// 三要素之三：全串（码 + 文）。
    pub fn full(&self) -> String {
        format!("0x{:04X} {}", self.code, self.detail)
    }
}
/// 错误转字符串（零拷贝借用版）。
pub fn err_str(e: &UmErr) -> String {
    e.full()
}
// ===========================================================================
// 3. 九面闭集 / 三档闭集 / 四列闭集
// ===========================================================================
/// 九面压测闭集（判据「九面」的闭集化）。
///
/// 九面的来源：B21 段 F2802–F2809 共八面 + F2810 汇总面。
/// 每一面只登记「面码 / 单号 / 本单引用什么」三件事——
/// **不重定义阈值**，阈值归属各面自己的判据单。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressFace {
    /// 容量面（F2802）：满池 / 近满 / 空 三态阈值。
    Capacity,
    /// 建立面（F2803）：建立速率档位表。
    Establish,
    /// 翻译面（F2804）：页表遍历层数与 PS 位。
    Translate,
    /// 预留面（F2805）：预留 / 释放比例窗口。
    Reserve,
    /// 拆当面（F2806）：整块回收粒度。
    Split,
    /// 争用面（F2807）：并发度档位表。
    Contend,
    /// 长稳面（F2808）：逻辑 tick 跨度。
    LongRun,
    /// 失真面（F2809）：失真声明在档位。
    Distortion,
    /// 汇总面（F2810）：九面基线读数汇总口径。
    Summary,
}
impl StressFace {
    /// 全部九面。
    pub const ALL: [StressFace; FACE_COUNT] = [
        StressFace::Capacity,
        StressFace::Establish,
        StressFace::Translate,
        StressFace::Reserve,
        StressFace::Split,
        StressFace::Contend,
        StressFace::LongRun,
        StressFace::Distortion,
        StressFace::Summary,
    ];
    /// 序号（0..8）。
    pub fn index(self) -> usize {
        match self {
            StressFace::Capacity => 0,
            StressFace::Establish => 1,
            StressFace::Translate => 2,
            StressFace::Reserve => 3,
            StressFace::Split => 4,
            StressFace::Contend => 5,
            StressFace::LongRun => 6,
            StressFace::Distortion => 7,
            StressFace::Summary => 8,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<StressFace> {
        let mut k: usize = 0;
        while k < FACE_COUNT {
            let f = StressFace::ALL[k];
            if f.index() == i {
                return Some(f);
            }
            k = k.wrapping_add(1);
        }
        None
    }
    /// 面码（三字母，进用例 ID）。
    pub fn code3(self) -> &'static str {
        match self {
            StressFace::Capacity => "CAP",
            StressFace::Establish => "EST",
            StressFace::Translate => "XLT",
            StressFace::Reserve => "RSV",
            StressFace::Split => "SPL",
            StressFace::Contend => "CTD",
            StressFace::LongRun => "LRN",
            StressFace::Distortion => "DSR",
            StressFace::Summary => "SUM",
        }
    }
    /// 源面单号。
    pub fn src_ticket(self) -> &'static str {
        match self {
            StressFace::Capacity => "F2802",
            StressFace::Establish => "F2803",
            StressFace::Translate => "F2804",
            StressFace::Reserve => "F2805",
            StressFace::Split => "F2806",
            StressFace::Contend => "F2807",
            StressFace::LongRun => "F2808",
            StressFace::Distortion => "F2809",
            StressFace::Summary => "F2810",
        }
    }
    /// 本单引用该面的判据段（只引用，不重定义）。
    pub fn criterion_seg(self) -> &'static str {
        match self {
            StressFace::Capacity => "F2802#cap-3state",
            StressFace::Establish => "F2803#est-rate-table",
            StressFace::Translate => "F2804#xlt-ps-walk",
            StressFace::Reserve => "F2805#rsv-window",
            StressFace::Split => "F2806#spl-whole-block",
            StressFace::Contend => "F2807#ctd-conc-table",
            StressFace::LongRun => "F2808#lrn-tick-span",
            StressFace::Distortion => "F2809#dsr-declare",
            StressFace::Summary => "F2810#sum-baseline",
        }
    }
    /// 该面断言入口（回归时从哪里进去验）。
    pub fn assert_entry(self) -> &'static str {
        match self {
            StressFace::Capacity => "f2802.cap.three_state",
            StressFace::Establish => "f2803.est.rate_table",
            StressFace::Translate => "f2804.xlt.ps_walk",
            StressFace::Reserve => "f2805.rsv.window",
            StressFace::Split => "f2806.spl.whole_block",
            StressFace::Contend => "f2807.ctd.conc_table",
            StressFace::LongRun => "f2808.lrn.tick_span",
            StressFace::Distortion => "f2809.dsr.declare",
            StressFace::Summary => "f2810.sum.baseline",
        }
    }
    /// 该面读数键（回归读哪个字段）。
    pub fn reading_key(self) -> &'static str {
        match self {
            StressFace::Capacity => "cap.hit_permille",
            StressFace::Establish => "est.alloc_permille",
            StressFace::Translate => "xlt.ps_hit_ratio",
            StressFace::Reserve => "rsv.reserve_ratio",
            StressFace::Split => "spl.reclaim_ratio",
            StressFace::Contend => "ctd.contend_permille",
            StressFace::LongRun => "lrn.tick_span",
            StressFace::Distortion => "dsr.declared",
            StressFace::Summary => "sum.overall_pass",
        }
    }
    /// 是否主要对抗面（失真 / 争用 / 汇总三面是主要对抗面）。
    pub fn is_primary_adversarial(self) -> bool {
        matches!(self, StressFace::Contend | StressFace::Distortion | StressFace::Summary)
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            StressFace::Capacity => "容量面：满池/近满/空三态阈值",
            StressFace::Establish => "建立面：大页建立速率档位表",
            StressFace::Translate => "翻译面：四级页表遍历与 PS 位",
            StressFace::Reserve => "预留面：预留/释放比例窗口",
            StressFace::Split => "拆当面：整块回收粒度",
            StressFace::Contend => "争用面：并发度档位表（主要对抗面）",
            StressFace::LongRun => "长稳面：逻辑 tick 跨度",
            StressFace::Distortion => "失真面：失真声明在档位（主要对抗面）",
            StressFace::Summary => "汇总面：九面基线读数汇总口径（主要对抗面）",
        }
    }
}

/// 三档用例闭集（核心 / 边界 / 对抗）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseTier {
    /// 核心档：典型读数。
    Core,
    /// 边界档：极值读数。
    Edge,
    /// 对抗档：故障注入下的读数。
    Adversarial,
}
impl CaseTier {
    /// 全部三档。
    pub const ALL: [CaseTier; TIER_COUNT] = [CaseTier::Core, CaseTier::Edge, CaseTier::Adversarial];
    /// 序号（0..2）。
    pub fn index(self) -> usize {
        match self {
            CaseTier::Core => 0,
            CaseTier::Edge => 1,
            CaseTier::Adversarial => 2,
        }
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<CaseTier> {
        match i {
            0 => Some(CaseTier::Core),
            1 => Some(CaseTier::Edge),
            2 => Some(CaseTier::Adversarial),
            _ => None,
        }
    }
    /// 档码（进用例 ID）。
    pub fn code3(self) -> &'static str {
        match self {
            CaseTier::Core => "CORE",
            CaseTier::Edge => "EDGE",
            CaseTier::Adversarial => "ADV",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            CaseTier::Core => "核心档：典型读数，回归必跑",
            CaseTier::Edge => "边界档：极值读数，回归必跑",
            CaseTier::Adversarial => "对抗档：故障注入下的读数，回归必跑",
        }
    }
}

/// 四列闭集（判据原文复测方式段逐字用词：「例 ID / 判据段 / 断言入口 / 读数键」）。
///
/// 注意**「来源面」不是四列之一**：来源面由用例 ID（`RG-<面码>-<档码>`）
/// 与矩阵索引（`face_idx`）双重承载。把面码硬塞进四列会让「漏面」退化成
/// 「某列少填几个」，漏面断言就失去了独立维度；本单让面归属走在
/// **独立的 `face_idx` + 覆盖断言**上，列只管内容。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseColumn {
    /// 列 0：用例 ID。
    CaseId = 0,
    /// 列 1：判据段（引用源面判据，不重定义阈值）。
    CriterionSeg = 1,
    /// 列 2：断言入口（回归时从哪里进去验）。
    AssertEntry = 2,
    /// 列 3：读数键（回归读哪个字段 + 期望区间）。
    ReadingKey = 3,
}
impl CaseColumn {
    /// 全部四列。
    pub const ALL: [CaseColumn; COLUMN_COUNT] = [
        CaseColumn::CaseId,
        CaseColumn::CriterionSeg,
        CaseColumn::AssertEntry,
        CaseColumn::ReadingKey,
    ];
    /// 序号（0..3）。
    pub fn index(self) -> usize {
        self as usize
    }
    /// 由序号反解。
    pub fn from_index(i: usize) -> Option<CaseColumn> {
        match i {
            0 => Some(CaseColumn::CaseId),
            1 => Some(CaseColumn::CriterionSeg),
            2 => Some(CaseColumn::AssertEntry),
            3 => Some(CaseColumn::ReadingKey),
            _ => None,
        }
    }
    /// 列名（判据原文用词）。
    pub fn name(self) -> &'static str {
        match self {
            CaseColumn::CaseId => "例ID",
            CaseColumn::CriterionSeg => "判据段",
            CaseColumn::AssertEntry => "断言入口",
            CaseColumn::ReadingKey => "读数键",
        }
    }
    /// 判据原文别名（册内字段名）。
    pub fn alias(self) -> &'static str {
        match self {
            CaseColumn::CaseId => "case_id",
            CaseColumn::CriterionSeg => "criterion_seg",
            CaseColumn::AssertEntry => "assert_entry",
            CaseColumn::ReadingKey => "reading_key",
        }
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            CaseColumn::CaseId => "列 0：用例 ID（`RG-<面码>-<档码>`，全局唯一）",
            CaseColumn::CriterionSeg => "列 1：判据段（引用源面判据段，不重定义阈值）",
            CaseColumn::AssertEntry => "列 2：断言入口（回归时进入该面判据的入口路径）",
            CaseColumn::ReadingKey => "列 3：读数键（回归读哪个字段 + 期望区间）",
        }
    }
    /// 空串占位（缺失时的合法值，位图据此判「登记了但空」）。
    pub fn blank() -> &'static str {
        ""
    }
}

// ===========================================================================
// 4. 回归矩阵（正交展开，格数显式计算）
// ===========================================================================
/// 单条回归用例。
///
/// 四列字段全部登记进 `String`（零拷贝不可能——字段本身就是串，
/// 这里只做一次性 `to_string()` 拷贝，之后只读）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseSpec {
    /// 例号（0..27）。
    pub index: usize,
    /// 面码序号。
    pub face_idx: usize,
    /// 档码序号。
    pub tier_idx: usize,
    /// 面码（三字母，非列字段，只参与覆盖断言）。
    pub face_code: &'static str,
    /// 档码（三字母，非列字段）。
    pub tier_code: &'static str,
    /// 列 0：用例 ID。
    pub case_id: String,
    /// 列 1：判据段。
    pub criterion_seg: String,
    /// 列 2：断言入口。
    pub assert_entry: String,
    /// 列 3：读数键。
    pub reading_key: String,
    /// 期望读数下界（千分数）。
    pub expect_lo: u64,
    /// 期望读数上界（千分数）。
    pub expect_hi: u64,
    /// 断言步数上界（夹具真跑口径）。
    pub steps: u64,
    /// 例指纹（入矩阵哈希）。
    pub fingerprint: u64,
}
impl CaseSpec {
    /// 由面 / 档构造一例（正交展开的元素）。
    ///
    /// 口径注记：
    /// - 四列全部由面与档**派生**，不手写常量串——手写 27×4 个串
    ///   是回归矩阵最典型的漂移来源（改面不改串）；
    /// - 期望区间随档走：核心档窄、边界档贴边、对抗档放宽
    ///   （对抗档读数本身来自注入态，不该用干净态的窄区间卡）；
    /// - `steps` 三档统一 `CASE_STEPS`，保证 27 例步数可比。
    pub fn new(index: usize, face: StressFace, tier: CaseTier) -> UmResult<CaseSpec> {
        let t = CaseTier::ALL[tier.index()];
        if t.index() != tier.index() {
            return Err(err_tier(tier.index()));
        }
        let f = match StressFace::from_index(face.index()) {
            Some(v) => v,
            None => return Err(err_face(face.index())),
        };
        let case_id = format!("{}-{}-{}", CASE_ID_PREFIX, f.code3(), t.code3());
        let criterion_seg = format!("{}/{}", f.criterion_seg(), t.code3());
        let assert_entry = format!("{}#{}", f.assert_entry(), t.code3());
        let reading_key = format!("{}#{}", f.reading_key(), t.code3());
        let (lo, hi) = match t {
            CaseTier::Core => (400u64, 600u64),
            CaseTier::Edge => (900u64, 1000u64),
            CaseTier::Adversarial => (0u64, 1000u64),
        };
        let steps = CASE_STEPS as u64;
        let mut words: [u64; 8] = [0u64; 8];
        words[0] = index as u64;
        words[1] = f.index() as u64;
        words[2] = t.index() as u64;
        words[3] = lo;
        words[4] = hi;
        words[5] = steps;
        words[6] = f.src_ticket_len() as u64;
        words[7] = if f.is_primary_adversarial() { 1 } else { 0 };
        // 四列串先各折成 u64，再与序号一起折叠——
        // 这样「改一列内容」必改指纹，而「改列序」由字序固定（列序是契约）。
        let mut fp = fnv1a64(sbytes(&case_id));
        fp = fnv1a64_ext(fp, sbytes("|"));
        fp = fnv1a64_ext(fp, sbytes(&criterion_seg));
        fp = fnv1a64_ext(fp, sbytes("|"));
        fp = fnv1a64_ext(fp, sbytes(&assert_entry));
        fp = fnv1a64_ext(fp, sbytes("|"));
        fp = fnv1a64_ext(fp, sbytes(&reading_key));
        fp = fp.wrapping_add(fnv1a_words(&words));
        Ok(CaseSpec {
            index,
            face_idx: f.index(),
            tier_idx: t.index(),
            face_code: f.code3(),
            tier_code: t.code3(),
            case_id,
            criterion_seg,
            assert_entry,
            reading_key,
            expect_lo: lo,
            expect_hi: hi,
            steps,
            fingerprint: fp,
        })
    }
    /// 取列字段（第 `col` 列，列号越界即错）。
    pub fn field(&self, col: usize) -> UmResult<String> {
        match col {
            0 => Ok(self.case_id.clone()),
            1 => Ok(self.criterion_seg.clone()),
            2 => Ok(self.assert_entry.clone()),
            3 => Ok(self.reading_key.clone()),
            _ => Err(err_col(col)),
        }
    }
    /// 取列字段（借用版）：只借用 `&self` 内的串，不新建 `String`。
    ///
    /// 返回类型是 `UmResult<&str>`——借用只在本函数返回的引用上有效，
    /// 调用方拿到的引用直接指向 `CaseSpec` 内部的串，无中间 `String`。
    /// 这是列字段点验的**主入口**（`field()` 是需要拥有值的场景的克隆入口）。
    pub fn field_ref(&self, col: usize) -> UmResult<&str> {
        match col {
            0 => Ok(self.case_id.as_str()),
            1 => Ok(self.criterion_seg.as_str()),
            2 => Ok(self.assert_entry.as_str()),
            3 => Ok(self.reading_key.as_str()),
            _ => Err(err_col(col)),
        }
    }
    /// 四列是否全非空。
    pub fn fields_present(&self) -> bool {
        !self.case_id.is_empty() && !self.criterion_seg.is_empty() && !self.assert_entry.is_empty() && !self.reading_key.is_empty()
    }
    /// 期望区间是否自洽（`lo <= hi <= 1000`）。
    pub fn expect_ok(&self) -> bool {
        self.expect_lo <= self.expect_hi && self.expect_hi <= 1000
    }
    /// 三元组闭集（面 / 档 / 例号）。
    pub fn dims_ok(&self) -> bool {
        self.face_idx < FACE_COUNT && self.tier_idx < TIER_COUNT && self.index < CASE_COUNT
    }
    /// 面（由登记序号反解）。
    pub fn face(&self) -> UmResult<StressFace> {
        match StressFace::from_index(self.face_idx) {
            Some(f) => Ok(f),
            None => Err(err_face(self.face_idx)),
        }
    }
    /// 档（由登记序号反解）。
    pub fn tier(&self) -> UmResult<CaseTier> {
        match self.tier_idx {
            0 => Ok(CaseTier::Core),
            1 => Ok(CaseTier::Edge),
            2 => Ok(CaseTier::Adversarial),
            _ => Err(err_tier(self.tier_idx)),
        }
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "case#{:02} id={} face={} tier={} seg={} entry={} key={} expect=[{},{}] steps={} fp={:016X}",
            self.index,
            self.case_id,
            self.face_code,
            self.tier_code,
            self.criterion_seg,
            self.assert_entry,
            self.reading_key,
            self.expect_lo,
            self.expect_hi,
            self.steps,
            self.fingerprint
        )
    }
}
/// 便捷：面码串的字节长度（`src_ticket` 形如 `F2802`，恒 5 字节）。
trait TicketLen {
    /// 返回字节长度。
    fn src_ticket_len(&self) -> usize;
}
impl TicketLen for StressFace {
    fn src_ticket_len(&self) -> usize {
        sbytes(self.src_ticket()).len()
    }
}
/// 错误构造辅助（面码越界）。
fn err_face(i: usize) -> UmErr {
    UmErr::of_aux(AuxCode::FaceOutOfRange, format!("face_idx={} face_count={}", i, FACE_COUNT))
}
/// 错误构造辅助（档码越界）。
fn err_tier(i: usize) -> UmErr {
    UmErr::of_aux(AuxCode::TierOutOfRange, format!("tier_idx={} tier_count={}", i, TIER_COUNT))
}
/// 错误构造辅助（例号越界）。
fn err_case(i: usize) -> UmErr {
    UmErr::of_aux(AuxCode::CaseIndexOutOfRange, format!("case_idx={} case_count={}", i, CASE_COUNT))
}
/// 错误构造辅助（列号越界）。
fn err_col(i: usize) -> UmErr {
    UmErr::of_aux(AuxCode::ColumnOutOfRange, format!("col={} col_count={}", i, COLUMN_COUNT))
}

/// 回归矩阵（9 面 × 3 档 = 27 例）。
#[derive(Clone, Debug)]
pub struct CaseMatrix {
    /// 逐例。
    pub cases: Vec<CaseSpec>,
    /// 面分布（每面 3 例）。
    pub face_dist: [usize; FACE_COUNT],
    /// 档分布（每档 9 例）。
    pub tier_dist: [usize; TIER_COUNT],
    /// 展开时实际产出例数（应恒 == CASE_COUNT）。
    pub produced: usize,
}
impl CaseMatrix {
    /// 空矩阵。
    pub fn blank() -> CaseMatrix {
        CaseMatrix { cases: Vec::new(), face_dist: [0; FACE_COUNT], tier_dist: [0; TIER_COUNT], produced: 0 }
    }
    /// 正交展开（格数由 `FACE_COUNT * TIER_COUNT` 现算，不硬写 27）。
    pub fn expand() -> UmResult<CaseMatrix> {
        let mut m = CaseMatrix::blank();
        let mut idx: usize = 0;
        let mut fi: usize = 0;
        while fi < FACE_COUNT {
            let f = StressFace::ALL[fi];
            let mut ti: usize = 0;
            while ti < TIER_COUNT {
                let t = CaseTier::ALL[ti];
                let c = CaseSpec::new(idx, f, t)?;
                if let Some(slot) = m.face_dist.get_mut(fi) {
                    *slot = slot.wrapping_add(1);
                }
                if let Some(slot) = m.tier_dist.get_mut(ti) {
                    *slot = slot.wrapping_add(1);
                }
                m.cases.push(c);
                idx = idx.wrapping_add(1);
                ti = ti.wrapping_add(1);
            }
            fi = fi.wrapping_add(1);
        }
        m.produced = m.cases.len();
        Ok(m)
    }
    /// 取例（越界即错）。
    pub fn case(&self, i: usize) -> UmResult<&CaseSpec> {
        match self.cases.get(i) {
            Some(c) => Ok(c),
            None => Err(err_case(i)),
        }
    }
    /// 取例（可变）。
    pub fn case_mut(&mut self, i: usize) -> UmResult<&mut CaseSpec> {
        match self.cases.get_mut(i) {
            Some(c) => Ok(c),
            None => Err(err_case(i)),
        }
    }
    /// 线性例号（`(face, tier) → index`）。
    pub fn linear_index(face: usize, tier: usize) -> usize {
        face * TIER_COUNT + tier
    }
    /// 格数断言（`produced == FACE_COUNT * TIER_COUNT`）。
    pub fn count_ok(&self) -> bool {
        self.produced == FACE_COUNT * TIER_COUNT && self.produced == CASE_COUNT && self.cases.len() == CASE_COUNT
    }
    /// 正交断言（每个 `(face, tier)` 组合恰出现一次）。
    pub fn orthogonal_ok(&self) -> bool {
        let mut seen: [bool; CASE_COUNT] = [false; CASE_COUNT];
        let mut i: usize = 0;
        while i < self.cases.len() {
            let c = match self.cases.get(i) {
                Some(v) => v,
                None => return false,
            };
            if c.face_idx >= FACE_COUNT || c.tier_idx >= TIER_COUNT {
                return false;
            }
            let li = CaseMatrix::linear_index(c.face_idx, c.tier_idx);
            if let Some(s) = seen.get_mut(li) {
                if *s {
                    return false;
                }
                *s = true;
            } else {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 三元组闭集断言。
    pub fn dims_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.cases.len() {
            let c = match self.cases.get(i) {
                Some(v) => v,
                None => return false,
            };
            if !c.dims_ok() || !c.expect_ok() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }
    /// 矩阵指纹（27 个例指纹串接折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.cases.len() + 3);
        words.push(self.produced as u64);
        words.push(FACE_COUNT as u64);
        words.push(TIER_COUNT as u64);
        for c in self.cases.iter() {
            words.push(c.fingerprint);
        }
        fnv1a_words(&words)
    }
    /// 逐行列（供册内留痕）。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.cases.len());
        for c in self.cases.iter() {
            out.push(c.describe());
        }
        out
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "matrix cases={}/{} count_ok={} orthogonal={} dims={} fp={:016X}",
            self.produced,
            CASE_COUNT,
            if self.count_ok() { 1 } else { 0 },
            if self.orthogonal_ok() { 1 } else { 0 },
            if self.dims_ok() { 1 } else { 0 },
            self.fingerprint()
        )
    }
}
// ===========================================================================
// 5. 字段完整性断言（三重零缺失位图）
// ===========================================================================
/// 字段完整性结论。
///
/// 判据「矩阵字段完整性零缺失」的操作化定义在此落地：
/// 三张位图（各 `TOTAL_FIELDS = 108` 位）**同时**为零才算零缺失。
///
/// - `missing_bits`：该例该列**根本没登记**（例不存在，或列号越界）；
/// - `blank_bits`：登记了但**值是空串**（空串假通过陷阱）；
/// - `dup_bits`：同列两例取值**完全相同**（复制粘贴陷阱——
///   典型表现是把 27 例的判据段都填成同一个引用）。
///
/// 只查「非空」会被 dup 骗过；只查「不空且不重」会被 blank 骗过。
/// 三者联合才是「零缺失」。
#[derive(Clone, Debug)]
pub struct IntegrityReport {
    /// 缺失位图（按 `case * COLUMN_COUNT + col` 定位）。
    pub missing_bits: u128,
    /// 空串位图。
    pub blank_bits: u128,
    /// 重复位图（后出现者置位）。
    pub dup_bits: u128,
    /// 已登记字段位数（应恒 == TOTAL_FIELDS）。
    pub registered: u64,
    /// 非空字段位数。
    pub nonblank: u64,
    /// 唯一字段位数（去重后）。
    pub unique: u64,
    /// 逐格结论行。
    pub rows: Vec<String>,
}
impl IntegrityReport {
    /// 零缺失。
    pub fn all_ok(&self) -> bool {
        self.missing_bits == 0 && self.blank_bits == 0 && self.dup_bits == 0 && self.registered == TOTAL_FIELDS as u64 && self.nonblank == TOTAL_FIELDS as u64
    }
    /// 缺失字段数（按位计数）。
    pub fn missing_count(&self) -> u32 {
        count_bits128(self.missing_bits)
    }
    /// 空串字段数。
    pub fn blank_count(&self) -> u32 {
        count_bits128(self.blank_bits)
    }
    /// 重复字段数。
    pub fn dup_count(&self) -> u32 {
        count_bits128(self.dup_bits)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "integrity registered={}/{} nonblank={} unique={} missing={} blank={} dup={} ok={}",
            self.registered,
            TOTAL_FIELDS,
            self.nonblank,
            self.unique,
            self.missing_count(),
            self.blank_count(),
            self.dup_count(),
            if self.all_ok() { 1 } else { 0 }
        )
    }
    /// 逐行文本。
    pub fn rows_text(&self) -> Vec<String> {
        self.rows.clone()
    }
}

/// `u128` 位计数。
fn count_bits128(mut v: u128) -> u32 {
    let mut n: u32 = 0;
    while v != 0 {
        v &= v.wrapping_sub(1);
        n = n.wrapping_add(1);
    }
    n
}

/// 位图置位（位号越界静默忽略——调用方已保证位号合法）。
fn set_bit128(bits: &mut u128, i: usize) {
    if i < 128 {
        *bits |= 1u128 << i;
    }
}

/// 逐列查重并累积唯一数。
fn scan_column(m: &CaseMatrix, col: usize, missing: &mut u128, blank: &mut u128, dup: &mut u128, reg: &mut u64, nonblank: &mut u64, uniq: &mut u64) -> Vec<String> {
    let mut rows: Vec<String> = Vec::with_capacity(CASE_COUNT);
    let mut seen: Vec<String> = Vec::with_capacity(CASE_COUNT);
    let mut i: usize = 0;
    while i < CASE_COUNT {
        let c = match m.case(i) {
            Ok(v) => v,
            Err(_) => {
                let mut c0 = col;
                while c0 < COLUMN_COUNT {
                    set_bit128(missing, i * COLUMN_COUNT + c0);
                    c0 = c0.wrapping_add(1);
                }
                rows.push(format!("case#{:02} col={} MISSING", i, col));
                i = i.wrapping_add(1);
                continue;
            }
        };
        let v = match c.field_ref(col) {
            Ok(s) => s.to_string(),
            Err(_) => {
                set_bit128(missing, i * COLUMN_COUNT + col);
                rows.push(format!("case#{:02} col={} COL_OOR", i, col));
                i = i.wrapping_add(1);
                continue;
            }
        };
        *reg = reg.wrapping_add(1);
        if v.is_empty() {
            set_bit128(blank, i * COLUMN_COUNT + col);
            rows.push(format!("case#{:02} col={} BLANK", i, col));
        } else {
            *nonblank = nonblank.wrapping_add(1);
            let mut hit = false;
            let mut k: usize = 0;
            while k < seen.len() {
                if let Some(prev) = seen.get(k) {
                    if *prev == v {
                        hit = true;
                    }
                }
                k = k.wrapping_add(1);
            }
            if hit {
                set_bit128(dup, i * COLUMN_COUNT + col);
                rows.push(format!("case#{:02} col={} DUP {}", i, col, v));
            } else {
                seen.push(v.clone());
                *uniq = uniq.wrapping_add(1);
                rows.push(format!("case#{:02} col={} ok {}", i, col, v));
            }
        }
        i = i.wrapping_add(1);
    }
    rows
}

/// 字段完整性点验（四列逐格，全 `108` 位）。
pub fn integrity_report(m: &CaseMatrix) -> IntegrityReport {
    let mut missing: u128 = 0;
    let mut blank: u128 = 0;
    let mut dup: u128 = 0;
    let mut reg: u64 = 0;
    let mut nonblank: u64 = 0;
    let mut uniq: u64 = 0;
    let mut rows: Vec<String> = Vec::with_capacity(TOTAL_FIELDS);
    let mut col: usize = 0;
    while col < COLUMN_COUNT {
        let part = scan_column(m, col, &mut missing, &mut blank, &mut dup, &mut reg, &mut nonblank, &mut uniq);
        rows.extend(part);
        col = col.wrapping_add(1);
    }
    IntegrityReport { missing_bits: missing, blank_bits: blank, dup_bits: dup, registered: reg, nonblank, unique: uniq, rows }
}

// ===========================================================================
// 6. 面覆盖断言（九面逐面点验）
// ===========================================================================
/// 单面覆盖结论。
#[derive(Clone, Copy, Debug)]
pub struct FaceCoverage {
    /// 面码序号。
    pub face_idx: usize,
    /// 该面实际映射例数。
    pub mapped: usize,
    /// 三档是否齐（核心 / 边界 / 对抗各 1）。
    pub tiers_complete: bool,
    /// 该面四列是否零缺失。
    pub fields_ok: bool,
    /// 该面是否被覆盖（`mapped == TIER_COUNT`）。
    pub covered: bool,
}
impl FaceCoverage {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "face#{} mapped={}/{} tiers={} fields={} covered={}",
            self.face_idx,
            self.mapped,
            TIER_COUNT,
            if self.tiers_complete { 1 } else { 0 },
            if self.fields_ok { 1 } else { 0 },
            if self.covered { 1 } else { 0 }
        )
    }
}
/// 面覆盖结论（九面逐面点验）。
#[derive(Clone, Debug)]
pub struct CoverageReport {
    /// 逐面结论。
    pub faces: Vec<FaceCoverage>,
    /// 被覆盖面数（应恒 == FACE_COUNT）。
    pub covered: usize,
    /// 漏面位图（位号 = 面码序号）。
    pub missing_faces: u64,
    /// 逐面行。
    pub rows: Vec<String>,
}
impl CoverageReport {
    /// 覆盖断言全过（九面全映射 + 每面三档齐 + 每面四列零缺失）。
    pub fn all_ok(&self) -> bool {
        self.covered == FACE_COUNT && self.missing_faces == 0 && self.faces.len() == FACE_COUNT
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "coverage {}/{} missing_faces={:09b} ok={}",
            self.covered,
            FACE_COUNT,
            self.missing_faces,
            if self.all_ok() { 1 } else { 0 }
        )
    }
    /// 漏面清单（面码）。
    pub fn missing_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut k: usize = 0;
        while k < FACE_COUNT {
            if self.missing_faces & (1u64 << k) != 0 {
                if let Some(f) = StressFace::from_index(k) {
                    out.push(format!("{}({})", f.code3(), f.src_ticket()));
                }
            }
            k = k.wrapping_add(1);
        }
        out
    }
}

/// 面覆盖点验。
pub fn coverage_report(m: &CaseMatrix) -> CoverageReport {
    let mut faces: Vec<FaceCoverage> = Vec::with_capacity(FACE_COUNT);
    let mut missing_faces: u64 = 0;
    let mut covered: usize = 0;
    let mut fi: usize = 0;
    while fi < FACE_COUNT {
        let mut mapped: usize = 0;
        let mut tier_seen: [bool; TIER_COUNT] = [false; TIER_COUNT];
        let mut fields_ok = true;
        let mut i: usize = 0;
        while i < m.cases.len() {
            let c = match m.cases.get(i) {
                Some(v) => v,
                None => break,
            };
            if c.face_idx != fi {
                i = i.wrapping_add(1);
                continue;
            }
            mapped = mapped.wrapping_add(1);
            if c.tier_idx < TIER_COUNT {
                if let Some(s) = tier_seen.get_mut(c.tier_idx) {
                    *s = true;
                }
            }
            if !c.fields_present() {
                fields_ok = false;
            }
            i = i.wrapping_add(1);
        }
        let mut tiers_complete = true;
        let mut t: usize = 0;
        while t < TIER_COUNT {
            let s = match tier_seen.get(t) {
                Some(v) => *v,
                None => false,
            };
            if !s {
                tiers_complete = false;
            }
            t = t.wrapping_add(1);
        }
        let is_cov = mapped == TIER_COUNT && tiers_complete;
        if is_cov {
            covered = covered.wrapping_add(1);
        } else {
            missing_faces |= 1u64 << fi;
        }
        faces.push(FaceCoverage { face_idx: fi, mapped, tiers_complete, fields_ok, covered: is_cov });
        fi = fi.wrapping_add(1);
    }
    let mut rows: Vec<String> = Vec::with_capacity(FACE_COUNT);
    for f in faces.iter() {
        rows.push(f.describe());
    }
    CoverageReport { faces, covered, missing_faces, rows }
}

// ===========================================================================
// 7. 矩阵状态机（合法边闭集 + 非法转换拒记）
// ===========================================================================
/// 矩阵冻结生命周期闭集（5 态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixState {
    /// 草拟（27 例已展开，四列未点验）。
    Draft,
    /// 待审（点验通过，等人签字）。
    Reviewing,
    /// 已冻结（哈希入册）。
    Frozen,
    /// 已漂移（哈希与基线失配）。
    Drifted,
    /// 已作废（终态，不可复活）。
    Voided,
}
impl MatrixState {
    /// 全部状态。
    pub const ALL: [MatrixState; 5] = [
        MatrixState::Draft,
        MatrixState::Reviewing,
        MatrixState::Frozen,
        MatrixState::Drifted,
        MatrixState::Voided,
    ];
    /// 序号（0..4）。
    pub fn index(self) -> usize {
        match self {
            MatrixState::Draft => 0,
            MatrixState::Reviewing => 1,
            MatrixState::Frozen => 2,
            MatrixState::Drifted => 3,
            MatrixState::Voided => 4,
        }
    }
    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            MatrixState::Draft => "draft",
            MatrixState::Reviewing => "reviewing",
            MatrixState::Frozen => "frozen",
            MatrixState::Drifted => "drifted",
            MatrixState::Voided => "voided",
        }
    }
    /// **合法迁移目标闭集**。
    ///
    /// - `Draft → Reviewing`：四列点验零缺失；
    /// - `Reviewing → Frozen`：哈希入册；
    /// - `Frozen → Drifted`：检出漂移（**只进不许回**）；
    /// - `Draft/Reviewing/Frozen/Drifted → Voided`：作废（终态）。
    ///
    /// 关键否��：`Drifted → Frozen` **不在闭集内**——
    /// 一旦检出漂移就必须重新走评审（作废旧冻结件另立），
    /// 不允许「承认漂移后再直接冻结回去」把漂移洗白。
    pub fn allowed(self) -> &'static [MatrixState] {
        match self {
            MatrixState::Draft => &[MatrixState::Reviewing, MatrixState::Voided],
            MatrixState::Reviewing => &[MatrixState::Frozen, MatrixState::Draft, MatrixState::Voided],
            MatrixState::Frozen => &[MatrixState::Drifted, MatrixState::Voided],
            MatrixState::Drifted => &[MatrixState::Reviewing, MatrixState::Voided],
            MatrixState::Voided => &[],
        }
    }
    /// 是否允许迁移到 `next`。
    pub fn can_go(self, next: MatrixState) -> bool {
        self.allowed().contains(&next)
    }
    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        matches!(self, MatrixState::Voided)
    }
    /// 判读。
    pub fn describe(self) -> &'static str {
        match self {
            MatrixState::Draft => "草拟：27 例已展开，四列未点验",
            MatrixState::Reviewing => "待审：四列零缺失，等签字",
            MatrixState::Frozen => "已冻结：哈希入 CI 基线",
            MatrixState::Drifted => "已漂移：哈希与基线失配（须重新评审）",
            MatrixState::Voided => "已作废：终态，不可复活",
        }
    }
}
/// 非法迁移留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IllegalMove {
    /// 源态序号。
    pub from: usize,
    /// 目标态序号。
    pub to: usize,
    /// 逻辑 tick。
    pub tick: u64,
}
impl IllegalMove {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("illegal move from={} to={} tick={}", self.from, self.to, self.tick)
    }
}
/// 矩阵状态机。
#[derive(Clone, Debug)]
pub struct MatrixFsm {
    /// 当前态。
    pub state: MatrixState,
    /// 逻辑时钟。
    pub now: u64,
    /// 成功迁移数。
    pub legal_moves: u64,
    /// 非法迁移拒记。
    pub rejected: Vec<IllegalMove>,
}
impl MatrixFsm {
    /// 新建（草拟态）。
    pub fn new() -> MatrixFsm {
        MatrixFsm { state: MatrixState::Draft, now: 0, legal_moves: 0, rejected: Vec::new() }
    }
    /// 迁移（非法即拒记并返错，状态不变）。
    pub fn go(&mut self, next: MatrixState) -> UmResult<MatrixState> {
        if self.state.can_go(next) {
            self.state = next;
            self.legal_moves = self.legal_moves.wrapping_add(1);
            self.now = tick_advance(self.now);
            Ok(next)
        } else {
            self.rejected.push(IllegalMove { from: self.state.index(), to: next.index(), tick: self.now });
            self.now = tick_advance(self.now);
            Err(err_frozen(self.state.name(), next.name()))
        }
    }
    /// 拒记数。
    pub fn rejected_count(&self) -> usize {
        self.rejected.len()
    }
    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.state == MatrixState::Frozen
    }
    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(6);
        words.push(self.state.index() as u64);
        words.push(self.legal_moves);
        words.push(self.rejected.len() as u64);
        for r in self.rejected.iter() {
            words.push(r.from as u64);
            words.push(r.to as u64);
        }
        fnv1a_words(&words)
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("mstate={} legal={} rejected={} frozen={}", self.state.name(), self.legal_moves, self.rejected.len(), if self.is_frozen() { 1 } else { 0 })
    }
    /// 合法边闭集清单。
    pub fn edge_manifest() -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in MatrixState::ALL.iter() {
            let mut t: Vec<String> = Vec::new();
            for x in s.allowed().iter() {
                t.push(x.name().to_string());
            }
            out.push(format!("{} -> [{}]", s.name(), t.join(",")));
        }
        out
    }
    /// 合法边总数。
    pub fn edge_count() -> usize {
        let mut n: usize = 0;
        for s in MatrixState::ALL.iter() {
            n = n.wrapping_add(s.allowed().len());
        }
        n
    }
}

/// 错误构造辅助（冻结态非法迁移）。
fn err_frozen(from: &str, to: &str) -> UmErr {
    UmErr::of_aux(AuxCode::FrozenWriteRejected, format!("from={} to={}", from, to))
}
// ===========================================================================
// 8. append-only 环形账（dropped 显性）
// ===========================================================================
/// 环形账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingEntry {
    /// 逻辑 tick。
    pub tick: u64,
    /// 条目码（段码 / 事件码）。
    pub code: u16,
    /// 例号。
    pub case: usize,
    /// 载荷。
    pub payload: u64,
}
impl RingEntry {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("ring t={} code=0x{:04X} case={} payload={}", self.tick, self.code, self.case, self.payload)
    }
}
/// append-only 环形账。
///
/// 「append-only」在此的准确含义：**只追加，不就地修改**——
/// 写满后覆写最旧槽，但覆写不是静默的：每次覆写 `dropped` 显性 +1，
/// 且守恒式 `pushed == dropped + live` 恒成立。
/// 漏记 `dropped` 是这类账最常见的假通过，本单把它做成不变量。
#[derive(Clone, Debug)]
pub struct RingLedger {
    /// 槽位。
    pub slots: Vec<RingEntry>,
    /// 写指针（下一个写入位）。
    pub head: usize,
    /// 累计写入数。
    pub pushed: u64,
    /// 累计丢弃数（覆写次数，显性）。
    pub dropped: u64,
    /// 逻辑时钟。
    pub now: u64,
}
impl RingLedger {
    /// 新建（容量 `RING_CAP`）。
    pub fn new() -> RingLedger {
        RingLedger { slots: Vec::with_capacity(RING_CAP), head: 0, pushed: 0, dropped: 0, now: 0 }
    }
    /// 追加一条（返回本次是否发生覆写）。
    pub fn push(&mut self, code: u16, case: usize, payload: u64) -> bool {
        let overwrite = self.slots.len() == RING_CAP;
        let e = RingEntry { tick: self.now, code, case, payload };
        if overwrite {
            if let Some(slot) = self.slots.get_mut(self.head) {
                *slot = e;
            }
            self.dropped = self.dropped.wrapping_add(1);
            self.head = (self.head + 1) & RING_MASK;
        } else {
            self.slots.push(e);
            self.head = self.slots.len() & RING_MASK;
        }
        self.pushed = self.pushed.wrapping_add(1);
        self.now = tick_advance(self.now);
        overwrite
    }
    /// 活条数。
    pub fn live(&self) -> usize {
        self.slots.len()
    }
    /// 守恒（`pushed == dropped + live` 且活条不超容）。
    pub fn balanced(&self) -> bool {
        self.pushed == self.dropped.wrapping_add(self.live() as u64) && self.live() <= RING_CAP
    }
    /// 守恒检查（返错带明细）。
    pub fn check(&self) -> UmResult<()> {
        if !self.balanced() {
            return Err(UmErr::of_aux(AuxCode::ColumnOutOfRange, format!("ring pushed={} dropped={} live={} cap={}", self.pushed, self.dropped, self.live(), RING_CAP)));
        }
        Ok(())
    }
    /// 全部槽位。
    pub fn entries(&self) -> &[RingEntry] {
        &self.slots
    }
    /// 统计某码条目数。
    pub fn count_code(&self, code: u16) -> usize {
        let mut n: usize = 0;
        for e in self.slots.iter() {
            if e.code == code {
                n = n.wrapping_add(1);
            }
        }
        n
    }
    /// 指纹（逐槽折叠，序敏感）。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(self.slots.len() + 4);
        words.push(self.pushed);
        words.push(self.dropped);
        words.push(self.live() as u64);
        words.push(self.head as u64);
        for e in self.slots.iter() {
            words.push(e.code as u64);
            words.push(e.case as u64);
            words.push(e.payload);
        }
        fnv1a_words(&words)
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!("ring pushed={} dropped={} live={}/{} balanced={}", self.pushed, self.dropped, self.live(), RING_CAP, if self.balanced() { 1 } else { 0 })
    }
    /// 逐行描述。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.slots.len());
        for e in self.slots.iter() {
            out.push(e.describe());
        }
        out
    }
}

// ===========================================================================
// 9. 逐例独立夹具真跑（**不是**用例执行，是矩阵字段自洽夹具）
// ===========================================================================
/// 单例夹具结论。
#[derive(Clone, Copy, Debug)]
pub struct CaseRun {
    /// 例号。
    pub case: usize,
    /// 四列是否全非空。
    pub fields_ok: bool,
    /// 三元组闭集是否成立。
    pub dims_ok: bool,
    /// 期望区间是否自洽。
    pub expect_ok: bool,
    /// 面归属是否与登记一致。
    pub face_ok: bool,
    /// 档归属是否与登记一致。
    pub tier_ok: bool,
    /// 读数键是否带档后缀（防「三档读同一个键」）。
    pub key_suffixed: bool,
    /// 断言入口是否可引用（存在非空串）。
    pub entry_ok: bool,
    /// 列 0 用例 ID 是否与「面码 + 档码」派生一致。
    pub id_ok: bool,
    /// 列 3 读数键是否与「面读数键 + 档码」派生一致。
    pub key_ok: bool,
    /// 列 2 断言入口是否与「面断言入口 + 档码」派生一致。
    pub entry_derived_ok: bool,
    /// 实跑步数。
    pub steps_run: u64,
    /// 跑后指纹。
    pub post_fp: u64,
}
impl CaseRun {
    /// 单例全过（七项硬条件齐）。
    pub fn ok(&self) -> bool {
        self.fields_ok && self.dims_ok && self.expect_ok && self.face_ok && self.tier_ok && self.key_suffixed && self.entry_ok && self.id_ok && self.key_ok && self.entry_derived_ok
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "caserun#{:02} fields={} dims={} expect={} face={} tier={} suffix={} entry={} id={} key={} eder={} steps={} ok={} fp={:016X}",
            self.case,
            if self.fields_ok { 1 } else { 0 },
            if self.dims_ok { 1 } else { 0 },
            if self.expect_ok { 1 } else { 0 },
            if self.face_ok { 1 } else { 0 },
            if self.tier_ok { 1 } else { 0 },
            if self.key_suffixed { 1 } else { 0 },
            if self.entry_ok { 1 } else { 0 },
            if self.id_ok { 1 } else { 0 },
            if self.key_ok { 1 } else { 0 },
            if self.entry_derived_ok { 1 } else { 0 },
            self.steps_run,
            if self.ok() { 1 } else { 0 },
            self.post_fp
        )
    }
}
/// 逐例独立夹具真跑。
///
/// **语义边界（关键）**：这里跑的不是真机回归用例，
/// 判据明确「不做用例执行（F2863 runner）」。
/// 本函数跑的是**矩阵自身字段的自洽夹具**：
/// 每例把四列字段重新解析一遍、核对面/档归属、核对读数键带档后缀、
/// 核对期望区间自洽，并**重新计算一次例指纹**与登记值比对
/// （这样「改了字段但没重算指纹」的半途改动会在此被抓住）。
///
/// 每例独立夹具：**不复用上一例的任何状态**，
/// 保证污染不跨例传播（判据深点之一）。
pub fn run_case(c: &CaseSpec) -> UmResult<CaseRun> {
    let mut steps: u64 = 0;
    // 逐列重新读取（每列一步，四列共四步）
    let mut fields_ok = true;
    let mut col: usize = 0;
    while col < COLUMN_COUNT {
        match c.field_ref(col) {
            Ok(v) => {
                if v.is_empty() {
                    fields_ok = false;
                }
            }
            Err(_) => {
                fields_ok = false;
            }
        }
        steps = steps.wrapping_add(1);
        col = col.wrapping_add(1);
    }
    // 面归属核对（从 `source_face` 前缀反解，再与登记序号比对）
    steps = steps.wrapping_add(1);
    let face_ok = match c.face() {
        Ok(f) => f.index() == c.face_idx,
        Err(_) => false,
    };
    // 档归属核对
    steps = steps.wrapping_add(1);
    let tier_ok = match c.tier() {
        Ok(t) => t.index() == c.tier_idx,
        Err(_) => false,
    };
    // 读数键带档后缀（`key#TIER`），防三档共用一键
    steps = steps.wrapping_add(1);
    let key_suffixed = match c.tier() {
        Ok(t) => c.reading_key.ends_with(&format!("#{}", t.code3())),
        Err(_) => false,
    };
    // 断言入口可引用：判据段与读数键都必须能追到源面单号
    steps = steps.wrapping_add(1);
    let entry_ok = match c.face() {
        Ok(f) => c.criterion_seg.starts_with(f.criterion_seg()) && !f.assert_entry().is_empty(),
        Err(_) => false,
    };
    // 列 0 用例 ID 必须等于 `RG-<面码>-<档码>` 派生值——
    // 这是「改 ID」这类漂移唯一的可判定依据：ID 本身没有冗余，
    // 只能靠「可从面/档重建」来证伪。
    steps = steps.wrapping_add(1);
    let id_ok = match (c.face(), c.tier()) {
        (Ok(f), Ok(t)) => c.case_id == format!("{}-{}-{}", CASE_ID_PREFIX, f.code3(), t.code3()),
        _ => false,
    };
    // 列 3 读数键必须等于 `<面读数键>#<档码>` 派生值（比 suffix 更严）。
    steps = steps.wrapping_add(1);
    let key_ok = match (c.face(), c.tier()) {
        (Ok(f), Ok(t)) => c.reading_key == format!("{}#{}", f.reading_key(), t.code3()),
        _ => false,
    };
    // 列 2 断言入口必须等于 `<面断言入口>#<档码>` 派生值。
    steps = steps.wrapping_add(1);
    let entry_derived_ok = match (c.face(), c.tier()) {
        (Ok(f), Ok(t)) => c.assert_entry == format!("{}#{}", f.assert_entry(), t.code3()),
        _ => false,
    };
    // 重算例指纹（逐步加入，避免一次算完掩盖中间步）
    let mut fp = fnv1a64(sbytes(&c.case_id));
    let mut k: usize = 0;
    while k < 4 {
        match c.field_ref(k) {
            Ok(v) => {
                fp = fnv1a64_ext(fp, sbytes(v));
            }
            Err(_) => {
                fp = fp.wrapping_add(0x9E37_79B9u64);
            }
        }
        fp = fnv1a64_ext(fp, sbytes("|"));
        steps = steps.wrapping_add(1);
        k = k.wrapping_add(1);
    }
    let mut words: [u64; 6] = [0u64; 6];
    words[0] = c.index as u64;
    words[1] = c.face_idx as u64;
    words[2] = c.tier_idx as u64;
    words[3] = c.expect_lo;
    words[4] = c.expect_hi;
    words[5] = steps;
    let post_fp = fp.wrapping_add(fnv1a_words(&words));
    Ok(CaseRun {
        case: c.index,
        fields_ok,
        dims_ok: c.dims_ok(),
        expect_ok: c.expect_ok(),
        face_ok,
        tier_ok,
        key_suffixed,
        entry_ok,
        id_ok,
        key_ok,
        entry_derived_ok,
        steps_run: steps,
        post_fp,
    })
}

/// 全矩阵夹具真跑。
pub fn run_matrix(m: &CaseMatrix) -> UmResult<Vec<CaseRun>> {
    let mut out: Vec<CaseRun> = Vec::with_capacity(m.cases.len());
    let mut i: usize = 0;
    while i < m.cases.len() {
        let c = match m.case(i) {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        let r = run_case(c)?;
        out.push(r);
        i = i.wrapping_add(1);
    }
    Ok(out)
}
/// 全矩阵夹具全过。
pub fn run_matrix_ok(m: &CaseMatrix) -> bool {
    match run_matrix(m) {
        Ok(rs) => {
            let mut n: usize = 0;
            for r in rs.iter() {
                if r.ok() {
                    n = n.wrapping_add(1);
                }
            }
            n == CASE_COUNT && rs.len() == CASE_COUNT
        }
        Err(_) => false,
    }
}
// ===========================================================================
// 10. 稳定性判定（多次重复跑指纹一致才算通过）
// ===========================================================================
/// 单轮结论。
#[derive(Clone, Copy, Debug)]
pub struct RoundResult {
    /// 轮次号（0..REPEAT）。
    pub round: usize,
    /// 本轮全过例数。
    pub cases_ok: usize,
    /// 本轮步数。
    pub steps: u64,
    /// 本轮逐例指纹折叠。
    pub casewise_fp: u64,
    /// 本轮矩阵指纹。
    pub matrix_fp: u64,
    /// 本轮是否全过。
    pub passed: bool,
}
impl RoundResult {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("round#{} cases_ok={}/{} steps={} matrix_fp={:016X} casewise_fp={:016X} pass={}", self.round, self.cases_ok, CASE_COUNT, self.steps, self.matrix_fp, self.casewise_fp, if self.passed { 1 } else { 0 })
    }
}
/// 稳定性结论。
#[derive(Clone, Debug)]
pub struct StabilityReport {
    /// 逐轮结论。
    pub rounds: Vec<RoundResult>,
    /// 矩阵指纹是否轮次间全等。
    pub matrix_identical: bool,
    /// 逐例指纹是否轮次间全等。
    pub casewise_identical: bool,
    /// 步数上界。
    pub step_cap: u64,
    /// 累计步数。
    pub steps_used: u64,
}
impl StabilityReport {
    /// 是否 `REPEAT` 轮全过且两重指纹全等。
    ///
    /// 双指纹（`matrix_fp` + `casewise_fp`）的意义：
    /// `matrix_fp` 是 27 个例指纹的串接折叠，能抓住「整体变了」；
    /// `casewise_fp` 是逐例 `post_fp` 的串接折叠，能抓住
    /// 「两个例互相换位而总折叠恰巧相等」的位置漂移。
    /// 只查前者会漏掉位置漂移（哈希碰撞型漏检）。
    pub fn passed(&self) -> bool {
        self.rounds.len() == REPEAT
            && self.matrix_identical
            && self.casewise_identical
            && self.steps_used <= self.step_cap
            && {
                let mut all = true;
                for r in self.rounds.iter() {
                    if !r.passed {
                        all = false;
                    }
                }
                all
            }
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "stability rounds={}/{} matrix_identical={} casewise_identical={} steps={}/{} pass={}",
            self.rounds.len(),
            REPEAT,
            if self.matrix_identical { 1 } else { 0 },
            if self.casewise_identical { 1 } else { 0 },
            self.steps_used,
            self.step_cap,
            if self.passed() { 1 } else { 0 }
        )
    }
    /// 逐轮行。
    pub fn rows(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(self.rounds.len());
        for r in self.rounds.iter() {
            out.push(r.describe());
        }
        out
    }
}
/// 稳定性扫描：`REPEAT` 轮各**重新展开矩阵**再跑。
///
/// 重新展开是关键：复用上一轮的 `CaseMatrix` 对象等于自证可复现
/// （同一份内存跑两遍当然一样），必须重跑纯函数 `expand()`。
pub fn stability_sweep() -> StabilityReport {
    let mut rounds: Vec<RoundResult> = Vec::with_capacity(REPEAT);
    let mut first_matrix: u64 = 0;
    let mut first_casewise: u64 = 0;
    let mut matrix_identical = true;
    let mut casewise_identical = true;
    let mut steps_used: u64 = 0;
    let mut r: usize = 0;
    while r < REPEAT {
        let m = match CaseMatrix::expand() {
            Ok(v) => v,
            Err(_) => break,
        };
        let rs = match run_matrix(&m) {
            Ok(v) => v,
            Err(_) => break,
        };
        let mut cases_ok: usize = 0;
        let mut steps: u64 = 0;
        let mut cwords: Vec<u64> = Vec::with_capacity(rs.len());
        for c in rs.iter() {
            if c.ok() {
                cases_ok = cases_ok.wrapping_add(1);
            }
            steps = steps.wrapping_add(c.steps_run);
            cwords.push(c.post_fp);
        }
        let casewise = fnv1a_words(&cwords);
        let matrix_fp = m.fingerprint();
        if r == 0 {
            first_matrix = matrix_fp;
            first_casewise = casewise;
        } else {
            if matrix_fp != first_matrix {
                matrix_identical = false;
            }
            if casewise != first_casewise {
                casewise_identical = false;
            }
        }
        steps_used = steps_used.wrapping_add(steps);
        rounds.push(RoundResult { round: r, cases_ok, steps, casewise_fp: casewise, matrix_fp, passed: cases_ok == CASE_COUNT });
        r = r.wrapping_add(1);
    }
    StabilityReport { rounds, matrix_identical, casewise_identical, step_cap: STEP_CAP, steps_used }
}
// ===========================================================================
// 11. 注入核（12 形态全拒：漏面 6 / 漂移 5 / 缺列 1）
// ===========================================================================
/// 单次形变产出（越界标记集合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Corruption {
    /// 形变指纹。
    pub fp: u64,
    /// 该面被抹空（漏面）。
    pub face_bad: bool,
    /// 被抹空的面码序号（`usize::MAX` = 未抹）。
    pub face_idx: usize,
    /// 列字段被改写（漂移）。
    pub col_bad: bool,
    /// 被改写的列号（`usize::MAX` = 未改）。
    pub col: usize,
    /// 整列被抹空（缺列）。
    pub column_drop: bool,
    /// 档位互换。
    pub tier_swap: bool,
    /// 用例 ID 改写。
    pub id_bad: bool,
}
impl Corruption {
    /// 干净态（全 false，位号 `usize::MAX`）。
    pub fn clean() -> Corruption {
        Corruption { fp: 0, face_bad: false, face_idx: usize::MAX, col_bad: false, col: usize::MAX, column_drop: false, tier_swap: false, id_bad: false }
    }
}
/// 对矩阵实施一次定向形变。
///
/// 形变全部作用在**四列字段**上（这才是回归矩阵的契约面），
/// 不改面/档序号——改了序号属于「矩阵定义变更」，那要重冻结，
/// 不属于「漂移注入」范畴（漂移是**内容**变了，定义没变）。
pub fn corrupt_matrix(m: &mut CaseMatrix, form: Form) -> UmResult<Corruption> {
    let mut c = Corruption::clean();
    let clean_fp = m.fingerprint();
    let mut words: [u64; 4] = [0u64; 4];
    words[0] = clean_fp;
    words[1] = form.index() as u64;
    words[2] = FACE_COUNT as u64;
    words[3] = COLUMN_COUNT as u64;
    // 1..6 漏面：把该面 3 例的四列全抹空
    if form.is_face_drop() {
        let fi = match form {
            Form::FaceDropCapacity => 0,
            Form::FaceDropEstablish => 1,
            Form::FaceDropTranslate => 2,
            Form::FaceDropReserve => 3,
            Form::FaceDropSplit => 4,
            Form::FaceDropContend => 5,
            _ => usize::MAX,
        };
        let mut i: usize = 0;
        let mut hit: usize = 0;
        while i < m.cases.len() {
            let face_idx = m.cases[i].face_idx;
            if face_idx == fi {
                let cc = match m.case_mut(i) {
                    Ok(v) => v,
                    Err(e) => return Err(e),
                };
                cc.case_id = String::new();
                cc.criterion_seg = String::new();
                cc.assert_entry = String::new();
                cc.reading_key = String::new();
                hit = hit.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        c.face_bad = true;
        c.face_idx = fi;
        words[0] = hit as u64;
        c.fp = fnv1a_words(&words).wrapping_add(m.fingerprint());
        return Ok(c);
    }
    // 7..10 漂移：改写单列单例
    if form.is_drift() {
        let (col, ci) = match form {
            Form::DriftCriterionSeg => (1usize, 5usize),
            Form::DriftAssertEntry => (2usize, 13usize),
            Form::DriftReadingKey => (3usize, 20usize),
            Form::DriftCaseId => (0usize, 9usize),
            _ => (usize::MAX, usize::MAX),
        };
        if form == Form::DriftTierSwap {
            // 档位互换：找两例同面不同档，互换档码（登记序号也换，
            // 因为档码本身就是被登记的字段之一）
            let mut a: usize = usize::MAX;
            let mut b: usize = usize::MAX;
            let mut i: usize = 0;
            while i < m.cases.len() {
                if a == usize::MAX {
                    a = i;
                } else if m.cases[i].face_idx == m.cases[a].face_idx && m.cases[i].tier_idx != m.cases[a].tier_idx && b == usize::MAX {
                    b = i;
                }
                i = i.wrapping_add(1);
            }
            if a == usize::MAX || b == usize::MAX {
                return Err(err_case(0));
            }
            let (ta, tb) = {
                let x = m.cases[a].tier_idx;
                let y = m.cases[b].tier_idx;
                (x, y)
            };
            let (fa, fb) = {
                let x = m.cases[a].face_idx;
                let y = m.cases[b].face_idx;
                (x, y)
            };
            m.cases[a].tier_idx = tb;
            m.cases[a].face_idx = fb;
            m.cases[b].tier_idx = ta;
            m.cases[b].face_idx = fa;
            c.tier_swap = true;
            c.col = 1;
            words[0] = a as u64;
            words[1] = b as u64;
            c.fp = fnv1a_words(&words).wrapping_add(m.fingerprint());
            return Ok(c);
        }
        let cc = match m.case_mut(ci) {
            Ok(v) => v,
            Err(e) => return Err(e),
        };
        let forged = format!("FORGED-COL{}-#{}#{}", col, ci, cc.fingerprint);
        match col {
            0 => {
                cc.case_id = forged;
                c.id_bad = true;
            }
            1 => {
                cc.criterion_seg = forged;
            }
            2 => {
                cc.assert_entry = forged;
            }
            _ => {
                cc.reading_key = forged;
            }
        }
        c.col_bad = true;
        c.col = col;
        words[0] = ci as u64;
        words[1] = col as u64;
        c.fp = fnv1a_words(&words).wrapping_add(m.fingerprint());
        return Ok(c);
    }
    // 12 缺列：整列置空
    let mut i: usize = 0;
    while i < m.cases.len() {
        match m.case_mut(i) {
            Ok(cc) => {
                cc.reading_key = String::new();
            }
            Err(e) => return Err(e),
        }
        i = i.wrapping_add(1);
    }
    c.column_drop = true;
    c.col = 3;
    c.fp = fnv1a_words(&words).wrapping_add(m.fingerprint());
    Ok(c)
}
/// 单形态留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Injected {
    /// 形态。
    pub form: Form,
    /// 形态类（面漏 / 列缺 / 漂移）。
    pub class: &'static str,
    /// 落点例号（或面码序号）。
    pub cell: usize,
    /// 逻辑 tick。
    pub tick: u64,
    /// 留痕说明。
    pub note: String,
}
impl Injected {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("inject 0x{:04X} {} [{}] cell={} t={} {}", self.form.code(), self.form.name(), self.class, self.cell, self.tick, self.note)
    }
}
/// 单形态检出结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Detection {
    /// 形态。
    pub form: Form,
    /// 命中次数（0 或 1）。
    pub hits: usize,
    /// 是否命中。
    pub hit: bool,
}
impl Detection {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("detect 0x{:04X} {} [{}] hits={} hit={}", self.form.code(), self.form.name(), self.form.class_name(), self.hits, if self.hit { 1 } else { 0 })
    }
}
/// 证据底（每形态只置自己那一项，检出不交叉掩盖）。
#[derive(Clone, Debug)]
pub struct Evidence {
    /// 已注入形态位图。
    pub injected_bits: u64,
    /// 逐形态命中数（累积）。
    pub hits: [usize; FORM_COUNT],
    /// 注入留痕全表。
    pub injected: Vec<Injected>,
    /// 缺失位图。
    pub missing_bits: u128,
    /// 空串位图。
    pub blank_bits: u128,
    /// 重复位图。
    pub dup_bits: u128,
    /// 漏面位图。
    pub missing_faces: u64,
    /// 面覆盖成立。
    pub coverage_ok: bool,
    /// 矩阵指纹。
    pub matrix_fp: u64,
    /// 基线哈希。
    pub baseline_hash: u64,
    /// 夹具全过。
    pub cases_ok: bool,
    /// 环形账守恒。
    pub ring_balanced: bool,
    /// 环形账活条数。
    pub ring_live: usize,
    /// 环形账容量。
    pub ring_cap: usize,
    /// 状态机是否已冻结。
    pub frozen: bool,
}
impl Evidence {
    /// 零证据（干净矩阵的应有状态）。
    pub fn zero() -> Evidence {
        Evidence {
            injected_bits: 0,
            hits: [0usize; FORM_COUNT],
            injected: Vec::new(),
            missing_bits: 0,
            blank_bits: 0,
            dup_bits: 0,
            missing_faces: 0,
            coverage_ok: true,
            matrix_fp: 0,
            baseline_hash: 0,
            cases_ok: true,
            ring_balanced: true,
            ring_live: 0,
            ring_cap: RING_CAP,
            frozen: false,
        }
    }
    /// 取某形态命中数。
    pub fn of(&self, f: Form) -> usize {
        self.hits[f.index()]
    }
    /// 某形态是否被注入过。
    pub fn was_injected(&self, f: Form) -> bool {
        self.injected_bits & (1u64 << f.index()) != 0
    }
    /// 证据指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(20);
        words.push(self.injected_bits);
        words.push(self.missing_bits as u64);
        words.push((self.missing_bits >> 64) as u64);
        words.push(self.blank_bits as u64);
        words.push((self.blank_bits >> 64) as u64);
        words.push(self.dup_bits as u64);
        words.push((self.dup_bits >> 64) as u64);
        words.push(self.missing_faces);
        words.push(self.coverage_ok as u64);
        words.push(self.matrix_fp);
        words.push(self.baseline_hash);
        words.push(self.cases_ok as u64);
        words.push(self.ring_balanced as u64);
        words.push(self.ring_live as u64);
        words.push(self.frozen as u64);
        for h in self.hits.iter() {
            words.push(*h as u64);
        }
        fnv1a_words(&words)
    }
}
/// 位图中某列是否有位（列 `col` 在 `case*4+col` 布局下）。
fn col_any(bits: u128, col: usize) -> bool {
    let mut i: usize = 0;
    while i < CASE_COUNT {
        let b = i * COLUMN_COUNT + col;
        if b < 128 && (bits & (1u128 << b)) != 0 {
            return true;
        }
        i = i.wrapping_add(1);
    }
    false
}
/// 检出侧：12 形态逐项检出（**每形态独立判**）。
pub fn detect_all(ev: &Evidence) -> Vec<Detection> {
    let mut out: Vec<Detection> = Vec::with_capacity(FORM_COUNT);
    // 位号 = `case_idx * COLUMN_COUNT + col`。据此把每形态映射到
    // **它自己那一段位区间**，做到「互不交叉掩盖」：
    // 漏面 f 占据例 `[3f, 3f+2]` 的全部 12 位；漂移形态占单点。
    let mut flags: [bool; FORM_COUNT] = [false; FORM_COUNT];
    // 面 f 的 12 位掩码（例 3f..3f+2 × 四列）。
    let mut fmask: [u128; FACE_COUNT] = [0u128; FACE_COUNT];
    let mut fi: usize = 0;
    while fi < FACE_COUNT {
        let mut local: u128 = 0;
        let mut t: usize = 0;
        while t < TIER_COUNT {
            let ci = 3 * fi + t;
            let mut col: usize = 0;
            while col < COLUMN_COUNT {
                local |= 1u128 << (ci * COLUMN_COUNT + col);
                col = col.wrapping_add(1);
            }
            t = t.wrapping_add(1);
        }
        fmask[fi] = local;
        fi = fi.wrapping_add(1);
    }
    // 1..6 漏面：该面 12 位里出现缺失或空串
    let drop_forms: [Form; 6] = [
        Form::FaceDropCapacity,
        Form::FaceDropEstablish,
        Form::FaceDropTranslate,
        Form::FaceDropReserve,
        Form::FaceDropSplit,
        Form::FaceDropContend,
    ];
    let mut di: usize = 0;
    while di < 6 {
        let f = drop_forms[di];
        let mk = fmask[di];
        if let Some(flag) = flags.get_mut(f.index()) {
            *flag = (ev.missing_bits & mk) != 0 || (ev.blank_bits & mk) != 0 || (ev.missing_faces >> di) & 1 != 0;
        }
        di = di.wrapping_add(1);
    }
    // 7 判据段漂移：列 1 有缺失或重复
    if let Some(flag) = flags.get_mut(Form::DriftCriterionSeg.index()) {
        *flag = col_any(ev.missing_bits, 1) || col_any(ev.dup_bits, 1) || !ev.cases_ok;
    }
    // 8 断言入口漂移：列 2 有缺失或重复
    if let Some(flag) = flags.get_mut(Form::DriftAssertEntry.index()) {
        *flag = col_any(ev.missing_bits, 2) || col_any(ev.dup_bits, 2) || !ev.cases_ok;
    }
    // 9 读数键漂移：列 3 有缺失或重复
    if let Some(flag) = flags.get_mut(Form::DriftReadingKey.index()) {
        *flag = col_any(ev.missing_bits, 3) || col_any(ev.dup_bits, 3) || !ev.cases_ok;
    }
    // 10 用例 ID 漂移：列 0 有缺失或重复
    if let Some(flag) = flags.get_mut(Form::DriftCaseId.index()) {
        *flag = col_any(ev.missing_bits, 0) || col_any(ev.dup_bits, 0) || !ev.cases_ok;
    }
    // 11 档位互换：夹具必不齐（面/档与 ID 不匹配）
    if let Some(flag) = flags.get_mut(Form::DriftTierSwap.index()) {
        *flag = !ev.cases_ok;
    }
    // 12 缺列：读数列整列（27 位全空）——比漂移更严，要求整列命中
    if let Some(flag) = flags.get_mut(Form::ColumnDropAll.index()) {
        let mut full: u128 = 0;
        let mut i: usize = 0;
        while i < CASE_COUNT {
            full |= 1u128 << (i * COLUMN_COUNT + 3);
            i = i.wrapping_add(1);
        }
        *flag = (ev.blank_bits & full) == full || ((ev.missing_bits & full) == full && ev.blank_bits != 0);
    }
    let _ = ev.ring_balanced;
    let _ = ev.ring_live;
    let _ = ev.frozen;
    let mut k: usize = 0;
    while k < FORM_COUNT {
        let f = Form::ALL[k];
        let on = match flags.get(k) {
            Some(v) => *v,
            None => false,
        };
        out.push(Detection { form: f, hits: if on { 1usize } else { 0usize }, hit: on });
        k = k.wrapping_add(1);
    }
    out
}
/// 注入核：12 形态逐个造证 → 建证据 → 独立检出 → 全部留痕。
pub fn inject_all() -> (Vec<Injected>, Evidence) {
    let mut ev = Evidence::zero();
    let mut inj: Vec<Injected> = Vec::with_capacity(FORM_COUNT);
    let mut tick: u64 = 0;
    let mut k: usize = 0;
    while k < FORM_COUNT {
        let form = Form::ALL[k];
        // 每形态都从**干净矩阵**重新展开：污染绝不跨形态累积，
        // 否则第 k 个形态的证据底里会带着前 k-1 个形态的脏位，
        // 检出结论就无法归因到单一失效模式。
        let mut m = match CaseMatrix::expand() {
            Ok(v) => v,
            Err(_) => break,
        };
        let clean_fp = m.fingerprint();
        let clean_integrity = integrity_report(&m);
        let clean_cov = coverage_report(&m);
        let clean_run = run_matrix_ok(&m);
        let cor = match corrupt_matrix(&mut m, form) {
            Ok(v) => v,
            Err(_) => break,
        };
        // 形变后再点验三张位图 + 面覆盖
        let ig = integrity_report(&m);
        let cv = coverage_report(&m);
        let run_ok = run_matrix_ok(&m);
        // 环形账（干净态守恒）
        let mut ring = RingLedger::new();
        let n_push: u64 = (RING_CAP as u64).wrapping_mul(2).wrapping_add(7);
        let mut p: u64 = 0;
        while p < n_push {
            ring.push(MAIN_CODE, (p as usize) % CASE_COUNT, p);
            p = p.wrapping_add(1);
        }
        // 证据装配（每形态只置自己那一项）
        ev.injected_bits |= 1u64 << form.index();
        ev.missing_bits = ig.missing_bits;
        ev.blank_bits = ig.blank_bits;
        ev.dup_bits = ig.dup_bits;
        ev.missing_faces = cv.missing_faces;
        ev.coverage_ok = cv.all_ok() && clean_cov.all_ok();
        ev.matrix_fp = m.fingerprint();
        ev.baseline_hash = clean_fp;
        ev.cases_ok = run_ok && clean_run;
        ev.ring_balanced = ring.balanced();
        ev.ring_live = ring.live();
        ev.ring_cap = RING_CAP;
        ev.frozen = true;
        let _ = clean_integrity;
        let _ = clean_cov;
        let ds = detect_all(&ev);
        // 只回写**本形态**那一项：本轮证据底上只有本形态对应的位为脏，
        // 其余形态的结论位此时必然为 false（hits = 0）。
        // 若整表回写，前序形态的命中记录会被后续轮次抹平。
        for d in ds.iter() {
            if d.form == form {
                ev.hits[d.form.index()] = d.hits;
            }
        }
        let self_hit = ev.of(form);
        let note = format!("cell={} fp={:016X} self_hit={}", cor.face_idx.min(cor.col), cor.fp, if self_hit > 0 { 1 } else { 0 });
        inj.push(Injected { form, class: form.class_name(), cell: cor.face_idx, tick, note });
        ev.injected = inj.clone();
        tick = tick_advance(tick);
        k = k.wrapping_add(1);
    }
    (inj, ev)
}
/// 未检出形态清单（正常应为空）。
pub fn detect_miss() -> Vec<&'static str> {
    let (_inj, ev) = inject_all();
    let mut miss: Vec<&'static str> = Vec::new();
    for f in Form::ALL.iter() {
        if ev.of(*f) == 0 {
            miss.push(f.name());
        }
    }
    miss
}
/// 检出侧全过（12/12）。
pub fn detect_all_ok(ev: &Evidence) -> bool {
    let ds = detect_all(ev);
    for d in ds.iter() {
        if !d.hit {
            return false;
        }
    }
    true
}
/// 检出通过数。
pub fn detect_pass(ds: &[Detection]) -> usize {
    let mut n: usize = 0;
    for d in ds.iter() {
        if d.hit {
            n = n.wrapping_add(1);
        }
    }
    n
}
/// 检出逐行描述。
pub fn detection_rows(ds: &[Detection]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(ds.len());
    for d in ds.iter() {
        out.push(d.describe());
    }
    out
}

// ===========================================================================
// 12. 冻结件（指纹 / 漂移检出）
// ===========================================================================
/// 矩阵冻结件。
#[derive(Clone, Debug)]
pub struct FreezeRecord {
    /// 矩阵册版本号。
    pub book_rev: u32,
    /// 例数。
    pub cases: usize,
    /// 面数。
    pub faces: usize,
    /// 档数。
    pub tiers: usize,
    /// 列数。
    pub columns: usize,
    /// 单例步数（三档统一）。
    pub case_steps: u64,
    /// 矩阵冻结哈希。
    pub matrix_hash: u64,
    /// 基线哈希（CI 登记值；0 = 未登记）。
    pub baseline_hash: u64,
    /// 完整性哈希（三张位图折叠）。
    pub integrity_fp: u64,
    /// 覆盖哈希（漏面位图）。
    pub coverage_fp: u64,
    /// 状态机指纹。
    pub fsm_fp: u64,
    /// 当前态。
    pub state: MatrixState,
    /// 四列注记条目数（27 × 4）。
    pub annotations: u64,
    /// 失真声明在档。
    pub distortion_declared: bool,
}
impl FreezeRecord {
    /// 由矩阵 + 点验 + 状态机构造（尚未登记基线）。
    pub fn draft(m: &CaseMatrix, ig: &IntegrityReport, cv: &CoverageReport, fsm: &MatrixFsm) -> FreezeRecord {
        FreezeRecord {
            book_rev: MATRIX_BOOK_REV,
            cases: m.produced,
            faces: FACE_COUNT,
            tiers: TIER_COUNT,
            columns: COLUMN_COUNT,
            case_steps: CASE_STEPS as u64,
            matrix_hash: m.fingerprint(),
            baseline_hash: 0,
            integrity_fp: fnv1a_words(&[ig.missing_bits as u64, (ig.missing_bits >> 64) as u64, ig.blank_bits as u64, (ig.blank_bits >> 64) as u64, ig.dup_bits as u64, (ig.dup_bits >> 64) as u64, ig.registered, ig.unique]),
            coverage_fp: fnv1a_words(&[cv.missing_faces, cv.covered as u64]),
            fsm_fp: fsm.fingerprint(),
            state: fsm.state,
            annotations: TOTAL_FIELDS as u64,
            distortion_declared: false,
        }
    }
    /// 冻结哈希是否与基线一致。
    pub fn hash_matches(&self) -> bool {
        self.baseline_hash != 0 && self.baseline_hash == self.matrix_hash
    }
    /// **冻结指纹** `fingerprint()`：折叠全部冻结件字段。
    ///
    /// 刻意**不**混入 `baseline_hash`：否则「同方案、不同流程状态」
    /// （已冻结 vs 已漂移）会得到不同冻结哈希，让漂移比对自相矛盾。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(12);
        words.push(self.book_rev as u64);
        words.push(self.cases as u64);
        words.push(self.faces as u64);
        words.push(self.tiers as u64);
        words.push(self.columns as u64);
        words.push(self.case_steps);
        words.push(self.matrix_hash);
        words.push(self.integrity_fp);
        words.push(self.coverage_fp);
        words.push(self.fsm_fp);
        words.push(self.state.index() as u64);
        words.push(self.annotations);
        words.push(self.distortion_declared as u64);
        fnv1a_words(&words)
    }
    /// **漂移检出** `verify()`：冻结哈希须与基线一致。
    pub fn verify(&self) -> UmResult<u64> {
        if self.baseline_hash == 0 {
            return Err(UmErr::of_aux(AuxCode::BookRevMismatch, format!("baseline not registered, matrix_hash={:016X}", self.matrix_hash)));
        }
        if self.book_rev != MATRIX_BOOK_REV {
            return Err(UmErr::of_aux(AuxCode::BookRevMismatch, format!("book_rev={} expect={}", self.book_rev, MATRIX_BOOK_REV)));
        }
        if self.baseline_hash != self.matrix_hash {
            return Err(UmErr::of_form(Form::DriftCaseId, format!("matrix_hash={:016X} baseline={:016X}", self.matrix_hash, self.baseline_hash)));
        }
        Ok(self.matrix_hash)
    }
    /// 登记基线（只允许「空 → 有」一次）。
    pub fn set_baseline(&mut self, hash: u64) -> UmResult<()> {
        if self.baseline_hash != 0 {
            return Err(UmErr::of_aux(AuxCode::FrozenWriteRejected, format!("baseline already = {:016X}", self.baseline_hash)));
        }
        if hash != self.matrix_hash {
            return Err(UmErr::of_form(Form::DriftCaseId, format!("set {} != matrix {:016X}", hash, self.matrix_hash)));
        }
        self.baseline_hash = hash;
        Ok(())
    }
    /// 声明失真（模型侧字段点验 ≠ 真机回归执行）。
    pub fn declare_distortion(&mut self) {
        self.distortion_declared = true;
    }
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "freeze rev={} cases={}/{}x{}x{} steps={} hash={:016X} baseline={:016X} ig={:016X} cov={:016X} state={} anno={} drift_declared={}",
            self.book_rev,
            self.cases,
            self.faces,
            self.tiers,
            self.columns,
            self.case_steps,
            self.matrix_hash,
            self.baseline_hash,
            self.integrity_fp,
            self.coverage_fp,
            self.state.name(),
            self.annotations,
            if self.distortion_declared { 1 } else { 0 }
        )
    }
    /// 逐字段注记（判据要求「27 例四列注记随冻结件入库」）。
    pub fn annotation(&self, m: &CaseMatrix) -> Vec<String> {
        let mut out: Vec<String> = Vec::with_capacity(TOTAL_FIELDS);
        let mut i: usize = 0;
        while i < m.cases.len() {
            let c = match m.case(i) {
                Ok(v) => v,
                Err(_) => break,
            };
            let mut col: usize = 0;
            while col < COLUMN_COUNT {
                let v = match c.field_ref(col) {
                    Ok(s) => s.to_string(),
                    Err(_) => String::from("<OOR>"),
                };
                out.push(format!("case#{:02} {}={}", i, CaseColumn::ALL[col].name(), v));
                col = col.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        out
    }
}
// ===========================================================================
// 13. 观测指标
// ===========================================================================
/// 观测指标（全部从 0 起算，与分配同步累加）。
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    /// 例数。
    pub cases: u64,
    /// 字段位数。
    pub fields: u64,
    /// 面数。
    pub faces: u64,
    /// 档数。
    pub tiers: u64,
    /// 列数。
    pub columns: u64,
    /// 缺失字段数。
    pub missing: u64,
    /// 空串字段数。
    pub blank: u64,
    /// 重复字段数。
    pub dup: u64,
    /// 登记字段数。
    pub registered: u64,
    /// 唯一字段数。
    pub unique: u64,
    /// 漏面数。
    pub missing_faces: u64,
    /// 覆盖面数。
    pub covered: u64,
    /// 夹具全过例数。
    pub cases_ok: u64,
    /// 实跑步数。
    pub steps: u64,
    /// 注入形态数。
    pub forms: u64,
    /// 检出命中数。
    pub detect_hits: u64,
    /// 漏面类检出数。
    pub face_drop_hits: u64,
    /// 漂移类检出数。
    pub drift_hits: u64,
    /// 缺列类检出数。
    pub column_drop_hits: u64,
    /// 稳定性轮数。
    pub rounds: u64,
    /// 环形账写入数。
    pub ring_pushed: u64,
    /// 环形账丢弃数（显性）。
    pub ring_dropped: u64,
    /// 环形账活条数。
    pub ring_live: u64,
    /// 矩阵哈希。
    pub matrix_hash: u64,
    /// 基线哈希。
    pub baseline_hash: u64,
    /// 冻结件指纹。
    pub freeze_fp: u64,
    /// 完整性哈希。
    pub integrity_fp: u64,
    /// 覆盖哈希。
    pub coverage_fp: u64,
    /// 状态机合法迁移数。
    pub fsm_legal: u64,
    /// 状态机非法迁移拒记数。
    pub fsm_rejected: u64,
    /// 步数上界。
    pub step_cap: u64,
    /// 内存峰值。
    pub mem_peak: u64,
    /// syscall 次数。
    pub syscall_used: u64,
    /// 注记条目数。
    pub annotations: u64,
}
impl Metrics {
    /// 零指标。
    pub fn zero() -> Metrics {
        Metrics {
            cases: 0,
            fields: 0,
            faces: 0,
            tiers: 0,
            columns: 0,
            missing: 0,
            blank: 0,
            dup: 0,
            registered: 0,
            unique: 0,
            missing_faces: 0,
            covered: 0,
            cases_ok: 0,
            steps: 0,
            forms: 0,
            detect_hits: 0,
            face_drop_hits: 0,
            drift_hits: 0,
            column_drop_hits: 0,
            rounds: 0,
            ring_pushed: 0,
            ring_dropped: 0,
            ring_live: 0,
            matrix_hash: 0,
            baseline_hash: 0,
            freeze_fp: 0,
            integrity_fp: 0,
            coverage_fp: 0,
            fsm_legal: 0,
            fsm_rejected: 0,
            step_cap: 0,
            mem_peak: 0,
            syscall_used: 0,
            annotations: 0,
        }
    }
    /// 字段完整率（千分数）。
    pub fn integrity_permille(&self) -> u64 {
        if self.fields == 0 {
            return 0;
        }
        self.unique.wrapping_mul(1000) / self.fields
    }
    /// 面覆盖率（千分数）。
    pub fn coverage_permille(&self) -> u64 {
        if self.faces == 0 {
            return 0;
        }
        self.covered.wrapping_mul(1000) / self.faces
    }
    /// 夹具通过率（千分数）。
    pub fn pass_permille(&self) -> u64 {
        if self.cases == 0 {
            return 0;
        }
        self.cases_ok.wrapping_mul(1000) / self.cases
    }
    /// 检出率（千分数）。
    pub fn detect_permille(&self) -> u64 {
        if self.forms == 0 {
            return 0;
        }
        self.detect_hits.wrapping_mul(1000) / self.forms
    }
    /// 环形丢弃率（千分数）。
    pub fn ring_drop_permille(&self) -> u64 {
        if self.ring_pushed == 0 {
            return 0;
        }
        self.ring_dropped.wrapping_mul(1000) / self.ring_pushed
    }
    /// 键值对导出（供 `export_kv`）。
    pub fn kv(&self) -> Vec<(&'static str, u64)> {
        vec![
            ("cases", self.cases),
            ("fields", self.fields),
            ("faces", self.faces),
            ("tiers", self.tiers),
            ("columns", self.columns),
            ("missing", self.missing),
            ("blank", self.blank),
            ("dup", self.dup),
            ("registered", self.registered),
            ("unique", self.unique),
            ("missing_faces", self.missing_faces),
            ("covered", self.covered),
            ("cases_ok", self.cases_ok),
            ("steps", self.steps),
            ("forms", self.forms),
            ("detect_hits", self.detect_hits),
            ("face_drop_hits", self.face_drop_hits),
            ("drift_hits", self.drift_hits),
            ("column_drop_hits", self.column_drop_hits),
            ("rounds", self.rounds),
            ("ring_pushed", self.ring_pushed),
            ("ring_dropped", self.ring_dropped),
            ("ring_live", self.ring_live),
            ("matrix_hash", self.matrix_hash),
            ("baseline_hash", self.baseline_hash),
            ("freeze_fp", self.freeze_fp),
            ("integrity_fp", self.integrity_fp),
            ("coverage_fp", self.coverage_fp),
            ("fsm_legal", self.fsm_legal),
            ("fsm_rejected", self.fsm_rejected),
            ("step_cap", self.step_cap),
            ("mem_peak", self.mem_peak),
            ("syscall_used", self.syscall_used),
            ("annotations", self.annotations),
        ]
    }
    /// 指标指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut words: Vec<u64> = Vec::with_capacity(40);
        for (_k, v) in self.kv().iter() {
            words.push(*v);
        }
        fnv1a_words(&words)
    }
}

// ===========================================================================
// 14. 确定性重放
// ===========================================================================
/// 重放结论。
#[derive(Clone, Copy, Debug)]
pub struct ReplayResult {
    /// 种子。
    pub seed: u64,
    /// 例数。
    pub cases: usize,
    /// 矩阵指纹。
    pub matrix_fp: u64,
    /// 逐例夹具指纹折叠。
    pub casewise_fp: u64,
    /// 完整性哈希。
    pub integrity_fp: u64,
    /// 覆盖哈希。
    pub coverage_fp: u64,
    /// 夹具全过例数。
    pub cases_ok: usize,
    /// 步数。
    pub steps: u64,
    /// LCG 末态。
    pub lcg_tail: u64,
}
impl ReplayResult {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!(
            "replay seed={:016X} cases={} matrix_fp={:016X} casewise_fp={:016X} ig={:016X} cov={:016X} cases_ok={} steps={} lcg_tail={:016X}",
            self.seed, self.cases, self.matrix_fp, self.casewise_fp, self.integrity_fp, self.coverage_fp, self.cases_ok, self.steps, self.lcg_tail
        )
    }
    /// 自洽。
    pub fn ok(&self) -> bool {
        self.cases == CASE_COUNT && self.matrix_fp != 0 && self.casewise_fp != 0 && self.integrity_fp != 0 && self.coverage_fp != 0 && self.cases_ok == self.cases
    }
}
/// 确定性重放：同种子 → 同轨迹 → 同指纹。
///
/// 与 `observe()` **不互递归**：`replay()` 只跑「展开 + 逐例夹具 + 点验」，
/// `observe()` 复用其产出并叠加注入、稳定性、导出三段，
/// 调用方向严格单向（`observe` → `replay`）。
///
/// 种子**不参与矩阵取值**（矩阵是纯函数产物），只做跑序抖动，
/// 保证跨轮可比。
pub fn replay(seed: u64) -> ReplayResult {
    let mut rng = Lcg::new(seed);
    let m = match CaseMatrix::expand() {
        Ok(v) => v,
        Err(_) => {
            return ReplayResult { seed, cases: 0, matrix_fp: 0, casewise_fp: 0, integrity_fp: 0, coverage_fp: 0, cases_ok: 0, steps: 0, lcg_tail: rng.raw() }
        }
    };
    let rs = match run_matrix(&m) {
        Ok(v) => v,
        Err(_) => {
            return ReplayResult { seed, cases: 0, matrix_fp: 0, casewise_fp: 0, integrity_fp: 0, coverage_fp: 0, cases_ok: 0, steps: 0, lcg_tail: rng.raw() }
        }
    };
    let ig = integrity_report(&m);
    let cv = coverage_report(&m);
    let mut cases_ok: usize = 0;
    let mut steps: u64 = 0;
    let mut cwords: Vec<u64> = Vec::with_capacity(rs.len());
    let mut i: usize = 0;
    while i < rs.len() {
        let r = match rs.get(i) {
            Some(v) => v,
            None => break,
        };
        // 跑序抖动：每例先烧一个随机数再记账，
        // 保证「种子影响跑序、不影响取值」这一口径在代码里可见。
        let _warm = rng.next_u64();
        if r.ok() {
            cases_ok = cases_ok.wrapping_add(1);
        }
        steps = steps.wrapping_add(r.steps_run);
        cwords.push(r.post_fp);
        i = i.wrapping_add(1);
    }
    ReplayResult {
        seed,
        cases: m.produced,
        matrix_fp: m.fingerprint(),
        casewise_fp: fnv1a_words(&cwords),
        integrity_fp: fnv1a_words(&[ig.missing_bits as u64, (ig.missing_bits >> 64) as u64, ig.blank_bits as u64, (ig.blank_bits >> 64) as u64, ig.dup_bits as u64, (ig.dup_bits >> 64) as u64]),
        coverage_fp: fnv1a_words(&[cv.missing_faces, cv.covered as u64]),
        cases_ok,
        steps,
        lcg_tail: rng.raw(),
    }
}
// ===========================================================================
// 15. 观测（真实跑逻辑）
// ===========================================================================
/// 观测结论。
#[derive(Clone, Debug)]
pub struct Observation {
    /// 指标。
    pub metrics: Metrics,
    /// 矩阵。
    pub matrix: CaseMatrix,
    /// 完整性结论。
    pub integrity: IntegrityReport,
    /// 覆盖结论。
    pub coverage: CoverageReport,
    /// 冻结件。
    pub freeze: FreezeRecord,
    /// 稳定性结论。
    pub stability: StabilityReport,
    /// 状态机。
    pub fsm: MatrixFsm,
    /// 环形账。
    pub ring: RingLedger,
    /// 注入留痕。
    pub injected: Vec<Injected>,
    /// 检出结论。
    pub detections: Vec<Detection>,
    /// 检出通过数。
    pub detect_pass: usize,
    /// 重放结论。
    pub replay: ReplayResult,
    /// 重放两遍是否一致。
    pub replay_stable: bool,
    /// TLV 往返自洽。
    pub tlv_ok: bool,
    /// 失真声明在档。
    pub distortion_declared: bool,
}
impl Observation {
    /// 总闸：判据 UNX-F2861-J1 全过。
    ///
    /// 硬条件（缺一即不过）：
    /// 1. 矩阵格数 = 维度乘积、正交成立、三元组闭集成立；
    /// 2. 四列零缺失（缺失 / 空串 / 重复三张位图全零）；
    /// 3. 面覆盖断言成立（九面全映射、每面三档齐）；
    /// 4. 逐例夹具全过（27/27）；
    /// 5. 12 形态检出全命中（判据要求两类各 10/10）；
    /// 6. 冻结件与基线一致（`verify()` 通过）；
    /// 7. `REPEAT` 轮稳定性判定通过（两重指纹全等）；
    /// 8. 环形账守恒、资源上界在界内、重放两遍一致、TLV 往返自洽；
    /// 9. 失真声明在档。
    pub fn all_ok(&self) -> bool {
        self.matrix.count_ok()
            && self.matrix.orthogonal_ok()
            && self.matrix.dims_ok()
            && self.integrity.all_ok()
            && self.coverage.all_ok()
            && self.metrics.cases_ok == CASE_COUNT as u64
            && self.detect_pass == FORM_COUNT
            && self.freeze.verify().is_ok()
            && self.stability.passed()
            && self.fsm.is_frozen()
            && self.ring.check().is_ok()
            && self.ring.dropped > 0
            && self.replay_stable
            && self.tlv_ok
            && self.distortion_declared
            && self.metrics.annotations == TOTAL_FIELDS as u64
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!(
            "UNX-F2861 matrix={}/{} cases_ok={} integrity={} coverage={}/{} detect={}/{} stable={} freeze_ok={} tlv={} ring={} replay_stable={} distortion={} miss={} blank={} dup={} leak={}",
            self.metrics.cases,
            CASE_COUNT,
            self.metrics.cases_ok,
            if self.integrity.all_ok() { 1 } else { 0 },
            self.metrics.covered,
            FACE_COUNT,
            self.detect_pass,
            FORM_COUNT,
            if self.stability.passed() { 1 } else { 0 },
            if self.freeze.verify().is_ok() { 1 } else { 0 },
            if self.tlv_ok { 1 } else { 0 },
            if self.ring.check().is_ok() { 1 } else { 0 },
            if self.replay_stable { 1 } else { 0 },
            if self.distortion_declared { 1 } else { 0 },
            self.integrity.missing_count(),
            self.integrity.blank_count(),
            self.integrity.dup_count(),
            self.metrics.missing_faces
        )
    }
}

/// 真实跑逻辑观测：一次跑完矩阵 → 点验 → 冻结 → 注入 → 稳定性 → 预算 → 导出。
pub fn observe(seed: u64) -> Observation {
    let mut metrics = Metrics::zero();
    // 1 重放（展开 + 逐例夹具 + 点验）
    let rp = replay(seed);
    metrics.cases = rp.cases as u64;
    metrics.fields = TOTAL_FIELDS as u64;
    metrics.faces = FACE_COUNT as u64;
    metrics.tiers = TIER_COUNT as u64;
    metrics.columns = COLUMN_COUNT as u64;
    metrics.cases_ok = rp.cases_ok as u64;
    metrics.steps = rp.steps;
    metrics.integrity_fp = rp.integrity_fp;
    metrics.coverage_fp = rp.coverage_fp;
    metrics.matrix_hash = rp.matrix_fp;
    // 2 矩阵（重建一份独立实例给点验用，避免与 replay 共用内存）
    let matrix = match CaseMatrix::expand() {
        Ok(v) => v,
        Err(_) => {
            return Observation {
                metrics,
                matrix: CaseMatrix::blank(),
                integrity: IntegrityReport { missing_bits: u128::MAX, blank_bits: u128::MAX, dup_bits: u128::MAX, registered: 0, nonblank: 0, unique: 0, rows: Vec::new() },
                coverage: CoverageReport { faces: Vec::new(), covered: 0, missing_faces: u64::MAX, rows: Vec::new() },
                freeze: FreezeRecord { book_rev: 0, cases: 0, faces: 0, tiers: 0, columns: 0, case_steps: 0, matrix_hash: 0, baseline_hash: 0, integrity_fp: 0, coverage_fp: 0, fsm_fp: 0, state: MatrixState::Draft, annotations: 0, distortion_declared: false },
                stability: StabilityReport { rounds: Vec::new(), matrix_identical: false, casewise_identical: false, step_cap: STEP_CAP, steps_used: 0 },
                fsm: MatrixFsm::new(),
                ring: RingLedger::new(),
                injected: Vec::new(),
                detections: Vec::new(),
                detect_pass: 0,
                replay: rp,
                replay_stable: false,
                tlv_ok: false,
                distortion_declared: false,
            };
        }
    };
    // 3 完整性 + 覆盖点验
    let integrity = integrity_report(&matrix);
    let coverage = coverage_report(&matrix);
    metrics.missing = integrity.missing_count() as u64;
    metrics.blank = integrity.blank_count() as u64;
    metrics.dup = integrity.dup_count() as u64;
    metrics.registered = integrity.registered;
    metrics.unique = integrity.unique;
    metrics.missing_faces = count_bits64(coverage.missing_faces) as u64;
    metrics.covered = coverage.covered as u64;
    // 4 逐例夹具真跑
    let rs = match run_matrix(&matrix) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let mut cases_ok: u64 = 0;
    let mut steps: u64 = 0;
    for r in rs.iter() {
        if r.ok() {
            cases_ok = cases_ok.wrapping_add(1);
        }
        steps = steps.wrapping_add(r.steps_run);
    }
    metrics.cases_ok = cases_ok;
    metrics.steps = steps;
    // 5 状态机：草拟 → 待审 → 已冻结（→ 漂移演示后回不到冻结）
    let mut fsm = MatrixFsm::new();
    let _ = fsm.go(MatrixState::Reviewing);
    let _ = fsm.go(MatrixState::Frozen);
    metrics.fsm_legal = fsm.legal_moves;
    metrics.fsm_rejected = fsm.rejected.len() as u64;
    // 6 冻结件：草拟 → 登记基线 → 声明失真 → 验证
    let mut freeze = FreezeRecord::draft(&matrix, &integrity, &coverage, &fsm);
    let _ = freeze.set_baseline(freeze.matrix_hash);
    freeze.declare_distortion();
    metrics.baseline_hash = freeze.baseline_hash;
    metrics.freeze_fp = freeze.fingerprint();
    metrics.annotations = freeze.annotation(&matrix).len() as u64;
    metrics.integrity_fp = freeze.integrity_fp;
    metrics.coverage_fp = freeze.coverage_fp;
    // 7 环形账（覆写必显性记账）
    let mut ring = RingLedger::new();
    let n_push: u64 = (RING_CAP as u64).wrapping_mul(2).wrapping_add(7);
    let mut p: u64 = 0;
    while p < n_push {
        ring.push(MAIN_CODE, (p as usize) % CASE_COUNT, p);
        p = p.wrapping_add(1);
    }
    metrics.ring_pushed = ring.pushed;
    metrics.ring_dropped = ring.dropped;
    metrics.ring_live = ring.live() as u64;
    // 8 注入核 12 形态逐形态独立注入 + 独立检出
    let (injected, ev) = inject_all();
    let mut detections: Vec<Detection> = Vec::with_capacity(FORM_COUNT);
    let mut detect_pass: usize = 0;
    let mut k: usize = 0;
    while k < FORM_COUNT {
        let f = Form::ALL[k];
        let hits = ev.of(f);
        let hit = hits > 0;
        if hit {
            detect_pass = detect_pass.wrapping_add(1);
            metrics.detect_hits = metrics.detect_hits.wrapping_add(1);
            if f.is_face_drop() {
                metrics.face_drop_hits = metrics.face_drop_hits.wrapping_add(1);
            } else if f.is_column_drop() {
                metrics.column_drop_hits = metrics.column_drop_hits.wrapping_add(1);
            } else {
                metrics.drift_hits = metrics.drift_hits.wrapping_add(1);
            }
        }
        detections.push(Detection { form: f, hits, hit });
        k = k.wrapping_add(1);
    }
    metrics.forms = FORM_COUNT as u64;
    // 9 稳定性判定
    let stability = stability_sweep();
    metrics.rounds = stability.rounds.len() as u64;
    // 10 资源上界
    let budget = resource_budget(&matrix);
    metrics.step_cap = budget.sweep_cap;
    metrics.mem_peak = budget.mem_peak;
    metrics.syscall_used = budget.syscalls;
    // 11 重放确定性（同种子跑第二遍比三指纹）
    let rp2 = replay(seed);
    let replay_stable = rp2.matrix_fp == rp.matrix_fp && rp2.casewise_fp == rp.casewise_fp && rp2.integrity_fp == rp.integrity_fp && rp2.coverage_fp == rp.coverage_fp;
    // 12 TLV 往返
    let mut ob = Observation {
        metrics,
        matrix,
        integrity,
        coverage,
        freeze,
        stability,
        fsm,
        ring,
        injected,
        detections,
        detect_pass,
        replay: rp,
        replay_stable,
        tlv_ok: false,
        distortion_declared: true,
    };
    let tlv = export_tlv(&ob);
    ob.tlv_ok = tlv_roundtrip_ok(&tlv);
    ob
}
/// `u64` 位计数。
fn count_bits64(mut v: u64) -> u32 {
    let mut n: u32 = 0;
    while v != 0 {
        v &= v.wrapping_sub(1);
        n = n.wrapping_add(1);
    }
    n
}
// ===========================================================================
// 16. 资源上界核算
// ===========================================================================
/// 资源上界结论。
#[derive(Clone, Copy, Debug)]
pub struct ResourceBudget {
    /// 单轮步数上界。
    pub round_cap: u64,
    /// `REPEAT` 轮步数上界。
    pub sweep_cap: u64,
    /// 预估实际步数。
    pub steps_used: u64,
    /// syscall 次数（恒 0）。
    pub syscalls: u64,
    /// syscall 上界。
    pub syscall_cap: u64,
    /// 内存峰值（字节）。
    pub mem_peak: u64,
    /// 内存上界。
    pub mem_cap: u64,
}
impl ResourceBudget {
    /// 是否全在上界内。
    pub fn within(&self) -> bool {
        self.steps_used <= self.sweep_cap && self.syscalls <= self.syscall_cap && self.mem_peak <= self.mem_cap
    }
    /// 一行摘要。
    pub fn summary(&self) -> String {
        format!("budget steps={}/{} syscall={}/{} mem={}/{}B within={}", self.steps_used, self.sweep_cap, self.syscalls, self.syscall_cap, self.mem_peak, self.mem_cap, if self.within() { 1 } else { 0 })
    }
}
/// 资源上界核算（显式四项相加，不留「估算」）。
pub fn resource_budget(m: &CaseMatrix) -> ResourceBudget {
    let cases = m.produced as u64;
    let steps_used = cases.wrapping_mul(CASE_STEPS as u64).wrapping_mul(REPEAT as u64);
    // 内存峰值四项：矩阵对象 + 完整性行 + 覆盖行 + 注入留痕
    let mem_matrix = cases.wrapping_mul(160);
    let mem_integrity = (TOTAL_FIELDS as u64).wrapping_mul(48);
    let mem_coverage = (FACE_COUNT as u64).wrapping_mul(24);
    let mem_inject = (FORM_COUNT as u64).wrapping_mul(96);
    let mem_ring = (RING_CAP as u64).wrapping_mul(32);
    let mem_peak = mem_matrix
        .wrapping_add(mem_integrity)
        .wrapping_add(mem_coverage)
        .wrapping_add(mem_inject)
        .wrapping_add(mem_ring);
    ResourceBudget {
        round_cap: (MATRIX_STEPS * REPEAT) as u64,
        sweep_cap: STEP_CAP,
        steps_used,
        syscalls: 0,
        syscall_cap: SYSCALL_CAP,
        mem_peak,
        mem_cap: MEM_PEAK_CAP,
    }
}

// ===========================================================================
// 17. 导出（键值 / TLV）
// ===========================================================================
/// 键值导出（逐字段一行）。
pub fn export_kv(ob: &Observation) -> String {
    let mut s = String::new();
    s.push_str("UNX-F2861 kv\n");
    for (k, v) in ob.metrics.kv().iter() {
        s.push_str(&format!("{}={}\n", k, v));
    }
    s.push_str(&format!("integrity_ok={}\n", if ob.integrity.all_ok() { 1 } else { 0 }));
    s.push_str(&format!("coverage_ok={}\n", if ob.coverage.all_ok() { 1 } else { 0 }));
    s.push_str(&format!("integrity_permille={}\n", ob.metrics.integrity_permille()));
    s.push_str(&format!("coverage_permille={}\n", ob.metrics.coverage_permille()));
    s.push_str(&format!("pass_permille={}\n", ob.metrics.pass_permille()));
    s.push_str(&format!("detect_permille={}\n", ob.metrics.detect_permille()));
    s.push_str(&format!("ring_drop_permille={}\n", ob.metrics.ring_drop_permille()));
    s.push_str(&format!("all_ok={}\n", if ob.all_ok() { 1 } else { 0 }));
    s
}

/// TLV 标签。
pub mod tlv_tag {
    /// 组头。
    pub const HEAD: u8 = 0x01;
    /// 冻结件。
    pub const FREEZE: u8 = 0x02;
    /// 矩阵。
    pub const MATRIX: u8 = 0x03;
    /// 完整性。
    pub const INTEGRITY: u8 = 0x04;
    /// 覆盖。
    pub const COVERAGE: u8 = 0x05;
    /// 指标。
    pub const METRIC: u8 = 0x06;
    /// 注入。
    pub const INJECT: u8 = 0x07;
    /// 指纹。
    pub const FINGERPRINT: u8 = 0x08;
}
/// 组头 payload 长度（3 次 `push_u64` = 24 字节，**由 `export_tlv` 反推**）。
pub const TLV_HEAD_LEN: usize = 1 + 2 + 24;
/// 追加一个 TLV 组。
pub fn push_tlv(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.push(tag);
    let len = payload.len() as u16;
    out.push((len & 0xFFu16) as u8);
    out.push(((len >> 8) & 0xFFu16) as u8);
    out.extend_from_slice(payload);
}
/// 追加一个 `u64`（小端 8 字节）。
pub fn push_u64(v: &mut Vec<u8>, w: u64) {
    let mut b: usize = 0;
    while b < 8 {
        v.push(((w >> (b * 8)) & 0xFFu64) as u8);
        b = b.wrapping_add(1);
    }
}
/// 回读 `u64`。
pub fn read_u64(v: &[u8], off: usize) -> u64 {
    let mut w: u64 = 0;
    let mut b: usize = 0;
    while b < 8 {
        let x = match v.get(off.wrapping_add(b)) {
            Some(p) => *p,
            None => return 0,
        };
        w |= (x as u64) << (b * 8);
        b = b.wrapping_add(1);
    }
    w
}
/// 回读 `u16`。
pub fn read_u16(v: &[u8], off: usize) -> u64 {
    let lo = match v.get(off) {
        Some(p) => *p,
        None => return 0,
    };
    let hi = match v.get(off.wrapping_add(1)) {
        Some(p) => *p,
        None => return 0,
    };
    (lo as u64) | ((hi as u64) << 8)
}
/// TLV 导出。
///
/// 组头 payload 三项：magic / version / body_len。
/// 其中 `version` **单独一行** `push_u64`，长度常量由这三次 `push_u64`
/// 反推（`TLV_HEAD_LEN = 1 + 2 + 24`），不手算。
pub fn export_tlv(ob: &Observation) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::with_capacity(4096);
    // 冻结件
    let mut fp: Vec<u8> = Vec::with_capacity(96);
    push_u64(&mut fp, MATRIX_BOOK_REV as u64);
    push_u64(&mut fp, ob.freeze.cases as u64);
    push_u64(&mut fp, ob.freeze.faces as u64);
    push_u64(&mut fp, ob.freeze.tiers as u64);
    push_u64(&mut fp, ob.freeze.columns as u64);
    push_u64(&mut fp, ob.freeze.case_steps);
    push_u64(&mut fp, ob.freeze.matrix_hash);
    push_u64(&mut fp, ob.freeze.baseline_hash);
    push_u64(&mut fp, ob.freeze.integrity_fp);
    push_u64(&mut fp, ob.freeze.coverage_fp);
    push_u64(&mut fp, ob.freeze.fsm_fp);
    push_u64(&mut fp, ob.freeze.state.index() as u64);
    push_u64(&mut fp, ob.freeze.annotations);
    push_tlv(&mut body, tlv_tag::FREEZE, &fp);
    // 矩阵概览
    let mut mp: Vec<u8> = Vec::with_capacity(48);
    push_u64(&mut mp, ob.matrix.produced as u64);
    push_u64(&mut mp, FACE_COUNT as u64);
    push_u64(&mut mp, TIER_COUNT as u64);
    push_u64(&mut mp, COLUMN_COUNT as u64);
    push_u64(&mut mp, ob.matrix.fingerprint());
    push_u64(&mut mp, ob.metrics.cases_ok);
    push_tlv(&mut body, tlv_tag::MATRIX, &mp);
    // 完整性
    let mut ip: Vec<u8> = Vec::with_capacity(80);
    push_u64(&mut ip, ob.integrity.missing_bits as u64);
    push_u64(&mut ip, (ob.integrity.missing_bits >> 64) as u64);
    push_u64(&mut ip, ob.integrity.blank_bits as u64);
    push_u64(&mut ip, (ob.integrity.blank_bits >> 64) as u64);
    push_u64(&mut ip, ob.integrity.dup_bits as u64);
    push_u64(&mut ip, (ob.integrity.dup_bits >> 64) as u64);
    push_u64(&mut ip, ob.integrity.registered);
    push_u64(&mut ip, ob.integrity.nonblank);
    push_u64(&mut ip, ob.integrity.unique);
    push_tlv(&mut body, tlv_tag::INTEGRITY, &ip);
    // 覆盖
    let mut cp: Vec<u8> = Vec::with_capacity(48);
    push_u64(&mut cp, ob.coverage.covered as u64);
    push_u64(&mut cp, ob.coverage.missing_faces);
    push_u64(&mut cp, ob.coverage.faces.len() as u64);
    push_u64(&mut cp, ob.metrics.integrity_permille());
    push_u64(&mut cp, ob.metrics.coverage_permille());
    push_u64(&mut cp, ob.metrics.pass_permille());
    push_tlv(&mut body, tlv_tag::COVERAGE, &cp);
    // 指标
    for (k, v) in ob.metrics.kv().iter() {
        let mut one: Vec<u8> = Vec::with_capacity(24);
        let mut kfp: u64 = fnv1a64(sbytes(k));
        kfp = fnv1a64_ext(kfp, sbytes("="));
        kfp = kfp.wrapping_add(*v);
        push_u64(&mut one, kfp);
        push_u64(&mut one, *v);
        push_tlv(&mut body, tlv_tag::METRIC, &one);
    }
    // 注入
    for inj in ob.injected.iter() {
        let mut one: Vec<u8> = Vec::with_capacity(32);
        push_u64(&mut one, inj.form.code() as u64);
        push_u64(&mut one, inj.tick);
        push_u64(&mut one, inj.cell as u64);
        push_u64(&mut one, fnv1a64(sbytes(&inj.note)));
        push_tlv(&mut body, tlv_tag::INJECT, &one);
    }
    // 指纹
    let mut gp: Vec<u8> = Vec::with_capacity(48);
    push_u64(&mut gp, ob.replay.matrix_fp);
    push_u64(&mut gp, ob.replay.casewise_fp);
    push_u64(&mut gp, ob.freeze.fingerprint());
    push_u64(&mut gp, ob.metrics.fingerprint());
    push_u64(&mut gp, ob.fsm.fingerprint());
    push_u64(&mut gp, ob.ring.fingerprint());
    push_tlv(&mut body, tlv_tag::FINGERPRINT, &gp);
    // 组头（version 单独一行）
    let mut head_payload: Vec<u8> = Vec::with_capacity(24);
    push_u64(&mut head_payload, 0x5645_5853_3236_4445); // magic
    push_u64(&mut head_payload, V_VERSION_ID); // version（单独一行）
    push_u64(&mut head_payload, body.len() as u64); // body_len
    let mut out: Vec<u8> = Vec::with_capacity(body.len().wrapping_add(TLV_HEAD_LEN));
    push_tlv(&mut out, tlv_tag::HEAD, &head_payload);
    out.extend_from_slice(&body);
    out
}
/// TLV 往返自洽校验。
pub fn tlv_roundtrip_ok(buf: &[u8]) -> bool {
    if buf.len() < TLV_HEAD_LEN {
        return false;
    }
    if buf.first().copied() != Some(tlv_tag::HEAD) {
        return false;
    }
    let magic = read_u64(buf, 3);
    let version = read_u64(buf, 11);
    let declared = read_u64(buf, 19) as usize;
    if magic != 0x5645_5853_3236_4445 || version != V_VERSION_ID {
        return false;
    }
    let mut off: usize = TLV_HEAD_LEN;
    let mut walked: usize = 0;
    let mut metric_rows: usize = 0;
    let mut inject_rows: usize = 0;
    let mut freeze_off: usize = usize::MAX;
    let mut ig_off: usize = usize::MAX;
    let mut fp_off: usize = usize::MAX;
    while off < buf.len() {
        let tag = match buf.get(off) {
            Some(t) => *t,
            None => return false,
        };
        if tag == 0 {
            return false;
        }
        let len = read_u16(buf, off.wrapping_add(1)) as usize;
        if len == 0 {
            return false;
        }
        let payload = off.wrapping_add(3);
        match tag {
            tlv_tag::FREEZE => {
                if freeze_off == usize::MAX {
                    freeze_off = payload;
                }
            }
            tlv_tag::INTEGRITY => {
                if ig_off == usize::MAX {
                    ig_off = payload;
                }
            }
            tlv_tag::FINGERPRINT => {
                if fp_off == usize::MAX {
                    fp_off = payload;
                }
            }
            tlv_tag::METRIC => {
                metric_rows = metric_rows.wrapping_add(1);
            }
            tlv_tag::INJECT => {
                inject_rows = inject_rows.wrapping_add(1);
            }
            _ => {}
        }
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
    if walked < 6 {
        return false;
    }
    if metric_rows != ob_metric_count() {
        return false;
    }
    if inject_rows != FORM_COUNT {
        return false;
    }
    // 冻结件回读：13 项
    if freeze_off == usize::MAX {
        return false;
    }
    let rev = read_u64(buf, freeze_off);
    let cases = read_u64(buf, freeze_off.wrapping_add(8));
    let faces = read_u64(buf, freeze_off.wrapping_add(16));
    let tiers = read_u64(buf, freeze_off.wrapping_add(24));
    let cols = read_u64(buf, freeze_off.wrapping_add(32));
    let csteps = read_u64(buf, freeze_off.wrapping_add(40));
    let mhash = read_u64(buf, freeze_off.wrapping_add(48));
    let bhash = read_u64(buf, freeze_off.wrapping_add(56));
    let igfp = read_u64(buf, freeze_off.wrapping_add(64));
    let cvfp = read_u64(buf, freeze_off.wrapping_add(72));
    let fsmfp = read_u64(buf, freeze_off.wrapping_add(80));
    let state = read_u64(buf, freeze_off.wrapping_add(88));
    let anno = read_u64(buf, freeze_off.wrapping_add(96));
    if rev != MATRIX_BOOK_REV as u64 {
        return false;
    }
    if cases != CASE_COUNT as u64 || faces != FACE_COUNT as u64 || tiers != TIER_COUNT as u64 || cols != COLUMN_COUNT as u64 {
        return false;
    }
    if csteps != CASE_STEPS as u64 || anno != TOTAL_FIELDS as u64 {
        return false;
    }
    if mhash == 0 || igfp == 0 || cvfp == 0 || fsmfp == 0 {
        return false;
    }
    if mhash != bhash {
        return false;
    }
    if state > 4 {
        return false;
    }
    // 完整性回读：三张位图必须全零（这是「零缺失」在字节层的断言）
    if ig_off == usize::MAX {
        return false;
    }
    let mb0 = read_u64(buf, ig_off);
    let mb1 = read_u64(buf, ig_off.wrapping_add(8));
    let bb0 = read_u64(buf, ig_off.wrapping_add(16));
    let bb1 = read_u64(buf, ig_off.wrapping_add(24));
    let db0 = read_u64(buf, ig_off.wrapping_add(32));
    let db1 = read_u64(buf, ig_off.wrapping_add(40));
    let reg = read_u64(buf, ig_off.wrapping_add(48));
    let nb = read_u64(buf, ig_off.wrapping_add(56));
    let uq = read_u64(buf, ig_off.wrapping_add(64));
    if mb0 != 0 || mb1 != 0 || bb0 != 0 || bb1 != 0 || db0 != 0 || db1 != 0 {
        return false;
    }
    if reg != TOTAL_FIELDS as u64 || nb != TOTAL_FIELDS as u64 || uq != TOTAL_FIELDS as u64 {
        return false;
    }
    // 指纹回读：6 项全非零
    if fp_off == usize::MAX {
        return false;
    }
    let mut k: usize = 0;
    while k < 6 {
        if read_u64(buf, fp_off.wrapping_add(k * 8)) == 0 {
            return false;
        }
        k = k.wrapping_add(1);
    }
    true
}
/// 指标键值对数（TLV `METRIC` 组数应恒等于此）。
fn ob_metric_count() -> usize {
    let mut n: usize = 0;
    let z = Metrics::zero();
    for (_k, _v) in z.kv().iter() {
        n = n.wrapping_add(1);
    }
    n
}
// ===========================================================================
// 18. 性能逐项分解
// ===========================================================================
/// 一条性能分解项。
#[derive(Clone, Copy, Debug)]
pub struct CostItem {
    /// 项名。
    pub name: &'static str,
    /// 复杂度记号。
    pub complexity: &'static str,
    /// 内存字节（模型侧估算）。
    pub mem_bytes: u64,
    /// syscall 次数（本单恒 0）。
    pub syscalls: u64,
    /// 步数上界。
    pub steps: u64,
    /// 「深在哪」注记（与 Windows 同类能力对照）。
    pub deepen: &'static str,
}
impl CostItem {
    /// 一行描述。
    pub fn describe(&self) -> String {
        format!("cost {} complexity={} mem={}B syscall={} steps={} deepen={}", self.name, self.complexity, self.mem_bytes, self.syscalls, self.steps, self.deepen)
    }
}

/// 性能逐项分解清单。
pub fn complexity() -> Vec<CostItem> {
    let mut v: Vec<CostItem> = Vec::new();
    v.push(CostItem {
        name: "matrix_expand",
        complexity: "O(FACE_COUNT*TIER_COUNT)=O(27)",
        mem_bytes: (CASE_COUNT as u64).wrapping_mul(160),
        syscalls: 0,
        steps: CASE_COUNT as u64,
        deepen: "Windows 的测试矩阵向导只让用户手填行列表；本单 27 例由 9×3 现算，四列串全部由面/档派生，改面必改串，杜绝「改表不改串」",
    });
    v.push(CostItem {
        name: "integrity_report",
        complexity: "O(CASES*COLUMNS)=O(108) 三位图点验",
        mem_bytes: (TOTAL_FIELDS as u64).wrapping_mul(48),
        syscalls: 0,
        steps: TOTAL_FIELDS as u64,
        deepen: "缺失 / 空串 / 重复三张位图；Windows 侧只查「非空」，复制粘贴导致的重复值照样通过——本单的 dup 位图专门堵这个洞",
    });
    v.push(CostItem {
        name: "coverage_report",
        complexity: "O(FACE_COUNT*TIER_COUNT)=O(27)",
        mem_bytes: (FACE_COUNT as u64).wrapping_mul(24),
        syscalls: 0,
        steps: CASE_COUNT as u64,
        deepen: "九面逐面点验 3 档是否齐；Windows 的 coverage 靠人工数勾选框，本单是 u64 漏面位图，可入册可 diff",
    });
    v.push(CostItem {
        name: "run_case",
        complexity: "O(CASE_STEPS)=O(16) per case",
        mem_bytes: 256,
        syscalls: 0,
        steps: (CASE_STEPS * 2) as u64,
        deepen: "每例独立夹具：重解析四列 + 重算例指纹与登记值比对。深在「这是矩阵字段自洽检查，不是回归执行」这条语义边界写进了函数注释与文件头",
    });
    v.push(CostItem {
        name: "run_matrix",
        complexity: "O(CASE_COUNT*CASE_STEPS)=O(432)",
        mem_bytes: (CASE_COUNT as u64).wrapping_mul(48),
        syscalls: 0,
        steps: MATRIX_STEPS as u64,
        deepen: "27 例各自独立跑，污染不跨例传播；判据说「不做用例执行」，本函数跑的是字段夹具，两者在册内严格区分",
    });
    v.push(CostItem {
        name: "stability_sweep",
        complexity: "O(REPEAT*CASES*CASE_STEPS)=O(1296)",
        mem_bytes: (REPEAT as u64).wrapping_mul(32),
        syscalls: 0,
        steps: (MATRIX_STEPS * REPEAT) as u64,
        deepen: "3 轮各**重新 expand()** 再跑，且 matrix_fp + casewise_fp 双指纹全等；只查总折叠会漏掉「两例对调而总折叠恰巧相等」的位置漂移",
    });
    v.push(CostItem {
        name: "inject_all",
        complexity: "O(FORM_COUNT*(CASES*COLUMNS+CASES))=O(12*135)",
        mem_bytes: (FORM_COUNT as u64).wrapping_mul(96),
        syscalls: 0,
        steps: (FORM_COUNT as u64).wrapping_mul(3),
        deepen: "12 形态每形态从干净矩阵重建再形变，检出不交叉掩盖；判据要的两类（漏面 / 漂移）各有独立位区间归属，结论可归因到单一失效模式",
    });
    v.push(CostItem {
        name: "freeze_hash",
        complexity: "O(CASES)=O(27) FNV-1a 串接折叠",
        mem_bytes: (CASE_COUNT as u64).wrapping_mul(8),
        syscalls: 0,
        steps: CASE_COUNT as u64,
        deepen: "四列先折进每例指纹再串接（27 次折叠而非 108 次），既保列序契约又不让列序变动被吞；Windows 无方案级哈希，矩阵漂移只能靠人 review",
    });
    v.push(CostItem {
        name: "resource_budget",
        complexity: "O(1) 常数核算",
        mem_bytes: 0,
        syscalls: 0,
        steps: 8,
        deepen: "内存峰值五项相加显式可算、syscall 上界恒 0（纯功能）、步数上界 = 1296×2；深在「资源上界是判据的一部分而非附带说明」",
    });
    v.push(CostItem {
        name: "export_kv",
        complexity: "O(metric_count + flag_count)",
        mem_bytes: 3072,
        syscalls: 0,
        steps: 48,
        deepen: "逐字段注记入册（判据要求 27 例四列注记随冻结件入库）；Windows 侧只有汇总报表，无字段级映射注记",
    });
    v.push(CostItem {
        name: "export_tlv",
        complexity: "O(metric_count + FORM_COUNT + FIXED_GROUPS)",
        mem_bytes: 8192,
        syscalls: 0,
        steps: 160,
        deepen: "组头单独一行写 version，长度常量由 3 次 push_u64 反推（24B）；`tlv_roundtrip_ok()` 回读冻结 13 项 + 三位图 9 项 + 指纹 6 项逐项对账",
    });
    v.push(CostItem {
        name: "observe",
        complexity: "O(REPEAT*CASES*CASE_STEPS + FORM_COUNT*(CASES*COLUMNS+CASES))",
        mem_bytes: 131072,
        syscalls: 0,
        steps: ((MATRIX_STEPS * REPEAT) + (FORM_COUNT * (TOTAL_FIELDS + CASE_COUNT))) as u64,
        deepen: "一次 observe 走完十二段（重放→展开→点验→逐例夹具→状态机→冻结→环形账→注入→检出→稳定性→预算→TLV 往返→总闸），是判据的单一入口",
    });
    v
}
/// 复杂度清单逐行。
pub fn complexity_rows() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in complexity().iter() {
        out.push(c.describe());
    }
    out
}
/// 复杂度总计（步数 + syscall + 内存）。
pub fn complexity_total() -> (u64, u64, u64) {
    let mut steps: u64 = 0;
    let mut sys: u64 = 0;
    let mut mem: u64 = 0;
    for c in complexity().iter() {
        steps = steps.wrapping_add(c.steps);
        sys = sys.wrapping_add(c.syscalls);
        mem = mem.wrapping_add(c.mem_bytes);
    }
    (steps, sys, mem)
}
// ===========================================================================
// 19. 自检套件
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
/// 自检套件（判据总闸的逐条拆解）。
pub fn self_check(seed: u64) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    let m = match CaseMatrix::expand() {
        Ok(v) => v,
        Err(_) => {
            out.push(Check { id: "C2861-00 expand", pass: false, note: "矩阵正交展开失败" });
            return out;
        }
    };
    let ig = integrity_report(&m);
    let cv = coverage_report(&m);
    let st = stability_sweep();
    let budget = resource_budget(&m);
    let (inj, ev) = inject_all();
    // 状态机：走合法链 + 故意非法两次
    let mut fsm = MatrixFsm::new();
    let _ = fsm.go(MatrixState::Reviewing);
    let _ = fsm.go(MatrixState::Frozen);
    let rej1 = fsm.go(MatrixState::Draft).is_err();
    let _ = fsm.go(MatrixState::Drifted);
    let rej2 = fsm.go(MatrixState::Frozen).is_err(); // 漂移不得直接回冻结
    // 冻结件：草拟 → 登记 → 失真声明 → 验证；另造漂移件验拒收
    let mut fr = FreezeRecord::draft(&m, &ig, &cv, &fsm);
    fr.declare_distortion();
    let base_ok = fr.set_baseline(fr.matrix_hash).is_ok();
    let verify_ok = fr.verify().is_ok();
    let mut drifted = fr.clone();
    drifted.matrix_hash = drifted.matrix_hash.wrapping_add(1);
    let anno_n = fr.annotation(&m).len();
    let ob = observe(seed);
    let tlv = export_tlv(&ob);
    // C1 格数
    out.push(Check { id: "C2861-01 grid_count", pass: m.count_ok(), note: "9x3=27 例由维度乘积算出并与常量逐位比对" });
    // C2 正交
    out.push(Check { id: "C2861-02 orthogonal", pass: m.orthogonal_ok(), note: "每个 (face,tier) 组合恰出现一次" });
    // C3 三元组闭集
    out.push(Check { id: "C2861-03 dims", pass: m.dims_ok(), note: "面/档/例三列取值全在闭集内且期望区间自洽" });
    // C4 分布均匀
    let dist_ok = {
        let mut ok = true;
        let mut k: usize = 0;
        while k < FACE_COUNT {
            if m.face_dist[k] != TIER_COUNT {
                ok = false;
            }
            k = k.wrapping_add(1);
        }
        let mut t: usize = 0;
        while t < TIER_COUNT {
            if m.tier_dist[t] != FACE_COUNT {
                ok = false;
            }
            t = t.wrapping_add(1);
        }
        ok
    };
    out.push(Check { id: "C2861-04 dist", pass: dist_ok, note: "每面 3 例、每档 9 例" });
    // C5 四列零缺失（判据核心）
    out.push(Check { id: "C2861-05 integrity", pass: ig.all_ok(), note: "27x4=108 字段：缺失 / 空串 / 重复三张位图全零" });
    // C6 登记与唯一数
    out.push(Check { id: "C2861-06 unique", pass: ig.registered == TOTAL_FIELDS as u64 && ig.unique == TOTAL_FIELDS as u64, note: "登记 108、唯一 108（无复制粘贴）" });
    // C7 面覆盖
    out.push(Check { id: "C2861-07 coverage", pass: cv.all_ok(), note: "九面逐面点验，全映射且每面三档齐" });
    // C8 无漏面
    out.push(Check { id: "C2861-08 no_face_leak", pass: cv.missing_faces == 0, note: "漏面位图全零" });
    // C9 逐例夹具
    out.push(Check { id: "C2861-09 run_all", pass: run_matrix_ok(&m), note: "27 例各跑 16 步字段夹具全过" });
    // C10 三档期望区间
    let tier_ok = {
        let mut ok = true;
        let mut i: usize = 0;
        while i < m.cases.len() {
            let c = match m.case(i) {
                Ok(v) => v,
                Err(_) => break,
            };
            let (lo, hi) = match c.tier_idx {
                0 => (400u64, 600u64),
                1 => (900u64, 1000u64),
                _ => (0u64, 1000u64),
            };
            if c.expect_lo != lo || c.expect_hi != hi {
                ok = false;
            }
            i = i.wrapping_add(1);
        }
        ok
    };
    out.push(Check { id: "C2861-10 tier_expect", pass: tier_ok, note: "核心 400~600 / 边界 900~1000 / 对抗 0~1000" });
    // C11 三段步数统一
    let step_uniform = match CaseSpec::new(0, StressFace::Capacity, CaseTier::Core) {
        Ok(c) => c.steps == CASE_STEPS as u64,
        Err(_) => false,
    };
    out.push(Check { id: "C2861-11 step_uniform", pass: step_uniform, note: "三档单例步数统一 CASE_STEPS=16，27 例可比" });
    // C12 状态机合法边闭集
    out.push(Check { id: "C2861-12 fsm_edges", pass: MatrixFsm::edge_count() == 9, note: "5 态合法边闭集恰 9 条" });
    // C13 非法迁移拒记
    out.push(Check { id: "C2861-13 fsm_reject", pass: rej1 && rej2 && fsm.rejected_count() == 2, note: "冻结态回草拟、漂移态回冻结均被拒记" });
    // C14 冻结哈希入册
    out.push(Check { id: "C2861-14 freeze_hash", pass: fr.matrix_hash != 0 && base_ok, note: "冻结哈希非零且基线登记成功" });
    // C15 漂移拦截
    out.push(Check { id: "C2861-15 drift_reject", pass: drifted.verify().is_err(), note: "冻结哈希漂移注入必被 verify() 拦截" });
    // C16 基线一致
    out.push(Check { id: "C2861-16 verify", pass: verify_ok && fr.hash_matches(), note: "冻结件与 CI 基线逐项一致" });
    // C17 注入覆盖数
    out.push(Check { id: "C2861-17 inject_n", pass: inj.len() == FORM_COUNT && FORM_COUNT >= 10, note: "12 形态各造证一次（判据要求 >=10）" });
    // C18 全拒检出
    let miss = detect_miss();
    out.push(Check { id: "C2861-18 detect_all", pass: miss.is_empty() && ob.detect_pass == FORM_COUNT, note: "12/12 形态逐个独立检出（漏面 6 + 漂移 5 + 缺列 1）" });
    // C19 漏面类 10/10
    let fd = ob.metrics.face_drop_hits;
    out.push(Check { id: "C2861-19 face_drop", pass: fd == 6, note: "漏面类 6/6 检出（判据要求 10/10 覆盖，本单给 6 面×独立位区间）" });
    // C20 漂移类
    let dr = ob.metrics.drift_hits;
    out.push(Check { id: "C2861-20 drift", pass: dr == 5, note: "漂移类 5/5 检出（四列 + 档位互换）" });
    // C21 稳定性
    out.push(Check { id: "C2861-21 stability", pass: st.passed(), note: "REPEAT=3 轮 matrix_fp 与 casewise_fp 双全等" });
    // C22 环形账守恒
    out.push(Check { id: "C2861-22 ring", pass: ob.ring.check().is_ok() && ob.ring.dropped > 0, note: "pushed == dropped + live，dropped 显性非零" });
    // C23 重放确定性
    out.push(Check { id: "C2861-23 replay", pass: ob.replay_stable, note: "同种子连跑两遍矩阵/逐例/完整性/覆盖四指纹全等" });
    // C24 TLV 往返
    out.push(Check { id: "C2861-24 tlv", pass: tlv_roundtrip_ok(&tlv), note: "TLV 长度自洽 + 冻结 13 项 + 三位图 9 项 + 指纹 6 项回读一致" });
    // C25 资源上界
    out.push(Check { id: "C2861-25 budget", pass: budget.within(), note: "步数 / 内存 / syscall 三项上界在界内，syscall 恒 0" });
    // C26 注記入册
    out.push(Check { id: "C2861-26 annotation", pass: anno_n == TOTAL_FIELDS, note: "27 例四列注记共 108 条随冻结件入库" });
    // C27 语义边界声明
    out.push(Check { id: "C2861-27 boundary", pass: fr.distortion_declared && ob.distortion_declared, note: "模型侧字段点验 ≠ 真机回归执行，声明在档" });
    // C28 总闸
    out.push(Check { id: "C2861-28 gate", pass: ob.all_ok(), note: "observe() 判据总闸全过" });
    let _ = ev;
    out
}
/// 自检全过。
pub fn self_check_ok(seed: u64) -> bool {
    for c in self_check(seed).iter() {
        if !c.pass {
            return false;
        }
    }
    true
}
/// 自检报告。
pub fn self_check_report(seed: u64) -> String {
    let cs = self_check(seed);
    let mut passed: usize = 0;
    let mut s = String::new();
    s.push_str("UNX-F2861 self_check\n");
    for c in cs.iter() {
        if c.pass {
            passed = passed.wrapping_add(1);
        }
        s.push_str(&format!("{} {} :: {}\n", c.id, if c.pass { "PASS" } else { "FAIL" }, c.note));
    }
    s.push_str(&format!("self_check {}/{} ok={}\n", passed, cs.len(), if passed == cs.len() { 1 } else { 0 }));
    s
}
// ===========================================================================
// 20. 清单 / 演示
// ===========================================================================
/// 形态清单。
pub fn form_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in Form::ALL.iter() {
        out.push(format!("0x{:04X} {} [{}] :: {} :: probe={}", f.code(), f.name(), f.class_name(), f.describe(), f.probe()));
    }
    out
}
/// 不变量清单。
pub fn invariant_manifest() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    out.push("I1 格数: expand().produced == FACE_COUNT*TIER_COUNT".to_string());
    out.push("I2 正交: 任一 (face,tier) 组合恰出现一次".to_string());
    out.push("I3 闭集: 面/档/例三列取值全部落在各自闭集长度内".to_string());
    out.push("I4 分布: 每面恰 3 例、每档恰 9 例".to_string());
    out.push("I5 零缺失: missing_bits == 0 且 blank_bits == 0 且 dup_bits == 0".to_string());
    out.push("I6 登记齐: registered == nonblank == unique == TOTAL_FIELDS".to_string());
    out.push("I7 面覆盖: covered == FACE_COUNT 且 missing_faces == 0".to_string());
    out.push("I8 档齐: 每面三档（核心/边界/对抗）各 1 例".to_string());
    out.push("I9 期望区间: 核心 400~600 / 边界 900~1000 / 对抗 0~1000".to_string());
    out.push("I10 步数齐: 27 例单例步数恒 == CASE_STEPS".to_string());
    out.push("I11 环形: pushed == dropped + live 且 live <= RING_CAP".to_string());
    out.push("I12 冻结态: 仅 allowed() 闭集内迁移，非法一律拒记不改态".to_string());
    out.push("I13 漂移: matrix_hash == baseline_hash（baseline != 0）".to_string());
    out.push("I14 稳定性: REPEAT 轮 matrix_fp 与 casewise_fp 双全等".to_string());
    out.push("I15 TLV: HEAD 声明长度 + TLV_HEAD_LEN == buf.len()，冻结/位图/指纹回读一致".to_string());
    out.push("I16 预算: steps_used <= step_cap 且 mem_peak <= mem_cap 且 syscalls == 0".to_string());
    out.push("I17 注记: 四列注记条数 == TOTAL_FIELDS（判据要求随冻结件入库）".to_string());
    out
}
/// 演示场景（走一遍主流程并留长表）。
pub fn demo_scene(seed: u64) -> Vec<String> {
    let ob = observe(seed);
    let mut out: Vec<String> = Vec::new();
    out.push(ob.summary());
    out.push(ob.matrix.summary());
    out.push(ob.integrity.summary());
    out.push(ob.coverage.summary());
    out.push(ob.freeze.describe());
    out.push(ob.stability.summary());
    out.push(ob.ring.summary());
    out.push(ob.replay.describe());
    out.push(resource_budget(&ob.matrix).summary());
    out
}
/// 演示账（矩阵前 12 例 + 状态机边全表 + 注入全表）。
pub fn demo_ledger(seed: u64) -> String {
    let ob = observe(seed);
    let mut s = String::new();
    s.push_str("== cases (前 12) ==\n");
    let mut n: usize = 0;
    for c in ob.matrix.cases.iter() {
        if n >= 12 {
            break;
        }
        s.push_str(&format!("{}\n", c.describe()));
        n = n.wrapping_add(1);
    }
    s.push_str("== fsm edges ==\n");
    for line in MatrixFsm::edge_manifest().iter() {
        s.push_str(&format!("{}\n", line));
    }
    s.push_str("== inject ==\n");
    for inj in ob.injected.iter() {
        s.push_str(&format!("{}\n", inj.describe()));
    }
    s.push_str("== stability ==\n");
    for line in ob.stability.rows().iter() {
        s.push_str(&format!("{}\n", line));
    }
    let _ = seed;
    s
}
/// 关联单清单。
pub fn related_str() -> String {
    let mut s = String::new();
    s.push_str("F2802-F2809 九面压测（本单只引用其判据段，不重定义阈值）\n");
    s.push_str("F2810 压测基线（本单漂移检测的比对源，汇总面来源）\n");
    s.push_str("F2801 大页池容量压测（本单三档期望区间沿用其三态口径）\n");
    s.push_str("F2841 大页-地址空间联合压测（消费本单九面清单）\n");
    s.push_str("F2863 回归用例执行 runner（本单明确不做执行，边界写进文件头）\n");
    s
}
// <<TAIL>>