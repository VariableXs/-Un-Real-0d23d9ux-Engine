//! unx0587_dupkey —— UNX-F0587 重复参数账（首现口径）。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//!       第 2128 行（UNX-A1-B30 · 命令行参数账 F581–F600 批次表 · AI-01 承办）：
//!
//!   | UNX-F0587 | 重复参数账（首现口径） | 200 | 已深化 |
//!   | 重复键账（注入同键 3 现——首现生效 1/1 断言+重复计数 2/2），重复簿可查 |
//!
//! 定位原文（同批次相邻条目，供链路定位）：
//!
//!   | UNX-F0586 | 参数值域核（域断言） | 200 | 已深化 |
//!   | 值域核账齐（整数 8 键域表+字符串 8 键长表），越域拒收率 100%（注入 4/4） |
//!
//!   | UNX-F0588 | 引号与转义账（字符串边角） | 200 | 已深化 |
//!   | 引号账齐（成对/嵌套/转义 3 样 3/3 与基准一致），未闭合拒收断言（1/1） |
//!
//! 链路定位：上游 F0582/F0585 保证「键值对长得对、值解析得出」，F0586
//!   保证「值站得住」——但都没回答「同一个键来两次怎么办」。本条冻结
//!   答案：**首现生效（first-win）**——同键多现时，第一次出现的值进入
//!   生效表，其后每一次出现全量忽略（不覆盖、不合并、不报错中断），
//!   但逐笔入重复簿（值+出现位置+与生效值同异标注）供事后审计。下游
//!   F0590 缺省值账在「键从未出现」的槽位上补缺省；F0593 参数快照封账
//!   消费的正是本条产出的首现生效表。F0583 白名单 24 键三型分布即本条
//!   键空间（8 布尔 + 8 整数 + 8 字符串，其中整数/字符串 16 键与 F0586
//!   两张域表逐字对齐——键名一致、顺序一致）；F0584 未知键显性弃置在
//!   本条承接为「白名单外重复双重违规分账」（同一未知键第二次出现 =
//!   白名单违规 + 重复违规两罪并记）。
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 首现口径 const 声明（FIRST_WIN_SEMANTICS，模块级冻结）：
//!   「同键多现：首现进生效表；后现全忽略但逐笔入重复簿（值+位置+
//!    同异标注）；重复计数 = 后现次数；总现数 = 1（首现）+ 重复数」。
//!   生效值恒等于首现值——这不是实现巧合而是结构保证：生效表槽位只在
//!   「该键第一次被接受」时写入，此后只读（first_win_selfcheck 面可对
//!   任意注入批次自证）。
//!
//! 组件清单（全手工定制，无空壳）：
//!   1. KEY_TABLE      键空间 24 键闭集（8 布尔自设计 + 8 整数 + 8 字符串
//!                     与 F0586 域表逐字对齐；每键来源注记）
//!   2. BOOL_TRUE_WORDS / BOOL_FALSE_WORDS 布尔词形闭集（true/on/1/yes、
//!                     false/off/0/no——比较口径规范化用）
//!   3. DupKeyError    诊断码闭集 0x1E90~0x1E9F（16 变体，逐变体中文
//!                     可操作诊断 detail()）
//!   4. ValCell        定长 64 字节值单元（零堆、Copy、上栈安全）
//!   5. canonical_bool 布尔词形规范化（8 词形 → 2 规范位）
//!   6. EventKind/Event/EventRing 事件账 32 槽滑窗（16 事件类逐类中文）
//!   7. DupEntry       重复簿条目（键+原文+出现位置+第几现+同异标注）
//!   8. UnknownRepeat  未知键复现账（双重违规分账的数据面）
//!   9. FirstWinLedger 首现口径账主体：生效表 24 槽 + 每键重复计数 +
//!                     同值/异值分列 + 重复簿 + 未知键复现账 + 人话报告 +
//!                     白名单联动审计 + 恒等式自审 + 健康分 + FNV 指纹
//!  10. ParamStream    参数流：定长环形 64 槽（键,值）流式登记 + 封流 +
//!                     一拍快照槽（封后写拦截 100%）
//!  11. FirstWinSnapshot 首现快照：生效表封账副本 + 指纹 + 首现断言面
//!                     （注入同键 3 现自证：生效 1/1 + 计数 2/2）
//!  12. TripleFace     三现注入自证面（J 判据核心断言结构化）
//!  13. render 面      重复键人话中文报告（「键 X 出现 3 次，生效值=首次
//!                     值 v1，后现值 v2/v3 被忽略」式）+ 同值/异值分列 +
//!                     双重违规清单 + 预算截断
//!  14. dual 面        双跑确定性对拍（同批次两跑：推入结果逐笔全等 +
//!                     账面指纹全等 + 快照指纹全等）
//!  15. injection 面   三注入全检出（同键三现 1/1+2/2、异值重复分列、
//!                     未知键重复双重违规）= 3/3
//!  16. fnv64          FNV-1a 64 位指纹（公开测试向量锁基）
//!
//! 性能逐项分解：
//!   push：键长卫 O(1) + 键查表 O(24×字节) + 值长卫 O(1) + 布尔规范化
//!         O(8×字节) + 环形写 O(值字节) + 账本入账 O(1)；同异判定
//!         O(值字节)；重复簿 push 摊还 O(1)；
//!   报告：O(重复键数 × 值字节)，预算 768 字节截断护顶；
//!   快照：O(24 槽 + 重复簿)；指纹 O(账面字节)；
//!   对拍：O(批 × push + 账面字节)；注入面 O(3)。
//!
//! 诊断码段：0x1E90~0x1E9F（16 码闭集）。段位择取实录：任务预核段
//!   0x1E90 起；开工复核 `grep -rn "0x1E9" kernel/varix/src` 命中 5 处
//!   ——均为 64 位 boot 种子常量（0x1E963ABF03B9CE59 族，见
//!   unx2422_uorder14.rs:3516 / unx2442_urefill.rs:3516 /
//!   unx2482_uorder9raw.rs:3515 / unx2502_uhptime.rs:3522 /
//!   unx2562_uaspcreate.rs:3526），并非 16 位诊断码；对 16 位精确码
//!   0x1E90..=0x1E9F 逐码 grep（词边界）零命中。顺位候选 0x1ED0 段复核
//!   被占（unx0395_heapledger.rs:158 魔数 0x1ED5_0001、unx4921_mdcsum.rs
//!   CRC32C 多项式 0x1EDC6F41）；0x1EF0 段亦被占（unx2762_umaptbl.rs
//!   头注声明独占 0x1EC0~0x1EFF 全段，0x1EF0 落其内）——两顺位均否决，
//!   回取预核段 0x1E90：16 位精确码全树零命中，独占确认。
//!
//! 纪律声明：no_std + alloc；零 unwrap/expect/panic!/运行期 assert!
//!   （编译期 `const _: () = assert!(…)` 允许；测试体内 assert! 是
//!   断言面本身，与内核路径分开）；可失败全走 Result<T, DupKeyError>；
//!   计数递增一律 wrapping、健康分扣减一律 saturating、报告拼接超预算
//!   显性截断不丢账；>64KB 结构体不上栈（本模块最大内联结构
//!   ParamStream ≈ 2KB，环形/簿/事件全走 Vec 堆）；const fn 内只用
//!   while+索引+手工字节比较，不用 slice::get/Vec 索引/Ord::min/max/
//!   derive PartialEq 的 ==/for 循环。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════
// 版本与全局常量（来源逐条注记）
// ═══════════════════════════════════════════════════════════════════════

/// 模块版本。
pub const DK_VERSION: &str = "unx0587_dupkey-v1";

/// 诊断段基址（0x1E90 段 · 本模块独占 · 择取实录见头注；16 位精确码
/// 全 kernel 树零命中，顺位 0x1ED0/0x1EF0 均被占后回取本段）。
pub const DK_SEG_BASE: u16 = 0x1E90;

/// 诊断段顶（基址 + 0x0F = 0x1E9F，16 码闭集）。
pub const DK_SEG_TOP: u16 = 0x1E9F;

/// 键空间总键数（F0583 白名单口径：24 键三型分布 = 8+8+8）。
pub const KEY_N: usize = 24;

/// 布尔键数（自设计 8 键，来源逐键注记于 KEY_TABLE）。
pub const BOOL_N: usize = 8;

/// 整数键数（与 F0586 INT_TABLE 逐字对齐）。
pub const INT_N: usize = 8;

/// 字符串键数（与 F0586 STR_TABLE 逐字对齐）。
pub const STR_N: usize = 8;

/// 参数流定长环形槽位上限（来源：F0581 参数行 256 字节上限 ÷ 最短对
/// 「k=v」+1 分隔空格 = 4 字节/对 → 64 对为单行理论极值，环形恒不溢出；
/// 溢出仅多行复用误用时触发，触发即拒收并计数）。
pub const STREAM_CAP: usize = 64;

/// 事件账滑窗上限（32 槽；满后滑出最旧并计 dropped）。
pub const EVENT_CAP: usize = 32;

/// 重复簿槽位上限（= STREAM_CAP：每次出现最多占 1 簿位，重复簿理论
/// 极值 = 环形极值，恒容得下；超限走 RingFull 先行拒收，簿永不溢出）。
pub const DUP_BOOK_CAP: usize = STREAM_CAP;

/// 未知键复现账槽位上限（16 槽：未知键本就罕见，16 槽覆盖全键位扫描
/// 冗余；满后只累计不新增条目，账不丢数）。
pub const UNKNOWN_CAP: usize = 16;

/// 单值字节预算（来源：F0589 单 token 64 字节界——值即 token，同界）。
pub const VAL_MAX: usize = 64;

/// 键名截断保存预算（24 字节：白名单最长键 sched.slice.us=13 字节，
/// 24 字节留未知键保存余量；超长键走 KeyTooLong 快速拒收不入账）。
pub const KEY_KEEP: usize = 24;

/// 人话报告字节预算默认值（768：24 键全重复的极值场景单行约 24 字节×
/// 64 现仍可截断护顶；超预算行边界截断加尾注，不丢账面数）。
pub const REPORT_BUDGET: usize = 768;

/// 人话报告最小可行预算（低于此连表头都放不下，显性报错而非空串）。
pub const REPORT_BUDGET_MIN: usize = 64;

/// 健康分满刻度（0~1000 整数定点，无浮点）。
pub const HEALTH_FULL: u32 = 1_000;

/// 健康分地板线（低于 600 = 三成扣分线，视引导参数面失控）。
pub const HEALTH_FLOOR: u32 = 600;

/// 检出率满刻度 ×1000（判据口径：100% = 1000‰）。
pub const CAUGHT_RATE_FULL: u64 = 1_000;

/// 首现口径 const 声明（模块级冻结语义——头注「首现口径 const 声明」
/// 一节的原文；`ParamStream::first_win_semantics()` 原样返回本常量）。
pub const FIRST_WIN_SEMANTICS: &str =
    "first-win：同键多现，首现进生效表，后现全忽略但逐笔入重复簿（值+位置+同异标注）；生效值恒等于首现值";

/// 与 F0586 值域核键空间对齐声明（整数/字符串 16 键逐字一致）。
pub const VALDOM_ALIGNMENT: &str =
    "键空间对齐：int/str 16 键与 unx0586_valdom INT_TABLE/STR_TABLE 逐字一致；bool 8 键为本条自设计（F0583 三型分布补全）";

// ═══════════════════════════════════════════════════════════════════════
// 诊断码闭集（0x1E90~0x1E9F，每变体 detail() 中文可操作诊断）
// ═══════════════════════════════════════════════════════════════════════

/// 重复参数账诊断码闭集（#[repr(u16)]，码值即段内偏移身份）。
#[repr(u16)]
#[derive(Clone, Copy, Debug)]
pub enum DupKeyError {
    /// 0x1E90 键不在 24 键闭集（白名单外——F0583 口径）。
    UnknownKey = 0x1E90,
    /// 0x1E91 空键（零字节键名，无法查表）。
    EmptyKey = 0x1E91,
    /// 0x1E92 键超长（> KEY_KEEP=24 字节，白名单最长键 13 字节，
    /// 超长键必不在闭集——快速拒收通道，不占未知键账槽）。
    KeyTooLong = 0x1E92,
    /// 0x1E93 值超长（> VAL_MAX=64 字节，F0589 token 界）。
    ValueTooLong = 0x1E93,
    /// 0x1E94 布尔键值非法（不在 8 词形闭集内）。
    BoolValInvalid = 0x1E94,
    /// 0x1E95 参数流环形满（64 槽占满仍登记——多行复用误用口径）。
    RingFull = 0x1E95,
    /// 0x1E96 封流后再写（seal 之后 push——封后写拦截 100% 面）。
    StreamSealed = 0x1E96,
    /// 0x1E97 未封流先取快照（快照须以封流为前提——账定才能拍）。
    StreamNotSealed = 0x1E97,
    /// 0x1E98 快照槽已用（每流恰一拍——防两次快照指纹漂移）。
    SnapshotSealed = 0x1E98,
    /// 0x1E99 簿位索引越界（重复簿/流位查询超界）。
    SlotOutOfRange = 0x1E99,
    /// 0x1E9A 键不在账（查询未出现键的生效值/重复清单）。
    KeyNotFound = 0x1E9A,
    /// 0x1E9B 双跑对拍失配（两跑指纹或逐笔结果不一致——确定性破坏）。
    FingerprintMismatch = 0x1E9B,
    /// 0x1E9C 账目恒等式自审失败（总现数≠首现+重复+拒收 等破账）。
    AuditFailed = 0x1E9C,
    /// 0x1E9D 健康分触底（< 600：引导参数面失控，须人工介入）。
    HealthFloorHit = 0x1E9D,
    /// 0x1E9E 报告预算过小（< 64 字节连表头放不下，拒绝渲染空壳）。
    ReportBudgetExceeded = 0x1E9E,
    /// 0x1E9F API 参数非法（如对拍空批次——确定性对拍无意义）。
    InvalidParam = 0x1E9F,
}

impl DupKeyError {
    /// 诊断码（段内身份；恒落 [DK_SEG_BASE, DK_SEG_TOP] 闭集）。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 错误是否落本段闭集（防漂移的公开卫）。
    pub const fn in_segment(self) -> bool {
        self.code() >= DK_SEG_BASE && self.code() <= DK_SEG_TOP
    }

    /// 中文可操作诊断（逐变体：是什么+为什么+怎么办）。
    pub fn detail(self) -> &'static str {
        match self {
            DupKeyError::UnknownKey => {
                "键不在 24 键闭集（F0583 白名单口径）。该键属白名单外参数：\
                 单次出现按 F0584 显性弃置记账；同一未知键再次出现将升级为\
                 双重违规（白名单外+重复）分账。请核对 KEY_TABLE 24 键拼写，\
                 或到白名单变更流程申请入表。"
            }
            DupKeyError::EmptyKey => {
                "键名为空串（零字节）。等号劈分（F0582）产出空左值通常是\
                 行首孤等号「=v」——请修引导参数行；本条快速拒收，不入账槽。"
            }
            DupKeyError::KeyTooLong => {
                "键名超 24 字节预算。白名单最长键 sched.slice.us 仅 13 字节，\
                 超长键必不在闭集——走快速拒收通道省账槽。请检查是否误把\
                 值或整行当键传入（等号劈分错位）。"
            }
            DupKeyError::ValueTooLong => {
                "值超 64 字节（F0589 单 token 界）。值即 token，同界同卫；\
                 超长值在本条拒收（不占环形槽）。请核对该键值的合法形态\
                 （长值请走字符串键并遵守 F0586 键长表）。"
            }
            DupKeyError::BoolValInvalid => {
                "布尔键的值不在 8 词形闭集（true/on/1/yes、false/off/0/no）。\
                 布尔键比较走规范化口径：8 词形归 2 规范位后再判同异。\
                 请改用闭集词形（推荐 true/false）。"
            }
            DupKeyError::RingFull => {
                "参数流 64 槽环形已满。单条 256 字节参数行（F0581 界）理论\
                 极值恰 64 对，正常单行永不触发；触发即多行复用误用——\
                 请一条参数行一个流，seal 后另起。已发生的 64 对账面保留。"
            }
            DupKeyError::StreamSealed => {
                "封流后再写。seal() 之后流进入只读口径（F0593 封账前置），\
                 再 push 即拦截（100% 面）。如需重登记请另起新流——本流\
                 账面与快照指纹已定。"
            }
            DupKeyError::StreamNotSealed => {
                "未封流先取快照。快照以封流为前提：账定（seal）才能拍\
                 （snapshot）。请先 seal() 再 snapshot()。"
            }
            DupKeyError::SnapshotSealed => {
                "快照槽已用。每流恰一拍：第二次拍会与第一次产生指纹比对\
                 歧义，故封槽。复扫需求请用 FirstWinSnapshot::reverify\
                 对拍既有快照，或另起新流重跑。"
            }
            DupKeyError::SlotOutOfRange => {
                "簿位/流位索引越界。请用 len()/dup_book_len() 先取上界，\
                 索引须 < 上界。本查询零副作用。"
            }
            DupKeyError::KeyNotFound => {
                "查询的键不在账：该键从未被接受（未首现）。请先核对该键\
                 是否被拒收过（unknown/bool/len 拒收计数），或换用\
                 dup_book_of 查重复面。"
            }
            DupKeyError::FingerprintMismatch => {
                "双跑对拍失配：同批次两跑的账面指纹或逐笔推入结果不一致。\
                 这破坏复扫确定性（F0594 口径）——请回放事件账定位首个\
                 分叉点，检查是否引入了时序/地址依赖。"
            }
            DupKeyError::AuditFailed => {
                "账目恒等式自审失败：总现数 ≠ 首现+重复+各类拒收，或\
                 重复簿条数 ≠ 重复计数总和，或同值+异值 ≠ 总重复。\
                 账已破——请保存 canonical_state() 全量转储后复位重放。"
            }
            DupKeyError::HealthFloorHit => {
                "健康分跌破地板线 600/1000：拒收或违规类扣分累计过大，\
                 引导参数面失控。请优先处理未知键/双重违规（可能是恶意\
                 或损坏的参数行），再复跑。"
            }
            DupKeyError::ReportBudgetExceeded => {
                "报告预算过小（< 64 字节）：连表头都放不下。渲染空壳不如\
                 显性报错——请给 ≥ REPORT_BUDGET_MIN 的预算（默认 768）。"
            }
            DupKeyError::InvalidParam => {
                "API 参数非法：如对拍空批次（空批次的「确定性」无意义）、\
                 注入面空样本。请传入非空批次后重试。"
            }
        }
    }
}

/// 诊断码闭集自审（编译期）：16 变体码值互异且全落 0x1E90~0x1E9F。
const _: () = assert!(DupKeyError::UnknownKey.code() == 0x1E90);
const _: () = assert!(DupKeyError::EmptyKey.code() == 0x1E91);
const _: () = assert!(DupKeyError::KeyTooLong.code() == 0x1E92);
const _: () = assert!(DupKeyError::ValueTooLong.code() == 0x1E93);
const _: () = assert!(DupKeyError::BoolValInvalid.code() == 0x1E94);
const _: () = assert!(DupKeyError::RingFull.code() == 0x1E95);
const _: () = assert!(DupKeyError::StreamSealed.code() == 0x1E96);
const _: () = assert!(DupKeyError::StreamNotSealed.code() == 0x1E97);
const _: () = assert!(DupKeyError::SnapshotSealed.code() == 0x1E98);
const _: () = assert!(DupKeyError::SlotOutOfRange.code() == 0x1E99);
const _: () = assert!(DupKeyError::KeyNotFound.code() == 0x1E9A);
const _: () = assert!(DupKeyError::FingerprintMismatch.code() == 0x1E9B);
const _: () = assert!(DupKeyError::AuditFailed.code() == 0x1E9C);
const _: () = assert!(DupKeyError::HealthFloorHit.code() == 0x1E9D);
const _: () = assert!(DupKeyError::ReportBudgetExceeded.code() == 0x1E9E);
const _: () = assert!(DupKeyError::InvalidParam.code() == 0x1E9F);
const _: () = assert!(DK_SEG_BASE == 0x1E90 && DK_SEG_TOP == 0x1E9F);
const _: () = assert!(KEY_N == BOOL_N + INT_N + STR_N);
const _: () = assert!(STREAM_CAP == 64 && DUP_BOOK_CAP == STREAM_CAP);
// ═══════════════════════════════════════════════════════════════════════
// 键空间 24 键闭集（8 布尔 + 8 整数 + 8 字符串）
// ═══════════════════════════════════════════════════════════════════════

/// 参数键型（三型分布口径——F0583 白名单：布尔/整数/字符串）。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    /// 布尔键（值走 8 词形闭集规范化）。
    Bool = 0,
    /// 整数键（值域归 F0586 INT_TABLE 管，本条只记账不判域）。
    Int = 1,
    /// 字符串键（值域归 F0586 STR_TABLE 管，本条只记账不判域）。
    Str = 2,
}

impl KeyKind {
    /// 型名（中文，报告/审计渲染用）。
    pub const fn text(self) -> &'static str {
        match self {
            KeyKind::Bool => "布尔",
            KeyKind::Int => "整数",
            KeyKind::Str => "字符串",
        }
    }
}

/// 键规格（键名 + 型 + 来源注记；来源注记是账的一部分——键空间冻结
/// 须可追溯每键为什么在表内）。
#[derive(Clone, Copy, Debug)]
pub struct KeySpec {
    /// 参数键名（&'static str，冻结）。
    pub key: &'static str,
    /// 键型。
    pub kind: KeyKind,
    /// 入表来源注记（逐键注记：F0586 对齐 / 本条自设计+出处）。
    pub note: &'static str,
}

/// 键空间 24 键闭集（顺序冻结：0..8 布尔、8..16 整数、16..24 字符串；
/// 整数/字符串 16 键与 unx0586_valdom 的 INT_TABLE/STR_TABLE 逐字对齐
/// ——键名一致、表内顺序一致，跨模块对账以本声明为准）。
pub const KEY_TABLE: [KeySpec; KEY_N] = [
    // ── 布尔 8 键（本条自设计，补全 F0583 三型分布；词形闭集见
    //    BOOL_TRUE_WORDS/BOOL_FALSE_WORDS）──────────────────────────
    // 键 1 nokaslr：关闭内核地址布局随机化。来源：Linux 同名引导参数
    // 惯例；no_std 内核页表布局固定化开关，调试/取证场景常开。
    KeySpec { key: "nokaslr", kind: KeyKind::Bool, note: "自设计：Linux 同名惯例，地址布局固定化" },
    // 键 2 quiet：安静模式（抑制低优先级日志输出）。来源：Linux 同名
    // 惯例；与 loglevel 联动（quiet 时 loglevel 视作封顶 3，联判归 F0591）。
    KeySpec { key: "quiet", kind: KeyKind::Bool, note: "自设计：Linux 同名惯例，低优日志抑制" },
    // 键 3 earlycon：早期控制台（页表就绪前的引导期输出）。来源：
    // Linux 同名惯例；x86_64 引导期 UART 直写开关。
    KeySpec { key: "earlycon", kind: KeyKind::Bool, note: "自设计：Linux 同名惯例，引导早期控制台" },
    // 键 4 smp.off：单核引导（跳过次核唤醒）。来源：APIC SMP 引导
    // 流程开关，dot 命名空间与 uart.baud 等既有键风格一致。
    KeySpec { key: "smp.off", kind: KeyKind::Bool, note: "自设计：次核唤醒跳过，单核除错口径" },
    // 键 5 watchdog：NMI 看门狗开启。来源：x86 NMI 看门狗惯例；与
    // panic.halt 互斥归 F0591 互斥表管辖。
    KeySpec { key: "watchdog", kind: KeyKind::Bool, note: "自设计：NMI 看门狗开关" },
    // 键 6 acpi.off：禁用 ACPI 表解析（回退 legacy 探测）。来源：
    // Linux acpi=off 惯例收编为布尔 dot 形。
    KeySpec { key: "acpi.off", kind: KeyKind::Bool, note: "自设计：acpi=off 惯例布尔化" },
    // 键 7 efi.debug：EFI 引导服务调试输出。来源：EFI runtime 调试
    // 开关，引导交接（F0581）前的输出面。
    KeySpec { key: "efi.debug", kind: KeyKind::Bool, note: "自设计：EFI 引导调试输出" },
    // 键 8 panic.halt：panic 时停机不重启。来源：panic 行为开关，
    // 与 watchdog 的处置互斥（双弃规则归 F0591）。
    KeySpec { key: "panic.halt", kind: KeyKind::Bool, note: "自设计：panic 停机口径开关" },
    // ── 整数 8 键（与 F0586 INT_TABLE 逐字对齐：键名一致、顺序一致；
    //    值域判归 F0586，本条只做重复记账）──────────────────────────
    // 键 9 loglevel：内核日志级别（F0586 域 [0,7] 步长 1）。
    KeySpec { key: "loglevel", kind: KeyKind::Int, note: "F0586 对齐：INT_TABLE 键 1" },
    // 键 10 uart.baud：串口波特率（F0586 域 [9600, 4000000]）。
    KeySpec { key: "uart.baud", kind: KeyKind::Int, note: "F0586 对齐：INT_TABLE 键 2" },
    // 键 11 console.cols：控制台列数（F0586 域 [20,512]）。
    KeySpec { key: "console.cols", kind: KeyKind::Int, note: "F0586 对齐：INT_TABLE 键 3" },
    // 键 12 console.rows：控制台行数（F0586 域 [10,256]）。
    KeySpec { key: "console.rows", kind: KeyKind::Int, note: "F0586 对齐：INT_TABLE 键 4" },
    // 键 13 heap.mb：内核堆驻留 MB（F0586 域 [64,4096] 步长 64）。
    KeySpec { key: "heap.mb", kind: KeyKind::Int, note: "F0586 对齐：INT_TABLE 键 5" },
    // 键 14 stack.kb：任务内核栈 KB（F0586 域 [8,1024] 步长 8）。
    KeySpec { key: "stack.kb", kind: KeyKind::Int, note: "F0586 对齐：INT_TABLE 键 6" },
    // 键 15 sched.slice.us：调度时间片微秒（F0586 域 [1000,1000000] 步长 100）。
    KeySpec { key: "sched.slice.us", kind: KeyKind::Int, note: "F0586 对齐：INT_TABLE 键 7" },
    // 键 16 dma.win.mb：DMA 窗口预算 MB（F0586 域 [1,1024]）。
    KeySpec { key: "dma.win.mb", kind: KeyKind::Int, note: "F0586 对齐：INT_TABLE 键 8" },
    // ── 字符串 8 键（与 F0586 STR_TABLE 逐字对齐：键名一致、顺序一致；
    //    键长/字符集归 F0586，本条只做重复记账）─────────────────────
    // 键 17 hostname：主机名（F0586 键长 ≤32）。
    KeySpec { key: "hostname", kind: KeyKind::Str, note: "F0586 对齐：STR_TABLE 键 1" },
    // 键 18 console.dev：控制台设备名（F0586 键长 ≤16）。
    KeySpec { key: "console.dev", kind: KeyKind::Str, note: "F0586 对齐：STR_TABLE 键 2" },
    // 键 19 boot.id：引导 ID（F0586 键长 ≤12，小写十六进制闭集）。
    KeySpec { key: "boot.id", kind: KeyKind::Str, note: "F0586 对齐：STR_TABLE 键 3" },
    // 键 20 log.tag：日志标签（F0586 键长 ≤16）。
    KeySpec { key: "log.tag", kind: KeyKind::Str, note: "F0586 对齐：STR_TABLE 键 4" },
    // 键 21 video.mode：视频模式名（F0586 键长 ≤8）。
    KeySpec { key: "video.mode", kind: KeyKind::Str, note: "F0586 对齐：STR_TABLE 键 5" },
    // 键 22 locale：区域名（F0586 键长 ≤8）。
    KeySpec { key: "locale", kind: KeyKind::Str, note: "F0586 对齐：STR_TABLE 键 6" },
    // 键 23 kernel.opt：内核选项名（F0586 键长 ≤24）。
    KeySpec { key: "kernel.opt", kind: KeyKind::Str, note: "F0586 对齐：STR_TABLE 键 7" },
    // 键 24 cmd.prefix：命令前缀（F0586 键长 ≤8）。
    KeySpec { key: "cmd.prefix", kind: KeyKind::Str, note: "F0586 对齐：STR_TABLE 键 8" },
];

/// 布尔真词形闭集（比较口径规范化：4 词形归真位）。
/// 来源：Linux kstrtobool 词族收窄冻结（Linux 收 y/n/t/f 等更多短形，
/// 本表收编常用 8 词形并冻结——短形 t/f 有拼写歧义风险，不入表）。
pub const BOOL_TRUE_WORDS: [&str; 4] = ["true", "on", "1", "yes"];

/// 布尔假词形闭集（比较口径规范化：4 词形归假位）。
pub const BOOL_FALSE_WORDS: [&str; 4] = ["false", "off", "0", "no"];

// ═══════════════════════════════════════════════════════════════════════
// 编译期自审（const fn：只 while+索引+手工字节比较，无 for/get/Ord）
// ═══════════════════════════════════════════════════════════════════════

/// const str 全等（逐字节 while 比较；const 语境禁 derive 的 ==）。
const fn dk_str_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut i = 0;
    while i < ab.len() {
        if ab[i] != bb[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// const str 非空。
const fn dk_str_nonempty(a: &str) -> bool {
    a.as_bytes().len() > 0
}

/// 键表形态自审：24 行齐、每键非空、来源注记非空、三型计数 8/8/8。
const fn key_table_shape_check() -> bool {
    if KEY_TABLE.len() != KEY_N {
        return false;
    }
    let mut bools = 0;
    let mut ints = 0;
    let mut strs = 0;
    let mut i = 0;
    while i < KEY_N {
        if !dk_str_nonempty(KEY_TABLE[i].key) {
            return false;
        }
        if !dk_str_nonempty(KEY_TABLE[i].note) {
            return false;
        }
        // const 语境禁 KeyKind 的 ==：以 u8 判型。
        let k = KEY_TABLE[i].kind as u8;
        if k == 0 {
            bools += 1;
        } else if k == 1 {
            ints += 1;
        } else {
            strs += 1;
        }
        i += 1;
    }
    bools == BOOL_N && ints == INT_N && strs == STR_N
}

/// 键表键名互异自审（24×24 上三角逐对比较；防同键双槽破首现账）。
const fn key_table_unique_check() -> bool {
    let mut i = 0;
    while i < KEY_N {
        let mut j = i + 1;
        while j < KEY_N {
            if dk_str_eq(KEY_TABLE[i].key, KEY_TABLE[j].key) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 布尔词形闭集自审：8 词形互异、非空、且真假两侧无交集
/// （同词形双义会使规范化歧义）。
const fn bool_words_shape_check() -> bool {
    let mut i = 0;
    while i < 4 {
        if !dk_str_nonempty(BOOL_TRUE_WORDS[i]) || !dk_str_nonempty(BOOL_FALSE_WORDS[i]) {
            return false;
        }
        let mut j = 0;
        while j < 4 {
            if dk_str_eq(BOOL_TRUE_WORDS[i], BOOL_FALSE_WORDS[j]) {
                return false;
            }
            j += 1;
        }
        // 真侧内部互异
        let mut j2 = i + 1;
        while j2 < 4 {
            if dk_str_eq(BOOL_TRUE_WORDS[i], BOOL_TRUE_WORDS[j2]) {
                return false;
            }
            j2 += 1;
        }
        // 假侧内部互异
        let mut j3 = i + 1;
        while j3 < 4 {
            if dk_str_eq(BOOL_FALSE_WORDS[i], BOOL_FALSE_WORDS[j3]) {
                return false;
            }
            j3 += 1;
        }
        i += 1;
    }
    true
}

/// 编译期锁：键表形态 + 键名互异 + 布尔词形闭集全过（破表即编译失败）。
const _: () = assert!(key_table_shape_check());
const _: () = assert!(key_table_unique_check());
const _: () = assert!(bool_words_shape_check());

// ═══════════════════════════════════════════════════════════════════════
// 键空间查询（运行期；查表 O(24×字节)，无哈希无分配）
// ═══════════════════════════════════════════════════════════════════════

/// 键查表：命中返回键位（0..24），未命中 None。
pub fn key_index(key: &str) -> Option<usize> {
    let mut i = 0;
    while i < KEY_N {
        if dk_str_eq(KEY_TABLE[i].key, key) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 键型查询（白名单外返回 None——与 key_index 同口径）。
pub fn key_kind_of(key: &str) -> Option<KeyKind> {
    match key_index(key) {
        Some(i) => Some(KEY_TABLE[i].kind),
        None => None,
    }
}

/// 键名查询（键位 → 键名；越界返回空串——查询面零 panic）。
pub fn key_name(idx: usize) -> &'static str {
    if idx < KEY_N {
        KEY_TABLE[idx].key
    } else {
        ""
    }
}

/// 白名单成员判定（F0583 口径：在 24 键闭集内 = 白名单内）。
pub fn is_whitelisted(key: &str) -> bool {
    key_index(key).is_some()
}

/// 键位合法性卫（供簿查询 API 前置校验）。
fn key_idx_guard(idx: usize) -> Result<usize, DupKeyError> {
    if idx >= KEY_N {
        // 键位越界与簿位越界同族：SlotOutOfRange 口径。
        Err(DupKeyError::SlotOutOfRange)
    } else {
        Ok(idx)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 布尔词形规范化（比较口径：8 词形 → 2 规范位）
// ═══════════════════════════════════════════════════════════════════════

/// 布尔词形规范化：命中真侧 4 词形 → true，假侧 4 词形 → false，
/// 闭集外 → BoolValInvalid（中文诊断建议推荐词形）。
///
/// 这是「同值重复 vs 异值重复」分列的关键：布尔键 "on" 与 "true" 原文
/// 不同但语义同一——按规范化位比较（同值）；而 "true" 与 "false" 是
/// 真异值。整数/字符串键不走规范化，按原文逐字节比较（口径见
/// `compare_note()`）。
pub fn canonical_bool(raw: &str) -> Result<bool, DupKeyError> {
    let mut i = 0;
    while i < BOOL_TRUE_WORDS.len() {
        if dk_str_eq(BOOL_TRUE_WORDS[i], raw) {
            return Ok(true);
        }
        i += 1;
    }
    let mut j = 0;
    while j < BOOL_FALSE_WORDS.len() {
        if dk_str_eq(BOOL_FALSE_WORDS[j], raw) {
            return Ok(false);
        }
        j += 1;
    }
    Err(DupKeyError::BoolValInvalid)
}

/// 同异比较口径注记（报告与文档渲染用——口径是账的一部分）。
pub fn compare_note() -> &'static str {
    "比较口径：布尔键按规范化词形位比较（on≡true、off≡false）；整数/字符串键按原文逐字节比较（0400 与 400 视为异值——数值等价归 F0586 值域核重写层，不归本条）"
}
// ═══════════════════════════════════════════════════════════════════════
// 定长值单元（ValCell：64 字节内联 + 1 字节长；Copy、零堆）
// ═══════════════════════════════════════════════════════════════════════

/// 定长值单元：原始值字节逐字节保真（含 NUL 与非 ASCII——渲染时
/// lossy 转义，比较时逐字节）。上限 VAL_MAX=64（F0589 token 界），
/// 超限在 store 时显性 ValueTooLong，单元保持原状。
#[derive(Clone, Copy, Debug)]
pub struct ValCell {
    /// 原始字节（定长 64，尾部零填充不入账）。
    pub b: [u8; VAL_MAX],
    /// 有效字节数（≤ 64）。
    pub len: u8,
}

impl ValCell {
    /// 空单元（const 构造：全零 + len 0）。
    pub const fn empty() -> ValCell {
        ValCell { b: [0; VAL_MAX], len: 0 }
    }

    /// 就地写入（超 64 字节显性拒收，单元内容不变）。
    pub fn store(&mut self, s: &str) -> Result<(), DupKeyError> {
        let bytes = s.as_bytes();
        if bytes.len() > VAL_MAX {
            return Err(DupKeyError::ValueTooLong);
        }
        let mut i = 0;
        while i < bytes.len() {
            self.b[i] = bytes[i];
            i += 1;
        }
        self.len = bytes.len() as u8;
        Ok(())
    }

    /// 值单元构造（拷贝语义，From<&str> 的 Result 版）。
    pub fn from_str(s: &str) -> Result<ValCell, DupKeyError> {
        let mut c = ValCell::empty();
        c.store(s)?;
        Ok(c)
    }

    /// 有效字节数。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空值（零有效字节）。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 有效字节切片（尾零不入）。
    pub fn as_slice(&self) -> &[u8] {
        &self.b[..self.len()]
    }

    /// 与字符串逐字节比较。
    pub fn eq_str(&self, s: &str) -> bool {
        self.as_slice() == s.as_bytes()
    }

    /// 与另一单元逐字节比较。
    pub fn eq_cell(&self, o: &ValCell) -> bool {
        self.as_slice() == o.as_slice()
    }

    /// 渲染（UTF-8 有效即原样；无效走 lossy 转义——零 panic 口径）。
    pub fn render(&self) -> String {
        match core::str::from_utf8(self.as_slice()) {
            Ok(s) => s.to_string(),
            Err(_) => lossy_bytes(self.as_slice()),
        }
    }
}

/// 字节串 lossy 渲染：可打印 ASCII 原样，其余 \xHH 转义
/// （保真不丢字节——账面渲染与原文可互证）。
pub fn lossy_bytes(b: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c >= 0x20 && c <= 0x7e {
            out.push(c as char);
        } else {
            out.push_str(&format!("\\x{:02x}", c));
        }
        i += 1;
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════
// FNV-1a 64 位指纹（常量与公开向量锁基）
// ═══════════════════════════════════════════════════════════════════════

/// FNV-1a 64 位偏移基（来源：FNV 公开参数集 14695981039346656037）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 位素数（来源：FNV 公开参数集 1099511628211）。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// FNV-1a 64 位（wrapping 乘加是定义性行为，非溢出隐患）。
pub fn fnv64(data: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

// ═══════════════════════════════════════════════════════════════════════
// 事件账（16 事件类 + 32 槽滑窗）
// ═══════════════════════════════════════════════════════════════════════

/// 事件类（u8 闭集，16 类；逐类中文——事件账是人查的第一入口）。
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 首现入账（键第一次被接受 → 生效表写入）。
    FirstWin = 0,
    /// 同值重复忽略（后现与生效值同——按比较口径判同）。
    DupSame = 1,
    /// 异值重复忽略（后现与生效值异）。
    DupDiff = 2,
    /// 未知键拒收（白名单外第一次出现，F0584 显性弃置口径）。
    UnknownKey = 3,
    /// 双重违规（同一未知键再次出现：白名单外 + 重复）。
    DoubleViolation = 4,
    /// 布尔词形闭集外拒收。
    BoolValReject = 5,
    /// 值超长拒收（>64 字节）。
    ValTooLongReject = 6,
    /// 键超长快速拒收（>24 字节）。
    KeyTooLongReject = 7,
    /// 环形满拒收（64 槽占满）。
    RingOverflow = 8,
    /// 封后写拦截（seal 后 push 被拒）。
    SealedWrite = 9,
    /// 封流（seal——账定）。
    Sealed = 10,
    /// 快照已拍（一拍槽用掉）。
    SnapshotTaken = 11,
    /// 双跑对拍完成（aux=0 全等 / 1 失配）。
    DualRun = 12,
    /// 注入面完成（aux=检出数）。
    InjectFace = 13,
    /// 账目恒等式自审通过。
    AuditPass = 14,
    /// 账目恒等式自审失败。
    AuditFail = 15,
}

impl EventKind {
    /// 事件类中文名（人查口径）。
    pub const fn text(self) -> &'static str {
        match self {
            EventKind::FirstWin => "首现入账",
            EventKind::DupSame => "同值重复忽略",
            EventKind::DupDiff => "异值重复忽略",
            EventKind::UnknownKey => "未知键拒收",
            EventKind::DoubleViolation => "双重违规",
            EventKind::BoolValReject => "布尔词形拒收",
            EventKind::ValTooLongReject => "值超长拒收",
            EventKind::KeyTooLongReject => "键超长拒收",
            EventKind::RingOverflow => "环形满拒收",
            EventKind::SealedWrite => "封后写拦截",
            EventKind::Sealed => "封流",
            EventKind::SnapshotTaken => "快照已拍",
            EventKind::DualRun => "双跑对拍",
            EventKind::InjectFace => "注入面",
            EventKind::AuditPass => "自审通过",
            EventKind::AuditFail => "自审失败",
        }
    }
}

/// 事件条目（Copy 紧凑：tick + 类 + 键位 + 辅助数）。
#[derive(Clone, Copy, Debug)]
pub struct Event {
    /// 账内时序（wrapping 递增——只序不刻钟）。
    pub tick: u32,
    /// 事件类。
    pub kind: EventKind,
    /// 相关键位（无关事件用 0xFF 哨兵）。
    pub key_idx: u8,
    /// 辅助数（如重复第几现、注入检出数）。
    pub aux: u16,
}

/// 键位哨兵（事件与键无关时占用；恒 ≥ KEY_N 可辨）。
pub const KEY_IDX_NONE: u8 = 0xFF;

/// 事件账 32 槽滑窗（满后滑出最旧并计 dropped——账不丢数只丢明细）。
#[derive(Clone, Debug)]
pub struct EventRing {
    v: Vec<Event>,
    dropped: u32,
}

impl EventRing {
    /// 构造（预分配 EVENT_CAP 槽）。
    pub fn new() -> EventRing {
        EventRing { v: Vec::with_capacity(EVENT_CAP), dropped: 0 }
    }

    /// 入账（满则滑出最旧；tick 由调用方账本递增后传入）。
    pub fn push(&mut self, e: Event) {
        if self.v.len() >= EVENT_CAP {
            self.v.remove(0);
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.v.push(e);
    }

    /// 在账明细条数（≤ EVENT_CAP）。
    pub fn len(&self) -> usize {
        self.v.len()
    }

    /// 是否无在账明细。
    pub fn is_empty(&self) -> bool {
        self.v.is_empty()
    }

    /// 滑出丢弃计数（明细丢数不丢账）。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 只读访问（时序序）。
    pub fn all(&self) -> &[Event] {
        &self.v
    }

    /// 按类过滤（人查：先看违规类）。
    pub fn of_kind(&self, k: EventKind) -> Vec<Event> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.v.len() {
            if self.v[i].kind == k {
                out.push(self.v[i]);
            }
            i += 1;
        }
        out
    }

    /// 事件账渲染（中文逐行：「tick 事件类 键 辅助」）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < self.v.len() {
            let e = self.v[i];
            let kname = if e.key_idx == KEY_IDX_NONE {
                "-".to_string()
            } else {
                key_name(e.key_idx as usize).to_string()
            };
            out.push_str(&format!(
                "tick{} {} {} aux={}\n",
                e.tick,
                e.kind.text(),
                kname,
                e.aux
            ));
            i += 1;
        }
        if self.dropped > 0 {
            out.push_str(&format!("（另有 {} 条滑出明细未呈现）\n", self.dropped));
        }
        out
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 推入结果 / 重复簿条目 / 未知键复现账
// ═══════════════════════════════════════════════════════════════════════

/// 推入结果（Copy：键位 + 是否首现 + 全局序 + 该键第几现 + 重复第几笔）。
#[derive(Clone, Copy, Debug)]
pub struct PushOutcome {
    /// 键位（0..24）。
    pub key_idx: u8,
    /// 是否首现（true=进生效表；false=重复忽略入簿）。
    pub first_win: bool,
    /// 全局出现序（流内第几个登记，1 起）。
    pub ordinal: u32,
    /// 该键第几现（1=首现，2=第一笔重复……）。
    pub occurrence: u16,
    /// 重复第几笔（首现时 0）。
    pub dup_no: u16,
}

impl PushOutcome {
    /// 结果一句话（中文：首现入账/重复忽略第 n 笔）。
    pub fn text(&self) -> String {
        let kname = key_name(self.key_idx as usize);
        if self.first_win {
            format!("{} 首现入账（全局序{}，生效）", kname, self.ordinal)
        } else {
            format!(
                "{} 第{}现忽略（全局序{}，重复第{}笔入簿）",
                kname, self.occurrence, self.ordinal, self.dup_no
            )
        }
    }
}

/// 重复簿条目（重复簿可查的原子：键 + 后现原文 + 出现位置 + 第几现 +
/// 与生效值同异标注——「键→重复值清单+每次出现位置」的清单单元）。
#[derive(Clone, Copy, Debug)]
pub struct DupEntry {
    /// 全局出现序（流内位置，1 起——「每次出现位置」）。
    pub ordinal: u32,
    /// 键位。
    pub key_idx: u8,
    /// 该键第几现（恒 ≥ 2：首现不入重复簿）。
    pub occurrence: u16,
    /// 后现原文（逐字节保真）。
    pub raw: ValCell,
    /// 与生效值（=首现值）是否同值（按 compare_note 比较口径）。
    pub same_as_effective: bool,
}

impl DupEntry {
    /// 条目一句话（中文）。
    pub fn text(&self) -> String {
        let kname = key_name(self.key_idx as usize);
        let tag = if self.same_as_effective { "同值" } else { "异值" };
        format!(
            "{} 第{}现@序{} 值「{}」（{}重复）",
            kname,
            self.occurrence,
            self.ordinal,
            self.raw.render(),
            tag
        )
    }
}

/// 未知键复现账条目（双重违规分账的数据面：白名单外键按截断保存
/// 记首现/末现序与次数；count ≥ 2 即双重违规）。
#[derive(Clone, Copy, Debug)]
pub struct UnknownRepeat {
    /// 键名截断保存（≤ KEY_KEEP=24 字节，超长键走 KeyTooLong 不入此账）。
    pub kept: [u8; KEY_KEEP],
    /// 保存的有效字节数。
    pub kept_len: u8,
    /// 出现次数（含首现；≥ 2 为双重违规）。
    pub count: u16,
    /// 首现全局序。
    pub first_ordinal: u32,
    /// 末现全局序。
    pub last_ordinal: u32,
}

impl UnknownRepeat {
    /// 键名渲染（lossy 转义保真）。
    pub fn key_render(&self) -> String {
        lossy_bytes(&self.kept[..self.kept_len as usize])
    }

    /// 是否双重违规（白名单外 + 重复）。
    pub fn is_double_violation(&self) -> bool {
        self.count >= 2
    }

    /// 条目一句话（中文）。
    pub fn text(&self) -> String {
        let tag = if self.is_double_violation() { "双重违规" } else { "单次弃置" };
        format!(
            "未知键「{}」出现{}次（首@序{} 末@序{}）——{}",
            self.key_render(),
            self.count,
            self.first_ordinal,
            self.last_ordinal,
            tag
        )
    }
}
// ═══════════════════════════════════════════════════════════════════════
// 首现口径账主体（FirstWinLedger）
// ═══════════════════════════════════════════════════════════════════════

/// 白名单联动审计报告（重复簿 ⊆ 白名单 的证明面 + 双重违规分账数）。
#[derive(Clone, Debug)]
pub struct WhitelistAudit {
    /// 重复簿条目总数（全部应在白名单键空间内）。
    pub dup_entries: u32,
    /// 落白名单外的重复簿条目数（结构性应为 0——未知键进不了重复簿）。
    pub outside_whitelist: u32,
    /// 白名单外重复（双重违规）笔数——独立分账，不混入重复簿。
    pub double_violations: u32,
    /// 联动是否成立（outside_whitelist == 0）。
    pub ok: bool,
}

/// 账目恒等式自审报告（逐恒等式（名, 过否）行 + 总判定）。
#[derive(Clone, Debug)]
pub struct AuditReport {
    /// 恒等式行（名 + 是否成立）。
    pub rows: Vec<(&'static str, bool)>,
    /// 全部恒等式成立。
    pub ok: bool,
}

/// 三现注入自证面（J 判据核心断言的结构化产物）。
#[derive(Clone, Debug)]
pub struct TripleFace {
    /// 注入出现次数（判据：3）。
    pub occurrences: u32,
    /// 生效表条目数（判据：1/1——一键一槽一值）。
    pub effective_entries: u32,
    /// 生效值是否等于首现值（首现口径本证）。
    pub effective_is_first: bool,
    /// 已计重复数（判据：2）。
    pub dup_counted: u32,
    /// 应计重复数（出现数 - 1）。
    pub dup_expected: u32,
    /// 断言面总判定（1/1 + 2/2 + 生效=首现 全过）。
    pub pass: bool,
    /// 一句话结论（中文）。
    pub line: String,
}

/// 首现口径账主体：生效表 + 每键重复计数 + 同值/异值分列 + 重复簿 +
/// 未知键复现账 + 事件账 + 恒等式自审 + 健康分 + 指纹。
///
/// 内联尺寸 ≈ 1.9KB（生效 24×65 + 计数数组），远低于 64KB 上栈红线；
/// 重复簿/未知账/事件明细走 Vec 堆。
#[derive(Clone, Debug)]
pub struct FirstWinLedger {
    /// 生效表值（24 槽；present=false 槽无效）。
    effective: [ValCell; KEY_N],
    /// 生效表占用位（首现时置位，此后只读——首现口径的结构保证）。
    present: [bool; KEY_N],
    /// 每键首现全局序。
    first_ordinal: [u32; KEY_N],
    /// 每键重复计数（后现次数）。
    dup_count: [u16; KEY_N],
    /// 总重复数（= 重复簿条数）。
    total_dups: u32,
    /// 同值重复数（按比较口径与生效值同）。
    same_val_dups: u32,
    /// 异值重复数。
    diff_val_dups: u32,
    /// 重复簿（全局序顺序 append）。
    dup_book: Vec<DupEntry>,
    /// 未知键复现账（≤ UNKNOWN_CAP 条，满后计数不增条）。
    unknown_ledger: Vec<UnknownRepeat>,
    /// 未知账槽满后未能立账的出现数（账不丢数）。
    unknown_overflow: u32,
    /// 事件账滑窗。
    events: EventRing,
    // ── 计数器组（wrapping 递增）──────────────────────────────────
    /// 首现入账数（= 生效表占用数）。
    first_wins: u32,
    /// 重复忽略数（= total_dups，恒等式复核对）。
    dup_ignored: u32,
    /// 未知键拒收数。
    unknown_rejects: u32,
    /// 布尔词形闭集外拒收数。
    bool_rejects: u32,
    /// 值超长拒收数。
    val_len_rejects: u32,
    /// 键超长快速拒收数。
    key_len_rejects: u32,
    /// 环形满拒收数。
    ring_overflows: u32,
    /// 封后写拦截数。
    sealed_writes: u32,
    /// 总尝试数（一切 push 意图，含拒收）。
    total_seen: u32,
    /// 账内时序（wrapping）。
    ticks: u32,
    /// 最近一次自审结果（None=未审）。
    last_audit_ok: Option<bool>,
}

impl FirstWinLedger {
    /// 构造（全零账面；Vec 预分配到位）。
    pub fn new() -> FirstWinLedger {
        FirstWinLedger {
            effective: [ValCell::empty(); KEY_N],
            present: [false; KEY_N],
            first_ordinal: [0; KEY_N],
            dup_count: [0; KEY_N],
            total_dups: 0,
            same_val_dups: 0,
            diff_val_dups: 0,
            dup_book: Vec::with_capacity(DUP_BOOK_CAP),
            unknown_ledger: Vec::with_capacity(UNKNOWN_CAP),
            unknown_overflow: 0,
            events: EventRing::new(),
            first_wins: 0,
            dup_ignored: 0,
            unknown_rejects: 0,
            bool_rejects: 0,
            val_len_rejects: 0,
            key_len_rejects: 0,
            ring_overflows: 0,
            sealed_writes: 0,
            total_seen: 0,
            ticks: 0,
            last_audit_ok: None,
        }
    }

    /// 首现口径声明原文（const 冻结语义的公开只读面）。
    pub fn first_win_semantics(&self) -> &'static str {
        FIRST_WIN_SEMANTICS
    }

    /// 账内时序。
    pub fn tick(&self) -> u32 {
        self.ticks
    }

    /// 时序递增（私有：一切入账路径统一走此口）。
    fn tick_inc(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
    }

    // ── 入账路径 ───────────────────────────────────────────────────

    /// 同异比较（比较口径见 compare_note()）：布尔按规范化位，其余按
    /// 原文逐字节。UTF-8 或词形闭集失效（理论不可达）回退字节比较。
    fn canon_eq(&self, key_idx: usize, a: &ValCell, b: &ValCell) -> bool {
        if KEY_TABLE[key_idx].kind == KeyKind::Bool {
            match (
                core::str::from_utf8(a.as_slice()),
                core::str::from_utf8(b.as_slice()),
            ) {
                (Ok(x), Ok(y)) => match (canonical_bool(x), canonical_bool(y)) {
                    (Ok(p), Ok(q)) => p == q,
                    _ => a.eq_cell(b),
                },
                _ => a.eq_cell(b),
            }
        } else {
            a.eq_cell(b)
        }
    }

    /// 接受入账（流已过五卫：封流/键空/键长/闭集/值长+布尔词形）。
    /// 首现 → 写生效表；后现 → 忽略入重复簿（首现口径本证路径）。
    pub fn record_accepted(
        &mut self,
        key_idx: usize,
        raw: &ValCell,
        ordinal: u32,
    ) -> Result<PushOutcome, DupKeyError> {
        let idx = key_idx_guard(key_idx)?;
        self.tick_inc();
        self.total_seen = self.total_seen.wrapping_add(1);
        if !self.present[idx] {
            // 首现：生效表唯一写入点（此后只读——生效值恒等于首现值）。
            self.present[idx] = true;
            self.effective[idx] = *raw;
            self.first_ordinal[idx] = ordinal;
            self.first_wins = self.first_wins.wrapping_add(1);
            self.events.push(Event {
                tick: self.ticks,
                kind: EventKind::FirstWin,
                key_idx: idx as u8,
                aux: 1,
            });
            Ok(PushOutcome {
                key_idx: idx as u8,
                first_win: true,
                ordinal,
                occurrence: 1,
                dup_no: 0,
            })
        } else {
            // 后现：全量忽略但逐笔入簿。
            let occurrence = self.dup_count[idx].saturating_add(2); // 第几现
            let same = self.canon_eq(idx, &self.effective[idx], raw);
            self.dup_count[idx] = self.dup_count[idx].wrapping_add(1);
            self.total_dups = self.total_dups.wrapping_add(1);
            self.dup_ignored = self.dup_ignored.wrapping_add(1);
            if same {
                self.same_val_dups = self.same_val_dups.wrapping_add(1);
            } else {
                self.diff_val_dups = self.diff_val_dups.wrapping_add(1);
            }
            let dup_no = self.dup_count[idx];
            self.dup_book.push(DupEntry {
                ordinal,
                key_idx: idx as u8,
                occurrence: occurrence as u16,
                raw: *raw,
                same_as_effective: same,
            });
            self.events.push(Event {
                tick: self.ticks,
                kind: if same { EventKind::DupSame } else { EventKind::DupDiff },
                key_idx: idx as u8,
                aux: dup_no,
            });
            Ok(PushOutcome {
                key_idx: idx as u8,
                first_win: false,
                ordinal,
                occurrence: occurrence as u16,
                dup_no,
            })
        }
    }

    /// 未知键入账（白名单外——F0584 显性弃置承接；同键再现升双重违规）。
    pub fn record_unknown(&mut self, key: &str, ordinal: u32) {
        self.tick_inc();
        self.total_seen = self.total_seen.wrapping_add(1);
        self.unknown_rejects = self.unknown_rejects.wrapping_add(1);
        // 查既有未知账（字节级比较，截断保真口径一致）。
        let bytes = key.as_bytes();
        let mut found: Option<usize> = None;
        let mut i = 0;
        while i < self.unknown_ledger.len() {
            let e = &self.unknown_ledger[i];
            if e.kept_len as usize == bytes.len() {
                let mut j = 0;
                let mut eq = true;
                while j < bytes.len() {
                    if e.kept[j] != bytes[j] {
                        eq = false;
                        break;
                    }
                    j += 1;
                }
                if eq {
                    found = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match found {
            Some(i) => {
                self.unknown_ledger[i].count = self.unknown_ledger[i].count.wrapping_add(1);
                self.unknown_ledger[i].last_ordinal = ordinal;
                // 第二现起：双重违规（白名单外 + 重复）。
                self.events.push(Event {
                    tick: self.ticks,
                    kind: EventKind::DoubleViolation,
                    key_idx: KEY_IDX_NONE,
                    aux: self.unknown_ledger[i].count,
                });
            }
            None => {
                if self.unknown_ledger.len() < UNKNOWN_CAP {
                    let mut kept = [0u8; KEY_KEEP];
                    let n = if bytes.len() < KEY_KEEP { bytes.len() } else { KEY_KEEP };
                    let mut j = 0;
                    while j < n {
                        kept[j] = bytes[j];
                        j += 1;
                    }
                    self.unknown_ledger.push(UnknownRepeat {
                        kept,
                        kept_len: n as u8,
                        count: 1,
                        first_ordinal: ordinal,
                        last_ordinal: ordinal,
                    });
                    self.events.push(Event {
                        tick: self.ticks,
                        kind: EventKind::UnknownKey,
                        key_idx: KEY_IDX_NONE,
                        aux: 1,
                    });
                } else {
                    // 账槽满：计数不立账（数不丢）。
                    self.unknown_overflow = self.unknown_overflow.wrapping_add(1);
                    self.events.push(Event {
                        tick: self.ticks,
                        kind: EventKind::UnknownKey,
                        key_idx: KEY_IDX_NONE,
                        aux: 0, // aux=0 标记未立账
                    });
                }
            }
        }
    }

    /// 布尔词形闭集外拒收入账。
    pub fn record_bool_reject(&mut self, key_idx: u8) {
        self.tick_inc();
        self.total_seen = self.total_seen.wrapping_add(1);
        self.bool_rejects = self.bool_rejects.wrapping_add(1);
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::BoolValReject,
            key_idx,
            aux: 0,
        });
    }

    /// 值超长拒收入账。
    pub fn record_val_len_reject(&mut self, key_idx: u8) {
        self.tick_inc();
        self.total_seen = self.total_seen.wrapping_add(1);
        self.val_len_rejects = self.val_len_rejects.wrapping_add(1);
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::ValTooLongReject,
            key_idx,
            aux: 0,
        });
    }

    /// 键形态拒收入账（aux=1 超长快速拒收 / aux=2 空键；两类同族
    /// 共用计数器——「键形态卫」，事件 aux 区分）。
    pub fn record_key_len_reject(&mut self, aux: u16) {
        self.tick_inc();
        self.total_seen = self.total_seen.wrapping_add(1);
        self.key_len_rejects = self.key_len_rejects.wrapping_add(1);
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::KeyTooLongReject,
            key_idx: KEY_IDX_NONE,
            aux,
        });
    }

    /// 环形满拒收入账。
    pub fn record_ring_overflow(&mut self, key_idx: u8) {
        self.tick_inc();
        self.total_seen = self.total_seen.wrapping_add(1);
        self.ring_overflows = self.ring_overflows.wrapping_add(1);
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::RingOverflow,
            key_idx,
            aux: 0,
        });
    }

    /// 封后写拦截入账（100% 拦截面）。
    pub fn record_sealed_write(&mut self, key_idx: u8) {
        self.tick_inc();
        self.total_seen = self.total_seen.wrapping_add(1);
        self.sealed_writes = self.sealed_writes.wrapping_add(1);
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::SealedWrite,
            key_idx,
            aux: 0,
        });
    }

    /// 封流入账（账定）。
    pub fn record_seal(&mut self) {
        self.tick_inc();
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::Sealed,
            key_idx: KEY_IDX_NONE,
            aux: self.effective_count() as u16,
        });
    }

    /// 快照已拍入账（一拍槽）。
    pub fn record_snapshot(&mut self) {
        self.tick_inc();
        self.events.push(Event {
            tick: self.ticks,
            kind: EventKind::SnapshotTaken,
            key_idx: KEY_IDX_NONE,
            aux: self.effective_count() as u16,
        });
    }

    // ── 查询面（重复簿可查）───────────────────────────────────────

    /// 生效表占用数。
    pub fn effective_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < KEY_N {
            if self.present[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 键是否已首现（生效表占用）。
    pub fn is_present(&self, key: &str) -> bool {
        match key_index(key) {
            Some(i) => self.present[i],
            None => false,
        }
    }

    /// 键是否已首现（键位口径；越界恒 false——查询面零 panic）。
    pub fn present_idx(&self, idx: usize) -> bool {
        if idx < KEY_N {
            self.present[idx]
        } else {
            false
        }
    }

    /// 每键重复计数表只读副本（24 槽全拷——快照构造用）。
    pub fn dup_count_table(&self) -> [u16; KEY_N] {
        self.dup_count
    }

    /// 生效值查询（键名口径；未首现 → KeyNotFound）。
    pub fn effective_cell(&self, key: &str) -> Result<ValCell, DupKeyError> {
        match key_index(key) {
            Some(i) => {
                if self.present[i] {
                    Ok(self.effective[i])
                } else {
                    Err(DupKeyError::KeyNotFound)
                }
            }
            None => Err(DupKeyError::KeyNotFound),
        }
    }

    /// 生效值查询（键位口径；越界 → SlotOutOfRange，未首现 → KeyNotFound）。
    pub fn effective_idx(&self, idx: usize) -> Result<ValCell, DupKeyError> {
        let i = key_idx_guard(idx)?;
        if self.present[i] {
            Ok(self.effective[i])
        } else {
            Err(DupKeyError::KeyNotFound)
        }
    }

    /// 生效值渲染（中文账面友好）。
    pub fn effective_str(&self, key: &str) -> Result<String, DupKeyError> {
        Ok(self.effective_cell(key)?.render())
    }

    /// 每键首现全局序（键未首现 → KeyNotFound）。
    pub fn first_ordinal_of(&self, key: &str) -> Result<u32, DupKeyError> {
        match key_index(key) {
            Some(i) => {
                if self.present[i] {
                    Ok(self.first_ordinal[i])
                } else {
                    Err(DupKeyError::KeyNotFound)
                }
            }
            None => Err(DupKeyError::KeyNotFound),
        }
    }

    /// 已生效键清单（键表序——确定性渲染口径）。
    pub fn present_keys(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < KEY_N {
            if self.present[i] {
                out.push(KEY_TABLE[i].key);
            }
            i += 1;
        }
        out
    }

    /// 某键重复计数（键在表内恒有答——未现键 0 重复；表外 KeyNotFound）。
    pub fn dup_count_of(&self, key: &str) -> Result<u32, DupKeyError> {
        match key_index(key) {
            Some(i) => Ok(self.dup_count[i] as u32),
            None => Err(DupKeyError::KeyNotFound),
        }
    }

    /// 某键总现数（首现 1 + 重复 n；表外 KeyNotFound）。
    pub fn occurrences_of_key(&self, key: &str) -> Result<u32, DupKeyError> {
        match key_index(key) {
            Some(i) => {
                if self.present[i] {
                    Ok(self.dup_count[i] as u32 + 1)
                } else {
                    Ok(0)
                }
            }
            None => Err(DupKeyError::KeyNotFound),
        }
    }

    /// 总重复数。
    pub fn total_duplicates(&self) -> u32 {
        self.total_dups
    }

    /// 同值重复数（按比较口径）。
    pub fn same_val_dups(&self) -> u32 {
        self.same_val_dups
    }

    /// 异值重复数。
    pub fn diff_val_dups(&self) -> u32 {
        self.diff_val_dups
    }

    /// 重复簿条数。
    pub fn dup_book_len(&self) -> usize {
        self.dup_book.len()
    }

    /// 重复簿整簿只读（全局序顺序）。
    pub fn dup_book_all(&self) -> &[DupEntry] {
        &self.dup_book
    }

    /// 重复簿逐条查询（越界 → SlotOutOfRange）。
    pub fn dup_entry(&self, i: usize) -> Result<DupEntry, DupKeyError> {
        if i < self.dup_book.len() {
            Ok(self.dup_book[i])
        } else {
            Err(DupKeyError::SlotOutOfRange)
        }
    }

    /// 键 → 重复值清单 + 每次出现位置（重复簿可查主面；表外键
    /// KeyNotFound；表内无重复键返回空清单——空清单是合法答案）。
    pub fn dup_book_of(&self, key: &str) -> Result<Vec<DupEntry>, DupKeyError> {
        let i = match key_index(key) {
            Some(i) => i,
            None => return Err(DupKeyError::KeyNotFound),
        };
        let mut out = Vec::new();
        let mut j = 0;
        while j < self.dup_book.len() {
            if self.dup_book[j].key_idx as usize == i {
                out.push(self.dup_book[j]);
            }
            j += 1;
        }
        Ok(out)
    }

    /// 有重复的键清单（键表序）。
    pub fn dup_keys(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < KEY_N {
            if self.dup_count[i] > 0 {
                out.push(KEY_TABLE[i].key);
            }
            i += 1;
        }
        out
    }

    /// 未知键复现账只读。
    pub fn unknown_repeats(&self) -> &[UnknownRepeat] {
        &self.unknown_ledger
    }

    /// 双重违规笔数（白名单外重复）。
    pub fn double_violation_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.unknown_ledger.len() {
            if self.unknown_ledger[i].is_double_violation() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 未知账出现数折算（Σ 复现账 count + 槽满溢出数；供恒等式对账）。
    fn unknown_occurrences_sum(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.unknown_ledger.len() {
            n = n.wrapping_add(self.unknown_ledger[i].count as u32);
            i += 1;
        }
        n.wrapping_add(self.unknown_overflow)
    }

    /// 未知账槽满溢出数。
    pub fn unknown_overflow(&self) -> u32 {
        self.unknown_overflow
    }

    /// 事件账只读。
    pub fn events(&self) -> &EventRing {
        &self.events
    }

    /// 计数器组只读（健康分与审计渲染用）。
    pub fn counters(&self) -> LedgerCounters {
        LedgerCounters {
            first_wins: self.first_wins,
            dup_ignored: self.dup_ignored,
            unknown_rejects: self.unknown_rejects,
            bool_rejects: self.bool_rejects,
            val_len_rejects: self.val_len_rejects,
            key_len_rejects: self.key_len_rejects,
            ring_overflows: self.ring_overflows,
            sealed_writes: self.sealed_writes,
            total_seen: self.total_seen,
        }
    }

    // ── 断言面 ─────────────────────────────────────────────────────

    /// 首现断言面（键名口径）：该键（生效条目数, 总现数, 重复计数,
    /// 生效值是否等于首现值）。生效值=首现值是结构保证，此面对外可查。
    pub fn first_win_face(&self, key: &str) -> Result<(u32, u32, u32, bool), DupKeyError> {
        let i = match key_index(key) {
            Some(i) => i,
            None => return Err(DupKeyError::KeyNotFound),
        };
        if !self.present[i] {
            return Err(DupKeyError::KeyNotFound);
        }
        let eff_n: u32 = 1; // 一键一槽一值（结构性 1）
        let occ = self.dup_count[i] as u32 + 1;
        Ok((eff_n, occ, self.dup_count[i] as u32, true))
    }

    /// 三现注入自证（J 判据核心面）：在 scratch 账本上注入同键三现
    /// （v1/v2/v3），断言 生效 1/1 + 重复计数 2/2 + 生效值=首现值。
    /// 主账面零染（scratch 独立构造，事件不入主账）。
    pub fn triple_inject_face(key: &str, v1: &str, v2: &str, v3: &str) -> Result<TripleFace, DupKeyError> {
        let idx = key_index(key).ok_or(DupKeyError::UnknownKey)?;
        let mut scratch = FirstWinLedger::new();
        let c1 = ValCell::from_str(v1)?;
        let c2 = ValCell::from_str(v2)?;
        let c3 = ValCell::from_str(v3)?;
        scratch.record_accepted(idx, &c1, 1)?;
        scratch.record_accepted(idx, &c2, 2)?;
        scratch.record_accepted(idx, &c3, 3)?;
        let occurrences = 3;
        let effective_entries = if scratch.present[idx] { 1 } else { 0 };
        let eff = scratch.effective[idx];
        let effective_is_first = eff.eq_cell(&c1);
        let dup_counted = scratch.dup_count[idx] as u32;
        let dup_expected = occurrences - 1;
        let pass = occurrences == 3
            && effective_entries == 1
            && effective_is_first
            && dup_counted == dup_expected;
        let line = format!(
            "键 {} 注入三现（{}/{}/{}）：生效 {}，生效值=首现值 {}，重复计数 {}/{} —— {}",
            key,
            v1,
            v2,
            v3,
            if effective_entries == 1 { "1/1" } else { "0/1" },
            if effective_is_first { "是" } else { "否" },
            dup_counted,
            dup_expected,
            if pass { "通过" } else { "失败" }
        );
        Ok(TripleFace {
            occurrences,
            effective_entries,
            effective_is_first,
            dup_counted,
            dup_expected,
            pass,
            line,
        })
    }

    // ── 人话报告面 ─────────────────────────────────────────────────

    /// 逐键重复报告行（键表序；只含有重复的键）。
    /// 行式：「键 X 出现 n 次，生效值=首次值 v1，后现值 v2（异值）/v3（同值）被忽略」。
    pub fn dup_report_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < KEY_N {
            if self.dup_count[i] == 0 {
                i += 1;
                continue;
            }
            let key = KEY_TABLE[i].key;
            let n = self.dup_count[i] as u32 + 1;
            let v1 = self.effective[i].render();
            // 后现值清单（全局序顺序 + 同异标注）。
            let mut laters = String::new();
            let mut j = 0;
            while j < self.dup_book.len() {
                let e = &self.dup_book[j];
                if e.key_idx as usize == i {
                    if !laters.is_empty() {
                        laters.push('/');
                    }
                    let tag = if e.same_as_effective { "同值" } else { "异值" };
                    laters.push_str(&format!("{}（{}）", e.raw.render(), tag));
                }
                j += 1;
            }
            out.push(format!(
                "键 {} 出现 {} 次，生效值=首次值 {}，后现值 {} 被忽略",
                key, n, v1, laters
            ));
            i += 1;
        }
        out
    }

    /// 人话报告（默认预算 768 字节；超预算行边界截断加尾注——账面数
    /// 不丢，明细走重复簿查询）。预算 < 64 → ReportBudgetExceeded。
    pub fn render_dup_report_with_budget(&self, budget: usize) -> Result<String, DupKeyError> {
        if budget < REPORT_BUDGET_MIN {
            return Err(DupKeyError::ReportBudgetExceeded);
        }
        let mut head = format!(
            "重复参数账（首现口径）：首现 {} 键，总重复 {} 笔（同值 {} / 异值 {}）；{}",
            self.effective_count(),
            self.total_dups,
            self.same_val_dups,
            self.diff_val_dups,
            compare_note()
        );
        let lines = self.dup_report_lines();
        if lines.is_empty() {
            head.push_str("；无重复键");
            return Ok(head);
        }
        let mut out = String::new();
        out.push_str(&head);
        out.push('\n');
        let mut shown = 0;
        let mut skipped = 0;
        let mut li = 0;
        while li < lines.len() {
            let line = &lines[li];
            if out.len() + line.len() + 1 + 48 > budget {
                // 48 = 截断尾注上限预算（「……（截断：余 n 行……）」）。
                skipped = lines.len() - li;
                break;
            }
            out.push_str(line);
            out.push('\n');
            shown += 1;
            li += 1;
        }
        if skipped > 0 {
            out.push_str(&format!(
                "……（截断：余 {} 行未呈现，明细走重复簿查询；已呈现 {} 行）\n",
                skipped, shown
            ));
        }
        Ok(out)
    }

    /// 人话报告（REPORT_BUDGET 默认预算口径）。
    pub fn render_dup_report(&self) -> Result<String, DupKeyError> {
        self.render_dup_report_with_budget(REPORT_BUDGET)
    }

    /// 双重违规清单（白名单外重复——中文逐笔）。
    pub fn double_violation_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.unknown_ledger.len() {
            let e = &self.unknown_ledger[i];
            if e.is_double_violation() {
                out.push(format!(
                    "双重违规：键「{}」不在白名单（第一罪），且出现 {} 次（第二罪：重复）；生效面零染——全量弃置",
                    e.key_render(),
                    e.count
                ));
            }
            i += 1;
        }
        if self.unknown_overflow > 0 {
            out.push(format!(
                "另有 {} 笔未知键出现因账槽满（{} 条）未逐键立账，计数保留",
                self.unknown_overflow, UNKNOWN_CAP
            ));
        }
        out
    }

    /// 账面全量转储（确定性：键表序 + 全局序；指纹的输入）。
    pub fn canonical_state(&self) -> String {
        let mut s = String::new();
        s.push_str(DK_VERSION);
        s.push_str("|FW|");
        let mut i = 0;
        while i < KEY_N {
            if self.present[i] {
                s.push_str(&format!(
                    "{}:{}:{}:{}|",
                    i,
                    self.first_ordinal[i],
                    self.effective[i].len(),
                    lossy_bytes(self.effective[i].as_slice())
                ));
            }
            i += 1;
        }
        s.push_str("|DB|");
        let mut j = 0;
        while j < self.dup_book.len() {
            let e = &self.dup_book[j];
            s.push_str(&format!(
                "{}:{}:{}:{}:{}|",
                e.ordinal,
                e.key_idx,
                e.occurrence,
                if e.same_as_effective { 1 } else { 0 },
                lossy_bytes(e.raw.as_slice())
            ));
            j += 1;
        }
        s.push_str("|UK|");
        let mut k = 0;
        while k < self.unknown_ledger.len() {
            let u = &self.unknown_ledger[k];
            s.push_str(&format!(
                "{}:{}:{}:{}|",
                lossy_bytes(&u.kept[..u.kept_len as usize]),
                u.count,
                u.first_ordinal,
                u.last_ordinal
            ));
            k += 1;
        }
        let c = self.counters();
        s.push_str(&format!(
            "|CT|{},{},{},{},{},{},{},{},{},{}|",
            c.first_wins,
            c.dup_ignored,
            c.unknown_rejects,
            c.bool_rejects,
            c.val_len_rejects,
            c.key_len_rejects,
            c.ring_overflows,
            c.sealed_writes,
            c.total_seen,
            self.unknown_overflow
        ));
        s
    }

    /// 账面指纹（FNV-1a 64 位 over canonical_state——双跑对拍的比对基）。
    pub fn fingerprint(&self) -> u64 {
        fnv64(self.canonical_state().as_bytes())
    }

    // ── 白名单联动审计 ─────────────────────────────────────────────

    /// 白名单联动审计：重复簿每条的键必须在白名单键空间内（结构性
    /// 保证——未知键进不了重复簿）；白名单外重复独立分账为双重违规。
    pub fn whitelist_dup_audit(&self) -> WhitelistAudit {
        let mut outside = 0;
        let mut j = 0;
        while j < self.dup_book.len() {
            let e = &self.dup_book[j];
            if e.key_idx as usize >= KEY_N {
                outside += 1;
            }
            j += 1;
        }
        WhitelistAudit {
            dup_entries: self.dup_book.len() as u32,
            outside_whitelist: outside,
            double_violations: self.double_violation_count(),
            ok: outside == 0,
        }
    }

    // ── 账目恒等式自审 ─────────────────────────────────────────────

    /// 恒等式自审（七式；任何一破即 ok=false）：
    ///   1. 总尝试 = 首现 + 重复 + 五类拒收（计数守恒）；
    ///   2. 首现数 = 生效表占用数（生效面守恒）；
    ///   3. 总重复 = 每键重复计数之和（重复面守恒）；
    ///   4. 重复簿条数 = 总重复（簿账一致）；
    ///   5. 总重复 = 同值 + 异值（分列守恒）；
    ///   6. 每键簿条数 = 该键重复计数 + 簿全局序严格递增（簿序守恒）；
    ///   7. 未知拒收 = 复现账出现数 + 槽满溢出（未知面守恒）。
    pub fn audit(&mut self) -> AuditReport {
        self.tick_inc();
        let c = self.counters();
        let mut rows: Vec<(&'static str, bool)> = Vec::new();
        // 1 计数守恒
        let rejects = c.unknown_rejects
            .wrapping_add(c.bool_rejects)
            .wrapping_add(c.val_len_rejects)
            .wrapping_add(c.key_len_rejects)
            .wrapping_add(c.ring_overflows)
            .wrapping_add(c.sealed_writes);
        let sum_ok = c
            .first_wins
            .wrapping_add(c.dup_ignored)
            .wrapping_add(rejects)
            == c.total_seen;
        rows.push(("总尝试=首现+重复+拒收", sum_ok));
        // 2 生效面守恒
        let eff = self.effective_count();
        rows.push(("首现数=生效占用数", c.first_wins == eff));
        // 3 重复面守恒
        let mut dup_sum: u32 = 0;
        let mut i = 0;
        while i < KEY_N {
            dup_sum = dup_sum.wrapping_add(self.dup_count[i] as u32);
            i += 1;
        }
        rows.push(("总重复=每键重复之和", self.total_dups == dup_sum));
        // 4 簿账一致
        rows.push(("重复簿条数=总重复", self.dup_book.len() as u32 == self.total_dups));
        // 5 分列守恒
        rows.push((
            "总重复=同值+异值",
            self.total_dups == self.same_val_dups.wrapping_add(self.diff_val_dups),
        ));
        // 6 簿序守恒：每键簿条数=键重复计数；簿全局序严格递增。
        let mut per_key_ok = true;
        let mut order_ok = true;
        let mut last_ord: u32 = 0;
        let mut j = 0;
        let mut per_key_counts = [0u16; KEY_N];
        while j < self.dup_book.len() {
            let e = &self.dup_book[j];
            if e.ordinal <= last_ord {
                order_ok = false;
            }
            last_ord = e.ordinal;
            per_key_counts[e.key_idx as usize] = per_key_counts[e.key_idx as usize].wrapping_add(1);
            j += 1;
        }
        let mut k = 0;
        while k < KEY_N {
            if per_key_counts[k] != self.dup_count[k] {
                per_key_ok = false;
            }
            k += 1;
        }
        rows.push(("簿序与每键条数守恒", per_key_ok && order_ok));
        // 7 未知面守恒（复现账之和 + 槽满溢出 = 未知拒收数）
        rows.push((
            "未知拒收=复现账+槽溢",
            self.unknown_occurrences_sum() == c.unknown_rejects,
        ));
        let ok = sum_ok
            && c.first_wins == eff
            && self.total_dups == dup_sum
            && self.dup_book.len() as u32 == self.total_dups
            && self.total_dups == self.same_val_dups.wrapping_add(self.diff_val_dups)
            && per_key_ok
            && order_ok
            && self.unknown_occurrences_sum() == c.unknown_rejects;
        self.last_audit_ok = Some(ok);
        self.events.push(Event {
            tick: self.ticks,
            kind: if ok { EventKind::AuditPass } else { EventKind::AuditFail },
            key_idx: KEY_IDX_NONE,
            aux: rows.len() as u16,
        });
        AuditReport { rows, ok }
    }

    /// 自审闸（失败上码 AuditFailed；成功 Ok(报告) 不拷贝——闸口径）。
    pub fn audit_gate(&mut self) -> Result<(), DupKeyError> {
        let rep = self.audit();
        if rep.ok {
            Ok(())
        } else {
            Err(DupKeyError::AuditFailed)
        }
    }

    /// 最近一次自审结果（None=未审）。
    pub fn last_audit_ok(&self) -> Option<bool> {
        self.last_audit_ok
    }

    // ── 健康分 ─────────────────────────────────────────────────────

    /// 健康分（0~1000 满分起扣，saturating 不回卷；重复本身不扣分——
    /// 重复是已入账的正常现象，扣的是「没入账就进不来」的违规面）：
    ///   未知键拒收 ×15 / 双重违规 ×40 / 布尔词形拒收 ×10 / 值超长 ×10 /
    ///   键超长 ×5 / 环形满 ×25 / 封后写 ×30 / 自审失败 ×100 /
    ///   重复卫生分（重复总量压力）至多扣 120。
    pub fn health(&self) -> u32 {
        let mut score = HEALTH_FULL;
        score = score.saturating_sub(self.unknown_rejects.saturating_mul(15));
        score = score.saturating_sub(self.double_violation_count().saturating_mul(40));
        score = score.saturating_sub(self.bool_rejects.saturating_mul(10));
        score = score.saturating_sub(self.val_len_rejects.saturating_mul(10));
        score = score.saturating_sub(self.key_len_rejects.saturating_mul(5));
        score = score.saturating_sub(self.ring_overflows.saturating_mul(25));
        score = score.saturating_sub(self.sealed_writes.saturating_mul(30));
        if self.last_audit_ok == Some(false) {
            score = score.saturating_sub(100);
        }
        // 重复卫生分：每笔重复 1 分、至多 120 分（64 槽极值场景封顶）。
        let hygiene = if self.total_dups > 120 { 120 } else { self.total_dups };
        score.saturating_sub(hygiene)
    }

    /// 健康分明细（中文逐项——人查「分去哪了」）。
    pub fn health_report(&self) -> String {
        let c = self.counters();
        let mut s = format!(
            "健康分 {}/{}：",
            self.health(),
            HEALTH_FULL
        );
        s.push_str(&format!(" 未知键拒收×15={} ", c.unknown_rejects.saturating_mul(15)));
        s.push_str(&format!("双重违规×40={} ", self.double_violation_count().saturating_mul(40)));
        s.push_str(&format!("布尔词形拒收×10={} ", c.bool_rejects.saturating_mul(10)));
        s.push_str(&format!("值超长×10={} ", c.val_len_rejects.saturating_mul(10)));
        s.push_str(&format!("键超长×5={} ", c.key_len_rejects.saturating_mul(5)));
        s.push_str(&format!("环形满×25={} ", c.ring_overflows.saturating_mul(25)));
        s.push_str(&format!("封后写×30={} ", c.sealed_writes.saturating_mul(30)));
        if self.last_audit_ok == Some(false) {
            s.push_str("自审失败×100=100 ");
        }
        let hygiene = if self.total_dups > 120 { 120 } else { self.total_dups };
        s.push_str(&format!("重复卫生≤120={}", hygiene));
        s
    }

    /// 健康闸（< 600 上码 HealthFloorHit；Ok 返回当前分）。
    pub fn health_gate(&self) -> Result<u32, DupKeyError> {
        let h = self.health();
        if h >= HEALTH_FLOOR {
            Ok(h)
        } else {
            Err(DupKeyError::HealthFloorHit)
        }
    }
}

/// 计数器组只读快照（审计渲染与健康分明细共用）。
#[derive(Clone, Copy, Debug)]
pub struct LedgerCounters {
    /// 首现入账数。
    pub first_wins: u32,
    /// 重复忽略数。
    pub dup_ignored: u32,
    /// 未知键拒收数。
    pub unknown_rejects: u32,
    /// 布尔词形拒收数。
    pub bool_rejects: u32,
    /// 值超长拒收数。
    pub val_len_rejects: u32,
    /// 键超长拒收数。
    pub key_len_rejects: u32,
    /// 环形满拒收数。
    pub ring_overflows: u32,
    /// 封后写拦截数。
    pub sealed_writes: u32,
    /// 总尝试数。
    pub total_seen: u32,
}
// ═══════════════════════════════════════════════════════════════════════
// 参数流（ParamStream：定长环形 64 槽（键,值）流式登记）
// ═══════════════════════════════════════════════════════════════════════

/// 流槽条目（一次出现的原位保存：键位 + 原值单元；Copy）。
#[derive(Clone, Copy, Debug)]
pub struct StreamEntry {
    /// 键位（0..24；未知/拒收不入槽）。
    pub key_idx: u8,
    /// 原值（逐字节保真）。
    pub val: ValCell,
    /// 全局出现序（1 起）。
    pub ordinal: u32,
}

/// 流统计（一次渲染的全景数——封面口径）。
#[derive(Clone, Copy, Debug)]
pub struct StreamStats {
    /// 在槽出现数。
    pub occurrences: u32,
    /// 唯一键数（= 生效表占用）。
    pub distinct_keys: u32,
    /// 重复笔数。
    pub dups: u32,
    /// 未知键拒收数。
    pub unknown_rejects: u32,
    /// 其他拒收数（布尔/值长/键长/环形/封后写合计）。
    pub other_rejects: u32,
    /// 是否已封流。
    pub sealed: bool,
    /// 剩余槽位。
    pub free: usize,
}

/// 参数流：loader 交来的（键,值）对按序流式登记——五卫前置
/// （封流/键空/键长/闭集/值长+布尔词形），过卫入环形并联动首现账本。
///
/// 环形 64 槽 = 单条 256 字节参数行（F0581 界）的理论极值对数，
/// 正常单行恒不溢出；封流（seal）后进入只读口径，再写 100% 拦截；
/// 快照槽一拍（防指纹漂移），复扫走 reverify。
#[derive(Clone, Debug)]
pub struct ParamStream {
    /// 环形槽（预分配 STREAM_CAP；逻辑上「定长」，满即拒收不覆盖）。
    ring: Vec<StreamEntry>,
    /// 在槽条数。
    count: usize,
    /// 全局出现序分配器（wrapping）。
    next_ordinal: u32,
    /// 封流位。
    sealed: bool,
    /// 快照槽占用位（一拍）。
    snapshot_taken: bool,
    /// 首现口径账（本流的账本——一流一账，隔离可对拍）。
    ledger: FirstWinLedger,
}

impl ParamStream {
    /// 构造（环形与账本各自预分配到位）。
    pub fn new() -> ParamStream {
        ParamStream {
            ring: Vec::with_capacity(STREAM_CAP),
            count: 0,
            next_ordinal: 0,
            sealed: false,
            snapshot_taken: false,
            ledger: FirstWinLedger::new(),
        }
    }

    /// 首现口径声明原文（const 冻结语义的流侧只读面）。
    pub fn first_win_semantics(&self) -> &'static str {
        FIRST_WIN_SEMANTICS
    }

    /// 在槽条数。
    pub fn len(&self) -> usize {
        self.count
    }

    /// 是否空流。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 容量（恒 STREAM_CAP）。
    pub fn capacity(&self) -> usize {
        STREAM_CAP
    }

    /// 剩余槽位。
    pub fn free_slots(&self) -> usize {
        STREAM_CAP - self.count
    }

    /// 是否已封流。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 快照槽是否已用。
    pub fn is_snapshot_taken(&self) -> bool {
        self.snapshot_taken
    }

    /// 账本只读（查询面全走账本）。
    pub fn ledger(&self) -> &FirstWinLedger {
        &self.ledger
    }

    /// 账本可变（注入面/自审面；封流后仍可审计——审计不改账面数）。
    pub fn ledger_mut(&mut self) -> &mut FirstWinLedger {
        &mut self.ledger
    }

    /// 逐槽查询（入槽序；越界 → SlotOutOfRange）。
    pub fn occurrence(&self, i: usize) -> Result<StreamEntry, DupKeyError> {
        if i < self.count {
            Ok(self.ring[i])
        } else {
            Err(DupKeyError::SlotOutOfRange)
        }
    }

    /// 键 → 全部出现（含首现；入槽序）——「每次出现位置」的流侧查询。
    pub fn occurrences_of(&self, key: &str) -> Result<Vec<StreamEntry>, DupKeyError> {
        let idx = match key_index(key) {
            Some(i) => i,
            None => return Err(DupKeyError::KeyNotFound),
        };
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.count {
            if self.ring[i].key_idx as usize == idx {
                out.push(self.ring[i]);
            }
            i += 1;
        }
        Ok(out)
    }

    /// 首现出现序分配（私有；wrapping）。
    fn alloc_ordinal(&mut self) -> u32 {
        self.next_ordinal = self.next_ordinal.wrapping_add(1);
        self.next_ordinal
    }

    /// 流式登记主口（五卫 → 入槽 → 联动账本；结果面见 PushOutcome）。
    ///
    /// 卫序（早期卫省账槽）：封流 → 键空 → 键超长 → 闭集 → 值超长 →
    /// 布尔词形 → 环形余量。每卫拒收都入事件账与计数器（账不丢）。
    pub fn push(&mut self, key: &str, val: &str) -> Result<PushOutcome, DupKeyError> {
        // 卫 1：封后写拦截（100% 面）。
        if self.sealed {
            // 键位尽量解析（便于事件定位）；解析失败用哨兵。
            let ki = key_index(key).map(|i| i as u8).unwrap_or(KEY_IDX_NONE);
            self.ledger.record_sealed_write(ki);
            return Err(DupKeyError::StreamSealed);
        }
        // 卫 2：空键（aux=2——键形态卫的空键分目）。
        if key.is_empty() {
            // 空键不入任何账槽（无从保存）；计数入事件账。
            self.ledger.record_key_len_reject(2);
            return Err(DupKeyError::EmptyKey);
        }
        // 卫 3：键超长快速拒收（>24 字节必不在闭集；省未知账槽；aux=1）。
        if key.as_bytes().len() > KEY_KEEP {
            self.ledger.record_key_len_reject(1);
            return Err(DupKeyError::KeyTooLong);
        }
        // 卫 4：闭集（白名单）——未知键显性弃置 + 复现升双重违规。
        let idx = match key_index(key) {
            Some(i) => i,
            None => {
                let ordinal = self.alloc_ordinal();
                self.ledger.record_unknown(key, ordinal);
                return Err(DupKeyError::UnknownKey);
            }
        };
        let ki = idx as u8;
        // 卫 5a：值超长。
        if val.as_bytes().len() > VAL_MAX {
            self.ledger.record_val_len_reject(ki);
            return Err(DupKeyError::ValueTooLong);
        }
        // 卫 5b：布尔词形闭集（布尔键专属——比较口径的前提）。
        if KEY_TABLE[idx].kind == KeyKind::Bool {
            if let Err(e) = canonical_bool(val) {
                self.ledger.record_bool_reject(ki);
                return Err(e);
            }
        }
        // 卫 6：环形余量（满即拒收，不覆盖——定长口径）。
        if self.count >= STREAM_CAP {
            self.ledger.record_ring_overflow(ki);
            return Err(DupKeyError::RingFull);
        }
        // 过五卫：入槽 + 联动账本。
        let cell = ValCell::from_str(val)?;
        let ordinal = self.alloc_ordinal();
        self.ring.push(StreamEntry { key_idx: ki, val: cell, ordinal });
        self.count += 1;
        self.ledger.record_accepted(idx, &cell, ordinal)
    }

    /// 批量登记（遇首个拒收即停——前缀语义：拒收前的成功笔保留）。
    pub fn push_batch(&mut self, pairs: &[(&str, &str)]) -> Result<Vec<PushOutcome>, DupKeyError> {
        let mut out = Vec::with_capacity(pairs.len());
        let mut i = 0;
        while i < pairs.len() {
            match self.push(pairs[i].0, pairs[i].1) {
                Ok(o) => out.push(o),
                Err(e) => return Err(e),
            }
            i += 1;
        }
        Ok(out)
    }

    /// 封流（账定；重复封流 → StreamSealed）。封流不可逆。
    pub fn seal(&mut self) -> Result<(), DupKeyError> {
        if self.sealed {
            return Err(DupKeyError::StreamSealed);
        }
        self.sealed = true;
        self.ledger.record_seal();
        Ok(())
    }

    /// 首现快照（须先 seal；每流恰一拍——二拍 → SnapshotSealed）。
    pub fn snapshot(&mut self) -> Result<FirstWinSnapshot, DupKeyError> {
        if !self.sealed {
            return Err(DupKeyError::StreamNotSealed);
        }
        if self.snapshot_taken {
            return Err(DupKeyError::SnapshotSealed);
        }
        let snap = FirstWinSnapshot::from_stream(self);
        self.snapshot_taken = true;
        self.ledger.record_snapshot();
        Ok(snap)
    }

    /// 流统计（封面口径一次渲染）。
    pub fn stats(&self) -> StreamStats {
        let c = self.ledger.counters();
        StreamStats {
            occurrences: self.count as u32,
            distinct_keys: self.ledger.effective_count(),
            dups: self.ledger.total_duplicates(),
            unknown_rejects: c.unknown_rejects,
            other_rejects: c.bool_rejects
                .wrapping_add(c.val_len_rejects)
                .wrapping_add(c.key_len_rejects)
                .wrapping_add(c.ring_overflows)
                .wrapping_add(c.sealed_writes),
            sealed: self.sealed,
            free: self.free_slots(),
        }
    }

    /// 复位成净流（复跑口径：双跑对拍与注入面在净流上重放；账面全清）。
    pub fn reset(&mut self) {
        self.ring.clear();
        self.count = 0;
        self.next_ordinal = 0;
        self.sealed = false;
        self.snapshot_taken = false;
        self.ledger = FirstWinLedger::new();
    }
}
// ═══════════════════════════════════════════════════════════════════════
// 首现快照（FirstWinSnapshot：流处理完成后生效表的封账副本）
// ═══════════════════════════════════════════════════════════════════════

/// 首现断言面（快照级自证汇总：一键一槽断言 + 生效=首现断言 + 重复
/// 计数断言——注入同键 3 现自证的聚合面）。
#[derive(Clone, Copy, Debug)]
pub struct FirstWinAssertFace {
    /// 已拍快照的键数（每键恰一槽）。
    pub keys: u32,
    /// 一键一槽断言（生效条目数 == 键数——无键双槽）。
    pub one_slot_per_key: bool,
    /// 生效=首现断言（对拍流首现原文全等——首现口径本证）。
    pub effective_equals_first: bool,
    /// 总重复数。
    pub total_dups: u32,
    /// 同值重复数。
    pub same_val_dups: u32,
    /// 异值重复数。
    pub diff_val_dups: u32,
    /// 重复计数断言（同值+异值 == 总重复）。
    pub dups_split_ok: bool,
    /// 断言面总判定。
    pub pass: bool,
}

/// 首现快照：封流后生效表的封账副本（键→首现生效值 + 每键重复计数 +
/// 重复簿副本 + 指纹）。快照不可变——消费方（F0590 缺省补齐 / F0593
/// 封账）以快照为准，与流的后续状态解耦。
#[derive(Clone, Debug)]
pub struct FirstWinSnapshot {
    /// 生效条目（键位序——确定性口径；键表序即快照序）。
    pub eff: Vec<(u8, ValCell)>,
    /// 每键重复计数副本（24 槽全拷）。
    pub per_key_dup: [u16; KEY_N],
    /// 总重复数。
    pub total_dups: u32,
    /// 同值重复数。
    pub same_val_dups: u32,
    /// 异值重复数。
    pub diff_val_dups: u32,
    /// 重复簿副本（全局序）。
    pub dup_book: Vec<DupEntry>,
    /// 快照指纹（构造时一次拍定）。
    pub fingerprint: u64,
    /// 快照时流内出现总数（含拒收前的总尝试口径另见 stats；此处为
    /// 过卫入槽数——「流处理完成」的规模注记）。
    pub occurrences: u32,
    /// 快照版本注记（DK_VERSION 同源）。
    pub version: &'static str,
    /// 首现口径声明原文（随快照冻结——快照自携带语义）。
    pub semantics: &'static str,
}

impl FirstWinSnapshot {
    /// 从封流构造（私有口：走 ParamStream::snapshot 卫门）。
    fn from_stream(s: &ParamStream) -> FirstWinSnapshot {
        let ledger = s.ledger();
        let mut eff = Vec::with_capacity(KEY_N);
        let mut i = 0;
        while i < KEY_N {
            if ledger.present_idx(i) {
                eff.push((i as u8, ledger.effective_idx(i).unwrap_or(ValCell::empty())));
            }
            i += 1;
        }
        let mut snap = FirstWinSnapshot {
            eff,
            per_key_dup: ledger.dup_count_table(),
            total_dups: ledger.total_duplicates(),
            same_val_dups: ledger.same_val_dups(),
            diff_val_dups: ledger.diff_val_dups(),
            dup_book: ledger.dup_book_all().to_vec(),
            fingerprint: 0,
            occurrences: s.len() as u32,
            version: DK_VERSION,
            semantics: FIRST_WIN_SEMANTICS,
        };
        snap.fingerprint = snap.compute_fingerprint();
        snap
    }

    /// 指纹输入（确定性：版本 + 语义哈希 + 生效条目 + 重复分列）。
    fn compute_fingerprint(&self) -> u64 {
        let mut s = String::new();
        s.push_str(self.version);
        s.push('|');
        s.push_str(&format!("{}", fnv64(self.semantics.as_bytes())));
        s.push_str("|FW|");
        let mut i = 0;
        while i < self.eff.len() {
            let (ki, cell) = &self.eff[i];
            s.push_str(&format!(
                "{}:{}:{}|",
                ki,
                cell.len(),
                lossy_bytes(cell.as_slice())
            ));
            i += 1;
        }
        s.push_str(&format!(
            "|DUP|{},{},{},{}|",
            self.total_dups, self.same_val_dups, self.diff_val_dups, self.occurrences
        ));
        fnv64(s.as_bytes())
    }

    /// 指纹复核（防快照被篡改/漂移——结构自证）。
    pub fn verify_fingerprint(&self) -> Result<(), DupKeyError> {
        if self.fingerprint == self.compute_fingerprint() {
            Ok(())
        } else {
            Err(DupKeyError::FingerprintMismatch)
        }
    }

    /// 快照内生效值查询（键名口径；不在快照 → KeyNotFound）。
    pub fn effective(&self, key: &str) -> Result<ValCell, DupKeyError> {
        match key_index(key) {
            Some(i) => {
                let mut j = 0;
                while j < self.eff.len() {
                    if self.eff[j].0 as usize == i {
                        return Ok(self.eff[j].1);
                    }
                    j += 1;
                }
                Err(DupKeyError::KeyNotFound)
            }
            None => Err(DupKeyError::KeyNotFound),
        }
    }

    /// 快照内生效值渲染。
    pub fn effective_str(&self, key: &str) -> Result<String, DupKeyError> {
        Ok(self.effective(key)?.render())
    }

    /// 每键重复计数（表外键 KeyNotFound）。
    pub fn dup_count_of(&self, key: &str) -> Result<u32, DupKeyError> {
        match key_index(key) {
            Some(i) => Ok(self.per_key_dup[i] as u32),
            None => Err(DupKeyError::KeyNotFound),
        }
    }

    /// 首现断言面（快照级自证）：
    ///   一键一槽（eff 无重复键位）+ 生效=首现（对拍流原文首现）+
    ///   重复分列守恒（同+异 == 总）。三面全过才 pass。
    pub fn assert_face(&self, stream: &ParamStream) -> Result<FirstWinAssertFace, DupKeyError> {
        // 一键一槽：eff 键位互异（构造即键表序扫描——结构性保证，
        // 此处复核防篡改）。
        let mut one_slot = true;
        let mut i = 0;
        while i < self.eff.len() {
            let mut j = i + 1;
            while j < self.eff.len() {
                if self.eff[i].0 == self.eff[j].0 {
                    one_slot = false;
                }
                j += 1;
            }
            i += 1;
        }
        // 生效=首现：对拍流的每次出现，首笔原文 == 快照生效值。
        let mut eff_first = true;
        let mut seen = [false; KEY_N];
        let mut k = 0;
        while k < stream.len() {
            let entry = match stream.occurrence(k) {
                Ok(e) => e,
                Err(_) => {
                    eff_first = false;
                    break;
                }
            };
            let ki = entry.key_idx as usize;
            if !seen[ki] {
                seen[ki] = true;
                // 流内首笔：须与快照生效值全等。
                match self.effective_idx_in_snap(ki) {
                    Some(cell) => {
                        if !cell.eq_cell(&entry.val) {
                            eff_first = false;
                        }
                    }
                    None => eff_first = false,
                }
            }
            k += 1;
        }
        let split_ok = self.total_dups == self.same_val_dups.wrapping_add(self.diff_val_dups);
        let pass = one_slot && eff_first && split_ok;
        Ok(FirstWinAssertFace {
            keys: self.eff.len() as u32,
            one_slot_per_key: one_slot,
            effective_equals_first: eff_first,
            total_dups: self.total_dups,
            same_val_dups: self.same_val_dups,
            diff_val_dups: self.diff_val_dups,
            dups_split_ok: split_ok,
            pass,
        })
    }

    /// 快照内键位生效值（私有辅：None=不在快照）。
    fn effective_idx_in_snap(&self, idx: usize) -> Option<ValCell> {
        let mut j = 0;
        while j < self.eff.len() {
            if self.eff[j].0 as usize == idx {
                return Some(self.eff[j].1);
            }
            j += 1;
        }
        None
    }

    /// 复扫对拍（F0594 口径承接：同原文重放复扫——流与快照互证；
    /// 分叉 → FingerprintMismatch）。
    pub fn reverify(&self, stream: &ParamStream) -> Result<(), DupKeyError> {
        let face = self.assert_face(stream)?;
        if face.pass && self.fingerprint == self.compute_fingerprint() {
            Ok(())
        } else {
            Err(DupKeyError::FingerprintMismatch)
        }
    }

    /// 快照摘要（中文一行——消费方入日志的封面）。
    pub fn render_summary(&self) -> String {
        format!(
            "首现快照[{}]：生效 {} 键 / 总现 {} / 重复 {}（同 {} 异 {}）/ 指纹 {:016x}",
            self.version,
            self.eff.len(),
            self.occurrences,
            self.total_dups,
            self.same_val_dups,
            self.diff_val_dups,
            self.fingerprint
        )
    }

    /// 快照生效表逐行（键表序；中文「键=值（重复 n）」式）。
    pub fn render_effective_table(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.eff.len() {
            let (ki, cell) = &self.eff[i];
            let kname = key_name(*ki as usize);
            let dup = self.per_key_dup[*ki as usize];
            let dup_note = if dup > 0 {
                format!("（重复 {} 笔被忽略）", dup)
            } else {
                String::new()
            };
            out.push(format!("{}={}{}", kname, cell.render(), dup_note));
            i += 1;
        }
        out
    }
}
// ═══════════════════════════════════════════════════════════════════════
// 双跑确定性对拍（dual 面）
// ═══════════════════════════════════════════════════════════════════════

/// 双跑对拍报告（两净流重放同批次：逐笔结果 + 账面指纹 + 快照指纹）。
#[derive(Clone, Debug)]
pub struct DualRunReport {
    /// 跑 A 账面指纹。
    pub fp_a: u64,
    /// 跑 B 账面指纹。
    pub fp_b: u64,
    /// 跑 A 快照指纹。
    pub snap_fp_a: u64,
    /// 跑 B 快照指纹。
    pub snap_fp_b: u64,
    /// 逐笔推入结果全等（首现位/序/第几现/重复笔号逐笔比对）。
    pub outcomes_match: bool,
    /// 总判定（指纹全等 + 逐笔全等）。
    pub all_match: bool,
    /// 首个分叉笔号（0 起；全等时无意义置批次长）。
    pub first_diverge_at: usize,
}

/// 双跑确定性对拍：同批次在两个净流上重放（push → seal → snapshot），
/// 三面比对：账面指纹、快照指纹、逐笔推入结果。任一分叉即失配。
pub fn dual_run(batch: &[(&str, &str)]) -> Result<DualRunReport, DupKeyError> {
    if batch.is_empty() {
        return Err(DupKeyError::InvalidParam);
    }
    // 跑 A。
    let mut sa = ParamStream::new();
    let mut oa = Vec::with_capacity(batch.len());
    let mut i = 0;
    while i < batch.len() {
        match sa.push(batch[i].0, batch[i].1) {
            Ok(o) => oa.push(o),
            Err(e) => oa_push_err_note(&mut oa, &e),
        }
        i += 1;
    }
    sa.seal()?;
    let snap_a = sa.snapshot()?;
    // 跑 B。
    let mut sb = ParamStream::new();
    let mut ob = Vec::with_capacity(batch.len());
    let mut j = 0;
    while j < batch.len() {
        match sb.push(batch[j].0, batch[j].1) {
            Ok(o) => ob.push(o),
            Err(e) => oa_push_err_note(&mut ob, &e),
        }
        j += 1;
    }
    sb.seal()?;
    let snap_b = sb.snapshot()?;
    // 逐笔比对（Ok 笔逐字段；Err 笔比码）。
    let mut outcomes_match = oa.len() == ob.len();
    let mut first_diverge_at = batch.len();
    if outcomes_match {
        let mut k = 0;
        while k < oa.len() {
            let (x, y) = (&oa[k], &ob[k]);
            let eq = match (x, y) {
                (PushNote::Ok(p), PushNote::Ok(q)) => {
                    p.key_idx == q.key_idx
                        && p.first_win == q.first_win
                        && p.ordinal == q.ordinal
                        && p.occurrence == q.occurrence
                        && p.dup_no == q.dup_no
                }
                (PushNote::Err(a), PushNote::Err(b)) => a == b,
                _ => false,
            };
            if !eq {
                outcomes_match = false;
                first_diverge_at = k;
                break;
            }
            k += 1;
        }
    } else {
        first_diverge_at = if oa.len() < ob.len() { oa.len() } else { ob.len() };
    }
    let fp_a = sa.ledger().fingerprint();
    let fp_b = sb.ledger().fingerprint();
    let all_match = outcomes_match && fp_a == fp_b && snap_a.fingerprint == snap_b.fingerprint;
    Ok(DualRunReport {
        fp_a,
        fp_b,
        snap_fp_a: snap_a.fingerprint,
        snap_fp_b: snap_b.fingerprint,
        outcomes_match,
        all_match,
        first_diverge_at,
    })
}

/// 对拍笔注（Ok/Err 统一装箱——逐笔比对的载体；私有）。
#[derive(Clone, Copy, Debug)]
enum PushNote {
    /// 成功笔。
    Ok(PushOutcome),
    /// 拒收笔（码）。
    Err(u16),
}

/// 拒收笔入注（私有辅）。
fn oa_push_err_note(v: &mut Vec<PushNote>, e: &DupKeyError) {
    v.push(PushNote::Err(e.code()));
}

/// 双跑闸（失配 → FingerprintMismatch；成功 Ok(报告)）。
pub fn dual_run_gate(batch: &[(&str, &str)]) -> Result<DualRunReport, DupKeyError> {
    let rep = dual_run(batch)?;
    if rep.all_match {
        Ok(rep)
    } else {
        Err(DupKeyError::FingerprintMismatch)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 注入面（injection：三注入全检出 3/3——J 判据扩展承接）
// ═══════════════════════════════════════════════════════════════════════

/// 单注入用例面（期面 + 检出面 + 人话结论）。
#[derive(Clone, Debug)]
pub struct CaseFace {
    /// 用例名（中文）。
    pub name: &'static str,
    /// 期望检出码/断言（人话）。
    pub want: &'static str,
    /// 是否检出。
    pub caught: bool,
    /// 结论一句话。
    pub detail: String,
}

/// 注入面报告（三用例：同键三现 1/1+2/2 / 异值重复分列 / 未知键重复
/// 双重违规；caught 3/3 = 检出率 1000‰）。
#[derive(Clone, Debug)]
pub struct InjectionSuite {
    /// 用例 1：同键三现（J 判据核心）。
    pub triple: CaseFace,
    /// 用例 2：异值重复（同值/异值分列检出）。
    pub diff_val: CaseFace,
    /// 用例 3：未知键重复（双重违规分账检出）。
    pub unknown_dup: CaseFace,
    /// 检出数。
    pub caught: u32,
    /// 用例总数。
    pub total: u32,
}

impl InjectionSuite {
    /// 全检出判定。
    pub fn all_caught(&self) -> bool {
        self.caught == self.total
    }

    /// 检出率（‰ 千分定点——1000‰ = 100%）。
    pub fn permille(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        (self.caught as u64).wrapping_mul(CAUGHT_RATE_FULL) / (self.total as u64)
    }
}

/// 注入面（在净 scratch 流上跑——主账零染；复用 ParamStream 全管线）：
///   用例 1 同键三现：heap.mb 注入 512/1024/2048 → 首现生效 1/1
///     （生效值=512）+ 重复计数 2/2（全异值 2 笔）；
///   用例 2 异值重复分列：loglevel 注入 3/3/7（同值+异值混排）→
///     同值 1 笔 / 异值 1 笔分列各就位；
///   用例 3 未知键重复：evil.key 注入 2 次 → 两次 UnknownKey 拒收 +
///     双重违规分账 1 笔。
pub fn injection_suite() -> InjectionSuite {
    // ── 用例 1：同键三现（J 判据核心：1/1 + 2/2）─────────────────
    let triple_face = match FirstWinLedger::triple_inject_face("heap.mb", "512", "1024", "2048") {
        Ok(f) => f,
        Err(_) => TripleFace {
            occurrences: 0,
            effective_entries: 0,
            effective_is_first: false,
            dup_counted: 0,
            dup_expected: 2,
            pass: false,
            line: "注入面用例 1 构造失败（不应发生）".to_string(),
        },
    };
    let triple_caught = triple_face.pass;
    let triple = CaseFace {
        name: "同键三现（首现生效 1/1 + 重复计数 2/2）",
        want: "生效 1/1 且生效值=首现值；重复 2/2",
        caught: triple_caught,
        detail: triple_face.line,
    };
    // ── 用例 2：异值重复分列（loglevel 3/3/7——同异混排）──────────
    let mut s2 = ParamStream::new();
    let mut diff_caught = false;
    let mut diff_detail = String::new();
    {
        let r1 = s2.push("loglevel", "3");
        let r2 = s2.push("loglevel", "3");
        let r3 = s2.push("loglevel", "7");
        // 三笔全 Ok 且 2/3 笔是重复，同值 1 + 异值 1。
        if r1.is_ok() && r2.is_ok() && r3.is_ok() {
            let same = s2.ledger().same_val_dups();
            let diff = s2.ledger().diff_val_dups();
            let dups = s2.ledger().total_duplicates();
            diff_caught = dups == 2 && same == 1 && diff == 1;
            diff_detail = format!(
                "loglevel 三现 3/3/7：首现 3 生效；重复 2 笔分列——同值 1（3）+ 异值 1（7）→ 实测 同{} 异{} 总{}",
                same, diff, dups
            );
        } else {
            diff_detail = "用例 2 推入异常（不应发生）".to_string();
        }
    }
    let diff_val = CaseFace {
        name: "异值重复分列（同值/异值各就位）",
        want: "同值 1 + 异值 1",
        caught: diff_caught,
        detail: diff_detail,
    };
    // ── 用例 3：未知键重复（双重违规分账）────────────────────────
    let mut s3 = ParamStream::new();
    let mut unknown_caught = false;
    let mut unknown_detail = String::new();
    {
        let e1 = s3.push("evil.key", "1");
        let e2 = s3.push("evil.key", "2");
        let both_unknown = matches!(e1, Err(DupKeyError::UnknownKey))
            && matches!(e2, Err(DupKeyError::UnknownKey));
        let dv = s3.ledger().double_violation_count();
        let ur = s3.ledger().unknown_repeats().len();
        // 两拒 + 复现账 1 条 + 双重违规 1 笔 + 生效面零染。
        unknown_caught = both_unknown && dv == 1 && ur == 1 && s3.ledger().effective_count() == 0;
        unknown_detail = format!(
            "evil.key 两现：两次白名单外拒收 + 复现账 {} 条 + 双重违规 {} 笔；生效面 {} 键（零染）",
            ur,
            dv,
            s3.ledger().effective_count()
        );
    }
    let unknown_dup = CaseFace {
        name: "未知键重复（双重违规分账）",
        want: "两拒 + 双重违规 1 笔 + 生效零染",
        caught: unknown_caught,
        detail: unknown_detail,
    };
    let caught = (triple_caught as u32) + (diff_caught as u32) + (unknown_caught as u32);
    InjectionSuite {
        triple,
        diff_val,
        unknown_dup,
        caught,
        total: 3,
    }
}

/// 注入闸（3/3 全检出才放行——检出率 1000‰ 口径）。
pub fn injection_gate() -> Result<InjectionSuite, DupKeyError> {
    let suite = injection_suite();
    if suite.all_caught() {
        Ok(suite)
    } else {
        Err(DupKeyError::AuditFailed)
    }
}
// ═══════════════════════════════════════════════════════════════════════
// 冒烟测试（cfg(test)：3 现注入 1/1+2/2 / 首现快照指纹 / 人话报告 /
// 白名单联动 / 双跑一致 / 同异分列 / 五卫拒收 / 环形满 / 快照一拍 /
// FNV 公开向量 / 恒等式自审 / 健康分 / 事件账滑窗 / 注入面 3/3）
// 测试体只用 ? 与 assert!/assert_eq!，零 unwrap/expect。
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// F0586 键空间冻结期望（int/str 16 键逐字对齐的基准清单——
    /// 与 unx0586_valdom INT_TABLE/STR_TABLE 键名列全等）。
    const F0586_INT_KEYS: [&str; 8] = [
        "loglevel",
        "uart.baud",
        "console.cols",
        "console.rows",
        "heap.mb",
        "stack.kb",
        "sched.slice.us",
        "dma.win.mb",
    ];
    const F0586_STR_KEYS: [&str; 8] = [
        "hostname",
        "console.dev",
        "boot.id",
        "log.tag",
        "video.mode",
        "locale",
        "kernel.opt",
        "cmd.prefix",
    ];

    /// 样例批次 A（含同键三现 + 布尔 + 未知键 + 越权值的多面样本）。
    fn batch_a() -> Vec<(&'static str, &'static str)> {
        vec![
            ("heap.mb", "512"),
            ("loglevel", "6"),
            ("heap.mb", "1024"),
            ("watchdog", "on"),
            ("heap.mb", "2048"),
            ("hostname", "varix-node-1"),
            ("watchdog", "true"),
            ("loglevel", "3"),
        ]
    }

    // ── 常量与诊断码闭集 ───────────────────────────────────────────

    #[test]
    fn const_tables_frozen() {
        assert_eq!(KEY_N, 24);
        assert_eq!(BOOL_N, 8);
        assert_eq!(INT_N, 8);
        assert_eq!(STR_N, 8);
        assert_eq!(DK_SEG_BASE, 0x1E90);
        assert_eq!(DK_SEG_TOP, 0x1E9F);
        assert_eq!(STREAM_CAP, 64);
        assert_eq!(DUP_BOOK_CAP, 64);
        assert_eq!(EVENT_CAP, 32);
        assert_eq!(UNKNOWN_CAP, 16);
        assert_eq!(VAL_MAX, 64);
        assert_eq!(KEY_KEEP, 24);
        assert_eq!(REPORT_BUDGET, 768);
        assert_eq!(HEALTH_FULL, 1_000);
        assert_eq!(HEALTH_FLOOR, 600);
        assert!(!FIRST_WIN_SEMANTICS.is_empty());
        assert!(!VALDOM_ALIGNMENT.is_empty());
        assert_eq!(FNV_OFFSET, 0xcbf29ce484222325);
        assert_eq!(FNV_PRIME, 0x100000001b3);
    }

    #[test]
    fn seg_codes_closed_and_details() {
        let all = [
            DupKeyError::UnknownKey,
            DupKeyError::EmptyKey,
            DupKeyError::KeyTooLong,
            DupKeyError::ValueTooLong,
            DupKeyError::BoolValInvalid,
            DupKeyError::RingFull,
            DupKeyError::StreamSealed,
            DupKeyError::StreamNotSealed,
            DupKeyError::SnapshotSealed,
            DupKeyError::SlotOutOfRange,
            DupKeyError::KeyNotFound,
            DupKeyError::FingerprintMismatch,
            DupKeyError::AuditFailed,
            DupKeyError::HealthFloorHit,
            DupKeyError::ReportBudgetExceeded,
            DupKeyError::InvalidParam,
        ];
        // 16 码互异、全落闭集、每变体诊断非空且中文可操作（含「。」）。
        let mut i = 0;
        while i < all.len() {
            assert!(all[i].in_segment());
            let mut j = i + 1;
            while j < all.len() {
                assert_ne!(all[i].code(), all[j].code());
                j += 1;
            }
            let d = all[i].detail();
            assert!(!d.is_empty());
            assert!(d.contains('。'), "诊断须句读可读：{}", d);
            i += 1;
        }
        assert_eq!(all.len(), 16);
    }

    // ── 键空间（24 键闭集 + F0586 对齐）────────────────────────────

    #[test]
    fn key_space_24_closed() {
        // 三型 8/8/8 分布（F0583 白名单口径）。
        let mut bools = 0;
        let mut ints = 0;
        let mut strs = 0;
        let mut i = 0;
        while i < KEY_N {
            match KEY_TABLE[i].kind {
                KeyKind::Bool => bools += 1,
                KeyKind::Int => ints += 1,
                KeyKind::Str => strs += 1,
            }
            // 每键来源注记非空（键空间冻结可追溯）。
            assert!(!KEY_TABLE[i].note.is_empty());
            i += 1;
        }
        assert_eq!((bools, ints, strs), (8, 8, 8));
        // 查表命中：24 键全命中且键位 = 表位。
        let mut k = 0;
        while k < KEY_N {
            assert_eq!(key_index(KEY_TABLE[k].key), Some(k));
            assert!(is_whitelisted(KEY_TABLE[k].key));
            assert_eq!(key_kind_of(KEY_TABLE[k].key), Some(KEY_TABLE[k].kind));
            assert_eq!(key_name(k), KEY_TABLE[k].key);
            k += 1;
        }
        // 越界键名查询零 panic（空串口径）。
        assert_eq!(key_name(KEY_N), "");
        assert_eq!(key_index("no.such.key"), None);
        assert!(!is_whitelisted("no.such.key"));
        assert_eq!(key_kind_of("no.such.key"), None);
    }

    #[test]
    fn key_space_aligns_f0586() {
        // int/str 16 键与 F0586 两张域表逐字全等（顺序一致）。
        let mut i = 0;
        while i < 8 {
            assert_eq!(KEY_TABLE[BOOL_N + i].key, F0586_INT_KEYS[i]);
            assert_eq!(KEY_TABLE[BOOL_N + i].kind, KeyKind::Int);
            assert_eq!(KEY_TABLE[BOOL_N + 16 + i].key, F0586_STR_KEYS[i]);
            assert_eq!(KEY_TABLE[BOOL_N + 16 + i].kind, KeyKind::Str);
            i += 1;
        }
        // 对齐声明原文含两侧模块名（可追溯）。
        assert!(VALDOM_ALIGNMENT.contains("unx0586_valdom"));
    }

    // ── 布尔词形规范化 ─────────────────────────────────────────────

    #[test]
    fn bool_canonicalization() -> Result<(), DupKeyError> {
        assert_eq!(canonical_bool("true")?, true);
        assert_eq!(canonical_bool("on")?, true);
        assert_eq!(canonical_bool("1")?, true);
        assert_eq!(canonical_bool("yes")?, true);
        assert_eq!(canonical_bool("false")?, false);
        assert_eq!(canonical_bool("off")?, false);
        assert_eq!(canonical_bool("0")?, false);
        assert_eq!(canonical_bool("no")?, false);
        // 闭集外全拒（含 Linux 短形 t/f——冻结不入表，拼写歧义口径）。
        for bad in ["t", "f", "TRUE", "On", "maybe", "", "2", "y"] {
            let r = canonical_bool(bad);
            assert!(matches!(r, Err(DupKeyError::BoolValInvalid)), "误收 {}", bad);
        }
        Ok(())
    }

    // ── 首现口径核心（J 判据）──────────────────────────────────────

    #[test]
    fn first_win_basic() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        // 同键三现：512 / 1024 / 2048 —— 首现 512 生效，后现全忽略。
        let o1 = s.push("heap.mb", "512")?;
        assert!(o1.first_win);
        assert_eq!(o1.occurrence, 1);
        assert_eq!(o1.dup_no, 0);
        let o2 = s.push("heap.mb", "1024")?;
        assert!(!o2.first_win);
        assert_eq!(o2.occurrence, 2);
        assert_eq!(o2.dup_no, 1);
        let o3 = s.push("heap.mb", "2048")?;
        assert!(!o3.first_win);
        assert_eq!(o3.occurrence, 3);
        assert_eq!(o3.dup_no, 2);
        // 生效值恒等于首现值（首现口径本证）。
        assert_eq!(s.ledger().effective_str("heap.mb")?, "512");
        // 重复计数 2/2。
        assert_eq!(s.ledger().dup_count_of("heap.mb")?, 2);
        assert_eq!(s.ledger().total_duplicates(), 2);
        assert_eq!(s.ledger().occurrences_of_key("heap.mb")?, 3);
        // 首现断言面（键名口径）：(生效 1, 总现 3, 重复 2, 生效=首现)。
        let face = s.ledger().first_win_face("heap.mb")?;
        assert_eq!(face, (1, 3, 2, true));
        // 结果面人话。
        assert!(o1.text().contains("首现入账"));
        assert!(o3.text().contains("重复第2笔"));
        Ok(())
    }

    #[test]
    fn triple_inject_face_j_criteria() -> Result<(), DupKeyError> {
        // J 判据核心：注入同键 3 现 —— 首现生效 1/1 断言 + 重复计数 2/2。
        let f = FirstWinLedger::triple_inject_face("heap.mb", "512", "1024", "2048")?;
        assert_eq!(f.occurrences, 3);
        assert_eq!(f.effective_entries, 1);
        assert!(f.effective_is_first);
        assert_eq!(f.dup_counted, 2);
        assert_eq!(f.dup_expected, 2);
        assert!(f.pass);
        assert!(f.line.contains("生效 1/1"));
        assert!(f.line.contains("2/2"));
        // 全同值三现同样 1/1 + 2/2（同值重复路径）。
        let f2 = FirstWinLedger::triple_inject_face("loglevel", "3", "3", "3")?;
        assert!(f2.pass);
        assert_eq!(f2.dup_counted, 2);
        // scratch 自证零染主账：face 内部独立账本。
        let mut s = ParamStream::new();
        s.push("heap.mb", "64")?;
        assert_eq!(s.ledger().total_duplicates(), 0);
        Ok(())
    }

    #[test]
    fn same_vs_diff_split() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        // 布尔键：on 与 true 规范化同位 → 同值重复。
        s.push("watchdog", "on")?;
        s.push("watchdog", "true")?;
        // off 异位 → 异值重复。
        s.push("watchdog", "off")?;
        // 整数键：3/3 同值、3/7 异值（原文逐字节口径）。
        s.push("loglevel", "3")?;
        s.push("loglevel", "3")?;
        s.push("loglevel", "7")?;
        assert_eq!(s.ledger().same_val_dups(), 2); // watchdog true + loglevel 3
        assert_eq!(s.ledger().diff_val_dups(), 2); // watchdog off + loglevel 7
        assert_eq!(s.ledger().total_duplicates(), 4);
        // 整数前导零口径：0400 与 400 异值（compare_note 冻结）。
        s.push("dma.win.mb", "64")?;
        s.push("dma.win.mb", "064")?;
        assert_eq!(s.ledger().diff_val_dups(), 3);
        assert!(compare_note().contains("逐字节"));
        Ok(())
    }

    // ── 重复簿可查 ─────────────────────────────────────────────────

    #[test]
    fn dup_book_query() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        s.push("heap.mb", "512")?;
        s.push("loglevel", "6")?;
        s.push("heap.mb", "1024")?;
        s.push("heap.mb", "2048")?;
        // 键 → 重复值清单 + 每次出现位置（全局序 3/4）。
        let book = s.ledger().dup_book_of("heap.mb")?;
        assert_eq!(book.len(), 2);
        assert_eq!(book[0].ordinal, 3);
        assert_eq!(book[1].ordinal, 4);
        assert!(book[0].raw.eq_str("1024"));
        assert!(book[1].raw.eq_str("2048"));
        assert!(!book[0].same_as_effective);
        assert!(book[0].text().contains("异值"));
        // 无重复键 → 空清单（合法答案）。
        let empty = s.ledger().dup_book_of("loglevel")?;
        assert!(empty.is_empty());
        // 表外键 → KeyNotFound。
        assert!(matches!(
            s.ledger().dup_book_of("no.such.key"),
            Err(DupKeyError::KeyNotFound)
        ));
        // 整簿与逐条（越界上码）。
        assert_eq!(s.ledger().dup_book_len(), 2);
        let e0 = s.ledger().dup_entry(0)?;
        assert_eq!(e0.key_idx as usize, key_index("heap.mb").unwrap_or(255));
        assert!(matches!(
            s.ledger().dup_entry(2),
            Err(DupKeyError::SlotOutOfRange)
        ));
        // 流侧：键 → 全部出现位置（含首现，序 1/3/4）。
        let occ = s.occurrences_of("heap.mb")?;
        assert_eq!(occ.len(), 3);
        assert_eq!(occ[0].ordinal, 1);
        assert_eq!(occ[1].ordinal, 3);
        assert_eq!(occ[2].ordinal, 4);
        assert_eq!(s.occurrence(0)?.ordinal, 1);
        assert!(matches!(s.occurrence(9), Err(DupKeyError::SlotOutOfRange)));
        Ok(())
    }

    // ── 五卫拒收面 ─────────────────────────────────────────────────

    #[test]
    fn guards_reject_all() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        // 空键。
        assert!(matches!(s.push("", "1"), Err(DupKeyError::EmptyKey)));
        // 键超长（>24 字节快速拒收）。
        let long_key = "kkkkkkkkkkkkkkkkkkkkkkkkkk"; // 25 字节
        assert_eq!(long_key.as_bytes().len(), 25);
        assert!(matches!(s.push(long_key, "1"), Err(DupKeyError::KeyTooLong)));
        // 值超长（>64 字节）。
        let long_val = "vvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvvv"; // 65 字节
        assert_eq!(long_val.as_bytes().len(), 65);
        assert!(matches!(s.push("hostname", long_val), Err(DupKeyError::ValueTooLong)));
        // 布尔词形闭集外。
        assert!(matches!(s.push("watchdog", "maybe"), Err(DupKeyError::BoolValInvalid)));
        // 未知键（白名单外）。
        assert!(matches!(s.push("evil.key", "1"), Err(DupKeyError::UnknownKey)));
        // 拒收全入计数器与事件账（账不丢）。
        let c = s.ledger().counters();
        assert_eq!(c.key_len_rejects, 2); // 空键 + 超长键同族
        assert_eq!(c.val_len_rejects, 1);
        assert_eq!(c.bool_rejects, 1);
        assert_eq!(c.unknown_rejects, 1);
        assert_eq!(c.total_seen, 5);
        assert_eq!(s.ledger().effective_count(), 0); // 生效面零染
        assert!(!s.ledger().events().is_empty());
        Ok(())
    }

    #[test]
    fn sealed_stream_blocks_writes() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        s.push("heap.mb", "512")?;
        s.seal()?;
        // 重复封流。
        assert!(matches!(s.seal(), Err(DupKeyError::StreamSealed)));
        // 封后写 100% 拦截（合法键+合法值也拦）。
        assert!(matches!(s.push("heap.mb", "1024"), Err(DupKeyError::StreamSealed)));
        assert!(matches!(s.push("loglevel", "6"), Err(DupKeyError::StreamSealed)));
        assert_eq!(s.ledger().counters().sealed_writes, 2);
        // 封后账面不再变化（重复数冻结）。
        assert_eq!(s.ledger().total_duplicates(), 0);
        Ok(())
    }

    #[test]
    fn ring_full_rejects() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        // 64 槽 = 256 字节行理论极值；同键 64 现灌满（重复也占槽——
        // 流是出现日志不是键表）。
        let mut i = 0;
        while i < 64 {
            let r = s.push("heap.mb", "512");
            assert!(r.is_ok(), "第 {} 笔应入槽", i + 1);
            i += 1;
        }
        assert_eq!(s.len(), 64);
        assert_eq!(s.free_slots(), 0);
        // 第 65 笔 → RingFull（拒收不覆盖）。
        assert!(matches!(s.push("heap.mb", "512"), Err(DupKeyError::RingFull)));
        assert_eq!(s.ledger().counters().ring_overflows, 1);
        assert_eq!(s.len(), 64);
        // 账面：首现 1 + 重复 63。
        assert_eq!(s.ledger().dup_count_of("heap.mb")?, 63);
        Ok(())
    }

    #[test]
    fn push_batch_prefix_semantics() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        // 前缀语义：拒收即停，成功笔保留。
        let r = s.push_batch(&[
            ("heap.mb", "512"),
            ("loglevel", "6"),
            ("evil.key", "1"), // 此笔拒
            ("hostname", "varix"), // 不会到达
        ]);
        assert!(matches!(r, Err(DupKeyError::UnknownKey)));
        assert_eq!(s.len(), 2);
        assert_eq!(s.ledger().effective_count(), 2);
        // 干净批次全过。
        let mut s2 = ParamStream::new();
        let outs = s2.push_batch(&[("heap.mb", "512"), ("heap.mb", "512")])?;
        assert_eq!(outs.len(), 2);
        assert!(outs[0].first_win);
        assert!(!outs[1].first_win);
        Ok(())
    }

    // ── 快照面（首现快照 + 指纹 + 断言面）──────────────────────────

    #[test]
    fn snapshot_lifecycle() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        // 未封先拍 → StreamNotSealed。
        assert!(matches!(s.snapshot(), Err(DupKeyError::StreamNotSealed)));
        for (k, v) in batch_a() {
            let _ = s.push(k, v);
        }
        s.seal()?;
        let snap = s.snapshot()?;
        // 一拍槽：二拍 → SnapshotSealed。
        assert!(matches!(s.snapshot(), Err(DupKeyError::SnapshotSealed)));
        assert!(s.is_snapshot_taken());
        // 快照生效表：4 键（heap.mb/loglevel/watchdog/hostname）。
        assert_eq!(snap.eff.len(), 4);
        assert_eq!(snap.effective_str("heap.mb")?, "512");
        assert_eq!(snap.effective_str("loglevel")?, "6");
        assert_eq!(snap.effective_str("watchdog")?, "on");
        assert_eq!(snap.effective_str("hostname")?, "varix-node-1");
        // 重复副本：总 3（heap.mb 2 + watchdog 1 + loglevel 1 = 4？——
        // 批次 A：heap.mb 3 现 2 重复、watchdog 2 现 1 重复（on≡true
        // 同值）、loglevel 2 现 1 重复（6→3 异值）→ 总 4。
        assert_eq!(snap.total_dups, 4);
        assert_eq!(snap.same_val_dups, 1); // watchdog true
        assert_eq!(snap.diff_val_dups, 3); // heap.mb×2 + loglevel 3
        // 指纹复核（结构自证）。
        snap.verify_fingerprint()?;
        // 首现断言面：一键一槽 + 生效=首现 + 分列守恒。
        let face = snap.assert_face(&s)?;
        assert!(face.one_slot_per_key);
        assert!(face.effective_equals_first);
        assert!(face.dups_split_ok);
        assert!(face.pass);
        assert_eq!(face.keys, 4);
        // 复扫对拍（F0594 口径承接：流与快照互证）。
        snap.reverify(&s)?;
        // 摘要与逐行渲染（键表序：watchdog=idx4 在 heap.mb=idx12 前）。
        assert!(snap.render_summary().contains("首现快照"));
        let table = snap.render_effective_table();
        assert_eq!(table.len(), 4);
        assert!(table[0].contains("watchdog=on"));
        let mut found_heap = false;
        for l in &table {
            if l.contains("heap.mb=512") && l.contains("重复 2 笔被忽略") {
                found_heap = true;
            }
        }
        assert!(found_heap, "heap.mb 行须含生效值与重复注记：{:?}", table);
        // 快照语义随行。
        assert_eq!(snap.semantics, FIRST_WIN_SEMANTICS);
        assert_eq!(snap.version, DK_VERSION);
        Ok(())
    }

    #[test]
    fn snapshot_fingerprint_deterministic() -> Result<(), DupKeyError> {
        // 同批次两流（同推同封）→ 快照指纹全等；不同批次 → 指纹必异。
        let mut a1 = ParamStream::new();
        let mut a2 = ParamStream::new();
        for (k, v) in batch_a() {
            let _ = a1.push(k, v);
            let _ = a2.push(k, v);
        }
        a1.seal()?;
        a2.seal()?;
        let s1 = a1.snapshot()?;
        let s2 = a2.snapshot()?;
        assert_eq!(s1.fingerprint, s2.fingerprint);
        assert_eq!(a1.ledger().fingerprint(), a2.ledger().fingerprint());
        let mut b = ParamStream::new();
        b.push("heap.mb", "513")?; // 微扰一字节
        b.seal()?;
        let sb = b.snapshot()?;
        assert_ne!(s1.fingerprint, sb.fingerprint);
        assert_ne!(a1.ledger().fingerprint(), b.ledger().fingerprint());
        Ok(())
    }

    // ── 人话报告 ───────────────────────────────────────────────────

    #[test]
    fn human_dup_report() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        s.push("heap.mb", "512")?;
        s.push("heap.mb", "1024")?;
        s.push("heap.mb", "2048")?;
        let rep = s.ledger().render_dup_report()?;
        // J 判据人话式样：「键 X 出现 3 次，生效值=首次值 v1，后现值 …被忽略」。
        assert!(rep.contains("键 heap.mb 出现 3 次"));
        assert!(rep.contains("生效值=首次值 512"));
        assert!(rep.contains("1024（异值）"));
        assert!(rep.contains("2048（异值）"));
        assert!(rep.contains("被忽略"));
        // 分列汇总头。
        assert!(rep.contains("同值 0 / 异值 2"));
        // 预算过小 → 显性报错（拒绝空壳渲染）。
        assert!(matches!(
            s.ledger().render_dup_report_with_budget(16),
            Err(DupKeyError::ReportBudgetExceeded)
        ));
        // 无重复键 → 空报告有明确口径注记。
        let mut s2 = ParamStream::new();
        s2.push("loglevel", "6")?;
        let rep2 = s2.ledger().render_dup_report()?;
        assert!(rep2.contains("无重复键"));
        Ok(())
    }

    #[test]
    fn report_budget_truncates() -> Result<(), DupKeyError> {
        // 多键重复灌到超预算：截断护顶 + 尾注不丢账面数。
        let mut s = ParamStream::new();
        let mut i = 0;
        let keys = [
            "heap.mb", "loglevel", "stack.kb", "dma.win.mb",
            "console.cols", "console.rows", "uart.baud",
        ];
        while i < keys.len() {
            let _ = s.push(keys[i], "1");
            let _ = s.push(keys[i], "2");
            let _ = s.push(keys[i], "3");
            i += 1;
        }
        let small = s.ledger().render_dup_report_with_budget(200)?;
        assert!(small.len() <= 200);
        assert!(small.contains("截断"));
        // 账面数不受截断影响。
        assert_eq!(s.ledger().total_duplicates(), 14);
        Ok(())
    }

    // ── 白名单联动 + 双重违规 ───────────────────────────────────────

    #[test]
    fn whitelist_linkage_and_double_violation() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        s.push("heap.mb", "512")?;
        s.push("heap.mb", "1024")?;
        // 白名单外重复：同一未知键两现。
        assert!(matches!(s.push("evil.key", "1"), Err(DupKeyError::UnknownKey)));
        assert!(matches!(s.push("evil.key", "2"), Err(DupKeyError::UnknownKey)));
        // 生效面零染（未知键永不入生效表）。
        assert!(!s.ledger().is_present("evil.key"));
        assert!(matches!(
            s.ledger().effective_cell("evil.key"),
            Err(DupKeyError::KeyNotFound)
        ));
        // 复现账 1 条 + 双重违规 1 笔。
        assert_eq!(s.ledger().unknown_repeats().len(), 1);
        assert_eq!(s.ledger().double_violation_count(), 1);
        let ur = s.ledger().unknown_repeats()[0];
        assert!(ur.is_double_violation());
        assert_eq!(ur.count, 2);
        assert!(ur.text().contains("双重违规"));
        // 双重违规人话清单：两罪并记 + 全量弃置。
        let lines = s.ledger().double_violation_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("不在白名单"));
        assert!(lines[0].contains("重复"));
        // 白名单联动审计：重复簿全在白名单内；白名单外重复独立分账。
        let aud = s.ledger().whitelist_dup_audit();
        assert!(aud.ok);
        assert_eq!(aud.dup_entries, 1);
        assert_eq!(aud.outside_whitelist, 0);
        assert_eq!(aud.double_violations, 1);
        Ok(())
    }

    // ── 恒等式自审 + 健康分 ────────────────────────────────────────

    #[test]
    fn audit_identities_hold() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        for (k, v) in batch_a() {
            let _ = s.push(k, v);
        }
        let _ = s.push("evil.key", "9"); // 未知键一现
        s.seal()?;
        let _ = s.snapshot()?;
        let rep = s.ledger_mut().audit();
        // 七恒等式全过。
        assert_eq!(rep.rows.len(), 7);
        for (name, ok) in &rep.rows {
            assert!(ok, "恒等式破：{}", name);
        }
        assert!(rep.ok);
        s.ledger_mut().audit_gate()?;
        assert_eq!(s.ledger().last_audit_ok(), Some(true));
        Ok(())
    }

    #[test]
    fn health_score_and_floor() -> Result<(), DupKeyError> {
        // 净账（零拒收零重复）满分。
        let mut clean = ParamStream::new();
        clean.push("heap.mb", "512")?;
        assert_eq!(clean.ledger().health(), 1_000);
        assert!(clean.ledger().health_gate().is_ok());
        // 重复是已入账正常现象：只扣卫生分（每笔 1 分）。
        let mut dups = ParamStream::new();
        dups.push("heap.mb", "512")?;
        dups.push("heap.mb", "512")?;
        assert_eq!(dups.ledger().health(), 999);
        assert!(dups.ledger().health_gate().is_ok());
        // 违规压分：同未知键 30 现 → 未知拒收 30×15=450 + 双重违规 40
        // = 490 扣 → 510 < 600 → HealthFloorHit。
        let mut dirty = ParamStream::new();
        let mut i = 0;
        while i < 30 {
            let _ = dirty.push("evil.key", "1");
            i += 1;
        }
        assert!(matches!(
            dirty.ledger().health_gate(),
            Err(DupKeyError::HealthFloorHit)
        ));
        // 明细人话可见「分去哪了」。
        let hr = dirty.ledger().health_report();
        assert!(hr.contains("未知键拒收×15=450"));
        assert!(hr.contains("双重违规×40=40"));
        // 恒等式在脏账上也守恒（拒收也是账）。
        dirty.ledger_mut().audit_gate()?;
        Ok(())
    }

    // ── 双跑确定性对拍 ─────────────────────────────────────────────

    #[test]
    fn dual_run_deterministic() -> Result<(), DupKeyError> {
        let rep = dual_run(&batch_a())?;
        assert!(rep.outcomes_match);
        assert_eq!(rep.fp_a, rep.fp_b);
        assert_eq!(rep.snap_fp_a, rep.snap_fp_b);
        assert!(rep.all_match);
        assert_eq!(rep.first_diverge_at, batch_a().len());
        // 闸口径。
        let via_gate = dual_run_gate(&batch_a())?;
        assert!(via_gate.all_match);
        // 空批次 → InvalidParam（空批次的确定性无意义）。
        assert!(matches!(dual_run(&[]), Err(DupKeyError::InvalidParam)));
        // 不同批次指纹必异（对拍面的检出方向）。
        let other: Vec<(&str, &str)> = vec![("heap.mb", "513")];
        let rep2 = dual_run(&other)?;
        assert_ne!(rep.fp_a, rep2.fp_a);
        Ok(())
    }

    // ── 注入面（3/3 全检出）────────────────────────────────────────

    #[test]
    fn injection_suite_3_of_3() {
        let suite = injection_suite();
        assert!(suite.triple.caught);
        assert!(suite.diff_val.caught);
        assert!(suite.unknown_dup.caught);
        assert_eq!(suite.caught, 3);
        assert_eq!(suite.total, 3);
        assert!(suite.all_caught());
        assert_eq!(suite.permille(), 1_000);
        // 逐用例人话。
        assert!(suite.triple.detail.contains("生效 1/1"));
        assert!(suite.triple.detail.contains("2/2"));
        assert!(suite.diff_val.detail.contains("同值 1"));
        assert!(suite.unknown_dup.detail.contains("双重违规 1"));
        // 注入闸。
        let via_gate = injection_gate();
        assert!(via_gate.is_ok());
    }

    // ── 事件账滑窗 ─────────────────────────────────────────────────

    #[test]
    fn event_ring_slides() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        // 40 笔首现 + 重复 > 32 槽：滑出最旧、dropped 计数。
        let mut i = 0;
        while i < 20 {
            let _ = s.push("heap.mb", "512"); // 首现 + 重复
            let _ = s.push("loglevel", "6");
            i += 1;
        }
        let ev = s.ledger().events();
        assert!(ev.len() <= EVENT_CAP);
        assert!(ev.dropped() > 0);
        // 按类过滤：DupSame 类存在且可数。
        let dups = ev.of_kind(EventKind::DupSame);
        assert!(!dups.is_empty());
        // 渲染含类名中文与滑出注记。
        let text = ev.render();
        assert!(text.contains("同值重复忽略"));
        assert!(text.contains("滑出"));
        Ok(())
    }

    // ── FNV 公开向量 ───────────────────────────────────────────────

    #[test]
    fn fnv64_public_vectors() {
        assert_eq!(fnv64(b""), 0xcbf29ce484222325);
        assert_eq!(fnv64(b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv64(b"foobar"), 0x85944171f73967e8);
    }

    // ── 值单元与 lossy 渲染 ────────────────────────────────────────

    #[test]
    fn valcell_and_lossy() -> Result<(), DupKeyError> {
        let mut c = ValCell::empty();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
        c.store("hello")?;
        assert_eq!(c.len(), 5);
        assert!(c.eq_str("hello"));
        assert!(!c.eq_str("hellx"));
        let c2 = ValCell::from_str("hello")?;
        assert!(c.eq_cell(&c2));
        assert_eq!(c.render(), "hello");
        // 超限拒收且原值保留。
        let long = "x".repeat(65);
        assert!(matches!(c.store(&long), Err(DupKeyError::ValueTooLong)));
        assert!(c.eq_str("hello"));
        // lossy：控制字节转义、可打印 ASCII 原样。
        assert_eq!(lossy_bytes(&[0x00, b'A', 0xff]), "\\x00A\\xff");
        assert_eq!(lossy_bytes(b"plain"), "plain");
        Ok(())
    }

    // ── 账面转储确定性与复位 ────────────────────────────────────────

    #[test]
    fn canonical_state_and_reset() -> Result<(), DupKeyError> {
        let mut a = ParamStream::new();
        let mut b = ParamStream::new();
        for (k, v) in batch_a() {
            let _ = a.push(k, v);
            let _ = b.push(k, v);
        }
        // 同批次两账 → canonical_state 逐字节全等。
        assert_eq!(a.ledger().canonical_state(), b.ledger().canonical_state());
        assert!(a.ledger().canonical_state().contains("DK1|FW|"));
        // 微扰一笔 → 转储必异。
        let _ = b.push("heap.mb", "4096");
        assert_ne!(a.ledger().canonical_state(), b.ledger().canonical_state());
        // 复位成净流：账面全清、可重放。
        let mut r = ParamStream::new();
        r.push("heap.mb", "512")?;
        r.seal()?;
        let _ = r.snapshot()?;
        r.reset();
        assert!(r.is_empty());
        assert!(!r.is_sealed());
        assert!(!r.is_snapshot_taken());
        assert_eq!(r.ledger().effective_count(), 0);
        assert_eq!(r.ledger().total_duplicates(), 0);
        // 复位后重放同批次与首跑指纹全等（复跑口径）。
        let mut ref_stream = ParamStream::new();
        for (k, v) in batch_a() {
            let _ = ref_stream.push(k, v);
        }
        ref_stream.seal()?;
        for (k, v) in batch_a() {
            let _ = r.push(k, v);
        }
        r.seal()?;
        assert_eq!(r.ledger().fingerprint(), ref_stream.ledger().fingerprint());
        Ok(())
    }

    // ── 流统计封面 ─────────────────────────────────────────────────

    #[test]
    fn stream_stats_cover() -> Result<(), DupKeyError> {
        let mut s = ParamStream::new();
        assert!(s.is_empty());
        assert_eq!(s.capacity(), 64);
        for (k, v) in batch_a() {
            let _ = s.push(k, v);
        }
        let st = s.stats();
        assert_eq!(st.occurrences, 8); // batch_a 8 笔全过卫
        assert_eq!(st.distinct_keys, 4);
        assert_eq!(st.dups, 4);
        assert_eq!(st.unknown_rejects, 0);
        assert_eq!(st.other_rejects, 0);
        assert!(!st.sealed);
        assert_eq!(st.free, 56);
        s.seal()?;
        assert!(s.stats().sealed);
        Ok(())
    }

    // ── 快照消费方视角（F0590/F0593 承接样例）─────────────────────

    #[test]
    fn snapshot_consumer_view() -> Result<(), DupKeyError> {
        // 缺省补齐联动样例：未现键查快照 → KeyNotFound（补缺省的信号）。
        let mut s = ParamStream::new();
        s.push("heap.mb", "512")?;
        s.push("heap.mb", "1024")?;
        s.seal()?;
        let snap = s.snapshot()?;
        assert!(snap.effective("stack.kb").is_err()); // 未现 → 补缺省
        assert_eq!(snap.effective("heap.mb")?.render(), "512");
        assert_eq!(snap.dup_count_of("heap.mb")?, 1);
        assert_eq!(snap.dup_count_of("stack.kb")?, 0); // 在表未现 = 0 重复
        assert!(matches!(
            snap.dup_count_of("no.such.key"),
            Err(DupKeyError::KeyNotFound)
        ));
        Ok(())
    }
}
