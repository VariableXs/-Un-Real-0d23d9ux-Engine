//! unx0587_dupkey2 —— UNX-F0587 重复参数账（首现口径）·第二自研实现。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点判据原文（全文抄录）
//! 来源：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//!       第 2128 行（UNX-A1-B30 · 命令行参数账 F581–F600 批次表 · AI-01 承办）：
//!
//!   | UNX-F0587 | 重复参数账（首现口径） | 200 | 已深化 |
//!   | 重复键账（注入同键 3 现——首现生效 1/1 断言+重复计数 2/2），重复簿可查 |
//!
//! J 判据（编号口径）：
//!
//!   UNX-F0587-J1 重复键账（注入同键 3 现——首现生效 1/1 断言+重复计数
//!   2/2），重复簿可查
//!
//! 定位原文（同批相邻条目）：
//!   · UNX-F0586 参数值域核（域断言）——本条键空间的整数/字符串 16 键与其
//!     INT_TABLE/STR_TABLE 逐字对齐（对齐声明见 KEY_SPACE_ALIGN_NOTE）；
//!   · UNX-F0588 引号与转义账承接本条产出的重复簿做字符串边角对拍；
//!   · UNX-F0590 缺省值账、UNX-F0591 参数冲突检出共享本条 24 键白名单。
//!
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 文件名与段位实录（任务纪律：先 ls 后写、不覆盖、不动 mod.rs）：
//!   · `ls kernel/varix/src/unxstar/ | grep unx058` 实测命中
//!     `unx0586_valdom.rs` 与 `unx0587_dupkey.rs`（后者 3715 行，为前批
//!     同任务既有实现，独占诊断段 0x1E90~0x1E9F）——预核名 unx0587_dupkey
//!     已被占，本文件取名 **unx0587_dupkey2**（空闲确认）；
//!   · 诊断段顺位实录：预核段 0x1E90 已被 unx0587_dupkey.rs 占用（其
//!     DK_SEG_BASE=0x1E90）→ 顺位 0x1F10：16 位精确码全树零命中，但同
//!     前缀 grep 命中 svstar2/veu10_corpus.rs:173 的 Unicode 区间字面量
//!     0x1F1E6..=0x1F1FF（非 u16 诊断码、数值不相交；然段位审计以 grep
//!     前缀命中为准，噪声段弃取）→ 顺位 **0x1F50**：词边界逐码 grep
//!     0x1F50..=0x1F5F 共 16/16 零命中（剔除 .mimosa 基线快照），独占
//!     确认（0x1F70 段被 unx2782_ubuddymerge.rs 的 0x1F40..0x1F7F 覆盖、
//!     0x1FB0 段被 unx2801_ubigpoolcap.rs 的 0x1F80..=0x1FBF 覆盖，均排除）。
//!   · 本文件为隔离自包含单件：不注册 mod.rs（任务纪律），经
//!     kernel/_attic/vx1024b_probe_unx0587 探针以 #[path] 直挂编译。
//!
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 首现口径 const 声明（FIRST_WIN_SEMANTICS，模块级冻结，指纹混入）：
//!   「同键多现：首现进生效表并冻结；后现不入生效表、逐笔入重复簿
//!    （值+出现位置+第几现+同异标注）；生效值 ≡ 首现值——由两处独立
//!    写点（影子首现 shadow_note / 生效表 effective_write）对拍结构性
//!    保证；重复计数 = 同键后现次数；该键总现数 = 1（首现）+ 重复计数」。
//!   「首现生效 1/1」＝该键生效命中恰 1 笔（期望 1、实得 1）；
//!   「重复计数 2/2」＝三现注入下重复恰 2 笔（期望 2、实得 2）。
//!
//! 组件清单（全手工定制，无空壳）：
//!   1. KEY_TABLE       键空间 24 键闭集（8 布尔自设计 + 8 整数 + 8 字符串
//!                      与 F0586 域表逐字对齐；逐键来源注记）
//!   2. FIRST_WIN_SEMANTICS / KEY_SPACE_ALIGN_NOTE 两条冻结 const 声明
//!   3. DupKey2Error    诊断码闭集 0x1F50~0x1F5F（16 变体，逐变体中文
//!                      可操作诊断 detail()）
//!   4. ValCell         定长 64 字节值/键名单元（零堆、Copy、上栈安全）
//!   5. canonical_bool  布尔词形规范化（true/on/1/yes、false/off/0/no
//!                      → 规范位，供「词形异同值」分列）
//!   6. DiffClass       后现差异标注三分类（同值/布尔词形异同值/异值）
//!   7. EventKind/Event/EventRing  事件账 32 槽滑窗（14 事件类逐类中文）
//!   8. DupOcc/DupRecord/DupView   重复簿（键→重复值清单+每次出现位置+
//!                      第几现+同异标注；可查面 dup_query/positions_of）
//!   9. UnknownOcc/UnknownLedger   未知键复现账（白名单外重复双重违规
//!                      分账的数据面）
//!  10. IdentityReport/WhitelistAudit/DupStats  自审三面（恒等式/白名单
//!                      联动/重复统计）
//!  11. FirstWinLedger  首现口径账主体：生效表 24 槽双写点（影子+生效）+
//!                      每键重复计数 + 同值/异值分列 + 重复簿 + 未知键
//!                      复现账 + 人话报告 + 恒等式自审 + 白名单联动审计
//!                      + 健康分 + 账面 FNV 指纹
//!  12. ParamStream     参数流：定长环形 64 槽（键,值）流式登记 + 六闸
//!                      （封流/空键/键长/值长/白名单/环形容量）+ 封流
//!  13. FirstWinSnapshot 首现快照：封流后生效表封账副本 + 快照指纹
//!                      （域分隔前缀，与账面指纹域分离）+ 计数面
//!  14. TripleProof     首现断言面（注入同键 3 现自证：生效 1/1 + 计数
//!                      2/2；首现值+两笔后现值随证）
//!  15. dual_run        双跑确定性对拍（同批次两跑：推入结果逐笔全等 +
//!                      账面指纹全等 + 快照指纹全等）
//!  16. injection_face  三注入面（同键三现 1/1+2/2、异值重复分列、未知
//!                      键重复双重违规）各检出 = 3/3
//!  17. fnv64           FNV-1a 64 位指纹（公开参考向量编译期锁基）
//!  18. render/explain  重复键人话中文报告（「键 X 出现 3 次，生效值=
//!                      首次值 v1，后现值 v2/v3 被忽略」式）+ 同值/异值
//!                      分列 + 双重违规清单 + 预算截断护顶
//!
//! 性能逐项分解：
//!   push：封流 O(1) + 空键 O(1) + 键长 O(1) + 值长 O(1) + 键查表
//!         O(24×字节) + 环形容量 O(1) + 账本入账 O(1)；同异判定
//!         O(值字节)（布尔词形异判定另加 O(2×4×字节)）；
//!   查询：dup_query O(重复簿记录数 + 键查表)；positions_of 同口径；
//!   报告：O(生效键 + 重复笔 × 值字节)，预算 1024 字节截断护顶；
//!   快照：O(24 槽)；快照指纹 O(账面字节)；账面指纹 O(账面字节)；
//!   对拍：O(批 × push + 账面字节)；注入面 O(3 × 常数)；
//!   健康：O(24 + 重复簿 + 未知账 + 白名单反查)。
//!
//! 纪律声明：no_std + alloc；零 unwrap/expect/panic!/运行期 assert!
//!   （编译期 `const _: () = assert!(…)` 允许；测试体内 assert! 是断言面
//!   本身，与内核路径分开）；可失败全走 Result<T, DupKey2Error>；计数
//!   递增一律 wrapping、健康分扣减一律 saturating、报告拼接超预算显性
//!   截断不丢账（截笔计数入恒等式）；>64KB 结构体不上栈（本模块最大
//!   内联结构 FirstWinLedger ≈ 4KB、FirstWinSnapshot ≈ 2KB，簿/账/事件
//!   全走 Vec 堆）；const fn 内只用 while+索引+手工比较，不用
//!   slice::get/Vec 索引/Ord::min/max/derive PartialEq 的 ==/for 循环。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════
// 版本与全局常量（来源逐条注记）
// ═══════════════════════════════════════════════════════════════════════

/// 模块版本（第二自研实现，与同任务前批实现 unx0587_dupkey-v1 相互独立）。
pub const DK2_VERSION: &str = "unx0587_dupkey2-v1";

/// 诊断段基址（0x1F50 段 · 本模块独占 · 词边界逐码 grep 16/16 零命中；
/// 0x1E90 被前批同任务实现占用、0x1F10 前缀噪声、0x1F70/0x1FB0 被邻段
/// 覆盖——顺位实录见头注「文件名与段位实录」一节）。
pub const DK2_SEG_BASE: u16 = 0x1F50;

/// 诊断段顶（基址 + 0x0F = 0x1F5F，16 码闭集）。
pub const DK2_SEG_TOP: u16 = 0x1F5F;

/// 首现口径冻结声明（判据 J1 的可执行化文本；账面/快照两域指纹均混入，
/// 防跨域指纹碰撞，亦防语义静默漂移）。
pub const FIRST_WIN_SEMANTICS: &str = "同键多现：首现进生效表并冻结；后现不入生效表、逐笔入重复簿；生效值≡首现值（双写点对拍保证）；重复计数=后现次数；总现数=1+重复计数";

/// 键空间对齐声明（跨模块对账以本条为准）：
/// 整数 8 键与字符串 8 键逐字对齐 unx0586_valdom 的 INT_TABLE/STR_TABLE
/// （键名一致、表内顺序一致）；布尔 8 键为本条自设计（F0583 三型分布
/// 的补全面）。
pub const KEY_SPACE_ALIGN_NOTE: &str = "int/str 16 键与 unx0586_valdom INT_TABLE/STR_TABLE 逐字一致；bool 8 键本条自设计";

/// 键空间键数（F0583 白名单口径：24 键三型分布 8+8+8）。
pub const KEY_N: usize = 24;

/// 键名截断保存预算（24 字节；与 unx0586_valdom KEY_KEEP 同值同源——
/// 白名单最长键 13 字节，24 字节为含点命名空间的护顶预算）。
pub const KEY_KEEP: usize = 24;

/// 值字节上限（64 字节；F0589 参数行长度守卫的「单 token 64 界」）。
pub const VAL_MAX: usize = 64;

/// 参数流环形容量（64 槽；自设计预算：24 键全键多现注入 + 余量，同域
/// 模块环形 64 槽量级惯例）。
pub const RING_CAP: usize = 64;

/// 重复簿容量（48 笔；自设计预算：单流环形 64 槽内即可触发的截笔验证面
/// —— 63 笔后现极端重复流可越容；满后截笔，计数不丢、恒等式护住）。
pub const DUP_BOOK_CAP: usize = 48;

/// 未知键复现账容量（48 笔；自设计预算：未知键不入环形，独立账面 48 笔
/// 滑窗 + dropped 计数）。
pub const UNKNOWN_CAP: usize = 48;

/// 事件账滑窗上限（32 槽；与 unx0586_valdom EVENT_CAP 同值同源）。
pub const EVENT_CAP: usize = 32;

/// 人话报告字节预算（1024 字节；自设计：串口诊断单屏上限量级）。
pub const REPORT_BUDGET: usize = 1024;

/// 单键解释字节预算（256 字节；自设计：单行诊断量级）。
pub const EXPLAIN_BUDGET: usize = 256;

/// 参数流摘要字节预算（512 字节；自设计：环形账半屏渲染量级）。
pub const STREAM_SUM_BUDGET: usize = 512;

/// 健康分满刻度（1000 定点；与同域模块 health() 口径一致）。
pub const HEALTH_FULL: u32 = 1000;

/// 未知键键位哨兵（事件/簿条目中 key_idx 无关位；0xFF 不落 0..24）。
pub const KEY_IDX_NONE: u8 = 0xFF;

/// 布尔真词形闭集（4 词；自设计：Linux 布尔参数常用词形族 true/on/1/yes）。
pub const BOOL_TRUE_WORDS: [&str; 4] = ["true", "on", "1", "yes"];

/// 布尔假词形闭集（4 词；自设计：与真词形族对偶 false/off/0/no）。
pub const BOOL_FALSE_WORDS: [&str; 4] = ["false", "off", "0", "no"];

/// FNV-1a 64 位偏移基（公开参考值：IETF draft-eastlake-fnv 附录 B）。
pub const FNV64_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 位素数（公开参考值：IETF draft-eastlake-fnv 附录 B）。
pub const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 账面指纹域分隔前缀（与快照指纹域分离，防跨域同值碰撞；自设计）。
pub const FP_DOMAIN_STATE: &[u8] = b"dk2.state.v1";

/// 快照指纹域分隔前缀（与账面指纹域分离；自设计）。
pub const FP_DOMAIN_SNAP: &[u8] = b"dk2.snap.v1";

// ═══════════════════════════════════════════════════════════════════════
// FNV-1a 64 位指纹（const fn · while+索引实现，编译期向量锁基）
// ═══════════════════════════════════════════════════════════════════════

/// FNV-1a 64 位链式折叠（const fn：while+索引，禁 for/slice::get）。
pub const fn fnv64(mut h: u64, bytes: &[u8]) -> u64 {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        h ^= b as u64;
        h = h.wrapping_mul(FNV64_PRIME);
        i += 1;
    }
    h
}

/// FNV-1a 64 位独立入口（偏移基起步；对 str 便捷封装，运行期）。
pub fn fnv64_str(s: &str) -> u64 {
    fnv64(FNV64_OFFSET, s.as_bytes())
}

// ═══════════════════════════════════════════════════════════════════════
// 诊断码闭集（0x1F50~0x1F5F，每变体 detail() 中文可操作诊断）
// ═══════════════════════════════════════════════════════════════════════

/// 重复参数账诊断码闭集（#[repr(u16)]，码值即段内身份；与同任务前批
/// 实现 unx0587_dupkey::DupKeyError 相互独立，以 2 后缀与段位区分）。
#[repr(u16)]
#[derive(Clone, Copy, Debug)]
pub enum DupKey2Error {
    /// 0x1F50 键不在 24 键闭集（白名单外——F0583 口径；复现入未知键账）。
    UnknownKey = 0x1F50,
    /// 0x1F51 空键（零字节键名，无法查表）。
    EmptyKey = 0x1F51,
    /// 0x1F52 键超长（> KEY_KEEP=24 字节）。
    KeyTooLong = 0x1F52,
    /// 0x1F53 值超长（> VAL_MAX=64 字节，F0589 token 界）。
    ValueTooLong = 0x1F53,
    /// 0x1F54 参数流环形满（64 槽占满仍推入）。
    RingFull = 0x1F54,
    /// 0x1F55 封流后写（seal 之后 push/seal 均落此码）。
    StreamSealed = 0x1F55,
    /// 0x1F56 快照未取前访问快照面（未封流先取快照/断言面）。
    SnapshotAbsent = 0x1F56,
    /// 0x1F57 重复簿查询未命中（该键无重复记录或键不存在）。
    QueryMiss = 0x1F57,
    /// 0x1F58 首现断言破缺（生效表未命中/生效值≠首现值——双写点失拍）。
    FirstWinBroken = 0x1F58,
    /// 0x1F59 重复计数恒等式破缺（计数≠后现笔数/三现口径≠2）。
    CountBroken = 0x1F59,
    /// 0x1F5A 白名单联动破缺（簿内键出闭集/未知账键入闭集）。
    WhitelistBroken = 0x1F5A,
    /// 0x1F5B 双跑对拍不齐（同批次两跑结果/指纹发散）。
    DualMismatch = 0x1F5B,
    /// 0x1F5C 注入面漏检（三注入未全检出）。
    InjectMissed = 0x1F5C,
    /// 0x1F5D 指纹面破缺（快照指纹复算不齐——封账副本被篡改或构造破缺）。
    FpBroken = 0x1F5D,
    /// 0x1F5E 账本恒等式破缺（总现/受理/重复/簿笔四账不闭合）。
    LedgerBroken = 0x1F5E,
    /// 0x1F5F 内部契约破缺（键序越纲/枚举外码——不可继续上账）。
    InternalBroken = 0x1F5F,
}

impl DupKey2Error {
    /// 码值（const fn：编译期断言与注入面对拍取码用）。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 中文可操作诊断（detail）：说什么坏、下一步动哪里。
    pub fn detail(self) -> &'static str {
        match self {
            DupKey2Error::UnknownKey => {
                "键不在 24 键闭集：先查 KEY_TABLE 取合法键清单；白名单外键一律拒收，复现已入未知键复现账（双重违规分账可查）"
            }
            DupKey2Error::EmptyKey => {
                "空键：零字节键名无法查表；核对参数行切分是否漏键名或多余等号"
            }
            DupKey2Error::KeyTooLong => {
                "键超长：超过 KEY_KEEP=24 字节预算；缩短键名或先并入白名单表"
            }
            DupKey2Error::ValueTooLong => {
                "值超长：超过 VAL_MAX=64 字节（F0589 单 token 界）；压缩值或拆行登记"
            }
            DupKey2Error::RingFull => {
                "参数流环形满：RING_CAP=64 槽占满；先封流取快照再开新流，勿向满流续推"
            }
            DupKey2Error::StreamSealed => {
                "封流后写：流已封账（seal 之后只读）；如需续写须开新流重放全批"
            }
            DupKey2Error::SnapshotAbsent => {
                "快照未取：先 seal() 封流再取快照/跑首现断言面（判据口径：流处理完成后）"
            }
            DupKey2Error::QueryMiss => {
                "重复簿查询未命中：该键无重复记录（或键不在闭集）；先用 dup_keys()/dup_stats() 看有重复的键清单"
            }
            DupKey2Error::FirstWinBroken => {
                "首现断言破缺：生效表未命中该键或生效值≠首现值；查双写点（shadow_note/effective_write）是否被旁路"
            }
            DupKey2Error::CountBroken => {
                "重复计数破缺：计数≠后现笔数或三现注入下计数≠2；查 accept_known 重复分支计数与重复簿截笔账是否闭合"
            }
            DupKey2Error::WhitelistBroken => {
                "白名单联动破缺：重复簿内键出了闭集或未知账键进了闭集；查 KEY_TABLE 是否被改动后重跑 whitelist_audit"
            }
            DupKey2Error::DualMismatch => {
                "双跑对拍不齐：同批次两跑推入结果或指纹发散；查账本是否引入时序/外部状态依赖"
            }
            DupKey2Error::InjectMissed => {
                "注入面漏检：三注入（三现自证/异值重复/未知键重复）未全检出；逐面排查后重跑 injection_face"
            }
            DupKey2Error::FpBroken => {
                "指纹面破缺：快照指纹复算不齐；封账副本被写或构造路径破缺——快照必须整体重建"
            }
            DupKey2Error::LedgerBroken => {
                "账本恒等式破缺：总现≠受理+重复或簿笔不闭合；先跑 identity_check() 定位 I1~I6 哪条断"
            }
            DupKey2Error::InternalBroken => {
                "内部契约破缺：键序越纲或枚举外码；此为不可继续上账的结构性错误，须修调用方"
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 键空间 24 键闭集（8 布尔自设计 + 8 整数 + 8 字符串与 F0586 逐字对齐）
// ═══════════════════════════════════════════════════════════════════════

/// 键型三分类（F0583 白名单三型分布口径）。
#[derive(Clone, Copy, Debug)]
pub enum KeyKind {
    /// 布尔键（词形闭集见 BOOL_TRUE_WORDS/BOOL_FALSE_WORDS）。
    Bool,
    /// 整数键（值域判归 F0586，本条只记重复账）。
    Int,
    /// 字符串键（长度/字符集判归 F0586，本条只记重复账）。
    Str,
}

impl KeyKind {
    /// 型名（中文）。
    pub fn text(self) -> &'static str {
        match self {
            KeyKind::Bool => "布尔",
            KeyKind::Int => "整数",
            KeyKind::Str => "字符串",
        }
    }

    /// 型位（报告/统计的固定槽序：0 布尔、1 整数、2 字符串）。
    pub fn slot(self) -> usize {
        match self {
            KeyKind::Bool => 0,
            KeyKind::Int => 1,
            KeyKind::Str => 2,
        }
    }
}

/// 键型相等（const fn：match 手工比较，不 derive PartialEq）。
const fn kind_eq(a: KeyKind, b: KeyKind) -> bool {
    matches!(
        (a, b),
        (KeyKind::Bool, KeyKind::Bool)
            | (KeyKind::Int, KeyKind::Int)
            | (KeyKind::Str, KeyKind::Str)
    )
}

/// 键规格（键名 + 型 + 来源注记；const 表驱动，无堆）。
#[derive(Clone, Copy, Debug)]
pub struct KeySpec {
    /// 键名（点命名空间风格）。
    pub key: &'static str,
    /// 键型。
    pub kind: KeyKind,
    /// 来源注记（对齐声明或自设计依据）。
    pub note: &'static str,
}

/// 键空间 24 键闭集（顺序冻结：0..8 布尔、8..16 整数、16..24 字符串；
/// 整数/字符串 16 键与 unx0586_valdom 的 INT_TABLE/STR_TABLE 逐字对齐
/// ——键名一致、表内顺序一致，跨模块对账以 KEY_SPACE_ALIGN_NOTE 为准）。
pub const KEY_TABLE: [KeySpec; KEY_N] = [
    // ── 布尔 8 键（本条自设计，补全 F0583 三型分布；词形闭集见
    //    BOOL_TRUE_WORDS/BOOL_FALSE_WORDS）──────────────────────────
    // 键 1 nokaslr：关闭内核地址布局随机化。来源：Linux 同名引导参数
    // 惯例；no_std 内核页表布局固定化开关，调试/取证场景常开。
    KeySpec {
        key: "nokaslr",
        kind: KeyKind::Bool,
        note: "自设计：Linux 同名惯例，地址布局固定化",
    },
    // 键 2 verbose：啰嗦引导日志。来源：Linux 同名惯例；旗标形态等价
    // 于 loglevel 高位，与 loglevel 的联判归 F0591 互斥表。
    KeySpec {
        key: "verbose",
        kind: KeyKind::Bool,
        note: "自设计：Linux 同名惯例，啰嗦日志旗标",
    },
    // 键 3 trace.boot：引导期跟踪点开启。来源：本内核引导跟踪面开关，
    // dot 命名空间与 uart.baud 等既有键风格一致。
    KeySpec {
        key: "trace.boot",
        kind: KeyKind::Bool,
        note: "自设计：引导期跟踪点开关",
    },
    // 键 4 mm.huge：大页晋升开启。来源：内存子系统大页策略开关；
    // 与 heap.mb（F0586 域表）同属 mm 命名空间。
    KeySpec {
        key: "mm.huge",
        kind: KeyKind::Bool,
        note: "自设计：大页晋升开关",
    },
    // 键 5 net.off：跳过网络栈初始化。来源：除错场景最小引导面；
    // off 词形落布尔假词形闭集（语义为「开启跳过」的祈使键）。
    KeySpec {
        key: "net.off",
        kind: KeyKind::Bool,
        note: "自设计：网络栈跳过开关",
    },
    // 键 6 fb.off：跳过帧缓冲初始化。来源：无头引导/串口控制台场景。
    KeySpec {
        key: "fb.off",
        kind: KeyKind::Bool,
        note: "自设计：帧缓冲跳过开关",
    },
    // 键 7 usb.off：跳过 USB 探测。来源：引导耗时裁剪（USB 枚举常为
    // 最长非必要路径）。
    KeySpec {
        key: "usb.off",
        kind: KeyKind::Bool,
        note: "自设计：USB 探测跳过开关",
    },
    // 键 8 nmi.watch：NMI 看门狗开启。来源：x86 NMI 看门狗惯例；
    // panic 处置互斥归 F0591。
    KeySpec {
        key: "nmi.watch",
        kind: KeyKind::Bool,
        note: "自设计：NMI 看门狗开关",
    },
    // ── 整数 8 键（与 F0586 INT_TABLE 逐字对齐：键名一致、顺序一致；
    //    值域判归 F0586，本条只做重复记账）──────────────────────────
    // 键 9 loglevel：内核日志级别（F0586 域 [0,7] 步长 1）。
    KeySpec {
        key: "loglevel",
        kind: KeyKind::Int,
        note: "F0586 对齐：INT_TABLE 键 1",
    },
    // 键 10 uart.baud：串口波特率（F0586 域 [9600,4000000]）。
    KeySpec {
        key: "uart.baud",
        kind: KeyKind::Int,
        note: "F0586 对齐：INT_TABLE 键 2",
    },
    // 键 11 console.cols：控制台列数（F0586 域 [20,512]）。
    KeySpec {
        key: "console.cols",
        kind: KeyKind::Int,
        note: "F0586 对齐：INT_TABLE 键 3",
    },
    // 键 12 console.rows：控制台行数（F0586 域 [10,256]）。
    KeySpec {
        key: "console.rows",
        kind: KeyKind::Int,
        note: "F0586 对齐：INT_TABLE 键 4",
    },
    // 键 13 heap.mb：内核堆 MB（F0586 域 [64,4096] 步长 64）。
    KeySpec {
        key: "heap.mb",
        kind: KeyKind::Int,
        note: "F0586 对齐：INT_TABLE 键 5",
    },
    // 键 14 stack.kb：栈 KB（F0586 域 [8,1024] 步长 8）。
    KeySpec {
        key: "stack.kb",
        kind: KeyKind::Int,
        note: "F0586 对齐：INT_TABLE 键 6",
    },
    // 键 15 sched.slice.us：调度片微秒（F0586 域 [1000,1000000] 步长 100）。
    KeySpec {
        key: "sched.slice.us",
        kind: KeyKind::Int,
        note: "F0586 对齐：INT_TABLE 键 7",
    },
    // 键 16 dma.win.mb：DMA 窗 MB（F0586 域 [1,1024]）。
    KeySpec {
        key: "dma.win.mb",
        kind: KeyKind::Int,
        note: "F0586 对齐：INT_TABLE 键 8",
    },
    // ── 字符串 8 键（与 F0586 STR_TABLE 逐字对齐：键名一致、顺序一致；
    //    长度/字符集判归 F0586，本条只做重复记账）────────────────────
    // 键 17 hostname：主机名（F0586 长 32 字节，主机名闭集）。
    KeySpec {
        key: "hostname",
        kind: KeyKind::Str,
        note: "F0586 对齐：STR_TABLE 键 1",
    },
    // 键 18 console.dev：控制台设备（F0586 长 16 字节）。
    KeySpec {
        key: "console.dev",
        kind: KeyKind::Str,
        note: "F0586 对齐：STR_TABLE 键 2",
    },
    // 键 19 boot.id：引导 ID（F0586 长 12 字节，十六进制闭集）。
    KeySpec {
        key: "boot.id",
        kind: KeyKind::Str,
        note: "F0586 对齐：STR_TABLE 键 3",
    },
    // 键 20 log.tag：日志标签（F0586 长 16 字节）。
    KeySpec {
        key: "log.tag",
        kind: KeyKind::Str,
        note: "F0586 对齐：STR_TABLE 键 4",
    },
    // 键 21 video.mode：视频模式（F0586 长 8 字节）。
    KeySpec {
        key: "video.mode",
        kind: KeyKind::Str,
        note: "F0586 对齐：STR_TABLE 键 5",
    },
    // 键 22 locale：区域名（F0586 长 8 字节）。
    KeySpec {
        key: "locale",
        kind: KeyKind::Str,
        note: "F0586 对齐：STR_TABLE 键 6",
    },
    // 键 23 kernel.opt：内核选项（F0586 长 24 字节）。
    KeySpec {
        key: "kernel.opt",
        kind: KeyKind::Str,
        note: "F0586 对齐：STR_TABLE 键 7",
    },
    // 键 24 cmd.prefix：命令前缀（F0586 长 8 字节）。
    KeySpec {
        key: "cmd.prefix",
        kind: KeyKind::Str,
        note: "F0586 对齐：STR_TABLE 键 8",
    },
];

/// 键名查位（O(24×字节) 手工逐字节比较；while+索引，运行期）。
pub fn key_index(key: &str) -> Option<usize> {
    let kb = key.as_bytes();
    let mut i = 0;
    while i < KEY_N {
        let sb = KEY_TABLE[i].key.as_bytes();
        if kb.len() == sb.len() {
            let mut j = 0;
            let mut eq = true;
            while j < kb.len() {
                let a = match kb.get(j) {
                    Some(v) => *v,
                    None => {
                        eq = false;
                        break;
                    }
                };
                let b = match sb.get(j) {
                    Some(v) => *v,
                    None => {
                        eq = false;
                        break;
                    }
                };
                if a != b {
                    eq = false;
                    break;
                }
                j += 1;
            }
            if eq {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 键位取名（越纲返回 None；渲染与对账用）。
pub fn key_name(idx: usize) -> Option<&'static str> {
    if idx < KEY_N {
        Some(KEY_TABLE[idx].key)
    } else {
        None
    }
}

/// 键位取型（越纲返回 None）。
pub fn key_kind_at(idx: usize) -> Option<KeyKind> {
    if idx < KEY_N {
        Some(KEY_TABLE[idx].kind)
    } else {
        None
    }
}

/// 键名取型（闭集外键返回 None）。
pub fn key_kind_of(key: &str) -> Option<KeyKind> {
    match key_index(key) {
        Some(i) => key_kind_at(i),
        None => None,
    }
}

/// 键型计数（const fn：while+索引遍历 const 表；编译期断言 8/8/8 用）。
const fn kind_count(want: KeyKind) -> usize {
    let mut i = 0;
    let mut n = 0;
    while i < KEY_N {
        if kind_eq(KEY_TABLE[i].kind, want) {
            n += 1;
        }
        i += 1;
    }
    n
}

// ═══════════════════════════════════════════════════════════════════════
// 编译期断言（const assert · 允许面）
// ═══════════════════════════════════════════════════════════════════════

const _: () = assert!(KEY_N == 24, "键空间闭集 24 键（F0583 口径）");
const _: () = assert!(DK2_SEG_BASE == 0x1F50, "诊断段基址 0x1F50");
const _: () = assert!(DK2_SEG_TOP == DK2_SEG_BASE + 0x0F, "诊断段顶 0x1F5F");
const _: () = assert!(
    DupKey2Error::UnknownKey as u16 == DK2_SEG_BASE,
    "首码贴段基"
);
const _: () = assert!(
    DupKey2Error::InternalBroken as u16 == DK2_SEG_TOP,
    "末码贴段顶"
);
const _: () = assert!(VAL_MAX == 64, "值上限 64（F0589 token 界）");
const _: () = assert!(KEY_KEEP == 24, "键名预算 24（F0586 同源）");
const _: () = assert!(kind_count(KeyKind::Bool) == 8, "布尔 8 键");
const _: () = assert!(kind_count(KeyKind::Int) == 8, "整数 8 键");
const _: () = assert!(kind_count(KeyKind::Str) == 8, "字符串 8 键");
const _: () = assert!(
    fnv64(FNV64_OFFSET, b"a") == 0xAF63_DC4C_8601_EC8C,
    "FNV-1a 64 参考向量 a"
);
const _: () = assert!(
    fnv64(FNV64_OFFSET, b"foobar") == 0x8594_4171_F739_67E8,
    "FNV-1a 64 参考向量 foobar"
);
const _: () = assert!(
    RING_CAP > KEY_N,
    "环形容量须容纳全键多现注入"
);

// ═══════════════════════════════════════════════════════════════════════
// 定长值单元 ValCell（零堆 · Copy · 上栈安全；值与键名共用）
// ═══════════════════════════════════════════════════════════════════════

/// 定长字节单元（VAL_MAX=64 字节 + u16 长度；≈66 字节，Copy 上栈安全；
/// 生效表/影子表/环形槽/重复簿/未知账共用）。
#[derive(Clone, Copy, Debug)]
pub struct ValCell {
    bytes: [u8; VAL_MAX],
    len: u16,
}

impl ValCell {
    /// 空单元（const fn）。
    pub const fn empty() -> ValCell {
        ValCell {
            bytes: [0; VAL_MAX],
            len: 0,
        }
    }

    /// 按预算装入字节（私有共用；越预算返回传入错误码）。
    fn from_bytes_capped(
        src: &[u8],
        cap: usize,
        err: DupKey2Error,
    ) -> Result<ValCell, DupKey2Error> {
        if src.len() > cap {
            return Err(err);
        }
        let mut cell = ValCell::empty();
        let mut i = 0;
        while i < src.len() {
            match src.get(i) {
                Some(b) => {
                    // i < src.len() <= cap <= VAL_MAX，写位恒在界内。
                    if i < VAL_MAX {
                        cell.bytes[i] = *b;
                    }
                }
                None => break,
            }
            i += 1;
        }
        cell.len = src.len() as u16;
        Ok(cell)
    }

    /// 装值（> VAL_MAX 报 ValueTooLong）。
    pub fn from_val(s: &str) -> Result<ValCell, DupKey2Error> {
        ValCell::from_bytes_capped(s.as_bytes(), VAL_MAX, DupKey2Error::ValueTooLong)
    }

    /// 装键名（> KEY_KEEP 报 KeyTooLong）。
    pub fn from_key(s: &str) -> Result<ValCell, DupKey2Error> {
        ValCell::from_bytes_capped(s.as_bytes(), KEY_KEEP, DupKey2Error::KeyTooLong)
    }

    /// 有效字节数。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 有效字节切片（get 守卫，恒不越界）。
    pub fn as_slice(&self) -> &[u8] {
        let n = self.len as usize;
        if n <= VAL_MAX {
            match self.bytes.get(..n) {
                Some(s) => s,
                None => &[],
            }
        } else {
            &[]
        }
    }

    /// 以 str 视图取值（构造期均为 UTF-8 字节；防御性回退空串）。
    pub fn as_str(&self) -> &str {
        match core::str::from_utf8(self.as_slice()) {
            Ok(s) => s,
            Err(_) => "",
        }
    }

    /// 字节级相等（while 手工比较；不 derive PartialEq）。
    pub fn same_bytes(&self, other: &ValCell) -> bool {
        if self.len != other.len {
            return false;
        }
        let a = self.as_slice();
        let b = other.as_slice();
        let mut i = 0;
        while i < a.len() && i < b.len() {
            let x = match a.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let y = match b.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if x != y {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 混入指纹（值字节 + 长度低字节；长度入指纹防「前缀零字节歧义」）。
    pub fn fold_fnv(&self, h: u64) -> u64 {
        let mut x = fnv64(h, self.as_slice());
        x = fnv64(x, &[(self.len & 0xFF) as u8]);
        x
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 布尔词形规范化（「词形异同值」分列的判定基）
// ═══════════════════════════════════════════════════════════════════════

/// 布尔规范位（词形族的等价类代表）。
#[derive(Clone, Copy, Debug)]
pub enum BoolWord {
    /// 真位（true/on/1/yes）。
    True,
    /// 假位（false/off/0/no）。
    False,
}

/// 规范位相等（const fn：match 手工比较）。
const fn bool_word_eq(a: BoolWord, b: BoolWord) -> bool {
    matches!(
        (a, b),
        (BoolWord::True, BoolWord::True) | (BoolWord::False, BoolWord::False)
    )
}

/// 布尔词形规范化（8 词形 → 2 规范位；非词形返回 None——本条不做值
/// 拒收（值域归 F0585/F0586），仅做词形等价判定）。
pub fn canonical_bool(word: &str) -> Option<BoolWord> {
    let mut i = 0;
    while i < BOOL_TRUE_WORDS.len() {
        if word == BOOL_TRUE_WORDS[i] {
            return Some(BoolWord::True);
        }
        i += 1;
    }
    let mut j = 0;
    while j < BOOL_FALSE_WORDS.len() {
        if word == BOOL_FALSE_WORDS[j] {
            return Some(BoolWord::False);
        }
        j += 1;
    }
    None
}

// ═══════════════════════════════════════════════════════════════════════
// 后现差异标注（同值 / 布尔词形异同值 / 异值 三分类）
// ═══════════════════════════════════════════════════════════════════════

/// 后现差异三分类（判据「后现值差异标注（同值重复 vs 异值重复分列）」
/// 的细化：布尔键词形异而规范位同者单列，不混入字节级异值）。
#[derive(Clone, Copy, Debug)]
pub enum DiffClass {
    /// 同值重复（字节级相同）。
    SameValue,
    /// 同值重复（布尔词形异）——两值分属同规范位的不同词形。
    BoolFormSame,
    /// 异值重复（字节级不同且非同规范位词形）。
    DiffValue,
}

impl DiffClass {
    /// 分类中文短名。
    pub fn text(self) -> &'static str {
        match self {
            DiffClass::SameValue => "同值重复",
            DiffClass::BoolFormSame => "同值重复（布尔词形异）",
            DiffClass::DiffValue => "异值重复",
        }
    }

    /// 两列视图归并位（同值列=SameValue+BoolFormSame；异值列=DiffValue）。
    pub fn same_column(self) -> bool {
        matches!(self, DiffClass::SameValue | DiffClass::BoolFormSame)
    }

    /// 簿条目标签位（指纹/事件 aux 用：1 同值、2 词形异、3 异值）。
    pub fn tag(self) -> u32 {
        match self {
            DiffClass::SameValue => 1,
            DiffClass::BoolFormSame => 2,
            DiffClass::DiffValue => 3,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 事件账（14 事件类 · 32 槽滑窗 · 满后滑出最旧并计 dropped）
// ═══════════════════════════════════════════════════════════════════════

/// 事件类闭集（首现/重复/异值/词形/未知/双重违规/闸拒/封流/快照/
/// 三现/簿溢/对拍/注入/自审）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    /// 首现入账（生效表写入）。
    StreamFirst,
    /// 重复入簿（后现笔）。
    StreamDup,
    /// 异值重复检出。
    DiffRepeat,
    /// 布尔词形异同值检出。
    BoolFormRepeat,
    /// 未知键拒收（复现入未知账）。
    StreamUnknown,
    /// 未知键双重违规（白名单外 + 重复）。
    UnknownDouble,
    /// 流闸拒收（aux：1 空键/2 键超长/3 值超长/4 环形满/5 封流后写）。
    RingReject,
    /// 封流。
    StreamSeal,
    /// 快照取。
    SnapTake,
    /// 三现自证面运行。
    TripleProofEv,
    /// 重复簿溢出截笔。
    BookOverflow,
    /// 双跑对拍运行。
    DualRunEv,
    /// 注入面运行。
    InjectEv,
    /// 账本自审运行。
    IdentityEv,
}

impl EventKind {
    /// 事件类中文短名。
    pub fn text(self) -> &'static str {
        match self {
            EventKind::StreamFirst => "首现入账",
            EventKind::StreamDup => "重复入簿",
            EventKind::DiffRepeat => "异值重复",
            EventKind::BoolFormRepeat => "布尔词形异同值",
            EventKind::StreamUnknown => "未知键拒收",
            EventKind::UnknownDouble => "未知键双重违规",
            EventKind::RingReject => "流闸拒收",
            EventKind::StreamSeal => "封流",
            EventKind::SnapTake => "快照取",
            EventKind::TripleProofEv => "三现自证",
            EventKind::BookOverflow => "重复簿溢出截笔",
            EventKind::DualRunEv => "双跑对拍",
            EventKind::InjectEv => "注入面",
            EventKind::IdentityEv => "账本自审",
        }
    }
}

/// 单条事件（key_idx=0xFF 表示无关键位；ord 为全局出现序号，0=未编序）。
#[derive(Clone, Copy, Debug)]
pub struct Event {
    /// 事件类。
    pub kind: EventKind,
    /// 关联键位（KEY_IDX_NONE=0xFF 表示无）。
    pub key_idx: u8,
    /// 全局出现序号（0=未编序的面板事件）。
    pub ord: u32,
    /// 辅助位（闸拒原因/分类标签/检出数等）。
    pub aux: u32,
}

impl Event {
    /// 事件渲染行（中文）。
    pub fn line(&self) -> String {
        let k = if (self.key_idx as usize) < KEY_N {
            match key_name(self.key_idx as usize) {
                Some(n) => n,
                None => "?",
            }
        } else {
            "-"
        };
        format!(
            "[{}] 键={} 位#{} 辅={}",
            self.kind.text(),
            k,
            self.ord,
            self.aux
        )
    }
}

/// 事件账滑窗（cap 槽；满后滑出最旧并计 dropped——账不丢计数）。
#[derive(Clone, Debug)]
pub struct EventRing {
    /// 事件滑窗。
    pub evs: Vec<Event>,
    /// 滑出计数（wrapping）。
    pub dropped: u32,
    /// 容量。
    pub cap: usize,
}

impl EventRing {
    /// 新建（cap=EVENT_CAP）。
    pub fn new() -> EventRing {
        EventRing {
            evs: Vec::new(),
            dropped: 0,
            cap: EVENT_CAP,
        }
    }

    /// 入账（满则滑出最旧；remove(0) 仅在 len>=cap>=1 时调用，恒非空，
    /// 界内安全）。
    pub fn push(&mut self, e: Event) {
        if self.cap == 0 {
            self.dropped = self.dropped.wrapping_add(1);
            return;
        }
        if self.evs.len() >= self.cap {
            self.evs.remove(0);
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.evs.push(e);
    }

    /// 已存条数。
    pub fn len(&self) -> usize {
        self.evs.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.evs.is_empty()
    }

    /// 逐条渲染（中文行）。
    pub fn render_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.evs.len() {
            match self.evs.get(i) {
                Some(e) => out.push(e.line()),
                None => break,
            }
            i += 1;
        }
        out
    }

    /// 按类计数（面板观测用）。
    pub fn count_of(&self, kind: EventKind) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.evs.len() {
            if let Some(e) = self.evs.get(i) {
                if e.kind == kind {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 重复簿（键 → 重复值清单 + 每次出现位置 + 第几现 + 同异标注）
// ═══════════════════════════════════════════════════════════════════════

/// 单笔后现（重复簿条目；Copy）。
#[derive(Clone, Copy, Debug)]
pub struct DupOcc {
    /// 全局出现序号（出现位置）。
    pub ord: u32,
    /// 第几现（首现=1，后现从 2 起算）。
    pub nth: u32,
    /// 后现值。
    pub val: ValCell,
    /// 同异标注。
    pub class: DiffClass,
}

/// 单键重复记录（簿内按首笔重复出现的插入序排列）。
#[derive(Clone, Debug)]
pub struct DupRecord {
    /// 键位（0..24）。
    pub key_idx: u8,
    /// 该键的后现清单。
    pub occs: Vec<DupOcc>,
}

/// 重复簿可查面（owned 视图，免借用纠缠）。
#[derive(Clone, Debug)]
pub struct DupView {
    /// 键名。
    pub key: &'static str,
    /// 键位。
    pub key_idx: usize,
    /// 首现值。
    pub first_val: ValCell,
    /// 首现位置（全局序号）。
    pub first_ord: u32,
    /// 重复总数（后现笔数）。
    pub dup_total: u32,
    /// 同值列（字节同）。
    pub same_col: u32,
    /// 同值列（布尔词形异）。
    pub form_col: u32,
    /// 异值列。
    pub diff_col: u32,
    /// 后现清单（值+位置+第几现+标注）。
    pub occs: Vec<DupOcc>,
}

impl DupView {
    /// 该键总现数（= 1 首现 + dup_total）。
    pub fn total_seen(&self) -> u32 {
        self.dup_total.wrapping_add(1)
    }

    /// 后现值斜杠串（v2/v3 式；供人话句拼装）。
    pub fn later_values_joined(&self) -> String {
        let mut s = String::new();
        let mut i = 0;
        while i < self.occs.len() {
            if let Some(o) = self.occs.get(i) {
                if i > 0 {
                    s.push('/');
                }
                s.push_str(o.val.as_str());
            }
            i += 1;
        }
        s
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 未知键复现账（白名单外重复——双重违规分账的数据面）
// ═══════════════════════════════════════════════════════════════════════

/// 单笔未知键出现（键名/值/全局序号/该未知键第几现）。
#[derive(Clone, Copy, Debug)]
pub struct UnknownOcc {
    /// 未知键名（截 KEY_KEEP 保存）。
    pub key: ValCell,
    /// 出现值（截 VAL_MAX 保存）。
    pub val: ValCell,
    /// 全局出现序号。
    pub ord: u32,
    /// 该未知键第几现（1 起）。
    pub nth: u32,
}

impl UnknownOcc {
    /// 是否双重违规笔（第 2 现起：白名单外 + 重复）。
    pub fn is_double(&self) -> bool {
        self.nth >= 2
    }

    /// 人话行（「键 X 第 n 现（位 #o，值 v）——白名单外+重复」式）。
    pub fn line(&self) -> String {
        if self.is_double() {
            format!(
                "键 {} 第 {} 现（位 #{}，值 {}）——白名单外+重复（双重违规）",
                self.key.as_str(),
                self.nth,
                self.ord,
                self.val.as_str()
            )
        } else {
            format!(
                "键 {} 第 {} 现（位 #{}，值 {}）——白名单外",
                self.key.as_str(),
                self.nth,
                self.ord,
                self.val.as_str()
            )
        }
    }
}

/// 未知键复现账（cap 槽滑窗 + dropped 计数；nth 由同键扫描累计）。
#[derive(Clone, Debug)]
pub struct UnknownLedger {
    /// 出现滑窗。
    pub occs: Vec<UnknownOcc>,
    /// 滑出/截笔计数。
    pub dropped: u32,
    /// 容量。
    pub cap: usize,
}

impl UnknownLedger {
    /// 新建（cap=UNKNOWN_CAP）。
    pub fn new() -> UnknownLedger {
        UnknownLedger {
            occs: Vec::new(),
            dropped: 0,
            cap: UNKNOWN_CAP,
        }
    }

    /// 同键既有现数（nth 计算基；线性扫描，账面 ≤48 笔）。
    fn count_nth(&self, key: &ValCell) -> u32 {
        let mut n: u32 = 0;
        let mut i = 0;
        while i < self.occs.len() {
            if let Some(o) = self.occs.get(i) {
                if o.key.same_bytes(key) {
                    n = n.wrapping_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 入账（先算第几现再入窗；返回第几现；满则滑出最旧并计 dropped
    /// ——remove(0) 仅在 len>=cap>=1 时调用，恒非空界内安全）。
    fn record(&mut self, key: ValCell, val: ValCell, ord: u32) -> u32 {
        let nth = self.count_nth(&key).wrapping_add(1);
        if self.cap == 0 {
            self.dropped = self.dropped.wrapping_add(1);
            return nth;
        }
        if self.occs.len() >= self.cap {
            self.occs.remove(0);
            self.dropped = self.dropped.wrapping_add(1);
        }
        self.occs.push(UnknownOcc { key, val, ord, nth });
        nth
    }

    /// 已存笔数。
    pub fn len(&self) -> usize {
        self.occs.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.occs.is_empty()
    }

    /// 双重违规笔数（第 2 现起）。
    pub fn double_violation_n(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.occs.len() {
            if let Some(o) = self.occs.get(i) {
                if o.is_double() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 双重违规键清单（去重保序；人话报告用）。
    pub fn double_keys(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.occs.len() {
            if let Some(o) = self.occs.get(i) {
                if o.is_double() {
                    let name = o.key.as_str().to_string();
                    let mut seen = false;
                    let mut j = 0;
                    while j < out.len() {
                        if let Some(x) = out.get(j) {
                            if *x == name {
                                seen = true;
                                break;
                            }
                        }
                        j += 1;
                    }
                    if !seen {
                        out.push(name);
                    }
                }
            }
            i += 1;
        }
        out
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 自审三面（恒等式 / 白名单联动 / 重复统计）
// ═══════════════════════════════════════════════════════════════════════

/// 账本恒等式自审报告（I1~I6；全过才 pass）。
#[derive(Clone, Copy, Debug)]
pub struct IdentityReport {
    /// I1 总现 = 受理 + 重复。
    pub i1_seen_split: bool,
    /// I2 重复总数 = 逐键计数之和。
    pub i2_dup_sum: bool,
    /// I3 逐键重复 = 同值 + 词形异 + 异值。
    pub i3_class_split: bool,
    /// I4 重复总数 = 簿内存笔 + 簿截笔。
    pub i4_book_close: bool,
    /// I5 生效值 ≡ 影子首现值（双写点对拍）。
    pub i5_shadow_match: bool,
    /// I6 簿内键位全在闭集且生效在位。
    pub i6_book_keys_valid: bool,
    /// 总判。
    pub pass: bool,
}

impl IdentityReport {
    /// 逐条中文行。
    pub fn lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        v.push(format!("I1 总现=受理+重复：{}", ok_cn(self.i1_seen_split)));
        v.push(format!("I2 重复总数=逐键和：{}", ok_cn(self.i2_dup_sum)));
        v.push(format!(
            "I3 逐键重复=同值+词形异+异值：{}",
            ok_cn(self.i3_class_split)
        ));
        v.push(format!(
            "I4 重复总数=簿存笔+簿截笔：{}",
            ok_cn(self.i4_book_close)
        ));
        v.push(format!(
            "I5 生效值≡影子首现值：{}",
            ok_cn(self.i5_shadow_match)
        ));
        v.push(format!(
            "I6 簿内键位全在闭集：{}",
            ok_cn(self.i6_book_keys_valid)
        ));
        v
    }
}

/// 白名单联动审计报告。
#[derive(Clone, Copy, Debug)]
pub struct WhitelistAudit {
    /// 未知账键全部确在闭集外（反查 key_index 全 None）。
    pub unknown_out_of_table: bool,
    /// 重复簿键全部确在闭集内且生效在位。
    pub book_inside_table: bool,
    /// 双重违规笔数（白名单外 + 重复）。
    pub double_violation_n: u32,
    /// 总判。
    pub pass: bool,
}

impl WhitelistAudit {
    /// 逐条中文行。
    pub fn lines(&self) -> Vec<String> {
        let mut v = Vec::new();
        v.push(format!(
            "未知账键全在闭集外：{}",
            ok_cn(self.unknown_out_of_table)
        ));
        v.push(format!(
            "重复簿键全在闭集内：{}",
            ok_cn(self.book_inside_table)
        ));
        v.push(format!("双重违规笔数：{}", self.double_violation_n));
        v
    }
}

/// 重复统计面（两列分列 + 型分布 + 重复榜首 5）。
#[derive(Clone, Debug)]
pub struct DupStats {
    /// 有重复的键数。
    pub keys_with_dups: u32,
    /// 重复总数。
    pub total_dups: u64,
    /// 同值列合计（含布尔词形异）。
    pub same_col: u64,
    /// 其中布尔词形异。
    pub form_col: u64,
    /// 异值列合计。
    pub diff_col: u64,
    /// 布尔键重复笔数。
    pub bool_dups: u64,
    /// 整数键重复笔数。
    pub int_dups: u64,
    /// 字符串键重复笔数。
    pub str_dups: u64,
    /// 重复榜首（键位，笔数；按笔数降、键位升，取前 5）。
    pub top: Vec<(usize, u32)>,
}

/// 布尔中文「通过/破缺」。
fn ok_cn(v: bool) -> &'static str {
    if v {
        "通过"
    } else {
        "破缺"
    }
}

// ═══════════════════════════════════════════════════════════════════════
// FirstWinLedger —— 首现口径账主体
// ═══════════════════════════════════════════════════════════════════════

/// 首现口径账（同键多现→首现生效 + 重复计数 + 重复簿 + 未知键复现账；
/// 内联定长区 ≈ 4KB 上栈安全，簿/账/事件走 Vec 堆）。
#[derive(Clone, Debug)]
pub struct FirstWinLedger {
    /// 生效表（写点二：仅首现写；此后只读）。
    effective: [ValCell; KEY_N],
    /// 影子首现表（写点一：与生效表两处独立写点，自审对拍）。
    shadow_first: [ValCell; KEY_N],
    /// 生效在位掩码。
    present: [bool; KEY_N],
    /// 首现全局序号。
    first_ord: [u32; KEY_N],
    /// 每键重复计数（后现笔数）。
    dup_count: [u32; KEY_N],
    /// 每键同值列（字节同）。
    same_n: [u32; KEY_N],
    /// 每键词形异同值列。
    form_n: [u32; KEY_N],
    /// 每键异值列。
    diff_n: [u32; KEY_N],
    /// 重复簿（键序插入序）。
    book: Vec<DupRecord>,
    /// 簿内存笔数。
    book_occ_kept: u32,
    /// 簿截笔数（容量溢出；计数不丢）。
    book_occ_dropped: u32,
    /// 未知键复现账。
    unknown: UnknownLedger,
    /// 事件账。
    events: EventRing,
    /// 已知键总现数（首现 + 后现）。
    total_seen: u64,
    /// 受理首现总数。
    total_accepted: u64,
    /// 重复总数（后现笔数）。
    total_dups: u64,
    /// 未知键出现总数。
    unknown_n: u64,
    /// 下一全局序号（1 起编；0 为「未编序」哨兵；wrapping）。
    ord_next: u32,
}

impl FirstWinLedger {
    /// 新建空账。
    pub fn new() -> FirstWinLedger {
        FirstWinLedger {
            effective: [ValCell::empty(); KEY_N],
            shadow_first: [ValCell::empty(); KEY_N],
            present: [false; KEY_N],
            first_ord: [0; KEY_N],
            dup_count: [0; KEY_N],
            same_n: [0; KEY_N],
            form_n: [0; KEY_N],
            diff_n: [0; KEY_N],
            book: Vec::new(),
            book_occ_kept: 0,
            book_occ_dropped: 0,
            unknown: UnknownLedger::new(),
            events: EventRing::new(),
            total_seen: 0,
            total_accepted: 0,
            total_dups: 0,
            unknown_n: 0,
            ord_next: 1,
        }
    }

    // ── 序号与事件 ────────────────────────────────────────────────────

    /// 分配全局出现序号（wrapping；1 起编，0 为哨兵）。
    fn next_ord(&mut self) -> u32 {
        let o = self.ord_next;
        self.ord_next = self.ord_next.wrapping_add(1);
        o
    }

    /// 记事件（键位 None/越纲 → 哨兵 0xFF）。
    fn note(&mut self, kind: EventKind, key: Option<usize>, ord: u32, aux: u32) {
        let ki = match key {
            Some(i) if i < KEY_N => i as u8,
            _ => KEY_IDX_NONE,
        };
        self.events.push(Event {
            kind,
            key_idx: ki,
            ord,
            aux,
        });
    }

    // ── 双写点（首现结构性保证）──────────────────────────────────────

    /// 写点一：影子首现（仅首现调用；独立于写点二的代码路径）。
    fn shadow_note(&mut self, key_idx: usize, val: &ValCell) {
        if key_idx < KEY_N {
            self.shadow_first[key_idx] = *val;
        }
    }

    /// 写点二：生效表（仅首现调用；写后该键生效槽只读）。
    fn effective_write(&mut self, key_idx: usize, val: &ValCell, ord: u32) {
        if key_idx < KEY_N {
            self.effective[key_idx] = *val;
            self.present[key_idx] = true;
            self.first_ord[key_idx] = ord;
        }
    }

    // ── 已知键入账（首现/后现分叉）───────────────────────────────────

    /// 已知键入账（key_idx 须为 key_index 查得位；首现写生效表，后现
    /// 入重复簿并分类标注）。内部契约：键位越纲报 InternalBroken。
    pub fn accept_known(
        &mut self,
        key_idx: usize,
        val: &str,
    ) -> Result<AcceptOut, DupKey2Error> {
        if key_idx >= KEY_N {
            return Err(DupKey2Error::InternalBroken);
        }
        let cell = ValCell::from_val(val)?;
        let ord = self.next_ord();
        let dup_before = self.dup_count[key_idx];
        if !self.present[key_idx] {
            // 首现：双写点（影子 → 生效），此后该键只读。
            self.shadow_note(key_idx, &cell);
            self.effective_write(key_idx, &cell, ord);
            self.total_seen = self.total_seen.wrapping_add(1);
            self.total_accepted = self.total_accepted.wrapping_add(1);
            self.note(EventKind::StreamFirst, Some(key_idx), ord, 0);
            Ok(AcceptOut {
                class: AcceptClass::First,
                ord,
                seen: 1,
                dup_after: 0,
                dup_class: None,
                book_kept: true,
            })
        } else {
            // 后现：不入生效表；分类 → 计数 → 簿。
            let dc = self.classify(key_idx, &cell);
            let nth = dup_before.wrapping_add(2); // 首现=1，本笔=第 nth 现
            self.dup_count[key_idx] = dup_before.wrapping_add(1);
            match dc {
                DiffClass::SameValue => {
                    self.same_n[key_idx] = self.same_n[key_idx].wrapping_add(1)
                }
                DiffClass::BoolFormSame => {
                    self.form_n[key_idx] = self.form_n[key_idx].wrapping_add(1);
                    self.note(EventKind::BoolFormRepeat, Some(key_idx), ord, dc.tag());
                }
                DiffClass::DiffValue => {
                    self.diff_n[key_idx] = self.diff_n[key_idx].wrapping_add(1);
                    self.note(EventKind::DiffRepeat, Some(key_idx), ord, dc.tag());
                }
            }
            self.total_seen = self.total_seen.wrapping_add(1);
            self.total_dups = self.total_dups.wrapping_add(1);
            let kept = self.book_push(key_idx, DupOcc {
                ord,
                nth,
                val: cell,
                class: dc,
            });
            self.note(EventKind::StreamDup, Some(key_idx), ord, dc.tag());
            Ok(AcceptOut {
                class: AcceptClass::Dup,
                ord,
                seen: nth,
                dup_after: self.dup_count[key_idx],
                dup_class: Some(dc),
                book_kept: kept,
            })
        }
    }

    /// 后现差异分类（字节同 → 同值；布尔键同规范位 → 词形异同值；
    /// 否则异值）。
    fn classify(&self, key_idx: usize, val: &ValCell) -> DiffClass {
        if key_idx >= KEY_N {
            return DiffClass::DiffValue;
        }
        let eff = self.effective[key_idx];
        if eff.same_bytes(val) {
            return DiffClass::SameValue;
        }
        if kind_eq(KEY_TABLE[key_idx].kind, KeyKind::Bool) {
            let a = canonical_bool(eff.as_str());
            let b = canonical_bool(val.as_str());
            if let (Some(wa), Some(wb)) = (a, b) {
                if bool_word_eq(wa, wb) {
                    return DiffClass::BoolFormSame;
                }
            }
        }
        DiffClass::DiffValue
    }

    /// 重复簿入笔（容量满则截笔：计数已入账，簿丢笔入 dropped；
    /// 返回是否存簿）。
    fn book_push(&mut self, key_idx: usize, occ: DupOcc) -> bool {
        if self.book_occ_kept >= DUP_BOOK_CAP as u32 {
            self.book_occ_dropped = self.book_occ_dropped.wrapping_add(1);
            self.note(EventKind::BookOverflow, Some(key_idx), occ.ord, 0);
            return false;
        }
        let mut i = 0;
        let mut placed = false;
        while i < self.book.len() {
            if let Some(r) = self.book.get(i) {
                if (r.key_idx as usize) == key_idx {
                    // 已有该键记录：追加后现。
                    if let Some(r) = self.book.get_mut(i) {
                        r.occs.push(occ);
                        placed = true;
                    }
                    break;
                }
            }
            i += 1;
        }
        if !placed {
            let mut rec = DupRecord {
                key_idx: key_idx as u8,
                occs: Vec::new(),
            };
            rec.occs.push(occ);
            self.book.push(rec);
        }
        self.book_occ_kept = self.book_occ_kept.wrapping_add(1);
        true
    }

    // ── 未知键复现入账（白名单外——双重违规分账数据面）───────────────

    /// 未知键出现入账（键名截 KEY_KEEP、值截 VAL_MAX 保存；返回第几现）。
    pub fn record_unknown(&mut self, key: &str, val: &str) -> Result<u32, DupKey2Error> {
        let kcell = ValCell::from_key(key)?;
        let vcell = ValCell::from_val(val)?;
        let ord = self.next_ord();
        self.unknown_n = self.unknown_n.wrapping_add(1);
        let nth = self.unknown.record(kcell, vcell, ord);
        if nth >= 2 {
            self.note(EventKind::UnknownDouble, None, ord, nth);
        }
        self.note(EventKind::StreamUnknown, None, ord, nth);
        Ok(nth)
    }

    // ── 查询面（重复簿可查）──────────────────────────────────────────

    /// 键位查重复视图（键不在闭集报 UnknownKey；无重复记录报 QueryMiss）。
    pub fn dup_query(&self, key: &str) -> Result<DupView, DupKey2Error> {
        let idx = match key_index(key) {
            Some(i) => i,
            None => return Err(DupKey2Error::UnknownKey),
        };
        if idx >= KEY_N || !self.present[idx] {
            return Err(DupKey2Error::QueryMiss);
        }
        let mut occs = Vec::new();
        let mut i = 0;
        while i < self.book.len() {
            if let Some(r) = self.book.get(i) {
                if (r.key_idx as usize) == idx {
                    let mut j = 0;
                    while j < r.occs.len() {
                        if let Some(o) = r.occs.get(j) {
                            occs.push(*o);
                        }
                        j += 1;
                    }
                    break;
                }
            }
            i += 1;
        }
        if occs.is_empty() && self.dup_count[idx] == 0 {
            return Err(DupKey2Error::QueryMiss);
        }
        Ok(DupView {
            key: KEY_TABLE[idx].key,
            key_idx: idx,
            first_val: self.effective[idx],
            first_ord: self.first_ord[idx],
            dup_total: self.dup_count[idx],
            same_col: self.same_n[idx],
            form_col: self.form_n[idx],
            diff_col: self.diff_n[idx],
            occs,
        })
    }

    /// 键位查出现位置清单（首现位 + 后现位，按出现序）。
    pub fn positions_of(&self, key: &str) -> Result<Vec<u32>, DupKey2Error> {
        let v = self.dup_query(key)?;
        let mut out = Vec::new();
        out.push(v.first_ord);
        let mut i = 0;
        while i < v.occs.len() {
            if let Some(o) = v.occs.get(i) {
                out.push(o.ord);
            }
            i += 1;
        }
        Ok(out)
    }

    /// 键位查生效值（未生效报 QueryMiss）。
    pub fn effective_value(&self, key: &str) -> Result<ValCell, DupKey2Error> {
        let idx = match key_index(key) {
            Some(i) => i,
            None => return Err(DupKey2Error::UnknownKey),
        };
        if idx < KEY_N && self.present[idx] {
            Ok(self.effective[idx])
        } else {
            Err(DupKey2Error::QueryMiss)
        }
    }

    /// 生效键名清单（表序）。
    pub fn keys_effective(&self) -> Vec<&'static str> {
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

    /// 有重复的键名清单（表序）。
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

    /// 重复统计面（两列分列 + 型分布 + 榜首 5）。
    pub fn dup_stats(&self) -> DupStats {
        let mut st = DupStats {
            keys_with_dups: 0,
            total_dups: self.total_dups,
            same_col: 0,
            form_col: 0,
            diff_col: 0,
            bool_dups: 0,
            int_dups: 0,
            str_dups: 0,
            top: Vec::new(),
        };
        let mut i = 0;
        while i < KEY_N {
            let d = self.dup_count[i] as u64;
            if d > 0 {
                st.keys_with_dups += 1;
                st.same_col += self.same_n[i] as u64;
                st.form_col += self.form_n[i] as u64;
                st.diff_col += self.diff_n[i] as u64;
                match KEY_TABLE[i].kind {
                    KeyKind::Bool => st.bool_dups += d,
                    KeyKind::Int => st.int_dups += d,
                    KeyKind::Str => st.str_dups += d,
                }
                st.top.push((i, self.dup_count[i]));
            }
            i += 1;
        }
        // 榜首：笔数降、键位升（sort_by 稳定全序，确定性）。
        st.top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        st.top.truncate(5);
        st
    }

    /// 未知键复现账只读访问。
    pub fn unknown_occs(&self) -> &[UnknownOcc] {
        &self.unknown.occs
    }

    /// 事件账只读访问。
    pub fn events(&self) -> &EventRing {
        &self.events
    }

    /// 总现数（已知键）。
    pub fn total_seen(&self) -> u64 {
        self.total_seen
    }

    /// 受理首现总数。
    pub fn total_accepted(&self) -> u64 {
        self.total_accepted
    }

    /// 重复总数（后现笔数）。
    pub fn total_dups(&self) -> u64 {
        self.total_dups
    }

    /// 未知键出现总数（含截笔）。
    pub fn unknown_n(&self) -> u64 {
        self.unknown_n
    }

    // ── 自审面 ────────────────────────────────────────────────────────

    /// 账本恒等式自审（I1~I6；恒等式失败不报错而回报——诊断面）。
    pub fn identity_check(&self) -> IdentityReport {
        let mut rep = IdentityReport {
            i1_seen_split: false,
            i2_dup_sum: false,
            i3_class_split: false,
            i4_book_close: false,
            i5_shadow_match: false,
            i6_book_keys_valid: false,
            pass: false,
        };
        // I1：总现 = 受理 + 重复（wrapping 等价比较）。
        rep.i1_seen_split =
            self.total_seen == self.total_accepted.wrapping_add(self.total_dups);
        // I2：重复总数 = 逐键计数之和。
        let mut sum: u64 = 0;
        let mut i = 0;
        while i < KEY_N {
            sum = sum.wrapping_add(self.dup_count[i] as u64);
            i += 1;
        }
        rep.i2_dup_sum = sum == self.total_dups;
        // I3 + I5 + I6（单趟扫描）。
        let mut i3 = true;
        let mut i5 = true;
        let mut i6 = true;
        let mut i = 0;
        while i < KEY_N {
            let d = self.dup_count[i];
            let s = self
                .same_n[i]
                .saturating_add(self.form_n[i])
                .saturating_add(self.diff_n[i]);
            if d != s {
                i3 = false;
            }
            if self.present[i] && !self.effective[i].same_bytes(&self.shadow_first[i]) {
                i5 = false;
            }
            i += 1;
        }
        rep.i3_class_split = i3;
        rep.i5_shadow_match = i5;
        // I4：重复总数 = 簿存笔 + 簿截笔。
        rep.i4_book_close = self.total_dups
            == (self.book_occ_kept as u64).wrapping_add(self.book_occ_dropped as u64);
        // I6：簿内键位全在闭集且生效在位（present 为 true 意味着键位合法，
        // 双重校验防表漂移）。
        let mut bi = 0;
        while bi < self.book.len() {
            if let Some(r) = self.book.get(bi) {
                let ki = r.key_idx as usize;
                if ki >= KEY_N || !self.present[ki] {
                    i6 = false;
                    break;
                }
            }
            bi += 1;
        }
        rep.i6_book_keys_valid = i6;
        rep.pass = rep.i1_seen_split
            && rep.i2_dup_sum
            && rep.i3_class_split
            && rep.i4_book_close
            && rep.i5_shadow_match
            && rep.i6_book_keys_valid;
        rep
    }

    /// 恒等式硬门（自审失败报 LedgerBroken——上账前调用方把关用）。
    pub fn require_identity(&self) -> Result<(), DupKey2Error> {
        if self.identity_check().pass {
            Ok(())
        } else {
            Err(DupKey2Error::LedgerBroken)
        }
    }

    /// 白名单联动审计（未知账反查须全 None；簿内键位须全在位）。
    pub fn whitelist_audit(&self) -> WhitelistAudit {
        let mut uo = true;
        let mut i = 0;
        while i < self.unknown.occs.len() {
            if let Some(o) = self.unknown.occs.get(i) {
                if key_index(o.key.as_str()).is_some() {
                    uo = false;
                    break;
                }
            }
            i += 1;
        }
        let mut bi = true;
        let mut j = 0;
        while j < self.book.len() {
            if let Some(r) = self.book.get(j) {
                let ki = r.key_idx as usize;
                if ki >= KEY_N || !self.present[ki] {
                    bi = false;
                    break;
                }
            }
            j += 1;
        }
        let dv = self.unknown.double_violation_n();
        WhitelistAudit {
            unknown_out_of_table: uo,
            book_inside_table: bi,
            double_violation_n: dv,
            pass: uo && bi,
        }
    }

    /// 白名单联动硬门。
    pub fn require_whitelist(&self) -> Result<(), DupKey2Error> {
        if self.whitelist_audit().pass {
            Ok(())
        } else {
            Err(DupKey2Error::WhitelistBroken)
        }
    }

    /// 健康分（0~1000 定点；恒等式破缺直接 0——结构性失账不可信）：
    /// 同值重复每笔 -2；布尔词形异每笔 -1；异值重复每笔 -8；双重违规
    /// 每笔 -15；簿截笔每笔 -1（信息性）；saturating 到 0。
    pub fn health(&self) -> u32 {
        let id = self.identity_check();
        if !id.pass {
            return 0;
        }
        let mut pen: u64 = 0;
        let mut i = 0;
        while i < KEY_N {
            pen = pen
                .wrapping_add((self.same_n[i] as u64).wrapping_mul(2))
                .wrapping_add(self.form_n[i] as u64)
                .wrapping_add((self.diff_n[i] as u64).wrapping_mul(8));
            i += 1;
        }
        pen = pen.wrapping_add((self.unknown.double_violation_n() as u64).wrapping_mul(15));
        pen = pen.wrapping_add(self.book_occ_dropped as u64);
        if pen >= HEALTH_FULL as u64 {
            0
        } else {
            HEALTH_FULL - pen as u32
        }
    }

    // ── 指纹面 ────────────────────────────────────────────────────────

    /// 账面指纹（域前缀 FP_DOMAIN_STATE；覆盖生效表/计数/分列/簿/未知账；
    /// 不含事件滑窗——观测面非账面态；确定性：全序遍历无哈希迭代）。
    pub fn state_fingerprint(&self) -> u64 {
        let mut h = fnv64(FNV64_OFFSET, FP_DOMAIN_STATE);
        h = fnv64(h, DK2_VERSION.as_bytes());
        h = fnv64(h, FIRST_WIN_SEMANTICS.as_bytes());
        h = fnv64(h, &[(KEY_N as u8), (RING_CAP as u8)]);
        let mut i = 0;
        while i < KEY_N {
            // 生效值 + 在位 + 首现位 + 计数 + 三分列。
            h = self.effective[i].fold_fnv(h);
            h = fnv64(h, &[if self.present[i] { 1 } else { 0 }]);
            h = fnv64(h, &self.first_ord[i].to_le_bytes());
            h = fnv64(h, &self.dup_count[i].to_le_bytes());
            h = fnv64(h, &self.same_n[i].to_le_bytes());
            h = fnv64(h, &self.form_n[i].to_le_bytes());
            h = fnv64(h, &self.diff_n[i].to_le_bytes());
            i += 1;
        }
        h = fnv64(h, &self.total_seen.to_le_bytes());
        h = fnv64(h, &self.total_accepted.to_le_bytes());
        h = fnv64(h, &self.total_dups.to_le_bytes());
        h = fnv64(h, &self.unknown_n.to_le_bytes());
        h = fnv64(h, &self.book_occ_kept.to_le_bytes());
        h = fnv64(h, &self.book_occ_dropped.to_le_bytes());
        // 重复簿（键位 + 逐笔 序号/第几现/标注/值）。
        let mut bi = 0;
        while bi < self.book.len() {
            if let Some(r) = self.book.get(bi) {
                h = fnv64(h, &[r.key_idx]);
                let mut oi = 0;
                while oi < r.occs.len() {
                    if let Some(o) = r.occs.get(oi) {
                        h = fnv64(h, &o.ord.to_le_bytes());
                        h = fnv64(h, &o.nth.to_le_bytes());
                        h = fnv64(h, &[o.class.tag() as u8]);
                        h = o.val.fold_fnv(h);
                    }
                    oi += 1;
                }
            }
            bi += 1;
        }
        // 未知账（键 + 值 + 序号 + 第几现）。
        let mut ui = 0;
        while ui < self.unknown.occs.len() {
            if let Some(o) = self.unknown.occs.get(ui) {
                h = o.key.fold_fnv(h);
                h = o.val.fold_fnv(h);
                h = fnv64(h, &o.ord.to_le_bytes());
                h = fnv64(h, &o.nth.to_le_bytes());
            }
            ui += 1;
        }
        h
    }

    // ── 人话诊断面 ────────────────────────────────────────────────────

    /// 单键人话解释（判据句式：
    /// 「键 X 出现 N 次，生效值=首次值 v1，后现值 v2/v3 被忽略（分列；
    /// 首现位 #a，重复位 #b/#c）」；预算 EXPLAIN_BUDGET 截断护顶）。
    pub fn explain_dup(&self, key: &str) -> Result<String, DupKey2Error> {
        let v = self.dup_query(key)?;
        let mut s = String::new();
        s.push_str(&format!(
            "键 {} 出现 {} 次，生效值=首次值 {}",
            v.key,
            v.total_seen(),
            v.first_val.as_str()
        ));
        s.push_str("，后现值 ");
        s.push_str(&v.later_values_joined());
        s.push_str(" 被忽略");
        // 分列标注（同值列含词形异单列注记）。
        if v.diff_col > 0 || v.same_col > 0 || v.form_col > 0 {
            s.push_str(&format!(
                "（异值 {} 笔",
                v.diff_col.wrapping_add(v.same_col).wrapping_add(v.form_col)
            ));
            if v.form_col > 0 {
                s.push_str(&format!("，其中布尔词形异同值 {}", v.form_col));
            }
            s.push(')');
        }
        // 位置（首现位 + 重复位）。
        s.push_str(&format!("；首现位 #{}", v.first_ord));
        let mut i = 0;
        while i < v.occs.len() {
            if let Some(o) = v.occs.get(i) {
                s.push_str(&format!("/重复位 #{}", o.ord));
            }
            i += 1;
        }
        Ok(truncate_owned(s, EXPLAIN_BUDGET))
    }

    /// 人话总报告（五节：生效表 / 重复键逐键 / 两列分列 / 未知键双重
    /// 违规 / 账面；预算 REPORT_BUDGET 截断护顶——截断不丢账，明细走
    /// dup_query/explain_dup 逐键取）。
    pub fn render_report(&self) -> String {
        let mut s = String::new();
        // 节一：首现生效表。
        let eff = self.keys_effective();
        s.push_str(&format!("── 首现生效表（{}/{} 键生效）──\n", eff.len(), KEY_N));
        let mut i = 0;
        while i < eff.len() {
            if let Some(k) = eff.get(i) {
                if let Ok(v) = self.effective_value(k) {
                    let pos = match key_index(k) {
                        Some(ix) => self.first_ord[ix],
                        None => 0,
                    };
                    s.push_str(&format!("  {}={}（首现位 #{}）\n", k, v.as_str(), pos));
                }
            }
            i += 1;
        }
        // 节二：重复键逐键人话。
        let dks = self.dup_keys();
        s.push_str(&format!(
            "── 重复键人话（{} 键重复，总重复 {} 笔）──\n",
            dks.len(),
            self.total_dups
        ));
        let mut i = 0;
        while i < dks.len() {
            if let Some(k) = dks.get(i) {
                if let Ok(line) = self.explain_dup(k) {
                    s.push_str("  ");
                    s.push_str(&line);
                    s.push('\n');
                }
            }
            i += 1;
        }
        // 节三：两列分列。
        let st = self.dup_stats();
        s.push_str(&format!(
            "── 同值/异值分列 ──\n  同值列（含词形异）：{} 笔（其中布尔词形异 {} 笔）\n  异值列：{} 笔\n",
            st.same_col.wrapping_add(st.form_col),
            st.form_col,
            st.diff_col
        ));
        // 节四：未知键双重违规。
        let dvk = self.unknown.double_keys();
        s.push_str(&format!(
            "── 未知键双重违规（{} 笔，涉 {} 键）──\n",
            self.unknown.double_violation_n(),
            dvk.len()
        ));
        let mut ui = 0;
        while ui < self.unknown.occs.len() {
            if let Some(o) = self.unknown.occs.get(ui) {
                if o.is_double() {
                    s.push_str("  ");
                    s.push_str(&o.line());
                    s.push('\n');
                }
            }
            ui += 1;
        }
        // 节五：账面。
        let id = self.identity_check();
        let wl = self.whitelist_audit();
        s.push_str(&format!(
            "── 账面 ──\n  恒等式 {}；白名单联动 {}；健康分 {}/{}；账面指纹 {:#018x}\n",
            ok_cn(id.pass),
            ok_cn(wl.pass),
            self.health(),
            HEALTH_FULL,
            self.state_fingerprint()
        ));
        truncate_owned(s, REPORT_BUDGET)
    }
}

/// 受理分类（首现/后现）。
#[derive(Clone, Copy, Debug)]
pub enum AcceptClass {
    /// 首现（入生效表）。
    First,
    /// 后现（入重复簿）。
    Dup,
}

impl AcceptClass {
    /// 中文短名。
    pub fn text(self) -> &'static str {
        match self {
            AcceptClass::First => "首现",
            AcceptClass::Dup => "后现",
        }
    }

    /// 标签位（对拍用：0 首现 / 1 后现）。
    pub fn tag(self) -> u32 {
        match self {
            AcceptClass::First => 0,
            AcceptClass::Dup => 1,
        }
    }
}

/// 已知键入账结果（推入方观测面）。
#[derive(Clone, Copy, Debug)]
pub struct AcceptOut {
    /// 受理分类。
    pub class: AcceptClass,
    /// 本笔全局序号。
    pub ord: u32,
    /// 该键第几现。
    pub seen: u32,
    /// 本笔后该键重复计数。
    pub dup_after: u32,
    /// 后现差异标注（首现为 None）。
    pub dup_class: Option<DiffClass>,
    /// 本笔是否存簿（容量满截笔为 false，计数不丢）。
    pub book_kept: bool,
}

// ═══════════════════════════════════════════════════════════════════════
// ParamStream —— 参数流（定长环形 64 槽 · 六闸 · 封流 · 快照）
// ═══════════════════════════════════════════════════════════════════════

/// 环形槽（一笔已受理 (键,值)：键位 + 原文 + 序号 + 受理分类 + 标注）。
#[derive(Clone, Copy, Debug)]
pub struct StreamSlot {
    /// 全局序号。
    pub ord: u32,
    /// 键位。
    pub key_idx: u8,
    /// 键名（截 KEY_KEEP）。
    pub key: ValCell,
    /// 值（截 VAL_MAX）。
    pub val: ValCell,
    /// 受理分类。
    pub accept: AcceptClass,
    /// 后现差异标注（首现为 None）。
    pub dup_class: Option<DiffClass>,
}

impl StreamSlot {
    /// 渲染行（「#ord 键=值（首现/后现[标注]）」）。
    pub fn line(&self) -> String {
        let cls = match self.dup_class {
            Some(c) => format!("{}·{}", self.accept.text(), c.text()),
            None => self.accept.text().to_string(),
        };
        format!(
            "#{} {}={}（{}）",
            self.ord,
            self.key.as_str(),
            self.val.as_str(),
            cls
        )
    }
}

/// 推入结果（Ok 面；Err 面走 DupKey2Error）。
#[derive(Clone, Copy, Debug)]
pub struct PushOutcome {
    /// 本笔全局序号。
    pub ord: u32,
    /// 键位。
    pub key_idx: usize,
    /// 受理分类。
    pub class: AcceptClass,
    /// 该键第几现。
    pub seen: u32,
    /// 本笔后该键重复计数。
    pub dup_after: u32,
    /// 后现差异标注。
    pub dup_class: Option<DiffClass>,
    /// 是否存簿。
    pub book_kept: bool,
}

/// 参数流（定长环形 + 首现口径账组合；判据口径：流处理完成后
/// （seal）才可取快照/跑首现断言面）。
#[derive(Clone, Debug)]
pub struct ParamStream {
    /// 首现口径账（pub：直查簿/未知账/事件/自审面）。
    pub ledger: FirstWinLedger,
    /// 环形（已受理 (键,值) 流；重复现也占槽——流记「到达」不记「生效」）。
    ring: Vec<StreamSlot>,
    /// 封流位。
    sealed: bool,
    /// 推入尝试总数（含拒收与未知）。
    push_attempts: u64,
    /// 受理总数。
    admitted: u64,
    /// 闸拒总数（空键/键长/值长/环满/封后写）。
    rejected: u64,
    /// 未知键尝试总数。
    unknown_attempts: u64,
}

impl ParamStream {
    /// 新建空流。
    pub fn new() -> ParamStream {
        ParamStream {
            ledger: FirstWinLedger::new(),
            ring: Vec::new(),
            sealed: false,
            push_attempts: 0,
            admitted: 0,
            rejected: 0,
            unknown_attempts: 0,
        }
    }

    /// 是否已封流。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 环形占槽数。
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// 环形是否空。
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    /// 环形容量。
    pub fn capacity(&self) -> usize {
        RING_CAP
    }

    /// 推入尝试总数。
    pub fn push_attempts(&self) -> u64 {
        self.push_attempts
    }

    /// 受理总数。
    pub fn admitted(&self) -> u64 {
        self.admitted
    }

    /// 闸拒总数。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// 未知键尝试总数。
    pub fn unknown_attempts(&self) -> u64 {
        self.unknown_attempts
    }

    /// 流账恒等式（尝试 = 受理 + 闸拒 + 未知）。
    pub fn stream_identity(&self) -> bool {
        self.push_attempts
            == self
                .admitted
                .wrapping_add(self.rejected)
                .wrapping_add(self.unknown_attempts)
    }

    /// 推入一笔（键,值）。六闸顺序（判据与预算链）：
    /// ①封流后写 ②空键 ③键超长 ④值超长 ⑤白名单外（未知键——
    /// 拒收但复现入未知账，双重违规分账）⑥环形容量满。
    /// 未知键不占环形槽（白名单层先行拒收）。
    pub fn push(&mut self, key: &str, val: &str) -> Result<PushOutcome, DupKey2Error> {
        self.push_attempts = self.push_attempts.wrapping_add(1);
        // 闸① 封流后写。
        if self.sealed {
            self.rejected = self.rejected.wrapping_add(1);
            self.ledger
                .note_pub(EventKind::RingReject, None, 0, 5);
            return Err(DupKey2Error::StreamSealed);
        }
        // 闸② 空键。
        if key.is_empty() {
            self.rejected = self.rejected.wrapping_add(1);
            self.ledger.note_pub(EventKind::RingReject, None, 0, 1);
            return Err(DupKey2Error::EmptyKey);
        }
        // 闸③ 键超长。
        if key.len() > KEY_KEEP {
            self.rejected = self.rejected.wrapping_add(1);
            self.ledger.note_pub(EventKind::RingReject, None, 0, 2);
            return Err(DupKey2Error::KeyTooLong);
        }
        // 闸④ 值超长。
        if val.len() > VAL_MAX {
            self.rejected = self.rejected.wrapping_add(1);
            self.ledger.note_pub(EventKind::RingReject, None, 0, 3);
            return Err(DupKey2Error::ValueTooLong);
        }
        // 闸⑤ 白名单外（未知键——复现入账后拒收）。
        let idx = match key_index(key) {
            Some(i) => i,
            None => {
                self.unknown_attempts = self.unknown_attempts.wrapping_add(1);
                // 键长/值长已在闸③④守住，此处入账不失败；结果仅作观测。
                let _nth = self.ledger.record_unknown(key, val)?;
                return Err(DupKey2Error::UnknownKey);
            }
        };
        // 闸⑥ 环形容量。
        if self.ring.len() >= RING_CAP {
            self.rejected = self.rejected.wrapping_add(1);
            self.ledger.note_pub(EventKind::RingReject, Some(idx), 0, 4);
            return Err(DupKey2Error::RingFull);
        }
        // 受理：先铸单元（界内必成，防御性走 Result），再入账占槽。
        let kcell = match ValCell::from_key(key) {
            Ok(c) => c,
            Err(e) => {
                self.rejected = self.rejected.wrapping_add(1);
                return Err(e);
            }
        };
        // 入账失败（理论不可达——键位来自查表、值长已过闸④）计入闸拒，
        // 保流账恒等式「尝试=受理+闸拒+未知」闭合。
        let out = match self.ledger.accept_known(idx, val) {
            Ok(o) => o,
            Err(e) => {
                self.rejected = self.rejected.wrapping_add(1);
                return Err(e);
            }
        };
        self.admitted = self.admitted.wrapping_add(1);
        self.ring.push(StreamSlot {
            ord: out.ord,
            key_idx: idx as u8,
            key: kcell,
            val: ValCell::empty(),
            accept: out.class,
            dup_class: out.dup_class,
        });
        // 回填值单元（占槽后独立回填，避免临时借用）。
        if let Some(slot) = self.ring.last_mut() {
            if let Ok(v) = ValCell::from_val(val) {
                slot.val = v;
            }
        }
        Ok(PushOutcome {
            ord: out.ord,
            key_idx: idx,
            class: out.class,
            seen: out.seen,
            dup_after: out.dup_after,
            dup_class: out.dup_class,
            book_kept: out.book_kept,
        })
    }

    /// 批量推入（逐笔走六闸；遇 Err 即停——调用方可检视账面后自决）。
    pub fn push_all(&mut self, batch: &[(&str, &str)]) -> Result<(), DupKey2Error> {
        let mut i = 0;
        while i < batch.len() {
            let (k, v) = match batch.get(i) {
                Some((k, v)) => (*k, *v),
                None => break,
            };
            self.push(k, v)?;
            i += 1;
        }
        Ok(())
    }

    /// 封流（重复封流报 StreamSealed——封账幂等禁止）。
    pub fn seal(&mut self) -> Result<(), DupKey2Error> {
        if self.sealed {
            self.ledger.note_pub(EventKind::RingReject, None, 0, 5);
            return Err(DupKey2Error::StreamSealed);
        }
        self.sealed = true;
        self.ledger.note_pub(EventKind::StreamSeal, None, 0, 0);
        Ok(())
    }

    /// 环形只读访问。
    pub fn ring_slots(&self) -> &[StreamSlot] {
        &self.ring
    }

    /// 参数流摘要（环形逐槽 + 流账四计数；预算 STREAM_SUM_BUDGET）。
    pub fn render_summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "参数流（{}/{} 槽，{}）：尝试 {} 受理 {} 闸拒 {} 未知 {}\n",
            self.ring.len(),
            RING_CAP,
            if self.sealed { "已封流" } else { "流开" },
            self.push_attempts,
            self.admitted,
            self.rejected,
            self.unknown_attempts
        ));
        let mut i = 0;
        while i < self.ring.len() {
            if let Some(sl) = self.ring.get(i) {
                s.push_str("  ");
                s.push_str(&sl.line());
                s.push('\n');
            }
            i += 1;
        }
        truncate_owned(s, STREAM_SUM_BUDGET)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// FirstWinSnapshot —— 首现快照（封流后生效表封账副本 + 指纹 + 计数面）
// ═══════════════════════════════════════════════════════════════════════

/// 首现快照（封账副本，整体 Copy ≈ 2KB 上栈安全；指纹域分离）。
#[derive(Clone, Copy, Debug)]
pub struct FirstWinSnapshot {
    /// 生效表副本。
    pub effective: [ValCell; KEY_N],
    /// 在位掩码副本。
    pub present: [bool; KEY_N],
    /// 每键重复计数副本。
    pub dup_count: [u32; KEY_N],
    /// 每键同值列副本。
    pub same_n: [u32; KEY_N],
    /// 每键词形异列副本。
    pub form_n: [u32; KEY_N],
    /// 每键异值列副本。
    pub diff_n: [u32; KEY_N],
    /// 受理首现总数副本。
    pub total_accepted: u64,
    /// 重复总数副本。
    pub total_dups: u64,
    /// 序号水位（封流时刻 ord_next——已编序笔数的 wrapping 口径）。
    pub ord_watermark: u32,
    /// 流尝试总数副本。
    pub push_attempts: u64,
    /// 未知键尝试总数副本。
    pub unknown_attempts: u64,
    /// 快照指纹（构造时以 refingerprint 铸入）。
    pub fingerprint: u64,
}

impl FirstWinSnapshot {
    /// 从参数流铸快照（ParamStream::snapshot 的铸体；要求已封流）。
    fn from_stream(s: &ParamStream) -> Result<FirstWinSnapshot, DupKey2Error> {
        if !s.sealed {
            return Err(DupKey2Error::SnapshotAbsent);
        }
        let l = &s.ledger;
        let mut snap = FirstWinSnapshot {
            effective: l.effective,
            present: l.present,
            dup_count: l.dup_count,
            same_n: l.same_n,
            form_n: l.form_n,
            diff_n: l.diff_n,
            total_accepted: l.total_accepted,
            total_dups: l.total_dups,
            ord_watermark: l.ord_next,
            push_attempts: s.push_attempts,
            unknown_attempts: s.unknown_attempts,
            fingerprint: 0,
        };
        snap.fingerprint = snap.refingerprint();
        Ok(snap)
    }

    /// 复算指纹（域前缀 FP_DOMAIN_SNAP + 逐域折叠；封账后被写则与
    /// 构造指纹不齐——fp_ok 检出）。
    pub fn refingerprint(&self) -> u64 {
        let mut h = fnv64(FNV64_OFFSET, FP_DOMAIN_SNAP);
        h = fnv64(h, DK2_VERSION.as_bytes());
        h = fnv64(h, FIRST_WIN_SEMANTICS.as_bytes());
        let mut i = 0;
        while i < KEY_N {
            h = self.effective[i].fold_fnv(h);
            h = fnv64(h, &[if self.present[i] { 1 } else { 0 }]);
            h = fnv64(h, &self.dup_count[i].to_le_bytes());
            h = fnv64(h, &self.same_n[i].to_le_bytes());
            h = fnv64(h, &self.form_n[i].to_le_bytes());
            h = fnv64(h, &self.diff_n[i].to_le_bytes());
            i += 1;
        }
        h = fnv64(h, &self.total_accepted.to_le_bytes());
        h = fnv64(h, &self.total_dups.to_le_bytes());
        h = fnv64(h, &self.ord_watermark.to_le_bytes());
        h = fnv64(h, &self.push_attempts.to_le_bytes());
        h = fnv64(h, &self.unknown_attempts.to_le_bytes());
        h
    }

    /// 指纹自检（封账副本完整性）。
    pub fn fp_ok(&self) -> bool {
        self.fingerprint == self.refingerprint()
    }

    /// 指纹硬门。
    pub fn require_fp(&self) -> Result<(), DupKey2Error> {
        if self.fp_ok() {
            Ok(())
        } else {
            Err(DupKey2Error::FpBroken)
        }
    }

    /// 生效值查询（未生效/闭集外报 QueryMiss/UnknownKey）。
    pub fn effective_value(&self, key: &str) -> Result<ValCell, DupKey2Error> {
        let idx = match key_index(key) {
            Some(i) => i,
            None => return Err(DupKey2Error::UnknownKey),
        };
        if idx < KEY_N && self.present[idx] {
            Ok(self.effective[idx])
        } else {
            Err(DupKey2Error::QueryMiss)
        }
    }

    /// 计数面（键 → 在位/生效命中/重复计数/总现；「首现生效 1/1 +
    /// 重复计数」两断言的数据面）。
    pub fn count_face(&self, key: &str) -> Result<CountFace, DupKey2Error> {
        let idx = match key_index(key) {
            Some(i) => i,
            None => return Err(DupKey2Error::UnknownKey),
        };
        if idx >= KEY_N {
            return Err(DupKey2Error::InternalBroken);
        }
        let present = self.present[idx];
        let dup = self.dup_count[idx];
        Ok(CountFace {
            key: KEY_TABLE[idx].key,
            key_idx: idx,
            present,
            effective_hits: if present { 1 } else { 0 },
            dup_count: dup,
            seen: if present { dup.wrapping_add(1) } else { 0 },
        })
    }

    /// 快照逐键转储行（生效键 + 重复计数；封账观测面）。
    pub fn dump_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < KEY_N {
            if self.present[i] {
                out.push(format!(
                    "{}={}（重复 {} 笔：同值 {} 词形异 {} 异值 {}）",
                    KEY_TABLE[i].key,
                    self.effective[i].as_str(),
                    self.dup_count[i],
                    self.same_n[i],
                    self.form_n[i],
                    self.diff_n[i]
                ));
            }
            i += 1;
        }
        out
    }
}

impl ParamStream {
    /// 取首现快照（判据口径：流处理完成后——未封流报 SnapshotAbsent；
    /// 事件账记 SnapTake）。
    pub fn snapshot(&mut self) -> Result<FirstWinSnapshot, DupKey2Error> {
        let snap = FirstWinSnapshot::from_stream(self)?;
        self.ledger
            .note_pub(EventKind::SnapTake, None, snap.ord_watermark, 0);
        Ok(snap)
    }

    /// 首现断言面（注入同键 3 现自证）：封流后在指定键上验证
    /// 「首现生效 1/1 + 重复计数 2/2」，并随证带走首现值与两笔后现值。
    pub fn assert_triple(&mut self, key: &str) -> Result<TripleProof, DupKey2Error> {
        if !self.sealed {
            return Err(DupKey2Error::SnapshotAbsent);
        }
        let idx = match key_index(key) {
            Some(i) => i,
            None => return Err(DupKey2Error::UnknownKey),
        };
        if idx >= KEY_N {
            return Err(DupKey2Error::InternalBroken);
        }
        let present = self.ledger.present[idx];
        let dup = self.ledger.dup_count[idx];
        // 三现口径：总现恰 3（首现 1 + 重复 2）。
        if dup != 2 || !present {
            self.ledger
                .note_pub(EventKind::TripleProofEv, Some(idx), 0, 1);
            if !present {
                return Err(DupKey2Error::FirstWinBroken);
            }
            return Err(DupKey2Error::CountBroken);
        }
        // 随证两笔后现值（簿截笔时 best-effort，缺失记 None 不伪造）。
        let mut laters: [Option<ValCell>; 2] = [None, None];
        let mut bi = 0;
        let mut taken = 0;
        while bi < self.ledger.book.len() && taken < 2 {
            if let Some(r) = self.ledger.book.get(bi) {
                if (r.key_idx as usize) == idx {
                    let mut oi = 0;
                    while oi < r.occs.len() && taken < 2 {
                        if let Some(o) = r.occs.get(oi) {
                            if taken < 2 {
                                laters[taken] = Some(o.val);
                                taken += 1;
                            }
                        }
                        oi += 1;
                    }
                    break;
                }
            }
            bi += 1;
        }
        let proof = TripleProof {
            key: KEY_TABLE[idx].key,
            key_idx: idx,
            first_val: self.ledger.effective[idx],
            later_vals: laters,
            effective_hits: 1,
            effective_expected: 1,
            dup_total: dup,
            dup_expected: 2,
            seen: 3,
        };
        self.ledger
            .note_pub(EventKind::TripleProofEv, Some(idx), proof.first_val.len() as u32, 0);
        if !proof.verify() {
            // 结构上不可达（present&&dup==2 已验）；防御性回 FirstWinBroken。
            return Err(DupKey2Error::FirstWinBroken);
        }
        Ok(proof)
    }
}

/// 计数面（键级「生效命中/重复计数/总现」数据面）。
#[derive(Clone, Copy, Debug)]
pub struct CountFace {
    /// 键名。
    pub key: &'static str,
    /// 键位。
    pub key_idx: usize,
    /// 是否在位。
    pub present: bool,
    /// 生效命中数（首现口径恒 ≤1）。
    pub effective_hits: u32,
    /// 重复计数。
    pub dup_count: u32,
    /// 总现数（在位时 =1+重复）。
    pub seen: u32,
}

/// 首现断言面随证（注入同键 3 现自证的物证包）。
#[derive(Clone, Copy, Debug)]
pub struct TripleProof {
    /// 键名。
    pub key: &'static str,
    /// 键位。
    pub key_idx: usize,
    /// 首现值（= 生效值）。
    pub first_val: ValCell,
    /// 两笔后现值（簿截笔时可能缺失——None 不伪造）。
    pub later_vals: [Option<ValCell>; 2],
    /// 生效命中实得（1/1 的分子）。
    pub effective_hits: u32,
    /// 生效命中期望（1/1 的分母）。
    pub effective_expected: u32,
    /// 重复计数实得（2/2 的分子）。
    pub dup_total: u32,
    /// 重复计数期望（2/2 的分母）。
    pub dup_expected: u32,
    /// 总现数。
    pub seen: u32,
}

impl TripleProof {
    /// 断言判定（生效 hits/expected == 1/1 且重复 total/expected == 2/2
    /// 且总现 == 3 且首现值在）。
    pub fn verify(&self) -> bool {
        self.effective_hits == self.effective_expected
            && self.effective_expected == 1
            && self.dup_total == self.dup_expected
            && self.dup_expected == 2
            && self.seen == 3
            && !self.first_val.is_empty()
    }

    /// 人话行（「键 X：首现生效 1/1，重复计数 2/2，生效值=首次值 v1，
    /// 后现 v2/v3 被忽略」）。
    pub fn line(&self) -> String {
        let mut laters = String::new();
        let mut i = 0;
        while i < 2 {
            if let Some(Some(v)) = self.later_vals.get(i) {
                if i > 0 {
                    laters.push('/');
                }
                laters.push_str(v.as_str());
            }
            i += 1;
        }
        format!(
            "键 {}：首现生效 {}/{}，重复计数 {}/{}，生效值=首次值 {}，后现 {} 被忽略",
            self.key,
            self.effective_hits,
            self.effective_expected,
            self.dup_total,
            self.dup_expected,
            self.first_val.as_str(),
            laters
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 事件记入公开面（ParamStream 六闸/封流/快照/断言用；内部 note 的
// 模块内可见性补口）
// ═══════════════════════════════════════════════════════════════════════

impl FirstWinLedger {
    /// 记事件（公开面：ParamStream 六闸/封流/快照/断言面入账）。
    pub fn note_pub(&mut self, kind: EventKind, key: Option<usize>, ord: u32, aux: u32) {
        self.note(kind, key, ord, aux);
    }

    /// 自审入账版（identity_check + IdentityEv 事件；面板/巡检用）。
    pub fn audit_now(&mut self) -> IdentityReport {
        let rep = self.identity_check();
        self.note(
            EventKind::IdentityEv,
            None,
            0,
            if rep.pass { 1 } else { 0 },
        );
        rep
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 双跑确定性对拍（同批次两跑：推入结果逐笔全等 + 双域指纹全等）
// ═══════════════════════════════════════════════════════════════════════

/// 双跑对拍报告。
#[derive(Clone, Copy, Debug)]
pub struct DualRunReport {
    /// 批大小。
    pub pushes: u32,
    /// 推入结果逐笔全等。
    pub outcomes_equal: bool,
    /// 账面指纹全等。
    pub state_fp_equal: bool,
    /// 快照指纹全等。
    pub snap_fp_equal: bool,
    /// 首笔发散位（None=全程全等）。
    pub first_divergence: Option<usize>,
    /// 甲跑账面指纹。
    pub state_fp_a: u64,
    /// 乙跑账面指纹。
    pub state_fp_b: u64,
    /// 甲跑快照指纹。
    pub snap_fp_a: u64,
    /// 乙跑快照指纹。
    pub snap_fp_b: u64,
    /// 总判。
    pub pass: bool,
}

/// 单跑标签（推入结果 → 五元组：受理位/分类位/第几现/重复后计数/
/// 标注位；Err 折叠：未知键 2、他错 3）。
fn outcome_tag(r: &Result<PushOutcome, DupKey2Error>) -> [u32; 5] {
    match r {
        Ok(o) => [
            1,
            o.class.tag(),
            o.seen,
            o.dup_after,
            match o.dup_class {
                Some(c) => c.tag(),
                None => 0,
            },
        ],
        Err(e) => {
            let cls = if e.code() == DupKey2Error::UnknownKey.code() {
                2
            } else {
                3
            };
            [0, cls, 0, 0, 0]
        }
    }
}

/// 双跑执行体（私有）：跑批 + 封流 + 快照，返回（标签序列, 流）。
fn dual_run_once(
    batch: &[(&str, &str)],
) -> (Vec<[u32; 5]>, ParamStream, Option<FirstWinSnapshot>) {
    let mut s = ParamStream::new();
    let mut tags = Vec::new();
    let mut i = 0;
    while i < batch.len() {
        let (k, v) = match batch.get(i) {
            Some((k, v)) => (*k, *v),
            None => break,
        };
        let r = s.push(k, v);
        tags.push(outcome_tag(&r));
        i += 1;
    }
    // 封流与快照失败（理论不可达——批内不重复 seal）折叠为 None。
    let snap = match s.seal() {
        Ok(()) => match s.snapshot() {
            Ok(sp) => Some(sp),
            Err(_) => None,
        },
        Err(_) => None,
    };
    (tags, s, snap)
}

/// 双跑确定性对拍（同批次两跑：标签逐笔全等 + 账面/快照双域指纹全等；
/// 任一发散记 first_divergence；快照缺失视为指纹不齐）。
pub fn dual_run(batch: &[(&str, &str)]) -> Result<DualRunReport, DupKey2Error> {
    let (tags_a, mut sa, snap_a) = dual_run_once(batch);
    let (tags_b, sb, snap_b) = dual_run_once(batch);
    let mut outcomes_equal = tags_a.len() == tags_b.len();
    let mut first_div: Option<usize> = None;
    if outcomes_equal {
        let mut i = 0;
        while i < tags_a.len() {
            let ta = match tags_a.get(i) {
                Some(t) => *t,
                None => break,
            };
            let tb = match tags_b.get(i) {
                Some(t) => *t,
                None => break,
            };
            let mut j = 0;
            let mut eq = true;
            while j < 5 {
                let x = match ta.get(j) {
                    Some(v) => *v,
                    None => 0,
                };
                let y = match tb.get(j) {
                    Some(v) => *v,
                    None => 1,
                };
                if x != y {
                    eq = false;
                    break;
                }
                j += 1;
            }
            if !eq {
                outcomes_equal = false;
                first_div = Some(i);
                break;
            }
            i += 1;
        }
    }
    let fp_a = sa.ledger.state_fingerprint();
    let fp_b = sb.ledger.state_fingerprint();
    let sfp_a = match &snap_a {
        Some(sp) => sp.fingerprint,
        None => 0,
    };
    let sfp_b = match &snap_b {
        Some(sp) => sp.fingerprint,
        None => 1,
    };
    let state_fp_equal = fp_a == fp_b;
    let snap_fp_equal = snap_a.is_some() && snap_b.is_some() && sfp_a == sfp_b;
    let pass = outcomes_equal && state_fp_equal && snap_fp_equal;
    sa.ledger.note_pub(
        EventKind::DualRunEv,
        None,
        batch.len() as u32,
        if pass { 1 } else { 0 },
    );
    Ok(DualRunReport {
        pushes: batch.len() as u32,
        outcomes_equal,
        state_fp_equal,
        snap_fp_equal,
        first_divergence: first_div,
        state_fp_a: fp_a,
        state_fp_b: fp_b,
        snap_fp_a: sfp_a,
        snap_fp_b: sfp_b,
        pass,
    })
}

/// 双跑硬门（不齐报 DualMismatch）。
pub fn dual_run_require(batch: &[(&str, &str)]) -> Result<(), DupKey2Error> {
    let rep = dual_run(batch)?;
    if rep.pass {
        Ok(())
    } else {
        Err(DupKey2Error::DualMismatch)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 注入面（三注入各检出：同键三现自证 / 异值重复 / 未知键重复）
// ═══════════════════════════════════════════════════════════════════════

/// 注入面报告（3 面；detected=全过数）。
#[derive(Clone, Copy, Debug)]
pub struct InjectionReport {
    /// 面 1 同键三现自证（生效 1/1 + 计数 2/2 + 快照指纹自检）。
    pub triple: bool,
    /// 面 2 异值重复注入（分列检出：异值列恰 1、同值列 0）。
    pub diff_repeat: bool,
    /// 面 3 未知键重复注入（3 现全拒 + 双重违规分账 + 白名单联动过）。
    pub unknown_repeat: bool,
    /// 检出面数（满 3）。
    pub detected: u32,
}

/// 注入面（判据 J1 的自证执行体）：三注入各检出 = 3/3。
pub fn injection_face() -> Result<InjectionReport, DupKey2Error> {
    let mut rep = InjectionReport {
        triple: false,
        diff_repeat: false,
        unknown_repeat: false,
        detected: 0,
    };
    // ── 面 1：同键三现注入自证（首现生效 1/1 + 重复计数 2/2）──
    let mut s1 = ParamStream::new();
    let r1a = s1.push("sched.slice.us", "4000");
    let r1b = s1.push("sched.slice.us", "5000");
    let r1c = s1.push("sched.slice.us", "6000");
    s1.seal()?;
    let snap = s1.snapshot()?;
    let proof = s1.assert_triple("sched.slice.us")?;
    let cf = snap.count_face("sched.slice.us")?;
    let triple = r1a.is_ok()
        && r1b.is_ok()
        && r1c.is_ok()
        && proof.verify()
        && snap.fp_ok()
        && cf.present
        && cf.effective_hits == 1
        && cf.dup_count == 2
        && cf.seen == 3
        && snap.effective_value("sched.slice.us")?.as_str() == "4000";
    // ── 面 2：异值重复注入（分列检出）──
    let mut s2 = ParamStream::new();
    let r2a = s2.push("hostname", "varix-a");
    let r2b = s2.push("hostname", "node-b");
    let diff_repeat = match (r2a, r2b) {
        (Ok(a), Ok(b)) => {
            a.class.tag() == 0
                && b.class.tag() == 1
                && match b.dup_class {
                    Some(DiffClass::DiffValue) => true,
                    _ => false,
                }
                && s2.ledger.dup_stats().diff_col == 1
                && s2.ledger.dup_stats().same_col == 0
        }
        _ => false,
    };
    // ── 面 3：未知键重复注入（3 现全拒 + 双重违规分账）──
    let mut s3 = ParamStream::new();
    let e1 = s3.push("no.such.key", "1");
    let e2 = s3.push("no.such.key", "2");
    let e3 = s3.push("no.such.key", "3");
    let all_unknown = match (&e1, &e2, &e3) {
        (Err(a), Err(b), Err(c)) => {
            a.code() == DupKey2Error::UnknownKey.code()
                && b.code() == DupKey2Error::UnknownKey.code()
                && c.code() == DupKey2Error::UnknownKey.code()
        }
        _ => false,
    };
    let occs = s3.ledger.unknown_occs();
    let nth_ok = occs.len() == 3
        && match (occs.first(), occs.get(1), occs.get(2)) {
            (Some(a), Some(b), Some(c)) => a.nth == 1 && b.nth == 2 && c.nth == 3,
            _ => false,
        };
    let dv = s3.ledger.unknown.double_violation_n();
    let audit = s3.ledger.whitelist_audit();
    let unknown_repeat =
        all_unknown && nth_ok && dv == 2 && audit.pass && audit.double_violation_n == 2;
    // 汇账（甲流记注入面事件）。
    s1.ledger.note_pub(
        EventKind::InjectEv,
        None,
        rep_detected(triple, diff_repeat, unknown_repeat),
        3,
    );
    rep.triple = triple;
    rep.diff_repeat = diff_repeat;
    rep.unknown_repeat = unknown_repeat;
    rep.detected = rep_detected(triple, diff_repeat, unknown_repeat);
    Ok(rep)
}

/// 三面检出计数。
fn rep_detected(a: bool, b: bool, c: bool) -> u32 {
    let mut n = 0;
    if a {
        n += 1;
    }
    if b {
        n += 1;
    }
    if c {
        n += 1;
    }
    n
}

/// 注入面硬门（未全检出报 InjectMissed）。
pub fn injection_face_require() -> Result<(), DupKey2Error> {
    let rep = injection_face()?;
    if rep.detected == 3 {
        Ok(())
    } else {
        Err(DupKey2Error::InjectMissed)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 工具：UTF-8 安全预算截断（报告/解释护顶；截断显性标注不丢账）
// ═══════════════════════════════════════════════════════════════════════

/// 预算截断（字节预算内最大 UTF-8 边界截断 + 显性截断标注；
/// 标注自身 ≈18 字节，超出预算部分即截断提示本身——账面数据另有
/// dup_query/explain_dup 逐键可查，不因截断丢失）。
fn truncate_owned(mut s: String, budget: usize) -> String {
    if s.len() <= budget {
        return s;
    }
    let mut end = budget;
    let b = s.as_bytes();
    while end > 0 {
        let is_cont = match b.get(end) {
            Some(c) => (c & 0xC0) == 0x80,
            None => false,
        };
        if !is_cont {
            break;
        }
        end -= 1;
    }
    s.truncate(end);
    s.push_str("…（预算截断）");
    s
}

// ═══════════════════════════════════════════════════════════════════════
// 冒烟与深度测试（探针 cargo test 面；断言即断言面本体）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // ── FNV 参考向量（公开参考值锁基；与编译期断言双保险）──────────
    #[test]
    fn fnv_reference_vectors() {
        assert_eq!(fnv64(FNV64_OFFSET, b""), 0xcbf29ce484222325);
        assert_eq!(fnv64(FNV64_OFFSET, b"a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv64(FNV64_OFFSET, b"foobar"), 0x85944171f73967e8);
        assert_eq!(fnv64_str("varix"), fnv64(FNV64_OFFSET, b"varix"));
    }

    // ── 键空间对齐：24 键闭集 + 三型 8/8/8 + F0586 16 键逐字对齐 ────
    #[test]
    fn key_table_alignment_and_kinds() {
        assert_eq!(KEY_TABLE.len(), 24);
        // F0586 INT_TABLE 8 键（逐字、同序）。
        let int_keys = [
            "loglevel",
            "uart.baud",
            "console.cols",
            "console.rows",
            "heap.mb",
            "stack.kb",
            "sched.slice.us",
            "dma.win.mb",
        ];
        // F0586 STR_TABLE 8 键（逐字、同序）。
        let str_keys = [
            "hostname",
            "console.dev",
            "boot.id",
            "log.tag",
            "video.mode",
            "locale",
            "kernel.opt",
            "cmd.prefix",
        ];
        let mut i = 8;
        while i < 16 {
            assert_eq!(KEY_TABLE[i].key, int_keys[i - 8]);
            assert!(matches!(KEY_TABLE[i].kind, KeyKind::Int));
            i += 1;
        }
        let mut j = 16;
        while j < 24 {
            assert_eq!(KEY_TABLE[j].key, str_keys[j - 16]);
            assert!(matches!(KEY_TABLE[j].kind, KeyKind::Str));
            j += 1;
        }
        let mut k = 0;
        while k < 8 {
            assert!(matches!(KEY_TABLE[k].kind, KeyKind::Bool));
            k += 1;
        }
        // 查位/查名闭环。
        assert_eq!(key_index("nokaslr"), Some(0));
        assert_eq!(key_index("sched.slice.us"), Some(14));
        assert_eq!(key_index("cmd.prefix"), Some(23));
        assert_eq!(key_index("no.such.key"), None);
        assert_eq!(key_name(23), Some("cmd.prefix"));
        assert_eq!(key_name(24), None);
        assert!(matches!(key_kind_of("heap.mb"), Some(KeyKind::Int)));
        assert!(matches!(key_kind_of("nmi.watch"), Some(KeyKind::Bool)));
    }

    // ── J1 核心：同键 3 现注入——首现生效 1/1 + 重复计数 2/2 ─────────
    #[test]
    fn triple_injection_first_win_1_of_1_and_2_of_2() -> Result<(), DupKey2Error> {
        let mut s = ParamStream::new();
        let a = s.push("sched.slice.us", "4000")?;
        let b = s.push("sched.slice.us", "5000")?;
        let c = s.push("sched.slice.us", "6000")?;
        // 逐笔观测：首现 → 后现 2 → 后现 3。
        assert!(matches!(a.class, AcceptClass::First));
        assert_eq!(a.seen, 1);
        assert_eq!(a.dup_after, 0);
        assert!(matches!(b.class, AcceptClass::Dup));
        assert_eq!(b.seen, 2);
        assert_eq!(b.dup_after, 1);
        assert!(matches!(c.class, AcceptClass::Dup));
        assert_eq!(c.seen, 3);
        assert_eq!(c.dup_after, 2);
        s.seal()?;
        let snap = s.snapshot()?;
        // 快照计数面：生效 1/1 + 重复 2/2 + 总现 3。
        let cf = snap.count_face("sched.slice.us")?;
        assert!(cf.present);
        assert_eq!(cf.effective_hits, 1);
        assert_eq!(cf.dup_count, 2);
        assert_eq!(cf.seen, 3);
        // 生效值=首现值（first-win 语义）。
        assert_eq!(snap.effective_value("sched.slice.us")?.as_str(), "4000");
        // 断言面随证包。
        let proof = s.assert_triple("sched.slice.us")?;
        assert!(proof.verify());
        assert_eq!(proof.effective_hits, 1);
        assert_eq!(proof.effective_expected, 1);
        assert_eq!(proof.dup_total, 2);
        assert_eq!(proof.dup_expected, 2);
        assert_eq!(proof.first_val.as_str(), "4000");
        // 后现随证值（owned 比较，避免临时借用悬挂）。
        let later_pair = match (proof.later_vals[0], proof.later_vals[1]) {
            (Some(x), Some(y)) => (x.as_str().to_string(), y.as_str().to_string()),
            _ => (String::from("?"), String::from("?")),
        };
        assert_eq!(later_pair, (String::from("5000"), String::from("6000")));
        // 重复簿可查：值清单 + 每次出现位置 + 第几现。
        let v = s.ledger.dup_query("sched.slice.us")?;
        assert_eq!(v.dup_total, 2);
        assert_eq!(v.occs.len(), 2);
        assert_eq!(v.occs[0].nth, 2);
        assert_eq!(v.occs[1].nth, 3);
        let pos = s.ledger.positions_of("sched.slice.us")?;
        assert_eq!(pos.len(), 3);
        assert_eq!(pos[0], a.ord);
        assert_eq!(pos[1], b.ord);
        assert_eq!(pos[2], c.ord);
        // 恒等式 + 健康分。
        s.ledger.require_identity()?;
        assert!(s.ledger.health() > 0);
        Ok(())
    }

    // ── 首现快照指纹：双跑稳定；首现口径敏感性分账（快照对后现值
    //    不敏感——后现不入生效表；账面指纹对后现值敏感——重复簿记值）──
    #[test]
    fn snapshot_fingerprint_stable_and_sensitive() -> Result<(), DupKey2Error> {
        let mk = |v3: &str| -> Result<(u64, u64), DupKey2Error> {
            let mut s = ParamStream::new();
            s.push("heap.mb", "512")?;
            s.push("heap.mb", "1024")?;
            s.push("heap.mb", v3)?;
            s.seal()?;
            let snap = s.snapshot()?;
            snap.require_fp()?;
            Ok((snap.fingerprint, s.ledger.state_fingerprint()))
        };
        let (sf1, lf1) = mk("2048")?;
        let (sf2, lf2) = mk("2048")?;
        let (sf3, lf3) = mk("4096")?;
        // 双跑稳定：同批次两域指纹全等。
        assert_eq!(sf1, sf2, "同批次双跑快照指纹须全等");
        assert_eq!(lf1, lf2, "同批次双跑账面指纹须全等");
        // 首现口径语义面：第三现（后现）值变——生效表不变（首现值
        // 仍是 512），故快照指纹不变；重复簿记后现值，账面指纹必变。
        assert_eq!(sf1, sf3, "快照=生效表封账副本，对后现值不敏感（首现口径）");
        assert_ne!(lf1, lf3, "账面指纹含重复簿后现值，须敏感");
        // 首现值变则快照指纹必变（生效表本体变化）。
        let mut s = ParamStream::new();
        s.push("heap.mb", "512")?;
        s.seal()?;
        let snap = s.snapshot()?;
        let mut s2 = ParamStream::new();
        s2.push("heap.mb", "1024")?;
        s2.seal()?;
        let snap2 = s2.snapshot()?;
        assert_ne!(snap.fingerprint, snap2.fingerprint);
        // 账面指纹与快照指纹域分离（不同域前缀不碰撞到同值）。
        assert_ne!(snap.fingerprint, s.ledger.state_fingerprint());
        Ok(())
    }

    // ── 人话报告：判据句式逐片在报告与单键解释中 ─────────────────────
    #[test]
    fn human_report_exact_sentence() -> Result<(), DupKey2Error> {
        let mut s = ParamStream::new();
        s.push("loglevel", "6")?;
        s.push("loglevel", "3")?;
        s.push("loglevel", "7")?;
        s.seal()?;
        // 单键解释：判据句式。
        let line = s.ledger.explain_dup("loglevel")?;
        assert!(line.contains("键 loglevel 出现 3 次"));
        assert!(line.contains("生效值=首次值 6"));
        assert!(line.contains("后现值 3/7 被忽略"));
        // 总报告含同句式 + 分列 + 账面节。
        let report = s.ledger.render_report();
        assert!(report.contains("首现生效表"));
        assert!(report.contains("重复键人话"));
        assert!(report.contains("键 loglevel 出现 3 次"));
        assert!(report.contains("生效值=首次值 6"));
        assert!(report.contains("后现值 3/7 被忽略"));
        assert!(report.contains("同值/异值分列"));
        assert!(report.contains("异值列：2 笔"));
        assert!(report.contains("账面指纹"));
        // 断言面人话行。
        let proof = s.assert_triple("loglevel")?;
        assert!(proof.line().contains("首现生效 1/1，重复计数 2/2"));
        Ok(())
    }

    // ── 同值/异值/布尔词形异 三分列 ────────────────────────────────────
    #[test]
    fn same_diff_and_bool_form_classification() -> Result<(), DupKey2Error> {
        let mut s = ParamStream::new();
        // 字节级同值重复。
        s.push("console.dev", "ttyS0")?;
        s.push("console.dev", "ttyS0")?;
        // 异值重复。
        s.push("boot.id", "0d23d90010")?;
        s.push("boot.id", "ffffffff00")?;
        // 布尔词形异（on vs true 同规范位）。
        s.push("nmi.watch", "on")?;
        s.push("nmi.watch", "true")?;
        s.seal()?;
        let v1 = s.ledger.dup_query("console.dev")?;
        assert_eq!(v1.same_col, 1);
        assert_eq!(v1.form_col, 0);
        assert_eq!(v1.diff_col, 0);
        let v2 = s.ledger.dup_query("boot.id")?;
        assert_eq!(v2.diff_col, 1);
        assert_eq!(v2.same_col, 0);
        let v3 = s.ledger.dup_query("nmi.watch")?;
        assert_eq!(v3.form_col, 1);
        assert_eq!(v3.same_col, 0);
        assert_eq!(v3.diff_col, 0);
        // 两列视图：同值列 = same + form。
        let st = s.ledger.dup_stats();
        assert_eq!(st.same_col + st.form_col, 2);
        assert_eq!(st.diff_col, 1);
        assert_eq!(st.keys_with_dups, 3);
        assert_eq!(st.bool_dups, 1);
        assert_eq!(st.int_dups, 0);
        assert_eq!(st.str_dups, 2);
        // 生效值仍为首现值。
        assert_eq!(s.ledger.effective_value("nmi.watch")?.as_str(), "on");
        Ok(())
    }

    // ── 白名单联动：未知键重复双重违规分账 ─────────────────────────────
    #[test]
    fn unknown_repeat_double_violation_whitelist_linkage() -> Result<(), DupKey2Error> {
        let mut s = ParamStream::new();
        // 首现：白名单外（单违规）。
        let e1 = s.push("ghost.key", "a");
        // 复现：白名单外 + 重复（双重违规）。
        let e2 = s.push("ghost.key", "b");
        let e3 = s.push("ghost.key", "c");
        assert!(matches!(&e1, Err(e) if e.code() == DupKey2Error::UnknownKey.code()));
        assert!(matches!(&e2, Err(e) if e.code() == DupKey2Error::UnknownKey.code()));
        assert!(matches!(&e3, Err(e) if e.code() == DupKey2Error::UnknownKey.code()));
        // 未知账：3 笔、nth 1..3、双重违规 2 笔、涉 1 键。
        let occs = s.ledger.unknown_occs();
        assert_eq!(occs.len(), 3);
        assert_eq!(occs[0].nth, 1);
        assert_eq!(occs[1].nth, 2);
        assert_eq!(occs[2].nth, 3);
        assert_eq!(s.ledger.unknown.double_violation_n(), 2);
        assert_eq!(s.ledger.unknown.double_keys().len(), 1);
        assert_eq!(s.ledger.unknown_n(), 3);
        assert_eq!(s.unknown_attempts(), 3);
        // 白名单联动审计过（未知账键全在闭集外；簿内键全在闭集内）。
        let audit = s.ledger.whitelist_audit();
        assert!(audit.pass);
        assert_eq!(audit.double_violation_n, 2);
        s.ledger.require_whitelist()?;
        // 人话报告含双重违规行。
        let report = s.ledger.render_report();
        assert!(report.contains("未知键双重违规（2 笔，涉 1 键）"));
        assert!(report.contains("ghost.key 第 2 现"));
        // 已知键首现不受未知键污染。
        assert!(s.push("heap.mb", "512").is_ok());
        assert_eq!(s.ledger.total_accepted(), 1);
        Ok(())
    }

    // ── 双跑确定性对拍（混批：合法/重复/未知/闸拒）───────────────────
    #[test]
    fn dual_run_deterministic_mixed_batch() -> Result<(), DupKey2Error> {
        let batch: &[(&str, &str)] = &[
            ("heap.mb", "512"),
            ("loglevel", "6"),
            ("heap.mb", "512"), // 同值重复
            ("heap.mb", "1024"), // 异值重复
            ("nmi.watch", "on"),
            ("nmi.watch", "true"), // 词形异同值
            ("no.such.key", "x"), // 未知键
            ("no.such.key", "y"), // 未知键复现（双重违规）
            ("", "empty-key"),   // 空键
            ("heap.mb", "3072"),
        ];
        let rep = dual_run(batch)?;
        assert!(rep.outcomes_equal);
        assert!(rep.state_fp_equal);
        assert!(rep.snap_fp_equal);
        assert_eq!(rep.first_divergence, None);
        assert!(rep.pass);
        dual_run_require(batch)?;
        // 空批：两跑皆空封流——指纹全等且通过。
        let rep0 = dual_run(&[])?;
        assert!(rep0.pass);
        Ok(())
    }

    // ── 六闸：空键/键超长/值超长/环满/封流后写 ─────────────────────────
    #[test]
    fn gates_reject_paths() -> Result<(), DupKey2Error> {
        let mut s = ParamStream::new();
        // 闸② 空键。
        assert!(matches!(
            s.push("", "v"),
            Err(e) if e.code() == DupKey2Error::EmptyKey.code()
        ));
        // 闸③ 键超长（>24 字节）。
        let long_key = "k.k.k.k.k.k.k.k.k.k.k.k.k.k.k.";
        assert!(long_key.len() > KEY_KEEP);
        assert!(matches!(
            s.push(long_key, "v"),
            Err(e) if e.code() == DupKey2Error::KeyTooLong.code()
        ));
        // 闸④ 值超长（>64 字节；构造 65 字节避免手数字数误差）。
        let long_val = "v".repeat(VAL_MAX + 1);
        assert!(long_val.len() > VAL_MAX);
        assert!(matches!(
            s.push("heap.mb", long_val.as_str()),
            Err(e) if e.code() == DupKey2Error::ValueTooLong.code()
        ));
        // 闸⑥ 环满：64 槽全占后第 65 笔拒。
        let mut i = 0;
        while i < RING_CAP {
            let v = format!("{}", 100 + i);
            assert!(s.push("loglevel", &v).is_ok());
            i += 1;
        }
        assert_eq!(s.len(), RING_CAP);
        assert!(matches!(
            s.push("loglevel", "7"),
            Err(e) if e.code() == DupKey2Error::RingFull.code()
        ));
        // 闸① 封流后写 + 重复封流。
        s.seal()?;
        assert!(matches!(
            s.push("heap.mb", "512"),
            Err(e) if e.code() == DupKey2Error::StreamSealed.code()
        ));
        assert!(matches!(
            s.seal(),
            Err(e) if e.code() == DupKey2Error::StreamSealed.code()
        ));
        // 流账恒等式：尝试 = 受理 + 闸拒 + 未知。
        assert!(s.stream_identity());
        assert_eq!(s.push_attempts(), 3 + RING_CAP as u64 + 1 + 1);
        Ok(())
    }

    // ── 快照闸：未封流先取快照/断言面拒收 ─────────────────────────────
    #[test]
    fn snapshot_gate_before_seal() {
        let mut s = ParamStream::new();
        assert!(s.push("heap.mb", "512").is_ok());
        assert!(matches!(
            s.snapshot(),
            Err(e) if e.code() == DupKey2Error::SnapshotAbsent.code()
        ));
        assert!(matches!(
            s.assert_triple("heap.mb"),
            Err(e) if e.code() == DupKey2Error::SnapshotAbsent.code()
        ));
    }

    // ── 查询面：未命中码 ───────────────────────────────────────────────
    #[test]
    fn query_miss_paths() {
        let mut s = ParamStream::new();
        assert!(s.push("heap.mb", "512").is_ok());
        // 闭集外键。
        assert!(matches!(
            s.ledger.dup_query("ghost.key"),
            Err(e) if e.code() == DupKey2Error::UnknownKey.code()
        ));
        assert!(matches!(
            s.ledger.effective_value("ghost.key"),
            Err(e) if e.code() == DupKey2Error::UnknownKey.code()
        ));
        // 闭集内但未生效/无重复。
        assert!(matches!(
            s.ledger.dup_query("loglevel"),
            Err(e) if e.code() == DupKey2Error::QueryMiss.code()
        ));
        assert!(matches!(
            s.ledger.effective_value("loglevel"),
            Err(e) if e.code() == DupKey2Error::QueryMiss.code()
        ));
        assert!(matches!(
            s.ledger.positions_of("loglevel"),
            Err(e) if e.code() == DupKey2Error::QueryMiss.code()
        ));
        // 生效后无重复仍 QueryMiss（重复簿可查面口径：无重复即无簿）。
        assert!(s.push("loglevel", "6").is_ok());
        assert!(matches!(
            s.ledger.dup_query("loglevel"),
            Err(e) if e.code() == DupKey2Error::QueryMiss.code()
        ));
    }

    // ── 重复簿容量：溢出截笔不丢账（I4 恒等式护住）────────────────────
    #[test]
    fn book_overflow_truncation_keeps_identity() -> Result<(), DupKey2Error> {
        let mut s = ParamStream::new();
        // 1 首现 + 50 后现 = 51 笔受理（≤ RING_CAP=64）；簿容 48 → 截 2 笔。
        assert!(s.push("loglevel", "0").is_ok());
        let mut i = 0;
        while i < 50 {
            let v = format!("{}", 1 + i);
            let out = s.push("loglevel", &v)?;
            // 截笔笔 book_kept=false（计数不丢）。
            if i >= DUP_BOOK_CAP {
                assert!(!out.book_kept);
            }
            i += 1;
        }
        assert_eq!(s.ledger.total_dups(), 50);
        assert_eq!(s.ledger.dup_query("loglevel")?.dup_total, 50);
        // 簿内存笔 = 48（截笔后视图只余存笔）。
        assert_eq!(s.ledger.dup_query("loglevel")?.occs.len(), DUP_BOOK_CAP);
        // 恒等式仍闭合（I4：重复总数 = 簿存 48 + 簿截 2）。
        let id = s.ledger.audit_now();
        assert!(id.i4_book_close);
        assert!(id.pass, "截笔后恒等式须仍闭合");
        assert_eq!(
            s.ledger.events().count_of(EventKind::IdentityEv),
            1,
            "audit_now 须入事件账"
        );
        assert_eq!(s.ledger.events().count_of(EventKind::BookOverflow), 2);
        s.ledger.require_identity()?;
        // 首现仍生效（first-win 不因簿截破）。
        assert_eq!(s.ledger.effective_value("loglevel")?.as_str(), "0");
        // 健康分因截笔扣减但恒等式过 → >0。
        assert!(s.ledger.health() > 0);
        Ok(())
    }

    // ── 事件账：滑窗丢旧 + 分类计数 + 渲染 ─────────────────────────────
    #[test]
    fn event_ring_cap_and_counts() -> Result<(), DupKey2Error> {
        let mut s = ParamStream::new();
        // 20 笔同键（1 首现 + 19 后现·异值）+ 1 笔另键首现：
        // 事件流 = 1 首现 + 19×（异值事件+重复入簿事件）+ 1 首现 = 40 条，
        // 滑窗 32 → 丢最旧 8 条（1 首现 + 后现 2/3/4 的异值 + 后现 2/3/4
        // 的入簿，即 首现1 + 异值4 + 入簿3）→ 窗内存：
        // 首现 1（另键）、入簿 16、异值 15。
        let mut i = 0;
        while i < 20 {
            let v = format!("{}", i);
            assert!(s.push("stack.kb", &v).is_ok());
            i += 1;
        }
        assert!(s.push("heap.mb", "512").is_ok());
        let ev = s.ledger.events();
        assert_eq!(ev.len(), EVENT_CAP);
        assert_eq!(ev.dropped, 8);
        assert_eq!(ev.count_of(EventKind::StreamFirst), 1);
        assert_eq!(ev.count_of(EventKind::StreamDup), 16);
        assert_eq!(ev.count_of(EventKind::DiffRepeat), 15);
        let lines = ev.render_lines();
        assert_eq!(lines.len(), EVENT_CAP);
        assert!(lines.iter().all(|l| !l.is_empty()));
        assert!(lines.iter().all(|l| l.contains('[')));
        Ok(())
    }

    // ── 恒等式 + 健康分：正常账健康高、脏账扣减、未知键污染扣减 ──────
    #[test]
    fn identity_and_health_faces() -> Result<(), DupKey2Error> {
        // 干净账：仅首现，无重复——健康满刻度。
        let mut clean = ParamStream::new();
        clean.push("heap.mb", "512")?;
        clean.push("loglevel", "6")?;
        clean.seal()?;
        assert_eq!(clean.ledger.health(), HEALTH_FULL);
        clean.ledger.require_identity()?;
        // 同值重复 1 笔：-2。
        let mut same = ParamStream::new();
        same.push("heap.mb", "512")?;
        same.push("heap.mb", "512")?;
        assert_eq!(same.ledger.health(), HEALTH_FULL - 2);
        // 异值重复 1 笔：-8。
        let mut diff = ParamStream::new();
        diff.push("heap.mb", "512")?;
        diff.push("heap.mb", "1024")?;
        assert_eq!(diff.ledger.health(), HEALTH_FULL - 8);
        // 词形异 1 笔：-1。
        let mut form = ParamStream::new();
        form.push("nmi.watch", "on")?;
        form.push("nmi.watch", "yes")?;
        assert_eq!(form.ledger.health(), HEALTH_FULL - 1);
        // 双重违规 1 笔：-15（push 未知键报 UnknownKey 是预期拒收，
        // 断言面口径不断流）。
        let mut unk = ParamStream::new();
        assert!(matches!(
            unk.push("ghost.key", "a"),
            Err(e) if e.code() == DupKey2Error::UnknownKey.code()
        ));
        assert!(matches!(
            unk.push("ghost.key", "b"),
            Err(e) if e.code() == DupKey2Error::UnknownKey.code()
        ));
        assert_eq!(unk.ledger.health(), HEALTH_FULL - 15);
        // 恒等式报告逐条中文行可渲染。
        let lines = unk.ledger.identity_check().lines();
        assert_eq!(lines.len(), 6);
        assert!(lines[0].contains("I1"));
        Ok(())
    }

    // ── 注入面：三注入各检出 = 3/3 ─────────────────────────────────────
    #[test]
    fn injection_face_all_3_of_3() -> Result<(), DupKey2Error> {
        let rep = injection_face()?;
        assert!(rep.triple, "面1 同键三现自证");
        assert!(rep.diff_repeat, "面2 异值重复注入");
        assert!(rep.unknown_repeat, "面3 未知键重复注入");
        assert_eq!(rep.detected, 3);
        injection_face_require()?;
        Ok(())
    }

    // ── 参数流摘要 + 环形槽视图 ────────────────────────────────────────
    #[test]
    fn stream_summary_and_slots() -> Result<(), DupKey2Error> {
        let mut s = ParamStream::new();
        s.push("heap.mb", "512")?;
        s.push("heap.mb", "1024")?;
        s.push("uart.baud", "115200")?;
        let slots = s.ring_slots();
        assert_eq!(slots.len(), 3);
        // 重复现也占槽（流记到达不记生效）。
        assert_eq!(slots[0].key.as_str(), "heap.mb");
        assert_eq!(slots[0].val.as_str(), "512");
        assert!(matches!(slots[0].accept, AcceptClass::First));
        assert!(matches!(slots[1].accept, AcceptClass::Dup));
        assert!(matches!(slots[1].dup_class, Some(DiffClass::DiffValue)));
        assert_eq!(slots[2].val.as_str(), "115200");
        let sum = s.render_summary();
        assert!(sum.contains("参数流（3/64 槽"));
        assert!(sum.contains("heap.mb=1024（后现·异值重复）"));
        assert!(sum.contains("uart.baud=115200（首现）"));
        Ok(())
    }

    // ── 位置与序号单调；ValCell 边角 ───────────────────────────────────
    #[test]
    fn ordinals_monotone_and_valcell_edges() -> Result<(), DupKey2Error> {
        let mut s = ParamStream::new();
        let a = s.push("heap.mb", "512")?;
        let b = s.push("loglevel", "6")?;
        let c = s.push("heap.mb", "1024")?;
        let d = s.push("loglevel", "6")?;
        assert!(a.ord < b.ord && b.ord < c.ord && c.ord < d.ord);
        // 位置清单按出现序。
        let hp = s.ledger.positions_of("heap.mb")?;
        assert_eq!(hp.len(), 2);
        assert_eq!(hp[0], a.ord);
        assert_eq!(hp[1], c.ord);
        // ValCell：长度/相等/指纹折叠。
        let x = ValCell::from_val("abc")?;
        let y = ValCell::from_val("abc")?;
        let z = ValCell::from_val("abd")?;
        assert!(x.same_bytes(&y));
        assert!(!x.same_bytes(&z));
        assert_eq!(x.len(), 3);
        assert!(!x.is_empty());
        assert_eq!(ValCell::empty().len(), 0);
        assert!(ValCell::empty().is_empty());
        assert_ne!(x.fold_fnv(1), z.fold_fnv(1));
        assert!(matches!(
            ValCell::from_val("x".repeat(VAL_MAX + 1).as_str()),
            Err(e) if e.code() == DupKey2Error::ValueTooLong.code()
        ));
        // 布尔词形闭集。
        assert!(matches!(canonical_bool("on"), Some(BoolWord::True)));
        assert!(matches!(canonical_bool("no"), Some(BoolWord::False)));
        assert!(canonical_bool("maybe").is_none());
        Ok(())
    }

    // ── 诊断码闭集：段位独占 + 逐变体 detail 非空 ──────────────────────
    #[test]
    fn error_segment_closed_set() {
        let all = [
            DupKey2Error::UnknownKey,
            DupKey2Error::EmptyKey,
            DupKey2Error::KeyTooLong,
            DupKey2Error::ValueTooLong,
            DupKey2Error::RingFull,
            DupKey2Error::StreamSealed,
            DupKey2Error::SnapshotAbsent,
            DupKey2Error::QueryMiss,
            DupKey2Error::FirstWinBroken,
            DupKey2Error::CountBroken,
            DupKey2Error::WhitelistBroken,
            DupKey2Error::DualMismatch,
            DupKey2Error::InjectMissed,
            DupKey2Error::FpBroken,
            DupKey2Error::LedgerBroken,
            DupKey2Error::InternalBroken,
        ];
        let mut i = 0;
        while i < all.len() {
            let c = all[i].code();
            assert!(
                c >= DK2_SEG_BASE && c <= DK2_SEG_TOP,
                "码 {:#06x} 须落 0x1F50~0x1F5F 段",
                c
            );
            assert!(!all[i].detail().is_empty());
            assert!(all[i].detail().len() >= 16, "detail 须为可操作中文诊断");
            i += 1;
        }
        assert_eq!(all.len(), 16);
        assert_eq!(all[0].code(), DK2_SEG_BASE);
        assert_eq!(all[15].code(), DK2_SEG_TOP);
    }

    // ── 报告预算截断：护顶不丢账（明细另查）────────────────────────────
    #[test]
    fn report_budget_truncation() -> Result<(), DupKey2Error> {
        let mut s = ParamStream::new();
        // 8 键 × 6 现 = 48 笔受理（≤64 槽；40 后现 ≤ 簿容 48）；
        // 长值（键名内嵌）迫使逐键人话行超长 → 报告超 1024 字节预算。
        let keys = [
            "loglevel",
            "uart.baud",
            "console.cols",
            "console.rows",
            "heap.mb",
            "stack.kb",
            "sched.slice.us",
            "dma.win.mb",
        ];
        let mut i = 0;
        while i < keys.len() {
            if let Some(k) = keys.get(i) {
                let mut j = 0;
                while j < 6 {
                    let v = format!("{}·v{}", k, j);
                    assert!(s.push(k, &v).is_ok());
                    j += 1;
                }
            }
            i += 1;
        }
        let report = s.ledger.render_report();
        assert!(
            report.len() <= REPORT_BUDGET + 32,
            "报告须被预算截断护顶（实测 {} 字节）",
            report.len()
        );
        assert!(report.contains("预算截断"), "截断须显性标注");
        // 截断不丢账：dup_query 逐键仍全量可查（8 键 × 5 后现全在簿）。
        let mut n = 0;
        while n < keys.len() {
            if let Some(k) = keys.get(n) {
                assert_eq!(s.ledger.dup_query(k)?.occs.len(), 5);
            }
            n += 1;
        }
        Ok(())
    }
}
